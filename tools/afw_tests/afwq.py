"""afwq.py PID OUTDIR NAME=tune6,tune7,tune0 ...: per configuration (CyberpunkVR_AfwTune [6] reconstruction, [7] OSR
jitter, [0] mode), 12 PNG stills with the head still; with the scene still every real frame of an eye is the same
picture, so each eye's stills differ only where a made frame differs from a real one: per eye, the PSNR of every
still against the eye's per-pixel median (worst and mean), and the port's GPU time of the second eye (CaptureSecond)."""
import ctypes as c, json, struct, subprocess, sys, time
from pathlib import Path
import numpy as np
from PIL import Image
ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/sim_harness')]
from live_symbols import resolve
from simxr import Sim
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid, out = int(sys.argv[1]), Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_AfwTune', 'CyberpunkVR_AfwStats', 'CyberpunkVR_OsrStats'])
k = c.WinDLL('kernel32'); k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101038, False, pid)
def rd(n, size):
    b, d = c.create_string_buffer(size), c.c_size_t(); k.ReadProcessMemory(h, int(sym[n], 16), b, size, c.byref(d)); return b.raw
def wr(n, data):
    b, d = c.create_string_buffer(data), c.c_size_t(); k.WriteProcessMemory(h, int(sym[n], 16), b, len(data), c.byref(d))
s = Sim(); s.head(0, 0, 0)
base = list(struct.unpack('<8i', rd('CyberpunkVR_AfwTune', 32)))
def psnr(a, b):
    m = np.mean((a.astype(np.float32) - b.astype(np.float32)) ** 2); return 99.0 if m <= 0 else 10 * np.log10(255 ** 2 / m)
for arg in sys.argv[3:]:
    name, vals = arg.split('='); v = [int(x) for x in vals.split(',')]
    t = list(base); t[6], t[7], t[0] = v[0], v[1], v[2]
    wr('CyberpunkVR_AfwTune', struct.pack('<8i', *t)); time.sleep(1.5)
    w0 = struct.unpack('<6q', rd('CyberpunkVR_AfwStats', 48)); o0 = struct.unpack('<3q', rd('CyberpunkVR_OsrStats', 24))
    files = []
    for i in range(12):
        for attempt in range(4):
            try:
                files.append(s.shot(str(out / f'{name}_{i:02d}'), eyes='both', eye_size=1400, fmt='jpg', quality=98, timeout=20)); break
            except TimeoutError:
                time.sleep(1.0)
    w1 = struct.unpack('<6q', rd('CyberpunkVR_AfwStats', 48)); o1 = struct.unpack('<3q', rd('CyberpunkVR_OsrStats', 24))
    res = []
    for e in (0, 1):
        ims = [np.asarray(Image.open(f[e]).convert('RGB')) for f in files]
        med = np.median(np.stack(ims), axis=0)
        ps = [psnr(im, med) for im in ims]
        res.append((min(ps), float(np.mean(ps))))
    print(f'{name}: tune6 {t[6]} tune7 {t[7]} mode {t[0]} | L worst {res[0][0]:.2f} mean {res[0][1]:.2f} dB | R worst {res[1][0]:.2f} mean {res[1][1]:.2f} dB '
          f'| warped {w1[0]-w0[0]} fallbacks {w1[1]-w0[1]} osr {o1[0]-o0[0]} hist {o1[2]-o0[2]} osr-failed {o1[1]-o0[1]}')
wr('CyberpunkVR_AfwTune', struct.pack('<8i', *base))
