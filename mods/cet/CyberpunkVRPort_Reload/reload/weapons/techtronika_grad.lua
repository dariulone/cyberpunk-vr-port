-- Techtronika Grad — physical-reload configuration.
--
-- A bolt sniper rifle with a long box magazine (47 x 183 x 105 mm, lying along the bore) in front of the grip. Read
-- off four takes with VRIK off (build/shotgun-reload-20261019/takes/grad_*; the analysis in build/rifles-20261020).
--
--   * `chamber_opener` (7) is the bolt, its knob `handle` (16) on the LEFT. The EMPTY reload is a whole sequence --
--     the bolt turns up 40 deg, strokes FORWARD 146 mm to throw the spent case (`bullet`, 6, flies a metre), turns
--     down, then comes back 108 mm and stays there for the magazine change, and only at the end goes home. What the
--     hand does in VR is the part that matters for the gun: back 108 mm and let go (`chamberOnRack = 'dry'`). The
--     forward ejection stroke is the animation's and is not driven.
--   * `chamber_opener_back` (15) rides it 1:1.
--   * `magazine_opener` (11), the catch, is 12 deg about X while the magazine is out: a `hatch`.
--   * Two-handed: the bolt on the LEFT TRIGGER, the magazine rocked out by hand (`handPull`).
return {
    id    = 'techtronika_grad',
    match = 'techtronika_grad',

    -- pwa_w_rifle_sniper__techtronika_grad.anims
    sounds = {
        slidePull    = 'w_gun_sniper_power_grad_lever_handle',
        slideRelease = 'w_gun_sniper_power_grad_lever_load',
        magOut       = 'w_gun_sniper_power_grad_mag_out',
        magIn        = 'w_gun_sniper_power_grad_mag_in',
        magGrab      = 'w_gun_sniper_power_grad_mag_rattle',
        eject        = 'w_gun_sniper_power_grad_shell_eject',
    },

    basis = 'weapon',

    rigs = {
        { which = 0, bones = 5,  names = { { 3, 'mag_std' }, { 4, 'mag_stdr' } } },
        { which = 1, bones = 18, names = { { 7, 'chamber_opener' }, { 11, 'magazine_opener' }, { 5, 'mag_slot' } } },
    },

    barrel = { front = 'vrp_muzzle_slot', back = 'vrp_barrel' },

    hatch = {
        which = 1, bone = 11,
        openTime  = 0.06,
        closeTime = 0.06,
        parts = {
            { which = 1, bone = 11, turn = 12.3, axis = { 1.0, 0.0, 0.0 }, from = 0.0, to = 1.0 },
        },
    },

    slide = {
        slot       = 'vrp_chamber_opener',
        handleSide = 'left',  -- the handle sticks out on the left only (the slide bone's vertices, x -0.040..+0.026 m): reload.lua handGrips
        which      = 1, bone = 7,
        travel     = 0.1080,
        restLocal  = { 0.00000, 0.06390, -0.01260 },
        localAxis  = { 0, 0, -1 },
        sign       = 1.0,
        -- 108 mm in 0.11 s back home (t=6.46..6.57 of grad_empty_a)
        releaseSpeed = 1.20,
        gripSnap   = true,
        -- the grip's wrist point is 122 mm from the bolt handle (below and left of it), and 50 mm round that caught
        -- a hand reaching for the handle late (the slide's magnet was too weak)
        previewRadius = 0.08,
        blendTime  = 0.15,
        pullSndAt  = 0.20,
        styleAngleMax = 180.0,
        grabWith   = 'trigger',
        -- THE CASE out of the port once the handle is this far back with a round in the chamber: our own effect
        -- on the gun's port slot, the case alone (build/rifles-20261020/make_case_fx.py)
        ejectAt    = 0.60,
        ejectFx    = 'vrp_eject_case',
        -- A BOLT GUN: worked after every shot, not only after an empty magazine. The game does that work itself --
        -- `add_cycle_round` (2.67 s: the lever at 0.77, the case out at 1.40) -- so the bolt is held home from the
        -- shot until it has been worked and that clip is over, the case it throws on `bullet` hidden for as long,
        -- and the trigger is kept from the game while a fired case is in the chamber (reload.lua `manual`).
        manual     = true,
        cycleHide  = { 6 },
        cycleHold  = 2.8,
        -- THE LEFT HAND ON THE BOLT while it pulls it back, grad_empty_a t=2.21..2.48 (2.3 mm / 0.7 deg -- the hand
        -- rides it the whole 108 mm, thumb 53 mm from it): left of the receiver and below.
        grips = {
            behind = { off = { -0.0654, -0.0584, -0.0857 },
                       rot = { 0.0680, -0.5313, 0.6170, 0.5766 },
                       pose = 'grad_handle_left' },
        },
        riders = {
            { bone = 15, ratio = 1.0 },
        },
    },

    mag = {
        enabled    = true,
        slot       = 'vrp_mag_slot',
        which      = 0,
        -- TRACK 0: a rifle's magazine rig idles at 1, 0 and the game's own reload drops index 0 while the magazine
        -- is out (the takes' `mt`); index 1 is the reload magazine. Held at 1, the gun kept its magazine after a drop.
        showTrack  = 0,
        mesh       = 'base\\weapons\\firearms\\rifle_sniper\\techtronika_grad\\entities\\meshes\\w_rifle_sniper__techtronika_grad__mag_std_01.mesh',
        entity     = 'base\\vrport\\vrp_mag_grad.ent',
        holdSlot   = 'WeaponLeft', holdSlotRight = 'WeaponRight',

        -- the mesh's bounding box: 47.3 x 183.3 x 105.3 mm, long along its Y (the bore)
        drop = {
            centre  = { 0.0, 0.0303, -0.0381 },
            flat    = 0.0236,
            long    = { 0, 1, 0 },
            thin    = { 1, 0, 0 },
            arm     = 0.0680,
            probe   = 0.0236,
            inertia = 0.00372,
        },

        -- THE HOLD, grad_empty_a / grad_empty_b: rigid over 31 / 19 frames (0.8-1.1 mm, 0.7 deg), the two the same to
        -- 0.1 mm, and the same hand all the way in.
        holdOffset   = { 0.08249, -0.03142, -0.06065 },
        holdRotBasis = { 0.24371, 0.72931, 0.27332, 0.57793 },

        handPull       = true,
        pullTrigger    = true,
        -- THE SUPPORT HAND ON THE MAGAZINE, as on the Pozhar (2026-10-26: grip alone holds the gun by the magazine,
        -- grip + trigger takes the magazine out): a second two-hand hold, CyberpunkVR_TwoHandGrip_grad@mag.ini
        -- (build/maghold-20261026/mag_holds.py: this gun's fitted carry round the seated magazine) (a bullpup: the magazine is BEHIND the grip) -- hold 1,
        -- taken away with no magazine in the well.
        holdAlt        = 1,

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

        -- STRAIGHT UP: the four takes' fits (worst residual 2.2-3.0 mm)
        wellAxis   = { -0.0084, -0.0161, -0.9998 },
        insertRun  = 0.080,
        seatAt     = 0.70,
        chamberOnRack = 'dry',

        poseStages = {
            { d = 0.080, off = { 0.08249, -0.03142, -0.06065 }, rot = { 0.24371, 0.72931, 0.27332, 0.57793 },
                         pose = 'grad_mag_left_carry' },
        },
        pose           = 'grad_mag_left_carry',
    },
}
