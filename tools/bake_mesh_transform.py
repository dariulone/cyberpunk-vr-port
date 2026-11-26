# -*- coding: utf-8 -*-
"""Bake a tuned local transform into a SKINNED mesh, through its bone rig matrices.

THE PROBLEM THIS SOLVES. A belt element is tuned on the body with the panel's sliders, which write
`SetLocalPosition` / `SetLocalOrientation` on the component. That looks right while standing still and
is wrong in two ways the moment anything moves:

  * the offset is applied in a frame that does not follow the hips, so the piece drifts as the camera
    turns;
  * the same numbers written into the .ent's localTransform compose differently again, and put the
    holder inside the player.

So the tuned numbers are only meaningful as a transform in the MESH's own space, carried by the bones
like the geometry itself. That is what this does.

HOW. Skinning evaluates, with the row-vector convention REDengine stores matrices in (rows X, Y, Z are
the basis, row W is the translation):

    v_world = v_mesh · B_i · M_bone_i          B_i = boneRigMatrices[i], the inverse bind matrix

Applying a transform T to the geometry in mesh space is therefore not a change to the vertices at all:

    v_world = (v_mesh · T) · B_i · M_bone_i = v_mesh · (T · B_i) · M_bone_i

so every rig matrix becomes `T · B_i` and nothing else in the file has to move. The vertex buffer, the
normals and the tangents are untouched, which matters: they are quantized and packed, and rewriting
them is where this kind of edit usually goes wrong.

The bounding box is transformed too -- all eight corners through T, then a new min/max -- or the piece
gets culled while it is still on screen.

WHAT T IS. The panel's numbers, in the panel's own order: roll about X, pitch about Y, yaw about Z,
then the translation. Rotation happens about the mesh origin, exactly as it did at runtime, which is
why the tuned position and rotation have to be baked together and cannot be split.

Usage:
    python tools/bake_mesh_transform.py <mesh.json> x y z rx ry rz
"""
import io
import json
import math
import sys


def euler_matrix(rx, ry, rz):
    """Row-vector rotation matrix for roll-X, then pitch-Y, then yaw-Z."""
    hr, hp, hy = math.radians(rx), math.radians(ry), math.radians(rz)
    cr, sr = math.cos(hr), math.sin(hr)
    cp, sp = math.cos(hp), math.sin(hp)
    cy, sy = math.cos(hy), math.sin(hy)
    rx_m = [[1, 0, 0], [0, cr, sr], [0, -sr, cr]]
    ry_m = [[cp, 0, -sp], [0, 1, 0], [sp, 0, cp]]
    rz_m = [[cy, sy, 0], [-sy, cy, 0], [0, 0, 1]]

    def mul3(a, b):
        return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]

    return mul3(mul3(rx_m, ry_m), rz_m)


def mat4(rot3, t):
    return [rot3[0] + [0.0], rot3[1] + [0.0], rot3[2] + [0.0], list(t) + [1.0]]


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


def main():
    # One or more transforms, six numbers each, composed LEFT TO RIGHT: the first is applied to the
    # geometry first. Order matters because a rotation and a translation do not commute, and a second
    # tuning pass on top of a first is exactly that case -- so a re-bake starts from the pristine mesh
    # and replays every pass rather than stacking onto what is already there.
    if len(sys.argv) < 8 or (len(sys.argv) - 2) % 6 != 0:
        print(__doc__)
        return 2
    path = sys.argv[1]
    vals = [float(v) for v in sys.argv[2:]]

    T = mat4([[1.0, 0, 0], [0, 1.0, 0], [0, 0, 1.0]], (0.0, 0.0, 0.0))
    for i in range(0, len(vals), 6):
        x, y, z, rx, ry, rz = vals[i:i + 6]
        T = mul4(T, mat4(euler_matrix(rx, ry, rz), (x, y, z)))
        print("  проход %d: pos %.4f %.4f %.4f  deg %.2f %.2f %.2f"
              % (i // 6 + 1, x, y, z, rx, ry, rz))

    d = json.load(io.open(path, encoding="utf-8"))
    root = d["Data"]["RootChunk"]

    mats = root.get("boneRigMatrices") or []
    for m in mats:
        md = m.get("Data", m)
        B = from_json(md)
        to_json(mul4(T, B), md)
    print("матриц привязки пересчитано: %d" % len(mats))

    bb = root["boundingBox"]
    mn = [bb["Min"][k] for k in ("X", "Y", "Z")]
    mx = [bb["Max"][k] for k in ("X", "Y", "Z")]
    lo = [1e30] * 3
    hi = [-1e30] * 3
    for i in range(8):
        p = [mn[0] if i & 1 else mx[0], mn[1] if i & 2 else mx[1], mn[2] if i & 4 else mx[2]]
        q = [sum(p[k] * T[k][j] for k in range(3)) + T[3][j] for j in range(3)]
        for j in range(3):
            lo[j] = min(lo[j], q[j])
            hi[j] = max(hi[j], q[j])
    for j, k in enumerate(("X", "Y", "Z")):
        bb["Min"][k], bb["Max"][k] = lo[j], hi[j]
    print("bbox    (%.4f,%.4f,%.4f) .. (%.4f,%.4f,%.4f)" % (lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]))
    print("центр   (%.4f,%.4f,%.4f)" % tuple((lo[j] + hi[j]) / 2 for j in range(3)))

    io.open(path, "w", encoding="utf-8", newline="").write(json.dumps(d, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
