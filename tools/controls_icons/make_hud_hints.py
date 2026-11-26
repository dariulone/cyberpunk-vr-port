"""The HUD hints' atlas (vrport\\ui\\vr_hud_hints.inkatlas in the port's one archive, mods\\archive\\cyberpunkvrport.archive;
its own cpvr_hud_hints.archive until 0.2.6).

    make_hud_hints.py BASE_JSON_DIR OUT_DIR

BASE_JSON_DIR holds vr_hud_hints.xbm.json / .inkatlas.json as WolvenKit writes them from the archive in the repository
(the controls guide's sheet: Kenney's Meta Quest glyphs and the first "rest"). Two rows of 112-px cells are added below
the sheet with the glyphs the HUD's chord hints use, drawn as the game's own pad glyphs are
(base\\gameplay\\gui\\common\\input\\icons_durango.inkatlas): a white shape with its letters cut out, premultiplied, so
the HUD's tint colours it like the glyph beside it:

    vr_rest_l / vr_rest_r   a Touch controller's thumbrest (a flat pad: a pill, the side's corner cut as LB / RB)
    vr_grip_l / vr_grip_r   the grip (a tall pad on the side, as LT / RT are tall)
    vr_tri                  PSVR2's Triangle touch

The sheet grows from 560 to 784 rows; the old parts keep their pixels (their UVs scaled to the new height). OUT_DIR gets
both JSON files, for WolvenKit's json_to_cr2w; the two cooked files then go into the extracted tree of the one archive
(cp77tools unbundle, copy to vrport\\ui, cp77tools pack -- scripts\\sync_assets.ps1's rule: the whole tree, never a part)."""
import base64
import copy
import json
import pathlib
import sys
from PIL import Image, ImageDraw, ImageFont

CELL = 112
SCALE = 4
NEW_ROWS = 2


def font(px):
    for name in ('seguibl.ttf', 'ariblk.ttf', 'arialbd.ttf'):
        try:
            return ImageFont.truetype(name, px)
        except OSError:
            continue
    return ImageFont.load_default()


def cut_text(d, box, text, px):
    """The text cut out of the shape (drawn in 0), centred in box."""
    f = font(px)
    b = d.textbbox((0, 0), text, font=f)
    tw, th = b[2] - b[0], b[3] - b[1]
    x0, y0, x1, y1 = box
    d.text(((x0 + x1 - tw) / 2 - b[0], (y0 + y1 - th) / 2 - b[1]), text, font=f, fill=0)


def pad(side, text, tall):
    """A pad glyph: a rounded body, one corner cut on its side (as the game's LB / RB), the word cut out."""
    s = CELL * SCALE
    im = Image.new('L', (s, s), 0)
    d = ImageDraw.Draw(im)
    if tall:
        w, h = int(s * 0.62), int(s * 0.84)
    else:
        w, h = int(s * 0.94), int(s * 0.66)
    x0, y0 = (s - w) // 2, (s - h) // 2
    x1, y1 = x0 + w, y0 + h
    r = int(min(w, h) * 0.30)
    d.rounded_rectangle([x0, y0, x1, y1], radius=r, fill=255)
    cut = int(min(w, h) * 0.42)
    # the side's top corner cut away, as the game's LB / RB (its outer side)
    if side == 'l':
        d.polygon([(x0 - 2, y0 - 2), (x0 + cut, y0 - 2), (x0 - 2, y0 + cut)], fill=0)
    else:
        d.polygon([(x1 + 2, y0 - 2), (x1 - cut, y0 - 2), (x1 + 2, y0 + cut)], fill=0)
    # two lines: the side's letter over the word
    if tall:
        cut_text(d, (x0, y0 + h * 0.10, x1, y0 + h * 0.48), side.upper(), int(h * 0.30))
        cut_text(d, (x0, y0 + h * 0.48, x1, y1 - h * 0.08), text, int(w * 0.27))
    else:
        cut_text(d, (x0, y0 + h * 0.06, x1, y0 + h * 0.46), side.upper(), int(h * 0.36))
        cut_text(d, (x0, y0 + h * 0.44, x1, y1 - h * 0.06), text, int(h * 0.36))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def triangle():
    s = CELL * SCALE
    im = Image.new('L', (s, s), 0)
    d = ImageDraw.Draw(im)
    m = int(s * 0.08)
    d.ellipse([m, m, s - m, s - m], fill=255)
    c, r = s / 2, s * 0.26
    pts = [(c, c - r), (c + r * 0.87, c + r * 0.5), (c - r * 0.87, c + r * 0.5)]
    d.polygon(pts, fill=0)
    inner = [(c + (x - c) * 0.45, c + (y - c) * 0.45 + r * 0.05) for x, y in pts]
    d.polygon(inner, fill=255)
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


