local dwell = require('aim_dwell')
local M = {state = {traces = 0, hits = 0, active = 0}}
local session = dwell.new()
local nextPoll, nextRefresh = 0, 0
local filter, slots, playerId

local function notify(target, on)
    local ok, err = pcall(function() target:VRPortSetWeaponAim(on) end)
    -- A fleeing crowd member can despawn before cleanup.
    if not ok and on then M.state.error = tostring(err) end
end

function M.clear()
    for _, target in pairs(dwell.clear(session)) do notify(target, false) end
    M.state.active = 0
    nextPoll, nextRefresh, playerId = 0, 0, nil
end

local function gameplayState(player, defs)
    if not player or not player:IsAttached() or player:IsDead() then return nil end
    local requests = Game.GetSystemRequestsHandler()
    if not requests or requests:IsPreGame() or requests:IsGamePaused() or GetVRSharedSlot(81) > .5 then return nil end
    local ui = Game.GetBlackboardSystem():Get(defs.UI_System)
    if not ui or ui:GetBool(defs.UI_System.IsInMenu) then return nil end
    local psm = player:GetPlayerStateMachineBlackboard()
    if not psm or psm:GetInt(defs.PlayerStateMachine.SceneTier) >= 3 then return nil end
    local upper = psm:GetInt(defs.PlayerStateMachine.UpperBody)
    if upper == 4 or upper == 5 then return nil end -- TemporaryUnequip / ForceEmptyHands
    return psm
end

local function drawnGun(player, psm, defs, weapon, side, slotSide)
    if not weapon or not weapon:IsA('gameweaponObject') or not weapon:IsAttached()
        or not player:VRPortHasEquippedAimWeapon(weapon) or not weapon:IsRanged() then return false end
    local item = weapon:GetItemID()
    local left = side == 0
    local active = EquipmentSystem.GetSlotActiveItem(player,
        left and EquipmentManipulationRequestSlot.Left or EquipmentManipulationRequestSlot.Right)
    -- A lone left primary can still be exposed through WeaponRight. Use its
    -- logical equipment state if the left equipment machine has no active item.
    if side ~= slotSide and not ItemID.IsValid(active) then
        left = false
        active = EquipmentSystem.GetSlotActiveItem(player, EquipmentManipulationRequestSlot.Right)
    end
    if psm:GetInt(left and defs.EquipmentLeft or defs.EquipmentRight) ~= 0 then return false end -- equipmentStates.Equipped
    if not ItemID.IsValid(active) or not ItemID.IsValid(item)
        or ItemID.GetCombinedHash(active) ~= ItemID.GetCombinedHash(item) then return false end
    local kind = WeaponObject.GetWeaponType(item)
    return UIInventoryItemsManager.IsItemTypeRangedWeapon(kind) or WeaponObject.IsOfType(item, gamedataItemType.Wea_GrenadeLauncher)
end

local function finite(v)
    return type(v) == 'number' and v == v and math.abs(v) < math.huge
end

local function hit(weapon)
    local transform = weapon:GetMuzzleSlotWorldTransform()
    if not transform then return nil end
    local q = transform.Orientation or (transform.GetOrientation and transform:GetOrientation())
    local p = transform.position or (transform.GetPosition and transform:GetPosition())
    if not (p and q and finite(q.i) and finite(q.j) and finite(q.k) and finite(q.r)) then return nil end
    local norm = q.i*q.i + q.j*q.j + q.k*q.k + q.r*q.r
    if math.abs(norm - 1) > .01 then return nil end
    if type(p.x) ~= 'number' then p = WorldPosition.ToVector4(p) end
    if not (finite(p.x) and finite(p.y) and finite(p.z)) then return nil end
    local f = Quaternion.GetForward(q)
    local origin = Vector4.new(p.x, p.y, p.z, 1)
    local endpoint = Vector4.new(p.x + f.x*30, p.y + f.y*30, p.z + f.z*30, 1)
    filter = filter or QueryFilter.ALL()
    M.state.traces = M.state.traces + 1
    local found, result = Game.GetSpatialQueriesSystem():SyncRaycastByQueryFilter(origin, endpoint, filter, false, false)
    if not (found and result) then return nil end
    local target = TraceResult.GetHitEntity(result)
    if not target or not target:IsA('NPCPuppet') or not target:IsAttached() or not target:IsActive() then return nil end
    M.state.hits = M.state.hits + 1
    return {id = tostring(target:GetEntityID().hash), target = target, weapon = tostring(weapon:GetEntityID().hash)}
