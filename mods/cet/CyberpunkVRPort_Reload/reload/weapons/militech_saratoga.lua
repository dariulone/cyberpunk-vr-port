-- Militech Saratoga — physical-reload configuration (2026-10-25).
--
-- A box magazine (36 x 116 x 209 mm, 40 rounds) in front of the trigger guard and a front charging handle.
-- Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/saratoga_*; gunsum.py, assets in
-- build/smg-20261025). The Guillotine shares the frame rig and the animations (budget_guillotine.lua).
return {
    id    = 'militech_saratoga',
    match = 'militech_saratoga',

    -- pwa_w_smg__militech_saratoga.anims
    sounds = {
        slidePull    = 'w_gun_smg_power_saratoga_handle_pull',
        slideRelease = 'w_gun_smg_power_saratoga_handle_release',
        magOut       = 'w_gun_smg_power_saratoga_mag_out',
        magIn        = 'w_gun_smg_power_saratoga_mag_in',
        magGrab      = 'w_gun_smg_power_saratoga_mag_rattle',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 16, names = { { 4, 'magazine_open' }, { 5, 'cocking' }, { 9, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.045..-0.004 m): reload.lua handGrips
        which      = 1, bone = 5,
        -- THE FRONT HANDLE, the MP5 way: every reload pulls it 53.4 mm back and turns it 20 deg up into its notch, holds
        -- it there through the magazine change and slaps it home (handle_pull / handle_release). Here it is a charging
        -- handle the hand racks and lets go: no notch, so no turn (a `turnAt` rider turns it all the way back, not at
        -- the end), and the rounds go in with the rack only after a dry magazine (`chamberOnRack = 'dry'`).
        travel     = 0.0534,
        restLocal  = { 0.00000, 0.05150, 0.27670 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        releaseSpeed = 1.20,
        gripSnap   = true,
        previewRadius = 0.06,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- THE ROUND OUT OF THE PORT once the handle is this far back with one in the chamber: our own effect
        -- on the gun's port slot, the case alone (build/smg-20261025/make_case_fx_smg.py), and its landing
        -- heard 0.45 s later (`caseSnd`, reload.lua)
        ejectAt    = 0.60,
        ejectFx    = 'vrp_eject_case',
        caseSnd    = 'w_casings_small_hit_floor',
        -- THE LEFT HAND ON IT as it starts back, saratoga_one_b t=1.02..1.12 (7.9 mm): left of the handle, below and
        -- behind it, the take's fingers (saratoga_handle_left).
        grips = {
            behind = { off = { -0.0598, -0.0667, -0.0607 },
                       rot = { 0.0162, 0.5213, -0.5545, -0.6484 },
                       pose = 'saratoga_handle_left' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: idle 1,0; the bank drops 0 and shows the reload magazine on 1 while it is out (the takes' `mt`).
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\smg\\militech_saratoga\\entities\\meshes\\w_smg__militech_saratoga__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_saratoga.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 35.6 x 116.0 x 209.4 mm, long along its Z
        drop = {
            centre  = { 0.0000, 0.0281, -0.0820 },
            flat    = 0.0178,
            arm     = 0.0869,
            probe   = 0.0178,
            inertia = 0.00478,
        },

        -- THE HOLD, three of the four takes: rigid over 12-33 frames, the three the same to 0.3 mm.
        -- CLOSED ON THE MESH (graspfit.py, 2026-10-25): that hold moved and the fingers bent until they touch the
        -- magazine and nothing is inside it; the numbers below and the poses are the fit's.
        holdOffset   = { -0.00346, 0.02687, 0.06214 },
        holdRotBasis = { -0.32140, -0.05125, 0.02844, 0.94513 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar and the rifles (2026-10-26, for the SMGs whose magazine
        -- sticks out): a second two-hand hold, CyberpunkVR_TwoHandGrip_saratoga@mag.ini
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

        -- UP AND BACK into the well: the four takes over the last 80 mm, averaged (residuals 2.6-11 mm).
        wellAxis   = { -0.0290, 0.3019, -0.9529 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { -0.00346, 0.02687, 0.06214 }, rot = { -0.32140, -0.05125, 0.02844, 0.94513 },
                         pose = 'saratoga_mag_left_carry' },
            { d = 0.030, off = { -0.00346, 0.02687, 0.06214 }, rot = { -0.32140, -0.05125, 0.02844, 0.94513 },
                         pose = 'saratoga_mag_left_seat' },
        },
        pose           = 'saratoga_mag_left_seat',
    },
}
