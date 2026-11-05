-- Nokota Copperhead — physical-reload configuration. The Darra Umbra is on the same frame rig and the same
-- animations (darra_umbra.lua).
--
-- A box magazine (25 x 176 x 200 mm, 30 rounds) on the shared assault-rifle magazine rig, rocked in front first
-- (the take turns it 69 deg -> 8 deg over the last 120 mm, then pushes the last 35 mm straight). Read off four takes
-- with VRIK off (build/shotgun-reload-20261019/takes/copperhead_*; the analysis in build/rifles-20261020).
--
--   * `reload_lever` (2) is the charging handle, on top at the front (z +340.7). Only the EMPTY reload works it: the
--     hand snaps it 93.8 mm back, holds it there 0.1 s and lets it fly home. No catch holds it -- the Crusher's case,
--     so the rounds go in with the rack (`chamberOnRack = 'dry'`).
--   * `counter_mass` (3) rides it the other way, 51.4 mm FORWARD against its 93.8 back (-0.548).
--   * Two-handed: the lever on the LEFT TRIGGER, the magazine rocked out by hand (`handPull`).
return {
    id    = 'nokota_copperhead',
    match = 'nokota_copperhead',

    -- pwa_w_rifle_assault__nokota_copperhead.anims
    sounds = {
        slidePull    = 'w_gun_rifle_power_copperhead_handle_pull',
        slideRelease = 'w_gun_rifle_power_copperhead_handle_release',
        magOut       = 'w_gun_rifle_power_copperhead_mag_out',
        magIn        = 'w_gun_rifle_power_copperhead_mag_in',
        magGrab      = 'w_gun_rifle_power_copperhead_mag_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 12, names = { { 2, 'reload_lever' }, { 3, 'counter_mass' }, { 7, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_reload_lever',
        handleSide = 'both',  -- the handle sticks out on both sides (the slide bone's vertices, x -0.041..+0.041 m): reload.lua handGrips
        which      = 1, bone = 2,
        travel     = 0.0938,
        restLocal  = { 0.00000, 0.04780, 0.34070 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 92 mm in 0.11 s back home (t=3.20..3.31 of copperhead_empty_a)
        releaseSpeed = 1.00,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- THE CASE out of the port once the handle is this far back with a round in the chamber: our own effect
        -- on the gun's port slot, the case alone (build/rifles-20261020/make_case_fx.py)
        ejectAt    = 0.60,
        ejectFx    = 'vrp_eject_case',
        -- THE LEFT HAND ON THE LEVER while it pulls and holds it, copperhead_empty_b t=3.02..3.18 (7.1 mm / 9.7 deg,
        -- thumb 33-41 mm and index 43-47 mm from the lever): left of the barrel and below it, reaching up.
        grips = {
            behind = { off = { -0.0754, -0.0996, -0.0369 },
                       rot = { -0.0292, 0.2760, -0.6560, -0.7018 },
                       pose = 'copperhead_handle_left' },
        },
        riders = {
            { bone = 3, ratio = -0.548 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_assault\\nokota_copperhead\\entities\\meshes\\w_rifle_assault__nokota_copperhead__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_copperhead.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 25.4 x 175.5 x 200.4 mm, long along its Z
        drop = {
            centre  = { 0.0, -0.0528, -0.0955 },
            flat    = 0.0127,
            arm     = 0.0875,
            probe   = 0.0127,
            inertia = 0.00591,
        },

        -- THE HOLD, all four takes: rigid over 15-36 frames (0.8-2.8 mm, 0.4-1.1 deg), within 0.6 mm of each other.
        -- ...LAID STRAIGHT IN THE HAND, AT THE TAKE'S PLACE. The take's magazine is twisted 5.5 deg about its own length
        -- off the hand (slightly crooked); the Crusher's hand borrowed for it tilted it 26 deg off the fist (far from
        -- square). So the take's position and the take's finger poses, with the turn squared up: the flat face on the
        -- palm, the length along the fist, the depth along the fingers -- holdRotBasis = bind(WeaponLeft)^-1 * those
        -- axes. (Take: holdRotBasis { 0.00258, 0.01499, 0.05161, 0.99855 }.)
        -- ...AND CLOSED ON THE MESH (2026-10-25, the magazine sat wrong in the hand): with the take's hand the fingers hooked 20-30 mm IN FRONT of the front edge (the knuckles already at it) and the thumb ran through the box -- the game's own animation does this; on a flat screen nobody sees it. tools/graspfit.py
        -- (build/shotgun-reload-20261019) moved the magazine (+6, +26, +11) mm in its own frame, turned it 0.4 deg, and bent the
        -- finger joints a few degrees (copperhead_mag_left_*), until the palm lies on the near face, the fingertips on the far
        -- one, the thumb on the box and nothing is inside the mesh. Before: holdOffset { -0.00719, 0.03988, 0.10561 }.
        holdOffset   = { -0.00270, 0.06705, 0.11565 },
        holdRotBasis = { -0.02075, -0.01497, 0.01903, 0.99949 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_copperhead@mag.ini
        -- (build/maghold-20261026/mag_holds.py: this gun's fitted carry round the seated magazine) -- hold 1,
        -- taken away with no magazine in the well.
        holdAlt        = 1,

        snapRadius     = 0.050,
        seatSnapRadius = 0.075,
        armDist        = 0.070,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 6.0,
        dropPush       = 1.1,

        -- UP AND FORWARD: the three takes that agree (one_a, one_b, empty_a; worst residual 4.3-8.3 mm). The rock-in
        -- before that is the take's own flourish; the last 35 mm are straight along this.
        wellAxis   = { 0.0173, -0.4352, -0.9002 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { -0.00270, 0.06705, 0.11565 }, rot = { -0.02075, -0.01497, 0.01903, 0.99949 },
                         pose = 'copperhead_mag_left_carry' },
            { d = 0.030, off = { -0.00270, 0.06705, 0.11565 }, rot = { -0.02075, -0.01497, 0.01903, 0.99949 },
                         pose = 'copperhead_mag_left_seat' },
        },
        pose           = 'copperhead_mag_left_seat',
    },
}
