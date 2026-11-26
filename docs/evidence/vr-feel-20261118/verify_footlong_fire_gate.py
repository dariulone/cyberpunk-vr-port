"""Check the identified M1911A1 prefab and selected ARM64 control-flow sites without running the app.

Usage: python -I -B verify_footlong_fire_gate.py path/to/game.apk path/to/footlong.base.hdog
Requires UnityPy (tested with 1.25.2). No network or extraction is required. Run without -O.
The assembly checks pin a static observation to one build; they do not measure elapsed game time.
"""
import argparse
from hashlib import sha256
from pathlib import Path
import struct
import sys
import zipfile


def offline(event, args):
    if event.startswith('socket.'):
        raise RuntimeError('This verifier only reads local files.')


def verified_member(apk, path, limit, digest):
    info = apk.getinfo(path)
    if info.file_size > limit:
        raise ValueError(f'Unexpected size: {path}')
    data = apk.read(info)
    if sha256(data).hexdigest() != digest:
        raise ValueError(f'Not the identified build: {path}')
    return data


def prefab_bytes(path):
    with path.open('rb') as f:
        assert f.read(4) == b'HDOG'
        f.seek(-8, 2)
        count, index = struct.unpack('<II', f.read(8))
        assert count == 2561  # Includes non-bundle records.
        f.seek(index)
        for _ in range(count):
            offset, size, length = struct.unpack('<IIB', f.read(9))
            name, kind = f.read(length), f.read(4)
            if name == b'footlong.base_M1911A1':
                assert kind == b'UABR' and size == 547662
                f.seek(offset)
                data = f.read(size)
                assert sha256(data).hexdigest() == 'b67e200bb4aeb260d17370dc44f65d70ef3f2d15ea96e46a78e3d9f348527f23'
                return data
    raise ValueError('No matching prefab')


def verify_prefab(path):
    import UnityPy
    env = UnityPy.load(prefab_bytes(path))
    obj = next(o for o in env.objects if o.path_id == 8972502190637715802)
    prefab = obj.read_typetree()
    assert prefab['m_Script'] == {'m_FileID': 1, 'm_PathID': -6239294602362215812} and prefab['m_Enabled']
    c = prefab['fireControlGroup']
    selectors = c['selectors']['selectors']
    modes = [(s['defaultIndex'], [p['type'] for p in s['selectorPositions']]) for s in selectors]
    assert (c['type'], c['isDoubleAction'], modes) == (3, 0, [(1, [2, 0]), (0, [0, 1])])
    assert abs(c['hammer']['dropTime'] - .010) < 1e-8
    for s in selectors:
        for p in s['selectorPositions']:
            for key in ('switchToTimeLR', 'switchFromTimeLR'):
                assert all(abs(v - .025) < 1e-8 for v in p[key].values())


def verify_code(so):
    header = struct.unpack_from('<Q', so, 32)[0]
    stride, count = struct.unpack_from('<HH', so, 54)
    segments = [struct.unpack_from('<IIQQQQQQ', so, header + i * stride) for i in range(count)]

    def word(va):
        seg = next(s for s in segments if s[0] == 1 and s[3] <= va and va + 4 <= s[3] + s[5])
        return struct.unpack_from('<I', so, seg[2] + va - seg[3])[0]

    def target(va, condition=False):
        w = word(va)
        if condition:  # b.eq imm19
            assert w & 0xff00001f == 0x54000000
            bits, operand = 19, w >> 5
        else:  # bl imm26
            assert w >> 26 == 0b100101
            bits, operand = 26, w
        sign = 1 << (bits - 1)
        signed = ((operand & (2 * sign - 1)) ^ sign) - sign
        return va + 4 * signed

    cyclic, fire, selector = 0x4e89688, 0x4de3d34, 0x4e906ec
    # Type 3: normalized state comparisons, set/read of the latch and post-Fire clear.
    offsets = (0x1560, 0x1578, 0x1584, 0x15a0, 0x15ac, 0x15b4, 0x16f4, 0x1818)
    assert [word(cyclic + p) for p in offsets] == [0x71000d1f, 0x1e222020, 0x1e212000, 0x1e212040,
                                                   0x52800029, 0x39054e69, 0x39454e69, 0x39054e7f]
    assert target(cyclic + 0x17a8) == 0x4e8b348 and target(cyclic + 0x1800) == fire
    # Virtual round state changes before Track(Shoot = 7); the later frame test skips effects only.
    assert target(fire + 0x160) == 0x4e6b13c and word(fire + 0x198) == 0x39008008
    assert word(fire + 0x1054) == 0x528000e1 and target(fire + 0x1064) == 0x4cac718
    assert target(fire + 0x106c) == target(fire + 0x1348) == 0x99a2b44
    assert target(fire + 0x1078, condition=True) == fire + 0x1358
    assert word(fire + 0x1070) == 0xb9409348 and word(fire + 0x1354) == 0xb9009340
    # An unchanged selector skips CurrentSwitchTime.
    assert target(selector + 0x6c, condition=True) == selector + 0xb4 and target(selector + 0x74) == 0x4e90c50


if __name__ == '__main__':
    if not __debug__:
        sys.exit('Run without Python optimization: this verifier uses assertions.')
    sys.addaudithook(offline)
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('apk', type=Path)
    p.add_argument('hdog', type=Path)
    args = p.parse_args()
    with zipfile.ZipFile(args.apk) as apk:
        so = verified_member(apk, 'lib/arm64-v8a/libil2cpp.so', 220_000_000,
                             'd211b0aea75a67b9a10854452c76f9c06d5a667abfad9b5caf3a1e3ccfdd0e4d')
        verified_member(apk, 'assets/bin/Data/Managed/Metadata/global-metadata.dat', 32_000_000,
                        '36b0500dbd4451f27540c2ffd4ee197076a76aea682763b9b19945763fc6fa4b')
    verify_prefab(args.hdog)
    verify_code(so)
    print('PASS: identified prefab, phase and selector data, state latch, Shoot before frame-effects check.')
    print('Elapsed time to next eligible or actual shot: not measured.')
