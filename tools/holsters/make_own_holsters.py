"""The port's holster shells: copies of "Valentino Holsters"' own meshes, in the port's archive (that mod's archive is
no longer needed at run time).

    py -3.13 -B tools/holsters/make_own_holsters.py                 all steps
    py -3.13 -B tools/holsters/make_own_holsters.py export          the mod's meshes, the game's pieces they are made
                                                                    from, the bodies, the knife -> .mesh + .glb
    py -3.13 -B tools/holsters/make_own_holsters.py parts           the knife bandolier's pieces
    py -3.13 -B tools/holsters/make_own_holsters.py geom            the glbs to import: the game's harness (the woman's
                                                                    refit), the mod's knife sheaths taking the whole knife
    py -3.13 -B tools/holsters/make_own_holsters.py import          WolvenKit import; every copy given the materials of
                                                                    the game piece it is made from, with a `black` look
    py -3.13 -B tools/holsters/make_own_holsters.py fit             where each weapon sits
    py -3.13 -B tools/holsters/make_own_holsters.py ent             the shell entities (vrport/holster/shell/*.ent)
    py -3.13 -B tools/holsters/make_own_holsters.py lua             mods/cet/CyberpunkVRPort_Holster/data/shells.lua
    py -3.13 -B tools/holsters/make_own_holsters.py pack            meshes + entities into the pack, the archive packed

WHAT. "Valentino Holsters" (Nexus 18100) made its holsters from the game's own pieces: the Valentino gang's holster
(gang_valentino\\item\\i1_092_belt__harness) on the thigh and under each arm, the Maelstrom bandolier's knife sheath
(gang_maelstrom\\item\\i1_025_slot__knife_bandolier) on the chest strap -- the game's vertices, scaled, reshaped and
placed on the body. The port's shells are copies of its meshes (the user's call, 2026-11-07: "copy his files, he took
them from the game"), read from the user's copy of the mod (VAL_ARCHIVE) at build time:
  - the leg holster: its `base_holsters` mesh (no thigh straps: the port hangs it from its own hip place), each shell
    showing the chunks of one kind of handgun (LEG_CHUNKS);
  - the holster under the arm: its harness mesh, showing that side's holster (UNDERARM_CHUNKS);
  - the chest knife sheath: its sheath mesh. The mod hangs a grip there, no blade; the port's copy is the whole combat
    knife, so the knife is turned down the sheath about its guard and the sheath stretched from its throat where the
    blade is longer or wider (ownshell.exe sheathfit);
  - the shoulder harness: the game's (the woman's carried onto her base body).
The mod's meshes carry no materials (its ArchiveXL file patches them in from its own material meshes): each copy is
given the materials of the game piece it is made from, with a `black` look (the gang's multilayer setup's Maelstrom
version, whose brass and gold layers are off: the holsters black, no gold); the sheath the bandolier's
own `maelstrom` look. Each weapon's pose in its holster (REF_FITS) is the one shellfit.exe found for the game's weapon
mesh in the mod's posed copy (make_holster_shell.py, 2026-11-03).
The heavy geometry is tools/holsters/ownshell.cpp; this file only moves files and numbers.
"""
import copy
import json
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / 'tools/hands_only'))
import make_hand_meshes as H  # noqa: E402 -- read_glb / accessor_fast / write_glb

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
EXE = HERE / 'ownshell.exe'
WORK = ROOT / 'build/holsters-own'
SOURCE, IMPORT, BINS, REF = WORK / 'source', WORK / 'import', WORK / 'bins', WORK / 'valref'
RAW = ROOT / 'mods/archive/source/raw'
SHELL_DEPOT = 'vrport\\holster\\shell\\'
OWN_DEPOT = 'vrport\\holster\\own\\'
LUA_OUT = ROOT / 'mods/cet/CyberpunkVRPort_Holster/data/shells.lua'
ARCHIVE = 'basegame_4_appearance.archive'

VAL = 'base\\characters\\garment\\gang_valentino\\item\\i1_092_belt__harness\\i1_092_ma_belt__harness'
PIECES = {
    'harness': VAL + '.mesh',                             # the harness itself
    'leg': VAL + '_hips_gun_holster_legs_left.mesh',      # the materials of the mod's leg holster
    'underarm': VAL + '_hips_gun_holster_left.mesh',      # the materials of the mod's holster under the arm
}
# the materials of the mod's knife sheath: the Maelstrom bandolier's
SHEATH = ('base\\characters\\garment\\gang_maelstrom\\item\\i1_025_slot__knife_bandolier\\'
          'i1_025_ma_slot__knife_bandolier.mesh')
