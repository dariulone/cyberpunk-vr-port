-- K.gunPlane and K.gunWalls as they are in init.lua, against a wall made of arithmetic: y = 1 in the world, the player turned by nothing, standing at the origin.
local f = assert(io.open(arg[1], 'rb')); local s = f:read('*a'):gsub('\r\n', '\n'); f:close()
local a = assert(s:find('K.GUN_PLANES, K.GUN_POINTS_NOW', 1, true))
local b = assert(s:find('local function weaponStop(side, w,', 1, true))
local code = s:sub(a, b - 1)
local env = {
    K = { GUN_TARGETS = { { 0, 0.1, 0 }, { 0, 0.3, 0 }, { 0, 0.5, 0 }, { 0, 0.7, 0 } } },
    GUN_INSIDE = 0.002, SHOVE_MAX_MASS = 30.0, WORLD_PRESETS = { 'p' }, worldPreset = 1,
    math = math, table = table, string = string, tostring = tostring, pcall = pcall, os = { clock = function() return _G.NOW end },
    spdlog = { info = function(t) print('  log: ' .. t) end }, worldNote = function(_, kind) print('  note: ' .. kind) end,
    Vector4 = { new = function(x, y, z, w) return { x = x, y = y, z = z, w = w } end },
    qrot = function(i, j, k, r, x, y, z)
        local tx, ty, tz = 2 * (j * z - k * y), 2 * (k * x - i * z), 2 * (i * y - j * x)
        return x + r * tx + (j * tz - k * ty), y + r * ty + (k * tx - i * tz), z + r * tz + (i * ty - j * tx)
    end,
}
local wall = { y = 1.0, staticOnly = false, rec = nil }
env.bodyOf = function() return wall.rec end
env.castRay = function(_, from, to, _, staticOnly, dynamicOnly)
    if staticOnly ~= wall.staticOnly and dynamicOnly == wall.staticOnly then return false end
    if from.y < wall.y and to.y >= wall.y then
        local t = (wall.y - from.y) / (to.y - from.y)
        return true, { position = { x = from.x + (to.x - from.x) * t, y = wall.y, z = from.z + (to.z - from.z) * t }, normal = { x = 0, y = -1, z = 0 }, material = 'm' }
    end
    return false
end
setmetatable(env, { __index = _G })
assert(load(code, 'gunWalls', 't', env))()
local K = env.K
_G.NOW = 0.0
local bad = 0
local function run(name, wristY, frames, want, tolerance)
    K.GUN_PLANES = {}
    local tmost, last = 0.0, 0.0
    for i = 1, frames do
        _G.NOW = _G.NOW + 0.022
        local ty = select(2, K.gunWalls(0, {}, 0, 0, 0, 1, 0.5, wristY, 1.3, 0.5, math.min(wristY, 0.9), 1.3, 0, 0, 0, 0, 0, 0, 1))
        last = ty
        if ty < tmost then tmost = ty end
    end
    local ok = math.abs(last - want) <= tolerance
    if not ok then bad = bad + 1 end
    print(string.format('%-34s push %.3f, most %.3f, wanted %.3f %s', name, last, tmost, want, ok and 'ok' or 'BAD'))
end
run('wall ahead, not reached', 0.2, 40, 0.0, 1e-6)
run('muzzle 20 cm in the wall', 0.5, 40, -0.22, 1e-3)
run('deep, wrist at 90 cm', 0.9, 40, -0.62, 1e-3)
wall.rec = { massless = false, mass = 2.0 }
run('a light thing: no stop', 0.5, 40, 0.0, 1e-6)
wall.rec = { massless = false, mass = 80.0 }
run('too heavy to shove: stops', 0.5, 40, -0.22, 1e-3)
wall.staticOnly = true; wall.rec = nil
run('static, past the sweeps', 0.5, 40, -0.22, 1e-3)
wall.staticOnly = false
-- going away: the plane does not outlive the ray by more than its life
K.GUN_PLANES = {}
for i = 1, 40 do _G.NOW = _G.NOW + 0.022; K.gunWalls(0, {}, 0, 0, 0, 1, 0.5, 0.5, 1.3, 0.5, 0.5, 1.3, 0, 0, 0, 0, 0, 0, 1) end
local after
for i = 1, 20 do _G.NOW = _G.NOW + 0.022; after = select(2, K.gunWalls(0, {}, 0, 0, 0, 1, 0.5, 0.0, 1.3, 0.5, 0.0, 1.3, 0, 0, 0, 0, 0, 0, 1)) end
print(string.format('hand drawn back: push %.3f, planes %d', after, #K.GUN_PLANES))
if after ~= 0.0 or #K.GUN_PLANES ~= 0 then bad = bad + 1 end
print('bad ' .. bad)
os.exit(bad)
