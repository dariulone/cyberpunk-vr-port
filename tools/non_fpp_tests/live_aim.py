import argparse,ctypes as c,json,math,struct,sys,time
from ctypes import wintypes as w
from pathlib import Path
root=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(root/'tools/swimming_tests'),str(root/'tools/roomscale_tests')]
from live_symbols import resolve
from live_read import Reader
p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--seconds',type=float,default=20)
p.add_argument('--dll',type=Path,default=root/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll');a=p.parse_args();assert not a.out.exists()
names=['CyberpunkVR_PanzerWeaponAimDebug','CyberpunkVR_PanzerWeaponAimDebugSeq','CyberpunkVR_RuntimeDiagnostics','CyberpunkVR_PanzerCannonDebug','CyberpunkVR_PanzerCannonDebugSeq']
s=resolve(a.pid,a.dll.resolve(),names);r=Reader(a.pid,r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
k=c.WinDLL('kernel32',use_last_error=True);k.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];k.OpenProcess.restype=w.HANDLE
k.WriteProcessMemory.argtypes=[w.HANDLE,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)];k.CloseHandle.argtypes=[w.HANDLE]
h=k.OpenProcess(0x28,False,a.pid);assert h;gate=int(s[names[2]],16);old=r.read(gate,4)
def write(data):
 b=c.create_string_buffer(data);n=c.c_size_t();assert k.WriteProcessMemory(h,gate,b,len(data),c.byref(n)) and n.value==len(data)
def shot():
 for _ in range(40):
  seq=r.read(int(s[names[4]],16),4)
  if struct.unpack('<I',seq)[0]&1:continue
  data=r.read(int(s[names[3]],16),96)
  if seq!=r.read(int(s[names[4]],16),4):continue
  v=struct.unpack('<5Q12fI4x',data)
  row=dict(calls=v[0],applied=v[1],owner=hex(v[2]),current_main_pose=v[3],stamp_us=v[4],position=v[5:8],native_forward=v[8:11],forward=v[11:14],target=v[14:17],reason=v[17])
  if row['calls'] and not row['reason']:
   delta=[a-b for a,b in zip(row['target'],row['position'])];length=math.sqrt(sum(x*x for x in delta));fn=math.sqrt(sum(x*x for x in row['forward']))
   if length>0 and fn>0:row['target_error_degrees']=math.degrees(math.acos(max(-1,min(1,sum(x*y for x,y in zip(delta,row['forward']))/length/fn))))
  return row
 raise RuntimeError('cannon snapshot stayed busy')
def snap():
 for _ in range(40):
  before=r.read(int(s[names[1]],16),4)
  if struct.unpack('<I',before)[0]&1:continue
  data=r.read(int(s[names[0]],16),128)
  if before!=r.read(int(s[names[1]],16),4):continue
  v=struct.unpack('<7Q17fI',data)
  row=dict(time=time.time(),calls=v[0],applied=v[1],position_writes=v[2],rotation_writes=v[3],owner=hex(v[4]),pose=v[5],stamp_us=v[6],
    native_position=v[7:10],native_rotation=v[10:14],position=v[14:17],rotation=v[17:21],target=v[21:24],reason=v[24])
  if row['calls'] and not row['reason']:
   x,y,z,qw=row['rotation'];f=[2*(x*y-z*qw),1-2*(x*x+z*z),2*(y*z+x*qw)]
   d=[a-b for a,b in zip(row['target'],row['position'])];length=math.sqrt(sum(x*x for x in d));fn=math.sqrt(sum(x*x for x in f))
   if length>0 and fn>0:row.update(angle_degrees=math.degrees(math.acos(max(-1,min(1,sum(x*y for x,y in zip(d,f))/length/fn)))),distance=length)
  row['shot']=shot();return row
 raise RuntimeError('aim diagnostic stayed busy')
rows=[];result={'symbols':s,'rows':rows}
try:
 write(struct.pack('<I',1));end=time.monotonic()+a.seconds
 while time.monotonic()<end:rows.append(snap());time.sleep(.025)
finally:
 write(old);result['gate_restored']=r.read(gate,4)==old;k.CloseHandle(h);r.close()
 if rows:
  result['delta']={n:rows[-1][n]-rows[0][n] for n in ['calls','applied','position_writes','rotation_writes']}
  angles=[x['angle_degrees'] for x in rows if 'angle_degrees' in x]
  result['max_angle_degrees']=max(angles) if angles else None
  result['reasons']=sorted(set(x['reason'] for x in rows))
  result['shot_delta']={n:rows[-1]['shot'][n]-rows[0]['shot'][n] for n in ['calls','applied']}
  shot_errors=[x['shot']['target_error_degrees'] for x in rows if 'target_error_degrees' in x['shot']]
  result['max_shot_target_error_degrees']=max(shot_errors) if shot_errors else None
 a.out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k not in ['rows','symbols']},indent=2))
