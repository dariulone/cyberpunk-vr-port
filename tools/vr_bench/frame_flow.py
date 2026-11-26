"""Does the headset keep getting new pictures while the GPU runs behind the game (#243 / PR #275)? One owned launch,
the port's display-cycle counters read once a second:

    py -3.13 -B tools/vr_bench/frame_flow.py --runtime sim|steamvr --label L [--res 3600x3900] [--steady 30]
                                             [--menus 8] [--save ManualSave-491]

A GPU-bound phase (the launcher's resolution raised with custom_res=1) and then pause-menu cycles (Esc on the game
window, as PR #275's reporter saw it start after a menu), then a recovery phase. For each second: display cycles with a
projection layer, of them NEW game pictures and repeats, resubmits of the last frame (no new capture taken), EMPTY ends
(no layers -- what made Steam Link reset its stream), cycles whose newest capture was still on the GPU (not done) and
of them the ones a finished older frame went out for. --runtime sim registers the bench simulator as HKLM's runtime;
steamvr turns SteamVR's null driver on (tools/steamvr_null) at the same per-eye size. Everything is put back.
Results: build/frame-flow/<stamp>-<label>.json and a short table.
"""
import argparse
import datetime
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(HERE), str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/swimming_tests'),
                str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/roomscale_tests')]
import launch_to_save as boot  # noqa: E402
from vr_bench import Game, GAME, PORT, LIVE, log  # noqa: E402

KEY = r'HKLM\SOFTWARE\Khronos\OpenXR\1'
SIM_JSON = Path(os.environ.get('CVR_SIM_DLL') or Path.home() / 'Desktop/OpenXR-Simulator-bench/bin/openxr_simulator.dll').with_suffix('.json')
STEAMVR_JSON = Path(r'C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json')
NULL = ROOT / 'tools/steamvr_null/steamvr_null.py'
LAUNCHER = PORT / 'config/vrport-launcher.ini'
INI = PORT / 'config/vrport.ini'
OUT = ROOT / 'build/frame-flow'
COUNTERS = ['CyberpunkVR_XrProjectionCycles', 'CyberpunkVR_XrRepeatCycles', 'CyberpunkVR_XrResubmits',
            'CyberpunkVR_XrEmptyEnds', 'CyberpunkVR_XrCaptureNotDone', 'CyberpunkVR_XrCapturePrevious',
            'CyberpunkVR_DebugCapOk']
SHORT = dict(zip(COUNTERS, ['cycles', 'repeat', 'resubmit', 'empty', 'notDone', 'previous', 'captured']))


def active_runtime():
    q = subprocess.run(['reg', 'query', KEY, '/v', 'ActiveRuntime'], capture_output=True, text=True).stdout
    return q.split('REG_SZ')[-1].strip() if 'REG_SZ' in q else ''


def set_runtime(path):
    subprocess.run(['reg', 'add', KEY, '/v', 'ActiveRuntime', '/t', 'REG_SZ', '/d', str(path), '/f'], check=True,
                   capture_output=True)


def live(step, state, *extra, timeout=400):
    r = subprocess.run([sys.executable, '-B', str(LIVE), step, '--state', str(state), *extra], cwd=ROOT,
                       capture_output=True, text=True, errors='replace', timeout=timeout)
    for line in (r.stdout + r.stderr).splitlines()[-12:]:
        print('   ', line, flush=True)
    if r.returncode:
        raise RuntimeError(f'live {step} failed ({r.returncode})')


