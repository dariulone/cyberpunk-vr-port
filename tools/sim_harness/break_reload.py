"""break_reload.py OUT RECORD SHOTS [--load]: a break-action worked as a player works it, in the running game
(devgame.py start): two rounds in, fired SHOTS times, opened (right stick), tipped muzzle-up slowly so the spent cases
slide out, back level; with --load the left hand then takes shells from the belt pouch (grip on it), brings them
along each empty chamber's line and seats them (the magnet takes them), lets go, and the gun is closed (right stick).
A still at each step (OUT/sheet.jpg side by side) and the mechanism's state line from log/reload_reach.json ("why":
act, hinge angle, chambers l/s/e, shells held) at each."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402
from simxr import Sim  # noqa: E402

SIDE = dict(pos=(.12, -.22, -.45), ypr=(70, -40, 0), mode='body')
TIP = dict(pos=(.12, -.08, -.40), ypr=(70, 80, 0), mode='body')


class Run:
    def __init__(self, out):
        self.out = out
        self.sim = Sim(D.game_pid(), focus_away=True)
        self.shots = []
        self.n = 0

    def state(self, label):
        r = S.fresh(None, most=0.6)
        line = f'{label}: {r.get("why", "-")}'
        print(line, flush=True)
        return r

    def still(self, label):
        name = f'{self.n}_{label}'
        self.n += 1
        try:
            self.sim.shot(str(self.out / name), eyes='left', timeout=30)
        except TimeoutError:
            pass  # the simulator answers late now and then; the still is written all the same
        self.shots.append(name)
        self.state(label)

    def sheet(self):
        from PIL import Image
        ims = []
        for name in self.shots:
            f = self.out / f'{name}_L.jpg'
            if f.exists():
                im = Image.open(f).crop((400, 300, 2080, 1900))
                ims.append(im.resize((im.width * 2 // 7, im.height * 2 // 7)))
        if not ims:
            return
        cols = min(len(ims), 5)
        rows = (len(ims) + cols - 1) // cols
        sheet = Image.new('RGB', (ims[0].width * cols, ims[0].height * rows))
        for k, im in enumerate(ims):
            sheet.paste(im, ((k % cols) * im.width, (k // cols) * im.height))
        sheet.save(self.out / 'sheet.jpg', quality=88)


def go_to(sim, key, tries=6):
    """The left controller onto the pose `key` of the account (want_local / pull_local), the position's residual taken
    off until within 4 mm (the target is recomputed from the hand each time: the shell's turn follows it)."""
    r = S.fresh(None, until=lambda x: key in x, most=1.0)
    if key not in r:
        return r
    pos = list(r[key][0])
    for _ in range(tries):
        sim.hand('left', pos=tuple(pos), quat=S.qmul(tuple(r[key][1]), S.F_INV), mode='body')
        last = None
        for _ in range(12):
            r = S.fresh(r)
            cur = (r.get('current_local') or [None])[0]
            if last and cur and sum((a - b) ** 2 for a, b in zip(last, cur)) ** 0.5 < 0.001:
                break
            last = cur
        if key not in r:
            break
        err = [w - c for w, c in zip(r[key][0], r['current_local'][0])]
        if sum(e * e for e in err) ** 0.5 < 0.004:
            break
        pos = [p + e for p, e in zip(pos, err)]
    return r


def load_one(run):
    """Pouch, grip, along the nearest empty chamber's line, seated, let go. True when a chamber went live."""
    sim = run.sim
    r = S.fresh(None, until=lambda x: x.get('kind') == 'pouch' and 'want_local' in x, most=2.0)
    if r.get('kind') != 'pouch':
        print('no pouch target:', r.get('kind'), r.get('why'), flush=True)
        return False
    for attempt in range(3):
        r = S.put_on(sim, r)
        print(f'at the pouch: palm {r.get("near", {}).get("d")} m (radius {r.get("radius")})', flush=True)
        sim.grip('left', 1.0)
        seen = []
        t0 = time.time()
        while time.time() - t0 < 1.5:
            r = S.fresh(r, most=0.3)
            seen.append(f'{r.get("kind")}:{"held=1" in (r.get("why") or "")}')
            if r.get('kind') == 'chamber':
                break
        if r.get('kind') == 'chamber':
            break
        print('nothing taken:', ' '.join(seen[-8:]), r.get('why'), flush=True)
        sim.grip('left', 0.0)
        sim.hand('left', **S.REST)
        time.sleep(0.8)
        r = S.fresh(None, until=lambda x: x.get('kind') == 'pouch' and 'want_local' in x, most=2.0)
    if r.get('kind') != 'chamber':
        return False
    run.still('taken')
    r = go_to(sim, 'pull_local')
    run.still('at_mouth')
    r = go_to(sim, 'want_local')
    r = S.fresh(r, until=lambda x: x.get('kind') != 'chamber', most=1.0)
    run.still('seated')
    sim.grip('left', 0.0)
    time.sleep(0.4)
    sim.hand('left', **S.REST)
    time.sleep(0.5)
    return True


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rec, shots = sys.argv[2], int(sys.argv[3])
    run = Run(out)
    sim = run.sim
    D.cmd('reach 1')
    D.cmd(f'weapon {rec}', wait='active weapon', timeout=40)
    sim.hands_follow_head(True)
    sim.head(0, -10)
    sim.hand('right', **SIDE)
    sim.hand('left', **S.REST)
    sim.trigger('right', 0.0)
    sim.grip('left', 0.0)
    time.sleep(1.2)
    print(D.cmd('ammo 2').strip().splitlines()[-1], flush=True)
    time.sleep(0.8)
    run.still('loaded')
    for _ in range(shots):
        sim.trigger('right', 1.0)
        time.sleep(0.15)
        sim.trigger('right', 0.0)
        time.sleep(0.9)
    run.still('fired')
    sim.tap('right', 'stick', hold=0.15)
    time.sleep(0.8)
    run.still('open')
    # slowly: a fast turn up is the flick that closes the barrels
    sim.glide('right', pos=TIP['pos'], ypr=TIP['ypr'], seconds=1.2, mode='body')
    time.sleep(1.5)
    run.still('tipped')
    sim.glide('right', pos=SIDE['pos'], ypr=SIDE['ypr'], seconds=1.2, mode='body')
    time.sleep(0.8)
    run.still('level')
    if '--load' in sys.argv:
        for _ in range(3):
            if not load_one(run):
                break
            why = S.fresh(None, most=0.4).get('why') or ''
            if ' ch=l,l ' in f' {why} ' or 'ch=l,l' in why:
                break
        run.still('loaded_by_hand')
        sim.tap('right', 'stick', hold=0.15)
        time.sleep(1.0)
        run.still('closed')
        sim.glide('right', pos=(.12, -.22, -.45), ypr=(40, -20, 0), seconds=0.8, mode='body')
        time.sleep(0.8)
        run.still('moved')
    run.sheet()


if __name__ == '__main__':
    main()
