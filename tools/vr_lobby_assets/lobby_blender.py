"""Builds the VR lobby (lobby_layout.py) as a Blender scene, with the game's own concrete textures, and renders it
from the reference panorama's point of view.

    blender --background --python tools/vr_lobby_assets/lobby_blender.py -- [--render] [--samples N] [--size W]

Writes art/vr_lobby/reference/lobby_reference.blend; with --render also art/vr_lobby/reference/renders/panorama.png
(360 x 180 from the player's eyes) -- tools/vr_lobby_assets/pano_views.py cuts it into the same views as the
reference.

The geometry is final mesh (bevels and cuts applied, UVs in metres) so that what is exported to the game
(lobby_export.py) is what is seen here. Each solid is one object; its glowing lines are part of it (material slot
'amber'). Custom properties carry what the export needs: lobby_band, lobby_collide, lobby_motion.
"""
import json
import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_layout as LL  # noqa: E402
import lobby_toys as LT  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'art/vr_lobby/reference'
TEX = ROOT / 'build/vr-lobby-20261027/concrete_png2'
POSES = ROOT / 'build/lobby-blender/poses'
LOGO = ROOT / 'build/lobby-blender/logo'
TILE = 3.0                       # the game's concrete textures cover 3 m
SKY = 2.0                        # the sky's brightness
HAZE = (0.34, 0.52, 0.88)        # the fog's colour, the sky's at the horizon
HAZE_DENSITY = 0.0030            # per metre at the ground
HAZE_HEIGHT = 120.0              # it thins by e every this many metres up
HAZE_BRIGHTNESS = 1.7


# ---------------------------------------------------------------- materials

def image(path, noncolor=False):
    img = bpy.data.images.load(str(path), check_existing=True)
    if noncolor:
        img.colorspace_settings.name = 'Non-Color'
    return img


def principled(m):
    return next(n for n in m.node_tree.nodes if n.type == 'BSDF_PRINCIPLED')


def flat_material(name, color, rough=0.5, metal=0.0, emission=0.0, ecolor=None):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    p = principled(m)
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Roughness'].default_value = rough
    p.inputs['Metallic'].default_value = metal
    if emission:
        p.inputs['Emission Color'].default_value = (*(ecolor or color), 1)
        p.inputs['Emission Strength'].default_value = emission
    m.diffuse_color = (*color, 1)
    return m


