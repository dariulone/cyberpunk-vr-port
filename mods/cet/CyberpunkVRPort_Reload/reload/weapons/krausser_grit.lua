-- Krausser Grit — physical-reload configuration.
--
-- Phantom Liberty's, so its assets live under `ep1\`. Read off four takes of its own reload with VRIK off
-- (build/shotgun-reload-20261019/takes/grit_{one,empty}_{a,b}; boxsum.py, parts.py, timeline.py there), the analysis
-- in build/pistols-20261020.
--
--   * 14 frame bones like the Crusher's, told apart by name. `mechanism` (4) is the top piece the left hand pinches
--     and drags 53.5 mm back -- in the EMPTY reload only, after the magazine is home. `end_mechanism` (11) and the
--     side `chamber_pull` (12) are what an empty gun holds back (26.2 and 50.0 mm); the game sends them home itself
--     once the hand lets go, so neither is driven here.
--   * The magazine is a long stick (25.4 x 107.2 x 261.3 mm, 30 rounds) on the shared 5-bone rig, and THE GUN TAKES
--     IT THE LAST 41.5 mm ITSELF: the hand lets go with it hanging 40 mm short, and 0.04 s later, the hand 30-45 cm
--     away, it is home (`mag_in_beep`) -- `selfSeat`, the Chao's and the Zhuo's way.
return {
    id    = 'krausser_grit',
    match = 'krausser_grit',

    -- pwa_w_handgun__krausser_grit.anims
    sounds = {
        magOut       = 'w_gun_grit_mag_out',
        magIn        = 'w_gun_grit_mag_in',
        magGrab      = 'w_gun_grit_mag_new_grab',
        slidePull    = 'w_gun_grit_handle_pull',
        slideRelease = 'w_gun_grit_handle_release',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 14, names = { { 7, 'r_magazine_open' }, { 12, 'chamber_pull' }, { 13, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_mechanism',
        which      = 1, bone = 4,
        -- 53.5 and 53.7 mm in the two empty takes, along the bone's local -Z: straight back along the bore. No stop:
        -- it is home in every frame but the rack's.
        travel     = 0.0535,
        restLocal  = { 0.00000, 0.04370, 0.01730 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- home at 1.5-1.6 m/s at the fastest frame of either take
        releaseSpeed = 1.50,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        -- THE CASE out of the port once the handle is this far back with a round in the chamber: our own effect
        -- on the gun's port slot, the case alone (build/rifles-20261020/make_case_fx.py)
        ejectAt    = 0.55,
        ejectFx    = 'vrp_eject_case',
        -- THE LEFT HAND OVER THE TOP-LEFT, pinching the part from behind: the three frames the part travels WITH the
        -- hand (grit_empty_b t=2.80..2.85, 7.7 mm / 5.9 deg) -- after that the hand flies on back and the part stays
        -- at -33 mm until it is let go.
        grips = {
            behind = { off = { -0.0515, -0.0254, 0.0337 },
                       rot = { 0.4158, 0.3447, -0.2743, -0.7956 },
                       pose = 'grit_handle_left' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- Source rig track showMagazine; VRRigTrackWrite resolves the destination slot itself.
        showTrack  = 0,
        mesh       = 'ep1\\weapons\\firearms\\handgun\\krausser_grit\\entities\\meshes\\w_handgun__krausser_grit__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_grit.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',
        drop = {
            centre  = { 0.0, -0.0339, -0.1232 },
            flat    = 0.0127,
            arm     = 0.118,
            probe   = 0.0127,
            inertia = 0.0067,
        },

        -- THE HOLD, the fresh magazine in the WeaponLeft slot: both empty takes, rigid over t=0.91..1.91 and
        -- 1.09..1.87 (1.5 / 2.3 mm, 0.7 deg), the same turn to the last digit and 6 mm apart, averaged.
        holdOffset   = { 0.00671, 0.02416, 0.09377 },
        holdRotBasis = { -0.00161, -0.05259, -0.23795, 0.96985 },

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

        -- UP INTO THE GRIP, which leans back: grit_empty_b's last 60 mm, 16 points, worst residual 1.7 mm.
        wellAxis   = { -0.0029, -0.2567, -0.9665 },
        -- the hand's part ends 41.5 mm short of a 100 mm run, and the catch pulls it the rest in 0.04 s (t=2.20..2.24)
        insertRun  = 0.100,
        seatAt     = 0.585,
        selfSeat   = { time = 0.05 },

        -- the mechanism is worked in the empty reload only: a magazine swapped with rounds left fires at once, one
        -- swapped into a dry gun waits for the rack
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.100, off = { 0.00671, 0.02416, 0.09377 }, rot = { -0.00161, -0.05259, -0.23795, 0.96985 },
                         pose = 'grit_mag_left_carry' },
            { d = 0.030, off = { 0.00671, 0.02416, 0.09377 }, rot = { -0.00161, -0.05259, -0.23795, 0.96985 },
                         pose = 'grit_mag_left_seat' },
        },
        pose           = 'grit_mag_left_seat',
    },
}