end

local function observe(observations, player, psm, defs, side, slotSide, weapon, hand)
    if not (hand.w > 0 and GetVRSharedSlot(side * 8) > .5) then return false end
    if not drawnGun(player, psm, defs, weapon, side, slotSide) then return false end
    local ok, result = pcall(hit, weapon)
    if ok and result then
        result.origin, result.slotSide = hand.w, slotSide
        observations[side] = result
    elseif not ok then
        M.state.error = tostring(result)
    end
    return true -- This equipped barrel owns the hand even when the ray misses.
end

function M.update(player, now)
    now = now or os.clock()
    -- Poll for acquisition at 10 Hz. Once a barrel has hit, observe every game
    -- update so even a one-frame holster, obstruction or tracking loss breaks dwell.
    if not next(session.candidates) and session.last and now >= session.last and now < nextPoll then return end
    nextPoll = now + .1
    local defs = Game.GetAllBlackboardDefs()
    local psm = gameplayState(player, defs)
    if not psm then M.clear(); return end
    local id = tostring(player:GetEntityID().hash)
    if playerId and playerId ~= id then M.clear() end
    playerId = id
    -- The native frame rejects stale tracking and head-aim fallback. Positions
    -- are ordered support hand first, weapon hand second. W identifies the origin.
    local primaryLeft = Game.VRWeaponInLeftHand()
    local frame = Game.VRMeleeInputFrame()
    local observations = {}
    local stamp = frame and frame[1] and frame[1].x
    if frame and #frame >= 3 and finite(stamp) and primaryLeft == Game.VRWeaponInLeftHand() then
        slots = slots or {TweakDBID.new('AttachmentSlots.WeaponLeft'), TweakDBID.new('AttachmentSlots.WeaponRight')}
        local transaction = Game.GetTransactionSystem()
        local left = transaction:GetItemInSlot(player, slots[1])
        local right = transaction:GetItemInSlot(player, slots[2])
        if primaryLeft then
            -- WeaponInLeftHand identifies a single physical left primary. Its
            -- tracking is frame[3] and shared slot 0 whichever attachment exposed it.
            if not observe(observations, player, psm, defs.PlayerStateMachine, 0, 0, left, frame[3]) then
                observe(observations, player, psm, defs.PlayerStateMachine, 0, 1, right, frame[3])
            end
        else
            -- Both lookups can expose one entity during a reattach. The
            -- support controller cannot establish a second dwell for it.
            if left and not (right and left:GetEntityID().hash == right:GetEntityID().hash) then
                observe(observations, player, psm, defs.PlayerStateMachine, 0, 0, left, frame[2])
            end
            observe(observations, player, psm, defs.PlayerStateMachine, 1, 1, right, frame[3])
        end
    end
    local previous = session.active
    if session.last and now < session.last then nextRefresh = 0 end
    local refresh = stamp ~= session.stamp and now >= nextRefresh
    local active, stopped = dwell.step(session, now, stamp, observations)
    for _, target in pairs(stopped) do notify(target, false) end
    if refresh then nextRefresh = now + .1 end
    M.state.active = 0
    for targetId, target in pairs(active) do
        if refresh or not previous[targetId] then notify(target, true) end
        M.state.active = M.state.active + 1
    end
end

return M
