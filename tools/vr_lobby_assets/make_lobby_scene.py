"""The VR lobby, from Blender to the game: build/lobby-reference/export (lobby_export.py) -> meshes, materials and
entities in mods/archive/source/raw/vrport/lobby, packed into the pack folder.

    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py            all of it
    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py meshes     WolvenKit import of each chunk's .glb
    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py materials  each chunk's materials
    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py entities   the entities and the Lobby mod's list of them
    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py toys       the port's own textures (the toys on the table)
    py -3.13 -B tools/vr_lobby_assets/make_lobby_scene.py pack       deserialize into the pack, pack the archive

Every chunk is one mesh (one submesh per material) and one entity holding it and its colliders: the near chunk
also holds the lights. The moving blocks share one entity: each is a mesh bound to its own
gameTransformAnimatorComponent, moved and turned by the same tracks the game's VR tutorial moves its blocks with.
The things to pick up (item chunks) are physical bodies: each mesh carries a dynamic rigid body (the port's
basketball mesh is the template: its body is re-shaped to the item -- a ball or the box round it -- with the item's
mass and inertia) and its entity holds it in an entPhysicalMeshComponent; the Lobby mod spawns them apart from the
rest (modules/items.lua), so they can be put back.
Entities are spawned by mods/cet/CyberpunkVRPort_Lobby at the player's position and heading when the lobby comes
up (modules/scene_list.lua, written here: the entities, the items, the screens, buttons and dials) -- the scene's
frame.
No entity holds more than 160 components (the game takes no more than 192 in one).
"""
import copy
import json
import math
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lobby_layout as LL  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
EXPORT = ROOT / 'build/lobby-reference/export'
WORK = ROOT / 'build/lobby-reference/game'
RAW = ROOT / 'mods/archive/source/raw/vrport/lobby'
PACK = ROOT / 'build/one-archive-20261027/pack/cyberpunkvrport'
OUT = ROOT / 'build/one-archive-20261027/out'
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
TEMPLATE_MESH = PACK / 'vrport/ui/vr_lobby_plane.mesh'
TEMPLATE_ENT = ROOT / 'mods/archive/source/raw/vrport/ui/vr_lobby_glass.ent.json'
TEMPLATE_MOVING = ROOT / 'build/vr-lobby-20261027/vrtut_json/q000_vr_moving_block_01.ent.json'
TEMPLATE_ITEM_MESH = PACK / 'vrbasketball/vr_basketball.mesh'
TEMPLATE_ITEM_ENT = PACK / 'vrbasketball/vr_basketball.ent'
CONCRETE_PNG = ROOT / 'build/vr-lobby-20261027/concrete_png2'
SCENE_LIST = ROOT / 'mods/cet/CyberpunkVRPort_Lobby/modules/scene_list.lua'
DEPOT = 'vrport\\lobby\\'
MAX_COMPONENTS = 160

WHITE = 'engine\\textures\\editor\\white.xbm'
METAL_BASE = 'engine\\materials\\metal_base.remt'
# The port's own textures (an `own` material: the toys on the table), imported onto copies of the game's concrete
# textures -- its colour, normal and roughness setups -- by the toys step, packed beside the lobby.
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
TOYS = ROOT / 'build/lobby-toys/xbm'
TOYS_DEPOT = DEPOT + 'toys\\'
TOY_TEMPLATES = {k: 'base\\surfaces\\materials\\concrete\\concrete_new\\concrete_new_clean_01_300_%s.xbm' % t
                 for k, t in (('d', 'd'), ('n', 'n'), ('r', 'r'), ('m', 'r'))}


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode or 'rror' in r.stdout[-600:]:
        print(r.stdout[-3000:], r.stderr[-2000:])
    if r.returncode:
        raise SystemExit('failed: %s' % (args[:3],))
    return r.stdout


def manifest():
    return json.loads((EXPORT / 'manifest.json').read_text(encoding='utf-8'))


# ---------------------------------------------------------------- meshes

def drawn(m):
    """The chunks with geometry of their own (the near chunk may have none: its entity holds the rest)."""
    return [ch for ch in m['chunks'] if ch.get('geometry', True)]


def cmd_meshes():
    """Each chunk's .glb onto a copy of a template .mesh (WolvenKit imports only onto an existing mesh), then
    serialized for the materials step."""
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    for ch in drawn(manifest()):
        name = 'vr_lobby_' + ch['name']
        shutil.copyfile(EXPORT / (ch['name'] + '.glb'), WORK / (name + '.glb'))
        shutil.copyfile(TEMPLATE_ITEM_MESH if ch.get('item') else TEMPLATE_MESH, WORK / (name + '.mesh'))
    count = len(drawn(manifest()))
    # The importer exits non-zero over its garment-support warning even when every file went in: its own count
    # is what says whether the import worked.
    r = subprocess.run([str(CLI), 'import', str(WORK), '-o', str(WORK), '-k'], capture_output=True, text=True)
    if 'Imported %d/%d file(s)' % (count, count) not in r.stdout:
        print(r.stdout[-3000:], r.stderr[-2000:])
        raise SystemExit('mesh import failed')
    run(CLI, 'convert', 'serialize', WORK, '-o', WORK, '-w', '*.mesh')
    print('meshes', count)


