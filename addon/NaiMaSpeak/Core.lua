local addonName, ns = ...

-- The release this file belongs to; the release workflow writes it here and in the .toc.
ns.VERSION = "0.3.0"

-- One event frame, one handler table keyed by event name.
ns.handlers = {}
local frame = CreateFrame("Frame")
ns.eventFrame = frame

function ns.On(event, fn)
    ns.handlers[event] = fn
    frame:RegisterEvent(event)
end

frame:SetScript("OnEvent", function(_, event, ...)
    local fn = ns.handlers[event]
    if fn then fn(...) end
end)

local DEFAULTS = {
    allLines = true,  -- also send lines without Chinese, shown untranslated in the overlay
    showLinks = true, -- add links after chat lines: translate again, explain, explain the name
    sourceWindow = "", -- tab name of the chat window whose settings pick the lines sent; "" sends all
    emptyWindow = "", -- tab name of the chat window kept empty for the overlay; "" keeps none
    repeatLines = 50, -- how many of the last chat lines the strip shows again while nothing new waits
    stripPosition = "topleft", -- the screen corner the strip is drawn in, or "custom"
    stripFrom = "topleft", -- for "custom": the screen corner the strip is nearest to,
    stripX = 0, -- and how far in from it the strip sits, in the game's units (the screen is 768 high)
    stripY = 0,
    minimap = true, -- show the button on the minimap
    minimapAngle = 195, -- where on the minimap's rim, in degrees counted from the right, leftwards
    buttons = "tabs", -- where the Translate and App buttons sit: "tabs", "top", "bottom" or "hidden"
}

function ns.Print(msg)
    DEFAULT_CHAT_FRAME:AddMessage("|cff66ccffNai Ma Speak:|r " .. msg)
end

-- Secret values (Midnight) cannot be compared or concatenated.
function ns.IsSecret(value)
    return issecretvalue ~= nil and issecretvalue(value) == true
end

ns.On("ADDON_LOADED", function(name)
    if name ~= addonName then return end
    NaiMaSpeakDB = NaiMaSpeakDB or {}
    for k, v in pairs(DEFAULTS) do
        if NaiMaSpeakDB[k] == nil then NaiMaSpeakDB[k] = v end
    end
    ns.db = NaiMaSpeakDB
    frame:UnregisterEvent("ADDON_LOADED")
end)

-- Tells the companion which version runs, for it to say when the game holds an older
-- addon than the folder does: the version in this file, which /reload reads again,
-- and the one in the .toc, which the game reads only when it starts.
local VERSION_TIME = 5 -- seconds between tellings, for a companion started later

function ns.SayVersion()
    local metadata = (C_AddOns and C_AddOns.GetAddOnMetadata) or GetAddOnMetadata
    local listed = metadata and metadata(addonName, "Version")
    -- With the character's name, which the companion can keep each one's chat by.
    -- UnitFullName gives both of a WoW: Forever character's names, where UnitName gives the first.
    local character = (UnitFullName and UnitFullName("player")) or (UnitName and UnitName("player"))
    if type(character) ~= "string" or ns.IsSecret(character) then character = "" end
    ns.SendToStrip("V\t" .. ns.VERSION .. "\t" .. (type(listed) == "string" and listed or "") .. "\t" .. character, "V\t")
end

-- Before 0.4.0 the addon's folder was named Translate. Left beside this one, the old
-- addon would draw a strip of its own and send every line twice, so it is turned off;
-- told by its title, since another addon may go by so plain a name. Its settings are
-- taken over, once, while both are loaded.
local OLD = "Translate"

function ns.RetireOld()
    local addons = C_AddOns
    if not (addons and addons.GetAddOnMetadata and addons.IsAddOnLoaded and addons.DisableAddOn) then return end
    -- Asking about an addon that is not installed is an error.
    local ok, title = pcall(addons.GetAddOnMetadata, OLD, "Title")
    if not ok or title ~= "Nai Ma Speak" or not addons.IsAddOnLoaded(OLD) then return end
    if type(TranslateDB) == "table" and not ns.db.tookOverOld then
        for key, value in pairs(TranslateDB) do ns.db[key] = value end
        ns.db.tookOverOld = true
    end
    addons.DisableAddOn(OLD)
    if addons.SaveAddOns then addons.SaveAddOns() end
    ns.Print("This addon's folder is now named " .. addonName .. ". The old one, " .. OLD .. ", is still installed and "
        .. "has been turned off, its settings taken over: type /reload, then delete Interface/AddOns/" .. OLD .. ".")
end

ns.On("PLAYER_LOGIN", function()
    ns.RetireOld()
    if ns.InitChat then ns.InitChat() end
    if ns.InitLayout then ns.InitLayout() end
    if ns.InitTabs then ns.InitTabs() end
    if ns.InitButtons then ns.InitButtons() end
    if ns.InitMinimap then ns.InitMinimap() end
    if ns.InitOptions then ns.InitOptions() end
    ns.SayVersion()
    if C_Timer and C_Timer.NewTicker then C_Timer.NewTicker(VERSION_TIME, ns.SayVersion) end
end)

-- The most lines "/tr test <count>" sends: 1 to 99 is what ChineseNumber writes.
local TEST_MOST = 99

-- 1 to 99 in Chinese numerals: 7 is 七, 12 is 十二, 30 is 三十.
function ns.ChineseNumber(n)
    local digits = { "一", "二", "三", "四", "五", "六", "七", "八", "九" }
    local tens, ones = math.floor(n / 10), n % 10
    return (tens > 1 and digits[tens] or "") .. (tens > 0 and "十" or "") .. (digits[ones] or "")
