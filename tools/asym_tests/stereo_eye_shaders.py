"""2.5 engine stereo: one dispatch over both eyes must take each pixel's eye from the pixel, not from the camera
block it was dispatched with (2026-10-23, clouds dark and sharp in the left eye, soft and nearly gone in the
right, CPCHECKSKYLIGHTFOG.rdc).

The volumetric clouds march at half resolution in ONE dispatch over the side-by-side frame (event 42315 of the
capture, the twins' original 8a6c46c528ce5160). The ray already comes from the pixel's own half: x past the
half width is eye 2 and takes the right eye's inverse (b12 rows 109-112), else the left's (rows 89-92). But the
aerial-perspective lookup into the froxel fog volume (192 x 104 over the whole frame, each eye's half its own)
turns the pixel's eye-local position back into a frame position with the CAMERA's eye -- b1 row 53.x, 2 for
the right eye --

    u = ((kind == 2 ? W : 0) + x_eye) * 0.5 / W,  then clamped into the half u fell in

and the dispatch has one camera block, the right eye's (P20 -0.2428). Every left-eye cloud pixel read the
fog of the right eye's half at its own eye-local position, a different direction: the left eye's clouds lost
the haze in front of them (measured in the low-res cloud target: same shape, offset 186 px, the right eye
1.25x brighter, up to 1.9x on the dark parts; composited, the left eye's clouds changed the sky 6x more).
The second lookup in the same shader, after the march, takes the frame position directly and is right.

The twin compares the pixel's eye (the shader's own phi, `select i1 x > W/2, i32 2, i32 1` in the two-halves
branch and 1 outside it) with 2 instead of the camera's kind. In the mono branch the camera kind is 0 and the
pixel's eye 1: both compare false, so the mono path is the engine's.

The recipe is structural, so every quality of the march (and any other shader with the same lookup) is found
by scanning: `--shaders` is a folder of original compute shaders named <FNV-1a>.dxil
(extract_shader_cache.py --kind cs over engine\\staticshader_final.cache).

A second recipe (frame_world, 2026-11-06, the eyes different under water, differenteyesunderwater.rdc): a pass
drawn once over both eyes that rebuilds the world position with the frame camera's clip -> world (b12 rows
65-68) from an NDC over the whole frame takes, in the two-halves mode, the pixel's half and its eye's rows. The
same twin halves the underwater murk in every mode (underwater_clarity, UNDERWATER_CLARITY).

Output: <FNV-1a of the original>.dxil signed containers for `stereo_eye\\` (Render/HiddenAreaShaders.cpp
group 9, CyberpunkVR_StereoEyePatch). `--shaders` may hold pixel shaders too (static_ps).

  py -3.13 -B stereo_eye_shaders.py --shaders DIR_WITH_ORIGINAL_DXIL --out OUT_DIR
"""
import argparse
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
import ham_shaders as ham          # disassemble / assemble / fnv1a / dxc + validator paths
import dxil_container as dc

