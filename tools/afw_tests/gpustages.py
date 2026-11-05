"""gpustages.py PID [frames]: the port's own GPU passes (src/Render/GpuStageProfile.cpp) in a running game, mean/max us."""
import ctypes as c, json, struct, sys, time
from pathlib import Path
ROOT = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort')
sys.path[:0] = [str(ROOT / 'tools/swimming_tests')]
from live_symbols import resolve
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
pid = int(sys.argv[1]); frames = int(sys.argv[2]) if len(sys.argv) > 2 else 32
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
              ['CyberpunkVR_GpuStageRequest', 'CyberpunkVR_GpuStageReport', 'CyberpunkVR_GpuStageReportSeq'])
k = c.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101038, False, pid)
def read(name, n):
    buf, done = c.create_string_buffer(n), c.c_size_t()
    k.ReadProcessMemory(h, int(sym[name], 16), buf, n, c.byref(done))
    return buf.raw
val, done = c.c_uint32(min(frames, 32)), c.c_size_t()
k.WriteProcessMemory(h, int(sym['CyberpunkVR_GpuStageRequest'], 16), c.byref(val), 4, c.byref(done))
report = {}
end = time.monotonic() + 20
while time.monotonic() < end:
    time.sleep(0.5)
    raw = read('CyberpunkVR_GpuStageReport', 2048).split(b'\0', 1)[0]
    try: report = json.loads(raw.decode('utf-8', 'replace'))
    except ValueError: continue
    if not report.get('pending') and report.get('batches'):
        break
for s in sorted(report.get('stages', []), key=lambda s: -s.get('mean_us', 0)):
    if s.get('count'):
        print(f"{s['name']:16s} n={s['count']:3d} mean {s['mean_us']:8.1f} us  max {s['max_us']:8.1f} us")
print({k2: v for k2, v in report.items() if k2 != 'stages'})
