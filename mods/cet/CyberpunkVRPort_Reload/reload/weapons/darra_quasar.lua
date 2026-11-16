-- Darra Polytechnic Quasar — physical-reload configuration.
--
-- Another revolver by name: what it reloads is a sixteen-round DISC (89.8 x 89.8 x 20 mm) that goes into the back of
-- the frame from the left, a box magazine in all but shape -- reload.lua's. Read off four takes with VRIK off
-- (build/shotgun-reload-20261019/takes/quasar_*; the analysis in build/pistols-20261020).
--
--   * Nothing on the frame is worked: `safe_lock` (6) turns 128 deg at the start of the EMPTY reloads only and stays
--     there -- an empty gun's state, which the game keeps itself.
--   * `mag_slot` is turned 45 deg about the weapon's X, and the seated disc with it: `seatRot`.
--   * The hand puts it in to 35 mm, takes a new hold and taps it home; the `_b` takes carry it some other way (30 cm
--     from the hand slot), so the hold is the `_a` pair's.
return {
    id    = 'darra_quasar',
    match = 'darra_quasar',

    -- pwa_w_revolver__darra_quasar.anims: no grab event in this bank, `mag_flip` is the disc turned over in the hand
    sounds = {
        magOut       = 'w_gun_revol_tech_quasar_mag_out',
        magIn        = 'w_gun_revol_tech_quasar_mag_in',
        magGrab      = 'w_gun_revol_tech_quasar_mag_flip',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 12, names = { { 8, 'button_01' }, { 9, 'switch_01' }, { 5, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        showTrack  = 0, -- showMagazine in the source rig, not its remapped pose-buffer slot
        mesh       = 'base\\weapons\\firearms\\revolver\\darra_quasar\\entities\\meshes\\w_revolver__darra_quasar__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_quasar.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the seated disc over 223 frames of quasar_empty_a: 45.0 deg about -X from the weapon, on the slot to 0.3 mm
        seatRot    = { -0.38268, 0.00001, 0.00002, 0.92388 },

        -- a flat disc: thin along its own Z
        drop = {
            centre  = { 0.0, 0.0, 0.0002 },
            flat    = 0.0100,
            long    = { 1, 0, 0 },
            thin    = { 0, 0, 1 },
            arm     = 0.035,
            probe   = 0.0100,
            inertia = 0.00134,
        },

        -- THE HOLD, quasar_one_a / quasar_empty_a, rigid over 25 / 24 frames (2.4-2.5 mm, 0.6 deg), the two the same
        -- to 0.1 mm.
        holdOffset   = { 0.03000, -0.01598, -0.00668 },
        holdRotBasis = { -0.60119, -0.14090, -0.18006, 0.76570 },

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

        -- IN FROM THE LEFT: quasar_empty_b's last 60 mm, 27 points, worst residual 5.2 mm
        wellAxis   = { -0.9996, 0.0192, -0.0230 },
        insertRun  = 0.080,
        seatAt     = 0.70,

        poseStages = {
            { d = 0.080, off = { 0.03000, -0.01598, -0.00668 }, rot = { -0.60119, -0.14090, -0.18006, 0.76570 },
                         pose = 'quasar_mag_left_carry' },
            { d = 0.025, off = { 0.03000, -0.01598, -0.00668 }, rot = { -0.60119, -0.14090, -0.18006, 0.76570 },
                         pose = 'quasar_mag_left_seat' },
        },
        pose           = 'quasar_mag_left_seat',
    },
}
