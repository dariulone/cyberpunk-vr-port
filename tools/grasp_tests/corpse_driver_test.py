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
nativeHandFrame=function()if noFrame then return nil end;return {{x=drawnX or 0,y=0,z=0},{x=0,y=0,z=0,w=1}}end
carryRelease=function(side) released[side]=true end''')
start=s.index('K.corpseContactMicrometres =');end=s.index('K.CORPSE_DRIVE =',start);L.execute(s[start:end])
f=L.execute('''return function(kind,first,second,n,bodyX,lost,near)
 ticks=0;queried=nil;released={};otherX=near and bodyX or 10
 local rec={kind=kind,proxy=42,index=7,pictured=true};local pos={x=bodyX,y=0,z=0};local rot={i=0,j=0,k=0,r=1};local dt=.02
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
L.globals().drawnX=1
f(12,True,False,1,1,3,False)
assert not L.globals().released[0] and L.globals().HOLD[0].lost==0, 'arm reach restriction dropped a body seated at the drawn hand'
L.globals().drawnX=0
L.globals().noFrame=True
f(12,True,False,1,1,2.99,False)
assert not L.globals().released[0] and L.globals().HOLD[0].lost==2.99, 'missing drawn pose advanced lost-grip timer'
L.globals().noFrame=False
f(12,True,False,1,1,2.99,False)
assert L.globals().released[0], 'valid drawn pose did not release a detached body'
print('PASS native one/two-hand ownership, legacy fallback, query visibility, timed lost-grip release')
print('PASS drawn-wrist lost-grip reference, out-of-reach controller, missing-pose grace, real detachment')
L.execute('native={1,42,18,16,0};VRPhysHeldBody=function()return native end')
f(12,True,False,1,1,3,False)
assert not L.globals().released[0] and L.globals().HOLD[0].lost==0, 'reference body7 articulation released a valid actual body18 anchor'
L.execute('native={1,42,18,16,600000}')
f(12,True,False,1,0,3,False)
assert L.globals().released[0], 'actual native contact loss was hidden by a stationary reference body'
for state in ['{1,42,18,200,600000}','{1,43,18,16,600000}','{0,42,18,16,600000}','{1,42,18,16,math.huge}']:
 L.execute('native='+state);f(12,True,False,1,1,2.99,False)
 assert not L.globals().released[0] and L.globals().HOLD[0].lost==2.99, 'invalid new contact data fell back to the wrong reference body'
L.execute('VRPhysHeldBody=function()error("no snapshot")end');f(12,True,False,1,1,2.99,False)
assert not L.globals().released[0], 'failed contact query fell back to reference coordinates'
L.execute('VRPhysHeldBody=function()return {1,42,18,16}end');f(12,True,False,1,1,3,False)
assert L.globals().released[0], 'old four-field DLL fallback changed silently'
L.execute('VRPhysHeldBody=nil')
print('PASS exact actual-anchor lost timer, real contact loss, invalid/stale/foreign skip and explicit old-DLL fallback')
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

# A second hand on another limb must remain an excluded anchor in floor-tow mode.
T=LuaRuntime(unpack_returned_tuples=True)
T.execute('K={};HOLD={};gen=0;VRCorpseHold=function(proxy,body,other,z,dt) lastOther=other;return 95 end')
start=s.index('K.CORPSE_DRIVE =');end=s.index('-- A RAGDOLL GOES BACK TO REST',start)
T.execute(s[start:end])
tick=T.eval('''function(limb,otherBody,sameProxy)
 local rec={proxy=42,index=8,limbHang=limb}
 gen=gen+1
 HOLD={[0]={rec=rec},[1]=otherBody and {rec={proxy=sameProxy and 42 or 43,index=otherBody}} or nil}
 K.corpseTick(rec,{z=1},.016);return lastOther,rec.whole end''')
