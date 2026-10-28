-- Malorian Overture — physical-reload configuration.
--
-- The thirteenth weapon and the FIRST REVOLVER, which makes it the first whose reload has no slide, no magazine
-- well and no part that slides at all. Everything on it turns:
--
--   * `magazine_open` (4) is the CRANE. It swings the cylinder 100 deg clear of the frame -- measured in both takes,
--     and in both it stays out for exactly 1.60 s before the game shuts it.
--   * `cylinder` (8) is the crane's CHILD and spins about its own axis. 57.7 deg is one chamber of six and 177.7 is
--     three. It turns in the ORDINARY reload and not at all in the empty one, which is the gun's own logic: with
--     every chamber spent there is nothing to index round.
--   * `mag_slot` (9) is a child of the CYLINDER, so the seat rides both -- crane and spin. Nothing has to be done
--     about that: read the slot and it is already where the cylinder put it.
--   * `hammer` (3) cocks 37.5 deg.
--
-- AND ITS "MAGAZINE" IS A SPEEDLOADER: eleven bones, six of them `bullet_01..06` on a 13.4 mm circle -- the six
-- chambers -- and twelve meshes, a shell and a tip per round. The spawned entity mirrors all twelve, which it can
-- because every shell's mesh already carries its own chamber (its `boneRigMatrices[0].W` is exactly minus its
-- bullet bone's position in the rig).
return {
    id    = 'malorian_overture',
    match = 'overture',

    -- ITS OWN BANK, read out of `pwa_w_revolver__malorian_overture.anims` -- and the names are not the family's,
    -- which is why the first guess at them was silent. On a revolver "mag out" and "mag in" are the CYLINDER
    -- leaving and returning; `bullets_out` is the cases falling, `bullets_in` the speedloader going home, and
    -- `arm` -- which plays in `safe_action`, the one clip where the hammer moves -- is the hammer itself.
    sounds = {
        hatchOpen = 'w_gun_revol_power_overture_mag_out',
        hatchShut = 'w_gun_revol_power_overture_mag_in',
        hammer    = 'w_gun_revol_power_overture_arm',
        spin      = 'w_gun_revol_power_overture_mag_spin',
        magOut    = 'w_gun_revol_power_overture_bullets_out',
        magIn     = 'w_gun_revol_power_overture_bullets_in',
        magGrab   = 'w_gun_revol_power_overture_grab_mag',
    },

    -- Its slots are all on the bore or on parts that MOVE -- `mag_slot` rides the cylinder, which rides the crane --
    -- so there is no stable pair to build a basis from. The weapon entity's own orientation is exact and rigid.
    basis = 'weapon',

    -- WE OWN THE BONES WHILE WE DRIVE THEM. A rig write rides ON TOP of what the animation graph poses, and on this
    -- weapon the game keeps its own hand on the crane -- so our 100 deg landed on top of whatever it was doing and
    -- the cylinder went forward instead of out, but only sometimes, which is what a race between two writers looks
    -- like. The log ruled out the alternative first: `spin` read 0.0 across 212 frames, so the hand-turn was never
    -- involved, and the crane's own state machine toggled cleanly the whole time.
    ownAnim = true,

    rigs = {
        { which = 1, bones = 10, names = { { 4, 'magazine_open' }, { 8, 'cylinder' }, { 3, 'hammer' } } },
        { which = 0, bones = 11, names = { { 1, 'magazine' }, { 3, 'bullet_01' }, { 2, 'reload_magazine' } } },
    },

    -- Both static: `barrel` sits at the rig's origin and `muzzle_slot` 281.3 mm down the bore from it. The magazine
    -- slot is NOT usable here for once -- it is a child of the cylinder, so it swings away with the crane.
    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    -- NO `slide` SECTION: there is no slide, no bolt and no charging handle on a revolver, and nothing on this rig
    -- translates in any clip at all.

    -- THE CRANE, on B. Not driven by the magazine's state like the Kang Tao Chao's cover: a cylinder swings out
    -- because the shooter pushes the catch and stays out until he shuts it, loaded or not.
    --
    -- 100.0 deg exactly, the same in every clip and both takes. The game slams it open in about a twentieth of a
    -- second; a quarter is nearer a hand doing it, and the swing's own shape (fast off the latch, easing into the
    -- stop) is the module's.
    -- THE AXIS IS THE RIG'S, AND IT IS NOT THE ONE THE EXPORT NAMES. The GLB calls this rotation "about Y"; the
    -- export permutes axes, and the mapping is exact and checkable on six bones at once -- GLB (x, y, z) is rig
    -- (x, -z, y). `pos_ironsight` reads GLB (0, 28.6, -49.4) against rig (0, 49.4, 28.6), the crane itself GLB
    -- (8.8, 103.5, 15.6) against rig (8.8, -15.6, 103.5), and so on. Put through it, the animation's 100 deg about
    -- GLB -Y is 100 deg about the rig's -Z. Taken at face value it turned the cylinder in place, the wrong way.
    hatch = {
        which = 1, bone = 4,
        manual    = true,
        axis      = { 0, 0, 1 },
        angle     = 100.0,
        sign      = -1.0,
        openTime  = 0.25,
        closeTime = 0.25,

        -- A FLICK OF THE WRIST SHUTS IT. 1.4 m/s of hand speed ACROSS the bore, measured in the player's own frame so
        -- that walking and sprinting do not count -- a deliberate snap sideways reaches two to three, and carrying
        -- the gun about stays under one. 0.30 s of dead time after it opens, or the same swing that brought it out
        -- would slam it shut again on the next frame.
        -- 3.0 m/s. 1.4 was any movement at all -- merely bringing the gun about reads near 6 in the player's own
        -- frame -- and 4.0 asked for more of a throw than the gesture wants. The log's own peaks bracket it: 0.01 at
        -- rest, 4.70 on a deliberate snap.
        flickSpeed = 3.0,
        flickArm   = 0.30,
    },

    -- THE CYLINDER, turned by fingers, and only while the crane is out. The axis comes from the slot rather than
    -- from the weapon, because once the crane is open the cylinder's axis has swung 100 deg away from the gun's own
    -- frame -- `vrp_cylinder` sits on the bone and carries whatever the crane has done with it.
    --
    -- The body is 43.8 mm across, so a hand within 90 mm of the axis and 60 mm along it is a hand ON it.
    spin = {
        which = 1, bone = 8,
        slot   = 'vrp_cylinder',
        -- ...and the same correction here: the cylinder's own spin is GLB Y, which is the rig's Z. That it comes out
        -- as Z is a check in itself -- Z is the bore, and a cylinder turns about the bore.
        axis   = { 0, 0, 1 },
        -- 110 mm ACROSS and 80 ALONG. The cylinder is 43.8 mm across and 48 long, so this is the part plus a palm:
        -- what has to fall inside the zone is not a fingertip on the metal but the tracked HAND POINT, which sits a
        -- palm's depth ahead of the wrist joint and swings several centimetres as the wrist turns (measured on the
        -- Ticon). It was briefly widened to 150/120 on the strength of a `rad` reading of 1170 -- which was not a
        -- distance at all but two coordinate spaces subtracted from each other. The `sp[rad= al=]` pair in the log
        -- now reads in metres from the axis and is what these two have to cover.
        -- 70 mm ACROSS and 55 ALONG, and they can be this close now that the contact point is the FINGER PADS
        -- rather than the tracked hand point. That point sits a palm's depth behind the fingers, so the zone had to
        -- be drawn out to 110/80 to catch it at all -- which meant the gesture engaged off the heel of the hand.
        -- The cylinder is 43.8 mm across and 48 long: pads laid on it read 25 to 45 mm from the axis.
        radius = 0.070,
        reach  = 0.055,
        -- ...and 1.6 times that to LOSE it. The zone is narrow enough that a hand resting at its edge would flicker
        -- in and out of it on tracking noise alone -- and worse, the preview pose moves the very fingers the zone is
        -- measured from, which is a loop rather than noise. The point is wrist-anchored now so the loop is gone, but
        -- a one-edged zone still chatters, so this stays.
        widen  = 1.6,
        -- The sign of the roll. Positive is the right-hand rule about the cylinder's own axis, which is what the
        -- tangent's cross product gives; if it ever reads backwards in the headset this is the one number to flip.
        gain   = 1.0,
        detent = 60.0,          -- six chambers, so it clicks every sixth of a turn
        -- ...and it turns with the cylinder SHUT as well as swung out. A real bolt locks a closed cylinder, but the
        -- game does not model one and a hand laid on a cylinder expects it to turn, so the bolt is not worth the
        -- gesture. The offset it accumulates stands until the gun is put away or reloaded.
        whenShut = true,

        -- ...AND IT KEEPS GOING. Flick it and it free-wheels, slowing on a viscous term plus dry bearing friction.
        -- The pair was chosen by integrating it rather than by feel -- a firm 600 deg/s flick runs 3.3 s and two
        -- full turns, a hard 1000 runs 4.2 s and nearly four. The first cut at 1.2 / 220 stopped it in 1.2 s and
        -- eight tenths of a turn, which is a cylinder with a dry bearing, not one anybody would call spun.
        --
        -- `maxRate` is four turns a second: a ceiling against a jump in the readback, not a limit a hand will reach.
        drag    = 0.50,
        stop    = 70.0,
        maxRate = 1440.0,

        -- The hand's shape for the gesture, shown as a preview the moment it is in the zone -- and it is the GAME'S
        -- OWN, out of `idle_break_01`: the flourish V does standing still, where the left hand comes off his side to
        -- the gun and the cylinder turns 49.9 deg smoothly over exactly those frames. The only place in the whole set
        -- where a hand turns this cylinder instead of the gun indexing it 60 deg at a time. (An earlier cut borrowed
        -- the Ticon's palm and opened it by eye; this needs no such apology.)
        pose      = 'overture_spin_left',
        blendTime = 0.12,
    },

    -- THE HAMMER, on the stick click. Cocked and HELD -- there is no spring to bring it back and nothing to release
    -- it but the shot, so the write is dropped the moment the ammo count falls.
    --
    -- 37.5 deg about the rig's X, from `safe_action`; recoil throws it 40.3, which is the game's own business. No
    -- animation anywhere puts a thumb on it: across every clip the hammer moves in those two only, and in
    -- `safe_action` the player's thumb reads 0.0 deg on all three joints. The gesture is ours, the travel is not.
    hammer = {
        which = 1, bone = 3,
        axis     = { 1, 0, 0 },
        angle    = 37.5,
        -- DOWN, not up: the animation's own 37.5 deg is `safe_action`, which LOWERS a cocked hammer, and taken
        -- with a positive sign it raised one instead.
        sign     = -1.0,
        cockTime = 0.18,
        dropTime = 0.05,

        -- Uniform VR trigger: break at 85%, reset at 20%, with either hammer state.
        -- The hammer still follows analog travel and falls over 0.035 s.
        -- Index-finger animation starts at trigFrom; the crane blocks firing.

        -- THE TRIGGER BLADE ITSELF CANNOT MOVE, and it is not a matter of finding the number: this rig has ten bones
        -- -- barrel_plug, barrel, pos_ironsight, hammer, magazine_open, fx_muzzle, scope_slot, muzzle_slot, cylinder,
        -- mag_slot -- and no trigger among them. The blade is part of the barrel's own mesh, welded to it as far as
        -- the skeleton is concerned, so moving it needs the weapon's MESH and RIG rebuilt, not a config line. The
        -- hammer is the only part of the action this weapon can actually animate.
        trigPose  = 'overture_index_pull',
        trigFrom  = 0.10,

        saBreak  = 0.85,
        daBreak  = 0.85,
        release  = 0.20,
        dryDrop  = 0.035,

        -- AND THE THUMB THAT DOES IT. The reach and the rest are the game's own, out of `empty_reload`: the arc
        -- over the top at t=0.367 and the hand settled on the hammer at t=0.600, which it then holds for 1.33 s of
        -- every reload. The PRESS is synthesised, and the config says so -- no clip anywhere pushes this hammer
        -- with a thumb, only rests one on it.
        --
        -- 0.24 s to arrive, 0.10 bearing down, 0.22 to withdraw once the hammer is home. The hammer itself is held
        -- until the press lands, so the finger moves it rather than accompanying it.
        poses = { over = 'overture_thumb_over', hold = 'overture_thumb_hold', press = 'overture_thumb_press' },
        reachTime = 0.24,
        pressTime = 0.10,
        awayTime  = 0.22,
    },

    mag = {
        -- MEASURED, on takes made after the recorder was widened. It used to write five magazine-rig bones -- right
        -- for twelve pistols, none of which had more -- and a revolver's speedloader has ELEVEN, six of them one per
        -- chamber. The rounds that carry the whole gesture were simply not in the file, which is why the first pass
        -- would not resolve and why a "hold" came out rigid at 690 mm from the wrist: that was two motionless bones,
        -- not a hand.
        --
        -- With all sixteen read the take tells the story plainly (the empty one): the loader starts in at t=1.58,
        -- the six rounds ride with it from 1.86, the plunger goes down at 2.69, the rounds SEAT at 2.82 and the
        -- empty body leaves from 2.95.
        enabled    = true,

        slot       = 'vrp_mag_slot',
        which      = 0,
        -- Source track 8 is showMagazineBullets. The old direct-buffer reader
        -- observed destination 9; native writes now use the verified remap.
        -- Live mapped readback on Rosco: source 8=1, source 9 absent.
        showTrack = 8,
        -- TWO ENTITIES, because a revolver gives up one thing and is given another. What the hand BRINGS is the
        -- speedloader -- `mag_stdr`, a star with a handle and its own six rounds. What FALLS OUT of the cylinder is
        -- the six spent cases, `mag_std`, and nothing else. One entity for both roles was visibly wrong in both
        -- directions: the holder fell out of the gun with the cases, and the rounds arrived without it.
        entity     = 'base\\vrport\\vrp_load_overture.ent',
        dropEntity = 'base\\vrport\\vrp_mag_overture.ent',

        -- WHAT THE HAND CARRIES, and on this weapon it cannot be the carrier component the other twelve use:
        -- one component draws one mesh and a speedloader is seven. So the game carries it, as a real ITEM in
        -- the hand slot -- measured with it attached, its world position and the WeaponLeft slot agree to
        -- 0.0000 m, which is the same exactness the carrier has and for the same reason: neither is placed by
        -- this script per frame.
        --
        -- `entity` above is still what FALLS when the loader is thrown away, and `dropEntity` what the cylinder
        -- gives up. Only the carried one moved.
        item       = 'Items.VRPortLoadOverture',
        -- Zero, and for once by construction rather than by measurement: our entity draws the shells skinned to
        -- `bullet_01..06`, which hang off `magazine` -- the SEATED subtree, whose origin is the rig's. So the rounds
        -- land in the chambers on their own. (The carried loader is a different subtree, `reload_magazine`, 29.2 mm
        -- away at rest -- which is what the hold below is measured on.)
        originOffset = { 0.0, 0.0, 0.0 },

        -- ...and then placed by eye in the headset: += { +15, +3, -28 } mm, with the rotation left alone and every
        -- finger at 1.00 -- so the shape of the hand was already right and only where the loader sits in it was not.
        -- A 32 mm correction on a hold measured to 7.3 mm is what a speedloader being PRESENTED rather than gripped
        -- looks like: the take can say how the hand is shaped and only the headset can say where the thing goes.
        --
        -- THE HOLD, from the ordinary take: the loader's body 29 mm from the wrist, held to 7.3 mm over the frames
        -- either side of the seat, and the rotation steady to 8.1 deg. The empty take agrees on the rotation to a
        -- few degrees and puts the body at 33 mm, so the two describe the same hand. Provisional to that extent --
        -- a speedloader is presented rather than pushed, and it is never as rigid as a box magazine in a fist.
        holdOffset   = { 0.02884, 0.02090, -0.00983 },
        holdRotBasis = { 0.18773, 0.37929, -0.42737, 0.79891 },

        -- SIX WAYS ROUND, ALL THE SAME LOADER. Its rounds sit evenly on a circle, so turning it by any multiple of
        -- 60 deg about that circle's axis gives back the same object -- and a player presenting one to the cylinder
        -- uses whichever of the six his wrist is already nearest. Pinning it to the one the take happened to record
        -- meant the magnet turned the hand up to half a revolution to reach it.
        --
        -- The axis is the BORE, in the magazine's own frame: a seated magazine ends up at the weapon's own
        -- orientation whatever the hold says (the hold cancels -- see `rollWays`), so its axes are the weapon's when
        -- it is home, and the weapon's Y is the bore. Nothing about the seated loader moves for any of the six.
        rollSym = { axis = { 0, 1, 0 }, n = 6 },

        -- The line the rounds come in on, the mean of both takes -- (0.2040, 0.1361, -0.9695) and
        -- (0.1552, 0.0284, -0.9875), 7 deg apart, each fitted over its own straight stretch to under 8 mm.
        -- ALONG THE BORE, and this one is geometry rather than a fit. The line fitted to the take -- (0.180,
        -- 0.082, -0.980), nearly straight down -- is the path the HAND takes, and on every other weapon that is
        -- also the well's own line. Not here: a speedloader goes into the CHAMBERS, and the chambers run along the
        -- bore. They stay there even with the cylinder swung out, because the crane turns about the bore axis and
        -- a rotation leaves every vector parallel to its own axis alone -- so 100 deg of crane changes nothing
        -- about which way a round has to travel to enter.
        --
        -- Out of the cylinder is towards the shooter, which is -Y: the rounds go in at the rear face, which is
        -- where `mag_slot` sits and where the cases come out.
        wellAxis   = { 0.0, -1.0, 0.0 },
        insertRun  = 0.110,
        seatAt     = 0.80,

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- A DISC, not a stick. The falling-body solver was written for a box magazine and its defaults say so --
        -- long along local Z, thin across X. A speedloader is the other shape entirely: six rounds on a 13.4 mm
        -- circle about the bore, so its LONG axes are the two in that circle and its THIN one is the axis itself.
        -- Told otherwise it lands on its edge and points the chambers anywhere.
        drop = {
            long    = { 1, 0, 0 },
            thin    = { 0, 0, 1 },
            centre  = { 0.0, 0.0, 0.0 },
            flat    = 0.021,      -- half its thickness: it comes to rest lying flat, like a coin
            arm     = 0.013,      -- the chamber circle
            probe   = 0.016,
            inertia = 0.00030,
        },

        -- IT EMPTIES WHEN IT IS TIPPED UP, not when a button is pressed. B belongs to the crane on this weapon, and
        -- a revolver's cases fall out of an open cylinder under their own weight -- which is why a shooter brings
        -- the muzzle up over his palm and why they stay put when he does not. 0.35 is the bore about 20 deg above
        -- level; below that nothing leaves the gun however long the cylinder is open.
        -- WHEN THEY LET GO, from friction rather than from taste. A case in a chamber is held by nothing but its
        -- own weight against the wall: tip the cylinder and gravity splits into a part ALONG the chamber, which
        -- pulls the case out, and one ACROSS it, which is all the friction has to work with. It slides when
        -- cos(theta) > mu * sin(theta), i.e. theta < atan(1/mu). Brass on steel is mu ~ 0.2 oiled and ~0.3 dirty;
        -- 0.50 at the headset's call -- a deliberately sticky chamber, 63 deg from vertical, so the bore has to
        -- come about 27 deg above level before anything moves, against 14 at 0.25. `dropMu` asks the module for
        -- that number (0.447) rather than naming a cosine by hand, which is what `dropTilt` used to do.
        dropMu     = 0.50,

        -- SIX CASES, EACH ITS OWN BODY. They leave along the chamber they sit in, one after another rather than as
        -- a ring -- staggered by where each sits, which is what a real cylinder does without simulating it -- and
        -- each carries a little of its own radial direction, which is what opens the six into a spray. The mesh we
        -- spawn already holds chamber 01's offset in its vertices, so `meshAt` subtracts it.
        eject = {
            entity  = 'base\\vrport\\vrp_case_overture.ent',
            -- ...AND A LIVE ROUND IS A DIFFERENT OBJECT. `entity` is a spent case -- the shell alone -- and this is the
            -- same shell with its bullet still in it. Which chambers get which is decided by the gun's own count at the
            -- moment the crane opens, so a cylinder fired three times gives up three cases and three rounds.
            liveEntity = 'base\\vrport\\vrp_round_overture.ent',
            slot    = 'vrp_cylinder',
            count   = 6,
            radius  = 0.0134,      -- the chamber circle, straight off the rig: bullet_01..06 lie on it 60 deg apart
            first   = 89.7,        -- where chamber 01 sits on that circle, in degrees
            -- THE MESH'S OWN FRAME. A case runs along its Y (bbox 0 .. 35.1 mm, against 11 across) and sits at its
            -- chamber along Z (8 .. 18.9, centre 13.4) -- so the turn onto the chamber is 90 deg about X, which sends
            -- Y to Z, and the chamber offset is read in that same frame.
            meshRot = { 0.70711, 0.0, 0.0, 0.70711 },
            -- ...and its origin is at the case HEAD (the shell's mesh runs Y 0 .. 35.1 toward the tip), so the head is
            -- what has to land on the seating plane -- no shift along the length at all.
            meshAt  = { 0.0, 0.0, 0.0134 },

            -- WHERE ALONG THE CHAMBER THEY SIT. The game's rounds hang off `mag_slot`, which is 23.9 mm from the
            -- cylinder's centre on its REAR face -- and the slot we read is the cylinder itself. Seated from the
            -- centre they sat half a case too deep; from the rear face they line up with the game's exactly, which is
            -- the whole point: the game's disappear and ours appear in the same place.
            seatZ   = -0.0239,
            -- OUT ALONG -Z: `mag_slot` sits on the cylinder's -Z face, so that is the end rounds go in and cases
            -- come out. And the chamber is 48 mm deep (the cylinder's mesh runs Z -23.9 .. +24.0), which is how far
            -- each case has to slide before it is clear and free to fall.
            axis    = { 0, 0, -1 },
            chamber = 0.048,
            -- KINETIC, and deliberately not the static figure above. `dropMu` = 0.50 is what has to be BROKEN to
            -- start a case moving, and it is high because the release should want a real tilt; once it IS moving
            -- the friction that resists it is lower, which is true of every dry contact there is. Sharing one
            -- number made them creep out: they were being held back by the value that only decides when to let go.
            mu      = 0.22,
            -- Assisted extractor travel after deliberate tilt; 48 mm clears in
            -- at most 0.14 s once each staggered case starts moving.
            minExitSpeed = 0.35,
            spread  = 0.12,
            -- A head start counted from the moment a case CAN move, not from when the cylinder opened -- and it only
            -- breaks ties, because the real ordering comes from `bias`.
            stagger = 0.022,
            -- HOW MUCH THE CHAMBER'S OWN ANGLE MATTERS. A case has clearance and rests on the low side of its chamber,
            -- so one rolled to the bottom of the circle is pinched hardest and leaves last. 0.35 swings the friction
            -- by a third either way, which is enough to string six of them out without any of them looking stuck.
            bias    = 0.35,
            -- NO SPIN IS GIVEN. A case in free flight has nothing to twist it, so it turns at whatever it earned
            -- pivoting over the chamber mouth on the way out -- see the module. Its own length is what that torque
            -- works on: 35.1 mm, straight off the shell's mesh.
            caseLen = 0.0351,
            life    = 3.0,
        },

        snapRadius     = 0.05,
        -- CLOSE IN, both ways, and this weapon needs it tighter than the family. The catch is a cylinder around the
        -- well line: `seatSnapRadius` is how far OFF that line the loader may be, `magnetDepth` how far back ALONG
        -- it the magnet reaches at all. The family's default for the second is 1.6 times the measured run, which
        -- here is 176 mm -- the loader was being taken hold of two hand-widths out from the cylinder.
        --
        -- 60 ACROSS and 80 ALONG, and the difference between the two numbers is the point: the zone wants to be
        -- WIDE where the hand comes at it and SHORT where it does not. 38 across was cut too far in the same pass
        -- that fixed the reach and left the pull weak sideways -- a speedloader is presented to six chambers with a
        -- whole hand behind it, not threaded into a slot, so it needs room off the line and none along it.
        seatSnapRadius = 0.060,
        magnetDepth    = 0.080,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        -- 4.0, up from the family's 2.6. The weight is (1 - perp/R) * gain clamped, so the gain decides how much of
        -- the zone pulls at FULL strength rather than easing: 2.6 gave full pull within 23 mm of the line and the
        -- rest of the way in was mush. 4.0 over 60 mm is full pull within 45 mm -- firm from the moment it catches.
        insertGain     = 4.0,
        dropPush       = 0.0,     -- it falls; nothing pushes it out along a well, because there is no well

        -- THE HAND, from the empty take. It carries the loader by its handle with the fingers closed -- index,
        -- middle, ring and little at 134 / 106 / 88 / 89 mm from the wrist -- and opens as the rounds go home:
        -- 147 / 154 / 142 / 125 by t=2.95, thirteen hundredths after the seat at 2.82. So there are two shapes and
        -- the change belongs at the seat, which is where the ladder puts it.
        poseStages = {
            { d = 0.110, off = { 0.02884, 0.02090, -0.00983 },
                         rot = { 0.18773, 0.37929, -0.42737, 0.79891 },
                         pose = 'overture_load_left_carry' },
            { d = 0.030, off = { 0.02884, 0.02090, -0.00983 },
                         rot = { 0.18773, 0.37929, -0.42737, 0.79891 },
                         pose = 'overture_load_left_carry' },
            { d = 0.000, off = { 0.02884, 0.02090, -0.00983 },
                         rot = { 0.18773, 0.37929, -0.42737, 0.79891 },
                         pose = 'overture_load_left_seated' },
        },
        pose           = 'overture_load_left_seated',
    },
}
