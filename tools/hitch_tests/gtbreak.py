"""gtbreak.py CAPTURE: GameThread samples grouped by marker frames (inclusive) and by leaf."""
import json, sys, collections
v = json.load(open(sys.argv[1], encoding='utf-8'))
gt = v['gt']
n = len(gt)
inc = collections.Counter(); leaf = collections.Counter()
for t, stack, extra in gt:
    seen = set()
    for f in stack:
        if f not in seen:
            inc[f] += 1; seen.add(f)
    leaf[stack[0] if stack else '?'] += 1
print('GT samples', n)
print('-- inclusive top 45')
for f, c in inc.most_common(45):
    print('  %5.1f%%  %s' % (100 * c / n, f))
print('-- leaf top 20')
for f, c in leaf.most_common(20):
    print('  %5.1f%%  %s' % (100 * c / n, f))
