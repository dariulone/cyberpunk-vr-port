"""The weapon a holster shows: one entity template whose appearances are every weapon's own appearances.

    py -3.13 -B tools/holsters/make_holster_weapon.py            all steps
    py -3.13 -B tools/holsters/make_holster_weapon.py extract    the factories and every weapon .app, as JSON
    py -3.13 -B tools/holsters/make_holster_weapon.py ent        vrport\\holster\\vrp_holster_weapon.ent (source)
    py -3.13 -B tools/holsters/make_holster_weapon.py lua        the appearance table the holster module reads
    py -3.13 -B tools/holsters/make_holster_weapon.py pack       the source into the pack, the archive packed

WHY. A weapon the player is not holding has no object in the world: the game spawns it (base\\weapons\\weapon_root.ent,
a gameplay object without appearances of its own) when it is drawn, with the appearance resource the item record names
(`appearanceResourceName`, a key of the weapons_appearances factory -> that weapon's .app) and the appearance in it
that matches the record's visual tags. A holster needs the same look on a plain entity that nothing plays with. So
this template has no components and one appearance per (weapon .app, appearance in it), named `<factory key>__<name>`
and pointing at the game's own .app: spawned with that name, the entity is built from the weapon's own components
(its meshes, the rigs they are skinned to, its decals), the way the drawn weapon is. Nothing is copied, so a patch
that changes a weapon's look changes the holster's with it; a patch that adds weapons needs this run again.

THE APPEARANCE TABLE (mods/cet/CyberpunkVRPort_Holster/data/appearances.lua): factory key -> the .app's appearances
in order, each with its visual tags: what the module needs to work out the look the game gives an item from its seed.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/holsters-20261103/weapon'
RAW = ROOT / 'mods/archive/source/raw'
ENT_REL = 'vrport/holster/vrp_holster_weapon.ent'
LUA_OUT = ROOT / 'mods/cet/CyberpunkVRPort_Holster/data/appearances.lua'
FACTORIES = ['base/gameplay/factories/items/weapons/weapons_appearances.csv',
             'ep1/gameplay/factories/items/weapons/weapons_appearances.csv']
SOURCES = {'base': GAME / 'archive/pc/content', 'ep1': GAME / 'archive/pc/ep1'}


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout[-2000:] + r.stderr[-2000:])
        raise SystemExit('failed: %s' % ' '.join(str(a) for a in args[:3]))
    return r.stdout


def regex_for(paths):
    return '^(' + '|'.join(p.replace('\\', '\\\\').replace('.', '\\.') for p in paths) + ')$'


def unbundle(paths, out):
    """The files at `paths` (depot paths, backslashes) out of whichever archive folder holds them."""
    for folder in SOURCES.values():
        run(CLI, 'unbundle', folder, '-o', out, '-r', regex_for(paths), '-v', 'Minimal')


def factory_rows():
    rows = []
    for rel in FACTORIES:
        j = WORK / 'json' / (rel.replace('/', '__') + '.json')
        for key, path, *_ in json.loads(j.read_text(encoding='utf-8'))['Data']['RootChunk']['data']:
            rows.append((key, path))
    return rows


def app_json(path):
    return WORK / 'json' / (path.replace('\\', '__') + '.json')


def cmd_extract():
    raw = WORK / 'raw'
    unbundle([p.replace('/', '\\') for p in FACTORIES], raw)
    (WORK / 'json').mkdir(parents=True, exist_ok=True)
    for rel in FACTORIES:
        src = raw / rel
        out = WORK / 'json_tmp'
        out.mkdir(parents=True, exist_ok=True)
        run(CLI, 'convert', 'serialize', src, '-o', out, '-v', 'Minimal')
        (out / (Path(rel).name + '.json')).replace(WORK / 'json' / (rel.replace('/', '__') + '.json'))
    apps = sorted({path for _, path in factory_rows()})
    todo = [p for p in apps if not app_json(p).exists()]
    if todo and not all((raw / p.replace('\\', '/')).exists() for p in todo):
        unbundle(todo, raw)
    missing, failed = 0, 0
    # One converter run for all of them (each run costs the CLI's start-up): the files are copied into one batch
    # folder under their depot paths flattened, so no two share a name, and serialized in one go.
    batch = WORK / 'batch'
    if batch.exists():
        import shutil
        shutil.rmtree(batch)
    batch.mkdir(parents=True)
    flat = {}
    for path in todo:
        src = raw / path.replace('\\', '/')
        if not src.exists():
            missing += 1
            print('missing in the archives:', path)
            continue
        name = path.replace('\\', '__')
        (batch / name).write_bytes(src.read_bytes())
        flat[name] = path
    if flat:
        out = WORK / 'json_tmp'
        out.mkdir(parents=True, exist_ok=True)
        try:
            run(CLI, 'convert', 'serialize', batch, '-o', out, '-v', 'Minimal')
        except SystemExit as e:
            print('the batch serialize failed:', e)
        for name, path in flat.items():
            j = out / (name + '.json')
            if j.exists():
                j.replace(app_json(path))
            else:
                failed += 1
                print('not serialized:', path)
    print('%d .app files: %d done now, %d missing, %d failed' % (len(apps), len(todo) - missing - failed, missing,
                                                                failed))


def appearances():
    """factory key -> [(appearance name, [visual tags])], in the .app's order."""
    out = {}
    for key, path in factory_rows():
        j = app_json(path)
        if not j.exists():
            continue
        root = json.loads(j.read_text(encoding='utf-8'))['Data']['RootChunk']
        rows = []
        for a in root.get('appearances', []):
            d = a.get('Data') or {}
            name = (d.get('name') or {}).get('$value')
            if not name:
                continue
            tags = [t.get('$value') for t in ((d.get('visualTags') or {}).get('tags') or []) if t.get('$value')]
            rows.append((name, tags))
        if rows:
            out[key] = (path, rows)
    return out


