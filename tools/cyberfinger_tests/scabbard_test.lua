local dir = assert(arg[1])
package.path = dir .. "?.lua;" .. package.path
local scabbard = require("modules/scabbard")
local shells = require("modules/shells")
local function copy(kind, name)
  return {typeName=kind, key="katana-item", cfKatana=kind=="Wea_Katana", cfScabbard=true, place={name=name, comp="slots", slot="hips",
          pos={1,2,3}, rot={0,0,0,1}}}
end
for _, hip in ipairs({"hip_l", "hip_r"}) do
  local c = copy("Wea_Katana", hip)
  local shell = assert(shells.wanted({c}, "w")["scabbard_katana-item"])
  assert(shell.slot == "hips" and shell.pose.p == c.place.pos)
  assert(shell.ent == "vrport\\holster\\shell\\cf_katana_scabbard.ent")
  c.cfScabbard = false
  assert(not next(shells.wanted({c}, "w")))
end
assert(not next(shells.wanted({copy("Wea_Katana", "shoulder_r")}, "m")))
assert(not next(shells.wanted({copy("Wea_Machete", "hip_l")}, "m")))

-- Inputs are sampled frames, including independent release order and movement away.
local function step(b, t, left, right, edge, distance, permitted, atHip)
  return scabbard.returnStep(b,t,permitted ~= false,left,right,edge,distance,atHip ~= false)
end
local function dock()
  local b={}
  assert(step(b,0,true,true,false,.4)=="hold" and not b.docked)
  assert(step(b,.1,true,true,false,.08)=="hold" and b.docked)
  return b
end
-- The sword hand alone commits once docked, even after an extended hold.
assert(step(dock(),2,true,false,false,.08)=="stow")
local supportFirst=dock()
assert(step(supportFirst,.2,false,true,false,.08)=="hold")
assert(step(supportFirst,.4,false,false,false,.08)=="stow")
assert(step(dock(),.2,false,false,false,.08)=="stow")
assert(step(dock(),.2,true,true,false,.21)=="cancel")
assert(step(dock(),.2,false,false,false,.08,false)=="cancel")
assert(step(dock(),.2,false,false,false,.08,true,false)=="cancel")
local b=dock()
assert(step(b,.2,false,true,false,.08)=="hold")
assert(step(b,.82,false,false,false,.08)=="cancel")
b=dock()
step(b,.2,false,true,false,.08)
assert(step(b,.24,true,true,false,.08)=="cancel")
assert(step(dock(),.2,true,false,false,.21)=="cancel")
assert(step(dock(),.2,true,false,false,.08,false)=="cancel")
b={}
step(b,0,true,true,false,.09)
assert(not b.docked and step(b,.2,false,false,false,.09)=="cancel")
b={}
assert(step(b,0,true,true,true,.09,false)=="hold" and not b.docked)
assert(step(b,.2,false,false,false,.09,false)=="cancel")
-- Bracing alone while drawing never commits a stow.
b={}
assert(step(b,0,true,false,false,.09,false)=="hold")
assert(step(b,.2,false,false,false,.09,false)=="cancel")
-- Exercise the actual Lua hand adapter with two tracked poses and a moving shell.
-- The port accepts LuaJIT integer suffixes in the unrelated appearance-seed calculation.
local file=assert(io.open(dir.."modules/holsters.lua"))
local source=file:read("*a"):gsub("(%x+)ULL","%1")
file:close()
local module=assert(load(source,"@holsters.lua"))()
local function up(fn,name)
  for i=1,100 do
    local k,v=debug.getupvalue(fn,i)
    if k==name then return v end
    if not k then break end
  end
  error("missing upvalue "..name)
end
local tick=up(up(module.update,"hand"),"scabbardTick")
local state=up(tick,"S")
CName={new=function(v)return v end}
Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w} end}
Quaternion={new=function(i,j,k,r)return {i=i,j=j,k=k,r=r} end}
WorldPosition={ToVector4=function(p)return p end}
local zero=Vector4.new(0,0,0,1)
local slots={GetSlotTransform=function()return true,{Position=zero,Orientation=Quaternion.new(0,0,0,1)} end}
local poses={[0]=Vector4.new(0,0,.075,1),[1]=Vector4.new(.5,0,0,1)}
local valid, tracked, stows, sound, reserved=true,true,0,nil,-1
function GetVRSharedSlot()return 0 end
function VRHandRawWorld(side)return poses[side] end
function GetLeftVRHandValid()return valid end
function GetRightVRHandValid()return valid end
function VRCyberFingerScabbardHand(side)reserved=side;return tracked end
local allowMelee=true
function VRAllowsGestureHolstering(melee)
  assert(melee==true,"katana return used the gun setting")
  return allowMelee
