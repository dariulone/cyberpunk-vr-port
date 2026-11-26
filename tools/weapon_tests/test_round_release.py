"""Run the production end-of-ejection/chambering branches for Malorian rounds."""
from pathlib import Path
from lupa import LuaRuntime

folder = Path(__file__).resolve().parents[2] / 'mods/cet/CyberpunkVRPort_Reload/reload'
source = (folder / 'reload.lua').read_text()
start = source.index('    local function releaseRoundPose(cr)')
helper = source[start:source.index('\n    end', start) + len('\n    end')]
lua = LuaRuntime()
lua.globals().cfg = lua.execute((folder / 'weapons/malorian_silverhand.lua').read_text())
lua.execute('''
sc=cfg.slide;slideS={};writes={};dt=1
function VRRigWriteOff(which,bone) writes[#writes+1]={'off',bone} end
function VRRigWriteScale(which,bone,scale) writes[#writes+1]={'scale',bone,scale} end
function VRRigWrite(which,bone,x,y,z) writes[#writes+1]={'offset',bone,x,y,z} end
function VRRigWriteRot() error('A finished round must release its rotation') end
function showSeated(scale) slideS.seatedShown=scale>.5 end
''')
lua.execute(helper + '\nrelease=releaseRoundPose')
start = source.index('    do\n        local cr = sc.chamberRound\n        if slideS.ejectT')
end = source.index('    -- THE CHAMBERING CHASE', start)
eject = lua.eval('function() local releaseRoundPose=release; ' + source[start:end] + ' end')
lua.execute('slideS={ejectT=0,seatedShown=true};writes={}')
eject()
lua.execute('''
assert(slideS.ejectT==nil and slideS.seatedShown==false and #writes==4)
assert(writes[1][1]=='off' and writes[2][1]=='scale' and writes[2][3]==.001)
assert(writes[3][1]=='off' and writes[4][1]=='scale' and writes[4][3]==.001)
''')
start = source.index('    do\n        local cr = sc.chamberRound\n        if cr and slideS.chaseV')
end = source.index('    fadeTick(', start)
chamber = lua.eval('function() local releaseRoundPose=release; ' + source[start:end] + ' end')
lua.execute('slideS={chaseV=.0001,rack=0,seatedShown=true};writes={}')
chamber()
lua.execute('''
assert(slideS.chaseV==nil and #writes==4)
assert(writes[3][1]=='off' and writes[3][2]==10)
assert(writes[4][1]=='off' and writes[4][2]==11)
''')
print('PASS: finished ejection keeps scale only; chambered rounds release all pose overrides.')
