"""pack.py RAW JSON OUT: the cooked meshes (RAW: the depot tree WolvenKit extracted, JSON: their cr2w_to_json files) packed
for profilegen.exe -- per mesh its depot path's FNV1a64, the path, the position quantization, and its LOD0 chunks' position
streams (Short4N, 8 bytes a vertex) and triangle lists (u16) as they are in the render buffer. No arithmetic on the positions here: the generator
decodes them."""
import base64
import json
import os
import struct
import sys

RAW, JSON_DIR, OUT = sys.argv[1:4]


def fnv(s):
    h = 0xcbf29ce484222325
    for b in s.encode('utf-8'):
        h ^= b
        h = (h * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h


paths = {}
for root, _, files in os.walk(RAW):
    for f in files:
        if f.endswith('.mesh'):
            paths[f] = os.path.relpath(os.path.join(root, f), RAW)

out = []
for name in sorted(os.listdir(JSON_DIR)):
    if not name.endswith('.mesh.json'):
        continue
    mesh = name[:-5]
    depot = paths.get(mesh)
    if not depot:
        continue
    d = json.load(open(os.path.join(JSON_DIR, name), encoding='utf-8'))
    blob = d['Data']['RootChunk'].get('renderResourceBlob', {}).get('Data')
    if not blob:
        continue
    h = blob['header']
    qs, qo = h['quantizationScale'], h['quantizationOffset']
    buf = base64.b64decode(blob['renderBuffer']['Bytes'])
    chunks = []
    for c in h['renderChunkInfos']:
        if not (c.get('lodMask', 1) & 1):
            continue
        els = c['chunkVertices']['vertexLayout']['elements']['Elements']
        pos = [e for e in els if e.get('usage') == 'PS_Position']
        if not pos or pos[0].get('type') != 'PT_Short4N':
            continue
        off = c['chunkVertices']['byteOffsets']['Elements'][pos[0].get('streamIndex', 0)]
        if c['chunkIndices'].get('pe', 'IBCT_IndexUShort') != 'IBCT_IndexUShort':
            continue
        at = h['indexBufferOffset'] + c['chunkIndices'].get('teOffset', 0)
        chunks.append((off, c['numVertices'], at, c.get('numIndices', 0)))
    if not chunks:
        continue
    rec = struct.pack('<QH', fnv(depot), len(depot)) + depot.encode('utf-8')
    rec += struct.pack('<6f', qs['X'], qs['Y'], qs['Z'], qo['X'], qo['Y'], qo['Z'])
    data = b''
    rec += struct.pack('<I', len(chunks))
    for off, n, at, ni in chunks:
        part = buf[off:off + 8 * n]
        idx = buf[at:at + 2 * ni]
        rec += struct.pack('<I', len(part) // 8) + part + struct.pack('<I', len(idx) // 2) + idx
    out.append(rec)
with open(OUT, 'wb') as f:
    f.write(struct.pack('<4sI', b'GPRF', len(out)))
    for r in out:
        f.write(r)
print('packed', len(out), 'meshes into', OUT)
