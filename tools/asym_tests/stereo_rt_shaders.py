"""2.5 engine stereo: ray-traced reflections take the view direction from each eye, not the stereo centre
(2026-11-02: a metal floor trim's reflection of a ceiling light was painted on in both eyes with RT reflections,
per-eye with SSR once the ambient and light passes took the eye's position, stereo_ssr_merge_shaders.py).

The reflection raygens (one DispatchRays per eye, each with its own gCommonConstants) reconstruct the pixel
through a clip->world inverse that is RELATIVE to a point -- rows 0-3 for the world, rows 8-11 for first-person
pixels (reverse-Z depth >= 0.9) -- and take the view direction as normalize(P). Measured on the live blocks
(scratchpad rt_origin.py): rows 8-11 converge at 0 (the eye), rows 0-3 at +-half the eye separation (0.0323 m
along right): the stereo centre. So in the world every eye reflects along the centre's ray at each surface point,
and the reflection sits on the surface.

The twin subtracts the eye's own position in that space before normalising: the point every ray of the matrix
meets, which for a reverse-Z projection is clip (0, 0, 1, 0), i.e. row k+2 of the inverse divided by its w.
For a per-eye (or mono) matrix it is 0 and nothing changes. P itself (the ray's start) is untouched.

The library's root signatures live in its RDAT part, which the assembler does not rebuild from the IR, so the
twin is the original container with only the DXIL part replaced (the edit adds no resources), re-signed.

Output: lib_<fnv1a>.dxil -> <fnv1a>.dxil signed containers for `stereo_rt\\` (Render/HiddenAreaShaders.cpp group 15,
CyberpunkVR_StereoRtPatch; swapped when the state object is created, Stereo/DeviceHooks.cpp CreateStateObject).

  python -B stereo_rt_shaders.py --shaders DIR_WITH_lib_<fnv1a>.dxil --out OUT_DIR
"""
import argparse
import re
import struct
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
import ham_shaders as ham
import dxil_container as dc

# True: the trace mirrors a second, biased direction (incident()); the transparent raygen mirrors normalize(P) itself.
LIBRARIES = {
    0x577c76e2620a7045: ('rgs_reflection_opaque_main', True),
    0xd65732f151ce113a: ('rgs_reflection_opaque_main, permutation', True),
    0x7c98c9cdc64d7f2b: ('rgs_reflection_transparent_main', False),
    # The same raygen as a mod in the user's load order rebuilt it (extra sun-shadow traces, values named rfSun*):
    # the game creates this one instead of 7c98 (2026-11-03, CreateStateObject library census).
    0x16a3f056d8ec9845: ('rgs_reflection_transparent_main (modded, rfSun)', False),
    # Path tracing: the camera ray and its biased start are mirrored/scattered the same way (view_survey.py, 2026-11-02).
    0x0267f3da6de86c72: ('rgs_reference_main (path tracing)', True),
    0x02bee30e717617b8: ('rgs_reference_main (path tracing)', True),
    0x10236eb566ac6d9d: ('rgs_reference_main (path tracing)', True),
    0x14d36c91832926df: ('rgs_reference_main (path tracing)', True),
    0x164be8c1dd53ac73: ('rgs_reference_main (path tracing)', True),
    0x2ff04db119472158: ('rgs_reference_main (path tracing)', True),
    0x352301e972f3c61c: ('rgs_reference_main (path tracing)', True),
    0x359ab2d4c741b037: ('rgs_reference_main (path tracing)', True),
    0x43488e0f859a73d5: ('rgs_reference_main (path tracing)', True),
    0x46cdc62694852ee0: ('rgs_reference_main (path tracing)', True),
    0x5ba4397c2fe06e8b: ('rgs_reference_main (path tracing)', True),
    0x63f054555eebd1a5: ('rgs_reference_main (path tracing)', True),
    0x64b1263d0da44c58: ('rgs_reference_main (path tracing)', True),
    0x6585e47e2f51ad46: ('rgs_reference_main (path tracing)', True),
    0x6ef7e2fd1fd688bd: ('rgs_reference_main (path tracing)', True),
    0x731517b1ff38ed48: ('rgs_reference_main (path tracing)', True),
    0x84d73272ee4e6f73: ('rgs_reference_main (path tracing)', True),
    0x8ed6e7f398c4a1db: ('rgs_reference_main (path tracing)', True),
    0x9db2680c58710bde: ('rgs_reference_main (path tracing)', True),
    0xa2ca96dfd49321ca: ('rgs_reference_main (path tracing)', True),
    0xa3ca49b4509a5d19: ('rgs_reference_main (path tracing)', True),
    0xa4af6c3110f0a703: ('rgs_reference_main (path tracing)', True),
    0xaa9d84ec0325d053: ('rgs_reference_main (path tracing)', True),
    0xaf5cad3d0bf55c73: ('rgs_reference_main (path tracing)', True),
    0xb3144e1befe933d1: ('rgs_reference_main (path tracing)', True),
    0xbc15cee0d3f6588a: ('rgs_reference_main (path tracing)', True),
    0xc74435a7c19ea8d3: ('rgs_reference_main (path tracing)', True),
    0xcac7cde26749eec2: ('rgs_reference_main (path tracing)', True),
    0xce1d1d6823c04efd: ('rgs_reference_main (path tracing)', True),
    0xd587a779ec7776fc: ('rgs_reference_main (path tracing)', True),
    0xd84417d7c6bd4b93: ('rgs_reference_main (path tracing)', True),
    0xe04c4b68549bb452: ('rgs_reference_main (path tracing)', True),
    0xe250375cafe70f83: ('rgs_reference_main (path tracing)', True),
    0xe99b3bedd37edf9c: ('rgs_reference_main (path tracing)', True),
}
F = '%dx.types.CBufRet.f32'
V = r'(%\d+)'


