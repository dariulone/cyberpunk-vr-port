-- Closest wrist point on the cant-relief arc, not a larger spherical grab zone.
-- The authored and upright endpoints leave a gap when the free wrist follows only part of the gun's roll.
local A = {}
function A.closest(ax, ay, az, phi, tx, ty, tz, hx, hy, hz)
    local ta = tx * ax + ty * ay + tz * az
    local ha = hx * ax + hy * ay + hz * az
    local px, py, pz = tx - ax * ta, ty - ay * ta, tz - az * ta
    local qx, qy, qz = hx - ax * ha, hy - ay * ha, hz - az * ha
    if px * px + py * py + pz * pz < 1e-8 or qx * qx + qy * qy + qz * qz < 1e-8 then return 0 end
    local cx, cy, cz = py * qz - pz * qy, pz * qx - px * qz, px * qy - py * qx
    local theta = math.atan2(cx * ax + cy * ay + cz * az, px * qx + py * qy + pz * qz)
    local lo, hi = math.min(0, phi), math.max(0, phi)
    local best, score = 0, -math.huge
    -- atan2 wraps at pi: compare both representations and the endpoints before choosing the bounded arc.
    for k = -1, 1 do
        local candidate = math.max(lo, math.min(hi, theta + k * 2 * math.pi))
        local alignment = math.cos(candidate - theta)
        if alignment > score then best, score = candidate, alignment end
    end
    return best
end
return A
