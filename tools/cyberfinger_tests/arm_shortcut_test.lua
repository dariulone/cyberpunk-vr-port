-- The stock B shortcut must not steal CyberFinger's eject/back button.
local callbacks, enabled, down, guarded, queued = {}, true, false, false, 0
local player = {
  VRArmBSetInputGuard = function(_, value) guarded = value end,
  GetActiveWeapon = function() return nil end,
}
local equipment = {
  GetPlayerData = function() return {GetActiveItem = function() return 'arms' end} end,
  QueueRequest = function() queued = queued + 1 end,
}
Game = {
  GetPlayer = function() return player end,
  GetScriptableSystemsContainer = function() return {Get = function() return equipment end} end,
}
CName = {new = function(value) return value end}
ItemID = {IsValid = function() return true end}
gamedataEquipmentArea = {ArmsCW = 'arms'}
gamedataItemType = {Cyb_MantisBlades = 'blades', Cyb_Launcher = 'launcher'}
WeaponObject = {IsOfType = function(_, kind) return kind == 'blades' end}
EquipmentManipulationAction = {RequestActiveMeleeware = 'equip'}
NewObject = function() return {} end
VRCyberFingerEnabled = function() return enabled end
GetVRSharedSlot = function(slot) return slot == 157 and (down and 1 or 0) or 0 end
registerForEvent = function(name, fn) callbacks[name] = fn end
spdlog = {error = function(message) error(message) end}
assert(loadfile(arg[1]))()
callbacks.onInit()
callbacks.onUpdate(.1); down = true; callbacks.onUpdate(.1)
assert(not guarded and queued == 0, 'CyberFinger B was stolen by arm-cyberware shortcut')
enabled = false; callbacks.onUpdate(.1)
assert(guarded and queued == 0, 'switching controllers with B held caused an equip')
down = false; callbacks.onUpdate(.1); down = true; callbacks.onUpdate(.1)
assert(guarded and queued == 1, 'stock-controller fresh B press no longer equips cyberware')
enabled = true; callbacks.onUpdate(.1)
assert(not guarded and queued == 1, 'CyberFinger takeover retained stock input guard')
print('PASS CyberFinger exclusion and stock arm-cyberware B shortcut')
