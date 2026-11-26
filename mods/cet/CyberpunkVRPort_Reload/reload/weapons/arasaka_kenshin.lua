-- Arasaka Kenshin — physical-reload configuration.
--
-- The first weapon in this set that is genuinely a different machine, and every difference is measured:
--
--   * ITS RIG IS 10 BONES, not the 11 the other four share. There is no `slider_down`, so `mag_slot` sits at index
--     7 instead of 8 -- the old signature would not have recognised it and the recording would have come back with
--     zeroed rig bones. Registered in reload/rigs.lua before the first take was made.
--   * IT HAS NO SLIDE STOP. Read off its own rig animation (w_handgun__arasaka_kenshin__base1.anims.glb), every
--     clip returns the handle home and none holds it back: empty_reload 98.6 -> 44.7 mm, recoil_shoulder -> 47.5,
--     safe_action -> 69.4, first_equip -> 32.0. So there is no `lockLocal` here, and the whole slide-stop machinery
--     stays switched off for this pistol.
--   * TWO PARTS MOVE, not one. `slide` is the REAR charging handle the hand pinches; `hammer` -- despite the name --
--     is the long part that rides over the top (its geometry runs 193 mm along the weapon, which no hammer does).
--     In empty_reload the handle travels 53.9 mm while that part travels 70.0, and it leads the handle by a frame or
--     two. That is a rider, exactly like the Silverhand's front slider.
--   * NO CASE. A tech pistol: its effect spawner has no `ejection_port` at all -- what it has is `charging`,
--     `charged`, `discharge` and a `reload`. So nothing is ejected and no `ejectFx` is set.
--   * ROUNDS COME FROM THE HANDLE, not from the magazine. With no stop there is nothing to hold the rounds back
--     against, so `chamberOnRack` does it explicitly: seating a magazine spends the reserve but loads nothing, and
--     the rounds go in when the handle has been worked. The rule for this weapon.
return {
    id    = 'arasaka_kenshin',
    match = 'arasaka_kenshin',

    -- Its own sound bank (pwa_w_handgun__arasaka_kenshin.anims, 45 events), and the names say what the part is:
    -- `handle_pull` / `handle_release` rather than slide_pull / slide_release. No casing sound exists, and none is
    -- wanted -- there is no case.
    sounds = {
        slidePull    = 'w_gun_pistol_tech_kenshin_handle_pull',
        slideRelease = 'w_gun_pistol_tech_kenshin_handle_release',
        magOut       = 'w_gun_pistol_tech_kenshin_mag_out',
        magIn        = 'w_gun_pistol_tech_kenshin_mag_in',
        magGrab      = 'w_gun_pistol_tech_kenshin_mag_new_grab',
    },

    -- The magazine slot is 11.0-14.8 mm off the bore line here -- better than the Liberty's 1.3 and the Unity's 5.8,
    -- but still centimetres of lever arm at best, and it would swing with the handle. The weapon entity's own
    -- orientation has neither problem.
    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 10, names = { { 5, 'slide' }, { 7, 'mag_slot' }, { 6, 'hammer' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_slide' },

    slide = {
        ejectLiveRound = false,
        slot       = 'vrp_slide',        -- the reference the grips are measured against; see the note below
        which      = 1, bone = 6,        -- the REAR charging handle -- the bone named `hammer`, settled by pushing it
                                         -- live and watching what moved: the part the pinch pulls
        -- WHICH BONE IS WHICH, and it was inverted here at first. Two parts move in this weapon's reload, and the one
        -- the hand pinches is `hammer` (bone 6), not `slide` (bone 5). Measured frame by frame on its own take:
        --
        --      t       bone 6        bone 5
        --      2.93    -26.1  moves  98.6   still     bone 6 leads
        --      2.97    -76.3         85.9   moves     bone 5 starts two frames later
        --      3.00    -94.1  back   67.1
        --      3.06    -92.0  held   44.8   back
        --      3.15    -24.6  home   50.3   still out  bone 6 is home first
        --      3.22    -24.6         97.7   home       bone 5 arrives three frames later
        --
        -- So bone 6 is the driven part and bone 5 rides it, delayed in both directions -- exactly as seen:
        -- behind the handle's slide a second slide follows with a delay, and comes back the same way.
        --
        -- 70.0 mm of travel from the rig animation's keyframes (the recorder's 67.4 is that span sampled at 45 Hz).
        travel     = 0.0700,
        restLocal  = { 0.00000, 0.02650, -0.02460 },
        -- NO `lockLocal`. Not an omission: this weapon has no empty-slide position, measured across every clip.
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- MEASURED off the DRIVEN part's release: -92.0 -> -66.4 -> -42.4 -> -24.6 mm, i.e. 67 mm in three frames of
        -- a 45 Hz take, about 1.0 m/s.
        releaseSpeed = 1.00,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 60.0,

        -- THE PART THAT RIDES OVER THE TOP is bone 5 -- confusingly the one called `slide`. It travels 53.9 mm to the
        -- handle's 70.0, hence the ratio, and it is LATE at both ends.
        --
        -- THE DELAY IS THE GAME'S OWN, taken off the rig animation's keyframes rather than tuned. Measured at three
        -- levels of the travel, and the answer is a constant shift at each of them:
        --
        --      level of travel      going out        coming home
        --      25 %                 +0.034 s         +0.067 s
        --      50 %                 +0.034 s         +0.067 s
        --      75 %                 +0.034 s         +0.068 s
        --
        -- A constant offset at every level is a pure TIME DELAY. A first-order follow was tried first and cannot be
        -- right at any time constant: it would lag more at 25 % than at 75 %. Hence delayOut/delayHome, and the two
        -- directions genuinely differ by a factor of two.
        riders = { { bone = 5, ratio = 0.770, delayOut = 0.034, delayHome = 0.067 } },

        -- ONE GRIP ONLY, and that is what the weapon has: a pinch on the rear handle. Measured from its own empty
        -- reload -- the left index tip closes to 25 mm of the handle's rear end and the thumb to 35, fingers either
        -- side of it, and the handle starts moving on the next frame. Window t=2.82..2.91, offset steady to 7.4 mm.
        --
        -- Worth writing down because it cost a wrong turn: measured against the SLIDE SLOT the same hand looks 22 cm
        -- away and "never touches the handle", because this slot sits in the MIDDLE of the slide body (its geometry
        -- runs -107.7..+108.0 mm about the bone) while the handle is at the rear end. The fingertips against that
        -- rear end are what showed the grip.
        grips = {
            behind = { off = { -0.0446, -0.2135,  0.0154 },
                       rot = { -0.1272,  0.0085, -0.6789, -0.7230 },
                       pose = 'kenshin_slide_left_behind' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\arasaka_kenshin\\entities\\meshes\\w_handgun__arasaka_kenshin__mag_std_01.mesh',

        entity     = 'base\\vrport\\vrp_mag_kenshin.ent',
        -- No `originOffset`: its magazine mesh has the origin on the top face, the end that enters the well
        -- (Z -134.2..+7.2 mm), the same convention as the Tamayura's and the Liberty's. Only the Nue's differs.
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- MEASURED, and this weapon has TWO measurements of it that differ by 2 cm on the third axis: the non-empty
        -- take gives { 0.11168, 0.00743, -0.06683 } (rotation steady to 0.42 deg) and the empty one
        -- { 0.11209, 0.01284, -0.04728 } (0.60 deg). They agree to 0.4 mm on the first axis, so it is one axis and
        -- one window that differ, not a bad reading.
        --
        -- THE HOLD, and this is the one on the whole set that measurement could not finish.
        --
        -- What a seating window measures is the MOUNT: once the magazine is in the well its orientation is the
        -- weapon's own, and on this pistol that reads as a magazine standing on its base on an open palm. The pose
        -- here is a FIST -- the magazine is gripped along its length, the way a lighter is -- and the orientation
        -- that goes with it exists only while the magazine is CARRIED, where the game turns it by the magazine rig's
        -- own bone. That bone cannot be composed into the weapon's frame from script: the magazine rig's root
        -- orientation is not recoverable, which was tried and rejected here by its own self-check (the composition
        -- missed the known seating value by 87 deg where it had to give zero, and its "rigid" window held to only
        -- 13.9 deg). The same wall is on record for the Silverhand -- 36 cm of residual, all 48 signed axis
        -- permutations worse.
        --
        -- So this pair was set by eye in the overlay tuner and printed from the game:
        --     base { 0.11168, 0.00743, -0.06683 } + { -0.110, -0.011, +0.095 }
        --     base rotation, then +12.10 deg about X, -110.93 about Y, -1.51 about Z, composed in that order
        -- Two independent signs that it is right rather than merely chosen: the result sits 28.4 mm from the hold
        -- slot where the carried frames of the recording put the magazine at ~30 mm, and it is turned 111.5 deg from
        -- the seated reading, inside the 93-122 deg the bone's own rotation suggested before its frame defeated us.
        holdOffset   = {  0.00168, -0.00357,  0.02817 },

        -- THE ROTATION IS MEASURED WHILE THE MAGAZINE IS CARRIED, not while it is seated, and that distinction is
        -- the whole of it. A seating window measures the MOUNT: once the magazine is in the well its orientation is
        -- the weapon's, and the hand has usually let go by then. While it is carried the game turns the magazine by
        -- its OWN rig bone (`magazine`, mag rig bone 1) -- up to 28 deg away from the seated value on this weapon --
        -- so the in-hand orientation is the mount's rotation composed with that bone's, and nothing else.
        --
        -- Measured over t=1.84..1.93 of the non-empty take, where that composition is constant to four decimal
        -- places -- a rigid hand-to-magazine relation, which is exactly what a hold is. It sits 93.4 deg from the
        -- seated reading, and that is the difference between a magazine standing on an open palm and one gripped
        -- along its length in a closed fist, which is what this weapon's pose does.
        -- Turned again in the overlay on top of the baked pair: -34.28 deg about X, +4.03 about Y, +165.38 about Z,
        -- composed in that order, and baked here so the sliders can go back to zero.
        holdRotBasis = {  0.07631, -0.29691, -0.91564, -0.26005 },

        snapRadius     = 0.10,
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 2.6,

        -- ROUNDS ONLY AFTER THE HANDLE IS WORKED. There is no stop to hold them back on this weapon, so the rule is
        -- stated here instead: the magazine spends the reserve when it seats and the gun stays empty until the
        -- handle has been pulled and let go.
        chamberOnRack = true,

        -- THE ROLL: a fist carrying it, an open palm pushing it home -- and the DEPTHS are remapped, which is the
        -- whole point of this block.
        --
        -- Measured on this weapon's own insertion, by how far the fingertips sit from the wrist: 90/84/74/77 mm while
        -- the magazine is between 122 and 59 mm out (a closed fist), then opening through 102, 128, 137, 143 as it
        -- goes 38, 28, 24, 22 mm in, and 146/150 -- flat palm -- at 10 and 3 mm.
        --
        -- Those last poses were unreachable as recorded. `seatAt` 0.70 clicks the magazine home at 30 % of the run
        -- still showing, i.e. at 37 mm of depth, so nothing below 37 was ever asked for and the hand stayed a fist
        -- all the way in. The pose FILES are the animation's; the depths below spread the same sequence over the band
        -- the player actually travels (122 down to the seat), so the palm arrives just as the magazine does.
        -- THE HAND SLIDES DOWN THE MAGAZINE AS IT GOES IN, which is what the animation does and what the weapon
        -- needs: gripped in a fist while carried, and by the time it is home the palm is under its base, pushing.
        -- That is a per-stage OFFSET and ROTATION, and both ends are measured:
        --   * the fist -- the pair set in the overlay and confirmed in VR, the magazine's origin 28 mm from the wrist;
        --   * the palm -- the SEATING window, 130 mm from the wrist, which IS the hand under the base at the moment
        --     of the push, and whose orientation is the well's because that is where the magazine already is.
        -- 132.1 deg and 146 mm between them, rolled over the 85 mm the hand travels; the 0.055 entry is a true slerp
        -- midpoint rather than a component-wise average, which would swing the magazine out through a wrong pose.
        --
        -- This was only possible after the depth was decoupled: `magS.depth` is now measured against the weapon's
        -- FIXED hold relation (see magFromRaw's `base`), so a stage can move the magazine without moving the ruler
        -- that chooses the stage. With them coupled this same table rang like a bell, and per-stage rotation dragged
        -- the wrist up and sideways because the magnet solves the wrist from the in-hand relation.
        --
        -- DEPTHS, set by eye in the headset rather than from the recording's own frames, and they follow `seatAt`: at
        -- 0.80 the magazine clicks home with 20 % of the run still out, i.e. at 24 mm of depth, so nothing below that
        -- is ever asked for.
        --
        -- THE CHANGE STARTS EARLY AND FINISHES QUICKLY: the fist lets go about a quarter of the way in (92 mm of 122)
        -- and the palm is under the base by 60 mm, then holds that the rest of the way. Both halves of this were
        -- tried the other way round first -- the whole roll squeezed into the last 20 mm read as the fist hanging on
        -- and then snatching, and spread evenly over the insertion it read as the hand changing its mind slowly.
        poseStages = {
            -- TWO WAYS OF CARRYING IT, and the module picks between them at random once per grab (`off2/rot2/pose2`).
            -- The second was dialled in by hand: the same recorded carry with per-joint weights and a 7.2 deg turn dialled
            -- in in the overlay and printed from the game. Only the CARRY stages have a variant -- by the time the
            -- palm is on the base both ways have to agree, because that end is the well's own geometry.
            { d = 0.122, off = {  0.00168, -0.00357,  0.02817 }, rot = {  0.07631, -0.29691, -0.91564, -0.26005 },
                         pose = 'kenshin_mag_left_d122',
                         off2 = {  0.01168,  0.00743,  0.04517 }, rot2 = {  0.03613, -0.34139, -0.90754, -0.24191 },
                         pose2 = 'kenshin_mag_left_d122_b' },   -- fist, carried
            { d = 0.092, off = {  0.00168, -0.00357,  0.02817 }, rot = {  0.07631, -0.29691, -0.91564, -0.26005 },
                         pose = 'kenshin_mag_left_d114',
                         off2 = {  0.01168,  0.00743,  0.04517 }, rot2 = {  0.03613, -0.34139, -0.90754, -0.24191 },
                         pose2 = 'kenshin_mag_left_d122_b' },   -- last of the fist, a quarter of the way in
            { d = 0.076, off = {  0.05668,  0.00193, -0.01933 }, rot = {  0.04853, -0.62925, -0.55324, -0.54370 },
                         pose = 'kenshin_mag_left_d022' },   -- half way down it, half turned (true slerp midpoint)
            { d = 0.060, off = {  0.11168,  0.00743, -0.06683 }, rot = {  0.00508, -0.75834, -0.01213, -0.65173 },
                         pose = 'kenshin_mag_left_d003' },   -- palm under the base already, two thirds of the way in
            { d = 0.018, off = {  0.11168,  0.00743, -0.06683 }, rot = {  0.00508, -0.75834, -0.01213, -0.65173 },
                         pose = 'kenshin_mag_left_d003' },   -- still the palm, pushing it the last of the way
            { d = 0.000, off = {  0.11168,  0.00743, -0.06683 }, rot = {  0.00508, -0.75834, -0.01213, -0.65173 },
                         pose = 'kenshin_mag_left_d003' },   -- seated, and the preview when reaching for it
        },
        pose           = 'kenshin_mag_left_d003',   -- the fallback and the reach-for-it preview: a palm

        -- MEASURED on the non-empty take: 13 samples between 5 and 120 mm out, all within 5.1 deg of this line.
        wellAxis   = { -0.0150, -0.3072, -0.9515 },
        insertRun  = 0.122,
        -- 0.85 for this pistol, settled by eye in the headset after 0.70 (too early), 0.90 (pushed visibly too far) and
        -- 0.80: it clicks in with 15 % of the run showing, 18 mm of depth. The others sit at 0.70 -- their magazines
        -- and wells are shorter, so the same fraction is a different distance in the hand.
        seatAt     = 0.85,
    },
}
