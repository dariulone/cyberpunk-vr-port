-- CyberpunkVRPort_Weapon -- "bullet from the weapon barrel" VR aim.
--
-- The F10 weapon-aim toggle selects controller 6DoF (Hand Aim) when enabled and HMD 3DoF
-- (Decoupled VR Head Aim) when disabled. BOTH launch from the live weapon muzzle, so nothing here
-- depends on which one is selected.
--
-- The aim ENABLE toggle lives in the VR imgui overlay (dxgi "Controls" -> "Bullet from weapon
-- barrel", writes shared[58]). This script:
--   1) installs the GetOrientation VMT instrument once (InstallVRProvInstrument) -- this ALSO
--      installs the override hooks that redirect the shot down the barrel (slot 33 / mode 6),
--   2) publishes the weapon muzzle world orientation each frame (SetVRMuzzleQuat) -- drives both
--      the launch override and the overlay barrel laser dot, and
--   3) publishes the live camera zoom (SetVRZoomLevel) for DIAGNOSTICS ONLY -- never use it to
--      scale the dot or the projection: MAIN's projection already carries the ADS magnification,
--      and applying both double-zooms ordinary weapons, and
--   4) VR MOTION MELEE: detects a real controller swing (weapon moved fast relative to the player)
--      and fires the game's NATIVE melee attack along the blade via redscript PlayerPuppet:VRMeleeSwing
--      (mod CyberpunkVRPort_Melee). The native box-sweep does collision/damage/reaction/stamina, so
--      it behaves like the flat game. A fast swing = power/cleave. No-op for guns (self-filtering).

-- ONE DEBUG SWITCH FOR THE WHOLE PORT, read from shared slot [156].
--
-- The plugin republishes the launcher's DEBUG checkbox there every frame, so this bridge obeys the
-- same switch as everything else and can be flipped without editing a file. It used to be a
-- hardcoded `local DEBUG = true`, which is how one session left 26 449 lines and 5 MB of per-frame
-- state in this mod's log alone.
--
-- Cached for a quarter second: this is called from onUpdate and a shared-memory read per frame to
-- decide whether to not log is a poor trade.
local dbgCache, dbgAt = false, -1.0
local function vrDebug()
    local now = (os and os.clock and os.clock()) or 0.0
    if now - dbgAt > 0.25 then
        dbgAt = now
        dbgCache = (type(GetVRSharedSlot) == 'function') and (GetVRSharedSlot(156) > 0.5) or false
    end
    return dbgCache
end

-- Routine chatter. Anything that must survive DEBUG=0 -- a failure, a one-time fact -- calls
-- logAlways instead.
local function logAlways(fmt, ...)
    local ok, s = pcall(string.format, fmt, ...)
    if ok then spdlog.info("[CyberpunkVRPort_Weapon] " .. s) end
end
local function logf(fmt, ...)
    if not vrDebug() then return end
    logAlways(fmt, ...)
end

local installed = false
local installTimer = 0.0

