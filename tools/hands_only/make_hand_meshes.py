"""The player's arms cut at the wrists, closed, for the hands-only mode.

    py -3.13 -B tools/hands_only/make_hand_meshes.py            all steps
    py -3.13 -B tools/hands_only/make_hand_meshes.py export     the arm meshes -> glb (build/hands-only-20261103/trim)
    py -3.13 -B tools/hands_only/make_hand_meshes.py trim       each glb cut at the wrist and capped (hand_trim.exe)
    py -3.13 -B tools/hands_only/make_hand_meshes.py import     WolvenKit import onto copies of the game's meshes
    py -3.13 -B tools/hands_only/make_hand_meshes.py entity     a vrp_hands__* component per mesh in the player entities
    py -3.13 -B tools/hands_only/make_hand_meshes.py lua        mods/cet/CyberpunkVRPort_HandsOnly/data/hand_meshes.lua
    py -3.13 -B tools/hands_only/make_hand_meshes.py pack       meshes + entities into the pack, the archive packed

WHY. Hands only draws the player's hands and nothing else. The bodies' own arms are cut by region into render chunks,
the hand its own chunk, so a chunk mask leaves the hand (make_hand_masks.py) -- but with an open ring at the wrist,
and every arm cyberware's skin is one chunk from the fingers to the middle of the forearm: masked, the Gorilla Arms
or the Mantis Blades kept a forearm with a ragged end. So each arm mesh gets a copy of its own with every triangle
past the wrist removed and the cut closed (tools/hands_only/hand_trim.cpp: the heavy geometry, in C++), imported onto
a copy of the game's mesh (its materials, appearances and chunks as they are, so a skin tone and a chunk mask mean
the same on it), as a plain skinned component of the player, off until the mode wants it.

THE CUT: 3 cm from the hand joint toward the elbow, the plane square to the forearm (forearm joint -> hand joint in
the bind pose). A chunk entirely past it (the upper arm, the forearm) is left as it is and masked off; the table
says which chunks of each copy to draw.

AT RUNTIME (CyberpunkVRPort_HandsOnly): the game's arm component is masked to nothing but stays the game's -- enabled,
disabled, re-masked as the game decides -- and its copy follows it: on while it is on, its skin tone, the game's
mask ANDed with the copy's own.
"""
import json
import re
import shutil
import struct
import subprocess
import sys
import zlib
import copy as _copy
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/hands-only-20261103/trim'
SOURCE, IMPORT, ENTITIES = WORK / 'source', WORK / 'import', WORK / 'entities'
ANALYSIS = ROOT / 'build/hands-only-20261103/all'
TRIM = Path(__file__).resolve().parent / 'hand_trim.exe'
PACK = ROOT / 'build/one-archive-20261027/pack/cyberpunkvrport'
RAW = ROOT / 'mods/archive/source/raw'
LUA_OUT = ROOT / 'mods/cet/CyberpunkVRPort_HandsOnly/data/hand_meshes.lua'
ARCHIVE = 'basegame_4_appearance.archive'
BS = chr(92)
OURS = BS.join(['vrport', 'body', 'hands'])
CUT, BAND = 0.03, 0.04
PREFIX = 'vrp_hands__'
ENTITY_FILES = {
    'base/characters/entities/player/player_ma_fpp.ent': 'ma',
    'base/characters/entities/player/player_wa_fpp.ent': 'wa',
    'ep1/characters/entities/player/player_ma_fpp_ep1.ent': 'ma',
    'ep1/characters/entities/player/player_wa_fpp_ep1.ent': 'wa',
}
TEMPLATE = 'vrp_latd_screen'    # a plain skinned mesh component of the port's, bound to the skeleton's root

# The first-person arms: the bodies' own and the arm cyberware's skin and hands. Not the pieces that are the
# cyberware's weapon (blades, wire, shells -- kept whole by the mask table), not the forearm parts (upper arms,
# mechanisms), not the photo mode or third-person variants, not the decals.
INCLUDE = re.compile(r'(_hq__(l|r|full)|_fpp__full_(l|r)|strongarms_(holstered_)?(cyberware_)?(l|r)|'
                     r'mantisblade_(holstered_)?(left|right)|launcher_holstered|monowire_whip_(l|r)(_cableless)?|'
                     r'monowire_whip_personal_link_l|monowire_personal_link_whip_l)\.mesh$')
