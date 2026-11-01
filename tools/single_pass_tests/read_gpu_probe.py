"""Run one opt-in offscreen in-game stereo draw comparison, then clear controls."""
import argparse
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
from startup_guard import require_normal_window
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--pid',required=True,type=int);p.add_argument('--out',required=True,type=Path)
p.add_argument('--native-reference',action='store_true',help='Compare against each eye\'s real native draw and late bindings')
p.add_argument('--rearm-completed',action='store_true',help='Rearm only a checked/empty/incomplete result whose GPU work has been released')
p.add_argument('--group',action='store_true',help='Compare every draw of the validated material in one native packet-consumer call')
p.add_argument('--main-view-mask',type=lambda value:int(value,0),default=0xffffffff,help='Per-source-draw MAIN visibility bits for the private group experiment')
p.add_argument('--main-reference-mask',type=lambda value:int(value,0),default=0xffffffff,help='Per-MAIN-draw reference bits for isolating private group differences')
p.add_argument('--prepare-gate-ms',type=int,default=0,choices=range(0,51),help='Bounded diagnostic GPU gate, released by MAIN preparation or the CPU watchdog')
p.add_argument('--late-visibility',action='store_true',help='Derive current-frame per-eye masks by matching actual MAIN draws before releasing GPU')
p.add_argument('--scene-depth',action='store_true',help='Full-resolution group resolve against each eye native depth/stencil and prior MRT contents')
p.add_argument('--scene-route',action='store_true',help='Replace this one native group in the visible scene, retaining conditional original draws as fallback')
p.add_argument('--gpu-times',action='store_true',help='Collect bounded GPU timestamps for draws, imports, resolve, copy-back and diagnostic readback')
p.add_argument('--direct-resolve',action='store_true',help='Resolve directly into native targets, eliminating the private resolved array and copy-back')
p.add_argument('--mixed-materials',action='store_true',help='Include adjacent validated shader variants while targets and stencil policy remain unchanged')
p.add_argument('--fine-rate',action='store_true',help='Compare private draws at 1x1 shading and restore native VRS after each draw')
p.add_argument('--source-rate-image',action='store_true',help='Compare private MAIN draws against the frozen VRCAM shading-rate image')
p.add_argument('--depth-prepass',action='store_true',help='Compare the opaque depth-only prepass; colour shading and native VRS remain separate')
p.add_argument('--start-indices',type=int,default=0,help='Select a later depth prefix by its first mesh index count; MAIN must match that exact anchor')
p.add_argument('--depth-run',type=int,default=0,help='Select a complete source run of the supported depth PSO (1-based, 0 means first available)')
args=p.parse_args()
symbols=resolve(args.pid,root/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll',
    ['CyberpunkVR_SinglePass*','CyberpunkVR_NativeStereo*','CyberpunkVR_StereoGpu*','CyberpunkVR_GeometryPacket*','g_menuModeValue'])
reader=Reader(args.pid,r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe',0,0)
k=reader.k;k.WriteProcessMemory.argtypes=[w.HANDLE,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)];k.WriteProcessMemory.restype=w.BOOL
handle=k.OpenProcess(0x1028,False,args.pid);assert handle
addr=lambda name:int(symbols[name],16)
read=lambda name:struct.unpack('<I',reader.read(addr(name),4))[0]
def write(name,value):
    data=c.c_uint32(value);done=c.c_size_t()
    if not k.WriteProcessMemory(handle,addr(name),c.byref(data),4,c.byref(done)) or done.value!=4:raise c.WinError(c.get_last_error())
controls=['CyberpunkVR_StereoGpuProbeRequest','CyberpunkVR_SinglePassTraceRequest','CyberpunkVR_NativeStereoProbeRequest',
          'CyberpunkVR_SinglePassTwinUpload','CyberpunkVR_SinglePassTraceState','CyberpunkVR_NativeStereoProbeState','CyberpunkVR_StereoGpuProbeNativeReference','CyberpunkVR_StereoGpuProbeGroup','CyberpunkVR_StereoGpuProbePrepareGateMs','CyberpunkVR_StereoGpuProbeLateVisibility','CyberpunkVR_StereoGpuProbeSceneDepth','CyberpunkVR_StereoGpuProbeSceneRoute','CyberpunkVR_StereoGpuProbeTiming','CyberpunkVR_StereoGpuProbeMixedMaterials']
