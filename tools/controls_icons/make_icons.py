"""Build the control-guide icons (docs/controls/icons/*.svg) from Kenney's Input Prompts pack.

    python tools/controls_icons/make_icons.py PATH/TO/kenney_input-prompts_1.5 [--preview PREVIEW.png]

PATH is the unzipped pack (https://kenney.nl/assets/input-prompts, CC0). Each icon is a Kenney glyph -- or one of the
VR gestures drawn below in the same style -- in white on a dark rounded tile, so it reads on GitHub's light and dark
themes alike and on the port's dark VR panels. --preview also renders a labelled contact sheet (needs rsvg-convert and
ImageMagick).
"""
import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "docs" / "controls" / "icons"
TILE = "#2b2d33"
W = "#FFFFFF"

# Output name -> (pack folder, file in its Vector folder).
KENNEY = {
    "a": ("Meta Quest", "quest_button_a"),
    "b": ("Meta Quest", "quest_button_b"),
    "x": ("Meta Quest", "quest_button_x"),
    "y": ("Meta Quest", "quest_button_y"),
    "menu": ("Meta Quest", "quest_button_menu"),
    "lt": ("Meta Quest", "quest_trigger_left"),
    "rt": ("Meta Quest", "quest_trigger_right"),
    "lg": ("Meta Quest", "quest_grip_left"),
    "rg": ("Meta Quest", "quest_grip_right"),
    "ls": ("Meta Quest", "quest_stick_l"),
    "ls_up": ("Meta Quest", "quest_stick_l_up"),
    "ls_down": ("Meta Quest", "quest_stick_l_down"),
    "ls_vertical": ("Meta Quest", "quest_stick_l_vertical"),
    "ls_press": ("Meta Quest", "quest_stick_l_press"),
    "rs": ("Meta Quest", "quest_stick_r"),
    "rs_up": ("Meta Quest", "quest_stick_r_up"),
    "rs_down": ("Meta Quest", "quest_stick_r_down"),
    "rs_left": ("Meta Quest", "quest_stick_r_left"),
    "rs_right": ("Meta Quest", "quest_stick_r_right"),
    "rs_horizontal": ("Meta Quest", "quest_stick_r_horizontal"),
    "rs_vertical": ("Meta Quest", "quest_stick_r_vertical"),
    "rs_press": ("Meta Quest", "quest_stick_r_press"),
    "key_f7": ("Keyboard & Mouse", "keyboard_f7"),
    "key_f8": ("Keyboard & Mouse", "keyboard_f8"),
    "key_f10": ("Keyboard & Mouse", "keyboard_f10"),
    "key_insert": ("Keyboard & Mouse", "keyboard_insert"),
    "key_escape": ("Keyboard & Mouse", "keyboard_escape"),
    "key_alt": ("Keyboard & Mouse", "keyboard_alt"),
    "key_enter": ("Keyboard & Mouse", "keyboard_enter"),
    "key_shift": ("Keyboard & Mouse", "keyboard_shift"),
}

# Kenney glyphs reused inside the drawn gestures.
PARTS = {
    "hand_open": ("Touch", "touch_hand_open"),
    "hand_closed": ("Touch", "touch_hand_closed"),
}

STROKE = f'fill="none" stroke="{W}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"'


def arrow_head(x, y, dx, dy, size=7):
    """A filled arrowhead at (x, y) pointing along (dx, dy)."""
    n = (dx * dx + dy * dy) ** 0.5
    ux, uy = dx / n, dy / n
    px, py = -uy, ux
    bx, by = x - ux * size, y - uy * size
    return (f'<path fill="{W}" d="M{x:.1f} {y:.1f} L{bx + px * size * 0.8:.1f} {by + py * size * 0.8:.1f} '
            f'L{bx - px * size * 0.8:.1f} {by - py * size * 0.8:.1f} Z"/>')


def part(parts, name, x, y, scale):
    """A Kenney glyph from PARTS, scaled and moved."""
    return f'<g transform="translate({x} {y}) scale({scale})">{parts[name]}</g>'


