"""Author player-only sight surfaces from cooked weapon appearances and their native apertures.

Pass the WolvenKit JSON directories for the weapon appearances, indicator meshes, the
short_02 camera mesh and attachment. Pass --ajax for the quest Ajax's embedded short_02.
Import the emitted files at their depot paths.
The runtime enables the named components only on the equipped weapon in VR.
"""
import argparse
import copy
import json
import subprocess
import sys
from pathlib import Path

import build_scope_assets as assets
from embed_scope_optics import panel_template


# Key, matching appearance file, native aperture mesh, optional open-sight centre.
# The smart weapons' original displays sit on the side or below their open sights.
# Their HUD glass uses the game's weapon-local GetScopeOffset, in metres.
SIGHTS = [
    ('guillotine', 'guillotine', 'w_smg__budget_guillotine__base1_01_indicator_rear', None),
    ('hercules', 'hercules', 'w_special__militech_hercules__base1_02_indicator_rear', None),
    ('pozhar', 'pozhar', 'w_rifle_precision__techtronika_pozhar__base1_01_indicator_front', None),
    ('pozhar_xmod', 'pozhar', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', None),
    ('ticon', 'ticon', 'w_handgun__militech_ticon__base1_01_indicator_crosshair', None),
    ('dante', 'scope_short_05', 'w_att__scope_short_05_01_indicator_rear', None),
    ('shingen', 'shingen', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', (0,.057893,.159352)),
    ('yukimura', 'yukimura', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', (0,0,.07)),
    ('ajax', 'militech_ajax', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', (0,.0678,.06661)),
    ('burya', 'techtronika_burya', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', (0,-.00365,.0724)),
    ('senkoh', 'senkoh', 'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front', (0,-.0592,.05025)),
]
CAMERAS = {'guillotine', 'hercules'}
BARREL_QUAD = 'w_rifle_precision__techtronika_pozhar__base1_01_indicator_front'
BARREL_COMPONENTS = {
    'pozhar_xmod':'w_rifle_precision__techtronika_pozhar__base1_01_cap.mesh',
    'shingen':'w_smg__arasaka_shingen__base1_01.mesh',
    'yukimura':'w_handgun__arasaka_yukimura__base1_01.mesh',
    'ajax':'w_rifle_assault__militech_ajax__base1_01.mesh',
    'burya':'w_revolver__techtronika_burya__base1_01.mesh',
    # The LX scope mesh is absent on the Raiju appearance.
    'senkoh':'w_smg__senkoh_lx__base1_01_indicator_front.mesh',
}


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, separators=(',', ':')), encoding='utf-8')


def surface(key, native, centre, material, mesh_dir):
    # All three guns use the same barrel bind frame. A rigid scene mesh on
    # these weapons does not go through the paired skinned sight VS in VR.
    pane=assets.read(mesh_dir/(BARREL_QUAD+'.mesh.json')) if key in BARREL_COMPONENTS else native
    doc=assets.isolate_lens(pane, 0)
    assets.flatten_camera_lens(doc)
    r=doc['Data']['RootChunk']
    if key in BARREL_COMPONENTS:
        bounds=native['Data']['RootChunk']['boundingBox']
        if centre:
            # Ajax projects onto a 16 x 10 mm pane in the stock rear housing.
            extents=(.00807,.00001,.00501) if key=='ajax' else (.026,.00001,.018)
        else:
            centre=[(bounds['Min'][a]+bounds['Max'][a])*.5 for a in 'XYZ']
            extents=[(bounds['Max']['X']-bounds['Min']['X'])*.5,.00001,
                     (bounds['Max']['Z']-bounds['Min']['Z'])*.5]
        h=r['renderResourceBlob']['Data']['header']
        for axis,origin,extent in zip('XYZ',centre,extents):
            h['quantizationOffset'][axis]=origin
            h['quantizationScale'][axis]=extent
            r['boundingBox']['Min'][axis]=origin-extent
            r['boundingBox']['Max'][axis]=origin+extent
    ink=copy.deepcopy(material)
    marker=-717.25 if key in CAMERAS else -718.25
    style={'ticon':1,'hercules':1,'shingen':2,'yukimura':2,'senkoh':3}.get(key,0)
    ink['values']=[{'$type':'Float','ImageScale':marker},
                   {'$type':'Float','AlphaAsOne':int(key in CAMERAS)},
                   {'$type':'Float','EnableRaytracedEmissive':0},
                   {'$type':'Vector4','IntensityPerLayer':{'$type':'Vector4','X':style,'Y':0,'Z':0,'W':0}}]
    assets.set_materials(doc,[('default',ink)])
    doc['Header']['ArchiveFileName']=f'base\\vrport\\optics\\builtin_{key}.mesh'
    return doc


def add_component(app, component, handle, handles):
    duplicate=copy.deepcopy(component)
    for field in ('parentTransform','skinning','meshTargetBinding'):
        binding=component.get(field)
        if not binding or binding.get('HandleId')=='-1':continue
        data=copy.deepcopy(binding.get('Data') or handles[binding['HandleRefId']])
        component[field]={'HandleId':str(handle),'Data':data}
        duplicate[field]={'HandleRefId':str(handle)}
        handle+=1
    package=app['compiledData']['Data']
    package['CruidDict'][str(len(package['Chunks']))]=component['id']
    package['Chunks'].append(component)
    app['components'].append(duplicate)
    return handle


def author(args):
    panel=panel_template(assets.read(args.attachment))
    material=assets.read(args.camera_mesh)['Data']['RootChunk']['localMaterialBuffer']['materials'][0]
    for key,_,stem,centre in SIGHTS:
        mesh=surface(key,assets.read(args.meshes/(stem+'.mesh.json')),centre,material,args.meshes)
        save(args.output/('builtin_'+key+'.mesh.json'),mesh)
    apps={f.name:f for f in args.apps.glob('*.app.json')}
    if args.ajax:
        # Both the integral collimator and the quest short_02 belong to this
        # file. Feed the embedded appearance into the same component pass.
        embedded=args.output/args.ajax.name
        subprocess.run([sys.executable,Path(__file__).with_name('embed_scope_optics.py'),
                        '--app',args.ajax,'--scope-app','short_02='+str(args.attachment),
                        '--out',embedded],check=True)
        apps[args.ajax.name]=embedded
    for file in sorted(apps.values()):
        doc=assets.read(file)
        handles={n['HandleId']:n['Data'] for n in assets.walk(doc) if 'HandleId' in n and 'Data' in n}
        handle=assets.next_handle(doc)
        added=0
        for i,(key,token,stem,centre) in enumerate(SIGHTS):
            if token not in file.name:continue
            for j,appearance in enumerate(doc['Data']['RootChunk']['appearances']):
                app=appearance['Data']; chunks=app['compiledData']['Data']['Chunks']
                def component_ending(filename):
                    return next((c for c in chunks if 'mesh' in c and
                                 assets.path_of(c['mesh']).endswith('\\'+filename)),None)
                if not centre and component_ending(stem+'.mesh') is None:continue
                source=component_ending(BARREL_COMPONENTS.get(key,stem+'.mesh'))
                if source is None:continue
                name='vr_builtin_'+key
                if any(c['name']['$value']==name for c in chunks):raise ValueError('Already authored '+name)
                mesh=copy.deepcopy(source)
                mesh.update(mesh=assets.ref('base/vrport/optics/builtin_'+key+'.mesh'),meshAppearance=assets.cname('default'),
                            chunkMask='1',LODMode='AlwaysVisible',forceLODLevel=0,isEnabled=0,name=assets.cname(name))
                if 'visibilityAnimationParam' in mesh:mesh['visibilityAnimationParam']=assets.cname('None')
                mesh['id']=str(7952810653862800000+i*10000+j*2)
                handle=add_component(app,mesh,handle,handles)
                if key in CAMERAS:
                    widget=copy.deepcopy(panel)
                    for field in ('parentTransform','meshTargetBinding'):
                        widget[field]['Data']['bindName']=assets.cname(name)
                    widget.update(name=assets.cname(name+'_panel'),id=str(int(mesh['id'])+1),isEnabled=0)
                    handle=add_component(app,widget,handle,handles)
                added+=1
        if added:save(args.output/file.name,doc)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for key in ('apps','meshes','attachment','camera-mesh','output'):
        parser.add_argument('--'+key,type=Path,required=True)
    parser.add_argument('--ajax',type=Path)
    author(parser.parse_args())
