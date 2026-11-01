"""The VR lobby's layout: every solid, glowing line, light, collider and moving block of the place, as data.

A construct waits in an endless being: a plaza of polished concrete at the bottom of a city of bare concrete blocks
that never ends -- towers rising round it into the sky, blocks hanging at every height, terraces and stairs leading
away in every direction, all of it fading into a blue haze. Reference: art/vr_lobby/references/panorama_v2.png
(a 360-degree panorama; tools/vr_lobby_assets/pano_views.py cuts it into views to compare with).

Frame: the player's at the start of the lobby -- X right, Y forward, Z up, metres, the player standing at the
origin. The place covers a square kilometre (x, y in -500..500).

Nothing here touches Blender or the game: lobby_blender.py builds the scene out of it, lobby_export.py takes the
built scene to the game. `py -3.13 tools/vr_lobby_assets/lobby_layout.py plan` draws it from above.
"""
import math
import random
from dataclasses import dataclass, field

WORLD_HALF = 500.0           # the place is 1 km x 1 km
EYE = 1.7

# Where the lobby stands. 'tutorial': the game's own VR tutorial hall -- its sectors streamed into the main menu's
# world, made twice the size, its columns, stairs and back wall taken out (tools/vr_lobby_assets/tutorial_lobby.py)
# -- with only this layout's hand-modelled pieces in it (the terminal, the table, the pod in its masses) and their
# light: the player is put on the hall's floor when the lobby comes up and the pieces stand round them there.
# 'city': the procedural city of concrete blocks this file builds, spawned wherever the player stands.
PLACE = 'tutorial'
# The tutorial hall's spawn (Night City coordinates; the game's heading in degrees, 0 = +Y, -90 = +X): the middle of
# its floor -- the point tutorial_lobby.py grows the hall about -- looking down the hall's length.
TUTORIAL_SPAWN = {'position': (-417.0, 422.5, -215.0), 'yaw': -90.0}
# The hall at night (no sun: by day it falls through the hall's gaps onto the floor, under heavy clouds too), its
# own dark teal haze (the tutorial's fog volumes, grown with it) and a little more of it: a fog volume over the
# hall, its density spread over 120 m of height (tried live by day: 180 turns the hall white from 15 m on).
TUTORIAL_HAZE = []
TUTORIAL_EXPOSURE = 0.5
TUTORIAL_ATMOSPHERE = {'hour': 21, 'minute': 30, 'weather': '24h_weather_light_clouds'}
CITY_ATMOSPHERE = {'hour': 21, 'minute': 30, 'weather': '24h_weather_light_clouds'}


def haze():
    """The fog volumes the near entity carries for the place."""
    return TUTORIAL_HAZE if PLACE == 'tutorial' else GAME_HAZE

# The haze in the game: fog volumes (entFogVolumeComponent) over the whole place, blue. Tuned live
# (tools/vr_lobby_assets/game_views.py): a volume's density is spread over its height -- one 420 m tall at density
# 20 hardly shows, 120 m tall at 20 fades the blocks a hundred metres off into the sky's blue the way the reference
# does (100 turns everything white) -- so the haze is two layers, thick to 100 m, thinner above to 220 m.
# Blender's preview haze is lobby_blender.py HAZE*.
GAME_HAZE = [{'centre': (0.0, 6.0, 40.0), 'size': (900.0, 900.0, 120.0), 'color': (0.42, 0.58, 1.0),
              'density': 20.0, 'falloff': 0.02, 'absorption': 0.3, 'blend': 10.0},
             {'centre': (0.0, 6.0, 160.0), 'size': (900.0, 900.0, 120.0), 'color': (0.48, 0.62, 1.0),
              'density': 8.0, 'falloff': 0.02, 'absorption': 0.3, 'blend': 10.0}]

# The game draws the place darker than the Blender preview (its lighting, exposure and the sky it reflects are its
# own), set against the reference panorama (region means -- sky, far, middle, floor -- of the eight horizon views,
# the error over all of them). The game's auto exposure answers any brightening of the place itself by exposing it
# down (a brighter floor alone only darkened the sky), so the place is exposed brighter instead: GAME_EXPOSURE, in
# stops, on the environment's own exposure compensation while the lobby is up (VRPortLobbyExposure,
# mods/redscript/CyberpunkVRPort_Lobby). At +0.5 the sky meets the reference's; then per material what the game's
# material takes instead of the preview's -- a factor on its tint, or one per channel, and its roughness range: the
# concrete a little darker and warmer, the polished floor paler, warmer and less of a mirror for the blue sky, the
# dark metal of the terminal and the pod a gunmetal grey rather than black. The error fell from 0.086 to 0.066-0.080
# (two runs: the auto exposure is still settling in some views). (Thinner haze darkens the sky and lifts the rest.)
GAME_EXPOSURE = 0.5
GAME_LOOK = {'floor_polished': {'tint': (2.8, 2.5, 2.3), 'rough_range': (0.15, 0.4)},
             'concrete': {'tint': (1.2, 1.05, 1.0)}, 'ground': {'tint': (1.6, 1.45, 1.35)},
             'metal_dark': {'tint': 4.0}}

# The terminal's screen: the menu's canvas (metres, inside the screen's brass corners) and its pixels, 3000 per
# metre -- the game draws it there (make_lobby_assets.py, mods/cet/CyberpunkVRPort_Lobby modules/terminal.lua, and
# src/Natives/MenuSurface.cpp, which allocates that many pixels for it).
TERMINAL_SCREEN = (0.86, 0.66)
TERMINAL_CANVAS = (2580, 1980)

# The figure in the pod: the game's realistic man of its shop windows, worn as the construct (make_construct.py's
# PROPS). The game draws the construct's own copy of it (POD_FIGURE_COPY); the layout stands it in with the game
# mesh's geometry, uncooked to glb from POD_FIGURE_ARCHIVE (lobby_pipeline.py poses).
POD_FIGURE = 'base\\environment\\decoration\\small_shops\\stands\\mannequin_real_bake\\mannequin_real_man_a_bake.mesh'
POD_FIGURE_ARCHIVE = 'basegame_4_gamedata.archive'
POD_FIGURE_COPY = 'vrport\\construct\\mannequin_real_man_a_bake.mesh'

# The table of things to pick up: the game's own Militech-style desk (the neo-militarist office line, the VR tutorial's
# corporate look), its pivot on the floor under its middle, its working side -- the knee opening and the inlay on its
# top -- along its +Y. Its top is TABLE_TOP high; the things rest there. The layout stands it in with the mesh's
# geometry (lobby_pipeline.py poses); its collision is one box round it.
TABLE_DESK = 'base\\environment\\decoration\\furniture\\office\\neomilitarism_office_desk\\neomilitarism_office_desk_d.mesh'
TABLE_DESK_ARCHIVE = 'basegame_3_nightcity.archive'
TABLE_DESK_APPEARANCE = 'default'
TABLE_DESK_SIZE = (2.068, 0.983)
TABLE_TOP = 0.8025
# The game meshes the layout stands in with their own geometry: mesh -> the archive it is uncooked from.
STAND_INS = {POD_FIGURE: POD_FIGURE_ARCHIVE, TABLE_DESK: TABLE_DESK_ARCHIVE}

AMBER = 'amber'              # the glowing line material
CONCRETE = 'concrete'        # bare cast concrete, the blocks
CONCRETE_DARK = 'concrete_dark'
FLOOR = 'floor_polished'     # the plaza's polished slabs
GROUND = 'ground'            # the rest of the ground, the same concrete less polished
SEAM = 'seam'                # dark joints between floor slabs
INLAY = 'inlay'              # dark polished stone, the plaza's disc
BRASS = 'brass'

# Detail bands: what the export merges together and how fine the geometry is.
NEAR, MID, FAR = 'near', 'mid', 'far'


def band_of(x, y):
    r = math.hypot(x, y)
    return NEAR if r < 34 else MID if r < 150 else FAR


