"""The pause menu in the game: Esc is tapped in the game's own window (SendInput, the foreground checked first), so the
VR menu path runs -- the menu panel's quad and its lazy follow (rust/crates/cvr-xr/src/menu.rs) -- for a few seconds;
then Esc again to close it. Reports the XR projection cycles and empty ends counted over the open menu, so a menu that
stops the submission shows. Prints one JSON line. The game must be in the world and is left running."""
import ctypes as c
import ctypes.wintypes as w
import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import _exports, _modules, game_pid  # noqa: E402
from call_in_game import rva  # noqa: E402

pid = game_pid()
state = json.loads(subprocess.check_output([sys.executable, '-B', str(Path(r'X:\Dev\CyberpunkVRPort\tools\quickboot_tests\windows.py')),
                                            '--pid', str(pid)], text=True))
rows = [r for r in state['windows'] if r['class'] == 'W2ViewportClass' and r['visible'] and not r['minimized']]
u = c.WinDLL('user32', use_last_error=True)
k = c.WinDLL('kernel32', use_last_error=True)
u.GetForegroundWindow.restype = w.HWND
u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
u.SetForegroundWindow.argtypes = [w.HWND]
u.SetForegroundWindow.restype = w.BOOL
k.OpenProcess.restype = c.c_void_p
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]


class Keyboard(c.Structure):
    _fields_ = [('vk', w.WORD), ('scan', w.WORD), ('flags', w.DWORD), ('time', w.DWORD), ('extra', c.c_size_t)]


class Payload(c.Union):
    _fields_ = [('key', Keyboard), ('padding', c.c_byte * 32)]


class Input(c.Structure):
    _fields_ = [('type', w.DWORD), ('payload', Payload)]


u.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
u.SendInput.restype = w.UINT
port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
ex = _exports(port[2])
handle = k.OpenProcess(0x0010, False, pid)


def counter(name):
    buf = c.c_uint64()
    got = c.c_size_t()
    k.ReadProcessMemory(handle, c.c_void_p(port[1] + ex[name]), c.byref(buf), 8, c.byref(got))
    return buf.value


def menu_mode():
    """g_menuModeValue, found by its PDB name (it is not exported)."""
    offset = rva(port[2], 'g_menuModeValue')
    if offset is None:
        return None
    buf = c.c_int32()
    got = c.c_size_t()
    k.ReadProcessMemory(handle, c.c_void_p(port[1] + offset), c.byref(buf), 4, c.byref(got))
    return buf.value


def tap_escape():
    event = Input()
    event.type = 1
    event.payload.key = Keyboard(0, 0x01, 8, 0, 0)  # scan code, KEYEVENTF_SCANCODE
    assert u.SendInput(1, c.byref(event), c.sizeof(Input)) == 1
    time.sleep(0.10)
    event.payload.key.flags = 10  # | KEYEVENTF_KEYUP
    assert u.SendInput(1, c.byref(event), c.sizeof(Input)) == 1


result = {'window': len(rows) == 1}
if rows:
    hwnd = int(rows[0]['handle'], 16)
    u.SetForegroundWindow(hwnd)
    time.sleep(0.25)
    owner = w.DWORD()
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), c.byref(owner))
    result['foreground'] = owner.value == pid
    if result['foreground']:
        before = (counter('CyberpunkVR_XrProjectionCycles'), counter('CyberpunkVR_XrEmptyEnds'))
        result['menu_mode_before'] = menu_mode()
        tap_escape()
        time.sleep(4.0)
        result['menu_mode_open'] = menu_mode()
        during = (counter('CyberpunkVR_XrProjectionCycles'), counter('CyberpunkVR_XrEmptyEnds'))
        tap_escape()
        time.sleep(2.0)
        result['menu_mode_after'] = menu_mode()
        result.update(projection_cycles_in_menu=during[0] - before[0], empty_ends_in_menu=during[1] - before[1])
print(json.dumps(result))
