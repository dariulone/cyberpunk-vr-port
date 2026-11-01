# rigdump.py PID OUT: the plugin's reference pose of both hands' subtrees (bone, parent, local pos, local rot),
# the ladder finger slots and the hand/slot bones, read from the live process through the PDB.
import ctypes as c, ctypes.wintypes as w, struct, sys
from pathlib import Path
sys.path.insert(0, r'C:\Users\dariulone\Desktop\CyberpunkVRPort\tools\swimming_tests')
from live_symbols import resolve
DLL = Path(r'C:\Users\dariulone\Desktop\CyberpunkVRPort\build\patchday\sdk25-build\bin\red4ext\plugins\cyberpunkvrport\Release\cyberpunkvrport.dll')
pid, out = int(sys.argv[1]), sys.argv[2]
MAX = 800
r = resolve(pid, DLL, ['*s_referenceLocal*', '*s_referenceValid*', '*s_ladderFingers*', 'g_VRBoneParent', 'g_VRBoneCount',
                       'g_VRRightBoneIdx', 'g_VRLeftBoneIdx', 'g_VRSmokeCigIdx', 'g_VRSmokeLighterIdx', '*g_VRBoneNames*'])
for k, v in r.items(): print(k, v)
def addr(part):
    for k, v in r.items():
        if part in k: return int(v, 16)
    raise KeyError(part)
k32 = c.WinDLL('kernel32', use_last_error=True)
k32.OpenProcess.restype = w.HANDLE
k32.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
h = k32.OpenProcess(0x0010 | 0x0400, False, pid)
def rd(a, n):
    b = c.create_string_buffer(n); got = c.c_size_t()
    if not k32.ReadProcessMemory(h, a, b, n, c.byref(got)): raise OSError('read %x' % a)
    return b.raw
cnt = struct.unpack('<i', rd(addr('g_VRBoneCount'), 4))[0]
par = struct.unpack('<%dh' % MAX, rd(addr('g_VRBoneParent'), 2 * MAX))
valid = rd(addr('s_referenceValid'), MAX)
lpos = struct.unpack('<%df' % (3 * MAX), rd(addr('s_referenceLocalPos'), 12 * MAX))
lrot = struct.unpack('<%df' % (4 * MAX), rd(addr('s_referenceLocalRot'), 16 * MAX))
fing = struct.unpack('<38i', rd(addr('s_ladderFingers'), 38 * 4))
idx = {n: struct.unpack('<i', rd(addr(n), 4))[0] for n in ('g_VRRightBoneIdx', 'g_VRLeftBoneIdx', 'g_VRSmokeCigIdx', 'g_VRSmokeLighterIdx')}
lines = ['count %d' % cnt]
for n, v in idx.items(): lines.append('%s %d' % (n, v))
for side in range(2): lines.append('fingers %d %s' % (side, ' '.join(str(x) for x in fing[side * 19:(side + 1) * 19])))
for side, hand in ((0, idx['g_VRLeftBoneIdx']), (1, idx['g_VRRightBoneIdx'])):
    for b in range(cnt):
        a, ok = b, False
        for _ in range(64):
            if a == hand: ok = True; break
            if a < 0: break
            a = par[a]
        if ok or b == hand or b in (hand,):
            lines.append('bone %d %d %d %s %s' % (b, par[b], valid[b], ' '.join('%.7f' % x for x in lpos[3 * b:3 * b + 3]),
                                                   ' '.join('%.7f' % x for x in lrot[4 * b:4 * b + 4])))
Path(out).write_text('\n'.join(lines) + '\n')
print(len(lines), 'lines')
