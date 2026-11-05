"""Where the GPU waits: reads a capture of src/Render/GpuQueueTimeline.cpp (hud/gpuq_<tag>.json from
build/monowire-housing-20261016/probe_gpuq.py) and splits every idle stretch of the GPU (no queue running
anything) between Present marks into

  starved  the next batch reached the driver after the GPU had already gone idle (CPU too late),
  held     the next batch was submitted before the idle stretch began but started after it ended
           (a fence wait on another queue, or the driver holding it).

Usage: gpu_queue_timeline.py <gpuq_x.json> [more.json ...] [--frames N]
"""
import json, sys, statistics
from collections import defaultdict


def load(path):
    with open(path, encoding='utf-8') as f:
        d = json.load(f)
    calib = d['calib']
    qpcf = calib[6]
    def to_ms(kind, gpu):
        freq, g0, c0 = calib[kind * 3], calib[kind * 3 + 1], calib[kind * 3 + 2]
        return ((gpu - g0) / freq + c0 / qpcf) * 1000.0
    subs, presents = [], []
    for r in d['records']:
        kind = r[0] & 0xFF
        if kind == 2:
            presents.append(r[2] * 1000.0 / qpcf)
            continue
        if kind > 2:   # queue Signal / Wait, see load_sync
            continue
        if not r[4] or not r[5]:
            continue
        subs.append(dict(kind=kind, lists=r[0] >> 8, queue=r[1], call=r[2] * 1000.0 / qpcf, ret=r[3] * 1000.0 / qpcf,
                         begin=to_ms(kind, r[4]), end=to_ms(kind, r[5])))
    return subs, presents


def load_sync(path):
    """Queue Signal (kind 3) and Wait (kind 4) calls of a capture: queue, call time (ms), fence, value."""
    with open(path, encoding='utf-8') as f:
        d = json.load(f)
    qpcf = d['calib'][6]
    return [dict(kind='signal' if (r[0] & 0xFF) == 3 else 'wait', queue=r[1], call=r[2] * 1000.0 / qpcf, fence=r[3], value=r[4])
            for r in d['records'] if (r[0] & 0xFF) in (3, 4)]


def union(intervals):
    out = []
    for a, b in sorted(intervals):
        if out and a <= out[-1][1]:
            out[-1][1] = max(out[-1][1], b)
        else:
            out.append([a, b])
    return out


