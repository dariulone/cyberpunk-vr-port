"""2.5 engine stereo: does each eye's camera block carry that eye's own position? (reflections painted onto
their surface in both eyes, SSR and RT alike, 2026-11-01)

A reflection has depth only because the two eyes reflect different rays: reflect(view, normal), the view
running from the eye. If both eyes' CameraShaderConsts (b1) hold one position in row 36 (the eye position,
the only camera row the RT raygens read), every surface point reflects the same ray in both eyes and the
reflection sits on the surface.

Reads the plugin's ring of camera blocks (Stereo/Grading.cpp cam_block_note, CyberpunkVR_DebugCamBlocks, 864
bytes each, kept while the runtime diagnostics are on) for a few seconds and, per uploading node and camera
kind (row 53.x: 0 centre, 1 left, 2 right), prints row 36 (position) and row 41 (forward); then, for each node
with both eyes, the distance between them against the separation handed to the engine
(CyberpunkVR_DebugEngineStereoValues[0]).

  py -3.11 -B eye_camera_blocks.py --pid PID [--seconds 2]
"""
import argparse
import math
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rt_dispatch_census import Process, node_names, resolve_exports

NOTE = struct.Struct('<IiII864s')          # CamBlockNote: node side seq pad block
RING = 64


def row(block, r):
    return struct.unpack_from('<4f', block, 16 * r)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--pid', type=int, required=True)
    ap.add_argument('--seconds', type=float, default=2.0)
    a = ap.parse_args()
    at = resolve_exports(a.pid, ['CyberpunkVR_DebugCamBlocks', 'CyberpunkVR_DebugCamBlockNext',
                                 'CyberpunkVR_RuntimeDiagnostics', 'CyberpunkVR_DebugEngineStereoValues'])
    p = Process(a.pid)
    diagnostics = p.read(at['CyberpunkVR_RuntimeDiagnostics'], 4)
    p.write(at['CyberpunkVR_RuntimeDiagnostics'], struct.pack('<i', 1))
    seen = {}                               # (node, side, kind) -> [count, last block]
    try:
        end = time.time() + a.seconds
        while time.time() < end:
            raw = p.read(at['CyberpunkVR_DebugCamBlocks'], NOTE.size * RING)
            for i in range(RING):
                node, side, seq, _, block = NOTE.unpack_from(raw, i * NOTE.size)
                if not seq:
                    continue
                kind = struct.unpack_from('<i', block, 16 * 53)[0]
                entry = seen.setdefault((node, side, kind), [0, None, set()])
                if seq not in entry[2]:
                    entry[2].add(seq)
                    entry[0] += 1
                    entry[1] = block
            time.sleep(0.01)
    finally:
        p.write(at['CyberpunkVR_RuntimeDiagnostics'], diagnostics)
    sep = struct.unpack('<3f', p.read(at['CyberpunkVR_DebugEngineStereoValues'], 12))
    names = node_names()
    print('separation handed to the engine %.4f (first person %.4f, convergence %g)\n' % sep)
    print('%-38s %4s %4s %6s   %-30s %s' % ('node', 'side', 'kind', 'blocks', 'row 36 (position)', 'row 41 (forward)'))
    for (node, side, kind), (count, block, _) in sorted(seen.items()):
        pos, fwd = row(block, 36), row(block, 41)
        print('%-38s %4d %4d %6d   %8.3f %8.3f %8.3f %5.2f   %6.3f %6.3f %6.3f'
              % ('%#x %s' % (node, names.get(node, '?')), side, kind, count, *pos, *fwd[:3]))
    print('\nper node, left (kind 1) vs right (kind 2):')
    nodes = {(n, s) for n, s, k in seen if k == 1} & {(n, s) for n, s, k in seen if k == 2}
    for node, side in sorted(nodes):
        l, r = row(seen[(node, side, 1)][1], 36), row(seen[(node, side, 2)][1], 36)
        d = math.dist(l[:3], r[:3])
        print('  %-38s eye distance %.4f  (%s)' % ('%#x %s' % (node, names.get(node, '?')), d,
              'SAME position' if d < 1e-4 else 'ratio to separation %.2f' % (d / sep[0] if sep[0] else float('nan'))))
    if not nodes:
        print('  no node uploaded both eyes\' blocks in the window')


if __name__ == '__main__':
    main()
