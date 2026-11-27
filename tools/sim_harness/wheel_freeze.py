"""wheel_freeze.py OUT [N] [--idle S]: the weapon wheel opened N times (Y held 1.2 s, let go) in the running game while
the game's main thread is sampled every 5 ms (tools/profile/sampler.exe). With --idle S the first opening waits S
seconds before (the wheel lets its entities go after 20 s unused: that opening spawns them again).

A frame's work is a run of samples whose innermost frame is not a wait in the system (ntdll / KERNELBASE / win32u);
the report per opening is the longest runs (ms) -- a freeze is one far longer than the frame budget -- and the port
functions inside the longest one (resolved through the port's PDB, tools/crash/read_dump.py)."""
import collections
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(Path(__file__).resolve().parent), str(ROOT / 'tools/crash')]
import devgame as D  # noqa: E402
import read_dump as R  # noqa: E402
from simxr import Sim  # noqa: E402

SAMPLER = ROOT / 'tools/profile/sampler.exe'
PORT = D.GAME / 'cyberpunkvrport'
BASE = 0x180000000
WAITS = ('ntdll.dll!', 'KERNELBASE.dll!', 'win32u.dll!')


def read(f):
    freq, rows = 1.0, []
    for line in Path(f).read_text(encoding='utf-8').splitlines():
        if line.startswith('# qpc_freq'):
            freq = float(line.split()[2])  # '# qpc_freq <ticks per second> thread <tid>'
            continue
        parts = line.split()
        if len(parts) < 2 or not parts[0].isdigit():
            continue
        rows.append((int(parts[0]), parts[1:]))
    rows.sort()
    return [(t / freq, fr) for t, fr in rows]


def busy_runs(rows):
    runs, start, n, prev = [], None, 0, None
    for t, fr in rows:
        busy = not fr[0].startswith(WAITS)
        if busy:
            if start is None:
                start, n, members = t, 0, []
            n += 1
            members.append(fr)
            prev = t
        elif start is not None:
            runs.append((prev - start + 0.005, members))
            start = None
    if start is not None:
        runs.append((prev - start + 0.005, members))
    return sorted(runs, key=lambda r: r[0], reverse=True)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    args = [a for a in sys.argv[2:] if not a.startswith('--')]
    n = int(args[0]) if args else 3
    idle = float(sys.argv[sys.argv.index('--idle') + 1]) if '--idle' in sys.argv else 0.0
    pid = D.game_pid()
    sim = Sim(pid, focus_away=True)
    sym = R.Symbols(str(PORT))
    sym.load('cyberpunkvrport.dll', BASE, 0x4000000)
    if idle:
        print(f'idle {idle:.0f} s first', flush=True)
        time.sleep(idle)
    for k in range(n):
        f = out / f'samples_{k}.txt'
        proc = subprocess.Popen([str(SAMPLER), str(pid), '3000', '5000', str(f)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(0.6)
        sim.button('left', 'y', True)
        time.sleep(1.2)
        sim.button('left', 'y', False)
        proc.wait(timeout=30)
        runs = busy_runs(read(f))
        print(f'open {k}: longest main-thread work without a wait: ' + ', '.join(f'{r[0] * 1000:.0f} ms' for r in runs[:5]), flush=True)
        if runs:
            where = collections.Counter()
            for fr in runs[0][1]:
                port = [x for x in fr if x.startswith('cyberpunkvrport.dll!')]
                names = [(sym.resolve(BASE + int(x.split('!')[1], 16)) or x).split('   ')[0] for x in port[:5]]
                where[' <- '.join(names[:4]) or fr[0]] += 1
            for key, c in where.most_common(4):
                print(f'      {c * 5:4d} ms  {key[:260]}', flush=True)
        time.sleep(1.5)


if __name__ == '__main__':
    main()
