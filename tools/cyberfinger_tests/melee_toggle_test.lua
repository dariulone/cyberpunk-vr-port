local requests={}
local types={fists='fists',sword='melee',knife='melee',gun='gun',arms='cyberware'}
local slots,baseFists={'gun','sword','knife'},{'fists'}
local paused,blocked=false,false
local active='sword'
local sceneTier,mounted,swimming,device=1,false,0,false
local player={GetEntityID=function() return {hash=1} end,
  GetActiveWeapon=function() return active and {GetItemID=function() return active end} end}
ItemID={IsValid=function(id) return types[id]~=nil end,GetCombinedHash=function(id) return id end}
WeaponObject={IsOfType=function(id,kind) return types[id]==kind end,
  IsMelee=function(id) return types[id]=='melee' or types[id]=='fists' or types[id]=='cyberware' end,
  IsCyberwareWeapon=function(id) return types[id]=='cyberware' end}
gamedataItemType={Wea_Fists='fists'}
gamedataEquipmentArea={Weapon='weapons',BaseFists='fists'}
EquipmentSystem={GetItemsInArea=function(_,area) return area=='weapons' and slots or baseFists end}
CName={new=function(s) return s end}
StatusEffectSystem={ObjectHasStatusEffectWithTag=function() return blocked end}
DrawItemRequest={new=function() return {} end}
Game={GetSystemRequestsHandler=function() return {IsGamePaused=function() return paused end} end,
  GetBlackboardSystem=function() return {GetLocalInstanced=function() return {
    GetInt=function(_,k) return k=='scene' and sceneTier or swimming end,
    GetBool=function(_,k) return k=='mounted' and mounted or (k=='device' and device) end} end} end,
  GetScriptableSystemsContainer=function() return {Get=function(_,name)
    assert(name=='EquipmentSystem');return {QueueRequest=function(_,request) requests[#requests+1]=request end}
  end} end}
function GetAllBlackboardDefs() return {PlayerStateMachine={SceneTier='scene',Swimming='swim',MountedToVehicle='mounted',IsUIZoomDevice='device'}} end
local module=dofile(assert(arg[1]))
local function toggle(wanted)
  local before=#requests
  local ok=module.toggle(player)
  assert(ok==(wanted~=nil),'unexpected toggle availability')
  assert(#requests==before+(wanted and 1 or 0),'wrong number of equipment requests')
  if wanted then assert(requests[#requests].itemID==wanted and requests[#requests].owner==player,'wrong equipment target') end
end
toggle('fists');active='fists';toggle('sword')
active='knife';module.observe(player);active='fists';toggle('knife')
-- A removed last-used melee item is replaced only by another equipped melee item.
slots={'gun','sword','arms'};toggle('sword')
slots={'gun','arms'};toggle(nil)
active='gun';toggle(nil);active='arms';toggle(nil);active=nil;toggle(nil)
slots={'sword'};active='sword';baseFists={};toggle(nil)
baseFists={'arms'};toggle(nil) -- cyberware never substitutes for bare fists
baseFists={'fists'};paused=true;toggle(nil);paused=false;blocked=true;toggle(nil);blocked=false
sceneTier=2;toggle(nil);sceneTier=1;mounted=true;toggle(nil);mounted=false
swimming=1;toggle(nil);swimming=0;device=true;toggle(nil);device=false
slots={'gun'};toggle(nil) -- scripted weapon outside the loadout is left alone
slots={'sword','knife'};active='knife';module.observe(player);module.observe(nil)
active='fists';toggle('sword') -- new player/session cannot inherit the old remembered knife
print('CyberFinger melee/fists equipment toggle passed')
