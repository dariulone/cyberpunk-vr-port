-- The hand's shape round what it holds in the lobby: the fingers closed on the thing's own shape and size, not a
-- recorded pose. The plugin does it (src/Anim/GraspPose.cpp) from a thing's entry -- `shape` (box, sphere, cylinder,
-- cone) and `half`, its half sizes in its own frame (scene_list.lua, modules/engrams.lua). Its shape and size make
-- the grasp: a can, a bottle, a cone or a bar wrapped across the palm; a ball in the spread fingers; a bigger box or a
-- plate flat on the palm where the hand took it; a small thing pinched between the index finger's pad and the thumb's.
--  - M.seat: where the thing lies for that grasp, in the hand's slot, from where it was when the hand took it;
--  - M.hold: the fingers then close on it joint by joint until they touch it and the thumb finds its side -- a big
--    box barely bends them, a ball has the pads on it, a can lies in the curled hand.
-- A thing whose whole box is not where a hand holds it (a figure: by its lower body, standing on the palm like a
-- book on its edge) names the part held: `grasp_half`, its half sizes, and `grasp_at`, its middle in the thing's
-- frame; the grasp is that part's and the thing lies in the hand where the part does.
-- Eased on from tracking when the grip takes it and off when it lets go. The plugin's hands are 0 left, 1 right; the
-- lobby's 1 left, 2 right.
local M = {}
local V = require("modules/vec")

local SHAPES = { box = 0, sphere = 1, cylinder = 2, cone = 3 }
local EASE_IN, EASE_OUT = 0.12, 0.18     -- s: from tracking to the grasp and back

local S = { { blend = 0, target = 0, shape = nil, applied = false }, { blend = 0, target = 0, shape = nil, applied = false } }

local function available()
  return type(VRGraspSeat) == "function" and type(VRGraspSet) == "function" and type(VRGraspBlend) == "function"
     and type(VRGraspClear) == "function"
end

-- A thing's shape for the grasp, from its entry: the plugin's code and the half sizes, or nil.
local function shapeOf(entry)
  local code = SHAPES[entry ~= nil and entry.shape or ""]
  local h = entry ~= nil and (entry.grasp_half or entry.half) or nil
  if code == nil or type(h) ~= "table" then return nil end
  return code, h
end

-- The held part's place for the thing at p turned q (its own place without a part).
local function part(entry, p, q)
  if type(entry.grasp_at) ~= "table" then return p end
  return V.add(p, V.qrot(q, entry.grasp_at))
end

-- Where a thing of this entry, at p turned q in the hand's slot when the hand takes it, lies for its grasp: its
-- position and turn in the slot, or nil (no shape, or the palm not measured yet).
function M.seat(hand, entry, p, q)
  local code, h = shapeOf(entry)
  if code == nil or not available() then return nil end
  local at = part(entry, p, q)
  local ok, t = pcall(VRGraspSeat, hand - 1, code, h[1], h[2], h[3], Vector4.new(at[1], at[2], at[3], 1),
                      Quaternion.new(q.i, q.j, q.k, q.r))
  if not ok or t == nil or t.position.w < 0.5 then return nil end
  local tq = { i = t.orientation.i, j = t.orientation.j, k = t.orientation.k, r = t.orientation.r }
  local tp = { t.position.x, t.position.y, t.position.z }
  if type(entry.grasp_at) == "table" then tp = V.sub(tp, V.qrot(tq, entry.grasp_at)) end
  return tp, tq
end

-- The hand takes hold of the thing of this entry, at this pose in its slot.
function M.hold(hand, entry, pos, rot)
  local code, h = shapeOf(entry)
  if code == nil or pos == nil or rot == nil or not available() then return end
  local H = S[hand]
  local at = part(entry, pos, rot)
  local ok = pcall(function()
    VRGraspBlend(hand - 1, H.blend)
    VRGraspSet(hand - 1, code, h[1], h[2], h[3], Vector4.new(at[1], at[2], at[3], 1),
               Quaternion.new(rot.i, rot.j, rot.k, rot.r))
  end)
  if not ok then return end
  H.applied, H.shape, H.target = true, entry.shape, 1
end

-- The hand lets go: the fingers ease back to tracking.
function M.release(hand)
  S[hand].target = 0
end

function M.update(dt)
  dt = dt or 0
  for hand = 1, 2 do
    local H = S[hand]
    if H.applied then
      local rate = H.target > H.blend and (1 / EASE_IN) or (1 / EASE_OUT)
      if H.blend < H.target then H.blend = math.min(H.target, H.blend + rate * dt)
      else H.blend = math.max(H.target, H.blend - rate * dt) end
      if H.blend <= 0.001 and H.target == 0 then
        pcall(VRGraspClear, hand - 1)
        H.applied, H.shape = false, nil
      else
        pcall(VRGraspBlend, hand - 1, H.blend)
      end
    end
  end
end

function M.close()
  for hand = 1, 2 do
    if S[hand].applied then pcall(VRGraspClear, hand - 1) end
    S[hand] = { blend = 0, target = 0, shape = nil, applied = false }
  end
end

function M.state() return { { kind = S[1].shape, blend = S[1].blend }, { kind = S[2].shape, blend = S[2].blend } } end

return M
