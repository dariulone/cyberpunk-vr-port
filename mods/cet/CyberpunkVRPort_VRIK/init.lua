-- VR controller raw tracking visualizer.
-- Draws world-space hand gizmos with DebugVisualizer so we can validate OpenXR
-- controller pose math without spawning props or touching weapons.

local isReady = false
local drawEnabled = true
local hideNativeArms = false -- kept for UI only; VRFPP hide path is disabled
local gizmoScale = 1.0
local dumpStatus = "idle"
local chunkDebugEnabled = false
local chunkDebugComponentIndex = 51
local chunkDebugHand = 1
local chunkDebugUseFullMask = true
local chunkDebugBit0 = 0
local chunkDebugBit1 = -1
local chunkDebugBit2 = -1
local chunkDebugBit3 = -1
local needRestoreArms = true
-- SPRINT-ANIM KILL (VR). Zero the anim-graph 'sprint' input every frame: the sprint
-- animation layer carries the arm pump AND the ~20cm forward camera lead (rig Up-GRPs
-- keep Z constant, hence the flat 1.6) -- measured source of the sprint start/stop
-- body/hands dive and the sprint snap-turn "double". The game itself uses this exact
-- input to run sprint WITHOUT the animation (shoot-while-sprinting perk path in
-- SprintEvents.OnUpdate), so speed, stamina and state logic stay fully intact.
local killSprintAnim = true

-- [CAMWRITE] engine-native camera orientation (dxgi xr_cam_write_mode=1):
-- dxgi publishes the desired WORLD camera quat in shared [100..103] with a
-- publish seq [151] (mode flag [84]). Each sim frame we convert it to camera-
-- LOCAL against the live parent orientation and write it through
-- FPPCameraComponent:SetLocalOrientation -- the exact path mouse input takes,
-- so every camera builder consumes ONE quat in-phase instead of racing the
-- locate-stomp (the intermittent head-turn jitter). SetVRCamAck(seq) tells
-- dxgi the path is alive; if we die, dxgi falls back to the legacy stomp.
local camWriteLastSeq = -1.0

-- VR hand tracking activation + IK calibration now live in the F10 in-headset
-- overlay (VRIK tab), published to the plugin via shared memory. This CET mod
-- keeps the per-frame plugin pump (UpdateVRIKAnimInputs), gizmos and diagnostics.

local status = {
    debugVisualizer = false,
    debugSource = "none",
    debugHistory = false,
    lastDrawErr = "none",
    leftValid = false,
    rightValid = false,
    leftRaw = "n/a",
    rightRaw = "n/a",
    leftWorld = "n/a",
    rightWorld = "n/a",
}

local function v4(x, y, z, w)
    return Vector4.new(x, y, z, w or 0.0)
end

local function add(a, b)
    return v4(a.x + b.x, a.y + b.y, a.z + b.z, 1.0)
end

local function sub(a, b)
    return v4(a.x - b.x, a.y - b.y, a.z - b.z, 1.0)
end

local function mul(v, s)
    return v4(v.x * s, v.y * s, v.z * s, 0.0)
end

local function vecStr(v)
    if not v then return "nil" end
    return string.format("%.2f %.2f %.2f", v.x, v.y, v.z)
end

local function makeColor(r, g, b, a)
    local ok, c = pcall(function()
        return Color.new(r, g, b, a or 255)
    end)
    if ok and c then return c end
    ok, c = pcall(function()
        return Color.new({ Red = r, Green = g, Blue = b, Alpha = a or 255 })
    end)
    if ok and c then return c end
    return nil
end

local BODY_LEFT = makeColor(0, 220, 255, 255)
local BODY_RIGHT = makeColor(255, 180, 0, 255)
local AXIS_RIGHT = makeColor(255, 64, 64, 255)
local AXIS_UP = makeColor(64, 255, 64, 255)
local AXIS_FWD = makeColor(64, 128, 255, 255)
local AXIS_TEXT = makeColor(255, 255, 255, 255)

local function setChunkPreset(index, hand, fullMask)
    chunkDebugEnabled = true
    chunkDebugUseFullMask = fullMask and true or false
    chunkDebugComponentIndex = index
    chunkDebugHand = hand
end

local function getMatrixMath()
    return GetSingleton('Matrix')
end

local function getQuatMath()
    return GetSingleton('Quaternion')
end

local function getDebugVisualizer(player)
    local ok, dvs = pcall(function()
        return GameInstance.GetDebugVisualizerSystem(player:GetGame())
    end)
    if ok and dvs then
        status.debugSource = 'GameInstance.GetDebugVisualizerSystem'
        return dvs
    end
    ok, dvs = pcall(function() return GetSingleton('gameDebugVisualizerSystem') end)
    if ok and dvs then
        status.debugSource = "GetSingleton('gameDebugVisualizerSystem')"
        return dvs
    end
    ok, dvs = pcall(function() return GetSingleton('DebugVisualizerSystem') end)
    if ok and dvs then
        status.debugSource = "GetSingleton('DebugVisualizerSystem')"
        return dvs
    end
    status.debugSource = 'none'
    return nil
end

local function getDebugHistory(player)
    local ok, ddh = pcall(function()
        return GameInstance.GetDebugDrawHistorySystem(player:GetGame())
    end)
    if ok and ddh then
        status.debugHistory = true
        return ddh
    end
    status.debugHistory = false
    return nil
