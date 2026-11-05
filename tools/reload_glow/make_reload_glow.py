"""The reload's glow: a gun's magazine drawn again in the game's amber pick-up highlight, over the magazine itself.

    py -3.13 -B tools/reload_glow/make_reload_glow.py              all of it
    py -3.13 -B tools/reload_glow/make_reload_glow.py meshes       the glow copies, their materials and the Lua list
    py -3.13 -B tools/reload_glow/make_reload_glow.py player       the two carriers on the player entities
    py -3.13 -B tools/reload_glow/make_reload_glow.py pack         deserialize into the pack, pack the archive

WHAT IS DRAWN. An empty magazine in the gun pulses, and once it is out the gun shows where it goes
(CyberpunkVRPort_Reload reload/glow.lua). Both are the gun's own magazine mesh, as the game has it, on the game's
pick-up highlight in amber: the shape is the magazine's and nothing has to be modelled per gun. The copy is drawn
over the gun's own magazine by a carrier on the player (vrp_glow_left / vrp_glow_right, on the hand slots the gun
and the held magazine already use), so its geometry is pushed out along its normals by INFLATE: drawn on the very
surface of the magazine it would fight it for the depth. The normals are averaged over the vertices that share a
place first, or the shell would open along every hard edge.

THE PULSE is the shader's own: base\\materials\\loot_drop_highlight.mt -- the belt's marks' material
(tools/make_glow_meshes.py) -- animates its highlight on the GPU, smooth at any frame rate. Two appearances: vrp_glow
pulses, vrp_glow_ghost is steady (APPEARANCES). The Lua only switches the copy on and picks one. (It used to step
through ten hologram materials a step brighter each, from the Lua: seven changes a second, a 5 fps animation.)

The magazines are the reload configs' own (`mesh = '...'` in reload/weapons/*.lua): a gun is in the list once it
has a physical reload of its magazine.

THE PULLED PART (`bolts`, in the same list): while rounds wait on the slide, the part the hand pulls (the config's
`slide` bone) pulses the same way. Its copy is the gun's own: the LOD 0 triangles of the body meshes its appearances
wear that are skinned to that bone -- and to the riders that go with it as one piece (ratio 1, no delay) -- written
in that bone's frame (the meshes' boneRigMatrices) and imported onto a copy of the lobby's plane mesh. The carrier
draws it on the weapon's vrp_<bone> slot, which reports the bone's live transform: the glow is wherever the part is
along its travel. A gun with no rig of its own, no such slot or no mesh of the part keeps the reach marker's brackets.
"""
import base64
import copy
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/vr_lobby_assets'), str(ROOT / 'tools/ent')]
import gltf  # noqa: E402
import make_lobby_scene as S  # noqa: E402
import make_construct as C  # noqa: E402
import add_hold_components as H  # noqa: E402
from make_lobby_scene import cname, depot  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\archive\pc')
PACK_ARCHIVE = ROOT / 'mods/archive/cyberpunkvrport.archive'
CONFIGS = ROOT / 'mods/cet/CyberpunkVRPort_Reload/reload/weapons'
LUA_LIST = ROOT / 'mods/cet/CyberpunkVRPort_Reload/reload/glow_list.lua'
WORK = ROOT / 'build/reload-glow/work'
BOLTS_WORK = ROOT / 'build/reload-glow/bolts'
RIGID = 0.03       # a rider whose ratio is this near 1 (and that does not lag) moves as one piece with the pulled part
BOUND = 0.5        # a vertex with this much of its weight on the part's bones is the part's
RAW = ROOT / 'mods/archive/source/raw/vrport/reload_glow'
DEPOT = 'vrport\\reload_glow\\'
CARRIERS = ('vrp_glow_left', 'vrp_glow_right')

INFLATE = 0.0015   # m along the normals: clear of the magazine's own surface, not visibly larger
HIGHLIGHT = 'base\\materials\\loot_drop_highlight.mt'
AMBER = (255, 170, 40)     # the belt's own amber mark (vrp_mark_amber.mi)
# appearance: (material, HighlightIntensity, SolidBlendingDistanceStart, End). The template's own blending range,
# 2..50 m, leaves everything nearer pulsing; 0..0.01 m stops the pulse (0..0 does not: measured on the belt's marks).
# The intensity is the belt mark's, which reads on a magazine.
APPEARANCES = {'vrp_glow': ('glow_pulse', 0.16, 2.0, 50.0),
               'vrp_glow_ghost': ('glow_steady', 0.10, 0.0, 0.01)}


