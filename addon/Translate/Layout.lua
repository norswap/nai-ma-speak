local _, ns = ...

-- Tells the companion where the chat windows are, so its overlay can sit on
-- one of them and hide while that window's tab is not selected.
--
-- One strip line: "L", a tab, then one record per chat window. Records are
-- separated by byte 30 and their fields by byte 31:
--   name, shown (1 or 0), left, top, width, height, font size
-- Lengths are physical screen pixels and positions are measured from the
-- strip's top-left corner, the one place of the game the companion can see.

local FIELD, RECORD = "\31", "\30"
local CHECK_TIME = 0.25 -- seconds between looks at the chat windows
local RESEND_TIME = 5   -- resend unchanged, for a companion started later

local function Round(value, step)
    return math.floor(value / step + 0.5) * step
end

-- Pure Lua, so it can be tested outside WoW.
function ns.LayoutRecord(name, shown, left, top, width, height, fontSize)
    name = name:gsub("[%z\1-\31]", "")
    return table.concat({ name, shown and 1 or 0, left, top, width, height, fontSize }, FIELD)
end

local function Describe()
    local _, screenHeight = GetPhysicalScreenSize()
    local stripLeft, stripTop = ns.StripOrigin()
    local records = {}
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        local name, fontSize = FCF_GetChatWindowInfo(i)
        if frame and type(name) == "string" and name ~= "" and (frame:IsShown() or frame.isDocked) then
            -- The background reaches a little past the text area on every side.
            local area = frame.Background or frame
            local left, top = area:GetLeft(), area:GetTop()
            if left and top then
                -- At effective scale 1 the screen is 768 units tall.
                local pixels = area:GetEffectiveScale() * screenHeight / 768
                records[#records + 1] = ns.LayoutRecord(
                    name, frame:IsVisible(),
                    Round(left * pixels, 1) - stripLeft, Round(screenHeight - top * pixels, 1) - stripTop,
                    Round(area:GetWidth() * pixels, 1), Round(area:GetHeight() * pixels, 1),
                    Round((fontSize or 14) * frame:GetEffectiveScale() * screenHeight / 768, 0.1))
            end
        end
    end
    return "L\t" .. table.concat(records, RECORD)
end

-- Mouse wheel steps over each chat window, counted since login: up adds,
-- down subtracts. The overlay cannot receive the wheel itself (clicks and
-- scrolling pass through it to the game), so with the overlay on a chat tab
-- the game window underneath gets the wheel and the count is sent as
-- "S\t<name>\t<count>" for the overlay to scroll by the difference. A count,
-- not single steps, so a newer one can replace one still waiting. Every count
-- is also sent now and then unchanged, so a companion started later knows where
-- counting stands before the first step. Counts start far apart from one login
-- to the next, so that a new start cannot be taken for scrolling.
local wheel = {}
local START = time() % 100000 * 1000

local function SayWheel(index, delta)
    local name = FCF_GetChatWindowInfo(index)
    if type(name) ~= "string" or name == "" then return end
    name = name:gsub("[%z\1-\31]", "")
    wheel[index] = (wheel[index] or START) + delta
    local key = "S\t" .. name .. "\t"
    ns.SendToStrip(key .. wheel[index], key)
end

function ns.WheelMoved(index, delta)
    if type(delta) == "number" then SayWheel(index, delta) end
end

local function WatchWheel()
    local hooked = 0
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        if frame and frame.HookScript then
            frame:HookScript("OnMouseWheel", function(_, delta) ns.WheelMoved(i, delta) end)
            hooked = hooked + 1
        end
    end
    local on = not GetCVarBool or GetCVarBool("chatMouseScroll")
    ns.wheelSource = hooked .. " chat windows hooked" .. (on and "" or ", but mouse wheel scrolling is off in the game's chat options")
end

function ns.InitLayout()
    if not (C_Timer and C_Timer.NewTicker and FCF_GetChatWindowInfo) then
        ns.layoutSource = "none (no chat window API)"
        return
    end
    ns.layoutSource = "FCF_GetChatWindowInfo"
    local ok, problem = pcall(WatchWheel)
    if not ok then ns.wheelSource = "failed: " .. tostring(problem) end
    local last, sentAt = nil, 0
    local ticker
    ticker = C_Timer.NewTicker(CHECK_TIME, function()
        local ok, text = pcall(Describe)
        if not ok then
            ns.layoutSource = "failed: " .. tostring(text)
            ns.Print("Could not read the chat windows, so the overlay cannot follow them: " .. tostring(text))
            ticker:Cancel()
            return
        end
        local now = GetTime()
        if text ~= last or now - sentAt >= RESEND_TIME then
            if now - sentAt >= RESEND_TIME then
                for i = 1, NUM_CHAT_WINDOWS or 10 do SayWheel(i, 0) end
            end
            last, sentAt = text, now
            ns.SendToStrip(text, "L\t")
        end
    end)
end
