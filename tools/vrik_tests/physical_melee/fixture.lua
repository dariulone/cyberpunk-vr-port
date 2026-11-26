-- Test-session module. All hits, marks and constraints are produced by the
-- installed mod. This observer samples in-game frames independently of the
-- controller sender, which keeps file round trips out of the timed swing.
local S={recording=false,frames={}}
local function xyz(v) return {v.x,v.y,v.z} end
local function player() return assert(Game.GetPlayer(),'player missing') end
local function weapon() return assert(player():VRPhysicalMeleeWeapon(),'drawn weapon missing') end
local function target()
    return S.tag and Game.GetDynamicEntitySystem():GetTagged(S.tag)[1]
end
local function cacheShapes(n)
    local comp=assert(n:FindComponentByName(CName.new('HitRepresentation')))
    local token=Game.GetResourceDepot():LoadResource(comp.resource)
    local resource=token and token:GetResource()
    if not resource then return false end
    local seen,caps={},{}
    local function add(v)
        if not v or not v.shape then return end
        local name=NameToString(v.name)
        if seen[name] then return end
        local ok,radius=pcall(function()return v.shape.radius end)
        if ok and radius and radius>0 then
            local height=0
            pcall(function()height=v.shape.height or 0 end)
            local t=v.shape.localTransform
            caps[#caps+1]={name=name,slot=v.slotName,center=Vector4.new(t.W.x,t.W.y,t.W.z,radius),
                axis=Vector4.new(t.X.x,t.X.y,t.X.z,height*.5)}
            seen[name]=true
        end
    end
    for _,v in ipairs(comp.appearanceOverrides or {}) do add(v.represenationOverride) end
    for _,v in ipairs(comp.representations or {}) do add(v) end
    for _,v in ipairs(resource.overrides or {}) do add(v.represenationOverride) end
    for _,v in ipairs(resource.representations or {}) do add(v) end
    S.comp,S.caps=comp,caps
    return #caps>0
