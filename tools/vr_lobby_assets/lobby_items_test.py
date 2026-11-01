"""The lobby's items in the live game, through the simulator's right controller: the hand is brought onto an item
(closed loop: the controller's head-local offsets against where the solved palm lands in the world), the grip closes,
the item is lifted, swung and let go, and finally the hand's pointer is aimed at the table's return button and the
trigger pulled -- reporting which item each hand holds and where the items are (lobby frame) at each step.

    py -3.13 tools/vr_lobby_assets/lobby_items_test.py [ITEM_NAME [SHOT.png]]      (default: item cube)

SHOT.png: the left eye's picture of the item held up before the eyes, after the lift.

The player is stood in front of the item, the body turned to the table with the turn stick, then the head on the
item. Where the place has no return button, the put-back runs through items().reset().
"""
import json
import math
import struct
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for d in ('tools/roomscale_tests', 'tools/quickboot_tests', 'build/shotgun-reload-20261019/harness'):
    sys.path.insert(0, str(ROOT / d))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import launch_to_save as L  # noqa: E402
import game_views as G  # noqa: E402
from common import lua  # noqa: E402
from live_symbols import loaded_module  # noqa: E402
from simulator_input import PadGate  # noqa: E402
from lobby_parkour_test import PUT  # noqa: E402

FRAME = '''local L = GetMod("CyberpunkVRPort_Lobby"); local a = L.debug().anchor
local yaw = math.rad(a.yaw); local c, s = math.cos(-yaw), math.sin(-yaw)
local function lf(x, y, z) local dx, dy = x - a.pos.x, y - a.pos.y; return c * dx - s * dy, s * dx + c * dy, z - a.pos.z end
'''
ITEMS = FRAME + '''local I = L.items(); local st = I.state(); local out = {}
for _, p in ipairs(I.positions()) do
  local x, y, z = lf(p.x, p.y, p.z)
  out[#out + 1] = string.format('"%s": [%.3f, %.3f, %.3f, %.2f]', p.name, x, y, z, p.speed)
end
print(string.format('{"left": "%s", "right": "%s", "items": {%s}}', tostring(st.left), tostring(st.right),
                    table.concat(out, ", ")))'''
# The right palm (as items.lua reads it) and the camera's forward, in the lobby frame.
PALM = FRAME + '''local pl = Game.GetPlayer(); local m = VRPalmModelPos(1)
local q = pl:GetWorldOrientation()
local function rot(v) local x, y, z = v.x, v.y, v.z
  local tx, ty, tz = 2 * (q.j * z - q.k * y), 2 * (q.k * x - q.i * z), 2 * (q.i * y - q.j * x)
  return x + q.r * tx + (q.j * tz - q.k * ty), y + q.r * ty + (q.k * tx - q.i * tz), z + q.r * tz + (q.i * ty - q.j * tx) end
local px, py, pz = rot(m); local p = pl:GetWorldPosition()
local x, y, z = lf(p.x + px, p.y + py, p.z + pz)
local f = Game.GetCameraSystem():GetActiveCameraForward()
print(string.format('[%.4f, %.4f, %.4f, %.4f, %.4f]', x, y, z, c * f.x - s * f.y, s * f.x + c * f.y))'''


BACK = 0.75        # m from the item to where the player stands: the capsule clear of the desk, the item in reach


def stand(st, back=0.95):
    """Where the player is put: in front of the table, `back` metres from the things on it towards the spawn,
    facing them; with the way from there to them (lobby frame, level) and the heading for PUT (0 = +Y, + left)."""
    pts = [v[:2] for v in st['items'].values()]
    c = [sum(p[0] for p in pts) / len(pts), sum(p[1] for p in pts) / len(pts)]
    n = math.hypot(*c)
    v = [-c[0] / n, -c[1] / n]                          # from the table towards the spawn
    at = (c[0] + v[0] * back, c[1] + v[1] * back)
    return at, (-v[0], -v[1]), math.degrees(math.atan2(v[0], -v[1]))