end

local function ShowHelp()
    ns.Print("/tr (or /nms)  open or close the box for typing a reply")
    ns.Print("/tr hello  translate English into Chinese and copy it, to paste into chat")
    ns.Print("/tr /g hello  the same, and open the chat box on guild (or /p, /s, /w Name, /2 ...)")
    ns.Print("/tr /name  explain your target's name in the overlay, or /tr /name 黑熊 for one you type")
    ns.Print("/tr /options  open the options (also under Options > AddOns)")
    ns.Print("/tr /all  toggle showing lines without Chinese in the overlay too")
    ns.Print("/tr /test  say a Chinese test line to check the strip and overlay")
    ns.Print("/tr /test 10  send that many short test lines to the overlay only (at most " .. TEST_MOST .. ")")
    ns.Print("/tr /demo  play a made-up Chinese conversation in your chat window, seen only by you")
    ns.Print("/tr /probe  report which client features this addon found")
    ns.Print("/tr /help  this list")
end

local commands = {
    options = function() ns.OpenOptions() end,
    all = function()
        ns.db.allLines = not ns.db.allLines
        ns.Print("The overlay shows " .. (ns.db.allLines and "all chat lines." or "Chinese lines only."))
    end,
    -- Explains a player's name in the overlay: the one given, or the target's.
    name = function(_, given)
        local name = given ~= "" and given or (UnitName and UnitName("target"))
        if ns.IsSecret(name) or type(name) ~= "string" or name == "" then
            ns.Print("Whose name? Target a player, or type one: /tr /name 黑熊")
            return
        end
        ns.ExplainName(name)
    end,
    -- With a count, sends that many short lines, each a number in Chinese,
    -- straight to the overlay: enough different lines to see how the
    -- translator copes with a burst. Nothing is said in chat: the game only
    -- lets an addon talk in /say right on a key press, not from a timer. The
    -- companion never reuses an earlier translation for a line marked
    -- "(Translate test)".
    test = function(count)
        if count then
            count = math.max(1, math.min(count, TEST_MOST))
            for i = 1, count do
                ns.SendTestLine(ns.ChineseNumber(i) .. " (Translate test)")
            end
            ns.Print("Sent " .. count .. " test lines to the overlay. They are not said in chat.")
            return
        end
        -- Mainline moved SendChatMessage into C_ChatInfo; probe both.
        local send = (C_ChatInfo and C_ChatInfo.SendChatMessage) or SendChatMessage
        if not send then
            ns.Print("No SendChatMessage on this client. Type /s 需要奶妈 by hand.")
            return
        end
        send("大家好，今天一起打副本嗎？ (Translate test)", "SAY")
        ns.Print("Sent a test line in /say. The companion overlay should show it in English shortly.")
    end,
    demo = function()
        ns.Demo()
        ns.Print("Playing a made-up conversation. Nothing is said in chat; only you see it.")
    end,
    probe = function()
        local _, _, _, toc = GetBuildInfo()
        ns.Print("Interface " .. tostring(toc))
        ns.Print("issecretvalue: " .. tostring(issecretvalue ~= nil))
        ns.Print("Chat filter API: " .. tostring(ns.filterSource))
        ns.Print("Chat window positions: " .. tostring(ns.layoutSource))
        ns.Print("Options panel: " .. tostring(ns.optionsSource))
        ns.Print("Lines sent: " .. ((ns.db.sourceWindow or "") == "" and "all chat" or ("what the " .. ns.db.sourceWindow .. " window shows")))
        ns.Print("Chat window copying: " .. tostring(ns.mirrorSource))
        ns.Print("Link clicks: " .. tostring(ns.linkSource))
        ns.Print("Overlay scrolling: " .. tostring(ns.wheelSource))
        ns.Print("Chat window buttons: " .. tostring(ns.buttonSource))
        ns.Print("Minimap button: " .. tostring(ns.minimapSource))
        ns.Print("Moving the strip: " .. tostring(ns.stripMoveSource))
        local font = ChatFrame1 and ChatFrame1.GetFont and ChatFrame1:GetFont()
        ns.Print("Chat font: " .. tostring(font))
        local w, h = GetPhysicalScreenSize()
        ns.Print("Physical screen: " .. tostring(w) .. "x" .. tostring(h))
        ns.Print("SendChatMessage: " .. ((C_ChatInfo and C_ChatInfo.SendChatMessage) and "C_ChatInfo" or (SendChatMessage and "global" or "none")))
    end,
    help = ShowHelp,
}

SLASH_NAIMASPEAK1 = "/tr"
SLASH_NAIMASPEAK2 = "/translate"
SLASH_NAIMASPEAK3 = "/nms"
-- "/tr" alone is the Translate button. A command is a word after a slash
-- ("/tr /test 5"), so no English sentence can be taken for one; a slash word
-- that is no command is a chat target ("/tr /g hello").
SlashCmdList.NAIMASPEAK = function(input)
    local text = strtrim(input or "")
    if text == "" then
        ns.SendCommand("reply")
        return
    end
    local word, argument = text:match("^/(%S+)%s*(.-)$")
    local fn = word and commands[strlower(word)]
    if fn then
        fn(tonumber(argument), argument)
    else
        ns.Reply(text)
    end
end
