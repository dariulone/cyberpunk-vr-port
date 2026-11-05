"""Start Cyberpunk once and return only after live gameplay and stable native HUD.

This is a development helper. It installs the repo's QuickBoot CET module, opens
the exact game executable, accepts this process's launcher after one second,
and handles the post-load Continue screen only while that screen is reported.
"""

import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests')]
from hud_readiness import wait_for_stable_hud

DEFAULT_GAME = Path(r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077")
SOURCE_MOD = ROOT / "mods" / "cet" / "CyberpunkVRPort_QuickBoot"
MOD_NAME = "CyberpunkVRPort_QuickBoot"
LAUNCHER_CLASS = "CyberpunkVRPortLauncherClass"
GAME_CLASS = "W2ViewportClass"
START_BUTTON_ID = 102
REPORT_QUEUE = Path.home() / "AppData" / "Local" / "REDEngine" / "ReportQueue"
WORLD_STABLE_SECONDS = 3.0
FOCUS_RETRY_SECONDS = 2.0
FOCUS_RETRY_INTERVAL = 0.10

user32 = c.WinDLL("user32", use_last_error=True)
user32.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
user32.EnumWindows.argtypes = [c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM), w.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
user32.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
user32.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
user32.IsWindowVisible.argtypes = [w.HWND]
user32.IsIconic.argtypes = [w.HWND]
user32.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user32.GetDlgItem.argtypes = [w.HWND, c.c_int]
user32.GetDlgItem.restype = w.HWND
user32.SendMessageTimeoutW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM,
                                     w.UINT, w.UINT, c.POINTER(c.c_size_t)]
user32.SendMessageTimeoutW.restype = c.c_size_t
user32.GetForegroundWindow.restype = w.HWND
user32.IsWindow.argtypes = [w.HWND]
user32.IsWindow.restype = w.BOOL
user32.SetForegroundWindow.argtypes = [w.HWND]
user32.SetForegroundWindow.restype = w.BOOL
user32.AttachThreadInput.argtypes = [w.DWORD, w.DWORD, w.BOOL]
user32.AttachThreadInput.restype = w.BOOL
user32.ShowWindowAsync.argtypes = [w.HWND, c.c_int]
user32.BringWindowToTop.argtypes = [w.HWND]
user32.SendInput.argtypes = [w.UINT, c.c_void_p, c.c_int]
user32.SendInput.restype = w.UINT
user32.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
user32.PostMessageW.restype = w.BOOL
kernel32 = c.WinDLL("kernel32", use_last_error=True)
kernel32.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
kernel32.CreateToolhelp32Snapshot.restype = w.HANDLE
kernel32.Process32FirstW.argtypes = [w.HANDLE, c.c_void_p]
kernel32.Process32NextW.argtypes = [w.HANDLE, c.c_void_p]
kernel32.CloseHandle.argtypes = [w.HANDLE]
kernel32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel32.OpenProcess.restype = w.HANDLE
kernel32.TerminateProcess.argtypes = [w.HANDLE, w.UINT]
kernel32.GetCurrentThreadId.restype = w.DWORD
kernel32.QueryFullProcessImageNameW.argtypes = [w.HANDLE, w.DWORD, w.LPWSTR, c.POINTER(w.DWORD)]


class ProcessEntry(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
                ("default_heap", c.c_size_t), ("module_id", w.DWORD),
                ("threads", w.DWORD), ("parent_pid", w.DWORD),
                ("priority", w.LONG), ("flags", w.DWORD),
                ("exe", w.WCHAR * 260)]


def process_ids(executable):
    snapshot = kernel32.CreateToolhelp32Snapshot(2, 0)
    if snapshot == w.HANDLE(-1).value:
        raise c.WinError(c.get_last_error())
    found = set()
    try:
        entry = ProcessEntry()
        entry.size = c.sizeof(entry)
        exists = kernel32.Process32FirstW(snapshot, c.byref(entry))
        while exists:
            if entry.exe.casefold() == executable.casefold():
                found.add(entry.pid)
            exists = kernel32.Process32NextW(snapshot, c.byref(entry))
    finally:
        kernel32.CloseHandle(snapshot)
    return found


def reporter_pids():
    return process_ids("CrashReporter.exe")


