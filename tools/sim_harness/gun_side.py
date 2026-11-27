"""gun_side.py OUT RECORD [steps]: the held gun turned side-on below the eyes, a still at each step -- what the player sees
of the gun's model while its parts move. Steps (default "rest,pump"):
    rest     a still as it is
    pump     the left hand on the slide or fore-end the reload offers, pressed, pulled a full stroke, a still pulled
             back, pushed home, let go, a still
    empty    the magazine emptied ("ammo 0"), a still after 1.5 s
    r3       the right stick clicked (a break-action opens or closes), a still after 1 s
    wait     1.5 s, a still
The stills are OUT/<n>_<step>_L.jpg, and OUT/sheet.jpg holds them cropped side by side."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402
from simxr import Sim  # noqa: E402

SIDE = dict(pos=(.12, -.22, -.45), ypr=(70, -40, 0), mode='body')


def pump(sim, out, n):
    D.cmd('reach 1')
    r = S.fresh(None, until=lambda x: 'want_local' in x, most=3.0)
    if 'want_local' not in r:
        return ['no grip offered']
    r = S.put_on(sim, r)
    how, pull = r.get('grab_with'), r.get('pull_local')
    home = r['want_local']
    S.press(sim, how, True)
    r = S.fresh(r, until=lambda x: x.get('grabbed'), most=0.8)
    lines = [f'grabbed {r.get("grabbed")}']
    if pull:
        sim.glide('left', pos=tuple(pull[0]), quat=S.qmul(tuple(pull[1]), S.F_INV), seconds=0.3, mode='body')
        r = S.fresh(r, until=lambda x: (x.get('rack') or 0.0) >= 0.9 * (x.get('travel') or 1.0), most=1.0)
        lines.append(f'pulled {r.get("rack")}')
        sim.shot(str(out / f'{n}_pulled'), eyes='left', timeout=30)
        sim.glide('left', pos=tuple(home[0]), quat=S.qmul(tuple(home[1]), S.F_INV), seconds=0.3, mode='body')
        r = S.fresh(r, until=lambda x: (x.get('rack') or 0.0) <= 0.1 * (x.get('travel') or 1.0), most=1.0)
        lines.append(f'home {r.get("rack")}')
    S.press(sim, how, False)
    time.sleep(0.4)
    sim.hand('left', **S.REST)
    time.sleep(0.6)
    sim.shot(str(out / f'{n}_home'), eyes='left', timeout=30)
    return lines


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rec = sys.argv[2]
    steps = (sys.argv[3] if len(sys.argv) > 3 else 'rest,pump').split(',')
    sim = Sim(D.game_pid(), focus_away=True)
    D.cmd(f'weapon {rec}', wait='active weapon', timeout=40)
    sim.hands_follow_head(True)
    sim.head(0, -10)
    sim.hand('right', **SIDE)
    S.press(sim, 'grip+trigger', False)
    sim.hand('left', **S.REST)
    time.sleep(1.2)
    shots = []
    for n, step in enumerate(steps):
        if step == 'rest':
            sim.shot(str(out / f'{n}_rest'), eyes='left', timeout=30)
            shots.append(f'{n}_rest')
        elif step == 'pump':
            print(' '.join(pump(sim, out, n)), flush=True)
            shots += [f'{n}_pulled', f'{n}_home']
        elif step == 'empty':
            print(D.cmd('ammo 0').strip().splitlines()[-1], flush=True)
            time.sleep(1.5)
            sim.shot(str(out / f'{n}_empty'), eyes='left', timeout=30)
            shots.append(f'{n}_empty')
        elif step == 'r3':
            sim.tap('right', 'stick', hold=0.15)
            time.sleep(1.0)
            sim.shot(str(out / f'{n}_r3'), eyes='left', timeout=30)
            shots.append(f'{n}_r3')
        elif step == 'wait':
            time.sleep(1.5)
            sim.shot(str(out / f'{n}_wait'), eyes='left', timeout=30)
            shots.append(f'{n}_wait')
    from PIL import Image
    ims = []
    for name in shots:
        f = out / f'{name}_L.jpg'
        if f.exists():
            im = Image.open(f).crop((500, 500, 2080, 1900))
            ims.append(im.resize((im.width * 2 // 5, im.height * 2 // 5)))
    if ims:
        sheet = Image.new('RGB', (ims[0].width * len(ims), ims[0].height))
        for k, im in enumerate(ims):
            sheet.paste(im, (k * im.width, 0))
        sheet.save(out / 'sheet.jpg', quality=88)
    print('shots', ' '.join(shots), flush=True)


if __name__ == '__main__':
    main()
