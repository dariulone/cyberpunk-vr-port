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

-- WHAT THIS FILE PAYS FOR EVERY FRAME, and what it no longer pays for.
--
-- Three publishers run on onUpdate (the takeover position has to be current, measured: at four times a
-- second a flying AV is tens of metres out), and each of them was re-resolving the same handles on every
-- one of those frames: `CName.new` hashes its string on every call, a scriptable-system lookup walks the
-- container, and `GetAllBlackboardDefs` / `GetBlackboardSystem` are two more trips into the game per
-- frame. None of those answers change while a session runs, so they are resolved once here and dropped
-- only when the player object goes away -- a load screen replaces the systems and the boards with it.
local memo = {}

local function memoCName(key, text)
  if memo[key] == nil then
    memo[key] = CName.new(text)
  end
  return memo[key]
end

local function memoSystem(key, text)
  if memo[key] == nil then
    pcall(function()
      memo[key] = Game.GetScriptableSystemsContainer():Get(memoCName(key .. "_cn", text))
    end)
  end
  return memo[key]
end

-- Called when there is no player: everything above belongs to the session that just ended.
local function memoDrop()
  memo = {}
end

local S = {
  on = true,
  sceneGate = true,    -- open the scene-camera path for ORDINARY scenes, not only braindances
  sceneInTakeover = false,  -- ...and during a device takeover as well: an A/B, see the panel
  latched = false,     -- this scene was judged to need the fix, decided once on entry
  remote = false,
  remotePos = "-",
  applied = false,
  forced = 0,          -- how many times the camera was put back
  note = "waiting for the player",
  acc = 0.0,
  owns = false,        -- the scene system says it is driving the camera
  tier = -1,
  fppFov = 0.0,
}

-- THE TAKEN-OVER CAMERA'S VIEWPOINT, three numbers only the person looking can choose. Kept in a file
-- next to this mod so a session does not start by re-dragging them, and pushed to the plugin whenever
-- they change or the mod loads. The plugin clamps them to +-3 m and applies them in the LENS's own frame:
-- right, forward, up.
local DEVCAM_FILE = "devcam.json"

-- THE VIEWPOINT OFFSET, BAKED PER KIND OF TAKEN-OVER CAMERA.
--
-- The AV turret's mission frames the eye somewhere a seated player's head is not, and these are the
-- numbers that fixed it in the headset. They are NOT global: a surveillance camera on a wall renders from
-- a place that is already right, and shifting it by half a metre would break a mechanism that works. So
-- the offset is keyed by what was taken over, the AV entry ships with the tuned values, and everything
-- else ships with zero.
--
-- Kept as a table rather than three numbers so the next mission that needs its own framing is one line.
local DEVCAM_BAKED = {
  av      = { right = -0.514, forward = 0.100, up = -0.224 },   -- AV turret, tuned 2026-09-01
  default = { right =  0.000, forward = 0.000, up =  0.000 },
}

local dev = { right = 0.0, forward = 0.0, up = 0.0 }   -- the ACTIVE context, i.e. what the plugin is given
local devUser = {}                                     -- per-context overrides read from devcam.json
local devCtx = "default"

local function devcamPush()
  if type(VRDevCamOffset) ~= "function" then return end
  pcall(function() VRDevCamOffset(dev.right, dev.forward, dev.up) end)
end

-- Which set applies: an override the player saved for this context, otherwise the baked one.
local function devcamApply(ctx)
  ctx = ctx or "default"
  devCtx = ctx
  local src = devUser[ctx] or DEVCAM_BAKED[ctx] or DEVCAM_BAKED.default
  dev.right, dev.forward, dev.up = src.right, src.forward, src.up
  devcamPush()
end

-- WHAT WAS TAKEN OVER, as the class the game itself reports. Measured in that seat: the AV comes back as
-- `vehicleAVBaseObject`, a wall camera as a device. Class and not a quest id on purpose -- the framing
-- problem belongs to the mount, and the next AV turret in another mission needs the same answer.
local function devcamContextFor(obj)
  if obj == nil then return "default" end
  local cls = ""
  pcall(function() cls = tostring(obj:GetClassName().value) end)
  if string.find(cls, "AV", 1, true) then return "av" end
  return "default"
end

local function devcamLoad()
  local f = io.open(DEVCAM_FILE, "r")
  if f == nil then return end
  local text = f:read("*a")
  f:close()
  local function triple(chunk)
    if chunk == nil then return nil end
    local t = {}
    for _, k in ipairs({ "right", "forward", "up" }) do
      local v = chunk:match('"' .. k .. '"%s*:%s*(-?%d+%.?%d*)')
      if v == nil then return nil end
      t[k] = tonumber(v) or 0.0
    end
    return t
  end
  local any = false
  for ctx in pairs(DEVCAM_BAKED) do
    local block = text:match('"' .. ctx .. '"%s*:%s*{(.-)}')
    local t = triple(block)
    if t ~= nil then devUser[ctx] = t; any = true end
  end
  -- THE OLD FLAT FILE, read as the AV context: that is the only thing it was ever tuned against, and
  -- silently dropping it would move the picture the player already approved.
  if not any then
    local t = triple(text)
    if t ~= nil then devUser.av = t end
  end
