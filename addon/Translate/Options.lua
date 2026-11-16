local addonName, ns = ...

-- The panel under Options > AddOns. Every control writes straight to
-- TranslateDB, the same values the /tr commands toggle.

local ALL = "" -- sourceWindow value that sends every line

-- Choices for "Lines sent to the overlay": all chat, or one entry per chat
-- window. Built each time the dropdown opens, so renamed and new windows
-- show up. A saved window that no longer exists stays listed, marked.
local function SourceOptions()
    local container = Settings.CreateControlTextContainer()
    container:Add(ALL, "All chat")
    local saved, found = ns.db.sourceWindow, false
    for _, name in ipairs(ns.ChatWindowNames()) do
        container:Add(name, "Copy the \"" .. name .. "\" window")
        found = found or name == saved
    end
    if type(saved) == "string" and saved ~= ALL and not found then
        container:Add(saved, "\"" .. saved .. "\" (no such window: all chat is sent)")
    end
    return container:GetData()
end

local function Build()
    local category = Settings.RegisterVerticalLayoutCategory("Translate")

    local function Checkbox(key, label, tooltip)
        local setting = Settings.RegisterAddOnSetting(category, "Translate_" .. key, key, ns.db,
            Settings.VarType.Boolean, label, true)
        Settings.CreateCheckbox(category, setting, tooltip)
        return setting
    end

    local source = Settings.RegisterAddOnSetting(category, "Translate_sourceWindow", "sourceWindow", ns.db,
        Settings.VarType.String, "Lines sent to the overlay", ALL)
    Settings.CreateDropdown(category, source, SourceOptions,
        "Send player chat and the game's own messages as they arrive, or copy one chat window: "
        .. "every line it shows, worded and colored the same, including what other addons print. "
        .. "Pick the message types and channels in that window's own settings "
        .. "(right-click its tab, Settings). Its tab does not have to be selected, "
        .. "so a spare tab can serve just to make the selection.")

    local buttons = Settings.RegisterAddOnSetting(category, "Translate_buttons", "buttons", ns.db,
        Settings.VarType.String, "Reply and Settings buttons", "tabs")
    Settings.CreateDropdown(category, buttons, function()
        local container = Settings.CreateControlTextContainer()
        container:Add("tabs", "On the tab row")
        container:Add("top", "Top right of the chat window")
        container:Add("bottom", "Bottom right of the chat window")
        container:Add("hidden", "Hidden")
        return container:GetData()
    end, "Where the two buttons sit on the main chat window. Reply opens the companion's reply box; "
        .. "Settings brings the companion's window over the game.")
    Settings.SetOnValueChangedCallback("Translate_buttons", function() ns.PlaceButtons() end, ns)

    Checkbox("allLines", "Include lines without Chinese",
        "Also send lines without Chinese. The overlay shows them untranslated, in order with the translated ones.")
    Checkbox("showLinks", "[T] links after Chinese lines",
        "Add a clickable [T] after each Chinese chat line, to translate it by copy and paste.")
    Checkbox("strip", "Pixel strip",
        "Draw chat lines as a small strip of colored blocks in the top-left corner, which the companion app reads. "
        .. "Without it the overlay gets no lines.")
    Settings.SetOnValueChangedCallback("Translate_strip", function()
        ns.SetStripShown(ns.db.strip)
        ns.PlaceButtons()
    end, ns)

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
