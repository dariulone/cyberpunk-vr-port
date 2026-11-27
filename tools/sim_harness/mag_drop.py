"""mag_drop.py OUT RECORD [RECORD ...]: each gun given and drawn in the running game (devgame.py start), held side-on
below the eyes, a still, the magazine let go of with B (the reload's release button), a still a second later -- the
gun's model with its magazine out (hatch covers, holders, bolts)."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
from simxr import Sim  # noqa: E402

SIDE = dict(pos=(.05, -.25, -.40), ypr=(75, -20, 0), mode='body')


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    sim = Sim(D.game_pid(), focus_away=True)
    names = []
    for rec in sys.argv[2:]:
        r = D.cmd(f'weapon {rec}', wait='active weapon', timeout=30)
        ok = any(rec in ln for ln in r.splitlines() if 'active weapon' in ln)
        sim.hands_follow_head(True)
        sim.head(0, -15)
        sim.hand('right', **SIDE)
        sim.hand('left', pos=(-.30, -.70, .20), ypr=(0, 0, 0), mode='body')
        time.sleep(1.5)
        short = rec.split('.')[-1]
        for label in ('in', 'out'):
            if label == 'out':
                sim.tap('right', 'b', hold=0.15)
                time.sleep(1.5)
            try:
                sim.shot(str(out / f'{short}_{label}'), eyes='left', timeout=30)
            except TimeoutError:
                pass
            names.append(f'{short}_{label}')
        print(rec, 'drawn' if ok else 'NOT DRAWN', flush=True)
    from PIL import Image, ImageDraw
    tiles = []
    for n in names:
        f = out / f'{n}_L.jpg'
        if f.exists():
            im = Image.open(f).crop((400, 600, 2200, 2200)).resize((540, 480))
            ImageDraw.Draw(im).text((8, 8), n, fill=(255, 255, 0))
            tiles.append(im)
    if tiles:
        cols = 4
        rows = (len(tiles) + cols - 1) // cols
        sheet = Image.new('RGB', (540 * cols, 480 * rows))
        for k, im in enumerate(tiles):
            sheet.paste(im, ((k % cols) * 540, (k // cols) * 480))
        sheet.save(out / 'sheet.jpg', quality=85)


if __name__ == '__main__':
    main()
