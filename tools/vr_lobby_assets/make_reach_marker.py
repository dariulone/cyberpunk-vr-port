"""A hand's reach marker as geometry in the world (mods/cet/CyberpunkVRPort_Reach modules/marker.lua): Into the
Radius 2's selector -- a thin ring open on one side, and four corner brackets once the trigger has locked -- drawn
the way ITR2 draws it, as a sprite in the world and not as a widget.

    py -3.13 -B tools/vr_lobby_assets/make_reach_marker.py            all of it
    py -3.13 -B tools/vr_lobby_assets/make_reach_marker.py mesh       the mesh and its materials
    py -3.13 -B tools/vr_lobby_assets/make_reach_marker.py entity     the entity
    py -3.13 -B tools/vr_lobby_assets/make_reach_marker.py pack       deserialize into the pack, pack the archive

  vrport\\ui\\vr_marker.mesh    the ring and the brackets, each a line and the soft stroke under it, flat in the mesh's
                               X-Z plane and facing -Y (built here with gltf.Builder, imported by WolvenKit onto a copy
                               of the lobby's plane mesh). The brackets four times, at BRACKET_SCALES: the snap as the
                               trigger locks is a step through them. Appearances l0 .. lLEVELS, every chunk on its
                               colour at that level, l0 nothing at all
  vrport\\ui\\marker\\*.mi      the game's world-marker material (hud_markers_notxaa), additive: what it does not draw
                               is not there; its colour the part's, its level the colour's multiplier
  vrport\\ui\\vr_marker.ent     one mesh component a part -- "ring", "brackets1" .. -- each showing only its own chunks
                               (chunkMask), so a part comes and goes by its appearance alone: a chunk mask changed at
                               run time is not read again until the component is switched off and on

THE SIZE IS ITR2's, read from its assets: its selectors (PS_Selector, PS_SelectorTrigger) are 2.5 cm sprites
(ParticleModuleSize, a constant -- whatever the thing and however far), and their textures fill them: the ring's stroke
centred at 0.917 of the half and 0.137 of it thick, open 70 deg; the brackets a square 0.955 of the sprite, each arm a
third of a side, 0.066 of the sprite thick.

WHY NOT A WORLD WIDGET, as it was: a world widget's texture is the engine's world-widget manager's, which lets go of it
while the plane is out of view and binds it again only once the plane is seen -- and the plane drew its material's
defaults, an opaque black square, in the frames between: with every first appearance, and each time the marker came
back into view. Two world widgets made in the main menu also took the confirm of the game's own popup.
"""
import copy
import json
import math
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parents[1] / 'tools/ent')]
import gltf  # noqa: E402
import make_construct as C  # noqa: E402
import make_lobby_scene as S  # noqa: E402
import add_hold_components as H  # noqa: E402
from make_lobby_scene import cname, depot  # noqa: E402

ROOT = HERE.parents[1]
RAW = ROOT / 'mods/archive/source/raw/vrport/ui'
MATERIALS = RAW / 'marker'
WORK = ROOT / 'build/reach-marker/work'
NAME = 'vr_marker'
DEPOT = 'vrport\\ui\\'

SPRITE = 0.025                       # ITR2's selector, across (m)
RING = SPRITE / 2 * 0.917            # the ring's radius: its stroke's centre
LINE = 0.149                         # the ring's stroke, of its radius
GAP = math.radians(70)               # the ring's opening, whole, toward +X
BOX = SPRITE / 2 * 0.955             # the brackets' half side
ARM = SPRITE * 0.318                 # a bracket's arm
BLINE = SPRITE * 0.066               # the brackets' stroke
SOFT = 2.6                           # the soft stroke under each line, of the line
SEGMENTS = 64                        # round the whole circle
BRACKET_SCALES = (1.35, 1.24, 1.12, 1.0)
LEVELS = 8
AUTO_HIDE = 100.0                    # m: a component's own hide distance (0, the default, is the mesh's size's)

# Linear colours, and how strong a line and its soft stroke are of them (the marker as it was drawn in ink).
WHITE, CYAN, AMBER = (0.94, 0.98, 1.0), (0.43, 0.86, 1.0), (1.0, 0.69, 0.16)
STRENGTH = {'line': 0.95, 'soft': 0.16}
# What each part's line and soft stroke are drawn in.
COLOURS = {'ring': {'line': ('white', WHITE), 'soft': ('cyan', CYAN)},
           'brackets': {'line': ('amber', AMBER), 'soft': ('amber', AMBER)}}
