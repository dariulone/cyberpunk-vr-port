"""simxr.py -- the OpenXR Simulator, driven from a test: full hand poses (position, rotation, roll), the head, the
buttons and axes, and captures of the COMPOSITED frame (HUD quads included) as stills or video.

USE THIS for pictures and clips of the game, not the simulator window's screenshot.bmp: that one is the mirror at
the window's size and zoom (a 1920x310 strip when the window is a strip), while a capture here is each eye at its own
resolution and aspect, composed by the simulator exactly as the runtime receives it (the runtime side: requires the
simulator build with capture.h, SimXR_HarnessVersion >= 2).

    from simxr import Sim, quat_ypr
    sim = Sim()                                   # the running game (newest Cyberpunk2077.exe) and its simulator
    sim.head(yaw=0, pitch=-20)                    # degrees; position in metres (x right, y up, z back)
    sim.hand('right', pos=(0.20, -0.30, -0.35), ypr=(0, -10, 45))      # body frame: head yaw only, roll 45 deg
    sim.hand('left', pos=(-0.2, 1.2, -0.4), quat=(0, 0, 0, 1), mode='world')
    sim.glide('right', pos=(0.1, -0.2, -0.3), ypr=(20, 0, -60), seconds=0.5)
    sim.grip('right', 1.0); sim.trigger('right', 0.0); sim.button('left', 'y', True)
    files = sim.shot(r'C:\\out\\holster', eyes='both', fmt='jpg')     # name_L / name_R / name_SBS
    sim.record_start(r'C:\\out\\clip.mp4', eyes='both', eye_size=1920, fps=30); ...; sim.record_stop()

FRAMES. OpenXR: +X right, +Y up, -Z forward; a controller points along its -Z with +Y up. `ypr` is yaw about +Y
(positive = turned left, as OpenXR's right-handed rotation), pitch about +X (positive = up), roll about the pointing
axis -Z (positive = the top turned to the right, clockwise as seen from behind). Body mode: the position is the
offset from the head turned by the head's yaw only (as the simulator always did), the rotation is in that same
yaw frame. World mode: both in the tracking space the head pose lives in.

The game must not have the foreground while a test drives the pad: focused, the simulator turns the keyboard and
mouse into controller input every frame and overwrites what is written here. `Sim()` hands the focus away.
"""
import ctypes
import ctypes.wintypes as w
import json
import math
import os
import struct
import subprocess
import time
from pathlib import Path

SIM_DIR = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'
HANDS = {'left': 0, 'right': 1, 0: 0, 1: 1}

_k = ctypes.WinDLL('kernel32', use_last_error=True)
_k.OpenProcess.restype = w.HANDLE
_k.ReadProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                 ctypes.POINTER(ctypes.c_size_t)]
_k.WriteProcessMemory.argtypes = _k.ReadProcessMemory.argtypes
_k.CreateToolhelp32Snapshot.restype = w.HANDLE


class _Module(ctypes.Structure):
    _fields_ = [('size', w.DWORD), ('mid', w.DWORD), ('pid', w.DWORD), ('gcnt', w.DWORD), ('pcnt', w.DWORD),
                ('base', ctypes.POINTER(ctypes.c_byte)), ('bytes', w.DWORD), ('hmod', w.HMODULE),
                ('name', ctypes.c_wchar * 256), ('path', ctypes.c_wchar * 260)]


def _modules(pid):
    snap = _k.CreateToolhelp32Snapshot(0x18, pid)
    m = _Module()
    m.size = ctypes.sizeof(m)
    out = []
    ok = _k.Module32FirstW(snap, ctypes.byref(m))
    while ok:
        out.append((m.name, ctypes.addressof(m.base.contents), m.path))
        ok = _k.Module32NextW(snap, ctypes.byref(m))
    _k.CloseHandle(snap)
    return out


