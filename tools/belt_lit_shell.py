# -*- coding: utf-8 -*-
"""The belt's mark inside each element's own mesh: the `_lit` twin components are taken out.

WHY. The game's garment pass (DoStartChangeAppearance_ComputePlayerGarment -> SmoothVertexNormals) numbers the
player's garment elements in ONE BYTE: a vertex is referred to as element | chunk << 8 | vertex << 16, and the
pass keeps a 256-entry table on top of that. A change to an item counts the set the player wears AND the item
that changes, so an item of N garments costs 2N in that pass. The port's belt part had 123 -- 21 for the belt,
51 elements (39 magazines, 9 grenades, 3 shells) and a `_lit` twin for each -- and 25 + 123 + 123 = 271 crashed
the game whenever the belt came off, went on or changed appearance in the world (a left-handed player's belt
swapping to its mirror, 2026-11-01: element 270 read past the table at sub_14026E178).

WHAT THE TWIN DID AND WHAT REPLACES IT. A twin drew the element's mesh a second time in a mark material, over the
element, so the object stayed and the mark was added; swapping the element's own appearance to `lit` replaced
the object's materials and left only the mark. Here each element's mesh gets the twin's job itself:

  * every opaque chunk gets a SHELL chunk -- a second chunk info over the SAME vertex and index ranges, so no
    geometry is copied; decal chunks get none (a decal drawn twice is a darker decal);
  * in every appearance the mesh had, a shell wears its chunk's own material: the same surface drawn twice in the
    same place, which shows nothing;
  * each such appearance A gets `A_lit`: its own materials on its chunks and the amber mark (vrp_lit5,
    base\\vrport\\vrp_mark_amber.mi) on the shells -- the object plus the mark, as the twin drew it;
  * the old `lit` .. `lit8` appearances (the mark alone, in eight colours) are dropped.

The belt module marks an element by swapping it to `<its appearance>_lit` and back (TacticalBelt init.lua,
litAppearanceFor), and the parts lose their 51 twins: 72 garments a belt, 25 + 72 + 72 = 169 in the pass.

Run on the pack's cooked files (make_lobby_scene.PACK), idempotent: a mesh that already has `_lit` appearances
and a part without twins are left alone. The originals are kept under the stage folder. Packs the archive.

    py -3.13 tools/belt_lit_shell.py [--no-pack]
"""
import copy
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'vr_lobby_assets'))
import make_lobby_scene as MS  # noqa: E402

V = '$value'
PACK = MS.PACK
CLI = MS.CLI
STAGE = MS.ROOT / 'build' / 'belt-lit-shell-20261101'
PARTS = ['vrp_belt_part_m', 'vrp_belt_part_w', 'vrp_belt_part_m_lh', 'vrp_belt_part_w_lh']
MARK = 'vrp_lit5'          # amber
TWIN = '_lit'


def run(*args):
    subprocess.run([str(a) for a in args], check=True, capture_output=True)


def cname(value):
    return {'$type': 'CName', '$storage': 'string', V: value}


def name_of(node):
    n = node.get('name') if isinstance(node, dict) else None
    return n.get(V) if isinstance(n, dict) else n


def handles(doc):
    """Every HandleId defined and every HandleRefId used, anywhere in the file."""
    defined, used = set(), set()

    def walk(n):
        if isinstance(n, dict):
            if 'HandleId' in n:
                defined.add(int(n['HandleId']))
            if 'HandleRefId' in n:
                used.add(int(n['HandleRefId']))
            for v in n.values():
                walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)
    walk(doc)
    return defined, used


def is_decal(material):
    m = material.lower()
    return 'decal' in m or m.startswith('dec_') or m == 'normal'


def shell_mesh(doc):
    """The shells and the `_lit` appearances on one mesh; False when it already has them."""
    root = doc['Data']['RootChunk']
    header = root['renderResourceBlob']['Data']['header']
    chunks = header['renderChunkInfos']
    topology = header['topology']
    apps = root['appearances']
    names = [name_of(a['Data']) for a in apps]
    if any(n.endswith(TWIN) for n in names):
        return False
    base = [a for a in apps if not name_of(a['Data']).startswith('lit')]
    if not base:
        raise SystemExit('no appearance to mark')
    first = [c[V] for c in base[0]['Data']['chunkMaterials']]
    if len(first) != len(chunks) or len(topology) != len(chunks):
        raise SystemExit('%d chunks, %d materials, %d topology entries' % (len(chunks), len(first), len(topology)))
    shell = [i for i in range(len(chunks)) if not is_decal(first[i])]
    if not shell:
        raise SystemExit('no opaque chunk to shell')
    for i in shell:
        chunks.append(copy.deepcopy(chunks[i]))
        topology.append(copy.deepcopy(topology[i]))
    nxt = max(handles(doc)[0]) + 1
    out = []
    for a in base:
        mats = a['Data']['chunkMaterials']
        own = [m[V] for m in mats]
        mats.extend(cname(own[i]) for i in shell)
        lit = copy.deepcopy(a)
        lit['HandleId'] = str(nxt)
        nxt += 1
        lit['Data']['name'] = cname(name_of(a['Data']) + TWIN)
        lit['Data']['chunkMaterials'] = [cname(m) for m in own] + [cname(MARK) for _ in shell]
        out += [a, lit]
    root['appearances'] = out
    return True


