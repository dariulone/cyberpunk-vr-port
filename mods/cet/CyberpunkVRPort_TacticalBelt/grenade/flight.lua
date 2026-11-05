-- CyberpunkVRPort — THE THROWN GRENADE, in flight.
--
-- WHY THE PORT FLIES ITS OWN. What the game launches is not the item you hand it: measured live, the
-- ItemObject is DESTROYED the instant the launch event is taken, and what flies is an entity of the
-- game's own. Ours therefore could not be seen -- a grenade's visual is skinned meshes only, and a
-- skinned mesh with no live skeleton draws as a ghost. That is the transparent thing on the picture,
-- and no amount of rendering-plane or setup-event work was ever going to fix it.
--
-- So the flight is ours and the explosion is the game's:
--
--     here            the port's own prop entity, plainly meshed and plainly visible, thrown on the
--                     hand's measured velocity and integrated frame by frame
--     at the end      VRPortDetonateGrenadeAt puts the REAL grenade at that point and sets it off,
--                     so the damage, the radius, the effect and the sound are the game's own
--
-- The integration is the reload module's falling magazine, which has been doing exactly this for a
-- while: a parabola, a swept ray against static world geometry, a tumble, and a bounce.

local M = {}
local E = {}

function M.setup(env)
    E = env
end

-- WHY STATIC-ONLY RAYS. Straight from the magazine drop, where it was measured: a plain ray from the
-- hand hits the WEAPON's own cooked collider six millimetres away -- the one this port put there for
-- the hands -- so anything thrown "landed" on the thing that threw it, at chest height. With
-- staticOnly the same ray misses and reaches the real floor. The player, their hands and every actor
-- go with it. The cost, stated plainly: a grenade falls THROUGH a simulated crate or a car bonnet.
-- ...AND WHY THE ORDER CHANGED. `Sight Blocker` is the preset whose whole purpose is to be stopped by
-- the things that block sight -- foliage first among them -- so a thrown grenade landed in bushes. A
-- bullet's preset is the honest model for a small heavy object: what stops a round stops a grenade.
local RAY_PRESETS = { "Bullet", "Camera", "Sight Blocker" }
local RAY_PICK, RAY_SYS = 1, nil

-- WHAT A GRENADE IS NOT STOPPED BY, matched against the hit's own physical material. Kept as a list of
-- fragments rather than exact names because a physmat is `foliage.physmat`, `grass_short.physmat` and
-- so on, and the point is the family, not the entry.
local SOFT = { "foliage", "grass", "leaf", "leaves", "bush", "plant", "vegetation", "tree", "fabric" }

local matSeen = 0

local function isSoft(mat)
    if mat == nil or mat == "" then return false end
    local m = string.lower(mat)
    for i = 1, #SOFT do
        if string.find(m, SOFT[i], 1, true) then return true end
    end
    return false
end

