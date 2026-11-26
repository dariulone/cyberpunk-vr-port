"""Execute the production Lua hinge adapter without a game or native pose writes."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().source_root=str(ROOT/'mods/cet/CyberpunkVRPort_Optics').replace('\\','/')
lua.execute('''
package.path=source_root..'/?.lua;'..package.path
function registerForEvent()end
CName={new=function(s)return s end}
calls={};accepted=true
Game={VRPhysicalOpticsHinge=function(weapon,enabled)calls[#calls+1]={weapon=weapon,enabled=enabled};return accepted end}
function VRRigWriteAbs()error('must not consume shared reload write slots')end
function VRRigWriteClear()error('must not clear reload ownership')end
function upvalue(fn,wanted)
 for i=1,100 do local n,v=debug.getupvalue(fn,i);if not n then break end;if n==wanted then return v end end
 error('missing upvalue '..wanted)
end
''')
lua.globals().mod=lua.execute((ROOT/'mods/cet/CyberpunkVRPort_Optics/init.lua').read_text(encoding='utf-8'))
lua.execute('''
local hinge=upvalue(upvalue(mod.update,'updateSightZoom'),'hinge');local state=upvalue(hinge,'state')
local weapon={id=42};state.profile={key='osprey'};state.opticSlot={}
hinge(weapon,false);assert(#calls==0)
hinge(weapon,true);assert(#calls==1 and calls[1].weapon==weapon and calls[1].enabled and state.hingeRequested)
hinge(weapon,true);assert(#calls==2,'active request must refresh native freshness')
hinge(nil,false);assert(#calls==3 and not calls[3].enabled and calls[3].weapon==nil and not state.hingeRequested)
hinge(nil,false);assert(#calls==3,'inactive frames should not call native')
state.profile={key='rasetsu'};hinge(weapon,true);assert(#calls==3,'other scope was changed')
state.profile={key='osprey'};state.opticSlot=nil;hinge(weapon,true);assert(#calls==3,'unavailable slot was accepted')
state.opticSlot={};accepted=false;hinge(weapon,true);assert(not state.hingeRequested and mod.hingeError,'native refusal hidden')
accepted=true;hinge(weapon,true)
local restore=upvalue(upvalue(mod.update,'scan'),'restoreWeapon');restore()
assert(not state.hingeRequested and not calls[#calls].enabled,'holster did not release owned request')
''')
print('PASS production hinge adapter: exact scope/ADS gating, refresh, native refusal, inactive cost and owned holster release')
