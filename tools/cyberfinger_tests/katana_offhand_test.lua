-- Exercise the CET state machine with delayed game attachment and the actual stroke detector.
package.path=assert(arg[1]).."?.lua;"..package.path
local main={serial=101,record="Katana"}
local second={serial=102,record="Katana"}
local fists={serial=3,record="Fists"}
local gun={serial=4,record="Gun"}
local available={Weapon={main,second},BaseFists={}}
local left,grip,lease=false,1,nil
local frame,x,paused=0,0,false
local slots,pending,stock,failMount={},nil,nil,false
local flats={["Fists.placementSlots"]={"Base"},["Katana.placementSlots"]={"Base","OtherMod"}}
ItemID={GetCombinedHash=function(id)return id.serial end,GetTDBID=function(id)return id.record end}
TweakDBID={new=function(record,field)return record..(field or "") end}
TweakDB={
    GetFlat=function(_,id)return {table.unpack(assert(flats[id]))} end,
    SetFlat=function(_,id,values)flats[id]=values;return true end,
}
WeaponObject={IsOfType=function(id,kind)return id.record==kind end,IsFists=function(id)return id==fists end}
gamedataItemType={Wea_Katana="Katana"}
gamedataEquipmentArea={Weapon="Weapon",BaseFists="BaseFists"}
EquipmentSystem={GetItemsInArea=function(_,which)return available[which] end}
Vector4={new=function(x,y,z,w)return {x=x,y=y,z=z,w=w} end}
local function pos(n)return Vector4.new(n,0,0,1) end
local function actor(id)
    return {GetItemID=function()return id end,GetEntityID=function()return {hash=id.serial} end,
        GetWorldPosition=function()return pos(x) end,VRMeleeReach=function()return .8 end,
        VRMeleeDirection=function()return pos(1) end}
end
local player,primary={},actor(main)
local begins,hits={},{}
function player:GetEntityID()return {hash=55} end
function player:GetActiveWeapon()return primary end
function player:GetWorldPosition()return pos(0) end
function player:VROffhandItem(side)return slots[side] end
function player:VRClearOffhandItem(side)
    slots[side]=nil
    if pending and pending.side==side then pending=nil end
