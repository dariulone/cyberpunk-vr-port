"""scope_passes.py spans.json [side=2]: what a view's scene passes are (side 2 = the camera scope). Needs a level-2 node
timeline (node_timeline.py --level 2): top-level spans carry 0x100|rtId for SceneDrv, nested spans (kind 16)
carry the side in bits 12-13. Per rtId of the view: CPU ms/frame of its SceneDrv spans, and the named nested
nodes inside them (by time containment on the same thread), with their own ms/frame."""
import bisect
import collections
import json
import re
import sys

d = json.load(open(sys.argv[1]))
kw = dict(a.split('=', 1) for a in sys.argv[2:] if '=' in a)
side = int(kw.get('side', '2'))
f = d['freq']; ms = 1000.0 / f
sp = d['spans']
names = {}
for m in re.finditer(r'\{\s*0x([0-9A-Fa-f]+),\s*"([^"]+)"\s*\}',
                     open(r'C:\Users\dariulone\Desktop\CyberpunkVRPort\src\Stereo\NodeNames.inc', encoding='utf-8').read()):
    names[int(m.group(1), 16)] = m.group(2)
nm = lambda r: names.get(r, '0x%06X' % r)
pres = sorted(s[0] for s in sp if s[3] == 3)
n = len(pres) - 1
t0, t1 = pres[0], pres[-1]
tops = [s for s in sp if s[3] == side and t0 <= s[0] < t1]
nested = [s for s in sp if s[3] == 16 and ((s[5] >> 12) & 3) == side and t0 <= s[0] < t1]
print('frames %d  top spans %d  nested spans %d' % (n, len(tops), len(nested)))
# top SceneDrv by rtId
pass_ms = collections.Counter(); pass_n = collections.Counter()
by_thread = collections.defaultdict(list)
for s in tops:
    if s[2] == 0x332D5C:
        rt = s[5] & 0xFF if s[5] & 0x100 else -1
        pass_ms[rt] += (s[1] - s[0]) * ms; pass_n[rt] += 1
        by_thread[s[4]].append(s)
for v in by_thread.values():
    v.sort(key=lambda s: s[0])
starts = {t: [s[0] for s in v] for t, v in by_thread.items()}
inner = collections.defaultdict(collections.Counter)
inner_n = collections.defaultdict(collections.Counter)
for s in nested:
    v = by_thread.get(s[4])
    if not v:
        continue
    i = bisect.bisect_right(starts[s[4]], s[0]) - 1
    if i < 0 or s[1] > v[i][1]:
        continue
    rt = v[i][5] & 0xFF
    depth = (s[5] >> 16) & 15
    inner[rt][(depth, s[2])] += (s[1] - s[0]) * ms
    inner_n[rt][(depth, s[2])] += 1
tot = sum(pass_ms.values())
print('SceneDrv total %.2f ms/frame over %.1f calls/frame' % (tot / n, sum(pass_n.values()) / n))
for rt, v in pass_ms.most_common(40):
    kids = ', '.join('%s%s %.2f' % ('  ' * (dep - 1), nm(r), t / n) for (dep, r), t in inner[rt].most_common(5))
    print('  rt %3d  %6.3f ms/fr  x%4.1f   %s' % (rt, v / n, pass_n[rt] / n, kids))
# other top-level nodes of the view
other = collections.Counter()
for s in tops:
    if s[2] != 0x332D5C:
        other[s[2]] += (s[1] - s[0]) * ms
print('other top-level nodes:', ', '.join('%s %.3f' % (nm(r), t / n) for r, t in other.most_common(12)))
# nested nodes overall for the view
allk = collections.Counter()
for rt, c in inner.items():
    for (dep, r), t in c.items():
        allk[(dep, r)] += t
print('nested nodes (all passes), ms/frame:')
for (dep, r), t in allk.most_common(25):
    print('   d%d %-40s %.3f' % (dep, nm(r), t / n))
