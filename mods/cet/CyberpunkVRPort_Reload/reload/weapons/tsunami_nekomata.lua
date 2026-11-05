-- Tsunami Nekomata — physical-reload configuration.
--
-- The tech sniper rifle: a box magazine (32 x 129 x 85 mm, 4 rounds) up into the receiver ahead of the grip, and a
-- bolt with a folding knob on the LEFT. Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/
-- nekomata_*; the analysis in build/rifles-20261020).
--
--   * `cocking_mechanism` (5) is the bolt, `l_cocking_mechanism` (16) its knob. In the EMPTY reload the knob unfolds
--     90 deg about its own Y, the bolt comes back 80 mm and stays there for the magazine change, then goes home and
--     the knob folds. In VR: back 80 mm and let go (`chamberOnRack = 'dry'`); the knob is a rider that only turns,
--     out by the first 5 mm. The rails' magnets (6-11, 18-71) move in every reload -- dressing, left to the game.
--   * THE HOLD IS MEASURED FROM THE WRIST (tools/wristhold.py): this bank keeps the WeaponLeft slot 287 mm off the
--     hand for the whole reload, so against the slot the magazine looks carried at arm's length. From the wrist it is
--     120 mm, and three of the four takes agree to 0.4 mm.
--   * Two-handed: the bolt on the LEFT TRIGGER, the magazine rocked out by hand (`handPull`).
return {
    id    = 'tsunami_nekomata',
    match = 'tsunami_nekomata',

    -- pwa_w_rifle_sniper__tsunami_nekomata.anims
    sounds = {
        slidePull    = 'w_gun_sniper_tech_nekomata_handle_pull',
        slideRelease = 'w_gun_sniper_tech_nekomata_handle_release',
        magOut       = 'w_gun_sniper_tech_nekomata_mag_out',
        magIn        = 'w_gun_sniper_tech_nekomata_mag_in',
        magGrab      = 'w_gun_sniper_tech_nekomata_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 72, names = { { 5, 'cocking_mechanism' }, { 12, 'mag_slot' }, { 14, 'back_bolt' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_mechanism',
        handleSide = 'both',  -- the handle sticks out on both sides (the slide bone's vertices, x -0.035..+0.035 m): reload.lua handGrips
        which      = 1, bone = 5,
        travel     = 0.0800,
        restLocal  = { 0.00000, 0.07360, 0.21840 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 80 mm in 0.06 s back home (t=4.16..4.22 of nekomata_empty_a)
        releaseSpeed = 1.50,
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
        -- THE LEFT HAND ON THE KNOB while it pulls the bolt, nekomata_empty_a t=1.18..1.38 (2.6 mm / 7.0 deg, the
        -- thumb 27-31 mm from it): left of the receiver and below.
        grips = {
            behind = { off = { -0.0666, -0.0506, -0.0632 },
                       rot = { -0.0498, -0.5394, 0.5624, 0.6248 },
                       pose = 'nekomata_handle_left' },
        },
        riders = {
            { bone = 16, ratio = 0.0, turn = 90.0, axis = { 0, 1, 0 }, turnAt = 0.005 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_sniper\\tsunami_nekomata\\entities\\meshes\\w_rifle_sniper__tsunami_nekomata__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_nekomata.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 31.6 x 128.5 x 85.0 mm, long along its Y (the bore)
        drop = {
            centre  = { 0.0, -0.0021, -0.0388 },
            flat    = 0.0158,
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            arm     = 0.0484,
            probe   = 0.0158,
            inertia = 0.00198,
        },

        -- THE HOLD, from the wrist in the bind WeaponLeft frame: nekomata_one_a / empty_a / empty_b (10-13 frames,
        -- 1.1-3.7 mm, 0.8-1.6 deg), the three the same to 0.4 mm.
        holdOffset   = { 0.06849, -0.00349, -0.03477 },
        holdRotBasis = { 0.08457, 0.73576, 0.25041, 0.62354 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_nekomata@mag.ini
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

        -- UP, a little from the left: the four takes' fits (worst residual 7.3-12.6 mm, the take rocks it in)
        wellAxis   = { -0.1035, 0.0159, -0.9945 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { 0.06849, -0.00349, -0.03477 }, rot = { 0.08457, 0.73576, 0.25041, 0.62354 },
                         pose = 'nekomata_mag_left_carry' },
            { d = 0.030, off = { 0.06849, -0.00349, -0.03477 }, rot = { 0.08457, 0.73576, 0.25041, 0.62354 },
                         pose = 'nekomata_mag_left_seat' },
        },
        pose           = 'nekomata_mag_left_seat',
    },
}
