"""The construct's material, tried live: several variants of it side by side in the running game, each on the
game's figure the lobby's pod holds and on the construct's own arms, without a restart (RedHotTools hot-loads the
archive; each variant's mask under its own name, as the game keeps a loaded texture by its path).

    py -3.13 -B tools/vr_lobby_assets/construct_lab.py build     variants -> build/lobby-construct/lab, hot-loaded
    py -3.13 -B tools/vr_lobby_assets/construct_lab.py spawn     the row of figures 3.2 m ahead of the player
    py -3.13 -B tools/vr_lobby_assets/construct_lab.py arms L R  the left arm in variant L, the right in R ('own':
                                                                 the construct's own arms back)
    py -3.13 -B tools/vr_lobby_assets/construct_lab.py clear     despawn, arms back; with the game closed, delete the
                                                                 lab archive

VARIANTS are overrides of make_construct.LOOK, numbered left to right. Nothing here goes into the port's archive:
the lab packs its own (vrport_construct_lab.archive) and `clear` deletes it from the game again.
"""
import json
import shutil
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_construct as C  # noqa: E402
import make_lobby_scene as S  # noqa: E402
from make_lobby_scene import cname, depot  # noqa: E402

ROOT = S.ROOT
LAB = ROOT / 'build/lobby-construct/lab'
RAW = LAB / 'raw'
PACK = LAB / 'pack'
OUT = LAB / 'out'
TEXTURES = LAB / 'textures'          # .xbm files packed with the lab under DEPOT
NAME = 'vrport_construct_lab.archive'
FIGURE_JSON = C.WORK / (C.target_name(C.LL.POD_FIGURE) + '.json')     # make_construct.py meshes writes it
DEPOT = 'vrport\\construct_lab\\'
SPACING = 0.65

# The lab's grid masks ({name: (fill, cell, line)}, as make_construct.GRIDS), written and imported into TEXTURES.
GRIDS = {'s150_m': (150, 20, 2), 's170_m': (170, 20, 2), 's185_m': (185, 20, 2), 's200_m': (200, 20, 2),
         'p220_m': (220, 20, 0)}
# Overrides of make_construct.LOOK (the tuned construct), numbered left to right.
NO_DOTS = {'DotsColor': ('color', (0, 0, 0)), 'DotsFresnelStrength': 0.0}


def grid(name):
    return dict(NO_DOTS, Scanline=('tex', DEPOT + name + '.xbm'))


SHELL = dict(SurfaceColor=('color', (255, 215, 165)), FallofColor=('color', (255, 240, 210)),
             OpaqueScanlineDensity=2.0, Opacity=0.25)
# The player's construct (make_construct.MATERIALS['construct']): its rim, how wide and how bright.
BODY = dict(OpaqueScanlineDensity=10.0, Opacity=0.7, SurfaceColor=('color', (255, 175, 95)))
PALE = ('color', (255, 228, 160))
# Half-Life: Alyx's hands, the reference (2026-10-28): a thin gold outline along the silhouette and the fingers, the
# hand inside it a faint see-through gold, no grid, nothing white-hot.
# The shader takes a colour far redder than it is written: (255, 196, 60) shows red, the gold wants green and blue up.
SURF = ('color', (150, 120, 30))
# Round 4's best (a thin gold outline, a faint gold hand), and the same blended rather than added: added, the hand is
# only ever lighter than what is behind it, and before the hall's bright haze it is a pale cream.
ALYX = dict(grid('p220_m'), Opacity=0.2, SurfaceColor=('color', (140, 110, 25)), FallofColor=('color', (255, 232, 110)),
            GlowStrength=0.40, FresnelStrength=3.0)
