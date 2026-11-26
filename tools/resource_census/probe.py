"""Opt-in resource lifetime report. Writes only diagnostic enable/request exports."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'single_pass_tests'))
import thread_stack_sampler as symbols

NAMES = ['Enable', 'Request', 'Completed', 'ReportSeq', 'ReportBytes', 'Report']


def collect(pid, enable=None, timeout=8, symbol_path=None):
    kernel = symbols.k
    kernel.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel.CloseHandle.argtypes = [w.HANDLE]
    process = kernel.OpenProcess(0x0400 | 0x0010 | 0x0020 | 0x0008, False, pid)
    if not process:
        raise c.WinError(c.get_last_error())
    initialized = False

    def read(address, size):
        data = c.create_string_buffer(size)
        copied = c.c_size_t()
        if not kernel.ReadProcessMemory(process, address, data, size, c.byref(copied)) or copied.value != size:
            raise c.WinError(c.get_last_error())
        return data.raw

    def write(address, data):
        copied = c.c_size_t()
        if not kernel.WriteProcessMemory(process, address, data, len(data), c.byref(copied)) or copied.value != len(data):
            raise c.WinError(c.get_last_error())

    try:
        symbols.initialize_symbols(process, symbol_path)
        initialized = True
        addresses = {name: symbols.resolve_named_symbol(process, 'cyberpunkvrport.dll', 'CyberpunkVR_ResourceCensus' + name) for name in NAMES}
        if enable is not None:
            write(addresses['Enable'], struct.pack('<I', enable))
        request = max(struct.unpack('<Q', read(addresses['Request'], 8))[0], struct.unpack('<Q', read(addresses['Completed'], 8))[0]) + 1
        if request > 0xffffffffffffffff:
            request = 1
        write(addresses['Request'], struct.pack('<Q', request))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            completed = struct.unpack('<Q', read(addresses['Completed'], 8))[0]
            if completed == request:
                first = struct.unpack('<Q', read(addresses['ReportSeq'], 8))[0]
                size = struct.unpack('<I', read(addresses['ReportBytes'], 4))[0]
                if not first & 1 and 0 < size < 1024 * 1024:
                    raw = read(addresses['Report'], size)
                    second = struct.unpack('<Q', read(addresses['ReportSeq'], 8))[0]
                    if first == second and struct.unpack('<Q', read(addresses['Completed'], 8))[0] == request:
                        report = json.loads(raw.decode('utf-8'))
                        report['reader'] = {'pid': pid, 'request': request, 'reportSequence': second}
                        return report
            time.sleep(.02)
        raise TimeoutError('No completed census report: the game must reach Present; do not run simultaneous report controllers')
    finally:
        if initialized:
            symbols.d.SymCleanup(process)
        kernel.CloseHandle(process)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--enable', type=int, choices=(0, 1, 2), help='0 stop new tracking; 1 API caller; 2 caller plus bounded stack')
    parser.add_argument('--symbol-path')
    parser.add_argument('--timeout', type=float, default=8)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = collect(args.pid, args.enable, args.timeout, args.symbol_path)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: result.get(key) for key in ('pid', 'mode', 'liveCookies', 'attachFailures', 'cookieLimitDrops', 'groupOverflows', 'error')}, indent=2))
