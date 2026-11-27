"""slide_grab.py OUT [RECORD] [--shots]: in the running game (devgame.py start), the free (left) hand works the held gun's
slide (or a pump's fore-end) as a player does -- put on the grip the reload offers, the grip pressed (the trigger for a
grabWith=trigger gun, both for a fore-end), pulled a full stroke back, let go -- with the port's own account at each
step (log/reload_reach.json, the "reach" command; it is rewritten every 100 ms and each step waits for the next one,
not a fixed time). RECORD, when given, is given and drawn first ("weapon" command). --shots: the left eye's stills.

The pose: reload_reach.json's want_local is the head-relative controller pose that puts the wrist on the grip; the
simulator's hands in body mode are head-relative with the head level and turned 0, and the port turns a simulated grip
up by CyberpunkVR_SimGripPitchDeg (75) -- so the controller is want_local with that turn taken off.
"""
import json
import math
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
from simxr import Sim  # noqa: E402

REACH = D.GAME / 'cyberpunkvrport/log/reload_reach.json'
PITCH = math.radians(75.0)
F_INV = (-math.sin(PITCH / 2), 0.0, 0.0, math.cos(PITCH / 2))
REST = dict(pos=(-.30, -.70, .20), ypr=(0, 0, 0), mode='body')


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def reach():
    for _ in range(20):
        try:
            return json.loads(REACH.read_text(encoding='utf-8'))
        except (OSError, ValueError):
            time.sleep(0.02)
    return {}


def fresh(after=None, until=None, most=1.0):
    """The next account written after `after` (its t), or the first that satisfies `until`, at most `most` seconds."""
    t_old = (after or {}).get('t', -1.0)
    t0 = time.time()
    r = reach()
    while time.time() - t0 < most:
        r = reach()
        if r.get('t', -1.0) > t_old and (until is None or until(r)):
            return r
        time.sleep(0.03)
    return r


def settle(seconds=0.5):
    """Kept for callers that want a plain wait; the account after it."""
    time.sleep(seconds)
    return reach()


def brief(r):
    n = r.get('near') or {}
    return (f"d={n.get('d')} angle={n.get('angle')} grabbed={r.get('grabbed')} rack={r.get('rack')} "
            f"why={r.get('why')} exit={r.get('exit')}")


def press(sim, how, on):
    v = 1.0 if on else 0.0
    if how in ('trigger', 'grip+trigger'):
        sim.trigger('left', v)
    if how != 'trigger':
        sim.grip('left', v)


def put_on(sim, r, turn=None):
    """The left hand onto want_local (turned by the quaternion `turn` in the controller's frame): the residual taken
    off until the wrist is within 4 mm of the grip (the hand arrives over a few frames: each step waits for the hand
    to stand still). The account after it."""
    pos = list(r['want_local'][0])
    for _ in range(6):
        rot = qmul(tuple(r['want_local'][1]), F_INV)
        if turn:
            rot = qmul(rot, turn)
        sim.hand('left', pos=tuple(pos), quat=rot, mode='body')
        last = None
        for _ in range(12):
            r = fresh(r)
            cur = (r.get('current_local') or [None])[0]
            if last and cur and sum((a - b) ** 2 for a, b in zip(last, cur)) ** 0.5 < 0.001:
                break
            last = cur
        if 'want_local' not in r:
            break
        err = [w - c for w, c in zip(r['want_local'][0], r['current_local'][0])]
        if sum(e * e for e in err) ** 0.5 < 0.004:
            break
        pos = [p + e for p, e in zip(pos, err)]
    return r


def run(out, record=None, shots=False):
    out = Path(out)
    sim = Sim(D.game_pid(), focus_away=True)
    D.cmd('reach 1')
    if record:
        D.cmd(f'weapon {record}', wait='active weapon', timeout=40)
    sim.hands_follow_head(True)
    sim.head(0, 0)
    sim.hand('right', pos=(.18, -.14, -.44), ypr=(0, 48, -5), mode='body')
    press(sim, 'grip+trigger', False)
    sim.hand('left', **REST)
    r = fresh(None, until=lambda x: 'want_local' in x or x.get('exit') not in (None, 'ok'), most=3.0)
    r = fresh(r, until=lambda x: 'want_local' in x, most=1.0)
    # the gun still coming up (a long draw): the grip waited for until it stands still (2 mm between two accounts)
    for _ in range(40):
        prev = (r.get('near') or {}).get('at')
        r = fresh(r, until=lambda x: 'want_local' in x, most=0.5)
        at = (r.get('near') or {}).get('at')
        if prev and at and sum((a - b) ** 2 for a, b in zip(prev, at)) ** 0.5 < 0.002:
            break
    log = [f'weapon {r.get("weapon")} ({r.get("kind")}, grab with {r.get("grab_with")}, radius {r.get("radius")}, '
           f'angle max {r.get("angle_max")}, travel {r.get("travel")})', 'rest: ' + brief(r)]
    if 'want_local' not in r:
        log.append('no grip target: ' + brief(r))
        return log, False
    r = put_on(sim, r)
    log.append('on the grip: ' + brief(r))
    pull = r.get('pull_local')
    travel = r.get('travel') or 0.0
    how = r.get('grab_with')
    if shots:
        out.mkdir(parents=True, exist_ok=True)
        sim.shot(str(out / 'a_on_grip'), eyes='left', timeout=30)
    press(sim, how, True)
    r = fresh(r, until=lambda x: x.get('grabbed'), most=0.8)
    log.append('pressed: ' + brief(r))
    grabbed = bool(r.get('grabbed'))
    if pull and grabbed:
        sim.glide('left', pos=tuple(pull[0]), quat=qmul(tuple(pull[1]), F_INV), seconds=0.2, mode='body')
        r = fresh(r, until=lambda x: (x.get('rack') or 0.0) >= 0.9 * travel, most=0.8)
        log.append('pulled: ' + brief(r))
        if shots:
            sim.shot(str(out / 'b_pulled'), eyes='left', timeout=30)
    press(sim, how, False)
    r = fresh(r, until=lambda x: not x.get('grabbed'), most=0.8)
    log.append('let go: ' + brief(r))
    if shots:
        sim.shot(str(out / 'c_let_go'), eyes='left', timeout=30)
    sim.hand('left', **REST)
    if shots:
        (out / 'result.txt').write_text('\n'.join(log), encoding='utf-8')
    return log, grabbed


if __name__ == '__main__':
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    lines, ok = run(args[0], args[1] if len(args) > 1 else None, shots='--shots' in sys.argv)
    print('\n'.join(lines))
    print('GRABBED' if ok else 'NOT GRABBED')
