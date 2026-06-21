-- CyberpunkVRPort — hand-to-visual-holster equip.
--
-- Outfit clothing items are entGarmentSkinnedMeshComponent — skinned to bones on the GPU, no CPU
-- world transform. We approximate the prop handle as a point offset OUTWARD from the hip bone in
-- the body's lateral direction (plugin publishes the distances). Side is classified by the equipped
-- outfit item names (katana / pistol / holster keywords).
--
-- Plugin (after VRIK FK):
--   shared[20] = right wrist -> right hip prop point  (hip + bodyRight * 0.18m)
--   shared[21] = right wrist -> left  hip prop point  (hip - bodyRight * 0.05m, closer for cross-body reach)
--   shared[22] = right wrist -> back-strap anchor (over right shoulder, for rifle/sniper)
--   shared[49] = right grip binary
--
-- Zone -> equip:
--   B (back, no VH required) -> primary weapon (slot 1, rifle/sniper)
--   L (left hip, VH katana)  -> melee
--   R (right hip, VH pistol) -> one-handed ranged (pistol — not sniper)

local PROX_R           = 0.25
local PROX_L           = 0.40
local PROX_BACK        = 0.35
local EQUIP_COOLDOWN_S = 0.6

local LEFT_SLOTS = {
    "OutfitSlots.ThighLeft", "OutfitSlots.KneeLeft", "OutfitSlots.AnkleLeft",
}
local RIGHT_SLOTS = {
    "OutfitSlots.ThighRight", "OutfitSlots.KneeRight", "OutfitSlots.AnkleRight",
}
local MELEE_KW  = { "katana", "sword", "blade", "_eqh", "s10_military", "knife", "machete" }
local RANGED_KW = { "pistol", "revolver", "holster" }

local TransactionSystem = nil
local SlotIDs = nil
local WeaponRightSlot = nil
local cache = { L = nil, R = nil }
local sinceScan = 0.0
local rightGripPrev = 0
local cooldown = 0.0
local lastZone = nil

local function classify(name)
    local lc = name:lower()
    for _, kw in ipairs(MELEE_KW)  do if lc:find(kw, 1, true) then return "melee"  end end
    for _, kw in ipairs(RANGED_KW) do if lc:find(kw, 1, true) then return "ranged" end end
    return nil
end

local function classifySide(pl, slots)
    for _, slotName in ipairs(slots) do
        local item = TransactionSystem:GetItemInSlot(pl, SlotIDs[slotName])
        if item then
            local nm = TDBID.ToStringDEBUG(item:GetItemID():GetTDBID())
            local cls = classify(nm)
            if cls then return cls end
        end
    end
    return nil
end

local function rescan(pl)
    cache.L = classifySide(pl, LEFT_SLOTS)
    cache.R = classifySide(pl, RIGHT_SLOTS)
end

registerForEvent('onInit', function()
    TransactionSystem = Game.GetTransactionSystem()
    WeaponRightSlot = TweakDBID.new("AttachmentSlots.WeaponRight")
    SlotIDs = {}
    for _, s in ipairs(LEFT_SLOTS)  do SlotIDs[s] = TweakDBID.new(s) end
    for _, s in ipairs(RIGHT_SLOTS) do SlotIDs[s] = TweakDBID.new(s) end
end)

