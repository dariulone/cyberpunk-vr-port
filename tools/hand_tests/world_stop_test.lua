-- Run with LuaJIT and the path of HandCollision/init.lua: it must parse, and the world stop, cut out of it, must behave.
local f = assert(io.open(arg[1], "rb"))
local src = f:read("*a"):gsub("\r\n", "\n")
f:close()
assert(loadstring(src, "init.lua"))
local body = assert(src:match("(local WORLD_ORIGIN = 13.-\nlocal function handsPush.-\nend\n)%s*%-%- THE FINGERS"), "world stop not found")
local fingerBody = assert(src:match("(%-%- THE FINGERS.-\nlocal function carry.-\nend\n)%s*return { worldStop"), "carry not found")
body = (body .. fingerBody):gsub("local CONTACT = 0.01", "local CONTACT = 0.0")
assert(src:find("pcall(HC.weaponRanged, lastW)", 1, true), "the NPC gate must reach the wrapper's weaponRanged")
assert(not src:find("HOLDER_EDGE", 1, true), "no guessing the left hand from the feet")
assert(src:find("SetVRReloadOwnedHand(-1)", 1, true), "the left wrist must be cleared at the start")
assert(src:find("SKIP[1 - holder]", 1, true), "the gun hand must not collide with its own support arm")
assert(not src:find("weaponStop", 1, true), "a gun goes where tracking puts it: no wall stops it")
assert(not src:find("fingerContact", 1, true), "the fingers collide with nothing")

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
  require = function(name) error('the world stop requires nothing: ' .. tostring(name)) end,
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
}, { __index = _G })
local FIRM = { [0] = false, [1] = false }
env.FIRM = FIRM
local chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM\n" .. body .. "\nreturn worldStop, handsPush", "world"))
local worldStop, handsPush = setfenv(chunk, env)()

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

-- WITH THE PHYSICS ON, A LOOSE THING IS SHOVED AND A HEAVY ONE STILL STOPS THE HAND.
env.bone = function(i)
  if i == 13 then return 0, 0, 1.4, 0, 0, 0, 1 end
  local s = i == 24 and -1 or 1
  return handPos[1], handPos[2], handPos[3], s, 0, 0, 0
end
chunk = assert(loadstring("local PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM=PX,PY,PZ,SKIP,HAND,bone,qrot,worldPos,FIRM\n" .. body .. "\nreturn worldStop, handsPush", "world"))
worldStop, handsPush = setfenv(chunk, env)()
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

-- THE CARRY, against a body of arithmetic. The fingers do not collide any more: their tests went with them.
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
  "\nreturn worldStop, handsPush, carry, carryHand, HOLD, trackHands", "carry"))
local carry, carryHand, HOLD, trackHands
worldStop, handsPush, carry, carryHand, HOLD, trackHands = setfenv(chunk, env)()
PX[0], PY[0], PZ[0], PX[1], PY[1], PZ[1] = 0, 0.6, 1.2, 0, 0.6, 1.4

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
