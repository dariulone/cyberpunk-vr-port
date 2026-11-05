-- Tsunami Kyubi — physical-reload configuration.
--
-- The Ajax's frame rig (18 bones, the same names) and the Ajax's magazine rig: the same folding handle, 85 mm (the
-- Kyubi's takes overshoot to 91 and settle), the same rock-in and the same hand -- kyubi_one_a / one_b hold the
-- magazine within 0.2 mm of the Ajax's. Its own magazine (24 x 206 x 200 mm) and entity; the Ajax's sound bank.
--
--   * BUT IT IS RACKED FROM BEHIND (the bolt is pulled from behind, by its own puller): `reload_mechanism` (9) is a
--     T on top of the receiver over the grip (base mesh: y 19..49 mm, z 34..48, its left wing out to x = -42 mm),
--     and the game's own reload only drags it along, 0.565 of the front handle. So the slide here is the T, 48 mm,
--     and the front handle rides it; nobody unfolds the front handle, so its hinge stays shut.
return {
    id    = 'tsunami_kyubi',
    match = 'tsunami_kyubi',

    -- the Ajax's bank (pwa_w_rifle_assault__militech_ajax.anims): no set of its own was found
    sounds = {
        slidePull    = 'w_gun_rifle_power_ajax_handle_pull',
        slideRelease = 'w_gun_rifle_power_ajax_handle_release',
        magOut       = 'w_gun_rifle_power_ajax_mag_out',
        magIn        = 'w_gun_rifle_power_ajax_mag_in',
        magGrab      = 'w_gun_rifle_power_ajax_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 18, names = { { 11, 'slider' }, { 15, 'magazine_lock' }, { 14, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        -- the rear T (was the Ajax's front handle: vrp_reload_front, bone 8, 85 mm from { 0, 0.0354, 0.1904 })
        slot       = 'vrp_reload_mechanism',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.043..+0.028 m): reload.lua handGrips
        which      = 1, bone = 9,
        travel     = 0.0480,
        restLocal  = { 0.00000, 0.03620, 0.04710 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- the front handle's 85 mm in 0.045 s back home (t=3.69..3.76 of ajax_empty_a), at the T's 0.565
        releaseSpeed = 0.85,
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
        -- THE M4 WAY: the palm over the receiver behind the T, fingers forward, index and middle hooked round the T's
        -- front either side of its stem (a pinch with the index and middle fingers, as on a real M4 -- the
        -- Grit's fist borrowed before read as a fist). No take has a hand on the T, so the hand is built: palm down
        -- (rotation about Z, 90 deg), `kyubi_handle_left`, and the wrist placed so the two hooks' tips stand just in
        -- front of the T at its own height (handleview.py probe=vrp_reload_mechanism). (Was the Ajax's on the
        -- unfolded front handle: off { -0.0735, -0.0426, -0.0385 }, rot { -0.3989, 0.2802, -0.5178, -0.7030 }.)
        grips = {
            hook = { off = { -0.0060, -0.1085, 0.0285 },
                     rot = { 0.0, 0.0, 0.7071, 0.7071 },
                     pose = 'kyubi_handle_left' },
        },
        riders = {
            -- the front handle, 1 / 0.565 of the T (the game's own reload moves the T 0.565 of it)
            { bone = 8,  ratio = 1.770 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_assault\\tsunami_kyubi\\entities\\meshes\\w_rifle_assault__tsunami_kyubi__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_kyubi.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 23.8 x 206.1 x 200.3 mm -- a curve, as long along Y as along Z
        drop = {
            centre  = { 0.0, 0.0695, -0.0872 },
            flat    = 0.0119,
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            arm     = 0.0912,
            probe   = 0.0119,
            inertia = 0.00688,
        },

        -- THE HOLD, ajax_one_a / one_b / empty_b: rigid over 19-20 frames (0.1 mm, 0.4-0.7 deg), the three the same to
        -- 0.3 mm. (empty_a is the other of the bank's two holds, 50 mm further up the magazine.)
        holdOffset   = { -0.00476, 0.03099, 0.02503 },
        holdRotBasis = { -0.47855, -0.06154, 0.03470, 0.87521 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_kyubi@mag.ini
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

        -- IN FROM BELOW AND IN FRONT, the curve's own line: the eight Ajax and Kyubi fits put Y 0.35..0.66, Z
        -- -0.75..-0.94 and X either side of zero (the rock-in, 11-16 mm residuals); this is their middle.
        wellAxis   = { 0.0, 0.5197, -0.8543 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { -0.00476, 0.03099, 0.02503 }, rot = { -0.47855, -0.06154, 0.03470, 0.87521 },
                         pose = 'ajax_mag_left_carry' },
            { d = 0.030, off = { -0.00476, 0.03099, 0.02503 }, rot = { -0.47855, -0.06154, 0.03470, 0.87521 },
                         pose = 'ajax_mag_left_seat' },
        },
        pose           = 'ajax_mag_left_seat',
    },
}