# The right hand's pointer ray on the return button's face: (u, v across it, length, hits).
RAY = '''local hits = GetMod("CyberpunkVRPort_Lobby").items().pointer()
local r = hits and hits[6]
print(r and string.format('[%.4f, %.4f, %.3f, %.0f]', r.x, r.y, r.z, r.w) or 'null')'''


def resets():
    return int(lua('print(GetMod("CyberpunkVRPort_Lobby").items().state().resets)').strip())


# The right palm's ray (what items are pointed at with) in the lobby frame: origin, unit direction.
POINTER = FRAME + '''local r = GetMod("CyberpunkVRPort_Lobby").items().hand(2).palm
if r == nil then print('null') return end
local ox, oy, oz = lf(r.o[1], r.o[2], r.o[3])
print(string.format('[%.4f, %.4f, %.4f, %.5f, %.5f, %.5f]', ox, oy, oz, c * r.d[1] - s * r.d[2],
                    s * r.d[1] + c * r.d[2], r.d[3]))'''


def reaching():
    """What the right hand reaches for from afar: (aimed at, locked, pulling) item names or None."""
    out = lua('local r = GetMod("CyberpunkVRPort_Lobby").items().state().reach[2]\n'
              'print(tostring(r.aim) .. "|" .. tostring(r.lock) .. "|" .. tostring(r.pull))').strip().split('|')
    return tuple(None if v == 'false' else v for v in out)


# The body's forward in the lobby frame.
BODY = FRAME + '''local f = Game.GetPlayer():GetWorldForward()
print(string.format('%.5f %.5f', c * f.x - s * f.y, s * f.x + c * f.y))'''


def body_heading():
    """The body's heading in the lobby frame, degrees (0 = +Y, + left), as PUT takes it."""
    x, y = (float(v) for v in lua(BODY).split())
    return math.degrees(math.atan2(-x, y))


def turn_body(pad, heading, tol=2.0):
    """The body turned to `heading` with the turn stick, the way a player turns: a teleport's heading does not hold
    in VR (the port keeps the body's own), and turning the head alone leaves the body where it was -- the table
    behind it. The turn is on the main hand's stick, the right controller's (x > 0 turns right; the left stick walks)."""
    stick = pad.controllers[1] + 36

    def off():
        return (heading - body_heading() + 180) % 360 - 180
    for _ in range(6):
        d = off()
        if abs(d) < tol:
            return body_heading()
        # Held (a short flick stays inside the stick's dead zone and ramp) until the heading is there, slower
        # for the last stretch.
        pad.write(stick, struct.pack('<2f', -math.copysign(0.6 if abs(d) < 30 else 1.0, d), 0.0))
        start, now = time.time(), d
        while time.time() - start < 8 and abs(now) >= tol / 2 and now * d > 0:
            now = off()
        pad.write(stick, struct.pack('<2f', 0.0, 0.0))
        time.sleep(0.3)
        if abs(off() - d) < 0.5:
            raise RuntimeError('the turn stick does not turn the body (%.1f deg off)' % d)
    raise RuntimeError('the body did not come round to %.0f deg (%.1f off)' % (heading, off()))


def items():
    return json.loads(lua(ITEMS).strip())


def palm():
    v = json.loads(lua(PALM).strip())
    return v[:3], v[3:]


def palm_at():
    return palm()[0]


def show(label):
    st = items()
    print(label, 'held L=%s R=%s' % (st['left'], st['right']), ' | '.join(
        '%s %s' % (k[5:], v) for k, v in st['items'].items()), flush=True)
    return st


