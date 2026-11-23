-- Read the player's gamepad range and remove model input slew only during a VR
-- wheel grab. The model's steering limits, speed assists and tire physics stay
-- native. Shared drive-model flats are restored when the grip/driver changes.
local patch = nil
local profileTime = 0.5
local reportedError = false
local reportedJolt = false
local rates = {
    turnUpdateInputSlowChangeSpeed = 100.0,
    turnUpdateInputFastChangeSpeed = 100.0,
    wheelTurnMaxAddPerSecond = 10000.0,
    wheelTurnMaxSubPerSecond = 10000.0,
}

local function flat(id, name) return TweakDBID.new(id, "." .. name) end
local function key(id) return tostring(id.hash) .. ":" .. tostring(id.length) end
local function same(a, b) return type(a) == "number" and math.abs(a - b) < 0.00001 end

local function restore()
    if not patch then return end
    for name, entry in pairs(patch.values) do
        local id = flat(patch.id, name)
        -- An external edit made while held belongs to its author, not this mod.
        if same(TweakDB:GetFlat(id), entry.applied) then
            assert(TweakDB:SetFlat(id, entry.before) ~= false, "cannot restore steering rate")
        end
        patch.values[name] = nil
    end
    patch = nil
end

local function apply(id)
    local wanted = key(id)
    if patch and patch.key == wanted then return end
    restore()
    local values = {}
    for name, rate in pairs(rates) do
        local before = TweakDB:GetFlat(flat(id, name))
        -- Not all mounted vehicle types use the road-vehicle drive model.
        if type(before) ~= "number" then return end
        values[name] = { before = before, applied = math.max(before, rate) }
    end
    patch = { id = id, key = wanted, values = values }
    for name, entry in pairs(values) do
        local field = flat(id, name)
        assert(TweakDB:SetFlat(field, entry.applied) ~= false, "cannot apply steering rate")
        assert(same(TweakDB:GetFlat(field), entry.applied), "steering rate not accepted")
    end
end

-- THE CABIN WHEEL FOR THE HANDS ON IT. While a hand holds the rim of a car's wheel the plugin keeps it on the wheel the
-- car draws: its steering_wheel slot, here in the player's model space (world = player position + player turn * model).
-- Read every frame of a hold only; a vehicle without the slot (a bike) leaves the plugin on its own estimate.
local function qmul(ai, aj, ak, ar, bi, bj, bk, br)
    return ar * bi + ai * br + aj * bk - ak * bj, ar * bj - ai * bk + aj * br + ak * bi,
           ar * bk + ai * bj - aj * bi + ak * br, ar * br - ai * bi - aj * bj - ak * bk
end
local function qrot(qi, qj, qk, qr, x, y, z)
    local tx, ty, tz = 2.0 * (qj * z - qk * y), 2.0 * (qk * x - qi * z), 2.0 * (qi * y - qj * x)
    return x + qr * tx + (qj * tz - qk * ty), y + qr * ty + (qk * tx - qi * tz), z + qr * tz + (qi * ty - qj * tx)
end
local function sendCabin(player, vehicle)
    if type(SetVRWheelCabin) ~= "function" or not vehicle:IsA(CName.new("vehicleCarBaseObject")) then return end
    local slots = vehicle:FindComponentByName(CName.new("vehicle_slots"))
    if not slots then return end
    local found, wt = slots:GetSlotTransform(CName.new("steering_wheel"))
    if not found or not wt then return end
    local wp = WorldPosition.ToVector4(WorldTransform.GetWorldPosition(wt))
    local wo = WorldTransform.GetOrientation(wt)
    local base, pq = player:GetWorldPosition(), player:GetWorldOrientation()
    local ii, ij, ik, ir = -pq.i, -pq.j, -pq.k, pq.r
    local x, y, z = qrot(ii, ij, ik, ir, wp.x - base.x, wp.y - base.y, wp.z - base.z)
    local mi, mj, mk, mr = qmul(ii, ij, ik, ir, wo.i, wo.j, wo.k, wo.r)
    SetVRWheelCabin(Vector4.new(x, y, z, 1.0), Quaternion.new(mi, mj, mk, mr))
end

