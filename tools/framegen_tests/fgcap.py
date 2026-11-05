"""fgcap.py PID N: N framegen captures during a steady head yaw sweep; prints each capture's camera yaw steps."""
import sys, time, math, ctypes as c, json
sys.path.insert(0, r'C:\Users\dariulone\Desktop\CyberpunkVRPort\tools\sim_harness'); sys.path.insert(0, r'C:\Users\dariulone\Desktop\CyberpunkVRPort\tools\swimming_tests')
from simxr import Sim
from live_symbols import resolve
from pathlib import Path
pid, count = int(sys.argv[1]), int(sys.argv[2])
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'); CAP = GAME / 'red4ext/plugins/cyberpunkvrport/log/framegen-capture'
sym = resolve(pid, GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_FramegenCapture'])
k = c.WinDLL('kernel32'); k.OpenProcess.restype = c.c_void_p
k.WriteProcessMemory.argtypes = k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k.OpenProcess(0x101038, False, pid); a = int(sym['CyberpunkVR_FramegenCapture'], 16)
def rd():
    b, d = c.c_int32(), c.c_size_t(); k.ReadProcessMemory(h, a, c.byref(b), 4, c.byref(d)); return b.value
def wr(v):
    x, d = c.c_int32(v), c.c_size_t(); k.WriteProcessMemory(h, a, c.byref(x), 4, c.byref(d))
s = Sim(); t0 = time.time(); done = []; before = set(p.name for p in CAP.iterdir())
state = 'idle'; nextAsk = 1.0
while len(done) < count and time.time() - t0 < 60:
    t = time.time() - t0
    s.head(30 * (((t / 2.0) % 2) - 1 if int(t / 2.0) % 2 == 0 else 1 - ((t / 2.0) % 2)), 0, 0)   # triangle wave: constant speed
    v = rd()
    if state == 'idle' and t > nextAsk: wr(1); state = 'asked'
    elif state == 'asked' and v in (2, -1):
        new = sorted(set(p.name for p in CAP.iterdir()) - before - set(done))
        done += new; wr(0); state = 'idle'; nextAsk = t + 0.7
    time.sleep(1 / 120)
s.head(0, 0, 0)
for name in done:
    cap = json.loads((CAP / name / 'capture.json').read_text())
    yaws = [math.degrees(2 * math.atan2(fr['eyes'][0]['orientation'][1], fr['eyes'][0]['orientation'][3])) for fr in cap['frames']]
    print(name, [fr['frame'] for fr in cap['frames']], 'poses', [fr['eyes'][0].get('pose') for fr in cap['frames']], 'dt ms', [round((b['eyes'][0]['qpc']-a['eyes'][0]['qpc'])/1e4,2) for a,b in zip(cap['frames'],cap['frames'][1:])], 'yaw steps', [round(b - a, 3) for a, b in zip(yaws, yaws[1:])])
