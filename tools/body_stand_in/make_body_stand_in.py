"""The body the first person draws when the game takes its own away.

    py -3.13 -B tools/body_stand_in/make_body_stand_in.py           all steps
    py -3.13 -B tools/body_stand_in/make_body_stand_in.py mesh      the body mesh without its neck stump (stump_cut.exe)
    py -3.13 -B tools/body_stand_in/make_body_stand_in.py entity    the vrp_body_stand_in component in the player entities
    py -3.13 -B tools/body_stand_in/make_body_stand_in.py pack      the mesh and the entities into the pack, the archive packed

WHY. The first-person body is the full body mesh, t0_000_p?a_base__full, put on by the character customization as one
of its parts. When V wears nothing on the legs but underwear, the game takes that part off and puts nothing in its
place: measured in the unmodded game (CET only, the port's plugin moved out) on a woman's save stripped through the
equipment system -- t0_000_pwa_base__full gone, its shadow and seam meshes, the neck and the feet still there, and it
comes back the moment something covers the legs again. The flat camera never looks down far enough to notice; in VR
the player stands on a pair of floating feet. The same happens on saves whose wardrobe set hides the legs. A man's
body stays (measured the same way: t0_000_pma_base__full still there in boxers), so only the woman's entities get it.

THE FIX is a component of the port's own, on the player entities: a garment skinned mesh of the body, bound to the
skeleton the way the wrist screen (vrp_latd_screen) is, off by default. The plugin's Lua turns it on, in the body's
skin tone, while the game's body part is missing, and off when it is back (mods/cet/CyberpunkVRPort_ForceFPP). As a
garment it is assembled with the clothes the way the game's body is, so what the clothes cover is hidden; it draws no
shadow of its own (the body's shadow mesh stays on the player all the time).

THE MESH is a copy of the game's body mesh (vrport\\body\\vrp_body_stand_in_pwa.mesh) without the triangles of its neck
stump. The game's own first-person neck cone removes those from the body by the garment assembly; the stand-in is a
garment itself, so nothing removed them, and the stump stood over the port's neck cap -- a dark disc in its middle
(tools/neck_cap). stump_cut.exe (build.bat) picks the triangles that stand above the cap; they are made degenerate in
the exported glb, which is imported onto the copy. Hiding the stump's chunk instead (chunk 1) took the collarbones too.
The game's meshes and parts stay as they are.
"""
import copy
import json
import re
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/neck_cap'))
import make_neck_cap as N  # noqa: E402 -- the glb readers, the converter and the neck caps' own paths

RAW = ROOT / 'mods/archive/source/raw'
STAGE = ROOT / 'build/body-stand-in/entities'
MESH_WORK = ROOT / 'build/body-stand-in/import'
BS = '\\'
# Per body: the game's body mesh, the port's copy without the stump, and the neck cap it makes room for.
BODIES = {
    'pwa': (BS.join(['base', 'characters', 'common', 'player_base_bodies', 'player_female_average',
                     't0_000_pwa_base__full.mesh']),
            BS.join(['vrport', 'body', 'vrp_body_stand_in_pwa.mesh']),
            BS.join(['vrport', 'body', 'vrp_neck_cap_pwa.mesh'])),
}
# The woman's player entities the port ships (the ones carrying vrp_latd_screen), and the body each is.
ENTITY_FILES = {
    'base/characters/entities/player/player_wa_fpp.ent': 'pwa',
    'ep1/characters/entities/player/player_wa_fpp_ep1.ent': 'pwa',
}
COMPONENT = 'vrp_body_stand_in'
TEMPLATE = 'vrp_latd_screen'    # a plain skinned mesh component of the port's, bound to the skeleton's root
TONE = '02_ca_limestone'        # any of the body mesh's appearances; the Lua sets the arms' one before showing it
MARGIN = 0.003                  # metres a body vertex stands above the cap to count as hiding it


