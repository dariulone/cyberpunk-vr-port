-- Arasaka Shingen — physical-reload configuration (2026-10-25, the first SMG).
--
-- A SMART SMG WITH ITS MAGAZINE ON TOP: a 58 x 220 x 67 mm block lying along the receiver at the back, seated 116 mm
-- behind the weapon origin and 49 mm up, put in FROM ABOVE. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/shingen_*; gunsum.py there, assets in build/smg-20261025).
--
--   * `cocking_device` (6) is the handle, on the LEFT of the receiver 185 mm forward. Empty, the gun holds it 75 mm back
--     (both empty takes start at z 110); the empty reload lets it fly home (0.08-0.15 m/s ramp, then one frame) with the
--     left hand on the fore-end -- a slide stop, released by the hand (or R3), like the Masamune's.
--   * `magazine_open` (7) is the catch: 4 mm in for the moment the magazine passes it, nothing to hold open.
--   * The game carries the gun's OWN magazine bone out and back (its tracks never change), so the hand's magazine is
--     ours and track 0 hides the gun's while it is out, as on the rifles.
return {
    id    = 'arasaka_shingen',
    match = 'arasaka_shingen',

    -- pwa_w_smg__arasaka_shingen.anims
    sounds = {
        slidePull    = 'w_gun_smg_smart_shingen_grab',
        slideRelease = 'w_gun_smg_smart_shingen_load',
        magOut       = 'w_gun_smg_smart_shingen_mag_out',
        magIn        = 'w_gun_smg_smart_shingen_mag_in',
        magGrab      = 'w_gun_smg_smart_shingen_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 15, names = { { 6, 'cocking_device' }, { 7, 'magazine_open' }, { 8, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_device',
        handleSide = 'both',  -- the handle sticks out on both sides (the slide bone's vertices, x -0.034..+0.034 m): reload.lua handGrips
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
        -- THE LEFT HAND ON A LEFT-SIDE HANDLE: the takes never touch it (the hand is on the fore-end when it flies
        -- home), so the Masamune's hand -- under and left of its handle, reaching over -- on this one.
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
        mesh       = 'base\\weapons\\firearms\\smg\\arasaka_shingen\\entities\\meshes\\w_smg__arasaka_shingen__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_shingen.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 58.5 x 220.4 x 67.4 mm, long along its Y (the Chao's way)
        drop = {
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            centre  = { 0.0, -0.0904, -0.0015 },
            flat    = 0.0292,
            arm     = 0.0810,
            probe   = 0.0292,
            inertia = 0.00443,
        },

        -- THE HOLD, shingen_empty_a / shingen_one_b: rigid over 34 / 26 frames (1.7 / 3.4 mm), the two the same to
        -- 0.2 mm; from the wrist into the bind WeaponLeft (wristhold.py) it is the same (the slot does not move).
        -- CLOSED ON THE MESH (graspfit.py, 2026-10-25): the take's hold moved and the fingers bent until they
        -- touch the magazine and nothing is inside it; the numbers below and the poses are the fit's.
        holdOffset   = { -0.00032, -0.00271, -0.02563 },
        holdRotBasis = { -0.76336, -0.34450, 0.13821, 0.52866 },

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

        -- DOWN FROM ABOVE: the four vertical approaches of the Shingen and the Warden (same frame, same seat) over the
        -- last 80 mm, averaged; two takes came in 40 deg from the right and are left out.
        -- ...OR SLID FORWARD ALONG THE BORE from behind, the magazine lying the way it sits (`wellAxes`; along the bore
        -- as well as from above, 2026-10-25): whichever line
        -- the magazine is nearer catches it.
        wellAxis   = { -0.0810, 0.1110, 0.9905 },
        wellAxes   = { { 0.0, -1.0, 0.0 } },
        insertRun  = 0.080,
        seatAt     = 0.70,

        poseStages = {
            { d = 0.080, off = { -0.00032, -0.00271, -0.02563 }, rot = { -0.76336, -0.34450, 0.13821, 0.52866 },
                         pose = 'shingen_mag_left_carry' },
            { d = 0.030, off = { -0.00032, -0.00271, -0.02563 }, rot = { -0.76336, -0.34450, 0.13821, 0.52866 },
                         pose = 'shingen_mag_left_seat' },
        },
        pose           = 'shingen_mag_left_seat',
    },
}
