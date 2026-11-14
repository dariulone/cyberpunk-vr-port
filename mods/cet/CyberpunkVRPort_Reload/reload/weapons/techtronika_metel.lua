-- Techtronika Metel — physical-reload configuration.
--
-- Phantom Liberty's, assets under `ep1\`. A revolver by name and a PISTOL by mechanism: a box magazine (41.0 x 132.5
-- x 155.9 mm, eight rounds) up into the grip, and an upper that locks back on an empty magazine. Read off four takes
-- with VRIK off (build/shotgun-reload-20261019/takes/metel_*; the analysis in build/pistols-20261020).
--
--   * `slider` (11) is the upper. Empty, the gun holds it 70.0 mm back; the empty reload ends with the left thumb on
--     it, a 2 mm nudge further back and it slams home in 0.07 s (`handle_release`). So it is the pistols' slide stop
--     -- R3 or the hand releases it.
--   * Two hinges fold with it: `front_hinge` (13) rides it 1:1 and turns 54 deg, `back_hinge` (12) only turns, 86 deg,
--     both at the stop -- riders that turn (`turn`, read from home with `abs`).
--   * `chamber` (6) spins a half turn at the start of every reload, on its own; nothing to drive.
--   * The hand lets the magazine go 80-130 mm short and it flies home in 0.09 s -- `selfSeat`, shortened (below).
return {
    id    = 'techtronika_metel',
    match = 'techtronika_metel',

    -- pwa_w_revolver__techtronika_metel.anims
    sounds = {
        magOut       = 'w_gun_metel_mag_out',
        magIn        = 'w_gun_metel_mag_in',
        magGrab      = 'w_gun_metel_grab_mag',
        slidePull    = 'w_gun_metel_grab',
        slideRelease = 'w_gun_metel_handle_release',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 15, names = { { 6, 'chamber' }, { 11, 'slider' }, { 12, 'back_hinge' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_slider',
        which      = 1, bone = 11,
        -- home at Z +62.9, on the stop at -7.1 (both empty takes to 0.1 mm), and the release nudge 2 mm deeper
        travel     = 0.0720,
        restLocal  = { 0.00000, 0.05320, 0.06290 },
        lockLocal  = { 0.00000, 0.05320, -0.00710 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 69 mm in 0.067 s at the release (t=3.088..3.155 of metel_empty_a)
        releaseSpeed = 1.00,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        -- THE LEFT HAND UNDER AND LEFT OF THE UPPER, thumb on it: metel_empty_a t=3.00..3.09, the five frames it is on
        -- the catch (10.3 mm / 9.4 deg).
        grips = {
            behind = { off = { -0.0563, 0.0500, -0.0744 },
                       rot = { 0.5534, 0.6018, 0.2416, -0.5227 },
                       pose = 'metel_slide_left' },
        },
        riders = {
            { bone = 13, ratio = 1.0, abs = true, turn = 54.0, axis = { 0, 0, 1 }, turnAt = 0.070 },
            { bone = 12, ratio = 0.0, abs = true, turn = 86.0, axis = { 0, 0, 1 }, turnAt = 0.070 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0, -- showMagazine in the source rig, not its remapped pose-buffer slot
        mesh       = 'ep1\\weapons\\firearms\\revolver\\techtronika_metel\\entities\\meshes\\w_revolver__techtronika_metel__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_metel.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        drop = {
            centre  = { 0.0, -0.0400, -0.0723 },
            flat    = 0.0205,
            arm     = 0.0575,
            probe   = 0.0205,
            inertia = 0.0035,
        },

        -- THE HOLD, metel_one_b / metel_empty_b: rigid over 37 / 33 frames (0.7 / 2.6 mm, 0.7 deg), the same to 0.2 mm.
        -- (empty_a holds it another way.)
        holdOffset   = { -0.02083, 0.07485, 0.07653 },
        holdRotBasis = { -0.08175, -0.24354, -0.10971, 0.96019 },

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

        -- UP AND FORWARD INTO A RAKED GRIP, 35 deg off vertical (metel_empty_b)
        wellAxis   = { 0.0047, -0.5675, -0.8234 },
        -- The takes let go 80 (empty_a) and 130 mm (empty_b) short -- a flourish; a hand in VR is let go at 60 mm and
        -- the catch takes it the rest the way the take's does, in under a tenth of a second.
        insertRun  = 0.120,
        seatAt     = 0.50,
        selfSeat   = { time = 0.08 },

        poseStages = {
            { d = 0.120, off = { -0.02083, 0.07485, 0.07653 }, rot = { -0.08175, -0.24354, -0.10971, 0.96019 },
                         pose = 'metel_mag_left_carry' },
        },
        pose           = 'metel_mag_left_carry',
    },
}
