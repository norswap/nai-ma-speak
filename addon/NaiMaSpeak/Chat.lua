local addonName, ns = ...

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

-- What this client calls the thing a link points to, by the kind of link and
-- the number in it. A link in chat shows the name in its sender's language; the
-- number is the same for everyone, so the game can give the name in ours. Nil
-- for a thing the game has not loaded yet: asking makes it load, and
-- `unnamed` then tells SendChat to try the line again shortly.
local unnamed = false
local OWN_NAME = {
    item = function(id)
        return C_Item and C_Item.GetItemNameByID and C_Item.GetItemNameByID(id) or GetItemInfo and (GetItemInfo(id))
    end,
    quest = function(id)
        local log = C_QuestLog or {}
        if log.RequestLoadQuestByID then log.RequestLoadQuestByID(id) end
        return log.GetTitleForQuestID and log.GetTitleForQuestID(id) or log.GetQuestInfo and (log.GetQuestInfo(id))
    end,
    spell = function(id)
        return C_Spell and C_Spell.GetSpellName and C_Spell.GetSpellName(id) or GetSpellInfo and (GetSpellInfo(id))
    end,
    achievement = function(id)
        return GetAchievementInfo and select(2, GetAchievementInfo(id))
    end,
}
OWN_NAME.enchant = OWN_NAME.spell

-- A link's text for the overlay: a Chinese name gives way to this client's
-- name for the thing, inside the brackets and after a quest's level
-- ("[[31] 名]"). The text stays as it is when the game has no name to give.
local function LinkText(kind, id, text)
    local lookup = OWN_NAME[kind]
    if not lookup or not HasChinese(text) then return text end
    local ok, name = pcall(lookup, tonumber(id))
    if not ok or not name or ns.IsSecret(name) or type(name) ~= "string" or name == "" then
        unnamed = true
        return text
    end
    return text:match("^[%[%]%d%s+]*") .. name .. text:match("[%]%s]*$")
end

