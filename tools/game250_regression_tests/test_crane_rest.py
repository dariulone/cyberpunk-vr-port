from pathlib import Path
from lupa import LuaRuntime
s=Path(__file__).resolve().parents[2].joinpath('mods/cet/CyberpunkVRPort_Reload/reload/reload.lua').read_text()
def extract(name):
 a=s.index('local function '+name+'(');return s[a:s.index('\nend',a)+4]
l=LuaRuntime()
l.execute(extract('qmul')+'\n'+extract('captureCraneRest')+'\n'+extract('driveCraneRest')+'''
local raw={.0088,-.0156,.1035,0,0,0,1}
VRRigBone=function(w,b,f) return raw[f+1] end
local s={p=0};local hc={which=1,bone=4,axis={0,0,1},sign=-1,closedRotation={0,0,0,1}}
-- Enter already open during a native clip. Sampling a stationary hinge for its entire hold is not a closed reference.
raw[6]=math.sin(math.rad(-50));raw[7]=math.cos(math.rad(-50))
for i=1,110 do captureCraneRest(s,hc,.016) end
assert(not s.rest and not s.restCandidate)
raw[6]=0;raw[7]=-1 -- equivalent quaternion sign on return to closed
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

# Frozen positional oracle, not the Lua axis or a sign test on a quaternion.
# Re-extract from a clean game with the instructions in overture_center.cpp.
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
weapon = root / 'mods/cet/CyberpunkVRPort_Reload/reload/weapons/malorian_overture.lua'
hatch_start = s.index('    local hc = cfg.hatch\n', s.index('function M.frame('))
hatch_end = s.index('\n    if REVOLVER then', hatch_start)
# Run the production input/update/write-off path. Stub only engine interfaces and
# unrelated controls. In particular, a closed CPVR hatch must NOT pin a native reload.
l.globals().cfg = l.execute(weapon.read_text(encoding='utf-8'))
l.execute('\n'.join(extract(f) for f in ('qrot', 'qmul', 'captureCraneRest', 'driveCraneRest')) + '''
local hatchS,magS,pressed={p=0},{state='in'},false
local function GetVRSharedSlot(i) assert(i==157);return pressed and 1 or 0 end
local function playSnd() end
local raw,override,offCount,writeCount=nil,nil,0,0
local function address(w,b) assert(w==1 and b==4,'wrong Overture rig/bone') end
VRRigBone=function(w,b,f) address(w,b);return raw[f+1] end
VRRigWriteRot=function(w,b,a) address(w,b);assert(a==0,'unexpected relative rotation') end
VRRigWriteAbs=function(w,b,...) address(w,b);local p={...};assert(p[8]==1);override=p;writeCount=writeCount+1;return 1 end
VRRigWriteOff=function(w,b) address(w,b);override=nil;offCount=offCount+1 end
local function tick(dt)
''' + s[hatch_start:hatch_end] + '''
end
-- RED local keys from retail 'reload'; q-vector basis conversion is (x,-z,y).
-- Bind cylinder position is parent-local, not model space. The cylinder's
-- own rotation cannot move its origin. Its parent barrel reverses local X.
local function sample(i,j,k,r) raw={.00879629888,-.0155906221,.103489958,i,j,k,r} end
local function closed() sample(.00034543555,.00034543555,.00034543555,.999999821) end
local function mid() sample(.00033725327,.00033725327,-.422475219,.906374395) end
local function opened() sample(.00031308399,.00031308399,-.765929580,.642924428) end
local function center()
    local p=override or raw
    local x,y,z=qrot(p[4],p[5],p[6],p[7],-.00883360114,.0264515206,-.0332183279)
    x,y,z=qrot(.00024431062,-.707106709,-.707106709,.000415861607,x+p[1],y+p[2],z+p[3])
    return {1000*x,1000*y,1000*z}
end
local function near(point,ref,label)
    for i=1,3 do assert(math.abs(point[i]-ref[i])<.15,
        string.format('%s: axis %d got %.4f expected %.4f mm',label,i,point[i],ref[i])) end
end
local closedRef={.0155,70.2961,10.8777}  -- idle_break_01 keys 0/26/52; reload keys 0/7/61
local midRef={-23.4265,70.2664,8.2191}   -- reload keys 8/60
local openRef={-36.4494,70.2434,-11.4365} -- reload keys 9/30/59
opened();for i=1,110 do tick(.016) end
assert(not hatchS.rest and not override,'latched a stationary native open pose')
closed();for i=1,20 do tick(.016) end
assert(hatchS.rest and not override);near(center(),closedRef,'idle closed')
local before=center();pressed=true;tick(.016);pressed=false
-- Native animation now holds the open pose: adding another rotation would double it.
opened();for i=1,24 do tick(.016) end
assert(override and writeCount>0);local after=center();near(after,openRef,'CPVR open')
local delta={after[1]-before[1],after[2]-before[2],after[3]-before[3]}
near(delta,{-36.4649,-.0527,-22.3142},'signed center displacement')
closed();pressed=true;tick(.016);pressed=false;for i=1,24 do tick(.016) end
assert(offCount==1 and not override,'native ownership not restored');near(center(),closedRef,'closed again')
-- Normal clip can move the point through all three positions without button input.
local prevWrites=writeCount
for _,key in ipairs({{closed,closedRef},{mid,midRef},{opened,openRef},{opened,openRef},{mid,midRef},{closed,closedRef}}) do
    key[1]();tick(1/30);near(center(),key[2],'normal reload');assert(not override)
end
assert(writeCount==prevWrites,'CPVR wrote during normal reload')
print(string.format('PASS: CPVR center delta = (%+.4f,%+.4f,%+.4f) mm; native reload unmasked.',delta[1],delta[2],delta[3]))
''')

# Optional independent comparison against the actual re-export, not needed in CI.
if __name__ == '__main__' and len(sys.argv) > 1:
    if len(sys.argv) not in (4, 5) or sys.argv[1] != '--measure':
        raise SystemExit('usage: test_crane_rest.py [--measure sampler.exe vanilla.glb [original-CPVR.log]]')
    output = subprocess.check_output(sys.argv[2:4], text=True)
    points = {}
    for row in output.splitlines()[1:]:
        key, center, delta, seat = row.split('|')
        clip, frame, _ = key.split()
        points[clip, int(frame)] = [tuple(map(float, v.split())) for v in (center, delta, seat)]
    closed = (.0155, 70.2961, 10.8777)
    open_ = (-36.4494, 70.2434, -11.4365)
    mid = (-23.4265, 70.2664, 8.2191)
    def check(got, expected):
        assert len(got) == len(expected) == 3 and all(abs(a-b) < .02 for a,b in zip(got, expected)), (got, expected)
    for frame in (0, 26, 52):
        check(points['idle_break_01', frame][0], closed)
    for clip in ('reload', 'empty_reload'):
        for frame, expected in ((0,closed),(7,closed),(8,mid),(9,open_),(30,open_),(59,open_),(60,mid),(61,closed)):
            check(points[clip, frame][0], expected)
        check(points[clip, 30][1], (-36.4649, -.0527, -22.3142))
        check(points[clip, 30][2], (-36.5315, -.0527, -22.3022))
    print('PASS: retail normal/empty reload and independent closed idle positions verified against GLB.')
    if len(sys.argv) == 5:
        import hashlib
        import re
        capture = Path(sys.argv[4]).read_bytes()
        assert hashlib.sha256(capture).hexdigest() == '749840d672c00c292b79431d91d87e9ea08615a6837a02b6d61fd19b25f75780'
        rows = re.findall(rb'^(closed|open|closed again):[^\r\n]*?mag\[in/[^\r\n]*? gap=\(([^)]+)\)', capture, re.M)
        assert [k for k,_ in rows] == [b'closed', b'open', b'closed again']
        shut, out, returned = [[float(v) for v in xyz.split(b',')] for _,xyz in rows]
        assert shut == returned
        # gap is hand MINUS the target derived from mag_slot, not a cylinder-bone readback.
        # Two rounded-to-mm readings can move the difference by up to 1 mm per axis.
        actual = tuple(round(1000*(a-b)) for a,b in zip(shut,out))
        assert all(abs(a-b) <= 1.0 for a,b in zip(actual, points['reload',30][2])), actual
        print(f'PASS: recorded CPVR target delta {actual} mm agrees with the vanilla slot delta.')