# The game's own material for its markers in the world: unlit, drawn after the frame's temporal antialiasing
# (renderstage_screenspace_vfx -- no smear behind a marker that moves with the hand), depth tested, writing no depth,
# premultiplied; additive at AdditiveAlphaBlend 1, so what it does not draw is not there and a part's `off` (no colour)
# is nothing at all. The shape is the geometry's: the texture is plain white. The hologram shader drew it a dim grey
# broken by its scanline cut (its colour is the fresnel rim's, and a plane turned to the eyes has none), the beam's
# screen material blends by its texture's alpha and drew an opaque black bar for `off`.
MARKER_MATERIAL = 'base\\fx\\shaders\\hud_markers_notxaa.mt'
WHITE_TEXTURE = 'engine\\textures\\editor\\white.xbm'
GAIN = 3.0                           # ColorMultiplier of a full line: bright enough to read over a lit wall
LOOK = [S.value('tex', 'Diffuse', WHITE_TEXTURE),
        S.value('float', 'AdditiveAlphaBlend', 1.0),
        S.value('float', 'UseOnMeshes', 1.0),
        S.value('float', 'TillingX', 1.0),
        S.value('float', 'TillingY', 1.0)]


def parts():
    """{part: [(chunk group, kind, colour)]}: the ring, then the brackets at each scale, in chunk order."""
    out = {'ring': [('p0_line', 'line'), ('p0_soft', 'soft')]}
    for k in range(len(BRACKET_SCALES)):
        out['brackets%d' % (k + 1)] = [('p%d_line' % (k + 1), 'line'), ('p%d_soft' % (k + 1), 'soft')]
    return out


def chunk_order():
    # gltf.Builder writes its groups sorted by name, and WolvenKit keeps that order for the chunks.
    return sorted(g for ps in parts().values() for g, _ in ps)


def material_name(part, kind, level):
    if level <= 0:
        return 'off'
    colour = COLOURS['ring' if part == 'ring' else 'brackets'][kind][0]
    return '%s_%s_%d' % (colour, kind, level)


# ---------------------------------------------------------------- the geometry

def point(x, z):
    return (x, 0.0, z)


def bar(b, group, x1, z1, x2, z2, width):
    """A bar from (x1, z1) to (x2, z2) in the X-Z plane, `width` thick, facing -Y."""
    dx, dz = x2 - x1, z2 - z1
    length = math.hypot(dx, dz)
    nx, nz = -dz / length * width / 2, dx / length * width / 2
    b.quad(group, point(x1 - nx, z1 - nz), point(x2 - nx, z2 - nz), point(x2 + nx, z2 + nz), point(x1 + nx, z1 + nz))


def ring(b, group, radius, width):
    """The ring open toward +X, its stroke round the radius."""
    arc = 2 * math.pi - GAP
    n = max(1, round(SEGMENTS * arc / (2 * math.pi)))
    r0, r1 = radius - width / 2, radius + width / 2
    for i in range(n):
        t0, t1 = GAP / 2 + arc * i / n, GAP / 2 + arc * (i + 1) / n
        b.quad(group, point(r0 * math.cos(t0), r0 * math.sin(t0)), point(r1 * math.cos(t0), r1 * math.sin(t0)),
               point(r1 * math.cos(t1), r1 * math.sin(t1)), point(r0 * math.cos(t1), r0 * math.sin(t1)))


def brackets(b, group, half, arm, width):
    """Four corner brackets round the square of half side `half`; an arm starts half a stroke out, so the corner is
    filled."""
    for cx, cz in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
        x, z = cx * half, cz * half
        bar(b, group, x + cx * width / 2, z, x - cx * arm, z, width)
        bar(b, group, x, z + cz * width / 2, x, z - cz * arm, width)


def geometry(path):
    b = gltf.Builder()
    ring(b, 'p0_line', RING, RING * LINE)
    ring(b, 'p0_soft', RING, RING * LINE * SOFT)
    for k, s in enumerate(BRACKET_SCALES):
        brackets(b, 'p%d_line' % (k + 1), BOX * s, ARM * s, BLINE * s)
        brackets(b, 'p%d_soft' % (k + 1), BOX * s, ARM * s, BLINE * s * SOFT)
    return b.write(str(path), NAME)


# ---------------------------------------------------------------- the materials

def srgb8(v):
    v = max(0.0, min(1.0, v))
    return round(255 * (12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055))


def look(colour, strength):
    return LOOK + [S.value('color', 'Color', colour), S.value('color', 'Second_Color', colour),
                   S.value('float', 'ColorMultiplier', round(strength, 4))]


def materials():
    """{name: values}: every part's colours at every level, and `off`."""
    out = {'off': look((0, 0, 0), 0.0)}
    for part in ('ring', 'brackets'):
        for kind in ('line', 'soft'):
            colour = tuple(srgb8(c) for c in COLOURS[part][kind][1])
            for level in range(1, LEVELS + 1):
                out[material_name(part, kind, level)] = look(colour, GAIN * STRENGTH[kind] * level / LEVELS)
    return out


