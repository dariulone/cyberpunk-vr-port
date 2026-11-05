"""2.5 engine stereo: the tiled light pass lights both eyes in ONE dispatch (2026-11-05, shared lighting).

The engine dispatches every tiled-light permutation once per eye (RenderLightBuffers), and each dispatch walks the
tile list of the whole side-by-side frame: a group of the other eye's half loads its tile and depth and leaves
(b1 row 53.x, the dispatch's eye, against the pixel's half -- leftWidth = b0 row 2.x / 2). Apart from that test the
shader reads nothing of the eye's camera block: b1 row 53 is its only b1 read, the eyes' pixel->world inverses are
both in b12 (rows 77-80 left, 97-100 right; the first-person ones beside them), picked by that same eye value.

The twin (an ALTERNATE of the stereo_merge twin, Render/HiddenAreaShaders.cpp group 16, CyberpunkVR_StereoLightMergePatch):

  * every use of b1 row 53.x becomes the pixel's own eye (1 left of leftWidth, 2 right of it; 0 stays 0, the mono
    frame), so the left eye's dispatch lights every tile with its own eye's matrices and none leaves early;
  * the right eye's dispatch (b1 row 53.x == 2) returns right after the group barrier: its tiles were lit;
  * the view origin (stereo_merge's eye_view: the eye's position instead of the engine's stereo centre in b12 row 0)
    is the pixel's eye position from that eye's inverse: its depth column (b12 row 79 / 99) over its w -- the point
    every ray of the eye starts from. In the mono frame the engine's row 0 stays.

A permutation that reads any other b1 row, or lacks the b12 layout, is not built (it keeps its stereo_merge twin).

  py -3.13 -B stereo_light_merge_shaders.py --shaders DIR_WITH_ORIGINAL_DXIL --out OUT_DIR
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

EYE_ROW = 53
INVERSE_ROWS = {1: (77, 78, 79, 80), 2: (97, 98, 99, 100)}   # b12 pixel->world, by eye: x, y, depth, constant
DEPTH_COLUMN = {1: 79, 2: 99}
LEFT_WIDTH = '@"\\01?leftWidth@@3HA"'
F, I = sm.F, sm.I


def cb_load(kind, handle, row):
    return (r'(?m)^  (%[\w.]+) = call ' + re.escape(kind) + r' @dx\.op\.cbufferLoadLegacy\.' +
            ('f32' if kind == F else 'i32') + r'\(i32 59, %dx\.types\.Handle ' + re.escape(handle) + r', i32 ' +
            str(row) + r'\)')


def check(ir, b1, b12):
    rows = set(re.findall(r'@dx\.op\.cbufferLoadLegacy\.(?:f32|i32)\(i32 59, %dx\.types\.Handle ' + re.escape(b1) +
                          r', i32 (\d+)\)', ir))
    if rows != {str(EYE_ROW)}:
        raise ValueError('b1 rows read: %s (only %d is per dispatch-safe)' % (sorted(rows), EYE_ROW))
    for eye, rows in INVERSE_ROWS.items():
        for row in rows:
            if not re.search(cb_load(F, b12, row), ir):
                raise ValueError('b12 row %d (eye %d inverse) is not read' % (row, eye))
    if LEFT_WIDTH not in ir:
        raise ValueError('no group-shared leftWidth')


def pixel_x(ir):
    """The pixel column compared with the group-shared left width (the half test)."""
    loads = re.findall(r'(?m)^  (%\d+) = load i32, i32 addrspace\(3\)\* ' + re.escape(LEFT_WIDTH), ir)
    xs = set()
    for value in loads:
        xs.update(re.findall(r'(?m)^  %\d+ = icmp ult i32 (%\d+), ' + re.escape(value) + r'$', ir))
    if len(xs) != 1:
        raise ValueError('pixel half test: %d candidates' % len(xs))
    return xs.pop()


def return_block(ir):
    """The block that is only `ret void` -- where the eye's early-outs go."""
    found = re.findall(r'(?m)^; <label>:(\d+) .*\n  ret void$', ir)
    if len(found) != 1:
        raise ValueError('return block: %d candidates' % len(found))
    return found[0]


def pixel_eye(tag, x, original):
    """Lines that make %<tag>.e: 0 when the dispatch's eye is 0 (mono), else the pixel's own eye."""
    return ['  %%%s.w = load i32, i32 addrspace(3)* %s, align 4' % (tag, LEFT_WIDTH),
            '  %%%s.l = icmp ult i32 %s, %%%s.w' % (tag, x, tag),
            '  %%%s.p = select i1 %%%s.l, i32 1, i32 2' % (tag, tag),
            '  %%%s.z = icmp eq i32 %s, 0' % (tag, original),
            '  %%%s.e = select i1 %%%s.z, i32 0, i32 %%%s.p' % (tag, tag, tag)]


def eye_of_pixel(ir, b1, x):
    """Every b1 row 53.x extract becomes the pixel's eye (its uses after it, not the extract)."""
    # The engine's own reads (numbered values); the origin's %ev.kind only tells mono from stereo, as the dispatch's
    # eye does.
    loads = [v for v in re.findall(cb_load(I, b1, EYE_ROW), ir) if re.fullmatch(r'%\d+', v)]
    if len(loads) < 2:
        raise ValueError('b1 row %d is read %d times (expected the early-out and the inverse choice)' % (EYE_ROW, len(loads)))
    for k, load in enumerate(loads):
        value = sm.extract(ir, load, 0, I)
        line = re.search(r'(?m)^  ' + re.escape(value) + r' = extractvalue .*$', ir)
        if ir.index(x + ' = ') > line.start():
            raise ValueError('the pixel column is defined after the eye is read')
        head, tail = ir[:line.end()], ir[line.end():]
        tail, n = re.subn(re.escape(value) + r'\b', '%%lm%d.e' % k, tail)
        if not n:
            raise ValueError('eye value %s has no uses' % value)
        ir = head + '\n' + '\n'.join(pixel_eye('lm%d' % k, x, value)) + tail
    return ir


