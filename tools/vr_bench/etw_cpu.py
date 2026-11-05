"""CPU samples of one process from a Windows Performance Recorder trace: by module, by thread and, for a module whose
PDB is at hand (the port's DLL), by function.

start() / stop(etl) drive wpr.exe with its built-in CPU profile (a sample per core every 1 ms; needs an elevated
shell). analyse(etl, pid) converts the trace with tracerpt.exe to a CSV beside it (deleted afterwards) and counts the
SampledProfile events of the process's threads; each sample stands for ~1 ms of CPU on one core.

  py -3.13 -B tools/vr_bench/etw_cpu.py trace.etl --pid 1234 [--symbols C:/path/cyberpunkvrport.dll]
"""
import argparse
import bisect
import ctypes as c
import ctypes.wintypes as w
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from pathlib import Path

USER = 19   # first user-data column of a tracerpt CSV line (after Clock-Time, Kernel(ms), User(ms))


def start():
    subprocess.run(['wpr.exe', '-cancel'], capture_output=True)   # a profile left running by a failed run
    r = subprocess.run(['wpr.exe', '-start', 'CPU', '-filemode'], capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError(f'wpr -start failed ({r.returncode}): {r.stdout.strip()} {r.stderr.strip()}')


def stop(etl):
    etl = Path(etl)
    etl.parent.mkdir(parents=True, exist_ok=True)
    r = subprocess.run(['wpr.exe', '-stop', str(etl)], capture_output=True, text=True)
    if r.returncode or not etl.exists():
        raise RuntimeError(f'wpr -stop failed ({r.returncode}): {r.stdout.strip()} {r.stderr.strip()}')
    return etl


def _int(text):
    text = text.strip()
    return int(text, 16) if text.lower().startswith('0x') else int(text)


def _quoted(line):
    return re.findall(r'"([^"]*)"', line)


def _name_from_hex(text):
    text = text.strip()
    if text.startswith('"'):
        return text.strip('"')
    try:
        return bytes.fromhex(text[2:]).decode('utf-16-le').split('\x00')[0]
    except ValueError:
        return ''


class Symbols:
    """dbghelp over one module loaded at the base the trace saw it at."""
    class Symbol(c.Structure):
        _fields_ = [('size', w.ULONG), ('type', w.ULONG), ('reserved', c.c_ulonglong * 2), ('index', w.ULONG),
                    ('bytes', w.ULONG), ('base', c.c_ulonglong), ('flags', w.ULONG), ('value', c.c_ulonglong),
                    ('address', c.c_ulonglong), ('register', w.ULONG), ('scope', w.ULONG), ('tag', w.ULONG),
                    ('length', w.ULONG), ('maxLength', w.ULONG), ('name', c.c_char * 1024)]

    def __init__(self, dll, base, size):
        self.d = c.WinDLL('dbghelp', use_last_error=True)
        self.h = c.c_void_p(0x45545743)
        d = self.d
        d.SymInitializeW.argtypes = [c.c_void_p, w.LPCWSTR, w.BOOL]
        d.SymLoadModuleExW.argtypes = [c.c_void_p, w.HANDLE, w.LPCWSTR, w.LPCWSTR, c.c_ulonglong, w.DWORD, c.c_void_p,
                                       w.DWORD]
        d.SymLoadModuleExW.restype = c.c_ulonglong
        d.SymFromAddr.argtypes = [c.c_void_p, c.c_ulonglong, c.POINTER(c.c_ulonglong), c.POINTER(Symbols.Symbol)]
        d.SymCleanup.argtypes = [c.c_void_p]
        d.SymSetOptions(2 | 4 | 0x80000)   # undecorate, deferred loads, no UI
        if not d.SymInitializeW(self.h, str(Path(dll).parent), False):
            raise c.WinError(c.get_last_error())
        if not d.SymLoadModuleExW(self.h, None, str(dll), None, base, size, None, 0):
            d.SymCleanup(self.h)
            raise c.WinError(c.get_last_error())
        self.cache = {}

    def name(self, address):
        if address in self.cache:
            return self.cache[address]
        s = Symbols.Symbol()
        s.size, s.maxLength = 88, 1023
        disp = c.c_ulonglong(0)
        value = s.name.decode(errors='replace') if self.d.SymFromAddr(self.h, address, c.byref(disp), c.byref(s)) \
            else '?'
        self.cache[address] = value
        return value

    def close(self):
        self.d.SymCleanup(self.h)


def filetime_now():
    """The trace's clock: 100 ns since 1601 (system time)."""
    import time
    return time.time_ns() // 100 + 116444736000000000


class ExeFunctions:
    """Function bounds of the game's exe (`rva_start\trva_end\tname` lines, exported from the IDA database of the
    same build): an exe sample's function and its name (sub_RVA when unnamed)."""
    def __init__(self, path):
        self.starts, self.ends, self.names = [], [], []
        with open(path, encoding='utf-8') as f:
            for line in f:
                a, b, n = line.rstrip('\n').split('\t')
                self.starts.append(int(a, 16)); self.ends.append(int(b, 16)); self.names.append(n)

    def find(self, rva):
        i = bisect.bisect_right(self.starts, rva) - 1
        if i >= 0 and rva < self.ends[i]:
            return self.starts[i], self.names[i]
        return None, '?'


def analyse(etl, pid=None, process=None, symbols=None, top=25, keep_csv=False, window=None, exe_functions=None):
    """Samples of process `pid` (or the first process named `process`) in trace `etl`. `symbols`: a DLL path whose
    PDB lies beside it; its samples are also counted per function. `window`: (start, end) in filetime_now() units;
    only samples inside it count (wpr keeps sampling while it starts and stops)."""
    etl = Path(etl)
    csv = etl.with_suffix('.csv')
    r = subprocess.run(['tracerpt.exe', str(etl), '-o', str(csv), '-of', 'CSV', '-y'], capture_output=True, text=True)
    if r.returncode or not csv.exists():
        raise RuntimeError(f'tracerpt failed ({r.returncode}): {r.stdout.strip()[-300:]}')
    try:
        return count(csv, pid, process, symbols, top, window, exe_functions)
    finally:
        if not keep_csv:
            csv.unlink(missing_ok=True)


def count(csv, pid=None, process=None, symbols=None, top=25, window=None, exe_functions=None):
    """analyse() on a tracerpt CSV."""
    images = defaultdict(dict)        # pid -> base -> (size, path)
    thread_pid, thread_name = {}, {}
    samples = []                      # (tid, ip)
    names = {}                        # pid -> process image name
    first = last = None
    with open(csv, encoding='utf-8', errors='replace') as f:
        for line in f:
            head = line[:40]
            if 'SampleProf' in head:
                p = line.split(',')
                if len(p) > USER + 2:
                    t = int(p[16])
                    if window and not window[0] <= t <= window[1]:
                        continue
                    samples.append((int(p[USER + 1]), int(p[USER], 16)))
                    first = t if first is None else first
                    last = t
            elif head.lstrip().startswith('Image,'):
                p = line.split(',')
                kind = p[1].strip()
                if kind in ('DCStart', 'Load') and len(p) > USER + 2:
                    q = _quoted(line)
                    images[_int(p[USER + 2])][_int(p[USER])] = (_int(p[USER + 1]), q[-1] if q else '?')
            elif head.lstrip().startswith('Thread,'):
                p = line.split(',')
                kind = p[1].strip()
                if kind in ('DCStart', 'Start') and len(p) > USER + 1:
                    tid = _int(p[USER + 1])
                    thread_pid[tid] = _int(p[USER])
                    n = _name_from_hex(p[-1])
                    if n:
                        thread_name[tid] = n
                elif kind == 'SetName' and len(p) > USER + 1:
                    q = _quoted(line)
                    if q:
                        thread_name[_int(p[USER + 1])] = q[-1]
            elif head.lstrip().startswith('Process,'):
                p = line.split(',')
                if p[1].strip() in ('DCStart', 'Start') and len(p) > USER + 1:
                    q = _quoted(line)
                    exe = next((x for x in q if x.lower().endswith('.exe')), '')
                    names.setdefault(_int(p[USER + 1]), exe)
    if pid is None and process:
        pid = next((k for k, v in names.items() if v.lower() == process.lower()), None)
        if pid is None:
            raise RuntimeError(f'no process {process} in the trace')
    mods = sorted([(b, s, Path(path.replace('\\', '/')).name) for b, (s, path) in images[pid].items()] +
                  [(b, s, Path(path.replace('\\', '/')).name) for b, (s, path) in images[0].items()])
    bases = [m[0] for m in mods]
    sym, sym_mod = None, None
    if symbols:
        target = Path(symbols).name.lower()
        hit = next((m for m in mods if m[2].lower() == target), None)
        if hit:
            sym, sym_mod = Symbols(symbols, hit[0], hit[1]), hit[2]
    by_module, by_thread, by_function = Counter(), Counter(), Counter()
    exe = ExeFunctions(exe_functions) if exe_functions else None
    exe_mod = next((m for m in mods if m[2].lower() == (names.get(pid, '') or '').lower()), None) if exe else None
    by_exe, by_exe_thread = Counter(), Counter()   # (rva, name) ; (thread group, rva, name)
    total = 0
    try:
        for tid, ip in samples:
            if thread_pid.get(tid) != pid:
                continue
            total += 1
            i = bisect.bisect_right(bases, ip) - 1
            module = mods[i][2] if i >= 0 and ip < mods[i][0] + mods[i][1] else '?'
            by_module[module] += 1
            by_thread[re.sub(r'\d+$', '#', thread_name.get(tid, '?'))] += 1
            if sym and module == sym_mod:
                by_function[sym.name(ip)] += 1
            if exe_mod and module == exe_mod[2]:
                rva, fname = exe.find(ip - exe_mod[0])
                key = ('%x' % rva if rva is not None else '?', fname)
                by_exe[key] += 1
                by_exe_thread[(re.sub(r'\d+$', '#', thread_name.get(tid, '?')),) + key] += 1
    finally:
        if sym:
            sym.close()
    seconds = (window[1] - window[0]) / 1e7 if window else \
        (last - first) / 1e7 if first is not None and last > first else 0.0
    return dict(pid=pid, process=names.get(pid, ''), seconds=seconds, samples=total,
                modules=by_module.most_common(top), threads=by_thread.most_common(top),
                functions=by_function.most_common(top), symbolsFor=sym_mod,
                exeFunctions=[(k[0], k[1], n) for k, n in by_exe.most_common(top)],
                exeByThread=[(k[0], k[1], k[2], n) for k, n in by_exe_thread.most_common(top * 2)])


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('etl')
    p.add_argument('--pid', type=int)
    p.add_argument('--process')
    p.add_argument('--symbols')
    p.add_argument('--top', type=int, default=25)
    a = p.parse_args()
    print(json.dumps(analyse(a.etl, a.pid, a.process, a.symbols, a.top), indent=1))


if __name__ == '__main__':
    main()
