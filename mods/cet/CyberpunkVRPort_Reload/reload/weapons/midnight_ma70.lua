-- Midnight Arms MA70 HB — physical-reload configuration (2026-10-25).
--
-- The Defender's frame and drum rigs and its animations (constitutional_defender.lua) with a box of its own:
-- 113 x 137 x 147 mm, 60 rounds. Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/
-- ma70_*). The belt is the Defender's; the box is mag_stdr_01 on `MagazineReload` (drumbox.py), and the
-- whole drum is hidden by scale on `magazine_base`. Its handle throws 158.9 mm.
return {
    id    = 'midnight_ma70',
    match = 'midnight_ma70',

    -- the Defender's bank: pwa_w_lmg__constitutional_defender.anims
    sounds = {
        slidePull    = 'w_gun_lmg_power_defender_handle_pull',
        slideRelease = 'w_gun_lmg_power_defender_handle_release',
        magOut       = 'w_gun_lmg_power_defender_mag_out',
        magIn        = 'w_gun_lmg_power_defender_mag_in',
        magGrab      = 'w_gun_lmg_power_defender_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 23, names = { { 1, 'magazine_base' }, { 2, 'magazine_opener' }, { 3, 'bullet_match_01' } } },
        { which = 1, bones = 22, names = { { 7, 'cocking_handle' }, { 11, 'magazine_rotator' }, { 13, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_handle',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.134..+0.007 m): reload.lua handGrips
        which      = 1, bone = 7,
        -- home at z +351.1, 158.9 mm back at the end of the empty takes' pull
        travel     = 0.1589,
        restLocal  = { 0.09300, -0.02830, 0.35110 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        releaseSpeed = 1.20,
        gripSnap   = true,
        previewRadius = 0.07,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- THE ROUND OUT OF THE PORT once the handle is this far back with one in the chamber: our own effect
        -- on the gun's port slot, the case alone (build/smg-20261025/make_case_fx_smg.py), and its landing
        -- heard 0.45 s later (`caseSnd`, reload.lua)
        ejectAt    = 0.60,
        ejectFx    = 'vrp_eject_case',
        caseSnd    = 'w_casings_large_hit_floor',
        -- THE LEFT HAND ON IT, ma70_empty_a t=4.75..5.00 (0.2 mm / 0.5 deg): reaching over the receiver to the
        -- handle on the right, the take's fingers (defender_handle_left).
        grips = {
            behind = { off = { -0.0374, -0.0565, -0.0193 },
                       rot = { 0.7588, 0.6238, -0.0452, -0.1816 },
                       pose = 'defender_handle_left' },
        },
        riders = {
            { bone = 8, ratio = 0.5395 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- NO TRACKS on the drum rig: hidden by scale on `magazine_base`, which carries the belt and the box.
        hideScale  = { 1 },
        mesh       = 'base\\weapons\\firearms\\lmg\\midnight_ma70hb\\entities\\meshes\\w_lmg__midnight_ma70hb__mag_stdr_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_ma70.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 112.8 x 136.8 x 147.4 (the box) mm, long along its Z
        drop = {
            centre  = { -0.0876, 0.1108, -0.1349 },
            flat    = 0.0564,
            arm     = 0.0173,
            probe   = 0.0564,
            inertia = 0.00337,
        },

        -- THE HOLD, ma70_empty_b t=2.04..3.02: the drum rigid in the hand for 45 frames (drumhold.py), from the
        -- wrist into the bind WeaponLeft -- the Defender's, the same animation.
        -- CLOSED ON THE MESH (graspfit.py, 2026-10-25): that hold moved and the fingers bent until they touch the
        -- magazine and nothing is inside it; the numbers below and the poses are the fit's.
        holdOffset   = { 0.14405, 0.01442, -0.20697 },
        holdRotBasis = { 0.21791, 0.81381, 0.22312, 0.49035 },

        handPull       = true,
        pullTrigger    = true,

        snapRadius     = 0.050,
        -- A STRONGER MAGNET (2026-10-25): 100 mm off the line instead of 75, 160 mm
        -- out instead of 128, gain 8.
        seatSnapRadius = 0.100,
        magnetDepth    = 0.160,
        armDist        = 0.070,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 8.0,
        dropPush       = 1.1,

        -- UP, FORWARD AND RIGHT into the receiver: all seven Defender / MA70 takes' last 80 mm, averaged.
        wellAxis   = { -0.2834, -0.4069, -0.8684 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { 0.14405, 0.01442, -0.20697 }, rot = { 0.21791, 0.81381, 0.22312, 0.49035 },
                         pose = 'ma70_mag_left_carry' },
            { d = 0.030, off = { 0.14405, 0.01442, -0.20697 }, rot = { 0.21791, 0.81381, 0.22312, 0.49035 },
                         pose = 'ma70_mag_left_seat' },
        },
        pose           = 'ma70_mag_left_seat',
    },
}
