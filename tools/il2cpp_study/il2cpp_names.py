"""il2cpp_names.py: names and code addresses of an IL2CPP game, from its global-metadata.dat (version 29) and
GameAssembly.dll, without running anything of the game's or anyone else's.

Writes OUT/methods.tsv (image, namespace, type, method, parameter names, RVA or -) and OUT/fields.tsv (image,
namespace, type, field, offset). Method addresses come from each image's Il2CppCodeGenModule in the binary: found as the
data record whose first pointer is the image's name string, then {methodPointerCount, methodPointers}.

usage: py il2cpp_names.py GameAssembly.dll global-metadata.dat OUT
"""
import os
import struct
import sys


def read_pe(data):
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    nsec = struct.unpack_from('<H', data, pe + 6)[0]
    optsize = struct.unpack_from('<H', data, pe + 20)[0]
    base = struct.unpack_from('<Q', data, pe + 24 + 24)[0]
    sections = []
    off = pe + 24 + optsize
    for i in range(nsec):
        name = data[off:off + 8].rstrip(b'\0').decode()
        vsize, va, rawsize, rawptr = struct.unpack_from('<IIII', data, off + 8)
        sections.append((name, va, vsize, rawptr, rawsize))
        off += 40
    return base, sections


class Image:
    def __init__(self, path):
        self.data = open(path, 'rb').read()
        self.base, self.sections = read_pe(self.data)

    def rva_to_off(self, rva):
        for name, va, vsize, raw, rawsize in self.sections:
            if va <= rva < va + max(vsize, rawsize):
                return raw + rva - va if rva - va < rawsize else None
        return None

    def off_to_rva(self, off):
        for name, va, vsize, raw, rawsize in self.sections:
            if raw <= off < raw + rawsize:
                return va + off - raw
        return None

    def u64_at_rva(self, rva):
        off = self.rva_to_off(rva)
        return None if off is None else struct.unpack_from('<Q', self.data, off)[0]


