"""Placed nodes of a 2.5 streaming sector (worldCooked*Node era), which no WolvenKit release reads.

    py -3.13 -B tools/patchday/sector25.py SECTOR...            one line per placed node (batch instances left out)
    py -3.13 -B tools/patchday/sector25.py --json OUT SECTOR... every placement as JSON

Layout (worked out on the q000 VR tutorial sectors, 2026-10-28):
- nodes = the chunks other than the worldStreamingSector itself and the entEntityInstanceData sub-objects, in
  file order;
- nodeData = buffer 0 (Kraken 'KARK' blob: magic, u32 raw size, stream), 144-byte records: position Vector4 @0,
  orientation quaternion (i j k r) @16, scale Vector3 @32, node index u16 @112, instance index u16 @128 (0xFFFF =
  none): the record's entry in its node's cooked data (a batch's mesh, a collision node's setup);
- a worldCookedStaticMeshNode keeps its batch in the chunk's native tail: VLQ byte length, then 33-byte records
  (u64 FNV1a64 of the lower-case mesh depot path, u64 appearance CName hash, f32 distance, ...); one nodeData record
  per placed mesh picks its record by the instance index;
- a worldCookedCollisionNode: native tail = VLQ byte length, 48-byte setups (u32 offset into compiledData @24, u16
  actor count @28); compiledData (its DataBuffer, Kraken) holds at each setup's offset its actors, 48 bytes each:
  world position as int32 fixed point (1/131072) @0, orientation quaternion @16, u16 first shape @32 (the shapes
  run on through the setup), u16 shape count @34 (collision_setups below).
Kraken is decoded in process by the game's own Oodle (bin/x64/oo2ext_7_win64.dll).
"""
import ctypes
import json
import struct
import sys
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cr2w_read  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
RECORD = 144
_oodle = None


def oodle():
    global _oodle
    if _oodle is None:
        _oodle = ctypes.WinDLL(str(GAME / 'bin/x64/oo2ext_7_win64.dll'))
        _oodle.OodleLZ_Decompress.restype = ctypes.c_int64
        _oodle.OodleLZ_Decompress.argtypes = [ctypes.c_char_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_int64] +             [ctypes.c_int] * 3 + [ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                                  ctypes.c_int64, ctypes.c_int]
        _oodle.OodleLZ_Compress.restype = ctypes.c_int64
        _oodle.OodleLZ_Compress.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_int,
                                            ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                                            ctypes.c_int64]
    return _oodle


def kraken(blob):
    if blob[:4] != b'KARK':
        return blob
    size = struct.unpack_from('<I', blob, 4)[0]
    out = ctypes.create_string_buffer(size)
    got = oodle().OodleLZ_Decompress(bytes(blob[8:]), len(blob) - 8, out, size, 1, 0, 0, None, 0, None, None, None,
                                     0, 3)
    if got != size:
        raise SystemExit('Kraken: %d of %d bytes' % (got, size))
    return out.raw


