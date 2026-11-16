local addonName, ns = ...

-- Captured Chinese lines, keyed by a running id the companion echoes back.
ns.messages = {}
ns.translations = {}
local nextId = 1
local oldestId = 1
local KEEP = 300
local PREFIX = "TR#"

local CHAT_EVENTS = {
    "CHAT_MSG_SAY", "CHAT_MSG_YELL", "CHAT_MSG_WHISPER",
    "CHAT_MSG_PARTY", "CHAT_MSG_PARTY_LEADER",
    "CHAT_MSG_RAID", "CHAT_MSG_RAID_LEADER", "CHAT_MSG_RAID_WARNING",
    "CHAT_MSG_INSTANCE_CHAT", "CHAT_MSG_INSTANCE_CHAT_LEADER",
    "CHAT_MSG_GUILD", "CHAT_MSG_OFFICER", "CHAT_MSG_CHANNEL",
}

-- Lines that are not player chat: the game's own messages, emotes, creatures
-- and your own whispers. They get no [T] link and only reach the overlay.
local INFO_EVENTS = {
    "CHAT_MSG_SYSTEM", "CHAT_MSG_EMOTE", "CHAT_MSG_TEXT_EMOTE", "CHAT_MSG_WHISPER_INFORM",
    "CHAT_MSG_MONSTER_SAY", "CHAT_MSG_MONSTER_YELL", "CHAT_MSG_MONSTER_WHISPER", "CHAT_MSG_MONSTER_PARTY",
    "CHAT_MSG_LOOT", "CHAT_MSG_MONEY", "CHAT_MSG_CURRENCY", "CHAT_MSG_SKILL",
    "CHAT_MSG_COMBAT_XP_GAIN", "CHAT_MSG_COMBAT_HONOR_GAIN", "CHAT_MSG_COMBAT_FACTION_CHANGE",
    "CHAT_MSG_COMBAT_MISC_INFO", "CHAT_MSG_TRADESKILLS", "CHAT_MSG_OPENING", "CHAT_MSG_PET_INFO",
    "CHAT_MSG_BG_SYSTEM_NEUTRAL", "CHAT_MSG_BG_SYSTEM_ALLIANCE", "CHAT_MSG_BG_SYSTEM_HORDE",
}

-- CJK Unified Ideographs (U+4E00..U+9FFF) start with UTF-8 lead bytes E4..E9.
local function HasChinese(text)
    return text:find("[\228-\233][\128-\191][\128-\191]") ~= nil
end

-- Hyperlinks and color codes would confuse the translator; send plain text.
local function StripCodes(text)
    text = text:gsub("|H.-|h(.-)|h", "%1")
    text = text:gsub("|c%x%x%x%x%x%x%x%x", "")
    text = text:gsub("|r", "")
    text = text:gsub("[\r\n]", " ") -- a newline separates lines on the strip
    return text
end

-- A line as the overlay draws it: links become their text, icons go, and the
-- color codes (|cAARRGGBB, |cnNAME: and |r) stay for the companion to apply.
local function Markup(text)
    text = text:gsub("|H.-|h(.-)|h", "%1")
    text = text:gsub("|T.-|t", "")
    text = text:gsub("|A.-|a", "")
    text = text:gsub("|K.-|k", "?")
    text = text:gsub("|n", " ")
    -- Tabs and newlines separate fields and lines on the strip. Not "%c",
    -- which in some locales also matches bytes inside Chinese characters.
    text = text:gsub("[%z\1-\31]", " ")
    return text
end

-- Length of the color code that starts at byte i, or nil if none does.
local function CodeLength(markup, i)
    if markup:find("^|c%x%x%x%x%x%x%x%x", i) then return 10 end
    if markup:find("^|r", i) then return 2 end
    local _, last = markup:find("^|cn[^:|]*:", i)
    return last and last - i + 1
end

