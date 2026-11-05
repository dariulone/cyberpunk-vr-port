local container = nil

-- Smart weapons aim through their smart HUD: keep the crosshair (lock-on markers)
-- visible for them while VR weapon aim hides every other one. LOCAL: all smart
-- weapons, not only the smart sniper rifle (Ashura).
local smartKey, smartValue = nil, false
local function smartSniperActive()
    local player = Game.GetPlayer()
    local weapon = player and player:GetActiveWeapon()
    if not weapon then smartKey = nil; return false end
    local key = tostring(weapon:GetEntityID().hash)
    if key == smartKey then return smartValue end
    smartKey, smartValue = key, false
    local record = weapon:GetWeaponRecord()
    local evolution = record and record:Evolution()
    if evolution then
        smartValue = EnumInt(evolution:Type()) == EnumInt(gamedataWeaponEvolution.Smart)
    end
    return smartValue
end

-- Read by the test harness: what the last crosshair update decided.
local M = { smart=false, hidden=nil, error=nil, hudScale=1, scaled=0 }

-- The smart HUD drawn at its flat-screen size fills most of the view in VR: draw
-- its frame, backdrop and decorations at SMART_HUD_SCALE. Each part is a box
-- centred on the screen, so its own centre pivot scales it about the aim point.
-- The lock-on markers (holderLock, holderRoot) follow their targets and keep 1.
local SMART_HUD_SCALE = 0.4
local SMART_HUD_PARTS = {
    {'smartGun', 'smartGunFrame'}, {'smartGun', 'dotContainer'}, {'smartGun', 'smartGunBG'},
    {'smartGun', 'smartGunFluff'}, {'smartGun', 'wmrk'}, {'smartFots'}, {'reticle_no_smart_link'},
}
local function child(widget, name)
    if not widget or not widget:IsA(CName.new('inkCompoundWidget')) then return nil end
    for i = 0, widget:GetNumChildren() - 1 do
        local c = widget:GetWidgetByIndex(i)
        if c and NameToString(c:GetName()) == name then return c end
    end
    return nil
end
local function scaleSmartHud(scale)
    local root = container:GetActiveCrosshairWidget()
    if not root then return end
    local n = 0
    for _, path in ipairs(SMART_HUD_PARTS) do
        local w = root
        for _, name in ipairs(path) do w = child(w, name) end
        if w then w:SetScale(Vector2.new({X=scale, Y=scale})); n = n + 1 end
    end
    M.hudScale, M.scaled = scale, n
end

local function hideCrosshair()
    if not GetVRWeaponAim() then M.hidden=false;return false end
    local ok, smart = pcall(smartSniperActive)
    M.smart = ok and smart or false
    M.error = (not ok) and tostring(smart) or nil
    M.hidden = not (ok and smart)
    return M.hidden
end

registerForEvent("onInit", function()
    -- Controlla se la funzione esiste (dentro onInit)
    if type(GetVRWeaponAim) ~= 'function' then
        print("[VR] ERROR: GetVRWeaponAim function not found!")
        return
    end
    

    Observe("gameuiCrosshairContainerController", "OnInitialize", function(this)
        container = this
        M.container = this
        print("[VR] Crosshair container captured")
    end)


    Observe("gameuiCrosshairBaseGameController", "OnCrosshairStateChange", function(this, old, new)
        if not container then return end
        if not IsDefined(container) then 
            container = nil 
            return
        end

        local aimEnabled = hideCrosshair()

        --print("[VR] OnCrosshairStateChange CALLED")        
        if aimEnabled then
            container:GetRootWidget():SetVisible(false)
            container:GetActiveCrosshairWidget():SetVisible(false)
        else
            container:GetRootWidget():SetVisible(true)
            container:GetActiveCrosshairWidget():SetVisible(true)
        end
        local okSmart, smart = pcall(smartSniperActive)
        pcall(scaleSmartHud, (okSmart and smart) and SMART_HUD_SCALE or 1)
    end)

    Observe("gameuiCrosshairBaseGameController", "UpdateCrosshairState", function(this)
        if not container then return end
        if not IsDefined(container) then 
            container = nil 
            return
        end

        local aimEnabled = hideCrosshair()
        
        --print("[VR] UpdateCrosshairState CALLED")        

        if aimEnabled then
            container:GetRootWidget():SetVisible(false)
            container:GetActiveCrosshairWidget():SetVisible(false)
        else
            container:GetRootWidget():SetVisible(true)
            container:GetActiveCrosshairWidget():SetVisible(true)
        end
        local okSmart, smart = pcall(smartSniperActive)
        pcall(scaleSmartHud, (okSmart and smart) and SMART_HUD_SCALE or 1)
    end)
end)

return M
