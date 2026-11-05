"""osrhands.py PID OUTDIR: hands moving in front of the eyes under AFW/OSR; both eyes shot per configuration
(each: name, OsrTune[2] near limit cm, OsrTune[3] edge distance px x100, AfwTune[3] debug tint).
    py -3.13 -B tools/afw_tests/osrhands.py PID OUTDIR  (an OSR or AFW launch, simulator, hands in view)"""
import ctypes as c
import struct
import sys
import threading
import time
from pathlib import Path

ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/sim_harness')]
from live_symbols import resolve  # noqa: E402
import simxr  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid = int(sys.argv[1])
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
              ['CyberpunkVR_OsrTune', 'CyberpunkVR_AfwTune', 'CyberpunkVR_OsrStats'])
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x1F0FFF, False, pid)


def wr(name, index, value):
    d, n = c.c_int32(value), c.c_size_t()
    k.WriteProcessMemory(h, int(sym[name], 16) + 4 * index, c.byref(d), 4, c.byref(n))


def rd(name, count, fmt='i', size=4):
    buf, n = c.create_string_buffer(count * size), c.c_size_t()
    k.ReadProcessMemory(h, int(sym[name], 16), buf, count * size, c.byref(n))
    return struct.unpack('<%d%s' % (count, fmt), buf.raw)


print('OsrTune', rd('CyberpunkVR_OsrTune', 4), 'AfwTune', rd('CyberpunkVR_AfwTune', 8))
s = simxr.Sim(pid=pid)
print('poses', s.pose('left'), s.pose('right'))
s.head(0, -10, 0)
L0, R0 = (-0.16, -0.28, -0.34), (0.16, -0.28, -0.34)
s.hand('left', pos=L0, ypr=(0, 20, 0))
s.hand('right', pos=R0, ypr=(0, 20, 0))
time.sleep(1.5)

stop = threading.Event()


def sway():
    # both hands sideways and back, 0.22 m in 0.18 s: fast enough that one frame moves them a few centimetres
    while not stop.is_set():
        for dx in (0.11, -0.11):
            s.glide('left', pos=(L0[0] + dx, L0[1], L0[2]), seconds=0.18)
            s.glide('right', pos=(R0[0] + dx, R0[1], R0[2]), seconds=0.18)


configs = [('osr', 100, 75, 0), ('osr_debug', 100, 75, 1), ('no_edge', 100, 0, 0), ('no_limit', 0, 75, 0)]
debug0 = rd('CyberpunkVR_AfwTune', 8)[3]
tune0 = rd('CyberpunkVR_OsrTune', 4)
t = threading.Thread(target=sway, daemon=True)
t.start()
try:
    for name, near, edge, debug in configs:
        wr('CyberpunkVR_OsrTune', 2, near)
        wr('CyberpunkVR_OsrTune', 3, edge)
        wr('CyberpunkVR_AfwTune', 3, debug)
        time.sleep(0.6)
        for i in range(3):
            print(name, i, s.shot(str(out / ('%s_%d.jpg' % (name, i))), 'both', fmt='jpg', quality=92))
            time.sleep(0.15)
finally:
    stop.set()
    t.join(timeout=2)
    wr('CyberpunkVR_OsrTune', 2, tune0[2])
    wr('CyberpunkVR_OsrTune', 3, tune0[3])
    wr('CyberpunkVR_AfwTune', 3, debug0)
    s.restore()
    s.head(0, 0, 0)
print('OsrStats', rd('CyberpunkVR_OsrStats', 3, 'q', 8) if sym.get('CyberpunkVR_OsrStats') else None)
