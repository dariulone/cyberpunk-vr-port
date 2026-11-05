"""afwstate.py PID [seconds]: the AFW / mono exports read from the live plugin, the frame rates over a window (game
frames by the HUD source serial, submitted frames, the simulator's), AFW's warp counters and how MAIN's eye alternates."""
import json, struct, sys, time
from pathlib import Path
ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid = int(sys.argv[1]); secs = float(sys.argv[2]) if len(sys.argv) > 2 else 10.0
I32 = ['CyberpunkVR_MonoMode', 'CyberpunkVR_MonoView', 'CyberpunkVR_EngineStereo', 'CyberpunkVR_IpdInWorldPos',
       'CyberpunkVR_Afw', 'CyberpunkVR_MainIsRightEye', 'CyberpunkVR_RuntimeDiagnostics', 'CyberpunkVR_HudPoseDebug',
       'CyberpunkVR_HudSubmittedFrames', 'CyberpunkVR_AfwStats']
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', I32)
r = Reader(pid, GAME / 'bin/x64/Cyberpunk2077.exe', 0, 0)
def rd(n): return struct.unpack('<i', r.read(int(sym[n], 16), 4))[0] if sym.get(n) else None
def stats(): return struct.unpack('<6q', r.read(int(sym['CyberpunkVR_AfwStats'], 16), 48)) if sym.get('CyberpunkVR_AfwStats') else None
for n in I32[:6]: print(n, rd(n))
import ctypes as c
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101028, False, pid)
def wr(n, v):
    d, done = c.c_int32(v), c.c_size_t()
    k.WriteProcessMemory(h, int(sym[n], 16), c.byref(d), 4, c.byref(done))
diag0 = rd('CyberpunkVR_RuntimeDiagnostics'); wr('CyberpunkVR_RuntimeDiagnostics', 1); time.sleep(0.5)
ST = Path.home() / 'AppData/Local/OpenXR-Simulator/runtime_status.json'
def sim():
    try: return json.loads(ST.read_text())['frame_count']
    except Exception: return None
def serial(): return struct.unpack_from('<Q', r.read(int(sym['CyberpunkVR_HudPoseDebug'], 16), 96), 80)[0]
g0 = serial(); a, s0, w0, t0 = rd('CyberpunkVR_HudSubmittedFrames'), sim(), stats(), time.monotonic()
eyes = []
while time.monotonic() - t0 < secs:
    eyes.append(rd('CyberpunkVR_MainIsRightEye')); time.sleep(0.003)
g1 = serial(); b, s1, w1, t1 = rd('CyberpunkVR_HudSubmittedFrames'), sim(), stats(), time.monotonic()
dt = t1 - t0
print(f'game fps (HUD source serial) {(g1 - g0) / dt:.1f}')
print(f'submitted fps {(b - a) / dt:.1f}' if a is not None else 'no submit counter',
      f'| sim fps {(s1 - s0) / dt:.1f}' if s0 is not None and s1 is not None else '')
if w0 and w1:
    print(f'afw warped {(w1[0] - w0[0]) / dt:.1f}/s fallbacks {(w1[1] - w0[1]) / dt:.1f}/s plugin state {w1[2]} '
          f'(total warped {w1[0]} fallbacks {w1[1]})')
    print(f'sequence in the window: alternating {w1[3] - w0[3]} same eye {w1[4] - w0[4]} after a gap {w1[5] - w0[5]}')
flips = sum(1 for x, y in zip(eyes, eyes[1:]) if x != y)
print(f'main eye samples {len(eyes)}: right {sum(1 for e in eyes if e)} flips {flips} ({flips / dt:.1f}/s)')
wr('CyberpunkVR_RuntimeDiagnostics', diag0)
