"""A two-PC NightCoop session for the port's multiplayer tests: this PC hosts, the second PC (HOME-PC) joins.

    py -3.13 -B tools/mp/coop.py launch [--save ManualSave-491]   # both games into the save, the harness simulator
    py -3.13 -B tools/mp/coop.py session                          # NightCoopHost here, NightCoopJoin there, wait
    py -3.13 -B tools/mp/coop.py status                           # both sides' NightCoop state
    py -3.13 -B tools/mp/coop.py close                            # both games closed, the runtimes put back

Both games run the OpenXR Simulator with the test harness (SimXR_* exports: tools/sim_harness/simxr.py drives the
hands), launched by tools/single_pass_tests/geometry_cost_live.py (launch, Continue into the world); the second PC's
in the logged-on user's desktop session (tools/remote/home_pc.py task). NightCoop's natives are called through
CET's CETBridge (tools/mp/cet.py). The state lives in build/mp/state.json.
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(HERE), str(ROOT / 'tools/remote')]
import cet  # noqa: E402
import home_pc  # noqa: E402

KEY = r'HKLM\SOFTWARE\Khronos\OpenXR\1'
HARNESS_HERE = Path(r'C:\Users\dariulone\Downloads\OpenXR-Simulator-v1.5.0-win64\openxr_simulator_abs.json')
# the absolute manifests: with the simulator's own relative one the elevated game's loader fails (XrInstance res=-51)
HARNESS_HOME = r'C:\Users\lone33\Desktop\OpenXR-Simulator-harness\openxr_simulator_abs.json'
LIVE = ROOT / 'tools/single_pass_tests/geometry_cost_live.py'
HOME_ROOT = r'X:\CyberpunkVRPort'
HOME_MP = r'X:\cvr-tmp\mp'
STATE = ROOT / 'build/mp/state.json'
PORT = 47800
HOME_ADDR = home_pc.HOST.split('@')[-1]
INI = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\config\vrport.ini'
# two games plus the simulators on two PCs: frame generation keeps the render load of each one down
COOP_INI = dict(xr_framegen='1')


def log(text):
    print(f'[coop {time.strftime("%H:%M:%S")}] {text}', flush=True)


def load():
    return json.loads(STATE.read_text(encoding='utf-8')) if STATE.exists() else {}


def save(st):
    STATE.parent.mkdir(parents=True, exist_ok=True)
    STATE.write_text(json.dumps(st, indent=1), encoding='utf-8')


def runtime_here():
    q = subprocess.run(['reg', 'query', KEY, '/v', 'ActiveRuntime'], capture_output=True, text=True).stdout
    return q.split('REG_SZ')[-1].strip() if 'REG_SZ' in q else ''


def set_runtime_here(path):
    subprocess.run(['reg', 'add', KEY, '/v', 'ActiveRuntime', '/t', 'REG_SZ', '/d', str(path), '/f'], check=True,
                   capture_output=True)


def home(script, timeout=120):
    r = home_pc.run(script, timeout=timeout)
    return r.stdout.strip()


def ini_set_here(values):
    """`values` into this PC's vrport.ini; returns the lines' old values ('' for a key not in the file)."""
    path = Path(INI)
    lines = path.read_text(encoding='utf-8').splitlines()
    old, seen = {}, set()
    for n, line in enumerate(lines):
        k, sep, v = line.partition('=')
        if sep and k.strip() in values:
            old[k.strip()] = v.strip()
            lines[n] = f'{k.strip()}={values[k.strip()]}'
            seen.add(k.strip())
    lines += [f'{k}={v}' for k, v in values.items() if k not in seen and v != '']
    old.update({k: '' for k in values if k not in seen})
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    return old


def ini_set_home(values):
    """The same on the second PC (one PowerShell script; the old values come back as k=v lines)."""
    pairs = ';'.join(f"'{k}'='{v}'" for k, v in values.items())
    script = (f"$p = '{INI}'; $want = @{{{pairs}}}; $lines = [Collections.Generic.List[string]]"
              "[IO.File]::ReadAllLines($p); $seen = @{}\n"
              "for ($n = 0; $n -lt $lines.Count; $n++) { $k, $v = $lines[$n] -split '=', 2\n"
              "  if ($null -ne $v -and $want.ContainsKey($k.Trim())) { 'old ' + $k.Trim() + '=' + $v.Trim(); "
              "$lines[$n] = $k.Trim() + '=' + $want[$k.Trim()]; $seen[$k.Trim()] = 1 } }\n"
              "foreach ($k in $want.Keys) { if (-not $seen[$k]) { 'old ' + $k + '='; "
              "if ($want[$k] -ne '') { $lines.Add($k + '=' + $want[$k]) } } }\n"
              "[IO.File]::WriteAllText($p, ($lines -join \"`n\") + \"`n\", (New-Object Text.UTF8Encoding $false))")
    out = home(script)
    return dict(l[4:].split('=', 1) for l in out.splitlines() if l.startswith('old '))


def my_address():
    out = subprocess.run(['powershell', '-NoProfile', '-Command',
                          "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -eq 'Ethernet 2' }).IPAddress"],
                         capture_output=True, text=True).stdout.strip()
    return out.splitlines()[0] if out else '192.168.2.243'


def guest_launch(stamp, save_name):
    """The second PC's game into the save, in its desktop session; returns the state's guest keys."""
    home_state = f'{HOME_MP}\\{stamp}-guest.json'
    home(f"New-Item -ItemType Directory -Force '{HOME_MP}' | Out-Null")
    cmd = (f'py -3.13 -B {HOME_ROOT}\\tools\\single_pass_tests\\geometry_cost_live.py launch --state {home_state} '
           f'--timeout 300 --save {save_name} && py -3.13 -B {HOME_ROOT}\\tools\\single_pass_tests\\geometry_cost_live.py '
           f'continue --state {home_state} --timeout 90')
    log(home_pc.task('CVR_coop_guest', cmd, f'{HOME_MP}\\{stamp}-guest.log').stdout.strip())
    return dict(guestState=home_state, guestLog=f'{HOME_MP}\\{stamp}-guest.log')


def wait_guest(st, seconds=420):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        text = home(f"if (Test-Path '{st['guestLog']}') {{ Get-Content '{st['guestLog']}' -Tail 3 }}")
        if '"continuePresses"' in text or 'stableHud' in text:
            log('guest in the world')
            return True
        if 'Traceback' in text or 'Error' in text:
            log(f'guest launch failed:\n{text}')
            return False
        time.sleep(5)
    log('guest not in the world in time')
    return False


def relaunch_guest(args):
    """Only the second PC's game again (its first launch failed), the host's left as it is."""
    st = load()
    if not st.get('launched'):
        sys.exit('no session: launch first')
    # only the game the guest launch started (its pid in the guest state)
    home(f"$s = '{st['guestState']}'; if (Test-Path $s) {{ $p = (Get-Content $s -Raw | ConvertFrom-Json).pid; "
         "if ($p) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue } }")
    time.sleep(5)
    home(f"Set-ItemProperty 'HKLM:\\SOFTWARE\\Khronos\\OpenXR\\1' -Name ActiveRuntime -Value '{HARNESS_HOME}'")
    st.update(guest_launch(time.strftime('%Y%m%d-%H%M%S'), args.save))
    save(st)
    wait_guest(st)


def launch(args):
    st = load()
    if st.get('launched'):
        sys.exit('a session is up: close it first')
    st = dict(runtimeHere=runtime_here(), runtimeHome=home(
        "(Get-ItemProperty 'HKLM:\\SOFTWARE\\Khronos\\OpenXR\\1').ActiveRuntime"))
    save(st)
    if args.framegen:
        st.update(iniHere=ini_set_here(COOP_INI), iniHome=ini_set_home(COOP_INI))
        save(st)
        log(f'frame generation on for the session (were: here {st["iniHere"]}, home {st["iniHome"]})')
    set_runtime_here(HARNESS_HERE)
    home(f"Set-ItemProperty 'HKLM:\\SOFTWARE\\Khronos\\OpenXR\\1' -Name ActiveRuntime -Value '{HARNESS_HOME}'")
    log(f'runtimes: here {runtime_here()}, home {HARNESS_HOME}')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    here_state = ROOT / f'build/mp/{stamp}-host.json'
    home_state = f'{HOME_MP}\\{stamp}-guest.json'
    # the second PC first (it loads in parallel); its launch runs in its desktop session
    st.update(launched=True, hostState=str(here_state))
    st.update(guest_launch(stamp, args.save))
    save(st)
    r = subprocess.run([sys.executable, '-B', str(LIVE), 'launch', '--state', str(here_state), '--timeout', '300',
                        '--save', args.save], cwd=ROOT, capture_output=True, text=True, errors='replace')
    print(r.stdout[-1500:], r.stderr[-800:])
    if json.loads(here_state.read_text(encoding='utf-8')).get('startup', {}).get('awaitingContinue'):
        r = subprocess.run([sys.executable, '-B', str(LIVE), 'continue', '--state', str(here_state), '--timeout', '90'],
                           cwd=ROOT, capture_output=True, text=True, errors='replace')
        print(r.stdout[-800:], r.stderr[-800:])
    log('host in the world; waiting for the guest')
    wait_guest(st)


def session(args):
    # The second PC hosts by default: this PC sits behind a second router (192.168.2.1, NAT) inside the second PC's
    # network, so the second PC's UDP never reaches it, while this PC's goes out and its replies come back.
    host_home = args.host == 'home'
    addr = args.address or (HOME_ADDR if host_home else my_address())
    names = {host_home: 'CVR-Host', not host_home: 'CVR-Guest'}
    # NightCoop starts no session without a nickname (its co-op menu asks for one)
    for side in (False, True):
        r = cet.call('exec', f'print(Game.NightCoopName("{names[side]}"))', on_home=side, timeout=30)
        log(f'{"home" if side else "here"} nickname {names[side]}: {r.get("result") or r}')
    r = cet.call('exec', f'print(Game.NightCoopHost({PORT}))', on_home=host_home, timeout=30)
    log(f'host ({"home" if host_home else "here"}): {r.get("result") or r}')
    time.sleep(3)
    r = cet.call('exec', f'print(Game.NightCoopJoin("{addr}", {PORT}))', on_home=not host_home, timeout=30)
    log(f'join {addr}:{PORT}: {r.get("result") or r}')
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        h = cet.call('eval', 'NightCoopSdk.InSession() and #NightCoopSdk.Peers() or -1', on_home=host_home, timeout=30)
        g = cet.call('eval', 'NightCoopSdk.InSession() and (NightCoopSdk.InHostWorld() and 1 or 0) or -1',
                     on_home=not host_home, timeout=30)
        log(f'host peers {h.get("result")}, guest in host world {g.get("result")}')
        if str(g.get('result')) == '1':
            st = load()
            st.update(hostSide='home' if host_home else 'here')
            save(st)
            return
        time.sleep(5)


def stop(_args):
    """Both sides out of the co-op session (the games stay)."""
    for side in (False, True):
        r = cet.call('exec', 'print(Game.NightCoopStop())', on_home=side, timeout=30)
        log(f'{"home" if side else "here"} stop: {r.get("result") or r}')


def status(_args):
    for side in (False, True):
        try:
            r = cet.call('exec', 'print(Game.NightCoopSdkStatus()); print(NightCoopSdk.InSession(), NightCoopSdk.IsHost(), '
                                 'NightCoopSdk.InHostWorld(), #NightCoopSdk.Peers())', on_home=side, timeout=20)
            text = r.get('result') or r.get('error') or ''
            log(('guest' if side else 'host') + ': ' + text.splitlines()[-1] if text else str(r))
        except Exception as e:
            log(('guest' if side else 'host') + f': {e}')


def close(_args):
    st = load()
    if st.get('hostState'):
        subprocess.run([sys.executable, '-B', str(LIVE), 'close', '--state', st['hostState']], cwd=ROOT)
    if st.get('guestState'):
        cmd = f'py -3.13 -B {HOME_ROOT}\\tools\\single_pass_tests\\geometry_cost_live.py close --state {st["guestState"]}'
        home_pc.task('CVR_coop_guest_close', cmd, f'{HOME_MP}\\close.log', wait=True, timeout=300)
    if st.get('runtimeHere'):
        set_runtime_here(st['runtimeHere'])
    if st.get('runtimeHome'):
        home(f"Set-ItemProperty 'HKLM:\\SOFTWARE\\Khronos\\OpenXR\\1' -Name ActiveRuntime -Value '{st['runtimeHome']}'")
    if st.get('iniHere'):
        ini_set_here(st['iniHere'])
    if st.get('iniHome'):
        ini_set_home(st['iniHome'])
    left = home("[bool](Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue)")
    log(f'closed; runtimes back (here {runtime_here()}); a game left on the second PC: {left}')
    log(f'QuickBoot taken out (it presses Continue at every launch): {remove_quickboot()}')
    STATE.unlink(missing_ok=True)


def remove_quickboot():
    """The test launches install QuickBoot (launch_to_save.py); a player's own launch must not have it: moved out of
    both games (here into build/quickboot-removed, there into X:\\cvr-tmp\\quickboot-removed)."""
    import shutil
    here = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\lua\CyberpunkVRPort_QuickBoot')
    said = []
    if here.exists():
        dst = ROOT / 'build/quickboot-removed' / time.strftime('%Y%m%d-%H%M%S')
        dst.mkdir(parents=True, exist_ok=True)
        shutil.move(str(here), str(dst / here.name))
        said.append('here')
    lua = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\lua'
    there = home(f"if (Test-Path '{lua}\\CyberpunkVRPort_QuickBoot') {{ New-Item -ItemType Directory -Force 'X:\\cvr-tmp\\quickboot-removed' | Out-Null; "
                 f"Remove-Item 'X:\\cvr-tmp\\quickboot-removed\\CyberpunkVRPort_QuickBoot' -Recurse -Force -ErrorAction SilentlyContinue; "
                 f"Move-Item '{lua}\\CyberpunkVRPort_QuickBoot' 'X:\\cvr-tmp\\quickboot-removed\\' -Force; 'moved' }}")
    if 'moved' in there:
        said.append('the second PC')
    return ', '.join(said) or 'not installed'


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest='cmd', required=True)
    l = sub.add_parser('launch')
    l.add_argument('--save', default='ManualSave-491')
    l.add_argument('--no-framegen', dest='framegen', action='store_false',
                   help='keep each PC\'s own xr_framegen (default: on for both during the session)')
    g = sub.add_parser('relaunch-guest')
    g.add_argument('--save', default='ManualSave-491')
    s = sub.add_parser('session')
    s.add_argument('--host', choices=['home', 'here'], default='home', help='which PC hosts (default: the second PC)')
    s.add_argument('--address', default='')
    sub.add_parser('stop')
    s.add_argument('--timeout', type=float, default=300)
    sub.add_parser('status')
    sub.add_parser('close')
    a = p.parse_args()
    dict(launch=launch, session=session, stop=stop, status=status, close=close,
         **{'relaunch-guest': relaunch_guest})[a.cmd](a)


if __name__ == '__main__':
    main()
