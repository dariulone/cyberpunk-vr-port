-- Exercise the complete data module against queued purchases and a native-like
-- customization system. No UI mocks can prove CET loading or visual results.
local root=... or "."
local G=dofile(root.."/mods/cet/CyberpunkVRPort_Menu/modules/glamdoc_data.lua")
CName={new=function(x)return x end};TDBID={ToStringDEBUG=function(x)return x end,IsValid=function(x)return x~=nil end}
NameToString=function(x)return x end
ItemID={GetTDBID=function(x)return x end,IsValid=function(x)return x~=nil and x~="none" end,HasFlag=function()return false end}
gameEItemIDFlag={Preview=1};gamedataEquipmentArea={ArmsCW="arm",ArmsCWStyle="style"}
gamedataItemType={Cyb_Launcher="launcher",Cyb_MantisBlades="blades",Cyb_NanoWires="wire",Cyb_StrongArms="strong"}
Color={new=function()return {} end};IsEP1=function()return false end
local player={GetEntityID=function()return {hash=7}end}
local installed="blades";local money=200;local facts={};local buys=0;local paid=0;local boughtRequest
local original={enabled=true,customizationItemID="owned",switcherName="arms",switcherIndex=1,primaryColorIndex=1,secondaryColorIndex=2}
local saved=original;local shown=original;local requests={};local equipmentRequests={};local inventory={};local stock={};local resets={}
local function raw(id,tag,extra)
 return {GetID=function()return id end,HasTag=function(_,t)return t=="Glamdoc" or t==tag or t==extra end,
   GetItemType=function()return id=="active" and installed or "style"end,GetQuantity=function()return 1 end}
end
local a,b,wrong,ep1=raw("owned","MantisBlades"),raw("new","MantisBlades"),raw("wrong","StrongArms"),raw("expansion","MantisBlades","EP1")
inventory={a};stock={{itemData=b,itemStack={available=true}},{itemData=wrong,itemStack={available=true}},{itemData=ep1,itemStack={available=true}}}
local function data(id)if id=="active"then return raw("active","MantisBlades")end;for _,v in ipairs(inventory)do if v:GetID()==id then return v end end end
local manager={GetVendorID=function()return "vendor"end,GetLocalPlayerCurrencyAmount=function()return money end,
 GetBuyingPrice=function()return 120 end,GetGlamDocItems=function()return stock end,
 BuyItemFromVendor=function(_,item,n)assert(n==1);buys=buys+1;boughtRequest=item end}
VendorItemAdditionalData={Make=function(v)return {IsAvailable=v.available}end}
local equip={GetActiveItem=function()return installed and "active" or "none"end,IsSideUpgradeEquipped=function()return false end,IsEquippable=function()return true end}
EquipmentSystem={GetData=function()return equip end};EquipmentSystemPlayerData={HasLongSleeves=function()return true end}
local ui={GetInventoryItemsManager=function()return {FlushEquippedItems=function()end}end}
UIInventoryItem={Make=function(_,v)return {IsRecipe=function()return false end,GetName=function()return v:GetID()end,
 GetQualityInt=function()return 1 end,GetIconPath=function()return ""end}end}
local brand={GetRecordID=function()return "brand"end}
CyberwareCustomizationHelper={GetDefaultPrimaryColorIndexFromCustomizationItem=function()return 0 end,
 GetDefaultSecondaryColorIndexFromCustomizationItem=function()return 4 end,GetBrandDataFromCustomizationItem=function()return brand end,
 GetColorFromBrandData=function(_,i)return {Red=i*40,Green=255,Blue=0}end,GetCyberarmTypeFromCustomizationItem=function()return "arms"end,
 GetCustomizationIndexFromCustomizationItem=function()return 3 end,GetLongSleevesCustomizationIndexFromCustomizationItem=function()return 7 end}
for _,name in ipairs({"ApplyCyberwareCustomizationDataRequest","SaveAndApplyCyberwareCustomizationDataRequest","EquipRequest","UnequipRequest"})do
 local kind=name;_G[name]={new=function()return {kind=kind}end}
