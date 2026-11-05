-- THE SHOTGUNS, on their own.
--
-- `reload.lua` is built around a slide and a box magazine, and `revolver.lua` around a cylinder. A shotgun is
-- neither, and it is two different guns besides:
--
--   * a BREAK-ACTION (Rostovic Satara / Palica / Igla / Testera). The right thumb pushes the top lever over, the
--     barrels drop open about the hinge, the fired cases come out of their chambers under their own weight --
--     the same friction model the Overture's cylinder uses -- and the live ones stay. The left hand takes as many
--     shells off the belt as there are empty chambers, the magnet seats them one at a time, and a flick of the
--     wrist swings the barrels shut again.
--   * a PUMP (Budget Carnage, Constitutional Tactician). Shells go in one at a time through the loading port under
--     the receiver; the fore-end is pulled back to throw the fired hull out of the ejection port and pushed
--     forward to chamber the next round. A dry gun takes its first shell straight into the open action.
--
-- Everything here was measured off the game's own reloads with VRIK switched off (the recorder takes under
-- build/shotgun-reload-20261019/takes): hinge and lever angles, the travel of each fore-end, how each hand holds
-- one shell or two, the loading-port line, the chamber positions. The configs say where each number came from.
--
-- One entry point, `M.frame`, which the parent calls instead of its own body for a weapon whose config has a
-- `shotgun` section. `H` carries the parent's helpers -- pose application, sounds, ammunition, the belt, the
-- carrier and item routes -- so nothing here reaches back into reload.lua.
local M = {}

local G = 9.81
local at2 = math.atan2 or math.atan

-- ------------------------------------------------------------------ math
local function len3(x, y, z) return math.sqrt(x * x + y * y + z * z) end

local function qmul(ax, ay, az, aw, bx, by, bz, bw)
    return aw * bx + ax * bw + ay * bz - az * by,
           aw * by - ax * bz + ay * bw + az * bx,
           aw * bz + ax * by - ay * bx + az * bw,
           aw * bw - ax * bx - ay * by - az * bz
end

local function qrot(i, j, k, r, x, y, z)
    local tx = 2.0 * (j * z - k * y)
    local ty = 2.0 * (k * x - i * z)
    local tz = 2.0 * (i * y - j * x)
    return x + r * tx + (j * tz - k * ty),
           y + r * ty + (k * tx - i * tz),
           z + r * tz + (i * ty - j * tx)
end

local function qnorm(i, j, k, r)
    local l = math.sqrt(i * i + j * j + k * k + r * r)
    if l < 1e-9 then return 0.0, 0.0, 0.0, 1.0 end
    return i / l, j / l, k / l, r / l
end

-- The shortest turn taking unit vector a onto unit vector b. A shell is round: it has no roll of its own that
-- matters, so the magnet only ever has to SWING it onto the chamber's line and can leave the roll to the hand.
local function qswing(ax, ay, az, bx, by, bz)
    local d = ax * bx + ay * by + az * bz
    if d < -0.999999 then
        local px, py, pz = 1.0, 0.0, 0.0
        if math.abs(ax) > 0.9 then px, py, pz = 0.0, 1.0, 0.0 end
        local cx, cy, cz = ay * pz - az * py, az * px - ax * pz, ax * py - ay * px
        local l = len3(cx, cy, cz)
        return cx / l, cy / l, cz / l, 0.0
    end
    return qnorm(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx, 1.0 + d)
end

local function qnlerp(a1, a2, a3, a4, b1, b2, b3, b4, t)
    local s = ((a1 * b1 + a2 * b2 + a3 * b3 + a4 * b4) < 0.0) and -1.0 or 1.0
    return qnorm(a1 + (b1 * s - a1) * t, a2 + (b2 * s - a2) * t, a3 + (b3 * s - a3) * t, a4 + (b4 * s - a4) * t)
end

local function smooth(u)
    if u <= 0.0 then return 0.0 elseif u >= 1.0 then return 1.0 end
    return u * u * (3.0 - 2.0 * u)
end

local function clamp(v, lo, hi)
    if v < lo then return lo elseif v > hi then return hi end
    return v
end

-- FRAMES are seven numbers, { x, y, z, qi, qj, qk, qr }, and these four are all that is ever done with them.
local function fset(o, x, y, z, i, j, k, r)
    o[1], o[2], o[3], o[4], o[5], o[6], o[7] = x, y, z, i, j, k, r
    return o
end

local function fcomp(A, B, o)
    local x, y, z = qrot(A[4], A[5], A[6], A[7], B[1], B[2], B[3])
    local i, j, k, r = qmul(A[4], A[5], A[6], A[7], B[4], B[5], B[6], B[7])
    return fset(o or {}, A[1] + x, A[2] + y, A[3] + z, i, j, k, r)
end

local function finv(A, o)
    local i, j, k, r = -A[4], -A[5], -A[6], A[7]
    local x, y, z = qrot(i, j, k, r, -A[1], -A[2], -A[3])
    return fset(o or {}, x, y, z, i, j, k, r)
end

-- A MESH'S OWN BIND, TAKEN BACK OUT. A weapon's shell meshes are skinned, and their vertices are stored where
-- the bone sits at rest -- the Satara's left shell 18 mm left of the magazine root, turned by the rig's usual
-- (0, .707, .707, 0). Drawn by a PLAIN component (which is what lets it draw at all without a skeleton) at a
-- transform T, a vertex lands at T * bind * v. To put the shell where a bone frame F says it is, T = F * bind^-1:
--     q_T = q_F * conj(q_bind)       p_T = p_F - q_T * p_bind
local function unbind(F, bp, bq, o)
    local i, j, k, r = qmul(F[4], F[5], F[6], F[7], -bq[1], -bq[2], -bq[3], bq[4])
    local x, y, z = qrot(i, j, k, r, bp[1], bp[2], bp[3])
    return fset(o or {}, F[1] - x, F[2] - y, F[3] - z, i, j, k, r)
end

-- ------------------------------------------------------------------ reading the game
local CN = {}
local function cname(n)
    local c = CN[n]
    if not c then c = CName.new(n); CN[n] = c end
    return c
end

local function slotTr(comp, name) return comp:GetSlotTransform(name) end
local function slotWorld(comp, name, o)
    if not comp then return nil end
    local ok, found, tr = pcall(slotTr, comp, cname(name))
    if not (ok and found and tr) then return nil end
    local okp, v = pcall(WorldPosition.ToVector4, tr.Position)
    if not (okp and v) then return nil end
    local q = tr.Orientation
    return fset(o or {}, v.x, v.y, v.z, q.i, q.j, q.k, q.r)
end

local function playerWorld(o)
    local ok, x, y, z, i, j, k, r = pcall(function()
        local pl = Game.GetPlayer()
        local p = pl:GetWorldPosition()
        local q = pl:GetWorldOrientation()
        return p.x, p.y, p.z, q.i, q.j, q.k, q.r
    end)
    if not ok or x == nil then return nil end
    return fset(o or {}, x, y, z, i, j, k, r)
end

local function weaponWorld(weapon, o)
    local ok, x, y, z, i, j, k, r = pcall(function()
        local p = weapon:GetWorldPosition()
        local q = weapon:GetWorldOrientation()
        return p.x, p.y, p.z, q.i, q.j, q.k, q.r
    end)
    if not ok or x == nil then return nil end
    return fset(o or {}, x, y, z, i, j, k, r)
end

-- World -> the player's own frame, which is the space every hand native speaks.
local function toModel(PW, A, o)
    local ci, cj, ck, cr = -PW[4], -PW[5], -PW[6], PW[7]
    local x, y, z = qrot(ci, cj, ck, cr, A[1] - PW[1], A[2] - PW[2], A[3] - PW[3])
    local i, j, k, r = qmul(ci, cj, ck, cr, A[4], A[5], A[6], A[7])
    return fset(o or {}, x, y, z, i, j, k, r)
end

-- The RAW tracked wrist, model space. Every magnet weight in this file is measured from it and never from the
-- drawn hand: measuring from the drawn hand is a loop -- the pin moves the hand, the hand moves the shell, the
-- shell moves the weight -- and the reload module learnt that one the hard way.
local function wristRaw(side, o)
    if type(VRHandRawModel) ~= 'function' or type(VRHandRawRot) ~= 'function' then return nil end
    local p = VRHandRawModel(side)
    local q = VRHandRawRot(side)
    if not (p and q) or (p.w or 1.0) < 0.5 then return nil end
    return fset(o or {}, p.x, p.y, p.z, q.i, q.j, q.k, q.r)
end

-- WRIST -> HOLD SLOT, read live off the bones (23 -> 26 left, 24 -> 28 right): rigid, so neither the collision
-- clamp nor our own pin changes it. The carry offsets are all measured in the hold slot.
local function wristToSlot(side, o)
    if type(VRBoneModelPos) ~= 'function' or type(VRBoneModelRot) ~= 'function' then return nil end
    local bw, bs = (side == 0) and 23 or 24, (side == 0) and 26 or 28
    local pw, qw, ps, qs = VRBoneModelPos(bw), VRBoneModelRot(bw), VRBoneModelPos(bs), VRBoneModelRot(bs)
    if not (pw and qw and ps and qs) or (pw.w or 0) < 0.5 or (ps.w or 0) < 0.5 then return nil end
    local x, y, z = qrot(-qw.i, -qw.j, -qw.k, qw.r, ps.x - pw.x, ps.y - pw.y, ps.z - pw.z)
    local i, j, k, r = qmul(-qw.i, -qw.j, -qw.k, qw.r, qs.i, qs.j, qs.k, qs.r)
    return fset(o or {}, x, y, z, i, j, k, r)
end

local function r3Down() return (GetVRSharedSlot(159) or 0.0) > 0.5 end

-- ------------------------------------------------------------------ the hand
-- THE PIN. A weight of 0 lets the hand go; anything else draws the wrist from where tracking has it toward the
-- target by that weight, position and rotation both.
local pinS = { side = nil }
local function unpin()
    if pinS.side == nil then return end
    local s = pinS.side
    pcall(function()
        VRHandStopModel(s, false, Vector4.new(0, 0, 0, 0))
        if type(VRHandStopRot) == 'function' then VRHandStopRot(s, 0, 0, 0, 0, 1) end
    end)
    pinS.side = nil
end

local function pin(side, w, Wr, T)
    if w <= 0.0 or not (Wr and T) then unpin(); return nil end
    if pinS.side ~= nil and pinS.side ~= side then unpin() end
    local x = Wr[1] + (T[1] - Wr[1]) * w
    local y = Wr[2] + (T[2] - Wr[2]) * w
    local z = Wr[3] + (T[3] - Wr[3]) * w
    local i, j, k, r = qnlerp(Wr[4], Wr[5], Wr[6], Wr[7], T[4], T[5], T[6], T[7], w)
    pcall(function()
        VRHandStopModel(side, true, Vector4.new(x, y, z, 1.0))
        if type(VRHandStopRot) == 'function' then VRHandStopRot(side, 1, i, j, k, r) end
    end)
    pinS.side = side
    return side
