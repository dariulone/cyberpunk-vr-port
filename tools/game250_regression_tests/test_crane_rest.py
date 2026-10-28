from pathlib import Path
from lupa import LuaRuntime
s=Path(__file__).resolve().parents[2].joinpath('mods/cet/CyberpunkVRPort_Reload/reload/reload.lua').read_text()
def extract(name):
 a=s.index('local function '+name+'(');return s[a:s.index('\nend',a)+4]
l=LuaRuntime()
l.execute(extract('qmul')+'\n'+extract('captureCraneRest')+'\n'+extract('driveCraneRest')+'''
local raw={.0088,-.0156,.1035,0,0,0,1}
VRRigBone=function(w,b,f) return raw[f+1] end
local s={p=0};local hc={which=1,bone=4,axis={0,0,1},sign=-1}
for i=1,12 do captureCraneRest(s,hc,.016) end
assert(s.rest and s.rest[1]==.0088)
-- Animation changes after open: the relative first-write base would be wrong.
raw[6]=math.sin(math.rad(60)/2);raw[7]=math.cos(math.rad(60)/2)
s.open=true;captureCraneRest(s,hc,.016)
local last
VRRigWriteRot=function(w,b,a) assert(a==0) end
VRRigWriteAbs=function(w,b,x,y,z,i,j,k,r,absolute) last={x,y,z,i,j,k,r};assert(absolute==1) end
assert(driveCraneRest(s,hc,100));assert(math.abs(last[6]+math.sin(math.rad(50)))<1e-6)
assert(math.abs(last[7]-math.cos(math.rad(50)))<1e-6)
assert(last[1]==.0088 and last[2]==-.0156 and last[3]==.1035)
assert(driveCraneRest(s,hc,0));assert(last[6]==0 and last[7]==1)
local missing={};raw={0,0,0,0,0,0,0}
for i=1,20 do captureCraneRest(missing,hc,.016) end
assert(not missing.rest and not driveCraneRest(missing,hc,100))
local moving={};raw={0,0,0,0,0,0,1}
for i=1,30 do raw[1]=i*.01;captureCraneRest(moving,hc,.016) end
assert(not moving.rest)
''')
print('PASS: closed-pose capture, reject missing/moving pose, absolute opening immune to animated base, exact closed transform.')