def cname(v):
    return {'$type': 'CName', '$storage': 'string', '$value': v}


def cmd_ent():
    table = appearances()
    entries = []
    for key in sorted(table):
        path, rows = table[key]
        for name, _ in rows:
            entries.append({'$type': 'entTemplateAppearance',
                            'appearanceName': cname(name),
                            'appearanceResource': {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string',
                                                                 '$value': path}, 'Flags': 'Default'},
                            'name': cname('%s__%s' % (key, name))})
    entity = {'$type': 'entEntity', 'customCameraTarget': 'ECCTV_All', 'renderSceneLayerMask': 'Default'}
    doc = {
        'Header': {'WolvenKitVersion': '1.0.0', 'WKitJsonVersion': '0.0.9', 'GameVersion': 2500,
                   'DataType': 'CR2W', 'ArchiveFileName': ''},
        'Data': {'Version': 195, 'BuildVersion': 0, 'RootChunk': {
            '$type': 'entEntityTemplate',
            'appearances': entries,
            'backendDataOverrides': [], 'bindingOverrides': [],
            'compiledData': {'BufferId': '0', 'Flags': 4063232,
                             'Type': 'WolvenKit.RED4.Archive.Buffer.RedPackage, WolvenKit.RED4, Version=8.18.0.0, '
                                     'Culture=neutral, PublicKeyToken=null',
                             'Data': {'Version': 4, 'Sections': 7, 'CruidIndex': 0, 'CruidDict': {'0': '0'},
                                      'Chunks': [dict(entity)]}},
            'compiledEntityLODFlags': 0, 'componentResolveSettings': [], 'components': [],
            'cookingPlatform': 'PLATFORM_PC', 'defaultAppearance': cname(entries[0]['name']['$value']),
            'entity': {'HandleId': '0', 'Data': dict(entity)},
            'includeInstanceBuffer': None, 'includes': [], 'inplaceResources': [], 'localData': None,
            'resolvedDependencies': [], 'visualTagsSchema': None}}}
    # The header and data layout of a WolvenKit 8.18 entity template serialization (as the port's own sources).
    ref = json.loads((RAW / 'base/vrport/vrp_mag_malorian.ent.json').read_text(encoding='utf-8'))
    doc['Header'] = dict(ref['Header'], ArchiveFileName='')
    for k in ref['Data']:
        if k != 'RootChunk':
            doc['Data'][k] = ref['Data'][k]
    out = RAW / (ENT_REL + '.json')
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=1), encoding='utf-8')
    print('%s: %d appearances over %d weapons' % (ENT_REL, len(entries), len(table)))


def lua_str(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


def cmd_lua():
    table = appearances()
    lines = ['-- Generated by tools/holsters/make_holster_weapon.py: do not edit.',
             '-- factory key (the record\'s appearanceResourceName) -> its .app\'s appearances in order, each',
             '-- { name, tags } -- the entity vrport\\holster\\vrp_holster_weapon.ent shows one as "<key>__<name>".',
             'return {']
    for key in sorted(table):
        path, rows = table[key]
        cells = ', '.join('{ %s, { %s } }' % (lua_str(n), ', '.join(lua_str(t) for t in tags)) for n, tags in rows)
        lines.append('  [%s] = { %s },' % (lua_str(key), cells))
    lines.append('}')
    LUA_OUT.parent.mkdir(parents=True, exist_ok=True)
    LUA_OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('%s: %d weapons' % (LUA_OUT.relative_to(ROOT), len(table)))


def cmd_pack():
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene  # noqa: E402 -- the port's one sources -> pack -> archive step
    make_lobby_scene.pack_sources([ENT_REL], ROOT / 'build/holsters-20261103/stage')
    make_lobby_scene.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['extract', 'ent', 'lua', 'pack']
    for step in steps:
        {'extract': cmd_extract, 'ent': cmd_ent, 'lua': cmd_lua, 'pack': cmd_pack}[step]()
