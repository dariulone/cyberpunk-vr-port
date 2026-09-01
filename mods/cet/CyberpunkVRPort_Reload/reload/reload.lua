-- CyberpunkVRPort — HAND-DRIVEN PHYSICAL RELOAD.
--
-- The weapon's parts (slide, magazine, hammer) are moved by the weapon's own animation; nothing in script can
-- touch them the ordinary way. But the plugin's pose-apply hook can, and `VRRigWrite(which, boneIndex, x, y, z)`
-- SETS a rig bone's parent-local translation each frame -- resolving the bone index to the shared pose buffer's
-- slot through the LIVE remap table, so it drives the right part (see memory weapon-rig-identification). This
-- module turns the free hand's motion into those writes: grab a part, move, and it follows; let go, it returns.
--
-- Per-weapon data (which bone, how far, grab zones) lives in reload/weapons/<id>.lua, so new weapons are a config
-- file, not code. Bones/travel/senses come from decoding the game's own reload animations, not guesses (memory
-- reload-animation-mechanics).
--
-- Frames: the free hand is read in MODEL space (VRHandRawModel); weapon parts are located from their vrp_ slots in
-- WORLD space and converted to model with the player's transform (same convention as the collision module). The
-- barrel direction is the model-space vector between the front and back slide slots; the free hand's travel
-- PROJECTED onto it is exactly the along-barrel rack distance, because a rotation preserves length -- so the bone's
-- own local frame never has to be reconstructed.
local M = {}

local CONFIGS = {}
do
    -- The list of weapons is DATA (reload/weapons/index.lua), not a literal here: a new pistol is a config file
    -- plus one line in that index, and this module never learns any weapon's name.
    local okI, names = pcall(function() return require('reload/weapons/index') end)
    if not (okI and names) then names = { 'malorian_silverhand' } end
    for _, name in ipairs(names) do
        local ok, cfg = pcall(function() return require('reload/weapons/' .. name) end)
        if ok and cfg then CONFIGS[#CONFIGS + 1] = cfg end
    end
end

-- Grip finger poses, RECORDED from the game's own reload with the recorder (runtime parent-local quats -- no
-- GLB conversion). Each is a list of { boneName, qx, qy, qz, qw }; poses are LEFT-hand named, so the plugin's
-- name match makes them a no-op for a right free hand. Loaded by name from each config's slide.grips[style].pose.
-- THE REVOLVER'S OWN CODE, in its own file. `reload.lua` is built around a slide and a box magazine and by the
-- twelfth pistol it knew that shape very well; a revolver is not it, and piling one into a 3400-line file made both
-- harder to read. Nothing here is a revolver's business until a config asks for it.
local REVOLVER = (function()
    local ok, m = pcall(function() return require('reload/revolver') end)
    return (ok and m) or nil
end)()

local POSES = {}
local function loadPose(name)
    if name and POSES[name] == nil then
        local ok, t = pcall(function() return require('reload/poses/' .. name) end)
        POSES[name] = (ok and t) or false
    end
end
-- No path table here any more. A recorded magazine PATH (reload/paths/malorian_mag.lua) was loaded and replayed for
-- a while, and the file is still on disk, but replaying it moved the magazine relative to the GUN -- which is the
-- one thing a magazine in a hand must not do. The entity route replaced it; loading a table nothing reads was just
-- noise. See the magazine block for the whole story.
for _, cfg in ipairs(CONFIGS) do
    local gs = cfg.slide and cfg.slide.grips
    if gs then
        for _, g in pairs(gs) do loadPose(g.pose); loadPose(g.pose2) end
    end
    -- ...and the poses that hang off a SECTION rather than off a grip. Missing one is a SILENT failure, not an
    -- error: it stays nil in POSES and every guard that reads it simply never fires. This has now caught two
    -- weapons -- the Ticon's thumb on its slide release and the Overture's on its hammer -- so the rule is: a new
    -- place to name a pose is a new line HERE, in the same commit.
    if cfg.slide then loadPose(cfg.slide.pressPose) end
    if cfg.hammer and cfg.hammer.poses then
        for _, k in ipairs({ 'over', 'hold', 'press' }) do loadPose(cfg.hammer.poses[k]) end
    end
    if cfg.hammer then loadPose(cfg.hammer.trigPose) end
    if cfg.spin then loadPose(cfg.spin.pose) end
    if cfg.mag then
        loadPose(cfg.mag.pose)
        loadPose(cfg.mag.poseInsert)
        -- `pose2` as well as `pose`: a stage may describe a second way of carrying the magazine (see vpick), and a
        -- pose that is never LOADED is not a missing file -- it is a nil in POSES, which makes stagePose return
        -- nothing and the hand fall back to the default open pose. That is what "the second pose has the fingers
        -- just open" was, with the file sitting on disk the whole time.
        for _, st in ipairs(cfg.mag.poseStages or {}) do loadPose(st.pose); loadPose(st.pose2) end
    end
end

-- Apply / release a finger grip pose on a hand (0 = left, 1 = right) through the plugin's reload-finger natives.
-- alpha0 is the starting blend: the blend is set BEFORE the pose activates, so the first animation pass already
-- mixes at that alpha instead of flashing the full pose for a frame.
-- LIVE TUNING from the CET overlay.
--
-- Every number in a weapon config is measured, and that is how it should stay -- but the game's hand re-grips the
-- magazine and ours cannot, so somewhere between the animation's several in-hand relations a single one has to be
-- chosen by eye. This makes that choice explicit and adjustable instead of argued about, and prints what was
-- chosen so it can go back into the config as a constant with the rest.
--
-- `curl` is per finger: 0 leaves that finger on tracking, 1 wears the recorded pose, in between blends.
M.tune = {
    seat = -1.0,        -- < 0 = use the weapon's own seatAt; otherwise this fraction of the run (overlay slider)
    hold  = false,                                  -- force a magazine into the hand, no reload needed
    dx = 0.0, dy = 0.0, dz = 0.0,                   -- m, added to holdOffset in the hold slot's own frame
    rx = 0.0, ry = 0.0, rz = 0.0,                   -- deg, turned on top of holdRotBasis
    rev   = 0,                                      -- bumped by the overlay so a slider move re-applies at once
    stage = -1.0,                                   -- >= 0 pins the pose to that depth, for judging one stage
    -- HOW FAR AHEAD A PLACED ENTITY IS PUT, counted in FRAMES of its own last step rather than in milliseconds:
    -- what is being cancelled is a frame of pipeline, not a fixed delay, so it has to follow the frame rate.
    -- A STOPGAP, AND IT IS MEANT TO BE DELETED. Two frames overshot ("оно теперь наоборот вперед меня уходит"),
    -- one fell short, and tuning a number by eye is not a fix -- it is a fudge standing in for holding the thing
    -- properly. The real answer is a mesh component on the player's own entity template, bound to the hand slot,
    -- which the engine carries in the same frame it draws the hand; when that lands, this and `leadWorld` go.
    lead  = 1.0,
}

-- The 19 joints a recorded pose carries, in the order the recorder writes them. Grouped so the overlay can put
-- a finger's joints together and so a whole finger can be set at once.
M.tune.joints = {
    { name = 'thumb',  idx = { 1, 6, 7 },        label = { 'meta', 'base', 'tip' } },
    { name = 'index',  idx = { 2, 8, 9, 10 },    label = { 'meta', 'base', 'mid', 'tip' } },
    { name = 'middle', idx = { 3, 11, 12, 13 },  label = { 'meta', 'base', 'mid', 'tip' } },
    { name = 'ring',   idx = { 4, 14, 15, 16 },  label = { 'meta', 'base', 'mid', 'tip' } },
    { name = 'pinky',  idx = { 5, 17, 18, 19 },  label = { 'meta', 'base', 'mid', 'tip' } },
}
M.tune.w = {}
for i = 1, 19 do M.tune.w[i] = 1.0 end

-- SCALE A JOINT'S ROTATION, and do it on the ANGLE rather than by mixing quaternions towards identity. A mix
-- cannot pass the recorded pose -- at 1 it IS the pose and there is nowhere further to go -- while the angle can
-- simply be multiplied, so 1.4 curls the joint 40 % harder than the animation does and 0 leaves it on tracking.
local function qscale(qi, qj, qk, qr, w)
    if w == nil or math.abs(w - 1.0) < 0.001 then return qi, qj, qk, qr end
    if w <= 0.0 then return 0.0, 0.0, 0.0, 1.0 end
    local n = math.sqrt(qi * qi + qj * qj + qk * qk)
    if n < 1e-6 then return qi, qj, qk, qr end
    -- LuaJIT, which CET runs, has no two-argument math.atan -- that is math.atan2 there
    local at2 = math.atan2 or math.atan
    local a = 2.0 * at2(n, qr)                -- the joint's own angle, signed
    local b = a * w * 0.5
    local sn = math.sin(b)
    return qi / n * sn, qj / n * sn, qk / n * sn, math.cos(b)
end

-- Apply the overlay's per-joint weights to a pose table.
local function curlPose(pose)
    if not pose then return pose end
    local w = M.tune.w
    local same = true
    for i = 1, 19 do if math.abs((w[i] or 1.0) - 1.0) > 0.001 then same = false; break end end
    if same then return pose end
    local out = {}
    for i = 1, #pose do
        local e = pose[i]
        local a, b, c, d = qscale(e[2], e[3], e[4], e[5], w[i])
        out[i] = { e[1], a, b, c, d }
    end
    return out
end

-- A POSE IS NAMED FOR ONE HAND AND THE PLUGIN MATCHES LITERALLY. `VRReloadFingerSet` walks that hand's own bone
-- list and compares names, so a `Left...` joint handed to the RIGHT hand matches nothing and the write is dropped
-- without a word -- the pose reads as applied and the fingers simply stay as tracked. Every file in the set is
-- left-named because the free hand is the left one whenever the gun is in the right, which is the usual case. The
-- limitation was known and written down here, and it bites the moment a pose has to go on the other wrist: the
-- thumb press belongs to the hand HOLDING the gun, and left-handed play hits it for every pose there is.
--
-- One rename where the pose is applied fixes all of it -- the two skeletons' bones differ only in the prefix.
local function forHand(hand, name)
    if hand == 1 then
        if string.sub(name, 1, 4) == 'Left' then return 'Right' .. string.sub(name, 5) end
    elseif string.sub(name, 1, 5) == 'Right' then
        return 'Left' .. string.sub(name, 6)
    end
    return name
end

-- NLERP BETWEEN TWO POSES. The magazine's ladder has done this inline since the Kenshin; the thumb needs it too,
-- and a sequence that SWITCHES poses instead of crossing between them reads as one frame per stage however many
-- stages it has -- which is exactly what "будто по 1 кадру" was.
local function mixPose(pa, pb, t)
    if not pa then return pb end
    if not pb or t <= 0.001 or #pb ~= #pa then return pa end
    if t >= 0.999 then return pb end
    local out = {}
    for i = 1, #pa do
        local x, y = pa[i], pb[i]
        local dp = x[2] * y[2] + x[3] * y[3] + x[4] * y[4] + x[5] * y[5]
        local sg = (dp < 0) and -1.0 or 1.0
        local qi = x[2] + (y[2] * sg - x[2]) * t
        local qj = x[3] + (y[3] * sg - x[3]) * t
        local qk = x[4] + (y[4] * sg - x[4]) * t
        local qr = x[5] + (y[5] * sg - x[5]) * t
        local l = math.sqrt(qi * qi + qj * qj + qk * qk + qr * qr)
        if l < 1e-6 then l = 1.0 end
        out[i] = { x[1], qi / l, qj / l, qk / l, qr / l }
    end
    return out
end

local function applyGrip(hand, pose, alpha0)
    if type(VRReloadFingerSet) ~= 'function' or type(VRReloadFingerApply) ~= 'function' or not pose then return end
    VRReloadFingerClear(hand)
    for i = 1, #pose do
        local p = pose[i]
        VRReloadFingerSet(hand, forHand(hand, p[1]), p[2], p[3], p[4], p[5])
    end
    if type(VRReloadFingerBlend) == 'function' then VRReloadFingerBlend(hand, alpha0 or 1.0) end
    VRReloadFingerApply(hand, 1)
end
local function clearGrip(hand)
    if type(VRReloadFingerApply) == 'function' then VRReloadFingerApply(hand, 0) end
end

-- FINGER POSE FADE. One hand at a time wears the grip pose, and its blend ramps over the grip's blendTime, so
-- the fingers GLIDE between tracking and the recorded curl (the preview the user asked for) -- never a teleport,
-- in either direction. The plugin nlerps the pose onto the live tracked fingers by this alpha every pass.
local fadeS = { hand = nil, key = nil, blend = 0.0, dir = 0 }
local function fadeIn(hand, g)
    if not g or not g.pose then return end
    if fadeS.hand ~= hand or fadeS.key ~= g.pose then
        if fadeS.hand and fadeS.hand ~= hand then clearGrip(fadeS.hand) end
        local pose = POSES[g.pose]
        if not pose then return end
        if fadeS.hand ~= hand then fadeS.blend = 0.0 end
        applyGrip(hand, curlPose(pose), fadeS.blend)
        fadeS.hand, fadeS.key = hand, g.pose
    end
    fadeS.dir = 1
end
-- The same fade, but handed a pose TABLE and a key of the caller's choosing. Stages are interpolated per frame,
-- so there is no file to name them by -- the key carries which two stages and how far between them, and that is
-- what tells the fade when the hand's target has really changed.
local function fadeInTable(hand, key, pose)
    if not pose then return end
    if fadeS.hand ~= hand or fadeS.key ~= key then
        if fadeS.hand and fadeS.hand ~= hand then clearGrip(fadeS.hand) end
        if fadeS.hand ~= hand then fadeS.blend = 0.0 end
        applyGrip(hand, curlPose(pose), fadeS.blend)
        fadeS.hand, fadeS.key = hand, key
    end
    fadeS.dir = 1
end
local function fadeOut()
    if fadeS.hand then fadeS.dir = -1 end
end
local function fadeReset()
    if fadeS.hand then clearGrip(fadeS.hand) end
    fadeS.hand, fadeS.key, fadeS.blend, fadeS.dir = nil, nil, 0.0, 0
end
local function fadeTick(dt, T)
    if not fadeS.hand or fadeS.dir == 0 then return end
    fadeS.blend = fadeS.blend + fadeS.dir * (dt / (T or 0.15))
    if fadeS.blend >= 1.0 then
        fadeS.blend, fadeS.dir = 1.0, 0
    elseif fadeS.blend <= 0.0 then
        fadeReset()
        return
    end
    if type(VRReloadFingerBlend) == 'function' then pcall(function() VRReloadFingerBlend(fadeS.hand, fadeS.blend) end) end
end

-- THE PULL-IN, ON A SPRING -- the same shape the two-hand hold uses for the support hand, and asked for
-- in those words. A grip that lands writes the wrist onto the recorded grip point; without a weight
-- that is a teleport, so the weight is a damped spring released toward 1 at the grab and the drawn
-- wrist is drawn from where the hand really is toward the grip by it.
--
-- Integrated in fixed 2 ms substeps for the reason every spring in this port is: a frame is too coarse
-- for it, and integrating once per frame makes the settle depend on the frame rate.
local PULL_MS   = 180.0     -- how long the hand takes to arrive
local PULL_ZETA = 0.75      -- a shade under 1: it settles rather than clicks into place
local pullS = { key = nil, x = 0.0, v = 0.0 }
local function pullInReset()
    pullS.key, pullS.x, pullS.v = nil, 0.0, 0.0
end
local function pullIn(key, dt)
    if pullS.key ~= key then pullS.key, pullS.x, pullS.v = key, 0.0, 0.0 end
    local wn = 6.283185307 / (PULL_MS / 1000.0)
    local d = dt or 0.016
    if d < 0.0 then d = 0.0 elseif d > 0.1 then d = 0.1 end
    local steps = math.floor(d / 0.002) + 1
    if steps > 64 then steps = 64 end
    local sdt = d / steps
    for _ = 1, steps do
        local a = -wn * wn * (pullS.x - 1.0) - 2.0 * PULL_ZETA * wn * pullS.v
        pullS.v = pullS.v + a * sdt
        pullS.x = pullS.x + pullS.v * sdt
    end
    -- clamped at 1: a pull-in that overshoots would push the hand THROUGH the grip it is arriving at
    if pullS.x < 0.0 then pullS.x = 0.0 elseif pullS.x > 1.0 then pullS.x = 1.0 end
    return pullS.x
end

-- CET's sandbox has no `_G`, but RED4ext natives are reachable as plain globals by name (referencing an
-- undefined one yields nil, not an error), so check them directly.
local function havesNatives()
    return type(VRRigWrite) == 'function'
       and type(VRRigWriteRot) == 'function'
       and type(VRRigWriteClear) == 'function'
       and type(GetVRSharedSlot) == 'function'
       and type(VRHandRawModel) == 'function'
end

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

-- Quaternion of the gun's rigid basis (columns F = barrel forward, D = mag-well down, S = F x D) -- the same
-- construction the recording analysis used, so a recorded grip rotation composes back exactly: qGun * gripRot.
local function basisQuat(fx, fy, fz, dx, dy, dz, sx, sy, sz)
    local tr = fx + dy + sz
    if tr > 0 then
        local s = math.sqrt(tr + 1.0) * 2.0
        return (dz - sy) / s, (sx - fz) / s, (fy - dx) / s, 0.25 * s
    elseif fx > dy and fx > sz then
        local s = math.sqrt(1.0 + fx - dy - sz) * 2.0
        return 0.25 * s, (dx + fy) / s, (sx + fz) / s, (dz - sy) / s
    elseif dy > sz then
        local s = math.sqrt(1.0 + dy - fx - sz) * 2.0
        return (dx + fy) / s, 0.25 * s, (sy + dz) / s, (sx - fz) / s
    else
        local s = math.sqrt(1.0 + sz - fx - dy) * 2.0
        return (sx + fz) / s, (sy + dz) / s, 0.25 * s, (fy - dx) / s
    end
end

local function playerFrame()
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local ok, base = pcall(function() return pl:GetWorldPosition() end)
    if not ok or not base then return nil end
    local ok2, q = pcall(function() return pl:GetWorldOrientation() end)
    if not ok2 or not q then return nil end
    return base.x, base.y, base.z, -q.i, -q.j, -q.k, q.r    -- conjugate: world -> model
end

local function slotModel(slotComp, name, bx, by, bz, ci, cj, ck, cr)
    local o, found, tr = pcall(function() return slotComp:GetSlotTransform(CName.new(name)) end)
    if not (o and found and tr) then return nil end
    local v = WorldPosition.ToVector4(tr.Position)
    return qrot(ci, cj, ck, cr, v.x - bx, v.y - by, v.z - bz)
end

-- WHERE THE FINGER PADS ARE, WITHOUT ASKING THE FINGERS. A cylinder is ROLLED with the pads, and the tracked hand
-- point every other gesture in this module uses sits a palm's depth behind them -- so the spin zone had to be drawn
-- far wider than the part it touches, and engaged off the heel of the hand.
--
-- BUT THE PADS THEMSELVES CANNOT BE READ FROM THE POSED HAND, and that is a feedback loop with a one-frame delay --
-- the same shape as the magazine magnet's, learnt there the hard way. Reading bones 65/66 works, and then: the zone
-- catches, the preview pose EXTENDS the fingers, the pads move several centimetres, the zone loses them, the pose
-- fades, the fingers curl back, the zone catches again. That is the judder as the hand comes near, and why it settles
-- once the hand is properly on the cylinder -- deep inside the zone the loop cannot reach the edge.
--
-- So the point is anchored to the WRIST bone (23 left, 24 right), which nothing here writes to, by a constant offset
-- measured in the wrist's own frame: over 264 frames of the Overture take the left pads sit at
-- (+119.4, -11.2, -42.1) mm from the wrist and over 409 frames of the shooting take the right ones at
-- (-116.6, +5.2, +25.7) -- mirrored, as two hands should be, and 120 to 127 mm out. It does not follow how far the
-- fingers are actually extended (106 to 147 mm of reach between an open hand and a fist), and that is the trade taken
-- on purpose: a stable point a few centimetres out beats an exact one that oscillates.
local PAD_WRIST = { [0] = 23, [1] = 24 }
local PAD_OFF   = { [0] = {  0.1194, -0.0112, -0.0421 },
                    [1] = { -0.1166,  0.0052,  0.0257 } }
local function padModel(side)
    local bi = PAD_WRIST[side]
    if not bi or type(VRBoneModelPos) ~= 'function' or type(VRBoneModelRot) ~= 'function' then return nil end
    local p = VRBoneModelPos(bi)
    local q = VRBoneModelRot(bi)
    if not (p and q and (p.w or 0) > 0.5) then return nil end
    local o = PAD_OFF[side]
    local dx, dy, dz = qrot(q.i, q.j, q.k, q.r, o[1], o[2], o[3])
    return p.x + dx, p.y + dy, p.z + dz
end

local function handModel(side)
    local h = VRHandRawModel(side)
    if not h then return nil end
    return h.x, h.y, h.z
end

local function len3(x, y, z) return math.sqrt(x * x + y * y + z * z) end

-- ------------------------------------------------------------- A DROPPED MAGAZINE
--
-- It is a spawned mesh entity with no collision of its own (the game will not simulate one for us -- see the
-- magazine block), so the fall is integrated here and the contacts come from a raycast against the real world.
-- What this replaces: a vertical fall from a standstill onto a floor ASSUMED to be at the player's own feet, ending
-- with the velocity zeroed and the orientation frozen at whatever angle the hand had.
--
-- The body is a box about 30 x 84 x 141 mm, read off the magazines' own bounding boxes and consistent across them
-- (Kenshin 30.1 x 84.0 x 141.4, Liberty/Unity 26.0 x 83.1 x 121.8, Tamayura 28.6 x 75.3 x 130.8), with LOCAL X the
-- thin axis and LOCAL Z the long one. Two numbers follow from that, and both matter on screen:
--   * `centre` -- the body's middle in its own axes, because a thrown object tumbles about its centre of mass, not
--     about the mesh origin, which sits on the face that goes into the well. The three bbox centres are
--     (1.4, -21.5, -63.5), (0, -22.0, -55.6) and (0, -20.0, -59.5) mm, so one number covers all of them; a magazine
--     whose origin is somewhere else already declares that difference as `mag.originOffset`, and it is subtracted.
--   * `flat` -- half the thin dimension: how high the centre sits once the magazine is lying on its side, which is
--     the pose it is put into when it stops.
--   * `long` and `thin` -- WHICH of the mesh's own axes those are. Every magazine in the set up to the Kang Tao Chao
--     is built the same way, long along local Z and thin across local X, so these were hardcoded; the Chao's is
--     built along local Y instead (its body measures 31.8 x 177.0 x 31.6 mm), and with the wrong axis the three
--     probes all sit inside the body, the ends go through the floor, and it lies down on its end.
local DROP = {
    radius   = 0.035,                  -- kept for the rest test's skin; the contact test uses the probes below
    centre   = { 0.0, -0.021, -0.060 },
    flat     = 0.015,
    long     = { 0, 0, 1 },
    thin     = { 1, 0, 0 },
    probe    = 0.028,                  -- three of these, at the centre and `arm` either way along the long axis
    arm      = 0.050,
    inertia  = 0.0022,                 -- (0.084^2 + 0.141^2)/12 for this body, per unit mass
    bounce   = 0.22,                   -- restitution. Every physics material in this game is authored at 0.2 or
                                       -- 0.05 (see the basketball measurements), and a polymer magazine on concrete
                                       -- is no livelier than that
    friction = 0.55,                   -- Coulomb, against the normal impulse
    spinAir  = 0.50,                   -- 1/s, in flight
    drag     = 0.10,                   -- 1/s, in flight
    restE    = 0.35,                   -- a contact closing slower than this does not bounce at all
    spinDamp = 0.10,                   -- shed per contact
    touchRest = 0.25,                  -- s of accumulated ground contact after which it simply lies down
    restV    = 0.45,                   -- a contact leaving less than this is the end of it
    settle   = 0.18,                   -- s to lie down
    maxV     = 8.0,                    -- what a hand is allowed to impart
    maxW     = 20.0,
}

-- The shortest way between two orientations. A component lerp is close enough for a single tumble step but not for
-- the settle, which can turn the magazine through 90 degrees.
local function qslerp(a, b, t)
    local d = a[1] * b[1] + a[2] * b[2] + a[3] * b[3] + a[4] * b[4]
    local s = (d < 0.0) and -1.0 or 1.0
    d = d * s
    local w1, w2
    if d > 0.9995 then
        w1, w2 = 1.0 - t, t * s
    else
        local th = math.acos(d)
        local sn = math.sin(th)
        w1, w2 = math.sin((1.0 - t) * th) / sn, s * math.sin(t * th) / sn
    end
    local q = { a[1] * w1 + b[1] * w2, a[2] * w1 + b[2] * w2, a[3] * w1 + b[3] * w2, a[4] * w1 + b[4] * w2 }
    local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3] + q[4] * q[4])
    if l < 1e-9 then return { a[1], a[2], a[3], a[4] } end
    return { q[1] / l, q[2] / l, q[3] / l, q[4] / l }
end

-- WHAT IT LANDS ON, asked of the world itself. `SyncRaycastByQueryPreset` is the one raycast form measured to answer
-- from script at all -- the ByCollisionGroup / ByCollisionPreset variants returned false against "Static" and
-- "World Static" during the basketball survey -- and its TraceResult carries position, normal and the physics
-- material's name. Which preset answers is remembered after the first hit, so the usual cost is one ray per frame
-- while a magazine is in the air, and the first few contacts are logged so the preset and the surface can be read
-- back from the log instead of guessed at.
local RAY_PRESETS = { 'Sight Blocker', 'Bullet', 'Camera' }
local RAY_PICK, RAY_SYS, RAY_LOG = 1, nil, 0
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
            -- staticOnly = TRUE, and this is the whole difference between a magazine that falls and one that hangs
            -- in the air. MEASURED live from the magazine well: a plain ray 6 cm down hits `metal.physmat` 6 mm below
            -- the slot -- the WEAPON's own cooked collider, which this port put there for the hands -- so a magazine
            -- leaving the well "landed" on the gun it was leaving, at chest height, and lay down on it. With
            -- staticOnly the same short ray misses and a 3 m ray reaches `concrete.physmat` at the real floor.
            -- The player's body, their hands and every other actor go with it, which is what we want.
            -- The cost, stated plainly: static world geometry only, so a magazine will fall THROUGH a car bonnet or a
            -- crate that is simulated. For litter that lives three seconds that is the right way round.
            local r1, r2 = RAY_SYS:SyncRaycastByQueryPreset(a, b, RAY_PRESETS[i], true)
            return r1, r2
        end)
        if ok and hit and tr then
            RAY_PICK = i
            if RAY_LOG < 4 then
                RAY_LOG = RAY_LOG + 1
                pcall(function()
                    spdlog.info(string.format('[MagDrop] hit via %s at z=%.3f n=(%.2f,%.2f,%.2f) mat=%s',
                        RAY_PRESETS[i], tr.position.z, tr.normal.x, tr.normal.y, tr.normal.z, tostring(tr.material)))
                end)
            end
            return { tr.position.x, tr.position.y, tr.position.z }, { tr.normal.x, tr.normal.y, tr.normal.z }
        end
    end
    return nil
end

-- SOUND. Every event name comes out of the weapon's OWN animation files: pwa_/pma_w_handgun__malorian_silverhand.anims
-- carry 52 `animAnimEvent_Sound` entries, and among them are exactly the ones this reload needs -- mag_out, mag_in,
-- mag_new_grab, handle_pull, handle_release. So the physical reload sounds like the game's own, because it IS the
-- game's own audio. Played on the weapon's entity, so it comes from the gun and not from the player's centre.
local function playSnd(weapon, name)
    if not (weapon and name) then return end
    pcall(function()
        Game.GetAudioSystem():Play(CName.new(name), weapon:GetEntityID(), CName.new(''))
    end)
end

-- ONE OF THE WEAPON'S OWN EFFECTS, by the name its .app gives it. This is how the game makes a flying case: not a
-- bone, an EFFECT. Every pistol's appearance carries an `entEffectSpawnerComponent` whose descriptors name it --
-- `ejection_port` -> base\fx\weapons\firearms\pistols\<gun>\w_pistol_<gun>_ejection_port.effect, and inside that
-- sits base\fx\weapons\bullet_shells\cal_9mm.particle with cal_9mm.mesh, a real cartridge case as a mesh particle.
-- So a weapon with no round in its rig (the Unity has none: 11 frame bones, not one of them a bullet) still ejects
-- a case, and it is the game's case, thrown by the game's own emitter out of the game's own port.
local function playFx(weapon, name)
    if not (weapon and name) then return end
    pcall(function()
        GameObjectEffectHelper.StartEffectEvent(weapon, CName.new(name))
    end)
end

-- TAKE THE SLIDE OFF THE GAME, or give it back. Not a guess -- read out of base\gameplaynim_graphs\weapon.animgraph:
-- the pose that holds the slide back on a dry magazine is an OVERRIDE layer, and its weight is
--
--     A & (!B)      A = (WeaponData.ammoRemaining == 0) & (WeaponStats.magazineCapacity > 0)
--                   B = TagValue('reload')
--
-- The `reload` tag belongs to the state machine's `Reload` state, and that state is entered from ANY state by the
-- external event `Reload` (globalTransitions[0]); `InterruptReload` leaves it. So pushing `Reload` makes the game
-- stop applying the empty-slide pose at all, which is better than out-shouting it every frame.
--
-- Measured, so the limit is on the record: the event alone does NOT close the slide on screen -- inside the Reload
-- state the weapon's own reload clip holds it back instead (slide slot to muzzle slot stayed 0.1470 m, where closed
-- is 0.105). It removes the FIGHT over the bone; the position is still ours to write.
local function pushAnimEvent(weapon, name)
    if not (weapon and name) then return end
    pcall(function() AnimationControllerComponent.PushEvent(weapon, CName.new(name)) end)
end

-- HOLD the stage instead of the bone. Two parts, and the second one is why the event alone was not enough:
--   * the external event `Reload` puts the graph in its Reload state, whose `reload` tag switches the empty-slide
--     override off (see pushAnimEvent);
--   * INSIDE that state the clip is chosen by `ReloadData.emptyReload` -- and an EMPTY reload's clip holds the slide
--     back by itself, which is why pushing the event alone left the slide visibly back (slot distance stayed
--     0.1470 m against 0.105 closed). Saying "this reload is not an empty one" asks for the clip whose slide is home.
-- Sent every frame the claim stands, deliberately: re-sending on a timer made the graph alternate between two poses
-- and the whole weapon jumped 2-3 cm several times a second ("дуло прыгает вверх вниз").
-- `ReloadData` is the same channel the game's own weapon state machine uses (weaponTransitions.script:
-- `SetAnimationParameterFeature('ReloadData', m_animReloadData)`), so nothing here is off the beaten path.
local gReloadFeat = nil
local function holdReloadStage(weapon)
    if not weapon then return end
    if gReloadFeat == nil then
        local ok, f = pcall(function() return AnimFeature_WeaponReload.new() end)
        gReloadFeat = (ok and f) or false
        if gReloadFeat then
            pcall(function()
                gReloadFeat.emptyReload = false
                gReloadFeat.amountToReload = 1
                gReloadFeat.continueLoop = false
            end)
        end
    end
    if gReloadFeat then
        pcall(function() AnimationControllerComponent.ApplyFeature(weapon, CName.new('ReloadData'), gReloadFeat) end)
    end
    pushAnimEvent(weapon, 'Reload')
end

-- Rounds left in the magazine. `GetMagazineAmmoCount` is the one that answers (measured live: 10/10 with the
-- Malorian in hand); every SETTER candidate failed from CET, so the module can WATCH the ammo but not spend it --
-- an ejected round is not deducted from the magazine yet (that needs a redscript hook).
local function magAmmo(weapon)
    local ok, n = pcall(function() return weapon:GetMagazineAmmoCount() end)
    if ok and type(n) == 'number' then return n end
    return nil
end

-- SPEND a round: the same event the game itself uses to take ammo out of a magazine (quick-melee does exactly
-- this in weaponTransitions.script -- `consumeEvent = new WeaponConsumeMagazineAmmoEvent; amount = N;
-- weapon.QueueEvent(consumeEvent)`). Direct setters are all rejected from CET, the event is not: a round thrown
-- out by hand is really gone from the magazine, so racking a full gun repeatedly empties it.
local function consumeAmmo(weapon, n)
    local ok = pcall(function()
        local evt = WeaponConsumeMagazineAmmoEvent.new()
        evt.amount = n or 1
        weapon:QueueEvent(evt)
    end)
    return ok
end