# ---------------------------------------------------------------- materials

def depot(path):
    return {'$type': 'ResourcePath', '$storage': 'string', '$value': path}


def cname(v):
    return {'$type': 'CName', '$storage': 'string', '$value': v}


def value(kind, name, v):
    if kind == 'tex':
        return {'$type': 'rRef:ITexture', name: {'DepotPath': depot(v), 'Flags': 'Default'}}
    if kind == 'float':
        return {'$type': 'Float', name: v}
    if kind == 'color':
        return {'$type': 'Color', name: {'$type': 'Color', 'Red': v[0], 'Green': v[1], 'Blue': v[2], 'Alpha': 255}}
    if kind == 'vec4':
        return {'$type': 'Vector4', name: {'$type': 'Vector4', 'X': v[0], 'Y': v[1], 'Z': v[2], 'W': v[3]}}
    raise ValueError(kind)


def texture_depot(png):
    """An extracted texture's PNG -> its depot path (base\\surfaces\\materials\\concrete\\...)."""
    rel = Path(png).relative_to(CONCRETE_PNG).with_suffix('.xbm')
    return 'base\\surfaces\\materials\\concrete\\' + str(rel).replace('/', '\\')


def srgb8(c):
    return tuple(max(0, min(255, round(255 * (max(0.0, x) ** (1 / 2.2))))) for x in c)


def game_material(spec, name=None):
    """metal_base set the way the Blender preview shows it (lobby_blender.py materials), with the material's game
    calibration (lobby_layout.GAME_LOOK: a factor on the tint -- or on the colour of a plain material -- or one per
    channel, and a textured one's roughness range)."""
    cal = LL.GAME_LOOK.get(name, {})
    k = cal.get('tint', 1.0)
    k = k if isinstance(k, (tuple, list)) else (k, k, k)
    if 'textured' in spec:
        t = spec['textured']
        rmin, rmax = cal.get('rough_range', t['rough_range'])
        values = [value('tex', 'BaseColor', texture_depot(t['diffuse'])),
                  value('vec4', 'BaseColorScale', (*(min(1.0, v * f) for v, f in zip(t['tint'], k)), 1.0)),
                  value('tex', 'Metalness', WHITE), value('float', 'MetalnessScale', 0.0),
                  value('tex', 'Roughness', texture_depot(t['rough'])),
                  value('float', 'RoughnessScale', rmax - rmin), value('float', 'RoughnessBias', rmin)]
        if t.get('normal'):
            values += [value('tex', 'Normal', texture_depot(t['normal'])),
                       value('float', 'NormalStrength', t['normal_strength'])]
        return METAL_BASE, values
    if 'own' in spec:
        tex = {k: TOYS_DEPOT + '%s_%s.xbm' % (spec['own']['name'], k) for k in 'dnrm'}
        return METAL_BASE, [value('tex', 'BaseColor', tex['d']), value('vec4', 'BaseColorScale', (1.0, 1.0, 1.0, 1.0)),
                            value('tex', 'Metalness', tex['m']), value('float', 'MetalnessScale', 1.0),
                            value('tex', 'Roughness', tex['r']), value('float', 'RoughnessScale', 1.0),
                            value('float', 'RoughnessBias', 0.0),
                            value('tex', 'Normal', tex['n']), value('float', 'NormalStrength', 1.0)]
    values = [value('tex', 'BaseColor', WHITE),
              value('vec4', 'BaseColorScale', (*(min(1.0, v * f) for v, f in zip(spec['base'], k)), 1.0)),
              value('tex', 'Metalness', WHITE), value('float', 'MetalnessScale', spec['metal']),
              value('tex', 'Roughness', WHITE), value('float', 'RoughnessScale', spec['roughness'])]
    if spec['emission_strength'] > 0.05:
        # Blender's emission strength -> the game's exposure value, the way the preview was tuned to look.
        values += [value('tex', 'Emissive', WHITE), value('color', 'EmissiveColor', srgb8(spec['emission'])),
                   value('float', 'EmissiveEV', round(math.log2(spec['emission_strength']) + 0.4, 2))]
    return METAL_BASE, values


def cmd_materials():
    m = manifest()
    specs = m['materials']
    RAW.mkdir(parents=True, exist_ok=True)
    for f in RAW.glob('vr_lobby_*.mesh.json'):
        f.unlink()
    for ch in drawn(m):
        name = 'vr_lobby_' + ch['name']
        doc = json.loads((WORK / (name + '.mesh.json')).read_text(encoding='utf-8-sig'))
        root = doc['Data']['RootChunk']
        chunks = len(root['renderResourceBlob']['Data']['header']['renderChunkInfos'])
        present = ch['materials']      # the glTF's materials, which gltf.Builder writes sorted by name
        if len(present) != chunks:
            raise SystemExit('%s: %d chunks, expected %d (%s)' % (name, chunks, len(present), present))
        root['materialEntries'] = [{'$type': 'CMeshMaterialEntry', 'index': i, 'isLocalInstance': 1,
                                    'name': cname(mat)} for i, mat in enumerate(present)]
        instances = []
        for i, mat in enumerate(present):
            if mat not in specs:
                raise SystemExit('%s: no material spec for %s' % (name, mat))
            base, values = game_material(specs[mat], mat)
            instances.append({'HandleId': str(100 + i), 'Data': {
                '$type': 'CMaterialInstance', 'audioTag': cname('None'),
                'baseMaterial': {'DepotPath': depot(base), 'Flags': 'Default'},
                'cookingPlatform': 'PLATFORM_None', 'enableMask': 0, 'metadata': None, 'resourceVersion': 4,
                'values': copy.deepcopy(values)}})
        root['preloadLocalMaterialInstances'] = instances
        root['appearances'][0]['Data']['chunkMaterials'] = [cname(mat) for mat in present]
        if ch.get('item'):
            item_body(root, ch['item'])
        (RAW / (name + '.mesh.json')).write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n',
                                                 encoding='utf-8', newline='\n')
    print('materials', len(m['chunks']))


