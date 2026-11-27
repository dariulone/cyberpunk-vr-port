"""gun_look.py OUT RECORD: the held gun in front and below the eyes, looked at from above, its slide or fore-end worked
by the left hand (slide_grab.py's steps) with a still at rest, on the grip, pulled back and let go -- what the player
sees of the gun's model while the reload works it."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402
from simxr import Sim  # noqa: E402

RIGHT = dict(pos=(.10, -.30, -.42), ypr=(-25, 10, 0), mode='body')


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rec = sys.argv[2]
    sim = Sim(D.game_pid(), focus_away=True)
    D.cmd('reach 1')
    D.cmd(f'weapon {rec}', wait='active weapon', timeout=40)
    sim.hands_follow_head(True)
    sim.head(0, 0)
    sim.hand('right', **RIGHT)
    S.press(sim, 'grip+trigger', False)
    sim.hand('left', **S.REST)
    r = S.fresh(None, until=lambda x: 'want_local' in x, most=3.0)
    for _ in range(40):
        prev = (r.get('near') or {}).get('at')
        r = S.fresh(r, until=lambda x: 'want_local' in x, most=0.5)
        at = (r.get('near') or {}).get('at')
        if prev and at and sum((a - b) ** 2 for a, b in zip(prev, at)) ** 0.5 < 0.002:
            break
    sim.shot(str(out / 'a_rest'), eyes='left', timeout=30)
    r = S.put_on(sim, r)
    how = r.get('grab_with')
    pull = r.get('pull_local')
    S.press(sim, how, True)
    r = S.fresh(r, until=lambda x: x.get('grabbed'), most=0.8)
    print('grabbed', r.get('grabbed'), flush=True)
    sim.shot(str(out / 'b_on_grip'), eyes='left', timeout=30)
    if pull:
        sim.glide('left', pos=tuple(pull[0]), quat=S.qmul(tuple(pull[1]), S.F_INV), seconds=0.3, mode='body')
        r = S.fresh(r, until=lambda x: (x.get('rack') or 0.0) >= 0.9 * (x.get('travel') or 1.0), most=1.0)
        print('rack', r.get('rack'), flush=True)
        sim.shot(str(out / 'c_pulled'), eyes='left', timeout=30)
        sim.glide('left', pos=tuple(r['want_local'][0]) if 'want_local' in r else tuple(pull[0]),
                  quat=S.qmul(tuple(pull[1]), S.F_INV), seconds=0.3, mode='body')
    S.press(sim, how, False)
    time.sleep(0.5)
    sim.hand('left', **S.REST)
    time.sleep(0.8)
    sim.shot(str(out / 'd_let_go'), eyes='left', timeout=30)


if __name__ == '__main__':
    main()
