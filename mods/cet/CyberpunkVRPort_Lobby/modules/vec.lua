-- Vectors as { x, y, z } arrays and quaternions as { r, i, j, k }, for the lobby's hands and the things they hold.
local M = {}

function M.v3(v) return { v.x, v.y, v.z } end
function M.add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
function M.sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
function M.mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
function M.dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
function M.len(a) return math.sqrt(M.dot(a, a)) end

function M.qmul(a, b)
  return { r = a.r * b.r - a.i * b.i - a.j * b.j - a.k * b.k, i = a.r * b.i + a.i * b.r + a.j * b.k - a.k * b.j,
           j = a.r * b.j - a.i * b.k + a.j * b.r + a.k * b.i, k = a.r * b.k + a.i * b.j - a.j * b.i + a.k * b.r }
end
function M.qconj(q) return { r = q.r, i = -q.i, j = -q.j, k = -q.k } end
function M.qrot(q, v)
  local p = M.qmul(M.qmul(q, { r = 0, i = v[1], j = v[2], k = v[3] }), M.qconj(q))
  return { p.i, p.j, p.k }
end
function M.quat(q) return { r = q.r, i = q.i, j = q.j, k = q.k } end

-- The lobby's frame (X right, Y forward, Z up, turned by the anchor's heading) to the world.
function M.yawQuat(deg)
  local a = math.rad(deg) / 2
  return { r = math.cos(a), i = 0, j = 0, k = math.sin(a) }
end
function M.toWorld(anchor, p)
  local w = M.qrot(M.yawQuat(anchor.yaw), p)
  return { anchor.pos.x + w[1], anchor.pos.y + w[2], anchor.pos.z + w[3] }
end

return M
