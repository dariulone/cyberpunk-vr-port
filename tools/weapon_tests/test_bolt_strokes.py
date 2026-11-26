"""Production Grad stroke selection and Malorian dry/tactical magazine seating."""
from pathlib import Path
from lupa import LuaRuntime

root = Path(__file__).resolve().parents[2]
folder = root / 'mods/cet/CyberpunkVRPort_Reload/reload'
source = (folder / 'reload.lua').read_text()
lua = LuaRuntime()
lua.globals().grad = lua.execute((folder / 'weapons/techtronika_grad.lua').read_text())
lua.globals().malorian = lua.execute((folder / 'weapons/malorian_silverhand.lua').read_text())
start = source.index('local function slideStroke(')
helper = source[start:source.index('\nend', start) + 4]
start = source.index('                    local pull = (hgx - slideS.hrx)')
pull = source[start:source.index('\n', source.index('local rack =', start))]
lua.execute(helper + '''
local state={fired=true};local mag={state='in'}
local shot=slideStroke(grad.slide,state,mag)
assert(shot.travel==.146 and shot.sign==-1 and shot.pullDirection==-1)
assert(shot.slot==grad.slide.slot and shot.riders==grad.slide.riders)
state.activeStroke=shot;state.grabbed=true;state.fired=false
assert(slideStroke(grad.slide,state,mag)==shot,'ejection reversed a held bolt')
state.grabbed=false;state.releasing=true
assert(slideStroke(grad.slide,state,mag)==shot,'spring reversed a held bolt')
state.releasing=false;state.drain=.1
assert(slideStroke(grad.slide,state,mag)==shot)
state.drain=0
assert(slideStroke(grad.slide,state,mag)==grad.slide)
state.fired=true;mag.pending=4
local load=slideStroke(grad.slide,state,mag)
assert(load==grad.slide and load.travel==.108 and load.sign==1)
-- Actual controller displacement expression: forward for a fired case,
-- backward for the empty-magazine loading stroke.
local function rackFor(sc,hgx)
    local hgy,hgz,ax,ay,az=0,0,1,0,0
    local slideS={hrx=0,hry=0,hrz=0}
''' + pull + '''
    return rack
end
assert(rackFor(shot,.1)>.09 and rackFor(shot,-.1)<0)
assert(rackFor(load,-.1)>.09 and rackFor(load,.1)<0)
''')

start = source.index('                local rack = (mc.chamberOnRack == true)')
end = source.index('                magS.carried = nil', start)
seat = source[start:end]
lua.execute('''
mc=malorian.mag;slideS={locked=false};weapon={};loaded=0
function giveReserve(_,n) reserve=reserve+n end
function reserveAmmo() return reserve end
function magCapacity() return 10 end
function setMagAmmo(_,n) loaded=n;reserve=reserve-n end
''')
tick = lua.eval('function() ' + seat + ' end')
for dry in (False, True):
    lua.execute('magS={dry=' + str(dry).lower() + '};reserve=17;loaded=0')
    tick()
    g = lua.globals()
    if dry:
        assert g.magS.pending == 10 and g.loaded == 0 and g.reserve == 17
    else:
        assert g.magS.pending is None and g.loaded == 10 and g.reserve == 7
lua.execute('magS={dry=true};reserve=3;loaded=0')
tick()
assert lua.globals().magS.pending == 3
print('PASS: Grad forward shot cycle, backward dry reload, stable held stroke; Malorian dry/tactical ammo accounting.')
