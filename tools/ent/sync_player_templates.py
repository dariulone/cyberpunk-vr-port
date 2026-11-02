"""The player templates' sources against the game's own: what the game ships, the port keeps.

    py -3.13 -B tools/ent/sync_player_templates.py check <vanilla json folder>
    py -3.13 -B tools/ent/sync_player_templates.py apply <vanilla json folder>

<vanilla json folder> holds the game's templates serialized by cp77tools, each as <file name>.ent.json.

WHY. mods/archive/source/raw is the one source of the player templates (make_lobby_scene.pack_sources builds the pack
from it), and it was taken from game 2.31. A patch changes those templates too, and nothing compared them: on 2.5 the
pack rebuilt from these sources dropped the game's 18 dual wield animation sets and put player_ranged.animgraph back
in place of player_dualwield.animgraph, so with two guns out each hand kept the pose of another state and no error
said so anywhere. Run `check` on every patch day: it lists each field the game and the source disagree on.

WHAT `apply` TAKES FROM THE GAME: the fields in SYNC, in the components named there, both in the template's components
and in its compiled package; and the dependencies the game lists that the source lacks. Everything else stays: the
port's own components and its slot edits are not the game's to overwrite, and `check` keeps reporting the differences
it does not take so that a new one is seen.
"""
import io
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RAW = ROOT / 'mods/archive/source/raw'
TEMPLATES = ['base/characters/entities/player/player_ma_fpp.ent', 'base/characters/entities/player/player_wa_fpp.ent',
             'base/characters/entities/player/replacer/player_wa_fpp_replacer.ent',
             'base/characters/entities/player/replacer/tutorial_replacer.ent',
             'ep1/characters/entities/player/player_ma_fpp_ep1.ent',
             'ep1/characters/entities/player/player_wa_fpp_ep1.ent']
# component name -> what of it follows the game
SYNC = {'root': ['animations'], 'AnimGraphResourceContainer': ['animGraphLookupTable'], 'HitPhysicalQueryMesh': ['filterData']}
# The fields a serialized file numbers for itself.
IGNORE = {'HandleId', 'HandleRefId', 'id', 'Flags', 'BufferId'}


def norm(value):
    if isinstance(value, dict):
        return {k: norm(v) for k, v in value.items() if k not in IGNORE}
    if isinstance(value, list):
        return [norm(v) for v in value]
    return value


def differences(ours, game, path, out):
    if type(ours) is not type(game):
        out.append((path, game, ours))
    elif isinstance(ours, dict):
        for key in sorted(set(ours) | set(game)):
            if key in ours and key in game:
                differences(ours[key], game[key], path + '/' + key, out)
            else:
                out.append((path + '/' + key, game.get(key, '<absent>'), ours.get(key, '<absent>')))
    elif isinstance(ours, list):
        if len(ours) != len(game):
            out.append((path, 'length %d' % len(game), 'length %d' % len(ours)))
        else:
            for i, (a, b) in enumerate(zip(ours, game)):
                differences(a, b, '%s/%d' % (path, i), out)
    elif ours != game:
        out.append((path, game, ours))


def take(ours, game):
    """`game`'s values into `ours`, where the two are the same shape: this file's handles keep their numbers. A list
    of another length is taken whole, and may hold no handle."""
    if isinstance(ours, dict) and isinstance(game, dict):
        for key, value in game.items():
            if key in IGNORE:
                continue
            if key in ours and isinstance(value, (dict, list)) and type(ours[key]) is type(value):
                if isinstance(value, list) and len(ours[key]) != len(value):
                    if 'HandleId' in json.dumps(value):
                        raise SystemExit('a list of handles differs in length: not taken')
                    ours[key] = value
                else:
                    take(ours[key], value)
            else:
                ours[key] = value
    elif isinstance(ours, list) and isinstance(game, list):
        for i, value in enumerate(game):
            if isinstance(value, (dict, list)):
                take(ours[i], value)
            else:
                ours[i] = value


def named(chunks):
    return {c['name']['$value']: c for c in chunks if isinstance(c, dict) and isinstance(c.get('name'), dict)}


def walk_template(doc):
    """The two lists of a template's components: the plain one and its compiled package's."""
    root = doc['Data']['RootChunk']
    yield 'components', root.get('components') or []
    yield 'compiled', ((root.get('compiledData') or {}).get('Data') or {}).get('Chunks') or []


def process(rel, vanilla_dir, apply):
    path = RAW / (rel + '.json')
    with io.open(path, encoding='utf-8', newline='') as handle:
        text = handle.read()
    doc = json.loads(text)
    game = json.load(io.open(vanilla_dir / (Path(rel).name + '.json'), encoding='utf-8'))
    print('==', Path(rel).name)
    changed = 0
    for (where, ours), (_, theirs) in zip(walk_template(doc), walk_template(game)):
        mine, vanilla = named(ours), named(theirs)
        for name, component in vanilla.items():
            if name not in mine:
                print('  %s: the game has a component the source lacks: %s' % (where, name))
                continue
            out = []
            differences(norm(mine[name]), norm(component), '', out)
            for field, in_game, in_source in out:
                top = field.split('/')[1]
                synced = top in SYNC.get(name, ())
                print('  %s %s%s  game %s  source %s%s' % (where, name, field, str(in_game)[:48], str(in_source)[:48],
                                                           '' if synced else '   [the port\'s, kept]'))
                changed += synced
            if apply:
                for key in SYNC.get(name, ()):
                    if key in component:
                        if key not in mine[name]:
                            mine[name][key] = component[key]
                        else:
                            take(mine[name][key], component[key])
    root, theirs = doc['Data']['RootChunk'], game['Data']['RootChunk']
    have = {d['DepotPath']['$value'] for d in root['resolvedDependencies']}
    missing = [d for d in theirs['resolvedDependencies'] if d['DepotPath']['$value'] not in have]
    print('  dependencies the source lacks: %d' % len(missing))
    if apply and (changed or missing):
        root['resolvedDependencies'].extend(missing)
        # Written as the file was, its indent, line endings and final newline kept.
        newline = '\r\n' if '\r\n' in text else '\n'
        second = text.split('\n', 2)[1]
        indent = (len(second) - len(second.lstrip(' '))) or 2
        out = json.dumps(doc, indent=indent, ensure_ascii=False).replace('\n', newline)
        if text.endswith(newline):
            out += newline
        with io.open(path, 'w', encoding='utf-8', newline='') as handle:
            handle.write(out)
        print('  written')
    return changed + len(missing)


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ('check', 'apply'):
        raise SystemExit(__doc__)
    apply = sys.argv[1] == 'apply'
    total = sum(process(rel, Path(sys.argv[2]), apply) for rel in TEMPLATES)
    print('fields and dependencies behind the game: %d' % total)


if __name__ == '__main__':
    main()
