-- Attach-only audit support. Load only after the root grants the exclusive
-- runtime lease. Never launches/exits/saves the game or touches an untagged NPC.
assert(WeaponAudit == nil, 'Previous WeaponAudit still owns its restoration snapshot')
WeaponAudit = {created={}, ammo={}, tag=CName.new('cpvr_weapon_audit_20261117')}
local A=WeaponAudit
local PERK=gamedataNewPerkType.Cool_Central_Milestone_2
local POINT=gamedataDevelopmentPointType.Primary
local function player() local p=Game.GetPlayer();assert(p,'no player');return p end
local function eq() return EquipmentSystem.GetData(player()) end
local function ts() return Game.GetTransactionSystem() end
local function equipment() return Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')) end
local function key(id) return ItemID.IsValid(id) and tostring(ItemID.GetCombinedHash(id)) or 'none' end
local function item(id)
 return {key=key(id),record=TDBID.ToStringDEBUG(ItemID.GetTDBID(id)),rngSeed=id.rngSeed,
   uniqueCounter=id.uniqueCounter,flags=id.flags}
end
local function inventory()
 local ok,list=ts():GetItemList(player());assert(ok,'inventory snapshot failed')
 local out={};for _,v in ipairs(list) do local id=v:GetID();out[key(id)]={id=id,quantity=ts():GetItemQuantity(player(),id)} end
 return out
end
local function effect(id)
 local list=Game.GetStatusEffectSystem():GetAppliedEffectsWithID(player():GetEntityID(),TweakDBID.new(id))
 return list and list[1]
