"""lua_by_mod.py CAPTURE [top]: GameThread samples with a Lua chain, grouped by mod, as a share of all GameThread
samples and in ms per frame; plus each mod's top innermost Lua functions.

Chunks carry only a file name ('@init.lua:456' = file:firstline), so the mod comes from the installed sources: a
mod matches an entry when it has that file with a 'function' on that line (line 0 = a main chunk). The outermost
entry decides; ties go to the mod matching most of the chain."""
import collections
import json
import os
import re
import sys

v = json.load(open(sys.argv[1], encoding='utf-8'))
top = int(sys.argv[2]) if len(sys.argv) > 2 else 4
GAME = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
ROOTS = [os.path.join(GAME, r'red4ext\plugins\cyberpunkvrport\lua'),
         os.path.join(GAME, r'bin\x64\plugins\cyber_engine_tweaks\mods')]
FN = re.compile(r'\bfunction\b')
# file basename -> [(mod, set of function lines)]
files = collections.defaultdict(list)
for root in ROOTS:
    if not os.path.isdir(root):
        continue
    for mod in os.listdir(root):
        base = os.path.join(root, mod)
        if not os.path.isdir(base):
            continue
        for dp, _, fs in os.walk(base):
            for f in fs:
                if f.endswith('.lua'):
                    try:
                        lines = open(os.path.join(dp, f), encoding='utf-8', errors='replace').read().split('\n')
                    except OSError:
                        continue
                    files[f].append((mod, {i + 1 for i, s in enumerate(lines) if FN.search(s)} | {0}))


def cands(entry):
    name, _, line = entry.lstrip('@=').rpartition(':')
    try:
        line = int(line)
    except ValueError:
        return set()
    return {m for m, fl in files.get(os.path.basename(name), []) if line in fl}


def mod_of(lua):
    outer = cands(lua[-1])
    if len(outer) == 1:
        return next(iter(outer))
    if not outer:
        return '?' + lua[-1]
    score = collections.Counter()
    for e in lua:
        for m in cands(e) & outer:
            score[m] += 1
    best = score.most_common(2)
    return best[0][0] if len(best) < 2 or best[0][1] > best[1][1] else '|'.join(sorted(outer))


gt = v['gt']
frames = len(v.get('frames') or []) or 1
secs = gt[-1][0] - gt[0][0]
per = collections.Counter()
inner = collections.defaultdict(collections.Counter)
withlua = 0
for r in gt:
    lua = [x for x in ((r[2] if len(r) > 2 else None) or []) if x != 'C']
    if not lua:
        continue
    withlua += 1
    m = mod_of(lua)
    per[m] += 1
    inner[m][lua[0]] += 1
n = len(gt)
print('GameThread samples %d over %.1f s (%d frames), with a Lua chain %d (%.1f%%)' % (n, secs, frames, withlua,
                                                                                    100 * withlua / n))
for m, c in per.most_common():
    print('%6.2f%% %5.2f ms/frame  %-34s %s' % (100 * c / n, c / n * secs * 1000.0 / frames, m,
                                              ', '.join('%s %d' % kv for kv in inner[m].most_common(top))))
