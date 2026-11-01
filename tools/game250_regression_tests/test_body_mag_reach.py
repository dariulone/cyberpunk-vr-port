from pathlib import Path
from lupa import LuaRuntime
s=Path(__file__).resolve().parents[2].joinpath('mods/cet/CyberpunkVRPort_Reload/reload/reload.lua').read_text()
def function(name):
 a=s.index('local function '+name+'('); b=s.index('\nend',a)+4
 return s[a:b]
l=LuaRuntime(unpack_returned_tuples=True)
l.execute('slots={};GetVRSharedSlot=function(i) return slots[i] end\n'+function('qrot')+'\n'+function('bodyMagReach')+'''
slots[127]=2;slots[0]=1;slots[8]=1;slots[16]=0;slots[17]=0;slots[18]=0;slots[19]=1
slots[1]=-.20;slots[2]=-.35;slots[3]=-.15
assert(bodyMagReach(0)) -- chest, even when avatar pouch is low
slots[9]=.20;slots[10]=-.85;slots[11]=-.10
assert(bodyMagReach(1)) -- either free hand, waist
slots[1]=-.7;assert(not bodyMagReach(0)) -- extended arm
slots[1]=0;slots[2]=0;assert(not bodyMagReach(0)) -- face
slots[2]=-1.3;assert(not bodyMagReach(0)) -- below pickup range
-- Same physical chest position while looking down 60 degrees.
local i,r=math.sin(math.pi/6),math.cos(math.pi/6)
slots[1],slots[2],slots[3]=qrot(-i,0,0,r,-.20,-.35,-.15)
slots[16]=i;slots[19]=r;assert(bodyMagReach(0))
slots[0]=0;assert(not bodyMagReach(0));slots[0]=1
slots[127]=3;assert(not bodyMagReach(0));slots[127]=4
slots[1]=0/0;assert(not bodyMagReach(0))
''')
# Exercise the production empty-well gate, including slide priority and low pouch.
a=s.index('                local beltD = beltMagDist(free)',s.index('local catchW = snapW'))
b=s.index('\n            end',a)
gate=s[a:b]
l.execute('''
body=false;beltD=2;dist=.4;slideS={};magS={slideHasPose=true};sc={};mc={};free=0;wx=0;wy=0;wz=0
beltMagDist=function() return beltD end;bodyMagReach=function() return body end
beltMagRadius=function() return .12 end;magFromRaw=function() return 0,0,0 end
len3=function() return dist end
pullW=function(d,r) return d>=0 and d<r and (1-d/r) or 0 end
function gate()
 local catchW,takeAnywhere=0,false
'''+gate+'''
 return catchW>0 or takeAnywhere
end
body=true;slideS.dist=.3;assert(gate()) -- distant slide no longer blocks chest
slideS.dist=.02;assert(not gate()) -- reachable slide wins
slideS.dist=.3;slideS.grabbed=true;assert(not gate())
slideS.grabbed=false;slideS.releasing=true;assert(not gate());slideS.releasing=false
body=false;assert(not gate()) -- low pouch still requires body proximity
beltD=.04;assert(gate()) -- original physical pouch works
beltD=nil;slideS.dist=-1;assert(gate()) -- preserve belt-less fallback
''')
print('PASS: body pickup both hands, head pitch, bounds, invalid tracking, low pouch, slide priority and legacy fallback.')
