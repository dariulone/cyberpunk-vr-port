# -*- coding: utf-8 -*-
"""The belt for a left-handed player: the magazines, the shells and the grenades on the other hip.

WHY. The ammunition rides the side of the support hand and the grenades the side of the weapon hand: a
right-handed player draws a magazine from the front left and a grenade from the front right. A left-handed
player holds the gun in the left hand, so both have to change sides -- the magazines and the shells to the
front right, the grenades to the front left.

WHY NOT AT RUNTIME. The elements are skinned garment components whose place is baked into their meshes'
rig matrices; moving one with SetLocalPosition is an offset on top of a skinned mesh, and swapping meshes
with ChangeResource is the per-rebuild churn the belt module was rebuilt to stop. So the other side is an
asset of its own -- a belt item of its own, which the module puts on instead of the port's belt for a
left-handed player (a garment is merged onto the player when it is attached, and naming another appearance
afterwards does not merge it again).

    Items.vrp_belt_lh   (mods/tweaks/vrport/Items.vrp_belt_lh.yaml) entityName vrp_belt_lh: a row added to
                        base/vrport/vrp_props.csv -> vrp_belt_lh.ent
    vrp_belt_lh.ent     vrp_belt_lh_&Male&TPP / FPP, &Female&TPP / FPP -> vrp_belt_{m,w}_lh.app; ONLY those, so the
                        garment merged at attach (before any appearance is named) is already the mirrored one
    vrp_belt_*_lh.app   the belt's .app with its part -> vrp_belt_part_{m,w}_lh.ent
    vrp_belt_part_*_lh  the same components under the same names (so the module's rules, marks and reach
                        apply untouched), each element pointing at its mirrored mesh
    <mesh>_lh.mesh      the element's mesh with its rig matrices mirrored; no vertex is touched

THE MIRROR, in the rig matrices (row vectors, v_model = v . B_j . M_j in the bind pose, M_j the bone's bind
transform = the inverse of the skeleton's inverse bind matrix for that bone, read from a pristine garment):

    B_j' = Mx_raw . B_j . (M_j . Mx . M_j^-1)

    Mx              x -> -x in entity space: across the body
    M_j.Mx.M_j^-1   the same mirror in bone j's own space (for Hips and Spine, which sit on the body's plane
                    of symmetry, a flip of their lateral axis: printed as a check)
    Mx_raw          the mesh's own geometry mirrored across its own centre plane x = c_raw

so the element lands at v . Mx_raw . E . Mx, E = B_j . M_j its placement: mirrored to the other side. Two
mirrors make a rotation, so the triangles keep their winding and their faces stay outward -- the object is
moved to the other side, not turned inside out. A magazine, a shell or a grenade is symmetric about its own
centre plane, so it looks the same as before; anything that is not is still the real object, placed the
mirrored way. The bounding box (stored in the placed frame) goes through the same map, corner by corner.

Usage:
    python tools/mirror_belt_elements.py [--pack]      mirror into the one-archive pack folder; --pack also
                                                       packs mods/archive/cyberpunkvrport.archive
"""
import copy
import io
import json
import re
import shutil
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / 'vr_lobby_assets'))

from bake_mesh_transform import mul4, from_json, to_json  # noqa: E402
from bake_belt_pass import inv4  # noqa: E402
import make_lobby_scene as MS  # noqa: E402

V = '$value'
BS = '\\'
PACK = MS.PACK
BELT = 'base' + BS + 'vrport' + BS + 'belt' + BS
WORK = MS.ROOT / 'build' / 'belt-mirror'
TRACKED = MS.ROOT / 'mods' / 'archive' / 'build'
REFERENCE = HERE / 'ref' / 'hips_reference.mesh.json'
# The elements that change sides: the magazine pouches and the shell row (support hand), the grenades and
# their holder (weapon hand). Each with its mark twin.
MIRRORED = re.compile(r'^(ucbeltmag_|ucbeltammo_shell|ucbeltgrenade)')
PARTS = ('vrp_belt_part_m', 'vrp_belt_part_w')
APPS = ('vrp_belt_m', 'vrp_belt_w')
SUFFIX = '_lh'
MX = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]


def load(path):
    return json.load(io.open(path, encoding='utf-8'))


def save(doc, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    io.open(path, 'w', encoding='utf-8', newline='').write(json.dumps(doc, indent=2))


def serialize(cooked, out_dir):
    """A cooked file from the pack -> its JSON in out_dir."""
    tmp = out_dir / 'cooked'
    if tmp.exists():
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)
    shutil.copyfile(cooked, tmp / cooked.name)
    MS.run(MS.CLI, 'convert', 'serialize', tmp / cooked.name, '-o', out_dir)
    return load(out_dir / (cooked.name + '.json'))


