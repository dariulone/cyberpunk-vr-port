local Policy = require('optics/policy')
local Profiles = require('optics/profiles')
local M = { enabled=true, path='none', magnification=1, error=nil,
    -- Counters for the test harness: camera switched off while on, stereo waits, re-applies.
    stats={ cameraOffs=0, stereoWaits=0, readyWaits=0, applies=0 } }
-- The path policy, reachable from the CET console (e.g. policy.cameraThreshold).
M.policy = Policy
local state = { elapsed=0, weaponID=nil, weaponKey=nil, originals={}, cameraOn=false }
-- The eye at a sight attachment brings the sight closer, with no aim trigger. Every
-- attachment alike (collimator, shader and camera scopes, native); the glass keeps its
-- own magnification. Iron sights get nothing; a smart sniper (Ashura) zooms only on the
-- trigger, as in the game. Per path: how far behind the sight the eye may be.
--   'approach' (default): both eye cameras move towards the eyepiece along the sight's
--     axis (SetVRSightApproach) -- the sight grows in view as when a real eye closes in,
--     while the world keeps its angular scale and head rotation stays 1:1;
--   'zoom': the game's own ADS zoom, x1.25 of the whole view.
local SIGHT_ZOOM_LEVEL = 1.25
local SIGHT_ZOOM = { collimator={ behind=0.80 },  -- a pistol sight at arm's length
    shader={ behind=0.30 }, camera={ behind=0.30 }, native={ behind=0.30 } }
local ENTER_AXIS, LEAVE_AXIS = 0.030, 0.045    -- eye to the sight's line, metres (hysteresis)
local RELEASE_HOLD = 0.35                       -- weapon layer stays synced while the world zooms back out
-- Approach: the eye goes to `keep` of its distance behind the sight, never nearer than
-- `relief` (the eyepiece rim stays outside the near plane) and at most `max` metres.
-- Live-tunable from the CET console: GetMod('CyberpunkVRPort_Optics').approach.max = 0.08
M.approach = { keep=0.5, relief=0.03, max=0.065 }
local APPROACH_TIME = 0.15
local sight = { looking=false, active=false, synced=false, hold=0, approach=0, ox=0, oy=0, oz=0 }
M.sightZoom, M.eyeToAxis, M.sightZoomEnabled, M.sightMode, M.sightApproach = 1, nil, true, 'approach', 0
M.shotFromSight = true   -- shots leave the sight line while the eye is on a sight (sightLine)
-- SHADER SCOPES SPLIT THEIR MAGNIFICATION. The lens magnifies the eye's own pre-upscale
-- render, so its sharpness falls with its magnification; a dedicated camera costs ~12 FPS
-- whatever its field. Part of the magnification is moved into the view itself: while the eye
-- is on the scope the whole view takes the game's ADS zoom Z (the weapon layer synced with it)
-- and the lens magnifies by M/Z, so the scene keeps its native M through the glass and the lens
-- samples a render Z times denser. Z runs linearly from lo.zoom at lo.mag to hi.zoom at hi.mag
-- (clamped): short_06 x1.25 (lens x1.36), long_05 x1.5 (lens x1.567).
M.shaderSplit = { enabled=true, lo={ mag=1.7, zoom=1.25 }, hi={ mag=2.35, zoom=1.5 } }
local function shaderZoom(mag)
    local cfg = M.shaderSplit
    if not cfg or not cfg.enabled or type(mag) ~= 'number' or mag <= 1 then return 1 end
    local t = (mag - cfg.lo.mag) / math.max(cfg.hi.mag - cfg.lo.mag, 1e-3)
    t = math.max(0, math.min(1, t))
    return math.min(mag, cfg.lo.zoom + (cfg.hi.zoom - cfg.lo.zoom) * t)
end
M.shaderZoom = shaderZoom

