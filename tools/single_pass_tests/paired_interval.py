"""Bounded paired roots without full per-frame packet tracing."""
import ctypes as c
import json
import struct
import time

from hud_readiness import wait_for_stable_hud

EAGER_FIELDS=('main','prepared','vrcam','inputSame','outputSame','changed','missing','replaced','failure',
              'outputDifferent','stale','tooLate','rawCount','finalCount','nativeCount','firstDifference')


def validate_eager(stats,mode,frames):
    if stats['compatible']!=1 or stats['verified']!=1 or stats['main']!=frames or stats['vrcam']!=frames:
        raise RuntimeError(f'Eager depth did not observe compatible paired inputs: {stats}')
    expected='replaced' if mode==2 else 'outputSame'
    if stats['failure'] or stats['outputDifferent'] or stats[expected]<30 or stats['inputSame']!=stats[expected]:
        raise RuntimeError(f'Eager depth did not match native output: {stats}')


def validate_stats(stats, frames):
    if stats['RunState'] != 2 or stats['RunFailure']:
        raise RuntimeError(f'Paired interval failed: {stats}')
    for name in ('RunFrames','RootSkipped','PairMerged','PairCompleted','DepthMain','DepthVrcam',
                 'RunEyeCopies','RunEyeSubmissions'):
        if stats[name] != frames:
            raise RuntimeError(f'Paired interval has incomplete or extra {name}: {stats}')


def validate_schedule(before,after,frames,cache=None):
    if len(before)!=16 or len(after)!=16 or any(after[i]-before[i]!=frames for i in (0,1,2)) or after[3]!=before[3]:
        raise RuntimeError(f'CPU preparation schedule did not cover every paired frame: {before} -> {after}')
    if cache is not None:
        hits,builds=(after[i]-before[i] for i in (12,13))
        if min(hits,builds)<0 or hits+builds!=frames or (cache and not hits) or (not cache and hits):
            raise RuntimeError(f'CPU schedule cache did not account for every plan: hits={hits}, builds={builds}')


def schedule_profile(before,after,frequency):
    if len(before)!=6 or len(after)!=6 or frequency<=0 or any(b<a for a,b in zip(before,after)):
        raise ValueError('Invalid CPU schedule timing interval')
    return {name:dict(calls=after[i*2]-before[i*2],milliseconds=(after[i*2+1]-before[i*2+1])*1000/frequency)
            for i,name in enumerate(('capture','plan','dependencies'))}


