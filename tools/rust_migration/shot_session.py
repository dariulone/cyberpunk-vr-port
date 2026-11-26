"""Both eyes as the headset shows them: a still of the composited frame from the OpenXR Simulator, so a probe can see
that a picture reaches the eyes. Writes <dir>/eyes_L.jpg, _R, _SBS at 512 px eye height (simxr Sim.shot, a simulator
build with the harness exports) or, without those exports, <dir>/eyes_mirror.bmp from the simulator's own screenshot
request (the mirror window: both eyes side by side), and prints one JSON line with the files, their sizes and, for the
mirror, each half's mean luminance and lit share. The game must be in the world and is left running."""
import json
import os
import shutil
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import Sim  # noqa: E402

SIM_DIR = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'


def bmp_halves(path):
    """Mean luminance (0..255) and the share of pixels brighter than 16, for the left and right halves of a BMP."""
    data = Path(path).read_bytes()
    offset = struct.unpack_from('<I', data, 10)[0]
    width, height = struct.unpack_from('<ii', data, 18)
    bpp = struct.unpack_from('<H', data, 28)[0]
    if bpp not in (24, 32):
        return None
    step = bpp // 8
    stride = (width * step + 3) & ~3
    rows = abs(height)
    halves = [[0.0, 0, 0], [0.0, 0, 0]]
    for y in range(0, rows, 4):
        base = offset + y * stride
        for x in range(0, width, 4):
            i = base + x * step
            b, g, r = data[i], data[i + 1], data[i + 2]
            lum = 0.2126 * r + 0.7152 * g + 0.0722 * b
            h = halves[0 if x < width // 2 else 1]
            h[0] += lum
            h[1] += lum > 16
            h[2] += 1
    return [{'mean': round(s / n, 1), 'lit': round(lit / n, 3)} for s, lit, n in halves]


def mirror_shot(out):
    requested = time.time()
    (SIM_DIR / 'screenshot_request.json').write_text(json.dumps({'eye': 'both', 'layer': 'all'}))
    end = time.monotonic() + 10
    shot, status = SIM_DIR / 'screenshot.bmp', SIM_DIR / 'screenshot_status.json'
    while time.monotonic() < end:
        if shot.exists() and status.exists() and status.stat().st_mtime >= requested:
            try:
                target = out / 'eyes_mirror.bmp'
                shutil.copy2(shot, target)
                return {'files': [str(target)], 'bytes': [target.stat().st_size], 'halves': bmp_halves(target)}
            except PermissionError:
                pass
        time.sleep(0.2)
    return {'error': 'screenshot timed out'}


out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
out.mkdir(parents=True, exist_ok=True)
try:
    sim = Sim(focus_away=False)
except RuntimeError:
    print(json.dumps(mirror_shot(out)))
    sys.exit(0)
try:
    files = sim.shot(out / 'eyes', eyes='both', eye_size=512, fmt='jpg', quality=85)
finally:
    sim.restore()
print(json.dumps({'files': files, 'bytes': [os.path.getsize(f) if os.path.exists(f) else 0 for f in files]}))
