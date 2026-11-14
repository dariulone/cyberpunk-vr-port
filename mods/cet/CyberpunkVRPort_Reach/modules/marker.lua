-- A hand's reach marker: a ring in the world on what that hand can take or use -- a thin white ring open on one side
-- on the grip point of what a hand points at, and four corner brackets on what the held trigger has locked.
--
-- ITS SIZE: 2.5 cm, whatever the thing and however far it is (tools/vr_lobby_assets/
-- make_reach_marker.py has the measures).
--
-- WHAT IT SHOWS, per hand (1 left, 2 right), as the caller says every frame (M.mark):
--   near   what the hand would take or use were the grip closed now: the ring, its opening toward the hand; `ready`
--          false while the hand is near but not yet in reach (the ring fainter).
--   aim    what the hand points at from afar: the ring.
--   lock   what the held trigger has locked: the ring gives way to the four brackets, snapping in.
--   pull   what flies to the hand: the brackets ride on it.
-- A hand not marked in a frame lets its marker fade out.
--
-- WHAT IT IS: geometry, a sprite in the world (vrport\ui\vr_marker.ent: a mesh component a part -- the ring, and the
-- brackets at each step of their snap -- on the game's own world-marker material, additive), turned to the eyes every
-- frame with the ring's opening toward the hand, and put a little before the thing. A part's fade is a step through
-- its appearances (l0 nothing .. LEVELS the full colour): its colour is its material's, which no component can
-- change.
-- It was a world widget, and a world widget's texture is the engine's, let go of while the plane is out of view and
-- bound again only once it is seen: the frames between drew an opaque black square, with every first appearance and
-- every time the marker came back into view.
--
-- BOTH ARE MADE ONCE THE GAME IS IN PLAY and kept while its world lasts, so the first thing a hand marks finds its
-- marker there. Not before a game and not while it is paused (a loading screen is paused too): the main menu and its
-- lobby make a hand's marker the first time that hand asks for one. The world-widget markers made in the main menu
-- while the game's own mod-check popup was up took its confirm -- the launch measured on 2026-10-29 pressed A eight
-- times without reaching it, and the same launch with them made on demand confirmed on the first press -- and nothing
-- made before a game is needed before one.
local M = {}

local ENT = "vrport\\ui\\vr_marker.ent"
-- make_reach_marker.py: the parts, and the brackets' scale at each step of their snap (BRACKET_SCALES)
local RING = "ring"
local BRACKETS = { "brackets1", "brackets2", "brackets3", "brackets4" }
local SNAP = { 1.35, 1.24, 1.12, 1.0 }
local LEVELS = 8                         -- every part's appearances: l0 (nothing) .. l8
local RESPAWN = 5.0                      -- s a spawn may take to land before it is made again
local LITTER = 30.0                      -- s a given-up spawn is still looked for, to be despawned if it lands
local ASK = 0.5                          -- s between two looks for the game being in play, while a marker is missing
local IN, OUT, MORPH = 14.0, 7.0, 11.0   -- 1/s: fading in, out, the ring giving way to the brackets and back

-- hands[h]: id, entity, comps (part -> component), level (part -> appearance level shown), shown (opacity now),
-- morph, at and eye (the last mark and the eyes it was seen from), want (this frame's mark). litter: spawns given up
-- before they landed.
local S = { time = 0, hands = { {}, {} }, litter = {} }

---------------------------------------------------------------- vectors

local function add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
local function sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
local function dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
local function cross(a, b) return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] } end
local function norm(v)
  local l = math.sqrt(dot(v, v))
  return l > 1e-6 and mul(v, 1 / l) or nil
end
local function approach(x, target, rate, dt) return x + (target - x) * math.min(1.0, rate * dt) end