end
local customization={GetSavedData=function()return saved end,QueueRequest=function(_,r)
 requests[#requests+1]=r;shown=r
 if r.kind=="SaveAndApplyCyberwareCustomizationDataRequest"then saved=r;saved.customizationItemID=r.customizationItem and r.customizationItem:GetID() or "none"end
end,RestoreCyberwareCustomization=function()shown=saved end,SetIndexForCustomizationSwitcher=function(_,name,index)shown={index=index};resets[#resets+1]={name,index}end,
 SetCustomizationParameters=function(_,enabled)shown.enabled=enabled end}
local systems={CyberwareCustomizationSystem=customization,UIInventoryScriptableSystem=ui,
 EquipmentSystem={QueueRequest=function(_,r)equipmentRequests[#equipmentRequests+1]=r end}}
Game={GetPlayer=function()return player end,GetScriptableSystemsContainer=function()return {Get=function(_,n)return systems[n]end}end,
 GetTransactionSystem=function()return {GetItemList=function()return true,inventory end,GetItemData=function(_,_,id)return data(id)end}end,
 GetInventoryManager=function()return {GetSpendByVendorID=function()return 0 end,SaveSpendByVendorID=function(_,_,cost)paid=cost end}end,
 GetQuestsSystem=function()return {GetFact=function(_,n)return facts[n]or 0 end,SetFact=function(_,n,v)facts[n]=v end,SetFactStr=function(_,n,v)facts[n]=v end}end}
TweakDBInterface={GetGlamdocRecord=function()return {GetFactThresholdsCount=function()return 1 end,GetFactThresholdsItem=function()
 return {SpendCost=function()return 100 end,SpendCount=function()return 0 end,FactKey=function()return "vendor-threshold"end}end}end}
local shared={};G.bind(shared)
local visit={active=true,glamdoc=true,vendor={},manager=manager}
local function begin()G.poll(0);return assert(G.begin(visit))end
local function item(key)for _,v in ipairs(G.rows())do if v.key==key then return v end end end
local s=begin();assert(s.area==2)
assert(#G.rows()==2 and item("owned") and item("new") and not item("expansion"),"stock must respect ownership and DLC")
local offered=item("new");assert(G.select(offered) and s.preview)
assert(saved==original and #equipmentRequests==0,"preview must not save/equip an unowned item")
assert(not G.finish(true),"unowned preview applied without purchase")
G.color("primary",3);G.color("secondary",0);G.swap();assert(s.primary==0 and s.secondary==3)
G.resetColors();assert(s.primary==0 and s.secondary==4)
assert(G.cancelPreview() and s.raw==a and s.primary==1 and s.secondary==2 and not s.dirty)
assert(G.select(item("owned")));G.remove();assert(not shown.enabled)
assert(G.select(offered));assert(G.cancelPreview());assert(not shown.enabled,"cancel preview lost pending remove")
assert(G.finish(false));assert(saved==original and shown==saved and #equipmentRequests==0,"cancel failed to restore original style")
s=begin();s.area=4;local incompatible=item("wrong");assert(not G.select(incompatible),"wrong arm type previewed")
assert(G.quote(incompatible).allowed and not G.quote(incompatible).compatible,"incompatible style should be purchasable for later")
assert(not G.buy(incompatible,true),"buy-and-apply accepted mismatched arm")
G.finish(false);s=begin();offered=item("new")
money=100;assert(not G.buy(offered) and buys==0,"stale balance allowed a purchase")
money=200;stock[1].itemStack.available=false;assert(not G.buy(offered) and buys==0,"vendor requirement ignored")
stock[1].itemStack.available=true
G.select(offered);G.color("primary",2);G.color("secondary",3)
assert(G.buy(offered,true));assert(not G.buy(offered,true) and buys==1,"duplicate pending purchase")
assert(saved==original,"buy request equipped before ownership transfer")
inventory[#inventory+1]=b;money=80;G.poll(.25)
assert(saved.customizationItemID=="new" and saved.primaryColorIndex==2 and saved.secondaryColorIndex==3,"purchased preview colors lost")
assert(saved.switcherIndex==7,"apply ignored long-sleeve variant")
assert(#equipmentRequests==1 and equipmentRequests[1].itemID=="new","received purchase not equipped")
assert(paid==120 and facts.glam_bought==1 and facts["vendor-threshold"]==1,"vendor purchase accounting/facts lost")
assert(shared.glamdocSession==nil,"apply-after-purchase session retained")
s=begin();assert(not item("new").vendor,"owned style offered for repurchase")
G.remove();G.finish(true);assert(saved.enabled==false and saved.customizationItemID=="none")
assert(equipmentRequests[#equipmentRequests].kind=="UnequipRequest")
G.poll(0);s=begin();s.area=4;incompatible=item("wrong");money=200
assert(G.buy(incompatible));G.finish(false);assert(s.closed and s.pending)
inventory[#inventory+1]=wrong;G.poll(.25);assert(paid==120 and shared.glamdocSession==nil,"closed purchase lost receipt or session")
assert(saved.enabled==false,"receipt after cancel unexpectedly equipped a style")
installed=nil;s=begin();s.area=2;assert(not G.compatible(a),"bare arms accepted cyberware style")
G.finish(false)
G.poll(0);saved={enabled=false,customizationItemID="none",switcherName="None",switcherIndex=0,primaryColorIndex=0,secondaryColorIndex=0}
installed="blades";s=begin();assert(G.select(item("owned")));assert(G.finish(false))
assert(resets[#resets][1]=="arms" and resets[#resets][2]==0,"unconfigured player's preview switcher was not reset on cancel")
print("PASS Glamdoc stock/DLC, arm compatibility, preview/colors/cancel, asynchronous purchase, no duplicate spend, receipts after close, apply/long sleeves/remove")
