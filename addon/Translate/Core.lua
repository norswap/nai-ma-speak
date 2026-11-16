local addonName, ns = ...

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
    showLinks = true, -- add a clickable [T] after Chinese chat lines
    strip = true,     -- draw new Chinese lines as a pixel strip for the companion
    allLines = true,  -- also send lines without Chinese, shown untranslated in the overlay
    sourceWindow = "", -- tab name of the chat window whose settings pick the lines sent; "" sends all
}

function ns.Print(msg)
    DEFAULT_CHAT_FRAME:AddMessage("|cff66ccffTranslate:|r " .. msg)
end

-- Secret values (Midnight) cannot be compared or concatenated.
function ns.IsSecret(value)
    return issecretvalue ~= nil and issecretvalue(value) == true
end

ns.On("ADDON_LOADED", function(name)
    if name ~= addonName then return end
    TranslateDB = TranslateDB or {}
    for k, v in pairs(DEFAULTS) do
        if TranslateDB[k] == nil then TranslateDB[k] = v end
    end
    ns.db = TranslateDB
    frame:UnregisterEvent("ADDON_LOADED")
end)

ns.On("PLAYER_LOGIN", function()
    if ns.InitChat then ns.InitChat() end
    if ns.InitLayout then ns.InitLayout() end
    if ns.InitOptions then ns.InitOptions() end
end)

-- The most lines "/tr test <count>" says, so a typo cannot flood /say.
local TEST_MOST = 30

-- 1 to 99 in Chinese numerals: 7 is 七, 12 is 十二, 30 is 三十.
function ns.ChineseNumber(n)
    local digits = { "一", "二", "三", "四", "五", "六", "七", "八", "九" }
    local tens, ones = math.floor(n / 10), n % 10
    return (tens > 1 and digits[tens] or "") .. (tens > 0 and "十" or "") .. (digits[ones] or "")
end

local function ShowHelp()
    ns.Print("/tr  open the translate window on the latest Chinese line")
    ns.Print("/tr links  toggle the [T] links after Chinese lines")
    ns.Print("/tr test  say a Chinese test line to check the strip and overlay")
    ns.Print("/tr test 10  say that many short test lines (at most " .. TEST_MOST .. ")")
    ns.Print("/tr options  open the options (also under Options > AddOns)")
    ns.Print("/tr all  toggle showing lines without Chinese in the overlay too")
    ns.Print("/tr strip  toggle the pixel strip the companion reads (top-left corner)")
    ns.Print("/tr probe  report which client features this addon found")
end

local commands = {
    [""] = function() ns.ShowLatest() end,
    links = function()
        ns.db.showLinks = not ns.db.showLinks
        ns.Print("[T] links " .. (ns.db.showLinks and "on." or "off."))
    end,
    options = function()
        if ns.OpenOptions then ns.OpenOptions() else ns.Print("No options panel on this client.") end
    end,
    all = function()
        ns.db.allLines = not ns.db.allLines
        ns.Print("The overlay shows " .. (ns.db.allLines and "all chat lines." or "Chinese lines only."))
    end,
    strip = function()
        ns.db.strip = not ns.db.strip
        ns.SetStripShown(ns.db.strip)
        ns.Print("Pixel strip " .. (ns.db.strip and "on." or "off. The overlay gets no new lines."))
    end,
    -- With a count, says that many short lines, each a number in Chinese, half
    -- a second apart: enough different lines to see how the translator copes
    -- with a burst. The companion never reuses an earlier translation for a
    -- line marked "(Translate test)".
    test = function(count)
        -- Mainline moved SendChatMessage into C_ChatInfo; probe both.
        local send = (C_ChatInfo and C_ChatInfo.SendChatMessage) or SendChatMessage
        if not send then
            ns.Print("No SendChatMessage on this client. Type /s 需要奶妈 by hand.")
            return
        end
        if not count then
            send("大家好，今天一起打副本嗎？ (Translate test)", "SAY")
            ns.Print("Sent a test line in /say. The companion overlay should show it in English shortly.")
            return
        end
        count = math.max(1, math.min(count, TEST_MOST))
        for i = 1, count do
            C_Timer.After((i - 1) * 0.5, function() send(ns.ChineseNumber(i) .. " (Translate test)", "SAY") end)
        end
        ns.Print("Saying " .. count .. " test lines in /say, half a second apart.")
    end,
    probe = function()
        local _, _, _, toc = GetBuildInfo()
        ns.Print("Interface " .. tostring(toc))
        ns.Print("issecretvalue: " .. tostring(issecretvalue ~= nil))
        ns.Print("Chat filter API: " .. tostring(ns.filterSource))
        ns.Print("Link click hook: " .. tostring(ns.linkSource))
        ns.Print("Chat window positions: " .. tostring(ns.layoutSource))
        ns.Print("Options panel: " .. tostring(ns.optionsSource))
        ns.Print("Lines sent: " .. ((ns.db.sourceWindow or "") == "" and "all chat" or ("what the " .. ns.db.sourceWindow .. " window shows")))
        ns.Print("Chat window copying: " .. tostring(ns.mirrorSource))
        ns.Print("Overlay scrolling: " .. tostring(ns.wheelSource))
        local font = ChatFrame1 and ChatFrame1.GetFont and ChatFrame1:GetFont()
        ns.Print("Chat font: " .. tostring(font))
        local w, h = GetPhysicalScreenSize()
        ns.Print("Physical screen: " .. tostring(w) .. "x" .. tostring(h) .. ", strip " .. (ns.db.strip and "on" or "off"))
        ns.Print("SendChatMessage: " .. ((C_ChatInfo and C_ChatInfo.SendChatMessage) and "C_ChatInfo" or (SendChatMessage and "global" or "none")))
    end,
    help = ShowHelp,
}

SLASH_TRANSLATE1 = "/tr"
SLASH_TRANSLATE2 = "/translate"
SlashCmdList.TRANSLATE = function(input)
    local cmd, argument = strlower(strtrim(input or "")):match("^(%S*)%s*(.-)$")
    local fn = commands[cmd] or ShowHelp
    fn(tonumber(argument))
end
