"""Renders the built lobby (art/vr_lobby/reference/lobby_reference.blend) from the player's eyes, 360 x 180.

    blender --background art/vr_lobby/reference/lobby_reference.blend --python tools/vr_lobby_assets/lobby_render.py
        -- [--samples N] [--size W] [--out NAME] [--camera NAME]
"""
import sys
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[2]
argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []


def arg(name, default):
    return type(default)(argv[argv.index(name) + 1]) if name in argv else default


scene = bpy.context.scene
scene.cycles.samples = arg('--samples', 64)
width = arg('--size', 2048)
camera = bpy.data.objects[arg('--camera', 'panorama')]
scene.camera = camera
if camera.data.type == 'PANO':
    scene.render.resolution_x, scene.render.resolution_y = width, width // 2
else:
    scene.render.resolution_x, scene.render.resolution_y = width, width * 3 // 4
scene.render.resolution_percentage = 100
prefs = bpy.context.preferences.addons['cycles'].preferences
prefs.compute_device_type = 'OPTIX'
prefs.get_devices()
for dev in prefs.devices:
    dev.use = dev.type == 'OPTIX'
scene.cycles.device = 'GPU'
out = ROOT / 'art/vr_lobby/reference/renders' / (arg('--out', 'panorama') + '.png')
out.parent.mkdir(parents=True, exist_ok=True)
scene.render.filepath = str(out)
bpy.ops.render.render(write_still=True)
print('RENDERED', out, flush=True)
