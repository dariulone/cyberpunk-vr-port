"""The camera scopes' camera, vr_optics_camera, on the player templates.

    py -3.13 -B tools/optics_tests/add_optics_camera.py

WHAT IT IS. A scope with a camera path (CyberpunkVRPort_Optics, path 'camera') shows what an
entRenderToTextureCameraComponent on the player sees: vr_optics_camera renders into base\\vrport\\optics\\scope_feed.dtex
(virtualCameraName vr_optics_feed), which the scope's lens samples. The Lua turns it on while such a scope is held and
places it in the WeaponRight slot's frame every frame; its binding is that slot (a camera on the player's root jerked
with every body-yaw step). Without the component the lens is black and the mod reports "Optics camera missing".

WHERE IT GOES. Into the templates' sources (mods/archive/source/raw), then the pack is built from them
(make_lobby_scene.pack_sources). It was first added to the pack's cooked copies only, and the next tool that rebuilt
the templates from their sources (tools/neck_cap) took it away with no error anywhere.

RESOLUTION 1024 x 1024, the size scope_feed.dtex is cooked at; the field of view is the Lua's, per scope, every frame.
Idempotent: a template that already carries the camera is left alone.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/vr_lobby_assets'), str(ROOT / 'tools/ent')]
import make_lobby_scene as S  # noqa: E402
import add_hold_components as H  # noqa: E402

NAME = 'vr_optics_camera'
FEED = 'vr_optics_feed'
TEXTURE = 'base\\vrport\\optics\\scope_feed.dtex'
SIZE = 1024
TEMPLATES = ['base/characters/entities/player/player_ma_fpp.ent', 'base/characters/entities/player/player_wa_fpp.ent',
             'ep1/characters/entities/player/player_ma_fpp_ep1.ent',
             'ep1/characters/entities/player/player_wa_fpp_ep1.ent']


def no_resource():
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'uint64', '$value': '0'}, 'Flags': 'Default'}


def camera(handle, comp_id):
    return {
        '$type': 'entRenderToTextureCameraComponent',
        'albedoDynamicTextureRes': no_resource(),
        'aspectRatio': 1,
        'backgroundColor': {'$type': 'Color', 'Alpha': 255, 'Blue': 0, 'Green': 0, 'Red': 0},
        'depthCutDistance': 0,
        'depthDynamicTextureRes': no_resource(),
        'dynamicTextureRes': {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': TEXTURE},
                              'Flags': 'Default'},
        'env': no_resource(),
        'farPlaneOverride': 16000,
        'features': {'$type': 'entRenderToTextureFeatures', 'antiAliasing': 'RTFP_PC', 'contactShadows': 1,
                     'localShadows': 1, 'reflections': 'RTFP_PC', 'renderDecals': 1, 'renderForwardNoTXAA': 0,
                     'renderParticles': 1, 'SSAO': 'RTFP_PC'},
        'fov': 8,
        'id': comp_id,
        'isEnabled': 0,
        'isReplicable': 0,
        'localTransform': H.identity_transform(),
        'motionBlurScale': 0,
        'name': H.cname(NAME),
        'nearPlaneOverride': 0.0500000007,
        'normalsDynamicTextureRes': no_resource(),
        'overrideBackgroundColor': 0,
        'params': {'$type': 'WorldRenderAreaSettings', 'areaParameters': []},
        'parentTransform': {
            'HandleId': str(handle),
            'Data': {'$type': 'entHardTransformBinding', 'bindName': H.cname(H.BIND_TO), 'enabled': 1,
                     'enableMask': H.enable_mask(), 'slotName': H.cname('WeaponRight')},
        },
        'particlesDynamicTextureRes': no_resource(),
        'renderingMode': 'Shaded',
        'renderSceneLayer': 'Default',
        'resolutionHeight': SIZE,
        'resolutionWidth': SIZE,
        'streamingDistance': 12500,
        'virtualCameraName': H.cname(FEED),
        'zoom': 1,
    }


def main():
    raw = ROOT / 'mods/archive/source/raw'
    for rel in TEMPLATES:
        where, what = H.add(str(raw / (rel + '.json')), (NAME,), lambda handle, base_id: [camera(handle, str(base_id))])
        print('%-32s %s' % (Path(rel).name, what))
    S.pack_sources(TEMPLATES, ROOT / 'build/optics-camera/ents')


if __name__ == '__main__':
    main()
