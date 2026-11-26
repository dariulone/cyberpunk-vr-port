-- Malorian Arms 3516 "Silverhand" — physical-reload configuration.
--
-- Every value here is grounded in data, not guessed:
--   * bone indices are the weapon's OWN rig bone indices (the plugin's VRRigWrite maps them to the shared
--     pose-buffer slot through the live remap; see memory weapon-rig-identification).
--   * which parts move, along which sense, and how far come from decoding the game's own reload animations
--     (memory reload-animation-mechanics): the magazine drops, a new one comes in, a round chambers, and the
--     slide + hammer move ONLY on empty_reload. Physically we let the player do any of it any time.
--
-- `match` is tested as a lowercase substring of the weapon's appearance / mesh name.
return {
    id    = 'malorian_silverhand',
    match = 'malorian_silverhand',

    -- SOUND, taken from the weapon's OWN animations: pwa_/pma_w_handgun__malorian_silverhand.anims carry 52
    -- `animAnimEvent_Sound` entries, and five of them are exactly this reload's moments. So the hand-driven reload
    -- sounds like the game's, because it is the game's audio, played on the weapon's own entity.
    sounds = {
        slidePull    = 'w_gun_pistol_power_silverhand_handle_pull',
        slideRelease = 'w_gun_pistol_power_silverhand_handle_release',   -- the spring home, chambering with it
        magOut       = 'w_gun_pistol_power_silverhand_mag_out',
        magIn        = 'w_gun_pistol_power_silverhand_mag_in',           -- the click
        magGrab      = 'w_gun_pistol_power_silverhand_mag_new_grab',     -- a fresh magazine into the hand
        -- The FEEDER (bullet_pull, the part that trails the slide with a delay). The pistol has NO event of its own for
        -- it -- its whole reload set is seven events (swing, mag_out, mag_new_grab, mag_in, handle_pull,
        -- handle_release, swing_short) and the delayed metal you hear in the original is `handle_release`, 0.43 s after
        -- the pull, which this module already plays when the slide is let go. So this one is an ADDITION, not a
        -- restoration, and it uses the pistol's own `swing_short` -- the last sound of its reload -- rather than the
        -- Overture's `bullets_in` it borrowed at first.
        slideRider   = 'w_gun_pistol_power_silverhand_swing_short',
        -- BORROWED, and marked as such: this pistol's animations carry no casing sound at all (searched them for
        -- shell / casing / eject -- nothing), so the flying round uses the Overture revolver's own casing event, which
        -- is a real event of the same family. Clear it or name another if it does not fit.
        eject        = 'w_gun_revol_power_overture_bullets_out',
    },

    -- HOW THE PLUGIN RECOGNISES THIS WEAPON'S RIGS. A pass belongs to a rig when the bone COUNT matches and the
    -- named bones sit at those indices -- structural, and the same test that was hard-coded in the hook before any
    -- second weapon could exist. `which` 0 is the magazine rig, 1 the frame rig; indices are rig bone indices, and
    -- the plugin hashes the names itself. Copy this block per weapon and change the numbers.
    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 16, names = { { 5, 'front_slider' }, { 6, 'back_slider' }, { 8, 'mag_slot' } } },
    },

    -- Two slots along the barrel give the barrel direction in model space; the scalar projection of the free
    -- hand's travel onto it is exactly the local slide/charge distance (a rotation preserves length), so no bone
    -- local frame has to be reconstructed. front is toward the muzzle.
    barrel = { front = 'vrp_front_slider', back = 'vrp_back_slider' },

    -- THE SLIDE. Racks along the barrel. In the animation this is back_slider (frame rig bone 6), ~100 mm of
    -- travel, and it only moves on empty_reload -- but physically the player may rack it whenever, and doing so
    -- with a live round chambered ejects it (handled by the reload FSM + redscript, not here).
    slide = {
        slot       = 'vrp_back_slider',  -- world locator; the grip targets and the rack anchor ride it
        which      = 1, bone = 6,        -- frame rig, back_slider
        travel     = 0.09,               -- m of rack (anim plateau ~80 mm, peak 86.6)
        localAxis  = { 0, 0, -1 },       -- bone-local rack direction (-Z along the barrel) -- CONFIRMED live
        sign       = 1.0,
        releaseSpeed = 1.5,              -- m/s the slide springs forward on release (anim measures ~1.3)
        gripSnap   = true,
        previewRadius = 0.05,            -- m: within this of a grip's recorded wrist point the finger-pose
                                         --    PREVIEW fades in, and gripping there latches the grab (no teleport)
        blendTime  = 0.15,               -- s the finger pose fades in/out (100-200 ms)
        -- WHERE ALONG THE RACK things fire, as a fraction of travel.
        -- `pullSndAt` is the "the slide really travelled" line: it gates BOTH the slide's sound and the rotator disc's
        -- flourish, because a grip that never moves the slide still counts as a grab and was setting off both every
        -- single time. 20 % of 90 mm = 18 mm, past any incidental hand drift.
        -- `riderSndAt` sits late, where the feeder (bullet_pull) actually catches up with the slide.
        pullSndAt  = 0.20,
        riderSndAt = 0.72,
        styleAngleMax = 60.0,            -- deg: the hand's orientation must be this close to a grip's recorded
                                         --    orientation to count as that style (pinch = hand along the barrel)

        -- Parts that ride the rack. `vec` is the bone-local offset PER METRE of rack (from empty_reload: slide
        -- travels 100 mm while front_slider gives 20 back; the round gives 66 back + 19 up, and it CATCHES UP
        -- with a delay -- the slide is fully open ~0.1 s before the round moves, hence `lag`). Both bullet (the
        -- visible round) and bullet_pull (its helper) get the same path. rotator and ammo_mover are CHILDREN of
        -- back_slider in the rig, so they ride through the hierarchy for free and are not listed.
        riders = {
            { bone = 5, vec = { 0, 0, -0.20 } },                  -- front_slider: front sleeve, instant, 20/100
            -- bullet_pull(9): the little part that trails the slide with a visible DELAY (spotted in
            -- the game's anim: "the round and some part get pulled along, delayed"). Same path the round takes
            -- when chambering, but it only follows the rack -- it never flies.
            { bone = 9, vec = { 0, 0.19, -0.66 }, lag = 0.15 },
        },

        -- THE ROUND. Pushing the three round bones apart LIVE (each a different way, while watched in the headset)
        -- settled what no anim reading could: ALL THREE carry a visible mesh and they sit on top of each other
        -- at rest -- bullet(10) is bullet-tipped, bullet_reload(11) is a spent casing, bullet_pull(9) is the
        -- third. That pile is why every "the wrong round flew out" round-trip disagreed: whichever one was
        -- driven, the others stayed put and read as "the round still sitting there".
        -- So they are driven as ONE round: same axis, same distance, so their pile stays a pile.
        -- flingVec is the ARC added on top of the fully-racked pose (side, up, back per metre): mostly UP, so the
        -- ejected round climbs clear of the port instead of disappearing under the slide.
        -- bullet_pull(9) is deliberately NOT in the set: driven, it flew out as a visible SECOND object beside
        -- the round. It stays home, where it is buried in the geometry.
        -- The round SITS still while the slide cracks open (see ejectAt) -- rideVec is only the chambering path,
        -- from the recoil anim's own numbers (back 0.66, up 0.19), and the base of nothing else.
        -- The eject is a real ballistic throw, not a slide along one line: an initial velocity in the gun's own
        -- frame (up / out the port / a little back) plus GRAVITY resolved into that frame each frame, so the round
        -- falls truly downward however the gun is tilted, and it TUMBLES while it flies.
        chamberRound = { bones = { 10, 11 }, rideVec = { 0, 0.19, -0.66 }, chaseLag = 0.08,
                         flingTime = 0.45,            -- s of visible flight before the round is dropped
                         v0 = { side = 1.1, up = 1.9, back = 0.5 },   -- m/s in the gun's frame
                         gravity = 9.81,              -- m/s^2, applied along the world's real down
                         spin = 900.0 },              -- deg/s of tumble about the round's cross axis
        ejectAt = 0.55,                  -- fraction of travel where the round FLIES. Below it the round just sits
                                         -- there: crack the slide open, look at the round, pull further and it
                                         -- goes -- the way a real gun behaves.

        -- THE GRIPS, one per style, RECORDED from the game's own empty_reload with VRIK off (reload_record v2).
        -- `off` = the free WRIST relative to the back_slider SLOT in the gun's rigid basis [barrel-forward,
        -- mag-well-down, side = fwd x down]; measured spread across the hold was 0.2 mm (behind) / 0.1 mm (palm),
        -- so this IS the game's placement -- no conversions, no tuning. `rot` = wrist rotation in the same basis
        -- (kept for a future wrist-orientation force; position-only today). `pose` = recorded finger locals.
        grips = {
            behind   = { off = { -0.1974, -0.0023, 0.0484 },
                         rot = { -0.6992,  0.0391, 0.0608, -0.7113 },
                         pose = 'malorian_slide_left_behind' },
            overhand = { off = { -0.1060,  0.0055, 0.0862 },
                         rot = {  0.4441,  0.4122, -0.6623,  0.4408 },
                         pose = 'malorian_slide_left_palm' },
        },
    },

    -- THE ROTATOR DISC. Spins ~180 deg once the slide comes home (from the reload animation, rotator = frame rig
    -- bone 13, rotation about local Y in the GLB). The runtime spin axis is CALIBRATED live.
    rotator = {
        which    = 1, bone = 13,
        spinAxis = { 0, 0, 1 },          -- local Z (barrel axis) -- CONFIRMED live: spins the disc in its plane
        spinAngle = 180.0,
        spinTime  = 0.30,                -- s for the there-and-back flourish
    },

    -- THE MAGAZINE. Two mechanisms, each the one the game itself uses:
    --   * the magazine IN THE GUN is hidden and shown through its rig's `showMagazine` float track (see showTrack);
    --   * the magazine IN THE HAND is a spawned ENTITY, because the rig's magazine bone is parented to the gun and
    --     therefore follows the gun however it is written -- and its mount frame is not recoverable from script (a
    --     least-squares fit over a recorded reload left 36 cm of residual, and all 48 signed axis permutations were
    --     worse). Do not retry bone-driving a held magazine.
    mag = {
        enabled    = true,
        chamberOnRack = 'dry',          -- the empty reload needs a slide stroke; a tactical swap keeps its chamber
        slot       = 'vrp_mag_std',      -- world locator of the well (measured: this slot does NOT ride the bone)
        which      = 0,                  -- the MAGAZINE rig (5 bones: mag_plug, magazine, magazine_reload, mag_std,
                                         -- mag_stdr). No bone of it is written any more -- see showTrack.
        -- VISIBILITY THROUGH THE GAME'S OWN SWITCH. The magazine mesh in the gun is drawn with
        -- `visibilityAnimationParam = showMagazine`, and that is a FLOAT TRACK of this rig: trackNames is
        -- ['showMagazine', 'showMagazineReload'] with referenceTracks [1, 0], so index 0 hides/shows the seated
        -- magazine and it rests at 1. Writing 0 there is exactly what the game's reload animation does.
        -- This is why the flow now touches NO BONE at all: holding a bone every frame pinned the game's magazine
        -- animation and a native reload then ejected nothing (which is what `idleFree` used to work around).
        showTrack  = 0,
        -- THE MAGNET, measured from the wrist to the pose that holds the magazine where it sits in the gun (so 0 means
        -- "your hand is exactly where the game's hand is when it holds this magazine"). Deliberately small: it bites
        -- only right at the magazine, the same 5 cm the slide's grip preview uses. Inside it the pull is weighted by
        -- distance -- nothing at the edge, full at the pose -- so nothing ever jumps, and gripping there takes the
        -- magazine out exactly as it sat.
        snapRadius = 0.05,
        -- THE WELL'S catch, and it is measured on the MAGAZINE, not the wrist: how far the magazine is from its seat.
        -- Wider than the grab's on purpose -- the player aims a visible magazine at a visible well, and his wrist can
        -- be 10 cm off the exact insertion pose while the magazine already looks nearly home.
        seatSnapRadius = 0.12,
        -- How far the magazine must LEAVE the well before the well is allowed to pull it back. Without it the magnet
        -- is at full strength the instant the magazine is gripped -- it is still in its seat then -- so the magazine
        -- could not be pulled out at all and re-seated immediately.
        armDist    = 0.12,
        -- Saturates the pull, so the last `seatSnapRadius / snapGain` (7.5 cm here) is FULL magnet and the approach
        -- ends in a snap instead of fading in linearly -- a plain ramp is weakest exactly where it should be firmest.
        snapGain   = 1.6,
        seatRadius = 0.03,               -- m from the seat, on the DRAWN magazine, at which it clicks in
        -- HELD AS A SPAWNED ENTITY, because the rig bone is parented to the gun and can never sit in a hand (see
        -- the magazine block). This template is the game's own magazine stripped to a plain mesh component, which
        -- is what makes it visible when spawned on its own -- the skinned original showed only its shadow.
        -- WHAT THE CARRIER DRAWS. The same mesh the spawned entity used, so nothing about the magazine
        -- changes on screen -- only who carries it. Read out of this weapon's own vrp_mag_*.ent rather
        -- than chosen; a weapon without this line keeps the entity route.
        mesh       = 'base\\weapons\\firearms\\handgun\\malorian_silverhand\\entities\\meshes\\w_handgun__malorian_silverhand__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_malorian.ent',
        -- THE HOLD SLOT the magazine rides: the game's own attachment slot for a held item, read through the player's
        -- ItemAttachmentSlots component. Measured live, `WeaponLeft` is bone 26 to 0.000 m with quaternion agreement
        -- 1.0000, so this is the same point the skeleton offers -- taken from the game rather than reconstructed.
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',
        -- THE MAGAZINE'S PLACE IN THE HAND -- a CONSTANT in the hold slot's frame, so every grab puts it in the same
        -- place whatever the wrist is doing and wherever the gun points. Nothing about the hand's relation to the gun
        -- enters it; snapshotting the weapon's orientation at the grab was the bug that made each grab differ.
        --
        -- Both values are the GAME's OWN in-hand placement, measured from the frames where its hand seats a magazine
        -- (reload_record_RELOADTRUE, t = 2.33..2.51). The window is the data's, not a pick: across it the magazine
        -- holds 0.0580 m from the slot, the offset steady to 0.3 mm and the rotation to 0.32 deg, while from t = 2.53
        -- the hand leaves (0.058 -> 0.205 m, 6 -> 74 deg). At that instant the magazine's pose is KNOWN: position is
        -- the mag-well slot's, orientation is the weapon's own -- the chain weapon -> barrel -> mag_slot -> magazine
        -- -> the mesh's rig matrix is four times the same quaternion (0, .7071, .7071, 0), a 180 deg turn, so the four
        -- cancel in pairs. (An extra factor of it, added by hand, was the earlier "points sideways with its bottom".)
        holdOffset = { 0.04393, -0.01952, -0.03251 },
        -- The rotation is stored against the gun's rigid BASIS rather than the weapon entity, because a recording has
        -- three slots and no weapon quaternion. The module multiplies in R = conj(qBasis) * qWeapon at runtime -- a
        -- fixed property of the asset, the same in model or world space -- so the product stays constant.
        holdRotBasis = { 0.44601, -0.47092, 0.58768, 0.48369 },
        gravity    = 9.81,                  -- m/s^2 for a dropped magazine (world -Z; no bounce)
        litterTime = 3.0,                   -- s a dropped magazine lies around before it is cleaned up
        pose       = 'malorian_mag_left',   -- recorded finger pose for holding a magazine
        -- ---- MEASURED, but NOT read by the current flow. Kept because each cost a measurement, and the next
        -- ---- iteration (a native slot attachment) may want them; nothing below is live.
        -- The in-hand placement computed from the frame where a recorded insertion SEATS the magazine (t=2.33 of
        -- reload_record_RELOADTRUE) -- the only instant its world pose is known: position = the mag slot's,
        -- orientation = the gun's. Anchored the way the game's insertion looks, the magazine's BOTTOM on the PALM;
        -- the bottom is 60.3 mm below the mesh origin along local Z (bbox 0 .. -0.0603, body 32 x 70 x 60 mm).
        -- Superseded by the live grab snapshot, which needs no offline convention to be right.
        refHandOffset = { 0.12243, -0.04151, 0.02156 },
        refHandQuat   = { -0.25724, -0.52391, 0.24768, -0.77331 },
        -- The mount bone's local axes for each gun direction: rest offset reads (0,0,-37) mm live == the GLB's
        -- (0,-38,0) through (x,-z,y), so local -Z is DOWN; driving -Y went BACKWARDS, so +Y is FORWARD.
        refAxes = { fwd = { 0, 1, 0 }, down = { 0, 0, -1 }, side = { 1, 0, 0 } },
        refPath = 'malorian_mag',           -- the recorded bone path (reload/paths/malorian_mag.lua)
        refBones = { seated = 1, carried = 2 },   -- `magazine` / `magazine_reload`; two SEPARATE animated components
                                                 -- draw them, and the plugin latches one mag rig at a time
    },
}
