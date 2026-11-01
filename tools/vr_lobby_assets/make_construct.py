"""The construct: the lobby's player as a gold hologram, the body a construct wears while it waits for one.

    py -3.13 -B tools/vr_lobby_assets/make_construct.py             all of it
    py -3.13 -B tools/vr_lobby_assets/make_construct.py material    vrport\\construct\\construct*.mi
    py -3.13 -B tools/vr_lobby_assets/make_construct.py meshes      the patch meshes and the construct's own meshes
    py -3.13 -B tools/vr_lobby_assets/make_construct.py pack        deserialize into the pack, pack the archive

The game gives a character another look through an appearance of the body meshes themselves (the cyberspace
replacers wear `cyberspace` on the man's body meshes). The main menu's puppet is the man's body (the woman's meshes
are covered too), so its meshes get one more appearance, `vrport_construct`, every chunk on the `construct` material: ArchiveXL's resource patch
(mods/archive/cyberpunkvrport.archive.xl, `resource: patch:`) adds it from a patch mesh of our own, one per body mesh
(each lists exactly as many chunks as its body mesh has; the material comes from the patch mesh, ArchiveXL resolves
it there).

Not the arms: a mesh that preloads its materials (the arms_hq meshes) loads every appearance's materials as it
loads, before ArchiveXL has registered a patched-in material, and the construct's would fail (and be logged as an
error every time the game starts). Such a mesh gets the construct's own copy instead -- the game's mesh with the
construct appearance and material only -- and its component is pointed at the copy while the lobby is up.

The material is the game's hologram shader: a body you see through, gold, its outline glowing (fresnel), with the
glitches, flicker and screen-space dots off -- they are made for a flat screen and shimmer differently in each eye.
Tuned live on mannequins and on the arms side by side (construct_lab.py), against the reference's construct
(art/vr_lobby/references/closeup.png): the shader's Scanline texture is its opacity mask, laid over the mesh's UVs
OpaqueScanlineDensity times. It is a soft cut, not a blend: a level below about 150 hides the surface, one above
about 200 shows all of it, the few levels between show part of it. The mask is a grid of our own (GRIDS, written and
imported here): lines at full level over a fill inside that band, so the body reads as a tinted fill with a brighter
wire grid on it. The black Diffuse leaves the colour to the shader's two colours, of which FallofColor (the rim,
boosted by GlowStrength) sets nearly all of what is seen: its green and blue set the hue (a little lower and the
glow goes red, a little higher and it goes yellow). Opacity sets how much of the world shows through.

The pod's shell in the lobby is a figure of the game's own, the realistic man of its shop windows (PROPS), in the
same appearance on its own instance of the material (construct_shell.mi): it stands in its dark niche across the
plaza, seen through the way the reference's figure is -- pale gold, its outline and its wire grid brighter than the
body -- and dimmer than the player's arms, which are seen from a hand's breadth away. The figure preloads its
materials too, so it is the construct's own copy that the scene places (lobby_blender.py's `construct body`).

CyberpunkVRPort_Lobby (modules/construct.lua) puts the puppet's body on it while the lobby is up; the meshes step
writes its list of the body's resources (modules/construct_list.lua).
"""
import base64
import copy
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_layout as LL  # noqa: E402
import make_lobby_scene as S  # noqa: E402
from make_lobby_scene import cname, depot, value  # noqa: E402

ROOT = S.ROOT
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
WORK = ROOT / 'build/lobby-construct/targets'
TEXTURES = ROOT / 'build/lobby-construct/textures'      # the grid masks: png written here, .xbm imported beside
RAW = ROOT / 'mods/archive/source/raw/vrport/construct'
XL = ROOT / 'mods/archive/cyberpunkvrport.archive.xl'
TEMPLATE_MESH = ROOT / 'mods/archive/source/raw/vrport/ui/vr_lobby_plane.mesh.json'
LUA_LIST = ROOT / 'mods/cet/CyberpunkVRPort_Lobby/modules/construct_list.lua'
DEPOT = 'vrport\\construct\\'
APPEARANCE = 'vrport_construct'
MATERIAL = 'construct'