def gestures(parts):
    """The VR-only inputs, drawn on the same 64-unit grid as Kenney's glyphs."""
    wheel = (f'<circle cx="32" cy="32" r="20" {STROKE}/>'
             f'<circle cx="32" cy="32" r="5" fill="{W}"/>'
             f'<path {STROKE} d="M14 34 H27 M37 34 H50 M32 37 V51"/>')
    return {
        # Left hand brought up to the left ear: a head and shoulders, the open hand down at the side, and the curve it
        # takes up to the ear.
        "ear_left": (f'<circle cx="43" cy="24" r="10" fill="{W}"/>'
                     f'<path fill="{W}" d="M27 57 Q28 41 43 41 Q58 41 59 57 Z"/>'
                     + part(parts, "hand_open", -9, 24, 0.55)
                     + f'<path {STROKE} d="M12 32 Q10 14 25 14"/>' + arrow_head(32, 17, 1, 0.45)),
        # Held: a stopwatch.
        "hold": (f'<circle cx="32" cy="35" r="18" {STROKE}/>'
                 f'<path {STROKE} d="M32 35 V25 M28 11 H36 M32 11 V17 M46 19 L49 16"/>'),
        # Squat in the room: a crouched figure and a down arrow.
        "squat": (f'<circle cx="26" cy="14" r="6" fill="{W}"/>'
                  f'<path {STROKE} d="M25 22 L23 36 L35 40 L29 54 M24 26 L36 32"/>'
                  f'<path {STROKE} d="M48 14 V40"/>' + arrow_head(48, 48, 0, 1)),
        # Swing a melee weapon: a blade and its arc.
        "swing": (f'<path {STROKE} d="M20 48 L44 18 M16 40 L28 52"/>'
                  f'<path {STROKE} d="M10 26 Q14 10 32 8"/>' + arrow_head(40, 8, 1, 0)),
        # Reach for a holster: a pistol grip down at the hip, the hand coming to it.
        "holster": (f'<rect x="18" y="34" width="38" height="9" rx="2" fill="{W}"/>'
                    f'<path fill="{W}" d="M19 41 H31 L28 59 H15 Z"/>'
                    f'<path fill="none" stroke="{W}" stroke-width="3.5" stroke-linecap="round" '
                    f'd="M31 43 Q32 51 40 47"/>'
                    + part(parts, "hand_closed", -2, -10, 0.55)
                    + f'<path {STROKE} d="M24 20 V25"/>' + arrow_head(24, 31, 0, 1)),
        # Throw: an opening hand and the direction of the throw.
        "throw": (part(parts, "hand_open", -6, 8, 0.75)
                  + f'<path {STROKE} d="M34 18 H52"/>' + arrow_head(60, 18, 1, 0)),
        # Grab the wheel or the handlebars.
        "wheel_grab": wheel + part(parts, "hand_closed", 20, -6, 0.6),
        # Steer: tilt the line through both controllers.
        "steer": (wheel.replace('cy="32"', 'cy="36"').replace("M14 34", "M14 38").replace("M37 34", "M37 38")
                  .replace("M32 37 V51", "M32 41 V55")
                  + f'<path {STROKE} d="M12 16 Q32 2 52 16"/>' + arrow_head(57, 21, 0.7, 0.7)
                  + arrow_head(7, 21, -0.7, 0.7)),
        # Horn: a hand on the middle of the wheel.
        "horn": (f'<circle cx="28" cy="36" r="18" {STROKE}/><circle cx="28" cy="36" r="7" fill="{W}"/>'
                 f'<path {STROKE} d="M50 26 Q54 32 50 38 M56 20 Q63 32 56 44"/>'),
    }


def inner(svg_text):
    """A Kenney SVG's drawing, without its <svg> element and empty <defs/>."""
    m = re.search(r"<svg[^>]*>(.*)</svg>", svg_text, re.S)
    if not m:
        raise ValueError("not an SVG")
    return re.sub(r"<defs\s*/>", "", m.group(1)).strip()


def tile(drawing, title):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64" role="img">'
            f"<title>{title}</title>"
            f'<rect width="64" height="64" rx="14" fill="{TILE}"/>'
            f'<g transform="translate(3.2 3.2) scale(0.9)">{drawing}</g></svg>\n')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pack", type=pathlib.Path)
    ap.add_argument("--preview", type=pathlib.Path)
    a = ap.parse_args()

    def load(folder, name):
        return inner((a.pack / folder / "Vector" / f"{name}.svg").read_text())

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.svg"):
        old.unlink()
    icons = {name: load(*src) for name, src in KENNEY.items()}
    icons.update(gestures({name: load(*src) for name, src in PARTS.items()}))
    for name, drawing in icons.items():
        (OUT / f"{name}.svg").write_text(tile(drawing, name.replace("_", " ")))
    shutil.copyfile(a.pack / "License.txt", OUT / "LICENSE-kenney.txt")
    print(f"{len(icons)} icons -> {OUT.relative_to(ROOT)}")

    if a.preview:
        with tempfile.TemporaryDirectory() as tmp:
            pngs = []
            for name in icons:
                png = pathlib.Path(tmp) / f"{name}.png"
                subprocess.run(["rsvg-convert", "-w", "96", "-h", "96", str(OUT / f"{name}.svg"), "-o", str(png)],
                               check=True)
                pngs.append(str(png))
            subprocess.run(["magick", "montage", "-background", "#f6f8fa", "-label", "%t", "-pointsize", "11",
                            *pngs, "-tile", "8x", "-geometry", "96x96+8+16", str(a.preview)], check=True)
        print(f"preview -> {a.preview}")


if __name__ == "__main__":
    sys.exit(main())
