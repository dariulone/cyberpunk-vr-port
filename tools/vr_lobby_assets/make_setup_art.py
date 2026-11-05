"""The first-launch setup's pictures (mods/cet/CyberpunkVRPort_Lobby modules/setup.lua): the two hands to choose the
weapon hand by, the left hand's three bindings (the right hand's copied, the left stick walking, the left stick turning),
the whole body and the hands alone to choose how the body is drawn by, and the T-pose, as one ink atlas.

    py -3.13 -B tools/vr_lobby_assets/make_setup_art.py

  setup_art/hand_left.png, hand_right.png, tpose.png   the sources (1254 px squares, black figures outlined in
  setup_art/bind_same.png, bind_walk.png, bind_turn.png  white on transparent; the two body ones 1024 px, the
  setup_art/body_full.png, body_hands.png              players' pictures with the background made transparent)
  vrport\\ui\\vr_lobby_setup.xbm                 the eight at 1024 px in a 4096 x 2048 sheet (CELLS: the hands and
                                                the bodies along the top, the T-pose and the bindings below),
                                                premultiplied, imported over the phone's UI texture so it is set
                                                up as the game's UI textures are (true colour, quality
                                                compression, one mip)
  vrport\\ui\\vr_lobby_setup.inkatlas            its parts, named as the files; only slot 0 (the 4K one) has a
                                                texture: ink takes slot 0 when the lower slots are empty

The texture goes straight into the pack folder; the atlas's JSON into mods/archive/source/raw/vrport/ui, deserialized
there by make_lobby_assets.to_pack(). make_lobby_scene.pack_archive() packs them.
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_lobby_scene as S  # noqa: E402

HERE = Path(__file__).resolve().parent
ART = HERE / 'setup_art'
WORK = S.ROOT / 'build/lobby-setup-art'
RAW = S.ROOT / 'mods/archive/source/raw/vrport/ui'
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
NAME = 'vr_lobby_setup'
DEPOT = 'vrport\\ui\\'
SHEET_W, SHEET_H, CELL = 4096, 2048, 1024
CELLS = {'hand_left': (0, 0), 'hand_right': (1, 0), 'tpose': (0, 1), 'body_full': (2, 0), 'body_hands': (3, 0),
         'bind_same': (1, 1), 'bind_walk': (2, 1), 'bind_turn': (3, 1)}
TEMPLATE = 'base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.xbm'


def premultiplied(im):
    """The colour multiplied by the alpha: the lobby's ink draws its pictures premultiplied, so whatever colour lies
    under a transparent pixel shows (a picture whose background was white under alpha 0 came up as a white square,
    tinted by the card)."""
    from PIL import Image
    alpha = im.getchannel('A')
    rgb = Image.new('RGB', im.size, (0, 0, 0))
    rgb.paste(im.convert('RGB'), mask=alpha)
    rgb.putalpha(alpha)
    return rgb


def sheet(out):
    from PIL import Image
    im = Image.new('RGBA', (SHEET_W, SHEET_H), (0, 0, 0, 0))
    for name, (cx, cy) in CELLS.items():
        part = Image.open(ART / (name + '.png')).convert('RGBA').resize((CELL, CELL), Image.LANCZOS)
        im.paste(premultiplied(part), (cx * CELL, cy * CELL))
    im.save(out)


def texture():
    """The sheet imported onto a copy of the template (cp77tools import --keep keeps the copy's setup)."""
    if WORK.exists():
        shutil.rmtree(WORK)
    (WORK / 'raw').mkdir(parents=True)
    (WORK / 'red').mkdir()
    sheet(WORK / 'raw' / (NAME + '.png'))
    S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content/basegame_1_engine.archive', '-o', WORK / 'template', '-r',
          TEMPLATE.replace('\\', '\\\\').replace('.', '\\.') + '$')
    shutil.copyfile(WORK / 'template' / TEMPLATE, WORK / 'red' / (NAME + '.xbm'))
    r = subprocess.run([str(S.CLI), 'import', str(WORK / 'raw'), '-k', '-o', str(WORK / 'red')], capture_output=True,
                       text=True)
    if 'Imported 1/1' not in r.stdout:
        print(r.stdout[-2000:], r.stderr[-1000:])
        raise SystemExit('the setup sheet was not imported')
    target = S.PACK / 'vrport/ui'
    target.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(WORK / 'red' / (NAME + '.xbm'), target / (NAME + '.xbm'))
    print('imported', target / (NAME + '.xbm'))


def path(value):
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': value} if value else
            {'$type': 'ResourcePath', '$storage': 'uint64', '$value': '0'}, 'Flags': 'Soft'}


def part(name, cx, cy):
    fx, fy = CELL / SHEET_W, CELL / SHEET_H
    return {'$type': 'inkTextureAtlasMapper',
            'clippingRectInPixels': {'$type': 'Rect', 'bottom': 0, 'left': 0, 'right': 0, 'top': 0},
            'clippingRectInUVCoords': {'$type': 'RectF', 'Bottom': (cy + 1) * fy, 'Left': cx * fx, 'Right': (cx + 1) * fx,
                                       'Top': cy * fy},
            'partName': {'$type': 'CName', '$storage': 'string', '$value': name}}


def atlas():
    """The atlas, on the main menu's (its header and fields), written as JSON into RAW."""
    template = S.ROOT / 'build/lobby-setup-art/template/menu_background.inkatlas.json'
    S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content/basegame_1_engine.archive', '-o', WORK / 'template', '-r',
          'main_menu\\\\menu_background\\.inkatlas$')
    S.run(S.CLI, 'convert', 'serialize', WORK / 'template/base/gameplay/gui/fullscreen/main_menu', '-o',
          WORK / 'template')
    doc = json.loads(template.read_text(encoding='utf-8-sig'))
    rc = doc['Data']['RootChunk']
    slots = rc['slots']['Elements']
    slots[0]['texture'] = path(DEPOT + NAME + '.xbm')
    slots[0]['parts'] = [part(n, cx, cy) for n, (cx, cy) in CELLS.items()]
    for s in slots[1:]:
        s['texture'], s['parts'], s['slices'] = path(None), [], []
    out = RAW / (NAME + '.inkatlas.json')
    out.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n', encoding='utf-8')
    print('wrote', out)


if __name__ == '__main__':
    texture()
    atlas()