# the knife the chest sheath takes (the combat knife): the sheath is stretched where its blade needs it
KNIFE = 'base\\weapons\\melee\\knife\\combat\\entities\\meshes\\w_knife__combat__grip1_01.mesh'
MAELSTROM_LOOK = VAL.rsplit('\\', 1)[0] + '\\textures\\ml_i1_092_ma_belt__harness_maelstrom.mlsetup'

# THE MOD'S MESHES, per body (m_/w_) and side
VAL_ARCHIVE = GAME / 'archive/pc/mod/rm_acc_holster_valentino.archive'
REF_LEG = 'raem\\accessories\\rm_legholster_val\\meshes\\%s_base_body_%s_legholster_val_base_holsters.mesh'
REF_HARNESS = 'raem\\accessories\\rm_harness_val_full\\meshes\\%s_base_body_harness_base.mesh'
REF_CKNIFE = 'raem\\accessories\\rm_harness_val_full\\meshes\\%s_base_body_harness_%s_cknife.mesh'
# their chunks: the leg holster per kind of handgun (the mod's visual tags rm_legholster_val_<s>_<kind>_show), the
# holster under each arm (rm_harness_val_full_pistol_<side>_show); a chunk of DECAL_VERTS vertices is the holster's
# stitch decal
LEG_CHUNKS = {'pistol': [4, 5], 'revolver': [6], 'overture': [7, 8], 'malorian': [9, 10]}
UNDERARM_CHUNKS = {'l': [9, 10], 'r': [11, 12]}
DECAL_VERTS = {'leg': 16, 'underarm': 36}

# Each weapon's pose in the mod's holster (its frame -> the body's model space; q = i, j, k, r): shellfit.exe's
# trimmed ICP of the game's weapon mesh onto the mod's posed copy (make_holster_shell.py, 2026-11-03). The mod's
# chest knife is a grip without a blade: its pose places the handle, and sheathfit turns the blade down the sheath
# (geom writes the knives' own poses to knife_fits.json).
REF_FITS = {
    'leg_r_pistol_m': ((0.233367, -0.018165, 0.804245), (-0.730834, 0.029945, -0.027599, 0.681340)),
    'leg_r_pistol_w': ((0.219913, -0.033817, 0.859173), (-0.727081, 0.030649, -0.023202, 0.685474)),
    'leg_r_revolver_m': ((0.234604, -0.000031, 0.785851), (-0.728593, 0.027426, -0.025749, 0.683913)),
    'leg_r_revolver_w': ((0.222138, -0.018998, 0.838478), (-0.724958, 0.028223, -0.012063, 0.688108)),
    'leg_r_overture_m': ((0.232598, -0.010643, 0.796311), (-0.731838, 0.024560, -0.028975, 0.680419)),
    'leg_r_overture_w': ((0.219452, -0.025476, 0.850229), (-0.725972, 0.039552, -0.029863, 0.685936)),
    'leg_r_malorian_m': ((0.232780, -0.023429, 0.816881), (-0.730671, 0.040599, -0.040951, 0.680291)),
    'leg_r_malorian_w': ((0.218903, -0.036264, 0.872312), (-0.730422, 0.043731, -0.039927, 0.680424)),
    'leg_l_pistol_m': ((-0.232489, -0.018889, 0.804144), (-0.726803, -0.026178, 0.030680, 0.685661)),
    'leg_l_pistol_w': ((-0.219437, -0.032961, 0.859160), (-0.726548, -0.032277, 0.023078, 0.685970)),
    'leg_l_revolver_m': ((-0.234336, -0.001024, 0.786133), (-0.724718, -0.028770, 0.027289, 0.687904)),
    'leg_l_revolver_w': ((-0.222943, -0.019334, 0.837638), (-0.723588, -0.053967, 0.031111, 0.687415)),
    'leg_l_overture_m': ((-0.232533, -0.010636, 0.796175), (-0.731558, -0.031752, 0.035522, 0.680112)),
    'leg_l_overture_w': ((-0.219811, -0.025235, 0.850248), (-0.726310, -0.038940, 0.026242, 0.685761)),
    'leg_l_malorian_m': ((-0.232978, -0.023210, 0.816881), (-0.730467, -0.044117, 0.037945, 0.680465)),
    'leg_l_malorian_w': ((-0.220473, -0.036983, 0.872323), (-0.729358, -0.058276, 0.045616, 0.680118)),
    'underarm_r_m': ((0.146798, 0.098839, 1.369810), (0.370078, -0.485333, 0.692505, -0.384618)),
    'underarm_r_w': ((0.123419, 0.105348, 1.375491), (0.412001, -0.467427, 0.687499, -0.372979)),
    'underarm_l_m': ((-0.147527, 0.101307, 1.370283), (-0.377334, -0.483730, 0.691376, 0.381608)),
    'underarm_l_w': ((-0.123433, 0.105823, 1.375138), (-0.412042, -0.467278, 0.686689, 0.374607)),
    'cknife_r_m': ((0.155705, 0.085980, 1.376757), (-0.478543, 0.857011, -0.190006, 0.020655)),
    'cknife_r_w': ((0.122489, 0.089164, 1.375921), (-0.398605, 0.900766, -0.168254, 0.037764)),
    'cknife_l_m': ((-0.155705, 0.087844, 1.376757), (0.478543, 0.857011, -0.190006, -0.020655)),
    'cknife_l_w': ((-0.122669, 0.088885, 1.375848), (0.397839, 0.900779, -0.169799, -0.038596)),
}
BODIES = {
    'm': ['base\\characters\\common\\player_base_bodies\\player_man_average\\t0_000_pma_base__full.mesh',
          'base\\characters\\common\\player_base_bodies\\player_man_average\\arms_hq\\a0_001_pma_base_hq__full.mesh'],
    'w': ['base\\characters\\common\\player_base_bodies\\player_female_average\\t0_000_pwa_base__full.mesh',
          'base\\characters\\common\\player_base_bodies\\player_female_average\\arms_hq\\a0_000_pwa_base_hq__full.mesh'],
}
SIDES = ('r', 'l')
KINDS = ('pistol', 'revolver', 'overture', 'malorian')


