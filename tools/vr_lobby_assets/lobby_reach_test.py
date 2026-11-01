"""The hands' reach in the lobby, through the simulator's right controller: the ring on an item as the palm comes near
(faint, then full once in reach), the ring on one pointed at from a few metres, the brackets as the grip holds it from
afar, and the pull as the hand is snatched back -- printing the marker's state (CyberpunkVRPort_Reach) and the lobby's reach state at
each step, with a headset shot of each in OUTDIR.

    py -3.13 tools/vr_lobby_assets/lobby_reach_test.py OUTDIR [ITEM_NAME]      (default: item cube)

The lobby must be up (the intro over). The items are put back first; the one tested is moved onto the table's front,
within an arm's reach of the player stood before the table, and later pointed at from further back.
"""
import ctypes
import json
import math
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_items_test as T  # noqa: E402  (its own imports put the harness on the path)
import game_views as G  # noqa: E402
from common import lua  # noqa: E402
from live_symbols import loaded_module  # noqa: E402
from simulator_input import PadGate  # noqa: E402
from lobby_parkour_test import PUT  # noqa: E402

# The marker of the right hand, and the lobby's reach state of that hand.
STATE = '''local m = GetMod("CyberpunkVRPort_Reach").state()[2]
local r = GetMod("CyberpunkVRPort_Lobby").items().state().reach[2]
local st = GetMod("CyberpunkVRPort_Lobby").items().state()
print(string.format('{"kind": "%s", "shown": %.2f, "morph": %.2f, "ring": %s, "aim": "%s", "lock": "%s", '
  .. '"pull": "%s", "snatch": %.2f, "right": "%s", "pulls": %d}', tostring(m.kind), m.shown, m.morph,
  tostring(m.ring or 0), tostring(r.aim), tostring(r.lock), tostring(r.pull), r.snatch or 0, tostring(st.right),
  st.pulls))'''
# The right hand's pointer ray and an item's centre, in the world.
RAY = '''local I = GetMod("CyberpunkVRPort_Lobby").items(); local r = I.ray(2); local e = I.entity("%s")
local p = e and e:GetWorldPosition()
if r == nil or p == nil then print("null") else
print(string.format('[%%.4f, %%.4f, %%.4f, %%.4f, %%.4f, %%.4f, %%.4f, %%.4f, %%.4f]', r.o[1], r.o[2], r.o[3], r.d[1],
  r.d[2], r.d[3], p.x, p.y, p.z)) end'''
# The rendered view's position, lobby frame.
VIEW = T.FRAME + '''local p = VRViewWorldPos(); local x, y, z = lf(p.x, p.y, p.z)
print(string.format('[%.3f, %.3f, %.3f]', x, y, z))'''
# An item put down at a point of the lobby frame (its body teleported while dynamic, at rest).
MOVE = T.FRAME + '''local I = L.items(); local e = I.entity("%s"); local c = e:FindComponentByName(CName.new("item"))
local b = c:CreatePhysicalBodyInterface(0); local yaw = math.rad(a.yaw); local cs, sn = math.cos(yaw), math.sin(yaw)
local x, y, z = %f, %f, %f
VRBodySetPos(b, Vector4.new(a.pos.x + cs * x - sn * y, a.pos.y + sn * x + cs * y, a.pos.z + z, 1), e:GetWorldOrientation())
VRBodySetVel(b, Vector4.new(0, 0, 0, 0)); VRBodySetAngVel(b, Vector4.new(0, 0, 0, 0)); print("ok")'''

# The right controller as the lobby reads it (modules/hands.lua `reach`: VRHandRawModel through the player), lobby
# frame: where a hand reaching past the arm's length takes things from, the drawn palm stopping short of it.
CONTROLLER = T.FRAME + '''local r = L.items().hand(2).reach
if r == nil then print("null") else local x, y, z = lf(r[1], r[2], r[3])
print(string.format('[%.4f, %.4f, %.4f]', x, y, z)) end'''


def controller_at():
    return json.loads(lua(CONTROLLER).strip())


def state():
    return json.loads(lua(STATE).strip())


def step(out, label):
    s = state()
    print('%-10s' % label, s, flush=True)
    G.shot().save(out / ('reach_%s.png' % label))
    return s


def face(target, height=1.7, lean=0.0):
    """The head turned and pitched to look at `target` (lobby frame), at `height` in the room and `lean` metres forward
    of the room's centre (a lean over the table: standing upright the arm does not reach down to it). In the lobby the
    view at head yaw 0 looks along the lobby frame's +Y whatever the body's heading, and the simulator's yaw turns it
    left (measured)."""
    v = json.loads(lua(VIEW).strip())
    d = [target[i] - v[i] for i in range(3)]
    yaw = math.atan2(d[0], d[1])
    pitch = max(-65.0, min(10.0, math.degrees(math.atan2(d[2], math.hypot(d[0], d[1])))))
    G.sim_send('head_pose_command.json', {'x': 0.0, 'y': height, 'z': -lean, 'yaw': -yaw, 'pitch': math.radians(pitch),
                                          'roll': 0.0})
    time.sleep(0.8)


