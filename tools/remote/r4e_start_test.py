"""Does RED4ext start in this game folder with one file set aside? (the second PC's error 1114 hunt)

    py -3.13 -B r4e_start_test.py [--aside bin\\x64\\version.dll] [--seconds 25]

Starts the game exe directly (no launcher), waits, reports whether a new RED4ext log appeared and whether the game is
still running, closes it and puts the file back.
"""
import argparse
import subprocess
import time
from pathlib import Path

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')


def main():
    a = argparse.ArgumentParser()
    a.add_argument('--aside', default='')
    a.add_argument('--seconds', type=float, default=25)
    args = a.parse_args()
    moved = None
    if args.aside:
        src = GAME / args.aside
        moved = src.with_name(src.name + '.aside-test')
        src.rename(moved)
    logs = GAME / 'red4ext/logs'
    before = {p.name for p in logs.glob('*.log')}
    t0 = time.time()
    p = subprocess.Popen([str(GAME / 'bin/x64/Cyberpunk2077.exe'), '-skipStartScreen'], cwd=GAME / 'bin/x64')
    try:
        time.sleep(args.seconds)
        new = [q for q in logs.glob('*.log') if q.name not in before or q.stat().st_mtime > t0]
        print('aside:', args.aside or '-', '| game alive:', p.poll() is None, '| red4ext log written:', bool(new))
        for q in new:
            print('  ', q.name, q.read_text(encoding='utf-8', errors='replace').splitlines()[-3:])
    finally:
        subprocess.run(['taskkill', '/PID', str(p.pid), '/T', '/F'], capture_output=True)
        time.sleep(3)
        if moved:
            moved.rename(GAME / args.aside)


if __name__ == '__main__':
    main()