def posix(depot):
    return depot.replace('\\', '/')


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout[-3000:] + r.stderr[-3000:])
        raise SystemExit('failed: %s' % ' '.join(str(a) for a in args[:3]))
    return r.stdout


def regex_for(paths):
    return '^(' + '|'.join(p.replace('\\', '[\\\\/]').replace('.', '[.]') for p in paths) + ')$'


def ours(name, body):
    """The depot path of our copy `name` for body `body`."""
    return OWN_DEPOT + 'vrp_own_%s_%s.mesh' % (name, body)


def copies():
    """our copy (name, body) -> (the mod's mesh, the game piece whose materials it takes, decal size or None)."""
    out = {}
    for b in BODIES:
        out[('underarm', b)] = (REF_HARNESS % b, PIECES['underarm'], DECAL_VERTS['underarm'])
        for s in SIDES:
            out[('leg_' + s, b)] = (REF_LEG % (b, s), PIECES['leg'], DECAL_VERTS['leg'])
            out[('cknife_' + s, b)] = (REF_CKNIFE % (b, s), SHEATH, None)
    return out


# ---------------------------------------------------------------- export

def cmd_export():
    garments = list(PIECES.values()) + [SHEATH]
    paths = garments + [p for b in BODIES.values() for p in b]
    src = GAME / 'archive/pc/content' / ARCHIVE
    run(CLI, 'unbundle', src, '-o', SOURCE, '-r', regex_for(garments))
    run(CLI, 'uncook', src, '-o', SOURCE, '-gp', GAME, '-r', regex_for(paths))
    if not (SOURCE / posix(KNIFE).replace('.mesh', '.glb')).exists():
        run(CLI, 'uncook', GAME / 'archive/pc/content', '-o', SOURCE, '-gp', GAME, '-r', regex_for([KNIFE]))
    paths.append(KNIFE)
    missing = [p for p in paths if not (SOURCE / posix(p).replace('.mesh', '.glb')).exists()]
    print('export: %d game meshes, missing %s' % (len(paths), missing))
    if not VAL_ARCHIVE.exists():
        raise SystemExit('"Valentino Holsters" (%s) is not installed: its meshes are copied from it' % VAL_ARCHIVE.name)
    mods = sorted({m for m, _, _ in copies().values()})
    run(CLI, 'uncook', VAL_ARCHIVE, '-o', REF, '-gp', GAME, '-r', regex_for(mods))
    missing = [p for p in mods if not (REF / posix(p)).exists() or not (REF / posix(p).replace('.mesh', '.glb')).exists()]
    print('the mod: %d meshes, missing %s' % (len(mods), missing))


# ---------------------------------------------------------------- glb <-> bin

def glb_meshes(doc):
    """(mesh index, primitive) of the garment's own submeshes (not the joint spheres)."""
    out = []
    for mi, mesh in enumerate(doc['meshes']):
        if not mesh.get('name', '').startswith('submesh_'):
            continue
        for pr in mesh['primitives']:
            out.append((mi, pr))
    return out


