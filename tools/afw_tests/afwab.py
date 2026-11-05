"""afwab.py PID OUTDIR NAME=mode,mvtype,threshold100,debug,clear,mvoff[,sx,sy] ...
For each configuration: write CyberpunkVR_AfwTune / AfwMotionScale into the running game, record the same head yaw sweep
(simulator capture, both eyes), pull frames at the sweep's fastest moments and put them on one sheet per configuration."""
import ctypes as c, math, struct, subprocess, sys, time
from pathlib import Path
ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/sim_harness')]
from live_symbols import resolve
from simxr import Sim
from PIL import Image, ImageDraw
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
VFRAME = ROOT / 'tools/sim_harness/vframe.exe'
pid, out = int(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
              ['CyberpunkVR_AfwTune', 'CyberpunkVR_AfwMotionScale', 'CyberpunkVR_AfwStats'])
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101038, False, pid)
def write(name, data):
    buf, done = c.create_string_buffer(data), c.c_size_t()
    assert k.WriteProcessMemory(h, int(sym[name], 16), buf, len(data), c.byref(done)), c.get_last_error()
def read(name, n):
    buf, done = c.create_string_buffer(n), c.c_size_t()
    k.ReadProcessMemory(h, int(sym[name], 16), buf, n, c.byref(done))
    return buf.raw
s = Sim()
import os
TIMES = tuple(float(x) for x in os.environ.get('AFW_TIMES', '0.05,1.25,2.5').split(','))
for arg in sys.argv[3:]:
    name, values = arg.split('=', 1)
    v = [float(x) for x in values.split(',')]
    # 6 or 8 ints (mode, mvtype, threshold, debug, clear, mvoff[, reconstruction, osr jitter]); scale stays 1
    tune = ([int(x) for x in v[:8]] + [0, 0])[:8]
    scale = [1.0, 1.0]
    write('CyberpunkVR_AfwTune', struct.pack('<8i', *tune))
    write('CyberpunkVR_AfwMotionScale', struct.pack('<2f', *scale))
    s.head(0, 0, 0)
    time.sleep(1.0)
    w0 = struct.unpack('<3q', read('CyberpunkVR_AfwStats', 24))
    clip = out / f'{name}.mp4'
    s.record_start(str(clip), eyes='both', eye_size=1400, fps=30, codec='hevc', max_seconds=8)
    t0 = time.time()
    while (t := time.time() - t0) < 3.2:
        s.head(35 * math.sin(2 * math.pi * t / 2.5), 0, 0)
        time.sleep(1 / 120)
    s.head(0, 0, 0)
    time.sleep(0.3)
    s.record_stop()
    w1 = struct.unpack('<3q', read('CyberpunkVR_AfwStats', 24))
    frames = []
    for t in TIMES:
        f = out / f'{name}_{t}.jpg'
        subprocess.run([str(VFRAME), str(clip), str(f), str(t)], capture_output=True)
        if f.exists(): frames.append(Image.open(f))
    if not frames:
        print(name, 'no frames'); continue
    w, hgt = frames[0].size
    sc = 0.5
    sheet = Image.new('RGB', (int(w * sc), (int(hgt * sc) + 8) * len(frames) + 40), (20, 20, 20))
    d = ImageDraw.Draw(sheet)
    d.text((10, 10), f'{name}: tune {tune[:6]} scale {scale}  warped {w1[0] - w0[0]} fallbacks {w1[1] - w0[1]}', fill=(255, 255, 0))
    for i, im in enumerate(frames):
        sheet.paste(im.resize((int(w * sc), int(hgt * sc))), (0, 40 + i * (int(hgt * sc) + 8)))
    sheet.save(out / f'{name}_sheet.jpg', quality=90)
    print(name, tune[:6], scale, 'warped', w1[0] - w0[0], 'fallbacks', w1[1] - w0[1], '->', out / f'{name}_sheet.jpg')
write('CyberpunkVR_AfwTune', struct.pack('<8i', 3, 3, 250, 0, 0, 0, 1, 1))
write('CyberpunkVR_AfwMotionScale', struct.pack('<2f', 1.0, 1.0))
