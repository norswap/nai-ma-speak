local _, ns = ...

-- The Translate and App buttons on the chat window, and replies typed as
-- "/tr /g hello". The addon cannot reach the companion except through the
-- pixel strip, so a click or a reply is sent as a strip line:
--   "C\treply" or "C\tsettings"   a button was clicked
--   "R\t<English>"                translate this and copy it for pasting
-- The companion shows the progress; the addon never learns the result.

local PLACES = {
    -- point on the bar, point on the chat window, x, y, and whether the buttons stand
    -- upright, one above the other, to take little room beside the window
    tabs = { "BOTTOMRIGHT", "TOPRIGHT", 0, 3 },
    top = { "TOPLEFT", "TOPRIGHT", 3, 0, true },
    bottom = { "BOTTOMLEFT", "BOTTOMRIGHT", 3, 0, true },
}

local THICK, GAP = 20, 2

local bar
local buttons = {}

function ns.SendCommand(name)
    local line = "C\t" .. name
    ns.SendToStrip(line, line)
end

-- The game's red button, as one picture each for at rest, pressed and under the mouse.
-- The game's button template is made of three pictures in a row, which cannot stand
-- upright; one picture can be turned by its corners.
local ART = "Interface\\Buttons\\UI-Panel-Button-"
-- The part of each picture that holds the button: left, right, top, bottom.
local L, R, T, B = 0, 0.625, 0, 0.6875
-- The game turns text about the middle of its top edge, not about its middle, which
-- moves it right and up by half the small font's height.
local TURNED_SHIFT = 5

local function MakeButton(text, length, command)
    local button = CreateFrame("Button", nil, bar)
    button:SetNormalTexture(ART .. "Up")
    button:SetPushedTexture(ART .. "Down")
    button:SetHighlightTexture(ART .. "Highlight", "ADD")
    button.art = { button:GetNormalTexture(), button:GetPushedTexture(), button:GetHighlightTexture() }
    button.label = button:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
    button.label:SetText(text)
    button.length = length
    button:SetScript("OnClick", function() ns.SendCommand(command) end)
    buttons[#buttons + 1] = button
end

local function Build()
    bar = CreateFrame("Frame", "TranslateButtons", UIParent)
    bar:SetFrameStrata("MEDIUM")
    MakeButton("Translate", 72, "reply")
    -- "App", not "Settings": the addon has settings of its own, in the game's options.
    MakeButton("App", 44, "settings")
end

-- Lays the buttons out side by side, or one above the other with their text turned a
-- quarter to the left. A game that cannot turn text keeps them side by side.
local function Arrange(upright)
    upright = upright and buttons[1].label.SetRotation ~= nil
    local previous, total = nil, -GAP
    for _, button in ipairs(buttons) do
        button:ClearAllPoints()
        if upright then
            button:SetSize(THICK, button.length)
            button:SetPoint("TOP", previous or bar, previous and "BOTTOM" or "TOP", 0, previous and -GAP or 0)
        else
            button:SetSize(button.length, THICK)
            button:SetPoint("LEFT", previous or bar, previous and "RIGHT" or "LEFT", previous and GAP or 0, 0)
        end
        if button.label.SetRotation then button.label:SetRotation(upright and math.pi / 2 or 0) end
        button.label:ClearAllPoints()
        button.label:SetPoint("CENTER", upright and -TURNED_SHIFT or 0, upright and -TURNED_SHIFT or 0)
        for _, picture in ipairs(button.art) do
            if upright then
                -- Each corner of the upright button shows the corner a quarter turn on.
                picture:SetTexCoord(R, T, L, T, R, B, L, B)
            else
                picture:SetTexCoord(L, R, T, B)
            end
        end
        previous, total = button, total + button.length + GAP
    end
    bar:SetSize(upright and THICK or total, upright and total or THICK)
end

-- Puts the buttons where the options say, on the main chat window, or hides them.
function ns.PlaceButtons()
    local place = PLACES[ns.db.buttons]
    local window = ChatFrame1 and (ChatFrame1.Background or ChatFrame1)
    if not place or not window then
        if bar then bar:Hide() end
        return
    end
    if not bar then Build() end
    Arrange(place[5])
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
