-- CyberpunkVRPort — THE GRENADE IN THE HAND.
--
-- Taking one off the belt, holding it, and putting it back. Its own file because it is not the belt:
-- the belt is components on a garment, and this is a grip button, an item in a hand slot and a pose
-- rewritten every frame. The reload module made the same cut for its revolver, for the same reason.
--
-- WHAT CROSSES THE BOUNDARY, and nothing else:
--   E.CFG, E.S          the belt's config and state -- the grab keeps its state in E.S.grab because
--                       two places outside this file read it: applyElements hides the element that is
--                       in the hand, and playerPlane must not count our own prop as a weapon
--   E.log               the module's log
--   E.propItem          which prop entity the equipped grenade wants, and the record pointed at it
--   E.propAllowSlot     the slot whitelist on that record -- a hand slot is not on it by default
--   E.playerPlane       scene or weapon, which decides how the item is attached
--   E.eulerQuat         the panel's degrees to the engine's quaternion
--   E.transactions      the transaction system, cached by the belt
--
-- Driven from init.lua:  GRENADE.tick(now, dt)  every frame,  GRENADE.release(pl)  to force it back.

local M = {}
local E = {}

-- EVERY TOP-LEVEL NAME IN THIS FILE, DECLARED ONCE, HERE.
--
-- The bodies below are in whatever order the work happened to leave them, and that is fine:
-- with the names already local, a function written above the state it reads still sees it.
-- Without this the file needed its declarations sorted by hand, and an hour of moving blocks
-- to satisfy that produced thirty names that resolved as nil globals -- `pinS`, `holdNow`,
-- `playSnd`, `gripDown` and the rest -- each of which killed the grab tick on the frame it
-- was reached. Nothing here changes what the code does; it changes when its names exist.
local holdNow, FLIGHT, GRIP_SLOT, HAND_SLOT, trace, grenadeAttach, grenadeRelease, traceAt
local pinTrace, updatePin, throwS, HIST, THROW_SLOTS, throwSample, releaseVelocity, modelToWorld
local handForward, consume, throwArm, throwDisarm, grenadeThrow, throwTick, grenadeParts, WRIST_BONE
local ANCHOR_BONE, qmul4, qrot4, grenadeHoldPose, loadPose, TYPES, customFor, typeInfo, POSE_CACHE
local PRESS_CACHE, pressPose, poseFor, GRENADE_POSE, PINCH_POSE, PIN_POSE, PIN_P, PULL_DIR, pinAxis
local THUMB_TIP, INDEX_TIP, pinchPoint, HOLD1_P, HOLD1_Q, HOLD_P, HOLD_Q, pinS, TRIGGER_SLOT, triggerAmount
local trgS, setTriggerBlock, leverS, holdPBuf, holdQBuf, leverPose, ARM_MODE, armMode, SND, muteStow, sndAt
local playSnd, leverTick, handBone, poseS, poseApply, poseClear, poseTick, pinPoseTick, claimHand
local magnetSet, magnetClear, rawHand, gripDown, updateGrenadeGrab, unequipWeapon

-- FORWARD-DECLARED, AT THE TOP, and it has to be at the top rather than merely "before holdNow".
--
-- `poseApply` and `grenadeHoldPose` both ask which hold is in force, and `holdNow` is defined far
-- below because it needs the per-type table. A name that is not yet a local resolves as a GLOBAL and
-- is nil, so every frame with a grenade in hand logged
--
--     grenade.lua:218: attempt to call global 'holdNow' (a nil value)
--
-- and the grab tick died there: no transform was ever written and the grenade sat wherever the slot
-- put it. The first attempt at this fix put the declaration on line 286 -- above `holdNow` but BELOW
-- `grenadeHoldPose` on line 200 -- and changed nothing, which is why it goes here.


-- THE FLIGHT is its own file: it is a ballistic integrator with a swept ray, and it has nothing to do
-- with hands, pins or levers.
FLIGHT = (function()
    local ok, m = pcall(function() return require("grenade/flight") end)
    return (ok and m) or nil
end)()


function M.setup(env)
    E = env
    -- The flight gets the trace as well: a spawn that fails inside a pcall says nothing otherwise, and
    -- that silence cost two rounds.
    if FLIGHT ~= nil then
        env.trace = function(fmt, ...) M.trace(fmt, ...) end
        FLIGHT.setup(env)
    end
end

-- THE GRIP BUTTON, per hand, off the plugin's shared block. The same two slots the smoking module
-- reads, and the same convention the rest of the port uses for a hand: 0 is left, 1 is right.
GRIP_SLOT = { [0] = 155, [1] = 49 }
HAND_SLOT = { [0] = "AttachmentSlots.WeaponLeft", [1] = "AttachmentSlots.WeaponRight" }

-- THE HAND SHAPE THE GAME ITSELF HOLDS A GRENADE IN, lifted out of its own throw rather than posed by
-- hand: reload_record_01, right hand, the four and a half seconds between the pin coming out and the
-- wind-up, where both wrists read zero. tools/pose_from_take.py made the file, tools/read_throw_take.py
-- found the window.
--
-- Poses are stored LEFT-named because the plugin's finger natives take a bone by name; `handBone` below
-- is the same rewrite the reload module does, so one file serves either hand.
-- A TRACE THAT SURVIVES THE CRASH.
--
-- spdlog buffers. Two dumps in a row were read as "the grab never ran" because its lines were not in
-- the log -- and the giveaway that this was wrong is that the belt's own re-equip line, which lands
-- every two seconds, is also missing for the whole minute before the crash. The tail was simply never
-- flushed, and absence proved nothing either time.
--
-- Open/write/close survives a crash, but pinTrace also calls this during idle.
-- Keep all file work behind the explicit diagnostic option; no forced override.

trace = function(fmt, ...)
    if not E.CFG.grab_trace then return end
    local ok, line = pcall(string.format, fmt, ...)
    if not ok then return end
    pcall(function()
        local f = io.open("grab_trace.log", "a")
        if f == nil then return end
        f:write(os.date("%H:%M:%S ") .. line .. string.char(10))
        f:close()
    end)
end

grenadeAttach = function(pl, side, host, now)
    -- ONE LINE PER GRAB, and it says how far the attach got. Two dumps in a row ended inside this
    -- function with nothing in the log, which narrowed it to "somewhere in here" and no further; the
    -- next one will name the step. A grab is a rare event, so this costs nothing to leave in.
    trace("attach start side=%d host=%s", side, tostring(host))
    local item, entity = E.propItem({ follow = "grenade" })
    if item == nil then trace("attach: no grenade item") return false end
    local slot = HAND_SLOT[side]
    trace("attach: item=%s entity=%s slot=%s", tostring(item), tostring(entity), slot)
    E.propAllowSlot(item, slot)
    trace("attach: slot allowed")
    local plane = E.playerPlane(pl, now) or 0
    local forced = E.CFG.grenade_grab_plane or 0
    if forced == 1 then plane = 0 elseif forced == 2 then plane = 2 end
    local ok = false
    pcall(function()
        if plane == 2 then ok = pl:VRPortHoldItem(item, slot)
        else ok = pl:VRPortHoldItemScene(item, slot) end
    end)
    trace("attach: plane=%d ok=%s", plane, tostring(ok))
    if not ok then return false end
    -- WHICH KIND, for everything that differs between them: how it is armed, which sound, which mesh.
    E.S.grab.kind = nil
    if entity ~= nil then
        E.S.grab.kind = string.match(tostring(entity), "^vrp_gren_(.+)$")
    end
    E.S.grab.side, E.S.grab.slot, E.S.grab.host = side, slot, host
    E.S.grab.plane, E.S.grab.at = plane, now
    E.S.grab.parts, E.S.grab.pin, E.S.grab.obj = nil, nil, nil   -- not resolvable for a frame or two
    pinS.out, pinS.armed, pinS.at, pinS.pull = false, false, nil, nil
    leverS.amount, leverS.key, leverS.shut = 0.0, nil, false  -- a fresh grenade is taken lever-open
    trace("attach: state written")
    -- THE MARK GOES OUT WITH THE THING IT MARKED -- but NOT from here. Writing to components inside
    -- the grab is what turned a working grab into a crash: the run before this call was added took a
    -- grenade and gave it back cleanly, and the two runs after it died inside the attach, before its
    -- own log line. So the hands are cleared and the extinguishing is left to the scan, which owns
    -- those components and runs on its own clock a fraction of a second later.
    E.S.litHand[0], E.S.litHand[1] = nil, nil
    playSnd("grab")
    E.log("grenade -> %s hand (%s, plane %d)", side == 1 and "right" or "left", tostring(entity), plane)
    trace("attach: done")
    return true
end

grenadeRelease = function(pl)
    if E.S.grab.slot == nil then return end
    poseClear()
    magnetClear()
    setTriggerBlock(false)
    leverS.amount, leverS.key, leverS.shut = 0.0, nil, false
    E.S.grab.lever = nil
    -- THE PIN GOES BACK IN. It was switched off on this entity, and the entity outlives the hold: put
    -- one back on the belt and take it again and it came back without a pin -- a grenade that can only
    -- ever be armed once. Home is also its transform, which the hold has been overwriting.
    if E.S.grab.pin ~= nil then
        for i = 1, #E.S.grab.pin do
            local c = E.S.grab.pin[i]
            pcall(function() c:Toggle(true) end)
        end
    end
    -- Before the item goes: the sound is played on it.
    playSnd("stow")
    local slot = E.S.grab.slot
    trace("release start slot=%s", slot)
    pcall(function() pl:VRPortDropItem(slot) end)
    trace("release: dropped")
    E.log("grenade released from %s", slot)
    E.S.grab.side, E.S.grab.slot, E.S.grab.host, E.S.grab.plane = nil, nil, nil, nil
    E.S.grab.parts, E.S.grab.pinned, E.S.grab.obj = nil, nil, nil
    trace("release: done")
end

-- THE FREE HAND AND THE PIN. Three ranges, and none of them is a gesture to be recognised -- they are
-- distances to a point the game's own throw put there.
--
--   preview   the fingers begin taking the shape, so the hand shows it has something to do here
--   grip      the shape is complete and the pull is armed
--   pull      movement ALONG the recorded direction adds up; enough of it and the pin is out. Along,
--             not merely away -- a hand drifting sideways is not pulling a pin, and the dot product is
--             what tells those apart.
-- WHAT THE PIN LOGIC SEES, four times a second while something is held. Guessing at this cost three
-- rounds -- no magnet, and no way to tell which of six conditions was the one failing -- so it says
-- all of them at once: whether it is even running, where the free hand is, how far the target is, and
-- what the grip reads. Behind `grab_trace` like everything else here.
traceAt = 0.0
pinTrace = function(now, fmt, ...)
    if not E.CFG.grab_trace then return end
    if now - traceAt < 0.25 then return end
    traceAt = now
    trace(fmt, ...)
end