def process_path(pid):
    handle = kernel32.OpenProcess(0x1000, False, pid)
    if not handle:
        return None
    try:
        value, length = c.create_unicode_buffer(32768), w.DWORD(32768)
        if not kernel32.QueryFullProcessImageNameW(handle, 0, value, c.byref(length)):
            return None
        return Path(value.value)
    finally:
        kernel32.CloseHandle(handle)


def find_codex_window(preferred):
    candidates = []
    for pid in process_ids("Codex.exe") | process_ids("ChatGPT.exe"):
        path = process_path(pid)
        # The Store Codex desktop package currently hosts its UI in ChatGPT.exe.
        # Do not select a different ChatGPT installation or the Codex CLI.
        if not path or not any(part.casefold().startswith("openai.codex_") for part in path.parts):
            continue
        candidates.extend(row for row in windows_for(pid)
                          if row["visible"] and row["width"] >= 320 and row["height"] >= 240)
    for row in candidates:
        if row["hwnd"] == preferred:
            return preferred
    # No Codex desktop at all (a launch from a terminal agent, e.g. tools/vr_bench): focus goes back to the window
    # that had it when the launch began.
    if not candidates and preferred and user32.IsWindow(preferred) and user32.IsWindowVisible(preferred):
        return preferred
    if len(candidates) != 1:
        raise RuntimeError(f"Expected one Codex desktop window, found {len(candidates)}")
    return candidates[0]["hwnd"]


def reporter_window_pids():
    found = set()
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    @callback_type
    def visit(hwnd, _):
        title = c.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, title, len(title))
        if title.value in ("Отчёт об ошибке Cyberpunk 2077",
                           "Отчет об ошибке Cyberpunk 2077"):
            found.add(process_id(hwnd))
        return True

    if not user32.EnumWindows(visit, 0):
        raise c.WinError(c.get_last_error())
    return found


def new_reporters(baseline):
    return (reporter_pids() | reporter_window_pids()) - baseline


def close_reporters(pids):
    for pid in pids:
        rows = windows_for(pid)
        for row in rows:
            if row["visible"] and "Cyberpunk 2077" in row["title"]:
                user32.PostMessageW(row["hwnd"], 0x0010, 0, 0)  # WM_CLOSE
        deadline = time.monotonic() + 2.0
        while pid in reporter_pids() and time.monotonic() < deadline:
            time.sleep(0.1)
        if pid in reporter_pids():
            handle = kernel32.OpenProcess(1, False, pid)  # PROCESS_TERMINATE
            if handle:
                try:
                    kernel32.TerminateProcess(handle, 1)
                finally:
                    kernel32.CloseHandle(handle)
        print(f"Closed CrashReporter.exe PID {pid}", flush=True)


def fail_for_crash(pid, started_at, baseline_reporters, detail):
    report = None
    reporters = set()
    deadline = time.monotonic() + 3.0
    while time.monotonic() < deadline:
        report = crash_report(pid, started_at) or report
        reporters |= new_reporters(baseline_reporters)
        if report and reporters:
            break
        time.sleep(0.1)
    for reporter in sorted(reporters):
        try:
            print(json.dumps(dict(reporterPid=reporter, executable=str(process_path(reporter)),
                                  windows=windows_for(reporter)), ensure_ascii=True), flush=True)
        except Exception as error:
            print(f"Reporter metadata unavailable for {reporter}: {error}", flush=True)
    if not report and reporters:
        # A live-thread dump is evidence of the waiting process, not an exception
        # dump. Bound the helper so a broken reporter cannot stall launch cleanup.
        try:
            directory=ROOT/'build'/'quickboot-launch'/'failure-evidence'
            directory.mkdir(parents=True,exist_ok=True)
            destination=directory/f'{pid}-{uuid.uuid4().hex}.dmp'
            code='import sys,json; sys.path.insert(0,sys.argv[1]); from live_dump import capture; print(json.dumps(capture(int(sys.argv[2]),sys.argv[3])))'
            result=subprocess.run([sys.executable,'-B','-c',code,str(ROOT/'tools'/'crash'),str(pid),str(destination)],
                                  capture_output=True,text=True,timeout=10)
            print(f"Reporter without native dump; live thread evidence: {result.stdout.strip() or result.stderr.strip()}",flush=True)
            report=crash_report(pid,started_at) or report
        except Exception as error:
            print(f"Live thread evidence unavailable: {error}",flush=True)
    if reporters:
        close_reporters(reporters)
    raise RuntimeError(f"{detail}; {report or 'no PID-matched report yet'}")


