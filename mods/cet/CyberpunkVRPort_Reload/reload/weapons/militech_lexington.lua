-- Militech Lexington — physical-reload configuration.
--
-- The seventh pistol, and the one that broke the most assumptions in the set:
--
--   * ITS FRAME RIG HAS 11 BONES, exactly like the Constitutional Unity's family, and is not that family at all --
--     different names in a different order, with no bone called `slide` anywhere. Its moving parts are `barrel_back`
--     (bone 3, the slide the hand racks: 108 mm of geometry) and `barrel_top` (bone 4, a small piece riding on top).
--     Registering it uncovered a real bug in the plugin: the signature table was keyed on (rig, bone count) alone, so
--     this weapon would have silently un-registered the four pistols that share its count. Fixed there.
--   * ITS MAGAZINE RIG IS THE SHARED ONE, base\weapons\firearms\handgun\rig\w_handgun__mag_std.rig, not one of its
--     own. Five bones as usual but in a different order: `magazine` is 2, mag_stdr 3, mag_std 4.
--   * THREE appearance packages carry the patch: the root, `__silenced`, and the part package -- 17 + 2 + 17
--     appearances between them.
--   * NO HAND EVER TOUCHES ITS SLIDE IN A RELOAD. Measured across all four takes: the left wrist never comes within
--     107 mm of the slide slot, and the slide simply runs off its catch. The pinch below therefore comes from
--     `first_equip`, which is the one animation in which a hand does rack it (read from the clips, confirmed).
return {
    id    = 'militech_lexington',
    match = 'militech_lexington',

    -- Its own bank, 55 events. There is no slide_pull in it, because the game never pulls this slide by hand -- but
    -- `safe_action`, the clip where BOTH parts travel 19.4 mm and return, plays `safe_handle_open` on the way back
    -- and `handle_close` on the way home. That is a rack and a slam, whatever the events are named after.
    -- (`mag_tap` also exists here, which nothing else in the set has: the palm slapping the base home.)
    sounds = {
        slidePull    = 'w_gun_pistol_power_lexington_safe_handle_open',
        slideRelease = 'w_gun_pistol_power_lexington_handle_close',
        magOut       = 'w_gun_pistol_power_lexington_mag_out',
        magIn        = 'w_gun_pistol_power_lexington_mag_in',
        magGrab      = 'w_gun_pistol_power_lexington_mag_new_grab',
        eject        = 'w_gun_revol_power_overture_bullets_out',   -- borrowed, as on all of them: no casing event
    },

    -- MEASURED: the magazine slot sits 14.9 mm off the bore line here, the widest of the set (the Liberty's is 1.3),
    -- but a lever arm of centimetres is still a poor axis when the weapon's own orientation is exact and free.
    basis = 'weapon',

    rigs = {
        { which = 1, bones = 11, names = { { 3, 'barrel_back' }, { 4, 'barrel_top' }, { 10, 'mag_slot' } } },
        { which = 0, bones = 5,  names = { { 2, 'magazine' }, { 4, 'mag_std' }, { 3, 'mag_stdr' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel_back' },

    slide = {
        ejectLiveRound = true,
        slot       = 'vrp_barrel_back',
        which      = 1, bone = 3,
        -- HOW FAR IT CAN GO, from its own rig animation rather than from one reload: empty_reload takes the slide to
        -- 52.6 mm behind the closed position, first_equip to 43.3, and the recorded takes to 44.7. The asset supports
        -- 52.6 and the reload uses most of it.
        travel     = 0.0526,
        -- Straight off the rig's bind pose, and confirmed live to 0.1 mm: barrel_back rests at local Z -45.5 mm.
        restLocal  = { 0.00010,  0.02460, -0.04550 },
        -- THE SLIDE STOP, measured in all three empty takes and steady to the tenth of a millimetre: the bone sits at
        -- -81.4 mm for the whole of an empty reload, i.e. 35.9 mm back, and `barrel_top` sits at -5.2 with it.
        lockLocal  = { 0.00010,  0.02460, -0.08140 },
        -- MEASURED: coming off the catch the game pulls it 8.8 mm deeper (-81.4 -> -90.2) before letting go. Its own
        -- animation asset goes 16.7 mm deeper, but the runtime blend is what the hand has to compete with.
        lockPull   = 0.009,
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- MEASURED off the three empty takes: 35.9 mm home in 0.06-0.07 s with a peak of 1.67 m/s, so 0.5-1.1 m/s
        -- depending on the take. The middle of that is what a spring looks like at 45 Hz.
        releaseSpeed = 0.80,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        ejectAt    = 0.55,
        styleAngleMax = 60.0,

        -- Its own ejection port with the smoke-and-sparks emitter blanked, cal_9mm.particle kept, as a descriptor
        -- named `vrp_eject_case` in all three appearance packages.
        ejectFx    = 'vrp_eject_case',

        -- THE PART ON TOP, and it is not on a fixed ratio. Measured frame by frame: it follows the slide 1:1 (23.2 mm
        -- against 23.1) until about 29.5 mm and then STOPS while the slide carries on to 52.6 -- a dragged part with
        -- its own stop. A ratio fitted to the deep end (0.56) would lag visibly through the whole first inch, so the
        -- rider carries `cap` instead. No delay either way: at home the two arrive within one frame of each other.
        riders = { { bone = 4, ratio = 1.000, cap = 0.0295 } },

        -- ONE GRIP, THE PINCH, and it is the only place in this weapon's animations where a hand is on the slide:
        -- `first_equip`. Its reloads never touch it.
        --
        -- FINGERS are exact -- taken from first_equip in the player GLB at t=1.967, where the weapon rig has the slide
        -- at its deepest, converted by the global (x, -z, y, w) export convention.
        --
        -- THE WRIST IS TRANSFERRED, not measured, and that is stated plainly because it is the one number here that no
        -- measurement of this weapon could give: a wrist cannot be read out of the animation files (the arm rig's
        -- WeaponRight frame relates to the runtime hand by an unknown mount rotation -- 36 cm of residual when it was
        -- fitted), and this pistol's reloads never put a hand there to record. What IS measured is the relation the
        -- pinch keeps to the slide's REAR END, and it holds across weapons: the Liberty's and the Unity's agreed to
        -- about a millimetre, and their rotations agree to 11.4 deg with the Kenshin's. So the Liberty's relation is
        -- decomposed in its own (bore, well, side) triad and rebuilt in this weapon's -- giving -43.2 mm along the
        -- bore, -66.3 along the well and +17.4 sideways from a rear end that sits 18.5 mm behind the slide slot.
        --
        -- If it sits wrong in VR, one recorded take while DRAWING the pistol settles it exactly, and that take is
        -- worth more than any further arithmetic here.
        grips = {
            -- ...plus 5 mm LEFT and 5 mm DOWN along the barrel, set by eye in VR. ADDED, not
            -- subtracted, and the distinction is worth keeping straight: unlike the
            -- magazine's `holdOffset`, which is the magazine's position relative to the hand, a slide grip's
            -- `off` IS the wrist's own position in the weapon's frame -- the target is slot + F*o1 + D*o2 + S*o3.
            behind = { off = { -0.0482, -0.0862,  0.0126 },
                       rot = {  0.0704, -0.0897,  0.6785,  0.7257 },
                       pose = 'lexington_slide_left_behind' },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,                  -- the shared rig's tracks are the same pair in the same order

        -- Its magazine's mesh is 27.6 x 82.5 x 135.6 mm with the origin on the top face, the end that enters the
        -- well -- the usual convention, so no `originOffset`.
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\militech_lexington\\entities\\meshes\\w_handgun__militech_lexington__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_lexington.ent',

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- MEASURED on the ordinary reload, rotation steady to 0.58 deg over the seating window.
        holdOffset   = {  0.13062, -0.02575, -0.05386 },
        holdRotBasis = {  0.03039, -0.76212, -0.01824, -0.64646 },

        snapRadius     = 0.10,
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 2.6,

        -- TWO WAYS OF CARRYING IT, one way of pushing it in -- and both halves of that are measured rather than
        -- chosen. Across the four takes the carry falls into two clusters: three of them agree to 1.6-7.8 deg of mean
        -- joint angle, the fourth is 32.8-34.1 deg away from all three (72.5 deg at the middle fingertip), which is a
        -- different hold and not noise. The game has three reload variants, so this is the animation's own doing.
        -- At the PUSH the same comparison gives 0.9-19.9 deg with no clustering at all, so the insertion is one
        -- ladder for both carries.
        --
        -- Depths follow the hand: measured, the fingers open between 114 and 87 mm out and stay open to the seat.
        -- AND THE TWO CARRIES PLACE THE MAGAZINE DIFFERENTLY, which is the point of having two: one holds it in a
        -- fist, the other flat on the palm, and a magazine cannot sit the same way in both. Each was set in the
        -- overlay tuner and printed from the game, and the log says which pose was on the hand at each print, so
        -- there is no guessing about which correction belongs to which:
        --     `carry`   (fist) += { -0.125, +0.093, +0.058 } m and -79.67 / -24.45 / -32.52 deg -> 166 mm and 81 deg
        --                        from the seated relation
        --     `carry_b` (palm) += {  0.000, +0.042,  0.000 } m, no rotation at all -> 42 mm and 0.4 deg
        -- The finger corrections from the same two prints are baked into the pose files themselves.
        --
        -- A HOLD DOES NOT CHANGE ON THE WAY IN. Each variant keeps ITS OWN placement through every stage, and only
        -- the fingers roll. The first cut rolled the placement towards the seated relation as the magazine went in,
        -- the way the Kenshin's hand slides down its magazine -- and on this pistol that reads as the magazine
        -- sliding backwards through the hand as you insert it (the magazine slid back along the palm by those 42 mm).
        -- The difference is what was tuned: on the Kenshin the roll IS the motion, here the two holds were set by eye
        -- as finished holds, and a hold that was judged right should not be walked away from.
        --
        -- Which is also why every stage carries off2/rot2: without them the palm variant would fall back to the
        -- fist's placement from the third stage on, which is the same jump by another route.
        --
        -- THE TWO VARIANTS DIFFER IN WHETHER THE HAND RE-GRIPS, and that is the whole of it:
        --
        --   * THE FIST slides down the magazine as it goes in, the Kenshin's motion exactly: the placement rolls from
        --     the fist to the hand's PALM-ON-THE-BASE relation while the fingers open, so the palm arrives under the
        --     base at the same moment the magazine does. 96 mm of slide along the magazine, which is what the game's
        --     own hand does.
        --
        --     WHERE the roll ends was measured, then moved by hand. Tracking the distance from
        --     the hold slot to the magazine's BASE through the whole push: 63-67 mm while it is carried, a minimum of
        --     43 mm at 78-88 mm of depth, 67-72 mm by the seat. So the game's own hand is on the base BEFORE the
        --     magazine is home and has left it again by the time it is, and ending the roll on the seating window
        --     left the hand a wrist's width behind the base (it was pushed home by the wrist, where the forearm already is).
        --
        --     First it was moved 24.3 mm STRAIGHT AT the base -- not along the magazine, which was tried and does
        --     nothing: sliding along its length takes 67.4 mm only down to 67.1, because the gap is sideways rather
        --     than lengthways. Then it was placed by eye from inside the headset: another 30 mm to the LEFT and
        --     30 mm back towards himself, read along the barrel. That is said about the WEAPON, so it is converted
        --     into the hold slot's frame by the weapon-to-slot rotation measured over the seating window -- and then
        --     SUBTRACTED, which is the half that went wrong first time round and shipped the hand 3 cm the other way:
        --     `off` is the magazine's position relative to the HAND, so moving the hand by d moves that number by -d.
        --     The 24.3 mm step above had the sign right; this one added instead of subtracting.
        --
        poseStages = {
            { d = 0.122, off = {  0.00562,  0.06725,  0.00414 }, rot = {  0.43701, -0.35123, -0.39559, -0.72744 },
                         pose  = 'lexington_mag_left_carry',
                         off2 = {  0.13062,  0.01625, -0.05386 }, rot2 = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose2 = 'lexington_mag_left_carry_b' },
            { d = 0.105, off = {  0.00562,  0.06725,  0.00414 }, rot = {  0.43701, -0.35123, -0.39559, -0.72744 },
                         pose  = 'lexington_mag_left_carry',
                         off2 = {  0.13062,  0.01625, -0.05386 }, rot2 = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose2 = 'lexington_mag_left_carry_b' },
            { d = 0.087, off = {  0.03894,  0.05551, -0.01712 }, rot = {  0.33837, -0.49303, -0.30352, -0.74182 },
                         pose  = 'lexington_mag_left_d078',      -- the fist starts sliding down it, fingers opening
                         off2 = {  0.11205,  0.00959, -0.05055 }, rot2 = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose2 = 'lexington_mag_left_d078' },
            { d = 0.040, off = {  0.10739,  0.01819, -0.06932 }, rot = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose  = 'lexington_mag_left_d040',      -- palm under the base, the slide-down complete
                         off2 = {  0.11205,  0.00959, -0.05055 }, rot2 = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose2 = 'lexington_mag_left_d040' },
            -- THE HAND IS LIFTED IN THE LAST STRETCH, by eye and in weapon-frame up (minus the measured well axis):
            -- the fist 30 mm at the seat and 20 at the stage before it, the palm 20 mm from the moment its fingers
            -- open (0.087 down).
            -- One conversion of `up` serves both variants: on these stages they hold the same SEAT rotation and
            -- differ only in position, so the hand is oriented the same way in each.
            { d = 0.000, off = {  0.09811,  0.01486, -0.06766 }, rot = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose  = 'lexington_mag_left_d000',      -- seated, and the preview
                         off2 = {  0.11205,  0.00959, -0.05055 }, rot2 = {  0.03039, -0.76212, -0.01824, -0.64646 },
                         pose2 = 'lexington_mag_left_d000' },
        },
        pose           = 'lexington_mag_left_d000',

        -- MEASURED on the ordinary reload: the final approach runs 77.8 mm along this line, worst deviation 5.5 deg.
        -- Shorter than the others (113-122 mm) because this magazine is fed in from closer.
        wellAxis   = { -0.0095, -0.2731, -0.9619 },
        -- MEASURED on the ordinary reload: the final approach runs 77.8 mm along this line, worst deviation 5.5 deg.
        insertRun  = 0.078,
        seatAt     = 0.70,
    },
}
