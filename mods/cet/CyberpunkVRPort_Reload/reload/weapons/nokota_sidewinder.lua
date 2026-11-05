-- Nokota Sidewinder — physical-reload configuration.
--
-- Built on the Copperhead (its receiver and stock are the Copperhead's parts) with a frame rig of its own: 13 bones,
-- the same `reload_lever` (2) and `counter_mass` (3), plus a magazine catch. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/sidewinder_*); the hand on the magazine is the Copperhead's to 0.5 mm.
--
--   * `reload_lever` comes back 75.0 mm, only in the EMPTY reload, nothing holds it (`chamberOnRack = 'dry'`);
--     `counter_mass` goes the other way, 44.5 mm FORWARD (-0.594).
--   * `magazine_opener` (6), the catch in front of the well, flips 49 deg as the magazine leaves and sits at 33 deg
--     until the next one is in: a `hatch`, open while the magazine is out.
return {
    id    = 'nokota_sidewinder',
    match = 'nokota_sidewinder',

    -- pwa_w_rifle_assault__nokota_sidewinder.anims (the bank is filed under smg_smart)
    sounds = {
        slidePull    = 'w_gun_smg_smart_sidewinder_handle_pull',
        slideRelease = 'w_gun_smg_smart_sidewinder_handle_release',
        magOut       = 'w_gun_smg_smart_sidewinder_mag_out',
        magIn        = 'w_gun_smg_smart_sidewinder_mag_in',
        magGrab      = 'w_gun_smg_smart_sidewinder_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 13, names = { { 2, 'reload_lever' }, { 6, 'magazine_opener' }, { 9, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    -- THE CATCH, parent-local as parts.py reads it (sidewinder_empty_a): 32.8 deg about its own Y while the magazine is
    -- out (the 49 deg flick as it leaves is the take's thumb on it, not a state)
    hatch = {
        which = 1, bone = 6,
        openTime  = 0.05,
        closeTime = 0.05,
        parts = {
            { which = 1, bone = 6, turn = 32.8, axis = { 0.001, 1.0, 0.001 }, from = 0.0, to = 1.0 },
        },
    },

    slide = {
        slot       = 'vrp_reload_lever',
        handleSide = 'both',  -- the handle sticks out on both sides (the slide bone's vertices, x -0.046..+0.046 m): reload.lua handGrips
        which      = 1, bone = 2,
        travel     = 0.0750,
        restLocal  = { 0.00000, 0.04780, 0.34070 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 75 mm in 0.1 s back home (t=3.18..3.29 of sidewinder_empty_b)
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
        -- THE LEFT HAND ON THE LEVER while it pulls and holds it, sidewinder_empty_b t=3.04..3.14 (3.4 mm / 2.2 deg,
        -- thumb 46-49 mm and index 42-46 mm from the lever): the Copperhead's hand to 20 mm.
        grips = {
            behind = { off = { -0.0717, -0.1170, -0.0375 },
                       rot = { -0.0779, 0.2600, -0.6691, -0.6918 },
                       pose = 'sidewinder_handle_left' },
        },
        riders = {
            { bone = 3, ratio = -0.594 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_assault\\nokota_sidewinder\\entities\\meshes\\w_rifle_assault__nokota_sidewinder__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_sidewinder.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 30.9 x 179.2 x 205.5 mm, long along its Z
        drop = {
            centre  = { 0.0, -0.0547, -0.0981 },
            flat    = 0.0154,
            arm     = 0.0873,
            probe   = 0.0154,
            inertia = 0.00620,
        },

        -- THE HOLD, all four takes: rigid over 14-45 frames (0.4-3.9 mm, 0.5-0.9 deg), within 0.6 mm of each other.
        -- ...LAID STRAIGHT IN THE HAND, AT THE TAKE'S PLACE. The take's magazine is twisted 5.5 deg about its own length
        -- off the hand (slightly crooked); the Crusher's hand borrowed for it tilted it 26 deg off the fist (far from
        -- square). So the take's position and the take's finger poses, with the turn squared up: the flat face on the
        -- palm, the length along the fist, the depth along the fingers -- holdRotBasis = bind(WeaponLeft)^-1 * those
        -- axes. (Take: holdRotBasis { 0.00259, 0.01489, 0.05152, 0.99856 }.)
        -- ...AND CLOSED ON THE MESH (2026-10-25, the magazine sat wrong in the hand): with the take's hand the fingers hooked 20-30 mm IN FRONT of the front edge (the knuckles already at it) and the thumb ran through the box -- the game's own animation does this; on a flat screen nobody sees it. tools/graspfit.py
        -- (build/shotgun-reload-20261019) moved the magazine (+8, +27, +14) mm in its own frame, turned it 0.0 deg, and bent the
        -- finger joints a few degrees (sidewinder_mag_left_*), until the palm lies on the near face, the fingertips on the far
        -- one, the thumb on the box and nothing is inside the mesh. Before: holdOffset { -0.00712, 0.03974, 0.10571 }.
        holdOffset   = { -0.00074, 0.06761, 0.11853 },
        holdRotBasis = { -0.02080, -0.01490, 0.02230, 0.99942 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_sidewinder@mag.ini
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

        -- UP AND FORWARD: the three takes that agree (one_a, one_b, empty_a; worst residual 4.2-7.8 mm)
        wellAxis   = { 0.0206, -0.4328, -0.9012 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { -0.00074, 0.06761, 0.11853 }, rot = { -0.02080, -0.01490, 0.02230, 0.99942 },
                         pose = 'sidewinder_mag_left_carry' },
            { d = 0.030, off = { -0.00074, 0.06761, 0.11853 }, rot = { -0.02080, -0.01490, 0.02230, 0.99942 },
                         pose = 'sidewinder_mag_left_seat' },
        },
        pose           = 'sidewinder_mag_left_seat',
    },
}
