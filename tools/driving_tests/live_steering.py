"""Bounded stationary steering trials through the private simulator's pose/grip IPC.

No throttle, keys, game-memory writes or direct player movement. Restores poses
and releases synthetic grips in finally. Requires an idle driver, neutral HMD,
and both hands already within the existing wheel grab zones.
"""
import argparse,ctypes,hashlib,json,math,os,re,struct,sys,time,uuid
from ctypes import wintypes
from pathlib import Path
sys.path[:0]=[str(Path(__file__).resolve().parents[1]/n) for n in ('swimming_tests','roomscale_tests')]
from live_symbols import resolve
from live_read import Reader
from simulator_probe import read_status

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--pid',type=int,required=True);p.add_argument('--dll',type=Path,required=True)
p.add_argument('--runtime',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
p.add_argument('--case',choices=('right','left','both','handoff','lock','micro','profile'),required=True)
p.add_argument('--profile',type=int,choices=(1,2,3,4),default=1)
p.add_argument('--frequency',type=float,default=.5)
p.add_argument('--amplitude',type=float,default=20)
p.add_argument('--prediction',type=int,choices=(0,1))
p.add_argument('--offsets',type=Path)
p.add_argument('--grip-mask',type=int,choices=(1,2,3))
p.add_argument('--duration',type=float,default=.5)
p.add_argument('--inspect-hold',type=float,default=0)
a=p.parse_args();assert .25<=a.duration<=1.5 and a.out.parent.is_dir() and not a.out.exists()
assert 0<=a.inspect_hold<=15
assert .25<=a.frequency<=1.5 and 0<a.amplitude<=60
user=ctypes.WinDLL('user32');user.GetForegroundWindow.restype=wintypes.HWND
user.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
user.GetWindowTextW.argtypes=[wintypes.HWND,wintypes.LPWSTR,ctypes.c_int]
user.IsIconic.argtypes=[wintypes.HWND]
def require_foreground():
    window=user.GetForegroundWindow();owner=wintypes.DWORD()
    user.GetWindowThreadProcessId(window,ctypes.byref(owner))
    title=ctypes.create_unicode_buffer(1024);user.GetWindowTextW(window,title,len(title))
    if owner.value!=a.pid or not title.value.startswith('Cyberpunk 2077') or user.IsIconic(window):
        raise RuntimeError('User must expand and focus the game before steering tests')
require_foreground()
# Fail before changing poses if another runtime is loaded.
resolve(a.pid,a.runtime.resolve(),[])
names=['g_handsStable','g_pSharedHands','g_isDriving','cvr::anim::g_wheel',
       'cvr::anim::g_wheelSpan','cvr::anim::g_wheelSteer','cvr::anim::g_wheelSteerDeg','g_liveControls','g_handsStableSeq',
       'g_VRIKFreshTotal','g_solveCacheN','s_vrikTransform']
syms=resolve(a.pid,a.dll.resolve(),names)
r=Reader(a.pid,r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
directory=Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator'
head=read_status(directory)['head_tracking'];assert all(abs(head[k])<1e-4 for k in ('yaw','pitch','roll'))
def read(name,fmt):return struct.unpack('<'+fmt,r.read(int(syms[name],16),struct.calcsize('<'+fmt)))
shared=read('g_pSharedHands','Q')[0]
def grip_values():return [struct.unpack('<f',r.read(shared+k*4,4))[0] for k in (155,49)]
rows=[];original=[];run=uuid.uuid4().hex;start=time.monotonic();phase='baseline';commanded=0;testing=False
offsets=json.loads(a.offsets.read_text(encoding='utf-8-sig')) if a.offsets else None
ini=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\red4ext\plugins\cyberpunkvrport\config\vrport.ini')
iniBefore=None;iniApplied=None;configReport={}
def config(key,fmt):return struct.unpack('<'+fmt,r.read(int(syms['g_liveControls'],16)+offsets[key],struct.calcsize('<'+fmt)))[0]
def atomic_settings(data):
    temporary=ini.with_name('vrport.'+run+'.writing');temporary.write_bytes(data)
    deadline=time.monotonic()+3
    while True:
        try:os.replace(temporary,ini);return
        except PermissionError:
            if time.monotonic()>deadline:raise
            time.sleep(.02)
def predict_config(value):
    global iniBefore,iniApplied
    assert offsets is not None,'compiled field offsets required'
    iniBefore=ini.read_bytes();a.out.with_suffix('.ini.before').write_bytes(iniBefore)
    text=iniBefore.decode('utf-8-sig')
    for name,value in (('xr_wheel_prediction',value),('xr_wheel_prediction_ms',8)):
        pattern=r'(?m)^'+name+r'\s*=.*$';line=name+'='+str(value)
        text=re.sub(pattern,line,text) if re.search(pattern,text) else text+'\n'+line+'\n'
    iniApplied=text.encode('utf-8');atomic_settings(iniApplied)
    deadline=time.monotonic()+4
    while config('prediction','i')!=a.prediction or abs(config('horizon','f')-8)>.001:
        if time.monotonic()>deadline:raise RuntimeError('prediction setting did not reach the native consumer')
        time.sleep(.025)
    configReport.update(prediction=config('prediction','i'),horizon=config('horizon','f'),limit=config('limit','f'),dead=config('dead','f'))
def sample():
    if testing:require_foreground()
    data=r.read(int(syms['cvr::anim::g_wheel'],16),128)
    hands=[]
    for side in range(2):
        i=side*64
        hands.append({'blend':struct.unpack_from('<f',data,i)[0],'engaged':bool(data[i+4]),
                      'at_wheel':bool(data[i+5]),'target':struct.unpack_from('<3f',data,i+12),
                      'animated':struct.unpack_from('<3f',data,i+24)})
    stamp=struct.unpack('<Q',r.read(int(syms['s_vrikTransform'],16)+48,8))[0]
    r.k.GetTickCount64.restype=ctypes.c_uint64
    row={'t':time.monotonic()-start,'phase':phase,'commanded_deg':commanded,'hands':hands,
         'camera_age_ms':r.k.GetTickCount64()-stamp,'fresh':read('g_VRIKFreshTotal','i')[0],
         'cache_count':read('g_solveCacheN','i')[0],
         'steer':read('cvr::anim::g_wheelSteer','f')[0],'angle':read('cvr::anim::g_wheelSteerDeg','f')[0],
         'sequence':read('g_handsStableSeq','I')[0],
         'driving':read('g_isDriving','?')[0]}
    rows.append(row);return row
def hold(seconds):
    end=time.monotonic()+seconds
    while time.monotonic()<end:sample();time.sleep(.01)
def send(value):
    path=directory/'tracking_frame_command.json';deadline=time.monotonic()+3
    while path.exists():
        if time.monotonic()>deadline:raise RuntimeError('Existing tracking command not consumed')
        time.sleep(.003)
    tmp=directory/(run+'.writing')
    tmp.write_text(json.dumps({**value,'_probe_run':run}),encoding='ascii');os.replace(tmp,path)
    while path.exists():
        if time.monotonic()>deadline:raise RuntimeError('Tracking command not consumed')
        time.sleep(.002)
def frame(held,angle=0,moving=None,profile=0):
    global commanded
    commanded=angle
    out={'enabled':True,**head['position'],**{k:head[k] for k in ('yaw','pitch','roll')},
         'lgrip':int(bool(held&2)),'rgrip':int(bool(held&1))}
    if profile:out.update(profile=profile,radius=radius,amplitude=a.amplitude,frequency=a.frequency)
    for h,old in enumerate(original):
        side=-1 if h==0 else 1;bit=2 if h==0 else 1
        turn=angle*math.pi/180 if ((held if moving is None else moving)&bit) else 0
        prefix='l' if h==0 else 'r'
        out.update({prefix+'x':old['x']+side*radius*(math.cos(turn)-1),
                    prefix+'y':old['y']-side*radius*math.sin(turn),prefix+'z':old['z'],
                    prefix+'yaw':old['yaw'],prefix+'pitch':old['pitch']})
    send(out)
def move(held,begin,end,duration):
    began=time.monotonic()
    while True:
        t=min(1,(time.monotonic()-began)/duration)
        frame(held,begin+(end-begin)*t);sample()
        if t>=1:break
        time.sleep(.004)

error=None;mutated=False;summary={}
try:
    first=sample();assert first['driving'] and all(h['at_wheel'] and not h['engaged'] for h in first['hands']), 'Idle driver with both hands near rim required'
    assert max(grip_values())<.1,'Release both grips before the test'
    if a.prediction is not None:predict_config(a.prediction)
    radius=max(.08,min(.45,read('cvr::anim::g_wheelSpan','f')[0]*.5))
    raw=read('g_handsStable','128f')
    for h in range(2):
        i=h*8;assert raw[i]==1
        x,y,z,w=raw[i+4:i+8]
        pitch=math.asin(max(-1,min(1,2*(w*x-y*z))))
        yaw=math.atan2(2*(w*y+x*z),1-2*(x*x+y*y))
        expected=(math.cos(yaw/2)*math.sin(pitch/2),math.sin(yaw/2)*math.cos(pitch/2),
                  -math.sin(yaw/2)*math.sin(pitch/2),math.cos(yaw/2)*math.cos(pitch/2))
        assert min(max(abs(v-u) for v,u in zip((x,y,z,w),expected)),max(abs(v+u) for v,u in zip((x,y,z,w),expected)))<.001,'controller roll cannot be restored'
        original.append(dict(zip(('x','y','z'),raw[i+1:i+4]))|{'yaw':yaw,'pitch':pitch})
    mutated=True;testing=True;frame(0);phase='settle';hold(.3)
    mask=a.grip_mask or {'right':1,'left':2,'both':3,'handoff':3,'lock':1,'micro':3,'profile':3}[a.case]
    phase='grab';frame(mask);hold(.28);grab=sample()
    assert all(grab['hands'][h]['engaged'] for h in range(2) if mask&(1<<h)),'Grip did not engage'
    assert abs(grab['steer'])<.01,'Grab changed steering at rest'
    if a.inspect_hold:
        phase='inspect';hold(a.inspect_hold)
    if a.case in ('right','left','both'):
        for direction in (1,-1):
            phase='turn_right' if direction==1 else 'turn_left'
            move(mask,0,60*direction,a.duration);hold(.2);tip=sample()
            assert tip['steer']*direction>.35,'Turn was absent or reversed'
            phase='return';move(mask,60*direction,0,a.duration);hold(.15)
            assert abs(sample()['angle'])<3,'Return did not center steering'
    elif a.case=='lock':
        summary['neutral_after_overtravel']=[]
        for direction in (1,-1):
            phase='beyond_lock';move(mask,0,140*direction,.8);hold(.1)
            peak=sample();assert peak['steer']*direction>.95,'Full lock not reached'
            phase='return_from_lock';move(mask,140*direction,0,.8);hold(.25)
            neutral=sample();summary['neutral_after_overtravel'].append(neutral['angle'])
            assert abs(neutral['angle'])<.05 and abs(neutral['steer'])<.0001,'Overtravel shifted the neutral position'
    elif a.case=='micro':
        def rim(row):
            right,left=[h['animated'] for h in row['hands']]
            return -math.degrees(math.atan2(right[2]-left[2],right[0]-left[0]))
        base=rim(sample());summary['micro_steps']=[]
        for angle in (4,0,-4,0,8,0,-8,0):
            phase='micro';begin=len(rows);frame(mask,angle);sample();hold(.3)
            segment=rows[begin:];last=segment[-1]
            native=math.remainder(rim(last)-base,360)
            intent=next((row for row in segment if row['steer']*angle>0),None) if angle else None
            response=next((row for row in segment if math.remainder(rim(row)-base,360)*angle>.4),None) if angle else None
            delay=(response['t']-intent['t'])*1000 if response and intent else None
            summary['micro_steps'].append({'command':angle,'angle':last['angle'],'steer':last['steer'],
                                          'native_delta':native,'sampled_input_to_response_ms':delay})
            if angle:assert native*angle>.4,'Native wheel did not follow micro-correction'
            else:assert abs(native)<1,'Native wheel did not settle after micro-correction'
    elif a.case=='profile':
        phase='profile';frame(mask,profile=a.profile)
        summary['profile_begin']=time.monotonic()-start
        hold(.6+(4/a.frequency if a.profile==1 else 5 if a.profile==2 else 4)+.6)
        phase='settled';frame(mask);hold(.25)
        neutral=sample();assert abs(neutral['angle'])<.05 and abs(neutral['steer'])<.0001,'Profile did not return to neutral'
    else:
        phase='turn';move(3,0,35,a.duration);hold(.1);previous=sample()['angle']
        for mask in (1,3,2,3):
            phase='handoff';frame(mask,35,moving=3);hold(.22)
            current=sample()['angle'];assert abs(current-previous)<3,'Hand change jumped steering';previous=current
    phase='release';frame(0);hold(.2);assert abs(sample()['steer'])<.001,'Release retained steering'
except BaseException as e:error=str(e)
finally:
    testing=False
    if mutated:
        try:frame(0);hold(.15);send({'enabled':False});hold(.1)
        except Exception as e:error=(error+'; ' if error else '')+'restore: '+str(e)
    for name in ('tracking_frame_command.json',run+'.writing'):
        path=directory/name
        try:
            if path.exists() and json.loads(path.read_text()).get('_probe_run')==run:path.unlink()
        except (OSError,ValueError):pass
    if mutated:
        summary['released_grips']=grip_values()
        summary['final_steer']=read('cvr::anim::g_wheelSteer','f')[0]
    if iniBefore is not None:
        try:
            current=ini.read_bytes()
            if current!=iniApplied:raise RuntimeError('settings changed externally; restore the two prediction keys manually from the saved backup')
            atomic_settings(iniBefore)
            configReport['restored_sha256']=hashlib.sha256(ini.read_bytes()).hexdigest()
            assert ini.read_bytes()==iniBefore
        except Exception as e:error=(error+'; ' if error else '')+'settings restore: '+str(e)
    r.close()
summary.update(case=a.case,error=error,samples=len(rows),minimum=min(v['steer'] for v in rows),maximum=max(v['steer'] for v in rows))
report={'pid':a.pid,'sha256':syms['sha256'],'summary':summary,'rows':rows,'head':head,'controllers':original,
        'settings':configReport,'profile':a.profile,'frequency':a.frequency,'amplitude':a.amplitude,
        'game_memory_writes':False,'input':'simulator grip + controller poses only'}
a.out.write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(summary,indent=2))
if error:raise RuntimeError(error)