# The shader's blend is premultiplied (src + dst * (1 - alpha)): blended (AdditiveAlphaBlend 0) the hand's alpha takes
# light from what is behind it, and a dark fill can sit under a bright outline the way Alyx's hands do.
DARK = dict(ALYX, AdditiveAlphaBlend=0.0)
# GlobalTint 1 leaves the hand clear inside its glowing outline -- Alyx's hand (round 6).
TINT = dict(DARK, Opacity=0.7, SurfaceColor=('color', (60, 45, 8)), GlobalTint=1.0)
# Round 8's pick: a glowing gold outline, a see-through gold inside -- but the outline blooms out past the hand; Alyx's is
# a neat line, no halo.
GOLD = dict(TINT, GlobalTint=0.8, FallofColor=('color', (255, 245, 150)), SurfaceColor=('color', (70, 62, 12)))
# Round 11: FallofColor paints the whole hand (blue: a blue hand, its outline a neat line); SurfaceColor next to
# nothing; SurfaceShadows the inside (red: a red hand). LightBleed and NormalStrength 0 keep the edge a clean line.
EDGE = dict(GOLD, GlowStrength=0.1, FresnelStrength=1.5, LightBleed=0.0, NormalStrength=0.0)
# Round 12's sixth, the pick (make_construct.MATERIALS['construct']): a darker FallofColor makes the outline a neat line
# with no halo, the hand a faint see-through amber inside it.
PICK = dict(EDGE, FallofColor=('color', (200, 160, 50)), GlowStrength=0.2)
# A touch easier to see (2026-10-28, round 14's fifth): fuller inside.
SEEN = dict(PICK, Opacity=0.85, GlobalTint=0.65)
# The inside of the hand glows while turning, the faster the more (2026-10-28): hologram.mt draws the back faces, then
# the front ones twice (one of them unblended, writing depth); hologram_txaa.mt, made for the temporal AA, draws the
# front faces once, blended.
# Round 16: the inside glows while turning where it is already over-bright (layers of the see-through arm adding up past
# white): the temporal AA spreads a little of a pixel far over white a long way. The hand kept under white instead.
# Glow 0.2: 43 clipped hand pixels still, 363 (worst 1376) turning; 0.1: 12 and 209 (2176); 0 or a darker 0.1: none.
# Round 18: while turning the inside of the arms fills solid red and green -- motion vectors in the colour. hologram.mt's
# second pass (renderstage_hologram_depth, unblended) against the one-pass templates.
# Round 20: FresnelStrength 10 is clean at GlobalTint 0.65 (red 4 -> 18) and not at 0.9 (0 -> 8680): GlobalTint, which
# this morning's construct had at 0, is what fills the inside while turning.
VARIANTS = [
    dict(SEEN, GlobalTint=0.0),
    dict(SEEN, GlobalTint=0.0, Opacity=0.5),
    dict(SEEN, GlobalTint=0.3),
    dict(SEEN, GlobalTint=0.65),
    dict(SEEN, GlobalTint=0.0, AdditiveAlphaBlend=1.0, Opacity=0.5),
    dict(SEEN, GlobalTint=0.0, FresnelStrength=3.0),
]
# The lobby's puppet is the man's (mods/tweaks/vrport/VRPort.LobbyBody.yaml).
ARMS = {'l': 'a0_000_pma_base_hq__l.mesh', 'r': 'a0_000_pma_base_hq__r.mesh'}


def mesh_doc(i):
    doc = json.loads(FIGURE_JSON.read_text(encoding='utf-8'))
    doc['Header']['ArchiveFileName'] = ''
    root = doc['Data']['RootChunk']
    chunks = len(root['appearances'][0]['Data']['chunkMaterials'])
    root['appearances'] = [{'HandleId': '0', 'Data': {
        '$type': 'meshMeshAppearance', 'chunkMaterials': [cname('lab')] * chunks, 'name': cname('default'),
        'tags': []}}]
    root['materialEntries'] = [{'$type': 'CMeshMaterialEntry', 'index': 0, 'isLocalInstance': 0, 'name': cname('lab')}]
    root['externalMaterials'] = [{'DepotPath': depot(DEPOT + 'lab_%d.mi' % i), 'Flags': 'Default'}]
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer', 'materials': [], 'rawData': None,
                                   'rawDataHeaders': []}
    root['preloadLocalMaterialInstances'] = []
    root['localMaterialInstances'] = []
    return doc


def arm_doc(side, i):
    """The construct's own arm, in variant i."""
    doc = json.loads((C.RAW / (ARMS[side] + '.json')).read_text(encoding='utf-8'))
    doc['Data']['RootChunk']['externalMaterials'] = [{'DepotPath': depot(DEPOT + 'lab_%d.mi' % i), 'Flags': 'Default'}]
    return doc


def entity_doc():
    doc = json.loads(S.TEMPLATE_ENT.read_text(encoding='utf-8-sig'))
    template = next(c for c in doc['Data']['RootChunk']['components'] if c['$type'] == 'entMeshComponent')
    comps = []
    n = len(VARIANTS)
    for i in range(n):
        x = (i - (n - 1) / 2) * SPACING
        comps.append(S.mesh_component(template, 2700000000000030001 + i, 'lab %d' % i, DEPOT + 'figure_%d.mesh' % i,
                                      (x, 0.0, 0.0), 3.14159265, shadows=False))
    doc['Data']['RootChunk']['components'] = comps
    return doc


