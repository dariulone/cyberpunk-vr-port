"""break_cycle.py OUT RECORD SHOTS: a break-action fired SHOTS times, opened (right stick), tipped muzzle-up so the spent
cases slide out, a still at each step and the port's reload lines; the gun is left open for the hand's reload."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
from simxr import Sim  # noqa: E402

SIDE = dict(pos=(.12, -.22, -.45), ypr=(70, -40, 0), mode='body')
TIP = dict(pos=(.12, -.10, -.40), ypr=(70, 35, 0), mode='body')


def ammo_line(text):
    return [ln for ln in text.splitlines() if '[dev] ammo' in ln or 'trg' in ln][-1:] or ['-']


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rec, shots = sys.argv[2], int(sys.argv[3])
    sim = Sim(D.game_pid(), focus_away=True)
    D.cmd(f'weapon {rec}', wait='active weapon', timeout=40)
    sim.hands_follow_head(True)
    sim.head(0, -10)
    sim.hand('right', **SIDE)
    sim.trigger('right', 0.0)
    time.sleep(1.2)
    print('before', D.cmd('ammo 2').strip().splitlines()[-1], flush=True)
    time.sleep(0.8)
    for _ in range(shots):
        sim.trigger('right', 1.0)
        time.sleep(0.15)
        sim.trigger('right', 0.0)
        time.sleep(0.9)
    sim.shot(str(out / '0_fired'), eyes='left', timeout=30)
    sim.tap('right', 'stick', hold=0.15)
    time.sleep(0.8)
    sim.shot(str(out / '1_open'), eyes='left', timeout=30)
    sim.glide('right', pos=TIP['pos'], ypr=TIP['ypr'], seconds=0.25, mode='body')
    time.sleep(1.2)
    sim.shot(str(out / '2_tipped'), eyes='left', timeout=30)
    sim.glide('right', pos=SIDE['pos'], ypr=SIDE['ypr'], seconds=0.4, mode='body')
    time.sleep(1.0)
    sim.shot(str(out / '3_back'), eyes='left', timeout=30)
    from PIL import Image
    ims = []
    for name in ('0_fired', '1_open', '2_tipped', '3_back'):
        f = out / f'{name}_L.jpg'
        if f.exists():
            im = Image.open(f).crop((400, 300, 2080, 1900))
            ims.append(im.resize((im.width * 2 // 5, im.height * 2 // 5)))
    sheet = Image.new('RGB', (ims[0].width * len(ims), ims[0].height))
    for k, im in enumerate(ims):
        sheet.paste(im, (k * im.width, 0))
    sheet.save(out / 'sheet.jpg', quality=88)


if __name__ == '__main__':
    main()
