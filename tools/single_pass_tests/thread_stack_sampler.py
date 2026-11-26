"""Sampling call-stack profiler for one thread of a live game (read-only, no injection).

Each sample suspends the thread, walks its stack with dbghelp StackWalk64 (x64 unwind data of
the live modules), and resumes it. At the default 100 Hz the thread is suspended for well under
5% of the time. Frames are recorded as module+RVA; exe frames are folded to their function start
using the executable's own .pdata, so inclusive/exclusive percentages are per native function.
"""
import argparse
import bisect
import collections
import ctypes as c
import ctypes.wintypes as w
import json
from pathlib import Path
import struct
import time
import symbol_session as ss
from symbol_identity import live_pe_identity, pe_identity

k = c.WinDLL('kernel32', use_last_error=True)
d = ss.load_dbghelp()
psapi = c.WinDLL('psapi', use_last_error=True)


class ADDRESS64(c.Structure):
    _fields_ = [('Offset', c.c_uint64), ('Segment', c.c_uint16), ('Mode', c.c_uint32)]


class KDHELP64(c.Structure):
    _fields_ = [('Thread', c.c_uint64), ('ThCallbackStack', c.c_uint32), ('ThCallbackBStore', c.c_uint32),
                ('NextCallback', c.c_uint32), ('FramePointer', c.c_uint32), ('KiCallUserMode', c.c_uint64),
                ('KeUserCallbackDispatcher', c.c_uint64), ('SystemRangeStart', c.c_uint64),
                ('KiUserExceptionDispatcher', c.c_uint64), ('StackBase', c.c_uint64), ('StackLimit', c.c_uint64),
                ('BuildVersion', c.c_uint32), ('RetpolineStubFunctionTableSize', c.c_uint32),
                ('RetpolineStubFunctionTable', c.c_uint64), ('RetpolineStubOffset', c.c_uint32),
                ('RetpolineStubSize', c.c_uint32), ('Reserved0', c.c_uint64 * 2)]


class STACKFRAME64(c.Structure):
    _fields_ = [('AddrPC', ADDRESS64), ('AddrReturn', ADDRESS64), ('AddrFrame', ADDRESS64), ('AddrStack', ADDRESS64),
                ('AddrBStore', ADDRESS64), ('FuncTableEntry', c.c_void_p), ('Params', c.c_uint64 * 4), ('Far', w.BOOL),
                ('Virtual', w.BOOL), ('Reserved', c.c_uint64 * 3), ('KdHelp', KDHELP64)]


CONTEXT_SIZE, CONTEXT_FULL = 0x4D0, 0x10000B
OFF_FLAGS, OFF_RSP, OFF_RBP, OFF_RIP = 0x30, 0x98, 0xA0, 0xF8
AddrModeFlat = 3
k.OpenProcess.restype = w.HANDLE
k.OpenThread.restype = w.HANDLE
k.SuspendThread.argtypes = k.ResumeThread.argtypes = [w.HANDLE]
k.SuspendThread.restype = k.ResumeThread.restype = w.DWORD
k.GetThreadContext.argtypes = [w.HANDLE, c.c_void_p]
d.SymInitializeW.argtypes = [w.HANDLE, w.LPCWSTR, w.BOOL]
d.SymFunctionTableAccess64.restype = c.c_void_p
d.SymFunctionTableAccess64.argtypes = [w.HANDLE, c.c_uint64]
d.SymGetModuleBase64.restype = c.c_uint64
d.SymGetModuleBase64.argtypes = [w.HANDLE, c.c_uint64]
d.StackWalk64.argtypes = [w.DWORD, w.HANDLE, w.HANDLE, c.POINTER(STACKFRAME64), c.c_void_p, c.c_void_p, c.c_void_p, c.c_void_p, c.c_void_p]
FUNCTION_TABLE = c.cast(d.SymFunctionTableAccess64, c.c_void_p)
MODULE_BASE = c.cast(d.SymGetModuleBase64, c.c_void_p)


