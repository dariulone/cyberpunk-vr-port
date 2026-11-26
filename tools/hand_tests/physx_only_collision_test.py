"""Exercise the full Lua update with overlapping hand/body bone samples.

The script must not resolve collisions or overwrite a reload-owned wrist. Real
PhysX contacts and CET loading are verified separately in the game.
"""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

ROOT=Path(__file__).resolve().parents[2]
MOD=ROOT/'mods/cet/CyberpunkVRPort_HandCollision'
lua=LuaRuntime(unpack_returned_tuples=True)
cache={}
def require(name):
    if name not in cache:cache[name]=lua.execute((MOD/(name+'.lua')).read_text(encoding='utf-8'))
    return cache[name]
lua.globals().require=require
lua.execute('''
events={};stops={};toggles={};logs={};mask=3;owner=-1
function registerForEvent(n,f) events[n]=f end
spdlog={info=function(s) logs[#logs+1]=s end}
Vector4={new=function(x,y,z,w) return {x=x,y=y,z=z,w=w} end}
Quaternion={new=function(i,j,k,r) return {i=i,j=j,k=k,r=r} end}
CName={new=function(x)return x end};TweakDBID={new=function(x)return x end}
function NameToString(x)return x end
function GetMod()return nil end
function GetVRSharedSlot(i) if i==162 then return owner end return 0 end
function VRHandCollision()return mask end
function VRHandRawModel()return Vector4.new(.1,.2,1,1) end
function VRHandRawRot()return Quaternion.new(0,0,0,1) end
function VRBoneModelPos()return Vector4.new(.1,.2,1,1) end
function VRBoneModelRot()return Quaternion.new(0,0,0,1) end
function VRHandStopModel(side,on,p)stops[#stops+1]={side=side,on=on,p=p} end
function VRHandStopRot()end
function SetVRDiagCapture()return true end
function VRPhysHandsPeople()end
function VRPhysHandsPerson()return true end
function VRNpcGrip()return 0 end
function VRPhysHandsGrab()return Vector4.new(0,0,0,0) end
player={GetActiveWeapon=function()return nil end,GetMountedVehicle=function()return nil end,
 GetWorldPosition=function()return Vector4.new(0,0,0,1) end,
 GetWorldOrientation=function()return Quaternion.new(0,0,0,1) end,
 GetEntityID=function()return {hash=123} end,FindComponentByName=function()return nil end}
components={}
for _,name in ipairs({'VRPortHand_l','VRPortHand_r'}) do
 components[#components+1]={GetName=function()return name end,
  Toggle=function(_,on)toggles[#toggles+1]={name=name,on=on} end}
end
Game={GetPlayer=function()return player end,VREntityComponents=function()return components end,
 GetTransactionSystem=function()return {GetItemInSlot=function()return nil end} end}
''')
lua.execute((MOD/'init.lua').read_text(encoding='utf-8'))
lua.execute('''
assert(events.onUpdate)
-- PhysX has not created bodies yet; overlapping samples must still stay raw.
for _,m in ipairs({3,67,0}) do
 mask=m
 for _,o in ipairs({-1,0,1}) do
  owner=o;stops={}
  events.onUpdate(1/90)
  local cleared={}
  for _,v in ipairs(stops) do
   assert(not v.on,'Lua collision displaced a wrist')
   assert(v.side~=owner,'reload wrist was overwritten')
   cleared[v.side]=true
  end
  for side=0,1 do if side~=owner then assert(cleared[side],'legacy stop not cleared') end end
 end
end
assert(#toggles==2,'legacy template hand capsules were not retired')
for _,v in ipairs(toggles) do assert(not v.on,'legacy push capsule enabled') end
''')
print('PhysX-only hand update: no Lua displacement, reload ownership, template cleanup PASS')
