local _, ns = ...

-- The pixel strip: the only live way out of the game. New Chinese lines are
-- drawn as a small grid of colored blocks that the companion app reads off
-- the screen. Each block carries 6 bits: 4 brightness levels per channel,
-- 64 colors in all.
--
-- Frame layout, row by row from the top-left block:
--   3 sync blocks (magenta, cyan, yellow), then the payload bytes
--   [seq] [length hi] [length lo] [text...] [fletcher16 hi] [fletcher16 lo]
--   packed 3 bytes -> 4 blocks, padded with black.
-- The text holds one or more chat lines, separated by newlines.
--
-- Under the frame sits the palette: one more row with the 64 colors in order.
-- A display's color profile, the game's gamma setting and the like change
-- every color on the way to the companion; it reads a block by finding the
-- palette color it looks like, so it need not know what was changed.
local COLS, ROWS = 64, 8 -- of the frame; the palette row comes after
local BLOCK = 2 -- physical screen pixels per block side
local SYNC = { 51, 15, 60 } -- (3,0,3) magenta, (0,3,3) cyan, (3,3,0) yellow
local CAPACITY = math.floor((COLS * ROWS - #SYNC) / 4) * 3
local MAX_TEXT = CAPACITY - 5
local SHOW_TIME = 0.25 -- seconds each frame stays up; the app polls faster
local MAX_QUEUE = 200

ns.STRIP = { cols = COLS, rows = ROWS, block = BLOCK, maxText = MAX_TEXT }

local function Fletcher16(bytes)
    local a, b = 0, 0
    for i = 1, #bytes do
        a = (a + bytes[i]) % 255
        b = (b + a) % 255
    end
    return b * 256 + a
end

-- Cuts at a UTF-8 character boundary so a long line stays valid text.
local function Truncate(text, max)
    if #text <= max then return text end
    local cut = max
    while cut > 0 do
        local c = text:byte(cut + 1)
        if c < 128 or c >= 192 then break end
        cut = cut - 1
    end
    return text:sub(1, cut)
end

ns.Truncate = Truncate

-- Returns the frame's COLS * ROWS symbols (0..63). Pure Lua, so it can be tested outside WoW.
function ns.EncodeFrame(seq, text)
    text = Truncate(text, MAX_TEXT)
    local bytes = { seq % 256, math.floor(#text / 256), #text % 256 }
    for i = 1, #text do bytes[#bytes + 1] = text:byte(i) end
    local sum = Fletcher16(bytes)
    bytes[#bytes + 1] = math.floor(sum / 256)
    bytes[#bytes + 1] = sum % 256

    local symbols = {}
    for i = 1, #SYNC do symbols[i] = SYNC[i] end
    for i = 1, #bytes, 3 do
        local v = (bytes[i] or 0) * 65536 + (bytes[i + 1] or 0) * 256 + (bytes[i + 2] or 0)
        symbols[#symbols + 1] = math.floor(v / 262144) % 64
        symbols[#symbols + 1] = math.floor(v / 4096) % 64
        symbols[#symbols + 1] = math.floor(v / 64) % 64
        symbols[#symbols + 1] = v % 64
    end
    for i = #symbols + 1, COLS * ROWS do symbols[i] = 0 end
    return symbols
end

local frame, textures
local queue = {}
-- Lines shown again, in turn, while the queue is empty: the last
-- `repeatLines` given to RepeatOnStrip, each a function that returns the
-- line's text. `turn` is the one shown last.
local again, turn = {}, 0
local seq = 0
local ticking = false

local WIDTH, HEIGHT = COLS * BLOCK, (ROWS + 1) * BLOCK
-- How far in from the edges the strip sits in a bottom corner while the game runs
-- in a window: a window's bottom corners are rounded (macOS, Windows 11), which
-- cuts into what is drawn right in them. In screen pixels, while the rounding is
-- twice as many of them on a high-density screen: enough for one of 50.
local MARGIN = 16

-- The strip's top-left corner in physical screen pixels from the screen's: in the
-- corner chosen in the options, or where the player dragged it. Whole pixels, so
-- every block covers exactly its own.
function ns.StripOrigin()
    local width, height = GetPhysicalScreenSize()
    local right, bottom = math.max(width - WIDTH, 0), math.max(height - HEIGHT, 0)
    local place = ns.db.stripPosition
    if place == "custom" then
        local function Within(value, most)
            return math.min(math.max(math.floor((tonumber(value) or 0) * height / 768 + 0.5), 0), most)
        end
        local from = ns.db.stripFrom or "topleft"
        local x, y = Within(ns.db.stripX, right), Within(ns.db.stripY, bottom)
        return from:find("right") and right - x or x, from:find("bottom") and bottom - y or y
    end
    local low = place == "bottomleft" or place == "bottomright"
    local fullscreen = GetCVarBool and GetCVarBool("gxMaximize")
    local margin = low and not fullscreen and MARGIN or 0
    local left = (place == "topright" or place == "bottomright") and math.max(right - margin, 0) or margin
    return left, low and math.max(bottom - margin, 0) or 0
end

-- Remembers where the strip was dropped, its top-left corner given in physical pixels:
-- as the distance from the nearest corner of the screen, in the game's own units (the
-- screen is 768 high), so that on another screen it sits by the same corner, as far in
-- as the rest of the interface.
function ns.KeepStripAt(left, top)
    local width, height = GetPhysicalScreenSize()
    local right, bottom = width - WIDTH - left, height - HEIGHT - top
    ns.db.stripPosition = "custom"
    ns.db.stripFrom = (bottom < top and "bottom" or "top") .. (right < left and "right" or "left")
    ns.db.stripX, ns.db.stripY = math.min(left, right) * 768 / height, math.min(top, bottom) * 768 / height
end

-- One block = one physical pixel grid cell, whatever the UI scale.
function ns.PlaceStrip()
    if not frame then return end
    local _, height = GetPhysicalScreenSize()
    local left, top = ns.StripOrigin()
    frame:SetScale(768 / height)
    frame:ClearAllPoints()
    frame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", left, -top)
end

-- While the game's Edit Mode is open the strip is outlined and can be dragged; where
-- it is dropped becomes its place. `edit` is the game's EditModeManagerFrame.
local function MoveInEditMode(edit)
    local outline = frame:CreateTexture(nil, "BACKGROUND")
    outline:SetPoint("TOPLEFT", -3, 3)
    outline:SetPoint("BOTTOMRIGHT", 3, -3)
    outline:SetColorTexture(0.4, 0.8, 1, 1)
    frame:SetMovable(true)
    frame:SetClampedToScreen(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", frame.StartMoving)
    frame:SetScript("OnDragStop", function()
        frame:StopMovingOrSizing()
        local _, height = GetPhysicalScreenSize()
        -- The frame's scale makes its units physical pixels, counted from the bottom.
        ns.KeepStripAt(frame:GetLeft(), height - frame:GetTop())
        ns.PlaceStrip()
    end)
    frame:SetScript("OnEnter", function()
        GameTooltip:SetOwner(frame, "ANCHOR_CURSOR")
        GameTooltip:SetText("Nai Ma Speak pixel block")
        GameTooltip:AddLine("Drag to move it. The companion app finds it again within seconds.", 1, 1, 1, true)
        GameTooltip:Show()
    end)
    frame:SetScript("OnLeave", function() GameTooltip:Hide() end)
    local function Editing(on)
        outline:SetShown(on)
        frame:EnableMouse(on)
    end
    edit:HookScript("OnShow", function() Editing(true) end)
    edit:HookScript("OnHide", function() Editing(false) end)
    Editing(edit:IsShown() == true)
end

local function Build()
    frame = CreateFrame("Frame", "NaiMaSpeakStrip", UIParent)
    frame:SetIgnoreParentScale(true)
    frame:SetFrameStrata("TOOLTIP")
    frame:SetSize(WIDTH, HEIGHT)
    textures = {}
    for i = 0, COLS * (ROWS + 1) - 1 do
        local t = frame:CreateTexture(nil, "OVERLAY")
        t:SetSize(BLOCK, BLOCK)
        t:SetPoint("TOPLEFT", (i % COLS) * BLOCK, -math.floor(i / COLS) * BLOCK)
        textures[i + 1] = t
    end
    for color = 0, COLS - 1 do ns.PaintBlock(COLS * ROWS + color + 1, color) end
    ns.PlaceStrip()
    if EditModeManagerFrame and EditModeManagerFrame.HookScript then
        local ok, problem = pcall(MoveInEditMode, EditModeManagerFrame)
        ns.stripMoveSource = ok and "Edit Mode" or "failed: " .. tostring(problem)
    else
        ns.stripMoveSource = "none (no Edit Mode on this client)"
    end
end

-- Colors the block at `index` (from 1) with one of the 64 colors.
function ns.PaintBlock(index, color)
    local r, g, b = math.floor(color / 16) % 4, math.floor(color / 4) % 4, color % 4
    textures[index]:SetColorTexture(r / 3, g / 3, b / 3, 1)
end

local function Paint(symbols)
    for i, s in ipairs(symbols) do ns.PaintBlock(i, s) end
end

local function ForgetOld()
    while #again > ns.db.repeatLines do
        table.remove(again, 1)
        turn = math.max(turn - 1, 0)
    end
end

-- The text of the next frame, or nil with nothing to show. Short lines share
-- a frame, so a busy channel does not fall behind.
local function NextText()
    local text = table.remove(queue, 1)
    if text then
        while queue[1] and #text + 1 + #queue[1] <= MAX_TEXT do
            text = text .. "\n" .. table.remove(queue, 1)
        end
        return text
    end
    ForgetOld()
    for _ = 1, #again do
        local line = again[turn % #again + 1]()
        if text and #text + 1 + #line > MAX_TEXT then break end
        text = text and (text .. "\n" .. line) or line
        turn = turn % #again + 1
    end
    return text
end

local function Advance()
    local text = NextText()
    if not text then
        ticking = false
        return
    end
    seq = (seq + 1) % 256
    Paint(ns.EncodeFrame(seq, text))
    ticking = true
    C_Timer.After(SHOW_TIME, Advance)
end

-- Text sent with a key is state, not a chat line: the chat window layout or a
-- window's scroll count. It jumps the queue, so the overlay follows the game
-- even with many lines waiting, and it replaces waiting text that starts
-- with the same key, since only the newest state matters.
function ns.SendToStrip(text, key)
    if not frame then Build() end
    if key then
        for i = #queue, 1, -1 do
            if queue[i]:sub(1, #key) == key then table.remove(queue, i) end
        end
        table.insert(queue, 1, text)
    else
        if #queue >= MAX_QUEUE then table.remove(queue, 1) end
        queue[#queue + 1] = text
    end
    if not ticking then Advance() end
end

-- Has the strip show a line again from time to time, once it has been sent.
-- `text` is called each time for the line as it then reads.
function ns.RepeatOnStrip(text)
    again[#again + 1] = text
    ForgetOld()
end

ns.On("DISPLAY_SIZE_CHANGED", function() ns.PlaceStrip() end)
ns.On("UI_SCALE_CHANGED", function() ns.PlaceStrip() end)