def continue_to_world(state, timeout=90):
    """geometry_cost_live's Continue without its HUD gate (under SteamVR's null driver the HUD check reads an
    untracked head): Space on the game window until QuickBoot reports the world ready; the counters then say what
    the headset got."""
    st = json.loads(state.read_text(encoding='utf-8'))
    pid, run_id = st['pid'], st['startup']['runId']
    status_path = boot.port_folder(boot.DEFAULT_GAME) / 'lua' / boot.MOD_NAME / 'quickboot_status.json'
    previous = boot.user32.GetForegroundWindow()
    deadline = time.monotonic() + timeout
    presses, next_press = 0, 0.0
    while time.monotonic() < deadline:
        if pid not in boot.process_ids('Cyberpunk2077.exe'):
            raise RuntimeError('the game exited')
        status = boot.read_status(status_path, run_id)
        if presses and boot.world_ready(status):
            log(f'world ready after {presses} Continue press(es)')
            return
        waiting = presses == 0 or (st['startup'].get('source') == 'loading-screen' and boot.can_press_continue(status))
        if waiting and time.monotonic() >= next_press:
            if presses >= 4:
                raise RuntimeError('Continue prompt remained after four Space presses')
            row = boot.only_visible_window(boot.windows_for(pid), boot.GAME_CLASS)
            if not row:
                raise RuntimeError('no game window to press Continue on')
            boot.press_continue(row, pid, previous, True)
            presses += 1
            next_press = time.monotonic() + 3.0
        time.sleep(0.1)
    raise TimeoutError('world not ready after Continue')


def ini_set(text, values):
    lines, seen = [], set()
    for line in text.splitlines():
        k, sep, _ = line.partition('=')
        if sep and k.strip() in values:
            lines.append(f'{k.strip()}={values[k.strip()]}')
            seen.add(k.strip())
        else:
            lines.append(line)
    return '\n'.join(lines + [f'{k}={v}' for k, v in values.items() if k not in seen]) + '\n'


def press_escape(pid):
    """Esc on the game window (the pause menu), focus given back to the window that had it."""
    row = boot.only_visible_window(boot.windows_for(pid), boot.GAME_CLASS)
    if not row:
        raise RuntimeError('no game window')
    previous = boot.user32.GetForegroundWindow()
    boot.focus_window(row['hwnd'])
    try:
        time.sleep(0.2)
        event = boot.Input()
        event.type = 1
        event.payload.key = boot.Keyboard(0, 0x01, 8, 0, 0)   # Esc scan code
        boot.user32.SendInput(1, boot.c.byref(event), boot.c.sizeof(event))
        time.sleep(0.12)
        event.payload.key.flags = 10
        boot.user32.SendInput(1, boot.c.byref(event), boot.c.sizeof(event))
        time.sleep(0.2)
    finally:
        try:
            boot.restore_foreground(previous, pid)
        except Exception as e:   # the measurement goes on with the game in front
            log(f'  focus not restored: {e}')


def thread_named(pid, name):
    """The id of the game's thread whose description is `name` (GetThreadDescription), or None."""
    import ctypes as c
    import ctypes.wintypes as w
    k = c.WinDLL('kernel32', use_last_error=True)
    k.CreateToolhelp32Snapshot.restype = w.HANDLE
    k.OpenThread.restype = w.HANDLE
    k.OpenThread.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    k.Thread32First.argtypes = k.Thread32Next.argtypes = [w.HANDLE, c.c_void_p]
    k.CloseHandle.argtypes = [w.HANDLE]
    k.LocalFree.argtypes = [c.c_void_p]
    k.GetThreadDescription.argtypes = [w.HANDLE, c.POINTER(w.LPWSTR)]

    class Entry(c.Structure):
        _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('tid', w.DWORD), ('pid', w.DWORD), ('base', w.LONG),
                    ('delta', w.LONG), ('flags', w.DWORD)]
    snap = k.CreateToolhelp32Snapshot(4, 0)   # TH32CS_SNAPTHREAD
    e = Entry()
    e.size = c.sizeof(Entry)
    found = None
    ok = k.Thread32First(snap, c.byref(e))
    while ok and found is None:
        if e.pid == pid:
            h = k.OpenThread(0x0800, False, e.tid)   # THREAD_QUERY_LIMITED_INFORMATION
            if h:
                text = w.LPWSTR()
                if k.GetThreadDescription(h, c.byref(text)) >= 0 and text.value == name:
                    found = e.tid
                if text:
                    k.LocalFree(c.cast(text, c.c_void_p))
                k.CloseHandle(h)
        ok = k.Thread32Next(snap, c.byref(e))
    k.CloseHandle(snap)
    return found