def textured_material(name, diffuse, normal, rough, tint, rough_range, normal_strength):
    """The game's metal_base as the export sets it: BaseColor texture x BaseColorScale, Roughness texture mapped
    into rough_range, Normal texture at normal_strength; UV = the mesh's (metres / 3)."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    n, l = m.node_tree.nodes, m.node_tree.links
    p = principled(m)
    uv = n.new('ShaderNodeUVMap')
    d = n.new('ShaderNodeTexImage'); d.image = image(diffuse)
    l.new(uv.outputs['UV'], d.inputs['Vector'])
    mix = n.new('ShaderNodeMix'); mix.data_type = 'RGBA'; mix.blend_type = 'MULTIPLY'
    mix.inputs['Factor'].default_value = 1.0
    l.new(d.outputs['Color'], mix.inputs['A']); mix.inputs['B'].default_value = (*tint, 1)
    l.new(mix.outputs['Result'], p.inputs['Base Color'])
    r = n.new('ShaderNodeTexImage'); r.image = image(rough, True)
    l.new(uv.outputs['UV'], r.inputs['Vector'])
    mr = n.new('ShaderNodeMapRange')
    mr.inputs['To Min'].default_value, mr.inputs['To Max'].default_value = rough_range
    l.new(r.outputs['Color'], mr.inputs['Value']); l.new(mr.outputs['Result'], p.inputs['Roughness'])
    if normal:
        nt = n.new('ShaderNodeTexImage'); nt.image = image(normal, True)
        l.new(uv.outputs['UV'], nt.inputs['Vector'])
        nm = n.new('ShaderNodeNormalMap'); nm.inputs['Strength'].default_value = normal_strength
        l.new(nt.outputs['Color'], nm.inputs['Color']); l.new(nm.outputs['Normal'], p.inputs['Normal'])
    p.inputs['Metallic'].default_value = 0.0
    m.diffuse_color = (*tint, 1)
    m['game'] = {'diffuse': str(diffuse), 'normal': str(normal) if normal else '', 'rough': str(rough),
                 'tint': list(tint), 'rough_range': list(rough_range), 'normal_strength': normal_strength}
    return m


def hologram_material():
    """The waiting body: amber, see-through, its edges and its mesh lines brighter."""
    m = bpy.data.materials.new('hologram')
    m.use_nodes = True
    n, l = m.node_tree.nodes, m.node_tree.links
    n.clear()
    out = n.new('ShaderNodeOutputMaterial')
    trans = n.new('ShaderNodeBsdfTransparent')
    glow = n.new('ShaderNodeEmission'); glow.inputs['Color'].default_value = (1.0, 0.42, 0.07, 1)
    glow.inputs['Strength'].default_value = 2.2
    lw = n.new('ShaderNodeLayerWeight'); lw.inputs['Blend'].default_value = 0.3
    wire = n.new('ShaderNodeWireframe'); wire.use_pixel_size = False; wire.inputs['Size'].default_value = 0.004
    edge = n.new('ShaderNodeMath'); edge.operation = 'POWER'; edge.inputs[1].default_value = 2.2
    l.new(lw.outputs['Facing'], edge.inputs[0])
    w = n.new('ShaderNodeMath'); w.operation = 'MULTIPLY'; w.inputs[1].default_value = 0.8
    l.new(wire.outputs['Fac'], w.inputs[0])
    base = n.new('ShaderNodeMath'); base.operation = 'ADD'; base.inputs[1].default_value = 0.0
    l.new(edge.outputs[0], base.inputs[0])
    tot = n.new('ShaderNodeMath'); tot.operation = 'ADD'; tot.use_clamp = True
    l.new(base.outputs[0], tot.inputs[0]); l.new(w.outputs[0], tot.inputs[1])
    mix = n.new('ShaderNodeMixShader')
    l.new(tot.outputs[0], mix.inputs['Fac']); l.new(trans.outputs[0], mix.inputs[1]); l.new(glow.outputs[0], mix.inputs[2])
    l.new(mix.outputs[0], out.inputs['Surface'])
    return m


def materials():
    c = TEX
    M = {}
    M[LL.CONCRETE] = textured_material(
        LL.CONCRETE, c / 'concrete_new/concrete_new_clean_01_300_d.png', c / 'concrete_old/concrete_old_panels_01_300_n.png',
        c / 'concrete_new/concrete_new_clean_01_300_r.png', (0.36, 0.38, 0.42), (0.45, 0.75), 0.8)
    M[LL.CONCRETE_DARK] = textured_material(
        LL.CONCRETE_DARK, c / 'concrete_new/concrete_new_clean_01_300_d.png', c / 'concrete_new/concrete_new_clean_01_300_n.png',
        c / 'concrete_new/concrete_new_clean_01_300_r.png', (0.10, 0.104, 0.112), (0.34, 0.58), 0.5)
    M[LL.FLOOR] = textured_material(
        LL.FLOOR, c / 'concrete_smooth/concrete_smooth_01_300_d.png', c / 'concrete_smooth/concrete_smooth_cracked_01_300_n.png',
        c / 'concrete_smooth/concrete_smooth_cracked_01_300_r.png', (0.30, 0.31, 0.33), (0.05, 0.22), 0.25)
    M[LL.GROUND] = textured_material(
        LL.GROUND, c / 'concrete_new/concrete_new_clean_01_300_d.png', c / 'concrete_new/concrete_new_clean_01_300_n.png',
        c / 'concrete_new/concrete_new_clean_01_300_r.png', (0.24, 0.25, 0.28), (0.25, 0.5), 0.4)
    M[LL.INLAY] = textured_material(
        LL.INLAY, c / 'concrete_smooth/concrete_smooth_01_300_d.png', None,
        c / 'concrete_smooth/concrete_smooth_cracked_01_300_r.png', (0.05, 0.052, 0.058), (0.06, 0.16), 0.0)
    M[LL.SEAM] = flat_material(LL.SEAM, (0.04, 0.042, 0.046), 0.6)
    M[LL.AMBER] = flat_material(LL.AMBER, (0.9, 0.45, 0.12), 0.4, 0.0, 10.0, (1.0, 0.46, 0.11))
    M['amber_soft'] = flat_material('amber_soft', (0.2, 0.1, 0.03), 0.5, 0.0, 0.12, (1.0, 0.46, 0.11))
    M[LL.BRASS] = flat_material(LL.BRASS, (0.55, 0.33, 0.10), 0.3, 1.0)
    M['metal_dark'] = flat_material('metal_dark', (0.03, 0.032, 0.036), 0.32, 0.6)
    M['underlight'] = flat_material('underlight', (0.02, 0.03, 0.03), 0.6, 0.0, 1.0, (0.12, 0.22, 0.22))
    M['screen'] = flat_material('screen', (0.004, 0.005, 0.007), 0.08)
    M['label'] = flat_material('label', (0.75, 0.75, 0.70), 0.5, 0.0, 0.7)
    M['label_gold'] = flat_material('label_gold', (0.85, 0.62, 0.24), 0.45, 0.0, 0.9, (1.0, 0.72, 0.32))
    M['chrome'] = flat_material('chrome', (0.62, 0.63, 0.65), 0.22, 1.0)
    M['menu_red'] = flat_material('menu_red', (0.30, 0.02, 0.02), 0.4, 0.0, 1.2, (0.55, 0.05, 0.04))
    M['hologram'] = hologram_material()
    LT.materials(M, image, principled)
    for name, m in M.items():
        m['lobby_material'] = name
    return M


# ---------------------------------------------------------------- geometry

class Builder:
    def __init__(self, M):
        self.M = M
        # The lobby's collection joins the scene only when it is built: until then the scene holds nothing but the
        # piece being cut, so evaluating each boolean does not re-evaluate the whole lobby.
        self.root = bpy.data.collections.new('LOBBY')
        self.cols = {}
        self.work = bpy.data.collections.new('lobby boolean work')
        bpy.context.scene.collection.children.link(self.work)

    def evaluate(self, obj):
        dg = bpy.context.evaluated_depsgraph_get()
        dg.update()
        return bpy.data.meshes.new_from_object(obj.evaluated_get(dg))

    def finish(self):
        bpy.context.scene.collection.children.unlink(self.work)
        bpy.data.collections.remove(self.work)
        bpy.context.scene.collection.children.link(self.root)

    def col(self, name):
        if name not in self.cols:
            c = bpy.data.collections.new(name)
            self.root.children.link(c)
            self.cols[name] = c
        return self.cols[name]

    # -- mesh helpers

    @staticmethod
    def box_bm(bm, center, size, matrix=None, mat=0):
        hx, hy, hz = (s / 2 for s in size)
        vs = [bm.verts.new((center[0] + x, center[1] + y, center[2] + z))
              for x, y, z in ((-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
                              (-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz))]
        faces = [(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)]
        out = []
        for f in faces:
            face = bm.faces.new([vs[i] for i in f])
            face.material_index = mat
            out.append(face)
        if matrix is not None:
            bmesh.ops.transform(bm, matrix=matrix, verts=vs)
        return vs, out

    def mesh_object(self, name, bm, materials, col, matrix_world=None):
        me = bpy.data.meshes.new(name)
        bm.to_mesh(me)
        bm.free()
        for m in materials:
            me.materials.append(self.M[m])
        o = bpy.data.objects.new(name, me)
        col.objects.link(o)
        if matrix_world is not None:
            o.matrix_world = matrix_world
        return o

    def boolean(self, obj, cutters):
        """obj minus the cutter boxes [(center, size)] in obj's own frame: an exact boolean, applied."""
        if not cutters:
            return
        work_col = self.work
        work_col.objects.link(obj)
        tmp = []
        for i, (c, s) in enumerate(cutters):
            bm = bmesh.new()
            self.box_bm(bm, c, s)
            me = bpy.data.meshes.new('cut')
            bm.to_mesh(me); bm.free()
            co = bpy.data.objects.new('cut', me)
            co.matrix_world = obj.matrix_world
            work_col.objects.link(co)
            co.hide_render = True
            tmp.append(co)
        group = bpy.data.collections.new('cutters')
        for co in tmp:
            group.objects.link(co)
        mod = obj.modifiers.new('cut', 'BOOLEAN')
        mod.operation = 'DIFFERENCE'
        mod.solver = 'EXACT'
        mod.operand_type = 'COLLECTION'
        mod.collection = group
        me = self.evaluate(obj)
        old = obj.data
        obj.modifiers.remove(mod)
        obj.data = me
        bpy.data.meshes.remove(old)
        for co in tmp:
            bpy.data.meshes.remove(co.data)
        bpy.data.collections.remove(group)
        work_col.objects.unlink(obj)

    @staticmethod
    def shade_smooth_sides(obj):
        """A cylinder's round side smooth, its ends flat."""
        for poly in obj.data.polygons:
            poly.use_smooth = abs(poly.normal.z) < 0.5

    @staticmethod
    def uv_metres(obj, tile=TILE):
        """Box-project UVs in world metres (one texture tile per `tile` metres), each face along its dominant axis."""
        me = obj.data
        mw = obj.matrix_world
        rot = mw.to_3x3()
        if not me.uv_layers:
            me.uv_layers.new(name='UVMap')
        uv = me.uv_layers.active.data
        for poly in me.polygons:
            nrm = rot @ poly.normal
            ax = max(range(3), key=lambda i: abs(nrm[i]))
            for li in poly.loop_indices:
                p = mw @ me.vertices[me.loops[li].vertex_index].co
                if ax == 2:
                    u, v = p.x, p.y * (1 if nrm.z > 0 else -1)
                elif ax == 0:
                    u, v = p.y * (1 if nrm.x > 0 else -1), p.z
                else:
                    u, v = p.x * (-1 if nrm.y > 0 else 1), p.z
                uv[li].uv = (u / tile, v / tile)

    # -- one solid of the layout

    def solid(self, s):
        w, d, h = s.size
        matrix = Matrix.Translation(Vector(s.origin)) @ Matrix.Rotation(s.yaw, 4, 'Z')
        bm = bmesh.new()
        cutters = []
        if s.shape == 'stairs':
            self.stairs_bm(bm, s)
        elif s.shape == 'arc':
            self.arc_bm(bm, s)
        else:
            vs, faces = self.box_bm(bm, (0, 0, h / 2), (w, d, h))
            if s.bevel > 0:
                bmesh.ops.bevel(bm, geom=list(bm.edges), offset=min(s.bevel, min(w, d, h) * 0.2), segments=2,
                                profile=0.5, affect='EDGES', clamp_overlap=True)
            stair = s.params.get('stair')
            if stair:
                cutters += self.stairwell_cutters(s, stair)
        for c in s.cuts:
            cutters.append((c.center, c.size))
        # Glowing lines: channels where they sink in (near band only; farther they sit on the face).
        strips_bm = []
        for st in s.strips:
            a, b, nrm = Vector(st.a), Vector(st.b), Vector(st.normal)
            length = (b - a).length
            if length < 0.05:
                continue
            along = (b - a).normalized()
            inset = st.inset if s.band == LL.NEAR else 0.0
            centre = (a + b) / 2 - nrm * inset
            size = self.strip_size(along, nrm, length, st.width, 0.008)
            if inset > 0:
                cutters.append((tuple(centre + nrm * 0.004), self.strip_size(along, nrm, length + 0.02,
                                                                             st.width + 0.02, inset * 2 + 0.01)))
            strips_bm.append((tuple(centre + nrm * 0.004), size))
        for c in s.cuts:
            if c.lit:
                strips_bm.append(self.slot_line(c))
        col = self.col(s.band + (' / moving' if s.group == 'moving' else ''))
        o = self.mesh_object(s.name, bm, [s.material, LL.AMBER], col, matrix)
        self.boolean(o, cutters)
        if strips_bm:
            bm = bmesh.new()
            bm.from_mesh(o.data)
            for c, size in strips_bm:
                self.box_bm(bm, c, size, mat=1)
            bm.to_mesh(o.data)
            bm.free()
        self.uv_metres(o)
        o['lobby_band'] = s.band
        o['lobby_collide'] = s.collide
        o['lobby_group'] = s.group
        o['lobby_shape'] = s.shape
        o['lobby_size'] = list(s.size)
        if s.params:
            o['lobby_params'] = {k: v for k, v in s.params.items() if isinstance(v, (int, float, str))}
            if 'stair' in s.params:
                o['lobby_stair'] = s.params['stair']
        return o

    @staticmethod
    def strip_size(along, nrm, length, width, thick):
        # Axis-aligned in the solid's frame: the long side along `along`, thin along the normal.
        size = [0.0, 0.0, 0.0]
        ia = max(range(3), key=lambda i: abs(along[i]))
        inn = max(range(3), key=lambda i: abs(nrm[i]))
        iw = 3 - ia - inn
        size[ia], size[inn], size[iw] = length, thick, width
        return tuple(size)

    @staticmethod
    def slot_line(c):
        """A line up the back wall of a lit slot (or along a lit groove)."""
        cx, cy, cz = c.center
        sx, sy, sz = c.size
        if sz >= max(sx, sy):                       # vertical slot
            if sx < sy:                             # cut into a left/right face
                back = cx - math.copysign(sx / 2 - 0.006, cx)
                return (back, cy, cz), (0.008, sy * 0.4, sz * 0.96)
            back = cy - math.copysign(sy / 2 - 0.006, cy)
            return (cx, back, cz), (sx * 0.4, 0.008, sz * 0.96)
        if sx >= sy:                                # horizontal groove on a front/back face
            back = cy - math.copysign(sy / 2 - 0.006, cy)
            return (cx, back, cz), (sx * 0.98, 0.008, sz * 0.35)
        back = cx - math.copysign(sx / 2 - 0.006, cx)
        return (back, cy, cz), (0.008, sy * 0.98, sz * 0.35)

    @staticmethod
    def stairwell_cutters(s, st):
        """The air above a flight cut into a terrace's front edge, as boxes: one per step and the landing above."""
        w, d, h = s.size
        front = -d / 2
        steps = st['steps']
        rise = (h - st['base']) / steps
        tread = st['run'] / steps
        out = []
        for k in range(steps - 1):
            top = st['base'] + (k + 1) * rise
            y0 = front - 0.05 if k == 0 else front + k * tread
            y1 = front + (k + 1) * tread
            out.append(((st['x'], (y0 + y1) / 2, (top + h + 0.6) / 2), (st['width'], y1 - y0 + 0.002, h + 0.6 - top)))
        return out

    @staticmethod
    def stairs_bm(bm, s):
        w, d, h = s.size
        steps, rise, run = s.params['steps'], s.params['rise'], s.params['run']
        for k in range(steps):
            top = (k + 1) * rise
            Builder.box_bm(bm, (0, -d / 2 + (k + 0.5) * run, top / 2), (w, run, top))
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=0.0005)

    @staticmethod
    def arc_bm(bm, s):
        p = s.params
        r0, r1, a0, a1 = p['r_in'], p['r_out'], p['a0'], p['a1']
        h = s.size[2]
        n = max(4, int((a1 - a0) * r1 / 2.5))
        rings = []
        for i in range(n + 1):
            a = a0 + (a1 - a0) * i / n
            c, sn = math.cos(a), math.sin(a)
            rings.append([bm.verts.new((r * c, r * sn, z)) for r, z in ((r0, 0), (r1, 0), (r1, h), (r0, h))])
        for i in range(n):
            q, r = rings[i], rings[i + 1]
            for j in range(4):
                bm.faces.new((q[j], r[j], r[(j + 1) % 4], q[(j + 1) % 4]))
        bm.faces.new(list(reversed(rings[0])))
        bm.faces.new(rings[-1])
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        if p.get('lit'):
            # A line round the underside, near the inner edge, facing down.
            for i in range(n):
                aa, bb = a0 + (a1 - a0) * (i + 0.03) / n, a0 + (a1 - a0) * (i + 0.97) / n
                rr = r0 + 0.35
                v = [bm.verts.new((rad * math.cos(a), rad * math.sin(a), -0.004))
                     for rad, a in ((rr, aa), (rr + 0.12, aa), (rr + 0.12, bb), (rr, bb))]
                f = bm.faces.new(v)
                f.material_index = 1
                f.normal_update()
                if f.normal.z > 0:
                    f.normal_flip()