# The main menu's puppet: the player body, the meshes of the components the lobby leaves on (listed live from its
# components), each with the resource its component holds. It is the man's (mods/tweaks/vrport/VRPort.LobbyBody.yaml);
# the woman's meshes stay for a game without that tweak. The woman's torso component holds a morph target over
# t0_000_pwa_base__full.mesh: the mesh is what is patched. The man's holds the mesh itself and stands in the main
# menu's elegant shoes, with no bare feet of its own: the shoes wear the construct too.
BODY = 'base\\characters\\common\\player_base_bodies\\player_female_average\\'
MAN = 'base\\characters\\common\\player_base_bodies\\player_man_average\\'
SHOES = 'base\\characters\\garment\\player_equipment\\feet\\s1_010_shoe__elegant\\s1_010_pma_shoe__elegant.mesh'
WORN = {BODY + 'arms_hq\\a0_000_pwa_base_hq__l.mesh': BODY + 'arms_hq\\a0_000_pwa_base_hq__l.mesh',
        BODY + 'arms_hq\\a0_000_pwa_base_hq__r.mesh': BODY + 'arms_hq\\a0_000_pwa_base_hq__r.mesh',
        BODY + 'fpp\\n0_000_pwa_fpp__neck.mesh': BODY + 'fpp\\n0_000_pwa_fpp__neck.mesh',
        BODY + 'l0_000_pwa_base__cs_flat.mesh': BODY + 'l0_000_pwa_base__cs_flat.mesh',
        BODY + 't0_000_pwa_base__full.mesh': BODY + 't0_000_pwa_base__full.morphtarget',
        MAN + 'arms_hq\\a0_000_pma_base_hq__l.mesh': MAN + 'arms_hq\\a0_000_pma_base_hq__l.mesh',
        MAN + 'arms_hq\\a0_000_pma_base_hq__r.mesh': MAN + 'arms_hq\\a0_000_pma_base_hq__r.mesh',
        MAN + 'fpp\\n0_000_pma_fpp__neck.mesh': MAN + 'fpp\\n0_000_pma_fpp__neck.mesh',
        MAN + 't0_000_pma_base__full.mesh': MAN + 't0_000_pma_base__full.mesh',
        SHOES: SHOES}
# Other game meshes wearing the construct: the figure in the pod.
PROPS = [LL.POD_FIGURE]
TARGETS = list(WORN) + PROPS
# Where each mesh is in the game.
ARCHIVES = {'basegame_4_appearance.archive': list(WORN), LL.POD_FIGURE_ARCHIVE: PROPS}

# The grid masks (512 x 512): the fill's level, the cell and the line in pixels.
GRIDS = {'construct_grid_m': (220, 20, 0),        # the player's body and arms, seen close: an even fill, no grid
         'construct_shell_grid_m': (185, 20, 2)}  # the pod's figure, across the plaza: a glassy body, less glow

