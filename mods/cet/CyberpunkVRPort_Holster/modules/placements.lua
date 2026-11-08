-- WHERE A HOLSTERED WEAPON HANGS. Each place is a slot of the player's own (a bone the engine carries the weapon's
-- copy with, VRAttachToSlot) and the copy's pose in that bone's frame: the weapon's origin -- the point the hand
-- holds it by, the grip of a gun and the handle of a blade -- and its axes.
--
-- THE BONES' FRAMES (the rig's bind matrices, and measured live 2026-11-03 on the H10 save): Hips (`slots` / hips)
-- and Spine3 (`ItemAttachmentSlots` / Chest) both have +x up along the spine (the hips' tilted 14 degrees back),
-- +y toward the back and +z to the body's right. Places are written in those bones' own axes, named in body words
-- -- U up, F forward, R right -- and turned into x, y, z here.
--
-- A WEAPON'S OWN AXES (its meshes, the frame the hand slot holds it in) differ by family:
--   guns:  +Y along the barrel, +Z the top (the sights), the origin at the grip;
--   melee: +Z along the blade or the shaft to the head, +Y the edge (a katana, a machete, a knife, an axe's bit),
--          the origin where the hand closes on the handle.
-- So a place gives, for each family it takes, `dir` -- where the barrel or the blade points from the hand -- and
-- `face` -- where the gun's top or the blade's edge looks.
--
-- THE BLADE AT THE LEFT HIP and THE BLADE ON THE BACK are the poses of the katanas of "Equippable Katana" (hip and
-- back), read from their meshes: skinned in the bind pose to Hips and to a right scapula joint, carried into the
-- Hips and Spine3 frames by the meshes' bind matrices -- the handle forward and up at the hip; over the right
-- shoulder, the blade down across the back to the left hip, the edge up toward the neck. The mod wears the hip blade
-- edge up; here it hangs as the hand holds it (see hip_l below).
--
-- `pull` (a blade at the side): the length the hand draws it out along its handle before it leaves the sheath and
-- comes into the hand.
local P = {}

local function v(u, f, r) return { u, -f, r } end           -- body words (up, forward, right) -> bone (x, y, z)

local function norm(a)
  local l = math.sqrt(a[1] * a[1] + a[2] * a[2] + a[3] * a[3])
  return { a[1] / l, a[2] / l, a[3] / l }
end

local function cross(a, b)
  return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] }
end

