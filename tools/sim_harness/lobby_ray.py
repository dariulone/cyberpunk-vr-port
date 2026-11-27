"""lobby_ray.py [--fast DEG_PER_S]: does a pointer beam in the main menu lobby move when only the head turns? The game
in the lobby (devgame.py start --menu), "reach 1" (the lobby's beams kept per frame in log/menu_rays.json), the right
controller held still in the room (the simulator's world mode) pointing at the holo menu, the head turned about the
neck +-10 degrees: fast sweeps, slow sweeps, then held. Per phase: how far the right beam's origin and its hit moved
(mm, the spread of all frames) against the eye's own travel."""
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
from simxr import Sim  # noqa: E402

RAYS = D.GAME / 'cyberpunkvrport/log/menu_rays.json'
HAND = dict(pos=(0.18, 1.35, -0.35), ypr=(0, 5, 0), mode='world')


def samples():
    try:
        return json.loads(RAYS.read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return []


def spread(points):
    pts = [p for p in points if p]
    if len(pts) < 2:
        return None
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    return round(sum((h - l) ** 2 for l, h in zip(lo, hi)) ** 0.5 * 1000.0, 1)


def phase(sim, name, yaws, dt):
    t0 = None
    sim.head(yaw=yaws[0], pitch=0.0)
    time.sleep(0.6)
    start = samples()
    t_from = start[-1][0] if start else 0.0
    for y in yaws:
        sim.head(yaw=y, pitch=0.0)
        time.sleep(dt)
    time.sleep(0.3)
    got = [s for s in samples() if s[0] > t_from and s[1] == 1]
    o = spread([s[2] for s in got])
    h = spread([s[3] for s in got])
    e = spread([s[4] for s in got])
    print(f'{name:>10}: {len(got)} frames  origin moved {o} mm  hit moved {h} mm  (eye {e} mm)', flush=True)
    return got


def sweep(a, b, deg_per_s, dt=0.011):
    steps = max(2, int(abs(b - a) / deg_per_s / dt))
    return [a + (b - a) * k / steps for k in range(steps + 1)]


def main():
    fast = float(sys.argv[sys.argv.index('--fast') + 1]) if '--fast' in sys.argv else 120.0
    sim = Sim(D.game_pid(), focus_away=True)
    D.cmd('reach 1', wait='[dev] reach', timeout=10)
    sim.hands_follow_head(False)
    sim.head(yaw=0.0, pitch=0.0)
    sim.hand('right', **HAND)
    sim.hand('left', pos=(-0.30, 0.70, 0.20), ypr=(0, 0, 0), mode='world')
    time.sleep(1.5)
    s = samples()
    last = [x for x in s if x[1] == 1][-1:] if s else []
    print('right beam now:', last, flush=True)
    phase(sim, 'held', [0.0] * 60, 0.011)
    phase(sim, 'fast', sweep(-10, 10, fast) + sweep(10, -10, fast) + sweep(-10, 10, fast) + sweep(10, -10, fast), 0.011)
    phase(sim, 'slow', sweep(-10, 10, 20.0) + sweep(10, -10, 20.0), 0.011)
    got = phase(sim, 'held +10', [10.0] * 60, 0.011)
    phase(sim, 'held -10', [-10.0] * 60, 0.011)
    sim.head(yaw=0.0, pitch=0.0)
    out = Path(D.GAME / 'cyberpunkvrport/log/lobby_ray_fast.json')
    out.write_text(json.dumps(got), encoding='utf-8')
    D.cmd('reach 0', wait='[dev] reach', timeout=10)


if __name__ == '__main__':
    main()