GOLD = (255, 205, 80)
RIM = (255, 235, 150)
LOOK = [value('float', 'GlitchChance', 0.0),
        value('float', 'AdditiveAlphaBlend', 1.0),
        value('float', 'OpaqueScanlineDensity', 100.0),
        value('tex', 'Scanline', 'engine\\textures\\editor\\white.xbm'),
        value('tex', 'Normal', 'engine\\textures\\editor\\normal.xbm'),
        value('tex', 'DotsTexture', 'base\\fx\\_textures\\quests\\q105\\fx_pointcloud_dot.xbm'),
        value('float', 'DotsSize', 80.0),
        value('color', 'DotsColor', (0, 0, 0)),         # no point cloud: the grid draws the body
        value('vec4', 'Projector1Position', (0.0, 0.0, -1.0, 0.0)),
        value('color', 'SurfaceColor', GOLD),
        value('color', 'SurfaceShadows', (0, 0, 0)),
        value('color', 'FallofColor', RIM),
        value('tex', 'Diffuse', 'base\\materials\\placeholder\\black.xbm'),
        value('float', 'GradientOffset', -100.0),
        value('float', 'GradientLength', 1.0),
        value('float', 'FresnelStrength', 6.0),
        value('float', 'DotsFresnelStrength', 0.0),
        value('float', 'GlowStrength', 2.4),
        value('float', 'DesaturationStrength', 0.0),
        value('float', 'FlickerThreshold', 0.0),
        value('float', 'FlickerChance', 0.0),
        value('float', 'ArtifactsChance', 0.0),
        value('float', 'LightBleed', 0.2),
        value('float', 'NormalStrength', 0.2),
        value('float', 'ScreenSpaceFlicker', 0.0),
        value('float', 'UseIsobars', 0.0),
        value('float', 'EntropyThreshold', 0.0),
        value('float', 'UseMovingDots', 0.0),
        value('float', 'IsHair', 0.0),
        value('float', 'ScanlineThickness', 0.35),
        value('float', 'Opacity', 0.12),
        value('float', 'GlobalTint', 0.0)]
# The materials, as overrides of LOOK, and which target wears which (the body: construct). The shell: pale gold, the
# reference's figure in its pod (tried side by side: construct_lab.py).
# The body is Half-Life: Alyx's hands (the reference, 2026-10-28; tried side by side on the man's arms, construct_lab.py
# rounds 4-12): a neat amber outline along the silhouette and the fingers, the hand inside it see-through, no grid, no
# halo. FallofColor paints the whole hand (SurfaceColor next to nothing, SurfaceShadows the inside); the shader works in
# linear light, so a written colour shows far redder than it reads. FresnelStrength is how far in from the outline
# FallofColor reaches: low, a rim along every edge; at 10 the whole body's one even tone. GlowStrength above ~0.3 blooms
# the outline out past the hand; LightBleed and NormalStrength blur it. The shader's blend is premultiplied (src + dst *
# (1 - alpha)): blended (AdditiveAlphaBlend 0) the hand can be darker than the hall behind it, added it is only ever a
# pale cream before the hall's haze. GlobalTint clears the inside (1: the outline alone).
# A touch fuller inside than the pick (Opacity 0.85, GlobalTint 0.65). While turning fast every hologram in the lobby
# (the arms and figures standing in the hall alike, on hologram.mt and its one-pass templates) fills red and green on
# one side -- whatever the values (2026-10-28, construct_lab.py rounds 16-21): the shader's, not the material's.
MATERIALS = {'construct': {'Scanline': ('tex', DEPOT + 'construct_grid_m.xbm'),
                           'DotsColor': ('color', (0, 0, 0)), 'DotsFresnelStrength': 0.0, 'AdditiveAlphaBlend': 0.0,
                           'Opacity': 0.85, 'GlowStrength': 0.2, 'FresnelStrength': 1.5, 'GlobalTint': 0.65,
                           'LightBleed': 0.0, 'NormalStrength': 0.0,
                           'SurfaceColor': ('color', (70, 62, 12)), 'FallofColor': ('color', (200, 160, 50))},
             'construct_shell': {'Scanline': ('tex', DEPOT + 'construct_shell_grid_m.xbm'),
                                 'OpaqueScanlineDensity': 2.0, 'Opacity': 0.25, 'GlowStrength': 0.6,
                                 'SurfaceColor': ('color', (255, 215, 165)),
                                 'FallofColor': ('color', (255, 240, 210))}}
SHELL = {LL.POD_FIGURE: 'construct_shell'}
HOLOGRAM = 'base\\fx\\shaders\\hologram.mt'
BASES = {}  # a material on another hologram template than HOLOGRAM


def target_name(path):
    return path.rsplit('\\', 1)[1]