EXCLUDE = re.compile(r'photo|_tpp|_dec|tattoo|injection|mud|blood|nails|fists|_blade|mechanisms|upperarm|_int_')


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode:
        print((r.stdout + r.stderr)[-3000:])
        raise SystemExit('failed: %s' % (args[:2],))
    return r.stdout + r.stderr


def fnv1a64(text):
    h = 0xCBF29CE484222325
    for b in text.encode('utf-8'):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def selected():
    index = json.loads((ANALYSIS / 'index.json').read_text(encoding='utf-8'))
    out = sorted({rel for rel in index.values() if INCLUDE.search(rel.lower()) and not EXCLUDE.search(rel.lower())})
    return out


def stem(depot):
    return depot.split(BS)[-1][:-len('.mesh')]


_NAMES = {}


def name_of(depot):
    """The copy's name: the mesh's own, with a few hex digits of its path when two meshes share a file name (the
    Mantis Blades have two of each, in the cyberware's folder and in its entities/meshes)."""
    if not _NAMES:
        all_ = selected()
        counts = {}
        for d in all_:
            counts[stem(d)] = counts.get(stem(d), 0) + 1
        for d in all_:
            _NAMES[d] = stem(d) if counts[stem(d)] == 1 else '%s_%06x' % (stem(d), fnv1a64(d.lower()) & 0xFFFFFF)
    return _NAMES.get(depot, stem(depot))


def ours(depot):
    return OURS + BS + name_of(depot) + '.mesh'


def posix(depot):
    return depot.replace(BS, '/')


# ---------------------------------------------------------------- export

def cmd_export():
    meshes = selected()
    rx = '^(' + '|'.join(m.replace(BS, BS + BS).replace('.', BS + '.') for m in meshes) + ')$'
    src = GAME / 'archive/pc/content' / ARCHIVE
    run(CLI, 'unbundle', src, '-o', SOURCE, '-r', rx)
    run(CLI, 'uncook', src, '-o', SOURCE, '-gp', GAME, '-r', rx)
    missing = [m for m in meshes if not (SOURCE / posix(m).replace('.mesh', '.glb')).exists()]
    print('export: %d meshes, %d missing %s' % (len(meshes), len(missing), missing[:4]))


# ---------------------------------------------------------------- glb

def read_glb(path):
    data = path.read_bytes()
    n = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + n])
    m = struct.unpack_from('<I', data, 20 + n)[0]
    return doc, data[20 + n + 8:20 + n + 8 + m]


COMP = {5126: np.float32, 5125: np.uint32, 5123: np.uint16, 5122: np.int16, 5121: np.uint8, 5120: np.int8}
WIDTH = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}


def accessor(doc, binary, i):
    a = doc['accessors'][i]
    v = doc['bufferViews'][a['bufferView']]
    dt = np.dtype(COMP[a['componentType']])
    k = WIDTH[a['type']]
    stride = v.get('byteStride') or k * dt.itemsize
    start = v.get('byteOffset', 0) + a.get('byteOffset', 0)
    rows = np.frombuffer(binary, dtype=np.uint8, count=(a['count'] - 1) * stride + k * dt.itemsize, offset=start)
    out = np.empty((a['count'], k), dtype=dt)
    for r in range(a['count']):
        out[r] = np.frombuffer(rows[r * stride:r * stride + k * dt.itemsize].tobytes(), dtype=dt)
    return out


def accessor_fast(doc, binary, i):
    a = doc['accessors'][i]
    v = doc['bufferViews'][a['bufferView']]
    dt = np.dtype(COMP[a['componentType']])
    k = WIDTH[a['type']]
    stride = v.get('byteStride') or k * dt.itemsize
    start = v.get('byteOffset', 0) + a.get('byteOffset', 0)
    if stride == k * dt.itemsize:
        return np.frombuffer(binary, dtype=dt, count=a['count'] * k, offset=start).reshape(a['count'], k).copy()
    return accessor(doc, binary, i)


