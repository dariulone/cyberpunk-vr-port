local I=require("modules/inventory_data")
local Images=require("modules/gameplay_data")
local D={}
function D.open()
  local c=I.open();c.inventory=Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem"))
  c.compare=InventoryItemPreferredComparisonResolver.Make(c.inventory)
  return c
end
D.close=I.close
local function item(ctx,visit,raw,side,quantity,buyback,available)
  if not raw or ItemID.HasFlag(raw:GetID(),gameEItemIDFlag.Preview) then return nil end
  -- Grid rows need the native UI item's cached metadata, not a full legacy
  -- InventoryItemData conversion (stats/attachments/tooltips for every item).
  local ui=side=="sell" and ctx.inventory:GetPlayerItemFromAnySource(raw) or UIInventoryItem.Make(visit.vendor,raw,ctx.inventory:GetInventoryItemsManager())
  if not ui then return nil end
  local id=raw:GetID()
  local v={id=id,key=side..":"..(buyback and "back:" or "")..I.key(id),ui=ui,raw=raw,side=side,
    buyback=buyback==true,quantity=quantity or raw:GetQuantity(),available=available~=false,
    name=ui:GetName(),quality=NameToString(ui:GetQualityName()),qualityInt=ui:GetQualityInt(),type=ui:GetItemType(),
    equipped=side=="sell" and ui:IsEquipped() or false,new=ui:IsNew()}
  -- UIInventoryItem resolves recipes to their crafting-result icon, unlike the
  -- legacy InventoryItemData icon path (empty for many vendor blueprints).
  local icon=v.ui:GetIconPath()
  if icon and icon~="" and icon~="None" then v.image=Images.image(TweakDBID.new(icon)) or v.image end
  v.recipe=v.ui:IsRecipe()
  v.price=(side=="sell" or buyback) and visit.manager:GetSellingPrice(v.id) or visit.manager:GetBuyingPrice(v.id)
  v.iconic=v.ui:IsIconic();v.favorite=v.ui:IsPlayerFavourite()
  -- ranged and melee weapons in tabs of their own (issue #221)
  v.melee=v.ui:IsWeapon() and raw:HasTag(CName.new("MeleeWeapon")) or false
  v.category=v.ui:IsWeapon() and (v.melee and "melee" or "ranged") or v.ui:IsClothing() and "clothing" or v.ui:IsPart() and "mods"
    or RPGManager.GetItemCategory(v.id)==gamedataItemCategory.Consumable and "consumable" or "other"
  return v
end
local function sort(a,b)
  if a.equipped~=b.equipped then return a.equipped end
  local qa,qb=a.ui:GetComparisonQualityF(),b.ui:GetComparisonQualityF()
  if qa~=qb then return qa>qb end
  if a.name~=b.name then return a.name<b.name end
  return a.key<b.key
end
function D.snapshot(ctx,visit)
  local out={sell={},buy={},byKey={},vendorMoney=MarketSystem.GetVendorMoney(visit.vendor),money=visit.manager:GetLocalPlayerCurrencyAmount(),name=I.loc(visit.manager:GetVendorName())}
  for _,raw in ipairs(visit.manager:GetItemsPlayerCanSellFast(Game.GetPlayer(),false)) do
    local v=item(ctx,visit,raw,"sell")
    if v then out.sell[#out.sell+1]=v;out.byKey[v.key]=v end
  end
  local sold=visit.manager:GetVendorSoldItems()
  for _,stock in ipairs(visit.manager:GetVendorInventoryItems()) do
    local raw=stock.itemData;local id=raw:GetID();local back=sold:GetItem(id)
    local backQty=back and math.min(back.quantity,raw:GetQuantity()) or 0
    local available=VendorItemAdditionalData.Make(stock.itemStack).IsAvailable
    for _,part in ipairs({{quantity=raw:GetQuantity()-backQty,back=false},{quantity=backQty,back=true}}) do
      if part.quantity>0 then
        local v=item(ctx,visit,raw,"buy",part.quantity,part.back,available)
        if v then out.buy[#out.buy+1]=v;out.byKey[v.key]=v end
      end
    end
  end
  table.sort(out.sell,sort);table.sort(out.buy,sort)
  return out
end
function D.beginSnapshot(ctx,visit)
  return {out={sell={},buy={},byKey={},vendorMoney=MarketSystem.GetVendorMoney(visit.vendor),
      money=visit.manager:GetLocalPlayerCurrencyAmount(),name=I.loc(visit.manager:GetVendorName())},
    sell=visit.manager:GetItemsPlayerCanSellFast(Game.GetPlayer(),false),
    buy=visit.manager:GetVendorInventoryItems(),sold=visit.manager:GetVendorSoldItems(),side="sell",index=1}
end
function D.stepSnapshot(ctx,visit,task,deadline)
  local function add(v) if v then task.out[v.side][#task.out[v.side]+1]=v;task.out.byKey[v.key]=v end end
  for _=1,96 do
    if task.side=="sell" then
      local raw=task.sell[task.index]
      if raw then add(item(ctx,visit,raw,"sell"));task.index=task.index+1
      else task.side="buy";task.index=1 end
    else
      local stock=task.buy[task.index]
      if not stock then return task.out end
      local raw=stock.itemData;local back=task.sold:GetItem(raw:GetID())
      local backQty=back and math.min(back.quantity,raw:GetQuantity()) or 0
      local available=VendorItemAdditionalData.Make(stock.itemStack).IsAvailable
      if raw:GetQuantity()>backQty then add(item(ctx,visit,raw,"buy",raw:GetQuantity()-backQty,false,available)) end
      if backQty>0 then add(item(ctx,visit,raw,"buy",backQty,true,available)) end
      task.index=task.index+1
    end
    if os.clock()>=deadline then break end
  end
end
function D.quote(ctx,visit,v,quantity)
  if not visit or not v then return {allowed=false,max=0,reason="visit"} end
  local p=Game.GetPlayer();local ts=Game.GetTransactionSystem()
  local owner=v.side=="sell" and p or visit.vendor
  local raw=ts:GetItemData(owner,v.id)
  if not raw then return {allowed=false,max=0,reason="missing"} end
  local count=raw:GetQuantity();local price
  if v.side=="sell" then
    price=visit.manager:GetSellingPrice(v.id)
    local kind=raw:GetItemType()
    if not visit.manager:CanPlayerSellItem(v.id) or ctx.ui:IsItemPlayerFavourite(v.id)
      or kind==gamedataItemType.Con_Inhaler or kind==gamedataItemType.Con_Injector or kind==gamedataItemType.Gad_Grenade then
      return {allowed=false,max=0,reason="protected",price=price}
    end
    if price>0 then count=math.min(count,math.floor(MarketSystem.GetVendorMoney(visit.vendor)/price)) end
  else
    price=v.buyback and visit.manager:GetSellingPrice(v.id) or visit.manager:GetBuyingPrice(v.id)
    local back=visit.manager:GetVendorSoldItems():GetItem(v.id)
    local backQty=back and back.quantity or 0
    count=v.buyback and math.min(count,backQty) or math.max(0,count-backQty)
    local found=false
    for _,s in ipairs(visit.manager:GetVendorInventoryItems()) do if s.itemData:GetID()==v.id then found=VendorItemAdditionalData.Make(s.itemStack).IsAvailable;break end end
    if not found then return {allowed=false,max=0,reason="requirements",price=price} end
    if raw:HasTag(CName.new("MustBeWearableToPurchase")) and not v.ui:GetRequirementsManager(p):IsEquippable() then return {allowed=false,max=0,reason="requirements",price=price} end
    if raw:HasTag(CName.new("Ammo")) then count=math.min(count,math.max(0,raw:GetStatValueByType(gamedataStatType.Quantity)-ts:GetItemQuantity(p,v.id))) end
    if raw:HasTag(CName.new("Grenade")) or raw:HasTag(CName.new("Injector")) or raw:HasTag(CName.new("Inhaler")) then count=math.min(count,1) end
    if price>0 then count=math.min(count,math.floor(visit.manager:GetLocalPlayerCurrencyAmount()/price)) end
  end
  count=math.max(0,math.floor(count));quantity=math.floor(quantity or 1)
  return {allowed=quantity>0 and quantity<=count,max=count,price=price,total=price*quantity,
    reason=count==0 and "funds" or nil,raw=raw,confirm=v.side=="sell" and (v.ui:IsIconic() or EquipmentSystem.GetInstance(p):IsEquipped(p,v.id))}
end
function D.transact(ctx,visit,v,quantity,confirmed)
  local quote=D.quote(ctx,visit,v,quantity)
  if not quote.allowed then return false end
  if quote.confirm and not confirmed then return "confirm" end
  if v.side=="sell" then visit.manager:SellItemToVendor(quote.raw,quantity)
  elseif v.buyback then visit.manager:BuybackItemFromVendor(quote.raw,quantity)
  else visit.manager:BuyItemFromVendor(quote.raw,quantity) end
  return true
end
-- SEVERAL AT ONCE (the sell side's selection, issue #221): whole stacks, in the order given, as far as the vendor's
-- money goes. Equipped and protected items (favourites, inhalers, injectors, grenades) are left where they are. Each
-- sale is a queued MarketSystem request, so the vendor's money is counted down here rather than read back (the read
-- lags a tick). Returns the stacks sold, the ones left, the eurodollars and the pieces.
function D.sellMany(ctx,visit,list)
  local p=Game.GetPlayer();local eq=EquipmentSystem.GetInstance(p)
  local money=MarketSystem.GetVendorMoney(visit.vendor)
  local items,skipped,earned,pieces=0,0,0,0
  for _,v in ipairs(list) do
    local quote=v.side=="sell" and D.quote(ctx,visit,v,1) or {allowed=false}
    if not quote.allowed or not quote.raw or eq:IsEquipped(p,v.id) then skipped=skipped+1
    else
      local q=quote.raw:GetQuantity()
      if quote.price>0 then q=math.min(q,math.floor(money/quote.price)) end
      if q<1 then skipped=skipped+1
      else
        visit.manager:SellItemToVendor(quote.raw,q)
        money=money-quote.price*q;earned=earned+quote.price*q;items=items+1;pieces=pieces+q
      end
    end
  end
  return items,skipped,earned,pieces
end
function D.signature(visit)
  return tostring(visit.manager:GetLocalPlayerCurrencyAmount())..":"..tostring(MarketSystem.GetVendorMoney(visit.vendor))..":"..tostring(Game.GetPlayer().curInventoryWeight)
end
return D
