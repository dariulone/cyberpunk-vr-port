"""Continuous depth routing with fence-safe target reuse and no bulk draw trace."""
import ctypes as c
import json
import struct
import time
from statistics import median

from hud_readiness import wait_for_stable_hud
from paired_interval import validate_stats,validate_schedule,schedule_profile
from early_depth_report import MAX_SOURCES,MAX_INSTANCE_ROWS,validate_camera_stages,validate_skinning
from observation_profile import decode as decode_observer

STATS=('considered','recorded','completed','admitted','fallback','created','reused','busy','constantDraws',
       'mainSources','peerSources','executed','references','maxPool','cancelled','failure')
FIELDS=('frame','selected','matched','mainSources','peerSources','mainCommands','peerUnconditional',
        'peerOptimized','peerFallback','decision','cpuAdmitted','references','mainTicks','mainFrequency','peerTicks','peerFrequency')
CPU_PHASES=('membership','mainPlan','mainEmit','peerMatchAndEmit','drawTotal')


def frame_cadence(rows,starts,frequency):
    if len(rows)!=len(starts) or len(rows)<2 or frequency<=0:raise ValueError('Incomplete frame cadence')
    ordered=sorted(zip((r['frame'] for r in rows),starts))
    if any(b[0]!=a[0]+1 or b[1]<=a[1] or a[1]<=0 for a,b in zip(ordered,ordered[1:])):
        raise ValueError('Frame cadence requires consecutive paired roots and increasing timestamps')
    periods=[(b[1]-a[1])*1000.0/frequency for a,b in zip(ordered,ordered[1:])]
    return dict(intervals=len(periods),medianMs=median(periods),meanMs=sum(periods)/len(periods),
                minMs=min(periods),maxMs=max(periods),pairsPerSecond=len(periods)*1000.0/sum(periods),
                limitation='Cadence between consecutive paired MAIN depth entries, including the rest of the experimental frame and observer overhead; not native-only FPS or GPU critical-path time.')


def cpu_profile(values,frequency):
    if len(values)!=10 or frequency<=0 or any(v<0 for v in values):raise ValueError('Invalid continuous CPU profile')
    return {name:dict(calls=values[i*2],milliseconds=values[i*2+1]*1000.0/frequency) for i,name in enumerate(CPU_PHASES)}


def validate_origin(origin,draws,skinning_calls=0):
    before,after=origin['before'],origin['after']
    if (origin['compatible']!=1 or origin['verified']!=1 or len(before)!=4 or len(after)!=4 or
            before[0]<128 or after[1] or any(b<a for a,b in zip(before,after))):
        raise RuntimeError('Native draw origin has not matched the legacy unwind')
    delta=[b-a for a,b in zip(before,after)]
    if not 0<=skinning_calls<=draws:raise RuntimeError('Invalid skinning origin call count')
    if origin['scoped']:
        if delta[0] or delta[3]!=skinning_calls or delta[2]<draws-skinning_calls:
            raise RuntimeError('Scoped/static and legacy/skinning origin coverage is incomplete')
    elif delta[0] or delta[2] or delta[3]<draws:
        raise RuntimeError('Legacy origin baseline is not isolated')


