"""Diagnostic only: which input carries a forward material's reflection (2026-11-03: the car hood's sharp mirror image
is drawn by forward ps 69a687939469a979; its eye_view twin is bound, yet the image stays painted on).

Builds the normal forward twin (stereo_ssr_merge_shaders.eye_view, no merge), then knocks out per eye:
  left eye  (b1 row 53.x == 1): every sample of the t44 probe atlas reads 0
  right eye (b1 row 53.x == 2): every t56 screen-reflection load fails its valid test (the probe result stays)
Whichever eye loses the mirror image names its source.

  py -3.11 -B forward_knockout.py --shader ORIGINAL.dxil --out OUT_DIR
"""
import argparse
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import ham_shaders as ham
import dxil_container as dc
import stereo_ssr_merge_shaders as sm

R = '%dx.types.ResRet.f32'


def srv(ir, register):
    m = re.search(r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandle\(i32 57, i8 0, i32 \d+, i32 ' + str(register) +
                  r', i1 false\)', ir)
    if not m:
        raise ValueError('t%d not bound' % register)
    return m.group(1)


def knockout(ir):
    b1 = sm.handle(ir, 2, 1)
    t44, t56 = srv(ir, 44), srv(ir, 56)
    lines = ir.split('\n')
    # the eye, after the handles in the entry block
    at = max(i for i, l in enumerate(lines) if '@dx.op.createHandle(i32 57' in l and not re.search(r'i32 %\d+, i1', l)) + 1
    lines[at:at] = ['  %%ko.l = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (sm.I, b1, sm.EYE_ROW),
                    '  %%ko.k = extractvalue %s %%ko.l, 0' % sm.I,
                    '  %ko.left = icmp eq i32 %ko.k, 1',
                    '  %ko.notright = icmp ne i32 %ko.k, 2']
    ir = '\n'.join(lines)
    # left: t44 samples -> 0
    probe = re.findall(r'(?m)^  (%\d+) = call ' + re.escape(R) + r' @dx\.op\.sample\w*\.f32\(i32 \d+, %dx\.types\.Handle ' + re.escape(t44) + ',', ir)
    n44 = 0
    for s in probe:
        def zero(m):
            v = '%%ko.v%s' % m.group(1)[1:]
            return '  %s = extractvalue %s %s, %s\n  %s = select i1 %%ko.left, float 0.000000e+00, float %s' % (
                v, R, s, m.group(2), m.group(1), v)
        ir, n = re.subn(r'(?m)^  (%\d+) = extractvalue ' + re.escape(R) + ' ' + re.escape(s) + r', (\d)$', zero, ir)
        n44 += n
    # right: the branch after each t56 load takes its invalid side
    n56 = 0
    for m in list(re.finditer(r'(?m)^  %\d+ = call ' + re.escape(R) + r' @dx\.op\.textureLoad\.f32\(i32 66, %dx\.types\.Handle ' +
                              re.escape(t56) + ',.*$', ir)):
        br = re.compile(r'(?m)^  br i1 (%\d+), label').search(ir, m.end())
        c = br.group(1)
        k = '%%ko.c%d' % n56
        ir = ir[:br.start()] + '  %s = and i1 %s, %%ko.notright\n  br i1 %s, label' % (k, c, k) + ir[br.end():]
        n56 += 1
    if not probe or not n56:
        raise ValueError('t44 samples %d, t56 loads %d' % (len(probe), n56))
    return ir, len(probe), n44, n56


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--shader', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    a = ap.parse_args()
    original = a.shader.read_bytes()
    with tempfile.TemporaryDirectory() as t:
        work = Path(t)
        ir, samples, values, loads = knockout(sm.eye_view(ham.disassemble(original, work)))
        container = ham.assemble(ir, work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('validator output hash mismatch')
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / a.shader.name).write_bytes(container)
    print('%s: t44 samples %d (%d values zeroed in the left eye), t56 valid tests %d (false in the right eye)' % (
        a.shader.name, samples, values, loads))


if __name__ == '__main__':
    main()