# ---------------------------------------------------------------- under the tutorial hall's floor

# The tutorial hall's floor plates are 2 cm slabs with vents cut through them, laid with 8 cm seams between their
# long sides (the kit's vr_architecture_floor_*: a 1 m plate is 0.92 m wide; grown x2, 16 cm), so the void under the hall showed
# through as holes and gaps -- black in the lobby's night. A sheet under the whole walkable floor (tutorial_lobby.py
# place: floor_rects, Night City coordinates) closes them, lit in the hall's teal: the vents and the seams read as lit
# from below, as the glass walkways between the plates already were (a dark sheet turned those into dark bands). It
# sits under the grown (x2) plates' undersides, and nothing collides with it.
UNDERFLOOR_DEPTH = 0.08


def underfloor(B):
    place = json.loads((ROOT / 'build/lobby-tutorial/place.json').read_text(encoding='utf-8'))
    spawn, h = place['spawn']['position'], math.radians(place['spawn']['yaw'])
    fwd, right = (-math.sin(h), math.cos(h)), (math.cos(h), math.sin(h))

    def frame(x, y):
        d = (x - spawn[0], y - spawn[1])
        return d[0] * right[0] + d[1] * right[1], d[0] * fwd[0] + d[1] * fwd[1]

    bm = bmesh.new()
    for x0, y0, x1, y1 in place['floor_rects']:
        vs = [bm.verts.new((*frame(x, y), -UNDERFLOOR_DEPTH)) for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1))]
        f = bm.faces.new(vs)
        if f.normal.z < 0:
            f.normal_flip()
    o = B.mesh_object('underfloor', bm, ['underlight'], B.col('near / floor'))
    B.uv_metres(o)
    o['lobby_band'] = LL.NEAR
    o['lobby_collide'] = 'none'


