"""Add the extracted file path to each candidate (string join only).

    py -3.13 -B tools/grip_generator/batch_sources.py CANDIDATES.jsonl OUT.jsonl [substring ...]
Only candidates whose depot path contains one of the substrings are written when any are given.
"""
import json, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
sources = {}
for line in (ROOT/'build/grip_meshes_20261116/raw_manifest.jsonl').open(encoding='utf-8'):
    r = json.loads(line); sources[(r['hash'], r['archive'])] = r['source']
picks = [s.lower() for s in sys.argv[3:]]
n = missing = 0
with open(sys.argv[2], 'w', encoding='utf-8') as out:
    for line in open(sys.argv[1], encoding='utf-8'):
        r = json.loads(line)
        if picks and not any(p in r['path'].lower() for p in picks): continue
        src = sources.get((r['hash'], r['archive']))
        if not src: missing += 1; continue
        r['source'] = src; out.write(json.dumps(r) + '\n'); n += 1
print('written', n, 'missing source', missing)
