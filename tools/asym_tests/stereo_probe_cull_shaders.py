"""2.5 clustered probe/decal bounds must use the current eye's inverse projection scale.

The native culler 355ba40185eeae65 (render node 0x410F1C, after the light culler) bins the environment probes and
decals into the 3D clusters and writes the per-tile probe mask (96x52 32-pixel tiles under engine stereo) that the
deferred ambient pass reads to pick its reflection probes. Like the light culler (stereo_light_cull_shaders.py) it
builds each tile's planes from the eye's own projection but scales its bounding sphere and AABB by the shared COM row
1, which under Engine Stereo describes the union camera, wider than either eye. A probe is then dropped from tiles it
covers, and the ambient pass changes probe set at a tile edge: a hard 32-pixel border across distant geometry (the
Pacifica truss and the facade behind it) that slides over the world when the head turns.

In the stereo path, derive (1/P00, -1/P11) from the same projection as the native planes: COM rows 11/12 for eye
index 0, 2/3 for eye index 1 (the native branch order). The mono path keeps the native scale, so mono is unchanged.
Output belongs in src/Shaders/stereo_probe_cull.
"""
import argparse
from pathlib import Path
import re
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
import ham_shaders as ham
import dxil_container as dc

KEY = 0x355ba40185eeae65


def eye_scale(ir):
    # %43 is the native eye index, %141 means mono, %8 is the COM. %151/%152 are COM row 1.
    anchor = '  %152 = extractvalue %dx.types.CBufRet.f32 %150, 1\n'
    if ir.count(anchor) != 1 or ir.count('  %141 = icmp eq i32 %140, 0\n') != 1:
        raise ValueError('Unexpected culling scale load')
    before, after = ir.split(anchor)
    block = '''  %pc.left = icmp eq i32 %43, 0
  %pc.rowx = select i1 %pc.left, i32 11, i32 2
  %pc.rowy = add i32 %pc.rowx, 1
  %pc.px = call %dx.types.CBufRet.f32 @dx.op.cbufferLoadLegacy.f32(i32 59, %dx.types.Handle %8, i32 %pc.rowx)
  %pc.py = call %dx.types.CBufRet.f32 @dx.op.cbufferLoadLegacy.f32(i32 59, %dx.types.Handle %8, i32 %pc.rowy)
  %pc.p00 = extractvalue %dx.types.CBufRet.f32 %pc.px, 0
  %pc.p11 = extractvalue %dx.types.CBufRet.f32 %pc.py, 1
  %pc.ex = fdiv fast float 1.000000e+00, %pc.p00
  %pc.ey = fdiv fast float -1.000000e+00, %pc.p11
  %pc.sx = select i1 %141, float %151, float %pc.ex
  %pc.sy = select i1 %141, float %152, float %pc.ey
'''
    for old, new in [('%151', '%pc.sx'), ('%152', '%pc.sy')]:
        after, count = re.subn(re.escape(old) + r'(?![\w.])', new, after)
        if count != 9:
            raise ValueError('Unexpected scale use count: %s = %d' % (old, count))
    return before + anchor + block + after


def build(original):
    if ham.fnv1a(original) != KEY:
        raise ValueError('Unsupported probe culler; expected %016x' % KEY)
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        container = ham.assemble(eye_scale(ham.disassemble(original, work)), work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('DXIL validator output hash mismatch')
    return container


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--shaders', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    original = (args.shaders / ('%016x.dxil' % KEY)).read_bytes()
    container = build(original)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / ('%016x.dxil' % KEY)).write_bytes(container)
    print('Validated %016x: %d -> %d bytes' % (KEY, len(original), len(container)))


if __name__ == '__main__':
    main()
