"""2.5 engine stereo: the deferred ambient pass takes each eye's reflections from the eye that traced them further
(2026-10-23, the brick wall that was lit differently in the two eyes, shadowcheckSBS.rdc).

Under engine stereo (Stereo/EngineStereo.cpp, feature 98) both eyes are one side-by-side frame. The SSR trace
marches each pixel's ray in its own eye's half and stops at that half's edge, so near each eye's INNER edge
(where its lens window ends and the other eye's goes on) rays leave the picture, the confidence drops and the
ambient pass falls back to the probe -- brighter than the reflection there: the right eye's left edge and the
left eye's right edge are each lit differently from the same wall in the other eye (+0.3..+0.9 and
-0.5..-1.6 log2 in the ambient output, the middle ~0). The same surface sits well inside the OTHER eye's
picture, whose rays found what this eye's could not.

The ambient pass (one fullscreen draw per eye, b1 row 53.x = 1 for the left eye) reads the SSR colour (t8) and
confidence (t9) at its own pixel, then composites

    result = probe + (exposure * colour - probe) * confidence

The twin reprojects the pixel's world position (the shader's own, from the eye's inverse in b12 rows 81-84 /
101-104) into the other eye through that eye's world->clip rows (b12 93-96 left, 113-116 right; checked
against the inverses on the capture to a quarter pixel), and where the other eye shows the same surface
(reverse-Z depth within TOLERANCE) and found more, moves towards its result by

    w = saturate((c_other - c_own) * STRENGTH)
    c = lerp(c_own, c_other, w)       colour = ((1 - w) c_own colour_own + w c_other colour_other) / c

-- the composite of lerp(own result, other result, w), so where both eyes traced the same the eye keeps its
own reflection (and its parallax) and only what it lost comes from the other eye. First-person pixels (the
hands and weapon, their own inverse) and the mono branch (b0 row 32.x == 0, the VRCAM path) are left as
they are; so is every pixel at full confidence.

Output: <FNV-1a of the original>.dxil signed containers for `stereo_merge\\` (Render/HiddenAreaShaders.cpp
group 8, CyberpunkVR_StereoMergePatch).

  py -3.13 -B stereo_ssr_merge_shaders.py --shaders DIR_WITH_ORIGINAL_DXIL --out OUT_DIR
"""
import argparse
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
import ham_shaders as ham
import dxil_container as dc

