"""Select hand-sized prop meshes from the geometry catalog (filtering stored bounds only; no geometry generated).

    py -3.13 -B tools/grip_generator/batch_candidates.py OUT.jsonl
"""
import collections, json, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT/'build/grip_meshes_20261116/geometry_catalog_v2.jsonl'
SEP = chr(92)
# Folders holding things a hand can pick up; everything else (characters, vehicles, architecture, fx, weapons'
# own parts, UI) is not a free physical prop.
KEEP = ('base' + SEP + 'environment' + SEP + 'decoration', 'ep1' + SEP + 'environment' + SEP + 'decoration',
        'base' + SEP + 'items', 'ep1' + SEP + 'items', 'base' + SEP + 'gameplay' + SEP + 'items',
        'base' + SEP + 'environment' + SEP + 'props', 'ep1' + SEP + 'environment' + SEP + 'props')
DROP = ('_lod', 'proxy', 'shadow', 'decal', 'occluder', 'collision', '_destr', 'debris', 'broken')
rows = {}
for line in CATALOG.open(encoding='utf-8'):
    r = json.loads(line)
    if r.get('status') == 'ok': rows[r['hash']] = r
picked = []; folders = collections.Counter()
for h, r in rows.items():
    p = r['path'].lower()
    if not p.startswith(KEEP) or any(d in p for d in DROP): continue
    ext = [b - a for a, b in zip(r['minimum'], r['maximum'])]
    if not (0.02 <= max(ext) <= 0.6 and r.get('triangles', 0) >= 4): continue
    picked.append(r); folders[SEP.join(p.split(SEP)[:5])] += 1
out = Path(sys.argv[1]); out.parent.mkdir(parents=True, exist_ok=True)
with out.open('w', encoding='utf-8') as f:
    for r in sorted(picked, key=lambda r: r['path']): f.write(json.dumps(r) + '\n')
print('ok meshes', len(rows), 'picked', len(picked), 'round', sum(1 for r in picked if r.get('round_profile')))
for k, v in folders.most_common(45): print(v, k)
