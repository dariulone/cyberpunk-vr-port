"""wkbreak.py CAPTURE: redDispatcher samples: idle vs busy, busy grouped inclusive / leaf / port-module share."""
import json, sys, collections
v = json.load(open(sys.argv[1], encoding='utf-8'))
wk = v['workers']
IDLE_LEAF = ('ntdll.dll!NtWaitForSingleObject', 'ntdll.dll!ZwWaitForAlertByThreadId', 'ntdll.dll!NtWaitForMultipleObjects',
             'ntdll.dll!ZwWaitForSingleObject', 'ntdll.dll!NtDelayExecution')
def stack_of(s):
    for x in s:
        if isinstance(x, list): return x
    return []
n = len(wk); busy = []; idle = 0; spin = 0
for s in wk:
    st = stack_of(s)
    if not st: continue
    if st[0] in IDLE_LEAF: idle += 1; continue
    if 'exe+143060' in st[:3] or 'exe+142190' in st[:2]: spin += 1
    busy.append(st)
print('worker samples', n, 'idle-wait %.1f%%' % (100 * idle / n), 'busy %.1f%%' % (100 * len(busy) / n), 'of busy, in idle loop top %.1f%%' % (100 * spin / max(1, len(busy))))
inc = collections.Counter(); leaf = collections.Counter(); port = 0
for st in busy:
    for f in set(st): inc[f] += 1
    leaf[st[0]] += 1
    if any('cyberpunkvrport.dll' in f for f in st): port += 1
b = len(busy)
print('busy samples with port code on the stack: %.1f%%' % (100 * port / b))
print('-- inclusive top 40 (of busy)')
for f, c in inc.most_common(40):
    print('  %5.1f%%  %s' % (100 * c / b, f[:150]))
print('-- leaf top 20')
for f, c in leaf.most_common(20):
    print('  %5.1f%%  %s' % (100 * c / b, f[:150]))