-- Native scope UI parts hidden while the port draws the lens: short_06's red
-- fill is smaller than the glass, which is tinted itself now, and showed as a
-- rectangle inside it; long_05's dot-matrix grid (opqGrid over its whole rear
-- panel) screened the image; long_02's front panel, laid over the whole glass,
-- tinted it blue with its full-canvas scanlines, gradients and noise; Grad's
-- (sniper_01) interference strip ran as a magenta band along the lens's lower
-- edge, and its crosshair grid (crpsshair_bg, 1148x348 canvas px round the
-- centre) as a red band across a glass that its green canvas tints whole. First
-- pass shortly after the scope appears (its widget spawns asynchronously), then
-- every few seconds.
local HIDE_FIRST, HIDE_EVERY = 0.4, 5.0
local HIDE = { short_06 = { glow=true, borderBoxTransp=true }, long_05 = { grid=true },
    long_02 = { effect=true, bg_fuzz=true, fuzz_img=true, scanline_fill=true, inkImageWidget2=true },
    sniper_01 = { interference=true, crpsshair_bg=true } }
M.widgetsHidden = 0

-- Native scope HUDs whose aim mark is off their canvas centre, while the port's panel
-- is centred on the aim point: Osprey draws its target brackets 96 of 524 canvas px
-- above centre, and the shot landed under them. The port's own reticle panel widget
-- is moved by this much (canvas px), so the brackets frame the point the bullet hits.
local HUD_SHIFT = { osprey = { x=0, y=96 } }

-- Sights whose own reticle does not show in the headset get the point the bullet hits
-- drawn by the plugin while the eye is on them (SetVRSightDot). None now: long_04's and
-- long_06's holo reticles do show (14.10) and the dot only doubled them.
local DOT_SIGHTS = {}

-- Holo panels whose reticle is drawn off the canvas centre. The panels are collimated
-- about their centre, which is the line the shot flies along (shots leave the sight line),
-- so the reticle is moved onto it. long_04: the corner frame you see is the back panel's
-- (464x208, 128 of 512 canvas px below centre) -- it stood 11 px under the bullet point
-- in a 960 px eye; the front panel's frame sits 128 above. Canvas px per component.
local HOLO_SHIFT = { w_att__scope_long_04 = {
    on_weapon_ui_01 = { x=0, y=128 }, on_weapon_ui_02 = { x=0, y=-128 } } }

-- The eyes are settled on the player in hand: the second view (VRCAM) is enabled on it, or no
-- second view is asked for at all. Engine stereo renders both eyes in MAIN's own frame and never
-- enables a VRCAM, so waiting for one there kept every sight on its flat-screen state: no
-- collimation, scope HUDs only while aiming, no camera scopes.
local stereoMod
local function stereoBound(player)
    stereoMod = stereoMod or GetMod('CyberpunkVRPort_Stereo')
    local stereo=stereoMod
    local selection=stereo and stereo.VrcamSel
    if not (stereo and stereo.ready and selection) or selection.loadingActive
        or selection.appliedName ~= selection.selected
        or selection.appliedId ~= tostring(player:GetEntityID().hash) then
        return false
    end
    if selection.appliedWant == false then return true end
    return selection.appliedWant == true and selection.enabledCount == 1
end

-- The per-frame math runs on plain numbers: a Quaternion or Vector4 made in Lua is a game value
-- (binding work, and garbage for the collector). Only what goes to the game is made as one.
local function qmul(ai, aj, ak, ar, bi, bj, bk, br)
    return ar*bi+ai*br+aj*bk-ak*bj,
        ar*bj-ai*bk+aj*br+ak*bi,
        ar*bk+ai*bj-aj*bi+ak*br,
        ar*br-ai*bi-aj*bj-ak*bk
end

local function multiply(a, b)
    return Quaternion.new(qmul(a.i, a.j, a.k, a.r, b.i, b.j, b.k, b.r))
end

-- v rotated by q, as a plain {x, y, z}.
local function rotate(q, v)
    local qi, qj, qk, qr = q.i, q.j, q.k, q.r
    local i, j, k, r = qmul(qi, qj, qk, qr, v.x, v.y, v.z, 0)
    i, j, k = qmul(i, j, k, r, -qi, -qj, -qk, qr)
    return {x = i, y = j, z = k}
end

local function toggle(component, value)
    if component and component:IsEnabled() ~= value then component:Toggle(value) end
end

