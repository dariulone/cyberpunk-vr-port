-- A door's visible terminal can belong to a separate entity. Discover actual
-- loaded world-screen owners instead of relying on the gaze interaction target.
local S={}
-- An owner farther than this from the eyes cannot hold a screen within touch reach
-- (screens sit at most ~4 m from their owner's origin); its components are not listed.
local OWNER_REACH_SQ=15*15
function S.resolve(view)
    local result,seen={},{}
    if not view then return result end
    local world=Game.VRInkWorldControllers()
    -- A numeric loop, not ipairs: should a slot ever come back nil, ipairs would
    -- stop there and hide every live terminal after it.
    for i=1,#world do
        local controller=world[i]
        if controller and controller:IsA('DeviceInkGameControllerBase') then
            local owner=controller:GetOwnerEntity()
            if owner and owner:IsA('Device') then
                local key=tostring(owner:GetEntityID().hash)
                local o=not seen[key] and owner:GetWorldPosition()
                seen[key]=true
                if o and (o.x-view.x)^2+(o.y-view.y)^2+(o.z-view.z)^2<OWNER_REACH_SQ then
                    for _,component in ipairs(Game.VREntityComponents(owner)) do
                        if component:IsA('WorldWidgetComponent') and component:IsEnabled() and component.sceneWidgetProperties.isInteractable then
                            local centre=component:GetLocalToWorld().W
                            local dx,dy,dz=centre.x-view.x,centre.y-view.y,centre.z-view.z
                            if dx*dx+dy*dy+dz*dz<2.25 then
                                result[#result+1]={component=component,owner=key,key=key..'/'..NameToString(component:GetName()),
                                    dropPoint=owner:IsA('DropPoint')}
                            end
                        end
                    end
                end
            end
        end
    end
    return result
end
return S