# ---------------------------------------------------------------- the plaza floor

def floor(B, L):
    col = B.col('near / floor')
    # The square kilometre of ground, under everything.
    bm = bmesh.new()
    B.box_bm(bm, (0, 0, -0.26), (2 * LL.WORLD_HALF, 2 * LL.WORLD_HALF, 0.5))
    g = B.mesh_object('ground', bm, [LL.GROUND], col)
    B.uv_metres(g)
    g['lobby_band'] = LL.FAR
    g['lobby_collide'] = 'box'
    # The polished plaza, a slab flush with the ground; the circle ahead is let into it, flush too.
    bm = bmesh.new()
    B.box_bm(bm, (0.0, 1.25, -0.15), (33.0, 32.5, 0.3))
    p = B.mesh_object('plaza', bm, [LL.FLOOR, LL.AMBER], col)
    cx, cy = LL.PLAZA_CIRCLE
    B.uv_metres(p)
    p['lobby_band'] = LL.NEAR
    p['lobby_collide'] = 'none'
    # The circle's dark disc and the joints.
    bm = bmesh.new()
    bmesh.ops.create_circle(bm, cap_ends=True, segments=96, radius=LL.DISC_R)
    bmesh.ops.translate(bm, verts=bm.verts, vec=(cx, cy, 0.002))
    o = B.mesh_object('plaza disc', bm, [LL.INLAY], col)
    B.uv_metres(o)
    o['lobby_band'] = LL.NEAR
    o['lobby_collide'] = 'none'
    bm = bmesh.new()
    amber = bmesh.new()

    def ring(target, c, r, w, a0, a1, z):
        n = max(24, int((a1 - a0) * r * 12))
        prev = None
        for i in range(n + 1):
            a = a0 + (a1 - a0) * i / n
            pair = [target.verts.new((c[0] + rr * math.cos(a), c[1] + rr * math.sin(a), z)) for rr in (r - w / 2, r + w / 2)]
            if prev:
                target.faces.new((prev[0], prev[1], pair[1], pair[0]))
            prev = pair

    def line(target, a, b, w, clear, z):
        a, b = Vector((*a, 0)), Vector((*b, 0))
        L_ = (b - a).length
        steps = max(1, int(L_ / 0.1))
        keep = []
        for i in range(steps):
            t0, t1 = i / steps, (i + 1) / steps
            m = a.lerp(b, (t0 + t1) / 2)
            if any(math.hypot(m.x - c[0], m.y - c[1]) < r for c, r in clear):
                continue
            if keep and abs(keep[-1][1] - t0) < 1e-9:
                keep[-1] = (keep[-1][0], t1)
            else:
                keep.append((t0, t1))
        d = (b - a).normalized()
        nrm = Vector((-d.y, d.x, 0)) * (w / 2)
        for t0, t1 in keep:
            p0, p1 = a.lerp(b, t0), a.lerp(b, t1)
            vs = [target.verts.new((q.x, q.y, z)) for q in (p0 - nrm, p1 - nrm, p1 + nrm, p0 + nrm)]
            target.faces.new(vs)

    for seam in L.seams:
        if seam[0] == 'ring':
            _, c, r, w, a0, a1 = seam
            ring(bm, c, r, w, a0, a1, 0.0015)
        elif seam[0] == 'line':
            _, a, b, w, clear = seam
            line(bm, a, b, w, clear, 0.0015)
        elif seam[0] == 'amber' and len(seam) == 6:
            _, c, r, w, a0, a1 = seam
            ring(amber, c, r, w, a0, a1, 0.0025)
        elif seam[0] == 'amber':
            _, a, b, w, clear = seam
            line(amber, a, b, w, clear, 0.002)
    o = B.mesh_object('plaza joints', bm, [LL.SEAM], col)
    o['lobby_band'] = LL.NEAR
    o['lobby_collide'] = 'none'
    o = B.mesh_object('plaza floor lines', amber, [LL.AMBER], col)
    o['lobby_band'] = LL.NEAR
    o['lobby_collide'] = 'none'


# ---------------------------------------------------------------- props round the player

def place(o, at, yaw, z=0.0):
    o.location = (at[0], at[1], z)
    o.rotation_euler = (0, 0, yaw)


def part(B, col, parent, name, center, size, mat, bevel=0.01, rot=None):
    bm = bmesh.new()
    B.box_bm(bm, (0, 0, 0), size)
    if bevel:
        bmesh.ops.bevel(bm, geom=list(bm.edges), offset=bevel, segments=2, profile=0.5, affect='EDGES',
                        clamp_overlap=True)
    o = B.mesh_object(name, bm, [mat], col)
    o.parent = parent
    o.location = center
    if rot:
        o.rotation_euler = rot
    return o


def text(col, parent, name, body, loc, size, mat, font, align='LEFT', rot=(math.pi / 2, 0, 0)):
    cu = bpy.data.curves.new(name, 'FONT')
    cu.body = body; cu.size = size; cu.align_x = align; cu.extrude = 0.0003
    cu.font = font
    o = bpy.data.objects.new(name, cu)
    col.objects.link(o)
    o.parent = parent; o.location = loc; o.rotation_euler = rot
    cu.materials.append(mat)
    return o


