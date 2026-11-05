"""Hitch profiler for the live game (read-only apart from brief thread suspends for stack walks).

Exact frame intervals come from the port's Present hook (CyberpunkVR_PresentTiming[0] = present
count, [1] = sum of QPC ticks between presents, [6] = QPC frequency), polled at ~1 kHz. In the same
loop: GameThread call stacks (default 100 Hz), redDispatcher stacks round-robin (default 150 Hz in
total), and cycle counters of every thread that used CPU in a pre-scan (default every 4 ms).
The analysis compares long frames ("hitches") with normal frames.
"""
import argparse, bisect, collections, ctypes as c, ctypes.wintypes as w, json, statistics as st, struct, sys, time
from pathlib import Path
sys.path[:0] = [r'C:\Users\dariulone\Desktop\CyberpunkVRPort\tools\single_pass_tests',
                r'C:\Users\dariulone\Desktop\CyberpunkVRPort\tools\swimming_tests',
                str(Path(__file__).resolve().parent)]
import thread_stack_sampler as ts
from luajit_reader import Reader
from thread_cpu_sample import threads, sample, measure
from live_symbols import resolve

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
EXE = GAME / 'bin/x64/Cyberpunk2077.exe'
k, d = ts.k, ts.d
k.QueryThreadCycleTime.argtypes = [w.HANDLE, c.POINTER(c.c_uint64)]
k.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
ntdll = c.WinDLL('ntdll')
ntdll.NtQueryInformationThread.argtypes = [w.HANDLE, c.c_int, c.c_void_p, c.c_ulong, c.c_void_p]

p = argparse.ArgumentParser()
p.add_argument('--pid', type=int, required=True)
p.add_argument('--seconds', type=float, default=60)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--gt-rate', type=float, default=100)
p.add_argument('--worker-rate', type=float, default=150)
p.add_argument('--cycle-ms', type=float, default=4)
a = p.parse_args()

print('pre-scan threads...', flush=True)
scan = measure(a.pid, 1.0)
names = {r['tid']: r['name'] for r in scan['threads']}
tracked = [r['tid'] for r in scan['threads'] if r['percentOfCore'] > 0.5 or r['name'] in ('GameThread',)]
gt = next(t for t, n in names.items() if n == 'GameThread')
workers = [t for t, n in names.items() if n.startswith('redDispatcher')]
timing = int(resolve(a.pid, DLL, ['CyberpunkVR_PresentTiming'])['CyberpunkVR_PresentTiming'], 16)

process = k.OpenProcess(0x0400 | 0x0010, False, a.pid)
d.SymSetOptions(0x2 | 0x4 | 0x10)
if not d.SymInitializeW(process, None, True):
    raise c.WinError(c.get_last_error())
mods = ts.modules(process)
# LuaJIT lives in CET and, for the port's own mods, in the plugin's Lua host
lua_mods = [(m[0], m[1]) for m in mods if m[2].lower() in ('cyber_engine_tweaks.asi', 'cyberpunkvr_stereo.dll')]
LR = Reader(process)
h_stack = {t: k.OpenThread(0x0002 | 0x0008 | 0x0040, False, t) for t in [gt] + workers}
h_cyc = {t: k.OpenThread(0x0800 | 0x0040, False, t) for t in tracked}
start_addr = {}
for t, h in h_cyc.items():
    v = c.c_uint64()
    if ntdll.NtQueryInformationThread(h, 9, c.byref(v), 8, None) == 0:
        start_addr[t] = v.value
buf = c.create_string_buffer(ts.CONTEXT_SIZE + 16)
addr = (c.addressof(buf) + 15) & ~15
rbuf = (c.c_uint64 * 7)()


def read_timing():
    n = c.c_size_t()
    k.ReadProcessMemory(process, timing, rbuf, 56, c.byref(n))
    return rbuf[0], rbuf[1], rbuf[6]


