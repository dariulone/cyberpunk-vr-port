-- Glamdoc's stock, compatibility and customization system, presented in the
-- world menu. Preview uses the game's non-persistent request on V's own arms;
-- saving/equipping happens on Apply, so Cancel never leaves an unowned style.
local G={}
local shared
local tags={"ProjectileLauncher","MantisBlades","NanoWires","StrongArms"}
local types={"Cyb_Launcher","Cyb_MantisBlades","Cyb_NanoWires","Cyb_StrongArms"}
G.labels={"LocKey#3722","LocKey#3717","LocKey#3720","LocKey#3718"}
local function current() return shared and shared.glamdocSession end
local function identity(p) return p and tostring(p:GetEntityID().hash) end
local function system(name) return Game.GetScriptableSystemsContainer():Get(CName.new(name)) end
local function record(id) return TDBID.ToStringDEBUG(ItemID.GetTDBID(id)) end
local function has(raw,tag) return raw and raw:HasTag(CName.new(tag)) end
function G.kind(raw)
  for i,tag in ipairs(tags) do if has(raw,tag) then return i end end
end
local function owned(s,key)
  local _,items=Game.GetTransactionSystem():GetItemList(s.player)
  for _,raw in ipairs(items or {}) do
    if record(raw:GetID())==key and not ItemID.HasFlag(raw:GetID(),gameEItemIDFlag.Preview) then return raw end
  end
end
function G.arm(s)
  s=s or current();if not s then return nil end
  local id=EquipmentSystem.GetData(s.player):GetActiveItem(gamedataEquipmentArea.ArmsCW)
  local raw=ItemID.IsValid(id) and Game.GetTransactionSystem():GetItemData(s.player,id)
  if raw then for i,name in ipairs(types) do if raw:GetItemType()==gamedataItemType[name] then return i end end end
end
function G.compatible(raw)
  local s=current();return s~=nil and G.kind(raw)~=nil and G.kind(raw)==G.arm(s)
end
local function defaults(raw)
  return CyberwareCustomizationHelper.GetDefaultPrimaryColorIndexFromCustomizationItem(raw),
    CyberwareCustomizationHelper.GetDefaultSecondaryColorIndexFromCustomizationItem(raw)
end
local function preview(s)
  if s.raw and s.enabled then
    local switcher=CyberwareCustomizationHelper.GetCyberarmTypeFromCustomizationItem(s.raw)
    local key=NameToString(switcher)
    for other,name in pairs(s.previewSwitchers) do if other~=key then s.system:SetIndexForCustomizationSwitcher(name,0,true) end end
    s.previewSwitchers[key]=switcher
    local request=ApplyCyberwareCustomizationDataRequest.new()
    request.enabled=true;request.customizationItem=s.raw
    request.primaryColorIndex=s.primary;request.secondaryColorIndex=s.secondary
    s.system:QueueRequest(request)
  else
    -- Native customization accepts a disabled state without replacing the
    -- saved item. The switcher and colors are restored when the session ends.
    for _,name in pairs(s.previewSwitchers) do s.system:SetIndexForCustomizationSwitcher(name,0,true) end
    if NameToString(s.baseline.switcherName)~="None" then s.system:SetIndexForCustomizationSwitcher(s.baseline.switcherName,0,true) end
    s.system:SetCustomizationParameters(false,Color.new(),Color.new())
  end
end
local function restore(s)
  if s.previewed and Game.GetPlayer() and identity(Game.GetPlayer())==s.playerKey then
    -- A player with no saved style has switcherName=None. Restore alone cannot
    -- undo a newly previewed arm switcher; reset every previewed channel first.
    for _,name in pairs(s.previewSwitchers) do s.system:SetIndexForCustomizationSwitcher(name,0,true) end
    s.system:RestoreCyberwareCustomization(true)
  end
  s.previewed=false
