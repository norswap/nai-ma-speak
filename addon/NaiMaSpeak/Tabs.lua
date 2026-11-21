local _, ns = ...

-- Keeps one chat tab empty, for the companion's overlay to sit on, and makes such a
-- tab. The game writes some lines (the answer to /played, for one) into whichever tab
-- is in front, whatever that tab is set to show, so the tab is emptied whenever a line
-- is added to it. What the tab is set to show still counts for "Lines sent to the
-- overlay": one tab can pick the lines and stay empty under their translations.

local KEEP = "NAIMASPEAK_KEEP_EMPTY"
local NAME = "NAIMASPEAK_NAME_TAB"
local SETTLE_TIME = 1.5 -- seconds a choice in the options has to stand before it is taken up
local kept = "" -- the name of the tab kept empty: the options' choice, once taken up
local choices = 0 -- counts the choices made in the options, so only the last is taken up

-- The chat window kept empty, if there is one and it still exists.
local function Kept()
    return kept ~= "" and ns.ChatWindowNamed(kept) or nil
end

local function Keep(name)
    kept = type(name) == "string" and name or ""
    local window = Kept()
    if window then window:Clear() end
end

-- Takes up the tab chosen in the options, once the choice has stood for a moment: the
-- options step through the tabs one at a time, and every tab passed on the way is a
-- choice made. A tab set to show chat is emptied only once the player agrees to lose
-- what it shows; `back(name)` puts the options back on the tab kept before otherwise.
function ns.EmptyChosen(back)
    choices = choices + 1
    local choice = choices
    local function Settle()
        local name = ns.db.emptyWindow
        if choice ~= choices or name == kept then return end
        local window = type(name) == "string" and name ~= "" and ns.ChatWindowNamed(name)
        if window and #(window.messageTypeList or {}) + #(window.channelList or {}) > 0 then
            StaticPopup_Show(KEEP, name, nil, { name = name, back = back })
        else
            Keep(name)
        end
    end
    if C_Timer and C_Timer.After then C_Timer.After(SETTLE_TIME, Settle) else Settle() end
end

-- The name offered for the tab the options' button makes.
ns.NEW_TAB = "Translated"

-- Whether the game has a chat window left to open: it allows only so many.
function ns.CanAddTab()
    return #ns.ChatWindowNames() < (NUM_CHAT_WINDOWS or 10)
end

local function Full()
    return "All " .. (NUM_CHAT_WINDOWS or 10) .. " chat windows the game allows are in use. "
        .. "Close a tab (right-click it, Close Window) to free one."
end

-- Opens a chat tab of that name, set to show nothing. Returns the window and its name
-- as kept, or nil and why not.
function ns.NewEmptyTab(name)
    name = strtrim(type(name) == "string" and name or "")
    if name == "" then return nil, "The tab needs a name." end
    if ns.ChatWindowNamed(name) then
        return nil, "There is already a tab named \"" .. name .. "\"."
    end
    local window = ns.CanAddTab() and FCF_OpenNewWindow(name)
    if not window then return nil, Full() end
    window:RemoveAllMessageGroups()
    window:RemoveAllChannels()
    return window, name
end

-- Asks what to name a new empty tab and makes it; `made(name)` is told once it is there.
function ns.AskNewEmptyTab(made)
    if ns.CanAddTab() then StaticPopup_Show(NAME, nil, nil, made) else ns.Print(Full()) end
end

-- A popup's text field goes by one of these names, depending on the client.
local function NameBox(dialog)
    return dialog.editBox or dialog.EditBox or (dialog.GetEditBox and dialog:GetEditBox())
end

local function MakeNamed(dialog, made)
    local window, name = ns.NewEmptyTab(NameBox(dialog):GetText())
    if window then made(name) else ns.Print(name) end
end

function ns.InitTabs()
    StaticPopupDialogs[KEEP] = {
        text = "The \"%s\" tab is set to show chat. Kept empty, it will show none of it, "
            .. "though lines can still be picked from it for the overlay.",
        button1 = "Keep it empty",
        button2 = "Cancel",
        -- Nothing to do for a choice the options have since moved on from.
        OnAccept = function(_, chosen)
            if ns.db.emptyWindow == chosen.name then Keep(chosen.name) end
        end,
        OnCancel = function(_, chosen)
            if ns.db.emptyWindow == chosen.name then chosen.back(kept) end
        end,
        timeout = 0,
        whileDead = true,
        hideOnEscape = true,
    }
    StaticPopupDialogs[NAME] = {
        text = "Name of the new chat tab, which will show nothing and be kept empty:",
        button1 = "Make it",
        button2 = "Cancel",
        hasEditBox = true,
        OnShow = function(dialog)
            local box = NameBox(dialog)
            box:SetText(ns.NEW_TAB)
            box:HighlightText()
        end,
        OnAccept = MakeNamed,
        EditBoxOnEnterPressed = function(box, made)
            local dialog = box:GetParent()
            MakeNamed(dialog, made or dialog.data)
            dialog:Hide()
        end,
        EditBoxOnEscapePressed = function(box) box:GetParent():Hide() end,
        timeout = 0,
        whileDead = true,
        hideOnEscape = true,
    }
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        if frame and type(frame.AddMessage) == "function" and hooksecurefunc then
            hooksecurefunc(frame, "AddMessage", function(window)
                if window == Kept() then window:Clear() end
            end)
        end
    end
    Keep(ns.db.emptyWindow)
end