class SYMBOL_INFOW(c.Structure):
    _fields_ = [('SizeOfStruct', c.c_uint32), ('TypeIndex', c.c_uint32), ('Reserved', c.c_uint64 * 2), ('Index', c.c_uint32),
                ('Size', c.c_uint32), ('ModBase', c.c_uint64), ('Flags', c.c_uint32), ('Value', c.c_uint64),
                ('Address', c.c_uint64), ('Register', c.c_uint32), ('Scope', c.c_uint32), ('Tag', c.c_uint32),
                ('NameLen', c.c_uint32), ('MaxNameLen', c.c_uint32), ('Name', c.c_wchar * 512)]


d.SymFromAddrW.argtypes = [w.HANDLE, c.c_uint64, c.POINTER(c.c_uint64), c.POINTER(SYMBOL_INFOW)]
d.SymFromAddrW.restype = w.BOOL


def initialize_symbols(process, symbol_path=None):
    mods = modules(process)
    live = {}
    for base, _, module in mods:
        try:
            live[base] = live_pe_identity(process, base)
        except (OSError, ValueError, struct.error) as error:
            live[base] = {'error': str(error)}
    state = ss.initialize(d, process, modules.paths.values(), symbol_path)
    state['modules'] = {}
    state['symbols'] = {}
    state['mappedImages'] = live
    initialize_symbols.state = state
    state['provenanceWarnings'] = []
    # Deferred PDB I/O must not happen while a sampled game thread is suspended.
    # Resolve a module address now, before the profiling clock and stack walks.
    for base, end, module_name in mods:
        probe = SYMBOL_INFOW()
        probe.SizeOfStruct, probe.MaxNameLen = 88, 511
        offset = c.c_uint64()
        d.SymFromAddrW(process, base + min(0x1000, max(0, end - base - 1)), c.byref(offset), c.byref(probe))
        module = ss.module_info(d, process, base)
        mapped = live.get(base, {})
        module['mappedImage'] = mapped
        if module.get('loadedPdb') and mapped.get('guid'):
            module['mappedIdentityVerified'] = mapped['guid'] == module.get('pdbGuid') and mapped['age'] == module.get('pdbAge')
        state['modules'][base] = module
        if module_name.lower() == 'cyberpunkvrport.dll' and (
                module.get('symbolType') not in ('pdb', 'dia') or not module.get('mappedIdentityVerified')):
            state['provenanceWarnings'].append(f'{module_name}: matching private PDB was not verified; use raw RVAs / qualified nearest hints')
    return mods


def resolve_symbols(process, pcs):
    """Bounded PDB names, or explicitly qualified nearest-symbol hints with raw RVAs."""
    names = {}
    state = getattr(initialize_symbols, 'state', {'modules': {}, 'symbols': {}})
    for pc in pcs:
        info = SYMBOL_INFOW()
        info.SizeOfStruct = 88  # sizeof(SYMBOL_INFOW) in C, with its one-character Name
        info.MaxNameLen = 511
        displacement = c.c_uint64()
        if d.SymFromAddrW(process, pc, c.byref(displacement), c.byref(info)):
            # DbgHelp leaves SYMBOL_INFO.ModBase zero for some export symbols.
            module_base = info.ModBase or d.SymGetModuleBase64(process, pc)
            module = state['modules'].get(module_base)
            if not module or module.get('symbolType') == 'deferred':
                module = ss.module_info(d, process, pc)
                mapped = state.get('mappedImages', {}).get(module_base, {})
                if not mapped:
                    try:
                        mapped = live_pe_identity(process, module_base)
                    except (OSError, ValueError, struct.error) as error:
                        mapped = {'error': str(error)}
                    state.setdefault('mappedImages', {})[module_base] = mapped
                module['mappedImage'] = mapped
                if module.get('loadedPdb') and mapped.get('guid'):
                    module['mappedIdentityVerified'] = mapped['guid'] == module.get('pdbGuid') and mapped['age'] == module.get('pdbAge')
                state['modules'][module_base] = module
            symbol_type = module.get('symbolType', 'unknown')
            label, bounded = ss.symbol_label(info.Name, info.Address, info.Size, pc, module_base, symbol_type, info.Flags)
            record = {'name': info.Name, 'address': info.Address, 'size': info.Size, 'displacement': displacement.value,
                      'rva': pc - module_base, 'moduleBase': module_base, 'flags': info.Flags,
                      'symbolType': symbol_type, 'bounded': bounded}
            state['symbols'][pc] = record
            if module.get('pdbUnmatched') or module.get('identityVerified') is False or module.get('mappedIdentityVerified') is False or (
                    symbol_type in ('pdb', 'dia') and module.get('mappedIdentityVerified') is not True):
                record['rejected'] = 'PDB identity is not verified'
                continue
            names[pc] = label
    return names