# The deferred ambient pass and its permutations in staticshader_final.cache (every pixel shader there that has
# the structure below; 19fe... is the one shadowcheckSBS.rdc ran).
SHADERS = {
    0x19fe7d5422b1206e: 'deferred ambient + SSR composite (one draw per eye)',
    0x056eafbcc8ee2a87: 'deferred ambient, permutation',
    0x2188c71b84b45ded: 'deferred ambient, permutation',
    0x68de2bc2f8971207: 'deferred ambient, permutation',
    0x6a766457f5744bdd: 'deferred ambient, permutation',
    0x710492da1c504db5: 'deferred ambient, permutation',
    0xb39515454d8975e2: 'deferred ambient, permutation',
    0xe3e6c1d2d6391955: 'deferred ambient, permutation',
}
# The ambient permutations without the SSR confidence input (t9) -- ray-traced reflections, SSR off: only the eye's
# view direction (eye_view), no merge. Found 2026-11-02 by their view direction from b12 row 0 at three sites, as above.
EYE_VIEW_ONLY = {
    0x3e66e55a562d5a52: 'deferred ambient, no SSR confidence',
    0x4b924f246e7f942d: 'deferred ambient, no SSR confidence',
    0x500518a96e1cba40: 'deferred ambient, no SSR confidence',
    0x613d2190f89b9ceb: 'deferred ambient, no SSR confidence',
    0x63496fcbd6a9b65f: 'deferred ambient, no SSR confidence',
    0x6f28d5ae4d4a1685: 'deferred ambient, no SSR confidence',
    0x70072ec8d5754627: 'deferred ambient, no SSR confidence',
    0x734a51eb31b97ac2: 'deferred ambient, no SSR confidence',
    0x8d79729b92cdcb37: 'deferred ambient, no SSR confidence',
    0xb362a6144137bf2a: 'deferred ambient, no SSR confidence',
    0xc0ca3d215f422a0c: 'deferred ambient, no SSR confidence',
    0xd16064d3f90c3cb2: 'deferred ambient, no SSR confidence',
}
# The tiled deferred light pass (8x8 compute, one dispatch per eye skipping the other half; every permutation in
# staticshader_final.cache): its specular takes the view from b12 row 0 too. eye_view(per_eye_dispatch=True), no merge.
LIGHT_PASS = {
    0x04ca186039e1f34e: 'tiled deferred lights',
    0x0a188ae486c1eef8: 'tiled deferred lights',
    0x113c2749c98ad7d6: 'tiled deferred lights',
    0x1231a5401edacf84: 'tiled deferred lights',
    0x13c550fe8dfd3c32: 'tiled deferred lights',
    0x15ed1be3d59e72a5: 'tiled deferred lights',
    0x1620d1f0dbd804e0: 'tiled deferred lights',
    0x171c20751f35471e: 'tiled deferred lights',
    0x187f113cf7fc9653: 'tiled deferred lights',
    0x1d8f7c21d7457039: 'tiled deferred lights',
    0x250b40619b111d76: 'tiled deferred lights',
    0x26ea6cc91ceb804a: 'tiled deferred lights',
    0x2822a5f81319ea65: 'tiled deferred lights',
    0x296f164da9fceea8: 'tiled deferred lights',
    0x2a0cfd5774b58f65: 'tiled deferred lights',
    0x2b5b364af64bd4f2: 'tiled deferred lights',
    0x2f4dbb335d1b1ab1: 'tiled deferred lights',
    0x30f6d328fa8c31d0: 'tiled deferred lights',
    0x31fd050136bc33b3: 'tiled deferred lights',
    0x34cd47fdabad1f90: 'tiled deferred lights',
    0x351bcc486b0f279c: 'tiled deferred lights',
    0x3bab3100fe0554a4: 'tiled deferred lights',
    0x3c0f22f19b3770bf: 'tiled deferred lights',
    0x3e5efa82a6413060: 'tiled deferred lights',
    0x47e07195e41b0b52: 'tiled deferred lights',
    0x48e019a8a4f4f49b: 'tiled deferred lights',
    0x49a52c3c7d3b20ba: 'tiled deferred lights',
    0x4ab9421834b16ad3: 'tiled deferred lights',
    0x4d78accf008a43fa: 'tiled deferred lights',
    0x509eaa6834d851f1: 'tiled deferred lights',
    0x518f688f209059ee: 'tiled deferred lights',
    0x53826c45343a7ac5: 'tiled deferred lights',
    0x53b045bf20677bf4: 'tiled deferred lights',
    0x58913de9d8133f12: 'tiled deferred lights',
    0x593990d07dee318a: 'tiled deferred lights',
    0x60badcbbe10ccb7a: 'tiled deferred lights',
    0x61ece71943374575: 'tiled deferred lights',
    0x69dd486927ef075a: 'tiled deferred lights',
    0x6c2f960fb3000d03: 'tiled deferred lights',
    0x6de8fa9909ce0cc4: 'tiled deferred lights',
    0x6ee9d44b1f126fa7: 'tiled deferred lights',
    0x700f965763aba302: 'tiled deferred lights',
    0x71d4872370b2b310: 'tiled deferred lights',
    0x72afb8a8ff3fd522: 'tiled deferred lights',
    0x78fe3f496d1cd549: 'tiled deferred lights',
    0x7970773a4ced80b7: 'tiled deferred lights',
    0x7c7a320ecb9c935b: 'tiled deferred lights',
    0x7cc7f68e2351aabf: 'tiled deferred lights',
    0x7e0958a796e3dc38: 'tiled deferred lights',
    0x81bab508da15212d: 'tiled deferred lights',
    0x84304a307b32f307: 'tiled deferred lights',
    0x8db1acfb9ef13745: 'tiled deferred lights',
    0x91be4af1a22ccc3c: 'tiled deferred lights',
    0x94b4b93690170318: 'tiled deferred lights',
    0x9981fa314d57ae9d: 'tiled deferred lights',
    0x9e6716647f70e90f: 'tiled deferred lights',
    0xa53770c7eee682be: 'tiled deferred lights',
    0xaa2b44710ed9f24b: 'tiled deferred lights',
    0xabb5f7b834de1e87: 'tiled deferred lights',
    0xaeeedf4c7025879b: 'tiled deferred lights',
    0xbcf86efc0fa3216f: 'tiled deferred lights',
    0xbd42959f1de0db6e: 'tiled deferred lights',
    0xc0363c97d79ef345: 'tiled deferred lights',
    0xc77c4034f21291ca: 'tiled deferred lights',
    0xc8950391539df973: 'tiled deferred lights',
    0xc8c9edb54ff8955c: 'tiled deferred lights',
    0xcd3a88e26a441cc9: 'tiled deferred lights',
    0xd00d3355390a0877: 'tiled deferred lights',
    0xd07c92b9ad5c9a08: 'tiled deferred lights',
    0xd2d441cd660ca547: 'tiled deferred lights',
    0xd3a693108d260112: 'tiled deferred lights',
    0xdf6fbcab9cd07625: 'tiled deferred lights',
    0xe18d1ee008432317: 'tiled deferred lights',
    0xedd7e5425573ffdd: 'tiled deferred lights',
    0xf3b9aefb9e0bce6c: 'tiled deferred lights',
    0xf92b3fc7330d0e97: 'tiled deferred lights',
    0xf947083084eb9d62: 'tiled deferred lights',
    0xfe24d3c94704ca87: 'tiled deferred lights',
}
TOLERANCE = 0.03       # reverse-Z depth, relative: the same surface
STRENGTH = 4.0         # all the way to the other eye at a quarter more confidence
FORWARD_ROWS = (93, 113)   # b12 world->clip: left eye, right eye (rows k..k+3 = clip x, y, z, w)
INVERSE_ROWS = (81, 101)   # b12 pixel->world the shader picks by eye (the twin only checks they are read)
EYE_ROW = 53           # b1: x = 1 for the left eye's draw