def run_paired_interval(pid, game, frames, check, inspect, report_path, change_view=None,eager_mode=0,schedule=False,schedule_cache=True):
    from live_read import Reader
    from live_symbols import resolve
    if not 30 <= frames <= 1200:
        raise ValueError('Paired interval must contain 30-1200 frames')
    symbols = resolve(pid, game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                      ['CyberpunkVR_Preparation*','CyberpunkVR_EagerDepth*','g_menuModeValue','CyberpunkVR_CullReuseMode'])
    reader = Reader(pid, game/'bin/x64/Cyberpunk2077.exe', 0, 0)
    k, handle = reader.k, None
    armed=False;original_cache=None
    k.WriteProcessMemory.argtypes = [c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
    k.WaitForSingleObject.argtypes = [c.c_void_p,c.c_uint32]
    address = lambda name: int(symbols['CyberpunkVR_Preparation'+name],16)
    read = lambda name: struct.unpack('<I',reader.read(address(name),4))[0]
    report = dict(pid=pid,dll=symbols['sha256'],requestedFrames=frames,samples=[],passed=False)
    def write(name,value,prefix='CyberpunkVR_Preparation'):
        data,done=c.c_uint32(value),c.c_size_t()
        if not k.WriteProcessMemory(handle,int(symbols[prefix+name],16),c.byref(data),4,c.byref(done)) or done.value!=4:
            raise c.WinError(c.get_last_error())
    def stats():
        result={name:read(name) for name in ('RunState','RunFailure','RunFrames','RootSkipped','PairMerged',
                                           'PairCompleted','RunEyeCopies','RunEyeSubmissions')}
        result['DepthMain'],result['DepthVrcam']=struct.unpack('<2I',reader.read(address('RunDepth'),8))
        return result
    def running():
        check()
        if read('RunState')!=1 or read('RunFailure'):
            raise RuntimeError(f'Paired interval stopped before active-mode inspection: {stats()}')
        return True
    def eager():
        value=dict(zip(EAGER_FIELDS,struct.unpack('<16I',reader.read(int(symbols['CyberpunkVR_EagerDepthStats'],16),64))))
        for name in ('Mode','Compatible','Verified'):
            value[name.lower()]=struct.unpack('<I',reader.read(int(symbols['CyberpunkVR_EagerDepth'+name],16),4))[0]
        return value
    def schedule_stats():
        deadline=time.monotonic()+1
        while time.monotonic()<deadline:
            seq=read('ScheduleSeq')
            if not seq&1:
                value=list(struct.unpack('<16I',reader.read(address('ScheduleStats'),64)))
                if seq==read('ScheduleSeq'):return value
            time.sleep(.001)
        raise TimeoutError('CPU schedule publication remained busy')
    schedule_cpu=lambda:list(struct.unpack('<6Q',reader.read(address('ScheduleCpu'),48)))
    try:
        if (any(read(name) for name in ('Request','PairRequest','RunRequest')) or read('State')==1 or read('RunState')==1
                or any(read(name)!=1 for name in ('PairVerified','PairCompatible','RootCompatible'))):
            raise RuntimeError('Paired interval requires an idle, compatible, verified warmup')
        if any(struct.unpack('<I',reader.read(int(symbols[name],16),4))[0]
               for name in ('g_menuModeValue','CyberpunkVR_CullReuseMode')):
            raise RuntimeError('Paired interval requires gameplay and native culling')
        handle=k.OpenProcess(0x101028,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        initial_eager=eager()
        if initial_eager['mode'] or (eager_mode==2 and initial_eager['verified']!=1):
            raise RuntimeError('Eager replacement requires an idle verified comparison')
        if read('ScheduleMode'):raise RuntimeError('CPU schedule already owned by another test')
        if schedule:
            schedule_before=schedule_stats()
            if read('ScheduleCompatible')!=1 or schedule_before[2]<1 or schedule_before[3]:
                raise RuntimeError('CPU schedule interval requires a successful bounded application first')
            original_cache=read('ScheduleCache');cpu_before=schedule_cpu()
            frequency=struct.unpack('<Q',reader.read(address('ScheduleCpuFrequency'),8))[0]
        armed=True
        write('Mode',eager_mode,'CyberpunkVR_EagerDepth')
        if schedule:write('ScheduleCache',int(schedule_cache));write('ScheduleMode',2)
        trace_before=(read('Count'),read('PacketCount'))
        write('RunStop',0);write('RunRequest',frames)
        deadline=time.monotonic()+48
        start_deadline=time.monotonic()+3
        while read('RunRequest') or read('RunState')!=1:
            check()
            if read('RunState')==3 or time.monotonic()>start_deadline:
                raise RuntimeError(f'Paired interval did not start: {stats()}')
            time.sleep(.05)
        report['hud']=wait_for_stable_hud(pid,game,10,running)
        for index in range(3 if change_view else 2):
            running()
            if change_view:
                pose=change_view(index)
                target=read('PairCompleted')+30
                while read('PairCompleted')<target:
                    running()
                    if time.monotonic()>deadline:raise TimeoutError('Paired interval pose did not settle')
                    time.sleep(.05)
            before=stats()
            picture=inspect(f'paired-active-{index}')
            running()
            after=stats()
            report['samples'].append(dict(before=before,after=after,image=picture,pose=pose if change_view else None))
            if index==0:
                target=after['PairCompleted']+90
                while read('PairCompleted')<target:
                    running()
                    if time.monotonic()>deadline:raise TimeoutError('Paired interval progress stalled')
                    time.sleep(.05)
        while read('RunState')==1:
            check()
            if time.monotonic()>deadline:raise TimeoutError('Paired interval did not finish')
            time.sleep(.1)
        report['final']=stats()
        validate_stats(report['final'],frames)
        if schedule:
            schedule_after=schedule_stats();cpu_after=schedule_cpu()
            report['cpuSchedule']=dict(before=schedule_before,after=schedule_after,cache=schedule_cache,
                cpuBefore=cpu_before,cpuAfter=cpu_after,frequency=frequency,profile=schedule_profile(cpu_before,cpu_after,frequency))
            validate_schedule(schedule_before,schedule_after,frames,schedule_cache)
        report['eager']=eager()
        if eager_mode:validate_eager(report['eager'],eager_mode,frames)
        report['fullTraceCountsUnchanged']=trace_before==(read('Count'),read('PacketCount'))
        if read('State')==1 or not report['fullTraceCountsUnchanged']:
            raise RuntimeError('Heavy preparation tracing was active during the paired interval')
        report['passed']=True
        return report
    except Exception as error:
        report['error']=str(error)
        try:report['eager']=eager()
        except OSError:pass
        raise
    finally:
        try:
            if armed and handle and k.WaitForSingleObject(handle,0)!=0:
                write('RunRequest',0);write('RunStop',1)
                deadline=time.monotonic()+3
                while read('RunState')==1:
                    check()
                    if time.monotonic()>deadline:raise TimeoutError('Paired interval cancellation did not drain')
                    time.sleep(.05)
                write('RunState',0);write('RunStop',0)
                write('Mode',0,'CyberpunkVR_EagerDepth')
                if schedule:
                    write('ScheduleMode',0)
                    if original_cache is not None:
                        write('ScheduleCache',original_cache)
                        if read('ScheduleCache')!=original_cache:raise RuntimeError('CPU schedule cache control was not restored')
                if read('PairRequest'):raise RuntimeError('Paired interval left a pairing request armed')
                report['cleanup']='restored'
        except Exception as error:
            report['cleanupError']=str(error);report['passed']=False
            raise
        finally:
            if handle:k.CloseHandle(handle)
            reader.close()
            report_path.write_text(json.dumps(report,indent=2)+'\n')