def process_id(hwnd):
    value = w.DWORD()
    user32.GetWindowThreadProcessId(hwnd, c.byref(value))
    return value.value


def windows_for(pid):
    rows = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    @callback_type
    def visit(hwnd, _):
        if process_id(hwnd) != pid:
            return True
        name = c.create_unicode_buffer(256)
        title = c.create_unicode_buffer(256)
        rect = w.RECT()
        user32.GetClassNameW(hwnd, name, len(name))
        user32.GetWindowTextW(hwnd, title, len(title))
        user32.GetClientRect(hwnd, c.byref(rect))
        rows.append({"hwnd": hwnd, "class": name.value, "title": title.value,
                     "visible": bool(user32.IsWindowVisible(hwnd)),
                     "minimized": bool(user32.IsIconic(hwnd)),
                     "width": rect.right - rect.left,
                     "height": rect.bottom - rect.top})
        return True

    if not user32.EnumWindows(visit, 0):
        raise c.WinError(c.get_last_error())
    return rows


def only_visible_window(rows, class_name):
    matches = [row for row in rows if row["class"] == class_name and row["visible"]
               and not row["minimized"]]
    if len(matches) > 1:
        raise RuntimeError(f"Multiple visible {class_name} windows in this PID")
    return matches[0] if matches else None


def click_launcher(row, pid):
    button = user32.GetDlgItem(row["hwnd"], START_BUTTON_ID)
    if not button or process_id(button) != pid:
        raise RuntimeError("Start Game button does not belong to this game process")
    title = c.create_unicode_buffer(256)
    user32.GetWindowTextW(button, title, len(title))
    if title.value != "Start Game":
        raise RuntimeError(f"Unexpected launcher button: {title.value!r}")
    result = c.c_size_t()
    if not user32.SendMessageTimeoutW(button, 0xF5, 0, 0, 2, 2000, c.byref(result)):
        raise RuntimeError("Start Game button did not accept the click")


class Keyboard(c.Structure):
    _fields_ = [("vk", w.WORD), ("scan", w.WORD), ("flags", w.DWORD),
                ("time", w.DWORD), ("extra", c.c_size_t)]


class Payload(c.Union):
    _fields_ = [("key", Keyboard), ("padding", c.c_byte * 32)]


class Input(c.Structure):
    _fields_ = [("type", w.DWORD), ("payload", Payload)]


def restore_foreground(previous, game_pid):
    if not previous or not user32.IsWindow(previous) or process_id(previous) == game_pid:
        return
    if process_id(user32.GetForegroundWindow()) != game_pid:
        return
    focus_window(previous)


class ForegroundRejected(RuntimeError):
    """The live target exists, but Windows did not grant foreground yet."""


def focus_window(hwnd):
    if not hwnd or not user32.IsWindow(hwnd):
        raise RuntimeError("Focus target no longer exists")
    if user32.IsIconic(hwnd):
        user32.ShowWindowAsync(hwnd, 9)  # SW_RESTORE
    if user32.GetForegroundWindow() == hwnd:
        return
    attached = []
    current = kernel32.GetCurrentThreadId()
    try:
        for window in (user32.GetForegroundWindow(), hwnd):
            thread = user32.GetWindowThreadProcessId(window, None) if window else 0
            if thread and thread != current and thread not in attached:
                if user32.AttachThreadInput(current, thread, True):
                    attached.append(thread)
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
        if user32.GetForegroundWindow() != hwnd:
            # The foreground lock refuses a process that did not send the last input event
            # while another window is in use. A synthetic ALT tap makes this process the last
            # input sender, which the documented SetForegroundWindow rules accept; ALT alone does
            # nothing in the window that currently owns the keyboard.
            user32.keybd_event(0x12, 0, 0, 0)        # VK_MENU down
            user32.keybd_event(0x12, 0, 0x0002, 0)   # VK_MENU up (KEYEVENTF_KEYUP)
            user32.SetForegroundWindow(hwnd)
    finally:
        for thread in reversed(attached):
            user32.AttachThreadInput(current, thread, False)
    foreground = user32.GetForegroundWindow()
    if foreground != hwnd:
        raise ForegroundRejected(f"Windows rejected the requested foreground window: target={hwnd}, actual={foreground}")