def analyse(path, frames_limit):
    subs, presents = load(path)
    print('==', path, len(subs), 'submissions,', len(presents), 'presents')
    if len(presents) < 3:
        print('   too few frames'); return None
    queues = defaultdict(list)
    for s in subs:
        queues[(s['kind'], s['queue'])].append(s)
    names = {}
    for i, (k, lst) in enumerate(sorted(queues.items(), key=lambda kv: -len(kv[1]))):
        names[k] = ('direct' if k[0] == 0 else 'compute') + '#%d' % i
    frames = [(presents[i], presents[i + 1]) for i in range(len(presents) - 1)]
    # The first and last frames of a capture are partial; skip one at each end.
    frames = frames[1:-1][:frames_limit] if frames_limit else frames[1:-1]
    busy_all = union([(s['begin'], s['end']) for s in subs])
    per_frame = []
    per_queue = defaultdict(lambda: dict(busy=0.0, subs=0, lists=0))
    gaps_by_kind = defaultdict(list)
    biggest = []
    for (f0, f1) in frames:
        length = f1 - f0
        busy = 0.0
        covered = []
        for a, b in busy_all:
            a2, b2 = max(a, f0), min(b, f1)
            if b2 > a2:
                busy += b2 - a2
                covered.append((a2, b2))
        # idle stretches inside the frame
        idle = []
        t = f0
        for a, b in covered:
            if a > t: idle.append((t, a))
            t = max(t, b)
        if t < f1: idle.append((t, f1))
        starved = held = 0.0
        for g0, g1 in idle:
            # the batch that ended the stretch: the earliest begin at or after g1 (within 0.01 ms)
            nxt = None
            for s in subs:
                if s['begin'] >= g1 - 0.01 and (nxt is None or s['begin'] < nxt['begin']):
                    nxt = s
            gap = g1 - g0
            if nxt is None:
                kind = 'tail'
            elif nxt['call'] >= g0:
                kind = 'starved'; starved += gap
            else:
                kind = 'held'; held += gap
            gaps_by_kind[kind].append(gap)
            if nxt is not None:
                biggest.append((gap, kind, g0 - f0, names[(nxt['kind'], nxt['queue'])], nxt['call'] - g0, nxt['begin'] - nxt['call'], nxt['lists']))
        for k, lst in queues.items():
            qb = union([(max(s['begin'], f0), min(s['end'], f1)) for s in lst if s['end'] > f0 and s['begin'] < f1])
            per_queue[k]['busy'] += sum(b - a for a, b in qb)
            inside = [s for s in lst if f0 <= s['call'] < f1]
            per_queue[k]['subs'] += len(inside)
            per_queue[k]['lists'] += sum(s['lists'] for s in inside)
        # where the GPU work of this frame starts / ends relative to Present
        per_frame.append(dict(length=length, busy=busy, idle=length - busy, starved=starved, held=held, gaps=len(idle)))
    n = len(per_frame)
    med = lambda key: statistics.median(f[key] for f in per_frame)
    mean = lambda key: statistics.mean(f[key] for f in per_frame)
    print('   frames %d  frame %.3f ms  GPU busy %.3f ms (%.1f%%)  idle %.3f  starved %.3f  held %.3f  gaps/frame %.1f' % (
        n, mean('length'), mean('busy'), 100 * mean('busy') / mean('length'), mean('idle'), mean('starved'), mean('held'), mean('gaps')))
    for k, q in sorted(per_queue.items(), key=lambda kv: -kv[1]['subs']):
        print('   %-10s queue %#x  busy %.3f ms/frame  submissions %.1f/frame  lists %.1f/frame' % (
            names[k], k[1], q['busy'] / n, q['subs'] / n, q['lists'] / n))
    for kind, lst in gaps_by_kind.items():
        print('   gaps %-8s count %5d  total %.3f ms/frame  median %.3f ms  p90 %.3f ms' % (
            kind, len(lst), sum(lst) / n, statistics.median(lst), sorted(lst)[int(len(lst) * 0.9)]))
    # Same-position gaps across frames: bucket the stretch start (ms after Present) to see recurring ones.
    buckets = defaultdict(lambda: [0.0, 0, defaultdict(int)])
    for gap, kind, at, q, late, lat, lists in biggest:
        b = buckets[(kind, round(at * 2) / 2)]
        b[0] += gap; b[1] += 1; b[2][q] += 1
    print('   recurring idle stretches (by start after Present, 0.5 ms buckets), ms/frame:')
    for (kind, at), (tot, cnt, qs) in sorted(buckets.items(), key=lambda kv: -kv[1][0])[:14]:
        print('     %-8s at %5.1f ms  %.3f ms/frame  (%d stretches; next batch on %s)' % (
            kind, at, tot / n, cnt, ', '.join('%s x%d' % kv for kv in sorted(qs.items(), key=lambda kv: -kv[1]))))
    lat = [s['begin'] - s['ret'] for s in subs]
    print('   submit-return -> begin: median %.3f ms  p10 %.3f  p90 %.3f' % (
        statistics.median(lat), sorted(lat)[len(lat) // 10], sorted(lat)[len(lat) * 9 // 10]))
    return per_frame


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    frames = 0
    if '--frames' in sys.argv:
        frames = int(sys.argv[sys.argv.index('--frames') + 1])
        args = [a for a in args if a != str(frames)]
    for p in args:
        analyse(p, frames)


if __name__ == '__main__':
    main()
