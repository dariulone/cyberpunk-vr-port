-- CyberpunkVRPort — VR basketball (tick bridge + measurement console).
--
-- The physics lives in redscript (VRBallTick). This module only supplies the real frame delta,
-- because the bounce top-up is computed from a numerically differentiated velocity and a wrong
-- dt shows up directly as a wrong impulse: v = dp/dt, and J = m*(e(v)-e_engine)*v.
--
-- Nothing here is a "feel" slider. The one number that must be measured before any of this is
-- trustworthy is the engine's own restitution, and DropTest() prints it.
--
-- Console (CET):
--   Game.GetPlayer():VRBallToggle()              spawn / despawn in front of you
--   Game.GetPlayer():VRBallSetCorrection(false)  raw engine bounce
--   Game.GetPlayer():VRBallDropTest(1.8)         drop from 1.8 m, prints e_effective
--   Game.GetPlayer():VRBallStatus()              position / kinematic / simulated

local DEBUG_SLOT = 156      -- the port-wide DEBUG switch, republished by the plugin every frame

local dbgCache, dbgAt = false, -1.0
local function vrDebug()
    local now = (os and os.clock and os.clock()) or 0.0
    if now - dbgAt > 0.25 then
        dbgAt = now
        dbgCache = (type(GetVRSharedSlot) == 'function') and (GetVRSharedSlot(DEBUG_SLOT) > 0.5) or false
    end
    return dbgCache
end

local logAcc = 0.0

-- Take the body capsules out of scene QUERIES, and keep them out.
--
-- The arm capsules carry the "World Dynamic" filter, because that is the only preset MEASURED to give them
-- contacts at all -- "Player Hitbox" and "Ragdoll Inner" both let a ball fly straight through and land
-- 1.8 m behind the player. But world geometry is exactly what a character controller must not be standing
-- inside: every capsule overlaps the controller's 0.25 m cylinder, and the first attempt at this launched
-- the player into the sky.
--
-- A PhysX character controller moves by sweeping scene queries; a ball hits things by simulation contacts.
-- Clearing IsQueryable keeps the contacts and hides the capsules from the movement system, which is the
-- same trick that stopped the player climbing over the ball.
--
-- Re-applied on a timer rather than once: a component that re-registers (state change, teleport, reload)
-- would come back queryable, and one queryable frame at chest depth is a shove.
local capsCache, capsAt, capsScanAt, capsMissAt = nil, -1.0, -1.0, -10.0

local function capsQueriesOff(now, pl)
    -- A scan that found nothing is not repeated for a second. A player without the capsules (the main menu's
    -- puppet) otherwise walked every component name through CET each frame: hundreds of userdata and strings a
    -- frame, ~18 MB/s of garbage, and every ~15 s a 340 ms GC atomic phase -- the menu's periodic freeze
    -- (2026-10-28).
    if not capsCache and now - capsMissAt < 1.0 then return end
    if not capsCache or now - capsScanAt > 5.0 then
        -- Empty scans used to convert every component name through CET every
        -- frame. The native prefilter still checks the current list each tick.
        if type(VRShouldScanBodyCapsules)=='function' and not VRShouldScanBodyCapsules(pl) then return end
        local found = {}
        local ok = pcall(function()
            local cs = Game.VREntityComponents(pl)
            for i = 1, #cs do
                if string.find(tostring(cs[i]:GetName()), 'VRPortBody_') then
                    local b = cs[i]:CreatePhysicalBodyInterface(0)
                    if b then found[#found + 1] = b end
                end
            end
        end)
        if not ok or #found == 0 then capsMissAt = now return end
        capsCache, capsScanAt = found, now
    end
    if now - capsAt < 0.5 then return end
    capsAt = now
    for i = 1, #capsCache do
        if not pcall(function() capsCache[i]:SetIsQueryable(false) end) then
            capsCache = nil            -- stale handles after a reload: rescan next tick
            return
        end
    end
end

-- A plain function for pcall: `pcall(function() ... end)` would allocate a closure every frame.
local function ballHas(pl) return pl:VRBallHas() end

registerForEvent('onUpdate', function(dt)
    dt = dt or 0.016
    -- A stalled or absurd frame delta would fabricate a huge velocity and fire a bogus impulse.
    -- Skip the frame instead of guessing: one dropped correction is invisible, a wrong one is not.
    if dt <= 0.0 or dt > 0.2 then return end

    local pl = Game.GetPlayer()
    if not pl then return end

    -- Before the ball check: the capsules have to be hidden from the movement system whether a ball
    -- exists or not, or the player gets shoved around by their own arms for no reason at all.
    capsQueriesOff((os and os.clock and os.clock()) or 0.0, pl)

    local ok, has = pcall(ballHas, pl)
    if not ok or not has then return end

    -- Grip state from the shared-memory bridge, the same slots the smoking module reads:
    -- [49] right grip, [155] left grip. Without the bridge there is no VR input at all, so the
    -- ball stays a pure physics object and only the bounce correction runs.
    local gripR, gripL = false, false
    if type(GetVRSharedSlot) == 'function' then
        gripR = GetVRSharedSlot(49) > 0.5
        gripL = GetVRSharedSlot(155) > 0.5
    end

    pcall(function() pl:VRBallTick(dt, gripR, gripL) end)

    if vrDebug() then
        logAcc = logAcc + dt
        if logAcc >= 0.5 then
            logAcc = 0.0
            pcall(function()
                spdlog.info(string.format("[VRBall] dt=%.4f lastImpact=%.3f m/s", dt, pl:VRBallLastImpact()))
            end)
        end
    end
end)