def resolve_named_symbol(process, module_name, name):
    """Exact named address, only when the disk image matches the mapped DLL."""
    state = initialize_symbols.state
    base, path = next((base, path) for base, path in modules.paths.items() if Path(path).name.lower() == module_name.lower())
    mapped, disk = state['mappedImages'][base], pe_identity(path)
    if not mapped.get('guid') or (mapped['guid'], mapped['age']) != (disk.get('guid'), disk.get('age')):
        raise RuntimeError(f'{module_name}: mapped DLL and disk image identity differ; refusing symbol address')
    provenance = state['modules'][base]
    if provenance.get('pdbUnmatched') or provenance.get('mappedIdentityVerified') is False or provenance.get('identityVerified') is False:
        raise RuntimeError(f'{module_name}: loaded PDB identity mismatch')
    d.SymFromNameW.argtypes = [w.HANDLE, w.LPCWSTR, c.POINTER(SYMBOL_INFOW)]
    d.SymFromNameW.restype = w.BOOL
    info = SYMBOL_INFOW()
    info.SizeOfStruct, info.MaxNameLen = 88, 511
    if not d.SymFromNameW(process, f'{Path(module_name).stem}!{name}', c.byref(info)):
        raise RuntimeError(f'Symbol missing in verified {module_name}: {name}')
    if not base <= info.Address < base + mapped['imageSize']:
        raise RuntimeError('Named symbol belongs to another image')
    return info.Address