def rot(yaw, v):
    c, s = math.cos(yaw), math.sin(yaw)
    return (c * v[0] - s * v[1], s * v[0] + c * v[1])


def facing(at, target=(0.0, 0.0)):
    """The yaw that turns a piece's local -Y side (its front) towards target."""
    return math.atan2(-(target[0] - at[0]), (target[1] - at[1])) + math.pi


@dataclass
class Strip:
    """A glowing line on one face of a solid, in the solid's frame (origin at the centre of its base):
    from a to b along the face whose outward normal is `normal`, `width` across. inset > 0 sinks it in a channel cut
    that deep into the face; 0 lays it on the face."""
    a: tuple
    b: tuple
    normal: tuple
    width: float = 0.03
    inset: float = 0.02


@dataclass
class Cut:
    """A box taken out of a solid, in the solid's frame. `strip` adds a glowing line along the cut's back wall."""
    center: tuple
    size: tuple
    lit: bool = False


@dataclass
class Solid:
    name: str
    origin: tuple            # centre of the base, scene frame
    size: tuple              # w (local X), d (local Y), h (Z)
    yaw: float = 0.0
    material: str = CONCRETE
    shape: str = 'box'       # box | stairs | arc | slab_floor
    bevel: float = 0.025
    cuts: list = field(default_factory=list)
    strips: list = field(default_factory=list)
    collide: str = 'box'     # box | stairs | none
    group: str = ''
    band: str = ''
    params: dict = field(default_factory=dict)

    def __post_init__(self):
        if not self.band:
            self.band = band_of(self.origin[0], self.origin[1])

    def world(self, p):
        """A point in this solid's frame -> the scene's."""
        x, y = rot(self.yaw, p[:2])
        return (self.origin[0] + x, self.origin[1] + y, self.origin[2] + p[2])


@dataclass
class Light:
    name: str
    position: tuple
    color: tuple             # linear 0..1
    power: float             # watts in Blender; the export scales it for the game
    kind: str = 'point'      # point | area
    size: tuple = (0.1, 0.1)
    target: tuple = None
    radius: float = 6.0      # the game's attenuation radius
    fog: bool = False


@dataclass
class Motion:
    """A block that moves the way the VR tutorial's blocks do: there and back along `travel` (metres), turning by
    `spin` degrees about Z, over `period` seconds, `phase` 0..1 into it at the start."""
    solid: Solid
    travel: tuple
    spin: float
    period: float
    phase: float


@dataclass
class Layout:
    solids: list = field(default_factory=list)
    lights: list = field(default_factory=list)
    motions: list = field(default_factory=list)
    seams: list = field(default_factory=list)       # floor joints: ('line', a, b, width) | ('ring', c, r, width, a0, a1)
    parkour: list = field(default_factory=list)     # [(name, [solid names], [(gap, rise)])]
    spawn: tuple = (0.0, 0.0, 0.0)
    props: dict = field(default_factory=dict)       # hand-modelled pieces: name -> (position, yaw)

    def add(self, solid):
        self.solids.append(solid)
        return solid


# ---------------------------------------------------------------- shapes

def strip_on(face, w, d, h, z0, z1=None, along=(0.0, 1.0), width=0.03, inset=0.02, vertical=False, at=0.5):
    """A line on face `face` ('front' -Y, 'back' +Y, 'left' -X, 'right' +X, 'bottom') of a w x d x h box.
    Horizontal: at height z0, over the fraction `along` of the face's length. Vertical: at fraction `at` across the
    face, from z0 up to z1."""
    if face == 'bottom':
        x0, x1 = (-w / 2 + w * along[0], -w / 2 + w * along[1])
        y = -d / 2 + d * at
        return Strip((x0, y, 0.0), (x1, y, 0.0), (0, 0, -1), width, inset)
    normals = {'front': (0, -1, 0), 'back': (0, 1, 0), 'left': (-1, 0, 0), 'right': (1, 0, 0)}
    n = normals[face]
    if face in ('front', 'back'):
        span, depth = w, d
        def p(t, z): return (-w / 2 + w * t, n[1] * d / 2, z)
    else:
        span, depth = d, w
        def p(t, z): return (n[0] * w / 2, -d / 2 + d * t, z)
    if vertical:
        return Strip(p(at, z0), p(at, z1), n, width, inset)
    return Strip(p(along[0], z0), p(along[1], z0), n, width, inset)


def block(L, name, at, size, yaw=0.0, z=0.0, material=CONCRETE, group='', collide='box', bevel=None, **kw):
    s = Solid(name, (at[0], at[1], z), size, yaw, material, group=group, collide=collide,
              bevel=(0.03 if band_of(*at) == NEAR else 0.06 if band_of(*at) == MID else 0.0) if bevel is None else bevel,
              **kw)
    return L.add(s)


def base_line(s, faces=('front', 'back', 'left', 'right'), z=0.06, width=0.035):
    """The amber line round a solid's foot, a few centimetres above the ground, in a shallow channel."""
    w, d, h = s.size
    for f in faces:
        s.strips.append(strip_on(f, w, d, h, z, along=(0.04, 0.96), width=width, inset=0.025))


def lip_line(s, faces=('front',), below=0.09, width=0.035):
    """The amber line under a solid's top edge."""
    w, d, h = s.size
    for f in faces:
        s.strips.append(strip_on(f, w, d, h, h - below, along=(0.03, 0.97), width=width, inset=0.025))


def slot(s, face='front', at=0.5, z0=None, z1=None, width=0.14, depth=0.16, lit=True):
    """A vertical slot cut into a face, a glowing line at its back."""
    w, d, h = s.size
    z0 = h * 0.12 if z0 is None else z0
    z1 = h * 0.88 if z1 is None else z1
    if face in ('front', 'back'):
        sy = -1 if face == 'front' else 1
        c = (-w / 2 + w * at, sy * (d / 2 - depth / 2 + 0.001), (z0 + z1) / 2)
        size = (width, depth + 0.002, z1 - z0)
    else:
        sx = -1 if face == 'left' else 1
        c = (sx * (w / 2 - depth / 2 + 0.001), -d / 2 + d * at, (z0 + z1) / 2)
        size = (depth + 0.002, width, z1 - z0)
    s.cuts.append(Cut(c, size, lit))


def groove(s, face, z, height=0.12, depth=0.12, lit=False, along=(0.0, 1.0)):
    """A horizontal groove right across a face (a construction joint or a deep shadow line)."""
    w, d, h = s.size
    if face in ('front', 'back'):
        sy = -1 if face == 'front' else 1
        x0, x1 = -w / 2 + w * along[0] - 0.01, -w / 2 + w * along[1] + 0.01
        s.cuts.append(Cut(((x0 + x1) / 2, sy * (d / 2 - depth / 2 + 0.001), z), (x1 - x0, depth + 0.002, height), lit))
    else:
        sx = -1 if face == 'left' else 1
        y0, y1 = -d / 2 + d * along[0] - 0.01, -d / 2 + d * along[1] + 0.01
        s.cuts.append(Cut((sx * (w / 2 - depth / 2 + 0.001), (y0 + y1) / 2, z), (depth + 0.002, y1 - y0, height), lit))


def notch(s, corner, fw, fd, fh, top=True):
    """A corner taken out of a block: a step, a ledge. corner = (sx, sy) in {-1, 1}."""
    w, d, h = s.size
    cx, cy = corner[0] * (w / 2 - fw / 2 + 0.002), corner[1] * (d / 2 - fd / 2 + 0.002)
    cz = h - fh / 2 + 0.002 if top else fh / 2 - 0.002
    s.cuts.append(Cut((cx, cy, cz), (fw + 0.004, fd + 0.004, fh + 0.004)))


