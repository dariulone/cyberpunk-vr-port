"""iostore_read.py UTOC [list FILTER | get NAME OUT]: UE5 IoStore (v5, unencrypted, indexed) -- list package files by
the directory index, or extract one chunk (Oodle via the oo2ext DLL Cyberpunk ships)."""
import ctypes
import struct
import sys

UTOC = sys.argv[1]
b = open(UTOC, 'rb').read()
(ver,) = struct.unpack_from('<B', b, 16)
hdr, n, nblk, blksz_entry, ncm, cmlen, blocksize, dirsize, parts = struct.unpack_from('<9I', b, 20)
seeds = struct.unpack_from('<I', b, 84)[0]
noperf = struct.unpack_from('<I', b, 96)[0]
p = hdr
chunk_ids = [b[p + 12 * i:p + 12 * i + 12] for i in range(n)]
p += 12 * n


def u40(x):
    return int.from_bytes(x, 'big')


offlen = [(u40(b[p + 10 * i:p + 10 * i + 5]), u40(b[p + 10 * i + 5:p + 10 * i + 10])) for i in range(n)]
p += 10 * n
p += 4 * seeds + 4 * noperf
blocks = []
for i in range(nblk):
    e = b[p + 12 * i:p + 12 * i + 12]
    off = int.from_bytes(e[0:5], 'little')
    csz = int.from_bytes(e[5:8], 'little')
    usz = int.from_bytes(e[8:11], 'little')
    blocks.append((off, csz, usz, e[11]))
p += 12 * nblk
methods = [b[p + cmlen * i:p + cmlen * (i + 1)].split(b'\0')[0].decode() for i in range(ncm)]
p += cmlen * ncm
d = b[p:p + dirsize]
q = 0


def i32():
    global q
    v = struct.unpack_from('<i', d, q)[0]; q += 4; return v


def fstr():
    global q
    ln = i32()
    if ln < 0:
        s = d[q:q - 2 * ln].decode('utf-16le'); q += -2 * ln
    else:
        s = d[q:q + ln].decode('utf-8', 'replace'); q += ln
    return s.rstrip('\0')


mount = fstr()
nd = i32()
dirs = [struct.unpack_from('<4I', d, q + 16 * i) for i in range(nd)]
q += 16 * nd
nf = i32()
files = [struct.unpack_from('<3I', d, q + 12 * i) for i in range(nf)]
q += 12 * nf
ns = i32()
strings = [fstr() for _ in range(ns)]
NONE = 0xFFFFFFFF
paths = {}


def walk(di, prefix):
    name, child, sib, fe = dirs[di]
    here = prefix + (strings[name] + '/' if name != NONE else '')
    while fe != NONE:
        fname, nxt, user = files[fe]
        paths[here + strings[fname]] = user
        fe = nxt
    c = child
    while c != NONE:
        walk(c, here)
        c = dirs[c][2]


walk(0, mount)
if sys.argv[2] == 'list':
    for k in sorted(paths):
        if sys.argv[3].lower() in k.lower():
            print(k, offlen[paths[k]][1])
    print('methods', methods, 'files', len(paths), file=sys.stderr)
    sys.exit()
name, out = sys.argv[3], sys.argv[4]
idx = paths[name]
off, ln = offlen[idx]
oo = ctypes.WinDLL('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/oo2ext_7_win64.dll')
oo.OodleLZ_Decompress.restype = ctypes.c_int64
ucas = open(UTOC[:-5] + '.ucas', 'rb')
first, last = off // blocksize, (off + ln - 1) // blocksize
data = b''
for bi in range(first, last + 1):
    boff, csz, usz, m = blocks[bi]
    ucas.seek(boff)
    src = ucas.read((csz + 15) // 16 * 16)[:csz]
    if m == 0:
        data += src[:usz]
        continue
    dst = ctypes.create_string_buffer(usz)
    got = oo.OodleLZ_Decompress(src, ctypes.c_int64(csz), dst, ctypes.c_int64(usz), 1, 0, 0, None, 0, None, None, None,
                                0, 3)
    assert got == usz, (got, usz, methods[m - 1])
    data += dst.raw
start = off - first * blocksize
open(out, 'wb').write(data[start:start + ln])
print('wrote', ln, file=sys.stderr)