-- THE CAR'S JOLTS, FOR THE DRIVING VIBRATION. The game's own rumble rarely fires and the audio cannot tell a road bump
-- from the engine or the radio, so the car's body is measured instead: a bump or a landing is a spike in its vertical
-- acceleration, a crash a sudden loss of speed. Hard braking stays under both. The level decays over ~90 ms, so a
-- one-frame spike is a felt kick, and goes to the plugin every frame (SetVRVehicleJolt; it is added to the game's rumble).
local BUMP_FROM, BUMP_SPAN = 3.0, 20.0      -- m/s^2 of vertical acceleration: felt from 3, full at 23
local CRASH_FROM, CRASH_SPAN = 4.0, 10.0    -- m/s of speed lost within CRASH_WINDOW: felt from 4, full at 14
local CRASH_WINDOW, DECAY = 0.2, 0.09
local jolt = nil
local function updateJolt(dt)
    if type(SetVRVehicleJolt) ~= "function" then return end
    local player = Game.GetPlayer()
    local vehicle = player and player:GetMountedVehicle()
    if not vehicle or not dt or dt <= 0 or dt > 0.1 then jolt = nil; return end
    local id = tostring(vehicle:GetEntityID().hash)
    local z = vehicle:GetWorldPosition().z
    local okSpeed, current = pcall(function() return vehicle:GetCurrentSpeed() end)
    local speed = okSpeed and type(current) == "number" and math.abs(current) or 0
    if not jolt or jolt.id ~= id then
        jolt = { id = id, z = z, vz = nil, level = 0, t = 0, speeds = {} }
        return
    end
    jolt.t = jolt.t + dt
    local vz = (z - jolt.z) / dt
    jolt.z = z
    if math.abs(vz) > 40 then jolt.vz = nil; jolt.speeds = {}; return end   -- a teleport, not a bump
    local bump = 0
    if jolt.vz then
        local az = (vz - jolt.vz) / dt
        bump = math.min(1, math.max(0, (math.abs(az) - BUMP_FROM) / BUMP_SPAN))
    end
    jolt.vz = vz
    local speeds = jolt.speeds
    speeds[#speeds + 1] = { t = jolt.t, v = speed }
    while #speeds > 1 and jolt.t - speeds[1].t > CRASH_WINDOW do table.remove(speeds, 1) end
    local fastest = speed
    for _, s in ipairs(speeds) do fastest = math.max(fastest, s.v) end
    local crash = math.min(1, math.max(0, (fastest - speed - CRASH_FROM) / CRASH_SPAN))
    jolt.level = math.max(bump, crash, jolt.level * math.exp(-dt / DECAY))
    SetVRVehicleJolt(jolt.level)
end

registerForEvent("onUpdate", function(dt)
    local joltOk, joltErr = pcall(updateJolt, dt)
    if not joltOk and not reportedJolt then print("[driving] vehicle jolt unavailable: " .. tostring(joltErr)); reportedJolt = true end
    local ok, err = pcall(function()
        local state = Game.GetVRWheelControlState()
        if state % 4 ~= 0 then
            local p = Game.GetPlayer()
            local v = p and p:GetMountedVehicle()
            if v then sendCabin(p, v) end
        end
        if state == 0 then restore(); profileTime = 0.5; return end
        profileTime = profileTime + math.max(0, dt or 0)
        if profileTime >= 0.5 then
            local settings = Game.GetSettingsSystem()
            local inner = settings:GetVar("/controls", "Axis_DeadzoneInnerFix"):GetValue()
            local outer = settings:GetVar("/controls", "Axis_DeadzoneOuter"):GetValue()
            assert(Game.SetVRWheelGamepadProfile(inner, outer), "invalid gamepad range")
            profileTime = 0
        end
        if state % 4 == 0 then restore(); return end
        local player = Game.GetPlayer()
        local vehicle = player and player:GetMountedVehicle()
        if not vehicle then restore(); return end
        local model = TweakDB:GetFlat(flat(vehicle:GetRecordID(), "vehDriveModelData"))
        if not model then restore(); return end
        apply(model)
    end)
    if not ok then
        pcall(restore)
        if not reportedError then print("[driving] wheel profile unavailable: " .. tostring(err)) end
        reportedError = true
    else
        reportedError = false
    end
end)

registerForEvent("onShutdown", function() pcall(restore) end)
return { GetState = function() return { active = patch ~= nil, model = patch and patch.key or nil } end }