def glb_to_bin(glb, out):
    doc, binary = H.read_glb(glb)
    parts = []
    for _, pr in glb_meshes(doc):
        at = pr['attributes']
        pos = H.accessor_fast(doc, binary, at['POSITION']).astype(np.float32)
        nrm = (H.accessor_fast(doc, binary, at['NORMAL']).astype(np.float32) if 'NORMAL' in at
               else np.zeros_like(pos))
        tan = (H.accessor_fast(doc, binary, at['TANGENT']).astype(np.float32) if 'TANGENT' in at
               else np.zeros((pos.shape[0], 4), np.float32))
        idx = H.accessor_fast(doc, binary, pr['indices']).astype(np.uint32).reshape(-1)
        parts.append(struct.pack('<ii', pos.shape[0], idx.shape[0] // 3) + pos.tobytes() + nrm.tobytes() +
                     tan.tobytes() + idx.tobytes())
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(struct.pack('<i', len(parts)) + b''.join(parts))


def bins_from(path):
    data = path.read_bytes()
    n = struct.unpack_from('<i', data, 0)[0]
    off = 4
    out = []
    for _ in range(n):
        nv, nt = struct.unpack_from('<ii', data, off)
        off += 8
        pos = np.frombuffer(data, np.float32, nv * 3, off).reshape(nv, 3); off += nv * 12
        nrm = np.frombuffer(data, np.float32, nv * 3, off).reshape(nv, 3); off += nv * 12
        tan = np.frombuffer(data, np.float32, nv * 4, off).reshape(nv, 4); off += nv * 16
        idx = np.frombuffer(data, np.uint32, nt * 3, off); off += nt * 12
        out.append((pos, nrm, tan, idx))
    return out


def bin_to_glb(src_glb, binfile, out):
    """The source glb with the garment's geometry replaced by `binfile`'s (same primitives, same vertex counts)."""
    doc, binary = H.read_glb(src_glb)
    arrays = {i: H.accessor_fast(doc, binary, i) for i in range(len(doc['accessors']))}
    used = {a['bufferView'] for a in doc['accessors']}
    keep_views = {vi: binary[v.get('byteOffset', 0):v.get('byteOffset', 0) + v['byteLength']]
                  for vi, v in enumerate(doc['bufferViews']) if vi not in used}
    new = bins_from(binfile)
    for (_, pr), (pos, nrm, tan, idx) in zip(glb_meshes(doc), new):
        at = pr['attributes']
        arrays[at['POSITION']] = pos.astype(arrays[at['POSITION']].dtype)
        if 'NORMAL' in at:
            arrays[at['NORMAL']] = nrm.astype(arrays[at['NORMAL']].dtype)
        if 'TANGENT' in at:
            arrays[at['TANGENT']] = tan.astype(arrays[at['TANGENT']].dtype)
        itype = np.uint16 if pos.shape[0] <= 65535 else np.uint32
        if idx.size == 0:
            idx = np.array([0, 0, 0], np.uint32)            # an emptied submesh keeps one degenerate triangle
        arrays[pr['indices']] = idx.astype(itype).reshape(-1, 1)
        doc['accessors'][at['POSITION']]['min'] = [float(x) for x in pos.min(0)]
        doc['accessors'][at['POSITION']]['max'] = [float(x) for x in pos.max(0)]
    H.write_glb(out, doc, arrays, keep_views)


def tool(*args):
    return run(EXE, *args)


def body_bin(body):
    """Both meshes of a base body as one .bin (its torso and legs, its arms)."""
    out = BINS / ('body_%s.bin' % body)
    if out.exists():
        return out
    parts = []
    for depot in BODIES[body]:
        b = BINS / ('body_%s_%d.bin' % (body, len(parts)))
        glb_to_bin(SOURCE / posix(depot).replace('.mesh', '.glb'), b)
        parts.append(b.read_bytes())
    count = sum(struct.unpack_from('<i', p, 0)[0] for p in parts)
    out.write_bytes(struct.pack('<i', count) + b''.join(p[4:] for p in parts))
    return out


# ---------------------------------------------------------------- geometry

def cmd_parts():
    """The bandolier's connected pieces (prim:piece, size, centre in the game's axes)."""
    BINS.mkdir(parents=True, exist_ok=True)
    b = BINS / 'sheath.bin'
    glb_to_bin(SOURCE / posix(SHEATH).replace('.mesh', '.glb'), b)
    for line in tool('parts', b).splitlines():
        r = json.loads(line)
        if r['verts'] >= 30:
            print(r)


def weapon_xyz(depot):
    """A weapon mesh's points (float32 xyz, game axes, the weapon's own frame)."""
    out = BINS / ('%s.xyz' % Path(depot).stem)
    doc, binary = H.read_glb(SOURCE / posix(depot).replace('.mesh', '.glb'))
    pts = []
    for _, pr in glb_meshes(doc):
        p = H.accessor_fast(doc, binary, pr['attributes']['POSITION']).astype(np.float32)
        pts.append(np.stack([p[:, 0], -p[:, 2], p[:, 1]], 1))
    np.concatenate(pts).astype(np.float32).tofile(out)
    return out


def pose_arg(key):
    p, q = REF_FITS[key]
    return 'pose=%s' % ','.join('%.7f' % v for v in tuple(p) + tuple(q))


def cmd_geom():
    """The glbs WolvenKit imports, each beside a copy of the mesh it goes onto."""
    BINS.mkdir(parents=True, exist_ok=True)
    for f in IMPORT.rglob('*'):
        if f.is_file():
            f.unlink()

    def stage(name, body, src_glb, binfile, mesh):
        dst = IMPORT / posix(ours(name, body)).replace('.mesh', '.glb')
        dst.parent.mkdir(parents=True, exist_ok=True)
        bin_to_glb(src_glb, binfile, dst)
        shutil.copyfile(mesh, dst.with_suffix('.mesh'))

    # the harness: the game's, the woman's carried onto her body
    src = SOURCE / posix(PIECES['harness']).replace('.mesh', '.glb')
    b = BINS / 'harness_m.bin'
    glb_to_bin(src, b)
    stage('harness', 'm', src, b, SOURCE / posix(PIECES['harness']))
    print('harness_w', tool('refit', body_bin('m'), body_bin('w'), b, BINS / 'harness_w.bin').strip())
    stage('harness', 'w', src, BINS / 'harness_w.bin', SOURCE / posix(PIECES['harness']))
    # the knife sheaths: the mod's, taking the whole knife
    knives = {}
    for body in BODIES:
        for side in SIDES:
            key = 'cknife_%s_%s' % (side, body)
            glb = REF / posix(REF_CKNIFE % (body, side)).replace('.mesh', '.glb')
            glb_to_bin(glb, BINS / ('%s_mod.bin' % key))
            r = json.loads(tool('sheathfit', BINS / ('%s_mod.bin' % key), weapon_xyz(KNIFE), BINS / ('%s.bin' % key),
                                pose_arg(key)).strip().splitlines()[-1])
            print('%-14s %s' % (key, {k: r[k] for k in ('flipped', 'tiltDeg', 'shiftMm', 'riseMm', 'outsideMm',
                                                       'stretch')}))
            knives[key] = {'p': r['p'], 'q': r['q']}
            stage('cknife_' + side, body, glb, BINS / ('%s.bin' % key), REF / posix(REF_CKNIFE % (body, side)))
    (WORK / 'knife_fits.json').write_text(json.dumps(knives, indent=1), encoding='utf-8')
    print('geom: %d glbs' % len(list(IMPORT.rglob('*.glb'))))


# ---------------------------------------------------------------- materials

def handle_ids(node, out):
    if isinstance(node, dict):
        if 'HandleId' in node:
            out.append(int(node['HandleId']))
        for v in node.values():
            handle_ids(v, out)
    elif isinstance(node, list):
        for v in node:
            handle_ids(v, out)
    return out


def setup_of(mat):
    for v in mat.get('values') or []:
        if 'MultilayerSetup' in v:
            return v
    return None


def add_black(d):
    """A `black` appearance on a mesh (its JSON form): the `default` one's chunks, with its multilayered material
    swapped for one using the Maelstrom setup (the piece's own Maelstrom material when it has one, else a copy of the
    default's)."""
    r = d['Data']['RootChunk']
    entries = r['materialEntries']
    local = r['localMaterialBuffer']['materials']
    by_name = {e['name']['$value']: e for e in entries}
    if any(a['Data']['name']['$value'] == 'black' for a in r['appearances']):
        return 'already'
    app0 = next(a for a in r['appearances'] if a['Data']['name']['$value'] == 'default')
    main = next(c['$value'] for c in app0['Data']['chunkMaterials']
                if setup_of(local[by_name[c['$value']]['index']]) is not None)
    black = next((e['name']['$value'] for e in entries if setup_of(local[e['index']]) is not None and
                  setup_of(local[e['index']])['MultilayerSetup']['DepotPath']['$value'] == MAELSTROM_LOOK), None)
    if black is None:
        mat = copy.deepcopy(local[by_name[main]['index']])
        setup_of(mat)['MultilayerSetup']['DepotPath']['$value'] = MAELSTROM_LOOK
        local.append(mat)
        entry = copy.deepcopy(by_name[main])
        entry['name']['$value'] = black = 'black'
        entry['index'] = len(local) - 1
        entries.append(entry)
    app = copy.deepcopy(app0)
    app['HandleId'] = str(max(handle_ids(d, [])) + 1)
    app['Data']['name']['$value'] = 'black'
    for c in app['Data']['chunkMaterials']:
        if c['$value'] == main:
            c['$value'] = black
    r['appearances'].append(app)
    return 'black'


def give_materials(target, donor, looks, decal_verts):
    """The donor mesh's materials onto the target mesh (both JSON forms); the target gets the donor's appearances
    `looks`, each chunk taking the look's main material, or its decal material where the chunk has `decal_verts`
    vertices."""
    tr, dr = target['Data']['RootChunk'], donor['Data']['RootChunk']
    for k in ('materialEntries', 'localMaterialBuffer', 'localMaterialInstances', 'externalMaterials',
              'preloadExternalMaterials', 'preloadLocalMaterialInstances'):
        if k in dr:
            tr[k] = copy.deepcopy(dr[k])
    chunks = tr['renderResourceBlob']['Data']['header']['renderChunkInfos']
    dapps = {a['Data']['name']['$value']: a for a in dr['appearances']}
    next_id = max(handle_ids(target, [0])) + 1
    apps = []
    for look in looks:
        mats = [c['$value'] for c in dapps[look]['Data']['chunkMaterials']]
        main = mats[0]
        decal = next((m for m in mats if m != main), main)
        app = copy.deepcopy(dapps[look])
        app['HandleId'] = str(next_id)
        next_id += 1
        names = [decal if decal_verts and c['numVertices'] == decal_verts else main for c in chunks]
        app['Data']['chunkMaterials'] = [cname(n) for n in names]
        apps.append(app)
    tr['appearances'] = apps
    return [c['numVertices'] for c in chunks]


def cmd_import():
    r = subprocess.run([str(CLI), 'import', str(IMPORT), '-o', str(IMPORT), '-k'], capture_output=True, text=True)
    done = re.search(r'Imported (\d+)/(\d+)', r.stdout + r.stderr)
    n = len(list(IMPORT.rglob('*.glb')))
    if not done or done.group(1) != done.group(2) or int(done.group(1)) != n:
        print((r.stdout + r.stderr)[-3000:])
        raise SystemExit('import failed')
    print('import:', done.group(0))
    # the materials, through the JSON forms: the donors (the game's pieces), our copies of the mod's meshes
    stage = WORK / 'materials'
    if stage.exists():
        shutil.rmtree(stage)
    meshes, js, out = stage / 'mesh', stage / 'json', stage / 'out'
    for d in (meshes, js, out):
        d.mkdir(parents=True)
    own = IMPORT / posix(OWN_DEPOT)
    for donor in set(PIECES.values()) | {SHEATH}:
        shutil.copyfile(SOURCE / posix(donor), meshes / ('donor_' + Path(donor).name))
    for (name, body), (mod, _, _) in copies().items():
        mine = own / Path(ours(name, body)).name
        shutil.copyfile(mine if mine.exists() else REF / posix(mod), meshes / mine.name)   # the knife: imported
    for body in BODIES:
        shutil.copyfile(own / Path(ours('harness', body)).name, meshes / Path(ours('harness', body)).name)
    run(CLI, 'convert', 'serialize', meshes, '-o', js)

    def load(name):
        return json.loads((js / (name + '.json')).read_text(encoding='utf-8-sig'))

    def save(name, d):
        (js / (name + '.json')).write_text(json.dumps(d, indent=1), encoding='utf-8')

    donors = {}
    for donor in set(PIECES.values()) | {SHEATH}:
        d = load('donor_' + Path(donor).name)
        if donor != SHEATH:
            add_black(d)
        donors[donor] = d
    for body in BODIES:
        name = Path(ours('harness', body)).name
        d = load(name)
        add_black(d)
        save(name, d)
    for (name, body), (mod, donor, decal) in copies().items():
        mine = Path(ours(name, body)).name
        d = load(mine)
        looks = ['default', 'maelstrom'] if donor == SHEATH else ['default', 'black']
        counts = give_materials(d, donors[donor], looks, decal)
        save(mine, d)
        print('%-26s %d chunks %s' % (mine, len(counts), counts))
    for f in js.glob('donor_*'):
        f.unlink()
    run(CLI, 'convert', 'deserialize', js, '-o', out)
    done = list(out.glob('*.mesh'))
    for f in done:
        shutil.copyfile(f, own / f.name)
    print('materials on %d copies' % len(done))


# ---------------------------------------------------------------- fits

def cmd_fit():
    """fits.json: each weapon's pose in its shell (REF_FITS, the knives' from geom) and the bind poses of the bones
    the shells hang from."""
    fits = {k: {'p': list(p), 'q': list(q)} for k, (p, q) in REF_FITS.items()}
    fits.update(json.loads((WORK / 'knife_fits.json').read_text(encoding='utf-8')))
    binds = {}
    for body in BODIES:
        glb = SOURCE / posix(BODIES[body][0]).replace('.mesh', '.glb')
        binds['chest_%s' % body] = glb_bind(glb, 'Spine3')
        binds['hips_%s' % body] = glb_bind(glb, 'Hips')
    WORK.mkdir(parents=True, exist_ok=True)
    (WORK / 'fits.json').write_text(json.dumps({'fits': fits, 'binds': binds}, indent=1), encoding='utf-8')
    print('fits: %d poses' % len(fits))


def glb_bind(path, bone):
    sys.path.insert(0, str(HERE))
    import make_holster_shell as S  # noqa: E402
    p, q = S.glb_bind(path, bone)
    return {'p': p, 'q': q}


# ---------------------------------------------------------------- entities

def depot(path):
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': path}, 'Flags': 'Default'}


def cname(s):
    return {'$type': 'CName', '$storage': 'string', '$value': s}


def shells():
    """shell entity name -> [(our mesh copy, its chunks shown (None: all), its look)]."""
    out = {}
    for b in BODIES:
        for s in SIDES:
            for k in KINDS:
                out['leg_%s_%s_%s' % (s, k, b)] = [(ours('leg_' + s, b), LEG_CHUNKS[k], 'black')]
            out['underarm_%s_%s' % (s, b)] = [(ours('underarm', b), UNDERARM_CHUNKS[s], 'black')]
            out['cknife_%s_%s' % (s, b)] = [(ours('cknife_' + s, b), None, 'maelstrom')]
        out['harness_%s' % b] = [(ours('harness', b), None, 'black')]
    return out


def binding(handle, to):
    """A component's parentTransform: hard-bound to the component `to` (moves with it)."""
    return {'HandleId': str(handle), 'Data': {
        '$type': 'entHardTransformBinding', 'bindName': cname(to), 'enabled': 1,
        'enableMask': {'$type': 'entTagMask',
                       'excludedTags': {'$type': 'redTagList', 'tags': [cname('NoBinding')]},
                       'hardTags': {'$type': 'redTagList', 'tags': []},
                       'softTags': {'$type': 'redTagList', 'tags': []}},
        'slotName': cname('None')}}


def cmd_ent():
    ref = json.loads((RAW / 'vrport/ui/vr_lobby_glass.ent.json').read_text(encoding='utf-8-sig'))
    template = next(c for c in ref['Data']['RootChunk']['components'] if c['$type'] == 'entMeshComponent')
    entity = {'$type': 'entEntity', 'customCameraTarget': 'ECCTV_All', 'renderSceneLayerMask': 'Default'}
    out_dir = RAW / posix(SHELL_DEPOT)
    out_dir.mkdir(parents=True, exist_ok=True)
    for f in out_dir.glob('vrp_shell_*.ent.json'):
        f.unlink()
    cid = 2800000000000020001
    for name, meshes in sorted(shells().items()):
        comps = []
        for i, (mesh, chunks, look) in enumerate(meshes):
            c = copy.deepcopy(template)
            c['id'] = str(cid)
            cid += 1
            c['name'] = cname('shell' if i == 0 else 'shell%d' % i)
            c['mesh'] = depot(mesh)
            c['meshAppearance'] = cname(look)
            c['chunkMask'] = '18446744073709551615' if chunks is None else str(sum(1 << k for k in chunks))
            c['visualScale'] = {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1}
            for k in ('castShadows', 'castLocalShadows', 'castRayTracedGlobalShadows', 'castRayTracedLocalShadows'):
                c[k] = 'Always'
            comps.append(c)
        # ONE ROOT: every further mesh is bound to the first. With two parentless placed components the engine makes
        # a placeholder root the meshes do not follow, and the holster module moves that root onto the bone
        compiled = [copy.deepcopy(c) for c in comps]
        for i in range(1, len(comps)):
            comps[i]['parentTransform'] = binding(100 + i, 'shell')
            compiled[i]['parentTransform'] = binding(200 + i, 'shell')
        cruid = {'0': '0'}
        for i, c in enumerate(comps):
            cruid[str(i + 1)] = c['id']
        doc = {'Header': dict(ref['Header'], ArchiveFileName=''), 'Data': {
            'Version': ref['Data']['Version'], 'BuildVersion': ref['Data']['BuildVersion'], 'RootChunk': {
                '$type': 'entEntityTemplate', 'appearances': [], 'backendDataOverrides': [], 'bindingOverrides': [],
                'compiledData': {'BufferId': '0', 'Flags': 4063232,
                                 'Type': ref['Data']['RootChunk']['compiledData']['Type'],
                                 'Data': {'Version': 4, 'Sections': 7, 'CruidIndex': 0, 'CruidDict': cruid,
                                          'Chunks': [dict(entity)] + compiled}},
                'compiledEntityLODFlags': 0, 'componentResolveSettings': [], 'components': comps,
                'cookingPlatform': 'PLATFORM_PC', 'defaultAppearance': cname('default'),
                'entity': {'HandleId': '0', 'Data': dict(entity)},
                'includeInstanceBuffer': None, 'includes': [], 'inplaceResources': [], 'localData': None,
                'resolvedDependencies': [], 'visualTagsSchema': None},
            'EmbeddedFiles': []}}
        (out_dir / ('vrp_shell_%s.ent.json' % name)).write_text(json.dumps(doc, indent=1) + '\n', encoding='utf-8',
                                                                 newline='\n')
    print('%d shell entities in %s' % (len(shells()), out_dir.relative_to(ROOT)))


# ---------------------------------------------------------------- the module's table

def cmd_lua():
    data = json.loads((WORK / 'fits.json').read_text(encoding='utf-8'))
    fits, binds = data['fits'], data['binds']

    def vec(v):
        return '{ %s }' % ', '.join('%.6f' % x for x in v)

    def pose(f):
        return '{ p = %s, q = %s }' % (vec(f['p']), vec(f['q']))

    def ent(n):
        return '"%s"' % (SHELL_DEPOT + 'vrp_shell_%s.ent' % n).replace('\\', '\\\\')

    lines = ['-- Generated by tools/holsters/make_own_holsters.py (copies of "Valentino Holsters"\' meshes): do not edit.',
             '-- Poses { p, q } are in the body\'s model space (a weapon\'s: its frame -> the model; a bone\'s: its bind',
             '-- pose); q is { i, j, k, r }. See modules/shells.lua.',
             'return {',
             '  bind = {']
    for b in ('m', 'w'):
        lines.append('    %s = { chest = %s, hips = %s },' % (b, pose(binds['chest_' + b]), pose(binds['hips_' + b])))
    lines += ['  },', '  -- the leg holster by kind of handgun: the entity and the pose of the weapon in it', '  leg = {']
    for s in SIDES:
        lines.append('    %s = {' % s)
        for k in KINDS:
            lines.append('      %s = { %s },' % (k, ', '.join('%s = { ent = %s, fit = %s }' % (
                b, ent('leg_%s_%s_%s' % (s, k, b)), pose(fits['leg_%s_%s_%s' % (s, k, b)])) for b in ('m', 'w'))))
        lines.append('    },')
    lines += ['  },', '  -- the harness (both shoulder straps)',
              '  harness = { m = %s, w = %s },' % (ent('harness_m'), ent('harness_w')),
              '  -- the holster under the arm and the chest knife sheath, with the pose of the weapon in it',
              '  underarm = {']
    for s in SIDES:
        lines.append('    %s = { %s },' % (s, ', '.join('%s = { ent = %s, fit = %s }' % (
            b, ent('underarm_%s_%s' % (s, b)), pose(fits['underarm_%s_%s' % (s, b)])) for b in ('m', 'w'))))
    lines += ['  },', '  cknife = {']
    for s in SIDES:
        lines.append('    %s = { %s },' % (s, ', '.join('%s = { ent = %s, fit = %s }' % (
            b, ent('cknife_%s_%s' % (s, b)), pose(fits['cknife_%s_%s' % (s, b)])) for b in ('m', 'w'))))
    lines += ['  },', '}']
    LUA_OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(LUA_OUT.relative_to(ROOT))


# ---------------------------------------------------------------- pack

def cmd_pack():
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene as N  # noqa: E402
    for f in (N.PACK / posix(OWN_DEPOT)).glob('*.mesh'):
        f.unlink()                      # copies no longer made leave the archive
    for f in IMPORT.rglob('*.mesh'):
        dst = N.PACK / f.relative_to(IMPORT)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(f, dst)
    rels = [posix(SHELL_DEPOT) + 'vrp_shell_%s.ent' % n for n in sorted(shells())]
    N.pack_sources(rels, WORK / 'shell_stage')
    N.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['export', 'geom', 'import', 'fit', 'ent', 'lua', 'pack']
    for step in steps:
        {'export': cmd_export, 'parts': cmd_parts, 'geom': cmd_geom, 'import': cmd_import, 'fit': cmd_fit,
         'ent': cmd_ent, 'lua': cmd_lua, 'pack': cmd_pack}[step]()
