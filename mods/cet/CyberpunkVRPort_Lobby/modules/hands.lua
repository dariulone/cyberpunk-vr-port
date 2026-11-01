-- The player's hands in the lobby, read once a frame for everything there a hand takes hold of (the things on the
-- table, the terminal's light dial): each palm in the world (the solved avatar's palm, model space, through the
-- player's transform), its rotation and velocity, the grip and the trigger. The palm is the hand that is drawn:
-- with the controller beyond the arm's reach it stops short, and `reach` is where the controller is (VRHandRawModel).
--
-- The lobby is the game's main menu, and in a menu the port's grip slots are held down so no gesture fires from a
-- menu press (Hooks/XInput.cpp); the VR menu's pointer reads the controllers raw, and the grips and triggers come
-- from it, and so does the hand's pointer ray (from the index finger's root along the hand). A hand holds one thing
-- at a time: whatever takes hold of it claims it until it lets go.
local M = {}
local V = require("modules/vec")

local GRIP_ON, GRIP_OFF = 0.6, 0.35  -- the analog grip closes the hand past the first, opens it below the second
local VEL_SMOOTH = 0.06              -- s the controller's velocity against the body is smoothed over (M.hands[].cvel)
local AXES = nil                     -- the plane the grips and triggers are read against (M.update)
-- The palm things are pointed at with, in the hand bone's frame (1 left, 2 right; tools/grasp_tests prints them from
-- the rig): the middle of its skin, and its normal -- out of the palm, the side the fingers close to. The same as the
-- game's reach (CyberpunkVRPort_Reach modules/world.lua).
local PALM_AT = { { 0.0396, 0.0039, -0.0124 }, { -0.0396, -0.0039, 0.0124 } }
local PALM_OUT = { { 0.0775, -0.0077, -0.9970 }, { -0.0775, 0.0077, 0.9970 } }

