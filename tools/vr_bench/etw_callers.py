"""Callers of a kind of work in a parsed stack trace (etw_stacks.py's cpu.stacks.pickle): the port function nearest
above each sample whose stack contains a frame matching --what (a regex over "module!function" names), with the
matching frame's name.

    py -3.13 -B tools/vr_bench/etw_callers.py build/vr-bench/<run>/cpu.etl --what "alloc|RtlAllocateHeap|free" [--fps 54.3]
    py -3.13 -B tools/vr_bench/etw_callers.py ... --what "ntdll" --leaf     # only samples whose leaf matches
"""
import argparse
import bisect
import pickle
import re
from collections import Counter
from pathlib import Path

import etw_cpu

DLL = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\cyberpunkvrport.dll')


def lines(a, pid, images, tp, samples, span):
    """--lines FUNC: the source lines the port function's own samples (its leaf) fall on."""
    import ctypes as c
    import ctypes.wintypes as w
    mods = sorted((b, s, Path(p.replace('\\', '/')).name) for b, (s, p) in images[pid].items())
    port = next(m for m in mods if m[2].lower() == 'cyberpunkvrport.dll')
    sym = etw_cpu.Symbols(str(DLL), port[0], port[1])

    class Line(c.Structure):
        _fields_ = [('size', w.DWORD), ('key', c.c_void_p), ('line', w.DWORD), ('file', c.c_wchar_p), ('address', c.c_ulonglong)]
    sym.d.SymGetLineFromAddrW64.argtypes = [c.c_void_p, c.c_ulonglong, c.POINTER(w.DWORD), c.POINTER(Line)]
    sym.d.SymSetOptions(2 | 4 | 0x10 | 0x80000)   # + SYMOPT_LOAD_LINES
    pat = re.compile(a.lines, re.I)
    hist, total = Counter(), 0
    for tid, frames, _ in samples:
        if tp.get(tid) != pid or not frames:
            continue
        ip = frames[0]
        if not (port[0] <= ip < port[0] + port[1]):
            continue
        name = sym.name(ip)
        if not pat.search(name):
            continue
        total += 1
        ln, disp = Line(), w.DWORD()
        ln.size = c.sizeof(Line)
        where = f'{Path(ln.file).name}:{ln.line}' if sym.d.SymGetLineFromAddrW64(sym.h, ip, c.byref(disp), c.byref(ln)) else '?'
        hist[(name.split('::')[-1], where)] += 1
    seconds = (span[1] - span[0]) / 1e7
    scale = 1.0 / (seconds * a.fps) if a.fps else 1.0 / seconds
    print(f'{total} leaf samples in functions matching {a.lines!r} ({total * scale:.3f} {"ms/frame" if a.fps else "ms/s"})')
    for (fn, where), n in hist.most_common(a.top):
        print(f'  {n * scale:8.3f}  {n:6d}  {fn}  {where}')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('etl')
    ap.add_argument('--what')
    ap.add_argument('--lines', help='a port function (regex): its own samples by source line')
    ap.add_argument('--leaf', action='store_true')
    ap.add_argument('--fps', type=float, default=0.0)
    ap.add_argument('--top', type=int, default=30)
    a = ap.parse_args()
    pid, images, tp, tn, samples, span = pickle.loads(Path(a.etl).with_suffix('.stacks.pickle').read_bytes())
    if a.lines:
        return lines(a, pid, images, tp, samples, span)
    mods = sorted([(b, s, Path(p.replace('\\', '/')).name) for b, (s, p) in images[pid].items()] +
                  [(b, s, Path(p.replace('\\', '/')).name) for b, (s, p) in images.get(0, {}).items()])
    bases = [m[0] for m in mods]
    port = next(m for m in mods if m[2].lower() == 'cyberpunkvrport.dll')
    sym = etw_cpu.Symbols(str(DLL), port[0], port[1])
    pat = re.compile(a.what, re.I)
    cache = {}

    def label(ip):
        v = cache.get(ip)
        if v is None:
            i = bisect.bisect_right(bases, ip) - 1
            m = mods[i] if i >= 0 and ip < mods[i][0] + mods[i][1] else None
            if m is port:
                v = ('port', 'port!' + sym.name(ip))
            else:
                v = ('other', (m[2] if m else '?') + '!' + ('%x' % (ip - m[0]) if m else ''))
            cache[ip] = v
        return v
    callers = Counter()
    total = 0
    for tid, frames, _ in samples:
        if tp.get(tid) != pid:
            continue
        labels = [label(ip) for ip in frames]
        idx = next((i for i, (_, n) in enumerate(labels) if pat.search(n)), None)
        if idx is None or (a.leaf and idx != 0):
            continue
        total += 1
        what = labels[idx][1]
        caller = next((n for k, n in labels[idx + 1:] if k == 'port' and not pat.search(n)), '(no port caller)')
        callers[(caller, re.sub(r'!.*', '', what) if labels[idx][0] == 'other' else what)] += 1
    seconds = (span[1] - span[0]) / 1e7
    scale = 1.0 / (seconds * a.fps) if a.fps else 1.0 / seconds
    print(f'{total} samples match ({total * scale:.3f} {"ms/frame" if a.fps else "ms/s"})')
    for (caller, what), n in callers.most_common(a.top):
        print(f'  {n * scale:8.3f}  {caller}  <-  {what}')


if __name__ == '__main__':
    main()
