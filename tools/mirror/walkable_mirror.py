"""The walkable mirror: the game's interactive mirror scenes with the player left out of them.

A mirror in the game is a scene: "look in the mirror" plays an FPP animation on the player (the slide to the sink and
the lean, FOV 30), puts the player in a workspot at the sink, and on "leave" plays the way back (anim, FOV reset,
unmount, gameplay transition). In VR that animation is the camera being moved for the player, and the workspot is the
player who cannot walk. The rest of the scene is the mirror itself and stays: the device ForceON/ForceOFF, the prefab
variant, the TPP double spawned into the mirrors area, the hub (leave / hairdresser / facial), Johnny.

ONLY THE MIRRORS THE PLAYER TURNS ON: the interactive ones, whose choice hub is titled MIRROR_TITLE ("Mirror") -- V's
apartment (mq000_01_apartment, its mirror part only) and every scene in base\\open_world\\scenes\\mirrors (Judy's,
the motels, the penthouses, El Coyote ...). A story scene that stands V at a mirror (the prologue starts: the nomad
garage, the corpo restroom) has no such hub and is not copied: its animations are the game's, and CET binds no double
there either (mods/cet/CyberpunkVRPort_VRIK binds only while a "Mirror" hub with a leave choice is up).

This tool takes those scenes out of the game and writes the port's copies with, in the mirror's part of the graph only
(everything reachable from a MIRROR_TITLE choice):
  - the player's own events gone: FPP PlaySkAnim, IK, player-camera params, Unmount, GameplayTransition, the player's
    anim features and look-ats;
  - a section that lost them squeezed to SQUEEZED_MS (what is left in it -- sockets, audio, the double's anims, which
    the port overwrites with the player's pose anyway -- starts within it), so the hub is up at once and "leave"
    turns the mirror off at once;
  - the player's UseWorkspot nodes cut off (nothing reaches them);
  - the same for the TPP double of V (the actors spawned as Character.q110_tpp_male_v / _female_v): its body
    animation (the walk to the sink and back -- its placement holds the double at the sink, the reflection jumped onto
    the player only after it; without it the double stays where the scene spawned it, a few metres off, until CET
    teleports it), IK, look-ats, Unmount, GameplayTransition and its workspots. Its face animations ('head': the
    facial choices) stay. Its sections keep their length.
Everything else -- Johnny, the device, the mirrors area, the spawns, everything outside the mirror's part -- is the
game's, byte for byte in JSON. mods/cet/CyberpunkVRPort_VRIK puts the double on the player and copies the VRIK pose
onto it. The versions\\ copies of a scene (the scene as older saves recorded it) are left alone.

    py -3.13 -B tools/mirror/walkable_mirror.py export      the scenes out of the game (build/mirror/game)
    py -3.13 -B tools/mirror/walkable_mirror.py patch       -> mods/archive/source/raw/<scene>.json, build/mirror/scenes.json
    py -3.13 -B tools/mirror/walkable_mirror.py pack        the copies into the port's archive
    (no arguments: all three)
"""
import copy
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/mirror'
SOURCE = WORK / 'game'
RAW = ROOT / 'mods/archive/source/raw'
LISTED = WORK / 'scenes.json'           # the scenes patch wrote, for pack
SCENE_PATTERNS = [
    r'mq000[\\/]scenes[\\/]mq000_01_apartment[.]scene$',         # V's apartment, H10 (its mirror part)
    r'open_world[\\/]scenes[\\/]mirrors[\\/][^\\/]+[.]scene$',     # every other interactive mirror
]
APARTMENT = 'mq000_01_apartment.scene'
MIRROR_TITLE = 'LocKey#46420'           # "Mirror": the hub of a mirror the player turns on
SQUEEZED_MS = 400
PLAYER_EVENTS = {'scnPlaySkAnimEvent', 'scnIKEvent', 'scneventsCameraParamsEvent', 'scnUnmountEvent',
                 'scnGameplayTransitionEvent', 'scneventsSetAnimFeatureEvent', 'scnLookAtEvent'}
DOUBLE_RECORD = re.compile(r'^Character\.q110_tpp_(male|female)_v$')


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode:
        print(r.stdout[-3000:], r.stderr[-2000:])
        raise SystemExit('failed: %s' % (args[:3],))
    return r.stdout


