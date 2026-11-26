-- Constitutional Arms Liberty — physical-reload configuration.
--
-- Measured from three recordings of its own reload with VRIK off: LIBERTYEMPTY_PALM (248 frames),
-- LIBERTYEMPTY_BACKFINGERS (213) and LIBERTYNOTEMPTY. Unlike the Tamayura and the Nue this pistol has its OWN rig
-- and its own animations, so nothing here is carried over except the magazine's hand poses -- and those are exact
-- rather than transferred, because the Unity uses THIS weapon's magazine mesh.
--
-- WHERE THE PATCH HAD TO GO, and it cost a round of empty recordings to find out: the Liberty has no root .app under
-- its own name. Its item record says `appearanceResourceName = Preset_Liberty_Rogue`, so the live entity is built
-- from `w_handgun__constitutional_liberty__rogue.app` -- which despite the filename holds ALL ELEVEN appearances
-- (default, yorinobu, dex, military1/2, neon1/2, pimp1/2, padre, rogue). Patching only the part package
-- `appearances\w_handgun__constitutional_liberty__base1_01.app` left the live mesh component a plain
-- entSkinnedMeshComponent and not one vrp_ slot resolved. Both files carry the host, the slots and the case
-- descriptor now.
--
-- ITS RIG IS THE SAME 11 BONES as the Unity's, Tamayura's and Nue's, with the same names at the same indices AND the
-- same bind pose: the slide bone rests at 85.9 mm and locks at 45.9, exactly as on the other three.
--
-- WHAT ITS ANIMATION DOES: it does not rack the slide either. The hand lifts it about 4.7 mm off the catch (45.9 ->
-- 50.6, held for five frames) and the slide runs home on its own: 52.4 -> 66.9 -> 81.6 -> 85.9, peak 0.67 m/s. Only
-- three or four frames of the whole take have the slide in motion.
return {
    id    = 'constitutional_liberty',
    match = 'constitutional_liberty',

    -- Its own sound bank: pwa_w_handgun__constitutional_liberty.anims carries 64 sound events, and every one this
    -- reload needs is there. It even has `mag_flip`, which the others do not -- its reload turns the magazine over.
    sounds = {
        slidePull    = 'w_gun_pistol_power_liberty_slide_pull',
        slideRelease = 'w_gun_pistol_power_liberty_slide_release',
        magOut       = 'w_gun_pistol_power_liberty_mag_out',
        magIn        = 'w_gun_pistol_power_liberty_mag_in',
        magGrab      = 'w_gun_pistol_power_liberty_mag_new_grab',
        eject        = 'w_gun_revol_power_overture_bullets_out',   -- borrowed, as on all four: no casing event here
    },

    -- MEASURED, and this pistol needs it more than any of them: the perpendicular residual of the magazine slot from
    -- the bore line is 1.3-2.4 mm here, against the Unity's 5.8. A basis built on that has a lever arm of
    -- millimetres, so "down" would be noise. The weapon entity's own orientation has none of that trouble.
    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 11, names = { { 5, 'slide' }, { 8, 'mag_slot' }, { 6, 'hammer' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_slide' },

    slide = {
        ejectLiveRound = true,
        slot       = 'vrp_slide',
        which      = 1, bone = 5,
        -- MEASURED, agreed by both channels: the bone's local Z (45.9 -> 85.9) and the slide slot's distance to the
        -- muzzle slot (200.0 -> 240.0 mm) both give 40.0 mm. That distance is also how long this pistol is compared
        -- with the others -- 240 mm against the Tamayura's 158 and the Unity's 149.
        travel     = 0.040,
        restLocal  = { 0.00000, 0.02160, 0.08590 },
        lockLocal  = { 0.00000, 0.02160, 0.04590 },
        -- MEASURED HERE, unlike on the Tamayura and the Nue where it had to be borrowed: the hand lifts the slide
        -- 45.9 -> 50.6 mm and holds it there for five frames before the catch lets go. 4.7 mm.
        lockPull   = 0.005,
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- MEASURED off the release ramp: 52.4 -> 66.9 -> 81.6 -> 85.9 over three frames, per-frame 0.66 / 0.67 / 0.20.
        releaseSpeed = 0.70,
        gripSnap   = true,
        -- 0.075, up from the 0.05 the others use: the palm's preview was catching only right at the pose and
        -- dropping out again (it caught too weakly). The radius is what decides both when the fingers fade in and
        -- how far out a grip can be taken, and this pistol is the longest of the set -- 240 mm from the slide slot to
        -- the muzzle against the Unity's 149 -- so the same 50 mm reads as a much tighter target on it.
        previewRadius = 0.075,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        ejectAt    = 0.55,
        styleAngleMax = 60.0,

        -- Its own `ejection_port` (w_pistol_liberty_ejection_port.effect) holds the same two emitters as the others:
        -- a smoke-and-sparks puff and cal_9mm.particle. Shipped here with the puff blanked as
        -- base\vrport\vrp_liberty_eject_case.effect, with a descriptor of this name in both .app files.
        ejectFx    = 'vrp_eject_case',

        -- THE GRIPS, MEASURED ON THIS PISTOL -- the first weapon since the Unity where that was possible, because
        -- its animation does put a hand on the slide. Both windows were taken where the SLIDE MOVES, not where the
        -- wrist merely sits still: for the first second of every take the left hand holds a rock-steady offset at
        -- 0.135 m from the slide slot, and that is the idle two-handed hold, not a grip. Reading it as one is a
        -- mistake this project has made before.
        --
        -- Expressed in the weapon's own frame, straight out of the recording: `wq` is the weapon's quaternion per
        -- frame, so off = R^-1 * (wrist - slide slot) and rot = conj(R) * wrist -- the same reconstruction reload.lua
        -- performs at runtime, done offline. No live probe and no conversion rotation involved.
        grips = {
            -- 5 frames, offset steady to 0.2 mm, rotation to 0.45 deg
            -- ...and then 25 mm FORWARD along the bore and 10 mm LEFT, set by eye in VR: as measured it sat too far
            -- back on the slide. The measurement is not wrong -- it is the game's own wrist, and the game's hand
            -- comes at the slide from a different arm than a tracked one does -- but where a VR palm wants to land
            -- is a thing only the headset can answer.
            overhand = { off = { -0.0631, -0.0899, -0.0341 },
                         rot = {  0.1781, -0.4405,  0.0719,  0.8769 },
                         pose = 'liberty_slide_left_palm' },
            -- 8 frames, 4.9 mm, 3.55 deg. Note off[2] = -0.1772 against the Unity's measured -0.1780: two different
            -- pistols, two different rigs, and the pinch lands in the same place behind the slide.
            behind   = { off = { -0.0436, -0.1772,  0.0163 },
                         -- looked for 3 cm higher than it is held, as on the Unity and for the same reason: a hand
                         -- coming from behind in VR arrives above the animation's wrist. "Up" is minus this pistol's
                         -- own measured well axis, the one direction known to be perpendicular to the bore.
                         previewOff = { -0.00025,  0.00977,  0.02837 },
                         rot = {  0.0704, -0.0897,  0.6785,  0.7257 },
                         pose = 'unity_slide_left_behind' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,                  -- its rig: trackNames = {showMagazine, showMagazineReload}, refs {1, 0}

        -- From its own magazine entity. No `originOffset`: this IS the mesh the Unity borrows, so its origin sits on
        -- the top face where the mount is, and the placement chain needs no correction.
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\constitutional_liberty\\entities\\meshes\\w_handgun__constitutional_liberty__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_liberty.ent',

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- MEASURED on the non-empty take, rotation steady to 0.25 deg. The raw measurement was
        -- { 0.11473, -0.00864, -0.04766 }, within 1.3 mm of the Unity's own { 0.11605, -0.00829, -0.04819 } -- the
        -- same magazine in the same kind of well, which is a good sign that neither reading is an artefact.
        -- Carried over from the Unity: the { 0, +0.012, +0.009 } nudge dialled in by hand in VR. That number is not
        -- about this weapon but about which single in-hand relation a VR hand should wear (the game's hand re-grips
        -- the magazine on the way in, so the animation cannot answer it), and the magazine and the poses here are
        -- the Unity's exactly.
        holdOffset   = {  0.11473,  0.00336, -0.03866 },
        holdRotBasis = {  0.00713, -0.68978,  0.05714, -0.72173 },

        snapRadius     = 0.10,
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 2.6,

        -- The Unity's stages verbatim, and here that is not a transfer at all: same magazine mesh, same hold, same
        -- run length (113.7 mm measured here against the Unity's 115), so the depths line up as they are.
        poseStages = {
            { d = 0.115, pose = 'unity_mag_left_d115' },
            { d = 0.080, pose = 'unity_mag_left_d080' },
            { d = 0.050, pose = 'unity_mag_left_d050' },
            { d = 0.025, pose = 'unity_mag_left_d025' },
            { d = 0.000, pose = 'unity_mag_left_d000' },
        },
        pose           = 'unity_mag_left_d000',

        -- MEASURED on the non-empty take: 16 samples between 5 and 115 mm out, all within 3.4 deg of this line. The
        -- two empty takes cannot give it -- their magazine bone swings 400 mm across the room while it is carried,
        -- which reads as 170 deg of "deviation" and is not the insertion at all.
        wellAxis   = {  0.0082, -0.3256, -0.9455 },
        insertRun  = 0.114,
        seatAt     = 0.70,
    },
}