def row(ir, handle, r):
    """The xyzw extracts of the (first) load of gCommonConstants row r."""
    load = re.search(r'(?m)^  ' + V + r' = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, %dx\.types\.Handle ' +
                     re.escape(handle) + r', i32 %d\)' % r, ir)
    if not load:
        raise ValueError('row %d is not read' % r)
    out = []
    for c in range(4):
        m = re.search(r'(?m)^  ' + V + r' = extractvalue ' + re.escape(F) + ' ' + re.escape(load.group(1)) + ', %d$' % c, ir)
        if not m:
            raise ValueError('row %d .%d is not extracted' % (r, c))
        out.append(m.group(1))
    return out


def definition(ir, value):
    m = re.search(r'(?m)^  ' + re.escape(value) + r' = (.*)$', ir)
    if not m:
        raise ValueError('no definition of ' + value)
    return m.group(1)


def eye_origin(ir, scale=1.0, biased=True):
    """scale != 1 is a diagnostic: the eye's offset from the centre multiplied, so a twin that controls what the
    headset shows makes reflections visibly wrong."""
    handle = re.search(r'(?m)^  ' + V + r' = call %dx\.types\.Handle @dx\.op\.createHandleForLib\.gCommonConstants\(', ir).group(1)
    # The first normalize(P): dot3(P, P) whose three operands are one perspective divide.
    for site in re.finditer(r'(?m)^  ' + V + r' = call float @dx\.op\.dot3\.f32\(i32 55, float ' + V + ', float ' + V + ', float ' +
                            V + r', float \2, float \3, float \4\).*$', ir):
        dot, p = site.group(1), site.group(2, 3, 4)
        divs = [re.fullmatch(r'fdiv fast float ' + V + ', ' + V, definition(ir, v)) for v in p]
        if all(divs) and len({d.group(2) for d in divs}) == 1:
            break
    else:
        raise ValueError('no dot3(P, P) of a perspective divide')
    w = divs[0].group(2)
    rsq = re.search(r'(?m)^  ' + V + r' = call float @dx\.op\.unary\.f32\(i32 25, float ' + re.escape(dot) + r'\)', ir).group(1)
    scales = [re.search(r'(?m)^  ' + V + r' = fmul fast float ' + re.escape(rsq) + ', ' + re.escape(v) + '$', ir) for v in p]
    if not all(scales):
        raise ValueError('normalize(P) is not rsqrt * P')
    # Which matrix (or which of two, by a select on w): the eye is row k+2 / its w.
    row2 = row(ir, handle, 2)
    select = re.fullmatch(r'select i1 ' + V + ', float ' + V + ', float ' + V, definition(ir, w))
    if select:
        cond = select.group(1)
        row10 = row(ir, handle, 10)
        # The true branch must be the rows 8-11 one: its w adds row 11.w.
        if row(ir, handle, 11)[3] not in definition(ir, select.group(2)):
            raise ValueError('the select does not take rows 8-11 when true')
    else:
        if row(ir, handle, 3)[3] not in definition(ir, w):
            raise ValueError('w does not come from rows 0-3')
        cond = row10 = None
    insert = []
    for c in range(3):
        insert.append('  %%ew.a%d = fdiv fast float %s, %s' % (c, row2[c], row2[3]))
        o = '%%ew.a%d' % c
        if cond:
            insert.append('  %%ew.b%d = fdiv fast float %s, %s' % (c, row10[c], row10[3]))
            insert.append('  %%ew.o%d = select i1 %s, float %%ew.b%d, float %%ew.a%d' % (c, cond, c, c))
            o = '%%ew.o%d' % c
        if scale != 1.0:
            insert.append('  %%ew.s%d = fmul fast float %s, %s' % (c, o, ham_float(scale)))
            o = '%%ew.s%d' % c
        insert.append('  %%ew.d%d = fsub fast float %s, %s' % (c, p[c], o))
    offset = [re.search(r'%ew\.d\d = fsub fast float \S+, (\S+)', line).group(1) for line in insert if '%ew.d' in line]
    d = ['%%ew.d%d' % c for c in range(3)]
    line = site.group(0)
    ir = ir.replace(line, '\n'.join(insert) + '\n' +
                    '  %s = call float @dx.op.dot3.f32(i32 55, float %s, float %s, float %s, float %s, float %s, float %s)' % (dot, *d, *d), 1)
    for c, m in enumerate(scales):
        ir = ir.replace(m.group(0), '  %s = fmul fast float %s, %s' % (m.group(1), rsq, d[c]), 1)
    return incident(ir, p, offset) if biased else ir