# The game's box collider is never thinner than this: a 16 mm shard boxed at its own size rested 17 mm above the
# table (as a 25 mm half-extent box would).
MIN_HALF = 0.025


def item_body(root, item):
    """The template's rigid body made the item's: a ball, or the box round its geometry, at its centre, with its
    mass and a solid body's inertia. The rest -- World Dynamic filter, damping, solver counts, depenetration limit --
    stays the ball's (tools/make_ball_mesh.py says why each)."""
    body = next(p for p in root['parameters'] if 'physicsData' in p['Data'])['Data']['physicsData']['Data']
    body = body['bodies'][0]['Data']
    params, shape = body['params'], body['collisionShapes'][0]['Data']
    m, c, h = item['mass'], item['centre'], item['half']
    centre = {'$type': 'Vector4', 'X': c[0], 'Y': c[1], 'Z': c[2], 'W': 0}
    identity = {'$type': 'Quaternion', 'i': 0, 'j': 0, 'k': 0, 'r': 1}
    if item['shape'] == 'sphere':
        r = max(h)
        shape['$type'] = 'physicsColliderSphere'
        shape['radius'] = r
        shape.pop('halfExtents', None)
        inertia = (0.4 * m * r * r,) * 3
        shape['material'] = cname('metal.physmat')
    else:
        # Below the least the game makes of a box, the box is that much and grows upwards only: its underside stays
        # the item's, so the item lies on the table, not above it.
        box = [max(v, MIN_HALF) for v in h]
        shape['$type'] = 'physicsColliderBox'
        shape['halfExtents'] = {'$type': 'Vector3', 'X': box[0], 'Y': box[1], 'Z': box[2]}
        centre = {'$type': 'Vector4', 'X': c[0], 'Y': c[1], 'Z': c[2] + box[2] - h[2], 'W': 0}
        shape.pop('radius', None)
        inertia = (m / 3 * (h[1] ** 2 + h[2] ** 2), m / 3 * (h[0] ** 2 + h[2] ** 2), m / 3 * (h[0] ** 2 + h[1] ** 2))
        shape['material'] = cname('plastic.physmat' if m < 0.2 else 'metal.physmat')
    shape['localToBody'] = {'$type': 'Transform', 'orientation': identity, 'position': centre}
    params['mass'] = m
    params['inertia'] = {'$type': 'Vector3', 'X': inertia[0], 'Y': inertia[1], 'Z': inertia[2]}
    # The mass sits where the item's geometry is, whatever the box.
    params['comOffset'] = {'$type': 'Transform', 'orientation': identity,
                           'position': {'$type': 'Vector4', 'X': c[0], 'Y': c[1], 'Z': c[2], 'W': 0}}
    params['linearDamping'] = 0.05
    params['angularDamping'] = 0.05
    # The ball's other looks, its material buffer and its three LODs are not the item's: one look, the materials set
    # above, one LOD (the glTF has one).
    root['appearances'] = root['appearances'][:1]
    root['appearances'][0]['Data']['name'] = cname('default')
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer', 'materials': [], 'rawData': None,
                                   'rawDataHeaders': []}
    root['localMaterialInstances'] = []
    root['externalMaterials'] = []
    root['lodLevelInfo'] = [0]


# ---------------------------------------------------------------- entities

def fixed(v):
    return {'$type': 'FixedPoint', 'Bits': int(round(v * 131072))}


def qmul(a, b):
    ar, ai, aj, ak = a
    br, bi, bj, bk = b
    return (ar * br - ai * bi - aj * bj - ak * bk, ar * bi + ai * br + aj * bk - ak * bj,
            ar * bj - ai * bk + aj * br + ak * bi, ar * bk + ai * bj - aj * bi + ak * br)


def quat(yaw=0.0, pitch=0.0):
    """Turned by yaw about Z, after tilting by pitch about the local X (a ramp's +Y end rising)."""
    q = qmul((math.cos(yaw / 2), 0, 0, math.sin(yaw / 2)), (math.cos(pitch / 2), math.sin(pitch / 2), 0, 0))
    return {'$type': 'Quaternion', 'i': q[1], 'j': q[2], 'k': q[3], 'r': q[0]}


