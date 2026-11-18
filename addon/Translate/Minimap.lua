local _, ns = ...

-- A button on the rim of the minimap. A click opens a menu with the addon's options
-- and the companion app's window, which is what the App button on the chat window
-- shows. The button can be dragged round the minimap, and turned off in the options.

local button

-- Where the pointer is round a center, in degrees counted from the right, leftwards.
function ns.AngleRound(centerX, centerY, x, y)
    return math.deg(math.atan2(y - centerY, x - centerX)) % 360
end

local function Place()
    local angle = math.rad(ns.db.minimapAngle)
    local radius = Minimap:GetWidth() / 2 + 5
    button:ClearAllPoints()
    button:SetPoint("CENTER", Minimap, "CENTER", math.cos(angle) * radius, math.sin(angle) * radius)
end

-- While the button is dragged: keeps it on the rim, nearest the pointer.
local function Follow()
    local centerX, centerY = Minimap:GetCenter()
    local x, y = GetCursorPosition()
    local scale = Minimap:GetEffectiveScale()
    ns.db.minimapAngle = ns.AngleRound(centerX, centerY, x / scale, y / scale)
    Place()
end

function ns.MinimapMenu(owner)
    MenuUtil.CreateContextMenu(owner, function(_, menu)
        menu:CreateTitle("Nai Ma Speak")
        menu:CreateButton("AddOn Options", function() ns.OpenOptions() end)
        menu:CreateButton("App Options", function() ns.SendCommand("settings") end)
    end)
end

-- Looks like the other addons' minimap buttons: an icon in a round gold border.
local function Build()
    button = CreateFrame("Button", "TranslateMinimapButton", Minimap)
    button:SetSize(31, 31)
    button:SetFrameStrata("MEDIUM")
    button:SetFrameLevel(8)
    button:SetHighlightTexture("Interface\\Minimap\\UI-Minimap-ZoomButton-Highlight")
    local background = button:CreateTexture(nil, "BACKGROUND")
    background:SetTexture("Interface\\Minimap\\UI-Minimap-Background")
    background:SetSize(24, 24)
    background:SetPoint("CENTER")
    local icon = button:CreateTexture(nil, "ARTWORK")
    icon:SetTexture("Interface\\AddOns\\Translate\\icon")
    icon:SetSize(18, 18)
    icon:SetPoint("CENTER")
    local border = button:CreateTexture(nil, "OVERLAY")
    border:SetTexture("Interface\\Minimap\\MiniMap-TrackingBorder")
    border:SetSize(53, 53)
    border:SetPoint("TOPLEFT")

    button:RegisterForClicks("AnyUp")
    button:SetScript("OnClick", ns.MinimapMenu)
    button:RegisterForDrag("LeftButton")
    button:SetScript("OnDragStart", function(self) self:SetScript("OnUpdate", Follow) end)
    button:SetScript("OnDragStop", function(self) self:SetScript("OnUpdate", nil) end)
    button:SetScript("OnEnter", function(self)
        GameTooltip:SetOwner(self, "ANCHOR_LEFT")
        GameTooltip:SetText("Nai Ma Speak")
        GameTooltip:AddLine("Click for the options. Drag to move.", 1, 1, 1)
        GameTooltip:Show()
    end)
    button:SetScript("OnLeave", function() GameTooltip:Hide() end)
end

-- Shows the button or hides it, as the options say.
function ns.PlaceMinimapButton()
    if not ns.db.minimap then
        if button then button:Hide() end
        return
    end
    if not button then Build() end
    Place()
    button:Show()
end

function ns.InitMinimap()
    if not (Minimap and MenuUtil and MenuUtil.CreateContextMenu) then
        ns.minimapSource = "none (no minimap or no menus on this client)"
        return
    end
    local ok, problem = pcall(ns.PlaceMinimapButton)
    ns.minimapSource = ok and (ns.db.minimap and "shown" or "turned off") or ("failed: " .. tostring(problem))
end
