"""Read candidate native PSOs and a bounded draw-binding snapshot."""
import argparse
import base64
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import sys
import time

root=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(root/'tools/swimming_tests'),str(root/'tools/roomscale_tests')]
from live_symbols import resolve
from live_read import Reader
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--pid',required=True,type=int);parser.add_argument('--frames',type=int,default=2,choices=range(1,5))
parser.add_argument('--out',required=True,type=Path);args=parser.parse_args()
symbols=resolve(args.pid,root/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll',
                ['CyberpunkVR_NativeStereo*','g_menuModeValue'])
reader=Reader(args.pid,r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
k=reader.k;k.WriteProcessMemory.argtypes=[w.HANDLE,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)];k.WriteProcessMemory.restype=w.BOOL
handle=k.OpenProcess(0x1028,False,args.pid);assert handle
addr=lambda name:int(symbols[name],16)
read=lambda name:struct.unpack('<I',reader.read(addr(name),4))[0]
state='CyberpunkVR_NativeStereoProbeState';request='CyberpunkVR_NativeStereoProbeRequest';seq='CyberpunkVR_NativeStereoProbeSeq'
def write(name,value):
    value=c.c_uint32(value);done=c.c_size_t()
    if not k.WriteProcessMemory(handle,addr(name),c.byref(value),4,c.byref(done)) or done.value!=4:raise c.WinError(c.get_last_error())
armed=False
try:
    assert read('CyberpunkVR_NativeStereoDrawBytes')==1096 and read('CyberpunkVR_NativeStereoPipelineBytes')==80,'Probe ABI mismatch'
    n=read('CyberpunkVR_NativeStereoPipelineCount');assert n<=8
    raw=reader.read(addr('CyberpunkVR_NativeStereoPipelines'),80*n) if n else b'';pipelines=[]
    for i in range(n):
        p=struct.unpack_from('<3Qi13I',raw,i*80)
        pipelines.append({'original':hex(p[0]),'instanced':hex(p[1]),'root':hex(p[2]),'result':p[3],
            'targets':p[4],'depthFormat':p[5],'samples':p[6],'cameraRoot':p[7],'cameraTableOffset':p[8],'formats':list(p[9:])})
    rows=[];full=False
    if n:
        assert read('g_menuModeValue')==0,'Load gameplay first'
        assert read(state)!=1 and read(request)==0,'Another probe is active'
        write(request,args.frames);armed=True;deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            if read(request)==0 and read(state) in (2,3):break
            time.sleep(.01)
        else:raise TimeoutError('Draw probe did not finish within 8 seconds')
        before=read(seq);count=read('CyberpunkVR_NativeStereoDrawCount');assert count<=4096 and not(before&1)
        raw=reader.read(addr('CyberpunkVR_NativeStereoDraws'),count*1096);assert before==read(seq);full=read(state)==3
        for i in range(count):
            b=raw[i*1096:(i+1)*1096];header=struct.unpack_from('<5Q6Ii7I',b)
            row=dict(zip(('order','qpc','list','pipeline','root','frame','node','side','indices','instances','firstIndex',
                          'baseVertex','firstInstance','vertexMask','tableMask','cbvMask','flags','rtCount','rtContiguous'),header))
            for name in ('list','pipeline','root','node'):row[name]=hex(row[name])
            row['index']=list(struct.unpack_from('<Q2I',b,96))
            row['vertices']={str(slot):list(struct.unpack_from('<Q2I',b,112+slot*16)) for slot in range(16) if row['vertexMask']&(1<<slot)}
            row['tables']={str(slot):hex(struct.unpack_from('<Q',b,368+slot*8)[0]) for slot in range(32) if row['tableMask']&(1<<slot)}
            row['cbvs']={str(slot):hex(struct.unpack_from('<Q',b,624+slot*8)[0]) for slot in range(32) if row['cbvMask']&(1<<slot)}
            row['targets']=[hex(x) for x in struct.unpack_from('<8Q',b,880)];row['depth']=hex(struct.unpack_from('<Q',b,944)[0]);rows.append(row)
            row['viewport']=list(struct.unpack_from('<6f',b,952));row['scissor']=list(struct.unpack_from('<4i',b,976))
            resource,gpu,size,valid,heap=struct.unpack_from('<3Q2I',b,992)
            row.update(instanceResource=hex(resource),instanceGpuBase=hex(gpu),instanceBytesTotal=size,instanceDataBytes=valid,instanceHeapType=heap)
            assert valid<=64
            if valid:row['instanceBase64']=base64.b64encode(b[1024:1024+valid]).decode()
            row['thread']=struct.unpack_from('<I',b,1088)[0]
    result={'pid':args.pid,'dll':symbols['sha256'],'assetStatus':read('CyberpunkVR_NativeStereoProbeAssetStatus'),'pipelines':pipelines,'full':full,'draws':rows}
    args.out.parent.mkdir(parents=True,exist_ok=True);args.out.write_text(json.dumps(result,indent=2))
    print(json.dumps({'pid':args.pid,'assetStatus':result['assetStatus'],'pipelines':pipelines,'draws':len(rows),'eyes':{side:sum(r['side']==side for r in rows) for side in (0,1)},'full':full,'out':str(args.out)},indent=2))
finally:
    if armed:write(request,0);write(state,0)
    k.CloseHandle(handle);reader.close()