def path_hash(path):
    """A depot path's resource hash (FNV-1a 64 of the lower-case path), the way the game keys its resources."""
    h = 0xcbf29ce484222325
    for b in path.lower().encode():
        h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h


# ---------------------------------------------------------------- material

def look(overrides):
    """LOOK with some of its values replaced (by name; a bare number is a float)."""
    values = copy.deepcopy(LOOK)
    for name, v in overrides.items():
        kind, v = v if isinstance(v, tuple) and isinstance(v[0], str) else ('float', v)
        values[next(k for k, e in enumerate(values) if name in e)] = value(kind, name, v)
    return values


def material_doc(values, base=HOLOGRAM):
    """A material instance of the hologram shader (or another of its templates)."""
    header = json.loads(TEMPLATE_MESH.read_text(encoding='utf-8'))['Header']
    header['ArchiveFileName'] = ''
    return {'Header': header, 'Data': {'Version': 195, 'BuildVersion': 0, 'EmbeddedFiles': [], 'RootChunk': {
        '$type': 'CMaterialInstance', 'audioTag': cname('None'),
        'baseMaterial': {'DepotPath': depot(base), 'Flags': 'Default'},
        'cookingPlatform': 'PLATFORM_PC', 'enableMask': 0, 'metadata': None, 'resourceVersion': 4,
        'values': values}}}


def grid_masks(grids=None, out=None):
    """Grid masks ({name: (fill, cell, line)}, GRIDS by default) as png in `out` (TEXTURES), imported to .xbm beside
    them. A line of 0: the fill alone, no grid."""
    from PIL import Image, ImageDraw
    grids, out = grids or GRIDS, out or TEXTURES
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    for name, (fill, cell, line) in grids.items():
        im = Image.new('L', (512, 512), fill)
        d = ImageDraw.Draw(im)
        for k in range(0, 512, cell) if line > 0 else ():
            d.rectangle((k, 0, k + line - 1, 511), fill=255)
            d.rectangle((0, k, 511, k + line - 1), fill=255)
        im.save(out / (name + '.png'))
    # The import exits non-zero even when every file went in (as the meshes' import does): its count is the answer.
    r = subprocess.run([str(S.CLI), 'import', str(out), '-o', str(out)], capture_output=True, text=True)
    missing = [n for n in grids if not (out / (n + '.xbm')).exists()]
    if missing or 'Imported %d/%d' % (len(grids), len(grids)) not in r.stdout:
        print(r.stdout[-2000:], r.stderr[-1000:])
        raise SystemExit('grid masks not imported: %s' % missing)


def cmd_material():
    """construct.mi and construct_shell.mi: the hologram shader in gold, and their grid masks."""
    RAW.mkdir(parents=True, exist_ok=True)
    for name, overrides in MATERIALS.items():
        doc = material_doc(look(overrides), BASES.get(name, HOLOGRAM))
        (RAW / (name + '.mi.json')).write_text(json.dumps(doc, indent=2), encoding='utf-8')
        print('material', name)
    grid_masks()
    print('grid masks', len(GRIDS))


# ---------------------------------------------------------------- meshes

def game_meshes():
    """Each target's mesh as the game has it, serialized."""
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    for archive, paths in ARCHIVES.items():
        pattern = '^(%s)$' % '|'.join(p.replace('\\', '\\\\').replace('.', '\\.') for p in paths)
        S.run(S.CLI, 'unbundle', GAME / 'archive/pc/content' / archive, '-o', WORK, '-r', pattern)
    S.run(S.CLI, 'convert', 'serialize', WORK, '-o', WORK, '-w', '*.mesh')
    return {t: json.loads((WORK / (target_name(t) + '.json')).read_text(encoding='utf-8')) for t in TARGETS}


def preloads(root):
    """Whether the game loads every appearance's materials with the mesh (PostLoad, for preloaded materials)."""
    return bool(root.get('preloadLocalMaterialInstances') or root.get('preloadExternalMaterials'))