registerForEvent('onUpdate', function(dt)
    dt = dt or 0.016
    cooldown = math.max(0.0, cooldown - dt)

    if type(GetVRSharedSlot) ~= 'function' then return end
    local pl = Game.GetPlayer()
    if not pl then return end

    sinceScan = sinceScan + dt
    if sinceScan >= 1.0 then sinceScan = 0.0; rescan(pl) end

    local g = (GetVRSharedSlot(49) > 0.5) and 1 or 0
    local edge = (g == 1 and rightGripPrev == 0)
    rightGripPrev = g
    if not edge or cooldown > 0 then return end

    local dR = GetVRSharedSlot(20)
    local dL = GetVRSharedSlot(21)
    local dB = GetVRSharedSlot(22)
    if dR < 0 then dR = 1e9 end
    if dL < 0 then dL = 1e9 end
    if dB < 0 then dB = 1e9 end

    -- Which body zone is the right hand reaching to? (shared by both modes)
    local zone
    if dB < PROX_BACK and dB <= math.min(dL, dR) then
        zone = "B"
    elseif dL < PROX_L and dL <= dR then
        zone = "L"
    elseif dR < PROX_R then
        zone = "R"
    end
    if not zone then return end

    -- Reliable "is a weapon in the right hand": read the WeaponRight slot directly. GetActiveWeapon()
    -- can return null / lag for a melee weapon right after an equip (or while it's in a lowered state),
    -- which would wrongly reset lastZone and send us into the RE-EQUIP branch instead of holstering —
    -- exactly the "it equips again instead of putting away" the katana shows.
    local rightItem = TransactionSystem:GetItemInSlot(pl, WeaponRightSlot)
    local hasWeapon = rightItem ~= nil

    -- shared[23]: 0/unset = immersive (classify by visual holster), 1 = simple slot mapping.
    -- (Inverted on the plugin side so the zero-initialised shared block defaults to immersive.)
    local simpleHolsters = (GetVRSharedSlot(23) > 0.5)

    if simpleHolsters then
        -- SIMPLE SLOT MODE: visual holsters ignored. Over-shoulder/back = EquipmentSlot1,
        -- right hip = EquipmentSlot2, left hip = EquipmentSlot3. Reaching the same zone again
        -- with a weapon out puts it away (melee routed through VRHolsterMelee to kill the swing).
        spdlog.info(string.format("[Holster] simple grip zone=%s lastZone=%s hasWpn=%s",
            tostring(zone), tostring(lastZone), tostring(hasWeapon)))
        if lastZone == zone and hasWeapon then
            local isMelee = (classify(TDBID.ToStringDEBUG(rightItem:GetItemID():GetTDBID())) == "melee")
            if isMelee then pl:VRHolsterMelee() else pl:VRUnequipWeapon() end
            lastZone = nil
            spdlog.info("[Holster] simple -> HOLSTER")
        else
            if zone == "B" then
                pl:VREquipPrimaryWeapon()     -- EquipmentSlot1
            elseif zone == "R" then
                pl:VREquipWeaponSlot2()        -- EquipmentSlot2
            else
                pl:VREquipWeaponSlot3()        -- EquipmentSlot3
            end
            lastZone = zone
            spdlog.info("[Holster] simple -> equip slot for zone " .. tostring(zone))
        end
        cooldown = EQUIP_COOLDOWN_S
        return
    end

    -- IMMERSIVE MODE: equip is chosen by which visual holster the hand reaches.
    rescan(pl)
    local cls
    if zone == "B" then
        cls = "primary"
    elseif zone == "L" then
        cls = cache.L
    else
        cls = cache.R
    end
    if not cls then return end

    spdlog.info(string.format("[Holster] grip zone=%s cls=%s lastZone=%s hasWpn=%s",
        tostring(zone), tostring(cls), tostring(lastZone), tostring(hasWeapon)))

    if lastZone == zone and hasWeapon then
        -- Melee uses VRHolsterMelee: it suppresses the VR swing (so reaching to the holster doesn't fire
        -- an attack that blocks the unequip) and then sends the same UnequipWeapon the keyboard B does.
        -- Ranged never swings, so the plain UnequipWeapon already works for it.
        if cls == "melee" then pl:VRHolsterMelee() else pl:VRUnequipWeapon() end
        lastZone = nil
        spdlog.info("[Holster] -> HOLSTER " .. tostring(cls))
    elseif cls == "melee" then
        pl:VREquipMeleeWeapon(); lastZone = zone
        spdlog.info("[Holster] -> equip melee")
    elseif cls == "primary" then
        pl:VREquipPrimaryWeapon(); lastZone = zone
        spdlog.info("[Holster] -> equip primary")
    else
        pl:VREquipOneHandedRangedWeapon(); lastZone = zone
        spdlog.info("[Holster] -> equip ranged")
    end
    cooldown = EQUIP_COOLDOWN_S
end)
