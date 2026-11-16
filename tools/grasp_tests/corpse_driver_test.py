"""Verify native corpse ownership retains query visibility and lost-grip cleanup."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
p=Path(__file__).resolve().parents[2]/'mods/cet/CyberpunkVRPort_HandCollision/init.lua'
s=p.read_text(encoding='utf-8');a=s.index('    if n == 0 then return end\n    -- Keep carried bodies');b=s.index('    -- where it should be,',a)
L=LuaRuntime(unpack_returned_tuples=True)
L.execute('''K={corpseTick=function() ticks=ticks+1 end,QUERY_HAND=.3,QUERY_BACK={},HAND_LOST_AT=.45,CORPSE_LOST=3};ticks=0
qrot=function(i,j,k,r,x,y,z)return x,y,z end
VRBodySetQueryable=function(body,on) queried=on end
handReal=function()return otherX,0,0 end
carryRelease=function(side) released[side]=true end''')
f=L.execute('''return function(kind,first,second,n,bodyX,lost,near)
 ticks=0;queried=nil;released={};otherX=near and bodyX or 10
 local rec={kind=kind,pictured=true};local pos={x=bodyX,y=0,z=0};local rot={i=0,j=0,k=0,r=1};local dt=.02
 local hands={[1]=0,[2]=1};local body=42
 local function hand(on)return {fast=on and {gx=0,gy=0,gz=0,cx=0,cy=0,cz=0} or nil,dx=3,lost=lost,px=0,py=0,pz=0,qi=0,qj=0,qk=0,qr=1}end
 HOLD={[0]=hand(first),[1]=hand(second)};local vx,vy,vz=0,0,0;
'''+s[a:b]+'''return "legacy" end''')
for first,second,n in [(True,False,1),(True,True,2)]:
 assert f(12,first,second,n,0,2,False) is None
 assert L.globals().ticks==1
 for side in range(n):assert L.globals().HOLD[side].dx is None and L.globals().HOLD[side].lost==0
 assert L.globals().queried is False
assert f(12,False,False,1,0,0,False)=='legacy'
assert f(12,True,False,2,0,0,False)=='legacy'
assert f(1,True,False,1,0,0,False)=='legacy'
f(12,True,False,1,0,0,True);assert L.globals().queried is True
f(12,True,True,2,1,3,False)
assert L.globals().released[0] and L.globals().released[1]
f(12,True,False,1,1,0,False);assert not L.globals().released[0]
print('PASS native one/two-hand ownership, legacy fallback, query visibility, timed lost-grip release')
# The dead-body profile uses the same body-type scale as living hand contacts.
R=LuaRuntime(unpack_returned_tuples=True)
R.execute('K={}; bodyType=function(e)return e.kind end; Game={FindEntityByID=function(id)return {kind=id} end}')
start=s.index('K.CORPSE_GRASP =');end=s.index('-- THE PART\'S DRAWN SURFACE',start)
R.execute(s[start:end])
radius=R.eval('function(segment,kind)return K.corpseRadius(segment,{id=kind})end')
for kind,scale in [('Man',1),('Woman',.88),('Fat',1.1),('Big',1.15)]:
 assert abs(radius(8,kind)-.04*scale)<1e-8
 assert abs(radius(12,kind)-.05*scale)<1e-8
taper=R.eval('function(t)return K.corpseRadius(8,{id="Woman"},t)end')
assert taper(.2)>taper(.5)>taper(.8)
assert abs(taper(.8)-.022*.88)<1e-8
assert taper(-1)==taper(0) and taper(2)==taper(1)
R.execute('''K.RAGDOLLS={[42]={kind='Woman'}};K.CORPSE_GRASP[8]={1,.04};K.CORPSE_CLOTH={};K.CORPSE_FLESH_LEAST=.7;K.CORPSE_FLESH_MOST=1.4
Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w}end}
qrot=function(i,j,k,r,x,y,z)return x,y,z end
VRBodySurface=function()return {x=surface,y=0,z=0,w=1}end
spdlog={info=function()end}''')
a=s.index('K.corpseCloth =');b=s.index('K.corpseGrasp =',a);R.execute(s[a:b])
cloth=R.eval('function(r)surface=r;return K.corpseCloth(1,{qi=0,qj=0,qk=0,qr=1,px=.2,py=0,pz=0},{proxy=42,index=0},8,0,0,0,0,0,1,0,0,0)end')
assert abs(cloth(.045)-.0352)<1e-8, 'collision capsule inflated the rendered forearm profile'
assert abs(cloth(.030)-.030)<1e-8, 'narrower measured surface was ignored'
assert cloth(.12) is None, 'foreign body surface accepted'
L.execute('assert(loadstring(...))',s)
print('PASS body-type forearm/shin profiles, collision-radius upper bound, narrow/foreign surface handling, Lua syntax')