def main():
    dll, meta_path, out = sys.argv[1:4]
    os.makedirs(out, exist_ok=True)
    m = open(meta_path, 'rb').read()
    sanity, version = struct.unpack_from('<Ii', m, 0)
    assert sanity == 0xFAB11BAF and version == 29, (hex(sanity), version)
    sec = struct.unpack_from('<62i', m, 8)
    S = {name: (sec[2 * i], sec[2 * i + 1]) for i, name in enumerate([
        'stringLiteral', 'stringLiteralData', 'string', 'events', 'properties', 'methods', 'parameterDefaultValues',
        'fieldDefaultValues', 'fieldAndParameterDefaultValueData', 'fieldMarshaledSizes', 'parameters', 'fields',
        'genericParameters', 'genericParameterConstraints', 'genericContainers', 'nestedTypes', 'interfaces',
        'vtableMethods', 'interfaceOffsets', 'typeDefinitions', 'images', 'assemblies', 'fieldRefs',
        'referencedAssemblies', 'attributeData', 'attributeDataRange', 'unresolvedVirtualCallParameterTypes',
        'unresolvedVirtualCallParameterRanges', 'windowsRuntimeTypeNames', 'windowsRuntimeStrings',
        'exportedTypeDefinitions'])}

    str_off = S['string'][0]

    def string(index):
        start = str_off + index
        return m[start:m.index(b'\0', start)].decode('utf-8', 'replace')

    def table(name, fmt):
        off, size = S[name]
        n = struct.calcsize(fmt)
        assert size % n == 0, (name, size, n)
        return [struct.unpack_from(fmt, m, off + i * n) for i in range(size // n)]

    images = table('images', '<10i')
    types = table('typeDefinitions', '<7iI8i8HII')
    methods = table('methods', '<5iIHHHH')
    params = table('parameters', '<iIi')
    fields = table('fields', '<iiI')
    nested = table('nestedTypes', '<i')

    # A nested type is named after the types that hold it.
    outer = {}
    for ti, t in enumerate(types):
        start, count = t[12], t[20]
        for k in range(count):
            outer[nested[start + k][0]] = ti

    def type_name(ti):
        t = types[ti]
        name = string(t[0])
        if ti in outer:
            return type_name(outer[ti]) + '/' + name
        return name

    pe = Image(dll)
    # The image names in the binary: rdata strings, then the data records that point at them.
    name_rva = {}
    for img in images:
        name = string(img[0]).encode() + b'\0'
        at = pe.data.find(b'\0' + name)
        while at >= 0:
            rva = pe.off_to_rva(at + 1)
            if rva is not None:
                name_rva.setdefault(name[:-1].decode(), []).append(rva)
            at = pe.data.find(b'\0' + name, at + 1)
    modules = {}
    wanted = {pe.base + rva: img for img, rvas in name_rva.items() for rva in rvas}
    data_secs = [s for s in pe.sections if s[0] in ('.data', '.rdata')]
    for sname, va, vsize, raw, rawsize in data_secs:
        for off in range(raw, raw + rawsize - 24, 8):
            ptr = struct.unpack_from('<Q', pe.data, off)[0]
            if ptr in wanted:
                count, pad, table_ptr = struct.unpack_from('<IIQ', pe.data, off + 8)
                if 0 < count < 200000 and pe.base < table_ptr < pe.base + 0x10000000:
                    modules[wanted[ptr]] = (count, table_ptr - pe.base)
    print('code modules found', len(modules), 'of', len(images))

    # Il2CppMetadataRegistration ends {fieldOffsetsCount, fieldOffsets, typeDefinitionsSizesCount,
    # typeDefinitionsSizes, ...}: both counts are the number of type definitions.
    field_offsets = None
    n = len(types)
    for sname, va, vsize, raw, rawsize in data_secs:
        for off in range(raw, raw + rawsize - 32, 8):
            a, pa, b, pb = struct.unpack_from('<QQQQ', pe.data, off)
            if a == n and b == n and pe.base < pa < pe.base + 0x10000000 and pe.base < pb < pe.base + 0x10000000:
                field_offsets = pa - pe.base
                break
        if field_offsets:
            break
    print('field offsets', hex(field_offsets) if field_offsets else None)

    def field_offset(ti, k):
        if not field_offsets:
            return None
        per_type = pe.u64_at_rva(field_offsets + 8 * ti)
        if not per_type:
            return None
        off = pe.rva_to_off(per_type - pe.base + 4 * k)
        return None if off is None else struct.unpack_from('<i', pe.data, off)[0]

    with open(os.path.join(out, 'methods.tsv'), 'w', encoding='utf-8') as fm, \
            open(os.path.join(out, 'fields.tsv'), 'w', encoding='utf-8') as ff:
        for img in images:
            iname = string(img[0])
            module = modules.get(iname)
            for ti in range(img[2], img[2] + img[3]):
                t = types[ti]
                ns, tn = string(t[1]), type_name(ti)
                for k in range(t[16]):   # method_count
                    md = methods[t[9] + k]
                    mname = string(md[0])
                    pnames = ','.join(string(params[md[3] + j][0]) for j in range(md[9])) if md[3] >= 0 else ''
                    rva = '-'
                    rid = md[5] & 0xFFFFFF
                    if module and 0 < rid <= module[0]:
                        ptr = pe.u64_at_rva(module[1] + 8 * (rid - 1))
                        if ptr:
                            rva = '%X' % (ptr - pe.base)
                    fm.write('%s\t%s\t%s\t%s\t%s\t%s\n' % (iname, ns, tn, mname, pnames, rva))
                for k in range(t[18]):   # field_count
                    fd = fields[t[8] + k]
                    fo = field_offset(ti, k)
                    ff.write('%s\t%s\t%s\t%s\t%s\n' % (iname, ns, tn, string(fd[0]), '-' if fo is None else '%X' % fo))
    print('done')


if __name__ == '__main__':
    main()
