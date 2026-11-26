local gesture=require('vehicle_gesture')
local M={state={actions=0,part=nil,error=nil}}
local hands={[0]=gesture.new(),[1]=gesture.new()}
local nextScan={[0]=0,[1]=0}
local filter,slots
local axes={{1,0,0},{0,1,0},{0,0,1}}

function M.clear()
    for side=0,1 do
        hands[side]=gesture.new();nextScan[side]=0
        if type(VRVehicleHandClaim)=='function' then VRVehicleHandClaim(side,false) end
    end
end
local function localPoint(vehicle,p)
    local b,q=vehicle:GetWorldPosition(),vehicle:GetWorldOrientation()
    local x,y,z=p.x-b.x,p.y-b.y,p.z-b.z
    local tx,ty,tz=2*(-q.j*z+q.k*y),2*(-q.k*x+q.i*z),2*(-q.i*y+q.j*x)
    return {x=x+q.r*tx-q.j*tz+q.k*ty,y=y+q.r*ty-q.k*tx+q.i*tz,z=z+q.r*tz-q.i*ty+q.j*tx}
end
local function paused(p)
    if not p then return true end
    local r=Game.GetSystemRequestsHandler()
    if not r or r:IsPreGame() or r:IsGamePaused() or GetVRSharedSlot(81)>.5 then return true end
    local defs=Game.GetAllBlackboardDefs()
    local psm=p:GetPlayerStateMachineBlackboard()
    if not psm or psm:GetInt(defs.PlayerStateMachine.SceneTier)>=3 then return true end
    local ui=Game.GetBlackboardSystem():Get(defs.UI_System)
    return ui and ui:GetBool(defs.UI_System.IsInMenu)
end
local function free(p,side)
    if GetVRSharedSlot(side*8)<=.5 then return false end
    slots=slots or {TweakDBID.new('AttachmentSlots.WeaponLeft'),TweakDBID.new('AttachmentSlots.WeaponRight')}
    local item=Game.GetTransactionSystem():GetItemInSlot(p,slots[side+1])
    if item and not (WeaponObject.IsOfType(item:GetItemID(),gamedataItemType.Wea_Fists)
        or WeaponObject.IsOfType(item:GetItemID(),gamedataItemType.Cyb_StrongArms)) then return false end
    local hc=GetMod('CyberpunkVRPort_HandCollision')
    return not (hc and hc.graspState and hc.graspState(side).held)
end
local function contact(p,side)
    filter=filter or QueryFilter.ALL()
    local spatial=Game.GetSpatialQueriesSystem()
    -- PhysX first confirms contact with this instance. Bounds only distinguish
    -- its moving panels; they do not add any hand collision geometry.
    for _,axis in ipairs(axes) do
        for sign=-1,1,2 do
            local a=Vector4.new(p.x+axis[1]*.12*sign,p.y+axis[2]*.12*sign,p.z+axis[3]*.12*sign,1)
            local found,trace=spatial:SyncRaycastByQueryFilter(p,a,filter,false,false)
            if found and trace then
                local v=TraceResult.GetHitEntity(trace)
                if v and v:IsA('vehicleCarBaseObject') and v:IsPlayerVehicle() then
                    -- Pick at the physical surface, not at the wrist behind
                    -- the palm. Authored colliders extend slightly beyond
                    -- their visual mesh (Hella hood: 5.6 cm).
                    local hit=trace.position
                    local part=VRVehiclePartNear(v,Vector4.new(hit.x,hit.y,hit.z,1),.10)
                    if part.w>.5 and v:VRPortCanTouchPanel(math.floor(part.x)) then
                        local id=tostring(v:GetEntityID().hash)
                        local other=hands[1-side].target
                        if not (other and other.id==id and other.part==part.x) then
                            return {vehicle=v,id=id,part=math.floor(part.x)}
                        end
                    end
                end
            end
        end
    end
end
function M.update(player,now)
    if paused(player) then M.clear();return end
    now=now or os.clock()
    for side=0,1 do
        local s=hands[side]
        local grip=GetVRSharedSlot(side==0 and 155 or 49)>.5
        if not free(player,side) then
            hands[side]=gesture.new();VRVehicleHandClaim(side,false)
        else
            local p=VRVehicleHandWorld(side)
            local target=s.target
            if target and not target.vehicle:IsPlayerVehicle() then
                hands[side]=gesture.new();VRVehicleHandClaim(side,false)
            else
                local pick
                if grip and not s.blocked and not target and p.w>.5 and now>=nextScan[side] then
                    nextScan[side]=now+.05;pick=contact(p,side)
                end
                local v=target and target.vehicle or (pick and pick.vehicle)
                local point=v and p.w>.5 and localPoint(v,p) or nil
                local act,busy=gesture.step(s,now,grip,pick,point)
                VRVehicleHandClaim(side,busy)
                if act then
                    if s.target.vehicle:VRPortTogglePanel(s.target.part) then
                        M.state.actions=M.state.actions+1;M.state.part=s.target.part
                        Game.SetVRHapticPulse(side,.4,35)
                    end
                elseif pick then Game.SetVRHapticPulse(side,.18,20) end
            end
        end
    end
end
return M
