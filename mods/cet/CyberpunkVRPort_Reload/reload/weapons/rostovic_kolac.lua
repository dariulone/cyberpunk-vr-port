-- Rostovic Kolac — physical-reload configuration.
--
-- A precision rifle with a box magazine (38 x 149 x 230 mm) behind the grip and a bolt worked by a handle on the LEFT.
-- Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/kolac_*; the analysis in
-- build/rifles-20261020).
--
--   * `cocking_handle` (6) is the handle: in the EMPTY reload it flips up 57 deg about its own Y, comes back 150 mm,
--     flies home and folds down again. `cocking` (14) rides it 1:1, the bolt `chamber` (11) 207 mm (1.38) and
--     `chamber_middle` (12) 161 mm (1.08). Nothing holds it back (`chamberOnRack = 'dry'`). The flip is a rider on the
--     handle itself, out by the first 5 mm of the pull.
--   * The magazine goes in straight up, to 108 mm on the carrying hand, then the hand moves under it and pushes it the
--     rest -- two holds, two `poseStages`.
--   * THE OLD MAGAZINE IS NOT THE NEW ONE: the longest rigid carry in the `_b` takes is the empty magazine being taken
--     away (2.3-2.9 s); the new one is carried at 4.4-5.0 s, and the `_a` takes agree on it to 0.1 mm.
return {
    id    = 'rostovic_kolac',
    match = 'rostovic_kolac',

    -- pwa_w_rifle_precision__rostovic_kolac.anims
    sounds = {
        slidePull    = 'w_gun_rifle_power_kolac_handle_pull',
        slideRelease = 'w_gun_rifle_power_kolac_handle_release',
        magOut       = 'w_gun_rifle_power_kolac_mag_out',
        magIn        = 'w_gun_rifle_power_kolac_mag_in',
        magGrab      = 'w_gun_rifle_power_kolac_mag_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 19, names = { { 6, 'cocking_handle' }, { 13, 'bullet' }, { 16, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_handle',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.065..-0.029 m): reload.lua handGrips
        which      = 1, bone = 6,
        travel     = 0.1500,
        restLocal  = { 0.03480, 0.06720, 0.03620 },
        -- ...and its rest TURN, which the absolute write must carry: the handle lies folded 59.6 deg round its Y
        -- (VRRigBone(1, 6, 3..6) at rest, = the rig's)
        restRot    = { 0.0, -0.4971, 0.0, 0.8677 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 150 mm in 0.17 s back home (t=6.36..6.53 of kolac_empty_a)
        releaseSpeed = 1.20,
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
        -- THE LEFT HAND ON THE HANDLE through the pull, kolac_empty_a t=6.00..6.30 (15.5 mm / 11.6 deg, the index
        -- 20-28 mm from it): behind and left of it, wrapped round from below.
        grips = {
            behind = { off = { -0.0437, -0.0491, -0.0354 },
                       rot = { -0.1961, 0.2922, -0.5517, -0.7561 },
                       pose = 'kolac_handle_left' },
        },
        riders = {
            { bone = 14, ratio = 1.0 },
            { bone = 11, ratio = 1.379 },
            { bone = 12, ratio = 1.076 },
            -- the handle's own bone: turn only (reload.lua writes it whole, the slide's translation and rest * turn)
            { bone = 6,  ratio = 1.0, turn = 57.0, axis = { 0, 1, 0 }, turnAt = 0.005 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_precision\\rostovic_kolac\\entities\\meshes\\w_rifle_precision__rostovic_kolac__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_kolac.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 37.9 x 149.3 x 229.6 mm, long along its Z
        drop = {
            centre  = { 0.0, 0.0007, -0.1021 },
            flat    = 0.0189,
            arm     = 0.0959,
            probe   = 0.0189,
            inertia = 0.00625,
        },

        -- THE CARRY, kolac_one_a / kolac_empty_a: rigid over 31-32 frames (2.0-4.3 mm, 0.8-1.5 deg), the two the same
        -- to 0.1 mm -- the magazine held well down its length, 210 mm from the wrist.
        holdOffset   = { 0.11420, 0.04710, -0.12187 },
        holdRotBasis = { 0.33557, 0.81022, 0.41493, 0.24246 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_kolac@mag.ini
        -- (build/maghold-20261026/mag_holds.py: this gun's fitted carry round the seated magazine) (a bullpup: the magazine is BEHIND the grip) -- hold 1,
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

        -- STRAIGHT UP: the four takes' fits (worst residual 0.1 mm)
        wellAxis   = { -0.0059, 0.0085, -0.9999 },
        insertRun  = 0.150,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        -- ...and the push: from 108 mm the hand is under the magazine (kolac_empty_a t=5.18..5.31)
        poseStages = {
            { d = 0.150, off = { 0.11420, 0.04710, -0.12187 }, rot = { 0.33557, 0.81022, 0.41493, 0.24246 },
                         pose = 'kolac_mag_left_carry' },
            { d = 0.100, off = { 0.14365, -0.05175, -0.15392 }, rot = { 0.16700, 0.88200, 0.09200, 0.43200 },
                         pose = 'kolac_mag_left_seat' },
        },
        pose           = 'kolac_mag_left_seat',
    },
}
