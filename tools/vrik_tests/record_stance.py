"""Read-only steady solved-pose recording; no HMD commands or game writes.

Use fresh debugger symbols in --addresses. The before/after boundary captures
are separate x64dbg software-breakpoint records, not inferred from this stream.
"""
from pathlib import Path
import argparse,json,struct,sys,time,hashlib
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'roomscale_tests'))
from live_read import Reader
p=argparse.ArgumentParser();p.add_argument('--addresses',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--seconds',type=float,default=2.5)
args=p.parse_args();assert 0<args.seconds<=10
cfg=json.loads(args.addresses.read_text());a={k:int(v,16) for k,v in cfg.items() if isinstance(v,str) and v.startswith('0x')}
args.out.mkdir(parents=True,exist_ok=True)
dataPath=args.out/'solved-model-stream.bin';metaPath=args.out/'solved-model-stream.json';assert not dataPath.exists() and not metaPath.exists()
r=Reader(cfg['pid'],Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe'),a['CyberpunkVR_RoomscaleDebug'],a['CyberpunkVR_RoomscaleDebugSeq'])
def raw(name,size):return r.read(a[name],size)
def unpack(name,fmt):return struct.unpack(fmt,raw(name,struct.calcsize(fmt)))
n=unpack('g_VRBoneCount','<i')[0];assert 25<n<=2048
owned=list(raw('g_VRUpperOwned',n));shadows=unpack('g_VRShadowSource','<'+str(n)+'i')
expected=sum(bool(x) or y>=0 for x,y in zip(owned,shadows));rows=[];payload=bytearray();skipped=0
started=time.monotonic();last=None
try:
 while time.monotonic()-started<args.seconds:
  tick=unpack('g_solveCacheTick','<I')[0];stamp=unpack('s_modelCacheCapturedMs','<Q')[0]
  if (tick,stamp)==last or unpack('g_solveCacheN','<i')[0]!=expected or raw('g_solveCacheModel',1)!=b'\x01':time.sleep(.004);continue
  pos=raw('s_modelCachePos',n*12);rot=raw('s_modelCacheRot',n*16);hands=raw('g_handsStable',128*4)
  if tick!=unpack('g_solveCacheTick','<I')[0] or stamp!=unpack('s_modelCacheCapturedMs','<Q')[0] or pos!=raw('s_modelCachePos',n*12) or rot!=raw('s_modelCacheRot',n*16):skipped+=1;continue
  rows.append({'elapsed_s':time.monotonic()-started,'tick':tick,'stamp_ms':stamp,'offset':len(payload),'hands_sequence':unpack('g_handsStableSeq','<I')[0],
    'rig_revision':unpack('s_rigRevision','<Q')[0],'armed':bool(raw('g_hasWeaponEquipped',1)[0]),'aiming':bool(raw('g_isAiming',1)[0]),
    'entity_position':unpack('CyberpunkVR_PlayerEntityPos','<3f'),'entity_rotation':unpack('CyberpunkVR_PlayerEntityQuat','<4f'),
    'raw_hand_targets':unpack('g_VRHandRawModel','<6f'),'raw_hand_rotations':unpack('g_VRHandRawRot','<8f'),'hands_snapshot':list(struct.unpack('<128f',hands))})
  payload.extend(pos);payload.extend(rot);last=(tick,stamp);time.sleep(.01)
finally:r.close()
assert len(rows)>=20,'not enough complete poses'
dataPath.write_bytes(payload)
meta={'pid':cfg['pid'],'bone_count':n,'owned_count':expected,'owned_mask':owned,'shadow_sources':shadows,'record_bytes':n*28,'record_layout':'n float3 model positions, then n float4 model rotations; only owned/shadow entries meaningful','skipped_in_progress':skipped,'frames':rows,'data_sha256':hashlib.sha256(payload).hexdigest(),'boundary':'Repeated equal cache reads and unchanged tick/stamp; hands/entity are separate observations, not asserted as one invocation'}
metaPath.write_text(json.dumps(meta,indent=2)+'\n')
print(json.dumps({'pid':cfg['pid'],'bone_count':n,'owned_count':expected,'frames':len(rows),'armed_values':list({x['armed'] for x in rows}),'skipped_in_progress':skipped,'data_sha256':meta['data_sha256']},indent=2))