def deserialize(doc, name, target_dir):
    """JSON -> the cooked `name` in target_dir (of the pack), and its tracked copy beside the belt's other
    assets (mods/archive/build mirrors the pack's base/vrport)."""
    tmp = WORK / 'deserialize'
    if tmp.exists():
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)
    save(doc, tmp / (name + '.json'))
    target_dir.mkdir(parents=True, exist_ok=True)
    began = time.time()
    log = MS.run(MS.CLI, 'convert', 'deserialize', tmp, '-o', target_dir)
    # The file has to be NEW: an older one of the same name standing there is no proof of anything.
    if not (target_dir / name).exists() or (target_dir / name).stat().st_mtime < began - 1:
        print(log[-2000:])
        raise SystemExit('deserialize gave no ' + name)
    tracked = TRACKED / (target_dir / name).relative_to(PACK)
    tracked.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(target_dir / name, tracked)


def bind_transforms():
    """Bone name -> its bind transform M_j (row vectors), from a pristine garment of this rig."""
    root = load(REFERENCE)['Data']['RootChunk']
    out = {}
    for name, m in zip(root['boneNames'], root['boneRigMatrices']):
        out[name[V]] = inv4(from_json(m.get('Data', m)))
    return out


def apply(p, m):
    return [sum(p[k] * m[k][j] for k in range(3)) + m[3][j] for j in range(3)]


def raw_mirror(c):
    """x -> 2c - x: the mesh's own geometry mirrored across its centre plane."""
    m = copy.deepcopy(MX)
    m[3][0] = 2.0 * c
    return m


def mirror_mesh(doc, binds, label):
    root = doc['Data']['RootChunk']
    names = [b[V] for b in root['boneNames']]
    mats = root['boneRigMatrices']
    bb = root['boundingBox']
    lo = [bb['Min'][k] for k in 'XYZ']
    hi = [bb['Max'][k] for k in 'XYZ']
    centre = [(lo[i] + hi[i]) / 2 for i in range(3)]
    # The placement and the element's centre in its own (raw) geometry, from the first bone: every bone of
    # an element places it the same way in the bind pose.
    B0 = from_json(mats[0].get('Data', mats[0]))
    E = mul4(B0, binds[names[0]])
    c_raw = apply(centre, inv4(E))[0]
    R_raw = raw_mirror(c_raw)
    worst = 0.0
    for name, m in zip(names, mats):
        if name not in binds:
            raise SystemExit('%s: bone %s is not in the reference rig' % (label, name))
        M = binds[name]
        local = mul4(mul4(M, MX), inv4(M))
        # The bone's own mirror should be a flip of one of its axes: off-diagonal terms are its asymmetry.
        worst = max(worst, max(abs(local[i][j]) for i in range(3) for j in range(3) if i != j))
        md = m.get('Data', m)
        to_json(mul4(mul4(R_raw, from_json(md)), local), md)
    # The box: each corner back into the raw geometry, mirrored there, placed, mirrored across the body.
    Ei = inv4(E)
    G = mul4(mul4(mul4(Ei, R_raw), E), MX)
    new_lo, new_hi = [1e30] * 3, [-1e30] * 3
    for i in range(8):
        p = [lo[0] if i & 1 else hi[0], lo[1] if i & 2 else hi[1], lo[2] if i & 4 else hi[2]]
        q = apply(p, G)
        for j in range(3):
            new_lo[j], new_hi[j] = min(new_lo[j], q[j]), max(new_hi[j], q[j])
    for j, k in enumerate('XYZ'):
        bb['Min'][k], bb['Max'][k] = new_lo[j], new_hi[j]
    # Check: the element's centre, carried through the new matrices, lands on the mirrored centre.
    B1 = from_json(mats[0].get('Data', mats[0]))
    raw_centre = apply(centre, Ei)
    got = apply(apply(raw_centre, B1), binds[names[0]])
    want = [-centre[0], centre[1], centre[2]]
    err = max(abs(got[i] - want[i]) for i in range(3))
    print('  %-34s centre (%+.3f %.3f %.3f) -> (%+.3f %.3f %.3f)  err %.1e  bone-mirror skew %.1e'
          % (label, centre[0], centre[1], centre[2], got[0], got[1], got[2], err, worst))
    if err > 1e-4 or worst > 1e-3:
        raise SystemExit('%s: the mirror does not hold' % label)


def mirrored_path(path):
    return re.sub(r'\.mesh$', SUFFIX + '.mesh', path)


def swap_meshes(node, used):
    """Every component (in the component list and in compiled data) of a mirrored element -> its mirror."""
    if isinstance(node, dict):
        name = node.get('name')
        mesh = node.get('mesh')
        if isinstance(name, dict) and isinstance(mesh, dict) and MIRRORED.match(str(name.get(V, ''))):
            path = mesh['DepotPath'][V]
            if not path.endswith(SUFFIX + '.mesh'):
                used.add(path)
                mesh['DepotPath'][V] = mirrored_path(path)
        for v in node.values():
            swap_meshes(v, used)
    elif isinstance(node, list):
        for v in node:
            swap_meshes(v, used)