def highlight_doc(intensity, start, end):
    return C.material_doc([S.value('color', 'HighlightColor', AMBER), S.value('float', 'HighlightIntensity', intensity),
                           S.value('float', 'SolidBlendingDistanceStart', start),
                           S.value('float', 'SolidBlendingDistanceEnd', end)], HIGHLIGHT)


# ---------------------------------------------------------------- the magazines

def magazines():
    """The game paths of the magazine meshes the reload configs name, sorted."""
    found = set()
    for f in CONFIGS.glob('*.lua'):
        for m in re.finditer(r"mesh\s*=\s*'([^']+\.mesh)'", f.read_text(encoding='utf-8')):
            found.add(m.group(1).replace('\\\\', '\\'))
    return sorted(found)


def glow_path(game_path):
    return DEPOT + game_path.rsplit('\\', 1)[1]


def extract(paths):
    """The game's meshes, serialized: {game path: document}."""
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    pattern = '^(%s)$' % '|'.join(re.escape(p) for p in paths)
    for folder in (GAME / 'content', GAME / 'ep1'):
        S.run(S.CLI, 'unbundle', folder, '-o', WORK, '-r', pattern)
    S.run(S.CLI, 'convert', 'serialize', WORK, '-o', WORK, '-w', '*.mesh')
    docs = {}
    for p in paths:
        f = WORK / (p.rsplit('\\', 1)[1] + '.json')       # the serializer writes them side by side, by name
        if not f.exists():
            raise SystemExit('not in the game: ' + p)
        docs[p] = json.loads(f.read_text(encoding='utf-8'))
    return docs


# ---------------------------------------------------------------- the geometry

SIZES = {'PT_Float1': 4, 'PT_Float2': 8, 'PT_Float3': 12, 'PT_Float4': 16, 'PT_Float16_2': 4, 'PT_Float16_4': 8,
         'PT_UShort1': 2, 'PT_UShort2': 4, 'PT_UShort4': 8, 'PT_UShort4N': 8, 'PT_Short1': 2, 'PT_Short2': 4,
         'PT_Short4': 8, 'PT_Short4N': 8, 'PT_UInt1': 4, 'PT_UInt2': 8, 'PT_UInt3': 12, 'PT_UInt4': 16,
         'PT_Int1': 4, 'PT_Int2': 8, 'PT_Int3': 12, 'PT_Int4': 16, 'PT_Color': 4, 'PT_UByte1': 1, 'PT_UByte1F': 1,
         'PT_UByte4': 4, 'PT_UByte4N': 4, 'PT_Byte4N': 4, 'PT_Dec4': 4}


def element(ci, usage):
    """(stream, offset in the vertex, stride of the stream, type) of a vertex element of a chunk."""
    els = [e for e in ci['chunkVertices']['vertexLayout']['elements']['Elements'] if e['type'] != 'PT_Invalid']
    at = {}
    for e in els:
        s = e['streamIndex']
        if e['usage'] == usage and e.get('usageIndex', 0) == 0 and usage not in at:
            at[usage] = (s, sum(SIZES[x['type']] for x in els[:els.index(e)] if x['streamIndex'] == s), e['type'])
    stride = {}
    for e in els:
        stride[e['streamIndex']] = stride.get(e['streamIndex'], 0) + SIZES[e['type']]
    s, off, kind = at[usage]
    return s, off, stride[s], kind


def pushed(pos, normals):
    """Positions pushed out by INFLATE along their normals, one normal a place (the mean of its vertices'): pushed
    each along its own, the shell would open along every hard edge."""
    _, group = np.unique(np.round(pos / 1e-5).astype(np.int64), axis=0, return_inverse=True)
    group = group.reshape(-1)
    mean = np.zeros((group.max() + 1, 3))
    np.add.at(mean, group, normals)
    mean /= np.maximum(np.linalg.norm(mean, axis=1, keepdims=True), 1e-9)
    return pos + mean[group] * INFLATE


