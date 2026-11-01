"""Frame cost of a live game over a window, for A/B runs of the Lua mods' host (CET or the plugin's, src/Lua).

Frame intervals come from the port's Present hook (CyberpunkVR_PresentTiming: [0] present count, [1] QPC ticks
summed between presents, [6] QPC frequency), polled at ~1 kHz. CPU time per frame of GameThread (where onUpdate
runs) and of the render thread that presents (where CET draws its overlay) comes from GetThreadTimes at the window's
ends. Read-only: no thread is suspended.   args: --pid PID [--seconds 60] [--out result.json] [--label text]
"""
import argparse
import json
import statistics as st
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE.parent / 'single_pass_tests'), str(HERE.parent / 'swimming_tests')]
import ctypes as c                                    # noqa: E402
from thread_cpu_sample import k, threads, sample      # noqa: E402
from live_symbols import resolve                      # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--seconds', type=float, default=60)
    p.add_argument('--out', type=Path)
    p.add_argument('--label', default='')
    a = p.parse_args()

    timing = int(resolve(a.pid, DLL, ['CyberpunkVR_PresentTiming'])['CyberpunkVR_PresentTiming'], 16)
    k.OpenProcess.restype = c.c_void_p
    process = k.OpenProcess(0x0010, False, a.pid)     # VM_READ
    k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    buf = (c.c_uint64 * 7)()

    def read_timing():
        n = c.c_size_t()
        k.ReadProcessMemory(process, timing, buf, 56, c.byref(n))
        return buf[0], buf[1], buf[6]

    before = {tid: sample(tid) for tid in threads(a.pid)}
    count0, ticks0, freq = read_timing()
    first_count = count0
    intervals = []
    t0 = time.perf_counter()
    while time.perf_counter() - t0 < a.seconds:
        cnt, tk, _ = read_timing()
        if cnt != count0:
            n = cnt - count0
            intervals += [(tk - ticks0) * 1000.0 / freq / n] * n
            count0, ticks0 = cnt, tk
        time.sleep(0.0005)
    elapsed = time.perf_counter() - t0
    after = {tid: sample(tid) for tid in threads(a.pid)}
    frames = count0 - first_count

    rows = []
    for tid, now in after.items():
        was = before.get(tid)
        if now and was:
            rows.append(dict(tid=tid, name=now[1], ms=1000 * (now[0] - was[0])))
    rows.sort(key=lambda r: -r['ms'])
    game = next((r for r in rows if r['name'] == 'GameThread'), None)
    render = next((r for r in rows if r['name'] == 'RenderThread'), None)
    q = st.quantiles(intervals, n=100) if len(intervals) > 100 else [0] * 99
    median = st.median(intervals) if intervals else 0
    result = dict(label=a.label, seconds=round(elapsed, 2), frames=frames,
                  fps=round(frames / elapsed, 2),
                  frameMs=dict(mean=round(st.fmean(intervals), 3) if intervals else 0, p50=round(q[49], 3),
                               p99=round(q[98], 3), max=round(max(intervals), 3) if intervals else 0),
                  hitches=sum(1 for x in intervals if x > 2 * median),
                  gameThreadMsPerFrame=round(game['ms'] / frames, 3) if game and frames else None,
                  renderThreadMsPerFrame=round(render['ms'] / frames, 3) if render and frames else None,
                  processCpuMsPerFrame=round(sum(r['ms'] for r in rows) / frames, 3) if frames else None,
                  top=[(r['name'] or str(r['tid']), round(r['ms'] / frames, 3)) for r in rows[:8]] if frames else [])
    print(json.dumps(result, indent=1))
    if a.out:
        a.out.write_text(json.dumps(result, indent=1), encoding='utf-8')


if __name__ == '__main__':
    main()