end

-- ONLY WHAT THE PLAYER HAS ACTUALLY OVERRIDDEN goes in the file. Writing every context would freeze the
-- baked values into a copy on the first slider move, and then a better default shipped later would never
-- reach anyone who had opened the panel once.
local function devcamSave(drop)
  if drop then
    devUser[devCtx] = nil
  else
    devUser[devCtx] = { right = dev.right, forward = dev.forward, up = dev.up }
  end
  local f = io.open(DEVCAM_FILE, "w")
  if f == nil then return end
  local parts = {}
  for _, ctx in ipairs({ "av", "default" }) do
    local t = devUser[ctx]
    if t ~= nil then
      parts[#parts + 1] = string.format('  "%s": { "right": %.3f, "forward": %.3f, "up": %.3f }',
                                        ctx, t.right, t.forward, t.up)
    end
  end
  if #parts == 0 then f:write("{}\n") else f:write("{\n" .. table.concat(parts, ",\n") .. "\n}\n") end
  f:close()
end

-- THE LENS POSE, AS A SCENE POSE. The braindance branch needs exactly one thing to engage: a valid pose
-- for the camera the engine renders through. In a braindance the scene system publishes it; in a takeover
-- nothing does, and the scene system's own pose points at a cinematic camera somewhere else -- which is
-- why feeding it made MAIN teleport. The taken-over camera's component has the pose, so it is published
-- as the scene pose and every braindance mechanism then works on its ordinary path: the fov write finds
-- the view by this pose, MAIN's base comes from the located buffer, and the push puts the second eye on
-- MAIN's own composition plus half an IPD.
local function lensPose(obj)
  local cam = nil
  pcall(function()
    for _, c in ipairs(obj:GetComponents()) do
      if string.find(tostring(c:GetClassName().value), "CameraComponent") then cam = c break end
    end
  end)
  if cam == nil then return nil end
  local m = nil
  pcall(function() m = cam:GetLocalToWorld() end)
  if m == nil or m.W == nil then return nil end
  -- Columns are the rotated basis: X right, Y forward, Z up, W the world position. Standard trace
  -- conversion, the branch picked by the largest diagonal term so no square root goes near zero.
  local m11, m21, m31 = m.X.x, m.X.y, m.X.z
  local m12, m22, m32 = m.Y.x, m.Y.y, m.Y.z
  local m13, m23, m33 = m.Z.x, m.Z.y, m.Z.z
  local tr = m11 + m22 + m33
  local i, j, k, r
  if tr > 0.0 then
    local s = math.sqrt(tr + 1.0) * 2.0
    r = 0.25 * s
    i = (m32 - m23) / s
    j = (m13 - m31) / s
    k = (m21 - m12) / s
  elseif m11 > m22 and m11 > m33 then
    local s = math.sqrt(1.0 + m11 - m22 - m33) * 2.0
    r = (m32 - m23) / s
    i = 0.25 * s
    j = (m12 + m21) / s
    k = (m13 + m31) / s
  elseif m22 > m33 then
    local s = math.sqrt(1.0 + m22 - m11 - m33) * 2.0
    r = (m13 - m31) / s
    i = (m12 + m21) / s
    j = 0.25 * s
    k = (m23 + m32) / s
  else
    local s = math.sqrt(1.0 + m33 - m11 - m22) * 2.0
    r = (m21 - m12) / s
    i = (m13 + m31) / s
    j = (m23 + m32) / s
    k = 0.25 * s
  end
  local n = math.sqrt(i * i + j * j + k * k + r * r)
  if n < 0.0001 then return nil end
  return { x = m.W.x, y = m.W.y, z = m.W.z, i = i / n, j = j / n, k = k / n, r = r / n }
end

-- WHICH UI OVERLAY OWNS THE B BUTTON: the phone, the radio port, the vehicle list.
--
-- None of the three is a menu -- the game's menu mode stays 0 for all of them -- so the port's gameplay
-- rules still apply to the pad, and B, which is Exit_Button in every one of them, was being held back for
-- the physical reload's magazine drop. With a weapon in hand there was no way to close any of them.
--
-- The plugin cannot look for itself: its polling runs on a worker thread where a call into the scripting
-- system is not safe. So this publishes the answer, and the plugin does two things with it -- lets B
-- through while an overlay is up, and blocks the magazine drop for a moment after it closes, so a spammed
-- close press cannot eject a magazine into gameplay.
--
-- THE PHONE IS A BLACKBOARD BOOL and costs one read, so it is asked every frame. THE OTHER TWO ARE
-- WIDGETS in the notification container -- the same two this port already scaled, identified by the marker
-- names in their own subtrees (RadioportTitle, vehicleIconContainer) rather than by the container being
-- non-empty, which quest notifications also make true. A widget walk is not free, so it runs ten times a
-- second: the state only has to be right BEFORE a press, and the plugin's own block window covers the rest.
local ui = { acc = 0.0, widget = false, sent = nil, what = "-" }

