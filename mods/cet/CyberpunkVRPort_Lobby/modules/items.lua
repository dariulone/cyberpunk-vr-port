-- The things on the table: physical bodies to pick up, carry and throw.
--
-- WHAT THEY ARE. Each item is an entity of its own (make_lobby_scene.py: an entPhysicalMeshComponent "item" over a
-- mesh carrying a dynamic rigid body shaped and weighted for it), spawned where it rests on the table
-- (scene_list.lua items, in the lobby's frame) apart from the rest of the place, so it can be spawned again. The
-- lobby opens them once the place has spawned: put there earlier, they would fall through a table not yet in the
-- world.
--
-- HOLDING ONE, the way the game holds a weapon: a closing grip takes the ONE item whose surface is nearest the palm,
-- within reach of it, and the item itself -- not a copy -- is carried by the hand's slot (the player's
-- ItemAttachmentSlots, WeaponLeft / WeaponRight) where the hand closed on it: its body made kinematic, its root made
-- a child of the slot in the engine's transform graph (VRAttachToSlot, src/Natives/EntityAttach.cpp). The engine then
-- moves it in the same pass as the hand's bone. A world body steered or placed from here reaches the screen a frame
-- behind the hand, and walking or turning with one in hand left it trailing behind the character by that frame. Its
-- body stays a World Dynamic body: kinematic, driven every frame to where the slot carries the item (the slot's pose
-- times the item's in it), it pushes what the item in hand runs into. It is out of scene queries while held (the
-- character controller sweeps them, and the player would walk into their own hand). Letting go takes the item off
-- the slot where it is, dynamic again, with the velocity it was carried at over the last frames: the throw.
--
-- A body is only moved in one go while it is DYNAMIC. SetTransform teleports a dynamic body, but drives a kinematic
-- one there as a kinematic target, swept in one step through everything between: the hand taking an engram from
-- among the others brought its body from the table into the palm that way and threw the ones beside it metres
-- (measured 2026-10-28: 2.5 m and 2.2 m, one at 100 m/s). So the body goes into the palm before it is made
-- kinematic, and is made dynamic before it is put where it was let go; carried, it only ever moves a frame's way.
--
-- Without the natives (an older plugin) the item is steered instead, the way the port's basketball is held
-- (mods/redscript/CyberpunkVRPort_Basketball): a dynamic body under a velocity command toward the hand, a bounded
-- acceleration, its own velocity the throw.
--
-- An item that has fallen out of the place or flown far away goes back on the table by itself (M.reset puts them all
-- back, for tests).
--
-- The hand closes round what it holds (modules/grips.lua: the thing seated in the palm and the fingers closed on its
-- shape and size, the entry's `shape` and `half`), and opens again as it lets go.
--
-- The list may carry more than the scene's things: the engrams (modules/engrams.lua) are things on the table too. One
-- can be TAKEN from the hand -- gone from the world until it is put back (M.take, M.restore); a reset leaves it
-- where it went.
--
-- REACHING, marked and done the way Into the Radius 2 does it, and the way the game's own things are reached for
-- (CyberpunkVRPort_Reach: it draws the marker, and its modules/world.lua is the same gesture in the game): a free hand
-- near an item sees a ring on the one it would take, faint while it is near and full once it is in reach. A free hand
-- pointing at an item further off sees the ring on that one; the grip closed on it holds it from afar (the ring turns
-- into brackets), and the hand snatched back from it pulls it to the hand -- flying, a dynamic body under a velocity
-- command, so it knocks what is in its way rather than passing through -- and the hand takes it as it arrives. Letting
-- go of the grip lets go of it. A pointer on a menu panel nearer than the item is the panel's; the trigger is the
-- panels' (it presses them), so it plays no part here.
--
-- The hands are modules/hands.lua's, read once a frame for everything in the lobby a hand takes hold of.
local M = {}
local Spawner = require("modules/spawner")
local Hands = require("modules/hands")
local Grips = require("modules/grips")
local V = require("modules/vec")

local TAG = "VRLobbyItem"
local ITEM = "item"                  -- the component holding each item's body
local REACH = 0.10                   -- how far from an item's surface the palm (or the controller) takes it
local GAIN, MAX_SPEED, ACCEL, SPIN, RECOVER = 25.0, 9.0, 80.0, 12.0, 0.20
local LOST_BELOW, LOST_AWAY, LOST_EVERY = 3.0, 60.0, 0.5
local SOUND_DIR = "CyberpunkVRPort_Menu/sounds/"
local HAPTIC_CLICK = 20
local SLOTS, HAND_SLOT = "ItemAttachmentSlots", { "WeaponLeft", "WeaponRight" }
local THROW_WINDOW = 0.06            -- s of carried motion the throw's velocity is measured over
local THROW_FRAMES = 2               -- frames after letting go the throw is given again
local HOVER = 0.22                   -- how far from an item's surface its ring shows (m)
local AIM_RANGE = 6.0                -- how far off a hand points at items (m)
local AIM_CONE = math.rad(6)         -- how far off the palm's ray, beyond the item's own size, still points at it
local RAY_RADIUS = 0.06              -- and how far off it besides, however near the hand (m)
local FLICK = 1.1                    -- m/s: the controller moving away from the held item this fast pulls it
local PULL_SPEED, PULL_ACCEL = 7.0, 70.0      -- the pulled item's flight: m/s at most, m/s2 at most
local PULL_SLOW = 4.0                -- 1/s: how its speed falls off near the hand (speed = distance x this)
local PULL_LONGEST = 2.0             -- s a pull may take before it gives up

-- held[hand]: the item a hand holds; attached (on the hand's slot), its pose in the slot (lp, lq) and the poses it was
-- carried through (trail), or where it sits in the hand for steering (offset, relRot). reach[hand]: the item a free
-- hand points at (aim), the one its grip holds from afar (lock; snatch: the fastest pull away read while held), the
-- one flying to it (pull, pullFor).
local S = { anchor = nil, list = nil, items = {}, held = { {}, {} }, giving = {}, resets = 0, throws = {},
            reach = { {}, {} }, pulls = 0 }

---------------------------------------------------------------- feedback

local function play(name)
  if type(VRMenuPlayWav) == "function" then pcall(VRMenuPlayWav, SOUND_DIR .. name .. ".wav", 0.8) end
end
local function haptic(hand)
  if type(VRHapticEvent) == "function" then pcall(VRHapticEvent, HAPTIC_CLICK, hand == 2 and 2 or 1) end
end

---------------------------------------------------------------- the items in the world

-- at: where in the world, when not where it rests (a thing given to a hand).
local function spawnItem(entry, at)
  local p = V.toWorld(S.anchor, entry.position)
  local r = entry.rotation
  local q = V.qmul(V.yawQuat(S.anchor.yaw), { r = r[1], i = r[2], j = r[3], k = r[4] })
  local tr = WorldTransform.new()
  local w = at or { p[1], p[2], p[3] + 0.01 }
  tr:SetPosition(Vector4.new(w[1], w[2], w[3], 1.0))
  tr:SetOrientation(Quaternion.new(q.i, q.j, q.k, q.r))
  return { entry = entry, id = Spawner.spawn(entry.ent, tr, TAG), home = p }
end

local function despawnItem(it)
  Spawner.delete(it.id)
  it.id, it.entity, it.body = nil, nil, nil
end

-- The entity and its body, once the spawn has landed.
local function resolve(it)
  if it.body ~= nil then return true end
  if not Spawner.spawned(it.id) then return false end
  pcall(function()
    it.entity = Game.FindEntityByID(it.id)
    if it.entity ~= nil then
      local c = it.entity:FindComponentByName(CName.new(ITEM))
      if c ~= nil then it.body = c:CreatePhysicalBodyInterface(0) end
    end
  end)
  return it.body ~= nil
end

-- The velocity a carried item moved at, linear and angular, over the last THROW_WINDOW of its trail.
local function throwVelocity(trail)
  local last = trail[#trail]
  if last == nil then return { 0, 0, 0 }, { 0, 0, 0 } end
  local first = last
  for n = #trail - 1, 1, -1 do
    first = trail[n]
    if last.t - first.t >= THROW_WINDOW then break end
  end
  local span = last.t - first.t
  if span <= 0 then return { 0, 0, 0 }, { 0, 0, 0 } end
  -- q_delta = q_last * conj(q_first): for the small turn of a few frames its vector part is half the axis times
  -- the angle.
  local qd = V.qmul(last.rot, V.qconj(first.rot))
  local s = (qd.r < 0 and -2 or 2) / span
  return V.mul(V.sub(last.pos, first.pos), 1 / span), { qd.i * s, qd.j * s, qd.k * s }
end

-- How far p (the world) is from an item's surface: its shape and half sizes in its own frame (scene_list.lua,
-- modules/engrams.lua), else its radius round its centre; nil when it cannot be read.
local function surface(it, p)
  local ok, d = pcall(function()
    local e = it.entry
    local l = V.qrot(V.qconj(V.quat(it.entity:GetWorldOrientation())), V.sub(p, V.v3(it.entity:GetWorldPosition())))
    if e.centre ~= nil then l = V.sub(l, e.centre) end
    local h = e.half
    if e.shape == nil or type(h) ~= "table" then return V.len(l) - (e.radius or 0) end
    if e.shape == "sphere" then return V.len(l) - h[1] end
    local q
    if e.shape == "cylinder" or e.shape == "cone" then
      q = { math.sqrt(l[1] * l[1] + l[2] * l[2]) - math.max(h[1], h[2]), math.abs(l[3]) - h[3] }
    else
      q = { math.abs(l[1]) - h[1], math.abs(l[2]) - h[2], math.abs(l[3]) - h[3] }
    end
    local out, inside = 0, -math.huge
    for _, c in ipairs(q) do
      out = out + math.max(c, 0) ^ 2
      inside = math.max(inside, c)
    end
    return math.sqrt(out) + math.min(inside, 0)
  end)
  return ok and d or nil
end

-- The hand's slot takes the item where it is; false when it cannot (the item is then steered).
local function attach(hand, it)
  if type(VRAttachToSlot) ~= "function" then return false end
  local ok, done = pcall(function()
    local slots = Game.GetPlayer():FindComponentByName(CName.new(SLOTS))
    if slots == nil then return false end
    it.body:SetIsKinematic(true)
    return VRAttachToSlot(it.entity, slots, CName.new(HAND_SLOT[hand]))
  end)
  if ok and done == true then return true end
  pcall(function() it.body:SetIsKinematic(false) end)
  return false
end

-- The body where the carried item is. The slot carries the entity, not its physics: left alone the kinematic body
-- stays where the hand took it -- colliding there, unseen -- and turned dynamic again it puts the item back there
-- (measured 2026-10-28: let go 25 cm above the table, the can was back on its spot the same frame).
local function bodyTo(it, pos, rot)
  pcall(function()
    VRBodySetPos(it.body, pos or it.entity:GetWorldPosition(), rot or it.entity:GetWorldOrientation())
  end)
end

-- The pose of the hand's slot in the world: its position (a WorldPosition, read through its getters) and turn.
local function slotPose(hand)
  local _, wt = Game.GetPlayer():FindComponentByName(CName.new(SLOTS)):GetSlotTransform(CName.new(HAND_SLOT[hand]))
  local p = wt:GetWorldPosition()
  return { p:GetX(), p:GetY(), p:GetZ() }, V.quat(wt:GetOrientation())
end

-- Where the slot carries a held item now, in the world (the same pass the engine moves it in), or nil.
local function carried(hand, P)
  local ok, pos, rot = pcall(function()
    local sp, sq = slotPose(hand)
    return V.add(sp, V.qrot(sq, P.lp)), V.qmul(sq, P.lq)
  end)
  if ok then return pos, rot end
  return nil
end

local function throwOn(T)
  pcall(VRBodySetVel, T.it.body, T.v)
  pcall(VRBodySetAngVel, T.it.body, T.w)
end

local function release(hand)
  local P = S.held[hand]
  local it = S.items[P.item]
  if it ~= nil and it.body ~= nil then
    if P.attached then
      local v, w = throwVelocity(P.trail)
      local ok, pos, rot = pcall(function() return it.entity:GetWorldPosition(), it.entity:GetWorldOrientation() end)
      pcall(VRDetachFromSlot, it.entity)
      pcall(function() it.body:SetIsKinematic(false) end)
      if ok then
        -- Where it was seen, not where the detach puts it: the engine restores the root from the world pose it
        -- keeps, which for a root a slot carried is not where the slot carried it (measured 25 cm off).
        pcall(function() it.entity:FindComponentByName(CName.new(ITEM)):SetLocalTransform(pos, rot) end)
        bodyTo(it, pos, rot)
      end
      -- The throw, given again on the next frames too: written in the frame the body is placed and made dynamic it
      -- can be lost to that.
      S.throws[#S.throws + 1] = { it = it, v = Vector4.new(v[1], v[2], v[3], 0), w = Vector4.new(w[1], w[2], w[3], 0),
                                  frames = THROW_FRAMES }
      S.lastThrow = V.len(v)
      throwOn(S.throws[#S.throws])
    end
    pcall(function() it.body:SetIsQueryable(true) end)
  end
  P.item, P.attached, P.trail, P.lp, P.lq = nil, false, nil, nil, nil
  Hands.release(hand, M)
  Grips.release(hand)
end

local function putBack(i)
  local it = S.items[i]
  for hand = 1, 2 do if S.held[hand].item == i then release(hand) end end
  despawnItem(it)
  S.items[i] = spawnItem(it.entry)
end

---------------------------------------------------------------- holding

-- Where in the hand an item taken lies, as a pose local to the hand's slot, from where it was when the hand took it
-- (`at`, `turn`). A thing of a known shape lies where the plugin seats it for its grasp (modules/grips.lua M.seat);
-- another, when only the controller reached it (an arm short of a reach over the table), is brought to the drawn
-- hand at the offset it had from the controller; else it stays where it is. Nil when the slot cannot be read.
local function seat(hand, it, H, snap, at, turn)
  local ok, pos, rot = pcall(function()
    local sp, sq = slotPose(hand)
    local inv = V.qconj(sq)
    local lq = V.qmul(inv, turn)
    local lp = V.qrot(inv, V.sub(at, sp))
    local p, q = Grips.seat(hand, it.entry, lp, lq)
    if p ~= nil then return p, q end
    if snap and H.reach ~= nil then
      local off = V.sub(at, H.reach)
      local l, most = V.len(off), it.entry.radius + 0.02
      if l > most then off = V.mul(off, most / l) end
      return V.qrot(inv, V.sub(V.add(H.pos, off), sp)), lq
    end
    return lp, lq
  end)
  if ok then return pos, rot end
  return nil
end

-- The hand holds item i: seated in the palm, the fingers closed on it.
local function hold(hand, i, snap)
  local H, P = Hands.hands[hand], S.held[hand]
  local it = S.items[i]
  local inv = V.qconj(H.rot)
  P.item = i
  P.offset = V.qrot(inv, V.sub(V.v3(it.entity:GetWorldPosition()), H.pos))
  P.relRot = V.qmul(inv, V.quat(it.entity:GetWorldOrientation()))
  Hands.claim(hand, M)
  pcall(function() it.body:SetIsQueryable(false) end)
  local at, turn = V.v3(it.entity:GetWorldPosition()), V.quat(it.entity:GetWorldOrientation())
  P.lp, P.lq = nil, nil
  if type(VRAttachToSlot) == "function" then P.lp, P.lq = seat(hand, it, H, snap, at, turn) end
  -- The body into the palm while it is still dynamic: a teleport, not a sweep across the table (see HOLDING ONE).
  local wpos, wrot = nil, nil
  if P.lp ~= nil then wpos, wrot = carried(hand, P) end
  if wpos ~= nil then
    bodyTo(it, Vector4.new(wpos[1], wpos[2], wpos[3], 1), Quaternion.new(wrot.i, wrot.j, wrot.k, wrot.r))
  end
  P.attached, P.trail, P.clock = attach(hand, it), {}, 0
  if P.attached and P.lp ~= nil then
    pcall(function()
      it.entity:FindComponentByName(CName.new(ITEM)):SetLocalTransform(
        Vector4.new(P.lp[1], P.lp[2], P.lp[3], 1), Quaternion.new(P.lq.i, P.lq.j, P.lq.k, P.lq.r))
    end)
  elseif wpos ~= nil then
    -- Not on the slot after all: steered from where its body went.
    P.offset = V.qrot(inv, V.sub(wpos, H.pos))
    P.relRot = V.qmul(inv, wrot)
  end
  Grips.hold(hand, it.entry, P.attached and P.lp or nil, P.attached and P.lq or nil)
  haptic(hand)
end

-- An item another hand holds or pulls, or one taken out of the world: not this hand's to reach for.
local function claimed(hand, i)
  local other = 3 - hand
  return S.held[other].item == i or S.reach[other].pull == i or S.items[i].gone
end

-- The item whose surface is nearest the hand within `within` (m): the drawn hand, or the controller when the arm
-- falls short of it (a hand reaching over the table); by the item's surface, not its centre -- a hand on a big ball
-- is far from its centre and near a small shard's. Its index, that distance, and whether only the controller got
-- there.
local function nearest(hand, within)
  local H = Hands.hands[hand]
  local best, bestD, bestFar = nil, nil, false
  for i, it in ipairs(S.items) do
    if not claimed(hand, i) and resolve(it) then
      local d = surface(it, H.pos)
      local far = d ~= nil and d >= within
      if far and H.reach ~= nil then d = surface(it, H.reach) end
      if d ~= nil and d < within and (bestD == nil or d < bestD) then best, bestD, bestFar = i, d, far end
    end
  end
  return best, bestD, bestFar
end

-- Takes the item in reach; true when there was one.
local function grab(hand)
  local best, _, far = nearest(hand, REACH)
  if best ~= nil then hold(hand, best, far) end
  return best ~= nil
end

local function carry(hand, dt)
  local H, P = Hands.hands[hand], S.held[hand]
  local it = S.items[P.item]
  if it == nil or not resolve(it) then release(hand); return end
  if P.attached then
    -- Carried by the slot: its body driven after it, a frame's way at a time (it pushes what the item runs into), and
    -- where it went, for the throw. Where the slot carries it, not the entity's world pose: that is the last
    -- transform pass's, the table's in the frame the hand took it, and the body would be swept back there.
    P.clock = P.clock + dt
    local pos, rot = nil, nil
    if P.lp ~= nil then pos, rot = carried(hand, P) end
    if pos == nil then
      local ok, p, q = pcall(function()
        return V.v3(it.entity:GetWorldPosition()), V.quat(it.entity:GetWorldOrientation())
      end)
      if ok then pos, rot = p, q end
    end
    if pos ~= nil then
      bodyTo(it, Vector4.new(pos[1], pos[2], pos[3], 1), Quaternion.new(rot.i, rot.j, rot.k, rot.r))
      P.trail[#P.trail + 1] = { t = P.clock, pos = pos, rot = rot }
      while #P.trail > 2 and P.clock - P.trail[2].t > THROW_WINDOW do table.remove(P.trail, 1) end
    end
    return
  end
  local cur = V.v3(it.entity:GetWorldPosition())
  local target = V.add(H.pos, V.qrot(H.rot, P.offset))
  local miss = V.len(V.sub(target, cur))
  local far = miss > RECOVER and math.min(miss / RECOVER, 12.0) or 1.0
  local want = V.add(V.add(H.vel, V.mul(V.sub(target, cur), GAIN)), { 0, 0, 9.81 * dt })
  local sp = V.len(want)
  if sp > MAX_SPEED * far then want = V.mul(want, MAX_SPEED * far / sp) end
  pcall(function()
    it.body:SetIsKinematic(false)
    it.body:SetIsQueryable(false)
  end)
  -- The pull: an impulse bounded by the hand's acceleration (the body's own velocity read back); written outright
  -- only when the velocity cannot be read.
  local pulled = pcall(function()
    local v = VRBodyGetVel(it.body)
    if v.w < 0.5 then error("no velocity") end
    local dv = V.sub(want, { v.x, v.y, v.z })
    local cap = ACCEL * far * dt
    local l = V.len(dv)
    if l > cap and l > 1e-4 then dv = V.mul(dv, cap / l) end
    local m = it.entry.mass
    it.body:AddLinearImpulse(Vector4.new(dv[1] * m, dv[2] * m, dv[3] * m, 0), true)
  end)
  if not pulled then pcall(VRBodySetVel, it.body, Vector4.new(want[1], want[2], want[3], 0)) end
  pcall(function()
    -- q_err = q_target * conj(q_current): for a small error its vector part is half the axis times the angle.
    local qt = V.qmul(H.rot, P.relRot)
    local qe = V.qmul(qt, V.qconj(V.quat(it.entity:GetWorldOrientation())))
    local s = qe.r < 0 and -1 or 1
    VRBodySetAngVel(it.body, Vector4.new(2 * SPIN * qe.i * s, 2 * SPIN * qe.j * s, 2 * SPIN * qe.k * s, 0))
  end)
end

---------------------------------------------------------------- reaching

local function reachMod()
  local ok, mod = pcall(GetMod, "CyberpunkVRPort_Reach")
  return ok and mod or nil
end

-- An item's centre in the world (its entity's origin, moved by the entry's centre when it has one).
local function centre(it)
  local ok, c = pcall(function()
    local p = V.v3(it.entity:GetWorldPosition())
    if it.entry.centre == nil then return p end
    return V.add(p, V.qrot(V.quat(it.entity:GetWorldOrientation()), it.entry.centre))
  end)
  return ok and c or nil
end

-- How far from an item's centre its farthest point is: where the marker hangs before it, toward the eyes, so that
-- no corner of it covers the ring.
local function bound(entry)
  local h = entry.half
  if entry.shape == nil or type(h) ~= "table" then return (entry.radius or 0.05) + 0.01 end
  if entry.shape == "sphere" then return h[1] + 0.01 end
  if entry.shape == "cylinder" or entry.shape == "cone" then
    return math.sqrt(math.max(h[1], h[2]) ^ 2 + h[3] ^ 2) + 0.01
  end
  return math.sqrt(h[1] * h[1] + h[2] * h[2] + h[3] * h[3]) + 0.01
end

-- What the marker is given for item `it` this frame.
local function markOf(it, kind, H, ready)
  return { pos = centre(it), radius = it.entry.radius, lift = bound(it.entry), kind = kind, ready = ready,
           from = H.pos }
end

-- The item the hand's palm points at, from the hand to AIM_RANGE and short of a panel its pointer is on (ui: how far
-- along that panel is): the one nearest the palm's ray for its size and distance, the ray RAY_RADIUS thick.
local function pointed(hand, ui)
  local ray = Hands.hands[hand].palm
  if ray == nil then return nil end
  local best, bestScore = nil, nil
  for i, it in ipairs(S.items) do
    if not claimed(hand, i) and resolve(it) then
      local c = centre(it)
      if c ~= nil then
        local v = V.sub(c, ray.o)
        local along = V.dot(v, ray.d)
        if along > 0.05 and along < AIM_RANGE and (ui == nil or along < ui) then
          local off = math.max(0, V.len(V.sub(v, V.mul(ray.d, along))) - (it.entry.radius or 0.05) - RAY_RADIUS)
          local score = math.atan2(off, along)
          if score < AIM_CONE and (bestScore == nil or score < bestScore) then best, bestScore = i, score end
        end
      end
    end
  end
  return best
end

-- Item i flies toward the palm: a velocity command along the way, slowing near the hand, its acceleration bounded,
-- gravity paid for.
local function fly(hand, i, dt)
  local H, it = Hands.hands[hand], S.items[i]
  local c = centre(it)
  if c == nil or H.pos == nil then return end
  local way = V.sub(H.pos, c)
  local dist = V.len(way)
  if dist < 1e-3 then return end
  local speed = math.min(PULL_SPEED, dist * PULL_SLOW + 1.0)
  local want = V.add(V.add(V.mul(way, speed / dist), H.vel or { 0, 0, 0 }), { 0, 0, 9.81 * dt })
  pcall(function()
    it.body:SetIsKinematic(false)
    local v = VRBodyGetVel(it.body)
    if v.w < 0.5 then error("no velocity") end
    local dv = V.sub(want, { v.x, v.y, v.z })
    local l, cap = V.len(dv), PULL_ACCEL * dt
    if l > cap and l > 1e-4 then dv = V.mul(dv, cap / l) end
    local m = it.entry.mass
    it.body:AddLinearImpulse(Vector4.new(dv[1] * m, dv[2] * m, dv[3] * m, 0), true)
  end)
end

local function endPull(hand)
  local R = S.reach[hand]
  R.pull, R.pullFor, R.lock = nil, nil, nil
  Hands.release(hand, M)
end

-- A free hand this frame (after the grip was read and anything in reach taken): its pull flown, or its lock kept,
-- snatched or taken, and the marker on what it reaches for. `closed`: the grip closed this frame on nothing in reach.
local function reach(hand, dt, ui, closed, marker)
  local H, R = Hands.hands[hand], S.reach[hand]
  if R.pull ~= nil then
    local it = S.items[R.pull]
    R.pullFor = R.pullFor + dt
    if it == nil or not resolve(it) or not H.grip or H.pos == nil or R.pullFor > PULL_LONGEST then
      endPull(hand)
    else
      local d = surface(it, H.pos)
      if d ~= nil and d < REACH then
        local i = R.pull
        endPull(hand)
        hold(hand, i, false)
        return
      end
      fly(hand, R.pull, dt)
      if marker then marker.mark(hand, markOf(it, "pull", H)) end
      return
    end
  end
  if H.pos == nil or S.held[hand].item ~= nil or (H.owner ~= nil and H.owner ~= M) then
    R.aim, R.lock = nil, nil
    return
  end
  if not H.grip then R.lock = nil end
  if R.lock ~= nil and claimed(hand, R.lock) then R.lock = nil end
  -- Held from afar: the hand snatched back from the item pulls it.
  if R.lock ~= nil and H.cvel ~= nil then
    local c = centre(S.items[R.lock])
    local away = c ~= nil and V.sub(H.pos, c) or nil
    local l = away ~= nil and V.len(away) or 0
    local speed = l > 1e-3 and V.dot(H.cvel, V.mul(away, 1 / l)) or 0
    R.snatch = math.max(R.snatch or 0, speed)
    if speed >= FLICK then
      R.pull, R.pullFor = R.lock, 0
      S.pulls = S.pulls + 1
      Hands.claim(hand, M)
      haptic(hand)
      return
    end
  end
  -- In reach: what the grip would take now (grab's own test); else near: the item the ring shows faint on.
  local take = nearest(hand, REACH)
  local near = take or nearest(hand, HOVER)
  if near ~= nil and R.lock == nil then
    R.aim = nil
    if marker then marker.mark(hand, markOf(S.items[near], "near", H, take ~= nil)) end
    return
  end
  R.aim = R.lock or pointed(hand, ui)
  if closed and R.lock == nil and R.aim ~= nil then
    R.lock, R.snatch = R.aim, 0
    haptic(hand)
  end
  local it = S.items[R.aim or 0]
  if it ~= nil and marker then marker.mark(hand, markOf(it, R.lock ~= nil and "lock" or "aim", H)) end
end

---------------------------------------------------------------- the lobby's calls

-- The items go onto the table, in the lobby's frame.
function M.open(anchor, list)
  M.close()
  if anchor == nil or list == nil then return end
  S.anchor, S.list = anchor, list
  for _, entry in ipairs(list) do S.items[#S.items + 1] = spawnItem(entry) end
end

function M.close()
  for hand = 1, 2 do
    if S.held[hand].item ~= nil then release(hand) end
    Hands.release(hand, M)
  end
  Grips.close()
  Spawner.clear(TAG)
  S.items, S.anchor, S.list = {}, nil, nil
  S.held, S.giving, S.throws, S.reach = { {}, {} }, {}, {}, { {}, {} }
end

-- Everything back where it rests; a thing taken (an engram in the neck) stays taken.
function M.reset()
  for i = 1, #S.items do
    if not S.items[i].gone then putBack(i) end
  end
  S.resets = S.resets + 1
  play("click")
end

-- After modules/hands.lua's update, the same frame. ui: per hand, how far along its pointer a menu panel takes it
-- (modules/holo.lua M.pointing), or nil.
function M.update(dt, ui)
  if S.anchor == nil then return end
  dt = dt or 0
  if dt <= 0 or dt > 0.2 then return end
  local marker = reachMod()
  for hand = 1, 2 do
    local H, P = Hands.hands[hand], S.held[hand]
    if P.item ~= nil and (not H.grip or H.pos == nil) then release(hand) end
    -- A thing given to this hand: held as soon as it is in the world, if the hand is still closed.
    local i = S.giving[hand]
    if i ~= nil and P.item == nil and S.items[i] ~= nil and resolve(S.items[i]) then
      S.giving[hand] = nil
      if H.grip and H.pos ~= nil and (H.owner == nil or H.owner == M) then hold(hand, i) end
    end
    local closed = false
    if P.item == nil and Hands.taking(hand) then closed = not grab(hand) end
    if P.item ~= nil then carry(hand, dt) end
    if P.item == nil and S.giving[hand] == nil then reach(hand, dt, ui and ui[hand], closed, marker) end
  end
  Grips.update(dt)
  for n = #S.throws, 1, -1 do
    local T = S.throws[n]
    if T.it.body ~= nil then throwOn(T) end
    T.frames = T.frames - 1
    if T.frames <= 0 or T.it.body == nil then table.remove(S.throws, n) end
  end
  -- Fallen out of the place or thrown far away: back on the table (looked at twice a second).
  S.lostAt = (S.lostAt or 0) - dt
  if S.lostAt > 0 then return end
  S.lostAt = LOST_EVERY
  for i, it in ipairs(S.items) do
    if it.entity ~= nil and not it.gone then
      local ok, p = pcall(function() return V.v3(it.entity:GetWorldPosition()) end)
      if ok and (p[3] < S.anchor.pos.z - LOST_BELOW or V.len(V.sub(p, it.home)) > LOST_AWAY) then putBack(i) end
    end
  end
end

-- What a hand holds: the entry and where the thing is in the world, or nil.
function M.held(hand)
  local it = S.items[S.held[hand].item or 0]
  if it == nil or it.entity == nil then return nil end
  local ok, p = pcall(function() return V.v3(it.entity:GetWorldPosition()) end)
  if not ok then return nil end
  return it.entry, p
end

-- The thing in a hand taken from it: out of the world until it is put back.
function M.take(hand)
  local i = S.held[hand].item
  local it = S.items[i or 0]
  if it == nil then return nil end
  release(hand)
  despawnItem(it)
  it.gone = true
  return it.entry
end

-- A thing on the table taken out of the world without a hand (an engram that went in before the lobby came up again).
function M.takeEntry(entry)
  for _, it in ipairs(S.items) do
    if it.entry == entry and not it.gone then
      despawnItem(it)
      it.gone = true
      return true
    end
  end
  return false
end

-- A thing taken put back on the table, found by its entry.
function M.restore(entry)
  for i, it in ipairs(S.items) do
    if it.entry == entry and it.gone then putBack(i) end
  end
end

-- A thing taken put into a hand: spawned at the palm, held there once it is in the world.
function M.give(entry, hand)
  local H = Hands.hands[hand]
  if H.pos == nil then return false end
  for i, it in ipairs(S.items) do
    if it.entry == entry and it.gone then
      S.items[i] = spawnItem(entry, H.pos)
      S.giving[hand] = i
      Hands.claim(hand, M)
      return true
    end
  end
  return false
end

function M.state()
  local held = {}
  for hand = 1, 2 do
    local P = S.held[hand]
    held[hand] = P.item and S.items[P.item] and S.items[P.item].entry.name or false
  end
  local spawned = 0
  for _, it in ipairs(S.items) do if resolve(it) then spawned = spawned + 1 end end
  local reaching = {}
  for hand = 1, 2 do
    local R = S.reach[hand]
    local name = function(i) return i ~= nil and S.items[i] ~= nil and S.items[i].entry.name or false end
    reaching[hand] = { aim = name(R.aim), lock = name(R.lock), pull = name(R.pull), snatch = R.snatch or 0 }
  end
  return { items = #S.items, resolved = spawned, left = held[1], right = held[2],
           resets = S.resets, attached = { S.held[1].attached == true, S.held[2].attached == true },
           seat = { S.held[1].lp or false, S.held[2].lp or false }, throw = S.lastThrow, grips = Grips.state(),
           reach = reaching, pulls = S.pulls }
end


-- For tests: an item's entity, by its name in the scene.
function M.entity(name)
  for _, it in ipairs(S.items) do
    if it.entry.name == name and resolve(it) then return it.entity end
  end
  return nil
end

-- For tests: a hand as modules/hands.lua reads it, and its pointer ray (origin o, unit direction d, the world).
function M.hand(hand) return Hands.hands[hand] end
function M.ray(hand) return Hands.hands[hand].ray end

-- For tests: where each item is now, in the world, and how fast it moves.
function M.positions()
  local out = {}
  for _, it in ipairs(S.items) do
    if resolve(it) then
      local ok, p = pcall(function() return V.v3(it.entity:GetWorldPosition()) end)
      if ok then
        local e = { name = it.entry.name, x = p[1], y = p[2], z = p[3], speed = -1 }
        pcall(function()
          local v = VRBodyGetVel(it.body)
          if v.w > 0.5 then e.speed = V.len({ v.x, v.y, v.z }) end
          local b = VRBodyGetPos(it.body)
          if b.w > 0.5 then e.body = { b.x, b.y, b.z } end
        end)
        out[#out + 1] = e
      end
    end
  end
  return out
end

return M
