"""The control icons as an ink atlas, for the lobby's controls guide (mods/cet/CyberpunkVRPort_Lobby modules/controls.lua).

    python tools/controls_icons/make_ink_icons.py KENNEY_PACK --cli WOLVENKIT_CLI --game GAME_DIR [--pack DIR]

  vrport\\ui\\vr_controls_icons.xbm        every icon of make_icons.py (Kenney's glyphs and the gestures drawn to match),
                                          white on clear -- premultiplied, so its colour is its alpha -- CELL px each in
                                          one sheet, for ink to tint in a panel's colours
  vrport\\ui\\vr_controls_icons.inkatlas   a part per icon, named as the icon; JSON in
                                          mods/archive/source/raw/vrport/ui, deserialized into the pack folder

THE TEXTURE IS WRITTEN, NOT IMPORTED. WolvenKit's import compresses through DirectXTex, which only runs on Windows; this
writes the game's phone UI texture's JSON (true colour, one mip, gamma, the UI group) with the sheet's pixels in it,
uncompressed (TCM_None: R8G8B8A8), and WolvenKit turns that JSON back into the .xbm -- which its Linux build does too.

--pack selects the staging directory for cyberpunkvrport.archive. Needs rsvg-convert and Pillow or ImageMagick.
"""
import argparse
import base64
import json
import math
import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import make_icons  # noqa: E402

NAME = "vr_controls_icons"
DEPOT = "vrport\\ui\\"
RAW = ROOT / "mods/archive/source/raw/vrport/ui"
PACK = ROOT / "build/one-archive-20261027/pack/cyberpunkvrport"   # make_lobby_scene.PACK
# px per icon (the 64-unit grid at 1.75x; the panels draw them at 104-124 px). The sheet stays near 2 MB: WolvenKit
# 9.1.1's Linux build lost the end of a 4 MB sheet's pixels (from byte 2,560,128) between its JSON and the .xbm, so
# every build is read back and compared (verify).
CELL = 112
COLS = 8
TEMPLATE_XBM = "base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.xbm"
TEMPLATE_ATLAS = "base\\gameplay\\gui\\fullscreen\\main_menu\\menu_background.inkatlas"


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode or "Error" in r.stdout:
        print(r.stdout[-3000:], r.stderr[-2000:])
        raise SystemExit("failed: %s" % (args[:3],))
    return r.stdout