HANDLE = r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandle\(i32 57, i8 {cls}, i32 \d+, i32 {reg}, i1 false\)'
F = '%dx.types.CBufRet.f32'
I = '%dx.types.CBufRet.i32'


def handle(ir, cls, register):
    found = re.findall(HANDLE.format(cls=cls, reg=register), ir)
    if len(found) != 1:
        raise ValueError('expected one handle for class %d register %d, found %d' % (cls, register, len(found)))
    return found[0]


def one(pattern, ir, what):
    found = re.findall(pattern, ir)
    if len(found) != 1:
        raise ValueError('%s: %d matches' % (what, len(found)))
    return found[0]


def texture_load(ir, srv, what):
    return one(r'(?m)^  (%\d+) = call %dx\.types\.ResRet\.f32 @dx\.op\.textureLoad\.f32\(i32 66, %dx\.types\.Handle ' +
               re.escape(srv) + r', i32 0, i32 (%\d+), i32 (%\d+), i32 undef', ir, what)


def extract(ir, value, comp, kind='%dx.types.ResRet.f32'):
    return one(r'(?m)^  (%\d+) = extractvalue ' + re.escape(kind) + ' ' + re.escape(value) + ', %d$' % comp, ir,
               'component %d of %s' % (comp, value))


def merge(ir):
    t0, t8, t9 = (handle(ir, 0, r) for r in (0, 8, 9))
    b0, b1, b12 = (handle(ir, 2, r) for r in (0, 1, 12))
    for row in INVERSE_ROWS:
        if not re.search(r'(?m)^  %\d+ = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, %dx\.types\.Handle ' +
                         re.escape(b12) + ', i32 %d\\)' % row, ir):
            raise ValueError('b12 row %d is not read: not the block layout the twin knows' % row)
    if not re.search(r'(?m)^  %\d+ = call ' + re.escape(I) + r' @dx\.op\.cbufferLoadLegacy\.i32\(i32 59, %dx\.types\.Handle ' +
                     re.escape(b1) + ', i32 %d\\)' % EYE_ROW, ir):
        raise ValueError('b1 row %d (the eye) is not read' % EYE_ROW)

    # The confidence at the pixel, the branch that loads the colour only where it is > 0, and its join.
    conf_load, x, y = texture_load(ir, t9, 'SSR confidence load')
    conf = extract(ir, conf_load, 0)
    test = one(r'(?m)^  (%\d+) = fcmp fast ogt float ' + re.escape(conf) + r', 0\.000000e\+00$', ir, 'confidence test')
    colour_block, join = one(r'(?m)^  br i1 ' + re.escape(test) + r', label %(\d+), label %(\d+)', ir, 'colour branch')
    colour_load, cx, cy = texture_load(ir, t8, 'SSR colour load')
    if (cx, cy) != (x, y):
        raise ValueError('colour and confidence read different pixels')
    own = []
    for comp in range(3):
        value = extract(ir, colour_load, comp)
        own.append(one(r'(?m)^  (%\d+) = phi float \[ ' + re.escape(value) + ', %' + colour_block +
                       r' \], \[ 0\.000000e\+00, %\d+ \]$', ir, 'colour phi %d' % comp))
    depth_load, dx, dy = texture_load(ir, t0, 'depth load')
    if (dx, dy) != (x, y):
        raise ValueError('depth read at another pixel')
    depth = extract(ir, depth_load, 0)

    # Mono: b0 row 32.x == 0, the test in the entry block (read again further on).
    entry = ir[:ir.index('\n; <label>:')]
    flag_load = one(r'(?m)^  (%\d+) = call ' + re.escape(I) + r' @dx\.op\.cbufferLoadLegacy\.i32\(i32 59, %dx\.types\.Handle ' +
                    re.escape(b0) + r', i32 32\)', entry, 'stereo flag load in the entry block')
    flag = extract(ir, flag_load, 0, I)
    mono = one(r'(?m)^  (%\d+) = icmp eq i32 ' + re.escape(flag) + ', 0$', ir, 'mono test')

    # The world position: the mono inverse (b12 rows 69-72) ends in three fdivs by one w; their phis with the
    # stereo value, which is select(first person, FPP inverse, eye inverse).
    mono_start = ir.index(re.search(r'(?m)^  %\d+ = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, '
                                    r'%dx\.types\.Handle ' + re.escape(b12) + r', i32 69\)', ir).group(0))
    divs = re.findall(r'(?m)^  (%\d+) = fdiv fast float %\d+, (%\d+)$', ir[mono_start:])[:3]
    if len(divs) != 3 or len({w for _, w in divs}) != 1:
        raise ValueError('mono world position: no three fdivs by one w')
    world, fpp = [], None
    for value, _ in divs:
        stereo, mono_value = one(r'(?m)^  %\d+ = phi float \[ (%\d+), %\d+ \], \[ (' + re.escape(value) + r'), %\d+ \]$', ir,
                                 'world phi of ' + value)
        world.append(one(r'(?m)^  (%\d+) = phi float \[ ' + re.escape(stereo) + r', %\d+ \], \[ ' + re.escape(mono_value) +
                         r', %\d+ \]$', ir, 'world phi'))
        flag_fpp = one(r'(?m)^  ' + re.escape(stereo) + r' = select i1 (%\d+), float %\d+, float %\d+$', ir, 'first-person select')
        if fpp not in (None, flag_fpp):
            raise ValueError('world components picked by different selects')
        fpp = flag_fpp

    # Split the join after its phis: everything that followed moves to %sm.join, so its successors' phis name
    # %sm.join as their predecessor, and every later use of the confidence and colour takes the merged value.
    head = re.search(r'(?m)^; <label>:' + join + r' .*\n((?:  %\d+ = phi .*\n)+)', ir)
    if not head:
        raise ValueError('join block %s has no phis' % join)
    cut = head.end()
    tail = re.sub(r', %' + join + r' \]', ', %sm.join ]', ir[cut:])
    for old, new in zip([conf] + own, ['%sm.c', '%sm.r', '%sm.g', '%sm.b']):
        tail = re.sub(re.escape(old) + r'\b', new, tail)

    lit = ham.lit
    rows = ['  %%sm.row%d = add i32 %%sm.base, %d' % (k, k) for k in (1, 2, 3)]
    for k in range(4):
        index = '%sm.base' if k == 0 else '%%sm.row%d' % k
        rows.append('  %%sm.m%d = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 %s)' % (k, F, b12, index))
        rows += ['  %%sm.m%d%s = extractvalue %s %%sm.m%d, %d' % (k, 'xyzw'[c], F, k, c) for c in range(4)]
        rows.append('  %%sm.d%d = call float @dx.op.dot3.f32(i32 55, float %%sm.m%dx, float %%sm.m%dy, float %%sm.m%dz, '
                    'float %s, float %s, float %s)' % (k, k, k, k, world[0], world[1], world[2]))
        rows.append('  %%sm.c%d = fadd fast float %%sm.d%d, %%sm.m%dw' % (k, k, k))
    load = lambda name, srv: ('  %%sm.%s = call %%dx.types.ResRet.f32 @dx.op.textureLoad.f32(i32 66, %%dx.types.Handle %s, '
                              'i32 0, i32 %%sm.tx, i32 %%sm.ty, i32 undef, i32 undef, i32 undef, i32 undef)' % (name, srv))
    body = [
        '  %sm.stereo = xor i1 ' + mono + ', true',
        '  %sm.part = fcmp fast olt float ' + conf + ', 1.000000e+00',
        '  %sm.eye = xor i1 ' + fpp + ', true',
        '  %sm.near = fcmp fast ogt float ' + depth + ', 0.000000e+00',
        '  %sm.g0 = and i1 %sm.stereo, %sm.part',
        '  %sm.g1 = and i1 %sm.g0, %sm.eye',
        '  %sm.go = and i1 %sm.g1, %sm.near',
        '  br i1 %sm.go, label %sm.body, label %sm.join',
        '',
        'sm.body:                                          ; preds = %' + join,
        '  %%sm.dims = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 3)' % (F, b0),
        '  %%sm.w = extractvalue %s %%sm.dims, 0' % F,
        '  %%sm.h = extractvalue %s %%sm.dims, 1' % F,
        '  %sm.hw = fmul fast float %sm.w, 5.000000e-01',
        '  %%sm.eyerow = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (I, b1, EYE_ROW),
        '  %%sm.eyeval = extractvalue %s %%sm.eyerow, 0' % I,
        '  %sm.left = icmp eq i32 %sm.eyeval, 1',
        # the OTHER eye's world->clip
        '  %%sm.base = select i1 %%sm.left, i32 %d, i32 %d' % (FORWARD_ROWS[1], FORWARD_ROWS[0]),
    ] + rows + [
        '  %sm.iw = fdiv fast float 1.000000e+00, %sm.c3',
        '  %sm.nx = fmul fast float %sm.c0, %sm.iw',
        '  %sm.ny = fmul fast float %sm.c1, %sm.iw',
        '  %sm.nz = fmul fast float %sm.c2, %sm.iw',
        '  %sm.qw = fmul fast float %sm.hw, 5.000000e-01',
        '  %sm.qh = fmul fast float %sm.h, 5.000000e-01',
        '  %sm.nqh = fsub fast float -0.000000e+00, %sm.qh',
        '  %sm.ex = call float @dx.op.tertiary.f32(i32 46, float %sm.nx, float %sm.qw, float %sm.qw)',
        '  %sm.ey = call float @dx.op.tertiary.f32(i32 46, float %sm.ny, float %sm.nqh, float %sm.qh)',
        '  %sm.v0 = fcmp fast ogt float %sm.c3, 0.000000e+00',
        '  %sm.v1 = fcmp fast oge float %sm.ex, 0.000000e+00',
        '  %sm.v2 = fcmp fast olt float %sm.ex, %sm.hw',
        '  %sm.v3 = fcmp fast oge float %sm.ey, 0.000000e+00',
        '  %sm.v4 = fcmp fast olt float %sm.ey, %sm.h',
        '  %sm.v01 = and i1 %sm.v0, %sm.v1',
        '  %sm.v23 = and i1 %sm.v2, %sm.v3',
        '  %sm.v03 = and i1 %sm.v01, %sm.v23',
        '  %sm.inside = and i1 %sm.v03, %sm.v4',
        '  %sm.hw1 = fadd fast float %sm.hw, -1.000000e+00',
        '  %sm.h1 = fadd fast float %sm.h, -1.000000e+00',
        '  %sm.exa = call float @dx.op.binary.f32(i32 35, float %sm.ex, float 0.000000e+00)',
        '  %sm.exc = call float @dx.op.binary.f32(i32 36, float %sm.exa, float %sm.hw1)',
        '  %sm.eya = call float @dx.op.binary.f32(i32 35, float %sm.ey, float 0.000000e+00)',
        '  %sm.eyc = call float @dx.op.binary.f32(i32 36, float %sm.eya, float %sm.h1)',
        # the other eye's half: the right one for the left eye
        '  %sm.off = select i1 %sm.left, float %sm.hw, float 0.000000e+00',
        '  %sm.sx = fadd fast float %sm.exc, %sm.off',
        '  %sm.tx = fptoui float %sm.sx to i32',
        '  %sm.ty = fptoui float %sm.eyc to i32',
        load('dl', t0),
        '  %sm.dq = extractvalue %dx.types.ResRet.f32 %sm.dl, 0',
        load('cl', t9),
        '  %sm.cq = extractvalue %dx.types.ResRet.f32 %sm.cl, 0',
        load('ql', t8),
        '  %sm.q0 = extractvalue %dx.types.ResRet.f32 %sm.ql, 0',
        '  %sm.q1 = extractvalue %dx.types.ResRet.f32 %sm.ql, 1',
        '  %sm.q2 = extractvalue %dx.types.ResRet.f32 %sm.ql, 2',
        '  %sm.dd = fsub fast float %sm.dq, %sm.nz',
        '  %sm.ad = call float @dx.op.unary.f32(i32 6, float %sm.dd)',
        '  %%sm.lim = fmul fast float %%sm.nz, %s' % lit(TOLERANCE),
        '  %sm.same = fcmp fast olt float %sm.ad, %sm.lim',
        '  %sm.ok = and i1 %sm.inside, %sm.same',
        '  %sm.gain = fsub fast float %sm.cq, ' + conf,
        '  %%sm.ws = fmul fast float %%sm.gain, %s' % lit(STRENGTH),
        '  %sm.wsat = call float @dx.op.unary.f32(i32 7, float %sm.ws)',
        '  %sm.wt = select i1 %sm.ok, float %sm.wsat, float 0.000000e+00',
        '  %sm.has = fcmp fast ogt float %sm.wt, 0.000000e+00',
        '  %sm.fc = call float @dx.op.tertiary.f32(i32 46, float %sm.wt, float %sm.gain, float ' + conf + ')',
        '  %sm.keep = fsub fast float 1.000000e+00, %sm.wt',
        '  %sm.ao = fmul fast float %sm.keep, ' + conf,
        '  %sm.bq = fmul fast float %sm.wt, %sm.cq',
        '  %sm.ic = fdiv fast float 1.000000e+00, %sm.fc',
    ]
    for comp, name in enumerate('rgb'):
        body += [
            '  %%sm.p%s = fmul fast float %%sm.ao, %s' % (name, own[comp]),
            '  %%sm.s%s = call float @dx.op.tertiary.f32(i32 46, float %%sm.bq, float %%sm.q%d, float %%sm.p%s)' % (name, comp, name),
            '  %%sm.u%s = fmul fast float %%sm.s%s, %%sm.ic' % (name, name),
            '  %%sm.m%s = select i1 %%sm.has, float %%sm.u%s, float %s' % (name, name, own[comp]),
        ]
    body += [
        '  %sm.mc = select i1 %sm.has, float %sm.fc, float ' + conf,
        '  br label %sm.join',
        '',
        'sm.join:                                          ; preds = %sm.body, %' + join,
        '  %%sm.c = phi float [ %s, %%%s ], [ %%sm.mc, %%sm.body ]' % (conf, join),
    ] + ['  %%sm.%s = phi float [ %s, %%%s ], [ %%sm.m%s, %%sm.body ]' % (name, own[comp], join, name)
         for comp, name in enumerate('rgb')]
    return ir[:cut] + '\n'.join(body) + '\n' + tail


