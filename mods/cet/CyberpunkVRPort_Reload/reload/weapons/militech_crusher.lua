-- Militech Crusher — physical-reload configuration.
--
-- A SHOTGUN WITH A BOX MAGAZINE, so it is reload.lua's and not shotgun.lua's: the machine is a pistol's -- a magazine
-- in a well in front of the trigger guard and a charging handle -- at a shotgun's size. Everything below was read off
-- four takes of its own reload with VRIK off (build/shotgun-reload-20261019/takes/crusher_*; tools boxmag.py,
-- boxsum.py, parts.py, timeline.py there), two with a round left and two empty.
--
--   * 14 frame bones: `reload_handle` (5) is the charging handle on the LEFT of the receiver, `magazine_lock` (11)
--     the catch in front of the well (it turns 16.3 deg while the magazine leaves; cosmetic, not driven), `mag_slot`
--     (10) the well. The magazine rig is the shared 5-bone one.
--   * THE HANDLE IS WORKED ONLY IN THE EMPTY RELOAD -- 70.0 mm back at t=3.13..3.31 and home by 3.40 -- and in no
--     other clip, and it never rests anywhere but home: no stop. Hence `chamberOnRack = 'dry'`: a magazine swapped with
--     rounds still in the gun goes on firing, one swapped into a dry gun waits for the handle.
--   * THE MAGAZINE IS DRAWN AT `mag_slot`. The magazine component hangs off it -- the rig chain alone would put the
--     mesh at the entity origin, in the grip, which is not where the game draws it -- so `vrp_mag_slot` on that bone
--     IS the seat, and the mesh's own origin lands on it to a tenth of a millimetre in every take.
return {
    id    = 'militech_crusher',
    match = 'militech_crusher',

    -- Its own bank (pwa_w_revolver__militech_crusher.anims). No `mag_new_grab` in it: the fresh magazine's rattle
    -- (frame 51 of every reload) is the sound of one coming out of a pouch.
    sounds = {
        slidePull    = 'w_gun_revol_power_crusher_handle_pull',
        slideRelease = 'w_gun_revol_power_crusher_handle_release',
        magOut       = 'w_gun_revol_power_crusher_mag_out',
        magIn        = 'w_gun_revol_power_crusher_mag_in',
        magGrab      = 'w_gun_revol_power_crusher_mag_rattle',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 14, names = { { 5, 'reload_handle' }, { 10, 'mag_slot' }, { 11, 'magazine_lock' } } },
    },

    -- The bore: `barrel` (the frame's own bone, at the origin) to the muzzle, 2.4 deg off the weapon's Y. Not the
    -- handle's slot -- it sits 27 mm off the bore, which would skew the rack axis by 6 deg.
    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    slide = {
        slot       = 'vrp_reload_handle',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.036..+0.014 m): reload.lua handGrips
        which      = 1, bone = 5,
        -- 70.0 mm, the same in both empty takes, along the bone's local -Z: straight back along the bore.
        travel     = 0.0700,
        restLocal  = { 0.02700, 0.00820, 0.16420 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 70 mm home in about 0.09 s: 1.17 and 1.59 m/s at the fastest frame of the two takes.
        releaseSpeed = 1.20,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        -- one grip, so no orientation test to choose between grips
        styleAngleMax = 180.0,
        -- ON THE LEFT TRIGGER, not the grip: the grip near this gun is the
        -- two-hand hold on its magazine. The trigger is kept from the game (it is the aim) while the hand is on it.
        grabWith   = 'trigger',
        -- THE LEFT HAND OVER THE TOP, onto the handle on the left of the receiver. The wrist relative to the handle's
        -- slot in the weapon's frame, over the 13 frames the handle travels in each empty take (the two agree to
        -- 0.2 mm / 0.1 deg; 15 mm of spread within each, the hand riding a part that moves 70 mm).
        grips = {
            behind = { off = { -0.0461, -0.0862, -0.0108 },
                       rot = {  0.2268, -0.0879,  0.5644,  0.7889 },
                       pose = 'crusher_handle_left' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 1, not 0. This magazine rig's float tracks read 0, 1, 0, 0 with the magazine home, and in the takes
        -- index 1 drops to 0 for exactly the stretch the gun's own magazine is out (t=0.82..1.71) while index 2 shows
        -- the one in the hand. Writing 0 to index 1 live hid it; 0 left it in the gun after a drop.
        showTrack  = 1,
        mesh       = 'base\\weapons\\firearms\\revolver\\militech_crusher\\entities\\meshes\\w_revolver__militech_crusher__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_crusher.ent',
        -- the mesh's origin is on the magazine's top face (Z -159.8 .. +13.3), the family's convention: no shift
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- 31.6 x 104.1 x 173.1 mm, long along its own Z and thin across X -- the family's shape, only bigger.
        drop = {
            centre  = { 0.0, 0.0118, -0.0733 },
            flat    = 0.0158,
            arm     = 0.070,
            probe   = 0.0158,
            inertia = 0.0034,
        },

        -- THE HOLD, measured, and it is one relation from the pouch to the well: the fresh magazine in the WeaponLeft
        -- slot, rigid over t=1.87..2.44 of the take (27 frames, 0.4 mm and 0.6 deg of spread) and still the same
        -- with the magazine home (t=2.69..2.91). Four takes agree to 0.2 mm and 0.03 deg.
        holdOffset   = { 0.00672, 0.03450, 0.04740 },
        holdRotBasis = { -0.24320, 0.00253, 0.07919, 0.96673 },

        -- A RIFLE'S MAGAZINE, rocked out by hand -- by GRIP + TRIGGER. A plain grip on it is the two-hand hold, as
        -- on a rifle the support hand holds the magazine well (a plain grip holds the magazine like a
        -- two-hand hold); B still drops it.
        handPull       = true,
        pullTrigger    = true,
        -- TWO HOLDS (the fore-end handle and the magazine): the game's own on the fore-end (CyberpunkVR_TwoHandGrip_crusher.ini)
        -- and one ON the magazine (..._crusher@mag.ini, made from the take with tools/hold_from_take.py) -- hold 1.
        -- With no magazine in the well that one is taken away; the fore-end stays.
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

        -- THE LAST 60 mm ARE A STRAIGHT LINE, up from under the gun and 7 deg forward: four takes, residuals 2.5 to
        -- 9.3 mm, averaged. Before that the magazine comes in from in front and rocks back 15-25 deg as it enters,
        -- which the magnet's swing covers.
        wellAxis   = { -0.0060, 0.1220, -0.9925 },
        insertRun  = 0.100,
        seatAt     = 0.70,

        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.100, off = { 0.00672, 0.03450, 0.04740 }, rot = { -0.24320, 0.00253, 0.07919, 0.96673 },
                         pose = 'crusher_mag_left_carry' },
            { d = 0.030, off = { 0.00672, 0.03450, 0.04740 }, rot = { -0.24320, 0.00253, 0.07919, 0.96673 },
                         pose = 'crusher_mag_left_seat' },
        },
        pose           = 'crusher_mag_left_seat',
    },
}
