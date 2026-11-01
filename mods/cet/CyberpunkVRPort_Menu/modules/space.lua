-- The world-space helpers the screens in the world share (modules/menu.lua, modules/wheel.lua): vectors as
-- { x, y, z } tables, the plane entity's orientation, and the head as the player sees from it.

local Space = {}

function Space.v3(x, y, z) return { x, y, z } end
function Space.add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
function Space.sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
function Space.mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
function Space.dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
function Space.cross(a, b)
  return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] }
end
function Space.norm(a)
  local n = math.sqrt(Space.dot(a, a))
  if n < 1e-6 then return { 0, 0, 0 }, 0 end
  return Space.mul(a, 1 / n), n
end
function Space.clamp(x, lo, hi) return math.max(lo, math.min(hi, x)) end

function Space.qrot(q, v)
  local ux, uy, uz, w = q.i, q.j, q.k, q.r
  local x, y, z = v[1], v[2], v[3]
  local cx, cy, cz = uy * z - uz * y, uz * x - ux * z, ux * y - uy * x
  local dx, dy, dz = uy * cz - uz * cy, uz * cx - ux * cz, ux * cy - uy * cx
  return { x + 2 * (w * cx + dx), y + 2 * (w * cy + dy), z + 2 * (w * cz + dz) }
end
function Space.qconj(q) return Quaternion.new(-q.i, -q.j, -q.k, q.r) end

-- The rotation whose columns are the world images of the local X, Y and Z axes.
function Space.quatFromBasis(X, Y, Z)
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

-- A plane entity with its canvas right along R and canvas up along U, centred on C. ORIENTATION, measured:
-- the widget component's own rotation is ignored, so the entity carries it; with the entity's local X =
-- canvas LEFT, local Y = canvas DOWN and local Z = toward the viewer the canvas reads upright and unmirrored.
function Space.transformFor(C, R, U)
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(C[1], C[2], C[3], 1.0))
  tr:SetOrientation(Space.quatFromBasis(Space.mul(R, -1), Space.mul(U, -1), Space.cross(R, U)))
  return tr
end

-- The head as the player sees from it: the rendered view pose (VRViewWorldPos / Rot -- the FPP camera
-- has neither the HMD's position tracking nor its lean, so a menu placed from it sat off to the side),
-- the game's camera only when the plugin has none. p, and the heading's forward f and right r.
function Space.view()
  local p, q = nil, nil
  if type(VRViewWorldPos) == "function" and type(VRViewWorldRot) == "function" then
    pcall(function()
      local wp = VRViewWorldPos()
      if wp ~= nil and wp.w ~= nil and wp.w > 0.5 then p, q = { wp.x, wp.y, wp.z }, VRViewWorldRot() end
    end)
  end
  if p == nil then
    local cam = Transform.new()
    local ok = pcall(function() Game.GetCameraSystem():GetActiveCameraWorldTransform(cam) end)
    if not ok then return nil end
    p, q = { cam.position.x, cam.position.y, cam.position.z }, cam.orientation
  end
  local f = Space.qrot(q, { 0, 1, 0 })
  local fh = Space.norm({ f[1], f[2], 0 })
  return { p = p, f = fh, r = { fh[2], -fh[1], 0 } }
end

return Space
