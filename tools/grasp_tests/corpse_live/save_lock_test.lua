local root=... or "."
CName={new=function(x)return x end};NameToString=function(x)return x end
EItemOperationType={ADD="ADD",REMOVE="REMOVE"};SaveLockRequest={new=function()return {}end}
local pending,events={},{}
local reasons={"CPVR_Root_Lock"}
local manager={m_saveLocks=reasons,QueueRequest=function(_,r)pending[#pending+1]=r;events[#events+1]=r.operation end,
 IsSavingLocked=function()return #reasons>0 end}
local engine=true
Game={GetScriptableSystemsContainer=function()return {Get=function(_,name)assert(name=="SaveLocksManager");return manager end}end,
 IsSavingLocked=function()return engine,{}end,GetAutoSaveSystem=function()return {InvalidateAutoSaveRequests=function()events[#events+1]="invalidate"end}end}
local function drain()
 for _,r in ipairs(pending)do
  if r.operation=="ADD"then reasons[#reasons+1]=r.reason
  else for i=#reasons,1,-1 do if reasons[i]==r.reason then table.remove(reasons,i)end end end
 end
 pending={}
end
dofile(root.."/tools/grasp_tests/corpse_live/bridge.lua")
local a=CorpseAudit
assert(not pcall(a.prepare),"gameplay mutation entered without lock verification")
a.lockSaving("CPVR_CorpseAudit_abc123")
assert(a.saveLockState().engineLocked and not a.saveLockState().owned,"independent root lock mistaken for own queued request")
assert(not pcall(a.confirmSaveLock),"pending own lock accepted")
drain();engine=false
assert(a.saveLockState().owned and not pcall(a.confirmSaveLock),"manager membership bypassed engine saving check")
engine=true;local state=a.confirmSaveLock();assert(state.owned and state.engineLocked and state.verified)
assert(events[#events]=="invalidate","pending autosaves not invalidated before gameplay")
assert(not pcall(a.releaseSaveLock,false),"failed restoration removed save lock")
assert(a.saveLockState().owned,"failed restoration lost owned lock")
a.releaseSaveLock(true)
assert(events[#events-1]=="invalidate" and events[#events]=="REMOVE","unlock did not invalidate autosaves first")
assert(a.saveLockState().owned,"queued REMOVE claimed instantaneous completion")
drain();state=a.saveLockState()
assert(not state.owned and state.engineLocked and #manager.m_saveLocks==1 and manager.m_saveLocks[1]=="CPVR_Root_Lock","removed another owner's lock")
-- Actual host: private list unavailable and script dispatch emits no observer ACK.
manager.m_saveLocks=nil
dofile(root.."/tools/grasp_tests/corpse_live/bridge.lua");a=CorpseAudit
a.lockSaving("CPVR_CorpseAudit_def456")
assert(not pcall(a.confirmSaveLock),"standalone unobservable membership accepted without verified root protection")
GoalSaveGuard={name="cpvr_goal_root_a1",verified=true,verification="causal-remove-readd",removed=false}
state=a.confirmSaveLock();assert(state.protected and not state.membershipKnown and state.rootVerified)
assert(not pcall(a.releaseSaveLock,false),"root protection bypassed failed restore")
drain();a.releaseSaveLock(true);drain()
assert(#reasons==1 and reasons[1]=="CPVR_Root_Lock","fallback release removed unrelated root reason")
print("PASS unique queued save lock, own+engine verification, mutation guard, failure retention, invalidation-before-remove, root lock preservation")
