"""A nearby limb may replace a farther ray hit only within the same ragdoll."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
s=(Path(__file__).resolve().parents[2]/'mods/cet/CyberpunkVRPort_HandCollision/init.lua').read_text(encoding='utf-8')
a=s.index('        local prefer = not rec\n');b=s.index('        if prefer and okB',a)
l=LuaRuntime(unpack_returned_tuples=True)
f=l.execute('''return function(rayKind, rayProxy, rayBody, nearKind, nearProxy, nearBody, rayDistance, nearDistance, failed)
local rec=rayKind and {kind=rayKind,proxy=rayProxy,index=rayBody} or nil
local info={x=nearKind,y=nearProxy,z=nearBody};local okB=true;local palm={}
VRBodySurface=function(proxy,body)if failed then error("body gone")end;return {w=body==rayBody and rayDistance or nearDistance}end
'''+s[a:b]+'''return prefer end''')
assert f(None,0,0,12,10,6,0,.02,False)
assert f(12,10,3,12,10,6,.11,.02,False)
assert not f(12,10,3,12,11,6,.11,.02,False)
assert not f(12,10,3,1,10,6,.11,.02,False)
assert not f(12,10,3,12,10,6,.02,.11,False)
assert not f(12,10,3,12,10,6,.025,.02,False)
assert not f(12,10,3,12,10,6,-1,.02,False)
assert not f(12,10,3,12,10,6,.11,.02,True)
print('PASS same-ragdoll closer-limb selection, unrelated objects, margin, failed/invalid surface')