def stand(at, heading):
    lua(PUT % (at[0], at[1], 0.05, heading))
    time.sleep(1.0)


def to_front(st, name):
    """The item moved onto the table's front, within an arm's reach of where T.stand puts the player."""
    at, way, _ = T.stand(st)
    z = st['items'][name][2] + 0.02
    lua(MOVE % (name, at[0] + way[0] * 0.45, at[1] + way[1] * 0.45, z))
    time.sleep(1.5)
    return T.items()['items'][name][:3]


def unfocus(pid):
    """The game must not have the foreground: with it focused the simulator turns the keyboard and mouse into
    controller input every frame, and the pad's writes are gone within milliseconds (harness/pad.py). The focus is
    handed to a terminal (SetForegroundWindow is refused, AppActivate is not)."""
    u = ctypes.windll.user32
    p = ctypes.c_ulong()
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(p))
    if p.value != pid:
        return
    ps = ("$t = Get-Process | Where-Object { $_.MainWindowTitle -ne '' -and $_.ProcessName -match "
          "'WindowsTerminal|powershell|cmd|claude|Code' } | Select-Object -First 1; "
          "if ($t) { [void](New-Object -ComObject WScript.Shell).AppActivate($t.Id) }")
    subprocess.run(['powershell', '-NoProfile', '-Command', ps], capture_output=True)
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(p))
    print('focus handed off' if p.value != pid else 'WARNING: the game keeps the foreground', flush=True)


# The right pointer's origin (the point a snatch is read from), the right palm and an item's centre, in the world.
SNATCH_FRAME = '''local pl = Game.GetPlayer(); local p = pl:GetWorldPosition(); local q = pl:GetWorldOrientation()
local A = { Vector4.new(1, 0, 0, 0), Vector4.new(0, 0, 1, 0), Vector4.new(1, 1, 0, 0) }
local h = VRMenuPointer(Vector4.new(p.x, p.y, p.z, 1), A[1], A[2], A[3]); local o = h[7]
local pm = VRPalmModelPos(1); local pw = q:Transform(Vector4.new(pm.x, pm.y, pm.z, 0))
local c = nil
for _, it in ipairs(GetMod("CyberpunkVRPort_Lobby").items().positions()) do if it.name == "%s" then c = it end end
print(string.format('[%%f,%%f,%%f,%%f,%%f,%%f,%%f,%%f,%%f]', o.x, o.y, o.z, p.x + pw.x, p.y + pw.y, p.z + pw.z, c.x, c.y, c.z))'''


def snatch_delta(hand, name, length=0.22, probe=0.05):
    """The controller move (head-local, as `hand.at`) that carries the pointer's origin `length` straight back from the
    item, toward the palm: the origin's response to each controller axis measured with slow probes (well under a
    snatch), then solved. Which way is "back" depends on how the player stands to the item, so it is measured."""
    def frame():
        return json.loads(lua(SNATCH_FRAME % name).strip())
    f0 = frame()
    cols = []
    for k in range(3):
        d = [0.0, 0.0, 0.0]
        d[k] = probe
        hand.sweep(d, 0.4)
        time.sleep(0.25)
        f = frame()
        cols.append([(f[i] - f0[i]) / probe for i in range(3)])
        hand.sweep([-v for v in d], 0.4)
        time.sleep(0.25)
    away = [f0[3 + i] - f0[6 + i] for i in range(3)]
    n = math.sqrt(sum(v * v for v in away))
    return T.solve(cols, [v / n * length for v in away])


def trigger(pad, value):
    """The right trigger, analog and click (the simulator's controller block: +21 click, +28 value)."""
    a = pad.controllers[1]
    pad.write(a + 21, bytes([value > 0.5]))
    pad.write(a + 28, struct.pack('<f', value))