def incident(ir, p, offset):
    """The ray the reflection is mirrored from is not normalize(P) itself but normalize(Q), Q = P - V*bias + N*bias
    (the biased ray start), still relative to the centre: 2026-11-02, the twin above alone (even with the offset x30)
    changed nothing in the headset. normalize(Q - eye) instead; Q stays the ray's start."""
    for site in re.finditer(r'(?m)^  ' + V + r' = call float @dx\.op\.dot3\.f32\(i32 55, float ' + V + ', float ' + V + ', float ' +
                            V + r', float \2, float \3, float \4\).*$', ir):
        dot, q = site.group(1), site.group(2, 3, 4)
        ok = True
        for c, v in enumerate(q):
            add = re.fullmatch(r'fadd fast float ' + V + ', ' + V, definition(ir, v))
            if not add or not any(re.fullmatch(r'fsub fast float ' + re.escape(p[c]) + ', %\\d+', definition(ir, o))
                                  for o in add.groups() if o.startswith('%') and not o.startswith('%ew')):
                ok = False
                break
        if ok:
            break
    else:
        raise ValueError('no normalize(P - V*bias + N*bias)')
    rsq = re.search(r'(?m)^  ' + V + r' = call float @dx\.op\.unary\.f32\(i32 25, float ' + re.escape(dot) + r'\)', ir).group(1)
    scales = []
    for v in q:
        m = re.search(r'(?m)^  ' + V + r' = fmul fast float (?:' + re.escape(v) + ', ' + re.escape(rsq) + '|' + re.escape(rsq) + ', ' +
                      re.escape(v) + ')$', ir)
        if not m:
            raise ValueError('normalize(Q) is not Q * rsqrt')
        scales.append(m)
    e = ['%%ei.d%d' % c for c in range(3)]
    insert = ['  %%ei.d%d = fsub fast float %s, %s' % (c, q[c], offset[c]) for c in range(3)]
    ir = ir.replace(site.group(0), '\n'.join(insert) + '\n' +
                    '  %s = call float @dx.op.dot3.f32(i32 55, float %s, float %s, float %s, float %s, float %s, float %s)' % (dot, *e, *e), 1)
    for c, m in enumerate(scales):
        ir = ir.replace(m.group(0), '  %s = fmul fast float %s, %s' % (m.group(1), e[c], rsq), 1)
    return ir