def construct_look(doc, chunks, material):
    """The construct appearance and its one material on a mesh document."""
    doc['Header']['ArchiveFileName'] = ''
    root = doc['Data']['RootChunk']
    root['appearances'] = [{'HandleId': '0', 'Data': {
        '$type': 'meshMeshAppearance', 'chunkMaterials': [cname(MATERIAL)] * chunks, 'name': cname(APPEARANCE),
        'tags': []}}]
    root['materialEntries'] = [{'$type': 'CMeshMaterialEntry', 'index': 0, 'isLocalInstance': 0,
                                'name': cname(MATERIAL)}]
    root['externalMaterials'] = [{'DepotPath': depot(DEPOT + material + '.mi'), 'Flags': 'Default'}]
    root['preloadExternalMaterials'] = []
    root['preloadLocalMaterialInstances'] = []
    root['localMaterialInstances'] = []
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer', 'materials': [], 'rawData': None,
                                   'rawDataHeaders': []}
    root['forceLoadAllAppearances'] = 0
    return doc


def patch_mesh(chunks, material):
    """A patch mesh: the construct appearance and its material, on the frame of a small mesh of ours (its geometry
    is not patched in: the patch takes the appearances only)."""
    doc = construct_look(json.loads(TEMPLATE_MESH.read_text(encoding='utf-8')), chunks, material)
    doc['Data']['RootChunk']['parameters'] = []
    return doc


# Chunks the construct does not show, by the material the game paints them with: the personal link's plug in the
# left palm (a chunk of its own on the arm).
HIDDEN_CHUNKS = {'personal_link'}


def hidden_chunks(root):
    app = root['appearances'][0]
    app = app.get('Data', app)
    return [k for k, m in enumerate(app['chunkMaterials']) if m['$value'] in HIDDEN_CHUNKS]


def hidden_mask(root):
    """A component's chunk mask that leaves out the chunks HIDDEN_CHUNKS paints (None: every chunk shows)."""
    hidden = hidden_chunks(root)
    return 0xFFFFFFFFFFFFFFFF & ~sum(1 << k for k in hidden) if hidden else None


# ---------------------------------------------------------------- the link's socket

# The skin round the personal link's plug is open where the plug sits (two holes in the palm, inside the plug's
# radius), and with the plug left out the construct's outline draws the socket's rim. The skin is closed instead: in
# the palm's own frame, everything within the plug's radius is drawn in to the socket's centre, the band out to
# SOCKET_OUT plug radii stretched over the whole disc, and the disc laid on the palm (a quadric fitted to the skin
# round it), its normals the palm's -- no triangle added, the buffers keep their size.
SOCKET_OUT = 1.6
SOCKET_DEPTH = 0.012  # the palm side only: the back of the hand is further than this below the palm


