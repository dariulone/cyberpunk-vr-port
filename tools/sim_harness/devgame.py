"""devgame.py -- a test session that stays open: the game on the OpenXR simulator, driven by commands to the port.

    py -3.13 -B tools/sim_harness/devgame.py start            # launch at the H10 save if no game runs, else attach
    py -3.13 -B tools/sim_harness/devgame.py start --menu     # launch and stay in the main menu lobby
    py -3.13 -B tools/sim_harness/devgame.py cmd "vendor gunsmith" --wait "vendor walk: turn" --timeout 120
    py -3.13 -B tools/sim_harness/devgame.py shot C:/out/name   # the left eye's still
    py -3.13 -B tools/sim_harness/devgame.py stop             # close the game, put the runtime back, QuickBoot out

The port reads log/dev_cmd.txt while a file `cvr_dev_commands` sits beside the game's executable (start makes it):
ping, vendor <name>, trade, weapon <record>, teleport x y z, roof [x y], reach 1|0 -- see rust/crates/cvr-ffi/src/
mechanics/dev_cmd.rs. Each command answers with a "[dev]" line; `cmd` returns the log lines written after it was sent
(up to --wait's marker or --timeout). Nothing is relaunched between commands: only `stop` closes the game.
"""
import argparse
import ctypes
import json
import os
import shutil
import subprocess
import sys
import time
import winreg
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/sim_harness'), str(ROOT / 'tools/vr_bench')]
import vr_bench as V  # noqa: E402
from simxr import Sim  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
LOG = GAME / 'cyberpunkvrport/log/cyberpunkvrport.log'
CMD = GAME / 'cyberpunkvrport/log/dev_cmd.txt'
MARKER = GAME / 'bin/x64/cvr_dev_commands'
QB = GAME / 'cyberpunkvrport/lua/CyberpunkVRPort_QuickBoot'
SIM_JSON = str(Path.home() / 'Downloads/OpenXR-Simulator-v1.5.0-win64/openxr_simulator_abs.json')
VD_JSON = r'C:\Program Files\Virtual Desktop Streamer\OpenXR\virtualdesktop-openxr.json'
KEY = r'SOFTWARE\Khronos\OpenXR\1'
LIVE = ROOT / 'tools/single_pass_tests/geometry_cost_live.py'
STATE = Path(os.environ.get('LOCALAPPDATA', '.')) / 'cvr_devgame.json'
HOLD = Path(os.environ.get('TEMP', '.')) / 'cvr_devgame_menu_hold'


def game_pid():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq Cyberpunk2077.exe', '/FO', 'CSV', '/NH'], capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split(',')]
        if len(parts) > 1 and parts[0].lower() == 'cyberpunk2077.exe':
            return int(parts[1])
    return None


def set_runtime(path):
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, KEY, 0, winreg.KEY_READ | winreg.KEY_WRITE) as k:
        winreg.SetValueEx(k, 'ActiveRuntime', 0, winreg.REG_SZ, path)


def log_size():
    try:
        return LOG.stat().st_size
    except OSError:
        return 0


def log_since(offset):
    try:
        with open(LOG, 'rb') as f:
            f.seek(offset)
            return f.read().decode('utf-8', errors='replace')
    except OSError:
        return ''