updatePin = function(now, dt)
    -- A BUTTON GRENADE NEEDS NO SECOND HAND. The game's own animation brings one -- measured, its thumb
    -- reaches 12 mm of the shell -- but that is an animation of a character, not a thing to ask a
    -- player to do: the button is on the grenade already in your hand, and your own thumb is on the
    -- trigger: a second hand has no job when a button arms it.
    if armMode() == "button" then
        pinPoseTick(nil, 0.0, dt)
        magnetClear()
        return
    end
    if not E.CFG.grenade_pin or E.S.grab.slot == nil or pinS.out then
        pinTrace(now, "pin off: cfg=%s slot=%s out=%s",
            tostring(E.CFG.grenade_pin), tostring(E.S.grab.slot), tostring(pinS.out))
        pinPoseTick(nil, 0.0, dt)
        magnetClear()
        return
    end
    local held = E.S.grab.side
    local free = (held == 1) and 0 or 1

    -- THE GRENADE'S FRAME, from the hold written last frame. A frame stale and it does not matter: a
    -- hand does not cross five centimetres in sixteen milliseconds.
    local ip, iq = pinS.itemP, pinS.itemQ
    if ip == nil or iq == nil then
        pinTrace(now, "pin: no item frame yet (parts=%s)",
            E.S.grab.parts and tostring(#E.S.grab.parts) or "nil")
        pinPoseTick(nil, 0.0, dt)
        return
    end

    local fp
    local ok = pcall(function() fp = VRBoneModelPos(WRIST_BONE[free]) end)
    if not (ok and fp) then pinPoseTick(nil, 0.0, dt) magnetClear() return end
    local cx, cy, cz = pinchPoint(free)
    if cx == nil then pinPoseTick(nil, 0.0, dt) magnetClear() return end

    -- The ring is the TOP of the pin, and PIN_P is the whole part's middle -- so the point the fingers
    -- are asked to meet sits a little above it. A knob rather than a second constant: the mesh gives
    -- the part's centre exactly and says nothing about where on it a hand would take hold.
    -- WHERE THE HAND GOES, per type, WITHOUT LOSING THE TUNING. `PIN_P` is the frag's ring as it was
    -- settled on the picture -- the mesh's own bounding centre plus the nudge in `grenade_pin_off` --
    -- and that is not a number to throw away for a fresh measurement. So the take supplies a DELTA
    -- instead: how far this type's arm point sits from the frag's, in the grenade's own frame.
    -- PER TYPE FIRST, exactly like the exit axis. The measured points are almost the same -- every
    -- grenade with a pin has its ring within 1 to 3 mm of the frag's -- so the shared number is not
    -- wrong; what it is, is unadjustable, and a ring is small enough that 3 mm is the difference
    -- between the fingers landing on it and beside it.
    local offByKind = E.CFG.grenade_pin_off_kind
    local ok_ = E.S.grab.kind
    local off = (type(offByKind) == "table" and ok_ ~= nil) and offByKind[ok_] or nil
    if off == nil then off = E.CFG.grenade_pin_off or {} end
    local dx, dy, dz = 0.0, 0.0, 0.0
    local ti = typeInfo()
    local base = TYPES["frag"]
    if ti ~= nil and ti.pin_p ~= nil and base ~= nil and base.pin_p ~= nil then
        dx = ti.pin_p[1] - base.pin_p[1]
        dy = ti.pin_p[2] - base.pin_p[2]
        dz = ti.pin_p[3] - base.pin_p[3]
    end
    local ppx, ppy, ppz = qrot4(iq[1], iq[2], iq[3], iq[4],
                                PIN_P[1] + (off.x or 0.0) + dx,
                                PIN_P[2] + (off.y or 0.0) + dy,
                                PIN_P[3] + (off.z or 0.0) + dz)
    ppx, ppy, ppz = ip[1] + ppx, ip[2] + ppy, ip[3] + ppz

    -- HOW FAR THE FINGERS ARE, not the wrist. Once the wrist is magnetised its bones stop moving, so
    -- the PULL is watched on the raw tracked hand instead -- but the approach is judged on the drawn
    -- fingers, which are what the player is aiming at the ring.
    local fx, fy, fz = cx, cy, cz
    local d = math.sqrt((fx - ppx) ^ 2 + (fy - ppy) ^ 2 + (fz - ppz) ^ 2)
    E.S.grab.pinDist = d
    pinTrace(now, "pin: free=%d pinch=%.3f wrist=%.3f grip=%s magnet=%s",
        free, d,
        math.sqrt((fp.x - ppx) ^ 2 + (fp.y - ppy) ^ 2 + (fp.z - ppz) ^ 2),
        tostring(gripDown(free)), tostring(pinS.magnet))

    local grip = E.CFG.grenade_pin_grip_m or 0.05
    local prev = E.CFG.grenade_pin_preview_m or 0.12

    if d > prev and pinS.magnet == nil then
        pinS.armed, pinS.at, pinS.pull = false, nil, nil
        pinS.prev, pinS.speed, pinS.grabQ = nil, 0.0, nil
        pinPoseTick(free, 0.0, dt)
        return
    end

    -- PREVIEW is a shape, not a commitment: the fingers take it as the hand closes, so the player can
    -- see this hand has something to do here before pressing anything.
    local want = 1.0
    if d > grip then want = 1.0 - (d - grip) / math.max(1e-3, prev - grip) end
    if want < 0.0 then want = 0.0 end
    pinPoseTick(free, want, dt)

    -- THE GRIP TAKES IT. Within reach and the button down: the wrist is held at the ring and the pull
    -- starts counting from where the raw hand was at that moment.
    if pinS.magnet == nil then
        if d <= grip and gripDown(free) then
            -- IN THE GRENADE'S FRAME, not the world's. What pulls a pin is the two hands SEPARATING,
            -- and which of them does the moving is not the pin's business -- but measured against the
            -- world it is: pulling the free hand away registers, pulling the grenade away with the
            -- other hand does not, and walking forwards registers as a pull with both hands still.
            -- All three were reported, and all three are this one mistake.
            local r0 = rawHand(free)
            local wx, wy, wz = (r0 and r0.x or fx), (r0 and r0.y or fy), (r0 and r0.z or fz)
            local ax0, ay0, az0 = qrot4(-iq[1], -iq[2], -iq[3], iq[4],
                                        wx - ip[1], wy - ip[2], wz - ip[3])
            pinS.at = { ax0, ay0, az0 }
            pinS.armed, pinS.pull = true, { 0.0, 0.0, 0.0 }
            pinS.prev, pinS.speed = nil, 0.0
            -- THE WRIST IS WHAT CAN BE HELD, and the fingers are what should land on the ring -- so
            -- the wrist goes to the ring MINUS the hand's own reach, which keeps the shape the player
            -- is making instead of straightening it.
            local fq = nil
            pcall(function() fq = VRBoneModelRot(WRIST_BONE[free]) end)
            magnetSet(free, ppx - (cx - fp.x), ppy - (cy - fp.y), ppz - (cz - fp.z),
                      fq and fq.i, fq and fq.j, fq and fq.k, fq and fq.r)
            pinS.grabQ = fq and { fq.i, fq.j, fq.k, fq.r } or nil
            trace("pin: magnet on, pinch %.3f from ring", d)
        end
        return
    end

    -- LET GO AND IT IS NOT PULLED. The pin goes back where it was and the hand is its own again.
    if not gripDown(free) then
        magnetClear()
        pinS.armed, pinS.at, pinS.pull = false, nil, nil
        pinS.prev, pinS.speed, pinS.grabQ = nil, 0.0, nil
        trace("pin: released without pulling")
        return
    end

    -- HOW FAR THE HAND HAS TRAVELLED SINCE IT TOOK THE RING. Not a direction test any more: a hand on
    -- a ring is holding it whichever way it goes, and the ring goes with it.
    -- `axial` IS DECLARED HERE ON PURPOSE. The tear-off test further down used to compute it from
    -- `mx`, which is a local of the block below and therefore nil by then -- arithmetic on nil,
    -- thrown every frame the moment the magnet came on, and outside any pcall. It took the whole
    -- belt tick with it: the hand froze at the last target it was given, the hold pose stopped
    -- being written and the pin never moved. Three separate-looking faults, one missing word.
    local travel, axial = 0.0, 0.0
    if pinS.at ~= nil then
        local rp = rawHand(free)
        local wx, wy, wz = (rp and rp.x or fx), (rp and rp.y or fy), (rp and rp.z or fz)
        -- the free hand, expressed in the grenade's own frame: this is the separation, and it moves
        -- when EITHER hand does
        local rx, ry, rz = qrot4(-iq[1], -iq[2], -iq[3], iq[4],
                                 wx - ip[1], wy - ip[2], wz - ip[3])
        local mx, my, mz = rx - pinS.at[1], ry - pinS.at[2], rz - pinS.at[3]
        travel = math.sqrt(mx * mx + my * my + mz * mz)
        E.S.grab.pinPull = travel
        -- ALONG THE AXIS ONLY, and never negative: a pin comes out of its hole or stays where it is,
        -- and no amount of pushing puts it further in.
        local ux, uy, uz = pinAxis()
        axial = mx * ux + my * uy + mz * uz
        local along = axial
        if along < 0.0 then along = 0.0 end
        E.S.grab.pinAlong = along
        -- WHAT THE PIN IS ALLOWED TO DO, which is not the same as what the hand did. A pin slides out
        -- of its hole and then stops -- and STOPPING is what makes the magnet a magnet: held at the
        -- hand's own position it was holding the hand where the hand already was, which is no hold at
        -- all (the magnet was weak, the hand moved away freely). Clamped, the hand is pulled back
        -- to the end of the pin's travel and has to be torn away from it.
        -- HOW FAR THE PIN ITSELF COMES OUT, which is a property of the pin and nothing to do with how
        -- hard it has to be pulled. Those were one number and it was wrong both ways: the hand ran ten
        -- centimetres before meeting any resistance -- far too much -- and by then the pin was
        -- hanging that far out of a grenade it is only about two centimetres long inside.
        local cap = E.CFG.grenade_pin_slide_m or 0.012
        if along > cap then along = cap end
        pinS.pull = { ux * along, uy * along, uz * along }
        pinS.along = along

        -- HOW FAST, because distance alone lets the pin be walked out at any speed and that is not
        -- how a pin comes off -- the hand could still draw it out slowly, without a tug. In the game's own
        -- throw the hand tore away at about 4 m/s, so the bar sits well under that and well over any
        -- speed a hand reaches while merely reaching.
        --
        -- Smoothed over roughly two frames: one frame of controller jitter is metres per second, and
        -- it would take the pin off a hand that never moved. The reload module smooths its own throw
        -- velocity the same way and for the same reason.
        -- ...and the SPEED of that separation, for the same reason. An absolute hand speed says nothing
        -- about a pin: a hand can be fast and stay put relative to the grenade, or slow and be tearing
        -- away from it because the other hand is going the other way.
        if pinS.prev ~= nil then
            local iv = 1.0 / ((dt and dt > 1e-3) and dt or 0.016)
            local sx = (rx - pinS.prev[1]) * iv
            local sy = (ry - pinS.prev[2]) * iv
            local sz = (rz - pinS.prev[3]) * iv
            local v = math.sqrt(sx * sx + sy * sy + sz * sz)
            pinS.speed = pinS.speed + (v - pinS.speed) * 0.45
        end
        pinS.prev = { rx, ry, rz }
        E.S.grab.pinSpeed = pinS.speed
    end

    -- THE HAND GOES WITH THE PIN. The magnet is not a freeze -- it is the correction that puts the
    -- pinch ON the ring -- so its target travels by the same vector the pin does. Held rigidly at the
    -- ring the drawn hand stood still while the pin flew off on its own, which is nobody's idea of
    -- pulling something.
    claimHand(pinS.magnet)
    local tx, ty, tz = ppx, ppy, ppz
    if pinS.pull ~= nil then
        local wx, wy, wz = qrot4(iq[1], iq[2], iq[3], iq[4],
                                 pinS.pull[1], pinS.pull[2], pinS.pull[3])
        tx, ty, tz = tx + wx, ty + wy, tz + wz
    end
    pcall(function()
        VRHandStopModel(pinS.magnet, true,
            Vector4.new(tx - (cx - fp.x), ty - (cy - fp.y), tz - (cz - fp.z), 1.0))
    end)
    if pinS.grabQ ~= nil and type(VRHandStopRot) == "function" then
        local q = pinS.grabQ
        pcall(function() VRHandStopRot(pinS.magnet, 1, q[1], q[2], q[3], q[4]) end)
    end

    if pinS.at ~= nil then
        -- ...AND THE TEAR-OFF is measured from where the pin stopped. Up to `slide` the hand is simply
        -- drawing it out; past that it is pulling against a pin that has nowhere left to go, and it is
        -- THAT pull -- far enough and fast enough -- which takes it off.
        -- Past the stop, measured the same way: how much further ALONG the axis the hand has gone than
        -- the pin was allowed to follow.
        local past = axial - (E.CFG.grenade_pin_slide_m or 0.012)
        if past >= (E.CFG.grenade_pin_pull_m or 0.06)
            and pinS.speed >= (E.CFG.grenade_pin_speed or 2.0) then
            pinS.out, pinS.armed, pinS.pull = true, false, nil
            magnetClear()
            if E.S.grab.pin ~= nil then
                for i = 1, #E.S.grab.pin do
                    local c = E.S.grab.pin[i]
                    pcall(function() c:Toggle(false) end)
                end
            end
            playSnd("pin")
            -- FELT AS WELL AS HEARD: a snap in the hand that pulled it, a weaker one through the grenade
            -- in the other (2 and 3 in src/Runtimes/Haptics.cpp; mask 1 left, 2 right).
            if type(VRHapticEvent) == "function" then
                pcall(VRHapticEvent, 2, free == 1 and 2 or 1)
                pcall(VRHapticEvent, 3, held == 1 and 2 or 1)
            end
            E.log("grenade: pin pulled (%.0f mm past the stop at %.1f m/s)", past * 1000, pinS.speed)
            trace("pin: OUT %.0f mm past the stop at %.1f m/s", past * 1000, pinS.speed)
        end
    end
end

-- THE THROW.
--
-- WHAT IS MEASURED is the hand's own velocity in MODEL space -- the space everything else here is in,
-- and the one in which the player's own travel has already cancelled out. That matters: the launch is
-- given the player's velocity separately (`ownerVelocityProvider` in vrport_hold.reds), so a throw
-- made from a moving car carries the car exactly once instead of twice or not at all.
--
-- Smoothed over roughly two frames, the same filter and the same reason as the reload's thrown
-- magazine: one frame of controller jitter is metres per second, and it would fling a grenade that was
-- merely let go of.
throwS = { prev = nil, vx = 0.0, vy = 0.0, vz = 0.0, pending = nil,
                 peak = 0.0, px = 0.0, py = 0.0, pz = 0.0, peakAt = 0.0,
                 armed = nil, armedAt = 0.0,
                 h = {}, hn = 0 }

-- HOW MANY SAMPLES THE WINDOW CAN HOLD. One per frame; 24 covers a quarter of a second at 90 Hz,
-- which is the window every VR toolkit uses for this (Unity's XR grab calls it "throw smoothing
-- duration" and defaults to 0.25 s).
HIST = 24

-- WHERE THE REAL GRENADES WAIT. Read off the record's own placementSlots, live: a frag grenade allows
-- WeaponLeft, GrenadeLeft and GrenadeRight, and WeaponLeft is the one the game itself uses
-- (`GrenadeLvl4HackEffector`).
--
-- THREE OF THEM, because one is not enough: a grenade in the air still owns the real item that will go
-- off when it lands, so a second throw needs a slot of its own. With one slot the second throw found
-- it occupied, armed nothing, and BOTH grenades were lost -- the new one went back on the belt and the
-- old one never exploded.
THROW_SLOTS = { "AttachmentSlots.WeaponLeft",
                      "AttachmentSlots.GrenadeLeft",
                      "AttachmentSlots.GrenadeRight" }

throwSample = function(side, dt)
    if side == nil then
        throwS.prev, throwS.vx, throwS.vy, throwS.vz = nil, 0.0, 0.0, 0.0
        throwS.peak, throwS.px, throwS.py, throwS.pz = 0.0, 0.0, 0.0, 0.0
        throwS.h, throwS.hn = {}, 0
        E.S.grab.throwSpeed, E.S.grab.throwPeak = nil, nil
        return
    end
    local p = rawHand(side)
    if p == nil then return end
    if throwS.prev ~= nil then
        local iv = 1.0 / ((dt and dt > 1e-3) and dt or 0.016)
        local a = 0.45
        throwS.vx = throwS.vx + ((p.x - throwS.prev[1]) * iv - throwS.vx) * a
        throwS.vy = throwS.vy + ((p.y - throwS.prev[2]) * iv - throwS.vy) * a
        throwS.vz = throwS.vz + ((p.z - throwS.prev[3]) * iv - throwS.vz) * a
    end
    throwS.prev = { p.x, p.y, p.z }
    local sp = math.sqrt(throwS.vx ^ 2 + throwS.vy ^ 2 + throwS.vz ^ 2)

    -- THE PEAK OF THE SWING, over a short window, and NOT the speed at the instant the grip opens.
    --
    -- Measured on the picture: three real throws all came out at 3.6 m/s and the grenade dropped at the
    -- player's feet. That is not a bad filter, it is the wrong moment -- a hand lets go at the END of
    -- the swing, where it is already decelerating into the follow-through, so the instant of release is
    -- the one instant in the whole gesture that does not describe it.
    --
    -- The peak is kept with its DIRECTION, because the two belong together: the fastest sample is also
    -- the one pointing where the arm was actually going, while the release sample points wherever the
    -- wrist happened to be turning as it opened.
    local now = os.clock()
    throwS.hn = throwS.hn + 1
    local slot = (throwS.hn - 1) % HIST + 1
    local e = throwS.h[slot]
    if e == nil then e = {} throwS.h[slot] = e end
    e[1], e[2], e[3], e[4] = throwS.vx, throwS.vy, throwS.vz, now

    E.S.grab.throwSpeed = sp
    -- the readout still wants a peak; the throw computes its own below
    local win = E.CFG.grenade_throw_window or 0.20
    local best = 0.0
    for i = 1, HIST do
        local q = throwS.h[i]
        if q ~= nil and (now - q[4]) <= win then
            local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3])
            if l > best then best = l end
        end
    end
    E.S.grab.throwPeak = best
end

-- THE RELEASE VELOCITY, ported from this port's own basketball, which took it in turn from HIGGS (Skyrim VR, adamhynek), src/hand.cpp GetMaxVelocity.
--
-- The PEAK of the window, averaged with its two neighbours. Not the instant of release: a hand lets
-- go after the arm has begun to slow and curve, so that sample is both slower and aimed elsewhere.
-- Not the average of the window either: that drags the peak of a snap throw back down into the
-- deceleration that follows it, which is the whole reason the window exists. And not the bare peak:
-- one frame of tracking would become the entire throw, so the three samples centred on it keep the
-- magnitude while dropping that sensitivity.
releaseVelocity = function()
    local now = os.clock()
    local win = E.CFG.grenade_throw_window or 0.20
    -- newest-first, in order, so "neighbours" means neighbours in time
    local seq = {}
    local n = math.min(throwS.hn, HIST)
    for k = 0, n - 1 do
        local q = throwS.h[(throwS.hn - 1 - k) % HIST + 1]
        if q ~= nil and (now - q[4]) <= win then seq[#seq + 1] = q end
    end
    if #seq == 0 then return throwS.vx, throwS.vy, throwS.vz end

    local peak, peakLen = 1, -1.0
    for i = 1, #seq do
        local q = seq[i]
        local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3])
        if l > peakLen then peakLen, peak = l, i end
    end
    local p = seq[peak]
    if peak > 1 and peak < #seq then
        local a, b = seq[peak - 1], seq[peak + 1]
        return (a[1] + p[1] + b[1]) / 3.0, (a[2] + p[2] + b[2]) / 3.0, (a[3] + p[3] + b[3]) / 3.0
    end
    return p[1], p[2], p[3]