def posix(p):
    return p.replace('\\', '/')


def val(x):
    if isinstance(x, dict):
        for k in ('$value', 'Data', 'id'):
            if k in x:
                return val(x[k])
    return x


# ---------------------------------------------------------------- export

def game_json(scene):
    return SOURCE / (Path(posix(scene)).name + '.json')


def exported():
    """Depot paths (posix) of the exported scenes, the versions/ copies left out."""
    out = []
    for f in sorted(SOURCE.rglob('*.scene')):
        rel = f.relative_to(SOURCE).as_posix()
        if '/versions/' not in rel:
            out.append(rel)
    return out


def cmd_export():
    for pattern in SCENE_PATTERNS:
        run(CLI, 'unbundle', GAME / 'archive/pc/content', '-o', SOURCE, '-r', pattern)
    scenes = exported()
    names = [Path(s).name for s in scenes]
    if len(set(names)) != len(names):
        raise SystemExit('two scenes with one file name: the converter writes them all into one folder')
    for scene in scenes:                # the converter writes each JSON into the output folder itself
        run(CLI, 'convert', 'serialize', SOURCE / scene, '-o', SOURCE)
    missing = [s for s in scenes if not game_json(s).exists()]
    print('export: %d scenes, not serialized: %s' % (len(scenes), missing))
    if APARTMENT not in names:
        raise SystemExit("V's apartment scene is not in the game any more (patch day: see SCENE_PATTERNS)")


# ---------------------------------------------------------------- patch

def walk(x, fn):
    """Depth-first in document order, the order the converter reads handles in."""
    if isinstance(x, dict):
        fn(x)
        for v in list(x.values()):
            walk(v, fn)
    elif isinstance(x, list):
        for v in x:
            walk(v, fn)


def handle_defs(doc):
    defs = {}

    def fn(d):
        if 'HandleId' in d and 'Data' in d:
            defs.setdefault(d['HandleId'], d['Data'])
    walk(doc, fn)
    return defs


def rehome_handles(doc, defs):
    """A handle defined only in a removed event and referenced by a kept one: the first reference becomes the
    definition (the converter resolves HandleRefId against handles read before it)."""
    seen = set()
    moved = []

    def fn(d):
        if 'HandleId' in d and 'Data' in d:
            seen.add(d['HandleId'])
        elif set(d) == {'HandleRefId'} and d['HandleRefId'] not in seen:
            hid = d.pop('HandleRefId')
            d['HandleId'] = hid
            d['Data'] = copy.deepcopy(defs[hid])
            seen.add(hid)
            moved.append(hid)
    walk(doc, fn)
    return moved


def performer_of(ev):
    for k in ('performer', 'actorId'):
        if k in ev:
            return k, val(ev[k])
    basic = ev.get('ikData', {}).get('basic') or ev.get('basicData', {}).get('basic') or ev.get('basic')
    if isinstance(basic, dict) and 'performerId' in basic:
        return 'performer', val(basic['performerId'])
    return None, None


def is_actor_event(ev, performers, actors, player):
    t = ev['$type']
    if t not in PLAYER_EVENTS:
        return False
    if t == 'scneventsCameraParamsEvent':
        return player and bool(ev.get('isPlayerCamera'))
    if not player and t == 'scnPlaySkAnimEvent' and val(ev.get('actorComponent')) != 'body':
        return False                    # the double's face
    kind, who = performer_of(ev)
    if kind == 'performer':
        return who in performers
    if kind == 'actorId':
        return who in actors
    return False


def is_cut_workspot(node, doubles):
    q = node.get('questNode', {}).get('Data', {})
    if q.get('$type') != 'questUseWorkspotNodeDefinition':
        return False
    ref = q.get('entityReference', {})
    if val(ref.get('reference')) == '#player':
        return True
    unique = str(val(ref.get('dynamicEntityUniqueName')) or '')
    return any(unique.endswith('_' + name) for name in doubles)


def double_record(actor):
    return str(val(actor.get('spawnDespawnParams', {}).get('specRecordId')) or '')


