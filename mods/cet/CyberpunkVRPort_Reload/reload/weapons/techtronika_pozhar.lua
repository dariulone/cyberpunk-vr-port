-- Techtronika Pozhar — physical-reload configuration.
--
-- A SHOTGUN WITH A BOX MAGAZINE, filed by the game under rifle_precision (w_rifle_precision__techtronika_pozhar), and
-- worked like the Militech Crusher: reload.lua's, read off four takes of its own reload with VRIK off
-- (build/shotgun-reload-20261019/takes/pozhar_*; boxmag.py, boxsum.py, parts.py, timeline.py there).
--
--   * 16 frame bones: `cocking_device` (10) is the charging handle, on the RIGHT of the receiver (weapon x +32 mm),
--     with its two pins; `slider` (9) the front shroud; `bullet` (12) the round in the chamber, which the game draws
--     itself. The magazine rig is the shared 5-bone one, a 206 mm box.
--   * THE HANDLE IS WORKED ONLY IN THE EMPTY RELOAD -- 64.0 mm back at t=3.11..3.13, held, home by 3.38 (1.34 and
--     1.38 m/s at the fastest frame) -- and the left hand does it, over the top. Hence `chamberOnRack = 'dry'`.
--   * Controls as on the Crusher (for a rifle-style magazine): GRIP on the magazine is the two-hand hold,
--     GRIP + TRIGGER takes it out, B drops it; the handle is on the LEFT TRIGGER; two two-hand holds, the fore-end
--     (CyberpunkVR_TwoHandGrip_pozhar.ini, the game's own) and the magazine (..._pozhar@mag.ini), the second taken
--     away with no magazine in the well.
return {
    id    = 'techtronika_pozhar',
    match = 'techtronika_pozhar',

    -- pwa_w_rifle_precision__techtronika_pozhar.anims
    sounds = {
        slidePull    = 'w_gun_rifle_power_pozhar_handle_pull',
        slideRelease = 'w_gun_rifle_power_pozhar_handle_release',
        magOut       = 'w_gun_rifle_power_pozhar_mag_out',
        magIn        = 'w_gun_rifle_power_pozhar_mag_in',
        magGrab      = 'w_gun_rifle_power_pozhar_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 16, names = { { 9, 'slider' }, { 10, 'cocking_device' }, { 12, 'bullet' } } },
    },

    -- the bore: `barrel` (the origin) to the muzzle, (0, 469.5, 48.0) mm
    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_cocking_device',
        handleSide = 'right',  -- the handle sticks out on the right only (the slide bone's vertices, x -0.029..+0.044 m): reload.lua handGrips
        which      = 1, bone = 10,
        -- 64.0 mm, the same in both empty takes, along the bone's local -Z: straight back along the bore
        travel     = 0.0640,
        restLocal  = { -0.03240, 0.02970, 0.16540 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        releaseSpeed = 1.35,
        gripSnap   = true,
        -- the grips' wrist points sit 96 mm from the handle, and 50 mm round a point that far off caught a hand
        -- reaching for the handle late (the magnet was a little weak with the preview pose on the slide)
        previewRadius = 0.07,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- THE GAME'S LEFT HAND on the handle on the right of the receiver: the wrist against the handle's slot with the
        -- handle held back, the two empty takes within 0.1 mm / 0.05 deg of each other -- and a hand from UNDER the
        -- gun, the wrist 52 mm below the handle and 81 behind it. Nobody racks a right-hand handle like that, from
        -- below: the gun is rolled to the left, the magazine to the right, and the hand
        -- comes down onto the handle from above. A cant is only taken out for FINDING a grip -- once gripped the
        -- hand is eased onto the real one -- so a rolled gun with the take's grip alone turned that wrist a quarter
        -- turn (the wrist has to turn). `side` is the same hand turned 90 deg about the bore through the
        -- handle, from under the gun to its right side: above it once the gun is rolled. The nearer one is taken.
        -- Both wear `_v`: the take's fingers with the index and middle spread, so the paddle goes through the gap
        -- between them (in the gap between the fingers).
        grips = {
            behind = { off = { 0.0287, -0.0812, -0.0521 },
                       rot = { 0.6680, 0.7060, 0.1669, -0.1660 },
                       pose = 'pozhar_handle_left_v' },
            side   = { off = { 0.0521, -0.0812, 0.0287 },
                       rot = { 0.3543, 0.6166, 0.5904, 0.3818 },
                       pose = 'pozhar_handle_left_v' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- Source rig track showMagazine; the old value 1 was its remapped destination, not its rig index.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_precision\\techtronika_pozhar\\entities\\meshes\\w_rifle_precision__techtronika_pozhar__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_pozhar.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- 39.9 x 123.3 x 205.8 mm, long along its own Z and thin across X
        drop = {
            centre  = { 0.0, 0.0132, -0.0941 },
            flat    = 0.0200,
            arm     = 0.083,
            probe   = 0.0200,
            inertia = 0.0048,
        },

        -- THE HOLD, the bottom of a long box in the hand: rigid over t=1.20..2.04 of the empty take (39 frames, 2.8 mm
        -- and 0.6 deg), three of the four takes within 0.1 mm / 0.01 deg.
        -- ...AND NOT THE HOLD A VR HAND CAN USE. The game's left hand has this magazine UPSIDE DOWN for one: the base
        -- under the thumb and the box out past the little finger, so a hand bringing it up into the well had to turn
        -- over, 172.5 deg (the wrist turned over as the magazine went in). Turning the box in the hand about the slot put
        -- it beside the fingers (the magazine hung above the hand). So the hand is the CRUSHER's -- base at the little
        -- finger, the box up past the thumb, 23 deg from a VR hand at this well -- with its finger poses, and the box
        -- moved (4.1, -11.0, 37.2) mm in its own frame so its -X face is on the same palm, its +Y face under the same
        -- fingertips and its base on the same heel of the hand (tools/borrow_hold.py). Was the take's own:
        -- holdOffset { 0.00018, -0.03219, -0.16932 }, holdRotBasis { 0.06170, -0.99724, -0.00937, 0.04033 }.
        -- ...AND CLOSED ON THE MESH (2026-10-25, the magazine sat wrong in the hand): with the Crusher's hand the thumb tip ran 12 mm into this box and the fingers stood 5-10 mm off it. tools/graspfit.py
        -- (build/shotgun-reload-20261019) moved the magazine (+12, +7, +23) mm in its own frame, turned it 8.5 deg, and bent the
        -- finger joints a few degrees (pozhar_mag_left_*), until the palm lies on the near face, the fingertips on the far
        -- one, the thumb on the box and nothing is inside the mesh. Before: holdOffset { 0.01126, 0.04312, 0.08516 }.
        holdOffset   = { 0.02126, 0.06185, 0.10163 },
        holdRotBasis = { -0.29726, 0.04696, 0.05848, 0.95184 },

        handPull       = true,
        pullTrigger    = true,
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

        -- UP FROM UNDER THE GUN AND FROM BEHIND, rocked in: the last 60 mm of the four takes, 13.5 .. 22 deg off the
        -- weapon's -Z towards the stock, averaged (residuals 8 .. 12 mm -- the rock)
        wellAxis   = { 0.0160, -0.2844, -0.9586 },
        insertRun  = 0.100,
        seatAt     = 0.70,

        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.100, off = { 0.02126, 0.06185, 0.10163 }, rot = { -0.29726, 0.04696, 0.05848, 0.95184 },
                         pose = 'pozhar_mag_left_carry' },
            { d = 0.030, off = { 0.02126, 0.06185, 0.10163 }, rot = { -0.29726, 0.04696, 0.05848, 0.95184 },
                         pose = 'pozhar_mag_left_seat' },
        },
        pose           = 'pozhar_mag_left_seat',
    },
}
