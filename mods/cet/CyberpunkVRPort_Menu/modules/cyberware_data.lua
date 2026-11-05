local I=require("modules/inventory_data")
local D={}
D.areas={"FrontalCortexCW","SystemReplacementCW","ArmsCW","EyesCW","MusculoskeletalSystemCW","HandsCW","NervousSystemCW","CardiovascularSystemCW","IntegumentarySystemCW","LegsCW"}
function D.open()
  local c=I.open();c.uiInventory=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem"))
  return c
end
D.close=I.close
function D.session(kit)
  local session=kit.ripperSession and kit.ripperSession()
  if not session or not session.active or not session.vendor or not session.manager then return nil end
  if session.ripperdoc==false then return nil end
  local p,v=Game.GetPlayer():GetWorldPosition(),session.vendor:GetWorldPosition()
  if (p.x-v.x)^2+(p.y-v.y)^2+(p.z-v.z)^2>64 then return nil end
  return session
end
function D.stats()
  local p=Game.GetPlayer();local ss=Game.GetStatsSystem();local sd=Game.GetStatsDataSystem()
  local function stat(n) return ss:GetStatValue(p:GetEntityID(),gamedataStatType[n]) end
  local total,over=stat("HumanityTotalMaxValue"),stat("HumanityOverallocationPossible")
  local armorMax=sd:GetValueFromCurve(CName.new("cyberware_curves"),stat("Level"),CName.new("max_armor_for_level"))
  local factor=stat("ArmorEffectivenessMultiplier")*sd:GetArmorEffectivenessValue(true)
  local health=stat("Health");local penalty=stat("EdgerunnerHealthReduction")
  return {used=stat("HumanityAllocated"),max=total,base=total-over,extra=over,
    over=stat("HumanityOverallocated"),healthLoss=penalty<1 and health/(1-penalty)*penalty or 0,
    armor=stat("Armor"),armorMax=armorMax,reduction=(1-1/(1+armorMax*factor))*100,
    edgerunner=PlayerDevelopmentSystem.GetData(p):IsNewPerkBoughtAnyLevel(gamedataNewPerkType.Tech_Master_Perk_3)}
end
function D.upgrade(item)
  if not item or not item.equipped then return nil end
  local ok,q,record,cost=RPGManager.CanUpgradeCyberware(Game.GetPlayer(),item.id,true,gamedataQuality.Invalid,true)
  if not record then return nil end
  local count=Game.GetTransactionSystem():GetItemQuantityWithDuplicates(Game.GetPlayer(),ItemID.CreateQuery(cost.materialRecordID))
  return {allowed=ok,quality=q,record=record,cost=cost,have=count}
end
function D.item(ctx,raw,vendor,ui)
  if not raw then return nil end
  local item=I.item(ctx,ctx.manager:GetInventoryItemData(raw))
  if not item then return nil end
  item.raw=raw;item.vendor=vendor==true
  item.ui=ui or ctx.uiInventory:GetPlayerItem(item.id)
  item.capacity=0;item.armor=raw:GetStatValueByType(gamedataStatType.Armor)
  item.requirements=RPGManager.GetEquipRequirements(Game.GetPlayer(),raw)
  for _,req in ipairs(item.requirements) do if req.statType==gamedataStatType.HumanityAvailable then item.capacity=req.requiredValue end end
  item.upgrade=not item.vendor and D.upgrade(item) or nil
  return item
