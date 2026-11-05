"""12 second, real-time preview of the first native TransformAnimator group. Does not modify the saved .blend."""
import bpy,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'art/vr_lobby/tutorial'
data=json.loads((OUT/'native_manifest.json').read_text(encoding='utf-8'))
group=data['animated_groups'][0];visible=set(group['mesh_objects'])
for obj in bpy.data.objects:
    if obj.type in ('MESH','CURVE','FONT') and obj.name not in visible:obj.hide_render=True
scene=bpy.context.scene;scene.camera=bpy.data.objects['Detail / native motion']
scene.render.resolution_x=720;scene.render.resolution_y=406;scene.render.resolution_percentage=100
scene.cycles.samples=8;scene.cycles.use_denoising=True
scene.frame_start=1;scene.frame_end=285;scene.frame_step=4
# Curves were authored at 24 fps. Every fourth frame at 6 output fps preserves native seconds.
scene.render.fps=6;scene.render.fps_base=1
if hasattr(scene.render.image_settings,'media_type'):scene.render.image_settings.media_type='VIDEO'
scene.render.image_settings.file_format='FFMPEG';scene.render.ffmpeg.format='MPEG4';scene.render.ffmpeg.codec='H264'
scene.render.ffmpeg.constant_rate_factor='MEDIUM';scene.render.ffmpeg.ffmpeg_preset='GOOD'
scene.render.filepath=str(OUT/'native_motion_preview.mp4')
bpy.ops.render.render(animation=True)
print('NATIVE_MOTION_PREVIEW_DONE',scene.render.filepath,flush=True)
