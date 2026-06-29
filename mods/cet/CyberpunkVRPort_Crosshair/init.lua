local container = nil

registerForEvent("onInit", function()
    -- Controlla se la funzione esiste (dentro onInit)
    if type(GetVRWeaponAim) ~= 'function' then
        print("[VR] ERROR: GetVRWeaponAim function not found!")
        return
    end
    

    Observe("gameuiCrosshairContainerController", "OnInitialize", function(this)
        container = this
        print("[VR] Crosshair container captured")
    end)


    Observe("gameuiCrosshairBaseGameController", "OnCrosshairStateChange", function(this, old, new)
        if not container then return end
        if not IsDefined(container) then 
            container = nil 
            return
        end

        local aimEnabled = GetVRWeaponAim()

        --print("[VR] OnCrosshairStateChange CALLED")        
        if aimEnabled then
            container:GetRootWidget():SetVisible(false)
            container:GetActiveCrosshairWidget():SetVisible(false)
        else
            container:GetRootWidget():SetVisible(true)
            container:GetActiveCrosshairWidget():SetVisible(true)
        end
    end)

    Observe("gameuiCrosshairBaseGameController", "UpdateCrosshairState", function(this)
        if not container then return end
        if not IsDefined(container) then 
            container = nil 
            return
        end

        local aimEnabled = GetVRWeaponAim()
        
        --print("[VR] UpdateCrosshairState CALLED")        

        if aimEnabled then
            container:GetRootWidget():SetVisible(false)
            container:GetActiveCrosshairWidget():SetVisible(false)
        else
            container:GetRootWidget():SetVisible(true)
            container:GetActiveCrosshairWidget():SetVisible(true)
        end
    end)
end)