end

-- MODEL SPACE OUT TO THE WORLD. Everything this module measures is relative to the player; the launch
-- takes a world direction, and the player's own orientation is the whole of the difference.
modelToWorld = function(x, y, z)
    local pl = Game.GetPlayer()
    local q = nil
    pcall(function() q = pl:GetWorldOrientation() end)
    if q == nil then return x, y, z end
    return qrot4(q.i, q.j, q.k, q.r, x, y, z)
end

-- Where the hand points, for a throw that is really a drop: a hand barely moving has no direction of
-- its own, and the palm's is the honest answer.
handForward = function(side)
    local w = nil
    local ok = pcall(function() w = VRBoneModelPos(WRIST_BONE[side]) end)
    if not (ok and w) then return nil end
    local cx, cy, cz = pinchPoint(side)
    if cx == nil then return nil end
    local dx, dy, dz = cx - w.x, cy - w.y, cz - w.z
    local n = math.sqrt(dx * dx + dy * dy + dz * dz)
    if n < 1e-4 then return nil end
    return dx / n, dy / n, dz / n
end

-- THE REAL GRENADE IS PUT IN PLACE WHEN THE PIN COMES OUT, and this is the whole of why the first
-- version threw nothing.
--
-- Measured, two ways round: launched by hand through the bridge -- attach, then launch a second later
-- -- the grenade flew and exploded. Launched by the module -- attach, then launch on the NEXT frame,
-- which is the first frame the item can even be found -- `VRPortThrowGrenade` returned TRUE and
-- nothing happened at all, not even at the player's feet. A queued event that returns true says the
-- event was accepted and nothing about the projectile being ready to take it.
--
-- The game says the same thing in its own code: `GrenadeLvl4HackEffector` does not launch when it
-- attaches. It registers an attachment-slot listener and launches from `OnItemEquippedVisual` -- when
-- the item has actually arrived in the hand. Arming at the pin gives that the seconds it wants, and it
-- is also what the gesture means: a grenade with its pin out is live, and the live one is the real one.
--
-- It is INVISIBLE while it waits (a grenade's meshes are skinned-only and draw nothing off a live
-- skeleton -- the reason this port has props at all), so nothing is seen twice.
-- SPEND ONE. The grenade left the hand, so it should leave the count -- the game does not do it for
-- us, because nothing about this throw went through the game's own throw.
--
-- WHICH RECORD, per slot: by the time a grenade in the air comes down the player may have switched
-- types, and taking one of the wrong kind out of the inventory would be a quiet theft.
consume = function(pl, rec)
    if not E.CFG.grenade_throw_consume then return end
    local mode = E.CFG.grenade_consume_mode or "charge"

    -- CHARGES, which is how the game itself spends a grenade since 2.0. The pool is 0..100, one throw
    -- costs GetGrenadeThrowCostClean (35 on the save this was measured on, so three throws), and it
    -- refills on its own over time. The item stays in the inventory the whole while, because the item
    -- was never the thing being spent.
    if mode == "charge" then
        local ok = false
        if type(pl.VRPortSpendGrenadeCharge) == "function" then
            pcall(function() ok = pl:VRPortSpendGrenadeCharge() end)
        end
        local left = nil
        pcall(function() left = pl:GetGrenadeCharges() end)
        trace("spent a charge -> %s, left %s", tostring(ok), tostring(left))
        if not ok then
            E.log("grenade: VRPortSpendGrenadeCharge missing -- the redscript module needs a restart")
        end
        return
    end

    -- KEPT, DEFAULT OFF, AND HERE IS THE WARNING. `RemoveItem` deletes the ITEM: measured on a live
    -- save, the frag went 1 -> 0, vanished from the inventory for good, and the game switched the
    -- equipped grenade to another type. It is not what a throw does and it never comes back.
    if mode == "item" and rec ~= nil then
        pcall(function()
            local ts = E.transactions()
            if ts == nil then return end
            ts:RemoveItem(pl, ItemID.FromTDBID(TweakDBID.new(rec)), 1)
        end)
        trace("REMOVED THE ITEM %s", tostring(rec))
    end
end

throwArm = function(pl)
    if throwS.armed ~= nil then return end
    local rec = (E.activeGrenade ~= nil) and E.activeGrenade() or nil
    if rec == nil then return end

    -- NEVER CREATE ONE. `VRPortHoldItem` gives the item when the player does not have it -- which is
    -- right for a prop of ours and very wrong here: arming at zero would mint a live grenade out of
    -- nothing, and with the throw spending one afterwards the count would wander. Measured, and the
    -- reason the count matters at all: after a full launch and detonation the inventory still read the
    -- same number, so the game does NOT spend a grenade thrown this way. The port has to, and a port
    -- that both mints and spends is just noise on the count.
    local have = nil
    pcall(function()
        local ts = E.transactions()
        if ts ~= nil then
            have = ts:GetItemQuantity(Game.GetPlayer(), ItemID.FromTDBID(TweakDBID.new(rec)))
        end
    end)
    if have ~= nil and have <= 0 then
        trace("arm: none left of %s", rec)
        return
    end

    -- The first slot nobody is using: not one a grenade in the air is waiting on, and not one that
    -- already has something in it.
    local busy = (FLIGHT ~= nil) and FLIGHT.slotsInUse() or {}
    local slot = nil
    for i = 1, #THROW_SLOTS do
        local c = THROW_SLOTS[i]
        if not busy[c] then
            local filled = false
            pcall(function() filled = pl:VRPortSlotFilled(c) end)
            if not filled then slot = c break end
        end
    end
    if slot == nil then
        trace("arm: no free slot (%d in the air)", (FLIGHT ~= nil) and FLIGHT.count() or 0)
        return
    end
    E.propAllowSlot(rec, slot)
    -- THE SCENE PLANE, not the weapon one, and this is why the thrown grenade was see-through.
    --
    -- `VRPortHoldItem` attaches with ERenderingPlane.RPl_Weapon -- right for something drawn beside the
    -- gun in first person, and wrong for an object that is about to be out in the world. The plane is
    -- decided at the ATTACH and cannot be changed afterwards (this port has measured that twice), so it
    -- has to be chosen here, before the throw exists. Held, the real grenade is invisible either way --
    -- its meshes are skinned-only -- so the scene plane costs nothing until it leaves.
    local ok = false
    pcall(function()
        if type(pl.VRPortHoldItemScene) == "function" then
            ok = pl:VRPortHoldItemScene(rec, slot)
        else
            ok = pl:VRPortHoldItem(rec, slot)
        end
    end)
    if not ok then
        trace("arm: %s would not attach", tostring(rec))
        return
    end
    throwS.armed, throwS.armedAt, throwS.armSlot = rec, os.clock(), slot
    throwS.recBySlot = throwS.recBySlot or {}
    throwS.recBySlot[slot] = rec
    -- AND IT IS OURS, SAY SO. `playerPlane` decides the rendering plane by asking "is there an item in
    -- a hand slot", and the armed grenade is one -- so arming it flipped the player into the weapon
    -- plane, which re-attached the prop, which reset the pin. The grenade disarmed itself in the same
    -- frame it armed, every time. Same shape as the crash the prop caused in that function once
    -- before: anything of ours in a hand slot has to be excluded there, by bookkeeping, not by asking
    -- the object what it is.
    -- EVERY SLOT OF OURS, as a set: `playerPlane` asks "is there an item in a hand slot" and would
    -- otherwise count a waiting grenade as a drawn weapon, flip the plane, and re-attach the prop --
    -- which resets the pin. That loop is what disarmed the throw for a whole evening.
    E.S.grab.armSlots = E.S.grab.armSlots or {}
    E.S.grab.armSlots[slot] = true
    trace("arm: %s in %s", rec, slot)
end

-- ...and it goes away again if the throw never happens.
throwDisarm = function(pl)
    if throwS.armed == nil then return end
    local slot = throwS.armSlot
    throwS.armed, throwS.armSlot = nil, nil
    if slot ~= nil and E.S.grab.armSlots ~= nil then E.S.grab.armSlots[slot] = nil end
    if pl == nil or slot == nil then return end
    pcall(function() pl:VRPortDropItem(slot) end)
    trace("arm: cleared %s", slot)
end

