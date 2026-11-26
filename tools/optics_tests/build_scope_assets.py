"""Author isolated optical surfaces from each native scope's aperture.

Inputs are WolvenKit JSON exports. The body, skin and original lens stay native;
the runtime enables one of the added surfaces only on the held player weapon.
Low-power appearances cover 1.00..3.99x in 0.01x increments. This avoids mutable
shared materials and changing render constants belonging to either eye.
"""
import argparse
import base64
import copy
import json
import struct
from pathlib import Path

import numpy as np

MAGNIFIERS = ['long_01', 'long_02', 'long_03', 'long_05', 'short_02', 'short_06',
              'sniper_01', 'sniper_02', 'sniper_03']
PROFILE_KEYS = MAGNIFIERS + ['rasetsu','osprey']


def walk(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def cname(value):
    return {'$type': 'CName', '$storage': 'string', '$value': value}


def ref(value):
    return {'DepotPath': {'$type': 'ResourcePath', '$storage': 'string',
                         '$value': value.replace('/', '\\')}, 'Flags': 'Default'}


def path_of(value):
    return value['DepotPath']['$value']


def depot_path(doc):
    path = doc['Header']['ArchiveFileName'].replace('\\', '/')
    if ':/' in path:
        for prefix in ('/base/', '/ep1/'):
            if prefix in path:
                return path[path.index(prefix) + 1:].replace('/', '\\')
        raise ValueError('Missing depot root: ' + path)
    return path.replace('/', '\\')


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def next_handle(doc):
    return max([int(v['HandleId']) for v in walk(doc)
                if 'HandleId' in v and str(v['HandleId']).isdigit()] + [0]) + 1


def isolate_lens(native, chunk_index):
    doc = copy.deepcopy(native)
    root = doc['Data']['RootChunk']
    header = root['renderResourceBlob']['Data']['header']
    header['renderChunkInfos'] = [header['renderChunkInfos'][chunk_index]]
    header['renderChunkInfos'][0]['lodMask'] = 1
    header['topology'] = [header['topology'][chunk_index]]
    header['renderLODs'] = [0]
    root['lodBoneMask'] = [1]
    root['lodLevelInfo'] = [0]
    root['appearances'] = []
    root['materialEntries'] = []
    root['externalMaterials'] = []
    root['preloadExternalMaterials'] = []
    root['localMaterialInstances'] = []
    root['preloadLocalMaterialInstances'] = []
    root['localMaterialBuffer'] = {'$type': 'meshMeshMaterialBuffer',
                                  'materials': [], 'rawData': None, 'rawDataHeaders': []}
    root['forceLoadAllAppearances'] = 0
    return doc


def set_materials(doc, variants):
    root = doc['Data']['RootChunk']
    handle = next_handle(doc)
    for index, (name, material) in enumerate(variants):
        root['appearances'].append({'HandleId': str(handle), 'Data': {
            '$type': 'meshMeshAppearance', 'name': cname(name),
            'chunkMaterials': [cname(name)], 'tags': []}})
        handle += 1
        root['materialEntries'].append({'$type': 'CMeshMaterialEntry', 'index': index,
                                       'isLocalInstance': 1, 'name': cname(name)})
        root['localMaterialBuffer']['materials'].append(material)


def low_material(native, zoom):
    material = copy.deepcopy(native)
    changes = {'DistortionStrenght': zoom, 'SphereMaskDistortionRadius': 0,
               'SphereMaskDistortionHardness': 1, 'AberrationStrenght': 0,
               'EmissiveTint': 0}
    values = material['values']
    values[:] = [v for v in values if not (v.keys() & changes.keys())]
    values.extend({'$type': 'Float', key: value} for key, value in changes.items())
    # These tints were authored for the flat-screen fake lens. In particular
    # long_05's EmissiveTint=3 overwhelms the magnified scene in daylight.
    colors = {'GlassTintMultiplier': 255, 'LensSpecularColor': 255, 'ScopeInside': 0}
    values[:] = [v for v in values if not (v.keys() & colors.keys())]
    values.extend({'$type': 'Color', key: {'$type': 'Color', 'Alpha': 255,
                   'Red': value, 'Green': value, 'Blue': value}} for key, value in colors.items())
    relief = next((v['EyeRelief'] for v in values if 'EyeRelief' in v), None)
    if relief is None:
        relief = {'$type': 'Vector4', 'X': .8, 'Y': .2, 'Z': 6, 'W': 0}
        values.append({'$type': 'Vector4', 'EyeRelief': relief})
    relief['W'] = 713.25
    # The flat-game shader offsets this mask by each fragment's viewing angle.
    # At 5 cm that closes an aligned scope to a pinhole. The physical tube and
    # aperture already occlude off-axis views; retain its authored edge fade.
    relief['Z'] = 0
    return material


def flatten_camera_lens(doc, depth_offset=0.0):
    root = doc['Data']['RootChunk']
    blob = root['renderResourceBlob']['Data']
    header = blob['header']
    chunk = header['renderChunkInfos'][0]
    offsets = chunk['chunkVertices']['byteOffsets']['Elements']
    strides = chunk['chunkVertices']['vertexLayout']['slotStrides']['Elements']
    data = bytearray(base64.b64decode(blob['renderBuffer']['Bytes']))
    scale = np.array([header['quantizationScale'][key] for key in 'XYZ'])
    offset = np.array([header['quantizationOffset'][key] for key in 'XYZ'])
    points = np.array([struct.unpack_from('<3h', data, offsets[0] + i * strides[0])
                       for i in range(chunk['numVertices'])]) * scale / 32767 + offset
    # Native weapon coordinates: +Y is the bore. The dedicated optical plane
    # must not inherit the bulged glass's varying normals.
    points[:, 1] = points[:, 1].mean() + depth_offset
    lo, hi = points.min(0), points.max(0)
    center, extent = (lo + hi) * .5, np.maximum((hi - lo) * .5, 1e-5)
    for i, point in enumerate(points):
        xyz = np.rint((point - center) / extent * 32767).astype(int)
        struct.pack_into('<3h', data, offsets[0] + i * strides[0], *xyz)
        struct.pack_into('<II', data, offsets[2] + i * strides[2], 1609564671, 536346623)
    for i, key in enumerate('XYZ'):
        header['quantizationScale'][key] = float(extent[i])
        header['quantizationOffset'][key] = float(center[i])
        root['boundingBox']['Min'][key] = float(lo[i])
        root['boundingBox']['Max'][key] = float(hi[i])
    blob['renderBuffer']['Bytes'] = base64.b64encode(data).decode()
    return dict(center=center.tolist(), size=(hi - lo).tolist())


def add_components(doc, mesh_path, key, panel_template, camera_only=False, reticles=(), appearances=None, id_base=0):
    # appearances: names of the appearances to author (None = all of them); a weapon whose iconic
    # appearance carries a scope as its own components gets them in that appearance only.
    handles = {v['HandleId']: v['Data'] for v in walk(doc) if 'HandleId' in v and 'Data' in v}
    handle = next_handle(doc)
    names = set()
    for appearance_index, app in enumerate(doc['Data']['RootChunk']['appearances']):
        app = app['Data']
        if appearances is not None and app['name']['$value'] not in appearances:
            continue
        package = app['compiledData']['Data']
        chunks = package['Chunks']
        assert len(chunks) == len(app['components'])
        main = next(c for c in chunks if path_of(c.get('mesh', {'DepotPath': {'$value': ''}})) == mesh_path)
        names.add(main['name']['$value'])
        for suffix in (('surface', 'panel') if camera_only else ('low', 'surface', 'panel')):
            component = copy.deepcopy(panel_template if suffix == 'panel' else main)
            component['name'] = cname('vr_optic_' + suffix)
            component['id'] = str(7952809653862800000 + id_base + PROFILE_KEYS.index(key) * 1000 + appearance_index * 3 + ('low', 'surface', 'panel').index(suffix))
            component['isEnabled'] = 0
            if suffix != 'panel':
                component['mesh'] = ref('base/vrport/optics/' + key + '_' + suffix + '.mesh')
                component['meshAppearance'] = cname('z100' if suffix == 'low' else 'default')
                component['chunkMask'] = '1'
                component['visibilityAnimationParam'] = cname('None')
                component['LODMode'] = 'AlwaysVisible'
                component['forceLODLevel'] = 0
            duplicate = copy.deepcopy(component)
            for field in ('parentTransform', 'skinning', 'meshTargetBinding'):
                if field not in component:
                    continue
                binding = component[field]
                if binding.get('HandleId') == '-1':
                    continue
                data = copy.deepcopy(binding.get('Data') or handles[binding['HandleRefId']])
                component[field] = {'HandleId': str(handle), 'Data': data}
                duplicate[field] = {'HandleRefId': str(handle)}
                handle += 1
            package['CruidDict'][str(len(chunks))] = component['id']
            chunks.append(component)
            app['components'].append(duplicate)
        for layer_index, layer in enumerate(reticles):
            for is_panel in (False, True):
                source = next(c for c in chunks if c['name']['$value'] == layer['originalPanel']) if is_panel else main
                component = copy.deepcopy(source)
                component['name'] = cname(layer['panel'] if is_panel else layer['surface'])
                component['id'] = str(7952809653962800000 + id_base + PROFILE_KEYS.index(key)*10000 + appearance_index*8 + layer_index*2 + int(is_panel))
                component['isEnabled'] = 0
                if is_panel:
                    component['sceneWidgetProperties']['renderingPlane'] = 'RPl_Weapon'
                else:
                    component['mesh'] = ref('base/vrport/optics/'+key+'_reticle_'+layer['side']+'.mesh')
                    component['meshAppearance'] = cname('default')
                    component['chunkMask'] = '1'
                    component['visibilityAnimationParam'] = cname('None')
                    component['LODMode'] = 'AlwaysVisible'
                    component['forceLODLevel'] = 0
                duplicate = copy.deepcopy(component)
                for field in ('parentTransform', 'skinning', 'meshTargetBinding'):
                    if field not in component or component[field].get('HandleId') == '-1': continue
                    binding = component[field]
                    data = copy.deepcopy(binding.get('Data') or handles[binding['HandleRefId']])
                    if is_panel: data['bindName'] = cname(layer['surface'])
                    component[field] = {'HandleId': str(handle), 'Data': data}
                    duplicate[field] = {'HandleRefId': str(handle)}
                    handle += 1
                package['CruidDict'][str(len(chunks))] = component['id']
                chunks.append(component)
                app['components'].append(duplicate)
    return sorted(names)


def planar_low_lens(doc):
    """Give the projective-centre shader one affine plane and full-float UVs.

    A native bulged lens with rounded half-float UVs produces a different
    derivative centre on each triangle as the camera turns. Only our isolated
    optical surface changes; the native housing and indicators stay intact.
    """
    blob=doc['Data']['RootChunk']['renderResourceBlob']['Data'];h=blob['header']
    c=h['renderChunkInfos'][0];vertex=c['chunkVertices'];offsets=vertex['byteOffsets']['Elements']
    strides=vertex['vertexLayout']['slotStrides']['Elements'];data=base64.b64decode(blob['renderBuffer']['Bytes'])
    old_uv=np.array([struct.unpack_from('<2e',data,offsets[1]+i*strides[1]) for i in range(c['numVertices'])])
    original=mesh_points(doc)
    fit=np.linalg.lstsq(np.c_[original[:,[0,2]],np.ones(len(original))],old_uv,rcond=None)[0]
    flip=np.array([fit[0,0]<0,fit[1,1]<0])
    flatten_camera_lens(doc)
    xyz=mesh_points(doc);lo=xyz[:,[0,2]].min(0);span=xyz[:,[0,2]].max(0)-lo
    uv=(xyz[:,[0,2]]-lo)/span;uv[:,flip]=1-uv[:,flip]
    data=base64.b64decode(blob['renderBuffer']['Bytes']);begin=h['indexBufferOffset']
    # Append a float32 UV vertex stream before the index buffer. Existing
    # vertex offsets and relative index offsets remain valid.
    extra=uv.astype('<f4').tobytes();extra+=bytes((-len(extra))%16)
    blob['renderBuffer']['Bytes']=base64.b64encode(data[:begin]+extra+data[begin:]).decode()
    offsets[1]=begin;strides[1]=8;h['indexBufferOffset']+=len(extra)
    element=next(e for e in vertex['vertexLayout']['elements']['Elements'] if e['streamIndex']==1 and e['usage']=='PS_TexCoord' and e['usageIndex']==0)
    element['type']='PT_Float2'


def mesh_points(doc, chunk_index=0):
    blob=doc['Data']['RootChunk']['renderResourceBlob']['Data']; header=blob['header']
    chunk=header['renderChunkInfos'][chunk_index]
    offset=chunk['chunkVertices']['byteOffsets']['Elements'][0]
    stride=chunk['chunkVertices']['vertexLayout']['slotStrides']['Elements'][0]
    data=base64.b64decode(blob['renderBuffer']['Bytes'])
    scale=np.array([header['quantizationScale'][k] for k in 'XYZ'])
    origin=np.array([header['quantizationOffset'][k] for k in 'XYZ'])
    return np.array([struct.unpack_from('<3h',data,offset+i*stride) for i in range(chunk['numVertices'])])*scale/32767+origin


def reticle_layers(app, native_dir, native_lens, index, key, geometry, save, single_indicator=False):
    """Keep each native reticle controller; draw its texture on the exit pupil."""
    definitions={v['HandleId']:v['Data'] for v in walk(app) if 'HandleId' in v and 'Data' in v}
    chunks=app['Data']['RootChunk']['appearances'][0]['Data']['compiledData']['Data']['Chunks']
    layers=[]
    for side in ('front','rear'):
        if single_indicator and side=='front':continue
        endings=('_indicator.mesh',) if single_indicator else ('indicator_'+side+'.mesh','indicators_'+side+'.mesh')
        source=next((c for c in chunks if 'mesh' in c and any(s in path_of(c['mesh'])
                    for s in endings)),None)
        if not source: continue
        source_name=source['name']['$value']
        widget=None
        for c in chunks:
            if c.get('$type')!='WorldWidgetComponent':continue
            binding=c['meshTargetBinding'];binding=binding.get('Data') or definitions[binding['HandleRefId']]
            if binding['bindName']['$value']==source_name:widget=c;break
        if not widget:continue
        source_mesh=read(native_dir/(Path(path_of(source['mesh']).replace('\\','/')).name+'.json'))
        material=copy.deepcopy(next(m for m in walk(source_mesh) if m.get('$type')=='CMaterialInstance'
                                    and path_of(m['baseMaterial']).endswith('parallaxscreen_transparent_ui.mt')))
        points=mesh_points(source_mesh); span=points.max(0)-points.min(0)
        depth=-.001*(len(layers)+1)
        plane_y=geometry['center'][1]+depth
        # Reconstruct the native reticle's angular projection at its original
        # plane even though its visible aperture is now in front of the camera.
        values=material['values']
        replaced={'ImageScale','IntensityPerLayer','AlphaAsOne','EnableRaytracedEmissive'}
        values[:]=[v for v in values if not (v.keys() & replaced)]
        values.extend([{'$type':'Float','ImageScale':-715.25},
                       {'$type':'Float','AlphaAsOne':0},
                       {'$type':'Float','EnableRaytracedEmissive':0},
                       {'$type':'Vector4','IntensityPerLayer':{'$type':'Vector4','X':float(span[0]),'Y':float(span[2]),'Z':float(points[:,1].mean()-plane_y),'W':1 if side=='rear' and not single_indicator else 0}}])
        lens=isolate_lens(native_lens,index);flatten_camera_lens(lens,depth)
        set_materials(lens,[('default',material)])
        save(lens,'base/vrport/optics/'+key+'_reticle_'+side+'.mesh')
        layers.append(dict(side=side,surface='vr_optic_reticle_'+side,
                           panel='vr_optic_reticle_'+side+'_panel',
                           originalPanel=widget['name']['$value'],originalMesh=source_name,
                           projection=[float(span[0]),float(span[2]),
                                       float(points[:,1].mean()-geometry['center'][1]),
                                       next((v['EmissiveEV'] for v in values if 'EmissiveEV' in v),1.0)]))
    return layers


def composite_material(native, layers):
    """Read camera and native UI from the weapon's shared 2x2 ink atlas."""
    material=copy.deepcopy(native)
    changes={'ImageScale','IntensityPerLayer','ScanlinesDensity'}
    material['values'][:]=[v for v in material['values'] if not (v.keys() & changes)]
    material['values'].append({'$type':'Float','ImageScale':-716.25})
    for side,parameter in [('front','IntensityPerLayer'),('rear','ScanlinesDensity')]:
        projection=next((v['projection'] for v in layers if v['side']==side),[0,0,0,0])
        material['values'].append({'$type':'Vector4',parameter:dict({'$type':'Vector4'},**dict(zip('XYZW',projection)))})
    return material


def rasetsu_composite_widget(native, feed):
    """Two non-overlapping atlas regions: angular camera and physical labels."""
    doc = copy.deepcopy(native)
    handles = {v['HandleId']: v['Data'] for v in walk(doc) if 'HandleId' in v and 'Data' in v}
    def resolve(handle):
        return handle.get('Data') or handles[handle['HandleRefId']]
    canvas_template = next(v for v in walk(feed) if v.get('$type') == 'inkCanvasWidget')
    image_template = next(v for v in walk(feed) if v.get('$type') == 'inkImageWidget')
    sequence = next_handle(doc)
    for instance in list(walk(doc)):
        if instance.get('$type') != 'inkWidgetLibraryItemInstance':
            continue
        old_handle = instance['rootWidget']
        old_root = resolve(old_handle)
        wrapper_id, children_id, image_id = [str(sequence+i) for i in range(3)]
        sequence += 3
        wrapper = copy.deepcopy(canvas_template)
        wrapper['name'] = cname('vr_optics_composite')
        wrapper['parentWidget'] = None
        wrapper['propertyManager'] = None
        wrapper['style'] = None
        wrapper['logicController'] = None
        wrapper['secondaryControllers'] = []
        wrapper['size'] = {'$type': 'Vector2', 'X': 1536, 'Y': 768}
        wrapper['fitToContent'] = 0
        old_root['parentWidget'] = {'HandleRefId': wrapper_id}
        transform = old_root['renderTransform']
        transform['scale'] = {'$type': 'Vector2', 'X': 768/old_root['size']['X'], 'Y': 768/old_root['size']['Y']}
        transform['translation'] = {'$type': 'Vector2', 'X': 768, 'Y': 0}
        old_root['renderTransformPivot'] = {'$type': 'Vector2', 'X': 0, 'Y': 0}
        old_root['layout']['anchor'] = 'TopLeft'
        old_root['layout']['anchorPoint'] = {'$type': 'Vector2', 'X': 0, 'Y': 0}
        old_root['layout']['margin'] = {'$type': 'inkMargin', 'left': 0, 'top': 0, 'right': 0, 'bottom': 0}
        old_root['layout']['HAlign'] = 'Left'
        old_root['layout']['VAlign'] = 'Top'
        image = copy.deepcopy(image_template)
        image['parentWidget'] = {'HandleRefId': wrapper_id}
        image['propertyManager'] = None
        image['style'] = None
        image['logicController'] = None
        image['secondaryControllers'] = []
        image['size'] = {'$type': 'Vector2', 'X': 768, 'Y': 768}
        image['fitToContent'] = 0
        image['opacity'] = 1
        image['visible'] = 1
        image['tintColor'] = {'$type': 'HDRColor', 'Red': 1, 'Green': 1, 'Blue': 1, 'Alpha': 1}
        image['layout']['anchor'] = 'TopLeft'
        image['layout']['anchorPoint'] = {'$type': 'Vector2', 'X': 0, 'Y': 0}
        image['layout']['margin'] = {'$type': 'inkMargin', 'left': 0, 'top': 0, 'right': 0, 'bottom': 0}
        image['layout']['HAlign'] = 'Left'
        image['layout']['VAlign'] = 'Top'
        assert not any('HandleId' in v for v in walk(image))
        wrapper['children'] = {'HandleId': children_id, 'Data': {
            '$type': 'inkMultiChildren', 'children': [old_handle, {'HandleId': image_id, 'Data': image}]}}
        instance['rootWidget'] = {'HandleId': wrapper_id, 'Data': wrapper}
    for value in walk(doc):
        if value.get('$type') == 'inkRectangleWidget' and value.get('name', {}).get('$value') == 'bg':
            value['opacity'] = 0
    # Moving the root introduces parent links to the new wrapper before its
    # original definition. Emit each handle at first use for WK's resolver.
    definitions = {v['HandleId']: v['Data'] for v in walk(doc) if 'HandleId' in v and 'Data' in v}
    emitted = set()
    def ordered(value):
        if isinstance(value, dict):
            handle = value.get('HandleRefId')
            if handle is None and 'Data' in value: handle = value.get('HandleId')
            if handle is not None:
                if handle in emitted: return {'HandleRefId': handle}
                assert handle in definitions, handle
                emitted.add(handle)
                return {'HandleId': handle, 'Data': ordered(definitions[handle])}
            return {k: ordered(v) for k, v in value.items()}
        if isinstance(value, list): return [ordered(v) for v in value]
        return value
    return ordered(doc)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--ep1', type=Path)
    parser.add_argument('--osprey', type=Path)
    parser.add_argument('--prototype', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(exist_ok=True, parents=True)
    def save(doc, path):
        doc['Header']['ArchiveFileName'] = path.replace('/', '\\')
        (args.out / (Path(path.replace('\\', '/')).name + '.json')).write_text(
            json.dumps(doc, separators=(',', ':')) + '\n', encoding='utf-8')
    # Keep the camera's pre-tonemap scene in HDR through the ink surface.
    # The eye tonemaps the lens with the world once; an LDR camera image would
    # be tone-mapped twice and lose the scene's highlights before compositing.
    feed = read(args.prototype / 'scope_feed.dtex.json')
    feed['Data']['RootChunk']['dataFormat'] = 'RGBA_Float16'
    save(feed, 'base/vrport/optics/scope_feed.dtex')
    surface = read(args.prototype / 'aperture_gain1.mesh.json')
    ui_material = surface['Data']['RootChunk']['preloadLocalMaterialInstances'][0]['Data']
    prototype = read(args.prototype / 'w_att__scope_sniper_01.app.json')
    panel = next(v for v in walk(prototype) if v.get('$type') == 'WorldWidgetComponent'
                 and v.get('name', {}).get('$value') == 'vr_optic_panel')
    # A camera feed needs more texels than the native label-only world widget.
    panel['screenAreaMultiplier'] = 8
    # These widgets belong to a held weapon, like the game's original sights.
    # Scene-plane widgets also participate in auxiliary-camera visibility/LOD.
    panel['sceneWidgetProperties']['renderingPlane'] = 'RPl_Weapon'
    panel['sceneWidgetProperties']['isAlwaysVisible'] = 0
    profiles = []
    WINDOW_GLASS = {'short_06': ('glass',)}
    for key in MAGNIFIERS:
        stem = 'w_att__scope_' + key
        native = read(args.native / (stem + '_01.mesh.json'))
        root = native['Data']['RootChunk']
        entry = next(e for e in root['materialEntries'] if e['isLocalInstance'] and
                     path_of(root['localMaterialBuffer']['materials'][e['index']]['baseMaterial']).endswith('glass_scope.mt'))
        materials = root['appearances'][0]['Data']['chunkMaterials']
        index = next(i for i, name in enumerate(materials) if name == entry['name'])
        assert root['renderResourceBlob']['Data']['header']['renderChunkInfos'][index]['lodMask'] == 1
        hidden = [index] + [i for i, name in enumerate(materials) if name['$value'] == 'highlight_blocker']
        # A tinted window in front of the lens darkens the magnified image (short_06's 'glass':
        # dark green, opacity 0.61, ~40 % of the light); the lens surface itself is the glass.
        hidden += [i for i, name in enumerate(materials) if name['$value'] in WINDOW_GLASS.get(key, ())]
        lens = isolate_lens(native, index)
        planar_low_lens(lens)
        set_materials(lens, [('z%03d' % z, low_material(root['localMaterialBuffer']['materials'][entry['index']], z / 100))
                             for z in range(100, 400)])
        save(lens, 'base/vrport/optics/' + key + '_low.mesh')
        lens = isolate_lens(native, index)
        geometry = flatten_camera_lens(lens)
        app = read(args.native / (stem + '.app.json'))
        reticles = reticle_layers(app, args.native, native, index, key, geometry, save)
        set_materials(lens, [('default', composite_material(ui_material,reticles))])
        save(lens, 'base/vrport/optics/' + key + '_surface.mesh')
        names = add_components(app, depot_path(native), key, panel, reticles=reticles)
        save(app, depot_path(app))
        body_mask = (1 << len(materials)) - 1 - sum(1 << i for i in hidden)
        profiles.append(dict(key=key, components=names, hideChunks=hidden, bodyMask=body_mask, geometry=geometry, reticles=reticles))
        print(key, names, 'hide', hidden, 'aperture', geometry)
    if args.ep1:
        native = read(args.ep1 / 'w_rifle_sniper__tsunami_rasetsu__base1_03_indicator.mesh.json')
        lens = isolate_lens(native, 0)
        geometry = flatten_camera_lens(lens)
        material = copy.deepcopy(ui_material)
        for value in material['values']:
            if 'ImageScale' in value: value['ImageScale'] = -714.25
            if 'IntensityPerLayer' in value: value['IntensityPerLayer']['Y'] = 5
        set_materials(lens, [('default', material)])
        save(lens, 'base/vrport/optics/rasetsu_surface.mesh')
        widget = rasetsu_composite_widget(read(args.ep1 / 'rasetsu_scope2.inkwidget.json'),
                                         read(args.prototype / 'scope_view.inkwidget.json'))
        save(widget, 'base/vrport/optics/rasetsu_view.inkwidget')
        rasetsu_panel = copy.deepcopy(panel)
        rasetsu_panel['widgetResource'] = ref('base/vrport/optics/rasetsu_view.inkwidget')
        names = set()
        for stem in ('w_rifle_sniper__tsunami_rasetsu', 'w_rifle_sniper__tsunami_rasetsu__base1_01'):
            app = read(args.ep1 / (stem + '.app.json'))
            names.update(add_components(app, depot_path(native), 'rasetsu', rasetsu_panel, camera_only=True))
            save(app, depot_path(app))
        profiles.append(dict(key='rasetsu', components=sorted(names), bodyMask=0,
                             cameraOnly=True, originalPanel='on_weapon_ui3445', frontCamera=True, geometry=geometry))
        print('rasetsu', sorted(names), geometry)
    if args.osprey:
        native=read(args.osprey/'w_rifle_sniper__nokota_osprey__base1_02_indicator.mesh.json')
        lens=isolate_lens(native,0)
        geometry=flatten_camera_lens(lens)
        names=set();reticles=[]
        for stem in ('w_rifle_sniper__nokota_osprey','w_rifle_sniper__nokota_osprey__base1_01'):
            app=read(args.osprey/(stem+'.app.json'))
            reticles=reticle_layers(app,args.osprey,native,0,'osprey',geometry,save,single_indicator=True)
            names.update(add_components(app,depot_path(native),'osprey',panel,camera_only=True,reticles=reticles))
            save(app,depot_path(app))
        set_materials(lens,[('default',composite_material(ui_material,reticles))])
        save(lens,'base/vrport/optics/osprey_surface.mesh')
        profiles.append(dict(key='osprey',components=sorted(names),bodyMask=0,cameraOnly=True,geometry=geometry,
                             reticles=reticles,scopeOffset=geometry['center']))
        print('osprey',sorted(names),geometry)
    args.out.with_suffix('.profiles.json').write_text(json.dumps(profiles, indent=2) + '\n', encoding='utf-8')
    lua = ['-- Generated from native scope apertures by build_scope_assets.py.', 'return {']
    for profile in profiles:
        for name in profile['components']:
            extra = ', cameraOnly=true' if profile.get('cameraOnly') else ''
            if profile.get('originalPanel'):
                extra += ", originalPanel='%s'" % profile['originalPanel']
            if profile.get('frontCamera'): extra += ', frontCamera=true'
            if profile.get('scopeOffset'):
                extra += ', scopeOffset={'+', '.join('%s=%.10g' % (k,v) for k,v in zip('xyz',profile['scopeOffset']))+'}'
            if profile.get('reticles'):
                layers=['{'+', '.join("%s='%s'" % (k,v) for k,v in layer.items() if k!='projection')+'}' for layer in profile['reticles']]
                extra += ', reticles={'+', '.join(layers)+'}'
            lua.append("  ['%s'] = { key='%s', bodyMask=%d%s }," % (name, profile['key'], profile['bodyMask'], extra))
    lua.append('}')
    args.out.with_suffix('.profiles.lua').write_text('\n'.join(lua) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
