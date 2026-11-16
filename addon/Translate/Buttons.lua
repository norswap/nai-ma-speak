local addonName, ns = ...

-- The Translate and Settings buttons on the chat window, and replies typed as
-- "/tr /g hello". The addon cannot reach the companion except through the
-- pixel strip, so a click or a reply is sent as a strip line:
--   "C\treply" or "C\tsettings"   a button was clicked
--   "R\t<English>"                translate this and copy it for pasting
-- The companion shows the progress; the addon never learns the result.

local PLACES = {
    -- point on the bar, point on the chat window, x, y
    tabs = { "BOTTOMRIGHT", "TOPRIGHT", 0, 3 },
    top = { "TOPRIGHT", "TOPRIGHT", -4, -4 },
    bottom = { "BOTTOMRIGHT", "BOTTOMRIGHT", -4, 4 },
}

local bar

function ns.SendCommand(name)
    local line = "C\t" .. name
    ns.SendToStrip(line, line)
end

local function MakeButton(text, width, command)
    local button = CreateFrame("Button", nil, bar, "UIPanelButtonTemplate")
    button:SetSize(width, 20)
    button:SetText(text)
    if GameFontNormalSmall then button:SetNormalFontObject(GameFontNormalSmall) end
    button:SetScript("OnClick", function() ns.SendCommand(command) end)
    return button
end

local function Build()
    bar = CreateFrame("Frame", "TranslateButtons", UIParent)
    bar:SetSize(138, 20)
    bar:SetFrameStrata("MEDIUM")
    local reply = MakeButton("Translate", 72, "reply")
    reply:SetPoint("LEFT", bar, "LEFT", 0, 0)
    local settings = MakeButton("Settings", 64, "settings")
    settings:SetPoint("LEFT", reply, "RIGHT", 2, 0)
end

-- Puts the buttons where the options say, on the main chat window, or hides them.
function ns.PlaceButtons()
    local place = PLACES[ns.db.buttons]
    local window = ChatFrame1 and (ChatFrame1.Background or ChatFrame1)
    if not place or not window or not ns.db.strip then
        if bar then bar:Hide() end
        return
    end
    if not bar then Build() end
    bar:ClearAllPoints()
    bar:SetPoint(place[1], window, place[2], place[3], place[4])
    bar:Show()
end

function ns.InitButtons()
    local ok, problem = pcall(ns.PlaceButtons)
    ns.buttonSource = ok and ("placed: " .. tostring(ns.db.buttons)) or ("failed: " .. tostring(problem))
end

-- Chat targets that are followed by a name.
local NAMED = { w = true, whisper = true, t = true, tell = true }

-- Splits "/g hello" into "g" and "hello", "/w Ann hello" into "w Ann" and
-- "hello", and leaves text without a leading slash whole. Returns nil and
-- what is missing when a target has nothing to say after it.
function ns.SplitReply(text)
    if text:sub(1, 1) ~= "/" then return nil, text end
    local target, rest = text:match("^/(%S+)%s+(.+)$")
    if not target then return nil, nil, "Say what? For example: /tr /g hello" end
    if NAMED[target:lower()] then
        local name, said = rest:match("^(%S+)%s+(.+)$")
        if not name then return nil, nil, "Whisper what? For example: /tr /w Name hello" end
        return target .. " " .. name, said
    end
    return target, rest
end

-- Sends English to the companion for translation, and opens the chat box on
-- the target (or on the channel it was last on), ready for the paste.
function ns.Reply(text)
    local target, said, problem = ns.SplitReply(text)
    if not said then
        ns.Print(problem)
        return
    end
    said = said:gsub("[%z\1-\31]", " ")
    local line = "R\t" .. said
    ns.SendToStrip(line, line)
    local open = (ChatFrameUtil and ChatFrameUtil.OpenChat) or ChatFrame_OpenChat
    if open and C_Timer and C_Timer.After then
        -- After this command's own chat box has closed.
        C_Timer.After(0, function() open(target and ("/" .. target .. " ") or "") end)
    end
end
