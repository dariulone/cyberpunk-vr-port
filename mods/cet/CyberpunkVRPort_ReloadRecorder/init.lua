-- CyberpunkVRPort — RELOAD RECORDER, its own mod.
--
-- Records the game's OWN reload animation per frame (VRIK off, so the anim actually plays on the arms): both
-- wrists, the WeaponLeft/WeaponRight anchor bones, the back_slider/front_slider/mag_std slots and every finger
-- joint of both hands — all in MODEL space. Grip poses and wrist placements are lifted from these takes exactly,
-- with no offline conversion (the GLB->runtime route failed for the arm rig; the recording route measured 0.2 mm
-- of spread across a hold).
--
-- A SEPARATE mod ON PURPOSE: CET keeps ONE callback per event per mod, so a second registerForEvent('onUpdate')
-- silently replaces the first. Hosting this recorder inside the HandCollision mod replaced its main loop and
-- killed collision + reload for three sessions with zero errors anywhere. Never merge it back.
--
-- ALSO RECORDS A GRENADE THROW, and nearly all of it for free: what the game does to the arms is the
-- same three things either way -- the two wrists, the hand ANCHOR bones (`wl` / `wr`, which is where an
-- item in AttachmentSlots.WeaponLeft/Right is put), and every finger joint. That covers the pin pull,
-- the wind-up and the release without a line of new code.
--
-- What is NOT free is where the grenade itself sits, because the weapon fields cannot answer it: they
-- read the ACTIVE WEAPON, and during a throw there is none -- `sl`/`sf`/`mg`/`wq` come back empty and
-- `mb`/`fb` are the holstered gun's, not the grenade's. So `hl` / `hr` below read the ITEM IN THE HAND
-- SLOT directly, which is the thing being held whatever it is.
--
-- Use: 1) VRIK off  2) CET overlay -> "VR Reload Recorder" -> Record  3) do the reload OR throw a
-- grenade  4) STOP + save.
-- Output: reload_record_NN.lua in THIS mod's directory (auto-numbered, never overwrites).

local REC = { on = false, t = 0.0, n = 0, samples = {}, sc = nil, last = nil }
local overlayOpen = false
registerForEvent('onOverlayOpen',  function() overlayOpen = true end)
registerForEvent('onOverlayClose', function() overlayOpen = false end)

-- Finger bone indices from player_bone_names.txt (fixed player skeleton). The dump order inside fL/fR follows
-- THESE lists: 5 metacarpals, then thumb 1-2, then index/middle/ring/pinky 1-2-3.
local FINGERS_L = { 33, 34, 35, 36, 37,  45, 55,  46, 56, 65,  47, 57, 66,  48, 58, 67,  49, 59, 68 }
local FINGERS_R = { 38, 39, 40, 41, 42,  50, 60,  51, 61, 69,  52, 62, 70,  53, 63, 71,  54, 64, 72 }

local function qrot(i, j, k, r, x, y, z)
    local tx = 2.0 * (j * z - k * y)
    local ty = 2.0 * (k * x - i * z)
    local tz = 2.0 * (i * y - j * x)
    return x + r * tx + (j * tz - k * ty),
           y + r * ty + (k * tx - i * tz),
           z + r * tz + (i * ty - j * tx)
end

local function qmul(ax, ay, az, aw, bx, by, bz, bw)
    return aw * bx + ax * bw + ay * bz - az * by,
           aw * by - ax * bz + ay * bw + az * bx,
           aw * bz + ax * by - ay * bx + az * bw,
           aw * bw - ax * bx - ay * by - az * bz
end

-- world -> model frame from the player transform (same convention as the HandCollision mod)
local function playerFrame()
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local ok, base = pcall(function() return pl:GetWorldPosition() end)
    if not ok or not base then return nil end
    local ok2, q = pcall(function() return pl:GetWorldOrientation() end)
    if not ok2 or not q then return nil end
    return base.x, base.y, base.z, -q.i, -q.j, -q.k, q.r
end

