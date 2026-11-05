-- Krausser Warden (Phantom Liberty) — physical-reload configuration (2026-10-25).
--
-- The Shingen's first eleven frame bones and its magazine rig: the magazine on top at the back (55 x 204 x 55 mm, 40
-- rounds), seated where the Shingen's is, put in from above. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/warden_*).
--
--   * `cocking_device` (6) on the LEFT, 75 mm back on the stop when empty, flying home at the end of the empty reload
--     (3.4 m/s, one frame) with the hand elsewhere -- a slide stop, released by the hand or R3.
--   * Unlike the Shingen the bank throws the spent magazine 0.6 m away and brings the fresh one on the RELOAD chain
--     (`magazine_reload` 2 -> `mag_stdr` 4; tracks 1,0 -> 1,1 -> 0,1), so the hold is the hand on that chain
--     (reloadchain.py) -- track 0 is still the gun's own magazine.
return {
    id    = 'krausser_warden',
    match = 'krausser_warden',

    -- pwa_w_smg__krausser_warden.anims
    sounds = {
        slidePull    = 'w_gun_warden_grab_handle',
        slideRelease = 'w_gun_warden_bolt_release',
        magOut       = 'w_gun_warden_mag_out',
        magIn        = 'w_gun_warden_mag_in',
        magGrab      = 'w_gun_warden_mag_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 11, names = { { 6, 'cocking_device' }, { 7, 'magazine_open' }, { 8, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_device',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.040..+0.017 m): reload.lua handGrips
        which      = 1, bone = 6,
        -- home at z +185.0, on the stop at +110.0 (both empty takes); 5 mm past the stop for the hand's release
        -- nudge (the Masamune's), so a pull from the stop has somewhere to go
        travel     = 0.0800,
        restLocal  = { 0.02830, 0.04980, 0.18500 },
        lockLocal  = { 0.02830, 0.04980, 0.11000 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        releaseSpeed = 1.00,
        gripSnap   = true,
        previewRadius = 0.06,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- no take touches it: the Shingen's borrowed hand (the Masamune's), the same handle at the same place
        grips = {
            behind = { off = { -0.0537, -0.0683, -0.0423 },
                       rot = { 0.2398, 0.6231, -0.2922, -0.6847 },
                       pose = 'copperhead_handle_left' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,
        mesh       = 'ep1\\weapons\\firearms\\smg\\krausser_warden\\entities\\meshes\\w_smg__krausser_warden__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_warden.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 55.3 x 203.9 x 55.2 mm, long along its Y
        drop = {
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            centre  = { 0.0, -0.0823, 0.0033 },
            flat    = 0.0276,
            arm     = 0.0743,
            probe   = 0.0276,
            inertia = 0.00372,
        },

        -- THE HOLD, the fresh magazine on the reload chain, warden_one_b t=2.24..2.80: rigid over 26 frames (0.6 deg),
        -- from the wrist into the bind WeaponLeft
        -- CLOSED ON THE MESH (graspfit.py, 2026-10-25): the take's hold moved and the fingers bent until they
        -- touch the magazine and nothing is inside it; the numbers below and the poses are the fit's.
        holdOffset   = { 0.01604, -0.00313, -0.03112 },
        holdRotBasis = { -0.61197, -0.46473, 0.55199, 0.32376 },

        handPull       = true,
        pullTrigger    = true,

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

        -- DOWN FROM ABOVE, the Shingen's well (same frame, same seat) -- or slid forward along the bore from behind
        -- (`wellAxes`, the Shingen's too, 2026-10-25)
        wellAxis   = { -0.0810, 0.1110, 0.9905 },
        wellAxes   = { { 0.0, -1.0, 0.0 } },
        insertRun  = 0.080,
        seatAt     = 0.70,

        poseStages = {
            { d = 0.080, off = { 0.01604, -0.00313, -0.03112 }, rot = { -0.61197, -0.46473, 0.55199, 0.32376 },
                         pose = 'warden_mag_left_carry' },
            { d = 0.030, off = { 0.01604, -0.00313, -0.03112 }, rot = { -0.61197, -0.46473, 0.55199, 0.32376 },
                         pose = 'warden_mag_left_seat' },
        },
        pose           = 'warden_mag_left_seat',
    },
}