def glb_triangles(data, doc, base):
    """Every primitive's triangles as global vertex indices, and for each primitive (index accessor offset, stride,
    format, first global vertex, triangle count)."""
    tris, prims, first = [], [], 0
    for mesh in doc['meshes']:
        for prim in mesh['primitives']:
            count, _, fmt, stride, offset = N.view(doc, base, prim['indices'])
            idx = [struct.unpack_from('<' + fmt, data, offset + k * stride)[0] for k in range(count)]
            tris.extend(first + i for i in idx)
            prims.append((offset, stride, fmt, first, count // 3))
            first += doc['accessors'][prim['attributes']['POSITION']]['count']
    return tris, prims


def cmd_mesh():
    tool = Path(__file__).with_name('stump_cut.exe')
    if not tool.exists():
        raise SystemExit('build tools/body_stand_in/stump_cut.exe first (build.bat)')
    for body, (game, ours, cap_mesh) in BODIES.items():
        src_glb = N.SOURCE / N.posix(game).replace('.mesh', '.glb')
        src_mesh = N.SOURCE / N.posix(game)
        if not src_glb.exists() or not src_mesh.exists():
            pattern = '^%s$' % re.escape(game)
            N.run(N.CLI, 'unbundle', N.GAME / 'archive/pc/content' / N.ARCHIVE, '-o', N.SOURCE, '-r', pattern)
            N.run(N.CLI, 'uncook', N.GAME / 'archive/pc/content' / N.ARCHIVE, '-o', N.SOURCE, '-gp', N.GAME, '-r', pattern)
        out_mesh = MESH_WORK / N.posix(ours)
        out_glb = out_mesh.with_suffix('.glb')
        out_mesh.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src_mesh, out_mesh)
        data, doc, base = N.read_glb(src_glb)
        verts, _ = N.glb_vertices(data, doc, base, False)
        tris, prims = glb_triangles(data, doc, base)
        cdata, cdoc, cbase = N.read_glb(N.IMPORT / N.posix(cap_mesh).replace('.mesh', '.glb'))
        cap, _ = N.glb_vertices(cdata, cdoc, cbase, False)
        cap_tris, _ = glb_triangles(cdata, cdoc, cbase)
        work = ROOT / 'build/body-stand-in/cut'
        work.mkdir(parents=True, exist_ok=True)
        pack_mesh = lambda v, t: (struct.pack('<I', len(v)) + b''.join(struct.pack('<3f', *p) for p in v) +
                                  struct.pack('<I%dI' % len(t), len(t) // 3, *t))
        (work / 'cap.bin').write_bytes(pack_mesh(cap, cap_tris))
        (work / 'body.bin').write_bytes(pack_mesh(verts, tris))
        r = subprocess.run([str(tool), str(work / 'cap.bin'), str(work / 'body.bin'), str(work / 'cut.bin'), str(MARGIN)],
                           capture_output=True, text=True)
        if r.returncode:
            raise SystemExit('stump_cut: %s' % (r.stdout + r.stderr))
        cut = (work / 'cut.bin').read_bytes()
        # The cut triangles made degenerate (all three corners the first one) in place: the buffers keep their sizes.
        k = 0
        for offset, stride, fmt, first, count in prims:
            for t in range(count):
                if cut[k + t]:
                    a = struct.unpack_from('<' + fmt, data, offset + 3 * t * stride)[0]
                    for c in (1, 2):
                        struct.pack_into('<' + fmt, data, offset + (3 * t + c) * stride, a)
            k += count
        out_glb.write_bytes(bytes(data))
        print('mesh: %s -- %s' % (ours, r.stderr.strip()))
    # Onto the copy (-k: the CR2W kept, its geometry replaced). The importer can exit non-zero over its
    # garment-support warning even when the file went in: its own count says whether it worked.
    r = subprocess.run([str(N.CLI), 'import', str(MESH_WORK), '-o', str(MESH_WORK), '-k'], capture_output=True, text=True)
    done = re.search(r'Imported (\d+)/(\d+)', r.stdout + r.stderr)
    if not done or done.group(1) != done.group(2) or done.group(1) != str(len(BODIES)):
        print((r.stdout + r.stderr)[-3000:])
        raise SystemExit('stand-in import failed')
    print('mesh import:', done.group(0))


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
        part = copy.deepcopy(template)
        # The template's bindings are handle references to the skeleton root's transform and skinning, shared.
        if 'HandleRefId' not in part['parentTransform'] or 'HandleRefId' not in part['skinning']:
            raise SystemExit('%s: %s does not reference shared bindings' % (entity, TEMPLATE))
        # A GARMENT component, as the game's own body parts are (the feet, l0_000_pwa_base__cs_flat: the same fields as
        # the wrist screen's plain skinned mesh, another type). Only a garment takes part in the garment assembly,
        # which hides the body under the clothes: a plain mesh of the body showed through a racing suit.
        part['$type'] = 'entGarmentSkinnedMeshComponent'
        part['name']['$value'] = COMPONENT
        part['id'] = component_id(entity)
        part['mesh']['DepotPath']['$value'] = BODIES[body][1]
        part['meshAppearance']['$value'] = TONE
        part['renderingPlaneAnimationParam']['$value'] = 'None'
        part['visibilityAnimationParam']['$value'] = 'None'
        part['castShadows'] = part['castLocalShadows'] = 'Never'
        part['chunkMask'] = '18446744073709551615'
        part['isEnabled'] = 0
        components.append(part)
        # Written the way WolvenKit writes it (a load and dump reproduces these files byte for byte), keeping the
        # file's line endings and its final newline.
        newline = '\r\n' if '\r\n' in text else '\n'
        out = json.dumps(doc, indent=2, ensure_ascii=False).replace('\n', newline)
        if text.endswith('\n'):
            out += newline
        path.write_bytes(out.encode('utf-8'))
        print('entity: %s + %s (%s)' % (entity, COMPONENT, BODIES[body][1]))


def cmd_pack():
    sys.path.insert(0, str(ROOT / 'tools/vr_lobby_assets'))
    import make_lobby_scene  # noqa: E402 -- the port's one sources -> pack -> archive step
    for _, ours, _ in BODIES.values():
        dst = N.PACK / N.posix(ours)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(MESH_WORK / N.posix(ours), dst)
        print('pack:', N.posix(ours))
    make_lobby_scene.pack_sources(list(ENTITY_FILES), STAGE)
    make_lobby_scene.pack_archive()


if __name__ == '__main__':
    for step in sys.argv[1:] or ['mesh', 'entity', 'pack']:
        {'mesh': cmd_mesh, 'entity': cmd_entity, 'pack': cmd_pack}[step]()
