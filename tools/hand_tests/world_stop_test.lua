-- Run with LuaJIT and the path of HandCollision/init.lua: it must parse, and the world stop, cut out of it, must behave.
local f = assert(io.open(arg[1], "rb"))
local src = f:read("*a"):gsub("\r\n", "\n")
f:close()
assert(loadstring(src, "init.lua"))
local body = assert(src:match("(local WORLD_ORIGIN = 13.-\nlocal function handsPush.-\nend\n)%s*%-%- THE FINGERS"), "world stop not found")
local fingerBody = assert(src:match("(%-%- THE FINGERS.-\nlocal function carry.-\nend\n)%s*return { worldStop"), "fingers not found")
body = (body .. fingerBody):gsub("local CONTACT = 0.01", "local CONTACT = 0.0")
assert(src:find("pcall(HC.weaponRanged, lastW)", 1, true), "the NPC gate must reach the wrapper's weaponRanged")
assert(not src:find("HOLDER_EDGE", 1, true), "no guessing the left hand from the feet")
assert(src:find("SetVRReloadOwnedHand(-1)", 1, true), "the left wrist must be cleared at the start")
assert(src:find("SKIP[1 - holder]", 1, true), "the gun hand must not collide with its own support arm")

local PX, PY, PZ = {}, {}, {}
local SKIP = { [0] = false, [1] = false }
local HAND = { [0] = { index = 23, cx = 0.04 }, [1] = { index = 24, cx = -0.04 } }
local fingers = { 1, 0, 0 }
local staticY, staticX, dynamicY = nil, nil, nil
local rays = { static = 0, dynamic = 0 }
local handPos = { 0, 0.5, 1.4 }
local notes = {}
shoves, contacts = {}, {}
local function crossing(a, b, axis, v)
  if not v or (a[axis] - v) * (b[axis] - v) > 0 then return nil end
  return (v - a[axis]) / (b[axis] - a[axis])
end
local function at(a, b, t) return { x = a.x + (b.x - a.x) * t, y = a.y + (b.y - a.y) * t, z = a.z + (b.z - a.z) * t } end
local onlyBullet = false
local function traceStatic(a, b, preset)
  if onlyBullet and preset ~= 'Bullet' then return false end
  local ty, tx = crossing(a, b, 'y', staticY), crossing(a, b, 'x', staticX)
  if ty and (not tx or ty <= tx) then return true, { position = at(a, b, ty), material = 'test', normal = { x = 0, y = -1, z = 0 } } end
  if tx then return true, { position = at(a, b, tx), material = 'test', normal = { x = -1, y = 0, z = 0 } } end
  return false
end
local function traceDynamic(a, b)
  local t = crossing(a, b, 'y', dynamicY)
  if t then return true, { position = at(a, b, t), material = 'test', normal = { x = 0, y = -1, z = 0 } } end
  return false