local function rayHit(x1, y1, z1, x2, y2, z2)
    if RAY_SYS == nil then
        local ok, s = pcall(function() return Game.GetSpatialQueriesSystem() end)
        RAY_SYS = (ok and s) or false
    end
    if not RAY_SYS then return nil end
    local a = Vector4.new(x1, y1, z1, 1.0)
    local b = Vector4.new(x2, y2, z2, 1.0)
    for n = 1, #RAY_PRESETS do
        local i = ((RAY_PICK - 2 + n) % #RAY_PRESETS) + 1
        local ok, hit, tr = pcall(function()
            local r1, r2 = RAY_SYS:SyncRaycastByQueryPreset(a, b, RAY_PRESETS[i], true)
            return r1, r2
        end)
        if ok and hit and tr then
            RAY_PICK = i
            local mat = ""
            pcall(function() mat = tostring(tr.material.value or tr.material) end)
            -- The first few materials go into the trace: what a bush actually reports is a fact worth
            -- having, and the list above was written from the family names, not from a measurement.
            if matSeen < 6 and E.trace then
                matSeen = matSeen + 1
                E.trace("flight: hit %s via %s", mat, RAY_PRESETS[i])
            end
            return { tr.position.x, tr.position.y, tr.position.z },
                   { tr.normal.x, tr.normal.y, tr.normal.z }, mat
        end
    end
    return nil
end

-- THE FIRST HARD THING ON THE RAY, not the first thing.
--
-- Ignoring a soft hit is not enough, and the symptom said so exactly: a grenade that landed on bushes went on
-- under the ground. A raycast returns its NEAREST hit and nothing else, so dropping that hit dropped the
-- whole ray -- foliage in front of the ground meant the ground was never tested at all, and the
-- grenade sailed through both.
--
-- So the ray is walked: hit a bush, step just past it, cast again from there to the same end point.
-- Four steps is plenty -- that is four layers of leaves in one frame's sweep -- and the cap is what
-- keeps a pathological case from looping.
local function rayFirstHard(x1, y1, z1, x2, y2, z2)
    local sx, sy, sz = x2 - x1, y2 - y1, z2 - z1
    local len = math.sqrt(sx * sx + sy * sy + sz * sz)
    if len < 1e-6 then return nil end
    local ux, uy, uz = sx / len, sy / len, sz / len
    local ax, ay, az = x1, y1, z1
    for _ = 1, 4 do
        local hp, hn, mat = rayHit(ax, ay, az, x2, y2, z2)
        if hp == nil then return nil end
        if not ((E.CFG.grenade_flight_soft ~= false) and isSoft(mat)) then
            return hp, hn, mat
        end
        local step = E.CFG.grenade_flight_soft_step or 0.05
        ax, ay, az = hp[1] + ux * step, hp[2] + uy * step, hp[3] + uz * step
        -- stepped past the far end: there is nothing hard on this sweep
        if ((x2 - ax) * ux + (y2 - ay) * uy + (z2 - az) * uz) <= 0.0 then return nil end
    end
    return nil
end

-- WHETHER THE FLOOR OF LAST RESORT IS REALLY THERE, asked once, on the frame a falling grenade crosses
-- the player's feet. It was taken on trust, and a grenade thrown off high ground landing on an invisible floor (the
-- high ground carried on, 2026-10-18) is what trust did: a grenade thrown off a ledge crosses that level past
-- the edge, missing everything, and lay down on the air at the height of the ledge.
--
-- So a ray goes straight down from just above that level, under the grenade and under the player, and
-- the two grounds are compared. These rays do not always see the floor that is drawn: in the metro
-- concourse (2026-10-18) the ground they find under the player is 0.48 m below the feet everywhere on
-- the walkway -- that is what the floor of last resort is for -- and 32 m below past the railing. Ground
-- under the grenade level with the ground under the feet and the floor stays; any lower -- a kerb is
-- 15 cm -- is a drop and the grenade falls on to it. Nothing under either, and the floor stays: a
-- place where the query system says nothing, or a player on a lift or a car, whose own floor these
-- rays do not see.
local function floorHolds(x, y, fz)
    local drop = E.CFG.grenade_flight_floor_drop or 0.05
    local depth = E.CFG.grenade_flight_floor_probe or 50.0
    local function groundBelow(gx, gy)
        local hp = rayFirstHard(gx, gy, fz + 0.1, gx, gy, fz - depth)
        return hp and hp[3] or nil
    end
    local below = groundBelow(x, y)
    if below == nil or below >= fz then return true end
    local p = nil
    pcall(function() p = Game.GetPlayer():GetWorldPosition() end)
    if p == nil then return true end
    local feet = groundBelow(p.x, p.y)
    local holds = feet == nil or below >= math.min(fz, feet) - drop
    if E.trace then E.trace("flight: falling past the feet at %.2f, ground %.2f below it, under the feet %s -> floor %s",
        fz, fz - below, feet and string.format("%.2f", fz - feet) or "none", tostring(holds)) end
    return holds
end

local function qmul(ax, ay, az, aw, bx, by, bz, bw)
    return aw * bx + ax * bw + ay * bz - az * by,
           aw * by - ax * bz + ay * bw + az * bx,
           aw * bz + ax * by - ay * bx + az * bw,
           aw * bw - ax * bx - ay * by - az * bz
end

-- EVERYTHING IN THE AIR RIGHT NOW. Each entry carries its own SLOT -- the one holding the real
-- grenade that will be detonated when this one lands -- so two in flight cannot be confused for each
-- other, and neither can be quietly forgotten.
local LIVE = {}

local function entityOf(f)
    if f.ent ~= nil then return f.ent end
    if f.id == nil then return nil end
    local e = nil
    pcall(function() e = Game.FindEntityByID(f.id) end)
    if IsDefined(e) then f.ent = e end
    return f.ent
end