def validate(report,frames,cancel_after=0):
    s=report['stats'];rows=report['rows'];p=report['preparation']
    skin=report.get('skinning',{})
    validate_origin(report['origin'],s['constantDraws'],skin['stats'][0] if skin.get('enabled') else 0)
    if report['state']!=2 or s['failure'] or report['liveFailure'] or p['RunState']!=2 or p['RunFailure']:
        raise RuntimeError('Continuous depth interval failed')
    if report.get('geometryChecks')!=[2,0]:raise RuntimeError('Geometry-only input projection was not verified for both eyes')
    proofs=report.get('instanceProofs')
    if (not proofs or len(proofs)!=2 or not 0<=proofs[0]<=s['constantDraws'] or proofs[1]<proofs[0]*48 or proofs[1]%48):
        raise RuntimeError('Invalid GPU instance proof coverage')
    storage=report.get('descriptorStorage',{})
    if not isinstance(storage.get('dense'),bool) or storage.get('active')!=int(storage['dense']):
        raise RuntimeError('Descriptor storage mode was not applied')
    metadata=report.get('liveMetadata',{})
    if (not isinstance(metadata.get('enabled'),bool) or metadata.get('samples',0)<1 or
            metadata.get('mismatches')!=0 or metadata.get('afterActive')!=0):
        raise RuntimeError('Live descriptor metadata was not verified or did not restore capture mode')
    upload=report.get('uploadLookup',{});counts=upload.get('counts',[])
    if (not isinstance(upload.get('enabled'),bool) or len(counts)!=6 or any(n<0 for n in counts) or
            (upload['enabled'] and any(counts[i]<s['constantDraws'] for i in (0,2,4,5))) or
            (not upload['enabled'] and (any(counts[:3]+counts[4:]) or counts[3]<s['constantDraws']))):
        raise RuntimeError('Upload lookup route was not exercised or baseline was not isolated')
    camera=report.get('nativeCamera',dict(enabled=False,stats=[0,0]))
    validate_camera_stages(camera,s['recorded'])
    if camera['stats']!=([s['recorded'],s['constantDraws']] if camera['enabled'] else [0,0]):
        raise RuntimeError('Native camera upload/draw coverage is incomplete')
    if (not 0<s['recorded']<=frames or s['considered']!=s['recorded'] or s['busy'] or s['recorded']!=s['completed'] or
            len(rows)!=s['completed'] or s['admitted']+s['fallback']!=s['completed'] or not s['admitted'] or
            not 1<=s['created']<=3 or s['created']+s['reused']!=s['recorded'] or s['reused']<1 or
            s['maxPool']!=s['created'] or s['references'] or not report['traceCountsUnchanged']):
        raise RuntimeError('Continuous depth coverage, pooling or trace isolation is incomplete')
    if cancel_after:
        n=s['considered']
        if not s['cancelled'] or not cancel_after<=n<frames:raise RuntimeError('Cancellation did not stop a running interval')
        for name in ('RootSkipped','PairMerged','PairCompleted','RunEyeCopies','RunEyeSubmissions'):
            if p[name]!=n:raise RuntimeError('Cancellation left a partial paired root')
        if p['RunFrames'] not in (n,n+1) or p['DepthMain'] not in (n,n+1) or p['DepthVrcam'] not in (n,n+1):
            raise RuntimeError('Cancellation exceeded one native tail frame')
    else:
        if s['considered']!=frames or s['cancelled']:raise RuntimeError('Continuous interval ended early')
        validate_stats(p,frames)
    if len({r['frame'] for r in rows})!=len(rows):raise RuntimeError('Repeated depth frame identity')
    if report.get('cadence')!=frame_cadence(rows,report.get('frameStarts',[]),report['cpu']['frequency']):
        raise RuntimeError('Reported cadence differs from native frame-start timestamps')
    for r in rows:
        if (not 0<=r['selected']<MAX_SOURCES or r['matched']>r['selected'] or r['mainSources']<r['selected'] or
                r['decision'] not in (0,1) or r['cpuAdmitted'] not in (0,1) or r['references'] or
                (r['decision'] and (not r['selected'] or not r['cpuAdmitted'] or r['matched']!=r['selected'])) or
                (not r['selected'] and (r['cpuAdmitted'] or r['peerUnconditional']!=r['peerSources'] or r['peerFallback'] or r['peerOptimized'])) or
                r['peerUnconditional']+r['peerFallback']!=r['peerSources'] or
                not all(r[k]>0 for k in ('mainTicks','mainFrequency','peerTicks','peerFrequency'))):
            raise RuntimeError('Invalid per-frame GPU admission or command accounting')
    arena_ok=report['arenas']==[0,0] if camera['enabled'] else (1<=report['arenas'][0]<=8*s['created'] and
        report['arenas'][0]<s['constantDraws'] and report['arenas'][1]>=s['reused'])
    if (not arena_ok or
            sum(r['selected'] for r in rows)!=s['constantDraws'] or sum(r['decision'] for r in rows)!=s['admitted'] or
            sum(r['mainSources'] for r in rows)!=s['mainSources'] or sum(r['peerSources'] for r in rows)!=s['peerSources'] or
            sum(r['mainCommands']+r['peerUnconditional']+(r['peerOptimized'] if r['decision'] else r['peerFallback']) for r in rows)!=s['executed']):
        raise RuntimeError('Continuous interval totals differ from GPU-completed rows')