end

-- THE PULL-IN, ON A SPRING: the weight a grab draws the wrist onto its grip with, released from 0 toward 1 the
-- moment the hand takes hold -- the same 180 ms, slightly under-damped spring the slide racking uses, and for
-- the same reason (a grip that lands at full weight is a teleport). Its own copy: the parent resets its spring
-- every frame its slide is not held, which on a shotgun is every frame.
local springS = { on = false, x = 0.0, v = 0.0 }
local function spring(dt)
    if not springS.on then springS.on, springS.x, springS.v = true, 0.0, 0.0 end
    local wn = 6.283185307 / 0.180
    local d = clamp(dt or 0.016, 0.0, 0.1)
    local n = math.min(64, math.floor(d / 0.002) + 1)
    local h = d / n
    for _ = 1, n do
        springS.v = springS.v + (-wn * wn * (springS.x - 1.0) - 2.0 * 0.75 * wn * springS.v) * h
        springS.x = springS.x + springS.v * h
    end
    springS.x = clamp(springS.x, 0.0, 1.0)
    return springS.x
end
local function springReset() springS.on = false end

-- The wrist that puts the hold slot at frame Sl: wrist = Sl * conj(rel).
local function wristFor(Sl, Rel, o)
    local i, j, k, r = qmul(Sl[4], Sl[5], Sl[6], Sl[7], -Rel[4], -Rel[5], -Rel[6], Rel[7])
    local x, y, z = qrot(i, j, k, r, Rel[1], Rel[2], Rel[3])
    return fset(o or {}, Sl[1] - x, Sl[2] - y, Sl[3] - z, i, j, k, r)
end

-- FINGERS ON THE FREE HAND, faded. One pose at a time; a change of pose on the same hand re-sends at the blend
-- it is already at, so a switch never flashes the new pose at full strength for a frame.
local finS = { hand = nil, key = nil, a = 0.0, dir = 0 }
local function finWant(H, hand, key)
    local pose = key and H.pose(key)
    if not pose then return end
    if finS.hand ~= hand or finS.key ~= key then
        if finS.hand ~= nil and finS.hand ~= hand then H.clearGrip(finS.hand); finS.a = 0.0 end
        H.applyGrip(hand, pose, finS.a)
        finS.hand, finS.key = hand, key
    end
    finS.dir = 1
end
local function finOff() if finS.hand ~= nil then finS.dir = -1 end end
local function finReset(H)
    if finS.hand ~= nil then H.clearGrip(finS.hand) end
    finS.hand, finS.key, finS.a, finS.dir = nil, nil, 0.0, 0
end
local function finTick(H, dt, T)
    if finS.hand == nil or finS.dir == 0 then return end
    finS.a = finS.a + finS.dir * dt / (T or 0.15)
    if finS.a >= 1.0 then finS.a, finS.dir = 1.0, 0
    elseif finS.a <= 0.0 then finReset(H); return end
    if type(VRReloadFingerBlend) == 'function' then
        local h, a = finS.hand, finS.a
        pcall(function() VRReloadFingerBlend(h, a) end)
    end
end

-- ------------------------------------------------------------------ loose bodies: cases, hulls, dropped shells
--
-- ONE LIST FOR EVERYTHING THAT LEAVES THE GUN. Each body is the SHELL'S OWN FRAME -- origin at the rim, +Z along
-- the shell toward its mouth -- integrated about its centre, and drawn through `unbind` so the mesh's baked
-- chamber offset is taken back out. They fall, they hit the static world (the same staticOnly ray the magazines
-- use: a shell must land on the floor and not on the gun it just left), they bounce once or twice and lie down.
local bodies = {}
local MAXB = 12

local function bodyDespawn(H, b)
    if not b.id then return end
    local ok, e = pcall(function() return Game.FindEntityByID(b.id) end)
    if ok and e then pcall(function() exEntitySpawner.Despawn(e) end)
    elseif H and H.litter then H.litter(b.id) end
    b.id = nil
end

local function bodyClear(H)
    for i = 1, #bodies do bodyDespawn(H, bodies[i]) end
    bodies = {}
end