def transform(pos, yaw=0.0):
    return {'$type': 'WorldTransform', 'Orientation': quat(yaw),
            'Position': {'$type': 'WorldPosition', 'x': fixed(pos[0]), 'y': fixed(pos[1]), 'z': fixed(pos[2])}}


def binding(handle, name):
    return {'HandleId': str(handle), 'Data': {
        '$type': 'entHardTransformBinding', 'bindName': cname(name), 'enabled': 1,
        'enableMask': {'$type': 'entTagMask',
                       'excludedTags': {'$type': 'redTagList', 'tags': [cname('NoBinding')]},
                       'hardTags': {'$type': 'redTagList', 'tags': []},
                       'softTags': {'$type': 'redTagList', 'tags': []}},
        'slotName': cname('None')}}


def mesh_component(template, cid, name, path, pos=(0, 0, 0), yaw=0.0, shadows=True, parent=None,
                   appearance='default', scale=1.0):
    c = copy.deepcopy(template)
    c['id'] = str(cid)
    c['name'] = cname(name)
    c['mesh']['DepotPath'] = depot(path)
    c['meshAppearance'] = cname(appearance)
    c['localTransform'] = transform(pos, yaw)
    c['visualScale'] = {'$type': 'Vector3', 'X': scale, 'Y': scale, 'Z': scale}
    c['LODMode'] = 'AlwaysVisible'
    c['castShadows'] = 'Always' if shadows else 'Never'
    c['castLocalShadows'] = 'Always' if shadows else 'Never'
    c['parentTransform'] = parent
    return c


def light_component(cid, name, pos, color, intensity, radius, specular=True, fog=False):
    """A point light; only a light meant to glow in the air is given to the volumetric fog (there a point light is
    a bright ball floating in it)."""
    return {
        '$type': 'entLightComponent', 'allowDistantLight': 0, 'areaRectSideA': 1, 'areaRectSideB': 1,
        'areaShape': 'ALS_Capsule', 'areaTwoSided': 1, 'attenuation': 'LA_InverseSquare', 'autoHideDistance': 80,
        'capsuleLength': 1, 'clampAttenuation': 0,
        'color': {'$type': 'Color', 'Red': color[0], 'Green': color[1], 'Blue': color[2], 'Alpha': 255},
        'colorGroupSaturation': 100, 'contactShadows': 'CSR_None', 'directional': 0, 'enableContactShadows': 0,
        'enableLocalShadows': 0, 'enableLocalShadowsForceStaticsOnly': 0, 'enableShadowAngleControl': 0,
        'envColorGroup': 'ECG_Default', 'EV': 0,
        'flicker': {'$type': 'rendSLightFlickering', 'flickerPeriod': 0.2, 'flickerStrength': 0, 'positionOffset': 0},
        'forceLODLevel': -1, 'group': 'LG_Group0', 'id': str(cid),
        'iesProfile': {'DepotPath': {'$type': 'ResourcePath', '$storage': 'uint64', '$value': '0'}, 'Flags': 'Soft'},
        'initialTransform': None, 'innerAngle': 30, 'intensity': intensity, 'isEnabled': 1, 'isReplicable': 0,
        'lightChannel': 'LC_Channel1, LC_Channel2, LC_Channel3, LC_Channel4, LC_Channel5, LC_Channel6, LC_Channel7, '
                        'LC_Channel8, LC_ChannelWorld, LC_Character, LC_Player, LC_Automated',
        'localTransform': transform(pos), 'name': cname(name), 'noSpecular': 0 if specular else 1, 'outerAngle': 45,
        'parentTransform': None, 'pathTracingLightUsage': 'PTLU_Everywhere', 'pathTracingOverrideScaleGI': 1,
        'placedEditorData': None, 'portalAngleCutoff': 0, 'radius': radius,
        'rayTracedShadowsPlatform': 'RLSP_All', 'rayTracingContactShadowRange': -1, 'rayTracingIntensityScale': 1,
        'rayTracingLightSourceRadius': -1, 'renderSceneLayerMask': 'Default', 'roughnessBias': 0,
        'rtxdiShadowStartingDistance': -1, 'scaleEnvProbes': 1, 'scaleGI': 1, 'scaleVolFog': 1 if fog else 0,
        'sceneDiffuse': 1, 'sceneSpecular': 1 if specular else 0, 'sceneSpecularScale': 100, 'shadowAngle': -1,
        'shadowFadeDistance': 15, 'shadowFadeRange': 5, 'shadowRadius': -1, 'shadowSoftnessMode': 'LSSM_Default',
        'softness': 2, 'sourceRadius': 0.1, 'spotCapsule': 0, 'temperature': -1, 'type': 'LT_Point',
        'unit': 'LU_Lumen', 'useInEnvProbes': 1, 'useInFog': 1 if fog else 0, 'useInGI': 1, 'useInParticles': 1,
        'useInTransparents': 1}