end

local function getCameraWorldPose(player, camera)
    camera = camera or player:GetFPPCameraComponent()
    if not camera then return nil, nil end
    local l2w = camera:GetLocalToWorld()
    if not l2w then return nil, nil end
    local mat = getMatrixMath()
    if not mat then return nil, nil end
    local camPos = mat:GetTranslation(l2w)
    local camQuat = mat:ToQuat(l2w)
    return camPos, camQuat
end

local function mapLocalPos(rawPos)
    return v4(rawPos.x, -rawPos.z, rawPos.y, 0.0)
end

local function mapLocalQuat(rawQuat)
    return Quaternion.new(rawQuat.i, -rawQuat.k, rawQuat.j, rawQuat.r)
end

local function getHandWorldPose(isLeft, camPos, camQuat)
    local validFn = isLeft and GetLeftVRHandValid or GetRightVRHandValid
    local posFn = isLeft and GetLeftVRHandPos or GetRightVRHandPos
    local rotFn = isLeft and GetLeftVRHandRot or GetRightVRHandRot
    if type(validFn) ~= 'function' or type(posFn) ~= 'function' or type(rotFn) ~= 'function' then
        return nil
    end

    local valid = validFn()
    if isLeft then
        status.leftValid = valid
    else
        status.rightValid = valid
    end
    if not valid then return nil end

    local rawPos = posFn()
    local rawQuat = rotFn()
    if isLeft then
        status.leftRaw = vecStr(rawPos)
    else
        status.rightRaw = vecStr(rawPos)
    end

    local quatMath = getQuatMath()
    if not quatMath then return nil end

    local localPos = mapLocalPos(rawPos)
    local localQuat = mapLocalQuat(rawQuat)

    local worldOffset = quatMath:Transform(camQuat, localPos)
    local worldPos = add(camPos, worldOffset)

    local localForward = quatMath:GetForward(localQuat)
    local localRight = quatMath:GetRight(localQuat)
    local localUp = quatMath:GetUp(localQuat)

    local worldForward = quatMath:Transform(camQuat, localForward)
    local worldRight = quatMath:Transform(camQuat, localRight)
    local worldUp = quatMath:Transform(camQuat, localUp)

    if isLeft then
        status.leftWorld = vecStr(worldPos)
    else
        status.rightWorld = vecStr(worldPos)
    end

    return {
        pos = worldPos,
        forward = worldForward,
        right = worldRight,
        up = worldUp,
    }
end

