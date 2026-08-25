-- CyberpunkVRPort_ForceFPP -- the player stays in first person, and cannot be switched out of it.
--
-- WHY IT IS THESE TWO THINGS AND NOT A HOOK. Third person in this game is the VEHICLE camera, and the
-- game already owns both halves of the problem; both were read out of its own scripts rather than
-- guessed:
--
--   BLOCKING THE SWITCH. vehicleTransition.swift acts on `ToggleVehCamera` only when
--   `IsVehicleCameraChangeBlocked` is false, and defaultTransition.swift defines that as
--
--       StatusEffectSystem.ObjectHasStatusEffectWithTag(owner, n"VehicleFPP") || ...VehicleCombatNoInterruptions
--
--   and the game ships the record for it: GameplayRestriction.VehicleFPP. Described from tweakdb.bin, it
--   carries two gameplay tags (GameplayRestriction, VehicleFPP), no packages, no actionRestriction, no
--   stat modifiers and no UI data, with infinite duration -- so applying it does exactly one thing and
--   nothing else. That is the whole block: no override of a native, nothing to fight with other mods.
--
--   PUTTING IT BACK. The restriction stops the toggle but does not move a camera that is already in
--   third person (a vehicle entered in TPP, or a save made there). defaultTransition.swift does that with
--
--       camEvent = new vehicleRequestCameraPerspectiveEvent(); camEvent.cameraPerspective = ...;
--       scriptInterface.executionOwner.QueueEvent(camEvent)
--
--   i.e. the event goes to the PLAYER, not to the vehicle, and vehicleCameraPerspective.FPP is 0.
--
-- POLLED TWICE A SECOND, deliberately. "Has the camera been moved out of first person" does not need
-- frame resolution, and a per-frame trip into the game's systems is expensive -- measured elsewhere in
-- this port at 13.5 ms for a single VirtualQuery and 41 ms for a per-frame component walk. Two checks a
-- second cost nothing and are indistinguishable to the eye.
--
-- THE RESTRICTION IS SAVABLE, so it is removed on shutdown. Otherwise a save made after this mod is
-- taken out would keep a status effect nothing owns any more, and the vehicle camera would stay locked
-- with no way left in the game to explain why.

local RESTRICTION = "GameplayRestriction.VehicleFPP"

local S = {
  on = true,
  remote = false,
  remotePos = "-",
  applied = false,
  forced = 0,          -- how many times the camera was put back
  note = "waiting for the player",
  acc = 0.0,
}

local function statusSystem()
  local s = nil
  pcall(function() s = Game.GetStatusEffectSystem() end)
  return s
end

-- Applied once and then only re-applied if something removed it: a load, a respawn, or a script that
-- clears effects. Checked rather than re-applied blindly, so no stack is added twice.
local function ensureRestriction(pl)
  local sys = statusSystem()
  if sys == nil or pl == nil then return false end
  local has = false
  pcall(function() has = sys:HasStatusEffect(pl:GetEntityID(), RESTRICTION) end)
  if has then
    S.applied = true
    return true
  end
  local ok = pcall(function() sys:ApplyStatusEffect(pl:GetEntityID(), RESTRICTION) end)
  S.applied = ok
  if ok then S.note = "camera switching blocked by the game's own restriction" end
  return ok
end

local function dropRestriction()
  local pl = Game.GetPlayer()
  local sys = statusSystem()
  if pl == nil or sys == nil then return end
  pcall(function() sys:RemoveStatusEffect(pl:GetEntityID(), RESTRICTION) end)
  S.applied = false
end

