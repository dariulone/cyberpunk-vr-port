"""Bounded input/camera identity inspection. Only toggles/restores diagnostic gate."""
import argparse,ctypes as c,json,struct,sys,time
from ctypes import wintypes as w
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'tools/roomscale_tests'),str(ROOT/'tools/swimming_tests')]
from live_symbols import resolve
from live_read import Reader
from motion_probe import camera
p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--out',type=Path,required=True)
p.add_argument('--dll',type=Path,default=ROOT/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll')
a=p.parse_args();assert not a.out.exists();a.out.parent.mkdir(exist_ok=True,parents=True)
names=['CyberpunkVR_FramegenReport','CyberpunkVR_FramegenReportSeq','CyberpunkVR_FramegenInputStages','CyberpunkVR_RuntimeDiagnostics',
       'CyberpunkVR_PoseIdLastCapture','CyberpunkVR_PoseIdCaptureMiss','CyberpunkVR_PoseIdCaptured',
       'cvr::framegen::cameras','cvr::framegen::pools']
s=resolve(a.pid,a.dll.resolve(),names);addr={n:int(s[n],16) for n in names}
r=Reader(a.pid,
 r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
k=c.WinDLL('kernel32',use_last_error=True);k.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];k.OpenProcess.restype=w.HANDLE
k.WriteProcessMemory.argtypes=[w.HANDLE,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)];k.CloseHandle.argtypes=[w.HANDLE]
h=k.OpenProcess(0x28,False,a.pid);assert h;gate=addr['CyberpunkVR_RuntimeDiagnostics'];old=r.read(gate,4)
def write(data):
 b=c.create_string_buffer(data);n=c.c_size_t();assert k.WriteProcessMemory(h,gate,b,len(data),c.byref(n)) and n.value==len(data)
def stable(address,size):
 for _ in range(10):
  one=r.read(address,size)
  if one==r.read(address,size):return one
 return None
rows=[];result={'symbols':s,'rows':rows}
def report():
 for _ in range(30):
  seq=struct.unpack('<I',r.read(addr['CyberpunkVR_FramegenReportSeq'],4))[0]
  if seq&1:continue
  raw=r.read(addr['CyberpunkVR_FramegenReport'],2048).split(b'\0')[0]
  if seq!=struct.unpack('<I',r.read(addr['CyberpunkVR_FramegenReportSeq'],4))[0]:continue
  if raw:return json.loads(raw)
 return None
try:
 write(struct.pack('<I',1));until=time.monotonic()+3
 while time.monotonic()<until:
  row={'time':time.time(),'report':report(),
       'stages':struct.unpack('<10Q',r.read(addr[names[2]],80)),
       'captured_pose':struct.unpack('<2Q',r.read(addr['CyberpunkVR_PoseIdLastCapture'],16)),
       'capture_miss':struct.unpack('<2Q',r.read(addr['CyberpunkVR_PoseIdCaptureMiss'],16)),
       'captured':struct.unpack('<2Q',r.read(addr['CyberpunkVR_PoseIdCaptured'],16)),'cameras':[],'inputs':[]}
  data=stable(addr['cvr::framegen::cameras'],2*8*112)
  if data:
   for view in range(2):
    for i in range(8):
     offset=(view*8+i)*112
     if data[offset+92]:row['cameras'].append(dict(view=view+1,frame=struct.unpack_from('<I',data,offset+88)[0],
           pose=struct.unpack_from('<Q',data,offset+96)[0],origin=struct.unpack_from('<Q',data,offset+104)[0],camera=camera(data,offset)))
  data=stable(addr['cvr::framegen::pools'],128)
  if data:
   for view in range(2):
    for i in range(4):
     pointer=struct.unpack_from('<Q',data,(view*4+i)*16)[0]
     if not pointer:continue
     try:value=stable(pointer,200)
     except OSError:continue
     if not value:continue
     frame,v,width,height,mw,mh=struct.unpack_from('<6I',value,152)
     if v!=view+1 or not 16<=width<=16384:continue
     row['inputs'].append(dict(view=v,frame=frame,pose=struct.unpack_from('<Q',value,120)[0],origin=struct.unpack_from('<Q',value,128)[0],
       submitted=bool(value[178]),evaluated=bool(value[179]),expected=value[192],completed=value[193],invalid=bool(value[194]),
       depth=bool(value[176]),motion=bool(value[177]),width=width,height=height,queue=hex(struct.unpack_from('<Q',value,24)[0])))
  rows.append(row);time.sleep(.04)
finally:
 write(old);result['gate_restored']=r.read(gate,4)==old;k.CloseHandle(h);r.close()
 a.out.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
last=rows[-1];print(json.dumps({**last,'cameras':[{k:v for k,v in c.items() if k!='camera'} for c in last['cameras']],
 'stage_delta':[b-a for a,b in zip(rows[0]['stages'],last['stages'])]},indent=2))