def stairs(L, name, foot, yaw, width, steps, rise, run, z=0.0, group='', lit=True):
    """A flight going up along the yaw's +Y from foot (the middle of its first riser)."""
    c = rot(yaw, (0.0, steps * run / 2))
    s = Solid(name, (foot[0] + c[0], foot[1] + c[1], z), (width, steps * run, steps * rise), yaw, CONCRETE,
              'stairs', 0.01, group=group, collide='stairs', params={'steps': steps, 'rise': rise, 'run': run})
    if lit:
        # A line along the foot of the first riser and one under the nosing of the top step.
        s.strips.append(Strip((-width / 2 + 0.15, -steps * run / 2 - 0.004, 0.05),
                              (width / 2 - 0.15, -steps * run / 2 - 0.004, 0.05), (0, -1, 0), 0.03, 0.0))
    return L.add(s)


def terrace(L, name, x0, x1, y0, y1, h, stair=None, z=0.0, yaw=0.0, group='', lips=('front',), bases=('front',)):
    """A raised level with a stair flight cut into its front edge: stair = (centre x, width, run, base z)."""
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    s = block(L, name, (cx, cy), (x1 - x0, y1 - y0, h), yaw, z, group=group, bevel=0.02)
    w, d = x1 - x0, y1 - y0
    if stair:
        sx, sw, srun, sbase = stair
        lx = sx - cx
        steps = max(3, round((h - sbase) / 0.18))
        s.params['stair'] = {'x': lx, 'width': sw, 'run': srun, 'base': sbase, 'steps': steps}
        # Lips on both sides of the stairwell, not across it.
        for f in lips:
            if f == 'front':
                left = (0.02, (lx - sw / 2 + w / 2) / w - 0.01)
                right = ((lx + sw / 2 + w / 2) / w + 0.01, 0.98)
                for part in (left, right):
                    if part[1] - part[0] > 0.02:
                        s.strips.append(strip_on('front', w, d, h, h - 0.09, along=part, width=0.04, inset=0.025))
                        s.strips.append(strip_on('front', w, d, h, 0.07, along=part, width=0.035, inset=0.025))
            else:
                lip_line(s, (f,))
        # The stairwell's own step lines: at the foot and under the top nosing.
        s.strips.append(Strip((lx - sw / 2 + 0.2, -d / 2 + 0.03, sbase + 0.05), (lx + sw / 2 - 0.2, -d / 2 + 0.03,
                                                                                 sbase + 0.05), (0, -1, 0), 0.03, 0.0))
    else:
        lip_line(s, lips, width=0.04)
        base_line(s, bases)
    return s


def arc(L, name, centre, r_in, r_out, a0, a1, z, h, group='', lit=True):
    """A curved slab of a ring hung round the plaza, from angle a0 to a1 (radians, from +X anticlockwise)."""
    s = Solid(name, (centre[0], centre[1], z), (r_out - r_in, 0, h), 0.0, CONCRETE, 'arc', 0.04, group=group,
              collide='none', params={'r_in': r_in, 'r_out': r_out, 'a0': a0, 'a1': a1, 'lit': lit},
              band=band_of(centre[0] + (r_in + r_out) / 2 * math.cos((a0 + a1) / 2),
                           centre[1] + (r_in + r_out) / 2 * math.sin((a0 + a1) / 2)))
    return L.add(s)


# ---------------------------------------------------------------- the plaza

PLAZA_CIRCLE = (0.0, 8.2)        # the circle ahead of the player: flush with the floor, a dark disc in an amber ring
CIRCLE_R = 4.3
DISC_R = 1.1
SPAWN_R = 1.9                    # the ring the player stands in


def plaza(L):
    # Joints of the polished slabs: rings round the player and round the basin, straight joints elsewhere.
    L.seams.append(('ring', (0.0, 0.0), SPAWN_R, 0.022, 0.0, 2 * math.pi))
    L.seams.append(('ring', (0.0, 0.0), SPAWN_R + 0.09, 0.012, 0.0, 2 * math.pi))
    L.seams.append(('ring', (0.0, 0.0), 6.3, 0.02, math.radians(120), math.radians(420)))
    L.seams.append(('ring', PLAZA_CIRCLE, DISC_R + 0.03, 0.03, 0.0, 2 * math.pi))
    L.seams.append(('ring', PLAZA_CIRCLE, 3.55, 0.016, 0.0, 2 * math.pi))
    L.seams.append(('ring', PLAZA_CIRCLE, 9.6, 0.02, 0.0, 2 * math.pi))
    L.seams.append(('amber', PLAZA_CIRCLE, CIRCLE_R + 0.02, 0.018, 0.0, 2 * math.pi))
    clear = [((0.0, 0.0), 6.4), (PLAZA_CIRCLE, 9.7)]
    for i in range(-5, 6):
        x = i * 3.0 + 0.4
        L.seams.append(('line', (x, -14.5), (x, 17.0), 0.018, clear))
    for j in range(-4, 11):
        y = j * 3.0 + 1.3
        L.seams.append(('line', (-15.5, y), (15.5, y), 0.018, clear))
    for k in range(10):          # radial joints round the circle
        a = 2 * math.pi * (k + 0.35) / 10
        c = PLAZA_CIRCLE
        L.seams.append(('line', (c[0] + 4.45 * math.cos(a), c[1] + 4.45 * math.sin(a)),
                        (c[0] + 9.5 * math.cos(a), c[1] + 9.5 * math.sin(a)), 0.016, []))
    for k in range(7):
        a = math.radians(140 + k * 40)
        L.seams.append(('line', ((SPAWN_R + 0.1) * math.cos(a), (SPAWN_R + 0.1) * math.sin(a)),
                        (6.3 * math.cos(a), 6.3 * math.sin(a)), 0.016, []))
    # Amber lines let into the floor, along the foot of the levels round the plaza.
    for name, a, b in (('floor line west', (-8.7, 2.1), (-8.7, 9.8)), ('floor line east', (9.4, -5.5), (9.4, 4.0)),
                       ('floor line north west', (-12.6, 15.6), (-4.6, 15.6)),
                       ('floor line north east', (4.8, 15.4), (13.0, 15.4)),
                       ('floor line rear', (-5.5, -12.6), (6.0, -12.6))):
        L.seams.append(('amber', a, b, 0.045, []))


# ---------------------------------------------------------------- near: the levels round the plaza

def near_architecture(L, rng):
    g = 'near'
    # Ahead: three levels rising away, each with its stairs cut into it, off centre and turned a little.
    terrace(L, 'level north 1', -15.5, 15.5, 17.0, 31.0, 1.0, stair=(0.0, 7.2, 2.6, 0.0), group=g)
    terrace(L, 'level north 2', -11.0, 12.5, 29.5, 46.0, 2.2, stair=(1.2, 5.6, 3.4, 1.0), group=g)
    terrace(L, 'level north 3', -9.0, 15.0, 44.5, 72.0, 3.6, stair=(-1.6, 6.4, 4.0, 2.2), group=g,
            yaw=math.radians(-3))
    # Left: a long low level along the plaza with a short flight up from it, a higher one behind.
    terrace(L, 'level west 1', -24.0, -9.6, -6.5, 13.5, 1.1, stair=None, group=g, lips=('right',), bases=('right',))
    s = stairs(L, 'stairs west 1', (-8.2, 10.8), math.radians(-90), 3.6, 6, 0.183, 0.36, group=g)
    terrace(L, 'level west 2', -36.0, -22.0, -14.0, 22.0, 2.6, group=g, lips=('right',), bases=())
    stairs(L, 'stairs west 2', (-20.6, 2.5), math.radians(-90), 4.2, 8, 0.1875, 0.34, z=1.1, group=g)
    # Right: past the pod a level at knee height, its stairs to the plaza at its south end.
    terrace(L, 'level east 1', 10.6, 24.0, -10.0, 13.0, 0.9, group=g, lips=('left',), bases=('left',))
    stairs(L, 'stairs east 1', (9.0, -6.2), math.radians(90), 4.6, 5, 0.18, 0.36, group=g)
    terrace(L, 'level east 2', 22.5, 36.0, -18.0, 26.0, 2.3, group=g, lips=('left',), bases=())
    stairs(L, 'stairs east 2', (21.0, 16.5), math.radians(90), 3.8, 8, 0.175, 0.34, z=0.9, group=g)
    # Behind: a level the width of the plaza, stairs up it at both ends, a higher one behind that.
    s = terrace(L, 'level south 1', -17.0, 17.0, -27.0, -14.6, 1.3, group=g, yaw=math.pi, lips=('front',),
                bases=('front',))
    stairs(L, 'stairs south 1 west', (-7.2, -13.0), math.pi, 4.4, 7, 0.186, 0.34, group=g)
    stairs(L, 'stairs south 1 east', (7.6, -13.0), math.pi, 5.2, 7, 0.186, 0.34, group=g)
    terrace(L, 'level south 2', -21.0, 19.0, -44.0, -25.0, 2.8, group=g, yaw=math.pi, lips=('front',), bases=())
    stairs(L, 'stairs south 2', (2.8, -25.3), math.pi, 6.0, 8, 0.1875, 0.36, z=1.3, group=g)