def _exports(path):
    """name -> RVA of a DLL's exports, read off the file (the loaded image's own path)."""
    data = Path(path).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    nsec, optsz = struct.unpack_from('<H', data, pe + 6)[0], struct.unpack_from('<H', data, pe + 20)[0]
    opt = pe + 24
    exp_rva = struct.unpack_from('<I', data, opt + 112)[0]               # PE32+: data directory 0
    secs = [struct.unpack_from('<8x4I', data, opt + optsz + i * 40) for i in range(nsec)]

    def off(rva):
        for vsize, va, rawsize, raw in secs:
            if va <= rva < va + max(vsize, rawsize):
                return raw + rva - va
        raise ValueError('rva %x' % rva)

    e = off(exp_rva)
    nnames, afuncs, anames, aords = struct.unpack_from('<4I', data, e + 24)
    out = {}
    for i in range(nnames):
        name_rva = struct.unpack_from('<I', data, off(anames) + 4 * i)[0]
        s = off(name_rva)
        name = data[s:data.index(b'\0', s)].decode()
        ordinal = struct.unpack_from('<H', data, off(aords) + 2 * i)[0]
        out[name] = struct.unpack_from('<I', data, off(afuncs) + 4 * ordinal)[0]
    return out


def game_pid():
    r = subprocess.run(['powershell', '-NoProfile', '-Command',
                        "(Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue | Sort-Object StartTime | "
                        "Select-Object -Last 1).Id"], capture_output=True, text=True)
    s = r.stdout.strip()
    if not s:
        raise RuntimeError('the game is not running')
    return int(s)


def unfocus_game():
    u = ctypes.windll.user32
    hwnd = u.GetForegroundWindow()
    pid = w.DWORD()
    u.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    name = ''
    r = subprocess.run(['powershell', '-NoProfile', '-Command', '(Get-Process -Id %d).ProcessName' % pid.value],
                       capture_output=True, text=True)
    name = r.stdout.strip()
    if name.lower() != 'cyberpunk2077':
        return
    ps = ("$t = Get-Process | Where-Object { $_.MainWindowTitle -ne '' -and $_.ProcessName -match "
          "'WindowsTerminal|powershell|cmd|Code' } | Select-Object -First 1; "
          "if ($t) { [void](New-Object -ComObject WScript.Shell).AppActivate($t.Id) }")
    subprocess.run(['powershell', '-NoProfile', '-Command', ps], capture_output=True)


# ---------------------------------------------------------------- quaternions (x, y, z, w)

def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def qaxis(axis, deg):
    h = math.radians(deg) * 0.5
    s = math.sin(h)
    return (axis[0] * s, axis[1] * s, axis[2] * s, math.cos(h))


def quat_ypr(yaw=0.0, pitch=0.0, roll=0.0):
    """Degrees: yaw about +Y, then pitch about +X, then roll about the pointing axis -Z (top to the right)."""
    return qmul(qmul(qaxis((0, 1, 0), yaw), qaxis((1, 0, 0), pitch)), qaxis((0, 0, -1), roll))


def slerp(a, b, t):
    d = sum(x * y for x, y in zip(a, b))
    if d < 0:
        b, d = tuple(-x for x in b), -d
    if d > 0.9995:
        q = tuple(x + (y - x) * t for x, y in zip(a, b))
    else:
        th = math.acos(d)
        s = math.sin(th)
        q = tuple((math.sin((1 - t) * th) * x + math.sin(t * th) * y) / s for x, y in zip(a, b))
    n = math.sqrt(sum(x * x for x in q))
    return tuple(x / n for x in q)


# ---------------------------------------------------------------- the simulator