GLYPHS = [
    ('vr_rest_l', lambda: pad('l', 'REST', False)),
    ('vr_rest_r', lambda: pad('r', 'REST', False)),
    ('vr_grip_l', lambda: pad('l', 'GRIP', True)),
    ('vr_grip_r', lambda: pad('r', 'GRIP', True)),
    ('vr_tri', triangle),
]


def main():
    base, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    xbm = json.loads((base / 'vr_hud_hints.xbm.json').read_text(encoding='utf-8-sig'))
    ink = json.loads((base / 'vr_hud_hints.inkatlas.json').read_text(encoding='utf-8-sig'))
    root = xbm['Data']['RootChunk']
    w, h = root['width'], root['height']
    blob = root['renderTextureResource']['renderResourceBlobPC']['Data']
    raw = base64.b64decode(blob['textureData']['Bytes'])
    assert len(raw) == w * h * 4, (len(raw), w, h)
    # rows bottom up in the texture: the sheet as drawn (and as its UVs read it) is the flip
    old = Image.frombytes('RGBA', (w, h), raw).transpose(Image.Transpose.FLIP_TOP_BOTTOM)
    parts = ink['Data']['RootChunk']['slots']['Elements'][0]['parts']
    names = [p['partName']['$value'] for p in parts]
    # an earlier run's glyphs are drawn again, not added twice
    keep = [p for p in parts if not p['partName']['$value'].startswith('vr_')]
    if len(keep) != len(parts):
        h = max(round(p['clippingRectInUVCoords']['Bottom'] * h) for p in keep)
        old = old.crop((0, 0, w, h))
        parts[:] = keep
    nh = h + NEW_ROWS * CELL
    sheet = Image.new('RGBA', (w, nh), (0, 0, 0, 0))
    sheet.paste(old, (0, 0))
    for p in parts:
        uv = p['clippingRectInUVCoords']
        uv['Top'] = uv['Top'] * h / nh
        uv['Bottom'] = uv['Bottom'] * h / nh
    cols = w // CELL
    template = copy.deepcopy(parts[0])
    for i, (name, draw) in enumerate(GLYPHS):
        x, y = (i % cols) * CELL, h + (i // cols) * CELL
        a = draw()
        # premultiplied white: the colour is the coverage
        sheet.paste(Image.merge('RGBA', (a, a, a, a)), (x, y))
        p = copy.deepcopy(template)
        p['clippingRectInUVCoords'] = {'$type': 'RectF', 'Bottom': (y + CELL) / nh, 'Left': x / w, 'Right': (x + CELL) / w, 'Top': y / nh}
        p['partName'] = {'$type': 'CName', '$storage': 'string', '$value': name}
        parts.append(p)
    data = sheet.transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes()
    blob['textureData']['Bytes'] = base64.b64encode(data).decode('ascii')
    root['height'] = nh
    hdr = blob['header']
    hdr['sizeInfo']['height'] = nh
    mip = hdr['mipMapInfo'][0]
    mip['layout']['rowPitch'] = w * 4
    mip['layout']['slicePitch'] = len(data)
    mip['placement']['size'] = len(data)
    hdr['textureInfo']['sliceSize'] = len(data)
    hdr['textureInfo']['textureDataSize'] = len(data)
    out.mkdir(parents=True, exist_ok=True)
    (out / 'vr_hud_hints.xbm.json').write_text(json.dumps(xbm), encoding='utf-8')
    (out / 'vr_hud_hints.inkatlas.json').write_text(json.dumps(ink, ensure_ascii=False, indent=1), encoding='utf-8')
    preview = Image.new('RGBA', sheet.size, (24, 34, 44, 255))
    preview.alpha_composite(sheet)
    preview.convert('RGB').save(out / 'hints_sheet.jpg', quality=92)
    print('sheet', w, 'x', nh, 'parts', len(parts), 'was', names[-3:])


main()
