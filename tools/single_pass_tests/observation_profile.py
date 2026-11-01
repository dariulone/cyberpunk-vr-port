"""Measure opt-in observer CPU wall intervals without changing their behavior."""
import ctypes as c
import struct
import time

PHASES=('cbvMetadata','otherDescriptorMetadata','descriptorCopy','descriptorHeaps',
        'uploadLookup','uploadStackUnwind','pipelineBinding','rootBinding')


def decode(before,after,frequency):
    if len(before)!=16 or len(after)!=16 or frequency<=0 or any(b<a for a,b in zip(before,after)):
        raise ValueError('Invalid observer profile counters')
    delta=[b-a for a,b in zip(before,after)]
    return {name:dict(calls=delta[i*2],milliseconds=delta[i*2+1]*1000.0/frequency) for i,name in enumerate(PHASES)}


def run(pid,game,seconds,check):
    from live_read import Reader
    from live_symbols import resolve
    names=['CyberpunkVR_ObservationProfileEnable','CyberpunkVR_ObservationProfileStats','g_menuModeValue',
           'CyberpunkVR_ConstantUploadNativeImports','CyberpunkVR_ConstantUploadNativeRejects','CyberpunkVR_ConstantUploadApiHits']
    symbols=resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',names)
    reader=Reader(pid,game/'bin/x64/Cyberpunk2077.exe',0,0);k=reader.k;handle=None;armed=False
    read32=lambda name:struct.unpack('<I',reader.read(int(symbols[name],16),4))[0]
    counters=lambda:struct.unpack('<16Q',reader.read(int(symbols[names[1]],16),128))
    def enable(value):
        data,done=c.c_uint32(value),c.c_size_t()
        if not k.WriteProcessMemory(handle,int(symbols[names[0]],16),c.byref(data),4,c.byref(done)) or done.value!=4:
            raise c.WinError(c.get_last_error())
    try:
        check()
        if read32(names[0]) or read32('g_menuModeValue'):raise RuntimeError('Observer profile requires idle counters and gameplay')
        k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
        handle=k.OpenProcess(0x101028,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        frequency=c.c_int64()
        if not k.QueryPerformanceFrequency(c.byref(frequency)):raise c.WinError(c.get_last_error())
        before=counters();uploads_before={name:read32(name) for name in names[3:]}
        armed=True;enable(1);start=time.monotonic()
        while time.monotonic()-start<seconds:
            check()
            if read32('g_menuModeValue'):raise RuntimeError('Game left gameplay during observer profile')
            time.sleep(.1)
        enable(0);elapsed=time.monotonic()-start
        time.sleep(.1)  # Let scopes already in flight publish after profiling is disabled.
        after=counters();phases=decode(before,after,frequency.value)
        if not any(v['calls'] for v in phases.values()):raise RuntimeError('No observer calls measured')
        return dict(pid=pid,dll=symbols['sha256'],elapsedSeconds=elapsed,qpcFrequency=frequency.value,
                    before=list(before),after=list(after),phases=phases,
                    uploadCounters={name:read32(name)-value for name,value in uploads_before.items()},
                    limitation='Inclusive CPU wall intervals, including lock contention and nested scopes. Do not sum them into frame time or CPU utilization.')
    finally:
        try:
            if armed:
                try:enable(0)
                except OSError:
                    k.WaitForSingleObject.argtypes=[c.c_void_p,c.c_uint32]
                    if k.WaitForSingleObject(handle,0)!=0:raise
        finally:
            if handle:k.CloseHandle(handle)
            reader.close()