def collider_component(cid, name, boxes, handle):
    """Kinematic boxes the player's controller stops at, the game's 'Device solid visible' filter
    (conference_holo_table.ent's): box = {center, half, yaw, pitch} in the scene's frame."""
    shapes = []
    for i, b in enumerate(boxes):
        c, h = b['center'], b['half']
        shapes.append({'HandleId': str(handle + i), 'Data': {
            '$type': 'physicsColliderBox', 'filterData': None,
            'halfExtents': {'$type': 'Vector3', 'X': h[0], 'Y': h[1], 'Z': h[2]},
            'isImported': 0, 'isObstacle': 0, 'isQueryShapeOnly': 0,
            'localToBody': {'$type': 'Transform', 'orientation': quat(b.get('yaw', 0.0), b.get('pitch', 0.0)),
                            'position': {'$type': 'Vector4', 'X': c[0], 'Y': c[1], 'Z': c[2], 'W': 0}},
            'material': cname('None'), 'materialApperanceOverrides': [], 'tag': cname('None'), 'volumeModifier': 1}})
    return {
        '$type': 'entColliderComponent', 'colliders': shapes,
        'comOffset': {'$type': 'Transform', 'orientation': quat(),
                      'position': {'$type': 'Vector4', 'X': 0, 'Y': 0, 'Z': 0, 'W': 0}},
        'dynamicTrafficSetting': {'$type': 'TrafficGenDynamicTrafficSetting', 'impact': 'Ignored'},
        'filterData': {'HandleId': str(handle + len(boxes)), 'Data': {
            '$type': 'physicsFilterData', 'customFilterData': None, 'preset': cname('Device solid visible'),
            'queryFilter': {'$type': 'physicsQueryFilter', 'mask1': '0', 'mask2': '139837704'},
            'simulationFilter': {'$type': 'physicsSimulationFilter', 'mask1': '114696', 'mask2': '23627'}}},
        'id': str(cid), 'inertia': {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1}, 'isEnabled': 1, 'isReplicable': 0,
        'localTransform': transform((0, 0, 0)), 'mass': 1, 'massOverride': -1, 'name': cname(name),
        'parentTransform': None, 'simulationType': 'Kinematic', 'startInactive': 0, 'useCCD': 0, 'volume': 0}


def fog_component(cid, name, haze):
    """One layer of the haze, a fog volume over the whole place (lobby_layout.GAME_HAZE)."""
    sx, sy, sz = haze['size']
    return {'$type': 'entFogVolumeComponent', 'absorption': haze['absorption'], 'blendFalloff': haze['blend'],
            'color': {'$type': 'Color', 'Red': round(255 * haze['color'][0]), 'Green': round(255 * haze['color'][1]),
                      'Blue': round(255 * haze['color'][2]), 'Alpha': 255},
            'densityFactor': haze['density'], 'densityFalloff': haze['falloff'], 'id': str(cid), 'isEnabled': 1,
            'isReplicable': 0, 'localTransform': transform(haze['centre']), 'name': cname(name),
            'parentTransform': None, 'size': {'$type': 'Vector3', 'X': sx, 'Y': sy, 'Z': sz}}


def lights_of(m, ids):
    """Area lights along the lines near the player become rows of point lights a few metres apart."""
    out = []
    for lt in m['lights']:
        color = srgb8(lt['color'])
        lumens = lt['power'] * 6.0
        if lt['type'] == 'AREA':
            sx, sy = lt['size']
            axis = lt['axis_x'] if sx >= sy else lt['axis_y']
            length = max(sx, sy)
            n = max(1, math.ceil(length / 3.0))
            d = lt['direction']
            for i in range(n):
                t = (i + 0.5) / n - 0.5
                p = [lt['position'][k] + axis[k] * length * t + d[k] * 0.35 for k in range(3)]
                out.append(light_component(next(ids), '%s %d' % (lt['name'], i), p, color, lumens / n,
                                           lt['radius'], specular=False, fog=lt['fog']))
        else:
            out.append(light_component(next(ids), lt['name'], lt['position'], color, lumens, lt['radius'],
                                       specular=True, fog=lt['fog']))
    return out


def move_track(handle, start, target, duration, start_time):
    def evaluator(h, offset):
        return {'HandleId': str(h), 'Data': {'$type': 'gameTransformAnimation_Position_InitialPosition',
                                             'offset': {'$type': 'Vector3', 'X': offset[0], 'Y': offset[1],
                                                        'Z': offset[2]}, 'offsetInWorldSpace': 0}}
    return {'$type': 'gameTransformAnimationTrackItem', 'duration': duration, 'startTime': start_time, 'impl': {
        'HandleId': str(handle), 'Data': {
            '$type': 'gameTransformAnimation_Move',
            'movement': {'HandleId': str(handle + 1), 'Data': {
                '$type': 'gameTransformAnimation_Movement_PredefinedFunction',
                'function': {'$type': 'EasingFunction', 'easingType': 'EET_InOut', 'transitionType': 'EET_Sine'}}},
            'startPositionEvaluator': evaluator(handle + 2, start),
            'targetPositionEvaluator': evaluator(handle + 3, target)}}}


def turn_track(handle, duration, reverse):
    return {'$type': 'gameTransformAnimationTrackItem', 'duration': duration, 'startTime': 0, 'impl': {
        'HandleId': str(handle), 'Data': {
            '$type': 'gameTransformAnimation_RotateOnAxis', 'axis': 'Z',
            'movement': {'HandleId': str(handle + 1), 'Data': {
                '$type': 'gameTransformAnimation_Movement_PredefinedFunction',
                'function': {'$type': 'EasingFunction', 'easingType': 'EET_In', 'transitionType': 'EET_Linear'}}},
            'numberOfFullRotations': 1, 'reverseDirection': 1 if reverse else 0, 'startAngle': 0}}}


