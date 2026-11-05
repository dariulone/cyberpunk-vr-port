-- Midnight Arms SOR-22 — physical-reload configuration.
--
-- A precision rifle whose box magazine (38 x 98 x 110 mm) goes in from the LEFT, 29 deg below horizontal, and whose
-- charging handle is on the RIGHT. Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/sor22_*;
-- the analysis in build/rifles-20261020).
--
--   * `cocking_handle` (5) comes back 84.4 mm in the EMPTY reload only, `front_pipe` (3) riding it 83.0 and the
--     `compressor` (6) 8.0; nothing holds it (`chamberOnRack = 'dry'`).
--   * THE TAKE RACKS IT WITH THE RIGHT HAND, the gun held by the left. In VR the right hand is the gun, so the left
--     reaches over the top onto the knob -- a hand of its own (see `grips`), the gun rolled handle-up or not
--     (the gun is tilted and the bolt pulled; nobody racks it from below).
--   * `magazine_open` (7), the cover over the well, swings 28 deg about -Z while the magazine is out: a `hatch`.
--   * Two-handed: the handle on the LEFT TRIGGER, the magazine rocked out by hand (`handPull`).
return {
    id    = 'midnight_sor22',
    match = 'midnight_sor22',

    -- pwa_w_rifle_precision__midnight_sor22.anims
    sounds = {
        slidePull    = 'w_gun_rifle_power_sor22_handle_pull',
        slideRelease = 'w_gun_rifle_power_sor22_handle_release',
        magOut       = 'w_gun_rifle_power_sor22_mag_out',
        magIn        = 'w_gun_rifle_power_sor22_mag_in',
        magGrab      = 'w_gun_rifle_power_sor22_mag_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 14, names = { { 5, 'cocking_handle' }, { 7, 'magazine_open' }, { 8, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    hatch = {
        which = 1, bone = 7,
        openTime  = 0.07,
        closeTime = 0.07,
        parts = {
            { which = 1, bone = 7, turn = 28.4, axis = { 0.0, 0.001, -1.0 }, from = 0.0, to = 1.0 },
        },
    },

    slide = {
        slot       = 'vrp_cocking_handle',
        handleSide = 'right',  -- the handle sticks out on the right only (the slide bone's vertices, x -0.014..+0.093 m): reload.lua handGrips
        which      = 1, bone = 5,
        travel     = 0.0844,
        restLocal  = { -0.01220, 0.04010, 0.23520 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 84 mm in 0.07 s back home (t=4.53..4.60 of sor22_empty_a)
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
        -- THE LEFT HAND OVER THE KNOB, from above (the bolt pose was in the wrong place). The handle is a stem out to the right
        -- ending in a cylinder 17 mm across, x 39..93 mm (base mesh, bone 5's vertices): 27..81 mm right of the bone,
        -- its axis along the gun's X. The Pozhar's two hands this had borrowed put the fist 33 and 53 mm off it --
        -- that gun's handle is a paddle beside the receiver -- and no take has a left hand on this one. So a hand of
        -- its own, built on the knob: fingers forward, index at the knob's end and the little finger at the stem
        -- (the fist's axis along the knob), the palm over it -- the hand reaching over the top, and with the gun
        -- rolled handle-up the same hand takes it like a lever, from the left. The fist's hollow (65 mm out, 30 mm
        -- palm-side of the wrist, as the posed hand measures on the Kolac) on the knob's middle.
        grips = {
            over = { off = { 0.0540, -0.0650, 0.0300 }, rot = { 0.0, 0.0, 0.7071, 0.7071 }, pose = 'copperhead_handle_left' },
        },
        riders = {
            { bone = 3, ratio = 0.983 },
            { bone = 6, ratio = 0.095 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_precision\\midnight_sor22\\entities\\meshes\\w_rifle_precision__midnight_sor22__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_sor22.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 38.0 x 97.7 x 109.5 mm, long along its Z
        drop = {
            centre  = { 0.0060, -0.0014, -0.0478 },
            flat    = 0.0190,
            arm     = 0.0357,
            probe   = 0.0190,
            inertia = 0.00179,
        },

        -- THE HOLD, sor22_one_b / sor22_empty_b: rigid over 30 / 19 frames (0.1-0.7 mm, 0.4-0.6 deg), the two the
        -- same to 0.1 mm, and the same hand all the way in.
        holdOffset   = { 0.08451, 0.00646, -0.03603 },
        holdRotBasis = { -0.01657, 0.70533, 0.14815, 0.69303 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_sor22@mag.ini
        -- (build/maghold-20261026/mag_holds.py: this gun's fitted carry round the seated magazine) (the magazine is on the left, 77 mm from the fore-end hold: the nearer one is offered) -- hold 1,
        -- taken away with no magazine in the well.
        holdAlt        = 1,

        snapRadius     = 0.050,
        -- A WIDER CATCH (the magnet did not catch properly): the well faces LEFT, so a magazine brought up from below
        -- the way every other rifle's goes sat outside the 75 mm line until it was all but in, and the 128 mm the
        -- run gives along the well (1.6 x 80) is less than this magazine is long
        seatSnapRadius = 0.095,
        magnetDepth    = 0.160,
        armDist        = 0.080,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 8.0,
        dropPush       = 1.1,

        -- IN FROM THE LEFT AND BELOW: the four takes' fits averaged (worst residual 3.4-6.2 mm)
        wellAxis   = { -0.8749, -0.0389, -0.4845 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { 0.08451, 0.00646, -0.03603 }, rot = { -0.01657, 0.70533, 0.14815, 0.69303 },
                         pose = 'sor22_mag_left_carry' },
        },
        pose           = 'sor22_mag_left_carry',
    },
}
