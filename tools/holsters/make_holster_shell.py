"""The holsters themselves: the shells of "Valentino Holsters" (Nexus mod 18100) round the weapons the port hangs on
the body.

    py -3.13 -B tools/holsters/make_holster_shell.py [ARCHIVE]          all steps
    py -3.13 -B tools/holsters/make_holster_shell.py fit [ARCHIVE]      every weapon's pose in the mod's holsters
    py -3.13 -B tools/holsters/make_holster_shell.py ent                one entity per shell (sources)
    py -3.13 -B tools/holsters/make_holster_shell.py lua                the table the holster module reads
    py -3.13 -B tools/holsters/make_holster_shell.py pack               the sources into the pack, the archive packed

ARCHIVE is the mod's rm_acc_holster_valentino.archive (default: the game's archive/pc/mod). The port ships none of the
mod's files: its entities name the mod's meshes, and the holster module shows them only when that archive is
installed (the mod's ArchiveXL file gives the meshes their materials).

WHAT IS USED. The mod dresses V in its holsters as clothes -- skinned garments, each part a chunk of a mesh that
visual tags show. The port takes three of them as plain meshes carried rigidly by one bone:
  - the LEG HOLSTER's shell (`*_legholster_val_base_holsters`, without the thigh straps): one per kind of handgun --
    pistol, revolver, Overture, Malorian -- on the drawing hip;
  - the SHOULDER HARNESS (`*_harness_base`): both shoulder loops and the back strap, with the holster under the arm
    on the off side when a pistol hangs there;
  - the CHEST KNIFE SHEATH (`*_harness_<side>_cknife`) on the harness strap, when a knife is on the chest.

WHERE. A garment mesh's vertices are in the body's model space (the bind pose); a mesh component draws them as they
are, so a shell carried by a bone sits where the mod put it once its local transform is the inverse of that bone's
bind pose. That is how the harness and the sheath hang (Spine3, the Chest slot) -- they are made to fit the body, so
the port's weapon goes into them: the weapon's place there IS the pose the mod gave its own weapon in that holster.
The leg holster instead goes with the port's own place for a pistol (at the belt, higher than the mod's thigh), so its
local transform carries the mod's weapon pose onto that place.

THE POSE THE MOD GAVE A WEAPON. The mod posed copies of the game's weapons in its holsters (`*_unity`, `*_overture`,
`*_cknife_grip`, ...), re-exported, so not vertex-for-vertex the game's. shellfit.exe (C++, this folder) lays the
game's own weapon mesh -- in the weapon's frame, the one the hand holds it in -- onto the mod's copy by trimmed ICP:
the answer is that weapon's pose in the body's model space.
"""
import copy
import json
import math
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/holsters-20261103/shell'
RAW = ROOT / 'mods/archive/source/raw'
DEPOT = 'vrport\\holster\\shell\\'
LUA_OUT = ROOT / 'mods/cet/CyberpunkVRPort_Holster/data/shells.lua'
FIT_EXE = HERE / 'shellfit.exe'
ARCHIVE_NAME = 'rm_acc_holster_valentino.archive'
BODIES = ('m', 'w')                      # the base bodies: {gender}_base_body
SIDES = ('r', 'l')

LEG = 'raem\\accessories\\rm_legholster_val\\meshes\\{b}_base_body_{s}_legholster_val_{part}.mesh'
HARNESS = 'raem\\accessories\\rm_harness_val_full\\meshes\\{b}_base_body_harness_{part}.mesh'

UNITY = ['base\\weapons\\firearms\\handgun\\constitutional_unity\\entities\\meshes\\w_handgun__constitutional_unity__base1_01.mesh']
NOVA = ['base\\weapons\\firearms\\revolver\\darra_nova\\entities\\meshes\\w_revolver__darra_nova__base1_01.mesh']
OVERTURE = ['base\\weapons\\firearms\\revolver\\malorian_overture\\entities\\meshes\\w_revolver__malorian_overture__base1_01.mesh',
            'base\\weapons\\firearms\\revolver\\malorian_overture\\entities\\meshes\\w_revolver__malorian_overture__rcv1_01.mesh']
