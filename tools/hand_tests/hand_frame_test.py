"""Native paired frames must bypass stale bones, parent transforms and RD bias."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
root=Path(__file__).resolve().parents[2]
source=(root/'mods/cet/CyberpunkVRPort_HandCollision/init.lua').read_text(encoding='utf-8')
def span(start,end):
    a=source.index(start);return source[a:source.index(end,a)]
frame=span('local nativeFrames, nativeFrameGen','-- HAIR THAT THE HANDS MOVE')
raw=span('local RD =','-- The middle of the palm')
palm=span('local function palmSkin(','local function carryAccel(')
find=span('local function carryFind(','local function carryDrive(')
tracked=span('K.handTracked = function(', 'K.npcGrabFollow = function(')
lua=LuaRuntime(unpack_returned_tuples=True)
lua.execute('''
K={PHYS=true,CARRY_PALM_REACH=.13,CARRY_REACH=.17,CARRY_NEAR=.1,CARRY_MAX_MASS=30}
gen=1;calls=0;valid=true;wake=0;casts=0
Vector4={new=function(x,y,z,w)return{x=x,y=y,z=z,w=w}end}
HAND={[0]={index=10,cx=1},[1]={index=20,cx=-1}};RAW={[1]={x=-500,y=-500,z=-500}};HOLD={};SKIP={};HV={[1]={0,0,0}}
FINGER_FROM_BODY=.3;WORLD_PRESETS={1};worldPreset=1
function bone()error('stale diagnostic bones were read')end
function qrot()error('native world data was transformed a second time')end
function qmul()error('native rotation was transformed a second time')end
function worldSystem()return{}end
function NameToString(s)return s end
function bodyWake()wake=wake+1 end
function bodyOf()return{kind=10,proxy=123,index=0,mass=2}end
function castRay(_,a,b)
 casts=casts+1;castFrom=a;castTo=b;return true,{material='metal',position=Vector4.new(10.04,20,30.1,1)}
end
function VRPhysHandFrame(side)
 calls=calls+1;if not valid then return{}end
 return{Vector4.new(10,20,30,1),Vector4.new(0,0,0,1),Vector4.new(11,21,31,1),Vector4.new(0,0,1,0),
 Vector4.new(.04,.01,.012,1),Vector4.new(0,0,1,1),Vector4.new(1,0,0,1),Vector4.new(.5,.4,1,1),Vector4.new(10.04,20,30,1)}
end
''')
api=lua.execute(frame+raw+palm+find+tracked+'''
return{world=handWorld,control=handReal,palm=palmSkin,measure=handMeasure,find=carryFind,tracked=K.handTracked}
''')
assert api.world(1,100,200,300,0,0,1,0)==(10,20,30,0,0,0,1)
assert api.control(1,100,200,300,0,0,1,0)==(11,21,31,0,0,1,0)
assert api.tracked(1,100,200,300,0,0,1,0)==(11,21,31,0,0,1,0)
api.measure(1)
assert api.palm(1)==(.04,.01,.012)
assert lua.globals().calls==1,'one script frame used several native pose samples'
assert api.find(1,100,200,300,0,0,1,0,None,True) is True
assert lua.globals().wake==0,'read-only selection woke a body'
assert lua.globals().casts==1
assert abs(lua.globals().castFrom.x-10.04)<1e-8
assert abs(lua.globals().castTo.z-30.13)<1e-8
lua.globals().valid=False;lua.globals().gen=2
assert api.world(1,100,200,300,0,0,1,0) is None
assert api.control(1,100,200,300,0,0,1,0) is None
assert api.tracked(1,100,200,300,0,0,1,0) is None
assert api.palm(1)==(.04,.01,.012),'tracking loss discarded rigid palm dimensions during release'
assert api.find(1,100,200,300,0,0,1,0,None,False) is None
assert lua.globals().casts==1,'invalid native pose fell back to stale geometry'
print('paired hand frame: world/control separation, measured palm, one sample, query-only probe and invalid tracking PASS')