def cmd_build():
    for d in (RAW, PACK, OUT):
        if d.exists():
            shutil.rmtree(d)
        d.mkdir(parents=True)
    for i, ov in enumerate(VARIANTS):
        ov = dict(ov)
        base = ov.pop('_base', None)
        doc = C.material_doc(C.look(ov))
        if base is not None:  # another hologram template, the same values
            doc['Data']['RootChunk']['baseMaterial']['DepotPath'] = depot(base)
        (RAW / ('lab_%d.mi.json' % i)).write_text(json.dumps(doc, indent=1), encoding='utf-8')
        (RAW / ('figure_%d.mesh.json' % i)).write_text(json.dumps(mesh_doc(i)), encoding='utf-8')
        for side in ARMS:
            (RAW / ('arm_%s_%d.mesh.json' % (side, i))).write_text(json.dumps(arm_doc(side, i)), encoding='utf-8')
    (RAW / 'lab.ent.json').write_text(json.dumps(entity_doc(), indent=1), encoding='utf-8')
    target = PACK / 'vrport/construct_lab'
    target.mkdir(parents=True)
    S.run(S.CLI, 'convert', 'deserialize', RAW, '-o', target)
    # The lab's own masks, tried on the shader.
    C.grid_masks(GRIDS, TEXTURES)
    for f in TEXTURES.glob('*.xbm'):
        shutil.copyfile(f, target / f.name)
    S.run(S.CLI, 'pack', PACK, '-o', OUT)
    packed = next(OUT.glob('*.archive'))
    hot = C.GAME / 'archive/pc/hot'
    hot.mkdir(exist_ok=True)
    shutil.copyfile(packed, hot / NAME)
    for _ in range(60):
        if not (hot / NAME).exists():
            break
        time.sleep(0.5)
    print('lab', len(VARIANTS), 'variants;', 'hot-loaded' if not (hot / NAME).exists() else 'NOT taken by the game')


SPAWN = '''
local p = Game.GetPlayer()
local pos, f = p:GetWorldPosition(), p:GetWorldForward()
local at = Vector4.new(pos.x + f.x * 3.2, pos.y + f.y * 3.2, pos.z, 1.0)
local tr = WorldTransform.new()
tr:SetPosition(at)
tr:SetOrientation(EulerAngles.new(0, 0, math.deg(math.atan2(-f.x, f.y))):ToQuat())
if CONSTRUCT_LAB then local e = Game.FindEntityByID(CONSTRUCT_LAB); if e then exEntitySpawner.Despawn(e) end end
CONSTRUCT_LAB = exEntitySpawner.Spawn("vrport\\\\construct_lab\\\\lab.ent", tr, "")
print("spawned")
'''

CLEAR = '''
if CONSTRUCT_LAB then local e = Game.FindEntityByID(CONSTRUCT_LAB); if e then exEntitySpawner.Despawn(e) end end
CONSTRUCT_LAB = nil
print("cleared")
'''

# Points the puppet's arm components (they hold the construct's own arms, or a lab arm -- known by the resource's
# hash, the side with it) at the given meshes.
ARMS_LUA = '''
local want = { [ [[%s]] ] = [[%s]], [ [[%s]] ] = [[%s]] }
local sides = { %s }
for _, c in ipairs(Game.GetPlayer():GetComponents()) do
  local h = nil
  pcall(function() h = tostring(c.mesh.hash):gsub("ULL$", "") end)
  local side = h and sides[h]
  if side then
    c.meshAppearance = CName.new("vrport_construct")
    c:ChangeResource(ResRef.FromString(want[side]))
    print(c:GetName().value, want[side])
  end
end
'''


def sides_lua():
    """Which side each arm resource the lab may find on the puppet is, by its path hash."""
    entries = []
    for side, mesh in ARMS.items():
        paths = [C.DEPOT + mesh] + [DEPOT + 'arm_%s_%d.mesh' % (side, i) for i in range(len(VARIANTS))]
        entries += ['["%d"] = "%s"' % (C.path_hash(p), side) for p in paths]
    return ', '.join(entries)


def arm_path(side, which):
    return C.DEPOT + ARMS[side] if which == 'own' else DEPOT + 'arm_%s_%s.mesh' % (side, which)


def lua(src):
    sys.path.insert(0, str(ROOT / 'build/shotgun-reload-20261019/harness'))
    from common import lua as run_lua
    return run_lua(src)


if __name__ == '__main__':
    cmd = sys.argv[1]
    if cmd == 'build':
        cmd_build()
    elif cmd == 'spawn':
        print(lua(SPAWN))
    elif cmd == 'arms':
        print(lua(ARMS_LUA % ('l', arm_path('l', sys.argv[2]), 'r', arm_path('r', sys.argv[3]), sides_lua())))
    elif cmd == 'clear':
        # The running game holds the archive open: the despawn works live, the file goes once the game is closed.
        try:
            print(lua(CLEAR))
            print(lua(ARMS_LUA % ('l', arm_path('l', 'own'), 'r', arm_path('r', 'own'), sides_lua())))
        except Exception as e:
            print('no live game:', e)
        for p in (C.GAME / 'archive/pc/mod' / NAME, C.GAME / 'archive/pc/hot' / NAME):
            if p.exists():
                try:
                    p.unlink()
                    print('removed', p)
                except PermissionError:
                    print('in use by the game, run clear again after closing it:', p)
