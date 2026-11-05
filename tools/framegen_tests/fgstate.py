"""fgstate.py PID [samples] [interval]: the framegen report (src/Framegen/Framegen.cpp, JSON) and the input counters
read from the running game, a few times; diagnostics on while it reads."""
import ctypes as c, json, struct, sys, time
from pathlib import Path
ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/swimming_tests')]
from live_symbols import resolve
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid = int(sys.argv[1]); samples = int(sys.argv[2]) if len(sys.argv) > 2 else 4; gap = float(sys.argv[3]) if len(sys.argv) > 3 else 2.0
names = ['CyberpunkVR_FramegenReport', 'CyberpunkVR_FramegenReportSeq', 'CyberpunkVR_RuntimeDiagnostics',
         'CyberpunkVR_FramegenInputStages', 'CyberpunkVR_FramegenCameraDelta', 'CyberpunkVR_FramegenNativeStages']
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', names)
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101038, False, pid)
def read(n, size):
    buf, done = c.create_string_buffer(size), c.c_size_t()
    k.ReadProcessMemory(h, int(sym[n], 16), buf, size, c.byref(done)); return buf.raw
def wr32(n, v):
    d, done = c.c_int32(v), c.c_size_t(); k.WriteProcessMemory(h, int(sym[n], 16), c.byref(d), 4, c.byref(done))
diag0 = struct.unpack('<i', read('CyberpunkVR_RuntimeDiagnostics', 4))[0]
wr32('CyberpunkVR_RuntimeDiagnostics', 1)
time.sleep(1.0)
st0 = struct.unpack('<10Q', read('CyberpunkVR_FramegenInputStages', 80))
for i in range(samples):
    time.sleep(gap)
    raw = read('CyberpunkVR_FramegenReport', 2048).split(b'\0', 1)[0].decode('utf-8', 'replace')
    try:
        r = json.loads(raw)
        print({kk: r[kk] for kk in ('enabled', 'active', 'backend', 'real_fps', 'fg_fps', 'output_fps', 'repeats_fps', 'cpu_ms', 'gpu_ms',
                                     'generation_ms', 'generation_span_ms', 'pace_ms', 'real', 'generated', 'repeated', 'skipped', 'inputs', 'low1', 'gpu_usage', 'status') if kk in r})
    except ValueError:
        print('report not JSON:', raw[:200])
st1 = struct.unpack('<10Q', read('CyberpunkVR_FramegenInputStages', 80))
print('input stages delta:', [b - a for a, b in zip(st0, st1)])
print('camera delta:', [round(x, 4) for x in struct.unpack('<12f', read('CyberpunkVR_FramegenCameraDelta', 48))])
print('native stages:', struct.unpack('<4Q', read('CyberpunkVR_FramegenNativeStages', 32)))
wr32('CyberpunkVR_RuntimeDiagnostics', diag0)