class LaunchFocus:
    def __init__(self, game_pid, codex_hwnd, clock=time.monotonic, activate=focus_window):
        self.pid, self.codex = game_pid, codex_hwnd
        self.clock, self.activate = clock, activate
        self.seen = self.game_at = self.codex_at = None
        self.retry_until = self.retry_next = None
        self.retry_error = None

    def reset_retry(self):
        self.retry_until = self.retry_next = None
        self.retry_error = None

    def check_retry_deadline(self, now):
        if self.retry_until is not None and now >= self.retry_until:
            raise TimeoutError(f"Foreground handoff was not verified within {FOCUS_RETRY_SECONDS:.1f}s: {self.retry_error}") from self.retry_error

    def try_activate(self, hwnd):
        now = self.clock()
        self.check_retry_deadline(now)
        if self.retry_next is not None and now < self.retry_next:
            return False
        if self.retry_until is None:
            self.retry_until = now + FOCUS_RETRY_SECONDS
        try:
            self.activate(hwnd)
        except ForegroundRejected as error:
            self.retry_error = error
            now = self.clock()
            self.check_retry_deadline(now)
            self.retry_next = now + FOCUS_RETRY_INTERVAL
            return False
        self.reset_retry()
        return True

    @property
    def complete(self):
        return self.codex_at is not None

    def update(self, row, launcher_visible=False):
        if launcher_visible:
            # The engine can create W2ViewportClass before showing our launcher.
            # That construction window must not start the five-second timer.
            if self.game_at is None:
                self.seen = None
                self.reset_retry()
            return None
        self.check_retry_deadline(self.clock())
        if self.complete or not row or row['class'] != GAME_CLASS or process_id(row['hwnd']) != self.pid:
            return None
        now = self.clock()
        if self.seen is None:
            self.seen = now
            return "Game viewport detected; focus game in five seconds, then Codex one second later"
        if self.game_at is None and now - self.seen >= 5.0:
            retrying = self.retry_until is not None
            if not self.try_activate(row['hwnd']):
                return None if retrying else "Windows deferred game foreground; retrying for up to two seconds"
            self.game_at = self.clock()
            return "Five seconds after game viewport: game foreground verified"
        if self.game_at is not None and now - self.game_at >= 1.0:
            retrying = self.retry_until is not None
            if not self.try_activate(self.codex):
                return None if retrying else "Windows deferred Codex foreground; retrying for up to two seconds"
            self.codex_at = self.clock()
            return "One second after game activation: Codex foreground verified"
        return None

    def report(self):
        return dict(verified=self.complete,
                    gameAfterSeconds=None if self.game_at is None else self.game_at - self.seen,
                    codexAfterGameSeconds=None if not self.complete else self.codex_at - self.game_at)


# Never called by this launcher (it stops on Continue, see main); an explicit continue step presses it.
def press_continue(row, pid, previous, restore_focus):
    if row["class"] != GAME_CLASS or process_id(row["hwnd"]) != pid:
        raise RuntimeError("Continue target is not the launched game")
    focus_window(row["hwnd"])
    if process_id(user32.GetForegroundWindow()) != pid:
        raise RuntimeError("Game did not receive foreground; Continue was not sent")
    try:
        time.sleep(0.25)
        if process_id(user32.GetForegroundWindow()) != pid:
            raise RuntimeError('Game lost foreground before Continue input')
        event = Input()
        event.type = 1
        event.payload.key = Keyboard(0, 0x39, 8, 0, 0)  # Space scan code
        if user32.SendInput(1, c.byref(event), c.sizeof(event)) != 1:
            raise RuntimeError("Continue key press failed")
        try:
            time.sleep(0.25)
        finally:
            event.payload.key.flags = 10
            if user32.SendInput(1, c.byref(event), c.sizeof(event)) != 1:
                raise RuntimeError("Continue key release failed")
        time.sleep(0.25)
    finally:
        if restore_focus:
            restore_foreground(previous, pid)


def read_status(path, run_id):
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) and value.get("runId") == run_id else None


def world_ready(status):
    return bool(status and status.get('phase') == 'world_ready' and status.get('player') and
                status.get('worldValid') and status.get('preGame') is False and
                status.get('loading') is False and status.get('paused') is False and
                status.get('stableFrames', 0) >= 30)


def can_press_continue(status):
    return bool(status and status.get('phase') == 'awaiting_continue' and
                status.get('continueReady') is True and status.get('player') is True and
                status.get('worldValid') is True and status.get('preGame') is False and
                status.get('loading') is True and status.get('paused') is False)


