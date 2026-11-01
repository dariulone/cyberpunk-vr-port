"""The lobby in the running game, seen in the same directions as the reference panorama (match_sheet.py's sets):
turns the OpenXR simulator's head, takes the left eye, and lays the shots beside the reference's views.

    py -3.13 tools/vr_lobby_assets/game_views.py TAG [--settle SECONDS]
Writes art/vr_lobby/match/game_TAG_horizon.png and game_TAG_updown.png; the head is put back level at the end.

The simulator takes head yaw/pitch in radians, positive yaw turning left (the views' yaw turns right).
"""
import json
import math
import os
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import match_sheet  # noqa: E402
import pano_views  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
SIM = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'
EYE_ASPECT = 2864 / 2648          # the simulator's eye render target, height / width
EYE_FOV = 100.0                   # horizontal degrees the left eye's crop covers, roughly


def sim_send(filename, value):
    path = SIM / filename
    for _ in range(100):
        if not path.exists():
            break
        time.sleep(0.05)
    tmp = SIM / (filename + '.tmp')
    tmp.write_text(json.dumps(value), encoding='utf-8')
    for _ in range(50):
        try:
            os.replace(tmp, path)
            return
        except PermissionError:
            time.sleep(0.02)
    raise RuntimeError('simulator did not take ' + filename)


def head(yaw_deg, pitch_deg):
    sim_send('head_pose_command.json', {'x': 0.0, 'y': 1.7, 'z': 0.0, 'yaw': -math.radians(yaw_deg),
                                        'pitch': math.radians(pitch_deg), 'roll': 0.0})


def shot():
    t = time.time()
    (SIM / 'screenshot_request.json').write_text('{"eye":"left","layer":"all"}')
    for _ in range(150):
        s = SIM / 'screenshot_status.json'
        if s.exists() and s.stat().st_mtime >= t:
            break
        time.sleep(0.1)
    im = Image.open(SIM / 'screenshot.bmp').convert('RGB')
    if im.width > im.height * 1.2:     # both eyes side by side
        im = im.crop((0, 0, im.width // 2, im.height))
    return im.resize((im.width, round(im.width * EYE_ASPECT)))


def main():
    tag = sys.argv[1]
    settle = float(sys.argv[sys.argv.index('--settle') + 1]) if '--settle' in sys.argv else 1.5
    ref = np.asarray(Image.open(match_sheet.REF).convert('RGB'))
    W = 360
    H = round(W * EYE_ASPECT)
    try:
        for name, views in match_sheet.SETS.items():
            sheet = Image.new('RGB', (W * 4, H * 4), (0, 0, 0))
            for i, (yaw, pitch) in enumerate(views):
                head(yaw, pitch)
                time.sleep(settle)
                game = shot().resize((W, H))
                r = pano_views.view(ref, yaw, pitch, EYE_FOV, W, H)
                col, row = i % 4, (i // 4) * 2
                for k, im in enumerate((r, game)):
                    ImageDraw.Draw(im).text((6, 4), '%s y%+d p%+d' % ('REF' if k == 0 else 'GAME', yaw, pitch),
                                            fill=(255, 255, 0))
                    sheet.paste(im, (col * W, (row + k) * H))
            out = ROOT / 'art/vr_lobby/match' / ('game_%s_%s.png' % (tag, name))
            sheet.save(out)
            print('->', out)
    finally:
        head(0, 0)


if __name__ == '__main__':
    main()
