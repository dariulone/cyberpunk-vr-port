"""Generate the quick reference's standalone controller diagrams and offline index."""
import argparse
import base64
from dataclasses import dataclass
from html import escape
from io import BytesIO
from pathlib import Path
from textwrap import wrap

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
CONTROLS = ("Stick", "Trigger", "Grip", "Menu", "Black wrist", "Pink wrist")
# Touch points in the original driver illustrations, from docs/manual/manual.html.
TOUCH_POINTS = (
    ((440, 260), (646, 318), (570, 472), (482, 466), (568, 1058), (564, 1238)),
    ((780, 191), (621, 252), (671, 381), (744, 383), (698, 978), (695, 1131)),
)
BADGES = (
    ((-115, 35), (115, 44), (115, 91), (-115, 86), (115, 161), (-115, 194)),
    ((115, 28), (-115, 39), (-115, 81), (115, 76), (115, 155), (-115, 185)),
)

# Pairs are always (physical left, physical right). A plain pair applies to both
# presets. For a mixed row, print the shared actions once before the differences.
@dataclass(frozen=True)
class Bindings:
    shared: tuple = (None, None)
    right_weapon: tuple = (None, None)
    left_weapon: tuple = (None, None)

    def bands(self):
        return (('shared', self.shared), ('rh', self.right_weapon), ('lh', self.left_weapon))


def by_weapon_hand(weapon, other=None, shared=(None, None)):
    return Bindings(shared=shared, right_weapon=(other, weapon), left_weapon=(weapon, other))


walk = ("Move or strafe. Full forward to sprint. Click to jump.",
        "Sideways to turn. Forward to dash, back to crouch. Click to jump, hold to charge.")
menus = ("Tap for next weapon. Hold for weapon wheel.",
         "Tap for inventory. Hold for OS cyberware.")
wrist_holds = ("Hold to pause", "Hold to heal")
wrists = by_weapon_hand("Tap to eject magazine", "Tap to quick reload", shared=wrist_holds)
pink = ("SteamVR dashboard", "VR menu")
back = ("Back or close", "Back or close")
select = ("Click laser target or confirm highlight",) * 2
starting_laser = "Laser starts here"
secondary = ("Secondary action (X)",) * 2
steer = ("Sideways to steer. Hold click for D-pad.", "Forward for gas, back for brake/reverse. Click for handbrake.")
exit_seat = "Hold 0.4 s to exit the vehicle"
driving_note = "Close your fingers near the steering wheel to steer by hand. In dialogue, highlight with right stick up/down. Press either Trigger or click the right stick to confirm."
focus = ("Navigate highlights", "Sideways to change tabs")
panel_select = ("Point and click",) * 2
hold_hud = "Hold both Menu buttons for half a second to toggle the HUD."

