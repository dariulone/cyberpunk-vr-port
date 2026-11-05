"""gtframe.py spans.json: the game-thread frame anatomy from the chain timeline (kinds 10 GT frame, 14 world tick,
15 counter waits, 6 scene wait, 7 frame submit, 3 present): medians per GT frame."""
import json, sys, statistics as st, collections
v = json.load(open(sys.argv[1])); ms = 1000.0 / v['freq']; sp = v['spans']
by = collections.defaultdict(list)
for s in sp: by[s[3]].append(s)
for k in by: by[k].sort()
K = {3: 'PRESENT', 6: 'SCENE_WAIT', 7: 'FRAME_SUBMIT', 8: 'VIEW_FLUSH', 9: 'VIEW_JOB', 10: 'GT_FRAME', 11: 'FRAME_TICK', 14: 'WORLD_TICK', 15: 'GT_COUNTER_WAIT'}
print({K.get(k, k): len(v2) for k, v2 in by.items() if k < 20})
fr = by[10]
rows = collections.defaultdict(list)
for a, b in zip(fr, fr[1:]):
    s0, e0 = a[0], a[1]; nxt = b[0]
    rows['gt_frame'].append((e0 - s0) * ms); rows['gt_period'].append((nxt - s0) * ms); rows['between'].append((nxt - e0) * ms)
    for k in (14, 15, 6, 7, 11):
        inside = [s for s in by.get(k, []) if s[0] >= s0 and s[0] < nxt]
        rows[K[k] + '_sum'].append(sum((s[1] - s[0]) for s in inside) * ms)
        if inside: rows[K[k] + '_start'].append((inside[0][0] - s0) * ms)
        rows[K[k] + '_n'].append(len(inside))
for k, vals in rows.items():
    if vals: print(f'  {k:22s} median {st.median(vals):7.2f}  p90 {sorted(vals)[int(0.9*len(vals))]:7.2f}')