end
-- Every implant the player owns, by equipment area: installed or not. UIInventoryScriptableSystem's GetPlayerAreaItems
-- answers the installed ones only (GetRawEquippedItems), so one bought earlier and never installed -- or a second
-- one for another slot -- was offered nowhere; the native ripperdoc lists the player's whole item map by area.
local function ownedByArea(p)
  local out={}
  local _,items=Game.GetTransactionSystem():GetItemList(p)
  for _,data in ipairs(items or {}) do
    local id=data:GetID()
    if not ItemID.HasFlag(id,gameEItemIDFlag.Preview) then
      local ok,area=pcall(function() return tonumber(EnumInt(EquipmentSystem.GetEquipAreaType(id))) end)
      if ok and area then
        local list=out[area];if not list then list={};out[area]=list end
        list[#list+1]=data
      end
    end
  end
  return out
end
function D.snapshot(ctx,kit)
  local p=Game.GetPlayer();local equip=EquipmentSystem.GetData(p)
  local out={groups={},byKey={},stats=D.stats(),vendor=D.session(kit)~=nil}
  local owned=ownedByArea(p)
  for _,area in ipairs(D.areas) do
    local enum=gamedataEquipmentArea[area];local group={area=area,slots={},items={},count=equip:GetNumberOfSlots(enum)}
    group.name=I.loc(TweakDBInterface.GetEquipmentAreaRecord(TweakDBID.new("EquipmentArea."..area)):LocalizedName())
    for slot=0,group.count-1 do
      local id=equip:GetItemInEquipSlot(enum,slot)
      local item=ItemID.IsValid(id) and D.item(ctx,Game.GetTransactionSystem():GetItemData(p,id)) or nil
      if item then item.slot=slot;out.byKey[item.key]=item end
      group.slots[#group.slots+1]={index=slot,item=item}
    end
    for _,data in ipairs(owned[tonumber(EnumInt(enum))] or {}) do
      local item=D.item(ctx,data,false,ctx.uiInventory:GetPlayerItem(data:GetID()))
      if item then group.items[#group.items+1]=item;out.byKey[item.key]=item end
    end
    table.sort(group.items,function(a,b) if a.equipped~=b.equipped then return a.equipped end;return a.name<b.name end)
    out.groups[#out.groups+1]=group
  end
  local ripper=D.session(kit)
  if ripper then
    for _,v in ipairs(ripper.manager:GetRipperDocItems()) do
      local ui=UIInventoryItem.Make(ripper.vendor,v.itemData,ctx.uiInventory:GetInventoryItemsManager())
      local item=D.item(ctx,v.itemData,true,ui)
      if item then
        item.key="vendor:"..item.key
        local a=VendorItemAdditionalData.Make(v.itemStack);item.available=a.IsAvailable
        item.price=ripper.manager:GetBuyingPrice(item.id)
        for _,g in ipairs(out.groups) do if gamedataEquipmentArea[g.area]==item.area then g.items[#g.items+1]=item;out.byKey[item.key]=item;break end end
      end
    end
  end
  return out
end
function D.check(kit,ctx,item,area,slot)
  local e=EquipmentSystem.GetData(Game.GetPlayer());local enum=gamedataEquipmentArea[area]
  local replaced=e:GetItemInEquipSlot(enum,slot)
  -- Native ripperdoc replaces the same cyberware type before filling another slot.
  if slot<0 or slot>=e:GetNumberOfSlots(enum) then return false,0 end
  local function cyberType(id) return TweakDBInterface.GetCName(TweakDBID.new(ItemID.GetTDBID(id),".cyberwareType"),CName.new("None")) end
  local kind=cyberType(item.id)
  for i=0,e:GetNumberOfSlots(enum)-1 do local id=e:GetItemInEquipSlot(enum,i)
    if ItemID.IsValid(id) and cyberType(id)==kind then slot=i;replaced=id;break end
  end
  local old=ItemID.IsValid(replaced) and Game.GetTransactionSystem():GetItemData(Game.GetPlayer(),replaced) or nil
  local oldReq=old and RPGManager.GetEquipRequirements(Game.GetPlayer(),old) or {}
  local excess=0;local ok=true
  for _,req in ipairs(RPGManager.GetEquipRequirements(Game.GetPlayer(),item.raw)) do
    local required=req.requiredValue
    for _,r in ipairs(oldReq) do if r.statType==req.statType then required=required-r.requiredValue;break end end
    local available=Game.GetStatsSystem():GetStatValue(Game.GetPlayer():GetEntityID(),req.statType)
    if required>available+.001 then ok=false;if req.statType==gamedataStatType.HumanityAvailable then excess=required-available end end
  end
  if old and old:HasTag(CName.new("UnequipBlocked")) then ok=false end
  if old and old:HasTag(CName.new("CapacityBooster")) then
    local stats=Game.GetStatsSystem();local p=Game.GetPlayer():GetEntityID()
    if stats:GetStatValue(p,gamedataStatType.HumanityAvailable)<stats:GetStatValue(p,gamedataStatType.CapacityBoosterHumanity) then ok=false end
  end
  if item.vendor and item.available==false then ok=false end
  return ok,excess,slot
end
function D.equip(kit,ctx,item,area,slot)
  local c=D.session(kit);if not c or c.purchase then return false end
  local allowed,_,target=D.check(kit,ctx,item,area,slot);if not allowed then return false end
  if item.vendor then
    local available=false
    for _,v in ipairs(c.manager:GetRipperDocItems()) do if v.itemData:GetID()==item.id and VendorItemAdditionalData.Make(v.itemStack).IsAvailable then available=true;break end end
    if not available or c.manager:GetBuyingPrice(item.id)>c.manager:GetLocalPlayerCurrencyAmount() then return false end
    local before={};for _,ui in ipairs(ctx.uiInventory:GetPlayerAreaItems(item.area)) do before[tostring(ui:GetID())]=true end
    c.purchase={id=item.id,record=ItemID.GetTDBID(item.id),slot=target,area=area,before=before,at=kit.S.time}
    c.manager:BuyItemFromVendor(item.raw,1)
    return true
  end
  if not Game.GetTransactionSystem():HasItem(Game.GetPlayer(),item.id) then return false end
  local equipment=Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
  local previous=EquipmentSystem.GetData(Game.GetPlayer()):GetItemInEquipSlot(gamedataEquipmentArea[area],target)
  if previous==item.id then return false end
  if ItemID.IsValid(previous) then
    local remove=UnequipRequest.new();remove.owner=Game.GetPlayer();remove.areaType=gamedataEquipmentArea[area];remove.slotIndex=target
    equipment:QueueRequest(remove)
  end
  local request=GameplayEquipRequest.new();request.owner=Game.GetPlayer();request.itemID=item.id;request.slotIndex=target
  equipment:QueueRequest(request)
  return true
end
function D.updatePurchase(kit,ctx)
  local s=D.session(kit);local purchase=s and s.purchase;if not purchase then return false end
  if purchase.polled and kit.S.time-purchase.polled<.1 then return false end
  purchase.polled=kit.S.time
  local _,items=Game.GetTransactionSystem():GetItemList(Game.GetPlayer())
  for _,raw in ipairs(items) do
    local id=raw:GetID()
    if ItemID.GetTDBID(id)==purchase.record and not purchase.before[tostring(id)] then
      s.purchase=nil;ctx.uiInventory:GetInventoryItemsManager():FlushEquippedItems()
      local item=D.item(ctx,raw);D.equip(kit,ctx,item,purchase.area,purchase.slot);return true
    end
  end
  if kit.S.time-purchase.at>5 then s.purchase=nil;kit.toast(kit.T.blocked);return true end
  return false
end
function D.prepareUpgrade(kit,ctx,item)
  if not D.session(kit) then return nil end
  local up=D.upgrade(item);if not up or not up.record then return nil end
  local seed=Game.GetInventoryManager():GetCyberwareUpgradeSeed()
  local data=RPGManager.GetCyberwareUpgradeData(Game.GetPlayer(),item.ui,up.record,up.cost,seed,ctx.uiInventory,RPGManager.ShouldShowCWChoice(Game.GetPlayer(),item.ui))
  return {base=item,up=up,data=data,seed=seed,choice=1}
end
function D.applyUpgrade(kit,ctx,upgrade)
  if not D.session(kit) then return false end
  local slot=EquipmentSystem.GetData(Game.GetPlayer()):GetSlotIndex(upgrade.base.id)
  if slot<0 or EquipmentSystem.GetData(Game.GetPlayer()):GetItemInEquipSlot(upgrade.base.area,slot)~=upgrade.base.id then return false end
  local up=D.upgrade(upgrade.base)
  if not up or not up.allowed or up.record:GetID()~=upgrade.up.record:GetID() or Game.GetInventoryManager():GetCyberwareUpgradeSeed()~=upgrade.seed then return false end
  local chosen=upgrade.data["option"..upgrade.choice.."GameItemData"]
  if not chosen then return false end
  local candidate=D.item(ctx,chosen,false,upgrade.data["option"..upgrade.choice.."InventoryItem"])
  local area
  for _,key in ipairs(D.areas) do if gamedataEquipmentArea[key]==candidate.area then area=key;break end end
  if not area or not D.check(kit,ctx,candidate,area,slot) then return false end
  local result=RipperdocTokenPopupCloseData.new();result.confirm=true;result.chosenOptionIndex=upgrade.choice
  result.chosenOptionData=chosen;result.costData=up.cost
  RPGManager.HandleBuyShardPopupClosed(Game.GetPlayer(),upgrade.base.id,result)
  return true
end
function D.signature()
  local e=EquipmentSystem.GetData(Game.GetPlayer());local a={}
  for _,area in ipairs(D.areas) do for s=0,e:GetNumberOfSlots(gamedataEquipmentArea[area])-1 do a[#a+1]=I.key(e:GetItemInEquipSlot(gamedataEquipmentArea[area],s)) end end
  local stats=D.stats();a[#a+1]=stats.used;a[#a+1]=stats.max;a[#a+1]=stats.armor
  for _,tier in ipairs({"Common","Uncommon","Rare","Epic","Legendary"}) do a[#a+1]=Game.GetTransactionSystem():GetItemQuantityWithDuplicates(Game.GetPlayer(),ItemID.CreateQuery(TweakDBID.new("Items."..tier.."Material1"))) end
  return table.concat(a,"|")
end
return D
