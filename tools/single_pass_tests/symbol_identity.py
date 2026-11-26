"""Read PE CodeView and MSF7 PDB identities without loading executable code."""
from pathlib import Path
import struct
import uuid


def pe_identity(path):
    data = Path(path).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Not a PE image')
    sections, optional_size = struct.unpack_from('<H', data, pe + 6)[0], struct.unpack_from('<H', data, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from('<H', data, opt)[0]
    directory = opt + (112 if magic == 0x20b else 96)
    debug_rva, debug_size = struct.unpack_from('<II', data, directory + 6 * 8)
    size = struct.unpack_from('<I', data, opt + 56)[0]
    table = opt + optional_size
    debug = None
    for i in range(sections):
        _, virtual_size, rva, raw_size, raw = struct.unpack_from('<8sIIII', data, table + 40 * i)
        if rva <= debug_rva < rva + max(virtual_size, raw_size):
            debug = raw + debug_rva - rva
            break
    result = {'imageSize': size, 'timestamp': struct.unpack_from('<I', data, pe + 8)[0]}
    if debug is None:
        return result
    for entry in range(debug, debug + debug_size, 28):
        _, _, _, _, kind, length, _, offset = struct.unpack_from('<IIHHIIII', data, entry)
        if kind == 2 and length >= 24 and data[offset:offset + 4] == b'RSDS':
            result.update(guid=str(uuid.UUID(bytes_le=data[offset + 4:offset + 20])),
                          age=struct.unpack_from('<I', data, offset + 20)[0],
                          embeddedPdb=data[offset + 24:offset + length].split(b'\0', 1)[0].decode('utf-8', 'replace'))
            return result
    return result


def pdb_identity(path):
    # Stream 1 starts with version, signature, age and the 16-byte GUID.
    with Path(path).open('rb') as source:
        header = source.read(56)
        if not header.startswith(b'Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0'):
            raise ValueError('Unsupported PDB format (expected MSF7)')
        block_size, _, _, directory_bytes, _, block_map = struct.unpack_from('<6I', header, 32)
        count = (directory_bytes + block_size - 1) // block_size
        if block_size < 512 or count * 4 > block_size:
            raise ValueError('Invalid or unsupported PDB directory')
        source.seek(block_map * block_size)
        blocks = struct.unpack('<%dI' % count, source.read(count * 4))
        chunks = []
        for block in blocks:
            source.seek(block * block_size)
            chunks.append(source.read(block_size))
        directory = b''.join(chunks)[:directory_bytes]
        stream_count = struct.unpack_from('<I', directory)[0]
        if stream_count < 2:
            raise ValueError('Missing PDB info stream')
        sizes = struct.unpack_from('<%dI' % stream_count, directory, 4)
        at = 4 + 4 * stream_count
        if sizes[0] != 0xffffffff:
            at += 4 * ((sizes[0] + block_size - 1) // block_size)
        if sizes[1] < 28 or sizes[1] == 0xffffffff:
            raise ValueError('Invalid PDB info stream')
        first_block = struct.unpack_from('<I', directory, at)[0]
        source.seek(first_block * block_size)
        info = source.read(28)
        return {'guid': str(uuid.UUID(bytes_le=info[12:28])), 'age': struct.unpack_from('<I', info, 8)[0]}


def matching_pdb(image, pdb):
    expected, actual = pe_identity(image), pdb_identity(pdb)
    return bool(expected.get('guid') and expected['guid'] == actual['guid'] and expected['age'] == actual['age'])


def live_pe_identity(process, base):
    """CodeView identity from the mapped image, not a possibly replaced disk DLL."""
    import ctypes as c
    from ctypes import wintypes as w
    kernel = c.WinDLL('kernel32', use_last_error=True)
    kernel.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]

    def read(rva, size):
        data = c.create_string_buffer(size)
        actual = c.c_size_t()
        if not kernel.ReadProcessMemory(process, base + rva, data, size, c.byref(actual)) or actual.value != size:
            raise OSError(c.get_last_error(), 'Cannot read mapped PE identity')
        return data.raw

    pe = struct.unpack_from('<I', read(0, 64), 0x3c)[0]
    header = read(pe, 264)
    if header[:4] != b'PE\0\0':
        raise ValueError('Mapped module is not a PE image')
    magic = struct.unpack_from('<H', header, 24)[0]
    debug_rva, debug_bytes = struct.unpack_from('<II', header, 24 + (112 if magic == 0x20b else 96) + 6 * 8)
    result = {'imageSize': struct.unpack_from('<I', header, 24 + 56)[0]}
    if not debug_rva or not debug_bytes:
        return result
    if debug_bytes > 65536:
        raise ValueError('Oversized mapped debug directory')
    directory = read(debug_rva, debug_bytes)
    for at in range(0, len(directory) - 27, 28):
        _, _, _, _, kind, length, rva, _ = struct.unpack_from('<IIHHIIII', directory, at)
        if kind != 2 or length < 24 or not rva:
            continue
        record = read(rva, 24)
        if record[:4] == b'RSDS':
            result.update(guid=str(uuid.UUID(bytes_le=record[4:20])), age=struct.unpack_from('<I', record, 20)[0])
            return result
    return result
