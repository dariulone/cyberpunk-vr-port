"""Bounded three-device simulator matrix, with read-only native telemetry.

The simulator IPC is sequential (one controller per command); its measured update
rate/skew is reported, not confused with synchronous hardware or the offline tests.
Only xr_tracked_body_rotation and simulator poses change. Both are restored in
finally. No injected game-memory writes, held buttons, focus changes or launches.
Run live_types.py against the matching PDB first; --types is its exact ABI report.
"""
import argparse,ctypes as ct,json,math,os,random,re,statistics,struct,sys,time,uuid
from pathlib import Path
sys.path[:0]=[str(Path(__file__).resolve().parents[1]/p) for p in ('roomscale_tests','swimming_tests')]
from live_read import Reader
from live_symbols import resolve
from simulator_probe import read_status
import capstone

D=math.pi/180
POSES={
 'rest':([[-.24,-.65,0],[.24,-.65,0]],[0,0]),
 'low_ready':([[-.25,-.45,-.2],[.25,-.45,-.2]],[0,0]),
 'rifle':([[-.1,-.24,-.55],[.06,-.24,-.35]],[0,0]),
 'two_handed_pistol':([[-.025,-.2,-.45],[.025,-.2,-.45]],[0,0]),
 'boxing_guard':([[-.32,-.25,-.42],[.24,-.28,-.4]],[0,0]),
 'crossed':([[.2,-.35,-.3],[-.2,-.35,-.3]],[0,0]),
 'hands_high':([[-.28,.2,-.2],[.28,.2,-.2]],[0,0]),
 'right_pistol':([[-.24,-.65,0],[.15,-.2,-.45]],[-math.pi/2,0]),
 'left_pistol':([[-.15,-.2,-.45],[.24,-.65,0]],[0,-math.pi/2]),
 'one_hand_high':([[-.24,-.65,0],[.25,.2,-.2]],[0,0]),
}
def wrap(a):return math.remainder(a,2*math.pi)
def add(a,b):return [x+y for x,y in zip(a,b)]
def sub(a,b):return [x-y for x,y in zip(a,b)]
def yawpos(p,a):
 c,s=math.cos(a),math.sin(a);return [c*p[0]+s*p[2],p[1],-s*p[0]+c*p[2]]
def mul(a,b):
 x,y,z,w=a;X,Y,Z,W=b
 return [w*X+x*W+y*Z-z*Y,w*Y-x*Z+y*W+z*X,w*Z+x*Y-y*X+z*W,w*W-x*X-y*Y-z*Z]
def quat(y,p=0,r=0):
 return mul(mul([0,math.sin(y/2),0,math.cos(y/2)],[math.sin(p/2),0,0,math.cos(p/2)]),[0,0,math.sin(r/2),math.cos(r/2)])
def rotate(q,p):return mul(mul(q,[*p,0]),[-q[0],-q[1],-q[2],q[3]])[:3]
def euler(q):
 x,y,z,w=q
 return math.atan2(2*(w*y+x*z),1-2*(x*x+y*y)),math.asin(max(-1,min(1,2*(w*x-y*z))))
def atomic(path,value):
 tmp=path.with_name(path.name+'.'+uuid.uuid4().hex+'.writing')
 tmp.write_bytes(value if isinstance(value,bytes) else json.dumps(value).encode('ascii'))
 for _ in range(200):
  try:os.replace(tmp,path);return
  except PermissionError:time.sleep(.005)
 raise RuntimeError('IPC/config file stayed locked: '+str(path))

