"""The game's own VR tutorial lobby as the VR lobby's place: the q000 tutorial location's streaming sectors (Night
City's world, under Japantown) streamed into the main menu's world, with the lobby's edits.

    py -3.13 -B tools/vr_lobby_assets/tutorial_lobby.py            all of it
    py -3.13 -B tools/vr_lobby_assets/tutorial_lobby.py sectors    the location's sectors, as the game loads them
    py -3.13 -B tools/vr_lobby_assets/tutorial_lobby.py layout     every placement, kit pieces with world boxes
    py -3.13 -B tools/vr_lobby_assets/tutorial_lobby.py cuts       what the lobby takes out (CUTS) -> cuts.json
    py -3.13 -B tools/vr_lobby_assets/tutorial_lobby.py block      the streaming block + the baked sectors, packed

The location is the prefab loc_q000_tutorial (lobby, the four courses): every sector descriptor of Night City's
block one of whose variants is a node of that prefab is copied into a block of the port's own, and ArchiveXL merges
that block into the main menu's streaming world (cyberpunkvrport.archive.xl, resource patch on
04_main_menu.streamingworld: ArchiveXL appends a patch block to any streaming world it is registered on). The
sectors are the game's, streamed in the menu's world the way they are in Night City's -- their lights, probes, fog
and moving plates with them -- once the player stands near them.

The lobby leaves out of the tutorial's hall what CUTS names (the block columns, the stairs and the course platform
behind them, the back wall with its door). A sector that loses anything is the port's own file
(vrport\\tutorial\\sectors, the block names it): the game's sector with the cut baked into its buffers here -- a
placed mesh scaled to nothing, an entity moved far below, the collision actors inside the pieces moved down there
too -- so nothing patches sectors at run time and Night City's own tutorial keeps all of it. The sectors' 2.5 layout
is read and their buffers rewritten by tools/patchday/sector25.py.

The hall's floor plates close their seams (SEAMS). A plate is made 4 cm short of its nominal length at both ends of
its local X (vr_architecture_floor_w100_l100: X -0.96..-0.04 of a 1 m pitch), so the rows of the tutorial's floor
are 8 cm apart, the void under the floor showing through. Grown twice the size that is 16 cm, a hole along every
row; so each plate whose top is the hall's floor is stretched along its local X to its nominal length, about its
middle -- the rows then meet edge to edge (none overlaps: no z-fighting), the collision under them unchanged.
"""
import copy
import json
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(ROOT / 'tools/patchday'))
import lobby_layout as LL  # noqa: E402
import make_lobby_scene as MS  # noqa: E402
import sector25  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
ARCHIVES = [GAME / 'archive/pc/content/basegame_3_nightcity.archive',   # the later one wins, as in the game
            GAME / 'archive/pc/ep1/ep1_1_nightcity.archive']
WORK = ROOT / 'build/lobby-tutorial'
SECTORS = WORK / 'sectors'
RAW = ROOT / 'mods/archive/source/raw/vrport/tutorial'
PACKED = MS.PACK / 'vrport/tutorial'
XL = ROOT / 'mods/archive/cyberpunkvrport.archive.xl'
NC_BLOCK = 'base\\worlds\\03_night_city\\_compiled\\default\\blocks\\all.streamingblock'
PREFAB = '$/03_night_city/c_westbrook/japan_town/loc_q000_tutorial_prefab3SJMLZI'
KIT = 'base\\environment\\decoration\\unique\\quest\\q000\\q000_vr_tutorial\\'
BLOCK_NAME = 'q000_tutorial.streamingblock'
BLOCK_PATH = 'vrport\\tutorial\\' + BLOCK_NAME
COPY_DEPOT = 'vrport\\tutorial\\sectors\\'
XL_BEGIN = '# --- tutorial lobby edits (written by tools/vr_lobby_assets/tutorial_lobby.py) ---'
XL_END = '# --- end of the tutorial lobby edits ---'

# The tutorial's hall (Night City coordinates): its floor, and what the lobby takes out of it. A box is
# ((x0, y0, z0), (x1, y1, z1)); a kit piece goes when its whole world box is inside, an entity when its position is.
FLOOR = -215.0
HALL = ((-445.0, 398.0, -240.0), (-366.0, 450.0, -150.0))
CUTS = {
    # The block columns down both sides of the hall: a core of cover blocks standing on the floor, tall and narrow,
    # inside the hall, and all that is clad on it -- plates and wires wholly within the core's footprint (and reach
    # round it), climbing as long as they touch, up to where the column runs into the hanging ceiling.
    'columns': {'cores_in': ((-437.5, 411.0, -217.5), (-388.0, 435.0, -150.0)), 'min_height': 5.0, 'max_side': 4.0,
                'reach': 0.8},
    # The stairs at the hall's end and the raised course platform behind them (its window walls, the course
    # selection buttons and stars, the Militech and T-Bug logos over it).
    'stairs_platform': {'pieces_in': ((-405.2, 412.8, -215.6), (-388.0, 432.2, -202.5)),
                        'entities_in': ((-405.2, 412.8, -216.0), (-388.0, 432.2, -202.5))},
    # The back wall with its double door (a thin wall plane), the small vestibule behind it -- its walls and its
    # floor plate -- and the painting on the wall.
    'back_wall': {'pieces_in': ((-435.2, 404.0, -216.0), (-433.8, 441.0, -194.0)),
                  'walls_in': ((-441.0, 419.0, -215.6), (-434.3, 426.0, -209.0)),
                  'floor_in': ((-441.0, 419.0, -215.6), (-434.3, 426.0, -214.5)),
                  'entities_in': ((-436.0, 415.0, -216.0), (-433.8, 430.0, -205.0))},
}
# The seams closed: a hall floor plate's local X (its length, "l" in the mesh name, in cm) stretched to the nominal.
SEAM_NAME = re.compile(r'_floor_w\d+_l(\d+)_')
SEAM_TOP = 0.06   # a plate is the hall's floor when its top is this close to FLOOR