end
function GetMod()return nil end
local weapon={IsMelee=function()return true end,GetEntityID=function()return {hash=9} end}
local pl={FindComponentByName=function()return slots end,GetActiveWeapon=function()return weapon end,
  GetEntityID=weapon.GetEntityID,VRHolsterMelee=function()stows=stows+1 end}
Game={GetSystemRequestsHandler=function()return {IsGamePaused=function()return false end} end,
  GetAudioSystem=function()return {Play=function(_,event)sound=event end} end}
local function entity()
  local root={SetLocalTransform=function(self,p)self.p=p end}
  return {FindComponentByName=function()return root end},root
end
local c=copy("Wea_Katana","hip_l")
c.place.pos={0,0,0}
c.place.pull=.22
c.place.pullAxis={0,0,-1}
c.entity=entity()
c.attached=true
c.handSide=1
local sh={attached=true}
local shellRoot
sh.entity,shellRoot=entity()
state.copies[c.key]=c
state.shells["scabbard_"..c.key]=sh
local function frame(left,right,edgeL,edgeR)
  state.t=state.t+.04
  tick(pl,{[c.key]=weapon},{[0]=left,[1]=right},{[0]=edgeL,[1]=edgeR},1)
end
frame(true,true,true,false)
assert(state.brace and reserved==0)
poses[0].x=.09
frame(true,true,false,false)
assert(math.abs(shellRoot.p.x-.09)<1e-6)
poses[1].x=.09
frame(true,true,false,false)
assert(state.brace.docked)
tracked=false
frame(false,false,false,false)
assert(not state.brace and reserved==-1 and stows==0 and shellRoot.p.x==0)
tracked=true
poses[0].x=0
-- Closing the support hand on the hilt beside the hip cannot arm a return.
frame(true,true,true,false)
assert(not state.brace)
frame(true,false,false,false)
assert(stows==0)
-- Brace first, bring the held sword back, then open its hand to stow.
poses[1].x=.5
frame(true,true,true,false)
assert(state.brace and not state.brace.docked)
poses[1].x=.09
frame(true,true,false,false)
assert(state.brace.docked)
frame(true,false,false,false)
assert(not state.brace and stows==1 and c.mode=="stow" and sound=="w_melee_katana_unequip")
frame(true,false,false,false)
frame(false,false,false,false)
assert(stows==1, "held scabbard hand or later release repeated the stow")
-- Either weapon can be selected from the wheel without changing the numbered slots.
local assign=up(up(module.update,"scan"),"assign")
ItemID={GetTDBID=function(id)return id end,GetCombinedHash=function(id)return id end}
TweakDBInterface={GetItemRecord=function()return {ItemType=function()return {Type=function()return {value="Wea_Katana"} end} end} end}
local cf,override=true,true
function VRCyberFingerEnabled()return cf end
function VRCyberFingerRightHandSwords()return cf and override end
local items={{slot=0,id=11},{slot=2,id=12}}
local wheelId=12
weapon.GetItemID=function()return wheelId end
Game.GetPlayer=function()return pl end
-- One-handed draw, late game attachment, a redraw during stow, and a refused draw.
do
  local settle=up(module.update,"settle")
  local reveal=up(module.update,"revealReal")
  local savedCopies=state.copies
  local sword=copy("Wea_Katana","hip_l")
  sword.cfKatana,sword.attached,sword.entity,sword.side=true,true,entity(),1
  sword.mode,sword.shown,sword.until_="hand",true,state.t+1
  state.copies={[sword.key]=sword}
  local function mesh(id,on)
    return {id=id,on=on,IsA=function()return true end,IsEnabled=function(m)return m.on end,
      Toggle=function(m,v)m.on=v end}
  end
  local blade,unused=mesh(1,true),mesh(2,false)
  local item={GetEntityID=function()return {hash=91} end,GetComponents=function()return {blade,unused} end}
  Game.VREntityComponents=function(obj)return obj:GetComponents() end
  Game.VRComponentKey=function(m)return m.id end
  Game.FindEntityByID=function(id)if id.hash==91 then return item end end
  VRDetachFromSlot=function()end
  VRAttachToSlot=function()return true end
  local equipped,attached={[sword.key]=true},{[sword.key]=item}
  settle(equipped,{})
  assert(sword.mode=="hand" and sword.shown,"the draw lost its preview before the blade arrived")
  settle(equipped,attached)
  assert(sword.mode=="place" and not sword.shown and blade.on,"one-handed draw lost the equipped blade")
  sword.mode,sword.stowAt="homing",state.t
  settle(equipped,attached)
  assert(not sword.shown and blade.on and not next(state.hiddenReal),"a redraw hid the attached blade")
  settle({},{}); settle({},{})
  assert(sword.shown,"the detached blade failed to return to the belt")
  sword.mode,sword.until_="hand",state.t-.1
  settle(equipped,{}); settle(equipped,{})
  assert(sword.mode=="place" and sword.shown,"refused draw lost the blade while it was still selected")
  -- A detached entity may be reused on redraw. Only re-enable components we hid.
  local hide=up(settle,"hideReal")
  hide(sword.key,item)
  assert(not blade.on)
  reveal({})
  assert(blade.on and not unused.on and not next(state.hiddenReal),"lost visibility while the hand slot was empty")
  state.copies=savedCopies
