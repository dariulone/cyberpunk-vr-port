-- The lobby's light: the terminal's СВЕТ dial (scene_list.lua dials.light) dims and brightens every lamp of the
-- place -- the scene's entLightComponents (make_lobby_scene.py lights_of: the rows along the amber lines, the lamps
-- by the props), each set to LEVEL_MAX times its own intensity at the most, none at the least.
--
-- TURN IT: a grip closed with the palm at the dial takes it, and while held the hand's roll about the dial's axis
-- turns it, clockwise brighter -- a quarter turn of the wrist is half its travel, a haptic tick every tenth. PRESS IT:
-- a fingertip pushed into its face, or a hand's pointer on it and a trigger pull, moves it to its next stop (off,
-- half, as built, twice, three times, and round again). The level is kept for the next time (light.txt, the mod's
-- folder).
local M = {}
local Hands = require("modules/hands")
local V = require("modules/vec")

local SCENE_TAG = "VRLobbyScene"
local LIGHT = "entLightComponent"
local LEVEL_MAX, LEVEL_DEFAULT = 3.0, 1.0
local STOPS = { 0.0, 0.5, 1.0, 2.0, 3.0 }
local TRAVEL = math.rad(270)          -- the dial's whole turn, off to LEVEL_MAX
local WRIST_GAIN = 1.5                -- dial turn per wrist turn
local REACH = 0.10                    -- beyond the dial's rim, how far from its face a grip takes it
local FILE = "light.txt"
local HAPTIC_TICK, HAPTIC_CLICK = 21, 20
local SOUND = "CyberpunkVRPort_Menu/sounds/click.wav"

local S = { level = nil, dial = nil, base = {}, applied = nil, held = nil }

local function haptic(event, hand)
  if type(VRHapticEvent) == "function" then pcall(VRHapticEvent, event, hand == 2 and 2 or 1) end
end

---------------------------------------------------------------- the level

local function load()
  local level = LEVEL_DEFAULT
  pcall(function()
    local f = io.open(FILE, "r")
    if f then
      local v = tonumber(f:read("*l"))
      f:close()
      if v ~= nil then level = math.max(0.0, math.min(LEVEL_MAX, v)) end
    end
  end)
  return level
end

local function save()
  pcall(function()
    local f = io.open(FILE, "w")
    if f then f:write(string.format("%.3f\n", S.level)); f:close() end
  end)
end

-- Every lamp of the place at its own intensity times the level. A lamp's own intensity is read the first time it is
-- seen (the place is spawned again with each lobby, its lamps as built).
local function apply()
  local n = 0
  pcall(function()
    for _, e in ipairs(Game.VRSpawnTagged(CName.new(SCENE_TAG))) do
      local id = tostring(e:GetEntityID().hash)
      for _, c in ipairs(Game.VREntityComponents(e)) do
        if c:GetClassName().value == LIGHT then
          local key = id .. "/" .. c:GetName().value
          if S.base[key] == nil then S.base[key] = c.intensity end
          c:SetIntensity(S.base[key] * S.level)
          n = n + 1
        end
      end
    end
  end)
  S.applied = S.level
  return n
end

local function set(level, hand)
  level = math.max(0.0, math.min(LEVEL_MAX, level))
  local before = math.floor(S.level / LEVEL_MAX * 10 + 1e-6)
  S.level = level
  if math.floor(level / LEVEL_MAX * 10 + 1e-6) ~= before and hand ~= nil then haptic(HAPTIC_TICK, hand) end
end

---------------------------------------------------------------- the dial

-- The angle of the hand about the dial's axis: whichever of the palm's two side axes lies flatter on the dial's
-- face, measured in it (counter-clockwise as the player sees the dial), that axis chosen when the dial is taken.
local function wristAngle(H, axis)
  local D = S.dial
  local a = V.qrot(H.rot, axis)
  return math.atan2(V.dot(a, D.u), V.dot(a, D.r))
end

local function take(hand)
  local D, H = S.dial, Hands.hands[hand]
  local d = V.sub(H.pos, D.c)
  local along = V.dot(d, D.n)
  local across = V.len(V.sub(d, V.mul(D.n, along)))
  if across > D.radius + REACH or along < -0.03 or along > REACH + 0.05 then return end
  local best, flat = nil, -1
  for _, axis in ipairs({ { 1, 0, 0 }, { 0, 0, 1 } }) do
    local a = V.qrot(H.rot, axis)
    local f = 1 - math.abs(V.dot(a, D.n))
    if f > flat then best, flat = axis, f end
  end
  S.held = { hand = hand, axis = best, last = wristAngle(H, best) }
  Hands.claim(hand, M)
  haptic(HAPTIC_CLICK, hand)
end

local function turn()
  local P = S.held
  local H = Hands.hands[P.hand]
  if not H.grip or H.pos == nil then
    Hands.release(P.hand, M)
    S.held = nil
    save()
    return
  end
  local a = wristAngle(H, P.axis)
  local d = a - P.last
  if d > math.pi then d = d - 2 * math.pi elseif d < -math.pi then d = d + 2 * math.pi end
  P.last = a
  -- Clockwise (the angle falling, as the player sees it) brighter.
  set(S.level - d * WRIST_GAIN / TRAVEL * LEVEL_MAX, P.hand)
end

local function step(hand)
  local next = STOPS[1]
  for _, v in ipairs(STOPS) do
    if v > S.level + 1e-3 then next = v; break end
  end
  set(next, nil)
  haptic(HAPTIC_CLICK, hand)
  if type(VRMenuPlayWav) == "function" then pcall(VRMenuPlayWav, SOUND, 0.8) end
  save()
end

---------------------------------------------------------------- the lobby's calls

-- The dial with the place (its frame in the lobby's): opened once the place has spawned, its lamps with it.
function M.open(anchor, dial)
  M.close()
  if S.level == nil then S.level = load() end
  S.base = {}
  if anchor ~= nil and dial ~= nil then
    local r = 2 * dial.radius
    S.dial = Hands.face(anchor, dial.centre, dial.right, dial.up, r, r)
    S.dial.n = V.qrot(V.yawQuat(anchor.yaw), dial.front)
    S.dial.radius = dial.radius
  end
  apply()
end

function M.close()
  if S.held ~= nil then Hands.release(S.held.hand, M) end
  S.dial, S.held, S.applied = nil, nil, nil
end

-- Before the items' update, the same frame, after modules/hands.lua's: a grip at the dial is the dial's.
function M.update(dt)
  if S.dial == nil then return end
  if S.held ~= nil then
    turn()
  else
    for hand = 1, 2 do
      if S.held == nil and Hands.taking(hand) then take(hand) end
    end
    local hand = Hands.pressed(S.dial, Hands.pointer(S.dial))
    if hand ~= nil then step(hand) end
  end
  if S.applied ~= S.level then apply() end
end

function M.state()
  return { level = S.level, held = S.held ~= nil and S.held.hand or false, open = S.dial ~= nil }
end

-- For tests and the settings: the level outright.
function M.set(level)
  set(level, nil)
  save()
end

-- For tests: both hands against the dial's face, as modules/hands.lua pointer gives them.
function M.pointer() return Hands.pointer(S.dial) end

return M