FOG_ORIGINS = ('SharedPixelConsts', 0), ('CameraShaderConsts', 36)


def fog_origin(ir):
    """Height fog along the reflected ray measures the hit from the stereo centre (SharedPixelConsts row 0, and the
    RT nodes' camera block row 36, the head centre): normalize(hit - centre). The eye is centre + the inverse's eye
    offset (row 2 / w, in world-aligned axes: the live offset matched the eyes' world difference). Returns
    (ir, sites)."""
    handle = re.search(r'(?m)^  ' + V + r' = call %dx\.types\.Handle @dx\.op\.createHandleForLib\.gCommonConstants\(', ir).group(1)
    centre = set()
    for name, r in FOG_ORIGINS:
        h = re.search(r'(?m)^  ' + V + r' = call %dx\.types\.Handle @dx\.op\.createHandleForLib\.' + name + r'\(', ir)
        if not h:
            continue
        for load in re.finditer(r'(?m)^  ' + V + r' = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, '
                                r'%dx\.types\.Handle ' + re.escape(h.group(1)) + r', i32 %d\)' % r, ir):
            for c in range(3):
                for m in re.finditer(r'(?m)^  ' + V + ' = extractvalue ' + re.escape(F) + ' ' + re.escape(load.group(1)) +
                                     ', %d$' % c, ir):
                    centre.add(m.group(1))
    sites = 0
    for site in list(re.finditer(r'(?m)^  ' + V + r' = call float @dx\.op\.dot3\.f32\(i32 55, float ' + V + ', float ' + V +
                                 ', float ' + V + r', float \2, float \3, float \4\).*$', ir)):
        subs = [re.fullmatch(r'fsub fast float ' + V + ', ' + V, definition(ir, v)) for v in site.group(2, 3, 4)]
        if not all(subs) or not all(s.group(2) in centre for s in subs):
            continue
        n = sites
        first = re.search(r'(?m)^  ' + re.escape(site.group(2)) + ' = .*$', ir)
        insert = ['  %%ef%d.r = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 2)' % (n, F, handle),
                  '  %%ef%d.w = extractvalue %s %%ef%d.r, 3' % (n, F, n)]
        for c, s in enumerate(subs):
            insert += ['  %%ef%d.e%d = extractvalue %s %%ef%d.r, %d' % (n, c, F, n, c),
                       '  %%ef%d.o%d = fdiv fast float %%ef%d.e%d, %%ef%d.w' % (n, c, n, c, n),
                       '  %%ef%d.c%d = fadd fast float %s, %%ef%d.o%d' % (n, c, s.group(2), n, c)]
        ir = ir[:first.start()] + '\n'.join(insert) + '\n' + ir[first.start():]
        for c, (v, s) in enumerate(zip(site.group(2, 3, 4), subs)):
            ir = re.sub(r'(?m)^  ' + re.escape(v) + r' = fsub fast float ' + re.escape(s.group(1)) + ', ' + re.escape(s.group(2)) + '$',
                        '  %s = fsub fast float %s, %%ef%d.c%d' % (v, s.group(1), n, c), ir, count=1)
        sites += 1
    return ir, sites


