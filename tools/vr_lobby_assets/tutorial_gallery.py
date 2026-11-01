"""Inspect the actual q000 VR tutorial meshes before composing the lobby."""
import bpy, math
from pathlib import Path
from mathutils import Vector
ROOT=Path(__file__).resolve().parents[2]
bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
mat=bpy.data.materials.new('Neutral inspection clay');mat.diffuse_color=(.22,.28,.33,1)
em=bpy.data.materials.new('Original emissive subset');em.diffuse_color=(1,.57,.12,1)
names=['vr_cover_block_h200_w200_l200_a','vr_cover_block_h200_w100_l300_a','vr_cover_block_h110_w100_l200_a',
       'vr_pillar_base_h100_w140_l140_a1','vr_architecture_floor_w600_l600_a','vr_architecture_stairs_h200_w300_a',
       'vr_architecture_wall_door_double_h400_l400_a','q000_vr_background_structure_01_a']
for i,name in enumerate(names):
    path=next((ROOT/'build/lobby-tutorial/gltf').rglob(name+'.glb'))
    before=set(bpy.data.objects);bpy.ops.import_scene.gltf(filepath=str(path))
    added=set(bpy.data.objects)-before
    meshes=[o for o in added if o.type=='MESH'];bounds=[o.matrix_world@Vector(v) for o in meshes for v in o.bound_box]
    lo=Vector([min(v[k] for v in bounds) for k in range(3)]);hi=Vector([max(v[k] for v in bounds) for k in range(3)])
    scale=3.3/max(hi-lo);offset=Vector(((i%4)*5,(i//4)*-6,0))
    for o in meshes:
        matrix=o.matrix_world.copy();o.parent=None;o.matrix_world=matrix
        o.location=(o.location-Vector(((lo.x+hi.x)/2,(lo.y+hi.y)/2,lo.z)))*scale+offset
        o.scale*=scale;o.data.materials.clear();o.data.materials.append(mat)
    for o in added:
        if o.type!='MESH':bpy.data.objects.remove(o,do_unlink=True)
    curve=bpy.data.curves.new('Label','FONT');curve.body=name.replace('vr_','').replace('architecture_','').replace('q000_','')
    curve.size=.18;curve.align_x='CENTER'
    label=bpy.data.objects.new('Label',curve);bpy.context.collection.objects.link(label);label.location=offset+Vector((0,-1.9,.02))
    label.rotation_euler=(0,0,0);curve.materials.append(em)
bpy.ops.object.camera_add(location=(15,-23,22));cam=bpy.context.object
cam.rotation_euler=(Vector((7.5,-2,1))-cam.location).to_track_quat('-Z','Y').to_euler();cam.data.type='ORTHO';cam.data.ortho_scale=22
s=bpy.context.scene;s.camera=cam;s.render.engine='BLENDER_WORKBENCH';s.render.resolution_x=1600;s.render.resolution_y=1000;s.render.resolution_percentage=100
s.display.shading.light='STUDIO';s.display.shading.color_type='MATERIAL';s.display.shading.show_cavity=True
s.render.filepath=str(ROOT/'build/lobby-tutorial/gallery.png');bpy.ops.render.render(write_still=True)
