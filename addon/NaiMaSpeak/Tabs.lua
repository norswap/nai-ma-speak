local _, ns = ...

-- Keeps one chat tab empty, for the companion's overlay to sit on, and makes such a
-- tab. The game writes some lines (the answer to /played, for one) into whichever tab
-- is in front, whatever that tab is set to show, so the tab is emptied whenever a line
-- is added to it. What the tab is set to show still counts for "Lines sent to the
-- overlay": one tab can pick the lines and stay empty under their translations.

local POPUP = "NAIMASPEAK_KEEP_EMPTY"
local asking = false -- the player has yet to confirm the tab chosen

-- The chat window kept empty, if one is chosen in the options and still exists.
local function Kept()
    local name = ns.db.emptyWindow
    if asking or type(name) ~= "string" or name == "" then return nil end
    return ns.ChatWindowNamed(name)
end

local function Empty()
    local window = Kept()
    if window then window:Clear() end
end

-- Takes up the tab just chosen in the options. A tab set to show chat is emptied only
-- once the player agrees to lose what it shows; `undo` puts the options back otherwise.
function ns.EmptyChosen(undo)
    local window = Kept()
    if window and #(window.messageTypeList or {}) + #(window.channelList or {}) > 0 then
        asking = true
        StaticPopup_Show(POPUP, ns.db.emptyWindow, nil, undo)
    else
        Empty()
    end
end

-- The name of the tab the options' button makes.
ns.NEW_TAB = "Translated"

-- Whether the game has a chat window left to open: it allows only so many.
function ns.CanAddTab()
    return #ns.ChatWindowNames() < (NUM_CHAT_WINDOWS or 10)
end

-- Opens a chat tab set to show nothing. Returns the window, or nil and why not.
function ns.NewEmptyTab()
    if ns.ChatWindowNamed(ns.NEW_TAB) then
        return nil, "There is already a tab named \"" .. ns.NEW_TAB .. "\"."
    end
    local window = ns.CanAddTab() and FCF_OpenNewWindow(ns.NEW_TAB)
    if not window then
        return nil, "All " .. (NUM_CHAT_WINDOWS or 10) .. " chat windows the game allows are in use. "
            .. "Close a tab (right-click it, Close Window) to free one."
    end
    window:RemoveAllMessageGroups()
    window:RemoveAllChannels()
    return window
end

function ns.InitTabs()
    StaticPopupDialogs[POPUP] = {
        text = "The \"%s\" tab is set to show chat. Kept empty, it will show none of it, "
            .. "though lines can still be picked from it for the overlay.",
        button1 = "Keep it empty",
        button2 = "Cancel",
        OnAccept = function()
            asking = false
            Empty()
        end,
        OnCancel = function(_, undo)
            asking = false
            undo()
        end,
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
    Empty()
end