def modules(process):
    handles = (w.HMODULE * 2048)()
    needed = w.DWORD()
    psapi.EnumProcessModulesEx.argtypes = [w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), w.DWORD]
    if not psapi.EnumProcessModulesEx(process, handles, c.sizeof(handles), c.byref(needed), 3):
        raise c.WinError(c.get_last_error())
    out = []
    paths = {}
    psapi.GetModuleFileNameExW.argtypes = [w.HANDLE, w.HMODULE, w.LPWSTR, w.DWORD]
    psapi.GetModuleInformation.argtypes = [w.HANDLE, w.HMODULE, c.c_void_p, w.DWORD]
    for i in range(needed.value // c.sizeof(w.HMODULE)):
        name = c.create_unicode_buffer(1024)
        psapi.GetModuleFileNameExW(process, handles[i], name, 1024)
        info = (c.c_uint64 * 3)()
        psapi.GetModuleInformation(process, handles[i], info, c.sizeof(info))
        out.append((info[0], info[0] + (info[1] & 0xFFFFFFFF), Path(name.value).name))
        paths[info[0]] = name.value
    modules.paths = paths
    return sorted(out)


def pdata_starts(exe):
    """Sorted function begin RVAs from the executable's exception directory."""
    data = Path(exe).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    sections = struct.unpack_from('<H', data, pe + 6)[0]
    optional = struct.unpack_from('<H', data, pe + 20)[0]
    exception_rva, exception_size = struct.unpack_from('<II', data, pe + 24 + 112 + 3 * 8)
    table = pe + 24 + optional
    for i in range(sections):
        _, vsize, vaddr, rsize, raw = struct.unpack_from('<8sIIII', data, table + 40 * i)
        if vaddr <= exception_rva < vaddr + max(vsize, rsize):
            start = raw + exception_rva - vaddr
            return sorted({struct.unpack_from('<I', data, start + 12 * n)[0] for n in range(exception_size // 12)})
    raise ValueError('No .pdata section')


def find_thread(pid, name):
    from thread_cpu_sample import threads, sample
    for tid in threads(pid):
        value = sample(tid)
        if value and value[1] == name:
            return tid
    raise RuntimeError(f'No thread named {name}')


def find_threads(pid, prefix):
    from thread_cpu_sample import threads, sample
    out = []
    for tid in threads(pid):
        value = sample(tid)
        if value and value[1].startswith(prefix):
            out.append(tid)
    if not out:
        raise RuntimeError(f'No thread named {prefix}*')
    return out


def sample_stacks(pid, tid, seconds, rate, depth, tids=None, symbol_path=None):
    """Round-robin over tids (or the one tid). Returns per-sample stacks and their thread ids."""
    tids = list(tids or [tid])
    process = k.OpenProcess(0x0400 | 0x0010, False, pid)
    handles = [k.OpenThread(0x0002 | 0x0008 | 0x0040, False, t) for t in tids]  # suspend/resume, context, query
    if not process or not all(handles):
        raise c.WinError(c.get_last_error())
    initialize_symbols(process, symbol_path)
    buffer = c.create_string_buffer(CONTEXT_SIZE + 16)
    address = (c.addressof(buffer) + 15) & ~15
    stacks, owners, failures, suspended = [], [], 0, 0.0
    period = 1.0 / rate
    end = time.perf_counter() + seconds
    turn = 0
    try:
        while time.perf_counter() < end:
            begin = time.perf_counter()
            index = turn % len(handles)
            turn += 1
            thread = handles[index]
            if k.SuspendThread(thread) == 0xFFFFFFFF:
                failures += 1
                continue
            try:
                c.memset(address, 0, CONTEXT_SIZE)
                struct.pack_into('<I', (c.c_char * CONTEXT_SIZE).from_address(address), OFF_FLAGS, CONTEXT_FULL)
                if not k.GetThreadContext(thread, c.c_void_p(address)):
                    failures += 1
                    continue
                view = (c.c_char * CONTEXT_SIZE).from_address(address)
                frame = STACKFRAME64()
                frame.AddrPC.Offset = struct.unpack_from('<Q', view, OFF_RIP)[0]
                frame.AddrStack.Offset = struct.unpack_from('<Q', view, OFF_RSP)[0]
                frame.AddrFrame.Offset = struct.unpack_from('<Q', view, OFF_RBP)[0]
                frame.AddrPC.Mode = frame.AddrStack.Mode = frame.AddrFrame.Mode = AddrModeFlat
                pcs = []
                while len(pcs) < depth and d.StackWalk64(0x8664, process, thread, c.byref(frame), c.c_void_p(address),
                                                         None, FUNCTION_TABLE, MODULE_BASE, None):
                    if not frame.AddrPC.Offset:
                        break
                    pcs.append(frame.AddrPC.Offset)
                stacks.append(pcs)
                owners.append(tids[index])
            finally:
                k.ResumeThread(thread)
                suspended += time.perf_counter() - begin
            remaining = period - (time.perf_counter() - begin)
            if remaining > 0:
                time.sleep(remaining)
        mods = modules(process)
        exe_base = next((m[0] for m in mods if m[2].lower().endswith('.exe')), None)
        unique = {pc - (1 if i else 0) for stack in stacks for i, pc in enumerate(stack)}
        exe_range = next(((m[0], m[1]) for m in mods if m[0] == exe_base), (0, 0))
        symbols = resolve_symbols(process, sorted(pc for pc in unique if not exe_range[0] <= pc < exe_range[1]))
    finally:
        d.SymCleanup(process)
        for thread in handles:
            k.CloseHandle(thread)
        k.CloseHandle(process)
    sample_stacks.owners = owners
    return stacks, mods, failures, suspended, symbols


def call_sites(stacks, mods, exe_name='Cyberpunk2077.exe', depth=8):
    """Raw exe call-site RVAs (return address - 1) of the first frames, for attribution inside functions."""
    exe = next(((base, end) for base, end, name in mods if name.lower() == exe_name.lower()), None)
    out = []
    for stack in stacks:
        row = []
        for i, pc in enumerate(stack[:depth]):
            at = pc - (1 if i else 0)
            row.append(f'{at - exe[0]:X}' if exe and exe[0] <= at < exe[1] else '-')
        out.append(row)
    return out


def symbolize(stacks, mods, exe_starts, exe_name='Cyberpunk2077.exe', symbols=None):
    bases = [m[0] for m in mods]

    def name(pc, caller):
        at = pc - (1 if caller else 0)
        i = bisect.bisect_right(bases, at) - 1
        if i < 0 or at >= mods[i][1]:
            return '?'
        base, _, module = mods[i]
        rva = at - base
        if symbols and (pc - (1 if caller else 0)) in symbols:
            return f'{module}!{symbols[pc - (1 if caller else 0)]}'
        if module.lower() == exe_name.lower() and exe_starts:
            j = bisect.bisect_right(exe_starts, rva) - 1
            if j >= 0:
                return f'exe+{exe_starts[j]:X}'
        return f'{module}+{rva:X}'

    return [[name(pc, index > 0) for index, pc in enumerate(stack)] for stack in stacks]


def aggregate(named):
    inclusive, exclusive, modules_ex = collections.Counter(), collections.Counter(), collections.Counter()
    for stack in named:
        if not stack:
            continue
        exclusive[stack[0]] += 1
        modules_ex[stack[0].split('!', 1)[0].split('+', 1)[0]] += 1
        for frame in set(stack):
            inclusive[frame] += 1
    total = len(named)
    pct = lambda counter: [(key, round(100 * value / total, 2)) for key, value in counter.most_common(80)]
    return dict(samples=total, inclusive=pct(inclusive), exclusive=pct(exclusive), exclusiveModules=pct(modules_ex))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--thread', default='GameThread')
    parser.add_argument('--tid', type=int, help='Sample this thread id instead of a named thread')
    parser.add_argument('--prefix', help='Round-robin over every thread whose name starts with this prefix')
    parser.add_argument('--seconds', type=float, default=8)
    parser.add_argument('--rate', type=float, default=100)
    parser.add_argument('--depth', type=int, default=64)
    parser.add_argument('--symbol-path', help='Additional directories containing exactly matching PDBs (semicolon-separated)')
    parser.add_argument('--exe', default=r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    tids = find_threads(args.pid, args.prefix) if args.prefix else [args.tid or find_thread(args.pid, args.thread)]
    tid = tids[0]
    started = time.perf_counter()
    stacks, mods, failures, suspended, symbols = sample_stacks(args.pid, tid, args.seconds, args.rate, args.depth, tids, args.symbol_path)
    elapsed = time.perf_counter() - started
    named = symbolize(stacks, mods, pdata_starts(args.exe), Path(args.exe).name, symbols)
    value = dict(pid=args.pid, tid=tid, thread=args.thread, seconds=elapsed, failures=failures,
                 suspendedFraction=suspended / elapsed, threads=tids, owners=sample_stacks.owners, **aggregate(named),
                 stacks=[' <- '.join(s[:24]) for s in named], callSites=[' <- '.join(s) for s in call_sites(stacks, mods, Path(args.exe).name)],
                 rawStacks=stacks, modules=mods, modulePaths=modules.paths, symbolSession=initialize_symbols.state)
    args.out.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8')
    print(json.dumps({key: value[key] for key in ('samples', 'failures', 'suspendedFraction')}, indent=1))
    for key, pct in value['inclusive'][:30]:
        print(f'{pct:6.2f}% incl  {key}')
    print('--- exclusive')
    for key, pct in value['exclusive'][:15]:
        print(f'{pct:6.2f}% excl  {key}')
