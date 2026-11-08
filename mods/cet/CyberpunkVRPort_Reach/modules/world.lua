-- What a hand reaches for in the game: the ring on the thing the game would pick up or use for that hand, and the
-- grip that does it.
--
-- THE HAND CHOOSES, THE GAME OFFERS. The game picks what the player interacts with by where the player looks (its
-- look-at ray). A free hand chooses instead: of the things the targeting system knows round the player
-- (GetTargetParts: loot on the ground, containers, devices), the one nearest its palm, or -- with nothing in reach
-- -- the one its pointer is on within a short pull of the palm (PULL_RANGE). Where the eyes look plays no part: a
-- thing is chosen by the hand pointing at it, never by being looked at. The game's look-at ray is turned onto that
-- thing from the eyes (VRReachAim, include/Natives/Reach.hpp), and the game does the rest the way it does for a look:
-- its own choice, prompt and highlight -- within its own reach (loot about 2 m from the player) and only for what the
-- eyes could see. The ring (modules/marker.lua) shows only once the game offers something for that thing -- its loot
-- prompt names it, or its choice hub is up while the thing is what it looks at -- so a ring always means the thing can
-- be taken. Of loot lying together the game keeps its prompt on one, whatever the look (a flat game's player gets the
-- same); a prompt kept on a thing beside the one chosen puts the ring on that one: it is what the grip takes.
--
-- THE GRIP TAKES IT. A grip closing on a thing in reach presses the game's interaction
-- button for a moment (VRReachUse): the game does what its button does there. A grip closing on a thing pointed at
-- holds it from where it lies (the ring becomes brackets); the hand snatched back toward the body pulls it: loot
-- flies to the palm on a low arc (the drop moved a frame at a time, Game.VREntitySetWorldTransform) and the game takes
-- it as it arrives; a container or a device is used where it stands. Loot the game will not take after all goes back
-- where it lay. Nothing is used that the game would not let a look use.
--
-- A WEAPON ENDS IN THE HAND. Taken by the hand the game draws weapons into (the left one in the left-handed mode), a
-- weapon flies into that hand's hold slot and turns on the way, so it arrives as the hand holds it; the game takes it
-- there and draws it at once (its own take-and-equip, DrawItemRequest), into the pose it already has. Grabbed in reach
-- it is drawn the same way. The other hand takes it like any loot.
--
-- NOT WHILE: the hand holds something (an item in its slot, or what the port put in it: a reload's magazine, the
-- support hand on the gun -- VRHandHolds; the other, free hand still reaches), the player sits in a vehicle, carries
-- a body or grapples, the scanner is up, a menu or the VR menu owns the controls, or the lobby is up (it marks its
-- own things).
local M = {}

local NEAR = 0.32            -- a thing whose point is this near the palm is in reach (m)
local PULL_RANGE = 1.6       -- how far from the palm a thing pointed at can be pulled (m)
local POINT_CONE = math.rad(7)
local RAY_RADIUS = 0.06      -- a thing this near the palm's ray is pointed at too, however near the hand (m)
-- The palm the hand points with, in the hand bone's frame (1 left, 2 right; tools/grasp_tests prints them from the rig):
-- the middle of its skin, and its normal -- out of the palm, the side the fingers close to.
local PALM_AT = { { 0.0396, 0.0039, -0.0124 }, { -0.0396, -0.0039, 0.0124 } }
local PALM_OUT = { { 0.0775, -0.0077, -0.9970 }, { -0.0775, 0.0077, 0.9970 } }
local GRIP_ON, GRIP_OFF = 0.65, 0.35
local LIST_EVERY = 0.25      -- s between refreshes of the things round the player
local CONFIRM_WITHIN = 0.35  -- s the game has to offer something for a chosen thing before the next is tried
local SNUB = 1.2             -- s a thing the game offered nothing for is passed over
local BESIDE = 0.6           -- m from the chosen thing within which the game's prompt on another thing is taken
local USE_FOR = 0.12         -- s the interaction button is held by one grip
-- The pull: the palm moving away from the held thing this fast (m/s, smoothed over VEL_SMOOTH s) snatches it; it flies
-- at about FLIGHT_SPEED, between FLIGHT_MIN and FLIGHT_MAX s, rising ARC m over its middle; the game is asked to take
-- it from TAKE_AT of the way on; one not taken GIVE_UP s after it landed goes back.
local FLICK, VEL_SMOOTH = 1.1, 0.06
local FLIGHT_SPEED, FLIGHT_MIN, FLIGHT_MAX, ARC = 4.0, 0.18, 0.45, 0.12
local TAKE_AT, GIVE_UP = 0.8, 0.5
-- A weapon for the hand is turned into the hold over this part of its flight, and taken as it lands; the game has
-- DRAW_WITHIN s to put it in the inventory before its draw is given up.
local ALIGN_BY, DRAW_WITHIN = 0.85, 1.0

