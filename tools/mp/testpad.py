"""The port's bounded test pad, written into the running game: buttons, the left stick and the triggers, each held for
a few seconds at most (the port lets an expired value go). Works with the game window in front, where the game polls
the pad (with the simulator's window in front the game ignores XInput): `front()` puts the game there.

    from testpad import Pad
    pad = Pad(); pad.front()
    pad.triggers(right=1.0, seconds=0.15)      # a shot
    pad.buttons(0x4000, seconds=0.2)           # X (the reload in the port's bindings)
    pad.stick(x=-0.8, y=0.0, seconds=2.0)      # the left stick
"""
import ctypes
import ctypes.wintypes as w
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'live'))
import probe  # noqa: E402

_u = ctypes.WinDLL('user32', use_last_error=True)
_k = ctypes.WinDLL('kernel32', use_last_error=True)
_k.GetTickCount.restype = w.DWORD

XINPUT = dict(up=0x1, down=0x2, left=0x4, right=0x8, start=0x10, back=0x20, ls=0x40, rs=0x80, lb=0x100, rb=0x200,
              a=0x1000, b=0x2000, x=0x4000, y=0x8000, lgrip=0x10000, rgrip=0x20000)


class Pad:
    def __init__(self, pid=None):
        self.pid = pid or probe.find_pid()
        self.t = probe.Target(self.pid, writable=True)
        base = self.t.module_base('cyberpunkvrport.dll')
        if base is None:
            raise RuntimeError('the port is not loaded')
        self.ex = probe.exports(self.t, base)

    def _put(self, name, data):
        if name not in self.ex:
            raise RuntimeError(f'{name} is not exported (an older build?)')
        self.t.write(self.ex[name], data)

    def _deadline(self, seconds):
        return (_k.GetTickCount() + int(seconds * 1000)) & 0xFFFFFFFF

    def buttons(self, mask, seconds=0.2):
        self._put('CyberpunkVR_TestPadButtons', struct.pack('<i', mask))
        time.sleep(seconds)
        self._put('CyberpunkVR_TestPadButtons', struct.pack('<i', 0))

    def stick(self, x=0.0, y=0.0, seconds=1.0):
        xi, yi = int(max(-1, min(1, x)) * 32767) & 0xFFFF, int(max(-1, min(1, y)) * 32767) & 0xFFFF
        self._put('CyberpunkVR_TestPadLeft', struct.pack('<Q', (self._deadline(seconds) << 32) | (yi << 16) | xi))

    def triggers(self, left=0.0, right=0.0, seconds=0.2):
        lv, rv = int(max(0, min(1, left)) * 255), int(max(0, min(1, right)) * 255)
        self._put('CyberpunkVR_TestPadTriggers', struct.pack('<Q', (self._deadline(seconds) << 32) | (rv << 8) | lv))

    def front(self):
        """The game's window in front (a foreground change the system allows: an Alt tap first)."""
        found = []

        @ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        def each(hwnd, _):
            pid = w.DWORD()
            _u.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == self.pid and _u.IsWindowVisible(hwnd) and _u.GetWindowTextLengthW(hwnd) > 0:
                found.append(hwnd)
            return True
        _u.EnumWindows(each, 0)
        if not found:
            return False
        _u.keybd_event(0x12, 0, 0, 0)
        _u.keybd_event(0x12, 0, 2, 0)
        ok = bool(_u.SetForegroundWindow(found[0]))
        time.sleep(0.5)
        return ok
