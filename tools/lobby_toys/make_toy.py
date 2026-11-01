"""A toy for the lobby's table from a Meshy model (tools/vr_lobby_assets/lobby_toys.py). Run by Blender:

    blender --background --factory-startup --python tools/lobby_toys/make_toy.py -- SRC.glb OUTDIR NAME HEIGHT

A model as it comes is some hundred thousand triangles with 4K textures -- a statue, not a thing for a hand. Here it
is:
  - stood up, its feet at the origin, HEIGHT metres tall;
  - collapsed to TRIANGLES (Blender's decimate keeps its UVs, so its own colour texture still fits);
  - its detail baked back as a tangent-space normal map from the full model, the full model's own normal map in it
    (Cycles bakes the shading normal);
  - written as OUTDIR/NAME_low.glb (geometry and UVs only) and OUTDIR/NAME_n.png.
The colour and the metal/roughness maps are the model's own, scaled and split by tools/lobby_toys/toy_maps.py.
"""
import sys
from pathlib import Path

import bpy
from mathutils import Vector

argv = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT, NAME, HEIGHT = Path(argv[0]).resolve(), Path(argv[1]).resolve(), argv[2], float(argv[3])
OUT.mkdir(parents=True, exist_ok=True)
TRIANGLES = 20000      # held in front of the eyes: smooth at arm's length, one object among dozens
NORMAL_SIZE = 2048

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(SRC))
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
for o in bpy.context.scene.objects:
    o.select_set(o in meshes)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
hi = bpy.context.view_layer.objects.active
hi.name = NAME + '_high'
bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM')
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Feet on the origin, HEIGHT tall.
pts = [hi.matrix_world @ Vector(c) for c in hi.bound_box]
lo_z = min(p.z for p in pts)
size = max(p.z for p in pts) - lo_z
cx = (min(p.x for p in pts) + max(p.x for p in pts)) / 2
cy = (min(p.y for p in pts) + max(p.y for p in pts)) / 2
k = HEIGHT / size
for v in hi.data.vertices:
    v.co = Vector(((v.co.x - cx) * k, (v.co.y - cy) * k, (v.co.z - lo_z) * k))
hi.data.update()
print('high: %d triangles, %.3f x %.3f x %.3f m' % (
    sum(len(p.vertices) - 2 for p in hi.data.polygons), *(hi.dimensions)), flush=True)

# The low one: the same mesh collapsed.
lo = hi.copy()
lo.data = hi.data.copy()
lo.name = NAME + '_low'
bpy.context.scene.collection.objects.link(lo)
tris = sum(len(p.vertices) - 2 for p in hi.data.polygons)
dec = lo.modifiers.new('decimate', 'DECIMATE')
dec.decimate_type = 'COLLAPSE'
dec.ratio = min(1.0, TRIANGLES / tris)
dec.use_collapse_triangulate = True
bpy.context.view_layer.objects.active = lo
for o in bpy.context.scene.objects:
    o.select_set(o is lo)
bpy.ops.object.modifier_apply(modifier='decimate')
bpy.ops.object.shade_smooth()
print('low: %d triangles' % sum(len(p.vertices) - 2 for p in lo.data.polygons), flush=True)

# The bake target on the low one: its own material with an image node, active.
img = bpy.data.images.new(NAME + '_n', NORMAL_SIZE, NORMAL_SIZE, alpha=False, float_buffer=False)
img.colorspace_settings.name = 'Non-Color'
mat = bpy.data.materials.new(NAME + '_bake')
mat.use_nodes = True
node = mat.node_tree.nodes.new('ShaderNodeTexImage')
node.image = img
mat.node_tree.nodes.active = node
lo.data.materials.clear()
lo.data.materials.append(mat)

scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.samples = 4
try:
    prefs = bpy.context.preferences.addons['cycles'].preferences
    for kind in ('OPTIX', 'CUDA'):
        try:
            prefs.compute_device_type = kind
            prefs.get_devices()
            if any(d.type == kind for d in prefs.devices):
                for d in prefs.devices:
                    d.use = d.type == kind
                scene.cycles.device = 'GPU'
                print('bake on', kind, flush=True)
                break
        except TypeError:
            continue
except KeyError:
    pass
for o in scene.objects:
    o.select_set(o is hi or o is lo)
bpy.context.view_layer.objects.active = lo
bake = scene.render.bake
bake.use_selected_to_active = True
# The cage and the ray reach: 2 and 6 mm on a 15 cm figure, in proportion on others.
bake.cage_extrusion = 0.002 * HEIGHT / 0.15
bake.max_ray_distance = 0.006 * HEIGHT / 0.15
bake.normal_space = 'TANGENT'
bake.margin = 8
bpy.ops.object.bake(type='NORMAL')
img.file_format = 'PNG'
img.save(filepath=str(OUT / (NAME + '_n.png')))
print('normal map baked', flush=True)

# The low mesh alone, its UVs and normals: lobby_blender.py dresses it.
lo.data.materials.clear()
for o in scene.objects:
    o.select_set(o is lo)
bpy.ops.export_scene.gltf(filepath=str(OUT / (NAME + '_low.glb')), use_selection=True, export_materials='NONE',
                          export_normals=True, export_texcoords=True, export_yup=True)
print('WROTE', OUT / (NAME + '_low.glb'), flush=True)