def eye_view(ir, per_eye_dispatch=False):
    """The direction to the viewer from the pixel's own eye (2026-11-02: puddles seen from above reflected the
    probe painted onto their surface in both eyes; measured at the road's disparity, SSR and RT off).

    The ambient pass builds its view direction normalize(b12 row 0 - P), P the pixel's world position from its
    eye's inverse -- and b12 row 0 is ONE position for both eyes: the engine's stereo centre (the same shader picks
    it over the eye's b1 row 36 for a distance when b0 row 32.x != 0). Both eyes then reflect the same direction at
    a surface point, so the probe's reflection (all of it where the SSR missed: looking down, the rays leave the
    picture) sits on the surface instead of behind it. In the stereo branch the twin takes b1 row 36 instead, the
    eye this draw is for (one draw per eye); the mono branch keeps row 0.

    per_eye_dispatch: the tiled light pass (LIGHT_PASS), one dispatch per eye that skips the other eye's half when
    b1 row 53.x is 1 or 2. Its specular takes the same centre origin (a metal trim's highlight painted on in both
    eyes); there the eye's row 36 applies when b0 row 32.x != 0 and the kind is an eye, tested where it is used."""
    b0, b1, b12 = (handle(ir, 2, r) for r in (0, 1, 12))
    if per_eye_dispatch:
        mono = None
    else:
        entry = ir[:ir.index('\n; <label>:')]
        flag_load = one(r'(?m)^  (%\d+) = call ' + re.escape(I) + r' @dx\.op\.cbufferLoadLegacy\.i32\(i32 59, %dx\.types\.Handle ' +
                        re.escape(b0) + r', i32 32\)', entry, 'stereo flag load in the entry block')
        mono = one(r'(?m)^  (%\d+) = icmp eq i32 ' + re.escape(extract(ir, flag_load, 0, I)) + ', 0$', entry, 'mono test')
    # The view origin: the b12 row 0 load whose xyz are subtracted from something (the first one; a later one,
    # inside the stereo branch, is the centre's distance and stays).
    load = re.search(r'(?m)^  (%\d+) = call ' + re.escape(F) + r' @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, %dx\.types\.Handle ' +
                     re.escape(b12) + r', i32 0\)', ir)
    if not load:
        raise ValueError('b12 row 0 is not read')
    origin = [extract(ir, load.group(1), c, F) for c in range(3)]
    last = re.search(r'(?m)^  ' + re.escape(origin[2]) + r' = .*$', ir)
    uses = 0
    for value in origin:
        if not re.search(r'(?m)^  %\d+ = fsub fast float ' + re.escape(value) + r', %\d+$', ir):
            raise ValueError('b12 row 0 is not used as an origin (%s)' % value)
    insert = ['  %%ev.row = call %s @dx.op.cbufferLoadLegacy.f32(i32 59, %%dx.types.Handle %s, i32 36)' % (F, b1)]
    if mono is None:
        insert += [
            '  %%ev.flag = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 32)' % (I, b0),
            '  %%ev.flagx = extractvalue %s %%ev.flag, 0' % I,
            '  %%ev.kind = call %s @dx.op.cbufferLoadLegacy.i32(i32 59, %%dx.types.Handle %s, i32 %d)' % (I, b1, EYE_ROW),
            '  %%ev.kindx = extractvalue %s %%ev.kind, 0' % I,
            '  %ev.two = icmp ne i32 %ev.flagx, 0',
            '  %ev.eye = icmp ne i32 %ev.kindx, 0',
            '  %ev.stereo = and i1 %ev.two, %ev.eye',
        ]
    else:
        insert += ['  %ev.stereo = xor i1 ' + mono + ', true']
    for c, value in enumerate(origin):
        insert += ['  %%ev.e%d = extractvalue %s %%ev.row, %d' % (c, F, c),
                   '  %%ev.o%d = select i1 %%ev.stereo, float %%ev.e%d, float %s' % (c, c, value)]
    head, tail = ir[:last.end()], ir[last.end():]
    for c, value in enumerate(origin):
        tail, n = re.subn(re.escape(value) + r'\b', '%%ev.o%d' % c, tail)
        uses += n
    if not uses:
        raise ValueError('b12 row 0 has no uses after its load')
    return head + '\n' + '\n'.join(insert) + tail


