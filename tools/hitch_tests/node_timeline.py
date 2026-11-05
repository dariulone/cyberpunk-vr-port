"""When each view's render graph runs within the frame (the node timeline, Stereo/Profiler.cpp).

Turns CyberpunkVR_NodeTimeline on for a window, reads the span ring out of the live game and reports,
per frame (Present to Present): where the MAIN and VRCAM top-level nodes start and end, how many of each
are in flight, the frame-graph builds and the gaps -- the critical-path question the per-node totals
cannot answer.   args: --pid PID --seconds S --out spans.json   |   --analyze spans.json
Render-chain steps (kinds 6-15) need the game started with CVR_CHAIN_TIMELINE=1, job spans with names
(kinds 20/21) with CVR_JOB_TIMELINE=1 (src/Stereo/ViewChainTimeline.cpp)."""
import argparse, bisect, collections, ctypes as c, ctypes.wintypes as w, json, statistics as st, struct, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/swimming_tests')]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
KINDS = {0: 'MAIN', 1: 'VRCAM', 2: 'OTHER', 3: 'PRESENT', 4: 'BUILD_MAIN', 5: 'BUILD_VRCAM',
         6: 'SCENE_WAIT', 7: 'FRAME_SUBMIT', 8: 'VIEW_FLUSH', 9: 'VIEW_JOB', 10: 'GT_FRAME', 11: 'FRAME_TICK',
         12: 'SUBSCENE', 13: 'SUBSCENE_LIST', 14: 'WORLD_TICK', 15: 'GT_COUNTER_WAIT', 16: 'NESTED', 20: 'JOB', 21: 'GT_JOB'}
SPAN, COUNT = struct.Struct('<qqIIII'), 1 << 19


def capture(pid, seconds, out, level=1):
    from live_symbols import resolve
    names = ['CyberpunkVR_NodeTimeline', 'CyberpunkVR_NodeTimelineRing', 'CyberpunkVR_NodeTimelineCursor', 'CyberpunkVR_PresentTiming']
    sym = {k: int(v, 16) for k, v in resolve(pid, DLL, names).items() if k in names}
    k = c.WinDLL('kernel32', use_last_error=True)
    k.OpenProcess.restype = w.HANDLE
    k.ReadProcessMemory.argtypes = k.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    h = k.OpenProcess(0x0010 | 0x0020 | 0x0008 | 0x0400, False, pid)

    def read(addr, n):
        b = c.create_string_buffer(n); d = c.c_size_t()
        if not k.ReadProcessMemory(h, addr, b, n, c.byref(d)): raise OSError(c.get_last_error())
        return b.raw

    def write_i32(addr, v):
        b = struct.pack('<i', v); d = c.c_size_t()
        if not k.WriteProcessMemory(h, addr, b, 4, c.byref(d)): raise OSError(c.get_last_error())

    freq = struct.unpack('<Q', read(sym['CyberpunkVR_PresentTiming'] + 6 * 8, 8))[0]
    write_i32(sym['CyberpunkVR_NodeTimeline'], level)
    try:
        c0 = struct.unpack('<Q', read(sym['CyberpunkVR_NodeTimelineCursor'], 8))[0]
        time.sleep(seconds)
    finally:
        write_i32(sym['CyberpunkVR_NodeTimeline'], 0)
    time.sleep(0.2)
    c1 = struct.unpack('<Q', read(sym['CyberpunkVR_NodeTimelineCursor'], 8))[0]
    raw = read(sym['CyberpunkVR_NodeTimelineRing'], SPAN.size * COUNT)
    first = max(c0, c1 - COUNT)
    spans = []
    for i in range(first, c1):
        s, e, rva, kind, tid, extra = SPAN.unpack_from(raw, (i % COUNT) * SPAN.size)
        if e >= s > 0: spans.append((s, e, rva, kind, tid, extra))
    spans.sort()
    # job names (kinds 20/21 carry the profiler name string as an offset into the exe)
    names = {}
    wanted = {x[5] for x in spans if x[3] in (20, 21) and x[5] != 0xFFFFFFFF}
    if wanted:
        sys.path[:0] = [str(ROOT / 'tools/single_pass_tests')]
        import thread_stack_sampler as ts
        exe = next(m[0] for m in ts.modules(h) if m[2].lower().endswith('cyberpunk2077.exe'))
        for off in wanted:
            try: names[off] = read(exe + off, 96).split(bytes(1))[0].decode('utf-8', 'replace')
            except OSError: pass
    out.write_text(json.dumps(dict(freq=freq, written=c1 - c0, kept=len(spans), spans=spans, names=names)))
    print('spans written %d kept %d' % (c1 - c0, len(spans)))


