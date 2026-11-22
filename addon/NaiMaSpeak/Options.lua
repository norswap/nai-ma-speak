local _, ns = ...

-- The panel under Options > AddOns. Every control writes straight to
-- NaiMaSpeakDB, the same values the /tr commands toggle.

local NONE = "" -- the value of a chat window setting with no window picked

-- Choices for a dropdown that picks a chat window by the name on its tab: `none`
-- for picking no window, then one entry per window, worded by `label`. Built each
-- time the dropdown opens, so renamed and new windows show up. A saved window that
-- no longer exists stays listed, with `missing` saying what happens instead.
local function WindowOptions(key, none, label, missing, skip)
    return function()
        local container = Settings.CreateControlTextContainer()
        container:Add(NONE, none)
        local saved, found = ns.db[key], false
        for _, name in ipairs(ns.ChatWindowNames()) do
            if name ~= skip then container:Add(name, label(name)) end
            found = found or name == saved
        end
        if type(saved) == "string" and saved ~= NONE and not found then
            container:Add(saved, "\"" .. saved .. "\" (no such window: " .. missing .. ")")
        end
        return container:GetData()
    end
end

-- Opens the panel; replaced below once there is one.
function ns.OpenOptions()
    ns.Print("No options panel on this client.")
end

local function Build()
    local category, layout = Settings.RegisterVerticalLayoutCategory("Nai Ma Speak")

    local function Checkbox(key, label, tooltip)
        local setting = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_" .. key, key, ns.db,
            Settings.VarType.Boolean, label, true)
        Settings.CreateCheckbox(category, setting, tooltip)
        return setting
    end

    local source = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_sourceWindow", "sourceWindow", ns.db,
        Settings.VarType.String, "Lines sent to the overlay", NONE)
    Settings.CreateDropdown(category, source,
        WindowOptions("sourceWindow", "All chat", function(name) return "Copy the \"" .. name .. "\" window" end,
            "all chat is sent"),
        "Send player chat and the game's own messages as they arrive, or copy one chat window: "
        .. "every line it shows, worded and colored the same, including what other addons print. "
        .. "Pick the message types and channels in that window's own settings "
        .. "(right-click its tab, Settings). Its tab does not have to be selected, "
        .. "so a spare tab can serve just to make the selection.")

    -- Emptying the combat log's tab would only break it.
    local combatLog = COMBATLOG and FCF_GetChatWindowInfo(COMBATLOG:GetID())
    local empty = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_emptyWindow", "emptyWindow", ns.db,
        Settings.VarType.String, "Tab kept empty", NONE)
    Settings.CreateDropdown(category, empty,
        WindowOptions("emptyWindow", "None", function(name) return name end, "none is kept empty", combatLog),
        "Throw away every line the game writes into this tab, so nothing shows behind the overlay. "
        .. "A choice is taken up once it has stood for a moment. "
        .. "Pick the tab the companion app's overlay is anchored to. The game writes some lines, such as "
        .. "the answer to /played, into whichever tab is in front, even one set to show nothing. "
        .. "Lines thrown away do not come back when this is turned off.")
    Settings.SetOnValueChangedCallback("NaiMaSpeak_emptyWindow", function()
        ns.EmptyChosen(function(name) empty:SetValue(name) end)
    end, ns)

    if CreateSettingsButtonInitializer and FCF_OpenNewWindow then
        local button = CreateSettingsButtonInitializer("Tab for the overlay", "New empty tab", function()
            ns.AskNewEmptyTab(function(name)
                empty:SetValue(name)
                ns.Print("Made the \"" .. name .. "\" tab, kept empty. "
                    .. "To put the overlay on it, pick it under \"Anchor to chat tab\" in the companion app.")
            end)
        end, "Open a chat tab that shows nothing, under a name you choose, and keep it empty. "
            .. "The game allows " .. (NUM_CHAT_WINDOWS or 10) .. " chat windows: with all of them in use "
            .. "this button is greyed out, until a tab is closed (right-click it, Close Window).", true)
        layout:AddInitializer(button)
        -- Greys the button out on clients where a button follows this; a click says why either way.
        if button.AddModifyPredicate then button:AddModifyPredicate(ns.CanAddTab) end
    end

    local repeated = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_repeatLines", "repeatLines", ns.db,
        Settings.VarType.Number, "Lines shown to the companion again", 50)
    Settings.CreateDropdown(category, repeated, function()
        local container = Settings.CreateControlTextContainer()
        container:Add(0, "None")
        for _, count in ipairs({ 20, 50, 100, 200 }) do container:Add(count, "The last " .. count) end
        return container:GetData()
    end, "While no new line waits, the last chat lines are sent to the companion again and again. "
        .. "Lines it missed, because the game was covered by another window or the companion was not running, "
        .. "then appear in the overlay in their place within seconds. Lines it already has are not shown twice.")

    local buttons = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_buttons", "buttons", ns.db,
        Settings.VarType.String, "Translate and App buttons", "tabs")
    Settings.CreateDropdown(category, buttons, function()
        local container = Settings.CreateControlTextContainer()
        container:Add("tabs", "On the tab row")
        container:Add("top", "Beside the chat window, top right")
        container:Add("bottom", "Beside the chat window, bottom right")
        container:Add("hidden", "Hidden")
        return container:GetData()
    end, "Where the two buttons sit on the main chat window. Translate opens and closes the box for typing a reply; "
        .. "App shows and hides the companion app's window over the game.")
    Settings.SetOnValueChangedCallback("NaiMaSpeak_buttons", function() ns.PlaceButtons() end, ns)

    local strip = Settings.RegisterAddOnSetting(category, "NaiMaSpeak_stripPosition", "stripPosition", ns.db,
        Settings.VarType.String, "Pixel block position", "topleft")
    Settings.CreateDropdown(category, strip, function()
        local container = Settings.CreateControlTextContainer()
        container:Add("topleft", "Top left")
        container:Add("topright", "Top right")
        container:Add("bottomleft", "Bottom left")
        container:Add("bottomright", "Bottom right")
        container:Add("custom", "Custom (drag it in Edit Mode)")
        return container:GetData()
    end, "Where on the screen the small block of colored squares is drawn, which the companion app reads the chat from. "
        .. "It can also be dragged anywhere while the game's Edit Mode is open (Game Menu, Edit Mode): "
        .. "this then reads Custom. Keep it where nothing covers it, and out of a window's rounded bottom corners. Top left is the one place that stays put "
        .. "when the game's window is resized; elsewhere the companion needs a few seconds to find it again.")
    Settings.SetOnValueChangedCallback("NaiMaSpeak_stripPosition", function() ns.PlaceStrip() end, ns)

    Checkbox("minimap", "Minimap button",
        "A button on the minimap that opens these options or the companion app's window. Drag it to move it round the minimap.")
    Settings.SetOnValueChangedCallback("NaiMaSpeak_minimap", function() ns.PlaceMinimapButton() end, ns)

    Checkbox("allLines", "Include lines without Chinese",
        "Also send lines without Chinese. The overlay shows them untranslated, in order with the translated ones.")
    Checkbox("showLinks", "[T], [?] and [?N] links after chat lines",
        "Add links after lines in the game's chat windows. After a Chinese message: [T] sends it to the overlay "
        .. "to be translated again, for a line that scrolled away or was dropped when chat was too busy, and "
        .. "[?] has it explained word by word, for learning Chinese. After a line from a player with a Chinese "
        .. "name: [?N] explains the name. All go ahead of everything waiting.")

    Settings.RegisterAddOnCategory(category)
    return category
end

function ns.InitOptions()
    if not (Settings and Settings.RegisterVerticalLayoutCategory and Settings.RegisterAddOnSetting
            and Settings.CreateDropdown and Settings.CreateCheckbox) then
        ns.optionsSource = "none (no Settings API)"
        return
    end
    -- The Settings API differs between client versions; a failure here must
    -- not take the rest of the addon down with it.
    local ok, result = pcall(Build)
    if not ok then
        ns.optionsSource = "failed: " .. tostring(result)
        return
    end
    ns.optionsSource = "Settings API"
    function ns.OpenOptions()
        Settings.OpenToCategory(result:GetID())
    end
end
