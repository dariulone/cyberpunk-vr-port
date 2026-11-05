"""worldtick_serial.py spans.json [top]: what keeps the world tick long (node_timeline.py --out, game started with
CVR_CHAIN_TIMELINE=1 CVR_JOB_TIMELINE=1).

Inside every world tick (kind 14) the named job spans (kind 20 workers, 21 game thread) are swept: per frame, how
long nothing ran, one job ran alone, two, or more; and per job name, its time inside the tick, its time running
alone and with one other. A job that runs alone is the tick's serial path: making it shorter makes the frame
shorter, the rest overlaps."""
import collections
import json
import statistics as st
import sys

v = json.load(open(sys.argv[1], encoding='utf-8'))
top = int(sys.argv[2]) if len(sys.argv) > 2 else 30
ms = 1000.0 / v['freq']
names = v.get('names', {})
spans = v['spans']
ticks = sorted((s for s in spans if s[3] == 14), key=lambda s: s[0])
jobs = sorted((s for s in spans if s[3] in (20, 21)), key=lambda s: s[0])


def label(s):
    n = names.get(str(s[5]))
    return (n or 'job %X' % s[2]) + (' [GT]' if s[3] == 21 else '')


levels = collections.defaultdict(list)       # per frame: ms with 0, 1, 2, 3+ jobs
inside = collections.Counter()
alone = collections.Counter()
pair = collections.Counter()
count = collections.Counter()
frames = 0
j0 = 0
for t in ticks:
    a, b = t[0], t[1]
    while j0 < len(jobs) and jobs[j0][1] < a - v['freq']:   # jobs are sorted by start; skip ones long gone
        j0 += 1
    events = []
    for s in jobs[j0:]:
        if s[0] > b:
            break
        lo, hi = max(s[0], a), min(s[1], b)
        if hi <= lo:
            continue
        events.append((lo, 1, s))
        events.append((hi, -1, s))
    if not events:
        continue
    frames += 1
    events.sort(key=lambda e: (e[0], e[1]))
    running = {}
    at = a
    spent = [0.0, 0.0, 0.0, 0.0]
    for x, d, s in events:
        if x > at:
            n = len(running)
            spent[min(n, 3)] += (x - at) * ms
            for r in running.values():
                inside[label(r)] += (x - at) * ms
                if n == 1:
                    alone[label(r)] += (x - at) * ms
                elif n == 2:
                    pair[label(r)] += (x - at) * ms
            at = x
        key = id(s)
        if d > 0:
            running[key] = s
            count[label(s)] += 1
        else:
            running.pop(key, None)
    if b > at:
        spent[0] += (b - at) * ms
    for i, x in enumerate(spent):
        levels[i].append(x)

print('world ticks %d, tick median %.2f ms' % (frames, st.median([(t[1] - t[0]) * ms for t in ticks])))
for i, n in enumerate(('none running', 'one alone', 'two', 'three or more')):
    print('  %-14s median %6.2f ms  p90 %6.2f' % (n, st.median(levels[i]), sorted(levels[i])[int(.9 * len(levels[i]))]))
print('-- jobs by time running alone (ms per tick): alone, with one other, inside the tick, runs per tick')
for name, t in alone.most_common(top):
    print('  %7.3f  %7.3f  %7.3f  %5.1f  %s' % (t / frames, pair[name] / frames, inside[name] / frames,
                                                count[name] / frames, name))
