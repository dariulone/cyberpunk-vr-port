"""The weapon wheel's cell art (mods/cet/CyberpunkVRPort_Menu modules/wheel.lua): its wedges and round cells as
one ink atlas, and the geometry the page draws and points with, in one place.

    py -3.13 -B tools/vr_lobby_assets/make_wheel_art.py [pack]

  wheel_art.exe (wheel_art.cpp)          draws the parts: white premultiplied by the shape's alpha (fill, edge,
                                        accent band)
  vrport\\ui\\vr_wheel.xbm                the sheet, imported over the phone's UI texture as make_setup_art.py does
  vrport\\ui\\vr_wheel.inkatlas           its parts; JSON in mods/archive/source/raw/vrport/ui, deserialized into
                                        the pack folder
  modules/wheel_art.lua                 the shapes' radii, spans and insets and every part's size and centre

`pack` packs the port's archive afterwards (make_lobby_scene.pack_archive).
"""
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_lobby_scene as S  # noqa: E402

HERE = Path(__file__).resolve().parent
WORK = S.ROOT / 'build/wheel-art'
TOOL = WORK / 'wheel_art.exe'
RAW = S.ROOT / 'mods/archive/source/raw/vrport/ui'
LUA = S.ROOT / 'mods/cet/CyberpunkVRPort_Menu/modules/wheel_art.lua'
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
NAME = 'vr_wheel'
DEPOT = 'vrport\\ui\\'
SHEET_W, SHEET_H = 2048, 1024
TEMPLATE = 'base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.xbm'

# Canvas pixels (the wheel's screen is 3200 px a metre). The weapons' wedges span 52 degrees each, edge to edge
# with their neighbours but for the inset (a gap of twice it, the same width from hub to rim); the arm's is
# smaller and set apart, the holster's smaller still; the round cells are discs of DISC.
WEAPON = {'inner': 170, 'outer': 480, 'half': 26, 'inset': 6, 'rc': 10}
ARM = {'inner': 200, 'outer': 450, 'half': 22, 'inset': 6, 'rc': 9}
HOLSTER = {'inner': 215, 'outer': 425, 'half': 18, 'inset': 6, 'rc': 8}
DISC = 76
SHAPES = (('weapon', WEAPON), ('arm', ARM), ('holster', HOLSTER))


def build_tool():
    src = HERE / 'wheel_art.cpp'
    if not TOOL.exists() or TOOL.stat().st_mtime < src.stat().st_mtime:
        S.run('cmd', '/c', HERE / 'build_wheel_art.cmd')


def draw():
    build_tool()
    WORK.mkdir(parents=True, exist_ok=True)
    shapes = ['wedge:%s:%g:%g:%g:%g:%g' % (name, g['inner'], g['outer'], g['half'], g['inset'], g['rc'])
              for name, g in SHAPES]
    shapes.append('disc:disc:%g' % DISC)
    S.run(TOOL, WORK / 'sheet.tga', WORK / 'parts.json', SHEET_W, SHEET_H, *shapes)
    return json.loads((WORK / 'parts.json').read_text(encoding='utf-8'))


def texture():
    """The sheet imported onto a copy of the template (cp77tools import --keep keeps the copy's setup)."""
    from PIL import Image
    for d in ('raw', 'red', 'template'):
        shutil.rmtree(WORK / d, ignore_errors=True)
    (WORK / 'raw').mkdir()
    (WORK / 'red').mkdir()
    Image.open(WORK / 'sheet.tga').convert('RGBA').save(WORK / 'raw' / (NAME + '.png'))
    S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content/basegame_1_engine.archive', '-o', WORK / 'template', '-r',
          TEMPLATE.replace('\\', '\\\\').replace('.', '\\.') + '$')
    shutil.copyfile(WORK / 'template' / TEMPLATE, WORK / 'red' / (NAME + '.xbm'))
    r = subprocess.run([str(S.CLI), 'import', str(WORK / 'raw'), '-k', '-o', str(WORK / 'red')], capture_output=True,
                       text=True)
    if 'Imported 1/1' not in r.stdout:
        print(r.stdout[-2000:], r.stderr[-1000:])
        raise SystemExit('the wheel sheet was not imported')
    target = S.PACK / 'vrport/ui'
    target.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(WORK / 'red' / (NAME + '.xbm'), target / (NAME + '.xbm'))
    print('imported', target / (NAME + '.xbm'))