def eye_tint(ir):
    """Diagnostic only: the reflection the raygen stores is tinted by eye (gCommonConstants row 204.w: 1 left red,
    2 right blue), so the headset shows whether the denoiser downstream keeps the eyes apart (each eye its own tint) or
    mixes them (purple in both). The opaque raygens store YCoCg on one path: the RGB is tinted before it."""
    handle = re.search(r'(?m)^  ' + V + r' = call %dx\.types\.Handle @dx\.op\.createHandleForLib\.gCommonConstants\(', ir).group(1)
    store = re.search(r'(?m)^  call void @dx\.op\.textureStore\.f32\(i32 67, %dx\.types\.Handle %\d+, i32 \S+, i32 \S+, i32 undef, '
                      r'float ' + V + ', float ' + V + ', float ' + V + r', float %\d+, i8 15\).*$', ir)
    values = list(store.group(1, 2, 3))
    rgb, after = values, store.start()
    phi = re.fullmatch(r'phi float \[ ' + V + r', %\d+ \], \[ ' + V + r', %\d+ \]', definition(ir, values[0]))
    if phi:
        dot = re.fullmatch(r'call float @dx\.op\.dot3\.f32\(i32 55, float ' + V + ', float ' + V + ', float ' + V + ', .*',
                           definition(ir, phi.group(1)))
        if dot:
            rgb = list(dot.group(1, 2, 3))
            after = max(re.search(r'(?m)^  ' + re.escape(v) + ' = .*$', ir).end() + 1 for v in rgb)
    k = {'r': ('1.000000e+00', '2.500000e-01'), 'g': ('5.000000e-01', '5.000000e-01'), 'b': ('2.500000e-01', '1.000000e+00')}
    insert = ['  %%et.l = call %%dx.types.CBufRet.i32 @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 204)' % handle,
              '  %et.e = extractvalue %dx.types.CBufRet.i32 %et.l, 3',
              '  %et.right = icmp eq i32 %et.e, 2']
    for c, n in enumerate('rgb'):
        insert += ['  %%et.k%s = select i1 %%et.right, float %s, float %s' % (n, k[n][1], k[n][0]),
                   '  %%et.%s = fmul fast float %s, %%et.k%s' % (n, rgb[c], n)]
    tinted = {v: '%%et.%s' % n for v, n in zip(rgb, 'rgb')}
    ir = ir[:after] + '\n'.join(insert) + '\n' + ir[after:]
    # the uses after the tint: the store itself, or the YCoCg dot3s and the raw path's phis
    def use(line):
        return re.sub(r'float (%\d+)(?=[,)\s])', lambda m: 'float ' + tinted.get(m.group(1), m.group(1)), line) \
            if 'dot3' in line or 'textureStore' in line else \
            re.sub(r'\[ (%\d+), ', lambda m: '[ ' + tinted.get(m.group(1), m.group(1)) + ', ', line)
    lines = ir.split('\n')
    start = ir[:after].count('\n') + len(insert)
    for i in range(start, len(lines)):
        if 'textureStore' in lines[i] or (phi and (' = phi float ' in lines[i] or 'dot3' in lines[i])):
            lines[i] = use(lines[i])
        if 'textureStore' in lines[i]:
            break
    return '\n'.join(lines)