-- ISSUE THE THROW. The prop goes back where it came from and the REAL grenade -- already waiting in
-- its slot since the pin came out -- is the thing that actually leaves the hand: our prop has no
-- projectile in it, it is a visual, and a grenade that does no damage is not a grenade.
grenadeThrow = function(pl, side)
    local vx, vy, vz = releaseVelocity()
    local sp = math.sqrt(vx * vx + vy * vy + vz * vz)
    local nowSp = math.sqrt(throwS.vx ^ 2 + throwS.vy ^ 2 + throwS.vz ^ 2)
    local dx, dy, dz
    if sp > (E.CFG.grenade_throw_dir_min or 0.30) then
        dx, dy, dz = vx / sp, vy / sp, vz / sp
    else
        dx, dy, dz = handForward(side)
        if dx == nil then return false end
    end
    local speed = sp * (E.CFG.grenade_throw_gain or 1.0)
    local lo = E.CFG.grenade_throw_min or 1.5
    local hi = E.CFG.grenade_throw_max or 28.0
    if speed < lo then speed = lo end
    if speed > hi then speed = hi end

    local rec = throwS.armed
    trace("throw: hand=%.2f release=%.2f -> %.2f  model %.2f %.2f %.2f  armed=%s",
        nowSp, sp, speed, dx, dy, dz, tostring(rec))
    if rec == nil then
        E.log("grenade: nothing armed to throw")
        trace("throw: nothing armed")
        return false
    end

    local wx, wy, wz = modelToWorld(dx, dy, dz)
    trace("throw: world %.2f %.2f %.2f", wx, wy, wz)

    -- The prop leaves first, silently: the release path is shared with putting one back on the belt.
    -- `throwS.armed` is cleared above, so the release below leaves the real grenade exactly where it is.
    local sx, sy, sz, sqi, sqj, sqk, sqr
    pcall(function()
        local o = E.S.grab.obj
        if o ~= nil and IsDefined(o) then
            local p = o:GetWorldPosition()
            local q = o:GetWorldOrientation()
            if p ~= nil then sx, sy, sz = p.x, p.y, p.z end
            if q ~= nil then sqi, sqj, sqk, sqr = q.i, q.j, q.k, q.r end
        end
    end)
    trace("throw: prop at %s", sx and string.format("%.1f %.1f %.1f", sx, sy, sz) or "НЕТ")

    muteStow = true
    grenadeRelease(pl)
    muteStow = false

    -- WHERE THE VISIBLE GRENADE IS RIGHT NOW, taken off the prop itself so the thrown mesh starts
    -- exactly where the held one was and there is no jump at the moment of release.

    -- OUR MESH FLIES, and the real grenade stays armed in its slot until the fuse ends -- see the note
    -- at the top of flight.lua for why the game's own launch could never be seen.
    local flew = false
    if FLIGHT == nil then trace("flight: module not loaded") end
    if FLIGHT ~= nil and sx ~= nil and (E.CFG.grenade_throw_own ~= false) then
        -- THE MESH THAT FLIES IS THE ONE THAT WAS IN THE HAND. `propItem` already resolves the
        -- equipped grenade to its prop entity -- frag, flash, smoke and the rest -- so a thrown
        -- flash grenade is a flash grenade in the air too.
        local _, ent = E.propItem({ follow = "grenade" })
        local path = (E.CFG.grenade_prop_dir or "base\\vrport\\") ..
                     (ent or "vrp_gren_frag") .. ".ent"
        -- THE PLAYER'S OWN SPEED GOES WITH IT, and leaving it out is what the wrong throw while
        -- running was. The hand is measured in MODEL space, which is the point of measuring it
        -- there -- the player's travel cancels, so the number is the ARM's swing and nothing else.
        -- That is right for the gesture and wrong for the object: a grenade let go of at a run
        -- leaves the hand already moving at the runner's speed, and ours left with only the arm.
        -- At 5 m/s that is more than the whole throw.
        --
        -- The game's own launch never had this: `MoveComponentVelocityProvider` on the redscript side
        -- adds the owner's velocity for it. Our own flight had nothing.
        local carry = E.CFG.grenade_throw_carry
        if carry == nil then carry = 1.0 end
        local cvx, cvy, cvz = 0.0, 0.0, 0.0
        if carry > 0.0 then
            pcall(function()
                local v = pl:GetVelocity()
                if v ~= nil then cvx, cvy, cvz = v.x * carry, v.y * carry, v.z * carry end
            end)
        end
        if (math.abs(cvx) + math.abs(cvy) + math.abs(cvz)) > 0.05 then
            trace("throw: carry %.2f %.2f %.2f", cvx, cvy, cvz)
        end
        flew = FLIGHT.launch(path, throwS.armSlot,
                             sx, sy, sz, sqi, sqj, sqk, sqr,
                             wx * speed + cvx, wy * speed + cvy, wz * speed + cvz)
        trace("flight: launch=%s at %.1f %.1f %.1f v=%.1f", tostring(flew), sx, sy, sz, speed)
    end
    if not flew then
        -- The old route, kept whole: the game's own launch, invisible but correct.
        throwS.pending = { slot = throwS.armSlot, dx = wx, dy = wy, dz = wz, side = side,
                           speed = speed, at = os.clock(), rec = rec, armedAt = throwS.armedAt }
    end
    if (E.CFG.grenade_throw_consume_at or "throw") == "throw" then consume(pl, rec) end
    -- The slot stays OURS -- the flight owns it now, until it lands and sets that grenade off.
    throwS.armed, throwS.armSlot = nil, nil
    trace("throw: %s at %.1f m/s dir %.2f %.2f %.2f", rec, speed, wx, wy, wz)
    return true
end

-- The pending launch, one frame or two later.
throwTick = function(pl)
    -- THE FLIGHT, and the explosion that ends it. The real grenade has been sitting armed in its slot
    -- since the pin came out; when our mesh stops, that is where the game is asked to set it off.
    if FLIGHT ~= nil then
        local done = FLIGHT.tick(E.S.grab.lastDt or 0.016)
        if done ~= nil then
            for i = 1, #done do
                local d = done[i]
                local ok = false
                if d.slot ~= nil and type(pl.VRPortDetonateGrenadeAt) == "function" then
                    pcall(function() ok = pl:VRPortDetonateGrenadeAt(d.slot, d.x, d.y, d.z) end)
                end
                E.log("grenade: detonated at %.1f %.1f %.1f -> %s", d.x, d.y, d.z, tostring(ok))
                trace("flight: detonate %s at %.1f %.1f %.1f -> %s",
                    tostring(d.slot), d.x, d.y, d.z, tostring(ok))
                if d.slot ~= nil and E.S.grab.armSlots ~= nil then E.S.grab.armSlots[d.slot] = nil end
                -- A flight with no slot is M.testThrow's: nothing was armed, so nothing is spent.
                if d.slot ~= nil and (E.CFG.grenade_throw_consume_at or "throw") == "detonate" then
                    consume(pl, throwS.recBySlot and throwS.recBySlot[d.slot])
                end
                if throwS.recBySlot ~= nil and d.slot ~= nil then throwS.recBySlot[d.slot] = nil end
                if not ok and d.slot ~= nil then
                    -- Nothing went off: do not leave a live grenade attached to the player.
                    pcall(function() pl:VRPortDropItem(d.slot) end)
                end
            end
        end
    end

    -- ARM AND DISARM, every frame, off the one fact that decides it: is the pin out.
    if E.S.grab.slot ~= nil and pinS.out and (E.CFG.grenade_throw ~= false) then
        throwArm(pl)
    elseif throwS.pending == nil and E.S.grab.slot == nil
        and not (FLIGHT ~= nil and FLIGHT.flying()) then
        throwDisarm(pl)
    end

    local p = throwS.pending
    if p == nil then return end
    if type(pl.VRPortThrowGrenade) ~= "function" then
        E.log("grenade: VRPortThrowGrenade is missing -- the redscript module needs a game restart")
        throwS.pending = nil
        return
    end
    local filled = false
    pcall(function() filled = pl:VRPortSlotFilled(p.slot) end)
    -- ...and settled. A grenade armed a moment ago is not ready to be a projectile yet, whatever the
    -- slot says -- that is the difference between the launch that flew and the one that did nothing.
    local ready = (os.clock() - (p.armedAt or 0.0)) >= (E.CFG.grenade_throw_ready or 0.15)
    if filled and ready then
        -- WHAT IS ACTUALLY IN THAT SLOT, read back rather than assumed. The launch is issued on
        -- whatever the slot holds, and "the call returned true" only means an event was queued -- if
        -- the thing it was queued on is not a grenade, true is exactly what you would still get.
        pcall(function()
            local ts = E.transactions()
            local o = ts and ts:GetItemInSlot(pl, TweakDBID.new(p.slot))
            if IsDefined(o) then
                trace("throw: slot holds %s  world %s",
                    tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(o:GetItemID()))),
                    (function()
                        local w = o:GetWorldPosition()
                        return w and string.format("%.1f %.1f %.1f", w.x, w.y, w.z) or "-"
                    end)())
            else
                trace("throw: slot is EMPTY at launch")
            end
        end)
        -- OUT OF THE HAND THAT THREW IT. The item sits in WeaponLeft because that is the slot the
        -- grenade's own record allows; the throw does not have to start there. The newer redscript
        -- entry point takes the player's hand slot and launches from it -- and falls back to the old
        -- one, so a reload of this module alone still throws while the redscript waits for a restart.
        local hand = (p.side == 1) and (E.CFG.grenade_throw_hand_r or "RightHand")
                                   or (E.CFG.grenade_throw_hand_l or "LeftHand")
        local ok, called = false, false
        if type(pl.VRPortThrowGrenadeFrom) == "function" then
            called = pcall(function()
                ok = pl:VRPortThrowGrenadeFrom(p.slot, hand, p.dx, p.dy, p.dz, p.speed)
            end)
            trace("throw: from %s call=%s ret=%s", hand, tostring(called), tostring(ok))
        else
            called = pcall(function() ok = pl:VRPortThrowGrenade(p.slot, p.dx, p.dy, p.dz, p.speed) end)
            trace("throw: (old entry, restart pending) call=%s ret=%s", tostring(called), tostring(ok))
        end
        E.log("grenade: thrown %s at %.1f m/s -> %s", p.rec, p.speed, tostring(ok))
        trace("throw: launched %s", tostring(ok))
        if ok then playSnd("throw") end
        -- WHETHER ONE IS SPENT is left OFF until it has been looked at: the item came out of the
        -- player's own inventory and the game may or may not have counted it already. Removing one that
        -- the game already took is a grenade stolen per throw; removing one whose entity is still in
        -- flight might take the projectile with it. Both are worth seeing before either is chosen.
        if ok and (E.CFG.grenade_throw_consume_at or "throw") == "detonate" then
            consume(pl, p.rec)
        end
        throwS.pending = nil
        if p.slot ~= nil and E.S.grab.armSlots ~= nil then E.S.grab.armSlots[p.slot] = nil end
    elseif os.clock() - p.at > 1.0 then
        E.log("grenade: the real one never resolved in %s -- throw abandoned", p.slot)
        pcall(function() pl:VRPortDropItem(p.slot) end)
        throwS.pending = nil
        if p.slot ~= nil and E.S.grab.armSlots ~= nil then E.S.grab.armSlots[p.slot] = nil end
    end
end