class Hand:
    """The simulator's right controller, head-local (x right, y up, -z forward), placed so the palm lands where asked
    in the lobby frame."""

    def __init__(self, pad):
        self.pad = pad
        self.at = [0.05, -0.35, -0.40]
        self.pitch = -0.9

    def put(self, at=None, settle=0.35):
        if at is not None:
            self.at = list(at)
        G.sim_send('controller_pose_command.json', dict(hand=1, posX=self.at[0], posY=self.at[1], posZ=self.at[2],
                                                        yaw=0, pitch=self.pitch, roll=0, trigger=0))
        time.sleep(settle)

    def jacobian(self, measure=palm_at, step=0.08):
        self.put()
        p0 = measure()
        cols = []
        for k in range(3):
            base = list(self.at)
            self.at[k] += step
            self.put()
            p = measure()
            cols.append([(p[i] - p0[i]) / step for i in range(3)])
            self.at = base
        self.put()
        return cols                                     # cols[k] = d measure / d controller axis k

    def reach(self, target, rounds=4, tol=0.02, measure=palm_at):
        """Closed loop: the controller moved until `measure()` (the palm in the lobby frame, by default) is at
        `target`."""
        for _ in range(rounds):
            J = self.jacobian(measure)
            p = measure()
            err = [target[i] - p[i] for i in range(3)]
            if math.sqrt(sum(e * e for e in err)) < tol:
                break
            d = solve(J, err)
            self.at = [self.at[i] + max(-0.25, min(0.25, d[i])) for i in range(3)]
            self.put()
        return measure()

    def sweep(self, delta, duration):
        """The controller moved by `delta` (head-local, as `at`) over `duration` seconds, written into the simulator's
        block directly -- its position there is the pose file's, offset for offset."""
        a = self.pad.controllers[1]
        p0 = list(struct.unpack('<3f', self.pad.r.read(a, 12)))
        start = time.perf_counter()
        while True:
            f = min(1.0, (time.perf_counter() - start) / duration)
            self.pad.write(a, struct.pack('<3f', *[p0[i] + delta[i] * f for i in range(3)]))
            if f >= 1.0:
                break
            time.sleep(0.002)
        self.at = [self.at[i] + delta[i] for i in range(3)]

    def aim(self, yaw, pitch, trigger=0.0, settle=0.45):
        G.sim_send('controller_pose_command.json', dict(hand=1, posX=self.at[0], posY=self.at[1], posZ=self.at[2],
                                                        yaw=yaw, pitch=pitch, roll=0, trigger=trigger))
        time.sleep(settle)
        return json.loads(lua(RAY).strip())

    def point_at_button(self, yaw=1.0, pitch=-0.6):
        """Closed loop on the ray's (u, v): the controller's yaw and pitch until the ray meets the button's middle."""
        for _ in range(8):
            r = self.aim(yaw, pitch)
            if r[3] > 0.5 and abs(r[0] - 0.5) < 0.2 and abs(r[1] - 0.5) < 0.2:
                return yaw, pitch, r
            ry, rp = self.aim(yaw + 0.05, pitch), self.aim(yaw, pitch - 0.05)
            a, b = (ry[0] - r[0]) / 0.05, (rp[0] - r[0]) / -0.05
            c, d = (ry[1] - r[1]) / 0.05, (rp[1] - r[1]) / -0.05
            det = a * d - b * c
            if abs(det) < 1e-6:
                break
            eu, ev = 0.5 - r[0], 0.5 - r[1]
            yaw += max(-0.3, min(0.3, (d * eu - b * ev) / det))
            pitch += max(-0.3, min(0.3, (-c * eu + a * ev) / det))
        return yaw, pitch, self.aim(yaw, pitch)

    def ray_miss(self, yaw, pitch, target, settle=0.35):
        """How far off `target` (lobby frame) the hand's pointer runs, the controller turned to yaw, pitch: its
        azimuth and elevation errors, radians."""
        G.sim_send('controller_pose_command.json', dict(hand=1, posX=self.at[0], posY=self.at[1], posZ=self.at[2],
                                                        yaw=yaw, pitch=pitch, roll=0, trigger=0))
        time.sleep(settle)
        r = json.loads(lua(POINTER).strip())
        if r is None:
            raise RuntimeError('the right hand has no pointer')
        o, d = r[:3], r[3:]
        t = [target[i] - o[i] for i in range(3)]
        az = (math.atan2(t[1], t[0]) - math.atan2(d[1], d[0]) + math.pi) % (2 * math.pi) - math.pi
        el = math.atan2(t[2], math.hypot(t[0], t[1])) - math.atan2(d[2], math.hypot(d[0], d[1]))
        return az, el

    def point_at(self, target, yaw=0.0, pitch=-0.5):
        """Closed loop on the pointer's miss: the controller's yaw and pitch until it runs through `target`."""
        for _ in range(8):
            e = self.ray_miss(yaw, pitch, target)
            if abs(e[0]) < 0.02 and abs(e[1]) < 0.02:
                break
            ey, ep = self.ray_miss(yaw + 0.05, pitch, target), self.ray_miss(yaw, pitch + 0.05, target)
            a, b = (ey[0] - e[0]) / 0.05, (ep[0] - e[0]) / 0.05
            c, d = (ey[1] - e[1]) / 0.05, (ep[1] - e[1]) / 0.05
            det = a * d - b * c
            if abs(det) < 1e-6:
                break
            yaw += max(-0.3, min(0.3, (-d * e[0] + b * e[1]) / det))
            pitch += max(-0.3, min(0.3, (c * e[0] - a * e[1]) / det))
        return yaw, pitch, self.ray_miss(yaw, pitch, target)

    def grip(self, on):
        a = self.pad.controllers[1]
        self.pad.write(a + 22, bytes([bool(on)]))
        self.pad.write(a + 32, struct.pack('<f', 1.0 if on else 0.0))


