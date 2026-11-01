"""The neck the first-person body keeps when the player looks down.

    py -3.13 -B tools/neck_cap/make_neck_cap.py            all of it
    py -3.13 -B tools/neck_cap/make_neck_cap.py export     the game's two FPP necks -> glb (build/neck-cap/source)
    py -3.13 -B tools/neck_cap/make_neck_cap.py weights    the cap made rigid on the chest -> build/neck-cap/import
    py -3.13 -B tools/neck_cap/make_neck_cap.py import     WolvenKit import onto copies of the game's meshes
    py -3.13 -B tools/neck_cap/make_neck_cap.py entity     the vrp_neck_cap component in the player entities (raw JSON)
    py -3.13 -B tools/neck_cap/make_neck_cap.py pack       meshes + entities into the pack folder, the archive packed

WHY. The first-person body is the full body mesh (t0_000_p?a_base__full) with its neck opening open: the head and
neck are not drawn in the first person. The game closes the opening with n0_000_p?a_fpp__neck.mesh, a low cone over
the stump -- but that cone belongs to the head item (Items.PlayerFppHead, slot TppHead), and nothing in that slot is
drawn in the first-person view, only in shadows. It still takes part in the garment assembly: the body's triangles
under it are removed as hidden (the garment's hidden triangle removal), so the opening grows by the cone's footprint.
The flat camera never looks down into it; in VR the player does, and sees through the chest -- the floor, or the
inside of a jacket, a black hole in the collar.

THE FIX is a cap the first-person view draws: the game's cone, copied to the port's own paths (vrport\\body), every
vertex but the rim welded to the body made rigid on Spine3 -- the chest the opening is skinned to, so the cap stays on
it whatever the head does (the port turns the neck bones with the headset) -- its own 1-2 cm hole at the top closed,
and added to the player entities as a
plain skinned mesh component, vrp_neck_cap, bound to the skeleton the way the wrist screen (vrp_latd_screen) is. Not a
garment, so it is drawn and removes nothing. It has the game cone's appearances, one per skin tone; the plugin gives it
the body's (Natives/NeckCap.cpp). The game's own cone and the game's meshes stay as they are.
"""
import collections
import copy
import json
import math
import re
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build/neck-cap'
SOURCE, IMPORT, ENTITIES = WORK / 'source', WORK / 'import', WORK / 'entities'
PACK = ROOT / 'build/one-archive-20261027/pack/cyberpunkvrport'
RAW = ROOT / 'mods/archive/source/raw'
ARCHIVE = 'basegame_4_appearance.archive'
BS = '\\'
# The game's cone -> the port's copy, per body.
MESHES = {
    'pma': (BS.join(['base', 'characters', 'common', 'player_base_bodies', 'player_man_average', 'fpp',
                     'n0_000_pma_fpp__neck.mesh']), BS.join(['vrport', 'body', 'vrp_neck_cap_pma.mesh'])),
    'pwa': (BS.join(['base', 'characters', 'common', 'player_base_bodies', 'player_female_average', 'fpp',
                     'n0_000_pwa_fpp__neck.mesh']), BS.join(['vrport', 'body', 'vrp_neck_cap_pwa.mesh'])),
}
# The player entities the port ships (the ones carrying vrp_latd_screen), and the body each is.
ENTITY_FILES = {
    'base/characters/entities/player/player_ma_fpp.ent': 'pma',
    'base/characters/entities/player/player_wa_fpp.ent': 'pwa',
    'ep1/characters/entities/player/player_ma_fpp_ep1.ent': 'pma',
    'ep1/characters/entities/player/player_wa_fpp_ep1.ent': 'pwa',
}
COMPONENT = 'vrp_neck_cap'
TEMPLATE = 'vrp_latd_screen'    # a plain skinned mesh component of the port's, bound to the skeleton's root
CHEST = 'Spine3'


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    text = r.stdout + r.stderr
    if r.returncode or 'Error' in text:
        print(text[-3000:])
    if r.returncode:
        raise SystemExit('failed: %s' % (args[:2],))
    return text


def posix(depot):
    return depot.replace(BS, '/')


def cmd_export():
    for game, _ in MESHES.values():
        pattern = '^%s$' % re.escape(game)
        run(CLI, 'unbundle', GAME / 'archive/pc/content' / ARCHIVE, '-o', SOURCE, '-r', pattern)
        run(CLI, 'uncook', GAME / 'archive/pc/content' / ARCHIVE, '-o', SOURCE, '-gp', GAME, '-r', pattern)
        glb = SOURCE / posix(game).replace('.mesh', '.glb')
        if not glb.exists():
            raise SystemExit('no %s' % glb)
        print('export:', glb.relative_to(ROOT))


# ---------------------------------------------------------------- glb