def can_confirm_save(status):
    dialog = status.get('loadConfirmation', {}) if status else {}
    return bool(status and status.get('fired') is True and status.get('preGame') is True and
                status.get('phase') == 'loading' and dialog.get('kind') == 'modded-save' and
                dialog.get('ready') is True)


class AutoProceed:
    """Bounded inputs for proven launch screens; never send a retry into gameplay."""
    def __init__(self, send):
        self.send = send
        self.counts = {}
        self.next_at = 0.0

    def update(self, status, now, loading_ready=False, startup_held=False):
        kind = ('modded-save' if can_confirm_save(status) else
                'loading-screen' if loading_ready and can_press_continue(status) else
                'startup-menu' if startup_held and world_ready(status) else None)
        if not kind or now < self.next_at:
            return False
        count = self.counts.get(kind, 0)
        if count >= 4:
            raise RuntimeError(f'{kind} remained after four simulator confirmations')
        self.send(kind)
        self.counts[kind] = count + 1
        self.next_at = now + 3.0
        print(f'Auto-confirm: {kind} ({count + 1})', flush=True)
        return True


def press_simulator_confirm(pid, game, simulator):
    from simulator_input import PadGate
    with PadGate(pid, game, simulator) as pad:
        pad.jump(False)
        time.sleep(.12)
        try:
            pad.jump(True)
            time.sleep(.18)
        finally:
            pad.jump(False)
        time.sleep(.2)


def viewport_ready(row):
    return bool(row and row.get('class') == GAME_CLASS and row.get('visible') and not row.get('minimized', True)
                and row.get('width', 0) >= 320 and row.get('height', 0) >= 240)


class NativeResumeGate:
    def __init__(self, expected_mode=4):
        self.expected_mode=expected_mode
        self.since=None
        self.tick=0

    def observe(self,mode,tick,now):
        if mode!=self.expected_mode or tick<=self.tick:
            self.since=None
        elif self.since is None:
            self.since=now
        self.tick=tick
        return self.since is not None and now-self.since>=.1


def native_loading_state(pid,game,symbols):
    from live_read import Reader
    from live_symbols import resolve
    if not symbols:
        symbols.update(resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                               ['CyberpunkVR_MenuSourceSeq','CyberpunkVR_MenuSource']))
    reader=Reader(pid,game/'bin/x64/Cyberpunk2077.exe',0,0)
    try:
        import struct
        sequence=int(symbols['CyberpunkVR_MenuSourceSeq'],16)
        before=struct.unpack('<I',reader.read(sequence,4))[0]
        raw=reader.read(int(symbols['CyberpunkVR_MenuSource'],16),168)
        after=struct.unpack('<I',reader.read(sequence,4))[0]
        if not before or before&1 or before!=after:
            return -1,0
        return struct.unpack_from('<i',raw,164)[0],struct.unpack_from('<Q',raw)[0]
    finally:
        reader.close()


def can_confirm_native_continue(sample, status):
    if not world_ready(status) or sample['menu'] != 1 or sample['prepared'] or sample['submitted']:
        return False
    evidence = status.get('menuEvidence', {})
    controllers = evidence.get('menuControllers', [])
    return (evidence.get('menuBlackboard') is False and
            any(row.get('class') == 'gameuiInGameMenuGameController' and row.get('visible') is False for row in controllers) and
            all(row.get('visible') is False or row.get('class') == 'CursorGameController' for row in controllers))


def crash_report(pid, started_at):
    try:
        reports = (path for path in REPORT_QUEUE.glob(f"Cyberpunk2077-*-{pid}-*")
                   if path.is_dir() and path.stat().st_mtime >= started_at)
        newest = max(reports, key=lambda path: path.stat().st_mtime, default=None)
        if newest is None:
            return None
        stack = newest / "stacktrace.txt"
        reason = stack.read_text(encoding="utf-8", errors="replace").strip() if stack.exists() else ""
        return f"{newest}: {reason or 'crash report created'}"
    except OSError:
        return None


def install_quickboot(destination):
    destination.mkdir(parents=True, exist_ok=True)
    backup = ROOT / "build" / "quickboot-launch" / "backups" / time.strftime("%Y%m%d-%H%M%S")
    for name in ("init.lua", "quickboot.json", "README.md"):
        source, target = SOURCE_MOD / name, destination / name
        if target.exists() and target.read_bytes() == source.read_bytes():
            continue
        if target.exists():
            backup.mkdir(parents=True, exist_ok=True)
            shutil.copy2(target, backup / name)
        shutil.copy2(source, target)
    return backup if backup.exists() else None


