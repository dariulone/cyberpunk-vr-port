"""The holster place icons of the VR menu's inventory (CyberpunkVRPort_Menu modules/inventory_view.lua): a figure and,
on a layer of its own, the weapon where it hangs, for every place and family the holsters have, both hands' sides --
over a man's figure or a woman's, as the player's body is.

    py -3.13 -B tools/vr_lobby_assets/make_holster_icons.py [pack]

  holster_icons.exe (holster_icons.cpp)  draws the parts (white premultiplied by alpha, 128 px cells)
  vrport\\ui\\vr_holster_icons.xbm         the sheet, imported over the phone's UI texture as the wheel's is
  vrport\\ui\\vr_holster_icons.inkatlas    its parts (JSON in mods/archive/source/raw/vrport/ui)
  modules/holster_icons.lua              the atlas path and the part names the page draws

The texture and the atlas are made the way make_wheel_art.py makes the weapon wheel's. `pack` packs the port's archive
afterwards (make_lobby_scene.pack_archive).
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_lobby_scene as S  # noqa: E402
import make_wheel_art as W  # noqa: E402

HERE = Path(__file__).resolve().parent
WORK = S.ROOT / 'build/holster-icons'
TOOL = WORK / 'holster_icons.exe'
LUA = S.ROOT / 'mods/cet/CyberpunkVRPort_Menu/modules/holster_icons.lua'
NAME = 'vr_holster_icons'
SHEET_W, SHEET_H, CELL = 1024, 1024, 128

# Every place a family can hang in (modules/placements.lua P.CHOICES), under its name on either side of the body.
FAMILIES = {
    'gun': ('hip_r', 'hip_l', 'thigh_r', 'thigh_l', 'underarm_l', 'underarm_r'),
    'blade': ('hip_l', 'hip_r', 'shoulder_r', 'shoulder_l', 'lower_back'),
    'long': ('shoulder_r', 'shoulder_l', 'lower_back'),
    'knife': ('chest_l', 'chest_r', 'thigh_r', 'thigh_l', 'lower_back'),
}
# The woman's figure (body_w) and, of the weapons, those on the back: drawn faint where her body hides them, so cut by
# her outline (<place>_<family>_w); every other weapon layer is the same over either figure.
BEHIND = ('shoulder_r', 'shoulder_l', 'lower_back')


def build_tool():
    src = HERE / 'holster_icons.cpp'
    if not TOOL.exists() or TOOL.stat().st_mtime < src.stat().st_mtime:
        S.run('cmd', '/c', HERE / 'build_holster_icons.cmd')


def draw():
    build_tool()
    WORK.mkdir(parents=True, exist_ok=True)
    parts = ['body', 'body_w', 'none'] + ['%s:%s' % (place, family) for family, places in FAMILIES.items()
                                          for place in places]
    parts += ['%s:%s:w' % (place, family) for family, places in FAMILIES.items() for place in places
              if place in BEHIND]
    S.run(TOOL, WORK / 'sheet.tga', WORK / 'parts.json', SHEET_W, SHEET_H, CELL, *parts)
    return json.loads((WORK / 'parts.json').read_text(encoding='utf-8'))


def lua(sheet):
    names = sorted(sheet['parts'])
    LUA.write_text('\n'.join([
        '-- Written by tools/vr_lobby_assets/make_holster_icons.py: the holster place icons\' atlas and its parts --',
        '-- "body" (the figure; "body_w" the woman\'s), "none" (not on the body) and <place>_<family> (the weapon where',
        '-- it hangs; <place>_<family>_w over the woman\'s figure where it differs), each a square cell drawn over the',
        '-- figure\'s.',
        'return {',
        '  ATLAS = "%s",' % (W.DEPOT + NAME + '.inkatlas').replace('\\', '\\\\'),
        '  parts = {',
        *['    ["%s"] = true,' % n for n in names],
        '  },',
        '}',
        '',
    ]), encoding='utf-8', newline='\n')
    print('wrote', LUA)


if __name__ == '__main__':
    sheet = draw()
    # the wheel's texture and atlas steps, for this sheet
    W.WORK, W.NAME = WORK, NAME
    W.texture()
    W.atlas(sheet)
    lua(sheet)
    if 'pack' in sys.argv[1:]:
        S.pack_archive()