assert tick(True,None,True)==(-2,95)
assert tick(True,19,True)==(-22,95)
assert tick(True,0,True)==(-3,95)
assert tick(False,19,True)==(19,95)
assert tick(True,19,False)==(-2,95)
assert tick(False,None,True)==(-1,95)
print('PASS one/two-hand corpse lift/tow modes, including body zero and unrelated corpse')

# The script reference (7) and actual held actor (18) differ in the live fixture.
# Exclude the other hand's actual actor, skip pending/stale/foreign snapshots,
# and avoid driving one native body twice through two distinct script references.
N=LuaRuntime(unpack_returned_tuples=True)
N.execute('''K={};HOLD={};gen=0;calls={};states={}
VRCorpseHold=function(proxy,body,other,z,dt) calls[#calls+1]={proxy=proxy,body=body,other=other};return 95 end
VRPhysHeldBody=function(side) local n=states[side];if n=='error' then error('native unavailable') end;return n end
function setup(a,b,fast)
 gen=gen+1;calls={};states={[0]=a,[1]=b}
 first={proxy=42,index=7,limbHang=true};second={proxy=42,index=8,limbHang=true}
 HOLD={[0]={rec=first,fast=fast and {} or nil},[1]=b and {rec=second,fast=fast and {} or nil} or nil}
end''')
N.execute(s[start:end])
N.execute('''setup({1,42,18,16},nil,true);K.corpseTick(first,{z=999},.016)
assert(#calls==1 and calls[1].body==18 and calls[1].other==-2 and first.index==7)
setup({1,42,18,16},{1,42,19,16},true)
K.corpseTick(first,{z=999},.016);K.corpseTick(second,{z=999},.016)
assert(#calls==2 and calls[1].body==18 and calls[1].other==-22 and calls[2].body==19 and calls[2].other==-21)
setup({1,42,18,16},{1,42,18,16},true)
K.corpseTick(first,{z=999},.016);K.corpseTick(second,{z=999},.016)
assert(#calls==1 and first.whole==95 and second.whole==95,'same native actor driven twice or mass not shared')
gen=gen+1;K.corpseTick(first,{z=999},.016);assert(#calls==2,'dedup survived into another update')
for _,bad in ipairs({{0,42,18,16},{1,42,18,200},{1,43,18,16},{1,42,-1,16},{},'error'}) do
 setup(bad,nil,true);K.corpseTick(first,{z=999},.016);assert(#calls==0,'invalid native hold fell back to reference actor')
 setup({1,42,18,16},bad,true);K.corpseTick(first,{z=999},.016);assert(#calls==0,'pending second hold could be towed')
end
setup({0,42,18,16},nil,false);K.corpseTick(first,{z=999},.016);assert(#calls==1 and calls[1].body==7,'legacy velocity path lost')
setup({1,42,18,16},nil,true);VRPhysHeldBody=nil;K.corpseTick(first,{z=999},.016);assert(calls[1].body==7,'old DLL fallback lost')
gen=gen+1;calls={};HOLD={};K.corpseTick(first,{z=999},.016);assert(#calls==0,'released body still towed')''')
print('PASS actual native anchors/exclusion, pending/stale/foreign safety, same-body dedup and legacy fallback')

# Expired activation requests must not accumulate across a long session.
T.execute('K={DEAD_ASKED={old=1,boundary=6,recent=9},DEAD_AGAIN=4};clockNow=10;os.clock=function()return clockNow end')
start=s.index('K.ragdollTick =');end=s.index('    if type(VRCorpseSpeed)',start)
T.execute(s[start:end]+'end')
T.execute('K.ragdollTick(.016);assert(K.DEAD_ASKED.old==nil and K.DEAD_ASKED.boundary==6 and K.DEAD_ASKED.recent==9)')
T.execute('clockNow=14;K.ragdollTick(.016);assert(next(K.DEAD_ASKED)==nil)')
print('PASS expired corpse cooldown eviction without shortening active cooldowns')
