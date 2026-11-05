"""osrmotion.py PID OUTDIR: head turning left-right (+-25 deg, ~50 deg/s), hands out of view: both eyes shot with and
without the debug tint; the made eye's fill / history share while the head moves."""
import ctypes as c
import math
import struct
import sys
import threading
import time
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/sim_harness')]
from live_symbols import resolve  # noqa: E402
import simxr  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid = int(sys.argv[1])
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
base = int(resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_AfwTune'])['CyberpunkVR_AfwTune'], 16)
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x1F0FFF, False, pid)


def wr(index, value):
    d, n = c.c_int32(value), c.c_size_t()
    k.WriteProcessMemory(h, base + 4 * index, c.byref(d), 4, c.byref(n))


s = simxr.Sim(pid=pid)
s.hand('left', pos=(-0.25, -0.75, 0.05), ypr=(0, -80, 0))
s.hand('right', pos=(0.25, -0.75, 0.05), ypr=(0, -80, 0))
stop = threading.Event()


def sweep():
    t0 = time.monotonic()
    while not stop.is_set():
        t = time.monotonic() - t0
        s.head(25 * math.sin(t * 2.0), 0, 0)
        time.sleep(0.011)


th = threading.Thread(target=sweep, daemon=True)
th.start()
try:
    time.sleep(1.0)
    for debug in (0, 1):
        wr(3, debug)
        time.sleep(0.5)
        for i in range(4):
            files = s.shot(str(out / ('turn%s_%d.jpg' % ('_debug' if debug else '', i))), 'both', fmt='jpg', quality=92)
            if debug:
                for f in files:
                    if f.endswith('_SBS.jpg'):
                        continue
                    a = np.asarray(Image.open(f)).astype(int)
                    mag = ((a[..., 0] > 150) & (a[..., 2] > 150) & (a[..., 1] < 110)).mean() * 100
                    grn = ((a[..., 1] > 150) & (a[..., 0] < 110) & (a[..., 2] < 110)).mean() * 100
                    print('%s: fill %.2f %%, history %.2f %%' % (Path(f).name, mag, grn))
            time.sleep(0.2)
finally:
    stop.set()
    th.join(timeout=2)
    wr(3, 0)
    s.head(0, 0, 0)
    s.restore()