end
local function refresh(s) s.revision=s.revision+1 end
function G.begin(visit)
  local s=current();if s and not s.closed then return s end
  if not visit or not visit.active or not visit.glamdoc or not Game.GetPlayer() then return nil end
  if s and s.pending then return nil end
  local p=Game.GetPlayer();local customization=system("CyberwareCustomizationSystem")
  if not customization then return nil end
  local saved=customization:GetSavedData()
  local raw=ItemID.IsValid(saved.customizationItemID) and Game.GetTransactionSystem():GetItemData(p,saved.customizationItemID) or nil
  local spend=Game.GetInventoryManager():GetSpendByVendorID(visit.manager:GetVendorID())
  s={visit=visit,player=p,playerKey=identity(p),system=customization,inventory=system("UIInventoryScriptableSystem"),
    baseline=saved,raw=raw,enabled=saved.enabled and raw~=nil,primary=saved.primaryColorIndex,secondary=saved.secondaryColorIndex,
    time=0,revision=1,spendCost=math.floor(spend/100),spendCount=spend%100,dirty=false,previewed=false,previewSwitchers={}}
  shared.glamdocSession=s;s.area=G.arm(s) or 1
  return s
end
local function row(s,raw,vendor,available)
  if not has(raw,"Glamdoc") or not G.kind(raw) or ItemID.HasFlag(raw:GetID(),gameEItemIDFlag.Preview) then return nil end
  local ui=UIInventoryItem.Make(vendor and s.visit.vendor or s.player,raw,s.inventory:GetInventoryItemsManager())
  if not ui or ui:IsRecipe() or EquipmentSystem.GetData(s.player):IsSideUpgradeEquipped(raw:GetID()) then return nil end
  return {raw=raw,id=raw:GetID(),key=record(raw:GetID()),ui=ui,name=ui:GetName(),kind=G.kind(raw),vendor=vendor,
    price=vendor and s.visit.manager:GetBuyingPrice(raw:GetID()) or 0,available=available~=false,
    quality=ui:GetQualityInt(),icon=ui:GetIconPath()}
