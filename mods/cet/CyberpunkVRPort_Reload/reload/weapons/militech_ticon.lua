-- Militech Ticon — physical-reload configuration.
--
-- The twelfth pistol and the first out of Phantom Liberty, so everything of its own lives under `ep1\`.
--
--   * ELEVEN bones, the third rig in the set with that count and unlike either of the others (the Unity family and
--     the Lexington). TWO large moving parts: `middle_slider` (3), the front shroud, and `end_slider` (5), the whole
--     upper -- and `muzzle_slot` and `fx_muzzle` are CHILDREN of the front one, so on this weapon the muzzle moves.
--   * Its magazine rig is the first with SIX bones -- an extra `bullet_rotator` -- which makes it unambiguous on
--     count alone. Its bind rotations are 92 and 132 deg where every other magazine in the set carries 180, so the
--     bone-versus-mesh turn was derived from the mesh matrix rather than taken from the others (it came out at the
--     familiar 180 all the same -- the BINDS differ, the mesh matrix does not).
--   * No `ejection_port` effect, so no casing: it has `charging` and `charged` instead, and an `fx_ejection_port`
--     slot with nothing hung on it.
return {
    id    = 'militech_ticon',
    match = 'ticon',

    -- Its own bank, and a detailed one: two different magazine exits (`mag_out` and `mag_out_fast`), a
    -- `mag_in_first_touch`, and `barrel_release` -- which plays in the EMPTY reloads only, on the frame the rear
    -- block goes home. That last one is this weapon's slide-release sound and the stick click plays it.
    sounds = {
        magOut       = 'w_gun_ticon_mag_out',
        magIn        = 'w_gun_ticon_mag_in',
        magGrab      = 'w_gun_ticon_grab_mag',
        slidePull    = 'w_gun_ticon_grab',
        slideRelease = 'w_gun_ticon_barrel_release',
    },

    basis = 'weapon',

    rigs = {
        { which = 1, bones = 11, names = { { 3, 'middle_slider' }, { 5, 'end_slider' }, { 6, 'mag_slot' } } },
        { which = 0, bones = 6,  names = { { 1, 'magazine' }, { 3, 'mag_std' }, { 5, 'bullet_rotator' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_end_slider' },

    slide = {
        ejectLiveRound = false,
        slot       = 'vrp_end_slider',
        which      = 1, bone = 5,
        -- MEASURED live, which corrects the animation on both ends: the bone rests at Z -41.8 (well, -40.8 in the
        -- take) and sits at -83.9 while the gun is dry -- 42.1 mm back, its stop. Worked by hand it reaches -88.5,
        -- so the full travel is 47.7 mm and the catch releases 5.6 mm deeper, exactly the pattern the Lexington has.
        travel     = 0.0477,
        restLocal  = { 0.00000,  0.02760, -0.04180 },
        lockLocal  = { 0.00000,  0.02760, -0.08390 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 47.7 mm home in 0.09 s, measured in the take (t=3.91 at -88.5 to t=4.00 at -40.8).
        releaseSpeed = 0.53,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        -- ...without which a grip takes the part but never takes the HAND: the block moved and the wrist was left
        -- wherever the player's arm happened to be. Every weapon in the set that has a grip declares it.
        gripSnap   = true,
        -- 90 mm rather than the default 50: this block is 280 mm long and a hand can meet it anywhere along it, so
        -- a radius sized for a pistol slide finds nothing. Read at the SLIDE level -- a per-grip one is ignored.
        -- 40 mm, and no `previewOff` -- the preview has to fade in exactly where the hand will be HELD and the snap
        -- has to take it in that same place, or the grip is offered at one point and applied at another.
        --
        -- 55 after a pass at 40: the point is right but a sphere on the block's rear edge is unforgiving, and a
        -- grip that has to be found twice is worse than one offered a centimetre early. Still under the set's
        -- default. (The module's default is 50 mm and only the Liberty went to 75.) This weapon is the reason it
        -- was tight at all: its grip point sits at the very REAR of the block, so a zone of any size hangs
        -- off the back of the gun towards the player -- 75 mm of it was a hand's width of empty air that still
        -- offered the grip. A sphere is the wrong shape for the job and 40 mm is the honest way to shrink it.
        previewRadius = 0.055,

        -- THE THUMB GOES WITH THE CLICK. The game closes this block with a thumb on its switch, and the take shows
        -- the press as an isolated thing: over the frames the block leaves the catch the metacarpal swings out
        -- 16.2 -> 37.9 deg while the two phalanges STRAIGHTEN, 8.9 -> 1.8 and 30.1 -> 12.3, and index and middle
        -- move 0.0 while ring and little move 1.0 and 2.0 -- they go on holding the gun. So the pose is three
        -- joints, laid on the HOLDING hand, and a pose writes only the joints it names.
        --
        -- The animation does the whole thing in 0.17 s -- one frame down, three held, one back -- on a hand nobody
        -- is looking at. Stretched here, because in VR it is in front of the player's face and he asked to be able
        -- to SEE the press: 0.08 s accelerating onto the button, 0.09 held against it, 0.20 easing off.
        pressPose  = 'ticon_thumb_press',
        pressIn    = 0.08,
        pressHold  = 0.09,
        pressOut   = 0.20,

        -- BOTH WAYS, by choice. The game only ever sends this block home on its own -- so the right stick
        -- click does that, and plays `barrel_release` -- but a hand should be able to pull it back too.
        --
        -- THE FINGERS ARE THE GAME'S, out of `first_equip` at t=2.767, the one clip in which its hand touches this
        -- part at all. THE WRIST IS NOT: that skeleton carries no weapon bones, so there is nothing there to measure
        -- a hand against, and the offset below is transferred from the Constitutional Unity's palm grip -- the same
        -- split the Lexington's pinch was built on, and like it, it wants an eye in the headset.
        -- THE GRIP POINT IS MEASURED ON THIS WEAPON, the Liberty's way -- off = R^-1 * (wrist - slide slot) in the
        -- weapon's own frame -- only from the ANIMATION PAIR rather than from a recorder take, because the game
        -- never puts a hand on this block during a reload. Both files are keyed to the same timeline, so at
        -- t=2.767 of `first_equip`:
        --
        --     the player's `pwa` GLB gives the left wrist relative to `WeaponRight`, which is where the gun hangs;
        --     the weapon's own GLB gives `end_slider` at the same instant, so the slot is read where it really was.
        --
        -- THAT IS WHAT MAKES IT RIGHT IN BOTH STATES: the offset is relative to the
        -- SLOT, and the slot rides the block. Verified rather than assumed -- through the take's release the bone
        -- runs -83.9 -> -40.8 and the slot moves +43.1 mm, all of it in the along-bore term, 1:1. So the same grip
        -- point sits at the block's rear whether the block is home or standing back on the empty catch.
        --
        -- TRANSFERRING THE LIBERTY'S WHOLESALE COULD NOT WORK, and the reason is worth keeping: `off` is read in the
        -- weapon's own axes -- X across, Y ALONG THE BORE, Z up (all three confirmed on this weapon: the bore
        -- measures (0, +0.9999, -0.0112), the seated magazine stands on +Z, and the block's travel lands entirely
        -- in Y). The Liberty's slot sits mid-slide, this one's at the very rear of a 290 mm block, so its -0.0899
        -- put the wrist 90 mm BEHIND the gun and its -0.0341 put it 34 mm BELOW -- the grip far from the slide.
        --
        -- The measured hand sits 71 mm out to the LEFT, level with the block and level with the slot. The side is
        -- not from the export's handedness, which cannot be read off this rig (every bone has X = 0): it is from
        -- the take, where the left wrist measures negative X in the weapon's frame in every frame of every take.
        --
        -- The ROTATION stays the Liberty's, and so do the fingers, bar the ring and little opened 5 % (see the pose
        -- file) -- the fingers and the wrist turn are one measurement and the headset
        -- has already passed them (the pose works). The style gate is off: with a single grip there is nothing to
        -- disambiguate, so an orientation test can only refuse a grip that should have been offered.
        --
        -- ...AND BACK UP AGAIN, to exactly the measured value. It went 50 mm down in two passes on the way the hand
        -- looked while pulling the block, and came 50 mm back when the grip started being offered half a hand too
        -- low and, worse, when the magazine stopped being takeable at all.
        --
        -- The two are one fault, and the geometry says so plainly. Measured from the magazine slot, the slide's grip
        -- point and the magazine's own reach point sit:
        --
        --     grip 50 mm down  ->  62.5 mm apart      -- with a 55 mm zone and a 40 mm one, all but touching
        --     grip as measured -> 104.7 mm apart      -- cleanly separated
        --
        -- and `takeAt` had just moved the magazine's reach 51 mm DOWN to the floorplate, straight towards the
        -- lowered grip. Whichever the hand was nearer took the squeeze, and near the base that was a coin toss.
        -- Two zones on one gun cannot be placed one at a time.
        styleAngleMax = 180.0,
        grips = {
            overhand = { off = { -0.0711,  0.0005, -0.0423 },
                         -- WHERE IT IS LOOKED FOR AND SHOWN, 50 mm above where it is HELD. Two separate places on
                         -- purpose, and the pair was arrived at one at a time: the hold is the headset's, judged on
                         -- how the hand sits while it drags the block back, and the preview is 50 mm up because
                         -- that is where a tracked arm actually arrives. Moving the hold to meet the preview took
                         -- the good half with it. `previewOff` moves the offered point and the pull, never the hold.
                         previewOff = { 0.0000, 0.0000, 0.0500 },
                         rot = {  0.1781, -0.4405,  0.0719,  0.8769 },
                         pose = 'ticon_slide_left_palm' },
        },

        -- `middle_slider` RIDES IT, but not on a fixed ratio: on recoil it gives 27.6 mm against the block's 75.0
        -- (0.368), and in the empty reload 9.7 against 54.7 (0.177) and 3.9 against 63.5 (0.062). That is a part
        -- with a stop of its own, like the Lexington's top piece -- 9.8 mm is the most it was ever seen to travel.
        riders = { { bone = 3, ratio = 0.368, cap = 0.0098 } },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'ep1\\weapons\\firearms\\handgun\\militech_ticon\\entities\\meshes\\w_handgun__militech_ticon__mag_std_01.mesh',

        entity     = 'base\\vrport\\vrp_mag_ticon.ent',
        -- The mesh matrix says zero; 3 mm BACK ALONG THE BORE is the headset's, once the axis below was right
        -- (5 mm, then 2 back the other way). At this size it is a constant and it belongs here -- 2 mm in the fist
        -- is beneath noticing, so the stage holds are left alone rather than compensated.
        --
        -- AND IT STAYS AT 5 mm, after a round of chasing a miss with it that it could never have fixed. The magazine
        -- was going in a good 25 mm too far forward AT THE TOP OF THE RUN and arriving correctly AT THE SEAT, and
        -- this knob is a CONSTANT: every millimetre put in here moves the magazine at the seat, in the hand and
        -- everywhere between by the same amount. Cranking it right at the top therefore broke the bottom, which is
        -- exactly what the headset reported at -50 and again at -40. An error that grows with depth and vanishes at
        -- the seat is a WELL AXIS error, and that is where the fix went -- see `wellAxis`.
        originOffset = { 0.00000, -0.00300, 0.00004 },

        -- Its magazine measures 36.4 x 81.1 x 127.7 mm, long along its own Z -- the FAMILY's shape, unlike the
        -- Kappa's and the Chao's, so the solver's defaults hold and only the size is given.
        drop = {
            centre  = { 0.0, -0.0208, -0.0581 },
            flat    = 0.0182,
            arm     = 0.045,
            probe   = 0.0182,
            inertia = 0.00190,
        },

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- OUT OF THE GRIP, like the nine before the Kappa -- and MEASURED ON THE MESH, which on this weapon is a
        -- different line from the one its magazine BONE travels. That distinction has never mattered before and it
        -- cost most of a session here.
        --
        -- The recorder's `mb` bone 1 is `magazine`, the bone that carries the travel; the mesh is skinned to
        -- `mag_std`, which hangs 111.1 mm off it. Through the insertion `magazine` also ROTATES -- 14 deg at the top
        -- of the run, 0.9 at the seat -- and on a 111 mm arm that swings the mesh's origin 27 mm at the top and 2 mm
        -- at the seat. So the bone runs down one line and the magazine the PLAYER SEES runs down another, 14.8 deg
        -- away, and the two meet only at the seat.
        --
        -- Which is the exact shape of what was seen: too much offset at the start, normal by the end of the
        -- insertion. Every earlier weapon got away with the bone because its mesh bone sat ON the travelling one.
        --
        -- The line below is the least-squares fit through the MESH ORIGIN's own path -- p(bone1) + R(bone1) * p(mag_std)
        -- per frame -- over the 23 frames of the clean one-magazine approach, 115 mm down to 24. Worst residual
        -- 9.6 mm overall and under 4 mm across the whole stretch the magnet works in. The frames are read in the
        -- rig's own frame, which is the weapon's: the magazine slot measures 0.0 deg from the weapon's orientation,
        -- and at the seat the mesh origin coincides with the slot to well under a millimetre.
        wellAxis   = { -0.0084, -0.3611, -0.9325 },
        -- 115 mm in the mesh's own metric, where the bone measured 111 for the same motion.
        insertRun  = 0.115,
        seatAt     = 0.70,

        -- The magnet ran on the default window, 1.6 x the run = 178 mm, and took the hand a good 5 cm before it had
        -- any business to -- while the magazine was still being swung round rather than lined up. 128 mm.
        magnetDepth = 0.128,

        -- MEASURED over the rigid stretch (15 frames, 6.3 mm of spread, 2.82 deg), and composed through the WHOLE
        -- bone chain -- which is where the first attempt went wrong and put the magazine in upside down.
        --
        -- The mesh is skinned to `mag_std`, NOT to `magazine`. Every earlier weapon could be measured on `magazine`
        -- and be right, because their `mag_std` sat on it with an identity rotation; this rig's carries a static
        -- 131.7 deg (verified static: 0.00 deg of spread over all 280 frames). Leaving it out turned the hold by
        -- exactly that, and 131.7 deg on a flat magazine reads as base up. So the chain is
        -- mount -> magazine -> mag_std -> the mesh turn, all four, and the rule is to compose down to the bone the
        -- MESH names rather than to the one that carries the travel.
        -- ...and then placed by eye in the headset and printed from the game: += { -44, -54, +38 } mm and
        -- 0 / -53.95 / -41.35 deg, composed in the tuner's own order. A 67 deg correction on top of a measured
        -- value, which is what a hold that is right about WHICH WAY UP but not about how it sits in the fist looks
        -- like -- and the size of it is itself a check that the 131.7 deg above had already been taken out.
        holdOffset   = { -0.03908, 0.02574, 0.00803 },
        holdRotBasis = { 0.15189, 0.22705, -0.13252, -0.95279 },

        -- 50 mm: the Silverhand's, the tightest value in the set that is known to work, against the 100 the rest
        -- carry by inheritance. `snapRadius` is the catch for REACHING a magazine seated in the gun -- nothing to do
        -- with the push, which runs on `seatSnapRadius` and `insertGain`.
        --
        -- AND NOTHING ELSE. A round was spent inventing knobs for this weapon -- a reach evaluated at a nominated
        -- ladder stage, and a catch measured against a point on the magazine's floorplate instead of against the
        -- wrist the hold reconstructs. Both are gone. The first is why the hand had to be contorted: aiming the
        -- reach at the PUSH relation meant the magnet turned the wrist to the pose that pushes a magazine in at the
        -- moment the player was reaching to pull one out. Eleven weapons do this the plain way and all eleven work.
        snapRadius     = 0.050,

        -- WHERE IT IS REACHED FOR, and this one is MEASURED IN THE HEADSET rather than judged. The catch reports
        -- its miss on the weapon's three axes, so a few honest reaches at the magazine settle it: over the four
        -- tightest frames -- the hand 30 to 43 mm out, which is as close as it got -- the gap read across +1, along
        -- the bore +6 and up -36 mm, the same three numbers each time. The hand sits 3.6 cm BELOW the wrist the
        -- hold implies, which is why the catch only ever fired from one contorted approach.
        --
        -- Added to the two passes before it, that is 31 mm forward along the bore and 66 mm down. The hold is
        -- right
        -- -- it is what makes the magazine sit correctly in the fist -- but the wrist it implies lands 2-3 cm back
        -- towards the shooter of where a tracked arm actually comes to the well. Same knob, same axes and same
        -- reason as a slide grip's `previewOff`: offered where the hand is, held where it was measured.
        previewOff     = { 0.001, 0.031, -0.066 },

        -- ...AND WHAT ACTUALLY DECIDES IT: the palm at the magazine's FLOORPLATE, facing up, in any orientation.
        -- `takePoint` is that spot in the magazine's own mesh axes from its seated origin -- its mesh runs
        -- Z -121.9 .. +5.8, and at the seat those axes are the weapon's, so the base is 121.9 mm straight down.
        --
        -- ...and 50 mm BACK along the bore, from the headset. Most of that is the magazine's own shape rather than
        -- a correction: its body runs Y -61.4 .. +19.7 about the origin, so the middle of the floorplate is already
        -- 21 mm behind the point directly under the origin. Hanging the zone off the origin put it that far forward
        -- before anything else, and a hand cupped under a magazine sits under its MIDDLE.
        --
        -- NOT A BALL: 90 mm across and 90 tall, but only 45 ALONG THE BORE. Wide in two directions on purpose --
        -- the tracked hand point sits ahead of the wrist joint and swings through several centimetres as the hand
        -- turns, and the whole point here is that any way round should work. Short in the third because a ball of
        -- that size hangs 90 mm out in front of the gun, where there is nothing to take hold of.
        --
        -- What keeps the width from being sloppy is the palm test rather than a tight zone. 0.25 -- 75 degrees
        -- either way about the WEAPON's up -- after 0.5 turned out to be the only asymmetric term in the whole
        -- check: the zone itself is centred on the axis and symmetric on every axis, so "works from the right, not
        -- from the left" could only come from the one test that reads the hand rather than its place. A palm cupped
        -- under a magazine from the left and from the right is tilted opposite ways, and 60 degrees clipped one.
        takePoint      = { 0.0, -0.050, -0.1219 },
        takeRadius     = { 0.090, 0.045, 0.090 },
        takeUp         = 0.25,

        -- WIDE AND HARD, so the magazine is ON the line from the first frame of the push rather than drifting onto
        -- it. The pull is weighted -- (1 - perp/R) * gain, clamped -- so the family's 0.055 and 2.6 only reach full
        -- strength within 34 mm of the centreline; 75 mm and a gain of 6 give full pull anywhere within 62 mm.
        seatSnapRadius = 0.075,
        -- 70 mm, NOT the family's 120. The magnet stays disarmed until the magazine has been taken this far out of
        -- the well, and on every other weapon 120 is comfortably inside a 120-150 mm run. This one runs 115 with the
        -- magnet's window ending at 128, so arming needed the magazine carried past 120 -- an 8 mm band it almost
        -- never hit. The log read `W=0.000` frame after frame at a perpendicular of 20-40 mm: what looked like a
        -- weak magnet, a bad axis and a crooked hold was a magnet that was never switched on.
        armDist        = 0.070,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 6.0,
        dropPush       = 1.1,

        -- THE HAND ROLLS TO THE BASE AND PUSHES FROM THERE -- the same motion the Kenshin, the Lexington and the
        -- Omaha have, and it needs per-stage PLACEMENT, not just per-stage poses. Measured the same way it was on
        -- the Lexington, by tracking the hold slot's distance to the magazine's own floorplate through the push:
        --
        --     depth 375..115 mm   wrist 61-95 mm from the mount, 69-133 from the base   -- carried by the body
        --     depth 102..3  mm    wrist 96-118 from the mount, 18-38 from the base      -- the palm IS on the base
        --
        -- So the re-grip happens at about 110 mm, near the top of the run, and the ladder rolls the placement from
        -- the carry to the base over the next 40. The push end is measured on its tightest frames (t=3.31..3.40)
        -- and then carried into the same corrected frame the carry lives in, by applying the headset's own
        -- correction to it -- otherwise the roll would start in a tuned pose and end in an untuned one and jump.
        poseStages = {
            { d = 0.111, off = { -0.03908, 0.02574, 0.00803 },
                         rot = { 0.15189, 0.22705, -0.13252, -0.95279 },
                         pose = 'ticon_mag_left_carry' },
            { d = 0.108, off = { -0.03908, 0.02574, 0.00803 },
                         rot = { 0.15189, 0.22705, -0.13252, -0.95279 },
                         pose = 'ticon_mag_left_carry' },
            { d = 0.085, off = { 0.09541, -0.00283, -0.01795 },
                         rot = { -0.17935, -0.60972, 0.12692, -0.76156 },
                         pose = 'ticon_mag_left_open' },
            { d = 0.034, off = { 0.09541, -0.00283, -0.01795 },
                         rot = { -0.17935, -0.60972, 0.12692, -0.76156 },
                         pose = 'ticon_mag_left_seated' },
        },
        pose           = 'ticon_mag_left_seated',
    },
}