# The hall made bigger: the whole location -- every placement, its collision, its lights' reach, the sectors'
# streaming boxes -- scaled SCALE times about PIVOT, the middle of the hall's floor (its plates run x -434.5..-400,
# y 414..429 once the cuts are out), where the player stands when the lobby comes up (lobby_layout.TUTORIAL_SPAWN).
SCALE = 2.0
PIVOT = LL.TUTORIAL_SPAWN['position']
# The floor's edge, where the hall's floor ends in the void: a fence the player cannot walk through (the Lobby
# mod's own colliders, tools/vr_lobby_assets/lobby_export.py), FENCE_HEIGHT tall in the grown hall.
GRID = 0.25
FENCE_HEIGHT = 3.0
PLACE_JSON = WORK / 'place.json'


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode or 'rror' in r.stdout[-600:]:
        print(r.stdout[-3000:], r.stderr[-2000:])
    if r.returncode:
        raise SystemExit('failed: %s' % (args[:3],))
    return r.stdout


def serialize(src, out):
    out.mkdir(parents=True, exist_ok=True)       # cp77tools writes nowhere that does not exist
    run(MS.CLI, 'convert', 'serialize', src, '-o', out)


# ---------------------------------------------------------------- the location's sectors

def vanilla_block():
    """Night City's block as WolvenKit JSON (unbundled and serialised once into WORK)."""
    js = WORK / 'vanilla_json' / 'all.streamingblock.json'
    if not js.exists():
        run(MS.CLI, 'unbundle', ARCHIVES[0], '-o', WORK / 'vanilla',
            '-r', r'03_night_city._compiled.default.blocks.all\.streamingblock$')
        serialize(WORK / 'vanilla' / NC_BLOCK, js.parent)
    return json.loads(js.read_text(encoding='utf-8-sig'))


def descriptors(doc):
    keep = [d for d in doc['Data']['RootChunk']['descriptors']
            if any(v['nodeRef']['$value'].startswith(PREFAB) for v in d.get('variants') or [])]
    if not keep:
        raise SystemExit('no descriptor carries ' + PREFAB)
    return keep


def depot(d):
    return d['data']['DepotPath']['$value']


def cmd_sectors():
    names = [depot(d).rsplit('\\', 1)[-1] for d in descriptors(vanilla_block())]
    regex = r'03_night_city._compiled.default.(%s)$' % '|'.join(re.escape(n) for n in names)
    if SECTORS.exists():
        shutil.rmtree(SECTORS)
    SECTORS.mkdir(parents=True)
    sources = {}
    for archive in ARCHIVES:
        out = WORK / 'unbundle' / archive.stem
        run(MS.CLI, 'unbundle', archive, '-o', out, '-r', regex)
        for n in names:
            f = out / 'base/worlds/03_night_city/_compiled/default' / n
            if f.exists():
                shutil.copyfile(f, SECTORS / n)
                sources[n] = archive.name
    missing = [n for n in names if n not in sources]
    if missing:
        raise SystemExit('sectors not found: %s' % missing)
    (SECTORS / 'sources.json').write_text(json.dumps(sources, indent=1), encoding='utf-8')
    print('sectors:', len(sources), {a: list(sources.values()).count(a) for a in set(sources.values())})


# ---------------------------------------------------------------- layout

def kit_bounds():
    """{kit mesh depot path: (min, max)} in the mesh's frame, from each mesh's boundingBox."""
    js = WORK / 'kit_json'
    if not js.exists():
        for archive in (GAME / 'archive/pc/content/basegame_3_nightcity.archive',
                        GAME / 'archive/pc/content/basegame_4_gamedata.archive'):
            run(MS.CLI, 'unbundle', archive, '-o', WORK / 'kit', '-r', r'q000_vr_tutorial.[^.]+\.mesh$')
        serialize(WORK / 'kit' / KIT, js)
    out = {}
    for f in js.glob('*.mesh.json'):
        box = json.loads(f.read_text(encoding='utf-8-sig'))['Data']['RootChunk']['boundingBox']
        out[KIT + f.name[:-len('.json')]] = ([box['Min'][a] for a in 'XYZ'], [box['Max'][a] for a in 'XYZ'])
    return out


