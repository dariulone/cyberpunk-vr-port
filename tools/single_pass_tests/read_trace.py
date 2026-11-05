"""Arm the bounded CPU trace in a matching live DLL and save an immutable copy."""
import argparse
import base64
import ctypes as c
from ctypes import wintypes as w
import json
import re
import struct
import sys
import time
from pathlib import Path

root=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(root/'tools/swimming_tests'))
sys.path.insert(0,str(root/'tools/roomscale_tests'))
from live_symbols import resolve
from live_read import Reader
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--pid',type=int,required=True)
parser.add_argument('--frames',type=int,default=3,choices=range(1,5))
parser.add_argument('--out',type=Path,required=True)
parser.add_argument('--native-upload',action='store_true',help='Compare the opt-in native 2048-byte camera upload/binding path')
args=parser.parse_args()
dll=root/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll'
symbols=resolve(args.pid,dll,['CyberpunkVR_SinglePassTrace*','CyberpunkVR_SinglePassTwin*','g_menuModeValue'])
reader=Reader(args.pid,r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
k=reader.k;k.WriteProcessMemory.argtypes=[w.HANDLE,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
k.WriteProcessMemory.restype=w.BOOL;handle=k.OpenProcess(0x1028,False,args.pid)
assert handle
addr=lambda name:int(symbols[name],16)
read=lambda name:struct.unpack('<I',reader.read(addr(name),4))[0]
state='CyberpunkVR_SinglePassTraceState';request='CyberpunkVR_SinglePassTraceRequest';seq='CyberpunkVR_SinglePassTraceSeq';count='CyberpunkVR_SinglePassTraceCount'
def write(name,value):
    value=c.c_uint32(value);done=c.c_size_t()
    if not k.WriteProcessMemory(handle,addr(name),c.byref(value),4,c.byref(done)) or done.value!=4:raise c.WinError(c.get_last_error())
armed=False;twinArmed=False
try:
    assert read('CyberpunkVR_SinglePassTraceRecordBytes')==4616,'Trace reader/DLL ABI mismatch'
    assert read('g_menuModeValue')==0,'Load gameplay first'
    assert read(state)!=1 and read(request)==0,'Another trace is active'
    twinNames=['CyberpunkVR_SinglePassTwin'+name for name in ('Allocations','Bindings','Failures')]
    beforeTwins=[read(name) for name in twinNames]
    if args.native_upload:
        assert read('CyberpunkVR_SinglePassTwinUpload')==0,'Another native upload probe is active'
        write('CyberpunkVR_SinglePassTwinUpload',1);twinArmed=True
    write(request,args.frames);armed=True;deadline=time.monotonic()+8
    while time.monotonic()<deadline:
        if read(request)==0 and read(state) in (2,3):break
        time.sleep(.01)
    else:raise TimeoutError('Trace did not finish within 8 seconds')
    before=read(seq);n=read(count);assert n<=512 and not(before&1)
    stride=4616
    raw=reader.read(addr('CyberpunkVR_SinglePassTraceRecords'),n*stride)
    assert before==read(seq)
    names={int(rva,16):name for rva,name in re.findall(r'\{\s*(0x[0-9A-Fa-f]+),\s*"([^"]+)"',(root/'src/Stereo/NodeNames.inc').read_text())}
    rows=[]
    for i in range(n):
        data=raw[i*stride:(i+1)*stride]
        order,qpc,obj,frame,node,kind,index,side,thread=struct.unpack_from('<3Q4IiI',data)
        row={'order':order,'qpc':qpc,'object':hex(obj),'frame':frame,'node':hex(node),'name':names.get(node,'?'),'kind':kind,'index':index,'side':side,'thread':thread}
        if kind==1:
            row['cameraBase64']=base64.b64encode(data[48:896]).decode()
            context,view,camera,width,height,valid,reason=struct.unpack_from('<3Q4I',data,896)
            row.update(context=hex(context),view=hex(view),cameraAddress=hex(camera),width=width,height=height,inputValid=bool(valid),predictionReason=reason)
            if valid:
                row['inputBase64']=base64.b64encode(data[936:1840]).decode()
                row['historyHeaderBase64']=base64.b64encode(data[1840:1856]).decode()
            prediction_valid,prediction_counter=struct.unpack_from('<2I',data,1856)
            row.update(predictionValid=bool(prediction_valid),predictionCounter=prediction_counter)
            if prediction_valid:row['predictionBase64']=base64.b64encode(data[1864:2768]).decode()
            shader_valid,previous_valid=struct.unpack_from('<2I',data,2768)
            row.update(shaderPredictionValid=bool(shader_valid),previousInputValid=bool(previous_valid))
            if shader_valid:row['shaderPredictionBase64']=base64.b64encode(data[2776:3624]).decode()
            if previous_valid:row['previousInputBase64']=base64.b64encode(data[3624:4528]).decode()
            row['validShaderWordMask']=[hex(x) for x in struct.unpack_from('<4Q',data,4528)]
            if prediction_valid:row['peerSourcePoseBase64']=base64.b64encode(data[4560:4592]).decode()
            cpu,gpu,byte_count,peer_side=struct.unpack_from('<2Q2I',data,4592)
            row.update(twinCpuDescriptor=hex(cpu),twinGpuAddress=hex(gpu),twinBytes=byte_count,predictionPeerSide=peer_side)
        rows.append(row)
    frequency=c.c_int64();k.QueryPerformanceFrequency(c.byref(frequency))
    twinDelta={name:read(symbol)-value for name,symbol,value in zip(('allocations','bindings','failures'),twinNames,beforeTwins)}
    result={'pid':args.pid,'dll':symbols['sha256'],'framesRequested':args.frames,'full':read(state)==3,'qpcFrequency':frequency.value,'nativeTwinUpload':args.native_upload,'twinDelta':twinDelta,'records':rows}
    args.out.parent.mkdir(parents=True,exist_ok=True);args.out.write_text(json.dumps(result,indent=2))
    print(json.dumps({'pid':args.pid,'records':n,'cameraBlocks':sum(x['kind']==1 for x in rows),'full':result['full'],'twinDelta':twinDelta,'out':str(args.out)}))
finally:
    if armed:write(request,0);write(state,0)
    if twinArmed:write('CyberpunkVR_SinglePassTwinUpload',0)
    k.CloseHandle(handle);reader.close()