def path(value):
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': value} if value else
            {'$type': 'ResourcePath', '$storage': 'uint64', '$value': '0'}, 'Flags': 'Soft'}


def part(name, p, sheet):
    w, h = sheet['width'], sheet['height']
    return {'$type': 'inkTextureAtlasMapper',
            'clippingRectInPixels': {'$type': 'Rect', 'bottom': 0, 'left': 0, 'right': 0, 'top': 0},
            'clippingRectInUVCoords': {'$type': 'RectF', 'Bottom': (p['y'] + p['h']) / h, 'Left': p['x'] / w,
                                       'Right': (p['x'] + p['w']) / w, 'Top': p['y'] / h},
            'partName': {'$type': 'CName', '$storage': 'string', '$value': name}}


def atlas(sheet):
    """The atlas, on the main menu's, written as JSON into RAW and deserialized into the pack folder."""
    S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content/basegame_1_engine.archive', '-o', WORK / 'template', '-r',
          'main_menu\\\\menu_background\\.inkatlas$')
    S.run(S.CLI, 'convert', 'serialize', WORK / 'template/base/gameplay/gui/fullscreen/main_menu', '-o',
          WORK / 'template')
    doc = json.loads((WORK / 'template/menu_background.inkatlas.json').read_text(encoding='utf-8-sig'))
    slots = doc['Data']['RootChunk']['slots']['Elements']
    slots[0]['texture'] = path(DEPOT + NAME + '.xbm')
    slots[0]['parts'] = [part(n, p, sheet) for n, p in sheet['parts'].items()]
    for s in slots[1:]:
        s['texture'], s['parts'], s['slices'] = path(None), [], []
    RAW.mkdir(parents=True, exist_ok=True)
    out = RAW / (NAME + '.inkatlas.json')
    out.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n', encoding='utf-8')
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copyfile(out, Path(tmp) / out.name)
        S.run(S.CLI, 'convert', 'deserialize', tmp, '-o', S.PACK / 'vrport/ui')
    print('wrote', out, 'and deserialized it into', S.PACK / 'vrport/ui')


def lua(sheet):
    def shape(g):
        return '{ inner = %g, outer = %g, half = %g, inset = %g }' % (g['inner'], g['outer'], g['half'], g['inset'])
    rows = ['  %s = { w = %d, h = %d, ox = %.3f, oy = %.3f },' % (n, p['w'], p['h'], p['ox'], p['oy'])
            for n, p in sorted(sheet['parts'].items())]
    LUA.write_text('\n'.join([
        '-- Written by tools/vr_lobby_assets/make_wheel_art.py: the weapon wheel\'s shapes (canvas px, degrees) and',
        '-- its atlas parts, each drawn at its own size with the wheel\'s centre (a disc\'s own) at ox, oy.',
        'return {',
        '  ATLAS = "%s",' % (DEPOT + NAME + '.inkatlas').replace('\\', '\\\\'),
        *['  %s = %s,' % (name, shape(g)) for name, g in SHAPES],
        '  disc = %g,' % DISC,
        '  parts = {',
        *['  ' + r for r in rows],
        '  },',
        '}',
        '',
    ]), encoding='utf-8', newline='\n')
    print('wrote', LUA)


if __name__ == '__main__':
    sheet = draw()
    texture()
    atlas(sheet)
    lua(sheet)
    if 'pack' in sys.argv[1:]:
        S.pack_archive()