# Filename, title, controls in CONTROLS order, and the note printed on the card.
contexts = [
    ("cyberfinger-controls", "Normal · On foot", [
        walk, by_weapon_hand("Release alone to draw in Offensive mode", shared=(None, "Take or interact at a prompt")),
        ("Toggle scanner", "Eat or use at a secondary prompt"), menus,
        wrist_holds, pink,
    ], "Offensive mode: raise empty hands and tap both Triggers together or punch once with each hand for fists. Grip at a visible katana hilt draws it. " + hold_hud),
    ("cyberfinger-combat", "Combat · Firearm", [
        by_weapon_hand("Turn sideways. Forward to dash, back to crouch.",
                       "Move or strafe. Full forward to sprint.",
                       shared=("Click to jump", "Click to jump, hold to charge")),
        by_weapon_hand("Fire", "Aim"), ("Toggle scanner", "Tap to crouch. Hold for slide release or hammer."), menus, wrists, pink,
    ], "Strafe with the stick opposite the gun. Reach with your free hand to grab the magazine or chamber handle for a physical reload. Gun gesture holstering is off by default."),
    ("cyberfinger-melee", "Combat · Fists and melee", [
        walk, ("Hold before swinging for a strong attack",) * 2,
        Bindings(right_weapon=("Switch fists / equipped melee", None),
                 left_weapon=("Toggle scanner", "Switch fists / equipped melee")), menus,
        wrist_holds, pink,
    ], "Bare fists: hold both Triggers with both hands raised to block. Release either Trigger or lower a hand to stop. A single Trigger modifies a physical punch. See the katana card for sword grips and strength."),
    ("cyberfinger-katana", "Katana · Drawn or at the hip", [
        walk, (None, None),
        ("Tap at hilt: draw / brace. Tap again: stow / release.",) * 2, menus, wrist_holds, pink,
    ], "Tap Grip at the sheathed hilt to draw from Holstered or Fists. Sword-hand Grip puts it away. Other Grip at the lower hilt toggles two hands. One hand: fast swing. Two hands: strong swing. Default: right hand draws from left hip; left-hand swords mirror this."),
    ("cyberfinger-car", "Car · Driving", [
        steer, by_weapon_hand("Draw weapon", "Horn"), (exit_seat, "Cycle camera view"),
        ("Tap to draw weapon", "Tap for inventory. Hold for OS cyberware if available."), wrist_holds, pink,
    ], driving_note),
    ("cyberfinger-car-combat", "Car + combat · Gun drawn", [
        steer, by_weapon_hand("Fire drawn gun", "Aim"), (exit_seat, "Cycle camera view"),
        ("Tap to switch weapon", "Tap for inventory. Hold for OS cyberware if available."), wrists, pink,
    ], "Center the right stick to coast. Weapon-hand firing uses the vehicle firing option. During dialogue, highlight with right stick up/down and confirm with either Trigger or right stick click."),
    ("cyberfinger-dialogue", "Conversation choices · On foot", [
        ("Move where the scene allows", "Up/down to highlight. Sideways to turn."),
        ("Confirm highlighted choice",) * 2,
    ], "The HUD appears while dialogue choices are available. Either Trigger confirms. While driving, you can also click the right stick."),
    ("cyberfinger-scanner", "Scanner · Quickhacks", [
        ("Push fully up/down to page quickhacks", None), (None, "Apply selected quickhack"),
        ("Toggle scanner", None),
    ], "With the default left movement stick, use your right Trigger to confirm the hack."),
    ("cyberfinger-game-menus", "Cyberpunk menus · Esc, inventory and wardrobe", [
        ("Navigate highlights", "Up/down to scroll lists. Sideways to change tabs."), by_weapon_hand(starting_laser, shared=select), secondary, back,
    ], "Move the left stick to use highlights. Move your pointing hand to resume the laser. Hold Grip when the game asks you to hold X."),
    ("cyberfinger-map", "Map", [
        by_weapon_hand("Up/down to zoom", shared=(None, "Sideways to change tabs")),
        by_weapon_hand("Select at laser", "Jump to V"),
        by_weapon_hand("Track waypoint at laser", "Open journal"), back,
    ], "Point with your weapon hand. Zoom, select and track follow that hand. Jump to V and open journal follow the opposite hand."),
    ("cyberfinger-journal", "Journal", [
        focus, by_weapon_hand("Select at laser", "Change sort order"), secondary, back,
    ], "Aim with your weapon hand and pull its Trigger to select. The other Trigger activates the journal’s Tab shortcut."),
    ("cyberfinger-vr-menu", "VR menu", [
        ("Navigate highlights", "Sideways to change tabs. Up/down to scroll."),
        panel_select, ("Item or sort options",) * 2, back, (None, None), pink,
    ], "Tap the right pink wrist button to open or close the VR menu. You can also select by touching a button with your fingertip."),
    ("cyberfinger-f10", "F10 · VR settings", [
        ("Navigate highlights", "Sideways to change tabs. Up/down to scroll."),
        by_weapon_hand(starting_laser, shared=panel_select),
        ("Right-click. Hold title handle to move panel.",) * 2, back,
    ], "Press F10 on the keyboard to open settings. The laser starts on your weapon hand. Pull the other Trigger to transfer it."),
    ("cyberfinger-cet", "CET · Mod windows", [
        by_weapon_hand("Up/down to scroll", "Sideways to change tabs"),
        by_weapon_hand(starting_laser, shared=("Click or hold to drag",) * 2),
        ("Right-click. Hold top strip to move panel.",) * 2, back,
    ], "The weapon hand starts with the laser. If you transfer it, scrolling follows the laser hand and tab switching follows the other hand."),
    ("cyberfinger-computer", "Computer", [
        (None, "Up/down to scroll"), (None, "Aim at screen and click"),
        (None, "Go back within the computer"), ("Exit computer",) * 2,
    ], "The hand opposite the movement stick controls the cursor. You can also touch the screen with your fingertip."),
    ("cyberfinger-picture-frame", "Apartment picture frame", [
        ("Navigate photo highlights", None), by_weapon_hand(starting_laser, shared=select),
        ("Secondary action or remove photo",) * 2, ("Close or cancel",) * 2,
    ], "Point at a photo and pull that hand’s Trigger to select it, or move the left stick and confirm the highlighted photo."),
]