def run(pid,game,frames,check,inspect,path,cancel_after=0,scoped_origin=True,combined_instances=True,dense_descriptors=True,profile_descriptors=False,native_camera=False,live_metadata=True,fast_uploads=True,camera_stages=1,skinning=False):
    from live_read import Reader
    from live_symbols import resolve
    if not 30<=frames<=1200 or not 0<=cancel_after<frames:raise ValueError('Invalid continuous depth budget')
    if camera_stages not in (1,3):raise ValueError('Invalid native camera stages')
    if skinning and not native_camera:raise ValueError('Skinning requires native camera')
    prefixes=['CyberpunkVR_EarlyDepth*','CyberpunkVR_Preparation*','CyberpunkVR_NativeStereo*',
              'CyberpunkVR_GeometryPacket*','CyberpunkVR_GeometryOrigin*','CyberpunkVR_DescriptorDrawCount','CyberpunkVR_DescriptorBindingCount',
              'CyberpunkVR_DescriptorDense*','CyberpunkVR_DescriptorLiveMetadata*','CyberpunkVR_ObservationProfile*',
              'CyberpunkVR_ConstantUploadFast*',
              'CyberpunkVR_ConstantCaptureEnable','g_menuModeValue']
    symbols=resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',prefixes)
    reader=Reader(pid,game/'bin/x64/Cyberpunk2077.exe',0,0);k=reader.k;handle=None;armed=False;origin_mode=None;instance_mode=None;descriptor_mode=None;metadata_mode=None;upload_mode=None;profiling=False
    read=lambda name:struct.unpack('<I',reader.read(int(symbols[name],16),4))[0]
    early=lambda name:read('CyberpunkVR_EarlyDepth'+name)
    prep=lambda name:read('CyberpunkVR_Preparation'+name)
    origin=lambda:list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_GeometryOriginStats'],16),32)))
    observer=lambda:list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_ObservationProfileStats'],16),128)))
    upload_stats=lambda:list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_ConstantUploadFastStats'],16),48)))
    trace_names=('CyberpunkVR_PreparationCount','CyberpunkVR_PreparationPacketCount','CyberpunkVR_NativeStereoDrawCount',
                 'CyberpunkVR_DescriptorDrawCount','CyberpunkVR_DescriptorBindingCount','CyberpunkVR_GeometryPacketCount','CyberpunkVR_GeometryPacketDataCount')
    trace=lambda:{name:read(name) for name in trace_names}
    stats=lambda:dict(zip(STATS,struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunStats'],16),128))))
    def preparation():
        result={name:prep(name) for name in ('RunState','RunFailure','RunFrames','RootSkipped','PairMerged','PairCompleted','RunEyeCopies','RunEyeSubmissions')}
        result['DepthMain'],result['DepthVrcam']=struct.unpack('<2I',reader.read(int(symbols['CyberpunkVR_PreparationRunDepth'],16),8))
        return result
    def write(name,value,prefix='CyberpunkVR_EarlyDepth'):
        data,done=c.c_uint32(value),c.c_size_t()
        if not k.WriteProcessMemory(handle,int(symbols[prefix+name],16),c.byref(data),4,c.byref(done)) or done.value!=4:
            raise c.WinError(c.get_last_error())
    def active():
        check()
        if early('RunState')!=1 or stats()['failure'] or read('g_menuModeValue'):
            raise RuntimeError(f'Depth interval not active in gameplay: {stats()}')
        metadata=report['liveMetadata'];metadata['samples']+=1
        if read('CyberpunkVR_DescriptorLiveMetadataActive')!=int(live_metadata):
            metadata['mismatches']+=1;raise RuntimeError('Unexpected descriptor metadata mode during continuous depth')
        if read('CyberpunkVR_ConstantUploadFastPath')!=int(fast_uploads):raise RuntimeError('Upload lookup mode changed during the interval')
        if early('CameraStages')!=camera_stages:raise RuntimeError('Camera stage mode changed during the interval')
        if early('Skinning')!=int(skinning):raise RuntimeError('Skinning mode changed during the interval')
        return True
    report=dict(pid=pid,dll=symbols['sha256'],requestedFrames=frames,cancelAfter=cancel_after,passed=False,images=[])
    try:
        if early('SourceCapacity')!=MAX_SOURCES or early('InstanceCapacity')!=MAX_INSTANCE_ROWS:
            raise ValueError('Early depth publication capacity ABI mismatch')
        if early('CameraStages')!=1:raise RuntimeError('Camera stages must start at their default')
        instance_mode=early('InstanceReadScope')
        if instance_mode!=1:raise RuntimeError('Instance read scope must start in its default mode')
        report['combinedInstanceReads']=combined_instances
        descriptor_mode=read('CyberpunkVR_DescriptorDenseSlots')
        if descriptor_mode!=1 or read('CyberpunkVR_DescriptorDenseActive')!=1 or read('CyberpunkVR_ObservationProfileEnable'):
            raise RuntimeError('Descriptor storage/profile controls are not idle')
        report['descriptorStorage']=dict(dense=dense_descriptors)
        metadata_mode=read('CyberpunkVR_DescriptorLiveMetadata')
        if metadata_mode!=1 or read('CyberpunkVR_DescriptorLiveMetadataActive'):
            raise RuntimeError('Live metadata controls are not idle')
        report['liveMetadata']=dict(enabled=live_metadata,samples=0,mismatches=0)
        upload_mode=read('CyberpunkVR_ConstantUploadFastPath')
        if read('CyberpunkVR_ConstantUploadFastStatsCount')!=6:raise RuntimeError('Upload lookup counter ABI mismatch')
        if upload_mode!=1:raise RuntimeError('Upload lookup must start in its default mode')
        report['uploadLookup']=dict(enabled=fast_uploads)
        if (any(early(n) for n in ('Request','State','RunRequest','RunState','RunStop','WholeTarget','RejectCommit','NativeCamera','Skinning')) or
                any(prep(n) for n in ('Request','PairRequest','RunRequest','RunState','ScheduleMode')) or prep('State')==1 or
                any(prep(n)!=1 for n in ('PairVerified','RootCompatible','PairCompatible','ScheduleCompatible')) or
                any(read(n) for n in ('CyberpunkVR_NativeStereoProbeRequest','CyberpunkVR_NativeStereoProbeState',
                    'CyberpunkVR_NativeStereoLiveBindings','CyberpunkVR_NativeStereoLiveFailure','CyberpunkVR_ConstantCaptureEnable','g_menuModeValue'))):
            raise RuntimeError('Depth interval requires idle controls and verified reference/schedule warmup')
        k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
        k.WaitForSingleObject.argtypes=[c.c_void_p,c.c_uint32]
        handle=k.OpenProcess(0x101028,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        schedule_before=list(struct.unpack('<16I',reader.read(int(symbols['CyberpunkVR_PreparationScheduleStats'],16),64)))
        schedule_cache=prep('ScheduleCache')
        if schedule_cache not in (0,1):raise RuntimeError('Unknown CPU schedule cache mode')
        schedule_cpu_before=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_PreparationScheduleCpu'],16),48)))
        schedule_frequency=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_PreparationScheduleCpuFrequency'],16),8))[0]
        origin_mode=read('CyberpunkVR_GeometryOriginMode')
        report['origin']=dict(scoped=scoped_origin,before=origin(),compatible=read('CyberpunkVR_GeometryOriginCompatible'),
                              verified=read('CyberpunkVR_GeometryOriginVerified'))
        o=report['origin']
        if origin_mode!=1 or o['compatible']!=1 or o['verified']!=1 or o['before'][0]<128 or o['before'][1]:
            raise RuntimeError(f'Origin requires a clean 128-draw reference comparison: {o}')
        report['traceBefore']=trace();armed=True
        write('LiveMetadata',int(live_metadata),'CyberpunkVR_Descriptor')
        write('FastPath',int(fast_uploads),'CyberpunkVR_ConstantUpload')
        # Let calls already inside the old route drain before taking counters.
        time.sleep(.2);check();report['uploadLookup']['before']=upload_stats()
        write('DenseSlots',int(dense_descriptors),'CyberpunkVR_Descriptor')
        until=time.monotonic()+3
        while read('CyberpunkVR_DescriptorDenseActive')!=int(dense_descriptors):
            check()
            if time.monotonic()>until:raise TimeoutError('Descriptor storage mode did not apply')
            time.sleep(.01)
        report['descriptorStorage']['active']=read('CyberpunkVR_DescriptorDenseActive')
        if profile_descriptors:
            report['descriptorProfile']=dict(before=observer())
            profiling=True;write('Enable',1,'CyberpunkVR_ObservationProfile')
        write('InstanceReadScope',int(combined_instances))
        write('NativeCamera',int(native_camera))
        write('CameraStages',camera_stages)
        write('Skinning',int(skinning))
        write('Mode',2 if scoped_origin else 0,'CyberpunkVR_GeometryOrigin')
        write('ScheduleMode',2,'CyberpunkVR_Preparation');write('RunRequest',frames)
        deadline=time.monotonic()+50;started=time.monotonic()+4
        while early('RunRequest') or early('RunState')!=1:
            check()
            if early('RunState')==3 or time.monotonic()>started:raise RuntimeError(f'Depth interval did not start: {stats()}')
            time.sleep(.01)
        report['hud']=wait_for_stable_hud(pid,game,10,active)
        report['images'].append(dict(stats=stats(),image=inspect('active-0')));active()
        target=cancel_after if cancel_after else stats()['completed']+frames//3
        while stats()['completed']<target:
            active()
            if time.monotonic()>deadline:raise TimeoutError('Continuous depth progress stalled')
            time.sleep(.05)
        if cancel_after:write('RunStop',1)
        else:report['images'].append(dict(stats=stats(),image=inspect('active-1')));active()
        while early('RunState')==1 or read('CyberpunkVR_NativeStereoLiveBindings'):
            check()
            if time.monotonic()>deadline:raise TimeoutError('Continuous depth did not drain')
            time.sleep(.05)
        if profiling:
            write('Enable',0,'CyberpunkVR_ObservationProfile');profiling=False
            time.sleep(.1)
            profile=report['descriptorProfile'];profile['after']=observer();profile['frequency']=schedule_frequency
            profile['phases']=decode_observer(profile['before'],profile['after'],schedule_frequency)
        report.update(state=early('RunState'),stats=stats(),preparation=preparation(),liveFailure=read('CyberpunkVR_NativeStereoLiveFailure'),
                      arenas=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunArenas'],16),16))),
                      deferred=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunDeferred'],16),16))))
        seq,count=early('RunSeq'),early('RunCount')
        if seq&1 or count>1200:raise RuntimeError('Incomplete depth row publication')
        raw=reader.read(int(symbols['CyberpunkVR_EarlyDepthRunRows'],16),count*128)
        report['frameStarts']=list(struct.unpack('<'+str(count)+'Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunFrameStarts'],16),count*8)))
        if early('RunSeq')!=seq:raise RuntimeError('Depth rows changed during read')
        report['rows']=[dict(zip(FIELDS,row)) for row in struct.iter_unpack('<16Q',raw)]
        report['traceAfter']=trace();report['traceCountsUnchanged']=report['traceBefore']==report['traceAfter']
        report['geometryChecks']=list(struct.unpack('<2I',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunGeometryChecks'],16),8)))
        report['instanceProofs']=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunInstanceProofs'],16),16)))
        report['nativeCamera']=dict(enabled=native_camera,stats=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunNativeCamera'],16),16))))
        report['nativeCamera'].update(stages=camera_stages,stageMasks=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunCameraStageMasks'],16),32))))
        report['skinning']=dict(enabled=skinning,
            stats=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunSkinningStats'],16),48))),
            proofs=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunShaderProofs'],16),32))),
            bindingChecks=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunBindingChecks'],16),32))))
        validate_skinning(report['skinning'],True)
        report['liveMetadata']['afterActive']=read('CyberpunkVR_DescriptorLiveMetadataActive')
        report['uploadLookup']['after']=upload_stats()
        report['uploadLookup']['counts']=[b-a for a,b in zip(report['uploadLookup']['before'],report['uploadLookup']['after'])]
        frequency=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunCpuFrequency'],16),8))[0]
        cpu=list(struct.unpack('<10Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunCpu'],16),80)))
        report['cpu']=dict(raw=cpu,frequency=frequency,phases=cpu_profile(cpu,frequency),
                           limitation='Inclusive CPU wall intervals; main/peer phases are nested within drawTotal, not additional frame-time savings.')
        report['cadence']=frame_cadence(report['rows'],report['frameStarts'],frequency)
        schedule_after=list(struct.unpack('<16I',reader.read(int(symbols['CyberpunkVR_PreparationScheduleStats'],16),64)))
        schedule_cpu_after=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_PreparationScheduleCpu'],16),48)))
        report['schedule']=dict(before=schedule_before,after=schedule_after,cache=bool(schedule_cache),cpuBefore=schedule_cpu_before,
            cpuAfter=schedule_cpu_after,frequency=schedule_frequency,profile=schedule_profile(schedule_cpu_before,schedule_cpu_after,schedule_frequency))
        report['origin']['after']=origin();report['origin']['verified']=read('CyberpunkVR_GeometryOriginVerified')
        validate_schedule(schedule_before,schedule_after,report['preparation']['PairCompleted'],bool(schedule_cache))
        validate(report,frames,cancel_after);report['passed']=True
        return report
    except Exception as error:
        report['error']=str(error)
        try:report.update(state=early('RunState'),stats=stats(),preparation=preparation(),liveFailure=read('CyberpunkVR_NativeStereoLiveFailure'))
        except OSError:pass
        try:
            report['lastDepthStats']=list(struct.unpack('<16I',reader.read(int(symbols['CyberpunkVR_EarlyDepthStats'],16),64)))
            report['arenas']=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunArenas'],16),16)))
            report['deferred']=list(struct.unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunDeferred'],16),16)))
            report['lastFailureData']=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthFailureData'],16),48)))
            report['cpuRaw']=list(struct.unpack('<10Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRunCpu'],16),80)))
        except OSError:pass
        raise
    finally:
        try:
            if armed and handle and k.WaitForSingleObject(handle,0)!=0:
                if profiling:write('Enable',0,'CyberpunkVR_ObservationProfile');profiling=False
                write('RunRequest',0);write('RunRequest',0,'CyberpunkVR_Preparation');write('RunStop',1);write('RunStop',1,'CyberpunkVR_Preparation')
                until=time.monotonic()+5
                while early('RunState')==1 or prep('RunState')==1 or read('CyberpunkVR_NativeStereoLiveBindings'):
                    check()
                    if time.monotonic()>until:raise TimeoutError('Continuous depth cancellation did not drain')
                    time.sleep(.05)
                for prefix in ('CyberpunkVR_EarlyDepth','CyberpunkVR_Preparation'):
                    write('RunState',0,prefix);write('RunStop',0,prefix)
                write('ScheduleMode',0,'CyberpunkVR_Preparation');write('LiveFailure',0,'CyberpunkVR_NativeStereo')
                write('NativeCamera',0)
                write('CameraStages',1)
                write('Skinning',0)
                if upload_mode is not None:
                    write('FastPath',upload_mode,'CyberpunkVR_ConstantUpload')
                    if read('CyberpunkVR_ConstantUploadFastPath')!=upload_mode:raise RuntimeError('Upload lookup mode was not restored')
                if metadata_mode is not None:
                    write('LiveMetadata',metadata_mode,'CyberpunkVR_Descriptor')
                    if read('CyberpunkVR_DescriptorLiveMetadata')!=metadata_mode or read('CyberpunkVR_DescriptorLiveMetadataActive'):
                        raise RuntimeError('Live metadata mode was not restored')
                if origin_mode is not None:
                    write('Mode',origin_mode,'CyberpunkVR_GeometryOrigin')
                    if read('CyberpunkVR_GeometryOriginMode')!=origin_mode:raise RuntimeError('Origin mode was not restored')
                if instance_mode is not None:
                    write('InstanceReadScope',instance_mode)
                    if early('InstanceReadScope')!=instance_mode:raise RuntimeError('Instance read scope was not restored')
                if descriptor_mode is not None:
                    write('DenseSlots',descriptor_mode,'CyberpunkVR_Descriptor')
                    until=time.monotonic()+3
                    while read('CyberpunkVR_DescriptorDenseActive')!=descriptor_mode:
                        check()
                        if time.monotonic()>until:raise TimeoutError('Descriptor storage mode was not restored')
                        time.sleep(.01)
                if prep('PairRequest') or early('State') or read('CyberpunkVR_NativeStereoProbeState'):raise RuntimeError('Continuous depth left active controls')
                report['cleanup']='restored'
        except Exception as error:
            report['cleanupError']=str(error);report['passed']=False
            raise
        finally:
            if handle:k.CloseHandle(handle)
            reader.close();path.write_text(json.dumps(report,indent=2)+'\n')
