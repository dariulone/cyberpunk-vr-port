"""Initial bytes of the external data symbols of COFF objects (a .obj, or every member of a .lib).

coff_data.py compare <cpp.obj> <rust.lib> [names.txt]   -- per symbol: size and bytes must agree
"""
import struct, sys

def coff_symbols(data):
    """{name: (bytes, size_hint)} for external symbols defined in data sections."""
    machine, nsec, _, symptr, nsym, optsz, _ = struct.unpack_from('<HHIIIHH', data, 0)
    if machine != 0x8664:
        return {}
    secs = []
    for i in range(nsec):
        name, vsize, vaddr, rawsize, rawptr, relptr, lnptr, nrel, nln, ch = struct.unpack_from('<8sIIIIIIHHI', data, 20 + optsz + i * 40)
        secs.append((name.rstrip(b'\0'), rawsize, rawptr, ch, nrel))
    strtab = symptr + nsym * 18
    syms = []
    i = 0
    while i < nsym:
        raw = data[symptr + i * 18: symptr + i * 18 + 18]
        name8, value, secnum, typ, sclass, naux = struct.unpack('<8sIhHBB', raw)
        if name8[:4] == b'\0\0\0\0':
            off = struct.unpack_from('<I', name8, 4)[0]
            end = data.index(b'\0', strtab + off)
            name = data[strtab + off:end].decode()
        else:
            name = name8.rstrip(b'\0').decode()
        syms.append((name, value, secnum, sclass, typ))
        i += 1 + naux
    out = {}
    for name, value, secnum, sclass, typ in syms:
        if sclass != 2 or secnum <= 0:   # IMAGE_SYM_CLASS_EXTERNAL, defined
            continue
        sname, rawsize, rawptr, ch, nrel = secs[secnum - 1]
        if ch & 0x20:   # code
            continue
        if not (ch & 0x40 or ch & 0x80):
            continue
        # size: up to the next symbol in this section, or the section's end
        later = sorted(v for n, v, s, c, t in syms if s == secnum and v > value)
        end = later[0] if later else rawsize
        if ch & 0x80 and rawptr == 0:
            body = bytes(end - value)
        else:
            body = data[rawptr + value: rawptr + end]
        out[name] = (body, end - value, nrel, sname.decode(errors='replace'))
    return out

def lib_members(data):
    assert data[:8] == b'!<arch>\n'
    pos = 8
    while pos + 60 <= len(data):
        hdr = data[pos:pos + 60]
        size = int(hdr[48:58].decode().strip())
        body = data[pos + 60: pos + 60 + size]
        name = hdr[:16].decode(errors='replace').strip()
        if name not in ('/', '//') and not name.startswith('/<'):
            yield body
        pos += 60 + size + (size & 1)

def load(path):
    data = open(path, 'rb').read()
    if data[:8] == b'!<arch>\n':
        out = {}
        for m in lib_members(data):
            out.update(coff_symbols(m))
        return out
    return coff_symbols(data)

if __name__ == '__main__':
    _, mode, cpp, rust = sys.argv[:4]
    names = None
    if len(sys.argv) > 4:
        names = [l.strip() for l in open(sys.argv[4]) if l.strip()]
    a, b = load(cpp), load(rust)
    keys = names if names else sorted(a)
    bad = 0
    for k in keys:
        if k not in a:
            print('MISSING in C++', k); bad += 1; continue
        if k not in b:
            print('MISSING in Rust', k); bad += 1; continue
        ab, asz, arel, asec = a[k]; bb, bsz, brel, bsec = b[k]
        n = len(bb)
        if asz < n or ab[:n] != bb or any(ab[n:]):
            print(f'DIFF {k}: cpp {asz}B {ab[:16].hex()} ({asec}) rust {bsz}B {bb[:16].hex()} ({bsec})'); bad += 1
    print(f'{len(keys)} compared, {bad} differences')
