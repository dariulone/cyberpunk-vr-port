"""SteamVR on its null driver: a headset that is only a desktop window, for runs of the port on SteamVR's OpenXR
runtime without the headset.

    py -3.13 -B tools/steamvr_null/steamvr_null.py on [--width 2648 --height 2864 --hz 90]
    py -3.13 -B tools/steamvr_null/steamvr_null.py off
    py -3.13 -B tools/steamvr_null/steamvr_null.py status
    py -3.13 -B tools/steamvr_null/steamvr_null.py stop      # close SteamVR

`on` and `off` edit Steam/config/steamvr.vrsettings only while SteamVR is closed (it rewrites that file while
it runs). `on` records the keys it changes, with their old values or their absence, in
build/steamvr-null/state.json and keeps a copy of the whole file; `off` puts exactly those keys back and leaves
everything SteamVR wrote in between alone. The null driver is selected with steamvr.forcedDriver = "null",
steamvr.activateMultipleDrivers = true (other drivers, the controllers' included, still load),
steamvr.requireHmd = false and a driver_null section with enable = true; SteamVR's own null driver
defaults (drivers/null/resources/settings/default.vrsettings) are not touched. The idle standby is switched
off (power.turnOffScreensTimeout, power.pauseCompositorOnStandby) because the null headset never moves.
A null universe without a room setup makes SteamVR start Room Setup, a scene app that closes the game: run
Room Setup once on the null driver first.
"""
import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SETTINGS = Path(r'C:\Program Files (x86)\Steam\config\steamvr.vrsettings')
STATE = ROOT / 'build' / 'steamvr-null' / 'state.json'
PROCESSES = ('vrmonitor.exe', 'vrserver.exe', 'vrcompositor.exe', 'vrdashboard.exe', 'vrwebhelper.exe',
             'vrstartup.exe', 'vrcompositor_x64.exe')
MISSING = '<missing>'


def running():
    out = subprocess.run(['tasklist', '/FO', 'CSV', '/NH'], capture_output=True, text=True).stdout.lower()
    return [p for p in PROCESSES if '"' + p + '"' in out]


def load():
    return json.loads(SETTINGS.read_text(encoding='utf-8'))


def save(data):
    # SteamVR writes the file with three-space indents and " : " separators.
    SETTINGS.write_text(json.dumps(data, indent=3, separators=(',', ' : ')) + '\n', encoding='utf-8')


def wanted(args):
    return {
        ('steamvr', 'forcedDriver'): 'null',
        ('steamvr', 'activateMultipleDrivers'): True,
        ('steamvr', 'requireHmd'): False,
        ('driver_null', 'enable'): True,
        ('driver_null', 'renderWidth'): args.width,
        ('driver_null', 'renderHeight'): args.height,
        ('driver_null', 'windowWidth'): args.window_width,
        ('driver_null', 'windowHeight'): args.window_height,
        ('driver_null', 'windowX'): 0,
        ('driver_null', 'windowY'): 0,
        ('driver_null', 'displayFrequency'): float(args.hz),
        # The null headset never moves, so SteamVR's idle rule (turnOffScreensTimeout, 5 s by default) puts it in
        # standby 5 s after any activity and the session drops to SYNCHRONIZED: no VISIBLE/FOCUSED, nothing shown.
        ('power', 'turnOffScreensTimeout'): 86400.0,
        ('power', 'pauseCompositorOnStandby'): False,
    }


def refuse_if_running():
    alive = running()
    if alive:
        sys.exit('SteamVR is running (%s): close it first (`stop`), it rewrites steamvr.vrsettings' % ', '.join(alive))


def on(args):
    refuse_if_running()
    if STATE.exists():
        sys.exit('Already on (%s); run `off` first' % STATE)
    data = load()
    STATE.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(SETTINGS, STATE.parent / ('steamvr.vrsettings.' + time.strftime('%Y%m%d-%H%M%S')))
    previous = []
    for (section, key), value in wanted(args).items():
        old = data.get(section, {}).get(key, MISSING)
        previous.append({'section': section, 'key': key, 'old': old, 'sectionExisted': section in data})
        data.setdefault(section, {})[key] = value
    STATE.write_text(json.dumps(previous, indent=1), encoding='utf-8')
    save(data)
    print('null driver ON: %dx%d per eye at %s Hz, window %dx%d' % (args.width, args.height, args.hz,
                                                                    args.window_width, args.window_height))


def off(_args):
    refuse_if_running()
    if not STATE.exists():
        sys.exit('Not on: no %s' % STATE)
    data = load()
    previous = json.loads(STATE.read_text(encoding='utf-8'))
    for row in previous:
        section = data.setdefault(row['section'], {})
        if row['old'] == MISSING:
            section.pop(row['key'], None)
        else:
            section[row['key']] = row['old']
    for row in previous:
        if not row['sectionExisted'] and row['section'] in data and not data[row['section']]:
            del data[row['section']]
    save(data)
    STATE.unlink()
    print('null driver OFF: %d keys restored' % len(previous))


def status(_args):
    data = load()
    print('SteamVR running:', ', '.join(running()) or 'no')
    print('state file:', STATE if STATE.exists() else 'none (off)')
    for section, key in wanted(argparse.Namespace(width=0, height=0, window_width=0, window_height=0, hz=0)):
        print('  %s.%s = %s' % (section, key, data.get(section, {}).get(key, MISSING)))


def stop(_args):
    for name in PROCESSES:
        subprocess.run(['taskkill', '/F', '/IM', name], capture_output=True)
    deadline = time.monotonic() + 15
    while running() and time.monotonic() < deadline:
        time.sleep(0.3)
    print('SteamVR running after stop:', ', '.join(running()) or 'no')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['on', 'off', 'status', 'stop'])
    parser.add_argument('--width', type=int, default=2648)
    parser.add_argument('--height', type=int, default=2864)
    parser.add_argument('--window-width', type=int, default=1920)
    parser.add_argument('--window-height', type=int, default=1080)
    parser.add_argument('--hz', type=float, default=90.0)
    args = parser.parse_args()
    {'on': on, 'off': off, 'status': status, 'stop': stop}[args.command](args)


if __name__ == '__main__':
    main()