class Pointer:
    """The right controller's yaw and pitch (radians, the simulator's), its position kept."""

    def __init__(self, hand):
        self.hand = hand
        self.yaw, self.pitch, self.trigger = 0.0, -0.3, 0.0

    def send(self, settle=0.35):
        h = self.hand
        G.sim_send('controller_pose_command.json', dict(hand=1, posX=h.at[0], posY=h.at[1], posZ=h.at[2], yaw=self.yaw,
                                                        pitch=self.pitch, roll=0, trigger=self.trigger))
        time.sleep(settle)

    def error(self, name):
        """The angles (azimuth, elevation) from the ray to the item's centre, radians."""
        v = json.loads(lua(RAY % name).strip())
        o, d, c = v[0:3], v[3:6], v[6:9]
        w = [c[i] - o[i] for i in range(3)]
        n = math.sqrt(sum(x * x for x in w))
        w = [x / n for x in w]
        az = lambda u: math.atan2(u[1], u[0])  # noqa: E731
        el = lambda u: math.asin(max(-1.0, min(1.0, u[2])))  # noqa: E731
        da = (az(w) - az(d) + math.pi) % (2 * math.pi) - math.pi
        return da, el(w) - el(d)

    def aim(self, name, rounds=8, tol=math.radians(1.0)):
        """Closed loop on the ray: yaw and pitch until the ray runs through the item's centre."""
        for _ in range(rounds):
            self.send()
            e = self.error(name)
            if math.hypot(*e) < tol:
                break
            J = []
            for k in range(2):
                base = (self.yaw, self.pitch)
                if k == 0:
                    self.yaw += 0.05
                else:
                    self.pitch += 0.05
                self.send()
                ek = self.error(name)
                J.append([(e[i] - ek[i]) / 0.05 for i in range(2)])
                self.yaw, self.pitch = base
            a, b, c, d = J[0][0], J[1][0], J[0][1], J[1][1]   # how much each of yaw, pitch takes off each error
            det = a * d - b * c
            if abs(det) < 1e-6:
                break
            self.yaw += max(-0.3, min(0.3, (d * e[0] - b * e[1]) / det))
            self.pitch += max(-0.3, min(0.3, (-c * e[0] + a * e[1]) / det))
        self.send()
        return self.error(name)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    name = sys.argv[2] if len(sys.argv) > 2 else 'item cube'
    lua('GetMod("CyberpunkVRPort_Lobby").items().reset()')
    time.sleep(1.5)
    st = T.items()
    at, way, heading = T.stand(st)
    stand(at, heading)
    target = to_front(st, name)
    print('target', [round(v, 3) for v in target], flush=True)
    face(target)
    pid = int(next(line for line in subprocess.run(
        ['tasklist', '/FO', 'CSV', '/NH', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True,
        text=True).stdout.splitlines() if 'Cyberpunk' in line).split(',')[1].strip('"'))
    sim = loaded_module(pid, 'openxr_simulator.dll')
    unfocus(pid)
    ok = True
    with PadGate(pid, T.L.DEFAULT_GAME, sim) as pad:
        hand = T.Hand(pad)
        hand.grip(False)
        trigger(pad, 0.0)
        # NEAR: the controller over the item, then down to it (standing, the drawn arm falls short of the table: the
        # lobby takes from where the controller is then).
        for label, up, shown in (('far_above', 0.40, None), ('near', 0.26, (0.3, 0.6)), ('in_reach', 0.14, (0.9, 1.01))):
            at_ = hand.reach([target[0], target[1], target[2] + up], measure=controller_at)
            print('controller', [round(v, 3) for v in at_], 'palm', [round(v, 3) for v in T.palm_at()], flush=True)
            s = step(out, label)
            if shown is not None:
                ok &= s['kind'] == 'near' and shown[0] <= s['shown'] <= shown[1]
        hand.put([0.12, -0.45, -0.20])
        # AIM, from further back.
        at, way, heading = T.stand(st, back=2.6)
        stand(at, heading)
        face(target)
        hand.at = [0.12, -0.28, -0.42]
        p = Pointer(hand)
        e = p.aim(name)
        print('aim error %.2f deg' % math.degrees(math.hypot(*e)), flush=True)
        time.sleep(0.4)
        s = step(out, 'aim')
        ok &= s['aim'] == name and s['kind'] == 'aim'
        snatch = snatch_delta(hand, name)
        # LOCK: the grip closed on the item pointed at holds it from afar (the brackets).
        hand.grip(True)
        time.sleep(0.6)
        s = step(out, 'lock')
        ok &= s['lock'] == name and s['morph'] > 0.9 and s['pull'] == 'false'
        # PULL: the hand snatched back toward the body (22 cm in 0.12 s, written into the simulator's block: a pose
        # command would also let go of the grip), the grip still closed; the item flies to the hand and is held.
        hand.sweep(snatch, 0.12)
        time.sleep(0.15)
        step(out, 'pull')
        held = False
        for _ in range(30):
            if state()['right'] == name:
                held = True
                break
            time.sleep(0.1)
        step(out, 'held')
        print('pulled into the hand' if held else 'FAIL: not in the hand', flush=True)
        ok &= held
        hand.grip(False)
        trigger(pad, 0.0)
        time.sleep(1.0)
        step(out, 'released')
    print('OK' if ok else 'FAIL')


if __name__ == '__main__':
    main()
