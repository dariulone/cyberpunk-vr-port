-- THE RELOAD'S GLOW: what the gun in one hand wants from the other, shown on the gun.
--
--   empty   its magazine is in and dry: the magazine pulses, lit amber over its own shape;
--   out     its magazine is out: the shape stays where the magazine goes, steady;
--   rack    rounds wait on the slide (a magazine seated into a dry gun, a manual action after its shot): the part the
--           hand pulls pulses as the magazine does, drawn on the weapon's slot of that part (vrp_<bone>: a slot on a
--           bone reports that bone's live transform, wherever the part is along its travel). A gun with no copy of
--           its part gets the reach marker's brackets (CyberpunkVRPort_Reach) on the slide's slot instead.
-- What the gun wants is the reload's own state (reload.lua M.wants); this module only draws it.
--
-- THE SHAPE is the magazine's own mesh again, on the game's pick-up highlight in amber, pushed out a millimetre and a half
-- along its normals so it does not fight the magazine for the depth (tools/reload_glow/make_reload_glow.py; the list
-- of copies is reload/glow_list.lua). It is drawn by a carrier of the player's (vrp_glow_left / vrp_glow_right,
-- tools/reload_glow: the slots the gun and the held magazine are on), the way the held magazine is: a component on
-- the hand's own slot is carried by the engine with the hand, exact, where a spawned entity would trail it a frame.
-- The copy is in the magazine's own frame, its origin where the magazine enters the well (most meshes: its top
-- face), so the carrier is put on the reload's own seated pose (reload.lua M.seated, the one a dropped magazine
-- leaves the gun from) as seen from the hand's slot. Gun and slot are rigid together, so that relation holds.
-- The pulled part's copy is the gun's own triangles skinned to that part, in the frame of its bone (glow_list.lua
-- `bolts`, keyed by the gun's magazine mesh), so the carrier goes where the part's slot is.
--
-- THE PULSE is the shader's own (base\materials\loot_drop_highlight.mt, the belt's marks' material): the copy's
-- appearance vrp_glow pulses on the GPU, smooth at any frame rate; vrp_glow_ghost is the same light, steady. Stepping
-- through ten materials a step brighter from here (what this was) changed it seven times a second: a 5 fps pulse.
local M = {}

local LIST = require('reload/glow_list')
local CARRIER = { [0] = 'vrp_glow_left', [1] = 'vrp_glow_right' }
local SLOT = { [0] = 'WeaponLeft', [1] = 'WeaponRight' }
local PULSE, GHOST = 'vrp_glow', 'vrp_glow_ghost'
local PERIOD = 3.0          -- s, one blink of the handle's brackets (a gun with no copy of its part)
local RACK_ON = 0.55        -- of a period the handle's brackets are shown

local S = { comp = {}, mesh = {}, look = {}, slots = nil, time = 0.0, missed = {} }
-- A player entity without the carriers or the slots component (a template the pack does not reach) is asked again
-- only this often: looked for every frame, they were most of the reload's cost (2.7 ms a frame).
local RETRY = 2.0

local function qconj(q) return -q.i, -q.j, -q.k, q.r end
local function qmul(ai, aj, ak, ar, bi, bj, bk, br)
    return ar * bi + ai * br + aj * bk - ak * bj,
           ar * bj - ai * bk + aj * br + ak * bi,
           ar * bk + ai * bj - aj * bi + ak * br,
           ar * br - ai * bi - aj * bj - ak * bk
end
local function qrot(qi, qj, qk, qr, x, y, z)
    local tx, ty, tz = 2.0 * (qj * z - qk * y), 2.0 * (qk * x - qi * z), 2.0 * (qi * y - qj * x)
    return x + qr * tx + (qj * tz - qk * ty), y + qr * ty + (qk * tx - qi * tz), z + qr * tz + (qi * ty - qj * tx)
end

-- By the engine's own lookup on the player (FindComponentByName), not a walk of its components through Lua.
local CARRIER_CN = {}
local function findCarrier(pl, cn) return pl:FindComponentByName(cn) end
local function carrier(side)
    local c = S.comp[side]
    if c then return c end
    if S.time - (S.missed[side] or -RETRY) < RETRY then return nil end
    local pl = Game.GetPlayer()
    local cn = CARRIER_CN[side]
    if not cn then cn = CName.new(CARRIER[side]); CARRIER_CN[side] = cn end
    local ok, found = pcall(findCarrier, pl, cn)
    if not (ok and found) then S.missed[side] = S.time; return nil end
    S.comp[side] = found
    -- drawn with the gun, at its finest LOD (see reload.lua's holdComp)
    pcall(function() found.renderingPlane = 'RPl_Weapon' end)
    pcall(function() found.forceLODLevel = 0 end)
    return found
end

-- Called through pcall without a closure: this runs every frame.
local function slotTransform(comp, name) return comp:GetSlotTransform(name) end
local function setLocal(c, p, q) c:SetLocalTransform(p, q) end

local function slotComp()
    if S.slots then return S.slots end
    if S.time - (S.missed.slots or -RETRY) < RETRY then return nil end
    S.missed.slots = S.time
    local pl = Game.GetPlayer()
    local ok, cs = pcall(function() return Game.VREntityComponents(pl) end)
    if not (ok and cs) then return nil end
    for i = 1, #cs do
        if tostring(cs[i]:GetName()):find('ItemAttachmentSlots') then S.slots = cs[i]; return cs[i] end
    end
    return nil
end

-- The glow on a side: an appearance of the copy (PULSE or GHOST), or nil for off. Written on a change only: an
-- appearance change is not free.
local function show(side, glowMesh, look)
    local c = carrier(side)
    if not c then return false end
    if not look then
        if S.look[side] then pcall(function() c:Toggle(false) end) end
        S.look[side] = nil
        return true
    end
    if S.mesh[side] ~= glowMesh then
        if not pcall(function() Game.VRComponentChangeResource(c, glowMesh, true) end) then return false end
        S.mesh[side], S.look[side] = glowMesh, nil
        pcall(function() c:Toggle(false) end)
    end
    if S.look[side] ~= look then
        pcall(function() Game.VRComponentChangeAppearance(c, CName.new(look), true) end)
        if not S.look[side] then pcall(function() c:Toggle(true) end) end
        S.look[side] = look
    end
    return true
end

-- The carrier on the hand's slot, at a world pose as seen from that slot.
local SLOT_CN = {}
local function carry(side, mx, my, mz, m1, m2, m3, m4)
    local sc, c = slotComp(), carrier(side)
    if not (sc and c) then return false end
    local cn = SLOT_CN[side]
    if not cn then cn = CName.new(SLOT[side]); SLOT_CN[side] = cn end
    local ok, found, tr = pcall(slotTransform, sc, cn)
    if not (ok and found and tr) then S.slots = nil; return false end
    local sp, sq = WorldPosition.ToVector4(tr.Position), tr.Orientation
    local ci, cj, ck, cr = qconj(sq)
    local lx, ly, lz = qrot(ci, cj, ck, cr, mx - sp.x, my - sp.y, mz - sp.z)
    local qi, qj, qk, qr = qmul(ci, cj, ck, cr, m1, m2, m3, m4)
    return pcall(setLocal, c, Vector4.new(lx, ly, lz, 1.0), Quaternion.new(qi, qj, qk, qr))
end

-- At the magazine's seated pose.
local function placeMagazine(side, reload, weapon, weaponSlots)
    if not reload.seated then return false end
    local okm, mx, my, mz, m1, m2, m3, m4 = pcall(reload.seated, weapon, weaponSlots)
    if not (okm and mx) then return false end
    return carry(side, mx, my, mz, m1, m2, m3, m4)
end

-- On the weapon's slot of the pulled part.
local PART_CN = {}
local function placePart(side, weaponSlots, slotName)
    local cn = PART_CN[slotName]
    if not cn then cn = CName.new(slotName); PART_CN[slotName] = cn end
    local ok, found, tr = pcall(slotTransform, weaponSlots, cn)
    if not (ok and found and tr) then return false end
    local p, q = WorldPosition.ToVector4(tr.Position), tr.Orientation
    return carry(side, p.x, p.y, p.z, q.i, q.j, q.k, q.r)
end

local function off()
    for side = 0, 1 do
        if S.look[side] then show(side, S.mesh[side], nil) end
    end
end

local reach = nil
local function markRack(side, weaponSlots, slotName, on)
    if not (on and weaponSlots and slotName) then return end
    reach = reach or GetMod('CyberpunkVRPort_Reach')
    if not (reach and reach.mark) then return end
    local ok, found, tr = pcall(function() return weaponSlots:GetSlotTransform(CName.new(slotName)) end)
    if not (ok and found and tr) then return end
    local okp, p = pcall(function() return WorldPosition.ToVector4(tr.Position) end)
    if not (okp and p) then return end
    pcall(reach.mark, side + 1, { pos = { p.x, p.y, p.z }, radius = 0.035, lift = 0.0, kind = 'lock', ready = true })
end

-- Every frame: the gun, its holder (0 left, 1 right), the weapon's own slot component, and the reload module.
function M.frame(reload, weapon, holder, weaponSlots, dt)
    S.time = S.time + (dt or 0.016)
    if not (reload and reload.wants and weapon and (holder == 0 or holder == 1)) then off(); return end
    local okW, want, mesh, slideSlot = pcall(reload.wants, weapon)
    if not okW then off(); return end
    if S.forced and S.time < S.forced.till then want = S.forced.want end
    local glowMesh = mesh and LIST.meshes[mesh] or nil
    local other = 1 - holder
    if S.look[other] then show(other, S.mesh[other], nil) end
    local part = want == 'rack' and mesh and LIST.bolts and LIST.bolts[mesh] or nil
    if want == 'rack' and not part then
        markRack(holder, weaponSlots, slideSlot, (S.time % PERIOD) < RACK_ON * PERIOD)
    end
    local look = nil
    if part or (glowMesh and want == 'empty') then
        look = PULSE
    elseif glowMesh and want == 'out' then
        look = GHOST
    end
    if part then
        glowMesh = part.mesh
        if look and not placePart(holder, weaponSlots, part.slot) then look = nil end
    elseif look and not placeMagazine(holder, reload, weapon, weaponSlots) then
        look = nil
    end
    show(holder, glowMesh, look)
end

function M.clear()
    off()
    S.comp, S.mesh, S.look, S.slots, S.missed = {}, {}, {}, nil, {}
end

-- Test: what the gun wants, said for it for that many seconds ('empty', 'out', 'rack').
function M.force(want, seconds)
    S.forced = { want = want, till = S.time + (seconds or 3.0) }
end

function M.debug() return S end

return M
