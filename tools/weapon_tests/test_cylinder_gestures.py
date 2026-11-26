"""Exercise production closure and cylinder interaction with tracked hand samples."""
from pathlib import Path
from lupa import LuaRuntime

root = Path(__file__).resolve().parents[2]
source = (root / 'mods/cet/CyberpunkVRPort_Reload/reload/reload.lua').read_text()
start = source.index('local function cylinderLateralFlick(')
flick = source[start:source.index('\nend', start) + 4]
lua = LuaRuntime()
lua.execute(flick + '''
local function motion(x,y,z,valid) return {x=x,y=y,z=z,w=valid or 1} end
for _,fps in ipairs({30,60,90,120}) do
    local dt=1/fps
    for _,direction in ipairs({-1,1}) do
        local state={}
        -- Tilting/returning with no lateral translation must never latch.
        for i=1,fps do
            assert(not cylinderLateralFlick(state,motion(0,0,0),1,0,0,dt,3))
            assert(not cylinderLateralFlick(state,motion(.3,0,0),1,0,0,dt,3))
        end
        -- Fast movement along the bore also leaves the cylinder open.
        assert(not cylinderLateralFlick(state,motion(0,8,0),1,0,0,dt,3))
        -- One tracking spike and a reversal are not a continuous swing.
        assert(not cylinderLateralFlick(state,motion(direction*4,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(-direction*4,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,nil,1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(direction*4,0,0),1,0,0,dt,3))
        assert(cylinderLateralFlick(state,motion(direction*4,0,0),1,0,0,dt,3))
        -- A stalled frame or lost tracking clears the partial gesture.
        assert(not cylinderLateralFlick(state,motion(4,0,0),1,0,0,.2,3))
        assert(not cylinderLateralFlick(state,motion(4,0,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(4,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(0/0,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(math.huge,0,0),1,0,0,dt,3))
        assert(not cylinderLateralFlick(state,motion(4,0,0),1,0,0,0/0,3))
    end
end
''')

start = source.index('    local sc2 = cfg.spin')
end = source.index('    -- ------------------------------------------------------------------ THE HAMMER', start)
lua.execute('''
cfg={spin={which=1,bone=8,axis={0,0,1},whenShut=true}}
hatchS={p=0,open=false}
spinS={ang=120,w=600,det=2,wrote=true,on=true,pp={},hand=1,faded=true}
dt=1/90;writes=0;fades=0
slotComp={GetSlotTransform=function() error('Locked cylinder offered a hand interaction') end}
VRRigWriteRot=function(which,bone,angle)
    assert(which==1 and bone==8 and angle==0);writes=writes+1
end
fadeOut=function() fades=fades+1 end
playSnd=function() error('Locked cylinder made a spin sound') end
''')
tick = lua.eval('function() ' + source[start:end] + ' end')
for _ in range(20):
    tick()
lua.execute('''
assert(spinS.ang==0 and spinS.w==0 and not spinS.wrote and not spinS.on)
assert(spinS.pp==nil and spinS.hand==nil and spinS.faded==nil)
assert(writes==1 and fades==1)
-- Closing starts before the crane has reached its latch: no new grip allowed.
hatchS.p=.95;hatchS.open=false
''')
tick()
print('PASS: lateral closure at 30/60/90/120 Hz; tilt, jitter and tracking-loss rejection; locked cylinder and pose release.')
