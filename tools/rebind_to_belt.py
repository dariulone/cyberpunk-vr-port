# -*- coding: utf-8 -*-
"""Put a rigid prop mesh on the player's belt by re-binding its single bone to the hips.

WHY. The belt's grenade holder is the NCPD frag holder and the game ships nothing else -- three
meshes, all frag -- so the belt cannot show the grenade that is actually equipped. The port already
has every grenade's own mesh, but those are authored around the origin and skinned to one bone named
`base`, which does not exist on the player, so dropping one on the belt shows nothing.

THE TRICK, and it is the same one the transform bake uses: for a mesh whose every vertex is weighted
to a single bone, WHICH bone that is and WHERE it sits are entirely in the header. Rename the bone to
`Hips` and give it the inverse bind matrix the belt's own holder uses, composed with a placement, and
the geometry rides the hips exactly like a garment -- no vertices touched, no reweighting, no export
through glTF.

    v_world = v_mesh · B · M_Hips        B := T_place · B_hips_of_the_holder

`B_hips_of_the_holder` comes from the pristine caibro holder, so the grenade inherits a binding that
is known to work on this rig. `T_place` moves the mesh from its own bbox centre to where the holder
ended up after tuning.

A mesh with extra bones (`lock_01`, `lock_02` on the recon and cutting grenades) is refused rather
than guessed at: those vertices reference bones 1 and 2, and silently pointing them at the hips would
misplace whatever is weighted to them.

MULTI-BONE MESHES are accepted with --all-to-hips, which points every bone at the hips with the same
matrix. That is right only when the extra bones are rigid parts of the same object -- the `lock_01` /
`lock_02` of the recon and cutting grenades are the clasp, which does not move on a belt -- and it is
opt-in for exactly that reason: pointing an articulated bone at the hips silently misplaces whatever
hangs off it.

Usage:
    python tools/rebind_to_belt.py <prop.mesh.json> <holder.mesh.json> tx ty tz rx ry rz [more 6...]
                                   [--all-to-hips]

Extra groups of six are further passes, composed LEFT TO RIGHT after the placement -- a tuning pass
from the panel goes on the end. Order matters, so a re-bake replays the whole chain from the pristine
mesh rather than stacking onto the last result.

--centre x y z replaces the mesh's own bbox centre: an object drawn as several meshes (a revolver's
speedloader, six rounds and a handle) is placed by the centre of all of them, each part re-bound with the
same matrix, so the parts stay assembled instead of each landing on the anchor by itself.
"""
import io
import json
import math
import sys

V = "$value"


def euler_matrix(rx, ry, rz):
    hr, hp, hy = math.radians(rx), math.radians(ry), math.radians(rz)
    cr, sr = math.cos(hr), math.sin(hr)
    cp, sp = math.cos(hp), math.sin(hp)
    cy, sy = math.cos(hy), math.sin(hy)
    a = [[1, 0, 0], [0, cr, sr], [0, -sr, cr]]
    b = [[cp, 0, -sp], [0, 1, 0], [sp, 0, cp]]
    c = [[cy, sy, 0], [-sy, cy, 0], [0, 0, 1]]

    def m3(u, v):
        return [[sum(u[i][k] * v[k][j] for k in range(3)) for j in range(3)] for i in range(3)]

    return m3(m3(a, b), c)


def mat4(r3, t):
    return [r3[0] + [0.0], r3[1] + [0.0], r3[2] + [0.0], list(t) + [1.0]]


