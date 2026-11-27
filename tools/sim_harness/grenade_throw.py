"""grenade_throw.py OUT [N] [--speed S]: grenades thrown as a player throws them, in the running game (devgame.py
start): weapons put away, the left hand on the belt's nearest grenade holder (log/reload_reach.json, kind "grenade",
with "reach 1"), the grip closed (the grenade in the hand), the left stick clicked (the thumb pulls the pin), a swing
forward and up over ~0.2 s, the grip opened at its end. Stills of the flight and of the blast (OUT/sheet.jpg), and the
port's lines for each throw: the release speed, the flight's landing, the detonation."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402
from simxr import Sim  # noqa: E402

REST = dict(pos=(-.25, -.55, -.15), ypr=(0, 0, 0), mode='body')


def grenade_reach(after=None, most=2.0):
    return S.fresh(after, until=lambda x: x.get('kind') == 'grenade' and 'want_local' in x, most=most)


def take(sim):
    """The left hand onto the nearest grenade holder, the grip closed; the account once the hand holds one."""
    for attempt in range(3):
        r = grenade_reach(most=4.0)
        if r.get('kind') != 'grenade' or 'want_local' not in r:
            print('no grenade holder in reach:', r.get('kind'), r.get('why'), flush=True)
            return None
        r = S.put_on(sim, r)
        print(f'at {r.get("weapon")}: palm {r.get("near", {}).get("d")} m (radius {r.get("radius")}) {r.get("why")}', flush=True)
        sim.grip('left', 1.0)
        r = S.fresh(r, until=lambda x: x.get('grabbed'), most=1.5)
        if r.get('grabbed'):
            return r
        print('nothing taken:', r.get('why'), flush=True)
        sim.grip('left', 0.0)
        sim.hand('left', **REST)
        time.sleep(0.8)
    return None


def swing(sim, speed):
    """Back by the shoulder, then forward and up through ~0.6 m at about `speed` m/s; the grip opened at the end."""
    a, b = (-.22, -.05, .15), (-.18, .20, -.50)
    sim.hand('left', pos=a, ypr=(0, 30, 0), mode='body')
    time.sleep(0.5)
    length = sum((q - p) ** 2 for p, q in zip(a, b)) ** 0.5
    steps = max(6, int(length / speed / 0.01))
    for k in range(1, steps + 1):
        f = k / steps
        sim.hand('left', pos=tuple(p + (q - p) * f for p, q in zip(a, b)), ypr=(0, 30 - 50 * f, 0), mode='body')
        time.sleep(0.01)
    sim.grip('left', 0.0)
    time.sleep(0.05)
    sim.hand('left', pos=(-.18, .10, -.45), ypr=(0, -20, 0), mode='body')


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    args = [a for a in sys.argv[2:] if not a.startswith('--')]
    n = int(args[0]) if args else 1
    speed = float(sys.argv[sys.argv.index('--speed') + 1]) if '--speed' in sys.argv else 6.0
    sim = Sim(D.game_pid(), focus_away=True)
    sim.hands_follow_head(True)
    sim.head(0, 5)
    D.cmd('holster', wait='[dev] holster', timeout=10)
    D.cmd('reach 1', wait='[dev] reach', timeout=10)
    sim.hand('left', **REST)
    time.sleep(1.5)
    shots = []
    for k in range(n):
        start = D.log_size()
        r = take(sim)
        if not r:
            break
        sim.hand('left', pos=(-.20, -.25, -.30), ypr=(0, 0, 0), mode='body')
        time.sleep(0.5)
        sim.tap('left', 'stick', hold=0.12)
        r = S.fresh(r, until=lambda x: 'pin True' in (x.get('why') or ''), most=3.0)
        print('pin:', r.get('why'), flush=True)
        swing(sim, speed)
        for i, delay in enumerate((0.05, 0.25, 0.4, 0.8, 1.2)):
            time.sleep(delay)
            name = f'{k}_{i}'
            try:
                sim.shot(str(out / name), eyes='left', timeout=30)
            except TimeoutError:
                pass
            shots.append(name)
        time.sleep(2.0)
        for line in D.log_since(start).splitlines():
            if '[belt] grenade' in line or 'flight:' in line:
                print('  ', line.strip()[:200], flush=True)
        sim.hand('left', **REST)
        time.sleep(1.0)
    D.cmd('reach 0', wait='[dev] reach', timeout=10)
    from PIL import Image
    ims = []
    for name in shots:
        f = out / f'{name}_L.jpg'
        if f.exists():
            im = Image.open(f)
            ims.append(im.resize((im.width // 4, im.height // 4)))
    if ims:
        cols = 5
        rows = (len(ims) + cols - 1) // cols
        sheet = Image.new('RGB', (ims[0].width * cols, ims[0].height * rows))
        for k, im in enumerate(ims):
            sheet.paste(im, ((k % cols) * im.width, (k // cols) * im.height))
        sheet.save(out / 'sheet.jpg', quality=85)


if __name__ == '__main__':
    main()