def terminal(B, L, M, font):
    col = B.col('near / props')
    at, yaw = L.props['terminal']
    root = bpy.data.objects.new('terminal', None); col.objects.link(root); place(root, at, yaw)
    root['lobby_prop'] = 'terminal'
    part(B, col, root, 'terminal body', (0, 0, 0.47), (1.12, 0.6, 0.94), 'metal_dark', 0.02)
    part(B, col, root, 'terminal plinth', (0, 0.02, 0.025), (1.06, 0.56, 0.05), LL.SEAM, 0.0)
    part(B, col, root, 'terminal top', (0, 0, 0.965), (1.2, 0.68, 0.05), 'metal_dark', 0.012)
    part(B, col, root, 'terminal front seam', (0, -0.302, 0.42), (1.08, 0.006, 0.012), LL.SEAM, 0.0)
    part(B, col, root, 'terminal drawer seam', (0, -0.302, 0.78), (1.08, 0.006, 0.012), LL.SEAM, 0.0)
    part(B, col, root, 'terminal foot line', (0, -0.302, 0.07), (1.0, 0.008, 0.022), LL.AMBER, 0.0)
    # The light dial, its brass rim and label, on the upper panel above the drawer seam as the reference has it. In
    # the game it is the lobby's light (lobby_dial: its face is the empty's -Y side).
    dial = bpy.data.objects.new('terminal dial', None); col.objects.link(dial); dial.parent = root
    dial.location = (0.33, -0.31, 0.865)
    dial['lobby_dial'] = 'light'
    for name, r, depth, mat, y in (('dial rim', 0.085, 0.02, LL.BRASS, -0.005), ('dial knob', 0.07, 0.05, 'metal_dark', -0.02),
                                   ('dial glow', 0.088, 0.004, LL.AMBER, 0.006)):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=r, radius2=r, depth=depth)
        o = B.mesh_object('terminal ' + name, bm, [mat], col)
        o.parent = dial; o.location = (0, y, 0); o.rotation_euler = (math.pi / 2, 0, 0)
    text(col, root, 'terminal light label', 'СВЕТ', (0.45, -0.304, 0.858), 0.035, M['label_gold'], font)
    part(B, col, root, 'terminal light rule', (0.478, -0.303, 0.848), (0.055, 0.003, 0.003), LL.BRASS, 0.0)
    # The screen on a short neck, leaning back.
    part(B, col, root, 'terminal cradle', (0, 0.1, 1.03), (0.86, 0.36, 0.13), 'metal_dark', 0.012)
    scr = bpy.data.objects.new('terminal screen', None); col.objects.link(scr); scr.parent = root
    scr.location = (0, 0.08, 1.47); scr.rotation_euler = (math.radians(-11), 0, 0)
    part(B, col, scr, 'screen casing', (0, 0.06, 0), (1.0, 0.19, 0.8), 'metal_dark', 0.02)
    part(B, col, scr, 'screen bezel', (0, -0.03, 0), (0.96, 0.02, 0.76), LL.CONCRETE_DARK, 0.008)
    part(B, col, scr, 'screen glass', (0, -0.046, 0), (0.9, 0.006, 0.7), 'screen', 0.0)
    for x in (-0.445, 0.445):
        for z in (-0.34, 0.34):
            part(B, col, scr, 'screen corner', (x * 0.98, -0.05, z * 0.98), (0.05, 0.003, 0.004), LL.BRASS, 0.0)
            part(B, col, scr, 'screen corner', (x * 0.98 + (0.023 if x < 0 else -0.023), -0.05, z * 0.98 + (-0.023 if z > 0 else 0.023)),
                 (0.004, 0.003, 0.05), LL.BRASS, 0.0)
    logo = sorted(LOGO.rglob('menu_background.png'))
    if logo:
        m = bpy.data.materials.new('logo'); m.use_nodes = True
        n, l = m.node_tree.nodes, m.node_tree.links
        n.clear()
        out = n.new('ShaderNodeOutputMaterial'); tex = n.new('ShaderNodeTexImage'); tex.image = image(logo[0])
        em = n.new('ShaderNodeEmission'); em.inputs['Color'].default_value = (1.0, 0.85, 0.1, 1); em.inputs['Strength'].default_value = 2.2
        tr = n.new('ShaderNodeBsdfTransparent'); mx = n.new('ShaderNodeMixShader')
        l.new(tex.outputs['Alpha'], mx.inputs['Fac']); l.new(tr.outputs[0], mx.inputs[1]); l.new(em.outputs[0], mx.inputs[2])
        l.new(mx.outputs[0], out.inputs['Surface'])
        me = bpy.data.meshes.new('logo')
        me.from_pydata([(-0.3, 0, -0.046), (0.3, 0, -0.046), (0.3, 0, 0.046), (-0.3, 0, 0.046)], [], [(0, 1, 2, 3)])
        uv = me.uv_layers.new()
        coords = [(.465676, .120671), (.987705, .120671), (.987705, .188041), (.465676, .188041)]
        for lp in me.loops:
            uv.data[lp.index].uv = coords[lp.vertex_index]
        me.materials.append(m)
        o = bpy.data.objects.new('screen logo', me); col.objects.link(o); o.parent = scr; o.location = (0, -0.051, 0.24)
    text(col, scr, 'screen title', 'БУФЕР', (0, -0.051, 0.13), 0.04, M['label'], font, 'CENTER')
    for i, label in enumerate(('ПРОДОЛЖИТЬ', 'НОВАЯ ИГРА', 'ЗАГРУЗИТЬ ИГРУ', 'НАСТРОЙКИ', 'НАСТРОЙКИ VR', 'ВЫХОД')):
        z = 0.06 - i * 0.075
        part(B, col, scr, 'menu row', (0, -0.05, z), (0.62, 0.003, 0.058), 'menu_red' if i == 1 else 'metal_dark', 0.0)
        text(col, scr, 'menu ' + label, label, (-0.28, -0.0525, z - 0.012), 0.028, M['label'], font)
    # What the screen shows here stands in for the terminal's menu, which the game draws there itself: on a canvas
    # just in front of the glass, its frame given by this empty (X right, Z up, -Y towards the viewer).
    for o in scr.children:
        if o.name.startswith(('menu', 'screen logo', 'screen title')):
            o['lobby_preview_only'] = True
    menu = bpy.data.objects.new('terminal menu', None); col.objects.link(menu); menu.parent = scr
    menu.location = (0, -0.052, 0)
    menu['lobby_screen'] = 'terminal'


def stand_in(col, parent, name, mesh):
    """The geometry of a game mesh (lobby_pipeline.py poses), under `parent` in the mesh's own frame, for the views
    only: the game draws the mesh itself (lobby_game_mesh on the parent)."""
    glb = POSES / (mesh[:-len('.mesh')].replace('\\', '/') + '.glb')
    if not glb.exists():
        raise RuntimeError('%s missing: run lobby_pipeline.py poses' % glb)
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(glb))
    added = set(bpy.data.objects) - before
    for o in added:
        for c in list(o.users_collection):
            c.objects.unlink(o)
        if o.type != 'MESH' or ('_LOD_' in o.name and '_LOD_1' not in o.name):
            continue
        me = o.data.copy(); me.transform(o.matrix_world)
        nb = bpy.data.objects.new(name, me); col.objects.link(nb)
        nb.parent = parent
        nb['lobby_stand_in'] = True
    for o in added:
        bpy.data.objects.remove(o)


