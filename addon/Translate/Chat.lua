local _, ns = ...

local CHAT_EVENTS = {
    "CHAT_MSG_SAY", "CHAT_MSG_YELL", "CHAT_MSG_WHISPER",
    "CHAT_MSG_PARTY", "CHAT_MSG_PARTY_LEADER",
    "CHAT_MSG_RAID", "CHAT_MSG_RAID_LEADER", "CHAT_MSG_RAID_WARNING",
    "CHAT_MSG_INSTANCE_CHAT", "CHAT_MSG_INSTANCE_CHAT_LEADER",
    "CHAT_MSG_GUILD", "CHAT_MSG_OFFICER", "CHAT_MSG_CHANNEL",
}

-- Lines that are not player chat: the game's own messages, emotes, creatures
-- and your own whispers.
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

-- A named color code (|cnNAME:) as an ordinary one, looked up among the
-- game's color objects. A name the game does not know stays as it is, and its
-- text then takes the color around it.
local function NamedColor(name)
    local color = _G[name]
    if type(color) ~= "table" or type(color.r) ~= "number" or type(color.g) ~= "number"
        or type(color.b) ~= "number" then
        return nil
    end
    return "|cff" .. ns.Hex(color.r, color.g, color.b)
end

-- A line as the overlay draws it: links become their text, icons go, and the
-- color codes (|cAARRGGBB, |r, and |cnNAME: for unknown names) stay for the
-- companion to apply.
local function Markup(text)
    text = text:gsub("|cn([^:|]*):", NamedColor)
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

function ns.Hex(r, g, b)
    local function Byte(v)
        v = type(v) == "number" and v or 1
        return math.floor(math.max(0, math.min(1, v)) * 255 + 0.5)
    end
    return string.format("%02x%02x%02x", Byte(r), Byte(g), Byte(b))
end

-- One line for the overlay: "<1 or 0>\t<prefix>\t<body>\t<rrggbb>". Prefix and
-- body are markup and together make the line as shown in chat, in the given
-- color. The leading 1 asks for the body to be translated; with 0 the line is
-- shown as it is. A body too long for the strip is cut, so the color after it
-- always arrives.
local function Send(translate, prefix, body, color)
    local room = ns.STRIP.maxText - (#prefix + #color + 4)
    local flag = translate and "1" or "0"
    ns.SendToStrip(flag .. "\t" .. prefix .. "\t" .. ns.Truncate(body, math.max(room, 0)) .. "\t" .. color)
end

ns.Markup, ns.Plain, ns.MarkupIndex = Markup, Plain, MarkupIndex

-- The chat lines already handled, by the game's lineID: the message's plain
-- text if it has Chinese, false if not. Every chat window reports a line, so
-- this is how a line is sent once, and how Mirror knows where the message
-- starts in the line a window words around it. Forgets the oldest beyond KEEP.
local KEEP = 300
local handled, order, first = {}, {}, 1

local function Remember(lineID, value)
    handled[lineID] = value
    order[#order + 1] = lineID
    if #order - first >= KEEP then
        handled[order[first]] = nil
        order[first] = nil
        first = first + 1
    end
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
    return ns.Hex(info.r, info.g, info.b), info
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

-- Takes a chat event's line once, and sends it to the pixel strip unless a
-- chat window is mirrored: then Mirror sends what that window shows, and the
-- line is only remembered for it. Lines without Chinese are sent only with
-- allLines on, so the overlay can show the whole conversation in order.
-- Lines that cannot be read (secret values) are left alone.
-- Arguments follow the CHAT_MSG_* payload: the 4th is the channel's full
-- name, the 8th its number, the 11th the lineID, the 12th the sender's GUID.
local function Capture(event, msg, author, _, channel, _, _, _, channelIndex, _, _, lineID, guid)
    if ns.IsSecret(msg) or ns.IsSecret(author) or ns.IsSecret(channel) or ns.IsSecret(lineID)
        or ns.IsSecret(channelIndex) or ns.IsSecret(guid) then
        return
    end
    if type(msg) ~= "string" then return end
    if lineID ~= nil and handled[lineID] ~= nil then return end
    local chinese = HasChinese(msg)
    if lineID ~= nil then Remember(lineID, chinese and Plain(Markup(msg))) end
    if MirroredWindow() or not (chinese or ns.db.allLines) then return end

    local name = type(author) == "string" and author ~= "" and Ambiguate(author, "short") or nil
    local color, info = LineColor(event, channelIndex)
    local around = AROUND_NAME[event]
    local prefix = ""
    if event == "CHAT_MSG_CHANNEL" and name then
        local label = type(channel) == "string" and channel ~= "" and ("[" .. channel .. "] ") or ""
        prefix = label .. "[" .. NameMarkup(name, guid, info) .. "]: "
    elseif around and name then
        prefix = around[1] .. NameMarkup(name, guid, info) .. around[2]
    end
    Send(chinese, Markup(prefix), Markup(msg), color)
end

-- Copies a line the mirrored chat window just added, so the overlay shows
-- what that window shows: every kind of line, worded and colored the same,
-- including what other addons print. Only the Chinese part after the
-- speaker is translated.
local function Mirror(frame, text, r, g, b)
    if not ns.db or not ns.db.strip or frame ~= MirroredWindow() then return end
    if ns.IsSecret(text) or ns.IsSecret(r) or ns.IsSecret(g) or ns.IsSecret(b) then return end
    if type(text) ~= "string" then return end

    -- The lineID in the speaker's link names the line Capture remembered.
    local lineID = tonumber(text:match("|Hplayer:[^:|]*:(%d+)"))
    local message = lineID and handled[lineID]

    local markup = Markup(text)
    local plain = Plain(markup)
    local color = ns.Hex(r, g, b)
    local prefix, body = "", markup
    if HasChinese(plain) then
        -- The message starts where its remembered text is found, or failing
        -- that (icons, other addons' edits) after the first ": ".
        local at = message and plain:find(message, 1, true)
        if not at then
            local _, last = plain:find(": ", 1, true)
            at = last and last + 1
        end
        if at and at > 1 then
            local cut = MarkupIndex(markup, at - 1)
            prefix, body = markup:sub(1, cut - 1), markup:sub(cut)
        end
    end
    if HasChinese(Plain(body)) then
        Send(true, prefix, body, color)
    elseif ns.db.allLines then
        -- Nothing to translate, or only the speaker's name is Chinese.
        Send(false, "", markup, color)
    end
end

-- A made-up Chinese line for the overlay, as if someone had said it.
function ns.SendTestLine(text)
    Send(true, "[Test]: ", text, "ffffff")
end

-- Runs Mirror for tests, and behind the hook where an error must not reach the chat window.
function ns.MirrorLine(frame, text, r, g, b)
    local ok, problem = pcall(Mirror, frame, text, r, g, b)
    if not ok then ns.mirrorSource = "failed: " .. tostring(problem) end
end

-- Runs before a chat window words and adds the line, so Mirror finds it remembered.
local function Filter(_, event, msg, author, ...)
    Capture(event, msg, author, ...)
    return false
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
        ns.Print("No chat filter API found. A copied chat window may translate the speaker's name with the line.")
    else
        for _, event in ipairs(CHAT_EVENTS) do addFilter(event, Filter) end
    end

    -- Also capture lines on channels no chat window shows. Capture skips
    -- lines the filter already took.
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
end