-- 1 left, 2 right: pos, rot, vel (the palm in the world), reach (the controller in the world), grip, gripPrev,
-- trigger, owner, ray (the menu pointer: origin o and unit direction d in the world, or nil), palm (the ray things are
-- pointed at with: o the palm's middle, d out of it, or nil), cvel (the controller's velocity against
-- the body, smoothed: what a snatch is read from -- the drawn palm follows it through the body's IK and lags a quick
-- move; the same reading as the game's reach, CyberpunkVRPort_Reach modules/world.lua).
M.hands = { {}, {} }

function M.update(dt)
  local pl = Game.GetPlayer()
  if pl == nil or type(VRPalmModelPos) ~= "function" then return false end
  local pp, pq = V.v3(pl:GetWorldPosition()), V.quat(pl:GetWorldOrientation())
  -- The pointer against any plane: only its grips and triggers are wanted here.
  local hits = nil
  if type(VRMenuPointer) == "function" then
    -- The plane's axes and size are the same every frame: made once (a frame's new userdata is the GC's work).
    AXES = AXES or { Vector4.new(1, 0, 0, 0), Vector4.new(0, 0, 1, 0), Vector4.new(1, 1, 0, 0) }
    local ok, h = pcall(VRMenuPointer, Vector4.new(pp[1], pp[2], pp[3], 1), AXES[1], AXES[2], AXES[3])
    if ok and h ~= nil and #h >= 8 then hits = h end
  end
  for hand = 1, 2 do
    local H = M.hands[hand]
    local ok, m, r = pcall(function() return VRPalmModelPos(hand - 1), VRPalmModelRot(hand - 1) end)
    if ok and m ~= nil and m.w > 0.5 then
      local pos = V.add(pp, V.qrot(pq, { m.x, m.y, m.z }))
      H.vel = (H.pos ~= nil and dt > 0) and V.mul(V.sub(pos, H.pos), 1 / dt) or { 0, 0, 0 }
      H.pos, H.rot = pos, V.qmul(pq, V.quat(r))
      H.palm = { o = V.add(pos, V.qrot(H.rot, PALM_AT[hand])), d = V.qrot(H.rot, PALM_OUT[hand]) }
      H.reach = nil
      if type(VRHandRawModel) == "function" then
        local okr, c = pcall(VRHandRawModel, hand - 1)
        if okr and c ~= nil and c.w > 0.5 then H.reach = V.add(pp, V.qrot(pq, { c.x, c.y, c.z })) end
      end
    else
      H.pos, H.reach, H.palm = nil, nil, nil
    end
    H.ray = nil
    local was = H.rel
    H.rel = nil
    if hits ~= nil then
      -- The pointer meets the plane, or (missing it) is given 2 m along: either way along the ray.
      local o, e = hits[4 * hand - 1], hits[4 * hand]
      local d = { e.x - o.x, e.y - o.y, e.z - o.z }
      local l = V.len(d)
      if l > 1e-3 then H.ray = { o = { o.x, o.y, o.z }, d = V.mul(d, 1 / l) } end
      H.rel = V.sub({ o.x, o.y, o.z }, pp)
    end
    if H.rel ~= nil and was ~= nil and dt > 1e-4 then
      local v = V.mul(V.sub(H.rel, was), 1 / dt)
      H.cvel = H.cvel and V.add(H.cvel, V.mul(V.sub(v, H.cvel), math.min(1, dt / VEL_SMOOTH))) or v
    else
      H.cvel = nil
    end
    local g = hits ~= nil and hits[4 * hand].w or 0
    H.gripPrev, H.grip = H.grip, g >= GRIP_ON or (H.grip == true and g > GRIP_OFF)
    H.trigger = hits ~= nil and hits[4 * hand - 1].w or 0
    if not H.grip then H.owner = nil end
  end
  return true
end

-- The grip closed this frame, on a hand nothing holds yet.
function M.taking(hand)
  local H = M.hands[hand]
  return H.pos ~= nil and H.grip == true and H.gripPrev ~= true and H.owner == nil
end

function M.claim(hand, who) M.hands[hand].owner = who end
function M.release(hand, who)
  if M.hands[hand].owner == who then M.hands[hand].owner = nil end
end

---------------------------------------------------------------- pressing a face

local TOUCH, PULL, LET_GO = 0.006, 0.75, 0.35

-- A face to press, in the world: { c = centre, r = right, u = up (unit), w, h = metres }.
function M.face(anchor, centre, right, up, w, h)
  local yq = V.yawQuat(anchor.yaw)
  return { c = V.toWorld(anchor, centre), r = V.qrot(yq, right), u = V.qrot(yq, up), w = w, h = h,
           pressed = {}, pulled = {} }
end

-- Both hands against the face, per hand (1 left, 2 right): fingertip, ray, ray origin (w: trigger), ray hit
-- (w: grip).
function M.pointer(F)
  if F == nil or type(VRMenuPointer) ~= "function" then return nil end
  -- A face does not move: its vectors are made once.
  F.v = F.v or { Vector4.new(F.c[1], F.c[2], F.c[3], 1), Vector4.new(F.r[1], F.r[2], F.r[3], 0),
                 Vector4.new(F.u[1], F.u[2], F.u[3], 0), Vector4.new(F.w, F.h, 0, 0) }
  local ok, hits = pcall(VRMenuPointer, F.v[1], F.v[2], F.v[3], F.v[4])
  if not ok or hits == nil or #hits < 8 then return nil end
  return hits
end

-- The hand that pressed the face this frame, if one did: a fingertip pushed into it, or the pointer on it and the
-- trigger pulled (an edge of either; the face keeps the last frame's).
function M.pressed(F, hits)
  if hits == nil then return nil end
  local fired = nil
  for hand = 1, 2 do
    local tip, ray, origin = hits[4 * hand - 3], hits[4 * hand - 2], hits[4 * hand - 1]
    local on = function(p) return p.x >= -0.2 and p.x <= 1.2 and p.y >= -0.2 and p.y <= 1.2 end
    local pressed = tip.w > 0.5 and on(tip) and tip.z <= TOUCH and tip.z > -0.05
    if pressed and not F.pressed[hand] then fired = hand end
    F.pressed[hand] = pressed
    local aimed = ray.w > 0.5 and on(ray)
    local pulled = aimed and (F.pulled[hand] and origin.w > LET_GO or origin.w >= PULL)
    if pulled and not F.pulled[hand] then fired = hand end
    F.pulled[hand] = pulled
  end
  return fired
end

return M