class Probe:
 def __init__(self,a):
  self.a=a;self.rows=[];self.results=[];self.commands=[];self.mode_events=[];self.rng=random.Random(240924)
  self.directory=Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator'
  self.game=a.game;self.ini=a.game/'red4ext/plugins/cyberpunkvrport/config/vrport.ini';self.saved_ini=self.ini.read_bytes()
  self.original_mode=int(re.search(rb'^xr_tracked_body_rotation\s*=\s*(\d+)',self.saved_ini,re.M)[1])
  names=['OpenXRManager::Get','CyberpunkVR_BodyYawRealignRad','CyberpunkVR_BodyYawFollow','CyberpunkVR_BodyYawFollowDeadDeg',
   'CyberpunkVR_BodyYawFinalRad','CyberpunkVR_BodyYawFinalValid','g_PlayerPoseHeadingReady','g_VRBoneCount','g_liveControls','s_trackedBody']
  self.symbols=resolve(a.pid,a.dll.resolve(),names);self.addr={k:int(self.symbols[k],16) for k in names}
  self.batch_runtime=resolve(a.pid,a.batch_runtime.resolve(),[]) if a.batch_runtime else None
  self.batch_active=False
  self.r=Reader(a.pid,str(a.game/'bin/x64/Cyberpunk2077.exe'),0,0)
  self.types=json.loads(a.types.read_text());assert self.types['sha256']==self.symbols['sha256'],'PDB layout report belongs to another DLL'
  self.t0=time.monotonic();self.phase='initial';self.expected=0
  self.original_head=None;self.original_hands=[];self.original_frame=None;self.last_command={};self.seq=0;self.run_id=uuid.uuid4().hex
  # Resolve the actual singleton from the matched Get() implementation. Both
  # construction and return must point at the same address within the DLL.
  md=capstone.Cs(capstone.CS_ARCH_X86,capstone.CS_MODE_64);md.detail=True
  found=[]
  for ins in md.disasm(self.r.read(self.addr['OpenXRManager::Get'],128),self.addr['OpenXRManager::Get']):
   if ins.mnemonic=='lea' and ins.operands[0].reg in (capstone.x86.X86_REG_RAX,capstone.x86.X86_REG_RBX) and ins.operands[1].type==capstone.x86.X86_OP_MEM and ins.operands[1].mem.base==capstone.x86.X86_REG_RIP:
    found.append(ins.address+ins.size+ins.operands[1].mem.disp)
  assert len(found)==2 and found[0]==found[1],'unrecognized singleton code'
  self.instance=found[0];assert int(self.symbols['module_base'],16)<self.instance<int(self.symbols['module_base'],16)+0x10000000
  f=self.types['cvr::body::TrackingFrame'];assert f['size']==112 and f['fields']['sequence']['offset']==88
  self.frame_addr=self.instance+self.types['OpenXRManager']['fields']['m_bodyTrackingFrame']['offset']
  self.mode_addr=self.addr['g_liveControls']+self.types['LiveControls']['fields']['xrTrackedBodyRotation']['offset']
  self.est_addr=self.addr['s_trackedBody'];self.est_fields=self.types['cvr::body::TrackedBodyYaw']['fields']
  self.kernel=ct.WinDLL('kernel32');self.freq=ct.c_longlong();self.kernel.QueryPerformanceFrequency(ct.byref(self.freq))
  self.kernel.GetExitCodeProcess.argtypes=[ct.c_void_p,ct.POINTER(ct.c_ulong)]
 def alive(self):
  code=ct.c_ulong()
  return bool(self.kernel.GetExitCodeProcess(self.r.handle,ct.byref(code))) and code.value==259
 def clear_pending(self):
  for name in ('head_pose_command.json','controller_pose_command.json','tracking_frame_command.json'):
   path=self.directory/name
   if path.exists():
    try:
     if json.loads(path.read_text()).get('_probe_run')==self.run_id:path.unlink()
    except (OSError,ValueError):pass
 def get(self,name,fmt='<f'):return struct.unpack(fmt,self.r.read(self.addr[name],struct.calcsize(fmt)))[0]
 def mode(self,value):
  content=self.ini.read_bytes();changed,n=re.subn(rb'(?m)^(xr_tracked_body_rotation\s*=\s*)\d+',lambda m:m[1]+str(value).encode(),content)
  assert n==1
  atomic(self.ini,changed)
  if not self.alive():return
  started=time.monotonic();deadline=started+6;retry_at=started+1;retries=0
  while struct.unpack('<i',self.r.read(self.mode_addr,4))[0]!=value:
   if time.monotonic()>deadline:raise RuntimeError('Live setting was not applied')
   if time.monotonic()>=retry_at:
    # The existing INI poll caches its timestamp before opening. If its open
    # races an atomic rename/share lock, re-notify the same content; never
    # overwrite a different setting supplied by another writer during the test.
    actual=int(re.search(rb'^xr_tracked_body_rotation\s*=\s*(\d+)',self.ini.read_bytes(),re.M)[1])
    if actual!=value:raise RuntimeError('Body setting changed by another writer')
    os.utime(self.ini,None);retries+=1;retry_at=time.monotonic()+1
   time.sleep(.02)
  self.mode_events.append({'value':value,'seconds':time.monotonic()-started,'renotify':retries})
 def frame(self):
  for _ in range(40):
   b=self.r.read(self.frame_addr,112)
   if b==self.r.read(self.frame_addr,112):
    f=struct.unpack('<21f3?x3Q',b)
    return {'head':f[:4],'position':f[4:7],'hands':[f[7:10],f[10:13]],'rotations':[f[13:17],f[17:21]],'valid':f[21],
     'tracked':f[22:24],'sequence':f[24],'stamp_us':f[25],'origin':f[26]}
  raise RuntimeError('No stable tracking snapshot')
 def sample(self):
  f=self.frame();qpc=ct.c_longlong();self.kernel.QueryPerformanceCounter(ct.byref(qpc))
  row={'t':time.monotonic()-self.t0,'phase':self.phase,'command':self.seq,'expected_deg':self.expected/D,
   'offset_deg':self.get('CyberpunkVR_BodyYawRealignRad')/D,'cone':self.get('CyberpunkVR_BodyYawFollowDeadDeg'),
   'active':self.get('CyberpunkVR_BodyYawFollow','<i'),'body_deg':self.get('CyberpunkVR_BodyYawFinalRad')/D,
   'body_valid':self.get('CyberpunkVR_BodyYawFinalValid','<i'),'heading_ready':self.get('g_PlayerPoseHeadingReady','<?'),
   'bones':self.get('g_VRBoneCount','<i'),'frame':f,'age_ms':(qpc.value/self.freq.value*1e6-f['stamp_us'])/1000}
  for name in ('target','output','evidence'):
   row['est_'+name]=struct.unpack('<f',self.r.read(self.est_addr+self.est_fields[name]['offset'],4))[0]
  row['est_anchor_yaw']=struct.unpack('<f',self.r.read(self.est_addr+self.est_fields['anchorYaw']['offset'],4))[0]
  row['est_anchor']=struct.unpack('<24f',self.r.read(self.est_addr,96))
  self.rows.append(row);return row
 def hold(self,seconds):
  end=time.monotonic()+seconds
  while time.monotonic()<end:self.sample();time.sleep(.01)
 def wait_file(self,path):
  end=time.monotonic()+3
  while path.exists():
   if time.monotonic()>end:raise RuntimeError('Simulator command not consumed: '+path.name)
   time.sleep(.001)
 def head(self,value):
  path=self.directory/'head_pose_command.json';self.wait_file(path);atomic(path,{**value,'_probe_run':self.run_id});self.wait_file(path)
 def hand(self,value):
  path=self.directory/'controller_pose_command.json';self.wait_file(path);atomic(path,{**value,'_probe_run':self.run_id});self.wait_file(path)
 def tracking(self,head=None,hands=None):
  value={'enabled':head is not None,'_probe_run':self.run_id}
  if head is not None:
   value.update(head)
   for prefix,c in zip(('l','r'),hands):
    for dst,src in (('x','posX'),('y','posY'),('z','posZ'),('yaw','yaw'),('pitch','pitch')):value[prefix+dst]=c[src]
  path=self.directory/'tracking_frame_command.json';self.wait_file(path);atomic(path,value);self.wait_file(path)
  self.batch_active=head is not None
 def pose(self,pose,body=0,head=None,pitch=0,roll=0,jitter=.002,gesture=None,head_shift=None):
  head=body if head is None else head
  # Independent noise for all three trackers; rotations are small and positions
  # have a 2 mm standard deviation. No controller trigger/button fields are sent.
  n=lambda s:[self.rng.gauss(0,s) for _ in range(3)]
  hn=n(jitter);head+=self.rng.gauss(0,.10*D) if jitter else 0
  hp=add(sub(self.neck,rotate(quat(head,pitch,roll),[0,-.1,.08])),hn)
  if head_shift is not None:hp=add(hp,head_shift)
  hc=dict(zip(('x','y','z'),hp));hc.update(yaw=head,pitch=pitch,roll=roll)
  ps,grip=POSES[pose];ps=[list(x) for x in ps];wr=[body,body]
  if gesture:ps,wr=gesture(ps,wr)
  cs=[]
  for h in (0,1):
   world=add(add(self.neck,yawpos(ps[h],body)),n(jitter))
   # Simulator's offset rotation uses the opposite yaw sign from its orientation
   # quaternion. Invert that exact position transform, rather than assuming a
   # conventional HMD-local offset. Resulting world poses are validated via XR.
   off=yawpos(sub(world,hp),head)
   cs.append(dict(hand=h,posX=off[0],posY=off[1],posZ=off[2],yaw=wr[h]-head+(self.rng.gauss(0,.10*D) if jitter else 0),pitch=grip[h]-pitch))
  start=time.monotonic();self.expected=body;self.seq+=1
  self.last_command={'id':self.seq,'head':hc,'controllers':cs,'body_deg':body/D,'duration_ms':None,'consumed':False}
  self.commands.append(self.last_command)
  # Head + left are available to the same runtime iteration; right follows one
  # IPC iteration later. Store duration so this transport skew stays visible.
  if self.batch_runtime:self.tracking(hc,cs)
  else:
   hf=self.directory/'head_pose_command.json';cf=self.directory/'controller_pose_command.json'
   self.wait_file(hf);self.wait_file(cf);atomic(hf,{**hc,'_probe_run':self.run_id});atomic(cf,{**cs[0],'_probe_run':self.run_id});self.wait_file(hf);self.wait_file(cf);self.hand(cs[1])
  self.last_command.update(duration_ms=(time.monotonic()-start)*1000,consumed=True);self.sample()
 def sweep(self,pose,a,b,speed,phase,jitter=.002):
  self.phase=phase;start=time.monotonic();duration=abs(b-a)/(speed*D)
  while True:
   u=min(1,(time.monotonic()-start)/duration) if duration else 1
   self.pose(pose,a+(b-a)*u,jitter=jitter)
   if u>=1:break
 def settled(self,pose,body,phase):
  self.phase=phase;self.pose(pose,body,jitter=0);self.hold(.55)
  rows=[r for r in self.rows if r['phase']==phase and r['t']>self.rows[-1]['t']-.12]
  return statistics.mean(r['offset_deg'] for r in rows)
 def prepare(self,pose):
  self.phase=pose+'/prepare';self.mode(0);self.pose(pose,0,jitter=0);self.hold(.7)
  self.mode(1);self.hold(.45)
  f=self.frame();assert f['valid'] and all(f['tracked'])
  assert self.get('CyberpunkVR_BodyYawFollow','<i')==1 and self.get('CyberpunkVR_BodyYawFollowDeadDeg')==0
  # Check the three raw OpenXR poses at neutral, not just command acknowledgments.
  for h in (0,1):assert math.dist(f['hands'][h],add(POSES[pose][0][h],[0,-.1,.08]))<.004,(pose,f)
  return self.get('CyberpunkVR_BodyYawRealignRad')/D
 def turn_case(self,pose,speed,sign):
  angle=sign*(10 if speed==5 else 30 if speed==30 else 60 if speed==90 else 90)*D
  label=f'{pose}/{sign*speed:+g}dps';baseline=self.get('CyberpunkVR_BodyYawRealignRad')/D;begin=len(self.rows)
  self.sweep(pose,0,angle,speed,label+'/out')
  reached=self.settled(pose,angle,label+'/hold')
  self.sweep(pose,angle,0,speed,label+'/return')
  returned=self.settled(pose,0,label+'/end')
  rows=self.rows[begin:];err=abs(wrap((reached-baseline)*D-angle))/D;returnerr=abs(wrap((returned-baseline)*D))/D
  moving=[r for r in rows if r['phase'].endswith(('/out','/return'))]
  healthy=all(r['active']==1 and r['cone']==0 and r['frame']['valid'] and all(r['frame']['tracked']) and 0<=r['age_ms']<=150 for r in rows)
  result={'kind':'turn','pose':pose,'speed_dps':sign*speed,'angle_deg':angle/D,'settled_error_deg':err,'return_error_deg':returnerr,
   'peak_moving_error_deg':max(abs(wrap((r['offset_deg']-baseline-r['expected_deg'])*D))/D for r in moving),
   'samples':len(rows),'invalid_frames':sum(not r['frame']['valid'] or not all(r['frame']['tracked']) for r in rows),
   'nonzero_cone':sum(r['cone']!=0 for r in rows),'healthy':healthy,'pass':err<3 and returnerr<3 and healthy}
  self.results.append(result);print(json.dumps(result),flush=True)
 def negatives(self,poses):
  for pose in poses:
   for kind in ('head_only','one_hand','wrist_only','idle'):
    baseline=self.prepare(pose);label=pose+'/'+kind;self.phase=label;begin=len(self.rows);start=time.monotonic()
    while (t:=time.monotonic()-start)<3:
     wave=math.sin(t*2*math.pi/3);head=70*D*wave if kind in ('head_only','one_hand','wrist_only') else 0
     gesture=None
     if kind=='one_hand':
      def gesture(ps,wr,wave=wave):
       ps[1][0]-=.25*wave;ps[1][1]+=.2*wave;wr[1]+=40*D*wave;return ps,wr
     if kind=='wrist_only':
      def gesture(ps,wr,wave=wave):return ps,[v+55*D*wave for v in wr]
     self.pose(pose,0,head,30*D*wave if kind=='head_only' else 0,20*D*wave if kind=='head_only' else 0,gesture=gesture)
    end=self.settled(pose,0,label+'/settled');rows=self.rows[begin:]
    peak=max(abs(wrap((r['offset_deg']-baseline)*D))/D for r in rows)
    healthy=all(r['active']==1 and r['cone']==0 and r['frame']['valid'] and all(r['frame']['tracked']) and 0<=r['age_ms']<=150 for r in rows)
    result={'kind':kind,'pose':pose,'peak_false_yaw_deg':peak,'final_drift_deg':abs(wrap((end-baseline)*D))/D,'healthy':healthy,'pass':peak<3 and healthy}
    self.results.append(result);print(json.dumps(result),flush=True)
 def mixed(self):
  cases=[('rest','walking'),('rest','counterlook'),('rifle','walking'),('rifle','pitch_up'),
   ('rifle','pitch_down'),('two_handed_pistol','pitch_up'),('two_handed_pistol','pitch_down'),
   ('right_pistol','reach'),('left_pistol','reach'),('one_hand_high','reach')]
  for pose,kind in cases:
   baseline=self.prepare(pose);label=pose+'/'+kind;begin=len(self.rows);self.phase=label;start=time.monotonic();last=0
   while (t:=time.monotonic()-start)<3:
    body=60*D*math.sin(t*2*math.pi/3);last=body;head=0 if kind=='counterlook' else body
    pitch=(89 if kind=='pitch_up' else -89 if kind=='pitch_down' else 0)*D
    gesture=None
    if kind=='walking':
     def gesture(ps,wr,t=t):
      ps[0][2]-=.15*math.sin(t*4);ps[1][2]+=.15*math.sin(t*4);return ps,wr
    if kind=='reach':
     def gesture(ps,wr,t=t):
      ps[1][1]+=.15*math.sin(t*3);ps[1][2]-=.08*math.sin(t*3);return ps,wr
    self.pose(pose,body,head,pitch,gesture=gesture)
   end=self.settled(pose,0,label+'/settled');rows=self.rows[begin:]
   peak=max(abs(wrap((r['offset_deg']-baseline-r['expected_deg'])*D))/D for r in rows)
   error=abs(wrap((end-baseline)*D))/D
   result={'kind':kind,'pose':pose,'moving_error_deg':peak,'settled_error_deg':error,'pass':error<3 and peak<25}
   self.results.append(result);print(json.dumps(result),flush=True)
 def run(self):
  error=None;restore_errors=[];poses_restored=False
  try:
   h=read_status(self.directory)['head_tracking'];self.original_head={**h['position'],**{k:h[k] for k in ('yaw','pitch','roll')}}
   assert all(abs(self.original_head[k])<1e-4 for k in ('yaw','pitch','roll')),'Start with a neutral simulator view to restore exact IPC controller offsets'
   self.neck=add([self.original_head[k] for k in ('x','y','z')],[0,-.1,.08])
   atomic(self.a.out.with_suffix('.ini.before'),self.saved_ini)
   self.mode(1);self.hold(.4);f=self.frame()
   assert f['valid'] and all(f['tracked']),'raw tracking unavailable'
   self.original_frame=f
   for h in (0,1):
    yaw,pitch=euler(f['rotations'][h]);q=quat(yaw,pitch)
    assert min(math.dist(q,f['rotations'][h]),math.dist([-x for x in q],f['rotations'][h]))<.001,'controller roll cannot be restored by IPC'
    self.original_hands.append(dict(hand=h,posX=f['hands'][h][0],posY=f['hands'][h][1],posZ=f['hands'][h][2],yaw=yaw,pitch=pitch))
   self.a.out.with_suffix('.restore.json').write_text(json.dumps({'head':self.original_head,'hands':self.original_hands,'mode':self.original_mode},indent=2))
   poses=self.a.poses.split(',') if self.a.poses else list(POSES)
   if hasattr(self,'custom_suite'):self.custom_suite()
   elif self.a.case=='negative':self.negatives(poses)
   elif self.a.case=='mixed':self.mixed()
   elif self.a.case=='hold':
    pose=poses[0];self.prepare(pose);self.sweep(pose,0,60*D,90,pose+'/turn')
    self.settled(pose,60*D,pose+'/settled');self.phase='weapon_rebind_hold'
    self.a.out.with_suffix('.ready').write_text('ready')
    print('READY: nonzero body offset held for external weapon-swap requests',flush=True)
    deadline=time.monotonic()+90
    while time.monotonic()<deadline and not self.a.out.with_suffix('.stop').exists():self.hold(.05)
    self.sweep(pose,60*D,0,90,pose+'/return');self.settled(pose,0,pose+'/returned')
   else:
    for pose in poses:
     self.prepare(pose)
     for speed in ([45] if self.a.case=='smoke' else [5,30,90,180,360]):
      for sign in (-1,1):self.turn_case(pose,speed,sign)
   self.phase='final';self.hold(.2)
  except BaseException as e:error=repr(e)
  finally:
   # Restore poses before returning the original mode; only its own setting is
   # replaced, preserving any other settings changed by hand in the meantime.
   try:
    self.clear_pending()
    if self.alive():
     if self.batch_active and len(self.original_hands)==2:
      self.tracking(self.original_head,self.original_hands);self.hold(.2);self.tracking()
     else:
      if self.original_head:self.head(self.original_head)
      for c in self.original_hands:self.hand(c)
     self.hold(.4)
     if self.original_frame is not None:
      restored=self.frame();assert restored['valid'] and all(restored['tracked']),'restored tracking unavailable'
      for h in (0,1):
       assert math.dist(restored['hands'][h],self.original_frame['hands'][h])<.002,'controller position was not restored'
       q0,q1=restored['rotations'][h],self.original_frame['rotations'][h]
       assert min(math.dist(q0,q1),math.dist(q0,[-x for x in q1]))<.002,'controller rotation was not restored'
      if restored['origin']==self.original_frame['origin']:
       assert math.dist(restored['position'],self.original_frame['position'])<.002,'head position was not restored'
     poses_restored=True
   except BaseException as e:restore_errors.append('poses: '+repr(e))
   try:self.mode(self.original_mode)
   except BaseException as e:restore_errors.append('setting: '+repr(e))
   self.clear_pending();self.r.close()
   report={'pid':self.a.pid,'symbols':self.symbols,'types':self.types,'case':self.a.case,'original_head':self.original_head,'original_hands':self.original_hands,
    'restored_mode':self.original_mode,'poses_restored':poses_restored,'rows':self.rows,'commands':self.commands,'results':self.results,'error':error,'restore_errors':restore_errors,
    'game_memory_writes':False,'focus_changed':False,'ipc_is_sequential':not bool(self.batch_runtime),'batch_runtime':self.batch_runtime,'mode_events':self.mode_events}
   self.a.out.write_text(json.dumps(report),encoding='utf-8')
  print(json.dumps({'cases':len(self.results),'failed':sum(not r['pass'] for r in self.results),'error':error,'restore_errors':restore_errors,'out':str(self.a.out)}),flush=True)
  if error or restore_errors:raise RuntimeError(error or str(restore_errors))
  if any(not r['pass'] for r in self.results):raise RuntimeError('Matrix has failures; inspect telemetry')

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--dll',type=Path,required=True)
 p.add_argument('--types',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
 p.add_argument('--batch-runtime',type=Path,help='Exact loaded private simulator DLL with atomic three-device IPC')
 p.add_argument('--game',type=Path,default=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'))
 p.add_argument('--case',choices=['smoke','matrix','negative','mixed','hold'],required=True);p.add_argument('--poses')
 a=p.parse_args();assert not a.out.exists() and a.out.parent.is_dir();Probe(a).run()