def right_dispatch_returns(ir, b1, ret):
    """After the first group barrier: the right eye's dispatch (b1 row 53.x == 2) goes to the return block."""
    barrier = re.search(r'(?m)^  call void @dx\.op\.barrier\(i32 80, i32 \d+\).*$', ir)
    if not barrier:
        raise ValueError('no group barrier')
    label = re.findall(r'(?m)^; <label>:(\d+) ', ir[:barrier.start()])
    if not label:
        raise ValueError('the barrier is in the entry block')
    block = label[-1]
    head, tail = ir[:barrier.end()], ir[barrier.end():]
    # Successors of the split block now come from lm.go: their phis' incoming label changes.
    tail = re.sub(r', %' + block + r' \]', ', %lm.go ]', tail)
    insert = ['  %%lm.d = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (I, b1, EYE_ROW),
              '  %%lm.dv = extractvalue %s %%lm.d, 0' % I,
              '  %lm.right = icmp eq i32 %lm.dv, 2',
              '  br i1 %%lm.right, label %%%s, label %%lm.go' % ret,
              '',
              'lm.go:                                            ; preds = %' + block]
    return head + '\n' + '\n'.join(insert) + tail


def eye_origin(ir, b0, b1, b12, x):
    """The view origin: the engine's b12 row 0 (stereo centre) becomes the pixel's eye position in the stereo frame."""
    load = re.search(cb_load(F, b12, 0), ir)
    if not load:
        raise ValueError('b12 row 0 is not read')
    origin = [sm.extract(ir, load.group(1), c, F) for c in range(3)]
    for value in origin:
        if not re.search(r'(?m)^  %\d+ = fsub fast float ' + re.escape(value) + r', %\d+$', ir):
            raise ValueError('b12 row 0 is not used as an origin (%s)' % value)
    last = re.search(r'(?m)^  ' + re.escape(origin[2]) + r' = .*$', ir)
    if ir.index(x + ' = ') > last.start():
        raise ValueError('the pixel column is defined after the origin')
    insert = [
        '  %%ev.flag = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 32)' % (I, b0),
        '  %%ev.flagx = extractvalue %s %%ev.flag, 0' % I,
        '  %%ev.kind = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (I, b1, EYE_ROW),
        '  %%ev.kindx = extractvalue %s %%ev.kind, 0' % I,
        '  %ev.two = icmp ne i32 %ev.flagx, 0',
        '  %ev.eye = icmp ne i32 %ev.kindx, 0',
        '  %ev.stereo = and i1 %ev.two, %ev.eye',
        '  %%ev.w = load i32, i32 addrspace(3)* %s, align 4' % LEFT_WIDTH,
        '  %%ev.l = icmp ult i32 %s, %%ev.w' % x,
        '  %%ev.row = select i1 %%ev.l, i32 %d, i32 %d' % (DEPTH_COLUMN[1], DEPTH_COLUMN[2]),
        '  %%ev.m = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 %%ev.row)' % (F, b12),
        '  %%ev.mw = extractvalue %s %%ev.m, 3' % F,
        '  %ev.iw = fdiv fast float 1.000000e+00, %ev.mw',
    ]
    for c, value in enumerate(origin):
        insert += ['  %%ev.m%d = extractvalue %s %%ev.m, %d' % (c, F, c),
                   '  %%ev.p%d = fmul fast float %%ev.m%d, %%ev.iw' % (c, c),
                   '  %%ev.o%d = select i1 %%ev.stereo, float %%ev.p%d, float %s' % (c, c, value)]
    head, tail = ir[:last.end()], ir[last.end():]
    uses = 0
    for c, value in enumerate(origin):
        tail, n = re.subn(re.escape(value) + r'\b', '%%ev.o%d' % c, tail)
        uses += n
    if not uses:
        raise ValueError('b12 row 0 has no uses after its load')
    return head + '\n' + '\n'.join(insert) + tail


def light_merge(ir):
    b0, b1, b12 = (sm.handle(ir, 2, r) for r in (0, 1, 12))
    check(ir, b1, b12)
    x = pixel_x(ir)
    ret = return_block(ir)
    ir = eye_origin(ir, b0, b1, b12, x)          # before the eye rewrite: it reads the dispatch's own eye
    ir = eye_of_pixel(ir, b1, x)
    return right_dispatch_returns(ir, b1, ret)


def build(original):
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        container = ham.assemble(light_merge(ham.disassemble(original, work)), work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('validator output hash mismatch')
    return container


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--shaders', type=Path, required=True, help='original shaders named <fnv1a>.dxil')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    built = skipped = 0
    for key in sorted(sm.LIGHT_PASS):
        source = args.shaders / ('%016x.dxil' % key)
        if not source.exists():
            print('MISSING %016x' % key); skipped += 1; continue
        original = source.read_bytes()
        if ham.fnv1a(original) != key:
            raise RuntimeError('%s does not hash to its name' % source)
        try:
            twin = build(original)
        except (ValueError, RuntimeError) as error:
            print('SKIP %016x: %s' % (key, str(error).splitlines()[0][:160])); skipped += 1; continue
        (args.out / ('%016x.dxil' % key)).write_bytes(twin)
        built += 1
    print('built %d, skipped %d' % (built, skipped))
    return 0 if built else 1


if __name__ == '__main__':
    sys.exit(main())
