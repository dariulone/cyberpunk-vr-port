"""A nearby physics prop wins a new grip over a distant loot hover.

Exercise the real Reach frame callback and the hand-collision ownership policy.
Actual PhysX selection and CET loading remain separate live checks.
"""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
root=Path(__file__).resolve().parents[2]
world=(root/'mods/cet/CyberpunkVRPort_Reach/modules/world.lua').read_text(encoding='utf-8')
collision=(root/'mods/cet/CyberpunkVRPort_HandCollision/init.lua').read_text(encoding='utf-8')

def fixture(near,physical=True,held=False,available=True):
    lua=LuaRuntime(unpack_returned_tuples=True)
    lua.execute('''
grip=0;nativeHeld=false;physical=true;available=true;probeCalls=0;useCalls=0;targetZ=1
Vector4={new=function(x,y,z,w)return{x=x,y=y,z=z,w=w}end}
Quaternion={new=function(i,j,k,r)return{i=i,j=j,k=k,r=r}end}
TweakDBID={new=function(v)return v end};function FromVariant(v)return v end
function IsDefined(e)return e~=nil end
function VRPalmModelPos(side)return Vector4.new(0,0,0,side==1 and 1 or 0)end
function VRPalmModelRot()return Quaternion.new(0,0,0,1)end
function VRMenuPointer()local p={};for i=1,8 do p[i]=Vector4.new(0,0,i==8 and 1 or 0,i==8 and grip or 0)end;return p end
function VRHandHolds()return nativeHeld end
function VRReachAim(p)lastAim=p end
function VRReachUse()useCalls=useCalls+1;return true end
function GetMod()if available then return {canGrabNear=function(side)assert(side==1);probeCalls=probeCalls+1;return physical end}end end
function GetAllBlackboardDefs()return{PlayerStateMachine={Vision=1,Carrying=2,Takedown=3},UIInteractions={LootData=4,InteractionChoiceHub=5}}end
entity={GetWorldPosition=function()return Vector4.new(0,0,targetZ,1)end,GetEntityID=function()return{hash=42}end,IsAttached=function()return true end,IsA=function()return false end,QueueEvent=function()end}
player={GetWorldPosition=function()return Vector4.new(0,0,0,1)end,GetWorldOrientation=function()return Quaternion.new(0,0,0,1)end,
 GetPlayerStateMachineBlackboard=function()return{GetInt=function()return 0 end,GetBool=function()return false end}end}
Game={GetPlayer=function()return player end,GetMountedVehicle=function()return nil end,
 GetSystemRequestsHandler=function()return{IsGamePaused=function()return false end}end,
 GetTransactionSystem=function()return{GetItemInSlot=function()return nil end}end,
 GetTargetingSystem=function()return{GetLookAtObject=function()return entity end}end,
 GetBlackboardSystem=function()return{Get=function()return{GetVariant=function(_,key)if key==4 then return{isActive=true,ownerId={hash=42}}end end}end}end}
function NewObject()return{}end
''')
    g=lua.globals();g.physical=physical;g.nativeHeld=held;g.available=available;g.targetZ=.1 if near else 1
    g.W=lua.execute(world)
    lua.execute('''local s=W.debug();local t={id="42",entity=entity,far=true,item=true};s.things={t};s.byId={[t.id]=t};s.listAt=10000;W.update(.016,nil)''')
    g.grip=1;g.W.update(.016,None)
    return lua

# Reach runs first: it yields this edge before locking the distant target.
lua=fixture(False)
assert not lua.globals().W.state()['target']
assert lua.globals().probeCalls==1 and lua.globals().useCalls==0
lua.globals().W.update(.016,None)
assert lua.globals().probeCalls==1,'probe repeated while grip stayed closed'
# Physics already ran: the native ownership signal excludes the hand entirely.
lua=fixture(False,held=True)
assert not lua.globals().W.state()['target'] and lua.globals().probeCalls==0
# The existing near interaction and distant pull remain available when appropriate.
lua=fixture(False,physical=False)
assert lua.globals().W.state()['target']['locked']
lua=fixture(True)
assert lua.globals().useCalls==1 and lua.globals().probeCalls==0
lua=fixture(False,available=False)
assert lua.globals().W.state()['target']['locked']

lua=LuaRuntime(unpack_returned_tuples=True)
lua.execute('K={};reach={};K.reachMod=function()return{world=function()return reach end}end')
start=collision.index('K.reachBusy = function(');end=collision.index('\nend\n',start)+5
lua.execute(collision[start:end])
lua.execute('''
reach={target={hand=2,confirmed=true,locked=false,how='aim'}}
assert(not K.reachBusy(1,true),'a distant hover blocked the physical grip')
assert(K.reachBusy(1),'ranged physics pull competed with loot hover')
reach.target.how='near';assert(K.reachBusy(1,true),'direct loot interaction lost ownership')
reach.target.how='aim';reach.target.locked=true;assert(K.reachBusy(1,true),'locked pull lost ownership')
reach={flight={hand=2}};assert(K.reachBusy(1,true));assert(not K.reachBusy(0,true))
reach={eating=2};assert(K.reachBusy(1,true));assert(not K.reachBusy(0,true))
''')
print('grip priority: both callback orders, nearby use, distant pull and active leases PASS')
