"""Require fresh HUD sources and successfully submitted HUD frames, not a cached texture."""

import ctypes as c
import struct
import time


class StableHud:
    def __init__(self, milliseconds=1000, frames=30):
        self.duration, self.frames = milliseconds, frames
        self.start = self.previous = None
        self.samples = 0

    def observe(self, sample):
        valid = (sample is not None and sample['menu'] == 0 and sample['tracked'] == 1 and sample['layers'] in (1, 2)
                 and sample['serial'] > 0 and 0 <= sample['now'] - sample['sourceTick'] < 300)
        if not valid:
            self.start = self.previous = None
            self.samples = 0
            return False
        previous = self.previous
        if previous and all(sample[k] >= previous[k] for k in ('now', 'prepared', 'submitted', 'serial', 'displayTime')):
            # Polling can read one fresh source twice. Do not count it as a new
            # frame, but keep the interval until the existing stall bound expires.
            if any(sample[k] == previous[k] for k in ('prepared', 'submitted', 'serial', 'displayTime')) and sample['now'] - previous['now'] <= 350:
                return False
        advanced = previous and all(sample[k] > previous[k]
                                    for k in ('now', 'prepared', 'submitted', 'serial', 'displayTime'))
        if not advanced or sample['now'] - previous['now'] > 350:
            self.start = sample.copy()
            self.samples = 1
        else:
            self.samples += 1
        self.previous = sample.copy()
        return (sample['now'] - self.start['now'] >= self.duration
                and sample['submitted'] - self.start['submitted'] >= self.frames
                and self.samples >= 5)

    def report(self):
        if not self.previous or not self.start:
            return dict(stableSamples=0)
        return dict(stableSamples=self.samples, stableMilliseconds=self.previous['now'] - self.start['now'],
                    submittedFrames=self.previous['submitted'] - self.start['submitted'],
                    sourceSerialAdvances=self.previous['serial'] - self.start['serial'], last=self.previous)


def wait_for_stable_hud(pid, game, timeout, check, on_sample=None):
    """Keep crash/world checks active while observing this launch's native HUD."""
    from live_read import Reader
    from live_symbols import resolve

    names = ['CyberpunkVR_RuntimeDiagnostics', 'g_menuModeValue', 'CyberpunkVR_HudPoseDebug',
             'CyberpunkVR_HudPoseDebugSeq', 'CyberpunkVR_HudSubmittedFrames']
    deadline = time.monotonic() + timeout
    symbols = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', names)
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    handle = None
    original = None
    stable = StableHud()
    sample = None
    k = reader.k
    read32 = lambda name: struct.unpack('<I', reader.read(int(symbols[name], 16), 4))[0]

    def diagnostics(value):
        data, done = c.c_uint32(value), c.c_size_t()
        if not k.WriteProcessMemory(handle, int(symbols[names[0]], 16), c.byref(data), 4, c.byref(done)) or done.value != 4:
            raise c.WinError(c.get_last_error())

    try:
        k.GetTickCount64.restype = c.c_uint64
        k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        handle = k.OpenProcess(0x101028, False, pid)  # synchronize, limited query, VM operation/write
        if not handle:
            raise c.WinError(c.get_last_error())
        original = read32(names[0])
        diagnostics(1)
        while time.monotonic() < deadline:
            if not check():
                stable.observe(None)
                time.sleep(.1)
                continue
            seq = read32('CyberpunkVR_HudPoseDebugSeq')
            if seq & 1:
                time.sleep(.01)
                continue
            data = reader.read(int(symbols['CyberpunkVR_HudPoseDebug'], 16), 96)
            if read32('CyberpunkVR_HudPoseDebugSeq') != seq:
                time.sleep(.01)
                continue
            prepared, display = struct.unpack_from('<Qq', data)
            tracked, layers = struct.unpack_from('<ii', data, 36)
            serial, tick = struct.unpack_from('<QQ', data, 80)
            submitted, = struct.unpack('<Q', reader.read(int(symbols['CyberpunkVR_HudSubmittedFrames'], 16), 8))
            sample = dict(now=k.GetTickCount64(), menu=read32('g_menuModeValue'), prepared=prepared,
                          displayTime=display, tracked=tracked, layers=layers, serial=serial,
                          sourceTick=tick, submitted=submitted)
            if on_sample is not None:
                on_sample(sample)
            if stable.observe(sample):
                if check():
                    return stable.report()
                stable.observe(None)
            time.sleep(.1)
        raise TimeoutError(f'HUD did not sustain fresh sources and successful submissions for one second; last={sample}')
    finally:
        try:
            if original is not None:
                try:
                    diagnostics(original)
                except OSError:
                    # Only an exited process can safely skip control restoration.
                    k.WaitForSingleObject.argtypes = [c.c_void_p, c.c_uint32]
                    if k.WaitForSingleObject(handle, 0) != 0:
                        raise
        finally:
            if handle:
                k.CloseHandle(handle)
            reader.close()