def controller_art(driver, side):
    path = driver / 'resources' / 'icons' / f'steamvr_cybrfngr_{side}_transp.png'
    with Image.open(path) as source:
        artwork = source.convert('RGBA')
    original_size = artwork.size
    artwork.thumbnail((360, 395), Image.Resampling.LANCZOS)
    buffer = BytesIO()
    artwork.quantize(128, method=Image.Quantize.FASTOCTREE).save(buffer, format='PNG', optimize=True)
    return original_size, base64.b64encode(buffer.getvalue()).decode('ascii')


def text(x, y, value, style, align='start'):
    return f'<text x="{x}" y="{y}" class="{style}" text-anchor="{align}">{escape(value)}</text>'


# Native menus and dialogue own their stick input.
foot_dpad = ("Holster all / toggle Offensive mode", "Phone / notification", "Pocket radio", "Call vehicle")
car_dpad = ("Use healing item", "Phone / notification", "Cycle lights", "Cycle camera view")
dpad_context = {
    "cyberfinger-controls": foot_dpad, "cyberfinger-combat": foot_dpad, "cyberfinger-katana": foot_dpad,
    "cyberfinger-melee": foot_dpad, "cyberfinger-car": car_dpad, "cyberfinger-car-combat": car_dpad,
}


def dpad_key(x, y, angle):
    # Four-way gamepad silhouette with the requested direction lit.
    cross = "M-6 -17H6V-6H17V6H6V17H-6V6H-17V-6H-6Z"
    return (f'<g transform="translate({x} {y})"><path d="{cross}" fill="#293943" stroke="#cad6da"/>'
            f'<path d="M-5 -16H5V-7L0 -2L-5 -7Z" transform="rotate({angle})" fill="white"/></g>')


