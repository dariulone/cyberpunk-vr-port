"""A minimal binary glTF writer for the VR lobby's generated meshes (tools/vr_lobby_assets/make_lobby_scene.py).

Geometry is given in the game's frame -- X right, Y forward, Z up, metres -- and written in glTF's (Y up, -Z forward),
which is what WolvenKit's importer turns back: game (x, y, z) = glTF (x, -z, y).
"""
import json
import math
import struct


def _to_gltf(v):
    return (v[0], v[2], -v[1])


class Builder:
    """Triangles grouped by material; each group becomes one primitive (one submesh after import)."""

    def __init__(self):
        self.groups = {}

    def group(self, material):
        return self.groups.setdefault(material, {'pos': [], 'nrm': [], 'uv': [], 'tan': [], 'idx': []})

    def quad(self, material, a, b, c, d, uv=((0, 0), (1, 0), (1, 1), (0, 1))):
        """A quad a-b-c-d, counter-clockwise seen from the side it faces."""
        g = self.group(material)
        e1 = [b[i] - a[i] for i in range(3)]
        e2 = [d[i] - a[i] for i in range(3)]
        n = _norm(_cross(e1, e2))
        t = _norm(e1)
        base = len(g['pos'])
        for p, t_uv in zip((a, b, c, d), uv):
            g['pos'].append(tuple(p))
            g['nrm'].append(n)
            g['uv'].append(t_uv)
            g['tan'].append(t)
        g['idx'] += [base, base + 1, base + 2, base, base + 2, base + 3]

    def tri(self, material, a, b, c, uv=((0, 0), (1, 0), (0, 1))):
        """A triangle a-b-c, counter-clockwise seen from the side it faces."""
        g = self.group(material)
        e1 = [b[i] - a[i] for i in range(3)]
        e2 = [c[i] - a[i] for i in range(3)]
        n = _norm(_cross(e1, e2))
        t = _norm(e1)
        base = len(g['pos'])
        for p, t_uv in zip((a, b, c), uv):
            g['pos'].append(tuple(p))
            g['nrm'].append(n)
            g['uv'].append(t_uv)
            g['tan'].append(t)
        g['idx'] += [base, base + 1, base + 2]

    def prism(self, top, rim, center, radius, z_top, thickness, sides=8, rim_height=None):
        """A flat regular prism (a floating slab): top and bottom faces in `top`, the side band in `rim` -- the
        whole side, or only its upper rim_height when given (the rest in `top`)."""
        cx, cy = center
        zb = z_top - thickness
        ring = [(cx + radius * math.cos(2 * math.pi * (i + 0.5) / sides),
                 cy + radius * math.sin(2 * math.pi * (i + 0.5) / sides)) for i in range(sides)]
        for i in range(sides):
            a, b = ring[i], ring[(i + 1) % sides]
            self.tri(top, (cx, cy, z_top), (a[0], a[1], z_top), (b[0], b[1], z_top))
            self.tri(top, (cx, cy, zb), (b[0], b[1], zb), (a[0], a[1], zb))
            split = z_top - (rim_height if rim_height else thickness)
            self.quad(rim, (a[0], a[1], split), (b[0], b[1], split), (b[0], b[1], z_top), (a[0], a[1], z_top))
            if rim_height:
                self.quad(top, (a[0], a[1], zb), (b[0], b[1], zb), (b[0], b[1], split), (a[0], a[1], split))

    def dome(self, material, center, radius, z_bottom, rings=24, segments=64, v_horizon=0.88, v_zenith=0.0):
        """An inward-facing hemisphere (plus a skirt down to z_bottom): seen from inside. V runs from v_horizon at
        the horizon to v_zenith at the top, the skirt keeping v_horizon."""
        cx, cy = center

        def p(theta, elev):
            r = radius * math.cos(elev)
            return (cx + r * math.cos(theta), cy + r * math.sin(theta), radius * math.sin(elev))

        for s in range(segments):
            t0, t1 = 2 * math.pi * s / segments, 2 * math.pi * (s + 1) / segments
            u0, u1 = s / segments, (s + 1) / segments
            for k in range(rings):
                e0, e1 = (math.pi / 2) * k / rings, (math.pi / 2) * (k + 1) / rings
                v0 = v_horizon + (v_zenith - v_horizon) * k / rings
                v1 = v_horizon + (v_zenith - v_horizon) * (k + 1) / rings
                # Counter-clockwise seen from inside: the normal points at the centre.
                self.quad(material, p(t1, e0), p(t0, e0), p(t0, e1), p(t1, e1),
                          ((u1, v0), (u0, v0), (u0, v1), (u1, v1)))
            a, b = p(t0, 0), p(t1, 0)
            self.quad(material, (b[0], b[1], z_bottom), (a[0], a[1], z_bottom), a, b,
                      ((u1, v_horizon), (u0, v_horizon), (u0, v_horizon), (u1, v_horizon)))

    def disc(self, material, center, r_in, r_out, z, segments=64):
        """A flat ring facing up."""
        cx, cy = center
        for s in range(segments):
            t0, t1 = 2 * math.pi * s / segments, 2 * math.pi * (s + 1) / segments
            self.quad(material,
                      (cx + r_in * math.cos(t0), cy + r_in * math.sin(t0), z),
                      (cx + r_out * math.cos(t0), cy + r_out * math.sin(t0), z),
                      (cx + r_out * math.cos(t1), cy + r_out * math.sin(t1), z),
                      (cx + r_in * math.cos(t1), cy + r_in * math.sin(t1), z))

    def box(self, material, center, size, rot=None, uv_scale=1.0, faces='all'):
        """An oriented box: rot is a 3x3 row-major matrix whose columns are the box's axes in the game frame.
        UVs run in metres times uv_scale on every face."""
        rot = rot or ((1, 0, 0), (0, 1, 0), (0, 0, 1))
        hx, hy, hz = size[0] / 2, size[1] / 2, size[2] / 2

        def p(x, y, z):
            return tuple(center[i] + rot[i][0] * x + rot[i][1] * y + rot[i][2] * z for i in range(3))

        def uv(w, h):
            return ((0, 0), (w * uv_scale, 0), (w * uv_scale, h * uv_scale), (0, h * uv_scale))

        sides = {
            'top': (p(-hx, -hy, hz), p(hx, -hy, hz), p(hx, hy, hz), p(-hx, hy, hz), uv(size[0], size[1])),
            'bottom': (p(-hx, hy, -hz), p(hx, hy, -hz), p(hx, -hy, -hz), p(-hx, -hy, -hz), uv(size[0], size[1])),
            'front': (p(-hx, -hy, -hz), p(hx, -hy, -hz), p(hx, -hy, hz), p(-hx, -hy, hz), uv(size[0], size[2])),
            'back': (p(hx, hy, -hz), p(-hx, hy, -hz), p(-hx, hy, hz), p(hx, hy, hz), uv(size[0], size[2])),
            'left': (p(-hx, hy, -hz), p(-hx, -hy, -hz), p(-hx, -hy, hz), p(-hx, hy, hz), uv(size[1], size[2])),
            'right': (p(hx, -hy, -hz), p(hx, hy, -hz), p(hx, hy, hz), p(hx, -hy, hz), uv(size[1], size[2])),
        }
        for name, (a, b, c, d, t_uv) in sides.items():
            if faces == 'all' or name in faces:
                self.quad(material, a, b, c, d, t_uv)

    def write(self, path, mesh_name):
        blob = bytearray()
        views, accessors, prims, materials = [], [], [], []

        def add(data, fmt, count, comp, kind, target, minmax=None):
            while len(blob) % 4:
                blob.append(0)
            off = len(blob)
            for item in data:
                blob.extend(struct.pack(fmt, *item) if isinstance(item, tuple) else struct.pack(fmt, item))
            views.append({'buffer': 0, 'byteOffset': off, 'byteLength': len(blob) - off, 'target': target})
            acc = {'bufferView': len(views) - 1, 'componentType': comp, 'count': count, 'type': kind}
            if minmax:
                acc['min'], acc['max'] = minmax
            accessors.append(acc)
            return len(accessors) - 1

        meshes, nodes = [], []
        for i, (material, g) in enumerate(sorted(self.groups.items())):
            pos = [_to_gltf(v) for v in g['pos']]
            nrm = [_to_gltf(v) for v in g['nrm']]
            # A tangent may carry its bitangent sign as a fourth value (exported meshes do); generated ones don't.
            tan = [(*_to_gltf(v[:3]), v[3] if len(v) > 3 else 1.0) for v in g['tan']]
            uv = [(u, 1.0 - v) for u, v in g['uv']]
            lo = [min(v[k] for v in pos) for k in range(3)]
            hi = [max(v[k] for v in pos) for k in range(3)]
            attrs = {
                'POSITION': add(pos, '<3f', len(pos), 5126, 'VEC3', 34962, (lo, hi)),
                'NORMAL': add(nrm, '<3f', len(nrm), 5126, 'VEC3', 34962),
                'TANGENT': add(tan, '<4f', len(tan), 5126, 'VEC4', 34962),
                'TEXCOORD_0': add(uv, '<2f', len(uv), 5126, 'VEC2', 34962),
            }
            indices = add(g['idx'], '<I', len(g['idx']), 5125, 'SCALAR', 34963)
            materials.append({'name': material})
            # WolvenKit matches submeshes by name: one glTF mesh per submesh, submesh_NN_LOD_1.
            name = 'submesh_%02d_LOD_1' % i
            meshes.append({'name': name, 'primitives': [{'attributes': attrs, 'indices': indices, 'material': i}]})
            nodes.append({'name': name, 'mesh': i})
        doc = {
            'asset': {'version': '2.0', 'generator': 'CyberpunkVRPort tools/vr_lobby_assets'},
            'scene': 0, 'scenes': [{'name': mesh_name, 'nodes': list(range(len(nodes)))}],
            'nodes': nodes,
            'meshes': meshes,
            'materials': materials,
            'buffers': [{'byteLength': len(blob)}],
            'bufferViews': views, 'accessors': accessors,
        }
        js = json.dumps(doc, separators=(',', ':')).encode()
        js += b' ' * ((4 - len(js) % 4) % 4)
        while len(blob) % 4:
            blob.append(0)
        out = struct.pack('<III', 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(blob))
        out += struct.pack('<II', len(js), 0x4E4F534A) + js
        out += struct.pack('<II', len(blob), 0x004E4942) + bytes(blob)
        with open(path, 'wb') as f:
            f.write(out)
        return {m: len(g['idx']) // 3 for m, g in self.groups.items()}


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _norm(v):
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return tuple(x / n for x in v)


def rotation(yaw=0.0, pitch=0.0, roll=0.0):
    """Row-major rotation: yaw about Z, then pitch about X, then roll about Y (radians)."""
    cy, sy = math.cos(yaw), math.sin(yaw)
    cp, sp = math.cos(pitch), math.sin(pitch)
    cr, sr = math.cos(roll), math.sin(roll)
    rz = ((cy, -sy, 0), (sy, cy, 0), (0, 0, 1))
    rx = ((1, 0, 0), (0, cp, -sp), (0, sp, cp))
    ry = ((cr, 0, sr), (0, 1, 0), (-sr, 0, cr))
    return _mul(_mul(rz, rx), ry)


def _mul(a, b):
    return tuple(tuple(sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)) for i in range(3))