def hit_stripes(ir, mode='hit'):
    """Diagnostic only (opaque raygens): the stored reflection is replaced by stripes of the main ray's hit position
    (origin + t * direction, centre-relative world metres), so the image is purely the rays' geometry. If the stripes
    sit at the reflected depth in the headset the rays are right and the painted-on image comes from the colour they
    fetch; if they are painted on too, the rays (or what follows them) are.
    mode 'dir': stripes of the ray DIRECTION instead (about 10 degrees each; the eyes should differ by ~2.5 degrees on
    a car 1.5 m away), and green = 0.5 + 12 (offset.x - offset.y) of the eye offset the twin subtracts, so the two
    eyes' green levels show whether that offset differs between them at run time."""
    trace = re.search(r'(?m)^  call void @dx\.op\.traceRay\.struct\.Payload\(i32 157, %dx\.types\.Handle %\d+, i32 16, i32 \d+, i32 \d+, '
                      r'i32 \d+, i32 \d+, float ' + V + ', float ' + V + ', float ' + V + r', float \S+, float ' + V + ', float ' + V +
                      ', float ' + V + r', .*\n  ' + V + r' = load float, float\* %\d+.*$', ir)
    o, d, t = trace.group(1, 2, 3), trace.group(4, 5, 6), trace.group(7)
    block = int(re.findall(r'(?m)^; <label>:(\d+)', ir[:trace.start()])[-1])
    if mode == 'dir':
        off = '%ew.o' if '%ew.o0 = ' in ir else '%ew.a'
        calc = ['  %%hs.a = fadd fast float %s, %s' % (d[0], d[1]), '  %%hs.b6 = fadd fast float %%hs.a, %s' % d[2],
                '  %hs.sum = fmul fast float %hs.b6, 6.000000e+00',
                '  %%hs.od = fsub fast float %s0, %s1' % (off, off),
                '  %hs.g0 = call float @dx.op.tertiary.f32(i32 46, float %hs.od, float 1.200000e+01, float 5.000000e-01)']
    else:
        calc = ['  %%hs.h%d = call float @dx.op.tertiary.f32(i32 46, float %s, float %s, float %s)' % (c, t, d[c], o[c]) for c in range(3)]
        calc += ['  %hs.a = fadd fast float %hs.h0, %hs.h1', '  %hs.sum = fadd fast float %hs.a, %hs.h2']
    calc += ['  %hs.f = call float @dx.op.unary.f32(i32 22, float %hs.sum)',          # Frc: one stripe per metre
             '  %hs.s = fcmp fast olt float %hs.f, 5.000000e-01',
             '  %hs.r0 = select i1 %hs.s, float 2.000000e+00, float 0.000000e+00',
             '  %hs.b0 = select i1 %hs.s, float 2.000000e+00, float 0.000000e+00']
    if mode != 'dir':
        calc.append('  %hs.g0 = select i1 %hs.s, float 0.000000e+00, float 2.000000e+00')
    ir = ir[:trace.end()] + '\n' + '\n'.join(calc) + ir[trace.end():]
    # the RGB the store (or its YCoCg conversion) takes: select x, 0, rgb in the block both paths reach
    store = re.search(r'(?m)^  call void @dx\.op\.textureStore\.f32\(i32 67, %dx\.types\.Handle %\d+, i32 \S+, i32 \S+, i32 undef, '
                      r'float ' + V + ', .*$', ir)
    value = store.group(1)
    scaled = re.fullmatch(r'fmul fast float ' + V + ', ' + V, definition(ir, value))
    if scaled:                                         # the permutation scales the YCoCg first
        value = next(v for v in scaled.groups() if definition(ir, v).startswith('phi float'))
    phi = re.fullmatch(r'phi float \[ ' + V + r', %\d+ \], \[ ' + V + r', %\d+ \]', definition(ir, value))
    dot = re.fullmatch(r'call float @dx\.op\.dot3\.f32\(i32 55, float ' + V + ', float ' + V + ', float ' + V + ', .*',
                       definition(ir, phi.group(1)))
    rgb = list(dot.group(1, 2, 3))
    first = re.search(r'(?m)^  ' + re.escape(rgb[0]) + ' = ', ir)
    head = list(re.finditer(r'(?m)^; <label>:(\d+) +; preds = (.*)$', ir[:first.start()]))[-1]
    preds = re.findall(r'%(\d+)', head.group(2))
    phis = []
    for c, n in enumerate('rgb'):
        inc = ', '.join('[ %s, %%%s ]' % ('%%hs.%s0' % n if int(p) > block else '0.000000e+00', p) for p in preds)
        phis.append('  %%hs.%s = phi float %s' % (n, inc))
    ir = ir[:head.end()] + '\n' + '\n'.join(phis) + ir[head.end():]
    swap = {v: '%%hs.%s' % n for v, n in zip(rgb, 'rgb')}
    lines = ir.split('\n')
    start = next(i for i, l in enumerate(lines) if l.startswith('  ' + rgb[0] + ' = '))
    for i in range(start + 1, len(lines)):
        if 'dot3' in lines[i]:
            lines[i] = re.sub(r'float (%\d+)(?=[,)\s])', lambda m: 'float ' + swap.get(m.group(1), m.group(1)), lines[i])
        elif ' = phi float ' in lines[i]:
            lines[i] = re.sub(r'\[ (%\d+), ', lambda m: '[ ' + swap.get(m.group(1), m.group(1)) + ', ', lines[i])
        if 'textureStore' in lines[i]:
            break
    return '\n'.join(lines)


