-- THE HOLSTERS: every weapon in the three weapon slots that is not in a hand hangs on the body, where the hand
-- reaches for it.
--
-- WHAT HANGS. A copy of the weapon -- vrport\holster\vrp_holster_weapon.ent shown in that weapon's own appearance
-- (tools/holsters/make_holster_weapon.py: "<factory key>__<appearance>", see ITS LOOK) -- spawned once per equipped
-- weapon and made a child of a bone slot (VRAttachToSlot), so the engine carries it with the body in the same pass as
-- the bones.
--
-- WHERE (modules/placements.lua): each kind of weapon has its places in order and takes the first free one --
-- pistols the right hip (at the belt), then under the left arm; blades and clubs the left hip, then the right and
-- the left shoulder; rifles, shotguns and the like the right shoulder, the left one, the lower back; knives the
-- chest under the left arm, the right thigh, the lower back. With several of a kind the one in the first weapon slot
-- has the first place. The left-handed controls turn every place over to the other side.
--
-- THE PAIR (dual wielding: the Cool perk Cool_Central_Milestone_2 and a handgun in weapon slot 3, the left-hand partner
-- of slot 0's) hangs on both hips before anything else -- over the places chosen in the menu -- the right hand's gun at
-- the right hip, the left hand's at the left one (the right hip's holster turned over), and every other weapon takes
-- the places that are left: a blade from the left hip goes over the shoulder, a second pistol under the arm. Each hand
-- takes and puts back its own gun of the pair; the game draws and holsters the two together, so the other gun stays
-- in its holster until the game has it in its hand -- or comes with its own hand when that hand takes it too.
--
-- IN HAND, GONE. The copy is hidden once the weapon itself is in the hand, and shown again once that hand has let
-- it go.
--
-- ITS LOOK. The one the game gives that very item, worked out the way the engine does when it builds the weapon
-- (sub_140254BE8, game 2.5, every spawn path of a weapon): among the appearances of its .app that carry ALL of the record's
-- visual tags (all of them when the record has none) the item's seed picks one; none carry them: `default`.
--
-- TAKING ONE. The drawing hand (the right one, the left one with the left-handed controls) closes its grip near a
-- copy's grip point:
--  - a place with `pull` (a pistol's holster, a blade's sheath): THE HAND TAKES HOLD OF IT WHERE IT IS. The wrist is
--    held exactly where it holds that weapon in the hand -- the copy's pose times the inverse of the weapon's hold
--    (P.hold: the slot's pose in the wrist's frame with that weapon really in the hand, not the empty hand's) -- and the
--    fingers close round the grip (the plugin's grasp, a bar there). The weapon stays in its holster until the hand draws
--    it out along it: the copy slides out with the hand, the hand stays on it, and once it is out by `pull` it rides in
--    the hand, in that same hold, until the weapon itself is there, in the copy's place, so nothing moves in the hand.
--    (The plugin's grasp would hold the slot at the rig's own pose and the weapon would come in 10-35 degrees off its
--    hold, seen beside the copy: this grasp leaves the slot to the game's animation, VRGraspPin.) Let go
--    before that and it slides back and the hand is free again. The wrist goes back to its controller gradually
--    (RELEASE_TIME), the weapon with it, so the weapon does not jump in the hand as it comes out;
--  - a place without (a knife on the chest): the weapon comes into the hand at once.
-- PUTTING IT AWAY. With a weapon in hand the same grip near its own place puts it away: the copy takes the weapon's
-- place in the hand and goes home by itself -- over the holster's mouth, then down into it -- while the game takes the
-- weapon out of the hand.
--
-- THE HOLSTERS THEMSELVES (modules/shells.lua): the port's own leg holster at the drawing hip, the shoulder harness
-- with the holster under the off arm and the knife sheath on the chest (copies of the game's gang pieces) hang round the
-- copies -- each a mesh carried by the same bone -- and stay there, empty, while the weapon is in the hand.
--
-- THE BELT'S GRENADES (CyberpunkVRPort_TacticalBelt) sit at the front of the belt on the drawing side. The belt asks
-- `H.near` before it lights one for a hand: a holstered weapon nearer to that palm than the grenade is what the
-- hand is reaching for, and the grenade stays dark (so the grip is the holster's).
local P = require("modules/placements")
local SH = require("modules/shells")
local LOOKS = require("data/appearances")

local H = {}

local ENT = "vrport\\holster\\vrp_holster_weapon.ent"
local SCAN_EVERY = 0.25           -- seconds between reading the weapon slots
local COOLDOWN = 0.6              -- seconds after a draw or a holster before the next one
local STOW_TIME = 0.35            -- seconds the copy takes from the hand into its holster
local IN_HAND_WAIT = 3.0          -- seconds the copy waits in the hand for the weapon itself
local RELEASE_TIME = 0.25         -- seconds the wrist takes from a hold back to its controller
local HOLD_REACHED = math.rad(3)  -- the game's animation has the slot in the weapon's hold within this
local HOLD_WAIT = 0.25            -- seconds a weapon arrived in the hand stays unseen waiting for that (only a wrong
                                  -- hold of a type not measured waits: the slot is there as the weapon arrives)
local BUILD_TIME = 1.0            -- seconds a new copy waits under the floor, hidden, once its appearance has loaded
local BUILD_LIMIT = 15.0          -- seconds it waits for that at most (an appearance that never loads hangs empty)
local SEAT_DELAY = 0.3            -- seconds it stays out of sight on its bone, hidden, before it takes its place
local OUT_OF_SIGHT = { -10, 0, 0 }   -- that pose in the bone's frame: 10 m under it (the bones' +x is up)
local LEARN_AFTER = 0.7           -- seconds a weapon has been in the hand before its hold is read
local WRIST = { [0] = 23, [1] = 24 }
local SLOT_R, SLOT_L = nil, nil

local S = {
  t = 0, scanAt = -1, cooldown = 0,
  copies = {},        -- by item key: { id, key, look, place, entity, attached, shown, item, mode, ... }
  shells = {},        -- by name (modules/shells.lua): { id, ent, comp, slot, pose, entity, attached }
  pull = nil,         -- { key, side, h0, s, rel }
  grasp = { [0] = 0, [1] = 0 }, graspWant = { [0] = 0, [1] = 0 },
  magnet = nil,       -- the hand held by a pull
  held = nil,         -- the wrist's last hold: { p, q } in model space
  release = nil,      -- the wrist going back to its controller: { side, p, q, at }
  holds = {},         -- item key -> the hold read with that weapon in the hand: { p, q }
  inHandAt = {},      -- item key -> since when that weapon has been in the hand
  holdRead = {},      -- item key -> the last reading of its hold (it must be steady to count)
  gripPrev = { [0] = false, [1] = false },
  looks = {},         -- item key -> the copy's appearance
  hiddenReal = {},    -- item key -> { id, names }: the weapon itself made unseen while its copy went home (stow)
  scene = false,      -- a scene plays the body this tick (sceneNow)
}

local function key(id) return tostring(ItemID.GetCombinedHash(id)) end

-- OFF THE BODY IN A SCENE (2026-11-06: "hide the holsters and the belt in cutscenes"). While a scene plays the
-- body -- the tier that suspends VRIK, the plugin's VRSceneOwnsBody, the same signal the hands-only mode and the
-- belt go by -- the copies and the holster shells are out of sight and no hand draws from them; they stay on their
-- bones and are seen again the tick the scene lets go.
-- IN A VEHICLE TOO (2026-11-06: "in the car all the holsters and the belt off, the same as in scenes"): a
-- seated body has nothing to draw from at the hips, and the copies only clipped through the seat. Mounted is asked
-- four times a second, not every tick.
local function askScene() return Game.VRSceneOwnsBody() end
local mountedAt, mounted = -1, false
local function inVehicle(now)
  if now - mountedAt >= 0.25 or now < mountedAt then
    mountedAt = now
    local ok, v = pcall(function() local pl = Game.GetPlayer(); return pl ~= nil and Game.GetMountedVehicle(pl) ~= nil end)
    mounted = ok and v == true
  end
  return mounted
end
local function sceneNow(now)
  local ok, v = pcall(askScene)
  return (ok and v == true) or inVehicle(now or 0)
end

local function drawLeft() return type(VRLeftHanded) == "function" and VRLeftHanded() == true end

---------------------------------------------------------------- the player's choices (the VR menu's inventory)

-- WHERE EACH WEAPON SLOT'S WEAPON HANGS, AND WHETHER THE HOLSTERED WEAPONS ARE SEEN (2026-11-05: "in the
-- inventory, let the slots change where the holsters are ... a switch to hide all the holsters"). Per weapon slot
-- (1..3): "auto" (the first free place of its kind, P.ORDER), a place of P.CHOICES for its kind (right-handed names,
-- turned over for the left hand as every place is), or "none" (not carried on the body). Hidden: the copies on the
-- body are not drawn -- they are still there to reach for, and a drawn weapon is seen from the moment it leaves.
-- Kept in the mod's folder, apart from saves.
local PREFS_FILE = "holster_places.json"
local PREFS = { slots = {}, hidden = false }

local function loadPrefs()
  local f = io.open(PREFS_FILE, "r")
  if not f then return end
  local text = f:read("*a")
  f:close()
  local ok, data = pcall(function() return json.decode(text) end)
  if not ok or type(data) ~= "table" then return end
  if type(data.slots) == "table" then
    for k, v in pairs(data.slots) do
      local i = tonumber(k)
      if i ~= nil and i >= 1 and i <= 3 and type(v) == "string" then PREFS.slots[i] = v end
    end
  end
  PREFS.hidden = data.hidden == true
end

local function savePrefs()
  local slots = {}
  for i = 1, 3 do slots[tostring(i)] = PREFS.slots[i] or "auto" end
  local ok, text = pcall(function() return json.encode({ slots = slots, hidden = PREFS.hidden }) end)
  if not ok then return false end
  local f = io.open(PREFS_FILE, "w")
  if not f then return false end
  f:write(text)
  f:close()
  return true
end

pcall(loadPrefs)

-- The place chosen for a weapon slot's weapon of `kind`: a place name, "none", or nil for the automatic one (a choice
-- its kind cannot take -- the slot now holds a weapon of another kind -- is automatic too).
local function chosen(slot, kind)
  local c = PREFS.slots[slot + 1]
  if c == nil or c == "auto" then return nil end
  if c == "none" then return "none" end
  for _, n in ipairs(P.CHOICES[kind] or {}) do
    if n == c then return c end
  end
  return nil
end

-- Names and slot ids made once: the frame asks for them every tick.
local EQUIPMENT_SYSTEM = nil
local function playerData(pl)
  EQUIPMENT_SYSTEM = EQUIPMENT_SYSTEM or CName.new("EquipmentSystem")
  local es = Game.GetScriptableSystemsContainer():Get(EQUIPMENT_SYSTEM)
  return es, es:GetPlayerData(pl)
end

---------------------------------------------------------------- rotations ({ i, j, k, r })

local function qmul(a, b)
  return { a[4] * b[1] + a[1] * b[4] + a[2] * b[3] - a[3] * b[2],
           a[4] * b[2] - a[1] * b[3] + a[2] * b[4] + a[3] * b[1],
           a[4] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[4],
           a[4] * b[4] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3] }
end
local function qconj(a) return { -a[1], -a[2], -a[3], a[4] } end
local function qrot(q, v)
  local r = qmul(qmul(q, { v[1], v[2], v[3], 0 }), qconj(q))
  return { r[1], r[2], r[3] }
end
local function qnorm(q)
  local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3] + q[4] * q[4])
  if l < 1e-8 then return { 0, 0, 0, 1 } end
  return { q[1] / l, q[2] / l, q[3] / l, q[4] / l }
