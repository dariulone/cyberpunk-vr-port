local root = ...
local nativeLocked, managerLocked, invalidations = true, true, 0
local markerCreates, markerDeletes = 0, 0
local state = {managed=false,spawning=false,spawned=false,entity=nil,acceptDelete=false,deleteImmediate=false}
local id = {hash=12345}; local mounted = nil
CName={new=function(v)return v end};TweakDBID={new=function(v)return v end}
Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w} end}
MappinData={new=function()return {} end};DynamicEntitySpec={new=function()return {} end}
gamedataMappinVariant={CustomPositionVariant=1};TweakDB={GetRecord=function()return {} end}
GoalSaveGuard={name="cpvr_goal_root_ab123",verified=true,verification="observer-and-engine"}
-- Native CET can wrap the same EntityID in a different userdata on every call.
local entity={GetEntityID=function()return {hash=id.hash} end}
local system={CreateEntity=function()return id end,GetEntity=function()return state.entity end,
 IsManaged=function()return state.managed end,IsSpawning=function()return state.spawning end,
 IsSpawned=function()return state.spawned end,DeleteEntity=function(_,target)
  assert(target==id,"only this run's exact entity ID may be deleted")
  if state.acceptDelete and state.deleteImmediate then state.managed=false;state.spawning=false;state.spawned=false;state.entity=nil end
  return state.acceptDelete
 end}
local player={GetMountedVehicle=function()return mounted end,GetEntityID=function()return {hash=9} end,
 GetWorldPosition=function()return {x=1,y=2,z=3} end,GetWorldForward=function()return {x=0,y=1,z=0} end,
 GetWorldOrientation=function()return {} end}
Game={GetPlayer=function()return player end,GetDynamicEntitySystem=function()return system end,
 IsSavingLocked=function()return nativeLocked end,
 GetScriptableSystemsContainer=function()return {Get=function()return {IsSavingLocked=function()return managerLocked end} end} end,
 GetAutoSaveSystem=function()return {InvalidateAutoSaveRequests=function()invalidations=invalidations+1 end} end,
 GetAllBlackboardDefs=function()return {UI_System={IsInMenu="menu"}} end,
 GetBlackboardSystem=function()return {Get=function()return {GetBool=function()return false end} end} end,
 GetMappinSystem=function()return {RegisterMappin=function()markerCreates=markerCreates+1;return markerCreates end,
  UnregisterMappin=function()markerDeletes=markerDeletes+1 end} end}
dofile(root..'/tools/render_runtime_audit/bridge.lua')
local A=RenderRuntimeAudit
local function denied(fn) local ok=pcall(fn);assert(not ok,"unsafe operation was accepted") end
nativeLocked=false;denied(function()A.begin("first")end);assert(not A.session)
nativeLocked=true;GoalSaveGuard.verified=false;denied(function()A.begin("first")end)
GoalSaveGuard.verified=true;A.begin("first")
managerLocked=false;denied(function()A.markers("first",{{1,2,3}})end);assert(markerCreates==0)
managerLocked=true;A.markers("first",{{1,2,3}});assert(markerCreates==1)
A.spawnVehicle("first","Vehicle.Test",{1,2,3})
local pending=A.cleanup("first")
assert(not pending.complete and pending.vehiclePending and A.session,"unmaterialized spawn lost its owner")
assert(markerDeletes==1)
state.managed=true;state.spawning=true;state.acceptDelete=true
pending=A.cleanup("first");assert(not pending.complete and A.session,"asynchronous cancellation marked complete early")
state.spawning=false;state.spawned=true;state.entity=entity
assert(A.vehicle("first").observed)
mounted=entity;denied(function()A.cleanup("first")end);assert(A.session)
mounted=nil;GoalSaveGuard.name="cpvr_goal_root_cd456";denied(function()A.cleanup("first")end)
GoalSaveGuard.name="cpvr_goal_root_ab123";state.deleteImmediate=true
assert(A.cleanup("first").complete and not A.session)
assert(invalidations==2,"creation/cleanup invalidate pending autosave requests; root lock is retained")
assert(GoalSaveGuard.verified and GoalSaveGuard.removed~=true)
print("PASS root save protection, exact async entity ownership, mounted rejection and cleanup completion")
