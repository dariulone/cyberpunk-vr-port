-- HOW MUCH FREE SPACE IS IN FRONT OF THE EYES, for the HUD panels to stay in front of the world
-- (include/Runtimes/HudClearance.hpp). A fan of rays across the panels' area, 5 across and 3 down, from the
-- camera; the nearest hit is converted to a depth along the camera's forward axis, which is where a flat
-- panel facing the player would touch it. 99 = nothing hit within MAX_RAY (open space).
--
-- SyncRaycastByQueryPreset with staticOnly = true, as the reload module measured: it is the raycast form that
-- answers from script, and static-only skips the player's own body and hands (the rays start inside V's head).
-- Static world geometry only: walls, doors, furniture. PEOPLE come from PlayerPuppet:VRHudPeopleDepth
-- (mods/redscript/CyberpunkVRPort_HudClearance): the targeting system's NPC query, each one a standing body.
local M = { clearance = 0, error = nil }
local elapsed = 0
local PRESETS = { 'Sight Blocker', 'Camera', 'Bullet' }
local pick = 1
local MAX_RAY = 5.5
local PEOPLE_RANGE = 3.0
local H = { -0.62, -0.31, 0.0, 0.31, 0.62 }   -- tangents: about +/-32 deg across
local V = { -0.40, 0.0, 0.36 }                -- about -22 / +20 deg up and down

local function cross(a, b) return { x = a.y * b.z - a.z * b.y, y = a.z * b.x - a.x * b.z, z = a.x * b.y - a.y * b.x } end
local function norm(a) local l = math.sqrt(a.x * a.x + a.y * a.y + a.z * a.z); if l < 1e-6 then return nil end; return { x = a.x / l, y = a.y / l, z = a.z / l } end

local function ray(sq, o, d)
    local a = Vector4.new(o.x, o.y, o.z, 1)
    local b = Vector4.new(o.x + d.x * MAX_RAY, o.y + d.y * MAX_RAY, o.z + d.z * MAX_RAY, 1)
    for n = 1, #PRESETS do
        local i = ((pick - 2 + n) % #PRESETS) + 1
        local hit, tr = sq:SyncRaycastByQueryPreset(a, b, PRESETS[i], true)
        if hit and tr and tr.position then pick = i; return tr.position end
        if n == 1 and not hit then return nil end   -- the remembered preset answered "nothing": trust it
    end
    return nil
end

local function measure()
    local player = Game.GetPlayer()
    if not player then return 0 end
    local defs = GetAllBlackboardDefs()
    local boards = Game.GetBlackboardSystem()
    local menu = boards and boards:Get(defs.UI_System)
    if not menu or menu:GetBool(defs.UI_System.IsInMenu) then return 0 end
    local cam = Game.GetCameraSystem()
    local sq = Game.GetSpatialQueriesSystem()
    if not cam or not sq then return 0 end
    local t = Transform.new(); cam:GetActiveCameraWorldTransform(t)
    local o, f = t.position, cam:GetActiveCameraForward()
    if not o or not f then return 0 end
    local r = norm(cross(f, { x = 0, y = 0, z = 1 }))       -- camera right (world up = +Z)
    if not r then return 0 end
    local u = cross(r, f)                                    -- camera up
    local nearest = 99.0
    for _, v in ipairs(V) do
        for _, h in ipairs(H) do
            local d = norm({ x = f.x + r.x * h + u.x * v, y = f.y + r.y * h + u.y * v, z = f.z + r.z * h + u.z * v })
            local p = d and ray(sq, o, d)
            if p then
                local depth = (p.x - o.x) * f.x + (p.y - o.y) * f.y + (p.z - o.z) * f.z
                if depth > 0.05 and depth < nearest then nearest = depth end
            end
        end
    end
    -- People: the same area, as tangents of the fan's outermost rays.
    -- (Called under pcall: with the script missing or not compiled the call errors, and the rays still count.)
    local ok, d = pcall(function()
        return player:VRHudPeopleDepth(Vector4.new(o.x, o.y, o.z, 1), Vector4.new(f.x, f.y, f.z, 0),
            Vector4.new(r.x, r.y, r.z, 0), Vector4.new(u.x, u.y, u.z, 0), H[#H], -V[1], PEOPLE_RANGE)
    end)
    if ok and type(d) == 'number' and d > 0.05 and d < nearest then nearest = d end
    return nearest
end

function M.tick(dt, overlayOpen)
    elapsed = elapsed + math.max(tonumber(dt) or 0, 0)
    if elapsed < 0.033 then return end
    elapsed = 0
    if type(SetVRHudClearance) ~= 'function' then return end
    local clearance = 0
    if not overlayOpen then
        local ok, c = pcall(measure)
        M.error = not ok and tostring(c) or nil
        clearance = ok and tonumber(c) or 0
    end
    M.clearance = clearance
    SetVRHudClearance(clearance)
end

return M