def inflate(root):
    """Every chunk pushed out by INFLATE along its normals averaged per place; the quantization made to fit."""
    blob = root['renderResourceBlob']['Data']
    h, buf = blob['header'], bytearray(base64.b64decode(blob['renderBuffer']['Bytes']))
    q, o = h['quantizationScale'], h['quantizationOffset']
    scale, offset = np.array([q['X'], q['Y'], q['Z']]), np.array([o['X'], o['Y'], o['Z']])
    chunks = []
    for ci in h['renderChunkInfos']:
        n = ci['numVertices']
        starts = ci['chunkVertices']['byteOffsets']['Elements']
        ps, poff, pstride, pkind = element(ci, 'PS_Position')
        ns, noff, nstride, nkind = element(ci, 'PS_Normal')
        assert pkind == 'PT_Short4N' and nkind == 'PT_Dec4', (pkind, nkind)
        prow = np.frombuffer(bytes(buf), dtype=np.uint8, count=pstride * n, offset=starts[ps]).reshape(n, pstride)
        raw = prow[:, poff:poff + 8].copy().view('<i2').reshape(n, 4)
        nrow = np.frombuffer(bytes(buf), dtype=np.uint8, count=nstride * n, offset=starts[ns]).reshape(n, nstride)
        packed = nrow[:, noff:noff + 4].copy().view('<u4').reshape(n)
        normals = np.stack([((packed >> (10 * i)) & 0x3FF) / 1023.0 * 2.0 - 1.0 for i in range(3)], 1)
        pos = raw[:, :3] / 32767.0 * scale + offset
        chunks.append((starts[ps], pstride, poff, raw, pushed(pos, normals)))
    every = np.concatenate([c[4] for c in chunks])
    lo, hi = every.min(0), every.max(0)
    new_offset = (lo + hi) / 2.0
    new_scale = np.maximum((hi - lo) / 2.0 * 1.0001, 1e-4)
    for start, stride, poff, raw, pos in chunks:
        n = len(pos)
        out = raw.copy()
        out[:, :3] = np.clip(np.round((pos - new_offset) / new_scale * 32767.0), -32767, 32767).astype('<i2')
        rows = np.frombuffer(buf, dtype=np.uint8, count=stride * n, offset=start).reshape(n, stride)
        rows[:, poff:poff + 8] = out.view(np.uint8).reshape(n, 8)
    q['X'], q['Y'], q['Z'] = (float(v) for v in new_scale)
    o['X'], o['Y'], o['Z'] = (float(v) for v in new_offset)
    blob['renderBuffer']['Bytes'] = base64.b64encode(bytes(buf)).decode()
    box = root.get('boundingBox')
    if box:
        for k, axis in enumerate('XYZ'):
            box['Min'][axis] = float(lo[k])
            box['Max'][axis] = float(hi[k])


# ---------------------------------------------------------------- the bolts

def lua_block(text, name):
    m = re.search(r"\n    %s\s*=\s*\{(.*?)\n    \}" % name, text, re.S)
    return m.group(1) if m else None


def bolt_guns():
    """The reload configs whose slide the hand pulls: {name, mag (the magazine mesh, the gun's key in the list),
    folder (the gun's), bone (the pulled part's on the gun's rig), riders (the rig bones that go with it as one
    piece: ratio 1, no delay)}."""
    out = []
    for f in sorted(CONFIGS.glob('*.lua')):
        t = f.read_text(encoding='utf-8')
        slide, mag = lua_block(t, 'slide'), lua_block(t, 'mag')
        bone = slide and re.search(r"\bbone\s*=\s*(\d+)", slide)
        mesh = mag and re.search(r"mesh\s*=\s*'([^']+\.mesh)'", mag)
        if not (bone and mesh):
            continue
        riders = [int(b) for x, b, r in re.findall(r"(\{\s*bone\s*=\s*(\d+)\s*,\s*ratio\s*=\s*([\d.]+)[^}]*\})", slide)
                  if abs(float(r) - 1.0) <= RIGID and 'delay' not in x]
        path = mesh.group(1).replace('\\\\', '\\')
        out.append({'name': f.stem, 'mag': path, 'folder': path.split('\\entities\\')[0], 'bone': int(bone.group(1)),
                    'riders': riders})
    return out