def strip_twins(doc):
    """A part's `_lit` twins out of both component lists, the compiled list's CRUIDs re-keyed; the number taken."""
    root = doc['Data']['RootChunk']
    comps = root['components']
    data = root['compiledData']['Data']
    chunks, cruids = data['Chunks'], data['CruidDict']
    keep_c = [c for c in comps if not (name_of(c) or '').endswith(TWIN)]
    gone = len(comps) - len(keep_c)
    if gone == 0:
        return 0
    kept, dictionary = [], {}
    for i, c in enumerate(chunks):
        if (name_of(c) or '').endswith(TWIN):
            continue
        dictionary[str(len(kept))] = cruids[str(i)]
        kept.append(c)
    if len(chunks) - len(kept) != gone:
        raise SystemExit('the two lists disagree: %d components, %d compiled' % (gone, len(chunks) - len(kept)))
    root['components'] = keep_c
    data['Chunks'] = kept
    data['CruidDict'] = dictionary
    defined, used = handles(doc)
    if used - defined:
        raise SystemExit('dangling HandleRefId %s' % sorted(used - defined)[:8])
    return gone


def serialized(files, folder):
    """Cooked files -> their JSON, in a folder of their own (the converter writes everything it reads)."""
    src, out = STAGE / ('in_' + folder), STAGE / ('json_' + folder)
    for d in (src, out):
        if d.exists():
            shutil.rmtree(d)
        d.mkdir(parents=True)
    for f in files:
        shutil.copyfile(f, src / f.name)
    run(CLI, 'convert', 'serialize', src, '-o', out)
    return out


def main():
    pack = '--no-pack' not in sys.argv
    belt = PACK / 'base' / 'vrport' / 'belt'
    meshes_dir = PACK / 'base' / 'vrport' / 'meshes'
    for sub in ('json', 'out'):
        if (STAGE / sub).exists():
            shutil.rmtree(STAGE / sub)
        (STAGE / sub).mkdir(parents=True)
    (STAGE / 'original').mkdir(parents=True, exist_ok=True)
    edited = []

    # The parts, and from their twins the meshes the mark has to move into.
    parts = serialized([belt / (p + '.ent') for p in PARTS], 'parts')
    meshes = set()
    for p in PARTS:
        doc = json.loads((parts / (p + '.ent.json')).read_text(encoding='utf-8'))
        for c in doc['Data']['RootChunk']['components']:
            if (name_of(c) or '').endswith(TWIN):
                meshes.add(c['mesh']['DepotPath'][V].split('\\')[-1])
        gone = strip_twins(doc)
        print('%-22s %d twins out, %d components' % (p, gone, len(doc['Data']['RootChunk']['components'])))
        if gone:
            (STAGE / 'json' / (p + '.ent.json')).write_text(json.dumps(doc, indent=2), encoding='utf-8')
            edited.append(('belt', p + '.ent'))
    if not meshes:
        # A second run: the twins are gone, so the meshes come from the parts' elements instead.
        meshes = {m.name for m in meshes_dir.glob('vrp_belt_*.mesh')
                  if m.name.startswith(('vrp_belt_mag_', 'vrp_belt_gren_', 'vrp_belt_ammo_shell'))}

    mesh_json = serialized([meshes_dir / m for m in sorted(meshes)], 'meshes')
    shelled = 0
    for m in sorted(meshes):
        doc = json.loads((mesh_json / (m + '.json')).read_text(encoding='utf-8'))
        if shell_mesh(doc):
            (STAGE / 'json' / (m + '.json')).write_text(json.dumps(doc, indent=2), encoding='utf-8')
            edited.append(('meshes', m))
            shelled += 1
    print('%d meshes shelled of %d' % (shelled, len(meshes)))
    if not edited:
        print('nothing to do')
        return 0

    run(CLI, 'convert', 'deserialize', STAGE / 'json', '-o', STAGE / 'out')
    for where, name in edited:
        target = (belt if where == 'belt' else meshes_dir) / name
        original = STAGE / 'original' / name
        if not original.exists():
            shutil.copyfile(target, original)
        cooked = STAGE / 'out' / name
        if not cooked.exists():
            raise SystemExit('not deserialized: %s' % cooked)
        shutil.copyfile(cooked, target)
    print('%d files into the pack (originals in %s)' % (len(edited), STAGE / 'original'))
    if pack:
        MS.pack_archive()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