end
local function overlap(a,b,edge,recordShapes)
    local depth,valid=0,0
    local occupied=recordShapes and {} or nil
    local zero=Vector4.new(0,0,0,0)
    for _,cap in ipairs(S.caps or {}) do
        local ok,t=S.comp:GetSlotTransform(cap.slot)
        if ok then
            valid=valid+1
            local m=VRBladeCapsuleInterval(WorldTransform.TransformInvPoint(t,a),WorldTransform.TransformInvPoint(t,b),
                zero,Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(t)),edge),cap.center,cap.axis,0)
            depth=math.max(depth,m.y)
            if occupied and m.w>.5 and m.y>0 then occupied[#occupied+1]={cap.name,NameToString(cap.slot),m.y} end
        end
    end
    return depth,valid,occupied
end
local function equipRequest(r)
    r.owner=player()
    Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(r)
end
local actions={}
function actions.slideControls() return require('slide_feedback_probe')() end
function actions.lock(c)
    S.tag=CName.new(c.tag)
    if not S.startPosition then
        local p=player()
        S.startPosition=xyz(p:GetWorldPosition())
        local f=p:GetWorldForward()
        S.startYaw=math.deg(math.atan2(-f.x,f.y))
    end
    local r=SaveLockRequest.new()
    r.operation=EItemOperationType.ADD
    r.reason=S.tag
    Game.GetScriptableSystemsContainer():Get(CName.new('SaveLocksManager')):QueueRequest(r)
    return true
end
function actions.locked() local locked=Game.IsSavingLocked();return locked end
function actions.restoreStart()
    assert(actions.locked() and not target(),'restore requires an empty, save-locked fixture')
    local p=player()
    local point=p:GetWorldPosition()
    local x,y,z=S.startPosition[1],S.startPosition[2],S.startPosition[3]
    if (point.x-x)^2+(point.y-y)^2+(point.z-z)^2>.04^2 then
        Game.GetTeleportationFacility():Teleport(p,Vector4.new(x,y,z,1),EulerAngles.new(0,0,S.startYaw))
        return false
    end
    return true
end
function actions.equip(c)
    assert(actions.locked(),'save lock required')
    local p=player()
    local ok,items=Game.GetTransactionSystem():GetItemList(p)
    assert(ok)
    S.item=nil
    for _,it in ipairs(items) do
        if ItemID.GetTDBID(it:GetID())==TweakDBID.new(c.item) then S.item=it:GetID();break end
    end
    assert(S.item,'save does not own requested weapon')
    local r=EquipRequest.new();r.itemID=S.item;r.slotIndex=0;r.addToInventory=false
    equipRequest(r)
    return true
end
function actions.draw() local r=DrawItemRequest.new();r.itemID=S.item;equipRequest(r);return true end
function actions.calibrate()
    local w=weapon();assert(w:GetItemID()==S.item,'wrong weapon')
    local basis=VRBladeTrackingBasis();assert(basis.w>.5,'tracking basis missing')
    local function track(v) return {basis.x*v.x+basis.y*v.y,v.z,basis.y*v.x-basis.x*v.y} end
    local a,u=w:GetWorldPosition(),w:VRMeleeDirection()
    return {blade=track(u),edge=track(w:GetWorldForward()),kind=w:VRBladeKind(),world=xyz(a),direction=xyz(u),
        reach=w:VRMeleeReach(),left=Game.VRWeaponInLeftHand(),floor=player():GetWorldPosition().z}
end
function actions.hand() return Game.VRWeaponInLeftHand() and 'left' or 'right' end
function actions.spawn(c)
    assert(actions.locked() and not target(),'trial target already present or save unlocked')
    local spec=DynamicEntitySpec.new()
    spec.recordID=TweakDBID.new(c.record)
    assert(TweakDBInterface.GetCharacterRecord(spec.recordID),'unknown fixture character record')
    spec.position=Vector4.new(c.point[1],c.point[2],c.point[3],1)
    spec.orientation=player():GetWorldOrientation()
    if c.facing then
        local pp=player():GetWorldPosition()
        local yaw=math.atan2(pp.x-c.point[1],c.point[2]-pp.y)
        if c.facing=='toward' then yaw=yaw+math.pi end
        spec.orientation=Quaternion.new(0,0,math.sin(yaw*.5),math.cos(yaw*.5))
    end
    spec.persistSpawn=false;spec.persistState=false;spec.alwaysSpawned=true;spec.tags={S.tag}
    Game.GetDynamicEntitySystem():CreateEntity(spec)
    return true
end
function actions.ready() local n=target();return n~=nil and n:IsAttached() end
function actions.prepare(c)
    local n=assert(target());n:GetAttitudeAgent():SetAttitudeTowards(player():GetAttitudeAgent(),EAIAttitude.AIA_Hostile)
    if c.targetSpeed<1 then
        n:SetIndividualTimeDilation(S.tag,c.targetSpeed,40,CName.new('None'),CName.new('None'))
    end
    return true
end
function actions.shapes() return cacheShapes(assert(target())) end
function actions.remove() S.caps,S.comp=nil,nil;Game.GetDynamicEntitySystem():DeleteTagged(S.tag);return true end
local function reactionSample(n)
    if not n then return nil end
    local component=n:GetHitReactionComponent()
    local feature=component and component:GetHitReactionData()
    local hold=n.vrBladeMovementHold
    local command=hold and hold.command
    return {
        hitTime=component and component:GetLastHitTimeStamp(),
        hitType=feature and feature.hitType,intensity=feature and feature.hitIntensity,
        variation=feature and feature.animVariation,stimId=component and component:GetLastStimID(),
        lastStim=component and NameToString(component:GetLastStimName()),
        holdJoint=hold and hold.jointSerial or 0,commandId=command and command.id,
        commandState=command and tostring(command.state),ragdoll=n:IsRagdolling()
    }
end
function actions.sample()
    local w=weapon();local n=target();local r=w.vrBladeReceipt
    local mod=GetMod('CyberpunkVRPort_Weapon')
    local pose=mod.BladePoseStatus();local d=mod.CyberwareStatus()
    local a=w:GetWorldPosition();local u=w:VRMeleeDirection();local reach=w:VRMeleeReach()
    local b=Vector4.new(a.x+reach*u.x,a.y+reach*u.y,a.z+reach*u.z,1)
    local inputs=Game.VRMeleeInputFrame()
    local rot=inputs and inputs[5]
    local side=Game.VRWeaponInLeftHand() and 0 or 1
    local raw=VRHandRawWorld(side)
    local palm=VRPalmWorldPos(side)
    local same=n and r and r.target and r.target:GetEntityID().hash==n:GetEntityID().hash
    local impact=same and r.impact
    local anchor=same and w:VRBladeAnchorWorld(true)
    local rearPosition=n and VrBladeRPG.Behind(player(),n)
    local joint=same and VRBladeJointPoint(side,r.jointSerial)
    local mark=same and w:VRBladeRenderedMarkPosition()
    local inkKind,inkTriangles
    if mark and mark.w>.5 then
        for _,slot in ipairs(r.slots.slots) do
            if slot.slotName==r.slot then
                local sample=VRBladePaintPoint(n,slot.boneName,r.serial)
                if sample and #sample==2 then inkKind,inkTriangles=sample[1].w,sample[2].w end
                break
            end
        end
    end
    local strength=Game.GetStatsSystem():GetStatValue(player():GetEntityID(),gamedataStatType.Strength)
    local probe=w:VRBladeLastProbe()
    local sweep=w.VRBladeLastSweep and w:VRBladeLastSweep()
    local hit,trace
    if n then hit,trace=Game.GetSpatialQueriesSystem():SyncRaycastByQueryPreset(a,b,CName.new('Melee'),false,false) end
    local obj=hit and TraceResult.GetHitEntity(trace)
    local narrow,count,occupied=overlap(a,b,w:GetWorldForward(),pose.edgeRequested)
    return {at=S.time,base=xyz(a),tip=xyz(b),hand=inputs and inputs[3] and xyz(inputs[3]),
        rawWorld=raw and raw.w>.5 and xyz(raw) or nil,resistance=pose.resistance or 0,
        wrist=palm and palm.w>.5 and xyz(palm) or nil,animatedAnchor=anchor and anchor.w>.5 and xyz(anchor) or nil,
        reaction=reactionSample(n),lodged=same and r.lodged or false,
        handRotation=rot and {rot.x,rot.y,rot.z,rot.w},edgeWorld=xyz(w:GetWorldForward()),origin=xyz(player():GetWorldPosition()),
        epoch=inputs and inputs[1] and inputs[1].y,speed=d.rightSwingSpeed or 0,
        serial=w:VRBladeConfirmedSerial(),hits=player().vrMeleeConfirmedHits,
        phase=pose.phase,exit=pose.exitReason,joint=joint and joint.w>.5 and xyz(joint) or nil,
        depth=pose.depth or 0,receiptDepth=pose.receiptDepth,holdRequested=pose.holdRequested,withdrawal=pose.withdrawal,
        slideTicks=w.vrBladeSlideTicks,
        jointSerial=same and r.jointSerial,simDepth=same and w:VRBladeMeasure(r.serial,a,b,Vector4.new(0,0,0,0)).x or 0,
        mark=mark and mark.w>.5 and xyz(mark) or nil,inkKind=inkKind,inkTriangles=inkTriangles,markRequests=same and r.vrMeshRequests,
        markStatus=n and n:VRBladeMarkDiagnostic(),puncture=same and r.puncture or false,
        same=same==true,released=same and r.released or false,entryBlocked=w:VRBladeEntryBlocked(),
        stealth=same and r.stealth or false,rear=impact and impact.rearAtStart or false,rearPosition=rearPosition or false,
        ruleStage=impact and impact.stealthStage,portMatched=impact and impact.portMatched or false,
        elite=n and n:IsElite() or false,alert=n and tostring(n:GetHighLevelStateFromBlackboard()),
        detection=n and n:GetDetectionPercentage(),physicalAfter=impact and impact.physicalAfter,
        eligible=n and n:IsActive() and not n:IsDead() and not ScriptedPuppet.IsDefeated(n) or false,
        hitShape=same and NameToString(r.shapeName) or nil,follow=same and w:VRBladeConfirmedFollowThrough() or false,
        hitMaterial=same and r.material or nil,shapeType=same and r.shapeData and tostring(r.shapeData:GetShapeType()) or nil,
        strength=strength,gesture=pose.lastGesture,attempts=pose.attempts,requests=pose.requests,
        edgeRequested=pose.edgeRequested,edgeStroke=pose.edgeStroke,edgeTravel=pose.edgeTravel,
        edgeCommand=pose.edgeCommand,edgeRailStage=pose.edgeRailStage,
        slotFollowing=pose.slotFollowing,slotGap=pose.slotGap,slotDepth=pose.slotDepth,
        followGate=pose.followGate,followSerial=pose.followSerial,followGeometry=pose.followGeometry,
        followWindow=pose.followWindow,followCounts=pose.followCounts,followAttempt=pose.followAttempt,occupiedDuringEdge=occupied,
        target=n and xyz(n:GetWorldPosition()),hp=n and Game.GetStatPoolsSystem():GetStatPoolValue(n:GetEntityID(),gamedataStatPoolType.Health,false),
        rayTarget=obj and obj:GetEntityID().hash==n:GetEntityID().hash or false,shapeDepth=narrow,shapes=count,
        queries=d.strokeQueries,starts=d.fullSwingStarts,whooshes=d.whooshes or 0,probe={probe.x,probe.y,probe.z,probe.w},
        sweep=sweep and {sweep.x,sweep.y,sweep.z,sweep.w},error=d.error}
end
function actions.begin(c)
    S.frames={};S.time=0;S.recording=true;S.sampleError=nil
    if c.releaseAI then
        local n=assert(target())
        n:UnsetIndividualTimeDilation()
    end
    if c.animate then
        local n=assert(target())
        n:UnsetIndividualTimeDilation()
        local hit=n:GetHitReactionComponent():GetHitReactionData()
        hit.hitType=3
        AnimationControllerComponent.ApplyFeatureToReplicate(n,CName.new('hit'),hit)
        AnimationControllerComponent.PushEventToReplicate(n,CName.new('hit'))
    end
    if c.moveTarget then
        local n=assert(target())
        n:UnsetIndividualTimeDilation()
        local p,f=n:GetWorldPosition(),n:GetWorldForward()
        local cmd=AITeleportCommand.new()
        local lateralMove=.60
        cmd.position=Vector4.new(p.x+lateralMove*f.y,p.y-lateralMove*f.x,p.z,1)
        cmd.rotation=math.deg(math.atan2(-f.x,f.y))
        cmd.doNavTest=false
        assert(n:GetAIControllerComponent():SendCommand(cmd),'NPC rejected movement command')
    end
    return actions.sample()
end
function actions.finish() S.recording=false;assert(not S.sampleError,S.sampleError);return S.frames end
local function readRequest()
    local file=io.open('request.json','r');if not file then return end
    local s=file:read('*a');file:close()
    local ok,c=pcall(json.decode,s)
    if ok and c and c.id~=S.last then return c end
end
registerForEvent('onUpdate',function(dt)
    if S.recording then
        S.time=S.time+dt
        local ok,row=pcall(actions.sample)
        if ok then S.frames[#S.frames+1]=row else S.sampleError=tostring(row);S.recording=false end
        if #S.frames>=1200 then S.sampleError='observation exceeded frame limit';S.recording=false end
    end
    local c=readRequest();if not c then return end
    S.last=c.id
    local ok,result=pcall(function() return assert(actions[c.action],'unknown action')(c) end)
    local f=assert(io.open('response.json','w'))
    f:write(json.encode({id=c.id,ok=ok,result=ok and result or nil,error=not ok and tostring(result) or nil}))
    f:close()
end)
