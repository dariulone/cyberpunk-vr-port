-- CyberpunkVRPort — VR smoking gestures (input bridge).
--
-- All proximity/gesture logic now lives in redscript (VRSmokeTick), which uses the REAL world
-- positions of the cig tip (cigar_tip slot), the lighter and the head (FPP camera) -- far more
-- accurate than the controller distance (the controllers floor at ~0.20 because the flame/tip
-- stick out from the hands). This module only feeds the VR inputs from the shared-memory bridge:
--   [154] left trigger analog -> lighter ignite (>=0.95) / light the cig when the flame meets it
--   [160] right trigger analog -> the same, left-handed (the lighter is in the right hand)
--   [49]  right grip (binary) -> near the mouth: cig INTO mouth, or OUT into the RIGHT hand
--   [155] left grip  (binary) -> near the mouth: cig INTO mouth, or OUT into the LEFT hand
--
-- The left pair used to be [67] and [68], on the strength of a line in shared_slots.h that called
-- them free. They are not. [67] carries the hand-sample millisecond stamp and [68] a QPC
-- timestamp -- both enormous next to a 0..1 trigger, so the lighter sat at "full squeeze" forever
-- and lit itself, and the left grip never read as released, which kept re-slotting the cigarette
-- out of the pose the .ini files define. Slots 154/155 are named in shared_slots.h now.

-- ONE DEBUG SWITCH FOR THE WHOLE PORT, read from shared slot [156].
--
-- The plugin republishes the launcher's DEBUG checkbox there every frame, so this bridge obeys the
-- same switch as everything else and can be flipped without editing a file. It used to be a
-- hardcoded `local DEBUG = true`, which is how one session left 26 449 lines and 5 MB of per-frame
-- state in this mod's log alone.
--
-- Cached for a quarter second: this is called from onUpdate and a shared-memory read per frame to
-- decide whether to not log is a poor trade.
local dbgCache, dbgAt = false, -1.0
local function vrDebug()
    local now = (os and os.clock and os.clock()) or 0.0
    if now - dbgAt > 0.25 then
        dbgAt = now
        dbgCache = (type(GetVRSharedSlot) == 'function') and (GetVRSharedSlot(156) > 0.5) or false
    end
    return dbgCache
end
local logAcc = 0.0

registerForEvent('onUpdate', function(dt)
    dt = dt or 0.016
    if type(GetVRSharedSlot) ~= 'function' then return end
    local pl = Game.GetPlayer()
    if not pl then return end

    local lt    = GetVRSharedSlot(154)
    local rt    = GetVRSharedSlot(160)
    local gripR = (GetVRSharedSlot(49)  > 0.5)
    local gripL = (GetVRSharedSlot(155) > 0.5)
    -- A trigger is 0..1. Anything else means the slot is carrying someone else's data again, and
    -- acting on it is what set the lighter alight on its own -- so refuse rather than guess.
    if not (lt >= 0.0 and lt <= 1.0) or not (rt >= 0.0 and rt <= 1.0) then return end

    local ok = pcall(function() pl:VRSmokeTick(lt, rt, gripR, gripL, dt) end)
    if not ok then return end

    if vrDebug() then
        logAcc = logAcc + dt
        if logAcc >= 0.4 then
            logAcc = 0.0
            local lit  = pl:VRSmokeIsLit()
            local ld   = pl:VRSmokeLastLightDist()
            local md   = pl:VRSmokeLastMouthDist()
            local mdL  = pl:VRSmokeLastMouthDistL()
            spdlog.info(string.format("[Smoke] lit=%s lt=%.2f rt=%.2f lightDist=%.3f mouthR=%.3f mouthL=%.3f gripR=%s gripL=%s",
                tostring(lit), lt, rt, ld, md, mdL, tostring(gripR), tostring(gripL)))
        end
    end
end)
