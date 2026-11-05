"""THE LEFT-HANDED WEAPON ANIMATIONS: the game's own, mirrored onto the other hand.

    py -3.13 -B tools/lefthanded_anims/generate.py                        all of it
    py -3.13 -B tools/lefthanded_anims/generate.py scan export mirror build xl pack   any of the steps, in order
    ... --only lexington,ajax                                             the sets whose path has any of these

THE WEAPON ALONE IN THE LEFT HAND IS A SINGLE WIELD. The player's ranged graph (player_dualwield.animgraph, the
player's "ranged" graph since 2.5) plays the single-wield animations -- idlepose_shoulder, add_recoil_single_shot_
shoulder, equip, first_equip, reload ... -- looked up, like every animation of the player, in the sets the player's
root component has active. A set is active while all of its variables are (animAnimSetupEntry.variableNames), the
lowest priority number winning. A weapon's own set is keyed on the item's friendlyName (w_handgun_militech_lexington),
which the engine sets as a variable on the owner when the item is attached -- with _left appended when the slot is
WeaponLeft (Cyberpunk2077.exe 2.5, RVA 0x219A300); the class sets (w_base_handgun ...) on a name no item record
holds, set for the weapon whichever hand it is in, and they carry the camera's recoil only. With the weapon alone in
the left hand the plugin hands the single-wield branch to it (src/Hooks/LeftHandedAnimation.cpp); what that branch
plays has to be of the left hand.

So the left hand's single-wield sets are generated from the right hand's: every player weapon set (pma_/pwa_,
firearms, and the default set every weapon falls back on) is exported, mirrored across the body (mirror_anims.cpp:
the hold, the squeeze of each round, the draw, the first draw's flourish, the reload, as the game animates them for
the right hand) under the same animation names, and built back into a set (AnimTool). Each is given to the player
templates through ArchiveXL (`animations:` in cyberpunkvrport.archive.xl, on the root component) under the
variables of the set it mirrors -- a weapon's own as the left slot names it (_left) -- plus vr_left_handed, the
port's own, set while the weapon is alone in the left hand (mods/redscript/CyberpunkVRPort_LeftHanded), and a
priority a step ahead of the original's.
The game's graph, the game's animations, the game's choice of them: only the hand is the other one.

The player templates read are the port's own (the pack's), since they are the ones the game loads.
"""
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(HERE), str(ROOT / 'tools/vr_lobby_assets')]
import make_lobby_scene as S  # noqa: E402

GAME_EXE = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe')
WORK = ROOT / 'build/lefthanded_anims/work'
TOOL = ROOT / 'build/lefthanded_anims/AnimTool/AnimTool.dll'
TOOL_PROJECT = HERE / 'AnimTool'
MIRROR = ROOT / 'build/lefthanded_anims/mirror_anims.exe'
XL = ROOT / 'mods/archive/cyberpunkvrport.archive.xl'
DEPOT = 'vrport\\lefthanded\\'
VARIABLE = 'vr_left_handed'
PRIORITY_STEP = 4            # ahead of the set mirrored, behind nothing that was ahead of it
XL_BEGIN = '# >>> left-handed weapon animations (tools/lefthanded_anims/generate.py)'
XL_END = '# <<< left-handed weapon animations'


def player_templates():
    roots = [S.PACK / 'base/characters/entities/player', S.PACK / 'ep1/characters/entities/player']
    return sorted(p for r in roots if r.exists() for p in r.rglob('*.ent'))


def depot_of(path):
    return str(path.relative_to(S.PACK)).replace('/', '\\')


def mirrored(set_path):
    """A set worth mirroring: a firearm's, or the default every weapon falls back on -- never a dual wield set."""
    low = set_path.lower()
    name = low.rsplit('\\', 1)[-1]
    if '\\animations\\weapon\\' not in low or '\\dual_wield\\' in low:
        return False
    # the default is pma_default_weapon on the man and default_weapon on the woman
    return '\\firearms\\' in low or name in ('pma_default_weapon.anims', 'default_weapon.anims')


def left_name(set_path):
    stem = set_path.rsplit('\\', 1)[-1][:-len('.anims')]
    return DEPOT + stem + '_left.anims'


def left_variable(variable):
    """The variable as it is set for the weapon on the left slot: a weapon's own name gets _left, a class's does not."""
    return variable if variable.startswith('w_base_') else variable + '_left'


def only_filter():
    for a in sys.argv[1:]:
        if a.startswith('--only'):
            value = a.split('=', 1)[1] if '=' in a else sys.argv[sys.argv.index(a) + 1]
            return [s.strip().lower() for s in value.split(',') if s.strip()]
    return None


def load_sets():
    sets = json.loads((WORK / 'sets.json').read_text(encoding='utf-8'))
    only = only_filter()
    if only:
        sets = {k: v for k, v in sets.items() if any(o in k.lower() for o in only)}
    return sets