-- The rotation whose columns are the world images of the local X, Y and Z axes.
local function quatFromBasis(X, Y, Z)
  local m00, m01, m02 = X[1], Y[1], Z[1]
  local m10, m11, m12 = X[2], Y[2], Z[2]
  local m20, m21, m22 = X[3], Y[3], Z[3]
  local tr = m00 + m11 + m22
  local qx, qy, qz, qw
  if tr > 0 then
    local s = math.sqrt(tr + 1.0) * 2
    qw, qx, qy, qz = 0.25 * s, (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s
  elseif m00 > m11 and m00 > m22 then
    local s = math.sqrt(1.0 + m00 - m11 - m22) * 2
    qw, qx, qy, qz = (m21 - m12) / s, 0.25 * s, (m01 + m10) / s, (m02 + m20) / s
  elseif m11 > m22 then
    local s = math.sqrt(1.0 + m11 - m00 - m22) * 2
    qw, qx, qy, qz = (m02 - m20) / s, (m01 + m10) / s, 0.25 * s, (m12 + m21) / s
  else
    local s = math.sqrt(1.0 + m22 - m00 - m11) * 2
    qw, qx, qy, qz = (m10 - m01) / s, (m02 + m20) / s, (m12 + m21) / s, 0.25 * s
  end
  return Quaternion.new(qx, qy, qz, qw)
end

-- The marker at C, seen from the eyes along N (toward them): the mesh is flat in its X-Z plane facing -Y, with the
-- ring's opening along +X -- so X is the opening's way (R turned by `turn` toward U), -Y the eyes', Z up across it.
local function transformFor(C, R, U, N, turn)
  local c, s = math.cos(turn), math.sin(turn)
  local X = add(mul(R, c), mul(U, s))
  local Z = sub(mul(U, c), mul(R, s))
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(C[1], C[2], C[3], 1.0))
  tr:SetOrientation(quatFromBasis(X, mul(N, -1), Z))
  return tr
end

-- Where the eyes are: the rendered view (VRViewWorldPos), the game's camera when the plugin has none; nil with no
-- world to look at.
local function eyes()
  local p = nil
  if type(VRViewWorldPos) == "function" then
    pcall(function()
      local wp = VRViewWorldPos()
      if wp ~= nil and wp.w ~= nil and wp.w > 0.5 then p = { wp.x, wp.y, wp.z } end
    end)
  end
  if p == nil then
    pcall(function()
      local cam = Transform.new()
      Game.GetCameraSystem():GetActiveCameraWorldTransform(cam)
      p = { cam.position.x, cam.position.y, cam.position.z }
    end)
  end
  return p
end

-- In play: a game (not the main menu's), the player there, and not paused.
local function inPlay()
  local ok, yes = pcall(function()
    local r = Game.GetSystemRequestsHandler()
    return not r:IsPreGame() and not r:IsGamePaused() and Game.GetPlayer() ~= nil
  end)
  return ok and yes == true
end

---------------------------------------------------------------- the entities

local function reset(h)
  h.id, h.entity, h.comps, h.level, h.at, h.eye = nil, nil, nil, nil, nil, nil
end

local function spawn(h, p)
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(p[1], p[2], p[3], 1.0))
  reset(h)
  pcall(function() h.id = exEntitySpawner.Spawn(ENT, tr, "") end)
  h.spawnedAt = S.time
end

-- Despawned now if it is there, or as soon as it lands.
local function despawnId(id)
  local gone = false
  pcall(function()
    local e = Game.FindEntityByID(id)
    if e ~= nil then
      exEntitySpawner.Despawn(e)
      gone = true
    end
  end)
  return gone
end