-- The camera itself. GetActivePerspective is on the vehicle's camera manager, and FPP is the zero
-- member of vehicleCameraPerspective; the request goes to the player.
local function forceFirstPerson(pl)
  local veh = nil
  pcall(function() veh = Game.GetMountedVehicle(pl) end)
  if veh == nil then return end

  local persp = nil
  pcall(function() persp = veh:GetCameraManager():GetActivePerspective() end)
  if persp == nil then return end
  if persp == vehicleCameraPerspective.FPP then return end

  local ok = pcall(function()
    local ev = vehicleRequestCameraPerspectiveEvent.new()
    ev.cameraPerspective = vehicleCameraPerspective.FPP
    pl:QueueEvent(ev)
  end)
  if ok then
    S.forced = S.forced + 1
    S.note = "was in third person -> put back to first"
  else
    S.note = "could not queue the camera request"
  end
end

-- WHICH SURVEILLANCE CAMERA THE PLAYER TOOK OVER, handed to the plugin.
--
-- The camera writer in the plugin recognises cameras by component name, and every surveillance camera in
-- the area is named `cameraComponent` -- measured: 20559 identity changes cycling between four objects,
-- so the second eye attached itself to a camera nobody had activated. The name is not an identity; the
-- position is, and only the script side knows which object is controlled. The plugin cannot ask: its own
-- periodic poll runs on the worker thread, where calling the script VM is not safe in this process.
--
-- So this publishes both the gate and the target four times a second. With nothing published the plugin
-- follows nothing, which is the safe default.
local function publishRemoteCamera()
  if type(VRRemoteCamera) ~= "function" then return end
  local sys = nil
  pcall(function() sys = Game.GetScriptableSystemsContainer():Get(CName.new("TakeOverControlSystem")) end)
  local obj = nil
  if sys ~= nil then pcall(function() obj = sys:GetControlledObject() end) end
  if obj == nil then
    pcall(function() VRRemoteCamera(0, 0.0, 0.0, 0.0) end)
    S.remote = false
    return
  end
  local p = nil
  pcall(function() p = obj:GetWorldPosition() end)
  if p == nil then
    pcall(function() VRRemoteCamera(0, 0.0, 0.0, 0.0) end)
    S.remote = false
    return
  end
  pcall(function() VRRemoteCamera(1, p.x, p.y, p.z) end)
  S.remote = true
  S.remotePos = string.format("%.2f %.2f %.2f", p.x, p.y, p.z)
end

registerForEvent("onInit", function()
  print("[ForceFPP] ready; the vehicle camera will be held in first person")
end)

registerForEvent("onUpdate", function(dt)
  if not S.on then return end
  local pl = Game.GetPlayer()
  if pl == nil then
    S.applied = false            -- a load screen: the effect goes with the old player object
    return
  end
  S.acc = S.acc + (dt or 0.016)
  if S.acc < 0.25 then return end
  S.acc = 0.0
  ensureRestriction(pl)
  forceFirstPerson(pl)
  -- four times a second: fast enough that the second eye reaches the camera within a blink of the
  -- takeover, cheap enough that it is one scriptable-system call and one position read
  publishRemoteCamera()
end)

registerForEvent("onShutdown", function()
  dropRestriction()
end)

local overlay = false
registerForEvent("onOverlayOpen", function() overlay = true end)
registerForEvent("onOverlayClose", function() overlay = false end)

registerForEvent("onDraw", function()
  if not overlay then return end
  pcall(function()
    ImGui.Begin("VR force FPP")
    local b, ch = ImGui.Checkbox("hold the player in first person", S.on)
    if ch then
      S.on = b
      if not b then dropRestriction() end
    end
    ImGui.Text("restriction applied: " .. tostring(S.applied))
    ImGui.Text(string.format("camera put back %d time(s)", S.forced))
    ImGui.Text(S.note)
    ImGui.Text("remote camera: " .. (S.remote and ("yes, at " .. tostring(S.remotePos)) or "no"))
    ImGui.Separator()
    ImGui.Text("Blocking is the game's own: GameplayRestriction.VehicleFPP makes")
    ImGui.Text("IsVehicleCameraChangeBlocked true, so ToggleVehCamera does nothing.")
    ImGui.End()
  end)
end)