def analyze(path):
    v = json.loads(Path(path).read_text())
    ms = 1000.0 / v['freq']
    spans = v['spans']
    presents = sorted(s[0] for s in spans if s[3] == 3)
    if len(presents) < 10: print('too few presents', len(presents)); return
    frames = list(zip(presents, presents[1:]))
    rows = []
    for a, b in frames:
        inside = [s for s in spans if a <= s[0] < b and s[3] != 3]
        r = dict(frameMs=(b - a) * ms)
        for kind, name in KINDS.items():
            if kind == 3: continue
            k = [s for s in inside if s[3] == kind]
            if not k: continue
            r[name] = dict(first=(min(s[0] for s in k) - a) * ms, last=(max(s[1] for s in k) - a) * ms,
                           busy=sum(s[1] - s[0] for s in k) * ms, n=len(k), threads=len({s[4] for s in k}))
        rows.append(r)
    print('frames %d  frame ms median %.2f' % (len(rows), st.median(r['frameMs'] for r in rows)))
    print('per frame, ms from the Present that opens it (median over frames):')
    for name in ('BUILD_VRCAM', 'VRCAM', 'BUILD_MAIN', 'MAIN', 'OTHER'):
        k = [r[name] for r in rows if name in r]
        if not k: continue
        print('  %-12s in %3d%% of frames  first %6.2f  last %6.2f  span %6.2f  busy(sum) %6.2f  nodes %5.0f  threads %4.1f' % (
            name, 100 * len(k) // len(rows), st.median(x['first'] for x in k), st.median(x['last'] for x in k),
            st.median(x['last'] - x['first'] for x in k), st.median(x['busy'] for x in k), st.median(x['n'] for x in k),
            st.median(x['threads'] for x in k)))
    # the render chain's own steps (Stereo/ViewChainTimeline.cpp): each occurrence inside a frame, by order
    chain = collections.defaultdict(list)
    for a, b in frames:
        seen = collections.Counter()
        for s in spans:
            if s[3] < 6 or not (a <= s[0] < b): continue
            name = KINDS[s[3]] + ('' if s[3] not in (8, 9, 12) else '/t%d/%06X' % (s[2] >> 24, s[2] & 0xFFFFFF))
            seen[name] += 1
            chain[(name, seen[name])].append(((s[0] - a) * ms, (s[1] - a) * ms, s[4]))
    if chain:
        print('render chain steps, ms from the Present (median; n = frames that had it):')
        for (name, k), v in sorted(chain.items(), key=lambda kv: st.median(x[0] for x in kv[1])):
            if len(v) < len(frames) // 4: continue
            print('  %-26s #%d  start %6.2f  end %6.2f  dur %6.2f  n %4d  threads %s' % (
                name, k, st.median(x[0] for x in v), st.median(x[1] for x in v), st.median(x[1] - x[0] for x in v), len(v),
                ','.join(str(t) for t, _ in collections.Counter(x[2] for x in v).most_common(2))))
    # in-flight node count per view across the frame, 0.5 ms bins
    bins = 40
    flight = {0: [0.0] * bins, 1: [0.0] * bins}
    for a, b in frames:
        L = (b - a) * ms
        for s in spans:
            if s[3] not in (0, 1) or s[1] < a or s[0] >= b: continue
            s0, s1 = max(s[0], a), min(s[1], b)
            for i in range(int((s0 - a) * ms / 0.5), min(bins, int((s1 - a) * ms / 0.5) + 1)):
                lo, hi = a + i * 0.5 / ms, a + (i + 1) * 0.5 / ms
                ov = max(0, min(s1, hi) - max(s0, lo))
                flight[s[3]][i] += ov / (0.5 / ms)
    n = len(frames)
    print('nodes in flight (mean over frames), 0.5 ms bins from the Present:')
    for i in range(bins):
        m, vr = flight[0][i] / n, flight[1][i] / n
        if m < .02 and vr < .02: continue
        print('  %5.1f ms  MAIN %4.2f %-12s VRCAM %4.2f %s' % (i * .5, m, '#' * int(m * 4), vr, '#' * int(vr * 4)))
    # the largest gaps inside each view's node stream (no node of that view running)
    for kind in (1, 0):
        gaps = collections.Counter()
        for a, b in frames:
            k = sorted((s[0], s[1], s[2]) for s in spans if s[3] == kind and a <= s[0] < b)
            end, prev = None, None
            for s0, s1, rva in k:
                if end is not None and s0 > end:
                    gaps['%06X->%06X' % (prev, rva)] += (s0 - end) * ms
                if end is None or s1 > end: end, prev = s1, rva
        print('%s idle gaps between nodes, ms/frame (top):' % KINDS[kind])
        for key, val in gaps.most_common(8): print('   %6.3f  %s' % (val / n, key))


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--pid', type=int); p.add_argument('--seconds', type=float, default=3)
    p.add_argument('--out', type=Path); p.add_argument('--analyze', type=Path); p.add_argument('--level', type=int, default=1, help='2 = nested nodes too')
    a = p.parse_args()
    if a.analyze: analyze(a.analyze)
    else:
        capture(a.pid, a.seconds, a.out, a.level); analyze(a.out)
