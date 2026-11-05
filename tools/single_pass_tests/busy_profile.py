"""Busy-only view of a thread_stack_sampler result: drop idle waits, attribute by module/function.

A sample is idle when its leaf is a kernel wait (the dispatcher's semaphore sleep). For busy
samples, 'self' attributes the leaf; 'plugin inclusive' counts samples with any CyberpunkVR_Stereo
frame, and 'plugin self' those whose leaf frames up to the first non-plugin caller are the plugin's
own code (hook overhead, not the native work it forwards).
"""
import collections
import json
import sys

IDLE = ('ntdll.dll!NtWaitForSingleObject', 'ntdll.dll!ZwWaitForAlertByThreadId', 'ntdll.dll!NtWaitForMultipleObjects',
        'ntdll.dll!ZwRemoveIoCompletion', 'ntdll.dll!NtDelayExecution')
PLUGIN = 'cyberpunkvrport.dll'


def module(frame):
    return frame.split('!', 1)[0].split('+', 1)[0]


def analyze(value, top=25):
    stacks = [row.split(' <- ') for row in value['stacks']]
    busy = [s for s in stacks if s and s[0] not in IDLE]
    total = len(busy)
    if not total:
        return dict(samples=len(stacks), busy=0)
    leaf_modules = collections.Counter(module(s[0]) for s in busy)
    plugin_any = sum(1 for s in busy if any(f.startswith(PLUGIN) for f in s))
    # Leaf time spent in the plugin itself or in runtime helpers called directly by it (locks, CRT).
    plugin_self = 0
    plugin_self_funcs = collections.Counter()
    for s in busy:
        for frame in s:
            if frame.startswith(PLUGIN):
                plugin_self += 1
                plugin_self_funcs[frame] += 1
                break
            if module(frame) not in ('ntdll.dll', 'KERNELBASE.dll', 'MSVCP140.dll', 'VCRUNTIME140.dll', 'ucrtbase.dll'):
                break
    inclusive = collections.Counter()
    for s in busy:
        for frame in set(s):
            inclusive[frame] += 1
    pct = lambda n: round(100 * n / total, 2)
    return dict(samples=len(stacks), busy=total, busyFraction=round(total / len(stacks), 4),
                leafModules=[(m, pct(n)) for m, n in leaf_modules.most_common(12)],
                pluginInclusive=pct(plugin_any), pluginSelf=pct(plugin_self),
                pluginSelfFunctions=[(f, pct(n)) for f, n in plugin_self_funcs.most_common(top)],
                inclusive=[(f, pct(n)) for f, n in inclusive.most_common(top * 2)])


if __name__ == '__main__':
    value = analyze(json.load(open(sys.argv[1], encoding='utf-8')))
    print(json.dumps(value, indent=1))
