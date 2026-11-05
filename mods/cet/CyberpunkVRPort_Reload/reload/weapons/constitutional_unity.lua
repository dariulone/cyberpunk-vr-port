-- Constitutional Arms Unity — physical-reload configuration.
--
-- Everything here is measured, and every number says where it came from. Two recordings of the game's own
-- empty_reload with VRIK off (reload_record_PALM / reload_record_FINGERSBACK, 248 and 256 frames) were read by
-- tools/analyze_reload_record.py -- the same reader reproduces the Silverhand's shipped grips to 0.1 mm on the
-- barrel axis, which is how the basis convention is known to match what reload.lua rebuilds at runtime.
--
-- HOW THIS PISTOL DIFFERS FROM THE SILVERHAND, all of it read off the data rather than assumed:
--   * ONE slide bone (`slide`, frame rig bone 5) where the Silverhand has front_slider + back_slider.
--   * NO round in the rig at all. Its 11 frame bones are barrel_plug, barrel, muzzle_slot, fx_muzzle,
--     weapon_trigger, slide, hammer, slider_down, mag_slot, scope_slot, pos_ironsight -- not a bullet among them.
--     The case is thrown by the game's own EFFECT instead; see `ejectFx`.
--   * NO rotator disc, so no flourish.
--   * Its empty reload does not RACK the slide: the slide is already locked back, the hand pulls it a couple of
--     millimetres to trip the catch and it springs home on its own (measured, both takes agree frame for frame:
--     42.5 mm at rest on the catch, 44.5 at the pull, then 41.2 -> 27.4 -> 10.6 -> 0.0 in four frames).
--     In VR the player may still rack it by hand whenever he likes -- that is the module's doing, not the anim's.
return {
    id    = 'constitutional_unity',
    match = 'constitutional_unity',

    -- SOUND, the weapon's own: pwa_w_handgun__constitutional_unity.anims names thirteen events and this reload's
    -- are all there, with none borrowed from another gun (the Silverhand had to borrow a casing sound; this one
    -- needs none because its case comes with the effect). Timings in its empty_reload, for reference:
    -- mag_out 2%, mag_new_grab 17%, mag_rattle 27%, mag_in 39%, slide_pull 62%, slide_release 69%.
    sounds = {
        slidePull    = 'w_gun_pistol_power_unity_slide_pull',
        slideRelease = 'w_gun_pistol_power_unity_slide_release',
        magOut       = 'w_gun_pistol_power_unity_mag_out',
        magIn        = 'w_gun_pistol_power_unity_mag_in',
        magGrab      = 'w_gun_pistol_power_unity_mag_new_grab',
        -- No `slideRider`: there is no feeder part on this pistol to make a second, delayed noise, and inventing
        -- one would be an addition rather than the game's sound.
        -- BORROWED and marked as such, exactly as on the Silverhand: this pistol's thirteen events contain no
        -- casing sound (searched for shell / casing / eject -- none), and the `ejection_port` effect is particles
        -- and materials, no audio. The Overture's is a real casing event of the same family.
        eject        = 'w_gun_revol_power_overture_bullets_out',
    },

    -- WHICH FRAME A GRIP IS EXPRESSED IN. Not the usual slot-derived one: every slot on this pistol lies on the
    -- barrel axis, so there is no honest "down" to take from them. Measured live, the perpendicular residual from
    -- the slide slot is 5.8 mm to the magazine slot, 11.0 to the barrel, 7.7 to the hammer, 27.6 to the ejection
    -- port -- against the Silverhand's magazine slot, which is a real well locator centimetres below the bore.
    -- A basis built on 5.8 mm came out with "down" pointing UP (down.Zup +0.376, side.Zup +0.787) and swung as the
    -- slide travelled: the grip sat below the slide, the hand shook, and the wrist rolled over at the end of the
    -- rack. The weapon entity's own orientation has no lever arm at all and does not move with the slide.
    basis = 'weapon',

    -- Rig signatures. Also listed in reload/rigs.lua, because a rig must be identified BEFORE it can be recorded
    -- and a config is written from that recording -- re-registering the same pair just overwrites it.
    -- The magazine rig is the same five bones as the Silverhand's, with the same names at the same indices.
    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 11, names = { { 5, 'slide' }, { 8, 'mag_slot' }, { 6, 'hammer' } } },
    },

    -- The barrel direction, from two slots on the gun. `back` is the slide slot, so it RIDES the rack -- which is
    -- also what makes the recorded grips ride it (the wrist keeps a constant offset from this point while the
    -- slide travels, and that constancy is the test that finds the grip in the first place).
    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_slide' },

    slide = {
        slot       = 'vrp_slide',
        which      = 1, bone = 5,        -- frame rig, `slide`
        -- MEASURED: 44.5 mm and 44.0 mm from the slide slot's travel in the two takes, 44.7 / 44.2 from the rig
        -- bone's own local translation. Half the Silverhand's 90 mm.
        travel     = 0.045,
        -- THE TWO PLACES THE SLIDE BONE ACTUALLY SITS, measured off an empty reload (reload_record_PALM_EMPTY):
        -- closed for the whole of a normal reload, and 40.0 mm back along local -Z while the magazine is dry. The
        -- module writes these ABSOLUTELY, so it no longer matters what the animation is doing or in which state the
        -- bone was first seen -- which is what made the slide snap shut the moment a grip closed on an empty gun.
        restLocal  = { 0.00000, 0.02160, 0.08590 },
        lockLocal  = { 0.00000, 0.02160, 0.04590 },
        -- How far the slide can be lifted OFF the catch before it goes home. Not a fraction of the travel: on the
        -- stop the slide is already back, and the game's own empty reload lifts it exactly 2.0 mm (45.9 -> 43.9)
        -- before it runs forward. Past this the catch is tripped and the slide is released whether the grip is
        -- still held or not -- a slide stop does not wait for the hand.
        lockPull   = 0.002,
        localAxis  = { 0, 0, -1 },       -- MEASURED: the bone's local translation runs (0.00, 0.00, -1.00) rearward
        sign       = 1.0,
        -- MEASURED off the spring in both takes: 44.5 mm gone in four frames of a 45 Hz recording, peak 0.76 m/s.
        releaseSpeed = 0.80,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,               -- 20% of 45 mm = 9 mm: past any incidental hand drift, as on the Silverhand
        ejectAt    = 0.55,               -- fraction of travel at which the case is thrown
        styleAngleMax = 60.0,

        -- THE CASE, made the way the GAME makes it. This pistol has no round bone to move, so nothing can be
        -- animated out of the port -- and it does not need to be: the appearance carries an
        -- `entEffectSpawnerComponent` whose `ejection_port` descriptor points at
        -- base\fx\weapons\firearms\pistols\constitutionalarms_unity\w_pistol_unity_ejection_port.effect, and inside
        -- that sits base\fx\weapons\bullet_shells\cal_9mm.particle with cal_9mm.mesh -- a real cartridge case as a
        -- mesh particle, thrown by the game's own emitter from the gun's own port. Confirmed live: firing the
        -- effect by name through GameObjectEffectHelper.StartEffectEvent ejects a case.
        -- OUR OWN effect, not the game's: `ejection_port` fires two emitters, the case and a puff of smoke and
        -- sparks. That puff belongs to a SHOT -- racking a slide throws a case, it does not burn powder -- so the
        -- effect is copied with the puff's particle blanked and shipped as base\vrport\vrp_unity_eject_case.effect,
        -- with a descriptor of this name added to the weapon's own effect spawner. Blanked rather than deleted:
        -- taking the track item out left a dangling handle and the file would not rebuild.
        ejectFx    = 'vrp_eject_case',

        -- No `riders` and no `chamberRound`: there is no part that trails the slide and no round bone to fling.

        -- THE GRIPS, from the game's own empty_reload, expressed in the WEAPON's frame (see `basis` above). `off`
        -- is the free WRIST relative to the slide slot -- so it rides the rack -- and `rot` is the wrist's rotation
        -- in the same frame. Read at the moment the hand is on the slide, which for this pistol is the instant
        -- before the catch trips rather than a rack, so each window is 4 frames with a 6.8 / 8.8 mm spread.
        --
        -- The numbers below were measured in the slot basis and then rotated into the weapon's frame by the fixed
        -- rotation between the two (R = 0.53040, 0.47027, 0.52801, 0.46768, read live). That rotation is a property
        -- of the asset, so the conversion is exact: rebuilt from either frame the target lands on the same point to
        -- 0.00000 m -- checked, not assumed. In the weapon's own frame the shape is plain to read: local Y runs
        -- along the barrel (palm 7.8 cm back, pinch 17.8 cm back) and local X is the offset below the bore.
        grips = {
            overhand = { off = { -0.0783, -0.0755, -0.0077 },
                         rot = { -0.0659,  0.3211,  0.0193, -0.9445 },
                         pose = 'unity_slide_left_palm' },
            -- The pinch is HELD where it was recorded, but LOOKED FOR 3 cm higher: a hand coming from behind in
            -- VR arrives above the animation's wrist, so the preview was missing it. `previewOff` moves only the
            -- point the distance is measured to. "Up" is not guessed -- it is minus the measured well axis, the one
            -- direction on this pistol known to be perpendicular to the bore and pointing away from the magazine.
            behind   = { off = { -0.0473, -0.1780, -0.0263 },
                         previewOff = { -0.00001, 0.00986, 0.02833 },
                         rot = {  0.2168, -0.2599,  0.6192,  0.7086 },
                         pose = 'unity_slide_left_behind' },
        },
    },

    -- THE MAGAZINE. Same two mechanisms as the Silverhand: the seated one is hidden through its rig's
    -- `showMagazine` float track, the held one is a spawned entity, because the rig's magazine bone is parented to
    -- the gun and can never sit in a hand.
    mag = {
        enabled    = true,
        -- The mount, and it is a MOUNT rather than the magazine: measured live, this slot sits on the bore axis
        -- (5.8 mm of perpendicular from it), about ten centimetres above the magazine body. That is fine -- every
        -- number below is measured against this same point, so the chain closes -- but it is why the gun's basis
        -- cannot be built from it and why `basis = 'weapon'` exists above.
        -- (`vrp_mag_std` was added to the asset to get the magazine's own bone and it resolves to the same point:
        -- this slot component does not reach that bone on this pistol, whatever it does on the Silverhand.)
        slot       = 'vrp_mag_slot',
        which      = 0,                  -- the magazine rig: five bones, the same names and indices as the
                                         -- Silverhand's, which is why one signature covers both
        showTrack  = 0,

        -- BUILT FROM THIS PISTOL'S OWN magazine entity (tools/make_mag_entity.py), never copied from the
        -- Silverhand's: WolvenKit does not re-serialise a .ent's compiled package, so a copied file drags the
        -- other weapon's meshes along. Checked -- this one references no Malorian resource at all.
        -- Its magazine mesh is the Constitutional LIBERTY's, shared between the two guns; that is also why the
        -- Unity ships no magazine rig of its own.
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\constitutional_liberty\\entities\\meshes\\w_handgun__constitutional_liberty__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_unity.ent',

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- WHERE THE GAME PUTS A MAGAZINE IN THE HAND. Measured from reload_record_03, the first take whose columns
        -- make this exact: the recorder now stores the WEAPON's own quaternion and the mount slot's ORIENTATION, so
        -- neither is reconstructed from slot positions any more (on this pistol that reconstruction hung off a
        -- 5.8 mm lever). Window t=2.24..2.29, rotation steady to 1.09 deg, offset steady to ~2 mm over the ten
        -- frames around it. The offset agrees to 1.3 mm with the earlier reconstruction -- the position was never
        -- the problem.
        -- Measured { 0.11605, -0.00829, -0.04819 }, then nudged { 0, +0.012, +0.009 } in VR through the CET
        -- tuner. That nudge is the one number here the animation cannot give: the game's hand RE-GRIPS the
        -- magazine on the way in (its origin 4.3-6.7 cm from the hold slot while pushing, 12.5 cm at the frame it
        -- seats -- and that last one is already after the hand let go), so which single relation a VR hand should
        -- wear has to be chosen by eye. Chosen once, written down, and no longer a slider.
        holdOffset   = { 0.11605,  0.00371, -0.03919 },
        -- The magazine's rotation in the HOLD SLOT's frame, from the mount's own orientation.
        --
        -- Careful with WHICH mount: `vrp_mag_std` and `vrp_mag_slot` sit at the SAME point (0.00000 m apart,
        -- measured) but their rotations differ by exactly 180 deg about (0, -0.7071, -0.7071). The recorder took
        -- the first name on its list, `vrp_mag_std`, while the module and the placement convention use
        -- `vrp_mag_slot` -- and that mismatch, not the asset, is where a phantom 180 deg came from. The value below
        -- is the recorded one carried over by that measured difference.
        --
        -- Against `vrp_mag_slot` the seated magazine faces exactly as the WEAPON does -- conj(weapon) * mount is
        -- {0, 0, 0, 1} to five decimals -- so this pistol needs no `seatRot` after all, the same as the Silverhand.
        -- Confirmed in play: a copy of the magazine entity spawned at this slot's transform sits exactly on the
        -- gun's own magazine, in place and the right way round.
        holdRotBasis = { -0.00630, 0.69303, -0.05530, 0.71877 },

        -- Behaviour, not geometry: the same values the Silverhand uses, because they are human-scale distances in
        -- metres and the hand is the same hand.
        -- MEASURED, not chosen. With the hand laid on the seated magazine the way a player reaches for it, the
        -- raw hand reads (-0.0672, -0.0520, -0.1175) from the mount in the weapon's frame while the game's own
        -- wrist at the insertion is at (-0.0064, -0.0911, -0.1256): 7.3 cm apart. The Silverhand's 5 cm catch can
        -- therefore never fire here -- the target is correct (the module reproduces the recorded wrist to the
        -- millimetre), it simply sits further from where a hand naturally goes on this pistol. 10 cm clears the
        -- measured gap with margin, and `snapGain` keeps the last 6 cm at full pull so it still ends in a snap.
        snapRadius     = 0.10,
        -- The magnet's SIDEWAYS catch -- how far off the well's centreline the magazine may be and still be drawn
        -- onto it. This is not the old distance-to-seat radius any more: the pull is perpendicular only, so the
        -- number can be small without making the well hard to find, and small is what keeps a hand that merely
        -- passes near the gun from being grabbed at.
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        -- TWO POSES, the way the animation has them: fingers around the magazine while it is carried, palm on
        -- its base while it is pushed home. The module switches as the magazine enters the well.
        -- THE HAND THROUGH THE INSERTION, sampled from the take at five depths and interpolated between them at
        -- runtime by the magazine's own depth. Not two grips switched over: the animation rolls from fingers
        -- around the magazine to a palm on its base, and this is that roll. Depth is measured OUT of the well, so
        -- the first entry is what the hand wears carrying it in the air and the last is the seated pose -- which
        -- is also what the preview shows when reaching for a magazine still in the gun.
        -- THE STAGES CARRY FINGERS ONLY, and the magazine keeps ONE place in the hand.
        --
        -- Per-stage placement was tried and measured: the game's hand really does re-grip, the magazine's origin
        -- sitting 4.3-6.7 cm from the hold slot while it is pushed in and 12.5 cm at the frame it seats. But that
        -- 12.5 cm is measured AFTER the hand lets go -- the distance jumps from 6 to 12.5 cm between two frames at
        -- t=2.20 -- so it is not an in-hand relation at all, and the honest in-hand ones put the magazine through
        -- the wrist when they are worn in VR. Placement stays the single value that reads correctly on the palm;
        -- only the fingers roll through the stages.
        poseStages = {
            { d = 0.115, pose = 'unity_mag_left_d115' },
            { d = 0.080, pose = 'unity_mag_left_d080' },
            { d = 0.050, pose = 'unity_mag_left_d050' },
            { d = 0.025, pose = 'unity_mag_left_d025' },
            { d = 0.000, pose = 'unity_mag_left_d000' },
        },

        -- The magnet keeps hold once it has caught: the catch stays narrow, the RELEASE is this much wider, and
        -- the pull saturates sooner. A weight that fades to nothing at the edge let a small sideways move start a
        -- slide out of the well -- less pull, more drift, less pull again.
        holdWiden  = 2.2,
        insertGain = 2.6,
        pose           = 'unity_mag_left_d000',     -- the fallback when there is no depth to place a stage by

        -- THE WELL, as a line rather than a point. Measured off the game's own insertion: the magazine rig's
        -- `magazine` bone runs 115 mm to its seat along one straight direction in the weapon's frame, and every
        -- sample between 10 and 60 mm out lies within 1.6 deg of it.
        wellAxis   = { 0.0002, -0.3285, -0.9445 },  -- unit, pointing OUT of the well
        insertRun  = 0.115,                          -- m the magazine travels, from the animation
        seatAt     = 0.70,                           -- catches at 70 % of the way in and lets the hand go
                                                     -- (was 0.80; ten per cent shallower by eye -- 80 %
                                                     -- meant pushing the magazine visibly too far into the well)
    },
}
