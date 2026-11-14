# handmesh.py RIG_JSON OUTDIR GLB_R GLB_L: the game's first-person hands (a0_000_pwa_fpp__full_r/_l.glb, as cp77tools
# uncooks them) and the anim rig's bone names (player_woman_skeleton.rig as JSON), unpacked for the harness: OUTDIR/
# rig_names.txt (index name) and OUTDIR/hand_r.bin, hand_l.bin. File decoding only: the bind matrices are copied as they
# are and every computation (inverses, the hand's frame, skinning) is the harness's (C++).
#
# hand_*.bin, little-endian:
#   'HMS1', u32 joints, u32 verts, u32 tris
#   joints x (char name[64], f32 inverseBind[16] column-major as glTF stores it)
#   verts  x (f32 pos[3], f32 nrm[3], u16 joint[8], f32 weight[8])
#   tris   x u32[3]
import json
import struct
import sys
from pathlib import Path

import numpy as np

COMP = {5126: np.float32, 5125: np.uint32, 5123: np.uint16, 5122: np.int16, 5121: np.uint8, 5120: np.int8}
WIDTH = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}
NORM = {5121: 255.0, 5123: 65535.0}


def read_glb(path):
    data = Path(path).read_bytes()
    n = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + n])
    m = struct.unpack_from('<I', data, 20 + n)[0]
    return doc, data[20 + n + 8:20 + n + 8 + m]


def accessor(doc, binary, i):
    a = doc['accessors'][i]
    v = doc['bufferViews'][a['bufferView']]
    dt = np.dtype(COMP[a['componentType']])
    w = WIDTH[a['type']]
    stride = v.get('byteStride', dt.itemsize * w)
    off = v.get('byteOffset', 0) + a.get('byteOffset', 0)
    raw = np.frombuffer(binary, dtype=np.uint8, count=stride * (a['count'] - 1) + dt.itemsize * w, offset=off)
    out = np.lib.stride_tricks.as_strided(raw, shape=(a['count'], dt.itemsize * w), strides=(stride, 1)).copy()
    out = out.view(dt).reshape(a['count'], w)
    if a.get('normalized') and a['componentType'] in NORM:
        out = out.astype(np.float32) / NORM[a['componentType']]
    return out


def convert(glb, out):
    doc, binary = read_glb(glb)
    skin = doc['skins'][0]
    names = [doc['nodes'][j]['name'] for j in skin['joints']]
    ibm = accessor(doc, binary, skin['inverseBindMatrices']).astype(np.float32)
    pos, nrm, jnt, wgt, tri = [], [], [], [], []
    base = 0
    for mesh in doc['meshes']:
        for p in mesh['primitives']:
            at = p['attributes']
            P = accessor(doc, binary, at['POSITION']).astype(np.float32)
            N = accessor(doc, binary, at['NORMAL']).astype(np.float32)
            J1 = accessor(doc, binary, at['JOINTS_1']) if 'JOINTS_1' in at else np.zeros((len(P), 4))
            W1 = accessor(doc, binary, at['WEIGHTS_1']) if 'WEIGHTS_1' in at else np.zeros((len(P), 4))
            J = np.concatenate([accessor(doc, binary, at['JOINTS_0']), J1], axis=1).astype(np.uint16)
            W = np.concatenate([accessor(doc, binary, at['WEIGHTS_0']).astype(np.float32), W1.astype(np.float32)], axis=1)
            I = accessor(doc, binary, p['indices']).reshape(-1, 3).astype(np.uint32) + base
            pos.append(P); nrm.append(N); jnt.append(J); wgt.append(W); tri.append(I)
            base += len(P)
    pos, nrm, jnt, wgt, tri = (np.concatenate(x) for x in (pos, nrm, jnt, wgt, tri))
    with open(out, 'wb') as f:
        f.write(b'HMS1' + struct.pack('<3I', len(names), len(pos), len(tri)))
        for nm, m in zip(names, ibm):
            f.write(nm.encode()[:63].ljust(64, b'\0') + m.astype('<f4').tobytes())
        vert = np.zeros(len(pos), dtype=[('p', '<f4', 3), ('n', '<f4', 3), ('j', '<u2', 8), ('w', '<f4', 8)])
        vert['p'], vert['n'], vert['j'], vert['w'] = pos, nrm, jnt, wgt
        f.write(vert.tobytes())
        f.write(tri.astype('<u4').tobytes())
    print('%s: %d joints, %d verts, %d tris' % (out, len(names), len(pos), len(tri)))


def main():
    rig, outdir, glb_r, glb_l = sys.argv[1:5]
    outdir = Path(outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    doc = json.loads(Path(rig).read_text(encoding='utf-8'))
    data = doc['Data']['RootChunk'] if 'Data' in doc else doc
    bones = [b['$value'] if isinstance(b, dict) else b for b in data['boneNames']]
    (outdir / 'rig_names.txt').write_text(''.join('%d %s\n' % (i, n) for i, n in enumerate(bones)))
    print('rig: %d bones' % len(bones))
    convert(glb_r, outdir / 'hand_r.bin')
    convert(glb_l, outdir / 'hand_l.bin')


if __name__ == '__main__':
    main()