def sheet(pack, work):
    """Every icon rendered white on clear, CELL px, into one premultiplied RGBA sheet; its bytes and each part's cell."""
    from PIL import Image
    icons = make_icons.glyphs(pack)
    names = list(icons)
    rows = math.ceil(len(names) / COLS)
    w, h = COLS * CELL, rows * CELL
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    parts = {}
    for i, name in enumerate(names):
        svg = work / (name + ".svg")
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">'
                       f"{icons[name]}</svg>")
        png = work / (name + ".png")
        run("rsvg-convert", "-w", CELL, "-h", CELL, svg, "-o", png)
        x, y = (i % COLS) * CELL, (i // COLS) * CELL
        out.alpha_composite(Image.open(png).convert("RGBA"), (x, y))
        parts[name] = {"x": x, "y": y, "w": CELL, "h": CELL}
    # premultiplied white: the colour is the coverage
    a = out.getchannel("A")
    out = Image.merge("RGBA", (a, a, a, a))
    out.save(work / (NAME + ".png"))
    # The game takes an uncompressed texture's first row as its BOTTOM: written top down, every part showed the cell
    # mirrored across the sheet's middle, upside down. The rows go in bottom up; the parts' UVs stay as drawn.
    return out.transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes(), w, h, parts


def templates(cli, game, work):
    """The phone's UI texture and the main menu's atlas from the game, as JSON."""
    ext = work / "template"
    regex = "|".join(t.replace("\\", "\\\\").replace(".", "\\.") + "$" for t in (TEMPLATE_XBM, TEMPLATE_ATLAS))
    run(cli, "extract", game / "archive/pc/content", "-o", ext, "-r", regex, "-v", "Minimal")
    js = work / "template_json"
    js.mkdir(exist_ok=True)
    for t in (TEMPLATE_XBM, TEMPLATE_ATLAS):
        run(cli, "convert", "serialize", ext / t.replace("\\", "/"), "-o", js, "-v", "Minimal")
    load = lambda n: json.loads((js / (n + ".json")).read_text(encoding="utf-8-sig"))
    return load("new_phone_assets.xbm"), load("menu_background.inkatlas")


def texture(doc, pixels, w, h):
    """The template texture with the sheet in it: uncompressed, one mip, its sizes and pitches."""
    r = doc["Data"]["RootChunk"]
    r["width"], r["height"] = w, h
    s = r["setup"]
    s["compression"], s["rawFormat"], s["hasMipchain"] = "TCM_None", "TRF_TrueColor", 0
    blob = r["renderTextureResource"]["renderResourceBlobPC"]["Data"]
    hd = blob["header"]
    pitch, size = w * 4, w * 4 * h
    hd["sizeInfo"]["width"], hd["sizeInfo"]["height"] = w, h
    mip = hd["mipMapInfo"][0]
    mip["layout"]["rowPitch"], mip["layout"]["slicePitch"] = pitch, size
    mip["placement"]["offset"], mip["placement"]["size"] = 0, size
    del hd["mipMapInfo"][1:]
    ti = hd["textureInfo"]
    ti["mipCount"], ti["sliceCount"], ti["sliceSize"], ti["textureDataSize"] = 1, 1, size, size
    blob["textureData"]["Bytes"] = base64.b64encode(pixels).decode("ascii")
    return doc


def path(value):
    return {"DepotPath": {"$type": "ResourcePath", "$storage": "string", "$value": value}, "Flags": "Soft"} if value \
        else {"DepotPath": {"$type": "ResourcePath", "$storage": "uint64", "$value": "0"}, "Flags": "Soft"}


def atlas(doc, w, h, parts):
    """The main menu's atlas with slot 0 on the sheet and a part per icon; the other slots empty."""
    slots = doc["Data"]["RootChunk"]["slots"]["Elements"]
    slots[0]["texture"] = path(DEPOT + NAME + ".xbm")
    slots[0]["parts"] = [{"$type": "inkTextureAtlasMapper",
                          "clippingRectInPixels": {"$type": "Rect", "bottom": 0, "left": 0, "right": 0, "top": 0},
                          "clippingRectInUVCoords": {"$type": "RectF", "Bottom": (p["y"] + p["h"]) / h,
                                                     "Left": p["x"] / w, "Right": (p["x"] + p["w"]) / w,
                                                     "Top": p["y"] / h},
                          "partName": {"$type": "CName", "$storage": "string", "$value": n}}
                         for n, p in parts.items()]
    for s in slots[1:]:
        s["texture"], s["parts"], s["slices"] = path(None), [], []
    return doc


def verify(cli, xbm, pixels, work):
    """The .xbm read back to JSON: its pixels must be the sheet's, byte for byte."""
    back = work / "verify"
    back.mkdir(exist_ok=True)
    run(cli, "convert", "serialize", xbm, "-o", back, "-v", "Minimal")
    doc = json.loads((back / (xbm.name + ".json")).read_text(encoding="utf-8-sig"))
    blob = doc["Data"]["RootChunk"]["renderTextureResource"]["renderResourceBlobPC"]["Data"]
    raw = base64.b64decode(blob["textureData"]["Bytes"])
    if raw != pixels:
        bad = next((i for i in range(min(len(raw), len(pixels))) if raw[i] != pixels[i]), min(len(raw), len(pixels)))
        raise SystemExit(f"{xbm.name}: its pixels differ from the sheet's from byte {bad} of {len(pixels)}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("kenney", type=pathlib.Path, help="the unzipped Kenney Input Prompts pack")
    ap.add_argument("--cli", type=pathlib.Path, required=True, help="WolvenKit's CLI (cp77tools / WolvenKit.CLI)")
    ap.add_argument("--game", type=pathlib.Path, required=True, help="the Cyberpunk 2077 folder")
    ap.add_argument("--pack", type=pathlib.Path, default=PACK, help="the pack folder the two files go into")
    a = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp)
        pixels, w, h, parts = sheet(a.kenney, work)
        xbm, ink = templates(a.cli, a.game, work)
        RAW.mkdir(parents=True, exist_ok=True)
        atlas_json = RAW / (NAME + ".inkatlas.json")
        atlas_json.write_text(json.dumps(atlas(ink, w, h, parts), ensure_ascii=False, indent=1) + "\n",
                              encoding="utf-8")
        src = work / "deserialize"
        src.mkdir()
        (src / (NAME + ".xbm.json")).write_text(json.dumps(texture(xbm, pixels, w, h)), encoding="utf-8")
        shutil.copyfile(atlas_json, src / atlas_json.name)
        out = a.pack / "vrport/ui"
        out.mkdir(parents=True, exist_ok=True)
        run(a.cli, "convert", "deserialize", src, "-o", out, "-v", "Minimal")
        verify(a.cli, out / (NAME + ".xbm"), pixels, work)
        print(f"{len(parts)} icons, {w} x {h} -> {out / (NAME + '.xbm')}, {out / (NAME + '.inkatlas')}")
        print("atlas JSON ->", atlas_json.relative_to(ROOT))


if __name__ == "__main__":
    sys.exit(main())
