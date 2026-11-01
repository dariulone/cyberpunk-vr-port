"""Exports the built lobby (art/vr_lobby/reference/lobby_reference.blend) for the game.

    blender --background art/vr_lobby/reference/lobby_reference.blend --python tools/vr_lobby_assets/lobby_export.py

Writes build/lobby-reference/export/: one .glb per chunk (WolvenKit's import turns each into a .mesh) and
manifest.json -- the chunks with their materials, the lights, the colliders and the moving blocks -- which
make_lobby_scene.py turns into materials and entities.

Chunks: everything near the player is one ('near', props included); the rest of the city is cut into eight
sectors round the plaza per detail band, so that the game can leave out what is behind the player; each moving
block is its own, in its own frame (its entity's animator moves it). A chunk whose material group would pass
65535 vertices is split. Geometry is in the scene's frame -- the player's at the start of the lobby, X right,
Y forward, Z up -- and the entities are spawned at that frame.

Colliders come from the objects' lobby_collide: 'box' = the solid's own box, 'stairs' = a ramp along the
flight, terraces with a stairwell = the parts round it plus a ramp in it. The plaza's ground is one large box.
In the tutorial hall (lobby_layout.PLACE) the ground is the hall's own floor: the near set carries the fence along
its edge instead (tutorial_lobby.py place -> build/lobby-tutorial/place.json, Night City coordinates, turned into
the frame of the spawn the lobby stands at).
"""
import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gltf  # noqa: E402
import lobby_layout as LL  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'build/lobby-reference/export'
CENTRE = Vector((0.0, 6.0, 0.0))
SECTORS = 8
MAX_VERTS = 60000
SKIP_COLLECTIONS = ('review / cameras', 'review / atmosphere', 'review / sky fill', 'lights')


def collections_of(o):
    return {c.name for c in o.users_collection}


def chunk_of(o):
    if o.get('lobby_motion') is not None:
        return 'moving_' + o.name.split()[-1]
    if o.name == 'ground':
        return 'ground'
    band = o.get('lobby_band')
    if band is None:
        # Props and their parts carry no band: they are the player's near set.
        return 'near'
    if band == LL.NEAR:
        return 'near'
    c = o.matrix_world.translation - CENTRE
    k = int(((math.atan2(c.x, c.y) + math.pi) / (2 * math.pi)) * SECTORS) % SECTORS
    return '%s_%d' % (band, k)


def plain(v):
    """Blender's ID properties as plain JSON values."""
    if hasattr(v, 'to_dict'):
        return {k: plain(x) for k, x in v.to_dict().items()}
    if hasattr(v, 'to_list'):
        return [plain(x) for x in v.to_list()]
    if isinstance(v, dict):
        return {k: plain(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)):
        return [plain(x) for x in v]
    return v


def material_name(m):
    return m.get('lobby_material', m.name) if m else 'concrete'


def box_uv(p, n):
    ax = max(range(3), key=lambda i: abs(n[i]))
    if ax == 2:
        u, v = p.x, p.y * (1 if n.z > 0 else -1)
    elif ax == 0:
        u, v = p.y * (1 if n.x > 0 else -1), p.z
    else:
        u, v = p.x * (-1 if n.y > 0 else 1), p.z
    return u / 3.0, v / 3.0


class Chunk:
    def __init__(self, name):
        self.name = name
        self.parts = [gltf.Builder()]
        self.frame = Matrix.Identity(4)      # scene -> chunk frame (moving blocks are in their own)

    def builder_for(self, material, verts):
        b = self.parts[-1]
        g = b.groups.get(material)
        if g is not None and len(g['pos']) + verts > MAX_VERTS:
            self.parts.append(gltf.Builder())
            b = self.parts[-1]
        return b