def build(original, with_merge=True, per_eye_dispatch=False):
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        ir = eye_view(ham.disassemble(original, work), per_eye_dispatch)
        container = ham.assemble(merge(ir) if with_merge else ir, work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('validator output hash mismatch')
    return container


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--shaders', type=Path, required=True, help='original shaders named <fnv1a>.dxil')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--forward', action='store_true',
                        help='build the forward material set (stereo_forward_shaders.txt; --shaders = shader_final.cache extract)')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    failed = 0
    if args.forward:
        rows = [line.split() for line in (Path(__file__).parent / 'stereo_forward_shaders.txt').read_text().splitlines()
                if line and not line.startswith('#')]
        work = [(int(h, 16), 'forward %s' % stage, mode == 'fresh') for h, stage, mode in rows]
    else:
        work = [(key, name, key in LIGHT_PASS) for key, name in
                list(SHADERS.items()) + list(EYE_VIEW_ONLY.items()) + list(LIGHT_PASS.items())]
    for key, name, per_eye_dispatch in work:
        source = args.shaders / ('%016x.dxil' % key)
        if not source.exists():
            print('MISSING %016x %s' % (key, name)); failed += 1; continue
        original = source.read_bytes()
        if ham.fnv1a(original) != key:
            raise RuntimeError('%s does not hash to its name' % source)
        try:
            patched = build(original, with_merge=key in SHADERS, per_eye_dispatch=per_eye_dispatch)
        except Exception as error:
            print('FAILED %016x %s: %s' % (key, name, error)); failed += 1; continue
        (args.out / ('%016x.dxil' % key)).write_bytes(patched)
        print('OK %016x %s: %d -> %d bytes' % (key, name, len(original), len(patched)))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