end
local function qangle(a, b)
  local d = math.abs(a[1] * b[1] + a[2] * b[2] + a[3] * b[3] + a[4] * b[4])
  return 2 * math.acos(math.min(1, d))
end
local function qslerp(a, b, t)
  local d = a[1] * b[1] + a[2] * b[2] + a[3] * b[3] + a[4] * b[4]
  if d < 0 then b = { -b[1], -b[2], -b[3], -b[4] }; d = -d end
  if d > 0.9995 then
    return qnorm({ a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t, a[4] + (b[4] - a[4]) * t })
  end
  local th = math.acos(d)
  local s, sa, sb = math.sin(th), math.sin((1 - t) * th), math.sin(t * th)
  return { (a[1] * sa + b[1] * sb) / s, (a[2] * sa + b[2] * sb) / s, (a[3] * sa + b[3] * sb) / s, (a[4] * sa + b[4] * sb) / s }
end
local function vnorm(a)
  local l = math.sqrt(a[1] * a[1] + a[2] * a[2] + a[3] * a[3])
  return { a[1] / l, a[2] / l, a[3] / l }
end
local function lerp3(a, b, t) return { a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t } end

-- The rotation whose columns are x, y, z.
local function fromAxes(x, y, z)
  local m00, m01, m02 = x[1], y[1], z[1]
  local m10, m11, m12 = x[2], y[2], z[2]
  local m20, m21, m22 = x[3], y[3], z[3]
  local tr = m00 + m11 + m22
  if tr > 0 then
    local s = math.sqrt(tr + 1) * 2
    return { (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s }
  elseif m00 > m11 and m00 > m22 then
    local s = math.sqrt(1 + m00 - m11 - m22) * 2
    return { 0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s }
  elseif m11 > m22 then
    local s = math.sqrt(1 + m11 - m00 - m22) * 2
    return { (m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s }
  else
    local s = math.sqrt(1 + m22 - m00 - m11) * 2
    return { (m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s }
  end
end

-- A world rotation in the player's model space (the space the wrist natives work in): its axes carried over.
local function modelQ(q)
  local ax = {}
  for i, v in ipairs({ { 1, 0, 0 }, { 0, 1, 0 }, { 0, 0, 1 } }) do
    local w = qrot(q, v)
    local m = VRWorldDirToModel(Vector4.new(w[1], w[2], w[3], 0))
    ax[i] = vnorm({ m.x, m.y, m.z })
  end
  return qnorm(fromAxes(ax[1], ax[2], ax[3]))
end

---------------------------------------------------------------- the body's frames

-- A slot's pose in the world: position { x, y, z } and rotation, or nil.
local function slotWorld(pl, compName, slotName)
  local comp = pl:FindComponentByName(CName.new(compName))
  if comp == nil then return nil end
  local ok, done, wt = pcall(function() return comp:GetSlotTransform(CName.new(slotName)) end)
  if not ok or not done or wt == nil then return nil end
  local p = WorldPosition.ToVector4(wt.Position)
  local o = wt.Orientation
  return { p.x, p.y, p.z }, qnorm({ o.i, o.j, o.k, o.r })
end

local function handSlotName(side) return side == 0 and "WeaponLeft" or "WeaponRight" end

-- The hand slot's pose in the wrist's frame (constant for the rig: measured on the H10 save as
-- p (-0.088, 0.042, 0.022), q (-0.471, 0.514, 0.508, 0.506) for the right hand at three different hand poses),
-- read when a grip closes so it is the rig the player has. `slotName`: another slot of the same component.
local function slotInWrist(pl, side, slotName)
  local sp, sq = slotWorld(pl, "ItemAttachmentSlots", slotName or handSlotName(side))
  if sp == nil then return nil end
  local ok, wp, wq = pcall(function() return VRBoneModelPos(WRIST[side]), VRBoneModelRot(WRIST[side]) end)
  if not ok or wp == nil or wq == nil then return nil end
  local m = VRWorldToModel(Vector4.new(sp[1], sp[2], sp[3], 1))
  local W = qnorm({ wq.i, wq.j, wq.k, wq.r })
  local smq = modelQ(sq)
  return { p = qrot(qconj(W), { m.x - wp.x, m.y - wp.y, m.z - wp.z }), q = qmul(qconj(W), smq) }
end

-- A copy's pose in the world, `s` metres drawn out of its place.
local function copyWorld(pl, c, s)
  local bp, bq = slotWorld(pl, c.place.comp, c.place.slot)
  if bp == nil then return nil end
  local p = c.place
  local lp = { p.pos[1], p.pos[2], p.pos[3] }
  if s and s > 0 and p.pullAxis then
    lp = { lp[1] + p.pullAxis[1] * s, lp[2] + p.pullAxis[2] * s, lp[3] + p.pullAxis[3] * s }
  end
  local wp = qrot(bq, lp)
  return { bp[1] + wp[1], bp[2] + wp[2], bp[3] + wp[3] }, qmul(bq, { p.rot[1], p.rot[2], p.rot[3], p.rot[4] })
end

---------------------------------------------------------------- the wrist and the fingers

-- THE HAND CLAIMED. The hand-collision module releases every wrist nobody claimed in its own pass (shared slot 162,
-- SetVRReloadOwnedHand), so a hold is claimed every frame it lasts.
local function claim(side)
  if type(SetVRReloadOwnedHand) == "function" then pcall(SetVRReloadOwnedHand, side == nil and -1 or side) end
end

-- The wrist where it holds a weapon whose root is at (p, q) in the world.
local function magnetTo(side, rel, p, q)
  local m = VRWorldToModel(Vector4.new(p[1], p[2], p[3], 1))
  local wq = qmul(modelQ(q), qconj(rel.q))
  local off = qrot(wq, rel.p)
  local mp = { m.x - off[1], m.y - off[2], m.z - off[3] }
  claim(side)
  pcall(VRHandStopModel, side, true, Vector4.new(mp[1], mp[2], mp[3], 1))
  pcall(VRHandStopRot, side, 1, wq[1], wq[2], wq[3], wq[4])
  S.magnet, S.held = side, { p = mp, q = wq }
end

local function magnetOff()
  S.release = nil
  if S.magnet == nil then return end
  local side = S.magnet
  S.magnet = nil
  claim(nil)
  pcall(VRHandStopModel, side, false, Vector4.new(0, 0, 0, 0))
  pcall(VRHandStopRot, side, 0, 0, 0, 0, 1)
end

-- THE WRIST BACK TO ITS CONTROLLER, from where the hold last had it. Let go at once, the hand and the weapon in it jump
-- to wherever the controller is by then, turned as the player's hand happens to be turned.
local function releaseWrist(side)
  if S.magnet == nil or S.held == nil then return magnetOff() end
  S.release = { side = side, p = S.held.p, q = S.held.q, at = S.t }
end

local function releaseTick()
  local r = S.release
  if r == nil then return end
  local u = (S.t - r.at) / RELEASE_TIME
  local ok, p, q = pcall(function() return VRHandRawModel(r.side), VRHandRawRot(r.side) end)
  if u >= 1 or not ok or p == nil or q == nil or p.w < 0.5 then return magnetOff() end
  u = u * u * (3 - 2 * u)
  local pos = lerp3(r.p, { p.x, p.y, p.z }, u)
  local rot = qslerp(r.q, qnorm({ q.i, q.j, q.k, q.r }), u)
  claim(r.side)
  pcall(VRHandStopModel, r.side, true, Vector4.new(pos[1], pos[2], pos[3], 1))
  pcall(VRHandStopRot, r.side, 1, rot[1], rot[2], rot[3], rot[4])
end

-- THE FINGERS ROUND THE GRIP: the plugin's grasp (src/Anim/GraspPose.cpp) on a bar in the hand's slot -- a gun's
-- grip down and a little back from the root (+Y the barrel, +Z the top), a blade's handle along its own axis.
-- The grasp is set in the frame of the rig's own slot (`now`), and the weapon is held in `hold`. It leaves the slot to the
-- game's animation.
local GRASP_BOX = 0
local function graspOn(side, melee, hold, now)
  if type(VRGraspSet) ~= "function" then return end
  local lp, lq = { 0, 0, -0.02 }, { 0, 0, 0, 1 }
  local hx, hy, hz = 0.014, 0.014, 0.05
  if not melee then
    -- the grip axis 16 degrees back from straight down
    lp, lq = { 0, -0.012, -0.035 }, { -0.139, 0, 0, 0.990 }
    hx, hy, hz = 0.013, 0.02, 0.045
  end
  if hold ~= nil and now ~= nil then
    local iq = qconj(now.q)
    local r = qrot(hold.q, lp)
    lp = qrot(iq, { hold.p[1] - now.p[1] + r[1], hold.p[2] - now.p[2] + r[2], hold.p[3] - now.p[3] + r[3] })
    lq = qnorm(qmul(qmul(iq, hold.q), lq))
  end
  pcall(VRGraspSet, side, GRASP_BOX, hx, hy, hz, Vector4.new(lp[1], lp[2], lp[3], 1),
        Quaternion.new(lq[1], lq[2], lq[3], lq[4]))
  if type(VRGraspPin) == "function" then pcall(VRGraspPin, side, false) end
  S.graspWant[side] = 1
end
local function graspOff(side) S.graspWant[side] = 0 end

local function graspTick(dt)
  if type(VRGraspBlend) ~= "function" then return end
  for side = 0, 1 do
    local b, w = S.grasp[side], S.graspWant[side]
    if b ~= w then
      local step = dt / (w > b and 0.10 or 0.18)
      b = w > b and math.min(w, b + step) or math.max(w, b - step)
      S.grasp[side] = b
      pcall(VRGraspBlend, side, b)
      if b <= 0 then pcall(VRGraspClear, side) end
    end
  end
end

---------------------------------------------------------------- what is in the hands

-- The items the hands hold (the weapon objects in their slots), and the ones the equipment system is putting there.
-- Every tick: plain functions for pcall and the slot lists made once, not a closure and a table per call.
local SLOTS, SIDES = nil, nil

local function activeItems(pd, out)
  for i = 1, #SIDES do
    local id = pd:GetSlotActiveItem(SIDES[i])
    if ItemID.IsValid(id) then out[key(id)] = true end
  end
end
local function inHands(pl, pd)
  local out, real = {}, {}
  SLOT_R = SLOT_R or TweakDBID.new("AttachmentSlots.WeaponRight")
  SLOT_L = SLOT_L or TweakDBID.new("AttachmentSlots.WeaponLeft")
  SLOTS = SLOTS or { SLOT_R, SLOT_L }
  SIDES = SIDES or { EquipmentManipulationRequestSlot.Right, EquipmentManipulationRequestSlot.Left }
  local ts = Game.GetTransactionSystem()
  for i = 1, #SLOTS do
    local obj = ts:GetItemInSlot(pl, SLOTS[i])
    if obj ~= nil then
      local k = key(obj:GetItemID())
      out[k], real[k] = true, obj
    end
  end
  pcall(activeItems, pd, out)
  return out, real
end

-- Which of n appearances an item's seed picks: the engine's sum, a PCG output step (sub_140254BE8).
local function seedPick(seed, n)
  local v = 0x5851F42D4C957F2DULL * seed + 1ULL
  local a = tonumber((v / 134217728ULL) % 4294967296ULL)   -- v >> 27, its low 32 bits
  local b = tonumber(v / 35184372088832ULL)                -- v >> 45
  local r = tonumber(v / 576460752303423488ULL)            -- v >> 59
  local u = bit32.bxor(a, b)
  return (bit32.bor(bit32.rshift(u, r), bit32.lshift(u, 31 - r)) % 4294967296) % n
end

-- The copy's appearance: see ITS LOOK at the top. A record's tags and an item's seed do not change: worked out once.
local function lookFor(id, k)
  local look = S.looks[k]
  if look ~= nil then return look end
  local rec = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(id))
  if rec == nil then return nil end
  local res = NameToString(TweakDB:GetFlat(TDBID.ToStringDEBUG(ItemID.GetTDBID(id)) .. ".appearanceResourceName"))
  local rows = LOOKS[res]
  if rows == nil then return nil end
  local tags = {}
  for _, t in ipairs(rec:VisualTags()) do tags[#tags + 1] = NameToString(t) end
  local fit = {}
  for _, r in ipairs(rows) do
    local has = {}
    for _, t in ipairs(r[2]) do has[t] = true end
    local all = true
    for _, t in ipairs(tags) do if not has[t] then all = false break end end
    if all then fit[#fit + 1] = r[1] end
  end
  local name = "default"
  if #fit > 0 then name = fit[seedPick(ItemID.GetRngSeed(id), #fit) + 1] end
  look = res .. "__" .. name
  S.looks[k] = look
  return look
end

local function itemTypeName(id)
  local name = nil
  pcall(function()
    local rec = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(id))
    name = rec:ItemType():Type().value
  end)
  if name == nil then
    pcall(function() name = tostring(TweakDBInterface.GetItemRecord(ItemID.GetTDBID(id)):ItemType():Type()) end)
    if name then name = name:match("(Wea_%w+)") end
  end
  return name
end

---------------------------------------------------------------- the copies

local function despawn(c)
  if c == nil then return end
  pcall(function()
    local e = Game.FindEntityByID(c.id)
    if e ~= nil then
      pcall(VRDetachFromSlot, e)
      exEntitySpawner.Despawn(e)
    end
  end)
end

-- Returns how many it has: none until the entity's appearance has loaded.
local function showEntity(e, on)
  local n = 0
  pcall(function()
    for _, comp in ipairs(Game.VREntityComponents(e)) do
      if comp:IsA(CName.new("entIVisualComponent")) or comp:IsA(CName.new("entLightComponent")) then
        comp:Toggle(on)
        n = n + 1
      end
    end
  end)
  return n
end

-- The weapon itself out of sight, remembered by its entity and the components that were on: only those come back on,
-- the ones the game keeps off (a sight's housing, an attachment not fitted) stay off. Called every tick it is to stay
-- hidden: a weapon still being built gets components switched on after this has switched them off (3 of 7 back within
-- 0.13 s, measured), and those were seen beside the copy at another angle (2026-11-05: "two copies at
-- different angles for a moment").
local function hideReal(k, obj)
  local ok, id = pcall(function() return obj:GetEntityID().hash end)
  id = ok and id or nil
  local rec = S.hiddenReal[k]
  if rec == nil or rec.id ~= id then
    rec = { id = id, names = {} }
    S.hiddenReal[k] = rec
  end
  pcall(function()
    for _, comp in ipairs(Game.VREntityComponents(obj)) do
      if (comp:IsA(CName.new("entIVisualComponent")) or comp:IsA(CName.new("entLightComponent"))) and comp:IsEnabled() then
        rec.names[NameToString(comp:GetName())] = true
        comp:Toggle(false)
      end
    end
  end)
end

local function unhideReal(rec, obj)
  pcall(function()
    if rec.id == nil or obj:GetEntityID().hash ~= rec.id then return end   -- a new object: drawn as the game made it
    for _, comp in ipairs(Game.VREntityComponents(obj)) do
      if rec.names[NameToString(comp:GetName())] then comp:Toggle(true) end
    end
  end)
end

-- An entity's visual and light components that are on, switched off; returns their names for showKept.
local function hideKept(e)
  local names = {}
  pcall(function()
    for _, comp in ipairs(Game.VREntityComponents(e)) do
      if (comp:IsA(CName.new("entIVisualComponent")) or comp:IsA(CName.new("entLightComponent"))) and comp:IsEnabled() then
        names[NameToString(comp:GetName())] = true
        comp:Toggle(false)
      end
    end
  end)
  return names
end

local function showKept(e, names)
  pcall(function()
    for _, comp in ipairs(Game.VREntityComponents(e)) do
      if names[NameToString(comp:GetName())] then comp:Toggle(true) end
    end
  end)
end

local function show(c, on)
  if c.entity == nil or c.shown == on then return end
  showEntity(c.entity, on)
  c.shown = on
end

-- The entity's root, as the engine picks it when it builds the entity (sub_14071C57C): the placeholder it makes when
-- the appearance has several placed components without a parent, else the only one (Carnage: its Receiver).
local function root(e)
  local r = e:FindComponentByName(CName.new("entPlaceholderComponent"))
  if r ~= nil then return r end
  for _, c in ipairs(Game.VREntityComponents(e) or {}) do
    if c:IsA(CName.new("entIPlacedComponent")) and c.parentTransform == nil then
      if r ~= nil then return nil end
      r = c
    end
  end
  return r
end

local function setLocal(c, p, q)
  if c.rootOf ~= c.entity or c.root == nil then c.root, c.rootOf = root(c.entity), c.entity end
  local r = c.root
  if r == nil then return false end
  r:SetLocalTransform(Vector4.new(p[1], p[2], p[3], 1), Quaternion.new(q[1], q[2], q[3], q[4]))
  return true
end

-- The copy where its place puts it, `s` metres drawn out along the pull axis.
local function seat(c, s)
  local p = c.place
  local x, y, z = p.pos[1], p.pos[2], p.pos[3]
  if s and s > 0 and p.pullAxis then
    x, y, z = x + p.pullAxis[1] * s, y + p.pullAxis[2] * s, z + p.pullAxis[3] * s
  end
  return setLocal(c, { x, y, z }, { p.rot[1], p.rot[2], p.rot[3], p.rot[4] })
end

local function attachTo(c, compName, slotName)
  local pl = Game.GetPlayer()
  local comp = pl:FindComponentByName(CName.new(compName))
  if comp == nil then return false end
  pcall(VRDetachFromSlot, c.entity)
  local ok, done = pcall(VRAttachToSlot, c.entity, comp, CName.new(slotName))
  return ok and done == true
end

local function attach(c)
  if not attachTo(c, c.place.comp, c.place.slot) then return false end
  c.mode = "place"
  return seat(c, 0)
end

local function spawn(c, template)
  local pl = Game.GetPlayer()
  local tr = WorldTransform.new()
  local p = pl:GetWorldPosition()
  tr:SetPosition(Vector4.new(p.x, p.y, p.z - 10.0, 1.0))   -- out of sight until it is on its bone
  tr:SetOrientation(Quaternion.new(0, 0, 0, 1))
  if template then
    c.id = exEntitySpawner.Spawn(template, tr)
  else
    c.id = exEntitySpawner.Spawn(ENT, tr, c.look)
  end
  c.entity, c.attached, c.shown, c.mode = nil, false, nil, nil
end

---------------------------------------------------------------- what hangs where

-- The three weapon slots and slot 3, the left hand's partner of slot 0 in dual wielding (the game uses no other).
local function equipped(pd)
  local out = {}
  for i = 0, 3 do
    local id = pd:GetItemInEquipSlot(gamedataEquipmentArea.Weapon, i)
    if ItemID.IsValid(id) then out[#out + 1] = { id = id, slot = i } end
  end
  return out
end

-- Whether the player can dual wield (the Cool perk): without it the game never draws slot 3's gun.
local DUAL_PERK = nil
local function dualPerk(pl)
  local ok, owned = pcall(function()
    DUAL_PERK = DUAL_PERK or gamedataNewPerkType.Cool_Central_Milestone_2
    return PlayerDevelopmentSystem.GetData(pl):IsNewPerkBoughtAnyLevel(DUAL_PERK)
  end)
  return ok and owned == true
end

-- The pair first (see THE PAIR at the top), then the places chosen in the menu, then every other weapon its kind's
-- first free one. A gun of the pair carries `side` (the hand that takes it: 0 left, 1 right), `pair` and `draw` (the
-- item drawn for either: slot 0's, which brings both).
local function assign(items, left, body, perk)
  local taken, out, auto = {}, {}, {}
  local shells = SH.available()
  local function put(a, place, extra)
    if shells then place = SH.place(place, body) end
    local o = { id = a.it.id, place = place, kind = a.kind, typeName = a.typeName, slot = a.it.slot }
    if extra then for k, v in pairs(extra) do o[k] = v end end
    out[#out + 1] = o
  end
  local by = {}
  for _, it in ipairs(items) do
    local typeName = itemTypeName(it.id) or ""
    by[it.slot] = { it = it, typeName = typeName, kind = P.kind(typeName) }
  end
  local r, l = by[0], by[3]
  local pair = perk and r ~= nil and l ~= nil and r.kind == "pistol" and l.kind == "pistol"
  if pair then
    -- both hips, whichever hand draws the rest (the left hip's place is the right one's turned over)
    taken.hip_r, taken.hip_l = true, true
    put(r, P.get("hip_r", false, false), { side = 1, pair = true, draw = r.it.id })
    put(l, P.get("hip_r", true, false), { side = 0, pair = true, draw = r.it.id })
  end
  for _, it in ipairs(items) do
    local a = by[it.slot]
    if it.slot <= 2 and not (pair and it.slot == 0) and a.kind and P.ORDER[a.kind] then
      local c = chosen(it.slot, a.kind)
      if c == "none" then
        -- not carried on the body
      elseif c ~= nil and not taken[c] then
        taken[c] = true
        put(a, P.get(c, left, P.melee(a.typeName)))
      else
        auto[#auto + 1] = a
      end
    end
  end
  for _, a in ipairs(auto) do
    for _, name in ipairs(P.ORDER[a.kind]) do
      if not taken[name] then
        taken[name] = true
        put(a, P.get(name, left, P.melee(a.typeName)))
        break
      end
    end
  end
  -- The mod's chest knife sheath and its holster under the arm are one spot on the harness: with a pistol under the
  -- off arm, the knife goes to the sheath on the other side.
  if shells then
    local under, chest = left and "underarm_r" or "underarm_l", left and "chest_r" or "chest_l"
    local gun, knife = nil, nil
    for _, a in ipairs(out) do
      if a.place.name == under then gun = a elseif a.place.name == chest then knife = a end
    end
    if gun and knife then knife.place = SH.place(P.get("chest_l", not left, true), body) end
  end
  return out
end

-- The shells the copies need (modules/shells.lua): spawned, kept, or let go; a kept one whose pose changed is put
-- on its bone again by settleShells.
local function scanShells(body)
  local want = SH.available() and not PREFS.hidden and SH.wanted(S.copies, body) or {}
  for name, sh in pairs(S.shells) do
    local w = want[name]
    if w == nil or w.ent ~= sh.ent or w.slot ~= sh.slot then
      despawn(sh)
      S.shells[name] = nil
    elseif w.pose.p[1] ~= sh.pose.p[1] or w.pose.p[2] ~= sh.pose.p[2] or w.pose.p[3] ~= sh.pose.p[3]
        or w.pose.q[4] ~= sh.pose.q[4] then
      sh.pose, sh.attached = w.pose, false
    end
  end
  for name, w in pairs(want) do
    if S.shells[name] == nil then
      local sh = { name = name, ent = w.ent, comp = w.comp, slot = w.slot, pose = w.pose }
      spawn(sh, w.ent)
      S.shells[name] = sh
    end
  end
end

local function settleShells()
  for _, sh in pairs(S.shells) do
    if sh.entity == nil and sh.id ~= nil then sh.entity = Game.FindEntityByID(sh.id) end
    if sh.entity ~= nil and not sh.attached then
      sh.attached = attachTo(sh, sh.comp, sh.slot) and setLocal(sh, sh.pose.p, sh.pose.q)
    end
    -- out of sight while a scene plays the body (OFF THE BODY IN A SCENE): only the parts that were on come back,
    -- a shell's appearance keeps some of its meshes off
    if sh.entity ~= nil then
      if S.scene and sh.sceneHidden == nil then
        sh.sceneHidden = hideKept(sh.entity)
      elseif not S.scene and sh.sceneHidden ~= nil then
        showKept(sh.entity, sh.sceneHidden)
        sh.sceneHidden = nil
      end
    end
  end
end

local function scan(pl, pd, enabled)
  local left = drawLeft()
  local body = SH.body()
  local want = {}
  if enabled then
    for _, a in ipairs(assign(equipped(pd), left, body, dualPerk(pl))) do
      local k = key(a.id)
      local look = lookFor(a.id, k)
      if look then
        want[k] = { id = a.id, look = look, place = a.place, kind = a.kind, typeName = a.typeName, side = a.side,
                    pair = a.pair, draw = a.draw }
      end
    end
  end
  for k, c in pairs(S.copies) do
    local w = want[k]
    -- (a new place waits until the copy is home: it is what is seen in the hand until the weapon is)
    local busy = c.mode == "hand" or c.mode == "stow" or c.mode == "homing" or (S.pull ~= nil and S.pull.key == k)
    if w == nil or (not busy and (w.look ~= c.look or w.place ~= c.place)) then
      if S.pull and S.pull.key == k then S.pull = nil; magnetOff() end
      despawn(c)
      S.copies[k] = nil
    end
  end
  for k, w in pairs(want) do
    local c = S.copies[k]
    if c == nil then
      c = { key = k, id = nil, item = w.id, look = w.look, place = w.place, kind = w.kind, typeName = w.typeName }
      spawn(c)
      S.copies[k] = c
    end
    -- (a pistol at the right hip keeps its copy as the pair comes and goes: only the hand and the item drawn change.
    -- Its hold in the hand is the dual wielding one in the pair: `holdKey`)
    c.handSide, c.pair, c.drawItem = w.side, w.pair == true, w.draw
    c.holdKey = w.pair and (k .. "|pair") or k
  end
  scanShells(body)
end

---------------------------------------------------------------- the copy in the hand and on its way home

-- Out of the holster: the copy in the weapon's hold until the weapon itself is there. It rides the hand's own bone, not
-- the weapon slot: that one jumps to the weapon's hold in the frame the weapon arrives, and a copy on it was seen for
-- that frame another 10-35 degrees round, a second blade crossing the first (2026-11-05).
local function intoHand(c, side, hold)
  local bone = side == 0 and "LeftHand" or "RightHand"
  if not attachTo(c, "ItemAttachmentSlots", bone) then return false end
  local b = slotInWrist(Game.GetPlayer(), side, bone)
  if b == nil or hold == nil then
    setLocal(c, { 0, 0, 0 }, { 0, 0, 0, 1 })
  else
    local iq = qconj(b.q)
    setLocal(c, qrot(iq, { hold.p[1] - b.p[1], hold.p[2] - b.p[2], hold.p[3] - b.p[3] }), qmul(iq, hold.q))
  end
  c.mode, c.side, c.until_, c.hold, c.realAt = "hand", side, S.t + IN_HAND_WAIT, hold, nil
  show(c, true)
  return true
end

-- The hold a weapon takes in the hand: read while it was there, or its type's.
local function holdOf(c, left)
  return S.holds[c.holdKey or c.key] or P.hold(c.typeName, c.place.melee, left)
end

-- The hand that takes a copy and puts it back: a gun of the pair its own one, every other weapon the drawing hand.
local function sideOf(c, drawSide) return c.handSide or drawSide end

-- The pair's other gun.
local function partnerOf(c)
  if not c.pair then return nil end
  for _, o in pairs(S.copies) do
    if o ~= c and o.pair then return o end
  end
  return nil
end

-- WHETHER A COPY'S WEAPON IS IN ITS HAND: for a gun of the pair the weapon itself in its hand's slot -- the game draws
-- both at once and the other gun stays in its holster until the animation has it in its hand, and it is put away
-- the same way; for every other weapon the item drawn or held (seen at once, as the request goes in).
local function inHand(c, held, real)
  if c.pair then return real[c.key] ~= nil end
  return held[c.key] == true
end

-- A WEAPON'S OWN HOLD, read once it has been in the hand a moment and the reading is steady (a swing moves the slot).
local function learnHolds(pl, real, drawSide)
  for k, c in pairs(S.copies) do
    local hk = c.holdKey or k
    if real[k] == nil then
      S.inHandAt[k], S.holdRead[k] = nil, nil
    elseif c.place.pull and S.holds[hk] == nil and c.mode == "place" then
      S.inHandAt[k] = S.inHandAt[k] or S.t
      if S.t - S.inHandAt[k] >= LEARN_AFTER then
        local r = slotInWrist(pl, sideOf(c, drawSide))
        local before = S.holdRead[k]
        if r ~= nil and before ~= nil and qangle(r.q, before.q) < math.rad(0.5) then S.holds[hk] = r end
        S.holdRead[k] = r
      end
    end
  end
end

local function home(c)
  if attach(c) then show(c, false) end
  c.mode = "place"
end

-- Put away: the copy takes the weapon's pose in the hand, in its place's bone frame, and goes from there over the
-- holster's mouth (`pull` out along its axis) and into it.
local function stow(pl, c, side, weaponObj)
  local hp, hq = slotWorld(pl, "ItemAttachmentSlots", handSlotName(side))
  local bp, bq = slotWorld(pl, c.place.comp, c.place.slot)
  if hp == nil or bp == nil then return false end
  local iq = qconj(bq)
  local lp = qrot(iq, { hp[1] - bp[1], hp[2] - bp[2], hp[3] - bp[3] })
  local lq = qmul(iq, hq)
  local p = c.place
  local mouth = { p.pos[1], p.pos[2], p.pos[3] }
  local over = p.pull or 0.10
  if p.pullAxis then mouth = { mouth[1] + p.pullAxis[1] * over, mouth[2] + p.pullAxis[2] * over, mouth[3] + p.pullAxis[3] * over } end
  c.stowFrom, c.stowMouth, c.stowAt, c.mode = { p = lp, q = lq }, mouth, S.t, "stow"
  setLocal(c, lp, lq)
  show(c, true)
  -- the weapon going out of the hand stays out of sight: the copy is what is seen going home
  if weaponObj ~= nil then hideReal(c.key, weaponObj) end
  return true
end

-- THE WEAPON MADE UNSEEN BY `stow` IS SEEN AGAIN once it is in a hand with its copy home. The game does not always
-- destroy the weapon it takes out of the hand: drawn again soon it hands back the same object, still unseen -- the gun
-- fired and reloaded with nothing in the hand, and its copy showed again on the hip as soon as it went back
-- (2026-11-04: "after a few draws the weapon's model is gone from the hand").
local function revealReal(real)
  for k, rec in pairs(S.hiddenReal) do
    local c = S.copies[k]
    local busy = c ~= nil and (c.mode == "stow" or c.mode == "homing" or c.mode == "hand")
    if real[k] ~= nil and not busy then
      unhideReal(rec, real[k])
      S.hiddenReal[k] = nil
    elseif c == nil and real[k] == nil then
      S.hiddenReal[k] = nil   -- no longer carried: nothing left to show
    end
  end
end

local function stowTick(c)
  local u = math.min(1, (S.t - c.stowAt) / STOW_TIME)
  local p = c.place
  local rot = { p.rot[1], p.rot[2], p.rot[3], p.rot[4] }
  local pos, q
  if u < 0.6 then
    local a = u / 0.6
    a = a * a * (3 - 2 * a)
    pos, q = lerp3(c.stowFrom.p, c.stowMouth, a), qslerp(c.stowFrom.q, rot, a)
  else
    local a = (u - 0.6) / 0.4
    pos, q = lerp3(c.stowMouth, p.pos, a), rot
  end
  setLocal(c, pos, q)
  -- home, and seen there while the game is still taking the weapon out of the hand
  if u >= 1 then c.mode = "homing" end
end

-- Each copy found once spawned, put on its bone, then shown or hidden by what is in the hands.
local function settle(held, real)
  for k, c in pairs(S.copies) do
    if c.entity == nil and c.id ~= nil then
      c.entity = Game.FindEntityByID(c.id)
      c.foundAt = S.t
    end
    -- A copy being built has its components switched on as its appearance loads, whatever was done to them before, and again
    -- a few frames after it goes on a bone (VRAttachToSlot): "hidden" it hung there beside the weapon in the hand, for
    -- good. So it stays out of sight for both, hidden every tick: built under the floor, on the bone 10 m under the bone, and
    -- only then in its place.
    if c.entity ~= nil and not c.attached then
      local n = showEntity(c.entity, false)
      c.shown = false
      if c.onBoneAt == nil then
        if n > 0 then c.builtBy = c.builtBy or S.t + BUILD_TIME end
        if ((c.builtBy ~= nil and S.t >= c.builtBy) or S.t - c.foundAt > BUILD_LIMIT)
            and attachTo(c, c.place.comp, c.place.slot) and setLocal(c, OUT_OF_SIGHT, { 0, 0, 0, 1 }) then
          c.onBoneAt = S.t
        end
      elseif S.t - c.onBoneAt >= SEAT_DELAY then
        c.mode = "place"
        c.attached = seat(c, 0)
      end
    end
    if c.entity ~= nil and c.attached then
      if c.mode == "hand" then
        -- The weapon itself is in the hand, in its hold: the grasp lets go, the copy goes home, hidden, and the weapon is
        -- seen where the copy was. A weapon whose hold is not known (a type not measured) comes in off the copy's: it
        -- stays unseen a moment and comes in anyway, with its hold read for next time. Or it never came.
        if real[k] ~= nil then
          c.realAt = c.realAt or S.t
          graspOff(c.side)
          local now = slotInWrist(Game.GetPlayer(), c.side)
          local reached = now == nil or c.hold == nil or qangle(now.q, c.hold.q) < HOLD_REACHED
          if reached or S.t - c.realAt > HOLD_WAIT then
            home(c)
          else
            hideReal(k, real[k])
          end
        elseif S.t > c.until_ then
          graspOff(c.side)
          home(c)
        end
      elseif c.mode == "stow" then
        stowTick(c)
        show(c, not S.scene)
        if real[k] ~= nil then hideReal(k, real[k]) end
      elseif c.mode == "homing" then
        show(c, not S.scene)
        if real[k] ~= nil then hideReal(k, real[k]) end
        if not (held[k] or inHand(c, held, real)) or S.t - c.stowAt > IN_HAND_WAIT then c.mode = "place" end
      else
        -- (hidden holsters: unseen on the body, seen once the hand draws it; none in a scene)
        local pulling = S.pull ~= nil and S.pull.key == k
        show(c, not S.scene and (pulling or (not inHand(c, held, real) and not PREFS.hidden)))
      end
    end
  end
end

---------------------------------------------------------------- the hand

local function gripSlot(left) return left and 155 or 49 end

local function handPos(left)
  local ok, p = pcall(VRHandRawWorld, left and 0 or 1)
  if not ok or p == nil or p.w < 0.5 then return nil end
  return { p.x, p.y, p.z }
end

local function dist(a, b)
  local dx, dy, dz = a[1] - b[1], a[2] - b[2], a[3] - b[3]
  return math.sqrt(dx * dx + dy * dy + dz * dz)
end

local function worldPos(e)
  local w = e:GetWorldPosition()
  return { w.x, w.y, w.z }
end

-- The copies a hand can take or put back now: on their bone, at their place.
local function reachable(c) return c.entity ~= nil and c.attached and (c.mode == nil or c.mode == "place") end

-- WHERE A HAND MEETS A COPY, in the world: its origin, the point the hand holds it by -- and for a place with `grab`
-- (over the shoulder) the far end of the handle or the stock too, `grab` metres out along it (placements.lua): the
-- hand is measured to the nearest point of that stretch.
local function grabSpan(pl, c)
  local o = worldPos(c.entity)
  local g = c.place.grab
  if g == nil then return o, nil end
  local _, wq = copyWorld(pl, c, 0)
  if wq == nil then return o, nil end
  local d = qrot(wq, c.place.pullLocal)
  return o, { o[1] + d[1] * g, o[2] + d[2] * g, o[3] + d[3] * g }
end

-- The distance from p to the stretch a..b (to a alone when b is nil).
local function spanDist(p, a, b)
  if b == nil then return dist(p, a) end
  local ab = { b[1] - a[1], b[2] - a[2], b[3] - a[3] }
  local l2 = ab[1] * ab[1] + ab[2] * ab[2] + ab[3] * ab[3]
  local t = 0
  if l2 > 1e-9 then
    t = ((p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2] + (p[3] - a[3]) * ab[3]) / l2
    t = math.max(0, math.min(1, t))
  end
  return dist(p, { a[1] + ab[1] * t, a[2] + ab[2] * t, a[3] + ab[3] * t })
end

local function draw(pl, c)
  local es = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
  local r = DrawItemRequest.new()
  r.owner = pl
  r.itemID = c.drawItem or c.item   -- (either gun of the pair: slot 0's, which brings both)
  es:QueueRequest(r)
  print("[Holster] draw " .. TDBID.ToStringDEBUG(ItemID.GetTDBID(r.itemID)) .. " from " .. c.place.name ..
        (c.pair and " (the pair)" or ""))
end

-- The guards of the old holster: a cigarette in the hands, a hand on the steering wheel, a grenade on the belt
-- under the hand (lit only when no holstered weapon is nearer, H.near), a throw wound up -- the grip is theirs.
local function gripOwnedElsewhere(pl, left)
  local okSmoke, smoking = pcall(function() return pl:VRSmokeHasCig() end)
  if okSmoke and smoking then return true end
  local wheelArmed = GetVRSharedSlot(163)
  if wheelArmed and (math.floor(wheelArmed / (left and 2 or 1)) % 2) == 1 then return true end
  local okBelt, grenade = pcall(function()
    local belt = GetMod("CyberpunkVRPort_TacticalBelt")
    return belt ~= nil and belt.grenadeReach ~= nil and belt.grenadeReach(left and 0 or 1)
  end)
  if okBelt and grenade then return true end
  local okThrow, throw = pcall(function()
    local w = pl:GetActiveWeapon()
    return w ~= nil and pl:VRMeleeThrowReady(w)
  end)
  return okThrow and throw == true
end

local function holsterHeld(pl)
  local melee = false
  pcall(function()
    local w = pl:GetActiveWeapon()
    melee = w ~= nil and w:IsMelee()
  end)
  if melee then pl:VRHolsterMelee() else pl:VRUnequipWeapon() end
  print("[Holster] put away")
end

-- A gun of the pair into its own hand at once, the fingers round its grip: the other hand is drawing the pair, or has.
-- `waits`: taken while the other hand is still pulling its gun out (let go before that, and this one goes back too).
local function takePartner(c, side, waits)
  local left = side == 0
  local hold = holdOf(c, left)
  if not intoHand(c, side, hold) then return false end
  graspOn(side, false, hold, P.holdEmpty(left))
  c.pairWait = waits
  print("[Holster] the pair's " .. TDBID.ToStringDEBUG(ItemID.GetTDBID(c.item)) .. " into the " ..
        (left and "left" or "right") .. " hand" .. (waits and " (the other hand still pulling)" or ""))
  return true
end

-- A pull let go before the weapon was out: the copy back in its place, and a gun of the pair taken by the other hand
-- meanwhile back in its own (nothing is drawn).
local function endPull(c)
  if c ~= nil and c.entity ~= nil then seat(c, 0) end
  if S.pull ~= nil then graspOff(S.pull.side) end
  S.pull = nil
  magnetOff()
  for _, o in pairs(S.copies) do
    if o.pairWait then
      o.pairWait = nil
      if o.mode == "hand" then
        graspOff(o.side)
        home(o)
      end
    end
  end
end

-- A weapon being drawn out of its place, the hand on it.
local function pullTick(pl, grip)
  local side = S.pull.side
  local c = S.copies[S.pull.key]
  local hp = handPos(side == 0)
  if c == nil or c.entity == nil or hp == nil or not grip then endPull(c) return end
  local wp, wq = copyWorld(pl, c, S.pull.s)
  if wp == nil then endPull(c) return end
  -- along the holster: out of it, the weapon's -Y (a gun, grip first) or -Z (a blade, along its handle)
  local d = qrot(wq, c.place.pullLocal)
  local s = (hp[1] - S.pull.h0[1]) * d[1] + (hp[2] - S.pull.h0[2]) * d[2] + (hp[3] - S.pull.h0[3]) * d[3]
  s = math.max(0, math.min(c.place.pull, s))
  S.pull.s = s
  seat(c, s)
  wp, wq = copyWorld(pl, c, s)
  if wp ~= nil and S.pull.rel ~= nil then magnetTo(side, S.pull.rel, wp, wq) end
  if s >= c.place.pull * 0.95 then
    -- out: the hand goes back to its controller, the copy rides in it until the weapon itself is there
    local hold = S.pull.rel
    S.pull = nil
    releaseWrist(side)
    if not intoHand(c, side, hold) then
      graspOff(side)
      seat(c, 0)
    end
    -- (the other gun taken meanwhile waits in its hand from now on, as this one does)
    for _, o in pairs(S.copies) do
      if o.pairWait then o.pairWait, o.until_ = nil, S.t + IN_HAND_WAIT end
    end
    draw(pl, c)
    S.cooldown = COOLDOWN
  end
end

-- One hand's new grip: the copy nearest to it that this hand takes and puts back.
local function grab(pl, held, real, side, drawSide)
  local left = side == 0
  local hp = handPos(left)
  if hp == nil or gripOwnedElsewhere(pl, left) then return end
  local nearest, nd = nil, 1e9
  for _, c in pairs(S.copies) do
    if reachable(c) and sideOf(c, drawSide) == side then
      local d = spanDist(hp, grabSpan(pl, c))
      if d < c.place.reach and d < nd then nearest, nd = c, d end
    end
  end
  if nearest == nil then return end

  if nearest.pair then
    local other = partnerOf(nearest)
    -- the other hand is pulling its gun out: this one comes along with this hand
    if S.pull ~= nil then
      if other ~= nil and S.pull.key == other.key then takePartner(nearest, side, true) end
      return
    end
    -- the pair on its way out (the other hand drew it, the game is bringing this gun): into this hand now
    if held[nearest.key] and not inHand(nearest, held, real) then
      takePartner(nearest, side, false)
      return
    end
  elseif S.pull ~= nil then
    return
  end
  if S.cooldown > 0 then return end

  if inHand(nearest, held, real) then
    -- the weapon in hand at its own place: it goes home by itself -- the pair both, each from its own hand
    if not held[nearest.key] then return end   -- (the game is already taking it out of the hand)
    stow(pl, nearest, side, real[nearest.key])
    local other = partnerOf(nearest)
    if other ~= nil and reachable(other) and real[other.key] ~= nil then
      stow(pl, other, sideOf(other, drawSide), real[other.key])
    end
    holsterHeld(pl)
    S.cooldown = COOLDOWN
    return
  end
  -- (another weapon in hand: the grip on a different copy takes that one instead, the game switching them)
  if nearest.place.pull then
    S.pull = { key = nearest.key, side = side, h0 = hp, s = 0, rel = holdOf(nearest, left) }
    graspOn(side, nearest.place.melee, S.pull.rel, P.holdEmpty(left))
    local wp, wq = copyWorld(pl, nearest, 0)
    if wp ~= nil and S.pull.rel ~= nil then magnetTo(side, S.pull.rel, wp, wq) end
    return
  end
  show(nearest, false)
  draw(pl, nearest)
  S.cooldown = COOLDOWN
end

-- THE HANDS: the drawing hand takes and puts back every weapon but the pair's left (right) gun, which is the other
-- hand's. One pull at a time; while it lasts the other hand only takes its gun of the pair.
local function hand(pl, held, real, dt, enabled)
  local drawSide = drawLeft() and 0 or 1
  local grips, edges = {}, {}
  for side = 0, 1 do
    local g = (GetVRSharedSlot(gripSlot(side == 0)) or 0) > 0.5
    grips[side], edges[side] = g, g and not S.gripPrev[side]
    S.gripPrev[side] = g
  end
  -- Sample even while suspended: neither a held press nor a missed release
  -- may be carried across a scene, mode switch, or player detach.
  if not enabled then
    if S.pull ~= nil then endPull(S.copies[S.pull.key]) end
    return
  end
  if S.pull ~= nil then pullTick(pl, grips[S.pull.side]) end
  -- the drawing hand first: the same grip on both sides in one frame starts its pull, the other hand comes along
  for _, side in ipairs({ drawSide, 1 - drawSide }) do
    if edges[side] and (S.pull == nil or S.pull.side ~= side) then grab(pl, held, real, side, drawSide) end
  end
end

---------------------------------------------------------------- the frame

function H.update(dt, enabled)
  dt = dt or 0
  S.t = S.t + dt
  S.cooldown = math.max(0, S.cooldown - dt)
  releaseTick()
  local pl = Game.GetPlayer()
  if pl == nil then hand(nil, nil, nil, dt, false) return end
  local es, pd = playerData(pl)
  if pd == nil then hand(pl, nil, nil, dt, false) return end
  local held, real = inHands(pl, pd)
  S.scene = sceneNow(S.t)
  if S.t - S.scanAt >= SCAN_EVERY then
    S.scanAt = S.t
    scan(pl, pd, enabled)
  end
  settle(held, real)
  revealReal(real)
  learnHolds(pl, real, drawLeft() and 0 or 1)
  settleShells()
  hand(pl, held, real, dt, enabled and not S.scene)
  graspTick(dt)
end

function H.release()
  if S.pull ~= nil then endPull(S.copies[S.pull.key]) end
  pcall(function()
    local pl = Game.GetPlayer()
    if pl == nil then return end
    local _, pd = playerData(pl)
    if pd == nil then return end
    local _, real = inHands(pl, pd)
    for k, rec in pairs(S.hiddenReal) do
      if real[k] ~= nil then unhideReal(rec, real[k]) end
    end
  end)
  S.hiddenReal = {}
  magnetOff()
  for side = 0, 1 do
    if S.grasp[side] > 0 and type(VRGraspClear) == "function" then pcall(VRGraspClear, side) end
    S.grasp[side], S.graspWant[side] = 0, 0
  end
  for k, c in pairs(S.copies) do despawn(c) end
  for _, sh in pairs(S.shells) do despawn(sh) end
  S.copies, S.shells, S.pull = {}, {}, nil
end

-- FOR THE BELT: the distance from a point in the player's MODEL space (the belt's palm, VRPalmModelPos) to the
-- nearest holstered weapon hand `side` could take or put back there (the drawing hand's, or its gun of the pair),
-- within that place's reach; nil when none.
function H.near(x, y, z, side)
  local drawSide = drawLeft() and 0 or 1
  local best = nil
  local pl = Game.GetPlayer()
  if pl == nil then return nil end
  local function model(w)
    if w == nil then return nil end
    local m = VRWorldToModel(Vector4.new(w[1], w[2], w[3], 1))
    return { m.x, m.y, m.z }
  end
  for _, c in pairs(S.copies) do
    if reachable(c) and sideOf(c, drawSide) == side then
      local a, b = grabSpan(pl, c)
      local d = spanDist({ x, y, z }, model(a), model(b))
      if d < c.place.reach and (best == nil or d < best) then best = d end
    end
  end
  return best
end

-- FOR THE VR MENU'S INVENTORY: the holster of weapon slot `slot` (0..2) -- nil when its weapon is not carried on the
-- body at all (none, fists, an implant, a grenade). `choice` is what is chosen ("auto", "none" or a place), `place`
-- where it hangs now (nil: nowhere, no place free or "none"), `choices` the places its kind may take; each name both
-- as chosen (right-handed) and as shown (this player's side: `shown`).
function H.slotInfo(slot)
  local pl = Game.GetPlayer()
  if pl == nil then return nil end
  local _, pd = playerData(pl)
  if pd == nil then return nil end
  local id = pd:GetItemInEquipSlot(gamedataEquipmentArea.Weapon, slot)
  if not ItemID.IsValid(id) then return nil end
  local typeName = itemTypeName(id) or ""
  local kind = P.kind(typeName)
  if kind == nil or P.CHOICES[kind] == nil then return nil end
  local left = drawLeft()
  local function shown(n) return left and (P.MIRROR_NAME[n] or n) or n end
  local place, pair = nil, false
  local k = key(id)
  for _, a in ipairs(assign(equipped(pd), left, SH.body(), dualPerk(pl))) do
    if key(a.id) == k then place, pair = a.place.name, a.pair == true end
  end
  local choices = {}
  for _, n in ipairs(P.CHOICES[kind]) do choices[#choices + 1] = { name = n, shown = shown(n) } end
  local auto = P.ORDER[kind][1]
  -- `pair`: slot 0's gun with its dual wielding partner -- at the right hip whatever is chosen (THE PAIR at the top)
  return { kind = kind, family = P.FAMILY[kind], choice = PREFS.slots[slot + 1] or "auto", place = place,
           choices = choices, auto = auto and shown(auto) or nil, left = left, pair = pair }
end

-- A slot's choice ("auto", "none" or a place of its kind's); the copies move at the next scan.
function H.setPlace(slot, name)
  if type(slot) ~= "number" or slot < 0 or slot > 2 then return false end
  PREFS.slots[slot + 1] = (name == nil or name == "") and "auto" or name
  S.scanAt = -1
  return savePrefs()
end

function H.hidden() return PREFS.hidden == true end
function H.setHidden(on)
  PREFS.hidden = on == true
  S.scanAt = -1
  return savePrefs()
end

-- For the harness: GetMod("CyberpunkVRPort_Holster").state()
function H.state()
  local out = { copies = {}, pull = S.pull and { key = S.pull.key, s = S.pull.s, rel = S.pull.rel ~= nil } or nil,
                cooldown = S.cooldown, magnet = S.magnet, grasp = { S.grasp[0], S.grasp[1] } }
  for k, c in pairs(S.copies) do
    local w = nil
    if c.entity ~= nil then pcall(function() w = worldPos(c.entity) end) end
    out.copies[#out.copies + 1] = { key = k, item = TDBID.ToStringDEBUG(ItemID.GetTDBID(c.item)), look = c.look,
      place = c.place.name, kind = c.kind, attached = c.attached, shown = c.shown, mode = c.mode, pos = w,
      hand = c.handSide, pair = c.pair }
  end
  out.shells = {}
  for name, sh in pairs(S.shells) do
    out.shells[#out.shells + 1] = { name = name, ent = sh.ent, attached = sh.attached, found = sh.entity ~= nil }
  end
  return out
end

-- The places, for tuning from the console: GetMod("CyberpunkVRPort_Holster").holsters().placements
H.placements = P
H.shells = SH
H._S = S          -- the module's state, for the harness

return H