def ham_float(value):
    """An LLVM IR float literal (a double hex pattern exactly representable as float)."""
    return '0x%016X' % struct.unpack('<Q', struct.pack('<d', struct.unpack('<f', struct.pack('<f', value))[0]))[0]


def replace_dxil(original, rebuilt):
    """original's parts, with the DXIL part taken from rebuilt; re-signed."""
    def table(c):
        return {fourcc: bytes(c[start - 8:start + size]) for fourcc, start, size in dc.parts(c)}
    old, new = table(original), table(rebuilt)
    order = [fourcc for fourcc, _, _ in dc.parts(original)]
    chunks = [new[b'DXIL'] if f == b'DXIL' else new[b'HASH'] if f == b'HASH' and b'HASH' in new else old[f] for f in order]
    header = len(order) * 4 + 32
    offsets, at = [], header
    for chunk in chunks:
        offsets.append(at); at += len(chunk)
    out = bytearray(original[:32])
    struct.pack_into('<I', out, 24, at)
    struct.pack_into('<I', out, 28, len(order))
    out += struct.pack('<%dI' % len(order), *offsets)
    for chunk in chunks:
        out += chunk
    return dc.sign(out)


def build(original, scale=1.0, biased=True, tint=False, stripes=False, dirs=False):
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        ir, fog = fog_origin(eye_origin(ham.disassemble(original, work), scale, biased))
        if tint:
            ir = eye_tint(ir)
        if stripes or dirs:
            ir = hit_stripes(ir, 'dir' if dirs else 'hit')
        rebuilt = ham.assemble(ir, work)
    container = replace_dxil(original, rebuilt)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('container hash mismatch')
    return container, fog


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--shaders', type=Path, required=True, help='original libraries named lib_<fnv1a>.dxil')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--debug-scale', type=float, default=1.0, help='diagnostic only: multiply the eye offset')
    parser.add_argument('--debug-tint', type=lambda s: int(s, 16), nargs='*', default=[],
                        help='diagnostic only: these libraries (hex) store their reflection tinted by eye')
    parser.add_argument('--debug-stripes', type=lambda s: int(s, 16), nargs='*', default=[],
                        help='diagnostic only: these opaque raygens (hex) store stripes of their hit position')
    parser.add_argument('--debug-dirs', type=lambda s: int(s, 16), nargs='*', default=[],
                        help='diagnostic only: these opaque raygens (hex) store stripes of their ray direction')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    failed = 0
    for key, (name, biased) in LIBRARIES.items():
        source = args.shaders / ('lib_%016x.dxil' % key)
        if not source.exists():
            print('MISSING %016x %s' % (key, name)); failed += 1; continue
        original = source.read_bytes()
        if ham.fnv1a(original) != key:
            raise RuntimeError('%s does not hash to its name' % source)
        try:
            patched, fog = build(original, args.debug_scale, biased, key in args.debug_tint,
                                 key in args.debug_stripes, key in args.debug_dirs)
        except Exception as error:
            print('FAILED %016x %s: %s' % (key, name, error)); failed += 1; continue
        (args.out / ('%016x.dxil' % key)).write_bytes(patched)
        print('OK %016x %s: %d -> %d bytes, %d fog origin(s)' % (key, name, len(original), len(patched), fog))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
