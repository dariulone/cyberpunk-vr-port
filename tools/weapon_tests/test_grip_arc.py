"""Small deterministic checks of the production cant-relief detector (not pose-data generation)."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
lua=LuaRuntime(unpack_returned_tuples=True)
lua.globals().A=lua.execute((ROOT/'mods/cet/CyberpunkVRPort_Reload/reload/grip_arc.lua').read_text(encoding='utf-8'))
lua.execute('''
local close=function(a,b)assert(math.abs(a-b)<1e-6,tostring(a).." != "..tostring(b))end
local q=math.sqrt(.5)
-- A 100 mm wrist offset rolled 90 degrees: either endpoint misses a halfway hand by 76.5 mm.
-- The existing 50 mm radius therefore failed although the hand lies exactly on the allowed arc.
local theta=A.closest(0,1,0,math.pi/2,.1,0,0,.1*q,0,-.1*q)
close(theta,math.pi/4)
local oldDistance=math.sqrt((.1-.1*q)^2+(.1*q)^2)
assert(oldDistance>.05)
close(A.closest(0,1,0,-math.pi/2,.1,0,0,.1*q,0,.1*q),-math.pi/4)
close(A.closest(0,1,0,math.pi/2,.1,0,0,-.1,0,0),math.pi/2)
close(A.closest(0,1,0,math.pi/2,.1,0,0,.1,0,.1),0)
-- Translation along the slide axis does not rotate the grip; distance still rejects remote wrists downstream.
close(A.closest(0,1,0,math.pi/2,.1,2,0,.1*q,9,-.1*q),math.pi/4)
close(A.closest(0,1,0,math.pi/2,0,1,0,.1,0,0),0)
close(A.closest(0,1,0,0,.1,0,0,0,0,-.1),0)
-- The opposite atan2 representation must not choose the wrong end around a half turn.
local e=1e-4
close(A.closest(0,1,0,math.pi,.1,0,0,-.1,0,e),math.pi)
''')
print('PASS: rolled/upright midpoint, both hands, endpoints, axial and wrap cases')