def read_glb(path):
    data = bytearray(path.read_bytes())
    n = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + n])
    return data, doc, 20 + n + 8      # the BIN chunk's data starts after its 8-byte header


def view(doc, base, index):
    """An accessor as (count, components, struct format of one component, stride, offset of element 0)."""
    a = doc['accessors'][index]
    v = doc['bufferViews'][a['bufferView']]
    fmt = {5126: 'f', 5125: 'I', 5123: 'H', 5121: 'B'}[a['componentType']]
    k = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}[a['type']]
    stride = v.get('byteStride') or k * struct.calcsize(fmt)
    start = base + v.get('byteOffset', 0) + a.get('byteOffset', 0)
    return a['count'], k, fmt, stride, start


def close_loop(loop, neighbours, keys, tris):
    """Triangles closing a boundary loop over its own vertices, wound against the boundary edges' direction in the
    faces next to them so they face the way those do. The ring is small but not convex in every projection (a fan
    left slivers facing down): ear clipping in the ring's own plane."""
    order, previous = [next(iter(loop))], None
    while True:
        step = [k for k in neighbours[order[-1]] if k != previous and k in loop]
        if not step or step[0] == order[0]:
            break
        previous = order[-1]
        order.append(step[0])
    if len(order) != len(loop):
        raise SystemExit('boundary loop does not walk round (%d of %d)' % (len(order), len(loop)))
    index = {}
    for i, k in enumerate(keys):
        index.setdefault(k, i)
    directed = {(keys[t[a]], keys[t[(a + 1) % 3]]) for t in tris for a in range(3)}
    if (order[0], order[1]) in directed:
        order.reverse()
    # The ring's plane: its centroid and the covariance's weakest direction; 2D coordinates in it.
    n = len(order)
    centre = [sum(k[c] for k in order) / n for c in range(3)]
    d = [[k[c] - centre[c] for c in range(3)] for k in order]
    cov = [[sum(p[a] * p[b] for p in d) for b in range(3)] for a in range(3)]
    normal = smallest_eigenvector(cov)
    u = [1.0, 0.0, 0.0] if abs(normal[0]) < 0.9 else [0.0, 1.0, 0.0]
    u = cross(cross(normal, u), normal)
    v = cross(normal, u)
    flat = [(sum(p[c] * u[c] for c in range(3)), sum(p[c] * v[c] for c in range(3))) for p in d]
    area = sum(flat[i][0] * flat[(i + 1) % n][1] - flat[(i + 1) % n][0] * flat[i][1] for i in range(n))
    sense = 1.0 if area > 0 else -1.0

    def turn(a, b, c):
        return ((flat[b][0] - flat[a][0]) * (flat[c][1] - flat[a][1]) -
                (flat[b][1] - flat[a][1]) * (flat[c][0] - flat[a][0])) * sense

    def inside(p, a, b, c):
        return turn(a, b, p) > 0 and turn(b, c, p) > 0 and turn(c, a, p) > 0

    left, out = list(range(n)), []
    while len(left) > 3:
        for i in range(len(left)):
            a, b, c = left[i - 1], left[i], left[(i + 1) % len(left)]
            if turn(a, b, c) <= 0 or any(inside(p, a, b, c) for p in left if p not in (a, b, c)):
                continue
            out.append((a, b, c))
            del left[i]
            break
        else:
            raise SystemExit('ear clipping found no ear in a ring of %d' % len(left))
    out.append(tuple(left))
    return [tuple(index[order[i]] for i in t) for t in out]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def smallest_eigenvector(m):
    """The eigenvector of a symmetric 3x3 matrix with the smallest eigenvalue (Jacobi rotations)."""
    a = [row[:] for row in m]
    vec = [[1.0 if i == j else 0.0 for j in range(3)] for i in range(3)]
    for _ in range(50):
        p, q = max(((0, 1), (0, 2), (1, 2)), key=lambda pq: abs(a[pq[0]][pq[1]]))
        if abs(a[p][q]) < 1e-18:
            break
        theta = 0.5 * math.atan2(2 * a[p][q], a[q][q] - a[p][p])
        c, s = math.cos(theta), math.sin(theta)
        for k in range(3):
            akp, akq = a[k][p], a[k][q]
            a[k][p], a[k][q] = c * akp - s * akq, s * akp + c * akq
        for k in range(3):
            apk, aqk = a[p][k], a[q][k]
            a[p][k], a[q][k] = c * apk - s * aqk, s * apk + c * aqk
        for k in range(3):
            vkp, vkq = vec[k][p], vec[k][q]
            vec[k][p], vec[k][q] = c * vkp - s * vkq, s * vkp + c * vkq
    j = min(range(3), key=lambda i: a[i][i])
    return [vec[0][j], vec[1][j], vec[2][j]]


