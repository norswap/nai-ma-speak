local _, ns = ...

-- Keeps one chat tab empty, for the companion's overlay to sit on. The game writes
-- some lines (the answer to /played, for one) into whichever tab is in front, whatever
-- that tab is set to show, so the tab is emptied whenever a line is added to it.
-- What the tab is set to show still counts for "Lines sent to the overlay": one tab
-- can pick the lines and stay empty under their translations.

local POPUP = "TRANSLATE_KEEP_EMPTY"
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