def extract_guns(guns):
    """The guns' meshes and rigs from the game, and their appearances -- the game's, with the pack's own over them
    (the pack's carry the vrp_<bone> slots) -- serialized under BOLTS_WORK/json."""
    if BOLTS_WORK.exists():
        shutil.rmtree(BOLTS_WORK)
    (BOLTS_WORK / 'json').mkdir(parents=True)
    (BOLTS_WORK / 'files').mkdir()
    folders = '|'.join(re.escape(g['folder']) for g in guns)
    for source, kinds in ((GAME / 'content', 'mesh|rig|app'), (GAME / 'ep1', 'mesh|rig|app'), (PACK_ARCHIVE, 'app')):
        S.run(S.CLI, 'unbundle', source, '-o', BOLTS_WORK / 'files', '-r', '^(%s)\\\\.*\\.(%s)$' % (folders, kinds))
    for kind in ('mesh', 'rig', 'app'):
        S.run(S.CLI, 'convert', 'serialize', BOLTS_WORK / 'files', '-o', BOLTS_WORK / 'json', '-w', '*.' + kind)


def read_json(path):
    return json.loads(path.read_text(encoding='utf-8'))['Data']['RootChunk']


def in_folder(gun, kind):
    """The serialized files of a kind under the gun's folder (the serializer writes them side by side, by name)."""
    folder = BOLTS_WORK / 'files' / gun['folder'].replace('\\', '/')
    return [BOLTS_WORK / 'json' / (f.name + '.json') for f in sorted(folder.rglob('*.' + kind))]


def names(lst):
    return [n['$value'] if isinstance(n, dict) else n for n in lst or []]


def at(ci, e):
    """(stream, offset in the vertex, stride of the stream) of a vertex element of a chunk."""
    els = [x for x in ci['chunkVertices']['vertexLayout']['elements']['Elements'] if x['type'] != 'PT_Invalid']
    s = e['streamIndex']
    return (s, sum(SIZES[x['type']] for x in els[:els.index(e)] if x['streamIndex'] == s),
            sum(SIZES[x['type']] for x in els if x['streamIndex'] == s))


def chunk_geometry(header, buf, ci, bones):
    """A chunk's positions and normals (model space), its triangles, and each vertex's weight on `bones` (mesh bone
    indices) together."""
    n = ci['numVertices']
    starts = ci['chunkVertices']['byteOffsets']['Elements']
    q, o = header['quantizationScale'], header['quantizationOffset']
    ps, poff, pstride, pkind = element(ci, 'PS_Position')
    ns, noff, nstride, nkind = element(ci, 'PS_Normal')
    assert pkind == 'PT_Short4N' and nkind == 'PT_Dec4', (pkind, nkind)
    raw = np.frombuffer(buf, np.uint8, pstride * n, starts[ps]).reshape(n, pstride)[:, poff:poff + 8]
    pos = raw.copy().view('<i2').reshape(n, 4)[:, :3] / 32767.0 * np.array([q['X'], q['Y'], q['Z']]) + \
        np.array([o['X'], o['Y'], o['Z']])
    packed = np.frombuffer(buf, np.uint8, nstride * n, starts[ns]).reshape(n, nstride)[:, noff:noff + 4]
    packed = packed.copy().view('<u4').reshape(n)
    nrm = np.stack([((packed >> (10 * i)) & 0x3FF) / 1023.0 * 2.0 - 1.0 for i in range(3)], 1)
    els = [e for e in ci['chunkVertices']['vertexLayout']['elements']['Elements'] if e['type'] != 'PT_Invalid']
    w = np.zeros(n)
    for e in els:
        if e['usage'] != 'PS_SkinIndices':
            continue
        we = next(x for x in els if x['usage'] == 'PS_SkinWeights' and x.get('usageIndex', 0) == e.get('usageIndex', 0))
        (si, so, sst), (wi, wo, wst) = at(ci, e), at(ci, we)
        idx = np.frombuffer(buf, np.uint8, sst * n, starts[si]).reshape(n, sst)[:, so:so + 4]
        wgt = np.frombuffer(buf, np.uint8, wst * n, starts[wi]).reshape(n, wst)[:, wo:wo + 4] / 255.0
        w += (wgt * np.isin(idx, bones)).sum(1)
    wide = ci['chunkIndices']['pe'] != 'IBCT_IndexUShort'
    start = header['indexBufferOffset'] + ci['chunkIndices']['teOffset']
    tri = np.frombuffer(buf, '<u4' if wide else '<u2', ci['numIndices'], start).astype(np.int64).reshape(-1, 3)
    return pos, nrm, tri, w


