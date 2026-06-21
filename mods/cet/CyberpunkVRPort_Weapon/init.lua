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
local MELEE_SWING_SPEED = 1.5  -- m/s of weapon motion relative to player — peaks at 2-5 m/s on a real swing
local MELEE_BOX = 0.22         -- blade hit radius (m) — tight to NPC body silhouette

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

        -- Publish the LIVE camera zoom so the dxgi overlay scales the barrel laser dot by the real
        -- scope magnification (scope changes GetZoom, NOT FOV; PSM.ZoomLevel is only a level index).
        if type(SetVRZoomLevel) == 'function' then
            local cam = pl and pl:GetFPPCameraComponent()
            if cam then
                local z = cam:GetZoom()
                if z and z > 0.0 then SetVRZoomLevel(z) end
            end
        end

        -- VR MOTION MELEE: probe every frame the weapon is being SWUNG (speed relative to the player,
        -- so walking/turning doesn't count); the redscript helper does precise per-NPC enter detection
        -- and queues a native damage hit. Detection requires a melee weapon in the right hand AND the
        -- redscript helper VRMeleeBladeHit to be compiled in.
        if not (meleeEnabled and pl and wpn and pl.VRMeleeBladeHit) then return end
        local isMelee = false
        pcall(function() isMelee = WeaponObject.IsMelee(wpn:GetItemID()) end)
        if not isMelee then return end

        local wp = wpn:GetWorldPosition()
        local pp = pl:GetWorldPosition()
        local q = GetSingleton('Quaternion')
        local fwd = q and q:GetForward(wpn:GetWorldOrientation())
        if not (wp and pp and fwd) then return end

        local rel = { x = wp.x - pp.x, y = wp.y - pp.y, z = wp.z - pp.z }
        if meleePrevRel then
            local dx, dy, dz = rel.x - meleePrevRel.x, rel.y - meleePrevRel.y, rel.z - meleePrevRel.z
            local speed = math.sqrt(dx*dx + dy*dy + dz*dz) / math.max(dt or 0.016, 0.001)
            if speed >= MELEE_SWING_SPEED then
                local strong = false
                if type(GetVRMeleeTrigger) == 'function' then strong = (GetVRMeleeTrigger() == 1) end
                pcall(function() pl:VRMeleeBladeHit(wpn, wp, fwd, MELEE_BOX, strong) end)
            end
        end
        meleePrevRel = rel
    end)
end)