def diagram(stem, title, rows, note, art):
    rows = [row if isinstance(row, Bindings) else Bindings(shared=row) for row in rows]
    parts = [text(32, 35, 'CyberFinger controls', 'caption'), text(32, 83, title, 'title'),
             text(32, 125, 'Shared', 'legend shared'),
             text(200, 125, 'RH · Right-handed weapon', 'legend rh'),
             text(549, 125, 'LH · Left-handed weapon', 'legend lh')]
    for side, (center, hand) in enumerate(((226, 'Left hand'), (734, 'Right hand'))):
        (width, height), png = art[side]
        scale = .15
        top = 193
        left = center - width * scale / 2
        parts += [text(center, 170, hand, 'hand', 'middle'),
                  f'<image x="{left}" y="{top}" width="{width * scale}" height="{height * scale}" xlink:href="data:image/png;base64,{png}"/>']
        for number, actions in enumerate(rows):
            if not any(pair[side] for mode, pair in actions.bands()):
                continue
            tx, ty = TOUCH_POINTS[side][number]
            dx, dy = BADGES[side][number]
            x, y = left + tx * scale, top + ty * scale
            bx, by = center + dx, top + dy
            parts += [f'<path d="M{x:.1f} {y:.1f}L{bx} {by}" stroke="#cad6da" stroke-width="2"/>',
                      f'<circle cx="{x:.1f}" cy="{y:.1f}" r="3" fill="#cad6da"/>',
                      f'<circle cx="{bx}" cy="{by}" r="12" fill="#cad6da"/>',
                      text(bx, by + 5, str(number + 1), 'marker', 'middle')]

    y = 411
    mode_names = {'shared': 'Shared', 'rh': 'Right-handed weapon', 'lh': 'Left-handed weapon'}
    description = []
    for number, row in enumerate(rows, start=1):
        start = y
        for mode, pair in row.bands():
            if not any(pair):
                continue
            y += 10
            prefix = '' if mode == 'shared' else mode.upper() + ' · '
            line_count = 0
            for action, hand, x, align in zip(pair, ('Left', 'Right'), (388, 572), ('end', 'start')):
                if not action:
                    continue
                lines = wrap(prefix + action, width=33)
                for i, line in enumerate(lines):
                    parts.append(text(x, y + 21 + i * 26, line, 'action ' + mode, align))
                line_count = max(line_count, len(lines))
                description.append(f'{mode_names[mode]}. {hand} {CONTROLS[number - 1]}: {action}.')
            y += line_count * 26
        if y > start:
            y += 10
            parts.append(f'<path d="M32 {start}h896" stroke="#33444e"/>')
            parts.append(text(480, (start + y) / 2 + 6, f'{number}  {CONTROLS[number - 1]}', 'control', 'middle'))
    parts.append(f'<path d="M32 {y}h896" stroke="#33444e"/>')
    for line in wrap(note, width=92):
        y += 28
        parts.append(text(32, y, line, 'note'))
    if stem in dpad_context:
        y += 42
        parts.append(text(32, y, 'D-pad · Hold left stick click, then push the right stick', 'legend shared'))
        for index, (direction, angle, action) in enumerate(zip(
                ('Up', 'Down', 'Left', 'Right'), (0, 180, 270, 90), dpad_context[stem])):
            x = 52 + (index % 2) * 445
            cy = y + 34 + (index // 2) * 44
            parts.extend((dpad_key(x, cy, angle), text(x + 32, cy + 7, direction + ' · ' + action, 'action shared')))
            description.append(f'D-pad {direction}: {action}.')
        y += 99
    height = int(y + 26)
    desc = ' '.join(description)
    return '\n'.join([f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="960" height="{height}" viewBox="0 0 960 {height}" role="img" aria-labelledby="title desc">',
                      f'<title id="title">{escape(title)} — CyberFinger controls</title>',
                      f'<desc id="desc">{escape(desc)} {escape(note)}</desc>',
                      '<style>text{font-family:Segoe UI,Arial,sans-serif;fill:white}.title{font-size:36px;font-weight:700}.caption,.note{font-size:18px}.caption{fill:#bed0d6}.hand{font-size:24px;font-weight:700}.marker{font-size:15px;fill:#101b20;font-weight:700}.control{font-size:18px;fill:#c4d7de;font-weight:700}.action{font-size:21px}.legend{font-size:20px;font-weight:700}.shared{fill:#fff}.rh{fill:#80d6ff}.lh{fill:#ff9dcc}</style>',
                      '<rect width="100%" height="100%" fill="#122028"/>', *parts, '</svg>'])


def write_guide(out, driver):
    art = [controller_art(driver, side) for side in ('left', 'right')]
    links = []
    for index, (stem, title, rows, note) in enumerate(contexts):
        filename = stem + '.svg'
        (out / filename).write_text(diagram(stem, title, rows, note, art), encoding='utf-8')
        current = ' aria-current="true"' if index == 0 else ''
        links.append(f'<a href="{filename}"{current}>{escape(title)}</a>')

    first, title, *_ = contexts[0]
    page = f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>CyberFinger controls — Cyberpunk VR</title>
<style>body{{margin:auto;max-width:960px;padding:16px;background:#122028;color:#f3f6f7;font:16px/1.5 system-ui}}nav{{display:flex;gap:8px;flex-wrap:wrap}}a{{color:#7ce6d0}}nav a{{padding:5px 10px;border:1px solid #52656c;border-radius:5px;text-decoration:none}}nav a[aria-current]{{background:#7ce6d0;color:#122028}}img{{display:block;width:100%;height:auto;margin:16px 0}}footer{{padding:12px 0}}</style>
</head><body><nav aria-label="Control context">{' '.join(links)}</nav>
<img src="{first}.svg" alt="{escape(title)} controller diagram">
<footer><a href="../../README_CYBRFNGR.md">Quick reference</a> · <a href="ARTWORK.md">Controller artwork</a></footer>
<script>
const links = document.querySelectorAll('nav a');
const diagram = document.querySelector('img');
for (const link of links) link.addEventListener('click', event => {{
  if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  for (const item of links) item.removeAttribute('aria-current');
  link.setAttribute('aria-current', 'true');
  diagram.src = link.href;
  diagram.alt = link.textContent + ' controller diagram';
}});
</script></body></html>'''
    (out / 'cyberfinger-controls.html').write_text(page, encoding='utf-8')
    (out / 'ARTWORK-LICENSE.txt').write_bytes((driver / 'LICENSE').read_bytes())


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, default=Path('D:/build/CyberFinger_SteamVRFusion'))
    args = parser.parse_args()
    write_guide(ROOT / 'docs' / 'controls', args.driver)
