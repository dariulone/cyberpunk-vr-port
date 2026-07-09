-- CyberpunkVRPort_Weapon -- "bullet from the weapon barrel" VR aim.
--
-- The aim ENABLE toggle lives in the VR imgui overlay (dxgi "Controls" -> "Bullet from weapon
-- barrel", writes shared[58]). This script:
--   1) installs the GetOrientation VMT instrument once (InstallVRProvInstrument) -- this ALSO
--      installs the override hooks that redirect the shot down the barrel (slot 33 / mode 6),
--   2) publishes the weapon muzzle world orientation each frame (SetVRMuzzleQuat) -- drives both
--      the launch override and the overlay barrel laser dot, and
--   3) publishes the live camera zoom (SetVRZoomLevel) so the overlay scales the laser dot while
--      scoped (a scope changes GetZoom 1.0 -> ~5.25, not the FOV), and
--   4) VR MOTION MELEE: detects a real controller swing (weapon moved fast relative to the player)
--      and fires the game's NATIVE melee attack along the blade via redscript PlayerPuppet:VRMeleeSwing
--      (mod CyberpunkVRPort_Melee). The native box-sweep does collision/damage/reaction/stamina, so
--      it behaves like the flat game. A fast swing = power/cleave. No-op for guns (self-filtering).

local function logf(fmt, ...)
    local ok, s = pcall(string.format, fmt, ...)
    if ok then spdlog.info("[CyberpunkVRPort_Weapon] " .. s) end
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
local function updateMuzzle(wpn)
    local xf = wpn:GetMuzzleSlotWorldTransform()
    if not xf then return end
    local q = xf.Orientation or (xf.GetOrientation and xf:GetOrientation())
    if q and type(SetVRMuzzleQuat) == 'function' then
        SetVRMuzzleQuat(q.i, q.j, q.k, q.r)
    end
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
            logf("InstallVRProvInstrument = %s", tostring(r))
            installed = true
        end
    end

    pcall(function()
        local pl = Game.GetPlayer()
        local wpn = pl and pl:GetActiveWeapon()
        if wpn then updateMuzzle(wpn) end

        -- weapon draw sound (see equipSnd* header): fires on entity change, any weapon class
        local curWid = nil
        if wpn then pcall(function() curWid = tostring(wpn:GetEntityID().hash) end) end
        if not equipSndInit then
            equipSndInit = true
            equipSndId = curWid
        elseif curWid ~= equipSndId then
            equipSndId = curWid
            if wpn and pl and pl.VREquipSound then
                pcall(function() pl:VREquipSound(wpn) end)
            end
        end

        -- Publish the LIVE camera zoom so the dxgi overlay scales the barrel laser dot by the real
        -- scope magnification (scope changes GetZoom, NOT FOV; PSM.ZoomLevel is only a level index).
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