def solve(cols, b):
    """x with sum_k x_k cols[k] = b (Cramer)."""
    A = [[cols[k][i] for k in range(3)] for i in range(3)]

    def det(m):
        return (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
    D = det(A)
    if abs(D) < 1e-6:
        return [0.0, 0.0, 0.0]
    out = []
    for k in range(3):
        m = [row[:] for row in A]
        for i in range(3):
            m[i][k] = b[i]
        out.append(det(m) / D)
    return out


def player_at():
    """Where the player stands, lobby frame (x, y)."""
    return json.loads(lua(FRAME + '''local p = Game.GetPlayer():GetWorldPosition(); local x, y = lf(p.x, p.y, p.z)
print(string.format('[%.3f, %.3f]', x, y))''').strip())


def face(target):
    """Turn the head (yaw) until the camera looks along the lobby-frame direction to `target` from the player."""
    yaw = 0.0
    for _ in range(3):
        G.head(yaw, -45)
        time.sleep(0.5)
        p, f = palm()
        here = player_at()
        want = math.atan2(target[1] - here[1], target[0] - here[0])
        have = math.atan2(f[1], f[0])
        d = (want - have + math.pi) % (2 * math.pi) - math.pi
        if abs(d) < math.radians(3):
            break
        yaw -= math.degrees(d)                          # game_views.head: +yaw turns right, clockwise from above
    return yaw


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else 'item cube'
    G.head(0, -45)
    st = items()
    _, way, heading = stand(st)
    # Straight in front of the item, the way the table faces the spawn.
    target = st['items'][name][:3]
    at = (target[0] - way[0] * BACK, target[1] - way[1] * BACK)
    lua(PUT % (at[0], at[1], 0.05, heading))
    time.sleep(1.0)
    st = show('start')
    target = st['items'][name][:3]
    pid = int(next(line for line in subprocess.run(
        ['tasklist', '/FO', 'CSV', '/NH', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True,
        text=True).stdout.splitlines() if 'Cyberpunk' in line).split(',')[1].strip('"'))
    sim = loaded_module(pid, 'openxr_simulator.dll')
    with PadGate(pid, L.DEFAULT_GAME, sim) as pad:
        print('body turned to %.1f deg (wanted %.1f)' % (turn_body(pad, heading), heading), flush=True)
        # Where the desk let the player stand (its collision may push the capsule back): the swings start from there.
        at = player_at()
        print('stands %.2f m from the item' % math.dist(at, target[:2]), flush=True)
        face(target)
        hand = Hand(pad)
        hand.grip(False)
        # The palm just above the item's top, then down onto it.
        above = [target[0], target[1], target[2] + 0.22]
        print('palm above', [round(v, 3) for v in hand.reach(above)], flush=True)
        print('palm at', [round(v, 3) for v in hand.reach([target[0], target[1], target[2] + 0.06])], flush=True)
        hand.grip(True)
        time.sleep(0.5)
        st = show('gripped')
        lift_at = [target[0], target[1], target[2] + 0.35]
        if st['right'] != name:
            # Out of the arm's reach across the desk: taken from afar, as a player does -- the pointer on it, the
            # grip closed (it locks), the hand snatched back (it flies to the hand).
            print('not in reach: taking it from afar', flush=True)
            hand.grip(False)
            time.sleep(0.3)
            hand.at = [0.12, -0.25, -0.35]
            ay, ap, miss = hand.point_at(target)
            print('pointer %.3f %.3f rad off, reaching %s' % (miss[0], miss[1], reaching()), flush=True)
            hand.grip(True)
            time.sleep(0.4)
            print('locked:', reaching()[1], flush=True)
            hand.sweep([0.0, 0.05, 0.35], 0.12)         # head-local +z: back towards the player, ~3 m/s
            time.sleep(1.5)
            st = show('pulled')
            lift_at = [at[0] + way[0] * 0.35, at[1] + way[1] * 0.35, target[2] + 0.35]
        if st['right'] != name:
            print('FAIL: the grip did not take', name)
        hand.reach(lift_at, rounds=2)
        time.sleep(0.8)
        lifted = show('lifted')
        rise = lifted['items'][name][2] - target[2]
        print('lift: %.3f m' % rise, 'OK' if rise > 0.2 else 'FAIL')
        if len(sys.argv) > 2:
            # The held item before the eyes, for a look at the grasp.
            G.head(0, -20)
            hand.reach([at[0] + way[0] * 0.3, at[1] + way[1] * 0.3, 1.45], rounds=2)
            time.sleep(0.8)
            G.shot().save(sys.argv[2])
            print('held, seen:', sys.argv[2], flush=True)
        # The throw: the palm swung 0.6 m on over the table and up (away from the player) at about 3 m/s, let go
        # at the end of the swing. The pose file is read too seldom for a swing, so the controller's position is
        # written straight into the simulator's block, every 2 ms (as the knife's probe). From in front of the
        # chest, where the arm has the whole swing ahead of it.
        hand.reach([at[0] + way[0] * 0.25, at[1] + way[1] * 0.25, 1.20], rounds=3)
        J = hand.jacobian()
        swing = solve(J, [way[0] * 0.5, way[1] * 0.5, 0.33])
        hand.sweep(swing, 0.2)
        hand.grip(False)
        thrown = show('released')
        time.sleep(2.0)
        landed = show('landed')
        moved = math.dist(landed['items'][name][:2], thrown['items'][name][:2])
        print('throw: %.2f m past the release, speed at release %.2f m/s' % (moved, thrown['items'][name][3]),
              'OK' if moved > 0.5 else 'FAIL')
        # The return button: the hand's pointer on it, the trigger pulled -- where the place has one; else the
        # put-back it would call.
        G.head(0, -45)
        r0 = resets()
        if lua('print(GetMod("CyberpunkVRPort_Lobby").items().pointer ~= nil)').strip() == 'true':
            hand.at = [0.1, -0.4, -0.3]
            ay, ap, ray = hand.point_at_button()
            print('ray on the button', ray, flush=True)
            hand.aim(ay, ap, 1.0, 0.6)
            hand.aim(ay, ap, 0.0, 0.4)
            print('button press:', 'OK' if resets() > r0 else 'FAIL', flush=True)
        else:
            lua('GetMod("CyberpunkVRPort_Lobby").items().reset()')
            print('no return button here: put back by items().reset()', 'OK' if resets() > r0 else 'FAIL',
                  flush=True)
        hand.at = [0.05, -0.35, -0.40]
        hand.put()
    time.sleep(1.5)
    back = show('back')
    home = math.dist(back['items'][name][:2], target[:2])
    print('back on the table: %.3f m from home' % home, 'OK' if home < 0.05 else 'FAIL')


if __name__ == '__main__':
    main()
