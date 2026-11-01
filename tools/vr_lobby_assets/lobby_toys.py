"""The toys on the lobby's table (lobby_blender.py table()): things to pick up like the rest, made to be looked at.

Each is a Meshy model made a toy by tools/lobby_toys (make_toy.py: stood up at its height, 20 k triangles, a normal
map baked from the full model; toy_maps.py: its own colour, roughness and metal maps). build/lobby-toys/<folder>/out
holds NAME_low.glb and NAME_d/n/r/m.png. Its material is an `own` one: the port's textures (make_lobby_scene.py
imports them), not the game's.
  chibi   the Maelstrom chibi, 25 cm
  skippy  Skippy, the talking smart gun's bullet avatar (quest mq007; the game has it only as video), 15 cm

A toy stands on its own origin, its face towards -Y (the player's side of the table); table() places it.
"""
from pathlib import Path

import bpy
from mathutils import Matrix

ROOT = Path(__file__).resolve().parents[2]
TOYS = {'chibi': ROOT / 'build/lobby-toys/maelstrom/out', 'skippy': ROOT / 'build/lobby-toys/skippy/out'}
# Where a hand holds each (modules/grips.lua): the part's grasp shape, half sizes and middle, in the toy's own frame
# (feet on the origin). The chibi by its lower body, standing on the palm the way a book stands on its edge: a plate
# 11 x 4.8 x 12.5 cm, its depth the thin side (fingers up its back, thumb on its chest). Skippy round its case, as a
# can is held: the case up to the nose, so the hand finds it wherever it takes it.
GRASPS = {'chibi': ('box', (0.055, 0.024, 0.0625), (0.0, -0.005, 0.0625)),
          'skippy': ('cylinder', (0.032, 0.032, 0.06), (-0.01, 0.0, 0.06))}


def materials(M, image, principled):
    """Each toy's material into M, named after it."""
    for name, folder in TOYS.items():
        M[name] = own_material(name, folder, image, principled)


def own_material(name, folder, image, principled):
    """The port's own textures on metal_base: NAME_d (colour), _n (tangent normal), _r (roughness), _m (metal), all
    in `folder`. The export carries them as m['own'] (make_lobby_scene.py imports and references them)."""
    maps = {k: folder / ('%s_%s.png' % (name, k)) for k in 'dnrm'}
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    n, l = m.node_tree.nodes, m.node_tree.links
    p = principled(m)
    d = n.new('ShaderNodeTexImage'); d.image = image(maps['d'])
    l.new(d.outputs['Color'], p.inputs['Base Color'])
    r = n.new('ShaderNodeTexImage'); r.image = image(maps['r'], True)
    l.new(r.outputs['Color'], p.inputs['Roughness'])
    mt = n.new('ShaderNodeTexImage'); mt.image = image(maps['m'], True)
    l.new(mt.outputs['Color'], p.inputs['Metallic'])
    nt = n.new('ShaderNodeTexImage'); nt.image = image(maps['n'], True)
    nm = n.new('ShaderNodeNormalMap')
    l.new(nt.outputs['Color'], nm.inputs['Color']); l.new(nm.outputs['Normal'], p.inputs['Normal'])
    m['own'] = {'name': name, **{k: str(v) for k, v in maps.items()}}
    return m


def toy(B, col, name):
    """The toy's mesh, standing on its origin, dressed in its material, in `col`."""
    glb = TOYS[name] / (name + '_low.glb')
    if not glb.exists():
        raise RuntimeError('%s missing: run tools/lobby_toys/make_toy.py' % glb)
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(glb))
    new = [o for o in bpy.data.objects if o not in before]
    o = next(x for x in new if x.type == 'MESH')
    for x in new:
        for c in list(x.users_collection):
            c.objects.unlink(x)
        if x is not o:
            bpy.data.objects.remove(x)
    o.parent = None
    o.rotation_mode = 'XYZ'                 # the importer's is a quaternion: table() turns the toy by its Euler angles
    o.matrix_world = Matrix.Identity(4)
    o.name = 'item ' + name
    o.data.materials.clear()
    o.data.materials.append(B.M[name])
    col.objects.link(o)
    shape, half, at = GRASPS[name]
    o['lobby_grasp'], o['lobby_grasp_half'], o['lobby_grasp_at'] = shape, half, at
    return o