MALORIAN = ['base\\weapons\\firearms\\handgun\\malorian_silverhand\\entities\\meshes\\w_handgun__malorian_silverhand__base1_01.mesh']
KNIFE = ['base\\weapons\\melee\\knife\\combat\\entities\\meshes\\w_knife__combat__grip1_01.mesh']

# The leg holster's shells by kind of handgun: the mod's weapon in it (its posed copy, the game's meshes of it) and
# the shell's chunks (the mod's visual tags rm_legholster_val_<s>_{pistol,revolver,overture,malorian}_show).
LEG_KINDS = {
    'pistol': ('unity', UNITY, [4, 5]),
    'revolver': ('nova', NOVA, [6]),
    'overture': ('overture', OVERTURE, [7, 8]),
    'malorian': ('malorian', MALORIAN, [9, 10]),
}
# The harness: the two shoulder loops, the back strap and their buckles (tag rm_harness_val_full: chunks 0-4), and the
# holster under each arm (rm_harness_val_full_pistol_{left,right}_show).
HARNESS_STRAPS = [0, 1, 2, 3, 4]
HARNESS_HOLSTER = {'l': [9, 10], 'r': [11, 12]}
ALL_CHUNKS = list(range(63))
# The look: every shell black, no gold (asked 2026-11-03). The mod's `default` has brass and gold buckles, and most
# of its other variants -- `solid_black`, and `default` on the knife sheath, among them -- take layers from
# another mod's material templates (tcpp_edit\...), which show magenta where that mod is missing. Its carbon
# variants use only the game's templates and its own: `carbon` (the leg holster, the harness) and `carbon_black` (the
# knife sheath) are black throughout.
LOOK = 'carbon'
LOOK_KNIFE = 'carbon_black'


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout[-2000:] + r.stderr[-2000:])
        raise SystemExit('failed: %s' % ' '.join(str(a) for a in args[:3]))
    return r.stdout


def regex_for(paths):
    return '^(' + '|'.join(p.replace('\\', '[\\\\/]').replace('.', '[.]') for p in paths) + ')$'


# ---------------------------------------------------------------- glb points

def glb_points(path):
    """Every vertex of a WolvenKit glb, in the game's axes (x right, y forward, z up; the glTF has y up, -z forward)."""
    import numpy as np
    data = Path(path).read_bytes()
    jl = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + jl])
    off = 20 + jl
    bl = struct.unpack_from('<I', data, off)[0]
    binary = data[off + 8: off + 8 + bl]
    out = []
    for mesh in doc['meshes']:
        for prim in mesh['primitives']:
            a = doc['accessors'][prim['attributes']['POSITION']]
            v = doc['bufferViews'][a['bufferView']]
            start = v.get('byteOffset', 0) + a.get('byteOffset', 0)
            stride = v.get('byteStride', 0) or 12
            raw = np.frombuffer(binary, dtype=np.uint8, count=stride * (a['count'] - 1) + 12, offset=start)
            rows = np.lib.stride_tricks.as_strided(raw, shape=(a['count'], 12), strides=(stride, 1))
            p = np.frombuffer(rows.tobytes(), dtype=np.float32).reshape(-1, 3)
            out.append(np.stack([p[:, 0], -p[:, 2], p[:, 1]], 1))
    return np.concatenate(out).astype(np.float32)


def glb_bind(path, bone):
    """A joint's bind pose in the model space of a skinned glb, game axes: (position, quaternion { i, j, k, r })."""
    import numpy as np
    data = Path(path).read_bytes()
    jl = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + jl])
    binary = data[20 + jl + 8:]
    skin = doc['skins'][0]
    acc = doc['accessors'][skin['inverseBindMatrices']]
    view = doc['bufferViews'][acc['bufferView']]
    start = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
    ibm = np.frombuffer(binary, dtype=np.float32, count=16 * acc['count'], offset=start).reshape(-1, 4, 4)
    names = [doc['nodes'][j].get('name', '?') for j in skin['joints']]
    world = np.linalg.inv(ibm[names.index(bone)].astype(np.float64).T)
    C = np.array([[1, 0, 0, 0], [0, 0, -1, 0], [0, 1, 0, 0], [0, 0, 0, 1]], dtype=np.float64)
    g = C @ world @ np.linalg.inv(C)
    return [float(x) for x in g[:3, 3]], mat_quat(g[:3, :3])


