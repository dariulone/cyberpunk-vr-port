"""Replay grip edge cases through the production Lua gesture state machine."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

root = Path(__file__).resolve().parents[2]
lua = LuaRuntime(unpack_returned_tuples=True)
lua.globals().gesture = lua.execute((root / 'mods/cet/CyberpunkVRPort_Driving/vehicle_gesture.lua').read_text())
lua.execute('''
local function point(x) return {x=x,y=0,z=0} end
local target={part=0}
local s=gesture.new()
-- A held grip on load/menu exit cannot acquire anything until released.
assert(not gesture.step(s,0,true,target,point(0)))
gesture.step(s,.1,false,nil,nil)
local act,busy=gesture.step(s,.2,true,target,point(0));assert(not act and busy)
assert(not gesture.step(s,.3,true,nil,point(.02)))
assert(gesture.step(s,.4,true,nil,point(.06)))
assert(not gesture.step(s,.5,true,nil,point(.12)))
assert(not gesture.step(s,.6,true,nil,point(0)))
gesture.step(s,.7,false,nil,nil)
gesture.step(s,.8,true,target,point(0))
assert(gesture.step(s,.91,true,nil,point(-.06))) -- closing push
gesture.step(s,1,false,nil,nil)
gesture.step(s,1.1,true,target,point(0))
assert(not gesture.step(s,1.2,true,nil,point(.5))) -- teleport
assert(not gesture.step(s,1.3,true,nil,point(.06)))
gesture.step(s,2,false,nil,nil)
gesture.step(s,2.1,true,nil,nil)
assert(not select(2,gesture.step(s,3,true,target,point(0)))) -- stale press
gesture.step(s,4,false,nil,nil)
gesture.step(s,4.1,true,target,point(0))
assert(not gesture.step(s,4.2,true,nil,nil)) -- loss of pose consumes grip
assert(not gesture.step(s,4.3,true,nil,point(.06)))
gesture.step(s,5,false,nil,nil)
gesture.step(s,5.1,true,target,point(0))
assert(not gesture.step(s,7.2,true,nil,point(.06))) -- timed out
''')
print('panel gesture: release, pull/push, once per press, tracking loss, teleport and timeout PASS')