-- cameraOff runs on every frame without a camera path. A camera this state did not switch on is
-- looked up (a component search by name on the player) this often rather than every frame.
local IDLE_CAMERA_CHECK = 0.25
local opticsCameraName
local function cameraOff(player)
    if state.cameraOn then M.stats.cameraOffs=M.stats.cameraOffs+1 end
    -- A save can restore the component enabled before this Lua state owns it.
    -- Check the actual component even when our local cameraOn starts false.
    local now = os.clock()
    if player and (state.cameraOn or now >= (state.nextIdleCameraCheck or 0)) then
        state.nextIdleCameraCheck = now + IDLE_CAMERA_CHECK
        opticsCameraName = opticsCameraName or CName.new('vr_optics_camera')
        toggle(player:FindComponentByName(opticsCameraName), false)
    end
    state.cameraOn = false
end

local function walkWidgets(widget, depth, visit)
    if not widget then return end
    visit(widget)
    if depth < 6 and widget:IsA(CName.new('inkCompoundWidget')) then
        for i = 0, widget:GetNumChildren() - 1 do walkWidgets(widget:GetWidgetByIndex(i), depth + 1, visit) end
    end
end

local function scopeWidgets(weapon, profile)
    local hide = profile and HIDE[profile.key]
    if not hide then return end
    state.hiddenWidgets = state.hiddenWidgets or {}
    local worldWidget = CName.new('WorldWidgetComponent')
    for _, component in ipairs(Game.VREntityComponents(weapon)) do
        if component:IsA(worldWidget) and component:IsEnabled() then
            local ok, root = pcall(function() return component:GetWidget() end)
            if ok and root then
                walkWidgets(root, 0, function(widget)
                    if hide[NameToString(widget:GetName())] and widget:IsVisible() then
                        widget:SetVisible(false)
                        table.insert(state.hiddenWidgets, widget)
                        M.widgetsHidden = M.widgetsHidden + 1
                    end
                end)
            end
        end
    end
end

local function updateWidgets(weapon, delta)
    if not HIDE[state.profile.key] then return end
    local key = state.weaponKey..':'..M.path
    if key ~= state.widgetKey then state.widgetKey, state.widgetHide = key, HIDE_EVERY - HIDE_FIRST end
    state.widgetHide = state.widgetHide + (delta or 0)
    if state.widgetHide >= HIDE_EVERY then
        state.widgetHide = 0
        scopeWidgets(weapon, state.profile)
    end
end

local function restoreWeapon()
    for _, widget in ipairs(state.hiddenWidgets or {}) do pcall(function() widget:SetVisible(true) end) end
    state.hiddenWidgets, state.widgetKey = {}, nil
    -- Resolve a live entity again; never dereference cached component wrappers
    -- after equipment swaps, unloading a save, or appearance replacement.
    local weapon = state.weaponID and Game.FindEntityByID(state.weaponID)
    if weapon then
        for _, component in ipairs(Game.VREntityComponents(weapon)) do
            local name = NameToString(component:GetName())
            local saved = state.originals[name]
            if saved then
                if saved.mask ~= nil then component.chunkMask = saved.mask end
                if saved.visibility then component.visibilityAnimationParam = saved.visibility end
                toggle(component, saved.enabled)
                if saved.mask ~= nil then Game.VRComponentRefreshAppearance(component) end
            end
            if name == 'vr_optic_low' or name == 'vr_optic_surface' or name == 'vr_optic_panel'
                or name:sub(1,17) == 'vr_optic_reticle_' then
                toggle(component, false)
            end
        end
    end
    state.originals = {}
    state.weaponID, state.weaponKey = nil, nil
    state.applied, state.profile = nil, nil
    state.low, state.lowApp = nil, nil
    state.smart, state.dotSight = nil, nil
end