def mat_quat(m):
    tr = m[0][0] + m[1][1] + m[2][2]
    if tr > 0:
        s = math.sqrt(tr + 1.0) * 2
        q = ((m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s)
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2
        q = (0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s)
    elif m[1][1] > m[2][2]:
        s = math.sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2
        q = ((m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s)
    else:
        s = math.sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2
        q = ((m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s)
    n = math.sqrt(sum(c * c for c in q))
    return [float(c / n) for c in q]


# ---------------------------------------------------------------- fit

def glb_of(depot_path, root):
    return root / (depot_path.replace('\\', '/')[:-len('.mesh')] + '.glb')


def uncook(source, paths, out):
    run(CLI, 'uncook', source, '-o', out, '-gp', GAME, '-r', regex_for(paths), '-v', 'Minimal')


def fit_jobs():
    """(name, the mod's posed weapon, the game's meshes of that weapon)."""
    jobs = []
    for b in BODIES:
        for s in SIDES:
            for kind, (part, game, _) in LEG_KINDS.items():
                jobs.append(('leg_%s_%s_%s' % (s, kind, b), LEG.format(b=b, s=s, part=part), game))
            jobs.append(('underarm_%s_%s' % (s, b), HARNESS.format(b=b, part=s + '_unity'), UNITY))
            jobs.append(('cknife_%s_%s' % (s, b), HARNESS.format(b=b, part=s + '_cknife_grip'), KNIFE))
    return jobs


def binds_needed():
    """(name, a mesh of that body skinned to the bone, bone)."""
    out = []
    for b in BODIES:
        out.append(('chest_%s' % b, HARNESS.format(b=b, part='base'), 'Spine3'))
        out.append(('hips_%s' % b, LEG.format(b=b, s='r', part='base_holsters'), 'Hips'))
    return out


def cmd_fit(archive):
    if not FIT_EXE.exists():
        raise SystemExit('%s missing: run build_shellfit.bat' % FIT_EXE)
    mod_glb, game_glb = WORK / 'mod_glb', WORK / 'game_glb'
    jobs = fit_jobs()
    mod_paths = sorted({j[1] for j in jobs} | {m for _, m, _ in binds_needed()})
    game_paths = sorted({g for j in jobs for g in j[2]})
    if not all(glb_of(p, mod_glb).exists() for p in mod_paths):
        uncook(archive, mod_paths, mod_glb)
    if not all(glb_of(p, game_glb).exists() for p in game_paths):
        uncook(GAME / 'archive/pc/content', game_paths, game_glb)
    import numpy as np
    pts = WORK / 'points'
    pts.mkdir(parents=True, exist_ok=True)
    fits = {}
    for name, target, sources in jobs:
        tgt = glb_points(glb_of(target, mod_glb))
        src = np.concatenate([glb_points(glb_of(p, game_glb)) for p in sources])
        (pts / (name + '_src.bin')).write_bytes(src.tobytes())
        (pts / (name + '_dst.bin')).write_bytes(tgt.tobytes())
        r = json.loads(run(FIT_EXE, pts / (name + '_src.bin'), pts / (name + '_dst.bin'), 0.10).strip().splitlines()[-1])
        fits[name] = {'q': r['q'], 't': r['t'], 'rms': r['rms'], 'near2mm': r['near2mm']}
        # the mod's female harness is refitted to that body (its weapons bent by a millimetre or two)
        flag = '' if r['rms'] < 0.0025 and r['near2mm'] > 0.6 else '   <-- poor fit'
        print('%-26s rms %.2f mm, %3.0f%% within 2 mm%s' % (name, r['rms'] * 1000, r['near2mm'] * 100, flag))
    binds = {}
    for name, mesh, bone in binds_needed():
        p, q = glb_bind(glb_of(mesh, mod_glb), bone)
        binds[name] = {'p': p, 'q': q}
    (WORK / 'fits.json').write_text(json.dumps({'fits': fits, 'binds': binds}, indent=1), encoding='utf-8')
    print('fits:', WORK / 'fits.json')


# ---------------------------------------------------------------- entities

def cname(v):
    return {'$type': 'CName', '$storage': 'string', '$value': v}


def depot(path):
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': path}, 'Flags': 'Default'}


def shells():
    """name -> (the mod's mesh, its chunks)."""
    out = {}
    for b in BODIES:
        for s in SIDES:
            for kind, (_, _, chunks) in LEG_KINDS.items():
                out['leg_%s_%s_%s' % (s, kind, b)] = (LEG.format(b=b, s=s, part='base_holsters'), chunks)
            out['underarm_%s_%s' % (s, b)] = (HARNESS.format(b=b, part='base'), HARNESS_HOLSTER[s])
            out['cknife_%s_%s' % (s, b)] = (HARNESS.format(b=b, part=s + '_cknife'), ALL_CHUNKS)
        out['harness_%s' % b] = (HARNESS.format(b=b, part='base'), HARNESS_STRAPS)
    return out


def mask(chunks):
    return str(sum(1 << c for c in chunks))


def cmd_ent():
    ref = json.loads((RAW / 'vrport/ui/vr_lobby_glass.ent.json').read_text(encoding='utf-8-sig'))
    template = next(c for c in ref['Data']['RootChunk']['components'] if c['$type'] == 'entMeshComponent')
    entity = {'$type': 'entEntity', 'customCameraTarget': 'ECCTV_All', 'renderSceneLayerMask': 'Default'}
    out_dir = RAW / DEPOT.replace('\\', '/')
    out_dir.mkdir(parents=True, exist_ok=True)
    for f in out_dir.glob('vrp_shell_*.ent.json'):
        f.unlink()
    cid = 2800000000000010001
    for name, (mesh, chunks) in sorted(shells().items()):
        c = copy.deepcopy(template)
        c['id'] = str(cid)
        c['name'] = cname('shell')
        c['mesh'] = depot(mesh)
        c['meshAppearance'] = cname(LOOK_KNIFE if name.startswith('cknife_') else LOOK)
        c['chunkMask'] = mask(chunks)
        c['visualScale'] = {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1}
        for k in ('castShadows', 'castLocalShadows', 'castRayTracedGlobalShadows', 'castRayTracedLocalShadows'):
            c[k] = 'Always'
        doc = {'Header': dict(ref['Header'], ArchiveFileName=''), 'Data': {
            'Version': ref['Data']['Version'], 'BuildVersion': ref['Data']['BuildVersion'], 'RootChunk': {
                '$type': 'entEntityTemplate', 'appearances': [], 'backendDataOverrides': [], 'bindingOverrides': [],
                'compiledData': {'BufferId': '0', 'Flags': 4063232,
                                 'Type': ref['Data']['RootChunk']['compiledData']['Type'],
                                 'Data': {'Version': 4, 'Sections': 7, 'CruidIndex': 0,
                                          'CruidDict': {'0': '0', '1': str(cid)},
                                          'Chunks': [dict(entity), copy.deepcopy(c)]}},
                'compiledEntityLODFlags': 0, 'componentResolveSettings': [], 'components': [c],
                'cookingPlatform': 'PLATFORM_PC', 'defaultAppearance': cname('default'),
                'entity': {'HandleId': '0', 'Data': dict(entity)},
                'includeInstanceBuffer': None, 'includes': [], 'inplaceResources': [], 'localData': None,
                'resolvedDependencies': [], 'visualTagsSchema': None},
            'EmbeddedFiles': []}}
        (out_dir / ('vrp_shell_%s.ent.json' % name)).write_text(json.dumps(doc, indent=1) + '\n', encoding='utf-8',
                                                                 newline='\n')
        cid += 1
    print('%d shell entities in %s' % (len(shells()), out_dir.relative_to(ROOT)))


# ---------------------------------------------------------------- the module's table

def lua_vec(v):
    return '{ %s }' % ', '.join('%.6f' % x for x in v)


def cmd_lua():
    data = json.loads((WORK / 'fits.json').read_text(encoding='utf-8'))
    fits, binds = data['fits'], data['binds']

    def pose(d):
        return '{ p = %s, q = %s }' % (lua_vec(d.get('t') or d.get('p')), lua_vec(d['q']))

    ent = lambda name: '"%s"' % (DEPOT + 'vrp_shell_' + name + '.ent').replace('\\', '\\\\')
    lines = ['-- Generated by tools/holsters/make_holster_shell.py from the meshes of "Valentino Holsters": do not edit.',
             '-- Poses { p, q } are in the body\'s model space (a weapon\'s: its frame -> the model; a bone\'s: its bind',
             '-- pose); q is { i, j, k, r }. See modules/shells.lua.',
             'return {',
             '  archive = "%s",' % ARCHIVE_NAME,
             '  bind = {']
    for b in BODIES:
        lines.append('    %s = { chest = %s, hips = %s },' % (b, pose(binds['chest_' + b]), pose(binds['hips_' + b])))
    lines += ['  },', '  -- the leg holster by kind of handgun: the entity and the pose of the mod\'s weapon in it', '  leg = {']
    for s in SIDES:
        lines.append('    %s = {' % s)
        for kind in LEG_KINDS:
            cells = ', '.join('%s = { ent = %s, fit = %s }' % (b, ent('leg_%s_%s_%s' % (s, kind, b)),
                                                             pose(fits['leg_%s_%s_%s' % (s, kind, b)])) for b in BODIES)
            lines.append('      %s = { %s },' % (kind, cells))
        lines.append('    },')
    lines += ['  },', '  -- the harness (both shoulder loops)', '  harness = { %s },' % ', '.join(
        '%s = %s' % (b, ent('harness_' + b)) for b in BODIES),
        '  -- the holster under the arm and the chest knife sheath on the harness, with the pose of the mod\'s weapon in it',
        '  underarm = {']
    for s in SIDES:
        lines.append('    %s = { %s },' % (s, ', '.join('%s = { ent = %s, fit = %s }' % (
            b, ent('underarm_%s_%s' % (s, b)), pose(fits['underarm_%s_%s' % (s, b)])) for b in BODIES)))
    lines += ['  },', '  cknife = {']
    for s in SIDES:
        lines.append('    %s = { %s },' % (s, ', '.join('%s = { ent = %s, fit = %s }' % (
            b, ent('cknife_%s_%s' % (s, b)), pose(fits['cknife_%s_%s' % (s, b)])) for b in BODIES)))
    lines += ['  },', '}']
    LUA_OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(LUA_OUT.relative_to(ROOT))


def cmd_pack():
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene  # noqa: E402 -- the port's one sources -> pack -> archive step
    rels = [DEPOT.replace('\\', '/') + 'vrp_shell_%s.ent' % n for n in sorted(shells())]
    make_lobby_scene.pack_sources(rels, ROOT / 'build/holsters-20261103/shell_stage')
    make_lobby_scene.pack_archive()


if __name__ == '__main__':
    args = sys.argv[1:]
    archive = next((a for a in args if a.lower().endswith('.archive')), str(GAME / 'archive/pc/mod' / ARCHIVE_NAME))
    steps = [a for a in args if not a.lower().endswith('.archive')] or ['fit', 'ent', 'lua', 'pack']
    for step in steps:
        if step == 'fit':
            cmd_fit(archive)
        else:
            {'ent': cmd_ent, 'lua': cmd_lua, 'pack': cmd_pack}[step]()