class Sim:
    MODES = {'legacy': 0, 'body': 1, 'world': 2}
    BUTTONS = {'trigger': 21, 'grip': 22, 'menu': 23, 'a': 24, 'x': 24, 'b': 25, 'y': 25, 'stick': 26}

    def __init__(self, pid=None, focus_away=True):
        self.pid = pid or game_pid()
        mod = next((m for m in _modules(self.pid) if m[0].lower() == 'openxr_simulator.dll'), None)
        if mod is None:
            raise RuntimeError('the game has no openxr_simulator.dll loaded (another runtime is active)')
        self.base, self.dll = mod[1], mod[2]
        ex = _exports(self.dll)
        if 'SimXR_Controllers' not in ex:
            raise RuntimeError('this simulator build has no harness exports (needs SimXR_HarnessVersion >= 2)')
        self.h = _k.OpenProcess(0x0438, False, self.pid)
        self.version = self._i32(self.base + ex['SimXR_HarnessVersion'])
        self.ctrl = [self._u64(self.base + ex['SimXR_Controllers'] + 8 * i) for i in range(2)]
        lay = struct.unpack('<8I', self._read(self.base + ex['SimXR_ControllerLayout'], 32))
        self.lay = dict(zip(('size', 'pos', 'yaw', 'roll', 'mode', 'orient', 'world', 'grip'), lay))
        self.saved = [self._read(c, self.lay['size']) for c in self.ctrl]
        if focus_away:
            unfocus_game()

    # memory
    def _read(self, addr, n):
        b = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t()
        if not _k.ReadProcessMemory(self.h, ctypes.c_void_p(addr), b, n, ctypes.byref(got)):
            raise OSError(ctypes.get_last_error())
        return b.raw

    def _write(self, addr, data):
        b = ctypes.create_string_buffer(bytes(data))
        got = ctypes.c_size_t()
        if not _k.WriteProcessMemory(self.h, ctypes.c_void_p(addr), b, len(data), ctypes.byref(got)):
            raise OSError(ctypes.get_last_error())

    def _i32(self, a): return struct.unpack('<i', self._read(a, 4))[0]
    def _u64(self, a): return struct.unpack('<Q', self._read(a, 8))[0]

    def restore(self):
        """The hands as they were when this Sim was made (poses, buttons, modes)."""
        for c, data in zip(self.ctrl, self.saved):
            self._write(c, data)

    # hands
    def hand(self, which, pos=None, quat=None, ypr=None, mode='body'):
        """One hand's pose. body/world: `quat` (x, y, z, w) or `ypr` degrees; legacy: `ypr` as the old yaw/pitch
        offsets plus a roll."""
        c = self.ctrl[HANDS[which]]
        m = self.MODES[mode]
        if m == 0:
            y, p, r = ypr or (0.0, 0.0, 0.0)
            if pos is not None:
                self._write(c + self.lay['pos'], struct.pack('<3f', *pos))
            self._write(c + self.lay['yaw'], struct.pack('<2f', math.radians(y), math.radians(p)))
            self._write(c + self.lay['roll'], struct.pack('<f', math.radians(r)))
        else:
            q = quat if quat is not None else quat_ypr(*(ypr or (0.0, 0.0, 0.0)))
            self._write(c + self.lay['orient'], struct.pack('<4f', *q))
            if pos is not None:
                self._write(c + (self.lay['world'] if m == 2 else self.lay['pos']), struct.pack('<3f', *pos))
        self._write(c + self.lay['mode'], struct.pack('<i', m))

    def pose(self, which):
        """(mode, pos, quat) as written: pos is the body offset (body/legacy) or the world position (world)."""
        c = self.ctrl[HANDS[which]]
        m = self._i32(c + self.lay['mode'])
        pos = struct.unpack('<3f', self._read(c + (self.lay['world'] if m == 2 else self.lay['pos']), 12))
        q = struct.unpack('<4f', self._read(c + self.lay['orient'], 16))
        return {0: 'legacy', 1: 'body', 2: 'world'}.get(m, m), pos, q

    def glide(self, which, pos=None, quat=None, ypr=None, seconds=0.4, mode=None):
        """Moves a hand there over `seconds` (position lerp, rotation slerp), from where it is now."""
        m0, p0, q0 = self.pose(which)
        mode = mode or (m0 if m0 in ('body', 'world') else 'body')
        q1 = quat if quat is not None else (quat_ypr(*ypr) if ypr is not None else q0)
        p1 = pos if pos is not None else p0
        n = max(1, int(seconds / 0.011))
        for i in range(1, n + 1):
            t = i / n
            self.hand(which, pos=tuple(a + (b - a) * t for a, b in zip(p0, p1)), quat=slerp(q0, q1, t), mode=mode)
            time.sleep(seconds / n)

    # inputs
    def button(self, which, name, on):
        self._write(self.ctrl[HANDS[which]] + self.BUTTONS[name], bytes([1 if on else 0]))

    def trigger(self, which, v):
        c = self.ctrl[HANDS[which]]
        self._write(c + 21, bytes([1 if v > 0.5 else 0]))
        self._write(c + self.lay['grip'] - 4, struct.pack('<f', v))   # triggerValue sits just before gripValue

    def grip(self, which, v):
        c = self.ctrl[HANDS[which]]
        self._write(c + 22, bytes([1 if v > 0.5 else 0]))
        self._write(c + self.lay['grip'], struct.pack('<f', v))

    def stick(self, which, x=0.0, y=0.0):
        self._write(self.ctrl[HANDS[which]] + self.lay['grip'] + 4, struct.pack('<2f', x, y))

    def tap(self, which, name, hold=0.12):
        self.button(which, name, True)
        time.sleep(hold)
        self.button(which, name, False)

    # head
    def head(self, yaw=0.0, pitch=0.0, roll=0.0, pos=(0.0, 1.7, 0.0)):
        """Degrees. Yaw positive = turned left (OpenXR); the command file's own yaw runs the other way."""
        _send('head_pose_command.json', {'x': pos[0], 'y': pos[1], 'z': pos[2], 'yaw': -math.radians(yaw),
                                         'pitch': math.radians(pitch), 'roll': math.radians(roll)})

    # captures
    def status(self):
        p = SIM_DIR / 'capture_status.json'
        try:
            return json.loads(p.read_text(encoding='utf-8'))
        except (OSError, ValueError):
            return {}

    def _request(self, req, want, timeout):
        seq0 = self.status().get('seq', -1)
        _send('capture_request.json', req)
        end = time.time() + timeout
        while time.time() < end:
            st = self.status()
            if st.get('seq', -1) != seq0 and st.get('state') in want:
                return st
            time.sleep(0.05)
        raise TimeoutError('the simulator did not answer %s' % req.get('type'))

    def shot(self, path, eyes='both', eye_size=0, fmt='jpg', quality=95, timeout=10):
        """A still of the composited frame per eye at its own resolution (eye_size 0) or at that eye height.
        Returns the files written: name_L / name_R / name_SBS for both eyes, one file for one eye."""
        st = self._request({'type': 'shot', 'path': str(path), 'eyes': eyes, 'eyeSize': eye_size,
                            'format': fmt, 'quality': quality}, ('shot_done',), timeout)
        if st.get('error'):
            raise RuntimeError(st['error'])
        return st.get('files', [])

    def record_start(self, path, eyes='both', eye_size=1920, fps=30, codec='hevc', mbps=0, max_seconds=0):
        """A hardware-encoded MP4 of the composited frames: HEVC in a plain MP4 (it needs its record_stop, a session
        end or max_seconds to be playable), H.264 in a fragmented MP4 when HEVC cannot take the size. Sizes are
        rounded down to multiples of 16."""
        st = self._request({'type': 'record_start', 'path': str(path), 'eyes': eyes, 'eyeSize': eye_size,
                            'fps': fps, 'codec': codec, 'mbps': mbps, 'maxSeconds': max_seconds},
                           ('recording',), 10)
        return st

    def record_stop(self, timeout=20):
        st = self._request({'type': 'record_stop'}, ('stopped',), timeout)
        if st.get('error'):
            raise RuntimeError(st['error'])
        return st


def _send(name, value):
    """A command file, written whole (a temp file renamed over it). The simulator may be reading the previous one at
    that moment -- the rename then fails with a sharing violation -- so it is retried for a while."""
    p = SIM_DIR / name
    tmp = p.with_suffix('.tmp')
    tmp.write_text(json.dumps(value), encoding='utf-8')
    for _ in range(200):
        try:
            os.replace(tmp, p)
            return
        except PermissionError:
            time.sleep(0.002)
    raise PermissionError('the simulator holds %s' % p)


if __name__ == '__main__':
    import sys
    s = Sim(focus_away=False)
    print('simulator', s.dll, 'harness v%d' % s.version, 'controllers', [hex(c) for c in s.ctrl], s.lay)
    if sys.argv[1:2] == ['shot']:
        print(s.shot(sys.argv[2], *(sys.argv[3:4] or ['both'])))