local function drawLine(dvs, a, b, color, life)
    local ok, err = pcall(function() dvs:DrawLine3D(a, b, color, life) end)
    if not ok then
        ok = pcall(function() dvs:DrawLine3D(a, b, color) end)
    end
    if not ok then
        ok = pcall(function() dvs:DrawLine3D(a, b) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawArrow(dvs, a, b, color, life)
    local ok, err = pcall(function() dvs:DrawArrow(a, b, color, life) end)
    if not ok then
        ok = pcall(function() dvs:DrawArrow(a, b, color) end)
    end
    if not ok then
        ok = pcall(function() dvs:DrawArrow(a, b) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawSphere(dvs, pos, radius, color, life)
    local ok, err = pcall(function() dvs:DrawWireSphere(pos, radius, color, life) end)
    if not ok then
        ok = pcall(function() dvs:DrawWireSphere(pos, radius, color) end)
    end
    if not ok then
        ok = pcall(function() dvs:DrawWireSphere(pos, radius) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawText3D(dvs, pos, text, color, life)
    local ok, err = pcall(function() dvs:DrawText3D(pos, text, color, life) end)
    if not ok then
        ok = pcall(function() dvs:DrawText3D(pos, text, color) end)
    end
    if not ok then
        ok = pcall(function() dvs:DrawText3D(pos, text) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawHistSphere(ddh, pos, radius, color, tag)
    local ok, err = pcall(function() ddh:DrawWireSphere(pos, radius, color, tag) end)
    if not ok then
        ok = pcall(function() ddh:DrawWireSphere(pos, radius, color, tostring(tag)) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawHistArrow(ddh, pos, direction, color, tag)
    local ok, err = pcall(function() ddh:DrawArrow(pos, direction, color, tag) end)
    if not ok then
        ok = pcall(function() ddh:DrawArrow(pos, direction, color, tostring(tag)) end)
    end
    if not ok then
        status.lastDrawErr = tostring(err)
    end
end

local function drawHandGizmo(dvs, name, hand, bodyColor)
    local life = 0.06
    local core = 0.025 * gizmoScale
    local side = 0.055 * gizmoScale
    local lift = 0.060 * gizmoScale
    local reach = 0.220 * gizmoScale

    local pos = hand.pos
    local rightPos = add(pos, mul(hand.right, side))
    local leftPos = sub(pos, mul(hand.right, side))
    local upPos = add(pos, mul(hand.up, lift))
    local downPos = sub(pos, mul(hand.up, lift))
    local fwdStart = sub(pos, mul(hand.forward, 0.020 * gizmoScale))
    local fwdEnd = add(pos, mul(hand.forward, reach))
    local palmTop = add(upPos, mul(hand.right, side * 0.45))
    local palmBottom = add(downPos, mul(hand.right, side * 0.45))
    local palmTopL = sub(upPos, mul(hand.right, side * 0.45))
    local palmBottomL = sub(downPos, mul(hand.right, side * 0.45))

    drawSphere(dvs, pos, core, bodyColor, life)
    drawLine(dvs, leftPos, rightPos, AXIS_RIGHT, life)
    drawLine(dvs, downPos, upPos, AXIS_UP, life)
    drawLine(dvs, palmBottomL, palmTopL, bodyColor, life)
    drawLine(dvs, palmBottom, palmTop, bodyColor, life)
    drawLine(dvs, palmTopL, palmTop, bodyColor, life)
    drawLine(dvs, palmBottomL, palmBottom, bodyColor, life)
    drawArrow(dvs, fwdStart, fwdEnd, AXIS_FWD, life)
    drawSphere(dvs, fwdEnd, core * 0.45, AXIS_FWD, life)

    drawText3D(dvs, add(upPos, mul(hand.up, core * 1.6)), name, bodyColor, life)
    drawText3D(dvs, add(rightPos, mul(hand.right, core * 0.9)), "R", AXIS_TEXT, life)
    drawText3D(dvs, sub(leftPos, mul(hand.right, core * 0.9)), "L", AXIS_TEXT, life)
    drawText3D(dvs, add(upPos, mul(hand.up, core * 0.9)), "U", AXIS_TEXT, life)
    drawText3D(dvs, sub(downPos, mul(hand.up, core * 0.9)), "D", AXIS_TEXT, life)
    drawText3D(dvs, add(fwdEnd, mul(hand.forward, core * 0.8)), "F", AXIS_TEXT, life)
end

registerForEvent('onInit', function()
    isReady = true
end)

local vrTrackingEnabled = false
local mouseDisableEnabled = true  -- Mouse/look pitch (Y) disabled by default for VR

-- Bridge with the dxgi F10 overlay. CET sandboxes these relative paths to this mod's
-- own folder, which is exactly where dxgi reads/writes them (absolute path).
local recenterCounter = os.time()   -- seed unique per session so dxgi sees a change
local settingsPollTimer = 0.0
local neckCapTimer = 0.0
local observersRegistered = false
local recenterPending = -1.0        -- seconds until a queued recenter fires; <0 = idle

-- Read the live native setting without sandboxed file IO on the game thread.
-- The bridge file remains a compatibility fallback for an older plugin.
local function readVrikSettings()
    if type(GetVRMouseYDisabled)=='function' then
        mouseDisableEnabled=GetVRMouseYDisabled()
        return
    end
    local f = io.open('vrik_settings.ini', 'r')
    if not f then return end
    local t = f:read('*a') or ''
    f:close()
    local v = t:match('disable_mouse_y%s*=%s*(%d+)')
    if v then mouseDisableEnabled = (tonumber(v) ~= 0) end
end

-- Ask dxgi to recenter the HMD base by bumping a counter it polls.
local function requestRecenter()
    recenterCounter = recenterCounter + 1
    local f = io.open('vrik_recenter.ini', 'w')
    if f then f:write('recenter=' .. tostring(recenterCounter) .. '\n'); f:close() end
end

-- Recenter when the player attaches to the world (save load / spawn). OnGameAttached
-- fires during the load/fade, before the HMD pose settles, so queue the recenter a
-- short delay later instead of firing immediately. 3.5s = enough for the loading fade
-- and the first gameplay frames to settle so the HMD pose used as the base is stable.
local function ensureObservers()
    if observersRegistered then return end
    observersRegistered = true
    pcall(function()
        Observe('PlayerPuppet', 'OnGameAttached', function()
            recenterPending = 3.5
        end)
    end)
end

registerHotkey('ToggleVRHands', 'Toggle VR Hands', function()
    vrTrackingEnabled = not vrTrackingEnabled
    if vrTrackingEnabled then
        pcall(function() Game.InstallVRAnimPoseHook() end)
        pcall(function() Game.ArmVRAnimPosePlayer() end)
        pcall(function() Game.SetVRBindMode(4) end)  -- 4 = full-arm model-space IK
    else
        pcall(function() Game.SetVRBindMode(0) end)
    end
end)

-- Mouse-Y disable is now owned by the F10 overlay (Tracking/Camera) via vrik_settings.ini;
-- the old CET hotkey was removed so the file poll doesn't fight a manual toggle.

-- Last FPP camera world pose, captured each frame for the diagnostic logger.
local lastCamPos = nil
local lastCamQuat = nil



registerHotkey('LogVRDiag', 'Log VR Hand Diagnostic', function()
    if type(SetVRDiagCapture) == 'function' then pcall(function() SetVRDiagCapture(1) end) end
    if type(LogVRDiag) == 'function' and lastCamPos and lastCamQuat then
        pcall(function()
            LogVRDiag(lastCamPos.x, lastCamPos.y, lastCamPos.z,
                      lastCamQuat.i, lastCamQuat.j, lastCamQuat.k, lastCamQuat.r)
        end)
    end
end)

local locoWarned = false

-- Public console helpers live in the mod environment, not a transient console
-- chunk. GetMod("CyberpunkVRPort_VRIK").EquipCyberware("gorilla") works after
-- every launch without first pasting a global Lua function.
local cyberwareRecords = {
    gorilla = "Items.AdvancedStrongArmsLegendary",
    mantis = "Items.AdvancedMantisBladesLegendary",
    monowire = "Items.AdvancedNanoWiresLegendary",
    launcher = "Items.AdvancedProjectileLauncherLegendary",
}
local pendingCyberware
local cyberwareStatus = "idle"
local api = {}
local function finishCyberware(status)
    pendingCyberware = nil
    cyberwareStatus = status
end
function api.GetCyberwareStatus() return cyberwareStatus end
function api.EquipCyberware(kind)
    if pendingCyberware then return false, "A cyberware change is already in progress" end
    local record = cyberwareRecords[kind]
    local player = Game.GetPlayer()
    if not record or not player then return false, "Use gorilla, mantis, monowire or launcher after loading a save" end
    local ts = Game.GetTransactionSystem()
    local function findItem()
        local ok, items = ts:GetItemList(player)
        if not ok then return nil end
        for _, item in ipairs(items or {}) do
            if TDBID.ToStringDEBUG(ItemID.GetTDBID(item:GetID())) == record then return item:GetID() end
        end
    end
    local id = findItem()
    if not id then Game.AddToInventory(record, 1); id = findItem() end
    if not id then return false, "The implant could not be found in the inventory" end
    local equipment = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
    local holster = NewObject("EquipmentSystemWeaponManipulationRequest")
    local uninstall = NewObject("UnequipRequest")
    -- NewObject needs the native RTTI name, not the REDscript EquipRequest alias.
    local install = NewObject("gameEquipRequest")
    if not holster or not uninstall or not install then return false, "Native equipment requests unavailable" end
    holster.owner = player
    holster.requestType = EquipmentManipulationAction.UnequipAll
    uninstall.owner = player
    uninstall.areaType = gamedataEquipmentArea.ArmsCW
    uninstall.slotIndex = 0
    install.owner = player
    install.itemID = id
    install.slotIndex = 0
    install.addToInventory = false
    pendingCyberware = { owner=tostring(player:GetEntityID().hash), id=id, kind=kind,
        elapsed=0, stable=0, phase="holstering", uninstall=uninstall, install=install }
    cyberwareStatus = "holstering"
    equipment:QueueRequest(holster)
    return true, record
end
local function updateCyberware(dt)
    local pending = pendingCyberware
    if not pending then return end
    dt = dt or 0
    pending.elapsed = pending.elapsed + dt
    local player = Game.GetPlayer()
    if not player or tostring(player:GetEntityID().hash) ~= pending.owner then
        finishCyberware("cancelled: player changed"); return
    end
    if pending.elapsed > 8 then finishCyberware("error: equipment transition timed out"); return end
    local equipment = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
    local data = equipment:GetPlayerData(player)
    local ts = Game.GetTransactionSystem()
    local right = ts:GetItemInSlot(player, TweakDBID.new("AttachmentSlots.WeaponRight"))
    local left = ts:GetItemInSlot(player, TweakDBID.new("AttachmentSlots.WeaponLeft"))
    if pending.phase == "holstering" then
        -- A queued holster is not a completed holster. Installing while the old
        -- melee cyberware is still drawn leaves its additional appearances on
        -- the player, even through EquipRequest. Wait for native detach and the
        -- holstered visual before touching the implant slot.
        local arms = ts:GetItemInSlot(player, TweakDBID.new("AttachmentSlots.RightArm"))
        if right or left or data:IsEquipPending() or not arms or not arms:IsVisualSpawnAttached() then
            pending.stable = 0; return
        end
        pending.stable = pending.stable + dt
        if pending.stable < 0.25 then return end
        if tostring(data:GetActiveItem(gamedataEquipmentArea.ArmsCW)) ~= tostring(pending.id) then
            equipment:QueueRequest(pending.uninstall)
            equipment:QueueRequest(pending.install)
        end
        pending.phase = "equipping"; pending.stable = 0; cyberwareStatus = pending.phase
        return
    end
    if pending.phase == "equipping" then
        if data:IsEquipPending() or tostring(data:GetActiveItem(gamedataEquipmentArea.ArmsCW)) ~= tostring(pending.id) then
            pending.stable = 0; return
        end
        pending.stable = pending.stable + dt
        if pending.stable < 0.25 then return end
        if pending.kind == "launcher" then finishCyberware("ready"); return end
        local draw = NewObject("EquipmentSystemWeaponManipulationRequest")
        draw.owner = player
        draw.requestType = EquipmentManipulationAction.RequestActiveMeleeware
        equipment:QueueRequest(draw)
        pending.phase = "drawing"; cyberwareStatus = pending.phase
        return
    end
    if right and tostring(right:GetItemID()) == tostring(pending.id) and right:IsVisualSpawnAttached() then
        finishCyberware("ready")
    end
end
registerForEvent("onShutdown", function() finishCyberware("idle") end)

-- PLAIN FUNCTIONS FOR THE PER-FRAME pcalls below: `pcall(function() ... end)` allocates a closure on
-- every call, and the mods' per-frame garbage is what CET's collector pays for (the GC step in
-- CyberpunkVRPort_Stereo).
-- The Locomotion field's id, read once: blackboard definitions are global and live as long as the game.
local locomotionId = nil
-- The Takedown, BodyCarrying and BodyDisposalDetailed fields ride the same read: a grapple, and lifting,
-- dropping, throwing or putting away a body, hand the body to their paired animation in the plugin;
-- carrying hands it the left arm
-- (AnimPose.cpp). Checked once, on the first push (natives are registered by then), so an older plugin
-- without the native costs nothing per frame.
local takedownId, bodyCarryingId, bodyDisposalId = nil, nil, nil
local hasPairedAnimNative = nil
local scannerVisionId, hasScannerStateNative = nil, nil
local locomotionDetailedId, hasLocomotionDetailedNative = nil, nil
local function pushLocomotion(player)
    locomotionId = locomotionId or Game.GetAllBlackboardDefs().PlayerStateMachine.Locomotion
    local bb = player:GetPlayerStateMachineBlackboard()
    if bb then
        SetVRLocomotionState(bb:GetInt(locomotionId))
        -- the detailed value: a fall, a jump, a landing (the plain one stays Default through them)
        if hasLocomotionDetailedNative == nil then hasLocomotionDetailedNative = type(SetVRLocomotionDetailed) == 'function' end
        if hasLocomotionDetailedNative then
            locomotionDetailedId = locomotionDetailedId or Game.GetAllBlackboardDefs().PlayerStateMachine.LocomotionDetailed
            SetVRLocomotionDetailed(bb:GetInt(locomotionDetailedId))
        end
        if hasScannerStateNative == nil then hasScannerStateNative = type(SetVRScannerState) == 'function' end
        if hasScannerStateNative then
            scannerVisionId = scannerVisionId or Game.GetAllBlackboardDefs().PlayerStateMachine.Vision
            SetVRScannerState(bb:GetInt(scannerVisionId))
        end
        if hasPairedAnimNative == nil then hasPairedAnimNative = type(SetVRPairedAnimationState) == 'function' end
        if hasPairedAnimNative then
            takedownId = takedownId or Game.GetAllBlackboardDefs().PlayerStateMachine.Takedown
            bodyCarryingId = bodyCarryingId or Game.GetAllBlackboardDefs().PlayerStateMachine.BodyCarrying
            bodyDisposalId = bodyDisposalId or Game.GetAllBlackboardDefs().PlayerStateMachine.BodyDisposalDetailed
            SetVRPairedAnimationState(bb:GetInt(takedownId), bb:GetInt(bodyCarryingId), bb:GetInt(bodyDisposalId))
        end
    end
end

local function applyCamWrite(player)
    if GetVRSharedSlot(84) ~= 1.0 then return end
    local s1 = GetVRSharedSlot(151)
    if s1 == 0.0 or s1 == camWriteLastSeq then return end
    local qx = GetVRSharedSlot(100); local qy = GetVRSharedSlot(101)
    local qz = GetVRSharedSlot(102); local qw = GetVRSharedSlot(103)
    -- Torn-publish guard: seq is written LAST on the dxgi side, so a
    -- changed re-read means the quat spans two publishes -> skip frame.
    if GetVRSharedSlot(151) ~= s1 then return end
    local cam = player:GetFPPCameraComponent()
    if not cam then return end
    local pw = player:GetWorldOrientation()
    if not pw or type(pw.i) ~= 'number' then return end
    -- local = conj(parent_world) * desired_world (Hamilton, game axes).
    local ci, cj, ck, cr = -pw.i, -pw.j, -pw.k, pw.r
    local li = cr*qx + ci*qw + cj*qz - ck*qy
    local lj = cr*qy - ci*qz + cj*qw + ck*qx
    local lk = cr*qz + ci*qy - cj*qx + ck*qw
    local lr = cr*qw - ci*qx - cj*qy - ck*qz
    local n = math.sqrt(li*li + lj*lj + lk*lk + lr*lr)
    if n < 1e-6 then return end
    cam:SetLocalOrientation(Quaternion.new(li/n, lj/n, lk/n, lr/n))
    camWriteLastSeq = s1
    SetVRCamAck(s1)
end

local function killSprint(player) AnimationControllerComponent.SetInputFloat(player, 'sprint', 0.0) end
local function worldOrientation(player) return player:GetWorldOrientation() end
local function worldYaw(player) return player:GetWorldYaw() end
local function worldPosition(player) return player:GetWorldPosition() end

-- THE MIRROR DOUBLE (src/Natives/VrikArming.cpp). Looking into a mirror is a scene that spawns an NPC copy of V at
-- the mirror (Character.q110_tpp_male_v in V's bathroom); the reflection shows that copy, not the player. A puppet
-- whose record is a TPP V of the player's gender, within MIRROR_FIND_M while the mirror's own hub (title "Mirror",
-- with a leave choice) is up, is bound, and it takes the tracked pose -- arms, and the head the first-person body does
-- not draw. It stays bound while it lives.
-- ONLY A MIRROR THE PLAYER TURNS ON. A story scene that stands V at a mirror (the prologue starts: the nomad garage,
-- the corpo restroom) spawns the same q110 double but shows no "Mirror" hub: nothing is bound there, VRIK stays
-- suspended for the scene as configured, and the scene's own animations play. In the port's copies the
-- double is not walked to the sink by the scene any more: it is where the scene spawned it, a few metres off, until
-- the first teleport puts it on the player. Johnny in the mirror (fact johnny_mirror) is the game's and not bound.
--
-- WALKING WITH THE MIRROR ON. The port's copies of the interactive mirror scenes (every "Mirror" hub in the game:
-- V's apartment and base\open_world\scenes\mirrors, tools/mirror/walkable_mirror.py) leave the
-- player out of them: no slide to the sink, no lean, no workspot, no way back -- the player walks on and the mirror
-- (its device, the mirrors area, the double, the hub: leave / hairdresser / facial) is the game's. A mirror scene the
-- port has no copy of still puts the player in its workspot: once its hub is up the player is let out of it. The
-- double goes where the player goes: an AI teleport command (the scene owns the double, a plain teleport does not
-- move it), out of any workspot the scene puts it in. X on the same prompt turns it off again (the hub's highlight
-- is put on its leave choice). Further than MIRROR_OFF_M from where the mirror started, the mirror turns
-- itself off by its own "leave" choice (the hub's choice marked quest-important): the scene's leave branch is the
-- only one that switches the reflection off (device ForceOFF, the mirrors area, the double despawned, the mirror
-- prefab variant). VRInteractionAction (src/Natives/InteractionAction.cpp) hands the hub its scroll and apply
-- actions directly, the way the game's own input does, and only while the mirror's hub is the active one.
-- VRIKMirrorDoubleFlags (global, for the harness): bit 0 = identify its passes by buffer A as well.
VRIKMirrorDoubleFlags = VRIKMirrorDoubleFlags or 0
local MIRROR_HUB_TITLE = 'LocKey#46420'
local MIRROR_FREE_AFTER = 0.2   -- seconds of the mirror's hub on screen before the player is let out (when the
                                -- "look in the mirror" prompt was not seen being taken: then at once)
local MIRROR_OFF_M = 4.0        -- metres from where the mirror started: it turns itself off
local MIRROR_STEP = 0.3         -- seconds between the hub actions of a leave
local MIRROR_FIND_M = 15.0      -- metres around the player the double is looked for once the mirror's hub is up
local mirrorTimer, mirrorBound = 0.0, false
local mirror = nil              -- { id, anchor, hubT, freed, stepT, yaw }
local function findMirrorDouble(player, within)
    local q = TargetSearchQuery.new()
    q.maxDistance = within
    q.testedSet = TargetingSet.Complete
    q.searchFilter = TSF_All(TSFMV.Obj_Puppet)
    local _, parts = Game.GetTargetingSystem():GetTargetParts(player, q)
    local playerHash = player:GetEntityID().hash
    local female = Game.NameToString(player:GetResolvedGenderName()) == 'Female'
    for _, part in ipairs(parts or {}) do
        local e = part:GetComponent():GetEntity()
        if e and e:GetEntityID().hash ~= playerHash and e:IsA('NPCPuppet') then
            local record = string.lower(TDBID.ToStringDEBUG(e:GetRecordID()) or '')
            if string.find(record, 'tpp', 1, true) and string.find(record, '_v$') and
               (string.find(record, 'female', 1, true) ~= nil) == female then
                return e
            end
        end
    end
    return nil
end
-- The mirror's hub on screen: its id, the index of its leave choice (nil when it has none), the active hub's id and
-- the highlighted index. nil when no mirror hub is up.
local function mirrorHub()
    local defs = GetAllBlackboardDefs().UIInteractions
    local bb = Game.GetBlackboardSystem():Get(defs)
    local hubs = FromVariant(bb:GetVariant(defs.DialogChoiceHubs))
    for _, hub in ipairs((hubs and hubs.choiceHubs) or {}) do
        if hub.title == MIRROR_HUB_TITLE then
            local leave = nil
            for i, c in ipairs(hub.choices or {}) do
                if leave == nil and c.type and (c.type.properties % 2) == 1 then leave = i - 1 end
            end
            return hub.id, leave, bb:GetInt(defs.ActiveChoiceHubID), bb:GetInt(defs.SelectedIndex)
        end
    end
    return nil
end
-- One step of leaving the mirror by its leave choice: the highlight moved onto it, then applied.
local function stepLeave()
    if type(VRInteractionAction) ~= 'function' then return end
    local id, leave, active, selected = mirrorHub()
    if id == nil or leave == nil or active ~= id then return end
    local action = 'ChoiceApply'
    if selected > leave then action = 'ChoiceScrollUp' elseif selected < leave then action = 'ChoiceScrollDown' end
    VRInteractionAction(CName.new(action))
end
local mirrorSeenT = 99.0   -- seconds since a mirror hub (the "look in the mirror" prompt or the mirror's own) was up
local function updateMirrorDouble(player, dt)
    local ws = Game.GetWorkspotSystem()
    if mirror == nil then
        mirrorTimer = mirrorTimer + dt
        mirrorSeenT = mirrorSeenT + dt
        if mirrorTimer < 0.1 then return end
        mirrorTimer = 0.0
        local hubId, leave = mirrorHub()
        if hubId ~= nil then mirrorSeenT = 0.0 end
        local inWorkspot = ws:IsActorInWorkspot(player)
        local double = nil
        if leave ~= nil and Game.GetQuestsSystem():GetFactStr('johnny_mirror') <= 0 then
            double = findMirrorDouble(player, MIRROR_FIND_M)
        end
        if double and VRMirrorDouble(double, VRIKMirrorDoubleFlags) == true then
            mirrorBound = true
            -- a mirror's scene (its prompt was just taken, or its hub is up): no workspot from the first frame
            mirror = { id = double:GetEntityID(), anchor = player:GetWorldPosition(), stepT = 0.0, hubT = 0.0,
                       freed = not inWorkspot, taken = mirrorSeenT < 3.0 }
        end
        return
    end
    local double = Game.FindEntityByID(mirror.id)
    if double == nil then
        VRMirrorDoubleClear()
        mirrorBound, mirror = false, nil
        return
    end
    mirrorTimer = mirrorTimer + dt
    if mirrorTimer >= 0.25 then
        mirrorTimer = 0.0
        VRMirrorDouble(double, VRIKMirrorDoubleFlags)   -- cheap for the same entity; rebinds a rebuilt rig
    end
    -- in the scene's workspot (at the start, or put back by a hub branch): let out once the mirror's hub is up
    if ws:IsActorInWorkspot(player) then
        mirror.freed = false
        if mirrorHub() ~= nil then mirror.hubT = mirror.hubT + dt else mirror.hubT = 0.0 end
        if mirror.taken or mirror.hubT >= MIRROR_FREE_AFTER then
            mirror.taken = false
            ws:SendFastExitSignal(player, Vector3.new(0, 0, 0), false, false, true, false)
            mirror.freed, mirror.hubT = true, 0.0
        end
        return
    end
    if not mirror.freed then return end
    -- X ON THE SAME PROMPT TURNS IT OFF: the highlight starts on the leave choice (first in V's bathroom, not in
    -- every mirror), so the confirm that turned the mirror on turns it off again. Once, for up to 3 s: a hub the
    -- player scrolls afterwards is theirs.
    if not mirror.homed then
        mirror.homeT = (mirror.homeT or 0.0) + dt
        mirror.stepT = mirror.stepT - dt
        if mirror.homeT > 3.0 then
            mirror.homed = true
        elseif mirror.stepT <= 0.0 then
            mirror.stepT = MIRROR_STEP
            local id, leave, active, selected = mirrorHub()
            if id ~= nil and leave ~= nil and active == id then
                if selected == leave then
                    mirror.homed = true
                elseif type(VRInteractionAction) == 'function' then
                    VRInteractionAction(CName.new(selected > leave and 'ChoiceScrollUp' or 'ChoiceScrollDown'))
                end
            end
        end
    end
    -- the double where the player is
    if ws:IsActorInWorkspot(double) then ws:SendFastExitSignal(double, Vector3.new(0, 0, 0), false, false, true, false) end
    local p = player:GetWorldPosition()
    local yaw = player:GetWorldYaw()
    local dp = double:GetWorldPosition()
    local dx, dy, dz = p.x - dp.x, p.y - dp.y, p.z - dp.z
    if dx * dx + dy * dy + dz * dz > 1e-4 or mirror.yaw == nil or math.abs(yaw - mirror.yaw) > 0.5 then
        local cmd = AITeleportCommand.new()
        cmd.position = p
        cmd.rotation = yaw
        cmd.doNavTest = false
        double:GetAIControllerComponent():SendCommand(cmd)
        mirror.yaw = yaw
    end
    -- far from the mirror: its own leave choice
    if Vector4.Distance(p, mirror.anchor) > MIRROR_OFF_M then
        mirror.stepT = mirror.stepT - dt
        if mirror.stepT <= 0.0 then
            mirror.stepT = MIRROR_STEP
            stepLeave()
        end
    end
end

registerForEvent('onUpdate', function(dt)
    local okCyberware, cyberwareError = pcall(updateCyberware, dt)
    if not okCyberware then
        finishCyberware("error: " .. tostring(cyberwareError))
        spdlog.error("[VRIK] " .. cyberwareStatus)
    end
    -- The first-person neck cap follows the body's skin tone (src/Natives/NeckCap.cpp) -- in the main menu's
    -- lobby too, where the rest of this update waits for the rig.
    neckCapTimer = neckCapTimer + (dt or 0.0)
    if neckCapTimer >= 0.25 then
        neckCapTimer = 0.0
        if type(VRNeckCapSync) == 'function' then pcall(VRNeckCapSync) end
    end
    if not isReady then return end

    -- Register the load observer once, and poll the overlay's mouse-Y setting.
    ensureObservers()
    settingsPollTimer = settingsPollTimer + (dt or 0.0)
    if settingsPollTimer >= 0.25 then
        settingsPollTimer = 0.0
        pcall(readVrikSettings)
    end

    -- Fire a queued (delayed) recenter once gameplay has settled and the player exists.
    if recenterPending >= 0.0 then
        recenterPending = recenterPending - (dt or 0.0)
        if recenterPending < 0.0 and Game.GetPlayer() then
            requestRecenter()
        end
    end

    local player = Game.GetPlayer()
    if not player then return end

    if type(VRMirrorDouble) == 'function' and not pcall(updateMirrorDouble, player, dt or 0.0) then
        if mirrorBound then pcall(VRMirrorDoubleClear) end
        mirrorBound, mirror = false, nil
    end

    -- LOCOMOTION STATE -> the plugin, for one decision it cannot make on its own: the sprint detent
    -- (left stick to the stop) must not stand a CROUCHING player up. The plugin sees buttons and poses,
    -- not the player state machine, and our own crouch gesture cannot stand in for the state -- the
    -- game's crouch is a toggle and the game clears it by itself on jumps and mantles.
    --
    -- Values are gamePSMLocomotionStates (1 = Crouch, 12 = CrouchSprint, 13 = CrouchDodge). This is the
    -- same blackboard the weapon-state redscript reads, one field over.
    if type(SetVRLocomotionState) == 'function' then
        local okLoco = pcall(pushLocomotion, player)
        -- LOUD ONCE, never per frame: a silently missing state looks exactly like "the crouch gate
        -- does not work", and that costs a debugging round to tell apart from a logic bug.
        if not okLoco and not locoWarned then
            locoWarned = true
            pcall(function() spdlog.info('[VRIK] locomotion state unavailable -- sprint crouch gate off') end)
        end
    end

    -- [CAMWRITE] apply the dxgi-published desired camera quat via the engine's
    -- own component path (see the comment at camWriteLastSeq).
    if type(GetVRSharedSlot) == 'function' and type(SetVRCamAck) == 'function' then
        pcall(applyCamWrite, player)
    end

    -- Per-frame: the PSM re-asserts anim inputs on sprint entry, so keep forcing it.
    if killSprintAnim then
        pcall(killSprint, player)
    end

    -- The FPP camera component once a frame: the pitch limits below and the camera pose further down.
    local cam = player:GetFPPCameraComponent()
    if mouseDisableEnabled then
        if cam then
            -- Force pitch to 0
            cam.pitchMax = 0.0
            cam.pitchMin = 0.0
        end
    else
        if cam then
            -- Restore defaults roughly
            cam.pitchMax = 80.0
            cam.pitchMin = -80.0
        end
    end
    -- We only do a one-shot restore in case a previous session hid the arms.
    if needRestoreArms and type(RestoreVRFppArms) == 'function' then
        pcall(function() RestoreVRFppArms() end)
        needRestoreArms = false
    end

    if type(UpdateVRIKAnimInputs) == 'function' then
        pcall(UpdateVRIKAnimInputs)
    end

    -- VR Transforms Update for Model-Space IK
    if not cam then return end
    local camPos, camQuat = getCameraWorldPose(player, cam)
    if not camPos or not camQuat then return end

    -- Remember the camera pose for the diagnostic logger hotkey, and keep the
    -- pre-write bone snapshot fresh while tracking is active.
    lastCamPos = camPos
    lastCamQuat = camQuat
    if vrTrackingEnabled and type(SetVRDiagCapture) == 'function' then
        pcall(SetVRDiagCapture, 1)
    end

    if type(SetVRPlayerYaw) == 'function' then
        -- Entity world ORIENTATION as a quaternion. The plugin needs the EXACT world->model
        -- rotation (the conjugate of this) to convert the gizmo's WORLD-space hand target into
        -- the bone buffer's MODEL space, so the hand lands exactly on the gizmo. NOTE:
        -- GetWorldOrientation() returns a Quaternion (fields i,j,k,r) -- its .yaw is nil, so the
        -- old code silently used yaw=0, which left the hand offset in world orientation (hands
        -- pointed the wrong way). Build from GetWorldYaw() only as a fallback.
        local eqi, eqj, eqk, eqr = 0.0, 0.0, 0.0, 1.0
        local haveQuat = false
        local oko, ori = pcall(worldOrientation, player)
        if oko and ori and type(ori.i) == 'number' then
            eqi, eqj, eqk, eqr = ori.i, ori.j, ori.k, ori.r
            haveQuat = true
        end
        local yaw = 0.0
        local oky, wy = pcall(worldYaw, player)
        if oky and type(wy) == 'number' then yaw = wy end
        if not haveQuat then
            local h = math.rad(yaw) * 0.5
            eqk, eqr = math.sin(h), math.cos(h)   -- Rz(yaw)
        end
        -- camPos is the FPP camera (= HMD) world position; entityPos is the player world origin.
        local ex, ey, ez = 0.0, 0.0, 0.0
        local okp, ppos = pcall(worldPosition, player)
        if okp and ppos then ex, ey, ez = ppos.x, ppos.y, ppos.z end
        pcall(SetVRPlayerYaw, yaw, camQuat.i, camQuat.j, camQuat.k, camQuat.r,
              camPos.x, camPos.y, camPos.z, ex, ey, ez,
              eqi, eqj, eqk, eqr)
        status.debugYaw = yaw   -- a number: a formatted string every frame was garbage nobody read
    end

    if not dvs and not ddh then return end

    local leftHand = getHandWorldPose(true, camPos, camQuat)
    local rightHand = getHandWorldPose(false, camPos, camQuat)

    if leftHand then
        drawHandGizmo(dvs, "LEFT", leftHand, BODY_LEFT)
    else
        status.leftRaw = "n/a"
        status.leftWorld = "n/a"
    end

    if rightHand then
        drawHandGizmo(dvs, "RIGHT", rightHand, BODY_RIGHT)
    else
        status.rightRaw = "n/a"
        status.rightWorld = "n/a"
    end
end)

-- The CET 'VR Hands Control' window has been removed; all controls live in the
-- F10 in-headset overlay (VRIK + HUD tabs). This mod now only runs the per-frame
-- onUpdate pump (UpdateVRIKAnimInputs) and draws the world-space hand gizmos.

return api
