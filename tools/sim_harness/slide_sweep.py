"""slide_sweep.py OUT [RECORD ...]: every gun whose slide or handle the hand works, in the running game (devgame.py start):
given and drawn ("weapon" command), its slide taken as slide_grab.py takes it (on the grip, pressed, a full stroke
back, let go), then once more with the wrist turned 70 degrees off the recorded grip (a player's hand is not the
recording's). One line per gun in OUT/sweep.txt and a folder of stills per gun."""
import math
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402
from simxr import Sim  # noqa: E402

GUNS = ['Items.Preset_Silverhand_3516', 'Items.Preset_Unity_Default', 'Items.Preset_Nue_Default',
        'Items.Preset_Liberty_Default', 'Items.Preset_Kenshin_Default', 'Items.Preset_Lexington_Default', 'Items.Preset_Crusher_Default',
        'Items.Preset_Pozhar_Default', 'Items.Preset_Grit_Default', 'Items.Preset_Metel_Default', 'Items.Preset_Masamune_Default',
        'Items.Preset_Copperhead_Default', 'Items.Preset_Umbra_Default', 'Items.Preset_Sidewinder_Default', 'Items.Preset_Ajax_Default',
        'Items.Preset_Kyubi_Default', 'Items.Preset_Kolac_Default', 'Items.Preset_Osprey_Default', 'Items.Preset_Grad_Default',
        'Items.Preset_Sor22_Default', 'Items.Preset_Nekomata_Default', 'Items.Preset_Shingen_Default', 'Items.Preset_Warden_Default',
        'Items.Preset_Saratoga_Default', 'Items.Preset_Guillotine_Default', 'Items.Preset_Pulsar_Default',
        'Items.Preset_Defender_Default', 'Items.Preset_MA70_Default']


def turned(deg):
    h = math.radians(deg) / 2
    return (0.0, math.sin(h), 0.0, math.cos(h))


def twist_test(sim, deg):
    """On the grip with the wrist turned `deg` about the controller's up: taken?"""
    sim.hand('left', **S.REST)
    r = S.fresh(None, until=lambda x: 'want_local' in x and not x.get('grabbed'), most=1.5)
    if 'want_local' not in r:
        return None, r
    r = S.put_on(sim, r, turned(deg))
    how = r.get('grab_with')
    S.press(sim, how, True)
    r2 = S.fresh(r, until=lambda x: x.get('grabbed'), most=0.6)
    S.press(sim, how, False)
    S.fresh(r2, until=lambda x: not x.get('grabbed'), most=0.8)
    sim.hand('left', **S.REST)
    return bool(r2.get('grabbed')), r


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    guns = [a for a in sys.argv[2:] if not a.startswith('--')] or GUNS
    t_all = time.time()
    sim = Sim(D.game_pid(), focus_away=True)
    rows = []
    for rec in guns:
        t_gun = time.time()
        r = D.cmd(f'weapon {rec}', wait='active weapon', timeout=40)
        drawn = next((ln.split('active weapon ')[1].split(' (')[0] for ln in r.splitlines() if 'active weapon' in ln), '-')
        if drawn != rec:
            rows.append(f'{rec}: not drawn (active {drawn})')
            print(rows[-1], flush=True)
            continue
        lines, grabbed = S.run(out / rec.split('.')[-1], shots='--shots' in sys.argv)
        stroke = next((ln for ln in lines if ln.startswith('pulled:')), '')
        rack = stroke.split('rack=')[1].split(' ')[0] if 'rack=' in stroke else '-'
        back = next((ln for ln in lines if ln.startswith('let go:')), '')
        twisted, tr = twist_test(sim, 70)
        n = tr.get('near') or {}
        rows.append(f'{rec}: {lines[0].split(" (")[0]} [{time.time() - t_gun:.1f} s] | exact: grabbed {grabbed}, stroke {rack} of {tr.get("travel")}, '
                    f'let go {back.split("grabbed=")[1].split(" ")[0] if "grabbed=" in back else "-"} | wrist 70: grabbed {twisted} '
                    f'(angle {n.get("angle")}, d {n.get("d")})')
        print(rows[-1], flush=True)
        (out / 'sweep.txt').write_text('\n'.join(rows), encoding='utf-8')
    rows.append(f'{len(guns)} guns in {time.time() - t_all:.0f} s')
    print(rows[-1], flush=True)
    (out / 'sweep.txt').write_text('\n'.join(rows), encoding='utf-8')


if __name__ == '__main__':
    main()