-- The rotation whose columns are the weapon's X, Y and Z in the bone's frame, as a quaternion { i, j, k, r }; `y`
-- is kept exactly, `z` made square to it.
local function quat(y, z)
  y = norm(y)
  local x = norm(cross(y, z))
  z = cross(x, y)
  local m00, m01, m02 = x[1], y[1], z[1]
  local m10, m11, m12 = x[2], y[2], z[2]
  local m20, m21, m22 = x[3], y[3], z[3]
  local tr = m00 + m11 + m22
  if tr > 0 then
    local s = math.sqrt(tr + 1.0) * 2
    return { (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s }
  elseif m00 > m11 and m00 > m22 then
    local s = math.sqrt(1.0 + m00 - m11 - m22) * 2
    return { 0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s }
  elseif m11 > m22 then
    local s = math.sqrt(1.0 + m11 - m00 - m22) * 2
    return { (m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s }
  else
    local s = math.sqrt(1.0 + m22 - m00 - m11) * 2
    return { (m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s }
  end
end
P.quat = quat

local HIPS = { comp = "slots", slot = "hips" }
local CHEST = { comp = "ItemAttachmentSlots", slot = "Chest" }

-- Every place for a right-handed player (the drawing hand the right one); turned over for the left. Each family a
-- place takes: { pos, dir, face } in body words.
local RIGHT = {
  -- a gun at the right hip, the grip on the belt -- muzzle down, its top forward so the grip stands back for the right
  -- hand, flat on the side; drawn up out of the holster by `pull`, grip first. Raised twice: it hung on the thigh,
  -- then just under the belt was still out of reach for players (asked 2026-11-04: "on the belt or a bit higher"),
  -- then 11 cm higher and 3 cm forward with a wider reach, and again 5 cm higher and 4 cm forward (asked 2026-11-05:
  -- "higher still and a little forward")
  -- (2 cm further out than first set: it went into the clothes, asked 2026-11-03)
  -- (the reach 3 cm shorter than 0.18: it drew when the hand only passed by, asked 2026-11-05)
  hip_r = { at = HIPS, reach = 0.15, pull = 0.13,
    gun = { pos = { 0.17, 0.06, 0.23 }, dir = { -1, 0.10, 0 }, face = { 0, 1, 0 } } },
  -- the left hip, the side the right hand draws across to: a blade with its handle forward and up, the blade down
  -- and back; drawn out `pull` along the handle before it leaves
  -- (3.2 cm further out from the hip than the mod's pose: it went into the clothes, asked 2026-11-03, and a blade lying
  -- flat comes nearer: 1.8 cm off the bare body for a machete, as it was on edge)
  -- ITS EDGE LOOKS OUT, AWAY FROM THE LEG, BECAUSE THAT IS HOW THE HAND HOLDS IT. In the hand the game has a blade's
  -- edge where the fingers point (P.HOLD): held out in front, forward at the enemy. So the hand closing over the handle
  -- from above -- palm down, the fingers out over the far side -- has the edge pointing out. Hung like that the blade
  -- never turns: not in the sheath, not when it leaves, not going back. Hung edge down it was a quarter turn off that
  -- hold, edge up a half turn with the wrist bent back, and the difference had to be made up somewhere: the blade spun
  -- about its own axis on the way out (2026-11-05: "held from below, and it turns about its axis").
  -- (the reach 3 cm shorter than 0.16, as at the right hip)
  hip_l = { at = HIPS, reach = 0.13, pull = 0.22,
    melee = { pos = { 0.105, 0.147, -0.178 }, dir = { -0.293, -0.944, -0.150 }, face = { -0.095, 0.185, -0.977 } } },
  -- a gun on a strap under the left arm, level, muzzle back, the grip down and forward for a cross draw, its side
  -- flat on the ribs; drawn forward out of the holster by `pull`. 3 cm further to the side and 1 cm back round the
  -- chest (asked 2026-11-05: it was in the way)
  underarm_l = { at = CHEST, reach = 0.14, pull = 0.12,
    gun = { pos = { -0.10, 0.11, -0.20 }, dir = { -0.10, -1, 0 }, face = { 1, 0, 0 } } },
  -- a knife on the chest under the left arm, handle up, blade down, its flat on the chest and the edge out toward
  -- the armpit; the chest there faces forward and out (F 0.81, R -0.59 on the base body), so the edge looks along
  -- it, back and out, or the flat stands 36 degrees off the chest and one edge sinks 3 cm in
  chest_l = { at = CHEST, reach = 0.12,
    melee = { pos = { -0.06, 0.179, -0.134 }, dir = { -1, 0, 0.20 }, face = { 0, -0.59, -0.81 } } },
  -- the back over the right shoulder: the blade (the back katana) or the barrel down across the back to the left
  -- hip, lying flat on the back -- a blade's edge and a gun's top up toward the neck, so a magazine hangs down.
  -- REACHED OVER THE SHOULDER, along the handle or the stock (`grab`, per family: metres out from the origin along
  -- the handle, the way it is drawn): the origin, where the hand closes, is low on the back -- a gun's grip 10 cm
  -- above Spine3 -- and a hand over the shoulder meets the stock or the pommel 20-30 cm above it, out of a reach
  -- round the origin alone (2026-11-05: "nothing can be taken from behind the shoulder")
  shoulder_r = { at = CHEST, reach = 0.18, grab = { gun = 0.32, melee = 0.22 },
    melee = { pos = { 0.175, -0.12, 0.16 }, dir = { -0.760, 0.053, -0.648 }, face = { 0.649, 0.008, -0.761 } },
    -- the grip 12.5 cm behind Spine3: the gun's side 1.5-2 cm off the bare back (17 cm left 6 cm of air)
    gun = { pos = { 0.10, -0.125, 0.12 }, dir = { -0.90, 0, -0.43 }, face = { 0.43, 0, -0.90 } } },
  -- across the lower back, the grip at the right, the barrel or the blade to the left, flat on the back: a gun's top
  -- up, a blade's edge down
  lower_back = { at = HIPS, reach = 0.15,
    gun = { pos = { 0.04, -0.15, 0.14 }, dir = { 0, 0, -1 }, face = { 1, 0, 0 } },
    melee = { pos = { 0.04, -0.15, 0.14 }, dir = { 0, 0, -1 }, face = { -1, 0, 0 } } },
  -- a knife low on the right thigh, blade down, flat on the thigh, the edge forward
  thigh_r = { at = HIPS, reach = 0.12,
    melee = { pos = { -0.24, 0.047, 0.19 }, dir = { -1, 0, 0 }, face = { 0, 1, 0 } } },
}
-- the left shoulder is the right one turned over
RIGHT.shoulder_l = { at = CHEST, reach = 0.18, grab = RIGHT.shoulder_r.grab, mirrorOf = "shoulder_r" }

-- THE HOLD OF A WEAPON IN THE HAND: the hand slot's pose in the wrist's frame with that weapon really there, right hand
-- (measured live 2026-11-05, constant while the hand moves). It is NOT the empty hand's (p -0.088 0.042 0.022, q -0.471
-- 0.514 0.508 0.506): the game's animation carries the slot, a katana 34.5 degrees and 2.7 cm from there. The left hand's
-- are these mirrored (measured: p -> -p, { i, j, k, r } -> { r, k, -j, -i }). A weapon of another type is learned the
-- first time it is in the hand (modules/holsters.lua); until then it takes its family's.
local HOLD_CLUB = { p = { -0.0601, 0.0317, 0.0250 }, q = { -0.5199, 0.5508, 0.5681, 0.3219 } }
local HOLD_PISTOL = { p = { -0.0748, 0.0321, 0.0136 }, q = { -0.5061, 0.5157, 0.5756, 0.3829 } }
local HOLD = {
  Wea_Katana = { p = { -0.0647, 0.0320, 0.0294 }, q = { -0.6178, 0.3754, 0.6161, 0.3128 } },
  Wea_Machete = HOLD_CLUB, Wea_OneHandedClub = HOLD_CLUB,
  Wea_Handgun = HOLD_PISTOL,
  Wea_Revolver = { p = { -0.0761, 0.0468, 0.0113 }, q = { -0.4743, 0.5834, 0.4813, 0.4506 } },
}
local HOLD_EMPTY = { p = { -0.0881, 0.0418, 0.0217 }, q = { -0.4709, 0.5140, 0.5078, 0.5061 } }
local function side(h, left)
  if not left then return h end
  local p, q = h.p, h.q
  return { p = { -p[1], -p[2], -p[3] }, q = { q[4], q[3], -q[2], -q[1] } }
end
function P.hold(itemTypeName, melee, left)
  return side(HOLD[itemTypeName] or (melee and HOLD_CLUB or HOLD_PISTOL), left)
end
-- The rig's own pose of the slot, the frame the plugin's grasp takes a thing in.
function P.holdEmpty(left) return side(HOLD_EMPTY, left) end

-- The order each kind of weapon takes its places in: the first one free is its own. The first is the "main" one --
-- with three of a kind, the weapon in the first weapon slot is there. Pistols never go on the back or the shoulders
-- (asked 2026-11-03): a third one is not carried on the body.
P.ORDER = {
  pistol = { "hip_r", "underarm_l" },
  blade  = { "hip_l", "shoulder_r", "shoulder_l" },
  long   = { "shoulder_r", "shoulder_l", "lower_back" },
  knife  = { "chest_l", "thigh_r", "lower_back" },
}

-- THE PLACES A PLAYER MAY CHOOSE for each kind (the VR menu's inventory, modules/holsters.lua H.setPlace); the first of
-- P.ORDER stays the automatic one. A pistol on the right thigh hangs as the knife there does -- the barrel down, its
-- top forward, the grip up at the side of the thigh: a drop-leg holster.
P.CHOICES = {
  pistol = { "hip_r", "thigh_r", "underarm_l" },
  blade  = { "hip_l", "shoulder_r", "shoulder_l", "lower_back" },
  long   = { "shoulder_r", "shoulder_l", "lower_back" },
  knife  = { "chest_l", "thigh_r", "lower_back" },
}
-- The family a kind is drawn as in the menu's icons (tools/vr_lobby_assets/make_holster_icons.py).
P.FAMILY = { pistol = "gun", blade = "blade", long = "long", knife = "knife" }

local MIRROR_NAME = { hip_r = "hip_l", hip_l = "hip_r", underarm_l = "underarm_r", chest_l = "chest_r",
  shoulder_r = "shoulder_l", shoulder_l = "shoulder_r", thigh_r = "thigh_l", lower_back = "lower_back" }
P.MIRROR_NAME = MIRROR_NAME

-- One family's pose: the place's own one, or the other family's (a gun's barrel where a blade points, its top where
-- the edge looks).
local function pose(def, melee)
  if def.mirrorOf then
    local src = pose(RIGHT[def.mirrorOf], melee)
    return { pos = { src.pos[1], src.pos[2], -src.pos[3] }, dir = { src.dir[1], src.dir[2], -src.dir[3] },
             face = { src.face[1], src.face[2], -src.face[3] } }
  end
  if melee then return def.melee or def.gun end
  return def.gun or def.melee
end

local function build(def, mirror, melee)
  local s = mirror and -1 or 1
  local p = pose(def, melee)
  local pos = v(p.pos[1], p.pos[2], p.pos[3] * s)
  local dir = v(p.dir[1], p.dir[2], p.dir[3] * s)
  local face = v(p.face[1], p.face[2], p.face[3] * s)
  -- a gun: Y = dir, Z = face; a melee weapon: Z = dir kept exactly, Y = face made square to it
  local q
  local nd = norm(dir)
  if melee then
    q = quat(cross(cross(nd, face), nd), nd)
  else
    q = quat(dir, face)
  end
  return { comp = def.at.comp, slot = def.at.slot, pos = pos, rot = q, reach = def.reach, pull = def.pull,
           pullAxis = def.pull and { -nd[1], -nd[2], -nd[3] } or nil,                 -- out along the handle
           pullLocal = melee and { 0, 0, -1 } or { 0, -1, 0 }, melee = melee,
           grab = def.grab and (melee and def.grab.melee or def.grab.gun) or nil }    -- along pullLocal from the origin
end

-- The place `name` (a right-handed name) as this player has it, for a melee weapon or a gun: turned over to the
-- other side when the drawing hand is the left one.
local cache = {}
function P.get(name, left, melee)
  left, melee = left == true, melee == true
  local ck = name .. (left and "|L" or "|R") .. (melee and "|m" or "|g")
  local c = cache[ck]
  if c then return c end
  local def = RIGHT[name]
  if def == nil then return nil end
  c = build(def, left, melee)
  c.name = left and (MIRROR_NAME[name] or name) or name
  cache[ck] = c
  return c
end

-- Which list a weapon of this item type takes its place from; nil for what is not carried on the body (fists, arm
-- cyberware, grenades). And whether it is a melee weapon (its own axes).
local KIND = {
  Wea_Handgun = "pistol", Wea_Revolver = "pistol",
  Wea_Katana = "blade", Wea_Sword = "blade", Wea_LongBlade = "blade", Wea_Machete = "blade", Wea_Axe = "blade",
  Wea_OneHandedClub = "blade", Wea_Chainsword = "blade",
  Wea_Knife = "knife", Wea_ShortBlade = "knife",
  Wea_AssaultRifle = "long", Wea_Rifle = "long", Wea_PrecisionRifle = "long", Wea_SniperRifle = "long",
  Wea_Shotgun = "long", Wea_ShotgunDual = "long", Wea_SubmachineGun = "long", Wea_LightMachineGun = "long",
  Wea_HeavyMachineGun = "long", Wea_TwoHandedClub = "long", Wea_Hammer = "long",
}
local MELEE = { Wea_Katana = true, Wea_Sword = true, Wea_LongBlade = true, Wea_Machete = true, Wea_Axe = true,
  Wea_OneHandedClub = true, Wea_Chainsword = true, Wea_Knife = true, Wea_ShortBlade = true, Wea_TwoHandedClub = true,
  Wea_Hammer = true }
function P.kind(itemTypeName) return KIND[itemTypeName] end
function P.melee(itemTypeName) return MELEE[itemTypeName] == true end

return P