local function numChildren(w)
  local n = 0
  pcall(function() n = w:GetNumChildren() end)
  return n
end

local function childAt(w, i)
  local c = nil
  pcall(function() c = w:GetWidgetByIndex(i) end)
  return c
end

local function widgetName(w)
  local nm = "?"
  pcall(function() nm = tostring(w:GetName().value) end)
  return nm
end

local function subtreeHas(w, want, depth)
  if depth > 6 then return false end
  local n = numChildren(w)
  for k = 0, n - 1 do
    local c = childAt(w, k)
    if c ~= nil then
      if widgetName(c) == want then return true end
      if subtreeHas(c, want, depth + 1) then return true end
    end
  end
  return false
end

local function findNamed(w, want, depth)
  if depth > 4 then return nil end
  local n = numChildren(w)
  for k = 0, n - 1 do
    local c = childAt(w, k)
    if c ~= nil then
      if widgetName(c) == want then return c end
      local deeper = findNamed(c, want, depth + 1)
      if deeper ~= nil then return deeper end
    end
  end
  return nil
end

-- THE CONTAINER IS FOUND ONCE, not ten times a second.
--
-- The first version walked EVERY ink layer to depth four looking for NotificationsContainer, and did it on
-- every probe -- hundreds of GetNumChildren / GetWidgetByIndex / GetName calls across the script boundary,
-- ten times a second, whether or not anything was open. The container is part of the HUD and outlives every
-- popup, so it is remembered; the walk happens again only if it goes away (a load screen), which the cheap
-- child-count read below detects.
local function notificationsContainer()
  if memo.notif ~= nil then
    local alive = false
    pcall(function() alive = memo.notif:GetNumChildren() >= 0 end)
    if alive then
      return memo.notif
    end
    memo.notif = nil
  end
  pcall(function()
    local layers = Game.GetInkSystem():GetLayers()
    for i = 1, #layers do
      local vw = nil
      pcall(function() vw = layers[i]:GetVirtualWindow() end)
      if vw ~= nil then
        local cont = findNamed(vw, "NotificationsContainer", 0)
        if cont ~= nil then
          memo.notif = cont
          return
        end
      end
    end
  end)
  return memo.notif
end

local function popupWidgetOpen()
  local cont = notificationsContainer()
  if cont == nil then
    return false, "-"
  end
  local hit, what = false, "-"
  -- The steady state is one child count and a visibility read per slot: with nothing open the container
  -- has no visible children and the marker scans below never run.
  pcall(function()
    local n = numChildren(cont)
    for k = 0, n - 1 do
      local slot = childAt(cont, k)
      local vis = false
      if slot ~= nil then pcall(function() vis = slot:IsVisible() end) end
      if vis then
        if subtreeHas(slot, "RadioportTitle", 0) then hit = true; what = "radio"; return end
        if subtreeHas(slot, "vehicleIconContainer", 0) then hit = true; what = "vehicle"; return end
      end
    end
  end)
  return hit, what
end

local function phoneOpen()
  local v = false
  pcall(function()
    if memo.bbDefs == nil then memo.bbDefs = Game.GetAllBlackboardDefs() end
    local defs = memo.bbDefs
    if memo.comDevice == nil then memo.comDevice = Game.GetBlackboardSystem():Get(defs.UI_ComDevice) end
    if memo.comDevice ~= nil then v = memo.comDevice:GetBool(defs.UI_ComDevice.ContactsActive) end
  end)
  return v == true
end

local function publishUiPopup(dt)
  if type(VRUiPopup) ~= "function" then return end
  ui.acc = ui.acc + (dt or 0.016)
  if ui.acc >= 0.1 then
    ui.acc = 0.0
    ui.widget, ui.what = popupWidgetOpen()
  end
  local open = ui.widget
  local what = ui.what
  if phoneOpen() then
    open = true
    what = (what ~= "-") and (what .. "+phone") or "phone"
  end
  S.popup = open
  S.popupWhat = open and what or "-"
  local want = open and 1 or 0
  if ui.sent ~= want then
    pcall(function() VRUiPopup(want) end)
    ui.sent = want
  end
end

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
-- CACHED, because this now runs every frame: the controlled object rarely changes and walking its
-- components per frame is the kind of always-on cost this project has paid for before.
local devCache = { obj = nil, cam = nil, fov0 = nil, sent = nil }

