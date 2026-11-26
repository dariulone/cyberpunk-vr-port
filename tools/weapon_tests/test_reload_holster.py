"""Exercise production heldWeapon resolution, including the formerly stale entity."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

source = (Path(__file__).resolve().parents[2] /
          'mods/cet/CyberpunkVRPort_Reload/init.lua').read_text(encoding='utf-8')
block = source[source.index('local lastW, wMiss'):source.index('-- ---- from init.lua 390')]
lua = LuaRuntime()
lua.execute('''
gun = {}; active = gun; right = gun; left = nil; slotError = false
player = {}
function activeWeapon() return active end
function worldPos() return {} end -- a despawn-pending weapon still answers!
TweakDBID = {new = function(s) return s end}
local transaction = {GetItemInSlot = function(_, _, id)
    if slotError then error('attachment API temporarily unavailable') end
    return id == 'AttachmentSlots.WeaponRight' and right or left
end}
Game = {GetPlayer = function() return player end,
        GetTransactionSystem = function() return transaction end}
''' + block + '''
assert(heldWeapon() == gun and heldConfirmed)
active = nil
assert(heldWeapon() == gun and heldConfirmed, 'transient active-weapon miss flickers')
right = nil
assert(heldWeapon() == nil and not heldConfirmed, 'holstered entity survived by world position')
left = gun
assert(heldWeapon() == gun and heldConfirmed, 'left-handed weapon was lost')
left = nil; slotError = true
assert(heldWeapon() == gun and not heldConfirmed, 'API failure must not light a stale weapon')
player = nil
assert(heldWeapon() == nil and not heldConfirmed, 'unloaded player retained weapon')
''')
print('PASS reload attachment fallback, immediate holster, left hand, API miss and unload')