end
local playerComps = {}
local env = setmetatable({
  PX = PX, PY = PY, PZ = PZ, SKIP = SKIP, HAND = HAND,
  spdlog = { info = function(s) notes[#notes + 1] = s end },
  NameToString = function(n) return tostring(n) end,
  CAPS = {
    { slot = 'vrp_f_index2_r', index = 40, r = 0.01 }, { slot = 'vrp_f_index3_r', index = 41, r = 0.01 },
    { slot = 'vrp_f_thumb1_l', index = 42, r = 0.01 }, { slot = 'vrp_f_thumb2_l', index = 43, r = 0.01 },
  },
  endpoints = function(c) return 0, 0.6, 1.4, 0, 0.66, 1.4 end,
  VRTraceInfo = function(tr) return { x = 10.0, y = 5.0, z = 1.0, w = 1.0 } end,
  VRTraceBody = function(tr) return { body = true } end,
  VRBodyIsKinematic = function() return 0 end,
  VRBodySetKinematic = function() return true end,
  VRBodyGetVel = function() return { x = 0, y = 0, z = 0, w = 1 } end,
  VRBodySetVel = function(b, v) shoves[#shoves + 1] = { v = v }; return true end,
  VRHandContactMargin = function() return 0.0 end,
  VRFingerContact = function(side, digit, gap) contacts[#contacts + 1] = { side, digit, gap }; return true end,
  gen = 1,
  require = function(name)
    if name == 'weapon_attachments' then return { w_att__power_03 = { -0.02, 0.02, 0, 0.2, -0.02, 0.02 }, w_att__scope_short_02 = { -0.04, 0.04, -0.05, 0.3, 0, 0.05 } } end
    assert(name == 'weapon_boxes')
    return { w_test__gun = { -0.02, 0.02, -0.03, 0.30, -0.09, 0.04 }, w_test__long = { -0.02, 0.02, -0.03, 0.45, -0.09, 0.04 } }
  end,
  Vector4 = { new = function(x, y, z, w) return { x = x, y = y, z = z, w = w } end },
  Game = {
    GetPlayer = function() return { isPlayer = true } end,
    VREntityComponents = function(e)
      if e.isPlayer then return playerComps end
      return e.comps
    end,
    GetSpatialQueriesSystem = function()
      return { SyncRaycastByQueryPreset = function(_, a, b, preset, static, dynamic)
        assert(static ~= dynamic, "a ray is one kind or the other")
        if static then rays.static = rays.static + 1; return traceStatic(a, b, preset) end
        rays.dynamic = rays.dynamic + 1
        assert(a.y > 0.34, "the dynamic ray must start outside the player's capsule")
        return traceDynamic(a, b)
      end }
    end,
  },
  bone = function(i)
    if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
    local s = i == 24 and -1 or 1
    return handPos[1], handPos[2], handPos[3], s * fingers[1], s * fingers[2], s * fingers[3], 0
  end,
  worldPos = function(e) return e:GetWorldPosition() end,
  qrot = function(i, j, k, r, x, y, z)
    if r == 0 then return i * (x >= 0 and 1 or -1), j * (x >= 0 and 1 or -1), k * (x >= 0 and 1 or -1) end
    return x, y, z
  end,
  -- the gun tests hold the hand at identity
}, { __index = _G })
local FIRM = { [0] = false, [1] = false }
env.FIRM = FIRM
local chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM\n" .. body .. "\nreturn worldStop, weaponStop, handsPush, fingerContact", "world"))
local worldStop, weaponStop, handsPush, fingerContact = setfenv(chunk, env)()

local function run(y, n)
  for _ = 1, n or 12 do
    PX[0], PY[0], PZ[0] = 0, y, 1.4
    PX[1], PY[1], PZ[1] = 0, y, 1.4
    worldStop(0, 0, 0, 0, 0, 0, 1)
  end
  return PY[0], PY[1]
end
local function near(a, b, what) if math.abs(a - b) > 0.01 then error(what .. ": " .. a .. " ~= " .. b, 2) end end
local function bothNear(y, want, what)
  local l, r = run(y)
  near(l, want, what .. " (left)"); near(r, want, what .. " (right)")
end

bothNear(0.6, 0.6, "free hand moved")
staticY = 2.0
bothNear(0.6, 0.6, "far wall stopped the hand")
staticY = 0.6
fingers = { 1, 0, 0 }
bothNear(0.7, 0.6 - 0.035, "palm stop")
fingers = { 0, 1, 0 }
bothNear(0.7, 0.6 - 0.12, "finger stop")
staticY, dynamicY = nil, 0.6
fingers = { 1, 0, 0 }
bothNear(0.7, 0.6 - 0.035, "prop stop")
assert(notes[#notes]:find("dynamic", 1, true), "the log must name the kind")
staticY, dynamicY = nil, nil
run(0.6)
near(PY[0], 0.6, "a stop outlived the thing")
staticY = 0.6
bothNear(1.1, 1.1, "lost hand still held")
staticY = 0.05
bothNear(0.6, 0.6, "origin inside geometry stopped the hand")
-- A wall only the second preset answers still stops the hand.
staticY = 0.6
onlyBullet = true
bothNear(0.7, 0.6 - 0.035, "a wall of the other preset")
onlyBullet = false
staticY = 0.6
SKIP[0] = true
local l, r = run(0.7, 12)
near(l, 0.7, "skipped hand stopped")
near(r, 0.6 - 0.035, "the other hand must still stop")
SKIP[0] = false
staticY, staticX = nil, 0.02
fingers = { 0, 1, 0 }
near(run(0.5), 0.5, "a wall beside the arm pulled the hand back")
near(PX[0], -0.015, "and the hand keeps its gap to it")
staticX = nil
staticY = nil
rays.static, rays.dynamic = 0, 0
run(0.05, 6)
assert(rays.static == 0 and rays.dynamic == 0, "a hand at the chest cast a ray")

-- THE GUN: its origin at the wrist (y=0.5), identity orientation. Box -3..+30 cm along the barrel, 2 cm to the sides.
local function makeGun(id, family)
  local g = { id = id, ranged = true, comps = { { GetName = function() return family .. '__base1_01' end } } }
  function g:GetEntityID() return { hash = self.id } end
  function g:IsRanged() return self.ranged end
  function g:GetWorldPosition() return { x = 0, y = 0.5, z = 1.4 } end
  function g:GetWorldOrientation() return { i = 0, j = 0, k = 0, r = 1 } end
  return g
end
local gun = makeGun(1, 'w_test__gun')
local function shoot(frames, w)
  for _ = 1, frames or 8 do
    PX[1], PY[1], PZ[1] = 0, 0.5, 1.4
    weaponStop(1, w or gun, 0, 0, 0, 0, 0, 0, 1)
  end
  return PY[1], PX[1]
end
handPos = { 0, 0.5, 1.4 }
env.bone = function(i)
  if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
  return handPos[1], handPos[2], handPos[3], 0, 0, 0, 1
end
chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM\n" .. body .. "\nreturn worldStop, weaponStop, handsPush, fingerContact", "world"))
worldStop, weaponStop, handsPush, fingerContact = setfenv(chunk, env)()
local y, x = shoot()
near(y, 0.5, "clear gun moved"); near(x, 0, "clear gun moved sideways")
assert(notes[#notes]:find("w_test__gun", 1, true), "the log must name the outline")
-- HEAD ON, TO THE MILLIMETRE: the barrel ends at y=0.80. A wall at 0.81 does not touch it, and it must not be pushed.
staticY = 0.81
near(shoot(), 0.5, "pushed before it touched")
staticY = 0.75
near(shoot(), 0.45, "gun at the wall")
staticY = nil
-- LYING AGAINST A WALL BESIDE IT: beyond the gun's side nothing moves; against it the arm's own gap is what the wrist keeps.
staticX = 0.06
y, x = shoot()
near(x, 0, "a wall beyond the gun's side")
staticX = 0.01
y, x = shoot()
near(x, -0.025, "gun lying against a wall")
near(y, 0.5, "and not along it")
-- A corner: out of both.
staticY, staticX = 0.75, 0.01
y, x = shoot()
near(y, 0.45, "corner, ahead"); near(x, -0.025, "corner, beside")
staticY, staticX = nil, nil
-- A wall right at the wrist: the arm keeps it off, and the rest of the gun goes back. It holds for a long gun too.
staticY = 0.51
near(shoot(), 0.51 - 0.30, "a gun pressed in")
near(shoot(8, makeGun(4, 'w_test__long')), 0.51 - 0.45, "a long gun pressed in")
-- Pressed through at the wall it does not let go.
assert(FIRM[1], "a gun at a wall takes its hold at once")
staticY = nil
shoot(8)
assert(not FIRM[1], "and a gun clear of it does not")
-- TOUCHING, THE WHOLE BOX IS ASKED EVERY FRAME, so a corner cannot be a frame behind.
staticY = 0.75
shoot(8)
rays.static = 0
shoot(1)
assert(rays.static >= 14, "touching, every point a frame: " .. rays.static)
staticY = nil
shoot(8)
rays.static = 0
shoot(1)
assert(rays.static <= 8, "clear, a few a frame: " .. rays.static)
-- THE GRIP IS MEASURED ONLY WHILE NOTHING HOLDS THE HAND. Pushed, a weapon read a little off the hand must not move the push.
staticY = 0.75
local wander = makeGun(9, 'w_test__gun')
shoot(8, wander)
near(PY[1], 0.45, "before the wander")
function wander:GetWorldPosition() return { x = 0, y = 0.47, z = 1.4 } end
near(shoot(8, wander), 0.45, "the push answered itself")
staticY = nil
-- A weapon not in this hand (a frame of a drop) moves nothing.
local far = makeGun(5, 'w_test__gun')
function far:GetWorldPosition() return { x = 5, y = 5, z = 1.4 } end
staticY = 0.75
near(shoot(8, far), 0.5, "a weapon out of the hand")
staticY = nil
-- An unknown weapon gets the default outline, and the log says so.
shoot(230, makeGun(6, 'w_unknown__thing'))
-- A WEAPON JUST DRAWN SAYS IT IS NOT RANGED, and that must not stick: the first draw of a session had no collision at all.
local late = makeGun(7, 'w_test__gun')
late.ranged = false
staticY = 0.75
near(shoot(8, late), 0.5, "not yet ranged, not stopped")
late.ranged = true
near(shoot(60, late), 0.45, "asked again, and stopped")
staticY = nil
do local seen = false; for _, n in ipairs(notes) do seen = seen or n:find("default", 1, true) ~= nil end; assert(seen, "an unknown weapon must fall back") end
-- WHAT IS MOUNTED COUNTS. A muzzle reaches on past the barrel, 0.30 + 0.20; the scope's length does not.
local armed = makeGun(11, 'w_test__gun')
armed.comps[#armed.comps + 1] = { GetName = function() return 'w_att__power_03_01' end, IsEnabled = function() return true end }
armed.comps[#armed.comps + 1] = { GetName = function() return 'w_att__scope_short_02_01' end, IsEnabled = function() return true end }
staticY = 0.95
near(shoot(40, armed), 0.95 - 0.50, "the muzzle")
staticY = nil
-- One that is on the weapon's model but switched off does not.
local bare = makeGun(12, 'w_test__gun')
bare.comps[#bare.comps + 1] = { GetName = function() return 'w_att__power_03_01' end, IsEnabled = function() return false end }
staticY = 0.75
near(shoot(40, bare), 0.75 - 0.30, "a muzzle not fitted")
staticY = nil
-- A melee weapon is not stopped.
local blade = makeGun(2, 'w_test__gun')
blade.ranged = false
staticY = 0.75
near(shoot(8, blade), 0.5, "melee weapon stopped")
staticY = nil

-- WITH THE PHYSICS ON, A LOOSE THING IS SHOVED AND A HEAVY ONE STILL STOPS THE HAND.
env.bone = function(i)
  if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
  local s = i == 24 and -1 or 1
  return handPos[1], handPos[2], handPos[3], s, 0, 0, 0
end
chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM\n" .. body .. "\nreturn worldStop, weaponStop, handsPush, fingerContact", "world"))
worldStop, weaponStop, handsPush, fingerContact = setfenv(chunk, env)()
PX[0], PY[0], PX[1], PY[1] = 0, 0.6, 0, 0.6
handsPush(true, nil, 100.0)
local material = 'plastic.physmat'
traceDynamic = function(a, b)
  local t = crossing(a, b, 'y', dynamicY)
  if t then return true, { position = at(a, b, t), material = material, normal = { x = 0, y = -1, z = 0 } } end
  return false
end
staticY, dynamicY = nil, 0.6
-- A loose thing stops the hand as anything does, and the hand knocks it: this one thing, away from the hand.
shoves = {}
bothNear(0.7, 0.6 - 0.035, "a light thing holds the hand until it moves")
assert(#shoves >= 1, "pressed against a light thing, it is shoved")
assert(shoves[1].v.y > 0, "away from the hand: " .. shoves[1].v.y)
local v = shoves[1].v
local speed = math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z)
assert(speed >= 0.25 and speed <= 3.0, "at a hand's pace: " .. speed)
material = 'cardboard.physmat'
env.gen = 2
shoves = {}
bothNear(0.7, 0.6 - 0.035, "cardboard")
assert(#shoves >= 1, "cardboard is not a car")
material = 'metal_car.physmat'
shoves = {}
bothNear(0.7, 0.6 - 0.035, "a car let the hand through")
material = 'glass_car.physmat'
bothNear(0.7, 0.6 - 0.035, "a car window let the hand through")
assert(#shoves == 0, "a car is never shoved")
material = 'concrete.physmat'
bothNear(0.7, 0.6 - 0.035, "a dynamic wall let the hand through")
assert(#shoves == 0, "a wall is never shoved")
PX[0], PY[0], PX[1], PY[1] = 0, 0.6, 0, 0.6
handsPush(false, nil, 100.0)
material = 'plastic.physmat'
shoves = {}
bothNear(0.7, 0.6 - 0.035, "without the physics a bottle stops the hand")
assert(#shoves == 0, "without the setting nothing is shoved")
dynamicY = nil

-- THE FINGERS: in something, resting on it, or clear; and a gun hand is not asked.
-- THE FINGERS: a path learned from the finger, rays along it, and an answer that does not depend on where the finger is now.
local realQrot = function(qi, qj, qk, qr, x, y, z)
  local tx, ty, tz = 2 * (qj * z - qk * y), 2 * (qk * x - qi * z), 2 * (qi * y - qj * x)
  return x + qr * tx + qj * tz - qk * ty, y + qr * ty + qk * tx - qi * tz, z + qr * tz + qi * ty - qj * tx
end
env.qrot = realQrot
local closed, wanted = 0.0, 1.0
local function tipAt(c)
  return 0, 0.55 + 0.08 * math.sin(c * math.pi / 2), 1.4 + 0.08 * math.cos(c * math.pi / 2)
end
local handQ = { 0, 0, 0, 1 }
env.bone = function(i)
  if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
  if i == 39 then return 0, 0.55, 1.4, 0, 0, 0, 1 end
  if i == 40 then return 0, 0.59, 1.4, 0, 0, 0, 1 end
  if i == 41 then local x, y, z = tipAt(closed); return x, y - 0.01, z, 0, 0, 0, 1 end
  return handPos[1], handPos[2], handPos[3], handQ[1], handQ[2], handQ[3], handQ[4]
end
env.endpoints = function(c) local x, y, z = tipAt(closed); return x, y - 0.01, z, x, y, z end
env.CAPS = {
  { slot = 'vrp_f_index1_r', index = 39, r = 0.01 }, { slot = 'vrp_f_index2_r', index = 40, r = 0.01 }, { slot = 'vrp_f_index3_r', index = 41, r = 0.0 },
}
env.VRFingerClosed = function() return closed end
env.VRFingerWanted = function() return wanted end
env.VRFingerContact = function(side, digit, limit, hold) contacts[#contacts + 1] = { side, digit, limit, hold }; return true end
env.RAW = { [0] = { x = 0, y = 0.6, z = 1.2 }, [1] = { x = 0, y = 0.6, z = 1.4 } }
local bodyPos, bodyVel, bodyMass = { x = 0, y = 0.7, z = 1.4, w = 1 }, { x = 0, y = 0, z = 0, w = 1 }, 1.0
local gripL, gripR, gripForce = 0.0, 0.0, 0.0
env.GetVRSharedSlot = function(s) return s == 155 and gripL or gripR end
env.VRGripForce = function() return gripForce end
env.VRTraceInfo = function() return { x = 10.0, y = 5.0, z = 1.0, w = bodyMass } end
local stuckBody = false
local theBody = { AddLinearImpulse = function(_, v)
  if stuckBody then return end
  bodyVel = { x = bodyVel.x + v.x / bodyMass, y = bodyVel.y + v.y / bodyMass, z = bodyVel.z + v.z / bodyMass, w = 1 }
end }
env.VRTraceBody = function() return theBody end
env.VRBodyGetPos = function() return bodyPos end
env.VRBodyGetRot = function() return { i = 0, j = 0, k = 0, r = 1 } end
env.VRBodyGetVel = function() return bodyVel end
env.VRBodySetVel = function(_, v) bodyVel = { x = v.x, y = v.y, z = v.z, w = 1 }; return true end
env.VRBodySetAngVel = function() return true end
env.VRBodySetPos = function(_, v) bodyPos = { x = v.x, y = v.y, z = v.z, w = 1 }; return true end
local setPos = env.VRBodySetPos
env.Quaternion = { new = function(i, j, k, r) return { i = i, j = j, k = k, r = r } end }
env.VRBodyGetDims = function() return { x = 0.1, y = 0.1, z = 0.1, w = 1 } end
chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM,CAPS,endpoints=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM,CAPS,endpoints\n" .. body ..
  "\nreturn worldStop, weaponStop, handsPush, fingerContact, carry, carryHand, HOLD, trackHands", "fingers"))
local carry, carryHand, HOLD, trackHands
worldStop, weaponStop, handsPush, fingerContact, carry, carryHand, HOLD, trackHands = setfenv(chunk, env)()
PX[0], PY[0], PZ[0], PX[1], PY[1], PZ[1] = 0, 0.6, 1.2, 0, 0.6, 1.4
local lastIndex
local function visit(holder, frames)
  contacts = {}
  for _ = 1, frames or 30 do fingerContact(holder, 0, 0, 0, 0, 0, 0, 1) end
  for _, c in ipairs(contacts) do if c[1] == 1 and c[2] == 1 then lastIndex = c[3] end end
  return lastIndex
end
staticY = nil
for _, c in ipairs({ 0, 0.25, 0.5, 0.75, 1.0 }) do closed = c; visit(nil, 12) end
assert(visit() == 1.0, "clear ahead: it may close")
staticY = 0.59   -- sin = .5: a third of the way round
closed = 0.1
local open = visit()
assert(open > 0.25 and open < 0.5, "a surface on the path limits it: " .. open)
closed = open    -- THE POINT: the finger now lies at the limit, and the answer is the same
near(visit(), open, "the limit moved with the finger")
closed = 0.05
near(visit(), open, "the limit moved with the finger, opening")
staticY = 0.55   -- level with the knuckle
assert(visit() < 0.01, "in the way of the straight finger: it stays open")
-- AND IS PRESSED BACK by what stands on the straight finger; free of it, it comes back.
local bends = {}
env.VRFingerBend = function(side, digit, angle) bends[side * 10 + digit] = angle; return true end
env.VRTraceInfo = env.VRTraceInfo
visit()
assert(not bends[11] or bends[11] == 0.0, "a closing finger is not bent back")
local wantedWas = wanted
wanted = 0.0
visit()
assert(bends[11] and bends[11] > 1.4, "a wall at the knuckle bends it right back: " .. tostring(bends[11]))
staticY = nil
do local t0 = os.clock(); while os.clock() - t0 < 0.15 do end end  -- a miss is forgiven for a moment
assert(visit() == 1.0, "and is free again")
assert(bends[11] == 0.0, "and it comes straight")
wanted = wantedWas
env.VRFingerBend = nil
lastIndex = nil
assert(visit(1) == nil, "the hand holding a gun has its own fingers")
staticY = nil

-- CARRYING. Body at y=.7, hand at .6, fingers along +y.
local logged = function(what)
  for i = #notes, 1, -1 do if notes[i]:find(what, 1, true) then return notes[i] end end
end
local dt = 1 / 90
local function frame(n)
  for _ = 1, n or 1 do
    trackHands(dt, 0, 0, 0, 0, 0, 0, 1)
    carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1)
    bodyVel = { x = bodyVel.x, y = bodyVel.y, z = bodyVel.z - 9.81 * dt, w = 1 }
    bodyPos = { x = bodyPos.x + bodyVel.x * dt, y = bodyPos.y + bodyVel.y * dt, z = bodyPos.z + bodyVel.z * dt, w = 1 }
    if bodyPos.z < 1.0 then bodyPos.z = 1.0; bodyVel.z = math.max(bodyVel.z, 0) end
    if staticY and bodyPos.y > staticY then bodyPos.y = staticY; bodyVel.y = math.min(bodyVel.y, 0) end  -- the wall, as physics would have it
  end
end
HAND[1].cx, HAND[0].cx = 0.04, 0.04
handQ = { 0, 0, 0.70711, 0.70711 }  -- the hand's +x along +y
handPos = { 0, 0.6, 1.4 }
dynamicY = 0.7
gripR = 1.0
frame(1)
assert(HOLD[1], "a grip at a thing takes it")
assert(logged("took a thing"):find("one hand", 1, true), "one hand")
dynamicY = nil
frame(40)
near(bodyPos.y, 0.6 + 0.085, "the place taken comes to the palm")
assert(bodyPos.z > 1.33 and bodyPos.z < 1.42, "and it is held up: " .. bodyPos.z)
env.RAW[1].y = 0.8
frame(40)
near(bodyPos.y, 0.8 + 0.085, "a light thing goes with the hand")
contacts = {}
for _ = 1, 4 do fingerContact(nil, 0, 0, 0, 0, 0, 0, 1) end
local held = false
for _, c in ipairs(contacts) do if c[1] == 1 and c[4] == true then held = true end end
assert(held, "the fingers hold what the hand has")

-- THE DRAWN HAND STAYS ON IT: the thing is stopped, the hand behind it.
local realSet = env.VRBodySetVel
env.VRBodySetVel = function() return true end
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
local stuckAt = bodyPos.y
env.RAW[1].y = stuckAt - 0.085 + 0.1
for _ = 1, 30 do
  trackHands(dt, 0, 0, 0, 0, 0, 0, 1); carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1)
end
PX[1], PY[1], PZ[1] = env.RAW[1].x, env.RAW[1].y, env.RAW[1].z
carryHand()
near(PY[1], stuckAt - 0.085 + 0.02, "the hand stays with the thing")
env.RAW[1].y = stuckAt - 0.085 + 0.4
for _ = 1, 30 do
  trackHands(dt, 0, 0, 0, 0, 0, 0, 1); carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1)
end
PY[1] = env.RAW[1].y
carryHand()
near(PY[1], env.RAW[1].y - 0.25, "but no further than it can")
env.RAW[1].y = stuckAt - 0.085 + 0.6
for _ = 1, 50 do
  trackHands(dt, 0, 0, 0, 0, 0, 0, 1); carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1)
end
assert(not HOLD[1], "too far from the real hand, it lets go")
env.VRBodySetVel = realSet
gripR = 0.0
frame(1)

-- HEAVY, AND TWO HANDS.
bodyMass = 30.0
env.VRTraceInfo = function() return { x = 10.0, y = 6.0, z = 1.0, w = bodyMass } end
bodyPos = { x = 0, y = 0.7, z = 1.0, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
env.RAW[1].y, env.RAW[1].z = 0.6, 1.0
env.RAW[0].y, env.RAW[0].z = 0.6, 1.0
PY[1], PZ[1], PY[0], PZ[0] = 0.6, 1.0, 0.6, 1.0
handPos = { 0, 0.6, 1.0 }
frame(30)  -- the hands have stopped from where the last test left them
bodyPos = { x = 0, y = 0.7, z = 1.0, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
dynamicY = 0.7
gripR = 1.0
frame(1)
assert(HOLD[1], "a heavy thing is taken")
dynamicY = nil
frame(5)
env.RAW[1].z = 1.3
frame(40)
assert(bodyPos.z < 1.02, "one hand does not lift 30 kg: " .. bodyPos.z)
near(bodyPos.y, 0.7, "and it is not pulled to the palm")
env.RAW[1].z = 1.0
gripL = 1.0
dynamicY = 0.7
frame(2)
assert(HOLD[0] and HOLD[1] and HOLD[0].rec == HOLD[1].rec, "the second hand takes the same thing")
assert(logged("took a thing"):find("second hand", 1, true), "second hand")
dynamicY = nil
env.RAW[1].z, env.RAW[0].z = 1.25, 1.25
frame(180)
assert(bodyPos.z > 1.2, "two hands lift it: " .. bodyPos.z)
gripL = 0.0
frame(1)
assert(HOLD[1] and not HOLD[0], "the other hand still has it")
assert(logged("the other hand has it"), "said so")
frame(30)
assert(bodyPos.z < 1.15, "one hand cannot keep it up: " .. bodyPos.z)
gripR = 0.0
frame(1)

-- HANDED OVER, THEN THROWN: light. Taking it with the other hand and letting go of the first does not throw it.
bodyMass = 1.0
env.VRTraceInfo = function() return { x = 10.0, y = 7.0, z = 1.0, w = bodyMass } end
bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
env.RAW[1].y, env.RAW[1].z, env.RAW[0].y, env.RAW[0].z = 0.6, 1.2, 0.6, 1.2
dynamicY = 0.7
gripR = 1.0
frame(1)
dynamicY = nil
frame(20)
dynamicY = bodyPos.y
gripL = 1.0
frame(2)
dynamicY = nil
assert(HOLD[0] and HOLD[1], "both hands")
frame(10)
gripR = 0.0
local before = #notes
frame(1)
assert(HOLD[0] and not HOLD[1], "handed over")
assert(notes[#notes]:find("the other hand has it", 1, true), "no throw while the other hand has it")
frame(20)
for _ = 1, 8 do env.RAW[0].y = env.RAW[0].y + 0.05; frame(1) end
gripL = 0.0
frame(1)
assert(not HOLD[0], "thrown")
local thrown = tonumber(logged("let go"):match("let go: ([%d%.]+)"))
assert(thrown > 2.0, "a throw has the swing's speed: " .. thrown)
assert(bodyVel.y > 2.0, "and the body has it: " .. bodyVel.y)

-- CALLED TWICE A FRAME, ONE COMMAND: the second call, with the body not having moved, tells it nothing. A THING WITHOUT MASS IS NOT TAKEN.
do
  bodyMass = 1.0
  env.VRTraceInfo = function() return { x = 10.0, y = 11.0, z = 1.0, w = bodyMass } end
  env.RAW[1].y, env.RAW[1].z = 0.6, 1.2
  frame(30)
  bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
  bodyVel = { x = 0, y = 0, z = 0, w = 1 }
  dynamicY = 0.7
  gripR = 1.0
  frame(1)
  dynamicY = nil
  frame(5)
  local writes = 0
  local realSet = env.VRBodySetVel
  env.VRBodySetVel = function(_, v) writes = writes + 1; return realSet(_, v) end
  carry(true, nil, 0.02, 0, 0, 0, 0, 0, 0, 1)
  writes = 0
  carry(true, nil, 0.002, 0, 0, 0, 0, 0, 0, 1)
  assert(writes == 0, "the body has not moved since the last command")
  env.VRBodySetVel = realSet
  gripR = 0.0
  frame(1)
  env.VRTraceInfo = function() return { x = 1.0, y = 12.0, z = 0.0, w = 0.0 } end
  bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
  dynamicY = 0.7
  gripR = 1.0
  frame(1)
  assert(not HOLD[1], "no mass: not a hand's to take")
  dynamicY = nil
  gripR = 0.0
  frame(1)
  -- A hand already closed takes what the other holds when it gets there.
  env.VRTraceInfo = function() return { x = 10.0, y = 13.0, z = 1.0, w = bodyMass } end
  bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
  bodyVel = { x = 0, y = 0, z = 0, w = 1 }
  env.RAW[0].y, env.RAW[0].z = 0.6, 1.2
  dynamicY = 0.7
  gripR = 1.0
  frame(1)
  gripL = 1.0
  assert(HOLD[1] and not HOLD[0], "one hand has it")
  dynamicY = bodyPos.y
  frame(10)
  dynamicY = nil
  assert(HOLD[0] and HOLD[0].rec == HOLD[1].rec, "the closed hand took it")
  gripL, gripR = 0.0, 0.0
  frame(2)
end

-- WHAT WAS WOKEN LIES DOWN AGAIN: kinematic once it has been still three seconds, and only then.
do
  local kin, flips = 1, {}
  local realIs, realKin = env.VRBodyIsKinematic, env.VRBodySetKinematic
  env.VRBodyIsKinematic = function() return kin end
  env.VRBodySetKinematic = function(_, on) kin = on and 1 or 0; flips[#flips + 1] = on; return true end
  bodyMass = 1.0
  env.VRTraceInfo = function() return { x = 10.0, y = 9.0, z = 1.0, w = bodyMass } end
  env.RAW[1].y, env.RAW[1].z = 0.6, 1.2
  frame(30)
  bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
  bodyVel = { x = 0, y = 0, z = 0, w = 1 }
  dynamicY = 0.7
  gripR = 1.0
  frame(1)
  dynamicY = nil
  assert(HOLD[1] and kin == 0, "a kinematic thing taken is moving")
  for _ = 1, 400 do frame(1) end
  assert(kin == 0, "held, it stays moving, however still it is")
  gripR = 0.0
  frame(1)
  bodyPos = { x = 0, y = 0.7, z = 1.0, w = 1 }
  bodyVel = { x = 0, y = 0, z = 0, w = 1 }
  for _ = 1, 200 do frame(1) end
  assert(kin == 0, "two seconds on the ground is not long enough")
  for _ = 1, 100 do frame(1) end
  assert(kin == 1, "three is")
  env.VRBodyIsKinematic, env.VRBodySetKinematic = realIs, realKin
end

-- A STRONG GRIP IS JUST A GRIP.
bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
env.RAW[1].y, env.RAW[1].z = 0.6, 1.2
frame(30)
bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
dynamicY = 0.7
gripR = 1.0
gripForce = 1.0
frame(1)
dynamicY = nil
frame(20)
assert(HOLD[1], "squeezing hard does nothing to it")

-- KEEPS UP WITH A FAST SWING: 2.9 kg, 5 m/s, and not left behind by more than a frame's travel.
bodyMass = 2.9
env.VRTraceInfo = function() return { x = 10.0, y = 8.0, z = 1.0, w = bodyMass } end
gripR = 0.0
frame(1)
bodyPos = { x = 0, y = 0.7, z = 1.2, w = 1 }
bodyVel = { x = 0, y = 0, z = 0, w = 1 }
dynamicY = 0.7
gripR = 1.0
frame(1)
dynamicY = nil
frame(30)
local gap0 = bodyPos.y - env.RAW[1].y
local worst = 0
for _ = 1, 30 do
  env.RAW[1].y = env.RAW[1].y + 5.0 * dt
  frame(1)
  worst = math.max(worst, math.abs(bodyPos.y - env.RAW[1].y - gap0))
end
assert(worst < 0.22, "a light thing keeps up with a swing, led a little: " .. worst)
assert(not HOLD[1].rec.blocked, "and a swing is not a wall")

-- AGAINST A WALL, PULLED: the thing is told to go through a wall that keeps it where it is. It may lean on it, not beat on it.
frame(30)
local realGet = env.VRBodyGetVel
local top = 0
env.VRBodyGetVel = function() return { x = 0, y = 0, z = 0, w = 1 } end
local wallSet = env.VRBodySetVel
env.VRBodySetVel = function(_, v)
  top = math.max(top, math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z))
  return true
end
env.RAW[1].y = env.RAW[1].y + 0.15
for _ = 1, 6 do trackHands(dt, 0, 0, 0, 0, 0, 0, 1); carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1) end
top = 0
for _ = 1, 20 do trackHands(dt, 0, 0, 0, 0, 0, 0, 1); carry(true, nil, dt, 0, 0, 0, 0, 0, 0, 1) end
assert(HOLD[1].rec.blocked, "it knows it is against something")
assert(top < 1.0, "pushing at a wall at a walking pace at most: " .. top)
env.VRBodyGetVel, env.VRBodySetVel = realGet, wallSet
frame(60)
assert(not HOLD[1].rec.blocked, "free again, it catches up")
gripR, gripForce = 0.0, 0.0
frame(1)
HAND[1].cx, HAND[0].cx = -0.04, 0.04
env.bone = function(i)
  if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
  return handPos[1], handPos[2], handPos[3], 0, 0, 0, 1
end

-- THE HANDS' PHYSICS: only an empty hand, clear of the body, and only when allowed.
local state = { [0] = nil, [1] = nil }
playerComps = {
  { GetName = function() return 'VRPortHand_l' end, Toggle = function(_, on) state[0] = on end },
  { GetName = function() return 'VRPortHand_r' end, Toggle = function(_, on) state[1] = on end },
}
local function hands(y0, y1, allowed, holder)
  PX[0], PY[0] = 0, y0
  PX[1], PY[1] = 0, y1
  handsPush(allowed, holder, 200.0)
end
hands(0.6, 0.6, true, nil)
assert(state[0] == true and state[1] == true, "clear empty hands must push")
hands(0.42, 0.42, true, nil)
assert(state[0] == true, "inside the margin it stays as it was")
hands(0.3, 0.6, true, nil)
assert(state[0] == false and state[1] == true, "a hand near the body must not push")
hands(0.6, 0.6, true, 1)
assert(state[0] == true and state[1] == false, "a hand with a weapon must not push")
hands(0.6, 0.6, false, nil)
assert(state[0] == false and state[1] == false, "not allowed, not pushing")
SKIP[0] = true
hands(0.6, 0.6, true, nil)
assert(state[0] == false and state[1] == true, "a hand left out of the solve must not push")
print("world_stop: ok")