end
function G.rows()
  local s=current();if not s or s.closed then return {} end
  local out,have={},{};local _,items=Game.GetTransactionSystem():GetItemList(s.player)
  for _,raw in ipairs(items or {}) do
    local v=row(s,raw,false,true)
    if v then have[v.key]=true;if v.kind==s.area then out[#out+1]=v end end
  end
  for _,stock in ipairs(s.visit.manager:GetGlamDocItems()) do
    local raw=stock.itemData
    if not have[record(raw:GetID())] and (IsEP1() or not has(raw,"EP1")) then
      local v=row(s,raw,true,VendorItemAdditionalData.Make(stock.itemStack).IsAvailable)
      if v and v.kind==s.area then out[#out+1]=v end
    end
  end
  table.sort(out,function(a,b)
    if a.vendor~=b.vendor then return not a.vendor end
    if a.quality~=b.quality then return a.quality>b.quality end
    if a.name~=b.name then return a.name<b.name end
    return a.key<b.key
  end)
  return out
end
function G.select(v)
  local s=current();if not s or s.closed or s.pending or not v or not G.compatible(v.raw) then return false,"mismatch" end
  local raw=v.vendor and v.raw or owned(s,v.key);if not raw then return false,"missing" end
  if v.vendor and not s.preview then
    s.beforePreview={raw=s.raw,enabled=s.enabled,primary=s.primary,secondary=s.secondary,dirty=s.dirty}
  end
  s.raw=raw;s.enabled=true;s.primary,s.secondary=defaults(raw);s.preview=v.vendor and v or nil
  if not v.vendor then s.beforePreview=nil;s.dirty=true end
  preview(s);s.previewed=true;refresh(s);return true
end
function G.cancelPreview()
  local s=current();if not s or not s.preview or s.pending then return false end
  local before=s.beforePreview
  s.raw=before.raw;s.enabled=before.enabled;s.primary=before.primary;s.secondary=before.secondary;s.dirty=before.dirty
  s.preview=nil;s.beforePreview=nil
  preview(s);s.previewed=true
  refresh(s);return true
end
function G.colors()
  local s=current();if not s or not s.raw or not s.enabled then return nil end
  local brand=CyberwareCustomizationHelper.GetBrandDataFromCustomizationItem(s.raw)
  if not brand or not TDBID.IsValid(brand:GetRecordID()) then return nil end
  local colors={};for i=0,4 do
    local c=CyberwareCustomizationHelper.GetColorFromBrandData(brand,i)
    colors[#colors+1]={c.Red/255,c.Green/255,c.Blue/255}
  end
  return colors
end
function G.color(channel,index)
  local s=current();if not s or s.closed or s.pending or not G.colors() or index<0 or index>4 then return false end
  if channel=="primary" then s.primary=index elseif channel=="secondary" then s.secondary=index else return false end
  if not s.preview then s.dirty=true end
  preview(s);s.previewed=true;refresh(s);return true
end
function G.swap()
  local s=current();if not s or s.pending or not G.colors() then return false end
  s.primary,s.secondary=s.secondary,s.primary
  if not s.preview then s.dirty=true end
  preview(s);s.previewed=true;refresh(s);return true
end
function G.resetColors()
  local s=current();if not s or s.pending or not G.colors() then return false end
  s.primary,s.secondary=defaults(s.raw)
  if not s.preview then s.dirty=true end
  preview(s);s.previewed=true;refresh(s);return true
end
function G.remove()
  local s=current();if not s or s.closed or s.pending or s.preview then return false end
  s.enabled=false;s.dirty=true;preview(s);s.previewed=true;refresh(s);return true
end
function G.quote(v)
  local s=current();if not s or s.closed or s.pending or not v or not v.vendor then return {allowed=false,reason="busy"} end
  if owned(s,v.key) then return {allowed=false,reason="owned"} end
  for _,stock in ipairs(s.visit.manager:GetGlamDocItems()) do
    local raw=stock.itemData
    if record(raw:GetID())==v.key then
      local price=s.visit.manager:GetBuyingPrice(raw:GetID())
      local allowed=VendorItemAdditionalData.Make(stock.itemStack).IsAvailable and (IsEP1() or not has(raw,"EP1"))
      if not allowed then return {allowed=false,reason="requirements",price=price} end
      return {allowed=s.visit.manager:GetLocalPlayerCurrencyAmount()>=price,reason="funds",price=price,raw=raw,
        compatible=G.compatible(raw) and EquipmentSystem.GetData(s.player):IsEquippable(raw)}
    end
  end
  return {allowed=false,reason="missing"}
end
function G.buy(v,applyAfter)
  local s=current();local q=G.quote(v);if not q.allowed then return false,q.reason end
  if applyAfter and not q.compatible then return false,"mismatch" end
  local keep=s.preview and s.preview.key==v.key
  s.pending={key=v.key,price=q.price,at=s.time,primary=keep and s.primary,secondary=keep and s.secondary,
    compatible=q.compatible,applyAfter=applyAfter==true}
  s.visit.manager:BuyItemFromVendor(q.raw,1);s.error=nil;refresh(s);return true
end
local function saveSpend(s)
  Game.GetInventoryManager():SaveSpendByVendorID(s.visit.manager:GetVendorID(),s.spendCost,s.spendCount)
  local data=TweakDBInterface.GetGlamdocRecord(s.visit.manager:GetVendorID())
  if data then
    for i=0,data:GetFactThresholdsCount()-1 do
      local t=data:GetFactThresholdsItem(i)
      if s.spendCost>t:SpendCost() and s.spendCount>t:SpendCount() and Game.GetQuestsSystem():GetFact(t:FactKey())<=i then
        Game.GetQuestsSystem():SetFact(t:FactKey(),i+1)
      end
    end
  end
end
function G.poll(dt)
  local s=current();if not s then return end
  s.time=s.time+math.max(0,dt or 0)
  if identity(Game.GetPlayer())~=s.playerKey then shared.glamdocSession=nil;return end
  if s.pending and s.time>=(s.nextPoll or 0) then
    s.nextPoll=s.time+.2
    local purchase=s.pending;local raw=owned(s,purchase.key)
    if raw then
      s.pending=nil;s.spendCost=s.spendCost+purchase.price;s.spendCount=s.spendCount+1
      Game.GetQuestsSystem():SetFactStr("glam_bought",1);saveSpend(s)
      if not s.closed and purchase.compatible and G.compatible(raw) then
        s.raw=raw;s.enabled=true;s.preview=nil;s.beforePreview=nil;s.dirty=true
        s.primary,s.secondary=defaults(raw)
        if purchase.primary then s.primary=purchase.primary;s.secondary=purchase.secondary end
        preview(s);s.previewed=true
      end
      refresh(s)
      if purchase.applyAfter and not s.closed then G.finish(true) end
    elseif s.time-purchase.at>15 then
      s.pending=nil;s.error="purchase-timeout";refresh(s)
    end
  end
  if s.closed and not s.pending then shared.glamdocLast={accepted=s.accepted,error=s.error};shared.glamdocSession=nil end
end
function G.finish(accept)
  local s=current();if not s or s.closed then return true end
  if accept and (s.pending or s.preview) then return false end
  if accept then
    if s.enabled and (not s.raw or not owned(s,record(s.raw:GetID())) or not G.compatible(s.raw)) then return false end
    if s.dirty then
      local request=SaveAndApplyCyberwareCustomizationDataRequest.new()
      request.enabled=s.enabled;request.customizationItem=s.enabled and s.raw or nil
      request.switcherName=s.raw and CyberwareCustomizationHelper.GetCyberarmTypeFromCustomizationItem(s.raw) or s.baseline.switcherName
      request.switcherIndex=0
      if s.enabled then
        local long=CyberwareCustomizationHelper.GetLongSleevesCustomizationIndexFromCustomizationItem(s.raw)
        request.switcherIndex=(long~=-1 and EquipmentSystemPlayerData.HasLongSleeves(s.player)) and long
          or CyberwareCustomizationHelper.GetCustomizationIndexFromCustomizationItem(s.raw)
      end
      request.primaryColorIndex=s.primary;request.secondaryColorIndex=s.secondary;s.system:QueueRequest(request)
      local equipment=system("EquipmentSystem")
      if s.enabled then
        local equip=EquipRequest.new();equip.owner=s.player;equip.itemID=s.raw:GetID();equip.slotIndex=0;equipment:QueueRequest(equip)
      else
        local unequip=UnequipRequest.new();unequip.owner=s.player;unequip.areaType=gamedataEquipmentArea.ArmsCWStyle
        unequip.slotIndex=0;unequip.force=false;equipment:QueueRequest(unequip)
      end
      local qs=Game.GetQuestsSystem();qs:SetFactStr("glam_equipped",s.enabled and 1 or 0)
      qs:SetFactStr("cyberware_changes_save",1)
      if s.primary~=s.baseline.primaryColorIndex or s.secondary~=s.baseline.secondaryColorIndex
        or (s.raw and s.raw:GetID()~=s.baseline.customizationItemID) then qs:SetFactStr("glamdoc_color",1) end
      s.previewed=false
    else restore(s) end
    s.accepted=true
  else restore(s) end
  saveSpend(s);s.closed=true;return true
end
function G.current() return current() end
function G.bind(state) shared=state end
function G.state()
  local s=current();return s and {active=not s.closed,area=s.area,preview=s.preview~=nil,pending=s.pending~=nil,dirty=s.dirty,
    primary=s.primary,secondary=s.secondary,error=s.error} or shared and shared.glamdocLast or {active=false}
end
return G