def patch(doc):
    """The report lines, or None when the scene has no mirror hub."""
    root = doc['Data']['RootChunk']
    actors = {val(a['actorId']) for a in root.get('playerActors', [])}
    performers = {a * 256 + 1 for a in actors}
    doubles = {a['actorName']: val(a['actorId']) for a in root.get('actors', [])
               if DOUBLE_RECORD.match(double_record(a))}
    double_actors = set(doubles.values())
    double_performers = {a * 256 + 1 for a in double_actors}
    graph = root['sceneGraph']['Data']['graph']
    nodes = {val(h['Data']['nodeId']): h['Data'] for h in graph}

    def outs(n):
        return [val(c['nodeId']) for s in n.get('outputSockets', []) for c in s.get('destinations', [])]

    starts = [nid for nid, n in nodes.items() if n['$type'] == 'scnChoiceNode' and
              val(n.get('displayNameOverride')) == MIRROR_TITLE]
    if not starts:
        return None
    part = set()
    todo = list(starts)
    while todo:
        nid = todo.pop()
        if nid in part:
            continue
        part.add(nid)
        todo += outs(nodes[nid])

    defs = handle_defs(doc)
    report = []
    for nid in sorted(part):
        n = nodes[nid]
        if n['$type'] not in ('scnSectionNode', 'scnRewindableSectionNode'):
            continue
        events = n.get('events', [])
        kept = [e for e in events if not is_actor_event(e['Data'], performers, actors, True)]
        squeeze = len(kept) < len(events)
        kept = [e for e in kept if not is_actor_event(e['Data'], double_performers, double_actors, False)]
        if len(kept) == len(events):
            continue
        n['events'] = kept
        if not squeeze:
            report.append('section %d: %d double events out' % (nid, len(events) - len(kept)))
            continue
        for e in kept:
            ev = e['Data']
            ev['startTime'] = min(int(ev.get('startTime', 0)), SQUEEZED_MS - 100)
            if 'duration' in ev:
                ev['duration'] = min(int(ev['duration']), SQUEEZED_MS - ev['startTime'])
        was = n['sectionDuration']['stu']
        n['sectionDuration']['stu'] = SQUEEZED_MS
        report.append('section %d: %d player/double events out, %d -> %d ms' % (
            nid, len(events) - len(kept), was, SQUEEZED_MS))
    cut = sorted(nid for nid in part if is_cut_workspot(nodes[nid], doubles))
    for nid in cut:
        if outs(nodes[nid]):
            raise SystemExit('workspot %d leads on; cut it by hand' % nid)
    for n in nodes.values():
        for s in n.get('outputSockets', []):
            s['destinations'] = [c for c in s.get('destinations', []) if val(c['nodeId']) not in cut]
    if cut:
        report.append('player/double workspots cut off: %s' % cut)
    moved = rehome_handles(doc, defs)
    if moved:
        report.append('%d handles re-homed' % len(moved))
    if not doubles:
        report.append('NOTE: no q110 double in this scene')
    return report


def cmd_patch():
    written, skipped = [], []
    for scene in exported():
        doc = json.loads(game_json(scene).read_text(encoding='utf-8-sig'))
        try:
            report = patch(doc)
        except SystemExit as e:
            raise SystemExit('%s: %s' % (scene, e))
        if report is None:
            skipped.append(scene)
            continue
        dst = RAW / (scene + '.json')
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(json.dumps(doc, indent=2, ensure_ascii=False), encoding='utf-8', newline='\n')
        written.append(scene)
        print(scene)
        for line in report:
            print('  ' + line)
    LISTED.write_text(json.dumps(written, indent=1), encoding='utf-8')
    print('patch: %d scenes written, %d without a mirror hub: %s' % (len(written), len(skipped), skipped))


# ---------------------------------------------------------------- pack

def cmd_pack():
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene as N  # noqa: E402
    N.pack_sources(json.loads(LISTED.read_text(encoding='utf-8')), WORK / 'stage')
    N.pack_archive()


if __name__ == '__main__':
    for step in sys.argv[1:] or ['export', 'patch', 'pack']:
        {'export': cmd_export, 'patch': cmd_patch, 'pack': cmd_pack}[step]()