def walk(h, lua=False):
    if k.SuspendThread(h) == 0xFFFFFFFF:
        return None
    try:
        c.memset(addr, 0, ts.CONTEXT_SIZE)
        view = (c.c_char * ts.CONTEXT_SIZE).from_address(addr)
        struct.pack_into('<I', view, ts.OFF_FLAGS, ts.CONTEXT_FULL)
        if not k.GetThreadContext(h, c.c_void_p(addr)):
            return None
        f = ts.STACKFRAME64()
        f.AddrPC.Offset = struct.unpack_from('<Q', view, ts.OFF_RIP)[0]
        f.AddrStack.Offset = struct.unpack_from('<Q', view, ts.OFF_RSP)[0]
        f.AddrFrame.Offset = struct.unpack_from('<Q', view, ts.OFF_RBP)[0]
        f.AddrPC.Mode = f.AddrStack.Mode = f.AddrFrame.Mode = ts.AddrModeFlat
        pcs = []
        while len(pcs) < 48 and d.StackWalk64(0x8664, process, h, c.byref(f), c.c_void_p(addr), None,
                                               ts.FUNCTION_TABLE, ts.MODULE_BASE, None):
            if not f.AddrPC.Offset:
                break
            pcs.append(f.AddrPC.Offset)
        chain = None
        if lua and any(a <= x < b for x in pcs for a, b in lua_mods):
            rdx, rbx = struct.unpack_from('<Q', view, 0x88)[0], struct.unpack_from('<Q', view, 0x90)[0]
            chain = []
            if LR.func(LR.q(rdx - 16)):
                if LR.L is None:
                    LR.find_state(rbx, rdx)
                chain = LR.chain(rdx)
            elif LR.L:
                rng = LR.stack_range()
                chain = LR.chain(rng[2]) if rng else []
        return (pcs, chain) if lua else pcs
    finally:
        k.ResumeThread(h)


def cycles():
    out = []
    for t in tracked:
        v = c.c_uint64()
        k.QueryThreadCycleTime(h_cyc[t], c.byref(v))
        out.append(v.value)
    return out


frames, gts, wks, cyc = [], [], [], []
count0, ticks0, freq = read_timing()
t0 = time.perf_counter()
end = t0 + a.seconds
next_gt = next_w = next_c = t0
wi = 0
print('sampling %.0f s: %d workers, %d tracked threads' % (a.seconds, len(workers), len(tracked)), flush=True)
while True:
    now = time.perf_counter()
    if now >= end:
        break
    cnt, tk, _ = read_timing()
    if cnt != count0:
        n = cnt - count0
        frames.append((now - t0, (tk - ticks0) * 1000.0 / freq / n, n))
        count0, ticks0 = cnt, tk
    if now >= next_c:
        cyc.append((time.perf_counter() - t0, cycles()))
        next_c = now + a.cycle_ms / 1000
    if now >= next_gt:
        ts_ = time.perf_counter() - t0
        r = walk(h_stack[gt], lua=True)
        if r and r[0]:
            gts.append((ts_, r[0], r[1]))
        next_gt = now + 1.0 / a.gt_rate
    elif now >= next_w:
        tid = workers[wi % len(workers)]
        wi += 1
        ts_ = time.perf_counter() - t0
        pcs = walk(h_stack[tid])
        if pcs:
            wks.append((ts_, tid, pcs))
        next_w = now + 1.0 / a.worker_rate
    time.sleep(0.0004)

exe_base = next(m[0] for m in mods if m[2].lower().endswith('.exe'))
exe_end = next(m[1] for m in mods if m[0] == exe_base)
unique = {pc - (1 if i else 0) for _, s, _ in gts for i, pc in enumerate(s)} | {pc - (1 if i else 0) for _, _, s in wks for i, pc in enumerate(s)}
symbols = ts.resolve_symbols(process, sorted(x for x in unique if not exe_base <= x < exe_end))
starts = {}
for t, sa in start_addr.items():
    sym = ts.resolve_symbols(process, [sa]).get(sa)
    mod = next((m for m in mods if m[0] <= sa < m[1]), None)
    starts[t] = (mod[2] + '!' + sym) if (mod and sym) else ('%s+%X' % (mod[2], sa - mod[0]) if mod else hex(sa))
d.SymCleanup(process)
exe_starts = ts.pdata_starts(EXE)
gt_named = ts.symbolize([s for _, s, _ in gts], mods, exe_starts, symbols=symbols)
wk_named = ts.symbolize([s for _, _, s in wks], mods, exe_starts, symbols=symbols)
# perfT0: time.perf_counter() at the start; on Windows that is QPC/frequency, so (perfT0 + t) * freq is the
# game's own QPC clock (node_timeline.py spans line up with these samples).
raw = dict(pid=a.pid, seconds=a.seconds, freq=freq, perfT0=t0, frames=frames,
           gt=[(t, s, ch) for (t, _, ch), s in zip(gts, gt_named)],
           workers=[(t, tid, s) for (t, tid, _), s in zip(wks, wk_named)],
           tracked=[dict(tid=t, name=names.get(t, ''), start=starts.get(t, '')) for t in tracked],
           cycles=cyc)
a.out.write_text(json.dumps(raw), encoding='utf-8')
print('frames %d  gt samples %d  worker samples %d  cycle snaps %d -> %s' % (len(frames), len(gts), len(wks), len(cyc), a.out))
