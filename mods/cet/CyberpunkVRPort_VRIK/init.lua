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

local function getCameraWorldPose(player)
    local camera = player:GetFPPCameraComponent()
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
local observersRegistered = false
local recenterPending = -1.0        -- seconds until a queued recenter fires; <0 = idle

-- dxgi writes vrik_settings.ini (disable_mouse_y); follow the overlay checkbox.
local function readVrikSettings()
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

registerForEvent('onUpdate', function(dt)
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

    if mouseDisableEnabled then
        local cam = player:GetFPPCameraComponent()
        if cam then
            -- Force pitch to 0
            cam.pitchMax = 0.0
            cam.pitchMin = 0.0
        end
    else
        local cam = player:GetFPPCameraComponent()
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
        pcall(function() UpdateVRIKAnimInputs() end)
    end

    local player = Game.GetPlayer()
    if not player then return end
    
    -- VR Transforms Update for Model-Space IK
    local camPos, camQuat = getCameraWorldPose(player)
    if not camPos or not camQuat then return end

    -- Remember the camera pose for the diagnostic logger hotkey, and keep the
    -- pre-write bone snapshot fresh while tracking is active.
    lastCamPos = camPos
    lastCamQuat = camQuat
    if vrTrackingEnabled and type(SetVRDiagCapture) == 'function' then
        pcall(function() SetVRDiagCapture(1) end)
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
        local oko, ori = pcall(function() return player:GetWorldOrientation() end)
        if oko and ori and type(ori.i) == 'number' then
            eqi, eqj, eqk, eqr = ori.i, ori.j, ori.k, ori.r
            haveQuat = true
        end
        local yaw = 0.0
        local oky, wy = pcall(function() return player:GetWorldYaw() end)
        if oky and type(wy) == 'number' then yaw = wy end
        if not haveQuat then
            local h = math.rad(yaw) * 0.5
            eqk, eqr = math.sin(h), math.cos(h)   -- Rz(yaw)
        end
        -- camPos is the FPP camera (= HMD) world position; entityPos is the player world origin.
        local ex, ey, ez = 0.0, 0.0, 0.0
        local okp, ppos = pcall(function() return player:GetWorldPosition() end)
        if okp and ppos then ex, ey, ez = ppos.x, ppos.y, ppos.z end
        pcall(function()
            SetVRPlayerYaw(yaw, camQuat.i, camQuat.j, camQuat.k, camQuat.r,
                           camPos.x, camPos.y, camPos.z, ex, ey, ez,
                           eqi, eqj, eqk, eqr)
        end)
        status.debugYaw = string.format("Yaw: %.2f", yaw)
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
