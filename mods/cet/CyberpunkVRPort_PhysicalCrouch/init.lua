-- Physical posture requests the native crouch PSM through its existing status
-- listener. Never toggle R3, scale movement, resize a capsule or write the camera.
local effect
local owner
local ownerId
local graphReady = false
local elapsed = 0
local warned = false

local function feedback(applied, actual)
  if type(SetVRPhysicalCrouchFeedback) == 'function' then
    SetVRPhysicalCrouchFeedback(applied, actual)
  end
end

local function release(player)
  if not player or not effect then return end
  local system = Game.GetStatusEffectSystem()
  if system and system:HasStatusEffect(player:GetEntityID(), effect) then
    system:RemoveStatusEffect(player:GetEntityID(), effect)
  end
end

local function update()
  local player = Game.GetPlayer()
  local idString = player and tostring(player:GetEntityID().hash)
  if owner and ownerId ~= idString then
    pcall(release, owner)
  end
  if ownerId ~= idString then graphReady = false end
  owner = player
  ownerId = idString
  if not player or not effect or not TweakDB:GetRecord(effect) or type(GetVRPhysicalCrouch) ~= 'function' then
    pcall(release, player)
    feedback(-1, 0)
    return
  end
  -- A DLL-only upgrade must not enable crouch without its camera graph asset.
  -- Check once per bound player; before readiness, no physical effect is owned.
  if not graphReady and type(SetRootGraphFloatVariable) == 'function' then
    graphReady = SetRootGraphFloatVariable(CName.new('vr_physical_crouch_camera'), 0.0) > 0
  end
  if not graphReady then pcall(release, player); feedback(-1, 0); return end
  local system = Game.GetStatusEffectSystem()
  if not system then feedback(-1, 0); return end
  local id = player:GetEntityID()
  local actual = player:GetPuppetPS():IsCrouch()
  local applied = system:HasStatusEffect(id, effect)
  local blocked = StatusEffectSystem.ObjectHasStatusEffectWithTag(player, CName.new('ForceStand'))
    or StatusEffectSystem.ObjectHasStatusEffectWithTag(player, CName.new('KeepState'))
  feedback(blocked and -1 or (applied and 1 or 0), actual and 1 or 0)
  local wanted = not blocked and GetVRPhysicalCrouch() ~= 0
  if wanted and not applied then
    local anchor = GetVRPhysicalCrouchCameraAnchor()
    if anchor and anchor.w > 0.5 then
      -- Capture the native camera mount BEFORE crouch changes its animation.
      -- The camera-only graph node keeps this mount; HMD displacement supplies
      -- the real height change. The ordinary crouch pose still owns the legs.
      AnimationControllerComponent.SetInputVector(player, CName.new('vr_physical_crouch_camera_anchor'), anchor)
      StatusEffectHelper.ApplyStatusEffect(player, effect)
    end
  elseif applied and not wanted then
    release(player)
  end
  -- Reconcile with the effect itself, not a remembered toggle edge. A load or a
  -- missed poll cannot invert the next crouch; native manual/quest state is intact.
end

-- THE RUST BUILD RUNS THIS BRIDGE (rust/crates/cvr-ffi/src/mechanics.rs physical_crouch, bit 3 of VRRustMechanics):
-- the 50 ms update, the effect's application and release, the feedback and the shutdown. The hook stays here -- the
-- crouch state's entry is reported to the plugin. The reference build has no such native, and the whole bridge keeps
-- running here.
local rustOwned = nil
local function rustOwnsBridge()
  if rustOwned == nil then
    rustOwned = type(VRRustMechanics) == 'function' and math.floor(VRRustMechanics() / 8) % 2 == 1
  end
  return rustOwned
end

-- THE RUST BUILD HOOKS THESE ITSELF (rust/crates/cvr-ffi/src/mechanics/game_hooks.rs, bit 1 of VRRustMechanics2).
local function rustHooks()
  return type(VRRustMechanics2) == 'function' and math.floor(VRRustMechanics2() / 2) % 2 == 1
end
registerForEvent('onInit', function()
  if rustOwnsBridge() then
    if rustHooks() then return end
    Observe('CrouchEvents', 'OnEnter', function(_, _, scriptInterface)
      if scriptInterface then VRPhysicalCrouchEnter(scriptInterface.executionOwner) end
    end)
    return
  end
  effect = TweakDBID.new('VRPort.PhysicalCrouch')
  Observe('CrouchEvents', 'OnEnter', function(_, _, scriptInterface)
    local player = Game.GetPlayer()
    if not player or not scriptInterface or tostring(scriptInterface.executionOwner:GetEntityID().hash) ~= tostring(player:GetEntityID().hash) then return end
    local physical = Game.GetStatusEffectSystem():HasStatusEffect(player:GetEntityID(), effect)
    AnimationControllerComponent.SetInputFloat(player, CName.new('vr_physical_crouch_camera'), physical and 1.0 or 0.0)
    -- Keep this value through OnExit: the native crouch blend already fades
    -- its entire branch out. Reset on the NEXT manual/native crouch entry,
    -- avoiding a camera dip during the stand-up blend.
  end)
end)
registerForEvent('onUpdate', function(dt)
  if rustOwnsBridge() then return end
  elapsed = elapsed + (dt or 0)
  if elapsed < 0.05 then return end
  elapsed = 0
  local ok, err = pcall(update)
  if not ok then
    pcall(function() release(owner); feedback(-1, 0) end)
    if not warned then
      warned = true
      spdlog.error('[PhysicalCrouch] ' .. tostring(err))
    end
  end
end)
registerForEvent('onShutdown', function()
  if rustOwnsBridge() then return end
  pcall(function() release(owner); feedback(-1, 0) end)
end)