local function scan(player, weapon)
    local id = weapon and weapon:GetEntityID()
    local key = id and tostring(id.hash)
    if key ~= state.weaponKey then
        restoreWeapon()
        state.weaponID, state.weaponKey = id, key
    end
    if not weapon or not weapon:IsRanged() then M.path='none';cameraOff(player);return end
    local components, profile, main, holo = {}, nil, nil, nil
    state.dotSight = false
    for _, component in ipairs(Game.VREntityComponents(weapon)) do
        local name = NameToString(component:GetName())
        components[name] = component
        for _, prefix in ipairs(DOT_SIGHTS) do
            if name:sub(1, #prefix) == prefix then state.dotSight = true end
        end
        for prefix, shifts in pairs(HOLO_SHIFT) do
            if name:sub(1, #prefix) == prefix then holo = shifts end
        end
        if Profiles[name] then profile, main = Profiles[name], component end
        -- SkinnedMeshComponent derives from ISkinTargetComponent, not
        -- MeshComponent. The native ADS lens/cap gates belong to this class.
        if component:IsA(CName.new('entSkinnedMeshComponent')) then
            local visibility = NameToString(component.visibilityAnimationParam)
            if visibility == 'visibleWhenAiming' or visibility == 'invisibleWhenAiming' then
                state.originals[name] = state.originals[name] or {
                    visibility=component.visibilityAnimationParam, enabled=component:IsEnabled() }
                component.visibilityAnimationParam = CName.new('None')
                toggle(component, visibility == 'visibleWhenAiming')
            end
        end
    end
    -- Holo reticles moved onto the panel centre (HOLO_SHIFT); every scan, as the widgets
    -- spawn asynchronously and are rebuilt with the attachment.
    for name, shift in pairs(holo or {}) do
        local panel = components[name]
        if panel then
            local ok, root = pcall(function() return panel:GetWidget() end)
            if ok and root then root:SetTranslation(Vector2.new({X=shift.x, Y=shift.y})) end
        end
    end
    local zoom = Game.GetStatsSystem():GetStatValue(weapon:GetEntityID(), gamedataStatType.ZoomLevel)
    local record = weapon:GetWeaponRecord()
    local sniper = record:ItemType():Type() == gamedataItemType.Wea_SniperRifle
    -- A smart sniper (Ashura) keeps the game's own ADS zoom and HUD on the trigger.
    local evolution = record:Evolution()
    state.smart = sniper and evolution ~= nil and evolution:Type() == gamedataWeaponEvolution.Smart
    -- Unknown sniper layouts must retain their native display until their
    -- aperture is authored. Open sights never acquire the flat-screen zoom.
    local path, mag = Policy.select(weapon:HasScope() or sniper, not profile and not sniper, sniper, zoom)
    if not profile then path = path == 'camera' and 'native' or path end
    M.path, M.magnification, M.profile = path, mag, profile and profile.key
    M.scopeOffset = profile and profile.scopeOffset
    state.profile = profile
    local low, surface, panel = components.vr_optic_low, components.vr_optic_surface, components.vr_optic_panel
    if profile and ((not low and not profile.cameraOnly) or not surface or not panel) then
        M.path='native';M.error='Missing authored optical surface: '..profile.key
        cameraOff(player);return
    end
    if profile then
        local name = NameToString(main:GetName())
        state.originals[name] = state.originals[name] or { mask=main.chunkMask, enabled=main:IsEnabled() }
        if state.originals[name].mask == nil then state.originals[name].mask = main.chunkMask end
        -- The lens takes what the view's own zoom leaves of the magnification (M.shaderSplit).
        local lensMag = path == 'shader' and mag / shaderZoom(mag) or mag
        M.lensMagnification = lensMag
        local appearance = Policy.lowAppearance(lensMag)
        state.low = low
        local applied = profile.key..':'..path..':'..appearance
        local missingState = tonumber(main.chunkMask) ~= profile.bodyMask
            or (low and low:IsEnabled() ~= (path == 'shader'))
            or surface:IsEnabled() ~= (path == 'camera')
            or panel:IsEnabled() ~= (path == 'camera')
            or (low and path == 'shader' and NameToString(low.meshAppearance) ~= (state.lowApp or appearance))
        if state.applied ~= applied or missingState then
            main.chunkMask = profile.bodyMask
            Game.VRComponentRefreshAppearance(main)
            toggle(low, false)
            if path == 'shader' then
                state.lowApp = appearance
                if not Game.VRComponentChangeAppearance(low, CName.new(state.lowApp), true) then
                    error('Cannot select lens magnification '..appearance)
                end
                toggle(low, true)
            end
            toggle(surface, path == 'camera')
            toggle(panel, path == 'camera')
            state.applied = applied
            M.stats.applies=M.stats.applies+1
        end
        if profile.originalPanel and components[profile.originalPanel] then
            local original=components[profile.originalPanel]
            state.originals[profile.originalPanel]=state.originals[profile.originalPanel]
                or {enabled=original:IsEnabled()}
            toggle(original,false)
        end
        -- The port's reticle surfaces carry the native panels on both magnified paths:
        -- on the camera path the camera surface composites them, on the shader path
        -- they sit just in front of the lens (the native rear panel hangs behind the
        -- glass, which hid its crosshair).
        local ours = path == 'camera' or path == 'shader'
        for _, layer in ipairs(profile.reticles or {}) do
            for _, name in ipairs({layer.originalPanel,layer.originalMesh}) do
                local original=components[name]
                if original then
                    state.originals[name]=state.originals[name] or {enabled=original:IsEnabled()}
                    toggle(original,not ours and state.originals[name].enabled)
                end
            end
            toggle(components[layer.surface],ours)
            toggle(components[layer.panel],ours)
            local shift = ours and HUD_SHIFT[profile.key]
            local panel = components[layer.panel]
            if panel then
                local ok, root = pcall(function() return panel:GetWidget() end)
                if ok and root then root:SetTranslation(Vector2.new({X=shift and shift.x or 0, Y=shift and shift.y or 0})) end
            end
        end
    else
        state.applied = nil
    end
    if path ~= 'camera' then cameraOff(player) end
    M.error = nil
end

-- Distance from the nearer eye to the sight's line while that eye is behind the
-- sight (the scope offset point the gallery aligns the eye to), else nil.
-- Also returns how far that eye is behind the sight and the sight's axis. The camera
-- already carries the approach this mod applied; the eye is taken without it.
local function eyeToAxis(weapon, maxBehind)
    local cam = Game.GetCameraSystem()
    local t = Transform.new();cam:GetActiveCameraWorldTransform(t)
    local cx, cy, cz = t.position.x - sight.ox, t.position.y - sight.oy, t.position.z - sight.oz
    local right = cam:GetActiveCameraRight()
    local half = (GetVRSharedSlot and GetVRSharedSlot(95)) or 0.032
    local q, wp = weapon:GetWorldOrientation(), weapon:GetWorldPosition()
    local authored = state.profile and state.profile.scopeOffset
    local o = authored and Vector4.new(authored.x,authored.y,authored.z,1) or weapon:GetScopeOffset()
    local s = rotate(q, o)
    local f = rotate(q, {x = 0, y = 1, z = 0})
    -- The active camera is the game's own view, MAIN: one eye (the right by
    -- default) or the head centre. GetVRMainEyeOffset says which, as its offset
    -- from the head centre along the camera's right axis; the eyes are the centre
    -- +-half IPD. (Guessing both sides around the camera put a phantom eye 6.4 cm
    -- away and zoomed a sight held there.)
    local mainEye = half
    if GetVRMainEyeOffset then
        local ok, offset = pcall(GetVRMainEyeOffset)
        if ok and type(offset) == 'number' then mainEye = offset end
    end
    local best, bestBehind
    for _, side in ipairs({-1, 1}) do
        local along = half*side - mainEye
        local vx = cx + right.x*along - (wp.x + s.x)
        local vy = cy + right.y*along - (wp.y + s.y)
        local vz = cz + right.z*along - (wp.z + s.z)
        local behind = -(vx*f.x + vy*f.y + vz*f.z)
        if behind > -0.03 and behind < maxBehind then
            local lx, ly, lz = vx + f.x*behind, vy + f.y*behind, vz + f.z*behind
            local d = math.sqrt(lx*lx + ly*ly + lz*lz)
            if not best or d < best then best, bestBehind = d, behind end
        end
    end
    return best, bestBehind, f, Vector4.new(wp.x + s.x, wp.y + s.y, wp.z + s.z, 1)
end

-- The shot leaves the sight line while the eye is on a sight (native SetVRSightLine): the
-- offset from the bore to the line through the sight, perpendicular to the barrel. Every
-- sight's mark looks parallel to the barrel, so a shot from the muzzle lands one sight height
-- (8-10 cm) below it at every range; from the sight line it lands on it.
local function sightLine(point, f)
    if not SetVRSightLine then return end
    if not point then
        if sight.lineOn then SetVRSightLine(0, 0, 0);sight.lineOn = false end
        return
    end
    local mx, my, mz = GetVRSharedSlot(200), GetVRSharedSlot(201), GetVRSharedSlot(202)
    if not (mx and my and mz) or mx*mx + my*my + mz*mz < 1 then return end
    local vx, vy, vz = point.x - mx, point.y - my, point.z - mz
    local along = vx*f.x + vy*f.y + vz*f.z
    SetVRSightLine(vx - f.x*along, vy - f.y*along, vz - f.z*along)
    sight.lineOn = true
end

local names = {}
local function cname(text)
    local name = names[text]
    if not name then name = CName.new(text);names[text] = name end
    return name
end

-- The zoom the aim state machine would send, at `level` and without the weapon's own
-- aim FOV (LocateCamera ties the weapon layer to the world's zoom, SetVRSightZoom).
-- zoomState Aimed is what lets the camera take it; the upper body stays unaimed, so
-- movement, spread and the arm pose are untouched. While the player aims the state
-- machine owns AnimFeature_AimPlayer and only the zoom level is ours. Both features
-- are re-sent every frame: aim exits reset them.
local function sendSightZoom(player, level, zoomed, aiming)
    local zoom = sight.zoomFeature or NewObject('AnimFeature_Zoom')
    sight.zoomFeature = zoom
    local fov = Game.GetCameraSystem():GetActiveCameraFOV()
    zoom.finalZoomLevel, zoom.weaponZoomLevel = level, 1
    zoom.weaponScopeFov, zoom.weaponAimFOV, zoom.worldFOV = fov, fov, fov
    zoom.zoomLevelNum, zoom.noWeaponAimInTime, zoom.noWeaponAimOutTime = 0, 0.2, 0.2
    zoom.shouldUseWeaponZoomStats, zoom.focusModeActive = false, false
    AnimationControllerComponent.ApplyFeatureToReplicate(player, cname('ZoomAnimData'), zoom)
    if aiming then return end
    local aim = sight.aimFeature or AnimFeature_AimPlayer.new()
    sight.aimFeature = aim
    aim:SetAimState(animAimState.Unaimed)
    aim:SetZoomState(zoomed and animAimState.Aimed or animAimState.Unaimed)
    aim:SetAimInTime(0.2)
    aim:SetAimOutTime(0.2)
    AnimationControllerComponent.ApplyFeatureToReplicate(player, cname('AnimFeature_AimPlayer'), aim)
end

local function sightDot(on)
    if sight.dot ~= on and SetVRSightDot then
        SetVRSightDot(on)
        sight.dot = on
    end
    M.sightDot = on
end

local function syncWeaponLayer(on)
    if sight.synced ~= on and SetVRSightZoom then
        SetVRSightZoom(on)
        sight.synced = on
    end
end

-- weapon == nil (or a path without open sights) lets the zoom go.
local function updateSightZoom(player, weapon, delta)
    local level = 1
    local rule = weapon and not state.smart and SIGHT_ZOOM[M.path]
    -- The aim state is read only when something below uses it: a sight rule, or a zoom of ours
    -- still to hand back. The scanner owns the zoom while it is up; UpperBody 6 = aimingState.
    local scanning, aiming = false, false
    if rule or sight.active then
        local machine = GetAllBlackboardDefs().PlayerStateMachine
        local board = Game.GetBlackboardSystem():GetLocalInstanced(player:GetEntityID(), machine)
        scanning = board:GetInt(machine.Vision) == 1
        aiming = board:GetInt(machine.UpperBody) == 6
    end
    local owned = scanning or (aiming and state.smart)
    local approach = 0
    if rule and not owned then
        local ok, d, behind, f, point = pcall(eyeToAxis, weapon, rule.behind)
        if not ok then d = nil end
        M.eyeToAxis = d
        sight.looking = d ~= nil and d < (sight.looking and LEAVE_AXIS or ENTER_AXIS)
        sightLine(sight.looking and M.shotFromSight and point or nil, f)
        if sight.looking and M.sightZoomEnabled then
            -- A split shader scope is brought closer by the view zoom itself. The eye is not
            -- moved in as well: nearer the eyepiece its glass grows faster than the far end of
            -- the tube, and the lens, magnifying less, reached the tube's edges (dark arcs in
            -- long_02/long_05 from x1.2-1.3 with the approach; none up to x1.5 without).
            local split = M.path == 'shader' and shaderZoom(M.magnification) > 1
            if split then level = shaderZoom(M.magnification) end
            if M.sightMode == 'zoom' then
                level = math.max(level, SIGHT_ZOOM_LEVEL)
            elseif not split then
                local cfg = M.approach
                approach = math.max(0, math.min(cfg.max, behind - math.max(cfg.relief, behind * cfg.keep)))
                sight.fx, sight.fy, sight.fz = f.x, f.y, f.z
            end
        end
    else
        sight.looking, M.eyeToAxis = false, nil
        sightLine(nil)
    end
    sightDot(sight.looking and state.dotSight == true)
    -- Eased both ways; leaving, the last axis carries it back out.
    sight.approach = sight.approach + (approach - sight.approach) * math.min(1, (delta or 0) / APPROACH_TIME)
    if math.abs(sight.approach - approach) < 0.0005 then sight.approach = approach end
    local a = sight.fx and sight.approach or 0
    local ox, oy, oz = (sight.fx or 0) * a, (sight.fy or 0) * a, (sight.fz or 0) * a
    if (ox ~= sight.ox or oy ~= sight.oy or oz ~= sight.oz) and SetVRSightApproach then
        SetVRSightApproach(ox, oy, oz)
        sight.ox, sight.oy, sight.oz = ox, oy, oz
    end
    M.sightApproach = a
    M.sightZoom = level
    if level > 1 then
        syncWeaponLayer(true)
        sight.hold = RELEASE_HOLD
        sendSightZoom(player, level, true, aiming)
        sight.active = true
        return
    end
    if sight.active then
        -- Back to what the state machine shows outside aiming; the scanner and a smart
        -- sniper's aim send their own.
        if not owned then sendSightZoom(player, 1, false, aiming) end
        sight.active = false
    end
    if sight.synced then
        sight.hold = sight.hold - (delta or 0)
        if sight.hold <= 0 then syncWeaponLayer(false) end
    end
end

local function update(delta)
    local player = Game.GetPlayer()
    if not player then
        state = {elapsed=0, originals={}, cameraOn=false};M.path='none';M.cameraFrame=nil
        sight.active, sight.looking, M.sightZoom = false, false, 1;syncWeaponLayer(false);sightDot(false);sightLine(nil)
        if (sight.ox ~= 0 or sight.oy ~= 0 or sight.oz ~= 0) and SetVRSightApproach then SetVRSightApproach(0, 0, 0) end
        sight.approach, sight.ox, sight.oy, sight.oz, M.sightApproach = 0, 0, 0, 0, 0;return
    end
    M.waitingForStereo=not stereoBound(player)
    if M.waitingForStereo and state.cameraOn then M.stats.stereoWaits=M.stats.stereoWaits+1 end
    if not M.enabled or not Game.VRPhysicalOpticsEnabled() or M.waitingForStereo then
        cameraOff(player)
        pcall(updateSightZoom, player, nil, delta)
        if state.weaponID then restoreWeapon() end
        M.path='none';return
    end
    local weapon = player:GetActiveWeapon()
    state.elapsed = state.elapsed + delta
    local key = weapon and tostring(weapon:GetEntityID().hash)
    if key ~= state.weaponKey or state.elapsed >= .2 then
        state.elapsed = 0
        scan(player, weapon)
    end
    local ok, err = pcall(updateSightZoom, player, weapon, delta)
    if not ok then M.sightError = tostring(err) end
    if weapon and state.profile and (M.path == 'shader' or M.path == 'camera') then
        ok, err = pcall(updateWidgets, weapon, delta)
        if not ok then M.widgetError = tostring(err) end
    end
    if M.path ~= 'camera' or not weapon then return end
    if not Game.VRPhysicalOpticsCameraReady() then
        M.waitingForStereo=true
        if state.cameraOn then M.stats.readyWaits=M.stats.readyWaits+1 end
        cameraOff(player)
        return
    end
    local camera = player:FindComponentByName(cname('vr_optics_camera'))
    if not camera then M.error='Optics camera missing';return end
    local q, wp = weapon:GetWorldOrientation(), weapon:GetWorldPosition()
    -- The catalog's player entity binds the camera to the WeaponRight slot, so its
    -- local transform is in that slot's frame and the engine carries it with the
    -- slot (and the weapon on it) when the frame renders. Placed relative to the
    -- player root it used the root's orientation at script time, and a body
    -- yaw-follow step landing later in the same frame turned the image with it --
    -- the scope view jerked a few times per head turn.
    if M.cameraFrame == nil then
        local ok, bound = pcall(function() return camera.parentTransform ~= nil end)
        M.cameraFrame = (ok and bound) and 'slot' or 'root'
    end
    local ep, eq = player:GetWorldPosition(), player:GetWorldOrientation()
    if M.cameraFrame == 'slot' then
        local slots = player:FindComponentByName(cname('ItemAttachmentSlots'))
        local ok, found, transform = pcall(slots.GetSlotTransform, slots, cname('WeaponRight'))
        if not (ok and found and transform) then M.error='WeaponRight slot unavailable';return end
        ep, eq = WorldPosition.ToVector4(transform.Position), transform.Orientation
    end
    local inverse = {i = -eq.i, j = -eq.j, k = -eq.k, r = eq.r}
    -- Some fixed sights (Osprey) report a cached scope-bone offset that differs
    -- from the visible aperture. Use its authored center in the weapon mesh.
    local authored = state.profile and state.profile.scopeOffset
    local offset = authored and {x = authored.x, y = authored.y, z = authored.z} or weapon:GetScopeOffset()
    offset.y = offset.y + .2
    if state.profile and state.profile.frontCamera then
        -- Rasetsu's rear LCD is not its optical sensor. Place that sensor at
        -- the native weapon front, retaining the LCD's optical height.
        offset.y = math.max(offset.y, weapon:GetMuzzleOffset().y + .02)
    end
    local world = rotate(q, offset)
    local lp = rotate(inverse, {x = wp.x+world.x-ep.x, y = wp.y+world.y-ep.y, z = wp.z+world.z-ep.z})
    camera:SetLocalTransform(Vector4.new(lp.x, lp.y, lp.z, 0), multiply(inverse,q))
    camera.fov = Policy.cameraFov(M.magnification)
    toggle(camera, true)
    state.cameraOn = true
end

-- The plugin draws a magnified scope's reticle on the glass and a collimator's
-- by angle; the game's reticle materials are the same for both, so it needs
-- the path chosen here (SetVRPhysicalOpticsPath).
local pathCodes = { none=0, collimator=1, shader=2, camera=3, native=4 }
local publishedPath
local function publishPath()
    local code = pathCodes[M.path] or 0
    if code ~= publishedPath and Game.SetVRPhysicalOpticsPath then
        Game.SetVRPhysicalOpticsPath(code)
        publishedPath = code
    end
end

M.update = update
registerForEvent('onUpdate', function(delta)
    local ok, err = pcall(M.update, delta)
    if not ok then M.error=tostring(err) end
    pcall(publishPath)
end)

registerForEvent('onShutdown', function()
    local player=Game.GetPlayer()
    if player then cameraOff(player);restoreWeapon() end
    if Game.SetVRPhysicalOpticsPath then pcall(Game.SetVRPhysicalOpticsPath, 0) end
    if SetVRSightApproach then pcall(SetVRSightApproach, 0, 0, 0) end
    if SetVRSightLine then pcall(SetVRSightLine, 0, 0, 0) end
    if SetVRSightDot then pcall(SetVRSightDot, false) end
    if SetVRSightZoom then pcall(SetVRSightZoom, false) end
end)

return M
