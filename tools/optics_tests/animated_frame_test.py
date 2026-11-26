"""Exercise the production optical frame against an independently moving skin bone."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().source_root=str(ROOT/'mods/cet/CyberpunkVRPort_Optics').replace('\\','/')
lua.execute('''
package.path=source_root..'/?.lua;'..package.path
function registerForEvent()end
CName={new=function(s)return s end}
WorldPosition={ToVector4=function(p)return p end}
Quaternion={new=function(i,j,k,r)return {i=i,j=j,k=k,r=r}end}
function upvalue(fn,wanted)
 for i=1,100 do local n,v=debug.getupvalue(fn,i);if not n then break end;if n==wanted then return v end end
 error('missing upvalue '..wanted)
end
''')
lua.globals().mod=lua.execute((ROOT/'mods/cet/CyberpunkVRPort_Optics/init.lua').read_text(encoding='utf-8'))
lua.execute('''
local frame=upvalue(mod.update,'opticalFrame');local state=upvalue(frame,'state')
local position={x=10,y=20,z=30};local rotation={i=0,j=0,k=0,r=1};local found=true
state.profile={key='osprey'}
state.opticSlot={GetSlotTransform=function(_,name)
 assert(name=='vrp_scope_hinge');return found,{Position=position,Orientation=rotation}
end}
local weapon={GetWorldPosition=function()return {x=100,y=200,z=300}end,
 GetWorldOrientation=function()return {i=0,j=0,k=0,r=1}end,
 GetScopeOffset=function()return {x=1,y=2,z=3}end}
local p,q=frame(weapon)
assert(math.abs(p.x-10.0275094006)<1e-9 and math.abs(p.y-19.9732059545)<1e-9)
assert(math.abs(q.i-.7071067811865475)<1e-12)
position={x=-.0595094006,y=.1643638775,z=-.0267940455}
rotation={i=-.7071067811865476,j=0,k=0,r=.7071067811865476}
p,q=frame(weapon)
assert(math.abs(p.x+.032)<1e-9 and math.abs(p.y-.15)<1e-9 and math.abs(p.z)<1e-9,
 'animated aperture fell back to weapon-root bind space')
assert(math.abs(q.r-1)<1e-12 and math.abs(q.i)<1e-12)
found=false;assert(frame(weapon)==nil,'unavailable slot silently invented a pose')
state.opticSlot=nil;assert(frame(weapon)==nil,'missing component silently invented a pose')
state.profile={key='long_01'};p,q=frame(weapon)
assert(p.x==101 and p.y==202 and p.z==303,'ordinary fixed scope frame changed')
''')
print('PASS production optical frame: animated bone position/rotation, missing-slot refusal, fixed-scope compatibility')