def reflection_environment(parent, compiler):
    environment=parent.copy()
    if compiler:
        environment['CVR_STEREO_REFLECTION_DXC']=str(compiler.resolve())
    else:
        environment.pop('CVR_STEREO_REFLECTION_DXC',None)
    return environment


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game-root", type=Path, default=DEFAULT_GAME)
    parser.add_argument("--timeout", type=float, default=240.0)
    parser.add_argument('--no-focus',action='store_true',help='Keep window focus unchanged; Continue uses the simulator pad')
    parser.add_argument('--save',help='Load this save (its folder name, e.g. AutoSave-8) instead of the latest one')
    parser.add_argument('--hold-file',type=Path,help='Test process only: stay in the main menu while this file exists')
    parser.add_argument('--launcher-hold',type=Path,help='Test process only: leave the resolution launcher open while this file exists (a debugger attaches before the game starts)')
    parser.add_argument('--mods-root',type=Path,help="The Lua mods folder QuickBoot runs from (default: the plugin's lua folder)")
    parser.add_argument('--start-screen',action='store_true',help='Launch as a player does, without -skipStartScreen: the plugin skips the start screen')
    parser.add_argument('--simulator',type=Path,help='Exact loaded simulator DLL, required with --no-focus')
    parser.add_argument('--pause-at-continue',action='store_true',help='Diagnostic mode: leave the post-load Continue screen open for inspection')
    parser.add_argument("--observe-native-bindings", action="store_true", help="Test process only: capture complete D3D graphics state")
    parser.add_argument('--bounds-trace', action='store_true', help='Test process only: observe native scene bounds writes')
    parser.add_argument('--paired-root', action='store_true', help='Test process only: enable the bounded paired-frame root hook')
    parser.add_argument("--family-manifest", type=Path, help="Test process only: create linked stereo families without routing draws")
    parser.add_argument('--reflection-dxc',type=Path,help='Test process only: exact dxcompiler.dll used to inspect unconverted depth shaders')
    parser.add_argument('--source-selector-manifest',type=Path,help='Test process only: track producer intent from startup for an audited selector catalog')
    parser.add_argument('--game-arg',action='append',default=[],help='Extra game command-line switch (repeatable), e.g. -forceAftermathDebug')
    args = parser.parse_args()
    if args.no_focus and (not args.simulator or not args.simulator.is_file()):
        parser.error('--no-focus requires the exact --simulator DLL')
    game = args.game_root.resolve()
    executable = game / "bin" / "x64" / "Cyberpunk2077.exe"
    if not executable.is_file() or args.timeout < 15:
        parser.error("Game executable is missing or timeout is below 15 seconds")
    if args.family_manifest and not args.family_manifest.is_file():
        parser.error("Family manifest is missing")
    if args.source_selector_manifest and (not args.family_manifest or not args.source_selector_manifest.is_file()):
        parser.error('Source selectors require a family package and an existing selector catalog')
    if args.reflection_dxc and (not args.family_manifest or not args.reflection_dxc.is_file() or args.reflection_dxc.name.lower()!='dxcompiler.dll'):
        parser.error('Reflection requires a family package and an existing dxcompiler.dll')

    # The port's Lua mods run in the plugin's host, from its lua folder (src/Lua); --mods-root names another
    # (CET's mods folder, for an A/B run with the mods back in CET).
    from_root = args.mods_root or game / "red4ext" / "plugins" / "cyberpunkvrport" / "lua"
    if not from_root.is_dir():
        parser.error(f"The Lua mods folder is missing: {from_root}")
    existing = subprocess.run(["tasklist", "/FI", "IMAGENAME eq Cyberpunk2077.exe", "/FO", "CSV"],
                              capture_output=True, text=True, check=True)
    if "Cyberpunk2077.exe" in existing.stdout:
        parser.error("Cyberpunk is already running; this command owns one launch only")

    mod = from_root / MOD_NAME
    backup = install_quickboot(mod)
    run_id = uuid.uuid4().hex
    run_control=dict(runId=run_id,requireNativeReady=True,allowResume=False,expiresAt=int(time.time()+args.timeout))
    if args.save: run_control['save']=args.save
    (mod / "quickboot_run.json").write_text(json.dumps(run_control), encoding="utf-8")
    print(f"QuickBoot installed; prior files backed up at {backup}" if backup else "QuickBoot ready", flush=True)
    user32.SetProcessDpiAwarenessContext(c.c_void_p(-4))
    # Only the focus sequence restores the desktop window; with --no-focus there is nothing to find.
    codex_foreground = None if args.no_focus else find_codex_window(user32.GetForegroundWindow())
    baseline_reporters = reporter_pids() | reporter_window_pids()
    started_at = time.time()
    environment = reflection_environment(os.environ,args.reflection_dxc)
    environment['CVR_STARTUP_DIAGNOSTICS'] = '1'
    if args.paired_root:
        environment['CVR_STEREO_PAIRED_ROOT']='1'
    else:
        environment.pop('CVR_STEREO_PAIRED_ROOT',None)
    if args.bounds_trace:
        environment['CVR_STEREO_BOUNDS_TRACE']='1'
    else:
        environment.pop('CVR_STEREO_BOUNDS_TRACE',None)
    if args.observe_native_bindings:
        environment['CVR_STEREO_OBSERVE_BINDINGS'] = '1'
    else:
        environment.pop('CVR_STEREO_OBSERVE_BINDINGS', None)
    if args.family_manifest:
        environment['CVR_STEREO_FAMILY_MANIFEST'] = str(args.family_manifest.resolve())
    else:
        environment.pop('CVR_STEREO_FAMILY_MANIFEST', None)
    if args.source_selector_manifest:
        environment['CVR_STEREO_SOURCE_SELECTORS']=str(args.source_selector_manifest.resolve())
    else:
        environment.pop('CVR_STEREO_SOURCE_SELECTORS',None)
    process = subprocess.Popen([str(executable)] + ([] if args.start_screen else ["-skipStartScreen"]) + list(args.game_arg),
                               cwd=executable.parent, env=environment)
    print(f"Launched PID {process.pid}", flush=True)
    deadline = time.monotonic() + args.timeout
    launcher_seen = None
    focus = LaunchFocus(process.pid, codex_foreground)
    # Normal runs confirm recognized startup screens. Renderer inspection can
    # explicitly opt out with --pause-at-continue.
    def awaiting_continue(source):
        print(json.dumps({"ready": True, "awaitingContinue": True, "source": source, "pid": process.pid,
                          "viewport": None if not game_window else [game_window["width"], game_window["height"]],
                          "runId": run_id,
                          "focusSequence": {"skipped": True} if args.no_focus else focus.report()}), flush=True)
        return 0
    clicked = False
    last_phase = None
    ready_since = None
    viewport_waiting = False
    resume_gate=NativeResumeGate()
    # Layer mode 2 includes the input-ready initial loading screen. Readiness
    # itself comes from the current controller's visible native skip panel.
    continue_gate=NativeResumeGate(expected_mode=2)
    resume_symbols={}
    resume_released=False
    status_path = mod / "quickboot_status.json"
    simulator_path=args.simulator
    def send_confirmation(kind):
        nonlocal simulator_path
        if simulator_path is None:
            from live_symbols import loaded_module
            simulator_path=loaded_module(process.pid,'openxr_simulator.dll')
        if simulator_path is None:
            raise RuntimeError('Automatic confirmations require a loaded OpenXR Simulator')
        press_simulator_confirm(process.pid,game,simulator_path)
    auto_proceed=AutoProceed(send_confirmation)
    while time.monotonic() < deadline:
        if process.poll() is not None:
            fail_for_crash(process.pid, started_at, baseline_reporters,
                           f"Game exited before world_ready (code {process.returncode})")
        now = time.monotonic()
        rows = windows_for(process.pid)
        launcher = only_visible_window(rows, LAUNCHER_CLASS)
        game_window = only_visible_window(rows, GAME_CLASS)
        if launcher and launcher_seen is None:
            launcher_seen = now
            print("Resolution launcher detected; Start Game in one second", flush=True)
        held = args.launcher_hold is not None and args.launcher_hold.exists()
        if launcher and not clicked and not held and now - launcher_seen >= 1.0:
            click_launcher(launcher, process.pid)
            clicked = True
            print("Start Game clicked", flush=True)
        event = None if args.no_focus else focus.update(game_window, bool(launcher))
        if event:
            print(event, flush=True)

        report = crash_report(process.pid, started_at)
        if report or new_reporters(baseline_reporters):
            fail_for_crash(process.pid, started_at, baseline_reporters,
                           "CrashReporter.exe or Cyberpunk 2077 error window appeared")

        status = read_status(status_path, run_id)
        if status:
            phase = status.get("phase")
            if phase != last_phase:
                print(f"QuickBoot: {phase}", flush=True)
                last_phase = phase
            if isinstance(phase, str) and phase.startswith("failed:"):
                raise RuntimeError(f"QuickBoot could not resume the save: {phase}")
            auto_proceed.update(status, now)
            if phase=='awaiting_native_menu' and not resume_released and not (args.hold_file and args.hold_file.exists()):
                mode,tick=native_loading_state(process.pid,game,resume_symbols)
                if resume_gate.observe(mode,tick,time.monotonic()):
                    run_control['allowResume']=True
                    (mod/'quickboot_run.json').write_text(json.dumps(run_control),encoding='utf-8')
                    resume_released=True
                    print('Initial native loading layer is idle; Continue released',flush=True)
            if phase == "world_ready" and (args.no_focus or focus.complete):
                if not world_ready(status):
                    raise RuntimeError("Incomplete world_ready proof from QuickBoot")
                if not viewport_ready(game_window):
                    ready_since = None
                    if not viewport_waiting:
                        print(f'World present; waiting for a usable game viewport: {game_window}', flush=True)
                    viewport_waiting = True
                    time.sleep(.10)
                    continue
                viewport_waiting = False
                if ready_since is None:
                    ready_since = now
                    print("World ready; checking crash stability", flush=True)
                if now - ready_since < WORLD_STABLE_SECONDS:
                    time.sleep(0.10)
                    continue

                def check_world():
                    nonlocal game_window
                    if process.poll() is not None:
                        fail_for_crash(process.pid, started_at, baseline_reporters,
                                       f"Game exited while verifying HUD (code {process.returncode})")
                    if crash_report(process.pid, started_at) or new_reporters(baseline_reporters):
                        fail_for_crash(process.pid, started_at, baseline_reporters,
                                       "Crash detected while verifying HUD")
                    game_window = only_visible_window(windows_for(process.pid), GAME_CLASS)
                    return viewport_ready(game_window) and world_ready(read_status(status_path, run_id))

                print("World present; waiting for closed native menu and stable rendered HUD", flush=True)
                native_wait_started = time.monotonic()

                class StartupMenuHeld(Exception):
                    pass

                def native_sample(sample):
                    if time.monotonic() - native_wait_started >= 4.0 and \
                            can_confirm_native_continue(sample, read_status(status_path, run_id)):
                        raise StartupMenuHeld()

                try:
                    hud = wait_for_stable_hud(process.pid, game, min(20.0, deadline - time.monotonic()), check_world, native_sample)
                except StartupMenuHeld:
                    if args.pause_at_continue:
                        return awaiting_continue('startup-menu')
                    auto_proceed.update(read_status(status_path, run_id),time.monotonic(),startup_held=True)
                    ready_since=None
                    continue
                print(json.dumps({"ready": True, "pid": process.pid,
                                  "viewport": [game_window["width"], game_window["height"]],
                                  "runId": run_id, "stableFrames": status["stableFrames"], "autoConfirmations":auto_proceed.counts,
                                  "focusSequence": {"skipped":True} if args.no_focus else focus.report(), "stableHud": hud}),
                      flush=True)
                return 0
            ready_since = None
            if can_press_continue(status):
                mode,tick=native_loading_state(process.pid,game,resume_symbols)
                continue_ready=continue_gate.observe(mode,tick,time.monotonic())
            else:
                continue_ready=continue_gate.observe(-1,0,time.monotonic())
            if continue_ready and game_window:
                if args.pause_at_continue:
                    return awaiting_continue('loading-screen')
                auto_proceed.update(status,time.monotonic(),loading_ready=True)
        else:
            continue_gate.observe(-1,0,time.monotonic())
        time.sleep(0.10)
    raise TimeoutError(f"World not ready in {args.timeout:.0f}s; last QuickBoot phase={last_phase!r}; viewport={game_window}")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, TimeoutError, OSError) as exc:
        print(f"QuickBoot launch failed: {exc}", file=sys.stderr)
        sys.exit(1)
