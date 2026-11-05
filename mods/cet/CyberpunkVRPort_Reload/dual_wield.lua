-- DUAL WIELDING: a gun in each hand (the game's own dual wielding, Cool perk Cool_Central_Milestone_2).
--
-- While the game reports two guns in hand the plugin fires each gun from its own hand's trigger (the XInput
-- merge, held up here every frame through SetVRDualWield; CyberpunkVRPort_DualWield routes the game's
-- ShootState machine). A physical reload needs a free hand and both are on a grip, so the module's hand-driven
-- reload stays out (init.lua) and this one reloads each gun on its own:
--   * a TAP of Y drops the LEFT gun's magazine and B the RIGHT one's -- the magazine leaves the gun and falls, the
--     rounds go with it, and the gun is left empty and without a magazine. Y held is the game's own Y again
--     (weapon switch, weapon wheel): the plugin's XInput merge times the press and counts only the taps;
--   * a swing of that hand DOWN brings a fresh magazine back and fills the gun from the reserve;
--   * X stays the game's own reload, which reloads every gun in hand that can take rounds and brings its own
--     magazine with it.
local M = { active = false, published = false }

local SLOT = { [0] = 'AttachmentSlots.WeaponLeft', [1] = 'AttachmentSlots.WeaponRight' }
local Y_TAPS = 57      -- shared slot: the merge's count of Y taps (kDualWieldYTap)
local B_BTN = 157      -- shared slot: B pressed (kRightSecondaryBtn); B never reaches the game with a gun out

-- THE SWING THAT PUTS A MAGAZINE BACK. Read off the controller in MODEL space, the player's own frame, so walking,
-- falling or riding a lift is no swing at all -- the same reason the revolver's flick is measured there.
local SWING = {
    speed  = 2.0,    -- m/s straight down, filtered: a deliberate flick, not a lowered arm
    smooth = 0.03,   -- s, the velocity filter's time constant
    arm    = 0.25,   -- s after the drop before a swing counts, so the magazine is seen to leave first
}

local hands = { [0] = {}, [1] = {} }
local falls = {}      -- dropped magazines, each falling or lying where it fell until it is taken away
local litter = {}     -- ids whose entity did not resolve yet when it was time to take them away

local function gunIn(player, side)
    local ok, gun = pcall(function()
        return Game.GetTransactionSystem():GetItemInSlot(player, TweakDBID.new(SLOT[side]))
    end)
    return ok and gun or nil
end

local function entKey(gun)
    local ok, h = pcall(function() return gun:GetEntityID().hash end)
    return ok and tostring(h) or nil
end

local function len3(x, y, z) return math.sqrt(x * x + y * y + z * z) end