MONUMENT_PILLAR = 6.4              # the T's stem: tall, the heavy head held high over the plinth


def monument_south(L):
    """Behind the player: a tall pillar with a lit slot holding a heavy block high over a low plinth."""
    g = 'monument south'
    yaw = math.radians(3)
    at = (0.6, -9.6)
    p = block(L, 'monument plinth', at, (4.6, 3.3, 0.8), yaw, group=g)
    base_line(p, ('front', 'left', 'right'))
    lip_line(p, ('front',), below=0.08)
    h = MONUMENT_PILLAR
    pil = block(L, 'monument pillar', at, (1.35, 1.35, h), yaw, 0.8, group=g)
    slot(pil, 'front', 0.5, 0.25, h - 0.25, width=0.16, depth=0.2)
    slot(pil, 'back', 0.5, 0.25, h - 0.25, width=0.16, depth=0.2)
    top = block(L, 'monument head', (at[0] - 0.15, at[1] + 0.2), (3.5, 3.4, 4.6), yaw + math.radians(4), 0.8 + h,
                group=g)
    top.strips.append(strip_on('bottom', 3.5, 3.4, 4.6, 0, along=(0.08, 0.92), at=0.06, width=0.05, inset=0.0))
    groove(top, 'front', 1.4, 0.1, 0.1)


def assemblies_near(L, rng):
    """Block assemblies round the plaza and on its levels: stacked, offset, turned, slotted, stepped."""
    def stack(name, at, parts, yaw, z=0.0, lit_base=True):
        """parts: (dx, dy, dz, w, d, h, dyaw degrees) on top of each other, in the assembly's frame."""
        out = []
        for i, (dx, dy, dz, w, d, h, dyaw) in enumerate(parts):
            p = rot(yaw, (dx, dy))
            s = block(L, '%s %d' % (name, i + 1), (at[0] + p[0], at[1] + p[1]), (w, d, h),
                      yaw + math.radians(dyaw), z + dz, group=name)
            out.append(s)
        if lit_base:
            base_line(out[0], ('front', 'left', 'right', 'back'))
        return out

    # Forward left, on level north 1: a tall block and a lower stepped one beside it.
    a = stack('north west pair', (-9.6, 22.2), [(0, 0, 0, 3.0, 3.0, 4.4, 0), (-0.3, 0.2, 4.4, 3.4, 3.2, 2.1, 6)],
              math.radians(7), 1.0)
    slot(a[0], 'front', 0.3, 0.3, 4.0)
    a[1].strips.append(strip_on('bottom', 3.4, 3.2, 2.1, 0, along=(0.1, 0.9), at=0.04, width=0.05, inset=0.0))
    b = stack('north west low', (-5.2, 25.6), [(0, 0, 0, 2.6, 2.4, 2.6, 0)], math.radians(-11), 1.0)
    notch(b[0], (1, -1), 1.1, 1.0, 0.9)
    lip_line(b[0], ('front', 'left'))
    # Forward right: a heavy block on the plaza, its lit foot reflecting in the floor.
    c = stack('north east block', (7.4, 10.6), [(0, 0, 0, 3.2, 3.0, 3.3, 0)], math.radians(-14))
    groove(c[0], 'front', 2.35, 0.1, 0.12, lit=True, along=(0.0, 0.62))
    d = stack('north east pair', (9.8, 21.4), [(0, 0, 0, 3.4, 2.8, 2.6, 0), (0.5, 0.4, 2.6, 2.8, 3.4, 4.8, -9)],
              math.radians(4), 1.0)
    slot(d[1], 'front', 0.72, 0.4, 4.2)
    e = stack('north east tall', (12.6, 26.8), [(0, 0, 0, 2.8, 3.4, 6.4, 0)], math.radians(-6), 1.0)
    lip_line(e[0], ('front', 'left'))
    # Left of the player, on level west 1: a block with a lit slot, a wider one set on it off centre.
    f = stack('west pillar', (-12.4, 0.4), [(0, 0, 0, 2.6, 2.6, 4.2, 0), (-0.5, 0.3, 4.2, 3.6, 3.1, 2.4, 9)],
              math.radians(-4), 1.1)
    slot(f[0], 'right', 0.5, 0.3, 3.9)
    f[1].strips.append(strip_on('bottom', 3.6, 3.1, 2.4, 0, along=(0.1, 0.9), at=0.95, width=0.05, inset=0.0))
    g_ = stack('west low', (-15.6, 8.8), [(0, 0, 0, 3.8, 2.6, 1.4, 0)], math.radians(13), 1.1)
    lip_line(g_[0], ('right', 'front'))
    h = stack('west far', (-17.8, -3.6), [(0, 0, 0, 3.2, 3.6, 7.5, 0), (0.8, -0.4, 7.5, 2.4, 2.4, 3.0, -7)],
              math.radians(8), 1.1)
    slot(h[0], 'right', 0.62, 0.5, 6.8, width=0.12)
    # Behind left / right.
    i = stack('south west', (-8.4, -7.6), [(0, 0, 0, 2.4, 2.4, 3.0, 0), (0.4, -0.2, 3.0, 3.1, 2.8, 2.6, -12)],
              math.radians(18))
    slot(i[1], 'front', 0.5, 0.2, 2.3)
    j = stack('south east', (8.8, -7.4), [(0, 0, 0, 3.4, 3.2, 1.9, 0), (0.6, 0.4, 1.9, 2.2, 2.2, 5.2, 7)],
              math.radians(-16))
    lip_line(j[0], ('front', 'left'))
    slot(j[1], 'left', 0.5, 0.3, 4.8)
    k = stack('south west high', (-12.6, -19.4), [(0, 0, 0, 3.0, 3.0, 5.2, 0)], math.radians(-7), 1.3)
    slot(k[0], 'front', 0.38, 0.3, 4.8)
    m = stack('south east high', (11.8, -20.2), [(0, 0, 0, 3.8, 3.0, 3.2, 0), (-0.4, 0.3, 3.2, 3.0, 3.6, 3.6, 11)],
              math.radians(9), 1.3)
    groove(m[1], 'front', 1.1, 0.12, 0.14, lit=True)
    # Right, past the pod, on level east 1.
    n = stack('east pillar', (15.2, 3.8), [(0, 0, 0, 3.0, 3.0, 4.2, 0)], math.radians(-9), 0.9)
    slot(n[0], 'left', 0.35, 0.3, 3.9)
    slot(n[0], 'left', 0.7, 0.3, 2.6)
    o = stack('east low', (16.2, -4.6), [(0, 0, 0, 4.2, 3.0, 2.4, 0)], math.radians(6), 0.9)
    lip_line(o[0], ('left', 'front'))
    # Low blocks on the plaza's edge to sit on and climb.
    for name, at, size, yaw in (('bench west', (-7.6, 13.4), (2.6, 1.1, 0.48), 9),
                                ('bench east', (6.4, -3.4), (2.2, 1.0, 0.45), -21),
                                ('bench south', (-4.4, -10.2), (2.4, 1.2, 0.5), 4)):
        s = block(L, name, at, size, math.radians(yaw), group='benches')
        base_line(s, ('front', 'back', 'left', 'right'), z=0.05, width=0.025)


