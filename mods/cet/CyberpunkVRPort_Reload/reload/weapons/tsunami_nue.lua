-- Tsunami Nue — physical-reload configuration.
--
-- The cheapest weapon in the set so far, and honestly so: it shares its rig, its animations and its sound bank with
-- the Arasaka Tamayura (which is assembled from two appearances inside this weapon's own .app), so everything the
-- ANIMATION determines is already measured and carries over exactly. What does NOT carry over is anything the MESH
-- determines, and there is one of those -- see holdOffset.
--
-- Its own assets: the frame mesh (collision baked from its shadow: barrel 18 541, slide 13 065, slider_down 1 740
-- bytes, each mapped to its own bone and Kinematic) and the magazine mesh, which is its own rather than borrowed.
return {
    id    = 'tsunami_nue',
    -- Careful: the Tamayura's components are named `w_handgun__arasaka_tamayura__*` and live in THIS weapon's .app,
    -- so a match on 'tsunami_nue' cannot pick it up by accident -- the component names differ even though the file
    -- is shared. The two configs are matched by their own component names and never collide.
    match = 'tsunami_nue',

    -- Its own sound bank, the same events the Tamayura borrows from it (pwa_w_handgun__tsunami_nue.anims, 52 sound
    -- events).
    sounds = {
        slidePull    = 'w_gun_pistol_power_nue_slide_pull',
        slideRelease = 'w_gun_pistol_power_nue_slide_release',
        magOut       = 'w_gun_pistol_power_nue_mag_out',
        magIn        = 'w_gun_pistol_power_nue_mag_in',
        magGrab      = 'w_gun_pistol_power_nue_mag_new_grab',
        eject        = 'w_gun_revol_power_overture_bullets_out',   -- borrowed, as on the other three: no casing event
    },

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
        -- The same numbers as the Tamayura, and not by assumption: rest and lock are properties of the RIG's bind
        -- pose and of the animation that drives it, and both are this weapon's own (the Tamayura borrows them).
        -- Measured on those takes: closed 85.9 mm, on the stop 45.9, 40.0 mm apart, release peaking at 0.80 m/s.
        travel     = 0.040,
        restLocal  = { 0.00000, 0.02160, 0.08590 },
        lockLocal  = { 0.00000, 0.02160, 0.04590 },
        lockPull   = 0.002,
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        releaseSpeed = 0.80,
        gripSnap   = true,
        previewRadius = 0.05,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        ejectAt    = 0.55,
        styleAngleMax = 60.0,

        -- Its own `ejection_port` is base\fx\weapons\firearms\pistols\tsunami_nue\w_pistol_nue_ejection_port.effect
        -- -- the same file the Tamayura uses, so the powder-free copy already shipped covers both. A descriptor of
        -- this name is now in all eight of this weapon's appearances.
        ejectFx    = 'vrp_eject_case',

        -- The Unity's grips and poses, as on the Tamayura and for the same reason: this animation presses the slide
        -- release button two-handed instead of racking the slide, so there is no hold window in it to read a grip
        -- from. Same rig, same slide slot on the same bone at the same rest, so the numbers are addressable here.
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

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0,

        -- Built from this weapon's OWN magazine entity (tools/make_mag_entity.py on
        -- entities\w_handgun__tsunami_nue__mag_std_01.ent).
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\tsunami_nue\\entities\\meshes\\w_handgun__tsunami_nue__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_nue.ent',

        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- THE ONE NUMBER THAT DOES NOT CARRY OVER, and why.
        --
        -- The animation is shared, so where the game's HAND is relative to the mount is identical to the Tamayura's
        -- measured { 0.10044, 0.02180, -0.03683 }. But we place a spawned ENTITY there, and an entity is placed by
        -- its ORIGIN -- which is a property of the mesh, not of the animation. Measured off the two meshes'
        -- bounding boxes: the Tamayura's magazine runs Z -124.9..+5.9 mm, i.e. its origin sits on the TOP face (the
        -- end that enters the well, which is also where the mount is -- that coincidence is why placing the entity
        -- at the mount reads correctly on the other two pistols). This one runs -65.6..+65.6: its origin is in the
        -- MIDDLE of the body. Placed unchanged, half the magazine would sit inside the hand.
        --
        -- So the placement is corrected by the difference between the two top faces, 59.7 mm, taken along the
        -- magazine's own long axis (local Z on both) and rotated into the hold slot's frame by holdRotBasis:
        -- (0, 0, -0.05970) -> (-0.05897, -0.00931, -0.00067), on top of this weapon's OWN measured mount offset
        -- { 0.10042, 0.02180, -0.03684 } (its takes agree with the Tamayura's to 0.02 mm -- the animation is shared).
        --
        -- THE SIGN IS SETTLED BY OBSERVATION, both directions tried in VR. With the correction as below the magazine
        -- sits slightly INTO the fingers -- a small residual. Flipped, it hung about 12 cm above the hand, which is
        -- twice the 59.7 mm correction: the signature of a wrong sign rather than a wrong size. So the direction here
        -- is right and what is left is a centimetre or two, which the overlay tuner is for -- print the result and
        -- bake it into this line rather than leaving it on a slider.
        -- THE PURE MEASUREMENT, with nothing added: where the game's own hand holds the magazine relative to the
        -- mount, from this weapon's takes (they agree with the Tamayura's to 0.02 mm -- the animation is shared).
        -- Everything else this magazine needs is geometry, and geometry belongs in `originOffset` below.
        holdOffset   = {  0.10042,  0.02180, -0.03684 },
        -- THE ENTITY-ORIGIN CORRECTION, in the magazine's own axes, applied by the module to the hand AND to the
        -- seat alike. Every number in it is the difference between the two magazine meshes' bodies, measured off
        -- their bounding boxes: this mesh's origin sits in the middle of the body while the Tamayura's (and the
        -- Unity's) sits at the end that enters the well, which is where the mount is -- and the mount is what the
        -- whole placement chain is measured against.
        --
        --      axis      Tamayura            Nue                 centre difference
        --      X        -14.3..+14.3 mm     -13.9..+13.9 mm       0.0 mm
        --      Y        -57.6..+17.7        -39.2..+39.5        -20.1
        --      Z       -124.9.. +5.9        -65.6..+65.6        -59.5
        --
        -- Both parts were first seen as symptoms rather than derived: -59.5 mm as "the magazine goes 5-7 cm too deep
        -- into the well" and -20.1 mm as "it enters about 2 cm to the side". The Y one is also confirmed twice over,
        -- because the nudge dialled in by hand in the overlay -- { +0.003, -0.025, -0.001 } in the hold
        -- slot's frame -- is this same 20.1 mm rotated into that frame, (+0.00314, -0.01966, -0.00286), to within
        -- half a millimetre on two axes. So it was never a matter of taste, and it is not a slider any more.
        originOffset = {  0.00000, -0.02011, -0.05950 },
        holdRotBasis = { -0.00498,  0.70312,  0.10594,  0.70312 },

        snapRadius     = 0.10,
        seatSnapRadius = 0.055,
        armDist        = 0.12,
        snapGain       = 1.6,
        seatRadius     = 0.03,
        gravity        = 9.81,
        litterTime     = 3.0,
        holdWiden      = 2.2,
        insertGain     = 2.6,

        -- The Unity's tuned hand, at the Tamayura's depths: the magazines are 27.9 x 78.7 x 131.2 mm here against
        -- 28.6 x 75.3 x 130.8 there and 26.0 x 83.1 x 121.8 on the Unity -- within a centimetre of each other on
        -- every axis, and 0.7 mm apart on the WIDTH, which is what a finger wrap follows.
        poseStages = {
            { d = 0.090, pose = 'unity_mag_left_d115' },
            { d = 0.063, pose = 'unity_mag_left_d080' },
            { d = 0.039, pose = 'unity_mag_left_d050' },
            { d = 0.020, pose = 'unity_mag_left_d025' },
            { d = 0.000, pose = 'unity_mag_left_d000' },
        },
        pose           = 'unity_mag_left_d000',

        -- The well and the run are the animation's, so they are this weapon's own measurement carried straight over
        -- (the Tamayura's takes ARE takes of this animation on this rig).
        -- THE WELL, refitted over both weapons' takes (see the Tamayura's config for the same numbers and why).
        wellAxis   = {  0.00225, -0.31940, -0.94760 },
        insertRun  = 0.089,
        seatAt     = 0.70,
    },
}
