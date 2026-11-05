"""mvcheck.py CAPTURE_DIR: are framegen's motion vectors and depth what the cameras say? For each eye of frames 1..2,
every depth texel is unprojected through its camera (reversed depth, the OpenXR fov window) and projected into the
same eye's camera of the frame before; that camera-only motion (UV, current -> previous) is compared with the engine
vector x motionScale. Static scenery should agree; the residual is object motion, jitter and errors."""
import json, sys
import numpy as np
from pathlib import Path
d = Path(sys.argv[1]); cap = json.loads((d / 'capture.json').read_text())
frames = cap['frames']
def load(fi, e, kind, shape, dtype, ch):
    a = np.fromfile(d / f'f{fi}_e{e}_{kind}.raw', dtype=dtype)
    w, h = shape
    return a[:w * h * ch].reshape(h, w, ch) if ch > 1 else a[:w * h].reshape(h, w)
def rays(cam, w, h):
    l, r, u, dn = [np.tan(x) for x in cam['fov']]
    xs = (np.arange(w) + 0.5) / w; ys = (np.arange(h) + 0.5) / h
    tx = l + (r - l) * xs[None, :]; ty = u - (u - dn) * ys[:, None]
    return np.broadcast_to(tx, (h, w)), np.broadcast_to(ty, (h, w)), (l, r, u, dn)
for fi in (1, 2):
    for e in (0, 1):
        cur, prev = frames[fi]['eyes'][e], frames[fi - 1]['eyes'][e]
        w, h = cur['depth'][0], cur['depth'][1]
        depth = load(fi, e, 'depth', (w, h), np.float32, 1)
        mv = load(fi, e, 'motion', (cur['motion'][0], cur['motion'][1]), np.float16, 2).astype(np.float32)
        n, f = cur['planes'][0], cur['planes'][1]
        if cur['inverted']: B, A = n * f / (f - n), -n / (f - n)
        else: B, A = -n * f / (f - n), f / (f - n)
        valid = depth > 1e-6
        z = np.where(valid, B / np.maximum(depth - A, 1e-12), 0)
        tx, ty, win = rays(cur, w, h)
        P = [np.array(cur['position'])[i] + z * (np.array(cur['forward'])[i] + tx * np.array(cur['right'])[i] + ty * np.array(cur['up'])[i]) for i in range(3)]
        rel = [P[i] - prev['position'][i] for i in range(3)]
        zf = sum(rel[i] * prev['forward'][i] for i in range(3))
        txp = sum(rel[i] * prev['right'][i] for i in range(3)) / np.maximum(zf, 1e-6)
        typ = sum(rel[i] * prev['up'][i] for i in range(3)) / np.maximum(zf, 1e-6)
        lp, rp, up, dp = [np.tan(x) for x in prev['fov']]
        u1 = (txp - lp) / (rp - lp); v1 = (up - typ) / (up - dp)
        u0 = (np.arange(w)[None, :] + 0.5) / w; v0 = (np.arange(h)[:, None] + 0.5) / h
        exp_u, exp_v = (u1 - u0), (v1 - v0)
        ms = cur['motionScale']
        eng_u, eng_v = mv[..., 0] * ms[0], mv[..., 1] * ms[1]
        # far, static-looking scenery (beyond 3 m: the hands and weapon are near and move with the head)
        m = valid & (z > 3.0) & (zf > 0.1)
        cam_moved = np.hypot(exp_u[m], exp_v[m]).mean() * w
        for name, su, sv in (('as is', 1, 1), ('negated', -1, -1)):
            du = (su * eng_u[m] - exp_u[m]) * w; dv = (sv * eng_v[m] - exp_v[m]) * h
            err = np.hypot(du, dv)
            print(f'frame {frames[fi]["frame"]} eye {e} [{name}]: camera motion {cam_moved:.2f} px; engine-vs-camera error '
                  f'median {np.median(err):.3f} px p90 {np.percentile(err, 90):.3f} (n {m.sum()}); mean bias u {du.mean():+.3f} v {dv.mean():+.3f} px')
        zero = np.hypot(eng_u[m] * w, eng_v[m] * h)
        print(f'      engine vector magnitude median {np.median(zero):.3f} px; depth valid {valid.mean():.3f}, z median {np.median(z[valid]):.1f} m')