-- AMMO, through the game's OWN API -- no writing of counts behind its back.
--   * `SetAmmoCountEvent` sets the magazine outright; the game itself uses it for the Ceaseless Lead perk's refund
--     (ceaselessLeadAmmoEffector), queued on the WEAPON with the weapon's ammo type.
--   * `WeaponObject.GetMagazineCapacity` is the magazine's size (10 for this pistol, measured live).
--   * the RESERVE is an ordinary inventory item -- `Ammo.HandgunAmmo` here -- so it reads with GetItemQuantity over
--     `ItemID.CreateQuery` and is spent with RemoveItem, exactly as RPGManager.GetAmmoCountValue does.
local BUILD = '2231-ammo2way'      -- bumped on every deploy; printed in the log line so the running build is never
                                   -- a guess again (the game hot-reloads mods, so file times prove nothing)

-- THE MAGAZINE COUNT, each direction through the mechanism that actually moves it. Measured, not assumed:
-- `SetAmmoCountEvent` with count = 0 queues fine and changes NOTHING -- the log showed `ammoEv=set0` every frame while
-- the gun kept reading 7 -- and the game itself only ever uses that event to ADD rounds (the Ceaseless Lead refund and
-- the MagazineAutoRefill on equip), never to take them away. Taking away is what
-- `WeaponConsumeMagazineAmmoEvent` is for, and this module already proves it works: it is how a round thrown out by
-- hand really leaves the magazine.
--
-- So: DOWN by consuming the difference, UP by the set event. Reports why it failed instead of swallowing it -- a class
-- a sub-module cannot see looks exactly like "the game overwrote our value", and that cost a round of guessing.
local ammoErr = '-'
local function setMagAmmo(weapon, n)
    if not weapon then ammoErr = 'no weapon'; return false end
    local cur = magAmmo(weapon)
    if cur == nil then ammoErr = 'no count'; return false end
    if n < 0 then n = 0 end
    if n == cur then ammoErr = 'at' .. tostring(n); return true end
    if n < cur then
        local ok = consumeAmmo(weapon, cur - n)
        ammoErr = ok and string.format('consume%d(%d->%d)', cur - n, cur, n) or 'consume FAILED'
        return ok
    end
    if type(SetAmmoCountEvent) == 'nil' then ammoErr = 'no SetAmmoCountEvent class'; return false end
    if type(WeaponObject) == 'nil' then ammoErr = 'no WeaponObject class'; return false end
    local ok, err = pcall(function()
        local e = SetAmmoCountEvent.new()
        e.ammoTypeID = WeaponObject.GetAmmoType(weapon)
        e.count = n
        weapon:QueueEvent(e)
    end)
    ammoErr = ok and string.format('set%d(%d->%d)', n, cur, n) or ('FAIL ' .. tostring(err))
    return ok
end

-- THE GAME'S OWN ANSWER to "is this gun dry" -- which is what puts the slide on its stop. Preferred over the
-- round count because it is the same question the weapon's own state machine asks, and it is true from the moment
-- the last round leaves rather than a frame later.
local function w0IsEmpty(weapon)
    if not weapon then return false end
    local ok, v = pcall(function() return weapon:IsMagazineEmpty() end)
    if ok and type(v) == 'boolean' then return v end
    local n = magAmmo(weapon)
    return (n or 1) <= 0
end

local function magCapacity(weapon)
    local ok, n = pcall(function() return WeaponObject.GetMagazineCapacity(weapon) end)
    return (ok and n) or 0
end

local function reserveAmmo(weapon)
    local ok, n = pcall(function()
        local id = WeaponObject.GetAmmoType(weapon)
        return Game.GetTransactionSystem():GetItemQuantity(Game.GetPlayer(),
                                                           ItemID.CreateQuery(ItemID.GetTDBID(id)))
    end)
    return (ok and n) or 0
end

local function takeReserve(weapon, n)
    if not (weapon and n and n > 0) then return end
    pcall(function()
        Game.GetTransactionSystem():RemoveItem(Game.GetPlayer(), WeaponObject.GetAmmoType(weapon), n)
    end)
end

-- weapon -> config. Cache only POSITIVE matches: a negative was cached permanently before, so a weapon whose
-- components were not yet loaded on the first frame stayed unmatched forever. Unmatched weapons re-enumerate each
-- frame (cheap and rare); a matched one caches after its first good frame and never enumerates again.
local matchCache = {}
local function cfgFor(weapon)
    local ok, id = pcall(function() return weapon:GetEntityID().hash end)
    local key = ok and tostring(id) or 'x'
    if matchCache[key] then return matchCache[key] end
    local ok2, cs = pcall(function() return weapon:GetComponents() end)
    if ok2 and cs then
        for i = 1, #cs do
            local nm = tostring(cs[i]:GetName()):lower()
            for _, cfg in ipairs(CONFIGS) do
                if string.find(nm, cfg.match, 1, true) then matchCache[key] = cfg; return cfg end
            end
        end
    end
    return nil
end

-- per-interactable runtime state
local slideS = { grabbed = false, style = '-', dist = -1, rack = 0, wasActive = false }
local rotS   = { t = -1 }    -- rotator spin timer; -1 = not spinning
local hatchS = { p = 0.0 }   -- a magazine cover's opening, 0 shut .. 1 open (see the HATCH block)
-- A REVOLVER'S HAMMER. Cocked by the stick and held there until the shot takes it, which is what a single-action
-- hammer does. Its own state because nothing else on the gun behaves like it: it is not sprung, it does not follow
-- the magazine, and it must hand the bone straight back the moment the weapon fires or the game's own recoil throw
-- would be fought every shot.
local hamS = { p = 0.0, want = 0.0, prev = false, wrote = false }
-- A CYLINDER TURNED BY HAND. Free only while the crane is out, which is the real thing's rule too.
local spinS = { ang = 0.0, ref = nil, hand = nil, wrote = false }
-- WHAT THE GAME IS ALLOWED TO SEE ON THE TRIGGER: 0 pass / 1 swallow / 2 press fully, shared[161], applied by the
-- XInput merge. Sent only on a change, and sent as 0 on every teardown -- a latched 1 is a gun that never fires
-- again, and it would outlive the weapon that asked for it.
local trgS = { mode = nil }
local function setTrg(v)
    if trgS.mode == v or type(SetVRTriggerMode) ~= 'function' then return end
    pcall(function() SetVRTriggerMode(v) end)
    trgS.mode = v
end

local ownS  = { tagged = false }   -- whether the weapon's own animation is currently being held off (see `ownAnim`)
-- THE THUMB PRESS: a short pose laid on the hand that HOLDS the gun, independent of `fadeS`. Two hands can wear
-- poses at once -- the finger natives take the hand as an argument, so the single-slot fade above is a convenience
-- and not a limit -- and this one has to run on the other wrist from everything else.
local pressS = { t = -1, blend = 0.0, hand = nil, on = false }
local exitAt = '-'           -- where M.frame last returned, for the diagnostic line

-- THE MAGAZINE, as a little state machine of its own:
--   in    seated in the well, under the game's own control -- nothing of ours is written and nothing of ours is in
--         the world; the port's magazines exist only for the duration of an interaction
--   hand  the spawned entity is in the free hand, held at a fixed pose in the hold slot
--   gone  no magazine in the gun and none in a hand; gripping at the magnet pose brings a fresh one (test flow)
-- A FALLING magazine is not a state: it is a second entity ticked independently, so a magazine can be thrown away and
-- a fresh one gripped in the same breath. At most two exist at once -- one falling, one held.
local magS = { state = 'in', hand = nil, off = { 0, 0, 0 }, t = 0, dist = -1, btnWas = false }

-- THE FRAME OF TRAVEL A WORLD-PLACED ENTITY IS OTHERWISE BEHIND BY.
--
-- Everything this module computes is MODEL space, and that space is self-consistent by construction: the base and
-- every slot come out of the same game state in the same call, so the player's own travel cancels out of all of
-- it. One thing leaves that space -- a spawned entity. It is placed by SetWorldTransform against a transform the
-- game state has, while the hand beside it is drawn from the pose of the frame being rendered. The two are a
-- frame apart, and a frame apart IS speed: nothing standing still, a hand's width at a run, closing again the
-- moment the player stops. The weapon's own parts never had this -- VRRigWrite puts them in the pose that the
-- same frame draws -- so only entities need correcting.
--
-- WHAT IS EXTRAPOLATED IS THE TARGET ITSELF, per placed thing, and that is the whole of the second attempt. The
-- first led by the PLAYER ROOT's smoothed velocity, and it was wrong twice over on the ground that showed it:
--
--   * the root is not the anchor. Over a step or a kerb the character controller lifts the capsule while the
--     camera and the skeleton are eased up after it, so the hand and the root are briefly going different ways
--     and no amount of root velocity describes where the hand will be.
--   * a filter has its own lag. Smoothed over three frames, the estimate is right only while the speed is
--     steady -- which is exactly the flat ground where the problem was already small, and never on the bumps,
--     where the velocity changes faster than the filter follows. "всё равно на неровностях отстаёт".
--
-- So the estimate is the last observed STEP of the very point being placed -- root motion, step easing, arm
-- swing and all, whatever caused it -- and the next step is predicted as that one. No dt appears anywhere: a
-- step is already a per-frame quantity, so a frame that takes longer carries itself and a variable frame time
-- cannot distort the answer. The second-order term (predicting the CHANGE in the step) is deliberately not
-- taken: it buys a little on a ramp and doubles the noise, and the noise here is a difference of two anchors,
-- which is the one quantity that shows up as the magazine shaking against a hand that is not.
local leadS = {}

local function leadWorld(key, x, y, z)
    local s = leadS[key]
    if not s then
        leadS[key] = { px = x, py = y, pz = z, dx = 0.0, dy = 0.0, dz = 0.0 }
        return x, y, z                          -- first frame: no step seen yet, so nothing is claimed
    end
    local rx, ry, rz = x - s.px, y - s.py, z - s.pz
    if (rx * rx + ry * ry + rz * rz) > 4.0 then
        -- two metres in one frame is a load, a fast travel or this entity being re-grabbed somewhere else --
        -- never a step. Deliberately generous: a car does 30 m/s and every frame of that is a real step the
        -- magazine has to take with it.
        s.dx, s.dy, s.dz = 0.0, 0.0, 0.0
    else
        -- lightly smoothed, and only lightly: two thirds of a step is taken at once so a bump is followed, and
        -- the third that is held back is what stops a single bad frame from being handed straight through.
        local a = 0.65
        s.dx = s.dx + (rx - s.dx) * a
        s.dy = s.dy + (ry - s.dy) * a
        s.dz = s.dz + (rz - s.dz) * a
    end
    s.px, s.py, s.pz = x, y, z
    local step = math.sqrt(s.dx * s.dx + s.dy * s.dy + s.dz * s.dz)
    local k = M.tune.lead or 0.0
    if k <= 0.0 then return x, y, z, step end
    return x + s.dx * k, y + s.dy * k, z + s.dz * k, step
end

-- HOW STALE THIS SCRIPT'S OWN READING IS, asked rather than guessed.
--
-- `VRPlayerEnginePos` is the player's world position taken off the ENGINE's state object at the body-yaw store
-- site, and `Game.GetPlayer():GetWorldPosition()` is the same quantity read here. Same number, two points in the
-- frame -- so the difference between them carries no systematic part at all and is time and nothing else. Add it
-- to a world placement and the entity sits where the hand is drawn instead of where the script last saw it.
--
-- It has to be THIS pair. The view-composed routes are the obvious candidate and they cannot do it: the port's
-- own note beside `wModel` in init.lua has VRViewWorldPos about a metre below the real camera, which put the gun
-- 0.397 m from the hand holding it -- a round trip through it is exact because both directions share the error,
-- and a one-way use of it is not. Anything built on that view, VRPalmWorldPos included, would hand a placement
-- that offset as a constant and call it freshness.
--
-- Returns zero when the native is absent or the store site has not identified the player yet, which is the right
-- answer for both: no claim, and the caller is exactly as good as it was before.
local function freshDelta(bx, by, bz)
    if type(VRPlayerEnginePos) ~= 'function' then return 0.0, 0.0, 0.0, -1.0 end
    local ok, p = pcall(function() return VRPlayerEnginePos() end)
    if not (ok and p) or (p.w or 0.0) < 0.5 then return 0.0, 0.0, 0.0, -1.0 end
    local dx, dy, dz = p.x - bx, p.y - by, p.z - bz
    -- a metre apart is not two reads of one frame: a load, a fast travel, or the store site holding a player
    -- that is no longer this one. Claim nothing rather than teleport a magazine.
    local d2 = dx * dx + dy * dy + dz * dz
    if d2 > 1.0 then return 0.0, 0.0, 0.0, -2.0 end
    return dx, dy, dz, math.sqrt(d2)
end

-- ------------------------------------------------------------- THE CARRIER
--
-- A mesh component that lives in the PLAYER'S OWN entity template, bound to `ItemAttachmentSlots` / `WeaponLeft`
-- and `WeaponRight` through an `entHardTransformBinding` (added by tools/ent/add_hold_components.py). The engine
-- carries it in the same pass that draws the hand, so it cannot be behind the hand -- not by a filter, not by a
-- prediction, by construction.
--
-- THIS REPLACES A SPAWNED ENTITY, AND THE REASON IS MEASURED. A world entity placed by SetWorldTransform reaches
-- the screen a frame after the hand it is placed against: at a run that frame is 21 cm, which is the magazine
-- floating clear of the palm. It is not the script's reading that is late -- the plugin's engine-side player
-- position and this script's own agree to under half a millimetre at 8 m/s -- so no amount of leading fixes it.
-- The script is simply not in that race. A component in the template is not in it either.
--
-- Verified live, 2026-08-16, with the carriers in place:
--     Toggle(true)                          the object appears in the hand
--     worldTransform vs the slot            separation 0.0000 m, both hands, read in ONE call
--     SetLocalTransform(pos, rot)           applies from the next frame; asked 0.0800, measured 0.0801
--     ChangeResource(ResRef, true)          swaps what is held; the ball became a Malorian magazine
--     component.mesh = '<path>'             DOES NOT take -- no error, and the old mesh keeps drawing
--
-- The last two lines are why the swap goes through ChangeResource and not through the obvious assignment.
--
-- ONE CARRIER IS ONE MESH, and that is a limit of this pair of components rather than of the idea: the Overture's
-- "magazine" is a speedloader of twelve meshes (six shells, six tips), which one component cannot draw. The
-- engine does it with more parts, and so can we -- the patcher writes a carrier in four lines and nothing stops
-- it writing six or twelve of them onto the same slot. Until that is needed, the revolver keeps the entity route
-- it already has in reload/revolver.lua, and the twelve box magazines move over.
local holdS = { comp = {}, mesh = {}, skin = {}, on = {} }

local function holdComp(side)
    local c = holdS.comp[side]
    if c then return c end
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local want = (side == 0) and 'vrp_hold_left' or 'vrp_hold_right'
    local ok, cs = pcall(function() return pl:GetComponents() end)
    if not (ok and cs) then return nil end
    for i = 1, #cs do
        if tostring(cs[i]:GetName()):find(want, 1, true) then
            holdS.comp[side] = cs[i]
            -- THE PLANE THE WEAPON IS DRAWN IN, and everything in the hands with it. In first person the gun and
            -- the arms are not in the scene plane -- they are drawn in `RPl_Weapon`, which is what keeps them out
            -- of the world's depth and lighting. A carrier left on `RPl_Scene` is composited against them rather
            -- than with them, and reads as a magazine you can see the gun through.
            --
            -- Set here as well as in the template: it costs one assignment on the frame the component is first
            -- found, and it means the Lua does not depend on the archive being the exact build that carries it.
            pcall(function() cs[i].renderingPlane = 'RPl_Weapon' end)
            return cs[i]
        end
    end
    return nil
end

-- ------------------------------------------------------------- A MANY-PART THING: A REAL ITEM
--
-- One carrier draws one mesh, and some props are not one mesh: the Overture's speedloader is seven
-- (six rounds and a handle) and lives in its own asset rather than on the weapon. What the game does
-- with a many-part thing in a hand is carry it as an ITEM, and it carries it exactly -- measured with
-- the loader attached, its world position and the WeaponLeft slot agreed to 0.0000 m.
--
-- FOUR THINGS HAD TO BE TRUE, and every one of them failed silently on the way (2026-08-16):
--   * the attach has to be made from REDSCRIPT. From CET `AddItemToSlot` returns true and attaches
--     nothing, at any argument count, while `RemoveItemFromSlot` from CET works -- so it is that one
--     function, not the boundary. Hence `VRPortHoldItem` in CyberpunkVRPort_Hold.
--   * the item's entity must be rooted in `gameItemObject`, not `entEntity`.
--   * that entity must carry a non-empty `appearances` list naming an .app.
--   * and the ArchiveXL factory csv must be a COOKED C2dArray, not text. Ours was text; ArchiveXL
--     logged "Loading factory" and got no rows, and the name resolved to nothing.
-- `true` from AddItemToSlot means the transaction was accepted and NOTHING about the slot, which is
-- why every check here reads the slot back instead.
local itemS = { name = {}, comps = {} }

local function itemHold(side, itemName, slotName)
    local pl = Game.GetPlayer()
    if not (pl and itemName and type(pl.VRPortHoldItem) == 'function') then return false end
    if itemS.name[side] == itemName then return true end
    local ok, res = pcall(function() return pl:VRPortHoldItem(itemName, slotName) end)
    if not (ok and res) then return false end
    itemS.name[side] = itemName
    itemS.comps[side] = nil                 -- the entity is not resolvable on the frame it is asked for
    return true
end