def bolt_shape(meshes, frame_bone, bones):
    """The pulled part of a gun: the LOD 0 triangles of its meshes whose corners are all on `bones` (names), in the
    frame of `frame_bone` -- what the weapon's vrp_<bone> slot reports. Positions pushed out by INFLATE, normals
    there, triangles counter-clockwise seen from outside. None when no mesh has the part."""
    frame = None
    for root in meshes:
        bn = names(root.get('boneNames'))
        if frame_bone in bn:
            m = root['boneRigMatrices'][bn.index(frame_bone)]
            # Rows X Y Z the axes, W the translation, a point a row vector: model space -> the bone's (measured: a
            # part's vertices land round the origin this way, and well off it the other).
            frame = np.array([[m[r]['X'], m[r]['Y'], m[r]['Z'], m[r]['W']] for r in 'XYZW'])
            break
    if frame is None:
        return None
    P, N, T = [], [], []
    count = 0
    for root in meshes:
        bn = names(root.get('boneNames'))
        on = [bn.index(b) for b in bones if b in bn]
        if not on:
            continue
        blob = root['renderResourceBlob']['Data']
        buf = base64.b64decode(blob['renderBuffer']['Bytes'])
        for ci in blob['header']['renderChunkInfos']:
            if not ci.get('lodMask', 1) & 1:
                continue
            pos, nrm, tri, w = chunk_geometry(blob['header'], buf, ci, on)
            keep = tri[(w[tri] > BOUND).all(1)]
            if not len(keep):
                continue
            used, local = np.unique(keep, return_inverse=True)
            P.append(pos[used])
            N.append(nrm[used])
            T.append(local.reshape(-1, 3) + count)
            count += len(used)
    if not P:
        return None
    pos, nrm, tri = np.concatenate(P), np.concatenate(N), np.concatenate(T)
    pos = np.c_[pos, np.ones(len(pos))] @ frame[:, :3]
    nrm = nrm @ frame[:3, :3]
    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-9)
    # The game's winding is whichever its meshes have; written counter-clockwise, the way glTF has them.
    face = np.cross(pos[tri[:, 1]] - pos[tri[:, 0]], pos[tri[:, 2]] - pos[tri[:, 0]])
    if (np.einsum('ij,ij->i', face, nrm[tri].sum(1)) < 0).sum() > len(tri) / 2:
        tri = tri[:, [0, 2, 1]]
    return pushed(pos, nrm), nrm, tri


def gun_meshes(gun, rig_name):
    """The meshes of the gun's own body, as its appearances wear them: every appearance file's first appearance, the
    skinned meshes named after the gun's rig (attachments and the receivers of other variants are not the gun's
    body), shadows and decals left out."""
    wanted = set()
    for a in in_folder(gun, 'app'):
        apps = read_json(a).get('appearances') or []
        first = apps and apps[0].get('Data', apps[0])
        for c in (first or {}).get('components') or []:
            m = c.get('mesh')
            if not m or c['$type'] != 'entSkinnedMeshComponent':
                continue
            name = str(m['DepotPath']['$value']).rsplit('\\', 1)[-1]
            if name.startswith(rig_name) and 'shadow' not in name and 'decal' not in name:
                wanted.add(name)
    return [read_json(p) for p in in_folder(gun, 'mesh') if p.name[:-5] in wanted]


def has_slot(gun, slot):
    """The pack's appearances of the gun carry the slot (a slot on a bone reports that bone's live transform)."""
    return any(('"%s"' % slot) in a.read_text(encoding='utf-8') for a in in_folder(gun, 'app'))


