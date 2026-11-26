-- The car seat's script side: the player's gamepad range and the cabin wheel's slot for the hands on the wheel, the
-- radio list's close and the autopilot chord. The road wheels' response to the wheel is the plugin's
-- (Anim/VehicleSteerCurve.cpp): it writes the driven car's live physics while a hand holds the wheel and puts it back
-- after, so no drive model in TweakDB is changed here -- an edited flat reaches every car spawned meanwhile,
-- the traffic's included, and never the car already driven.
local profileTime = 0.5
local reportedError = false
local vehicleHands = require('vehicle_hands')

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

-- B CLOSES THE RADIO LIST IN THE CAR. The list (D-pad left held) closes on `cancel`, the pad's B, which in a car is
-- held out of the game as the exit button, and Start closes it only to open the pause menu on the release (measured).
-- The plugin counts each B press made with a popup up (shared slot 175); on a change the open list is closed the way
-- its own cancel does. Only one modal list is ever up; it is forgotten when it uninitializes.
local POPUP_BACK_SLOT = 175
local modalList, popupBacks = nil, nil
local function popupBack()
    local n = GetVRSharedSlot(POPUP_BACK_SLOT)
    if popupBacks == nil or n == popupBacks then popupBacks = n; return end
    popupBacks = n
    local list = modalList
    modalList = nil
    if list then list:Close() end
end

registerForEvent("onInit", function()
    ObserveAfter("BaseModalListPopupGameController", "OnInitialize", function(self) modalList = self end)
    ObserveAfter("BaseModalListPopupGameController", "OnUninitialize", function() modalList = nil end)
end)

-- THE AUTOPILOT ON THE CYBERWARE CHORD. In the driver seat the plugin hands the chord (left grip + left stick click,
-- held) here instead of pressing the cyberware key. The game's own autodrive is L3 held for half a second, which the
-- port never produces; this queues what the autodrive widget queues, so the system's own checks -- the player's own
-- car, a valid lane, no race -- still decide, and it says so on the HUD when it refuses.
local function autopilotChord()
    local ok, taken = pcall(function() return Game.VRAutopilotChordTake() end)
    if not ok or not taken then return end
    local ads = Game.GetScriptableSystemsContainer():Get(CName.new("AutoDriveSystem"))
    if not ads then return end
    if ads:GetAutodriveEnabled() then
        ads:QueueRequest(DisableAutoDriveRequest.new())
    else
        ads:QueueRequest(EnableAutoDriveRequest.new())
    end
end

registerForEvent("onUpdate", function(dt)
    local chordOK, chordError = pcall(autopilotChord)
    if not chordOK and not reportedError then print('[driving] autopilot chord: ' .. tostring(chordError)) end
    local backOK, backError = pcall(popupBack)
    if not backOK then modalList = nil; print('[driving] popup close: ' .. tostring(backError)) end
    local handsOK, handsError = pcall(vehicleHands.update, Game.GetPlayer(), os.clock())
    if not handsOK then
        vehicleHands.clear()
        if vehicleHands.state.error ~= tostring(handsError) then print('[driving] panel interaction: ' .. tostring(handsError)) end
        vehicleHands.state.error = tostring(handsError)
    end
    local ok, err = pcall(function()
        local state = Game.GetVRWheelControlState()
        if state % 4 ~= 0 then
            local p = Game.GetPlayer()
            local v = p and p:GetMountedVehicle()
            if v then sendCabin(p, v) end
        end
        if state == 0 then profileTime = 0.5; return end
        profileTime = profileTime + math.max(0, dt or 0)
        if profileTime >= 0.5 then
            local settings = Game.GetSettingsSystem()
            local inner = settings:GetVar("/controls", "Axis_DeadzoneInnerFix"):GetValue()
            local outer = settings:GetVar("/controls", "Axis_DeadzoneOuter"):GetValue()
            assert(Game.SetVRWheelGamepadProfile(inner, outer), "invalid gamepad range")
            profileTime = 0
        end
    end)
    if not ok then
        if not reportedError then print("[driving] wheel profile unavailable: " .. tostring(err)) end
        reportedError = true
    else
        reportedError = false
    end
end)

registerForEvent("onShutdown", function() vehicleHands.clear() end)
return { GetState = function() return { hands = vehicleHands.state } end }