def west_court(L):
    """Left of the player, between the plaza and level west 1, behind the terminal and the table: a low terrace
    stepping up towards the level with a step onto it, a stack of blocks with a lit slot on it, a tall notched column
    and a low wall -- the ledges, lifts and cuts the reference's left side is made of."""
    g = 'west court'
    terrace(L, 'level west court', -9.6, -6.0, -5.2, 1.6, 0.45, group=g, lips=('right', 'front'), bases=('right',))
    step = block(L, 'west court step', (-5.55, -1.6), (0.9, 1.8, 0.22), group=g)
    base_line(step, ('right',), z=0.05, width=0.025)
    a = block(L, 'west court stack 1', (-7.9, -2.9), (2.2, 2.0, 2.8), math.radians(10), 0.45, group=g)
    slot(a, 'right', 0.5, 0.3, 2.5)
    b = block(L, 'west court stack 2', (-8.1, -2.7), (1.6, 2.6, 1.5), math.radians(24), 3.25, group=g)
    b.strips.append(strip_on('bottom', 1.6, 2.6, 1.5, 0, along=(0.1, 0.9), at=0.92, width=0.04, inset=0.0))
    col = block(L, 'west court column', (-6.7, 3.6), (1.4, 1.4, 6.2), math.radians(-7), group=g)
    slot(col, 'right', 0.5, 0.35, 5.6, width=0.12)
    notch(col, (1, -1), 0.55, 0.5, 1.4)
    wall = block(L, 'west court wall', (-4.4, -5.7), (3.8, 0.6, 0.95), math.radians(22), group=g)
    base_line(wall, ('front', 'back'), z=0.05, width=0.03)
    lip_line(wall, ('back',), below=0.07, width=0.03)


def pod_mass(L):
    """The pod's frame and the masses it is set into (the pod's niche, liner, label and body are props)."""
    g = 'pod'
    at = L.props['pod'][0]
    yaw = L.props['pod'][1]

    def put(name, dx, dy, z, w, d, h, dyaw=0.0, material=CONCRETE_DARK):
        p = rot(yaw, (dx, dy))
        return block(L, name, (at[0] + p[0], at[1] + p[1]), (w, d, h), yaw + math.radians(dyaw), z, material,
                     group=g, bevel=0.012)

    found = put('pod foundation', 0.15, 0.25, 0.0, 3.3, 2.2, 0.16)
    found.strips.append(strip_on('front', 3.3, 2.2, 0.16, 0.08, along=(0.04, 0.42), width=0.03, inset=0.02))
    found.strips.append(strip_on('front', 3.3, 2.2, 0.16, 0.08, along=(0.62, 0.96), width=0.03, inset=0.02))
    put('pod left jamb', -0.95, 0.25, 0.16, 0.7, 1.7, 3.1)
    put('pod left upper', -1.0, 0.45, 3.26, 0.85, 2.1, 1.5, 2)
    put('pod crown', -0.2, 0.55, 4.0, 2.4, 1.9, 0.95, -3)
    put('pod high block', -0.45, 1.05, 4.95, 3.0, 2.1, 1.7, 5, CONCRETE)
    put('pod right jamb', 0.95, 0.25, 0.16, 0.6, 1.7, 3.84)
    lab = put('pod label block', 1.7, 0.05, 1.55, 0.95, 1.2, 2.05, -4)
    lab.strips.append(strip_on('front', 0.95, 1.2, 2.05, 0.0, 2.05, vertical=True, at=0.06, width=0.025, inset=0.02))
    bench = put('pod side bench', 2.15, 0.35, 0.16, 1.9, 2.0, 1.35, -6, CONCRETE)
    slot(bench, 'front', 0.22, 0.12, 1.2, width=0.09, depth=0.12)
    slot(bench, 'front', 0.48, 0.12, 1.2, width=0.09, depth=0.12)
    base_line(bench, ('front', 'right'))
    put('pod back mass', 0.6, 2.2, 0.0, 4.6, 2.4, 6.4, 3, CONCRETE)


# ---------------------------------------------------------------- the city round it

def level_top(L, x, y):
    """The top of the highest level round the plaza under (x, y), 0 off them."""
    top = 0.0
    for s in L.solids:
        if not s.name.startswith('level'):
            continue
        lx, ly = rot(-s.yaw, (x - s.origin[0], y - s.origin[1]))
        if abs(lx) <= s.size[0] / 2 and abs(ly) <= s.size[1] / 2:
            top = max(top, s.origin[2] + s.size[2])
    return top


def towers(L, rng):
    """Tall blocks standing round the plaza, set back as they rise, some turned a few degrees at each step. The
    nearest ring rises far over the plaza, so that looking up the player sees them close in on the sky."""
    placed = []

    def tower(n, x, y, w, d, h_total, r, z0=0.0):
        yaw = rng.choice((0.0, math.pi / 2)) + math.radians(rng.uniform(-14, 14))
        z = z0
        segs = max(1, min(6, int(h_total / rng.uniform(18, 40))))
        cw, cd, cx, cy = w, d, x, y
        for k in range(segs):
            sh = h_total / segs * rng.uniform(0.7, 1.3) if k < segs - 1 else max(6.0, h_total - z)
            s = block(L, 'tower %03d.%d' % (n, k), (cx, cy), (cw, cd, sh), yaw, z, group='towers',
                      collide='box' if k == 0 else 'none')
            if k > 0 and rng.random() < 0.7:
                s.strips.append(strip_on('bottom', cw, cd, sh, 0, along=(0.05, 0.95), at=0.03,
                                         width=max(0.06, r * 0.003), inset=0.0))
            if rng.random() < 0.3 and r < 110:
                slot(s, rng.choice(('front', 'left', 'right', 'back')), rng.uniform(0.25, 0.75),
                     sh * 0.1, sh * 0.9, width=0.2 + r * 0.003, depth=0.3)
            if k == 0 and r < 90 and rng.random() < 0.6:
                base_line(s, ('front', 'back', 'left', 'right'), width=0.06)
            if rng.random() < 0.25 and r < 150:
                groove(s, rng.choice(('front', 'left', 'right', 'back')), sh * rng.uniform(0.3, 0.8), 0.25, 0.3)
            z += sh
            # The next segment shrinks or grows a little, shifts, sometimes turns; now and then a slab juts out.
            cw *= rng.uniform(0.75, 1.15)
            cd *= rng.uniform(0.75, 1.15)
            off = rot(yaw, (rng.uniform(-0.25, 0.25) * w, rng.uniform(-0.25, 0.25) * d))
            cx, cy = cx + off[0], cy + off[1]
            if rng.random() < 0.35:
                yaw += math.radians(rng.uniform(-9, 9))
            if rng.random() < 0.3:
                jw, jd, jh = cw * rng.uniform(1.3, 2.0), cd * rng.uniform(0.5, 0.8), rng.uniform(2.0, 4.0)
                t = block(L, 'tower %03d.%d jut' % (n, k), (cx, cy), (jw, jd, jh), yaw, z - jh * rng.uniform(0.5, 2.0),
                          group='towers', collide='none')
                t.strips.append(strip_on('bottom', jw, jd, jh, 0, along=(0.06, 0.94), at=0.08,
                                         width=max(0.06, r * 0.003), inset=0.0))

    keep_clear = [(s.origin[0], s.origin[1], math.hypot(s.size[0], s.size[1]) / 2 + 1.5) for s in L.solids
                  if s.band == NEAR and not s.name.startswith('level')]
    keep_clear += [(s.world((st['x'], -s.size[1] / 2 + st['run'] / 2, 0))[0],
                    s.world((st['x'], -s.size[1] / 2 + st['run'] / 2, 0))[1], max(st['width'], st['run']) / 2 + 5.0)
                   for s in L.solids if s.name.startswith('level') and (st := s.params.get('stair'))]
    keep_clear += [(s.origin[0], s.origin[1], max(s.size[0], s.size[1]) / 2 + 5.0) for s in L.solids
                   if s.shape == 'stairs']

    def ring_of(count, r0, r1, mode, h0, h1, size0, size1):
        n = 0
        for _ in range(40000):
            if n >= count:
                return
            r = rng.triangular(r0, r1, mode)
            a = rng.uniform(0, 2 * math.pi)
            x, y = r * math.sin(a), r * math.cos(a) + 6.0
            w, d = rng.uniform(size0, size1), rng.uniform(size0, size1)
            if abs(x) < 12 + w / 2 and 40 < y < 160 and rng.random() < 0.85:
                continue      # the vista up the stairs stays open, mostly
            if any(math.hypot(x - px, y - py) < (w + pw) * 0.62 + 2.0 for px, py, pw in placed):
                continue
            if -18 - w / 2 < x < 18 + w / 2 and -17 - d / 2 < y < 19 + d / 2:
                continue      # the plaza
            if any(math.hypot(x - cx, y - cy) < cr + max(w, d) / 2 for cx, cy, cr in keep_clear):
                continue      # stairs, their landings, what stands on the levels
            placed.append((x, y, max(w, d)))
            n += 1
            tower(len(placed), x, y, w, d, rng.uniform(h0, h1), r, level_top(L, x, y))

    ring_of(34, 24.0, 60.0, 34.0, 70.0, 230.0, 4.0, 9.0)
    ring_of(30, 60.0, 110.0, 70.0, 90.0, 260.0, 5.0, 12.0)
    ring_of(150, 95.0, 300.0, 120.0, 40.0, 190.0, 7.0, 18.0)
    ring_of(90, 300.0, 520.0, 380.0, 50.0, 220.0, 12.0, 30.0)