-- THE HEADSET FOV, WRITTEN INTO THE TAKEN-OVER CAMERA'S OWN FIELD.
--
-- The plugin cannot do this one: for the mount's camera its patch callback returns before the fov block
-- ever runs -- measured, with the classifier returning kind 3 seven thousand times (DebugPatchCamDevice
-- = 7226) while g_devCamFovOrig stayed 0.0 and g_devCamFovSaved stayed 0. Writing the component field
-- from here does reach the picture: set live to 110 in the turret, the view widened at once.
--
-- The value is not a constant. The port forces its own vertical into the PLAYER's camera component, so
-- that component is read and mirrored -- if the headset or the resolution changes, this follows without
-- anyone editing a number. The authored value is kept and handed back when the takeover ends, so the
-- turret is left as it was found.
local function holdLensFov()
  if devCache.cam == nil then return end
  local want = 0.0
  pcall(function()
    -- The player's own camera component, resolved once per session rather than once per frame: this runs
    -- while a takeover is live and the value it reads is the port's own forced fov.
    if memo.fppCam == nil then memo.fppCam = Game.GetPlayer():GetFPPCameraComponent() end
    if memo.fppCam ~= nil then want = memo.fppCam:GetFOV() end
  end)
  if not (want > 1.0 and want < 179.0) then return end
  pcall(function()
    local cur = devCache.cam.fov
    if devCache.fov0 == nil and cur ~= nil and cur > 1.0 and cur < 179.0 and
       math.abs(cur - want) > 0.01 then
      devCache.fov0 = cur
    end
    if cur == nil or math.abs(cur - want) > 0.01 then devCache.cam.fov = want end
  end)
end

local function releaseLensFov()
  if devCache.cam ~= nil and devCache.fov0 ~= nil then
    local f = devCache.fov0
    pcall(function() devCache.cam.fov = f end)
  end
  devCache.fov0 = nil
end

local function publishRemoteCamera()
  if type(VRRemoteCamera) ~= "function" then return end
  local sys = memoSystem("takeover", "TakeOverControlSystem")
  local obj = nil
  if sys ~= nil then pcall(function() obj = sys:GetControlledObject() end) end

  if obj == nil then
    if devCtx ~= "default" then devcamApply("default") end
    releaseLensFov()
    devCache.obj, devCache.cam, devCache.sent = nil, nil, nil
    pcall(function() VRRemoteCamera(0, 0.0, 0.0, 0.0) end)
    if S.lastEntityId ~= nil and type(VRTakeoverEntity) == "function" then
      pcall(function() VRTakeoverEntity("0") end)
      S.lastEntityId = nil
    end
    S.remote = false
    return
  end
  -- THE CAMERA COMPONENT'S OWN POSITION, not the object's. The plugin believes a camera only while it
  -- sits within a metre and a half of what this publishes, and on the AV turret the mount puts its
  -- `cameraComponent` metres away from the entity origin -- so the claim never landed, the lens was never
  -- latched, and the picture stayed at the turret's own 60 degrees on first entry. Measured in that seat:
  -- the component's matrix column W read (-1440.87, 198.32, 623.96) while the entity sat elsewhere.
  --
  -- Falls back to the object's position when it has no camera component, which is the case the tolerance
  -- was written for in the first place.
  -- A new object may want a different framing; the context is decided once per takeover.
  local ctxNow = devcamContextFor(obj)
  if ctxNow ~= devCtx then devcamApply(ctxNow) end

  if devCache.obj ~= obj then
    releaseLensFov()
    devCache.obj, devCache.cam, devCache.sent = obj, nil, nil
    pcall(function()
      for _, c in ipairs(obj:GetComponents()) do
        if string.find(tostring(c:GetClassName().value), "CameraComponent") then
          devCache.cam = c
          break
        end
      end
    end)
  end
  local p = nil
  if devCache.cam ~= nil then
    pcall(function()
      local m = devCache.cam:GetLocalToWorld()
      if m ~= nil and m.W ~= nil then p = m.W end
    end)
  end
  -- THE IDENTITY, not the place. The plugin walks component+0x50 -> entity+0x48 (the offsets the RED4ext
  -- SDK documents for ent::IComponent::owner and ent::Entity::entityID) and compares this id, so a moving
  -- mount and a dozen cameras nearby cannot confuse it. Sent as a string because the hash is 64 bits and
  -- a Lua number would truncate it.
  if type(VRTakeoverEntity) == "function" then
    local id = nil
    pcall(function() id = tostring(obj:GetEntityID().hash) end)
    if id ~= nil then
      if id ~= S.lastEntityId then
        pcall(function() VRTakeoverEntity(id) end)
        S.lastEntityId = id
      end
    end
  end

  holdLensFov()

  if p == nil then pcall(function() p = obj:GetWorldPosition() end) end
  if p == nil then
    pcall(function() VRRemoteCamera(0, 0.0, 0.0, 0.0) end)
    S.remote = false
    return
  end
  pcall(function() VRRemoteCamera(1, p.x, p.y, p.z) end)
  S.remote = true
  S.remotePos = string.format("%.2f %.2f %.2f", p.x, p.y, p.z)