local function despawn(h)
  if h.id ~= nil and not despawnId(h.id) then S.litter[#S.litter + 1] = { id = h.id, at = S.time } end
  reset(h)
end

local function sweep()
  for i = #S.litter, 1, -1 do
    local l = S.litter[i]
    if despawnId(l.id) or S.time - l.at > LITTER then table.remove(S.litter, i) end
  end
end

-- The entity and its parts once the spawn has landed; gone again with the world it was in.
local function resolve(h)
  if h.id == nil then return false end
  if h.comps ~= nil then
    local alive = false
    pcall(function() alive = Game.FindEntityByID(h.id) ~= nil end)
    if alive then return true end
    reset(h)
    S.play = false                         -- its world is gone: in play again only once asked again
    return false
  end
  pcall(function()
    local e = Game.FindEntityByID(h.id)
    if e == nil then return end
    local comps = { [RING] = e:FindComponentByName(CName.new(RING)) }
    for i = 1, #BRACKETS do comps[BRACKETS[i]] = e:FindComponentByName(CName.new(BRACKETS[i])) end
    for _, c in pairs(comps) do if c == nil then return end end
    h.entity, h.comps, h.level = e, comps, {}
  end)
  return h.comps ~= nil
end

-- A part at a level (0: nothing), written on a change only.
local function show(h, part, level)
  if h.level[part] == level then return end
  local c = h.comps[part]
  if pcall(function() Game.VRComponentChangeAppearance(c, CName.new("l" .. level), true) end) then h.level[part] = level end
end

local function level(v) return math.max(0, math.min(LEVELS, math.floor(LEVELS * v + 0.5))) end

---------------------------------------------------------------- the calls

-- Marks what a hand reaches for, this frame. t: pos (the thing's centre, the world), radius (its size, m: how far
-- before its centre the ring hangs when `lift` does not say), kind ("near", "aim", "lock", "pull"), ready (near: in
-- reach), from (the hand, the world: where the ring opens to), lift (how far before the centre toward the eyes).
function M.eyes() return eyes() end

function M.mark(hand, t)
  local h = S.hands[hand]
  if h ~= nil and t ~= nil and t.pos ~= nil then h.want = t end
end

function M.update(dt)
  dt = dt or 0
  S.time = S.time + dt
  if #S.litter > 0 then sweep() end
  local eye = nil
  for hand = 1, 2 do
    if S.pinned ~= nil and S.pinned[hand] ~= nil then S.hands[hand].want = S.pinned[hand] end
    local h, t = S.hands[hand], S.hands[hand].want
    S.hands[hand].want = nil
    if h.id == nil then
      if t == nil and S.time >= (S.nextAsk or 0) then
        S.nextAsk = S.time + ASK
        S.play = inPlay()
      end
      if t ~= nil or S.play then
        eye = eye or eyes()
        if eye ~= nil then spawn(h, eye) end
      end
    elseif h.comps == nil and S.time - (h.spawnedAt or S.time) > RESPAWN then
      despawn(h)                         -- a spawn that never landed (a world going away under it)
    end
    if resolve(h) then
      local lock = (t ~= nil and (t.kind == "lock" or t.kind == "pull")) and 1.0 or 0.0
      local goal = t == nil and 0.0 or ((t.kind == "near" and t.ready == false) and 0.45 or 1.0)
      h.shown = approach(h.shown or 0, goal, goal > (h.shown or 0) and IN or OUT, dt)
      h.morph = approach(h.morph or 0, lock, MORPH, dt)
      if t ~= nil then
        eye = eye or eyes()
        h.at, h.eye = t, eye
      end
      local a = h.at
      if a ~= nil and h.eye ~= nil then
        -- Turned to the eyes, before the thing by its lift, the opening toward the hand.
        local N = norm(sub(h.eye, a.pos))
        if N ~= nil then
          local U = norm(sub({ 0, 0, 1 }, mul(N, N[3]))) or norm(cross(N, { 1, 0, 0 }))
          local R = cross(U, N)
          local turn = h.lastTurn or 0
          if a.from ~= nil then
            local d = sub(a.from, a.pos)
            local x, y = dot(d, R), dot(d, U)
            if x * x + y * y > 1e-6 then turn = math.atan2(y, x) end
          end
          h.lastTurn = turn
          -- The brackets stand square to the eyes: the turn gives way as they come.
          local C = add(a.pos, mul(N, a.lift or (a.radius or 0.05)))
          pcall(function() Game.VREntitySetWorldTransform(h.entity, transformFor(C, R, U, N, turn * (1.0 - h.morph))) end)
        end
      end
      -- The ring fades as the brackets come; the brackets snap in through their steps.
      local m = h.morph
      local snap, best = 1, math.huge
      for i = 1, #SNAP do
        local d = math.abs(SNAP[i] - (1.35 - 0.35 * m))
        if d < best then snap, best = i, d end
      end
      show(h, RING, level(h.shown * (1.0 - m)))
      local bl = level(h.shown * m)
      for i = 1, #BRACKETS do show(h, BRACKETS[i], i == snap and bl or 0) end
    end
  end
end

-- Both markers out of the world.
function M.clear()
  for hand = 1, 2 do
    despawn(S.hands[hand])
    S.hands[hand] = {}
  end
end

-- For tests: a mark held every frame until pinned again with nil.
function M.pin(hand, t)
  S.pinned = S.pinned or {}
  S.pinned[hand] = t
end

-- For tests: each hand's marker as it stands.
function M.state()
  local out = {}
  for hand = 1, 2 do
    local h = S.hands[hand]
    out[hand] = { spawned = h.id ~= nil, resolved = h.comps ~= nil, shown = h.shown or 0, morph = h.morph or 0,
                  kind = h.at and h.at.kind or false, ring = h.level and h.level[RING] or 0, entity = h.entity }
  end
  return out
end

return M