end
gamedataEquipmentArea={Weapon="Weapon"}
ItemID.IsValid=function(id)return id~=nil end
local pd={GetItemInEquipSlot=function(_,area,slot)
  for _,entry in ipairs(items) do if entry.slot==slot then return entry.id end end
end}
local equipment={GetPlayerData=function()return pd end}
Game.GetScriptableSystemsContainer=function()return {Get=function()return equipment end} end
local function checkPair(left,primarySide)
  VRLeftHanded=function()return left end
  local list=assign(items,left,"w",false)
  assert(#list==2)
  assert(list[1].id==items[1].id and list[1].side==primarySide)
  assert(list[1].place.name==(primarySide==1 and "hip_l" or "hip_r"))
  assert(list[2].id==items[2].id and list[2].place.name~=list[1].place.name)
  assert(list[2].side==1-primarySide and list[2].primary==items[1].id)
  assert(module.slotInfo(items[1].slot).place==list[1].place.name)
  assert(module.slotInfo(items[2].slot).place==list[2].place.name)
end
checkPair(false,1); checkPair(true,1)
local placement = require("modules/placements")
local families = {"Wea_Revolver","Wea_SubmachineGun","Wea_ShotgunDual","Wea_ShortBlade","Gad_Grenade","Wea_Katana","Wea_SniperRifle","Wea_Hammer"}
for i, family in ipairs(families) do
  for j, other in ipairs(families) do
    assert(placement.holsterEnabled(other, 2^(i-1)) == (i == j), family .. " switch also affected " .. other)
  end
end
VRCyberFingerHolsterTypes=function() return 255-32 end
assert(#assign(items,true,"w",false)==0 and not module.slotInfo(0), "disabled swords still have scabbards")
local originalGetMod, oldCopies, oldShells = GetMod, state.copies, state.shells
state.copies, state.shells = {}, {}
local offhandStows = 0
GetMod=function(name)
  if name=="CyberpunkVRPort_Weapon" then return {
    offhandBlade=function() return {GetItemID=function() return 12 end} end,
    stowOffhand=function() offhandStows=offhandStows+1 end
  } end
end
wheelId=11
local scan=up(module.update,"scan")
scan(pl,pd,true)
assert(offhandStows==0 and not next(state.copies) and not next(state.shells),"hiding scabbards unequipped a blade")
wheelId=99; scan(pl,pd,true)
assert(offhandStows==1,"removing the main katana kept an invalid offhand blade")
GetMod, state.copies, state.shells=originalGetMod, oldCopies, oldShells
VRCyberFingerHolsterTypes=nil
wheelId=11; checkPair(true,1)
items[1].id,items[2].id=12,11; checkPair(false,1)
override=false;checkPair(true,0)
cf=false
for _,a in ipairs(assign(items,false,"w",false)) do assert(not a.primary) end

-- Use the solved wrist actually published by VRPalmModelPos. The centre of the
-- closed hand must land on the wrap even when a different pose drives the raw wrist.
local hand=up(module.update,"hand")
local grab=up(hand,"grab")
local palms={}
local wristRot
local solved={}
function VRPalmModelPos(side)return solved[side] end
function VRPalmModelRot(side)return wristRot end
function VRPalmWorldPos(side)return palms[side] end
-- Left wrist at identity and right wrist rotated about X by 180 degrees.
local function placeFist(side,x,y,z)
  local dx=side==0 and .0647 or -.0647
  solved[side]=Vector4.new(x-dx,y+.032,z+.0294,1)
end
function VRWorldToModel(p)return p end
function VRWorldDirToModel(p)return p end
function VRHandRawModel(side)return poses[side] end
function VRHandRawRot(side)return wristRot end
for side=0,1 do
  local sword=copy("Wea_Katana",side==1 and "hip_l" or "hip_r")
  local hipX=side==1 and -.2 or .2
  sword.key,sword.item,sword.handSide="draw",side==1 and 11 or 12,side
  sword.primary=side==0 and 11 or nil
  sword.place.pos,sword.place.pull,sword.place.pullLocal={hipX,0,0},.22,{0,0,-1}
  sword.place.knucklesUp,sword.place.melee=true,true
  sword.entity,sword.attached=entity(),true
  state.copies={draw=sword};state.cooldown=0;state.pull=nil
  local correct=side==0 and Quaternion.new(0,0,0,1) or Quaternion.new(1,0,0,0)
  local wrong=side==0 and Quaternion.new(1,0,0,0) or Quaternion.new(0,0,0,1)
  wristRot=correct
  poses[side]=Vector4.new(hipX+.15,0,-.02,1)
  local function reach()
    local wrist=solved[side]
    return module.near(wrist.x,wrist.y,wrist.z,side)
  end
  solved[side]=Vector4.new(hipX,0,-.065,1)
  assert(not reach(),"wrist alone could still grab the handle")
  grab(pl,{},{},side,1);assert(not state.pull)
  for _,z in ipairs({.012,-.18}) do
    placeFist(side,hipX,0,z)
    assert(not reach())
    grab(pl,{},{},side,1);assert(not state.pull,"guard or lower hilt started a draw")
  end
  placeFist(side,hipX+.015,0,-.065)
  assert(reach(),"belt arbitration ignored the closed hand on the wrap")
  wristRot=wrong
  grab(pl,{}, {},side,1);assert(not state.pull,"wrong wrist orientation started a draw")
  wristRot=correct
  grab(pl,{}, {},1-side,1);assert(not state.pull,"wrong hand drew a blade")
  grab(pl,{}, {},side,1);assert(state.pull and state.pull.side==side)
  local wrist=state.held.p
  assert(math.abs(wrist[1]+(side==0 and .0647 or -.0647)-hipX)<1e-6
     and math.abs(wrist[2]-.032)<1e-6 and math.abs(wrist[3]-.0294+.065)<1e-6,
     "gesture draw pinned the wrist instead of the visible fist to the hilt")
end
state.pull=nil
-- The physical Grip works from empty hands and fists. A press is tied to one item
-- and one operation. Stowing uses only unequip, even with another weapon in the wheel.
do
  local buttons=up(module.update,"katanaButtons")
  local beforeCopies, beforeCF=state.copies,cf
  local beforeSystems=Game.GetScriptableSystemsContainer
  local drawn,stowed,offers=nil,0,{}
  local tap
  cf=true
  gamedataItemType={Wea_Katana="katana"}
  WeaponObject={IsOfType=function(id)return id==11 or id==12 end}
  DrawItemRequest={new=function()return {} end}
  TDBID={ToStringDEBUG=tostring}
  Game.GetScriptableSystemsContainer=function()return {Get=function()return {
    QueueRequest=function(_,r)assert(not drawn);drawn=r.itemID end} end} end
  function VRCyberFingerKatanaGrip(side,eq,support)
    offers[side]={eq,support};return tap==side and eq>0
  end
  for side=0,1 do
    local other=1-side
    local c=copy("Wea_Katana",side==0 and "hip_r" or "hip_l")
    c.item,c.handSide,c.mode,c.shown=11,side,"place",true
    c.place.pos={0,0,0};c.entity=entity();c.attached=true
    state.copies={[c.key]=c}
    local function actor(n)return {GetItemID=function()return n end} end
    for _,hands in ipairs({{}, {[side]=actor(99)}}) do
      tap=side;drawn=nil
      wristRot=side==0 and Quaternion.new(0,0,0,1) or Quaternion.new(1,0,0,0)
      solved[side]=Vector4.new(0,0,-.065,1)
      buttons(pl,hands,nil,{},true);assert(not drawn and offers[side][1]==0,"wrist alone offered Grip draw")
      placeFist(side,0,0,.012)
      buttons(pl,hands,nil,{},true);assert(not drawn and offers[side][1]==0,"cross-guard offered Grip draw")
      placeFist(side,0,0,-.065)
      solved[side].w=0
      buttons(pl,hands,nil,{},true);assert(not drawn and offers[side][1]==0,"invalid pose offered Grip draw")
      solved[side].w=1
      buttons(pl,hands,nil,{},false);assert(not drawn)
      buttons(pl,hands,nil,{},true);assert(drawn==11,"fist on hilt missed a Grip draw")
    end
    local main=actor(11)
    local old=pl.VRHolsterMelee
    pl.VRHolsterMelee=function()stowed=stowed+1 end
    tap=nil;drawn=nil
    buttons(pl,{[side]=main},nil,{[c.key]=main},false)
    assert(offers[side][1]>0 and offers[other][2]>0 and offers[other][1]==0)
    tap=side;local before=stowed
    buttons(pl,{[side]=main},nil,{[c.key]=main},false)
    assert(stowed==before+1 and not drawn,"stowing also requested another weapon")
    pl.VRHolsterMelee=old
  end
  Game.GetScriptableSystemsContainer=beforeSystems
  state.copies,cf=beforeCopies,beforeCF
  VRCyberFingerKatanaGrip=nil
end
-- Returning the off-hand sword requires both hands at its scabbard.
local secondary=copy("Wea_Katana","hip_r")
secondary.key,secondary.primary,secondary.item="second",11,12
secondary.handSide,secondary.place.pos,secondary.attached=0,{0,0,0},true
secondary.entity=entity()
state.copies={second=secondary};state.cooldown=0
state.shells.scabbard_second={attached=true}
local released,closed,paused=0,1,false
function GetMod()return {offhandBlade=function()return weapon end,stowOffhand=function(_,id)
  assert(id==12);released=released+1;return true
end} end
function VRCyberFingerMeleeHand(side,blade)assert(side==0 and blade);return closed end
Game.GetSystemRequestsHandler=function()return {IsGamePaused=function()return paused end} end
local function sample(grip,x,supportX,supportValid,dt)
  closed=grip
  palms[0]=Vector4.new(x,0,0,1)
  palms[1]=Vector4.new(supportX or 0,0,.075,supportValid==false and 0 or 1)
  hand(pl,{second=true},{second=weapon,["11"]=weapon},dt or .02,true)
end
sample(1,.05,.35); sample(0,.05,.35); assert(released==0,"single hand put the second blade away")
sample(1,.05); sample(0,.05,.35); assert(released==0,"moving the brace hand away must cancel")
sample(1,.05); sample(0,.05,0,false); sample(0,.05); assert(released==0,"lost brace pose committed a return")
sample(1,.05); sample(0,.05,0,true,.5); assert(released==0,"stale dock survived a frame gap")
sample(1,.05); sample(0,.35); assert(released==0)
sample(1,.05); sample(-1,.05); sample(0,.05); assert(released==0)
sample(1,.05); paused=true; sample(0,.05); paused=false; sample(0,.05); assert(released==0)
allowMelee=false
sample(1,.05); sample(0,.05); assert(released==0,"disabled melee holstering returned a blade")
allowMelee=true
sample(1,.05); sample(0,.05); sample(0,.05); assert(released==1)
-- Existing placements and cosmetic visibility survive hiding gear and reopening CET.
local cf, offensive = true, nil
local saved = {slots={["1"]="hip_l"}, hidden=true}
local drawn, grenade, holsters, stowedGrenades = false, false, 0, 0
local notices = {}
local actor = {VRHasDrawnCombatItem=function() return drawn end,
  VRHolsterAllForOffensiveMode=function() holsters=holsters+1 end}
local belt = {grenade={held=function() return grenade end, reservedSlots=function() return false,false,false end, stow=function()
  if grenade then stowedGrenades=stowedGrenades+1 end
  grenade=false
end}}
local env = setmetatable({
  json={decode=function() return saved end, encode=function(data) saved=data; return "{}" end},
  io={open=function(name)
    assert(name=="holster_places.json")
    return {read=function() return "{}" end, write=function() end, close=function() end}
  end},
  Game={GetPlayer=function() return actor end, GetSystemRequestsHandler=Game.GetSystemRequestsHandler},
  GetMod=function(name) if name=="CyberpunkVRPort_TacticalBelt" then return belt end end,
  VRCyberFingerEnabled=function() return cf end,
  VRCyberFingerSetOffensiveMode=function(value) offensive=value end,
  VRCyberFingerShowModeNotice=function(id) notices[#notices+1]=({"Offensive mode OFF","Offensive mode ON","Putting weapons away..."})[id] end,
  GetVRSharedSlot=function() return 1 end,
}, {__index=_G})
local function reopen() return assert(load(source,"@holsters.lua","t",env))() end
local gear=reopen()
gear.syncOffensiveMode()
assert(offensive==true and not gear.wornGearHidden() and gear.hidden() and #notices==0)
assert(gear.toggleWornGear() and saved.wornGearHidden and saved.slots["1"]=="hip_l" and saved.hidden)
assert(offensive==false and notices[1]=="Offensive mode OFF")
offensive=nil; gear=reopen(); gear.syncOffensiveMode()
assert(offensive==false and gear.wornGearHidden() and gear.near(0,0,0,0)==nil)
local s=up(gear.update,"S")
s.copies.fake={}; s.shells.fake={}
gear.update(.016,true); gear.update(.016,true)
assert(not next(s.copies) and not next(s.shells) and s.gripPrev[0] and s.gripPrev[1])
cf=false
assert(not gear.wornGearHidden() and not gear.toggleWornGear() and saved.wornGearHidden)
cf=true
assert(gear.toggleWornGear() and offensive==true and not reopen().wornGearHidden() and saved.hidden)
drawn=true; gear.syncOffensiveMode(.1)
drawn=false; gear.syncOffensiveMode(.5)
assert(not gear.wornGearHidden() and not saved.wornGearHidden,"ordinary stow overrode saved On")
drawn=true
assert(gear.toggleWornGear() and holsters==1 and saved.wornGearHidden)
gear.syncOffensiveMode(1)
assert(not gear.wornGearHidden() and offensive,"engine still has a weapon attached")
drawn=false; gear.syncOffensiveMode(.21)
assert(gear.wornGearHidden() and not offensive)
drawn=true; gear.syncOffensiveMode(.01)
assert(not gear.wornGearHidden() and offensive and saved.wornGearHidden,"temporary draw changed saved Off")
drawn=false; gear.syncOffensiveMode(.05)
assert(not gear.wornGearHidden() and not offensive,"weapon swap flashed the belt or enabled empty Trigger draw")
grenade=true; gear.syncOffensiveMode(.05)
assert(not gear.wornGearHidden() and offensive,"grenade was ignored")
assert(gear.toggleWornGear() and stowedGrenades==1 and saved.wornGearHidden)
gear.syncOffensiveMode(.21)
assert(gear.wornGearHidden() and not offensive)
grenade=true; gear.syncOffensiveMode(.01)
grenade=false; gear.syncOffensiveMode(.21)
assert(gear.wornGearHidden() and saved.wornGearHidden,"releasing the last grenade must restore Off")
assert(notices[2]=="Offensive mode ON" and notices[3]=="Putting weapons away...")
assert(notices[4]=="Offensive mode OFF" and notices[5]=="Offensive mode ON")
assert(notices[6]=="Putting weapons away..." and notices[7]=="Offensive mode OFF")
assert(notices[8]=="Offensive mode ON" and notices[9]=="Offensive mode OFF" and #notices==9)
print("CyberFinger katana scabbard and worn gear checks passed")