def start(extra_env, menu=False):
    MARKER.write_text('dev commands\n', encoding='utf-8')
    pid = game_pid()
    if pid:
        print('attached, pid', pid, flush=True)
        return pid
    set_runtime(SIM_JSON)
    state = Path(os.environ.get('TEMP', '.')) / 'cvr_devgame_live.json'
    env = os.environ.copy()
    env.update(extra_env)
    if menu:
        # the main menu's lobby: QuickBoot held there by the hold file (it stays while the file does); the launcher
        # runs on in the background
        HOLD.write_text('hold\n', encoding='utf-8')
        off = log_size()
        subprocess.Popen([sys.executable, '-B', str(ROOT / 'tools/quickboot_tests/launch_to_save.py'), '--timeout', '600',
                          '--pause-at-continue', '--hold-file', str(HOLD)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
        t0 = time.time()
        while time.time() - t0 < 300 and '[VRLobby] from Rust: the scene up' not in log_since(off):
            time.sleep(2)
        pid = game_pid()
        STATE.write_text(json.dumps({'pid': pid}), encoding='utf-8')
        print('in the lobby' if pid else 'no game', 'pid', pid, flush=True)
        return pid
    subprocess.run([sys.executable, '-B', str(LIVE), 'launch', '--state', str(state), '--timeout', '300', '--save', 'ManualSave-491'],
                   capture_output=True, text=True, errors='replace', timeout=420, env=env)
    subprocess.run([sys.executable, '-B', str(LIVE), 'continue', '--state', str(state), '--timeout', '90'],
                   capture_output=True, text=True, errors='replace', timeout=200, env=env)
    pid = json.loads(state.read_text(encoding='utf-8')).get('pid')
    sim = Sim(pid, focus_away=True)
    sim.hand('right', pos=(0.30, -0.70, 0.20), ypr=(0, 0, 0))
    sim.hand('left', pos=(-0.30, -0.70, 0.20), ypr=(0, 0, 0))
    sim.head(yaw=0, pitch=0)
    STATE.write_text(json.dumps({'pid': pid}), encoding='utf-8')
    print('started, pid', pid, flush=True)
    return pid


def cmd(text, wait=None, timeout=20.0):
    """Send one command; the log lines written after it, up to the marker `wait` (default: its "[dev]" answer)."""
    off = log_size()
    tmp = CMD.with_suffix('.tmp')
    tmp.write_text(text + '\n', encoding='utf-8')
    for _ in range(50):
        try:
            os.replace(tmp, CMD)
            break
        except OSError:
            time.sleep(0.1)
    marker = wait or '[dev] '
    t0 = time.time()
    while time.time() - t0 < timeout:
        new = log_since(off)
        if marker in new:
            return new
        time.sleep(0.25)
    return log_since(off)


def shot(path, eyes='left'):
    pid = game_pid()
    sim = Sim(pid, focus_away=True)
    try:
        return sim.shot(str(path), eyes=eyes, timeout=30)
    except Exception as e:  # noqa: BLE001
        return f'shot: {e}'


def stop():
    pid = game_pid()
    if pid:
        user32 = ctypes.windll.user32
        cb = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
        hwnds = []

        def visit(h, _):
            p = ctypes.c_ulong()
            user32.GetWindowThreadProcessId(ctypes.c_void_p(h), ctypes.byref(p))
            if p.value == pid and user32.IsWindowVisible(ctypes.c_void_p(h)):
                hwnds.append(h)
            return True
        user32.EnumWindows(cb(visit), None)
        for h in hwnds:
            user32.PostMessageW(ctypes.c_void_p(h), 0x0010, 0, 0)
        for _ in range(40):
            if not game_pid():
                break
            time.sleep(1)
    time.sleep(4)
    HOLD.unlink(missing_ok=True)
    if QB.exists():
        dst = ROOT / 'build/quickboot-removed' / time.strftime('rust-%Y%m%d-%H%M%S')
        dst.mkdir(parents=True, exist_ok=True)
        shutil.move(str(QB), str(dst / QB.name))
    set_runtime(VD_JSON)
    print('stopped; runtime put back', flush=True)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest='what', required=True)
    s = sub.add_parser('start')
    s.add_argument('env', nargs='*', help='KEY=VALUE for a fresh launch')
    s.add_argument('--menu', action='store_true', help='stay in the main menu lobby (no save loaded)')
    c = sub.add_parser('cmd')
    c.add_argument('text')
    c.add_argument('--wait')
    c.add_argument('--timeout', type=float, default=20.0)
    h = sub.add_parser('shot')
    h.add_argument('path')
    sub.add_parser('stop')
    a = ap.parse_args()
    if a.what == 'start':
        start(dict(e.split('=', 1) for e in a.env), a.menu)
    elif a.what == 'cmd':
        print(cmd(a.text, a.wait, a.timeout))
    elif a.what == 'shot':
        print(shot(a.path))
    else:
        stop()


if __name__ == '__main__':
    main()
