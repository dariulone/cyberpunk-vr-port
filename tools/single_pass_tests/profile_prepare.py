"""Bounded native CPU preparation sample; no replay, draw skipping or GPU gate.

Uses existing counters and restores controls even on failure. Stage totals include
their child scopes; worker wall time is NOT a critical-path/FPS prediction.
"""
import argparse
import ctypes as c
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / ('tools/' + name))
                for name in ('roomscale_tests', 'swimming_tests', 'quickboot_tests')]
from live_read import Reader
from live_symbols import resolve
import launch_to_save as boot

PREFIX = 'CyberpunkVR_'
SCOPES = ('Stage', 'Gather', 'Filter', 'Finalize', 'SortA', 'SortB', 'SortC', 'SortFinal')
EYES = ('Main', 'Vrcam')
CONTROLS = ('RuntimeDiagnostics', 'PrepareProfileEnable')
GUARDS = ('CullReuseMode', 'PrepareCacheAuditEnable', 'MaterializeProfileEnable',
          'CullCallbackProfileEnable', 'ProfEnable', 'GeometryPacketRequest',
          'GeometryPacketResolve', 'NativeStereoProbeRequest', 'StereoGpuProbeRequest',
          'StereoGpuProbePrepareGateMs', 'SinglePassTwinUpload', 'SinglePassTraceRequest')
ARRAYS = ('BucketCalls', 'BucketDesc', 'BucketTicks', 'BucketSortATicks',
          'BucketSortBTicks', 'BucketSortCTicks', 'BucketSortFinalTicks',
          'FilterIn', 'FilterOut', 'FilterCalls')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', required=True, type=int)
    parser.add_argument('--seconds', type=float, default=15)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    if not 2 <= args.seconds <= 60:
        parser.error('Use a bounded 2-60 second sample')
    game = boot.DEFAULT_GAME
    ini = (game / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini').read_text(encoding='utf-8-sig')
    if 'xr_framegen=0' not in ini.splitlines():
        raise RuntimeError('This measurement requires frame generation disabled')
    scalars = [f'DebugPrepare{scope}Ticks{eye}' for scope in SCOPES for eye in EYES]
    scalars += [f'DebugPrepareCalls{eye}' for eye in EYES]
    arrays = [f'DebugPrepare{kind}{eye}' for kind in ARRAYS for eye in EYES]
    states = ('GeometryPacketState', 'NativeStereoProbeState', 'StereoGpuProbeState', 'SinglePassTraceState')
    names = [PREFIX + n for n in (*CONTROLS, *GUARDS, *scalars, *arrays, *states,
                                 'MonoCascadeReport', 'MonoCascadeReportSeq')] + ['g_menuModeValue']
    dll = game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
    symbols = resolve(args.pid, dll, names)
    reader = Reader(args.pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    k = reader.k
    k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    k.WriteProcessMemory.restype = c.c_int
    handle = k.OpenProcess(0x1028, False, args.pid)
    if not handle:
        reader.close()
        raise c.WinError(c.get_last_error())
    address = lambda name: int(symbols[PREFIX + name], 16)
    read32 = lambda name: struct.unpack('<I', reader.read(address(name), 4))[0]
    old = {}
    started = time.time()

    def write(name, value):
        data, done = c.c_uint32(value), c.c_size_t()
        if not k.WriteProcessMemory(handle, address(name), c.byref(data), 4, c.byref(done)) or done.value != 4:
            raise c.WinError(c.get_last_error())

    def check():
        crash = boot.crash_report(args.pid, started)
        if crash:
            boot.close_reporters(boot.reporter_window_pids())
            raise RuntimeError(crash)
        if struct.unpack('<I', reader.read(int(symbols['g_menuModeValue'], 16), 4))[0]:
            raise RuntimeError('Gameplay left during sample')

    def snapshot():
        values = {name: struct.unpack('<Q', reader.read(address(name), 8))[0] for name in scalars}
        values.update({name: struct.unpack('<256Q', reader.read(address(name), 2048)) for name in arrays})
        return values

    def cascade_cycle():
        for _ in range(50):
            seq = read32('MonoCascadeReportSeq')
            if seq & 1:
                continue
            data = reader.read(address('MonoCascadeReport'), 4096).split(b'\0', 1)[0]
            if read32('MonoCascadeReportSeq') == seq:
                return json.loads(data)['cycle']
        raise RuntimeError('Cascade report busy')

    try:
        check()
        for name in GUARDS + ('PrepareProfileEnable',):
            if read32(name):
                raise RuntimeError('Conflicting diagnostic already active: ' + name)
        for name in states:
            if read32(name) in ((1, 2) if name == 'StereoGpuProbeState' else (1,)):
                raise RuntimeError('Conflicting recorder in flight: ' + name)
        old = {name: read32(name) for name in CONTROLS}
        frequency = c.c_int64()
        if not k.QueryPerformanceFrequency(c.byref(frequency)):
            raise c.WinError(c.get_last_error())
        write('RuntimeDiagnostics', 1)
        before = snapshot()
        cycle_before = cascade_cycle()
        begin = time.perf_counter()
        write('PrepareProfileEnable', 1)
        while time.perf_counter() - begin < args.seconds:
            time.sleep(.25)
            check()
        write('PrepareProfileEnable', 0)
        elapsed = time.perf_counter() - begin
        time.sleep(.1)  # Existing in-flight scopes can finish without clearing counters.
        after = snapshot()
        cycle_after = cascade_cycle()
        if cycle_after <= cycle_before:
            raise RuntimeError('No rendered sun-shadow cycles observed; reject sample')
        delta = {name: after[name] - before[name] for name in scalars}
        if any(value < 0 for value in delta.values()):
            raise RuntimeError('Counters reset during sample')
        scale = 1000 / frequency.value
        scopes = {eye: {scope: delta[f'DebugPrepare{scope}Ticks{eye}'] * scale for scope in SCOPES}
                  for eye in EYES}
        buckets = []
        for index in range(256):
            row = {eye: {kind: after[f'DebugPrepare{kind}{eye}'][index] - before[f'DebugPrepare{kind}{eye}'][index]
                         for kind in ARRAYS} for eye in EYES}
            if any(row[e]['BucketCalls'] or row[e]['FilterCalls'] for e in EYES):
                buckets.append(dict(stage=index // 4, mode=index % 4, eyes=row))
        buckets.sort(key=lambda r: -sum(r['eyes'][eye]['BucketTicks'] for eye in EYES))
        report = dict(pid=args.pid, dll=symbols['sha256'], elapsedSeconds=elapsed,
                      qpcFrequency=frequency.value, scopesMs=scopes, counters=delta, buckets=buckets,
                      approximateRenderedPairs=cycle_after - cycle_before,
                      note='Nested inclusive CPU wall times; do not sum parent/children or infer FPS. '
                           'Cascade cycle denominator is approximate (250 ms report cadence and 100 ms settling). '
                           'No rendering work was skipped.')
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({k: v for k, v in report.items() if k != 'buckets'}, indent=2))
    finally:
        try:
            if not boot.crash_report(args.pid, started):
                for name, value in reversed(list(old.items())):
                    write(name, value)
        finally:
            k.CloseHandle(handle)
            reader.close()


if __name__ == '__main__':
    main()