def mesh_paths(node, out):
    if isinstance(node, dict):
        mesh = node.get('mesh')
        if isinstance(node.get('name'), dict) and isinstance(mesh, dict) and 'DepotPath' in mesh:
            out.add(mesh['DepotPath'][V])
        for v in node.values():
            mesh_paths(v, out)
    elif isinstance(node, list):
        for v in node:
            mesh_paths(v, out)
    return out


def swap_dependencies(doc, used):
    """The entity's preload list follows its components: the mirrors in, an original out once nothing uses it."""
    deps = doc['Data']['RootChunk'].get('resolvedDependencies', [])
    live = mesh_paths(doc['Data']['RootChunk'], set())
    have = {d['DepotPath'][V] for d in deps}
    kept = [d for d in deps if d['DepotPath'][V] not in used or d['DepotPath'][V] in live]
    for path in sorted(used):
        mirror = mirrored_path(path)
        if mirror not in have and deps:
            entry = copy.deepcopy(deps[0])
            entry['DepotPath'][V] = mirror
            kept.append(entry)
    deps[:] = kept


def mirror_app(doc, part):
    """The mirrored belt's own copy of an .app: every reference to the part (its values and its overrides)
    pointed at the mirrored part. A file of its own and not a second appearance in this one: an appearance's
    compiled data carries handles of its own, which a copy beside it does not get to share."""
    old, new = BELT + part + '.ent', BELT + part + SUFFIX + '.ent'

    def walk(n):
        if isinstance(n, dict):
            for k, v in n.items():
                if isinstance(v, str) and v == old:
                    n[k] = new
                else:
                    walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)

    walk(doc['Data']['RootChunk'])


def split_entities(doc):
    """vrp_belt.ent keeps its own four appearances; vrp_belt_lh.ent gets the same four, named
    vrp_belt_lh_&<body>&<camera>, on the mirrored .app files -- and nothing else, so whatever the item pipeline
    falls back to while the belt is attached, the garment merged onto the player is the mirrored one."""
    apps = doc['Data']['RootChunk']['appearances']
    apps[:] = [a for a in apps if not a['name'][V].startswith('vrp_belt_lh_')]
    lh = copy.deepcopy(doc)
    for a in lh['Data']['RootChunk']['appearances']:
        a['name'][V] = a['name'][V].replace('vrp_belt_&', 'vrp_belt_lh_&', 1)
        res = a['appearanceResource']['DepotPath']
        res[V] = re.sub(r'\.app$', SUFFIX + '.app', res[V])
    return lh


def add_factory_row(doc):
    """The item's entityName -> the mirrored root entity, beside the belt's own row."""
    rows = doc['Data']['RootChunk']['compiledData']
    rows[:] = [r for r in rows if r[0] != 'vrp_belt' + SUFFIX]
    base = next(r for r in rows if r[0] == 'vrp_belt')
    rows.append(['vrp_belt' + SUFFIX, re.sub(r'\.ent$', SUFFIX + '.ent', base[1])] + base[2:])


def main():
    pack = '--pack' in sys.argv[1:]
    src = WORK / 'src'
    binds = bind_transforms()
    belt_dir = PACK / 'base' / 'vrport' / 'belt'
    used = set()
    parts = {}
    for part in PARTS:
        doc = serialize(belt_dir / (part + '.ent'), src)
        swapped = set()
        swap_meshes(doc, swapped)
        swap_dependencies(doc, swapped)
        used |= swapped
        parts[part] = doc
    print('mirroring %d meshes' % len(used))
    for path in sorted(used):
        rel = Path(*path.split(BS))
        out = Path(mirrored_path(str(rel)))
        if (PACK / out).exists() and (PACK / out).stat().st_mtime > (PACK / rel).stat().st_mtime:
            print('  %-34s kept (newer than its source)' % rel.stem)
            continue
        doc = serialize(PACK / rel, src / 'meshes')
        mirror_mesh(doc, binds, rel.stem)
        deserialize(doc, out.name, PACK / out.parent)
    for part, doc in parts.items():
        deserialize(doc, part + SUFFIX + '.ent', belt_dir)
    for app, part in zip(APPS, PARTS):
        doc = serialize(belt_dir / (app + '.app'), src)
        mirror_app(doc, part)
        deserialize(doc, app + SUFFIX + '.app', belt_dir)
    doc = serialize(belt_dir / 'vrp_belt.ent', src)
    lh = split_entities(doc)
    deserialize(doc, 'vrp_belt.ent', belt_dir)
    deserialize(lh, 'vrp_belt' + SUFFIX + '.ent', belt_dir)
    factory = PACK / 'base' / 'vrport' / 'vrp_props.csv'
    doc = serialize(factory, src)
    add_factory_row(doc)
    deserialize(doc, factory.name, factory.parent)
    print('Items.vrp_belt_lh -> vrp_belt_lh -> vrp_belt_lh.ent: vrp_belt_lh_&{Male,Female}&{TPP,FPP} -> '
          'vrp_belt_{m,w}_lh.app -> vrp_belt_part_{m,w}_lh.ent')
    if pack:
        MS.pack_archive()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
