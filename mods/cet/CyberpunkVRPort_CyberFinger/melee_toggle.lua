-- Only loadout melee weapons and the BaseFists item. Never use the game's
-- general weapon cycle/last-weapon request: either can choose a firearm.
local M={}
local owner,lastMelee
local function key(id) return tostring(ItemID.GetCombinedHash(id)) end
local function fists(id) return ItemID.IsValid(id) and WeaponObject.IsOfType(id,gamedataItemType.Wea_Fists) end
local function melee(id)
  return ItemID.IsValid(id) and WeaponObject.IsMelee(id) and not fists(id) and not WeaponObject.IsCyberwareWeapon(id)
end
function M.observe(player)
  local current=player and tostring(player:GetEntityID().hash)
  if current~=owner then owner,lastMelee=current,nil end
  if not player then return end
  local weapon=player:GetActiveWeapon()
  local id=weapon and weapon:GetItemID()
  if id and melee(id) then lastMelee=key(id) end
end
function M.toggle(player)
  if not player then return false end
  M.observe(player)
  if Game.GetSystemRequestsHandler():IsGamePaused() then return false end
  local defs=GetAllBlackboardDefs().PlayerStateMachine
  local state=Game.GetBlackboardSystem():GetLocalInstanced(player:GetEntityID(),defs)
  if not state or state:GetInt(defs.SceneTier)>1 or state:GetInt(defs.Swimming)>0 or
      state:GetBool(defs.MountedToVehicle) or state:GetBool(defs.IsUIZoomDevice) then return false end
  for _,tag in ipairs({'NoWeapons','NoCombat','NoWeaponWheel','Melee','Fists',
      'FirearmsNoSwitch','FirearmsNoUnequipNoSwitch','ShootingRangeCompetition'}) do
    if StatusEffectSystem.ObjectHasStatusEffectWithTag(player,CName.new(tag)) then return false end
  end
  local weapon=player:GetActiveWeapon()
  local active=weapon and weapon:GetItemID()
  if not active or not (fists(active) or melee(active)) then return false end
  local target
  if fists(active) then
    for _,id in ipairs(EquipmentSystem.GetItemsInArea(player,gamedataEquipmentArea.Weapon) or {}) do
      if melee(id) then
        target=target or id
        if key(id)==lastMelee then target=id;break end
      end
    end
  else
    -- Only switch from an ordinary weapon still equipped in the loadout.
    local equipped=false
    for _,id in ipairs(EquipmentSystem.GetItemsInArea(player,gamedataEquipmentArea.Weapon) or {}) do
      if key(id)==key(active) then equipped=true;break end
    end
    if not equipped then return false end
    for _,id in ipairs(EquipmentSystem.GetItemsInArea(player,gamedataEquipmentArea.BaseFists) or {}) do
      if fists(id) then target=id;break end
    end
  end
  if not target then return false end -- no melee slot/fists: no gun fallback
  local request=DrawItemRequest.new()
  request.owner=player;request.itemID=target
  Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(request)
  return true
end
return M