def floating(L, rng):
    """Blocks hanging at every height, in loose clusters, spread evenly over the sky's angles so that wherever the
    player looks up there are blocks, thinning into the haze with distance. The sky right over the plaza stays
    open."""
    placed = []
    n = 0
    for _ in range(90000):
        if n >= 640:
            break
        r = rng.triangular(18.0, 420.0, 60.0)
        a = rng.uniform(0, 2 * math.pi)
        x, y = r * math.sin(a), 6.0 + r * math.cos(a)
        elevation = math.radians(rng.uniform(6.0, 66.0))
        z = max(7.0, EYE + r * math.tan(elevation))
        if z > 330:
            continue
        if abs(x) < 9 and 20 < y < 120 and z < 14:
            continue      # the vista up the stairs
        scale = 1.0 + r / 110
        w, d, h = (rng.uniform(3.0, 9.0) * scale, rng.uniform(3.0, 9.0) * scale, rng.uniform(2.2, 8.0) * scale)
        if rng.random() < 0.18:
            w, h = w * 2.2, h * 0.5              # a long slab
        if rng.random() < 0.12:
            h = h * 2.4                          # a hanging column
        if any(abs(x - px) < (w + pw) / 2 + 1.2 and abs(y - py) < (d + pd) / 2 + 1.2 and abs(z - pz) < (h + ph) / 2 + 1.5
               for px, py, pz, pw, pd, ph in placed):
            continue
        placed.append((x, y, z + h / 2, w, d, h))
        n += 1
        yaw = rng.choice((0.0, math.pi / 2)) + math.radians(rng.uniform(-18, 18))
        s = block(L, 'hanging %03d' % n, (x, y), (w, d, h), yaw, z, group='hanging', collide='none')
        lw = max(0.05, r * 0.0035)
        # One glowing line to a hanging block, its cantilever counted with it: the first of these it draws.
        lit = False
        if rng.random() < 0.62:
            # The line along the bottom edge on the side facing the plaza.
            face = 'front' if abs(math.cos(a)) > abs(math.sin(a)) else 'left'
            s.strips.append(strip_on(face, w, d, h, 0.06 + lw, along=(0.04, 0.96), width=lw, inset=0.0))
            lit = True
        if rng.random() < 0.3:
            at = rng.uniform(0.1, 0.9)
            if not lit:
                s.strips.append(strip_on('bottom', w, d, h, 0, along=(0.06, 0.94), at=at, width=lw * 1.4, inset=0.0))
                lit = True
        if rng.random() < 0.3 and r < 120:
            face, gz, glow = rng.choice(('front', 'left')), h * rng.uniform(0.3, 0.7), rng.random() < 0.4
            groove(s, face, gz, 0.15 * scale, 0.3, lit=glow and not lit)
            lit = lit or glow
        # A smaller block stuck under or on top of it, offset: a cantilever.
        if rng.random() < 0.35 and r < 200:
            k = rng.uniform(0.35, 0.65)
            dz = -h * k * 0.8 if rng.random() < 0.5 else h
            off = rot(yaw, (rng.uniform(-0.3, 0.3) * w, rng.uniform(-0.3, 0.3) * d))
            t = block(L, 'hanging %03d.b' % n, (x + off[0], y + off[1]), (w * k, d * k * 1.2, h * k * 0.8),
                      yaw + math.radians(rng.uniform(-12, 12)), z + dz, group='hanging', collide='none')
            if dz < 0 and rng.random() < 0.6 and not lit:
                t.strips.append(strip_on('bottom', w * k, d * k * 1.2, h * k * 0.8, 0, along=(0.08, 0.92), at=0.5,
                                         width=lw, inset=0.0))


def canopy(L, rng):
    """The big blocks hanging close over the plaza's edges, 12-40 m up: what the player sees first on looking up,
    dark undersides with amber lines, the sky between them."""
    placed = []
    n = 0
    for _ in range(20000):
        if n >= 46:
            break
        r = rng.uniform(17.0, 48.0)
        a = rng.uniform(0, 2 * math.pi)
        x, y = r * math.sin(a), 6.0 + r * math.cos(a)
        z = rng.uniform(11.0, 16.0 + r * 0.55)
        if math.degrees(math.atan2(math.hypot(x, y - 6.0), z)) < 26:
            continue
        w, d, h = rng.uniform(4.5, 11.0), rng.uniform(4.0, 9.0), rng.uniform(3.0, 7.5)
        if rng.random() < 0.25:
            w, h = w * 1.8, h * 0.55
        if any(abs(x - px) < (w + pw) / 2 + 2.0 and abs(y - py) < (d + pd) / 2 + 2.0 and abs(z - pz) < (h + ph) / 2 + 2.5
               for px, py, pz, pw, pd, ph in placed):
            continue
        placed.append((x, y, z + h / 2, w, d, h))
        n += 1
        yaw = math.atan2(x, y - 6.0) + rng.choice((0.0, math.pi / 2)) + math.radians(rng.uniform(-20, 20))
        s = block(L, 'canopy %02d' % n, (x, y), (w, d, h), yaw, z, group='canopy', collide='none')
        # One glowing line to a block: along the bottom edge of the side facing the plaza. The other details are
        # cut, not lit (the random draws stay as they were, so the rest of the city keeps its place).
        s.strips.append(strip_on('front', w, d, h, 0.1, along=(0.04, 0.96), width=0.06, inset=0.0))
        if rng.random() < 0.6:
            rng.uniform(0.15, 0.85)                 # where a second line along the underside went
        if rng.random() < 0.4:
            face, gz = rng.choice(('front', 'left', 'right')), h * rng.uniform(0.35, 0.7)
            rng.random()                            # whether the groove glowed
            groove(s, face, gz, 0.16, 0.25)
        if rng.random() < 0.45:
            k = rng.uniform(0.4, 0.7)
            off = rot(yaw, (rng.uniform(-0.3, 0.3) * w, rng.uniform(-0.3, 0.3) * d))
            t = block(L, 'canopy %02d.b' % n, (x + off[0], y + off[1]), (w * k, d * k, h * rng.uniform(0.6, 1.4)),
                      yaw + math.radians(rng.uniform(-14, 14)), z + h, group='canopy', collide='none')
            if rng.random() < 0.5:
                slot(t, rng.choice(('front', 'left')), 0.5, None, None, width=0.14, depth=0.2, lit=False)