def add_object(chunk, o, dg):
    ev = o.evaluated_get(dg)
    try:
        me = ev.to_mesh()
    except RuntimeError:
        return
    if me is None or not me.polygons:
        ev.to_mesh_clear()
        return
    mw = chunk.frame @ o.matrix_world
    rot = mw.to_3x3().inverted().transposed()
    me.calc_loop_triangles()
    has_uv = bool(me.uv_layers)
    if has_uv:
        try:
            me.calc_tangents(uvmap=me.uv_layers.active.name)
        except RuntimeError:
            has_uv = False
    uv = me.uv_layers.active.data if has_uv else None
    normals = me.corner_normals
    mats = [material_name(s.material) for s in o.material_slots] or ['concrete']
    by_mat = {}
    for tri in me.loop_triangles:
        by_mat.setdefault(mats[min(tri.material_index, len(mats) - 1)], []).append(tri)
    for mat, tris in by_mat.items():
        b = chunk.builder_for(mat, len(tris) * 3)
        g = b.group(mat)
        for tri in tris:
            base = len(g['pos'])
            for li in tri.loops:
                loop = me.loops[li]
                p = mw @ me.vertices[loop.vertex_index].co
                n = (rot @ normals[li].vector).normalized()
                g['pos'].append(tuple(p))
                g['nrm'].append(tuple(n))
                if has_uv:
                    g['uv'].append(tuple(uv[li].uv))
                    t = (mw.to_3x3() @ loop.tangent).normalized()
                    g['tan'].append((t.x, t.y, t.z, loop.bitangent_sign))
                else:
                    g['uv'].append(box_uv(o.matrix_world @ me.vertices[loop.vertex_index].co, n))
                    t = n.orthogonal().normalized()
                    g['tan'].append((t.x, t.y, t.z, 1.0))
            g['idx'] += [base, base + 1, base + 2]
    ev.to_mesh_clear()


# ---------------------------------------------------------------- colliders

def box_collider(center, half, yaw=0.0, pitch=0.0):
    return {'center': [round(v, 4) for v in center], 'half': [round(v, 4) for v in half],
            'yaw': round(yaw, 6), 'pitch': round(pitch, 6)}


def tutorial_fence():
    """The fence round the tutorial hall's floor, in the lobby's frame: the spawn at the origin, its heading +Y (the
    game's heading h looks along (-sin h, cos h); X is right of it)."""
    place = json.loads((ROOT / 'build/lobby-tutorial/place.json').read_text(encoding='utf-8'))
    spawn, h = place['spawn']['position'], math.radians(place['spawn']['yaw'])
    fwd, right = (-math.sin(h), math.cos(h)), (math.cos(h), math.sin(h))
    out = []
    for cx, cy, cz, hx, hy, hz in place['fence']:
        d = (cx - spawn[0], cy - spawn[1])
        centre = (d[0] * right[0] + d[1] * right[1], d[0] * fwd[0] + d[1] * fwd[1], cz - spawn[2])
        out.append(box_collider(centre, (hx, hy, hz), yaw=-h))     # its Night City axes, seen from the spawn
    return out


def colliders_of(o):
    kind = o.get('lobby_collide', 'none')
    if kind == 'none' or o.get('lobby_motion') is not None:
        return []
    size = o.get('lobby_size')
    if size is None:
        return []
    w, d, h = size
    mw = o.matrix_world
    yaw = mw.to_euler().z
    base = mw.translation

    def world(lx, ly, lz):
        return mw @ Vector((lx, ly, lz))

    out = []
    if kind == 'stairs':
        p = plain(o.get('lobby_params', {}))
        steps, rise, run = p.get('steps', 1), p.get('rise', 0.18), p.get('run', 0.3)
        total_run, total_rise = steps * run, steps * rise
        slope = math.atan2(total_rise, total_run)
        length = math.hypot(total_run, total_rise)
        # A ramp under the nosings: its top face goes from the foot of the first riser to the top of the last.
        thick = 0.3
        c = world(0, 0, total_rise / 2) - Vector((0, 0, 0))
        n_up = Vector((0, -math.sin(slope), math.cos(slope)))
        local = Vector((0, 0, total_rise / 2)) - n_up * (thick / 2)
        out.append(box_collider(world(*local), (w / 2, length / 2, thick / 2), yaw, slope))
        return out
    stair = o.get('lobby_stair')
    if stair:
        st = plain(stair)
        x, sw, srun, sbase = st['x'], st['width'], st['run'], st['base']
        front = -d / 2
        left, right = x - sw / 2, x + sw / 2
        if left > -w / 2 + 0.01:
            out.append(box_collider(world((-w / 2 + left) / 2, 0, h / 2), ((left + w / 2) / 2, d / 2, h / 2), yaw))
        if right < w / 2 - 0.01:
            out.append(box_collider(world((right + w / 2) / 2, 0, h / 2), ((w / 2 - right) / 2, d / 2, h / 2), yaw))
        out.append(box_collider(world(x, front + srun + (d - srun) / 2, h / 2), (sw / 2, (d - srun) / 2, h / 2), yaw))
        if sbase > 0:
            out.append(box_collider(world(x, front + srun / 2, sbase / 2), (sw / 2, srun / 2, sbase / 2), yaw))
        slope = math.atan2(h - sbase, srun)
        length = math.hypot(srun, h - sbase)
        thick = 0.3
        n_up = Vector((0, -math.sin(slope), math.cos(slope)))
        local = Vector((x, front + srun / 2, sbase + (h - sbase) / 2)) - n_up * (thick / 2)
        out.append(box_collider(world(*local), (sw / 2, length / 2, thick / 2), yaw, slope))
        return out
    if kind in ('box', 'mesh'):
        out.append(box_collider(world(0, 0, h / 2), (w / 2, d / 2, h / 2), yaw))
    return out


