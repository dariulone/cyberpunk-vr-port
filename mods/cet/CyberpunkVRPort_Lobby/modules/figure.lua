-- The figure: V at full height in the tutorial hall, at the player's right (the scene's figure spot: a bearing from
-- the player's heading and a distance), turned to face the player's spot. It is the body a new game is made on and
-- the body an engram's V comes back as.
--
-- WHAT IT IS. The game's photo mode puppet (Character.Player_Puppet_Photomode cloned per body, its template the
-- photo mode's player_ma / player_wa entity -- Phantom Liberty's when the expansion is on): an NPC puppet with the
-- player's character customization on it, spawned through the plugin's spawn system. A clone of the main menu's
-- own preview puppet record spawns too, and is never seen. The character creation system dresses one puppet (its
-- target, src/Lobby/CreationTarget.cpp): pointed at the figure, every option it applies lands on the figure.
--
-- SENT ELSEWHERE (M.goTo) it glides there over GLIDE s, still facing the player's spot. The puppet's own place stays
-- where it was spawned -- its movement holds it there, a teleport or a set transform is undone -- and what moves is
-- its visual offset (the visualOffset component every mesh hangs from), set each frame to the way from there.
local M = {}

local TAG = "VRLobbyFigure"
local GLIDE = 0.6                        -- s
local SOURCE = "Character.Player_Puppet_Photomode"
local RECORDS = { [true] = "Character.VRPort_FigureMale", [false] = "Character.VRPort_FigureFemale" }
local TEMPLATES = {
  ep1 = { [true] = "ep1\\characters\\entities\\player\\photo_mode\\player_ma_photomode_ep1.ent",
          [false] = "ep1\\characters\\entities\\player\\photo_mode\\player_wa_photomode_ep1.ent" },
  base = { [true] = "base\\characters\\entities\\player\\photo_mode\\player_ma_photomode.ent",
           [false] = "base\\characters\\entities\\player\\photo_mode\\player_wa_photomode.ent" },
}

-- The puppet; the lobby's frame and the spot it was spawned at; where it is seen now, where it was sent from and to,
-- and how far it has gone (s).
local S = { id = nil, male = nil, entity = nil, clock = 0, anchor = nil, home = nil, at = nil, from = nil, to = nil,
            t = 0 }

local function ep1()
  local ok, on = pcall(function() return Game.GetSystemRequestsHandler():IsAdditionalContentEnabled(CName.new("EP1")) end)
  return ok and on == true
end

-- The record for a body, made the first time it is wanted.
local function record(male)
  local name = RECORDS[male]
  local made = false
  pcall(function() made = TweakDB:GetRecord(name) ~= nil end)
  if not made then
    pcall(function() TweakDB:CloneRecord(name, SOURCE) end)
  end
  pcall(function() TweakDB:SetFlat(name .. ".entityTemplatePath", TEMPLATES[ep1() and "ep1" or "base"][male]) end)
  return name
end

-- Where the figure stands, in the world, and the heading that faces the player's spot.
local function placeOf(anchor, spec)
  local b, yaw = math.rad(spec.bearing), math.rad(anchor.yaw)
  local x, y = spec.dist * math.sin(b), spec.dist * math.cos(b)
  local cs, sn = math.cos(yaw), math.sin(yaw)
  local wx, wy = cs * x - sn * y, sn * x + cs * y
  local pos = Vector4.new(anchor.pos.x + wx, anchor.pos.y + wy, anchor.pos.z, 1.0)
  -- An entity's forward is (-sin h, cos h): toward the player's spot, back along (wx, wy).
  local heading = math.deg(math.atan2(wx, -wy))
  return pos, heading
end

-- The figure of this body, spawned at the scene's spot (the one standing there goes if it is the other body).
function M.spawn(anchor, spec, male)
  if anchor == nil or spec == nil then return false end
  male = male ~= false
  if S.id ~= nil and S.male == male then return true end
  M.clear()
  local ok, err = pcall(function()
    local pos, heading = placeOf(anchor, spec)
    S.id = Game.VRSpawnRecord(TweakDBID.new(record(male)), CName.new(""), pos, EulerAngles.new(0, 0, heading):ToQuat(),
      { CName.new("VRLobby"), CName.new(TAG) })
  end)
  if not ok then M.lastError = tostring(err) end
  S.male, S.entity, S.clock = male, nil, 0
  S.anchor, S.home = anchor, { bearing = spec.bearing, dist = spec.dist }
  S.at, S.from, S.to, S.t = S.home, S.home, S.home, GLIDE
  return ok
end

-- The figure sent to another spot (a bearing and a distance in the lobby's frame, as M.spawn's).
function M.goTo(spec)
  if S.id == nil or spec == nil then return end
  if S.to.bearing == spec.bearing and S.to.dist == spec.dist then return end
  S.from, S.to, S.t = S.at, { bearing = spec.bearing, dist = spec.dist }, 0
end

local function smooth(x)
  x = math.max(0, math.min(1, x))
  return x * x * (3 - 2 * x)
end

-- On its way: the visual offset from the spawned spot to the one between, in the puppet's own frame (X right,
-- Y forward), turned by the difference of the headings.
function M.update(dt)
  if S.id == nil or S.t >= GLIDE then return end
  local e = M.entity()
  if e == nil then return end
  S.t = math.min(GLIDE, S.t + (dt or 0))
  local k = smooth(S.t / GLIDE)
  S.at = { bearing = S.from.bearing + (S.to.bearing - S.from.bearing) * k, dist = S.from.dist + (S.to.dist - S.from.dist) * k }
  local home, h0 = placeOf(S.anchor, S.home)
  local pos, h = placeOf(S.anchor, S.at)
  local r = math.rad(h0)
  local dx, dy = pos.x - home.x, pos.y - home.y
  pcall(function()
    e:FindComponentByName(CName.new("visualOffset")):SetLocalTransform(
      Vector4.new(dx * math.cos(r) + dy * math.sin(r), -dx * math.sin(r) + dy * math.cos(r), 0, 1),
      EulerAngles.new(0, 0, h - h0):ToQuat())
  end)
end

-- The figure's entity, once it is in the world.
function M.entity()
  if S.id == nil then return nil end
  if S.entity == nil then
    pcall(function() S.entity = Game.FindEntityByID(S.id) end)
  end
  return S.entity
end

function M.male() return S.male end
function M.standing() return S.id ~= nil end

function M.clear()
  pcall(function() Game.VRSpawnDeleteTagged(CName.new(TAG)) end)
  S.id, S.male, S.entity = nil, nil, nil
  S.anchor, S.home, S.at, S.from, S.to = nil, nil, nil, nil, nil
end

function M.state()
  return { spawned = S.id ~= nil, entity = M.entity() ~= nil, male = S.male, error = M.lastError,
           at = S.at and string.format("%.1f/%.2f", S.at.bearing, S.at.dist) or nil }
end

return M
