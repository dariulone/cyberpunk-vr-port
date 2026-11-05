-- The backpack: what the game's own backpack screen lists (UI/fullscreen/backpack/backpack_main.swift
-- PopulateInventory), its crafting components, and the actions its hold buttons do -- favourite (weapons only),
-- drop, disassemble, disassemble the junk.
local I=require("modules/inventory_data")
local Images=require("modules/gameplay_data")
local B={}
-- The filter buttons, in the game's order; a category is offered only while an item of it is in the backpack.
B.FILTERS={"AllItems","RangedWeapons","MeleeWeapons","Clothes","Consumables","Grenades","Attachments","SoftwareMods","Programs","Cyberware","Junk","Quest"}
-- Made on first use: the module loads before CET has the game's types (CName, the enums).
local CATEGORY,HIDDEN
local function prepare()
  if CATEGORY then return end
  CATEGORY={}
  for _,name in ipairs(B.FILTERS) do
    local ok,value=pcall(function() return EnumInt(ItemFilterCategory[name]) end)
    if ok then CATEGORY[tostring(value)]=name end
  end
  HIDDEN={CName.new("HideInBackpackUI"),CName.new("SoftwareShard")}
end
local function player() return Game.GetPlayer() end
local function inventory() return Game.GetScriptableSystemsContainer():Get(CName.new("UIInventoryScriptableSystem")) end
-- the favourites themselves: a UI item's cached flag lags a change made while it is alive
local function uiSystem() return Game.GetScriptableSystemsContainer():Get(CName.new("UIScriptableSystem")) end
function B.isFavorite(id) return uiSystem():IsItemPlayerFavourite(id) end
local function hidden(raw)
  for _,tag in ipairs(HIDDEN) do if raw:HasTag(tag) then return true end end
  return false
end
-- One grid entry from the native UI item: its cached metadata only, no InventoryItemData conversion (700+ items).
local function item(ui,raw)
  local id=ui:GetID()
  local v={id=id,key=I.key(id),ui=ui,raw=raw,name=ui:GetName(),quality=NameToString(ui:GetQualityName()),
    qualityInt=ui:GetQualityInt(),type=ui:GetItemType(),equipped=ui:IsEquipped(),new=ui:IsNew(),iconic=ui:IsIconic(),
    favorite=B.isFavorite(id),quantity=ui:GetQuantity(true),junk=ui:IsJunk(),quest=ui:IsQuestItem(),weapon=ui:IsWeapon()}
  local icon=ui:GetIconPath()
  if icon and icon~="" and icon~="None" then v.image=Images.image(TweakDBID.new(icon)) end
  v.category=v.quest and "Quest" or CATEGORY[tostring(EnumInt(ui:GetFilterCategory()))]
  if v.weapon then
    v.melee=raw:HasTag(CName.new("MeleeWeapon"))
    v.ammo=v.melee and 0 or ui:GetAmmo()
    v.typeName=GetLocalizedText(UIItemsHelper.GetItemTypeKey(v.type))
    v.typeIcon=Images.image(TweakDBID.new(NameToString(UIItemsHelper.GetWeaponTypeIcon(v.type))))
  end
  return v