def animation(name, items, delay):
    return {'$type': 'gameTransformAnimationDefinition', 'autoStart': 1, 'autoStartDelay': round(delay, 2),
            'looping': 1, 'name': cname(name), 'reverse': 0,
            'timeline': {'$type': 'gameTransformAnimationTimeline', 'items': items}, 'timeScale': 1,
            'timesToPlay': 1}


def cmd_entities():
    m = manifest()
    doc = json.loads(TEMPLATE_ENT.read_text(encoding='utf-8-sig'))
    template = next(c for c in doc['Data']['RootChunk']['components'] if c['$type'] == 'entMeshComponent')
    ids = iter(range(2700000000000010001, 2700000000000020000))
    written = []
    for f in RAW.glob('vr_lobby_*.ent.json'):
        f.unlink()
    items = item_entities(m)
    for ch in m['chunks']:
        if ch['moving'] or ch.get('item'):
            continue
        name = 'vr_lobby_' + ch['name']
        comps = []
        if ch.get('geometry', True):
            comps.append(mesh_component(template, next(ids), ch['name'], DEPOT + name + '.mesh',
                                        shadows=not ch['name'].startswith('far')))
        boxes = [] if '_p' in ch['name'] else m['colliders'].get(ch['name'], [])
        if boxes:
            comps.append(collider_component(next(ids), 'collider', boxes, 900))
        if ch['name'] == 'near':
            # The game's own meshes in the scene: the table's desk (casting shadows), the pod's shell (a hologram,
            # casting none).
            for inst in m.get('instances', []):
                comps.append(mesh_component(template, next(ids), inst['name'], inst['mesh'], inst['position'],
                                            inst['yaw'], shadows=inst.get('shadows', False),
                                            appearance=inst['appearance'], scale=inst['scale']))
            comps += lights_of(m, ids)
            for i, h in enumerate(LL.haze()):
                comps.append(fog_component(next(ids), 'haze %d' % (i + 1), h))
        if len(comps) > MAX_COMPONENTS:
            raise SystemExit('%s: %d components' % (name, len(comps)))
        d = copy.deepcopy(doc)
        d['Data']['RootChunk']['components'] = comps
        (RAW / (name + '.ent.json')).write_text(json.dumps(d, ensure_ascii=False, indent=1) + '\n',
                                                encoding='utf-8', newline='\n')
        written.append(name)
    # The moving blocks: one gameObject (the tutorial's moving block entity), an animator and a mesh per block.
    moving = json.loads(TEMPLATE_MOVING.read_text(encoding='utf-8-sig'))
    root = moving['Data']['RootChunk']
    placeholder = next(c for c in root['components'] if c['$type'] == 'entPlaceholderComponent')
    comps = [copy.deepcopy(placeholder)]
    handle = 1000
    for mv in m['motions']:
        anim_name = 'animator ' + mv['chunk']
        yaw = mv['yaw']
        c, s = math.cos(-yaw), math.sin(-yaw)
        tx, ty, tz = mv['travel']
        local = (c * tx - s * ty, s * tx + c * ty, tz)      # the animator's offsets are in its own frame
        half = mv['period'] / 2
        anims = [animation('move', [move_track(handle, (0, 0, 0), local, half, 0),
                                    move_track(handle + 4, local, (0, 0, 0), half, half)], mv['phase'] * mv['period'])]
        handle += 8
        if mv['spin']:
            anims.append(animation('turn', [turn_track(handle, mv['period'] * 4, mv['spin'] < 0)], 0))
            handle += 2
        comps.append({'$type': 'gameTransformAnimatorComponent', 'animations': anims, 'id': str(next(ids)),
                      'isReplicable': 0, 'localTransform': transform(mv['position'], yaw), 'name': cname(anim_name),
                      'parentTransform': binding(handle, placeholder['name']['$value'])})
        handle += 1
        comps.append(mesh_component(template, next(ids), mv['chunk'], DEPOT + 'vr_lobby_' + mv['chunk'] + '.mesh',
                                    shadows=True, parent=binding(handle, anim_name)))
        handle += 1
    if len(comps) > MAX_COMPONENTS:
        raise SystemExit('moving: %d components' % len(comps))
    root['components'] = comps
    if m['motions']:
        (RAW / 'vr_lobby_moving.ent.json').write_text(json.dumps(moving, ensure_ascii=False, indent=1) + '\n',
                                                      encoding='utf-8', newline='\n')
        written.append('vr_lobby_moving')
    tutorial = LL.PLACE == 'tutorial'
    atmosphere = LL.TUTORIAL_ATMOSPHERE if tutorial else LL.CITY_ATMOSPHERE
    lines = ['-- Written by tools/vr_lobby_assets/make_lobby_scene.py, all in the lobby\'s frame: the entities the lobby',
             '-- spawns; the things to pick up (entity, where it rests, its rotation r i j k, its size for the grip, kg, the',
             '-- shape and half sizes the hand closes on); the',
             '-- screens the game draws on and the buttons it presses (centre, right, up, the side facing out, size); the',
             '-- dials it turns (centre, right, up, the side facing out, the radius of the rim); the exposure the place is',
             '-- shown at, in stops over the game\'s auto exposure, and the clock and weather it is held at; where the',
             '-- player is put when the lobby comes up (Night City coordinates, the heading in degrees; none = where the',
             '-- player stands): the frame (lobby_layout.PLACE).',
             'return {', '  exposure = %.2f,' % (LL.TUTORIAL_EXPOSURE if tutorial else LL.GAME_EXPOSURE),
             '  atmosphere = { hour = %d, minute = %d, weather = "%s" },' % (
                 atmosphere['hour'], atmosphere['minute'], atmosphere['weather'])]
    if tutorial:
        lines.append('  place = { position = %s, yaw = %.1f },' % (lua_vec(LL.TUTORIAL_SPAWN['position']),
                                                                  LL.TUTORIAL_SPAWN['yaw']))
        h = LL.HOLO_MENU
        w = h['window']
        lines.append('  holo = { bearing = %.1f, dist = %.2f, height = %.2f, panel = %s, gap = %.2f,' % (
            h['bearing'], h['dist'], h['height'], lua_vec(h['panel']), h['gap']))
        lines.append('           window = { bearing = %.1f, dist = %.2f, height = %.2f, size = %s } },' % (
            w['bearing'], w['dist'], w['height'], lua_vec(w['size'])))
        lines.append('  figure = { bearing = %.1f, dist = %.2f },' % (LL.FIGURE['bearing'], LL.FIGURE['dist']))
    lines.append('  entities = {')
    lines += ['    "vrport\\\\lobby\\\\%s.ent",' % n for n in written]
    lines += ['  },', '  items = {']
    for it in items:
        held = ''
        if it.get('grasp_half'):
            held = ', grasp_half = %s, grasp_at = %s' % (lua_vec(it['grasp_half']), lua_vec(it['grasp_at']))
        lines.append('    { name = "%s", ent = "vrport\\\\lobby\\\\%s.ent", position = %s, rotation = %s, radius = %.4f,'
                     ' mass = %.3f, shape = "%s", half = %s%s },' % (
                         it['label'], it['entity'], lua_vec(it['position']), lua_vec(it['rotation']), it['radius'],
                         it['mass'], it['shape'], lua_vec(it['half']), held))
    lines += ['  },', '  screens = {']
    for name, s in sorted(m.get('screens', {}).items()):
        size = LL.TERMINAL_SCREEN if name == 'terminal' else None
        lines.append('    %s = { centre = %s, right = %s, up = %s, front = %s, size = %s },' % (
            name, lua_vec(s['centre']), lua_vec(s['right']), lua_vec(s['up']), lua_vec(s['front']), lua_vec(size)))
    lines += ['  },', '  buttons = {']
    for name, s in sorted(m.get('buttons', {}).items()):
        lines.append('    ["%s"] = { centre = %s, right = %s, up = %s, front = %s, size = %s },' % (
            name, lua_vec(s['centre']), lua_vec(s['right']), lua_vec(s['up']), lua_vec(s['front']), lua_vec(s['size'])))
    lines += ['  },', '  dials = {']
    for name, s in sorted(m.get('dials', {}).items()):
        lines.append('    %s = { centre = %s, right = %s, up = %s, front = %s, radius = %.4f },' % (
            name, lua_vec(s['centre']), lua_vec(s['right']), lua_vec(s['up']), lua_vec(s['front']), s['radius']))
    lines += ['  },', '}']
    SCENE_LIST.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print('entities', len(written), 'items', len(items))