local function vecStr(t)
    if not t then return '' end
    local s = {}
    for i = 1, #t do s[i] = string.format('%.5f', t[i]) end
    return table.concat(s, ',')
end

-- one bone as 7 floats (model-space position + rotation quat)
local function bonePQ(idx)
    local p = VRBoneModelPos(idx)
    local q = VRBoneModelRot(idx)
    if not (p and q) then return nil end
    return { p.x, p.y, p.z, q.i, q.j, q.k, q.r }
end

-- a finger list flattened to 19 x 7 floats (zeros for a bone that failed to read, so the order never shifts)
local function fingerRow(list)
    local out = {}
    for i = 1, #list do
        local v = bonePQ(list[i])
        if v then for k = 1, 7 do out[#out + 1] = v[k] end
        else for k = 1, 7 do out[#out + 1] = 0.0 end end
    end
    return out
end

-- THE WEAPON RIGS' OWN BONES, as the game leaves them: `VRRigBone(which, bone, field)` returns that bone's
-- parent-local translation (fields 0..2) and rotation quaternion (3..6), read out of the pose buffer before the
-- port writes anything. which 0 = magazine rig, 1 = frame rig.
--
-- This exists because nothing else can see a weapon part move: the vrp_ slots do not ride the bones (the magazine
-- slot is really the WELL, measured), and a skinned mesh component's GetLocalToWorld is just the weapon entity's
-- transform -- recording mag_std and mag_stdr that way produced two bit-identical columns.
-- EVERY magazine-rig bone, for the same reason the frame's list is exhaustive one comment below. Five was right
-- for twelve pistols running -- mag_plug, magazine, magazine_reload, mag_std, mag_stdr, and no magazine rig in the
-- set had more -- and it stopped being right at the first REVOLVER: the Malorian Overture's speedloader has ELEVEN,
-- six of them `bullet_01..06`, one per chamber. The take recorded the first five and said nothing, which is why its
-- speedloader would not resolve: the rounds that carry the whole gesture were never in the file.
local MAG_BONES   = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 }
-- EVERY frame-rig bone, not a hand-picked eight. Those indices were the Silverhand's parts; on another weapon the
-- same numbers are different bones (the Unity's rig has 11 bones, so half of them do not even exist). Reading all
-- sixteen costs a few hundred bytes a frame and makes a recording readable for any gun.
-- Twenty-one since the Kang Tao Zhuo, whose eight barrels and their muzzles fill a 21-bone frame rig.
local FRAME_BONES = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20 }
-- THE RECEIVER, the third rig: a break-action shotgun's stock and action, where its top lever (`barrel_open`) lives.
-- Six bones on every Rostovic; reading ten costs nothing on a rig that has fewer.
-- Nineteen since the Kang Tao Zhuo: its magazine sits in the RECEIVER, between two pairs of holders and their clips.
local RCV_BONES   = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18 }
local function rigRow(which, bones)
    if type(VRRigBone) ~= 'function' then return nil end
    local out = {}
    for i = 1, #bones do
        for f = 0, 6 do
            local ok, v = pcall(function() return VRRigBone(which, bones[i], f) end)
            out[#out + 1] = (ok and v) or 0.0
        end
    end
    return out
end

-- THE MAGAZINE RIG'S FLOAT TRACKS, as the game drives them. A shotgun's shells are not bones that leave -- they are
-- meshes switched on and off (showMagazineLeft / ReloadLeft / Right / ReloadRight), and WHEN each switches is the
-- whole timeline of a reload: which chamber is emptied, when a fresh shell appears in the hand, when it is home.
local function trackRow(which)
    if type(VRRigTrack) ~= 'function' then return nil end
    local out = {}
    local ok, n = pcall(function() return VRRigTrack(which, -1) end)
    n = (ok and n) or 0
    if n > 8 then n = 8 end
    for i = 0, n - 1 do
        local o, v = pcall(function() return VRRigTrack(which, i) end)
        out[#out + 1] = (o and v) or 0.0
    end
    return out
end

-- ...and the gun's own round count, so a take says when each shell was counted in.
local function ammoNow()
    local pl = Game.GetPlayer()
    local w = pl and pl:GetActiveWeapon()
    if not w then return nil end
    local ok, n = pcall(function() return w:GetMagazineAmmoCount() end)
    return (ok and type(n) == 'number') and n or nil
end

-- one weapon slot position in MODEL space
local function slotModel(name)
    local pl = Game.GetPlayer()
    local w = pl and pl:GetActiveWeapon()
    if not w then return nil end
    if not REC.sc then
        local ok, cs = pcall(function() return Game.VREntityComponents(w) end)
        if ok and cs then
            for i = 1, #cs do
                if string.find(tostring(cs[i]:GetClassName()), 'SlotComponent') then
                    local o, f = pcall(function() return cs[i]:GetSlotTransform(CName.new(name)) end)
                    if o and f then REC.sc = cs[i]; break end
                end
            end
        end
    end
    if not REC.sc then return nil end
    local o, f, tr = pcall(function() return REC.sc:GetSlotTransform(CName.new(name)) end)
    if not (o and f and tr) then return nil end
    local v = WorldPosition.ToVector4(tr.Position)
    local bx, by, bz, ci, cj, ck, cr = playerFrame()
    if not bx then return nil end
    local x, y, z = qrot(ci, cj, ck, cr, v.x - bx, v.y - by, v.z - bz)
    return { x, y, z }
end

-- A slot's ORIENTATION, in model space. Position alone is not enough for anything that has to be PLACED rather
-- than merely reached: a magazine's seated pose is the mount's whole transform, and reconstructing its rotation
-- from three slot positions costs a basis whose roll comes off a 5.8 mm lever on this pistol.
local function slotRotModel(name)
    if not REC.sc then return nil end
    local o, f, tr = pcall(function() return REC.sc:GetSlotTransform(CName.new(name)) end)
    if not (o and f and tr) then return nil end
    local q = tr.Orientation
    local _, _, _, ci, cj, ck, cr = playerFrame()
    if not ci then return nil end
    local a, b, c, d = qmul(ci, cj, ck, cr, q.i, q.j, q.k, q.r)
    return { a, b, c, d }
end

local function slotRotAny(names)
    for i = 1, #names do
        local v = slotRotModel(names[i])
        if v then return v end
    end
    return nil
end

-- THE WEAPON'S OWN ORIENTATION, in model space. Exact and rigid -- no lever arm, unaffected by a moving slide --
-- so every grip and every hold measured against it is free of the roll error a slot-derived basis carries.
local function weaponRotModel()
    local pl = Game.GetPlayer()
    local w = pl and pl:GetActiveWeapon()
    if not w then return nil end
    local ok, q = pcall(function() return w:GetWorldOrientation() end)
    if not (ok and q) then return nil end
    local _, _, _, ci, cj, ck, cr = playerFrame()
    if not ci then return nil end
    local a, b, c, d = qmul(ci, cj, ck, cr, q.i, q.j, q.k, q.r)
    return { a, b, c, d }
end

-- ...and its POSITION, which a weapon whose slots all ride moving parts cannot otherwise give: a break-action's
-- every slot past the hinge swings with the barrel, so the receiver's own place has to be read off the entity.
local function weaponPosModel()
    local pl = Game.GetPlayer()
    local w = pl and pl:GetActiveWeapon()
    if not w then return nil end
    local ok, p = pcall(function() return w:GetWorldPosition() end)
    if not (ok and p) then return nil end
    local bx, by, bz, ci, cj, ck, cr = playerFrame()
    if not bx then return nil end
    local x, y, z = qrot(ci, cj, ck, cr, p.x - bx, p.y - by, p.z - bz)
    return { x, y, z }
end

-- WHATEVER IS IN THAT HAND SLOT, in model space: position and orientation together.
--
-- The point of it is the OFFSET from the anchor bone beside it. A grenade is put in the hand by the
-- game at some pose relative to `wl`/`wr`, and that pose is exactly what the port has to reproduce for
-- the thing to sit in the fingers instead of floating near them -- the grenade has to sit
-- right by itself. Reading the object rather than a slot name also means it works for anything held.
local function heldPQ(slot)
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local ts = Game.GetTransactionSystem()
    if not ts then return nil end
    local o = nil
    pcall(function() o = ts:GetItemInSlot(pl, TweakDBID.new(slot)) end)
    if not IsDefined(o) then return nil end
    local p, q = nil, nil
    pcall(function() p = o:GetWorldPosition() end)
    pcall(function() q = o:GetWorldOrientation() end)
    if not (p and q) then return nil end
    local bx, by, bz, ci, cj, ck, cr = playerFrame()
    if not bx then return nil end
    local x, y, z = qrot(ci, cj, ck, cr, p.x - bx, p.y - by, p.z - bz)
    local a, b, c, d = qmul(ci, cj, ck, cr, q.i, q.j, q.k, q.r)
    return { x, y, z, a, b, c, d }
end

-- WHICH RECORD IS IN THE HAND. A throw take is worth nothing if it cannot be told which grenade it was,
-- and the type is the one thing that changes what the hand holds.
local function heldName(slot)
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local ts = Game.GetTransactionSystem()
    if not ts then return nil end
    local o = nil
    pcall(function() o = ts:GetItemInSlot(pl, TweakDBID.new(slot)) end)
    if not IsDefined(o) then return nil end
    local nm = nil
    pcall(function() nm = tostring(TDBID.ToStringDEBUG(ItemID.GetTDBID(o:GetItemID()))) end)
    return nm
end

-- the first of several candidate slot names that this weapon actually has
local function slotAny(names)
    for i = 1, #names do
        local v = slotModel(names[i])
        if v then return v end
    end
    return nil
end

-- Register every known rig signature before recording: without identification the plugin publishes no rig bones and
-- every `fb` reads zero, which is exactly what a recording of an unknown weapon looked like.
local function registerRigs()
    if type(VRRigSignature) ~= 'function' then return 0 end
    local ok, list = pcall(function() return require('rigs') end)
    if not (ok and list) then return 0 end
    local n = 0
    for _, r in ipairs(list) do
        local nm = r.names or {}
        local o = pcall(function()
            VRRigSignature(r.which, r.bones,
                           nm[1] and nm[1][1] or -1, nm[1] and nm[1][2] or '',
                           nm[2] and nm[2][1] or -1, nm[2] and nm[2][2] or '',
                           nm[3] and nm[3][1] or -1, nm[3] and nm[3][2] or '',
                           nm[4] and nm[4][1] or -1, nm[4] and nm[4][2] or '')
        end)
        if o then n = n + 1 end
    end
    pcall(function() spdlog.info('[Recorder] registered ' .. n .. ' rig signatures') end)
    return n
end

local function recStart()
    registerRigs()
    REC.on, REC.t, REC.n, REC.samples, REC.sc = true, 0.0, 0, {}, nil
    COMP = {}                       -- component handles belong to the weapon instance, not to the session
    -- live-animated bones with VRIK off: the pose hook publishes the whole animated skeleton while this is on
    pcall(function() VRRecordFK(1) end)
end

-- next free take name, so takes never overwrite each other
local function freeName()
    for i = 1, 99 do
        local name = string.format('reload_record_%02d.lua', i)
        local fh = io.open(name, 'r')
        if fh then fh:close() else return name end
    end
    return 'reload_record_overflow.lua'
end

local function recDump()
    pcall(function() VRRecordFK(0) end)
    local name = freeName()
    local ok, f = pcall(io.open, name, 'w')
    if ok and f then
        f:write('-- VR reload recording v2, MODEL space. l/r = wrist L/R, wl/wr = WeaponLeft/WeaponRight bone,\n')
        f:write('-- each 7 floats (x,y,z, qi,qj,qk,qr). sl/sf/mg = back_slider/front_slider/mag_std slot pos.\n')
        f:write('-- fL/fR = 19 bones x 7 floats, order: InHand Thumb,Index,Middle,Ring,Pinky; Thumb1,2;\n')
        f:write('-- Index1,2,3; Middle1,2,3; Ring1,2,3; Pinky1,2,3.\n')
        f:write('-- mq = magazine mount slot ROTATION, wq = the WEAPON own rotation, 4 floats each, model space:\n')
        f:write('-- with these two, nothing has to be reconstructed from slot positions.\n')
        f:write('-- mb = magazine rig bones 0,1,2,3,4 (mag_plug, magazine, magazine_reload, mag_std, mag_stdr),\n')
        f:write('-- rb = receiver rig bones 0..9 (a break-action top lever), mt = magazine rig float tracks,\n')
        f:write('--      am = the magazine round count.\n')
        f:write('-- fb = frame rig bones 5,6,9,10,11,12,13,14 (front/back slider, bullet_pull, bullet,\n')
        f:write('--      bullet_reload, hammer, rotator, ammo_mover); 7 floats per bone: local pos xyz + quat xyzw.\n')
        f:write('-- hl/hr = the ITEM held in AttachmentSlots.WeaponLeft/Right, 7 floats, model space; hn = its\n')
        f:write('--      record name. Empty on a reload, and the whole point of a grenade take: hr against wr\n')
        f:write('--      is the pose the game holds the thing at, which is what the port has to reproduce.\n')
        f:write('return {\n')
        for i = 1, REC.n do
            local s = REC.samples[i]
            f:write(string.format('{t=%.4f,l={%s},r={%s},wl={%s},wr={%s},sl={%s},sf={%s},mg={%s},mq={%s},wq={%s},wp={%s},mb={%s},fb={%s},rb={%s},mt={%s},am=%s,fL={%s},fR={%s},hl={%s},hr={%s},hn=%q},\n',
                s.t, vecStr(s.l), vecStr(s.r), vecStr(s.wl), vecStr(s.wr),
                vecStr(s.sl), vecStr(s.sf), vecStr(s.mg), vecStr(s.mq), vecStr(s.wq), vecStr(s.wp),
                vecStr(s.mb), vecStr(s.fb), vecStr(s.rb), vecStr(s.mt), tostring(s.am or 'nil'), vecStr(s.fL), vecStr(s.fR),
                vecStr(s.hl), vecStr(s.hr), tostring(s.hn or '')))
        end
        f:write('}\n'); f:close()
        pcall(function() spdlog.info('[Recorder] saved ' .. REC.n .. ' frames to ' .. name) end)
        REC.last = name
    else
        pcall(function() spdlog.info('[Recorder] io.open FAILED for ' .. name .. '; ' .. REC.n .. ' frames lost') end)
        REC.last = 'SAVE FAILED'
    end
end

registerForEvent('onDraw', function()
    if not overlayOpen then return end
    pcall(function()
        ImGui.Begin('VR Reload Recorder')
        if type(VRRecordFK) ~= 'function' then
            ImGui.Text('OLD PLUGIN - VRRecordFK missing, bones will freeze')
        end
        if REC.on then
            ImGui.Text(string.format('RECORDING  %.2fs  %d frames', REC.t, REC.n))
            if ImGui.Button('STOP + save') then REC.on = false; recDump() end
        else
            ImGui.Text('1) VRIK off  2) Record  3) reload  4) Stop')
            if REC.last then ImGui.Text('last: ' .. tostring(REC.last)) end
            if ImGui.Button('Record') then recStart() end
        end
        ImGui.End()
    end)
end)

registerForEvent('onUpdate', function(dt)
    if not REC.on then return end
    if type(VRBoneModelPos) ~= 'function' or type(VRBoneModelRot) ~= 'function' then return end
    REC.t = REC.t + (dt or 0.016)
    REC.n = REC.n + 1
    REC.samples[REC.n] = {
        t  = REC.t,
        l  = bonePQ(23), r  = bonePQ(24),
        wl = bonePQ(26), wr = bonePQ(28),
        -- SLOTS BY CANDIDATE NAME. Weapons do not agree on part names -- the Silverhand has front_slider and
        -- back_slider where the Unity has one `slide`, a `barrel` and a `muzzle_slot` -- and a recording whose slot
        -- fields are empty is useless: the gun's basis is built from them, and without a basis no grip can be
        -- extracted. So each field takes the first name that resolves on THIS weapon.
        -- The RACKING part first: a grip is measured against the thing the hand holds, and that thing moves.
        -- `vrp_barrel` is only a fallback for a weapon with no slide slot -- anchoring a slide grip to it
        -- would leave the hand behind when the slide travels.
        -- THE RACKING SLOT, by name, most specific first. Every entry before `vrp_barrel` names a slot that RIDES
        -- the moving part; `vrp_barrel` does not, and a take anchored on it measures the rack against a fixed point.
        -- That is not hypothetical: the Lexington's whole first set of takes came back that way -- the giveaway is a
        -- slot-to-muzzle distance that never changes while the bone travels 36 mm -- and they had to be rescued by
        -- rebuilding the moving slot from three known ones. `barrel_back` is the Lexington's, `barrel_front` the
        -- Omaha's; no weapon in the set has both, so the order between them does not matter.
        sl = slotAny({ 'vrp_back_slider', 'vrp_slide', 'vrp_barrel_back', 'vrp_barrel_front',
                       'vrp_magazine_open', 'vrp_slider', 'vrp_end_slider',
                       'vrp_cylinder', 'vrp_cocking_handle', 'vrp_foreend', 'vrp_barrel_hinge',
                       'vrp_reload_handle', 'vrp_cocking_device', 'vrp_barrel' }),
        sf = slotAny({ 'vrp_front_slider', 'vrp_muzzle_slot' }),
        mg = slotAny({ 'vrp_mag_slot', 'vrp_mag_std' }),
        -- SAME ORDER AS THE MODULE'S `mag.slot`, and that matters: two slots can share a position and
        -- differ by 180 deg in rotation (they do on the Unity), so recording one and placing by the
        -- other turns the magazine back to front.
        mq = slotRotAny({ 'vrp_mag_slot', 'vrp_mag_std' }),   -- the mount's ROTATION: a magazine is PLACED
        wq = weaponRotModel(),                                -- the weapon's own frame, measured not reconstructed
        wp = weaponPosModel(),                                -- ...and where it is
        -- THE WEAPON'S OWN BONES, straight out of the pose buffer: this is the only honest view of what the game
        -- does to the parts. (The first attempt recorded the mag_std / mag_stdr MESH components instead and the two
        -- came out bit-identical -- a skinned mesh component's GetLocalToWorld is just the weapon entity's
        -- transform, its local position is a plain zero. Useless for this.)
        mb = rigRow(0, MAG_BONES),      -- magazine rig, every bone: a speedloader's six rounds live past index 4
        fb = rigRow(1, FRAME_BONES),    -- frame rig: front/back slider, bullet trio, hammer, rotator, ammo_mover
        rb = rigRow(2, RCV_BONES),      -- receiver rig: a break-action's top lever
        mt = trackRow(0),               -- the magazine rig's visibility tracks, game-driven
        am = ammoNow(),
        fL = fingerRow(FINGERS_L),
        fR = fingerRow(FINGERS_R),
        -- THE THING IN EACH HAND, pose and record name. Empty on an ordinary reload -- nothing is in a
        -- hand slot then -- and the whole point of a throw take.
        hl = heldPQ("AttachmentSlots.WeaponLeft"),
        hr = heldPQ("AttachmentSlots.WeaponRight"),
        hn = heldName("AttachmentSlots.WeaponRight") or heldName("AttachmentSlots.WeaponLeft"),
    }
end)

registerForEvent('onShutdown', function()
    pcall(function() VRRecordFK(0) end)
end)

-- THE SAME TWO BUTTONS, for a harness: GetMod('CyberpunkVRPort_ReloadRecorder').start() / .stop(), so a take can be
-- started and saved from the CET bridge with the overlay closed. `stop` returns the file it wrote.
return {
    start = function() recStart() return true end,
    stop  = function()
        if not REC.on then return REC.last end
        REC.on = false
        recDump()
        return REC.last
    end,
    status = function() return REC.on, REC.n, REC.t end,
}
