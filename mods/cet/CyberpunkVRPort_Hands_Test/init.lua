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

registerForEvent('onUpdate', function(dt)
    if not isReady then return end
    if type(IsVRHandLinked) ~= 'function' then return end

    -- VRFPP arm hiding/chunk debug is fully disabled.
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

registerForEvent('onDraw', function()
    if not isReady then return end

    ImGui.SetNextWindowPos(100, 100, ImGuiCond.FirstUseEver)
    ImGui.SetNextWindowSize(500, 250, ImGuiCond.FirstUseEver)
    ImGui.Begin('VR Controller Gizmos')

    ImGui.Text('DebugVisualizer: ' .. tostring(status.debugVisualizer))
    ImGui.Text('Debug source: ' .. status.debugSource)
    ImGui.Text('DebugHistory: ' .. tostring(status.debugHistory))
    ImGui.Text('Last draw err: ' .. status.lastDrawErr)
    ImGui.Text('Draw gizmos: ' .. tostring(drawEnabled))
    ImGui.SameLine()
    if ImGui.Button(drawEnabled and 'Disable##gizmos' or 'Enable##gizmos') then
        drawEnabled = not drawEnabled
    end

    ImGui.Text('Hide native arms: ' .. tostring(hideNativeArms))
    ImGui.SameLine()
    if ImGui.Button(hideNativeArms and 'Show arms##toggle' or 'Hide arms##toggle') then
        hideNativeArms = not hideNativeArms
        -- Feature disabled; keep UI state but never call hide path.
        if not hideNativeArms then needRestoreArms = true end
    end

    if ImGui.Button('Dump FPP components') then
        if type(DumpVRFppComponents) == 'function' then
            local ok, result = pcall(function() return DumpVRFppComponents() end)
            if ok then
                dumpStatus = 'dumped ' .. tostring(result) .. ' components'
            else
                dumpStatus = 'dump failed: ' .. tostring(result)
            end
        else
            dumpStatus = 'native function missing'
        end
    end
    ImGui.SameLine()
    ImGui.Text(dumpStatus)

    ImGui.Separator()
    ImGui.Text('Character hand chunk debug:')
    ImGui.Text('FPP chunk debug disabled in this build')
    if ImGui.Button('Disable chunk debug') then
        chunkDebugEnabled = false
        needRestoreArms = true
    end

    if ImGui.Button('Bigger gizmos') then
        gizmoScale = gizmoScale + 0.15
    end
    ImGui.SameLine()
    if ImGui.Button('Smaller gizmos') then
        gizmoScale = math.max(0.35, gizmoScale - 0.15)
    end
    ImGui.SameLine()
    ImGui.Text(string.format('Scale %.2f', gizmoScale))

    ImGui.Separator()
    ImGui.Text('Left valid: ' .. tostring(status.leftValid))
    ImGui.Text('Left raw:   ' .. status.leftRaw)
    ImGui.Text('Left world: ' .. status.leftWorld)

    ImGui.Separator()
    ImGui.Text('Right valid: ' .. tostring(status.rightValid))
    ImGui.Text('Right raw:   ' .. status.rightRaw)
    ImGui.Text('Right world: ' .. status.rightWorld)

    ImGui.End()
end)
