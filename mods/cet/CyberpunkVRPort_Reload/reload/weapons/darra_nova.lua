-- Darra Polytechnic DR5 Nova — physical-reload configuration.
--
-- A REVOLVER BY NAME ONLY: nothing on its frame moves in any reload (11 bones, `top_button` / `front_button` still
-- in all four takes), and what is swapped is the whole CYLINDER, a six-round block that comes out and goes in as one
-- piece -- a box magazine, so reload.lua's and not revolver.lua's. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/nova_*; the analysis in build/pistols-20261020).
--
--   * The cylinder goes in FROM THE LEFT, across the frame, turning the last 20-30 deg into place as it seats.
--   * Its mesh sits on `mag_slot` to 2.6 mm at the seat, so no origin correction.
--   * The `_b` takes carry it some other way, 60 cm from the hand slot -- not a hold; the `_a` pair is.
return {
    id    = 'darra_nova',
    match = 'darra_nova',

    -- pwa_w_revolver__darra_nova.anims
    sounds = {
        magOut       = 'w_gun_revol_power_nova_mag_out',
        magIn        = 'w_gun_revol_power_nova_mag_in',
        magGrab      = 'w_gun_revol_power_nova_mag_new_grab',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 11, names = { { 8, 'top_button' }, { 9, 'front_button' }, { 5, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0, -- showMagazine in the source rig, not its remapped pose-buffer slot
        mesh       = 'base\\weapons\\firearms\\revolver\\darra_nova\\entities\\meshes\\w_revolver__darra_nova__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_nova.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- 61.6 x 68.9 x 62.0 mm: a short drum, its length along its own Y (the bore, once seated)
        drop = {
            centre  = { 0.0, 0.0018, 0.0 },
            flat    = 0.0308,
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            arm     = 0.005,
            probe   = 0.0310,
            inertia = 0.00072,
        },

        -- SIX WAYS ROUND ITS OWN AXIS. A cylinder of six chambers is the same object every 60 deg about the bore, so
        -- the hand is given whichever of the six it is nearest (the Overture's loader does the same) and the magnet
        -- never has to turn the wrist more than 30 deg about it.
        rollSym    = { axis = { 0, 1, 0 }, n = 6 },

        -- THE HOLD, nova_one_a / nova_empty_a, rigid over 12 frames each (1.7-1.8 mm, 0.5-1.2 deg), the two agreeing
        -- to 0.1 mm.
        holdOffset   = { 0.03577, 0.01595, -0.02182 },
        holdRotBasis = { -0.24709, -0.02931, 0.25855, 0.93340 },

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

        -- IN FROM THE LEFT: the last 40 mm of nova_empty_a, 15 points (worst residual 13.6 mm -- the swing-in).
        wellAxis   = { -0.9721, 0.1103, -0.2068 },
        insertRun  = 0.080,
        seatAt     = 0.70,

        poseStages = {
            { d = 0.080, off = { 0.03577, 0.01595, -0.02182 }, rot = { -0.24709, -0.02931, 0.25855, 0.93340 },
                         pose = 'nova_mag_left_carry' },
            { d = 0.025, off = { 0.03577, 0.01595, -0.02182 }, rot = { -0.24709, -0.02931, 0.25855, 0.93340 },
                         pose = 'nova_mag_left_seat' },
        },
        pose           = 'nova_mag_left_seat',
    },
}
