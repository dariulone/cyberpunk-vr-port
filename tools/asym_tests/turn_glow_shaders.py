"""Pixel shaders that light up while the camera turns, made not to (docs: memory cpvr-lobby-cc-engrams).

The hologram materials (hologram.mt and its one-pass templates) light their sides by how far the camera turned
since the last frame:

    turn = clamp(90 * (1 - dot(forward, previous forward)), 0, 0.9)     b1 row 41, row 45 (Render/StereoShaderCamera.hpp:
                                                                         rows 40-42 the camera's axes, 44-46 last frame's)
    colour += turn * (the sides facing along the camera's right axis, b1 row 40, by +-0.8 steps x10)

On a monitor only a flick of the mouse turns the camera that far in a frame. In a headset the camera is the head,
and with the stick turning the body the inside of a hologram near the eyes -- the lobby construct's arms, the
side facing the body -- fills with its colours five times over, the faster the turn the more: "the inner part of
the arm lights up with a strange light when turning", whatever the material's values (2026-10-28, lobby).

Each twin reads the turn as none: the dot product of the two forward axes is 1. Nothing else changes.

Shaders come from the game's engine\\shader_final.cache (extract_shader_cache.py --kind ps); output:
<FNV-1a>.dxil signed containers for `view_dir\\` (Render/HiddenAreaShaders.cpp group 6, CyberpunkVR_ViewDirPatch:
material shaders taking the camera's axes for the view).

  py -3.13 -B turn_glow_shaders.py --shaders DIR_OF_PS --out OUT_DIR [--only HASH ...] [--scan]"""
import argparse
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hidden_area_tests'))
import ham_shaders as ham
import dxil_container as dc

CAMERA, ROW_FORWARD, ROW_PREVIOUS_FORWARD = 1, 41, 45


class Skip(Exception):
    pass


def handle(ir, register):
    found = re.findall(r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandle\(i32 57, i8 2, i32 \d+, i32 '
                       + str(register) + r', i1 false\)', ir)
    if len(found) != 1:
        raise Skip('cb%d handles: %d' % (register, len(found)))
    return found[0]


def components(ir, h, row):
    """Every extracted component of the row's loads: {value: component}."""
    out = {}
    for load in re.findall(r'(?m)^  (%\d+) = call %dx\.types\.CBufRet\.f32 @dx\.op\.cbufferLoadLegacy\.f32\(i32 59, '
                           r'%dx\.types\.Handle ' + re.escape(h) + r', i32 ' + str(row) + r'\)', ir):
        for v, k in re.findall(r'(?m)^  (%\d+) = extractvalue %dx\.types\.CBufRet\.f32 ' + re.escape(load)
                               + r', ([0-3])', ir):
            out[v] = int(k)
    return out


def patch(ir):
    """Returns (ir, number of turn terms removed)."""
    b1 = handle(ir, CAMERA)
    now, before = components(ir, b1, ROW_FORWARD), components(ir, b1, ROW_PREVIOUS_FORWARD)
    if not now or not before:
        raise Skip('no forward axes')
    count = 0
    for m in re.finditer(r'(?m)^  (%\d+) = call float @dx\.op\.dot4\.f32\(i32 56, ((?:float %\d+, ){7}float %\d+)\).*$',
                         ir):
        args = re.findall(r'%\d+', m.group(2))
        a, b = args[:4], args[4:]
        if ([now.get(v) for v in a] == [0, 1, 2, 3] and [before.get(v) for v in b] == [0, 1, 2, 3]) or \
           ([before.get(v) for v in a] == [0, 1, 2, 3] and [now.get(v) for v in b] == [0, 1, 2, 3]):
            ir = ir.replace(m.group(0), '  %s = fadd fast float 1.000000e+00, 0.000000e+00' % m.group(1), 1)
            count += 1
    if not count:
        raise Skip('no turn term')
    if 'call float @dx.op.dot4.f32(' not in ir:   # the validator rejects an unused declaration
        ir = re.sub(r'(?m)^declare float @dx\.op\.dot4\.f32\(.*\n', '', ir)
    return ir, count


def build(original):
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        ir, count = patch(ham.disassemble(original, work))
        container = ham.assemble(ir, work)
    if dc.dxil_hash(container) != container[4:20]:
        raise RuntimeError('validator output hash mismatch')
    return container, count


def scan(ll_dir):
    """The disassembled shaders (ll_all/ps) that have the turn term, without building anything."""
    found = []
    for p in sorted(ll_dir.glob('*.ll')):
        ir = p.read_text(encoding='utf-8', errors='replace')
        if 'dot4.f32' not in ir:
            continue
        try:
            _, count = patch(ir)
        except Skip:
            continue
        found.append(p.stem)
        print('turn term x%d: %s' % (count, p.stem))
    print('%d shaders' % len(found))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--shaders', type=Path, required=True)
    ap.add_argument('--out', type=Path)
    ap.add_argument('--only', nargs='*')
    ap.add_argument('--scan', action='store_true', help='--shaders is a folder of disassembled .ll: list matches')
    args = ap.parse_args()
    if args.scan:
        scan(args.shaders)
        return
    args.out.mkdir(parents=True, exist_ok=True)
    names = args.only or sorted(p.stem for p in args.shaders.glob('*.dxil'))
    done = failed = 0
    for name in names:
        original = (args.shaders / (name + '.dxil')).read_bytes()
        try:
            container, count = build(original)
        except Skip as e:
            if args.only:
                print('SKIP %s: %s' % (name, e))
            continue
        except Exception as e:
            failed += 1
            print('FAIL %s: %s' % (name, str(e)[-600:]))
            continue
        key = ham.fnv1a(original)
        if '%016x' % key != name.lower():
            print('note: %s hashes to %016x' % (name, key))
        (args.out / ('%016x.dxil' % key)).write_bytes(container)
        done += 1
        print('OK %s: %d turn term(s), %d -> %d bytes' % (name, count, len(original), len(container)))
    print('built %d, failed %d' % (done, failed))


if __name__ == '__main__':
    main()