def cmd_scan():
    """Every mirrored set of every player template's root component, with its variables and priority."""
    tmp = WORK / 'templates'
    if tmp.exists():
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)
    sets = {}
    for ent in player_templates():
        S.run(S.CLI, 'convert', 'serialize', ent, '-o', tmp)
        doc = json.loads((tmp / (ent.name + '.json')).read_text(encoding='utf-8'))
        roots = [c for c in doc['Data']['RootChunk'].get('components') or []
                 if c.get('$type') == 'entAnimatedComponent' and c.get('name', {}).get('$value') == 'root']
        for root in roots:
            for e in root['animations'].get('gameplay') or []:
                path = e['animSet']['DepotPath']['$value']
                if not mirrored(path):
                    continue
                key = json.dumps([[v['$value'] for v in e.get('variableNames') or []], e['priority']])
                sets.setdefault(path, {}).setdefault(key, []).append(depot_of(ent))
    (WORK / 'sets.json').write_text(json.dumps(sets, indent=1), encoding='utf-8')
    print(len(sets), 'sets on', len(player_templates()), 'player templates')


def build_tool():
    if not TOOL.exists() or TOOL.stat().st_mtime < (TOOL_PROJECT / 'Program.cs').stat().st_mtime:
        S.run('dotnet', 'build', TOOL_PROJECT, '-c', 'Release', '-o', TOOL.parent)


def cmd_export():
    """Each set as the game loads it (the pack's own over the game's) -> cooked copy + glTF, deltas left as deltas."""
    build_tool()
    src = WORK / 'src'
    game, local = [], []
    for path in load_sets():
        out = src / path.replace('\\', '/')
        if (out.parent / (out.name + '.glb')).exists():
            continue
        own = S.PACK / path.replace('\\', '/')
        if own.exists():
            out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(own, out)
            local.append(str(out))
        else:
            game.append(path)
    for batch in (game, local):
        if batch:
            print(S.run('dotnet', TOOL, 'export', GAME_EXE, src, *batch).strip())


def cmd_mirror():
    """Each exported set mirrored onto the left hand (mirror_anims.exe; a set without animations writes nothing)."""
    if not MIRROR.exists() or MIRROR.stat().st_mtime < (HERE / 'mirror_anims.cpp').stat().st_mtime:
        S.run('cmd', '/c', HERE / 'build_mirror.cmd')
    src, dst = WORK / 'src', WORK / 'mirrored'
    dst.mkdir(parents=True, exist_ok=True)
    jobs = []
    for path in load_sets():
        glb = src / (path.replace('\\', '/') + '.glb')
        out = dst / (path.rsplit('\\', 1)[-1] + '.glb')
        out.unlink(missing_ok=True)
        jobs.append(f'{glb}|{out}')
    job_file = WORK / 'mirror_jobs.txt'
    job_file.write_text('\n'.join(jobs), encoding='utf-8')
    for line in S.run(MIRROR, '--list', job_file).splitlines():
        if 'no animations' in line:
            print(line)
    print('mirrored', len(list(dst.glob('*.glb'))), 'sets')


def cmd_build():
    jobs = []
    for path in load_sets():
        glb = WORK / 'mirrored' / (path.rsplit('\\', 1)[-1] + '.glb')
        if not glb.exists():
            continue
        cooked = WORK / 'src' / path.replace('\\', '/')
        out = S.PACK / left_name(path).replace('\\', '/')
        jobs.append(f'{cooked}|{glb}|{out}')
    job_file = WORK / 'build_jobs.txt'
    job_file.write_text('\n'.join(jobs), encoding='utf-8')
    build_tool()
    print(S.run('dotnet', TOOL, 'build', GAME_EXE, job_file).strip()[-3000:])


def cmd_xl():
    """The animations block of the manifest: each left-handed set on the player templates of the set it mirrors."""
    lines = [XL_BEGIN,
             '# The game\'s single-wield weapon animations mirrored onto the left hand, on the player\'s root component',
             '# while the weapon is alone in the left hand (vr_left_handed, w_<weapon>_left).',
             'animations:']
    sets = json.loads((WORK / 'sets.json').read_text(encoding='utf-8'))
    for path in sorted(sets):
        if not (S.PACK / left_name(path).replace('\\', '/')).exists():
            continue
        for key, entities in sorted(sets[path].items()):
            variables, priority = json.loads(key)
            lines.append('  - entity:')
            lines += ['      - ' + e for e in sorted(set(entities))]
            lines.append('    component: root')
            lines.append('    set: ' + left_name(path))
            lines.append('    vars: [' + ', '.join([VARIABLE] + [left_variable(v) for v in variables]) + ']')
            lines.append('    priority: %d' % (priority - PRIORITY_STEP))
    lines.append(XL_END)
    text = XL.read_text(encoding='utf-8')
    block = '\n'.join(lines) + '\n'
    if XL_BEGIN in text:
        text = re.sub(re.escape(XL_BEGIN) + r'.*?' + re.escape(XL_END) + r'\n?', lambda m: block, text, flags=re.S)
    else:
        text = text.rstrip('\n') + '\n\n' + block
    XL.write_text(text, encoding='utf-8', newline='\n')
    print('manifest:', sum(1 for ln in lines if ln.startswith('  - entity')), 'entries')


def cmd_pack():
    S.pack_archive()


if __name__ == '__main__':
    WORK.mkdir(parents=True, exist_ok=True)
    order = ['scan', 'export', 'mirror', 'build', 'xl', 'pack']
    steps = [a for a in sys.argv[1:] if a in order] or order
    for step in steps:
        globals()['cmd_' + step]()