def write_glb(path, doc, arrays, keep_views):
    """`arrays`: accessor index -> numpy array (count x width) of its component type. Every accessor gets a tightly
    packed view of its own; views no accessor uses (images) are copied from `keep_views` as they were."""
    blob = bytearray()
    views = []
    remap = {}
    for vi, raw in keep_views.items():
        while len(blob) % 4:
            blob.append(0)
        remap[vi] = len(views)
        views.append({'buffer': 0, 'byteOffset': len(blob), 'byteLength': len(raw)})
        blob += raw
    for ai, a in enumerate(doc['accessors']):
        arr = np.ascontiguousarray(arrays[ai])
        while len(blob) % 4:
            blob.append(0)
        a['bufferView'] = len(views)
        a.pop('byteOffset', None)
        a['count'] = int(arr.shape[0])
        a['componentType'] = {np.dtype(v): k for k, v in COMP.items()}[arr.dtype]
        views.append({'buffer': 0, 'byteOffset': len(blob), 'byteLength': arr.nbytes})
        blob += arr.tobytes()
    for img in doc.get('images', []):
        if 'bufferView' in img:
            img['bufferView'] = remap[img['bufferView']]
    doc['bufferViews'] = views
    doc['buffers'] = [{'byteLength': len(blob)}]
    body = json.dumps(doc, separators=(',', ':')).encode()
    body += b' ' * (-len(body) % 4)
    blob += b'\0' * (-len(blob) % 4)
    total = 12 + 8 + len(body) + 8 + len(blob)
    out = struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(body), 0x4E4F534A) + body
    out += struct.pack('<II', len(blob), 0x004E4942) + bytes(blob)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


def sides_of(doc, binary):
    """(hand joint position, unit forearm->hand axis) for each arm the skin has, in the glb's bind pose."""
    skin = doc['skins'][0]
    ibm = accessor_fast(doc, binary, skin['inverseBindMatrices'])
    names = [doc['nodes'][j].get('name') for j in skin['joints']]

    def at(n):
        m = ibm[names.index(n)].reshape(4, 4).T          # glTF matrices are column-major
        return np.linalg.inv(m)[:3, 3]

    out = []
    for side, s in (('Left', 'l'), ('Right', 'r')):
        # the wrist: the hand joint, or on the rig the Mantis Blades' drawn arms use, its inner wrist joint
        hand = next((n for n in (side + 'Hand', s + '_innerWrist01_JNT') if n in names), None)
        if hand is not None and side + 'ForeArm' in names:
            h, f = at(hand), at(side + 'ForeArm')
            out.append((h, (h - f) / np.linalg.norm(h - f)))
    return out


# ---------------------------------------------------------------- trim

