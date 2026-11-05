-- A door's visible terminal can belong to a separate entity. Discover actual
-- loaded world-screen owners instead of relying on the gaze interaction target.
local S={}
-- An owner farther than this from the eyes cannot hold a screen within touch reach
-- (screens sit at most ~4 m from their owner's origin); its components are not listed.
local OWNER_REACH_SQ=15*15
-- A lift's floor panels belong to its LiftDevice, whose origin can lie far down the shaft: ~90 m from a panel half a
-- metre from the eyes in a tall building (a player's report, 2026-11: the lift buttons stopped answering when 0.2.0
-- added the owner reach above). Such an owner is kept when one of its own screens is within OWNER_REACH_SQ; they are
-- few, and this is measured once per walk, not on every look.
local FAR_OWNER_REACH_SQ=250*250
local FAR_OWNER_CLASSES={'LiftDevice'}
local WIDGET_CLASS=nil
-- THE WORLD'S CONTROLLERS ARE WALKED ONCE A SECOND, not on every look. The city holds a few
-- hundred world screens and the walk asks each one for its owner, the owner's class, id and
-- position: ~1-2 ms on the game thread, which ran 10 times a second (0.13 ms a frame on average
-- and a spike in every tenth frame). Devices do not move, so the owners within reach are kept (by
-- id, looked up again on each look: one streamed out meanwhile is simply not found) and each look
-- goes over those alone. The walk runs again after a second, or sooner once the eyes have moved
-- REWALK_MOVE_SQ from where it last ran (a sprint covers 4 m in about half a second; the reach of
-- 15 m leaves the margin for anything coming into touch range meanwhile).
local REWALK_SECONDS=1.0
local REWALK_MOVE_SQ=4*4
local near,walkedAt,walkedFrom={},-1e9,nil
local function farOwner(owner)
    for i=1,#FAR_OWNER_CLASSES do
        if owner:IsA(FAR_OWNER_CLASSES[i]) then return true end
    end
    return false
end
local function screenWithin(owner,view,reachSq)
    for _,component in ipairs(Game.VREntityComponentsOf(owner,WIDGET_CLASS)) do
        local centre=component:GetLocalToWorld().W
        if (centre.x-view.x)^2+(centre.y-view.y)^2+(centre.z-view.z)^2<reachSq then return true end
    end
    return false
end
local function walk(view)
    near={}
    local seen={}
    local world=Game.VRInkWorldControllers()
    -- A numeric loop, not ipairs: should a slot ever come back nil, ipairs would
    -- stop there and hide every live terminal after it.
    for i=1,#world do
        local controller=world[i]
        if controller and controller:IsA('DeviceInkGameControllerBase') then
            local owner=controller:GetOwnerEntity()
            if owner and owner:IsA('Device') then
                local id=owner:GetEntityID()
                local key=tostring(id.hash)
                if not seen[key] then
                    seen[key]=true
                    local o=owner:GetWorldPosition()
                    local d2=o and (o.x-view.x)^2+(o.y-view.y)^2+(o.z-view.z)^2
                    if d2 and (d2<OWNER_REACH_SQ or
                            (d2<FAR_OWNER_REACH_SQ and farOwner(owner) and screenWithin(owner,view,OWNER_REACH_SQ))) then
                        near[#near+1]={id=id,key=key,dropPoint=owner:IsA('DropPoint')}
                    end
                end
            end
        end
    end
end
-- The screens alone, filtered natively: walking all of a device's components with an IsA call on each was ~20 game
-- calls a frame for one screen.
function S.resolve(view)
    local result={}
    if not view then return result end
    WIDGET_CLASS=WIDGET_CLASS or CName.new('WorldWidgetComponent')
    local now=os.clock()
    local moved=not walkedFrom or (walkedFrom.x-view.x)^2+(walkedFrom.y-view.y)^2+(walkedFrom.z-view.z)^2>REWALK_MOVE_SQ
    if moved or now-walkedAt>=REWALK_SECONDS then
        walk(view)
        walkedAt=now;walkedFrom={x=view.x,y=view.y,z=view.z}
    end
    for i=1,#near do
        local entry=near[i]
        local owner=Game.FindEntityByID(entry.id)
        if owner then
            for _,component in ipairs(Game.VREntityComponentsOf(owner,WIDGET_CLASS)) do
                if component:IsEnabled() and component.sceneWidgetProperties.isInteractable then
                    local centre=component:GetLocalToWorld().W
                    local dx,dy,dz=centre.x-view.x,centre.y-view.y,centre.z-view.z
                    if dx*dx+dy*dy+dz*dz<2.25 then
                        result[#result+1]={component=component,owner=entry.key,key=entry.key..'/'..NameToString(component:GetName()),
                            dropPoint=entry.dropPoint}
                    end
                end
            end
        end
    end
    return result
end
-- Forget the walk (a new session, the player changed): the next look walks again.
function S.reset() near,walkedAt,walkedFrom={},-1e9,nil end
return S