PART_MIN = 0.015      # a prop's part thinner than this (a seam, a line, a glass) is in nobody's way


def part_collider(o):
    """A prop's part: its own box (the mesh's bounds in its frame), turned the way the part is -- a heading and a
    tilt about its X, which is all a box collider takes."""
    loc, rot, scale = o.matrix_world.decompose()
    pts = [Vector(v) for v in o.bound_box]
    lo = Vector([min(p[k] for p in pts) for k in range(3)])
    hi = Vector([max(p[k] for p in pts) for k in range(3)])
    half = [(hi[k] - lo[k]) / 2 * abs(scale[k]) for k in range(3)]
    if min(half) * 2 < PART_MIN:
        return []
    e = rot.to_euler('XYZ')         # R = Rz(yaw) Ry(0) Rx(pitch)
    if abs(e.y) > 1e-3:
        raise SystemExit('%s: a part turned about its Y cannot be a box collider' % o.name)
    return [box_collider(o.matrix_world @ ((lo + hi) / 2), half, e.z, e.x)]


def arc_colliders(o, arc):
    """A curved slab (the table): per layer [r0, r1, z0, z1] about `centre` from angle a0 to a1 (0 along +Y), in
    `segments` boxes, each the chord-to-outer-radius box of its piece of the ring."""
    cx, cy = arc['centre']
    a0, a1 = arc['angles']
    n = arc['segments']
    half_a = (a1 - a0) / (2 * n)
    yaw0 = o.matrix_world.to_euler('XYZ').z
    out = []
    for r0, r1, z0, z1 in arc['layers']:
        inner = r0 * math.cos(half_a)
        for i in range(n):
            a = a0 + (2 * i + 1) * half_a
            rc = (inner + r1) / 2
            local = Vector((cx + rc * math.sin(a), cy + rc * math.cos(a), (z0 + z1) / 2))
            out.append(box_collider(o.matrix_world @ local, (r1 * math.sin(half_a), (r1 - inner) / 2, (z1 - z0) / 2),
                                    yaw0 - a))
    return out