-- THE HELD ITEM'S OWN MESH COMPONENTS, cached.
--
-- Ported from the reload module's item route, including the two things that are silent if got wrong:
-- the entity is NOT resolvable on the frame it was asked for, so this returns nil for a frame or two
-- and must be retried; and a component that ships switched off stays off -- turning it on here would
-- light up a twin that is meant to be dark.
grenadeParts = function()
    if E.S.grab.parts ~= nil then return E.S.grab.parts end
    if E.S.grab.slot == nil then return nil end
    local pl = Game.GetPlayer()
    local ts = E.transactions()
    if pl == nil or ts == nil then return nil end
    local e = nil
    pcall(function() e = ts:GetItemInSlot(pl, TweakDBID.new(E.S.grab.slot)) end)
    if not IsDefined(e) then return nil end
    local list = {}
    pcall(function()
        for _, c in ipairs(Game.VREntityComponents(e)) do
            if string.find(tostring(c:GetClassName()), "Mesh", 1, true)
                and type(c.SetLocalTransform) == "function" then
                local en = nil
                pcall(function() en = c.isEnabled end)
                if en ~= false then list[#list + 1] = c end
            end
        end
    end)
    if #list == 0 then return nil end
    E.S.grab.parts = list
    E.S.grab.obj = e            -- what the sounds are played on, once it exists
    -- THE PIN IS SEVERAL COMPONENTS, and not the obvious one.
    --
    -- A grenade prop carries each mesh three times: the game's own SKINNED component, which is the one
    -- named `..._safe_pin_01` and which draws NOTHING -- a skinned mesh with no live skeleton is why
    -- the port had to make these props in the first place -- and two plain copies the port added,
    -- `mag_mesh_NN` and `vrp_lit_N`, which are what is actually seen. Searching for "safe_pin" finds
    -- only the invisible one, so the pin was being moved and hidden where nobody could see it.
    --
    -- The generator wrote all three groups in the same order, so the pin's INDEX among the skinned
    -- components names its plain twins: index 4 gives `mag_mesh_04` and `vrp_lit_4`.
    E.S.grab.pin = nil
    pcall(function()
        local idx, seen = nil, 0
        for _, c in ipairs(Game.VREntityComponents(e)) do
            local cn = tostring(c:GetName().value)
            if string.find(tostring(c:GetClassName()), "SkinnedMesh", 1, true) then
                if string.find(string.lower(cn), "safe_pin", 1, true) then idx = seen break end
                seen = seen + 1
            end
        end
        if idx == nil then return end
        local want = {
            [(idx == 0) and "mag_mesh" or string.format("mag_mesh_%02d", idx)] = true,
            ["vrp_lit_" .. tostring(idx)] = true,
        }
        local pins = {}
        for _, c in ipairs(list) do
            local cn = tostring(c:GetName().value)
            if want[cn] or string.find(string.lower(cn), "safe_pin", 1, true) then
                pins[#pins + 1] = c
            end
        end
        if #pins > 0 then E.S.grab.pin = pins end
    end)
    -- A PIN IS WHOLE WHEN A GRENADE IS TAKEN, always. The prop entity is reused between holds -- one
    -- record, one item -- so a pin switched off on the last grenade is still off on the next one, and
    -- every grenade after the first came out already armed. Putting it back at RESOLVE time covers
    -- that whatever happened to the previous hold and however it ended.
    if E.S.grab.pin ~= nil and not pinS.out then
        for i = 1, #E.S.grab.pin do
            local c = E.S.grab.pin[i]
            pcall(function() c:Toggle(true) end)
        end
        trace("pin: restored on a fresh grenade")
    end
    trace("parts resolved: %d, pin parts: %s", #list,
        E.S.grab.pin and tostring(#E.S.grab.pin) or "0")
    return list
end

-- WHERE THE GAME HOLDS A GRENADE, MEASURED, in the WRIST's own frame.
--
-- Against the hand ANCHOR the game adds nothing at all -- 0.0 mm and 0.00 degrees across 165 frames --
-- which is why attaching to the slot looked like the whole answer. It is not, and the reason is that
-- the take was recorded with VRIK OFF: there the animation drives the anchor and the anchor is the
-- hand. With VRIK on, the TRACKER drives the wrist and nothing drives the anchor, so the two part
-- company and a thing hung on the anchor is left wherever the idle rig put it.
--
-- The wrist is the bone that means the same in both worlds, so the hold is stored against it:
--
--     position  -0.0830  0.0516  0.0267 m   -- 101 mm from the wrist
--     rotation  -0.731265 -0.032951 0.109304 0.672472   -- 95.48 degrees
--
-- Recorded on the RIGHT hand. The left is given the same numbers: the take has a grenade in the left
-- for a quarter of a second and that is not enough to measure a hold from.
-- wrist and hand-anchor bone indices, per hand, from the recorder's own list
WRIST_BONE  = { [0] = 23, [1] = 24 }
ANCHOR_BONE = { [0] = 26, [1] = 28 }

qmul4 = function(ax, ay, az, aw, bx, by, bz, bw)
    return aw * bx + ax * bw + ay * bz - az * by,
           aw * by - ax * bz + ay * bw + az * bx,
           aw * bz + ax * by - ay * bx + az * bw,
           aw * bw - ax * bx - ay * by - az * bz
end

qrot4 = function(i, j, k, r, x, y, z)
    local tx, ty, tz = 2.0 * (j * z - k * y), 2.0 * (k * x - i * z), 2.0 * (i * y - j * x)
    return x + r * tx + (j * tz - k * ty),
           y + r * ty + (k * tx - i * tz),
           z + r * tz + (i * ty - j * tx)
end

-- THE SAME TRANSFORM TO EVERY PART, and that is not an approximation: each piece's place inside the
-- prop is baked into its own mesh, not into its component transform, so composing the offset onto
-- each part's own local transform would apply that placement twice. The reload module measured this
-- the hard way and its note says so; this is the same prop shape and the same answer.
--
-- WHAT IS WRITTEN is the step from where the engine puts the item -- the anchor -- to where the hold
-- says it belongs, off the wrist:
--
--     local = anchor^-1  *  wrist  *  recorded
--
-- Both bones are read fresh every frame, so nothing here assumes the anchor is anywhere in particular.
grenadeHoldPose = function()
    local parts = grenadeParts()
    if parts == nil then return end
    local side = E.S.grab.side
    if side == nil then return end

    local wp, wq, ap, aq
    local ok = pcall(function()
        wp = VRBoneModelPos(WRIST_BONE[side]);  wq = VRBoneModelRot(WRIST_BONE[side])
        ap = VRBoneModelPos(ANCHOR_BONE[side]); aq = VRBoneModelRot(ANCHOR_BONE[side])
    end)
    if not (ok and wp and wq and ap and aq) then return end

    -- the hold, in model space
    local HP, HQ = holdNow()
    local hx, hy, hz = qrot4(wq.i, wq.j, wq.k, wq.r, HP[1], HP[2], HP[3])
    hx, hy, hz = wp.x + hx, wp.y + hy, wp.z + hz
    local mi, mj, mk, mr = qmul4(wq.i, wq.j, wq.k, wq.r, HQ[1], HQ[2], HQ[3], HQ[4])
    -- WHERE THE GRENADE ENDED UP, in model space, kept for the pin: it is the only frame both the
    -- recording and the live hand agree on, and it is computed here anyway.
    pinS.itemP = { hx, hy, hz }
    pinS.itemQ = { mi, mj, mk, mr }

    -- ...expressed in the anchor's frame, which is what a component's local transform is
    local li, lj, lk, lr = qmul4(-aq.i, -aq.j, -aq.k, aq.r, mi, mj, mk, mr)
    local lx, ly, lz = qrot4(-aq.i, -aq.j, -aq.k, aq.r, hx - ap.x, hy - ap.y, hz - ap.z)

    -- ...plus whatever the panel is nudging it by, which ships at zero and exists so the measurement
    -- can be corrected on the picture without touching the numbers it came from.
    -- THE NUDGE IS PER TYPE. Neither window of the cutting grenade's take reads right in the hand, and
    -- picking between two wrong numbers is not a method -- so the correction is tuned on the picture,
    -- for the type being held, and cannot disturb the ones that already sit correctly.
    local o = E.CFG.grenade_hold or {}
    local byKind = E.CFG.grenade_hold_kind
    if type(byKind) == "table" and E.S.grab.kind ~= nil and byKind[E.S.grab.kind] ~= nil then
        o = byKind[E.S.grab.kind]
    end
    local ni, nj, nk, nr = E.eulerQuat(o.rx, o.ry, o.rz)
    li, lj, lk, lr = qmul4(li, lj, lk, lr, ni, nj, nk, nr)
    lx, ly, lz = lx + (o.x or 0.0), ly + (o.y or 0.0), lz + (o.z or 0.0)

    -- THE PIN IS NOT WRITTEN HERE ONCE IT IS BEING PULLED. Every other part of the grenade sits on the
    -- one transform; the pin has to leave the hand that holds the body, so while a hand is on it, it
    -- gets a transform of its own -- the same one, walked along the pull direction by however far the
    -- hand has actually travelled. That is the difference between a pin that VANISHES and a pin that
    -- comes out.
    local pin = E.S.grab.pin
    local isPin = {}
    if pin ~= nil then for i = 1, #pin do isPin[pin[i]] = true end end
    for i = 1, #parts do
        local c = parts[i]
        if not (isPin[c] and pinS.pull ~= nil) then
            pcall(function()
                c:SetLocalTransform(Vector4.new(lx, ly, lz, 1.0), Quaternion.new(li, lj, lk, lr))
            end)
        end
    end
    if pin ~= nil and pinS.pull ~= nil then
        -- WHERE THE HAND WENT, and nowhere else. The first version walked the pin along a direction
        -- taken from the recorded throw, and it was wrong three ways at once: the pin left upwards
        -- instead of along the pull, it moved further than the hand did, and the hand -- pinned by the
        -- magnet -- stood still while its own pin flew away.
        --
        -- Following the hand's own travel fixes all three by construction. Pull a little and it comes
        -- out a little; push back and it goes back in; let go and it is home. It is a vector, in model
        -- space, turned into the anchor's frame like everything else here.
        -- `pinS.pull` is in the GRENADE's frame; a component's local transform is in the anchor's, so
        -- it goes out through model space -- the item's own rotation first, then back into the anchor.
        local wx, wy, wz = qrot4(mi, mj, mk, mr, pinS.pull[1], pinS.pull[2], pinS.pull[3])
        local ax, ay, az = qrot4(-aq.i, -aq.j, -aq.k, aq.r, wx, wy, wz)
        for i = 1, #pin do
            local c = pin[i]
            pcall(function()
                c:SetLocalTransform(Vector4.new(lx + ax, ly + ay, lz + az, 1.0),
                                    Quaternion.new(li, lj, lk, lr))
            end)
        end
    end
end

loadPose = function(name)
    local ok, t = pcall(function() return require("grenade/poses/" .. name) end)
    return (ok and t) or nil
end

-- The settled grip, lever under the thumb -- 165 frames of the game's own hold, after the pin.
-- WHAT EACH TYPE MEASURES OUT AS, generated by tools/build_grenade_types.py from one recorder take
-- per grenade: where it sits in the holding wrist before and after it is armed, where the free hand
-- arrives, and whether that hand pulls a ring or presses a button.
--
-- The differences are small and they are real -- 11 mm between the frag and the incendiary, which by
-- the standard of something held in VR is not noise.
TYPES = (function()
    local ok, t = pcall(function() return require("grenade/poses/types") end)
    return (ok and t) or {}
end)()

-- WHICH TYPES ARE ALLOWED THEIR OWN DATA, one by one, and nothing is rolled out to all of them at
-- once. The frag was working and got changed underneath by a generalisation it never needed -- so a
-- type opts IN, by name, and every type that is not on the list behaves exactly as it did before any
-- of this existed.
customFor = function(kind)
    if kind == nil then return false end
    local c = E.CFG.grenade_custom
    return (type(c) == "table") and c[kind] == true
end

typeInfo = function()
    local k = E.S.grab.kind
    if not customFor(k) then return nil end
    return TYPES[k]
end

-- The per-type finger poses, loaded on first use and remembered. A type with no take of its own falls
-- back to the frag's, which is what every grenade used before this.
POSE_CACHE = {}

-- THE THUMB PRESSING THE BUTTON, and this one is AUTHORED rather than measured -- the only such number
-- in the grenade.
--
-- The game has no animation of it: in its own takes the button is pressed by the OTHER hand's thumb
-- (12 mm from the shell, measured), and the holding hand never moves. Since the port arms a button
-- grenade with its own trigger, there was nothing to show for it at all -- no
-- press animation, exactly.
--
-- So the hold pose is bent at the thumb. The axis is the one the recorded curls already use: every
-- finger joint in these poses turns about its local Z, so the press is a rotation about Z composed
-- onto the two thumb joints, and how far is a knob.
PRESS_CACHE = {}

pressPose = function(base, deg)
    if base == nil then return nil end
    local key = string.format("%d", math.floor(deg * 10.0 + 0.5))
    local got = PRESS_CACHE[key]
    if got ~= nil then return got end
    local half = math.rad(deg) * 0.5
    local sz, cz = math.sin(half), math.cos(half)
    local out = {}
    for i = 1, #base do
        local p = base[i]
        local nm = p[1]
        if nm == "LeftHandThumb1" or nm == "LeftHandThumb2" then
            -- q_new = q * rot_z(deg)
            local x, y, z, w = p[2], p[3], p[4], p[5]
            out[i] = { nm,
                       x * cz + y * sz,
                       y * cz - x * sz,
                       z * cz + w * sz,
                       w * cz - z * sz }
        else
            out[i] = p
        end
    end
    PRESS_CACHE[key] = out
    return out
end

poseFor = function(which, fallback)
    local k = E.S.grab.kind
    if k == nil then return fallback end
    -- THE PRESS IS NOT PART OF THE OPT-IN. `grenade_custom` exists to keep a type's measured HOLD away
    -- from a hold that was tuned and works -- but a button grenade's thumb press has no tuned version
    -- to protect: without `<kind>_thumb` there is nothing to show at all.
    if which ~= "thumb" and not customFor(k) then return fallback end
    local key = k .. "_" .. which
    local got = POSE_CACHE[key]
    if got == nil then
        got = loadPose(key) or false
        POSE_CACHE[key] = got
    end
    return got or fallback
end

GRENADE_POSE = loadPose("grenade_hold")
-- The shape the hand closes into as it takes the thing, before the pin is touched.
PINCH_POSE = loadPose("grenade_pinch")
-- The free hand on the ring: the ten frames between the hands meeting and the tear-away.
PIN_POSE = loadPose("grenade_pin_left")

-- THE PIN, and the pull that takes it out -- both in the HOLDING wrist's frame, so they travel with
-- the hand and nothing is recomputed when the player moves.
--
--   PIN_P     where the FREE WRIST sits while its fingers are on the ring: 227 mm from the holding
--             wrist. The wrist and not the ring, because the wrist is what can be measured on a live
--             player; the fingers reach the rest of the way.
--   PULL_DIR  the direction the free hand leaves in: 393 mm in 0.11 s, about 3.6 m/s.
-- IN THE GRENADE'S OWN FRAME, not the wrist's -- and that is the whole of why the first attempt never
-- triggered. The take was recorded with VRIK off, where the wrist is animation-driven; with VRIK on
-- the player's wrist is wherever the tracker says, at whatever roll, so a point stored in ITS frame
-- lands somewhere the hand never goes. The grenade is different: this module puts it there, so its
-- frame is known exactly and means the same in both worlds.
--
--   PIN_P     THE RING ITSELF, out of the geometry: the bounding-box centre of the grenade's own
--             `..._safe_pin_01` mesh, 22 mm from the item's origin. Every part of the prop shares one
--             transform -- each piece's place is baked into its own mesh -- so a part's bbox centre in
--             mesh space IS where that part sits relative to the item.
--
--             The first version put this at 65 mm, taken from where the recorded FINGERS were, and it
--             held the hand three to four centimetres short of the ring. Fingers approach a thing;
--             they do not sit on its centre. The geometry does not have that problem.
--   PULL_DIR  where the pinch goes when it tears away: 441 mm in 0.11 s, from the take.
--
-- BY THE FINGERS AND NOT THE WRIST, which is the whole of why the first two attempts never armed: the
-- pinch sits 98 mm ahead of the wrist, so with the fingers already ON the ring the wrist still reads
-- 115 mm away (fingers on the pin, readout 0.115). Measuring the hand by its wrist and then asking
-- for five centimetres is asking for something that cannot happen.
PIN_P = { -0.0005, -0.0188, 0.0125 }
PULL_DIR = { 0.044, -0.723, 0.689 }

-- THE PIN'S OWN AXIS, in the grenade's frame: straight out from the body through the ring, which is
-- the only way a pin in a hole can travel. Taken as the direction of PIN_P, normalised -- the ring's
-- offset from the item's origin IS that outward direction.
--
-- Everything the hand does is projected onto it, and that answers three complaints at once: circling
-- the hand carried the pin around with it (a circle has no component along the axis, so now it moves
-- nothing), the pin drifted sideways out of its hole, and pushing forward drove it back INTO the
-- grenade -- which the clamp at zero below now forbids.
-- THE DEFAULT comes from the geometry -- the ring's offset from the item's origin, which is the
-- direction a pin in a hole travels -- but it is only a default. The bounding box of a part says where
-- that part is and nothing about which way its shaft runs, and 34 degrees of it turned out to be up:
-- the pin comes out a little upwards. So the axis is a knob, normalised wherever it is set.
pinAxis = function()
    -- PER TYPE FIRST. The ring points out of a different face on every grenade, and one axis tuned on
    -- the frag sent the incendiary's pin off upwards. The panel writes into this table for whichever
    -- grenade is in the hand, so tuning one type cannot disturb another.
    local byKind = E.CFG.grenade_pin_axis_kind
    local k = E.S.grab.kind
    local a = (type(byKind) == "table" and k ~= nil) and byKind[k] or nil
    if a == nil then a = E.CFG.grenade_pin_axis end
    local x, y, z = -0.0005, -0.0188, 0.0125
    if a ~= nil then x, y, z = a.x or x, a.y or y, a.z or z end
    local n = math.sqrt(x * x + y * y + z * z)
    if n < 1e-6 then return 0.0, -1.0, 0.0 end
    return x / n, y / n, z / n
end

-- Thumb tip and index tip, per hand, out of the player skeleton -- the same indices the recorder's
-- finger rows are built from.
THUMB_TIP = { [0] = 55, [1] = 60 }
INDEX_TIP = { [0] = 65, [1] = 69 }

-- WHERE THE HAND IS PINCHING, in model space, or nil if the fingers cannot be read.
pinchPoint = function(hand)
    local a, b
    local ok = pcall(function()
        a = VRBoneModelPos(THUMB_TIP[hand])
        b = VRBoneModelPos(INDEX_TIP[hand])
    end)
    if not (ok and a and b) then return nil end
    return (a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5
end

-- TWO HOLDS, NOT ONE. Between the frames before the pull and the four seconds after it the grenade
-- moves 6.8 mm and turns 8.63 degrees in the hand. On a flat screen that rounds to nothing; on an
-- object half a metre from the eye in VR it is plainly visible -- so
-- both are kept and the pin decides which is in force.
HOLD1_P = { -0.0767, 0.0493, 0.0254 }              -- 13 frames, t=1.40..1.64, pin still in
HOLD1_Q = { 0.7317, -0.0221, -0.1596, -0.6623 }

HOLD_P = { -0.0830, 0.0516, 0.0267 }
HOLD_Q = { -0.731265, -0.032951, 0.109304, 0.672472 }

-- The free hand's state lives here because the hold depends on it: the pin being out is what selects
-- the second grip, for the transform and for the fingers alike.
pinS = { hand = nil, blend = 0.0, armed = false, at = nil, out = false,
               magnet = nil, pull = nil, itemP = nil, itemQ = nil,
               prev = nil, speed = 0.0, grabQ = nil, along = 0.0 }

-- THE TRIGGER, per hand, as an analog 0..1 off the plugin's shared block: 160 is the right, 154 the
-- left. Reading it here has no effect on the game -- these slots carry the controller INTO the port;
-- what the game sees is a separate channel, blocked below.
TRIGGER_SLOT = { [0] = 154, [1] = 160 }

triggerAmount = function(side)
    if side == nil or type(GetVRSharedSlot) ~= "function" then return 0.0 end
    local v = 0.0
    pcall(function() v = GetVRSharedSlot(TRIGGER_SLOT[side]) or 0.0 end)
    -- A trigger is 0..1. Anything else means the slot is carrying somebody else's data -- the smoking
    -- module lost an evening to exactly that -- so it is refused rather than clamped.
    if v ~= v or v < 0.0 or v > 1.0 then return 0.0 end
    return v
end

-- WHAT THE GAME SEES ON THAT TRIGGER, which is not the same question. Shared slot 161: 0 pass it
-- through, 1 swallow it, 2 press it fully. With a grenade in the hand a squeeze is the lever, not a
-- shot, and the gun must not go off underneath it.
--
-- Written EVERY FRAME while it is blocking, and only ONCE when it stops. The slot has another writer
-- -- the reload's revolver hammer, which sets 0 whenever no gun is held -- and a single write that
-- somebody else overwrites is a trigger that quietly comes back to life. The one thing that must never
-- happen is a latched 1 outliving the grenade, which is a gun that never fires again, so every path
-- that ends a hold clears it.
trgS = { blocking = false }

setTriggerBlock = function(on)
    if type(SetVRTriggerMode) ~= "function" then return end
    if on then
        trgS.blocking = true
        pcall(function() SetVRTriggerMode(1) end)
    elseif trgS.blocking then
        trgS.blocking = false
        pcall(function() SetVRTriggerMode(0) end)
    end
end

-- HOW FAR THE LEVER IS PRESSED, 0..1, ramped rather than switched so the finger closes on it instead
-- of snapping shut.
leverS = { amount = 0.0, key = nil, pose = nil, shut = false, armAt = nil }
holdPBuf = { 0.0, 0.0, 0.0 }
holdQBuf = { 0.0, 0.0, 0.0, 1.0 }

-- THE PRESS IS THE TWO RECORDED HOLDS, CROSS-FADED. `grenade_pinch` is the hand the moment it takes
-- the grenade and `grenade_hold` the settled grip with the lever under the thumb; measured against
-- each other they differ by 6.8 mm and 8.63 degrees, and mostly in the thumb -- which is the lever.
--
-- It is an approximation and worth saying so: the game never lets the lever go, so there is no take of
-- a truly open hand anywhere, and `grenade_lever_open` exists to push the released end further open
-- than the pinch if the movement reads too small on the picture.
-- WHICH OF THE TWO HOLDS IS THE PRESSED ONE, settled by looking rather than by reasoning.
--
-- The first version had it the other way about, on the argument that `grenade_pinch` is the hand at
-- the moment it catches the grenade and `grenade_hold` the settled grip with the lever under the
-- thumb. In the headset that ran backwards -- so the ends are
-- named here instead of being inferred, and every user of them goes through these names.
--
-- It is worth saying why the reasoning was worth so little: both takes come from a throw in which the
-- game NEVER releases the lever, so neither is an open hand and which of the two is "more closed" was
-- never something the measurement could answer. Only the picture could.
-- Which end of the lever is which. Filled per type in holdNow, the frag's own as the starting value.
local OPEN_P,  OPEN_Q,  OPEN_POSE  = HOLD_P,  HOLD_Q,  GRENADE_POSE
local SHUT_P,  SHUT_Q,  SHUT_POSE  = HOLD1_P, HOLD1_Q, PINCH_POSE

leverPose = function(t)
    local a, b = OPEN_POSE, SHUT_POSE
    if a == nil or b == nil or #a ~= #b then return b or a end
    local key = math.floor(t * 100.0 + 0.5)
    if leverS.pose ~= nil and leverS.key == key then return leverS.pose end
    if leverS.pose == nil then
        -- Built once and mutated in place afterwards: this runs every frame a grenade is held, and a
        -- fresh table of nineteen tables per frame is garbage nobody needs.
        local buf = {}
        for i = 1, #a do buf[i] = { a[i][1], 0.0, 0.0, 0.0, 1.0 } end
        leverS.pose = buf
    end
    for i = 1, #a do
        local p, q = a[i], b[i]
        -- q and -q are the same rotation; blending them without this cancels the bone out
        local d = p[2] * q[2] + p[3] * q[3] + p[4] * q[4] + p[5] * q[5]
        local sg = (d >= 0.0) and 1.0 or -1.0
        local x = p[2] + (q[2] * sg - p[2]) * t
        local y = p[3] + (q[3] * sg - p[3]) * t
        local z = p[4] + (q[4] * sg - p[4]) * t
        local w = p[5] + (q[5] * sg - p[5]) * t
        local n = math.sqrt(x * x + y * y + z * z + w * w)
        if n < 1e-6 then n = 1.0 end
        local e = leverS.pose[i]
        e[2], e[3], e[4], e[5] = x / n, y / n, z / n, w / n
    end
    leverS.key = key
    return leverS.pose
end

-- WHICH HOLD IS IN FORCE, transform and fingers together -- they were measured as one thing and they
-- move as one thing.
-- HOW EACH GRENADE IS ARMED, and this is not a guess: the game's own animations name it.
--
--     pma_w_explosives_00N__*.anims -> the sound event on the equip timeline
--         grenade_arm_pin      001 frag, 002 flash, 003 piercing, 004 incendiary, 005 biohazard
--         grenade_arm_button   006 dronelure, 007 recon, 008 cutting, 009 sonic (the EMP)
--
-- The meshes agree, which is what makes it safe to build on: frag, flash, incendiary, biohazard and
-- smoke each ship a `..._safe_pin_01` mesh, and recon and cutting do not. Smoke has no per-type
-- animation at all -- it uses the shared `pma_grenade.anims` -- so its pin is read off the mesh.
--
-- A BUTTON TYPE HAS NO SECOND-HAND STEP. There is nothing to pull: the thumb presses it, which is the
-- trigger this module already reads for the lever, and the grenade is live.
ARM_MODE = {
    frag = "pin", flash = "pin", incendiary = "pin", biohazard = "pin",
    piercing = "pin", smoke = "pin", ozob = "pin",
    recon = "button", cutting = "button", emp = "button", dronelure = "button",
}

armMode = function()
    -- The take's own answer first -- it was generated from the same animations this table was read
    -- from, so they agree; the table is what makes a type work before anyone has recorded it.
    local ti = typeInfo()
    if ti ~= nil and ti.pin ~= nil then return ti.pin and "pin" or "button" end
    local kind = E.S.grab.kind
    return (kind ~= nil and ARM_MODE[kind]) or "pin"
end

holdNow = function()
    -- PER TYPE, when there is a take for it. `p1/q1` is the hand as it takes the grenade -- the lever
    -- open -- and `p2/q2` the settled grip; which end is which was settled on the picture and is the
    -- same for every type, so only the numbers change here.
    -- THE FRAG'S HOLD FOR EVERY TYPE, by default, and that is a judgement the picture made against the
    -- measurement.
    --
    -- The takes really do differ: the biohazard's origin sits 31 mm from the frag's in the wrist's
    -- frame, and the recon, cutting and EMP are held at a completely different angle -- two families of
    -- quaternion, not noise. But the frag's placement is the one that was tuned and looked right on
    -- every grenade (the earlier placement sat right in the hand), and a measurement that makes the
    -- picture worse loses to the picture.
    --
    -- The table is not thrown away: `grenade_hold_per_type` turns it on, and the per-type FINGER POSES
    -- and arming modes are used either way -- those were never in question.
    -- ONE GATE, AND IT IS THE LIST. `grenade_hold_per_type` was the first attempt at this and it stayed
    -- behind as a second, older veto: `typeInfo` allowed the recon through by name and the next line
    -- threw it away again, so switching types on in `grenade_custom` changed nothing at all. Two gates
    -- for one decision is one gate too many.
    local ti = typeInfo()
    if ti ~= nil and ti.p1 ~= nil and ti.p2 ~= nil then
        OPEN_P, OPEN_Q, OPEN_POSE = ti.p2, ti.q2, poseFor("hold", GRENADE_POSE)
        SHUT_P, SHUT_Q, SHUT_POSE = ti.p1, ti.q1, poseFor("pinch", PINCH_POSE)
        -- A BUTTON GRENADE HAS NO LEVER, so the trigger has nothing to cross-fade: both ends are the
        -- settled hold and the thing simply does not move in the hand. On the cutting grenade the fade
        -- ran the other way and slid it sideways, which is exactly what a lever that does not exist
        -- looks like when it is animated anyway.
        if not ti.pin then
            -- THE PRESSED HAND IS THE RESTING HAND, and that is the second time this file has learned
            -- the same lesson: a grenade is CARRIED with the thumb already on its button, exactly as
            -- the frag is carried with the lever already under the thumb. So the idle end is the
            -- pressed one and the trigger moves AWAY from it -- the
            -- press is already on in the way it is held, so it runs the other way.
            --
            -- The travel is the take's own and small: 7.5 mm on the cutting grenade, 7.4 on the EMP,
            -- 11.9 on the recon.
            -- TWO SHAPES OF BUTTON GRENADE, because the two takes are not the same gesture.
            --
            --   "lift"  (cutting, EMP)  What is on the screen while holding IS the pressed hand -- the
            --           thumb is already down on the button. So the PRESSED end is exactly that, and
            --           the idle end is the same hand with the thumb LIFTED off it. The grenade itself
            --           does not move at all.
            --
            --   "swap"  (recon)         Left exactly as it was approved: the idle hand is the recorded
            --           press and the trigger moves to the settled grip. Not to be touched.
            local mode = (E.CFG.grenade_press_mode or {})[E.S.grab.kind] or "lift"
            local basePose = poseFor("hold", GRENADE_POSE)
            if mode == "swap" then
                OPEN_P, OPEN_Q = ti.p1, ti.q1
                SHUT_P, SHUT_Q = ti.p2, ti.q2
                OPEN_POSE = poseFor("thumb", nil) or basePose
                SHUT_POSE = basePose
            else
                OPEN_P, OPEN_Q = ti.p2, ti.q2
                SHUT_P, SHUT_Q = ti.p2, ti.q2
                -- THE LIFT IS POSITIVE ABOUT Z. `pressPose` was written to bend a thumb ONTO a button and
                -- the negative of it looked like the same thing again -- the thumb went down on the
                -- picture either way. Measured against the report rather than reasoned: this rig's
                -- thumb lifts on +Z, so the sign that was "obviously" the release was the press.
                SHUT_POSE = basePose
                OPEN_POSE = pressPose(basePose, E.CFG.grenade_button_lift_deg or 22.0) or basePose
            end
        end
    else
        -- EXACTLY WHAT WORKED, and not one file more. These two poses and these two transforms are the
        -- frag's, tuned on the picture and approved -- and swapping in freshly generated per-type files
        -- underneath them broke the frag, which had never been in question. A generalisation that
        -- changes the case it was generalised FROM is not a generalisation, it is a regression.
        OPEN_P, OPEN_Q, OPEN_POSE = HOLD_P, HOLD_Q, GRENADE_POSE
        SHUT_P, SHUT_Q, SHUT_POSE = HOLD1_P, HOLD1_Q, PINCH_POSE
        -- A button grenade has no lever on the frag's numbers either: the transform stays put and only
        -- the thumb moves.
        -- THE ONE THING A BUTTON GRENADE ADDS, and it adds nothing to the others. `<kind>_thumb` is the
        -- THROWING hand at the moment its own thumb is furthest out -- 46 to 64 mm on the button kinds
        -- against 11 to 22 for a lever, so the press really is on that hand and really is measured.
        -- The grenade itself does not move: only the thumb does.
        if armMode() == "button" then
            SHUT_P, SHUT_Q = HOLD_P, HOLD_Q
            SHUT_POSE = poseFor("thumb", nil)
                     or pressPose(OPEN_POSE, E.CFG.grenade_button_curl_deg or 22.0) or OPEN_POSE
        end
    end
    local o = E.CFG.grenade_lever_open or 0.0
    local t = o + (1.0 - o) * (leverS.amount or 0.0)
    if t > -0.001 and t < 0.001 then return OPEN_P, OPEN_Q, OPEN_POSE end
    if t >= 0.999 then return SHUT_P, SHUT_Q, SHUT_POSE end

    holdPBuf[1] = OPEN_P[1] + (SHUT_P[1] - OPEN_P[1]) * t
    holdPBuf[2] = OPEN_P[2] + (SHUT_P[2] - OPEN_P[2]) * t
    holdPBuf[3] = OPEN_P[3] + (SHUT_P[3] - OPEN_P[3]) * t

    local d = OPEN_Q[1] * SHUT_Q[1] + OPEN_Q[2] * SHUT_Q[2]
            + OPEN_Q[3] * SHUT_Q[3] + OPEN_Q[4] * SHUT_Q[4]
    local sg = (d >= 0.0) and 1.0 or -1.0
    local n = 0.0
    for i = 1, 4 do
        holdQBuf[i] = OPEN_Q[i] + (SHUT_Q[i] * sg - OPEN_Q[i]) * t
        n = n + holdQBuf[i] * holdQBuf[i]
    end
    n = math.sqrt(n)
    if n < 1e-6 then n = 1.0 end
    for i = 1, 4 do holdQBuf[i] = holdQBuf[i] / n end

    return holdPBuf, holdQBuf, leverPose(t)
end

-- SOUND. Every name here was read out of the game's OWN grenade animations -- an animation carries its
-- audio as `animAnimEvent_Sound` entries on its own timeline, so playing the event name plays exactly
-- what the game plays, rather than something that resembles it. The physical reload was built the same
-- way, off the weapons' .anims, and for the same reason.
--
--     base\animations\items\player\grenade\pma_grenade.anims
--         equip / cg_equip        -> grenade_equip
--         unequip / cg_unequip    -> grenade_unequip
--         cg_charge_start         -> w_expl_frag_grenade_foley_raise_cover
--         cg_charge_end           -> w_expl_frag_grenade_foley_lower_cover
--         cg_throw / throw_quick  -> grenade_throw          (the throw is not built yet)
--     ...\w_explosives_001__frag_grenade\pma_w_explosives_001__frag_grenade.anims
--         grenade_equip           -> grenade_arm_pin        the pin, on the equip timeline
--
-- THE LEVER'S PAIR is the one inference in the list: `cg_charge_start` is the game cooking a grenade in
-- the hand and its foley is named for a cover being raised, which is what a safety lever is. Every name
-- is a live key, so a better one can replace it without touching this file.
SND = {
    grab     = "grenade_equip",
    stow     = "grenade_unequip",
    throw    = "grenade_throw",
    pin      = "grenade_arm_pin",
    button   = "grenade_arm_button",
    lever    = "w_expl_frag_grenade_foley_raise_cover",
    leverOff = "w_expl_frag_grenade_foley_lower_cover",
}

-- Played on the GRENADE, so it comes from the hand rather than from the player's centre -- and that
-- matters more in VR than it does flat. The player is the fallback for the frame or two before the
-- item's entity resolves, which is exactly when the grab sound has to fire.
muteStow = false
sndAt = nil

playSnd = function(which)
    if E.CFG.grenade_sound == false then return end
    -- A throw ends the hold too, and it goes through the same release path -- but a grenade leaving the
    -- hand at speed does not sound like one being put back on the belt.
    if which == "stow" and muteStow then return end
    local name = E.CFG["grenade_snd_" .. which]
    if name == nil then name = SND[which] end
    if name == nil or name == "" then return end
    local id = nil
    pcall(function()
        local o = E.S.grab.obj
        if o ~= nil and IsDefined(o) then id = o:GetEntityID() end
    end)
    if id == nil then pcall(function() id = Game.GetPlayer():GetEntityID() end) end
    if id == nil then return end
    -- EVERY SOUND IS TRACED. They are rare -- a grab, an arming, a throw -- so the line costs nothing,
    -- and a sound that misbehaves is almost always a sound played too often rather than the wrong one.
    -- Without this, a sound that seems broken has no evidence behind it at all.
    -- NOT TWICE IN A BREATH. Traced from the game: the lever pair fired five times in two seconds while
    -- the trigger sat near its threshold, which is what a broken sound sounds like. The hysteresis on
    -- the lever is wide already, so this is about the finger genuinely wobbling, not about the code --
    -- and a foley that long simply must not restart that often.
    local now = os.clock()
    sndAt = sndAt or {}
    local gap = (which == "lever" or which == "leverOff")
                and (E.CFG.grenade_snd_gap_lever or 0.45)
                or (E.CFG.grenade_snd_gap or 0.12)
    if (now - (sndAt[which] or -99.0)) < gap then return end
    sndAt[which] = now
    -- WHO IS MAKING THE SOUND, and why only some types have to say.
    --
    -- `AudioSystem:Play(event, entityID, emitterName)` -- the third argument is the emitter, and it is
    -- normally left empty because the prop's own entity carries a `gameaudioSoundComponent` whose
    -- `audioName` registers it: `gre_ent_cutting`, `gre_ent_frag_grenade` and so on. The EMP is the one
    -- grenade whose visual entity ships NO such component (six components in total, against the cutting
    -- grenade's fifty-five), so its emitter was never registered and every event went nowhere -- the EMP
    -- worked, only without sound. The name it should have is in the game's own audio metadata, which
    -- lists exactly nine: gre_ent_frag_grenade, _flash, _incendiary, _biohazard, _recon, _cutting,
    -- _smoke, _emp and _sonic_bubble.
    --
    -- Named PER TYPE and empty for everyone else on purpose: the seven that already sound right resolve
    -- their emitter from the asset, and there is nothing to gain by re-routing them.
    local emitter = (E.CFG.grenade_snd_emitter or {})[E.S.grab.kind]
    trace("snd %s (%s) emitter=%s", which, tostring(name), tostring(emitter or "-"))
    pcall(function()
        Game.GetAudioSystem():Play(CName.new(name), id, CName.new(emitter or ""))
    end)
end

-- Ramped toward whatever the trigger says, every frame the grenade is held.
--
-- ONCE THE PIN IS OUT THE LEVER IS NOT OPTIONAL: let go of it and the thing goes off, so it is held
-- whatever the trigger reads. Releasing it after the pin is exactly what the THROW will hang off, so
-- this is the line that changes when the throw is built.
leverTick = function(dt)
    local side = E.S.grab.side
    if side == nil or not E.CFG.grenade_lever then
        leverS.amount, leverS.key, leverS.shut = 0.0, nil, false
        E.S.grab.lever = nil
        return
    end
    local want = pinS.out and 1.0 or triggerAmount(side)
    local step = (dt or 0.016) * (E.CFG.grenade_lever_speed or 8.0)
    if leverS.amount < want then leverS.amount = math.min(want, leverS.amount + step)
    else leverS.amount = math.max(want, leverS.amount - step) end
    E.S.grab.lever = leverS.amount

    -- ONE CLICK PER PRESS, and the two edges are far apart on purpose: a single threshold at a finger
    -- resting halfway would chatter the sound at frame rate.
    -- A BUTTON GRENADE HAS NO LEVER, so it has no lever foley either: what its trigger does is arm it,
    -- and the game's own sound for that is `grenade_arm_button`.
    local hasLever = armMode() == "pin"
    if not leverS.shut and leverS.amount >= 0.7 then
        leverS.shut = true
        -- ON A BUTTON GRENADE THE TRIGGER IS THE BUTTON. Held long enough it goes live, with the
        -- game's own `grenade_arm_button` under the thumb; from then on letting go throws it, which is
        -- the same rule a pulled pin gives the others.
        if armMode() == "button" and not pinS.out then
            leverS.armAt = os.clock()
        end
        if hasLever then playSnd("lever") end
    elseif leverS.shut and leverS.amount <= 0.3 then
        leverS.shut = false
        leverS.armAt = nil
        if hasLever then playSnd("leverOff") end
    end

    -- The press has to be HELD: a brush of the trigger does not arm a grenade.
    if armMode() == "button" and not pinS.out and leverS.armAt ~= nil
        and (os.clock() - leverS.armAt) >= (E.CFG.grenade_button_hold_s or 0.35) then
        pinS.out, leverS.armAt = true, nil
        playSnd("button")
        E.log("grenade: armed by button")
        trace("armed: button")
    end


end


handBone = function(hand, name)
    if hand == 1 then
        if string.sub(name, 1, 4) == "Left" then return "Right" .. string.sub(name, 5) end
    elseif string.sub(name, 1, 5) == "Right" then
        return "Left" .. string.sub(name, 6)
    end
    return name
end

-- The plugin nlerps a pose onto the live tracked fingers by a blend factor, so the curl arrives instead
-- of snapping -- the same treatment every grip in the reload gets, and the reason a hand closing on a
-- grenade does not look like a cut.
poseS = { hand = nil, blend = 0.0 }

poseApply = function(hand, blend, pose)
    if pose == nil then local _, _, p = holdNow() pose = p end
    if pose == nil or type(VRReloadFingerSet) ~= "function" then return end
    pcall(function()
        VRReloadFingerClear(hand)
        for i = 1, #pose do
            local p = pose[i]
            VRReloadFingerSet(hand, handBone(hand, p[1]), p[2], p[3], p[4], p[5])
        end
        if type(VRReloadFingerBlend) == "function" then VRReloadFingerBlend(hand, blend) end
        VRReloadFingerApply(hand, 1)
    end)
end

poseClear = function()
    if poseS.hand == nil then return end
    pcall(function() VRReloadFingerApply(poseS.hand, 0) end)
    poseS.hand, poseS.blend = nil, 0.0
end

-- Ramp in while the grenade is held, ramp out when it is gone. `dt` is the belt's own frame step.
poseTick = function(hand, dt)
    if not E.CFG.grenade_grab_pose then
        if poseS.hand ~= nil then poseClear() end
        return
    end
    if hand == nil then
        if poseS.hand ~= nil then
            poseS.blend = poseS.blend - (dt or 0.016) * 6.0
            if poseS.blend <= 0.0 then poseClear() else poseApply(poseS.hand, poseS.blend) end
        end
        return
    end
    if poseS.hand ~= nil and poseS.hand ~= hand then poseClear() end
    poseS.hand = hand
    poseS.blend = math.min(1.0, poseS.blend + (dt or 0.016) * 6.0)
    poseApply(hand, poseS.blend)
end

-- THE FREE HAND, on its own fade. A different hand from the one wearing the hold, and the plugin's
-- finger natives take the hand as an argument, so the two never contend.
pinPoseTick = function(hand, want, dt)
    if hand == nil or want <= 0.0 then
        if pinS.hand ~= nil then
            pinS.blend = pinS.blend - (dt or 0.016) * 6.0
            if pinS.blend <= 0.0 then
                pcall(function() VRReloadFingerApply(pinS.hand, 0) end)
                pinS.hand, pinS.blend = nil, 0.0
            else
                poseApply(pinS.hand, pinS.blend, poseFor(armMode() == "pin" and "pin" or "press", PIN_POSE))
            end
        end
        return
    end
    if pinS.hand ~= nil and pinS.hand ~= hand then
        pcall(function() VRReloadFingerApply(pinS.hand, 0) end)
        pinS.blend = 0.0
    end
    pinS.hand = hand
    local step = (dt or 0.016) * 6.0
    if pinS.blend < want then pinS.blend = math.min(want, pinS.blend + step)
    else pinS.blend = math.max(want, pinS.blend - step) end
    poseApply(hand, pinS.blend, poseFor(armMode() == "pin" and "pin" or "press", PIN_POSE))
end

-- HOLDING A WRIST WHERE IT SHOULD BE. The same two natives the reload uses for its grips: one pins
-- the drawn wrist at a model-space point, the other its rotation. Both are plugin-side state and MUST
-- be released -- a hand left magnetised outlives the grenade and points at a pin that is gone.
--
-- ONE OWNER PER HAND is the port's own rule, learned on the collision work: two writers on one wrist
-- is a shake, not an average. Nothing else in this module touches a wrist, and the reload only takes
-- one while a weapon is being held, which is not while a grenade is.
-- CLAIMING THE WRIST IS NOT OPTIONAL, and this is what a magnet that never held was missing.
--
-- The hand-collision module runs its own pass every frame and ends it with, in its own words, a
-- release of every wrist that is not claimed:
--
--     if ownRel ~= 0 then VRHandStopModel(0, false, ...) end
--     if ownRel ~= 1 then VRHandStopModel(1, false, ...) end
--
-- `ownRel` is shared slot 162, and `SetVRReloadOwnedHand` is what writes it. So a hold placed without
-- a claim is cancelled in the same frame it is made -- the pull logged, and no magnet -- exactly: the
-- arithmetic ran, the hand was simply let go again before it could be seen.
--
-- Claimed every frame while the magnet holds, because the claim is a statement about THIS frame and
-- the two mods have no guaranteed order between them.
claimHand = function(hand)
    if type(SetVRReloadOwnedHand) ~= "function" then return end
    pcall(function() SetVRReloadOwnedHand((hand == 0 or hand == 1) and hand or -1) end)
end

magnetSet = function(hand, x, y, z, qi, qj, qk, qr)
    if type(VRHandStopModel) ~= "function" then return end
    claimHand(hand)
    pcall(function() VRHandStopModel(hand, true, Vector4.new(x, y, z, 1.0)) end)
    -- AND THE ROTATION. Position alone leaves the wrist free to turn, and a hand that can spin on a
    -- ring it is supposedly gripping does not read as holding anything. Locked to whatever the wrist
    -- had when it took hold, so the shape the player made is the shape that stays.
    if qi ~= nil and type(VRHandStopRot) == "function" then
        pcall(function() VRHandStopRot(hand, 1, qi, qj, qk, qr) end)
    end
    pinS.magnet = hand
end

magnetClear = function()
    if pinS.magnet == nil then return end
    local h = pinS.magnet
    pinS.magnet = nil
    claimHand(nil)
    pcall(function() VRHandStopModel(h, false, Vector4.new(0, 0, 0, 0)) end)
    if type(VRHandStopRot) == "function" then
        pcall(function() VRHandStopRot(h, 0, 0, 0, 0, 1) end)
    end
end

-- WHERE THE HAND REALLY IS, as opposed to where it is drawn. Once the wrist is magnetised its BONE
-- stops moving, so a pull measured off the bone can never register -- it is the raw tracked hand that
-- travels, and that is the one the pull has to watch.
rawHand = function(hand)
    if type(VRHandRawModel) ~= "function" then return nil end
    local p = nil
    pcall(function() p = VRHandRawModel(hand) end)
    return p
end

gripDown = function(side)
    if type(GetVRSharedSlot) ~= "function" then return false end
    local v = 0.0
    pcall(function() v = GetVRSharedSlot(GRIP_SLOT[side]) or 0.0 end)
    return v > 0.5
end

-- PUT THE WEAPON AWAY, the way the holster does it: the equipment system's own UnequipWeapon, so the
-- game's state machine holsters it and empties the hand slot itself. `VRHolsterMelee` is that same
-- request plus a stamp that stops the motion melee reading the reach as a swing (a swing in progress is
-- what made a melee unequip get dropped); the plain request is the fallback without the holster module.
unequipWeapon = function(pl)
    local done = false
    if type(pl.VRHolsterMelee) == "function" then
        done = pcall(function() pl:VRHolsterMelee() end)
    end
    if done then return end
    pcall(function()
        local req = EquipmentSystemWeaponManipulationRequest.new()
        req.owner = pl
        req.requestType = EquipmentManipulationAction.UnequipWeapon
        Game.GetScriptableSystemsContainer():Get("EquipmentSystem"):QueueRequest(req)
    end)
end

-- Put the grenade in a hand, or take it out again.
--
-- WHERE IT SITS IS NOT TUNED, it is where the game itself puts one. Measured off the game's own throw
-- (reload_record_01, tools/read_throw_take.py): the held grenade's transform and the WeaponRight
-- ANCHOR bone were the same point to within 0.1 mm for the whole hold. So attaching to the hand slot
-- is the placement -- there is no offset to find, and none to get wrong.
--
-- THE PLANE IS CHOSEN AT ATTACH and cannot be changed afterwards; a hand with no gun is drawn in the
-- scene plane and a hand with one in the weapon plane, so following the player means DETACHING and
-- attaching again with the other variant. That is what `plane` is remembered for.

updateGrenadeGrab = function(now, dt)
    local pl = Game.GetPlayer()
    if pl == nil then return end
    E.S.grab.lastDt = dt
    -- The fingers follow the hold, in both directions, whatever ends it -- and WHICH hold, since the
    -- grip before the pin is not the grip after it.
    -- THE LEVER FIRST: the hold pose and the hold transform both read it, and a frame-late lever is a
    -- finger that presses after the trigger is already back up.
    leverTick(dt)
    -- The hand's speed, every frame it holds something, and the pending launch whether it does or not.
    throwSample(E.S.grab.slot ~= nil and E.S.grab.side or nil, dt)
    throwTick(pl)
    -- ...and the gun stays quiet while the thing is in the hand.
    setTriggerBlock(E.S.grab.slot ~= nil and E.CFG.grenade_trigger_block ~= false)
    poseTick(E.S.grab.slot ~= nil and E.S.grab.side or nil, dt)
    updatePin(now, dt)

    if not E.CFG.grenade_grab then
        if E.S.grab.slot ~= nil then grenadeRelease(pl) end
        return
    end

    -- HELD: let go when the grip does, and re-attach if the player changed plane under it.
    if E.S.grab.slot ~= nil then
        local side = E.S.grab.side
        -- PINNED FROM THE PANEL stays put. Tuning how the thing sits in the hand means looking at it
        -- from several angles with both hands free for the sliders, and a hold that ends the moment
        -- the grip is released cannot be looked at at all.
        if E.S.grab.pinned then
            grenadeHoldPose()
            E.S.litHand[0], E.S.litHand[1] = nil, nil
            return
        end
        if not gripDown(side) then
            -- AN ARMED GRENADE DOES NOT GO BACK ON THE BELT. With the pin out, letting go IS the throw
            -- -- there is nothing else it could mean -- and a hand that is barely moving still lets it
            -- go, which is a grenade dropped at your feet and behaves exactly as it should.
            local armed = pinS.out and (E.CFG.grenade_throw ~= false)
            trace("release: pin_out=%s throw_cfg=%s speed=%.2f",
                tostring(pinS.out), tostring(E.CFG.grenade_throw),
                E.S.grab.throwSpeed or -1.0)
            if armed and grenadeThrow(pl, side) then
                E.S.litHand[0], E.S.litHand[1] = nil, nil
                E.S.grab.was[0], E.S.grab.was[1] = gripDown(0), gripDown(1)
                return
            end
            grenadeRelease(pl)
        else
            -- The pose is written every frame on purpose: the slot puts the item back on the anchor
            -- each time the skeleton is posed, so a single write would last exactly one frame.
            grenadeHoldPose()
            -- Nothing of ours may relight the element that is in the hand, and the cheapest way to
            -- mean it is to keep saying so: the scan runs on its own clock and this costs a table
            -- write when nothing is lit anyway.
            -- Same reason as above: say what should be lit, never write it from here.
            E.S.litHand[0], E.S.litHand[1] = nil, nil
            local plane = E.playerPlane(pl, now) or 0
            local forced = E.CFG.grenade_grab_plane or 0
            if forced == 1 then plane = 0 elseif forced == 2 then plane = 2 end
            if plane ~= E.S.grab.plane then
                local host = E.S.grab.host
                trace("replane %s -> %s", tostring(E.S.grab.plane), tostring(plane))
                -- A PIN THAT IS OUT STAYS OUT ACROSS A RE-ATTACH. The prop is torn down and rebuilt
                -- here for the rendering plane alone, and `grenadeAttach` quite rightly starts a fresh
                -- grenade with its pin in -- but this is the SAME grenade, and the player pulled that
                -- pin. Losing it here is what disarmed the throw; that particular replane is gone now,
                -- but a legitimate one -- the player draws a gun with a live grenade in the other hand
                -- -- would do it again.
                local wasOut = pinS.out
                grenadeRelease(pl)
                grenadeAttach(pl, side, host, now)
                pinS.out = wasOut
            end
        end
        E.S.grab.was[0], E.S.grab.was[1] = gripDown(0), gripDown(1)
        return
    end

    -- A GRAB WAITING FOR THE WEAPON TO GO (see below). It completes the moment no weapon is left in a
    -- hand slot, as an ordinary grab from that holder; letting go, or a weapon that is still there after
    -- two seconds, drops it.
    local pend = E.S.grab.pending
    if pend ~= nil then
        if not gripDown(pend.side) or (now - pend.at) > 2.0 then
            trace("pending grab dropped: grip=%s waited=%.2f", tostring(gripDown(pend.side)), now - pend.at)
            -- A grip still closed after the wait has had its turn: the hand opens before it tries again.
            if gripDown(pend.side) then E.S.grab.spent = E.S.grab.spent or {}; E.S.grab.spent[pend.side] = true end
            E.S.grab.pending = nil
        elseif E.weaponInHands(pl) == false then
            E.S.grab.pending = nil
            grenadeAttach(pl, pend.side, pend.host, now)
        end
        E.S.grab.was[0], E.S.grab.was[1] = gripDown(0), gripDown(1)
        return
    end

    -- FREE: a grip that CLOSES while that hand is at the grenade holder takes it. With the grip read from the
    -- fingers a closed grip is enough: the hand can be closed already when it reaches the belt.
    local closedTakes = false
    if type(VRGripFromFingers) == "function" then pcall(function() closedTakes = VRGripFromFingers() == true end) end
    E.S.grab.spent = E.S.grab.spent or {}
    for side = 0, 1 do
        local down = gripDown(side)
        local edge = down and (closedTakes and not E.S.grab.spent[side] or not E.S.grab.was[side])
        E.S.grab.was[side] = down
        if not down then E.S.grab.spent[side] = nil end
        if edge and E.S.grab.slot == nil then
            local host = E.S.litHand[side]
            -- The proximity test the mark already ran IS the "at the belt" condition -- see the note
            -- on E.CFG.grenade_grab. Nothing else can start a grab.
            if host ~= nil and string.sub(host, 1, 13) == "ucbeltgrenade" then
                -- NEVER INTO A HAND A WEAPON IS IN. The holders do not light while a weapon is held
                -- (init.lua, updateHighlight), so this is the second line, for a grip that lands in the
                -- moment between a draw and the next highlight tick. Attaching anyway is the bug this
                -- guards: the grenade took the weapon's slot with high priority, the equipment system
                -- still believed the gun was in the hand, and after the throw the hand stayed in the
                -- pistol grip with the holster unable to draw. So the weapon is put away properly first
                -- and the grenade is taken once the slot is empty.
                if E.weaponInHands ~= nil and E.weaponInHands(pl) == true then
                    trace("grab with a weapon in hand: unequip first (side=%d)", side)
                    unequipWeapon(pl)
                    E.S.grab.pending = { side = side, host = host, at = now }
                else
                    grenadeAttach(pl, side, host, now)
                end
            end
        end
    end
end

-- PUT ONE IN THE HAND AND LEAVE IT THERE, from the panel. `host` is the belt element it should be
-- taken from -- the caller knows which one is showing; nil means "do not hide anything".
function M.pin(side, host)
    local pl = Game.GetPlayer()
    if pl == nil then return false end
    if E.S.grab.slot ~= nil then grenadeRelease(pl) end
    local ok = grenadeAttach(pl, side, host, os.clock())
    E.S.grab.pinned = ok or nil
    return ok
end

function M.unpin()
    local pl = Game.GetPlayer()
    E.S.grab.pinned = nil
    if pl ~= nil then grenadeRelease(pl) end
end

function M.pinned() return E.S.grab.pinned == true end

-- A THROW WITHOUT A HAND, to see where a thrown grenade comes down (2026-10-18), from the
-- CET console:
--     GetMod("CyberpunkVRPort_TacticalBelt").grenade.testThrow(6, 2)
-- The equipped grenade's prop leaves 0.4 m ahead at hand height along the view, `speed` m/s forward and
-- `up` m/s up, and flies exactly as a thrown one does (throwTick ticks it). No real grenade is armed for
-- it, so it sets nothing off: it lies where it stopped until the fuse and is gone.
function M.testThrow(speed, up)
    if FLIGHT == nil then return false end
    local pl = Game.GetPlayer()
    if pl == nil then return false end
    local ok, launched = pcall(function()
        local p = pl:GetWorldPosition()
        local f = Game.GetCameraSystem():GetActiveCameraForward()
        local h = math.sqrt(f.x * f.x + f.y * f.y)
        if h < 1e-3 then return false end
        local fx, fy = f.x / h, f.y / h
        local _, ent = E.propItem({ follow = "grenade" })
        local path = (E.CFG.grenade_prop_dir or "base\\vrport\\") .. (ent or "vrp_gren_frag") .. ".ent"
        local v = speed or 6.0
        return FLIGHT.launch(path, nil, p.x + fx * 0.4, p.y + fy * 0.4, p.z + 1.4, 0.0, 0.0, 0.0, 1.0,
                             fx * v, fy * v, up or 2.0)
    end)
    trace("testThrow: %s", tostring(ok and launched))
    return ok and launched == true
end

M.disarm = throwDisarm
M.tick = updateGrenadeGrab
M.release = grenadeRelease
M.trace = trace

return M