def mul4(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def from_json(m):
    def row(k, w):
        v = m[k]
        return [v["X"], v["Y"], v["Z"], w]
    return [row("X", 0.0), row("Y", 0.0), row("Z", 0.0), row("W", 1.0)]


def to_json(m, dst):
    for i, k in enumerate(("X", "Y", "Z", "W")):
        dst[k]["X"], dst[k]["Y"], dst[k]["Z"] = m[i][0], m[i][1], m[i][2]
        dst[k]["W"] = 0.0 if k != "W" else 1.0


def bones_of(root):
    return [(b.get(V) if isinstance(b, dict) else b) for b in (root.get("boneNames") or [])]


def centre(root):
    bb = root["boundingBox"]
    return [(bb["Min"][k] + bb["Max"][k]) / 2.0 for k in ("X", "Y", "Z")]


def main():
    argv = [a for a in sys.argv[1:] if a != "--all-to-hips"]
    all_to_hips = len(argv) != len(sys.argv) - 1
    shared = None
    if "--centre" in argv:
        i = argv.index("--centre")
        shared = [float(v) for v in argv[i + 1:i + 4]]
        del argv[i:i + 4]
    if len(argv) < 8 or (len(argv) - 2) % 6 != 0:
        print(__doc__)
        return 2
    prop_path, holder_path = argv[0], argv[1]
    vals = [float(v) for v in argv[2:]]

    prop = json.load(io.open(prop_path, encoding="utf-8"))
    proot = prop["Data"]["RootChunk"]
    names = bones_of(proot)
    if len(names) != 1 and not all_to_hips:
        print("у меша %d костей (%s) -- нужен --all-to-hips, если это жёсткие части одной детали"
              % (len(names), ", ".join(names)))
        return 1

    holder = json.load(io.open(holder_path, encoding="utf-8"))
    hroot = holder["Data"]["RootChunk"]
    hnames = bones_of(hroot)
    if "Hips" not in hnames:
        print("в эталоне нет кости Hips")
        return 1
    hips = hroot["boneRigMatrices"][hnames.index("Hips")]
    B = from_json(hips.get("Data", hips))

    c = shared if shared is not None else centre(proot)
    # the mesh's (or its object's) centre to the origin, then every pass in order
    I3 = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
    T = mat4(I3, (-c[0], -c[1], -c[2]))
    for i in range(0, len(vals), 6):
        x, y, z, rx, ry, rz = vals[i:i + 6]
        T = mul4(T, mul4(mat4(euler_matrix(rx, ry, rz), (0.0, 0.0, 0.0)), mat4(I3, (x, y, z))))
        print("  проход %d: pos %.4f %.4f %.4f  deg %.2f %.2f %.2f"
              % (i // 6 + 1, x, y, z, rx, ry, rz))

    hips_name = {"$type": "CName", "$storage": "string", "$value": "Hips"}
    keep = len(proot["boneRigMatrices"]) if all_to_hips else 1
    proot["boneNames"] = [dict(hips_name) for _ in range(keep)]
    for i in range(keep):
        src = proot["boneRigMatrices"][i]
        to_json(mul4(T, B), src.get("Data", src))
    proot["boneRigMatrices"] = proot["boneRigMatrices"][:keep]
    if proot.get("boneVertexEpsilons"):
        proot["boneVertexEpsilons"] = proot["boneVertexEpsilons"][:keep]

    bb = proot["boundingBox"]
    lo, hi = [1e30] * 3, [-1e30] * 3
    mn = [bb["Min"][k] for k in ("X", "Y", "Z")]
    mx = [bb["Max"][k] for k in ("X", "Y", "Z")]
    for i in range(8):
        p = [mn[0] if i & 1 else mx[0], mn[1] if i & 2 else mx[1], mn[2] if i & 4 else mx[2]]
        q = [sum(p[k] * T[k][j] for k in range(3)) + T[3][j] for j in range(3)]
        for j in range(3):
            lo[j], hi[j] = min(lo[j], q[j]), max(hi[j], q[j])
    for j, k in enumerate(("X", "Y", "Z")):
        bb["Min"][k], bb["Max"][k] = lo[j], hi[j]

    io.open(prop_path, "w", encoding="utf-8", newline="").write(json.dumps(prop, indent=2))
    print("кость %s -> Hips,  центр (%.4f, %.4f, %.4f) -> (%.4f, %.4f, %.4f)"
          % (names[0], c[0], c[1], c[2],
             (lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
