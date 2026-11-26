-- Arasaka Tamayura — physical-reload configuration.
--
-- Measured from two recordings of the game's own reload with VRIK off (reload_record_EMPTY_TAMAYURA and
-- reload_record_NOTEMPTYTAMAYURA, 241 and 253 frames, every column present). Where a number could not be measured
-- it says so and says where it came from instead -- there is nothing here that was chosen by eye.
--
-- WHAT THIS PISTOL IS. It ships its own meshes but borrows the Tsunami Nue's animations and rig, and it has no .app
-- of its own: it is assembled by two appearances INSIDE the Nue's file, `tamayura_default` and `tamayura_2077`. The
-- collision host, the vrp_ slots and the case effect went into those two only; the Nue's own eight are untouched.
--
-- ITS RIG IS THE UNITY'S, bone for bone: 11 frame bones with the same names at the same indices (slide 5, hammer 6,
-- mag_slot 8) and the shared 5-bone magazine rig. So one signature covers both weapons, and -- measured -- the slide
-- bone even rests and locks at the same two places (85.9 / 45.9 mm on local Z).
--
-- WHAT THE ANIMATION DOES AND DOES NOT DO, straight off the two takes:
--   * a NORMAL reload never touches the slide: the bone reads 85.9 mm on all 253 frames.
--   * an EMPTY one does not RACK it either -- it presses the slide-release BUTTON. The left hand comes in from
--     0.160 to 0.129 m of the slide slot with its thumb reaching 42.5 mm, and on the very next frame the slide runs
--     home: 45.9 -> 56.6 -> 74.2 -> 85.1 -> 85.9 in four frames, peak 0.80 m/s.
--   * that press is a two-handed hold, so there is no free-hand gesture in it to reuse -- the port's own slide
--     release is the right stick click (vrshared::kRightStickClick), and racking by hand is the module's doing.
return {
    id    = 'arasaka_tamayura',
    match = 'arasaka_tamayura',

    -- SOUND, all of it this weapon's own: pwa_w_handgun__tsunami_nue.anims carries 52 sound events and every one
    -- this reload needs is among them (equip, grab, mag_in, mag_new_grab, mag_out, mag_rattle, melee_in, melee_out,
    -- move, slide_pull, slide_release, unequip).
    sounds = {
        slidePull    = 'w_gun_pistol_power_nue_slide_pull',
        slideRelease = 'w_gun_pistol_power_nue_slide_release',
        magOut       = 'w_gun_pistol_power_nue_mag_out',
        magIn        = 'w_gun_pistol_power_nue_mag_in',
        magGrab      = 'w_gun_pistol_power_nue_mag_new_grab',
        -- BORROWED and marked as such, exactly as on the other two: this set has no casing sound (searched for
        -- shell / casing / eject) and the ejection_port effect is particles only, no audio.
        eject        = 'w_gun_revol_power_overture_bullets_out',
    },

    -- The weapon's own frame, for the same reason as the Unity: this rig's slots all lie on the bore axis, so a
    -- slot-derived "down" is noise with a lever arm of millimetres, and it swings as the slide travels.
    basis = 'weapon',

    -- Rig signatures. Identical to the Unity's -- same asset family -- and re-registering the same pair just
    -- overwrites it, so listing them here costs nothing and keeps the config self-contained.
    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 11, names = { { 5, 'slide' }, { 8, 'mag_slot' }, { 6, 'hammer' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_slide' },

    slide = {
        ejectLiveRound = true,
        slot       = 'vrp_slide',
        which      = 1, bone = 5,        -- frame rig, `slide`
        -- MEASURED, and it is the CATCH travel: 40.0 mm, agreed by both channels -- the bone's own local Z
        -- (45.9 -> 85.9) and the slide slot's distance to the muzzle slot (158.0 -> 118.1 mm). How much further a
        -- hand could pull it is NOT in the data, because this pistol's animation never racks the slide, so the
        -- travel stops where the measurement does rather than being invented.
        travel     = 0.040,
        restLocal  = { 0.00000, 0.02160, 0.08590 },   -- closed, on all 253 frames of the normal reload
        lockLocal  = { 0.00000, 0.02160, 0.04590 },   -- on the stop, 40.0 mm back, for 140 frames of the empty one
        -- The lift that trips the catch. Not measured on this pistol -- its animation presses the button instead of
        -- lifting the slide -- so it is the Unity's measured 2.0 mm, the same rig and the same mechanism.
        lockPull   = 0.002,
        localAxis  = { 0, 0, -1 },       -- MEASURED: only local Z moves, and rearward is negative
        sign       = 1.0,
        -- MEASURED off the release: 40 mm in four frames, per-frame 0.49 / 0.80 / 0.50 m/s.
        releaseSpeed = 0.80,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        ejectAt    = 0.55,
        styleAngleMax = 60.0,

        -- The case as the game makes it: this appearance's `ejection_port` points at
        -- base\fx\weapons\firearms\pistols\tsunami_nue\w_pistol_nue_ejection_port.effect, which holds the same two
        -- emitters as the Unity's -- a smoke-and-sparks puff and cal_9mm.particle, a real cartridge case as a mesh
        -- particle. Shipped here with the puff blanked (base\vrport\vrp_nue_eject_case.effect) and a descriptor of
        -- this name added to both tamayura appearances: a racked slide throws a case, it does not burn powder.
        ejectFx    = 'vrp_eject_case',

        -- THE GRIPS ARE THE UNITY'S, and this is the one transfer in the file. They cannot be measured here: this
        -- pistol's reload never puts a hand on the slide (the empty one presses the button two-handed, the normal
        -- one leaves the slide alone), so there is no hold window to read. The transfer is not blind -- both are
        -- expressed in the WEAPON's frame relative to the slide slot, both weapons carry the same rig with the
        -- slide slot on the same bone at the same rest, and the poses are the same hand.
        --
        -- The known difference, measured: from the slide slot to the muzzle slot this pistol is 118.1 mm against
        -- the Unity's 104.6, so it is 13.5 mm longer ahead of the grip point. If the hand sits a centimetre off
        -- along the barrel, that is where it comes from -- nudge `off`'s second component, and the CET tuner is
        -- there for exactly that.
        grips = {
            overhand = { off = { -0.0783, -0.0755, -0.0077 },
                         rot = { -0.0659,  0.3211,  0.0193, -0.9445 },
                         pose = 'unity_slide_left_palm' },
            behind   = { off = { -0.0473, -0.1780, -0.0263 },
                         previewOff = { -0.00001, 0.00986, 0.02833 },
                         rot = {  0.2168, -0.2599,  0.6192,  0.7086 },
                         pose = 'unity_slide_left_behind' },
        },
    },

    -- THE MAGAZINE, all measured on this weapon.
    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,                  -- the rig documents it: trackNames = {showMagazine, showMagazineReload},
                                         -- referenceTracks = {1, 0}, so index 0 rests at 1 = shown

        -- BUILT FROM THIS PISTOL'S OWN magazine entity (tools/make_mag_entity.py on
        -- entities\w_handgun__arasaka_tamayura__mag_std_01.ent), never copied: WolvenKit does not re-serialise a
        -- .ent's compiled package, so a copied file drags the other weapon's meshes along as a stray shadow.
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\arasaka_tamayura\\entities\\meshes\\w_handgun__arasaka_tamayura__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_tamayura.ent',

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- WHERE THE GAME PUTS THE MAGAZINE IN THE HAND. From the empty take's seating window, t=2.55..2.75, ten
        -- frames, offset steady to 1.8 mm and rotation to 0.51 deg. The other take agrees on the offset to 0.16 mm
        -- ({0.10060, 0.02184, -0.03683}) from a window of its own, which is the cross-check.
        holdOffset   = { 0.10044,  0.02180, -0.03683 },
        -- The magazine's rotation in the hold slot's frame, from the MOUNT's own orientation. On this pistol the
        -- mount and the weapon agree to 0.1 deg -- so unlike the Unity there is no 180 deg to carry over, and no
        -- `seatRot` is needed.
        holdRotBasis = { -0.00498,  0.70312,  0.10594,  0.70312 },

        -- Behaviour in metres, the same as the other two: these are human-scale distances and the hand is the same.
        snapRadius     = 0.10,
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 2.6,

        -- THE HAND THROUGH THE INSERTION. Each stage IS a frame of the game's own approach rather than a round
        -- number: this insertion is ten frames long and its depths are 90, 80, 72, 67, 55, 36, 12, 2 mm, so asking
        -- for "20 mm" pulls in frames from outside the approach entirely (it did, once -- a 0 mm frame from before
        -- the swing). Five frames spread over the approach, plus the seated hold at the end.
        --
        -- THE FINGER POSES ARE THE UNITY'S, and the transfer is measured rather than assumed. This weapon's own
        -- recorded stages read wrong in VR for the same reason the Unity's did before they were shaped by hand: the
        -- animation's hand is mid-motion and RE-GRIPS, so its curls do not describe a magazine resting in a palm.
        -- The Unity's were tuned in the overlay until they did, and the two magazines are near enough for that work
        -- to carry -- 26.0 x 83.1 x 121.8 mm against 28.6 x 75.3 x 130.8 (the Unity's is the Constitutional
        -- Liberty's mesh, shared between guns): under a centimetre apart on every axis, 2.6 mm on the WIDTH, which
        -- is what a finger wrap actually follows, and both with their origin on the magazine's top face so a pose
        -- sits at the same place along the body.
        --
        -- The DEPTHS are this weapon's: each stage keeps the Unity's fraction of the run (1.00, 0.70, 0.43, 0.22, 0)
        -- applied to the 89.6 mm measured here instead of the Unity's 115. Placement (holdOffset / holdRotBasis)
        -- stays measured per weapon above -- only the shape of the hand is shared, which is the part that is about
        -- the hand rather than the gun.
        --
        -- If this ever needs tuning: the overlay tuner works on whatever weapon is in hand, but the poses named
        -- below are the UNITY's files, so bake the result into tamayura_-named copies and point these lines at them
        -- rather than editing a pose the other pistol is already using.
        poseStages = {
            { d = 0.090, pose = 'unity_mag_left_d115' },
            { d = 0.063, pose = 'unity_mag_left_d080' },
            { d = 0.039, pose = 'unity_mag_left_d050' },
            { d = 0.020, pose = 'unity_mag_left_d025' },
            { d = 0.000, pose = 'unity_mag_left_d000' },
        },
        pose           = 'unity_mag_left_d000',      -- the fallback when there is no depth to place a stage by

        -- THE WELL AS A LINE, off the game's own insertion: the magazine rig's `magazine` bone runs to its seat
        -- along one straight direction in the weapon's frame, every sample between 10 and 60 mm out within 2.9 deg
        -- of it. Practically the Unity's line (0.0002, -0.3285, -0.9445) -- same rig, same well -- but this is this
        -- pistol's own measurement.
        -- REFITTED over all four takes of this animation (two here, two of the Tsunami Nue, which drives the same
        -- rig with the same anims). The earlier value came from three frames of one take and was 3.7 deg away from
        -- the Nue's; the honest reading is that neither had the samples to settle it. Fitting only the frames
        -- between 5 and 90 mm out -- i.e. the straight run, not the carry swing that follows the bone 800 mm across
        -- the room -- the two NON-empty takes agree to 0.5 deg: (+0.0030, -0.3171, -0.9484) at 6.0 deg worst and
        -- (+0.0015, -0.3217, -0.9468) at 4.0. Their mean is below; the two empty takes are unusable here (70-80 deg
        -- worst) because their approach is four frames long.
        wellAxis   = {  0.00225, -0.31940, -0.94760 },  -- unit, pointing OUT of the well
        -- MEASURED: 89.6 mm here and 88.5 on the Nue's take of the same animation.
        insertRun  = 0.089,
        seatAt     = 0.70,                           -- catches at 70 % of the way in and lets the hand go
    },
}