counterNames=['CyberpunkVR_SinglePassTwin'+s for s in ('Allocations','Bindings','Failures')]
armed=False;report=None
controls.append('CyberpunkVR_StereoGpuProbeFineRate')
controls.append('CyberpunkVR_StereoGpuProbeDepthPrepass')
controls.append('CyberpunkVR_StereoGpuProbeStartIndices')
controls.append('CyberpunkVR_StereoGpuProbeStartRun')
if args.depth_prepass:controls+=['CyberpunkVR_GeometryPacketRequest','CyberpunkVR_GeometryPacketResolve']
try:
    assert not args.group or args.native_reference,'Group mode requires native references'
    assert 0<=args.main_view_mask<=0xffffffff and 0<=args.main_reference_mask<=0xffffffff
    assert args.group or (args.main_view_mask==args.main_reference_mask==0xffffffff),'Masks require group mode'
    assert not args.prepare_gate_ms or args.group,'Preparation gate requires group mode'
    assert not args.late_visibility or (args.group and args.prepare_gate_ms and args.main_view_mask==args.main_reference_mask==0xffffffff),'Late matching requires a gate and default masks'
    assert not args.scene_depth or args.late_visibility,'Scene resolve requires automatic per-eye matching'
    assert not args.scene_route or args.scene_depth,'Visible routing requires scene-depth comparison'
    assert not args.gpu_times or args.native_reference,'GPU timing requires native eye references'
    assert not args.direct_resolve or args.scene_route,'Direct resolve requires scene routing'
    assert not args.mixed_materials or (args.scene_depth and read('CyberpunkVR_NativeStereoStaticAssetStatus')==1),'Mixed materials need native-depth comparison and the validated static VS asset'
    assert not args.fine_rate or (args.scene_depth and not args.scene_route),'Fine-rate diagnosis must use private scene targets'
    assert not args.source_rate_image or (args.scene_depth and not args.scene_route and not args.fine_rate),'Source-rate diagnosis must use private scene targets'
    assert not args.depth_prepass or (args.scene_depth and not args.mixed_materials and not args.fine_rate and not args.source_rate_image),'Depth prepass requires the native-depth scene path'
    assert 0<=args.start_indices<=0xffffffff and (not args.start_indices or args.depth_prepass),'Mesh anchor requires depth mode'
    assert 0<=args.depth_run<=128 and (not args.depth_run or args.depth_prepass),'Run selection requires depth mode'
    assert read('g_menuModeValue')==0,'Load gameplay first'
    game_windows=require_normal_window(args.pid)
    assert read('CyberpunkVR_SinglePassTraceRecordBytes')==4616 and read('CyberpunkVR_NativeStereoDrawBytes')==1096,'ABI mismatch'
    prior_state=read('CyberpunkVR_StereoGpuProbeState')
    assert prior_state==0 or (args.rearm_completed and prior_state in (3,5,6)), 'GPU probe is in flight, failed, or already completed without explicit rearm'
    assert read('CyberpunkVR_SinglePassTwinUpload')==0 and all(read(name)==0 for name in controls[:3]),'A request is pending'
    assert read('CyberpunkVR_SinglePassTraceState')!=1 and read('CyberpunkVR_NativeStereoProbeState')!=1,'A recorder is active'
    if args.depth_prepass:
        assert read('CyberpunkVR_GeometryPacketState')!=1 and read('CyberpunkVR_GeometryPacketRequest')==0 and read('CyberpunkVR_GeometryPacketResolve')==0,'Instance upload observer is already owned'
    count=read('CyberpunkVR_NativeStereoPipelineCount');assert 0<count<=8,'No candidate PSO'
    pipelines=reader.read(addr('CyberpunkVR_NativeStereoPipelines'),count*80)
    assert any(struct.unpack_from('<Q',pipelines,i*80+8)[0] and struct.unpack_from('<i',pipelines,i*80+24)[0]==0 for i in range(count)),'No accepted view-instanced PSO'
    before=[read(name) for name in counterNames];armed=True
    assert before[0]<=100,'Keep space in the bounded 128-descriptor probe heap'
    if prior_state:write('CyberpunkVR_StereoGpuProbeState',0)
    write('CyberpunkVR_StereoGpuProbeNativeReference',int(args.native_reference))
    write('CyberpunkVR_StereoGpuProbeGroup',int(args.group))
    write('CyberpunkVR_StereoGpuProbeMainViewMask',args.main_view_mask);write('CyberpunkVR_StereoGpuProbeMainReferenceMask',args.main_reference_mask)
    write('CyberpunkVR_StereoGpuProbePrepareGateMs',args.prepare_gate_ms)
    write('CyberpunkVR_StereoGpuProbeLateVisibility',int(args.late_visibility))
    write('CyberpunkVR_StereoGpuProbeSceneDepth',int(args.scene_depth))
    write('CyberpunkVR_StereoGpuProbeSceneRoute',2 if args.direct_resolve else int(args.scene_route))
    write('CyberpunkVR_StereoGpuProbeTiming',int(args.gpu_times))
    write('CyberpunkVR_StereoGpuProbeMixedMaterials',int(args.mixed_materials))
    write('CyberpunkVR_StereoGpuProbeFineRate',2 if args.source_rate_image else int(args.fine_rate))
    write('CyberpunkVR_StereoGpuProbeDepthPrepass',int(args.depth_prepass))
    write('CyberpunkVR_StereoGpuProbeStartIndices',args.start_indices)
    write('CyberpunkVR_StereoGpuProbeStartRun',args.depth_run)
    if args.depth_prepass:
        # Start observing the native DEFAULT instance buffer before its upload.
        # The bounded packet capture owns/invalidates that CPU shadow; no GPU wait.
        write('CyberpunkVR_GeometryPacketResolve',2);write('CyberpunkVR_GeometryPacketRequest',3)
    write('CyberpunkVR_SinglePassTwinUpload',1);write('CyberpunkVR_StereoGpuProbeRequest',1)
    write('CyberpunkVR_NativeStereoProbeRequest',3);write('CyberpunkVR_SinglePassTraceRequest',2)
    deadline=time.monotonic()+8
    while time.monotonic()<deadline:
        state=read('CyberpunkVR_StereoGpuProbeState')
        if state in (3,4,5,6):break
        time.sleep(.01)
    else:state=read('CyberpunkVR_StereoGpuProbeState')
    # Stop metadata writers before copying their immutable diagnostic storage.
    for name in controls:write(name,0)
    args.out.parent.mkdir(parents=True,exist_ok=True)
    for prefix,record,limit in (('SinglePassTrace',4616,512),('NativeStereo',1096,4096)):
        seq='CyberpunkVR_'+('SinglePassTraceSeq' if prefix=='SinglePassTrace' else 'NativeStereoProbeSeq')
        countName='CyberpunkVR_'+('SinglePassTraceCount' if prefix=='SinglePassTrace' else 'NativeStereoDrawCount')
        dataName='CyberpunkVR_'+('SinglePassTraceRecords' if prefix=='SinglePassTrace' else 'NativeStereoDraws')
        for attempt in range(100):
            version=read(seq)
            if version&1:time.sleep(.001);continue
            n=read(countName);assert n<=limit;blob=reader.read(addr(dataName),n*record) if n else b''
            if version==read(seq):break
        else:raise RuntimeError('Recorder did not settle')
        args.out.with_name(args.out.stem+'-'+prefix+'.bin').write_bytes(blob)
    report={'pid':args.pid,'dll':symbols['sha256'],'state':state,'nativeReference':args.native_reference,
        'gameWindows':game_windows,
        'group':args.group,'depthPrepass':args.depth_prepass,'startIndices':args.start_indices,'depthRun':args.depth_run,'mixedMaterials':args.mixed_materials,'sourceDraws':read('CyberpunkVR_StereoGpuProbeSourceDraws'),'mainDraws':read('CyberpunkVR_StereoGpuProbeMainDraws'),
        'lateVisibility':args.late_visibility,'matchedDraws':read('CyberpunkVR_StereoGpuProbeMatchedDraws'),'fallbackDraws':read('CyberpunkVR_StereoGpuProbeFallbackDraws'),
        'mainViewMask':hex(args.main_view_mask),'mainReferenceMask':hex(args.main_reference_mask),
        'referenceSkipped':read('CyberpunkVR_StereoGpuProbeReferenceSkipped'),
        'result':struct.unpack('<i',reader.read(addr('CyberpunkVR_StereoGpuProbeResult'),4))[0],
        'differentBytes':struct.unpack('<Q',reader.read(addr('CyberpunkVR_StereoGpuProbeDifferentBytes'),8))[0] if state in (3,5) else None,
        'comparedBytes':struct.unpack('<Q',reader.read(addr('CyberpunkVR_StereoGpuProbeComparedBytes'),8))[0] if state in (3,5) else None,
        'coverage':list(struct.unpack('<2Q',reader.read(addr('CyberpunkVR_StereoGpuProbeCoverage'),16))) if state in (3,5) else None,
        'queueWaits':read('CyberpunkVR_StereoGpuProbeQueueWaits') if 'CyberpunkVR_StereoGpuProbeQueueWaits' in symbols else None,
        'twinDelta':{name:read(symbol)-prior for name,symbol,prior in zip(('allocations','bindings','failures'),counterNames,before)}}
    if state in (3,5):
        report['eyeDifferences']=list(struct.unpack('<2Q',reader.read(addr('CyberpunkVR_StereoGpuProbeEyeDifferences'),16)))
        report['targetDifferences']=list(struct.unpack('<4Q',reader.read(addr('CyberpunkVR_StereoGpuProbeTargetDifferences'),32)))
        n=read('CyberpunkVR_StereoGpuProbeDifferenceCount');assert n<=128
        samples=reader.read(addr('CyberpunkVR_StereoGpuProbeDifferences'),n*28) if n else b''
        report['differenceSamples']=[dict(zip(('target','eye','plane','x','y','reference','shared'),values)) for values in struct.iter_unpack('<7I',samples)]
    report['sceneDepth']={'enabled':args.scene_depth,'reject':read('CyberpunkVR_StereoGpuProbeSceneReject'),
        'size':list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeSceneSize'),8))),
        'phase':list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeScenePhase'),8))),
        'samples':list(struct.unpack('<2Q',reader.read(addr('CyberpunkVR_StereoGpuProbeSceneSamples'),16))) if state in (3,5) else None,
        'routeRequested':args.scene_route,'directResolve':args.direct_resolve,'committed':read('CyberpunkVR_StereoGpuProbeSceneCommitted'),
        'routedDraws':list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeSceneRouted'),8)))}
    rate=struct.unpack('<8I',reader.read(addr('CyberpunkVR_StereoGpuProbeVrs'),32))
    report['vrs']={'forceFine':args.fine_rate,'native':[list(rate[:4]),list(rate[4:])],
        'images':[hex(v) for v in struct.unpack('<2Q',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsImage'),16))],
        'stateChanges':list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsChanges'),8)))}
    report['vrs'].update(sourceImageOverride=args.source_rate_image,
        size=list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsSize'),8))),
        captured=list(struct.unpack('<2I',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsCaptured'),8))),
        differentTexels=struct.unpack('<Q',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsDifference'),8))[0],
        histograms=[list(struct.unpack('<16Q',reader.read(addr('CyberpunkVR_StereoGpuProbeVrsHistogram')+eye*128,128))) for eye in range(2)])
    if args.gpu_times and state in (3,5):
        stages=('nativeReference','commonDraw','importReference' if args.direct_resolve else 'importReferenceAndResolve','resolve','copyBack','diagnosticReadback','nativeFallback')
        frequencies=struct.unpack('<2Q',reader.read(addr('CyberpunkVR_StereoGpuProbeFrequency'),16))
        ticks=struct.unpack('<14Q',reader.read(addr('CyberpunkVR_StereoGpuProbeTicks'),112))
        counts=struct.unpack('<14I',reader.read(addr('CyberpunkVR_StereoGpuProbeTimedCount'),56))
        report['gpuTiming']={'frequencies':list(frequencies),'dropped':read('CyberpunkVR_StereoGpuProbeTimingDrops'),
            'stages':{name:{'count':list(counts[i*2:i*2+2]),'ms':[ticks[i*2+eye]*1000/frequencies[eye] if frequencies[eye] else None for eye in range(2)]} for i,name in enumerate(stages)}}
    k.QueryPerformanceFrequency.argtypes=[c.POINTER(c.c_int64)];k.QueryPerformanceFrequency.restype=w.BOOL
    frequency=c.c_int64();assert k.QueryPerformanceFrequency(c.byref(frequency)) and frequency.value>0
    timestamps={name:struct.unpack('<Q',reader.read(addr('CyberpunkVR_StereoGpuProbePrepareGate'+name),8))[0] for name in ('Start','Main','Release')}
    report['prepareGate']={'requestedMs':args.prepare_gate_ms,'reason':read('CyberpunkVR_StereoGpuProbePrepareGateReason'),
        'mainMs':(timestamps['Main']-timestamps['Start'])*1000/frequency.value if timestamps['Main'] and timestamps['Start'] else None,
        'releaseMs':(timestamps['Release']-timestamps['Start'])*1000/frequency.value if timestamps['Release'] and timestamps['Start'] else None}
    args.out.write_text(json.dumps(report,indent=2));print(json.dumps({k:v for k,v in report.items() if k!='differenceSamples'},indent=2))
finally:
    if armed:
        for name in controls:write(name,0)
        write('CyberpunkVR_StereoGpuProbeMainViewMask',0xffffffff);write('CyberpunkVR_StereoGpuProbeMainReferenceMask',0xffffffff)
    k.CloseHandle(handle);reader.close()
assert report and report['state']==3 and report['differentBytes']==0 and all(report['coverage']),{k:v for k,v in (report or {}).items() if k!='differenceSamples'}
if args.late_visibility:assert report['matchedDraws']>0 and report['prepareGate']['reason']==1,'No shared draws or GPU released by the watchdog'
if args.scene_depth:assert all(report['sceneDepth']['samples']),'No passing group samples in one or both eyes'
if args.scene_route:assert report['sceneDepth']['committed']==1 and report['sceneDepth']['routedDraws']==[report['sourceDraws'],report['mainDraws']],'Visible group did not replace all selected native draws'
if args.gpu_times:assert report['gpuTiming']['dropped']==0 and all(report['gpuTiming']['frequencies']),'Incomplete GPU timing'