-- The form of a word that goes with a number, of those the game is given as
-- "|4one:many;" ("|4one:few:many;" in Russian) after the number: "2 |4hour:hours;".
local function Counted(number, between, forms)
    local list = {}
    for form in forms:gmatch("[^:]+") do list[#list + 1] = form end
    local n = tonumber(number) or 0
    local form
    if #list < 3 then
        form = n == 1 and list[1] or list[2]
    elseif n % 10 == 1 and n % 100 ~= 11 then
        form = list[1]
    elseif n % 10 >= 2 and n % 10 <= 4 and (n % 100 < 12 or n % 100 > 14) then
        form = list[2]
    else
        form = list[3]
    end
    return number .. between .. (form or list[1] or "")
end

-- A line as the overlay draws it: links become their text, icons go, and the
-- color codes (|cAARRGGBB, |r, and |cnNAME: for unknown names) stay for the
-- companion to apply.
local function Markup(text)
    text = text:gsub("|cn([^:|]*):", NamedColor)
    text = text:gsub("|H(%a+):(%d+)[^|]*|h(.-)|h", LinkText)
    text = text:gsub("|H.-|h(.-)|h", "%1")
    text = text:gsub("|T.-|t", "")
    text = text:gsub("|A.-|a", "")
    -- Text the game draws but keeps from addons: a community channel's messages,
    -- Battle.net names.
    text = text:gsub("|K.-|k", "(hidden from addons)")
    text = text:gsub("(%d+)(%D-)|4([^;]*);", Counted)
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

-- One line for the overlay: "<kind>\t<prefix>\t<body>\t<rrggbb>\t<speaker>\t<number>".
-- Prefix and body are markup and together make the line as shown in chat, in
-- the given color; the speaker is the player's name as shown, or empty. Kind
-- 1 asks for the body to be translated; with 0 the line is shown as it is.
-- T, E and N are a line the player clicked a link on: to translate it again,
-- to explain it word by word, or to explain the speaker's name. They go ahead
-- of everything waiting, here and in the companion, and have no number. A
-- body too long for the strip is cut, so the fields after it always arrive.
local function Line(kind, prefix, body, color, speaker, number)
    local fields = { kind, prefix, body, color, speaker or "", number }
    local others = #table.concat(fields, "\t") - #body
    fields[3] = ns.Truncate(body, math.max(ns.STRIP.maxText - others, 0))
    return table.concat(fields, "\t")
end

-- Sends a line the player asked for by a link or a command.
local function Ask(kind, prefix, body, color, speaker)
    local line = Line(kind, prefix, body, color, speaker)
    ns.SendToStrip(line, line)
end

-- The number tells the lines of the chat apart and puts them in order. The
-- strip shows the last of them again and again while nothing new waits, for a
-- companion that was not looking the first time (the game was covered, or the
-- companion not running); it drops the numbers it has seen. Numbers start at
-- the time of login, so they keep rising from one login to the next.
local number = time() * 1000

local function NextNumber()
    number = number + 1
    return string.format("%.0f", number)
end

local NAME_TRIES = 5 -- how many times a line is built again for the names of its links

-- Sends a line of the chat. `build` returns its kind (0 or 1), prefix, body,
-- color and speaker, or nothing for a line that is not sent. Where the game
-- could not yet name a link, the line goes out with the sender's Chinese name
-- for it, which the companion translates word for word. The game loads the
-- name meanwhile, so the line is built again the next few times the strip
-- repeats it; the companion puts a line that reads differently under the same
-- number in the earlier one's place.
local function SendChat(build)
    unnamed = false
    local kind, prefix, body, color, speaker = build()
    if not kind then return end
    local own = NextNumber()
    local line = Line(kind, prefix, body, color, speaker, own)
    local tries = unnamed and NAME_TRIES or 0
    ns.SendToStrip(line)
    ns.RepeatOnStrip(function()
        if tries > 0 then
            unnamed = false
            local kind, prefix, body, color, speaker = build()
            if kind then line = Line(kind, prefix, body, color, speaker, own) end
            tries = unnamed and tries - 1 or 0
        end
        return line
    end)
end

ns.Markup, ns.Plain, ns.MarkupIndex = Markup, Plain, MarkupIndex

-- The chat lines already handled, by the game's lineID: the message's plain
-- text if it has Chinese, false if not. Every chat window reports a line, so
-- this is how a line is sent once, and how the message is found in the line a
-- window words around it. Forgets the oldest beyond KEEP.
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

-- The chat window with this name on its tab.
function ns.ChatWindowNamed(wanted)
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
    return ns.ChatWindowNamed(wanted)
end

-- A chat event's line for the overlay, worded as the game words it: its kind,
-- prefix, body, color and speaker, as SendChat's `build` returns them.
local function Worded(event, msg, author, channel, channelIndex, guid)
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
    return HasChinese(msg) and "1" or "0", Markup(prefix), Markup(msg), color, name
end

-- Chat the game keeps from addons for a time. During a boss fight, a Mythic+
-- run or a PvP match a chat event still comes, but its text and its speaker
-- cannot be read, and the chat windows' filters are not run for it. The line's
-- lineID is never kept back, and by it the game gives the text and the speaker
-- once the restriction is over. So such a line takes its place in the overlay
-- at once, saying that it is hidden, and is sent again under the same number
-- when it can be read, to be translated in that place.
local PLAYER_CHAT = {}
for _, event in ipairs(CHAT_EVENTS) do PLAYER_CHAT[event] = true end
local HIDDEN_FOR_NOW = "(hidden from addons for now; translated when the game allows it)"
local HIDDEN = "(hidden from addons)"
local HIDDEN_CHECK = 2 -- seconds between tries to read the waiting lines
local waiting = {} -- the hidden lines not yet read, oldest first
local watch -- the ticker that tries them, while any wait
local readBack, lost = 0, 0

-- A value unless the game keeps it from addons.
local function Known(value)
    if not ns.IsSecret(value) then return value end
end

-- Whether the game is keeping chat from addons now; nil where it cannot be asked.
local function ChatHidden()
    local ask = C_ChatInfo and C_ChatInfo.InChatMessagingLockdown
    if not ask then return nil end
    local ok, hidden = pcall(ask)
    return ok and Known(hidden) == true
end

-- What the game says of a chat line by its lineID, if it says it readably.
local function OfLine(ask, lineID)
    if not ask then return nil end
    local ok, value = pcall(ask, lineID)
    return ok and Known(value) or nil
end

-- Whether a chat window shows this kind of line, by its tab's settings: the
-- channels by name or zone, as the game matches them, the rest by their group.
local function Shows(window, event, zone, baseName)
    if event == "CHAT_MSG_CHANNEL" then
        for i, name in ipairs(window.channelList or {}) do
            local zoned = type(zone) == "number" and zone > 0 and (window.zoneChannelList or {})[i] == zone
            if zoned or (type(baseName) == "string" and name:upper() == baseName:upper()) then return true end
        end
        return false
    end
    if type(ChatTypeGroup) ~= "table" then return true end
    for _, group in ipairs(window.messageTypeList or {}) do
        for _, shown in ipairs(ChatTypeGroup[group] or {}) do
            if shown == event then return true end
        end
    end
    return false
end

-- A hidden line that will not be read after all keeps its place, and says so.
local function GiveUp(line)
    line.text = Line("0", line.label, HIDDEN, line.color, "", line.number)
    lost = lost + 1
    ns.SendToStrip(line.text)
end

-- Reads the waiting lines in order, as far as the game now gives them. A line
-- it does not give though chat is no longer kept back is one it has dropped.
local function Reveal()
    while waiting[1] do
        local line = waiting[1]
        local msg = OfLine(C_ChatInfo.GetChatLineText, line.lineID)
        if type(msg) == "string" and msg ~= "" then
            local author = OfLine(C_ChatInfo.GetChatLineSenderName, line.lineID)
            local guid = OfLine(C_ChatInfo.GetChatLineSenderGUID, line.lineID)
            local kind, prefix, body, color, speaker = Worded(line.event, msg, author, line.channel, line.channelIndex, guid)
            if handled[line.lineID] ~= nil then handled[line.lineID] = kind == "1" and Plain(body) end
            line.text = Line(kind, prefix, body, color, speaker, line.number)
            readBack = readBack + 1
            ns.SendToStrip(line.text)
        elseif ChatHidden() then
            return
        else
            GiveUp(line)
        end
        table.remove(waiting, 1)
    end
    if watch then
        watch:Cancel()
        watch = nil
    end
end

-- Runs Reveal behind the ticker, where an error would come again every time:
-- the line it stopped at is given up, and /tr /probe says what went wrong.
local hiddenProblem
local function TryReveal()
    local ok, problem = pcall(Reveal)
    if ok then return end
    hiddenProblem = tostring(problem)
    local line = table.remove(waiting, 1)
    if line then pcall(GiveUp, line) end
end

-- Gives a hidden line of player chat its place in the overlay, once. With a
-- chat window copied, only the lines that window shows.
local function Hide(event, zone, channel, channelIndex, baseName, lineID)
    if not PLAYER_CHAT[event] or type(lineID) ~= "number" or handled[lineID] ~= nil then return end
    local window = MirroredWindow()
    if window and not Shows(window, event, zone, baseName) then return end
    Remember(lineID, false)
    local label = AROUND_NAME[event] and (AROUND_NAME[event][1]:gsub("%[$", "")) or ""
    if event == "CHAT_MSG_CHANNEL" then
        label = type(channel) == "string" and channel ~= "" and ("[" .. channel .. "] ") or ""
    end
    local line = { lineID = lineID, event = event, channel = channel, channelIndex = channelIndex,
        label = Markup(label), color = (LineColor(event, channelIndex)), number = NextNumber() }
    if ChatHidden() ~= nil and C_ChatInfo.GetChatLineText then
        line.text = Line("0", line.label, HIDDEN_FOR_NOW, line.color, "", line.number)
        ns.SendToStrip(line.text)
        waiting[#waiting + 1] = line
        if #waiting > KEEP then GiveUp(table.remove(waiting, 1)) end
        watch = watch or C_Timer.NewTicker(HIDDEN_CHECK, TryReveal)
    else
        GiveUp(line)
    end
    ns.RepeatOnStrip(function() return line.text end)
end

-- How hidden chat has fared, for /tr /probe.
function ns.HiddenChat()
    local can = ChatHidden() ~= nil and C_ChatInfo.GetChatLineText ~= nil
    return "Hidden chat: " .. (can and "read back when the game allows it" or "cannot be read back on this client")
        .. "; " .. #waiting .. " waiting, " .. readBack .. " read back, " .. lost .. " not read"
        .. (hiddenProblem and ("; failed: " .. hiddenProblem) or "")
end

-- Takes a chat event's line once, and sends it to the pixel strip unless a
-- chat window is mirrored: then Mirror sends what that window shows, and the
-- line is only remembered for it. Lines without Chinese are sent only with
-- allLines on, so the overlay can show the whole conversation in order.
-- A line whose text or speaker the game keeps back goes to Hide. Returns the
-- line's lineID and whether its message has Chinese, for its links.
-- Arguments follow the CHAT_MSG_* payload: the 4th is the channel's full
-- name, the 7th its zone, the 8th its number, the 9th its name alone, the
-- 11th the lineID, the 12th the sender's GUID.
local function Capture(event, msg, author, _, channel, _, _, zone, channelIndex, baseName, _, lineID, guid)
    if ns.IsSecret(msg) or ns.IsSecret(author) or ns.IsSecret(guid) then
        -- An error here would come with every line said during the fight.
        local ok, problem = pcall(Hide, event, Known(zone), Known(channel), Known(channelIndex), Known(baseName), Known(lineID))
        if not ok then hiddenProblem = tostring(problem) end
        return
    end
    if ns.IsSecret(channel) or ns.IsSecret(lineID) or ns.IsSecret(channelIndex) then return end
    if type(msg) ~= "string" then return end
    if lineID ~= nil and handled[lineID] ~= nil then return lineID, handled[lineID] ~= false end
    local chinese = HasChinese(msg)
    if lineID ~= nil then Remember(lineID, chinese and Plain(Markup(msg))) end
    if MirroredWindow() or not (chinese or ns.db.allLines) then return lineID, chinese end

    SendChat(function() return Worded(event, msg, author, channel, channelIndex, guid) end)
    return lineID, chinese
end

-- Breaks a line, as a chat window words it, into the part before the message
-- and the message, both as markup, and finds the speaker's name as shown.
-- Without Chinese in the message there is nothing to translate: then the
-- first part is nil and the second the whole line.
local function Split(text)
    -- The speaker's link holds the name as shown, and the lineID of the
    -- message Capture remembered. The overlay has no use for our own links.
    local lineID, shown = text:match("|Hplayer:[^:|]*:(%d+)[^|]*|h(.-)|h")
    -- A community channel's link has no lineID.
    shown = shown or text:match("|HplayerCommunity:[^|]*|h(.-)|h")
    local speaker = shown and Plain(Markup(shown)):gsub("^%[(.*)%]$", "%1") or ""
    local message = lineID and handled[tonumber(lineID)]
    text = text:gsub(" *|Haddon:" .. addonName .. ":%a:%d+|h.-|h", "")

    local markup = Markup(text)
    local plain = Plain(markup)
    local prefix, body = "", markup
    if HasChinese(plain) then
        -- The message starts where its remembered text is found, or failing
        -- that (icons, other addons' edits, a line long past) after the first ": ".
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
    -- Without Chinese in the body, only the speaker's name is Chinese, or nothing is.
    if HasChinese(Plain(body)) then return prefix, body, speaker end
    return nil, markup, speaker
end

-- Sends a line as a chat window shows it.
local function SendShown(text, r, g, b)
    SendChat(function()
        local prefix, body, speaker = Split(text)
        if prefix then return "1", prefix, body, ns.Hex(r, g, b), speaker end
        if ns.db.allLines then return "0", "", body, ns.Hex(r, g, b), speaker end
    end)
end

local function Mirror(frame, text, r, g, b)
    if not ns.db or frame ~= MirroredWindow() then return end
    if ns.IsSecret(text) or ns.IsSecret(r) or ns.IsSecret(g) or ns.IsSecret(b) then return end
    if type(text) ~= "string" then return end
    SendShown(text, r, g, b)
end

-- The links after a line: [T] sends it to the overlay to be translated again
-- and [?] to be explained word by word, for a message with Chinese; [?N] has
-- the speaker's name explained, for a name with Chinese. A link names what it
-- asks for, as the kind of strip line to send, and the line.
local function Links(lineID, chineseMessage, chineseName)
    local function Link(kind, label)
        return "|Haddon:" .. addonName .. ":" .. kind .. ":" .. lineID .. "|h|cff66ccff" .. label .. "|r|h"
    end
    return (chineseMessage and (Link("T", "[T]") .. Link("E", "[?]")) or "") .. (chineseName and Link("N", "[?N]") or "")
end

-- Sends a line to the overlay when one of its links is clicked, read back from
-- the chat window it was clicked in, so any line still on show can be asked
-- for. The companion marks it as asked for and handles it first.
local function SendAgain(kind, lineID, clicked)
    local link = "|Haddon:" .. addonName .. ":%a:" .. lineID .. "|h"
    local windows = { clicked }
    for i = 1, NUM_CHAT_WINDOWS or 10 do windows[#windows + 1] = _G["ChatFrame" .. i] end
    for _, window in ipairs(windows) do
        local count = window.GetNumMessages and window:GetNumMessages()
        for i = 1, type(count) == "number" and count or 0 do
            local text, r, g, b = window:GetMessageInfo(i)
            if type(text) == "string" and not ns.IsSecret(text) and text:find(link) then
                local prefix, body, speaker = Split(text)
                if kind == "N" then
                    -- Only the name matters; the companion words the line itself.
                    Ask(kind, "", "", ns.Hex(r, g, b), speaker)
                elseif prefix then
                    Ask(kind, prefix, body, ns.Hex(r, g, b), speaker)
                end
                return
            end
        end
    end
    ns.Print("That line is no longer in a chat window, so it cannot be sent again.")
end

-- Asks the companion to explain a player's name, as a [?N] link does.
function ns.ExplainName(name)
    Ask("N", "", "", "ffffff", name)
end

-- A made-up Chinese line for the overlay, as if someone had said it.
function ns.SendTestLine(text)
    SendChat(function() return "1", "[Test]: ", text, "ffffff", "Test" end)
end

-- A made-up conversation, for trying the overlay or picturing it when chat is
-- quiet: event, channel name and number, speaker, class, message.
local DEMO = {
    { "CHAT_MSG_CHANNEL", "1. General - Undercity", 1, "奶茶 半糖", "PRIEST", "血色修道院 教堂 缺坦缺DD 有補 來的密我" },
    { "CHAT_MSG_CHANNEL", "2. Trade - City", 2, "老王 打鐵", "WARRIOR",
        "收 |cffffffff|Hitem:4338::::::::60:::::|h[魔紋布]|h|r 一組50銀 有多少收多少" },
    { "CHAT_MSG_WHISPER", nil, nil, "奶茶 半糖", "PRIEST", "你是術士嗎? 血色教堂缺DD 要不要來" },
    { "CHAT_MSG_PARTY_LEADER", nil, nil, "奶茶 半糖", "PRIEST", "人齊了 門口集合 大家先修裝" },
    { "CHAT_MSG_PARTY", nil, nil, "阿肥 不怕痛", "WARRIOR", "我坦 第一次來 路不熟 請多包涵" },
    { "CHAT_MSG_PARTY", nil, nil, "月光 小賊", "ROGUE", "沒關係 跟著我走 王的打法我會先講" },
    { "CHAT_MSG_PARTY", nil, nil, "熊貓 不吃竹", "MAGE", "開怪前等我變羊 不要打羊喔" },
    { "CHAT_MSG_GUILD", nil, nil, "Mira Oakheart", "DRUID", "anyone up for Uldaman later tonight?" },
    { "CHAT_MSG_SAY", nil, nil, "小魚 愛睡覺", "HUNTER", "有人要一起解血色十字軍的任務嗎" },
    { "CHAT_MSG_PARTY", nil, nil, "阿肥 不怕痛", "WARRIOR", "滅團了 抱歉 我一次拉太多" },
    { "CHAT_MSG_PARTY_LEADER", nil, nil, "奶茶 半糖", "PRIEST", "沒事 跑屍回來 這次慢慢拉" },
    { "CHAT_MSG_WHISPER", nil, nil, "月光 小賊", "ROGUE", "你打得不錯 下次再一起 加個好友吧" },
}
local DEMO_GAP = 1.5 -- seconds between its lines
local demoID = 2 ^ 30 -- its lineIDs, clear of the game's

-- Plays the conversation: each line is added to the copied chat window, or the
-- main one, worded and colored as the game would, and reaches the overlay the
-- way real chat does. Nothing is said in chat; only this player sees it.
function ns.Demo()
    for i, line in ipairs(DEMO) do
        C_Timer.After((i - 1) * DEMO_GAP, function()
            local event, channel, index, name, class, msg = unpack(line, 1, 6)
            demoID = demoID + 1
            local chinese = HasChinese(msg)
            Remember(demoID, chinese and Plain(Markup(msg)))
            local _, info = LineColor(event, index)
            local r, g, b = info and info.r or 1, info and info.g or 1, info and info.b or 1
            local color = RAID_CLASS_COLORS and RAID_CLASS_COLORS[class]
            local shown = color and color.colorStr and ("|c" .. color.colorStr .. name .. "|r") or name
            local around = AROUND_NAME[event] or { "[", "]: " }
            local stamp = GetChatTimestampFormat and GetChatTimestampFormat() or CHAT_TIMESTAMP_FORMAT
            local text = (type(stamp) == "string" and date(stamp) or "") .. (channel and ("[" .. channel .. "] ") or "")
                .. around[1] .. "|Hplayer:" .. name .. ":" .. demoID .. ":" .. event:sub(10) .. "|h" .. shown .. "|h"
                .. around[2] .. msg
            if ns.db.showLinks and (chinese or HasChinese(name)) then
                text = text .. " " .. Links(demoID, chinese, HasChinese(name))
            end
            local window = MirroredWindow()
            local target = window or DEFAULT_CHAT_FRAME
            target:AddMessage(text, r, g, b)
            -- A copied window's line is sent by its hook.
            if not window then SendShown(text, r, g, b) end
        end)
    end
end

-- Runs Mirror for tests, and behind the hook where an error must not reach the chat window.
function ns.MirrorLine(frame, text, r, g, b)
    local ok, problem = pcall(Mirror, frame, text, r, g, b)
    if not ok then ns.mirrorSource = "failed: " .. tostring(problem) end
end

-- Runs before a chat window words and adds the line, so Mirror finds it
-- remembered. Adds the links to a line with Chinese in its message or in its
-- speaker's name.
local function Filter(_, event, msg, author, ...)
    local lineID, chinese = Capture(event, msg, author, ...)
    local named = type(author) == "string" and not ns.IsSecret(author) and HasChinese(author)
    if not lineID or not ns.db.showLinks or not (chinese or named) then return false end
    return false, msg .. " " .. Links(lineID, chinese, named), author, ...
end

-- A community channel's line: the game keeps its message from addons, so only
-- the speaker's name can be asked about. The 11th argument is the lineID.
local function CommunityFilter(_, _, msg, author, ...)
    local lineID = select(9, ...)
    if ns.IsSecret(msg) or ns.IsSecret(author) or ns.IsSecret(lineID) then return false end
    if not ns.db.showLinks or type(msg) ~= "string" or type(lineID) ~= "number" then return false end
    if type(author) ~= "string" or not HasChinese(author) then return false end
    return false, msg .. " " .. Links(lineID, false, true), author, ...
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
        addFilter("CHAT_MSG_COMMUNITIES_CHANNEL", CommunityFilter)
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

    -- Clicks on |Haddon:...| links arrive through EventRegistry.
    if EventRegistry and EventRegistry.RegisterCallback then
        EventRegistry:RegisterCallback("SetItemRef", function(_, link, _, _, window)
            local name, kind, lineID = tostring(link):match("^addon:([^:]+):([TEN]):(%d+)")
            if name == addonName then SendAgain(kind, lineID, type(window) == "table" and window or nil) end
        end, ns)
        ns.linkSource = "EventRegistry SetItemRef"
    else
        ns.linkSource = "none: the links after chat lines do nothing on this client"
    end
end
