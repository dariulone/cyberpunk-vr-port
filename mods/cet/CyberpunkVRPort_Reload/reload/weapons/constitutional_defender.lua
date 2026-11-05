-- Constitutional Arms Defender — physical-reload configuration (2026-10-25, the first LMG).
--
-- A BOX DRUM on the left of the receiver (113 x 385 x 142 mm, 80 rounds) and a charging handle on the right
-- with a 158 mm throw. Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/defender_*;
-- drumhold.py there, assets in build/smg-20261025). The MA70 shares the rigs (midnight_ma70.lua).
--
--   * THE DRUM IS TWO COMPONENTS on one 23-bone rig: `Magazine` draws the belt, `MagazineReload` the box,
--     every bone under `magazine_base` (1). The rig has NO TRACKS, so the gun's drum is hidden by scale
--     (`hideScale`, the Ashura's way) -- both components follow the one bone. The held drum is
--     vrp_mag_defender.ent: the belt's visible copy plus the box (drumbox.py).
--   * `cocking_handle` (7) goes 158 mm back and home only on the empty reload, the left hand on it the whole
--     way; `cocking_middle` (8) rides it 85 mm (0.54). No stop: the rounds go in with the rack.
--   * `magazine_rotator` (11) and the two `grapping_mechanism`s latch the drum after it seats -- the game's
--     own flourish, left alone.
return {
    id    = 'constitutional_defender',
    match = 'constitutional_defender',

    -- pwa_w_lmg__constitutional_defender.anims
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
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.108..-0.055 m): reload.lua handGrips
        which      = 1, bone = 7,
        -- home at z +351.1, 158 mm back at the end of the empty takes' pull (both the same to 0.5 mm)
        travel     = 0.1580,
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
        -- THE LEFT HAND ON IT, ma70_empty_a t=4.75..5.00 (0.2 mm / 0.5 deg; defender_empty_b the same to 4 mm):
        -- reaching over the receiver to the handle on the right, the take's fingers (defender_handle_left).
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
        mesh       = 'base\\weapons\\firearms\\lmg\\constitutional_defender\\entities\\meshes\\w_lmg__constitutional_defender__mag_stdr_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_defender.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 113.3 x 384.9 x 141.9 (the box) mm, long along its Y
        drop = {
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            centre  = { -0.0689, 0.0087, -0.1100 },
            flat    = 0.0566,
            arm     = 0.1359,
            probe   = 0.0566,
            inertia = 0.01403,
        },

        -- THE HOLD, defender_empty_a t=1.62..2.67: the drum rigid in the hand for 48 frames (drumhold.py: the rig
        -- root moved by what `magazine_base` did), from the wrist into the bind WeaponLeft.
        -- CLOSED ON THE MESH (graspfit.py, 2026-10-25): that hold moved and the fingers bent until they touch the
        -- magazine and nothing is inside it; the numbers below and the poses are the fit's.
        holdOffset   = { 0.14060, -0.00073, -0.20901 },
        holdRotBasis = { 0.19516, 0.76327, 0.35170, 0.50560 },

        handPull       = true,
        pullTrigger    = true,

        snapRadius     = 0.050,
        -- A STRONGER MAGNET (2026-10-25): a drum this size is lined up by eye from
        -- further off -- 100 mm off the line instead of 75, 160 mm out instead of 128, gain 8.
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

        -- UP, FORWARD AND RIGHT into the receiver: all seven Defender / MA70 takes' last 80 mm, averaged
        -- (residuals 12-18 mm: the drum is rocked in).
        wellAxis   = { -0.2834, -0.4069, -0.8684 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { 0.14060, -0.00073, -0.20901 }, rot = { 0.19516, 0.76327, 0.35170, 0.50560 },
                         pose = 'defender_mag_left_carry' },
            { d = 0.030, off = { 0.14060, -0.00073, -0.20901 }, rot = { 0.19516, 0.76327, 0.35170, 0.50560 },
                         pose = 'defender_mag_left_seat' },
        },
        pose           = 'defender_mag_left_seat',
    },
}
