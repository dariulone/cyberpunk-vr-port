"""The built port onto both PCs of the co-op rig: this PC's game folder and the second PC's (HOME-PC, over scp).

    py -3.13 -B tools/mp/deploy_both.py [--only here|home]

Copies cyberpunkvrport.dll / .pdb into <game>\\cyberpunkvrport and the loader (powrprof.dll) into <game>\\bin\\x64 when
its bytes differ. Refuses while a game runs on the side it deploys to (the DLL is locked, and a run must not change
under it). Prints each side's DLL hash afterwards: both must match.
"""
import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/remote')]
import home_pc  # noqa: E402

BUILD = ROOT / 'build/patchday/sdk25-build/bin/red4ext/plugins/cyberpunkvrport/Release'
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
HOME_GAME = 'C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077'
FILES = [('cyberpunkvrport.dll', 'cyberpunkvrport'), ('cyberpunkvrport.pdb', 'cyberpunkvrport'),
         ('powrprof.dll', 'bin/x64')]


def md5(path):
    return hashlib.md5(Path(path).read_bytes()).hexdigest().upper()


def game_running_here():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq Cyberpunk2077.exe', '/NH'], capture_output=True, text=True)
    return 'Cyberpunk2077.exe' in out.stdout


def deploy_here():
    if game_running_here():
        sys.exit('here: a game runs, close it first')
    for name, folder in FILES:
        src, dst = BUILD / name, GAME / folder / name
        if not src.exists():
            continue
        if dst.exists() and md5(src) == md5(dst):
            continue
        dst.write_bytes(src.read_bytes())
        print(f'here: {folder}/{name}')
    print('here:', md5(GAME / 'cyberpunkvrport/cyberpunkvrport.dll')[:12])


def deploy_home():
    running = home_pc.run("[bool](Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue)").stdout.strip()
    if running == 'True':
        sys.exit('home: a game runs, close it first')
    hashes = home_pc.run(
        "foreach ($f in " + ','.join(f"'{HOME_GAME}/{folder}/{name}'" for name, folder in FILES) + ") { "
        "if (Test-Path $f) { (Get-FileHash $f -Algorithm MD5).Hash } else { '-' } }").stdout.split()
    for (name, folder), remote in zip(FILES, hashes):
        src = BUILD / name
        if not src.exists() or md5(src) == remote:
            continue
        if home_pc.scp(str(src), f'{home_pc.HOST}:{HOME_GAME}/{folder}/{name}') != 0:
            sys.exit(f'home: {name} failed')
        print(f'home: {folder}/{name}')
    print('home:', home_pc.run(f"(Get-FileHash '{HOME_GAME}/cyberpunkvrport/cyberpunkvrport.dll' -Algorithm MD5).Hash")
          .stdout.strip()[:12])


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument('--only', choices=['here', 'home'])
    args = a.parse_args()
    print('build:', md5(BUILD / 'cyberpunkvrport.dll')[:12])
    if args.only != 'home':
        deploy_here()
    if args.only != 'here':
        deploy_home()


if __name__ == '__main__':
    main()