-- ITS PARTS, cached: the placement below writes their local transforms and there are seven of them.
-- Collected on the first frame the entity resolves, which is not the frame it was asked for.
local function itemParts(side, slotName)
    local got = itemS.comps[side]
    if got then return got end
    local pl = Game.GetPlayer()
    local ts = Game.GetTransactionSystem()
    if not (pl and ts) then return nil end
    local ok, e = pcall(function() return ts:GetItemInSlot(pl, TweakDBID.new(slotName)) end)
    if not (ok and e) then return nil end
    local okc, cs = pcall(function() return e:GetComponents() end)
    if not (okc and cs) then return nil end
    local list = {}
    for i = 1, #cs do
        local c = cs[i]
        if tostring(c:GetClassName()):find('Mesh') and type(c.SetLocalTransform) == 'function' then
            local oke, en = pcall(function() return c.isEnabled end)
            if not (oke and en == false) then                        -- the switched-off twin stays off
                -- The plane is NOT set here, and the attempt is worth recording: assigning
                -- `renderingPlane` on an attached item's components does nothing -- the prop stayed
                -- transparent -- because the game applies the plane when it TAKES the item, from the
                -- sixth argument of AddItemToSlot. It is passed as RPl_Weapon in the redscript bridge
                -- (CyberpunkVRPort_Hold), which is the only place that decides it. The carrier
                -- components on the player are a different matter: nothing else owns those, so the
                -- assignment there is the one that works.
                list[#list + 1] = c
            end
        end
    end
    if #list == 0 then return nil end
    itemS.comps[side] = list
    return list
end

-- THE MEASURED HOLD, written to every part the same. The entity is where the game put it -- on the
-- slot, exactly -- and the grip this module measured is an offset from that slot, so it goes on the
-- components' local transforms.
--
-- THE SAME ONE TO ALL SEVEN, and that is not an approximation: each piece's place inside the prop is
-- baked into its own MESH, not into its component transform. The Overture's config says so about the
-- other end of the same rig -- "every shell's mesh already carries its own chamber (its
-- boneRigMatrices[0].W is exactly minus its bullet bone's position)". Composing the grip onto the
-- components' own local transforms therefore applies that placement TWICE, and it showed: the pose
-- was right with the plain write and wrong with the composition. Measured by looking, in the headset,
-- which for a grip is the only instrument there is.
local function itemPlace(side, slotName, lx, ly, lz, qi, qj, qk, qr)
    local parts = itemParts(side, slotName)
    if not parts then return false end
    for i = 1, #parts do
        pcall(function()
            parts[i]:SetLocalTransform(Vector4.new(lx, ly, lz, 1.0), Quaternion.new(qi, qj, qk, qr))
        end)
    end
    return true
end

local function itemDrop(side, slotName)
    if itemS.name[side] == nil then return end
    local pl = Game.GetPlayer()
    if pl and type(pl.VRPortDropItem) == 'function' then
        pcall(function() pl:VRPortDropItem(slotName) end)
    end
    itemS.name[side], itemS.comps[side] = nil, nil
end

-- WHAT IS BEING HELD. The resource change is not free, so it is done on a change of mesh and not per frame.
local function holdShow(side, meshPath, appearance)
    local c = holdComp(side)
    if not (c and meshPath) then return false end
    if holdS.mesh[side] ~= meshPath then
        local ok = pcall(function() c:ChangeResource(ResRef.FromString(meshPath), true) end)
        if not ok then return false end
        holdS.mesh[side] = meshPath
        holdS.skin[side] = nil
    end
    if appearance and holdS.skin[side] ~= appearance then
        pcall(function() c:ChangeAppearance(CName.new(appearance), true) end)
        holdS.skin[side] = appearance
    end
    if not holdS.on[side] then
        local ok2 = pcall(function() c:Toggle(true) end)
        if not ok2 then return false end
        holdS.on[side] = true
    end
    return true
end

-- HOW IT SITS IN THE HAND, in the SLOT's own frame -- which is what the module measured in the first place, so
-- nothing has to be composed through the player to place it and nothing can go stale on the way.
local function holdPlace(side, lx, ly, lz, qi, qj, qk, qr)
    local c = holdComp(side)
    if not c then return false end
    return pcall(function()
        c:SetLocalTransform(Vector4.new(lx, ly, lz, 1.0), Quaternion.new(qi, qj, qk, qr))
    end)
end

local function holdHide(side)
    for s = 0, 1 do
        if side == nil or side == s then
            if holdS.on[s] then
                local c = holdComp(s)
                if c then pcall(function() c:Toggle(false) end) end
                holdS.on[s] = false
            end
        end
    end
end

-- A NEW OBJECT IS NOT THE OLD ONE MOVED. Spawning, seating and holstering all put the next thing under a key
-- metres from where the last one was, and a step measured across that gap is not a step at all -- it is one
-- frame of the magazine thrown at whatever the difference happened to be. The 2 m reject above is the backstop
-- for a teleport; this is the ordinary case, and it is cheaper to say so than to make the backstop tight enough
-- to catch it, which would also catch a car.
local function leadReset(key)
    leadS[key] = nil
end

-- Entity ids whose despawn could not be carried out yet (the entity had not finished spawning). Retried every frame
-- until they resolve; without this a magazine can be left in the world with nothing holding its id.
local magLitter = {}
local function magLitterSweep()
    if #magLitter == 0 or type(exEntitySpawner) == 'nil' then return end
    for i = #magLitter, 1, -1 do
        local ok, e = pcall(function() return Game.FindEntityByID(magLitter[i]) end)
        if ok and e then
            pcall(function() exEntitySpawner.Despawn(e) end)
            table.remove(magLitter, i)
        end
    end
end

-- Hand the magazine back to the game, completely: the spawned entity goes away and the gun's own `showMagazine`
-- track is released so the seated magazine reappears under the animation's control. Rig 0 is the magazine rig and
-- its two tracks are `showMagazine` / `showMagazineReload`, in that order (from the .rig's trackNames).
--
-- This runs whenever the weapon changes or is lost. Without it, holstering mid-grab left a magazine entity floating
-- in the world AND the gun's own magazine invisible, with the state machine still convinced it was in the hand.
local function magHome()
    for _, key in ipairs({ 'entId', 'dropId' }) do           -- the held one and anything still falling
        local id = magS[key]
        if id then
            local ok, e = pcall(function() return Game.FindEntityByID(id) end)
            if ok and e then pcall(function() exEntitySpawner.Despawn(e) end)
            else magLitter[#magLitter + 1] = id end          -- not resolvable yet: the sweep will get it
            magS[key] = nil
        end
    end
    holdHide()                              -- and the carrier lets go with everything else...
    itemDrop(0, 'AttachmentSlots.WeaponLeft'); itemDrop(1, 'AttachmentSlots.WeaponRight')  -- ...and the item
    leadReset('hand'); leadReset('glide')   -- whatever was being led is gone; the next one starts its own step
    magS.dropP, magS.dropV, magS.dropQ, magS.dropT = nil, nil, nil, nil
    magS.dropC, magS.dropW, magS.dropRest, magS.dropHits, magS.hold = nil, nil, nil, nil, nil
    magS.dropTouch = nil
    -- a cover belongs to the weapon that has one: holster mid-reload and the next gun must not inherit an opening
    hatchS.p, hatchS.wrote = 0.0, false
    -- and a magazine caught mid-glide is simply gone with the entity that was doing it
    if magS.state == 'glide' then magS.state = 'gone' end
    magS.glide = nil
    if type(VRRigTrackWrite) == 'function' then
        pcall(function() VRRigTrackWrite(0, 0, 1.0, 0); VRRigTrackWrite(0, 1, 0.0, 0) end)
    end
    -- and let go of the wrist, both hands. A magnet left active on a weapon switch pins the hand at a pose whose
    -- gun no longer exists -- the same failure the slide's rotation lock had to be taught to release.
    if type(VRHandStopModel) == 'function' and (magS.magnet or 0) > 0 then
        pcall(function()
            for h = 0, 1 do
                VRHandStopModel(h, false, Vector4.new(0, 0, 0, 0))
                if type(VRHandStopRot) == 'function' then VRHandStopRot(h, 0, 0, 0, 0, 1) end
            end
        end)
    end
    magS.magnet = 0
    magS.state, magS.hand, magS.t, magS.armed = 'in', nil, 0, false
    magS.slotComp = nil            -- the cached slot component belongs to the player object that is going away
    -- ...and so do the carriers. A handle kept across a load or a respawn points at a player that no longer
    -- exists, and the next Toggle would be written into it; the same rule the slot component above already has.
    holdS.comp[0], holdS.comp[1] = nil, nil
    holdS.mesh[0], holdS.mesh[1] = nil, nil
    holdS.skin[0], holdS.skin[1] = nil, nil
end

-- The magazine rig's float tracks as the GAME leaves them, for the log: count, then showMagazine / showMagazineReload.
-- EVERY track, not the first two. Two was enough while a magazine rig had three of them and the only question was
-- whether `showMagazine` had been found; a revolver's speedloader has NINE -- six `bulletUsed`, then the two reload
-- flags and `showMagazineBullets` -- and "the rounds stay in the cylinder" can only be answered by seeing which
-- index actually moves. Printed as a row, so the one that is ours stands out against the ones the game drives.
-- Reports the flick's PEAK lateral hand speed and clears it, so each log line covers its own interval rather than
-- the whole session -- the shape `magJitDbg` uses for the magnet's ranges, for the same reason.
local function latPeak()
    local v = hatchS.lat or -1
    hatchS.lat = nil
    return v
end

local function magTrackDbg()
    if type(VRRigTrack) ~= 'function' then return 'n/a' end
    local ok, s = pcall(function()
        local n = VRRigTrack(0, -1)
        local out = {}
        for i = 0, math.min(n, 12) - 1 do out[#out + 1] = string.format('%.2f', VRRigTrack(0, i)) end
        return string.format('n%d [%s]', n, table.concat(out, ' '))
    end)
    return ok and s or 'err'
end

-- The magnet's numbers over the last log interval: frames seen, frames with the magnet on, and the RANGES of the
-- wrist-to-pose distance, the weight, the target and the raw hand. Ranges rather than instants, because a shake IS a
-- range: this says whether the target moves, the hand moves, or neither -- the last meaning someone else writes it.
local function magJitDbg()
    local j = magS.jit
    magS.jit = nil
    if not (j and j.n and j.n > 0) then return '' end
    local function rng(k)
        local lo, hi = j[k .. 'lo'], j[k .. 'hi']
        if lo == nil then return '-' end
        return string.format('%.3f..%.3f', lo, hi)
    end
    return string.format(' magnet[n=%d on=%d arm=%s raw->seat=%.3f seen->seat=%.3f d=%s w=%s tgt=%s raw=%s]',
        j.n, j.on or 0, magS.armed and 'Y' or 'n', magS.insD or -1, magS.seenD or -1,
        rng('d'), rng('w'), rng('t'), rng('r'))
        .. string.format(' depth=%.3f perp=%.3f', magS.insD or -1, magS.perp or -1)
end

-- diagnostics for the log line; while grabbed it adds the plugin's write telemetry, so a dead slide names its
-- own cause from one log read: applied frozen = the write loop is not running (rig asleep / not identified),
-- applied counting but the part still = wrong slot or poisoned base (base shows in mm).
M.dbg = function()
    local extra = ''
    if slideS.grabbed and type(VRRigWriteDiag) == 'function' then
        local ok, s = pcall(function()
            local lods = {}
            for l = 0, 3 do
                lods[#lods + 1] = string.format('a4=%d:%d/%d/%d/sl%d/b%d',
                    l, VRRigWriteDiag(0, 20 + l), VRRigWriteDiag(0, 24 + l), VRRigWriteDiag(0, 28 + l),
                    VRRigWriteDiag(0, 32 + l), VRRigWriteDiag(0, 36 + l) % 100000)
            end
            local map = {}
            for e = 0, 3 do
                map[#map + 1] = string.format('%d>%d', VRRigWriteDiag(0, 40 + e * 2), VRRigWriteDiag(0, 41 + e * 2))
            end
            return string.format(' wr[slot=%d pin=%d applied=%d base=(%d,%d,%d)mm | seen/rn/dst/slots/buf %s | map0 %s]',
                VRRigWriteDiag(0, 10), VRRigWriteDiag(0, 11), VRRigWriteDiag(0, 3),
                VRRigWriteDiag(0, 7), VRRigWriteDiag(0, 8), VRRigWriteDiag(0, 9),
                table.concat(lods, ' '), table.concat(map, ' '))
        end)
        if ok then extra = s end
    end
    return string.format('[%s] exit=%s slide[%s%s] style=%s dist=%.3f rack=%.3f rotor=%s hatch=%.2f ch=%s ammo=%s mag[%s d=%.3f]%s',
        BUILD, exitAt, slideS.grabbed and 'GRAB' or 'idle',
        (slideS.locked and '+LOCK' or (slideS.userClosed and '+HELD' or '')) .. (slideS.tagged and '+TAG' or ''), slideS.style, slideS.dist, slideS.rack,
        (rotS.t >= 0) and 'SPIN' or '-',
        hatchS.p or 0.0,
        slideS.chambered and 'Y' or (slideS.ejectT and 'FLING' or (slideS.chaseV and 'CHASE' or 'n')),
        tostring(slideS.ammo) .. ((slideS.spent == false) and '(consume FAILED)' or '')
            .. (magS.carried and ('/carried' .. tostring(magS.carried)) or '')
            .. (magS.loaded and ('/loaded' .. tostring(magS.loaded)) or '') .. ' ammoEv=' .. ammoErr,
        string.format('%s/%s h=%s ent=%s%s pose=%s fade=%s/%.2f trk=%s %s', magS.state, tostring(magS.route or '-'), tostring(magS.hand),
            magS.entId and 'yes' or 'no',
            -- a falling magazine reports its age, its speed and its contacts, so "it landed wrong" can be read
            -- off one log line instead of guessed at
            magS.dropId and string.format('+fall%.1fs v=%.2f hits=%d%s', magS.dropT or 0,
                magS.dropV and len3(magS.dropV[1], magS.dropV[2], magS.dropV[3]) or -1,
                magS.dropHits or 0, magS.dropRest and ' REST' or '') or '',
            tostring(magS.poseOK), tostring(fadeS.key), fadeS.blend or -1,
            -- The GAME's own float-track values, read back before our write lands. This is the proof line for the
            -- visibility mechanism: the magazine rig must report n2, and with a magazine seated the two tracks read
            -- 1.00 / 0.00 -- exactly the rig's referenceTracks. Anything else means the buffer is not the tracks.
            magTrackDbg(),
            (magS.diag or '') .. (magS.dbg2 or '')),
        magS.dist, (extra or '')
            .. string.format(' rev[ham p=%.2f want=%.0f ammo=%s last=%s | crane=%.2f open=%s | spin=%.0f]',
                hamS.p or 0, hamS.want or 0, tostring(slideS.ammo), tostring(hamS.last),
                hatchS.p or 0, tostring(hatchS.open), spinS.ang or 0)
            .. string.format(' craneBone=%s latMax=%.2f spin=%.0f sp[%s]', tostring(hatchS.read),
                latPeak(), spinS.ang or 0, tostring(spinS.dbg))
            .. ((REVOLVER and (' ' .. REVOLVER.diag() .. ' trg=' .. tostring(hamS.dbg))) or '')
            .. ' slideWhy[' .. tostring(slideS.why) .. ']') .. magJitDbg()
end


-- WHICH HANDS THE STEERING WHEEL OWNS. The wheel grab publishes them in shared[163] (bit0 = right,
-- bit1 = left), raised on PROXIMITY so there is no press edge to race. A grip that is holding the wheel
-- is not a grip: driving one-handed with a gun out puts the LEFT hand -- this module's magazine hand --
-- on the wheel, which is exactly the case the shoot-while-driving mode exists for.
--
-- NOTE THE TWO CONVENTIONS: h == 0 is the LEFT hand in this module, and bit0 is the RIGHT hand there.
local function wheelOwns(h)
    if type(GetVRSharedSlot) ~= 'function' then return false end
    local m = math.floor(GetVRSharedSlot(163) or 0.0)
    if m <= 0 then return false end
    local bit = (h == 0) and 2 or 1
    return (m % (bit * 2)) >= bit
end

function M.frame(weapon, slotComp, holder, dt)
    if not havesNatives() then exitAt = 'natives'; return end
    dt = dt or 0.016
    magLitterSweep()
    -- WHICH HAND THIS MODULE OWNS this frame -- the wrist it is holding through VRHandStop*, so the collision solve
    -- can leave that hand alone. Two writers on one wrist is the shake: the collision publishes a push-out from the
    -- gun every frame and the reload then writes its own hold over it, and the plugin sees the pin dropped and
    -- re-taken each frame. The project's own rule from the collision work says it plainly: one owner per hand.
    M.ownedHand = nil
    -- THE PULL-IN BELONGS TO ONE GRAB. Cleared while nothing is grabbed, so the next grip starts
    -- from the hand instead of from wherever the last one left the weight.
    if not slideS.grabbed then pullInReset() end
    if (not weapon) or (not slotComp) or (holder ~= 0 and holder ~= 1) then
        setTrg(0)                                -- the trigger is the game's again the moment the gun is not ours
        -- ...and a hand-turned cylinder does not outlive the grip: angle, momentum and detent all go
        spinS.ang, spinS.pp, spinS.hand, spinS.w, spinS.det, spinS.on = 0.0, nil, nil, 0.0, nil, false
        if spinS.faded then fadeOut(); spinS.faded = nil end
        if magS.state ~= 'in' or magS.entId then magHome() end   -- weapon put away: no floating magazine, none hidden
        if slideS.grabbed or fadeS.hand then
            pcall(VRRigWriteClear); slideS.grabbed = false; slideS.hand = nil; fadeReset()
            if pressS.on then clearGrip(pressS.hand) end
            pressS.t, pressS.on, pressS.hand = -1, false, nil
            -- the wrist rotation lock is plugin-side state: it MUST be released here or a weapon lost mid-grab
            -- leaves the hand pointing at the recorded grip forever
            if type(VRHandStopRot) == 'function' then
                pcall(function() VRHandStopRot(0, 0, 0, 0, 0, 1); VRHandStopRot(1, 0, 0, 0, 0, 1) end)
            end
        end
        exitAt = 'noholder'; return
    end
    local cfg = cfgFor(weapon)
    if not cfg then exitAt = 'nocfg'; return end
    local snd = cfg.sounds

    -- A weapon switch/redraw invalidates the identified rig buffers and every latched write base. Reset both, or
    -- the write loop can sit latched on the PREVIOUS instance's buffer and the slide ignores the hand ("fresh
    -- draw: slide does nothing; holster and draw again: works"). The LOD fallback in the hook covers the other
    -- half of that report (a pass whose remap dropped the bone).
    local okId, wid = pcall(function() return weapon:GetEntityID().hash end)
    local wkey = okId and tostring(wid) or '?'
    if wkey ~= slideS.weaponKey then
        slideS.weaponKey = wkey
        slideS.chambered = true          -- a freshly drawn gun is treated as chambered
        slideS.ejectT, slideS.chaseV = nil, nil
        slideS.locked, slideS.lockBase, slideS.userClosed, slideS.catchOff = false, nil, false, false
        slideS.tagged = false
        pcall(VRRigWriteClear)
        if type(VRRigReset) == 'function' then pcall(VRRigReset) end
        -- TELL THE PLUGIN HOW TO RECOGNISE THIS WEAPON'S RIGS. Identification used to be three hard-coded `if`s in
        -- the hook -- bone counts 5 and 16 with the Silverhand's own bone-name hashes -- so no second weapon could
        -- ever be seen. Now each config carries its own signature (bone count plus named bones at their indices) and
        -- registers it here; the plugin hashes the names itself (FNV1a64, which IS the CName hash, verified against
        -- all five of the old constants). Re-registering the same pair overwrites, so a redraw costs nothing.
        -- Every KNOWN signature (reload/rigs.lua) plus whatever this weapon's own config declares. The shared list
        -- exists because a rig has to be identified before it can be recorded, and a config is written from that
        -- recording -- so it cannot be the only source. Nothing about any weapon lives in the plugin.
        local sigs = {}
        do
            local ok, list = pcall(function() return require('reload/rigs') end)
            if ok and list then for _, r in ipairs(list) do sigs[#sigs + 1] = r end end
        end
        if cfg.rigs then for _, r in ipairs(cfg.rigs) do sigs[#sigs + 1] = r end end
        if type(VRRigSignature) == 'function' then
            for _, r in ipairs(sigs) do
                local n = r.names or {}
                local ok2, slot = pcall(function()
                    return VRRigSignature(r.which, r.bones,
                                   n[1] and n[1][1] or -1, n[1] and n[1][2] or '',
                                   n[2] and n[2][1] or -1, n[2] and n[2][2] or '',
                                   n[3] and n[3][1] or -1, n[3] and n[3][2] or '',
                                   n[4] and n[4][1] or -1, n[4] and n[4][2] or '')
                end)
                -- A FULL SIGNATURE TABLE MUST SAY SO. The native answers -1 when it has no slot left, and a rig that
                -- was never registered is a rig the pose hook cannot identify -- which reads in game as a weapon
                -- whose slide simply does nothing, with nothing in any log to explain it. This project has been
                -- caught by silent table saturation four times; one line is cheap.
                if ok2 and slot == -1 then
                    pcall(function()
                        spdlog.info(string.format('[Reload] RIG SIGNATURE REJECTED (table full): which=%d bones=%d',
                                                  r.which or -1, r.bones or -1))
                    end)
                end
            end
        end
        magHome()                        -- the new weapon's magazine is its own, and ours must not stay hidden
    end

    -- THE HOLDER, DEBOUNCED. It is detected fresh every frame and it FLICKERS: measured in the log, 6 frames of
    -- holder=0 inside 20 while the gun never left the right hand. Every flicker moves `free` to the other wrist --
    -- and that wrist is the one the magazine's grip is read from and the one the pose and the magnet are written
    -- to. So a single bad frame both drops the magazine (the other hand is not squeezing) and pins the GUN hand's
    -- wrist, which is exactly the pair of symptoms seen. A hand does not change in 20 ms; make it prove it.
    if holder ~= M.holderSeen then
        M.holderSeen, M.holderFor = holder, 0.0
    else
        M.holderFor = (M.holderFor or 0.0) + dt
    end
    if M.holderOk == nil or (M.holderFor >= 0.25 and holder ~= M.holderOk) then M.holderOk = holder end
    holder = M.holderOk

    local free = (holder == 1) and 0 or 1     -- the free hand is the one not holding the gun
    local bx, by, bz, ci, cj, ck, cr = playerFrame()
    if not bx then exitAt = 'noframe'; return end

    -- barrel direction (model space, unit, toward the muzzle)
    local fx, fy, fz = slotModel(slotComp, cfg.barrel.front, bx, by, bz, ci, cj, ck, cr)
    local kx, ky, kz = slotModel(slotComp, cfg.barrel.back,  bx, by, bz, ci, cj, ck, cr)
    if not (fx and kx) then exitAt = 'nobarrel'; return end
    local ax, ay, az = fx - kx, fy - ky, fz - kz
    local al = len3(ax, ay, az)
    if al < 1e-4 then return end
    ax, ay, az = ax / al, ay / al, az / al

    local hx, hy, hz = handModel(free)
    if not hx then exitAt = 'nohand'; return end
    exitAt = 'ok'
    -- A GRIP HOLDING THE STEERING WHEEL IS NOT A GRIP (see wheelOwns above). Reported as "not
    -- pressed" here rather than gated at each of the six call sites, because that is what it means:
    -- the button is spoken for.
    local function gripOf(h)
        if wheelOwns(h) then return false end
        return (GetVRSharedSlot(h == 0 and 155 or 49) or 0.0) > 0.5
    end

    -- ----------------------------------------------------------------- SLIDE
    -- A WEAPON NEED NOT HAVE ONE. The Kang Tao Chao has no slide, no bolt and no charging handle: nothing on its
    -- frame rig translates in any clip at all, and its whole reload is the magazine plus a cover that swings. An
    -- empty table keeps every `sc.` read below nil-safe, and the two places that would actually write a bone are
    -- gated -- the rest/release block on `cfg.slide` right below, and everything else on `spx`, which cannot resolve
    -- without a slot to resolve.
    local sc = cfg.slide or {}
    local la = sc.localAxis or { 0, 0, -1 }
    -- Parts that ride the rack. Each rider carries a bone-local offset vector per metre of rack; a rider with
    -- `lag` trails the slide exponentially -- in the game's anim the round only catches up ~0.1 s after the
    -- slide is fully open, and that delay is most of what makes the extraction read as real. Rider state lives
    -- in slideS.riderV so the trailing keeps converging through the release spring and the rotator window
    -- (rig writes clear only after both finish, by which time an 0.08 s lag has settled).
    slideS.riderV = slideS.riderV or {}
    -- WHERE THE SLIDE IS RIGHT NOW, as the game left it, before anything of ours lands on it.
    local function slideRead()
        if type(VRRigBone) ~= 'function' then return nil end
        local ok, x = pcall(function() return VRRigBone(sc.which, sc.bone, 0) end)
        local o2, y = pcall(function() return VRRigBone(sc.which, sc.bone, 1) end)
        local o3, z = pcall(function() return VRRigBone(sc.which, sc.bone, 2) end)
        if not (ok and o2 and o3 and x and y and z) then return nil end
        return x, y, z
    end

    -- IS THE GAME HOLDING THE SLIDE BACK? On an empty magazine it does, and that changes what "rest" means.
    -- Judged against the two positions the bone actually takes, both measured off an empty reload: closed
    -- (0, 21.6, 85.9) mm and held back (0, 21.6, 45.9) mm, 40.0 mm apart along the bone's local -Z.
    local function slideLockedNow()
        local lk = sc.lockLocal
        if not lk then return false end
        local x, y, z = slideRead()
        if not x then return false end
        local rl = sc.restLocal or { 0, 0, 0 }
        local dl = len3(x - lk[1], y - lk[2], z - lk[3])
        local dr = len3(x - rl[1], y - rl[2], z - rl[3])
        return dl < dr
    end

    local function driveSlide(r)
        local d = r * (sc.sign or 1.0)
        -- ABSOLUTE when the weapon says where its slide rests. The relative write is base + offset, and the base
        -- is latched the first time the bone is seen -- so a gun first seen CLOSED has 0 meaning "closed", and on
        -- an empty magazine, where the game holds the slide BACK, writing 0 snapped it shut the instant a grip was
        -- taken. An absolute write does not care what the animation is doing or when we first looked.
        local rl = sc.restLocal
        if rl and type(VRRigWriteAbs) == 'function' then
            local base = (slideS.lockBase or rl)
            pcall(function()
                VRRigWriteAbs(sc.which, sc.bone,
                    base[1] + la[1] * d, base[2] + la[2] * d, base[3] + la[3] * d, 0, 0, 0, 1, 1)
            end)
        else
            VRRigWrite(sc.which, sc.bone, la[1] * d, la[2] * d, la[3] * d)
        end
        if sc.riders then
            -- A DELAY LINE, because that is what the game's own animation does. Measured on the Kenshin's rig
            -- animation, the trailing part passes 25 %, 50 % and 75 % of its travel a CONSTANT 0.034 s after the
            -- driven one going out and a constant 0.067 s coming home. A constant shift at every level is a pure
            -- time delay; a first-order follow (`lag` below, which the Silverhand uses) would compress the early part
            -- and stretch the late one, so it cannot reproduce this however it is tuned. The two directions differ,
            -- so they are separate numbers.
            slideS.histT = (slideS.histT or 0.0) + dt
            local hist = slideS.hist
            if not hist then hist = {}; slideS.hist = hist end
            hist[#hist + 1] = { slideS.histT, d }
            while #hist > 1 and hist[1][1] < slideS.histT - 0.30 do table.remove(hist, 1) end
            -- WHICH WAY IT IS GOING IS REMEMBERED, not re-derived every frame. The two delays differ by a factor of
            -- two, so a frame where `d` does not change must keep the direction it had: a part held at full travel,
            -- and above all the DRAIN (which writes a constant zero while the history empties), both look like "not
            -- moving inward" -- and reading that as outward halves the delay mid-flight and jumps the trailing part
            -- 30-odd millimetres forward instead of letting it run home.
            local dd = d - (slideS.histD or 0.0)
            if dd > 1e-6 then slideS.histDir = 1 elseif dd < -1e-6 then slideS.histDir = -1 end
            local outward = (slideS.histDir or 1) >= 0
            slideS.histD = d
            local function delayed(del)
                if not del or del <= 0 or #hist < 2 then return d end
                local want = slideS.histT - del
                if hist[1][1] >= want then return hist[1][2] end
                for k = #hist, 2, -1 do
                    if hist[k - 1][1] <= want and want <= hist[k][1] then
                        local span = hist[k][1] - hist[k - 1][1]
                        local t2 = (span > 1e-6) and (want - hist[k - 1][1]) / span or 0.0
                        return hist[k - 1][2] + (hist[k][2] - hist[k - 1][2]) * t2
                    end
                end
                return d
            end
            for i = 1, #sc.riders do
                local rd = sc.riders[i]
                local v = d
                if rd.delayOut or rd.delayHome or rd.delay then
                    v = delayed(outward and (rd.delayOut or rd.delay) or (rd.delayHome or rd.delay))
                elseif rd.lag and rd.lag > 0 then
                    local prev = slideS.riderV[i] or 0.0
                    v = prev + (d - prev) * (1.0 - math.exp(-dt / rd.lag))
                end
                slideS.riderV[i] = v
                local vec = rd.vec
                if vec then
                    VRRigWrite(sc.which, rd.bone, vec[1] * v, vec[2] * v, vec[3] * v)
                else
                    local rr2 = v * (rd.ratio or 1.0)
                    -- A RIDER CAN HAVE ITS OWN STOP. Measured on the Militech Lexington: its top piece follows the
                    -- slide 1:1 (23.2 mm against 23.1) until about 29.5 mm and then stays put while the slide carries
                    -- on to 52.6 -- so this is a dragged part with its own limit, not a part on a fixed ratio, and a
                    -- ratio fitted to the deep end (0.56) would have it lagging visibly through the whole first inch.
                    if rd.cap and rr2 > rd.cap then rr2 = rd.cap end
                    VRRigWrite(sc.which, rd.bone, la[1] * rr2, la[2] * rr2, la[3] * rr2)
                end
            end
        end
        -- the CHAMBERED round (the live, bullet-tipped one the player sees) rides the extractor 1:1 while it is
        -- there; an empty chamber has nothing to ride
        -- The chambered round is NOT dragged along by the rack: it SITS in the chamber, so cracking the slide open
        -- lets the player look at it -- pull a little further and it flies. (Riding the slide 1:1 was the earlier
        -- cut; it made the round leave with the very first millimetre.)
    end
    -- visibility of the chambered round (the no-tracks scale switch); also remembered, so the ammo watch below
    -- only writes the scale on a real change instead of every frame
    local function showSeated(s)
        local show = (s > 0.5)
        slideS.seatedShown = show
        -- Showing RELEASES the scale write (0 = off) rather than forcing 1.0, so a visible round is the game's own
        -- scale and nothing of ours lingers on it; hiding forces it to nothing.
        local v = show and 0.0 or 0.001
        local cr = sc.chamberRound
        if cr and type(VRRigWriteScale) == 'function' then
            for i = 1, #cr.bones do
                local bn = cr.bones[i]
                pcall(function() VRRigWriteScale(sc.which, bn, v) end)
            end
        end
    end
    -- HAND THE SLIDE BACK TO THE ANIMATION, bone and riders both. Not "write the rest position" -- that is still an
    -- override, and a hardcoded one: it pinned the slide to a measured number, so the recoil kick stopped showing at
    -- all. A write slot in the plugin applies on every pose pass for ever once registered, and an offset of zero is
    -- not a release either (it means base + 0). `VRRigWriteOff` makes the slot dormant, so the very next pass leaves
    -- the game's own pose standing -- recoil, slide stop, everything the animation does.
    local function releaseSlide()
        if type(VRRigWriteOff) ~= 'function' then return false end
        pcall(function() VRRigWriteOff(sc.which, sc.bone) end)
        if sc.riders then
            for i = 1, #sc.riders do
                local rd = sc.riders[i]
                if rd.bone then pcall(function() VRRigWriteOff(sc.which, rd.bone) end) end
                slideS.riderV[i] = 0.0
            end
        end
        slideS.rack, slideS.lockBase = 0.0, nil
        return true
    end

    -- THE SLIDE STOP.
    --
    -- The game puts the slide on its stop when the magazine runs dry and takes it off again by itself the moment a
    -- fresh one goes in. That second half is the game's convenience, not a gun's behaviour, and it takes the
    -- release away from the player -- so the stop is held here until he racks it off himself.
    --
    -- WHICH STAGE THIS IS comes from the weapon, not from geometry: `IsMagazineEmpty()`. Measured live with the
    -- slide on its stop, it answered true while the bone read z = 0.0459 -- exactly the position measured off an
    -- empty reload. Position alone cannot tell the stop from recoil, which puts the bone in the same place on
    -- every shot; the weapon's own state can, and needs no dwell and no threshold to do it.
    --
    -- Nothing holds the slide CLOSED afterwards, and nothing needs to: the rounds are handed over at the moment
    -- the catch trips (see the magazine's seating), so by the time the slide is home the gun is no longer dry and
    -- the game has no reason to want it back. Rack a gun with no magazine in it and the slide returns to the stop,
    -- which is also what the real thing does.
    -- HOISTED out of the block below, which the thumb-press tick now splits in two: the second half still asks
    -- whether the gun is dry, and a `local` declared inside the first half would have been nil there.
    local empty = w0IsEmpty(weapon)
    if sc.lockLocal and sc.restLocal then
        if not empty then
            slideS.locked, slideS.userClosed = false, false
        elseif slideLockedNow() and not slideS.userClosed then
            slideS.locked = true
        end
        -- THE SLIDE RELEASE BUTTON: the right stick CLICK, which the VR core now consumes for exactly this (it
        -- used to be crouch, and crouch is the same stick pushed fully down -- shared slot 159,
        -- vrshared::kRightStickClick). A thumb closes a locked-back slide the way it does on the real pistol,
        -- without a whole hand reaching over the gun. Rising edge only, or a held click would fight the hand.
        local clickNow = (GetVRSharedSlot(159) or 0.0) > 0.5
        local clickEdge = clickNow and not slideS.clickPrev
        slideS.clickPrev = clickNow
        if clickEdge and empty and slideS.locked and not (slideS.grabbed or slideS.releasing) then
            slideS.userClosed, slideS.locked = true, false
            playSnd(weapon, snd and snd.slideRelease)
            -- ...and the thumb goes with it. The game's own hand does this: at the frame `end_slider` leaves the
            -- catch its thumb reaches over and pushes, while every other finger goes on holding the gun. Started
            -- here, run by the block below.
            if sc.pressPose and POSES[sc.pressPose] then pressS.t, pressS.hand = 0.0, holder end
            -- the rounds a waiting magazine brought go in now, exactly as they do when the hand rides it home
            if magS.pending and magS.pending > 0 then
                setMagAmmo(weapon, magS.pending)
                magS.pending = nil
            end
        end

        -- THE PLAYER'S ACTION OUTRANKS THE STOP: a hand that has ridden the slide home has settled the question
        -- until the gun is reloaded.
        if slideS.userClosed then slideS.locked = false end
    end

    -- THE THUMB PRESS, ticked outside the stop's guard so one already running always finishes. Ramp up, hold,
    -- ramp down -- the same shape as the finger fade, on its own state because it belongs to the OTHER hand.
    --
    -- It stands down if that hand is already wearing a reload pose: `fadeS` owns one hand at a time and the two
    -- would overwrite each other's joints. In practice they never want the same wrist -- everything else in this
    -- module poses the FREE hand -- but the holder detector can flip, and a flip must not leave a thumb stuck.
    if pressS.t >= 0 then
        -- THE SHAPE IS THE WHOLE POINT. A linear ramp of a few frames reads as a teleport: the thumb is simply
        -- somewhere else next frame and nothing about it says it hit anything. What sells a press is the STROKE --
        -- the thumb ACCELERATES onto the button, stops dead against it, dwells, and then comes off in its own
        -- unhurried time. So three separate spans with different curves rather than one symmetric ramp:
        --
        --   in    u*u          -- starts at nothing and is quickest at the moment of contact: that IS the impact
        --   hold  1            -- the pause against the stop, which is what makes the contact read as contact
        --   out   smoothstep   -- eased at both ends, so the thumb relaxes off the button instead of snapping back
        --
        -- Slower than the animation's own 0.17 s all told, deliberately: the game plays this on a hand the player
        -- is not looking at, and here it is a thing he asked to be able to see.
        local Ti = (cfg.slide and cfg.slide.pressIn)   or 0.07
        local Th = (cfg.slide and cfg.slide.pressHold) or 0.08
        local To = (cfg.slide and cfg.slide.pressOut)  or 0.18
        pressS.t = pressS.t + dt
        local a
        if pressS.t < Ti then
            local u = pressS.t / Ti
            a = u * u
        elseif pressS.t < Ti + Th then
            a = 1.0
        else
            local u = (pressS.t - Ti - Th) / To
            if u >= 1.0 then a = 0.0 else a = 1.0 - u * u * (3.0 - 2.0 * u) end
        end
        if a > 1.0 then a = 1.0 end
        local pp = cfg.slide and cfg.slide.pressPose and POSES[cfg.slide.pressPose]
        if pressS.t >= Ti + Th + To or not pp or fadeS.hand == pressS.hand then
            if pressS.on then clearGrip(pressS.hand) end
            pressS.t, pressS.on, pressS.hand = -1, false, nil
        else
            if not pressS.on then
                -- applied at whatever alpha the curve is at, which on the first frame is ~0: the fingers start
                -- exactly where tracking has them and move off from there, which is what makes it a stroke
                applyGrip(pressS.hand, pp, a)
                pressS.on = true
            elseif type(VRReloadFingerBlend) == 'function' then
                pcall(function() VRReloadFingerBlend(pressS.hand, a) end)
            end
        end
    end

    if cfg.slide and sc.lockLocal and sc.restLocal then

        -- THE CLAIM IS HELD AS A STAGE, NOT AS A BONE. This is the whole point: the slide sits back because the
        -- graph applies an override while the magazine is dry, so the fix is to switch that override off, not to
        -- shout over it with a measured position every frame. Holding the bone was both a hardcode and a fight --
        -- and while the module wrote a rest value the game's own recoil could not show at all.
        --
        -- Held while the hand is on the slide AND after it has closed it, because until a magazine goes in the gun
        -- is still dry and the override would come straight back.
        if empty and (slideS.userClosed or slideS.grabbed or slideS.releasing) then
            holdReloadStage(weapon)
            slideS.tagged = true
        elseif slideS.tagged then
            pushAnimEvent(weapon, 'InterruptReload')     -- rounds again, or no claim left: the weapon is the game's
            slideS.tagged = false
        end

    end

    -- NOTHING OF OURS ON THE BONE unless a hand is actually on it -- then the game's own recoil, slide stop and
    -- reload are all visible, and nothing of ours pins anything.
    --
    -- OUTSIDE the stop block on purpose. It used to live inside it, gated on `lockLocal`, so a weapon with no slide
    -- stop at all (the Kenshin) never released anything: the last absolute write stood for the rest of the session,
    -- and for a delayed rider that write is a value from the past -- the trailing part stopped short of home and
    -- stayed there ("при отпускании верхняя часть сама не доходит").
    --
    -- The DRAIN is the other half of that. A rider reads the driven part's position from `delayHome` ago, so at the
    -- instant the driven part reaches home the rider is still that far behind; letting the bone go right there
    -- freezes it behind. Writing zero for one delay longer walks the history to zero and takes the rider with it.
    -- `userClosed` only counts on a weapon that HAS a stop -- there it deliberately keeps our closed write on the
    -- bone so the graph cannot put the slide back on the catch. Without a stop it is meaningless, and honouring it
    -- pinned the bones for the rest of the session (see where it is set).
    if cfg.slide and not (slideS.grabbed or slideS.releasing or slideS.locked or (sc.lockLocal and slideS.userClosed)) then
        if (slideS.drain or 0) > 0 then
            slideS.drain = slideS.drain - dt
            driveSlide(0)
        elseif not releaseSlide() then
            driveSlide(0)                                   -- old plugin without the release native: keep the rest
        end
    end

    local spx, spy, spz = slotModel(slotComp, sc.slot, bx, by, bz, ci, cj, ck, cr)

    -- The gun's rigid basis: F = barrel forward (ax..az), D = mag-well "down" (mag slot made perpendicular to F),
    -- S = F x D. Every recorded grip lives in this frame; without the mag slot there is no preview and no snap.
    local dxb, dyb, dzb, sxb, syb, szb
    if spx then
        local mgx, mgy, mgz = slotModel(slotComp, (cfg.mag and cfg.mag.slot) or 'vrp_mag_std',
                                        bx, by, bz, ci, cj, ck, cr)
        if mgx then
            local dmx, dmy, dmz = mgx - spx, mgy - spy, mgz - spz
            local dd = dmx * ax + dmy * ay + dmz * az
            local px, py, pz = dmx - dd * ax, dmy - dd * ay, dmz - dd * az
            local pl = len3(px, py, pz)
            if pl > 1e-4 then
                dxb, dyb, dzb = px / pl, py / pl, pz / pl
                sxb = ay * dzb - az * dyb
                syb = az * dxb - ax * dzb
                szb = ax * dyb - ay * dxb
                -- WORLD DOWN, expressed in the round bone's own axes -- what makes the eject arc fall correctly
                -- however the gun is tilted. The bone frame is read off the anim data: local +Z is the muzzle
                -- direction (localAxis is (0,0,-1) = rearward), local +Y is up away from the magazine well
                -- (rideVec lifts the round by +Y), local +X is the cross axis. Model space has Z up, so world
                -- down is (0,0,-1) and each component is just minus that axis's Z.
                local ux, uy, uz = -dxb, -dyb, -dzb              -- local +Y (up)
                local cx = uy * az - uz * ay                      -- local +X = Y x Z, Z = barrel forward
                local cy = uz * ax - ux * az
                local cz = ux * ay - uy * ax
                slideS.gLoc = { -cz, -uz, -az }
            end
        end
    end
    -- THE FRAME A GRIP LIVES IN. By default the slot-derived basis above -- which needs the magazine slot to sit
    -- WELL off the barrel axis. On the Silverhand it does; on the Unity every slot is on the axis, and its magazine
    -- slot's perpendicular residual measures 5.8 mm (live: barrel 11.0, hammer 7.7, ejection port 27.6). A basis
    -- built on 5.8 mm is wrong and unstable at once: "down" came out pointing UP (down.Zup = +0.376, side.Zup =
    -- +0.787), so a grip's 7.8 cm side offset went vertical -- and the residual SWINGS as the slide travels, which
    -- is the hand shaking and the wrist rolling over at the end of the rack.
    --
    -- `basis = 'weapon'` takes the frame from the weapon entity's own orientation: exact, rigid, no lever arm, and
    -- unchanged by the rack. The barrel axis (ax) is left alone -- it has a 107 mm lever and is used for the rack
    -- projection, not for the roll.
    local gWeapon = false
    local gFx, gFy, gFz = ax, ay, az
    local gDx, gDy, gDz = dxb, dyb, dzb
    local gSx, gSy, gSz = sxb, syb, szb
    if cfg.basis == 'weapon' then
        local okq, wq0 = pcall(function() return weapon:GetWorldOrientation() end)
        if okq and wq0 then
            local w1, w2, w3, w4 = qmul(ci, cj, ck, cr, wq0.i, wq0.j, wq0.k, wq0.r)   -- weapon frame, MODEL space
            gWeapon = true
            gFx, gFy, gFz = qrot(w1, w2, w3, w4, 1, 0, 0)
            gDx, gDy, gDz = qrot(w1, w2, w3, w4, 0, 1, 0)
            gSx, gSy, gSz = qrot(w1, w2, w3, w4, 0, 0, 1)
            -- the slot basis gates the grip section below; a weapon-framed gun must not be gated by a slot it does
            -- not need, so seed it when the magazine slot could not give one
            if not dxb then
                dxb, dyb, dzb = gDx, gDy, gDz
                sxb, syb, szb = gSx, gSy, gSz
            end
        end
    end

    -- a grip's recorded wrist point, in model space; anchored to the slide slot, so it RIDES the rack
    -- `preview` shifts only the point the hand is MEASURED against, never the point it is held at. Where a hand
    -- naturally arrives and where the game's hand was recorded need not be the same, and moving the recorded point
    -- to meet the player moves the whole grip with it -- the rack then rides 3 cm off the slide.
    local function gripTarget(g, preview)
        local o = g.off
        local x, y, z = o[1], o[2], o[3]
        -- `previewOff` moves the point the grip is LOOKED FOR at and NOTHING else. The preview is a finger pose on
        -- the hand where the hand is -- it must never move the wrist. Only gripping does that.
        if preview and g.previewOff then
            x, y, z = x + g.previewOff[1], y + g.previewOff[2], z + g.previewOff[3]
        end
        return spx + gFx * x + gDx * y + gSx * z,
               spy + gFy * x + gDy * y + gSy * z,
               spz + gFz * x + gDz * y + gSz * z
    end

    -- THE AMMO WATCH. An empty magazine means an empty chamber: the round is not drawn at all (so a dry gun shows
    -- no round through the port -- the user's ask), nothing can be ejected, and nothing chambers. Reloading in the
    -- game refills the magazine, and the round reappears by itself here. Note the module can only READ the count;
    -- every setter was rejected from CET, so an ejected round is not yet deducted from the magazine.
    slideS.ammo = magAmmo(weapon)
    -- the hand-closed latch lasts until the gun is dry again: only then can the game legitimately lock the slide
    if slideS.ammo ~= nil and not slideS.ejectT and not slideS.chaseV then
        if slideS.ammo <= 0 then
            if slideS.chambered or slideS.seatedShown ~= false then
                slideS.chambered = false
                showSeated(0.001)
            end
        elseif (not slideS.chambered) and (not slideS.grabbed) and slideS.rack <= 0 then
            slideS.chambered = true
            showSeated(1.0)
        end
    end

    if spx and dxb then
        if slideS.grabbed then
            -- Everything in the grabbed state uses the hand LATCHED at grab time. Deriving "free" per frame sent
            -- the release to the WRONG hand whenever the holder detector flipped mid-grab -- the pinch brings both
            -- hands together at the grip, which is exactly the flip case -- and the pinch hand then stayed
            -- rotation-locked forever (the reported bug).
            local H = slideS.hand or free
            if (not gripOf(H)) or slideS.trip then
                slideS.grabbed, slideS.releasing, slideS.trip = false, true, nil  -- spring forward, NOT teleport
                -- COMING OFF THE CATCH, the spring runs to CLOSED, not back to the catch. While the slide is held
                -- back the rack is counted from the catch, so "rack = 0" means the catch -- and letting go simply
                -- put it back on the stop. Re-express the position against the closed rest before the spring
                -- starts: the same place, measured from the other end, so nothing jumps and the run is longer by
                -- exactly the stop's own 40 mm.
                if slideS.locked and sc.restLocal and sc.lockLocal then
                    local rl, lk = sc.restLocal, sc.lockLocal
                    slideS.rack = slideS.rack + len3(rl[1] - lk[1], rl[2] - lk[2], rl[3] - lk[3])
                    slideS.lockBase = nil
                end
                slideS.catchOff = false
                playSnd(weapon, snd and snd.slideRelease)            -- the spring, and the round chambering with it
                slideS.sndRider = false
                fadeOut()                                            -- fingers glide back to tracking
                pcall(function() VRHandStopModel(H, false, Vector4.new(0, 0, 0, 0)) end)
                if type(VRHandStopRot) == 'function' then pcall(function() VRHandStopRot(H, 0, 0, 0, 0, 1) end) end
                slideS.hand = nil
                -- (chambering happens on the way home -- see the releasing branch -- so the ejected round is
                -- not instantly re-shown at the port, which read as it being sucked back in)
            else
                local hgx, hgy, hgz = handModel(H)
                if hgx then
                    -- the slide follows the hand: how far it pulled back along the barrel, 0..travel
                    local pull = (hgx - slideS.hrx) * ax + (hgy - slideS.hry) * ay + (hgz - slideS.hrz) * az
                    local rack = -pull
                    -- OFF THE CATCH IT IS A FLICK, NOT A RACK. With the slide held back there is nowhere to pull:
                    -- the stop is already near the end of its travel, and all a hand can do is lift it clear.
                    -- Measured in the game's own empty reload -- the bone goes 45.9 -> 43.9 mm and then straight
                    -- forward -- that lift is 2.0 mm. So the pull is capped there, and reaching it trips the catch
                    -- and lets the slide go home, grip still held or not, which is what a slide stop does.
                    -- ...AND THEN FORWARD, BY HAND. Lifting it clear is only half of it: the hand that took the
                    -- slide off its catch should be able to RIDE it home, not stand and watch it go. So once the
                    -- catch is off, the travel opens the other way -- a negative rack walks the slide forward from
                    -- the catch to closed, the stop's own 40 mm.
                    local cap, lo, stop = sc.travel, 0.0, 0.0
                    if slideS.locked and sc.restLocal and sc.lockLocal then
                        local rl, lk = sc.restLocal, sc.lockLocal
                        stop = len3(rl[1] - lk[1], rl[2] - lk[2], rl[3] - lk[3])
                        cap = sc.lockPull or 0.002
                        if slideS.catchOff then lo = -stop end
                    end
                    if rack > cap then rack = cap elseif rack < lo then rack = lo end
                    slideS.rack = rack
                    driveSlide(rack)
                    if slideS.locked and rack >= cap then slideS.catchOff = true end
                    -- ridden all the way home: the stop is spent, and the gun loads if a magazine is waiting
                    if slideS.catchOff and stop > 0 and rack <= -stop + 0.002 then
                        slideS.locked, slideS.lockBase, slideS.catchOff = false, nil, false
                        slideS.rack, slideS.userClosed = 0.0, true
                        driveSlide(0)
                        playSnd(weapon, snd and snd.slideRelease)
                        if magS.pending and magS.pending > 0 then
                            setMagAmmo(weapon, magS.pending)
                            magS.pending = nil
                        end
                    end
                    -- THE SLIDE only sounds once it is really travelling: without a threshold every grip clicked,
                    -- because a grip that never moves the slide still counts as a grab.
                    if not slideS.moved and rack > (sc.pullSndAt or 0.05) * sc.travel then
                        slideS.moved = true                  -- also what gates the rotator's flourish
                        playSnd(weapon, snd and snd.slidePull)
                    end
                    -- THE FEEDER (bullet_pull) trails the slide with a delay and gets its own sound, late in the pull
                    -- where it actually catches up.
                    if not slideS.sndRider and rack > (sc.riderSndAt or 0.72) * sc.travel then
                        slideS.sndRider = true
                        playSnd(weapon, snd and snd.slideRider)
                    end
                    -- EJECT: pulling past the ejection point throws the chambered round out on a ballistic arc.
                    -- An empty chamber (no ammo) has nothing to throw -- racking a dry gun just moves metal.
                    if slideS.chambered and rack >= (sc.ejectAt or 0.85) * sc.travel then
                        slideS.chambered = false
                        -- Only arm the fling for a weapon that HAS round bones to fling. Without them nothing ever
                        -- clears the timer, and a stuck `ejectT` freezes the ammo watch that re-chambers the round.
                        slideS.ejectT = sc.chamberRound and 0.0 or nil
                        slideS.spent = consumeAmmo(weapon, 1)   -- the thrown round leaves the magazine for real
                        playSnd(weapon, snd and snd.eject)
                        playFx(weapon, sc.ejectFx)              -- the game's own casing, for a gun with no round bone
                    end
                    -- GLUE the wrist to the slide: recorded offset + recorded orientation, both rebuilt from the
                    -- LIVE gun basis every frame, so the hand rides the rack and turns with the gun (spread across
                    -- the recorded hold was 0.2 mm / 0.2 deg -- this IS the game's own placement).
                    if sc.gripSnap and slideS.grip and type(VRHandStopModel) == 'function' then
                        local tx, ty, tz = gripTarget(slideS.grip)
                        M.ownedHand = H                      -- this wrist is ours while it is glued to the slide
                        -- PULLED IN, NOT SNAPPED. The weight is a spring released at the grab (see pullIn);
                        -- at 0 the wrist is exactly where the player's hand is, at 1 it is on the recorded
                        -- grip, and in between it travels. The slide itself still answers the RAW hand from
                        -- the first frame, so this is what is SEEN and never what is measured.
                        local pw = pullIn('slide', dt)
                        local px, py, pz = tx, ty, tz
                        local hrx, hry, hrz = handModel(H)
                        if hrx and pw < 0.999 then
                            px = hrx + (tx - hrx) * pw
                            py = hry + (ty - hry) * pw
                            pz = hrz + (tz - hrz) * pw
                        end
                        VRHandStopModel(H, true, Vector4.new(px, py, pz, 1.0))
                        local gq = slideS.grip.rot
                        if gq and type(VRHandStopRot) == 'function' then
                            local b1, b2, b3, b4 = basisQuat(gFx, gFy, gFz, gDx, gDy, gDz, gSx, gSy, gSz)
                            local ri, rj, rk, rw = qmul(b1, b2, b3, b4, gq[1], gq[2], gq[3], gq[4])
                            -- the turn comes in on the same weight, from the hand's real orientation --
                            -- the magazine magnet's own nlerp, for the same reason it is used there
                            local hq = (type(VRHandRawRot) == 'function') and VRHandRawRot(H) or nil
                            if hq and pw < 0.999 then
                                local d2 = hq.i * ri + hq.j * rj + hq.k * rk + hq.r * rw
                                local sg = (d2 < 0) and -1.0 or 1.0
                                local qi = hq.i + (ri * sg - hq.i) * pw
                                local qj = hq.j + (rj * sg - hq.j) * pw
                                local qk = hq.k + (rk * sg - hq.k) * pw
                                local qr = hq.r + (rw * sg - hq.r) * pw
                                local l2 = math.sqrt(qi * qi + qj * qj + qk * qk + qr * qr)
                                if l2 > 1e-6 then
                                    ri, rj, rk, rw = qi / l2, qj / l2, qk / l2, qr / l2
                                end
                            end
                            VRHandStopRot(H, 1, ri, rj, rk, rw)
                        end
                    end
                end
            end
        else
            -- IDLE / PREVIEW. The grip STYLE is picked by what the hand is DOING, not by a zone: the nearest
            -- recorded wrist point wins, gated by the hand's orientation matching that grip's recorded
            -- orientation -- the pinch is the grip whose hand lies ALONG the barrel (the user's rule, and exactly
            -- what the recording holds). Inside previewRadius the finger pose fades in as a PREVIEW; gripping
            -- there latches the grab -- and since the hand is already at the recorded point, nothing teleports.
            -- A HAND WITH A MAGAZINE IN IT IS BUSY. The slide's grip points and the magazine's insertion pose are
            -- close enough for both preview zones to contain the same wrist, and then the two fought over the finger
            -- pose every frame -- each fading its own in and the other's out (seen as two blends at once at the well).
            -- While a magazine is held, only the magazine's pose may run.
            if not gripOf(free) then magS.gripLock = false end
            local best, bestStyle, bestD = nil, '-', 1e9
            local dbgRaw, dbgAng, dbgPer = nil, nil, nil
            if sc.grips and magS.state ~= 'hand' then
                local hq = (type(VRHandRawRot) == 'function') and VRHandRawRot(free) or nil
                local b1, b2, b3, b4 = basisQuat(gFx, gFy, gFz, gDx, gDy, gDz, gSx, gSy, gSz)
                for styleName, g in pairs(sc.grips) do
                    local tx, ty, tz = gripTarget(g, true)      -- detection only; the hold stays where it was
                    local d = len3(hx - tx, hy - ty, hz - tz)
                    dbgPer = (dbgPer or '') .. string.format(' %s:d=%.3f', styleName, d)
                    if dbgRaw == nil or d < dbgRaw then dbgRaw = d end
                    if d < bestD then
                        local angOk = true
                        if hq and g.rot then
                            local ri, rj, rk, rw = qmul(b1, b2, b3, b4, g.rot[1], g.rot[2], g.rot[3], g.rot[4])
                            local dq = math.abs(hq.i * ri + hq.j * rj + hq.k * rk + hq.r * rw)
                            if dq > 1.0 then dq = 1.0 end
                            local a = 2.0 * math.deg(math.acos(dq))
                            dbgPer = dbgPer .. string.format('/a=%.0f', a)
                            if dbgAng == nil or a < dbgAng then dbgAng = a end
                            angOk = a <= (sc.styleAngleMax or 60.0)
                        end
                        if angOk then best, bestStyle, bestD = g, styleName, d end
                    end
                end
            end
            slideS.dist = (bestD < 1e9) and bestD or -1
            -- WHY a grip was not offered. "dist=-1" alone cannot tell a hand that is far from one whose
            -- orientation failed the style gate, and those want opposite fixes.
            -- PER GRIP, because two independent minima cannot be compared: the nearest grip and the
              -- best-aligned one need not be the same grip, and reading them as a pair says the hand was in range
              -- when no single grip ever was.
            slideS.why = string.format('grips=%s magSt=%s wb=%s best=%.3f%s',
                sc.grips and 'Y' or 'n', tostring(magS.state), gWeapon and 'Y' or 'n',
                (bestD < 1e9) and bestD or -1, dbgPer or ' none')
            slideS.style = best and bestStyle or slideS.style
            -- WHERE THE HAND ACTUALLY IS relative to the grip on offer, split on the weapon's own axes -- across,
            -- along the bore, up. A single distance cannot answer "the snap lands higher than where I reached",
            -- because it cannot say WHICH WAY the gap points; these three can, and they are in the same units and
            -- the same order as the grip's `off`, so a systematic gap can be read off the log and moved straight
            -- into the config instead of being guessed at.
            if best then
                local btx, bty, btz = gripTarget(best, true)
                slideS.why = slideS.why .. string.format(' gap=(%+.3f,%+.3f,%+.3f)',
                    (hx - btx) * gFx + (hy - bty) * gFy + (hz - btz) * gFz,
                    (hx - btx) * gDx + (hy - bty) * gDy + (hz - btz) * gDz,
                    (hx - btx) * gSx + (hy - bty) * gSy + (hz - btz) * gSz)
            end

            -- NEAREST WINS HERE TOO. Arbitrating only on the magazine's side was half a fix: the slide latched a
            -- squeeze anywhere inside its preview radius no matter what else was nearer, and once it had the hand
            -- the magazine was vetoed by the `grabbed` guard. Widening the radius then made the slide swallow the
            -- magazine outright -- reported as "магазин не достаётся и очень далеко срабатывает". The two halves
            -- have to agree, so each part stands down for whichever the hand is actually closer to.
            local magNearer = (magS.snapD or -1) >= 0 and (magS.snapD < bestD)
            if best and bestD <= (sc.previewRadius or 0.05) and not magNearer then
                fadeIn(free, best)                                   -- the preview: fingers curl in over blendTime
                magS.slideHasPose = true
                -- A SQUEEZE BELONGS TO WHATEVER IT STARTED ON. `gripLock` already says "this squeeze is spent" for
                -- the magazine -- it is what stops a still-closed hand pulling the magazine straight back out of the
                -- well the moment it clicks in. The slide never honoured it, so that same held squeeze rolled
                -- straight on to the slide the instant the magazine was done: "после магазина сразу на затвор
                -- переключается при держащем грипе". Now it is one lock for both parts, cleared by letting go.
                if gripOf(free) and not magS.gripLock then
                    -- GRAB, latching the hand (see the grabbed branch for why)
                    slideS.grabbed, slideS.releasing = true, false
                    slideS.hand = free
                    slideS.grip = best
                    slideS.hrx, slideS.hry, slideS.hrz = hx, hy, hz  -- rack reference
                    magS.gripLock = true                             -- ...and spend it, so the magazine cannot have it
                    slideS.drain = 0.0                               -- a hand on it outranks a drain in progress
                    -- START FROM WHERE THE SLIDE IS. On the catch that is 40 mm back, not zero: writing zero here
                    -- is what snapped it shut the moment the grip closed.
                    slideS.lockBase = slideS.locked and sc.lockLocal or nil
                    -- A HAND ON THE SLIDE MEANS THE GAME LETS GO OF IT. While the magazine is dry the graph drives
                    -- the slide onto its catch every pass, and a hand pulling against that is a tug of war we only
                    -- win by shouting louder. The `Reload` event drops the override outright (see pushAnimEvent).
                    if w0IsEmpty(weapon) and not slideS.tagged then
                        pushAnimEvent(weapon, 'Reload')
                        slideS.tagged = true
                    end
                    driveSlide(0)                                    -- register + latch, at the right place
                end
            else
                -- Left the slide's zone. Only fade OUT if the slide is what owns the pose right now -- the magazine
                -- block runs after this one and asks for its own pose, and an unconditional fadeOut here cancelled
                -- it in the same frame (reported as "there is no pose and no blend").
                if magS.slideHasPose then fadeOut(); magS.slideHasPose = false end
            end

            if slideS.releasing then
                -- spring the slide forward at a fixed speed
                slideS.rack = slideS.rack - (sc.releaseSpeed or 1.5) * dt
                if slideS.rack <= 0 then
                    slideS.rack, slideS.releasing = 0, false
                    do  -- how long the riders still need after the driven part is home
                        local dr = 0.0
                        for _, rd in ipairs(sc.riders or {}) do
                            dr = math.max(dr, rd.delayHome or rd.delay or 0.0)
                        end
                        slideS.drain = (dr > 0) and (dr + 0.05) or 0.0
                    end
                    -- HOME, AND IT STAYS THERE. Coming off the slide stop the player has closed the action, so the
                    -- lock is spent and the rest position becomes the closed one again -- and it is written from
                    -- here on, so the animation cannot put the slide back on the catch behind his hand.
                    --
                    -- ONLY FOR A WEAPON THAT HAS A STOP. This claim exists to defeat the graph's empty-slide
                    -- override; a weapon with no stop has nothing to defeat, and setting it there was a real bug:
                    -- `userClosed` is only ever cleared inside the stop block or when a magazine seats, so on the
                    -- Kenshin the first rack latched it for good, the release/drain block below stopped running, and
                    -- the last write stood forever. For a delayed rider that write is the value from `delayHome` ago
                    -- -- full travel -- so the trailing part parked fully back and never moved again: it could not
                    -- "follow the pull" because it was already there, and it never came home after the release.
                    if sc.lockLocal then
                        slideS.userClosed = true      -- the player rode it home; it stays home
                    end
                    if slideS.locked then
                        -- NOT `userClosed = false` here. It was, and that is why the hold never survived a single
                        -- frame: the claim was set on the line above and cleared on this one, so the stop won and
                        -- the slide walked back onto the catch with the log still saying LOCK.
                        slideS.locked, slideS.lockBase = false, nil
                        playSnd(weapon, snd and snd.slideRelease)
                    end
                    -- AND THE ROUNDS GO IN HERE, outside that branch. They used to be inside it, and once the claim
                    -- began clearing `locked` at the moment the catch trips -- a frame or two before the spring
                    -- finishes -- the branch stopped running: the magazine was spent out of the reserve and the gun
                    -- stayed empty and unable to fire. Chambering belongs to the slide coming home, nothing else.
                    if magS.pending and magS.pending > 0 then
                        setMagAmmo(weapon, magS.pending)
                        magS.pending = nil
                    end
                    driveSlide(0)                         -- slide home
                    -- The disc only flourishes if the slide REALLY travelled (the same `pullSndAt` threshold the
                    -- sound uses). A grip that never moved the slide still ends in this branch, and it was spinning
                    -- the disc every single time.
                    if cfg.rotator and slideS.moved then rotS.t = 0 end
                    slideS.moved = false
                else
                    driveSlide(slideS.rack)
                end
            elseif rotS.t >= 0 then
                driveSlide(0)   -- keep the LAGGED riders converging home through the rotator window, so they
                                -- reach zero before the final rig-write clear instead of snapping at it
            end
            -- CHAMBERING: once the slide is on its way home past halfway (and the eject fling has finished),
            -- the next round appears and lag-chases the slide into the chamber -- the same reuse of the round
            -- meshes the game's own recoil anim does. TODO ammo accounting: decrement the magazine on eject and
            -- skip this when it is empty.
            -- ...and only if the magazine actually holds a round to strip (a dry gun stays empty: nothing shows,
            -- nothing chambers, and the next rack throws nothing).
            if (not slideS.chambered) and (not slideS.ejectT) and (not slideS.grabbed)
               and slideS.rack < 0.5 * sc.travel and (slideS.ammo == nil or slideS.ammo > 0) then
                slideS.chambered = true
                showSeated(1.0)                                  -- the next round becomes visible...
                slideS.chaseV = sc.travel * (sc.sign or 1.0)     -- ...at the open position, and chases home
            end
        end
    end
    -- THE EJECT FLING: the seated round shoots up-and-back out of the port for flingTime, then vanishes --
    -- mimicking the recoil anim's own move (a bare vanish read as "the wrong round disappeared").
    do
        local cr = sc.chamberRound
        if slideS.ejectT and cr then
            slideS.ejectT = slideS.ejectT + dt
            local ft = cr.flingTime or 0.2
            if slideS.ejectT >= ft then
                slideS.ejectT = nil
                showSeated(0.001)
                for i = 1, #cr.bones do
                    VRRigWrite(sc.which, cr.bones[i], 0, 0, 0)
                    VRRigWriteRot(sc.which, cr.bones[i], 0.0, 1, 0, 0)
                end
            else
                -- BALLISTIC THROW from where the round sits (it never rode the slide): p = v0*t + g*t^2/2, with
                -- v0 in the gun's frame (up, out the port, a little back) and gravity resolved into that frame
                -- (slideS.gLoc) so the fall is truly downward whatever the gun's attitude. Plus a tumble.
                local t2 = slideS.ejectT
                local v = cr.v0 or { side = 1.1, up = 1.9, back = 0.5 }
                local g = slideS.gLoc or { 0, -1, 0 }
                local gk = 0.5 * (cr.gravity or 9.81) * t2 * t2
                local px2 =  v.side * t2 + g[1] * gk
                local py2 =  v.up   * t2 + g[2] * gk
                local pz2 = -v.back * t2 + g[3] * gk
                local ang = (cr.spin or 900.0) * t2
                for i = 1, #cr.bones do
                    VRRigWrite(sc.which, cr.bones[i], px2, py2, pz2)
                    VRRigWriteRot(sc.which, cr.bones[i], ang, 1, 0, 0)
                end
            end
        end
    end
    -- THE CHAMBERING CHASE: the NEXT round appears at the open position and lag-trails the closing slide into
    -- the chamber -- the same meshes, reused, exactly as the game's own recoil anim reuses them.
    do
        local cr = sc.chamberRound
        if cr and slideS.chaseV then
            local d3 = slideS.rack * (sc.sign or 1.0)
            slideS.chaseV = slideS.chaseV + (d3 - slideS.chaseV) * (1.0 - math.exp(-dt / (cr.chaseLag or 0.08)))
            local rv = cr.rideVec or la
            for i = 1, #cr.bones do
                VRRigWrite(sc.which, cr.bones[i],
                    rv[1] * slideS.chaseV, rv[2] * slideS.chaseV, rv[3] * slideS.chaseV)
            end
            if math.abs(slideS.chaseV) < 0.1 * sc.travel then
                slideS.chaseV = nil
                for i = 1, #cr.bones do
                    VRRigWrite(sc.which, cr.bones[i], 0, 0, 0)
                    VRRigWriteRot(sc.which, cr.bones[i], 0.0, 1, 0, 0)   -- clear any leftover tumble
                end
            end
        end
    end
    fadeTick(dt, sc.blendTime or 0.15)

    -- ============================================================ THE MAGAZINE
    -- Held as a REAL SPAWNED ENTITY, not as a rig bone. The bone is parented to the gun, so anything written to it
    -- follows the gun's position AND orientation -- which is why a "held" magazine kept swimming and spinning -- and
    -- its mount frame cannot be recovered from script (a least-squares fit over a recorded reload left 36 cm of
    -- residual; all 48 signed axis permutations were worse). An entity lives in WORLD space instead, so it can sit
    -- exactly at the hand, with the hand's own orientation, and turn with the wrist because it IS at the wrist.
    --
    -- The entity is `base\vrport\vrp_mag_malorian.ent`, shipped by the port: the game's own magazine template
    -- stripped to a plain entMeshComponent on the real magazine mesh. The game's version draws it with a SKINNED
    -- mesh needing a live skeleton, so spawned standalone only its companion _shadow mesh appears -- the "I only
    -- see a shadow" in the headset.
    local mc = (cfg.mag and cfg.mag.enabled) and cfg.mag or nil
    if mc and mc.entity then
            -- WHERE THE MAGAZINE SITS IN THE HAND, per stage. It is not one relation: measured off the take, the
            -- magazine is 4.5 cm from the hold slot while it is carried and 12.5 cm at the instant it seats -- the
            -- hand slides down it and changes over to a palm on the base. Holding it at the seated relation while
            -- wearing the carry fingers is what put two fingers inside the magazine and left a gap at the others.
            --
            -- Measurable at every depth because the approach runs down the well's own centreline: the magazine's
            -- pose there is seat + axis * depth with the mount's rotation, so the in-hand relation follows.
            -- `magS.depth` is last frame's -- the current one cannot be known before the magazine is placed, and a
            -- frame of lag on a hand is nothing.
            -- the overlay's nudge, applied to whatever the config or a stage produced
            -- The variant's field if this stage has one, the plain field otherwise.
            local function vpick(stage, key)
                if magS.variant == 2 and stage[key .. '2'] then return stage[key .. '2'] end
                return stage[key]
            end

            local function tuned(o, q)
                local t = M.tune
                if t.dx == 0 and t.dy == 0 and t.dz == 0 and t.rx == 0 and t.ry == 0 and t.rz == 0 then
                    return o, q
                end
                local o2 = { o[1] + t.dx, o[2] + t.dy, o[3] + t.dz }
                local function ax(a, i, j, k)
                    local h = math.rad(a) * 0.5
                    local sn = math.sin(h)
                    return { i * sn, j * sn, k * sn, math.cos(h) }
                end
                local q2 = q
                for _, e in ipairs({ ax(t.rx, 1, 0, 0), ax(t.ry, 0, 1, 0), ax(t.rz, 0, 0, 1) }) do
                    local a1, a2, a3, a4 = qmul(q2[1], q2[2], q2[3], q2[4], e[1], e[2], e[3], e[4])
                    q2 = { a1, a2, a3, a4 }
                end
                return o2, q2
            end

            -- WHICH WAY UP THE HAND HAS IT, chosen per grab and composed onto whatever `holdAtRaw` returns. Only
            -- while a magazine is really in the hand: the preview has its own relation and must not inherit the
            -- roll of the last one carried.
            local function holdAtRaw()
                local st = mc.poseStages
                local d = (M.tune.stage >= 0) and M.tune.stage or magS.depth
                if not st or #st == 0 or not st[1].off then
                    return tuned(mc.holdOffset or { 0, 0, 0 }, mc.holdRotBasis or { 0, 0, 0, 1 })
                end
                if d == nil or d < 0 then d = 0 end
                local a, b, t = st[1], st[1], 0.0
                if d >= st[1].d then a, b, t = st[1], st[1], 0.0
                elseif d <= st[#st].d then a, b, t = st[#st], st[#st], 0.0
                else
                    for i = 1, #st - 1 do
                        if d <= st[i].d and d >= st[i + 1].d then
                            local span = st[i].d - st[i + 1].d
                            a, b = st[i], st[i + 1]
                            t = (span > 1e-6) and (st[i].d - d) / span or 0.0
                            break
                        end
                    end
                end
                local ao, bo, ar, br = vpick(a, 'off'), vpick(b, 'off'), vpick(a, 'rot'), vpick(b, 'rot')
                if not (ao and bo) then
                    return tuned(mc.holdOffset or { 0, 0, 0 }, mc.holdRotBasis or { 0, 0, 0, 1 })
                end
                if t <= 0.001 then return tuned(ao, ar) end
                local o = { ao[1] + (bo[1] - ao[1]) * t,
                            ao[2] + (bo[2] - ao[2]) * t,
                            ao[3] + (bo[3] - ao[3]) * t }
                local x, y = ar, br
                local dp = x[1] * y[1] + x[2] * y[2] + x[3] * y[3] + x[4] * y[4]
                local sg = (dp < 0) and -1.0 or 1.0
                local q = { x[1] + (y[1] * sg - x[1]) * t, x[2] + (y[2] * sg - x[2]) * t,
                            x[3] + (y[3] * sg - x[3]) * t, x[4] + (y[4] * sg - x[4]) * t }
                local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3] + q[4] * q[4])
                if l > 1e-6 then q = { q[1] / l, q[2] / l, q[3] / l, q[4] / l } end
                return tuned(o, q)
            end

            -- WHICH WAY UP THE HAND HAS IT, chosen per grab and composed onto whatever the ladder returns. Applied
            -- only while a magazine is really in the hand: the preview has its own relation (`previewRot`) and must
            -- not inherit the roll of the last one carried.
            local function holdAt()
                local o, r = holdAtRaw()
                local rr = magS.roll
                if rr and r and (magS.state == 'hand' or magS.state == 'glide') then
                    local q1, q2, q3, q4 = qmul(r[1], r[2], r[3], r[4], rr[1], rr[2], rr[3], rr[4])
                    return o, { q1, q2, q3, q4 }
                end
                return o, r
            end

        local wx, wy, wz = slotModel(slotComp, mc.slot, bx, by, bz, ci, cj, ck, cr)
        local haveSpawner = (type(exEntitySpawner) ~= 'nil')
        if wx and haveSpawner then
            -- THE HOLD SLOT, read straight from the game. `ItemAttachmentSlots.WeaponLeft` / `.WeaponRight` is the
            -- slot the game attaches a HELD item to -- it is exactly how the cigarette rides the hand: the cigarette
            -- is a real ITEM the game puts in that slot, and this port only drives the slot's bone.
            --
            -- A spawned entity cannot be attached there natively, and that is a fact of the API, not a shortcut: the
            -- player's slot component exposes GetSlotTransform to script and NOTHING else (probed live), while
            -- TransactionSystem:AddItemToSlot wants an ITEM record, which a bare mesh entity has none of, and
            -- Game.CreateObject is not exposed to CET at all. Holding the entity IN the slot's own transform is the
            -- same thing by construction, and that is measured, not assumed: ItemAttachmentSlots.WeaponLeft reads
            -- 0.000 m away from bone 26 composed through the player transform, with quaternion agreement 1.0000.
            --
            -- Reading the slot also retires three assumptions the composition needed -- the bone index, the freshness
            -- of the FK snapshot, and VRHandRawWorld (measured 0.460 m from a weapon slot against the player-route's
            -- 0.187 m: it composes through the RENDER VIEW, ~0.33 m high, which was the "magazine floats half a metre
            -- above my hand"). The slot transform is already world, so nothing is composed at all.
            local function holdSlotTr(side)
                if not magS.slotComp then
                    local pl = Game.GetPlayer()
                    local ok, cs = pcall(function() return pl:GetComponents() end)
                    if not (ok and cs) then return nil end
                    for i = 1, #cs do
                        if tostring(cs[i]:GetName()):find('ItemAttachmentSlots') then magS.slotComp = cs[i]; break end
                    end
                end
                if not magS.slotComp then return nil end
                local nm = (side == 0) and (mc.holdSlot or 'WeaponLeft') or (mc.holdSlotRight or 'WeaponRight')
                local ok, f, tr = pcall(function() return magS.slotComp:GetSlotTransform(CName.new(nm)) end)
                if not (ok and f and tr) then magS.slotComp = nil; return nil end   -- stale handle after a load
                local okp, p = pcall(function() return WorldPosition.ToVector4(tr.Position) end)
                local okq, q = pcall(function() return tr.Orientation end)
                if not (okp and p and okq and q) then return nil end
                return p, q
            end
            local function magEnt()
                if not magS.entId then return nil end
                local ok, e = pcall(function() return Game.FindEntityByID(magS.entId) end)
                return (ok and e) or nil
            end
            -- ONE magazine at a time, keyed on the ENTITY ID rather than on a resolved entity. Spawning is not
            -- instant: for a frame or two `Game.FindEntityByID` still answers nil, and code that asked "is there an
            -- entity?" spawned a second one -- then a third -- each orphaning the last, which is exactly the three
            -- magazines that showed up in the world.
            -- WHICH APPEARANCE THE MAGAZINE IS DRAWN IN. Every one of these weapons ships several, and the gun's
            -- own magazine follows the gun -- `meshAppearance` on its magazine component. A spawned mesh entity
            -- knows nothing about that and comes up in `default`, so a legendary Kang Tao Kappa (whose magazine
            -- reads `legendary2`) sat there with a red magazine in the gun and a blue one in the hand.
            --
            -- Read the gun, not the item record: the record would need a table of appearance names per weapon, and
            -- the component already holds the answer the renderer is using.
            local function magSkin()
                local ok, cs = pcall(function() return weapon:GetComponents() end)
                if not (ok and cs) then return nil end
                for i = 1, #cs do
                    local c = cs[i]
                    local nm = tostring((c.name and c.name.value) or '')
                    if string.find(nm, 'mag_std') then
                        local o, a = pcall(function() return c.meshAppearance end)
                        local v = (o and a) and tostring(a.value or a) or ''
                        -- the first one that is not the default IS the answer: a weapon's shadow and decal
                        -- magazine components stay on `default` whatever the gun wears
                        if v ~= '' and v ~= 'default' then return v end
                    end
                end
                return nil
            end
            -- ...and put it on ours. The entity does not always resolve on the frame it is spawned (the same
            -- asynchrony the despawn has to cope with), so the wanted skin is remembered and retried.
            local function applySkin()
                local skin = magS.skinWant
                if not skin then return end
                local e = magEnt()
                if not e then return end
                local done = pcall(function()
                    local cs = e:GetComponents()
                    for i = 1, #cs do
                        local c = cs[i]
                        if tostring((c.name and c.name.value) or '') == 'mag_mesh' then
                            c.meshAppearance = CName.new(skin)
                            if type(c.RefreshAppearance) == 'function' then c:RefreshAppearance() end
                        end
                    end
                end)
                if done then magS.skinWant = nil end
            end

            -- `ent` overrides which entity is spawned. WHAT LEAVES A GUN AND WHAT A HAND BRINGS ARE NOT ALWAYS THE
            -- SAME OBJECT: on a revolver the cylinder gives up six spent CASES and the hand brings a SPEEDLOADER --
            -- a star with its own handle and its own rounds. One entity for both roles put the holder in the
            -- cylinder and dropped it along with the cases.
            local function spawnMag(px, py, pz, qi, qj, qk, qr, ent)
                if magS.entId then return true end
                local ok = pcall(function()
                    local tr = WorldTransform.new()
                    tr:SetPosition(Vector4.new(px, py, pz, 1.0))
                    tr:SetOrientation(Quaternion.new(qi, qj, qk, qr))
                    magS.entId = exEntitySpawner.Spawn(ent or mc.entity, tr, '')
                end)
                magS.hold = nil          -- a fresh magazine carries none of the last one's motion...
                leadReset('hand')        -- ...and none of its step either: it appears, it did not travel here
                magS.skinWant = magSkin()
                applySkin()
                return ok and magS.entId ~= nil
            end
            -- Despawn, and if the entity will not resolve YET (same asynchrony as the spawn), keep the id on a litter
            -- list and retry each frame. Dropping an unresolvable id is how a magazine gets left in the world for good.
            local function despawnMag()
                local e = magEnt()
                if e then pcall(function() exEntitySpawner.Despawn(e) end)
                elseif magS.entId then magLitter[#magLitter + 1] = magS.entId end
                magS.entId = nil
            end
            local function placeMag(px, py, pz, qi, qj, qk, qr, side, lx, ly, lz, k1, k2, k3, k4)
                -- THE CARRIER OWNS THE HELD MAGAZINE. The world pair is still computed and still used -- the throw
                -- velocity is taken off it below and the seating test measures against it -- but what is DRAWN is
                -- placed in the slot's own frame, where nothing composes and nothing can be a frame late.
                local onCarrier = (side ~= nil) and holdS.on[side] and (lx ~= nil)
                -- ...and the item route, which has NEITHER a spawned entity nor a carrier -- the game holds the
                -- thing itself. It has to be named in this guard or the early return below drops the frame
                -- before the placement runs, which is exactly what happened: the log showed the grab taking the
                -- item route and not one placement marker after it, so the grip and the rendering plane were
                -- never written and both looked like they had failed on their own.
                local onItem = (side ~= nil) and (lx ~= nil) and (itemS.name[side] ~= nil)
                local e = magEnt()
                if not (e or onCarrier or onItem) then return false end
                if e and magS.skinWant then applySkin() end
                -- HOW FAST THE HAND IS MOVING IT. This is the only place a held magazine is placed, so it is the only
                -- place that can know -- and a thrown magazine needs it: released while the arm swings it should fly,
                -- released while the hand is still it should drop. Smoothed over roughly two frames on purpose: one
                -- frame of controller jitter is metres per second and would fling a magazine that was merely let go.
                local ph = magS.hold
                if ph then
                    local iv = 1.0 / ((dt > 1e-3) and dt or 1e-3)
                    local a = 0.45
                    ph.vx = ph.vx + ((px - ph.px) * iv - ph.vx) * a
                    ph.vy = ph.vy + ((py - ph.py) * iv - ph.vy) * a
                    ph.vz = ph.vz + ((pz - ph.pz) * iv - ph.vz) * a
                    -- and how fast it is TURNING: w = 2*vec(q * conj(qPrev))/dt, with the antipode folded first --
                    -- the two representations of one orientation differ in sign, and reading across that flip gives
                    -- a spin of hundreds of rad/s out of a hand that barely moved.
                    local si = (qi * ph.qi + qj * ph.qj + qk * ph.qk + qr * ph.qr < 0.0) and -1.0 or 1.0
                    local d1, d2, d3 = qmul(qi * si, qj * si, qk * si, qr * si, -ph.qi, -ph.qj, -ph.qk, ph.qr)
                    ph.wx = ph.wx + (2.0 * d1 * iv - ph.wx) * a
                    ph.wy = ph.wy + (2.0 * d2 * iv - ph.wy) * a
                    ph.wz = ph.wz + (2.0 * d3 * iv - ph.wz) * a
                    ph.px, ph.py, ph.pz = px, py, pz
                    ph.qi, ph.qj, ph.qk, ph.qr = qi, qj, qk, qr
                else
                    magS.hold = { px = px, py = py, pz = pz, qi = qi, qj = qj, qk = qk, qr = qr,
                                  vx = 0.0, vy = 0.0, vz = 0.0, wx = 0.0, wy = 0.0, wz = 0.0 }
                end
                -- AND HERE, at the write, the frame the entity would otherwise be behind by. Above this line the
                -- raw target is what everything reads -- the throw velocity was just taken off it, and the
                -- seating test takes it next -- so this touches only what is drawn. It has to be that way: the
                -- gun's own seat is read from the same stale game state, so the two are stale TOGETHER and the
                -- geometry between them is right. Only their common anchor is behind.
                --
                -- WHAT THE MEASUREMENT SETTLED, 2026-08-16, 34 samples at 3 to 8 m/s:
                --
                --     anchor = 0 mm throughout        the plugin's engine-side player position and this
                --                                     script's own read agree to under half a millimetre, at
                --                                     eight metres a second. The script's reading is NOT a
                --                                     frame behind the engine, so the gap is not there.
                --     |want - got| = 0.20..0.24 m     one frame of the target's own travel, which is what that
                --                                     difference has to be: `got` is read before this frame's
                --                                     write, so it holds the last one. Standing still it is
                --                                     0.000 m, which is the control.
                --
                -- The second number is the size of the problem: at a run, ONE frame is 21 cm, and 21 cm is the
                -- magazine floating clear of the hand. Whatever the lag is, it is downstream of everything this
                -- script can read -- so `freshDelta` stays (it costs nothing and holds the anchor honest) and
                -- the lead is the only lever left. It is empirical on purpose: a frame is a visible 21 cm, so
                -- turning it on the slider while running says how many frames it really is. One was measurably
                -- better and still not enough, which is why the default is two.
                if onCarrier then
                    -- and here it ends: one write, in the frame the hand is drawn in, with no correction of any
                    -- kind on it. The lead and the anchor below are the entity route's, and only its.
                    holdPlace(side, lx, ly, lz, k1, k2, k3, k4)
                    magS.diag = (magS.diag or '') .. ' carrier'
                    return true
                end
                -- ...and the item route ends the same way, one step further out: the game has the thing on the
                -- slot already, so all that is written is the measured grip, onto its parts.
                if onItem then
                    local slotName = (side == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight'
                    magS.diag = (magS.diag or '') ..
                        (itemPlace(side, slotName, lx, ly, lz, k1, k2, k3, k4) and ' item' or ' item(waiting)')
                    return true
                end
                local dax, day, daz, dad = freshDelta(bx, by, bz)
                local ax, ay, az, step = leadWorld('hand', px + dax, py + day, pz + daz)
                -- the sentinels are named rather than printed as a negative distance: this line is the one that
                -- decides whether the reading was stale at all, and "anchor=-1000mm" answers nothing
                magS.diag = (magS.diag or '') ..
                    string.format(' anchor=%s step=%.0fmm lead=%.0fmm',
                        (dad >= 0.0) and string.format('%.0fmm', 1000.0 * dad)
                            or ((dad > -1.5) and 'no-native' or 'rejected'),
                        1000.0 * (step or 0.0),
                        1000.0 * len3(ax - px - dax, ay - py - day, az - pz - daz))
                return pcall(function()
                    local tr = WorldTransform.new()
                    tr:SetPosition(Vector4.new(ax, ay, az, 1.0))
                    tr:SetOrientation(Quaternion.new(qi, qj, qk, qr))
                    e:SetWorldTransform(tr)
                end)
            end
            -- THE SEATED MAGAZINE'S VISIBILITY, through the game's own switch. The mesh in the gun is drawn with
            -- `visibilityAnimationParam = showMagazine`, and that names a FLOAT TRACK on the magazine rig -- index 0
            -- of its two, `showMagazine` and `showMagazineReload`, resting at 1 and 0 (all four facts read out of
            -- w_handgun__malorian_silverhand__mag_std.rig and the magazine .ent, so nothing here is inferred).
            -- Writing 0 to that track is precisely what the game's own reload animation does to make the magazine
            -- disappear, and dropping the write hands it straight back to the animation.
            --
            -- This replaces shrinking the bone's scale to 0.001. That trick worked, but it had to register a BONE
            -- write, and holding a bone every frame PINS the game's magazine animation -- with it on, a native reload
            -- stopped ejecting the magazine at all. The track route touches no bone, so the game's own motion keeps
            -- running underneath and `idleFree` is no longer papering over anything.
            local function gunMag(show)
                if type(VRRigTrackWrite) ~= 'function' then return false end
                local trk = mc.showTrack or 0
                if show then
                    return pcall(function() VRRigTrackWrite(mc.which, trk, 1.0, 0) end)   -- back to the animation
                end
                return pcall(function() VRRigTrackWrite(mc.which, trk, 0.0, 1) end)       -- hidden, held by us
            end

            local hmx, hmy, hmz = handModel(free)
            magS.dist = hmx and len3(hmx - wx, hmy - wy, hmz - wz) or -1
            local grip = hmx and gripOf(free)
            -- WHICH HAND, AND IS IT SQUEEZING. "It drops while I hold the grip" can only be read from these: the
            -- drop happens on `not grip`, and `grip` is sampled on `free` -- so if the holder flips, or the slide
            -- claims the hand, the squeeze is read off the wrong wrist and the magazine is let go.
            magS.dbg2 = string.format(' g=%s lock=%s free=%d H=%s hand=%s',
                grip and 'Y' or 'n', magS.gripLock and 'Y' or 'n', free,
                tostring(magS.hand), hmx and 'Y' or 'NIL')

            local btn = (GetVRSharedSlot(157) or 0.0) > 0.5
            local btnEdge = btn and not magS.btnWas
            magS.btnWas = btn

            -- A GRIP THAT SEATED A MAGAZINE IS SPENT. Without this the grab is a level test, so the same held grip
            -- picked the magazine straight back out of the well the moment it clicked in -- to take another one the
            -- player has to let go and squeeze again, which is also what a hand does.
            if not grip then magS.gripLock = false end
            local gripTake = grip and not magS.gripLock

            magS.poseOK = (POSES[mc.pose] ~= nil) and (POSES[mc.pose] ~= false)

            -- THE WRIST POSE THAT HOLDS THE MAGAZINE WHERE IT SITS IN THE GUN -- the magnet's target, and the same
            -- one for both directions, because pulling out and pushing in are the same hand pose.
            --
            -- Computed, not recorded: the magazine's seated pose is known (the mag-well slot, and the weapon's own
            -- orientation), its place in the hand is the constant above, and the wrist -> hold-slot relation is
            -- measured live off bones 23/24 -> 26/28. Inverting that chain gives the wrist pose wanted:
            --     q_slot = q_mag * conj(K)          p_slot = p_mag - q_slot * holdOffset
            --     q_wrist = q_slot * conj(rel)      p_wrist = p_slot - q_wrist * relPos
            -- The WRIST -> HOLD SLOT relation, read live off the bones. Rigid, so it is unaffected by the collision
            -- clamp (which only ever translates a whole rigid set) and by our own pin (which moves parent and child
            -- together): both the difference and the relative rotation cancel.
            local function wristToSlot(side)
                local bw = (side == 0) and 23 or 24
                local bs = (side == 0) and 26 or 28
                local pw = (type(VRBoneModelPos) == 'function') and VRBoneModelPos(bw) or nil
                local qw2 = (type(VRBoneModelRot) == 'function') and VRBoneModelRot(bw) or nil
                local ps = (type(VRBoneModelPos) == 'function') and VRBoneModelPos(bs) or nil
                local qs2 = (type(VRBoneModelRot) == 'function') and VRBoneModelRot(bs) or nil
                if not (pw and qw2 and ps and qs2) then return nil end
                if not (pw.w and pw.w > 0.5 and ps.w and ps.w > 0.5) then return nil end
                local e1, e2, e3, e4 = qmul(-qw2.i, -qw2.j, -qw2.k, qw2.r, qs2.i, qs2.j, qs2.k, qs2.r)
                local rx, ry, rz = qrot(-qw2.i, -qw2.j, -qw2.k, qw2.r, ps.x - pw.x, ps.y - pw.y, ps.z - pw.z)
                return rx, ry, rz, e1, e2, e3, e4
            end

            -- WHERE THE MAGAZINE WOULD BE IF NOTHING HELD THE HAND -- built from the RAW tracked wrist, so it is a
            -- pure function of tracking. Every magnet weight is measured with this and never with the drawn hand.
            --
            -- Measured why: driving the weight off the drawn slot is a feedback loop with a one-frame delay -- the
            -- magnet pulls the hand, the hand carries the slot, the slot moves the magazine closer, so the weight
            -- rises and pulls harder. The log caught it exactly: the weight swinging 0.023..0.546 inside one second
            -- while the hand itself moved 4 cm. That was the shake.
            -- The entity-origin correction, rotated by whatever the magazine's orientation is at the time.
            local function originShift(qi, qj, qk, qr)
                local o = mc.originOffset
                if not o then return 0.0, 0.0, 0.0 end
                return qrot(qi, qj, qk, qr, o[1], o[2], o[3])
            end

            -- THE RULER the depth is measured with: a relation that does NOT depend on the current depth -- stage 1's
            -- offset, which is the CARRY placement of the variant actually in hand, or the plain hold for a weapon
            -- that does not stage its placement.
            --
            -- Fixedness is the point: the DEPTH is a projection of the magazine's position on the well axis, so if it
            -- were computed with the CURRENT stage's offset then depth would pick a stage, the stage would move the
            -- magazine and the magazine would change the depth, at frame rate -- the ringing staged offsets produced
            -- before. The variant is chosen once per grab, so reading stage 1 keeps that guarantee.
            --
            -- It was `mc.holdOffset` outright until a weapon carried a magazine far from where it seats it: the
            -- Lexington's fist hold is 166 mm from its seated relation, so the depth described a magazine 166 mm away
            -- from the real one and the well never armed ("в кулаке шахта не работает").
            local function magBaseOff()
                local st = mc.poseStages
                local o = st and st[1] and vpick(st[1], 'off')
                return o or mc.holdOffset or { 0, 0, 0 }
            end

            local function magFromRaw(side, base)
                local p = VRHandRawModel(side)
                local q = (type(VRHandRawRot) == 'function') and VRHandRawRot(side) or nil
                local rx, ry, rz, e1, e2, e3, e4 = wristToSlot(side)
                if not (p and q and rx) then return nil end
                local s1, s2, s3, s4 = qmul(q.i, q.j, q.k, q.r, e1, e2, e3, e4)     -- the slot, from the raw wrist
                local ax2, ay2, az2 = qrot(q.i, q.j, q.k, q.r, rx, ry, rz)
                local ho = base and magBaseOff() or (holdAt())
                local ox, oy, oz = qrot(s1, s2, s3, s4, ho[1], ho[2], ho[3])
                return p.x + ax2 + ox, p.y + ay2 + oy, p.z + az2 + oz
            end

            -- `extra` turns the hold by a rotation in the MAGAZINE's own axes before the wrist is solved from it.
            -- It exists for one shape: a magazine that is a cylinder has a free roll about its own length -- the eye
            -- cannot see it, so the same magazine can be TAKEN one way round and PUSHED IN the other, which is what
            -- a hand naturally does. One rigid relation cannot express both; this is the difference between them,
            -- and it is applied to the preview only (see `mag.previewRot`).
            local function magWristTarget(side, extra)
                if not dxb then return nil end
                local okw, wq = pcall(function() return weapon:GetWorldOrientation() end)
                if not (okw and wq) then return nil end
                local wmi, wmj, wmk, wmr = qmul(ci, cj, ck, cr, wq.i, wq.j, wq.k, wq.r)     -- weapon, model space
                local b1, b2, b3, b4 = basisQuat(gFx, gFy, gFz, gDx, gDy, gDz, gSx, gSy, gSz)
                local r1, r2, r3, r4 = qmul(-b1, -b2, -b3, b4, wmi, wmj, wmk, wmr)
                local _, hb = holdAt()
                if extra then
                    local e1x, e2x, e3x, e4x = qmul(hb[1], hb[2], hb[3], hb[4], extra[1], extra[2], extra[3], extra[4])
                    hb = { e1x, e2x, e3x, e4x }
                end
                local k1, k2, k3, k4 = qmul(hb[1], hb[2], hb[3], hb[4], r1, r2, r3, r4)     -- slot -> magazine
                -- HOW THE SEATED MAGAZINE IS TURNED relative to the weapon. The Silverhand needs nothing here: its
                -- rig chain is four 180 deg quaternions that cancel in pairs, so its magazine faces exactly as the
                -- gun does. That is NOT general -- the Unity's mount is turned a clean 180 deg about (0, .707, .707)
                -- (measured over 171 seated frames, 0.46 deg of spread), and assuming otherwise put the magazine in
                -- the hand back to front and made the grab target unreachable.
                local sr = mc.seatRot
                local mi, mj, mk, mr = wmi, wmj, wmk, wmr
                if sr then mi, mj, mk, mr = qmul(wmi, wmj, wmk, wmr, sr[1], sr[2], sr[3], sr[4]) end
                local s1, s2, s3, s4 = qmul(mi, mj, mk, mr, -k1, -k2, -k3, k4)              -- the slot pose wanted
                local ho = (holdAt())
                local ox, oy, oz = qrot(s1, s2, s3, s4, ho[1], ho[2], ho[3])
                -- NO ORIGIN SHIFT HERE, and it was tried: this target is a WRIST position, and the chain from it to
                -- the entity already carries the shift (handTarget adds it). Adding it again aimed the magnet a
                -- second 59.7 mm away and made the miss worse rather than better -- measured in play. What this
                -- function aims at is the uncorrected chain landing on the well, which is exactly the condition that
                -- puts the shifted entity on the shifted seat.
                local sx2, sy2, sz2 = wx - ox, wy - oy, wz - oz
                local rx, ry, rz, e1, e2, e3, e4 = wristToSlot(side)
                if not rx then return nil end
                local t1, t2, t3, t4 = qmul(s1, s2, s3, s4, -e1, -e2, -e3, e4)
                local vx, vy, vz = qrot(t1, t2, t3, t4, rx, ry, rz)
                return sx2 - vx, sy2 - vy, sz2 - vz, t1, t2, t3, t4
            end

            -- WHICH WAY UP THE HAND IS HOLDING IT. A cylinder can be carried either way round its own length and
            -- both are real grips -- the game's hand happens to use one, and a player reaching for the magazine has
            -- no way of knowing which. So ask the hand: of the two relations, take the one whose wrist is nearer the
            -- wrist that is actually there. The magazine looks identical either way, so nothing about it moves.
            --
            -- The current choice is cleared for the comparison, or the test would be "current versus its flip" and
            -- could sit there toggling.
            -- ...AND A SPEEDLOADER HAS SIX OF THEM, not two. `rollSym` says the magazine is n-fold symmetric about
            -- an axis of its own -- the Overture's loader is six rounds evenly round a circle, so any multiple of
            -- 60 deg about that circle's axis is the same object seen again -- and the argument above then applies
            -- n times over instead of twice. Without it the wrist is dragged to ONE arbitrary roll of the six, which
            -- can be a half turn from where the hand already is: "руку разворачивает в обратную сторону". With it
            -- the worst it can ever ask for is half a step, 30 deg.
            --
            -- IT COSTS NOTHING ELSEWHERE, and that is provable rather than hoped for. The wrist target is
            -- `b * conj(hold)` (b being the weapon's own basis), so the magazine it carries lands at
            -- `b * conj(hold) * hold = b` -- the weapon's orientation, whatever the hold is. Compose a roll R onto
            -- the hold and the same cancellation happens: `b * conj(hold*R) * (hold*R) = b`. The seated magazine is
            -- untouched by construction; the roll is visible only in the WRIST, which is the thing being chosen.
            --
            -- The axis is `{ 0, 1, 0 }` for the same reason: since the seated magazine's orientation IS the weapon's,
            -- the magazine's own axes coincide with the weapon's when it is home, and the weapon's Y is the bore.
            local function rollWays()
                if mc.rollWays then return mc.rollWays end
                if mc.rollSym then
                    local a = mc.rollSym.axis or { 0, 1, 0 }
                    local n = mc.rollSym.n or 2
                    local l = len3(a[1], a[2], a[3])
                    if l < 1e-6 or n < 2 then return nil end
                    local out = {}
                    for k = 1, n - 1 do
                        local h = math.pi * k / n                 -- half of the k-th turn, 2*pi*k/n
                        local sn = math.sin(h) / l
                        out[#out + 1] = { a[1] * sn, a[2] * sn, a[3] * sn, math.cos(h) }
                    end
                    return out
                end
                if mc.rollAlt then return { mc.rollAlt } end
                return nil
            end

            local function pickRoll(side)
                local ways = rollWays()
                if not ways then return nil end
                local hq = (type(VRHandRawRot) == 'function') and VRHandRawRot(side) or nil
                if not hq then return magS.roll end
                local keep = magS.roll
                magS.roll = nil
                local best, bd = nil, nil
                for k = 0, #ways do
                    local w = (k > 0) and ways[k] or nil
                    local _, _, _, q1, q2, q3, q4 = magWristTarget(side, w)
                    if q1 then
                        local d = math.abs(hq.i * q1 + hq.j * q2 + hq.k * q3 + hq.r * q4)
                        if bd == nil or d > bd then best, bd = w, d end
                    end
                end
                magS.roll = keep
                if bd == nil then return keep end
                return best
            end

            -- WHICH OF TWO GRIPS THE HAND IS ACTUALLY MAKING. A weapon may describe two ways of taking its
            -- magazine, and they are not always interchangeable. The Lexington's two carries are -- either is a
            -- fine way to hold that magazine in the air, so it picks at random. The Kappa's are not: the palm goes
            -- round the body and the pinch takes the rear END, 46 mm apart along the magazine, and choosing by coin
            -- reads as arbitrary ("щипковый должен быть только когда берёшь сзади за магазин").
            --
            -- So ask the question the roll already asks: solve the wrist each grip would need and take whichever is
            -- nearer the wrist that is really there. Position and angle in one score, a radian counted as 50 mm.
            --
            -- Only when TAKING ONE OUT OF THE GUN. A magazine conjured from nowhere has no rear end to reach behind
            -- -- the hand is wherever the player's hand was -- so that case keeps the first grip.
            local function pickVariant(side)
                local st = mc.poseStages
                if not (st and st[1] and st[1].off2) then return nil end
                local hp = VRHandRawModel(side)
                local hq = (type(VRHandRawRot) == 'function') and VRHandRawRot(side) or nil
                if not (hp and hq) then return nil end
                local keep, best, bi = magS.variant, nil, 1
                for v = 1, 2 do
                    magS.variant = v
                    local tx, ty, tz, q1, q2, q3, q4 = magWristTarget(side)
                    if tx then
                        local d = len3(tx - hp.x, ty - hp.y, tz - hp.z)
                        local dot = math.abs(hq.i * q1 + hq.j * q2 + hq.k * q3 + hq.r * q4)
                        if dot > 1.0 then dot = 1.0 end
                        local sc = d + 2.0 * math.acos(dot) * 0.05
                        if (best == nil) or (sc < best) then best, bi = sc, v end
                    end
                end
                magS.variant = keep
                return bi
            end

            -- THE MAGNET. Weighted by distance -- nothing at `snapRadius`, full at the target -- so there is no
            -- threshold to cross and nothing can jump: grabbing while magnetised takes the magazine out exactly as it
            -- sat, and bringing it back draws the hand into the insertion pose the same way. Small radius on purpose:
            -- it only bites right at the magazine.
            --
            -- The pull is applied from the RAW tracked hand, never from the rendered one, so it cannot feed back on
            -- itself and creep the hand toward the target over frames.
            local twx, twy, twz, tq1, tq2, tq3, tq4 = magWristTarget(free)
            -- WHERE IT IS REACHED FOR, which need not be where it is HELD -- the positional twin of `previewRot`,
            -- and the same idea the slide's grips have carried since the Unity: a tracked arm does not arrive where
            -- the animation's wrist sits, so the catch is offered where the hand really comes to and the hold stays
            -- where it was tuned. Read in the weapon's own axes, like a grip's `off`: X across, Y along the bore,
            -- Z up. It moves the CATCH and the pull that goes with it, never the hold and never the insertion.
            local rox, roy, roz = 0.0, 0.0, 0.0
            local mw1, mw2, mw3, mw4
            do
                local okq4, wq4 = pcall(function() return weapon:GetWorldOrientation() end)
                if okq4 and wq4 then
                    mw1, mw2, mw3, mw4 = qmul(ci, cj, ck, cr, wq4.i, wq4.j, wq4.k, wq4.r)
                    if mc.previewOff then
                        rox, roy, roz = qrot(mw1, mw2, mw3, mw4,
                            mc.previewOff[1], mc.previewOff[2], mc.previewOff[3])
                    end
                end
            end
            local snapR = mc.snapRadius or 0.05
            local snapD = (twx and hmx) and len3(hmx - twx - rox, hmy - twy - roy, hmz - twz - roz) or -1
            -- ...OR: A PLACE ON THE GUN, PLUS "PALM UP". Everything above measures the hand against a wrist target
            -- reconstructed through the hold relation, and that point is doubly unfriendly: it is invisible, and
            -- the tracked hand point is not the wrist JOINT but a spot ahead of it, so turning the hand swings it
            -- through several centimetres. Inside a 50 mm ball that leaves exactly one orientation that reaches --
            -- "превью применяется только если мои пальцы смотрят по направлению ствола".
            --
            -- What a player actually does is bring his PALM to the magazine, any way round, palm up. So: the catch
            -- is a ball on the magazine itself, wide enough to swallow the turning arc, and the only thing asked of
            -- the hand is which way the palm faces.
            --
            -- THE PALM NORMAL IS THE WRIST'S LOCAL -Z, and it is worth saying how that was got, because the first
            -- attempt got it wrong. Reading which wrist axis lines up with "up" during the game's own push gives
            -- +X -- but that is where the HAND POINTS, the fist's direction, not where the palm faces. The honest
            -- way is the hand's own geometry, which needs no assumption at all: take the four knuckles, fit the
            -- direction wrist -> knuckles and the direction index -> little, and their cross product IS the palm
            -- normal; the sign follows from the fingertips, which lie on the palm side when the fingers curl.
            -- Over all 280 frames of the take: along (+0.993, -0.072, +0.092) = +X, across (-0.002, +1.000,
            -- -0.030) = +Y, palm (+0.090, -0.030, -0.995) = -Z. Coherence 1.00 on all three.
            if mc.takePoint and mw1 and hmx then
                local o5, tp5 = mc.originOffset or { 0, 0, 0 }, mc.takePoint
                local m1, m2, m3, m4 = mw1, mw2, mw3, mw4
                local sr5 = mc.seatRot
                if sr5 then m1, m2, m3, m4 = qmul(m1, m2, m3, m4, sr5[1], sr5[2], sr5[3], sr5[4]) end
                local bx5, by5, bz5 = qrot(m1, m2, m3, m4, o5[1] + tp5[1], o5[2] + tp5[2], o5[3] + tp5[3])
                snapD = len3(hmx - wx - bx5, hmy - wy - by5, hmz - wz - bz5)
                snapR = mc.takeRadius or 0.09
                -- A BALL IS THE WRONG SHAPE HERE and a single radius cannot say so. The zone has to be wide across
                -- and tall -- that is what lets any orientation of the hand reach it, since the tracked point swings
                -- as the hand turns -- and at the same time SHORT along the bore, or it hangs out in front of the
                -- gun where there is nothing to take. So `takeRadius` may be three numbers instead of one, read in
                -- the weapon's own axes (across, along the bore, up): the test becomes an ellipsoid.
                local ex, ey, ez = qrot(-m1, -m2, -m3, m4,
                    hmx - wx - bx5, hmy - wy - by5, hmz - wz - bz5)
                magS.gapV = { ex, ey, ez }          -- the miss on the weapon's axes, from the zone ACTUALLY in use
                if type(mc.takeRadius) == 'table' then
                    local R5 = mc.takeRadius
                    local u5 = math.sqrt((ex / R5[1]) ^ 2 + (ey / R5[2]) ^ 2 + (ez / R5[3]) ^ 2)
                    -- expressed back as a distance against a radius, so the weight below is unchanged
                    snapR = math.max(R5[1], R5[2], R5[3])
                    snapD = u5 * snapR
                end
                if mc.takeUp then
                    local hq5 = (type(VRHandRawRot) == 'function') and VRHandRawRot(free) or nil
                    if hq5 then
                        local pa = mc.palmAxis or { 0, 0, -1 }
                        local px5, py5, pz5 = qrot(hq5.i, hq5.j, hq5.k, hq5.r, pa[1], pa[2], pa[3])
                        local ux5, uy5, uz5 = qrot(mw1, mw2, mw3, mw4, 0, 0, 1)
                        local dp5 = px5 * ux5 + py5 * uy5 + pz5 * uz5
                        magS.palmDot = dp5
                        if dp5 < mc.takeUp then snapD = -1 end
                    end
                end
            end
            -- WHERE THE HAND IS relative to the catch, on the weapon's own three axes -- across, along the bore, up
            -- -- in the same units and the same order as `previewOff`, so whatever it reads can be added to that
            -- line directly. A single distance says the catch was missed; these three say which way.
            -- ...and REPORT THE ZONE THAT IS ACTUALLY IN USE. This printed the miss from the old wrist target while
            -- the catch had already moved to the magazine's base, so the numbers described a test nothing ran.
            if hmx and mw1 then
                local g7 = magS.gapV
                if not g7 and twx then
                    g7 = { qrot(-mw1, -mw2, -mw3, mw4,
                               hmx - twx - rox, hmy - twy - roy, hmz - twz - roz) }
                end
                if g7 then
                    magS.dbg2 = (magS.dbg2 or '') ..
                        string.format(' snapD=%.3f gap=(%+.3f,%+.3f,%+.3f) palm=%.2f',
                            snapD, g7[1], g7[2], g7[3], magS.palmDot or -9)
                end
            end
            magS.gapV = nil
            -- A SEATED MAGAZINE IS NOT THERE TO BE TAKEN, unless the weapon says it is.
            --
            -- On a pistol the magazine is held by a catch and leaves on the button -- that is the only way it comes
            -- out, and reaching in with the left hand to pull one out of a latched well is a move that does not
            -- exist. On a rifle it is the opposite: the magazine is rocked out by hand, so `handPull = true` in its
            -- config keeps everything below exactly as it was.
            --
            -- KILLING `snapD` HERE, at the source, is what makes it one rule instead of three. Everything downstream
            -- reads this number: `snapW` feeds the finger preview at the well (so no ghost pose appears over a
            -- magazine that cannot be gripped), `catchW` feeds the take branch (so a squeeze there does nothing),
            -- and the published `magS.snapD` is what the SLIDE's block compares against next frame (so a magazine
            -- that is out of play can no longer win the wrist away from the slide's own grip).
            --
            -- Only the seated state is affected. A magazine in the hand still seats, a dropped one is still
            -- conjured anywhere, and the button drop below is untouched -- it is the whole point.
            --
            -- A REVOLVER IS EXEMPT AND NOT BY DEFAULT-GUESSING: `mc.eject` marks the weapons that have no magazine
            -- and no catch at all, where "it only comes out on the button" describes nothing -- the rounds leave a
            -- swung-out cylinder by gravity. Those keep their own rules in reload/revolver.lua.
            if magS.state == 'in' and not mc.handPull and not mc.eject then snapD = -1 end
            local snapW = 0.0
            if snapD >= 0 and snapD <= snapR then snapW = 1.0 - snapD / snapR end
            -- PUBLISHED for the slide's block, which runs earlier in the frame and so reads last frame's value --
            -- a frame late, and it cannot race the decision it gates (the same trick `magS.insW` uses for the roll).
            magS.snapD = snapD
            -- JITTER WATCH. Ranges over the log interval, so a shake names its own source instead of being argued
            -- about: a wide `tgt` with a narrow `raw` means the target is moving (our computation), both narrow means
            -- something else writes the wrist, and a `w` that swings means the magnet is switching on and off.
            do
                local j = magS.jit
                if not j then j = { n = 0 }; magS.jit = j end
                j.n = j.n + 1
                if snapW > 0 then j.on = (j.on or 0) + 1 end
                local function span(k, v)
                    if v == nil then return end
                    local lo, hi = j[k .. 'lo'], j[k .. 'hi']
                    if lo == nil or v < lo then j[k .. 'lo'] = v end
                    if hi == nil or v > hi then j[k .. 'hi'] = v end
                end
                span('d', snapD); span('w', snapW); span('t', twx); span('r', hmx)
            end
            -- The slide owns the free hand while it is racked or springing home; two magnets on one wrist would
            -- fight, and the slide's is the one the player is using.
            --
            -- ...AND WHILE IT IS MERELY OFFERING ITS GRIP, TOO. Guarding only on `grabbed` guards one frame too late:
            -- the hand hovering in the slide's preview zone was still inside the magazine's catch, so the squeeze
            -- meant to rack the slide pulled the magazine out of the gun instead ("где затвор он берёт магазин").
            -- The slide's block runs earlier in the frame and leaves `slideHasPose` set, which says exactly "a grip
            -- is on offer here" -- and this is the mirror of the rule already in that block, where a magazine in the
            -- hand suppresses the slide's pose. Whichever part is being offered owns the squeeze.
            -- NEAREST WINS, rather than "the slide always wins". A flat veto was enough while the two zones barely
            -- touched, but a preview radius wide enough to be comfortable overlaps the magazine outright, and then
            -- the veto trades one bug for its mirror. Both distances are already measured -- `slideS.dist` to the
            -- grip on offer, `snapD` to the magazine's own pose -- so the part the hand is actually nearer takes it.
            local slD = slideS.dist or -1
            if slideS.grabbed or slideS.releasing
               or (magS.slideHasPose and slD >= 0 and snapD >= 0 and slD < snapD) then snapW = 0.0 end
            local function magRelease(s)
                if s == nil then return end
                pcall(function() VRHandStopModel(s, false, Vector4.new(0, 0, 0, 0)) end)
                if type(VRHandStopRot) == 'function' then
                    pcall(function() VRHandStopRot(s, 0, 0, 0, 0, 1) end)
                end
            end
            local function magMagnet(side, w, ax_, ay_, az_, aq1, aq2, aq3, aq4)
                -- An explicit target overrides the grab pose. The insertion needs to aim at a point that SLIDES
                -- along the well rather than at the seat itself, and everything else still wants the seat.
                local twx, twy, twz = ax_ or twx, ay_ or twy, az_ or twz
                local tq1, tq2, tq3, tq4 = aq1 or tq1, aq2 or tq2, aq3 or tq3, aq4 or tq4
                -- NEVER PIN THE HAND THAT HOLDS THE GUN. The holder is detected per frame and it flickers; `side`
                -- follows it, so without this a bad frame writes the magnet onto the gun hand and that wrist stops
                -- turning. The magazine is only ever the free hand's business.
                if side == holder then
                    magRelease(magS.magSide); magS.magSide, magS.magnet = nil, 0
                    return
                end
                -- RELEASE THE HAND WE PINNED, not the hand we were handed this frame. Those are not always the same
                -- wrist, and the one pinned a frame ago then had nobody left to let it go -- a single flicker locked
                -- the gun hand for good, which is exactly what "the right hand stops rotating" was.
                if magS.magSide ~= nil and magS.magSide ~= side then
                    magRelease(magS.magSide); magS.magSide, magS.magnet = nil, 0
                end
                if not twx or w <= 0 then
                    if (magS.magnet or 0) > 0 then magRelease(side) end
                    magS.magnet, magS.magSide = 0, nil
                    return
                end
                local px = hmx + (twx - hmx) * w
                local py = hmy + (twy - hmy) * w
                local pz = hmz + (twz - hmz) * w
                M.ownedHand = side                 -- ours this frame: the collision solve must not also publish it
                pcall(function() VRHandStopModel(side, true, Vector4.new(px, py, pz, 1.0)) end)
                local hq = (type(VRHandRawRot) == 'function') and VRHandRawRot(side) or nil
                if hq and type(VRHandStopRot) == 'function' then
                    local d = hq.i * tq1 + hq.j * tq2 + hq.k * tq3 + hq.r * tq4
                    local s = (d < 0) and -1.0 or 1.0
                    local qi = hq.i + (tq1 * s - hq.i) * w
                    local qj = hq.j + (tq2 * s - hq.j) * w
                    local qk = hq.k + (tq3 * s - hq.k) * w
                    local qr = hq.r + (tq4 * s - hq.r) * w
                    local l = math.sqrt(qi * qi + qj * qj + qk * qk + qr * qr)
                    if l > 1e-6 then
                        pcall(function() VRHandStopRot(side, 1, qi / l, qj / l, qk / l, qr / l) end)
                    end
                end
                magS.magnet, magS.magSide = w, side
            end

            -- The magazine's SEATED pose in world space: the well slot, and the weapon's own orientation (a seated
            -- magazine is rigidly part of the gun). Used to drop one out of the gun, and it doubles as a check on the
            -- constants -- an entity spawned here has to coincide with the magazine the game draws.
            local function seatedWorld()
                local okt, f, tr = pcall(function() return slotComp:GetSlotTransform(CName.new(mc.slot)) end)
                if not (okt and f and tr) then return nil end
                local okp, p = pcall(function() return WorldPosition.ToVector4(tr.Position) end)
                local okw, wq = pcall(function() return weapon:GetWorldOrientation() end)
                if not (okp and p and okw and wq) then return nil end
                -- WHERE THE ENTITY'S ORIGIN GOES, which is not the mount for every magazine. The mount is the top of
                -- the well, and on most of these meshes the origin sits on the magazine's top face, so aiming the
                -- origin at the mount reads correctly. The Tsunami Nue's mesh has its origin in the MIDDLE of the
                -- body (Z -65.6..+65.6 against the Tamayura's -124.9..+5.9), and aiming that at the mount pushed the
                -- magazine 5-7 cm too deep into the gun -- reported exactly so, and 59.7 mm is exactly the difference
                -- between the two top faces. `originOffset` is that difference, in the magazine's own axes, and it
                -- has to apply HERE as well as in the hand or the two disagree by the same amount.
                -- ...and HOW IT IS TURNED, which is the weapon's own orientation only when the magazine's mesh
                -- happens to be built on the weapon's axes. `mc.seatRot` says otherwise, and it used to be honoured
                -- in `magWristTarget` and nowhere else -- so the magazine the hand aimed at and the magazine that
                -- fell out on B disagreed by exactly that rotation. Invisible on a magazine that looks the same
                -- either way round, which is every one of them so far; not invisible on a Kang Tao Chao, whose
                -- magazine is a 177 mm cylinder that would come out lying across the gun instead of along it.
                -- The SAME quaternion has to carry the origin shift, since `originOffset` is in magazine axes.
                local s1, s2, s3, s4 = wq.i, wq.j, wq.k, wq.r
                local sr = mc.seatRot
                if sr then s1, s2, s3, s4 = qmul(s1, s2, s3, s4, sr[1], sr[2], sr[3], sr[4]) end
                local ox, oy, oz = originShift(s1, s2, s3, s4)
                return p.x + ox, p.y + oy, p.z + oz, s1, s2, s3, s4
            end

            -- One tunable set covers every magazine in the set (they are within a centimetre of each other), but a
            -- weapon may override any single field through `mag.drop`.
            local function dcfg(k)
                local o = mc.drop
                local v = o and o[k]
                if v ~= nil then return v end
                return DROP[k]
            end
            -- The body's centre in its own axes. A magazine whose mesh origin is not on the face that enters the well
            -- declares that difference as `originOffset` (the Nue's origin is mid-body), and the placement chain adds
            -- it -- so here it comes back off, or the centre would be that far out for exactly those weapons.
            local function dropCentre()
                local c, o = dcfg('centre'), mc.originOffset
                if not o then return c end
                return { c[1] - o[1], c[2] - o[2], c[3] - o[3] }
            end

            local function dropClear()
                if not magS.dropId then return end
                local ok, e = pcall(function() return Game.FindEntityByID(magS.dropId) end)
                if ok and e then pcall(function() exEntitySpawner.Despawn(e) end)
                else magLitter[#magLitter + 1] = magS.dropId end
                magS.dropId, magS.dropP, magS.dropV, magS.dropQ, magS.dropT = nil, nil, nil, nil, nil
                magS.dropC, magS.dropW, magS.dropRest, magS.dropHits, magS.dropTouch = nil, nil, nil, nil, nil
            end

            -- WHERE A MAGAZINE ENDS UP: on its side. The THIN axis of the body goes along the surface normal, while
            -- the LONG one keeps whatever heading it had, projected into the surface. Which of the mesh's own axes
            -- those are is per weapon (`drop.long` / `drop.thin`) -- the set's magazines are built along local Z, the
            -- Chao's along local Y. The face it is already showing is the face it keeps -- picking the other one would
            -- be a visible 180 deg roll for no reason. The last stretch is played out over `settle`, not snapped.
            local function dropSettle(hp, hn)
                local q = magS.dropQ
                local LG, TH = dcfg('long'), dcfg('thin')
                local ax1, ay1, az1 = qrot(q[1], q[2], q[3], q[4], TH[1], TH[2], TH[3])
                local nx2, ny2, nz2 = hn[1], hn[2], hn[3]
                if (ax1 * nx2 + ay1 * ny2 + az1 * nz2) < 0.0 then nx2, ny2, nz2 = -nx2, -ny2, -nz2 end
                local hx2, hy2, hz2 = qrot(q[1], q[2], q[3], q[4], LG[1], LG[2], LG[3])
                local d2 = hx2 * nx2 + hy2 * ny2 + hz2 * nz2
                hx2, hy2, hz2 = hx2 - d2 * nx2, hy2 - d2 * ny2, hz2 - d2 * nz2
                local hl = len3(hx2, hy2, hz2)
                if hl < 1e-3 then                       -- standing on its end: any heading in the surface will do
                    -- the third axis, thin x long, whatever those two are on this magazine
                    hx2, hy2, hz2 = qrot(q[1], q[2], q[3], q[4],
                                         TH[2] * LG[3] - TH[3] * LG[2],
                                         TH[3] * LG[1] - TH[1] * LG[3],
                                         TH[1] * LG[2] - TH[2] * LG[1])
                    d2 = hx2 * nx2 + hy2 * ny2 + hz2 * nz2
                    hx2, hy2, hz2 = hx2 - d2 * nx2, hy2 - d2 * ny2, hz2 - d2 * nz2
                    hl = len3(hx2, hy2, hz2)
                end
                if hl < 1e-3 then return end            -- degenerate: leave it as it landed
                hx2, hy2, hz2 = hx2 / hl, hy2 / hl, hz2 / hl
                local yx, yy, yz = hy2 * nz2 - hz2 * ny2, hz2 * nx2 - hx2 * nz2, hx2 * ny2 - hy2 * nx2
                local t1, t2, t3, t4 = basisQuat(nx2, ny2, nz2, yx, yy, yz, hx2, hy2, hz2)
                local fl = dcfg('flat')
                magS.dropQ0 = { q[1], q[2], q[3], q[4] }
                magS.dropQ1 = { t1, t2, t3, t4 }
                magS.dropC0 = { magS.dropC[1], magS.dropC[2], magS.dropC[3] }
                magS.dropC1 = { hp[1] + hn[1] * fl, hp[2] + hn[2] * fl, hp[3] + hn[3] * fl }
                magS.dropRest = 0.0
                magS.dropV[1], magS.dropV[2], magS.dropV[3] = 0.0, 0.0, 0.0
                magS.dropW[1], magS.dropW[2], magS.dropW[3] = 0.0, 0.0, 0.0
            end

            -- A FALLING magazine is its OWN entity, separate from the one in the hand, and it is ticked independently
            -- of the state machine. That is what lets the player throw one away and grip a fresh one in the same
            -- breath: at most two exist at once -- one falling, one held -- and never more, because a second drop
            -- takes the first one away immediately.
            --
            -- These entities live only for the interaction: seated, the game's own magazine is on show and nothing of
            -- ours is in the world at all.
            local function dropTick()
                if not (magS.dropId and magS.dropC) then return end
                magS.dropT = (magS.dropT or 0) + dt
                -- A hitch must not carry it through the floor. The contact test IS swept, but one 200 ms frame is two
                -- metres of fall and the ray would already start on the far side of a wall.
                local h = (dt > 0.04) and 0.04 or dt
                local c, v, w, q = magS.dropC, magS.dropV, magS.dropW, magS.dropQ
                local rad, ctr = dcfg('radius'), dropCentre()

                if magS.dropRest then
                    magS.dropRest = magS.dropRest + h
                    local t = magS.dropRest / (dcfg('settle') or 0.18)
                    if t > 1.0 then t = 1.0 end
                    q = qslerp(magS.dropQ0, magS.dropQ1, t)
                    magS.dropQ = q
                    for k = 1, 3 do c[k] = magS.dropC0[k] + (magS.dropC1[k] - magS.dropC0[k]) * t end
                else
                    v[3] = v[3] - (mc.gravity or 9.81) * h
                    local dg = 1.0 - dcfg('drag') * h
                    if dg < 0.0 then dg = 0.0 end
                    v[1], v[2], v[3] = v[1] * dg, v[2] * dg, v[3] * dg
                    local sg = 1.0 - dcfg('spinAir') * h
                    if sg < 0.0 then sg = 0.0 end
                    w[1], w[2], w[3] = w[1] * sg, w[2] * sg, w[3] * sg
                    -- the tumble: q' = q + 0.5*dt*(w (x) q), w a pure quaternion in WORLD axes
                    if (w[1] * w[1] + w[2] * w[2] + w[3] * w[3]) > 1e-8 then
                        local a1, a2, a3, a4 = qmul(w[1] * 0.5 * h, w[2] * 0.5 * h, w[3] * 0.5 * h, 0.0,
                                                    q[1], q[2], q[3], q[4])
                        q = { q[1] + a1, q[2] + a2, q[3] + a3, q[4] + a4 }
                        local l = math.sqrt(q[1] * q[1] + q[2] * q[2] + q[3] * q[3] + q[4] * q[4])
                        if l > 1e-9 then q = { q[1] / l, q[2] / l, q[3] / l, q[4] / l } end
                        magS.dropQ = q
                    end
                    local n1, n2, n3 = c[1] + v[1] * h, c[2] + v[2] * h, c[3] + v[3] * h
                    local sx2, sy2, sz2 = n1 - c[1], n2 - c[2], n3 - c[3]
                    local sl = len3(sx2, sy2, sz2)
                    -- THREE PROBES ALONG THE BODY, not one sphere at its middle. A magazine is 141 mm long and 30
                    -- thick, so a single 35 mm sphere at the centre leaves both ends unguarded and they go straight
                    -- through the floor while it tumbles -- which is exactly what "вылезает за текстуру" was. The
                    -- probes sit at the centre and 50 mm either way along the LONG axis, and at 28 mm each they cover
                    -- the whole body end to end.
                    --
                    -- Each is swept from where it was to where it is going, one radius further, so a fast fall cannot
                    -- step over a floor between two frames (4 m/s is 4 cm at 90 fps).
                    local LG = dcfg('long')
                    local ax1, ay1, az1 = qrot(q[1], q[2], q[3], q[4], LG[1], LG[2], LG[3])
                    local rr = dcfg('probe')
                    local arm = dcfg('arm')
                    local fz = bz + (mc.floorLift or 0.02)
                    local touching, best = {}, nil
                    for pi = -1, 1 do
                        local ox, oy, oz = ax1 * arm * pi, ay1 * arm * pi, az1 * arm * pi
                        local px, py, pz = c[1] + ox, c[2] + oy, c[3] + oz
                        local qx, qy, qz = n1 + ox, n2 + oy, n3 + oz
                        local hp2, hn2
                        if sl > 1e-4 then
                            local kk = (sl + rr) / sl
                            hp2, hn2 = rayHit(px, py, pz, px + sx2 * kk, py + sy2 * kk, pz + sz2 * kk)
                            -- a face we are LEAVING is not a contact: that is a ray started inside geometry, or the
                            -- far side of a thin surface, and treating it as one parks the magazine in the air
                            if hp2 and (v[1] * hn2[1] + v[2] * hn2[2] + v[3] * hn2[3]) > -1e-4 then hp2 = nil end
                        end
                        if not hp2 and v[3] < 0.0 and (qz - rr) <= fz then
                            -- NOTHING FROM THE WORLD: the level floor under the player's own feet, which is all this
                            -- had before there was a raycast. Kept so a place where the query system says nothing
                            -- behaves as it always did instead of dropping the magazine for ever.
                            hp2, hn2 = { qx, qy, fz }, { 0.0, 0.0, 1.0 }
                        end
                        if hp2 then
                            -- how far this probe has to come back out along the normal to sit ON the surface
                            local pen = rr + 0.001 - ((qx - hp2[1]) * hn2[1] + (qy - hp2[2]) * hn2[2]
                                                      + (qz - hp2[3]) * hn2[3])
                            if pen > 0.0 then
                                touching[#touching + 1] = { pen = pen, hp = hp2, hn = hn2, rx = ox, ry = oy, rz = oz }
                                best = touching[#touching]
                            end
                        end
                    end
                    c[1], c[2], c[3] = n1, n2, n3
                    -- EVERY penetrating probe is answered, not just the deepest one. Resolving one and leaving the
                    -- others is what makes a long body pump itself: the single impulse torques it, the far end digs
                    -- in, and the next frame does the same the other way. Simulated end-on against a floor, one probe
                    -- per frame ran away to 49 m/s and 981 rad/s; all three settle in under a second.
                    for ci = 1, #touching do
                        local ct = touching[ci]
                        local hn, rx, ry, rz = ct.hn, ct.rx, ct.ry, ct.rz
                        -- push out only what is past a millimetre, and only 80 % of it: correcting the whole
                        -- penetration every frame is itself an energy source
                        local corr = ct.pen - 0.001
                        if corr > 0.0 then
                            c[1] = c[1] + hn[1] * corr * 0.8
                            c[2] = c[2] + hn[2] * corr * 0.8
                            c[3] = c[3] + hn[3] * corr * 0.8
                        end
                        -- A PROPER CONTACT IMPULSE, because the contact is off-centre: the velocity that matters is
                        -- the one AT the probe (v + w x r), and the impulse that answers it feeds back into both the
                        -- velocity and the spin through the lever arm. That is what makes an end-on landing cartwheel
                        -- and a flat one stop dead, with no hand-tuned "how much of this becomes spin".
                        local cvx = v[1] + (w[2] * rz - w[3] * ry)
                        local cvy = v[2] + (w[3] * rx - w[1] * rz)
                        local cvz = v[3] + (w[1] * ry - w[2] * rx)
                        local vn = cvx * hn[1] + cvy * hn[2] + cvz * hn[3]
                        if vn < 0.0 then
                            local II = dcfg('inertia')                 -- inertia per unit mass, (b^2 + c^2)/12
                            local mu = dcfg('friction')
                            -- NO BOUNCE below a walking pace. A restitution that still applies at 10 cm/s is the
                            -- classic resting-contact jitter: the body never stops arguing with the floor.
                            local e2 = (-vn > dcfg('restE')) and dcfg('bounce') or 0.0
                            local cx = ry * hn[3] - rz * hn[2]
                            local cy = rz * hn[1] - rx * hn[3]
                            local cz = rx * hn[2] - ry * hn[1]
                            local kn = 1.0 + (cx * cx + cy * cy + cz * cz) / II
                            local jn = -(1.0 + e2) * vn / kn
                            v[1] = v[1] + hn[1] * jn
                            v[2] = v[2] + hn[2] * jn
                            v[3] = v[3] + hn[3] * jn
                            w[1] = w[1] + (ry * hn[3] - rz * hn[2]) * jn / II
                            w[2] = w[2] + (rz * hn[1] - rx * hn[3]) * jn / II
                            w[3] = w[3] + (rx * hn[2] - ry * hn[1]) * jn / II
                            -- Coulomb friction on the same contact, bounded by mu*jn and by the slide itself
                            local tx2, ty2, tz2 = cvx - vn * hn[1], cvy - vn * hn[2], cvz - vn * hn[3]
                            local tl = len3(tx2, ty2, tz2)
                            if tl > 1e-6 then
                                local ux, uy, uz = tx2 / tl, ty2 / tl, tz2 / tl
                                local dx2 = ry * uz - rz * uy
                                local dy2 = rz * ux - rx * uz
                                local dz2 = rx * uy - ry * ux
                                local kt = 1.0 + (dx2 * dx2 + dy2 * dy2 + dz2 * dz2) / II
                                local jt = tl / kt
                                if jt > mu * jn then jt = mu * jn end
                                v[1] = v[1] - ux * jt
                                v[2] = v[2] - uy * jt
                                v[3] = v[3] - uz * jt
                                w[1] = w[1] - (ry * uz - rz * uy) * jt / II
                                w[2] = w[2] - (rz * ux - rx * uz) * jt / II
                                w[3] = w[3] - (rx * uy - ry * ux) * jt / II
                            end
                            local dl = 1.0 - dcfg('spinDamp')
                            if dl < 0.0 then dl = 0.0 end
                            w[1], w[2], w[3] = w[1] * dl, w[2] * dl, w[3] * dl
                            local wl = len3(w[1], w[2], w[3])
                            local wm = dcfg('maxW')
                            if wl > wm then w[1], w[2], w[3] = w[1] / wl * wm, w[2] / wl * wm, w[3] / wl * wm end
                        end
                        magS.dropHits = (magS.dropHits or 0) + 1
                    end
                    if best then
                        -- WHEN IT IS DONE. Either it has gone quiet, or it has simply been touching the ground long
                        -- enough: a magazine landing on a corner keeps a spin the solver will not shed in any sane
                        -- number of frames, and it should lie down rather than dance. Simulated, every attitude
                        -- settles between 0.54 and 0.88 s.
                        magS.dropTouch = (magS.dropTouch or 0.0) + h
                        if (len3(v[1], v[2], v[3]) < dcfg('restV') and len3(w[1], w[2], w[3]) < 3.0)
                           or (magS.dropTouch > dcfg('touchRest')) then
                            dropSettle(best.hp, best.hn)
                        end
                    end
                end
                -- the ENTITY is placed by its mesh origin; everything above moves the body's centre
                local ox2, oy2, oz2 = qrot(q[1], q[2], q[3], q[4], ctr[1], ctr[2], ctr[3])
                magS.dropP = { c[1] - ox2, c[2] - oy2, c[3] - oz2 }
                local ok, e = pcall(function() return Game.FindEntityByID(magS.dropId) end)
                if ok and e then
                    pcall(function()
                        local tr = WorldTransform.new()
                        tr:SetPosition(Vector4.new(magS.dropP[1], magS.dropP[2], magS.dropP[3], 1.0))
                        tr:SetOrientation(Quaternion.new(q[1], q[2], q[3], q[4]))
                        e:SetWorldTransform(tr)
                    end)
                end
                if magS.dropT >= (mc.litterTime or 3.0) then dropClear() end
            end
            -- Let the held magazine GO: it becomes the falling one (the id is handed over, never re-spawned), and the
            -- hand is free to take another immediately. The velocity and the spin are the HAND's, measured over the
            -- last frames of the carry in placeMag -- so a magazine thrown away flies and one merely released drops.
            local function dropRelease(px, py, pz, qi, qj, qk, qr, vx, vy, vz, wx2, wy2, wz2, ent)
                dropClear()                                   -- only ever one falling magazine
                -- WHAT FALLS IS NOT ALWAYS WHAT THE GUN GIVES UP, and the line above used to assume it was:
                -- "a magazine already in the hand has its entity, so this only spawns for a drop out of the
                -- weapon itself". That was true while everything held was a spawned entity. It is not now --
                -- a magazine on the carrier and a prop carried as an item both have no entity at all -- so a
                -- hand release fell through to `dropEntity`, and on the Overture those are two different
                -- objects: the hand brings a SPEEDLOADER (star, handle, six rounds) and the cylinder gives up
                -- six spent CASES. Letting go of the loader dropped the cases: "магазин револьвера
                -- выкидывается без держателя".
                --
                -- So the caller says which. The gun's own drop passes nothing and keeps `dropEntity`; the hand
                -- release passes what the hand was holding. On the other twelve the two are the same object
                -- and `dropEntity` is nil, which is why this never showed there.
                if not magS.entId then spawnMag(px, py, pz, qi, qj, qk, qr, ent or mc.dropEntity) end
                magS.dropId, magS.entId = magS.entId, nil
                magS.dropQ = { qi, qj, qk, qr }
                local ctr = dropCentre()
                local ox2, oy2, oz2 = qrot(qi, qj, qk, qr, ctr[1], ctr[2], ctr[3])
                magS.dropC = { px + ox2, py + oy2, pz + oz2 }
                magS.dropP = { px, py, pz }
                magS.dropV = { vx or 0.0, vy or 0.0, vz or 0.0 }
                magS.dropW = { wx2 or 0.0, wy2 or 0.0, wz2 or 0.0 }
                local vl = len3(magS.dropV[1], magS.dropV[2], magS.dropV[3])
                local vm = dcfg('maxV')
                if vl > vm then for k = 1, 3 do magS.dropV[k] = magS.dropV[k] / vl * vm end end
                local wl = len3(magS.dropW[1], magS.dropW[2], magS.dropW[3])
                local wm = dcfg('maxW')
                if wl > wm then for k = 1, 3 do magS.dropW[k] = magS.dropW[k] / wl * wm end end
                magS.dropT, magS.dropRest, magS.dropHits, magS.dropTouch = 0.0, nil, 0, 0.0
            end

            -- The finger pose, blended like the slide's: bring the hand to the magazine and it fades in as a PREVIEW.
            -- fadeOut only when this pose is the one on the hand, so the slide's preview is never cancelled by ours
            -- (an unconditional fadeOut here was the "there is no pose and no blend" report).
            -- THE HAND ROLLS THROUGH THE INSERTION, it does not switch between two grips. In the game's own
            -- animation the fingers are around the magazine on the way in and the palm is on its base at the end,
            -- and everything between is a stage of one motion. So the pose is sampled from the take at several
            -- DEPTHS and interpolated by the magazine's current depth -- the same quantity the magnet uses, so
            -- the fingers and the pull always agree about where the magazine is.
            --
            -- Depth is measured OUT of the well, so stage[1] is the deepest-out (carrying) and the last is seated.
            -- Beyond either end the nearest stage simply holds, which is what makes the carry pose the one the
            -- hand wears in the air and the seated pose the one the preview shows at the well.
            local stageCache = { key = nil, pose = nil }
            local function stagePose(depth)
                local st = mc.poseStages
                if not st or #st == 0 then return nil, nil end
                local d = (M.tune.stage >= 0) and M.tune.stage or depth
                if d == nil or d < 0 then d = 0 end
                local a, b, t = st[1], st[1], 0.0
                if d >= st[1].d then
                    a, b, t = st[1], st[1], 0.0
                elseif d <= st[#st].d then
                    a, b, t = st[#st], st[#st], 0.0
                else
                    for i = 1, #st - 1 do
                        if d <= st[i].d and d >= st[i + 1].d then
                            local span = st[i].d - st[i + 1].d
                            a, b = st[i], st[i + 1]
                            t = (span > 1e-6) and (st[i].d - d) / span or 0.0
                            break
                        end
                    end
                end
                -- quantised, so a still hand does not rebuild the table every frame
                local an, bn = vpick(a, 'pose'), vpick(b, 'pose')
                local key = string.format('mag|%s|%s|%.2f|%d', an, bn, t, M.tune.rev or 0)
                if stageCache.key == key then return stageCache.pose, key end
                local pa, pb = POSES[an], POSES[bn]
                if not pa then return nil, nil end
                local out = pa
                if pb and t > 0.005 and #pb == #pa then
                    out = {}
                    for i = 1, #pa do
                        local x, y = pa[i], pb[i]
                        local dp = x[2] * y[2] + x[3] * y[3] + x[4] * y[4] + x[5] * y[5]
                        local sgn = (dp < 0) and -1.0 or 1.0
                        local qi = x[2] + (y[2] * sgn - x[2]) * t
                        local qj = x[3] + (y[3] * sgn - x[3]) * t
                        local qk = x[4] + (y[4] * sgn - x[4]) * t
                        local qr = x[5] + (y[5] * sgn - x[5]) * t
                        local l = math.sqrt(qi * qi + qj * qj + qk * qk + qr * qr)
                        if l < 1e-6 then l = 1.0 end
                        out[i] = { x[1], qi / l, qj / l, qk / l, qr / l }
                    end
                end
                stageCache.key, stageCache.pose = key, out
                return out, key
            end
            local poseCarry = { pose = mc.pose }
            local function magFade(on, side, depth)
                if on then
                    local ps, key = stagePose(depth)
                    if ps then fadeInTable(side, key, ps) else fadeIn(side, poseCarry) end
                elseif fadeS.key == mc.pose or (fadeS.key and string.sub(fadeS.key, 1, 4) == 'mag|') then
                    fadeOut()
                end
            end
            -- Where the magazine goes: the hold slot's pose plus a CONSTANT offset and rotation in that slot's frame.
            -- Grab it from any wrist angle, at any distance, with the gun pointed anywhere -- it lands in the same
            -- place in the hand every time, because nothing here reads the hand's relation to the gun.
            --
            -- That was the bug this replaces: the rotation used to be SNAPSHOTTED off the weapon at the instant of
            -- the grab, so whatever angle the wrist happened to have relative to the gun became the magazine's pose
            -- in the hand -- "от смещения или разной позиции кисти он берётся по-разному".
            --
            -- The constants are the GAME's own in-hand placement, from the frames where its hand seats a magazine
            -- (reload_record_RELOADTRUE, t = 2.33..2.51 -- the stretch chosen by the data: the magazine holds 0.0580 m
            -- from the slot, the offset steady to 0.3 mm and the rotation to 0.32 deg; from t = 2.53 the hand leaves
            -- and those frames are out). At that instant the magazine's pose is KNOWN -- position = the mag-well slot,
            -- orientation = the weapon's own, since the chain weapon -> barrel -> mag_slot -> magazine -> the mesh's
            -- rig matrix is four times the same (0,.7071,.7071,0) and cancels in pairs.
            --
            -- `holdRotBasis` is stored against the gun's rigid BASIS, not the weapon entity, because the recording
            -- carries three slots and no weapon quaternion. The missing link is R = conj(qBasis) * qWeapon, a fixed
            -- property of the weapon asset -- identical in model or world space, since the player's rotation cancels
            -- out of it -- so it is measured live and the product is constant however the gun is held.
            local function handTarget(side)
                local sp, sq = holdSlotTr(side)
                if not sp then magS.diag = 'hold slot did not resolve'; return nil end
                if not dxb then magS.diag = 'no gun basis'; return nil end
                local okw, wq = pcall(function() return weapon:GetWorldOrientation() end)
                if not (okw and wq) then magS.diag = 'no weapon orientation'; return nil end
                local ho = (holdAt())
                local ex, ey, ez = qrot(sq.i, sq.j, sq.k, sq.r, ho[1], ho[2], ho[3])
                local tx, ty, tz = sp.x + ex, sp.y + ey, sp.z + ez
                local wmi, wmj, wmk, wmr = qmul(ci, cj, ck, cr, wq.i, wq.j, wq.k, wq.r)   -- weapon, model space
                local b1, b2, b3, b4 = basisQuat(gFx, gFy, gFz, gDx, gDy, gDz, gSx, gSy, gSz)
                local r1, r2, r3, r4 = qmul(-b1, -b2, -b3, b4, wmi, wmj, wmk, wmr)        -- R = conj(basis)*weapon
                local _, hb = holdAt()
                local k1, k2, k3, k4 = qmul(hb[1], hb[2], hb[3], hb[4], r1, r2, r3, r4)   -- slot -> magazine
                local ri, rj, rk, rr = qmul(sq.i, sq.j, sq.k, sq.r, k1, k2, k3, k4)
                do  -- the same origin correction as the seat, so the two never disagree
                    local ox, oy, oz = originShift(ri, rj, rk, rr)
                    tx, ty, tz = tx + ox, ty + oy, tz + oz
                end
                do  -- what we asked for versus where the entity actually is, and the slot itself
                    local e = magEnt()
                    local ep = nil
                    if e then
                        local okp, p2 = pcall(function() return e:GetWorldPosition() end)
                        if okp then ep = p2 end
                    end
                    magS.diag = string.format('slot(%.2f,%.2f,%.2f) want(%.2f,%.2f,%.2f) got(%s)',
                        sp.x, sp.y, sp.z, tx, ty, tz,
                        ep and string.format('%.2f,%.2f,%.2f', ep.x, ep.y, ep.z) or '-')
                end
                -- ...AND THE SAME PLACEMENT IN THE SLOT'S OWN FRAME, which is where it was measured. Everything
                -- above composes it out to the world for the entity route and for the seating test; the carrier
                -- wants it before that composition, because its parent IS the slot. So the local pair is the
                -- world pair with the two `qmul(sq, ...)` steps left off, and the origin correction turned by the
                -- local rotation instead of the world one -- not a second derivation, the same one stopped early.
                local olx, oly, olz = qrot(k1, k2, k3, k4,
                                           (mc.originOffset or { 0, 0, 0 })[1],
                                           (mc.originOffset or { 0, 0, 0 })[2],
                                           (mc.originOffset or { 0, 0, 0 })[3])
                return tx, ty, tz, ri, rj, rk, rr,
                       ho[1] + olx, ho[2] + oly, ho[3] + olz, k1, k2, k3, k4
            end

            -- No grab snapshot any more: the in-hand pose is a constant (see handTarget), so there is nothing to
            -- memorise and no way for one grab to differ from the next.

            dropTick()          -- a falling magazine keeps falling whatever the hand is doing

            -- WITH THE MAGAZINE OUT, THE GUN HOLDS NO ROUNDS -- and it has to STAY at zero. One event at the moment of
            -- the pull is not enough: the game puts rounds back on its own (a freshly drawn weapon comes with a full
            -- magazine, and its own reload logic tops one up), which is why the count did not go to zero. So it is held
            -- there for as long as no magazine is in the well, and only re-sent when it has actually drifted.
            if magS.state ~= 'in' then
                local n = magAmmo(weapon)
                if n and n > 0 then setMagAmmo(weapon, 0) end
            end

            -- THE PULL CURVE. A plain 1 - d/R is weakest exactly where it matters, which reads as a mushy magnet; a
            -- gain saturates it, so the last `R/gain` of the approach is FULL pull -- a snap you can feel -- while the
            -- edge still fades in from nothing and cannot jump.
            local gain = mc.snapGain or 1.6
            local function pullW(d, R, g)
                if not d or d < 0 or d > R then return 0.0 end
                local w = (1.0 - d / R) * (g or gain)
                return (w > 1.0) and 1.0 or w
            end

            -- SEATING, as its own step, because it does not always happen at the moment the hand lets go. On a
            -- weapon with `selfSeat` the gun takes the magazine the last stretch by itself, and everything here --
            -- the entity leaving the world, the game's own magazine coming back, the rounds, the click -- belongs at
            -- the END of that, not at the start.
            local function seatMag(H)
                -- SEATED. Everything of ours leaves the world -- the held magazine AND anything still falling --
                -- so once a magazine is in the gun there is nothing of the port's left lying around.
                despawnMag(); holdHide(); dropClear(); gunMag(true); magFade(false)
                itemDrop(0, 'AttachmentSlots.WeaponLeft'); itemDrop(1, 'AttachmentSlots.WeaponRight')
                if H then magMagnet(H, 0) end
                magS.state, magS.hand, magS.armed = 'in', nil, false
                magS.depth = 0.0
                magS.glide = nil
                -- ...and these rounds are not to be dumped again until the gun has been shut on them. Only a
                -- revolver reads it (see the crane block); on every other weapon it is set and never looked at.
                magS.freshLoad = true

                -- THE ROUNDS. The magazine that was pulled out brings back what it had; a fresh one is filled from
                -- the reserve and takes only as much as the reserve HAS, spending exactly that -- so ten in the
                -- pocket and an empty magazine leaves the pocket empty and the magazine at ten, and three leaves
                -- three.
                -- A MAGAZINE IS NOT A LOADED GUN. With the slide on its stop the chamber is empty, and the
                -- game keeps the slide back for exactly as long as it believes that -- so the rounds are held
                -- back with it and handed over when the slide is released. This is the same thing twice: the
                -- gun's own logic and the gun's own behaviour, instead of our write fighting the animation
                -- frame by frame for control of the bone.
                -- `chamberOnRack` makes that the rule for a weapon whose slide has no stop at all. The Arasaka
                -- Kenshin is one: measured across every clip of its own rig animation, its charging handle never
                -- rests anywhere but home, so there is nothing to hold back and nothing to read as "empty". A
                -- magazine going in therefore cannot be what loads it -- the handle has to be worked, which is
                -- what the weapon does in its own reload and what the player asked for here.
                if slideS.locked or mc.chamberOnRack then
                    local load = magS.carried
                    if not load then
                        load = magCapacity(weapon)
                        local have = reserveAmmo(weapon)
                        if have < load then load = have end
                        takeReserve(weapon, load)
                    end
                    magS.pending = load
                    magS.loaded = load
                elseif magS.carried then
                    setMagAmmo(weapon, magS.carried)
                else
                    local load = magCapacity(weapon)
                    local have = reserveAmmo(weapon)
                    if have < load then load = have end
                    setMagAmmo(weapon, load)
                    takeReserve(weapon, load)
                    magS.loaded = load
                end
                magS.carried = nil
                -- a magazine in a closed gun is an ordinary loaded gun again: the claim is spent and the
                -- game's own handling of the slide resumes
                slideS.userClosed = false
                playSnd(weapon, snd and snd.magIn)       -- the click
                magS.gripLock = true                     -- this squeeze is used up; let go to take another
            end

            -- WHAT THE CATCH IS MEASURED ON depends on what there is to aim at.
            --   * a magazine SEATED in the gun is a visible thing at a known place, so the catch is the small
            --     `snapRadius` measured wrist-to-pose: it must bite only right at the magazine (the user's rule).
            --   * an EMPTY well has no magazine to take, so a squeeze conjures one WHEREVER the hand is -- the player
            --     dropped his magazine and needs another, and making him find an invisible pose for it was a dead end
            --     after every drop. The hand only has to be free: if the slide has it, the slide keeps it.
            local catchW = snapW
            local takeAnywhere = false
            if magS.state == 'gone' then
                local gx, gy, gz = magFromRaw(free)          -- from the RAW hand: a weight must never feed itself
                local goneD = gx and len3(gx - wx, gy - wy, gz - wz) or -1
                catchW = pullW(goneD, mc.seatSnapRadius or 0.12)
                -- the same rule, and it matters more here: with the well empty a squeeze conjures a magazine
                -- ANYWHERE, so without this a hand on the slide of a dry gun got a magazine instead of a rack
                local slD2 = slideS.dist or -1
                local busy = slideS.grabbed or slideS.releasing
                             or (magS.slideHasPose and slD2 >= 0 and goneD >= 0 and slD2 < goneD)
                if busy then catchW = 0.0 end
                magS.insD = goneD
                takeAnywhere = not busy
            end

            if magS.state == 'in' or magS.state == 'gone' then
                -- Nothing of ours is written here, so the game keeps its own magazine and its own reload. Bring the
                -- hand in and it magnetises and the fingers fade in; grip THERE and the magazine comes out with no
                -- shift at all, because the hand is already exactly where it holds it.
                -- REACHING for a magazine is not the same wrist as PUSHING one in, on a weapon whose magazine can be
                -- held either way round. `previewRot` is that difference; without it the preview drags the wrist to
                -- the insertion roll, which on the Chao is palm-up at a moment the hand wants to be palm-down.
                -- THE PREVIEW NEVER MOVES THE WRIST. Only a held grip does -- the same rule the slide's preview
                -- follows, and now the magazine's too.
                --
                -- It used to pull: the weight that fades the fingers in also drew the hand onto the grab pose, so
                -- that gripping took the magazine "exactly as it sat" with nothing jumping. That is a real benefit
                -- and it is being given up on purpose. Having a hand dragged somewhere it did not ask to go, every
                -- time it passes near a magazine, is worse than a magazine that shifts once when it is taken -- and
                -- the shift is bounded by `snapRadius`, which is 50 mm here and can be tightened per weapon.
                --
                -- THIS IS ALL TWELVE WEAPONS, not just this one. The rule is worth more than the per-weapon feel.
                magMagnet(free, 0)
                magFade(catchW > 0, free)
                -- WHAT LETS THE ROUNDS GO. On a box-magazine gun it is the button. On a REVOLVER there is no button
                -- to press -- the cases fall out of an open cylinder when the muzzle comes up, and stay put when it
                -- does not, which is why a shooter tips one back over his palm. `dropTilt` is the cosine of the bore
                -- against straight up, and with it set the button plays no part: the crane being open and the gun
                -- being tipped is the whole trigger.
                -- THE CASES COME OUT WITH THE CYLINDER, not with the fall. Swinging a cylinder out shows its
                -- rounds; whether they then drop is a separate question, answered by gravity a moment later. So
                -- ours are put in the chambers the instant the crane is open and the game's are hidden right then,
                -- and there is no hand-off at all -- which is what the seam was: the game's vanished and ours
                -- appeared in a slightly different place, and every millimetre of that had to be chased.
                --
                -- After that the tilt does nothing but decide whether they slide, which is physics and lives in the
                -- revolver module. `dropTilt` / `dropMu` still gate the WEAPON going empty, because that is not the
                -- same event: rounds shown are not rounds lost.
                if mc.eject and REVOLVER and magS.state == 'in' then
                    local open = (hatchS.p or 0.0) > 0.9
                    -- ...AND WHAT IS IN THEM IS WHAT THE GUN STILL HAS. A cylinder does not hold six of one thing:
                    -- it holds however many rounds are left and spent cases in the rest, and the two look nothing
                    -- alike -- one has a bullet in it. So the count is read at the moment the crane opens, before
                    -- anything else touches it, and handed to the module to distribute.
                    --
                    -- NOT gated on the gun being empty, which was the previous rule and was wrong: a shooter dumps
                    -- a half-fired cylinder over his palm all the time, and that is the whole point of doing it by
                    -- hand. The one thing that must not happen is dumping the six he JUST loaded, and `freshLoad`
                    -- covers exactly that -- it holds until the crane has been shut once.
                    if open and not magS.casesOut and not magS.freshLoad then
                        local okc, fc, tc = pcall(function()
                            return slotComp:GetSlotTransform(CName.new(mc.eject.slot or 'vrp_cylinder'))
                        end)
                        if okc and fc and tc then
                            local o1, pp1 = pcall(function() return WorldPosition.ToVector4(tc.Position) end)
                            local o2, qq1 = pcall(function() return tc.Orientation end)
                            if o1 and pp1 and o2 and qq1 then
                                local live = slideS.ammo or 0
                                gunMag(false)
                                REVOLVER.eject({ cfg = mc, spawner = exEntitySpawner },
                                               { x = pp1.x, y = pp1.y, z = pp1.z,
                                                 qi = qq1.i, qj = qq1.j, qk = qq1.k, qr = qq1.r }, live)
                                magS.casesOut = true
                            end
                        end
                    elseif (not open) and magS.casesOut then
                        -- shut again before everything fell out: the gun keeps the LIVE ones that are still in
                        -- their chambers -- not the spent cases, which were never ammunition, and not the live ones
                        -- that already dropped, which are on the floor.
                        local back = REVOLVER.heldLive()
                        REVOLVER.clear({ spawner = exEntitySpawner })
                        gunMag(true)
                        magS.casesOut = false
                        -- ...and the gun keeps EXACTLY what is still in its chambers. Only ever downwards, which
                        -- is a plain consume: with nothing fallen this is `n -> n` and does nothing at all. Nothing
                        -- is ever handed back up, because nothing is ever taken -- the trigger is stopped at the
                        -- source now (see the hammer block) instead of the magazine being emptied behind the game's
                        -- back, which is what quietly cost six rounds out of the pocket per crane cycle.
                        setMagAmmo(weapon, back)
                    end
                    -- ...and the gun is empty only when the last one has actually left its chamber
                    if magS.casesOut and REVOLVER.spent() then
                        magS.state, magS.hand, magS.carried = 'gone', nil, nil
                        setMagAmmo(weapon, 0)
                        playSnd(weapon, snd and snd.magOut)
                        magS.casesOut = false
                    end
                    -- A FRESH SIX IS SAFE UNTIL THE CRANE HAS BEEN SHUT. Loading happens with the cylinder open, so
                    -- without this the rounds seated a moment ago are spawned straight back out and dropped.
                    if not open then magS.freshLoad = false end
                end

                -- THE THRESHOLD IS COMPUTED, not chosen. A case slides out of its chamber when the axial part of
                -- gravity beats the friction the radial part provides -- theta < atan(1/mu) -- which at mu = 0.25
                -- is 76 deg from vertical, the bore about 14 deg above level. It gates nothing here any more on a
                -- weapon with an `eject` section; the sliding itself is where it acts.
                local tipped = false
                local thr = mc.dropTilt
                if mc.dropMu and REVOLVER then thr = REVOLVER.tiltThreshold(mc.dropMu) end
                if thr and not mc.eject then
                    tipped = (hatchS.p or 0.0) > 0.9 and az >= thr
                end
                if (btnEdge and not thr or tipped) and magS.state == 'in' then
                    -- B: the magazine leaves the gun and falls, like the flat game's mag-drop. The hand is not
                    -- involved, so a fresh one can be gripped while this one is still in the air.
                    local sx3, sy3, sz3, s1, s2, s3, s4 = seatedWorld()
                    if sx3 then
                        gunMag(false)
                        -- Out of the WELL, not out of a hand: no hand velocity to inherit, so it leaves along the
                        -- well itself -- `wellAxis` is the measured line the game's own insertion runs on, and its
                        -- positive direction is out of the gun. Straight world-down would be wrong the moment the
                        -- pistol is held at an angle, which in VR it always is.
                        -- HOW HARD IT LEAVES, and IN WHICH SPACE. 0.35 m/s was gone in a tenth of a second -- gravity
                        -- owns the magazine before the eye can see which way it went -- so 1.1 m/s, which keeps the
                        -- well's own direction for about 20 cm.
                        --
                        -- THE DIRECTION IS BUILT IN WORLD SPACE, from the weapon's world orientation. It used to be
                        -- built from `gFx..gSz`, and those are MODEL space -- the player's own frame -- while the fall
                        -- integrates in world (its gravity is world -Z). The two differ by the player's rotation, so
                        -- the magazine left along a direction turned by however the player happened to be facing:
                        -- "я держу пистолет в наклоне и он улетает как-то под углом назад".
                        local push = mc.dropPush or 1.1
                        local kx, ky, kz = 0.0, 0.0, -push
                        local wa0 = mc.wellAxis
                        local okq0, wq0 = pcall(function() return weapon:GetWorldOrientation() end)
                        if wa0 and okq0 and wq0 then
                            local ux, uy, uz = qrot(wq0.i, wq0.j, wq0.k, wq0.r, wa0[1], wa0[2], wa0[3])
                            kx, ky, kz = ux * push, uy * push, uz * push
                        end
                        if mc.eject and REVOLVER then
                            -- SIX BODIES, each out of its own chamber. The cylinder's slot says where they start
                            -- and which way they leave: it rides the crane, so it already carries however far the
                            -- cylinder has swung out and however far it has been turned by hand.
                            local okc, fc, tc = pcall(function()
                                return slotComp:GetSlotTransform(CName.new(mc.eject.slot or 'vrp_cylinder'))
                            end)
                            local cp, cq
                            if okc and fc and tc then
                                local o1, pp1 = pcall(function() return WorldPosition.ToVector4(tc.Position) end)
                                local o2, qq1 = pcall(function() return tc.Orientation end)
                                if o1 and pp1 and o2 and qq1 then cp, cq = pp1, qq1 end
                            end
                            if cp then
                                REVOLVER.eject({ cfg = mc, spawner = exEntitySpawner },
                                               { x = cp.x, y = cp.y, z = cp.z,
                                                 qi = cq.i, qj = cq.j, qk = cq.k, qr = cq.r })
                            end
                        else
                            dropRelease(sx3, sy3, sz3, s1, s2, s3, s4, kx, ky, kz)
                        end
                        magS.state, magS.hand, magS.carried = 'gone', nil, nil
                        setMagAmmo(weapon, 0)                -- dropped: the gun is empty and those rounds are gone
                        playSnd(weapon, snd and snd.magOut)
                    end
                elseif M.tune.hold and magS.state ~= 'hand' then
                    -- the overlay asked for a magazine in the hand: no reach, no squeeze, just hold one. The same
                    -- three routes as the real grab below, or the tuner would be judging a placement nobody uses.
                    local tx, ty, tz, qi, qj, qk, qr = handTarget(free)
                    local slotT = (free == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight'
                    local tookT = tx and (mc.mesh ~= nil) and holdShow(free, mc.mesh, magSkin())
                    if tookT then magS.route = 'carrier'
                    elseif tx and mc.item and itemHold(free, mc.item, slotT) then
                        tookT, magS.route = true, 'item'
                    end
                    if tx and (tookT or spawnMag(tx, ty, tz, qi, qj, qk, qr)) then
                        if not tookT then magS.route = 'entity' end
                        magS.state, magS.hand, magS.armed, magS.carried = 'hand', free, false, nil
                        magFade(true, free, 1.0)
                    end
                elseif gripTake and (catchW > 0 or takeAnywhere) then
                    local tx, ty, tz, qi, qj, qk, qr = handTarget(free)
                    -- THE CARRIER FIRST, THEN THE ITEM, AND THE SPAWNER ONLY FOR WHAT NEITHER CAN DRAW. Two
                    -- lines of a weapon's config decide it, and nothing else in the module has to know: `mag.mesh`
                    -- is one mesh and rides the player's own carrier component; `mag.item` is a many-part prop and
                    -- is carried by the game as a real item. Both are exact; only the spawned entity is a frame
                    -- behind, and that is now the fallback rather than the rule.
                    --
                    -- ...and only once there is somewhere to put it: turning either on before the target resolved
                    -- would leave a magazine drawn in the hand with no state behind it.
                    local took = tx and (mc.mesh ~= nil) and holdShow(free, mc.mesh, magSkin())
                    if took then magS.route = 'carrier' end
                    if tx and not took and mc.item then
                        took = itemHold(free, mc.item, (free == 0) and 'AttachmentSlots.WeaponLeft'
                                                                   or 'AttachmentSlots.WeaponRight')
                        if took then magS.route = 'item' end
                    end
                    if tx and (took or spawnMag(tx, ty, tz, qi, qj, qk, qr)) then
                        -- WHICH ROUTE CARRIED IT, remembered rather than inferred. The placement markers below
                        -- only print while something is in the hand, so a grab that fell through to the spawner
                        -- left nothing in the log to say so -- which is exactly the question that had to be
                        -- answered by looking at the magazine instead of reading a number.
                        if not took then magS.route = 'entity' end
                        -- out of the WELL, or a fresh one out of nowhere: different sounds, and the game has both
                        playSnd(weapon, (magS.state == 'in') and (snd and snd.magOut) or (snd and snd.magGrab))
                        -- AMMO. Pulling the magazine out empties the GUN that instant -- which is what the HUD should
                        -- read -- and the rounds that were in it travel with the magazine, so putting that same one
                        -- back returns exactly them. A FRESH magazine carries nothing yet: it fills from the reserve
                        -- at the moment it seats.
                        if magS.state == 'in' then
                            -- AN EMPTY MAGAZINE IS SPENT, not carried. `carried = 0` is a number, and every number is
                            -- TRUE in Lua, so the seating branch below took it and loaded nothing: pull the dry
                            -- magazine out of a dry gun, push one back, and the gun stayed at zero ("патроны почему-то
                            -- не добавляются"). Arguably it was right -- that magazine WAS empty -- but in VR there is
                            -- no shelf of individual magazines to swap between, so a dry one is done with and the next
                            -- one seated comes full out of the reserve, which is what reloading means.
                            local had = magAmmo(weapon) or 0
                            magS.carried = (had > 0) and had or nil
                            setMagAmmo(weapon, 0)
                        else
                            magS.carried = nil
                        end
                        gunMag(false)                        -- the gun's magazine is out
                        -- A MAGAZINE THAT HAS JUST COME INTO THE HAND IS BEING CARRIED, so start it on the CARRY
                        -- stage. `magS.depth` is last frame's by design, and after a seat it is 0.0 -- so the first
                        -- frame of a fresh grab wore the SEATED pose and the magazine appeared turned along the
                        -- fingers for exactly one frame before correcting itself. A depth past the far stage clamps
                        -- to the carry one, which is what a magazine in the air should wear.
                        magS.depth = 1.0
                        -- WHICH WAY THIS ONE IS HELD. A weapon may describe more than one carry -- `off2/rot2/pose2`
                        -- on a stage -- and the choice is made ONCE, here, not per frame: a hand that re-picked every
                        -- frame would shake, and the same reload should look the same all the way through.
                        magS.variant = (mc.variantByHand
                                        and ((magS.state == 'in' and pickVariant(free)) or 1))
                                       or ((math.random(2) == 2) and 2 or 1)
                        -- ...and which way up, if this magazine can be held both ways
                        magS.roll, magS.insW = pickRoll(free), 0.0
                        magS.state, magS.hand = 'hand', free
                        magS.armed = false                   -- the well cannot pull until this has left the well
                        magFade(true, free)
                    end
                end
            elseif magS.state == 'hand' then
                -- handTarget writes the slot / wanted / actual triple into magS.diag, so the log shows on its own
                -- whether the entity is tracking the slot
                local H = magS.hand or free
                -- STILL FREE TO TURN IT OVER. Until the magnet has hold of the hand, a player who rolls the magazine
                -- in his fist should be followed, not fought -- but the choice must FREEZE the moment the pull
                -- starts, or the wrist target would jump 180 deg in the middle of an insertion. Last frame's weight
                -- is the test, which is a frame late and cannot race the pull it is gating.
                if (mc.rollAlt or mc.rollSym or mc.rollWays) and (magS.insW or 0) <= 0 then
                    magS.roll = pickRoll(H)
                end
                -- READ THE SQUEEZE FROM THE HAND THAT HOLDS IT. `grip` above is sampled on `free`, and the two are
                -- not always the same wrist -- one flickered holder frame and the magazine was released because the
                -- OTHER hand happened not to be squeezing.
                grip = gripOf(H)
                local tx, ty, tz, qi, qj, qk, qr, lx, ly, lz, k1, k2, k3, k4 = handTarget(H)
                if tx then placeMag(tx, ty, tz, qi, qj, qk, qr, H, lx, ly, lz, k1, k2, k3, k4) end
                magS.hand = H
                -- THE WELL'S MAGNET, along the well rather than at a point.
                --
                -- Pulling the hand at the SEAT is what left the magazine hovering outside, to be finished off by
                -- hand: from anywhere but dead in line the shortest way to the seat runs through the gun. A well is
                -- a slot, so the magnet takes the magazine's offset from the seat apart -- how far ALONG the well it
                -- still is, and how far OFF its centreline -- and pulls out only the second. The hand is put on the
                -- line and left free to push in, which is what seating a magazine feels like.
                --
                -- `wellAxis` is MEASURED, not chosen: the game's own insertion runs 115 mm along one straight line
                -- in the weapon's frame, worst deviation 1.6 deg (magazine rig bone 1, whose parent is the mount).
                --
                -- ARMED only once the magazine has actually LEFT the well, or the magnet holds it in the gun at the
                -- instant of the grab and re-seats it at once.
                local mrx, mry, mrz = magFromRaw(H, true)     -- BASE relation: see magFromRaw's note on the loop
                local insR = mc.seatSnapRadius or 0.12
                local run  = mc.insertRun or 0.115
                local awx, awy, awz = nil, nil, nil
                local wa = mc.wellAxis
                if wa and gFx then
                    awx = gFx * wa[1] + gDx * wa[2] + gSx * wa[3]
                    awy = gFy * wa[1] + gDy * wa[2] + gSy * wa[3]
                    awz = gFz * wa[1] + gDz * wa[2] + gSz * wa[3]
                end
                local depth, perp, insW = -1, -1, 0.0
                local gx2, gy2, gz2 = nil, nil, nil
                if mrx and awx then
                    local vx2, vy2, vz2 = mrx - wx, mry - wy, mrz - wz
                    depth = vx2 * awx + vy2 * awy + vz2 * awz          -- + = still out of the well
                    perp  = len3(vx2 - depth * awx, vy2 - depth * awy, vz2 - depth * awz)
                    if depth > (mc.armDist or insR) then magS.armed = true end
                    local d2 = (depth > 0) and depth or 0
                    -- the seating wrist pose, slid back out along the well by however deep the magazine is now:
                    -- correcting to THAT leaves the push free and fixes only the sideways error
                    if twx then gx2, gy2, gz2 = twx + awx * d2, twy + awy * d2, twz + awz * d2 end
                    -- ONCE CAUGHT, IT KEEPS HOLD. A pull that fades linearly to nothing at the edge lets the
                    -- magazine slip out of the well from a small sideways move -- the weight drops, the magazine
                    -- drifts further, the weight drops again. So the catch is narrow but the RELEASE is wide: it
                    -- takes the magazine properly off the line to lose it, the same latch the slide's grip uses.
                    -- HOW FAR OUT THE MAGNET MAY REACH along the well. 1.6 times the measured run is generous on
                    -- purpose -- most of these weapons are approached from anywhere and the pull has to find the
                    -- hand -- but on a weapon whose magazine slides in along its own length that same window puts
                    -- the catch a whole magazine clear of the gun, and it grabs while the player is still lining up.
                    -- `magnetDepth` is the depth in metres past which it simply does not act.
                    local mdep = mc.magnetDepth or (run * 1.6)
                    if magS.armed and depth >= -0.02 and depth <= mdep then
                        local R = magS.inWell and (insR * (mc.holdWiden or 2.2)) or insR
                        insW = pullW(perp, R, mc.insertGain)
                        magS.inWell = insW > 0
                    else
                        magS.inWell = false
                    end
                elseif mrx then
                    -- no measured axis for this weapon: the old pull straight at the seat
                    depth = len3(mrx - wx, mry - wy, mrz - wz)
                    perp = depth
                    if depth > (mc.armDist or insR) then magS.armed = true end
                    insW = magS.armed and pullW(depth, insR) or 0.0
                end
                if slideS.grabbed or slideS.releasing then insW = 0.0 end
                magS.insW = insW                     -- read at the top of the next frame, to freeze the roll choice
                -- ONE depth for the fingers AND for the in-hand placement. They were taking it by different
                -- rules -- the fingers clamped to the carry stage whenever the magnet was off, the placement used
                -- the raw projection on the well's axis, which out in the open is whatever the arm happens to be
                -- doing. So the hand wore the carry grip while the magazine was placed at the seated relation,
                -- 8 cm further out: it sat on the forearm instead of the palm.
                local stageD = (insW > 0 and awx and depth >= 0) and depth or 1.0
                magS.insD, magS.perp, magS.depth = depth, perp, stageD
                -- THE STAGES ONLY RUN WHILE THE MAGNET DOES. Depth is a projection on the well's axis, and out in
                -- the open that projection changes with every move of the arm even though the magazine is nowhere
                -- near the gun -- the fingers drifted through the whole insertion just from carrying it about.
                -- Off the well the hand holds the carry stage: a depth past the far end clamps to it.
                magFade(true, H, stageD)
                magMagnet(H, insW, gx2, gy2, gz2)
                -- CLICKING IN is judged on the magazine the player SEES -- the drawn one, which the magnet has already
                -- pulled home -- while the pull itself stays on the raw hand. Judging it on the raw hand meant the
                -- magazine could sit visibly in the well and refuse to seat until the real hand was 3 cm from a pose
                -- nobody can see: the magnet did the work and got no credit, which is what felt unfinished.
                local sx4, sy4, sz4 = seatedWorld()
                local seenD = (tx and sx4) and len3(tx - sx4, ty - sy4, tz - sz4) or -1
                magS.seenD = seenD
                -- CLICKS AT A DEPTH, not within a radius of the seat: `seatAt` of the way in and the magazine is
                -- caught and the hand let go -- how a magazine behaves, and what was asked for. Weapons with no
                -- measured well axis keep the old radius test.
                local seatNow
                if awx and depth >= 0 then
                    -- HOW DEEP IT HAS TO GO before it clicks in. `seatAt` is a fraction of the measured run, and
                    -- the tuner can override it live (M.tune.seat >= 0) because "too deep" is a judgement made by
                    -- looking, not a quantity in the recording -- the animation's hand lets go at a depth its own
                    -- magazine mesh happens to reach, and a different mesh reads differently at the same number.
                    local seatFrac = mc.seatAt or 0.70
                    if (M.tune and (M.tune.seat or -1) >= 0) then seatFrac = M.tune.seat end
                    seatNow = (depth <= (1.0 - seatFrac) * run) and (perp <= insR)
                else
                    seatNow = (seenD >= 0 and seenD <= (mc.seatRadius or 0.03))
                end
                if magS.armed and grip and seatNow then
                    -- THE GUN MAY TAKE IT FROM HERE. On the Kang Tao Chao the game's own hand lets go 58 mm short
                    -- and the magazine covers the rest by itself -- so seating it on the spot is a teleport, which
                    -- is exactly what it looked like. With `selfSeat` the hand is freed at once and the magazine
                    -- keeps going, under nobody's control, until it is home.
                    -- `a and f() or nil` TRUNCATES: an `and`/`or` expression yields exactly one value, so all
                    -- seven of seatedWorld's came back as the first plus six nils, and the next line did arithmetic
                    -- on one of them. Every frame, at the exact moment of seating -- so the magazine never latched
                    -- and whatever the wrist had been told last stood. A plain `if` is the only safe way to make a
                    -- multi-value call conditional.
                    local gx, gy, gz, g1, g2, g3, g4
                    if mc.selfSeat then gx, gy, gz, g1, g2, g3, g4 = seatedWorld() end
                    local e0 = gx and magEnt() or nil
                    if e0 then
                        local okp, ep = pcall(function() return e0:GetWorldPosition() end)
                        local okq, eq = pcall(function() return e0:GetWorldOrientation() end)
                        magFade(false)
                        magMagnet(H, 0)
                        -- WHERE IT STARTS FROM, in the GUN's frame and not the world's. The glide lasts a tenth of a
                        -- second and the gun is in a moving hand for all of it, so a start point latched in world
                        -- space would have the magazine sliding towards where the well USED to be.
                        local px5 = (okp and ep) and { ep.x, ep.y, ep.z } or { tx, ty, tz }
                        local pq5 = (okq and eq) and { eq.i, eq.j, eq.k, eq.r } or { qi, qj, qk, qr }
                        local dx5, dy5, dz5 = qrot(-g1, -g2, -g3, g4, px5[1] - gx, px5[2] - gy, px5[3] - gz)
                        local r1, r2, r3, r4 = qmul(-g1, -g2, -g3, g4, pq5[1], pq5[2], pq5[3], pq5[4])
                        magS.glide = { t = 0.0, d0 = { dx5, dy5, dz5 }, q0 = { r1, r2, r3, r4 } }
                        leadReset('glide')       -- a glide starts where it starts; nothing stepped to get there
                        magS.state, magS.hand, magS.armed = 'glide', nil, false
                        magS.gripLock = true                 -- this squeeze is used up; let go to take another
                    else
                        seatMag(H)
                    end
                elseif not grip and not M.tune.hold then
                    -- let go: it falls from where the hand had it, and the hand is free at once. The carrier
                    -- stops drawing it here and `dropRelease` finds no entity, so it spawns the falling body at
                    -- exactly this point -- the handover needs no extra step, it was already written that way.
                    holdHide()
                    itemDrop(H, (H == 0) and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight')
                    magFade(false)
                    magMagnet(H, 0)
                    local px, py, pz, q1, q2, q3, q4 = tx, ty, tz, qi, qj, qk, qr
                    if not px then
                        local e = magEnt()
                        local okp, ep = false, nil
                        if e then okp, ep = pcall(function() return e:GetWorldPosition() end) end
                        if okp and ep then px, py, pz, q1, q2, q3, q4 = ep.x, ep.y, ep.z, 0, 0, 0, 1 end
                    end
                    if px then
                        local hv = magS.hold
                        -- ...and what falls is what the hand was holding, named rather than assumed: on the
                        -- Overture the loader and the cases are different objects (see dropRelease).
                        dropRelease(px, py, pz, q1, q2, q3, q4,
                                    hv and hv.vx, hv and hv.vy, hv and hv.vz,
                                    hv and hv.wx, hv and hv.wy, hv and hv.wz,
                                    mc.entity)
                    else despawnMag() end
                    magS.state, magS.hand, magS.armed, magS.carried = 'gone', nil, false, nil
                    magS.depth = 0.0
                end
            elseif magS.state == 'glide' then
                -- THE GUN TAKING ITS OWN MAGAZINE. Nobody is holding this one: it runs from where the hand let go
                -- to the seat by itself, and only on arrival does the game's magazine come back, the rounds go in
                -- and the click play. Measured on the Chao, whose animation does exactly this: the hand opens
                -- 58 mm short and the magazine covers those 58 mm in 0.11 s.
                --
                -- Both ends are read in the GUN's frame every frame -- `seatedWorld` fresh, the start as the offset
                -- latched at release -- so the whole move is rigid to the weapon however the player waves it about.
                local g = magS.glide
                local sx5, sy5, sz5, s1, s2, s3, s4 = seatedWorld()
                if not (g and sx5) then
                    seatMag(nil)
                else
                    local T = (type(mc.selfSeat) == 'table' and mc.selfSeat.time) or 0.11
                    g.t = g.t + dt
                    local f = g.t / ((T > 1e-3) and T or 1e-3)
                    if f > 1.0 then f = 1.0 end
                    -- eased out: a magazine pulled home by a catch arrives, it does not stop dead
                    local e5 = 1.0 - (1.0 - f) * (1.0 - f)
                    local k5 = 1.0 - e5
                    local ox5, oy5, oz5 = qrot(s1, s2, s3, s4, g.d0[1] * k5, g.d0[2] * k5, g.d0[3] * k5)
                    local qq = qslerp(g.q0, { 0.0, 0.0, 0.0, 1.0 }, e5)
                    local w1, w2, w3, w4 = qmul(s1, s2, s3, s4, qq[1], qq[2], qq[3], qq[4])
                    local e5e = magEnt()
                    if e5e then
                        -- the same correction on its own key: the seat is read from the gun's slot, so a glide
                        -- crossing it while the player walks trails exactly as a held magazine does
                        local ga, gb, gc = freshDelta(bx, by, bz)
                        local gx6, gy6, gz6 = leadWorld('glide', sx5 + ox5 + ga, sy5 + oy5 + gb, sz5 + oz5 + gc)
                        pcall(function()
                            local tr = WorldTransform.new()
                            tr:SetPosition(Vector4.new(gx6, gy6, gz6, 1.0))
                            tr:SetOrientation(Quaternion.new(w1, w2, w3, w4))
                            e5e:SetWorldTransform(tr)
                        end)
                    end
                    magS.depth = len3(g.d0[1] * k5, g.d0[2] * k5, g.d0[3] * k5)
                    if f >= 1.0 then seatMag(nil) end
                end
            end
        end
    end

    -- ------------------------------------------------------------- ROTATOR (the disc on the slide)
    -- A quick ~180 deg flourish once the slide comes home, matching the reload animation (rotator spins near the
    -- end). sin() sweeps the angle 0 -> spinAngle -> 0 over spinTime, then control returns to the game.
    local rc = cfg.rotator
    if rc and rotS.t >= 0 then
        rotS.t = rotS.t + dt
        local frac = rotS.t / (rc.spinTime or 0.28)
        if frac >= 1.0 then
            rotS.t = -1
            VRRigWriteRot(rc.which, rc.bone, 0.0, rc.spinAxis[1], rc.spinAxis[2], rc.spinAxis[3])
        else
            local ang = (rc.spinAngle or 180.0) * math.sin(math.pi * frac)
            VRRigWriteRot(rc.which, rc.bone, ang, rc.spinAxis[1], rc.spinAxis[2], rc.spinAxis[3])
        end
    end

    -- ------------------------------------------------------------- THE HATCH (a cover that follows the magazine)
    --
    -- Some weapons do not expose their magazine until something moves out of the way. The Kang Tao Chao is the first
    -- here: its whole upper section (`magazine_open`) swings 90 deg about the bore, and until it does there is no
    -- opening for a magazine to pass through. The game opens it as part of its reload CLIP -- which the physical
    -- reload never plays -- so without this the magazine would travel through a shut cover.
    --
    -- WHAT DRIVES IT is the magazine's own state, not a hand and not a timer: open whenever the magazine is anywhere
    -- but home, shut the moment it seats. That is exactly what the animation does, only keyed to the player instead
    -- of to a clip -- and it means it opens whether the magazine was dropped on B, pulled out by hand, or a fresh one
    -- was conjured for an empty gun.
    --
    -- The SHAPE of the swing is the animation's own, measured off `magazine_open` in `empty_reload`: it rocks about
    -- 6 deg the WRONG way over the first two frames of 0.167 s, then swings to 90 in the remaining 0.1. That
    -- wind-up is most of why a powered cover reads as powered rather than as a rotating number.
    --
    -- THE ANGLE IS A PURE FUNCTION OF THE PROGRESS, never of the direction. Branching the curve on "opening" versus
    -- "closing" looks harmless and is not: grab a fresh magazine while the cover is half shut and the same progress
    -- suddenly means a different angle, so the cover jumps. One curve, read forwards or backwards, cannot do that --
    -- and it gives the closing swing a small rebound off the shut position for free, which the animation also has.
    local hc = cfg.hatch
    if hc then
        -- WHAT DECIDES IT. By default the magazine's own state: open whenever the magazine is anywhere but home,
        -- which is what a cover over a magazine well does. A REVOLVER's crane is not that -- the cylinder swings out
        -- because the shooter pushes the catch, and it stays out until he shuts it, with or without a speedloader in
        -- the gun. `manual` makes B a toggle for it, tracked here rather than in the magazine's block because the
        -- crane is the gun's own part and has nothing to do with whether the magazine is in.
        local want = (magS.state ~= 'in') and 1.0 or 0.0
        if hc.manual then
            local b2 = (GetVRSharedSlot(157) or 0.0) > 0.5
            if b2 and not hatchS.btnPrev then
                hatchS.open = not hatchS.open
                -- on the EDGE, not on arrival: a crane is heard leaving the frame, not reaching the end of its swing
                playSnd(weapon, snd and (hatchS.open and snd.hatchOpen or snd.hatchShut))
            end
            hatchS.btnPrev = b2

            -- ...AND A FLICK OF THE WRIST SHUTS IT, which is how a revolver is really closed: the gun is snapped
            -- sideways and the cylinder's own inertia carries it home against the latch. The button is still there
            -- for anyone who would rather press it.
            --
            -- MEASURED IN MODEL SPACE, and that is not a detail: the weapon's WORLD velocity carries the player's own
            -- locomotion, and a walk is already 1.5 m/s -- the cylinder would slam shut every time he broke into a
            -- run. Model space is the player's own frame, so only the hand's motion relative to him counts.
            --
            -- The direction taken is ACROSS THE BORE (the weapon's own X), because that is the axis the crane swings
            -- about and the only one a sideways snap loads. Either way round counts: a flick is a flick, and which
            -- hand the gun is in decides which way it reads.
            --
            -- `flickArm` is a dead time after the crane opens. Without it the same motion that swung the gun out to
            -- look inside shuts it again on the next frame.
            hatchS.openT = hatchS.open and ((hatchS.openT or 0.0) + dt) or 0.0
            if hatchS.open and hc.flickSpeed and gFx then
                local hfx, hfy, hfz = handModel(holder)
                if hfx and hatchS.hp and dt > 1e-4 then
                    local vx6 = (hfx - hatchS.hp[1]) / dt
                    local vy6 = (hfy - hatchS.hp[2]) / dt
                    local vz6 = (hfz - hatchS.hp[3]) / dt
                    local lat = math.abs(vx6 * gFx + vy6 * gFy + vz6 * gFz)
                    -- the PEAK since the last log line, not the instant: a flick lasts two or three frames, and a
                    -- sample taken every few seconds will never once land on one
                    if lat > (hatchS.lat or 0.0) then hatchS.lat = lat end
                    if hatchS.openT > (hc.flickArm or 0.30) and lat >= hc.flickSpeed then
                        hatchS.open = false
                        playSnd(weapon, snd and snd.hatchShut)
                    end
                end
                if hfx then hatchS.hp = { hfx, hfy, hfz } else hatchS.hp = nil end
            else
                hatchS.hp = nil
            end
            want = hatchS.open and 1.0 or 0.0
        end
        local p = hatchS.p or 0.0
        if p ~= want then
            local T = (want > p) and (hc.openTime or 0.17) or (hc.closeTime or 0.13)
            local step = dt / ((T > 1e-3) and T or 1e-3)
            p = (want > p) and math.min(1.0, p + step) or math.max(0.0, p - step)
            hatchS.p = p
        end
        local A, wu = hc.angle or 90.0, hc.windUp or 0.0
        local ang
        if p < 0.20 then       ang = -wu * (p / 0.20)                     -- winding up
        elseif p < 0.40 then   ang = -wu                                  -- held, while the latch lets go
        else
            -- and away: FAST off the latch, easing into the stop. A straight ramp reads as a servo; the sine is what
            -- the animation actually does -- measured, the swing is half over in the first third of its time
            -- (-6 -> +42 -> +90), and sin(pi/2 * f) reproduces those three keys to 0.3 deg.
            ang = -wu + (A + wu) * math.sin(math.pi * 0.5 * ((p - 0.40) / 0.60))
        end
        if p <= 0.0 then
            if hatchS.wrote then
                VRRigWriteRot(hc.which, hc.bone, 0.0, hc.axis[1], hc.axis[2], hc.axis[3])  -- angle 0 = hand it back
                hatchS.wrote = false
            end
        else
            VRRigWriteRot(hc.which, hc.bone, ang * (hc.sign or 1.0), hc.axis[1], hc.axis[2], hc.axis[3])
            hatchS.wrote = true
        end
    end

    if REVOLVER then
        -- the cylinder's live pose goes with the tick: a case still in its chamber rides the gun
        local cyl = nil
        if cfg.eject or (cfg.mag and cfg.mag.eject) then
            local ec2 = (cfg.mag and cfg.mag.eject) or cfg.eject
            local okc2, fc2, tc2 = pcall(function()
                return slotComp:GetSlotTransform(CName.new(ec2.slot or 'vrp_cylinder'))
            end)
            if okc2 and fc2 and tc2 then
                local o3, q3 = pcall(function() return tc2.Orientation end)
                local o4, p3 = pcall(function() return WorldPosition.ToVector4(tc2.Position) end)
                if o3 and q3 and o4 and p3 then
                    cyl = { x = p3.x, y = p3.y, z = p3.z, qi = q3.i, qj = q3.j, qk = q3.k, qr = q3.r }
                end
            end
        end
        REVOLVER.tick({ dt = dt, spawner = exEntitySpawner, cyl = cyl })
    end

    -- ----------------------------------------------------------------- THE CYLINDER, TURNED BY FINGERS
    --
    -- Only while the crane is OUT. Swung into the frame a cylinder is locked by the bolt, and turning one there
    -- would be turning a part that cannot move.
    --
    -- The axis is not the weapon's and cannot be taken from it: the cylinder rides the crane, so once that is open
    -- its axis has swung 100 deg away from anything the gun's own frame knows. It is read from the SLOT instead --
    -- `vrp_cylinder` sits on the bone, so its orientation IS the cylinder's, whatever the crane has done with it.
    --
    -- What the hand contributes is the angle it sweeps AROUND that axis, not how far it moves: a finger laid on the
    -- cylinder and dragged across turns it, and the same finger moved along the axis does nothing. So the hand is
    -- projected into the cylinder's own plane and only its bearing is taken.
    -- SWUNG OUT, OR SHUT, IF THE WEAPON SAYS SO. A cylinder in the frame is held by the bolt on a real revolver,
    -- which is why this began as open-only -- but a hand laid on it and turned is a gesture a player expects to do
    -- whenever he can see the cylinder, and the bolt is not a thing the game models. `spin.whenShut` allows it at
    -- the shut end too. Never MID-SWING: the crane is travelling then and two writers on one bone is the one thing
    -- this weapon has already taught.
    local sc2 = cfg.spin
    local spinOK = sc2 and ((hatchS.p or 0.0) > 0.9 or (sc2.whenShut and (hatchS.p or 0.0) <= 0.0))
    if sc2 and not spinOK then spinS.dbg = 'midswing' end
    local spinDriven = false
    if spinOK then
        local okt2, f2, tr2 = pcall(function() return slotComp:GetSlotTransform(CName.new(sc2.slot)) end)
        -- the pads if the wrist bone reads, the tracked hand point if it does not. A plain `if`, not `a or b`: an
        -- `and`/`or` expression yields ONE value and would silently drop two thirds of the position.
        local hpx, hpy, hpz = padModel(free)
        if not hpx then hpx, hpy, hpz = handModel(free) end
        if not (okt2 and f2 and tr2) then spinS.dbg = 'noslot'
        elseif not hpx then spinS.dbg = 'nohand' end
        if okt2 and f2 and tr2 and hpx then
            local okp2, cp = pcall(function() return WorldPosition.ToVector4(tr2.Position) end)
            local okq2, cq = pcall(function() return tr2.Orientation end)
            if okp2 and cp and okq2 and cq then
                -- ONE SPACE FOR BOTH, and this is the whole reason the gesture never once fired. `handModel` answers
                -- in MODEL space -- the player's own frame, a couple of metres across -- while a slot transform
                -- answers in WORLD space, which in this game is a thousand metres and more from the map's origin.
                -- Subtracting one from the other put the hand 1170 m off the cylinder's axis, so no radius could
                -- ever have been wide enough and no amount of widening would have helped. The log said it in one
                -- line: `sp[rad=1170.146 al=+913.702 g=n]`.
                --
                -- This is `slotModel`'s own body inlined -- the transform is already in hand, so there is no reason
                -- to fetch it a second time -- plus the same world-to-model turn for the orientation that
                -- `magWristTarget` does with the weapon's.
                local ax3 = sc2.axis or { 0, 1, 0 }
                local cmx, cmy, cmz = qrot(ci, cj, ck, cr, cp.x - bx, cp.y - by, cp.z - bz)
                local m1, m2, m3, m4 = qmul(ci, cj, ck, cr, cq.i, cq.j, cq.k, cq.r)
                local axx, axy, axz = qrot(m1, m2, m3, m4, ax3[1], ax3[2], ax3[3])
                local vx3, vy3, vz3 = hpx - cmx, hpy - cmy, hpz - cmz
                local along = vx3 * axx + vy3 * axy + vz3 * axz
                local rx3, ry3, rz3 = vx3 - along * axx, vy3 - along * axy, vz3 - along * axz
                local rad = len3(rx3, ry3, rz3)
                -- Same rule as gripOf above (this site predates it and reads the slot directly).
                local grip = (not wheelOwns(free))
                             and ((GetVRSharedSlot(free == 0 and 155 or 49) or 0.0) > 0.5)
                -- WHERE THE HAND ACTUALLY IS relative to the cylinder, in the cylinder's own terms: how far off its
                -- axis and how far along it. The gate is those two against `radius` and `reach`, so a gesture that
                -- never fires is one of them out of range -- and that cannot be reasoned out from the mesh, because
                -- what has to fall inside the zone is the tracked HAND POINT, a palm's depth ahead of the wrist.
                spinS.dbg = string.format('rad=%.3f al=%+.3f g=%s w=%.0f', rad, along, grip and 'Y' or 'n',
                    spinS.w or 0)
                -- THE PREVIEW, as every other grip in this module has one: the hand takes the shape of the gesture as
                -- soon as it is IN the zone, before the squeeze, so a player can see what the cylinder will do. Not
                -- while a speedloader is in that hand -- the carry pose outranks it, and nothing about presenting six
                -- rounds looks like rolling a cylinder.
                -- A HAND WITH A SPEEDLOADER IN IT CANNOT ROLL THE CYLINDER. It is holding six rounds by their star:
                -- the pads are on that, not on the cylinder, and the squeeze that holds it is the same squeeze this
                -- gesture reads. Without this, carrying a loader past the cylinder spun it -- and the loader's own
                -- carry pose was fighting the spin preview for the same wrist, which the preview already stood down
                -- from. The DRIVE has to stand down too, not just the pose.
                --
                -- `glide` counts: that is the loader on its way home under nobody's control, and the hand is only
                -- just off it.
                local magInHand = (magS.state == 'hand' or magS.state == 'glide')
                                  and (magS.hand == nil or magS.hand == free)
                -- NARROW TO CATCH, WIDE TO LOSE. The same latch the slide's grip and the well's magnet use, and for
                -- the same reason: a zone with one edge chatters as soon as the hand sits on it, and the hand always
                -- ends up sitting on it. Once it has the cylinder it takes a real move away to give it up.
                local R7 = sc2.radius or 0.09
                local A7 = sc2.reach or 0.06
                if spinS.on then
                    local wd = sc2.widen or 1.5
                    R7, A7 = R7 * wd, A7 * wd
                end
                local inZone = rad > 0.005 and rad <= R7 and math.abs(along) <= A7 and not magInHand
                spinS.on = inZone
                if sc2.pose and inZone then
                    fadeIn(free, sc2)
                    spinS.faded = true
                elseif spinS.faded then
                    fadeOut(); spinS.faded = nil
                end
                if grip and inZone then
                    -- ROLLING CONTACT, which is what a finger on a cylinder is. The surface goes with the finger,
                    -- so the cylinder's rate is the finger's speed ALONG THE TANGENT over the radius it touches
                    -- at: w = v_t / r. This replaces a bearing angle -- how far round the axis the hand had got
                    -- -- and it is better on three counts.
                    --
                    -- It is measured RELATIVE to the cylinder, so gun and hand moving together spin nothing; a
                    -- bearing taken in a frame that rides the gun cannot tell that from a real turn. A finger
                    -- swept across in a STRAIGHT line rolls it exactly as far as it should, which is how the
                    -- gesture is actually made -- nobody traces an arc round the axis. And a hand pushing
                    -- straight AT the cylinder contributes nothing at all, which falls out of the projection
                    -- rather than needing a rule.
                    --
                    -- The radius has a floor: a contact on the axis itself would divide by nothing.
                    local tx7 = axy * rz3 - axz * ry3
                    local ty7 = axz * rx3 - axx * rz3
                    local tz7 = axx * ry3 - axy * rx3
                    local tl7 = len3(tx7, ty7, tz7)
                    if tl7 > 1e-6 and spinS.pp and spinS.hand == free and dt > 1e-4 then
                        tx7, ty7, tz7 = tx7 / tl7, ty7 / tl7, tz7 / tl7
                        local vhx = (hpx - spinS.pp[1]) - (cmx - spinS.pp[4])
                        local vhy = (hpy - spinS.pp[2]) - (cmy - spinS.pp[5])
                        local vhz = (hpz - spinS.pp[3]) - (cmz - spinS.pp[6])
                        local vt = (vhx * tx7 + vhy * ty7 + vhz * tz7) / dt
                        local w = math.deg(vt / math.max(rad, 0.012)) * (sc2.gain or 1.0)
                        local mx = sc2.maxRate or 1440.0
                        if w > mx then w = mx elseif w < -mx then w = -mx end
                        spinS.ang = (spinS.ang or 0.0) + w * dt
                        -- ...and the rate it hands over on release, averaged with the last frame's so that one
                        -- frame of tracking noise does not decide how hard it was thrown
                        spinS.w = (spinS.w or 0.0) * 0.5 + w * 0.5
                    end
                    spinS.pp = { hpx, hpy, hpz, cmx, cmy, cmz }
                    spinS.hand = free
                    spinDriven = true
                else
                    spinS.pp, spinS.hand = nil, nil
                end
            end
        end
    else
        spinS.pp, spinS.hand = nil, nil
    end
    -- ------------------------------------------------------------------ AND THEN IT FREE-WHEELS
    --
    -- Off the hand the cylinder keeps whatever rate it was given and slows the way something on a bearing slows: a
    -- VISCOUS term proportional to the rate, and a CONSTANT one that does not care how fast it is going (dry friction
    -- in the bearing, which is what finally stops it rather than letting it creep on for ever).
    --
    --     dw/dt = -drag * w  -  stop * sign(w)
    --
    -- It free-wheels whether the crane is out or shut and even while the crane is travelling: they are different
    -- bones, and a cylinder does not stop turning because the frame it sits in is moving.
    if sc2 and not spinDriven then
        local w = spinS.w or 0.0
        if math.abs(w) > 1.0 then
            spinS.ang = (spinS.ang or 0.0) + w * dt
            w = w - (sc2.drag or 1.2) * w * dt
            local cou = (sc2.stop or 220.0) * dt
            if w > 0.0 then w = math.max(0.0, w - cou) else w = math.min(0.0, w + cou) end
            spinS.w = w
        else
            spinS.w = 0.0
        end
    end
    -- ONE CLICK PER CHAMBER, wherever the angle came from -- the hand's or its own momentum. A cylinder does not hiss
    -- round, it indexes: six detents a turn on this gun, so the sound is tied to CROSSING a multiple of 60 deg.
    if sc2 then
        local step5 = sc2.detent or 60.0
        local n5 = math.floor((spinS.ang or 0.0) / step5)
        if spinS.det ~= n5 then
            if spinS.det ~= nil then playSnd(weapon, snd and snd.spin) end
            spinS.det = n5
        end
    end
    if sc2 then
        local ax4 = sc2.axis or { 0, 1, 0 }
        if math.abs(spinS.ang) > 0.05 then
            VRRigWriteRot(sc2.which, sc2.bone, spinS.ang, ax4[1], ax4[2], ax4[3])
            spinS.wrote = true
        elseif spinS.wrote then
            VRRigWriteRot(sc2.which, sc2.bone, 0.0, ax4[1], ax4[2], ax4[3])
            spinS.wrote = false
        end
        -- THE BOLT DROPS INTO ITS NOTCH. Shutting the crane re-indexes the cylinder to the gun's own alignment, so
        -- whatever a hand had turned it to is given up at the latch -- and its momentum with it, which is the more
        -- visible half: a cylinder still free-wheeling inside a closed frame is a cylinder nothing is holding.
        --
        -- On the EDGE of latching, not while shut. While shut is every frame, and this weapon allows a closed
        -- cylinder to be turned -- zeroing it per frame would mean it could never be turned at all.
        local shut = (hatchS.p or 0.0) <= 0.0
        if shut and not spinS.wasShut then
            spinS.ang, spinS.w, spinS.det = 0.0, 0.0, nil
        end
        spinS.wasShut = shut
    end

    -- ------------------------------------------------------------------ THE HAMMER (a revolver's, on the stick)
    --
    -- The stick CLICK cocks it and it stays cocked, because that is what a hammer does -- there is no spring here
    -- to bring it back and nothing else to release it but the shot. So the write is dropped the moment the weapon
    -- FIRES: the game's own recoil clip throws the hammer, and holding a rig write through that would be a tug of
    -- war we would win and should not.
    --
    -- Nothing in this weapon's animations does this. Measured across every clip, the hammer moves in exactly two --
    -- `recoil_shoulder` (40.3 deg) and `safe_action` (37.5) -- and in neither does the player's thumb move at all,
    -- 0.0 deg on all three joints. So the travel is the game's own and the gesture is ours.
    local mc2 = cfg.hammer
    if not mc2 then setTrg(0) end
    if mc2 then
        -- THE CLICK TOGGLES IT: down on one press, back on the next. A shot would be the honest release, and it is
        -- still honoured below, but the ammo count is not a reliable trigger on this weapon -- the log reads it as
        -- `0` and `nil` by turns -- and a thumb that can only ever push one way is worse than one that cannot.
        local clk = (GetVRSharedSlot(159) or 0.0) > 0.5
        if clk and not hamS.prev then
            hamS.want = (hamS.want > 0.5) and 0.0 or 1.0
            hamS.seq = cfg.hammer.poses and 0.0 or nil     -- the thumb sets off; the hammer waits for it below
            hamS.sounded = false
        end
        hamS.prev = clk

        -- ------------------------------------------------------------- THE TRIGGER, AND WHAT IT DOES TO THE HAMMER
        --
        -- A revolver's trigger is not a switch, and this is the one weapon in the set where that shows. Squeeze it
        -- with the hammer down and it CARRIES the hammer back and lets go near the end of its travel -- double
        -- action, a long deliberate pull. Squeeze it with the hammer already cocked and the sear lets go almost at
        -- once -- single action, a touch. The same finger, two entirely different guns.
        --
        -- So the analog value drives the hammer directly (shared[160], published by the merge; [30] is the same
        -- trigger as a FLAG and a flag cannot express travel), and the port decides where in that travel the shot
        -- falls rather than the game's own threshold:
        --
        --     below the break   the trigger is SWALLOWED   -- the game sees nothing, so nothing fires early
        --     at the break      the trigger is PRESSED     -- the game fires, on the frame the hammer drops
        --     cylinder out      swallowed throughout       -- the action still works, it just lands on nothing
        --
        -- That last line is also the whole fire block, and it replaces emptying the magazine behind the game's
        -- back: a gun that is merely disconnected from its trigger keeps its rounds and its count, so opening the
        -- crane costs nothing and a speedloader put in with the crane still open cannot be fired either.
        local ta = (type(GetVRSharedSlot) == 'function' and GetVRSharedSlot(160)) or 0.0
        local craneOpen = (hatchS.p or 0.0) > 0.9
        local SA  = mc2.saBreak or 0.20
        local DA  = mc2.daBreak or 0.75
        local REL = mc2.release or 0.10
        local cocked = (hamS.want or 0) > 0.5 or (hamS.p or 0) > 0.9
        local brk = cocked and SA or DA
        if ta < REL then hamS.fired = false end          -- the finger came off: the action can be worked again

        if hamS.fall then
            -- IT FALLS FASTER THAN IT WAS CARRIED, because nothing is easing it down any more.
            hamS.fall = hamS.fall + dt
            local DN = mc2.dryDrop or 0.035
            if hamS.fall >= DN then
                hamS.p, hamS.want, hamS.fall = 0.0, 0.0, nil
            else
                hamS.p = (hamS.p0 or 1.0) * (1.0 - hamS.fall / DN)
            end
        elseif ta > 0.02 and not hamS.fired then
            -- carried back by the squeeze: all the way from rest in double action, already there in single
            hamS.p = cocked and 1.0 or math.min(1.0, ta / brk)
            if ta >= brk then
                hamS.p0, hamS.fall, hamS.fired = hamS.p, 0.0, true
                playSnd(weapon, snd and (snd.dryFire or snd.hammer))
                hamS.seq = nil                    -- the trigger finger is doing this, not the thumb
                if hamS.key then clearGrip(holder); hamS.key = nil end
            end
        end
        -- ...and the mode that goes with it. `fired` keeps the trigger pressed until the finger comes off, so a
        -- held squeeze behaves as it does in flat rather than firing once and going dead.
        local mode = 0
        if craneOpen then mode = 1
        elseif hamS.fired then mode = 2
        elseif ta > 0.0 then mode = (ta >= brk) and 2 or 1 end
        setTrg(mode)
        hamS.dbg = string.format('%.2f %s%s m%d', ta, cocked and 'SA' or 'DA', hamS.fired and '!' or '', mode)

        -- THE SHOT LETS IT GO. `slideS.ammo` is read every frame further up; a drop in it is a round fired.
        local a2 = slideS.ammo
        if hamS.last ~= nil and a2 ~= nil and a2 < hamS.last then hamS.want = 0.0 end
        hamS.last = a2
        -- THE THUMB GOES FIRST, AND THE HAMMER WAITS FOR IT. A hammer that starts moving on the same frame as the
        -- click reads as a hammer moving by itself: what sells it is the finger arriving, bearing down, and the
        -- hammer going only then. So the click starts a THUMB sequence and the hammer's own ramp is held until the
        -- press lands in the middle of it.
        --
        --   reach   over -> hold, the arc the game's hand makes coming off the grip     (`reachTime`)
        --   press   the bearing-down shape, and the hammer is released to move          (`pressTime`)
        --   hold    resting on the hammer for as long as it stays where it was put
        --   away    the thumb withdraws once the hammer is home again
        --
        -- Laid on the hand that HOLDS the gun and only three joints wide, so the grip is untouched.
        local hp = cfg.hammer.poses
        if hp and not hamS.fall and not (ta > 0.02) then
            -- A KEYFRAMED TRACK, crossed between every frame -- pose AND strength both. Switching from one pose to
            -- the next at a boundary is what made it read a frame at a time; six keys and an nlerp make the same
            -- 0.6 s continuous. The shape is the game's own reach: out over the top, down onto the hammer, a short
            -- bearing-down, and away.
            --
            -- AND IT ALWAYS COMES BACK. The first cut left the thumb resting on the hammer until the next click,
            -- which is what the game does -- but the game is holding the hammer through a reload, and here the
            -- press is a press: the finger does its work and returns to the grip either way.
            local RT, PT = mc2.reachTime or 0.24, mc2.pressTime or 0.10
            local AW = mc2.awayTime or 0.22
            local KEY = {
                { 0.00,        hp.over,  0.0 },
                { RT * 0.55,   hp.over,  1.0 },
                { RT,          hp.hold,  1.0 },
                { RT + PT,     hp.press, 1.0 },   -- the impact: quick in, and the hammer is freed here
                { RT + PT * 2, hp.hold,  1.0 },
                { RT + PT * 2 + AW, hp.hold, 0.0 },
            }
            if hamS.seq then
                hamS.seq = hamS.seq + dt
                local t3 = hamS.seq
                if t3 >= KEY[#KEY][1] then
                    if hamS.key then clearGrip(holder); hamS.key = nil end
                    hamS.seq = nil
                else
                    local a, b, u = KEY[1], KEY[1], 0.0
                    for k = 1, #KEY - 1 do
                        if t3 >= KEY[k][1] and t3 <= KEY[k + 1][1] then
                            local span = KEY[k + 1][1] - KEY[k][1]
                            a, b = KEY[k], KEY[k + 1]
                            u = (span > 1e-6) and (t3 - KEY[k][1]) / span or 0.0
                            break
                        end
                    end
                    -- smoothstep on the crossing, so no key reads as a corner
                    local us = u * u * (3.0 - 2.0 * u)
                    local pose = mixPose(POSES[a[2]], POSES[b[2]], us)
                    local al = a[3] + (b[3] - a[3]) * us
                    if pose then
                        -- the table is rebuilt every frame by design; `applyGrip` re-sends it, and at three joints
                        -- that is three native calls, which is nothing next to the magazine's nineteen
                        applyGrip(holder, pose, al)
                        hamS.key = 'ham'
                    end
                end
            elseif hamS.key then
                clearGrip(holder); hamS.key = nil
            end
        end

        -- ------------------------------------------------------------------ THE TRIGGER FINGER
        --
        -- It BENDS with the squeeze: from the player's own tracked finger to the shape the game's hand makes with its
        -- finger in the trigger (`reload_02`, the one clip that holds this gun throughout -- 22/6/7 deg of knuckle
        -- bend at its most extended, 30/39/2 at its most curled). The mapping onto the controller's analog is ours and
        -- runs from `trigFrom` to wherever the shot breaks, so the finger arrives exactly as the hammer lets go.
        --
        -- THREE JOINTS, the index's KNUCKLES alone, laid on the hand that HOLDS the gun -- a pose writes only what
        -- it names, so the grip is untouched. Not the metacarpal: that joint is the finger's spread, and driving it
        -- swung the whole finger away from the hand instead of folding it -- the finger lifting, not bending.
        --
        -- Re-sent EVERY FRAME: the game poses this hand too, and a pose written once is a pose the animation graph
        -- paints over on its next pass.
        --
        -- The thumb outranks it. Both want the same wrist, and while the thumb is cocking the hammer the trigger
        -- finger has nothing to do -- so `hamS.seq` running means the thumb owns the hand and this stands down,
        -- rather than the two of them clearing each other's joints turn about.
        local tp = mc2.trigPose and POSES[mc2.trigPose]
        if tp and not hamS.seq and fadeS.hand ~= holder then
            local FROM = mc2.trigFrom or 0.10
            local u = (ta - FROM) / math.max(0.01, brk - FROM)
            if u < 0 then u = 0 elseif u > 1 then u = 1 end
            if u > 0.0 then
                -- ONE pose and a WEIGHT, not two poses and a mix between them. The open end of this travel is the
                -- player's OWN finger -- the plugin nlerps the pose onto the live tracked joints by this weight on
                -- every pass -- so there is nothing to record for it and nothing that can fight the hand at rest.
                applyGrip(holder, tp, u)
                hamS.fin = true
            elseif hamS.fin then
                clearGrip(holder); hamS.fin = nil
            end
        elseif hamS.fin then
            clearGrip(holder); hamS.fin = nil
        end

        -- ONLY WHEN THERE IS SOMEWHERE TO GO. Without this guard the ramp is an oscillator: at p == want the test
        -- `want > p` is false, so it takes the falling branch and walks the hammer back down, which makes the test
        -- true again. The log caught it exactly -- `want` steady at 1 while p swung 0.54 to 1.00, one dropTime's
        -- worth of travel per bounce. The hatch's own ramp has this guard; this one was written without it.
        -- the hammer may not move until the thumb is actually on it and pressing
        local freed = (not cfg.hammer.poses) or (hamS.seq == nil)
                      or hamS.seq >= (mc2.reachTime or 0.24)
        if freed and not hamS.sounded then
            playSnd(weapon, snd and snd.hammer)
            hamS.sounded = true
        end
        if freed and hamS.p ~= hamS.want and not hamS.fall and not (ta > 0.02) then
            local T2 = (hamS.want > hamS.p) and (mc2.cockTime or 0.18) or (mc2.dropTime or 0.05)
            local st = dt / ((T2 > 1e-3) and T2 or 1e-3)
            hamS.p = (hamS.want > hamS.p) and math.min(1.0, hamS.p + st) or math.max(0.0, hamS.p - st)
        end
        local ax2 = mc2.axis or { 1, 0, 0 }
        if hamS.p <= 0.0 then
            if hamS.wrote then
                VRRigWriteRot(mc2.which, mc2.bone, 0.0, ax2[1], ax2[2], ax2[3])
                hamS.wrote = false
            end
        else
            -- eased at the top so the thumb's own effort reads: quick off the rest, slow onto the sear
            local f2 = math.sin(math.pi * 0.5 * hamS.p)
            VRRigWriteRot(mc2.which, mc2.bone, (mc2.angle or 37.5) * (mc2.sign or 1.0) * f2,
                          ax2[1], ax2[2], ax2[3])
            hamS.wrote = true
        end
    end

    -- --------------------------------------------------------------- WHILE WE DRIVE IT, WE OWN IT
    --
    -- A rig write does not REPLACE what the animation graph poses -- it rides on top of it. As long as the graph
    -- leaves a bone alone that is the same thing, and every weapon so far has been driven while the game was doing
    -- nothing with the part. A REVOLVER breaks that: the game keeps its own hand on the crane, and our 100 deg then
    -- lands on whatever it happens to be posing, so the cylinder goes where neither of us asked. Intermittently --
    -- "иногда всё ок" -- which is exactly what a race between two writers looks like.
    --
    -- The cure is the one the slide stop already uses: hold the weapon in its reload stage, which is the switch that
    -- takes the graph's hands off, and let it go again the moment we stop driving. Behind `ownAnim` because it is a
    -- claim on the whole weapon, and the eleven guns that do not need it should not make it.
    -- WHAT THE BONE ACTUALLY HOLDS, read back rather than assumed. A rig write is a request; if the result wanders
    -- while the request is steady, the wandering is somebody else's -- and this is the one measurement that can say
    -- so. Logged as an angle AND an axis, because the two mean different culprits: a moving axis says the pose we
    -- ride on has moved, a moving angle says our own value did.
    if cfg.hatch and type(VRRigBone) == 'function' then
        local q = {}
        for f = 3, 6 do
            local ok, v = pcall(function() return VRRigBone(cfg.hatch.which, cfg.hatch.bone, f) end)
            q[#q + 1] = (ok and v) or 0.0
        end
        local w = math.abs(q[4])
        if w > 1.0 then w = 1.0 end
        local a = math.deg(2.0 * math.acos(w))
        local sn = math.sqrt(math.max(1e-9, 1.0 - w * w))
        hatchS.read = string.format('%.0f/(%+.2f,%+.2f,%+.2f)', a, q[1] / sn, q[2] / sn, q[3] / sn)
    end
    if cfg.ownAnim then
        local mine = (hatchS.p or 0.0) > 0.0 or (hamS.p or 0.0) > 0.0 or (spinS.wrote or false)
        if mine then
            holdReloadStage(weapon)
            ownS.tagged = true
        elseif ownS.tagged then
            pushAnimEvent(weapon, 'InterruptReload')   -- the weapon is the game's again
            ownS.tagged = false
        end
    end

    -- Hand every rig write back to the game once nothing is being driven, so the gun animates normally again.
    -- NOTE: an eject fling or a chambering chase in flight also counts as driven -- clearing mid-move would
    -- snap the round meshes.
    local active = slideS.grabbed or slideS.releasing or (rotS.t >= 0)
                or (slideS.ejectT ~= nil) or (slideS.chaseV ~= nil)
                or (magS.state ~= 'in')     -- a magazine out of the well is being driven too
                -- ...and a cover still swinging shut, which happens AFTER the magazine is home: without this the
                -- clear lands on the very frame it seats and the cover snaps shut instead of closing
                or ((hatchS.p or 0.0) > 0.0)
                or ((hamS.p or 0.0) > 0.0)  -- ...and a cocked hammer, which is held until the shot
                or (spinS.wrote or false)   -- ...and a cylinder the player has turned
    if (not active) and slideS.wasActive then
        pcall(VRRigWriteClear)
        -- the clear drops the hide-scale too, so forget it was applied and let the ammo watch re-assert it (an
        -- empty chamber must stay empty-looking after the writes are handed back)
        slideS.seatedShown = nil
    end
    slideS.wasActive = active
end

return M
