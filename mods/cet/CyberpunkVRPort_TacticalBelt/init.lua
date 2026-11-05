-- CyberpunkVRPort_TacticalBelt -- the belt the player always wears, and what hangs on it.
--
-- WHY A BELT AT ALL. The port has run out of buttons. Everything a hand can do physically -- draw the
-- magazine, rack the slide, reach for the holster -- was won back from a button, and grenades, spare
-- magazines and the rest still live on a wheel. A belt is where those things go in real life, so it is
-- where they should go here: an object at the waist that the hand reaches for.
--
-- WHY IT IS A GARMENT ITEM AND NOT A COMPONENT ON THE PLAYER TEMPLATE. Baking meshes into
-- `player_ma_fpp.ent` is this port's own trick for the VRCAM cameras and the held-object carriers, and it
-- was the first thing tried here. It is the wrong tool for clothing: a garment item in an attachment slot
-- gets garment support and the body-type mesh variants ArchiveXL substitutes for garment items only. A
-- hand-placed skinned mesh gets none of it and clips through whatever is actually being worn.
--
-- WHY ITS OWN SLOT. The belt goes into AttachmentSlots.VRPortWaist, a slot the port registers on the
-- player itself (mods/tweaks/vrport/AttachmentSlots.VRPortWaist.yaml), with the engine's own
-- TransactionSystem. It used to go into EquipmentEx's OutfitSlots.Waist, which exists only while
-- EquipmentEx is installed. EquipmentEx is optional now and does not see the port's belt: the record lists
-- only the port's slot, so the wardrobe patches pass it by and an outfit can never hold a second one. The
-- Ezio belts (variants 1-3) are another mod's records that list only OutfitSlots.Waist; they still need
-- EquipmentEx, and the route "outfit" still hands a belt to its wardrobe.
--
-- WHY LUA AND NOT REDSCRIPT. Calling EquipmentEx from redscript makes it a COMPILE-TIME dependency, and
-- a redscript file that fails to compile switches off every redscript mod in the game, not just itself.
-- From here the outfit system is looked up by name and a missing EquipmentEx is simply a nil. Lua also
-- reaches the two things this needs and redscript does not: a component's RTTI properties, and
-- the plugin's per-component appearance change (Game.VRComponentChangeAppearance).
--
-- HOW THE PIECES MOVE (read off the asset, not guessed):
--
--   the item          Items.ucbelt_d / _e / _f  -- black / green / FDE tan, each WITH accessories
--                     (a/b/c are the same three colours with the pouches stripped, and they carry no
--                     separate components to toggle, so they are no use here)
--   the entity        base\ezio\ucbelt\ucbelt_group.ent -> ucbeltacc.ent, which carries SEVEN separate
--                     entGarmentSkinnedMeshComponents, one per element -- which is what makes an element
--                     switchable at all:
--
--                       ucbeltacc          the strap itself
--                       ucbeltmag          magazine pouch     front-left,  9 x 7 x 10 cm
--                       ucbeltsoftpouch    small soft pouch   front-right, 6 x 7 x 8 cm
--                       ucbelthardpouch    hard case          right side, 17 x 31 x 11 cm
--                       ucbeltdualpouch    twin pouch         rear, 37 x 24 x 9 cm
--                       ucbeltbagpouch     bag                rear-left,  13 x 10 x 9 cm
--                       ucbeltradio        radio              right hip,   3 x 7 x 17 cm
--
--   the colour        every element mesh carries appearance_01 (black), _02 (green), _03 (FDE tan), so
--                     the colour is a per-component appearance and changes with no unequip.
--
-- The sizes above are the meshes' own bounding boxes in bind pose (male body; the female meshes sit
-- about 8 cm higher and are ~15% narrower). They are written down here because the reach-for-an-element
-- step will need them, and measuring them twice is waste.