def stall_game_thread(pid, seconds):
    """The game thread suspended for `seconds` (no new game frame, the XR frame loop runs on): what the headset gets
    through a stall longer than the port's hold of the last frame."""
    import ctypes as c
    k = c.WinDLL('kernel32', use_last_error=True)
    k.OpenThread.restype = c.c_void_p
    tid = thread_named(pid, 'GameThread')
    if not tid:
        raise RuntimeError('no GameThread in the game')
    h = k.OpenThread(0x0002, False, tid)   # THREAD_SUSPEND_RESUME
    if not h:
        raise c.WinError(c.get_last_error())
    try:
        k.SuspendThread(c.c_void_p(h))
        time.sleep(seconds)
    finally:
        k.ResumeThread(c.c_void_p(h))
        k.CloseHandle(c.c_void_p(h))


class Sampler:
    def __init__(self, game):
        self.game, self.rows = game, []
        self.last = self.read()

    def read(self):
        return {SHORT[n]: self.game.u64(n) for n in COUNTERS}

    def tick(self, phase):
        cur = self.read()
        d = {k: cur[k] - self.last[k] for k in cur}
        d['new'] = d['cycles'] - d['repeat']
        d['phase'] = phase
        d['t'] = round(time.time(), 2)
        self.last = cur
        self.rows.append(d)
        return d


def run_phase(sampler, phase, seconds, every=None, pid=None):
    """`seconds` of one-second samples; `every` (s) presses Esc that often (a menu opened and closed in turn)."""
    end = time.monotonic() + seconds
    next_press = time.monotonic() + (every or 1e9)
    while time.monotonic() < end:
        time.sleep(max(0.0, 1.0 - (time.monotonic() % 1.0)))
        if every and time.monotonic() >= next_press:
            press_escape(pid)
            next_press = time.monotonic() + every
        d = sampler.tick(phase)
        log(f'  {phase:8s} cycles {d["cycles"]:3d} new {d["new"]:3d} repeat {d["repeat"]:3d} resubmit {d["resubmit"]:3d} '
            f'empty {d["empty"]:3d} notDone {d["notDone"]:3d} previous {d["previous"]:3d} captured {d["captured"]:3d}')