def table(B, L, M, font):
    col = B.col('near / props')
    at, yaw = L.props['table']
    root = bpy.data.objects.new('table', None); col.objects.link(root); place(root, at, yaw)
    root['lobby_prop'] = 'table'
    # The game's own desk (lobby_layout.TABLE_DESK), its working side to the player. Local frame: -Y towards the
    # player, the top at TABLE_TOP. Its collision is one box round it, drawn by nothing.
    desk = bpy.data.objects.new('table desk', None); col.objects.link(desk); desk.parent = root
    desk.rotation_euler = (0, 0, math.pi)
    desk['lobby_game_mesh'] = LL.TABLE_DESK
    desk['lobby_game_appearance'] = LL.TABLE_DESK_APPEARANCE
    desk['lobby_game_shadows'] = True
    stand_in(col, desk, 'table desk geometry', LL.TABLE_DESK)
    w, d = LL.TABLE_DESK_SIZE
    o = part(B, col, root, 'table desk collision', (0, 0, LL.TABLE_TOP / 2), (w, d, LL.TABLE_TOP), 'metal_dark', 0.0)
    o['lobby_collider_only'] = True
    table_top = LL.TABLE_TOP
    # Things to pick up: in the game each is a physical body of its own (lobby_item = its collision shape,
    # lobby_mass = kilograms), held in the hand, thrown, and put back on the table when lost.
    bpy.ops.mesh.primitive_uv_sphere_add(segments=40, ring_count=20, radius=0.11, location=(-0.32, 0.2, table_top + 0.11))
    s = bpy.context.object; s.name = 'item sphere'; s.data.materials.append(M['metal_dark'])
    for c in list(s.users_collection):
        c.objects.unlink(s)
    col.objects.link(s); s.parent = root
    bpy.ops.object.shade_smooth()
    s['lobby_item'], s['lobby_mass'] = 'sphere', 2.0
    o = part(B, col, root, 'item cube', (0.02, 0.24, table_top + 0.085), (0.17, 0.17, 0.17), LL.BRASS, 0.006)
    o['lobby_item'], o['lobby_mass'] = 'box', 1.2
    # The can: bright metal, its label a dark band round the middle.
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=40, radius1=0.033, radius2=0.033, depth=0.122)
    for z in (-0.04, 0.042):
        bmesh.ops.bisect_plane(bm, geom=list(bm.verts) + list(bm.edges) + list(bm.faces), plane_co=(0, 0, z),
                               plane_no=(0, 0, 1))
    bm.normal_update()
    for f in bm.faces:
        f.material_index = 1 if abs(f.calc_center_median().z) < 0.041 and abs(f.normal.z) < 0.5 else 0
    o = B.mesh_object('item can', bm, ['chrome', 'metal_dark'], col); o.parent = root; o.location = (0.24, 0.26, table_top + 0.061)
    B.shade_smooth_sides(o)
    o['lobby_item'], o['lobby_mass'] = 'box', 0.35
    o['lobby_grasp'] = 'cylinder'           # the hand closes on a can, not on the box round it
    # The shards: dark slabs with a brass rim round the top; where they lie the engrams lie (modules/engrams.lua: the
    # first marks where their row starts, the second which way it runs).
    for x, y in ((0.16, -0.14), (0.3, -0.2)):
        bm = bmesh.new()
        _, faces = B.box_bm(bm, (0, 0, 0), (0.11, 0.06, 0.016))
        top = [f for f in faces if f.calc_center_median().z > 0.0079]
        ring = bmesh.ops.inset_region(bm, faces=top, thickness=0.006, depth=0.0)['faces']
        for f in ring:
            f.material_index = 1
        o = B.mesh_object('item shard', bm, ['metal_dark', LL.BRASS], col); o.parent = root; o.location = (x, y, table_top + 0.008)
        o['lobby_item'], o['lobby_mass'] = 'box', 0.05
    # The toys (lobby_toys.py), one at each end of the table, turned a little towards the player.
    o = toy(LT.toy(B, col, 'chibi'), root, (-0.62, 0.0), 0.35, table_top)
    o['lobby_item'], o['lobby_mass'] = 'box', 0.6
    o = toy(LT.toy(B, col, 'skippy'), root, (0.58, 0.1), -0.35, table_top)
    o['lobby_item'], o['lobby_mass'] = 'box', 0.15


def toy(o, root, at, yaw, top):
    """A toy built standing on its own origin, placed standing at `at` on the table: its origin moved to its middle,
    as every item's is (the game's grasp and marker take the item's origin for its centre)."""
    pts = [Vector(v) for v in o.bound_box]
    c = Vector([(min(p[k] for p in pts) + max(p[k] for p in pts)) / 2 for k in range(3)])
    o.data.transform(Matrix.Translation(-c))
    if o.get('lobby_grasp_at') is not None:
        o['lobby_grasp_at'] = list(Vector(o['lobby_grasp_at']) - c)
    o.parent = root
    o.rotation_euler = (0, 0, yaw)
    o.location = Vector((at[0], at[1], top)) + Matrix.Rotation(yaw, 3, 'Z') @ c
    return o