end
function player:VRMountOffhandItem(primaryID,id,side)
    assert(primaryID==primary:GetItemID() and id~=primaryID)
    local allowed=flats[id.record..".placementSlots"]
    assert(allowed[#allowed]=="AttachmentSlots.VRPortOffhand"..(side==0 and "Left" or "Right"))
    if not failMount then pending={id=id,side=side} end
    return not failMount
end
function player:VRCyberwareBeginSwing(w,lane,strong)
    begins[#begins+1]={id=w:GetItemID(),lane=lane,frame=frame,strong=strong}
    return true
end
function player:VRCyberwareUpdateSwing()end
function player:VRMeleeWhoosh()end
function player:VRMeleeSweepHit(w,start,tip,_,__,radius,passive)
    if not passive then hits[w:GetItemID()]=radius end
    if w:GetItemID()==fists then assert(start.x==tip.x) end
end
function VRCyberFingerEnabled()return true end
function VRCyberFingerMeleeHand(side,blade)
    lease=side>=0 and blade and side or nil
    return grip
end
function VRHandRawWorld()return pos(x) end
local busy=false
function GetMod()return {handBusy=function()return busy end} end
Game={VRWeaponInLeftHand=function()return left end,GetVRSharedSlot=function()return 0 end,
    GetTransactionSystem=function()return {GetItemInSlot=function()return stock end} end,
    GetSystemRequestsHandler=function()return {IsGamePaused=function()return paused end} end,
    VRMeleeInputFrame=function()return {{x=frame*20,y=frame,z=0},pos(x),pos(x),{},{} } end}
local offhand=require("katana_offhand")
local ordinary=require("ordinary_melee")
local function tick(count,advance)
    for _=1,count do
        frame=frame+1
        x=x+(advance or 0)
        if pending then slots[pending.side]=actor(pending.id);pending=nil end
        offhand.update(.02,player,primary,true)
        ordinary.update(.02,player,primary,pos(x),pos(1),pos(0),{ticks=0})
    end
end
local function swing()tick(52);tick(3,.09);tick(5) end
local function count(id)
    local n=0
    for _,b in ipairs(begins) do if b.id==id then n=n+1 end end
    return n
end
tick(2)
available.BaseFists={fists}
tick(22)
assert(slots[0]:GetItemID()==fists and not lease)
grip=0;swing()
assert(count(fists)==0 and count(main)>0,"an open hand must not punch")
grip=1;busy=true;swing()
assert(count(fists)==0,"bracing must not punch")
busy=false;swing()
assert(count(fists)==1 and hits[fists]==.09 and hits[main]==.22)
local a,b=begins[#begins-1],begins[#begins]
assert(a.frame==b.frame and a.id==fists and a.lane==0 and b.id==main and b.lane==1)
local originalFrame,originalTrigger=Game.VRMeleeInputFrame,Game.GetVRSharedSlot
grip=0
Game.GetVRSharedSlot=function()return 1 end
swing()
assert(begins[#begins].id==main and not begins[#begins].strong,"one-handed katana used Trigger strength")
Game.GetVRSharedSlot=function()return 0 end
Game.VRMeleeInputFrame=function()
    local sample=originalFrame();sample[1].z=1;return sample
end
swing()
assert(begins[#begins].id==main and begins[#begins].strong,"two-handed katana failed to use strong attacks")
Game.VRMeleeInputFrame,Game.GetVRSharedSlot=originalFrame,originalTrigger
grip=1
assert(not offhand.draw(player,main,main,0) and not offhand.draw(player,main,gun,0))
assert(not offhand.draw(player,main,second,1))
assert(offhand.draw(player,main,second,0))
tick(20)
assert(offhand.blade(player):GetItemID()==second and lease==0)
assert(flats["Fists.placementSlots"][2]==nil)
grip=0;swing()
assert(count(second)==1 and hits[second]==.22,"the exact equipped second sword should hit even with an open hand")
grip=-1;swing()
assert(count(second)==1,"tracking loss must suppress the secondary swing")
grip=1
table.insert(flats["Katana.placementSlots"],"AddedWhileDrawn")
assert(offhand.stow(player,second))
tick(16)
assert(not offhand.blade(player) and slots[0]:GetItemID()==fists and lease==nil)
assert(table.concat(flats["Katana.placementSlots"],",")=="Base,OtherMod,AddedWhileDrawn")
left=true;tick(20)
assert(not slots[0] and slots[1]:GetItemID()==fists)
assert(offhand.draw(player,main,second,1));tick(20)
assert(lease==1 and offhand.blade(player):GetItemID()==second)
available.Weapon={main};tick(22)
assert(slots[1]:GetItemID()==fists and not lease)
available.Weapon={main,second};failMount=true
assert(offhand.draw(player,main,second,1));tick(110);failMount=false;tick(15)
assert(slots[1]:GetItemID()==fists and not offhand.blade(player),"failed draw must release the hand")
assert(offhand.draw(player,main,second,1));tick(20)
stock=actor(gun);tick(2)
assert(not offhand.blade(player) and not lease)
stock=nil;primary=actor(gun);tick(20)
assert(not slots[0] and not slots[1])
assert(table.concat(flats["Fists.placementSlots"],",")=="Base")
assert(#available.Weapon==2 and available.Weapon[2]==second)
-- Run the stock carry and handedness handlers from the real weapon update.
local events={}
function registerForEvent(name,fn) events[name]=fn end
assert(loadfile(arg[1].."init.lua"))()
local function up(fn,name)
    for i=1,100 do
        local k,v=debug.getupvalue(fn,i)
        if k==name then return v end
        if not k then break end
    end
    error("missing upvalue "..name)
end
local update=up(up(events.onUpdate,"combatFrame"),"updateWeapon")
local rehand=up(up(update,"weaponFrame"),"handednessFrame")
do
    local guard=up(up(update,"weaponFrame"),"updateFistGuard")
    local hold,adds,removes=false,0,0
    gamedataItemType.Wea_Fists="Fists"
    gamedataStatType={IsBlocking="block",IsDeflecting="parry"}
    gameStatModifierType={Additive=1}
    RPGManager={CreateStatModifier=function(kind) assert(kind=="block");return 42 end}
    Game.GetStatsSystem=function()return {
        AddModifier=function(_,id,mod)assert(mod==42);adds=adds+1 end,
        RemoveModifier=function(_,id,mod)assert(mod==42);removes=removes+1 end} end
    VRCyberFingerFistBlock=function(equipped)return equipped and hold end
    local bare=actor(fists)
    do
        local combat=require("cyberware")
        local sweeps,swishes=0,0
        local savedSweep,savedWhoosh=player.VRMeleeSweepHit,player.VRMeleeWhoosh
        player.VRMeleeSweepHit=function() sweeps=sweeps+1 end
        player.VRMeleeWhoosh=function() swishes=swishes+1 end
        local initial=count(fists)
        local function move(n,delta,blocking)
            for _=1,n do
                frame=frame+1
                x=x+delta
                combat.ordinary(.02,player,bare,pos(x),pos(1),pos(0),blocking)
            end
        end
        ordinary.reset()
        move(60,0,true)
        move(4,.09,true)
        assert(count(fists)==initial and sweeps==0 and swishes==0,"moving a held guard attacked")
        move(20,0,false)
        move(4,.09,false)
        assert(count(fists)==initial+1 and sweeps>0 and swishes==1,"guard release did not rearm the punch")
        player.VRMeleeSweepHit,player.VRMeleeWhoosh=savedSweep,savedWhoosh
    end
    assert(guard(player,bare) and adds==0)
    hold=true;guard(player,bare);guard(player,bare)
    assert(adds==1 and removes==0,"held guard stacked block modifiers")
    hold=false;guard(player,bare);assert(removes==1,"released guard stayed active")
    hold=true;guard(player,bare);paused=true;guard(player,bare);paused=false
    assert(adds==2 and removes==2,"pause kept the fist block")
    guard(player,bare);assert(not guard(player,actor(gun)) and removes==3,"guard survived a weapon change")
end
local cf,setting,gunLeft=true,true,true
local requests=0
VRCyberFingerEnabled=function()return cf end
VRCyberFingerRightHandSwords=function()return cf and setting end
VRLeftHanded=function()return gunLeft end
player.VRLeftHandedRehand=function()requests=requests+1;return false end
spdlog={info=function()end}
rehand(player,.016)
cf=false; rehand(player,.016)
cf=true; rehand(player,.016)
assert(requests==0,"a controller focus change put away a one-handed sword")
setting=false;rehand(player,.016)
gunLeft=false;rehand(player,.016)
assert(requests==2,"an explicit sword or weapon-hand change was ignored")

local carry=up(update,"carryTick")
local checked=0
VRWeaponInLeftHand=function()return false end
Game.GetPlayer=function()return player end
GameObject={IsDualWielding=function()return false end}
player.GetActiveWeapon=function()return primary end
up(carry,"cyberware").kind=function()return 0 end
Game.GetTransactionSystem=function()return {GetItemInSlot=function()checked=checked+1;return primary end} end
GetVRSharedSlot=function()return 1 end
local source=WeaponObject.IsOfType
local recognized=0
WeaponObject.IsOfType=function(id,kind) recognized=recognized+1;return source(id,kind) end
if not up(carry,"CARRY_LEFT") then
    carry(.016)
    assert(checked==0,"disabled carry touched a weapon")
    for i=1,100 do
        local name=debug.getupvalue(carry,i)
        if name=="CARRY_LEFT" then debug.setupvalue(carry,i,true);break end
    end
end
carry(.016)
assert(checked==1 and recognized==1,"a second grip failed to take the katana-specific path")
print("Off-hand katana and punch tests passed")