def item_entities(m):
    """One entity per thing to pick up: the port's ball entity (its entPhysicalMeshComponent, Dynamic, the mesh's
    own body and filter) holding the item's mesh."""
    tmp = WORK / 'item_template'
    if tmp.exists():
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)
    shutil.copyfile(TEMPLATE_ITEM_ENT, tmp / 'item.ent')
    run(CLI, 'convert', 'serialize', tmp, '-o', tmp, '-w', '*.ent')
    template = json.loads((tmp / 'item.ent.json').read_text(encoding='utf-8-sig'))
    out = []
    for ch in m['chunks']:
        item = ch.get('item')
        if not item:
            continue
        name = 'vr_lobby_' + ch['name']
        doc = copy.deepcopy(template)
        doc['Header']['ArchiveFileName'] = ''
        root = doc['Data']['RootChunk']
        for c in [root['components'][0]] + [c for c in root['compiledData']['Data']['Chunks']
                                            if c.get('$type') == 'entPhysicalMeshComponent']:
            c['mesh']['DepotPath'] = depot(DEPOT + name + '.mesh')
            c['name'] = cname('item')
        # The ball's template carries the dependency lists of the entity it was made from (vending machine meshes,
        # flagged as embedded though nothing is): spawned where those are not loaded -- the main menu's world -- the
        # entity waits for them off the main thread and the engine asserts. The item depends on its own mesh only.
        root['inplaceResources'] = []
        root['resolvedDependencies'] = [{'DepotPath': depot(DEPOT + name + '.mesh'), 'Flags': 'Default'}]
        (RAW / (name + '.ent.json')).write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n',
                                                encoding='utf-8', newline='\n')
        w, x, y, z = item['rotation']
        out.append({'label': item['label'], 'entity': name, 'position': item['position'],
                    'rotation': [w, x, y, z], 'radius': max(item['half']), 'mass': item['mass'],
                    'shape': item.get('grasp') or item['shape'], 'half': item['half'],
                    'grasp_half': item.get('grasp_half'), 'grasp_at': item.get('grasp_at')})
    return out