def pod(B, L, M, font):
    col = B.col('near / props')
    at, yaw = L.props['pod']
    root = bpy.data.objects.new('pod', None); col.objects.link(root); place(root, at, yaw)
    root['lobby_prop'] = 'pod'
    part(B, col, root, 'niche back', (0, 0.62, 1.7), (1.25, 0.08, 3.1), LL.SEAM, 0.0)
    for x in (-0.6, 0.6):
        part(B, col, root, 'niche jamb', (x, 0.0, 1.7), (0.06, 1.25, 3.1), 'metal_dark', 0.004)
        part(B, col, root, 'niche upright line', (x * 0.94, -0.6, 1.72), (0.014, 0.012, 2.95), LL.AMBER, 0.0)
    part(B, col, root, 'niche lintel', (0, 0.0, 3.27), (1.26, 1.25, 0.06), 'metal_dark', 0.004)
    part(B, col, root, 'niche top line', (0, -0.3, 3.22), (1.0, 0.04, 0.01), LL.AMBER, 0.0)
    part(B, col, root, 'niche sill', (0, 0.0, 0.2), (1.2, 1.25, 0.05), 'metal_dark', 0.006)
    part(B, col, root, 'niche floor glow', (0, -0.1, 0.228), (0.9, 0.7, 0.004), 'amber_soft', 0.0)
    text(col, root, 'pod name', 'ОБОЛОЧКА', (1.3, -0.56, 2.5), 0.075, M['label'], font,
         rot=(math.pi / 2, 0, math.radians(-4)))
    text(col, root, 'pod state', 'ОЖИДАНИЕ СВЯЗИ', (1.3, -0.56, 2.3), 0.05, M['label'], font,
         rot=(math.pi / 2, 0, math.radians(-4)))
    part(B, col, root, 'pod label rule', (1.62, -0.565, 2.4), (0.64, 0.004, 0.004), M['label'] and 'label', 0.0,
         (0, 0, math.radians(-4)))
    # The figure: the game's realistic man (lobby_layout.POD_FIGURE), in the construct's hologram. The game draws
    # the construct's own copy of its mesh (make_construct.py), placed by this empty's transform -- the imported
    # geometry is the game mesh's, in its own frame under it.
    glb = POSES / (LL.POD_FIGURE[:-len('.mesh')].replace('\\', '/') + '.glb')
    if not glb.exists():
        raise RuntimeError('%s missing: run lobby_pipeline.py poses' % glb)
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(glb))
    added = set(bpy.data.objects) - before
    body = bpy.data.objects.new('construct body', None); col.objects.link(body); body.parent = root
    body.location = (0, 0.1, 0.23)
    body.rotation_euler = (0, 0, math.pi)
    body.scale = (1.56,) * 3
    body['lobby_game_mesh'] = LL.POD_FIGURE_COPY
    body['lobby_game_appearance'] = 'vrport_construct'
    for o in added:
        for c in list(o.users_collection):
            c.objects.unlink(o)
        if o.type != 'MESH':
            continue
        me = o.data.copy(); me.transform(o.matrix_world)
        me.materials.clear(); me.materials.append(M['hologram'])
        for f in me.polygons:
            f.use_smooth = True
        nb = bpy.data.objects.new('construct', me); col.objects.link(nb)
        nb.parent = body
    for o in added:
        bpy.data.objects.remove(o)


# ---------------------------------------------------------------- moving blocks