local CFG = {
    enabled = true,

    -- Put the belt on and keep it on. Off leaves it to the wardrobe, for a player who wants to manage
    -- the thing themselves.
    auto_equip = true,
    -- Name the belt's appearance after equipping it. The item pipeline leaves ours on `default`,
    -- which no belt entity defines, so nothing is drawn until someone says which one.
    force_appearance = true,

    -- THE BELT'S OWN HARDWARE, one entry per piece.
    --
    -- The NCPD duty belt exists in the base game twice over: as one all-in-one mesh, and as twenty
    -- separate pieces. `vrp_belt` is the all-in-one -- what has been drawing all along -- and the
    -- twenty are components beside it, shipped disabled. Turning the pouches off means switching the
    -- all-in-one off and the strap on, plus whatever else should stay.
    --
    -- This is how 707 Tactical Battle Belts composes its belts, and it is how every other element on
    -- this belt already works: a named component, toggled per frame.
    -- CONFIRMED ON THE PICTURE: the all-in-one off and `strap` on gives the bare belt, which is all
    -- this port wants: nothing but the belt, with the port's own elements laid over it.
    -- The asset ships in exactly this state too, so the two cannot disagree.
    belt_all_in_one = false,
    belt_pieces = {
        strap = true, back = false,
        mag = false, mag_l = false, mag_r = false, mag_back = false,
        pouch_small = false, pouch_l = false, pouch_r = false,
        bag_front_l = false, bag_front_r = false, bag_back_l = false, bag_back_r = false,
        radio = false, radio_front = false,
        torch = false, torch_front = false,
        gren_front = false, gren_mid = false, gren_back = false,
    },

    -- THE FEMALE BODY GETS ITS OWN, and it has to, because the MESHES differ.
    --
    -- Measured: the base game ships the belt split into twenty pieces for the MALE rig only
    -- (`i1_016_ma_belt__police*`); the female body has one all-in-one, `i1_016_wa_belt__police_eq`.
    -- So `strap` on a female body draws the male belt, which is 33 mm wider across than the female
    -- one -- it looked too wide, and it was.
    --
    -- Hence: female wears the all-in-one and trims its pouches with the chunk mask; male wears the
    -- strap piece. Exactly ONE table is ever in force, never both, like the offsets layer.
    -- The female belt is `vrp_belt_strap` too, but pointing at OUR shrunk copy of the male strap:
    -- the female mesh has four render chunks and they are its four LODs, so its pouches cannot be
    -- masked off. Measured, not assumed.
    belt_all_in_one_female = false,
    belt_pieces_female = {
        strap = true, back = false,
        mag = false, mag_l = false, mag_r = false, mag_back = false,
        pouch_small = false, pouch_l = false, pouch_r = false,
        bag_front_l = false, bag_front_r = false, bag_back_l = false, bag_back_r = false,
        radio = false, radio_front = false,
        torch = false, torch_front = false,
        gren_front = false, gren_mid = false, gren_back = false,
    },

    -- WHICH PIECES OF THE BELT ARE DRAWN.
    --
    -- The NCPD duty belt is ONE mesh of thirteen sub-meshes -- the strap plus its pouches, magazine
    -- carriers, flashlight and radio -- and they all share one material, so nothing in the asset says
    -- which chunk is which. The only way to find out is to switch one off and look, which is why this
    -- is a live table rather than a number typed into the asset.
    --
    -- A PLAIN ARRAY of 32 booleans, entry i+1 for chunk i, `true` = hidden. An array and not a keyed
    -- table because belt.cfg is JSON: numeric keys come back as STRINGS and would silently stop
    -- matching. All false is the whole belt; once the picture is right the mask gets baked into the
    -- asset and these go back to false.
    belt_chunk_off = { false, false, false, false, false, false, false, false,
                       false, false, false, false, false, false, false, false,
                       false, false, false, false, false, false, false, false,
                       false, false, false, false, false, false, false, false },

    -- ...and its female twin. A bit index means a sub-mesh of ONE mesh, and the two bodies wear
    -- different meshes, so a mask found on one says nothing about the other.
    belt_chunk_off_female = { false, false, false, false, false, false, false, false,
                              false, false, false, false, false, false, false, false,
                              false, false, false, false, false, false, false, false,
                              false, false, false, false, false, false, false, false },

    -- WHAT THE MODULE PUTS ON, and it does it every two seconds whether or not anything else was
    -- equipped by hand. That is the whole of the old Ezio belt coming back: the new belt
    -- was in the game and correct, and `ensureEquipped` kept putting the old one back over it.
    --
    -- The three Ezio variants stay in the list -- they are what every placement was tuned on and the
    -- port still works with them -- and the port's OWN belt is the fourth.
    variants = { "Items.ucbelt_d", "Items.ucbelt_e", "Items.ucbelt_f", "Items.vrp_belt" },
    -- The mod calls these black / green / FDE tan, but every element mesh carries its OWN material table
    -- under the same appearance name, so what an index looks like is a question for the eye: 02 reads
    -- yellow on the magazine pouch. Indexes kept, labels honest.
    variant_names = { "01 dark", "02 khaki", "03 tan", "NCPD (наш)" },
    variant = 4,
    -- BUMP THIS WHEN THE LIST CHANGES. belt.cfg keeps the chosen index, which is right -- it is the
    -- player's choice -- but an index means nothing once the list under it has moved, and a file
    -- written before the port had a belt of its own points at Ezio's first variant forever. The same
    -- one-shot upgrade the throw numbers use.
    belt_variant_v = 1,

    -- The appearance each element wears; same three colours, applied per component.
    appearances = { "appearance_01", "appearance_02", "appearance_03" },

    -- THE SLOTS. The port's own for a belt whose record lists it (the port's belt); EquipmentEx's for one
    -- that lists only that (the Ezio belts) -- see slotID. Neither is read back from belt.cfg: which slot
    -- an item goes in is the code's business, and files written before the port had its own slot carry
    -- OutfitSlots.Waist.
    slot = "AttachmentSlots.VRPortWaist",
    outfit_slot = "OutfitSlots.Waist",

    -- The garment components carry this prefix on the player entity.
    prefix = "ucbelt",

    -- THE PLANE THE BELT IS DRAWN IN, and this one was measured rather than reasoned about.
    --
    -- First person does not draw everything in one plane: the arms and the weapon go to RPl_Weapon so
    -- they stay out of the world's depth and lighting, which is also why this port's held-object carriers
    -- sit on that plane. With a weapon drawn the belt dropped behind the other clothing, and the census
    -- says why:
    --
    --   t2_073_pma_vest__tactic_collar   renderingPlaneAnimationParam = renderPlane
    --   t0_000_pma_base__full (body)     None
    --   l1_021_pma_pants__cargo          None
    --   ucbelt* (ours)                   None
    --
    -- ...while renderSceneLayerMask reads 3 on every one of them, weapon out or not. So the plane is not
    -- a field we can copy -- it is driven by an ANIMATION PARAMETER. The first-person graph raises
    -- `renderPlane` when a weapon comes out, every component subscribed to it moves to the weapon plane,
    -- and there it composites with the gun instead of with the world. The vest subscribes. The belt did
    -- not, so the vest was drawn over it.
    --
    -- Subscribing the belt to the same parameter is what the vanilla garment already does. Re-applied
    -- after every attach, because an attach rebuilds the components from the asset.
    render_plane_param = "renderPlane",

    -- ArchiveXL's garment offsets push one garment layer clear of the one under it, and EquipmentEx only
    -- turns them on inside its outfit mode. They were the first suspect for the belt being swallowed by
    -- the clothes -- and they were the wrong one: the cause was the render-plane parameter above, and it
    -- is fixed there. So this is OFF. What is left of it otherwise is a repeating engine call with
    -- nothing to show for it, and that is not what should be running while a crash is unexplained.
    garment_offsets = false,

    -- How the colour is changed:
    --   "item"      re-attach the d/e/f variant, so the appearance comes from the item's own definition
    --   "component" ChangeAppearance on each element -- instant, but it fights that definition
    -- Measured: poking a garment component leaves the assembled proxy in place and the surface flickers
    -- between the two materials, so the item route is the default.
    color_route = "item",

    -- "attach" puts the belt straight into the slot with the TransactionSystem and leaves the wardrobe
    -- alone; "outfit" hands it to EquipmentEx, which switches its outfit mode on. See attachDirect.
    route = "attach",

    -- OUR OWN ELEMENTS, hung on the belt's own components.
    --
    -- Each entry takes over one host component: the host's mesh is swapped for the one named here and
    -- its appearance set, once, when the belt is (re)found. The position is NOT a setting -- it comes
    -- from the mesh's own skinning, and these meshes are the game's own NCPD duty-belt hardware, already
    -- skinned to the player rig and already sitting at the waist.
    --
    -- {g} is the body: ma for the male rig, wa for the female one.
    --
    -- The game also ships, in the same folder, _frag_grenade_mid / _back, _mag_l / _mag_r / _mag_back,
    -- _flashlight, _radio, _radio_front, _pouch_l / _pouch_r / _pouch_small, _bag_front_l / _bag_front_r
    -- / _bag_back_l -- so more elements cost a line here and nothing else.
    -- EMPTY ON PURPOSE. An element used to be assembled at runtime: take one of the belt's own
    -- components, point it at another mesh with ChangeResource, set its appearance and its offset. It
    -- worked -- a grenade appeared on the belt and it was placed by a number -- but the swap sits on a
    -- component the garment system owns, and the module was running it again on every re-enumeration:
    -- once every five seconds, all session, rebuilding render proxies each time. That is not a thing to
    -- ship, and it is not what elements should be anyway.
    --
    -- Elements belong in an asset: one component per element, mesh and material and place baked, and
    -- Toggle the only thing done at runtime. The recipe, all of it measured, is kept here for that work:
    --
    --   mesh        base\characters\garment\corp_ncpd\item\i1_016_belt__police_eq\
    --               i1_016_{ma|wa}_belt__police_frag_grenade_front.mesh   (also _mid, _back;
    --               _mag_l / _mag_r / _mag_back, _flashlight, _radio, _radio_front,
    --               _pouch_l / _pouch_r / _pouch_small, _bag_front_l / _bag_front_r / _bag_back_l)
    --   appearance  valentinos / tyger_claws / 6th / aldecados / wraiths   (the grenade's own sets)
    --   place       that mesh's bbox centre is (-0.2085, +0.0515, 1.0555) in entity space, which is the
    --               NCPD duty belt's line; ours wants (+0.0745, +0.0505, +0.0095) added to reach the
    --               magazine pouch of this belt
    --   rule        grenade -- see readState
    elements = {
        -- THE GRENADE HOLDER, front right, on the pouch whose rule is already "grenade".
        --
        -- The mesh is ours: a copy of 707 Tactical Battle Belts' `i1_016_ma_belt__police_frag_grenade_
        -- front.mesh`, which is the NCPD duty-belt hardware, skinned to Hips + Spine and therefore
        -- already riding the player's rig. Copying it into our own archive rather than pointing at
        -- caibro's path means the 707 mod does not have to be installed.
        --
        -- OUR EIGHT COLOURS were added to it beside its own five sets (valentinos, tyger_claws, 6th,
        -- aldecados, wraiths), so `hl` can be either -- one of ours for the port's own marking, or one
        -- of the mesh's own sets if a recolour reads better than a wash.
        --
        -- THE OFFSET is the difference between two measured numbers, not a guess: the mesh's own bbox
        -- centre is (-0.2085, 0.0513, 1.0556) -- the NCPD belt's line -- and CFG.centres puts this
        -- host at (0.171, 0.052, 1.069). Whether a local offset moves a SKINNED component at all is
        -- the one thing here that only the picture can answer; if it does not, the holder will sit at
        -- the NCPD line instead, which is front left of centre.
        -- OFF: taking over ucbeltsoftpouch COSTS a pouch, and the holder is meant to be an addition,
        -- not a replacement -- ucbeltsoftpouch is one more pouch, not a grenade holder. The
        -- holder becomes an EIGHTH component in the belt's own ucbeltacc.ent instead; this entry stays
        -- as the record of the offsets, which were measured and are still the right numbers if a host
        -- ever has to be taken over.
        --[[
        {
            name = "grenades",
            host = "ucbeltsoftpouch",
            mesh = "base\\vrport\\meshes\\vrp_belt_gren_front.mesh",
            appearance = "valentinos",
            hl = "lit",
            ox = 0.3795, oy = 0.0007, oz = 0.0134,
        },
        ]]
        -- THE MAGAZINES stay on the belt's own mag pouch, which is already in the right place -- where
        -- the ucbeltmag pouch is now. Our copy of caibro's mag mesh is built and shipped
        -- (vrp_belt_mag.mesh, own sets militech / ncpd / arasaka / border_patrol / kang_tao /
        -- 6th_Street plus our eight), so swapping to it is one line here if the Ezio pouch is not
        -- what should be there.
        --   { name = "mags", host = "ucbeltmag",
        --     mesh = "base\\vrport\\meshes\\vrp_belt_mag.mesh",
        --     appearance = "militech", hl = "lit",
        --     ox = 0.0745, oy = 0.0507, oz = 0.0094 },
    },

    -- WHERE EACH OF THE BELT'S OWN ELEMENTS SITS, entity space, off the meshes' bounding boxes (male
    -- body; the female meshes sit ~8 cm higher in entity space, but the same distance from the hips,
    -- which is the frame these are converted into, so one table serves both).
    -- THE FEMALE BODY'S OWN OFFSETS, and why they are a layer instead of a bake.
    --
    -- The belt meshes are SHARED between the two bodies -- one mesh per element, not one per rig --
    -- so baking a correction measured on the female body moves the male one by exactly as much. These
    -- forty-one were tuned on the female body and are almost all the same shape: 12 to 48 mm back
    -- along Y and 12 to 82 mm up on Z, which is the two rigs' hips differing, plus a handful of
    -- magazines that also want a roll.
    --
    -- Only ONE layer is ever applied: this one on a female body, `offsets` on a male one. They are
    -- never added together, so no two euler pairs have to be composed, and the panel edits whichever
    -- is in force -- which also means `tools/bake_belt_offsets.py`, which reads offsets.json, can only
    -- ever bake the male layer. That is the point.
    offsets_female = {
        -- the pump's row of three shells sat ~5 cm inside the body (it needed to come forward a little); the whole
        -- row follows its leader (OFFSET_GROUP)
        ucbeltammo_shell1          = { x = 0.0000, y = 0.0500, z = 0.0000, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade              = { x = -0.0120, y = -0.0120, z = 0.0160, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_biohazard    = { x = 0.0000, y = -0.0200, z = 0.0280, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_cutting      = { x = -0.0080, y = -0.0160, z = 0.0160, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_emp          = { x = 0.0180, y = -0.0260, z = 0.0040, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_flash        = { x = -0.0100, y = -0.0100, z = 0.0480, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_incendiary   = { x = -0.0180, y = -0.0060, z = 0.0300, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_ozob         = { x = 0.0040, y = -0.0140, z = 0.0220, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_recon        = { x = 0.0100, y = -0.0180, z = 0.0220, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltgrenade_smoke        = { x = -0.0260, y = 0.0100, z = 0.0320, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_ashura           = { x = 0.0000, y = -0.0140, z = 0.0440, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_burya            = { x = 0.0000, y = -0.0020, z = 0.0420, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_chao             = { x = 0.0320, y = -0.0120, z = 0.0000, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_copperhead       = { x = 0.0000, y = -0.0060, z = 0.0140, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_crusher          = { x = -0.0140, y = -0.0160, z = 0.0380, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_dian             = { x = 0.0000, y = 0.1780, z = 0.0380, rx = 8.50, ry = 0.00, rz = 0.00 },
        ucbeltmag_grit             = { x = 0.0000, y = -0.0120, z = 0.0200, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_guillotine       = { x = 0.0000, y = -0.0140, z = 0.0240, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_kappa            = { x = 0.0000, y = -0.0160, z = 0.0000, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_kenshin          = { x = -0.0220, y = -0.0220, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_kyubi            = { x = -0.0140, y = -0.0120, z = 0.0160, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_lexington        = { x = -0.0060, y = -0.0160, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_liberty          = { x = -0.0580, y = -0.0280, z = 0.0260, rx = 0.00, ry = 0.00, rz = -18.50 },
        ucbeltmag_masamune         = { x = -0.0160, y = -0.0260, z = 0.0300, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_metel            = { x = -0.0260, y = -0.0340, z = 0.0340, rx = 0.00, ry = 0.00, rz = -5.50 },
        ucbeltmag_nova             = { x = -0.0100, y = -0.0120, z = 0.0360, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_nue              = { x = -0.0340, y = -0.0220, z = 0.0540, rx = 0.00, ry = 0.00, rz = -11.50 },
        ucbeltmag_omaha            = { x = -0.0260, y = -0.0100, z = 0.0420, rx = 0.00, ry = 0.00, rz = -3.00 },
        ucbeltmag_palica           = { x = 0.0000, y = -0.0160, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_pozhar           = { x = 0.0060, y = -0.0040, z = 0.0360, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_pulsar           = { x = 0.0000, y = -0.0120, z = 0.0280, rx = 0.00, ry = 0.00, rz = 4.00 },
        ucbeltmag_quasar           = { x = 0.0060, y = -0.0060, z = 0.0400, rx = 0.00, ry = 0.00, rz = 6.00 },
        ucbeltmag_saratoga         = { x = 0.0000, y = -0.0120, z = 0.0000, rx = 0.00, ry = 0.00, rz = -4.00 },
        ucbeltmag_satara           = { x = 0.0080, y = -0.0140, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_senkoh           = { x = -0.0160, y = -0.0380, z = 0.0220, rx = 0.00, ry = 0.00, rz = -6.50 },
        ucbeltmag_sidewinder       = { x = 0.0000, y = -0.0140, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_silverhand       = { x = 0.0000, y = -0.0160, z = 0.0340, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_tamayura         = { x = -0.0280, y = -0.0280, z = 0.0180, rx = 0.00, ry = 0.00, rz = -12.00 },
        ucbeltmag_ticon            = { x = -0.0320, y = -0.0260, z = 0.0440, rx = 0.00, ry = 0.00, rz = -12.00 },
        ucbeltmag_umbra            = { x = 0.0260, y = -0.0160, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
        ucbeltmag_unity            = { x = -0.0180, y = -0.0080, z = 0.0240, rx = 0.00, ry = 0.00, rz = -7.00 },
        ucbeltmag_yukimura         = { x = 0.0040, y = -0.0180, z = 0.0260, rx = 0.00, ry = 0.00, rz = 0.00 },
    },

    centres = {
        ucbeltmag       = { -0.134,  0.102, 1.065 },
        ucbeltsoftpouch = {  0.171,  0.052, 1.069 },
        ucbelthardpouch = {  0.079, -0.019, 1.072 },
        ucbeltradio     = {  0.181, -0.020, 1.118 },
        ucbeltdualpouch = { -0.018, -0.055, 1.081 },
        ucbeltbagpouch  = { -0.095, -0.158, 1.086 },
        ucbeltacc       = { -0.002,  0.003, 1.065 },
        -- The holder's own bbox centre, read off the mesh: it is the NCPD duty belt's line, which is
        -- where a component skinned to that mesh actually sits. Measured, not chosen.
        -- Starting centres, straight off each mesh's own bounding box. They move with the bake, and
        -- until the holders are tuned they are only a first guess at where the hand has to be.
        -- One centre for the whole row: the hand reaches for the shells, not for a shell.
        ucbeltammo_shell1   = { -0.1476,  0.0782,  1.0463 },
        ucbeltammo_shell2   = { -0.1253,  0.0933,  1.0480 },
        ucbeltammo_shell3   = { -0.1029,  0.1084,  1.0496 },
        ucbeltgrenade       = {  0.1431,  0.1178,  1.0797 },
        ucbeltgrenade_flash = {  0.1251,  0.1098,  1.0810 },
        ucbeltgrenade_incendiary = {  0.1371,  0.1138,  1.0890 },
        ucbeltgrenade_biohazard = {  0.1351,  0.1218,  1.0890 },
        ucbeltgrenade_smoke     = {  0.1411,  0.1098,  1.1050 },
        ucbeltgrenade_recon     = {  0.1211,  0.1298,  1.0950 },
        ucbeltgrenade_cutting   = {  0.1331,  0.1238,  1.0790 },
        -- The EMP -- the game calls it the sonic grenade, `w_explosives_009__sonic`. It rides the
        -- same pouch as the six before it, so it inherits their centre rather than getting a new
        -- one guessed at: its belt mesh was rebound with the recon's own bone matrices.
        ucbeltgrenade_emp       = {  0.1211,  0.1218,  1.1030 },
        -- Ozob's Nose sits where the flashbang sits -- its belt mesh was given that element's
        -- binding verbatim, so it inherits a seat that is already tuned.
        ucbeltgrenade_ozob      = {  0.1251,  0.1118,  1.0970 },
    },

    -- The element the hand is reaching for lights up. OFF: this ran per frame in the session that
    -- crashed twice with a null `this` inside the engine, and it is the only thing that was new there.
    -- It is not exonerated, so it is not armed. Switch it on from the panel to test it deliberately.
    -- A LIVE OFFSET PER BELT COMPONENT, in metres, entity space: { x = right, y = forward, z = up }.
    --
    -- This is the tuning surface -- the panel writes here and the tick writes it onto the component,
    -- so a pouch can be dragged into place with it on the body. Whatever ends up here gets baked into
    -- the asset afterwards and the entry goes back to zero.
    --
    -- IT WORKS ON THESE COMPONENTS, which was not obvious: an offset baked into the .ent's
    -- localTransform and a mesh moved by its quantizationOffset STACKED, and that is how the holder
    -- overshot the hard pouch. So the component transform is live on a garment skinned component after
    -- all; what is dead is nothing, and both levers have to be counted together.
    -- EMPTY, and it has to stay that way for anything skinned.
    --
    -- The sliders below write SetLocalPosition / SetLocalOrientation, and on a garment component that
    -- is the wrong frame twice over: the offset does not follow the hips, so the piece drifts as the
    -- camera turns, and the rotation turns about the entity origin rather than about the piece. It is
    -- a TUNING surface and nothing else.
    --
    -- What a tuned result becomes is a transform baked into the mesh's bone rig matrices --
    -- tools/bake_mesh_transform.py, which multiplies every inverse bind matrix by it. Then the bones
    -- carry it exactly like the geometry, and the camera has nothing to do with it. The holder's
    -- (0.1180, 0.1540, -0.0100 / 0, -1.5, -66.5) is already in vrp_belt_gren_front.mesh that way.
    --
    -- EXCEPT the male body's own tuned entries (MALE_TUNED_OFFSETS, put in by upgradeOffsetsCfg):
    -- the meshes are shared with the female body, so a bake would move her pieces by as much, and
    -- these stay a layer for the same reason offsets_female is one. Translations only.
    offsets = {},
    offsets_v = 0,

    -- TAKING A GRENADE OFF THE BELT. Grip the controller with the palm at the grenade holder and it
    -- goes into that hand; let go and it goes back on the belt.
    --
    -- ONLY FROM THE BELT, and that is the whole rule: the hand has to be at the holder, which is the
    -- same proximity test the mark already runs, so there is no second radius and no way to conjure a
    -- grenade out of the air.
    grenade_grab = true,
    -- WHICH PLANE THE HELD GRENADE IS ATTACHED IN. 0 follows the player, 1 forces the scene plane,
    -- 2 forces the weapon plane.
    --
    -- It is a knob rather than a decision because the plane cannot be read back off the object and
    -- cannot be changed after the attach -- so the only instrument that settles it is the picture, and
    -- one reload of this module per value beats a rebuild per guess. What is already known: attached
    -- with the WEAPON plane, live, with a gun in the other hand, the grenade was visible.
    -- 0, FOLLOW THE PLAYER. It was 2 while the grenade could not be seen in the scene plane, and the
    -- price of 2 was the "transparent hand": an empty hand is drawn in the scene plane, the weapon plane
    -- is drawn on top of it, so the grenade covered the fingers that hold it. Measured 2026-10-09 with
    -- the per-type props, right hand, no weapon, simulator screenshots: in the scene plane the grenade is
    -- drawn and the back of the hand covers it as it should; in the weapon plane it is drawn over the
    -- hand. A grenade is only taken with no weapon in hand, so it starts in the scene plane, and follows
    -- the player into the weapon plane if a weapon comes out in the other hand.
    grenade_grab_plane = 0,
    -- THE RECORDED FINGER POSE, ON. It was switched off for a while on suspicion of a crash, and it was
    -- innocent: the fault was a plane check asking the held prop `GetItemID`, which is a weapon's
    -- question and dereferenced a null. Twenty-eight clean grab-and-release cycles after that was
    -- removed settled it.
    --
    -- It has to be on for the hold to be TUNED at all -- the grenade is being lined up against the
    -- hand that grips it, and with the fingers tracking freely there is no grip to line up with.
    grenade_grab_pose = true,
    -- THE GRAB TRACE, off now that it has done its job. It found the crash by being the only record
    -- that survived one: it opens, writes and closes per line, while the module's ordinary log lost a
    -- whole minute before each dump -- which is what made "the line is not there" look like evidence
    -- twice, and it was not.
    --
    -- Kept rather than removed: the next thing that dies inside a grab will want it back, and turning
    -- it on is one click.
    grab_trace = false,

    -- HOW THE GRENADE SITS IN THE HAND, in the hand slot's own frame: metres and degrees.
    --
    -- Measured first, so this starts from a fact rather than a guess: in the game's own throw the held
    -- grenade's transform equals the WeaponRight ANCHOR exactly -- 0.00 degrees of rotation against it
    -- across 165 frames, with 0.61 of frame noise. The game adds nothing. What is wrong in VR is the
    -- anchor itself: with no weapon animation playing it is not driven to the tracked hand, so the
    -- thing hung on it inherits whatever the idle rig left there.
    --
    -- So this offset is not "the game's pose recreated" -- it is the correction from where the anchor
    -- happens to be to where the hand actually is, and only the picture can settle it.
    grenade_hold = { x = 0.0, y = 0.0, z = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },

    -- THE PIN. Distances from the free wrist to where the game's own throw held it, measured in the
    -- holding wrist's frame; the pull is how far along the recorded direction the hand has to travel.
    grenade_pin = true,
    -- Closer than the first guess: 0.22 started the fingers a whole forearm away -- the finger pose
    -- came on too far out. These are distances to the ring itself, so they should feel like reaching
    -- for something small.
    -- Distances from the PINCH to the ring, so they are fingertip distances now and not arm's-length
    -- ones: with the fingers actually on it the reading was 0.115 measured off the wrist and about a
    -- centimetre measured off the pinch.
    grenade_pin_preview_m = 0.10,
    grenade_pin_grip_m = 0.04,
    -- HOW FAR THE PIN COMES OUT before it stops. A pin is a couple of centimetres of wire in a hole,
    -- and this is also where the hand starts being held back -- so it is what the resistance feels
    -- like, not just what is drawn.
    grenade_pin_slide_m = 0.002,
    -- WHICH WAY THE PIN SLIDES, in the grenade's frame. The geometric direction -- the ring's own
    -- offset -- runs a third of the way upwards, and on the picture that read as the pin leaving
    -- upwards rather than straight out. Flattened here; normalised in the module, so only the
    -- proportions matter.
    -- Flat now: straight out of the body with no vertical component at all. Two rounds of taking the
    -- rise out of it were still visibly upwards, so this brackets it from the other side -- if the pin
    -- now leaves slightly DOWNWARDS, the answer is between this and the last value.
    grenade_pin_axis = { x = -0.02, y = -0.83, z = 0.0 },

    -- THE SAFETY LEVER, pressed by the trigger of the hand that holds the grenade.
    grenade_lever = true,
    -- How fast the finger closes on it, in units of the full press per second: 8 is about an eighth of
    -- a second end to end, which is a finger rather than a switch.
    grenade_lever_speed = 8.0,
    -- WHERE THE OPEN HAND SITS on the pinch->hold line. 0 is the recorded pinch exactly; negative
    -- extrapolates past it, opening the hand further than any take shows, because the game never lets
    -- the lever go and there is no recording of a hand that has.
    grenade_lever_open = 0.0,
    -- AND THE GUN STAYS QUIET while a grenade is held: the squeeze is the lever, not a shot.
    grenade_trigger_block = true,

    -- SOUND, and every event name is a key so a better one can be tried without a reload of anything.
    -- The defaults are read out of the game's own grenade animations -- see the note in grenade.lua.
    grenade_sound = true,
    grenade_snd_grab = "grenade_equip",
    grenade_snd_stow = "grenade_unequip",
    grenade_snd_pin = "grenade_arm_pin",
    -- The types without a pin: recon, cutting and the EMP are armed by a thumb on a button, and the
    -- game has its own event for it.
    grenade_snd_button = "grenade_arm_button",
    -- The Wwise emitter a type's sounds go out on, when its own entity does not register one. Only the
    -- EMP needs it: its visual entity has no `gameaudioSoundComponent` at all, so nothing was ever
    -- heard from it. The other seven resolve theirs from the asset and are deliberately absent here.
    grenade_snd_emitter = { emp = "gre_ent_emp" },

    -- WHERE ON THE HAND THE BELT IS MEASURED FROM.
    --
    -- The point is `VRPalmModelPos`, which is the SOLVED avatar's `RightHandMiddle1` /
    -- `LeftHandMiddle1` -- the middle finger's base knuckle. That is a reasonable place to call "the
    -- hand", and it is not where everyone's own sense of "I am touching it" sits: in the headset it read
    -- as somewhere between the knuckles and the end of the forearm.
    --
    -- So it can be moved, in the palm's OWN frame, and the panel prints where it ends up. Zero keeps
    -- the knuckle, which is what every measurement in this file so far was taken against.
    --   X  across the palm      (+ towards the thumb on the right hand)
    --   Y  along the fingers    (+ forwards, out of the fist)
    --   Z  out of the palm      (+ away from the palm face)
    highlight_hand_off = { x = 0.0, y = 0.0, z = 0.0 },
    -- How long that button has to be held before the grenade is live.
    grenade_button_hold_s = 0.35,
    -- How far the thumb bends onto the button, in degrees. Authored, not measured: the game presses
    -- that button with the other hand, so there is no take of the holding hand doing it.
    grenade_button_curl_deg = 22.0,
    -- Whether each type uses its OWN measured hold. Off: the frag's, which is the one that was tuned
    -- and looks right on all of them. The measurements are in grenade/poses/types.lua either way.
    grenade_hold_per_type = false,
    -- WHICH TYPES USE THEIR OWN MEASURED DATA. Only the ones listed here; everything else keeps the
    -- frag's, which is what was tuned and what looks right. On by default for the three that are armed
    -- by a button, because they have a press to show and the frag's numbers have nothing for it.
    -- WHICH TYPES USE THEIR OWN MEASURED HOLD.
    --
    -- The pin family keeps the frag's, which was tuned on the picture and looks right on all of them.
    -- The three armed by a button do NOT: the takes put them in a different family of grip entirely --
    -- a different quaternion, not a nudge -- and on the frag's numbers the cutting grenade sits as
    -- if it had a pin to pull and the recon sits above the hand rather than in it.
    --
    -- They were switched off once already on the strength of a test that cannot have meant anything:
    -- that build was erroring every frame on a nil global and never wrote a transform at all. A
    -- judgement made on a broken build is not a judgement.
    grenade_custom = { cutting = true, recon = true, emp = true },
    -- The pin's exit direction, per type, in the grenade's own frame. The panel writes here for the
    -- grenade currently in hand; anything absent falls back to `grenade_pin_axis`.
    grenade_pin_axis_kind = {},
    -- The pin's GRAB POINT, per type, in the grenade's own frame -- where the free hand's fingers are
    -- put. Empty: everything falls back to `grenade_pin_off`, which is the frag's tuned point and is
    -- within 3 mm of every other pin grenade's measured one. The panel writes here for whatever is in
    -- the hand, so dialling one type in cannot move another.
    -- TUNED ON THE BODY AND APPROVED, 2026-09-04, one grenade at a time. Only Z moved: the ring sits
    -- further out of the shell on the frag and closer on the flashbang, and X/Y were right already.
    -- Anything not listed falls back to `grenade_pin_off` above -- incendiary and biohazard were
    -- checked at that value and left there.
    grenade_pin_off_kind = {
        frag  = { x = 0.0, y = 0.0, z = 0.0150 },
        ozob  = { x = 0.0, y = 0.0, z = 0.0120 },
        flash = { x = 0.0, y = 0.0, z = 0.0070 },
        smoke = { x = 0.0, y = 0.0, z = 0.0130 },
    },
    -- WHICH MEASURED POINT A BUTTON GRENADE RESTS AT, per type: "p1" is the window just after the
    -- arming, "p2" the settled one. Judged on the picture -- the recon sits right on p1, the cutting
    -- grenade and the EMP on p2, because the arming moment lands at a different place in each take.
    -- WHICH of a button grenade's two measured windows is used for its single hold transform. It does
    -- not move between them any more -- the thumb does the moving -- so this only picks the seat.
    -- HOW EACH BUTTON GRENADE ANIMATES ITS PRESS.
    --   "lift"  the hold on screen IS the press; idle is that hand with the thumb lifted, and the
    --           grenade does not move between them
    --   "swap"  the recorded press is the idle hand and the trigger moves to the settled grip -- the
    --           recon's arrangement, approved as it is and left alone
    grenade_press_mode = { recon = "swap" },
    -- How far the thumb lifts off the button while carrying, in degrees.
    grenade_button_lift_deg = 22.0,
    -- The hand-tuned correction to the hold, PER TYPE. The panel writes here for whatever is in the
    -- hand, so dialling in the cutting grenade cannot move the frag.
    grenade_hold_kind = {},
    -- The shortest gap between two plays of the same sound. The lever pair gets its own, longer one --
    -- traced from the game, it fired five times in two seconds off a finger resting on the threshold.
    grenade_snd_gap = 0.12,
    grenade_snd_gap_lever = 0.45,
    grenade_snd_lever = "w_expl_frag_grenade_foley_raise_cover",
    grenade_snd_leverOff = "w_expl_frag_grenade_foley_lower_cover",
    grenade_snd_throw = "grenade_throw",

    -- THE THROW. Speed is the hand's own, in metres per second, times the gain; the clamps are what
    -- keeps a twitch from becoming a mortar and a gentle release from becoming nothing at all.
    grenade_throw = true,
    -- WHY A MULTIPLIER AT ALL, and why THIS one -- which is the second answer to that question.
    --
    -- The two ends are measured. A real throwing motion peaks at about 5.4 m/s AT THE WRIST; the
    -- game's own frag grenade leaves the hand at 19 m/s quick-thrown and 25 m/s aimed
    -- (Items.Preset_Grenade_Frag_Default_inline0). A tracker cannot see the wrist snap or the lever
    -- arm of the fingers, which is where most of a real throw's speed comes from, so some multiplier
    -- is unavoidable.
    --
    -- 3.5 matched the GAME. It did not match the HAND: too fast from the start, not physical. In
    -- VR the second one is what is being judged -- the arm that just moved is right there to compare
    -- against -- so the number is set by proportionality to the gesture instead, and a grenade simply
    -- does not fly as far as the flat game throws it.
    --
    -- The two reference points, for the next adjustment: 1.0 is exactly what the wrist did, 3.5 is
    -- what the game does. The slider is in the panel and persists.
    -- HOW MUCH OF THE PLAYER'S OWN VELOCITY THE GRENADE LEAVES WITH. 1.0 is what physics says and
    -- what the game's own throw does; 0 is the old behaviour, where a grenade thrown at a run was
    -- released with the arm's swing alone and fell behind the runner.
    grenade_throw_carry = 1.0,
    grenade_throw_gain = 2.0,
    grenade_throw_min = 1.5,
    -- Room above the game's aimed throw for a hard one, without letting a tracking spike become a
    -- mortar shell.
    grenade_throw_max = 45.0,
    -- Below this the hand has no direction worth using and the palm's own is taken instead.
    grenade_throw_dir_min = 0.30,
    -- How far back the peak of the swing is looked for. Long enough to cover a wind-up, short enough
    -- that a throw is not given the speed of something the hand did a moment before.
    grenade_throw_window = 0.20,
    -- How long the real grenade has to have been in the hand before it can be launched. Pulling the
    -- pin arms it, so in practice this is always satisfied long before the throw; it exists for the
    -- player who pulls and throws in one motion.
    grenade_throw_ready = 0.15,
    -- The player's own hand slots, which is where a throw leaves from. Named rather than hard-coded
    -- because the game's helper uses "RightHand" and the left one has never been checked.
    grenade_throw_hand_r = "RightHand",
    grenade_throw_hand_l = "LeftHand",

    -- THE PORT'S OWN THROWN GRENADE. Off returns to the game's launch, which is correct and invisible.
    grenade_throw_own = true,
    grenade_prop_dir = "base\\vrport\\",
    -- The flight. Gravity is the frag grenade's own AccelerationZ, read out of TweakDB; the rest is
    -- what a small heavy object does.
    grenade_flight_gravity = -10.5,
    grenade_flight_drag = 0.05,
    grenade_flight_bounce = 0.35,
    grenade_flight_spin = 0.9,
    grenade_flight_radius = 0.035,
    grenade_flight_rest_ms = 1.2,
    -- Whether foliage stops a thrown grenade. It should not, and the ray preset that used to be tried
    -- first exists precisely to be stopped by it.
    grenade_flight_soft = true,
    -- How far past a soft hit the ray restarts, in metres.
    grenade_flight_soft_step = 0.05,
    -- The floor of last resort, at the player's feet, used only while falling through it and only where
    -- a ray straight down finds no ground more than floor_drop metres below the ground under the feet
    -- (rays floor_probe metres long; a kerb is 0.15).
    grenade_flight_floor = true,
    grenade_flight_floor_lift = 0.02,
    grenade_flight_floor_drop = 0.05,
    grenade_flight_floor_probe = 50.0,
    -- How long it lives. The fuse runs from the throw; a grenade that has come to rest gets the
    -- shorter one, so it does not sit on the floor for a second and a half doing nothing.
    grenade_fuse_s = 2.5,
    grenade_fuse_after_hit_s = 1.2,
    -- Whether a thrown grenade is taken out of the inventory. OFF until it is known whether the game
    -- has already counted it -- see the note in grenade.lua.
    grenade_throw_consume = true,
    -- WHEN it is taken out of the inventory. "throw" is the moment it leaves the hand, which is what a
    -- player expects to see on the count; "detonate" waits until it has gone off, which is safe from
    -- the one risk "throw" carries -- the armed instance is an inventory item too, and removing one
    -- could in principle be the one waiting in the slot. If a throw ever stops exploding, this is the
    -- key to move, and the trace says `detonate ... -> false` when that is what happened.
    grenade_throw_consume_at = "throw",
    -- HOW it is spent. "charge" is the game's own way since 2.0 -- a pool that refills over time, the
    -- item untouched. "item" deletes the item from the inventory for good and is kept only as the
    -- record of a wrong turn; "off" spends nothing.
    grenade_consume_mode = "charge",
    -- ...and how far PAST that stop the hand has to pull to take it off.
    grenade_pin_pull_m = 0.06,
    -- ...and how fast it has to be going by then. The game's own hand tore away at about 4 m/s; below
    -- this the pin just slides in and out with the hand, which is what a careful tug should do.
    grenade_pin_speed = 2.0,
    -- Where on the pin the fingers take hold, in the grenade's frame: the mesh gives the part's centre
    -- and the ring is the top of it.
    grenade_pin_off = { x = 0.0, y = 0.0, z = 0.012 },

    highlight = true,
    -- true: only the port's own elements can be marked -- a magazine, a grenade, a shell (their meshes carry
    -- the amber mark) or an entry in CFG.elements. false restores the original behaviour, where the nearest
    -- belt component of any kind was recoloured, the strap and the mod's own pouches included.
    highlight_only_ours = true,
    highlight_rate_hz = 20,
    -- Also the reach: a magazine or a shell is taken from the belt inside it (magRadius / shellRadius).
    -- 0.14 until 2026-10-26, a little too tight to find a pouch by feel; see upgradeHighlightRadiusCfg.
    highlight_radius_m = 0.17,
    highlight_radius_v = 0,
    -- CROUCHED, THE REACH IS WIDER. Down on the knees the belt is where the body bends rather than where the
    -- arm falls, and a hand feeling for a pouch there misses it at the standing radius; this multiplies the
    -- reach (and so the mark, which is the same test) while the game has the player crouched.
    crouch_reach_scale = 1.5,
    -- Leaves at a wider radius than it enters, so a hand held on the boundary cannot make the surface
    -- flicker between the two material sets.
    highlight_release = 1.35,
    -- What an element of the belt's own wears when lit, if it has no `hl` of its own. Only used by the
    -- "appearance" style below.
    highlight_appearance = "appearance_03",

    -- HOW an element is lit:
    --   "engine"      the game's own focus outline, re-asserted every tick (see engineHighlight)
    --   "appearance"  swap the element's material set -- works on garment components, reads weakly
    highlight_style = "engine",

    -- The outline's look, as the two enums the engine derives its palette from:
    --   INTERACTION 0/4 (blue, the "you can use this" colour)   IMPORTANT_INTERACTION 1/5
    --   QUEST 2/6 (yellow)   ITEM 8/3 (what loot uses)          HOSTILE 9/0 (red)
    highlight_type = 0,
    highlight_outline = 4,
    -- 0 default, 1 outline only, 2 fill only, 3 outline+fill
    highlight_context = 1,
    -- true draws it through walls, which reads as a soft wash rather than a crisp line
    highlight_through_walls = false,
    -- THE OBJECT MUST BE ON THE SCENE PLANE BEFORE THE CLAIM IS MADE. Established 2026-09-02 on a
    -- katana held in the hand, one variable at a time:
    --
    --     claim while the object is on RPl_Weapon      nothing
    --     move it to RPl_Scene, no new claim           nothing -- the standing claim is not re-read
    --     claim again, now that it is on RPl_Scene     OUTLINED
    --
    -- So the weapon plane really is a gate, and it gates the CLAIM, not the drawing: a claim made
    -- against a first-person proxy is dropped, and moving the object afterwards does not resurrect it.
    -- This is also why attaching through VRPortHoldItemScene worked all along -- that path happens to
    -- do exactly this order, slot-with-scene first and highlight second.
    --
    -- gameItemEventsPropagateRenderingPlane is what moves it. Its plane field is not exposed to
    -- redscript and every name guessed for it from CET was wrong, so the event goes out with the field
    -- at its default -- which is 0, and 0 is RPl_Scene (the enum reads RPl_Scene, RPl_Background,
    -- RPl_Weapon). Confirmed on the picture: the katana visibly moved into the world plane.
    -- BELT PROPS: the elements that can actually be outlined.
    --
    -- A mesh hung on the belt is a component on the PLAYER, and a component has no game object, so the
    -- engine's focus outline can never reach it -- measured three ways on 2026-09-02: the belt object
    -- itself carries a gameVisionModeComponent and a GameplayRoleComponent and still does not outline
    -- (its geometry rides the player), the player does not outline, and an ITEM attached to a slot,
    -- whose template carries a vision component and whose meshes are on RPl_Scene, outlines at once.
    --
    -- So a prop is an item in one of the port's own belt slots. `item` is what goes in the slot, and
    -- up/back/right move it by writing its mesh components' local positions -- the slot's own
    -- customOffset is ignored on this path (0, 0.18 and 2.0 all landed in the same place, checked
    -- live). The axis names are what the picture showed, not what the field names suggest:
    --
    --     +X is UP        +Y is BACKWARD        +Z is to the player's RIGHT
    --
    -- Everything here is saved to belt.cfg, so a position tuned with the sliders survives a restart.
    props = {
        { slot = "AttachmentSlots.VRPortBeltR", item = "Items.VRPortPropR", on = false, lit = false, auto = true, up = -0.02, back = -0.02, right = 0.17, rx = 0.0, ry = 0.0, rz = 0.0 },
        -- A GRENADE MATCHING THE EQUIPPED ONE.
        --
        -- `follow = "grenade"` reads what is equipped (GetActiveGadget) and points ONE record of ours,
        -- Items.VRPortGrenade, at the prop entity for that type, re-attaching when the type changes.
        --
        -- It is a prop and not the equipped grenade itself, and that is measured rather than chosen:
        -- the real one attaches to a slot for real and draws nothing, because a grenade's visual is
        -- entSkinnedMeshComponent only and a skinned mesh with no live skeleton is invisible. Read in
        -- one frame, the magazine prop's mesh sits at world 579.92 -2426.67 174.91 (the player's hip)
        -- while the grenade's sits at 0.18 0.02 0.10 -- its local offset, never transformed, which is
        -- why the panel read 2503 m to the hand. tools/make_grenade_props.py builds ours the way the
        -- magazine was built: a plain entMeshComponent beside every skinned one.
        { slot = "AttachmentSlots.VRPortBelt6", follow = "grenade", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        { slot = "AttachmentSlots.VRPortBelt1", item = "Items.VRPortProp1", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        { slot = "AttachmentSlots.VRPortBelt2", item = "Items.VRPortProp2", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        { slot = "AttachmentSlots.VRPortBelt3", item = "Items.VRPortProp3", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        { slot = "AttachmentSlots.VRPortBelt4", item = "Items.VRPortProp4", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        { slot = "AttachmentSlots.VRPortBelt5", item = "Items.VRPortProp5", on = false, lit = false, auto = true, up = 0.0, back = 0.0, right = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 },
        -- Belt6 is the grenade's, below. TWO PROPS MUST NEVER SHARE A SLOT: each one attaches its
        -- own item and evicts the other's, which reads on screen as the two blinking and swapping
        -- places several times a second.
    },

    -- How close a palm has to come before a prop lights up, and how much further it has to go before
    -- it goes out. The same hysteresis the belt's own elements use, for the same reason: a hand held
    -- on the boundary would otherwise flicker.
    prop_radius_m = 0.06,
    prop_release = 1.5,

    -- HOW A PROP IS MARKED WHEN THE HAND REACHES FOR IT, and which plane it is drawn in. The two are
    -- tied together, which is the whole reason there is a choice at all:
    --
    --   the engine's focus outline reaches an object ONLY on ERenderingPlane.RPl_Scene, so an
    --   outlined prop has to live in the world -- where it sinks into the thigh, is occluded by
    --   clothing and takes world lighting;
    --   the first-person plane draws over the body the way the gun and the arms do, which is what a
    --   thing on your belt should look like in VR -- and nothing there can be outlined.
    --
    -- A LIGHT does not care about the plane. Every prop carries one, `vrp_glow`, baked in by
    -- tools/ent/add_prop_light.py and shipped switched off; the belt toggles it on the same proximity
    -- decision the outline uses. No HUD claim, no renewal timer, no transition to restart, and none
    -- of the glitch intro the fill style plays.
    --
    --   prop_mark   0 = outline   1 = the material mark   2 = outline + mark   3 = light
    --
    -- EMISSIVE is the surface of the prop itself lighting up, and it is the only marking that works on
    -- the first-person plane. Every prop mesh is our own copy carrying a second appearance, `lit`,
    -- whose material is CDPR's own emissive shader in white (tools/make_glow_meshes.py); switching it
    -- is one ChangeAppearance per mesh component, with no HUD claim, no renewal, no fade and no plane
    -- to satisfy.
    --
    -- 3 = the light component, kept because it is built and reversible, but it is a light SOURCE: it
    -- lights the thigh and the clothes rather than the prop itself. That is a light from it, not a
    -- light on it, and it is why it is not the default.
    -- THE PLANE IS NOT A CHOICE. It follows the player, and there is no knob for it: while the weapon
    -- is holstered the props are in the scene with everything else, and the moment a weapon comes out
    -- they go to RPl_Weapon with the gun and the arms. The rule: the belt's elements are in the plane
    -- the player is in, scene or weapon, and there is no choice of
    -- plane at all.
    --
    -- HOW, and the first attempt at this was wrong: subscribing the prop's components to the
    -- `renderPlane` animation parameter, the way the player's vest does, CANNOT WORK. Read out of the
    -- engine's own reflection data:
    --
    --     entSkinnedMeshComponent             renderingPlaneAnimationParam   CName at 0x238
    --     entMorphTargetSkinnedMeshComponent  renderingPlaneAnimationParam   present
    --     entPhysicalMeshComponent            visibilityAnimationParam only
    --     entMeshComponent  (PLAIN)           NO SUCH FIELD -- only renderingPlane at 0x184
    --
    -- Every prop draws through PLAIN entMeshComponent copies, and that is not incidental: a skinned
    -- component needs a live skeleton and draws nothing standalone, which is the whole reason the
    -- grenade was invisible to begin with. So the subscription had nowhere to land, and because the
    -- write went through a pcall it failed in silence and read like a working mechanism.
    --
    -- What the plain component does have is the plain `renderingPlane` enum, so the port drives it
    -- itself: read the plane the player is actually in, write it onto the prop's components. Pinning
    -- the plane at the attach instead is what produced both halves of the old problem:
    -- pinned to the scene, the drawn gun composites over the prop; pinned to RPl_Weapon, the prop is
    -- drawn over the body forever, including in the states where the player is not in that plane.
    --
    -- The two keys are kept, because nothing measured gets thrown out of this file, but NOTHING READS
    -- THEM any more -- the behaviour above is unconditional.
    prop_follow_plane = true,
    prop_plane = 0,
    prop_mark = 1,
    -- WHICH LIT APPEARANCE -- that is, WHICH COLOUR. All eight are the same material at the same
    -- strength and differ only in HighlightColor, so this is a taste setting and nothing else:
    --
    --   1 magenta   2 white   3 cyan   4 green   5 amber   6 red   7 blue   8 violet
    --
    -- The material is base/materials/loot_drop_highlight.mt -- the game's own "this can be picked up"
    -- highlight, found in the headset after a rule had ruled it out in writing.
    --
    -- HighlightIntensity is FIXED at 0.08 in all eight, chosen on the picture out of a 0.05..0.30
    -- sweep. For scale, CDPR's own uses of this same material (base/gameplay/loot/materials) run from
    -- 0.15 on dead bodies to 1.00 on rare loot, so ours sits deliberately below all of it -- a prop
    -- half a metre from the eye does not need what a crate across the street needs.
    --
    -- The pulsing is stopped by SolidBlendingDistanceStart = 0 with End = 0.01, a tiny NON-zero range.
    -- Measured in both directions. CDPR's `_dimmed` files pin both to 0 and that does NOT work here --
    -- it puts the animation straight back -- so their value is not transferable and the measurement
    -- wins over it.
    prop_glow_level = 1,
    -- HOW FAR OUTSIDE the surface the lit twins sit, and therefore how thick the edge reads. Written
    -- into the live components every time they are switched on, so this is tunable with the prop in
    -- front of you; the value baked into the asset is the same 1.05.
    prop_rim_scale = 1.05,
    -- Written straight into the live light component, so these are tunable from the panel with the
    -- prop in front of you rather than guessed here and rebuilt.
    prop_glow_lm = 60.0,
    prop_glow_r = 0.35,
    -- THE PROPS' OWN LOOK, kept apart from the belt elements' so the two cannot drag each other.
    --
    -- outlineType 8 is DISTRACTION, which is WHITE -- and white is the port's own marker: the game
    -- never picks it for loot, quests or hostiles, which is exactly what lets the replacement vision
    -- shader thicken our line and leave every one of the game's alone. Do not change it to a colour
    -- the game uses without changing that shader too.
    --
    -- context: 0 default, 1 outline only, 2 fill only, 3 outline+fill. 1 is what "a clean line, no
    -- wash inside" means, and it is what stops the magenta fill.
    -- How often a standing claim is re-issued, seconds. NOT every frame: each claim under a new name
    -- restarts the HUD's own transition, and ping-ponging two names at frame rate is what made the
    -- outline shimmer -- an artefact of the renewal, not of the game's outline. It still has to be
    -- renewed at all, because a single claim is dropped when the HUD reviews the actor as the player
    -- comes closer, which is the reason engineHighlight alternates names in the first place.
    prop_renew_s = 0.35,
    prop_outline = 8,
    -- highlightType is the FILL style, and the game's own script enum (EFocusForcedHighlightType)
    -- ends at 15 = INVALID. FocusForcedHighlightData.IsValid() accepts a claim if EITHER the fill or
    -- the outline is valid, so asking for INVALID here requests the outline and nothing else -- which
    -- is the first thing to try against the glitchy intro effect the game plays when a highlight
    -- appears, because that effect belongs to the fill style, not to the line.
    --   0 INTERACTION  1 IMPORTANT_INTERACTION  2 QUEST  3 DISTRACTION  8 ITEM  9 HOSTILE  15 INVALID
    -- 6 = WEAKSPOT: checked on the picture, this is the one that comes up with no glitchy intro
    -- effect. 0 (INTERACTION) plays one, 15 (INVALID) also worked but 6 is what was actually looked at.
    prop_type = 6,
    -- 0 DEFAULT, 1 OUTLINE, 2 FILL, 3 FULL.
    --
    -- 3 = the line AND the wash across the surface, which is the engine's own answer to "make the
    -- object itself glow" and costs nothing. It was on 1 only because an early magenta debug fill in
    -- the port's replacement vision shader used to sit on top of it; that probe has been deleted.
    prop_context = 3,
    highlight_proxy_push = true,
    -- Plane pushes per second. The claim is renewed every tick (see engineHighlight); the plane only
    -- has to be re-asserted because equipment events put the object back on the weapon plane, and
    -- twice a second is fast enough that the gap is never seen.
    highlight_push_hz = 2.0,

    -- TEMPORARY probe, see reassertDevices. Off.
    probe_devices = false,

    -- Which elements show, and when:
    --   always / never
    --   grenade   a grenade sits in the Gadget slot
    --   weapon    a weapon is in a weapon slot at all
    --   inhand    a weapon is actually in the right hand -- i.e. when a reload could happen
    rules = {
        ucbeltacc       = "always",
    -- THE BELT'S OWN MAG POUCH IS THE PISTOL HOLDER. It is already modelled, already placed by the
    -- belt's author and already the right size, so a second pouch of ours next to it would be the
    -- same object twice; the pistol needs none, ucbeltmag already does it better.
    ucbeltmag       = "weapon=pistol",
        -- The small front-right pouch is the one that reads as a grenade pouch; the rear twin pouch does
        -- not. Both are one click apart in the panel, because only the picture can settle it.
        ucbeltsoftpouch = "grenade",
        ucbelthardpouch = "always",
        ucbeltdualpouch = "always",
        ucbeltbagpouch  = "always",
        ucbeltradio     = "always",
        -- OURS, added to the belt's own ucbeltacc.ent as an EIGHTH component so no pouch is taken
        -- over. Shown only when there is something to hold, like the pouch that used to stand in for
        -- it. Its `_lit` twin is not listed here on purpose -- see the skip in applyElements.
        -- THE THREE AMMUNITION HOLDERS, one per class of weapon in the loadout.
        --
        -- The game ships a rifle-magazine pouch (the NCPD belt's, four of them) and NOTHING that
        -- holds shotgun shells -- searched: every `shell` in the game's garments is architecture.
        -- So the shells are the Satara's own pair of cartridges, whose two bones are literally named
        -- l_bullet / r_bullet, re-bound to the hips the same way the grenades were.
        -- PUMPS ONLY: a break-action's pair is its own clip on the mag pouch (the Palica's square cells, the
        -- Satara's two shells), and "weapon=shotgun" put this row on the belt beside it the moment the class
        -- lookup started working.
        ucbeltammo_shell1   = "weapon=pump",
        ucbeltammo_shell2   = "weapon=pump",
        ucbeltammo_shell3   = "weapon=pump",
        ucbeltgrenade       = "grenade=frag",
        ucbeltgrenade_flash = "grenade=flash",
        ucbeltgrenade_incendiary = "grenade=incendiary",
        ucbeltgrenade_biohazard = "grenade=biohazard",
        ucbeltgrenade_smoke     = "grenade=smoke",
        ucbeltgrenade_recon     = "grenade=recon",
        ucbeltgrenade_cutting   = "grenade=cutting",
        ucbeltgrenade_emp       = "grenade=emp",
        ucbeltgrenade_ozob      = "grenade=ozob",
    },

    -- Last on the list on purpose: a belt missing from the one cutscene that shows the player with a
    -- weapon looks worse than a belt that is simply always there.
    hide_in_cutscene = false,
    cutscene_tier = 3,

    -- Equipment does not change per frame, and this file is not going to be the reason a frame is late.
    rate_hz = 4,
    verbose = false,
}

local CFG_FILE = "belt.cfg"

local S = {
    ringAt = {},           -- per hand (0 left, 1 right): the lit element's reach-marker point, hips frame
    ready = false,
    acc = 0.0,
    belt = nil,          -- the ItemObject in the waist slot
    beltHash = nil,      -- its EntityID hash, so a new object invalidates the cache
    comps = {},          -- name -> component, for the object above
    dumped = false,
    lastEquipTry = 0.0,
    attachPendingAt = nil, -- when the attach still in flight was accepted; nil when none is
    detachedAt = nil,      -- when a belt of ours was last taken off; the next one waits SWAP_SETTLE after it
    outfitMissing = false,
    route = "-",
    recheck = nil,
    settleAt = nil,      -- when the current component set was found; nothing is written before it settles
    dressed = {},        -- host -> the mesh we last put on it, so a swap happens once per attach
    lit = nil,           -- what is lit right now, as a name, for the panel and the log only
    litOn = {},          -- host -> true for every host actually lit
    litHand = {},        -- side (0/1) -> the host THAT hand holds; two hands, two marks
    litFrom = {},        -- host -> the appearance it wore before, for the no-twin fallback
    grab = { side = nil, slot = nil, host = nil, plane = nil, at = 0.0, was = {} },
    hlAcc = 0.0,
    probeCount = 0,
    magHost = nil,       -- the pouch showing the weapon-in-hand's own magazine, or nil
    magD = {},           -- side (0/1) -> palm distance to it, metres; what the reload reaches for
    magTaken = false,    -- the reload has that magazine in a hand, so the pouch is empty
    shellD = {},         -- side (0/1) -> palm distance to the nearest loose shell in the row, metres
    shellNear = {},      -- side (0/1) -> which of the three that nearest one is
    shellTaken = nil,    -- the shell the reload is carrying: that one draws empty until it is used
    hlFlip = false,
    hand = nil,          -- last palm position used, for the panel
    handDist = nil,
    offsetsAt = 0.0,
    planeWrites = 0,     -- how many times the plane parameter had to be (re)applied
    colorAt = nil,
    state = { grenade = false, weapon = false, inhand = false, tier = 0 },
    want = {},           -- name -> bool, what we last asked for
}

local memo = {}
local startupClothing = require("startup_clothing").new()

local function log(fmt, ...)
    local line = string.format(fmt, ...)
    print("[belt] " .. line)
    -- ...and into the mod's own log file. print() only reaches the CET console, which cannot be read
    -- from outside the game, and that cost a round trip the first time this file ran.
    pcall(function() spdlog.info(line) end)
end

-- ---------------------------------------------------------------------------------------------------
-- config

-- A ROW OF IDENTICAL PIECES IS ONE THING, not three.
--
-- Three shells side by side are one holder: they are tuned together, they light together, and giving
-- each its own sliders would only be a way to break the row apart. Each member names the leader whose
-- sliders it follows; the leader is an ordinary host and knows nothing about them.
--
-- The spacing itself is NOT here -- it is baked into the three meshes, 2.7 cm apart, because a
-- component transform on a skinned part does not survive the camera. Changing the spacing means
-- re-baking the row, which is three lines of arithmetic in the deploy.
-- EVERY FIREARM IN THE GAME, by the piece of its record name that names the model.
--
-- The belt carries the magazine of the weapon the player actually has -- each weapon its own,
-- anything else looks wrong -- so there is one component and one mesh per entry here, each the
-- weapon's OWN magazine re-bound to the hips. The list was read off the archives rather than written
-- by hand (tools/build_belt_mags.py), which is how it went from the twenty-four I first guessed at to
-- the fifty-one that exist.
--
-- Longest first: `senkoh` has to be tried before `nue` would match something that is not a Nue.
--
-- Missing on purpose: the Defender's magazine mesh carries no bones at all, so there is nothing to
-- bind to the hips and no belt copy of it exists.
local WEAPON_MAG = {
    "copperhead",
    "guillotine",
    "sidewinder",
    "silverhand",
    "palica",
    "satara",
    "lexington",
    "masamune",
    "overture",
    "saratoga",
    "tamayura",
    "yukimura",
    "crusher",
    "kenshin",
    "liberty",
    "ashura",
    "pozhar",
    "pulsar",
    "quasar",
    "senkoh",
    "burya",
    "kappa",
    "kyubi",
    "metel",
    "omaha",
    "ticon",
    "umbra",
    "unity",
    "chao",
    "dian",
    "grit",
    "nova",
    "nue",
}

-- CARRIED ON THE BACK, and therefore deliberately absent from the list above -- by
-- design. A rifle slung over the shoulder is reached for over the shoulder, so a magazine
-- for it on the hip would be a second place the hand has to know about:
--
--   borg authority ajax achilles grad hercules hmg kolac
--   nekomata osprey rasetsu shingen sor22 uragan warden zhuo
--
-- They are gone from the asset too, not just switched off: no mesh, no component, no rule.
--
-- Absent for a different reason: `carnage` and `tactician`. Those are PUMP shotguns, and their
-- ammunition is already on this belt -- the row of loose shells, which `weapon=pump` turns on.
-- Measured, their `mag_std_01` IS a single shell: 2.4 x 7.2 cm on one bone named `mag_std`, and
-- 2.1 x 5.1 for the Tactician. A magazine for them would be the same object twice.
--
-- The break-action pair is not that and stays in the list: a Satara or a Palica takes TWO rounds held
-- together and inserted as one, which is a loading clip and its own object on the belt. Both were
-- briefly swept into the shell row on my inference, and that was wrong.
-- tools/build_belt_mags.py holds the same list in BACK_CARRIED, which is what generated this.

-- The magazines' rules and centres are GENERATED rather than typed: fifty-one pairs of lines that
-- differ only in one word are fifty-one chances to mistype one, and every one of them would fail the
-- same silent way -- a component that never turns on.
--
-- They all start at the belt's own mag pouch. Only one is ever drawn, and the re-bind centred every
-- WHERE EACH MAGAZINE ACTUALLY SITS, read out of its own mesh by tools/belt_centres.py
-- after every bake. They used to share one anchor, which was honest only while they were
-- all un-tuned; each has been placed by hand since, and the hand has to be measured
-- against where the thing IS. A weapon missing here falls back to that shared anchor.
-- WHERE EACH MAGAZINE ACTUALLY SITS, read out of its own mesh by tools/belt_centres.py
-- after every bake. They used to share one anchor, which was honest only while they were
-- all un-tuned; each has been placed by hand since, and the hand has to be measured
-- against where the thing IS. A weapon missing here falls back to that shared anchor.
local MAG_CENTRES = {
    ashura       = { -0.1033,  0.1244,  1.0943 },
    burya        = { -0.1347,  0.1031,  1.0796 },
    chao         = { -0.1197,  0.1337,  1.0735 },
    copperhead   = { -0.1114,  0.1124,  1.0636 },
    crusher      = { -0.0874,  0.1305,  1.0416 },
    dian         = { -0.1292,  0.0963,  1.0067 },
    grit         = { -0.1391,  0.1038,  1.0276 },
    guillotine   = { -0.0905,  0.1327,  1.0136 },
    kappa        = { -0.1045,  0.1293,  1.0730 },
    kenshin      = { -0.1124,  0.1152,  1.0636 },
    kyubi        = { -0.1350,  0.0913,  1.0842 },
    lexington    = { -0.1238,  0.1025,  1.0616 },
    liberty      = { -0.0985,  0.1141,  1.0616 },
    masamune     = { -0.1195,  0.1152,  1.0296 },
    metel        = { -0.1062,  0.1197,  1.0250 },
    nova         = { -0.1045,  0.1327,  1.0796 },
    nue          = { -0.1086,  0.1191,  1.0496 },
    omaha        = { -0.0981,  0.1155,  1.0450 },
    overture     = { -0.1104,  0.1396,  1.0756 },
    palica       = { -0.0907,  0.1363,  1.0951 },
    pozhar       = { -0.1272,  0.0997,  1.0753 },
    pulsar       = { -0.1222,  0.1155,  1.1099 },
    quasar       = { -0.1055,  0.1283,  1.0672 },
    saratoga     = { -0.1200,  0.1244,  1.0568 },
    satara       = { -0.1040,  0.1080,  1.0700 },
    senkoh       = { -0.1075,  0.1272,  1.0556 },
    sidewinder   = { -0.1313,  0.1022,  1.0312 },
    silverhand   = { -0.1248,  0.1145,  1.0891 },
    tamayura     = { -0.1118,  0.1152,  1.0716 },
    ticon        = { -0.1175,  0.1032,  1.0516 },
    umbra        = { -0.1371,  0.1090,  1.0516 },
    unity        = { -0.1121,  0.1051,  1.0676 },
    yukimura     = { -0.1125,  0.1273,  1.0536 },
}

-- mesh on the same point, so one anchor is the honest starting place for all of them.
for _, k in ipairs(WEAPON_MAG) do
    CFG.rules["ucbeltmag_" .. k] = "weaponmag=" .. k
    CFG.centres["ucbeltmag_" .. k] = MAG_CENTRES[k] or { -0.1497, 0.0949, 1.0756 }
end

local OFFSET_GROUP = {
    ucbeltammo_shell2 = "ucbeltammo_shell1",
    ucbeltammo_shell3 = "ucbeltammo_shell1",
}

-- A MAGAZINE DRAWN AS SEVERAL MESHES. The Overture's is its speedloader -- the handle and six rounds, seven meshes
-- (tools/build_belt_mags.py SPECIAL), which one component cannot draw. The handle is the element; the rounds are
-- its row, `_p2` .. `_p7`, placed with it at bake time, shown by its rule, moved by its offset, lit with it and
-- reached for as it.
local MAG_PARTS = { overture = 7 }
for k, n in pairs(MAG_PARTS) do
    for i = 2, n do
        local part = "ucbeltmag_" .. k .. "_p" .. i
        CFG.rules[part] = "weaponmag=" .. k
        OFFSET_GROUP[part] = "ucbeltmag_" .. k
    end
end

-- The sliders of the element this component follows: its row's leader, and for a mark twin the
-- element it marks. A twin that does not follow its element during tuning is a mark left behind.
local MAG_PREFIX = "ucbeltmag_"

-- ma / wa: the NCPD belt pieces come in one mesh per body, and the player's own gender decides which.
local function bodyTag()
    if memo.bodyTag == nil then
        local g = nil
        pcall(function() g = tostring(Game.GetPlayer():GetResolvedGenderName().value) end)
        memo.bodyTag = (g ~= nil and string.find(g, "Female", 1, true) ~= nil) and "wa" or "ma"
    end
    return memo.bodyTag
end

-- WHICH LAYER THE BODY IN THE GAME USES. `bodyTag` is the module's own gender read, already used to
-- pick the ma/wa belt pieces, so this costs nothing and cannot disagree with them.
local function offsetTable()
    if bodyTag() == "wa" and type(CFG.offsets_female) == "table" then return CFG.offsets_female end
    return CFG.offsets
end

local function offsetFor(name)
    local base = string.match(name, "^(.*)_lit$") or name
    -- EVERY MAGAZINE IS TUNED ON ITS OWN. They were briefly collapsed onto one shared entry on the
    -- grounds that the re-bind centres them all on the same point -- but a centre is not a fit: a
    -- sniper magazine is fifteen centimetres of curved box and a revolver round is four, and the
    -- angle each has to hang at differs with its shape. The sharing was an assumption, not a requirement.
    return offsetTable()[OFFSET_GROUP[base] or base]
end

-- THE LEFT-HANDED BELT. The ammunition rides the support hand's hip and the grenades the weapon hand's, so
-- for a left-handed player both change sides: the port's belt is replaced by its mirror (Items.vrp_belt_lh,
-- built by tools/mirror_belt_elements.py) -- the same components under the same names, each element's mesh
-- mirrored across the body. A belt of its own, not an appearance: the garment is merged onto the player when
-- the belt is attached, and naming another appearance later does not merge it again. What was tuned for an
-- element stays true for its mirror, read mirrored: x and the turns about Y and Z change sign. -1 for such an
-- element while the belt worn is the mirror, else 1.
local MIRRORED = { "ucbeltmag_", "ucbeltammo_shell", "ucbeltgrenade" }
local LEFT_HANDED_BELT = { ["Items.vrp_belt"] = "Items.vrp_belt_lh" }
local function mirrorSign(name)
    if not S.beltMirrored then return 1 end
    for _, p in ipairs(MIRRORED) do
        if string.sub(name, 1, #p) == p then return -1 end
    end
    return 1
end

local function leftHanded()
    local ok, yes = pcall(function() return VRLeftHanded() end)
    return ok and yes == true
end

-- The belt this player wears: the variant chosen, or its mirror for a left-handed player where it has one.
local function wantedRecord()
    local rec = CFG.variants[CFG.variant]
    if rec ~= nil and LEFT_HANDED_BELT[rec] ~= nil and leftHanded() then return LEFT_HANDED_BELT[rec] end
    return rec
end

-- Everyone who lights when this one does: the leader and the whole row under it.
local function markFamily(host)
    if host == nil then return {} end
    local lead = OFFSET_GROUP[host] or host
    local out = { lead }
    for member, leader in pairs(OFFSET_GROUP) do
        if leader == lead then out[#out + 1] = member end
    end
    return out
end

local OFFSETS_FILE = "offsets.json"
local OFFSETS_FEMALE_FILE = "offsets_female.json"

-- The version of the variant list, and the index to fall back to when it moves. Kept beside the
-- loader rather than inside CFG so that reading the file cannot change either of them.
local CFG_VARIANT_V = 1
local CFG_VARIANT_DEFAULT = 4

local function loadCfg()
    local f = io.open(CFG_FILE, "r")
    if f == nil then return end
    local text = f:read("*a")
    f:close()
    local ok, data = pcall(function() return json.decode(text) end)
    if not ok or type(data) ~= "table" then return end
    -- MERGED BY KEY, NOT REPLACED, for the two tables that the CODE extends: `centres` and `rules`.
    -- Taking them wholesale means a file saved before a host existed silently deletes it -- which is
    -- exactly what happened to ucbeltgrenade: the component was added, the belt showed it, and it
    -- could never be reached by the hand because its measured centre was wiped by a cfg written the
    -- session before. Same lesson as the props merge below, same shape of bug.
    -- `centres` IS NOT READ BACK. It is not a setting: every entry is the element's own mesh bounding
    -- box, written by tools/belt_centres.py, and it changes whenever a mesh is re-baked. The file is
    -- written at shutdown from whatever was live, so reading it back can only ever pin the value the
    -- meshes had BEFORE the last bake -- which is exactly what happened: six grenades were tuned and
    -- baked, the meshes moved 18 to 34 mm, and the proximity test went on measuring the hand against
    -- where they used to be. The distance measured from who knows where was that.
    local MERGE = { rules = true, offsets = true }
    -- ...and the variant LIST is not a setting either: it is what the module ships, and a saved copy
    -- can only ever be an older, shorter one. Read back, it silently removed the port's own belt from
    -- the list entirely -- which is why switching to it changed nothing on the body.
    local SKIP = { centres = true, variants = true, variant_names = true, slot = true, outfit_slot = true }
    for k, v in pairs(data) do
        if SKIP[k] then
            -- nothing: the code owns it
        elseif k ~= "props" and CFG[k] ~= nil and type(v) == type(CFG[k]) then
            if MERGE[k] and type(v) == "table" then
                for kk, vv in pairs(v) do
                    -- THE PORT'S OWN COMPONENTS KEEP THE CODE'S RULE. Everything else in `rules` is a
                    -- choice a person made about the belt's own pouches and is theirs to keep, but a
                    -- rule for one of ours describes what the element IS -- "show the frag holder when
                    -- a frag grenade is equipped" -- and a stale copy of it in a saved file is a bug,
                    -- not a preference. This exact thing had the frag holder set to `always` while the
                    -- code said `grenade=frag`, so both grenades hung on the belt at once.
                    local mine = (k == "rules"
                        and (string.sub(kk, 1, 13) == "ucbeltgrenade"
                             or string.sub(kk, 1, 10) == "ucbeltammo"
                             or string.sub(kk, 1, 10) == "ucbeltmag_"))
                    if not mine then CFG[k][kk] = vv end
                end
            else
                CFG[k] = v
            end
        end
    end
    -- PROPS ARE MERGED BY SLOT, NEVER REPLACED. The saved file carries a whole prop list, and taking
    -- it wholesale means an old file also restores the old `item` and `slot` -- which is exactly how a
    -- fixed shared-item list came back after the fix and put two props on one item again, so they went
    -- on evicting each other once a second. What a person tunes is the placement and the switches;
    -- which item goes in which slot is the code's business.
    -- The chosen index survives a reload, but not a change to the list beneath it.
    if data.belt_variant_v ~= CFG_VARIANT_V then
        CFG.variant = CFG_VARIANT_DEFAULT
        log("variant list changed -- selection reset to %d (%s)",
            CFG.variant, tostring(CFG.variants[CFG.variant]))
    end

    if type(data.props) == "table" and type(CFG.props) == "table" then
        local saved = {}
        for i = 1, #data.props do
            local sp = data.props[i]
            if type(sp) == "table" and sp.slot ~= nil then saved[sp.slot] = sp end
        end
        for i = 1, #CFG.props do
            local p = CFG.props[i]
            local sp = saved[p.slot]
            if sp ~= nil then
                for _, f in ipairs({ "on", "lit", "auto", "up", "back", "right", "rx", "ry", "rz" }) do
                    if sp[f] ~= nil and type(sp[f]) == type(p[f]) then p[f] = sp[f] end
                end
            end
        end
    end
end

local function saveCfg()
    local ok, text = pcall(function() return json.encode(CFG) end)
    if not ok then return end
    local f = io.open(CFG_FILE, "w")
    if f == nil then return end
    f:write(text)
    f:close()
end

-- ---------------------------------------------------------------------------------------------------
-- handles, resolved once per session. A load screen replaces the systems, so the memo is dropped when
-- the player object goes away -- the same shape the ForceFPP module already uses.

-- Declared HERE and not beside the panel, because onUpdate has to know whether the overlay is open
-- before it decides what is safe to touch. See the pause guard in onUpdate.
local overlay = false

local function clearGarmentCache()
    S.belt = nil
    S.beltHash = nil
    S.comps = {}
    S.want = {}
    S.dumped = false
    -- ...and the belt's own components, which are RAW pointers into the player entity. The player is
    -- destroyed and rebuilt on a save switch, and a pointer kept across that is a read of freed
    -- memory -- an access violation, which is what the crash on loading a save was.
    S.beltParts = nil
    S.beltPartsFor = nil
    S.beltComp = nil
    S.beltCompName = nil
    S.dressed = {}
    S.litOn, S.litFrom, S.litHand = {}, {}, {}
    S.lit = nil
    S.toDress, S.settleAt, S.recheck = false, nil, nil
end

local function memoDrop()
    memo = {}
    -- S.attachPendingAt IS KEPT. A request in flight is the engine's, not this cache's: the player reads nil for a
    -- frame and is attached again around a load, and clearing it there let the next tick ask again -- the second
    -- request superseded the first and the item factory freed its half-loaded belt mesh (read 0x0 at
    -- exe+0xB0C787, 2026-10-29, vrp_belt_gren_ozob). It ends when the belt lands, on detach, or after ATTACH_WAIT;
    -- a request lost with the previous session costs at most that wait.
    startupClothing:Reset()
    S.compsFor = nil
    clearGarmentCache()
end

local function outfitSystem()
    if memo.outfit == nil then
        pcall(function()
            memo.outfit = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentEx.OutfitSystem"))
        end)
        if memo.outfit == nil then
            if not S.outfitMissing then
                S.outfitMissing = true
                log("EquipmentEx is not installed -- the port's own belt goes into its own slot, the Ezio belts cannot be worn")
            end
        else
            S.outfitMissing = false
        end
    end
    return memo.outfit
end

local function transactions()
    if memo.ts == nil then pcall(function() memo.ts = Game.GetTransactionSystem() end) end
    return memo.ts
end

local function equipSystem()
    if memo.es == nil then
        pcall(function()
            memo.es = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
        end)
    end
    return memo.es
end

-- Whether the record lists the slot among its placementSlots: AddItemToSlot refuses any other.
local function listsSlot(rec, slot)
    local found = false
    pcall(function()
        local want = TweakDBID.new(slot)
        for _, id in ipairs(TweakDB:GetFlat(rec .. ".placementSlots") or {}) do
            if id.hash == want.hash and id.length == want.length then found = true return end
        end
    end)
    return found
end

-- THE SLOT THE CHOSEN VARIANT GOES IN: the port's own when its record lists it, EquipmentEx's waist slot
-- otherwise (the Ezio belts list only that one). Per variant, so a colour switch moves slots with it.
local function slotID()
    local i = CFG.variant
    if i < 1 or i > #CFG.variants then i = 1 end
    local key = "slot_" .. tostring(i)
    if memo[key] == nil then
        local name = listsSlot(CFG.variants[i], CFG.slot) and CFG.slot or CFG.outfit_slot
        pcall(function() memo[key] = TweakDBID.new(name) end)
        memo[key .. "_name"] = name
    end
    return memo[key]
end

local function slotName()
    slotID()
    local i = CFG.variant
    if i < 1 or i > #CFG.variants then i = 1 end
    return memo["slot_" .. tostring(i) .. "_name"] or CFG.slot
end

-- Whether a slot item is one of the module's belts (any variant).
local function isOurBelt(item)
    local rec = nil
    pcall(function() rec = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(item:GetItemID()))) end)
    for _, v in ipairs(CFG.variants) do
        if rec == v then return true end
    end
    return false
end

local function variantID()
    local i = CFG.variant
    if i < 1 or i > #CFG.variants then i = 1 end
    local rec = wantedRecord() or CFG.variants[i]
    local key = "variant_" .. rec
    if memo[key] == nil then
        pcall(function() memo[key] = TweakDBID.new(rec) end)
    end
    return memo[key]
end

-- ---------------------------------------------------------------------------------------------------
-- the belt object and its elements

-- Copied verbatim from mods/cet/CyberpunkVRPort_Reload/reload/reload.lua (qrot, qmul) so a recorded
-- rotation composes the same way in both modules.
local function qrot(i, j, k, r, x, y, z)
    local tx = 2.0 * (j * z - k * y)
    local ty = 2.0 * (k * x - i * z)
    local tz = 2.0 * (i * y - j * x)
    return x + r * tx + (j * tz - k * ty),
           y + r * ty + (k * tx - i * tz),
           z + r * tz + (i * ty - j * tx)
end

local function qmul(ax, ay, az, aw, bx, by, bz, bw)
    return aw * bx + ax * bw + ay * bz - az * by,
           aw * by - ax * bz + ay * bw + az * bx,
           aw * bz + ax * by - ay * bx + az * bw,
           aw * bw - ax * bx - ay * by - az * bz
end

-- The Hips bone in bind pose, out of player_man_skeleton.rig. An element's place is stored in the
-- ENTITY's frame (that is how a mesh's bounding box is expressed) and has to be read in the BONE's.
local HIPS_TX, HIPS_TY, HIPS_TZ = 0.0, -0.0151072554, 0.96074301
local HIPS_QI, HIPS_QJ, HIPS_QK, HIPS_QR = 0.701699018, -0.087263152, 0.701704204, -0.0872637928

local function toHipsFrame(x, y, z)
    return qrot(-HIPS_QI, -HIPS_QJ, -HIPS_QK, HIPS_QR, x - HIPS_TX, y - HIPS_TY, z - HIPS_TZ)
end

-- CALLS MADE EVERY FRAME, WITHOUT A CLOSURE EACH. `pcall(function() ... end)` allocates a closure per
-- call, and the mods' per-frame garbage is what CET's collector has to keep up with (see the GC step in
-- CyberpunkVRPort_Stereo). pcall takes these plain functions and their arguments instead.
local function hashOf(obj) return tostring(obj:GetEntityID().hash) end
local function isEnabled(c) return c:IsEnabled() end
local function gamePaused() return Game.GetSystemRequestsHandler():IsGamePaused() end
local function localPosition(c) local v = c:GetLocalPosition() return v.x, v.y, v.z end
local function localOrientation(c) local q = c:GetLocalOrientation() return q.i, q.j, q.k, q.r end
local function planeParamOf(c) return tostring(c.renderingPlaneAnimationParam.value or c.renderingPlaneAnimationParam) end
local function toggle(c, on) c:Toggle(on) end

local function entityHash(obj)
    local ok, h = pcall(hashOf, obj)
    return ok and h or nil
end

-- WHERE THE MESHES ACTUALLY LIVE, measured live -- and the first version of this file had it wrong.
--
-- The item in the slot is a gameGarmentItemObject and it carries only the item wrapper:
-- gameinteractionsComponent, gameStatsComponent, gameScanningComponent, WorkspotMapperComponent,
-- entExternalComponent and friends. NOT ONE mesh. The garment system merges the item entity's seven
-- entGarmentSkinnedMeshComponents onto the PLAYER entity, where they sit among its 239 components under
-- their own names (ucbeltacc, ucbeltmag, ucbeltsoftpouch, ucbelthardpouch, ucbeltdualpouch,
-- ucbeltbagpouch, ucbeltradio).
--
-- So the item in the slot is only the "is the belt on" test, and everything that touches an element goes
-- through the player. Confirmed on the picture: Toggle(false) on ucbeltradio took the radio off the hip,
-- and recolouring ucbeltmag recoloured the magazine pouch.
--
-- The list is re-enumerated when the slot item changes, when a cached handle stops answering, and on a
-- slow timer -- because a garment assembly can be rebuilt underneath us and a handle from before the
-- rebuild points at nothing.
local function beltComponents()
    local ts = transactions()
    local pl = Game.GetPlayer()
    if ts == nil or pl == nil then return nil end

    local obj = nil
    pcall(function() obj = ts:GetItemInSlot(pl, slotID()) end)
    if obj == nil then
        S.belt, S.beltHash, S.comps = nil, nil, {}
        return nil
    end
    S.belt = obj
    local h = entityHash(obj)

    -- Staleness is decided by the slot item's identity and by a timer, and NOT by calling a method on a
    -- cached handle to see whether it answers: pcall does not catch an access violation, so such a
    -- "check" is the crash it was meant to prevent.
    local stale = (h ~= S.beltHash) or (entityHash(pl) ~= S.compsFor) or (next(S.comps) == nil)
        or (os.clock() - (S.recheck or 0.0)) > 5.0
    if not stale then return S.comps end

    -- THE PLAYER'S OWN IDENTITY IS PART OF THE TEST. Loading another save destroys the player and
    -- builds a new one; every component in `S.comps` then belongs to the entity that is gone, while the
    -- belt ITEM can come back looking the same -- so the item alone does not notice. And `S.dressed`,
    -- which records the mesh each host was given, describes hosts that no longer exist: a host rebuilt
    -- with the save wears whatever the asset ships until that record is thrown away.
    local who = entityHash(pl)
    if who ~= S.compsFor then
        S.compsFor = who
        S.dressed = {}
        memo.magByEnt = nil
    end
    S.beltHash = h
    S.recheck = os.clock()
    S.settleAt = os.clock()
    local found = {}
    local list = nil
    pcall(function() list = Game.VREntityComponents(pl) end)
    if list == nil then
        -- No list: the names are known from the asset, so ask for them one at a time.
        for name, _ in pairs(CFG.rules) do
            local c = nil
            pcall(function() c = pl:FindComponentByName(CName.new(name)) end)
            if c ~= nil then found[name] = c end
        end
    else
        for i = 1, #list do
            local nm = nil
            pcall(function() nm = tostring(list[i]:GetName().value) end)
            if nm ~= nil and string.find(nm, CFG.prefix, 1, true) then found[nm] = list[i] end
        end
    end
    S.comps = found

    -- A host keeps its swapped mesh only until the garment is rebuilt, so the swap belongs here, beside
    -- the discovery that detects the rebuild -- and NOT in the tick, where a ChangeResource per element
    -- per frame would be absurd.
    --
    -- The swap itself is deferred to the first tick AFTER the settle window: writing a mesh into a
    -- garment component while its assembly is still being built is the shape of fault this module was
    -- suspected of, and waiting a fifth of a second costs nothing anyone can see.
    -- NOT cleared: what a host is wearing survives a re-enumeration, because the component may well be
    -- the same one. dressElements decides per host by looking at it.
    S.toDress = true
    local count = 0
    for _, _ in pairs(found) do count = count + 1 end
    if count > 0 and not S.dumped then
        S.dumped = true
        local names = {}
        for nm, _ in pairs(found) do names[#names + 1] = nm end
        table.sort(names)
        log("belt elements on the player: %d -- %s", #names, table.concat(names, ", "))
    end
    return S.comps
end

-- ATTACHED, NOT "EQUIPPED", and the difference matters.
--
-- EquipmentEx's public EquipItem() would do this in one call, but it also calls Activate() on the way
-- in, and Activate() is a wardrobe MODE: it clears the visuals of every base clothing slot and clones
-- what the player is wearing into outfit slots. The player looks the same afterwards, but from then on
-- changing clothes in the inventory does not change what is drawn until they go through the outfit
-- wardrobe. That is a real change to how the player's game works, and it is not what "put a belt on the
-- player" should cost.
--
-- The slot is a genuine attachment slot on the player: the port's own is in the player character's
-- `attachmentSlots` (Character.Player_Puppet_Base.yaml), and EquipmentEx adds its OutfitSlots.* records
-- the same way (EquipmentEx.reds, RegisterOutfitSlots). So an item goes in with the engine's own
-- TransactionSystem, with no outfit mode and nothing else touched.
--
-- The item is a PREVIEW item: created from the record, given to the player and attached. It is a visual
-- with no inventory entry and no save weight -- which is right for something that is simply always
-- there. Preview items do not survive a load, so the tick re-asserts it; that costs one GetItemInSlot.
--
-- AddItemToSlot is ASYNCHRONOUS: measured, GetItemInSlot still reads nil in the tick the attach returns
-- true, and the belt is there a moment later. So a success is not re-checked immediately -- otherwise
-- every attach looks like a failure and the next tick attaches a second belt.
local function attachDirect(pl, ts)
    local ok = false
    pcall(function()
        local rid = ItemID.FromTDBID(variantID())
        local pid = ts:CreatePreviewItemID(rid)
        ts:GivePreviewItemByItemID(pl, rid)
        ok = ts:AddItemToSlot(pl, slotID(), pid, true) and true or false
    end)
    return ok
end

-- The mesh swaps, run once per component set and only after it has settled.
local function dressElements()
    if not S.toDress then return end
    if S.settleAt == nil or (os.clock() - S.settleAt) < 0.25 then return end
    S.toDress = false
    for _, el in ipairs(CFG.elements) do
        local host = S.comps[el.host]
        if host ~= nil and el.mesh ~= nil and el.mesh ~= "" then
            -- ALREADY WEARING IT? The element's appearance is ours and nothing else sets it, so it is
            -- the cheapest honest test of "has this component already been dressed" -- and without it
            -- the swap repeated on every re-enumeration, five seconds apart, for the whole session.
            local cur = nil
            pcall(function() cur = tostring(host.meshAppearance.value or host.meshAppearance) end)
            -- A MARKED element counts as dressed. The test is "is it wearing what we put on it", and
            -- while the hand is near it is wearing one of our `lit*` appearances instead -- without this
            -- the re-enumeration would decide it is undressed and run ChangeResource again, which is
            -- exactly the every-five-seconds churn this element system was rebuilt to stop.
            local marked = (cur ~= nil) and (string.sub(cur, 1, 3) == "lit")
            if cur ~= nil and el.appearance ~= nil and (cur == el.appearance or marked) then
                S.dressed[el.host] = el.name
            else
                local path = string.gsub(el.mesh, "{g}", bodyTag())
                local okMesh, okApp = nil, nil
                pcall(function() okMesh = Game.VRComponentChangeResource(host, path, false) end)
                if el.appearance ~= nil and el.appearance ~= "" then
                    pcall(function() okApp = Game.VRComponentChangeAppearance(host, CName.new(el.appearance), false) end)
                end
                if okMesh == true then S.dressed[el.host] = el.name end
                pcall(function()
                    host:SetLocalPosition(Vector4.new(el.ox or 0.0, el.oy or 0.0, el.oz or 0.0, 1.0))
                end)
                log("element %s dressed on %s: mesh=%s appearance=%s (was %s)",
                    el.name, el.host, tostring(okMesh), tostring(okApp), tostring(cur))
            end
        elseif host == nil then
            log("element %s wants host %s, which the belt does not have", el.name, tostring(el.host))
        end
    end
end

-- NAME THE APPEARANCE OURSELVES, and here is the measurement that says we have to.
--
-- With the port's own belt in the waist slot the game reported `appearance = default` and FOUR mesh
-- components short of zero -- nothing drawn at all -- while the entity's four appearances
-- (`vrp_belt_&Male&TPP` and friends) were sitting right there. Asking for one of them by hand made
-- the belt appear with its meshes. So the entity, the .app, the factory row and the item record are
-- all correct; what does not happen is the pipeline composing `<appearanceName><Gender><Camera>` and
-- setting it, and it falls back to `default`, which no belt entity of ours defines.
--
-- Rather than keep guessing at why, the module names it: it already knows the body, it already owns
-- the equip, and one call is cheaper than another archive rebuild. TPP first because that is what
-- drew when it was tried by hand; FPP is the fallback for a body that only ships that one.
local function forceBeltAppearance(pl, ts)
    if not CFG.force_appearance then return end
    local o = nil
    pcall(function() o = ts:GetItemInSlot(pl, slotID()) end)
    if o == nil then return end
    -- The stem is the item's own name plus an underscore -- `Items.vrp_belt` -> `vrp_belt_`,
    -- `Items.ucbelt_d` -> `ucbelt_d_` -- which holds for all four variants and needs no TweakDB read
    -- whose debug formatting would have to be parsed back.
    local rec = wantedRecord() or ""
    local stem = string.match(rec, "^Items%.(.+)$")
    if stem == nil then return end
    stem = stem .. "_"
    local body = (bodyTag() == "wa") and "Female" or "Male"
    -- ONE OF OURS OR NOT, and the test is exactly that. Keying it on the single value `default` was
    -- a guess about how it fails, and it made the whole thing silent the moment it failed some other
    -- way: the probe had left `vrp_belt_&Male&TPP` on a female body, which is neither `default` nor
    -- right, so the function returned before it could say a word.
    local now = nil
    pcall(function() now = tostring(o:GetCurrentAppearanceName().value) end)
    now = now or ""
    -- What the belt wears, not what was asked: its elements are where its appearance put them.
    S.beltMirrored = string.find(now, "_lh_&", 1, true) ~= nil
    local want = string.format("%s&%s&TPP", stem, body)
    if now == want then
        if S.appSaid ~= now then
            S.appSaid = now
            log("belt appearance is %s", now)
        end
        return
    end
    -- ONE REQUEST PER TICK, and the read-back is not the verdict.
    --
    -- The name says it: `ScheduleAppearanceChange` SCHEDULES. Reading the appearance back in the same
    -- tick returns the OLD one every time, so a loop that asks TPP, reads stale, then asks FPP simply
    -- overwrites its own first request -- which is exactly what the log showed: two "became default"
    -- lines and the belt landing on FPP. So one request goes out per call and the next call, two
    -- seconds later, judges it.
    local w = string.format("%s&%s&TPP", stem, body)
    if S.appAsked == w and now ~= "" and now ~= "default" then
        -- TPP was asked for and something else stuck: this body only ships the other camera.
        w = string.format("%s&%s&FPP", stem, body)
    end
    S.appAsked = w
    pcall(function() o:ScheduleAppearanceChange(CName.new(w)) end)
    log("belt appearance: было %s, прошу %s", now, w)
end

-- THE ITEM'S REAL ID, out of the player's own inventory.
--
-- Measured, and it is the whole reason the belt never reached the body: the module handed
-- `EquipItem` a TweakDBID, and on the retry an `ItemID.FromTDBID`, which is an id with NO SEED.
-- EquipmentEx answers `true` to both and then does not consider the item equipped -- its own
-- `IsEquipped` read false while the transaction slot happily held the belt, so the garment was never
-- merged onto the player. Handing it an id taken from the inventory flipped `IsEquipped` to true in
-- one call, live.
--
-- The item is added if the player does not carry one; `AddToInventory` is additive and safe. The id
-- is NOT cached: an item can be dropped, sold or replaced, and a stale id is the same bug again.
local function inventoryItemID(pl, ts, rec)
    local found = nil
    pcall(function()
        local _, list = ts:GetItemList(pl)
        if type(list) ~= "table" then return end
        for _, d in ipairs(list) do
            local nm = nil
            pcall(function() nm = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(d:GetID()))) end)
            if nm == rec then found = d:GetID() return end
        end
    end)
    return found
end

-- THE BELT'S OWN GARMENT, once the wardrobe has merged it onto the player. It is NOT on the item any
-- more at that point -- a merged garment moves -- so it is looked up on the player and cached by name,
-- the way every other per-frame component lookup in this port is.
-- WHICH SET OF BELT SETTINGS THIS BODY USES. Same rule as the offsets: exactly one is applied, never
-- a sum, and the panel edits whichever is in force.
local function beltAllInOne()
    if bodyTag() == "wa" then return CFG.belt_all_in_one_female ~= false end
    return CFG.belt_all_in_one ~= false
end

local function beltPieceTable()
    if bodyTag() == "wa" and type(CFG.belt_pieces_female) == "table" then
        return CFG.belt_pieces_female
    end
    return CFG.belt_pieces or {}
end

local function beltChunkTable()
    if bodyTag() == "wa" and type(CFG.belt_chunk_off_female) == "table" then
        return CFG.belt_chunk_off_female
    end
    return CFG.belt_chunk_off or {}
end

-- The mask the table adds up to. Bit i is chunk i, set means drawn.
local function beltChunkMask()
    local m = 0
    local off = beltChunkTable()
    for i = 0, 31 do
        if off[i + 1] ~= true then m = m + 2 ^ i end
    end
    return m
end

-- Written only when it differs, like every other live field here. `chunkMask` is a plain component
-- field, so it takes at runtime -- unlike a mesh or a material, which the renderer keeps.
-- THE ORDER THE PANEL SHOWS THEM IN, and the only place the list is written down.
local BELT_PIECES = {
    "strap", "back",
    "mag", "mag_l", "mag_r", "mag_back",
    "pouch_small", "pouch_l", "pouch_r",
    "bag_front_l", "bag_front_r", "bag_back_l", "bag_back_r",
    "radio", "radio_front", "torch", "torch_front",
    "gren_front", "gren_mid", "gren_back",
}

-- Every `vrp_belt*` component the player is wearing, by name. Cached, and thrown away the moment the
-- cached belt component stops being one -- a garment survives a wardrobe change by being rebuilt, and
-- a stale component pointer is written into nothing.
local function beltParts(pl, who)
    -- THE CACHE IS VALIDATED ON ITSELF, and the first version was not: it kept the map as long as
    -- `S.beltComp` -- a DIFFERENT variable, filled by another function -- was alive. The first scan
    -- runs before the garment is merged, so the map is empty; the next tick `S.beltComp` exists and the
    -- empty map is returned for the rest of the session. Toggling from the panel then did nothing while
    -- the same Toggle from outside worked, which is exactly what was reported.
    -- KEYED ON THE PLAYER, not on whether one entry still answers IsDefined. A freed component can
    -- still pass that test, and on a save switch every one of these belongs to an entity that no
    -- longer exists. The entity hash changes the moment the player does, and that is the only signal
    -- worth trusting here.
    who = who or entityHash(pl)
    local cached = S.beltParts
    if cached ~= nil and S.beltPartsFor == who and who ~= nil then
        local ok, empty = false, true
        for _, c in pairs(cached) do
            empty = false
            ok = IsDefined(c)
            break
        end
        if ok then return cached end
        -- NO BELT WORN IS AN ANSWER TOO. An empty map failed the test above, so the walk below ran again
        -- every frame: ~280 components and a name string each, 2 ms of the game thread for nothing. It is
        -- looked for again once a second, which still finds a belt put on later.
        if empty and os.clock() - (S.beltPartsScan or -1.0) < 1.0 then return cached end
    end
    S.beltComp, S.beltCompName = nil, nil
    local map = {}
    pcall(function()
        for _, c in ipairs(Game.VREntityComponents(pl)) do
            local nm = tostring(c:GetName().value)
            if string.sub(nm, 1, 8) == "vrp_belt" then map[nm] = c end
        end
    end)
    S.beltParts = map
    S.beltPartsFor = who
    S.beltPartsScan = os.clock()
    return map
end

-- The component names, built once: the all-in-one first, then the pieces in panel order.
local BELT_PIECE_NAMES = { "vrp_belt" }
for _, k in ipairs(BELT_PIECES) do BELT_PIECE_NAMES[#BELT_PIECE_NAMES + 1] = "vrp_belt_" .. k end

local function chunkMaskOf(c) return c.chunkMask end

-- Internal A/B controls; none of these switches retains native component handles.
-- legacyBeltWalk: every belt piece reads IsEnabled() and the worn piece's chunkMask every frame.
local PERFORMANCE = { legacyHighlightScan = false, legacyEmptyPropPoll = false, legacyBeltWalk = false }

-- How many pieces re-read IsEnabled() per frame. A piece changes by our own Toggle, which the walk
-- records, or when the garment is rebuilt, which replaces the parts map and with it these records;
-- three a frame still re-reads each of the 21 every seven frames, against a change from outside.
-- IsDefined stays on every piece every frame.
local PIECE_READS_PER_FRAME = 3
-- The worn piece's chunkMask is read again every eighth frame while the piece and the mask stay the same.
local MASK_READ_FRAMES = 8

-- One read, a write only when it differs -- the same shape as every other live toggle here.
--
-- Returns the piece that is on afterwards, the all-in-one first and then the pieces in panel order,
-- so the answer is stable rather than whatever `pairs` happens to yield: WHICHEVER BELT PIECE IS ON is
-- the one a chunk mask can do anything to. It used to be hardcoded to `vrp_belt`, the all-in-one --
-- and the moment the belt switched to the bare `strap` the mask section became a row of ticks acting
-- on a component nobody was wearing. NO FAST PATH ON `S.beltComp`: it was validated with `IsDefined`
-- and a call to `IsEnabled`, and calling anything on a component whose entity has been destroyed is
-- the fault itself -- the check WAS the crash. The answer comes from this walk, every frame; the
-- chunk mask used to walk the pieces and call IsEnabled on each of them a second time.
-- `who` is the player's entity hash when the caller already has it this frame.
local function applyBeltPieces(pl, who)
    local map = beltParts(pl, who)
    local pieces = beltPieceTable()
    if S.pieceMap ~= map then S.pieceMap, S.pieceOn = map, {} end
    local known = S.pieceOn
    local count = #BELT_PIECE_NAMES
    local first = S.pieceRead or 0
    S.pieceRead = (first + PIECE_READS_PER_FRAME) % count
    local worn, wornName = nil, nil
    for i, nm in ipairs(BELT_PIECE_NAMES) do
        local on
        if i == 1 then on = beltAllInOne() else on = (pieces[BELT_PIECES[i - 1]] == true) end
        local c = map[nm]
        if c ~= nil and IsDefined(c) then
            local is = known[nm]
            if PERFORMANCE.legacyBeltWalk or is == nil or (i - 1 - first) % count < PIECE_READS_PER_FRAME then
                local okE
                okE, is = pcall(isEnabled, c)
                if not okE then is = nil end
            end
            local now = is
            if is ~= on and pcall(toggle, c, on) and is ~= nil then now = on end
            known[nm] = now
            if now == true and worn == nil then worn, wornName = c, nm end
        end
    end
    return worn, wornName
end

-- `c` is the piece applyBeltPieces left on this frame.
local function applyBeltChunks(c, name)
    S.beltComp, S.beltCompName = c, name
    if c == nil then return end
    local want = beltChunkMask()
    if not PERFORMANCE.legacyBeltWalk and c == S.maskComp and want == S.maskWant and (S.maskSkip or 0) > 0 then
        S.maskSkip = S.maskSkip - 1
        return
    end
    S.maskComp, S.maskWant, S.maskSkip = c, want, MASK_READ_FRAMES - 1
    local okM, have = pcall(chunkMaskOf, c)
    if not okM then have = nil end
    if have ~= nil and tostring(have) == tostring(want) then return end
    pcall(function() c.chunkMask = want end)
    -- ...and a nudge, because a field the renderer already read does not re-read itself.
    pcall(function() c:Toggle(false) c:Toggle(true) end)
end

-- A SECOND COPY IN EQUIPMENTEX'S WAIST SLOT. Before the port had its own slot the belt went into
-- OutfitSlots.Waist, and an EquipmentEx outfit saved then can still hold it. With the belt worn in the
-- port's slot the same record there is a second belt in the same place, so it is taken off through the
-- wardrobe (its outfit forgets it) or the TransactionSystem. Only that exact record: anything else the
-- player wears at the waist is theirs. One GetItemInSlot, and nothing when EquipmentEx is absent.
local function dropOutfitDuplicate(pl, ts, want)
    if slotName() == CFG.outfit_slot then return end
    local id, o = nil, nil
    pcall(function() id = TweakDBID.new(CFG.outfit_slot) o = ts:GetItemInSlot(pl, id) end)
    if o == nil then return end
    local rec = nil
    pcall(function() rec = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(o:GetItemID()))) end)
    if rec ~= want then return end
    local off = nil
    local outfit = outfitSystem()
    if outfit ~= nil then pcall(function() off = outfit:UnequipSlot(id) end) end
    if off ~= true then pcall(function() ts:RemoveItemFromSlot(pl, id, true) end) end
    log("%s was also in %s (an EquipmentEx outfit from before the port's own slot) -- taken off there", rec, CFG.outfit_slot)
end

-- Seconds an accepted belt attach is given to land before it is asked for again.
local ATTACH_WAIT = 30.0
-- ONE BELT ON THE PLAYER AT A TIME. Taking a belt off and putting another on are both asynchronous, and asked
-- in the same tick the player's appearance change took both belts' garments at once. The game's garment pass
-- numbers the player's garment elements in one byte (256 at most, tools/belt_lit_shell.py), and a change counts
-- the worn set plus the changing item, so two belts in one pass is the case that runs out first. The next belt
-- goes on once the slot is empty and this long after the last one came off.
local SWAP_SETTLE = 1.5

local function ensureEquipped(now)
    if not CFG.auto_equip then return end
    if now - S.lastEquipTry < 2.0 then return end
    S.lastEquipTry = now

    local pl = Game.GetPlayer()
    local ts = transactions()
    if pl == nil or ts == nil then return end

    -- WHICH belt is on, not merely whether one is.
    --
    -- This used to return the moment the slot held anything at all, which is why switching the module
    -- to its own belt changed nothing on the body: the Ezio one was already in the waist slot, so the
    -- equip was never attempted and the old belt simply stayed. "Keep the belt on" has to mean keep
    -- THIS belt on, or the setting is a setting that does nothing.
    local have = nil
    pcall(function() have = ts:GetItemInSlot(pl, slotID()) end)
    if have ~= nil then
        S.attachPendingAt = nil
        local want = wantedRecord()
        local worn = nil
        pcall(function() worn = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(have:GetItemID()))) end)
        if worn == nil or worn == want then
            -- ...and the right belt still needs its appearance named. The log said the swap had
            -- happened and `Items.vrp_belt` was in the slot, yet nothing was drawn: the naming only
            -- ran after a fresh equip, and by then the equip never happened again because the slot
            -- already held the right thing. A fix that only runs on a transition does not fix a
            -- state.
            forceBeltAppearance(pl, ts)
            dropOutfitDuplicate(pl, ts, want)
            return
        end
        -- The wrong one: take it off and let the equip below put the right one on. Logged always --
        -- a belt swapping itself under the player is worth one line whatever `verbose` says.
        log("waist holds %s, wanted %s -- swapping", tostring(worn), tostring(want))
        -- In EquipmentEx's slot the wardrobe takes it off, so its outfit forgets it too. `UnequipSlot` is
        -- the one that takes a slot; `UnequipItem` takes an item and answered nil to a slot, which is how
        -- the old belt kept its place.
        local o2 = (slotName() == CFG.outfit_slot) and outfitSystem() or nil
        local off = nil
        if o2 ~= nil then
            pcall(function() off = o2:UnequipSlot(slotID()) end)
            if off ~= true then pcall(function() off = o2:UnequipItem(have:GetItemID()) end) end
        end
        if off ~= true then
            pcall(function() ts:RemoveItemFromSlot(pl, slotID(), false) end)
        end
        -- ...and the right one on a later tick, once this one is off (SWAP_SETTLE).
        S.detachedAt = now
        return
    end
    if S.detachedAt ~= nil then
        if now - S.detachedAt < SWAP_SETTLE then return end
        S.detachedAt = nil
    end

    -- ONE REQUEST IN FLIGHT. An accepted attach can take far longer than this 2 s tick while a save
    -- loads (tens of seconds under a capture tool). Asking again meanwhile replaced the spawn in flight,
    -- and the engine's item factory then released the half-loaded belt meshes (the lit twins' external
    -- materials still loading) and crashed reading a null material handle. Measured: 16-22 requests per
    -- load and two crashes in the factory tick, mesh vrp_belt_gren_recon. Wait for that request instead.
    if S.attachPendingAt ~= nil and now - S.attachPendingAt < ATTACH_WAIT then return end

    if CFG.route ~= "outfit" then
        if attachDirect(pl, ts) then
            -- AddItemToSlot accepted an asynchronous request. A still-empty
            -- slot here must not activate EquipmentEX and hide base clothing.
            S.attachPendingAt = now
            S.route = "attach"
            log("queued direct attachment of %s into %s", wantedRecord(), slotName())
            return
        end
    end

    -- Fall back to EquipmentEx's own path, for a belt that goes in its slot. It works where the direct
    -- attach does not -- and it is also what a player who WANTS the wardrobe to own the belt would choose
    -- (route = "outfit"). The port's own belt lists only the port's slot, which the wardrobe does not
    -- manage.
    if slotName() ~= CFG.outfit_slot then return end
    local outfit = outfitSystem()
    if outfit == nil then return end
    local rec = wantedRecord()
    local iid = inventoryItemID(pl, ts, rec)
    if iid == nil then
        pcall(function() Game.AddToInventory(rec, 1) end)
        iid = inventoryItemID(pl, ts, rec)
    end
    if iid == nil then
        log("no %s in the inventory and AddToInventory did not make one -- cannot equip", rec)
        return
    end
    local ok = nil
    pcall(function() ok = outfit:EquipItem(iid, slotID()) end)
    -- ...and say whether EquipmentEx AGREES, which is the thing that was silently false.
    local eq = nil
    pcall(function() eq = outfit:IsEquipped(iid) end)
    log("equip %s -> %s, EquipmentEx counts it as equipped: %s", rec, tostring(ok), tostring(eq))
    S.route = "outfit"
    forceBeltAppearance(pl, ts)
end

-- Off from BOTH slots: the colour switch changes the variant, and with it the slot, before it takes the
-- old belt off. Only the module's own belts are touched; EquipmentEx's wardrobe unequips the one in its
-- slot, so its outfit forgets it as well.
detach = function()
    S.attachPendingAt = nil -- the next equip asks for the variant wanted now
    local pl = Game.GetPlayer()
    local ts = transactions()
    if pl ~= nil and ts ~= nil then
        for _, name in ipairs({ CFG.slot, CFG.outfit_slot }) do
            local id, o = nil, nil
            pcall(function() id = TweakDBID.new(name) o = ts:GetItemInSlot(pl, id) end)
            if o ~= nil and isOurBelt(o) then
                local off = nil
                local outfit = (name == CFG.outfit_slot) and memo.outfit or nil
                if outfit ~= nil then pcall(function() off = outfit:UnequipSlot(id) end) end
                if off ~= true then pcall(function() ts:RemoveItemFromSlot(pl, id, true) end) end
                S.detachedAt = os.clock()
            end
        end
    end
    S.belt, S.beltHash, S.comps, S.want = nil, nil, {}, {}
end

-- ---------------------------------------------------------------------------------------------------
-- what the player is carrying, and therefore what shows on the belt

-- Whether the thing in the gadget slot is a grenade rather than some other gadget. Read off the
-- record, because "the gadget area is always grenades" is an assumption and this costs one flat read.
local function isGrenade(id)
    local grenade = nil
    pcall(function()
        local tdbid = ItemID.GetTDBID(id)
        local t = TweakDB():GetFlat(TweakDBID.new(tdbid, ".itemType"))
        if t ~= nil then
            local name = tostring(t.value or t)
            grenade = string.find(name, "Gad_Grenade", 1, true) ~= nil
        end
    end)
    if grenade == nil then return true end   -- record unreadable: the gadget slot is grenades in practice
    return grenade
end

-- WHAT KIND OF AMMUNITION A WEAPON EATS, keyed by the game's OWN gamedataItemType number.
--
-- The numbers are read straight out of the game's script dump rather than matched on a record name:
-- a name is a guess that breaks on the next weapon whose record is called something unexpected, and
-- there are fifty of them. `Wea_Handgun` is 81 in this build and every weapon in the game answers
-- with one of these, so a class is a lookup and not a search.
--
-- An unmapped type -- melee, the grenade launcher, a cyberarm -- simply has no holder, which is the
-- right answer for it.
-- BY NAME: the enum's numbers are the build's, and 2.5 moved them all by one (Wea_Rifle 92 -> 93), which made a
-- revolver a rifle and an assault rifle nothing. They are read off the running game once (weaponClassOf).
local WEAPON_CLASS_BY_NAME = {
    Wea_Handgun = "pistol",
    Wea_Revolver = "pistol",
    Wea_Shotgun = "shotgun",
    Wea_ShotgunDual = "shotgun",
    Wea_AssaultRifle = "rifle",
    Wea_Rifle = "rifle",
    Wea_PrecisionRifle = "rifle",
    Wea_SniperRifle = "rifle",
    -- A submachine gun feeds from a stick magazine and reads as one on a belt, so it rides the rifle
    -- pouch rather than the pistol one even though its round is a pistol round. Same for the two
    -- machine guns. Move a line if the picture disagrees -- that is all this table is.
    Wea_SubmachineGun = "rifle",
    Wea_LightMachineGun = "rifle",
    Wea_HeavyMachineGun = "rifle",
}
local WEAPON_CLASS = nil

-- tonumber: CET hands EnumInt back as a 64-bit cdata (94ULL), and a cdata is never equal to the plain number key it
-- prints as -- so a lookup by it missed on every weapon and no `weapon=<class>` holder was ever shown.
local function weaponClassOf(itemType)
    if WEAPON_CLASS == nil then
        WEAPON_CLASS = {}
        for name, cls in pairs(WEAPON_CLASS_BY_NAME) do
            local ok, v = pcall(function() return tonumber(EnumInt(gamedataItemType[name])) end)
            if ok and v ~= nil then WEAPON_CLASS[v] = cls end
        end
    end
    return WEAPON_CLASS[tonumber(EnumInt(itemType))]
end

-- THE GRENADE KINDS, as substrings of the record name. One list, used both to pick the belt element
-- that is shown and, further down, to pick the prop entity -- they must not drift apart.
local GRENADE_KINDS = { "frag", "flash", "incendiary", "biohazard", "recon", "cutting",
                        "smoke", "emp", "ozob" }

-- MODELS THAT CARRY ANOTHER MODEL'S AMMUNITION. The Igla and the Testera are built on the Satara and
-- load the Satara's own two shells -- their magazine .app files point at
-- w_shotgun_dual__rostovic_satara__mag_std_01_left_01.mesh and _right_01.mesh, nothing of their own --
-- so the pouch for them is the Satara's pair, not a second copy of it.
local MAG_ALIAS = {
    igla    = "satara",
    testera = "satara",
}

-- THE PUMPS, which have no magazine of their own on this belt -- their ammunition is the row of loose shells. Named
-- so that a pump in hand is still an answer: with none, the belt fell back to the loadout's next weapon and hung
-- that one's magazine on the pouch -- an Ashura's under a Tactician.
local PUMP = "pump"
local PUMP_MODELS = { "carnage", "tactician" }

-- The model this string names, or nil.
local function magKeyIn(name)
    if name == nil then return nil end
    local low = string.lower(name)
    for _, k in ipairs(PUMP_MODELS) do
        if string.find(low, k, 1, true) then return PUMP end
    end
    for _, k in ipairs(WEAPON_MAG) do
        if string.find(low, k, 1, true) then return k end
    end
    for k, to in pairs(MAG_ALIAS) do
        if string.find(low, k, 1, true) then return to end
    end
    return nil
end

-- WHICH WEAPON THIS OBJECT IS, read off its OWN components.
--
-- The record it came from does not know. A gun looted off the world carries a name like
-- `Items.VHard_50_CoolRef_Weapon5`, which names no model at all, and its `FriendlyName` is worse than
-- useless -- measured on a Tamayura in the player's hand, that field read `w_handgun_tsunami_nue`.
-- The components are the asset itself and cannot lie: `w_handgun__arasaka_tamayura__mag_std_01...`.
--
-- This is the same identification the reload module makes (its `cfgFor`), for the same reason.
--
-- Cached by entity, because enumerating a weapon's components every frame is not free and the answer
-- cannot change for a given object. Only positive answers are kept: a weapon whose components have
-- not finished streaming on the first frame would otherwise stay unidentified for good.
local function magKeyOfWeapon(obj)
    if obj == nil then return nil end
    -- MEMOISED BY THE RECORD, NOT BY THE ENTITY.
    --
    -- Entity ids are REUSED: load another save and a fresh weapon can land on the hash a different one
    -- had, so the cache answers with the previous save's magazine -- a Silverhand in hand and a kappa
    -- pouch on the belt, which is exactly what was reported. A record cannot collide that way, and two
    -- weapons that share a record genuinely share a magazine.
    local h = nil
    pcall(function() h = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(obj:GetItemID()))) end)
    if h ~= nil then
        if memo.magByEnt == nil then memo.magByEnt = {} end
        if memo.magByEnt[h] ~= nil then return memo.magByEnt[h] end
    end
    -- THE MAGAZINE'S OWN COMPONENTS FIRST. A weapon is assembled from other weapons' parts often enough that the
    -- first name to match is not the weapon's: the Tactician carries an Ashura decal
    -- (w_rifle_sniper__tsunami_ashura__base1_01_decal_01_d) and the Palica the Satara's receiver
    -- (w_shotgun_dual__rostovic_satara__rcv1_01), and each hung the wrong model's ammunition on the belt. The
    -- `__mag_` parts name the gun the magazine belongs to; anything else only when no magazine part names one.
    local key = nil
    pcall(function()
        local cs = Game.VREntityComponents(obj)
        for pass = 1, 2 do
            for _, c in ipairs(cs) do
                local n = tostring(c:GetName().value)
                if pass == 2 or string.find(n, "__mag_", 1, true) then
                    local k = magKeyIn(n)
                    if k ~= nil then key = k break end
                end
            end
            if key ~= nil then break end
        end
    end)
    if key ~= nil and h ~= nil then memo.magByEnt[h] = key end
    return key
end

-- The `weapon=<class>` holders one weapon puts on the belt: its class, and `pump` for a pump gun (`mk`, its magazine
-- key, says which).
local function weaponClasses(id, mk)
    local cls = {}
    local rec = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(id))
    if rec ~= nil then
        local c = weaponClassOf(rec:ItemType():Type())
        if c ~= nil then cls[c] = true end
    end
    if mk == PUMP then cls.pump = true end
    return cls
end

local function readState()
    local st = { grenade = false, weapon = false, inhand = false, tier = 0, gtype = nil,
                 wclass = {}, wmag = nil, gcount = nil }
    local pl = Game.GetPlayer()
    if pl == nil then return st end

    local es = equipSystem()
    if es ~= nil then
        pcall(function()
            local id = es:GetItemInEquipSlot(pl, "Gadget", 0)
            st.grenade = ItemID.IsValid(id) and isGrenade(id)
        end)
        -- WHICH KIND, from the active gadget's record name. The belt shows the grenade that is
        -- actually equipped, so a flashbang on the belt is a flashbang and not the frag holder the
        -- police belt ships -- which is the only one the game has a belt mesh for.
        pcall(function()
            local eq = equipSystem()
            local id = eq:GetPlayerData(pl):GetActiveGadget()
            if id ~= nil and ItemID.IsValid(id) then
                local nm = TDBID.ToStringDEBUG(ItemID.GetTDBID(id))
                if nm ~= nil then
                    local low = string.lower(nm)
                    for _, k in ipairs(GRENADE_KINDS) do
                        if string.find(low, k, 1, true) then st.gtype = k break end
                    end
                end
            end
        end)
        -- HOW MANY THROWS ARE LEFT, which is a CHARGE count and not an item count.
        --
        -- Both of the obvious sources are wrong. The equipment slot keeps naming a grenade long after
        -- the last throw -- it is the TYPE that is equipped, not a stack. And the inventory quantity
        -- does not move when a grenade is thrown at all: measured, 1 frag in the inventory and 2
        -- charges available, and the two are simply different things since 2.0.
        pcall(function() st.gcount = pl:GetGrenadeCharges() end)

        -- ...and the real source, because that area is empty in this version of the game.
        if not st.grenade then
            local ts2 = transactions()
            if ts2 ~= nil then
                pcall(function()
                    st.grenade = ts2:GetItemQuantityByTag(pl, CName.new("Grenade")) > 0
                end)
            end
        end
        -- THE BELT CARRIES ONE WEAPON'S AMMUNITION: the one in hand (below), or with everything holstered the
        -- loadout's first. Every weapon of the loadout at once put a shotgun's shells and a pistol's magazine on it
        -- together, and drawing one of them changed nothing.
        for slot = 0, 2 do
            if st.weapon then break end
            pcall(function()
                local id = es:GetItemInEquipSlot(pl, "Weapon", slot)
                if ItemID.IsValid(id) then
                    st.weapon = true
                    -- A HOLSTERED WEAPON IS ONLY AN ItemID, with no object to read components off,
                    -- so all that is left for it is the record name. That works for a gun bought in a
                    -- shop and fails for one looted off the world -- but it can only ever produce the
                    -- right answer or none, so it stands in until the weapon is actually drawn.
                    st.wmag = magKeyIn(TDBID.ToStringDEBUG(ItemID.GetTDBID(id)))
                    st.wclass = weaponClasses(id, st.wmag)
                end
            end)
        end
    end

    -- In hand is a different question from equipped, and it is the one a magazine pouch cares about.
    -- Same slot the reload module already reads.
    local ts = transactions()
    if ts ~= nil then
        pcall(function()
            if memo.slotWeaponRight == nil then
                memo.slotWeaponRight = TweakDBID.new("AttachmentSlots.WeaponRight")
                memo.slotWeaponLeft = TweakDBID.new("AttachmentSlots.WeaponLeft")
            end
            -- The left-handed hold carries a single weapon in WeaponLeft.
            local w = ts:GetItemInSlot(pl, memo.slotWeaponRight) or ts:GetItemInSlot(pl, memo.slotWeaponLeft)
            st.inhand = w ~= nil
            -- THE WEAPON IN HAND WINS. It is the one about to be reloaded, and it is the one the belt
            -- should be carrying ammunition for; the loadout's first weapon is only the fallback for
            -- when everything is holstered. Its magazine from the record name when its parts name none.
            if w ~= nil then
                local id = w:GetItemID()
                local k = magKeyOfWeapon(w) or magKeyIn(TDBID.ToStringDEBUG(ItemID.GetTDBID(id)))
                st.wmag = k
                st.wclass = weaponClasses(id, k)
            end
        end)
    end

    -- The cutscene signal is the one the rest of the port already uses: SceneTier off the player state
    -- machine blackboard, not GetSceneSystemCameraControlEnabled, which reads true in plain gameplay.
    pcall(function()
        if memo.bbDefs == nil then memo.bbDefs = Game.GetAllBlackboardDefs() end
        if memo.psm == nil then
            memo.psm = Game.GetBlackboardSystem():GetLocalInstanced(pl:GetEntityID(), memo.bbDefs.PlayerStateMachine)
        end
        if memo.psm ~= nil then st.tier = memo.psm:GetInt(memo.bbDefs.PlayerStateMachine.SceneTier) end
    end)
    return st
end

-- Which element, if any, this host is currently wearing. One place, so the panel and the tick cannot
-- disagree about whose rule counts -- they did, and the buttons silently wrote a value nothing read.
local function elementOn(host)
    for _, el in ipairs(CFG.elements) do
        if el.host == host and S.dressed[host] == el.name then return el end
    end
    return nil
end

local function ruleFor(host)
    local el = elementOn(host)
    if el ~= nil then return el.rule or "always", true end
    return CFG.rules[host] or "always", false
end

local function setRule(host, value)
    local el = elementOn(host)
    if el ~= nil then el.rule = value else CFG.rules[host] = value end
    S.want[host] = nil
end

local function wanted(rule, st)
    -- "grenade=flash" shows the element only while that kind is the equipped one. The plain "grenade"
    -- rule still means any grenade at all, which is what a pouch wants and what a holder for one
    -- specific kind does not.
    -- NOTHING ON THE BELT WHEN THERE IS NOTHING TO SHOW. `st.gcount` is nil when the count could not
    -- be read at all, and only a real zero hides the element -- a failed read must not empty the belt.
    local none = (st.gcount ~= nil and st.gcount <= 0)
    local kind = string.match(rule or "", "^grenade=(.+)$")
    if kind ~= nil then return (not none) and st.grenade and st.gtype == kind end
    -- "weapon=shotgun" shows the element while the belt carries a weapon of that class (readState: the one in
    -- hand, or with everything holstered the loadout's first).
    -- The plain "weapon" rule still means any weapon at all, which is what a general-purpose pouch
    -- wants and what a holder for one kind of ammunition does not.
    local cls = string.match(rule or "", "^weapon=(.+)$")
    if cls ~= nil then return st.wclass ~= nil and st.wclass[cls] == true end
    -- "weaponmag=copperhead" shows that weapon's own magazine, and only while it is the weapon whose
    -- ammunition the belt is carrying. Exactly one of the fifty-one is ever true.
    local mk = string.match(rule or "", "^weaponmag=(.+)$")
    -- ...and it is EMPTY while the player is carrying it. The reload module says so; a pouch that
    -- still shows a magazine the hand is holding is the same lie as a grenade left on the belt after
    -- it was thrown.
    if mk ~= nil then return (not S.magTaken) and st.wmag == mk end
    if rule == "always" then return true end
    if rule == "never" then return false end
    if rule == "grenade" then return (not none) and st.grenade end
    if rule == "weapon" then return st.weapon end
    if rule == "inhand" then return st.inhand end
    return true
end

-- Compared against what the component ACTUALLY reports rather than against what we asked for last time.
-- That costs one IsEnabled per element per tick and buys the only thing that matters: if anything else
-- moves an element -- a garment rebuild, another mod, a live experiment -- the belt heals itself within a
-- tick instead of staying wrong until a reload.
--
-- The colour is deliberately NOT re-asserted here. Writing an appearance the item's own definition
-- disagrees with produces a surface that flickers between the two, so the colour is set once, through
-- the route the item itself uses.
-- Degrees to the quaternion the engine wants, in the order this project has always used: roll about
-- the item's own X, then pitch about Y, then yaw about Z -- the same three axes the position sliders
-- move along, named off the picture as X right, Y forward, Z up.
--
-- One copy, used by the belt's live offsets here and by the props' placement further down.
local function eulerQuat(rx, ry, rz)
    local hr, hp, hy = math.rad(rx or 0.0) * 0.5, math.rad(ry or 0.0) * 0.5, math.rad(rz or 0.0) * 0.5
    local cr, sr = math.cos(hr), math.sin(hr)
    local cp, sp = math.cos(hp), math.sin(hp)
    local cy, sy = math.cos(hy), math.sin(hy)
    return sr * cp * cy - cr * sp * sy,
           cr * sp * cy + sr * cp * sy,
           cr * cp * sy - sr * sp * cy,
           cr * cp * cy + sr * sp * sy
end

local function applyElements(st)
    local comps = beltComponents()
    if comps == nil then return end

    local hideAll = CFG.hide_in_cutscene and st.tier >= CFG.cutscene_tier

    for name, comp in pairs(comps) do
        -- THE PORT'S OWN MARK TWINS ARE NOT RULED. A `*_lit` component is driven by the hand, in
        -- setLit, and a rule here would fight it: applyElements runs four times a second and writes
        -- whenever the state differs from what it wants, so the mark would go out a quarter of a
        -- second after it came on.
        local isTwin = string.find(name, "_lit", -4, true) ~= nil
        local rule = ruleFor(name)
        local want = wanted(rule, st)
        if hideAll then want = false end
        -- WHAT IS IN THE HAND IS NOT ALSO ON THE BELT. The rule still says "show the frag holder", and
        -- it is right -- a frag grenade is still equipped -- so the hold has to override it rather than
        -- be expressed through it.
        if S.grab.host ~= nil and (name == S.grab.host or name == S.grab.host .. "_lit") then
            want = false
        end
        -- ...and the same for a loose shell the reload is carrying to a pump gun's loading port.
        if S.shellTaken ~= nil and (name == S.shellTaken or name == S.shellTaken .. "_lit") then
            want = false
        end

        local okE, is = pcall(isEnabled, comp)
        if not okE then is = nil end
        if not isTwin and is ~= nil and is ~= want then
            S.want[name] = want
            pcall(toggle, comp, want)
            if CFG.verbose then log("%s -> %s (was %s)", name, tostring(want), tostring(is)) end
        end

        -- THE PANEL'S OWN OFFSET for this component, whatever it is -- an element of ours, one of the
        -- belt's own pouches, or a baked component like the grenade holder. Read once, written only
        -- when it differs, exactly like the element offsets below.
        local off = offsetFor(name)
        if off ~= nil then
            local sx = mirrorSign(name)
            local wx, wy, wz = sx * (off.x or 0.0), off.y or 0.0, off.z or 0.0
            local okL, cx, cy, cz = pcall(localPosition, comp)
            if not okL then cx, cy, cz = nil, nil, nil end
            if cx == nil or math.abs(cx - wx) > 0.0002
                or math.abs(cy - wy) > 0.0002 or math.abs(cz - wz) > 0.0002 then
                pcall(function() comp:SetLocalPosition(Vector4.new(wx, wy, wz, 1.0)) end)
            end
            -- AND THE ROTATION, on the same terms. Degrees in the panel because that is what a person
            -- can aim with; the quaternion is what the engine takes, and it is also what gets baked
            -- into the asset afterwards, so the log prints both.
            local qi, qj, qk, qr = eulerQuat(off.rx, sx * (off.ry or 0.0), sx * (off.rz or 0.0))
            local okQ, ai, aj, ak, ar = pcall(localOrientation, comp)
            if not okQ then ai, aj, ak, ar = nil, nil, nil, nil end
            if ai == nil or math.abs(ai - qi) > 0.0004 or math.abs(aj - qj) > 0.0004
                or math.abs(ak - qk) > 0.0004 or math.abs(ar - qr) > 0.0004 then
                pcall(function() comp:SetLocalOrientation(Quaternion.new(qi, qj, qk, qr)) end)
            end
        end

        -- The element's offset, held the same way as its visibility: one read, a write only when it
        -- differs. This is what makes the panel's sliders live.
        for _, el in ipairs(CFG.elements) do
            if el.host == name and S.dressed[name] == el.name then
                local wx, wy, wz = el.ox or 0.0, el.oy or 0.0, el.oz or 0.0
                local okL, cx, cy, cz = pcall(localPosition, comp)
                if not okL then cx, cy, cz = nil, nil, nil end
                if cx == nil or math.abs(cx - wx) > 0.0002
                    or math.abs(cy - wy) > 0.0002 or math.abs(cz - wz) > 0.0002 then
                    pcall(function() comp:SetLocalPosition(Vector4.new(wx, wy, wz, 1.0)) end)
                end
            end
        end

        -- One CName read per element per tick, and a write only when the value is not ours -- which in
        -- practice is once per attach.
        if CFG.render_plane_param ~= "" then
            local okR, cur = pcall(planeParamOf, comp)
            if not okR then cur = nil end
            if cur ~= nil and cur ~= CFG.render_plane_param then
                pcall(function() comp.renderingPlaneAnimationParam = CName.new(CFG.render_plane_param) end)
                S.planeWrites = S.planeWrites + 1
                if CFG.verbose then log("%s plane param %s -> %s", name, cur, CFG.render_plane_param) end
            end
        end
    end
end

local detach   -- defined below; the colour route takes the belt off and puts the other one on
local function applyColor()
    local i = CFG.variant
    if i < 1 or i > #CFG.variants then i = 1 end

    if CFG.color_route == "component" then
        local comps = beltComponents()
        if comps == nil then return false end
        local app = CFG.appearances[i] or CFG.appearances[1]
        local done = 0
        for _, comp in pairs(comps) do
            local ok = nil
            pcall(function() ok = Game.VRComponentChangeAppearance(comp, CName.new(app), false) end)
            if ok == true then done = done + 1 end
        end
        log("colour -> %s (%s) on %d component(s)", CFG.variant_names[i] or "?", app, done)
        return done > 0
    end

    -- The item route: take the belt off and put the other variant on. One frame without a belt, and the
    -- appearance then comes from the item definition, so nothing is fighting it afterwards.
    detach()
    S.lastEquipTry = 0.0
    log("colour -> %s via the item (%s)", CFG.variant_names[i] or "?", CFG.variants[i])
    return true
end

-- ---------------------------------------------------------------------------------------------------
-- the element the hand is reaching for

-- Re-resolved on a timer rather than probed: see the note in beltComponents about why calling into a
-- cached handle is not a liveness test.
local function slotsComponent()
    local now = os.clock()
    if memo.slots ~= nil and memo.slotsAt ~= nil and (now - memo.slotsAt) < 5.0 then
        return memo.slots
    end
    local pl = Game.GetPlayer()
    if pl == nil then return nil end
    memo.slots = nil
    memo.slotsAt = now
    pcall(function() memo.slots = pl:FindComponentByName(CName.new("slots")) end)
    return memo.slots
end

-- The hips in MODEL space: position and orientation. Model space is where VRPalmModelPos already is, so
-- nothing has to be composed through the render view -- the mistake the reload module measured at 0.46 m.
-- Read every frame (the magazine reach), so no closure per call and the slot name made once.
local hipsSlotName = nil
local function playerModelBasis(pl)
    local base = pl:GetWorldPosition()
    local q = pl:GetWorldOrientation()
    return base.x, base.y, base.z, -q.i, -q.j, -q.k, q.r        -- conjugate: world -> model
end
local function hipsSlotWorld(sc)
    hipsSlotName = hipsSlotName or CName.new("hips")
    local found, tr = sc:GetSlotTransform(hipsSlotName)
    if not found or tr == nil then return nil end
    local v = WorldPosition.ToVector4(tr.Position)
    local o = tr.Orientation
    return v.x, v.y, v.z, o.i, o.j, o.k, o.r
end

local function hipsModelFrame()
    local pl = Game.GetPlayer()
    local sc = slotsComponent()
    if pl == nil or sc == nil then return nil end
    local ok, bx, by, bz, ci, cj, ck, cr = pcall(playerModelBasis, pl)
    if not ok or bx == nil then return nil end

    local okS, vx, vy, vz, qi, qj, qk, qr = pcall(hipsSlotWorld, sc)
    if not okS or vx == nil or qi == nil then return nil end
    local px, py, pz = vx - bx, vy - by, vz - bz

    px, py, pz = qrot(ci, cj, ck, cr, px, py, pz)                 -- hips position, model space
    qi, qj, qk, qr = qmul(ci, cj, ck, cr, qi, qj, qk, qr)          -- hips rotation, model space
    return px, py, pz, qi, qj, qk, qr
end

local function elementCentre(host)
    -- On the left-handed belt the element hangs mirrored, and so does its centre.
    local sx = mirrorSign(host)
    local el = elementOn(host)
    if el ~= nil and el.cx ~= nil then
        return sx * (el.cx + (el.ox or 0.0)), el.cy + (el.oy or 0.0), el.cz + (el.oz or 0.0)
    end
    local c = CFG.centres[host]
    if c == nil then return nil end
    -- ...PLUS WHATEVER THE COMPONENT IS ACTUALLY WEARING. `centres` is the mesh's own bounding-box
    -- centre, which is where the element sits with no offset written; the runtime offset moves the
    -- component and the centre has to go with it or the hand is measured against where the thing used
    -- to be. On the female body that is the whole forty-one-element layer -- 12 to 82 mm, every
    -- element its own -- and it is exactly the wrong grab radius / grab maths it produced.
    --
    -- ONLY THE TRANSLATION. A rotation in that offset turns the piece about its own origin, not about
    -- the belt, so it does not move the centre; adding it here would swing the reach point metres.
    local o = offsetFor(host)
    if o == nil then return sx * c[1], c[2], c[3] end
    return sx * (c[1] + (o.x or 0.0)), c[2] + (o.y or 0.0), c[3] + (o.z or 0.0)
end

-- The port's own elements: a magazine, a grenade, a shell. Each mesh carries its own mark (below).
local OURS = { MAG_PREFIX, "ucbeltgrenade", "ucbeltammo_shell" }
local function ours(host)
    for _, p in ipairs(OURS) do
        if string.sub(host, 1, #p) == p then return true end
    end
    return false
end

-- THE MARK IS IN THE ELEMENT'S OWN MESH (tools/belt_lit_shell.py): every appearance A it has comes with `A_lit`,
-- the same materials on the object and the amber mark on a shell drawn over the same geometry. So a mark is the
-- appearance it wears now with `_lit` after it -- a grenade keeps its gang's decals, a magazine its skin.
local LIT = "_lit"
local function litAppearanceFor(host, cur)
    local function marked(name)
        if name == nil or name == "" then return nil end
        if string.sub(name, -#LIT) == LIT then return name end
        return name .. LIT
    end
    if ours(host) then return marked(cur) end
    local el = elementOn(host)
    if el ~= nil and el.hl ~= nil and el.hl ~= "" then
        -- `hl = "lit"` means OUR marking; any other name is one of the element's own mesh's sets, passed through.
        if string.sub(el.hl, 1, 3) == "lit" then return marked(cur) end
        return el.hl
    end
    return CFG.highlight_appearance
end

-- Puts the object on the scene plane, which it has to be on BEFORE a highlight claim is made or the
-- claim is dropped. Rate-limited: the claim is renewed every tick, the plane does not need to be.
local pushLast, pushFor = 0.0, nil
local function pushProxy(obj)
    local now = os.clock()
    local period = 1.0 / math.max(0.1, CFG.highlight_push_hz or 2.0)
    -- Always push immediately when the target changes; otherwise hold the rate.
    if obj == pushFor and (now - pushLast) < period then return end
    pushLast, pushFor = now, obj
    pcall(function() obj:QueueEvent(NewObject("gameItemEventsPropagateRenderingPlane")) end)
end

-- The engine's outline on one object, built the way the reference mod builds it. Called EVERY tick for
-- the lit element: a single claim is cancelled again as soon as the HUD reviews that actor, which it
-- does when the player comes closer -- the opposite of loot, whose outline IS the HUD's own.
local function engineHighlight(obj, name)
    if obj == nil then return false end
    -- ALTERNATING NAMES, and this is not a nicety. HasForcedHighlightOnStack compares sourceID +
    -- sourceName + the two types, so repeating the identical claim is a no-op inside the component, and
    -- UpdateActiveForceHighlight then compares object identity and also does nothing -- which is why a
    -- highlight cancelled by the HUD never came back. Two names, ping-ponging, make every tick a new
    -- claim; the previous one is withdrawn so the stack cannot grow.
    -- BEFORE the claim, never after: a claim made while the object is still on the weapon plane is
    -- dropped, and moving it afterwards does not bring that claim back.
    if CFG.highlight_proxy_push then pushProxy(obj) end
    S.hlFlip = not S.hlFlip
    local live = name .. (S.hlFlip and "_a" or "_b")
    local dead = name .. (S.hlFlip and "_b" or "_a")
    pcall(function()
        local x = FocusForcedHighlightData.new()
        x.isSavable = false
        x.hudData = HighlightInstance.new()
        x.hudData.context = CFG.highlight_context
        x.highlightType = CFG.highlight_type
        x.outlineType = CFG.highlight_outline
        x.priority = EPriority.Absolute
        x.sourceName = CName.new(dead)
        obj:CancelForcedVisionAppearance(x)
    end)
    name = live
    local ok = false
    pcall(function()
        local d = FocusForcedHighlightData.new()
        d.isSavable = false
        d.hudData = HighlightInstance.new()
        d.hudData.instant = true
        d.hudData.context = CFG.highlight_context
        d.highlightType = CFG.highlight_type
        d.outlineType = CFG.highlight_outline
        d.inTransitionTime = 0.0
        d.outTransitionTime = 0.0
        d.priority = EPriority.Absolute
        d.sourceName = CName.new(name)
        d.isRevealed = CFG.highlight_through_walls
        obj:ForceVisionAppearance(d)
        ok = true
    end)
    return ok
end

local function engineHighlightOff(obj, name)
    if obj == nil then return end
    pcall(function()
        local d = FocusForcedHighlightData.new()
        d.isSavable = false
        d.hudData = HighlightInstance.new()
        d.hudData.context = CFG.highlight_context
        d.highlightType = CFG.highlight_type
        d.outlineType = CFG.highlight_outline
        d.priority = EPriority.Absolute
        d.sourceName = CName.new(name)
        obj:CancelForcedVisionAppearance(d)
    end)
end

-- A HOST WITH A TWIN is marked by switching the twin on, not by swapping its own appearance.
--
-- Swapping the appearance replaces the material, so the pouch stops being a pouch and becomes the
-- effect -- the objection that killed this route on the props: not an outline, the grenade
-- itself turned see-through. Object PLUS edge needs two components, and for our own elements the
-- second one is baked into the belt's ucbeltacc.ent beside the first.
--
-- No scaling is involved and none is possible: a skinned component has no visualScale field. What
-- keeps the two apart is the material itself -- loot_drop_highlight.mt carries OFFSET_NormalBias in
-- its rasterizer state, which is the game's own way of pushing a highlight off the surface it marks.
-- ONLY WHAT THE PORT PUT THERE IS MARKED.
--
-- The proximity scan walks every ucbelt* component the belt has, and marking the nearest one means
-- the strap and the mod's own pouches change colour too, and only the listed
-- elements may. A host qualifies only if it is ours (a magazine, a grenade, a shell: its mesh carries
-- the mark) or it is declared in CFG.elements.
--
-- The switch is kept so the old behaviour is one click away, but it is off.
local function markable(host)
    if host == nil then return false end
    if CFG.highlight_only_ours == false then return true end
    if string.sub(host, 1, #MAG_PREFIX) == MAG_PREFIX then
        -- A pouch whose magazine is in the player's hand is not markable at all, or the scan would light it
        -- again the moment it is unmarked below.
        return not (S.magTaken and host == S.magHost)
    end
    return ours(host) or elementOn(host) ~= nil
end

-- Light or put out one host and everything that lights with it: each one swapped to its appearance's `_lit` set
-- and back (litAppearanceFor). The mark used to be a twin component per element drawing the mesh again in the
-- mark's material; 51 twins made the belt 123 garments, and an item that changes counts twice in the game's
-- garment pass, whose element numbers are one byte -- 25 + 123 + 123 crashed it whenever the belt came off or
-- went on in the world. In the mesh the mark costs no component.
local function markSet(host, on)
    local complete = true
    local pl = Game.GetPlayer()
    if pl == nil then return false end
    local function currentComponent(name)
        local c = pl:FindComponentByName(CName.new(name))
        return c ~= nil and IsDefined(c) and c or nil
    end
    for _, m in ipairs(markFamily(host)) do
        local ok, component = pcall(currentComponent, m)
        if not ok or component == nil then
            if on then complete = false end
        elseif on then
            local cur = nil
            pcall(function()
                cur = tostring(component.meshAppearance.value or component.meshAppearance)
            end)
            local want = litAppearanceFor(m, cur)
            if cur ~= nil and want ~= nil and cur ~= want then
                S.litFrom[m] = cur
                local changed, accepted = pcall(function() return Game.VRComponentChangeAppearance(component, CName.new(want), true) end)
                if not changed or accepted ~= true then complete = false end
            end
        elseif S.litFrom[m] ~= nil then
            local back = S.litFrom[m]
            local changed, accepted = pcall(function() return Game.VRComponentChangeAppearance(component, CName.new(back), true) end)
            if changed and accepted == true then S.litFrom[m] = nil else complete = false end
        end
    end
    return complete
end

-- THE LIT SET, not a single lit host. Two hands reach for two different things, and with one slot the
-- second hand's element could only be lit by putting the first one out -- with both hands at once only
-- one item lit. So what is applied is a SET, and the work is the difference between
-- what is lit now and what should be.
local function applyLit(want)
    for host, _ in pairs(S.litOn) do
        if want[host] == nil then
            if markSet(host, false) then S.litOn[host] = nil end
        end
    end
    for host, _ in pairs(want) do
        if S.litOn[host] == nil then
            if markSet(host, true) then
                S.litOn[host] = true
                if CFG.verbose then log("lit %s", host) end
            end
        end
    end
    local names = {}
    for host, _ in pairs(S.litOn) do names[#names + 1] = host end
    table.sort(names)
    S.lit = (#names > 0) and table.concat(names, "+") or nil
end

-- TEMPORARY, off by default: re-assert the engine outline on nearby devices so the "does repetition
-- beat the HUD's cancel when you walk up" question gets an answer from the picture. Delete with the
-- investigation.
local function reassertDevices()
    local pl = Game.GetPlayer()
    if pl == nil then return 0 end
    local list = nil
    pcall(function() list = pl:GetEntitiesAroundObject(40.0) end)
    if list == nil then return 0 end
    local n = 0
    for i = 1, #list do
        local v = list[i]
        local isDev = false
        pcall(function() isDev = v:IsDevice() end)
        if isDev and engineHighlight(v, "vrpBeltProbe") then n = n + 1 end
    end
    return n
end

-- ---------------------------------------------------------------------------------------------------
-- BELT PROPS

local propS = { obj = {}, objAt = {}, emptySlot = {}, mesh = {}, claimAt = {}, item = {}, at = {}, placed = {}, lit = {}, dist = {}, flip = {},
                glowComp = {}, glow = {}, emis = {}, plane = {}, fresh = {}, want = {} }

-- The slot lookup and the component walk are both cheap ONCE and wasteful every frame, and props are
-- read every frame now -- the distance readout and the light have to keep up with a hand, not with the
-- belt's five-per-second housekeeping. So both are cached per prop and only re-taken when the cache
-- comes back empty or half a second has passed, which is fast enough to notice a re-attach.
local function propObj(i, p, now)
    local o = propS.obj[i]
    local fresh = (now - (propS.objAt[i] or -1.0)) < 0.5
    -- A confirmed empty, disabled slot needs only the same half-second check
    -- as an occupied one. Enabling it bypasses this negative cache immediately.
    -- Failed lookups and changed slot names must always retry.
    if fresh and (o ~= nil or (not PERFORMANCE.legacyEmptyPropPoll and not p.on
        and propS.emptySlot[i] and propS.emptySlot[i] == p.slot)) then return o end
    local pl = Game.GetPlayer()
    local ts = transactions()
    if pl == nil or ts == nil then return nil end
    o = nil
    local read = pcall(function() o = ts:GetItemInSlot(pl, TweakDBID.new(p.slot)) end)
    propS.emptySlot[i] = (read and o == nil) and p.slot or nil
    if o ~= propS.obj[i] then
        -- Both caches key on the object, so both go: a component handle from the previous object is
        -- the classic way to end up driving something that is no longer in the slot.
        propS.mesh[i], propS.glowComp[i] = nil, nil
        -- A DIFFERENT object in the slot, so everything believed about the marking belongs to the old
        -- one. It is not enough to forget it: the item that comes back from the inventory is often the
        -- SAME entity, and it can arrive still lit from its previous life -- so the mark is forced out
        -- on the new object (updateProps consumes this flag).
        if o ~= nil then propS.fresh[i] = true end
    end
    propS.obj[i], propS.objAt[i] = o, now
    return o
end

-- The first mesh component of a prop, kept once. It is the one the world position is read off, and
-- walking 14 components for it every frame is the kind of thing that turns into a frame cost.
local function propMesh(i, obj)
    local c = propS.mesh[i]
    if c ~= nil then return c end
    pcall(function()
        for _, x in ipairs(Game.VREntityComponents(obj)) do
            if string.find(tostring(x:GetClassName()), "MeshComponent") then c = x return end
        end
    end)
    propS.mesh[i] = c
    return c
end

-- Moves the prop by writing its mesh components' local positions. The slot's customOffset does not
-- work on this path, so this is the only lever -- and it is enough, because every mesh of the item
-- gets the same delta and the item has no other geometry.
-- Degrees to the quaternion the engine wants. Angles are what a person can actually place a pouch
-- with; the quaternion is what gets baked into the asset afterwards, so the panel prints it too.
-- Order is roll about the item's own X, then pitch about Y, then yaw about Z -- the same three axes
-- the position sliders use, and they were named off the picture: X up, Y back, Z right.
local function propQuat(p)
    return eulerQuat(p.rx, p.ry, p.rz)
end

local function propPlace(p, obj)
    if obj == nil then return false end
    local qi, qj, qk, qr = propQuat(p)
    local n = 0
    pcall(function()
        for _, c in ipairs(Game.VREntityComponents(obj)) do
            if string.find(tostring(c:GetClassName()), "MeshComponent") then
                pcall(function() c:SetLocalPosition(Vector4.new(p.up, p.back, p.right, 1.0)) end)
                pcall(function() c:SetLocalOrientation(Quaternion.new(qi, qj, qk, qr)) end)
                -- THE PLANE FOLLOWS THE WEAPON, instead of being picked once at the attach.
                --
                -- In first person the plane is raised by the animation graph through a parameter named
                -- `renderPlane`: a component subscribed to it moves to RPl_Weapon when a weapon comes
                -- out and composites with the arms and the gun, and drops back to the scene when the
                -- weapon goes away. A component that does NOT subscribe stays in the scene plane and
                -- the drawn gun and the first-person clothing are composited over it -- which is
                -- exactly what a belt prop looks like with a weapon in hand.
                --
                -- `renderSceneLayerMask` is not the field: it reads 3 on every component in both
                -- states, so following it achieves nothing (measured, and the experiment was deleted).
                --
                -- This has to be re-applied after anything that rebuilds the components from the
                -- asset, which a re-attach does -- hence here, in the pass that already runs on every
                -- fresh object.
                -- The subscription only EXISTS on skinned components (see the field table in CFG), so
                -- it is only written there. It buys the props nothing today -- their skinned components
                -- are the game's own and draw nothing -- but it is correct for any prop that ever draws
                -- through one, and writing it on a plain component is a silent no-op that reads like a
                -- working mechanism, which is exactly what this cost a round to.
                if string.find(tostring(c:GetClassName()), "Skinned") then
                    pcall(function() c.renderingPlaneAnimationParam = CName.new("renderPlane") end)
                end
                -- The lit twins ride with the object, so they take the same placement -- but never the
                -- enabled state, which is the belt's to drive.
                n = n + 1
            end
        end
    end)
    return n > 0
end

-- The claim has to be renewed, exactly as engineHighlight explains: a single one is cancelled again as
-- soon as the HUD reviews the actor. The outline colour is the port's own white, so the thicker line
-- the replacement shader draws applies to it and to nothing the game outlines itself.
-- Lighting a prop, and putting it out again.
--
-- TWO THINGS HAD TO CHANGE HERE, both learned the hard way today. A claim made with a long
-- outTransitionTime cannot be cancelled -- CancelForcedVisionAppearance starts a fade of that same
-- length, so a 3600-second claim goes out in an hour. And a single claim does not hold anyway: the
-- HUD reviews the actor as the player comes closer and drops it. So the claim carries a ZERO out
-- time and is renewed every tick under alternating names, which is what engineHighlight above does
-- and for the same reasons -- HasForcedHighlightOnStack keys on the source name, so repeating one
-- name is a no-op inside the component and the withdrawn claim never comes back.
local function propClaim(nm, out)
    local d = FocusForcedHighlightData.new()
    local ok = pcall(function()
        d.isSavable = false
        d.hudData = HighlightInstance.new()
        d.hudData.instant = true
        d.hudData.context = CFG.prop_context or 1
        d.highlightType = CFG.prop_type or 0
        d.outlineType = CFG.prop_outline or 8
        d.inTransitionTime = 0.0
        d.outTransitionTime = out
        d.priority = EPriority.Absolute
        d.sourceName = CName.new(nm)
        d.isRevealed = CFG.highlight_through_walls
    end)
    if not ok then return nil end
    return d
end

local function propLight(obj, base, on, flip)
    if obj == nil then return end
    local live = base .. (flip and "_a" or "_b")
    local dead = base .. (flip and "_b" or "_a")
    local dd = propClaim(dead, 0.0)
    if dd ~= nil then pcall(function() obj:CancelForcedVisionAppearance(dd) end) end
    local dl = propClaim(live, 0.0)
    if dl == nil then return end
    if on then
        pcall(function() obj:ForceVisionAppearance(dl) end)
    else
        pcall(function() obj:CancelForcedVisionAppearance(dl) end)
    end
end

-- THE OTHER WAY TO MARK A PROP: its own light, instead of the engine's outline.
--
-- Every prop entity carries an entLightComponent named `vrp_glow`, shipped switched off (see
-- tools/ent/add_prop_light.py). Turning it on marks the prop on EITHER rendering plane, which the
-- outline cannot do -- the outline reaches an object only on RPl_Scene, and that is the only reason
-- the props are attached into the world at all.
--
-- Intensity and radius are written on every toggle rather than baked, so the panel's sliders land on
-- the prop that is in front of the player instead of costing a rebuild.
local function propGlowComp(i, obj)
    local c = propS.glowComp[i]
    if c ~= nil then return c end
    pcall(function()
        for _, x in ipairs(Game.VREntityComponents(obj)) do
            local nm = tostring(x:GetName()):match("%-%-%[%[ (.-) %-%-%]%]") or ""
            if nm == "vrp_glow" then c = x return end
        end
    end)
    propS.glowComp[i] = c
    return c
end

-- THE PROP'S OWN SURFACE LIGHTING UP.
--
-- Our prop meshes are copies that carry a second appearance, `lit`, whose material is an emissive one
-- of ours; `default` is the mesh exactly as the game authored it. So marking a prop is a swap between
-- two appearances and nothing else -- no claim on the HUD, no transition to restart, no glitch intro,
-- and, the reason it exists at all, no dependence on the rendering plane.
--
-- Only the PLAIN mesh components are touched. The skinned ones point at the game's own meshes, which
-- carry no `lit`, and they draw nothing anyway.
local function propEmissive(i, obj, on)
    if obj == nil then return false end
    local level = CFG.prop_glow_level or 1
    local names = { "lit", "lit2", "lit3", "lit4", "lit5", "lit6", "lit7", "lit8" }
    local want = names[level] or "lit"
    local n = 0
    -- ONLY THE TWINS. Swapping the appearance on the object's own components REPLACES its material,
    -- and with the fresnel one that leaves a dark half-transparent shape instead of a grenade -- the
    -- effect standing in for the object rather than marking it. An outline is the object PLUS an edge,
    -- so the edge is its own set of components (vrp_lit_*, added by tools/ent/add_lit_twins.py) drawing
    -- the same geometry one per cent larger, and all that happens here is switching them on.
    pcall(function()
        for _, c in ipairs(Game.VREntityComponents(obj)) do
            local nm = tostring(c:GetName()):match("%-%-%[%[ (.-) %-%-%]%]") or ""
            if string.sub(nm, 1, 8) == "vrp_lit_" then
                if on then
                    pcall(function() Game.VRComponentChangeAppearance(c, CName.new(want), false) end)
                    local k = CFG.prop_rim_scale or 1.05
                    pcall(function() c.visualScale = Vector3.new(k, k, k) end)
                end
                pcall(function() c:Toggle(on and true or false) end)
                n = n + 1
            end
        end
    end)
    return n > 0
end

local function propGlow(i, obj, on)
    if obj == nil then return false end
    local c = propGlowComp(i, obj)
    if c == nil then return false end
    if on then
        pcall(function() c.intensity = CFG.prop_glow_lm or 60.0 end)
        pcall(function() c.radius = CFG.prop_glow_r or 0.35 end)
    end
    local ok = pcall(function() c:Toggle(on and true or false) end)
    return ok
end

-- Where a prop is, in world space. The item's own GetWorldPosition is meaningless for a thing in a
-- slot -- it reads hundreds of metres off -- but its mesh component's local-to-world matrix is right,
-- and it ALREADY carries the local offset written with SetLocalPosition. Measured with the palm
-- resting on the element: the matrix alone gives 0.038 m, adding the offset again gives 0.109, and
-- routing the palm through the hips frame gave 0.180. So: matrix, nothing else.
local function propWorld(c)
    if c == nil then return nil end
    local m = nil
    pcall(function() m = c:GetLocalToWorld() end)
    if m == nil then return nil end
    local w = nil
    pcall(function() w = m.W end)
    if w == nil then return nil end
    return w.x, w.y, w.z
end

-- Palm to prop, both in world space, nothing in between.
--
-- The palm comes from VRPalmWorldPos, which is the port's own native: it takes the solved rig's palm
-- (g_VRPalmModelL/R -- the PALM, not the wrist) and converts it with the VR view's own transform.
-- That is strictly better than converting it by hand through the player entity, which is what this
-- used to do and what left a residual error. The prop comes from its mesh's local-to-world matrix,
-- which already carries the local offset the sliders write.
local function propHandDistance(c, palmW)
    local wx, wy, wz = propWorld(c)
    if wx == nil then return nil end
    local dx, dy, dz = palmW.x - wx, palmW.y - wy, palmW.z - wz
    return math.sqrt(dx * dx + dy * dy + dz * dz)
end

-- Distance from either palm to every armed prop, in its own pass so it does not depend on whether the
-- belt's own element highlight is switched on. Runs at the same rate as that one and costs a hand
-- frame plus one matrix read per prop.
local function updatePropDistances(now)
    if CFG.props == nil then return end
    local any = false
    for i = 1, #CFG.props do
        if CFG.props[i].on and CFG.props[i].auto then any = true end
    end
    if not any then
        for i = 1, #CFG.props do propS.dist[i] = nil end
        return
    end
    if type(VRPalmWorldPos) ~= "function" then return end
    -- Both palms once, not once per prop.
    local pw = {}
    for side = 0, 1 do
        local palm = nil
        pcall(function() palm = VRPalmWorldPos(side) end)
        if palm ~= nil and palm.w ~= 0.0 then pw[#pw + 1] = palm end
    end
    for i = 1, #CFG.props do
        local p = CFG.props[i]
        local best = nil
        if p.on and p.auto and #pw > 0 then
            local obj = propObj(i, p, now)
            if obj ~= nil then
                local c = propMesh(i, obj)
                for k = 1, #pw do
                    local d = propHandDistance(c, pw[k])
                    if d ~= nil and (best == nil or d < best) then best = d end
                end
            end
        end
        propS.dist[i] = best
    end
end

-- AddItemToSlot refuses any slot the ITEM's record does not list, and a followed item is a different
-- record for every rarity -- so the port's own belt slot has to be added to whichever one is equipped
-- right now. It is a flat, appended once per record and remembered, so this costs nothing after the
-- first sight of a given grenade.
--
-- The game's own AttachmentSlots.GrenadeLeft/Right work too and need none of this, but they put the
-- grenade where the game decided; ours are the ones the panel's sliders can move.
local propAllowed = {}
local function propAllowSlot(itemName, slot)
    if itemName == nil then return end
    local key = itemName .. "|" .. slot
    if propAllowed[key] then return end
    propAllowed[key] = true
    local flat = itemName .. ".placementSlots"
    local cur = nil
    pcall(function() cur = TweakDB:GetFlat(flat) end)
    if cur == nil then return end
    local arr, have = {}, false
    for i = 1, #cur do
        arr[#arr + 1] = cur[i]
        if string.find(tostring(cur[i]), slot, 1, true) then have = true end
    end
    if have then return end
    arr[#arr + 1] = TweakDBID.new(slot)
    pcall(function() TweakDB:SetFlat(flat, arr) end)
    pcall(function() TweakDB:Update(itemName) end)
    log("prop: allowed %s on %s", slot, itemName)
end

-- WHICH PROP ENTITY STANDS IN FOR WHICH GRENADE. The equipped record's name carries the type in it --
-- Items.GrenadeFragLegendaryPlus, Items.GrenadeBiohazardRarePlus, Items.GrenadeReconRegularHack --
-- so the type is read out of the name rather than out of a tag, which also survives rarity variants
-- and the hack/preset spellings for free. Every entity here is registered in vrp_props.csv and built
-- by tools/make_grenade_props.py.
--
-- `piercing` and `sonic` have no appearance resource of their own in the archives, so there is no
-- prop for them; they fall back to the frag body, which is the closest thing the game ships.
local GRENADE_PROPS = {
    { "frag",       "vrp_gren_frag" },
    { "flash",      "vrp_gren_flash" },
    { "incendiary", "vrp_gren_incendiary" },
    { "biohazard",  "vrp_gren_biohazard" },
    { "recon",      "vrp_gren_recon" },
    { "cutting",    "vrp_gren_cutting" },
    { "smoke",      "vrp_gren_smoke" },
    { "emp",        "vrp_gren_emp" },
    -- OZOB'S NOSE IS A FRAG GRENADE, measured: attaching the real item puts
    -- `w_explosives_001__frag_grenade_01` in the hand, and its recorder take gives the frag's
    -- hold to four decimals with a pin at 24 mm. The prop is a copy of the frag's for that
    -- reason -- it needs a name of its own only so the belt and the record can tell them apart.
    { "ozob",       "vrp_gren_ozob" },
}
local GRENADE_ITEM = "Items.VRPortGrenade"
local grenadeEntity = nil   -- what the record is pointed at right now

-- A RECORD PER GRENADE TYPE, and the reason is measured rather than stylistic.
--
-- The old design was one record whose `entityName` flat was re-pointed per type, and it looked right
-- because the flat really does change. What does not change is the ENTITY: the game caches the
-- resolved template against the RECORD, once per session. Read back live with a flash grenade
-- equipped:
--
--     entityName   vrp_gren_flash                      <- the flat is correct
--     mesh in hand w_explosives_001__frag_grenade_01   <- and the entity is still the frag one
--
-- Removing the prop from the inventory and re-attaching does not help either -- the cache is on the
-- record, not the item. A CLONE with its own entityName resolves properly, checked the same way:
-- `w_explosives_002__flash_grenade_01`, which is what a flashbang should be. So each type gets a
-- record of its own, cloned at startup, and nothing is re-pointed at runtime any more.
local GRENADE_RECORDS = {}

local function ensureGrenadeRecords()
    for i = 1, #GRENADE_PROPS do
        local kind, entity = GRENADE_PROPS[i][1], GRENADE_PROPS[i][2]
        local path = GRENADE_ITEM .. "_" .. kind
        local made = false
        pcall(function()
            -- Cloning over an existing record fails; on a mod reload that is the normal case and the
            -- flats below are re-applied regardless, so the record is correct either way.
            TweakDB:CloneRecord(path, GRENADE_ITEM)
            made = true
        end)
        local ok = pcall(function()
            TweakDB:SetFlat(path .. ".entityName", CName.new(entity))
            TweakDB:SetFlat(path .. ".placementSlots", {
                TweakDBID.new("AttachmentSlots.VRPortBelt6"),
                TweakDBID.new("AttachmentSlots.WeaponLeft"),
                TweakDBID.new("AttachmentSlots.WeaponRight"),
            })
            TweakDB:Update(path)
        end)
        if ok then GRENADE_RECORDS[kind] = path end
        if CFG.verbose then
            log("prop: record %s -> %s (new=%s ok=%s)", path, entity, tostring(made), tostring(ok))
        end
    end
end

-- Point the one grenade record at a prop entity. `entityName` is a flat, so this is the whole of
-- "show the grenade that is equipped" -- no record per type, and no record per rarity.
local function setGrenadeEntity(entity)
    if entity == grenadeEntity then return end
    local ok = pcall(function()
        TweakDB:SetFlat(GRENADE_ITEM .. ".entityName", CName.new(entity))
        TweakDB:Update(GRENADE_ITEM)
    end)
    if ok then
        grenadeEntity = entity
        log("prop: grenade -> %s", entity)
    end
end

-- What a prop should be showing right now, and the identity to compare against last tick. A fixed
-- `item` is both; a followed one attaches the same record every time and changes only its entity, so
-- the entity is what the caller has to watch for a re-attach.
local function propItem(p)
    if p.follow == nil then return p.item, p.item end
    if p.follow == "grenade" then
        local pl = Game.GetPlayer()
        local eq = equipSystem()
        if pl == nil or eq == nil then return nil, nil end
        -- GetActiveGadget, not GetActiveItem. Checked live with a grenade equipped and eight in the
        -- inventory: every equipment area -- Gadget, QuickSlot, Consumable -- reported nothing, and
        -- GetActiveGadget returned the record straight away.
        local id = nil
        pcall(function()
            local data = eq:GetPlayerData(pl)
            id = data:GetActiveGadget()
        end)
        if id == nil or not ItemID.IsValid(id) then return nil, nil end
        local nm = nil
        pcall(function() nm = TDBID.ToStringDEBUG(ItemID.GetTDBID(id)) end)
        if nm == nil or nm == "" then return nil, nil end
        -- Only grenades: the quick slot also holds other gadgets, and hanging a cyberdeck shard on
        -- the belt would be nonsense.
        if not isGrenade(id) then return nil, nil end
        local low = string.lower(nm)
        local entity = "vrp_gren_frag"
        for k = 1, #GRENADE_PROPS do
            if string.find(low, GRENADE_PROPS[k][1], 1, true) then
                entity = GRENADE_PROPS[k][2]
                break
            end
        end
        -- The per-type record if it exists, and the shared one only as a fallback -- which now means
        -- "the clone failed", not "this is how it works".
        local kind = nil
        for k = 1, #GRENADE_PROPS do
            if GRENADE_PROPS[k][2] == entity then kind = GRENADE_PROPS[k][1] break end
        end
        local rec = (kind ~= nil) and GRENADE_RECORDS[kind] or nil
        if rec ~= nil then return rec, entity end
        setGrenadeEntity(entity)
        return GRENADE_ITEM, entity
    end
    return p.item, p.item
end

-- THE PLANE THE PLAYER IS IN, read off the player rather than inferred.
--
-- A skinned component on the player that IS subscribed to `renderPlane` -- the vest is one -- carries
-- the player's current plane in its own `renderingPlane` field, because the animation graph writes it
-- there. Mirroring that number is "follow the player" literally, with nothing guessed, and it is also
-- right in the states where a guess from "is a weapon in hand" would not be: a scene, a device
-- takeover, a braindance.
--
-- The component walk is cached and the field is read at 10 Hz rather than every frame.
local PLANE_NAME = { [0] = "RPl_Scene", [1] = "RPl_Background", [2] = "RPl_Weapon" }
local planeS = { comp = nil, at = -1.0, val = nil, tried = -1.0, src = "-", wr = "-" }
-- DO NOT CALL THIS, AND DO NOT REVIVE THE ROUTE IT BELONGS TO.
--
-- Reading `renderingPlane` off a live component, and walking the player's components probing them for
-- `renderingPlaneAnimationParam`, TOOK THE GAME DOWN -- twice from the CET bridge, and the same walk
-- was running once a second inside this module. A `pcall` does not protect it: an access violation in
-- native code is taken by REDEngine's own vectored handler before Lua ever sees it, which is the same
-- trap already written down for __try/__except on the C++ side.
--
-- It is kept because this project does not delete what it learned, and because an unnamed comment
-- would not stop the next attempt. The plane source that replaced it is below, and that one only uses
-- GetItemInSlot -- an API every other part of this module already leans on every tick.
local function planeOf_DO_NOT_CALL(c)
    local v = nil
    pcall(function() v = tostring(c.renderingPlane) end)
    if v == nil then return nil end
    if string.find(v, "Weapon") then return 2 end
    if string.find(v, "Background") then return 1 end
    if string.find(v, "Scene") then return 0 end
    return tonumber(v)
end
-- A WEAPON IN A HAND SLOT, and nothing else. That is what puts the first person into RPl_Weapon to
-- begin with -- equipment.swift:740 asks for that plane on equip -- so the presence of an item in a
-- weapon slot answers the same question the animation parameter answers, through an API this module
-- already uses on every tick for the props themselves.
--
-- What it cannot know is a state where the game lowers the plane WITHOUT unequipping: a scene, a
-- device takeover, a braindance. The stronger source for those is the plane a subscribed component
-- reports, and reading that crashed the game (see planeOf_DO_NOT_CALL), so it is not on the table.
--
-- Uncached, for the grenade grab: a grenade must never go into a hand slot a weapon is still in (see
-- grenade.lua, the free-hand branch). playerPlane below is the cached reading of the same thing.
local function weaponInHands(pl)
    local ts = transactions()
    if ts == nil or pl == nil then return nil end
    -- OUR OWN PROPS DO NOT COUNT AS A WEAPON. The test above asks "is there an item in a hand slot",
    -- and a grenade taken off the belt is exactly that -- so without this exclusion, putting one in an
    -- empty hand would make this function answer RPl_Weapon and move the whole belt into the weapon
    -- plane, on the strength of the very object whose plane it is being asked about. A loop, and one
    -- that only appears once something of ours can be held.
    -- WE KNOW WHAT WE PUT THERE, so nothing has to be asked of the object.
    --
    -- The first version of this read the item back -- GetItemID, then GetTDBID, then ToStringDEBUG --
    -- to see whether the thing in the hand was ours. It crashed the game about a second after a
    -- grenade went into the hand, twice, with an access violation reading 0x8 and 0x10: our prop is not
    -- a weapon, and asking a weapon's question of it dereferences a null. A second, because this
    -- function is cached for 0.2 s and the props tick reaches it a few frames later.
    --
    -- The bookkeeping answers the same question for free: `S.grab.slot` is the hand slot this module
    -- filled, and there is never more than one.
    local function realWeapon(slot)
        -- NEITHER OF OUR TWO. `grab.slot` holds the prop the player is looking at; `grab.armSlot` holds
        -- the live grenade waiting to be thrown, which is invisible and is not a weapon by any reading
        -- -- but it IS an item in a hand slot, and this function's whole test is that.
        if S.grab.slot == slot then return false end
        if S.grab.armSlots ~= nil and S.grab.armSlots[slot] then return false end
        local o = ts:GetItemInSlot(pl, TweakDBID.new(slot))
        return IsDefined(o)
    end
    local armed = false
    pcall(function()
        armed = realWeapon("AttachmentSlots.WeaponRight") or realWeapon("AttachmentSlots.WeaponLeft")
    end)
    return armed
end

local function playerPlane(pl, now)
    if (now - planeS.at) < 0.2 then return planeS.val end
    planeS.at = now
    local armed = weaponInHands(pl)
    if armed == nil then return planeS.val end
    planeS.val = armed and 2 or 0
    planeS.src = armed and "weapon" or "empty"
    return planeS.val
end

-- MEASURED NOT TO WORK, 2026-09-03. Kept, not deleted, so the next attempt does not repeat it.
--
-- The theory was that `renderingPlane` is read when the render proxy is built, so writing the field and
-- then toggling the component off and straight back on -- the one variable the older "writing
-- renderingPlane after the attach does not take" experiment never turned -- would make it stick. It
-- does not. The write call goes through (`wr=RPl_Weapon` in the panel) and the prop stays in the plane
-- it was attached with.
--
-- The reason, as far as it goes: the plane is a property of the ITEM, applied by the transaction system
-- when it takes the item, and the component field is a copy that nothing re-reads afterwards. The only
-- call that writes the item's plane is the attach itself -- which is what updateProps now does.
--
-- The plane is written on every mesh component, the lit twins included, so the mark travels with the
-- prop. Only the ENABLED ones are rebuilt: a disabled twin has no proxy to rebuild and picks the field
-- up when the belt switches it on.
local function propPlaneApply(i, obj, plane)
    if obj == nil then return false end
    local n = 0
    pcall(function()
        for _, c in ipairs(Game.VREntityComponents(obj)) do
            if string.find(tostring(c:GetClassName()), "MeshComponent") then
                -- BY NAME first: Enum.new takes the member name in every CET build, while the integer
                -- form is not accepted by all of them. The value is NOT read back afterwards -- reading
                -- this property off a live component is what crashed the game -- so the picture is what
                -- proves the write, and `wr` in the panel only reports that the call itself went through.
                local nm = PLANE_NAME[plane] or 'RPl_Scene'
                local ok = pcall(function() c.renderingPlane = Enum.new('ERenderingPlane', nm) end)
                if not ok then ok = pcall(function() c.renderingPlane = plane end) end
                planeS.wr = ok and nm or 'call failed'

                local on = false
                pcall(function() on = c:IsEnabled() end)
                if on then
                    pcall(function() c:Toggle(false) end)
                    pcall(function() c:Toggle(true) end)
                end
                n = n + 1
            end
        end
    end)
    return n > 0
end

-- PUT EVERY MARKING OUT, whatever was on, and forget the state.
--
-- WHY THIS HAS TO EXIST, and it is the whole of the highlight left hanging in the air on walking away:
-- VRPortDropItem is RemoveItemFromSlot(..., false) and the `false` means DO NOT DESTROY -- the item
-- goes back to the inventory alive, with everything done to it still done. A prop detached while lit
-- therefore keeps its lit twins enabled, its vision claim standing and its light burning, and what is
-- left is a highlight hanging in the air where the prop was, staying there as the player walks away.
--
-- It also happened with EVERY marking, not just the current material, and that follows
-- from the same cause: all three mechanisms live ON THE OBJECT, and the drop withdrew none of them.
-- So this is not per-mechanism -- it puts all three out unconditionally, without looking at prop_mark,
-- because the mark may well have been changed since the prop was lit.
--
-- Both alternating claim names are cancelled: only one of them is standing and which one depends on
-- the flip state at the moment it was made.
local function propUnmark(i, obj)
    if obj ~= nil then
        propEmissive(i, obj, false)
        propGlow(i, obj, false)
        propLight(obj, "vrpProp" .. i, false, true)
        propLight(obj, "vrpProp" .. i, false, false)
    end
    propS.lit[i], propS.emis[i], propS.glow[i], propS.claimAt[i] = nil, nil, nil, nil
end

-- One tick of prop upkeep. Cheap by construction: the attach is only attempted once a second and only
-- while the slot is empty, the placement is only written when a slider actually moved, and the claim
-- is only renewed for a prop that is meant to be lit.
local function updateProps(now)
    if CFG.props == nil then return end
    local pl = Game.GetPlayer()
    if pl == nil then return end
    for i = 1, #CFG.props do
        local p = CFG.props[i]
        local obj = propObj(i, p, now)
        -- Keep cleanup for occupied slots and the debug panel's live item label.
        -- Empty disabled slots have no item/plane/highlight work to perform.
        if p.on or obj ~= nil or overlay or PERFORMANCE.legacyEmptyPropPoll then
        local want_item, want_tag = propItem(p)
        -- A followed item that changed -- another grenade type equipped, or none -- means the slot has
        -- to be emptied before the new one can go in. The tag, not the record: a followed prop always
        -- attaches the same record and only its entity changes.
        if want_tag ~= propS.item[i] and obj ~= nil then
            -- OUT BEFORE IT GOES, always: see propUnmark. The drop does not destroy the item.
            propUnmark(i, obj)
            pcall(function() pl:VRPortDropItem(p.slot) end)
            propS.obj[i], propS.mesh[i], propS.objAt[i], propS.placed[i] = nil, nil, nil, nil
            obj = nil
        end
        propS.item[i] = want_tag
        -- FOLLOW THE PLAYER'S PLANE, BY RE-ATTACHING. The plane belongs to the ITEM and the transaction
        -- system applies it when it takes the item, so the attach is the only call that writes it:
        -- writing `renderingPlane` on the components afterwards does not take, with or without a proxy
        -- rebuild (propPlaneApply, measured twice).
        --
        -- The drop and the attach happen in the SAME tick -- `at` is cleared so the once-a-second attach
        -- guard below lets it through immediately, and control falls straight into the obj == nil branch
        -- in this same iteration -- so the prop is not missing for a frame.
        --
        -- Only on a CHANGE, which in practice means a weapon being drawn or put away.
        local plane = playerPlane(pl, now)
        if obj ~= nil and plane ~= nil and propS.plane[i] ~= nil and plane ~= propS.plane[i] then
            propUnmark(i, obj)
            pcall(function() pl:VRPortDropItem(p.slot) end)
            propS.obj[i], propS.mesh[i], propS.objAt[i], propS.placed[i] = nil, nil, nil, nil
            propS.at[i], propS.plane[i], propS.want[i] = 0.0, nil, plane
            planeS.wr = "reattach " .. tostring(plane)
            obj = nil
        end
        if p.on and want_item ~= nil then
            if obj == nil then
                if (propS.at[i] or 0) + 1.0 < now and type(pl.VRPortHoldItemScene) == "function" then
                    propS.at[i] = now
                    propS.placed[i] = nil
                    propS.obj[i], propS.mesh[i], propS.objAt[i] = nil, nil, nil
                    propAllowSlot(want_item, p.slot)
                    -- THE PLANE IS DECIDED HERE and nowhere else -- this is the only call that writes
                    -- it onto the item. VRPortHoldItem and VRPortHoldItemScene differ in exactly that
                    -- one argument (vrport_hold.reds).
                    --
                    -- `want` carries the plane the follow logic asked for; on the very first attach it
                    -- is empty and the player's current plane is used, so a prop attached with a weapon
                    -- already in hand starts in the right plane instead of jumping a tick later.
                    local wplane = propS.want[i] or plane or 0
                    propS.plane[i], propS.want[i] = wplane, nil
                    propS.glow[i], propS.glowComp[i], propS.emis[i] = nil, nil, nil
                    pcall(function()
                        if wplane == 2 then pl:VRPortHoldItem(want_item, p.slot)
                        else pl:VRPortHoldItemScene(want_item, p.slot) end
                    end)
                end
            else
                -- A newly resolved object starts dark, whatever it was doing in its previous life.
                if propS.fresh[i] then
                    propS.fresh[i] = nil
                    propUnmark(i, obj)
                    propS.plane[i] = nil
                end

                local key = string.format("%.4f/%.4f/%.4f/%.2f/%.2f/%.2f", p.up, p.back, p.right, p.rx or 0, p.ry or 0, p.rz or 0)
                if propS.placed[i] ~= key then
                    if propPlace(p, obj) then propS.placed[i] = key end
                end
                -- The hand decides, unless the prop is pinned lit from the panel.
                local want = p.lit
                if p.auto then
                    local d = propS.dist[i]
                    local enter = CFG.prop_radius_m
                    local leave = enter * (CFG.prop_release or 1.4)
                    if d == nil then
                        want = false
                    elseif propS.lit[i] then
                        want = d <= leave
                    else
                        want = d <= enter
                    end
                    if p.lit then want = true end
                end
                -- The DECISION is every frame, so the light answers the hand at once. The CLAIM is
                -- only re-issued on a change or every prop_renew_s, so the HUD's transition is not
                -- restarted under it.
                local changed = (want ~= propS.lit[i])
                local mark = CFG.prop_mark or 0

                -- The outline, when the mark asks for it. Switching the mark to glow-only withdraws a
                -- standing claim once rather than leaving it burning on the prop.
                if mark == 0 or mark == 2 then
                    local due = (now - (propS.claimAt[i] or -1.0)) >= (CFG.prop_renew_s or 0.35)
                    if changed or due then
                        propS.flip[i] = not propS.flip[i]
                        propLight(obj, "vrpProp" .. i, want, propS.flip[i])
                        propS.claimAt[i] = now
                    end
                elseif propS.claimAt[i] ~= nil then
                    propLight(obj, "vrpProp" .. i, false, propS.flip[i])
                    propLight(obj, "vrpProp" .. i, false, not propS.flip[i])
                    propS.claimAt[i] = nil
                end

                -- The emissive swap and the light, on a CHANGE only: doing either every frame
                -- would be a call per frame for a state that did not move.
                local wantEmis = (mark == 1 or mark == 2) and want or false
                -- The level counts as part of the state, so turning the slider re-applies at once
                -- instead of waiting for the hand to leave and come back.
                local emisKey = wantEmis and (CFG.prop_glow_level or 1) or false
                if emisKey ~= propS.emis[i] then
                    if propEmissive(i, obj, wantEmis) then propS.emis[i] = emisKey end
                end
                local wantGlow = (mark == 3) and want or false
                if wantGlow ~= propS.glow[i] then
                    if propGlow(i, obj, wantGlow) then propS.glow[i] = wantGlow end
                end

                propS.lit[i] = want
            end
        elseif obj ~= nil then
            propS.placed[i] = nil
            propUnmark(i, obj)
            pcall(function() pl:VRPortDropItem(p.slot) end)
        end
        end
    end
end

-- THE TUNING, WRITTEN OUT CONTINUOUSLY, to its own file beside belt.cfg.
--
-- Nothing about baking a tuned placement may depend on remembering to press a button or on a log line
-- surviving: the offsets are the work, and losing them costs the whole session at the sliders. So they
-- go to offsets.json a second after the last change, every time, and tools/bake_belt_offsets.py reads
-- that file. belt.cfg keeps them too, but it is written only at shutdown and is the module's own
-- business; this file exists to be read from outside while the game is running.
local function writeOffsetFile(path, tbl)
    local parts = {}
    local names = {}
    for nm, _ in pairs(tbl or {}) do names[#names + 1] = nm end
    table.sort(names)
    for _, nm in ipairs(names) do
        local o = tbl[nm] or {}
        parts[#parts + 1] = string.format(
            '  "%s": { "x": %.5f, "y": %.5f, "z": %.5f, "rx": %.3f, "ry": %.3f, "rz": %.3f }',
            nm, o.x or 0, o.y or 0, o.z or 0, o.rx or 0, o.ry or 0, o.rz or 0)
    end
    local f = io.open(path, "w")
    if f == nil then return 0 end
    local NL = string.char(10)
    f:write("{" .. NL .. table.concat(parts, "," .. NL) .. NL .. "}" .. NL)
    f:close()
    return #names
end

local function writeOffsets(now)
    if S.offDirty == nil then return end
    if now - S.offDirty < 1.0 then return end
    S.offDirty = nil
    -- TWO FILES, NEVER ONE. `offsets.json` is what tools/bake_belt_offsets.py reads and bakes, and the
    -- female layer must never end up there: baking a correction measured on that body would move the
    -- male one by exactly as much, on meshes both share. So it gets a file of its own, readable from
    -- outside for the same reason -- to be lifted into the module's defaults by hand -- and invisible
    -- to the bake.
    local n = writeOffsetFile(OFFSETS_FILE, CFG.offsets)
    local nf = writeOffsetFile(OFFSETS_FEMALE_FILE, CFG.offsets_female)
    if CFG.verbose then log("offsets written (%d male, %d female)", n, nf) end
end

-- THE RECORD OF THE GRENADE THE PLAYER ACTUALLY HAS, as opposed to the prop standing in for it. The
-- throw needs it: a prop carries no projectile, so what leaves the hand has to be the real item.
--
-- Same source as the belt element's own -- GetActiveGadget, which is the one equipment call that
-- answers with a grenade -- and the same guard, because the quick slot holds other gadgets too.
local function activeGrenadeName()
    local pl = Game.GetPlayer()
    local eq = equipSystem()
    if pl == nil or eq == nil then return nil end
    local id = nil
    pcall(function() id = eq:GetPlayerData(pl):GetActiveGadget() end)
    if id == nil or not ItemID.IsValid(id) then return nil end
    if not isGrenade(id) then return nil end
    local nm = nil
    pcall(function() nm = TDBID.ToStringDEBUG(ItemID.GetTDBID(id)) end)
    if nm == nil or nm == "" then return nil end
    return nm
end

-- WHICH GRENADE ELEMENT IS ON THE BELT RIGHT NOW. The panel's button needs it for the same reason the
-- grab does: whatever goes into the hand has to stop being on the belt at the same moment.
local function grenadeElementShowing()
    for name, comp in pairs(S.comps) do
        if string.sub(name, 1, 13) == "ucbeltgrenade" and string.sub(name, -4) ~= "_lit" then
            local on = nil
            pcall(function() on = comp:IsEnabled() end)
            if on == true then return name end
        end
    end
    return nil
end

-- THE GRENADE IN THE HAND lives in grenade/ -- its code, and the finger poses lifted out of the
-- game's own throw. Its state stays in S.grab here,
-- because applyElements and playerPlane both have to know what is being held.
local GRENADE = (function()
    local ok, m = pcall(function() return require("grenade/grenade") end)
    if not ok or not m then return nil end
    m.setup({
        CFG = CFG, S = S, log = log,
        propItem = propItem, propAllowSlot = propAllowSlot, playerPlane = playerPlane,
        weaponInHands = weaponInHands,
        activeGrenade = activeGrenadeName,
        eulerQuat = eulerQuat, transactions = transactions,
    })
    return m
end)()


-- THE POINT ON THE HAND, in model space, with the panel's offset applied in the palm's own frame.
-- One function because two things ask it now -- the element highlight and the magazine reach -- and a
-- reach that disagreed with the mark it lights would be unexplainable from inside the headset.
local function palmPoint(side)
    local okP, p = pcall(VRPalmModelPos, side)
    if not okP then p = nil end
    if p == nil or p.w == 0.0 then return nil end
    local ho = CFG.highlight_hand_off
    if ho ~= nil and (math.abs(ho.x or 0) + math.abs(ho.y or 0) + math.abs(ho.z or 0)) > 1e-6 then
        local ok = pcall(function()
            local q = VRPalmModelRot(side)
            if q == nil then return end
            local ox, oy, oz = qrot(q.i, q.j, q.k, q.r, ho.x or 0.0, ho.y or 0.0, ho.z or 0.0)
            p = { x = p.x + ox, y = p.y + oy, z = p.z + oz, w = p.w }
        end)
        if not ok then return p end
    end
    return p
end

-- HOW FAR EACH HAND IS FROM THE MAGAZINE POUCH, every tick, and deliberately NOT part of the
-- highlight scan above.
--
-- That scan is gated on `highlight` and rate-limited to a few hertz, because it only lights a mark.
-- This number decides whether a reload can happen at all, and a reload that stops working because a
-- cosmetic switch was turned off is a bug waiting to be filed. It costs two palm reads and one
-- centre.
-- THE ROW OF LOOSE SHELLS, measured the same way and for the same reason: a pump shotgun is loaded one
-- shell at a time off this row, and that reach must not depend on the cosmetic highlight either. Only
-- while the row is actually on the belt (a shotgun in the loadout), and nearest of the three, because the
-- hand reaches for a shell, not for the row's middle.
local SHELL_ROW = { "ucbeltammo_shell1", "ucbeltammo_shell2", "ucbeltammo_shell3" }

local function updateShellReach(hx, hy, hz, qi, qj, qk, qr)
    S.shellD[0], S.shellD[1] = nil, nil
    S.shellNear[0], S.shellNear[1] = nil, nil
    local st = S.state
    if not (st and st.wclass and st.wclass.shotgun) then return end
    for side = 0, 1 do
        local p = palmPoint(side)
        if p ~= nil then
            for _, host in ipairs(SHELL_ROW) do
                if S.comps[host] ~= nil and host ~= S.shellTaken then
                    local ex, ey, ez = elementCentre(host)
                    if ex ~= nil then
                        local ax, ay, az = toHipsFrame(ex, ey, ez)
                        local wx, wy, wz = qrot(qi, qj, qk, qr, ax, ay, az)
                        local dx, dy, dz = (hx + wx) - p.x, (hy + wy) - p.y, (hz + wz) - p.z
                        local d = math.sqrt(dx * dx + dy * dy + dz * dz)
                        if S.shellD[side] == nil or d < S.shellD[side] then
                            S.shellD[side], S.shellNear[side] = d, host
                        end
                    end
                end
            end
        end
    end
end

-- The reach and the mark's radius, wider while crouched (CFG.crouch_reach_scale). Read with the highlight's
-- own rate: the crouch flag comes from the player's persistent state, which is cheap but not free.
local function reachRadius()
    local r = CFG.highlight_radius_m
    local now = os.clock()
    if S.crouchAt == nil or now - S.crouchAt > 0.25 then
        S.crouchAt = now
        local pl = Game.GetPlayer()
        local ok, crouched = pcall(function() return pl ~= nil and pl:GetPuppetPS():IsCrouch() end)
        S.crouched = ok and crouched == true
    end
    if S.crouched then r = r * (CFG.crouch_reach_scale or 1.0) end
    return r
end

local function updateMagReach()
    S.magD[0], S.magD[1] = nil, nil
    local st = S.state
    local k = st and st.wmag or nil
    local host = (k ~= nil) and ("ucbeltmag_" .. k) or nil
    S.magHost = (host ~= nil and S.comps[host] ~= nil) and host or nil
    local hx, hy, hz, qi, qj, qk, qr = hipsModelFrame()
    if hx == nil then return end
    updateShellReach(hx, hy, hz, qi, qj, qk, qr)
    if S.magHost == nil then return end
    local ex, ey, ez = elementCentre(S.magHost)
    if ex == nil then return end
    local ax, ay, az = toHipsFrame(ex, ey, ez)
    local wx, wy, wz = qrot(qi, qj, qk, qr, ax, ay, az)
    for side = 0, 1 do
        local p = palmPoint(side)
        if p ~= nil then
            local dx, dy, dz = (hx + wx) - p.x, (hy + wy) - p.y, (hz + wz) - p.z
            S.magD[side] = math.sqrt(dx * dx + dy * dy + dz * dz)
        end
    end
end

-- THE REACH MARKER (CyberpunkVRPort_Reach, modules/marker.lua) on the element each hand has lit: the ring the world's
-- things and the reload's parts get, so a pouch is marked the way everything else a hand can take is. Every frame --
-- a frame without it lets the ring fade -- at the element's place in the hips frame as the last highlight tick found
-- it, carried by the hips as they are now. Without that mod, nothing.
local reachMod = nil
local function markReach()
    local at = S.ringAt
    if at[0] == nil and at[1] == nil then return end
    if reachMod == nil then reachMod = GetMod("CyberpunkVRPort_Reach") or false end
    if not (reachMod and reachMod.mark) then return end
    local hx, hy, hz, qi, qj, qk, qr = hipsModelFrame()
    if hx == nil then return end
    local ok, bx, by, bz, ci, cj, ck, cr = pcall(playerModelBasis, Game.GetPlayer())
    if not ok or bx == nil then return end
    for side = 0, 1 do
        local a = at[side]
        if a ~= nil and S.litHand[side] ~= nil then
            local mx, my, mz = qrot(qi, qj, qk, qr, a[1], a[2], a[3])
            local wx, wy, wz = qrot(-ci, -cj, -ck, cr, hx + mx, hy + my, hz + mz)
            local from = nil
            local pp = palmPoint(side)
            if pp ~= nil then
                local fx, fy, fz = qrot(-ci, -cj, -ck, cr, pp.x, pp.y, pp.z)
                from = { bx + fx, by + fy, bz + fz }
            end
            pcall(reachMod.mark, side + 1, { pos = { bx + wx, by + wy, bz + wz }, radius = 0.03, lift = 0.03,
                                             kind = "near", ready = true, from = from })
        end
    end
end

local function updateHighlight(dt)
    if not CFG.highlight then
        if S.lit ~= nil then S.litHand[0], S.litHand[1] = nil, nil applyLit({}) end
        return
    end
    S.hlAcc = S.hlAcc + (dt or 0.016)
    local period = 1.0 / math.max(1.0, CFG.highlight_rate_hz)
    if S.hlAcc < period then return end
    S.hlAcc = 0.0

    if CFG.probe_devices then
        S.probeCount = reassertDevices()
    end

    if next(S.comps) == nil then
        if S.lit ~= nil then S.litHand[0], S.litHand[1] = nil, nil applyLit({}) end
        return
    end
    if type(VRPalmModelPos) ~= "function" then return end

    local hx, hy, hz, qi, qj, qk, qr = hipsModelFrame()
    if hx == nil then return end

    -- EACH HAND KEEPS ITS OWN ELEMENT. The two are resolved independently and the results are unioned,
    -- so reaching for a grenade with one hand and a magazine with the other lights both.
    local enter = reachRadius()
    local leave = enter * (CFG.highlight_release or 1.35)
    local nearest = nil
    local targets = nil
    -- WITH A WEAPON IN HAND THE GRENADES ARE NOT ON OFFER. A lit holder is what lets a grip take one
    -- (grenade.lua starts a grab only at the holder this hand has lit), and a grenade put into the hand
    -- slot the gun is in pushed the gun out of the slot behind the equipment system's back: the hand was
    -- left in the pistol grip with nothing in it and the holster could not draw again. So the holders
    -- simply do not light until the weapon is put away.
    local plNow = Game.GetPlayer()
    local noGrenades = plNow ~= nil and playerPlane(plNow, os.clock()) == 2
    local litBefore = { [0] = S.litHand[0], [1] = S.litHand[1] }

    for side = 0, 1 do
        local p = palmPoint(side)
        if p ~= nil then
            -- Both hands test the same components in the same hips frame.
            -- Read their enabled state and centres once per highlight tick,
            -- lazily after a valid palm. The snapshot contains only names and
            -- coordinates and expires here; there is no cross-frame cache.
            if targets == nil or PERFORMANCE.legacyHighlightScan then
                targets = {}
                for host, comp in pairs(S.comps) do
                    local ex, ey, ez = elementCentre(host)
                    local eligible = ex ~= nil and markable(host)
                        and not (noGrenades and string.find(host, "ucbeltgrenade", 1, true) == 1)
                    local on = nil
                    if eligible or PERFORMANCE.legacyHighlightScan then
                        local okE, v = pcall(isEnabled, comp)
                        if okE then on = v end
                    end
                    if eligible and on == true then
                        local ax, ay, az = toHipsFrame(ex, ey, ez)
                        local wx, wy, wz = qrot(qi, qj, qk, qr, ax, ay, az)
                        targets[#targets + 1] = {host = host, x = hx + wx, y = hy + wy, z = hz + wz,
                                                 ax = ax, ay = ay, az = az}
                    end
                end
            end
            local best, bestD = nil, nil
            for _, target in ipairs(targets) do
                local dx, dy, dz = target.x - p.x, target.y - p.y, target.z - p.z
                local d = math.sqrt(dx * dx + dy * dy + dz * dz)
                if bestD == nil or d < bestD then best, bestD = target.host, d end
            end
            -- The row a piece belongs to is what the hand is really reaching for.
            if best ~= nil then best = OFFSET_GROUP[best] or best end
            -- Hysteresis is per hand: it holds what THIS hand already has until that one is left
            -- behind, which is what stops a mark flickering between two elements of equal distance.
            local held = S.litHand[side]
            -- ...and one this hand already had lit is let go of too: it is no longer a target, so the
            -- hysteresis below would otherwise keep holding it whenever nothing else is within reach.
            if noGrenades and held ~= nil and string.find(held, "ucbeltgrenade", 1, true) == 1 then
                S.litHand[side], held = nil, nil
            end
            if best == nil or bestD == nil then
                S.litHand[side] = nil
            elseif held == best then
                if bestD > leave then S.litHand[side] = nil end
            elseif bestD <= enter then
                S.litHand[side] = best
            end
            if bestD ~= nil and (nearest == nil or bestD < nearest) then nearest = bestD end
            -- Where the reach marker goes for this hand: the nearest piece of what it has lit, in the hips frame.
            local lit, ring, ringD = S.litHand[side], nil, nil
            if lit ~= nil then
                for _, target in ipairs(targets) do
                    if (OFFSET_GROUP[target.host] or target.host) == lit then
                        local dx, dy, dz = target.x - p.x, target.y - p.y, target.z - p.z
                        local d = dx * dx + dy * dy + dz * dz
                        if ringD == nil or d < ringD then ring, ringD = target, d end
                    end
                end
            end
            S.ringAt[side] = ring and { ring.ax, ring.ay, ring.az } or nil
            S.handSide = S.handSide or {}
            local hs = S.handSide[side]
            if hs == nil then hs = {}; S.handSide[side] = hs end
            hs.best, hs.d = best, bestD
            S.hand = p
        else
            S.litHand[side] = nil
            S.ringAt[side] = nil
        end
    end
    S.handDist = nearest

    -- A TICK IN THE HAND THAT REACHED IT: the moment an element lights for a hand, that hand feels it,
    -- so the belt can be found without looking down. Only on the edge -- a hand resting on a pouch
    -- does not keep buzzing. 1 = the belt-touch profile (src/Runtimes/Haptics.cpp); mask 1 left, 2 right.
    if type(VRHapticEvent) == "function" then
        for side = 0, 1 do
            local h = S.litHand[side]
            if h ~= nil and h ~= litBefore[side] then
                pcall(VRHapticEvent, 1, side == 1 and 2 or 1)
            end
        end
    end

    local want = {}
    for side = 0, 1 do
        local h = S.litHand[side]
        if h ~= nil then want[h] = true end
    end
    applyLit(want)
end

-- ---------------------------------------------------------------------------------------------------

-- THE SAVED CONFIG WINS OVER THE DEFAULTS ABOVE, and that is right for anything the player has tuned
-- -- but it also means a default that is CORRECTED can never reach a machine that already has a
-- belt.cfg. The throw scale was wrong by a factor of four; leaving it to be fixed by hand on every
-- install is not a fix.
--
-- So the throw numbers carry a version. Bumping it re-applies them ONCE, and every slider moved after
-- that persists as normal.
local THROW_CFG_VERSION = 4

local function upgradeThrowCfg()
    if CFG.grenade_throw_v == THROW_CFG_VERSION then return end
    CFG.grenade_throw_v = THROW_CFG_VERSION
    CFG.grenade_throw_gain = 2.0
    CFG.grenade_throw_consume = true
    CFG.grenade_throw_max = 45.0
    CFG.grenade_throw_min = 1.5
    log("throw defaults upgraded to v%d (gain %.1f)", THROW_CFG_VERSION, CFG.grenade_throw_gain)
end

-- The grab plane the same way: every belt.cfg written while the default was 2 carries 2, and only a
-- versioned re-apply gets the corrected default to it.
local GRAB_PLANE_CFG_VERSION = 1

local function upgradeGrabPlaneCfg()
    if CFG.grenade_grab_plane_v == GRAB_PLANE_CFG_VERSION then return end
    CFG.grenade_grab_plane_v = GRAB_PLANE_CFG_VERSION
    CFG.grenade_grab_plane = 0
    log("grenade grab plane upgraded to v%d (follow the player)", GRAB_PLANE_CFG_VERSION)
end

-- The male body's tuned placements, the same way: a saved belt.cfg carries its own entry for every
-- host the panel touched, zeros included, and `offsets` is merged over the defaults key by key. The
-- pump's three shells follow ucbeltammo_shell1 (OFFSET_GROUP). The version lives in CFG, so it is
-- read back and the entries are put in once; a slider moved after that persists.
local OFFSETS_CFG_VERSION = 1
local MALE_TUNED_OFFSETS = {
    ucbeltammo_shell1 = { x = 0.0180, y = 0.0260, z = 0.1020, rx = 0.00, ry = 0.00, rz = 0.00 },
    ucbeltmag_dian    = { x = 0.0240, y = 0.0260, z = 0.0580, rx = 0.00, ry = 0.00, rz = 0.00 },
}

local function upgradeOffsetsCfg()
    if CFG.offsets_v == OFFSETS_CFG_VERSION then return end
    CFG.offsets_v = OFFSETS_CFG_VERSION
    for host, o in pairs(MALE_TUNED_OFFSETS) do
        CFG.offsets[host] = { x = o.x, y = o.y, z = o.z, rx = o.rx, ry = o.ry, rz = o.rz }
    end
    log("male offsets upgraded to v%d (dian, shells)", OFFSETS_CFG_VERSION)
end

-- The reach radius likewise: every belt.cfg written while the default was 0.14 carries 0.14.
local HIGHLIGHT_RADIUS_CFG_VERSION = 1

local function upgradeHighlightRadiusCfg()
    if CFG.highlight_radius_v == HIGHLIGHT_RADIUS_CFG_VERSION then return end
    CFG.highlight_radius_v = HIGHLIGHT_RADIUS_CFG_VERSION
    CFG.highlight_radius_m = 0.17
    log("reach radius upgraded to v%d (%.2f m)", HIGHLIGHT_RADIUS_CFG_VERSION, CFG.highlight_radius_m)
end

registerForEvent("onInit", function()
    loadCfg()
    upgradeThrowCfg()
    upgradeGrabPlaneCfg()
    upgradeOffsetsCfg()
    upgradeHighlightRadiusCfg()
    ensureGrenadeRecords()
    -- Player entity IDs can be reused across saves (the live player is 1ULL).
    -- Reset on the actual attach event as well as nil/player-change detection.
    Observe("PlayerPuppet", "OnGameAttached", function() memoDrop() end)
    S.ready = true
    log("ready; item=%s slot=%s auto_equip=%s", CFG.variants[CFG.variant], CFG.slot, tostring(CFG.auto_equip))
end)

-- EVERY CACHED COMPONENT BELONGS TO ONE PLAYER ENTITY, and loading a save builds a new one.
--
-- `S.comps`, `S.beltParts`, `S.beltComp` and `S.dressed` are raw pointers into the player. The tick
-- reads them in several places -- the reach, the highlight, the props -- and only ONE of those paths
-- (`beltComponents`, through applyElements) ever checked whether the player was still the same. The
-- others ran first, on the frame the new player appeared, and read freed memory: the crash on loading
-- a save, twice.
--
-- So the check moves to the top of the tick, before anything can touch them, and it is the whole test:
-- a different entity hash means everything cached is thrown away. `memoDrop` does the same on the
-- frames where the player is briefly nil; this covers the swaps where it never is.
local function dropIfPlayerChanged(pl, who)
    if who == nil or who == S.compsFor then return end
    startupClothing:Reset()
    S.compsFor = who
    -- ...and nothing else runs this second. See the note above this function.
    S.holdOff = os.clock() + 1.0
    if GRENADE then
        pcall(function() GRENADE.release(pl) end)
    end
    S.comps = {}
    S.dressed = {}
    S.beltParts = nil
    S.beltPartsFor = nil
    S.beltComp = nil
    S.beltCompName = nil
    S.litOn, S.litFrom, S.litHand = {}, {}, {}
    S.lit = nil
    S.beltHash = nil
    memo.magByEnt = nil
end

registerForEvent("onUpdate", function(dt)
    if not S.ready or not CFG.enabled then return end
    -- The main menu and character creation have a player of their own, the menu world's puppet, and the
    -- belt is not worn there. Attaching to it crashed: a request superseded there had the item factory free the
    -- half-loaded belt meshes (read 0x0 at exe+0xB0C787, the fault ATTACH_WAIT exists for).
    local okG, preGame = pcall(function() return Game.GetSystemRequestsHandler():IsPreGame() end)
    if okG and preGame then return end

    local pl = Game.GetPlayer()
    if pl == nil then
        memoDrop()
        return
    end
    -- Menus, load screens and the photo mode: nothing here has any business writing to a component
    -- while the game is not actually being played.
    -- APPLIED WHILE PAUSED ONLY WITH THE PANEL OPEN, and that qualifier is the whole fix for the
    -- crash on loading a save.
    --
    -- The pause guard is not only about the pause menu: a LOADING SCREEN is paused too, and that is
    -- exactly when the player entity is torn down and rebuilt. Running these there wrote into cached
    -- components belonging to the entity that was going away -- a read of freed memory, which is the
    -- access violation the report shows. With the overlay open nothing is being swapped, so the live
    -- panel keeps working and the dangerous window is gone.
    -- NOTHING NATIVE IS TOUCHED WHILE THE GAME IS PAUSED, and that now includes the belt's own
    -- pieces.
    --
    -- I opened that window on purpose, so a tick in the panel would show on the body immediately. It
    -- cost two crashes: a loading screen is paused, and so is the overlay during `reload mods` -- and
    -- both tear down objects while these functions write `Toggle()` and `chunkMask` into cached
    -- component pointers. The second one faulted reading address 0x8 on this very thread.
    --
    -- There is no safe way to hold a native handle across a teardown, so the window is closed. A tick
    -- in the panel now shows when the overlay closes, a second later. That is a fair price.
    local okP, paused = pcall(gamePaused)
    if okP and paused then return end
    -- The player's identity once a frame; the parts walk below needs the same answer.
    local who = entityHash(pl)
    dropIfPlayerChanged(pl, who)
    -- The quiet second after a player swap. Everything below this line touches native objects.
    if S.holdOff ~= nil then
        if os.clock() < S.holdOff then return end
        S.holdOff = nil
    end
    applyBeltChunks(applyBeltPieces(pl, who))

    updateMagReach()
    updateHighlight(dt)
    markReach()

    -- EVERY FRAME, deliberately. The belt's own housekeeping runs on CFG.rate_hz and that is right for
    -- it, but a prop has to answer a hand: the distance readout and the light both looked sluggish
    -- while they sat behind that gate. What makes it affordable is the caching above -- per frame this
    -- is two palm reads, one matrix read per armed prop and a subtraction.
    local nowF = os.clock()
    -- Every frame, like the props and for the same reason: a grip is an edge, and an edge sampled four
    -- times a second is an edge that gets missed.
    if GRENADE then GRENADE.tick(nowF, dt) end
    updatePropDistances(nowF)
    updateProps(nowF)

    S.acc = S.acc + (dt or 0.016)
    local period = 1.0 / math.max(1.0, CFG.rate_hz)
    if S.acc < period then return end
    S.acc = 0.0
    local now = os.clock()

    ensureEquipped(now)
    -- The repair is for EquipmentEx's outfit mode, which a belt in its waist slot can switch on; the
    -- port's own slot is not one EquipmentEx manages.
    if CFG.auto_equip and not startupClothing.done and slotName() == CFG.outfit_slot then
        local refresh = startupClothing:Tick(now, pl, transactions(), outfitSystem(), slotID(), wantedRecord())
        if refresh then
            if refresh.ok and refresh.done then
                log("startup clothing refreshed once: restored=%d slots=%d", refresh.restored, refresh.refreshed)
            elseif not refresh.ok then
                log("startup clothing refresh %s: %s", refresh.done and "stopped" or "deferred", refresh.error)
            end
            if refresh.changed then
                -- Garment refresh can rebuild components on the player. Drop
                -- raw component pointers without rearming the one-shot job.
                clearGarmentCache()
                S.holdOff = os.clock() + 1.0
                return
            end
        end
    end
    dressElements()
    writeOffsets(now)

    -- Idempotent and cheap at this rate; see the note on garment_offsets.
    if CFG.garment_offsets and now - S.offsetsAt > 5.0 then
        S.offsetsAt = now
        pcall(function() ArchiveXL.EnableGarmentOffsets() end)
    end

    S.state = readState()
    applyElements(S.state)
end)

registerForEvent("onOverlayOpen", function() overlay = true end)
registerForEvent("onOverlayClose", function() overlay = false; saveCfg() end)

registerForEvent("onDraw", function()
    if not overlay then return end
    pcall(function()
        local w, h = GetDisplayResolution()
        ImGui.SetNextWindowPos(w * 0.21, h * 0.04, ImGuiCond.FirstUseEver)
        ImGui.SetNextWindowSize(w * 0.24, h * 0.91, ImGuiCond.FirstUseEver)
        ImGui.Begin("VR tactical belt")

        CFG.enabled = ImGui.Checkbox("enabled", CFG.enabled)
        CFG.auto_equip = ImGui.Checkbox("keep it equipped", CFG.auto_equip)

        ImGui.Separator()
        ImGui.Text(string.format("slot: %s   EquipmentEx: %s", slotName(),
            outfitSystem() ~= nil and "installed" or "not installed"))
        ImGui.Text(string.format("belt object:   %s   (route: %s)", S.belt ~= nil and "yes" or "no", S.route))
        ImGui.Text(string.format("render plane param: %s   applied: %d",
            CFG.render_plane_param, S.planeWrites))
        ImGui.Text(string.format("grenade: %s   weapon: %s   in hand: %s   SceneTier: %d",
            tostring(S.state.grenade), tostring(S.state.weapon), tostring(S.state.inhand), S.state.tier))

        ImGui.Separator()
        ImGui.Text("colour")
        for i = 1, #CFG.variants do
            if ImGui.RadioButton(CFG.variant_names[i] or CFG.variants[i], CFG.variant == i) then
                CFG.variant = i
                if not applyColor() then
                    -- ChangeAppearance unavailable: fall back to the variant ITEM, which means taking
                    -- the belt off and putting the other colour on. Slower, and visible for a frame.
                    detach()
                    S.lastEquipTry = 0.0
                end
                saveCfg()
            end
            if i < #CFG.variants then ImGui.SameLine() end
        end

        ImGui.Separator()
        ImGui.Text("elements")
        local comps = S.comps
        local names = {}
        for nm, _ in pairs(comps) do names[#names + 1] = nm end
        table.sort(names)
        if #names == 0 then
            ImGui.Text("  no components yet -- the belt is not on")
        end
        for _, nm in ipairs(names) do
            local rule, fromElement = ruleFor(nm)
            if fromElement then rule = rule .. " (el)" end
            local enabled = "?"
            pcall(function() enabled = tostring(comps[nm]:IsEnabled()) end)
            local worn = S.dressed[nm]
            ImGui.Text(string.format("  %-18s rule=%-8s on=%-5s %s", nm, rule, enabled,
                worn ~= nil and ("[" .. worn .. "]") or ""))
            if ImGui.SmallButton("always##" .. nm) then setRule(nm, "always") end
            ImGui.SameLine()
            if ImGui.SmallButton("never##" .. nm) then setRule(nm, "never") end
            ImGui.SameLine()
            if ImGui.SmallButton("grenade##" .. nm) then setRule(nm, "grenade") end
            ImGui.SameLine()
            if ImGui.SmallButton("weapon##" .. nm) then setRule(nm, "weapon") end
            ImGui.SameLine()
            if ImGui.SmallButton("inhand##" .. nm) then setRule(nm, "inhand") end
        end

        ImGui.Separator()
        CFG.grenade_grab = ImGui.Checkbox("take the grenade off the belt on grip", CFG.grenade_grab)
        CFG.grenade_grab_plane = ImGui.SliderInt("plane: 0 follow / 1 scene / 2 weapon##gg",
            CFG.grenade_grab_plane or 0, 0, 2)
        CFG.grenade_grab_pose = ImGui.Checkbox("recorded grip: the lever under the thumb",
            CFG.grenade_grab_pose)
        CFG.grab_trace = ImGui.Checkbox("trace every grab step to grab_trace.log", CFG.grab_trace)
        ImGui.Text(string.format("   in hand: %s  slot %s  plane %s  parts %s",
            tostring(S.grab.host or "-"), tostring(S.grab.slot or "-"), tostring(S.grab.plane or "-"),
            S.grab.parts and tostring(#S.grab.parts) or "-"))
        if ImGui.CollapsingHeader("как граната сидит в руке") then
            -- WHOSE HOLD IS BEING EDITED, said out loud: these sliders move the type currently held and
            -- nothing else. With one shared set, every fix for one grenade broke another.
            local hk = S.grab.kind
            if hk ~= nil then
                CFG.grenade_hold_kind = CFG.grenade_hold_kind or {}
                if CFG.grenade_hold_kind[hk] == nil then
                    local g = CFG.grenade_hold or {}
                    CFG.grenade_hold_kind[hk] = { x = g.x or 0.0, y = g.y or 0.0, z = g.z or 0.0,
                                                  rx = g.rx or 0.0, ry = g.ry or 0.0, rz = g.rz or 0.0 }
                end
            end
            ImGui.Text(string.format("   правится: %s", hk or "общая (ничего в руке)"))
            -- THE THING HAS TO BE IN THE HAND TO BE AIMED AT. Gripping puts it there and releasing
            -- takes it away, which is exactly wrong for tuning: the sliders are on the other side of
            -- the overlay and both hands are needed for them. These put it there and leave it.
            if ImGui.Button("в правую руку") then
                if GRENADE then GRENADE.pin(1, grenadeElementShowing()) end
            end
            ImGui.SameLine()
            if ImGui.Button("в левую") then
                if GRENADE then GRENADE.pin(0, grenadeElementShowing()) end
            end
            ImGui.SameLine()
            if ImGui.Button("убрать") then
                if GRENADE then GRENADE.unpin() end
            end
            ImGui.Text(string.format("   закреплена: %s   до чеки %s   тяга %s",
                (GRENADE and GRENADE.pinned()) and "да" or "нет",
                S.grab.pinDist and string.format("%.3f", S.grab.pinDist) or "-",
                S.grab.pinPull and string.format("%.3f", S.grab.pinPull) or "-"))
            ImGui.Text(string.format("   вдоль оси: %s м   скорость: %s м/с",
                S.grab.pinAlong and string.format("%.3f", S.grab.pinAlong) or "-",
                S.grab.pinSpeed and string.format("%.2f", S.grab.pinSpeed) or "-"))
            CFG.grenade_pin = ImGui.Checkbox("чека выдёргивается свободной рукой", CFG.grenade_pin)
            CFG.grenade_pin_preview_m = ImGui.DragFloat("preview, м##pin",
                CFG.grenade_pin_preview_m, 0.005, 0.05, 0.50, "%.3f")
            CFG.grenade_pin_grip_m = ImGui.DragFloat("захват, м##pin",
                CFG.grenade_pin_grip_m, 0.005, 0.02, 0.40, "%.3f")
            CFG.grenade_pin_slide_m = ImGui.DragFloat("выход чеки, м##pin",
                CFG.grenade_pin_slide_m, 0.001, 0.000, 0.05, "%.4f")
            -- THE AXIS BELONGS TO THE TYPE IN THE HAND. Tuning it while holding an incendiary must not
            -- move the frag's, which is already right; the incendiary's pin came out the wrong way.
            local kind = S.grab.kind
            local pa = CFG.grenade_pin_axis
            if kind ~= nil then
                CFG.grenade_pin_axis_kind = CFG.grenade_pin_axis_kind or {}
                if CFG.grenade_pin_axis_kind[kind] == nil then
                    CFG.grenade_pin_axis_kind[kind] = { x = pa.x, y = pa.y, z = pa.z }
                end
                pa = CFG.grenade_pin_axis_kind[kind]
            end
            ImGui.Text(string.format("   направление выхода (куда едет чека) -- %s",
                kind or "по умолчанию"))
            pa.x = ImGui.DragFloat("выход x##pin", pa.x, 0.01, -1.0, 1.0, "%.3f")
            pa.y = ImGui.DragFloat("выход y##pin", pa.y, 0.01, -1.0, 1.0, "%.3f")
            pa.z = ImGui.DragFloat("выход z##pin", pa.z, 0.01, -1.0, 1.0, "%.3f")

            ImGui.Separator()
            ImGui.Text(string.format("   рычаг: %s",
                S.grab.lever and string.format("%.2f", S.grab.lever) or "-"))
            CFG.grenade_lever = ImGui.Checkbox("триггер прожимает рычаг", CFG.grenade_lever)
            CFG.grenade_trigger_block = ImGui.Checkbox("не стрелять, пока граната в руке",
                CFG.grenade_trigger_block)
            CFG.grenade_lever_speed = ImGui.DragFloat("скорость прожатия##lev",
                CFG.grenade_lever_speed, 0.5, 1.0, 30.0, "%.1f")
            CFG.grenade_lever_open = ImGui.DragFloat("раскрытие рычага##lev",
                CFG.grenade_lever_open, 0.05, -2.0, 0.0, "%.2f")

            ImGui.Separator()
            ImGui.Text(string.format("   рука: %s м/с   пик: %s м/с",
                S.grab.throwSpeed and string.format("%.2f", S.grab.throwSpeed) or "-",
                S.grab.throwPeak and string.format("%.2f", S.grab.throwPeak) or "-"))
            CFG.grenade_throw = ImGui.Checkbox("бросок: отпустить грип с выдернутой чекой", CFG.grenade_throw)
            ImGui.Text("   1.0 = ровно как кисть,  3.5 = как кидает игра (19-25 м/с)")
            CFG.grenade_throw_gain = ImGui.DragFloat("сила броска##thr",
                CFG.grenade_throw_gain, 0.05, 0.2, 5.0, "%.2f")
            CFG.grenade_throw_min = ImGui.DragFloat("мин, м/с##thr",
                CFG.grenade_throw_min, 0.1, 0.0, 10.0, "%.1f")
            CFG.grenade_throw_max = ImGui.DragFloat("макс, м/с##thr",
                CFG.grenade_throw_max, 0.5, 5.0, 60.0, "%.1f")
            CFG.grenade_throw_window = ImGui.DragFloat("окно пика, с##thr",
                CFG.grenade_throw_window, 0.01, 0.05, 0.60, "%.2f")
            CFG.grenade_throw_consume = ImGui.Checkbox("списывать гранату из инвентаря",
                CFG.grenade_throw_consume)
            ImGui.Text(string.format("   гранат в инвентаре: %s",
                (S.state and S.state.gcount) and tostring(S.state.gcount) or "-"))

            ImGui.Separator()
            CFG.grenade_sound = ImGui.Checkbox("звуки гранаты", CFG.grenade_sound)
            -- Each name is playable on its own, so a candidate can be heard without doing the gesture
            -- that fires it -- which is the only way to tell a wrong name from a silent one.
            for _, k in ipairs({ "grab", "pin", "lever", "leverOff", "throw", "stow" }) do
                local key = "grenade_snd_" .. k
                CFG[key] = ImGui.InputText(k .. "##snd", CFG[key] or "", 64)
                ImGui.SameLine()
                if ImGui.SmallButton("play##" .. k) then
                    pcall(function()
                        Game.GetAudioSystem():Play(CName.new(CFG[key]),
                            Game.GetPlayer():GetEntityID(), CName.new(""))
                    end)
                end
            end
            CFG.grenade_pin_pull_m = ImGui.DragFloat("рывок сверх упора, м##pin",
                CFG.grenade_pin_pull_m, 0.005, 0.01, 0.30, "%.3f")
            CFG.grenade_pin_speed = ImGui.DragFloat("рывок, м/с##pin",
                CFG.grenade_pin_speed, 0.05, 0.0, 6.0, "%.2f")
            -- WHERE the hand takes hold is not the same as WHERE the pin goes, and the two are easy to
            -- confuse: ring z only moves the hand's magnet higher. It does, and the labels now say so.
            -- THE GRAB POINT BELONGS TO THE TYPE IN THE HAND, on the same terms as the axis above:
            -- seeded from the shared number so nothing moves until a slider is touched.
            local po = CFG.grenade_pin_off
            if kind ~= nil then
                CFG.grenade_pin_off_kind = CFG.grenade_pin_off_kind or {}
                if CFG.grenade_pin_off_kind[kind] == nil then
                    CFG.grenade_pin_off_kind[kind] = { x = po.x, y = po.y, z = po.z }
                end
                po = CFG.grenade_pin_off_kind[kind]
            end
            ImGui.Text(string.format("   точка хвата (где встаёт рука) -- %s", kind or "по умолчанию"))
            po.x = ImGui.DragFloat("хват x##pin", po.x, 0.002, -0.10, 0.10, "%.4f")
            po.y = ImGui.DragFloat("хват y##pin", po.y, 0.002, -0.10, 0.10, "%.4f")
            po.z = ImGui.DragFloat("хват z##pin", po.z, 0.002, -0.10, 0.10, "%.4f")
            -- The numbers have to be able to LEAVE the session: belt.cfg keeps them, but baking one
            -- into the defaults means reading it, and a slider position is not readable.
            if ImGui.SmallButton("в лог##pin") then
                log("pin %s  хват %.4f %.4f %.4f   выход %.3f %.3f %.3f",
                    kind or "default", po.x or 0.0, po.y or 0.0, po.z or 0.0,
                    pa.x or 0.0, pa.y or 0.0, pa.z or 0.0)
            end
            ImGui.SameLine()
            if ImGui.SmallButton("все в лог##pin") then
                for k2, v2 in pairs(CFG.grenade_pin_off_kind or {}) do
                    local a2 = (CFG.grenade_pin_axis_kind or {})[k2] or CFG.grenade_pin_axis
                    log("pin %s  хват %.4f %.4f %.4f   выход %.3f %.3f %.3f",
                        k2, v2.x or 0.0, v2.y or 0.0, v2.z or 0.0,
                        a2.x or 0.0, a2.y or 0.0, a2.z or 0.0)
                end
            end
            local o = (hk ~= nil and CFG.grenade_hold_kind and CFG.grenade_hold_kind[hk])
                   or CFG.grenade_hold
            o.x = ImGui.DragFloat("x##gh", o.x, 0.002, -0.4, 0.4, "%.4f")
            o.y = ImGui.DragFloat("y##gh", o.y, 0.002, -0.4, 0.4, "%.4f")
            o.z = ImGui.DragFloat("z##gh", o.z, 0.002, -0.4, 0.4, "%.4f")
            o.rx = ImGui.DragFloat("roll X##gh", o.rx, 0.5, -180.0, 180.0, "%.1f")
            o.ry = ImGui.DragFloat("pitch Y##gh", o.ry, 0.5, -180.0, 180.0, "%.1f")
            o.rz = ImGui.DragFloat("yaw Z##gh", o.rz, 0.5, -180.0, 180.0, "%.1f")
            if ImGui.Button("0##gh") then
                o.x, o.y, o.z, o.rx, o.ry, o.rz = 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
            end
            ImGui.SameLine()
            if ImGui.Button("в лог##gh") then
                log("grenade_hold  %.4f %.4f %.4f   %.1f %.1f %.1f", o.x, o.y, o.z, o.rx, o.ry, o.rz)
            end
        end
        CFG.highlight = ImGui.Checkbox("light the element the hand reaches for", CFG.highlight)
        CFG.probe_devices = ImGui.Checkbox("PROBE: re-assert engine outline on nearby devices", CFG.probe_devices)
        ImGui.Text(string.format("   re-asserted this tick: %d", S.probeCount or 0))
        ImGui.Text(string.format("nearest: %s   %s m   radius %.2f",
            tostring(S.lit or "-"),
            S.handDist ~= nil and string.format("%.3f", S.handDist) or "-",
            CFG.highlight_radius_m))
        -- BOTH HANDS, NAMED. One number for "nearest" hides which hand found it and what it found,
        -- and that is most of a distance measured from who knows where.
        for side = 0, 1 do
            local h = (S.handSide or {})[side]
            ImGui.Text(string.format("   %s: %s   %s m",
                side == 1 and "правая" or "левая",
                h and tostring(h.best or "-") or "-",
                (h and h.d) and string.format("%.3f", h.d) or "-"))
        end
        CFG.highlight_radius_m = ImGui.DragFloat("radius##hl", CFG.highlight_radius_m, 0.005, 0.03, 0.40, "%.3f")
        CFG.crouch_reach_scale = ImGui.DragFloat("x crouched##hl", CFG.crouch_reach_scale or 1.5, 0.05, 1.0, 3.0, "%.2f")

        ImGui.Text("точка на руке: костяшка среднего пальца + смещение в осях ладони (м)")
        local ho = CFG.highlight_hand_off
        ho.x = ImGui.DragFloat("поперёк ладони##hoff", ho.x or 0.0, 0.002, -0.20, 0.20, "%.3f")
        ho.y = ImGui.DragFloat("вдоль пальцев##hoff", ho.y or 0.0, 0.002, -0.20, 0.20, "%.3f")
        ho.z = ImGui.DragFloat("из ладони##hoff", ho.z or 0.0, 0.002, -0.20, 0.20, "%.3f")
        if ImGui.SmallButton("0##hoff") then ho.x, ho.y, ho.z = 0.0, 0.0, 0.0 end

        ImGui.Separator()
        ImGui.Text("element offsets (metres, entity space: X across, Y fore-aft, Z up)")
        for _, el in ipairs(CFG.elements) do
            ImGui.Text(string.format("%s on %s", el.name, el.host))
            local ch
            el.ox, ch = ImGui.DragFloat("X##" .. el.name, el.ox or 0.0, 0.002, -0.5, 0.5, "%.4f")
            el.oy, ch = ImGui.DragFloat("Y##" .. el.name, el.oy or 0.0, 0.002, -0.5, 0.5, "%.4f")
            el.oz, ch = ImGui.DragFloat("Z##" .. el.name, el.oz or 0.0, 0.002, -0.5, 0.5, "%.4f")
            if ImGui.SmallButton("zero##" .. el.name) then el.ox, el.oy, el.oz = 0.0, 0.0, 0.0 end
            ImGui.SameLine()
            if ImGui.SmallButton("save##" .. el.name) then
                saveCfg()
                log("%s offset saved: %.4f %.4f %.4f", el.name, el.ox, el.oy, el.oz)
            end
        end

        ImGui.Separator()
        -- WHICH PIECES OF THE BELT ARE DRAWN. Thirteen sub-meshes in one mesh sharing one material,
        -- so nothing but the eye can say which is which: tick one, look, tick the next.
        ImGui.Separator()
        -- THE BELT ITSELF, piece by piece. Untick the all-in-one and tick `strap` to get a bare belt;
        -- everything else is optional hardware.
        -- THE TABLE IN FORCE, not always the male one: the two bodies wear different meshes, so
        -- editing the wrong one looks like the panel doing nothing.
        local fem = (bodyTag() == "wa")
        ImGui.Text(fem and "тело: женское (i1_016_wa_belt__police_eq)"
                        or "тело: мужское (куски i1_016_ma_belt__police*)")
        local aio = ImGui.Checkbox("пояс целиком (с подсумками)", beltAllInOne())
        if fem then CFG.belt_all_in_one_female = aio else CFG.belt_all_in_one = aio end
        ImGui.Text("отдельные куски (галка = показать)")
        local bp = beltPieceTable()
        if type(bp) ~= "table" then
            bp = {}
            if fem then CFG.belt_pieces_female = bp else CFG.belt_pieces = bp end
        end
        local shown = 0
        for i, k in ipairs(BELT_PIECES) do
            if i % 3 ~= 1 then ImGui.SameLine() end
            bp[k] = ImGui.Checkbox(k .. "##piece", bp[k] == true)
            if bp[k] == true then shown = shown + 1 end
        end
        ImGui.Text(string.format("   включено кусков: %d   компонентов найдено: %d",
            shown, (function() local n = 0 for _ in pairs(S.beltParts or {}) do n = n + 1 end return n end)()))
        if ImGui.SmallButton("голый ремень##piece") then
            -- On the female body the pieces do not exist, so the bare belt there is the all-in-one
            -- with its pouches masked off, not a strap component.
            for _, k in ipairs(BELT_PIECES) do bp[k] = false end
            if fem then
                CFG.belt_all_in_one_female = true
            else
                CFG.belt_all_in_one = false
                bp.strap, bp.back = true, true
            end
        end
        ImGui.SameLine()
        if ImGui.SmallButton("вернуть как было##piece") then
            for _, k in ipairs(BELT_PIECES) do bp[k] = false end
            if fem then CFG.belt_all_in_one_female = true else CFG.belt_all_in_one = true end
        end
        ImGui.SameLine()
        if ImGui.SmallButton("в лог##piece") then
            local on = {}
            for _, k in ipairs(BELT_PIECES) do if bp[k] == true then on[#on + 1] = k end end
            log("belt pieces: целиком=%s, куски [%s]", tostring(CFG.belt_all_in_one ~= false),
                table.concat(on, ","))
        end

        ImGui.Separator()
        ImGui.Text("куски пояса (галка = спрятать)")
        local off = beltChunkTable()
        if type(off) ~= "table" then
            off = {}
            if bodyTag() == "wa" then CFG.belt_chunk_off_female = off else CFG.belt_chunk_off = off end
        end
        for i = 0, 15 do
            if i % 4 ~= 0 then ImGui.SameLine() end
            off[i + 1] = ImGui.Checkbox(tostring(i) .. "##chunk", off[i + 1] == true)
        end
        local m = 0
        for i = 0, 31 do
            if off[i + 1] ~= true then m = m + 2 ^ i end
        end
        ImGui.Text(string.format("   маска: %.0f   действует на: %s", m,
            tostring(S.beltCompName or "-")))
        if ImGui.SmallButton("все видны##chunk") then
            for i = 1, 32 do off[i] = false end
        end
        ImGui.SameLine()
        if ImGui.SmallButton("в лог##chunk") then
            local hid = {}
            for i = 0, 31 do if off[i + 1] == true then hid[#hid + 1] = tostring(i) end end
            log("belt chunks: маска %.0f, спрятаны [%s]", m, table.concat(hid, ","))
        end

        ImGui.Separator()
        CFG.garment_offsets = ImGui.Checkbox("ArchiveXL garment offsets", CFG.garment_offsets)
        if ImGui.SmallButton("colour route: " .. CFG.color_route) then
            CFG.color_route = (CFG.color_route == "item") and "component" or "item"
        end
        CFG.hide_in_cutscene = ImGui.Checkbox("hide in cutscenes", CFG.hide_in_cutscene)
        CFG.verbose = ImGui.Checkbox("log every toggle", CFG.verbose)
        if ImGui.Button("equip now") then S.lastEquipTry = 0.0; ensureEquipped(os.clock()) end
        ImGui.SameLine()
        if ImGui.Button("take off") then detach() end
        ImGui.SameLine()
        if ImGui.Button("dump") then
            S.dumped = false
            S.beltHash = nil
            beltComponents()
            -- ...and the player's own components, in case the garment parts land there instead of on
            -- the item object. Which of the two it is decides where every later step has to look.
            local pl = Game.GetPlayer()
            local list = nil
            pcall(function() list = Game.VREntityComponents(pl) end)
            if list ~= nil then
                local hits = {}
                for i = 1, #list do
                    local nm = nil
                    pcall(function() nm = tostring(list[i]:GetName().value) end)
                    if nm ~= nil and string.find(nm, "belt", 1, true) then hits[#hits + 1] = nm end
                end
                log("player carries %d belt-ish component(s) of %d: %s", #hits, #list, table.concat(hits, ", "))
            else
                log("the player's components are unavailable")
            end
        end

        ImGui.Separator()
        if ImGui.CollapsingHeader("belt props (outlineable elements)") then
            ImGui.TextWrapped("An element that can be outlined has to be an ITEM in a slot, not a mesh " ..
                "on the belt: a mesh there is a component on the player and the engine outline never " ..
                "reaches a component. Sliders are metres, in the item's own frame -- up, back, right.")
            for i = 1, #CFG.props do
                local p = CFG.props[i]
                ImGui.PushID("prop" .. i)
                local slotShort = string.gsub(p.slot, "AttachmentSlots%.", "")
                if p.follow ~= nil then slotShort = slotShort .. "  <- " .. p.follow end
                p.on = ImGui.Checkbox(slotShort, p.on)
                ImGui.SameLine()
                p.auto = ImGui.Checkbox("hand lights it", p.auto)
                ImGui.SameLine()
                p.lit = ImGui.Checkbox("always lit", p.lit)
                if p.on then
                    local d = propS.dist[i]
                    if p.follow ~= nil then
                        ImGui.Text("   entity: " .. tostring(propS.item[i] or "none equipped"))
                    end
                    ImGui.Text(string.format("   hand: %s   outline: %s",
                        d and string.format("%.3f m", d) or "--",
                        propS.lit[i] and "ON" or "off"))
                end
                if p.on then
                    p.up    = ImGui.DragFloat("up##" .. i,    p.up,    0.002, -0.60, 0.60, "%.3f")
                    p.back  = ImGui.DragFloat("back##" .. i,  p.back,  0.002, -0.60, 0.60, "%.3f")
                    p.right = ImGui.DragFloat("right##" .. i, p.right, 0.002, -0.60, 0.60, "%.3f")
                    p.rx = ImGui.DragFloat("turn X (roll)##" .. i,  p.rx or 0.0, 0.5, -180.0, 180.0, "%.1f")
                    p.ry = ImGui.DragFloat("turn Y (pitch)##" .. i, p.ry or 0.0, 0.5, -180.0, 180.0, "%.1f")
                    p.rz = ImGui.DragFloat("turn Z (yaw)##" .. i,   p.rz or 0.0, 0.5, -180.0, 180.0, "%.1f")
                    local qi, qj, qk, qr = propQuat(p)
                    ImGui.Text(string.format("quat  i %.5f  j %.5f  k %.5f  r %.5f", qi, qj, qk, qr))
                    if ImGui.SmallButton("zero pos") then p.up, p.back, p.right = 0.0, 0.0, 0.0 end
                    ImGui.SameLine()
                    if ImGui.SmallButton("zero turn") then p.rx, p.ry, p.rz = 0.0, 0.0, 0.0 end
                end
                ImGui.PopID()
                ImGui.Separator()
            end
            -- PLANE AND MARK, together, because one dictates the other. On RPl_Scene the props are in
            -- the world: draw a weapon and the gun and the arms, which are on the first-person plane,
            -- composite against them and it reads wrong. RPl_Weapon fixes that and takes the engine
            -- outline away with it, because the outline reaches nothing off the scene plane. The glow
            -- is what marks a prop there.
            -- NO PLANE WIDGET. The plane follows the player and is not a setting -- a control that
            -- changes nothing is worse than no control. See the note in CFG.
            -- THE PLANE READ-OUT. Not decoration: whether the player's plane can be read at all, and
            -- whether the props actually followed it, are two separate unknowns, and this line answers
            -- both without another build. `player` is the plane the module believes the player is in,
            -- `src` is where that number came from -- `vest` is the subscribed skinned component, i.e.
            -- the engine's own answer, `weapon` is the fallback guess -- and `props` is the plane last
            -- written onto the props. 0 = RPl_Scene, 2 = RPl_Weapon.
            do
                ImGui.Text(string.format("plane: player=%s  src=%s  props=%s  wr=%s",
                                         tostring(planeS.val or "nil"), tostring(planeS.src),
                                         tostring(propS.plane[1] or "nil"), tostring(planeS.wr)))
            end
            -- EVERY BELT COMPONENT, THREE SLIDERS EACH. The list is whatever is actually on the
            -- player right now, so it covers the belt's own pouches and anything the port added.
            -- Values are metres in entity space: x right, y forward, z up.
            if ImGui.CollapsingHeader("положение элементов пояса") then
                -- NO SLIDERS FOR THE MARK TWINS. A `*_lit` component draws the same mesh as the
                -- element it marks, so it is moved by the same bake and there is nothing to tune on it
                -- separately -- a second set of sliders for it is a way to put the mark somewhere its
                -- own element is not.
                local names = {}
                for nm, _ in pairs(S.comps) do
                    if string.sub(nm, -4) ~= "_lit" and OFFSET_GROUP[nm] == nil then
                        names[#names + 1] = nm
                    end
                end
                table.sort(names)
                -- WHAT IS ON THE BODY COMES FIRST. With a magazine per weapon this list runs to eighty
                -- entries, and the only one worth reaching for is whatever is actually drawn right now
                -- -- so those are lifted to the top and opened, and the rest wait underneath.
                local live, rest = {}, {}
                for _, nm in ipairs(names) do
                    local on = false
                    pcall(function() on = S.comps[nm]:IsEnabled() end)
                    if on then live[#live + 1] = nm else rest[#rest + 1] = nm end
                end
                names = {}
                for _, nm in ipairs(live) do names[#names + 1] = nm end
                for _, nm in ipairs(rest) do names[#names + 1] = nm end
                local liveCount = #live
                -- WHAT THE LIST ACTUALLY HOLDS. "the magazines are not shown" can mean two different
                -- faults -- the module never found them, or it found them and the rows are not drawn --
                -- and one line separates the two without another round trip.
                local ng, nmag, nam = 0, 0, 0
                for _, x in ipairs(names) do
                    if string.sub(x, 1, 13) == "ucbeltgrenade" then ng = ng + 1
                    elseif string.sub(x, 1, 10) == "ucbeltmag_" then nmag = nmag + 1
                    elseif string.sub(x, 1, 11) == "ucbeltammo_" then nam = nam + 1 end
                end
                ImGui.Text(string.format("в списке %d: гранат %d, магазинов %d, патронов %d, на теле %d",
                    #names, ng, nmag, nam, liveCount))
                -- A FILTER INSTEAD OF FORTY-SIX COLLAPSING HEADERS.
                --
                -- The headers were the bug that kept being hit: the magazines are almost always
                -- disabled, so they landed in the closed half of the list and their sliders could not
                -- be reached: no editing shown for the magazine ones. Whatever the header state
                -- was doing, it is not worth debugging: type a few letters and the rows appear, open.
                if type(S.offFilter) ~= "string" then S.offFilter = "" end
                -- IN A pcall, AND ITS RESULT TYPE-CHECKED. An ImGui call that throws takes the REST OF
                -- THE SECTION with it -- everything after it in this draw simply does not appear, which
                -- is indistinguishable from "the rows are missing". `InputText` is the only call here
                -- whose presence and signature this module has never verified in this CET build, so it
                -- is the one that gets the guard; the buttons below reach every row without it.
                local okf, txt = pcall(function()
                    return ImGui.InputText("фильтр##off", S.offFilter, 64)
                end)
                if okf and type(txt) == "string" then
                    S.offFilter = txt
                elseif not okf then
                    ImGui.Text("(поле ввода недоступно -- пользуйся кнопками)")
                end
                if ImGui.SmallButton("всё##offfilter") then S.offFilter = "ucbelt" end
                ImGui.SameLine()
                if ImGui.SmallButton("на теле##offfilter") then S.offFilter = "" end
                ImGui.SameLine()
                if ImGui.SmallButton("маг##offfilter") then S.offFilter = "ucbeltmag_" end
                ImGui.SameLine()
                if ImGui.SmallButton("гран##offfilter") then S.offFilter = "ucbeltgrenade" end
                ImGui.SameLine()
                if ImGui.SmallButton("патр##offfilter") then S.offFilter = "ucbeltammo_" end
                ImGui.Text(string.format("фильтр = [%s]", tostring(S.offFilter)))
                if ImGui.SmallButton("в лог##offlist") then
                    log("offsets list: всего %d, гранат %d, магазинов %d, патронов %d, на теле %d",
                        #names, ng, nmag, nam, liveCount)
                    log("offsets list: %s", table.concat(names, ", "))
                end
                -- THE LAYER THE BODY IS ACTUALLY WEARING, so a slider moved here is the slider
                -- that moves the belt. Editing the male layer while standing on a female body would
                -- look like nothing happening at all.
                local OT = offsetTable()
                for i, nm in ipairs(names) do
                    local o = OT[nm]
                    if o == nil then
                        o = { x = 0.0, y = 0.0, z = 0.0, rx = 0.0, ry = 0.0, rz = 0.0 }
                        OT[nm] = o
                    end
                    -- ALL SIX, not just the rotations. A table that carries only the numbers that were non-zero is a
                    -- table whose `o.x` is nil, and `math.abs(nil)` two lines down throws -- taking every row
                    -- after it out of the panel. That is what the missing magazine rows were, on the female
                    -- body only, because that layer was generated that way and the male one never was.
                    o.x, o.y, o.z = o.x or 0.0, o.y or 0.0, o.z or 0.0
                    o.rx, o.ry, o.rz = o.rx or 0.0, o.ry or 0.0, o.rz or 0.0
                    -- Collapsed, or eighty elements of six sliders each is a wall rather than a panel.
                    -- A `*` marks one that has been tuned, so it can be found again after the weapon
                    -- it belongs to has been swapped away.
                    --
                    -- NO `goto` HERE. A jump past the locals below is exactly the construct LuaJIT
                    -- rejects, and this module has already failed to load once over it -- so the body
                    -- is nested under the header instead of skipped around.
                    local tuned = (math.abs(o.x) + math.abs(o.y) + math.abs(o.z)
                                   + math.abs(o.rx) + math.abs(o.ry) + math.abs(o.rz)) > 1e-6
                    -- Shown when it matches the filter, or when it is on the body and nothing is
                    -- typed -- so the panel opens on what is being worn and everything else is one
                    -- word away.
                    local f = S.offFilter or ""
                    local open = (f ~= "" and string.find(nm, f, 1, true) ~= nil)
                                 or (f == "" and i <= liveCount)
                    if open then
                    local label = nm
                    if i <= liveCount then label = label .. "  <- на теле" end
                    if tuned then label = label .. "  *" end
                    ImGui.Text(label)
                    o.x = ImGui.DragFloat("x##" .. nm, o.x, 0.002, -0.6, 0.6, "%.4f")
                    o.y = ImGui.DragFloat("y##" .. nm, o.y, 0.002, -0.6, 0.6, "%.4f")
                    o.z = ImGui.DragFloat("z##" .. nm, o.z, 0.002, -0.6, 0.6, "%.4f")
                    o.rx = ImGui.DragFloat("roll X##" .. nm, o.rx, 0.5, -180.0, 180.0, "%.1f")
                    o.ry = ImGui.DragFloat("pitch Y##" .. nm, o.ry, 0.5, -180.0, 180.0, "%.1f")
                    o.rz = ImGui.DragFloat("yaw Z##" .. nm, o.rz, 0.5, -180.0, 180.0, "%.1f")
                    -- Any movement schedules a write; the write itself happens a second later, off the
                    -- draw path, so dragging a slider does not touch the disk on every frame.
                    local sig = string.format("%.5f/%.5f/%.5f/%.3f/%.3f/%.3f", o.x, o.y, o.z, o.rx, o.ry, o.rz)
                    if S.offSig == nil then S.offSig = {} end
                    if S.offSig[nm] ~= sig then S.offSig[nm], S.offDirty = sig, os.clock() end
                    if ImGui.Button("0##" .. nm) then
                        o.x, o.y, o.z, o.rx, o.ry, o.rz = 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
                    end
                    ImGui.Separator()
                    end
                end
                if ImGui.Button("записать offsets.json") then S.offDirty = 0.0 end
                ImGui.SameLine()
                if ImGui.Button("напечатать всё в лог") then
                    for _, nm in ipairs(names) do
                        local o = CFG.offsets[nm] or {}
                        local qi, qj, qk, qr = eulerQuat(o.rx, o.ry, o.rz)
                        log("offset %-20s pos %.4f %.4f %.4f  deg %.1f %.1f %.1f  quat %.6f %.6f %.6f %.6f",
                            nm, o.x or 0, o.y or 0, o.z or 0, o.rx or 0, o.ry or 0, o.rz or 0,
                            qi, qj, qk, qr)
                    end
                end
            end
            ImGui.Text("mark")
            CFG.prop_mark = ImGui.DragInt("mark: 0 outline  1 material  2 both  3 light", CFG.prop_mark or 1, 1, 0, 3)
            CFG.prop_glow_level = ImGui.DragInt("colour: 1 magenta 2 white 3 cyan 4 green 5 amber 6 red 7 blue 8 violet", CFG.prop_glow_level or 1, 1, 1, 8)
            CFG.prop_rim_scale = ImGui.DragFloat("rim thickness (twin scale)", CFG.prop_rim_scale or 1.05, 0.002, 1.0, 1.3, "%.3f")
            CFG.prop_glow_lm = ImGui.DragFloat("glow, lumens", CFG.prop_glow_lm or 60.0, 1.0, 0.0, 600.0, "%.0f")
            CFG.prop_glow_r = ImGui.DragFloat("glow radius, m", CFG.prop_glow_r or 0.35, 0.01, 0.05, 3.0, "%.2f")
            ImGui.Separator()
            CFG.prop_radius_m = ImGui.DragFloat("light-up radius", CFG.prop_radius_m, 0.002, 0.01, 0.50, "%.3f")
            CFG.prop_release = ImGui.DragFloat("release factor", CFG.prop_release, 0.01, 1.0, 2.5, "%.2f")
            CFG.prop_renew_s = ImGui.DragFloat("claim renew, s", CFG.prop_renew_s, 0.01, 0.05, 2.0, "%.2f")
            CFG.prop_outline = ImGui.DragInt("outline type (8 = white, ours)", CFG.prop_outline, 1, 0, 9)
            CFG.prop_context = ImGui.DragInt("context (1 = line only)", CFG.prop_context, 1, 0, 3)
            CFG.prop_type = ImGui.DragInt("highlight type", CFG.prop_type, 1, 0, 9)
            ImGui.TextWrapped("Values are written to belt.cfg when the overlay closes, so a tuned " ..
                "position survives a restart.")
        end

        ImGui.End()
    end)
end)

registerForEvent("onShutdown", function()
    saveCfg()
    -- WHAT THIS MODULE HOLDS ON THE PLUGIN SIDE HAS TO BE GIVEN BACK. Three things outlive a mod
    -- reload because they live in the plugin's shared block, not in Lua: the swallowed trigger
    -- (slot 161), the magnetised wrist and the wrist claim. Reload the belt with a grenade in hand and
    -- a latched 161 is a gun that never fires again -- the plugin's own note on that slot says so in
    -- as many words -- so the hold is ended here whatever else happens, and the trigger is handed back
    -- a second time directly in case it never got that far.
    if GRENADE then
        pcall(function() GRENADE.release(Game.GetPlayer()) end)
        -- ...and the LIVE one, if a pin was pulled and never thrown: a real grenade left attached
        -- across a mod reload is a grenade nobody owns.
        pcall(function() GRENADE.disarm(Game.GetPlayer()) end)
    end
    if type(SetVRTriggerMode) == "function" then pcall(function() SetVRTriggerMode(0) end) end
end)

-- WHAT OTHER MODS MAY ASK THIS ONE, and the only thing in this file that is not private.
--
-- The reload module takes a magazine off the belt instead of conjuring one at the hand, and to do
-- that it has to know three things: whether the weapon in hand HAS a pouch here, how far each palm is
-- from it, and it has to be able to say "I am carrying it now" so the pouch draws empty. CET hands a
-- mod's return value to `GetMod("CyberpunkVRPort_TacticalBelt")`, which is the cheapest channel there
-- is -- no shared slot, no redscript field, no per-frame file.
--
-- Everything here is a read of state this module computes anyway; nothing starts work of its own.
return {
    performance = PERFORMANCE,
    -- What the belt last read of the player (readState): the weapon whose ammunition it carries, the grenade, ...
    state = function() return S.state end,
    -- THE GRIP AT A LIT GRENADE HOLDER BELONGS TO THE GRENADE. The holster's right-hip zone reaches the
    -- grenade holders, so one squeeze there used to draw (or put away) a weapon AND take a grenade into
    -- the same hand slot. The holster asks this and stands down while it is true. It is false whenever
    -- a weapon is in hand -- the holders do not light then -- so putting the weapon away still works.
    -- A grenade already in that hand, or a grab of it waiting on the weapon, counts too: the two mods run
    -- in no fixed order, and the belt may have taken this very squeeze earlier in the same frame.
    grenadeReach = function(side)
        if not CFG.grenade_grab then return false end
        if S.grab.slot ~= nil then return S.grab.side == side end
        if S.grab.pending ~= nil then return S.grab.pending.side == side end
        local h = S.litHand[side]
        return h ~= nil and string.sub(h, 1, 13) == "ucbeltgrenade"
    end,
    -- The grenade module itself, for the panel's own controls driven from a tool (pin / unpin), and the
    -- config, so a test can set the grab plane without the overlay.
    grenade = GRENADE,
    cfg = CFG,
    -- The ammunition holders actually drawn right now (magazines and the shell row), for tests: a picture from the
    -- head cannot see past the vest to the hips.
    shownAmmo = function()
        local out = {}
        for nm, c in pairs(S.comps) do
            if string.sub(nm, 1, 10) == "ucbeltmag_" or string.sub(nm, 1, 10) == "ucbeltammo" then
                local on = false
                pcall(function() on = c:IsEnabled() end)
                if on and string.sub(nm, -4) ~= "_lit" then out[#out + 1] = nm end
            end
        end
        table.sort(out)
        return table.concat(out, " ")
    end,
    -- The magazine key of the weapon in hand, but only while this belt actually carries a pouch for
    -- it. nil means "conjure one the old way": a weapon we never built a magazine for must not become
    -- unreloadable.
    magKey = function()
        if S.magHost == nil then return nil end
        return string.match(S.magHost, "^ucbeltmag_(.+)$")
    end,
    -- Palm to that pouch, in metres. nil when there is no pouch or no hand.
    magDist = function(side)
        return S.magD[side]
    end,
    -- The radius the belt itself lights the pouch at, so the reach and the mark agree.
    magRadius = function()
        return reachRadius()
    end,
    -- THE LOOSE SHELLS, for a pump gun: palm to the nearest one of the row, nil when the row is not on
    -- the belt or there is no hand; the same light-up radius; and "I am carrying one" -- the nearest
    -- shell of that hand draws empty until the reload says it went into the gun or was dropped.
    shellDist = function(side)
        return S.shellD[side]
    end,
    shellRadius = function()
        return reachRadius()
    end,
    shellTaken = function(on, side)
        if on then
            if S.shellTaken == nil then S.shellTaken = S.shellNear[side or 0] end
        else
            S.shellTaken = nil
        end
    end,
    -- The reload owns that magazine now: the pouch is empty until it says otherwise. Called every
    -- frame with the current truth rather than on transitions, so a missed edge cannot leave the belt
    -- permanently bare.
    magTaken = function(on)
        on = on and true or false
        if on == S.magTaken then return end
        S.magTaken = on
        if not on then return end
        -- UNMARK FIRST, HIDE AFTER, and that order is the whole of this. A magazine has no `_lit`
        -- twin -- it is marked by swapping its OWN appearance -- so the restore has to be written to a
        -- component that is still attached. Left to the next scan it would run a frame later, on a
        -- component this module has by then disabled, where an appearance write does not reach the
        -- renderer: put the magazine back and the pouch returned still wearing the mark.
        --
        -- The grenade had the same shape and the same fix (`propUnmark`): a thing that leaves the belt
        -- gives its highlight back at the moment it leaves, not when something notices it is gone.
        local host = S.magHost
        if host == nil then return end
        for side = 0, 1 do
            if S.litHand[side] == host then S.litHand[side] = nil end
        end
        if S.litOn[host] then
            markSet(host, false)
            S.litOn[host] = nil
            S.lit = nil
        end
    end,
}