end

-- A BRAINDANCE IS RUNNING, and the FOV the game reports for its camera.
--
-- The plugin cannot be told WHICH object the braindance renders through -- that camera belongs to the
-- scene, and script has no handle on it -- but the active camera's FOV is a number the plugin can match
-- a component against, and inside a braindance it is nothing like the one the port forces for the
-- headset (measured: 55.879 against 103.982). That mismatch is the bug the player sees as one eye still
-- lying on Judy's bed while the other is in the recording.
local function publishBraindance()
  if type(VRBraindance) ~= "function" then return end
  local pl0 = Game.GetPlayer()
  local on = false
  pcall(function()
    local sys = memoSystem("braindance", "BraindanceSystem")
    if sys ~= nil then on = sys:GetIsInBraindance() end
  end)

  -- ORDINARY SCENES TOO, and this is the whole point of the change: a braindance is not the only place
  -- where the scene takes the camera. In a plain dialogue the game renders through a camera nothing of
  -- ours reaches -- measured in a Tier 4 scene: the ACTIVE camera was at fov 30.150 while the player's
  -- own component still read the 110 this port forces, so the headset got a 30 degree picture and the
  -- second eye kept following the player's camera instead of the one on screen.
  --
  -- THE STATE IS SceneTier, AND NOT GetSceneSystemCameraControlEnabled -- that was tried first and is
  -- not a signal at all: it reads TRUE in ordinary gameplay too (measured: owns=true, SceneTier=1, and
  -- the active camera's fov equal to the component's 110.000, i.e. nothing wrong to fix). Tier separates
  -- the two cleanly: 1 in gameplay, 4 in the dialogue above.
  --
  -- The fov MISMATCH would be the most direct test and is deliberately not the gate: the moment the fix
  -- works the mismatch disappears, the gate would close, the fov would fall back and the whole thing
  -- would oscillate. It is shown in the panel instead, where it is the thing to read.
  --
  -- Everything downstream -- the scene pose through VRSceneCamera, the scene fov through VRBdSceneFov,
  -- the headset fov forced into the view that matches it -- is machinery that already works; only the
  -- condition that opens it was too narrow.
  local sceneOwns = false
  pcall(function()
    -- The scene interface: resolved once and re-resolved only if it comes back nil. This gate runs
    -- every frame, and the handle does not change while a session lasts.
    if memo.sceneIface == nil then memo.sceneIface = Game.GetSceneSystem():GetScriptInterface() end
    local si = memo.sceneIface
    if si ~= nil then sceneOwns = si:GetSceneSystemCameraControlEnabled() end
  end)
  S.owns = sceneOwns
  local tier = -1
  pcall(function()
    local bb = pl0 and pl0:GetPlayerStateMachineBlackboard()
    if memo.bbDefs == nil then memo.bbDefs = Game.GetAllBlackboardDefs() end
    if bb ~= nil then tier = bb:GetInt(memo.bbDefs.PlayerStateMachine.SceneTier) end
  end)
  S.tier = tier

  -- ...AND ONLY A SCENE THAT ACTUALLY NEEDS IT. Tier alone was not enough and it showed: sitting in the
  -- helicopter is a Tier 2 scene whose camera already runs at the forced fov, and with the gate open the
  -- plugin's identity test -- "the camera whose own fov equals the one the script reports" -- matched
  -- EVERY camera in the frame. The log filled with
  --     PatchCamera: braindance camera <new addr> (was <addr>) fov=110.000 want=109.999
  -- a fresh address every frame, so the second eye's lens jumped every frame and BOTH views jerked. The
  -- plugin even warns about this shape: "a stream of them would mean the fov is not an identity either".
  --
  -- So the fov must DIFFER for the fix to have a target, and the decision is LATCHED for the scene: it is
  -- taken on the first frame, before our own write lands, and held until the scene ends. Testing the
  -- mismatch every frame would be self-cancelling -- the write makes the two fovs equal, the gate would
  -- close, the fov would fall back, and it would oscillate.
  local activeFov, compFov = 0.0, 0.0
  pcall(function() activeFov = Game.GetCameraSystem():GetActiveCameraFOV() or 0.0 end)
  pcall(function()
    local c = pl0 and pl0:GetFPPCameraComponent()
    if c ~= nil then compFov = c:GetFOV() end
  end)
  S.bdFov, S.fppFov = activeFov, compFov
  local mismatch = (activeFov > 1.0) and (compFov > 1.0) and (math.abs(activeFov - compFov) > 2.0)

  -- ...AND NEVER WHILE A DEVICE TAKEOVER OWNS THE SECOND EYE. The two mechanisms write the SAME latch
  -- (g_camObjDevice), so with both live they fight over it: measured at the AV turret, the log filled with
  --     PatchCamera: device camera component <A> (was <B>)      5225 re-claims
  -- as the device path re-took the lens the scene path had just grabbed by fov match, and the pair sat a
  -- steady 5.5 m apart (ipd 0.0640) because the second eye's base was coming from the scene pose instead
  -- of the turret. Reported as "vrcam отстает ну дерганье есть".
  --
  -- The takeover wins, and that is not a preference: the game itself renders MAIN through the taken-over
  -- camera -- "нет меня перекинуло на голову игрока а main сидит на турели" -- so the device path is the
  -- one that has the right anchor. The scene path exists for a camera nothing can name; a turret has a
  -- name and a position, and the port already follows it.
  local takeover = false
  pcall(function()
    local tos = memoSystem("takeover", "TakeOverControlSystem")
    if tos ~= nil and tos:GetControlledObject() ~= nil then takeover = true end
  end)
  S.takeover = takeover

  -- A TAKEOVER DOES NOT OPEN THE BRAINDANCE BRANCH. It was tried, in two shapes -- the scene system's own
  -- pose, and the lens pose published in its place -- and neither produced a usable picture; the branch
  -- carries braindance assumptions that do not hold on a mount. The takeover has its own path in the
  -- plugin instead (xr_dev_cam_in_locate).
  local blockedByTakeover = takeover and not S.sceneInTakeover
  if tier >= 2 and not blockedByTakeover then
    if mismatch then S.latched = true end
  else
    S.latched = false
  end
  if S.sceneGate and tier >= 2 and S.latched and not blockedByTakeover then on = true end

  -- THE ACTIVE CAMERA'S FOV, PUBLISHED EVEN WITH THE GATE CLOSED. During a takeover the plugin writes the
  -- headset fov into the view it identifies by the LENS pose, and this number is the seed that path needs.
  -- Without it the turret rendered at its own 60 -- read straight off the panel as "fov 60 active".
  if takeover and type(VRBdSceneFov) == "function" and activeFov > 1.0 then
    pcall(function() VRBdSceneFov(activeFov) end)
  end

  if not on then
    pcall(function() VRBraindance(0, 0.0) end)
    if type(VRSceneCamera) == "function" then
      pcall(function() VRSceneCamera(0, 0, 0, 0, 0, 0, 0, 1) end)
    end
    S.bd = false
    return
  end
  -- WHERE THE LIVE PLAYER'S CAMERA IS. The replacer carries a component named `camera` too, so the
  -- plugin cannot pick MAIN by that name while both entities are loaded -- it flaps between them, which
  -- reads as a jitter the size of the head offset. Published every tick, and the plugin only consults it
  -- inside a braindance.
  if type(VRPlayerCamera) == "function" then
    local pcam = nil
    pcall(function() pcam = pl0 and pl0:FindComponentByName(CName.new("camera")) end)
    local m = nil
    if pcam ~= nil then pcall(function() m = pcam:GetLocalToWorld() end) end
    if m ~= nil and m.W ~= nil then
      pcall(function() VRPlayerCamera(1, m.W.x, m.W.y, m.W.z) end)
      S.pcam = string.format("%.2f %.2f %.2f", m.W.x, m.W.y, m.W.z)
    else
      pcall(function() VRPlayerCamera(0, 0.0, 0.0, 0.0) end)
    end
  end

  local si0 = nil
  pcall(function() si0 = Game.GetSceneSystem():GetScriptInterface() end)
  -- THE HEAD GETS THE CAMERA BACK. In a braindance the SCENE takes camera control, and the camera it
  -- renders through is one neither the plugin nor script can name -- so nothing of ours reaches it: the
  -- right eye cannot be turned by the head and its FOV is the scene's. Handing control back puts the
  -- player's own camera in front again, which this port drives: both eyes then follow the head at the
  -- headset's FOV. Measured: with control off the FOV came right and the view followed the head at once.
  --
  -- Re-asserted every tick because the scene puts it back: the getter also lags a call behind, so it is
  -- never trusted -- the write is simply repeated while the braindance runs.
  -- AND IT IS LEFT WITH THE SCENE. Taking it away does give the head the view, but it switches the
  -- braindance into its EDITING mode -- a free camera -- and what is wanted is PLAYBACK: the replay
  -- drives where the camera goes, the head decides where it looks. That is done on the plugin side, by
  -- composing the head onto the orientation the scene has just written, exactly the way a surveillance
  -- camera's own aim becomes the base for the head. Off by default here; the knob stays for testing.
  if S.bdFreeLook == true and si0 ~= nil then
    pcall(function() si0:SetSceneSystemCameraControlEnabled(false, 0.0) end)
    S.bdFree = true
  else
    S.bdFree = false
  end

  -- ...AND THE SCENE'S OWN CAMERA POSE, the only thing about that camera either side can get hold of:
  -- the plugin's classifier never sees the object and script has no handle on it, but the scene system
  -- reports where it is and how it is turned. The plugin uses it as the second eye's lens -- the same
  -- lens the surveillance-camera fix moves that eye with.
  local si = nil
  pcall(function() si = Game.GetSceneSystem():GetScriptInterface() end)
  -- with the head driving, the scene's camera pose is not the lens any more: the player's own camera is
  if S.bdFreeLook == true then
    if type(VRSceneCamera) == "function" then
      pcall(function() VRSceneCamera(0, 0, 0, 0, 0, 0, 0, 1) end)
    end
  elseif si ~= nil and type(VRSceneCamera) == "function" then
    -- ONLY WHILE THE SCENE ACTUALLY OWNS THE CAMERA. In the braindance editor the player flies the
    -- camera and this getter keeps returning the last pose the replay left behind -- so the second eye,
    -- which is placed from it, stayed nailed to that spot while MAIN flew away. With control released
    -- the gate drops and both eyes go back to the player's own camera, which the editor is flying.
    local owns = false
    pcall(function() owns = si:GetSceneSystemCameraControlEnabled() end)
    S.bdOwns = owns
    if not owns then
      pcall(function() VRSceneCamera(0, 0, 0, 0, 0, 0, 0, 1) end)
      S.scam = "editor: the player flies the camera"
    else
      local sp, sq = nil, nil
      pcall(function() sp = si:GetSceneSystemCameraLastCameraPosition() end)
      pcall(function() sq = si:GetSceneSystemCameraLastCameraOrientation() end)
      local sv = nil
      if sp ~= nil then pcall(function() sv = WorldPosition.ToVector4(sp) end) end
      -- AND ONLY A REAL ONE. Outside a scene this getter returns the origin -- measured (0,0,0), which is
      -- 1785 m from the player -- and handing that over would place the second eye's lens in the void.
      local real = (sv ~= nil) and ((math.abs(sv.x) + math.abs(sv.y) + math.abs(sv.z)) > 1.0)
      if real and sq ~= nil then
        pcall(function() VRSceneCamera(1, sv.x, sv.y, sv.z, sq.i, sq.j, sq.k, sq.r) end)
        S.scam = string.format("%.1f %.1f %.1f", sv.x, sv.y, sv.z)
      else
        pcall(function() VRSceneCamera(0, 0, 0, 0, 0, 0, 0, 1) end)
        S.scam = "the scene reports no camera pose"
      end
    end
  end
  -- Read once, above, where the gate decision needs them. Their DIFFERENCE is the defect: the active
  -- camera is what the headset shows, the component is where this port's forced fov lands.
  local fov = activeFov
  -- AND HAND IT TO THE PLUGIN. The braindance fov write identifies its view by matching against a fov
  -- it had to LEARN from a frame where the pose test happened to match -- 54 times out of 1801 -- so
  -- until that landed the replay rendered at its own fov, and every new scene started over. This is
  -- the same number, a tick earlier, and it costs one call.
  if type(VRBdSceneFov) == "function" then
    pcall(function() VRBdSceneFov(fov) end)
  end
  -- The gate, not an identity: with it up the plugin takes MAIN's own position and aim as the lens for
  -- the second eye and forces the headset's FOV into MAIN. Measured in the process -- during a
  -- braindance the second view sits 2.8 km from the recording with the scene's 55.879 -- and nothing of
  -- ours reaches the camera the game renders, so MAIN is the only thing left to copy.
  pcall(function() VRBraindance(1, fov) end)
  S.bd = true
  S.bdFov = fov