def animate(o, m, fps=24):
    """Keyframes for a moving block: there and back along its travel, turning by its spin, over its period."""
    start = o.location.copy()
    yaw0 = o.rotation_euler.z
    frames = int(m.period * fps)
    off = int(m.phase * frames)
    for i in range(0, 5):
        t = i / 4
        k = (1 - math.cos(2 * math.pi * t)) / 2
        o.location = start + Vector(m.travel) * k
        o.rotation_euler.z = yaw0 + math.radians(m.spin) * k
        o.keyframe_insert('location', frame=1 + (i * frames) // 4 - off)
        o.keyframe_insert('rotation_euler', frame=1 + (i * frames) // 4 - off)
    act = o.animation_data.action
    curves = list(getattr(act, 'fcurves', []) or [])
    for layer in getattr(act, 'layers', []):       # Blender 4.4+: layered actions
        for strip in layer.strips:
            for bag in strip.channelbags:
                curves += list(bag.fcurves)
    for fc in curves:
        fc.modifiers.new('CYCLES')
    o.location = start
    o.rotation_euler.z = yaw0
    o['lobby_motion'] = {'travel': list(m.travel), 'spin': m.spin, 'period': m.period, 'phase': m.phase}


# ---------------------------------------------------------------- world, light, cameras

def world():
    w = bpy.data.worlds.new('lobby sky')
    bpy.context.scene.world = w
    w.use_nodes = True
    n, l = w.node_tree.nodes, w.node_tree.links
    n.clear()
    out = n.new('ShaderNodeOutputWorld')
    bg = n.new('ShaderNodeBackground'); bg.inputs['Strength'].default_value = SKY
    tc = n.new('ShaderNodeTexCoord')
    sep = n.new('ShaderNodeSeparateXYZ'); l.new(tc.outputs['Generated'], sep.inputs[0])
    # Blue overhead, pale and bright near the horizon, a warmer band at it.
    ramp = n.new('ShaderNodeValToRGB')
    cr = ramp.color_ramp
    cr.elements[0].position = 0.0; cr.elements[0].color = (0.55, 0.66, 0.84, 1)
    cr.elements[1].position = 1.0; cr.elements[1].color = (0.03, 0.10, 0.36, 1)
    e = cr.elements.new(0.06); e.color = (0.36, 0.53, 0.86, 1)
    e = cr.elements.new(0.35); e.color = (0.10, 0.26, 0.66, 1)
    zc = n.new('ShaderNodeMath'); zc.operation = 'MAXIMUM'; zc.inputs[1].default_value = 0.0
    l.new(sep.outputs['Z'], zc.inputs[0]); l.new(zc.outputs[0], ramp.inputs['Fac'])
    # Clouds: stretched noise, brighter, more of them near the horizon.
    sc = n.new('ShaderNodeVectorMath'); sc.operation = 'MULTIPLY'; sc.inputs[1].default_value = (1.0, 1.0, 3.5)
    l.new(tc.outputs['Generated'], sc.inputs[0])
    nz = n.new('ShaderNodeTexNoise'); nz.inputs['Scale'].default_value = 2.4; nz.inputs['Detail'].default_value = 8.0
    nz.inputs['Roughness'].default_value = 0.62
    l.new(sc.outputs[0], nz.inputs['Vector'])
    cl = n.new('ShaderNodeValToRGB'); cl.color_ramp.elements[0].position = 0.5; cl.color_ramp.elements[1].position = 0.75
    l.new(nz.outputs['Fac'], cl.inputs['Fac'])
    mixc = n.new('ShaderNodeMix'); mixc.data_type = 'RGBA'
    l.new(cl.outputs['Color'], mixc.inputs['Factor'])
    l.new(ramp.outputs['Color'], mixc.inputs['A']); mixc.inputs['B'].default_value = (0.70, 0.78, 0.92, 1)
    l.new(mixc.outputs['Result'], bg.inputs['Color'])
    l.new(bg.outputs[0], out.inputs['Surface'])
    # The haze, the way the game's fog works: with distance everything blends into the fog's colour, whether the
    # sky reaches that air or not -- a volume that absorbs and glows the fog colour at the same rate. Thick near
    # the ground, thinning with height, so the sky overhead stays clear. (A world volume would swallow the sky
    # itself: it lies infinitely far.)
    m = bpy.data.materials.new('haze')
    m.use_nodes = True
    mn, ml = m.node_tree.nodes, m.node_tree.links
    mn.clear()
    mout = mn.new('ShaderNodeOutputMaterial')
    absorb = mn.new('ShaderNodeVolumeAbsorption'); absorb.inputs['Color'].default_value = (0, 0, 0, 1)
    glow = mn.new('ShaderNodeEmission'); glow.inputs['Color'].default_value = (*HAZE, 1)
    geo = mn.new('ShaderNodeNewGeometry')
    sz = mn.new('ShaderNodeSeparateXYZ'); ml.new(geo.outputs['Position'], sz.inputs[0])
    hz = mn.new('ShaderNodeMath'); hz.operation = 'MULTIPLY'; hz.inputs[1].default_value = -1.0 / HAZE_HEIGHT
    ml.new(sz.outputs['Z'], hz.inputs[0])
    ex = mn.new('ShaderNodeMath'); ex.operation = 'EXPONENT'; ml.new(hz.outputs[0], ex.inputs[0])
    dens = mn.new('ShaderNodeMath'); dens.operation = 'MULTIPLY'; dens.inputs[1].default_value = HAZE_DENSITY
    ml.new(ex.outputs[0], dens.inputs[0])
    ml.new(dens.outputs[0], absorb.inputs['Density'])
    em = mn.new('ShaderNodeMath'); em.operation = 'MULTIPLY'; em.inputs[1].default_value = HAZE_BRIGHTNESS
    ml.new(dens.outputs[0], em.inputs[0]); ml.new(em.outputs[0], glow.inputs['Strength'])
    add = mn.new('ShaderNodeAddShader')
    ml.new(absorb.outputs[0], add.inputs[0]); ml.new(glow.outputs[0], add.inputs[1])
    ml.new(add.outputs[0], mout.inputs['Volume'])
    bm = bmesh.new()
    Builder.box_bm(bm, (0, 0, 190), (1100, 1100, 400))
    me = bpy.data.meshes.new('haze'); bm.to_mesh(me); bm.free(); me.materials.append(m)
    ho = bpy.data.objects.new('haze', me)
    hc = bpy.data.collections.new('review / atmosphere'); bpy.context.scene.collection.children.link(hc)
    hc.objects.link(ho)
    # The game lights the plaza from its sky probes whatever hangs overhead; here a broad soft light over the plaza
    # stands in for that (review only, not exported).
    fill = bpy.data.lights.new('sky fill', 'AREA'); fill.shape = 'DISK'; fill.size = 60.0
    fill.energy = 4500.0; fill.color = (0.70, 0.80, 1.0)
    fo = bpy.data.objects.new('sky fill', fill); fo.location = (0, 6, 60)
    hc2 = bpy.data.collections.new('review / sky fill'); bpy.context.scene.collection.children.link(hc2)
    hc2.objects.link(fo)
    # Low warm sun behind the city for the edges, weak: the light is the sky's.
    sun = bpy.data.lights.new('low sun', 'SUN'); sun.energy = 2.4; sun.color = (1.0, 0.9, 0.78); sun.angle = math.radians(12)
    so = bpy.data.objects.new('low sun', sun); bpy.context.scene.collection.objects.link(so)
    so.rotation_euler = (math.radians(52), 0, math.radians(215))


def lights(B, L):
    col = B.col('lights')
    for lt in L.lights:
        if lt.kind == 'area':
            d = bpy.data.lights.new(lt.name, 'AREA'); d.shape = 'RECTANGLE'
            d.size, d.size_y = lt.size
        else:
            d = bpy.data.lights.new(lt.name, 'POINT'); d.shadow_soft_size = 0.2
        d.energy = lt.power; d.color = lt.color
        o = bpy.data.objects.new(lt.name, d); col.objects.link(o); o.location = lt.position
        if lt.target:
            o.rotation_euler = (Vector(lt.target) - Vector(lt.position)).to_track_quat('-Z', 'Y').to_euler()
        o['lobby_radius'] = lt.radius
        o['lobby_fog'] = lt.fog


def cameras(B):
    col = B.col('review / cameras')
    d = bpy.data.cameras.new('panorama')
    d.type = 'PANO'
    try:
        d.panorama_type = 'EQUIRECTANGULAR'
    except (AttributeError, TypeError):
        d.cycles.panorama_type = 'EQUIRECTANGULAR'
    d.clip_start = 0.05; d.clip_end = 2000
    o = bpy.data.objects.new('panorama', d); col.objects.link(o)
    o.location = (0, 0, LL.EYE); o.rotation_euler = (math.pi / 2, 0, 0)
    bpy.context.scene.camera = o
    for yaw, pitch in ((0, 0), (-60, -10), (65, -5), (180, 5)):
        d = bpy.data.cameras.new('view %d %d' % (yaw, pitch)); d.lens_unit = 'FOV'; d.angle = math.radians(90)
        d.clip_start = 0.05; d.clip_end = 2000
        o = bpy.data.objects.new('view %+d %+d' % (yaw, pitch), d); col.objects.link(o)
        o.location = (0, 0, LL.EYE)
        o.rotation_euler = (math.pi / 2 + math.radians(pitch), 0, -math.radians(yaw))


# ---------------------------------------------------------------- main

def build():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o)
    for c in list(bpy.data.collections):
        bpy.data.collections.remove(c)
    scene = bpy.context.scene
    scene.unit_settings.system = 'METRIC'
    L = LL.build()
    M = materials()
    B = Builder(M)
    font = bpy.data.fonts.load('C:/Windows/Fonts/bahnschrift.ttf')
    if LL.PLACE != 'tutorial':
        floor(B, L)         # the tutorial hall has its own
    else:
        underfloor(B)
    for i, s in enumerate(L.solids):
        B.solid(s)
        if i % 250 == 0:
            print('solids', i, '/', len(L.solids), flush=True)
    for m in L.motions:
        animate(B.solid(m.solid), m)
    for name, make in (('terminal', terminal), ('table', table), ('pod', pod)):
        if name in L.props:
            make(B, L, M, font)
    world()
    lights(B, L)
    cameras(B)
    B.finish()
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'GPU'
    try:
        prefs = bpy.context.preferences.addons['cycles'].preferences
        prefs.compute_device_type = 'OPTIX'
        prefs.get_devices()
        for dev in prefs.devices:
            dev.use = dev.type == 'OPTIX'
    except Exception as e:  # noqa: BLE001 - CPU rendering still works
        print('no OptiX:', e)
    scene.cycles.use_denoising = True
    scene.cycles.max_bounces = 6
    scene.cycles.volume_bounces = 0
    scene.cycles.volume_step_rate = 4.0
    scene.cycles.sample_clamp_indirect = 4.0
    scene.cycles.caustics_reflective = False
    scene.cycles.caustics_refractive = False
    scene.cycles.blur_glossy = 1.0
    scene.view_settings.view_transform = 'AgX'
    try:
        scene.view_settings.look = 'AgX - Medium High Contrast'
    except TypeError:
        pass
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 1, 24 * 40
    scene.frame_set(1)
    OUT.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'lobby_reference.blend'))
    print('BUILT objects', len(bpy.data.objects), flush=True)
    return scene


def main():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    scene = build()
    if '--render' in argv:
        samples = int(argv[argv.index('--samples') + 1]) if '--samples' in argv else 64
        width = int(argv[argv.index('--size') + 1]) if '--size' in argv else 2048
        scene.cycles.samples = samples
        scene.render.resolution_x, scene.render.resolution_y = width, width // 2
        scene.render.resolution_percentage = 100
        scene.camera = bpy.data.objects['panorama']
        (OUT / 'renders').mkdir(parents=True, exist_ok=True)
        scene.render.filepath = str(OUT / 'renders/panorama.png')
        bpy.ops.render.render(write_still=True)
        print('RENDERED', scene.render.filepath, flush=True)


if __name__ == '__main__':
    main()
