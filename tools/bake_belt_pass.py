# -*- coding: utf-8 -*-
"""APPEND a tuning pass to a belt element that is already baked, without replaying its chain by hand.

WHY THIS EXISTS AND WHY `bake_mesh_transform` ALONE IS WRONG HERE.

A belt element's rig matrix is the whole chain composed:

    B = T_chain · B_hips            B_hips := the rig's Hips inverse bind matrix
                                    T_chain := the placement plus every pass tuned since

`bake_mesh_transform` writes `B_new = T · B`, which puts T at the FRONT of the chain, not the end.
That is correct exactly once -- on a mesh straight off the rebind, where T_chain is the placement
itself -- and wrong on every re-bake after it: each bake has to chain from the original placement.

The size of being wrong, measured on this belt: the grenades' T_chain is a ~6 degree rotation, so a
20 mm slider tuned on the body lands about 2 mm away from where the player put it. Small, and there
is no reason to accept it when the correct form is one more multiplication.

WHAT APPENDING IS, in closed form. We want

    B_final = T_chain · P · B_hips      and we know      B = T_chain · B_hips

so, with no need to know T_chain or to rebuild anything from the pristine mesh:

    B_final = B · B_hips^-1 · P · B_hips

The pass P is then applied in the frame the runtime slider works in -- the component's own local
transform, which is entity space -- because B_hips · M_Hips = I in the bind pose. Prepending applies
it in the mesh's space after the placement instead, which is the 6 degrees.

The geometry transform that corresponds to it, for the bounding box, is `C = B_final · B^-1`.

WHERE B_hips COMES FROM. Any garment skinned to this rig carries the same Hips inverse bind matrix --
it is a property of the skeleton, not of the mesh -- so it is read out of a pristine belt garment from
the Ezio archive rather than from one of our own meshes, every one of which has a chain baked into it.

Usage:
    python tools/bake_belt_pass.py <hips_reference.mesh.json> <mesh.json> x y z rx ry rz
"""
import io
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from bake_mesh_transform import euler_matrix, mat4, mul4, from_json, to_json


def inv4(m):
    """Inverse of an affine row-vector matrix: a general 3x3 inverse plus the translation."""
    a = [row[:3] for row in m[:3]]
    det = (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
           - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
           + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
    if abs(det) < 1e-12:
        raise SystemExit("матрица вырождена, обратить нельзя")
    inv = [[0.0] * 3 for _ in range(3)]
    for i in range(3):
        for j in range(3):
            r = [x for k, x in enumerate(a) if k != j]
            sub = [[x for k, x in enumerate(row) if k != i] for row in r]
            cof = sub[0][0] * sub[1][1] - sub[0][1] * sub[1][0]
            inv[i][j] = cof * ((-1) ** (i + j)) / det
    t = m[3][:3]
    ti = [-sum(t[k] * inv[k][j] for k in range(3)) for j in range(3)]
    return mat4(inv, ti)


def hips_matrix(path):
    d = json.load(io.open(path, encoding="utf-8"))
    root = d["Data"]["RootChunk"]
    names = [b["$value"] for b in root["boneNames"]]
    if "Hips" not in names:
        raise SystemExit("в эталоне нет кости Hips: %s" % path)
    m = root["boneRigMatrices"][names.index("Hips")]
    return from_json(m.get("Data", m))


def main():
    if len(sys.argv) != 9:
        print(__doc__)
        return 2
    hips_path, mesh_path = sys.argv[1], sys.argv[2]
    x, y, z, rx, ry, rz = [float(v) for v in sys.argv[3:]]

    H = hips_matrix(hips_path)
    Hi = inv4(H)
    P = mat4(euler_matrix(rx, ry, rz), (x, y, z))
    # the pass, expressed in the mesh's own space: B^-1 · (B · Hi · P · H) reduces to Hi · P · H
    # conjugated by B, and that is what both the rig matrices and the bounding box get.
    K = mul4(mul4(Hi, P), H)

    d = json.load(io.open(mesh_path, encoding="utf-8"))
    root = d["Data"]["RootChunk"]
    mats = root.get("boneRigMatrices") or []
    if not mats:
        raise SystemExit("в меше нет boneRigMatrices: %s" % mesh_path)

    for m in mats:
        md = m.get("Data", m)
        B = from_json(md)
        to_json(mul4(B, K), md)
    print("  проход pos %.4f %.4f %.4f  deg %.2f %.2f %.2f -> %d матриц"
          % (x, y, z, rx, ry, rz, len(mats)))

    # THE BOX GETS `P`, NOT THE MESH-SPACE FORM. It is stored in the frame the placement left it in --
    # B_hips not applied -- which is the frame the panel's numbers are already in. Using the mesh-space
    # conjugate here put satara's box a metre and a half from the magazine it is supposed to bound, and
    # a box in the wrong place is a magazine culled while it is on screen.
    C = P
    bb = root["boundingBox"]
    mn = [bb["Min"][k] for k in ("X", "Y", "Z")]
    mx = [bb["Max"][k] for k in ("X", "Y", "Z")]
    lo, hi = [1e30] * 3, [-1e30] * 3
    for i in range(8):
        p = [mn[0] if i & 1 else mx[0], mn[1] if i & 2 else mx[1], mn[2] if i & 4 else mx[2]]
        q = [sum(p[k] * C[k][j] for k in range(3)) + C[3][j] for j in range(3)]
        for j in range(3):
            lo[j], hi[j] = min(lo[j], q[j]), max(hi[j], q[j])
    for j, k in enumerate(("X", "Y", "Z")):
        bb["Min"][k], bb["Max"][k] = lo[j], hi[j]
    print("  bbox центр (%.4f, %.4f, %.4f)" % tuple((lo[j] + hi[j]) / 2 for j in range(3)))

    io.open(mesh_path, "w", encoding="utf-8", newline="").write(json.dumps(d, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