def bolts():
    """The pulled parts' glow copies, imported onto copies of the lobby's plane mesh: {magazine mesh: (depot path of
    the copy, the slot it is drawn on, its document)}."""
    guns = bolt_guns()
    if '--reuse' not in sys.argv:
        extract_guns(guns)
    imports = BOLTS_WORK / 'import'
    if imports.exists():
        shutil.rmtree(imports)
    imports.mkdir()
    planned = {}
    for gun in guns:
        if gun['mag'] in planned:
            continue                       # a gun of the same body and magazine as one already made (Unity, Liberty)
        rigs = [r for r in in_folder(gun, 'rig') if '__mag' not in r.name]
        if not rigs:
            print('  %-26s no rig of its own: the brackets' % gun['name'])
            continue
        rig = names(read_json(rigs[0])['boneNames'])
        pulled = rig[gun['bone']]
        slot = 'vrp_' + pulled
        if not has_slot(gun, slot):
            print('  %-26s no %s slot: the brackets' % (gun['name'], slot))
            continue
        shape = bolt_shape(gun_meshes(gun, rigs[0].name[:-len('.rig.json')]), pulled,
                           [pulled] + [rig[b] for b in gun['riders'] if b < len(rig)])
        if shape is None:
            print('  %-26s no mesh has %s: the brackets' % (gun['name'], pulled))
            continue
        pos, nrm, tri = shape
        name = 'bolt_' + gun['name']
        b = gltf.Builder()
        g = b.group('glow')
        # Any tangent across the normal: the shader's normal map is flat.
        tan = np.cross(nrm, np.where(np.abs(nrm[:, 2:3]) < 0.9, [[0, 0, 1]], [[1, 0, 0]]))
        tan /= np.maximum(np.linalg.norm(tan, axis=1, keepdims=True), 1e-9)
        g['pos'] = [tuple(map(float, p)) for p in pos]
        g['nrm'] = [tuple(map(float, n)) for n in nrm]
        g['tan'] = [tuple(map(float, t)) for t in tan]
        g['uv'] = [(0.0, 0.0)] * len(pos)
        g['idx'] = [int(i) for i in tri.reshape(-1)]
        b.write(str(imports / (name + '.glb')), name)
        shutil.copyfile(S.TEMPLATE_MESH, imports / (name + '.mesh'))
        planned[gun['mag']] = (name, slot)
        print('  %-26s %s on %s: %d triangles' % (gun['name'], pulled, slot, len(tri)))
    # The importer exits non-zero over its garment-support warning even when the files went in: its own count says.
    r = subprocess.run([str(S.CLI), 'import', str(imports), '-o', str(imports), '-k'], capture_output=True, text=True)
    if 'Imported %d/%d file(s)' % (len(planned), len(planned)) not in r.stdout:
        print(r.stdout[-3000:], r.stderr[-2000:])
        raise SystemExit('bolt import failed')
    S.run(S.CLI, 'convert', 'serialize', imports, '-o', imports, '-w', '*.mesh')
    out = {}
    for mag, (name, slot) in planned.items():
        doc = json.loads((imports / (name + '.mesh.json')).read_text(encoding='utf-8-sig'))
        out[mag] = (DEPOT + name + '.mesh', slot, glow_look(doc))
    return out


def glow_look(doc):
    """The glow's appearances on a mesh document (APPEARANCES), every chunk on that appearance's material."""
    doc['Header']['ArchiveFileName'] = ''
    root = doc['Data']['RootChunk']
    first = root['appearances'][0]
    chunks = len(first.get('Data', first)['chunkMaterials'])
    # A handle id is the document's, not the list's: numbered past every other handle the mesh keeps.
    root['appearances'] = []
    base = H.max_handle(doc) + 1
    looks = [(name, material) for name, (material, *_) in APPEARANCES.items()]
    root['appearances'] = [{'HandleId': str(base + k), 'Data': {
        '$type': 'meshMeshAppearance', 'chunkMaterials': [cname(material)] * chunks,
        'name': cname(name), 'tags': []}} for k, (name, material) in enumerate(looks)]
    root['materialEntries'] = [{'$type': 'CMeshMaterialEntry', 'index': k, 'isLocalInstance': 0,
                                'name': cname(material)} for k, (_, material) in enumerate(looks)]
    root['externalMaterials'] = [{'DepotPath': depot(DEPOT + material + '.mi'), 'Flags': 'Default'}
                                 for _, material in looks]
    root['preloadExternalMaterials'] = []
    root['preloadLocalMaterialInstances'] = []
    root['localMaterialInstances'] = []
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer', 'materials': [], 'rawData': None,
                                   'rawDataHeaders': []}
    root['forceLoadAllAppearances'] = 0
    return doc