def trim_one(src, dst):
    doc, binary = read_glb(src)
    arrays = {i: accessor_fast(doc, binary, i) for i in range(len(doc['accessors']))}
    used = {a['bufferView'] for a in doc['accessors']}
    keep_views = {}
    for vi, v in enumerate(doc['bufferViews']):
        if vi not in used:
            keep_views[vi] = binary[v.get('byteOffset', 0):v.get('byteOffset', 0) + v['byteLength']]
    sides = sides_of(doc, binary)
    if not sides:
        return None
    chunks = []
    for mi, mesh in enumerate(doc['meshes']):
        m = re.match(r'submesh_(\d+)_LOD_(\d+)', mesh.get('name', ''))
        chunk = int(m.group(1)) if m else mi
        pr = mesh['primitives'][0]
        at = pr['attributes']
        pos = arrays[at['POSITION']].astype(np.float32)
        nrm = arrays[at['NORMAL']].astype(np.float32)
        idx = arrays[pr['indices']].astype(np.uint32).reshape(-1)
        head = struct.pack('<III', pos.shape[0], idx.shape[0], len(sides))
        for h, ax in sides:
            head += struct.pack('<3f3f', *h, *ax)
        head += struct.pack('<ff', CUT, BAND)
        tin, tout = WORK / 'tmp_in.bin', WORK / 'tmp_out.bin'
        tin.write_bytes(head + pos.tobytes() + nrm.tobytes() + idx.tobytes())
        r = subprocess.run([str(TRIM), str(tin), str(tout)], capture_output=True, text=True)
        if r.returncode:
            raise SystemExit('hand_trim: %s %s' % (src.name, r.stderr))
        res = tout.read_bytes()
        kc, nc, loops = struct.unpack_from('<III', res, 0)
        kept = np.frombuffer(res, dtype=np.uint32, count=kc, offset=12)
        new = np.frombuffer(res, dtype=np.uint8, count=nc * 32, offset=12 + kc * 4).reshape(nc, 32) if nc else None
        # a chunk the cut leaves nothing of is left whole, and masked off
        hand_left = kc > 0
        chunks.append({'chunk': chunk, 'triangles': idx.shape[0] // 3, 'kept': (kc // 3) if hand_left else 0,
                       'loops': loops, 'draw': hand_left})
        if not hand_left:
            continue
        if nc:
            # the cap's vertices go over vertices the cut left unused: the vertex count stays (see hand_trim.cpp)
            slot = new[:, 0:4].copy().view(np.uint32).reshape(-1)
            source = new[:, 4:8].copy().view(np.uint32).reshape(-1)
            npos = new[:, 8:20].copy().view(np.float32).reshape(-1, 3)
            nnrm = new[:, 20:32].copy().view(np.float32).reshape(-1, 3)
            for name, ai in at.items():
                arr = arrays[ai].copy()
                arr[slot] = arr[source]
                if name == 'POSITION':
                    arr[slot] = npos.astype(arr.dtype)
                elif name == 'NORMAL':
                    arr[slot] = nnrm.astype(arr.dtype)
                elif name == 'TANGENT':
                    n0 = nnrm[0]
                    ref = np.array([0, 0, 1], np.float32) if abs(n0[2]) < 0.9 else np.array([1, 0, 0], np.float32)
                    tg = np.cross(n0, ref)
                    tg /= np.linalg.norm(tg)
                    arr[slot] = np.array([tg[0], tg[1], tg[2], 1.0], np.float32).astype(arr.dtype)
                arrays[ai] = arr
        count = arrays[at['POSITION']].shape[0]
        itype = np.uint16 if count <= 65535 else np.uint32
        arrays[pr['indices']] = kept.astype(itype).reshape(-1, 1)
        p = arrays[at['POSITION']]
        doc['accessors'][at['POSITION']]['min'] = [float(x) for x in p.min(0)]
        doc['accessors'][at['POSITION']]['max'] = [float(x) for x in p.max(0)]
    write_glb(dst, doc, arrays, keep_views)
    return chunks


def cmd_trim():
    report = {}
    for depot in selected():
        src = SOURCE / posix(depot).replace('.mesh', '.glb')
        if not src.exists():
            print('no glb:', depot)
            continue
        dst = IMPORT / posix(ours(depot)).replace('.mesh', '.glb')
        chunks = trim_one(src, dst)
        if chunks is None:
            print('no arm joints:', depot)
            continue
        # the copy's CR2W: the game's mesh, its geometry replaced on import
        shutil.copyfile(SOURCE / posix(depot), IMPORT / posix(ours(depot)))
        report[depot] = chunks
        print('%-56s %s' % (name_of(depot), ' '.join('%d:%d/%d%s' % (c['chunk'], c['kept'], c['triangles'],
                                                                  '+cap%d' % c['loops'] if c['loops'] else '')
                                                    for c in chunks)))
    (WORK / 'trim.json').write_text(json.dumps(report, indent=1), encoding='utf-8')


# ---------------------------------------------------------------- import

def cmd_import():
    r = subprocess.run([str(CLI), 'import', str(IMPORT), '-o', str(IMPORT), '-k'], capture_output=True, text=True)
    done = re.search(r'Imported (\d+)/(\d+)', r.stdout + r.stderr)
    report = json.loads((WORK / 'trim.json').read_text(encoding='utf-8'))
    if not done or done.group(1) != done.group(2) or int(done.group(1)) != len(report):
        print((r.stdout + r.stderr)[-3000:])
        raise SystemExit('hand import failed')
    print('import:', done.group(0))


# ---------------------------------------------------------------- entity

def body_of(depot):
    s = stem(depot)
    return 'wa' if ('_wa__' in s or '_pwa_' in s) else 'ma'


def component_id(entity, name):
    return str((zlib.crc32(('%s|%s' % (entity, name)).encode()) << 32) | zlib.crc32(name.encode()))


