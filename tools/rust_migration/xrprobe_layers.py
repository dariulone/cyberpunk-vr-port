"""What a run submitted, from an xr_probe recording (tools/xr_probe): every distinct layer signature of the sampled
xrEndFrame calls (type, pose, FOV, rect, quad size, eye visibility, depth) with how often it was seen, for an A/B
between two builds on the same scenario.

    python xrprobe_layers.py A.jsonl [B.jsonl]

With two files it prints the signatures each run has that the other lacks; a static simulated head should leave the
sets equal (the counts depend on timing)."""
import json
import sys
from collections import Counter


def r(v, nd=6):
    return round(float(v), nd)


def layer_sig(layer):
    t = layer.get('type')
    if t == 'projection':
        views = []
        for v in layer.get('views', []):
            p, f = v['pose'], v['fov']
            views.append((v['eye'], tuple(r(x) for x in p['pos']), tuple(r(x) for x in p['quat']),
                          (r(f['radL']), r(f['radR']), r(f['radU']), r(f['radD'])),
                          (v['rect']['x'], v['rect']['y'], v['rect']['w'], v['rect']['h']), v.get('imageArrayIndex')))
        return ('projection', layer.get('layerFlags'), tuple(views))
    if t == 'quad':
        p = layer['pose']
        return ('quad', layer.get('eyeVisibility'), tuple(r(x, 4) for x in p['pos']), tuple(r(x, 4) for x in p['quat']),
                tuple(r(x, 4) for x in layer.get('sizeM', [])),
                (layer['rect']['x'], layer['rect']['y'], layer['rect']['w'], layer['rect']['h']))
    return (t,)


def frames(path):
    out = Counter()
    kinds = Counter()
    for line in open(path, encoding='utf-8', errors='replace'):
        if '"ev":"endFrame"' not in line:
            continue
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        layers = ev.get('layers', [])
        kinds[(ev.get('layerCount'), bool(ev.get('depth')), tuple(l.get('type') for l in layers))] += 1
        for layer in layers:
            out[layer_sig(layer)] += 1
    return out, kinds


def show(title, sigs, kinds):
    print(f'== {title}: {sum(kinds.values())} sampled frames')
    for k, n in kinds.most_common():
        print(f'  {n:6d}  layers={k[0]} depth={k[1]} {k[2]}')
    for s, n in sigs.most_common(12):
        print(f'  {n:6d}  {s}')


a_sigs, a_kinds = frames(sys.argv[1])
show(sys.argv[1], a_sigs, a_kinds)
if len(sys.argv) > 2:
    b_sigs, b_kinds = frames(sys.argv[2])
    show(sys.argv[2], b_sigs, b_kinds)
    only_a = [s for s in a_sigs if s not in b_sigs]
    only_b = [s for s in b_sigs if s not in a_sigs]
    print(f'== signatures only in A: {len(only_a)}, only in B: {len(only_b)}')
    for s in only_a[:10]:
        print('  A', a_sigs[s], s)
    for s in only_b[:10]:
        print('  B', b_sigs[s], s)