def cmd_meshes():
    paths = magazines()
    print(len(paths), 'magazines')
    docs = extract(paths)
    if RAW.exists():
        shutil.rmtree(RAW)
    RAW.mkdir(parents=True)
    for material, intensity, start, end in APPEARANCES.values():
        doc = highlight_doc(intensity, start, end)
        (RAW / (material + '.mi.json')).write_text(json.dumps(doc, indent=2), encoding='utf-8')
    for p, doc in docs.items():
        inflate(doc['Data']['RootChunk'])
        glow_look(doc)
        name = glow_path(p).rsplit('\\', 1)[1]
        (RAW / (name + '.json')).write_text(json.dumps(doc, indent=1), encoding='utf-8')
    print('bolts:')
    parts = bolts()
    for mesh, slot, doc in parts.values():
        (RAW / (mesh.rsplit('\\', 1)[1] + '.json')).write_text(json.dumps(doc, indent=1), encoding='utf-8')

    def lua(s):
        return s.replace('\\', '\\\\')
    lines = ['-- Generated by tools/reload_glow/make_reload_glow.py: a gun\'s magazine mesh -> its glow copy, drawn',
             '-- over the magazine by the glow carrier (reload/glow.lua); and -> the glow copy of the part its hand',
             '-- pulls, drawn on the weapon\'s slot of that part. Appearances vrp_glow (pulsing, the shader\'s own',
             '-- animation) and vrp_glow_ghost (steady).',
             'return {', '    meshes = {']
    for p in paths:
        lines.append("        ['%s'] = '%s'," % (lua(p), lua(glow_path(p))))
    lines += ['    },', '    bolts = {']
    for p in sorted(parts):
        mesh, slot, _ = parts[p]
        lines.append("        ['%s'] = { mesh = '%s', slot = '%s' }," % (lua(p), lua(mesh), slot))
    lines += ['    },', '}', '']
    LUA_LIST.write_text('\n'.join(lines), encoding='utf-8', newline='\n')
    print('glow copies', len(docs), 'magazines,', len(parts), 'bolts ->', RAW)


# ---------------------------------------------------------------- the carriers

def cmd_player():
    """vrp_glow_left / vrp_glow_right on every player template that carries the hold carriers, then those templates
    deserialized into the pack.

    THE CARRIERS GO INTO THE TEMPLATES' SOURCES (mods/archive/source/raw), not into the pack's cooked copies: a tool
    that rebuilds a template builds it from its source (tools/neck_cap), and carriers added only to the cooked copy
    went with the first such rebuild -- the glow had nothing to draw on and no error said so. The cooked copy keeps
    its component names in a compressed buffer, so it cannot be searched for them either."""
    raw = ROOT / 'mods/archive/source/raw'
    built = []
    for js in sorted(raw.rglob('*.ent.json')):
        root = json.loads(js.read_text(encoding='utf-8'))['Data']['RootChunk']
        if 'vrp_hold_left' not in [H.named(c) for c in root.get('components') or []]:
            continue
        where, what = H.patch(str(js), *CARRIERS)
        print('%-40s %s' % (js.name, what))
        built.append(js.relative_to(raw).with_suffix(''))
    if not built:
        raise SystemExit('no player template carries vrp_hold_left')
    S.pack_sources(built, ROOT / 'build/reload-glow/ents')


def cmd_pack():
    target = S.PACK / 'vrport/reload_glow'
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    S.run(S.CLI, 'convert', 'deserialize', RAW, '-o', target)
    print('reload glow', len(list(target.glob('*'))), 'files')
    S.pack_archive()


if __name__ == '__main__':
    steps = [a for a in sys.argv[1:] if not a.startswith('--')] or ['meshes', 'player', 'pack']
    for step in steps:
        {'meshes': cmd_meshes, 'player': cmd_player, 'pack': cmd_pack}[step]()