HANDLE = re.compile(r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandle\(i32 57, i8 2, i32 \d+, i32 (\d+), i1 false\)')
KIND_ROW = 53                      # b1 CameraShaderConsts +848: the camera's eye, 0 centre / 1 left / 2 right
FRAME_ROW = 3                      # b0 row 3.x: the frame's full width


def cbv_handle(ir, register):
    found = [h for h, r in HANDLE.findall(ir) if int(r) == register]
    return found[0] if len(found) == 1 else None


def pixel_eye(ir):
    """The two-halves branch's eye of the pixel: phi i32 [ select(x > W/2, 2, 1) ], [ 1 ] -- one per shader."""
    sels = set(re.findall(r'(?m)^  (%\d+) = select i1 %\d+, i32 2, i32 1$', ir))
    phis = [p for p, s in re.findall(r'(?m)^  (%\d+) = phi i32 \[ (%\d+), %\d+ \], \[ 1, %\d+ \]$', ir) if s in sels]
    phis += [p for p, s in re.findall(r'(?m)^  (%\d+) = phi i32 \[ 1, %\d+ \], \[ (%\d+), %\d+ \]$', ir) if s in sels]
    return phis[0] if len(phis) == 1 else None


def eye_lookups(ir):
    """Every `kind == 2` that picks the right half's frame offset -- select(kind == 2, W + x, x) -- compares the
    pixel's eye instead. 0 sites for a shader without such a lookup."""
    b1, b0 = cbv_handle(ir, 1), cbv_handle(ir, 0)
    if not b1 or not b0:
        return ir, 0
    loads = re.findall(r'(?m)^  (%\d+) = call %dx\.types\.CBufRet\.i32 @dx\.op\.cbufferLoadLegacy\.i32\(i32 59, '
                       r'%dx\.types\.Handle ' + re.escape(b1) + r', i32 ' + str(KIND_ROW) + r'\)', ir)
    widths = set()
    for load in re.findall(r'(?m)^  (%\d+) = call %dx\.types\.CBufRet\.f32 @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, '
                           r'%dx\.types\.Handle ' + re.escape(b0) + r', i32 ' + str(FRAME_ROW) + r'\)', ir):
        widths |= set(re.findall(r'(?m)^  (%\d+) = extractvalue %dx\.types\.CBufRet\.f32 ' + re.escape(load) + ', 0$', ir))
    sites = []
    for load in loads:
        for kind in re.findall(r'(?m)^  (%\d+) = extractvalue %dx\.types\.CBufRet\.i32 ' + re.escape(load) + ', 0$', ir):
            for cmp in re.findall(r'(?m)^  (%\d+) = icmp eq i32 ' + re.escape(kind) + r', 2$', ir):
                for chosen, x in re.findall(r'(?m)^  %\d+ = select i1 ' + re.escape(cmp) + r', float (%\d+), float (%\d+)$', ir):
                    m = re.search(r'(?m)^  ' + re.escape(chosen) + r' = fadd fast float (%\d+), (%\d+)$', ir)
                    others = [o for o in m.groups() if o != x] if m and x in m.groups() else []
                    if len(others) == 1 and others[0] in widths:
                        sites.append((cmp, kind))
    if not sites:
        return ir, 0
    eye = pixel_eye(ir)
    if not eye:
        raise ValueError('eye lookup without the pixel eye phi')
    if ir.index('  ' + eye + ' = ') > min(ir.index('  ' + c + ' = ') for c, _ in sites):
        raise ValueError('pixel eye defined after the lookup')
    for cmp, kind in set(sites):
        ir, n = re.subn(r'(?m)^  ' + re.escape(cmp) + r' = icmp eq i32 ' + re.escape(kind) + r', 2$',
                        '  %s = icmp eq i32 %s, 2' % (cmp, eye), ir, count=1)
        if n != 1:
            raise ValueError('lookup %s not rewritten' % cmp)
    return ir, len(set(sites))


FRAME_WORLD_ROW = 65              # b12 SharedPixelConsts rows 65-68: the frame camera's clip -> world (NDC x, -NDC y,
EYE_WORLD_ROWS = (77, 97)         # depth, 1) -- the same form for the left eye (77-80) and the right eye (97-100)
HALVES_ROW = 32                   # b0 row 32.x: the engine's two-halves (side by side) mode


def frame_world(ir):
    """A pass drawn once over both eyes that rebuilds its world position with the FRAME's clip -> world.

    The underwater volume (static ps 4145873fbf34bf87, differenteyesunderwater.rdc event 44008: one fullscreen
    draw into a 1536 x 830 target over the whole side-by-side frame) takes the pixel's NDC over the whole frame
    and b12 rows 65-68 -- the stereo centre over the union of both lens windows. A left-eye pixel at its own NDC
    u lands at u/2 - 1/2 of the union: its ray up to 0.84 in tangent off the eye's, the fog distance and the depth
    under the surface from a wrong point, mirrored in the other eye. Then the camera's near-plane point is built
    from the frame's uv the same way.

    In the two-halves mode the twin takes x within the pixel's half, that eye's rows (77-80 left, 97-100 right,
    the same form: verified on the capture, the eye positions 32 mm either side of the centre and the window
    shifts -+0.2685 along the right axis) and, for the near-plane point, the frame uv whose centre ray is the
    eye's ray (rows 65 and 68 against the eye's rows 0 and 3). Off the mode the shader is the engine's."""
    b0, b12 = cbv_handle(ir, 0), cbv_handle(ir, 12)
    if not b0 or not b12 or re.search(r'cbufferLoadLegacy\.i32\(i32 59, %dx\.types\.Handle ' + re.escape(b0) +
                                      r', i32 ' + str(HALVES_ROW) + r'\)', ir):
        return ir, 0
    F = '%dx.types.CBufRet.f32'
    loads = []
    for k in range(4):
        found = re.findall(r'(?m)^  (%\d+) = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, '
                           r'%dx\.types\.Handle ' + re.escape(b12) + r', i32 ' + str(FRAME_WORLD_ROW + k) + r'\)', ir)
        if len(found) != 1:
            return ir, 0
        loads.append(found[0])

    def comp(load, c):
        found = re.findall(r'(?m)^  (%\d+) = extractvalue ' + re.escape(F) + ' ' + re.escape(load) + ', %d$' % c, ir)
        if len(found) != 1:
            raise ValueError('row component %s.%d' % (load, c))
        return found[0]

    def one(pattern, what):
        found = re.findall(pattern, ir)
        if len(found) != 1:
            raise ValueError(what)
        return found[0]
    # ndc.x = ((X + 0.5) / W) * 2 - 1 with X the pixel in the frame, W the frame width; used against row 65.x
    row0x = comp(loads[0], 0)
    ndc = one(r'(?m)^  %\d+ = fmul fast float ' + re.escape(row0x) + r', (%\d+)$', 'ndc x against row 65.x')
    twice = one(r'(?m)^  ' + re.escape(ndc) + r' = fadd fast float (%\d+), -1\.000000e\+00$', 'ndc x - 1')
    uv = one(r'(?m)^  ' + re.escape(twice) + r' = fmul fast float (%\d+), 2\.000000e\+00$', 'uv x * 2')
    centred, width = one(r'(?m)^  ' + re.escape(uv) + r' = fdiv fast float (%\d+), (%\d+)$', 'uv x')
    x = one(r'(?m)^  ' + re.escape(centred) + r' = fadd fast float (%\d+), 5\.000000e-01$', 'pixel centre')
    near_u = one(r'(?m)^  (%\d+) = fdiv fast float ' + re.escape(x) + ', ' + re.escape(width) + '$', 'near-plane uv')
    for value in (ndc, near_u):
        uses = re.findall(r'(?m)^  %\d+ = .*' + re.escape(value) + r'\b', ir)
        if not uses:
            raise ValueError('%s unused' % value)
    q = '%sw.'
    head = [
        '  %smode = call %%dx.types.CBufRet.i32 @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (q, b0, HALVES_ROW),
        '  %son = extractvalue %%dx.types.CBufRet.i32 %smode, 0' % (q, q),
        '  %ssbs = icmp ne i32 %son, 0' % (q, q),
        '  %shalf = fmul fast float %s, 5.000000e-01' % (q, width),
        '  %sright = fcmp fast oge float %s, %shalf' % (q, x, q),
        '  %soff = select i1 %sright, float %shalf, float 0.000000e+00' % (q, q, q),
        '  %slx = fsub fast float %s, %soff' % (q, x, q),
        '  %slc = fadd fast float %slx, 5.000000e-01' % (q, q),
        '  %slu = fdiv fast float %slc, %shalf' % (q, q, q),
        '  %sl2 = fmul fast float %slu, 2.000000e+00' % (q, q),
        '  %sndc = fadd fast float %sl2, -1.000000e+00' % (q, q),
        '  %sx = select i1 %ssbs, float %sndc, float %s' % (q, q, q, ndc),
        '  %seye = select i1 %sright, i32 %d, i32 %d' % (q, q, EYE_WORLD_ROWS[1], EYE_WORLD_ROWS[0]),
        '  %srow0 = select i1 %ssbs, i32 %seye, i32 %d' % (q, q, q, FRAME_WORLD_ROW),
        '  %srow1 = add i32 %srow0, 1' % (q, q),
        '  %srow2 = add i32 %srow0, 2' % (q, q),
        '  %srow3 = add i32 %srow0, 3' % (q, q),
    ]
    row0 = [comp(loads[0], c) for c in range(3)]
    row3 = [comp(loads[3], c) for c in range(3)]
    tail = [
        '  %sc0 = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 %d)' % (q, F, b12, FRAME_WORLD_ROW),
        '  %sc3 = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 %d)' % (q, F, b12, FRAME_WORLD_ROW + 3),
    ]
    for c, axis in enumerate('xyz'):
        tail += [
            '  %sc0%s = extractvalue %s %sc0, %d' % (q, axis, F, q, c),
            '  %sc3%s = extractvalue %s %sc3, %d' % (q, axis, F, q, c),
            '  %sh%s = fmul fast float %s, %sx' % (q, axis, row0[c], q),
            '  %sd%s = fadd fast float %sh%s, %s' % (q, axis, q, axis, row3[c]),
            '  %se%s = fsub fast float %sd%s, %sc3%s' % (q, axis, q, axis, q, axis),
        ]
    tail += [
        '  %snum = call float @dx.op.dot3.f32(i32 55, float %sex, float %sey, float %sez, float %sc0x, float %sc0y, float %sc0z)'
        % ((q,) * 7),
        '  %sden = call float @dx.op.dot3.f32(i32 55, float %sc0x, float %sc0y, float %sc0z, float %sc0x, float %sc0y, float %sc0z)'
        % ((q,) * 7),
        '  %sxc = fdiv fast float %snum, %sden' % (q, q, q),
        '  %suc1 = fadd fast float %sxc, 1.000000e+00' % (q, q),
        '  %suc = fmul fast float %suc1, 5.000000e-01' % (q, q),
        '  %su = select i1 %ssbs, float %suc, float %s' % (q, q, q, near_u),
    ]
    lines = ir.split('\n')
    # uses first, while the original names still mean only themselves
    for i, line in enumerate(lines):
        if line.startswith('  ') and ' = ' in line:
            name, rest = line.split(' = ', 1)
            if name.strip() in (ndc, near_u):
                continue
            rest = re.sub(re.escape(ndc) + r'\b', q + 'x', rest)
            rest = re.sub(re.escape(near_u) + r'\b', q + 'u', rest)
            lines[i] = name + ' = ' + rest
    for k, load in enumerate(loads):
        at = next(i for i, l in enumerate(lines) if l.startswith('  %s = ' % load))
        lines[at], n = re.subn(r'i32 %d\)' % (FRAME_WORLD_ROW + k), 'i32 %srow%d)' % (q, k), lines[at])
        if n != 1:
            raise ValueError('row %d load not rewritten' % (FRAME_WORLD_ROW + k))
    at = next(i for i, l in enumerate(lines) if l.startswith('  %s = ' % ndc))
    if at > next(i for i, l in enumerate(lines) if l.startswith('  %s = ' % loads[0])):
        raise ValueError('ndc x defined after the matrix load')
    lines[at + 1:at + 1] = head
    at = next(i for i, l in enumerate(lines) if l.startswith('  %s = ' % near_u))
    if at < next(i for i, l in enumerate(lines) if l.startswith('  %s = ' % loads[3])):
        raise ValueError('near-plane uv before the matrix loads')
    lines[at + 1:at + 1] = tail
    return '\n'.join(lines), 1


UNDERWATER_CLARITY = 0.5         # the murk's optical depth and its darkening with depth, of the engine's (2026-11-06)


def underwater_clarity(ir):
    """Half the murk under water (halved, 2026-11-06). The volume's target feeds the composite
    (static ps 01e26a036e75187d): x is the fog's distance term, in-scattering 1 - exp(-x * density) towards the
    water's colour, and y the depth under the surface, the per-channel darkening saturate(z * y / extinction) --
    the composite's only uses of both. Both are scaled here, so the water lets twice as far through; its blur,
    which follows the in-scattering, eases with it. z (near the surface) and w (the surface line) are left."""
    for channel in (0, 1):
        m = re.search(r'(?m)^  call void @dx\.op\.storeOutput\.f32\(i32 5, i32 0, i32 0, i8 ' + str(channel) +
                      r', float (%\d+)\)(?:  ;[^\n]*)?$', ir)
        if not m:
            raise ValueError('underwater output %d' % channel)
        scaled = '%%uc.%d' % channel
        line = m.group(0)
        ir = ir.replace(line, '  %s = fmul fast float %s, %s\n%s' % (scaled, m.group(1), ham.lit(UNDERWATER_CLARITY),
                                                                   line.replace(m.group(1) + ')', scaled + ')')), 1)
    return ir


def build(original):
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        ir = ham.disassemble(original, work)
        ir, sites = eye_lookups(ir)
        ir, world = frame_world(ir)
        if world:
            ir = underwater_clarity(ir)
        sites += world
        if not sites:
            return None, 0
        container = ham.assemble(ir, work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('validator output hash mismatch')
    return container, sites


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--shaders', type=Path, required=True, help='original compute shaders named <fnv1a>.dxil')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    made = failed = 0
    for source in sorted(args.shaders.glob('*.dxil')):
        original = source.read_bytes()
        key = ham.fnv1a(original)
        if '%016x' % key != source.stem:
            raise RuntimeError('%s does not hash to its name' % source)
        try:
            patched, sites = build(original)
        except Exception as error:
            print('FAILED %016x: %s' % (key, error)); failed += 1; continue
        if patched is None:
            continue
        (args.out / ('%016x.dxil' % key)).write_bytes(patched)
        made += 1
        print('OK %016x: %d eye lookup(s), %d -> %d bytes' % (key, sites, len(original), len(patched)))
    print('%d twin(s), %d failed' % (made, failed))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