def rotate(q, v):
    x, y, z, w = q
    tx, ty, tz = 2 * (y * v[2] - z * v[1]), 2 * (z * v[0] - x * v[2]), 2 * (x * v[1] - y * v[0])
    return (v[0] + w * tx + y * tz - z * ty, v[1] + w * ty + z * tx - x * tz, v[2] + w * tz + x * ty - y * tx)


def world_box(local, pos, quat, scale):
    lo, hi = local
    pts = [rotate(quat, (cx * scale[0], cy * scale[1], cz * scale[2]))
           for cx in (lo[0], hi[0]) for cy in (lo[1], hi[1]) for cz in (lo[2], hi[2])]
    return ([min(p[i] for p in pts) + pos[i] for i in range(3)], [max(p[i] for p in pts) + pos[i] for i in range(3)])


def cmd_layout():
    bounds = kit_bounds()
    names = {sector25.fnv1a64(p): p for p in bounds}
    rows, unresolved = [], set()
    for f in sorted(SECTORS.glob('*.streamingsector')):
        for k, r in enumerate(sector25.placements(f)):
            row = {'sector': f.name, 'record': k, 'node': r['node'], 'class': r['class'], 'name': r['name'],
                   'ref': r['ref'], 'pos': r['pos'], 'quat': r['quat'], 'scale': r['scale'], 'instance': r['instance']}
            if 'mesh_hash' in r:
                path = names.get(r['mesh_hash'])
                if path:
                    row['mesh'] = path
                    row['min'], row['max'] = world_box(bounds[path], r['pos'], r['quat'], r['scale'])
                else:
                    unresolved.add(r['mesh_hash'])
            rows.append(row)
    (WORK / 'layout.json').write_text(json.dumps(rows), encoding='utf-8')
    print('layout:', len(rows), 'placements,', sum('mesh' in r for r in rows), 'kit pieces,',
          len(unresolved), 'other meshes (not the kit: never cut)')


# ---------------------------------------------------------------- cuts

def inside(box, lo, hi=None):
    hi = hi or lo
    return all(box[0][i] <= lo[i] and hi[i] <= box[1][i] for i in range(3))