local S = { time = 0, things = {}, byId = {}, listAt = -1, hands = { {}, {} }, target = nil, snubbed = {},
            uses = 0, pulls = 0, draws = 0, lastAim = false, flight = nil, draw = nil }

---------------------------------------------------------------- vectors

local function v3(v) return { v.x, v.y, v.z } end
local function add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
local function sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
local function dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
local function len(a) return math.sqrt(dot(a, a)) end
local function qrot(q, v)
  local x, y, z = v[1], v[2], v[3]
  local tx, ty, tz = 2 * (q.j * z - q.k * y), 2 * (q.k * x - q.i * z), 2 * (q.i * y - q.j * x)
  return { x + q.r * tx + (q.j * tz - q.k * ty), y + q.r * ty + (q.k * tx - q.i * tz),
           z + q.r * tz + (q.i * ty - q.j * tx) }
end
local function slerp(a, b, t)
  local d = a.i * b.i + a.j * b.j + a.k * b.k + a.r * b.r
  local sign = d < 0 and -1 or 1
  d = math.abs(d)
  local wa, wb = 1 - t, t
  if d < 0.9995 then
    local th = math.acos(d)
    local sn = math.sin(th)
    wa, wb = math.sin((1 - t) * th) / sn, math.sin(t * th) / sn
  end
  wb = wb * sign
  local i, j, k, r = a.i * wa + b.i * wb, a.j * wa + b.j * wb, a.k * wa + b.k * wb, a.r * wa + b.r * wb
  local n = math.sqrt(i * i + j * j + k * k + r * r)
  return Quaternion.new(i / n, j / n, k / n, r / n)
end

---------------------------------------------------------------- the game's state

-- A hand with something in it takes nothing: an item in its weapon slot (a gun, a blade, a grenade, a cigarette; bare
-- fists and Gorilla Arms are a free hand), or what the port put there (a reload's magazine, the support hand on the
-- gun -- VRHandHolds).
local slots = nil
local function holds(pl, hand)
  if type(VRHandHolds) == "function" then
    local ok, yes = pcall(VRHandHolds, hand - 1)
    if ok and yes then return true end
  end
  slots = slots or { TweakDBID.new("AttachmentSlots.WeaponLeft"), TweakDBID.new("AttachmentSlots.WeaponRight") }
  local item = nil
  pcall(function() item = Game.GetTransactionSystem():GetItemInSlot(pl, slots[hand]) end)
  if item == nil then return false end
  local free = false
  pcall(function()
    local id = item:GetItemID()
    free = WeaponObject.IsOfType(id, gamedataItemType.Wea_Fists) or WeaponObject.IsOfType(id, gamedataItemType.Cyb_StrongArms)
  end)
  return not free
end

-- The hand the game draws a weapon into: the left one in the left-handed mode.
local function weaponHand()
  return (type(VRLeftHanded) == "function" and VRLeftHanded() == true) and 1 or 2
