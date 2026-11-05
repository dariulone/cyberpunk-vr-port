"""The engrams on the lobby's table: the game's own data shard made a thing to pick up.

    py -3.13 -B tools/vr_lobby_assets/make_engrams.py          all of it
    py -3.13 -B tools/vr_lobby_assets/make_engrams.py mesh     vrport\\engram\\vr_engram_shard.mesh and its entities
    py -3.13 -B tools/vr_lobby_assets/make_engrams.py pack     deserialize into the pack, pack the archive

The shard is the one lying on desks all over Night City (base\\environment\\decoration\\electronics\\hardware\\shard\\
shard_a.mesh, 22 x 46 x 4 mm, in its four looks default, B, C and D), its geometry and its materials as the game has
them. Only its body is changed: the game's is a static mesh collider; the engram's is the dynamic box the lobby's
own shard was given (make_lobby_scene.py item_body: the port's basketball body, re-shaped -- the same filter,
damping and solver counts), made the size of this shard and its mass. Each look is an entity of its own
(vr_engram_<look>.ent: the lobby shard's entity, an entPhysicalMeshComponent "item" on this mesh in that look), so
modules/engrams.lua gives each playthrough's engram a look of its own.
"""
import copy
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_lobby_scene as S  # noqa: E402
from make_lobby_scene import cname, depot  # noqa: E402

ROOT = S.ROOT
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
GAME_MESH = 'base\\environment\\decoration\\electronics\\hardware\\shard\\shard_a.mesh'
ARCHIVE = 'basegame_3_nightcity.archive'
LOOKS = ['default', 'B', 'C', 'D']
WORK = ROOT / 'build/lobby-engram'
RAW = ROOT / 'mods/archive/source/raw/vrport/engram'
DEPOT = 'vrport\\engram\\'
MESH = 'vr_engram_shard.mesh'
# The lobby's own shard: its body (dynamic, shaped by item_body) and its entity.
BODY_FROM = ROOT / 'mods/archive/source/raw/vrport/lobby/vr_lobby_item_2.mesh.json'
ENT_FROM = ROOT / 'mods/archive/source/raw/vrport/lobby/vr_lobby_item_2.ent.json'
MASS = 0.02


def handle_ids(node, out):
    """Every HandleId in a document's tree."""
    if isinstance(node, dict):
        if 'HandleId' in node:
            out.append(int(node['HandleId']))
        for v in node.values():
            handle_ids(v, out)
    elif isinstance(node, list):
        for v in node:
            handle_ids(v, out)
    return out


def renumber(node, start):
    """The handles of a subtree moved past `start`, the references among them with them."""
    ids = {}

    def walk(n):
        if isinstance(n, dict):
            if 'HandleId' in n:
                ids.setdefault(n['HandleId'], str(start + len(ids)))
                n['HandleId'] = ids[n['HandleId']]
            for v in n.values():
                walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)

    def refs(n):
        if isinstance(n, dict):
            if 'HandleRefId' in n and n['HandleRefId'] in ids:
                n['HandleRefId'] = ids[n['HandleRefId']]
            for v in n.values():
                refs(v)
        elif isinstance(n, list):
            for v in n:
                refs(v)

    walk(node)
    refs(node)
    return node


def shard_body(root, body_param):
    """The game shard's static collider swapped for the lobby shard's dynamic body, sized and weighed for it."""
    bb = root['boundingBox']
    lo = [bb['Min'][k] for k in 'XYZ']
    hi = [bb['Max'][k] for k in 'XYZ']
    centre = [(a + b) / 2 for a, b in zip(lo, hi)]
    half = [(b - a) / 2 for a, b in zip(lo, hi)]
    item = {'mass': MASS, 'centre': centre, 'half': half, 'shape': 'box'}
    params = [p for p in root['parameters'] if p['Data']['$type'] != 'meshMeshParamPhysics']
    start = max(handle_ids(root, [0])) + 1
    params.append(renumber(copy.deepcopy(body_param), start))
    root['parameters'] = params
    # item_body shapes the body, and leaves the mesh one look and no materials: the shard keeps its own.
    keep = {k: copy.deepcopy(root[k]) for k in ('appearances', 'localMaterialBuffer', 'localMaterialInstances',
                                                  'externalMaterials', 'materialEntries') if k in root}
    S.item_body(root, item)
    root.update(keep)
    return max(half)


def cmd_mesh():
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    pattern = '^%s$' % GAME_MESH.replace('\\', '\\\\').replace('.', '\\.')
    S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content' / ARCHIVE, '-o', WORK, '-r', pattern)
    S.run(S.CLI, 'convert', 'serialize', WORK, '-o', WORK, '-w', '*.mesh')
    doc = json.loads((WORK / 'shard_a.mesh.json').read_text(encoding='utf-8-sig'))
    doc['Header']['ArchiveFileName'] = ''
    lobby = json.loads(BODY_FROM.read_text(encoding='utf-8'))['Data']['RootChunk']
    body = next(p for p in lobby['parameters'] if p['Data']['$type'] == 'meshMeshParamPhysics')
    radius = shard_body(doc['Data']['RootChunk'], body)
    if RAW.exists():
        shutil.rmtree(RAW)
    RAW.mkdir(parents=True)
    (RAW / (MESH + '.json')).write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n', encoding='utf-8',
                                        newline='\n')
    template = json.loads(ENT_FROM.read_text(encoding='utf-8'))
    for look in LOOKS:
        ent = copy.deepcopy(template)
        root = ent['Data']['RootChunk']
        for c in [root['components'][0]] + [c for c in root['compiledData']['Data']['Chunks']
                                            if c.get('$type') == 'entPhysicalMeshComponent']:
            c['mesh']['DepotPath'] = depot(DEPOT + MESH)
            c['meshAppearance'] = cname(look)
        root['resolvedDependencies'] = [{'DepotPath': depot(DEPOT + MESH), 'Flags': 'Default'}]
        (RAW / ('vr_engram_%s.ent.json' % look.lower())).write_text(json.dumps(ent, ensure_ascii=False, indent=1) + '\n',
                                                                    encoding='utf-8', newline='\n')
    print('engram mesh, radius %.3f m, %d looks' % (radius, len(LOOKS)))


def cmd_pack():
    target = S.PACK / 'vrport/engram'
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    S.run(S.CLI, 'convert', 'deserialize', RAW, '-o', target)
    print('engram', len(list(target.glob('*'))), 'files')
    S.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['mesh', 'pack']
    for step in steps:
        {'mesh': cmd_mesh, 'pack': cmd_pack}[step]()