-- F is the shell's frame in the WORLD at release, v its velocity, w (rad/s) about axis wa.
local function bodySpawn(H, ent, bind, bq, len, F, vx, vy, vz, w, wa)
    if type(exEntitySpawner) == 'nil' or not ent then return nil end
    if #bodies >= MAXB then bodyDespawn(H, bodies[1]); table.remove(bodies, 1) end
    local T = unbind(F, bind, bq)
    local id = nil
    pcall(function()
        local tr = WorldTransform.new()
        tr:SetPosition(Vector4.new(T[1], T[2], T[3], 1.0))
        tr:SetOrientation(Quaternion.new(T[4], T[5], T[6], T[7]))
        id = exEntitySpawner.Spawn(ent, tr, '')
    end)
    if not id then return nil end
    local half = (len or 0.07) * 0.5
    local cx, cy, cz = qrot(F[4], F[5], F[6], F[7], 0.0, 0.0, half)
    local b = {
        id = id, bind = bind, bq = bq, half = half,
        c = { F[1] + cx, F[2] + cy, F[3] + cz },              -- centre of mass, world
        q = { F[4], F[5], F[6], F[7] },
        v = { vx or 0.0, vy or 0.0, vz or 0.0 },
        w = w or 0.0, wa = wa or { 1.0, 0.0, 0.0 },
        t = 0.0, life = 7.0, rest = false, hits = 0,
    }
    bodies[#bodies + 1] = b
    return b
end

local BODY = { radius = 0.012, bounce = 0.28, friction = 0.55, restV = 0.35, settle = 0.20 }

local function bodyPlace(b)
    local e = Game.FindEntityByID(b.id)
    if not e then return end
    local hx, hy, hz = qrot(b.q[1], b.q[2], b.q[3], b.q[4], 0.0, 0.0, b.half)
    local F = fset(b.F or {}, b.c[1] - hx, b.c[2] - hy, b.c[3] - hz, b.q[1], b.q[2], b.q[3], b.q[4])
    b.F = F
    local T = unbind(F, b.bind, b.bq, b.T)
    b.T = T
    pcall(function()
        local tr = WorldTransform.new()
        tr:SetPosition(Vector4.new(T[1], T[2], T[3], 1.0))
        tr:SetOrientation(Quaternion.new(T[4], T[5], T[6], T[7]))
        Game.VREntitySetWorldTransform(e, tr)
    end)
end

local function bodyTick(H, dt)
    if #bodies == 0 then return end
    for n = #bodies, 1, -1 do
        local b = bodies[n]
        b.t = b.t + dt
        if not b.id or b.t >= b.life then
            bodyDespawn(H, b)
            table.remove(bodies, n)
        elseif not b.rest then
            local px, py, pz = b.c[1], b.c[2], b.c[3]
            b.v[3] = b.v[3] - G * dt
            local nx, ny, nz = px + b.v[1] * dt, py + b.v[2] * dt, pz + b.v[3] * dt
            -- the turn it earned leaving the chamber, about its centre; nothing in the air changes it
            if b.w ~= 0.0 then
                local h = b.w * dt * 0.5
                local s = math.sin(h)
                local q1, q2, q3, q4 = qmul(b.wa[1] * s, b.wa[2] * s, b.wa[3] * s, math.cos(h),
                                            b.q[1], b.q[2], b.q[3], b.q[4])
                b.q[1], b.q[2], b.q[3], b.q[4] = qnorm(q1, q2, q3, q4)
            end
            local hit, nrm = nil, nil
            if H.rayHit then hit, nrm = H.rayHit(px, py, pz + 0.02, nx, ny, nz - BODY.radius) end
            if hit and nrm then
                b.hits = b.hits + 1
                nx, ny, nz = hit[1] + nrm[1] * BODY.radius, hit[2] + nrm[2] * BODY.radius, hit[3] + nrm[3] * BODY.radius
                local vn = b.v[1] * nrm[1] + b.v[2] * nrm[2] + b.v[3] * nrm[3]
                if vn < 0.0 then
                    local tx, ty, tz = b.v[1] - vn * nrm[1], b.v[2] - vn * nrm[2], b.v[3] - vn * nrm[3]
                    local e = (-vn > 0.6) and BODY.bounce or 0.0
                    b.v[1] = tx * (1.0 - BODY.friction) - vn * e * nrm[1]
                    b.v[2] = ty * (1.0 - BODY.friction) - vn * e * nrm[2]
                    b.v[3] = tz * (1.0 - BODY.friction) - vn * e * nrm[3]
                end
                b.w = b.w * 0.45
                if len3(b.v[1], b.v[2], b.v[3]) < BODY.restV then
                    -- IT LIES DOWN. A shell standing on its rim reads as a prop placed by hand; one on its
                    -- side reads as dropped. The turn is the swing that lays its axis into the floor.
                    local ax, ay, az = qrot(b.q[1], b.q[2], b.q[3], b.q[4], 0.0, 0.0, 1.0)
                    local d = ax * nrm[1] + ay * nrm[2] + az * nrm[3]
                    local fx, fy, fz = ax - d * nrm[1], ay - d * nrm[2], az - d * nrm[3]
                    local fl = len3(fx, fy, fz)
                    if fl < 1e-4 then fx, fy, fz, fl = 1.0, 0.0, 0.0, 1.0 end
                    local s1, s2, s3, s4 = qswing(ax, ay, az, fx / fl, fy / fl, fz / fl)
                    local q1, q2, q3, q4 = qmul(s1, s2, s3, s4, b.q[1], b.q[2], b.q[3], b.q[4])
                    b.lieQ = { qnorm(q1, q2, q3, q4) }
                    b.lieFrom = { b.q[1], b.q[2], b.q[3], b.q[4] }
                    b.lieT = 0.0
                    b.rest = true
                    b.v[1], b.v[2], b.v[3] = 0.0, 0.0, 0.0
                end
            end
            b.c[1], b.c[2], b.c[3] = nx, ny, nz
            bodyPlace(b)
        elseif b.lieT and b.lieT < BODY.settle then
            b.lieT = b.lieT + dt
            local u = smooth(b.lieT / BODY.settle)
            local f, l = b.lieFrom, b.lieQ
            b.q[1], b.q[2], b.q[3], b.q[4] = qnlerp(f[1], f[2], f[3], f[4], l[1], l[2], l[3], l[4], u)
            bodyPlace(b)
        end
    end
end

-- ------------------------------------------------------------------ shared bits of state
local S = {
    key = nil,          -- the weapon entity these numbers belong to
    kind = nil,
    trg = 0,
    owned = nil,        -- the wrist this module pinned this frame (published by the parent)
    diag = '-',
    flickPeak = 0.0,
}

local function writeOff(which, bone)
    if type(VRRigWriteOff) == 'function' then pcall(function() VRRigWriteOff(which, bone) end) end
end

local function trackSet(which, idx, v)
    if type(VRRigTrackWrite) == 'function' then pcall(function() VRRigTrackWrite(which, idx, v, 1) end) end
end

local function trackFree(which, idx)
    if type(VRRigTrackWrite) == 'function' then pcall(function() VRRigTrackWrite(which, idx, 1.0, 0) end) end
end

-- ============================================================================ BREAK-ACTION
--
-- act: closed -> unlatch (thumb on the lever) -> opening (barrels falling) -> open -> closing -> closed
local B = {}

local function bChambersFromAmmo(sc, n)
    -- The gun's own count is the only record there is: n live rounds, and they are the LAST n in the firing
    -- order -- the left barrel goes first, so a gun fired once has a fired left chamber and a live right one.
    local ch = sc.chambers
    B.ch = B.ch or {}
    local live = n or 0
    for k = #ch, 1, -1 do
        local c = B.ch[k] or {}
        B.ch[k] = c
        if live > 0 then c.st = 'live'; live = live - 1 else c.st = 'spent' end
        c.slide, c.sv, c.out = 0.0, 0.0, false
    end
end

local function bRelease(H, sc)
    -- every bone and track this module ever writes on a break-action, handed back to the animation
    local fw = sc.frameWhich or 1
    writeOff(fw, sc.hinge.bone)
    if sc.lever then writeOff(sc.lever.which or 2, sc.lever.bone) end
    for _, rd in ipairs(sc.riders or {}) do writeOff(rd.which or fw, rd.bone) end
    local mw = sc.magWhich or 0
    for _, c in ipairs(sc.chambers) do
        trackFree(mw, c.live.track)
        if c.spent then trackFree(mw, c.spent.track); writeOff(mw, c.spent.bone) end
    end
    B.wrote = false
end

local function bHeldDrop(H, sc, throw)
    local h = B.held
    if not h then return end
    -- SHELLS LET GO OF fall where they were, as rounds: they never went into the gun, so they cost nothing.
    if throw and h.world then
        for _, s in ipairs(h.list) do
            if s.F and sc.roundEntity then
                bodySpawn(H, sc.roundEntity, sc.roundBind or sc.chambers[1].pos, sc.bindRot, sc.shellLen,
                          s.F, h.vx or 0.0, h.vy or 0.0, h.vz or 0.0, 0.0, nil)
            end
        end
    end
    H.itemDrop(h.side, (h.side == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight')
    B.held = nil
    -- and whatever the magnet had caught goes with them: a catch left behind kept the gun "busy" and the flick
    -- could never shut it again
    B.magnet, B.near = nil, nil
    unpin()
    finOff()
    H.beltTaken(false)
end

local function bResetAll(H, sc, weapon)
    if sc and B.wrote then bRelease(H, sc) end
    if B.held and sc then bHeldDrop(H, sc, false) end
    B.held = nil
    if B.thumbOn and B.holder ~= nil then H.clearGrip(B.holder) end
    B.thumbOn, B.thumbT = false, nil
    B.act, B.t, B.hinge, B.hv, B.lever, B.leverV = 'closed', 0.0, 0.0, 0.0, 0.0, 0.0
    B.wrote = false
    B.qPrev = nil
    B.magnet = nil
    if sc and weapon then bChambersFromAmmo(sc, H.magAmmo(weapon) or 0) end
end

-- THE THUMB ON THE TOP LEVER, keyframed on the hand that HOLDS the gun -- three joints wide, so the grip itself
-- is untouched. Out of the game's own empty reload (satara_empty_c, right hand): the thumb lifts off the grip at
-- 0.88, is straight out with the lever over by 1.07, holds through the barrels dropping and comes back at 1.27.
local function bThumb(H, sc, holder, dt)
    local th = sc.thumb
    if not (th and B.thumbT) then return end
    B.thumbT = B.thumbT + dt
    local RT, PT, HT, AT = th.reach or 0.10, th.pressTime or 0.08, th.hold or 0.30, th.away or 0.18
    local t = B.thumbT
    local KEY = B.thumbKey
    if not KEY then
        KEY = {
            { 0.0,               th.over,  0.0 },
            { RT,                th.over,  1.0 },
            { RT + PT,           th.press, 1.0 },
            { RT + PT + HT,      th.press, 1.0 },
            { RT + PT + HT + AT, th.press, 0.0 },
        }
        B.thumbKey = KEY
    end
    if t >= KEY[#KEY][1] then
        if B.thumbOn then H.clearGrip(holder) end
        B.thumbOn, B.thumbT = false, nil
        return
    end
    local a, b, u = KEY[1], KEY[2], 0.0
    for k = 1, #KEY - 1 do
        if t >= KEY[k][1] and t <= KEY[k + 1][1] then
            a, b = KEY[k], KEY[k + 1]
            local span = b[1] - a[1]
            u = (span > 1e-6) and (t - a[1]) / span or 0.0
            break
        end
    end
    local us = smooth(u)
    local pose = H.mixPose(H.pose(a[2]), H.pose(b[2]), us)
    if pose then
        H.applyGrip(holder, pose, a[3] + (b[3] - a[3]) * us)
        B.thumbOn = true
    end
end

-- HOW FAST THE GUN IS PITCHING, in the player's own frame, about the hinge's own axis. Positive is muzzle UP.
-- Model space, so turning the body, walking or a snap turn read nothing; only the wrist does.
local function bPitchRate(Wm, dt)
    local q = Wm
    local rate = 0.0
    if B.qPrev and dt > 1e-4 then
        local p = B.qPrev
        local s = ((q[4] * p[1] + q[5] * p[2] + q[6] * p[3] + q[7] * p[4]) < 0.0) and -1.0 or 1.0
        local d1, d2, d3, d4 = qmul(q[4] * s, q[5] * s, q[6] * s, q[7] * s, -p[1], -p[2], -p[3], p[4])
        local vn = len3(d1, d2, d3)
        if vn > 1e-7 then
            local ang = 2.0 * at2(vn, d4)
            if ang > math.pi then ang = ang - 2.0 * math.pi end
            local hx, hy, hz = qrot(q[4], q[5], q[6], q[7], 1.0, 0.0, 0.0)
            rate = math.deg(ang / dt) * ((d1 * hx + d2 * hy + d3 * hz) / vn)
        end
    end
    B.qPrev = B.qPrev or {}
    B.qPrev[1], B.qPrev[2], B.qPrev[3], B.qPrev[4] = q[4], q[5], q[6], q[7]
    return rate
end

-- THE SPENT CASE IN ITS CHAMBER, moved by the weapon's own bone so it rides the barrels exactly -- an entity
-- placed every frame is a frame behind the gun it is in, a bone write is not. The case only becomes a world body
-- when it clears the chamber mouth, and it leaves the way the Overture's cases leave a swung-out cylinder:
--
--     along the chamber   a = g_eff . out - mu * |g_eff across|       (it slides when that is positive)
--
-- with g_eff gravity LESS the chamber's own acceleration, so a sharp jerk of the gun throws them out exactly
-- the way it does on the real thing, and holding it level with the breech up keeps them in.
-- `out` is the chamber's line out of the breech. A shell cannot leave by the muzzle -- unlike a cylinder, a
-- barrel is open at one end only -- so only that one direction is ever taken.
local function bCaseTick(H, sc, k, Cw, dt, acc, open)
    local c = B.ch[k]
    if c.st ~= 'spent' then return end
    local ej = sc.eject or {}
    local L = sc.spentLen or sc.shellLen or 0.08
    -- the extractor's own lift, as the barrels go over: nothing moves until they are half way down
    local lift = (ej.extract or 0.008) * clamp((open - 0.5) * 2.0, 0.0, 1.0)
    if (c.slide or 0.0) < lift then c.slide = lift; if (c.sv or 0.0) < 0.0 then c.sv = 0.0 end end
    local ox, oy, oz = qrot(Cw[4], Cw[5], Cw[6], Cw[7], 0.0, 0.0, -1.0)      -- out of the breech, world
    local gx, gy, gz = 0.0, 0.0, -G
    local kI = ej.inertia or 1.0
    if acc then gx, gy, gz = gx - acc[1] * kI, gy - acc[2] * kI, gz - acc[3] * kI end
    local along = gx * ox + gy * oy + gz * oz
    local px, py, pz = gx - along * ox, gy - along * oy, gz - along * oz
    local nrm = len3(px, py, pz)
    local muK, muS = ej.mu or 0.30, ej.muStatic or 0.40
    local sv = c.sv or 0.0
    local a
    if sv > 1e-3 then a = along - muK * nrm
    elseif sv < -1e-3 then a = along + muK * nrm
    elseif along > muS * nrm then a = along - muK * nrm
    elseif along < -muS * nrm and c.slide > lift + 1e-4 then a = along + muK * nrm
    else a, sv = 0.0, 0.0 end
    if open < 0.98 then a = math.min(a, 0.0) end     -- the barrels are still dropping: the extractor has it
    sv = sv + a * dt
    c.slide = c.slide + sv * dt
    if c.slide <= lift then c.slide = lift; if sv < 0.0 then sv = 0.0 end end
    c.sv = sv
    c.dbg = string.format('%.0fmm a=%+.1f', c.slide * 1000.0, a)
    if c.slide >= L then
        -- CLEAR OF THE MOUTH. From here it is a body in the world, starting exactly where the bone left it and
        -- with the velocity it had: the chamber's own plus its slide. It tips as it goes -- the part of gravity
        -- across the case is what pulled it over the lip -- so it gets the turn a rod pivoting there would.
        local F = fcomp(Cw, fset({}, 0.0, 0.0, -c.slide, 0.0, 0.0, 0.0, 1.0))
        local cv = B.cvel or { 0.0, 0.0, 0.0 }
        -- tumble about the horizontal line across the case: the way a rod tips over an edge
        local wl = len3(-oy, ox, 0.0)
        local wa = (wl > 1e-4) and { -oy / wl, ox / wl, 0.0 } or { 1.0, 0.0, 0.0 }
        local spin = (ej.tumble or 6.0) * (nrm / G)
        bodySpawn(H, sc.caseEntity, sc.caseBind or sc.chambers[1].pos, sc.bindRot, L, F,
                  cv[1] + ox * sv, cv[2] + oy * sv, cv[3] + oz * sv, spin, wa)
        c.st, c.slide, c.sv = 'empty', 0.0, 0.0
        c.hideNext = true
        H.playSnd(B.weapon, sc.sounds and sc.sounds.caseOut)
    end
end

-- The chambers, in the world: the magazine root sits at `vrp_mag_slot` (it rides the hinge), and each chamber's
-- shell bone rests at its own offset there, turned the rig's usual way.
local function bChamberFrames(sc, MS)
    B.Cw = B.Cw or {}
    for k, c in ipairs(sc.chambers) do
        B.Cw[k] = fcomp(MS, fset(B.Cw[k] or {}, c.pos[1], c.pos[2], c.pos[3],
                                 sc.bindRot[1], sc.bindRot[2], sc.bindRot[3], sc.bindRot[4]), B.Cw[k])
    end
    return B.Cw
end

-- THE CHAMBERS' LOOK, while the barrels are open. Live: the game's own seated shell. Spent: the game's own FIRED
-- shell mesh (the `_stdr` meshes are 8 mm longer -- the crimp has opened -- and the rig shows them on the reload
-- bones at exactly the same rest), slid out by the case physics. Empty: neither.
local function bChamberLook(sc)
    local mw = sc.magWhich or 0
    for k, c in ipairs(sc.chambers) do
        local st = B.ch[k]
        if st.st == 'live' then
            trackSet(mw, c.live.track, 1.0)
            if c.spent then trackSet(mw, c.spent.track, 0.0) end
        elseif st.st == 'spent' then
            trackSet(mw, c.live.track, 0.0)
            if c.spent then
                trackSet(mw, c.spent.track, 1.0)
                local x, y, z = qrot(sc.bindRot[1], sc.bindRot[2], sc.bindRot[3], sc.bindRot[4], 0.0, 0.0, -(st.slide or 0.0))
                pcall(function() VRRigWrite(mw, c.spent.bone, x, y, z) end)
            end
        else
            trackSet(mw, c.live.track, 0.0)
            if c.spent then
                -- one more frame of the fired mesh at the mouth, while the body that replaces it resolves
                if st.hideNext then
                    st.hideNext = false
                else
                    trackSet(mw, c.spent.track, 0.0)
                end
            end
        end
    end
    B.wrote = true
end

-- ------------------------------------------------------------------ the shells in the hand
-- `held.list` is the shells still in the hand, each with the item part that draws it and its hold in the slot.
local function bHeldParts(H, sc, h)
    if h.parts then return h.parts end
    local ok, e = pcall(function()
        return Game.GetTransactionSystem():GetItemInSlot(Game.GetPlayer(),
            TweakDBID.new((h.side == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight'))
    end)
    if not (ok and e) then return nil end
    local okc, cs = pcall(function() return Game.VREntityComponents(e) end)
    if not (okc and cs) then return nil end
    local parts = {}
    for i = 1, #cs do
        local nm = tostring(cs[i]:GetName())
        if string.find(nm, 'shell_a', 1, true) then parts.a = cs[i]
        elseif string.find(nm, 'shell_b', 1, true) then parts.b = cs[i] end
    end
    if not (parts.a and parts.b) then return nil end
    -- the full mesh, held at the face (the item carries it too; this covers an archive built before it did)
    pcall(function() parts.a.forceLODLevel = 0; parts.b.forceLODLevel = 0 end)
    h.parts = parts
    return parts
end

local function bHeldPlace(H, sc, h)
    local parts = bHeldParts(H, sc, h)
    if not parts then return false end
    local used = { a = false, b = false }
    for _, s in ipairs(h.list) do
        local c = parts[s.part]
        if c then
            used[s.part] = true
            local bp = (s.part == 'a') and sc.meshBind.a or sc.meshBind.b
            local T = unbind(s.hold, bp, sc.bindRot, s.T)
            s.T = T
            pcall(function()
                c:SetLocalTransform(Vector4.new(T[1], T[2], T[3], 1.0), Quaternion.new(T[4], T[5], T[6], T[7]))
            end)
        end
    end
    for _, p in ipairs({ 'a', 'b' }) do
        if (h.shown or {})[p] ~= used[p] then
            local c = parts[p]
            pcall(function() c:Toggle(used[p]) end)
            h.shown = h.shown or {}
            h.shown[p] = used[p]
        end
    end
    return true
end

local function holdFrame(t)
    return fset({}, t.off[1], t.off[2], t.off[3], t.rot[1], t.rot[2], t.rot[3], t.rot[4])
end

local function bTake(H, sc, side, n)
    local slot = (side == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight'
    if not H.itemHold(side, sc.item, slot) then return false end
    local h = { side = side, list = {}, shown = {}, n = n }
    if n >= 2 then
        h.list[1] = { part = 'a', hold = holdFrame(sc.carry2[1]) }
        h.list[2] = { part = 'b', hold = holdFrame(sc.carry2[2]) }
        h.pose = sc.carry2.pose
    else
        h.list[1] = { part = 'a', hold = holdFrame(sc.carry1) }
        h.pose = sc.carry1.pose
    end
    B.held = h
    H.beltTaken(true)
    H.playSnd(B.weapon, sc.sounds and sc.sounds.grab)
    return true
end

-- THE MAGNET, one shell into one chamber at a time. Every shell still in the hand is tried against every empty
-- chamber, and the pair whose shell lies nearest the chamber's line wins; the one already caught keeps it until
-- it is clearly off the line, so two near-equal pairs cannot trade the hand between them frame by frame.
--
-- Depth is measured ALONG the chamber from the breech face: a shell lined up with its tip at the mouth reads -L,
-- seated reads 0. The hand pushes it in; the magnet only holds it ON the line and square to it.
local function bMagnet(H, sc, side, Wr, Rel, PW)
    local h = B.held
    if not (h and Wr and Rel) then unpin(); return end
    local mg = sc.magnet or {}
    local L = sc.shellLen or 0.08
    local R, reach, gain = mg.radius or 0.045, mg.reach or 0.07, mg.gain or 3.0
    local cosA = math.cos(math.rad(mg.angle or 55.0))
    local Sl = fcomp(Wr, Rel)                                   -- the hold slot, from the RAW wrist
    local best, bestP = nil, nil
    for si, s in ipairs(h.list) do
        local F = fcomp(Sl, s.hold, s.Fm)
        s.Fm = F
        local ax, ay, az = qrot(F[4], F[5], F[6], F[7], 0.0, 0.0, 1.0)
        for k, c in ipairs(B.ch) do
            if c.st == 'empty' then
                local Cm = B.Cm[k]
                local ix, iy, iz = qrot(Cm[4], Cm[5], Cm[6], Cm[7], 0.0, 0.0, 1.0)
                local dx, dy, dz = F[1] - Cm[1], F[2] - Cm[2], F[3] - Cm[3]
                local d = dx * ix + dy * iy + dz * iz
                local px, py, pz = dx - d * ix, dy - d * iy, dz - d * iz
                local perp = len3(px, py, pz)
                local al = ax * ix + ay * iy + az * iz
                local keep = B.magnet and B.magnet.si == si and B.magnet.k == k
                local rr = keep and R * (mg.widen or 1.5) or R
                if d > -(L + reach) and d < 0.02 and perp < rr and al > cosA then
                    local score = perp - (keep and 0.01 or 0.0)
                    if not best or score < bestP then
                        best, bestP = { si = si, k = k, d = d, perp = perp, ix = ix, iy = iy, iz = iz,
                                        ax = ax, ay = ay, az = az, ang = math.deg(math.acos(clamp(al, -1.0, 1.0))) }, score
                    end
                end
            end
        end
    end
    B.magnet = best
    if not best then
        -- NOTHING IN THE ZONE: say how far off the nearest pair is, which is what a test (or a person reading the
        -- log) needs to bring a hand in -- depth along the chamber, distance off its line, angle to it
        local near = nil
        for _, s in ipairs(h.list) do
            local F = s.Fm
            local ax, ay, az = qrot(F[4], F[5], F[6], F[7], 0.0, 0.0, 1.0)
            for k, c in ipairs(B.ch) do
                if c.st == 'empty' then
                    local Cm = B.Cm[k]
                    local ix, iy, iz = qrot(Cm[4], Cm[5], Cm[6], Cm[7], 0.0, 0.0, 1.0)
                    local dx, dy, dz = F[1] - Cm[1], F[2] - Cm[2], F[3] - Cm[3]
                    local d = dx * ix + dy * iy + dz * iz
                    local perp = len3(dx - d * ix, dy - d * iy, dz - d * iz)
                    local ang = math.deg(math.acos(clamp(ax * ix + ay * iy + az * iz, -1.0, 1.0)))
                    if not near or perp < near.perp then near = { k = k, d = d, perp = perp, ang = ang } end
                end
            end
        end
        B.near = near
        unpin()
        return
    end
    B.near = nil
    local s = h.list[best.si]
    local F = s.Fm
    local Cm = B.Cm[best.k]
    -- ON THE LINE, square to it, at the depth the hand has it at (never past home)
    local dt0 = clamp(best.d, -(L + reach), 0.0)
    local sx, sy, sz, sw = qswing(best.ax, best.ay, best.az, best.ix, best.iy, best.iz)
    local q1, q2, q3, q4 = qmul(sx, sy, sz, sw, F[4], F[5], F[6], F[7])
    local Ft = fset(B.Ft or {}, Cm[1] + best.ix * dt0, Cm[2] + best.iy * dt0, Cm[3] + best.iz * dt0, q1, q2, q3, q4)
    B.Ft = Ft
    -- slot = shell * conj(hold), wrist = slot * conj(rel)
    B.tmp1 = finv(s.hold, B.tmp1)
    local Slt = fcomp(Ft, B.tmp1, B.tmp2)
    B.tmp2 = Slt
    local Wt = wristFor(Slt, Rel, B.Wt)
    B.Wt = Wt
    local w = clamp((1.0 - best.perp / R) * gain, 0.0, 1.0)
    -- and it eases in over the last `reach` of approach, so it does not snap on at the edge of the zone
    if best.d < -L then w = w * clamp(1.0 + (best.d + L) / reach, 0.0, 1.0) end
    best.w = w
    S.owned = pin(side, w, Wr, Wt)
    -- SEATED: the round is in. The game's own shell takes its place in the same frame the hand's goes, and the
    -- gun's count goes up by one -- paid out of the reserve by the game's own event, the way every reload is.
    if best.d >= -L * (1.0 - (mg.seatAt or 0.80)) and w > 0.5 then
        local c = B.ch[best.k]
        c.st = 'live'
        table.remove(h.list, best.si)
        local n = (H.magAmmo(B.weapon) or 0) + 1
        H.setMagAmmo(B.weapon, n)
        B.ammoLast = n
        H.playSnd(B.weapon, sc.sounds and sc.sounds.shellIn)
        B.magnet = nil
        unpin()
        S.owned = nil
        if #h.list == 0 then
            -- the hand is empty: let go of the item, open the fingers off the shells
            H.itemDrop(h.side, (h.side == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight')
            B.held = nil
            H.beltTaken(false)
            if sc.releasePose then finWant(H, side, sc.releasePose); B.releaseT = 0.25 else finOff() end
        end
    end
end

local function bFrame(H, sc, weapon, slotComp, holder, dt)
    local free = (holder == 1) and 0 or 1
    B.weapon, B.holder = weapon, holder
    S.owned = nil
    local PW = playerWorld(B.PW); B.PW = PW
    local WW = weaponWorld(weapon, B.WW); B.WW = WW
    if not (PW and WW) then return 'sg:noframe' end
    local Wm = toModel(PW, WW, B.Wm); B.Wm = Wm

    -- THE AMMO WATCH. The gun's own count is the truth about how many rounds are LIVE; what is spent and what is
    -- empty only this module knows. A drop while shut is a shot, and it took the first live chamber in order.
    local am = H.magAmmo(weapon) or 0
    if B.ammoLast == nil then B.ammoLast = am end
    if am < B.ammoLast and B.act == 'closed' then
        local d = B.ammoLast - am
        for k = 1, #B.ch do
            if d > 0 and B.ch[k].st == 'live' then
                B.ch[k].st, B.ch[k].slide, B.ch[k].sv, B.ch[k].dbg = 'spent', 0.0, 0.0, nil
                d = d - 1
            end
        end
    elseif am > B.ammoLast then
        -- rounds that did not come through our own seating: the game's reload, a refill. Count the live ones
        -- from the gun again and keep whatever else we knew.
        local live = 0
        for k = 1, #B.ch do if B.ch[k].st == 'live' then live = live + 1 end end
        if am > live then
            local add = am - live
            for k = #B.ch, 1, -1 do
                if add > 0 and B.ch[k].st ~= 'live' then B.ch[k].st = 'live'; add = add - 1 end
            end
        end
    end
    B.ammoLast = am

    -- THE CLICK, THE LEVER, THE BARRELS
    local r3 = r3Down()
    local r3Edge = r3 and not B.r3Prev
    B.r3Prev = r3
    local hc = sc.hinge
    local snd = sc.sounds or {}
    local th = sc.thumb or {}
    local RT, PT = th.reach or 0.10, th.pressTime or 0.08
    B.t = (B.t or 0.0) + dt

    local rate = bPitchRate(Wm, dt)
    if B.act == 'open' and rate > S.flickPeak then S.flickPeak = rate end

    if B.act == 'closed' then
        if r3Edge and (B.settle or 0.0) <= 0.0 then
            B.act, B.t = 'unlatch', 0.0
            B.thumbT, B.thumbKey = 0.0, nil
            B.leverSnd = false
        end
    elseif B.act == 'unlatch' then
        -- the lever goes over under the thumb, and only once the thumb is ON it: a part that moves before the
        -- finger arrives reads as moving by itself
        local u = clamp((B.t - RT * 0.6) / (RT * 0.4 + PT), 0.0, 1.0)
        B.lever = smooth(u)
        if u > 0.0 and not B.leverSnd then H.playSnd(weapon, snd.lever); B.leverSnd = true end
        if u >= 1.0 then
            B.act, B.t, B.hv = 'opening', 0.0, 0.0
            H.playSnd(weapon, snd.open)
        end
    elseif B.act == 'opening' then
        -- FALLING OPEN. Measured off the Satara's empty reload: 3.9 deg at 0.96, 33.5 at 1.02, 44.6 at 1.09 --
        -- under a twentieth of a second from latch to stop, which is a barrel dropping, not one being lowered.
        -- So: a constant angular acceleration, and a small rebound off the stop.
        B.hv = B.hv + (hc.accel or 5200.0) * dt
        B.hinge = B.hinge + B.hv * dt
        local A = hc.angle or 55.0
        if B.hinge >= A then
            B.hinge = A
            B.hv = -B.hv * (hc.bounce or 0.22)
            B.bounces = (B.bounces or 0) + 1
            if math.abs(B.hv) < 25.0 or B.bounces > 3 then
                B.hinge, B.hv, B.bounces = A, 0.0, 0
                B.act, B.t = 'open', 0.0
                S.flickPeak = 0.0
            end
        end
    elseif B.act == 'open' then
        local fl = sc.flick or {}
        local busy = B.magnet ~= nil
        -- A FLICK LASTS SEVERAL FRAMES; A TRACKING JUMP LASTS ONE. Two frames in a row over the threshold, and
        -- nothing faster than a wrist can turn: measured, a controller pose that merely jumped (the simulator's,
        -- set in one write) read 646 deg/s for a single frame and shut the barrels on its own.
        if rate >= (fl.rate or 240.0) and rate <= (fl.max or 2000.0) then B.flickN = (B.flickN or 0) + 1
        else B.flickN = 0 end
        if (r3Edge or (B.t > (fl.arm or 0.35) and (B.flickN or 0) >= (fl.frames or 2) and not busy)) then
            B.act, B.t = 'closing', 0.0
            B.closeFrom = B.hinge
            B.flickAt = rate
        end
    elseif B.act == 'closing' then
        -- SHUT IN ONE MOVEMENT: the barrels come up under their own momentum and slam on the latch. The native
        -- close is 55.5 deg to nothing in six hundredths of a second, and accelerating all the way.
        local T = hc.closeTime or 0.07
        local u = clamp(B.t / T, 0.0, 1.0)
        B.hinge = (B.closeFrom or hc.angle) * (1.0 - u * u)
        if u >= 1.0 then
            B.hinge, B.act, B.t = 0.0, 'closed', 0.0
            H.playSnd(weapon, snd.close)
            B.leverKick = 0.0
            B.settle = 0.30
            -- whatever was still in a chamber stays in it: a case that never fell out is still a fired case
            for k = 1, #B.ch do B.ch[k].slide, B.ch[k].sv = 0.0, 0.0 end
        end
    end

    -- the lever: under the thumb while it is pressed, sprung back when the thumb lets go, and a small kick at the
    -- latch when the barrels shut (5 deg in the native close, 0.15 s)
    if B.act == 'opening' or B.act == 'open' then
        local hold = (B.thumbT ~= nil) and (B.thumbT < RT + PT + (th.hold or 0.30))
        if not hold then B.lever = math.max(0.0, B.lever - dt / ((sc.lever and sc.lever.back) or 0.12)) end
    elseif B.act == 'closed' and B.leverKick then
        B.leverKick = B.leverKick + dt
        local u = B.leverKick / 0.15
        B.lever = (u < 1.0) and (math.sin(math.pi * u) * 0.12) or 0.0
        if u >= 1.0 then B.leverKick = nil end
    end
    bThumb(H, sc, holder, dt)

    -- the chambers, in the world and in the player's frame
    local open = clamp(B.hinge / (hc.angle or 55.0), 0.0, 1.0)
    local MS = slotWorld(slotComp, sc.slot or 'vrp_mag_slot', B.MS)
    B.MS = MS
    local active = B.act ~= 'closed' or (B.settle or 0.0) > 0.0 or B.held ~= nil
    if MS and active then
        local Cw = bChamberFrames(sc, MS)
        B.Cm = B.Cm or {}
        for k = 1, #Cw do B.Cm[k] = toModel(PW, Cw[k], B.Cm[k]) end
        -- the chamber's acceleration, for the cases: smoothed, because two frames of hand jitter is metres per
        -- second squared, and clamped, because a load or a teleport is not a jerk of the wrist
        local p0 = Cw[1]
        local acc = nil
        if B.cpos and dt > 1e-4 then
            local vx, vy, vz = (p0[1] - B.cpos[1]) / dt, (p0[2] - B.cpos[2]) / dt, (p0[3] - B.cpos[3]) / dt
            if len3(vx, vy, vz) > 20.0 then vx, vy, vz = 0.0, 0.0, 0.0 end
            local cv = B.cvel or { vx, vy, vz }
            local ax, ay, az = (vx - cv[1]) / dt, (vy - cv[2]) / dt, (vz - cv[3]) / dt
            local a = 0.35
            cv[1], cv[2], cv[3] = cv[1] + (vx - cv[1]) * a, cv[2] + (vy - cv[2]) * a, cv[3] + (vz - cv[3]) * a
            B.cvel = cv
            local sa = B.cacc or { 0.0, 0.0, 0.0 }
            sa[1], sa[2], sa[3] = sa[1] + (ax - sa[1]) * a, sa[2] + (ay - sa[2]) * a, sa[3] + (az - sa[3]) * a
            local al = len3(sa[1], sa[2], sa[3])
            if al > 60.0 then sa[1], sa[2], sa[3] = sa[1] * 60.0 / al, sa[2] * 60.0 / al, sa[3] * 60.0 / al end
            B.cacc = sa
            acc = sa
        end
        B.cpos = B.cpos or {}
        B.cpos[1], B.cpos[2], B.cpos[3] = p0[1], p0[2], p0[3]
        if B.act ~= 'closed' and B.act ~= 'unlatch' then
            for k = 1, #B.ch do bCaseTick(H, sc, k, Cw[k], dt, acc, open) end
        end
    else
        B.cpos, B.cvel, B.cacc = nil, nil, nil
    end

    -- THE BELT: as many shells as there are chambers without a live round, the moment the hand closes on the
    -- pouch. Without a belt (or for a weapon the belt has no pouch for) a squeeze anywhere well clear of the gun
    -- brings them, which is what every other reload here does.
    local grip = H.gripOf(free)
    local gripEdge = grip and not B.gripPrev
    B.gripPrev = grip
    local need = 0
    for k = 1, #B.ch do if B.ch[k].st ~= 'live' then need = need + 1 end end
    local Wr = wristRaw(free, B.Wr); B.Wr = Wr
    local Rel = wristToSlot(free, B.Rel); B.Rel = Rel
    if not B.held and (B.act == 'open' or B.act == 'opening') and need > 0 and gripEdge and Wr then
        local bd = H.beltMagDist(free)
        local ok = false
        if bd ~= nil then ok = bd <= H.beltMagRadius()
        elseif B.Cm and B.Cm[1] then ok = len3(Wr[1] - B.Cm[1][1], Wr[2] - B.Cm[1][2], Wr[3] - B.Cm[1][3]) > 0.30 end
        if ok then bTake(H, sc, free, math.min(need, 2)) end
    end

    if B.held then
        local h = B.held
        if h.side ~= free then bHeldDrop(H, sc, false)
        else
            bHeldPlace(H, sc, h)
            finWant(H, free, h.pose)
            -- world frames of the shells, for a drop: the hand's velocity is taken off the first one
            if Wr and Rel and PW then
                local Sl = fcomp(Wr, Rel, B.Slm)
                B.Slm = Sl
                h.world = true
                for _, s in ipairs(h.list) do
                    local Fm = fcomp(Sl, s.hold, s.Fmw)
                    s.Fmw = Fm
                    s.F = fcomp(PW, Fm, s.F)
                end
                local f1 = h.list[1] and h.list[1].F
                if f1 and dt > 1e-4 then
                    if h.px then
                        local a = 0.45
                        h.vx = (h.vx or 0.0) + ((f1[1] - h.px) / dt - (h.vx or 0.0)) * a
                        h.vy = (h.vy or 0.0) + ((f1[2] - h.py) / dt - (h.vy or 0.0)) * a
                        h.vz = (h.vz or 0.0) + ((f1[3] - h.pz) / dt - (h.vz or 0.0)) * a
                    end
                    h.px, h.py, h.pz = f1[1], f1[2], f1[3]
                end
            end
            if not grip then
                bHeldDrop(H, sc, true)
            elseif B.act == 'open' and B.Cm then
                bMagnet(H, sc, free, Wr, Rel, PW)
            else
                unpin()
            end
        end
    else
        unpin()
        if B.releaseT then
            B.releaseT = B.releaseT - dt
            if B.releaseT <= 0.0 then B.releaseT = nil; finOff() end
        end
    end
    finTick(H, dt, 0.15)

    -- WRITES. Once a bone has been written it is written EVERY frame until the release -- including the frame it
    -- comes back to zero, or the slot would go on applying the last non-zero angle and the barrels would sit a
    -- degree open until the hand-back.
    local fw = sc.frameWhich or 1
    if B.hinge > 0.0 or B.wrote then
        local ax = hc.axis
        pcall(function() VRRigWriteRot(fw, hc.bone, B.hinge * (hc.sign or 1.0), ax[1], ax[2], ax[3]) end)
        B.wrote = true
    end
    local lv = sc.lever
    if lv and (B.lever > 0.0 or B.wrote) then
        local ax = lv.axis
        local ang = B.lever * (lv.angle or 40.0) * (lv.sign or 1.0)
        pcall(function() VRRigWriteRot(lv.which or 2, lv.bone, ang, ax[1], ax[2], ax[3]) end)
        B.wrote = true
    end
    for _, rd in ipairs(sc.riders or {}) do
        -- a latch that slides out under the lever and stays most of the way out while the barrels are open
        local f = B.lever
        if rd.follow == 'latch' and (B.act == 'opening' or B.act == 'open') then f = math.max(f, rd.openHold or 0.75) end
        if f > 0.0 or B.wrote then
            local v = rd.vec
            pcall(function() VRRigWrite(rd.which or fw, rd.bone, v[1] * f, v[2] * f, v[3] * f) end)
        end
    end
    if active and B.act ~= 'closed' and B.act ~= 'unlatch' then bChamberLook(sc) end
    if B.act == 'closed' then
        B.settle = math.max(0.0, (B.settle or 0.0) - dt)
        if B.wrote and B.settle <= 0.0 and not B.leverKick and B.hinge <= 0.0 then bRelease(H, sc) end
    end

    -- the claim on the weapon's animation, for a weapon whose graph fights the barrels
    if sc.ownAnim then
        if B.act ~= 'closed' then H.holdReloadStage(weapon); B.tagged = true
        elseif B.tagged then H.pushAnimEvent(weapon, 'InterruptReload'); B.tagged = false end
    end

    -- THE TRIGGER: nothing fires with the barrels off the latch
    S.trg = (B.act ~= 'closed') and 1 or 0
    H.setTrg(S.trg)

    local cs = {}
    for k = 1, #B.ch do
        local c = B.ch[k]
        cs[#cs + 1] = string.sub(c.st, 1, 1) .. ((c.st == 'spent' and c.dbg) and ('(' .. c.dbg .. ')') or '')
    end
    local m = B.magnet
    S.diag = string.format('sg[break %s t=%.2f hinge=%.1f lever=%.2f ch=%s am=%d held=%s mag=%s flick=%.0f/%.0f bodies=%d]',
        B.act, B.t, B.hinge, B.lever, table.concat(cs, ','), am, B.held and tostring(#B.held.list) or '-',
        m and string.format('k%d d=%.3f p=%.3f w=%.2f', m.k, m.d, m.perp, m.w or 0) or '-',
        S.flickPeak, B.flickAt or 0, #bodies)
    return 'sg:break:' .. B.act
end

-- ============================================================================ PUMP
local P = {}

local function pRelease(H, sc)
    local pc = sc.pump
    writeOff(pc.which or 1, pc.bone)
    for _, rd in ipairs(pc.riders or {}) do writeOff(rd.which or pc.which or 1, rd.bone) end
    if sc.gate then writeOff(sc.gate.which or 1, sc.gate.bone) end
    P.wrote = false
end

local function pDropShell(H, sc, throw)
    if not P.shell then return end
    local s = P.shell
    if throw and s.F and sc.roundEntity then
        bodySpawn(H, sc.roundEntity, sc.shellBind or { 0, 0, 0 }, sc.bindRot, sc.shellLen, s.F,
                  s.vx or 0.0, s.vy or 0.0, s.vz or 0.0, 0.0, nil)
    end
    H.holdHide(s.side)
    H.shellTaken(false)
    P.shell = nil
    P.magnet = nil
end

local function pResetAll(H, sc, weapon)
    -- the two-hand hold's hand back on the fore-end at rest, and the left trigger back to the game
    H.twoHandSlide(0.0, 0.0, 0.0)
    M.cycleFree()
    H.ltBlock(false)
    if sc and P.wrote then pRelease(H, sc) end
    if sc and P.shell then pDropShell(H, sc, false) end
    P.s, P.engaged, P.back, P.gate, P.gateT = 0.0, false, false, 0.0, nil
    P.magnet = nil
    local am = weapon and H.magAmmo(weapon) or 0
    P.chambered, P.spentIn = am > 0, false
    P.ammoLast = am
end

-- A HULL OUT OF THE EJECTION PORT, thrown to the right and up the way the game's own effect throws them.
local function pEject(H, sc, WW, live)
    local ej = sc.ejectPort or {}
    local p = ej.pos or { 0.012, 0.13, 0.055 }
    local F = fcomp(WW, fset({}, p[1], p[2], p[3], sc.bindRot[1], sc.bindRot[2], sc.bindRot[3], sc.bindRot[4]))
    local v = ej.vel or { 1.6, -0.3, 1.3 }
    local vx, vy, vz = qrot(WW[4], WW[5], WW[6], WW[7], v[1], v[2], v[3])
    local wax, way, waz = qrot(WW[4], WW[5], WW[6], WW[7], 0.0, 0.0, 1.0)
    local cv = P.wvel or { 0.0, 0.0, 0.0 }
    bodySpawn(H, live and sc.roundEntity or sc.caseEntity, live and (sc.shellBind or { 0, 0, 0 }) or (sc.caseBind or { 0, 0, 0 }),
              sc.bindRot, live and sc.shellLen or sc.spentLen, F,
              cv[1] + vx, cv[2] + vy, cv[3] + vz, ej.spin or 14.0, { wax, way, waz })
end

-- The fore-end and everything that rides it, ABSOLUTE from the rig's own rest. A relative write is the bone's
-- latched base plus an offset, and the base is latched the first time the bone is written -- which on a pump is
-- the frame after a shot, when the game's own cycling clip has already started pulling the fore-end back. So
-- the pin "at rest" held it wherever that clip happened to be. The rest comes from the .rig, so nothing the
-- animation does can move it.
local function pWrite(sc)
    local pc = sc.pump
    local la, r0 = pc.axis, pc.restLocal
    local s = P.s
    pcall(function()
        VRRigWriteAbs(pc.which or 1, pc.bone, r0[1] + la[1] * s, r0[2] + la[2] * s, r0[3] + la[3] * s, 0, 0, 0, 0, 1)
    end)
    for _, rd in ipairs(pc.riders or {}) do
        local v = s * (rd.ratio or 1.0)
        local ax, r1 = rd.axis or la, rd.restLocal
        pcall(function()
            VRRigWriteAbs(rd.which or pc.which or 1, rd.bone, r1[1] + ax[1] * v, r1[2] + ax[2] * v, r1[3] + ax[3] * v,
                          0, 0, 0, 0, 1)
        end)
    end
    P.wrote = true
end

-- THE LOADING GATE (the Carnage's flap, the Tactician's lifter): it swings as a shell goes past it.
local function pGate(sc, dt)
    local g = sc.gate
    if not g then return end
    local want = P.gateWant or 0.0
    P.gateWant = nil
    if P.gateT then
        P.gateT = P.gateT + dt
        local u = P.gateT / (g.time or 0.25)
        if u >= 1.0 then P.gateT = nil else want = math.max(want, math.sin(math.pi * u)) end
    end
    P.gate = want
    if want > 0.0 or P.gateOn then
        local ax = g.axis
        local ang = want * (g.angle or 20.0) * (g.sign or 1.0)
        pcall(function() VRRigWriteRot(g.which or 1, g.bone, ang, ax[1], ax[2], ax[3]) end)
        P.gateOn = want > 0.0
        P.wrote = true
    end
end

-- A LINE INTO THE GUN, in the weapon's frame: the rim of the shell travels from `from` to `to` while its
-- orientation turns from one to the other. Returns the depth along it (0 at the entry, 1 home), the distance
-- off it, and the model-space frame the shell should be at for that depth.
local function pLine(L, Wm, F, o)
    local fx, fy, fz = qrot(Wm[4], Wm[5], Wm[6], Wm[7], L.from[1], L.from[2], L.from[3])
    local tx, ty, tz = qrot(Wm[4], Wm[5], Wm[6], Wm[7], L.to[1], L.to[2], L.to[3])
    fx, fy, fz, tx, ty, tz = fx + Wm[1], fy + Wm[2], fz + Wm[3], tx + Wm[1], ty + Wm[2], tz + Wm[3]
    local dx, dy, dz = tx - fx, ty - fy, tz - fz
    local run = len3(dx, dy, dz)
    if run < 1e-5 then return nil end
    dx, dy, dz = dx / run, dy / run, dz / run
    local rx, ry, rz = F[1] - fx, F[2] - fy, F[3] - fz
    local d = rx * dx + ry * dy + rz * dz
    local px, py, pz = rx - d * dx, ry - d * dy, rz - d * dz
    local u = d / run
    local uc = clamp(u, 0.0, 1.0)
    local a1, a2, a3, a4 = qmul(Wm[4], Wm[5], Wm[6], Wm[7], L.fromRot[1], L.fromRot[2], L.fromRot[3], L.fromRot[4])
    local b1, b2, b3, b4 = qmul(Wm[4], Wm[5], Wm[6], Wm[7], L.toRot[1], L.toRot[2], L.toRot[3], L.toRot[4])
    local q1, q2, q3, q4 = qnlerp(a1, a2, a3, a4, b1, b2, b3, b4, uc)
    local T = fset(o or {}, fx + dx * run * uc, fy + dy * run * uc, fz + dz * run * uc, q1, q2, q3, q4)
    -- the axis the shell should have THERE: what the hand's shell is compared against. Not the line itself --
    -- a round dropped into an open action goes in sideways, across its own length.
    local ex, ey, ez = qrot(q1, q2, q3, q4, 0.0, 0.0, 1.0)
    return u, len3(px, py, pz), T, ex, ey, ez
end

-- ...and the shell's frame AT a place on the line, `u` past 1 carrying on the line's own way with the last
-- recorded orientation: where a shell that has clicked is pushed on to.
local function pAt(L, Wm, u, o)
    local a = clamp(u, 0.0, 1.0)
    local x = L.from[1] + (L.to[1] - L.from[1]) * u
    local y = L.from[2] + (L.to[2] - L.from[2]) * u
    local z = L.from[3] + (L.to[3] - L.from[3]) * u
    local wx, wy, wz = qrot(Wm[4], Wm[5], Wm[6], Wm[7], x, y, z)
    local a1, a2, a3, a4 = qmul(Wm[4], Wm[5], Wm[6], Wm[7], L.fromRot[1], L.fromRot[2], L.fromRot[3], L.fromRot[4])
    local b1, b2, b3, b4 = qmul(Wm[4], Wm[5], Wm[6], Wm[7], L.toRot[1], L.toRot[2], L.toRot[3], L.toRot[4])
    local q1, q2, q3, q4 = qnlerp(a1, a2, a3, a4, b1, b2, b3, b4, a)
    return fset(o or {}, Wm[1] + wx, Wm[2] + wy, Wm[3] + wz, q1, q2, q3, q4)
end

local function pFrame(H, sc, weapon, slotComp, holder, dt)
    local free = (holder == 1) and 0 or 1
    P.weapon = weapon
    -- THE CYCLE'S HULL. After every shot the game plays the pump's own `cycle_round` (1.83 s), and on the magazine
    -- rig it shows the reload shell -- track `showMagazineReload` -- and throws it out of the port about 0.8 s after
    -- the shot: the hull still seen by itself after the shot once both muzzle effects were clean (found by holding
    -- each track at 0 through a burst 0.8 s after the shot; only this one took it away). The rack throws the hull
    -- here, so the track is held at 0 while the pump is in VR hands -- let go by every reset, and while the game's
    -- own reload runs (M.gameReload), whose hand carries a shell on it.
    if sc.cycleTrack and P.cycHeld ~= sc.cycleTrack then
        M.cycleFree()
        if type(VRRigTrackWrite) == 'function' then
            pcall(function() VRRigTrackWrite(0, sc.cycleTrack, 0.0, 1) end)
            P.cycHeld = sc.cycleTrack
        end
    end
    S.owned = nil
    local pc = sc.pump
    local snd = sc.sounds or {}
    local PW = playerWorld(P.PW); P.PW = PW
    local WW = weaponWorld(weapon, P.WW); P.WW = WW
    if not (PW and WW) then return 'sg:noframe' end
    local Wm = toModel(PW, WW, P.Wm); P.Wm = Wm
    if P.wpos and dt > 1e-4 then
        local a = 0.4
        local cv = P.wvel or { 0.0, 0.0, 0.0 }
        cv[1] = cv[1] + ((WW[1] - P.wpos[1]) / dt - cv[1]) * a
        cv[2] = cv[2] + ((WW[2] - P.wpos[2]) / dt - cv[2]) * a
        cv[3] = cv[3] + ((WW[3] - P.wpos[3]) / dt - cv[3]) * a
        if len3(cv[1], cv[2], cv[3]) > 20.0 then cv[1], cv[2], cv[3] = 0.0, 0.0, 0.0 end
        P.wvel = cv
    end
    P.wpos = P.wpos or {}
    P.wpos[1], P.wpos[2], P.wpos[3] = WW[1], WW[2], WW[3]

    -- THE AMMO WATCH: the gun counts the tube and the chamber together. A drop with a round chambered is a shot,
    -- and it leaves a fired hull in the chamber that only the fore-end can get out.
    local am = H.magAmmo(weapon) or 0
    if P.ammoLast == nil then P.ammoLast = am end
    if am < P.ammoLast and P.chambered then
        P.chambered, P.spentIn = false, true
    elseif am > P.ammoLast and not P.ourAdd then
        -- the game's own reload (or a refill) chambers as it goes
        P.chambered, P.spentIn = true, false
    end
    P.ourAdd = false
    P.ammoLast = am
    local cap = H.magCapacity(weapon) or 0

    -- THE FORE-END, worked by GRIP + TRIGGER of the hand on it and by nothing else: a hand that merely holds the gun
    -- under the two-hand grip never racks it by accident (the pump moves only with grip +
    -- trigger held). The stroke is the hand's travel along the barrel MEASURED AGAINST THE GUN, so carrying the
    -- whole gun -- pulling it in, the recoil -- moves nothing. While the two-hand hold is closed the hand stays the
    -- hold's (its fingers, its weld, the aim it steers) and the hold's support point rides the fore-end
    -- (H.twoHandSlide); with the hand free the module pins it to the fore-end itself.
    local grip = H.gripOf(free)
    local gripEdge = grip and not P.gripPrev
    P.gripPrev = grip
    local work = grip and H.trigOf(free)
    local twoH = H.twoHand()
    local Wr = wristRaw(free, P.Wr); P.Wr = Wr
    local Rel = wristToSlot(free, P.Rel); P.Rel = Rel
    local bx, by, bz = qrot(Wm[4], Wm[5], Wm[6], Wm[7], pc.dir[1], pc.dir[2], pc.dir[3])   -- rearward, model
    local rx, ry, rz = pc.restW[1], pc.restW[2], pc.restW[3]
    local pg = H.handGrip(pc.grip, free, 1)         -- recorded for the left hand; the right one's is its mirror
    local go = pg.off
    local sup = free == H.supportHand()             -- the two-hand hold's hand: the free one, whichever it is
    -- the wrist on the fore-end at the current travel: rest + travel along the stroke + the measured hold
    local function gripAt(s, o)
        local x, y, z = rx + pc.dir[1] * s + go[1], ry + pc.dir[2] * s + go[2], rz + pc.dir[3] * s + go[3]
        local wx, wy, wz = qrot(Wm[4], Wm[5], Wm[6], Wm[7], x, y, z)
        local q1, q2, q3, q4 = qmul(Wm[4], Wm[5], Wm[6], Wm[7], pg.rot[1], pg.rot[2], pg.rot[3], pg.rot[4])
        return fset(o or {}, Wm[1] + wx, Wm[2] + wy, Wm[3] + wz, q1, q2, q3, q4)
    end
    local T0 = gripAt(P.s, P.T0); P.T0 = T0
    local dHand = Wr and len3(Wr[1] - T0[1], Wr[2] - T0[2], Wr[3] - T0[3]) or 99.0
    -- the hand's place along the barrel, from the gun's own origin. Under a closed hold the published wrist is the
    -- weld, which rides the gun and so never moves along it: the stroke is read off the controller itself.
    local Wa = Wr
    if twoH and sup then Wa = H.twoHandCtrl(P.Wc) or Wr; P.Wc = Wa end
    local along = Wa and ((Wa[1] - Wm[1]) * bx + (Wa[2] - Wm[2]) * by + (Wa[3] - Wm[3]) * bz) or 0.0
    local onFore = (twoH and sup) or dHand <= (pc.grabRadius or 0.10)
    -- THE SAME SQUEEZE MUST NOT AIM: the left trigger is the game's aim, and grip + trigger on the fore-end is the
    -- rack. Held off the game for as long as the gripping hand is on the fore-end.
    H.ltBlock(free == H.aimHand() and grip and onFore and not P.shell)
    -- THE REACH MARKER on the fore-end while a rack is owed (a fired hull in the chamber, or nothing chambered with
    -- rounds in the tube) and the hand is on it or in reach of it
    if not P.engaged and onFore and not P.shell and Wr and (P.spentIn or (not P.chambered and am > 0)) then
        H.reachMark(free, T0[1], T0[2], T0[3], Wr[1], Wr[2], Wr[3])
    end
    if not P.engaged then
        if work and onFore and not P.shell and Wr then
            P.engaged, P.along0, P.s0, P.viaHold = true, along, P.s, twoH
        end
    elseif not work or (P.viaHold and not twoH) then
        P.engaged = false
        unpin()
        if finS.key == pc.pose then finOff() end
    end
    local travel = pc.travel
    if P.engaged and Wr then
        local s = clamp(P.s0 + (along - P.along0) * (pc.gain or 1.0), 0.0, travel)
        local prev = P.s
        P.s = s
        if s > 0.2 * travel and prev <= 0.2 * travel and not P.back then H.playSnd(weapon, snd.pumpOpen) end
        if s >= 0.85 * travel and not P.back then
            P.back = true
            -- OUT OF THE PORT: the fired hull, or a live round if one was chambered
            if P.spentIn then pEject(H, sc, WW, false); P.spentIn = false
            elseif P.chambered then
                pEject(H, sc, WW, true)
                P.chambered = false
                H.setMagAmmo(weapon, math.max(0, am - 1))
                P.ammoLast = math.max(0, am - 1)
            end
        end
        if P.back and s <= 0.08 * travel then
            P.back = false
            -- AND HOME: the next round off the tube goes into the chamber
            if not P.chambered and P.ammoLast > 0 then P.chambered = true end
            H.playSnd(weapon, snd.pumpClose)
        end
        if not P.viaHold then
            local T = gripAt(s, P.Tg); P.Tg = T
            local w = spring(dt)
            S.owned = pin(free, w, Wr, T)
            finWant(H, free, pc.pose)
        end
    elseif not P.shell then
        springReset()
        if pinS.side == free then unpin() end
        if finS.key == pc.pose then finOff() end
    end
    -- the two-hand hold's drawn hand, wherever the fore-end is: run back by the stroke, along the barrel
    if twoH and sup and P.s > 0.0005 then H.twoHandSlide(bx * P.s, by * P.s, bz * P.s)
    else H.twoHandSlide(0.0, 0.0, 0.0) end

    -- SHELLS, one at a time: off the row on the belt (or out of nowhere without one), into the tube through the
    -- loading port -- or, with the action open and nothing chambered, straight into the chamber.
    if not P.shell and not P.engaged and gripEdge and Wr then
        local bd = H.beltShellDist(free)
        local ok
        if bd ~= nil then ok = bd <= H.beltShellRadius()
        else ok = len3(Wr[1] - Wm[1], Wr[2] - Wm[2], Wr[3] - Wm[3]) > 0.35 end
        if ok and am < cap and H.holdShow(free, sc.shellMesh, nil) then
            P.shell = { side = free, hold = holdFrame(sc.carry) }
            H.shellTaken(true, free)
            H.playSnd(weapon, snd.grab)
        end
    end
    if P.shell then
        local s = P.shell
        if s.side ~= free then pDropShell(H, sc, false)
        else
            local T = unbind(s.hold, sc.shellBind or { 0, 0, 0 }, sc.bindRot, s.T)
            s.T = T
            H.holdPlace(free, T[1], T[2], T[3], T[4], T[5], T[6], T[7])
            finWant(H, free, sc.carry.pose)
            local Fm = nil
            if Wr and Rel then
                local Sl = fcomp(Wr, Rel, P.Slm); P.Slm = Sl
                Fm = fcomp(Sl, s.hold, s.Fm); s.Fm = Fm
                s.F = fcomp(PW, Fm, s.F)
                if dt > 1e-4 and s.px then
                    local a = 0.45
                    s.vx = (s.vx or 0.0) + ((s.F[1] - s.px) / dt - (s.vx or 0.0)) * a
                    s.vy = (s.vy or 0.0) + ((s.F[2] - s.py) / dt - (s.vy or 0.0)) * a
                    s.vz = (s.vz or 0.0) + ((s.F[3] - s.pz) / dt - (s.vz or 0.0)) * a
                end
                s.px, s.py, s.pz = s.F[1], s.F[2], s.F[3]
            end
            local seatMode = nil
            if s.push then
                local pu = s.push
                pu.t = pu.t + dt
                local uu = pu.u0 + (pu.u1 - pu.u0) * smooth(clamp(pu.t / pu.T, 0.0, 1.0))
                local Tt = pAt(pu.line, Wm, uu, P.Tt); P.Tt = Tt
                if Rel then
                    P.tmp1 = finv(s.hold, P.tmp1)
                    local Slt = fcomp(Tt, P.tmp1, P.tmp2)
                    P.tmp2 = Slt
                    local Wt = wristFor(Slt, Rel, P.Wt); P.Wt = Wt
                    S.owned = pin(free, 1.0, Wr, Wt)
                end
                if pu.mode == 'tube' then P.gateWant = 1.0 end
                P.magnet = { u = uu, perp = 0.0, w = 1.0, mode = pu.mode, ang = 0.0 }
                if pu.t >= pu.T then seatMode = pu.mode end
            elseif not grip then
                pDropShell(H, sc, true)
                unpin()
                finOff()
            elseif Fm then
                -- which line: the open action takes a round into the chamber, otherwise the tube
                local mg = sc.magnet or {}
                local line, mode = sc.tube, 'tube'
                if sc.port and (not P.chambered) and P.s >= 0.85 * travel then line, mode = sc.port, 'port' end
                local u, perp, Tt, dx, dy, dz = pLine(line, Wm, Fm, P.Tt)
                P.Tt = Tt
                local ax, ay, az = qrot(Fm[4], Fm[5], Fm[6], Fm[7], 0.0, 0.0, 1.0)
                local al = u and (ax * dx + ay * dy + az * dz) or -1.0
                P.near = u and { u = u, perp = perp, ang = math.deg(math.acos(clamp(al, -1.0, 1.0))), mode = mode } or nil
                local R = mg.radius or 0.05
                local keep = P.magnet ~= nil
                local rr = keep and R * (mg.widen or 1.5) or R
                if u and u > -(mg.reach or 0.8) and u < 1.05 and perp < rr and al > math.cos(math.rad(mg.angle or 65.0))
                   and (mode == 'port' or am < cap) then
                    local w = clamp((1.0 - perp / R) * (mg.gain or 3.0), 0.0, 1.0)
                    if u < 0.0 then w = w * clamp(1.0 + u / (mg.reach or 0.8), 0.0, 1.0) end
                    P.tmp1 = finv(s.hold, P.tmp1)
                    local Slt = fcomp(Tt, P.tmp1, P.tmp2)
                    P.tmp2 = Slt
                    local Wt = wristFor(Slt, Rel, P.Wt); P.Wt = Wt
                    S.owned = pin(free, w, Wr, Wt)
                    P.magnet = { u = u, perp = perp, w = w, mode = mode, ang = P.near and P.near.ang or 0.0 }
                    if mode == 'tube' then P.gateWant = clamp((u - 0.25) / 0.5, 0.0, 1.0) end
                    if u >= (mg.seatAt or 0.80) and w > 0.5 then
                        -- THE CLICK, and then the shell goes on in by itself: under the thumb, the rest of the way
                        -- into the tube (`pushTo` of the line, past the rim's last recorded place) or home in the
                        -- chamber, the hand carried with it -- instead of vanishing at the mouth (pushed
                        -- deeper). Counted in when it gets there.
                        H.playSnd(weapon, (mode == 'port') and (snd.shellInChamber or snd.shellIn) or snd.shellIn)
                        s.push = { t = 0.0, T = mg.pushTime or 0.10, u0 = u, line = line, mode = mode,
                                   u1 = (mode == 'port') and (mg.pushPort or 1.0) or (mg.pushTo or 1.25) }
                        if s.push.u1 < u then s.push.u1 = u end
                    end
                else
                    P.magnet = nil
                    unpin()
                end
            end
            if seatMode then
                local n = am + 1
                H.setMagAmmo(weapon, n)
                P.ourAdd = true
                P.ammoLast = n
                if seatMode == 'port' then P.chambered = true
                else P.gateT = 0.5 * (sc.gate and sc.gate.time or 0.25) end
                H.holdHide(free)
                H.shellTaken(false)
                P.shell, P.magnet = nil, nil
                unpin()
                S.owned = nil
                finOff()
            end
        end
    end
    finTick(H, dt, 0.15)
    pGate(sc, dt)

    -- the fore-end is held where it was left, and pinned at rest while a rack is owed so the game's own
    -- cycling animation cannot run it for the player
    if P.engaged or P.s > 0.0005 or not P.chambered then pWrite(sc)
    elseif P.wrote and not P.gateOn then pRelease(H, sc) end

    -- THE TRIGGER: only a chambered round with the action shut
    S.trg = (P.chambered and P.s <= 0.003) and 0 or 1
    H.setTrg(S.trg)

    local m = P.magnet
    S.diag = string.format('sg[pump s=%.0fmm eng=%s back=%s ch=%s spent=%s am=%d/%d shell=%s mag=%s gate=%.2f bodies=%d]',
        P.s * 1000.0, P.engaged and 'Y' or 'n', P.back and 'Y' or 'n', P.chambered and 'Y' or 'n',
        P.spentIn and 'Y' or 'n', am, cap, P.shell and 'Y' or 'n',
        m and string.format('%s u=%.2f p=%.3f w=%.2f', m.mode, m.u, m.perp, m.w) or '-', P.gate or 0, #bodies)
    return 'sg:pump'
end

-- ============================================================================ entry points
local function cleanup(H, sc, weapon)
    if S.kind == 'break' then bResetAll(H, sc, weapon)
    elseif S.kind == 'pump' then pResetAll(H, sc, weapon) end
    unpin()
    finReset(H)
    H.setTrg(0)
    S.owned = nil
end

function M.frame(H, weapon, slotComp, holder, dt, cfg)
    local sc = cfg.shotgun
    dt = dt or 0.016
    local okId, wid = pcall(function() return weapon:GetEntityID().hash end)
    local key = okId and tostring(wid) or '?'
    if key ~= S.key or S.cfg ~= cfg then
        if S.cfg then cleanup(H, S.cfg.shotgun, nil) end
        S.key, S.cfg, S.kind = key, cfg, sc.kind
        -- the sounds sit at the config's top level, where every other weapon keeps them
        if not sc.sounds then sc.sounds = cfg.sounds end
        if sc.kind == 'break' then bResetAll(H, sc, weapon) else pResetAll(H, sc, weapon) end
    end
    local r
    if sc.kind == 'break' then r = bFrame(H, sc, weapon, slotComp, holder, dt)
    else r = pFrame(H, sc, weapon, slotComp, holder, dt) end
    return r, S.owned
end

-- The weapon is gone (holstered, dropped, no holder): everything of ours lets go. Bodies already in the air keep
-- falling -- they are the world's now -- and are ticked by whatever frame comes next.
function M.lost(H)
    if S.cfg then cleanup(H, S.cfg.shotgun, nil) end
    S.key, S.cfg, S.kind = nil, nil, nil
end

-- The cycle's hull track handed back to the game (see pFrame).
function M.cycleFree()
    if P.cycHeld and type(VRRigTrackWrite) == 'function' then
        local idx = P.cycHeld
        pcall(function() VRRigTrackWrite(0, idx, 0.0, 0) end)
    end
    P.cycHeld = nil
end

-- The game's own reload took the gun.
function M.yield(H, weapon)
    if S.cfg then cleanup(H, S.cfg.shotgun, nil) end
    -- whatever the game's reload ends with, the counts are read fresh from it on the next frame
    S.key = nil
end

function M.tick(H, dt) bodyTick(H, dt or 0.016) end

function M.active()
    if S.kind == 'break' then
        return B.act ~= 'closed' or B.held ~= nil or (B.settle or 0.0) > 0.0
    elseif S.kind == 'pump' then
        return P.engaged or P.shell ~= nil or (P.s or 0.0) > 0.0005
    end
    return false
end

function M.diag()
    local n = B.near
    if n and S.kind == 'break' then
        return (S.diag or '-') .. string.format(' near[k%d d=%.3f p=%.3f a=%.0f]', n.k, n.d, n.perp, n.ang)
    end
    local q = P.near
    if q and S.kind == 'pump' and P.shell then
        return (S.diag or '-') .. string.format(' near[%s u=%.2f p=%.3f a=%.0f]', q.mode, q.u, q.perp, q.ang)
    end
    return S.diag or '-'
end

-- The module's own state, for the test harness: read, never written.
function M.debug() return S, B, P, bodies end

return M
