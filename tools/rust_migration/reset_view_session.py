"""The OpenXR Simulator's Reset View in the game: Home is posted to the simulator's window, which the port's bridge
(rust/crates/cvr-xr/src/simulator_recenter.rs, formerly src/Runtimes/SimulatorRecenter.cpp) brackets as an external
pose reset, so the recenter base is captured again. Counts the port log's "Base pose captured" lines before and after.
Prints one JSON line. The game must be in the world (tools/quickboot_tests/launch_to_save.py) and is left running."""
import ctypes
import ctypes.wintypes as wt
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import game_pid  # noqa: E402

LOG = Path(sys.argv[1]) if len(sys.argv) > 1 else None
u = ctypes.WinDLL('user32', use_last_error=True)
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
WM_KEYDOWN, WM_KEYUP, VK_HOME = 0x0100, 0x0101, 0x24


def captures():
    if not LOG or not LOG.exists():
        return -1
    return LOG.read_text(encoding='utf-8', errors='replace').count('Base pose captured')


def simulator_window(pid):
    found = []

    def visit(hwnd, _):
        owner = wt.DWORD()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid:
            return True
        title = ctypes.create_unicode_buffer(256)
        u.GetWindowTextW(hwnd, title, 256)
        if 'OpenXR Simulator' in title.value:
            found.append(hwnd)
            return False
        return True

    u.EnumWindows(WNDENUMPROC(visit), 0)
    return found[0] if found else None


pid = game_pid()
window = simulator_window(pid)
before = captures()
result = {'window': bool(window), 'before': before}
if window:
    u.PostMessageW(window, WM_KEYDOWN, VK_HOME, 0)
    u.PostMessageW(window, WM_KEYUP, VK_HOME, 0)
    time.sleep(2.0)
    after = captures()
    result.update(after=after, recentered=after > before)
print(json.dumps(result))