local function despawn(f)
    local e = entityOf(f)
    if e ~= nil then pcall(function() exEntitySpawner.Despawn(e) end) end
    f.id, f.ent = nil, nil
end

function M.count()
    return #LIVE
end

function M.flying()
    return #LIVE > 0
end

-- Which detonation slots are spoken for. The caller picks a free one for the next throw.
function M.slotsInUse()
    local t = {}
    for i = 1, #LIVE do
        if LIVE[i].slot ~= nil then t[LIVE[i].slot] = true end
    end
    return t
end

function M.clearAll()
    for i = #LIVE, 1, -1 do
        despawn(LIVE[i])
        table.remove(LIVE, i)
    end
end

-- THROW IT. Position and orientation are world; the velocity is world too -- the caller measures the
-- hand in model space and rotates it out, because that is where the player's own travel has already
-- cancelled.
function M.launch(entPath, slot, px, py, pz, qi, qj, qk, qr, vx, vy, vz)
    if type(exEntitySpawner) == "nil" or entPath == nil then return false end
    local S = { slot = slot, t = 0.0, rest = nil,
                p = { px, py, pz }, v = { vx, vy, vz },
                q = { qi or 0.0, qj or 0.0, qk or 0.0, qr or 1.0 } }
    -- A THROWN GRENADE TUMBLES, and the axis is the one the throw itself implies: across the
    -- direction of travel, like anything let go of by a hand that was swinging. Speed scales with the
    -- throw so a gentle lob does not spin like a fastball.
    local sp = math.sqrt(vx * vx + vy * vy + vz * vz)
    local ax, ay, az = -vy, vx, 0.0
    local an = math.sqrt(ax * ax + ay * ay + az * az)
    if an < 1e-4 then ax, ay, az, an = 1.0, 0.0, 0.0, 1.0 end
    local rate = (E.CFG.grenade_flight_spin or 0.9) * sp
    S.w = { ax / an * rate, ay / an * rate, az / an * rate }
    S.t, S.rest = 0.0, nil

    -- THE FORM MATTERS. `WorldTransform.SetPosition(tr, WorldPosition...)` -- the static-call spelling
    -- -- fails silently inside a pcall and the spawn simply never happens, which is what the grenade still flying
    -- the old way was: the flight was attempted every throw and returned false every time. The reload
    -- module has been spawning magazines for months with the method form and a plain Vector4, so that
    -- is what this uses.
    local ok, err = pcall(function()
        local tr = WorldTransform.new()
        tr:SetPosition(Vector4.new(px, py, pz, 1.0))
        tr:SetOrientation(Quaternion.new(S.q[1], S.q[2], S.q[3], S.q[4]))
        S.id = exEntitySpawner.Spawn(entPath, tr, "")
    end)
    if not ok or S.id == nil then
        if E.trace then E.trace("flight: spawn failed -- %s", tostring(err)) end
        return false
    end
    LIVE[#LIVE + 1] = S
    return true
end

-- One step for ONE grenade. Returns true when it is finished.
local function step(S, h)
    S.t = S.t + h

    local fuse = E.CFG.grenade_fuse_s or 2.5
    if S.rest ~= nil then
        -- Landed. It sits where it stopped until the fuse runs out, which is what a grenade does.
        if S.t >= fuse or (S.t - S.rest) >= (E.CFG.grenade_fuse_after_hit_s or 1.2) then
            return true
        end
    else
        local g = E.CFG.grenade_flight_gravity or -10.5     -- the grenade's own, from its record
        S.v[3] = S.v[3] + g * h
        local drag = 1.0 - (E.CFG.grenade_flight_drag or 0.05) * h
        if drag < 0.0 then drag = 0.0 end
        S.v[1], S.v[2], S.v[3] = S.v[1] * drag, S.v[2] * drag, S.v[3] * drag

        -- the tumble
        if (S.w[1] ^ 2 + S.w[2] ^ 2 + S.w[3] ^ 2) > 1e-8 then
            local a1, a2, a3, a4 = qmul(S.w[1] * 0.5 * h, S.w[2] * 0.5 * h, S.w[3] * 0.5 * h, 0.0,
                                        S.q[1], S.q[2], S.q[3], S.q[4])
            local nq = { S.q[1] + a1, S.q[2] + a2, S.q[3] + a3, S.q[4] + a4 }
            local l = math.sqrt(nq[1] ^ 2 + nq[2] ^ 2 + nq[3] ^ 2 + nq[4] ^ 2)
            if l > 1e-9 then S.q = { nq[1] / l, nq[2] / l, nq[3] / l, nq[4] / l } end
        end

        local nx = S.p[1] + S.v[1] * h
        local ny = S.p[2] + S.v[2] * h
        local nz = S.p[3] + S.v[3] * h

        -- SWEPT, not sampled. A grenade at 20 m/s covers a fifth of a metre per frame, and a point
        -- test would walk it straight through a wall.
        local r = E.CFG.grenade_flight_radius or 0.035
        local sx, sy, sz = nx - S.p[1], ny - S.p[2], nz - S.p[3]
        local sl = math.sqrt(sx * sx + sy * sy + sz * sz)
        local hp, hn, mat = nil, nil, nil
        if sl > 1e-4 then
            local k = (sl + r) / sl
            hp, hn, mat = rayFirstHard(S.p[1], S.p[2], S.p[3],
                                       S.p[1] + sx * k, S.p[2] + sy * k, S.p[3] + sz * k)
        end

        -- A FLOOR OF LAST RESORT, at the player's own feet. Ported from the magazine drop, where it
        -- earns its place the same way: static-only rays miss things, and a grenade that misses
        -- everything falls forever. It is only ever used while DESCENDING through that level, and only
        -- where nothing lower is there to land on (floorHolds), so a throw off a balcony uses the real
        -- geometry all the way down.
        if hp == nil and S.v[3] < 0.0 and (E.CFG.grenade_flight_floor ~= false) then
            local fz = nil
            pcall(function()
                local p = Game.GetPlayer():GetWorldPosition()
                if p ~= nil then fz = p.z + (E.CFG.grenade_flight_floor_lift or 0.02) end
            end)
            if fz ~= nil and (nz - r) <= fz and S.p[3] >= fz and floorHolds(nx, ny, fz) then
                hp, hn, mat = { nx, ny, fz }, { 0.0, 0.0, 1.0 }, "floor"
            end
        end

        if hp ~= nil then
            local e = E.CFG.grenade_flight_bounce or 0.35
            local vn = S.v[1] * hn[1] + S.v[2] * hn[2] + S.v[3] * hn[3]
            S.p = { hp[1] + hn[1] * r, hp[2] + hn[2] * r, hp[3] + hn[3] * r }
            S.v = { (S.v[1] - 2.0 * vn * hn[1]) * e,
                    (S.v[2] - 2.0 * vn * hn[2]) * e,
                    (S.v[3] - 2.0 * vn * hn[3]) * e }
            S.w = { S.w[1] * 0.5, S.w[2] * 0.5, S.w[3] * 0.5 }
            -- Slow enough on the surface and it is simply lying there; that starts the short fuse.
            local left = math.sqrt(S.v[1] ^ 2 + S.v[2] ^ 2 + S.v[3] ^ 2)
            if left < (E.CFG.grenade_flight_rest_ms or 1.2) then
                S.rest = S.t
                S.v = { 0.0, 0.0, 0.0 }
                S.w = { 0.0, 0.0, 0.0 }
            end
        else
            S.p = { nx, ny, nz }
        end

        if S.t >= fuse then return true end
    end

    local e = entityOf(S)
    if e ~= nil then
        pcall(function()
            local tr = WorldTransform.new()
            tr:SetPosition(Vector4.new(S.p[1], S.p[2], S.p[3], 1.0))
            tr:SetOrientation(Quaternion.new(S.q[1], S.q[2], S.q[3], S.q[4]))
            Game.VREntitySetWorldTransform(e, tr)
        end)
    end
    return false
end

-- ADVANCE EVERYTHING. Returns the ones that finished this frame -- each with the point it ended at and
-- the slot holding its real grenade -- and the caller detonates them.
function M.tick(dt)
    if #LIVE == 0 then return nil end
    local h = (dt and dt > 0.0) and dt or 0.016
    if h > 0.04 then h = 0.04 end
    local done = nil
    for i = #LIVE, 1, -1 do
        local f = LIVE[i]
        if step(f, h) then
            done = done or {}
            done[#done + 1] = { slot = f.slot, x = f.p[1], y = f.p[2], z = f.p[3] }
            despawn(f)
            table.remove(LIVE, i)
        end
    end
    return done
end

return M
