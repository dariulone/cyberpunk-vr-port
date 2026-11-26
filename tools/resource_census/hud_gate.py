"""Read-only verification that the HUD separate-window startup experiment applied."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'single_pass_tests'))
from native_textures import mapped_export

LABELS = ('copyCalls', 'returnedEntries', 'whitelistMatches', 'forcedChanges', 'bypassedMatches')


def validates(mode, counts, expected):
    calls, entries, matched, forced, bypassed = counts
    if mode != expected or not calls or not entries or not matched:
        return False
    if expected == 0:
        return forced == 0 and bypassed == matched
    return forced > 0 and forced <= matched and bypassed == 0


def collect(pid):
    import thread_stack_sampler as modules
    kernel = modules.k
    kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel.CloseHandle.argtypes = [w.HANDLE]
    process = kernel.OpenProcess(0x410, False, pid)
    if not process:
        raise c.WinError(c.get_last_error())
    def read(address, size):
        raw, copied = c.create_string_buffer(size), c.c_size_t()
        if not kernel.ReadProcessMemory(process, address, raw, size, c.byref(copied)) or copied.value != size:
            raise c.WinError(c.get_last_error())
        return raw.raw
    try:
        plugin = next((base for base, _, name in modules.modules(process) if name.lower() == 'cyberpunkvrport.dll'), None)
        if plugin is None:
            raise ValueError('CyberpunkVRPort.dll is not loaded')
        mode_address = mapped_export(read, plugin, 'CyberpunkVR_HudSeparateWindows')
        counts_address = mapped_export(read, plugin, 'CyberpunkVR_HudSeparateWindowsCounts')
        for _ in range(8):
            mode = read(mode_address, 4); raw = read(counts_address, 40)
            stable = raw == read(counts_address, 40) and mode == read(mode_address, 4)
            if stable:
                break
        counts = struct.unpack('<5Q', raw)
        return dict(pid=pid, moduleBase=hex(plugin), mode=struct.unpack('<I', mode)[0],
                    counts=dict(zip(LABELS, counts)), stable=stable,
                    scope='independent atomic counters; query after gameplay startup has completed; no PDB/native calls or writes')
    finally:
        kernel.CloseHandle(process)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--expect', type=int, choices=(0, 1))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    report = collect(args.pid)
    if args.expect is not None:
        report['expected'] = args.expect
        report['startupApplied'] = report['stable'] and validates(report['mode'], tuple(report['counts'][name] for name in LABELS), args.expect)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
    if not report['stable'] or report.get('startupApplied') is False:
        raise SystemExit(2)