def rings(L, rng):
    """Broken rings of curved slabs hung round the plaza, one above the other."""
    for name, r, z, h, depth, gaps in (('ring 1', 52.0, 27.0, 2.6, 7.0, 6), ('ring 2', 76.0, 41.0, 3.2, 9.0, 6),
                                       ('ring 3', 112.0, 64.0, 4.0, 12.0, 7)):
        segs = 20
        skip = set(rng.sample(range(segs), gaps))
        for k in range(segs):
            if k in skip:
                continue
            a0 = 2 * math.pi * k / segs + rng.uniform(0.0, 0.02)
            a1 = 2 * math.pi * (k + 1) / segs - rng.uniform(0.01, 0.04)
            dz = rng.uniform(-0.8, 0.8)
            dr = rng.uniform(-1.2, 1.2)
            arc(L, '%s seg %02d' % (name, k), (0.0, 6.0), r + dr, r + dr + depth, a0, a1, z + dz, h, group=name)


def ground_far(L, rng):
    """The far field to the edge of the square kilometre: low blocks and steps on the ground under the towers, so
    there is no empty floor anywhere."""
    n = 0
    for gx in range(-480, 481, 32):
        for gy in range(-480, 481, 32):
            if abs(gx) < 60 and -70 < gy < 90:
                continue
            for _ in range(2):
                x, y = gx + rng.uniform(-14, 14), gy + rng.uniform(-14, 14)
                w, d, h = rng.uniform(6, 20), rng.uniform(6, 20), rng.uniform(1.0, 7.0)
                n += 1
                s = block(L, 'ground block %04d' % n, (x, y), (w, d, h), math.radians(rng.uniform(-30, 30)),
                          group='ground', collide='box')
                if rng.random() < 0.35:
                    s.strips.append(strip_on('front', w, d, h, h - 0.25, along=(0.03, 0.97), width=0.25, inset=0.0))


# ---------------------------------------------------------------- parkour

JUMP_RISE = 0.55      # what a running jump climbs comfortably: its top is about 1 m (measured live), 0.75 s up
JUMP_GAP = (0.4, 1.0) # edge to edge; a running jump covers about 3 m


def parkour(L, rng):
    """Two short routes of carved blocks up from the plaza's sides to a lookout over it on level north 1: west from
    level west 1, east from level east 1. Each block is 0.55 m over the last, its near edge 0.4-1 m past the last
    one's far edge (lobby_parkour_test.py runs them in the game)."""
    def jump_block(name, at, top, w, d, yaw, z0, style):
        s = block(L, name, at, (w, d, top - z0), math.radians(yaw), z0, group='parkour', bevel=0.02)
        h = top - z0
        if style == 'window' and h > 1.1:
            s.cuts.append(Cut((0.0, 0.0, h * 0.5), (w * 0.38, d + 0.1, h * 0.42)))
        elif style == 'bay' and h > 1.1:
            s.cuts.append(Cut((w * 0.3, -d * 0.3, h * 0.48), (w * 0.44, d * 0.44, h * 0.5)))
        elif style == 'slot':
            slot(s, 'front', 0.5, h * 0.15, h * 0.8, width=0.12, depth=0.14)
        s.strips.append(strip_on('front', w, d, h, h - 0.08, along=(0.15, 0.85), width=0.025, inset=0.015))
        return s

    def lookout(name, at, top, w, d, yaw):
        s = block(L, name, at, (w, d, top - 1.0), math.radians(yaw), 1.0, group='parkour', bevel=0.02)
        lip_line(s, ('front', 'left', 'right'), below=0.07, width=0.03)
        base_line(s, ('front', 'left', 'right'))
        return s.name

    def route(name, start, start_top, blocks, end):
        """blocks: (centre, w, d, yaw, z0, style) going north; each top JUMP_RISE over the last."""
        names, top = [], start_top
        for i, (at, w, d, yaw, z0, style) in enumerate(blocks):
            top += JUMP_RISE
            names.append(jump_block('parkour %s %d' % (name, i + 1), at, top, w, d, yaw, z0, style).name)
        names.append(lookout('lookout ' + name, end[0], top + JUMP_RISE, end[1], end[2], end[3]))
        L.parkour.append((name, [start] + names))

    # z0: the plaza's floor (0) or level north 1's top (1.0, north of y 17).
    route('west', 'level west 1', 1.1,
          [((-13.0, 15.2), 2.6, 1.8, 4, 0.0, 'slot'), ((-11.2, 18.2), 2.2, 2.2, -6, 1.0, 'window'),
           ((-13.6, 21.2), 2.4, 2.2, 7, 1.0, 'bay'), ((-12.4, 24.6), 2.2, 2.4, -3, 1.0, 'plain')],
          ((-10.4, 27.6), 3.0, 2.8, 5))
    route('east', 'level east 1', 0.9,
          [((13.4, 14.7), 2.6, 1.8, -5, 0.0, 'window'), ((15.6, 17.6), 2.2, 2.2, 6, 1.0, 'bay'),
           ((13.2, 20.6), 2.4, 2.2, -8, 1.0, 'slot'), ((15.8, 23.4), 2.2, 2.4, 4, 1.0, 'plain')],
          ((17.8, 26.4), 3.0, 2.8, -6))


# ---------------------------------------------------------------- moving blocks

def moving(L, rng):
    """Blocks drifting the way the tutorial's do: slowly up and down, along, round."""
    for i in range(14):
        r = rng.uniform(22, 90)
        a = rng.uniform(0, 2 * math.pi)
        x, y = r * math.sin(a), 6.0 + r * math.cos(a)
        z = rng.uniform(9.0, 30.0)
        w, d, h = rng.uniform(2.5, 6.0), rng.uniform(2.5, 6.0), rng.uniform(2.0, 5.0)
        s = Solid('moving %02d' % (i + 1), (x, y, z), (w, d, h), math.radians(rng.uniform(0, 90)), CONCRETE,
                  bevel=0.04, collide='none', group='moving')
        s.strips.append(strip_on('front', w, d, h, 0.08, along=(0.05, 0.95), width=0.05, inset=0.0))
        kind = rng.choice(('rise', 'rise', 'slide', 'turn'))
        if kind == 'rise':
            travel, spin = (0.0, 0.0, rng.uniform(2.5, 6.0)), 0.0
        elif kind == 'slide':
            t = rot(rng.uniform(0, math.pi), (rng.uniform(4.0, 9.0), 0.0))
            travel, spin = (t[0], t[1], rng.uniform(-1.0, 1.0)), 0.0
        else:
            travel, spin = (0.0, 0.0, rng.uniform(0.5, 1.5)), rng.choice((90.0, -90.0, 180.0))
        L.motions.append(Motion(s, travel, spin, rng.uniform(18.0, 40.0), rng.uniform(0, 1)))


# ---------------------------------------------------------------- lights

def lights(L):
    warm = (1.0, 0.52, 0.16)
    # Light the glowing lines near the player would throw on the floor and walls (in the game emissive surfaces
    # light nothing): long soft area lights just off them.
    for name, pos, size, target, power in (
            ('north 1 lip west', (-8.0, 16.6, 0.95), (7.0, 0.1), (-8.0, 14.0, 0.0), 80),
            ('north 1 lip east', (8.5, 16.6, 0.95), (6.0, 0.1), (8.5, 14.0, 0.0), 70),
            ('west 1 lip', (-9.3, 3.5, 1.05), (0.1, 9.0), (-6.0, 3.5, 0.0), 80),
            ('east 1 lip', (10.3, 1.5, 0.85), (0.1, 8.0), (7.0, 1.5, 0.0), 60),
            ('south 1 lip', (0.0, -14.3, 1.25), (12.0, 0.1), (0.0, -11.0, 0.0), 90),
            ('west court lip', (-5.9, -1.8, 0.5), (0.1, 6.0), (-3.0, -1.8, 0.0), 45),
            ('monument slot', (0.6, -8.6, 0.8 + MONUMENT_PILLAR / 2), (0.2, MONUMENT_PILLAR - 1.0), (0.6, -5.0, 0.6),
             55)):
        L.lights.append(Light(name, pos, warm, power, 'area', size, target, radius=7.0))
    terminal_light(L)


