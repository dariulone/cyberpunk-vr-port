"""Run the exact hotload payload with the real module and verify callback/rollback ownership."""
from pathlib import Path
import importlib.util
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('optics_hot_payload',ROOT/'tools/weapon_tests/live/hotload_optics.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().policy=lua.execute((ROOT/'mods/cet/CyberpunkVRPort_Optics/optics/policy.lua').read_text())
lua.execute('''
eventCount=0;function registerForEvent()eventCount=eventCount+1 end
CName={new=function(s)return s end};WorldPosition={ToVector4=function(p)return p end}
Quaternion={new=function(i,j,k,r)return {i=i,j=j,k=k,r=r}end}
local player={GetActiveWeapon=function()return nil end,FindComponentByName=function()return nil end}
Game={GetPlayer=function()return player end,IsSavingLocked=function()return true end}
GoalSaveGuard={verified=true,verification='causal-remove-readd',name='cpvr_goal_root_deadbeef'}
oldUpdate=function()error('old callback unexpectedly ran')end
oldStats={applies=12};live={update=oldUpdate,path='none',enabled=false,policy=policy,stats=oldStats}
function GetMod(name)if name=='CyberpunkVRPort_Optics' then return live else return {}end end
''')
lua.execute(module.patch_source('offline_test',ROOT/'mods/cet/CyberpunkVRPort_Optics'))
lua.execute('''
assert(eventCount==0,'hotload registered duplicate events')
assert(live.update~=oldUpdate and GoalOpticsPatch.token=='offline_test')
live.update(.016)
assert(not live.enabled and live.policy==policy,'runtime options were replaced with defaults')
assert(GoalOpticsPatch.restore() and GoalOpticsPatch==nil)
assert(live.update==oldUpdate and live.stats==oldStats and live.path=='none','rollback lost owned original fields')
assert(eventCount==0,'rollback altered native event registrations')
''')
print('PASS exact hotload payload: no duplicate events, preserved options, one existing update replaced, owned rollback')