def vertex_streams(h, buf, ci):
    """A chunk's positions (bind pose, metres) and normals, and where they are in the buffer."""
    import numpy as np
    q, o = h['quantizationScale'], h['quantizationOffset']
    scale, offset = np.array([q['X'], q['Y'], q['Z']]), np.array([o['X'], o['Y'], o['Z']])
    at, n = ci['chunkVertices']['byteOffsets']['Elements'], ci['numVertices']
    stride = (at[1] - at[0]) // n  # stream 0 ends where stream 1 starts
    raw = np.frombuffer(bytes(buf), dtype='<i2', count=stride // 2 * n, offset=at[0]).reshape(n, -1)[:, :3]
    packed = np.frombuffer(bytes(buf), dtype='<u4', count=2 * n, offset=at[2]).reshape(n, 2)[:, 0]
    normals = np.stack([((packed >> (10 * i)) & 0x3FF) / 1023 * 2 - 1 for i in range(3)], 1)
    return raw / 32767 * scale + offset, normals, packed, (at[0], stride, at[2]), (scale, offset)


def close_socket(root):
    """The skin round the hidden chunks closed (see SOCKET_OUT), in the mesh's render buffer."""
    import numpy as np
    blob = root['renderResourceBlob']['Data']
    h, buf = blob['header'], bytearray(base64.b64decode(blob['renderBuffer']['Bytes']))
    infos = h['renderChunkInfos']
    for hidden in hidden_chunks(root):
        plug = vertex_streams(h, buf, infos[hidden])[0]
        centre = plug.mean(0)
        radius = np.linalg.norm(plug - centre, axis=1).max()
        # The skin the plug sits in: the chunk with the most vertices round it.
        near = [(np.sum(np.linalg.norm(vertex_streams(h, buf, ci)[0] - centre, axis=1) < radius), k)
                for k, ci in enumerate(infos) if k != hidden]
        k = max(near)[1]
        pos, nrm, packed, (p_at, stride, n_at), (scale, offset) = vertex_streams(h, buf, infos[k])
        d = np.linalg.norm(pos - centre, axis=1)
        # The palm's frame, from the skin round the socket (outside it, on the plug's side).
        ring = (d > 1.2 * radius) & (d < 2.2 * radius)
        up = nrm[ring].mean(0)
        ring &= nrm @ up > 0.2
        _, _, vt = np.linalg.svd(pos[ring] - pos[ring].mean(0))
        n = vt[2] if vt[2] @ up > 0 else -vt[2]
        t1 = np.cross(n, [0.0, 0.0, 1.0] if abs(n[2]) < 0.9 else [1.0, 0.0, 0.0])
        t1 /= np.linalg.norm(t1)
        t2 = np.cross(n, t1)
        local = np.stack([(pos - centre) @ t1, (pos - centre) @ t2, (pos - centre) @ n], 1)
        x, y, z = local[ring].T
        coef = np.linalg.lstsq(np.stack([x * x, x * y, y * y, x, y, np.ones_like(x)], 1), z, rcond=None)[0]
        rho = np.hypot(local[:, 0], local[:, 1])
        out = SOCKET_OUT * radius
        disc = (rho < out) & (np.abs(local[:, 2]) < SOCKET_DEPTH)
        # Drawn in: radius -> the centre, out -> out.
        scale_in = np.clip((rho - radius) / (out - radius), 0.0, 1.0) * out / np.maximum(rho, 1e-9)
        nx, ny = local[:, 0] * scale_in, local[:, 1] * scale_in
        a, b, c, dx, dy, f = coef
        nz = a * nx * nx + b * nx * ny + c * ny * ny + dx * nx + dy * ny + f
        flat = centre + np.outer(nx, t1) + np.outer(ny, t2) + np.outer(nz, n)
        normal = (np.outer(-(2 * a * nx + b * ny + dx), t1) + np.outer(-(b * nx + 2 * c * ny + dy), t2)
                  + np.outer(np.ones_like(nx), n))
        normal /= np.linalg.norm(normal, axis=1)[:, None]
        # Laid on the palm inside, the skin as it was at the disc's edge.
        w = np.clip((out - rho) / (0.25 * out), 0.0, 1.0)[:, None]
        new_pos = pos * (1 - w) + flat * w
        new_nrm = nrm * (1 - w) + normal * w
        new_nrm /= np.linalg.norm(new_nrm, axis=1)[:, None]
        for v in np.nonzero(disc)[0]:
            q = np.clip(np.round((new_pos[v] - offset) / scale * 32767), -32768, 32767).astype('<i2')
            buf[p_at + stride * v:p_at + stride * v + 6] = q.tobytes()
            u = np.clip(np.round((new_nrm[v] + 1) / 2 * 1023), 0, 1023).astype(np.uint32)
            word = int(u[0]) | int(u[1]) << 10 | int(u[2]) << 20 | (int(packed[v]) & 0xC0000000)
            buf[n_at + 8 * v:n_at + 8 * v + 4] = word.to_bytes(4, 'little')
        print('  socket closed: chunk %d, %d vertices round chunk %d' % (k, int(disc.sum()), hidden))
    blob['renderBuffer']['Bytes'] = base64.b64encode(bytes(buf)).decode('ascii')


def cmd_meshes():
    game = game_meshes()
    RAW.mkdir(parents=True, exist_ok=True)
    for old in RAW.glob('*.mesh.json'):
        old.unlink()
    own, masks = [], {}
    for t, doc in game.items():
        root = doc['Data']['RootChunk']
        chunks = len(root['renderResourceBlob']['Data']['header']['renderChunkInfos'])
        material = SHELL.get(t, MATERIAL)
        if preloads(root):
            masks[t] = hidden_mask(root)  # the game's chunks, before the copy is made of them
            if masks[t] is not None:
                close_socket(root)
            out, kind = construct_look(doc, chunks, material), 'own copy'
            own.append(t)
        else:
            out, kind = patch_mesh(chunks, material), 'patch'
        (RAW / (target_name(t) + '.json')).write_text(json.dumps(out, indent=1), encoding='utf-8')
        print(kind, target_name(t), chunks, 'chunks,', material)
    # The scene places the figure's own copy (lobby_layout.POD_FIGURE_COPY).
    if LL.POD_FIGURE not in own or DEPOT + target_name(LL.POD_FIGURE) != LL.POD_FIGURE_COPY:
        raise SystemExit('the scene places the pod figure as %s, which the construct has no copy as'
                         % LL.POD_FIGURE_COPY)
    check_xl(own)
    write_lua_list(own, masks)


def check_xl(own):
    """The archive's manifest patches each patched mesh with its patch mesh, and nothing with an own copy."""
    xl = XL.read_text(encoding='utf-8')
    missing = [t for t in TARGETS if t not in own and ((DEPOT + target_name(t)) not in xl or t not in xl)]
    stray = [t for t in own if (DEPOT + target_name(t)) in xl]
    if missing or stray:
        raise SystemExit('cyberpunkvrport.archive.xl: patch %s; do not patch %s' % (missing, stray))


def write_lua_list(own, masks):
    """The Lobby mod's list: which components wear the construct, by the resource they hold, and the construct's
    own copies (a component is pointed at the copy, and back at the game's mesh after; with a mask, the chunks of
    the copy shown)."""
    lines = ['-- Written by tools/vr_lobby_assets/make_construct.py: the construct appearance, the resources (path hashes)',
             '-- of the body meshes that carry it -- a component holding one of them wears it -- and the construct\'s own',
             '-- copies of the meshes that cannot carry it (the component is pointed at the copy while it is worn, its',
             '-- chunk mask set to mask: the chunks the construct shows).',
             'return {', '  appearance = "%s",' % APPEARANCE, '  worn = {']
    for mesh, held in WORN.items():
        if mesh in own:
            mask = '' if masks.get(mesh) is None else ', mask = 0x%016XULL' % masks[mesh]
            lines.append('    ["%d"] = { path = [[%s]], own = [[%s]]%s },' % (path_hash(held), held,
                                                                              DEPOT + target_name(mesh), mask))
        else:
            lines.append('    ["%d"] = { path = [[%s]] },' % (path_hash(held), held))
    lines += ['  },', '  own = {']
    lines += ['    ["%d"] = true,' % path_hash(DEPOT + target_name(t)) for t in own if t in WORN]
    lines += ['  },', '}']
    LUA_LIST.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')


# ---------------------------------------------------------------- pack

def cmd_pack():
    target = S.PACK / 'vrport/construct'
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    S.run(S.CLI, 'convert', 'deserialize', RAW, '-o', target)
    for name in GRIDS:
        shutil.copyfile(TEXTURES / (name + '.xbm'), target / (name + '.xbm'))
    print('construct', len(list(target.glob('*'))), 'files')
    S.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['material', 'meshes', 'pack']
    for step in steps:
        {'material': cmd_material, 'meshes': cmd_meshes, 'pack': cmd_pack}[step]()
