"""THE CYCLE'S SOUNDS OFF THE SHOT, for the guns whose action is worked by hand in VR.

    py -3.13 -B tools/strip_cycle_sounds.py [--pack]

WHY. After every shot the game plays the weapon's `add_cycle_round` (and its _01 / ironsight variants): the pump
racked by the left hand, the Grad's bolt lever worked -- and their sounds ride that animation as sound events of the
weapon's own FPP set (pwa_). In VR the player works the action, and the reload module plays the same sounds at the
stroke (reload/shotgun.lua pumpOpen / pumpClose, reload.lua slidePull / slideRelease / eject). With the events left
in, the whole pump or lever is heard right after the shot, before the hand has moved.

WHICH. Every pwa_ firearm set was scanned (2026-10-31): only these three carry sounds in their cycle animations --
the two pump shotguns and the Grad. Only the cycle's sound events go; the reload's, the equip's and the rest stay,
and so does the animation itself.

WHERE. The game's set is overridden by a copy in the pack under its own depot path (the pack's own wins over the
game's), and the left-handed copy (vrport\\lefthanded\\<set>_left.anims, tools/lefthanded_anims/generate.py) is
stripped in place -- it carries the same events, read back from the glTF. The generator exports the pack's own copy
of a set when there is one, so the stale export of each set is removed to make its next run start from this one.
"""
import json
import re
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'vr_lobby_assets'))
import make_lobby_scene as MS  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
WORK = MS.ROOT / 'build' / 'strip-cycle-sounds'
LEFT_WORK = MS.ROOT / 'build' / 'lefthanded_anims' / 'work' / 'src'
SETS = (
    'base\\animations\\weapon\\firearms\\shotgun\\budget_carnage\\pwa_w_shotgun__budget_carnage.anims',
    'base\\animations\\weapon\\firearms\\shotgun\\constitutional_tactician\\pwa_w_shotgun__constitutional_arms_tactician.anims',
    'base\\animations\\weapon\\firearms\\rifle_sniper\\techtronika_grad\\pwa_w_rifle_sniper__techtronika_grad.anims',
)
CYCLE = re.compile(r'^add_cycle_round')
V = '$value'


def strip(doc):
    """Sound events off the cycle animations; returns how many went."""
    gone = 0
    for entry in doc['Data']['RootChunk']['animations']:
        entry = entry.get('Data', entry)
        anim = entry['animation'].get('Data', entry['animation'])
        if not CYCLE.match(anim['name'][V]):
            continue
        events = entry.get('events')
        events = events.get('Data', events) if isinstance(events, dict) else None
        if not events or not events.get('events'):
            continue
        kept = [e for e in events['events'] if 'Sound' not in e.get('Data', e).get('$type', '')]
        gone += len(events['events']) - len(kept)
        events['events'] = kept
    return gone


def convert(cooked, depot):
    """A cooked set -> stripped -> the pack at `depot`."""
    tmp = WORK / 'json'
    if tmp.exists():
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)
    src = WORK / 'cooked'
    if src.exists():
        shutil.rmtree(src)
    src.mkdir(parents=True)
    shutil.copyfile(cooked, src / cooked.name)
    MS.run(MS.CLI, 'convert', 'serialize', src / cooked.name, '-o', tmp)
    path = tmp / (cooked.name + '.json')
    doc = json.loads(path.read_text(encoding='utf-8'))
    gone = strip(doc)
    name = depot.rsplit('\\', 1)[-1]
    out = WORK / 'out'
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    (out / (name + '.json')).write_text(json.dumps(doc, indent=2), encoding='utf-8')
    target = MS.PACK / depot.rsplit('\\', 1)[0].replace('\\', '/')
    target.mkdir(parents=True, exist_ok=True)
    MS.run(MS.CLI, 'convert', 'deserialize', out, '-o', target)
    print('%-58s %d sound events off the cycle' % (name, gone))
    return gone


def main():
    raw = WORK / 'raw'
    if raw.exists():
        shutil.rmtree(raw)
    for depot in SETS:
        own = MS.PACK / depot.replace('\\', '/')
        if not own.exists():
            MS.run(MS.CLI, 'unbundle', GAME / 'archive' / 'pc' / 'content' / 'basegame_1_engine.archive', '-o', raw,
                   '-r', re.escape(depot) + '$')
            own = raw / depot.replace('\\', '/')
        convert(own, depot)
        stem = depot.rsplit('\\', 1)[-1][:-len('.anims')]
        left = 'vrport\\lefthanded\\' + stem + '_left.anims'
        if (MS.PACK / left.replace('\\', '/')).exists():
            convert(MS.PACK / left.replace('\\', '/'), left)
        stale = LEFT_WORK / (depot.replace('\\', '/') + '.glb')
        for f in (stale, stale.with_name(stale.name + '.fallback.json'), LEFT_WORK / depot.replace('\\', '/')):
            f.unlink(missing_ok=True)
    if '--pack' in sys.argv[1:]:
        MS.pack_archive()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
