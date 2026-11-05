"""The lobby's controls in the live game, through the simulator's right hand (lobby_items_test.Hand's closed loops):

  dial    the terminal's СВЕТ dial taken with the grip, the wrist rolled one way and back, let go -- the light level
          before, at the far roll and after; then a pointer press on it (one stop on). --roll: the wrist rolled about
          the controller's own axis (a simulator build that takes a controller roll); without it, the same turn
          made of a yaw and a pitch
  button  the fingertip brought in front of the table's return button and pushed into it -- the resets count

    py -3.13 tools/vr_lobby_assets/lobby_controls_test.py [dial] [button] [--roll]
"""
import json
import math
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_items_test as T  # noqa: E402
G, lua = T.G, T.lua
ROLL = '--roll' in sys.argv

TIP = '''local hits, w, h = GetMod("CyberpunkVRPort_Lobby").items().pointer()
local t = hits and hits[5]
print(t and t.w > 0.5 and string.format('[%.4f, %.4f, %.4f]', (t.x - 0.5) * w, (0.5 - t.y) * h, t.z) or 'null')'''
# The right hand's ray on the dial's face (the dial from the scene list, through the lobby's anchor).
DIAL_RAY = '''local a = GetMod("CyberpunkVRPort_Lobby").debug().anchor
local yaw = math.rad(a.yaw); local c, s = math.cos(yaw), math.sin(yaw)
local function w(v, p) return Vector4.new((p and a.pos.x or 0) + c * v[1] - s * v[2], (p and a.pos.y or 0) + s * v[1] + c * v[2],
                                       (p and a.pos.z or 0) + v[3], p and 1 or 0) end
local hits = VRMenuPointer(w({%s}, true), w({%s}), w({%s}), Vector4.new(%f, %f, 0, 0))
local r = hits and hits[6]
print(r and string.format('[%%.4f, %%.4f, %%.3f, %%.0f]', r.x, r.y, r.z, r.w) or 'null')'''


def light():
    return json.loads(lua('''local s = GetMod("CyberpunkVRPort_Lobby").state().lights
print(string.format('{"level": %.3f, "held": "%s"}', s.level, tostring(s.held)))''').strip())


def scene_list():
    """The dial and the button from the generated scene list (lobby frame)."""
    text = (T.ROOT / 'mods/cet/CyberpunkVRPort_Lobby/modules/scene_list.lua').read_text(encoding='utf-8')

    def vec(line, key):
        i = line.index(key + ' = {') + len(key) + 4
        return [float(v) for v in line[i:line.index('}', i)].split(',')]
    dial = next(l for l in text.splitlines() if l.strip().startswith('light = {'))
    button = next(l for l in text.splitlines() if l.strip().startswith('["return"]'))
    return ({k: vec(dial, k) for k in ('centre', 'front', 'right', 'up')} | {'radius': float(
        dial[dial.index('radius = ') + 9:].split()[0].rstrip(','))},
            {k: vec(button, k) for k in ('centre', 'front')})


def stand_before(target, front, dist, eye=1.7, lean=0.0):
    """The player put `dist` in front of a face (along its front, level), facing it; the head at `eye` metres and
    `lean` metres ahead of the body (leaning over: the character's capsule keeps the body off the table)."""
    f = [front[0], front[1]]
    n = math.hypot(*f)
    f = [f[0] / n, f[1] / n]
    x, y = target[0] + f[0] * dist, target[1] + f[1] * dist
    heading = math.degrees(math.atan2(f[0], -f[1]))     # facing back along -front: 0 = +Y, positive = left
    lua(T.PUT % (x, y, 0.05, heading))
    time.sleep(1.0)
    yaw = T.face(target)
    here = json.loads(lua(T.FRAME + '''local p = Game.GetPlayer():GetWorldPosition(); local x, y = lf(p.x, p.y, p.z)
print(string.format('[%.3f, %.3f]', x, y))''').strip())
    print('stood at', here, 'asked', [round(x, 3), round(y, 3)], flush=True)
    if eye != 1.7 or lean:
        # OpenXR tracking space: -Z ahead at no yaw, the head's yaw about +Y.
        psi = -math.radians(yaw)
        G.sim_send('head_pose_command.json', {'x': -math.sin(psi) * lean, 'y': eye, 'z': -math.cos(psi) * lean,
                                              'yaw': psi, 'pitch': math.radians(-45), 'roll': 0.0})
        time.sleep(0.8)


