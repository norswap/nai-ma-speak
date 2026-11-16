local addonName, ns = ...

-- Tells the companion where the chat windows are, so its overlay can sit on
-- one of them and hide while that window's tab is not selected.
--
-- One strip line: "L", a tab, then one record per chat window. Records are
-- separated by byte 30 and their fields by byte 31:
--   name, shown (1 or 0), left, top, width, height, font size, background alpha
-- Lengths are physical screen pixels and positions are measured from the
-- top-left corner of the game's picture, where the strip is drawn.

local FIELD, RECORD = "\31", "\30"
local CHECK_TIME = 0.25 -- seconds between looks at the chat windows
local RESEND_TIME = 5   -- resend unchanged, for a companion started later

local function Round(value, step)
    return math.floor(value / step + 0.5) * step
end

-- Pure Lua, so it can be tested outside WoW.
function ns.LayoutRecord(name, shown, left, top, width, height, fontSize, alpha)
    name = name:gsub("%c", "")
    return table.concat({ name, shown and 1 or 0, left, top, width, height, fontSize, alpha }, FIELD)
end

local function Describe()
    local _, screenHeight = GetPhysicalScreenSize()
    local records = {}
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        local name, fontSize, _, _, _, alpha = FCF_GetChatWindowInfo(i)
        if frame and type(name) == "string" and name ~= "" and (frame:IsShown() or frame.isDocked) then
            -- The background reaches a little past the text area on every side.
            local area = frame.Background or frame
            local left, top = area:GetLeft(), area:GetTop()
            if left and top then
                -- At effective scale 1 the screen is 768 units tall.
                local pixels = area:GetEffectiveScale() * screenHeight / 768
                records[#records + 1] = ns.LayoutRecord(
                    name, frame:IsVisible(),
                    Round(left * pixels, 1), Round(screenHeight - top * pixels, 1),
                    Round(area:GetWidth() * pixels, 1), Round(area:GetHeight() * pixels, 1),
                    Round((fontSize or 14) * frame:GetEffectiveScale() * screenHeight / 768, 0.1),
                    Round(alpha or 0, 0.01))
            end
        end
    end
    return "L\t" .. table.concat(records, RECORD)
end

function ns.InitLayout()
    if not (C_Timer and C_Timer.NewTicker and FCF_GetChatWindowInfo) then
        ns.layoutSource = "none (no chat window API)"
        return
    end
    ns.layoutSource = "FCF_GetChatWindowInfo"
    local last, sentAt = nil, 0
    local ticker
    ticker = C_Timer.NewTicker(CHECK_TIME, function()
        if not ns.db.strip then return end
        local ok, text = pcall(Describe)
        if not ok then
            ns.layoutSource = "failed: " .. tostring(text)
            ns.Print("Could not read the chat windows, so the overlay cannot follow them: " .. tostring(text))
            ticker:Cancel()
            return
        end
        local now = GetTime()
        if text ~= last or now - sentAt >= RESEND_TIME then
            last, sentAt = text, now
            ns.SendToStrip(text, true)
        end
    end)
end