-- VR motion-melee tuning + state. A VR swing (the player's own hand = the animation) deals damage via
-- redscript on the touched enemy. NO RT injection (that would play the game's own attack animation).
local meleeEnabled = true
local meleePrevRel = nil       -- weapon pos relative to player, last frame (so walking != a swing)
local MELEE_SWING_SPEED = 2.5  -- m/s of weapon motion relative to player — peaks at 2-5 m/s on a real swing
local MELEE_BOX = 0.22         -- blade hit radius (m) — tight to NPC body silhouette

-- SWING WHOOSH: in the flat game the whoosh rides on the attack anim's audio events, which a VR
-- swing never plays — so redscript VRMeleeWhoosh replays the weapon's own audio-config whoosh
-- (per-family, positional on the weapon). Fired here on the swing EDGE: once per swing episode
-- (speed crossing the threshold re-arms only after the hand slows down), speed picks fast/normal.
-- Speed for the whoosh is measured over a ~90 ms SLIDING WINDOW of the blade offset, not per
-- frame: the weapon transform updates on its own cadence (not every render frame), so per-frame
-- speed alternates spike/zero and any "N consecutive frames" gate can never latch. The window
-- integrates across that. Teleports (snap turn rotates the whole rig -> rel jumps once) are cut
-- by the single-frame discontinuity check, which resets the window instead of whooshing.
local WHOOSH_SWING_SPEED = 3.0  -- m/s over the window: a REAL swing (hit gate 2.5 is contact-gated)
local WHOOSH_REARM_SPEED = 1.0  -- m/s: below this the swing is over -> re-arm
local WHOOSH_FAST_SPEED  = 4.2  -- m/s: at/above this play the fast whoosh variant
local WHOOSH_MIN_GAP     = 0.25 -- s: hard anti-spam floor between whooshes
local WHOOSH_WINDOW      = 0.09 -- s: sliding window span
local WHOOSH_MIN_SPAN    = 0.04 -- s: don't judge speed until the window has this much history
local WHOOSH_TELEPORT    = 0.30 -- m in ONE frame = rig teleport (snap/cut), reset the window
local WHOOSH_EQUIP_MUTE  = 0.8  -- s after a weapon change (the draw arm-raise is fast weapon motion)
local whooshDebug = false       -- log windowed speed peaks to the CET log for tuning
local whooshArmed = true
local whooshLast  = -1.0
local whooshWpnId = nil
local whooshEquipUntil = -1.0
local whooshBuf = {}            -- ring of {t, x, y, z} rel samples
local whooshPeak = 0.0          -- debug: per-episode peak
local whooshLastDir = nil       -- unit velocity at the last whoosh: a combo re-arms on REVERSAL
                                -- (in a fast series the hand never drops below the re-arm speed
                                -- between strokes — it flips direction instead)

-- WEAPON DRAW SOUND (any weapon, not only melee): the draw anim never plays in VR, so its equip
-- audio never fires; redscript VREquipSound replays the weapon's own equip event on the entity.
-- Trigger: active-weapon ENTITY changed (holster respawns the entity, so re-draws count too).
-- First observation after mod load is swallowed (savegame restore is not a draw).
local equipSndInit = false
local equipSndId = nil

-- VR GUARD — native block/deflect via the game's own mitigation STATS, ZERO debuffs.
-- damageManager.script gates the player's incoming-hit mitigation purely on stats:
--   IsBlocking==1   -> melee hits WasBlocked (stamina damage instead of health); with the Blades
--                      perk (Reflexes_Right_Perk_2_1) + stamina, bullets WasBulletDeflected;
--   IsDeflecting==1 -> melee hits WasDeflected('Parry' — the attacker staggers); with the perk,
--                      bullets WasBulletParried (reflected AT the shooter).
-- The flat game sets IsBlocking from the PSM Block state — which also drags in the debuffs
-- (AimWalk slow-walk, sprint interrupt, block anims). We set the stats DIRECTLY and never touch
-- the PSM (no LT, no 'MeleeBlock' action):
--   blade pointing FORWARD (thrust cone = attack intent)   -> guard OFF
--   blade in ANY other orientation (across/up/down/reverse) -> guard ON, same frame
--   guard OFF->ON transition -> IsDeflecting for a short PARRY window (raise-to-parry gesture,
--                               native stagger / perk bullet-reflect), then settles to IsBlocking
-- No PSM state => full walk/sprint speed, no anim events, frame-instant transitions both ways,
-- and it composes with swings freely (your own slash keeps the guard up unless the blade points
-- forward mid-arc — native-VR "always guarded" feel). Stamina still drains per blocked hit
-- natively (DealStaminaDamage), so it is not god mode.
local GUARD_THRUST_DOT   = 0.50  -- blade within ~60° of body-forward = attack intent, guard off
local GUARD_PARRY_WINDOW = 0.25  -- s of IsDeflecting right after guard entry (gesture parry)
local guardClock = 0.0           -- accumulated onUpdate time (drives the parry window)
local guardParryUntil = -1.0
local guardWasOn = false
local guardBlockMod = nil        -- IsBlocking stat modifier handle (applied = guarding)
local guardParryMod = nil        -- IsDeflecting stat modifier handle (applied = parry window)

-- Apply/remove the two stat modifiers to match the wanted phase. Idempotent per frame.

-- CAMERA RECOIL, KILLED AT THE WEAPON. The game kicks the player's heading on every shot -- measured
-- in the plugin at ~1 deg peak within 150 ms of a round, read straight out of the heading delta. In a
-- flat shooter that kick IS recoil; in VR you aim with the controller, so all of it lands on the head
-- and reads as a sideways jerk of the view. The hand recoil the plugin now applies replaces it.
--
-- A MULTIPLIER ON THE WEAPON, not 316 TweakXL entries. The values live in per-weapon inline stat
-- modifiers (Items.Base_<Weapon>_Constant_Stats_inline6/7 and friends -- 316 of them across the game,
-- and that list was written out before this was tried), so overriding them by data means enumerating
-- every weapon and still missing every modded one. One multiplier of zero on the equipped weapon
-- covers all of them, including weapons this mod has never heard of.
--
-- Re-applied when the weapon entity changes, because the modifier lives on the ENTITY: a fresh draw
-- is a fresh entity and starts with the game's own stats again.
-- Peak camera kick each weapon family owns, in the game's own degrees, lifted straight out of
-- TweakDB (max RecoilKickMax across that family's variants). This IS the per-weapon table -- the
-- live stats system was tried first and returns a flat zero for these, because the recoil system
-- reads them from the weapon RECORD and never registers them as tracked stats.
local RECOIL_KICK_BY_FAMILY = {
    ['achilles'] = 2.0,
    ['ajax'] = 1.3,
    ['ashura'] = 1.0,
    ['buck'] = 8.0,
    ['burya'] = 9.0,
    ['carnage'] = 12.0,
    ['chao'] = 0.4,
    ['copperhead'] = 0.9,
    ['cpo'] = 4.0,
    ['crusher'] = 5.6,
    ['defender'] = 0.466,
    ['dian'] = 0.24,
    ['grad'] = 8.0,
    ['guillotine'] = 0.65,
    ['hmg'] = 0.8,
    ['igla'] = 8.0,
    ['kappa'] = 0.24,
    ['kenshin'] = 0.8,
    ['kolac'] = 3.8,
    ['kyubi'] = 1.4,
    ['lexington'] = 1.0,
    ['liberty'] = 2.0,
    ['ma70'] = 0.6,
    ['masamune'] = 1.2,
    ['nekomata'] = 7.2,
    ['nova'] = 3.5,
    ['nue'] = 3.2,
    ['omaha'] = 2.8,
    ['overture'] = 4.0,
    ['palica'] = 5.6,
    ['pozhar'] = 5.4,
    ['pulsar'] = 0.85,
    ['quasar'] = 1.2,
    ['rocketlauncher'] = 15.0,
    ['saratoga'] = 0.6,
    ['satara'] = 9.6,
    ['senkoh'] = 0.8,
    ['shingen'] = 0.28,
    ['sidewinder'] = 0.56,
    ['silverhand'] = 1.9,
    ['slaughtomatic'] = 1.3,
    ['sor22'] = 3.5,
    ['tactician'] = 11.0,
    ['testera'] = 10.4,
    ['umbra'] = 1.05,
    ['unity'] = 2.25,
    ['yukimura'] = 0.7,
    ['zhuo'] = 2.8,
}

-- FELT RECOIL, WHERE THE GAME'S NUMBER IS NOT THE ANSWER. The table above is TweakDB's own kick and it
-- is what every weapon is scaled by; these are the ones where the result was judged in the headset and
-- came back wrong. Kept separate from the generated table on purpose -- that one is data and must stay
-- regenerable, this one is a handful of measured corrections with a reason each.
--
--   unity  2.25 -> 0.87   Its own number puts it at 38.4 deg, which is inside the ceiling's compression
--                         where every heavy pistol lands within four degrees of every other. Halved on
--                         the user's call after firing it: 19.1 deg, and it sits below the knee again,
--                         where the ladder still has room to tell weapons apart.
local RECOIL_KICK_OVERRIDE = {
    ['unity'] = 0.87,
}

local recoilKilled = nil       -- entity id hash the modifiers are currently attached to
-- THE KICK IS CACHED BY WEAPON TYPE, and that is not an optimisation.
--
-- The multiplier that kills the camera kick lives on the weapon ENTITY, so the second time the same gun
-- is drawn its RecoilKickMax already reads 0 -- our own doing. Reading it then and publishing it would
-- hand the plugin a zero; skipping the publish (which is what happened) leaves the plugin on the
-- PREVIOUS weapon's number. Measured exactly that way: Overture in hand, 0.989 published, which is the
-- Lexington. Cached by record id, the first honest read is the one that counts, for every later draw.
local recoilKickCache = {}
local RECOIL_STATS = { 'RecoilKickMin', 'RecoilKickMax', 'RecoilKickMinADS', 'RecoilKickMaxADS',
                       'RecoilAngle', 'RecoilAngleADS' }
local function killCameraRecoil(wpn, wid)
    if not wpn or recoilKilled == wid then return end
    -- WHICH WEAPON, FROM ITS RECORD NAME. `Items.Preset_Lexington_Neon` -> "lexington". The table above
    -- holds the game's own peak kick per family, which is the number the hand spring scales its angle
    -- and its settle time by.
    --
    -- READING IT LIVE DOES NOT WORK, and that is measured, not assumed: GetStatValue returns 0 for
    -- RecoilKickMax whether it is asked with the weapon's entity id or with the StatsObjectID its item
    -- data carries (log: `kick=0 sid=true`). The recoil system takes these out of the weapon record and
    -- never registers them as tracked stats, so the value has to come from the record side -- and a
    -- table generated from TweakDB is exactly that, just resolved ahead of time.
    local key = nil
    pcall(function() key = TDBID.ToStringDEBUG(ItemID.GetTDBID(wpn:GetItemID())) end)
    -- AND FROM friendlyName, because the record name is not always the weapon's name. Quest and iconic
    -- guns are named after the quest that hands them out: River's revolver is `Items.sq029_rivers_gun`,
    -- which says nothing about what it is -- measured as `kick=nil` in the probe. Its friendlyName is
    -- `w_revolver_malorian_overture`, which says everything, and it is the same identifier the animation
    -- paths use. Both strings are searched, so a weapon has to hide its family from both to be missed.
    local fname = nil
    if key then pcall(function() fname = TweakDB:GetFlat(key .. '.friendlyName') end) end
    local kick = nil
    local famName = nil
    if key then
        local low = string.lower(key .. ' ' .. tostring(fname or ''))
        -- LONGEST MATCH WINS, because `pairs` has no order and a record name can contain more than one
        -- family: whichever key happened to come first would decide, and it would decide differently
        -- from run to run. The longest match is the specific one.
        local best = 0
        for fam, v in pairs(RECOIL_KICK_BY_FAMILY) do
            if #fam > best and string.find(low, fam, 1, true) then kick = v; famName = fam; best = #fam end
        end
    end
    -- ...AND THE NAME ITSELF, for the two-hand grip. That hold is a property of the weapon -- a pistol's
    -- support hand is on the same grip, a rifle's is out on the handguard -- so the plugin keeps one
    -- captured file per weapon and needs a name to key it by. It cannot work one out on its own: the rig
    -- signature identifies only the thirteen weapons the reload knows, while the family is already
    -- resolved right here. Per DRAW, not per frame, so the pose path still takes nothing from CET.
    -- WEAPONS THE RECOIL TABLE DOES NOT NAME still need a name for their two-hand hold. Ticon and
    -- Tamayura have no RecoilKickMax rows in TweakDB, so the family match above finds nothing and the
    -- fallback was the RECORD id -- which produced CyberpunkVR_TwoHandGrip_itemscraftable_legendary_ticon
    -- .ini, a file keyed to one VARIANT: capture on the legendary and the common one has no hold at all.
    -- These two tokens are the same ones the reload module matches those weapons by.
    if not famName and key then
        local low2 = string.lower(key .. ' ' .. tostring(fname or ''))
        for _, w in ipairs({ 'ticon', 'tamayura' }) do
            if string.find(low2, w, 1, true) then famName = w break end
        end
    end
    if type(SetVRWeaponName) == 'function' then
        SetVRWeaponName(famName or (key and string.lower(key)) or '')
    end
    -- NEVER LEAVE THE PREVIOUS WEAPON'S NUMBER IN PLACE. Publishing nothing when a weapon is unknown
    -- means the hand keeps kicking like whatever was drawn before it -- silently, and wrongly. The
    -- reference kick is the honest answer to "unknown": it is the Lexington, i.e. the angle that was
    -- tuned in the headset.
    if famName and RECOIL_KICK_OVERRIDE[famName] then kick = RECOIL_KICK_OVERRIDE[famName] end
    if not (kick and kick > 0.0) then kick = 1.0 end
    if type(SetVRWeaponKick) == 'function' then SetVRWeaponKick(kick) end
    logAlways('recoil: key=%s kick=%s', tostring(key), tostring(kick))
    -- STRAIGHT TO A FILE, because the module's spdlog log stopped accepting lines after a mod reload
    -- (the file was reopened and nothing more was appended, while the value provably reached the
    -- plugin). A diagnostic that can go quiet is worse than none: this one is opened, written, flushed
    -- and closed on the spot, so what it says is what happened.
    pcall(function()
        local f = io.open('recoil_probe.txt', 'a')
        if f then
            f:write(string.format('key=%s kick=%s wid=%s', tostring(key), tostring(kick), tostring(wid)))
            f:write(string.char(10))
            f:close()
        end
    end)

    local sid = nil
    pcall(function()
        local data = Game.GetTransactionSystem():GetItemData(Game.GetPlayer(), wpn:GetItemID())
        sid = data and data:GetStatsObjectID() or nil
    end)
    local ok2, err = pcall(function()
        local ss = Game.GetStatsSystem()
        for _, name in ipairs(RECOIL_STATS) do
            local st = gamedataStatType[name]
            if st then
                -- Multiplier, not Additive: the kick is a positive number the weapon owns, and only a
                -- factor of zero removes it whatever that number is.
                ss:AddModifier(sid or wpn:GetEntityID(),
                               RPGManager.CreateStatModifier(st, gameStatModifierType.Multiplier, 0.0))
            end
        end
    end)
    if ok2 then recoilKilled = wid else logAlways('recoil: kill failed: %s', tostring(err)) end
end

local function guardStats(pl, wantParry, wantBlock)
    local ss = Game.GetStatsSystem()
    local id = pl:GetEntityID()
    if wantParry and not guardParryMod then
        guardParryMod = RPGManager.CreateStatModifier(gamedataStatType.IsDeflecting, gameStatModifierType.Additive, 1.0)
        ss:AddModifier(id, guardParryMod)
    elseif not wantParry and guardParryMod then
        ss:RemoveModifier(id, guardParryMod)
        guardParryMod = nil
    end
    if wantBlock and not guardBlockMod then
        guardBlockMod = RPGManager.CreateStatModifier(gamedataStatType.IsBlocking, gameStatModifierType.Additive, 1.0)
        ss:AddModifier(id, guardBlockMod)
    elseif not wantBlock and guardBlockMod then
        ss:RemoveModifier(id, guardBlockMod)
        guardBlockMod = nil
    end
end

-- Publish the muzzle world orientation. The plugin (SetVRMuzzleQuat) uses it for the launch
-- override (bullet leaves the barrel) AND writes the muzzle forward to shared mem for the
-- overlay's barrel laser dot.
local muzzlePosWarned = false
local muzzlePosProbed = false
local muzzleEnumDone = false
local function updateMuzzle(wpn)
    local xf = wpn:GetMuzzleSlotWorldTransform()
    if not xf then return end
    local q = xf.Orientation or (xf.GetOrientation and xf:GetOrientation())
    if q and type(SetVRMuzzleQuat) == 'function' then
        SetVRMuzzleQuat(q.i, q.j, q.k, q.r)
    end
    -- The POSITION half of the same transform, which used to be dropped on the floor. The launch
    -- override replaced the shot's direction with the muzzle's and left its origin at the game
    -- camera -- i.e. at the left eye -- so the bullet flew parallel to the barrel but started an
    -- IPD away from the eye that was aiming. pcall'd because a wrong accessor here would take the
    -- whole weapon mod down with it, and with it the barrel dot and the aim override.
    -- Position, fetched the same way the line above fetches orientation: field first, then
    -- getter. The probe said xf.Position is nil outright, and WorldTransform exposes it as
    -- GetWorldPosition() -- exactly the field-or-method shape already used for the quaternion.
    -- Enumerated, not guessed: this object exposes `position` (lower case) and GetPosition().
    -- `Position` and GetWorldPosition() -- the two names I tried first -- do not exist on it.
    local pos = xf.position
    if not pos and xf.GetPosition then pos = xf:GetPosition() end

    -- Neither exists on this object, so stop guessing names one per round-trip and ask it what
    -- it has. Orientation is the control: that one is known to work, so if it does not show up
    -- in the listing then the listing itself is the thing that does not work here.
    if not pos and not muzzleEnumDone then
        muzzleEnumDone = true
        local keys = {}
        pcall(function()
            for k, _ in pairs(xf) do keys[#keys + 1] = tostring(k) end
        end)
        logf("muzzle xf: type=%s  keys=[%s]", type(xf), table.concat(keys, ", "))
        local names = { 'Position', 'position', 'WorldPosition', 'Translation', 'Trans',
                        'Orientation', 'GetWorldPosition', 'GetPosition', 'ToVector4',
                        'GetOrientation' }
        local found = {}
        for _, n in ipairs(names) do
            local t = 'nil'
            pcall(function() t = type(xf[n]) end)
            if t ~= 'nil' then found[#found + 1] = n .. '=' .. t end
        end
        logf("muzzle xf members: %s", table.concat(found, "  "))
        -- And the weapon itself, in case the muzzle position is reachable from there instead.
        local wt = 'nil'
        pcall(function() wt = tostring(wpn:GetWorldPosition()) end)
        logf("muzzle fallback: wpn:GetWorldPosition() = %s", wt)
    end

    if type(SetVRMuzzlePos) == 'function' and pos then
        local x, y, z
        -- WorldPosition keeps 17-bit fixed point, the same 1/131072 the render camera and the
        -- instance transforms use; a plain Vector4 keeps floats. Take whichever this build has.
        pcall(function()
            if type(pos.x) == 'number' then
                x, y, z = pos.x, pos.y, pos.z
            elseif pos.x and pos.x.Bits then
                x, y, z = pos.x.Bits / 131072.0, pos.y.Bits / 131072.0, pos.z.Bits / 131072.0
            elseif pos.ToVector4 then
                local v = pos:ToVector4()
                if v then x, y, z = v.x, v.y, v.z end
            elseif WorldPosition and WorldPosition.ToVector4 then
                local v = WorldPosition.ToVector4(pos)
                if v then x, y, z = v.x, v.y, v.z end
            end
        end)
        if x then
            SetVRMuzzlePos(x, y, z)
            if not muzzlePosProbed then
                muzzlePosProbed = true
                logf("muzzlePos OK: (%.4f, %.4f, %.4f)", x, y, z)
            end
        elseif not muzzlePosWarned then
            muzzlePosWarned = true
            logf("muzzlePos: got %s but no component accessor matched (x type=%s)",
                 type(pos), type(pos.x))
        end
    elseif not muzzlePosWarned then
        muzzlePosWarned = true
        logf("muzzlePos: native=%s pos=%s", type(SetVRMuzzlePos), type(pos))
    end
end

-- Trace the weapon's bore against world collision on CET's game-script side. The current
-- CET/CP2077 build exposes the hit as TraceResult.position; this same contract is already used by
-- the physical-reload raycast in this tree. Keep physics out of the DXGI Present path.
local BARREL_RAY_MAX_M = 1000.0
local BARREL_RAY_PRESET = 'Sight Blocker'
local BARREL_LOS_ENDPOINT_OFFSET_M = 0.005
local BARREL_LOS_GLASS_STEP_M = 0.005
local BARREL_LOS_MAX_GLASS_HITS = 8
local BARREL_EYE_MAX_AGE_S = 0.020
local barrelRaySystem = nil
local barrelRayCallWarned = false
local barrelNpcCallWarned = false
local barrelLosCallWarned = false
local barrelEyeLastSeq = 0
local barrelEyeAgeS = math.huge
local barrelLosLogAt = -1.0
local barrelRayWasActive = false

local function publishBarrelRayMiss(mainVisible, secondVisible)
    if type(SetVRBarrelRayHit) == 'function' then
        SetVRBarrelRayHit(0.0, 0.0, 0.0, 0,
            mainVisible == false and 0 or 1,
            secondVisible == false and 0 or 1)
    end
end

local function stopBarrelRay()
    if barrelRayWasActive then
        publishBarrelRayMiss()
        barrelRayWasActive = false
    end
end

-- Read the two eye origins as one render-side snapshot. A missing or stale packet disables only
-- occlusion (fail-open); it never disables the existing mode-2 hit provider or direction marker.
local function readBarrelEyes(dt)
    barrelEyeAgeS = barrelEyeAgeS + math.max(dt or 0.0, 0.0)
    for _ = 1, 3 do
        local s0 = math.floor(GetVRSharedSlot(178) or 0.0)
        if s0 > 0 and s0 % 2 == 0 then
            local mx, my, mz = GetVRSharedSlot(172), GetVRSharedSlot(173), GetVRSharedSlot(174)
            local sx, sy, sz = GetVRSharedSlot(175), GetVRSharedSlot(176), GetVRSharedSlot(177)
            local s1 = math.floor(GetVRSharedSlot(178) or 0.0)
            if s0 == s1 and s1 % 2 == 0 then
                if s1 ~= barrelEyeLastSeq then
                    barrelEyeLastSeq = s1
                    barrelEyeAgeS = 0.0
                end
                local mainLen2 = mx*mx + my*my + mz*mz
                local secondLen2 = sx*sx + sy*sy + sz*sz
                local ex, ey, ez = mx - sx, my - sy, mz - sz
                local separation2 = ex*ex + ey*ey + ez*ez
                if barrelEyeAgeS <= BARREL_EYE_MAX_AGE_S and mainLen2 > 1.0 and
                   secondLen2 > 1.0 and separation2 > 0.0001 and separation2 < 0.04 then
                    return Vector4.new(mx, my, mz, 1.0), Vector4.new(sx, sy, sz, 1.0)
                end
                return nil, nil
            end
        end
    end
    return nil, nil
end

local function makeBarrelLosEnd(eye, finalHit, fx, fy, fz)
    if finalHit then
        local dx, dy, dz = finalHit.x - eye.x, finalHit.y - eye.y, finalHit.z - eye.z
        local d2 = dx*dx + dy*dy + dz*dz
        if d2 <= BARREL_LOS_ENDPOINT_OFFSET_M * BARREL_LOS_ENDPOINT_OFFSET_M then
            return nil
        end
        local invD = 1.0 / math.sqrt(d2)
        return Vector4.new(
            finalHit.x - dx * invD * BARREL_LOS_ENDPOINT_OFFSET_M,
            finalHit.y - dy * invD * BARREL_LOS_ENDPOINT_OFFSET_M,
            finalHit.z - dz * invD * BARREL_LOS_ENDPOINT_OFFSET_M,
            1.0)
    end
    -- A finite physics segment tests visibility only. The rendered fallback remains optical
    -- infinity; each eye gets its own parallel segment along the same muzzle direction.
    return Vector4.new(eye.x + fx * BARREL_RAY_MAX_M,
                       eye.y + fy * BARREL_RAY_MAX_M,
                       eye.z + fz * BARREL_RAY_MAX_M, 1.0)
end

-- Keep the exact EntityID selected by the synchronous redscript nearest-hit query. Identity is an
-- optional side channel only: a getter/marshalling failure must never discard an otherwise valid
-- NPC surface hit, because XYZ/W keep their existing contract and Ray B must remain usable.
local function queryNpcBarrelRay(player, rayStart, rayEnd)
    if not player or not player.VRFindNpcBarrelRayHit then return nil, nil end
    local hit = player:VRFindNpcBarrelRayHit(rayStart, rayEnd)
    if not hit or (hit.w or 0.0) <= 0.5 then return hit, nil end
    if not player.VRGetNpcBarrelRayEntity then return hit, nil end

    local idOk, entityKey = pcall(function()
        local entityId = player:VRGetNpcBarrelRayEntity()
        if not entityId then return nil end
        return tostring(entityId.hash)
    end)
    if not idOk then return hit, nil end
    return hit, entityKey
end

local function barrelDotVisibleFromEye(player, eye, finalHit, finalNpcKey, fx, fy, fz)
    local rayEnd = makeBarrelLosEnd(eye, finalHit, fx, fy, fz)
    if not rayEnd then return true, 0.0, 'near' end

    -- Use the same world collision semantics already validated by the muzzle provider. The first
    -- probe's generic QueryFilter missed vehicles and added no proven coverage. Glass materials are
    -- transparent to the dot LOS: step through each glass hit on the same segment rather than
    -- declaring the whole remaining segment clear and missing an opaque blocker behind the pane.
    local dx, dy, dz = rayEnd.x - eye.x, rayEnd.y - eye.y, rayEnd.z - eye.z
    local rayLengthSq = dx*dx + dy*dy + dz*dz
    if rayLengthSq <= 0.000001 then return true, 0.0, 'near' end
    local invRayLength = 1.0 / math.sqrt(rayLengthSq)
    local dirX, dirY, dirZ = dx * invRayLength, dy * invRayLength, dz * invRayLength
    local worldStart = eye
    local glassHits = 0
    for _ = 1, BARREL_LOS_MAX_GLASS_HITS + 1 do
        local worldOk, worldSuccess, worldResult = pcall(function()
            return barrelRaySystem:SyncRaycastByQueryPreset(
                worldStart, rayEnd, BARREL_RAY_PRESET, false)
        end)
        if not worldOk then
            if not barrelLosCallWarned then
                barrelLosCallWarned = true
                logAlways('barrel world LOS failed; visibility remains fail-open: %s',
                    tostring(worldSuccess))
            end
            return true, -1.0, 'world-error'
        end
        if not (worldSuccess and worldResult and worldResult.position) then
            break
        end

        local p = worldResult.position
        local materialName = string.lower(tostring(worldResult.material or ''))
        if not string.find(materialName, 'glass', 1, true) then
            local bx, by, bz = p.x - eye.x, p.y - eye.y, p.z - eye.z
            return false, math.sqrt(bx*bx + by*by + bz*bz), 'world'
        end

        glassHits = glassHits + 1
        if glassHits > BARREL_LOS_MAX_GLASS_HITS then
            break
        end
        worldStart = Vector4.new(
            p.x + dirX * BARREL_LOS_GLASS_STEP_M,
            p.y + dirY * BARREL_LOS_GLASS_STEP_M,
            p.z + dirZ * BARREL_LOS_GLASS_STEP_M,
            1.0)
        local rx, ry, rz = rayEnd.x - worldStart.x, rayEnd.y - worldStart.y,
                           rayEnd.z - worldStart.z
        if rx*dirX + ry*dirY + rz*dirZ <= 0.0 then
            break
        end
    end

    -- Sight Blocker intentionally ignores character bodies. Only after the world segment is clear
    -- ask the already-validated official HitRepresentation provider whether an NPC blocks it.
    local npcOk, npcResult, npcEntityKey = pcall(function()
        return queryNpcBarrelRay(player, eye, rayEnd)
    end)
    if not npcOk then
        if not barrelLosCallWarned then
            barrelLosCallWarned = true
            logAlways('barrel NPC LOS failed; visibility remains fail-open: %s',
                tostring(npcResult))
        end
        return true, -1.0, 'npc-error'
    end
    if npcResult and (npcResult.w or 0.0) > 0.5 then
        local bx, by, bz = npcResult.x - eye.x, npcResult.y - eye.y, npcResult.z - eye.z
        local blockM = math.sqrt(bx*bx + by*by + bz*bz)
        -- Once the muzzle ray has selected this NPC as the actual receiving surface, its own
        -- overlapping HitRepresentation shapes must not make the aiming dot disappear. A
        -- different NPC still blocks normally, as do the world/vehicle/prop checks above.
        if finalNpcKey and npcEntityKey and npcEntityKey == finalNpcKey then
            return true, blockM,
                glassHits > 0 and ('npc-self-glass' .. tostring(glassHits)) or 'npc-self'
        end
        return false, blockM,
            glassHits > 0 and ('npc-glass' .. tostring(glassHits)) or 'npc'
    end
    return true, -1.0, glassHits > 0 and ('clear-glass' .. tostring(glassHits)) or 'clear'
end

local function updateBarrelRay(dt)
    if type(GetVRSharedSlot) ~= 'function' then return end
    if type(SetVRBarrelRayHit) ~= 'function' then return end
    if GetVRSharedSlot(181) < 0.5 then
        stopBarrelRay()
        return
    end
    barrelRayWasActive = true
    if GetVRSharedSlot(27) < 0.5 or GetVRSharedSlot(203) < 0.5 then
        publishBarrelRayMiss()
        return
    end

    local px, py, pz = GetVRSharedSlot(200), GetVRSharedSlot(201), GetVRSharedSlot(202)
    local fx, fy, fz = GetVRSharedSlot(24), GetVRSharedSlot(25), GetVRSharedSlot(26)
    local f2 = fx * fx + fy * fy + fz * fz
    if px * px + py * py + pz * pz < 1.0 or f2 < 0.25 then
        publishBarrelRayMiss()
        return
    end

    local invF = 1.0 / math.sqrt(f2)
    fx, fy, fz = fx * invF, fy * invF, fz * invF
    local from = Vector4.new(px, py, pz, 1.0)
    local to = Vector4.new(px + fx * BARREL_RAY_MAX_M,
                           py + fy * BARREL_RAY_MAX_M,
                           pz + fz * BARREL_RAY_MAX_M, 1.0)
    if barrelRaySystem == nil then
        barrelRaySystem = Game.GetSpatialQueriesSystem() or false
    end
    if not barrelRaySystem then
        publishBarrelRayMiss()
        return
    end

    -- Sight Blocker with dynamic collision covers world, props, vehicles and glass without
    -- self-hitting the tested player hands or weapons. NPC body surfaces use Ray B below.
    local worldHit, worldDistanceSq = nil, nil
    local success, result = barrelRaySystem:SyncRaycastByQueryPreset(
        from, to, BARREL_RAY_PRESET, false)
    if success and result then
        local p = result.position
        if p and type(p.x) == 'number' and type(p.y) == 'number' and type(p.z) == 'number' then
            local dx, dy, dz = p.x - px, p.y - py, p.z - pz
            worldHit = p
            worldDistanceSq = dx * dx + dy * dy + dz * dz
        end
    end

    -- Ray B uses the exact same from/to and has no cross-frame cache: W=0 is an immediate miss.
    -- Targeting supplies and deduplicates candidates in redscript; native HitRepresentation gives
    -- the closest animated body-surface enter point for each candidate.
    local player = Game.GetPlayer()
    local npcHit, npcDistanceSq = nil, nil
    local npcOk, npcResult, npcEntityKey = pcall(function()
        return queryNpcBarrelRay(player, from, to)
    end)
    if npcOk then
        if npcResult and (npcResult.w or 0.0) > 0.5 and
           type(npcResult.x) == 'number' and type(npcResult.y) == 'number' and
           type(npcResult.z) == 'number' then
            local dx, dy, dz = npcResult.x - px, npcResult.y - py, npcResult.z - pz
            npcHit = npcResult
            npcDistanceSq = dx * dx + dy * dy + dz * dz
        end
    else
        if not barrelNpcCallWarned then
            barrelNpcCallWarned = true
            logAlways('barrel ray: NPC surface query failed; Ray A remains active: %s',
                tostring(npcResult))
        end
    end

    local finalHit, finalDistanceSq, finalNpcKey = worldHit, worldDistanceSq, nil
    if npcHit and (not finalDistanceSq or npcDistanceSq < finalDistanceSq) then
        finalHit, finalDistanceSq, finalNpcKey = npcHit, npcDistanceSq, npcEntityKey
    end

    local mainVisible, secondVisible = true, true
    local mainBlockM, secondBlockM = -1.0, -1.0
    local mainSource, secondSource = 'no-eyes', 'no-eyes'
    local mainEye, secondEye = readBarrelEyes(dt)
    if player and mainEye and secondEye then
        mainVisible, mainBlockM, mainSource = barrelDotVisibleFromEye(
            player, mainEye, finalHit, finalNpcKey, fx, fy, fz)
        secondVisible, secondBlockM, secondSource = barrelDotVisibleFromEye(
            player, secondEye, finalHit, finalNpcKey, fx, fy, fz)
    end
    if vrDebug() then
        local now = (os and os.clock and os.clock()) or 0.0
        if now - barrelLosLogAt >= 1.0 then
            barrelLosLogAt = now
            logAlways('barrel LOS: eyes=%d hit=%d main=%s/%s block=%.3fm second=%s/%s block=%.3fm',
                (mainEye and secondEye) and 1 or 0,
                finalHit and 1 or 0,
                mainVisible and 'clear' or 'blocked', mainSource, mainBlockM,
                secondVisible and 'clear' or 'blocked', secondSource, secondBlockM)
        end
    end
    if finalHit then
        SetVRBarrelRayHit(finalHit.x, finalHit.y, finalHit.z, 1,
            mainVisible and 1 or 0, secondVisible and 1 or 0)
        return
    end

    publishBarrelRayMiss(mainVisible, secondVisible)
end

registerForEvent('onInit', function()
    logf("weapon-aim init")
end)

registerForEvent('onUpdate', function(dt)
    -- install the GetOrientation VMT instrument + override hooks once, after RTTI is ready
    if not installed then
        installTimer = installTimer + (dt or 0.016)
        if installTimer > 3.0 and type(InstallVRProvInstrument) == 'function' then
            local r = 0
            pcall(function() r = InstallVRProvInstrument() end)
            -- The weapon-aim family: XFORM-GETTER, the shot bracket and physArgSnapshot. All of it has
            -- been sitting at installed=0, which is why every one of those counters reads zero in the
            -- dump. It is what the launch ORIGIN has to be found with -- the provider slots do not
            -- carry it (slots 3..42 return nothing that looks like a world position). Read-only until
            -- something is told to mutate.
            if type(InstallWeaponAimHook) == 'function' then
                logf('InstallWeaponAimHook = %s', tostring(InstallWeaponAimHook()))
            else
                logf('InstallWeaponAimHook: native missing')
            end
            logf("InstallVRProvInstrument = %s", tostring(r))
            installed = true
        end
    end

    pcall(function()
        local pl = Game.GetPlayer()
        local wpn = pl and pl:GetActiveWeapon()
        -- THE MUZZLE GOES FIRST, AND NOTHING IS ALLOWED IN FRONT OF IT. Everything in this callback
        -- shares one pcall, so whatever runs first owns the frame: put something ahead of this line and
        -- a throw in it stops the muzzle quaternion from being published at all, the plugin keeps
        -- yesterday's orientation, and the bullet leaves the barrel pointing the wrong way. That was
        -- tried -- the recoil block was moved above this line to isolate it -- and the aim broke
        -- immediately. Isolation belongs in the OTHER direction: the muzzle keeps its place and the
        -- newcomer gets its own pcall.
        if wpn then updateMuzzle(wpn) end
        local okRay, errRay = pcall(function()
            if wpn then updateBarrelRay(dt) else stopBarrelRay() end
        end)
        if not okRay then
            publishBarrelRayMiss()
            if not barrelRayCallWarned then
                barrelRayCallWarned = true
                logAlways('barrel ray: query failed: %s', tostring(errRay))
            end
        end
        if wpn then
            local wid = nil
            pcall(function() wid = tostring(wpn:GetEntityID().hash) end)
            local okR, errR = pcall(killCameraRecoil, wpn, wid)
            if not okR then
                pcall(function()
                    local f = io.open('recoil_probe.txt', 'a')
                    if f then f:write('killCameraRecoil threw: ' .. tostring(errR) .. string.char(10)); f:close() end
                end)
            end
        end

        -- weapon draw sound (see equipSnd* header): fires on entity change, any weapon class
        local curWid = nil
        if wpn then pcall(function() curWid = tostring(wpn:GetEntityID().hash) end) end
        if not equipSndInit then
            equipSndInit = true
            equipSndId = curWid
        elseif curWid ~= equipSndId then
            equipSndId = curWid
            recoilKilled = nil
            if wpn and pl and pl.VREquipSound then
                pcall(function() pl:VREquipSound(wpn) end)
            end
        end

        -- Publish the LIVE camera zoom so the dxgi overlay scales the barrel laser dot by the real
        -- DIAGNOSTIC ONLY (scope changes GetZoom, NOT FOV; PSM.ZoomLevel is only a level index).
        -- The overlay takes ADS magnification from MAIN's own projection, not from this.
        if type(SetVRZoomLevel) == 'function' then
            local cam = pl and pl:GetFPPCameraComponent()
            if cam then
                local z = cam:GetZoom()
                if z and z > 0.0 then SetVRZoomLevel(z) end
            end
        end

        guardClock = guardClock + (dt or 0.016)

        -- VR MOTION MELEE: probe every frame the weapon is being SWUNG (speed relative to the player,
        -- so walking/turning doesn't count); the redscript helper does precise per-NPC enter detection
        -- and queues a native damage hit. Detection requires a melee weapon in the right hand AND the
        -- redscript helper VRMeleeBladeHit to be compiled in.
        if not (meleeEnabled and pl and wpn and pl.VRMeleeBladeHit) then
            -- guard cleanup on unequip/holster/death: the stats must never outlive the blade in hand
            if pl then guardStats(pl, false, false) else guardParryMod = nil; guardBlockMod = nil end
            guardWasOn = false
            return
        end
        local isMelee = false
        pcall(function() isMelee = WeaponObject.IsMelee(wpn:GetItemID()) end)
        if not isMelee then
            guardStats(pl, false, false)
            guardWasOn = false
            return
        end

        local wp = wpn:GetWorldPosition()
        local pp = pl:GetWorldPosition()
        local q = GetSingleton('Quaternion')
        local fwd = q and q:GetForward(wpn:GetWorldOrientation())
        if not (wp and pp and fwd) then return end

        -- Blade offset from the player, WORLD axes (translation-compensated). NOTE: do NOT rotate
        -- this into the body's local frame — in this port the body heading follows the HMD and
        -- micro-jitters every frame; with a ~0.5 m lever arm that basis jitter reads as a constant
        -- phantom 1-2 m/s, which starves the whoosh re-arm (no swing sounds) while the equip
        -- transient still fires. World frame is also the physically right frame for a whoosh
        -- (speed through the AIR); snap-turn teleport spikes are 1-frame and die on the hold gate.
        local rel = { x = wp.x - pp.x, y = wp.y - pp.y, z = wp.z - pp.z }

        -- Blade speed relative to the player (walking is not a swing), from the last frame.
        local speed = 0.0
        if meleePrevRel then
            local dx, dy, dz = rel.x - meleePrevRel.x, rel.y - meleePrevRel.y, rel.z - meleePrevRel.z
            speed = math.sqrt(dx*dx + dy*dy + dz*dz) / math.max(dt or 0.016, 0.001)
        end
        meleePrevRel = rel

        -- VR GUARD decision (see the header above): guard ON unless the blade points into the
        -- forward thrust cone. thrust = dot(normalized 3D blade fwd, normalized horizontal body
        -- fwd): forward-horizontal ≈ 1 (no guard), up/down/across ≈ 0, reverse < 0 (guard).
        local guardOn = false
        local pfwd = pl:GetWorldForward()
        if pfwd then
            local pfx, pfy = pfwd.x, pfwd.y
            local pfl = math.sqrt(pfx*pfx + pfy*pfy)
            local bfx, bfy, bfz = fwd.x, fwd.y, fwd.z
            local bfl = math.sqrt(bfx*bfx + bfy*bfy + bfz*bfz)
            if pfl > 0.001 and bfl > 0.001 then
                local thrust = (bfx*pfx + bfy*pfy) / (pfl * bfl)
                guardOn = thrust < GUARD_THRUST_DOT
            end
        end
        if guardOn and not guardWasOn then
            guardParryUntil = guardClock + GUARD_PARRY_WINDOW   -- fresh raise => parry window
        end
        guardWasOn = guardOn
        if guardOn then
            local parry = guardClock < guardParryUntil
            guardStats(pl, parry, not parry)
        else
            guardStats(pl, false, false)
        end

        -- Swing fires independently of the guard: the stat-based block has no attack-exit
        -- semantics (that was a PSM concept), and mid-swing the blade usually leaves the thrust
        -- cone anyway. Native stamina drain on blocked hits keeps block+slash honest.
        -- equip mute: weapon changed -> the draw motion is fast, silence the whoosh window
        local wid = nil
        pcall(function() wid = tostring(wpn:GetEntityID().hash) end)
        if wid ~= whooshWpnId then
            whooshWpnId = wid
            whooshEquipUntil = guardClock + WHOOSH_EQUIP_MUTE
            whooshBuf = {}
            whooshArmed = false   -- re-arms on the first calm window after the draw
        end

        -- sliding-window blade speed (see WHOOSH_* header): teleport check, push, trim, measure
        local last = whooshBuf[#whooshBuf]
        if last then
            local jx, jy, jz = rel.x - last.x, rel.y - last.y, rel.z - last.z
            if math.sqrt(jx*jx + jy*jy + jz*jz) > WHOOSH_TELEPORT then whooshBuf = {} end
        end
        whooshBuf[#whooshBuf + 1] = { t = guardClock, x = rel.x, y = rel.y, z = rel.z }
        while whooshBuf[1] and (guardClock - whooshBuf[1].t) > WHOOSH_WINDOW do
            table.remove(whooshBuf, 1)
        end
        local wSpeed = 0.0
        local wvx, wvy, wvz = 0.0, 0.0, 0.0   -- unit velocity direction over the window
        local oldest = whooshBuf[1]
        if oldest then
            local span = guardClock - oldest.t
            if span >= WHOOSH_MIN_SPAN then
                local dx, dy, dz = rel.x - oldest.x, rel.y - oldest.y, rel.z - oldest.z
                local dist = math.sqrt(dx*dx + dy*dy + dz*dz)
                wSpeed = dist / span
                if dist > 0.001 then wvx, wvy, wvz = dx/dist, dy/dist, dz/dist end
            end
        end

        if wSpeed < WHOOSH_REARM_SPEED then
            if whooshDebug and whooshPeak > 0.5 then logf("whoosh peak %.2f m/s", whooshPeak) end
            whooshPeak = 0.0
            whooshArmed = true
        elseif (not whooshArmed) and whooshLastDir and wSpeed >= WHOOSH_SWING_SPEED then
            -- combo stroke: still fast but the motion direction flipped vs the last whoosh
            local d = wvx*whooshLastDir.x + wvy*whooshLastDir.y + wvz*whooshLastDir.z
            if d < -0.1 then whooshArmed = true end
        end
        if whooshDebug and wSpeed > whooshPeak then whooshPeak = wSpeed end
        -- whoosh: once per swing episode (re-arm after the hand slows), independent of hits
        if whooshArmed and wSpeed >= WHOOSH_SWING_SPEED
           and guardClock >= whooshEquipUntil
           and (guardClock - whooshLast) >= WHOOSH_MIN_GAP and pl.VRMeleeWhoosh then
            whooshArmed = false
            whooshLast = guardClock
            whooshLastDir = { x = wvx, y = wvy, z = wvz }
            local strongW = false
            if type(GetVRMeleeTrigger) == 'function' then strongW = (GetVRMeleeTrigger() == 1) end
            pcall(function() pl:VRMeleeWhoosh(wpn, wSpeed >= WHOOSH_FAST_SPEED, strongW) end)
        end
        if speed >= MELEE_SWING_SPEED then
            local strong = false
            if type(GetVRMeleeTrigger) == 'function' then strong = (GetVRMeleeTrigger() == 1) end
            pcall(function() pl:VRMeleeBladeHit(wpn, wp, fwd, MELEE_BOX, strong) end)
        end
    end)
end)

registerForEvent('onShutdown', function()
    pcall(function()
        local pl = Game.GetPlayer()
        if pl then guardStats(pl, false, false) end
    end)
end)
