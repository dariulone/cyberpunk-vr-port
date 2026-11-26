-- Loaded by the attach-only harness after acquiring the exclusive bridge lease.
-- No game launch/exit/save. Geometry and trajectories are computed by corpse_probe.exe.
CorpseAudit={tag=CName.new("cpvr_corpse_regression"),bodies={},massBaseline={},massTracked={},massRestored={}}
local A=CorpseAudit
local function xyz(p)return {p.x,p.y,p.z}end
local function quat(q)return {q.i,q.j,q.k,q.r}end
local function integer(v)return tonumber(v) or tonumber(tostring(v):match("%d+"))end
local function inventoryState()
 local ok,items=Game.GetTransactionSystem():GetItemList(Game.GetPlayer());assert(ok,"inventory snapshot failed")
 local state={};for _,raw in ipairs(items) do local id=raw:GetID()
  state[tostring(ItemID.GetCombinedHash(id))]={id=id,quantity=raw:GetQuantity(),record=TDBID.ToStringDEBUG(ItemID.GetTDBID(id))}
 end
 return state
end
function A.inventoryDifference(before,now)
 local result={extra={},missing={},quantityChanges={}}
 for key,value in pairs(now) do
  if not before[key] then result.extra[#result.extra+1]={key=key,record=value.record,quantity=value.quantity}
  elseif value.quantity~=before[key].quantity then
   result.quantityChanges[#result.quantityChanges+1]={key=key,record=value.record,before=before[key].quantity,now=value.quantity}
  end
 end
 for key,value in pairs(before) do if not now[key] then result.missing[#result.missing+1]={key=key,record=value.record,quantity=value.quantity} end end
 result.equal=#result.extra==0 and #result.missing==0 and #result.quantityChanges==0
 return result
end
local function playerState()
 local p=Game.GetPlayer();return {position=xyz(p:GetWorldPosition()),forward=xyz(p:GetWorldForward()),rotation=quat(p:GetWorldOrientation())}
end
function A.lockSaving(reason)
 assert(type(reason)=="string" and reason:match("^CPVR_CorpseAudit_[a-f0-9]+$"),"invalid unique save-lock reason")
 assert(not A.saveLockReason,"save lock already requested")
 A.saveLockName=reason;A.saveLockReason=CName.new(reason)
 local request=SaveLockRequest.new();request.operation=EItemOperationType.ADD;request.reason=A.saveLockReason
 Game.GetScriptableSystemsContainer():Get(CName.new("SaveLocksManager")):QueueRequest(request)
 return reason
end
function A.saveLockState()
 local manager=Game.GetScriptableSystemsContainer():Get(CName.new("SaveLocksManager"))
 local owned=false;local reasons={};local membershipKnown=manager.m_saveLocks~=nil
 -- Use exact membership when exposed. On hosts without that private field,
 -- require root's independently verified token, not just a generic locked bit.
 for _,name in ipairs(manager.m_saveLocks or {}) do
  reasons[#reasons+1]=NameToString(name)
  if A.saveLockReason and name==A.saveLockReason then owned=true end
 end
 local locked,locks=Game.IsSavingLocked()
 local managerLocked=manager:IsSavingLocked()==true
 local root=GoalSaveGuard
 local rootVerified=root and root.verified==true and root.removed~=true and type(root.name)=="string"
  and root.name:match("^cpvr_goal_root_[a-f0-9]+$")~=nil
  and (root.verification=="causal-remove-readd" or root.verification=="observer-and-engine")
 local engineLocks={};for _,value in ipairs(locks or {}) do engineLocks[#engineLocks+1]=tostring(value) end
 return {owned=owned,membershipKnown=membershipKnown,engineLocked=locked==true,managerLocked=managerLocked,
  protected=locked==true and managerLocked and (owned or rootVerified)==true,
  rootVerified=rootVerified==true,rootName=rootVerified and root.name or nil,
  rootVerification=rootVerified and root.verification or nil,
  protection=owned and "own-membership" or rootVerified and "verified-root" or "unverified",reasons=reasons,engineLocks=engineLocks,
  reason=A.saveLockName,verified=A.saveLockVerified==true}
end
function A.confirmSaveLock()
 local state=A.saveLockState()
 assert(state.protected,"neither own membership nor a verified retained root lock protects this session")
 Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()
 A.saveLockVerified=true;A.protection=state.owned and "own-membership" or "verified-root";A.rootSaveLockName=state.rootName
 return A.saveLockState()
end
function A.releaseSaveLock(restorationVerified)
 assert(restorationVerified==true and A.saveLockVerified and A.saveLockReason,"retain save lock until restoration is verified")
 assert(A.saveLockState().protected,"saving protection disappeared before restoration completed")
 Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()
 local request=SaveLockRequest.new();request.operation=EItemOperationType.REMOVE;request.reason=A.saveLockReason
 Game.GetScriptableSystemsContainer():Get(CName.new("SaveLocksManager")):QueueRequest(request)
 A.saveLockReleaseRequested=true
end
function A.begin()
 assert(type(VRPhysHeldBody)=="function","build lacks VRPhysHeldBody diagnostic")
 assert(type(VRBodySolverMass)=="function","build lacks uncached solver-mass diagnostic")
 assert(type(VRPhysHandFrame)=="function","build lacks hand frames")
 local p=assert(Game.GetPlayer());local eq=EquipmentSystem.GetData(p)
 local previous=Game.GetDynamicEntitySystem():GetTagged(A.tag)
 assert(not previous or #previous==0,"previous test NPC must be restored/removed before another session")
 local h=GetMod("CyberpunkVRPort_Holster")
 local weapon=p:GetActiveWeapon()
 A.original={player=p,entity=tostring(p:GetEntityID().hash),position=p:GetWorldPosition(),rotation=p:GetWorldOrientation(),
   forward=p:GetWorldForward(),arms=eq:GetActiveItem(gamedataEquipmentArea.ArmsCW),weapons={},active=weapon and weapon:GetItemID(),
   holsters=h and h.hidden(),money=Game.GetTransactionSystem():GetItemQuantity(p,ItemID.FromTDBID(TweakDBID.new("Items.money")))}
 A.original.inventory=inventoryState()
 for i=0,2 do A.original.weapons[i+1]=eq:GetItemInEquipSlot(gamedataEquipmentArea.Weapon,i) end
 for side=0,1 do assert(not GetMod("CyberpunkVRPort_HandCollision").graspState(side).held,"release current held object before corpse regression") end
 local encoded={};for key,value in pairs(A.original.inventory) do encoded[#encoded+1]={key=key,record=value.record,quantity=value.quantity} end
 return {player=playerState(),equipment={arms=tostring(A.original.arms),active=A.original.active and tostring(A.original.active)},holsters=A.original.holsters,inventory=encoded}
end
function A.prepare()
 assert(A.saveLockVerified,"no gameplay mutations before verified save lock")
 local r=EquipmentSystemWeaponManipulationRequest.new();r.owner=Game.GetPlayer();r.requestType=EquipmentManipulationAction.UnequipAll
 r.equipAnimType=gameEquipAnimationType.Instant;r.removeItemFromEquipSlot=false
 Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")):QueueRequest(r)
 local h=GetMod("CyberpunkVRPort_Holster");if h then assert(h.setHidden(true),"cannot hide belt holsters during probe") end
end
function A.spawn(position,record)
 assert(A.saveLockVerified,"no NPC spawn before verified save lock")
 assert(A.original,"begin must run first")
 local old=Game.GetDynamicEntitySystem():GetTagged(A.tag)
 assert(not old or #old==0,"previous corpse test entity must be restored/removed first")
 local spec=DynamicEntitySpec.new();spec.recordID=TweakDBID.new(record or "Character.LowlifeMale")
 spec.position=Vector4.new(position[1],position[2],position[3],1);spec.orientation=Game.GetPlayer():GetWorldOrientation()
 spec.persistState=false;spec.persistSpawn=false;spec.alwaysSpawned=true;spec.tags={A.tag}
 A.npcID=Game.GetDynamicEntitySystem():CreateEntity(spec)
end
function A.kill()
 assert(A.saveLockVerified,"no death/ragdoll mutation before verified save lock")
 local list=Game.GetDynamicEntitySystem():GetTagged(A.tag);A.npc=list and list[#list]
 if not A.npc then return false end
 A.slots=nil
 for _,c in ipairs(Game.VREntityComponents(A.npc)) do if NameToString(c:GetName())=="Item_Attachment_Slot" then A.slots=c;break end end
 assert(A.slots,"spawned NPC has no attachment slots")
 A.npc:Kill(nil,false,false)
 return true
end
function A.teleport(position,yaw)
 assert(A.saveLockVerified,"no teleport before verified save lock")
 Game.GetTeleportationFacility():Teleport(Game.GetPlayer(),Vector4.new(position[1],position[2],position[3],1),EulerAngles.new(0,0,yaw))
end
function A.captureMasses(probes)
 assert(A.saveLockVerified,"save lock must precede acquisition")
 for side=0,1 do local hold=VRPhysHeldBody(side);local proxy=integer(hold[2])
  if integer(hold[1])==1 and A.massBaseline[tostring(proxy)..":"] then return {proxy=proxy,alreadyCaptured=true} end
 end
 local near;local seen={}
 local sys=Game.GetSpatialQueriesSystem()
 for _,ray in ipairs(probes.rays) do
  local a,b=ray[1],ray[2]
  for _,preset in ipairs({'Sight Blocker','Bullet','Camera'}) do
   local hit,tr=sys:SyncRaycastByQueryPreset(Vector4.new(a[1],a[2],a[3],1),Vector4.new(b[1],b[2],b[3],1),preset,false,true)
   if hit and tr then
    local info=VRTraceInfo(tr);seen[#seen+1]={source=preset,kind=info.x,proxy=info.y,body=info.z}
    if info.x==12 then near=info;break end
   end
  end
  if near then break end
 end
 if not near then for _,point in ipairs(probes.points) do
  local info=VRNearBody(Vector4.new(point[1],point[2],point[3],1),.10,10000)
  if info then seen[#seen+1]={source='near',kind=info.x,proxy=info.y,body=info.z};if info.x==12 then near=info;break end end
 end end
 assert(near,"mass baseline found no ragdoll: "..json.encode(seen))
 local proxy=near.y;local prefix=tostring(proxy)..":"
 if A.massBaseline[prefix] then return {proxy=proxy,alreadyCaptured=true} end
 for side=0,1 do local hold=VRPhysHeldBody(side)
  assert(not (integer(hold[1])==1 and integer(hold[2])==proxy),"cannot capture original mass after solver override")
 end
 local values={};local count=VRProxyBodies(proxy)
 for body=0,count-1 do
  local mass=VRBodySolverMass(proxy,body)
  if mass and mass>0 then A.massBaseline[prefix..tostring(body)]=mass;values[#values+1]={body=body,mass=mass} end
 end
 assert(#values>0,"no readable physical mass baseline")
 A.massBaseline[prefix]=true
 return {proxy=proxy,bodies=values,selection=seen}
end
function A.heldSurface(side,point)
 local hold=VRPhysHeldBody(side)
 assert(integer(hold[1])==1 and integer(hold[4])<200,"no fresh actual held actor for surface query")
 local p=VRBodySurface(integer(hold[2]),integer(hold[3]),Vector4.new(point[1],point[2],point[3],1))
 assert(p and p.w>=0,"actual held shape surface unavailable")
 return xyz(p)
end
function A.standingGround(candidates)
 local out={};local sys=Game.GetSpatialQueriesSystem()
 for _,candidate in ipairs(candidates) do
  local hits={}
  for _,ray in ipairs(candidate) do
   local a,b=ray[1],ray[2]
   local hit,tr=sys:SyncRaycastByQueryPreset(Vector4.new(a[1],a[2],a[3],1),Vector4.new(b[1],b[2],b[3],1),'Sight Blocker',true,false)
   hits[#hits+1]=hit and tr and xyz(tr.position) or false
  end
  out[#out+1]=hits
 end
 return out
end
local function bodyPose(proxy,body)
 local key=tostring(proxy)..":"..tostring(body)
 local handle=A.bodies[key]
 if not handle then handle=VRBodyOf(proxy,body);A.bodies[key]=handle end
 if not handle then return nil end
 local p=VRBodyGetPos(handle);if not p or p.w<.5 then A.bodies[key]=nil;return nil end
 return {p=xyz(p),q=quat(VRBodyGetRot(handle))}
end
function A.sample(full)
 local out={player=playerState(),dead=A.npc and A.npc:IsDead() or false,npc=A.npc and tostring(A.npc:GetEntityID().hash),slots={},hands={}}
 local names=full and {"Head","Chest","Hips","LeftArm","RightArm","LeftForeArm","RightForeArm","LeftHand","RightHand","LegLeft","LegRight","LeftFoot","RightFoot"} or {"Hips"}
 if A.slots then for _,name in ipairs(names) do
  local ok,t=A.slots:GetSlotTransform(CName.new(name));if ok then out.slots[name]=xyz(WorldPosition.ToVector4(t.Position)) end
 end end
 for side=0,1 do
  local frame={};for _,v in ipairs(VRPhysHandFrame(side)) do frame[#frame+1]={v.x,v.y,v.z,v.w} end
  local g=GetMod("CyberpunkVRPort_HandCollision").graspState(side)
  local ids=VRPhysHeldBody(side);assert(#ids>=4,"invalid native hold diagnostic layout")
  local proxy,body=integer(ids[2]),integer(ids[3])
  local n={active=integer(ids[1])==1,proxy=tostring(ids[2]),body=body,ageMs=integer(ids[4])}
  n.contactMetricAvailable=#ids>=5;n.contactValid=false
  if n.contactMetricAvailable then
   local error=integer(ids[5])
   if n.active and n.ageMs<200 and error and error>=0 and error<=1000000000000 then
    n.contactValid=true;n.contactErrorMicrometres=error
   end
  end
  local hand={frame=frame,native=n,grasp={held=g.held==true,kind=g.kind,proxy=g.proxy,body=g.body,part=g.part,fast=g.fast,mass=g.mass,wholeMass=g.wholeMass}}
  if g.held and g.proxy then hand.referencePose=bodyPose(g.proxy,g.body) end
  if n.active then
   local info=VRBodyInfo(proxy,body);local key=tostring(proxy)..":"..tostring(body)
   hand.nativePose=bodyPose(proxy,body);hand.nativeKind=info.x;hand.reportedCarryMass=info.w
   hand.solverMass=VRBodySolverMass(proxy,body);hand.baselineMass=A.massBaseline[key]
   A.massTracked[key]={proxy=proxy,body=body};A.massRestored[key]=false
  end
  out.hands[side+1]=hand
 end
 return out
end
function A.masses()
 local holds={VRPhysHeldBody(0),VRPhysHeldBody(1)};local out={}
 for key,body in pairs(A.massTracked) do
  local holders=0
  for _,hold in ipairs(holds) do if integer(hold[1])==1 and integer(hold[2])==body.proxy and integer(hold[3])==body.body then holders=holders+1 end end
  local mass=VRBodySolverMass(body.proxy,body.body);local baseline=A.massBaseline[key]
  local previous=A.massRestored[key]==true
  if holders==0 and baseline and mass==baseline then A.massRestored[key]=true end
  out[#out+1]={proxy=tostring(body.proxy),body=body.body,holders=holders,baseline=baseline,solverMass=mass,
    reportedCarryMass=mass>0 and VRBodyInfo(body.proxy,body.body).w or nil,previousReleaseVerified=previous}
 end
 return out
end
function A.near(side)
 return GetMod("CyberpunkVRPort_HandCollision").canGrabNear(side)
end
function A.removeNPC()
 Game.GetDynamicEntitySystem():DeleteTagged(A.tag)
 A.npc,A.npcID,A.slots=nil,nil,nil;A.bodies={}
end
function A.restore()
 local o=A.original;if not o then return end
 A.removeNPC()
 local p=Game.GetPlayer();assert(p and tostring(p:GetEntityID().hash)==o.entity,"player changed; restore requires original player")
 local h=GetMod("CyberpunkVRPort_Holster");if h then h.setHidden(o.holsters==true) end
 Game.GetTeleportationFacility():Teleport(p,o.position,Quaternion.ToEulerAngles(o.rotation))
 local sys=Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
 if o.active then
  local r=DrawItemRequest.new();r.owner=p;r.itemID=o.active;r.equipAnimationType=gameEquipAnimationType.Instant;sys:QueueRequest(r)
 else
  local r=EquipmentSystemWeaponManipulationRequest.new();r.owner=p;r.requestType=EquipmentManipulationAction.UnequipAll
  r.equipAnimType=gameEquipAnimationType.Instant;r.removeItemFromEquipSlot=false;sys:QueueRequest(r)
 end
end
function A.restored()
 local o=assert(A.original);local p=Game.GetPlayer();local eq=EquipmentSystem.GetData(p);local same=eq:GetActiveItem(gamedataEquipmentArea.ArmsCW)==o.arms
 for i=0,2 do same=same and eq:GetItemInEquipSlot(gamedataEquipmentArea.Weapon,i)==o.weapons[i+1] end
 local weapon=p:GetActiveWeapon();local active=weapon and weapon:GetItemID()
 same=same and ((not active and not o.active) or active==o.active)
 local tagged=Game.GetDynamicEntitySystem():GetTagged(A.tag);local ids0,ids1=VRPhysHeldBody(0),VRPhysHeldBody(1)
 local inventory=A.inventoryDifference(o.inventory,inventoryState())
 return {player=playerState(),original={position=xyz(o.position),forward=xyz(o.forward),rotation=quat(o.rotation)},equipment=same,
  inventory=inventory.equal,inventoryDifference=inventory,
  npcRemoved=not tagged or #tagged==0,noHold=integer(ids0[1])==0 and integer(ids1[1])==0,
  money=Game.GetTransactionSystem():GetItemQuantity(p,ItemID.FromTDBID(TweakDBID.new("Items.money")))==o.money}
end
function A.restorePosition()
 local p=Game.GetPlayer();local o=assert(A.original)
 Game.GetTeleportationFacility():Teleport(p,o.position,Quaternion.ToEulerAngles(p:GetWorldOrientation()))
end