-- THE GUN'S OWN MAGAZINE, hidden on THIS gun only. reload.lua hides it through the rig's showMagazine track, and that
-- is no use with a gun in each hand: the plugin writes a track by rig SIGNATURE, and two pistols can share one (the
-- Liberty and the Nue both carry the 5-bone mag_std rig), so it would empty both guns. The mesh components are the
-- gun's own -- every one named mag_std*, the magazine, its reload twin and their decals -- and switching those off
-- is per entity. Only the ones that were on are switched back.
local function magParts(gun)
    local out = {}
    local ok, cs = pcall(function() return Game.VREntityComponents(gun) end)
    if not (ok and cs) then return out end
    for i = 1, #cs do
        local c = cs[i]
        local okn, nm = pcall(function() return tostring(c:GetName().value) end)
        if okn and nm and string.find(nm, 'mag_std', 1, true) then out[#out + 1] = c end
    end
    return out
end

local function hideMag(h, gun)
    if h.hidden then return end
    h.hidden = {}
    local parts = magParts(gun)
    for i = 1, #parts do
        local c = parts[i]
        local okE, en = pcall(function() return c:IsEnabled() end)
        if okE and en and pcall(function() c:Toggle(false) end) then h.hidden[#h.hidden + 1] = c end
    end
end

local function showMag(h)
    if not h.hidden then return end
    for i = 1, #h.hidden do
        local c = h.hidden[i]
        pcall(function() c:Toggle(true) end)
    end
    h.hidden = nil
end

-- The skin the gun's magazine is drawn in (reload.lua magSkin): the first mag_std component that is not on
-- `default`, since the shadow and decal ones stay there whatever the gun wears.
local function magSkin(gun)
    local parts = magParts(gun)
    for i = 1, #parts do
        local ok, a = pcall(function() return parts[i].meshAppearance end)
        local v = (ok and a) and tostring(a.value or a) or ''
        if v ~= '' and v ~= 'default' then return v end
    end
    return nil
end

-- Where the magazine sits in the gun, in world space -- reload.lua seatedWorld, against this gun's own slots.
local function seated(gun, mc, U)
    local cn = CName.new(mc.slot or 'vrp_mag_slot')
    local ok, cs = pcall(function() return Game.VREntityComponents(gun) end)
    if not (ok and cs) then return nil end
    for i = 1, #cs do
        local c = cs[i]
        local okc, cls = pcall(function() return tostring(c:GetClassName()) end)
        if okc and cls and string.find(cls, 'SlotComponent', 1, true) then
            local okt, f, tr = pcall(function() return c:GetSlotTransform(cn) end)
            if okt and f and tr then
                local okp, p = pcall(function() return WorldPosition.ToVector4(tr.Position) end)
                local okw, wq = pcall(function() return gun:GetWorldOrientation() end)
                if not (okp and p and okw and wq) then return nil end
                local s1, s2, s3, s4 = wq.i, wq.j, wq.k, wq.r
                local sr = mc.seatRot
                if sr then s1, s2, s3, s4 = U.qmul(s1, s2, s3, s4, sr[1], sr[2], sr[3], sr[4]) end
                local o = mc.originOffset
                local ox, oy, oz = 0.0, 0.0, 0.0
                if o then ox, oy, oz = U.qrot(s1, s2, s3, s4, o[1], o[2], o[3]) end
                return p.x + ox, p.y + oy, p.z + oz, s1, s2, s3, s4, wq
            end
        end
    end
    return nil
end

local function despawn(id, tries)
    local ok, e = pcall(function() return Game.FindEntityByID(id) end)
    if ok and e then
        pcall(function() exEntitySpawner.Despawn(e) end)
        return true
    end
    litter[#litter + 1] = { id = id, tries = (tries or 0) + 1 }
    return false
end

local function sweepLitter()
    if #litter == 0 then return end
    local keep = litter
    litter = {}
    for i = 1, #keep do
        local l = keep[i]
        if l.tries < 300 then despawn(l.id, l.tries) end
    end
end

-- A DROPPED MAGAZINE: the same body and contact model as reload.lua's dropTick (three probes along the long axis,
-- impulses with friction against the world's static geometry, then lying down on its side), on a table of its own
-- so each hand's magazine falls independently of the other and of the one-gun module's state.
local function dcfg(f, U, k)
    local o = f.mc.drop
    local v = o and o[k]
    if v ~= nil then return v end
    return U.DROP[k]
end

local function settle(f, U, hp, hn)
    local q = f.q
    local LG, TH = dcfg(f, U, 'long'), dcfg(f, U, 'thin')
    local ax1, ay1, az1 = U.qrot(q[1], q[2], q[3], q[4], TH[1], TH[2], TH[3])
    local nx2, ny2, nz2 = hn[1], hn[2], hn[3]
    if (ax1 * nx2 + ay1 * ny2 + az1 * nz2) < 0.0 then nx2, ny2, nz2 = -nx2, -ny2, -nz2 end
    local hx2, hy2, hz2 = U.qrot(q[1], q[2], q[3], q[4], LG[1], LG[2], LG[3])
    local d2 = hx2 * nx2 + hy2 * ny2 + hz2 * nz2
    hx2, hy2, hz2 = hx2 - d2 * nx2, hy2 - d2 * ny2, hz2 - d2 * nz2
    local hl = len3(hx2, hy2, hz2)
    if hl < 1e-3 then
        hx2, hy2, hz2 = U.qrot(q[1], q[2], q[3], q[4],
                               TH[2] * LG[3] - TH[3] * LG[2],
                               TH[3] * LG[1] - TH[1] * LG[3],
                               TH[1] * LG[2] - TH[2] * LG[1])
        d2 = hx2 * nx2 + hy2 * ny2 + hz2 * nz2
        hx2, hy2, hz2 = hx2 - d2 * nx2, hy2 - d2 * ny2, hz2 - d2 * nz2
        hl = len3(hx2, hy2, hz2)
    end
    if hl < 1e-3 then return end
    hx2, hy2, hz2 = hx2 / hl, hy2 / hl, hz2 / hl
    local yx, yy, yz = hy2 * nz2 - hz2 * ny2, hz2 * nx2 - hx2 * nz2, hx2 * ny2 - hy2 * nx2
    local t1, t2, t3, t4 = U.basisQuat(nx2, ny2, nz2, yx, yy, yz, hx2, hy2, hz2)
    local fl = dcfg(f, U, 'flat')
    f.q0 = { q[1], q[2], q[3], q[4] }
    f.q1 = { t1, t2, t3, t4 }
    f.c0 = { f.c[1], f.c[2], f.c[3] }
    f.c1 = { hp[1] + hn[1] * fl, hp[2] + hn[2] * fl, hp[3] + hn[3] * fl }
    f.rest = 0.0
    f.v[1], f.v[2], f.v[3] = 0.0, 0.0, 0.0
    f.w[1], f.w[2], f.w[3] = 0.0, 0.0, 0.0
end

local function applySkin(f)
    if not f.skin then return end
    local ok, e = pcall(function() return Game.FindEntityByID(f.id) end)
    if not (ok and e) then return end
    local done = pcall(function()
        local cs = Game.VREntityComponents(e)
        for i = 1, #cs do
            local c = cs[i]
            if tostring((c.name and c.name.value) or '') == 'mag_mesh' then
                c.meshAppearance = CName.new(f.skin)
                Game.VRComponentRefreshAppearance(c)
            end
        end
    end)
    if done then f.skin = nil end
end

-- One step of one magazine. False once it has been taken away.
local function fallStep(f, U, dt, floorZ)
    f.t = f.t + dt
    applySkin(f)
    local h = (dt > 0.04) and 0.04 or dt
    local c, v, w, q = f.c, f.v, f.w, f.q
    if f.rest then
        f.rest = f.rest + h
        local t = f.rest / (dcfg(f, U, 'settle') or 0.18)
        if t > 1.0 then t = 1.0 end
        q = U.qslerp(f.q0, f.q1, t)
        f.q = q
        for k = 1, 3 do c[k] = f.c0[k] + (f.c1[k] - f.c0[k]) * t end
    else
        v[3] = v[3] - (f.mc.gravity or 9.81) * h
        local dg = 1.0 - dcfg(f, U, 'drag') * h
        if dg < 0.0 then dg = 0.0 end
        v[1], v[2], v[3] = v[1] * dg, v[2] * dg, v[3] * dg
        local sg = 1.0 - dcfg(f, U, 'spinAir') * h
        if sg < 0.0 then sg = 0.0 end
        w[1], w[2], w[3] = w[1] * sg, w[2] * sg, w[3] * sg
        if (w[1] * w[1] + w[2] * w[2] + w[3] * w[3]) > 1e-8 then
            local a1, a2, a3, a4 = U.qmul(w[1] * 0.5 * h, w[2] * 0.5 * h, w[3] * 0.5 * h, 0.0,
                                          q[1], q[2], q[3], q[4])
            q = { q[1] + a1, q[2] + a2, q[3] + a3, q[4] + a4 }
            local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3] + q[4] * q[4])
            if l > 1e-9 then q = { q[1] / l, q[2] / l, q[3] / l, q[4] / l } end
            f.q = q
        end
        local n1, n2, n3 = c[1] + v[1] * h, c[2] + v[2] * h, c[3] + v[3] * h
        local sx2, sy2, sz2 = n1 - c[1], n2 - c[2], n3 - c[3]
        local sl = len3(sx2, sy2, sz2)
        local LG = dcfg(f, U, 'long')
        local ax1, ay1, az1 = U.qrot(q[1], q[2], q[3], q[4], LG[1], LG[2], LG[3])
        local rr, arm = dcfg(f, U, 'probe'), dcfg(f, U, 'arm')
        local fz = floorZ + (f.mc.floorLift or 0.02)
        local touching, best = {}, nil
        for pi = -1, 1 do
            local ox, oy, oz = ax1 * arm * pi, ay1 * arm * pi, az1 * arm * pi
            local px, py, pz = c[1] + ox, c[2] + oy, c[3] + oz
            local qx, qy, qz = n1 + ox, n2 + oy, n3 + oz
            local hp2, hn2
            if sl > 1e-4 then
                local kk = (sl + rr) / sl
                hp2, hn2 = U.rayHit(px, py, pz, px + sx2 * kk, py + sy2 * kk, pz + sz2 * kk)
                if hp2 and (v[1] * hn2[1] + v[2] * hn2[2] + v[3] * hn2[3]) > -1e-4 then hp2 = nil end
            end
            if not hp2 and v[3] < 0.0 and (qz - rr) <= fz then
                hp2, hn2 = { qx, qy, fz }, { 0.0, 0.0, 1.0 }
            end
            if hp2 then
                local pen = rr + 0.001 - ((qx - hp2[1]) * hn2[1] + (qy - hp2[2]) * hn2[2] + (qz - hp2[3]) * hn2[3])
                if pen > 0.0 then
                    touching[#touching + 1] = { pen = pen, hp = hp2, hn = hn2, rx = ox, ry = oy, rz = oz }
                    best = touching[#touching]
                end
            end
        end
        c[1], c[2], c[3] = n1, n2, n3
        for ci = 1, #touching do
            local ct = touching[ci]
            local hn, rx, ry, rz = ct.hn, ct.rx, ct.ry, ct.rz
            local corr = ct.pen - 0.001
            if corr > 0.0 then
                c[1], c[2], c[3] = c[1] + hn[1] * corr * 0.8, c[2] + hn[2] * corr * 0.8, c[3] + hn[3] * corr * 0.8
            end
            local cvx = v[1] + (w[2] * rz - w[3] * ry)
            local cvy = v[2] + (w[3] * rx - w[1] * rz)
            local cvz = v[3] + (w[1] * ry - w[2] * rx)
            local vn = cvx * hn[1] + cvy * hn[2] + cvz * hn[3]
            if vn < 0.0 then
                local II, mu = dcfg(f, U, 'inertia'), dcfg(f, U, 'friction')
                local e2 = (-vn > dcfg(f, U, 'restE')) and dcfg(f, U, 'bounce') or 0.0
                local cx, cy, cz = ry * hn[3] - rz * hn[2], rz * hn[1] - rx * hn[3], rx * hn[2] - ry * hn[1]
                local jn = -(1.0 + e2) * vn / (1.0 + (cx * cx + cy * cy + cz * cz) / II)
                v[1], v[2], v[3] = v[1] + hn[1] * jn, v[2] + hn[2] * jn, v[3] + hn[3] * jn
                w[1], w[2], w[3] = w[1] + cx * jn / II, w[2] + cy * jn / II, w[3] + cz * jn / II
                local tx2, ty2, tz2 = cvx - vn * hn[1], cvy - vn * hn[2], cvz - vn * hn[3]
                local tl = len3(tx2, ty2, tz2)
                if tl > 1e-6 then
                    local ux, uy, uz = tx2 / tl, ty2 / tl, tz2 / tl
                    local dx2, dy2, dz2 = ry * uz - rz * uy, rz * ux - rx * uz, rx * uy - ry * ux
                    local jt = tl / (1.0 + (dx2 * dx2 + dy2 * dy2 + dz2 * dz2) / II)
                    if jt > mu * jn then jt = mu * jn end
                    v[1], v[2], v[3] = v[1] - ux * jt, v[2] - uy * jt, v[3] - uz * jt
                    w[1], w[2], w[3] = w[1] - dx2 * jt / II, w[2] - dy2 * jt / II, w[3] - dz2 * jt / II
                end
                local dl = 1.0 - dcfg(f, U, 'spinDamp')
                if dl < 0.0 then dl = 0.0 end
                w[1], w[2], w[3] = w[1] * dl, w[2] * dl, w[3] * dl
                local wl, wm = len3(w[1], w[2], w[3]), dcfg(f, U, 'maxW')
                if wl > wm then w[1], w[2], w[3] = w[1] / wl * wm, w[2] / wl * wm, w[3] / wl * wm end
            end
        end
        if best then
            f.touch = f.touch + h
            if (len3(v[1], v[2], v[3]) < dcfg(f, U, 'restV') and len3(w[1], w[2], w[3]) < 3.0)
               or (f.touch > dcfg(f, U, 'touchRest')) then
                settle(f, U, best.hp, best.hn)
            end
        end
    end
    local ctr = f.ctr
    local ox2, oy2, oz2 = U.qrot(q[1], q[2], q[3], q[4], ctr[1], ctr[2], ctr[3])
    local ok, e = pcall(function() return Game.FindEntityByID(f.id) end)
    if ok and e then
        pcall(function()
            local tr = WorldTransform.new()
            tr:SetPosition(Vector4.new(c[1] - ox2, c[2] - oy2, c[3] - oz2, 1.0))
            tr:SetOrientation(Quaternion.new(q[1], q[2], q[3], q[4]))
            Game.VREntitySetWorldTransform(e, tr)
        end)
    end
    if f.t >= (f.mc.litterTime or 3.0) then
        despawn(f.id)
        return false
    end
    return true
end

-- The magazine leaves the gun along its own well (reload.lua's B drop: `wellAxis` points out of the gun, `dropPush`
-- is how hard), as the weapon's own magazine entity in the gun's own skin.
local function spawnFall(gun, mc, U)
    if type(exEntitySpawner) == 'nil' or not (mc and mc.entity) or mc.eject then return end
    local px, py, pz, s1, s2, s3, s4, wq = seated(gun, mc, U)
    if not px then return end
    local id = nil
    pcall(function()
        local tr = WorldTransform.new()
        tr:SetPosition(Vector4.new(px, py, pz, 1.0))
        tr:SetOrientation(Quaternion.new(s1, s2, s3, s4))
        id = exEntitySpawner.Spawn(mc.dropEntity or mc.entity, tr, '')
    end)
    if not id then return end
    local cen = (mc.drop and mc.drop.centre) or U.DROP.centre
    local o = mc.originOffset
    local ctr = o and { cen[1] - o[1], cen[2] - o[2], cen[3] - o[3] } or { cen[1], cen[2], cen[3] }
    local cx, cy, cz = U.qrot(s1, s2, s3, s4, ctr[1], ctr[2], ctr[3])
    local push = mc.dropPush or 1.1
    local kx, ky, kz = 0.0, 0.0, -push
    local wa = mc.wellAxis
    if wa then
        local ux, uy, uz = U.qrot(wq.i, wq.j, wq.k, wq.r, wa[1], wa[2], wa[3])
        kx, ky, kz = ux * push, uy * push, uz * push
    end
    falls[#falls + 1] = {
        id = id, mc = mc, ctr = ctr, skin = magSkin(gun),
        q = { s1, s2, s3, s4 }, c = { px + cx, py + cy, pz + cz },
        v = { kx, ky, kz }, w = { 0.0, 0.0, 0.0 }, t = 0.0, touch = 0.0,
    }
end

local function sound(gun, cfg, U, which)
    local name = cfg and cfg.sounds and cfg.sounds[which]
    if name then U.playSnd(gun, name) end
end

local function log(s)
    pcall(function() spdlog.info('[DualWield] ' .. s) end)
end

-- THE DROP: the magazine is out of the gun and on its way down, the gun is empty and shows an empty well. The rounds
-- go with the magazine -- the game's own consume event takes them, the setters are rejected from CET.
local function dropMag(side, h, gun, reload, U)
    local _, cfg = reload.cfgOf(gun)
    local mc = cfg and cfg.mag
    local n = U.magAmmo(gun) or 0
    if n > 0 then U.setMagAmmo(gun, 0) end
    spawnFall(gun, mc, U)
    hideMag(h, gun)
    sound(gun, cfg, U, 'magOut')
    h.out, h.t = true, 0.0
    log(string.format('%s magazine dropped (%d rounds)', side == 0 and 'left' or 'right', n))
end

-- THE SWING: a fresh magazine, filled from the reserve. The set event pays for itself -- the rounds in a magazine
-- are part of the inventory count, so raising it spends the reserve (reload.lua setMagAmmo).
local function returnMag(side, h, gun, reload, U)
    local cap = U.magCapacity(gun)
    local inv = U.reserveAmmo(gun)
    local n = (inv < cap) and inv or cap
    if n <= 0 then return false end
    local _, cfg = reload.cfgOf(gun)
    U.setMagAmmo(gun, n)
    showMag(h)
    sound(gun, cfg, U, 'magIn')
    h.out = false
    log(string.format('%s magazine back (%d rounds)', side == 0 and 'left' or 'right', n))
    return true
end

-- The hand's vertical speed in model space, filtered.
local function handVz(h, side, dt)
    if type(VRHandRawModel) ~= 'function' then return 0.0 end
    local p = VRHandRawModel(side)
    if not (p and (p.w or 0) > 0.5) or dt <= 1e-4 then
        h.pz = nil
        return 0.0
    end
    if h.pz then
        local a = 1.0 - math.exp(-dt / SWING.smooth)
        h.vz = (h.vz or 0.0) + ((p.z - h.pz) / dt - (h.vz or 0.0)) * a
    else
        h.vz = 0.0
    end
    h.pz = p.z
    return h.vz
end

-- THE GAME'S RELOAD OF THIS HAND'S GUN. With a gun in each hand the state machine reports each one on its own entry:
-- measured, the left gun reloading reads LeftWeapon = 2 while RightWeapon stays at 5 -- so reload.lua's
-- gameReloading, which reads RightWeapon, never sees the left gun's.
local PSM_KEY = { [0] = 'LeftWeapon', [1] = 'RightWeapon' }
local function handReloading(player, side)
    local ok, v = pcall(function()
        return player:GetPlayerStateMachineBlackboard():GetInt(GetAllBlackboardDefs().PlayerStateMachine[PSM_KEY[side]])
    end)
    return ok and v == 2                     -- gamePSMRangedWeaponStates.Reload
end

local function handFrame(side, player, dual, reload, U, dt)
    local h = hands[side]
    local gun = dual and gunIn(player, side) or nil
    local key = gun and entKey(gun) or nil
    -- A gun that is put away or swapped takes nothing of ours with it: its components are switched back on, and
    -- it stays empty -- that magazine is on the floor.
    if h.out and (not gun or key ~= h.key) then
        showMag(h)
        h.out = false
    end
    h.key = key

    local edge
    if side == 0 then
        local taps = GetVRSharedSlot(Y_TAPS) or 0.0
        edge = h.taps ~= nil and taps ~= h.taps
        h.taps = taps
    else
        local btn = (GetVRSharedSlot(B_BTN) or 0.0) > 0.5
        edge = btn and not h.btnWas
        h.btnWas = btn
    end
    local vz = handVz(h, side, dt)
    if not gun then return end

    local reloading = handReloading(player, side)
    if h.out then
        h.t = h.t + dt
        -- The game's reload (X, or the trigger on an empty gun) brings a magazine of its own.
        if reloading or (h.t > 0.3 and (U.magAmmo(gun) or 0) > 0) then
            showMag(h)
            h.out = false
            log((side == 0 and 'left' or 'right') .. ' magazine back with the game\'s reload')
        elseif h.t > SWING.arm and vz < -SWING.speed then
            returnMag(side, h, gun, reload, U)
        end
    elseif edge and not reloading then
        dropMag(side, h, gun, reload, U)
    end
end

-- THE LEFT BARREL, for the hitscan hook (src/Hooks/WeaponAim.cpp): the right gun's muzzle is published by the
-- weapon mod, and a shot the game fires from the left gun is sent out of this one. Same accessors as the weapon
-- mod's updateMuzzle: the transform's `position` (17-bit fixed point or floats) and `Orientation`.
local function publishLeftMuzzle(gun)
    if type(SetVRLeftMuzzle) ~= 'function' then return end
    local ok = pcall(function()
        local xf = gun:GetMuzzleSlotWorldTransform()
        local q = xf and (xf.Orientation or (xf.GetOrientation and xf:GetOrientation()))
        local pos = xf and (xf.position or (xf.GetPosition and xf:GetPosition()))
        if not (q and pos) then error('no transform') end
        local x, y, z
        if type(pos.x) == 'number' then
            x, y, z = pos.x, pos.y, pos.z
        elseif pos.x and pos.x.Bits then
            x, y, z = pos.x.Bits / 131072.0, pos.y.Bits / 131072.0, pos.z.Bits / 131072.0
        else
            local v = WorldPosition.ToVector4(pos)
            x, y, z = v.x, v.y, v.z
        end
        SetVRLeftMuzzle(x, y, z, q.i, q.j, q.k, q.r)
    end)
    if not ok then pcall(SetVRLeftMuzzle, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0) end
end

-- WHICH GUN A SHOT CAME FROM: the shot carries its weapon's item record, and the hook compares it with these two
-- (TweakDBID hashes as signed 32-bit, 0 = no gun). muzzlePosition in the shot is empty for hitscan.
local function recordHash(gun)
    local ok, h = pcall(function() return ItemID.GetTDBID(gun:GetItemID()).hash end)
    if not ok or type(h) ~= 'number' then return 0 end
    if h >= 2147483648 then h = h - 4294967296 end
    return h
end

local function publishRecords(player, left)
    if type(SetVRDualWieldRecords) ~= 'function' then return end
    local right = left and gunIn(player, 1) or nil
    pcall(SetVRDualWieldRecords, left and recordHash(left) or 0, right and recordHash(right) or 0)
end

-- Once per frame, before the hand-driven reload. Returns true while two guns are in hand.
function M.frame(reload, dt)
    if type(SetVRDualWield) ~= 'function' then return false end
    dt = dt or 0.016
    local player = Game.GetPlayer()
    local dual = false
    if player then
        local ok, v = pcall(function() return GameObject.IsDualWielding(player) end)
        dual = ok and v == true
    end
    pcall(SetVRDualWield, dual and 1 or 0)
    M.active = dual

    local gun = dual and gunIn(player, 0) or nil
    if gun then
        publishLeftMuzzle(gun)
    elseif M.published and type(SetVRLeftMuzzle) == 'function' then
        pcall(SetVRLeftMuzzle, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
    end
    if gun or M.published then publishRecords(player, gun) end
    M.published = gun ~= nil

    local U = reload and reload.util
    if not (U and reload.cfgOf and type(GetVRSharedSlot) == 'function') then return dual end
    for side = 0, 1 do handFrame(side, player, dual, reload, U, dt) end

    if #falls > 0 then
        local floorZ = 0.0
        pcall(function() floorZ = player:GetWorldPosition().z end)
        local keep = {}
        for i = 1, #falls do
            local ok, alive = pcall(fallStep, falls[i], U, dt, floorZ)
            if ok and alive then
                keep[#keep + 1] = falls[i]
            elseif not ok then
                despawn(falls[i].id)
                log('fall step failed: ' .. tostring(alive))
            end
        end
        falls = keep
    end
    sweepLitter()
    return dual
end

-- Everything put back: the guns' magazines on, the dropped ones taken out of the world (init.lua onShutdown).
function M.release()
    for side = 0, 1 do
        showMag(hands[side])
        hands[side].out = false
    end
    for i = 1, #falls do despawn(falls[i].id) end
    falls = {}
end

function M.dbg()
    local function one(side)
        local h = hands[side]
        return string.format('%s%s vz=%.2f', side == 0 and 'L' or 'R', h.out and ':out' or '', h.vz or 0.0)
    end
    return string.format('%s %s falls=%d', one(0), one(1), #falls)
end

return M