def test_dial(hand, dial):
    c, f = dial['centre'], dial['front']
    stand_before(c, f, 0.55)
    lua('GetMod("CyberpunkVRPort_Lobby").state()')
    before = light()
    print('dial: level before', before, flush=True)
    hand.pitch = 0.0
    at = [c[i] + f[i] * 0.06 for i in range(3)]
    print('palm at the dial', [round(v, 3) for v in hand.reach(at)], flush=True)
    hand.grip(True)
    time.sleep(0.5)
    st = light()
    print('gripped', st, 'OK' if st['held'] == '2' else 'FAIL', flush=True)

    def pose(yaw, pitch, roll):
        G.sim_send('controller_pose_command.json', dict(hand=1, posX=hand.at[0], posY=hand.at[1], posZ=hand.at[2],
                                                        yaw=yaw, pitch=pitch, roll=roll, trigger=0))
        time.sleep(0.4)

    if ROLL:
        # The wrist rolled about the controller's own pointing axis, the hand held out to the dial: the way the dial
        # is turned with a real controller (the simulator build that takes a controller roll).
        yaw, twist = 0.0, lambda a: pose(0.0, hand.pitch, -a)
    else:
        # The simulator's controller has no roll: the hand is turned a quarter left (its right side towards the dial)
        # and pitched, a turn about its own X, now the dial's axis.
        hand.grip(False)
        time.sleep(0.3)
        yaw = 1.5708
        pose(yaw, 0.0, 0.0)
        hand.grip(True)
        time.sleep(0.4)
        print('gripped sideways', light(), flush=True)
        twist = lambda a: pose(yaw, a, 0.0)  # noqa: E731
    rolled = []
    for a in (0.25, 0.5, 0.75):
        twist(a)
        rolled.append(light()['level'])
    print('turned', rolled, flush=True)
    for a in (0.5, 0.25, 0.0):
        twist(a)
    back = light()['level']
    hand.grip(False)
    time.sleep(0.4)
    after = light()
    turned = abs(rolled[-1] - before['level'])
    print('dial turn: %.2f -> %.2f -> %.2f' % (before['level'], rolled[-1], back),
          'OK' if turned > 0.3 and abs(back - before['level']) < 0.15 else 'FAIL', 'released', after, flush=True)
    # A pointer press: the ray on the dial, the trigger pulled.
    hand.at = [0.1, -0.35, -0.3]
    ray_lua = DIAL_RAY % (', '.join(map(str, c)), ', '.join(map(str, dial['right'])), ', '.join(map(str, dial['up'])),
                          2 * dial['radius'], 2 * dial['radius'])
    old = T.RAY
    T.RAY = ray_lua
    try:
        ay, ap, ray = hand.point_at_button(yaw=0.0, pitch=-0.3)
    finally:
        T.RAY = old
    print('ray on the dial', ray, flush=True)
    l0 = light()['level']
    T.RAY = ray_lua
    try:
        hand.aim(ay, ap, 1.0, 0.6)
        hand.aim(ay, ap, 0.0, 0.4)
    finally:
        T.RAY = old
    l1 = light()['level']
    print('dial press: %.2f -> %.2f' % (l0, l1), 'OK' if abs(l1 - l0) > 0.1 else 'FAIL', flush=True)


def tip():
    return json.loads(lua(TIP).strip())


def descend(hand, measure, target, step=0.04, rounds=12, tol=0.008):
    """Coordinate descent on the controller's head-local offsets: each axis tried a step either way, kept where it
    brings `measure()` nearer `target`; the step halved when no axis helps. For where the arm is near its full reach
    and a Jacobian taken there misleads."""
    def err():
        m = measure()
        return math.dist(m, target), m
    best, m = err()
    for _ in range(rounds):
        if best < tol:
            break
        improved = False
        for k in range(3):
            for sgn in (1, -1):
                base = list(hand.at)
                hand.at[k] += sgn * step
                hand.put()
                e, mm = err()
                if e < best:
                    best, m, improved = e, mm, True
                    break
                hand.at = base
        if not improved:
            step /= 2
            if step < 0.005:
                break
    hand.put()
    return m


def test_button(hand, button):
    c, f = button['centre'], button['front']
    # At the table's front, leaning over the tray.
    stand_before(c, f, 0.47, eye=1.25, lean=0.5)
    hand.pitch = -0.6
    hand.at = [0.05, -0.45, -0.30]
    hand.put()
    r0 = T.resets()
    print('tip before', [round(v, 3) for v in descend(hand, tip, [0.0, 0.0, 0.03], step=0.06, rounds=20)],
          flush=True)
    print('tip pressed', [round(v, 3) for v in descend(hand, tip, [0.0, 0.0, -0.012], step=0.02, rounds=10)],
          flush=True)
    time.sleep(0.3)
    print('button by fingertip:', 'OK' if T.resets() > r0 else 'FAIL', flush=True)
    hand.at = [0.05, -0.35, -0.40]
    hand.pitch = -0.9
    hand.put()


def main():
    which = [a for a in sys.argv[1:] if not a.startswith('--')] or ['dial', 'button']
    dial, button = scene_list()
    pid = int(next(line for line in subprocess.run(
        ['tasklist', '/FO', 'CSV', '/NH', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True,
        text=True).stdout.splitlines() if 'Cyberpunk' in line).split(',')[1].strip('"'))
    sim = T.loaded_module(pid, 'openxr_simulator.dll')
    with T.PadGate(pid, T.L.DEFAULT_GAME, sim) as pad:
        hand = T.Hand(pad)
        hand.grip(False)
        if 'dial' in which:
            test_dial(hand, dial)
        if 'button' in which:
            test_button(hand, button)
    G.head(0, 0)


if __name__ == '__main__':
    main()