def cmd_entity():
    report = json.loads((WORK / 'trim.json').read_text(encoding='utf-8'))
    for entity, body in ENTITY_FILES.items():
        path = RAW / (entity + '.json')
        text = path.read_text(encoding='utf-8')
        doc = json.loads(text)
        components = doc['Data']['RootChunk']['components']
        components[:] = [c for c in components if not (c.get('name') or {}).get('$value', '').startswith(PREFIX)]
        template = next(c for c in components if (c.get('name') or {}).get('$value') == TEMPLATE)
        if 'HandleRefId' not in template['parentTransform'] or 'HandleRefId' not in template['skinning']:
            raise SystemExit('%s: %s does not reference shared bindings' % (entity, TEMPLATE))
        added = 0
        for depot in sorted(report):
            if body_of(depot) != body:
                continue
            name = PREFIX + name_of(depot)
            c = _copy.deepcopy(template)
            c['name']['$value'] = name
            c['id'] = component_id(entity, name)
            c['mesh']['DepotPath']['$value'] = ours(depot)
            c['meshAppearance']['$value'] = 'default'
            # with the game's arms: a weapon out, the arms and the weapon go to the weapon plane, and a hand left in
            # the scene plane has the weapon drawn over it -- seen through the fingers (2026-11-05)
            c['renderingPlaneAnimationParam']['$value'] = 'renderPlane'
            c['visibilityAnimationParam']['$value'] = 'None'
            c['castShadows'] = c['castLocalShadows'] = 'Never'
            c['isEnabled'] = 0
            components.append(c)
            added += 1
        newline = '\r\n' if '\r\n' in text else '\n'
        out = json.dumps(doc, indent=2, ensure_ascii=False).replace('\n', newline)
        if text.endswith('\n'):
            out += newline
        path.write_bytes(out.encode('utf-8'))
        print('entity: %s + %d hand components' % (entity, added))


# ---------------------------------------------------------------- lua

def cmd_lua():
    report = json.loads((WORK / 'trim.json').read_text(encoding='utf-8'))
    lines = ['-- Generated by tools/hands_only/make_hand_meshes.py: do not edit.',
             '-- The game\'s arm mesh (the hash of its depot path, a component\'s mesh.hash) -> the player component that',
             '-- is its copy cut at the wrists (vrport\\body\\hands) and the mask of the copy\'s chunks to draw.',
             'return {']
    for depot in sorted(report):
        mask = 0
        for c in report[depot]:
            if c['draw']:
                mask |= 1 << c['chunk']
        lines.append('  ["%d"] = { "%s", %d }, -- %s' % (fnv1a64(depot.lower()), PREFIX + name_of(depot), mask,
                                                       depot.split(BS)[-1]))
    lines.append('}')
    LUA_OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('%s: %d meshes' % (LUA_OUT.relative_to(ROOT), len(report)))


# ---------------------------------------------------------------- pack

def cmd_pack():
    report = json.loads((WORK / 'trim.json').read_text(encoding='utf-8'))
    if ENTITIES.exists():
        shutil.rmtree(ENTITIES)
    for entity in ENTITY_FILES:
        src = RAW / (entity + '.json')
        stage = ENTITIES / Path(entity).parent
        stage.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, stage / src.name)
    run(CLI, 'convert', 'deserialize', ENTITIES, '-o', ENTITIES)
    for entity in ENTITY_FILES:
        built = ENTITIES / Path(entity).name
        if not built.exists():
            raise SystemExit('not deserialized: %s' % built)
        dst = PACK / entity
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(built, dst)
    for depot in report:
        dst = PACK / posix(ours(depot))
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(IMPORT / posix(ours(depot)), dst)
    print('pack: %d entities, %d meshes' % (len(ENTITY_FILES), len(report)))
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene  # noqa: E402 -- the port's one pack -> archive step
    make_lobby_scene.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['export', 'trim', 'import', 'entity', 'lua', 'pack']
    for step in steps:
        {'export': cmd_export, 'trim': cmd_trim, 'import': cmd_import, 'entity': cmd_entity, 'lua': cmd_lua,
         'pack': cmd_pack}[step]()