-- Markup without its color codes: the text as read.
local function Plain(markup)
    local parts, i = {}, 1
    while i <= #markup do
        local code = CodeLength(markup, i)
        if code then
            i = i + code
        else
            local nextBar = markup:find("|", i + 1, true) or (#markup + 1)
            parts[#parts + 1] = markup:sub(i, nextBar - 1)
            i = nextBar
        end
    end
    return table.concat(parts)
end

-- Where in markup the text starts that follows `count` bytes of plain text.
-- Color codes at the cut stay with the part before it.
local function MarkupIndex(markup, count)
    local i, seen = 1, 0
    while i <= #markup do
        local code = CodeLength(markup, i)
        if code then
            i = i + code
        elseif seen >= count then
            break
        else
            i = i + 1
            seen = seen + 1
        end
    end
    return i
end

local function Hex(r, g, b)
    local function Byte(v)
        v = type(v) == "number" and v or 1
        return math.floor(math.max(0, math.min(1, v)) * 255 + 0.5)
    end
    return string.format("%02x%02x%02x", Byte(r), Byte(g), Byte(b))
end

-- One line for the overlay: "<id>\t<prefix>\t<body>\t<rrggbb>". Prefix and body
-- are markup and together make the line as shown in chat, in the given color;
-- the body is the part to translate. Id 0 marks a line shown as it is. A body
-- too long for the strip is cut, so the color after it always arrives.
local function Send(id, prefix, body, color)
    local room = ns.STRIP.maxText - (#tostring(id) + #prefix + #color + 3)
    ns.SendToStrip(id .. "\t" .. prefix .. "\t" .. ns.Truncate(body, math.max(room, 0)) .. "\t" .. color)
end

ns.Markup, ns.Plain, ns.MarkupIndex, ns.Hex = Markup, Plain, MarkupIndex, Hex

function ns.CopyText(id)
    local entry = ns.messages[id]
    return entry and (PREFIX .. id .. " " .. entry.text) or ""
end

-- Chat frames each run the filter, so one line can arrive several times.
-- lineID is unique per line and maps it to a single stored id.
local byLine = {}

local function Store(text, author, lineID)
    local id = nextId
    nextId = nextId + 1
    ns.messages[id] = { text = StripCodes(text), author = author, lineID = lineID }
    if lineID then byLine[lineID] = id end
    while nextId - oldestId > KEEP do
        local old = ns.messages[oldestId]
        if old and old.lineID then byLine[old.lineID] = nil end
        ns.messages[oldestId] = nil
        ns.translations[oldestId] = nil
        oldestId = oldestId + 1
    end
    return id
end

function ns.ShowLatest()
    ns.ShowMessage(nextId - 1)
end

-- The companion replies with "TR#<id> <english>". Without a prefix the text
-- belongs to the line currently open in the window.
function ns.ReceiveTranslation(text, fallbackId)
    local id, english = text:match("^%s*" .. PREFIX .. "(%d+)%s+(.+)$")
    id = tonumber(id) or fallbackId
    english = english or text
    if not id or not ns.messages[id] then
        ns.Print("That translation's line is no longer in history.")
        return
    end
    ns.translations[id] = english
    local author = ns.messages[id].author or "?"
    DEFAULT_CHAT_FRAME:AddMessage("|cff66ccff[EN]|r " .. author .. ": " .. english)
    ns.RefreshResult(id)
end

-- How the game words each kind of line around the speaker's name.
local AROUND_NAME = {
    CHAT_MSG_SAY = { "[", "] says: " }, CHAT_MSG_YELL = { "[", "] yells: " },
    CHAT_MSG_WHISPER = { "[", "] whispers: " }, CHAT_MSG_WHISPER_INFORM = { "To [", "]: " },
    CHAT_MSG_PARTY = { "[Party] [", "]: " }, CHAT_MSG_PARTY_LEADER = { "[Party Leader] [", "]: " },
    CHAT_MSG_RAID = { "[Raid] [", "]: " }, CHAT_MSG_RAID_LEADER = { "[Raid Leader] [", "]: " },
    CHAT_MSG_RAID_WARNING = { "[Raid Warning] [", "]: " },
    CHAT_MSG_INSTANCE_CHAT = { "[Instance] [", "]: " }, CHAT_MSG_INSTANCE_CHAT_LEADER = { "[Instance Leader] [", "]: " },
    CHAT_MSG_GUILD = { "[Guild] [", "]: " }, CHAT_MSG_OFFICER = { "[Officer] [", "]: " },
    CHAT_MSG_EMOTE = { "", " " },
    CHAT_MSG_MONSTER_SAY = { "", " says: " }, CHAT_MSG_MONSTER_YELL = { "", " yells: " },
    CHAT_MSG_MONSTER_WHISPER = { "", " whispers: " }, CHAT_MSG_MONSTER_PARTY = { "[Party] ", ": " },
}

-- The color the game gives a kind of line, as the player set it up.
local function LineColor(event, channelIndex)
    local kind = event:sub(10) -- after "CHAT_MSG_"
    local info = ChatTypeInfo and ChatTypeInfo[kind]
    if kind == "CHANNEL" and ChatTypeInfo and type(channelIndex) == "number" then
        info = ChatTypeInfo["CHANNEL" .. channelIndex] or info
    end
    if not info then return "ffffff", nil end
    return Hex(info.r, info.g, info.b), info
end

-- The name in its class color, when the player turned that on for this kind of line.
local function NameMarkup(name, guid, info)
    if info and info.colorNameByClass and type(guid) == "string" and GetPlayerInfoByGUID then
        local _, class = GetPlayerInfoByGUID(guid)
        local color = class and RAID_CLASS_COLORS and RAID_CLASS_COLORS[class]
        if color and color.colorStr then return "|c" .. color.colorStr .. name .. "|r" end
    end
    return name
end

-- Lines without Chinese that were already sent, so each goes out once
-- although every chat frame reports it. Forgets the oldest beyond KEEP.
local plainSeen, plainOrder, plainFirst = {}, {}, 1

local function SeenPlain(lineID)
    if not lineID then return false end
    if plainSeen[lineID] then return true end
    plainSeen[lineID] = true
    plainOrder[#plainOrder + 1] = lineID
    if #plainOrder - plainFirst >= KEEP then
        plainSeen[plainOrder[plainFirst]] = nil
        plainOrder[plainFirst] = nil
        plainFirst = plainFirst + 1
    end
    return false
end

-- The names on the tabs of the game's chat windows, in window order.
function ns.ChatWindowNames()
    local names = {}
    if not FCF_GetChatWindowInfo then return names end
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        local name = FCF_GetChatWindowInfo(i)
        if frame and type(name) == "string" and name ~= "" and (frame:IsShown() or frame.isDocked) then
            names[#names + 1] = name
        end
    end
    return names
end

local function WindowNamed(wanted)
    if not FCF_GetChatWindowInfo then return nil end
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        if frame and FCF_GetChatWindowInfo(i) == wanted then return frame end
    end
end

-- The chat window whose lines are copied to the overlay, if one is chosen in
-- the options and still exists. Without one, lines are sent as their chat
-- events arrive.
local function MirroredWindow()
    local wanted = ns.db.sourceWindow
    if type(wanted) ~= "string" or wanted == "" then return nil end
    return WindowNamed(wanted)
end

-- Stores a Chinese line once and sends it to the pixel strip. Returns its id,
-- or nil when the line is not Chinese or cannot be read (secret values).
-- With allLines on, other lines also go to the strip, with id 0, so the
-- overlay can show the whole conversation in order; they are not stored.
-- While a chat window is mirrored, nothing is sent from here: Chinese lines
-- are only stored, so their [T] link works and Mirror can find them.
-- Arguments follow the CHAT_MSG_* payload: the 4th is the channel's full
-- name, the 8th its number, the 11th the lineID, the 12th the sender's GUID.
local function Capture(event, msg, author, _, channel, _, _, _, channelIndex, _, _, lineID, guid)
    if ns.IsSecret(msg) or ns.IsSecret(author) or ns.IsSecret(channel) or ns.IsSecret(lineID)
        or ns.IsSecret(channelIndex) or ns.IsSecret(guid) then
        return nil
    end
    if type(msg) ~= "string" then return nil end
    local chinese = HasChinese(msg)
    local mirrored = MirroredWindow() ~= nil
    if not chinese and (mirrored or not ns.db.allLines) then return nil end
    if chinese and lineID and byLine[lineID] then return byLine[lineID] end
    if not chinese and SeenPlain(lineID) then return nil end

    local name = type(author) == "string" and author ~= "" and Ambiguate(author, "short") or nil
    local id = chinese and Store(msg, name or "?", lineID) or 0
    if mirrored then return id end

    local color, info = LineColor(event, channelIndex)
    local around = AROUND_NAME[event]
    local prefix = ""
    if event == "CHAT_MSG_CHANNEL" and name then
        local label = type(channel) == "string" and channel ~= "" and ("[" .. channel .. "] ") or ""
        prefix = label .. "[" .. NameMarkup(name, guid, info) .. "]: "
    elseif around and name then
        prefix = around[1] .. NameMarkup(name, guid, info) .. around[2]
    end
    Send(id, Markup(prefix), Markup(msg), color)
    return chinese and id or nil
end

-- Copies a line the mirrored chat window just added, so the overlay shows
-- what that window shows: every kind of line, worded and colored the same,
-- including what other addons print. Only the Chinese part after the
-- speaker is translated.
local function Mirror(frame, text, r, g, b)
    if not ns.db or not ns.db.strip or frame ~= MirroredWindow() then return end
    if ns.IsSecret(text) or ns.IsSecret(r) or ns.IsSecret(g) or ns.IsSecret(b) then return end
    if type(text) ~= "string" then return end

    -- Our own [T] link names the stored line; so does the lineID in the speaker's link.
    local linkPattern = "|Haddon:" .. addonName .. ":(%d+)|h.-|h"
    local id = tonumber(text:match(linkPattern))
    text = text:gsub(" *" .. linkPattern, "")
    local lineID = tonumber(text:match("|Hplayer:[^:|]*:(%d+)"))
    id = id or (lineID and byLine[lineID])
    local entry = id and ns.messages[id]

    local markup = Markup(text)
    local plain = Plain(markup)
    local color = Hex(r, g, b)
    local prefix, body = "", markup
    if HasChinese(plain) then
        -- The message starts where its stored text is found, or failing that
        -- (icons, other addons' edits) after the first ": ".
        local at = entry and plain:find(entry.text, 1, true)
        if not at then
            local _, last = plain:find(": ", 1, true)
            at = last and last + 1
        end
        if at and at > 1 then
            local cut = MarkupIndex(markup, at - 1)
            prefix, body = markup:sub(1, cut - 1), markup:sub(cut)
        end
    end
    local bodyText = Plain(body)
    if not HasChinese(bodyText) then
        -- Nothing to translate, or only the speaker's name is Chinese.
        if ns.db.allLines then Send(0, "", markup, color) end
        return
    end
    if not entry then id = Store(bodyText, "?", nil) end
    Send(id, prefix, body, color)
end

-- Runs Mirror for tests, and behind the hook where an error must not reach the chat window.
function ns.MirrorLine(frame, text, r, g, b)
    local ok, problem = pcall(Mirror, frame, text, r, g, b)
    if not ok then ns.mirrorSource = "failed: " .. tostring(problem) end
end

local function Filter(_, event, msg, author, ...)
    local id = Capture(event, msg, author, ...)
    if not id or not ns.db.showLinks then return false end
    local link = "|Haddon:" .. addonName .. ":" .. id .. "|h|cff66ccff[T]|r|h"
    return false, msg .. " " .. link, author, ...
end

local function OnLinkClicked(link)
    if type(link) ~= "string" then return end
    local name, id = link:match("^addon:([^:]+):(%d+)")
    if name == addonName then ns.ShowMessage(tonumber(id)) end
end

function ns.InitChat()
    -- Mainline moved some ChatFrame_ globals into ChatFrameUtil; probe both.
    local addFilter = ChatFrame_AddMessageEventFilter
    ns.filterSource = "ChatFrame_AddMessageEventFilter"
    if not addFilter and ChatFrameUtil and ChatFrameUtil.AddMessageEventFilter then
        addFilter = ChatFrameUtil.AddMessageEventFilter
        ns.filterSource = "ChatFrameUtil.AddMessageEventFilter"
    end
    if not addFilter then
        ns.filterSource = "none"
        ns.Print("No chat filter API found. Inline [T] links are off; the overlay still works.")
    else
        for _, event in ipairs(CHAT_EVENTS) do addFilter(event, Filter) end
    end

    -- Also capture lines on channels no chat window shows. Capture skips
    -- lines the filter already stored.
    for _, event in ipairs(CHAT_EVENTS) do
        ns.On(event, function(...) Capture(event, ...) end)
    end
    for _, event in ipairs(INFO_EVENTS) do
        ns.On(event, function(...) Capture(event, ...) end)
    end

    -- Every chat window is hooked once; Mirror acts only on the chosen one.
    local hooked = 0
    for i = 1, NUM_CHAT_WINDOWS or 10 do
        local frame = _G["ChatFrame" .. i]
        if frame and type(frame.AddMessage) == "function" and hooksecurefunc then
            hooksecurefunc(frame, "AddMessage", ns.MirrorLine)
            hooked = hooked + 1
        end
    end
    ns.mirrorSource = hooked .. " chat windows hooked"

    -- Clicks on |Haddon:...| links arrive through EventRegistry on Mainline.
    if EventRegistry and EventRegistry.RegisterCallback then
        EventRegistry:RegisterCallback("SetItemRef", function(_, link) OnLinkClicked(link) end, ns)
        ns.linkSource = "EventRegistry SetItemRef"
    else
        ns.linkSource = "none (use /tr)"
    end
end