end

-- Where a weapon sits in hand `hand`, in the world: that hand's hold slot of the player's ItemAttachmentSlots, which a
-- held weapon's root is exactly on.
local HOLD = { "WeaponLeft", "WeaponRight" }
local function holdSlot(pl, hand)
  local owner = nil
  pcall(function() owner = tostring(pl:GetEntityID().hash) end)
  if owner ~= S.slotOwner then S.slotComp, S.slotOwner = nil, owner end
  if S.slotComp == nil then
    pcall(function()
      local cs = Game.VREntityComponents(pl)
      for i = 1, #cs do
        if tostring(cs[i]:GetName()):find("ItemAttachmentSlots") then S.slotComp = cs[i]; break end
      end
    end)
    if S.slotComp == nil then return nil end
  end
  local ok, p, q = pcall(function()
    local found, tr = S.slotComp:GetSlotTransform(CName.new(HOLD[hand]))
    if not found then return nil end
    return v3(WorldPosition.ToVector4(tr.Position)), tr.Orientation
  end)
  if not ok then S.slotComp = nil end
  if not ok or p == nil then return nil end
  return p, q
end

local bbDefs = nil
-- The three state ids, read once (blackboard definitions live as long as the game): every frame each read of
-- `bbDefs.PlayerStateMachine.X` was a trip through the bindings, as the closure for pcall was an allocation.
local VISION, CARRYING, TAKEDOWN = nil, nil, nil
local function blockedNow(pl)
  if VRLobbyActive ~= nil and VRLobbyActive() then return true end
  if Game.GetSystemRequestsHandler():IsGamePaused() then return true end
  if Game.GetMountedVehicle(pl) ~= nil then return true end
  if VISION == nil then
    bbDefs = bbDefs or GetAllBlackboardDefs()
    local d = bbDefs.PlayerStateMachine
    VISION, CARRYING, TAKEDOWN = d.Vision, d.Carrying, d.Takedown
  end
  local psm = pl:GetPlayerStateMachineBlackboard()
  -- The scanner up, a body carried, a grapple or a takedown: both hands are taken.
  return psm:GetInt(VISION) ~= 0 or psm:GetBool(CARRYING) or psm:GetInt(TAKEDOWN) ~= 0
end
local function blocked(pl)
  local ok, no = pcall(blockedNow, pl)
  return ok and no == true
end

-- What the game offers now: the owner of its loot prompt, and whether a choice hub is up.
local function offers()
  local owner, hub = nil, false
  pcall(function()
    bbDefs = bbDefs or GetAllBlackboardDefs()
    local d = bbDefs.UIInteractions
    local bb = Game.GetBlackboardSystem():Get(d)
    local loot = FromVariant(bb:GetVariant(d.LootData))
    if loot ~= nil and loot.isActive then owner = tostring(loot.ownerId.hash) end
    local h = FromVariant(bb:GetVariant(d.InteractionChoiceHub))
    hub = h ~= nil and h.active == true and #h.choices > 0
  end)
  return owner, hub
end

---------------------------------------------------------------- the things round the player

-- Loot, containers and devices: what a hand can take or use. People, vehicles and the player are not.
local function usable(e)
  local yes = false
  pcall(function()
    yes = e:IsA("gameItemDropObject") or e:IsA("gameItemObject") or e:IsA("gameLootContainerBase") or e:IsA("Device")
  end)
  return yes
end