def write_glb(path, doc, bin_data):
    body = json.dumps(doc, separators=(',', ':')).encode()
    body += b' ' * (-len(body) % 4)
    bin_data = bytes(bin_data) + b'\0' * (-len(bin_data) % 4)
    total = 12 + 8 + len(body) + 8 + len(bin_data)
    out = struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(body), 0x4E4F534A) + body
    out += struct.pack('<II', len(bin_data), 0x004E4942) + bin_data
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


def reweight(src, dst):
    data, doc, base = read_glb(src)
    closings = []
    joints = [doc['nodes'][i]['name'] for i in doc['skins'][0]['joints']]
    if CHEST not in joints:
        raise SystemExit('%s: no %s among its joints' % (src.name, CHEST))
    chest = joints.index(CHEST)
    moved = kept = 0
    for mesh in doc['meshes']:
        for prim in mesh['primitives']:
            attrs = prim['attributes']
            count, _, pf, ps, po = view(doc, base, attrs['POSITION'])
            keys = [tuple(round(c, 4) for c in struct.unpack_from('<3' + pf, data, po + i * ps)) for i in range(count)]
            icount, _, idf, ids, ido = view(doc, base, prim['indices'])
            tris = [[struct.unpack_from('<' + idf, data, ido + (3 * t + c) * ids)[0] for c in range(3)]
                    for t in range(icount // 3)]
            # The open boundary, by position (a seam splits a vertex): edges used by one triangle.
            edges = collections.Counter()
            for t in tris:
                for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
                    edges[tuple(sorted((keys[a], keys[b])))] += 1
            # The rim on the body: of the boundary's loops, the lower one (the other is the small hole at the
            # cone's top). glTF is Y up. The rim is tilted, so the loops are told apart by connection, not height.
            neighbours = collections.defaultdict(set)
            for (a, b), n in edges.items():
                if n == 1:
                    neighbours[a].add(b)
                    neighbours[b].add(a)
            loops, seen = [], set()
            for start in neighbours:
                if start in seen:
                    continue
                loop, todo = set(), [start]
                while todo:
                    k = todo.pop()
                    if k not in loop:
                        loop.add(k)
                        todo.extend(neighbours[k] - loop)
                seen |= loop
                loops.append(loop)
            rim = min(loops, key=lambda loop: sum(k[1] for k in loop) / len(loop))
            top = [loop for loop in loops if loop is not rim]
            if len(top) != 1:
                raise SystemExit('%s: %d boundary loops besides the rim' % (src.name, len(top)))
            closings.append((prim, tris, close_loop(top[0], neighbours, keys, tris)))
            # Eight influences: two joint/weight sets. The chest takes the first slot of the first set, every other
            # slot of both sets is cleared.
            sets = []
            for n in range(2):
                if 'JOINTS_%d' % n not in attrs:
                    continue
                _, jk, jf, js, jo = view(doc, base, attrs['JOINTS_%d' % n])
                _, wk, wf, ws, wo = view(doc, base, attrs['WEIGHTS_%d' % n])
                if wf != 'f':
                    raise SystemExit('%s: weights are not floats' % src.name)
                sets.append((jk, jf, js, jo, wk, wf, ws, wo))
            for i in range(count):
                if keys[i] in rim:
                    kept += 1
                    continue
                for n, (jk, jf, js, jo, wk, wf, ws, wo) in enumerate(sets):
                    struct.pack_into('<' + jf * jk, data, jo + i * js, *([chest if n == 0 else 0] + [0] * (jk - 1)))
                    struct.pack_into('<' + wf * wk, data, wo + i * ws, *([1.0 if n == 0 else 0.0] + [0.0] * (wk - 1)))
                moved += 1
    # The cone's top is a 1-2 cm hole (where the game's neck went on): closed, or the eye looks down through it.
    # The primitive gets a new index accessor -- its old triangles and the closing ones -- appended to the buffer.
    bin_data = bytearray(data[base:base + doc['buffers'][0]['byteLength']])
    for prim, tris, closing in closings:
        old = doc['accessors'][prim['indices']]
        fmt = {5125: 'I', 5123: 'H'}[old['componentType']]
        flat = [i for t in tris + closing for i in t]
        if fmt == 'H' and max(flat) > 0xFFFF:
            raise SystemExit('%s: indices outgrow 16 bits' % src.name)
        bin_data += b'\0' * (-len(bin_data) % 4)
        offset = len(bin_data)
        bin_data += struct.pack('<%d%s' % (len(flat), fmt), *flat)
        doc['bufferViews'].append({'buffer': 0, 'byteOffset': offset, 'byteLength': len(bin_data) - offset,
                                   'target': 34963})
        doc['accessors'].append({'bufferView': len(doc['bufferViews']) - 1, 'componentType': old['componentType'],
                                 'count': len(flat), 'type': 'SCALAR'})
        prim['indices'] = len(doc['accessors']) - 1
    doc['buffers'][0]['byteLength'] = len(bin_data)
    write_glb(dst, doc, bin_data)
    return moved, kept, sum(len(c) for _, _, c in closings)


def cmd_weights():
    if IMPORT.exists():
        shutil.rmtree(IMPORT)
    for game, ours in MESHES.values():
        moved, kept, closed = reweight(SOURCE / posix(game).replace('.mesh', '.glb'),
                                       IMPORT / posix(ours).replace('.mesh', '.glb'))
        shutil.copyfile(SOURCE / posix(game), IMPORT / posix(ours))       # the game's mesh: the import's template
        print('weights: %s -> %d vertices on %s, %d rim vertices as they were, top closed with %d triangles'
              % (Path(posix(ours)).name, moved, CHEST, kept, closed))


def cmd_import():
    # Onto the copies beside them (-k: the CR2W kept, its geometry buffer replaced). The importer exits non-zero
    # over its garment-support warning even when every file went in: its own count says whether it worked.
    r = subprocess.run([str(CLI), 'import', str(IMPORT), '-o', str(IMPORT), '-k'], capture_output=True, text=True)
    done = re.search(r'Imported (\d+)/(\d+)', r.stdout + r.stderr)
    if not done or done.group(1) != done.group(2) or done.group(1) != str(len(MESHES)):
        print((r.stdout + r.stderr)[-3000:])
        raise SystemExit('neck import failed')
    print('import:', done.group(0))


# ---------------------------------------------------------------- entities

def component_id(entity):
    """A stable CRUID for the component in one entity file (the same file always gets the same id)."""
    return str((zlib.crc32(('%s|%s' % (entity, COMPONENT)).encode()) << 32) | zlib.crc32(COMPONENT.encode()))


def cmd_entity():
    for entity, body in ENTITY_FILES.items():
        path = RAW / (entity + '.json')
        text = path.read_text(encoding='utf-8')
        doc = json.loads(text)
        components = doc['Data']['RootChunk']['components']
        components[:] = [c for c in components if (c.get('name') or {}).get('$value') != COMPONENT]
        template = next(c for c in components if (c.get('name') or {}).get('$value') == TEMPLATE)
        cap = copy.deepcopy(template)
        # The template's bindings are handle references to the skeleton root's transform and skinning, shared.
        if 'HandleRefId' not in cap['parentTransform'] or 'HandleRefId' not in cap['skinning']:
            raise SystemExit('%s: %s does not reference shared bindings' % (entity, TEMPLATE))
        cap['name']['$value'] = COMPONENT
        cap['id'] = component_id(entity)
        cap['mesh']['DepotPath']['$value'] = MESHES[body][1]
        cap['meshAppearance']['$value'] = '02_ca_limestone'
        cap['renderingPlaneAnimationParam']['$value'] = 'None'
        cap['visibilityAnimationParam']['$value'] = 'None'
        cap['castShadows'] = cap['castLocalShadows'] = 'Never'
        cap['isEnabled'] = 1
        components.append(cap)
        # Written the way WolvenKit writes it (a load and dump reproduces these files byte for byte), keeping
        # the file's line endings and its final newline.
        newline = '\r\n' if '\r\n' in text else '\n'
        out = json.dumps(doc, indent=2, ensure_ascii=False).replace('\n', newline)
        if text.endswith('\n'):
            out += newline
        path.write_bytes(out.encode('utf-8'))
        print('entity: %s + %s (%s)' % (entity, COMPONENT, MESHES[body][1]))


def cmd_pack():
    if ENTITIES.exists():
        shutil.rmtree(ENTITIES)
    for entity in ENTITY_FILES:
        src = RAW / (entity + '.json')
        stage = ENTITIES / Path(entity).parent
        stage.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, stage / src.name)
    run(CLI, 'convert', 'deserialize', ENTITIES, '-o', ENTITIES)
    for entity in ENTITY_FILES:
        built = ENTITIES / Path(entity).name      # the converter writes every file into the output folder itself
        if not built.exists():
            raise SystemExit('not deserialized: %s' % built)
        dst = PACK / entity
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(built, dst)
        print('pack:', entity)
    for game, ours in MESHES.values():
        dst = PACK / posix(ours)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(IMPORT / posix(ours), dst)
        print('pack:', posix(ours))
        stale = PACK / posix(game)      # an earlier build overrode the game's own cone: the game keeps its own
        if stale.exists():
            stale.unlink()
            print('pack: removed', posix(game))
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene  # noqa: E402 -- the port's one pack -> archive step
    make_lobby_scene.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['export', 'weights', 'import', 'entity', 'pack']
    for step in steps:
        {'export': cmd_export, 'weights': cmd_weights, 'import': cmd_import, 'entity': cmd_entity,
         'pack': cmd_pack}[step]()
