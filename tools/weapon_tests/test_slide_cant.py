"""Run the production slide preview/grab block with a canted pistol and bounded mock inputs."""
from pathlib import Path
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
s=(ROOT/'mods/cet/CyberpunkVRPort_Reload/reload/reload.lua').read_text(encoding='utf-8')
arc=(ROOT/'mods/cet/CyberpunkVRPort_Reload/reload/grip_arc.lua').read_text(encoding='utf-8')
def extract(a,b):
 start=s.index(a);return s[start:s.index(b,start)]
mathcode=extract('local function qrot(',"-- THE OTHER HAND'S GRIP.")
mathcode+=chr(10)+extract('local function slideStroke(', '-- Capture a stable CLOSED crane')
basis=extract('local function basisQuat(', 'local function playerFrame()')
block=extract('            if not gripOf(free) then magS.gripLock = false end','            if slideS.releasing then')
setup=r'''
local M={gripArc=ARC}
local gWeapon=true
local ax,ay,az=0,1,0
-- Weapon rolled +90 degrees around its bore.
local gFx,gFy,gFz=0,0,-1
local gDx,gDy,gDz=0,1,0
local gSx,gSy,gSz=1,0,0
local spx,spy,spz=0,0,0
local g={off={.1,0,0},rot={0,0,0,1},pose='fixture'}
local sc={grips={behind=g},previewRadius=.05,styleAngleMax=60}
local cfg={slide=sc}
local free=0
local slideS={grabbed=false,releasing=false},magS
magS={state='in',gripLock=false,snapD=-1}
local pressed=true
local hq={i=0,j=math.sin(math.pi/8),k=0,r=math.cos(math.pi/8)}
local hx,hy,hz=.1/math.sqrt(2),0,-.1/math.sqrt(2)
local marks,driven=0,0
local function len3(x,y,z)return math.sqrt(x*x+y*y+z*z)end
local function gripOf()return pressed end
local function VRHandRawRot()return hq end
local function handGrips(recorded)return {recorded}end
local function gripTarget(grip)return qrot(0,math.sqrt(.5),0,math.sqrt(.5),unpack(grip.off))end
local function fadeIn()end
local function fadeOut()end
local function aimHand()return 0 end
local function w0IsEmpty()return false end
local function driveSlide()driven=driven+1 end
local function pushAnimEvent()end
M.reachMark=function()marks=marks+1 end
M.squeeze=function()return pressed end
'''
# Lua declarations are deliberately separate: assignment in a multi-local declaration differs between versions.
setup=setup.replace('local slideS={grabbed=false,releasing=false},magS','local slideS={grabbed=false,releasing=false}\nlocal magS')
checks=r'''
assert(slideS.grabbed and driven==1 and marks==1,'half-canted slide not acquired')
assert(slideS.grip==g and slideS.hand==0,'authored hold or working hand changed')
assert(magS.gripLock,'squeeze was not consumed')
'''
L=LuaRuntime();L.globals().ARC=L.execute(arc)
L.execute(mathcode+basis+setup+block+checks)
# Negative controls use the same production detector: a closer magazine and a remote hand must not be stolen.
for setup2 in (setup.replace("snapD=-1","snapD=0"),setup.replace('hx,hy,hz=.1/math.sqrt(2),0,-.1/math.sqrt(2)','hx,hy,hz=1,0,-1')):
 L=LuaRuntime();L.globals().ARC=L.execute(arc)
 L.execute(mathcode+basis+setup2+block+"assert(not slideS.grabbed and driven==0,'slide stole an invalid reach')")
print('PASS production slide: intermediate cant latches; magazine priority and reach radius retained')