local function refresh(pl)
  local list, byId = {}, {}
  pcall(function()
    local q = TSQ_ALL()
    q.maxDistance = PULL_RANGE + 1.0     -- from the player: the palm is up to an arm's length off
    q.testedSet = TargetingSet.Complete
    q.ignoreInstigator = true
    local ok, parts = Game.GetTargetingSystem():GetTargetParts(pl, q)
    if not ok or parts == nil then return end
    local seen = {}
    for _, part in ipairs(parts) do
      local c = part:GetComponent()
      local e = c and c:GetEntity()
      if e ~= nil then
        -- A string: the 64-bit hash itself is a cdata, a table key only by its identity.
        local id = tostring(e:GetEntityID().hash)
        if not seen[id] and usable(e) then
          seen[id] = true
          list[#list + 1] = { id = id, entity = e, item = e:IsA("gameItemDropObject") or e:IsA("gameItemObject") }
          byId[id] = list[#list]
        end
      end
    end
  end)
  S.things, S.byId = list, byId
  for id, till in pairs(S.snubbed) do
    if till < S.time then S.snubbed[id] = nil end
  end
end

-- Where a thing is this frame: its origin, nudged up a little (a drop's origin is its bottom). Little: a drop lies
-- flat, a weapon on the ground is some 7 cm tall, and the look-at ray turned onto a point above it misses it.
local function where(t)
  local ok, p = pcall(function() return v3(t.entity:GetWorldPosition()) end)
  if not ok then return nil end
  return { p[1], p[2], p[3] + (t.item and 0.02 or 0.0) }
end

---------------------------------------------------------------- the hands

local AXES = nil
local function hands(pl, dt)
  local hits = nil
  local wp = pl:GetWorldPosition()
  if type(VRMenuPointer) == "function" then
    AXES = AXES or { Vector4.new(1, 0, 0, 0), Vector4.new(0, 0, 1, 0), Vector4.new(1, 1, 0, 0) }
    local ok, h = pcall(VRMenuPointer, Vector4.new(wp.x, wp.y, wp.z, 1), AXES[1], AXES[2], AXES[3])
    if ok and h ~= nil and #h >= 8 then hits = h end
  end
  local pp, pq = v3(wp), pl:GetWorldOrientation()
  for hand = 1, 2 do
    local H = S.hands[hand]
    H.pos, H.ray, H.palm = nil, nil, nil
    local ok, m = pcall(VRPalmModelPos, hand - 1)
    if ok and m ~= nil and m.w > 0.5 then H.pos = add(pp, qrot(pq, { m.x, m.y, m.z })) end
    -- The ray things are pointed at with: out of the middle of the palm.
    local okr, r = pcall(VRPalmModelRot, hand - 1)
    if H.pos ~= nil and okr and r ~= nil then
      H.palm = { o = add(H.pos, qrot(pq, qrot(r, PALM_AT[hand]))), d = qrot(pq, qrot(r, PALM_OUT[hand])) }
    end
    local grip = 0
    local was = H.rel
    H.rel = nil
    if hits ~= nil then
      local o, e = hits[4 * hand - 1], hits[4 * hand]
      grip = e.w
      local d = { e.x - o.x, e.y - o.y, e.z - o.z }
      local l = len(d)
      if l > 1e-3 then H.ray = { o = { o.x, o.y, o.z }, d = mul(d, 1 / l) } end
      H.rel = sub({ o.x, o.y, o.z }, pp)
    end
    -- The controller's velocity against the body, smoothed: what a snatch is read from. The controller's own, from the
    -- pointer's origin (the tracked wrist): the drawn palm follows it through the body's IK and lags a quick move.
    if H.rel ~= nil and was ~= nil and dt > 1e-4 then
      local v = mul(sub(H.rel, was), 1 / dt)
      local k = math.min(1, dt / VEL_SMOOTH)
      H.vel = H.vel and add(H.vel, mul(sub(v, H.vel), k)) or v
    else
      H.vel = nil
    end
    H.gripPrev = H.grip
    H.grip = grip >= GRIP_ON or (H.grip == true and grip > GRIP_OFF)
    H.busy = holds(pl, hand)
  end
end

---------------------------------------------------------------- choosing

-- The thing a hand reaches for: nearest the palm within NEAR, else the one its palm points at within PULL_RANGE of
-- the palm -- off the palm's ray by no more than RAY_RADIUS or POINT_CONE, whichever is wider there; how ("near",
-- "aim"), and how near (m) or how far off the ray beyond that radius (rad).
local function choose(H)
  if H.pos == nil or H.busy then return nil end
  local best, bestD = nil, nil
  for _, t in ipairs(S.things) do
    if t.pos ~= nil and (S.snubbed[t.id] or -1) < S.time then
      local d = len(sub(t.pos, H.pos))
      if d < NEAR and (bestD == nil or d < bestD) then best, bestD = t, d end
    end
  end
  if best ~= nil then return best, "near", bestD end
  if H.palm == nil then return nil end
  local bestA = nil
  for _, t in ipairs(S.things) do
    if t.pos ~= nil and (S.snubbed[t.id] or -1) < S.time and len(sub(t.pos, H.pos)) <= PULL_RANGE then
      local v = sub(t.pos, H.palm.o)
      local along = dot(v, H.palm.d)
      if along > 0.1 then
        local a = math.atan2(math.max(0, len(sub(v, mul(H.palm.d, along))) - RAY_RADIUS), along)
        if a < POINT_CONE and (bestA == nil or a < bestA) then best, bestA = t, a end
      end
    end
  end
  if best ~= nil then return best, "aim", bestA end
  return nil
end

local function aim(p)
  if type(VRReachAim) ~= "function" then return end
  if p ~= nil then
    pcall(VRReachAim, Vector4.new(p[1], p[2], p[3], 1))
    S.lastAim = true
  elseif S.lastAim then
    pcall(VRReachAim, Vector4.new(0, 0, 0, 0))
    S.lastAim = false
  end
end

local function use()
  if type(VRReachUse) ~= "function" then return end
  pcall(VRReachUse, USE_FOR)
  S.uses = S.uses + 1
end

-- The weapon a thing on the ground is (its ItemID), or nil.
local function weaponIn(thing)
  local id = nil
  pcall(function()
    local e = thing.entity
    local io = e:IsA("gameItemDropObject") and e:GetItemObject() or e
    if io ~= nil and io:IsA("gameweaponObject") then id = io:GetItemID() end
  end)
  return id
end

-- What hand `hand` takes: the game's interaction button pressed on it, and a weapon taken by the weapon hand drawn
-- into it once the game has it in the inventory (drawTaken).
local function take(hand, thing)
  use()
  local weapon = hand == weaponHand() and weaponIn(thing) or nil
  if weapon ~= nil then S.draw = { id = weapon, till = S.time + DRAW_WITHIN } end
end

-- The game's take-and-equip (player.swift OnItemAddedEvent: a weapon tagged TakeAndEquip): a DrawItemRequest, made
-- at once -- the weapon is in the hand already.
local function drawTaken(pl)
  local D = S.draw
  if D == nil then return end
  local has = false
  pcall(function() has = Game.GetTransactionSystem():HasItem(pl, D.id) end)
  if has then
    S.draw = nil
    local ok = pcall(function()
      local r = DrawItemRequest.new()
      r.owner = pl
      r.itemID = D.id
      r.equipAnimationType = gameEquipAnimationType.Instant
      Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")):QueueRequest(r)
    end)
    if ok then S.draws = S.draws + 1 end
  elseif S.time > D.till then
    S.draw = nil
  end
end

---------------------------------------------------------------- the pull

local function alive(e)
  local ok, yes = pcall(function() return IsDefined(e) and e:IsAttached() end)
  return ok and yes == true
end

local function place(e, p, q)
  pcall(function()
    local tr = WorldTransform.new()
    tr:SetPosition(Vector4.new(p[1], p[2], p[3], 1.0))
    tr:SetOrientation(q)
    Game.VREntitySetWorldTransform(e, tr)
  end)
end

-- Loot snatched by hand `hand`: it leaves from where it lies toward the palm -- a weapon for that hand toward its hold.
local function pull(hand, thing)
  local H = S.hands[hand]
  local from = where(thing)
  local ok, p, q = pcall(function() return thing.entity:GetWorldPosition(), thing.entity:GetWorldOrientation() end)
  if from == nil or H.pos == nil or not ok then return false end
  local d = len(sub(H.pos, from))
  S.flight = { hand = hand, thing = thing, start = { p.x, p.y, p.z }, lift = from[3] - p.z, q = q, since = S.time,
               duration = math.max(FLIGHT_MIN, math.min(FLIGHT_MAX, d / FLIGHT_SPEED)), asked = false, landed = nil,
               weapon = hand == weaponHand() and weaponIn(thing) ~= nil }
  S.pulls = S.pulls + 1
  return true
end

-- A frame of the flight: the thing on its arc to the palm (where the palm is now), the game asked to take it near
-- the end, and put back where it lay if it is still there GIVE_UP after landing. A weapon for the hand flies to its
-- hold (where the hold is now) turning from how it lay to how the hand holds it, and is taken once it is there.
local function fly(pl, marker)
  local F = S.flight
  local H = S.hands[F.hand]
  local e = F.thing.entity
  if not alive(e) then S.flight = nil; return end
  local s = math.min(1, (S.time - F.since) / F.duration)
  local k = s * s * (3 - 2 * s)
  local to, q, takeAt = nil, F.q, TAKE_AT
  if F.weapon then
    local hp, hq = holdSlot(pl, F.hand)
    if hp ~= nil then
      local r = math.min(1, s / ALIGN_BY)
      to, q, takeAt = hp, slerp(F.q, hq, r * r * (3 - 2 * r)), 1
    end
  end
  to = to or (H.pos and sub(H.pos, { 0, 0, F.lift })) or F.start
  local p = add(add(F.start, mul(sub(to, F.start), k)), { 0, 0, ARC * 4 * k * (1 - k) })
  place(e, p, q)
  local at = { p[1], p[2], p[3] + F.lift }
  aim(at)
  if s >= 1 and F.landed == nil then F.landed = S.time end
  if s >= takeAt and not F.asked then
    local owner = offers()
    if owner == F.thing.id then
      take(F.hand, F.thing)
      F.asked = true
    end
  end
  if F.landed ~= nil and S.time - F.landed > GIVE_UP then
    place(e, F.start, F.q)
    S.snubbed[F.thing.id] = S.time + SNUB
    S.flight = nil
    return
  end
  if marker ~= nil then
    marker.mark(F.hand, { pos = at, radius = 0.05, lift = 0.06, kind = "lock", ready = true, from = H.pos })
  end
end

---------------------------------------------------------------- the frame

function M.update(dt, marker)
  dt = dt or 0
  S.time = S.time + dt
  if S.held ~= nil then
    if S.time < S.held.till then aim(S.held.point); return end
    S.held = nil
  end
  local pl = Game.GetPlayer()
  if pl ~= nil then drawTaken(pl) end
  if pl == nil or type(VRPalmModelPos) ~= "function" or blocked(pl) then
    aim(nil)
    S.target = nil
    return
  end
  if S.time - S.listAt >= LIST_EVERY then
    refresh(pl)
    S.listAt = S.time
  end
  for _, t in ipairs(S.things) do t.pos = where(t) end
  hands(pl, dt)
  if S.flight ~= nil then
    fly(pl, marker)
    return
  end
  -- A hold keeps its hand and its thing while the grip is closed. Otherwise each free hand (its grip not already
  -- closed on something else) chooses: a thing in reach before one pointed at, the nearer, the truer pointed.
  local T = S.target
  local hand, thing, how = nil, nil, nil
  if T ~= nil and T.locked and S.hands[T.hand].grip and not S.hands[T.hand].busy then
    hand, thing, how = T.hand, T.thing, "lock"
    thing.pos = thing.pos or where(thing)
  else
    local score = nil
    for h = 1, 2 do
      local H = S.hands[h]
      if not (H.grip and H.gripPrev) then
        local t, kind, by = choose(H)
        if t ~= nil then
          local sc = (kind == "near" and 0 or 10) + by
          if score == nil or sc < score then hand, thing, how, score = h, t, kind, sc end
        end
      end
    end
  end
  if thing == nil then
    S.target = nil
    aim(nil)
    return
  end
  local H = S.hands[hand]
  if T == nil or T.thing.id ~= thing.id or T.hand ~= hand then
    T = { hand = hand, thing = thing, since = S.time, confirmed = false, locked = false }
    S.target = T
  end
  T.how = how
  aim(thing.pos)
  -- The game's word on it, a frame after the ray is on it.
  local owner, hub = offers()
  local looked = nil
  pcall(function()
    local o = Game.GetTargetingSystem():GetLookAtObject(pl, false, false)
    if o ~= nil then looked = tostring(o:GetEntityID().hash) end
  end)
  -- What the grip would take: this thing, or the one beside it the game keeps its loot prompt on.
  local offered = nil
  if owner == thing.id or (hub and looked == thing.id) then
    offered = thing
  elseif owner ~= nil then
    local other = S.byId[owner]
    if other ~= nil and other.pos ~= nil and len(sub(other.pos, thing.pos)) <= BESIDE then offered = other end
  end
  T.offered = offered
  T.confirmed = offered ~= nil
  if not T.confirmed then
    if S.time - T.since > CONFIRM_WITHIN then
      S.snubbed[thing.id] = S.time + SNUB
      S.target = nil
    end
    return
  end
  if how == "near" then
    -- In reach: the grip closing on it takes it.
    if H.grip and not H.gripPrev then take(hand, offered) end
  else
    -- Pointed at: the grip closing on it holds it from afar; held, the palm snatched away from it pulls loot to the
    -- hand and uses a container or a device where it stands.
    if H.grip and not H.gripPrev then T.locked = true end
    if not H.grip then T.locked = false end
    if T.locked and H.vel ~= nil and H.pos ~= nil then
      local away = sub(H.pos, offered.pos)
      local l = len(away)
      local speed = l > 1e-3 and dot(H.vel, mul(away, 1 / l)) or 0
      T.snatch = math.max(T.snatch or 0, speed)
      if speed >= FLICK then
        T.locked = false
        S.target = nil
        if not (offered.item and pull(hand, offered)) then take(hand, offered) end
        return
      end
    end
  end
  if marker ~= nil then
    local kind = how == "near" and "near" or (T.locked and "lock" or "aim")
    marker.mark(hand, { pos = offered.pos, radius = offered.item and 0.05 or 0.07,
                        lift = offered.item and 0.06 or 0.04, kind = kind, ready = true, from = H.pos })
  end
end

function M.state()
  local T = S.target
  local F = S.flight
  return { things = #S.things, uses = S.uses, pulls = S.pulls, draws = S.draws, drawing = S.draw ~= nil,
           target = T and { hand = T.hand, id = T.thing.id, how = T.how, confirmed = T.confirmed, locked = T.locked,
                            offered = T.offered and T.offered.id or false, snatch = T.snatch or 0 } or false,
           flight = F and { hand = F.hand, id = F.thing.id, asked = F.asked, landed = F.landed ~= nil } or false,
           hands = { { busy = S.hands[1].busy, grip = S.hands[1].grip },
                     { busy = S.hands[2].busy, grip = S.hands[2].grip } } }
end

function M.debug() return S end

-- Test: the game's look-at ray kept on a point of the world (X, Y, Z) for that many seconds, no hand choosing.
function M.hold(point, seconds)
  S.held = { point = point, till = S.time + (seconds or 1) }
  S.target = nil
end

return M