def lua_vec(v):
    return '{ %s }' % ', '.join('%.5f' % x for x in v)


# ---------------------------------------------------------------- pack

def own_maps():
    """Every `own` material's maps: {xbm name: (png, kind)}."""
    out = {}
    for spec in manifest()['materials'].values():
        if 'own' in spec:
            for k in 'dnrm':
                out['%s_%s' % (spec['own']['name'], k)] = (spec['own'][k], k)
    return out


def cmd_toys():
    """The `own` materials' maps imported onto copies of the game's concrete textures (cp77tools import --keep keeps
    each copy's setup: colour, normal or greyscale)."""
    from PIL import Image
    maps = own_maps()
    for d in ('raw', 'red', 'template'):
        shutil.rmtree(TOYS / d, ignore_errors=True)
        (TOYS / d).mkdir(parents=True)
    run(CLI, 'unbundle', GAME / 'archive/pc/content/basegame_3_nightcity.archive', '-o', TOYS / 'template', '-r',
        'concrete_new_clean_01_300_[dnr]\\.xbm$')
    for name, (png, k) in maps.items():
        img = Image.open(png).convert('RGB')
        if k == 'n':
            # Blender bakes +Y up the image; the game's normal maps have it down the image (the concrete panels' seams
            # are grooves only that way round) on tangent frames like the lobby's.
            r, g, b = img.split()
            img = Image.merge('RGB', (r, g.point(lambda v: 255 - v), b))
        img.save(TOYS / 'raw' / (name + '.png'))
        shutil.copyfile(TOYS / 'template' / TOY_TEMPLATES[k], TOYS / 'red' / (name + '.xbm'))
    r = subprocess.run([str(CLI), 'import', str(TOYS / 'raw'), '-k', '-o', str(TOYS / 'red')], capture_output=True,
                       text=True)
    if 'Imported %d/%d' % (len(maps), len(maps)) not in r.stdout:
        print(r.stdout[-2000:], r.stderr[-1000:])
        raise SystemExit('the toys\' textures were not imported')
    print('toys', len(maps), 'textures')


def cmd_pack():
    target = PACK / 'vrport/lobby'
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    run(CLI, 'convert', 'deserialize', RAW, '-o', target)
    maps = own_maps()
    if maps:
        (target / 'toys').mkdir()
        for name in maps:
            src = TOYS / 'red' / (name + '.xbm')
            if not src.exists():
                raise SystemExit('%s missing: run the toys step' % src)
            shutil.copyfile(src, target / 'toys' / src.name)
    print('lobby', len(list(target.rglob('*.*'))), 'files')
    pack_archive()


def pack_sources(rels, stage):
    """Sources under mods/archive/source/raw, each named by its depot path (`rel`, without .json), deserialized into
    the pack through `stage` (emptied first). The source is the file every tool edits; the pack's copy is only ever
    built from it, so no edit lives in a cooked file another tool rebuilds."""
    sources = ROOT / 'mods/archive/source/raw'
    if len({Path(rel).name for rel in rels}) != len(rels):
        raise SystemExit('two sources with one file name: the converter writes them all into one folder')
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    for rel in rels:
        shutil.copyfile(sources / (str(rel) + '.json'), stage / (Path(rel).name + '.json'))
    run(CLI, 'convert', 'deserialize', stage, '-o', stage)
    for rel in rels:
        cooked = stage / Path(rel).name      # the converter writes every file into the output folder itself
        if not cooked.exists():
            raise SystemExit('not deserialized: %s' % cooked)
        (PACK / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(cooked, PACK / rel)
        print('pack:', Path(rel).as_posix())


def pack_archive():
    """The whole pack folder -> the port's one archive, in mods/archive."""
    OUT.mkdir(parents=True, exist_ok=True)
    run(CLI, 'pack', PACK, '-o', OUT)
    # A scanner may still hold the archive just written: try again for a few seconds.
    for attempt in range(20):
        try:
            shutil.copyfile(OUT / 'cyberpunkvrport.archive', ROOT / 'mods/archive/cyberpunkvrport.archive')
            break
        except OSError:
            if attempt == 19:
                raise
            time.sleep(0.5)
    print('packed', (OUT / 'cyberpunkvrport.archive').stat().st_size, 'bytes')


if __name__ == '__main__':
    steps = sys.argv[1:] or ['meshes', 'materials', 'entities', 'toys', 'pack']
    for step in steps:
        {'meshes': cmd_meshes, 'materials': cmd_materials, 'entities': cmd_entities, 'toys': cmd_toys,
         'pack': cmd_pack}[step]()
