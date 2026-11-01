"""Perspective views out of an equirectangular 360 panorama, for comparing a Blender scene with its reference.

    py -3.13 tools/vr_lobby_assets/pano_views.py PANORAMA OUT_DIR [--fov 90] [--size 960x720]

Yaw is clockwise from the panorama's centre column (the scene's +Y), pitch up from the horizon. Writes
OUT_DIR/view_y{yaw}_p{pitch}.png for the standard set of directions.
"""
import argparse
import math
from pathlib import Path

import numpy as np
from PIL import Image

VIEWS = [(yaw, 0) for yaw in range(-180, 180, 45)] + [(0, 35), (90, 35), (-90, 35), (180, 35), (0, 70),
                                                       (-60, -35), (60, -35), (0, -45)]


def view(pano, yaw, pitch, fov, width, height):
    h, w = pano.shape[:2]
    f = (width / 2) / math.tan(math.radians(fov) / 2)
    xs = np.arange(width) - (width - 1) / 2
    ys = (height - 1) / 2 - np.arange(height)
    x, y = np.meshgrid(xs, ys)
    # Camera frame: +Z forward, +X right, +Y up; pitch about X, then yaw about the vertical.
    d = np.stack([x, y, np.full_like(x, f)], -1)
    d /= np.linalg.norm(d, axis=-1, keepdims=True)
    cp, sp = math.cos(math.radians(pitch)), math.sin(math.radians(pitch))
    dy = d[..., 1] * cp + d[..., 2] * sp
    dz = -d[..., 1] * sp + d[..., 2] * cp
    cy, sy = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
    dx = d[..., 0] * cy + dz * sy
    dz = -d[..., 0] * sy + dz * cy
    lon = np.arctan2(dx, dz)
    lat = np.arcsin(np.clip(dy, -1, 1))
    u = (lon / (2 * math.pi) + 0.5) * w - 0.5
    v = (0.5 - lat / math.pi) * h - 0.5
    u0 = np.floor(u).astype(int); v0 = np.clip(np.floor(v).astype(int), 0, h - 2)
    fu = (u - u0)[..., None]; fv = np.clip(v - v0, 0, 1)[..., None]
    u1 = (u0 + 1) % w; u0 %= w
    p = pano.astype(np.float32)
    out = (p[v0, u0] * (1 - fu) * (1 - fv) + p[v0, u1] * fu * (1 - fv) + p[v0 + 1, u0] * (1 - fu) * fv
           + p[v0 + 1, u1] * fu * fv)
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('panorama'); ap.add_argument('out')
    ap.add_argument('--fov', type=float, default=90.0); ap.add_argument('--size', default='960x720')
    a = ap.parse_args()
    width, height = (int(v) for v in a.size.split('x'))
    pano = np.asarray(Image.open(a.panorama).convert('RGB'))
    out = Path(a.out); out.mkdir(parents=True, exist_ok=True)
    for yaw, pitch in VIEWS:
        view(pano, yaw, pitch, a.fov, width, height).save(out / ('view_y%+04d_p%+03d.png' % (yaw, pitch)))
    print('views', len(VIEWS), '->', out)


if __name__ == '__main__':
    main()