end

registerForEvent("onInit", function()
  devcamLoad()
  devcamPush()
  print(string.format("[ForceFPP] ready; lens offset right=%.2f forward=%.2f up=%.2f",
                      dev.right, dev.forward, dev.up))
end)

registerForEvent("onUpdate", function(dt)
  -- Which overlay owns B right now; see publishUiPopup. Every frame, because the answer has to be right
  -- BEFORE the press, and the phone half of it is a single blackboard read.
  publishUiPopup(dt)

  -- BEFORE the mod's own switch: a braindance is not a first-person preference, and the second eye has
  -- to follow that camera whether or not the FPP hold is wanted.
  publishBraindance()

  -- THE TAKEOVER POSITION, EVERY FRAME. The plugin believes a camera only within a metre and a half of
  -- what this publishes, and at four times a second a flying AV moves far further than that between
  -- updates: measured in the turret, the published point was (-1421.0, 176.7, 623.4) while the lens was
  -- at (-1439.2, 198.1, 623.9) -- 28 m out, so the claim never happened at all (DebugPatchCamDevice = 0)
  -- and with it went the fov, the lens and the second eye's base. Standing still it worked, which is why
  -- this took so long to see.
  publishRemoteCamera()
  if not S.on then return end
  local pl = Game.GetPlayer()
  if pl == nil then
    S.applied = false            -- a load screen: the effect goes with the old player object
    memoDrop()                   -- ...and so do the cached systems, boards and widgets
    return
  end
  S.acc = S.acc + (dt or 0.016)
  if S.acc < 0.25 then return end
  S.acc = 0.0
  ensureRestriction(pl)
  forceFirstPerson(pl)
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
    ImGui.Text("UI overlay owning B: " .. tostring(S.popupWhat or "-"))
    local g, gch = ImGui.Checkbox("scene camera path in ordinary scenes", S.sceneGate)
    if gch then S.sceneGate = g end
    ImGui.Text(string.format("gate open: %s   latched: %s   takeover: %s   SceneTier: %d",
               tostring(S.bd == true), tostring(S.latched == true),
               tostring(S.takeover == true), S.tier or -1))
    ImGui.Text(string.format("fov  active camera %.2f   player component %.2f   diff %.2f",
               S.bdFov or 0.0, S.fppFov or 0.0, math.abs((S.bdFov or 0.0) - (S.fppFov or 0.0))))
    ImGui.Text("scene camera: " .. tostring(S.scam or "-"))
    ImGui.Text("pose gate (scene owns, plugin side): " .. tostring(S.bdOwns == true))
    ImGui.Text(string.format("camera put back %d time(s)", S.forced))
    ImGui.Text(S.note)
    ImGui.Text("remote camera: " .. (S.remote and ("yes, at " .. tostring(S.remotePos)) or "no"))
    ImGui.Separator()
    if S.takeover then
      local tc, vc = nil, nil
      pcall(function()
        local tos = Game.GetScriptableSystemsContainer():Get(CName.new("TakeOverControlSystem"))
        local o = tos and tos:GetControlledObject()
        if o ~= nil then
          for _, c in ipairs(o:GetComponents()) do
            local cl = tostring(c:GetClassName().value)
            if cl:find("CameraComponent") then tc = c break end
          end
        end
      end)
      pcall(function()
        for _, c in ipairs(Game.GetPlayer():GetComponents()) do
          if tostring(c:GetClassName().value) == "entRenderToTextureCameraComponent" and c:IsEnabled() then
            vc = c break
          end
        end
      end)
      local function w(c) local m = nil; pcall(function() m = c:GetLocalToWorld() end); return m and m.W or nil end
      local a, b = tc and w(tc), vc and w(vc)
      if a and b then
        local dx, dy, dz = b.x - a.x, b.y - a.y, b.z - a.z
        ImGui.Text(string.format("lens -> vrcam: %.3f m  (%.3f %.3f %.3f)",
                   math.sqrt(dx * dx + dy * dy + dz * dz), dx, dy, dz))
      else
        ImGui.Text("lens -> vrcam: the turret's camera component was not found")
      end
    end
    local sb, sbch = ImGui.Checkbox("scene path during a takeover (A/B)", S.sceneInTakeover)
    if sbch then S.sceneInTakeover = sb end
    ImGui.Text("off = the device path owns the second eye (no jerk, no head steering?)")
    ImGui.Text("on  = the scene path writes MAIN through the located buffer as well")
    ImGui.Separator()
    ImGui.Text("Viewpoint of a taken-over camera (turret, surveillance), metres:")
    local changed = false
    ImGui.Text("offsets for: " .. tostring(devCtx) ..
               (devUser[devCtx] ~= nil and "  (saved override)" or "  (baked default)"))
    local v, ch = ImGui.SliderFloat("right / left", dev.right, -3.0, 3.0)
    if ch then dev.right = v; changed = true end
    v, ch = ImGui.SliderFloat("forward / back", dev.forward, -3.0, 3.0)
    if ch then dev.forward = v; changed = true end
    v, ch = ImGui.SliderFloat("up / down", dev.up, -3.0, 3.0)
    if ch then dev.up = v; changed = true end
    if ImGui.Button("back to the shipped default") then
      devUser[devCtx] = nil
      devcamApply(devCtx)
      devcamSave(true)
    end
    if ImGui.Button("reset to the lens itself") then
      dev.right, dev.forward, dev.up = 0.0, 0.0, 0.0
      changed = true
    end
    if changed then
      devcamPush()
      devcamSave(false)
    end
    ImGui.Text("Applies only while a camera is taken over; forward is the lens's own")
    ImGui.Text("horizontal aim, up is world up, and both eyes move together.")
    ImGui.Separator()
    ImGui.Text("Blocking is the game's own: GameplayRestriction.VehicleFPP makes")
    ImGui.Text("IsVehicleCameraChangeBlocked true, so ToggleVehCamera does nothing.")
    ImGui.End()
  end)
end)
