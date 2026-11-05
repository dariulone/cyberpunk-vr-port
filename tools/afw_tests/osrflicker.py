"""osrflicker.py PID OUTDIR: a still scene (head still, hands out of view) shot again and again per reconstruction
(AfwTune[6] 1 OSR / 0 PureDark): how much each eye changes between shots (the eyes take turns being drawn and made,
so a made eye that differs from the drawn one flickers), and the made eye's fill share under the debug tint."""
import ctypes as c
import struct
import sys
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
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_AfwTune'])
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x1F0FFF, False, pid)
base = int(sym['CyberpunkVR_AfwTune'], 16)


def wr(index, value):
    d, n = c.c_int32(value), c.c_size_t()
    k.WriteProcessMemory(h, base + 4 * index, c.byref(d), 4, c.byref(n))


def rd():
    buf, n = c.create_string_buffer(32), c.c_size_t()
    k.ReadProcessMemory(h, base, buf, 32, c.byref(n))
    return struct.unpack('<8i', buf.raw)


def grey(path):
    im = Image.open(path).convert('L')
    im = im.resize((im.width // 4, im.height // 4))
    return np.asarray(im, dtype=np.float32)


tune0 = rd()
print('AfwTune', tune0)
s = simxr.Sim(pid=pid)
s.head(0, 0, 0)
s.hand('left', pos=(-0.25, -0.75, 0.05), ypr=(0, -80, 0))
s.hand('right', pos=(0.25, -0.75, 0.05), ypr=(0, -80, 0))
time.sleep(2.0)
try:
    for name, recon in (('osr', 1), ('afw', 0)):
        wr(6, recon)
        wr(3, 0)
        time.sleep(1.0)
        shots = {'L': [], 'R': []}
        for i in range(10):
            files = s.shot(str(out / ('%s_%d.jpg' % (name, i))), 'both', fmt='jpg', quality=92)
            for f in files:
                if f.endswith('_L.jpg'):
                    shots['L'].append(grey(f))
                elif f.endswith('_R.jpg'):
                    shots['R'].append(grey(f))
            time.sleep(0.07)
        for eye, frames in shots.items():
            d = [float(np.abs(a - b).mean()) for a, b in zip(frames, frames[1:])]
            print('%s eye %s: change between shots mean %.2f max %.2f (grey levels)' % (name, eye, np.mean(d), max(d)))
        if recon == 1:
            wr(3, 1)
            time.sleep(0.6)
            for i in range(2):
                files = s.shot(str(out / ('osr_debug_%d.jpg' % i)), 'both', fmt='jpg', quality=92)
                for f in files:
                    if f.endswith('_SBS.jpg'):
                        continue
                    a = np.asarray(Image.open(f)).astype(int)
                    mag = ((a[..., 0] > 150) & (a[..., 2] > 150) & (a[..., 1] < 110)).mean() * 100
                    grn = ((a[..., 1] > 150) & (a[..., 0] < 110) & (a[..., 2] < 110)).mean() * 100
                    print('debug %s: fill (magenta-ish) %.2f %%, history (green-ish) %.2f %%' % (Path(f).name, mag, grn))
            wr(3, 0)
finally:
    wr(6, tune0[6])
    wr(3, tune0[3])
    s.restore()