def terminal_light(L):
    """A warm downlight over the terminal so its screen frame reads (the pod's niche is lit by its own lines and the
    body standing in it)."""
    t_at = L.props['terminal'][0]
    L.lights.append(Light('terminal key', (t_at[0] + 0.2, t_at[1] - 0.8, 2.6), (1.0, 0.75, 0.55), 25, 'area',
                          (0.6, 0.6), (t_at[0], t_at[1], 0.8), radius=4.0))


# ---------------------------------------------------------------- all of it

# Where the hand-modelled pieces stand: bearing from the player's heading (degrees, + to the right) and distance (m).
PROPS = {'terminal': (-60.0, 2.6), 'table': (-112.0, 2.3), 'pod': (62.0, 5.0)}
# In the tutorial hall the menu is a hologram in the air (HOLO_MENU, mods/cet/CyberpunkVRPort_Lobby modules/holo.lua)
# and the only piece is the table, off to the left and a step away: out of the view the player spawns with (the desk is
# 2 m long, so at 1.85 m it spans +-29 deg round its bearing: -147 .. -89 deg, past a headset's +-55), and clear of the
# menu's left panel (-61 deg) -- turn left to find it.
TUTORIAL_PROPS = {'table': (-118.0, 1.85)}
# The hologram menu's middle (bearing, distance, height of its centre) and its panels' size in metres -- the
# terminal's screen, whose plane and canvas each panel is: the panel the player is on in the middle, the one before
# it to its left; a new one comes in from the right.
# Near and at the eyes: at 1.1 m a panel is 48 x 38 deg (it was 28 x 22 at 1.7 m), its middle 15 cm under eyes at 1.7 m.
# A panel is a screen of its own, the terminal's made a little larger (the same canvas, 2630 px a metre) to read
# from the spot more easily than the terminal's 0.86 x 0.66 m.
HOLO_SCREEN = (0.98, 0.75)
# The picture of a save beside the middle panel (CONTINUE pointed at): a screen of its own, the same canvas smaller.
HOLO_PREVIEW = (0.52, 0.40)
# The new game's window (modules/holo.lua M.window, modules/creation.lua): once the life path is chosen the middle panel
# grows into a window a little larger than V at full height, who stands behind its right part -- seen through it, the
# glass additive -- the new game's rows on its left. Upright (the panels lean to the eyes; V stands), its foot near the
# floor, 1.5 m ahead: +15 .. -47 deg from eyes at 1.7 m, V behind it larger than at 2 m (it read as small there). Its
# canvas has 1450 px a metre (as many a degree at 2 m as a panel's at 1.1 m): its rows and letters read larger.
HOLO_WINDOW = (2.2, 2.0)
HOLO_WINDOW_CANVAS = (3190, 2900)
# Straight ahead: at -12 deg it read as off to the left.
HOLO_MENU = {'bearing': 0.0, 'dist': 1.1, 'height': 1.55, 'panel': HOLO_SCREEN, 'gap': 0.08,
             'window': {'bearing': -10.0, 'dist': 1.5, 'height': 1.1, 'size': HOLO_WINDOW}}
# Where the figure the new game builds (and an engram brings back) stands, facing the player: right of the menu.
FIGURE = {'bearing': 38.0, 'dist': 2.3}

def build(seed=2077):
    rng = random.Random(seed)
    L = Layout()
    # Hand-modelled pieces round the player (lobby_blender.py), each turned to the spawn and set well apart, so that
    # the player stands in the open: the terminal the VR menu stands on, front left; the table of things to pick up
    # beside it, left and a little behind; the pod with the waiting body, right, across the open floor from them.
    for name, (bearing, dist) in (TUTORIAL_PROPS if PLACE == 'tutorial' else PROPS).items():
        at = (dist * math.sin(math.radians(bearing)), dist * math.cos(math.radians(bearing)))
        L.props[name] = (at, facing(at))
    if PLACE == 'tutorial':
        return L
    plaza(L)
    near_architecture(L, rng)
    monument_south(L)
    assemblies_near(L, rng)
    west_court(L)
    pod_mass(L)
    parkour(L, rng)
    towers(L, rng)
    floating(L, rng)
    canopy(L, rng)
    rings(L, rng)
    ground_far(L, rng)
    moving(L, rng)
    lights(L)
    return L


def plan(path='build/lobby-reference/plan.png', extent=60.0, size=1400):
    from PIL import Image, ImageDraw
    L = build()
    img = Image.new('RGB', (size, size), (20, 22, 26))
    g = ImageDraw.Draw(img)
    k = size / (2 * extent)

    def px(x, y):
        return (size / 2 + x * k, size / 2 - (y - 6.0) * k)

    colours = {'near': (150, 150, 140), 'towers': (190, 190, 190), 'hanging': (80, 120, 170), 'parkour': (160, 220, 110),
               'moving': (220, 120, 220), 'pod': (230, 150, 70), 'benches': (200, 200, 120)}
    for s in L.solids:
        if s.shape == 'arc':
            p = s.params
            pts = [px(s.origin[0] + rr * math.cos(p['a0'] + (p['a1'] - p['a0']) * t / 8),
                      s.origin[1] + rr * math.sin(p['a0'] + (p['a1'] - p['a0']) * t / 8))
                   for rr in (p['r_in'],) for t in range(9)]
            g.line(pts, fill=(170, 110, 200), width=2)
            continue
        w, d, _ = s.size
        corners = [s.world((sx * w / 2, sy * d / 2, 0)) for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
        col = colours.get(s.group, colours.get(s.band, (120, 120, 120)))
        if s.group.startswith('level') or s.name.startswith('level'):
            col = (110, 125, 140)
        g.polygon([px(c[0], c[1]) for c in corners], outline=col)
    for m in L.motions:
        c = m.solid.origin
        g.ellipse([px(c[0], c[1])[0] - 4, px(c[0], c[1])[1] - 4, px(c[0], c[1])[0] + 4, px(c[0], c[1])[1] + 4],
                  outline=(220, 120, 220))
    for seam in L.seams:
        if seam[0] == 'ring':
            _, c, r, w, a0, a1 = seam
            pts = [px(c[0] + r * math.cos(a0 + (a1 - a0) * t / 64), c[1] + r * math.sin(a0 + (a1 - a0) * t / 64))
                   for t in range(65)]
            g.line(pts, fill=(90, 90, 90))
        elif seam[0] == 'amber' and len(seam) == 5:
            g.line([px(*seam[1]), px(*seam[2])], fill=(255, 160, 60), width=2)
    for name, (at, yaw) in L.props.items():
        p = px(*at)
        g.ellipse([p[0] - 6, p[1] - 6, p[0] + 6, p[1] + 6], fill=(255, 170, 60))
        g.text((p[0] + 8, p[1] - 6), name, fill=(255, 200, 120))
    p = px(0, 0)
    g.polygon([(p[0], p[1] - 10), (p[0] - 6, p[1] + 6), (p[0] + 6, p[1] + 6)], fill=(80, 220, 120))
    from pathlib import Path
    out = Path(__file__).resolve().parents[2] / path
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out)
    counts = {}
    for s in L.solids:
        counts[s.band] = counts.get(s.band, 0) + 1
    print('solids', len(L.solids), counts, 'motions', len(L.motions), 'lights', len(L.lights), '->', out)


if __name__ == '__main__':
    import sys
    if len(sys.argv) > 1 and sys.argv[1] == 'plan':
        plan()
        plan('build/lobby-reference/plan_far.png', 520.0)
