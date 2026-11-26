"""INCLUSIVE CPU of the port from a wpr CPU trace with stacks (vr_bench's cpu.etl, kept with CVR_KEEP_ETL=1): which
port function, with everything it calls (the game, the driver, the kernel), costs how much, on which threads.

    py -3.13 -B tools/vr_bench/etw_stacks.py build/vr-bench/<run>/cpu.etl [--fps 54.3] [--pid N] [--top 30]

Each SampleProf (1 ms of one core) gets the StackWalk events that follow it for its thread (a kernel and a user
part); its frames are named by module, the port's by function (its PDB beside the installed DLL, dbghelp), the game's
by function (build/patchday/ida/2.5/functions.tsv, exported from the IDA database of the same build). Reported per
frame (--fps, else per second):
  * inclusive time of each port function (a sample counts once per function on its stack), overall and on the game
    thread / the render workers;
  * the port's own entry points: the outermost port frame of each stack (where the game called into the port) --
    the whole cost of a hook, its calls back into the game included;
  * for the top entry points, where the time inside them goes (the leaf module / function);
  * game functions inclusive on the game thread outside any port frame (what the game itself spends).
"""
import argparse
import bisect
import json
import re
import subprocess
from collections import Counter, defaultdict
from pathlib import Path

import etw_cpu

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'cyberpunkvrport/cyberpunkvrport.dll'
EXE_FUNCTIONS = ROOT / 'build/patchday/ida/2.5/functions.tsv'
USER = etw_cpu.USER


def parse(csv, pid=None, process='Cyberpunk2077.exe'):
    images = defaultdict(dict)
    thread_pid, thread_name, names = {}, {}, {}
    samples = []            # [tid, [frames...]]
    open_by_tid = {}        # tid -> index of its newest sample (takes the stack walks that follow)
    span = [0, 0]           # first and last sample clock (100 ns)
    with open(csv, encoding='utf-8', errors='replace') as f:
        for line in f:
            head = line[:40].lstrip()
            if head.startswith('PerfInfo') and 'SampleProf' in line[:40]:
                p = line.split(',')
                if len(p) > USER + 2:
                    tid = int(p[USER + 1])
                    t = int(p[16])
                    span[0] = span[0] or t
                    span[1] = t
                    if pid is None:
                        pid = next((k for k, v in names.items() if v.lower() == process.lower()), None)
                    if thread_pid.get(tid) != pid:
                        open_by_tid[tid] = None      # another process: its stacks are not kept
                        continue
                    open_by_tid[tid] = len(samples)
                    samples.append([tid, [int(p[USER], 16)], False])
            elif head.startswith('StackWalk'):
                p = line.split(',')
                if len(p) > USER + 3:
                    tid = int(p[USER + 2])
                    i = open_by_tid.get(tid)
                    if i is not None:
                        s = samples[i]
                        frames = [int(x, 16) for x in p[USER + 3:] if x.strip()]
                        # the kernel part first, then the user part: the user part continues the stack
                        if not s[2]:
                            s[1] = frames
                            s[2] = True
                        else:
                            s[1].extend(frames)
            elif head.startswith('Image,'):
                p = line.split(',')
                if p[1].strip() in ('DCStart', 'Load') and len(p) > USER + 2:
                    q = etw_cpu._quoted(line)
                    images[etw_cpu._int(p[USER + 2])][etw_cpu._int(p[USER])] = (etw_cpu._int(p[USER + 1]), q[-1] if q else '?')
            elif head.startswith('Thread,'):
                p = line.split(',')
                kind = p[1].strip()
                if kind in ('DCStart', 'Start') and len(p) > USER + 1:
                    tid = etw_cpu._int(p[USER + 1])
                    thread_pid[tid] = etw_cpu._int(p[USER])
                    n = etw_cpu._name_from_hex(p[-1])
                    if n:
                        thread_name[tid] = n
                elif kind == 'SetName' and len(p) > USER + 1:
                    q = etw_cpu._quoted(line)
                    if q:
                        thread_name[etw_cpu._int(p[USER + 1])] = q[-1]
            elif head.startswith('Process,'):
                p = line.split(',')
                if p[1].strip() in ('DCStart', 'Start') and len(p) > USER + 1:
                    q = etw_cpu._quoted(line)
                    exe = next((x for x in q if x.lower().endswith('.exe')), '')
                    names.setdefault(etw_cpu._int(p[USER + 1]), exe)
    return pid, images, thread_pid, thread_name, samples, span


