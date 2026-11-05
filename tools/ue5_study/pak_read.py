"""pak_read.py PAK [list FILTER | get NAME OUT]: read a UE5 v11 pak (unencrypted index) -- list its files or extract one."""
import struct
import sys

PAK = sys.argv[1]
f = open(PAK, 'rb')
f.seek(0, 2)
N = f.tell()
f.seek(N - 300)
tail = f.read()
mi = tail.rfind(struct.pack('<I', 0x5A6F12E1))
ver, ioff, isz = struct.unpack_from('<IQQ', tail, mi + 4)
comp_names = []
p = mi + 4 + 4 + 8 + 8 + 20
for k in range(5):
    nm = tail[p + 32 * k:p + 32 * (k + 1)].split(b'\0')[0].decode()
    comp_names.append(nm)
f.seek(ioff)
idx = f.read(isz)


class R:
    def __init__(s, b, o=0):
        s.b, s.o = b, o

    def u32(s):
        v = struct.unpack_from('<I', s.b, s.o)[0]; s.o += 4; return v

    def i32(s):
        v = struct.unpack_from('<i', s.b, s.o)[0]; s.o += 4; return v

    def u64(s):
        v = struct.unpack_from('<Q', s.b, s.o)[0]; s.o += 8; return v

    def fstr(s):
        n = s.i32()
        if n == 0:
            return ''
        if n < 0:
            raw = s.b[s.o:s.o - 2 * n]; s.o += -2 * n; return raw.decode('utf-16le').rstrip('\0')
        raw = s.b[s.o:s.o + n]; s.o += n; return raw.decode('utf-8', 'replace').rstrip('\0')


r = R(idx)
mount = r.fstr()
nentries = r.i32()
seed = r.u64()
has_ph = r.u32()
if has_ph:
    r.u64(); r.u64(); r.o += 20
has_fd = r.u32()
fd_off = fd_size = 0
if has_fd:
    fd_off = r.u64(); fd_size = r.u64(); r.o += 20
enc_size = r.i32()
encoded = idx[r.o:r.o + enc_size]
r.o += enc_size
f.seek(fd_off)
fd = R(f.read(fd_size))
files = {}
ndirs = fd.i32()
for _ in range(ndirs):
    d = fd.fstr()
    nf = fd.i32()
    for _ in range(nf):
        fn = fd.fstr()
        eo = fd.i32()
        files[(mount + d + fn).replace('../../../', '')] = eo


def decode(off):
    e = R(encoded, off)
    v = e.u32()
    bs = e.u32() if (v & 0x3f) == 0x3f else ((v & 0x3f) << 11)
    cm = (v >> 23) & 0x3f
    offset = e.u32() if v & (1 << 31) else e.u64()
    usize = e.u32() if v & (1 << 30) else e.u64()
    size = (e.u32() if v & (1 << 29) else e.u64()) if cm else usize
    encrypted = bool(v & (1 << 22))
    nb = (v >> 6) & 0xffff
    blocks = []
    if nb:
        # in-file header: FPakEntry serialized before the data
        hdr = 8 + 8 + 8 + 4 + 20 + (4 + nb * 16 if cm else 0) + 1 + 4
        if nb == 1 and not encrypted:
            blocks = [(hdr, hdr + size)]
        else:
            start = hdr
            for _ in range(nb):
                bsz = e.u32()
                blocks.append((start, start + bsz))
                start += (bsz + 15) // 16 * 16 if encrypted else bsz
    return dict(offset=offset, usize=usize, size=size, cm=cm, bs=bs, nb=nb, blocks=blocks, enc=encrypted)


if sys.argv[2] == 'list':
    for name in sorted(files):
        if len(sys.argv) < 4 or sys.argv[3].lower() in name.lower():
            print(name)
    print('comp', comp_names, 'files', len(files), file=sys.stderr)
else:
    name, out = sys.argv[3], sys.argv[4]
    info = decode(files[name])
    print(info, comp_names, file=sys.stderr)
    data = b''
    if info['cm']:
        import ctypes
        oo = ctypes.WinDLL('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/oo2ext_7_win64.dll')
        oo.OodleLZ_Decompress.restype = ctypes.c_int64
        left = info['usize']
        for a, b in info['blocks']:
            f.seek(info['offset'] + a)
            src = f.read(b - a)
            want = min(info['bs'] or info['usize'], left)
            dst = ctypes.create_string_buffer(want)
            got = oo.OodleLZ_Decompress(src, ctypes.c_int64(len(src)), dst, ctypes.c_int64(want), 1, 0, 0, None, 0,
                                        None, None, None, 0, 3)
            assert got == want, (got, want)
            data += dst.raw
            left -= want
    else:
        for a, b in info['blocks'] or [(0, 0)]:
            f.seek(info['offset'] + a)
            data += f.read(b - a)
    if not info['nb']:
        hdr = 8 + 8 + 8 + 4 + 20 + 1 + 4
        f.seek(info['offset'] + hdr)
        data = f.read(info['usize'])
    open(out, 'wb').write(data)