def kraken_pack(raw):
    """raw -> a 'KARK' blob (Kraken, Oodle's Normal level), as the game's buffers are stored."""
    out = ctypes.create_string_buffer(len(raw) + len(raw) // 8 + 65536)
    size = oodle().OodleLZ_Compress(8, bytes(raw), len(raw), out, 4, None, None, None, None, 0)
    if size <= 0:
        raise SystemExit('Kraken: compressing %d bytes failed' % len(raw))
    return b'KARK' + struct.pack('<I', len(raw)) + out.raw[:size]


def with_buffers(data, new):
    """The CR2W file `data` with buffers replaced: new = {buffer index: stored bytes, memory size}. The buffers run
    back to back from the objects' end to the file's end; the buffer table, the header's end of buffers and the
    CRCs (a table's over its bytes, the header's over its 160 bytes with 0xDEADBEEF in the CRC field) follow."""
    c = cr2w_read.Cr2w(data)
    objects_end, buffers_end = struct.unpack_from('<II', data, 24)
    if buffers_end != len(data):
        raise SystemExit('CR2W with data past its buffers')
    at, body, table = objects_end, b'', bytearray()
    for i, (flags, index, off, disk, mem, crc) in enumerate(c.buffers):
        if off != at:
            raise SystemExit('buffer %d at %d, expected %d' % (i, off, at))
        stored, size = new.get(i, (data[off:off + disk], mem))
        table += struct.pack('<IIIIII', flags, index, objects_end + len(body), len(stored), size, zlib.crc32(stored))
        body += stored
        at += disk
    out = bytearray(data[:objects_end]) + body
    table_off = c.tables[5][0]
    out[table_off:table_off + len(table)] = table
    struct.pack_into('<I', out, 40 + 12 * 5 + 8, zlib.crc32(table))
    struct.pack_into('<II', out, 28, len(out), 0xDEADBEEF)
    struct.pack_into('<I', out, 32, zlib.crc32(bytes(out[:160])))
    return bytes(out)


def fnv1a64(path):
    """The hash a cooked batch names its mesh by (lower-case depot path)."""
    h = 0xCBF29CE484222325
    for b in path.lower().encode('utf-8'):
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def vlq(d, at):
    """CR2W's variable-length number: 6 bits in the first byte (bit 6 = more), then 7 per byte (bit 7 = more)."""
    b = d[at]
    v, shift, at = b & 0x3F, 6, at + 1
    more = b & 0x40
    while more:
        b = d[at]
        v |= (b & 0x7F) << shift
        shift, at, more = shift + 7, at + 1, b & 0x80
    return v, at


def batch_records(c, i):
    """A worldCookedStaticMeshNode's batch: (mesh path hash, appearance hash, distance) per slot."""
    _cls, _f, _p, size, off, _t, _crc = c.chunks[i]
    try:
        _props, end = c.props(off, off + size)
    except cr2w_read.Raw:
        end = off
    length, at = vlq(c.d, end)
    if length % 33:
        raise SystemExit('batch tail of %d bytes is not whole 33-byte records' % length)
    return [struct.unpack_from('<QQf', c.d, at + 33 * k) for k in range(length // 33)]


def collision_nodes(data):
    """{node index: (compiledData buffer index, [(setup's offset into compiledData, actor count), ...])} for every
    worldCookedCollisionNode of the sector file `data`."""
    c = cr2w_read.Cr2w(data)
    nodes = [i for i in range(1, len(c.chunks)) if c.names[c.chunks[i][0]] != 'entEntityInstanceData']
    out = {}
    for index, ci in enumerate(nodes):
        if c.names[c.chunks[ci][0]] != 'worldCookedCollisionNode':
            continue
        _cls, _f, _p, size, off, _t, _crc = c.chunks[ci]
        props, end = c.props(off, off + size)
        buf = next(v['$buffer'] for name, _t2, v in props if name == 'compiledData')
        length, at = vlq(data, end)
        if length % 48:
            raise SystemExit('collision tail of %d bytes is not whole 48-byte setups' % length)
        out[index] = (buf, [struct.unpack_from('<IH', data, at + 48 * s + 24) for s in range(length // 48)])
    return out


# A collision setup's tail record (48 bytes): @0 the half size of its bounds (3 floats), @24 these counts.
SETUP_FIELDS = ('offset', 'actors', 'shapes', 'positions', 'rotations', 'scales', 'materials', 'presets',
                'material_indices', 'shape_indices', 'flags')
SETUP_PARTS = (('actors', 48), ('shapes', 32), ('positions', 12), ('rotations', 16), ('scales', 12),
               ('materials', 8), ('presets', 8))


def collision_layout(data):
    """{node index: (chunk index, compiledData buffer index, offset of its 48-byte setup records in data, count)}
    for every worldCookedCollisionNode of the sector file `data`."""
    c = cr2w_read.Cr2w(data)
    nodes = [i for i in range(1, len(c.chunks)) if c.names[c.chunks[i][0]] != 'entEntityInstanceData']
    out = {}
    for index, ci in enumerate(nodes):
        if c.names[c.chunks[ci][0]] != 'worldCookedCollisionNode':
            continue
        _cls, _f, _p, size, off, _t, _crc = c.chunks[ci]
        props, end = c.props(off, off + size)
        buf = next(v['$buffer'] for name, _t2, v in props if name == 'compiledData')
        length, at = vlq(data, end)
        if length % 48 or at + length != off + size:
            raise SystemExit('collision tail of %d bytes is not whole 48-byte setups to the chunk end' % length)
        out[index] = (ci, buf, at, length // 48)
    return out


def read_setup(cooked, record):
    """One setup's parts, verbatim, out of compiledData: actors (48 bytes each: int32 fixed-point position @0,
    orientation @16, u16 first shape @32, u16 shape count @34, u16 scale index @36 or 0xFFFF), shapes (32: a box's
    half extents or a geometry hash, then u16 position index @20 and rotation index @22, 0xFFFF = none), shape
    positions (Vector3), rotations (quaternion), scales (Vector3: an actor's own, over its shapes' sizes and
    offsets), materials and presets (8 bytes each), then 4 bytes per material index and one per shape index."""
    n = dict(zip(SETUP_FIELDS, struct.unpack_from('<IHHHHHHHHHH', record, 24)))
    at = n['offset']
    parts = {'bounds': list(struct.unpack_from('<3f', record, 0)), 'record': bytes(record)}
    for name, size in SETUP_PARTS:
        parts[name] = bytearray(cooked[at:at + size * n[name]])
        at += size * n[name]
    parts['indices'] = bytes(cooked[at:at + 4 * n['material_indices'] + n['shape_indices']])
    parts['end'] = at + len(parts['indices'])
    return parts


def write_setups(setups):
    """compiledData out of read_setup parts -- the setups back to back, each but the last padded to 16 bytes -- and
    their tail records with the offsets, the scale counts and the bounds written back."""
    out, records = bytearray(), []
    for k, p in enumerate(setups):
        rec = bytearray(p['record'])
        struct.pack_into('<3f', rec, 0, *p['bounds'])
        struct.pack_into('<I', rec, 24, len(out))
        struct.pack_into('<H', rec, 36, len(p['scales']) // 12)
        for name, _size in SETUP_PARTS:
            out += p[name]
        out += p['indices']
        if k < len(setups) - 1:
            out += bytes(-len(out) % 16)
        records.append(bytes(rec))
    return bytes(out), records


def rotate(q, v):
    """v turned by the quaternion q (i, j, k, r)."""
    x, y, z, w = q
    tx, ty, tz = 2 * (y * v[2] - z * v[1]), 2 * (z * v[0] - x * v[2]), 2 * (x * v[1] - y * v[0])
    return (v[0] + w * tx + y * tz - z * ty, v[1] + w * ty + z * tx - x * tz, v[2] + w * tz + x * ty - y * tx)


def shape_centres(parts):
    """Per actor of a setup, the world centres of its shapes: the actor's position plus its orientation applied to
    the shape's offset under the actor's scale."""
    positions = [struct.unpack_from('<3f', parts['positions'], 12 * k) for k in range(len(parts['positions']) // 12)]
    scales = [struct.unpack_from('<3f', parts['scales'], 12 * k) for k in range(len(parts['scales']) // 12)]
    out = []
    for a in range(len(parts['actors']) // 48):
        rec = 48 * a
        pos = [v / 131072 for v in struct.unpack_from('<3i', parts['actors'], rec)]
        quat = struct.unpack_from('<4f', parts['actors'], rec + 16)
        first, count, si = struct.unpack_from('<3H', parts['actors'], rec + 32)
        scale = scales[si] if si != 0xFFFF else (1.0, 1.0, 1.0)
        centres = []
        for k in range(first, first + count):
            pi = struct.unpack_from('<H', parts['shapes'], 32 * k + 20)[0]
            off = positions[pi] if pi != 0xFFFF else (0.0, 0.0, 0.0)
            d = rotate(quat, [off[i] * scale[i] for i in range(3)])
            centres.append(tuple(pos[i] + d[i] for i in range(3)))
        out.append(centres)
    return out


def qmul(a, b):
    """The quaternion product a * b (i, j, k, r)."""
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def shape_boxes(parts):
    """Per actor of a setup, (world centre, world half extents) of each of its shapes: a box under the actor's scale
    and both orientations, as the axis-aligned box round it; a mesh shape (a geometry hash, no extents) as
    (centre, None)."""
    rotations = [struct.unpack_from('<4f', parts['rotations'], 16 * k) for k in range(len(parts['rotations']) // 16)]
    scales = [struct.unpack_from('<3f', parts['scales'], 12 * k) for k in range(len(parts['scales']) // 12)]
    out = []
    for a, centres in enumerate(shape_centres(parts)):
        rec = 48 * a
        quat = struct.unpack_from('<4f', parts['actors'], rec + 16)
        first, _count, si = struct.unpack_from('<3H', parts['actors'], rec + 32)
        scale = scales[si] if si != 0xFFFF else (1.0, 1.0, 1.0)
        boxes = []
        for k, centre in enumerate(centres):
            s = 32 * (first + k)
            if struct.unpack_from('<2I', parts['shapes'], s + 8) == (0, 3):
                boxes.append((centre, None))
                continue
            ext = struct.unpack_from('<3f', parts['shapes'], s)
            ri = struct.unpack_from('<H', parts['shapes'], s + 22)[0]
            local = rotations[ri] if ri != 0xFFFF else (0.0, 0.0, 0.0, 1.0)
            axes = [rotate(local, [ext[i] if i == j else 0.0 for i in range(3)]) for j in range(3)]
            axes = [rotate(quat, [v[i] * scale[i] for i in range(3)]) for v in axes]
            boxes.append((centre, tuple(sum(abs(v[i]) for v in axes) for i in range(3))))
        out.append(boxes)
    return out


def float_props(c, ci, wanted):
    """{property name: offset of its value in the file} for the Float properties named in `wanted` that chunk ci
    stores (a property left at its default is not stored)."""
    _cls, _f, _p, size, off, _t, _crc = c.chunks[ci]
    at, end, out = off + 1, off + size, {}
    while at + 2 <= end:
        name = struct.unpack_from('<H', c.d, at)[0]
        if name == 0:
            break
        typ, length = struct.unpack_from('<HI', c.d, at + 2)
        if c.names[name] in wanted and c.names[typ] == 'Float':
            out[c.names[name]] = at + 8
        at += 4 + length
    return out


def collision_setups(path):
    """{node index: [setup actor positions, ...]} for every worldCookedCollisionNode: the setups in order (a nodeData
    record's instance index picks one), each the list of its actors' world positions (actor index = list index)."""
    data = Path(path).read_bytes()
    c = cr2w_read.Cr2w(data)
    out = {}
    for index, (buf, layout) in collision_nodes(data).items():
        b = c.buffers[buf]
        cooked = kraken(data[b[2]:b[2] + b[3]])
        setups = []
        for s, (start, count) in enumerate(layout):
            actors, shape = [], 0
            for a in range(count):
                rec = start + 48 * a
                first, shapes = struct.unpack_from('<2H', cooked, rec + 32)
                if first != shape:
                    raise SystemExit('%s: setup %d actor %d: shapes from %d, expected %d' % (path, s, a, first, shape))
                shape += shapes
                actors.append(tuple(v / 131072 for v in struct.unpack_from('<3i', cooked, rec)))
            setups.append(actors)
        out[index] = setups
    return out


def placements(path):
    data = Path(path).read_bytes()
    c = cr2w_read.Cr2w(data)
    chunks = [c.chunk(i) for i in range(len(c.chunks))]
    batches = {i: batch_records(c, i) for i, ch in enumerate(chunks) if ch['class'] == 'worldCookedStaticMeshNode'}
    nodes = [(i, ch) for i, ch in enumerate(chunks) if i and ch['class'] != 'entEntityInstanceData']
    off, size = c.buffers[0][2], c.buffers[0][3]
    raw = kraken(data[off:off + size])
    if len(raw) % RECORD:
        raise SystemExit('%s: nodeData %d bytes is not whole %d-byte records' % (path, len(raw), RECORD))
    out = []
    for k in range(len(raw) // RECORD):
        r = raw[k * RECORD:(k + 1) * RECORD]
        index = struct.unpack_from('<H', r, 112)[0]
        if index >= len(nodes):
            raise SystemExit('%s: node index %d past %d nodes' % (path, index, len(nodes)))
        ci, ch = nodes[index]
        props = {name: v for name, _t, v in ch['props']}
        ref = next((v['$ref'] for v in props.values() if isinstance(v, dict) and v.get('$ref')), None)
        slot = struct.unpack_from('<H', r, 128)[0]
        row = {'node': index, 'class': ch['class'], 'name': props.get('debugName'), 'ref': ref,
               'pos': struct.unpack_from('<3f', r, 0), 'quat': struct.unpack_from('<4f', r, 16),
               'scale': struct.unpack_from('<3f', r, 32), 'instance': None if slot == 0xFFFF else slot}
        if ci in batches:
            if slot != 0xFFFF:
                if slot >= len(batches[ci]):
                    raise SystemExit('%s: batch slot %d past %d records' % (path, slot, len(batches[ci])))
                row['mesh_hash'], row['app_hash'], row['dist'] = batches[ci][slot]
        out.append(row)
    return out


def main():
    args = sys.argv[1:]
    js = None
    if args[:1] == ['--json']:
        js, args = args[1], args[2:]
    every = {Path(p).stem: placements(p) for p in args}
    if js:
        Path(js).write_text(json.dumps(every), encoding='utf-8')
        print(sum(len(v) for v in every.values()), 'placements ->', js)
        return
    for name, rows in every.items():
        print('==', name, len(rows), 'placements')
        for r in rows:
            if r['class'] != 'worldCookedStaticMeshNode':
                print('  %-30s %-55s (%.1f %.1f %.1f) %s' % (r['class'], (r['name'] or '')[:55], *r['pos'],
                                                             (r['ref'] or '').rsplit('\\', 1)[-1]))


if __name__ == '__main__':
    main()
