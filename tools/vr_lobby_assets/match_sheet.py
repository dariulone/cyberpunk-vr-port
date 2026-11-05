"""Side by side: the reference panorama's views and a render's, the same directions.

    py -3.13 tools/vr_lobby_assets/match_sheet.py RENDER_PANORAMA OUT_PREFIX
Writes OUT_PREFIX_horizon.png and OUT_PREFIX_updown.png (reference left, render right, per direction).
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pano_views  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
REF = ROOT / 'art/vr_lobby/references/panorama_v2.png'
SETS = {'horizon': [(-90, 0), (-45, 0), (0, 0), (45, 0), (90, 0), (135, 0), (180, 0), (-135, 0)],
        'updown': [(0, 35), (90, 35), (180, 35), (-90, 35), (0, 70), (-60, -35), (60, -35), (0, -45)]}


def main():
    render, prefix = sys.argv[1], sys.argv[2]
    ref = np.asarray(Image.open(REF).convert('RGB'))
    ren = np.asarray(Image.open(render).convert('RGB'))
    W, H = 400, 300
    for name, views in SETS.items():
        sheet = Image.new('RGB', (W * 4, H * 4), (0, 0, 0))
        for i, (yaw, pitch) in enumerate(views):
            col, row = i % 4, (i // 4) * 2
            for k, src in enumerate((ref, ren)):
                im = pano_views.view(src, yaw, pitch, 90.0, W, H)
                ImageDraw.Draw(im).text((6, 4), '%s y%+d p%+d' % ('REF' if k == 0 else 'NEW', yaw, pitch),
                                        fill=(255, 255, 0))
                sheet.paste(im, (col * W, (row + k) * H))
        sheet.save('%s_%s.png' % (prefix, name))
    print('sheets ->', prefix)


if __name__ == '__main__':
    main()
