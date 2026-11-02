"""Bounded HMD bend trajectories; read-only game memory, simulator pose commands.

The initial simulator pose is restored in finally. No keys, weapon changes or
game-memory writes. Cache double reads reject updates in progress; scalar
diagnostics are adjacent observations, not a same-invocation breakpoint proof.
"""
from pathlib import Path
import argparse,json,math,os,struct,sys,time,uuid
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'roomscale_tests'))
from live_read import Reader
from simulator_probe import read_status,send,await_pose

p=argparse.ArgumentParser();p.add_argument('--addresses',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
p.add_argument('--case',choices=['look','bend','cone','vertical','floor','reach','onset','girdle'],required=True)
a=p.parse_args();assert not a.out.exists() and a.out.parent.is_dir()
cfg=json.loads(a.addresses.read_text());addr={k:int(v,16) for k,v in cfg.items() if isinstance(v,str) and v.startswith('0x')}
directory=Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator'
initial=read_status(directory);h=initial['head_tracking'];assert all(abs(h[k])<1e-4 for k in ['yaw','pitch','roll']), 'neutral simulator angles required'
original={**h['position'],'yaw':0.,'pitch':0.,'roll':0.};current=dict(original)
r=Reader(cfg['pid'],Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe'),addr['CyberpunkVR_RoomscaleDebug'],addr['CyberpunkVR_RoomscaleDebugSeq'])
def raw(name,n):return r.read(addr[name],n)
def get(name,fmt):return struct.unpack(fmt,raw(name,struct.calcsize(fmt)))
n=get('g_VRBoneCount','<i')[0];assert 25<n<=2048
names={int(line.split('\t')[0]):line.split('\t')[1] for line in (a.out.parent/'bone-names.txt').read_text().splitlines()[1:]};assert len(names)==n
indices=sorted(set(range(25))|{i for i,name in names.items() if
 (name.startswith('shadow_') and name[7:] in {'Hips','Spine3','LeftUpLeg','LeftLeg','LeftFoot','RightUpLeg','RightLeg','RightFoot','LeftShoulder','RightShoulder','LeftArm','RightArm','LeftForeArm','RightForeArm','LeftHand','RightHand','Head'}) or
 'scapula_' in name or '_SHL_' in name or name in {'LeftEye','RightEye','l_J_eye_JNT','r_J_eye_JNT'}})
rows=[];skipped=0;start=time.monotonic()
original_controllers=[];controller_commands=[]
def send_controller(value):
 # Verified against the active simulator's CheckControllerPoseCommand and
 # GetControllerPose: yaw-only position offsets, yaw/pitch angle offsets.
 # Omitting trigger/buttonA leaves all action state unchanged.
 target=directory/'controller_pose_command.json';tmp=directory/('controller_pose_command.'+uuid.uuid4().hex+'.writing')
 tmp.write_text(json.dumps(value),encoding='ascii')
 for _ in range(50):
  try:os.replace(tmp,target);break
  except PermissionError:time.sleep(.01)
 else:raise RuntimeError('controller command stayed locked')
 deadline=time.monotonic()+3
 while target.exists():
  if time.monotonic()>deadline:raise RuntimeError('controller command not consumed')
  time.sleep(.005)
 controller_commands.append(dict(time=time.monotonic()-start,**value))
def reach_controllers():
 peak=path(1,math.radians(65))
 t=max(0,min(1,(original['y']-current['y'])/(original['y']-peak['y'])))
 for hand,initial in enumerate(original_controllers):
  begin=(original['x']+initial['posX'],original['y']+initial['posY'],original['z']+initial['posZ'])
  goal=(original['x']+(-.2 if hand==0 else .2),.10,original['z']-.30)
  world=[x+(y-x)*t for x,y in zip(begin,goal)]
  dx,dy,dz=[world[i]-current[k] for i,k in enumerate(('x','y','z'))]
  c,s=math.cos(current['yaw']),math.sin(current['yaw'])
  send_controller({'hand':hand,'posX':dx*c+dz*s,'posY':dy,'posZ':-dx*s+dz*c,
   'yaw':initial['yaw']-current['yaw'],'pitch':initial['pitch']-current['pitch']})
def sample(label):
 global skipped
 for _ in range(15):
  tick=get('g_solveCacheTick','<I')[0];stamp=get('s_modelCacheCapturedMs','<Q')[0]
  count=get('g_solveCacheN','<i')[0]
  if count<=0 or raw('g_solveCacheModel',1)!=b'\x01':skipped+=1;time.sleep(.002);continue
  pos=raw('s_modelCachePos',n*12);rot=raw('s_modelCacheRot',n*16);owned=raw('s_modelCacheOwned',n)
  body=raw('g_VRBodyBone',132)
  if tick!=get('g_solveCacheTick','<I')[0] or stamp!=get('s_modelCacheCapturedMs','<Q')[0] or count!=get('g_solveCacheN','<i')[0] or sum(bool(x) for x in owned)!=count or pos!=raw('s_modelCachePos',n*12) or rot!=raw('s_modelCacheRot',n*16) or body!=raw('g_VRBodyBone',132):
   skipped+=1;time.sleep(.002);continue
  seq=get('g_handsStableSeq','<I')[0];hands=get('g_handsStable','<128f')
  if seq!=get('g_handsStableSeq','<I')[0]:skipped+=1;continue
  entry={'label':label,'time':time.monotonic()-start,'commanded':dict(current),'cache_tick':tick,'cache_stamp_ms':stamp,
   'cache_count':count,'hands_sequence':seq,'hands':hands,
   'bend_radians':get('g_VRIKBodyBendAngle','<f')[0],'pelvis_delta':get('g_VRIKBodyBendPelvis','<3f'),
   'cone_degrees':get('CyberpunkVR_BodyYawFollowDeadDeg','<f')[0],'body_realign':get('CyberpunkVR_BodyYawRealignRad','<f')[0],
   'body_yaw_error':get('CyberpunkVR_DebugBodyFollowErrDeg','<f')[0],
   'right_hand_target':get('g_VRIKDbgTarget','<3f'),'left_hand_target':get('g_VRIKDbgTargetL','<3f'),
   'body_bones':list(struct.iter_unpack('<3f',body)),'body_bones_valid':get('g_VRBodyBoneOk','<11i'),
   'model':{names[i]:{'position':struct.unpack_from('<3f',pos,i*12),'rotation':struct.unpack_from('<4f',rot,i*16)} for i in indices if owned[i]},
   'roomscale':r.sample()}
  if 'g_VRIKBodyEyeBound' in addr:
   entry.update(eye_bound=bool(raw('g_VRIKBodyEyeBound',1)[0]),eye_target=get('g_VRIKBodyEyeTarget','<3f'),
    eye_solved=get('g_VRIKBodyEyeSolved','<3f'),girdle=get('g_VRIKGirdleReach','<8f'),neck_mount=get('s_neckMount','<3f'))
   eyes=['l_J_eye_JNT','r_J_eye_JNT'] if 'l_J_eye_JNT' in entry['model'] else ['LeftEye','RightEye']
   if all(e in entry['model'] for e in eyes):
    eye=[sum(entry['model'][e]['position'][k] for e in eyes)*.5 for k in range(3)]
    entry['cache_eye_solved_error_mm']=math.dist(eye,entry['eye_solved'])*1000
  rows.append(entry)
  if entry['roomscale']['gates']!=15:raise RuntimeError('roomscale gate changed')
  if math.dist(entry['roomscale']['after'][:2],rows[0]['roomscale']['after'][:2])>.35:raise RuntimeError('body moved beyond bounded probe')
  return entry
 raise RuntimeError('unable to obtain stable cache')
def hold(label,seconds=.6):
 until=time.monotonic()+seconds
 while time.monotonic()<until:sample(label);time.sleep(.02)
def path(t,angle=0.,yaw=0.,vertical=0.):
 theta=angle*t;psi=yaw*t
 if a.case=='look':return {**original,'pitch':-theta}
 if a.case=='vertical':return {**original,'y':original['y']-vertical*t}
 # Lower the neck along a waist hinge AND pitch the HMD toward the floor.
 # Add the physical eye/neck lever so this is a head trajectory, not just
 # a synthetic neck trajectory fed into the optical HMD position.
 oy=.08*math.cos(theta)-.15*math.sin(theta)
 oz=-.08*math.sin(theta)-.15*math.cos(theta)
 return {**original,'x':original['x']+math.sin(psi)*oz,
  'y':original['y']+.63*(math.cos(theta)-1)+oy-.08-(.44*t if a.case=='reach' else .35*t if a.case=='floor' else .12*t if a.case=='onset' else 0),
  'z':original['z']-.63*math.sin(theta)*(.15 if a.case=='onset' else 1)+math.cos(psi)*oz+.15,'pitch':-theta,'yaw':psi}
def sweep(label,fn,seconds=1.8):
 global current
 begun=time.monotonic()
 while True:
  t=min(1.,(time.monotonic()-begun)/seconds);u=t*t*(3-2*t)
  current=fn(u);send(directory,current)
  if a.case=='reach':reach_controllers()
  sample(label)
  if t>=1:break
  time.sleep(.008)
 await_pose(directory,current)
report={'pid':cfg['pid'],'dll_sha256':cfg['dll_sha256'],'case':a.case,'initial':initial,'original':original,'samples':rows,
 'observation':'double-read stable cached model transforms plus adjacent scalar diagnostics; body publication has separate lifetime',
 'game_memory_writes':False,'keys_sent':False,'focus_changed':False}
try:
 hold('baseline');angle=math.radians(65 if a.case in ('floor','reach') else 50)
 if a.case in ('reach','girdle'):
  fresh=rows[-1]['hands']
  for hand in range(2):
   i=hand*8;assert fresh[i]==1,'controller unavailable before reach'
   x,y,z,w=fresh[i+4:i+8];pitch=math.asin(max(-1,min(1,2*(w*x-y*z))));yaw=math.atan2(2*(w*y+x*z),1-2*(x*x+y*y))
   cy,sy,cp,sp=math.cos(yaw/2),math.sin(yaw/2),math.cos(pitch/2),math.sin(pitch/2)
   expected=(cy*sp,sy*cp,-sy*sp,cy*cp)
   assert min(max(abs(v-u) for v,u in zip((x,y,z,w),expected)),max(abs(v+u) for v,u in zip((x,y,z,w),expected)))<1e-4,'cannot restore original controller roll'
   original_controllers.append({'hand':hand,'posX':fresh[i+1],'posY':fresh[i+2],'posZ':fresh[i+3],'yaw':yaw,'pitch':pitch})
  report['original_controllers']=original_controllers;report['controller_commands']=controller_commands
 if a.case=='girdle':
  # Near and far targets per hand, with the other hand kept close. Head stays
  # at its original pose; no trigger, grip or button is changed.
  near=[{**c,'posZ':-.2} for c in original_controllers]
  for c in near:send_controller(c)
  hold('near',.7)
  for hand in (1,0):
   name='right' if hand else 'left'
   for returning in (False,True):
    begun=time.monotonic()
    while True:
     t=min(1.,(time.monotonic()-begun)/1.2);u=t*t*(3-2*t);u=1-u if returning else u
     send_controller({**near[hand],'posZ':-.2-.4*u});sample(name+('_return' if returning else '_extend'))
     if t>=1:break
     time.sleep(.008)
    hold(name+('_near' if returning else '_far'),.7)
 elif a.case=='onset':
  for degrees in (5,10,15,20,25,30,35,40,35,30,25,20,15,10,5,0):
   current=path(degrees/40,math.radians(40));send(directory,current);await_pose(directory,current)
   hold('onset_%02d'%degrees,.3)
 elif a.case=='vertical':
  sweep('lower',lambda t:path(t,vertical=.35));hold('lowered');sweep('rise',lambda t:path(1-t,vertical=.35))
 else:
  sweep('lower_and_pitch' if a.case!='look' else 'look_down',lambda t:path(t,angle));hold('bent' if a.case!='look' else 'looked_down')
  if a.case=='cone':
   bent=path(1,angle)
   sweep('turn_while_bent',lambda t:{**bent,'yaw':math.radians(45)*t});hold('bent_turn')
   sweep('unturn_while_bent',lambda t:{**bent,'yaw':math.radians(45)*(1-t)})
  sweep('straighten',lambda t:path(1-t,angle))
 hold('returned',.8)
except BaseException as e:report['error']=f'{type(e).__name__}: {e}'
finally:
 try:
  current=dict(original);send(directory,original)
  for controller in original_controllers:send_controller(controller)
  report['restored']=await_pose(directory,original);hold('restored',.35)
  if original_controllers:
   final=rows[-1]['hands']
   report['controller_restore_max_position_error']=max(abs(final[h*8+k+1]-c[key]) for h,c in enumerate(original_controllers) for k,key in enumerate(('posX','posY','posZ')))
   if report['controller_restore_max_position_error']>.001:raise RuntimeError('controller restore not confirmed')
 except BaseException as e:report['cleanup_error']=f'{type(e).__name__}: {e}'
 r.close();report['skipped_reads']=skipped;a.out.write_text(json.dumps(report,indent=2)+'\n')
def dist(x,y):return math.dist(x['roomscale']['after'][:2],y['roomscale']['after'][:2])*1000
summary={'case':a.case,'samples':len(rows),'max_bend_degrees':max(math.degrees(x['bend_radians']) for x in rows),'max_packet_bend_degrees':max(math.degrees(x['hands'][114]) for x in rows),
 'max_cone_degrees':max(x['cone_degrees'] for x in rows),'max_body_travel_mm':max(dist(rows[0],x) for x in rows),'body_return_error_mm':dist(rows[0],rows[-1]),'end_bend_degrees':math.degrees(rows[-1]['bend_radians']),
 'cache_counts':sorted(set(x['cache_count'] for x in rows)),'max_body_realign_deg':max(abs(math.degrees(x['body_realign']-rows[0]['body_realign'])) for x in rows),
 'error':report.get('error'),'cleanup_error':report.get('cleanup_error')}
anchored=[x for x in rows if x.get('eye_bound') and x.get('cache_eye_solved_error_mm',1e9)<.5]
if anchored:
 summary['eye_samples_consistent_with_cache']=len(anchored)
 summary['max_eye_forward_lead_mm']=max((x['eye_solved'][1]-x['eye_target'][1])*1000 for x in anchored)
 summary['max_eye_above_target_mm']=max((x['eye_solved'][2]-x['eye_target'][2])*1000 for x in anchored)
 deep=[x for x in anchored if x['bend_radians']>math.radians(25)]
 summary['deep_eye_error_max_mm']=max((math.dist(x['eye_solved'],x['eye_target'])*1000 for x in deep),default=0)
print(json.dumps(summary,indent=2));a.out.with_suffix('.summary.json').write_text(json.dumps(summary,indent=2)+'\n')
if report.get('error') or report.get('cleanup_error'):raise SystemExit(1)