end
-- The snapshot as a job producer (modules/jobs.lua): a slice of the transaction list a step.
function B.snapshotJob()
  local items,i,out,inv
  return function()
    if not items then
      prepare()
      local _;_,items=Game.GetTransactionSystem():GetItemList(player())
      items=items or {};i=0;out={list={},byKey={},junk={},categories={}};inv=inventory();inv:FlushTempData()
      return false
    end
    local last=math.min(#items,i+60)
    while i<last do
      i=i+1
      local raw=items[i];local id=raw:GetID()
      if not ItemID.HasFlag(id,gameEItemIDFlag.Preview) and not hidden(raw) then
        local ui=inv:GetPlayerItem(id)
        if ui then
          local v=item(ui,raw)
          out.list[#out.list+1]=v;out.byKey[v.key]=v
          if v.junk then out.junk[#out.junk+1]=v end
          if v.category then out.categories[v.category]=true end
        end
      end
    end
    if i<#items then return false end
    return true,out,#out.list
  end
end
function B.filtered(snapshot,filter)
  if filter=="AllItems" then return snapshot.list end
  local out={}
  for _,v in ipairs(snapshot.list) do if v.category==filter then out[#out+1]=v end end
  return out
end
-- The two component grids of the game's backpack: name, icon and how many the player has.
function B.materials()
  local p=player();local ts=Game.GetTransactionSystem();local out={}
  for _,set in ipairs({{"common",UIInventoryHelper.GetCommonCraftingMaterials()},{"hacking",UIInventoryHelper.GetHackingCraftingMaterials()}}) do
    local list={}
    for _,tdb in ipairs(set[2]) do
      local rec=TweakDBInterface.GetItemRecord(tdb)
      local icon=UIInventoryItemsManager.ResolveItemIconName(tdb,rec,false)
      list[#list+1]={id=tdb,name=GetLocalizedItemNameByCName(rec:DisplayName()),image=icon and Images.image(TweakDBID.new(icon)) or nil,
        quantity=ts:GetItemQuantity(p,ItemID.CreateQuery(tdb))}
    end
    out[set[1]]=list
  end
  return out
end
-- What the selected item allows, read again at click time from the live item (backpack_main.swift
-- NewShowItemHints / OnItemDisplayClick / OnItemDisplayHold).
function B.actions(v)
  local p=player();local raw=v and Game.GetTransactionSystem():GetItemData(p,v.id)
  if not raw then return {} end
  local ui=inventory():GetPlayerItem(v.id) or v.ui
  local blocked=raw:HasTag(CName.new("UnequipBlocked"))
  local equipped=EquipmentSystem.GetInstance(p):IsEquipped(p,v.id)
  local favorite=B.isFavorite(v.id)
  local inCar=false
  pcall(function() inCar=p:GetPlayerStateMachineBlackboard():GetInt(GetAllBlackboardDefs().PlayerStateMachine.Vehicle)~=0 end)
  local count=Game.GetTransactionSystem():GetItemQuantity(p,v.id)
  local a={favorite=favorite,canFavorite=ui:IsWeapon(),max=math.max(1,count),iconic=ui:IsIconic()}
  a.drop=not equipped and not blocked and not favorite and not inCar and not ui:IsQuestItem() and not ui:IsIconic()
    and not raw:HasTag(CName.new("Quest")) and RPGManager.CanItemBeDropped(p,raw) and InventoryGPRestrictionHelper.CanDrop(ui,p)
    and ItemActionsHelper.GetDropAction(v.id)~=nil
  a.disassemble=not equipped and not blocked and not favorite and ItemActionsHelper.GetDisassembleAction(v.id)~=nil
  return a
end
function B.setFavorite(v,on)
  local request=UIScriptableSystemSetItemPlayerFavourite.new();request.itemID=v.id;request.favourite=on==true
  uiSystem():QueueRequest(request)
  return true
end
function B.drop(v,quantity)
  local a=B.actions(v);if not a.drop then return false end
  quantity=math.max(1,math.min(a.max,math.floor(quantity or 1)))
  if quantity==1 then ItemActionsHelper.DropItem(player(),v.id)
  else
    local params=ItemModParams.new();params.itemID=v.id;params.quantity=quantity
    RPGManager.DropManyItems(player(),{params})
  end
  return true
end
function B.disassemble(v,quantity)
  local a=B.actions(v);if not a.disassemble then return false end
  quantity=math.max(1,math.min(a.max,math.floor(quantity or 1)))
  ItemActionsHelper.DisassembleItem(player(),v.id,quantity)
  return true
end
-- A batch picked in the backpack's selection mode, whole stacks. Each item is read again as B.actions does for one;
-- what it does not allow is left where it is and counted. Iconic weapons stay out of a batch disassembly (one at a
-- time they ask first). Drops go in one DropManyItems call, as the game drops a stack.
function B.dropMany(list)
  local params,items,skipped={},0,0
  for _,v in ipairs(list or {}) do
    local a=B.actions(v)
    if a.drop then
      local p=ItemModParams.new();p.itemID=v.id;p.quantity=a.max;params[#params+1]=p;items=items+a.max
    else skipped=skipped+1 end
  end
  -- (CET fills the GameInstance parameter itself)
  if #params>0 then RPGManager.DropManyItems(player(),params) end
  return items,skipped
end
function B.disassembleMany(list)
  local items,skipped=0,0
  for _,v in ipairs(list or {}) do
    local a=B.actions(v)
    if a.disassemble and not a.iconic then
      ItemActionsHelper.DisassembleItem(player(),v.id,a.max);items=items+a.max
    else skipped=skipped+1 end
  end
  return items,skipped
end
-- Every junk item, whole stacks (backpack_main.swift OnDisassembleJunkPopupClosed).
function B.disassembleJunk(snapshot)
  local n=0
  for _,v in ipairs(snapshot and snapshot.junk or {}) do
    local count=Game.GetTransactionSystem():GetItemQuantity(player(),v.id)
    if count>0 then ItemActionsHelper.DisassembleItem(player(),v.id,count);n=n+count end
  end
  return n
end
function B.junkCount(snapshot)
  local n=0;for _,v in ipairs(snapshot and snapshot.junk or {}) do n=n+v.quantity end
  return n
end
return B
