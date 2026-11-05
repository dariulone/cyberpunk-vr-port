-- Nokota Osprey — physical-reload configuration.
--
-- Phantom Liberty's bolt-action sniper rifle: a box magazine (39 x 142 x 151 mm, 6 rounds) up into the receiver
-- behind the grip, and a bolt on the LEFT. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/osprey_*; the analysis in build/rifles-20261020).
--
--   * `cocking_mechanism` (5) is the bolt and `handle` (12) its knob: in the EMPTY reload the knob lifts 91 deg about
--     its own Y, the bolt comes back 128 mm, goes home and the knob drops. Nothing holds it back
--     (`chamberOnRack = 'dry'`); the lift is a rider that only turns, up by the first 5 mm of the pull.
--   * `magazine_release` (7), the catch in front of the well, is pressed 35 deg about -X while the magazine is out: a
--     `hatch`.
--   * THE HOLD IS MEASURED FROM THE WRIST (tools/wristhold.py): this bank moves the WeaponLeft slot 165 mm off the
--     hand while the magazine is carried and puts it back at 116 mm from the well, so against the slot the take
--     looks like a carry and then a second hold -- against the wrist it is one hold all the way in.
--   * Two-handed: the bolt on the LEFT TRIGGER, the magazine rocked out by hand (`handPull`).
return {
    id    = 'nokota_osprey',
    match = 'nokota_osprey',

    -- pwa_w_rifle_sniper__nokota_osprey.anims
    sounds = {
        slidePull    = 'w_gun_osprey_handle_pull',
        slideRelease = 'w_gun_osprey_handle_release',
        magOut       = 'w_gun_osprey_mag_out',
        magIn        = 'w_gun_osprey_mag_in',
        magGrab      = 'w_gun_osprey_mag_rattle',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 13, names = { { 5, 'cocking_mechanism' }, { 7, 'magazine_release' }, { 10, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    hatch = {
        which = 1, bone = 7,
        openTime  = 0.05,
        closeTime = 0.05,
        parts = {
            { which = 1, bone = 7, turn = 35.1, axis = { -1.0, 0.0, 0.0 }, from = 0.0, to = 1.0 },
        },
    },

    slide = {
        slot       = 'vrp_cocking_mechanism',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.032..+0.018 m): reload.lua handGrips
        which      = 1, bone = 5,
        travel     = 0.1280,
        restLocal  = { 0.02470, 0.06870, -0.03670 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 128 mm in 0.07 s back home (t=5.60..5.67 of osprey_empty_a)
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
        -- THE LEFT HAND ON THE KNOB as it lifts it, osprey_empty_a t=5.22..5.40 (6.5 mm / 0.4 deg, thumb 45-50 mm and
        -- index 38-45 mm from the bolt): left of it, a little behind.
        grips = {
            behind = { off = { -0.0462, -0.0531, -0.0119 },
                       rot = { 0.1801, -0.1324, 0.6281, 0.7453 },
                       pose = 'osprey_handle_left' },
        },
        riders = {
            { bone = 12, ratio = 0.0, turn = 91.0, axis = { 0, 1, 0 }, turnAt = 0.005 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'ep1\\weapons\\firearms\\rifle_sniper\\nokota_osprey\\entities\\meshes\\w_rifle_sniper__nokota_osprey__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_osprey.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 39.1 x 142.3 x 150.6 mm, long along its Z
        drop = {
            centre  = { 0.0, -0.0040, -0.0688 },
            flat    = 0.0195,
            arm     = 0.0557,
            probe   = 0.0195,
            inertia = 0.00358,
        },

        -- THE HOLD, from the wrist in the bind WeaponLeft frame: osprey_one_b t=3.69..4.71 and osprey_empty_b
        -- t=3.53..3.84 (0.5-3.3 mm, 0.5-0.6 deg), the two the same to 0.9 mm.
        holdOffset   = { 0.06560, 0.09366, -0.04935 },
        holdRotBasis = { 0.03105, 0.75662, 0.58958, 0.28099 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_osprey@mag.ini
        -- (build/maghold-20261026/mag_holds.py: this gun's fitted carry round the seated magazine) (a bullpup: the magazine is behind the grip) -- hold 1,
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

        -- STRAIGHT UP: the four takes' fits (worst residual 0.9-3.0 mm)
        wellAxis   = { -0.0349, -0.0058, -0.9994 },
        insertRun  = 0.120,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.120, off = { 0.06560, 0.09366, -0.04935 }, rot = { 0.03105, 0.75662, 0.58958, 0.28099 },
                         pose = 'osprey_mag_left_carry' },
            { d = 0.050, off = { 0.06560, 0.09366, -0.04935 }, rot = { 0.03105, 0.75662, 0.58958, 0.28099 },
                         pose = 'osprey_mag_left_seat' },
        },
        pose           = 'osprey_mag_left_seat',
    },
}
