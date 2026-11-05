"""The VR lobby's assets: the terminal's screen, the menu the game draws on the terminal in the lobby.

    py -3.13 -B tools/vr_lobby_assets/make_lobby_assets.py

Derived from the VR menu's own sources in mods/archive/source/raw/vrport/ui (the same engine templates, see
build/vr-menu-20261021/make_assets.py for where those came from), written next to them; serialize with WolvenKit
(json_to_cr2w) and pack into cyberpunkvrport.archive.

  vrport\\ui\\vr_lobby.inkwidget     one library item "Root": an empty canvas of lobby_layout.TERMINAL_CANVAS pixels
  vrport\\ui\\vr_lobby_plane.mesh    the VR menu screen's plane and material (parallaxscreen_transparent_ui.mt,
                                    additive: black is nothing -- the terminal's own dark glass is behind it)
  vrport\\ui\\vr_lobby.ent           the plane ("display", off until the canvas is drawn) at TERMINAL_SCREEN metres and
                                    a WorldWidgetComponent "vrp_lobby_ui" drawing into it; Natives/MenuSurface.cpp
                                    gives it a texture of its own at the canvas's size
  vrport\\ui\\vr_lobby_holo.ent      the same screen at lobby_layout.HOLO_SCREEN metres: a panel of the hologram menu
  vrport\\ui\\vr_lobby_preview.ent   and at HOLO_PREVIEW: the window beside it with a save's picture
  vrport\\ui\\vr_lobby_window.inkwidget / vr_lobby_window.ent   the new game's window: a canvas of
                                    lobby_layout.HOLO_WINDOW_CANVAS pixels at HOLO_WINDOW metres
  vrport\\ui\\vr_lobby_glass_plane.mesh / vr_lobby_glass.ent   a frosted glass plane the same size (the scene's
                                    entities are written on this entity's frame, make_lobby_scene.py)

A hand's reach marker (mods/cet/CyberpunkVRPort_Reach) is geometry of its own: make_reach_marker.py.
"""
import copy
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_layout as LL  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
RAW = ROOT / 'mods/archive/source/raw/vrport/ui'
SCREEN = LL.TERMINAL_SCREEN
CANVAS = LL.TERMINAL_CANVAS
GLASS_TINT = (40, 40, 48)


def read(name):
    return json.loads((RAW / name).read_text(encoding='utf-8-sig'))


def write(name, data):
    path = RAW / name
    newline = '\r\n' if path.exists() and b'\r\n' in path.read_bytes() else '\n'
    path.write_bytes((json.dumps(data, ensure_ascii=False, indent=1) + '\n').replace('\n', newline).encode('utf-8'))
    print('wrote', path)


def depot(path):
    return {'$type': 'ResourcePath', '$storage': 'string', '$value': path}


def component(entity, name):
    return next(c for c in entity['Data']['RootChunk']['components'] if c['name']['$value'] == name)


def widget(name='vr_lobby.inkwidget', canvas=CANVAS):
    w = read('vr_menu.inkwidget.json')
    root = w['Data']['RootChunk']['libraryItems'][0]['package']['Data']['File']['RootChunk']['rootWidget']['Data']
    root['size']['X'], root['size']['Y'] = canvas
    write(name + '.json', w)


def plane(source, name, tint=None):
    m = read(source)
    if tint:
        for v in m['Data']['RootChunk']['preloadLocalMaterialInstances'][0]['Data']['values']:
            if 'TintColor' in v:
                v['TintColor'].update({'Red': tint[0], 'Green': tint[1], 'Blue': tint[2], 'Alpha': 255})
    write(name + '.json', m)


def screen(name='vr_lobby.ent', size=SCREEN, canvas='vr_lobby.inkwidget'):
    e = read('vr_menu.ent.json')
    display = component(e, 'display')
    display['mesh']['DepotPath'] = depot('vrport\\ui\\vr_lobby_plane.mesh')
    display['visualScale'].update({'X': size[0], 'Y': size[1], 'Z': 1})
    ui = component(e, 'vrp_menu_ui')
    ui['name']['$value'] = 'vrp_lobby_ui'
    ui['widgetResource']['DepotPath'] = depot('vrport\\ui\\' + canvas)
    ui['sceneWidgetProperties']['projectionPlaneSize'].update({'X': size[0], 'Y': size[1]})
    write(name + '.json', e)


def glass():
    e = read('vr_menu_glass.ent.json')
    display = component(e, 'display')
    display['mesh']['DepotPath'] = depot('vrport\\ui\\vr_lobby_glass_plane.mesh')
    display['visualScale'].update({'X': SCREEN[0], 'Y': SCREEN[1], 'Z': 1})
    display['isEnabled'] = 1
    component(e, 'vrp_menu_glass')['name']['$value'] = 'vrp_lobby_glass'
    write('vr_lobby_glass.ent.json', e)


def to_pack():
    """The lobby's screen files, deserialized into the pack folder (make_lobby_scene.py packs it)."""
    import shutil
    import tempfile
    import make_lobby_scene as S
    with tempfile.TemporaryDirectory() as tmp:
        for f in RAW.glob('vr_lobby*.json'):
            shutil.copyfile(f, Path(tmp) / f.name)
        S.run(S.CLI, 'convert', 'deserialize', tmp, '-o', S.PACK / 'vrport/ui')
    print('deserialized into', S.PACK / 'vrport/ui')


if __name__ == '__main__':
    widget()
    widget('vr_lobby_window.inkwidget', LL.HOLO_WINDOW_CANVAS)
    plane('vr_menu_plane.mesh.json', 'vr_lobby_plane.mesh')
    plane('vr_menu_glass_plane.mesh.json', 'vr_lobby_glass_plane.mesh', GLASS_TINT)
    screen()
    screen('vr_lobby_holo.ent', LL.HOLO_SCREEN)
    screen('vr_lobby_preview.ent', LL.HOLO_PREVIEW)
    screen('vr_lobby_window.ent', LL.HOLO_WINDOW, 'vr_lobby_window.inkwidget')
    glass()
    to_pack()