def summary(rows):
    out = {}
    for phase in dict.fromkeys(r['phase'] for r in rows):
        rs = [r for r in rows if r['phase'] == phase]
        tot = {k: sum(r[k] for r in rs) for k in ('cycles', 'new', 'repeat', 'resubmit', 'empty', 'notDone', 'previous', 'captured')}
        tot['seconds'] = len(rs)
        tot['secondsWithoutNew'] = sum(1 for r in rs if r['new'] == 0)
        tot['minNewPerSecond'] = min(r['new'] for r in rs)
        out[phase] = tot
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--runtime', choices=['sim', 'steamvr'], required=True)
    p.add_argument('--label', required=True)
    p.add_argument('--save', default='ManualSave-491')
    p.add_argument('--res', default='3600x3900', help='per-eye size (custom_res=1); "" keeps the launcher as it is')
    p.add_argument('--steady', type=float, default=30)
    p.add_argument('--menus', type=int, default=8, help='Esc presses (a menu opened, closed, opened...)')
    p.add_argument('--menu-every', type=float, default=4.0)
    p.add_argument('--recover', type=float, default=20)
    p.add_argument('--setting', action='append', default=[], help='vrport.ini key=value for this run (repeatable)')
    p.add_argument('--stalls', type=int, default=0, help='game-thread stalls after the recovery (0 = none)')
    p.add_argument('--stall-seconds', type=float, default=3.0)
    args = p.parse_args()
    if boot.process_ids('Cyberpunk2077.exe'):
        sys.exit('close the game first')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    OUT.mkdir(parents=True, exist_ok=True)
    state = OUT / f'{stamp}-{args.label}-state.json'
    previous_runtime = active_runtime()
    launcher_before = LAUNCHER.read_bytes() if LAUNCHER.exists() else None
    ini_before = INI.read_bytes()
    null_on = False
    result = dict(label=args.label, runtime=args.runtime, res=args.res, stamp=stamp)
    launched = False
    try:
        if args.res:
            w, h = args.res.lower().split('x')
            LAUNCHER.write_text(ini_set((launcher_before or b'').decode('utf-8', errors='replace'),
                                        dict(width=w, height=h, custom_res='1')), encoding='utf-8')
        if args.setting:
            INI.write_text(ini_set(ini_before.decode('utf-8', errors='replace'),
                                   dict(kv.split('=', 1) for kv in args.setting)), encoding='utf-8')
        result['settings'] = args.setting
        if args.runtime == 'sim':
            set_runtime(SIM_JSON)
        else:
            w, h = (args.res or '2648x2864').lower().split('x')
            subprocess.run([sys.executable, '-B', str(NULL), 'on', '--width', w, '--height', h], check=True, cwd=ROOT)
            null_on = True
            set_runtime(STEAMVR_JSON)
        log(f'runtime {active_runtime()}')
        live('launch', state, '--timeout', '300', '--save', args.save, timeout=420)
        launched = True
        st = json.loads(state.read_text(encoding='utf-8'))
        if st.get('startup', {}).get('awaitingContinue'):
            continue_to_world(state)
        pid = json.loads(state.read_text(encoding='utf-8'))['pid']
        game = Game(pid, symbols=COUNTERS + ['CyberpunkVR_RuntimeDiagnostics'])
        game.set32('CyberpunkVR_RuntimeDiagnostics', 1)
        time.sleep(10)
        sampler = Sampler(game)
        log('steady (GPU-bound)')
        run_phase(sampler, 'steady', args.steady)
        log(f'menus: {args.menus} Esc presses {args.menu_every:.0f} s apart')
        run_phase(sampler, 'menus', args.menus * args.menu_every + 0.5, every=args.menu_every, pid=pid)
        if args.menus % 2:
            press_escape(pid)   # the menu closed again
        log('recovery')
        run_phase(sampler, 'recover', args.recover)
        for i in range(args.stalls):
            log(f'stall {i + 1}: the game thread suspended {args.stall_seconds:.1f} s')
            th = __import__('threading').Thread(target=stall_game_thread, args=(pid, args.stall_seconds))
            th.start()
            run_phase(sampler, 'stall', args.stall_seconds + 4)
            th.join()
        result['rows'] = sampler.rows
        result['summary'] = summary(sampler.rows)
        game.set32('CyberpunkVR_RuntimeDiagnostics', 0)
        game.close()
    finally:
        if launched:
            try:
                live('close', state, timeout=180)
            except Exception as e:
                log(f'close failed: {e}')
        if launcher_before is not None:
            LAUNCHER.write_bytes(launcher_before)
        INI.write_bytes(ini_before)
        set_runtime(previous_runtime)
        if null_on:
            for _ in range(3):
                subprocess.run([sys.executable, '-B', str(NULL), 'stop'], cwd=ROOT)
                if subprocess.run([sys.executable, '-B', str(NULL), 'off'], cwd=ROOT).returncode == 0:
                    break
                time.sleep(5)
        log(f'runtime back: {active_runtime()}')
    out = OUT / f'{stamp}-{args.label}.json'
    out.write_text(json.dumps(result, indent=1), encoding='utf-8')
    log(f'results {out}')
    for phase, t in result.get('summary', {}).items():
        log(f'{phase:8s} {t}')


if __name__ == '__main__':
    main()
