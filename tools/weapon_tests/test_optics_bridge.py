"""Exercise the real optics bridge serialization and compiled geometry checker offline."""
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
from lupa.luajit21 import LuaRuntime

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('weapon_optics_fixture',ROOT/'tools/weapon_tests/live/run.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
lua=LuaRuntime(unpack_returned_tuples=True)
lua.execute('''
CName={new=function(v)return v end}
gamedataNewPerkType={Cool_Central_Milestone_2=1}
gamedataDevelopmentPointType={Primary=1}
Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w}end}
Transform={new=function()return {}end}
WorldPosition={ToVector4=function(p)return p end}
local slots={GetSlotTransform=function(_,name)
 assert(name=='vrp_scope_hinge')
 return true,{Position={x=-.0595094006,y=.1643638775,z=-.0267940455},
 Orientation={i=-.7071067811865476,j=0,k=0,r=.7071067811865476}}
end}
local weapon={GetWorldPosition=function()return {x=-.032,y=.15,z=0}end,
 GetWorldOrientation=function()return {i=0,j=0,k=0,r=1}end,
 FindComponentByName=function(_,name)assert(name=='FX_slots');return slots end}
local player={GetActiveWeapon=function()return weapon end}
local camera={GetActiveCameraWorldTransform=function(_,t)t.position={x=0,y=0,z=0}end,
 GetActiveCameraForward=function()return {x=0,y=1,z=0}end,
 GetActiveCameraRight=function()return {x=1,y=0,z=0}end}
hit={x=0,y=25,z=0};rayCalls=0
local spatial={SyncRaycastByQueryPreset=function(_,a,b,preset,staticOnly)
 assert(a.x==0 and a.y==0 and b.y==150 and preset=='Sight Blocker' and staticOnly==true)
 rayCalls=rayCalls+1;return true,{position=hit}
end}
Game={GetPlayer=function()return player end,GetCameraSystem=function()return camera end,
 GetSpatialQueriesSystem=function()return spatial end}
GetMod=function()return {scopeOffset={x=0,y=0,z=0},eyeToAxis=0,sightApproach=0}end
GetVRSharedSlot=function(id)assert(id==95);return .032 end
GetVRMainEyeOffset=function()return 0 end
''')
lua.execute((ROOT/'tools/weapon_tests/live/bridge.lua').read_text(encoding='utf-8'))
lua.execute('WeaponAudit.guard=function()end') # Save protection has its own production regression.

def plain(value):
    if hasattr(value,'items'):
        entries=dict(value.items())
        if entries and set(entries)==set(range(1,len(entries)+1)):
            return [plain(entries[i]) for i in range(1,len(entries)+1)]
        return {key:plain(item) for key,item in entries.items()}
    return value

audit=object.__new__(module.Audit)
audit.out=Path(tempfile.mkdtemp(prefix='weapon-optics-bridge-',dir=ROOT/'build'))
audit.args=SimpleNamespace(generator=ROOT/'build/render-runtime-audit/Release/render_audit_math.exe')
audit.call=lambda code:plain(lua.eval(code))
result=audit.optics_geometry('distant',scene=True)
assert result['aligned'] and result['distantStaticTarget'] and result['bestEye']==0
lua.execute('hit={x=0,y=2,z=0}')
result=audit.optics_geometry('near',scene=True)
assert result['aligned'] and not result['distantStaticTarget'] and lua.globals().rayCalls==2
print('PASS actual Lua array call serialization + measured transforms + C++ distant/near target gate')