def group(name):
    n = re.sub(r'\d+$', '#', name or '?')
    if n == 'GameThread':
        return 'game'
    if n.startswith('redDispatcher') or n.startswith('Job'):
        return 'workers'
    if n.upper().startswith('CVR') or 'cvr' in n:
        return 'port threads'
    return 'other'


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('etl')
    ap.add_argument('--fps', type=float, default=0.0)
    ap.add_argument('--pid', type=int)
    ap.add_argument('--top', type=int, default=30)
    ap.add_argument('--dll', default=str(DLL))
    ap.add_argument('--json', help='write the tables here too')
    a = ap.parse_args()
    etl = Path(a.etl)
    csv = etl.with_suffix('.csv')
    if not csv.exists():
        r = subprocess.run(['tracerpt.exe', str(etl), '-o', str(csv), '-of', 'CSV', '-y'], capture_output=True, text=True)
        if r.returncode:
            raise SystemExit(f'tracerpt failed: {r.stdout[-300:]}')
    import pickle
    cache_file = etl.with_suffix('.stacks.pickle')   # the parsed samples: a second analysis skips the CSV
    if cache_file.exists() and cache_file.stat().st_mtime >= csv.stat().st_mtime:
        pid, images, thread_pid, thread_name, samples, span = pickle.loads(cache_file.read_bytes())
    else:
        pid, images, thread_pid, thread_name, samples, span = parse(csv, a.pid)
        cache_file.write_bytes(pickle.dumps((pid, dict(images), thread_pid, thread_name, samples, span)))
    images = defaultdict(dict, images)
    mods = sorted([(b, s, Path(path.replace('\\', '/')).name) for b, (s, path) in images[pid].items()] +
                  [(b, s, Path(path.replace('\\', '/')).name) for b, (s, path) in images[0].items()])
    bases = [m[0] for m in mods]
    port = next((m for m in mods if m[2].lower() == 'cyberpunkvrport.dll'), None)
    exe = next((m for m in mods if m[2].lower() == 'cyberpunk2077.exe'), None)
    sym = etw_cpu.Symbols(a.dll, port[0], port[1]) if port else None
    exef = etw_cpu.ExeFunctions(EXE_FUNCTIONS) if EXE_FUNCTIONS.exists() and exe else None

    def name(ip):
        i = bisect.bisect_right(bases, ip) - 1
        if i < 0 or ip >= mods[i][0] + mods[i][1]:
            return ('?', '?')
        m = mods[i]
        if port and m is port:
            return ('port', sym.name(ip))
        if exe and m is exe and exef:
            _, fn = exef.find(ip - m[0])
            return ('exe', fn)
        return (m[2], m[2])
    cache = {}

    def named(ip):
        v = cache.get(ip)
        if v is None:
            v = cache[ip] = name(ip)
        return v

    # modules that only serve their caller: a sample there is charged to the first frame above them
    system = {'ntdll.dll', 'ntoskrnl.exe', 'kernelbase.dll', 'kernel32.dll', 'nvwgf2umx.dll', 'nvlddmkm.sys',
              'd3d12core.dll', 'd3d12.dll', 'dxgi.dll', 'win32kfull.sys', 'win32kbase.sys', 'win32u.dll',
              'user32.dll', 'ucrtbase.dll', 'vcruntime140.dll', 'msvcp140.dll', 'dxgkrnl.sys', 'hal.dll',
              'msvcp_win.dll', 'gdi32full.dll', 'nvapi64.dll', 'combase.dll', 'rpcrt4.dll', 'advapi32.dll'}
    ours = [(tid, frames) for tid, frames, _ in samples if thread_pid.get(tid) == pid]
    first = last = None
    inclusive, inclusive_by_group = Counter(), defaultdict(Counter)
    entry, entry_by_group, entry_leaf = Counter(), defaultdict(Counter), defaultdict(Counter)
    game_incl_outside = Counter()
    total_by_group = Counter()
    in_port_by_group = Counter()
    own_by_group, own_sys = defaultdict(Counter), defaultdict(Counter)
    for tid, frames in ours:
        g = group(thread_name.get(tid))
        total_by_group[g] += 1
        nf = [named(ip) for ip in frames]
        # OWN: the first frame from the leaf that is not a serving system module; a port function there is charged
        # with this sample (its own code, and the driver / OS calls it makes)
        for kind, fn in nf:
            if kind in ('port', 'exe') or kind.lower() not in system:
                if kind == 'port':
                    own_by_group[g][fn] += 1
                    leaf = nf[0]
                    own_sys[fn][leaf[0] if leaf[0] not in ('port', 'exe') else 'own code'] += 1
                break
        port_frames = [fn for kind, fn in nf if kind == 'port']
        for fn in set(port_frames):
            inclusive[fn] += 1
            inclusive_by_group[g][fn] += 1
        if port_frames:
            in_port_by_group[g] += 1
            outer = port_frames[-1]          # the frame list runs from the leaf to the thread's root
            entry[outer] += 1
            entry_by_group[g][outer] += 1
            leaf_kind, leaf_fn = nf[0]
            entry_leaf[outer][leaf_fn if leaf_kind in ('port', 'exe') else leaf_kind] += 1
        elif g == 'game':
            for fn in set(fn for kind, fn in nf if kind == 'exe'):
                game_incl_outside[fn] += 1
    if sym:
        sym.close()
    seconds = (span[1] - span[0]) / 1e7 if span[1] > span[0] else 0.0
    frames_total = seconds * a.fps if a.fps else 0.0
    # one sample = 1 ms of one core: per frame with --fps, else per second of the trace
    scale = (1.0 / frames_total) if frames_total else (1.0 / seconds if seconds else 1.0)
    unit = 'ms/frame' if frames_total else 'ms/s'
    fmt = lambda n: f'{n * scale:8.3f}'
    out = dict(seconds=seconds, fps=a.fps, unit=unit)
    print(f'process {pid}: {len(ours)} samples over {seconds:.1f} s ({unit}); by thread group: ' +
          ', '.join(f'{g} {fmt(n).strip()} (in port code {fmt(in_port_by_group[g]).strip()})'
                    for g, n in total_by_group.most_common()))
    out['groups'] = {g: dict(samples=n, inPort=in_port_by_group[g]) for g, n in total_by_group.items()}
    out['inclusive'] = inclusive.most_common(a.top)
    out['inclusiveByGroup'] = {g: c.most_common(a.top) for g, c in inclusive_by_group.items()}
    out['entries'] = entry.most_common(a.top)
    out['entriesByGroup'] = {g: c.most_common(a.top) for g, c in entry_by_group.items()}
    out['entryLeaves'] = {fn: entry_leaf[fn].most_common(8) for fn, _ in entry.most_common(12)}
    out['gameOutsidePort'] = game_incl_outside.most_common(a.top)
    own_all = Counter()
    for c in own_by_group.values():
        own_all.update(c)
    out['own'] = own_all.most_common(a.top)
    out['ownByGroup'] = {g: c.most_common(a.top) for g, c in own_by_group.items()}
    print(f'\nOWN port cost ({unit}; its code + the driver/OS calls it makes, not the game code it wraps): ' +
          ', '.join(f'{g} {fmt(sum(c.values())).strip()}' for g, c in own_by_group.items()))
    for fn, n in out['own']:
        split = ', '.join(f'{k} {v}' for k, v in own_sys[fn].most_common(3))
        by = ', '.join(f'{g} {fmt(own_by_group[g][fn]).strip()}' for g in own_by_group if own_by_group[g][fn])
        print(f'  {fmt(n)}  {fn}   [{by}; leaf: {split}]')
    print(f'\nINCLUSIVE port functions ({unit}, all threads):')
    for fn, n in out['inclusive']:
        print(f'  {fmt(n)}  {fn}')
    for g in ('game', 'workers', 'port threads', 'other'):
        if g in entry_by_group:
            print(f'\nPORT ENTRY POINTS on {g} ({unit}; the outermost port frame, everything below it):')
            for fn, n in entry_by_group[g].most_common(a.top):
                print(f'  {fmt(n)}  {fn}')
    print('\nWHERE THE TOP ENTRY POINTS SPEND (leaf, samples):')
    for fn, leaves in out['entryLeaves'].items():
        print(f'  {fn}: ' + ', '.join(f'{l} {n}' for l, n in leaves))
    print(f'\nGAME THREAD, game functions inclusive outside port code ({unit}):')
    for fn, n in out['gameOutsidePort'][:20]:
        print(f'  {fmt(n)}  {fn}')
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1), encoding='utf-8')


if __name__ == '__main__':
    main()