def stacks(pieces):
    """Vertical stacks: pieces whose footprints overlap (40% of the smaller) with touching z ranges."""
    parent = list(range(len(pieces)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def area(p):
        return (p['max'][0] - p['min'][0]) * (p['max'][1] - p['min'][1])

    for i, a in enumerate(pieces):
        for j in range(i + 1, len(pieces)):
            b = pieces[j]
            ox = min(a['max'][0], b['max'][0]) - max(a['min'][0], b['min'][0])
            oy = min(a['max'][1], b['max'][1]) - max(a['min'][1], b['min'][1])
            if ox <= 0 or oy <= 0 or ox * oy < 0.4 * min(area(a), area(b)):
                continue
            if max(a['min'][2], b['min'][2]) - min(a['max'][2], b['max'][2]) < 0.3:
                parent[find(i)] = find(j)
    out = {}
    for i, p in enumerate(pieces):
        out.setdefault(find(i), []).append(p)
    return list(out.values())


def select(rows):
    """{cut name: [rows]} -- what each of CUTS takes out."""
    kit = [r for r in rows if 'mesh' in r and inside(HALL, r['min'], r['max'])]
    ents = [r for r in rows if r['class'] == 'worldEntityNode' and inside(HALL, r['pos'])]
    out = {}
    hall_floor = lambda p: 'floor' in p['mesh'] and p['max'][2] <= FLOOR + 0.05      # noqa: E731 -- always stays
    rule = CUTS['columns']
    blocks = [p for p in kit if 'cover_block' in p['mesh'] or 'pillar_base' in p['mesh']]
    out['columns'] = []
    for core in stacks(blocks):
        lo = [min(p['min'][k] for p in core) for k in range(3)]
        hi = [max(p['max'][k] for p in core) for k in range(3)]
        centre = [(lo[k] + hi[k]) / 2 for k in range(3)]
        if not (lo[2] <= FLOOR + 0.6 and hi[2] - lo[2] >= rule['min_height'] and
                max(hi[0] - lo[0], hi[1] - lo[1]) <= rule['max_side'] and
                inside(rule['cores_in'], centre[:2] + [lo[2]])):
            continue
        r = rule['reach']
        foot = ([lo[0] - r, lo[1] - r], [hi[0] + r, hi[1] + r])
        within = [p for p in kit if all(foot[0][i] <= p['min'][i] and p['max'][i] <= foot[1][i] for i in range(2))
                  and p['max'][2] >= lo[2] - 0.3 and not hall_floor(p)]
        taken, top = [], lo[2]
        for p in sorted(within, key=lambda p: p['min'][2]):      # climb: each piece must touch what is below
            if p['min'][2] <= top + 0.3:
                taken.append(p)
                top = max(top, p['max'][2])
        # The wire panels hung on a column are wider than it: their middle on the column is enough.
        taken += [p for p in kit if 'wires' in p['mesh'] and p not in taken and p['min'][2] <= top
                  and all(foot[0][i] <= (p['min'][i] + p['max'][i]) / 2 <= foot[1][i] for i in range(2))]
        out['columns'] += taken
    rule = CUTS['stairs_platform']
    out['stairs_platform'] = [p for p in kit if inside(rule['pieces_in'], p['min'], p['max']) and not hall_floor(p)] + \
                             [e for e in ents if inside(rule['entities_in'], e['pos'])]
    rule = CUTS['back_wall']
    out['back_wall'] = [p for p in kit if inside(rule['pieces_in'], p['min'], p['max'])] + \
                       [p for p in kit if 'wall' in p['mesh'] and inside(rule['walls_in'], p['min'], p['max'])] + \
                       [p for p in kit if 'floor' in p['mesh'] and inside(rule['floor_in'], p['min'], p['max'])] + \
                       [e for e in ents if inside(rule['entities_in'], e['pos'])]
    return out


def seams(rows, gone):
    """{sector: [{index, k, centre}]}: the hall's floor plates (top at FLOOR, not cut) stretched along their local X
    by k to their nominal length, about their local middle `centre` (see SEAMS above)."""
    bounds = kit_bounds()
    out = {}
    for r in rows:
        m = SEAM_NAME.search(r.get('mesh', ''))
        if (not m or (r['sector'], r['record']) in gone or abs(r['max'][2] - FLOOR) > SEAM_TOP or
                not inside(HALL, r['min'], r['max'])):
            continue
        lo, hi = bounds[r['mesh']]
        length = hi[0] - lo[0]
        k = int(m.group(1)) / 100.0 / length
        if not 1.0 <= k < 1.2:
            raise SystemExit('%s: %.3f m long, nominal %s cm' % (r['mesh'], length, m.group(1)))
        out.setdefault(r['sector'], []).append({'index': r['record'], 'k': k, 'centre': (lo[0] + hi[0]) / 2,
                                                'length': length})
    return out


def cmd_cuts():
    rows = json.loads((WORK / 'layout.json').read_text(encoding='utf-8'))
    chosen = select(rows)
    gone = {}
    for name, picked in chosen.items():
        for r in picked:
            gone[(r['sector'], r['record'])] = r
    boxes = [(r['min'], r['max']) for r in gone.values() if 'min' in r]
    per_sector = {}
    for (sector, record), r in gone.items():
        per_sector.setdefault(sector, {'nodes': [], 'collisions': []})['nodes'].append(
            {'index': record, 'type': r['class']})
    # Collision actors of a piece that goes. An actor is the collision of one kit piece, its shapes' centre the
    # piece's (to the centimetre): the piece nearest that centre is its own, and it goes with that piece. An actor no
    # piece is that near goes when its centre is inside a piece that goes (a hair of slack) -- not by its origin,
    # the piece's pivot: a floor plate's corner, on the edge of whatever stands next to it.
    grid = {}
    for r in rows:
        if 'min' in r:
            c = tuple((r['min'][i] + r['max'][i]) / 2 for i in range(3))
            grid.setdefault(tuple(int(v // 2) for v in c), []).append((c, (r['sector'], r['record']) in gone))

    def own_piece(p):
        near = [(sum((a - b) ** 2 for a, b in zip(c, p)), cut) for k in
                ((int(p[0] // 2) + i, int(p[1] // 2) + j, int(p[2] // 2) + h)
                 for i in (-1, 0, 1) for j in (-1, 0, 1) for h in (-1, 0, 1)) for c, cut in grid.get(k, ())]
        best = min(near, default=None)
        return best[1] if best is not None and best[0] < 0.05 ** 2 else None

    def goes(shapes):
        mid = tuple(sum(p[i] for p in shapes) / len(shapes) for i in range(3))
        own = own_piece(mid)
        if own is not None:
            return own
        return any(all(lo[i] - 0.05 <= p[i] <= hi[i] + 0.05 for i in range(3)) for p in shapes for lo, hi in boxes)
    actor_count = 0
    for f in sorted(SECTORS.glob('*.streamingsector')):
        centres = collision_centres(f)
        if not centres:
            continue
        for r in (r for r in rows if r['sector'] == f.name and r['class'] == 'worldCookedCollisionNode'):
            actors = centres[(r['node'], r['instance'])]
            hit = [a for a, shapes in enumerate(actors) if shapes and goes(shapes)]
            if hit:
                per_sector.setdefault(f.name, {'nodes': [], 'collisions': []})['collisions'].append(
                    {'index': r['record'], 'expectedActors': len(actors), 'actors': hit})
                actor_count += len(hit)
    plates = 0
    for sector, edits in seams(rows, gone).items():
        per_sector.setdefault(sector, {'nodes': [], 'collisions': []})['seams'] = edits
        plates += len(edits)
    counts = {}
    for r in rows:
        counts[r['sector']] = counts.get(r['sector'], 0) + 1
    for sector, cut in per_sector.items():
        cut['expectedNodes'] = counts[sector]
    (WORK / 'cuts.json').write_text(json.dumps({'cuts': {k: sorted('%s:%d' % (r['sector'], r['record']) for r in v)
                                                         for k, v in chosen.items()},
                                                'sectors': per_sector}, indent=1), encoding='utf-8')
    for name, picked in chosen.items():
        print('cut %-16s %3d placements (%d entities)' % (name, len(picked),
                                                           sum(r['class'] == 'worldEntityNode' for r in picked)))
    print('collision actors:', actor_count, 'in', sum(bool(c['collisions']) for c in per_sector.values()), 'sectors;',
          len(per_sector), 'sectors copied; seams closed on', plates, 'floor plates')


# ---------------------------------------------------------------- block + baked sectors

GONE_Z = -2000.0   # where a baked-out entity and collision actor go, out of everyone's reach
FIXED = 131072     # a collision actor's position: int32 fixed point
# What of a light grows with the place: its reach and the distances it is seen from; its intensity grows with the
# square, so that what it lights, twice as far away, is lit as brightly. A probe's and a fog volume's distances.
LIGHT_LENGTHS = ('radius', 'autoHideDistance', 'shadowFadeDistance', 'sourceRadius', 'capsuleLength')
PROBE_LENGTHS = ('streamingDistance', 'streamingHeight', 'blendRange', 'nearClipDistance', 'farClipDistance')


def grow(p):
    """Where the lobby's bigger hall puts a Night City point of the location (SCALE about PIVOT)."""
    return tuple(PIVOT[i] + SCALE * (p[i] - PIVOT[i]) for i in range(3))


def seam_offset(quat, scale, seam):
    """A plate stretched along its local X by seam k about its local middle: where its origin moves (world) and how
    far each end moves along that axis (world vector), for the grown record's quat and scale."""
    shift = scale[0] * seam['centre'] * (1.0 - seam['k'])
    end = scale[0] * seam['length'] * (seam['k'] - 1.0) / 2
    return rotate(quat, (shift, 0.0, 0.0)), rotate(quat, (end, 0.0, 0.0))


def grow_records(records, cut):
    """nodeData in place: every record moved and scaled with the place (its position, its two bounds points @56 and
    @68, its scale, its streaming distance @124), the hall's floor plates stretched over their seams, then the cut
    ones taken out -- a mesh scaled to zero, an entity moved down to GONE_Z as well."""
    for k in range(len(records) // sector25.RECORD):
        at = k * sector25.RECORD
        for o in (0, 56, 68):
            struct.pack_into('<3f', records, at + o, *grow(struct.unpack_from('<3f', records, at + o)))
        struct.pack_into('<3f', records, at + 32, *(v * SCALE for v in struct.unpack_from('<3f', records, at + 32)))
        struct.pack_into('<f', records, at + 124, struct.unpack_from('<f', records, at + 124)[0] * SCALE)
    for seam in cut.get('seams', ()):
        at = seam['index'] * sector25.RECORD
        quat = struct.unpack_from('<4f', records, at + 16)
        scale = list(struct.unpack_from('<3f', records, at + 32))
        move, end = seam_offset(quat, scale, seam)
        pos = struct.unpack_from('<3f', records, at)
        struct.pack_into('<3f', records, at, *(pos[i] + move[i] for i in range(3)))
        scale[0] *= seam['k']
        struct.pack_into('<3f', records, at + 32, *scale)
        lo, hi = struct.unpack_from('<3f', records, at + 56), struct.unpack_from('<3f', records, at + 68)
        struct.pack_into('<3f', records, at + 56, *(min(lo[i], hi[i]) - abs(end[i]) for i in range(3)))
        struct.pack_into('<3f', records, at + 68, *(max(lo[i], hi[i]) + abs(end[i]) for i in range(3)))
    for n in cut['nodes']:
        at = n['index'] * sector25.RECORD
        struct.pack_into('<3f', records, at + 32, 0.0, 0.0, 0.0)
        if n['type'] == 'worldEntityNode':
            struct.pack_into('<f', records, at + 8, GONE_Z)


def grow_chunks(data, c):
    """The chunks' own lengths, in place: a light's reach and intensity, a probe's and a fog volume's distances, a
    mesh batch's distances (its 33-byte records' f32 @16)."""
    for ci, ch in enumerate(c.chunks):
        cls = c.names[ch[0]]
        if cls == 'worldStaticLightNode':
            for name, at in sector25.float_props(c, ci, LIGHT_LENGTHS + ('intensity',)).items():
                k = SCALE * SCALE if name == 'intensity' else SCALE
                struct.pack_into('<f', data, at, struct.unpack_from('<f', data, at)[0] * k)
        elif cls in ('worldReflectionProbeNode', 'worldStaticFogVolumeNode'):
            for at in sector25.float_props(c, ci, PROBE_LENGTHS).values():
                struct.pack_into('<f', data, at, struct.unpack_from('<f', data, at)[0] * SCALE)
        elif cls == 'worldCookedStaticMeshNode':
            _cls, _f, _p, size, off, _t, _crc = ch
            _props, end = c.props(off, off + size)
            length, at = sector25.vlq(data, end)
            for r in range(length // 33):
                o = at + 33 * r + 16
                struct.pack_into('<f', data, o, struct.unpack_from('<f', data, o)[0] * SCALE)


def grow_collision(cooked, records, cut_actors):
    """A worldCookedCollisionNode's setups with the place grown: every actor moved, every scale entry grown and the
    actors without one given (SCALE, SCALE, SCALE) -- an actor's scale carries its shapes' sizes and offsets -- the
    bounds grown; cut_actors = {setup: [actor]} moved down to GONE_Z. -> (compiledData, tail records)."""
    setups = [sector25.read_setup(cooked, r) for r in records]
    for s, p in enumerate(setups):
        scales = [struct.unpack_from('<3f', p['scales'], 12 * k) for k in range(len(p['scales']) // 12)]
        p['scales'] = bytearray(b''.join(struct.pack('<3f', *(v * SCALE for v in sc)) for sc in scales))
        plain = None
        for a in range(len(p['actors']) // 48):
            rec = 48 * a
            pos = grow([v / FIXED for v in struct.unpack_from('<3i', p['actors'], rec)])
            if a in cut_actors.get(s, ()):
                pos = (pos[0], pos[1], GONE_Z)
            struct.pack_into('<3i', p['actors'], rec, *(round(v * FIXED) for v in pos))
            if struct.unpack_from('<H', p['actors'], rec + 36)[0] == 0xFFFF:
                if plain is None:
                    plain = len(p['scales']) // 12
                    p['scales'] += struct.pack('<3f', SCALE, SCALE, SCALE)
                struct.pack_into('<H', p['actors'], rec + 36, plain)
        p['bounds'] = [v * SCALE for v in p['bounds']]
    return sector25.write_setups(setups)


def bake(data, cut):
    """The sector file `data` for the lobby's hall: the place grown SCALE times about PIVOT (records, chunk lengths,
    collision), and `cut` (cuts.json, one sector; None = nothing cut) taken out of it."""
    cut = cut or {'nodes': [], 'collisions': []}
    data = bytearray(data)
    c = sector25.cr2w_read.Cr2w(bytes(data))
    off, size = c.buffers[0][2], c.buffers[0][3]
    records = bytearray(sector25.kraken(data[off:off + size]))
    count = len(records) // sector25.RECORD
    if cut.get('expectedNodes', count) != count:
        raise SystemExit('nodeData has %d records, the cut was made on %d' % (count, cut['expectedNodes']))
    gone = {}
    for col in cut['collisions']:
        at = col['index'] * sector25.RECORD
        node, slot = struct.unpack_from('<H', records, at + 112)[0], struct.unpack_from('<H', records, at + 128)[0]
        gone.setdefault(node, {})[slot] = (col['actors'], col['expectedActors'])
    grow_records(records, cut)
    grow_chunks(data, c)
    new = {0: (sector25.kraken_pack(records), len(records))}
    for node, (ci, buf, at, n) in sector25.collision_layout(bytes(data)).items():
        b = c.buffers[buf]
        tails = [bytes(data[at + 48 * s:at + 48 * s + 48]) for s in range(n)]
        cut_actors = {}
        for slot, (actors, expected) in gone.get(node, {}).items():
            if struct.unpack_from('<H', tails[slot], 28)[0] != expected:
                raise SystemExit('collision setup %d: the cut was made on %d actors' % (slot, expected))
            cut_actors[slot] = actors
        blob, tails = grow_collision(sector25.kraken(data[b[2]:b[2] + b[3]]), tails, cut_actors)
        data[at:at + 48 * n] = b''.join(tails)
        new[buf] = (sector25.kraken_pack(blob), len(blob))
    return sector25.with_buffers(bytes(data), new)


def collision_centres(path):
    """{(node, setup): [[shape centre, ...] per actor]} of a sector file."""
    data = Path(path).read_bytes()
    c = sector25.cr2w_read.Cr2w(data)
    out = {}
    for node, (ci, buf, at, n) in sector25.collision_layout(data).items():
        b = c.buffers[buf]
        cooked = sector25.kraken(data[b[2]:b[2] + b[3]])
        for s in range(n):
            out[(node, s)] = sector25.shape_centres(sector25.read_setup(cooked, data[at + 48 * s:at + 48 * s + 48]))
    return out


def close(a, b, tol=2e-3):
    return all(abs(x - y) <= tol * max(1.0, abs(x)) for x, y in zip(a, b))


def check_baked(name, cut, path):
    """Read the baked copy back: every record and collision shape where grow() puts it, the cut ones out."""
    cut = cut or {'nodes': [], 'collisions': []}
    before, after = sector25.placements(SECTORS / name), sector25.placements(path)
    nodes = {n['index']: n['type'] for n in cut['nodes']}
    stretched = {s['index']: s for s in cut.get('seams', ())}
    for k, (a, b) in enumerate(zip(before, after)):
        pos, scale = grow(a['pos']), tuple(v * SCALE for v in a['scale'])
        if k in stretched:
            move, _ = seam_offset(a['quat'], scale, stretched[k])
            pos = tuple(pos[i] + move[i] for i in range(3))
            scale = (scale[0] * stretched[k]['k'], scale[1], scale[2])
        if k in nodes:
            scale = (0.0, 0.0, 0.0)
            if nodes[k] == 'worldEntityNode':
                pos = (pos[0], pos[1], GONE_Z)
        if not (close(b['pos'], pos) and close(b['scale'], scale) and b['quat'] == a['quat'] and
                b['class'] == a['class'] and b.get('mesh_hash') == a.get('mesh_hash')):
            raise SystemExit('%s: record %d reads back wrong' % (name, k))
    gone = set()
    for col in cut['collisions']:
        r = before[col['index']]
        gone.update((r['node'], r['instance'], a) for a in col['actors'])
    centres_a, centres_b = collision_centres(SECTORS / name), collision_centres(path)
    for key, actors in centres_a.items():
        for a, shapes in enumerate(actors):
            for p, q in zip(shapes, centres_b[key][a]):
                if key + (a,) in gone:
                    if q[2] > GONE_Z + 100:
                        raise SystemExit('%s: collision %s actor %d not taken out' % (name, key, a))
                elif not close(q, grow(p)):
                    raise SystemExit('%s: collision %s actor %d at %s, expected %s' % (name, key, a, q, grow(p)))


def grow_box(d):
    """A sector descriptor's streaming box, grown with the place."""
    box = d['streamingBox']
    for corner in ('Min', 'Max'):
        for a, v in zip('XYZ', grow([box[corner][a] for a in 'XYZ'])):
            box[corner][a] = v


def drop_manifest_section():
    """Take out the streaming section an earlier version of this tool wrote into the manifest (sector patches)."""
    text = XL.read_text(encoding='utf-8')
    if XL_BEGIN in text:
        head, rest = text.split(XL_BEGIN, 1)
        text = head.rstrip('\n') + '\n' + rest.split(XL_END, 1)[1].lstrip('\n')
        XL.write_text(text, encoding='utf-8', newline='\n')


def cmd_block():
    doc = vanilla_block()
    keep = copy.deepcopy(descriptors(doc))
    cuts = json.loads((WORK / 'cuts.json').read_text(encoding='utf-8'))['sectors']
    if PACKED.exists():
        shutil.rmtree(PACKED)
    (PACKED / 'sectors').mkdir(parents=True)
    for d in keep:
        name = depot(d).rsplit('\\', 1)[-1]
        baked = PACKED / 'sectors' / name
        baked.write_bytes(bake((SECTORS / name).read_bytes(), cuts.get(name)))
        check_baked(name, cuts.get(name), baked)
        d['data']['DepotPath']['$value'] = COPY_DEPOT + name
        grow_box(d)
    out = copy.deepcopy(doc)
    out['Data']['RootChunk']['descriptors'] = keep
    out['Header']['ArchiveFileName'] = BLOCK_PATH
    RAW.mkdir(parents=True, exist_ok=True)
    (RAW / (BLOCK_NAME + '.json')).write_text(json.dumps(out, indent=1), encoding='utf-8')
    run(MS.CLI, 'convert', 'deserialize', RAW, '-o', PACKED)
    if not (PACKED / BLOCK_NAME).exists():
        raise SystemExit('deserialize gave no ' + BLOCK_NAME)
    drop_manifest_section()
    print('block: %d sectors, all the port\'s own, grown x%g about %s; %d of them cut' % (
        len(keep), SCALE, PIVOT, len(cuts)))
    MS.pack_archive()


# ---------------------------------------------------------------- the floor and its fence

def floor_boxes():
    """The grown hall's walkable floor as the collision has it: the (x0, y0, x1, y1) of every box shape of the baked
    sectors whose top is at the floor's height (the cut ones are far below)."""
    out = []
    for f in sorted((PACKED / 'sectors').glob('*.streamingsector')):
        data = f.read_bytes()
        c = sector25.cr2w_read.Cr2w(data)
        for node, (ci, buf, at, n) in sector25.collision_layout(data).items():
            b = c.buffers[buf]
            cooked = sector25.kraken(data[b[2]:b[2] + b[3]])
            for s in range(n):
                parts = sector25.read_setup(cooked, data[at + 48 * s:at + 48 * s + 48])
                for boxes in sector25.shape_boxes(parts):
                    for centre, half in boxes:
                        if half is not None and abs(centre[2] + half[2] - FLOOR) < 0.1 * SCALE:
                            out.append((centre[0] - half[0], centre[1] - half[1], centre[0] + half[0],
                                        centre[1] + half[1]))
    return out


def cmd_place():
    """place.json: the spawn and the fence round the grown hall's floor (Night City coordinates). The floor is the
    floor-high collision the spawn stands on and all that touches it, on a GRID x SCALE grid (a cell is floor when a
    box reaches into it, so the seams between plates are no holes); the fence runs along every edge between that
    floor and what is not, one box per straight run."""
    boxes = floor_boxes()
    cell = GRID * SCALE
    x0 = min(b[0] for b in boxes)
    y0 = min(b[1] for b in boxes)
    nx = int((max(b[2] for b in boxes) - x0) / cell) + 3
    ny = int((max(b[3] for b in boxes) - y0) / cell) + 3
    x0, y0 = x0 - cell, y0 - cell
    solid = set()
    for bx0, by0, bx1, by1 in boxes:
        for i in range(int((bx0 - x0) / cell), int((bx1 - x0) / cell) + 1):
            for j in range(int((by0 - y0) / cell), int((by1 - y0) / cell) + 1):
                cx, cy = x0 + (i + 0.5) * cell, y0 + (j + 0.5) * cell
                if bx0 - cell / 2 < cx < bx1 + cell / 2 and by0 - cell / 2 < cy < by1 + cell / 2:
                    solid.add((i, j))
    start = (int((PIVOT[0] - x0) / cell), int((PIVOT[1] - y0) / cell))
    if start not in solid:
        raise SystemExit('the spawn %s is not on the floor' % (PIVOT,))
    floor, todo = {start}, [start]
    while todo:
        i, j = todo.pop()
        for n in ((i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)):
            if n in solid and n not in floor:
                floor.add(n)
                todo.append(n)
    # Edges between a floor cell and the cell beside it that is not: runs along x (at a y line) and along y.
    runs = []
    for (dx, dy) in ((0, 1), (0, -1), (1, 0), (-1, 0)):
        edges = sorted((i, j) for i, j in floor if (i + dx, j + dy) not in floor)
        if dy:
            lines = {}
            for i, j in edges:
                lines.setdefault(j, []).append(i)
            for j, xs in lines.items():
                y = y0 + (j + (1 if dy > 0 else 0)) * cell
                for a, b in spans(xs):
                    runs.append(((x0 + a * cell + x0 + (b + 1) * cell) / 2, y, (b + 1 - a) * cell / 2, 0.05))
        else:
            lines = {}
            for i, j in edges:
                lines.setdefault(i, []).append(j)
            for i, ys in lines.items():
                x = x0 + (i + (1 if dx > 0 else 0)) * cell
                for a, b in spans(ys):
                    runs.append((x, (y0 + a * cell + y0 + (b + 1) * cell) / 2, 0.05, (b + 1 - a) * cell / 2))
    fence = [[cx, cy, FLOOR + FENCE_HEIGHT / 2, hx, hy, FENCE_HEIGHT / 2] for cx, cy, hx, hy in runs]
    # The floor itself as rectangles (x0, y0, x1, y1): the sheet under it that closes its seams and vents
    # (lobby_blender.py underfloor).
    rects = [[x0 + a * cell, y0 + j0 * cell, x0 + (b + 1) * cell, y0 + (j1 + 1) * cell]
             for (a, b), j0, j1 in floor_rects(floor)]
    PLACE_JSON.write_text(json.dumps({'spawn': LL.TUTORIAL_SPAWN, 'floor_cells': len(floor), 'cell': cell,
                                      'fence': fence, 'floor_rects': rects}, indent=1), encoding='utf-8')
    plot_place(floor, runs, x0, y0, cell)
    area = len(floor) * cell * cell
    print('place: floor %.0f m2 (%d cells of %.2f m), fence %d boxes -> %s' % (area, len(floor), cell, len(fence),
                                                                            PLACE_JSON))


def floor_rects(cells):
    """The cells as rectangles: each row's runs, a run carried on while the rows after it have the same one:
    [((first column, last column), first row, last row), ...]."""
    rows = {}
    for i, j in cells:
        rows.setdefault(j, []).append(i)
    out, open_ = [], {}
    for j in range(min(rows), max(rows) + 2):
        runs = set(spans(rows.get(j, [])))
        for run in [r for r in open_ if r not in runs]:
            out.append((run, open_.pop(run), j - 1))
        for run in runs:
            open_.setdefault(run, j)
    return out


def spans(values):
    """Consecutive runs of the sorted integers: [(first, last), ...]."""
    out = []
    for v in sorted(values):
        if out and v == out[-1][1] + 1:
            out[-1][1] = v
        else:
            out.append([v, v])
    return [tuple(s) for s in out]


def plot_place(floor, runs, x0, y0, cell, px=6):
    """build/lobby-tutorial/place.png: the floor from above (+X right, +Y up), the fence orange, the spawn green."""
    from PIL import Image, ImageDraw
    i0, j0 = min(i for i, _ in floor) - 1, min(j for _, j in floor) - 1
    floor = {(i - i0, j - j0) for i, j in floor}
    x0, y0 = x0 + i0 * cell, y0 + j0 * cell
    w = max(i for i, _ in floor) + 2
    h = max(j for _, j in floor) + 2
    img = Image.new('RGB', (w * px, h * px), (18, 20, 24))
    g = ImageDraw.Draw(img)
    for i, j in floor:
        g.rectangle([i * px, (h - 1 - j) * px, (i + 1) * px - 1, (h - j) * px - 1], fill=(120, 125, 130))
    for cx, cy, hx, hy in runs:
        a = ((cx - hx - x0) / cell * px, (h - (cy + hy - y0) / cell) * px)
        b = ((cx + hx - x0) / cell * px, (h - (cy - hy - y0) / cell) * px)
        g.rectangle([a, b], outline=(255, 150, 40))
    s = ((PIVOT[0] - x0) / cell * px, (h - (PIVOT[1] - y0) / cell) * px)
    g.ellipse([s[0] - 5, s[1] - 5, s[0] + 5, s[1] + 5], fill=(80, 220, 120))
    img.save(WORK / 'place.png')


if __name__ == '__main__':
    steps = sys.argv[1:] or ['sectors', 'layout', 'cuts', 'block', 'place']
    for step in steps:
        {'sectors': cmd_sectors, 'layout': cmd_layout, 'cuts': cmd_cuts, 'block': cmd_block,
         'place': cmd_place}[step]()
