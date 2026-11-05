"""maplines.py: the last '[lua] profile:' line of the port log, each 'file:line' entry shown with the code at that
line in every installed mod that has such a file (init.lua is ambiguous between mods)."""
import os, re, sys
LOG = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\red4ext\plugins\cyberpunkvrport\log\cyberpunkvrport.log'
ROOT = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\red4ext\plugins\cyberpunkvrport\lua'
line = [l for l in open(LOG, encoding='utf-8', errors='replace') if '[lua] profile:' in l][-1]
entries = re.findall(r'([\w\./\[\]]+):(\d+)( \(outside onUpdate\))? ([\d.]+)%', line)
cache = {}
def lines_of(p):
    if p not in cache:
        try: cache[p] = open(p, encoding='utf-8', errors='replace').read().split('\n')
        except OSError: cache[p] = None
    return cache[p]
for f, n, out, pct in entries[:40]:
    n = int(n); hits = []
    if chr(92) in f:
        cands = [os.path.join(ROOT, f)]
    else:
        cands = [os.path.join(ROOT, m, f) for m in os.listdir(ROOT)]
    for p in cands:
        L = lines_of(p)
        if L and n <= len(L):
            hits.append((os.path.relpath(p, ROOT), L[n - 1].strip()[:110]))
    print(f'{pct:>5}% {f}:{n}{out}')
    for h in hits[:6]: print('        ', h[0], '|', h[1])
