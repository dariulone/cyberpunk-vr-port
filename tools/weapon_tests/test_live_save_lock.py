"""Exercise production audit lock ownership/ordering without attaching to a game."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

lua=LuaRuntime()
lua.execute('''
CName={new=function(s)return s end}
function NameToString(s)return s end
gamedataNewPerkType={Cool_Central_Milestone_2=34}
gamedataDevelopmentPointType={Primary=0}
EItemOperationType={ADD=1,REMOVE=2}
SaveLockRequest={new=function()return {}end}
requests={};locks={root_session_guard=true};invalidations=0;engineEnabled=true
local manager={QueueRequest=function(_,request)table.insert(requests,request)end}
Game={GetScriptableSystemsContainer=function()return {Get=function()return manager end}end,
 GetAutoSaveSystem=function()return {InvalidateAutoSaveRequests=function()invalidations=invalidations+1 end}end,
 IsSavingLocked=function()local count=0;for _ in pairs(locks)do count=count+1 end;return engineEnabled and count>0,{}end}
function process()
 local request=table.remove(requests,1);assert(request)
 if request.operation==EItemOperationType.ADD then locks[request.reason]=true else locks[request.reason]=nil end
 return request
end
''')
source=(Path(__file__).parent/'live/bridge.lua').read_text(encoding='utf-8')
lua.execute(source)
lua.execute('''
assert(not pcall(WeaponAudit.acquireSaveLock,'cpvr_weapon_audit_abcdef0123'),'bare engine true must not authorize audit')
assert(#requests==0)
GoalSaveGuard={name='cpvr_goal_root_123abc',verified=false,verification='causal-remove-readd'}
assert(not pcall(WeaponAudit.acquireSaveLock,'cpvr_weapon_audit_abcdef0123'),'unverified root token accepted')
GoalSaveGuard.verified=true;GoalSaveGuard.verification='guessed'
assert(not pcall(WeaponAudit.acquireSaveLock,'cpvr_weapon_audit_abcdef0123'),'noncausal verification accepted')
GoalSaveGuard.verification='causal-remove-readd'
WeaponAudit.acquireSaveLock('cpvr_weapon_audit_abcdef0123')
assert(invalidations==1 and #requests==1)
assert(pcall(WeaponAudit.requireSaveLock),'verified root protection must not depend on an unconfirmed own observer ACK')
process();assert(locks.cpvr_weapon_audit_abcdef0123)
engineEnabled=false;assert(not pcall(WeaponAudit.requireSaveLock),'root token cannot replace current engine block')
engineEnabled=true
WeaponAudit.original={};WeaponAudit.restored=function()return {ok=false}end
assert(not pcall(WeaponAudit.releaseSaveLock),'failed restoration released save lock')
assert(#requests==0 and locks.cpvr_weapon_audit_abcdef0123)
WeaponAudit.restored=function()return {ok=true}end
WeaponAudit.releaseSaveLock();assert(invalidations==2 and #requests==1)
assert(requests[1].reason=='cpvr_weapon_audit_abcdef0123' and requests[1].operation==EItemOperationType.REMOVE)
process();local released=WeaponAudit.saveLockState()
assert(released.ownRemoveRequested and released.protectedByVerifiedRoot and locks.root_session_guard)
assert(not pcall(WeaponAudit.requireSaveLock),'closing audit accepted a new mutation')
WeaponAudit=nil
''')
lua.execute(source)
lua.execute('''
WeaponAudit.acquireSaveLock('cpvr_weapon_audit_112233');process()
assert(pcall(WeaponAudit.requireSaveLock))
GoalSaveGuard.name='cpvr_goal_root_456def'
assert(not pcall(WeaponAudit.requireSaveLock),'different root token silently replaced the proven guard')
GoalSaveGuard.name='cpvr_goal_root_123abc'
WeaponAudit.releaseSaveLock();process();assert(locks.root_session_guard)
''')
print('PASS production save protection: causal root token + engine check, failed restore retained, exact own REMOVE, stale token rejected')
