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

local function ShowHelp()
    ns.Print("/tr  open the translate window on the latest Chinese line")
    ns.Print("/tr links  toggle the [T] links after Chinese lines")
    ns.Print("/tr test  say a Chinese test line to check the strip and overlay")
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
    test = function()
        -- Mainline moved SendChatMessage into C_ChatInfo; probe both.
        local send = (C_ChatInfo and C_ChatInfo.SendChatMessage) or SendChatMessage
        if not send then
            ns.Print("No SendChatMessage on this client. Type /s 需要奶妈 by hand.")
            return
        end
        send("大家好 (Translate test)", "SAY")
        ns.Print("Sent a test line in /say. The companion overlay should show it in English shortly.")
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
        local w, h = GetPhysicalScreenSize()
        ns.Print("Physical screen: " .. tostring(w) .. "x" .. tostring(h) .. ", strip " .. (ns.db.strip and "on" or "off"))
        ns.Print("SendChatMessage: " .. ((C_ChatInfo and C_ChatInfo.SendChatMessage) and "C_ChatInfo" or (SendChatMessage and "global" or "none")))
    end,
    help = ShowHelp,
}

SLASH_TRANSLATE1 = "/tr"
SLASH_TRANSLATE2 = "/translate"
SlashCmdList.TRANSLATE = function(input)
    local cmd = strlower(strtrim(input or ""))
    local fn = commands[cmd] or ShowHelp
    fn()
end