# ---------------------------------------------------------------- main

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for f in OUT.glob('*.glb'):
        f.unlink()
    dg = bpy.context.evaluated_depsgraph_get()
    scene = bpy.context.scene
    scene.frame_set(1)
    chunks = {}
    colliders = {}
    motions = []
    items = []
    for o in scene.objects:
        if o.type not in ('MESH', 'FONT'):
            continue
        if collections_of(o) & set(SKIP_COLLECTIONS):
            continue
        if o.name.startswith('construct') or o.get('lobby_stand_in'):
            continue        # stands in for a game mesh (lobby_game_mesh on its parent: the instances below)
        if o.get('lobby_collider_only'):
            continue        # a prop's collision drawn by nothing (its box, with the props' below)
        if o.get('lobby_preview_only'):
            continue        # stands in for something the game draws itself (the VR menu on the terminal)
        if o.get('lobby_item') is not None:
            items.append(o)
            continue        # a body of its own (below)
        name = chunk_of(o)
        ch = chunks.get(name)
        if ch is None:
            ch = chunks[name] = Chunk(name)
            if name.startswith('moving_'):
                ch.frame = o.matrix_world.inverted()
        add_object(ch, o, dg)
        cl = colliders_of(o)
        if cl:
            key = 'near' if name == 'near' else name.split('_')[0] + '_' + name.split('_')[-1]
            colliders.setdefault(key, []).extend(cl)
        m = o.get('lobby_motion')
        if m is not None:
            mw = o.matrix_world
            m = plain(m)
            motions.append({'chunk': name, 'position': list(mw.translation), 'yaw': mw.to_euler().z,
                            'travel': m['travel'], 'spin': m['spin'], 'period': m['period'], 'phase': m['phase']})
    # The things to pick up: each its own chunk in its own frame (its origin, unscaled), with its collision shape --
    # the box round its geometry, or the ball in it -- its mass and where it rests.
    item_info = {}
    for i, o in enumerate(sorted(items, key=lambda o: o.name)):
        name = 'item_%d' % i
        ch = chunks[name] = Chunk(name)
        loc, rot, _ = o.matrix_world.decompose()
        ch.frame = Matrix.LocRotScale(loc, rot, None).inverted()
        add_object(ch, o, dg)
        local = Matrix.Diagonal((*o.matrix_world.to_scale(), 1.0))
        pts = [local @ Vector(v) for v in o.bound_box]
        lo = Vector([min(p[k] for p in pts) for k in range(3)])
        hi = Vector([max(p[k] for p in pts) for k in range(3)])
        item_info[name] = {'label': o.name, 'position': list(loc), 'rotation': [rot.w, rot.x, rot.y, rot.z],
                           'shape': o['lobby_item'], 'grasp': o.get('lobby_grasp'), 'mass': float(o['lobby_mass']),
                           'centre': list((lo + hi) / 2), 'half': list((hi - lo) / 2)}
        if o.get('lobby_grasp_half') is not None:
            # The part a hand holds (modules/grips.lua): its half sizes and middle in the item's frame.
            item_info[name]['grasp_half'] = plain(o['lobby_grasp_half'])
            item_info[name]['grasp_at'] = plain(o['lobby_grasp_at'])
    # The ground: the square kilometre, its top at 0 (the plaza's slab is flush with it) -- in the tutorial hall its
    # own floor, fenced.
    if LL.PLACE == 'tutorial':
        colliders.setdefault('near', []).extend(tutorial_fence())
    else:
        colliders.setdefault('near', []).append(box_collider((0, 0, -0.5), (LL.WORLD_HALF, LL.WORLD_HALF, 0.5)))
    # The props: a box per solid part, in the part's own frame; the curved table as boxes along its arc.
    for root in scene.objects:
        if root.get('lobby_prop') is None:
            continue
        for c in root.children_recursive:
            if c.type != 'MESH' or c.name.startswith('construct') or c.get('lobby_stand_in') or c.get('lobby_item') is not None                     or c.get('lobby_preview_only') or c.get('lobby_collide') == 'none':
                continue
            if c.get('lobby_arc') is not None:
                colliders['near'].extend(arc_colliders(c, json.loads(c['lobby_arc'])))
            else:
                colliders['near'].extend(part_collider(c))
    # The edge of the square kilometre.
    for k, (x, y, hx, hy) in enumerate(((0, LL.WORLD_HALF, LL.WORLD_HALF, 0.5), (0, -LL.WORLD_HALF, LL.WORLD_HALF, 0.5),
                                        (LL.WORLD_HALF, 0, 0.5, LL.WORLD_HALF), (-LL.WORLD_HALF, 0, 0.5, LL.WORLD_HALF))):
        if LL.PLACE != 'tutorial':
            colliders['near'].append(box_collider((x, y, 50.0), (hx, hy, 50.0)))
    # The game's own meshes placed in the scene (the pod's shell): the mesh, its appearance, where.
    instances = []
    for o in scene.objects:
        if o.get('lobby_game_mesh') is None:
            continue
        loc, rot, scale = o.matrix_world.decompose()
        e = rot.to_euler()
        if abs(e.x) > 1e-4 or abs(e.y) > 1e-4 or max(scale) - min(scale) > 1e-4:
            raise SystemExit('%s: a game mesh is only turned about Z and scaled evenly' % o.name)
        instances.append({'name': o.name, 'mesh': o['lobby_game_mesh'], 'appearance': o['lobby_game_appearance'],
                          'position': list(loc), 'yaw': e.z, 'scale': scale.x,
                          'shadows': bool(o.get('lobby_game_shadows', False))})
    # The screens the game draws on (the terminal's menu): centre, the canvas's right and up, the side it faces.
    screens = {}
    for o in scene.objects:
        if o.get('lobby_screen') is None:
            continue
        m = o.matrix_world
        screens[o['lobby_screen']] = {'centre': list(m.translation),
                                      'right': list((m.to_3x3() @ Vector((1, 0, 0))).normalized()),
                                      'up': list((m.to_3x3() @ Vector((0, 0, 1))).normalized()),
                                      'front': list((m.to_3x3() @ Vector((0, -1, 0))).normalized())}
    # The buttons the game presses (the table's return button): the face pressed, its axes and size.
    buttons = {}
    for o in scene.objects:
        if o.get('lobby_button') is None:
            continue
        m = o.matrix_world
        front = (m.to_3x3() @ Vector((0, -1, 0))).normalized()
        dims = o.dimensions
        buttons[o['lobby_button']] = {'centre': list(m.translation + front * (dims.y / 2)),
                                      'right': list((m.to_3x3() @ Vector((1, 0, 0))).normalized()),
                                      'up': list((m.to_3x3() @ Vector((0, 0, 1))).normalized()),
                                      'front': list(front), 'size': [dims.x, dims.z]}
    # The dials (the terminal's light): the dial's face (its empty's -Y side), its axes and the rim's radius.
    dials = {}
    for o in scene.objects:
        if o.get('lobby_dial') is None:
            continue
        m = o.matrix_world
        rim = max((c.dimensions.x / 2 for c in o.children if c.type == 'MESH'), default=0.05)
        dials[o['lobby_dial']] = {'centre': list(m.translation), 'right': list((m.to_3x3() @ Vector((1, 0, 0))).normalized()),
                                  'up': list((m.to_3x3() @ Vector((0, 0, 1))).normalized()),
                                  'front': list((m.to_3x3() @ Vector((0, -1, 0))).normalized()), 'radius': rim}
    manifest = {'version': 1, 'frame': 'player at the start of the lobby: X right, Y forward, Z up, metres',
                'chunks': [], 'lights': [], 'colliders': colliders, 'motions': motions, 'instances': instances,
                'screens': screens, 'buttons': buttons, 'dials': dials}
    for name, ch in sorted(chunks.items()):
        for i, b in enumerate(ch.parts):
            if not b.groups:
                continue
            part = name if i == 0 else '%s_p%d' % (name, i)
            counts = b.write(OUT / (part + '.glb'), part)
            verts = {m: len(g['pos']) for m, g in b.groups.items()}
            lo = [min(min(v[k] for v in g['pos']) for g in b.groups.values()) for k in range(3)]
            hi = [max(max(v[k] for v in g['pos']) for g in b.groups.values()) for k in range(3)]
            manifest['chunks'].append({'name': part, 'materials': sorted(counts), 'triangles': counts,
                                       'vertices': verts, 'bounds': [lo, hi],
                                       'moving': name.startswith('moving_'), 'item': item_info.get(name)})
            print('chunk', part, sum(counts.values()), 'tris', sorted(counts), flush=True)
    # The near chunk always: its entity holds the place's colliders, lights, haze and the game's own meshes, also when
    # nothing of the layout's own is drawn there (the table is the game's desk).
    if not any(c['name'] == 'near' for c in manifest['chunks']):
        manifest['chunks'].append({'name': 'near', 'geometry': False, 'materials': [], 'triangles': {},
                                   'vertices': {}, 'bounds': None, 'moving': False, 'item': None})
        print('chunk near: nothing drawn', flush=True)
    for o in scene.objects:
        if o.type != 'LIGHT' or 'lights' not in collections_of(o):
            continue
        d = o.data
        entry = {'name': o.name, 'type': d.type, 'position': list(o.matrix_world.translation),
                 'color': list(d.color), 'power': d.energy, 'radius': o.get('lobby_radius', 6.0),
                 'fog': bool(o.get('lobby_fog', False))}
        if d.type == 'AREA':
            entry['size'] = [d.size, d.size_y]
            entry['axis_x'] = list(o.matrix_world.to_3x3() @ Vector((1, 0, 0)))
            entry['axis_y'] = list(o.matrix_world.to_3x3() @ Vector((0, 1, 0)))
            entry['direction'] = list(o.matrix_world.to_3x3() @ Vector((0, 0, -1)))
        manifest['lights'].append(entry)
    materials = {}
    for m in bpy.data.materials:
        key = material_name(m)
        if 'game' in m.keys():
            materials[key] = {'textured': plain(m['game'])}
        elif 'own' in m.keys():
            materials[key] = {'own': plain(m['own'])}
        else:
            nodes = m.node_tree.nodes if m.node_tree else []
            p = next((n for n in nodes if n.type == 'BSDF_PRINCIPLED'), None)
            if p is None:
                continue
            materials[key] = {'base': list(p.inputs['Base Color'].default_value)[:3],
                              'roughness': p.inputs['Roughness'].default_value,
                              'metal': p.inputs['Metallic'].default_value,
                              'emission': list(p.inputs['Emission Color'].default_value)[:3],
                              'emission_strength': p.inputs['Emission Strength'].default_value}
    manifest['materials'] = materials
    manifest['haze'] = [dict(h) for h in LL.haze()]
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=1), encoding='utf-8')
    print('EXPORTED', len(manifest['chunks']), 'chunks', sum(len(v) for v in colliders.values()), 'colliders',
          len(motions), 'motions', flush=True)


main()
