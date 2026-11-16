-- Techtronika Burya — physical-reload configuration.
--
-- A revolver whose cylinder is a four-round BLOCK (55.5 x 77.6 x 55.4 mm) swapped whole from the left -- reload.lua's,
-- like the Nova. Read off four takes with VRIK off (build/shotgun-reload-20261019/takes/burya_*; the analysis in
-- build/pistols-20261020).
--
--   * THE TOP OPENS BY ITSELF: `slider` (6), the upper the scope rides, lifts 11.8 mm and tips 2.8 deg as each reload
--     starts (`open`) and drops back as it ends (`close`), the hand never on it (its wrist is half a metre off). That is
--     a cover over the well, so it is a `hatch`, open whenever the block is out.
--   * The block comes in turned ~70 deg about its own axis and the hand turns it into place as it seats (t=2.60..2.69
--     of burya_empty_b): four chambers, so `rollSym` -- the magnet never turns that wrist more than 45 deg.
return {
    id    = 'techtronika_burya',
    match = 'techtronika_burya',

    -- pwa_w_revolver__techtronika_burya.anims
    sounds = {
        magOut       = 'w_gun_revol_tech_burya_mag_out',
        magIn        = 'w_gun_revol_tech_burya_mag_in',
        magGrab      = 'w_gun_revol_tech_burya_mag_thump',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 10, names = { { 6, 'slider' }, { 7, 'reload_button' }, { 8, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    -- THE TOP, parent-local as parts.py reads it off all three takes that show it (11.75-11.95 mm, 2.8-3.0 deg): up
    -- in 0.07 s, down in about as long.
    hatch = {
        which = 1, bone = 6,
        openTime  = 0.08,
        closeTime = 0.10,
        sounds = { open = 'w_gun_revol_tech_burya_open', shut = 'w_gun_revol_tech_burya_close' },
        parts = {
            { which = 1, bone = 6, move = { 0.0, 0.01175, 0.00129 }, turn = 2.8, axis = { 1.0, 0.014, 0.014 },
              from = 0.0, to = 1.0 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0, -- showMagazine in the source rig, not its remapped pose-buffer slot
        mesh       = 'base\\weapons\\firearms\\revolver\\techtronika_burya\\entities\\meshes\\w_revolver__techtronika_burya__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_burya.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        drop = {
            centre  = { 0.0, 0.0, 0.0 },
            flat    = 0.0277,
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            arm     = 0.011,
            probe   = 0.0277,
            inertia = 0.00076,
        },

        rollSym    = { axis = { 0, 1, 0 }, n = 4 },

        -- THE HOLD, all three takes that carry it in the hand slot (one_b, empty_a, empty_b): rigid over 17-20 frames
        -- (0.2-1.0 mm, 0.2-2.2 deg), within 0.7 mm and 0.1 deg of each other.
        holdOffset   = { 0.01797, -0.02714, -0.02026 },
        holdRotBasis = { -0.83668, -0.19540, 0.21675, 0.46347 },

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

        -- IN FROM THE LEFT and a little from above: the two empty takes' fits averaged (10 points / 8.4 mm and 30 / 17)
        wellAxis   = { -0.9935, -0.0155, 0.1132 },
        insertRun  = 0.070,
        seatAt     = 0.70,

        poseStages = {
            { d = 0.070, off = { 0.01797, -0.02714, -0.02026 }, rot = { -0.83668, -0.19540, 0.21675, 0.46347 },
                         pose = 'burya_mag_left_carry' },
            { d = 0.020, off = { 0.01797, -0.02714, -0.02026 }, rot = { -0.83668, -0.19540, 0.21675, 0.46347 },
                         pose = 'burya_mag_left_seat' },
        },
        pose           = 'burya_mag_left_seat',
    },
}
