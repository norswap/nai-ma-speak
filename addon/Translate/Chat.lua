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

local LABELS = {
    CHAT_MSG_SAY = "Say", CHAT_MSG_YELL = "Yell", CHAT_MSG_WHISPER = "Whisper",
    CHAT_MSG_PARTY = "Party", CHAT_MSG_PARTY_LEADER = "Party",
    CHAT_MSG_RAID = "Raid", CHAT_MSG_RAID_LEADER = "Raid", CHAT_MSG_RAID_WARNING = "Raid Warning",
    CHAT_MSG_INSTANCE_CHAT = "Instance", CHAT_MSG_INSTANCE_CHAT_LEADER = "Instance",
    CHAT_MSG_GUILD = "Guild", CHAT_MSG_OFFICER = "Officer",
}

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

-- Whether a chat window is set up to show a message, going by the message
-- types and channels ticked in that window's own settings. This mirrors how
-- the game picks the windows a line appears in.
function ns.WindowShows(frame, event, zoneChannel, baseName)
    if event == "CHAT_MSG_CHANNEL" then
        for index, value in pairs(frame.channelList or {}) do
            local sameZone = type(zoneChannel) == "number" and zoneChannel > 0
                and frame.zoneChannelList and frame.zoneChannelList[index] == zoneChannel
            if sameZone or (type(baseName) == "string" and strupper(value) == strupper(baseName)) then
                return true
            end
        end
        return false
    end
    for _, group in pairs(frame.messageTypeList or {}) do
        if "CHAT_MSG_" .. group == event then return true end
        for _, groupEvent in pairs(ChatTypeGroup and ChatTypeGroup[group] or {}) do
            if groupEvent == event then return true end
        end
    end
    return false
end

-- Whether a line goes to the overlay: everything, or with a source window
-- chosen in the options, only what that window is set up to show. The window
-- need not be visible; it only lends its settings. If it no longer exists,
-- everything is sent.
local function FromSource(event, zoneChannel, baseName)
    local wanted = ns.db.sourceWindow
    if type(wanted) ~= "string" or wanted == "" then return true end
    local frame = WindowNamed(wanted)
    if not frame then return true end
    return ns.WindowShows(frame, event, zoneChannel, baseName)
end

-- Stores a Chinese line once and sends it to the pixel strip. Returns its id,
-- or nil when the line is not Chinese or cannot be read (secret values).
-- With allLines on, other lines also go to the strip, with id 0, so the
-- overlay can show the whole conversation in order; they are not stored.
-- Chinese lines outside the source window are stored, so their [T] link
-- works, but not sent.
-- Arguments follow the CHAT_MSG_* payload: the 7th is the zone channel id,
-- the 9th the channel's name without its number, the 11th the lineID.
local function Capture(event, msg, author, _, channel, _, _, zoneChannel, _, baseName, _, lineID)
    if ns.IsSecret(msg) or ns.IsSecret(author) or ns.IsSecret(channel) or ns.IsSecret(lineID)
        or ns.IsSecret(zoneChannel) or ns.IsSecret(baseName) then
        return nil
    end
    if type(msg) ~= "string" then return nil end
    local chinese = HasChinese(msg)
    if not chinese and not ns.db.allLines then return nil end
    if chinese and lineID and byLine[lineID] then return byLine[lineID] end
    local wanted = FromSource(event, zoneChannel, baseName)
    if not chinese and (not wanted or SeenPlain(lineID)) then return nil end

    local name = type(author) == "string" and Ambiguate(author, "short") or "?"
    local label = LABELS[event]
    if event == "CHAT_MSG_CHANNEL" and type(channel) == "string" and channel ~= "" then
        label = channel
    end
    local speaker = label and ("[" .. label .. "] " .. name) or name
    if not chinese then
        ns.SendToStrip("0\t" .. speaker .. "\t" .. StripCodes(msg))
        return nil
    end
    local id = Store(msg, name, lineID)
    if wanted then
        ns.SendToStrip(id .. "\t" .. speaker .. "\t" .. ns.messages[id].text)
    end
    return id
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

    -- Clicks on |Haddon:...| links arrive through EventRegistry on Mainline.
    if EventRegistry and EventRegistry.RegisterCallback then
        EventRegistry:RegisterCallback("SetItemRef", function(_, link) OnLinkClicked(link) end, ns)
        ns.linkSource = "EventRegistry SetItemRef"
    else
        ns.linkSource = "none (use /tr)"
    end
end
