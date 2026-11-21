-- Tests for the addon, run outside the game with Lua 5.1:
--     lua5.1 tests/addon_test.lua        (from the repository root)
-- The game's functions the addon calls are replaced by small stand-ins, and what the
-- addon would draw on the pixel strip is collected as text.

local ADDON = "addon/NaiMaSpeak/"

-- An object whose every method does nothing and returns another such object, standing
-- in for frames, textures and buttons.
local function Dummy()
    return setmetatable({}, { __index = function() return function() return Dummy() end end })
end

-- A chat window: AddMessage and HookScript are real enough to hook, the rest is a dummy.
local function ChatWindow()
    local window = Dummy()
    window.added, window.scripts, window.isDocked = {}, {}, true
    window.AddMessage = function(self, text, r, g, b) self.added[#self.added + 1] = { text, r, g, b } end
    window.GetNumMessages = function(self) return #self.added end
    window.GetMessageInfo = function(self, i) return unpack(self.added[i]) end
    window.Clear = function(self) self.added = {} end
    window.messageTypeList, window.channelList = {}, {}
    window.HookScript = function(self, script, fn) self.scripts[script] = fn end
    return window
end

local printed, said, opened, timers = {}, {}, {}, {}
strlower, strtrim = string.lower, function(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end
function CreateFrame() return Dummy() end
UIParent = Dummy()
DEFAULT_CHAT_FRAME = { AddMessage = function(_, text) printed[#printed + 1] = text end }
SlashCmdList = {}
StaticPopupDialogs = {}
local popup
function StaticPopup_Show(which, _, _, data) popup = { dialog = StaticPopupDialogs[which], data = data } end
C_Timer = { After = function(_, fn) timers[#timers + 1] = fn end, NewTicker = function() return Dummy() end }
function time() return 1700000000 end
function GetPhysicalScreenSize() return 1920, 1080 end
local fullscreen = false
function GetCVarBool(name) return name ~= "gxMaximize" or fullscreen end
function GetAddOnMetadata() return "0.0.1" end
function hooksecurefunc(object, name, hook)
    local original = object[name]
    object[name] = function(...) original(...); hook(...) end
end
function Ambiguate(name) return name end
function SendChatMessage(text, kind) said[#said + 1] = kind .. " " .. text end
function ChatFrame_OpenChat(text) opened[#opened + 1] = text end
local filters = {}
function ChatFrame_AddMessageEventFilter(event, fn) filters[event] = fn end
NUM_CHAT_WINDOWS = 2
ChatFrame1, ChatFrame2 = ChatWindow(), ChatWindow()
local tabNames = { "General", "Pick" }
function FCF_GetChatWindowInfo(i) return tabNames[i], 14 end
ChatTypeInfo = {
    SAY = { r = 1, g = 1, b = 1 }, GUILD = { r = 0.25, g = 1, b = 0.25, colorNameByClass = true },
    SYSTEM = { r = 1, g = 1, b = 0 }, CHANNEL = { r = 1, g = 0.75, b = 0.75 }, CHANNEL2 = { r = 0, g = 0.5, b = 1 },
}
RAID_CLASS_COLORS = { MAGE = { colorStr = "ff3fc7eb" } }
local itemNames = { [4338] = "Mageweave Cloth" }
function GetItemInfo(id) return itemNames[id] end
C_QuestLog = { GetTitleForQuestID = function(id) return id == 681 and "Stromgarde Badges" or nil end }
function GetPlayerInfoByGUID() return "Mage", "MAGE" end
RED_FONT_COLOR = { r = 1, g = 0.125, b = 0.125 }
local clickLink
EventRegistry = {
    RegisterCallback = function(_, _, fn) clickLink = function(link, window) fn(nil, link, "[T]", "LeftButton", window) end end,
}

-- Load the addon as the game does, in the order of NaiMaSpeak.toc, and log in.
local ns = {}
for _, file in ipairs({ "Core.lua", "Strip.lua", "Chat.lua", "Layout.lua", "Tabs.lua", "Buttons.lua", "Minimap.lua", "Options.lua" }) do
    assert(loadfile(ADDON .. file))("NaiMaSpeak", ns)
end
ns.handlers.ADDON_LOADED("NaiMaSpeak")
ns.handlers.PLAYER_LOGIN()

-- The strip shows one frame at a time; `Frames` plays out the waiting ones and returns
-- the text of each frame shown since the last call, tabs written as " | ". The numbers
-- that end chat lines are left out, and no line is shown again, except where that is
-- what is tested.
ns.db.repeatLines = 0
local frames, numbered = {}, false
local encode = ns.EncodeFrame
ns.EncodeFrame = function(seq, text)
    if not numbered then text = (text .. "\n"):gsub("\t17%d+\n", "\n"):sub(1, -2) end
    frames[#frames + 1] = text:gsub("\t", " | ")
    return encode(seq, text)
end
local function Frames()
    while #timers > 0 do table.remove(timers, 1)() end
    local shown = frames
    frames = {}
    return shown
end
Frames() -- what was sent at login

local failures = 0
local function Check(name, got, want)
    if type(got) == "table" then got = table.concat(got, "\n") end
    if type(want) == "table" then want = table.concat(want, "\n") end
    if got ~= want then
        failures = failures + 1
        print("FAILED: " .. name .. "\n  got:  " .. tostring(got):gsub("\n", "\n        ") .. "\n  want: " .. tostring(want):gsub("\n", "\n        "))
    end
end

-- A chat event as the game delivers it: through every chat window's filter, then to
-- the addon's own handler. Returns the lineID and the message as the filter left it.
local lineID = 0
local function Chat(event, text, channel, channelIndex)
    lineID = lineID + 1
    local args = { text, "Bob", "", channel or "", "", "", 0, channelIndex or 0, "", 0, lineID, "guid-1" }
    filters[event](nil, event, unpack(args))
    local _, shown = filters[event](nil, event, unpack(args))
    ns.handlers[event](unpack(args))
    return lineID, shown or text
end

function GetUnitName(unit, whole) return unit == "player" and whole and "Nors Darkraven" or nil end
ns.SayVersion()
Check("the version is told, with the one the game read at its start and the character", Frames(),
    { "V | " .. ns.VERSION .. " | 0.0.1 | Nors Darkraven" })

-- Chat events, with no chat window copied ------------------------------------------

Chat("CHAT_MSG_SAY", "hello |cffff0000red|r")
Chat("CHAT_MSG_GUILD", "副本缺補")
Chat("CHAT_MSG_CHANNEL", "wts |cffa335ee|Hitem:1|h[Sword]|h|r |TInterface\\icon:0|t", "2. Trade - City", 2)
ns.handlers.CHAT_MSG_SYSTEM("Bob has come online.", "")
ns.handlers.CHAT_MSG_MONSTER_YELL("You will die!", "Ragnaros")
Check("each event's line is sent once, worded and colored as the game does", Frames(), {
    "0 | [Bob] says:  | hello |cffff0000red|r | ffffff | Bob",
    "1 | [Guild] [|cff3fc7ebBob|r]:  | 副本缺補 | 40ff40 | Bob\n"
        .. "0 | [2. Trade - City] [Bob]:  | wts |cffa335ee[Sword]|r  | 0080ff | Bob\n"
        .. "0 |  | Bob has come online. | ffff00 | \n"
        .. "0 | Ragnaros yells:  | You will die! | ffffff | Ragnaros",
})

NaiMaSpeakDB.allLines = false
Chat("CHAT_MSG_SAY", "plain")
Chat("CHAT_MSG_SAY", "缺坦")
Check("without allLines only Chinese lines are sent", Frames(), { "1 | [Bob] says:  | 缺坦 | ffffff | Bob" })
NaiMaSpeakDB.allLines = true

-- A copied chat window ----------------------------------------------------------------

-- [T] links ------------------------------------------------------------------------------

local asked, withLink = Chat("CHAT_MSG_GUILD", "缺補")
local _, plainLine = Chat("CHAT_MSG_SAY", "no link here")
Check("a Chinese line gets its two links", withLink,
    "缺補 |Haddon:NaiMaSpeak:T:" .. asked .. "|h|cff66ccff[T]|r|h|Haddon:NaiMaSpeak:E:" .. asked .. "|h|cff66ccff[?]|r|h")
Check("other lines get none", plainLine, "no link here")
Frames()
-- The line as a chat window shows it, long after the addon forgot the message itself.
ChatFrame1:AddMessage("|Hplayer:Bob:424242:GUILD|h[|cff3fc7ebBob|r]|h: " .. withLink, 0.25, 1, 0.25)
ChatFrame1:AddMessage("|Hplayer:小明:424243:SAY|h[|cff3fc7eb小明|r]|h says: hi |Haddon:NaiMaSpeak:N:77|h[?N]|h", 1, 1, 1)
ns.SendToStrip("0\t\twaiting 1")
ns.SendToStrip("0\t\twaiting 2")
clickLink("addon:NaiMaSpeak:T:" .. asked, ChatFrame1)
clickLink("addon:NaiMaSpeak:E:" .. asked, ChatFrame1)
clickLink("addon:Other:T:" .. asked, ChatFrame1)
clickLink("item:1", ChatFrame1)
clickLink("addon:NaiMaSpeak:N:77")
Check("a click sends the line, read from the window, ahead of what is waiting", Frames(), {
    "0 |  | waiting 1",
    "N |  |  | ffffff | 小明\nE | [|cff3fc7ebBob|r]:  | 缺補 | 40ff40 | Bob\nT | [|cff3fc7ebBob|r]:  | 缺補 | 40ff40 | Bob\n0 |  | waiting 2",
})
local _, named = filters.CHAT_MSG_SAY(nil, "CHAT_MSG_SAY", "hello", "小明", "", "", "", "", 0, 0, "", 0, 555555, "guid-2")
Check("an English line from a Chinese name gets only the name's link", named, "hello |Haddon:NaiMaSpeak:N:555555|h|cff66ccff[?N]|r|h")
local _, hidden = filters.CHAT_MSG_COMMUNITIES_CHANNEL(nil, "CHAT_MSG_COMMUNITIES_CHANNEL", "|Kx37|k", "小明", "", "", "", "", 0, 8, "", 0, 555556, "guid-2")
Check("a community channel's line, its message hidden, gets the name's link too", hidden, "|Kx37|k |Haddon:NaiMaSpeak:N:555556|h|cff66ccff[?N]|r|h")
Frames()
ChatFrame1:AddMessage("|HplayerCommunity:小明:57:1:17:15|h[小明]|h: " .. hidden, 1, 1, 1)
clickLink("addon:NaiMaSpeak:N:555556", ChatFrame1)
Check("and a click on it asks about the name", Frames(), { "N |  |  | ffffff | 小明" })
Frames()
printed = {}
clickLink("addon:NaiMaSpeak:T:99999", ChatFrame1)
Check("a line no window shows any more says so", #printed, 1)
NaiMaSpeakDB.showLinks = false
local _, unlinked = Chat("CHAT_MSG_GUILD", "缺坦")
Check("no links with the option off", unlinked, "缺坦")
NaiMaSpeakDB.showLinks = true
Frames()

-- A copied chat window ----------------------------------------------------------------

NaiMaSpeakDB.sourceWindow = "Pick"
local id, linked = Chat("CHAT_MSG_GUILD", "來打副本 |cffa335ee|Hitem:1|h[劍]|h|r")
Check("events send nothing while a window is copied", Frames(), {})

local function Shown(text, r, g, b) ChatFrame2:AddMessage(text, r or 1, g or 1, b or 1) end
Shown("|Hchannel:GUILD|h[Guild]|h |Hplayer:Bob:" .. id .. ":GUILD|h[|cff3fc7ebBob|r]|h: " .. linked, 0.25, 1, 0.25)
ChatFrame1:AddMessage("not the copied window", 1, 1, 1)
Shown("|cff33ff99DBM|r: Pull in |cffffff005|r")
Shown("12:30 |Hplayer:小明:999:SAY|h[小明]|h says: hello there")
Shown("12:31 |Hchannel:channel:8|h[8. Global]|h |HplayerCommunity:小明:57:1:1795077845730000:156503|h[|cffc69b6d小明|r]|h: |Kx37|k")
Shown("|Hplayer:小明:998:SAY|h[小明]|h says: {star} 缺坦")
Shown("你獲得了物品", 1, 1, 0)
Shown("|cnRED_FONT_COLOR:Low health|r and |cnNO_SUCH_COLOR:this|r")
Check("the copied window's lines, split where the message starts", Frames(), {
    "1 | [Guild] [|cff3fc7ebBob|r]:  | 來打副本 |cffa335ee[劍]|r | 40ff40 | Bob",
    "0 |  | |cff33ff99DBM|r: Pull in |cffffff005|r | ffffff | \n"
        .. "0 |  | 12:30 [小明] says: hello there | ffffff | 小明\n"
        .. "0 |  | 12:31 [8. Global] [|cffc69b6d小明|r]: (hidden from addons) | ffffff | 小明\n"
        .. "1 | [小明] says:  | {star} 缺坦 | ffffff | 小明\n"
        .. "1 |  | 你獲得了物品 | ffff00 | \n"
        .. "0 |  | |cffff2020Low health|r and |cnNO_SUCH_COLOR:this|r | ffffff | ",
})

Shown(string.rep("長", 200))
local long = Frames()[1]
Check("a line too long for a frame keeps its color at the end", long:sub(-12), " | ffffff | ")
Check("and fits the frame", #long <= ns.STRIP.maxText + 8, true) -- " | " is 2 longer than a tab, 4 times
NaiMaSpeakDB.sourceWindow = ""

-- Markup helpers -----------------------------------------------------------------------

Check("Plain drops color codes only", ns.Plain("a|cff112233b|cnRED_FONT_COLOR:c|r|rd|x"), "abcd|x")
Check("MarkupIndex keeps codes at the cut with the part before", ns.MarkupIndex("|cff112233ab|rcd", 2), 15)

-- Links ---------------------------------------------------------------------------------

Chat("CHAT_MSG_SAY", "收|cffffffff|Hitem:4338::::::::60:::::|h[魔紋布]|h|r 和 |cffffff00|Hquest:681:37|h[[37] 激流堡符印]|h|r")
Chat("CHAT_MSG_SAY", "|cffffffff|Hitem:9999::::|h[不明之物]|h|r")
Check("a link is named as this client names the thing, where the game knows it", Frames(), {
    "1 | [Bob] says:  | 收|cffffffff[Mageweave Cloth]|r 和 |cffffff00[[37] Stromgarde Badges]|r | ffffff | Bob",
    "1 | [Bob] says:  | |cffffffff[不明之物]|r | ffffff | Bob",
})

-- A link the game names a moment later: the line reads differently when the strip
-- repeats it, under the same number.
numbered, ns.db.repeatLines = true, 1
Chat("CHAT_MSG_SAY", "|cffffffff|Hitem:7777::::|h[符文布]|h|r")
itemNames[7777] = "Runecloth"
table.remove(timers, 1)()
Check("the line goes out as it is, and is repeated with the name once the game has it",
    { frames[1]:match("(%[[^%[]-%])|r"), frames[2]:match("(%[[^%[]-%])|r"), tostring(frames[1]:match("%d+$") == frames[2]:match("%d+$")) },
    { "[符文布]", "[Runecloth]", "true" })
numbered, ns.db.repeatLines, frames = false, 0, {}
Frames()

-- The strip's queue ----------------------------------------------------------------------

ns.SendToStrip("0\t\tfirst")
ns.SendToStrip("0\t\tsecond")
ChatFrame2.scripts.OnMouseWheel(nil, 1)
ChatFrame2.scripts.OnMouseWheel(nil, 1)
ChatFrame1.scripts.OnMouseWheel(nil, -1)
ns.SendToStrip("L\tolder", "L\t")
ns.SendToStrip("L\tnewer", "L\t")
Check("state jumps the queue and replaces older state of its kind", Frames(), {
    "0 |  | first",
    "L | newer\nS | General | -1\nS | Pick | 2\n0 |  | second",
})

-- Chat lines are numbered, and the last of them are shown again while nothing waits.
numbered, ns.db.repeatLines = true, 2
for _, text in ipairs({ "one", "two", "three" }) do Chat("CHAT_MSG_SAY", text) end
table.remove(timers, 1)()
local first = table.concat(frames, "\n")
local one, two, three = first:match("one.- | (%d+)\n.-two.- | (%d+)\n.-three.- | (%d+)$")
Check("chat lines end in numbers that start at the time of login and rise",
    { one:sub(1, 10), two - one, three - two }, { "1700000000", 1, 1 })
frames = {}
table.remove(timers, 1)()
table.remove(timers, 1)()
Check("the last lines are shown again, and again", { frames[1], frames[2] },
    { first:match("\n(.*)$"), first:match("\n(.*)$") })
ns.SendToStrip("0\t\tnew")
table.remove(timers, 1)()
Check("a new line goes first", frames[3], "0 |  | new")
numbered, ns.db.repeatLines, frames = false, 0, {}
Frames()

local symbols = encode(7, "hi")
Check("a frame fills the strip", #symbols, ns.STRIP.cols * ns.STRIP.rows)
Check("and starts with the sync blocks", table.concat({ symbols[1], symbols[2], symbols[3] }, ","), "51,15,60")

-- Buttons and slash commands -------------------------------------------------------------

local slash = SlashCmdList.NAIMASPEAK
slash("")
slash("/g sure, I can heal")
slash("/w Ann on my way")
slash("help me please")
Check("replies and the button go to the companion", Frames(), {
    "C | reply",
    "R | help me please\nR | on my way\nR | sure, I can heal",
})
Check("and the chat box reopens on the target", opened, { "/g ", "/w Ann ", "" })

printed = {}
slash("/g")
slash("/w Ann")
Check("a target with nothing to say is refused", #printed, 2)
Check("nothing was sent for it", Frames(), {})

slash("/test 2")
Check("counted test lines go to the overlay", Frames(), { "1 | [Test]:  | 一 (Translate test) | ffffff | Test\n1 | [Test]:  | 二 (Translate test) | ffffff | Test" })
Check("and are not said in chat", #said, 0)
slash("/TEST")
Check("the plain test line is said", said[1], "SAY 大家好，今天一起打副本嗎？ (Translate test)")
Frames()

local target = "小明"
function UnitName(unit) return unit == "target" and target or nil end
slash("/name")
slash("/name 黑熊 Lz")
target = nil
printed = {}
slash("/NAME")
Check("a name to explain: the target's, or one typed", Frames(), { "N |  |  | ffffff | 小明", "N |  |  | ffffff | 黑熊 Lz" })
Check("with neither, it asks whose", #printed, 1)

-- A tab kept empty ---------------------------------------------------------------

local function Undo(name) ns.db.emptyWindow = name end
local function Settle() while #timers > 0 do table.remove(timers, 1)() end end
-- Stepping through the options to "Pick" passes "General", which shows chat.
ChatFrame1.messageTypeList = { "SAY" }
ChatFrame2:AddMessage("an old line")
local before = #ChatFrame2.added
popup = nil
ns.db.emptyWindow = "General"
ns.EmptyChosen(Undo)
ChatFrame1:AddMessage("passing by")
ns.db.emptyWindow = "Pick"
ns.EmptyChosen(Undo)
Check("a tab is not emptied the moment it is chosen", #ChatFrame2.added, before)
Settle()
Check("a tab passed on the way is left alone, and not asked about", tostring(#ChatFrame1.added > 0) .. " " .. tostring(popup), "true nil")
Check("a tab chosen to be kept empty is emptied once the choice has stood", #ChatFrame2.added, 0)
ChatFrame2:AddMessage("Total time played: 3 days")
ChatFrame1:AddMessage("hello")
Check("and so is every line added to it", #ChatFrame2.added, 0)
Check("other tabs keep theirs", ChatFrame1.added[#ChatFrame1.added][1], "hello")
ns.db.sourceWindow = "Pick"
ChatFrame2:AddMessage("[Guild] [Bob]: 副本缺補", 0.25, 1, 0.25)
ns.db.sourceWindow = ""
Check("its lines can still be picked for the overlay", Frames(), { "1 | [Guild] [Bob]:  | 副本缺補 | 40ff40 | " })

ns.db.emptyWindow = "General"
ns.EmptyChosen(Undo)
Settle()
ChatFrame1:AddMessage("still here")
ChatFrame2:AddMessage("gone")
Check("a tab set to show chat is kept until the player agrees, and the one before stays empty",
    { ChatFrame1.added[#ChatFrame1.added][1], #ChatFrame2.added }, { "still here", 0 })
popup.dialog.OnCancel(nil, popup.data)
ChatFrame1:AddMessage("and here")
Check("refusing puts the options back on the tab kept before", ns.db.emptyWindow .. " " .. ChatFrame1.added[#ChatFrame1.added][1], "Pick and here")
ns.db.emptyWindow = "General"
ns.EmptyChosen(Undo)
Settle()
popup.dialog.OnAccept(nil, popup.data)
Check("agreeing empties it", #ChatFrame1.added, 0)
ns.db.emptyWindow = "Gone"
ns.EmptyChosen(Undo)
Settle()
ChatFrame1:AddMessage("kept")
Check("a tab that no longer exists empties none", #ChatFrame1.added, 1)
ns.db.emptyWindow = ""
ns.EmptyChosen(Undo)
Settle()

-- A new empty tab -----------------------------------------------------------------

local cleared = {}
function FCF_OpenNewWindow(name)
    NUM_CHAT_WINDOWS = 3
    ChatFrame3 = ChatWindow()
    ChatFrame3.RemoveAllMessageGroups = function() cleared[#cleared + 1] = "types" end
    ChatFrame3.RemoveAllChannels = function() cleared[#cleared + 1] = "channels" end
    tabNames[3] = name
    return ChatFrame3
end
local _, problem = ns.NewEmptyTab("Overlay")
Check("with every chat window in use, no tab is made", problem:match("^All 2 chat windows") ~= nil, true)
printed, popup = {}, nil
ns.AskNewEmptyTab(function() end)
Check("nor is its name asked for", tostring(popup) .. " " .. #printed, "nil 1")
ChatFrame2.isDocked = false
ChatFrame2.IsShown = function() return false end
_, problem = ns.NewEmptyTab("  ")
Check("a tab needs a name", problem, "The tab needs a name.")
-- The popup asks for the name, offering one; the player types another.
local typed, made
local dialog = { editBox = { SetText = function(_, text) typed = text end, HighlightText = function() end, GetText = function() return typed end } }
ns.AskNewEmptyTab(function(name) made = name end)
popup.dialog.OnShow(dialog)
Check("a name is offered", typed, "Translated")
typed = " Overlay "
popup.dialog.OnAccept(dialog, popup.data)
Check("a free chat window becomes a tab of the name typed that shows nothing",
    { made, tabNames[3], table.concat(cleared, " ") }, { "Overlay", "Overlay", "types channels" })
printed = {}
popup.dialog.OnAccept(dialog, popup.data)
Check("a second tab of that name is not made", printed[1]:match("There is already a tab named \"Overlay\"") ~= nil, true)

-- The minimap button ----------------------------------------------------------------

-- Where the strip is drawn ----------------------------------------------------------------

local function Origin(place)
    NaiMaSpeakDB.stripPosition = place or NaiMaSpeakDB.stripPosition
    local left, top = ns.StripOrigin()
    return left .. "," .. top
end
Check("the strip starts in the top-left corner", Origin("topleft"), "0,0")
local right, bottom = 1920 - ns.STRIP.cols * ns.STRIP.block, 1080 - (ns.STRIP.rows + 1) * ns.STRIP.block
Check("or sits in another corner of a 1920x1080 screen, clear of a window's rounded bottom corners",
    Origin("topright") .. " " .. Origin("bottomleft") .. " " .. Origin("bottomright"),
    right .. ",0 6," .. (bottom - 6) .. " " .. (right - 6) .. "," .. (bottom - 6))
fullscreen = true
Check("and right in the corner in fullscreen", Origin("bottomleft") .. " " .. Origin("bottomright"),
    "0," .. bottom .. " " .. right .. "," .. bottom)
fullscreen = false
ns.KeepStripAt(300.4, 99.6)
Check("or where it was dragged, on whole pixels", Origin(), "300,100")
ns.KeepStripAt(40, bottom - 60)
Check("a strip dropped near the bottom", Origin(), "40," .. (bottom - 60))
local screen = GetPhysicalScreenSize
function GetPhysicalScreenSize() return 3840, 2160 end
Check("is as near the bottom on a screen twice the size, twice as far in",
    Origin(), "80," .. (2160 - (ns.STRIP.rows + 1) * ns.STRIP.block - 120))
function GetPhysicalScreenSize() return 1280, 720 end
ns.KeepStripAt(1280, 0)
GetPhysicalScreenSize = screen
Check("and one dropped past the edge stays on the screen", Origin(), right .. ",0")
NaiMaSpeakDB.stripPosition = "topleft"

local entries = {}
MenuUtil = {
    CreateContextMenu = function(_, fill)
        fill(nil, { CreateTitle = function() end, CreateButton = function(_, text, click) entries[text] = click end })
    end,
}
ns.MinimapMenu()
entries["App Options"]()
Check("the minimap menu shows the companion app's window", Frames(), { "C | settings" })
printed = {}
entries["AddOn Options"]()
Check("and opens the addon's options, or says there are none", #printed, 1)
local angles = {}
for _, at in ipairs({ { 1, 0 }, { 0, 1 }, { -1, 0 }, { 0, -1 } }) do angles[#angles + 1] = ns.AngleRound(10, 10, 10 + at[1], 10 + at[2]) end
Check("the button follows the pointer round the minimap", table.concat(angles, " "), "0 90 180 270")

local toc = assert(io.open(ADDON .. "NaiMaSpeak.toc")):read("*a")
Check("the .toc and Core.lua name the same version", toc:match("## Version: (%S+)"), ns.VERSION)

Check("SLASH aliases", SLASH_NAIMASPEAK1 .. " " .. SLASH_NAIMASPEAK3, "/tr /nms")
local numbers = {}
for _, n in ipairs({ 1, 9, 10, 11, 20, 21, 99 }) do numbers[#numbers + 1] = ns.ChineseNumber(n) end
Check("Chinese numbers", table.concat(numbers, " "), "一 九 十 十一 二十 二十一 九十九")

-- The demo ----------------------------------------------------------------------------------

Frames()
printed = {}
ns.Demo()
local demo = table.concat(Frames(), "\n")
Check("the demo's lines reach the overlay as chat does", demo:match("^[^\n]*"),
    "1 | [1. General - Undercity] [奶茶 半糖]:  | 血色修道院 教堂 缺坦缺DD 有補 來的密我 | ffbfbf | 奶茶 半糖")
Check("all of them, the one without Chinese too", select(2, demo:gsub("[^\n]+", "")), 12)
Check("and are added to the chat window with their links", printed[1]:match("|Haddon.*$"),
    "|Haddon:NaiMaSpeak:T:1073741825|h|cff66ccff[T]|r|h|Haddon:NaiMaSpeak:E:1073741825|h|cff66ccff[?]|r|h"
        .. "|Haddon:NaiMaSpeak:N:1073741825|h|cff66ccff[?N]|r|h")

-- The old addon, named Translate ---------------------------------------------------------------

local loaded, disabled = { Translate = "Nai Ma Speak" }, {}
C_AddOns = {
    GetAddOnMetadata = function(name) return loaded[name] or error("Invalid AddOn name") end,
    IsAddOnLoaded = function(name) return loaded[name] ~= nil end,
    DisableAddOn = function(name) disabled[#disabled + 1] = name end,
}
TranslateDB = { repeatLines = 20 }
printed = {}
ns.RetireOld()
Check("the old addon is turned off and its settings taken over", { disabled[1], NaiMaSpeakDB.repeatLines, #printed }, { "Translate", 20, 1 })
loaded, disabled = { Translate = "Translate" }, {}
ns.RetireOld()
loaded = {}
ns.RetireOld()
Check("another addon of that name, or none, is left alone", #disabled, 0)
C_AddOns, TranslateDB, NaiMaSpeakDB.repeatLines = nil, nil, 50

if failures > 0 then
    print(failures .. " failed")
    os.exit(1)
end
print("addon tests passed")