def cmd_mesh():
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    tris = geometry(WORK / (NAME + '.glb'))
    shutil.copyfile(S.TEMPLATE_MESH, WORK / (NAME + '.mesh'))
    # The importer exits non-zero over its garment-support warning even when the file went in: its own count is
    # what says whether the import worked (make_lobby_scene.cmd_meshes).
    r = subprocess.run([str(S.CLI), 'import', str(WORK), '-o', str(WORK), '-k'], capture_output=True, text=True)
    if 'Imported 1/1 file(s)' not in r.stdout:
        print(r.stdout[-3000:], r.stderr[-2000:])
        raise SystemExit('mesh import failed')
    S.run(S.CLI, 'convert', 'serialize', WORK, '-o', WORK, '-w', '*.mesh')
    doc = json.loads((WORK / (NAME + '.mesh.json')).read_text(encoding='utf-8-sig'))
    doc['Header']['ArchiveFileName'] = ''
    root = doc['Data']['RootChunk']
    order = chunk_order()
    infos = root['renderResourceBlob']['Data']['header']['renderChunkInfos']
    if len(infos) != len(order):
        raise SystemExit('%d chunks, expected %d (%s)' % (len(infos), len(order), order))
    kind_of = {g: (part, kind) for part, ps in parts().items() for g, kind in ps}
    mats = materials()
    names = list(mats)
    root['appearances'] = []
    base = H.max_handle(doc) + 1
    root['appearances'] = [{'HandleId': str(base + level), 'Data': {
        '$type': 'meshMeshAppearance', 'name': cname('l%d' % level), 'tags': [],
        'chunkMaterials': [cname(material_name(*kind_of[g], level)) for g in order]}}
        for level in range(0, LEVELS + 1)]
    root['materialEntries'] = [{'$type': 'CMeshMaterialEntry', 'index': i, 'isLocalInstance': 0, 'name': cname(n)}
                               for i, n in enumerate(names)]
    root['externalMaterials'] = [{'DepotPath': depot(DEPOT + 'marker\\' + n + '.mi'), 'Flags': 'Default'}
                                 for n in names]
    root['preloadExternalMaterials'] = []
    root['preloadLocalMaterialInstances'] = []
    root['localMaterialInstances'] = []
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer', 'materials': [], 'rawData': None,
                                   'rawDataHeaders': []}
    # A level is a step of a fade: every appearance's materials loaded with the mesh, none fetched mid-fade.
    root['forceLoadAllAppearances'] = 1
    (RAW / (NAME + '.mesh.json')).write_text(json.dumps(doc, indent=1) + '\n', encoding='utf-8', newline='\n')
    if MATERIALS.exists():
        shutil.rmtree(MATERIALS)
    MATERIALS.mkdir(parents=True)
    for n, values in mats.items():
        (MATERIALS / (n + '.mi.json')).write_text(json.dumps(C.material_doc(values, MARKER_MATERIAL), indent=1) + '\n',
                                                  encoding='utf-8', newline='\n')
    print('marker mesh: %d chunks %s, %d materials' % (len(order), tris, len(mats)))


# ---------------------------------------------------------------- the entity

def cmd_entity():
    """The marker's entity on its old one (the VR menu beam's): its mesh component once a part, nothing else."""
    path = RAW / (NAME + '.ent.json')
    e = json.loads(path.read_text(encoding='utf-8-sig'))
    root = e['Data']['RootChunk']
    template = next(c for c in root['components'] if c['$type'] == 'entMeshComponent')
    order = chunk_order()
    first_id = int(template['id'])
    comps = []
    for k, (part, ps) in enumerate(parts().items()):
        c = copy.deepcopy(template)
        c['name'] = cname(part)
        c['id'] = str(first_id + k)
        c['mesh']['DepotPath'] = depot(DEPOT + NAME + '.mesh')
        c['meshAppearance'] = cname('l0')
        c['visualScale'] = {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1}
        c['isEnabled'] = 1
        c['autoHideDistance'] = AUTO_HIDE
        c['chunkMask'] = str(sum(1 << order.index(g) for g, _ in ps))
        comps.append(c)
    root['components'] = comps
    path.write_text(json.dumps(e, indent=1) + '\n', encoding='utf-8', newline='\n')
    print('marker entity:', ', '.join('%s %s' % (c['name']['$value'], c['chunkMask']) for c in comps))


# ---------------------------------------------------------------- the pack

def cmd_pack():
    target = S.PACK / 'vrport/ui'
    stale = target / (NAME + '.inkwidget')
    if stale.exists():
        stale.unlink()
    if (target / 'marker').exists():
        shutil.rmtree(target / 'marker')
    (target / 'marker').mkdir(parents=True)
    S.run(S.CLI, 'convert', 'deserialize', RAW / (NAME + '.mesh.json'), '-o', target)
    S.run(S.CLI, 'convert', 'deserialize', RAW / (NAME + '.ent.json'), '-o', target)
    S.run(S.CLI, 'convert', 'deserialize', MATERIALS, '-o', target / 'marker')
    print('marker:', len(list((target / 'marker').glob('*.mi'))), 'materials in the pack')
    S.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['mesh', 'entity', 'pack']
    for step in steps:
        {'mesh': cmd_mesh, 'entity': cmd_entity, 'pack': cmd_pack}[step]()