end
local function held(side)
 return ts():GetItemInSlot(player(),TweakDBID.new(side==0 and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight'))
end
local function weapon(w)
 if not w then return false end
 return {item=item(w:GetItemID()),entity=tostring(w:GetEntityID().hash),ammo=WeaponObject.GetMagazineAmmoCount(w),
   capacity=WeaponObject.GetMagazineCapacity(w)}
end
local function settingsFileMod() return GetMod('CyberpunkVRPort_Holster') end
-- This engine does not expose OnSaveLockRequest observers. Root proves its
-- distinct lock by a mutation-free causal remove(false)/re-add(true) sequence.
-- A merely true engine check or an unverified token cannot substitute for that.
local function verifiedRootSaveGuard()
 local root=GoalSaveGuard
 return type(root)=='table' and root.verified==true and root.verification=='causal-remove-readd'
   and type(root.name)=='string' and root.name:match('^cpvr_goal_root_[a-f0-9]+$')~=nil
   and root.removed~=true and (not A.rootSaveLockName or A.rootSaveLockName==root.name)
end
-- The combined boot hands off an exclusively owned, already save-locked PID.
-- A boot with opaque reasons still exposes the manager's native locked query.
local function hasBootLock()
 if not A.bootLockManager or not A.bootLockManager:IsSavingLocked() then return false end
 if A.bootLockReason then
  for _,reason in ipairs(A.bootLockManager.m_saveLocks or {}) do if reason==A.bootLockReason then return true end end
  return false
 end
 return true
end
function A.acquireSaveLock(name,allowNativeBootLock)
 assert(type(name)=='string' and name:match('^cpvr_weapon_audit_[a-f0-9]+$'),'invalid own save-lock name')
 assert(not A.saveLockName,'save lock already requested')
 if allowNativeBootLock and not verifiedRootSaveGuard() then
  local manager=Game.GetScriptableSystemsContainer():Get(CName.new('SaveLocksManager'))
  assert(Game.IsSavingLocked() and manager:IsSavingLocked(),'combined boot must already own a native save lock')
  A.bootLockManager=manager
  for _,reason in ipairs(manager.m_saveLocks or {}) do
   if reason~=CName.new(name) then A.bootLockReason=reason;break end
  end
 end
 assert(verifiedRootSaveGuard() or hasBootLock(),'verified causal root guard or existing native boot lock required')
 local blocked=Game.IsSavingLocked();assert(blocked==true,'engine saving is not blocked by verified root guard')
 local manager=assert(Game.GetScriptableSystemsContainer():Get(CName.new('SaveLocksManager')),'no SaveLocksManager')
 A.saveLockName=name;A.rootSaveLockName=verifiedRootSaveGuard() and GoalSaveGuard.name or nil
 assert(A.saveLockName~=A.rootSaveLockName,'audit must not own root lock')
 Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()
 local request=SaveLockRequest.new();request.reason=CName.new(name);request.operation=EItemOperationType.ADD
 manager:QueueRequest(request)
 A.saveLockRequested=true
end
function A.saveLockState()
 local blocked,locks=Game.IsSavingLocked()
 assert(type(blocked)=='boolean','authoritative Game.IsSavingLocked result unavailable')
 return {name=A.saveLockName,rootName=A.rootSaveLockName,ownAddRequested=A.saveLockRequested==true,
   ownRemoveRequested=A.saveLockRemoveRequested==true,ownRequestAcknowledgementAvailable=false,
   protectedByVerifiedRoot=verifiedRootSaveGuard() and blocked,protectedByNativeBootLock=hasBootLock() and blocked,
   engineBlocked=blocked,engineLockCount=locks and #locks or 0}
end
function A.requireSaveLock()
 local state=A.saveLockState()
 assert(state.ownAddRequested and not state.ownRemoveRequested and (state.protectedByVerifiedRoot or state.protectedByNativeBootLock),
   'verified root save protection is unavailable or audit is closing')
end
function A.begin()
 A.requireSaveLock()
 assert(not A.original,'snapshot already exists')
 local p=player();local e=eq();local d=PlayerDevelopmentSystem.GetData(p)
 local board=p:GetPlayerStateMachineBlackboard();local defs=GetAllBlackboardDefs().PlayerStateMachine
 assert(board:GetInt(defs.Vehicle)==0,'leave vehicle before weapon audit')
 assert(board:GetInt(defs.Combat)~=1,'leave combat before weapon audit')
 assert(not effect('BaseStatusEffect.ValentinosFrenzySE'),'existing Hurricane must finish first')
 local tagOk,tagged=ts():GetItemListByTag(p,A.tag)
 assert(tagOk and #tagged==0,'previous tagged weapon audit items need restoration first')
 local inv=inventory();local slots={}
 for i=0,e:GetNumberOfSlots(gamedataEquipmentArea.Weapon,true)-1 do slots[i+1]=e:GetItemInEquipSlot(gamedataEquipmentArea.Weapon,i) end
 local c=p:GetFPPCameraComponent();local cameraPosition=c:GetLocalPosition()
 local o={player=p,entity=tostring(p:GetEntityID().hash),inventory=inv,slots=slots,
   selected=e:GetActiveItem(gamedataEquipmentArea.Weapon),active=p:GetActiveWeapon() and p:GetActiveWeapon():GetItemID(),
   perk=d:IsNewPerkBought(PERK),perkUnlocked=d:IsNewPerkUnlocked(PERK),points=d:GetDevPoints(POINT),firedFact=Game.GetQuestsSystem():GetFactStr('player_fired_a_weapon'),
   cameraPosition=cameraPosition,holsters=settingsFileMod().hidden(),held={},holsterPlaces={},
   gadget=e:GetItemIDFromHotkey(EHotkey.RB),stamina=Game.GetStatPoolsSystem():GetStatPoolValue(p:GetEntityID(),gamedataStatPoolType.Stamina,false)}
 for i=0,2 do local info=settingsFileMod().slotInfo(i);o.holsterPlaces[i+1]=info and info.choice or 'auto' end
 for side=0,1 do local w=held(side);o.held[side+1]=w and {id=w:GetItemID(),ammo=WeaponObject.GetMagazineAmmoCount(w)} or false end
 local tequila=effect('BaseStatusEffect.Triple_Tequila_Stack')
 if tequila then
  o.tequila={count=tequila:GetStackCount(),remaining=tequila.remainingDuration,duration=tequila.duration,
    initial=tequila.initialApplicationTimestamp,last=tequila.lastApplicationTimestamp,
    instigator=tequila.instigatorRecordID,instigatorEntity=tequila.instigatorEntityID,direction=tequila.direction}
 end
 A.original=o
 local encoded={equipment={},inventory={},perk=o.perk,perkUnlocked=o.perkUnlocked,points=o.points,firedFact=o.firedFact,holsters=o.holsters,
   money=ts():GetItemQuantity(p,ItemID.CreateQuery(TweakDBID.new('Items.money'))),
   leftHanded=VRLeftHanded(),tequila=o.tequila and {count=o.tequila.count,remaining=o.tequila.remaining} or false}
 for i,id in ipairs(slots) do encoded.equipment[i]=item(id) end
 for _,v in pairs(inv) do local row=item(v.id);row.quantity=v.quantity;encoded.inventory[#encoded.inventory+1]=row end
 return encoded
end
function A.guard()
 assert(A.original and tostring(player():GetEntityID().hash)==A.original.entity,'player/save changed; do not mutate another player')
 A.requireSaveLock()
end
function A.perkReady(ordinaryOnly)
 A.guard();local d=PlayerDevelopmentSystem.GetData(player())
 if not ordinaryOnly then assert(d:HasEnoughtAttributePoints(PERK),'fixture does not meet native attribute requirement') end
 if not d:IsNewPerkUnlocked(PERK) then assert(d:UnlockNewPerk(PERK),'native perk unlock failed') end
 local max=d:GetNewPerkMaxLevel(PERK);assert(max>=2,'native Hurricane perk definition unavailable')
 local target=ordinaryOnly and 1 or max
 for _=1,target do if d:IsNewPerkBought(PERK)>=target then break end;assert(d:BuyNewPerk(PERK,true),'native forceBuy failed') end
 assert(d:IsNewPerkBought(PERK)>=target,'required dual-wield rank not active');return target
end
function A.create(record,index)
 A.guard();local id=ItemID.CreateFromSeedWithOffset(TweakDBID.new(record),3235770000,index)
 assert(ItemID.IsValid(id) and not ItemID.IsQuery(id),'test ItemID is not an exact instance')
 assert(not ts():HasItem(player(),id),'test ItemID already exists')
 assert(ts():GiveItem(player(),id,1,{A.tag}),'test item creation failed')
 local data=assert(ts():GetItemData(player(),id),'created item unavailable');id=data:GetID()
 assert(not A.original.inventory[key(id)],'creation resolved an original item')
 A.created[#A.created+1]=id
 return {index=#A.created,item=item(id)}
end
function A.holster()
 A.guard();local r=EquipmentSystemWeaponManipulationRequest.new();r.owner=player()
 r.requestType=EquipmentManipulationAction.UnequipAll;r.equipAnimType=gameEquipAnimationType.Instant
 r.removeItemFromEquipSlot=false;equipment():QueueRequest(r)
end
function A.unequipSlots()
 A.guard();for i=0,#A.original.slots-1 do local r=UnequipRequest.new();r.owner=player()
  r.areaType=gamedataEquipmentArea.Weapon;r.slotIndex=i;r.force=true;equipment():QueueRequest(r) end
end
local function equip(id,slot)
 local r=GameplayEquipRequest.new();r.owner=player();r.itemID=id;r.slotIndex=slot
 r.forceEquipWeapon=false;r.blockUpdateWeaponActiveSlots=true;equipment():QueueRequest(r)
end
function A.loadout(rightIndex,leftIndex)
 A.guard();equip(assert(A.created[rightIndex]),0)
 if leftIndex then equip(assert(A.created[leftIndex]),3) end
end
function A.draw(index)
 A.guard();local r=DrawItemRequest.new();r.owner=player();r.itemID=assert(A.created[index])
 r.equipAnimationType=gameEquipAnimationType.Instant;equipment():QueueRequest(r)
end
function A.fill()
 A.guard();local U=GetMod('CyberpunkVRPort_Reload').reload().util
 for side=0,1 do local w=held(side);if w then
  local ammoID=WeaponObject.GetAmmoType(w);local rec=ItemID.GetTDBID(ammoID);local name=TDBID.ToStringDEBUG(rec)
  if not A.ammo[name] then
   local query=ItemID.CreateQuery(rec);local before=0
   for _,v in pairs(A.original.inventory) do if ItemID.IsOfTDBID(v.id,rec) then before=before+v.quantity end end
   A.ammo[name]={id=query,record=rec,quantity=before}
   assert(ts():GiveItemByTDBID(player(),rec,200),'ammo top-up failed')
  end
  assert(U.setMagAmmo(w,U.magCapacity(w)),'magazine fill failed')
 end end
end
function A.dry()
 A.guard();local w=assert(player():GetActiveWeapon(),'no active weapon')
 assert(GetMod('CyberpunkVRPort_Reload').reload().util.setMagAmmo(w,0),'empty magazine request failed')
end
function A.sample()
 local p=player();local board=p:GetPlayerStateMachineBlackboard();local defs=GetAllBlackboardDefs().PlayerStateMachine
 local g=GetMod('CyberpunkVRPort_Reload').glow().debug();local gl={}
 for side=0,1 do local c=p:FindComponentByName(CName.new(side==0 and 'vrp_glow_left' or 'vrp_glow_right'))
  gl[side+1]={look=g.look[side] or false,enabled=c and c:IsEnabled() or false} end
 return {tick=GetVRSharedSlot(99),dual=GameObject.IsDualWielding(p),flags=GetVRSharedSlot(165),
   shoot=board:GetInt(defs.ShootState),upper=board:GetInt(defs.UpperBody),
   frenzy=effect('BaseStatusEffect.ValentinosFrenzySE')~=nil,left=weapon(held(0)),right=weapon(held(1)),glow=gl}
end
function A.optics()
 local w=assert(player():GetActiveWeapon());local out={weapon=weapon(w),components={},state=A.sample()}
 for _,c in ipairs(Game.VREntityComponents(w)) do
  if c:IsA(CName.new('entSkinnedMeshComponent')) or c:IsA(CName.new('entPhysicalMeshComponent')) then
   local name=NameToString(c:GetName())
   if name:find('scope',1,true) or name:find('osprey',1,true) then
    out.components[#out.components+1]={name=name,enabled=c:IsEnabled(),visibility=NameToString(c.visibilityAnimationParam)}
   end
  end
 end
 local m=GetMod('CyberpunkVRPort_Optics');out.path=m.path;out.error=m.error;out.eyeToAxis=m.eyeToAxis;out.sightApproach=m.sightApproach
 out.frameError=m.frameError;out.hingeRequested=m.hingeRequested;out.hingeError=m.hingeError
 return out
end
function A.opticsGeometry()
 A.guard()
 local w=assert(player():GetActiveWeapon(),'no optic weapon')
 local cam=Game.GetCameraSystem();local t=Transform.new();cam:GetActiveCameraWorldTransform(t)
 local function xyz(v)return {v.x,v.y,v.z}end
 local q=w:GetWorldOrientation();local mod=GetMod('CyberpunkVRPort_Optics')
 local offset=assert(mod.scopeOffset,'authored optic offset unavailable')
 assert(type(GetVRMainEyeOffset)=='function','MAIN eye offset API unavailable')
 local scopeBone=false
 local slots=w:FindComponentByName(CName.new('FX_slots'))
 if slots then
  local found,transform=slots:GetSlotTransform(CName.new('vrp_scope_hinge'))
  if found and transform then local p=WorldPosition.ToVector4(transform.Position);local bq=transform.Orientation
   scopeBone={slot='vrp_scope_hinge',p=xyz(p),q={bq.i,bq.j,bq.k,bq.r}}
  end
 end
 return {camera={p=xyz(t.position),forward=xyz(cam:GetActiveCameraForward()),right=xyz(cam:GetActiveCameraRight()),
  halfIpd=GetVRSharedSlot(95),mainEyeOffset=GetVRMainEyeOffset()},
  weapon={p=xyz(w:GetWorldPosition()),q={q.i,q.j,q.k,q.r}},scopeOffset=xyz(offset),
  scopeBone=scopeBone,modEyeToAxis=mod.eyeToAxis,sightApproach=mod.sightApproach}
end
function A.opticsRay(a,b)
 A.guard()
 local hit,tr=Game.GetSpatialQueriesSystem():SyncRaycastByQueryPreset(
  Vector4.new(a[1],a[2],a[3],1),Vector4.new(b[1],b[2],b[3],1),'Sight Blocker',true,false)
 return hit and tr and tr.position and {tr.position.x,tr.position.y,tr.position.z} or false
end
function A.opticsPanels()
 A.guard();local out={};local weapon=assert(player():GetActiveWeapon())
 for _,name in ipairs({'vr_optic_panel','vr_optic_reticle_rear_panel','on_weapon_ui3445'}) do
  local component=weapon:FindComponentByName(CName.new(name));local row={name=name,present=component~=nil}
  if component then
   row.enabled=component:IsEnabled();local root=component:GetWidget();row.rootPresent=root~=nil
   if root then
    row.widgets={};local function walk(w,path,depth)
     if #row.widgets>=64 then return end
     local size=w:GetSize();local controller=w:GetController()
     row.widgets[#row.widgets+1]={path=path,name=NameToString(w:GetName()),visible=w:IsVisible(),opacity=w:GetOpacity(),
      size={size.X,size.Y},controller=controller and NameToString(controller:GetClassName()) or nil}
     if depth<5 and w:IsA(CName.new('inkCompoundWidget')) then
      for i=0,w:GetNumChildren()-1 do walk(w:GetWidgetByIndex(i),path..'/'..i,depth+1)end
     end
    end
    walk(root,'root',0)
   end
  end
  out[#out+1]=row
 end
 return out
end
function A.opticsProbe(name)
 A.guard();assert(name=='vr_optic_panel' or name=='vr_optic_reticle_rear_panel' or name=='on_weapon_ui3445')
 local component=assert(player():GetActiveWeapon():FindComponentByName(CName.new(name)))
 return Game.VRPhysicalOpticsProbe(component)
end
function A.camera(x,y,z)
 A.guard();player():GetFPPCameraComponent():SetLocalPosition(Vector4.new(x,y,z,1))
end
function A.belt()
 return {holsters=settingsFileMod().state(),grenade=GetMod('CyberpunkVRPort_TacticalBelt').state()}
end
function A.grenade(index)
 A.guard();eq():AssignItemToHotkey(assert(A.created[index]),EHotkey.RB)
end
function A.restoreSlots()
 A.guard();A.holster();A.unequipSlots()
 for i,id in ipairs(A.original.slots) do if ItemID.IsValid(id) then equip(id,i-1) end end
 player():GetFPPCameraComponent():SetLocalPosition(A.original.cameraPosition)
end
function A.restoreDraw()
 A.guard();local id=A.original.active or A.original.selected
 if ItemID.IsValid(id) then local r=DrawItemRequest.new();r.owner=player();r.itemID=id
  r.equipAnimationType=gameEquipAnimationType.Instant;equipment():QueueRequest(r) end
end
function A.restoreFinish()
 A.guard();local o=A.original;local p=player();local d=PlayerDevelopmentSystem.GetData(p)
 -- Undo only the temporary perk ranks; forceBuy never changes spent points.
 while d:IsNewPerkBought(PERK)>o.perk do local ok=d:ForceSellNewPerk(PERK);assert(ok,'perk removal failed') end
 assert(d:IsNewPerkBought(PERK)==o.perk,'perk rank mismatch')
 d:AddDevelopmentPoints(o.points-d:GetDevPoints(POINT),POINT)
 if not o.perkUnlocked and d:IsNewPerkUnlocked(PERK) then assert(d:LockNewPerk(PERK),'original perk availability restore failed') end
 for _,saved in ipairs(o.held) do if saved then
  for side=0,1 do local w=held(side);if w and key(w:GetItemID())==key(saved.id) then
   assert(GetMod('CyberpunkVRPort_Reload').reload().util.setMagAmmo(w,saved.ammo),'original magazine restore failed')
  end end
 end end
 Game.GetQuestsSystem():SetFactStr('player_fired_a_weapon',o.firedFact)
 eq():AssignItemToHotkey(o.gadget,EHotkey.RB)
 settingsFileMod().setHidden(o.holsters)
 for i,place in ipairs(o.holsterPlaces) do settingsFileMod().setPlace(i-1,place) end
 -- GiveItem's dynamic tags are not retained by every weapon archetype. Exact
 -- created IDs are authoritative; the tag scan also catches partial creation.
 for _,id in ipairs(A.created) do assert(not o.inventory[key(id)],'refuse to remove an original item')
  local n=ts():GetItemQuantity(p,id);if n>0 then assert(ts():RemoveItem(p,id,n),'exact test item removal failed') end end
 -- Include a GiveItem that succeeded before a later lookup threw: every item
 -- with this tag was absent at begin, and original inventory IDs remain protected.
 local ok,tagged=ts():GetItemListByTag(p,A.tag);assert(ok,'tagged item cleanup scan failed')
 for _,data in ipairs(tagged) do local id=data:GetID();assert(not o.inventory[key(id)],'refuse to remove an original item')
  local n=ts():GetItemQuantity(p,id);if n>0 then assert(ts():RemoveItem(p,id,n),'test item removal failed') end end
end
function A.restoreAmmo()
 A.guard();local p=player()
 for _,v in pairs(A.ammo) do local delta=v.quantity-ts():GetItemQuantity(p,v.id)
  if delta>0 then assert(ts():GiveItemByTDBID(p,v.record,delta),'ammo restore failed')
  elseif delta<0 then assert(ts():RemoveItem(p,v.id,-delta),'ammo excess removal failed') end
 end
 local t=A.original.tequila
 if t and not effect('BaseStatusEffect.Triple_Tequila_Stack') then
  assert(Game.GetStatusEffectSystem():ApplyStatusEffect(p:GetEntityID(),TweakDBID.new('BaseStatusEffect.Triple_Tequila_Stack'),t.instigator,t.instigatorEntity,t.count,t.direction),'Tequila restore failed')
  local restored=assert(effect('BaseStatusEffect.Triple_Tequila_Stack'))
  restored.remainingDuration=t.remaining;restored.duration=t.duration
  restored.initialApplicationTimestamp=t.initial;restored.lastApplicationTimestamp=t.last
 end
 if not A.original.active then A.holster() end
 Game.GetStatPoolsSystem():RequestSettingStatPoolValue(p:GetEntityID(),gamedataStatPoolType.Stamina,A.original.stamina,p,false)
end
function A.restored()
 A.guard();local o=A.original;local current=inventory();local differences={}
 for k,v in pairs(o.inventory) do if not current[k] or current[k].quantity~=v.quantity then differences[#differences+1]='original inventory '..k end end
 for k in pairs(current) do if not o.inventory[k] then differences[#differences+1]='extra inventory '..k end end
 for i,id in ipairs(o.slots) do if key(eq():GetItemInEquipSlot(gamedataEquipmentArea.Weapon,i-1))~=key(id) then differences[#differences+1]='weapon slot '..(i-1) end end
 local active=player():GetActiveWeapon();if (active and key(active:GetItemID()) or 'none')~=(o.active and key(o.active) or 'none') then differences[#differences+1]='active weapon' end
 local d=PlayerDevelopmentSystem.GetData(player())
 if d:IsNewPerkBought(PERK)~=o.perk or d:GetDevPoints(POINT)~=o.points or d:IsNewPerkUnlocked(PERK)~=o.perkUnlocked then differences[#differences+1]='perk/points/unlocked' end
 if Game.GetQuestsSystem():GetFactStr('player_fired_a_weapon')~=o.firedFact then differences[#differences+1]='firing fact' end
 if key(eq():GetItemIDFromHotkey(EHotkey.RB))~=key(o.gadget) then differences[#differences+1]='gadget hotkey' end
 local tequila=effect('BaseStatusEffect.Triple_Tequila_Stack')
 if (tequila and tequila:GetStackCount() or 0)~=(o.tequila and o.tequila.count or 0) then differences[#differences+1]='Tequila stacks' end
 return {ok=#differences==0,differences=differences,state=A.sample(),
   stamina={requested=o.stamina,current=Game.GetStatPoolsSystem():GetStatPoolValue(player():GetEntityID(),gamedataStatPoolType.Stamina,false)}}
end
function A.releaseSaveLock()
 assert(A.saveLockName and A.saveLockRequested,'own save lock was not requested')
 if A.saveLockRemoveRequested then return end
 A.requireSaveLock()
 -- No snapshot means begin failed before any actor/input mutations. Otherwise
 -- verify exact restoration in the game before permitting even our own unlock.
 if A.original then assert(A.restored().ok,'restoration incomplete: retain own save lock') end
 Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()
 local request=SaveLockRequest.new();request.reason=CName.new(A.saveLockName);request.operation=EItemOperationType.REMOVE
 Game.GetScriptableSystemsContainer():Get(CName.new('SaveLocksManager')):QueueRequest(request)
 A.saveLockRemoveRequested=true
end
