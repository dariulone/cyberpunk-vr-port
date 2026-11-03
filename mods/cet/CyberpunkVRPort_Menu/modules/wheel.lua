-- THE WEAPON WHEEL: the game's radial wheel without the pause. Hold Y and a small wheel hangs in front of the
-- body, a little past the reach of an outstretched arm; point at a cell with either hand (the ray, or the
-- fingertip up close) and let go of Y. The game keeps running throughout -- no slowed time, no UI context, the
-- player still walks, turns and shoots.
--
-- THE HOLD. The plugin takes the hold of Y from the game (Hooks/WeaponWheelHold.hpp) while this module calls
-- VRWeaponWheel(true) every frame: a press is withheld until it is decided, a tap still reaches the game as
-- SwitchItem (the next weapon), and a press held past the game's own 0.3 s is reported here (2) and never seen
-- by the game -- not its wheel, and not the SwitchItem release that ends a hold, which draws the last weapon
-- when nothing is in hand. While the wheel could not open (the game's own conditions, WeaponWheelDecisions),
-- the module does not arm, and Y is the game's as it was.
--
-- WHERE IT HANGS. In the body's frame -- the player entity, whose yaw is the body's -- so it turns with the
-- body and not with the hand or the head: in front of the weapon hand (across from where it is when the
-- wheel opens, kept on that hand's side), REACH ahead of the chest bone and LIFT above it (shoulder height, a
-- little past an outstretched arm), tilted to face the eyes. The offset is taken when it opens; leaning in
-- does not push it away.
--
-- THE CELLS are the game's (radialWheel.swift) in the shape of its own wheel: the three weapon slots as large
-- wedges across the top, each on its quality's colour with the ammo under the weapon; arm cyberware as a
-- smaller wedge on the right and the holster (all weapons put away, the game's crossed-out pistol) as a
-- smaller one still on the left; the grenades (the RB hotkey) and the consumables (D-pad up) as round cells
-- below with their count. An empty slot shows the inventory's empty-slot silhouette. Point at a hotkey's cell
-- and its choices come out on an arc outside the rim; let go over one and it is assigned. Applied with the
-- game's own requests: DrawItemRequest, the UnequipAll manipulation, HotkeyAssignmentRequest; refused where
-- the game's wheel refuses them (status effect tags).
--
-- WHAT IT IS. vr_menu.ent (the menu's screen, 3200 x 1600 px on 1.00 x 0.50 m, additive) with the wheel
-- drawn in its middle from its own atlas (wheel_art.lua, made by tools/vr_lobby_assets/make_wheel_art.py),
-- the round matte glass of the menu's icons (vr_menu_icon.ent) behind the rim and behind each choice, and the
-- menu's beam for the hand that points. Spawned on the press, so it is ready when the hold is decided; let go
-- of after IDLE seconds unused.

local K = require("modules/ink")
local L = require("modules/locale")
local I = require("modules/inventory_data")
local D = require("modules/data")
local Icons = require("modules/gameplay_data")
local A = require("modules/wheel_art")
local Space = require("modules/space")

local add, sub, mul, dot, cross, norm = Space.add, Space.sub, Space.mul, Space.dot, Space.cross, Space.norm
local clamp, qrot, qconj, transformFor = Space.clamp, Space.qrot, Space.qconj, Space.transformFor

local Wheel = {}

local ENT = "vrport\\ui\\vr_menu.ent"
local BEAM_ENT = "vrport\\ui\\vr_menu_beam.ent"
local DISC_ENT = "vrport\\ui\\vr_menu_icon.ent"
local UI = "vrp_menu_ui"
local SHADOW = "base\\gameplay\\gui\\fullscreen\\inventory\\shadow_icons.inkatlas"

local CW, CH, PX = 3200, 1600, 3200        -- canvas pixels, pixels per metre
local W, H = CW / PX, CH / PX
local CX, CY = CW / 2, CH / 2
local BEAM_LEN = K.BEAM.len

-- The wheel, canvas px: the cancel hub, the round cells' circle (the wedges' radii are wheel_art's), the rim
-- the cells end inside, with no line of its own, and the glass under it (0.32 m across); a hotkey's choices on
-- an arc outside the rim.
local R_HUB, R_RIM = 135, 508
local R_CELLS, R_CELL = 335, A.disc
local R_ARC, R_CHOICE, ARC_STEP, MAX_CHOICES = 625, 64, 17, 9
local ARC_KEEP = 0.35                      -- seconds on another cell before the arc folds
local SMOKE_WAIT = 3.0                     -- seconds the cigarette waits for the weapons to be put away
-- the cigarette has no icon of its own (its UIIcon is the placeholder): the pack's
local CIGARETTE = "UIIcon.base_junk_item_cigarette_pack_a"
local GLASS_BACK = 0.006

-- Metres, in the body's frame.
-- An outstretched arm reaches about 0.8 m ahead of the body's axis at shoulder height (0.2 m over the chest
-- bone): the wheel hangs a little past the fingertips, so it is pointed at, not touched by accident. Across
-- from the weapon hand: its side's shoulder line at least, as far out as the hand when it opens.
local REACH, LIFT = 0.90, 0.20
local SIDE_MIN, SIDE_MAX, SIDE_DEFAULT = 0.10, 0.35, 0.18
local HOVER, THROUGH = 0.10, -0.05         -- a fingertip this close in front of the screen points with itself
local IDLE = 20.0
local HAPTIC_CLICK = 20

local SOUND_DIR = "CyberpunkVRPort_Menu/sounds/"

-- Clockwise from the top (degrees, 0 = up): a wedge (its shape in wheel_art) is centred on its angle, a round
-- cell sits there on R_CELLS. The empty-slot silhouettes and the holster's crossed-out pistol are the
-- inventory's (shadow_icons), as on the game's wheel.
local SECTORS = {
  { kind = "weapon", slot = 0, shape = "weapon", angle = -52, empty = "gun" },
  { kind = "weapon", slot = 1, shape = "weapon", angle = 0, empty = "gun" },
  { kind = "weapon", slot = 2, shape = "weapon", angle = 52, empty = "gun" },
  { kind = "cyberware", shape = "arm", angle = 108, empty = "cyberware" },
  { kind = "misc", hotkey = "RB", angle = 157, empty = "granade" },
  { kind = "misc", hotkey = "DPAD_UP", angle = 203, empty = "consumable" },
  { kind = "holster", shape = "holster", angle = 252 },
}
for _, s in ipairs(SECTORS) do
  local g = s.shape and A[s.shape]
  if g ~= nil then s.inner, s.outer, s.half = g.inner, g.outer, g.half end
end

local S = {
  time = 0.0,
  open = false,
  screen = nil,          -- { id, entity, comp, root, built, gpu, shown }
  beam = nil,
  glass = nil,           -- the disc under the rim
  choiceGlass = {},      -- discs under the choices
  ctx = nil,             -- InventoryDataManagerV2, while the screen lives
  anchor = nil,          -- { C, R, U } in the body's frame
  slots = {},            -- by sector: { item, blocked, active, choices }
  hover = nil,           -- { index } or { index, choice }, or { hub }
  arc = nil,             -- the sector whose choices are out on the arc
  usedAt = 0,
  allowedAt = -1, allowedValue = false,
  hand = nil,
  fits = {},
  stats = {},
}

local function sound(kind)
  local file = ({ open = "open.wav", close = "close.wav", click = "click.wav" })[kind]
  if file ~= nil and type(VRMenuPlayWav) == "function" then
    pcall(VRMenuPlayWav, SOUND_DIR .. file, D.effectsVolume())
  end
end

local function haptic(hand)
  if hand ~= nil and type(VRHapticEvent) == "function" then pcall(VRHapticEvent, HAPTIC_CLICK, hand == 1 and 1 or 2) end
end

local function pos(angle, r)
  local a = math.rad(angle)
  return CX + math.sin(a) * r, CY - math.cos(a) * r
end

-- Degrees from a to b, -180..180.
local function turn(a, b)
  return (b - a + 540) % 360 - 180
end

-- An item's quality colour; the plus tiers (RarePlus, LegendaryPlusPlus) take their tier's.
local function qualityColor(item)
  local q = item and item.quality
  if q == nil then return nil end
  return K.QUALITY[q] or K.QUALITY[(q:gsub("Plus", ""))]
end

---------------------------------------------------------------- the game's conditions

local function tagged(pl, tag)
  local yes = false
  pcall(function() yes = StatusEffectSystem.ObjectHasStatusEffectWithTag(pl, CName.new(tag)) end)
  return yes
end

local psmDefs = nil
local function psm(pl)
  local bb = nil
  pcall(function()
    if psmDefs == nil then psmDefs = Game.GetAllBlackboardDefs().PlayerStateMachine end
    bb = Game.GetBlackboardSystem():GetLocalInstanced(pl:GetEntityID(), Game.GetAllBlackboardDefs().PlayerStateMachine)
  end)
  return bb
end

-- WeaponWheelDecisions.EnterCondition, as far as script outside the state machine can read it, and the
-- port's own: no scene, no pause, the VR menu closed.
local function allowedNow(pl, menuOpen)
  if menuOpen then return false end
  local paused = false
  pcall(function() paused = Game.GetSystemRequestsHandler():IsGamePaused() or Game.GetSystemRequestsHandler():IsPreGame() end)
  if paused then return false end
  for _, tag in ipairs({ "NoRadialMenus", "NoWeaponWheel", "FocusModeLocomotion" }) do
    if tagged(pl, tag) then return false end
  end
  local replacer = false
  pcall(function() replacer = pl:IsReplacer() end)
  if replacer then return false end
  local bb = psm(pl)
  if bb == nil then return false end
  local ok, verdict = pcall(function()
    if bb:GetInt(psmDefs.SceneTier) >= 3 then return false end
    if bb:GetInt(psmDefs.Swimming) > 0 then return false end
    if bb:GetInt(psmDefs.CombatGadget) > 0 then return false end
    local vehicle = bb:GetInt(psmDefs.Vehicle)
    if vehicle == 4 then return false end                                   -- entering, switching seats
    if bb:GetBool(psmDefs.MountedToVehicle) and vehicle ~= 2 and not bb:GetBool(psmDefs.MountedToCombatVehicle) then
      return false
    end
    if vehicle == 1 and not VehicleTransition.CanEnterDriverCombat() then return false end
    return true
  end)
  return ok and verdict == true
end

-- A prompt on Y is the game's: forcing a door is Choice2_Hold, the same hold of the same button. It only keeps the
-- wheel from taking a new press, so walking past one does not close a wheel already open.
local function promptOnY()
  local yes = false
  pcall(function()
    local d = Game.GetAllBlackboardDefs().UIInteractions
    local hub = FromVariant(Game.GetBlackboardSystem():Get(d):GetVariant(d.InteractionChoiceHub))
    if hub == nil or not hub.active then return end
    for _, c in ipairs(hub.choices) do
      local action, key = NameToString(c.inputAction), tostring(c.rawInputKey)
      if action:find("Choice2", 1, true) == 1 or action == "PickUpBody" or key:find("IK_Pad_Y_TRIANGLE", 1, true) then
        yes = true
      end
    end
  end)
  return yes
end

local function weaponsBlocked(pl)
  for _, tag in ipairs({ "NoCombat", "FirearmsNoSwitch", "ShootingRangeCompetition", "Fists", "FirearmsNoUnequipNoSwitch" }) do
    if tagged(pl, tag) then return true end
  end
  return false
end

---------------------------------------------------------------- what is on it

local slotRight, slotLeft = nil, nil
local function inHand(pl)
  local keys = {}
  pcall(function()
    slotRight = slotRight or TweakDBID.new("AttachmentSlots.WeaponRight")
    slotLeft = slotLeft or TweakDBID.new("AttachmentSlots.WeaponLeft")
    local ts = Game.GetTransactionSystem()
    for _, slot in ipairs({ slotRight, slotLeft }) do
      local item = ts:GetItemInSlot(pl, slot)
      if item ~= nil then keys[I.key(item:GetItemID())] = true end
    end
  end)
  return keys
end

local function hotkeyItems(ctx, hotkey)
  local out, seen = {}, {}
  pcall(function()
    for _, kind in ipairs(Hotkey.GetScope(hotkey)) do
      for _, data in ipairs(ctx.manager:GetPlayerItemsByType(kind)) do
        local item = I.item(ctx, data)
        if item ~= nil and not seen[item.key] then
          seen[item.key] = true
          out[#out + 1] = item
        end
      end
    end
  end)
  return out
end

-- A hotkey's choices and the one on it.
local function hotkeyEntry(ctx, e)
  e.hotkey = EHotkey[e.sector.hotkey]
  e.choices = hotkeyItems(ctx, e.hotkey)
  local current = I.item(ctx, ctx.manager:GetHotkeyItemData(e.hotkey))
  if current ~= nil then
    for _, c in ipairs(e.choices) do if c.key == current.key then e.item = c end end
  end
  e.item = e.item or e.choices[1]
end

-- The cigarette (CyberpunkVRPort_Smoking) first among the consumables' choices while that mod is in:
-- VRSmokeToggle lights one, or puts away the one in hand (then it is ringed as the current choice).
local function smokeChoice(pl)
  local ok, has = pcall(function() return pl:VRSmokeHasCig() end)
  if not ok or has == nil then return nil end
  local image = nil
  pcall(function() image = Icons.image(TweakDBID.new(CIGARETTE)) end)
  return { key = "vrsmoke", smoke = true, lit = has == true, image = image, typeName = "",
    name = has and L.tr("Убрать сигарету", "Put the cigarette away") or L.tr("Сигарета", "Cigarette") }
end

-- The cells' contents, read when the wheel opens (radialWheel.swift: RefreshWeapons, RefreshCyberware,
-- RefreshHotkeys).
local function gather(pl)
  local ctx = S.ctx
  pcall(function() ctx.manager:MarkToRebuild(); ctx.manager:ClearInventoryItemDataCache() end)
  local held = inHand(pl)
  local blocked = weaponsBlocked(pl)
  local slots = {}
  for i, s in ipairs(SECTORS) do
    local e = { sector = s }
    pcall(function()
      if s.kind == "weapon" then
        e.item = I.item(ctx, ctx.manager:GetItemDataEquippedInArea(gamedataEquipmentArea.Weapon, s.slot))
        e.blocked = blocked
      elseif s.kind == "cyberware" then
        local item = I.item(ctx, ctx.manager:GetItemDataEquippedInArea(gamedataEquipmentArea.ArmsCW, 0))
        if item ~= nil and item.type ~= gamedataItemType.Cyb_Launcher then
          e.item = item
          local strongArms = item.type == gamedataItemType.Cyb_StrongArms
          e.blocked = blocked or (tagged(pl, "NoArmsCW") and not strongArms)
        end
      elseif s.kind == "holster" then
        e.blocked = tagged(pl, "FirearmsNoUnequip")
      elseif s.kind == "misc" then
        hotkeyEntry(ctx, e)
        local smoke = s.hotkey == "DPAD_UP" and smokeChoice(pl) or nil
        if smoke ~= nil then table.insert(e.choices, 1, smoke) end
      end
    end)
    e.active = e.item ~= nil and held[e.item.key] == true
    slots[i] = e
  end
  return slots
end

---------------------------------------------------------------- applying

local function equipment() return Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")) end

local function holster(pl)
  local r = NewObject("EquipmentSystemWeaponManipulationRequest")
  r.owner = pl
  r.requestType = EquipmentManipulationAction.UnequipAll
  r.equipAnimType = gameEquipAnimationType.Default
  equipment():QueueRequest(r)
end

-- The cigarette: the one in hand is put away; otherwise one is lit -- with a weapon in hand the weapons are
-- put away first (the cigarette and the lighter take both hands' weapon slots) and it comes once both hands
-- are empty (tick, S.smokeAt). Drawing a weapon or holstering later puts it away again (the smoking mod's
-- OnEquipmentSystemWeaponManipulationRequest).
local function smoke(pl)
  if pl:VRSmokeHasCig() then
    pl:VRSmokeToggle()
  elseif next(inHand(pl)) ~= nil then
    holster(pl)
    S.smokeAt = S.time
  else
    pl:VRSmokeToggle()
  end
  return true
end

local function apply(hover)
  local pl = Game.GetPlayer()
  if pl == nil or hover == nil then return false end
  local e = S.slots[hover.index]
  if e == nil then return false end
  local s = e.sector
  local ok, done = pcall(function()
    if s.kind == "misc" then
      local c = hover.choice and e.choices[hover.choice]
      if c ~= nil and c.smoke then return smoke(pl) end
      if c == nil or (e.item ~= nil and c.key == e.item.key) then return false end
      local r = HotkeyAssignmentRequest.Construct(c.id, e.hotkey, pl, EHotkeyRequestType.Assign)
      if not r:IsValid() then return false end
      equipment():QueueRequest(r)
      return true
    end
    if e.blocked then return false end
    if s.kind == "holster" then
      holster(pl)
      return true
    end
    if e.item == nil or e.active then return false end
    local r = DrawItemRequest.new()
    r.owner = pl
    r.itemID = e.item.id
    equipment():QueueRequest(r)
    return true
  end)
  if not ok then print("[VRWheel] apply failed: " .. tostring(done)) end
  return ok and done == true
end

---------------------------------------------------------------- entities

local function spawn(path, tr)
  local id = nil
  pcall(function() id = exEntitySpawner.Spawn(path, tr, "") end)
  return { id = id }
end

local function despawn(h)
  if h == nil or h.id == nil then return end
  pcall(function()
    local e = Game.FindEntityByID(h.id)
    if e ~= nil then exEntitySpawner.Despawn(e) end
  end)
  h.id, h.entity, h.comp, h.root = nil, nil, nil, nil
end

local function resolve(h, component)
  if h == nil or h.id == nil then return false end
  local function done() return h.comp ~= nil and (component ~= UI or h.root ~= nil) end
  if done() then return true end
  pcall(function()
    h.entity = Game.FindEntityByID(h.id)
    if h.entity ~= nil then h.comp = h.entity:FindComponentByName(CName.new(component)) end
    if h.comp ~= nil and component == UI then h.root = h.comp:GetWidget() end
  end)
  return done()
end

local function place(h, tr)
  if h ~= nil and h.entity ~= nil then pcall(function() Game.VREntitySetWorldTransform(h.entity, tr) end) end
end

-- The screen's display stays off until the plugin reports a finished GPU write made after the canvas was
-- built (VRMenuSurfaceReady), and then only while the wheel is up.
local function display(h, on)
  if h == nil or h.entity == nil or h.shown == on then return end
  pcall(function() h.entity:FindComponentByName(CName.new("display")):Toggle(on) end)
  h.shown = on
end

local function showGlass(h, on, scale)
  if h == nil or not resolve(h, "display") then return end
  if scale ~= nil and h.scale ~= scale then
    pcall(function()
      h.comp.visualScale = Vector3.new(scale, scale, 1.0)
      h.comp:Toggle(false)
    end)
    h.scale, h.shown = scale, false
  end
  if h.shown ~= on then
    pcall(function() h.comp:Toggle(on) end)
    h.shown = on
  end
end

local function release()
  despawn(S.screen); despawn(S.beam); despawn(S.glass)
  for _, g in ipairs(S.choiceGlass) do despawn(g) end
  S.screen, S.beam, S.glass, S.choiceGlass = nil, nil, nil, {}
  if S.ctx ~= nil then pcall(I.close, S.ctx) end
  S.ctx, S.fits = nil, {}
  S.open, S.hover, S.arc, S.anchor = false, nil, nil, nil
end

---------------------------------------------------------------- the body's frame

local function body()
  local pl = Game.GetPlayer()
  if pl == nil then return nil end
  local out = nil
  pcall(function()
    local wp = pl:GetWorldPosition()
    out = { p = { wp.x, wp.y, wp.z }, q = pl:GetWorldOrientation() }
  end)
  return out
end

local function dominantHand()
  return (type(VRLeftHanded) == "function" and VRLeftHanded() == true) and 1 or 2
end

-- Taken when the wheel opens: ahead of the chest bone, across from the weapon hand, facing the eyes.
local function anchorFor(b)
  local view = Space.view()
  if b == nil or view == nil then return nil end
  local eye = qrot(qconj(b.q), sub(view.p, b.p))
  local chest = nil
  pcall(function()
    local c = VRBodyBonePos(2)
    if c ~= nil and c.w > 0.5 then chest = { c.x, c.y, c.z } end
  end)
  chest = chest or { 0, eye[2] - 0.05, eye[3] - 0.35 }
  -- the hand's side of the body (model x is the body's right), out to where the hand is
  local side = dominantHand() == 2 and 1 or -1
  local x = SIDE_DEFAULT
  pcall(function()
    local hand = VRHandRawModel(dominantHand() == 2 and 1 or 0)
    if hand ~= nil and hand.w > 0.5 then x = clamp((hand.x - chest[1]) * side, SIDE_MIN, SIDE_MAX) end
  end)
  local C = { chest[1] + x * side, chest[2] + REACH, chest[3] + LIFT }
  local N = norm(sub(eye, C))
  local R = norm(sub({ 1, 0, 0 }, mul(N, N[1])))
  return { C = C, R = R, U = cross(N, R) }
end

local function frame(b)
  local a = S.anchor
  if a == nil or b == nil then return nil end
  return add(b.p, qrot(b.q, a.C)), qrot(b.q, a.R), qrot(b.q, a.U)
end

---------------------------------------------------------------- drawing

-- An item picture, fitted into w x h around (x, y) once its size is known (a sprite reports it a frame on).
local function picture(parent, image, x, y, w, h, op, col)
  if image == nil then return nil end
  local pic = K.image(parent, x, y, 1, 1, image.atlas, image.part, col or K.WHITE, .001)
  pic:SetFitToContent(true)
  pic:SetRenderTransformPivot(Vector2.new({ X = 0, Y = 0 }))
  pic:SetScale(Vector2.new({ X = .001, Y = .001 }))
  S.fits[#S.fits + 1] = { widget = pic, x = x, y = y, w = w, h = h, op = op or 1 }
  return pic
end

local function fitPictures()
  for i = #S.fits, 1, -1 do
    local f = S.fits[i]
    local size = f.widget:GetDesiredSize()
    if size.X > 1 and size.Y > 1 then
      local k = math.min(f.w / size.X, f.h / size.Y)
      f.widget:SetFitToContent(false)
      K.size(f.widget, size.X * k, size.Y * k)
      f.widget:SetScale(Vector2.new({ X = 1, Y = 1 }))
      K.move(f.widget, f.x - size.X * k / 2, f.y - size.Y * k / 2)
      f.widget:SetOpacity(f.op)
      table.remove(S.fits, i)
    end
  end
end

local function label(parent, cx, cy, w, text, size, col)
  return K.text(parent, cx - w / 2, cy - size, w, size * 2, text, size, col, "center")
end

-- A part of the wheel's atlas at its own size times `scale`, its centre point (the wheel's centre for a
-- wedge, a disc's own) on (cx, cy), turned clockwise by `angle` about that point.
local function art(parent, name, angle, cx, cy, col, op, scale)
  local p = A.parts[name]
  local k = scale or 1
  local w, h = p.w * k, p.h * k
  local dx, dy = (p.w / 2 - p.ox) * k, (p.h / 2 - p.oy) * k
  local a = math.rad(angle)
  local c, s = math.cos(a), math.sin(a)
  local img = K.image(parent, cx + dx * c - dy * s - w / 2, cy + dx * s + dy * c - h / 2, w, h, A.ATLAS, name, col, op)
  if angle ~= 0 then
    pcall(function()
      img:SetRenderTransformPivot(Vector2.new({ X = 0.5, Y = 0.5 }))
      img:SetRotation(angle)
    end)
  end
  return img
end

-- A count on a small pill, under a hotkey item.
local function badge(parent, cx, cy, value)
  local str = tostring(value)
  local w, h = 28 + 13 * #str, 34
  K.roundFill(parent, cx - w / 2, cy - h / 2, w, h, K.CYAN, .2, 12)
  K.roundEdge(parent, cx - w / 2, cy - h / 2, w, h, K.CYAN, .85, 12)
  K.text(parent, cx - w / 2, cy - h / 2, w, h, str, 24, K.WHITE, "center")
end

-- A weapon's attachment slots, true where a part is in (InventoryItemData.GetAttachments, as I.slots reads
-- them, without making an item of each part).
local function attachments(item)
  local out, seen = {}, {}
  pcall(function()
    for _, part in ipairs(InventoryItemData.GetAttachments(item.data)) do
      local key = tostring(part.SlotID)
      if not seen[key] then
        seen[key] = true
        out[#out + 1] = not InventoryItemData.IsEmpty(part.ItemData)
      end
    end
  end)
  return out
end

-- One dot per attachment slot over the weapon, as the inventory's tiles mark them: filled where a part is
-- in, a ring where the slot is empty.
local function attachmentDots(parent, cx, cy, slots)
  local step = 22
  local x0 = cx - (#slots - 1) * step / 2
  for i, filled in ipairs(slots) do
    local x = x0 + (i - 1) * step
    K.image(parent, x - 7, cy - 7, 14, 14, K.ATLAS.phone, "circle", K.RED, 1)
    if not filled then K.image(parent, x - 4, cy - 4, 8, 8, K.ATLAS.phone, "circle", K.BLACK, 1) end
  end
end

-- A weapon's ammo under its picture, as the inventory's tiles show it: two bars and the count.
local function ammo(parent, cx, cy, count)
  local str = tostring(count)
  local x = cx - (16 + 13 * #str) / 2
  K.rect(parent, x, cy - 7, 3, 14, K.CYAN, 1)
  K.rect(parent, x + 6, cy - 7, 3, 14, K.CYAN, 1)
  K.text(parent, x + 16, cy - 14, 13 * #str + 12, 28, str, 24, K.CYAN)
end

-- A cell's look at rest: the fill and the band in its item's quality, the edge by its state.
local function rest(cell)
  local col = cell.col
  K.tint({ cell.fill }, cell.empty and K.GREY or col, cell.empty and .1 or (cell.band and .5 or .4))
  if cell.band ~= nil then K.tint({ cell.band }, col, cell.empty and 0 or .9) end
  K.tint(cell.edge, (cell.empty and K.GREY) or (cell.blocked and K.RED) or K.CYAN, cell.empty and .28 or .55)
  cell.glow:SetOpacity(0)
end

-- The parts that never change, built once per screen.
local function buildFrame(h)
  local root = h.root
  root:RemoveAllChildren()
  h.warm = K.rect(root, 0, 0, 1, 1, K.BLACK, 1)
  local w = K.canvas(root, 0, 0, CW, CH)
  -- the glass's own light, as on the menu's windows (K.holoPanel) but with no frame: a sheen brightening
  -- towards the edge and faint scan lines across the disc
  art(w, "disc_fill", 0, CX, CY, K.CYAN, .1, (R_RIM + 6) / R_CELL)
  for y = -R_RIM + 18, R_RIM - 18, 18 do
    local half = math.sqrt(R_RIM * R_RIM - y * y) - 10
    K.rect(w, CX - half, CY + y, 2 * half, 1, K.CYAN, .045)
  end
  local hub = R_HUB / R_CELL
  h.hubGlow = art(w, "disc_fill", 0, CX, CY, K.CYAN, .12, hub)
  h.hubRing = { art(w, "disc_edge", 0, CX, CY, K.CYAN, .5, hub) }
  h.hubMark = { K.slash(w, CX, CY, 64, 6, K.GREY, .7), K.slash(w, CX, CY, 64, 6, K.GREY, .7) }
  pcall(function() h.hubMark[2]:SetRotation(45.0) end)
  h.hubName = label(w, CX, CY - 18, 250, "", 30, K.TEXT)
  h.hubInfo = label(w, CX, CY + 30, 250, "", 24, K.GREY)
  h.cells = {}
  for i, s in ipairs(SECTORS) do
    if s.shape then
      local cx, cy = pos(s.angle, (s.inner + s.outer) / 2 + 10)
      h.cells[i] = {
        fill = art(w, s.shape .. "_fill", s.angle, CX, CY, K.CYAN, 0),
        band = art(w, s.shape .. "_band", s.angle, CX, CY, K.CYAN, 0),
        glow = art(w, s.shape .. "_fill", s.angle, CX, CY, K.YELLOW, 0),
        edge = { art(w, s.shape .. "_edge", s.angle, CX, CY, K.CYAN, .55) },
        content = K.canvas(w, 0, 0, CW, CH), cx = cx, cy = cy,
        mark = { pos(s.angle, s.inner + 24) },
      }
    else
      local x, y = pos(s.angle, R_CELLS)
      local c = K.canvas(w, x - R_CELL, y - R_CELL, 2 * R_CELL, 2 * R_CELL)
      h.cells[i] = {
        fill = art(c, "disc_fill", 0, R_CELL, R_CELL, K.CYAN, 0),
        glow = art(c, "disc_fill", 0, R_CELL, R_CELL, K.YELLOW, 0),
        edge = { art(c, "disc_edge", 0, R_CELL, R_CELL, K.CYAN, .55) },
        content = K.canvas(c, 0, 0, 2 * R_CELL, 2 * R_CELL),
        cx = R_CELL, cy = R_CELL, mark = { R_CELL, 18 },
      }
    end
  end
  h.choiceLayer = K.canvas(w, 0, 0, CW, CH)
  local top = K.canvas(root, 0, 0, CW, CH)
  h.cursor = K.canvas(top, 0, 0, 64, 64)
  h.cursorRing = K.ring(h.cursor, 32, 32, 17, 2, K.YELLOW, .95, 28)
  K.image(h.cursor, 29, 29, 6, 6, K.ATLAS.phone, "circle", K.WHITE, 1)
  h.cursor:SetOpacity(0)
  h.built = true
end

local function text(e)
  local s = e.sector
  if s.kind == "holster" then return L.tr("Убрать оружие", "Holster") end
  if e.item ~= nil then return e.item.name end
  return ""
end

-- The cells' contents, drawn again each time the wheel opens.
local function fillCells(h)
  S.fits = {}
  for i, e in ipairs(S.slots) do
    local cell = h.cells[i]
    cell.content:RemoveAllChildren()
    local s = e.sector
    local x, y = cell.cx, cell.cy
    local item = e.item
    local empty = item == nil and s.kind ~= "holster"
    local dim = e.blocked and .35 or 1
    if s.kind == "holster" then
      picture(cell.content, { atlas = SHADOW, part = "unequip" }, x, y, 130, 70, e.blocked and .3 or .9, K.TEXT)
    elseif empty and s.empty ~= nil then
      local pw, ph = 96, 70
      if s.kind == "weapon" then pw, ph = 200, 80 elseif s.kind == "cyberware" then pw, ph = 130, 70 end
      picture(cell.content, { atlas = SHADOW, part = s.empty }, x, y, pw, ph, .4, K.GREY)
    elseif item ~= nil and item.image ~= nil then
      if s.kind == "weapon" then
        -- attachment dots over the picture, the ammo and the name under it
        local slots = attachments(item)
        if #slots > 0 then attachmentDots(cell.content, x, y - 90, slots) end
        picture(cell.content, item.image, x, y - 22, 220, 104, dim)
        local below = y + 44
        if not item.melee and item.ammo ~= nil and item.ammo >= 0 then
          ammo(cell.content, x, below, item.ammo)
          below = below + 32
        end
        label(cell.content, x, below + 11, 240, K.clip(item.name, 18), 22, K.TEXT)
      elseif s.kind == "cyberware" then
        picture(cell.content, item.image, x, y, 150, 120, dim)
      else
        picture(cell.content, item.image, x, y - 4, 96, 96, dim)
      end
    elseif not empty then
      label(cell.content, x, y, s.shape and 240 or 2 * R_CELL - 20, text(e), 26, K.TEXT)
    end
    if s.kind == "misc" and item ~= nil and (item.quantity or 0) > 0 then
      badge(cell.content, x, y + R_CELL - 6, item.quantity)
    end
    if e.active then K.image(cell.content, cell.mark[1] - 8, cell.mark[2] - 8, 16, 16, K.ATLAS.phone, "circle", K.YELLOW, 1) end
    cell.empty, cell.blocked = empty, e.blocked
    cell.col = qualityColor(item) or K.CYAN
    rest(cell)
  end
end

local function choiceAngle(e, n, k)
  return e.sector.angle + (k - (n + 1) / 2) * ARC_STEP
end

-- A hotkey's choices on the arc outside the rim, centred on its cell, the one on the hotkey ringed in yellow.
local function drawChoices(h, index)
  h.choiceLayer:RemoveAllChildren()
  h.choiceCells = {}
  local e = index and S.slots[index]
  if e == nil or e.choices == nil or #e.choices == 0 then return end
  local n = math.min(#e.choices, MAX_CHOICES)
  local k0 = R_CHOICE / R_CELL
  for k = 1, n do
    local c = e.choices[k]
    local x, y = pos(choiceAngle(e, n, k), R_ARC)
    local canvas = K.canvas(h.choiceLayer, x - R_CHOICE, y - R_CHOICE, 2 * R_CHOICE, 2 * R_CHOICE)
    local current = (c.smoke and c.lit) or (not c.smoke and e.item ~= nil and c.key == e.item.key)
    art(canvas, "disc_fill", 0, R_CHOICE, R_CHOICE, qualityColor(c) or K.CYAN, .4, k0)
    h.choiceCells[k] = {
      x = x, y = y, current = current,
      glow = art(canvas, "disc_fill", 0, R_CHOICE, R_CHOICE, K.YELLOW, 0, k0),
      edge = { art(canvas, "disc_edge", 0, R_CHOICE, R_CHOICE, current and K.YELLOW or K.CYAN, current and .9 or .6, k0) },
    }
    picture(canvas, c.image, R_CHOICE, R_CHOICE - 4, 84, 84, 1)
    if (c.quantity or 0) > 0 then badge(canvas, R_CHOICE, 2 * R_CHOICE - 4, c.quantity) end
  end
end

---------------------------------------------------------------- pointing

-- What the canvas point (x, y) is on: the hub, a wedge or a round cell, or a choice on the open arc. While
-- the arc is out, the band outside the cells under its span is the hotkey's own cell: a hand going from the
-- cell to a choice at the arc's far end cuts across the neighbouring cells' directions, and must not fold the
-- arc on the way.
local function hit(x, y)
  local dx, dy = x - CX, y - CY
  local r = math.sqrt(dx * dx + dy * dy)
  if r < R_HUB then return { hub = true } end
  local a = math.deg(math.atan2(dx, -dy)) % 360
  local e = S.arc and S.slots[S.arc]
  if e ~= nil and r > R_CELLS + R_CELL then
    local n = math.min(#e.choices, MAX_CHOICES)
    if math.abs(turn(e.sector.angle, a)) <= (n + 1) / 2 * ARC_STEP then
      if r > R_ARC + R_CHOICE + 40 then return nil end
      if r > R_ARC - R_CHOICE - 20 then
        for k = 1, n do
          if math.abs(turn(choiceAngle(e, n, k), a)) <= ARC_STEP / 2 then return { index = S.arc, choice = k } end
        end
      end
      return { index = S.arc }
    end
  end
  if r > R_RIM + 60 then return nil end
  local nearest, best = nil, 1e9
  for i, slot in ipairs(S.slots) do
    local s = slot.sector
    local d = math.abs(turn(s.angle, a))
    if s.shape then
      if d <= s.half and r >= s.inner - 20 then return { index = i } end
    else
      local cx, cy = pos(s.angle, R_CELLS)
      if (x - cx) ^ 2 + (y - cy) ^ 2 <= (R_CELL + 18) ^ 2 then return { index = i } end
    end
    if d < best then nearest, best = i, d end
  end
  -- between the cells inside the rim: the nearest one by direction
  if r >= R_HUB + 30 and nearest ~= nil then return { index = nearest } end
  return nil
end

local function same(a, b)
  if a == nil or b == nil then return a == b end
  return a.hub == b.hub and a.index == b.index and a.choice == b.choice
end

-- Where the hands point on the screen: the fingertip when it is up close, else the ray. The hand that points
-- at the wheel wins; of two, the weapon hand.
local function point(C, R, U)
  if type(VRMenuPointer) ~= "function" then return nil end
  local out = nil
  pcall(function()
    out = VRMenuPointer(Vector4.new(C[1], C[2], C[3], 1), Vector4.new(R[1], R[2], R[3], 0),
      Vector4.new(U[1], U[2], U[3], 0), Vector4.new(W, H, 0, 0))
  end)
  if out == nil or #out < 8 then return nil end
  local first = dominantHand()
  for _, hand in ipairs({ first, 3 - first }) do
    local base = (hand - 1) * 4
    local tip, ray, origin, at = out[base + 1], out[base + 2], out[base + 3], out[base + 4]
    local u, v, beam = nil, nil, false
    if tip.w > 0.5 and tip.z < HOVER and tip.z > THROUGH then
      u, v = tip.x, tip.y
    elseif ray.w > 0.5 then
      u, v, beam = ray.x, ray.y, true
    end
    if u ~= nil and u >= 0 and u <= 1 and v >= 0 and v <= 1 then
      local x, y = u * CW, v * CH
      local dx, dy = x - CX, y - CY
      if dx * dx + dy * dy < (R_ARC + R_CHOICE + 60) ^ 2 then
        return { hand = hand, x = x, y = y, beam = beam, origin = { origin.x, origin.y, origin.z },
          at = { at.x, at.y, at.z }, length = ray.z }
      end
    end
  end
  return nil
end

local function buildBeam(h)
  h.root:RemoveAllChildren()
  h.strip = K.beamBuild(h.root, K.CYAN, 0.25, K.WHITE, 0.9)
  h.built = true
end

local function showBeam(p, view)
  local h = S.beam
  if h == nil or not resolve(h, UI) then return end
  if not h.built then buildBeam(h) end
  if not h.gpu then
    local ok, done = pcall(VRMenuSurfaceReady, h.comp, h.root)
    h.gpu = ok and done == true
    if not h.gpu then return end
  end
  if p == nil or not p.beam or view == nil then
    display(h, false)
    return
  end
  local dir = norm(sub(p.at, p.origin))
  local toCam = sub(view.p, p.origin)
  local n = norm(sub(toCam, mul(dir, dot(toCam, dir))))
  if n[1] == 0 and n[2] == 0 and n[3] == 0 then return end
  place(h, transformFor(add(p.origin, mul(dir, BEAM_LEN / 2)), cross(n, dir), mul(dir, -1)))
  K.beamLength(h.strip, p.length)
  display(h, true)
end

local function paintHover(h, hover)
  for i, cell in ipairs(h.cells) do
    local on = hover ~= nil and hover.index == i and hover.choice == nil and not cell.empty
    if on then
      cell.glow:SetOpacity(cell.blocked and .15 or (cell.band and .45 or .5))
      K.tint(cell.edge, cell.blocked and K.RED or K.YELLOW, 1)
    else
      rest(cell)
    end
  end
  for k, cell in ipairs(h.choiceCells or {}) do
    local on = hover ~= nil and hover.index == S.arc and hover.choice == k
    cell.glow:SetOpacity(on and .5 or 0)
    K.tint(cell.edge, (on or cell.current) and K.YELLOW or K.CYAN, on and 1 or (cell.current and .9 or .6))
  end
  local e = hover and hover.index and S.slots[hover.index]
  if e ~= nil then
    local item = hover.choice and e.choices[hover.choice] or nil
    local name = item and item.name or text(e)
    h.hubName:SetText(K.clip(name, 18))
    -- an item with no type of its own (a consumable) reads "None"
    local kind = (item or e.item) and (item or e.item).typeName or ""
    if kind == "None" then kind = "" end
    h.hubInfo:SetText(e.blocked and L.tr("Недоступно", "Unavailable") or kind)
    K.tint(h.hubMark, K.GREY, 0)
  else
    h.hubName:SetText("")
    h.hubInfo:SetText("")
    K.tint(h.hubMark, hover and hover.hub and K.YELLOW or K.GREY, hover and hover.hub and 1 or .7)
  end
  K.tint(h.hubRing, hover and hover.hub and K.YELLOW or K.CYAN, hover and hover.hub and 1 or .5)
  h.hubGlow:SetOpacity(hover and hover.hub and .35 or .12)
end

---------------------------------------------------------------- open / close

local function prepare(b)
  if S.screen ~= nil and S.screen.id ~= nil then return end
  S.anchor = anchorFor(b)
  local C, R, U = frame(b)
  if C == nil then return end
  local tr = transformFor(C, R, U)
  S.screen = spawn(ENT, tr)
  S.beam = spawn(BEAM_ENT, tr)
  S.glass = spawn(DISC_ENT, tr)
  S.choiceGlass = {}
  if S.ctx == nil then
    local ok, ctx = pcall(I.open)
    S.ctx = ok and ctx or nil
  end
  if type(SetVRDiagCapture) == "function" then pcall(SetVRDiagCapture, 1) end
  S.usedAt = S.time
end

local function open(pl, b)
  S.anchor = anchorFor(b)
  if S.anchor == nil or S.ctx == nil then return false end
  S.lang = D.language()
  L.set(S.lang)
  S.slots = gather(pl)
  S.open, S.hover, S.arc, S.arcAway, S.hand = true, nil, nil, nil, nil
  S.filled = false
  S.openedAt = S.time
  sound("open")
  return true
end

local function close(applied)
  if not S.open then return end
  S.open = false
  local h = S.screen
  if h ~= nil then
    display(h, false)
    if h.cursor ~= nil then h.cursor:SetOpacity(0) end
  end
  display(S.beam, false)
  showGlass(S.glass, false)
  for _, g in ipairs(S.choiceGlass) do showGlass(g, false) end
  if applied then
    sound("click")
    haptic(S.hand)
  else
    sound("close")
  end
  S.hover, S.arc = nil, nil
  S.usedAt = S.time
end

-- The matte glass: one disc under the wheel, one under each choice on the open arc.
local function placeGlass(C, R, U, h)
  local N = cross(R, U)
  if S.glass ~= nil and resolve(S.glass, "display") then
    place(S.glass, transformFor(add(C, mul(N, -GLASS_BACK)), R, U))
    showGlass(S.glass, true, 2 * (R_RIM + 10) / PX)
  end
  local spots = h.choiceCells or {}
  for k = 1, math.max(#spots, #S.choiceGlass) do
    local g = S.choiceGlass[k]
    local c = spots[k]
    if c ~= nil then
      if g == nil then
        g = spawn(DISC_ENT, transformFor(C, R, U))
        S.choiceGlass[k] = g
      end
      if resolve(g, "display") then
        local at = add(add(add(C, mul(R, (c.x - CX) / PX)), mul(U, (CY - c.y) / PX)), mul(N, -GLASS_BACK))
        place(g, transformFor(at, R, U))
        showGlass(g, true, 2 * (R_CHOICE + 6) / PX)
      end
    elseif g ~= nil then
      showGlass(g, false)
    end
  end
end

---------------------------------------------------------------- the frame

local function tick(dt, menuOpen)
  local pl = Game.GetPlayer()
  if S.time - S.allowedAt > 0.1 then
    S.allowedAt = S.time
    S.allowedValue = allowedNow(pl, menuOpen)
    S.promptOnY = promptOnY()
  end
  local allowed = S.allowedValue
  local hold = 0
  if type(VRWeaponWheel) == "function" then
    local ok, v = pcall(VRWeaponWheel, (allowed and not S.promptOnY) or S.open)
    hold = ok and v or 0
  end
  -- a cigarette waiting for the weapons to be put away (smoke)
  if S.smokeAt ~= nil then
    if next(inHand(pl)) == nil then
      S.smokeAt = nil
      pcall(function() if not pl:VRSmokeHasCig() then pl:VRSmokeToggle() end end)
    elseif S.time - S.smokeAt > SMOKE_WAIT then
      S.smokeAt = nil
    end
  end
  local b = body()
  if b == nil then return end

  -- spawned on the press and built while the hold is undecided, so it is ready by the time it is a hold
  if hold >= 1 and allowed then prepare(b) end
  local h = S.screen
  if h ~= nil and resolve(h, UI) then
    if not h.built then buildFrame(h) end
    if not h.gpu then
      local ok, done = pcall(VRMenuSurfaceReady, h.comp, h.root)
      h.gpu = ok and done == true
    end
  end
  if hold == 2 and not S.open and allowed then open(pl, b) end
  if S.open and (not allowed or hold == 0) then
    close(hold == 0 and allowed and apply(S.hover))
  end
  if not S.open then
    if S.screen ~= nil and S.time - S.usedAt > IDLE then release() end
    return
  end

  if h == nil or not h.built then return end
  if not S.filled then
    fillCells(h)
    S.filled = true
    S.arcShown = false
  end
  fitPictures()
  local C, R, U = frame(b)
  if C == nil then return end
  place(h, transformFor(C, R, U))
  if not h.gpu then return end
  if h.warm ~= nil then h.root:RemoveChild(h.warm); h.warm = nil end
  display(h, true)

  local view = Space.view()
  local p = point(C, R, U)
  local hover = p and hit(p.x, p.y) or nil
  -- the arc opens on a hotkey's cell with a choice to make, and stays out while the pointer is off the wheel
  -- (a hand sweeping out to it may leave it for a moment). Another cell folds it only once the pointer has
  -- stayed there ARC_KEEP: a hand going straight from the cell to a choice at the arc's far end crosses the
  -- neighbouring cells on the way. The hub folds it at once.
  if hover ~= nil and hover.index ~= nil and hover.choice == nil and hover.index ~= S.arc then
    S.arcAway = S.arcAway or S.time
    if S.arc == nil or S.time - S.arcAway >= ARC_KEEP then
      -- out when there is something to choose besides the hotkey's own item
      local e = S.slots[hover.index]
      S.arc = (e.sector.kind == "misc" and e.choices ~= nil and #e.choices > (e.item and 1 or 0)) and hover.index or nil
      S.arcAway = nil
    end
  else
    S.arcAway = nil
    if hover ~= nil and hover.hub then S.arc = nil end
  end
  if S.arcShown ~= S.arc then
    drawChoices(h, S.arc)
    S.arcShown = S.arc
    S.hoverPainted = false
  end
  if not same(hover, S.hover) or not S.hoverPainted then
    paintHover(h, hover)
    S.hover = hover
    S.hoverPainted = true
  end
  S.hand = p and p.hand or S.hand
  if p ~= nil then
    K.move(h.cursor, p.x - 32, p.y - 32)
    h.cursor:SetOpacity(1)
  else
    h.cursor:SetOpacity(0)
  end
  placeGlass(C, R, U, h)
  showBeam(p, view)
  S.stats = { hand = p and p.hand, x = p and p.x, y = p and p.y, hover = hover, C = C }
end

---------------------------------------------------------------- driven by init.lua

function Wheel.update(dt, menuOpen)
  S.time = S.time + (dt or 0)
  if Game.GetPlayer() == nil then
    if type(VRWeaponWheel) == "function" then pcall(VRWeaponWheel, false) end
    if S.screen ~= nil or S.ctx ~= nil then
      S.screen, S.beam, S.glass, S.choiceGlass, S.ctx = nil, nil, nil, {}, nil
      S.open, S.hover, S.arc = false, nil, nil
    end
    return
  end
  local ok, err = pcall(tick, dt or 0, menuOpen)
  if not ok then
    print("[VRWheel] " .. tostring(err))
    S.lastError = tostring(err)
    pcall(close, false)
  end
end

function Wheel.release()
  if type(VRWeaponWheel) == "function" then pcall(VRWeaponWheel, false) end
  pcall(close, false)
  release()
end

-- For the harness and the console: GetMod("CyberpunkVRPort_Menu").wheel()
function Wheel.state()
  local slots = {}
  for i, e in ipairs(S.slots) do
    slots[i] = { kind = e.sector.kind, angle = e.sector.angle, item = e.item and e.item.name, active = e.active,
      blocked = e.blocked, quality = e.item and e.item.quality, ammo = e.item and e.item.ammo,
      quantity = e.item and e.item.quantity, choices = e.choices and #e.choices or nil }
  end
  return { open = S.open, allowed = S.allowedValue, hover = S.hover, arc = S.arc, slots = slots,
    screen = S.screen ~= nil and S.screen.id ~= nil, gpu = S.screen and S.screen.gpu, stats = S.stats,
    lastError = S.lastError, anchor = S.anchor }
end

return Wheel
