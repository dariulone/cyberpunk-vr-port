"""Read native DynamicTexture construction origins; optionally arm this diagnostic only."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import sys
import time
from native_textures import mapped_export

LAYOUT = (1, 1024, 344, 48, 76, 88, 32, 80)
PREFIX = 'CyberpunkVR_DynamicTextureOrigins'
COUNTERS = ('Observed', 'InFlight', 'DroppedReentrant', 'DroppedBusy', 'Overwritten')


def decode_rows(raw, count, layout=LAYOUT, since=0):
    if tuple(layout) != LAYOUT or len(raw) != layout[1] * layout[2]:
        raise ValueError('Unknown/truncated native origin publication layout')
    if since < 0 or since > count:
        raise ValueError('Origin count reset or invalid starting count')
    first = max(1, count - layout[1] + 1)
    records = []
    for ordinal in range(max(first, since + 1), count + 1):
        at = ((ordinal - 1) % layout[1]) * layout[2]
        number, qpc, obj, control, serial, caller = struct.unpack_from('<6Q', raw, at)
        if number != ordinal:
            raise ValueError('Torn/wrong generation native origin row')
        width, height, mips, samples, fmt, flags, reserved, backing = struct.unpack_from('<II4BIQ', raw, at + 48)
        thread, depth, row_flags, _ = struct.unpack_from('<4I', raw, at + 72)
        if depth > 32:
            raise ValueError('Invalid native origin stack length')
        frames = struct.unpack_from('<32Q', raw, at + 88)[:depth]
        records.append(dict(ordinal=number, qpc=qpc, object=hex(obj), control=hex(control), objectSerial=serial,
            caller=hex(caller), thread=thread, flags=row_flags,
            descriptor=dict(width=width, height=height, mips=mips, samples=samples, format=fmt, flags=flags, reserved=reserved, backing=hex(backing)),
            stack=[hex(pc) for pc in frames]))
    return dict(firstAvailableOrdinal=first, overwrittenSinceRequested=max(0, first - 1 - since), records=records)


def collect(pid, enable=None, since=0):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'single_pass_tests'))
    import thread_stack_sampler as modules
    kernel = modules.k
    kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel.WriteProcessMemory.argtypes = kernel.ReadProcessMemory.argtypes
    kernel.CloseHandle.argtypes = [w.HANDLE]
    process = kernel.OpenProcess(0x410 | (0x28 if enable is not None else 0), False, pid)
    if not process:
        raise c.WinError(c.get_last_error())
    def read(address, size):
        raw, got = c.create_string_buffer(size), c.c_size_t()
        if not kernel.ReadProcessMemory(process, address, raw, size, c.byref(got)) or got.value != size:
            raise c.WinError(c.get_last_error())
        return raw.raw
    def value(address, fmt):
        return struct.unpack('<' + fmt, read(address, struct.calcsize('<' + fmt)))
    try:
        loaded = modules.modules(process)
        plugin = next((base for base, _, name in loaded if name.lower() == 'cyberpunkvrport.dll'), None)
        if plugin is None:
            raise ValueError('CyberpunkVRPort.dll is not loaded')
        names = ('Enable', 'Status', 'Sequence', 'Count', 'QpcFrequency', 'Layout', 'Rows') + COUNTERS
        address = {name: mapped_export(read, plugin, PREFIX + name) for name in names}
        status = value(address['Status'], 'I')[0]
        layout = value(address['Layout'], '8I')
        if layout != LAYOUT:
            raise ValueError(f'Unsupported origin layout: {layout}')
        current_count = value(address['Count'], 'Q')[0]
        if since < 0 or since > current_count:
            raise ValueError('Origin count reset or invalid starting count')
        if enable is not None:
            if enable not in (0, 1) or (enable and status != 1):
                raise ValueError(f'Native origin observer cannot be armed: status={status}')
            raw = struct.pack('<I', enable); got = c.c_size_t()
            if not kernel.WriteProcessMemory(process, address['Enable'], raw, len(raw), c.byref(got)) or got.value != len(raw):
                raise c.WinError(c.get_last_error())
        for _ in range(40):
            seq = value(address['Sequence'], 'Q')[0]
            if seq & 1:
                time.sleep(.002); continue
            count = value(address['Count'], 'Q')[0]
            raw = read(address['Rows'], layout[1] * layout[2])
            if seq == value(address['Sequence'], 'Q')[0]:
                break
        else:
            raise TimeoutError('Native origin publication remained busy')
        report = decode_rows(raw, count, layout, since)
        counters = {name: value(address[name], 'Q')[0] for name in COUNTERS}
        report.update(pid=pid, status=status, enabled=value(address['Enable'], 'I')[0], count=count, sequence=seq,
            qpcFrequency=value(address['QpcFrequency'], 'Q')[0], layout=layout, coverage=counters,
            coverageSettled=counters['InFlight'] == 0 and counters['Observed'] == count + counters['DroppedReentrant'] + counters['DroppedBusy'],
            scope='native factory returns while armed; no native lifetime calls or resource refs; reflection/clone paths may bypass this factory',
            modules=[dict(name=name, base=hex(base), size=end-base, path=modules.modules.paths.get(base)) for base,end,name in loaded])
        def locate(pc):
            number = int(pc, 16)
            for base, end, name in loaded:
                if base <= number < end:
                    return dict(address=pc, module=name, rva=hex(number-base))
            return dict(address=pc, module=None)
        for row in report['records']:
            row['callerModule'] = locate(row['caller'])
            row['stackModules'] = [locate(pc) for pc in row['stack']]
        return report
    finally:
        kernel.CloseHandle(process)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--enable', type=int, choices=(0, 1))
    parser.add_argument('--since', type=int, default=0)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = collect(args.pid, args.enable, args.since)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: result[key] for key in ('pid','status','enabled','count','firstAvailableOrdinal','overwrittenSinceRequested','coverage','coverageSettled')}, indent=2))
