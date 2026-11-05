"""Reference composition using Cyberpunk's original q000 VR tutorial meshes.

The manifest keeps depot paths, transforms and appearances for native spawning. Blender materials are previews
of the extracted game textures; REDengine will use the original multilayer shaders.
"""
import bpy, bmesh, json, math, random, sys
from pathlib import Path
from mathutils import Vector, Matrix
sys.path.insert(0,str(Path(__file__).resolve().parent))
import blender_lobby as B
import tutorial_animation

ROOT=B.ROOT;OUT=ROOT/'art/vr_lobby/tutorial';WORK=ROOT/'build/lobby-tutorial'
DEPOT='base\\environment\\decoration\\unique\\quest\\q000\\q000_vr_tutorial\\'
MATROOT=WORK/'materials'
RNG=random.Random(15032077)
CACHE={};MANIFEST=[]
CUSTOM=[]
PARKOUR=[]
MICRO_TERRACES=[]
FILL_GROUPS=[]
ANIMATED=[]
WORLD_SIZE=1000.0
CIRCLE_Y=30.0
CIRCLE_AREA=40.0
CIRCLE_RADIUS=math.sqrt(CIRCLE_AREA/math.pi)
CIRCLE_LEVEL=-.24

def read(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def value(v):return v.get('$value',v) if isinstance(v,dict) else v
def image_node(nodes,path,noncolor=False):
    path=MATROOT/Path(path.replace('\\','/')).with_suffix('.png')
    if not path.exists():return None
    img=bpy.data.images.load(str(path),check_existing=True);img.pack()
    if noncolor:img.colorspace_settings.name='Non-Color'
    node=nodes.new('ShaderNodeTexImage');node.image=img;return node

def stone_floor_material():
    m=B.stone('Grey stone floor / reference slabs',(.10,.108,.12),.37,True)
    m['floor_finish']=True;m['slab_size_m']=3.0;m['joint_width_m']=.018
    n=m.node_tree.nodes;l=m.node_tree.links;p=n.get('Principled BSDF');p.inputs['Metallic'].default_value=.025
    geom=n.new('ShaderNodeNewGeometry');sep=n.new('ShaderNodeSeparateXYZ');l.new(geom.outputs['Position'],sep.inputs[0])
    borders=[]
    for axis in ('X','Y'):
        scale=n.new('ShaderNodeMath');scale.operation='MULTIPLY';scale.inputs[1].default_value=1/3
        frac=n.new('ShaderNodeMath');frac.operation='FRACT'
        inv=n.new('ShaderNodeMath');inv.operation='SUBTRACT';inv.inputs[0].default_value=1
        dist=n.new('ShaderNodeMath');dist.operation='MINIMUM'
        edge=n.new('ShaderNodeMath');edge.operation='LESS_THAN';edge.inputs[1].default_value=.003
        l.new(sep.outputs[axis],scale.inputs[0]);l.new(scale.outputs[0],frac.inputs[0]);l.new(frac.outputs[0],inv.inputs[1])
        l.new(frac.outputs[0],dist.inputs[0]);l.new(inv.outputs[0],dist.inputs[1]);l.new(dist.outputs[0],edge.inputs[0]);borders.append(edge.outputs[0])
    joint=n.new('ShaderNodeMath');joint.operation='MAXIMUM';l.new(borders[0],joint.inputs[0]);l.new(borders[1],joint.inputs[1])
    base=p.inputs['Base Color'].links[0].from_socket
    mix=n.new('ShaderNodeMixRGB');l.new(joint.outputs[0],mix.inputs[0]);l.new(base,mix.inputs[1])
    mix.inputs[2].default_value=(.035,.038,.043,1);l.new(mix.outputs[0],p.inputs['Base Color'])
    normal=p.inputs['Normal'].links[0].from_socket
    inv=n.new('ShaderNodeMath');inv.operation='SUBTRACT';inv.inputs[0].default_value=1;l.new(joint.outputs[0],inv.inputs[1])
    bump=n.new('ShaderNodeBump');bump.inputs['Strength'].default_value=.25;bump.inputs['Distance'].default_value=.004
    l.new(inv.outputs[0],bump.inputs['Height']);l.new(normal,bump.inputs['Normal']);l.new(bump.outputs[0],p.inputs['Normal'])
    p.inputs['Emission Strength'].default_value=0
    return m

def floor_finish(o):
    index=len(o.data.materials);o.data.materials.append(FLOOR_STONE);o.data.update()
    for face in o.data.polygons:
        if face.normal.z>.5:face.material_index=index
    o['stone_floor_finish']=True

def original_material(name,data):
    if name in bpy.data.materials:return bpy.data.materials[name]
    m=B.material(name,(.0356,.0356,.0356),.4,.05)
    n=m.node_tree.nodes;l=m.node_tree.links;p=n.get('Principled BSDF')
    if 'MultilayerSetup' in data:
        setup=read(MATROOT/Path(data['MultilayerSetup'].replace('\\','/')+'.json'))['Data']['RootChunk']
        masks=Path(data['MultilayerMask'].replace('\\','/'))
        uv=n.new('ShaderNodeTexCoord');last_color=last_rough=last_metal=None
        for i,layer in enumerate(setup['layers']):
            template=read(MATROOT/Path(layer['material']['DepotPath']['$value'].replace('\\','/')+'.json'))['Data']['RootChunk']
            def override(key,default):
                name=layer[key]['$value']
                v=next((v['v'] for v in template['overrides'][key] if v['n']['$value']==name),default)
                return v.get('Elements',v) if isinstance(v,dict) else v
            color=override('colorScale',[.0356]*3)
            rgb=n.new('ShaderNodeRGB');rgb.outputs[0].default_value=(*color,1)
            rough=n.new('ShaderNodeValue');levels=override('roughLevelsOut',[.5,.3])
            # REDengine stores output scale/bias, not min/max endpoints.
            rough.outputs[0].default_value=max(.1,min(.95,levels[0]*.5+levels[1]))
            metal=n.new('ShaderNodeValue');metal.outputs[0].default_value=.88 if 'metal/' in layer['material']['DepotPath']['$value'].replace('\\','/') else .025
            mask_path=masks.parent/(masks.stem+'_layers')/(masks.stem+'_'+str(i)+'.xbm')
            mask=image_node(n,str(mask_path),True)
            if i==0 or not mask:
                last_color,last_rough,last_metal=rgb.outputs[0],rough.outputs[0],metal.outputs[0]
            else:
                opacity=n.new('ShaderNodeMath');opacity.operation='MULTIPLY';opacity.inputs[1].default_value=layer.get('opacity',1)
                l.new(mask.outputs['Color'],opacity.inputs[0]);l.new(uv.outputs['UV'],mask.inputs['Vector'])
                sockets=[]
                for a,b in ((last_color,rgb.outputs[0]),(last_rough,rough.outputs[0]),(last_metal,metal.outputs[0])):
                    mix=n.new('ShaderNodeMixRGB');l.new(opacity.outputs[0],mix.inputs[0]);l.new(a,mix.inputs[1]);l.new(b,mix.inputs[2]);sockets.append(mix.outputs[0])
                last_color,last_rough,last_metal=sockets
        l.new(last_color,p.inputs['Base Color']);l.new(last_rough,p.inputs['Roughness']);l.new(last_metal,p.inputs['Metallic'])
        # Original fine surface map, with its authored UVs. Microblend parity is left to REDengine.
        tex=image_node(n,'base\\surfaces\\materials\\common\\stone_common_polished_01_300_n.xbm',True)
        if tex:
            tiling=n.new('ShaderNodeVectorMath');tiling.operation='SCALE';tiling.inputs[3].default_value=8
            l.new(uv.outputs['UV'],tiling.inputs[0]);l.new(tiling.outputs[0],tex.inputs['Vector'])
            norm=n.new('ShaderNodeNormalMap');norm.inputs['Strength'].default_value=.18;l.new(tex.outputs['Color'],norm.inputs[1]);l.new(norm.outputs[0],p.inputs['Normal'])
    elif 'DiffuseTexture' in data:
        tex=image_node(n,data['DiffuseTexture'])
        if tex:
            l.new(tex.outputs['Color'],p.inputs['Base Color']);l.new(tex.outputs['Color'],p.inputs['Emission Color'])
            p.inputs['Emission Strength'].default_value=.12
            transparent=n.new('ShaderNodeBsdfTransparent');mix=n.new('ShaderNodeMixShader')
            l.new(tex.outputs['Alpha'],mix.inputs[0]);l.new(transparent.outputs[0],mix.inputs[1]);l.new(p.outputs[0],mix.inputs[2])
            l.new(mix.outputs[0],n.get('Material Output').inputs['Surface'])
    return m

MATERIALS={}
for filename in ('block.Material.Material.json','floor.Material.json','structure.Material.json'):
    p=MATROOT/filename
    if p.exists():
        for mat in read(p).get('Materials',[]):MATERIALS.setdefault(mat['Name'],mat['Data'])

def prototype(name):
    if name in CACHE:return CACHE[name]
    path=next((WORK/'gltf').rglob(name+'.glb'))
    spec=read(ROOT/'build/vr-lobby-20261027/vrtut_json'/(name+'.mesh.json'))['Data']['RootChunk']
    slots=spec['appearances'][0]['Data']['chunkMaterials'];slotnames=[m['$value'] for m in slots]
    bbox=spec['boundingBox'];lo=bbox['Min'];hi=bbox['Max']
    pivot=Vector(((lo['X']+hi['X'])/2,(lo['Y']+hi['Y'])/2,lo['Z']))
    before=set(bpy.data.objects);bpy.ops.import_scene.gltf(filepath=str(path));added=set(bpy.data.objects)-before
    items=[]
    for o in added:
        if o.type!='MESH':continue
        idx=int(o.name.split('_')[1]);slot=slotnames[min(idx,len(slotnames)-1)]
        mesh=o.data.copy();mesh.transform(Matrix.Translation(-pivot)@o.matrix_world);mesh.materials.clear()
        # Animated tutorial light strips are replaced by steady amber in the lobby preview.
        mat=AMBER if slot=='lights' else original_material(slot,MATERIALS.get(slot,{}))
        mesh.materials.append(mat)
        for face in mesh.polygons:face.material_index=0
        if name==SLAB:
            mesh.materials.append(FLOOR_STONE);mesh.update()
            for face in mesh.polygons:
                if face.normal.z>.5:face.material_index=1
        items.append(mesh)
    for o in added:bpy.data.objects.remove(o,do_unlink=True)
    CACHE[name]=(items,pivot,Vector((hi['X']-lo['X'],hi['Y']-lo['Y'],hi['Z']-lo['Z'])))
    return CACHE[name]

def kit(name,pos,dimensions,col,yaw=0,label=None):
    # Clear corridors connect the forecourt to all four horizons. Large background assemblies stay overhead.
    if name==BACKGROUND:
        x,y,z=pos;w,d,h=dimensions
        corridor=((y>35 and abs(x)<9+w/2) or
                  (y<-25 and abs(x+5)<10+w/2) or
                  (x>24 and abs(y-13)<10+d/2) or
                  (x<-24 and abs(y-2)<10+d/2))
        if corridor:pos=(x,y,max(z,17.0))
    meshes,pivot,size=prototype(name);scale=Vector([dimensions[i]/size[i] for i in range(3)])
    rotation=Matrix.Rotation(yaw,4,'Z')
    for i,mesh in enumerate(meshes):
        o=bpy.data.objects.new((label or name)+f' / {i}',mesh);col.objects.link(o);o.location=pos;o.scale=scale;o.rotation_euler.z=yaw
        o['depot_path']=DEPOT+name+'.mesh';o['appearance']='default';o['original_game_geometry']=True
    native_pos=Vector(pos)-rotation.to_3x3()@Vector([pivot[k]*scale[k] for k in range(3)])
    MANIFEST.append({'name':label or name,'mesh':DEPOT+name+'.mesh','appearance':'default',
                     'position':list(native_pos),'rotation_degrees':[0,0,math.degrees(yaw)],'scale':list(scale),
                     'authoring_center_bottom':list(pos),'dimensions':list(dimensions),
                     'streaming_cell':[math.floor(pos[0]/64),math.floor(pos[1]/64)],
                     'detail_band':'near' if math.hypot(pos[0],pos[1])<70 else 'middle' if math.hypot(pos[0],pos[1])<180 else 'far'})
    return pos,dimensions,yaw

BLOCK='vr_cover_block_h200_w200_l200_a'
SLAB='vr_cover_block_h110_w100_l200_a'
FLOOR='vr_architecture_floor_w600_l600_a'
STAIR='vr_architecture_stairs_h200_w300_a'
BACKGROUND='q000_vr_background_structure_01_a'

def lit_block(name,pos,size,col,yaw=0,light=True,asset=BLOCK):
    kit(asset,pos,size,col,yaw,name)
    if light:
        p=B.empty(name+' / light',pos,col,yaw)
        B.box(name+' / recessed light',(0,-size[1]/2-.009,.06),(size[0]*.9,.025,.027),AMBER,col,.003,p)

def block_cluster(name,pos,width,depth,foot,pieces,col,yaw):
    lit_block(name+' / broad foundation',pos,(width,depth,foot),col,yaw,asset=SLAB)
    rot=Matrix.Rotation(yaw,3,'Z');root=B.empty(name+' / upper seam',pos,col,yaw)
    B.box('Foundation top ribbon',(0,-depth/2-.008,foot-.045),(width*.9,.015,.026),AMBER,col,.003,root)
    for i,(x,y,z,w,d,h,angle) in enumerate(pieces):
        at=Vector(pos)+rot@Vector((x,y,foot+z))
        lit_block(name+f' / offset mass {i}',tuple(at),(w,d,h),col,yaw+math.radians(angle),light=(i%3==1))

def floor_ribbon(name,pos,length,yaw,col):
    root=B.empty(name,pos,col,yaw)
    B.box(name+' / inset',(0,0,.008),(length,.12,.016),bpy.data.materials['Black recess'],col,.005,root)
    B.box(name+' / light',(0,0,.019),(length*.96,.043,.008),AMBER,col,.002,root)

def fill_ground_regions(col):
    rng=random.Random(70933);occupied=[]
    def envelope(x,y,w,d,yaw):
        return (x,y,(abs(w*math.cos(yaw))+abs(d*math.sin(yaw)))/2,
                (abs(w*math.sin(yaw))+abs(d*math.cos(yaw)))/2)
    for v in CUSTOM:
        if v['type'] in ('ground_with_basin_cutout','micro_terrain'):continue
        size=v.get('size') or [v['width_m'],v['depth_m'],v['height_m']]
        occupied.append(envelope(v['position'][0],v['position'][1],size[0],size[1],math.radians(v.get('yaw_degrees',0))))
    for v in MANIFEST:
        if v['authoring_center_bottom'][2]<.5:
            occupied.append(envelope(*v['authoring_center_bottom'][:2],*v['dimensions'][:2],math.radians(v['rotation_degrees'][2])))
    for x,y in ((0,0),(8,18),(10,34),(4,-8),(8,-8),(14,16),(-9,-1)):
        occupied.append((x,y,2.0,2.0))
    regions=[('Ring west fill',(-27,-8.5,19,40),10),('Ring east fill',(8.5,28,18,40),10),
             ('East north fill',(29,111,33,49),12),('East south fill',(29,101,-20,-7),11),
             ('West north fill',(-108,-33,18,35),12),('West south fill',(-101,-28,-25,-12),11)]
    for name,(x0,x1,y0,y1),target in regions:
        placed=[]
        for attempt in range(4000):
            if len(placed)>=target:break
            x,y=rng.uniform(x0,x1),rng.uniform(y0,y1)
            w,d,h=rng.uniform(1.7,3.8),rng.uniform(1.6,3.6),rng.uniform(1.1,5.5);yaw=rng.uniform(-math.pi,math.pi)
            e=envelope(x,y,w,d,yaw)
            if abs(x)<7+e[2] and 16<y<47:continue
            if any(abs(x-ox)<e[2]+ex+.50 and abs(y-oy)<e[3]+ey+.50 for ox,oy,ex,ey in occupied):continue
            label=name+f' / mass {len(placed)+1:02}'
            lit_block(label,(x,y,0),(w,d,h),col,yaw,light=rng.random()<.42)
            occupied.append(e);placed.append({'name':label,'position':[x,y,0],'size':[w,d,h],'yaw_degrees':math.degrees(yaw)})
        assert len(placed)>=target*.7,(name,len(placed),target)
        FILL_GROUPS.append({'name':name,'requested_count':target,'blocks':placed})

def solid_mesh(name,vertices,faces,mat,col):
    mesh=bpy.data.meshes.new(name);mesh.from_pydata(vertices,[],faces);mesh.materials.append(mat)
    bm=bmesh.new();bm.from_mesh(mesh);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(mesh);bm.free()
    o=bpy.data.objects.new(name,mesh);col.objects.link(o)
    o['collision']='closed solid';o['custom_lobby_geometry']=True
    return o

def soften(o,width=.025):
    m=o.modifiers.new('Cast edge chamfer','BEVEL');m.width=width;m.segments=2
    o.modifiers.new('Weighted solid normals','WEIGHTED_NORMAL')

def recess(o,name,loc,size,col):
    # Actual subtractive geometry. Cut before the edge bevel so the recess has its own reveals.
    cutter=B.box(name,loc,size,None,col,0)
    m=o.modifiers.new('Recess / '+name,'BOOLEAN');m.operation='DIFFERENCE';m.solver='EXACT';m.object=cutter
    bpy.context.view_layer.objects.active=o;o.select_set(True)
    bpy.ops.object.modifier_apply(modifier=m.name)
    bpy.data.objects.remove(cutter,do_unlink=True);o.select_set(False)

def combine_solid(body,other,operation):
    m=body.modifiers.new('Integrated terrain '+operation,'BOOLEAN');m.operation=operation;m.solver='EXACT';m.object=other
    bpy.context.view_layer.objects.active=body;body.select_set(True)
    bpy.ops.object.modifier_apply(modifier=m.name)
    bpy.data.objects.remove(other,do_unlink=True);body.select_set(False)

def inset_terrace(name,center,width,depth,top,base,run,stair_width,offset,col,mat,yaw=0):
    """One watertight terrace with its stair flight cut INTO the front edge; no separate stair object."""
    count=max(3,math.ceil((top-base)/.18));step=(top-base)/count;tread=run/count;front=-depth/2
    body=B.box(name,(0,0,top/2),(width,depth,top),mat,col,0)
    body['custom_lobby_geometry']=True;body['collision']='recessed terrace with stairwell'
    # The cutter is the air above the treads. It crosses the facade but ends at the upper walking plane.
    profile=[(front-.06,base+step)]
    for k in range(count):
        y=front+(k+1)*tread;h=base+(k+1)*step
        profile.append((y,h))
        if k<count-1:profile.append((y,h+step))
    profile += [(front+run,top+.5),(front-.06,top+.5)]
    n=len(profile);verts=[(x,y,z) for x in (offset-stair_width/2,offset+stair_width/2) for y,z in profile]
    faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
    cutter=solid_mesh('Stairwell subtraction',verts,faces,mat,col)
    combine_solid(body,cutter,'DIFFERENCE');floor_finish(body);soften(body,.013)
    body.location.x,body.location.y=center;body.rotation_euler.z=yaw
    left=offset-stair_width/2;right=offset+stair_width/2
    boxes=[{'center':[(-width/2+left)/2,0,top/2],'size':[left+width/2,depth,top]},
           {'center':[(right+width/2)/2,0,top/2],'size':[width/2-right,depth,top]},
           {'center':[offset,run/2,top/2],'size':[stair_width,depth-run,top]}]
    for k in range(count):
        h=base+(k+1)*step
        boxes.append({'center':[offset,front+(k+.5)*tread,h/2],'size':[stair_width,tread,h]})
    CUSTOM.append({'name':body.name,'type':'inset_terrace','position':[center[0],center[1],0],
                   'size':[width,depth,top],'yaw_degrees':math.degrees(yaw),'base_height_m':base,
                   'top_height_m':top,'stair_run_m':run,'stair_width_m':stair_width,'stair_offset_m':offset,
                   'stair_local_y_range':[front,front+run],'steps':count,'riser_m':step,
                   'integrated_stairs':True,'collision_local_boxes':boxes})
    body['integrated_stairs']=True;body['steps']=count
    # Interrupted ribbons at the mouth of the stairs, recessed into the foot of the surrounding terrace.
    light_root=B.empty(name+' / threshold ribbons',(center[0],center[1],0),col,yaw)
    for a,b in ((-width/2+.22,left-.16),(right+.16,width/2-.22)):
        if b-a>.4:
            B.box('Threshold shadow channel',((a+b)/2,front-.009,base+.045),(b-a,.027,.07),
                  bpy.data.materials['Black recess'],col,.003,light_root)
            B.box('Threshold amber ribbon',((a+b)/2,front-.025,base+.045),((b-a)*.95,.012,.026),
                  AMBER,col,.002,light_root)
    return body

def recessed_plaza(col,mat):
    # One continuous forecourt. The circle is BELOW the surrounding floor, with broad shallow approaches.
    body=B.box('Central plaza / recessed circle and shallow levels',(0,30,-.265),(42,30,.55),mat,col,0)
    regions=[
      ('West shallow shelf',[(-18,23),(-9,24.2),(-9.2,31),(-17,29.8)],.27),
      ('East low shelf',[(9,21.5),(17,20.7),(18.4,27.2),(9.2,28.6)],.22),
      ('Northwest staggered shelf',[(-17.8,33.5),(-8.5,34.6),(-8.7,41.5),(-18.6,39.2)],.30),
      ('Northeast shelf',[(9.1,32.2),(17.2,31.1),(18.1,39),(9.0,40.4)],.26),
      ('Front left ledge',[(-14,16),(-6.5,16.9),(-6.9,22.3),(-15.5,21.1)],.23),
      ('Front right ledge',[(7.8,16),(17,16.8),(16.5,21.6),(8,21)],.28),
    ]
    for name,points,h in regions:
        n=len(points);verts=[(x,y,z) for z in (-.04,h) for x,y in points]
        faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
        part=solid_mesh(name,verts,faces,mat,col);combine_solid(body,part,'UNION')
        MICRO_TERRACES.append({'name':name,'polygon_xy':points,'top_height_m':h})
    basin=[(-6.4,24.5),(5.8,24),(7,29),(6.2,36.8),(-5.8,37.3),(-7.2,31.2)]
    n=len(basin);verts=[(x,y,z) for z in (CIRCLE_LEVEL,.6) for x,y in basin]
    faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
    combine_solid(body,solid_mesh('Circle basin subtraction',verts,faces,mat,col),'DIFFERENCE')
    MICRO_TERRACES.append({'name':'Lowered circle basin','polygon_xy':basin,'top_height_m':CIRCLE_LEVEL})
    # Gentle transitions in/out of the depression; there is no raised rectangular island under the ring.
    for name,profile in [
        ('Front descent',[(21.8,.01),(23.9,CIRCLE_LEVEL),(24.7,CIRCLE_LEVEL),(24.7,.65),(21.8,.65)]),
        ('Return toward stairs',[(36.5,CIRCLE_LEVEL),(37.4,CIRCLE_LEVEL),(39.8,.01),(39.8,.65),(36.5,.65)])]:
        n=len(profile)
        verts=[(x,y,z) for x in (-4.0,4.0) for y,z in profile]
        faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
        combine_solid(body,solid_mesh(name,verts,faces,mat,col),'DIFFERENCE')
    floor_finish(body);soften(body,.007);body['custom_lobby_geometry']=True;body['collision']='terrain mesh'
    CUSTOM.append({'name':body.name,'type':'micro_terrain','position':[0,30,-.54],
                   'size':[42,30,.84],'collision_mesh_required':True,'levels':MICRO_TERRACES,
                   'circle_depressed':True,'descent_m':.25})
    return body

def cast_stairs(name,pos,width,run,rise,col,mat):
    count=max(4,math.ceil(rise/.18));tread=run/count;step=rise/count
    profile=[(0,-.15),(run,-.15),(run,rise)]
    for k in range(count,0,-1):
        profile.append(((k-1)*tread,k*step))
        if k>1:profile.append(((k-1)*tread,(k-1)*step))
    n=len(profile);vertices=[(x,y-run/2,z) for x in (-width/2,width/2) for y,z in profile]
    faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]
    faces += [(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
    o=solid_mesh(name,vertices,faces,mat,col);o.location=pos;soften(o,.012)
    o['steps']=count;o['tread_m']=tread;o['riser_m']=step
    CUSTOM.append({'name':o.name,'type':'cast_stairs','position':list(pos),'width_m':width,'run_m':run,
                   'rise_m':rise,'steps':count,'riser_m':step,'tread_m':tread,
                   'collision_boxes':[{'center':[pos[0],pos[1]-run/2+(k+.5)*tread,pos[2]+(k+1)*step/2],
                                       'size':[width,tread,(k+1)*step]} for k in range(count)]})
    return o

def inverted_t(name,pos,width,depth,base_height,stem_width,total_height,col,mat,yaw=0,stem_offset=0):
    # One closed inverted-T profile: a broad continuous foot and narrower vertical mass.
    profile=[(-width/2,0),(width/2,0),(width/2,base_height),(stem_offset+stem_width/2,base_height),
             (stem_offset+stem_width/2,total_height),(stem_offset-stem_width/2,total_height),
             (stem_offset-stem_width/2,base_height),(-width/2,base_height)]
    n=len(profile);verts=[(x,y,z) for y in (-depth/2,depth/2) for x,z in profile]
    faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
    o=solid_mesh(name,verts,faces,mat,col)
    # Recesses remain solid at the back, and do not remove the broad landing surfaces on top.
    for sign in (-1,1):
        recess(o,'Foot channel',(0,sign*(depth/2-.06),base_height*.50),(width*.84,.34,.22),col)
        recess(o,'Narrow construction seam',(stem_offset+stem_width*.18,sign*(depth/2-.08),base_height+(total_height-base_height)*.49),
               (.19,.44,(total_height-base_height)*.82),col)
    soften(o,.032);o.location=pos;o.rotation_euler.z=yaw
    root=B.empty(name+' / inset lighting',pos,col,yaw)
    for sign in (-1,1):
        B.box('Inset amber foot light',(0,sign*(depth/2-.205),base_height*.50-.075),
              (width*.76,.014,.022),AMBER,col,.003,root)
        B.box('Inset vertical light',(stem_offset+stem_width*.18,sign*(depth/2-.285),base_height+(total_height-base_height)*.49),
              (.026,.014,(total_height-base_height)*.76),AMBER,col,.003,root)
    CUSTOM.append({'name':name,'type':'inverted_t','position':list(pos),'yaw_degrees':math.degrees(yaw),
                   'width_m':width,'depth_m':depth,'base_height_m':base_height,'stem_width_m':stem_width,
                   'height_m':total_height,'stem_offset_m':stem_offset,'recess_depth_m':.30,
                   'collision_local_boxes':[{'center':[0,0,base_height/2],'size':[width,depth,base_height]},
                       {'center':[stem_offset,0,base_height+(total_height-base_height)/2],'size':[stem_width,depth,total_height-base_height]}]})
    return o

def compound_t(name,pos,total_height,col,mat,rng,yaw=0,width=None,depth=None,stem_width=None,landing=False):
    width=width or rng.uniform(8.1,12.8);depth=depth or rng.uniform(4.2,6.6)
    stem_width=stem_width or rng.uniform(3.6,5.7);base_height=rng.uniform(.85,1.55)
    offset=0 if landing else rng.uniform(-.85,.85)
    inverted_t(name,pos,width,depth,base_height,stem_width,total_height,col,mat,yaw,offset)
    rot=Matrix.Rotation(yaw,3,'Z')
    # Unequal secondary masses make each T an assembly, rather than copies of one freestanding monument.
    for j,side in enumerate((-1,1)):
        if j==1 and rng.random()<.4:continue
        wing=max(.75,(width-stem_width)/2-abs(offset)-.18)
        size=[wing*rng.uniform(.65,.92),depth*rng.uniform(.55,.87),rng.uniform(.8,2.4)]
        local=Vector((offset+side*(stem_width/2+size[0]/2+.04),rng.uniform(-.22,.22)*depth,base_height))
        at=Vector(pos)+rot@local;cyaw=yaw+rng.uniform(-.08,.08)
        o=B.box(name+f' / secondary mass {j+1}',(at.x,at.y,at.z+size[2]/2),size,mat,col,.025,rotation=(0,0,cyaw))
        o['custom_lobby_geometry']=True
        CUSTOM.append({'name':o.name,'type':'t_block_stack','position':list(at),'size':size,
                       'yaw_degrees':math.degrees(cyaw),'collision_local_boxes':[{'center':[0,0,size[2]/2],'size':size}]})

def landing_gap(a,b):
    def polygon(v):
        yaw=math.radians(v.get('yaw_degrees',0));c,s=math.cos(yaw),math.sin(yaw)
        return [(v['center'][0]+c*x-s*y,v['center'][1]+s*x+c*y)
                for x,y in [(-v['width_m']/2,-v['depth_m']/2),(v['width_m']/2,-v['depth_m']/2),
                            (v['width_m']/2,v['depth_m']/2),(-v['width_m']/2,v['depth_m']/2)]]
    def cross(a,b,p):return (b[0]-a[0])*(p[1]-a[1])-(b[1]-a[1])*(p[0]-a[0])
    def inside(p,poly):
        cs=[cross(poly[i],poly[(i+1)%4],p) for i in range(4)]
        return min(cs)>=-1e-8 or max(cs)<=1e-8
    def distance(p,a,b):
        dx,dy=b[0]-a[0],b[1]-a[1]
        t=max(0,min(1,((p[0]-a[0])*dx+(p[1]-a[1])*dy)/(dx*dx+dy*dy)))
        return math.hypot(p[0]-a[0]-t*dx,p[1]-a[1]-t*dy)
    pa,pb=polygon(a),polygon(b)
    if any(inside(p,pb) for p in pa) or any(inside(p,pa) for p in pb):return 0.0
    for i in range(4):
        x,y=pa[i],pa[(i+1)%4]
        for j in range(4):
            u,v=pb[j],pb[(j+1)%4]
            if (cross(x,y,u)*cross(x,y,v)<=0 and cross(u,v,x)*cross(u,v,y)<=0 and
                max(min(x[0],y[0]),min(u[0],v[0]))<=min(max(x[0],y[0]),max(u[0],v[0]))+1e-8 and
                max(min(x[1],y[1]),min(u[1],v[1]))<=min(max(x[1],y[1]),max(u[1],v[1]))+1e-8):return 0.0
    return min([distance(p,pb[j],pb[(j+1)%4]) for p in pa for j in range(4)]+
               [distance(p,pa[j],pa[(j+1)%4]) for p in pb for j in range(4)])

def carved_jump_block(name,pos,width,depth,height,yaw,col,mat,rng,avoid_style=None):
    o=B.box(name,(0,0,height/2),(width,depth,height),mat,col,0)
    base=min(height*.30,max(.18,height*.18));cap=min(height*.36,max(.20,height*.22));cuts=[]
    def cut(label,at,size):
        recess(o,label,at,size,col);cuts.append({'name':label,'center':list(at),'size':list(size)})
    if height<.9:
        kind='shallow inset'
        cut('Low block recess',(0,-depth/2+.07,height*.50),(width*.52,.30,height*.25))
    else:
        kind=rng.choice([v for v in ('through window','corner bay','offset recesses') if v!=avoid_style])
        opening=(height-base-cap)*rng.uniform(.62,.90)
        bottom=base+(height-base-cap-opening)*rng.uniform(.2,.8);z=bottom+opening/2
        if kind=='through window':
            cut('Through window',(rng.uniform(-.035,.035)*width,0,z),(width*rng.uniform(.32,.42),depth+.2,opening))
        elif kind=='corner bay':
            side=rng.choice((-1,1))
            cut('Corner bay',(side*width*.43,-depth*.39,z),(width*.46,depth*.43,opening))
        else:
            cut('Front inset',(-width*.13,-depth/2+.06,z),(width*.48,depth*.40,opening))
            cut('Rear offset inset',(width*.18,depth/2-.06,z+.02),(width*.33,depth*.32,opening*.72))
    soften(o,.018);o.location.x,o.location.y=pos;o.rotation_euler.z=yaw
    o['custom_lobby_geometry']=True;o['parkour_top_height_m']=height;o['collision']='carved mesh'
    o['solid_base_thickness_m']=base;o['solid_landing_thickness_m']=cap
    return o,{'cutout_style':kind,'cutouts':cuts,'solid_base_thickness_m':base,
              'solid_landing_thickness_m':cap,'minimum_side_pier_m':width*.25,
              'collision_mesh_required':True}

def parkour_route(name,points,target,col,mat):
    route=[]
    previous_cut_style=None
    route_rng=random.Random(sum((i+1)*ord(c) for i,c in enumerate(name)))
    for i,(x,y,h,width,depth) in enumerate(points):
        angle=route_rng.uniform(-9,11);yaw=math.radians(angle)
        cut_rng=random.Random(600+i*17+sum(ord(c) for c in name))
        o,carving=carved_jump_block(f'{name} / jump block {i+1}',(x,y),width,depth,h,yaw,col,mat,cut_rng,previous_cut_style)
        previous_cut_style=carving['cutout_style']
        o['parkour_top_height_m']=h;o['collision']='carved mesh';o['custom_lobby_geometry']=True
        route.append({'name':o.name,'center':[x,y],'width_m':width,'depth_m':depth,'top_height_m':h,'yaw_degrees':angle})
        CUSTOM.append({'name':o.name,'type':'parkour_block','position':[x,y,0],'size':[width,depth,h],'yaw_degrees':angle,
                       **carving})
        # Only a small flush guide under the landing edge, keeping the top clear for the player's feet.
        root=B.empty(name+' / guide orientation',(x,y,0),col,yaw)
        B.box(name+' / jump guide',(0,-depth/2-.005,max(.12,h-.13)),(width*.68,.012,.022),AMBER,col,.003,root)
    route.append(target)
    # Record real edge-to-edge gaps and vertical rise; runtime jump/collision tuning comes in-engine.
    jumps=[]
    for a,b in zip(route,route[1:]):
        jumps.append({'from':a['name'],'to':b['name'],'edge_gap_m':landing_gap(a,b),'rise_m':b['top_height_m']-a['top_height_m']})
    assert max(j['edge_gap_m'] for j in jumps)<1.0 and max(j['rise_m'] for j in jumps)<=.75
    PARKOUR.append({'name':name,'landings':route,'jumps':jumps,'runtime_jump_tested':False})

def panel_logo(parent,col):
    atlas=next((ROOT/'build/lobby-blender/logo').rglob('menu_background.png'))
    img=bpy.data.images.load(str(atlas),check_existing=True);img.pack()
    m=bpy.data.materials.new('Original Cyberpunk logo');m.use_nodes=True;n=m.node_tree.nodes;l=m.node_tree.links;n.clear()
    out=n.new('ShaderNodeOutputMaterial');tex=n.new('ShaderNodeTexImage');tex.image=img
    glow=n.new('ShaderNodeEmission');glow.inputs[0].default_value=(1,.75,.06,1);glow.inputs[1].default_value=1.6
    trans=n.new('ShaderNodeBsdfTransparent');mix=n.new('ShaderNodeMixShader')
    l.new(tex.outputs['Alpha'],mix.inputs[0]);l.new(trans.outputs[0],mix.inputs[1]);l.new(glow.outputs[0],mix.inputs[2]);l.new(mix.outputs[0],out.inputs[0])
    mesh=bpy.data.meshes.new('Logo plane');mesh.from_pydata([(-.44,-.067,.27),(.44,-.067,.27),(.44,-.067,.49),(-.44,-.067,.49)],[],[(0,1,2,3)])
    uv=mesh.uv_layers.new();coords=[(.465676218,1-.879329026),(.987704933,1-.879329026),(.987704933,1-.811958849),(.465676218,1-.811958849)]
    for loop in mesh.loops:uv.data[loop.index].uv=coords[loop.vertex_index]
    o=bpy.data.objects.new('Cyberpunk 2077 / game atlas',mesh);col.objects.link(o);o.parent=parent;mesh.materials.append(m)

def floor_ring(name,centre,radius,width,mat,col,z=.012):
    # Flat inlay, not a raised pipe. The two radii share a plane just above the floor.
    vertices=[];faces=[];segments=192
    for i in range(segments):
        a=2*math.pi*i/segments
        for r in (radius-width/2,radius+width/2):
            vertices.append((centre[0]+r*math.cos(a),centre[1]+r*math.sin(a),z))
    for i in range(segments):
        j=(i+1)%segments;faces.append((2*i,2*i+1,2*j+1,2*j))
    mesh=bpy.data.meshes.new(name);mesh.from_pydata(vertices,[],faces);mesh.materials.append(mat)
    o=bpy.data.objects.new(name,mesh);col.objects.link(o)

def build():
    global AMBER,FLOOR_STONE
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    floors=B.collection('01 / Native tutorial floor');near=B.collection('02 / Native near composition');air=B.collection('03 / Suspended tutorial blocks')
    far=B.collection('04 / Infinity / original background structures');ceiling=B.collection('05 / Ceiling');props=B.collection('06 / Terminal and table');pod=B.collection('07 / Shell alcove')
    lighting=B.collection('08 / Atmosphere');cameras=B.collection('09 / Reference cameras')
    parkour=B.collection('10 / Cast inverted-T blocks and parkour')
    motion=B.collection('11 / Native moving VR blocks')
    AMBER=B.material('Lobby steady amber',(1,.48,.11),.4,0,5)
    metal=B.material('Graphite metal',(.012,.015,.02),.28,.65);black=B.material('Black recess',(.003,.004,.006),.5,0)
    trim=B.material('Brass trim',(.25,.15,.045),.25,.8);letter=B.material('Warm white labels',(.7,.72,.66),.4,0,.6)
    glass=B.material('Screen black',(.002,.003,.004),.4,0);glass.node_tree.nodes.get('Principled BSDF').inputs['Specular IOR Level'].default_value=.1
    font=bpy.data.fonts.load('C:/Windows/Fonts/ARIALN.TTF')
    cast=B.stone('Cast solid / stone composite',(.065,.075,.09),.58)
    FLOOR_STONE=stone_floor_material()
    # The tutorial's black panels and white stripe geometry are removed, not merely tinted over.
    ground=B.box('One square kilometre stone floor',(0,0,-.04),(WORLD_SIZE,WORLD_SIZE,.08),FLOOR_STONE,floors,0)
    ground['stone_floor_finish']=True
    cutter=B.box('Forecourt clearance',(0,30,0),(42,30,2),None,floors,0)
    combine_solid(ground,cutter,'DIFFERENCE');ground['custom_lobby_geometry']=True
    CUSTOM.append({'name':ground.name,'type':'ground_with_basin_cutout','position':[0,0,-.08],
                   'size':[WORLD_SIZE,WORLD_SIZE,.08],'collision_mesh_required':True})
    recessed_plaza(floors,cast)
    for r,w in ((.85,.025),(CIRCLE_RADIUS,.055),(CIRCLE_RADIUS+.10,.018)):
        floor_ring('Central circle / 40 square metres',(0,CIRCLE_Y),r,w*2,trim if r>CIRCLE_RADIUS else black,floors,CIRCLE_LEVEL+.012)
    B.cylinder('Inset centre',(0,CIRCLE_Y,CIRCLE_LEVEL+.004),.84,.012,black,floors)
    # Every flight is a recess in a terrace, with upper floor continuing on both sides and behind it.
    # Offsets, dimensions and directions differ; successive levels overlap enough to remain connected.
    inset_terrace('First terrace / inset stairwell',(0,56),22,30,1.2,0,5.2,8.6,-1.0,near,cast)
    inset_terrace('Second terrace / offset stairwell',(2,86.5),25,43,2.6,1.2,7,6.4,3.0,near,cast,math.radians(5))
    inset_terrace('Third terrace / angled stairwell',(-1.4,134.5),30,65,4.2,2.6,10,7.2,-4.6,near,cast,math.radians(-6))
    inset_terrace('Fourth terrace / offset stairwell',(4,191.5),32,63,6.0,4.2,13,9.4,3.5,near,cast,math.radians(3))
    # A second open vista behind the observer, with depth comparable to the forward route.
    inset_terrace('Rear first continuation',(-3,-56),24,30,1.1,0,5.5,7.0,3.0,near,cast,math.pi)
    inset_terrace('Rear second continuation',(-9,-90),29,44,2.5,1.1,7.5,6.3,-3.4,near,cast,math.pi)
    inset_terrace('Rear third continuation',(-4,-143),34,68,4.1,2.5,11,8.0,4.0,near,cast,math.radians(184))
    inset_terrace('Rear fourth continuation',(-12,-221),36,94,5.8,4.1,13,9.0,-3.0,near,cast,math.radians(177))
    # Right: a nearer staircase turning outward. Left: a longer, lower passage before the first rise.
    inset_terrace('East first stair passage',(38,13),19,30,.85,0,5,6.0,-3.0,near,cast,math.radians(-90))
    inset_terrace('East second passage',(71,18),23,42,2.0,.85,7,7.0,2.0,near,cast,math.radians(-96))
    inset_terrace('East third passage',(119,12),28,62,3.6,2.0,9,8.0,-4.0,near,cast,math.radians(-87))
    inset_terrace('East far passage',(194,21),34,92,5.3,3.6,13,9.0,3.0,near,cast,math.radians(-94))
    inset_terrace('West low passage',(-67,1),20,44,.45,0,5,7.3,-2.0,near,cast,math.radians(94))
    inset_terrace('West second passage',(-118,-3),25,64,1.6,.45,7,7.0,2.0,near,cast,math.radians(86))
    inset_terrace('West far passage',(-196,5),32,104,2.9,1.6,10,9.0,-3.0,near,cast,math.radians(93))
    for name,pos,length,angle in [
        ('West entry ribbon',(-6.5,5.6,0),3.2,12),('West diagonal ribbon',(-11.2,9.4,0),4.6,-9),
        ('East entry ribbon',(8.5,8.0,0),3.7,-16),('East approach ribbon',(16.0,12.2,0),4.8,7),
        ('Front stair left ribbon',(-8.1,39.5,0),4.0,4),('Front stair right ribbon',(7.2,40.2,0),4.9,-8),
        ('Rear entry ribbon',(-5.0,-11.0,0),3.3,-13),('Rear approach ribbon',(5.5,-32.0,0),4.4,11)]:
        floor_ribbon(name,pos,length,math.radians(angle),near)
    block_cluster('Reference left tiered assembly',(-9.6,15.3,0),7.2,5.5,.85,
        [(-1.25,.2,0,2.4,2.7,3.3,0),(.95,.75,0,1.55,2.0,4.6,-9),
         (2.45,-.4,0,1.8,2.5,1.25,13),(-1.0,.25,3.3,2.8,2.9,.65,5)],near,math.radians(16))
    block_cluster('Reference right low assembly',(11.5,25.7,0),8.3,5.2,.65,
        [(-1.8,.45,0,2.8,3.1,2.1,6),(1.5,.1,0,2.3,2.6,3.65,-12),
         (.5,-1.2,0,2.5,1.8,1.1,18)],near,math.radians(-11))
    # Fixed random seed gives irregular compositions without changing the scene between angle captures.
    local_rng=random.Random(271026)
    arrangements=[
        ('West foreground assembly',(-16,-5,0),(6.6,8.1),95),
        ('East tall assembly',(22,-6,0),(9.2,11.7),-103),
        ('West middle assembly',(-27,14,0),(9.1,12.4),48),
        ('East offset assembly',(31,29,0),(6.8,9.4),-29),
        ('Northwest assembly',(-24,44,0),(7.7,10.6),20),
        ('Northeast high assembly',(29,62,0),(11.0,14.0),-13),
        ('Rear left foreground',(-17,-24,0),(5.5,7.3),169),
        ('Rear right foreground',(19,-34,0),(8.6,10.8),207),
        ('Rear upper left',(-19,-58,0),(9.2,12.5),153),
        ('Rear upper right',(17,-83,0),(5.9,8.1),193),
        ('Rear east flank',(31,-21,0),(7.3,9.0),-129),
        ('Rear west flank',(-32,-44,0),(10.5,13.8),134)]
    for name,pos,heights,angle in arrangements:
        compound_t(name,pos,local_rng.uniform(*heights),parkour,cast,local_rng,math.radians(angle))
    compound_t('West route landing',(-18.5,22,0),4.8,parkour,cast,local_rng,math.radians(5),width=9.2,depth=5.0,stem_width=4.4,landing=True)
    compound_t('East route landing',(20.9,25.4,0),5.65,parkour,cast,local_rng,math.radians(-9),width=9.8,depth=5.0,stem_width=4.2,landing=True)
    parkour_route('West zigzag',[
        (-7.8,0,.55,3.0,2.7),(-10.7,3.2,1.15,2.8,2.4),(-8.6,6.1,1.8,3.1,2.9),
        (-11.6,8.8,2.4,2.7,3.2),(-13.9,12,2.95,3.5,2.8),(-11.3,14.9,3.6,2.9,3.0),(-14.2,17.8,4.1,3.1,2.5)],
        {'name':'West route landing','center':[-18.5,22],'width_m':4.4,'depth_m':5.0,'top_height_m':4.8,'yaw_degrees':5},parkour,cast)
    parkour_route('East staggered',[
        (9,-6,.65,3.4,2.9),(11.8,-2.5,1.3,3.0,3.1),(9.5,.95,1.95,3.6,2.8),(13,4.1,2.55,3.0,3.2),
        (15.6,7.8,3.2,3.4,3.2),(12.8,11.5,3.0,3.5,2.6),(15.7,14.7,3.65,3.0,2.8),
        (18.7,18,4.3,3.6,3.0),(16.4,21.5,4.95,3.1,3.2)],
        {'name':'East route landing','center':[20.9,25.4],'width_m':4.2,'depth_m':5.0,'top_height_m':5.65,'yaw_degrees':-9},parkour,cast)
    fill_ground_regions(near)
    # Big but separated blocks: every gap reveals a deeper layer.
    heroes=[(-10.8,22,10,4.8,4.8,6.4),(-6.7,31,13,4.5,4.8,6.0),(.8,38,17,6.3,5.4,6.0),
            (10.2,28,10,4.8,5.0,7.5),(16,36,13,5,5,6.5),(-17,39,14,5.8,5,6.3),
            (-2.0,60,12,6.1,6.2,6.0),(8.0,66,16,6,6,6.5),(-12,72,15,6,6,8)]
    for i,(x,y,z,w,d,h) in enumerate(heroes):
        lit_block('Reference floating block '+str(i),(x,y,z),(w,d,h),air,RNG.uniform(-.42,.42))
    for row,y in enumerate((40,58,81,110,146,191,245,310)):
        for col,x in enumerate((-58,-39,-20,0,20,39,58)):
            if col==3 and row>0:continue  # the ascending path remains open beyond the circle
            xx=x+RNG.uniform(-2,2)
            if row<5:
                for tier in range(2):
                    lit_block(f'Mid depth {row}/{col}/{tier}',(xx,y+RNG.uniform(-3,3),8+tier*10+RNG.uniform(-1,3)),
                              (RNG.uniform(3,7.2),RNG.uniform(3,6.8),RNG.uniform(5,14)),air,
                              yaw=RNG.uniform(-.65,.65),light=col%2==0)
            else:
                kit(BACKGROUND,(xx,y,3+RNG.uniform(-1,3)),(14,14,23),far,math.radians(90*(col%4)),f'Tutorial backdrop row {row} column {col}')
    # Side/rear layers make it a place to turn in, rather than a stage backed by a picture.
    for i in range(42):
        a=RNG.uniform(math.pi,2*math.pi);r=RNG.uniform(47,165)
        x,y=r*math.cos(a),r*math.sin(a);z=RNG.uniform(4,16)
        if abs(x)<22 and y<-25:z=max(z,16)
        if r<135:
            lit_block(f'Rear floating depth {i}',(x,y,z),(RNG.uniform(4,8),RNG.uniform(4,8),RNG.uniform(8,23)),
                      air,RNG.uniform(-.7,.7),light=i%3!=1)
        else:
            kit(BACKGROUND,(x,y,z),(RNG.uniform(11,20),RNG.uniform(12,21),RNG.uniform(19,36)),
                far,RNG.uniform(-math.pi,math.pi),f'Rear tutorial depth {i}')
    for row,r in enumerate((38,62,96,145,218)):
        for angle in (-82,-68,-54,54,68,82):
            a=math.radians(angle+RNG.uniform(-6,6));rad=r+RNG.uniform(-7,9)
            if rad<170:
                lit_block(f'Side floating depth {row}/{angle}',(rad*math.sin(a),rad*math.cos(a),RNG.uniform(15,23)),
                          (RNG.uniform(4,7),RNG.uniform(4,8),RNG.uniform(9,23)),air,RNG.uniform(-.8,.8))
            else:
                kit(BACKGROUND,(rad*math.sin(a),rad*math.cos(a),RNG.uniform(3,12)),
                    (RNG.uniform(12,18),RNG.uniform(11,17),RNG.uniform(21,34)),far,
                    math.radians(angle+RNG.uniform(-35,35)),f'Side depth {row}/{angle}')
    # Full 1 km x 1 km extent. The outer field uses instances of the already cached tutorial background mesh.
    # It is split into 64 m cells in the manifest for the eventual native streamer; no kilometre-sized entity.
    for gx in range(-480,481,48):
        for gy in range(-480,481,48):
            if math.hypot(gx,gy)<160 or (abs(gx)<20 and gy>0):continue
            x,y=gx+RNG.uniform(-5,5),gy+RNG.uniform(-5,5)
            h=RNG.uniform(22,47);w=RNG.uniform(13,22)
            kit(BACKGROUND,(x,y,RNG.uniform(3,10)),(w,w,h),far,RNG.uniform(-math.pi,math.pi),f'Outer sector {gx}/{gy}')
    for i in range(30):
        a=2*math.pi*i/30+RNG.uniform(-.08,.08);r=RNG.uniform(15,31)
        kit(BLOCK,(r*math.cos(a),7+r*math.sin(a),RNG.uniform(18,35)),
            (RNG.uniform(3,5),RNG.uniform(3,5),RNG.uniform(12,28)),air,RNG.uniform(-.5,.5),'Upper tutorial spire')
    # A broken canopy, built with the same meshes. The sky remains open over the player's head.
    for i in range(24):
        if i in (3,8,9,15,21):continue
        a=2*math.pi*i/24
        kit(SLAB,(13.5*math.cos(a),8+13.5*math.sin(a),9.0),(3.6,7,.65),ceiling,a+math.pi/2,'Tutorial canopy segment')
    for name,pos,yaw,scale in [('West native moving group',(-24,46,18),18,1.8),
                               ('East native moving group',(33,53,22),-28,2.4),
                               ('Rear native moving group',(-46,-46,26),67,2.0)]:
        record=tutorial_animation.build_group(name,ROOT/'build/vr-lobby-20261027/vrtut_json/q000_vr_moving_block_01.ent.json',
                                              pos,math.radians(yaw),scale,motion,prototype)
        record['source_depot']=DEPOT+'q000_vr_moving_block_01.ent';ANIMATED.append(record)
    # Terminal proportions for the supplied panorama. Panel and controls remain separate editable objects.
    c=B.empty('Terminal assembly',(-1.75,3.3,0),props,math.radians(8))
    B.box('Terminal body',(0,0,.4),(1.25,.65,.8),metal,props,.018,c)
    B.box('Terminal top',(0,0,.84),(1.36,.74,.08),metal,props,.015,c)
    B.box('Terminal foot light',(0,-.337,.06),(1.16,.025,.025),AMBER,props,.003,c)
    B.cylinder('Light dial rim',(.35,-.35,.52),.105,.028,trim,props,c,(math.pi/2,0,0))
    B.cylinder('Light dial',(.35,-.38,.52),.085,.05,metal,props,c,(math.pi/2,0,0))
    B.text('Light label','СВЕТ',(.49,-.34,.49),.04,letter,props,c,font)
    screen=B.empty('Tilted terminal screen',(0,.05,1.54),props);screen.parent=c;screen.rotation_euler.x=-.18
    B.box('Screen casing',(0,0,0),(1.12,.07,1.30),metal,props,.018,screen)
    B.box('Screen glass',(0,-.04,0),(1.025,.009,1.2),glass,props,.005,screen)
    for x in (-.49,.49):B.box('Screen trim',(x,-.05,0),(.006,.004,1.14),trim,props,.001,screen)
    for z in (-.565,.565):B.box('Screen trim',(0,-.05,z),(.98,.004,.006),trim,props,.001,screen)
    panel_logo(screen,props)
    B.text('Buffer','Б У Ф Е Р',(0,-.055,.19),.044,letter,props,screen,font,'CENTER')
    for i,label in enumerate(['ПРОДОЛЖИТЬ','НОВАЯ ИГРА','ЗАГРУЗИТЬ ИГРУ','НАСТРОЙКИ','НАСТРОЙКИ VR','ВЫХОД']):
        z=.06-i*.092
        B.box('Menu row '+label,(0,-.051,z),(.86,.004,.078),glass,props,.002,screen)
        B.text(label,label,(-.39,-.057,z-.016),.035,letter,props,screen,font)
        if i==1:
            for zz in (z-.04,z+.04):B.box('Selection line',(0,-.06,zz),(.86,.003,.0025),AMBER,props,0,screen)
    # Low object table fills the foreground, with the top actually visible from the reference camera.
    t=B.empty('Table assembly',(-1.55,1.15,0),props,math.radians(24))
    B.box('Table base',(0,0,.27),(1.60,1.12,.54),metal,props,.035,t)
    tabletop=B.material('Table graphite surface',(.04,.045,.05),.4,.1)
    B.box('Table slab',(0,0,.59),(1.85,1.35,.12),tabletop,props,.025,t)
    B.box('Table light',(-.805,0,.33),(.025,.80,.04),AMBER,props,.004,t)
    B.box('Object tray',(.14,-.45,.675),(1.20,.30,.04),metal,props,.006,t)
    for yy in (-.61,-.29):B.box('Tray lip',(.14,yy,.71),(1.25,.035,.08),metal,props,.005,t)
    B.text('Reset objects','ВЕРНУТЬ ПРЕДМЕТЫ',(-.25,-.635,.68),.040,letter,props,t,font)
    B.box('Reset button',(-.40,-.64,.70),(.065,.018,.055),trim,props,.005,t)
    bpy.ops.mesh.primitive_uv_sphere_add(segments=40,ring_count=20,radius=.125)
    ball=B.move(bpy.context.object,props);ball.name='Black sphere';ball.parent=t;ball.location=(-.45,.14,.78);ball.data.materials.append(metal)
    for f in ball.data.polygons:f.use_smooth=True
    B.box('Amber cube',(-.02,.22,.76),(.20,.20,.21),trim,props,.006,t)
    B.cylinder('Can',(.36,.22,.80),.078,.30,metal,props,t)
    for z in (.656,.944):B.cylinder('Can rim',(.36,.22,z),.080,.012,trim,props,t)
    for x,y in ((.29,-.06),(.58,.02)):
        B.box('Data shard',(x,y,.664),(.17,.10,.02),metal,props,.004,t)
        B.box('Data shard contact',(x,y+.025,.676),(.12,.022,.002),trim,props,0,t)
    # Layered asymmetrical shell frame: no giant lintel intersecting the opening.
    # Original VR blocks form the outer mass; a thin metal liner defines the niche.
    pyaw=math.radians(-17);pc=Vector((4.0,5.0,0));rot=Matrix.Rotation(pyaw,3,'Z')
    def pk(label,at,size,lit=False):
        v=pc+rot@Vector(at);lit_block(label,tuple(v),size,pod,pyaw,lit)
    pk('Shell foundation',(0,.1,0),(2.5,1.5,.18),True)
    pk('Shell left lower mass',(-.98,.20,.18),(.65,1.4,2.15))
    pk('Shell left upper offset',(-1.02,.35,2.33),(.73,1.7,1.35))
    pk('Shell crown left',(-.42,.48,3.68),(1.90,1.35,.75))
    pk('Shell high rear block',(-.35,1.10,4.43),(2.70,1.60,1.30))
    pk('Shell crown right',(.82,.60,3.52),(.60,1.55,2.20))
    pk('Shell right column',(.96,.25,.18),(.65,1.5,3.34))
    pk('Shell name block',(1.08,-.40,1.70),(.77,.35,1.48))
    pk('Shell low side bench',(1.92,.55,.18),(1.60,1.75,.86),True)
    p=B.empty('Shell liner',pc,pod,pyaw)
    B.box('Niche rear',(0,.84,1.84),(1.28,.09,3.30),black,pod,.005,p)
    for x in (-.63,.63):
        B.box('Inner metal jamb',(x,.12,1.82),(.07,1.25,3.3),metal,pod,.006,p)
        B.box('Amber inner upright',(x,-.52,1.82),(.015,.012,3.28),AMBER,pod,.002,p)
    B.box('Inner lintel',(0,.12,3.48),(1.32,1.25,.07),metal,pod,.006,p)
    B.box('Upper internal light',(0,.2,3.43),(1.12,.06,.012),AMBER,pod,.002,p)
    B.box('Niche sill',(0,.05,.21),(1.32,1.35,.05),trim,pod,.009,p)
    B.text('Shell name','ОБОЛОЧКА',(.76,-.586,2.51),.066,letter,pod,p,font)
    B.text('Shell state','ОЖИДАНИЕ СВЯЗИ',(.76,-.586,2.32),.044,letter,pod,p,font)
    # Reuse the game's neutral mannequin, rigid and upright, with a contour preview shader.
    before=set(bpy.data.objects);path=next((ROOT/'build/lobby-blender/poses').rglob('mannequin_b_full_d.glb'));bpy.ops.import_scene.gltf(filepath=str(path))
    added=set(bpy.data.objects)-before;body=B.empty('Construct',(0,.15,.18),pod);body.parent=p;holo=B.hologram_material()
    for o in added:
        if o.type=='MESH':
            mesh=o.data.copy();mesh.transform(o.matrix_world);o.data=mesh;o.parent=body;o.location=(0,0,0);o.rotation_euler=(0,0,math.pi);o.scale=(1.56,)*3
            mesh.materials.clear();mesh.materials.append(holo)
            for face in mesh.polygons:face.material_index=0;face.use_smooth=True
            B.move(o,pod)
    for o in added:
        if o.type!='MESH':bpy.data.objects.remove(o,do_unlink=True)
    # Atmosphere grows with distance. The first metres stay crisp; the end of the geometry cannot be seen.
    B.world_setup();bpy.context.scene.world.node_tree.nodes.get('Background').inputs['Strength'].default_value=.75
    B.area('Sky',(0,20,44),100000,(.68,.82,1),42,(0,20,0),lighting)
    B.area('Far blue haze',(0,100,17),230000,(.20,.58,1),60,(0,10,10),lighting)
    B.area('East depth haze',(140,20,24),140000,(.22,.56,1),60,(18,10,6),lighting)
    B.area('West depth haze',(-145,8,19),120000,(.24,.58,1),55,(-15,3,4),lighting)
    B.area('Front soft fill',(-3,-12,12),3500,(.7,.82,1),15,(0,8,1),lighting)
    B.area('Rear atmospheric backlight',(-15,-95,24),150000,(.25,.58,1),50,(0,-12,6),lighting)
    B.area('Rear courtyard sky fill',(3,-18,18),6000,(.65,.80,1),22,(-4,-35,2),lighting)
    B.area('Rear terrace amber wash',(-8,-21,1.3),130,(1,.46,.12),2.5,(-8,-16,.1),lighting)
    B.area('Rear upper amber wash',(-13,-42,2.3),180,(1,.46,.12),3,(-13,-36,.8),lighting)
    B.area('Warm side bounce',(-15,10,6),3500,(1,.54,.2),12,(0,8,2),lighting)
    B.area('Shell downlight',(4.0,4.9,3.35),60,(1,.52,.15),.8,(4.0,5.2,1.2),lighting)
    for x,y in ((-5,8),(6,10),(-9,23),(10,25)):
        B.area('Amber ground wash',(x,y,.6),95,(1,.48,.13),2,(x,y-1,0),lighting)
    fog=bpy.data.materials.new('Distance blue haze');fog.use_nodes=True;n=fog.node_tree.nodes;n.clear();l=fog.node_tree.links
    output=n.new('ShaderNodeOutputMaterial');v=n.new('ShaderNodeVolumePrincipled');v.inputs['Color'].default_value=(.50,.70,1,1);v.inputs['Anisotropy'].default_value=.1
    geo=n.new('ShaderNodeNewGeometry');length=n.new('ShaderNodeVectorMath');length.operation='LENGTH';l.new(geo.outputs['Position'],length.inputs[0])
    fade=n.new('ShaderNodeMapRange');fade.inputs['From Min'].default_value=10;fade.inputs['From Max'].default_value=125
    fade.inputs['To Min'].default_value=.0007;fade.inputs['To Max'].default_value=.012;fade.clamp=True
    l.new(length.outputs['Value'],fade.inputs[0]);l.new(fade.outputs[0],v.inputs['Density']);l.new(v.outputs[0],output.inputs['Volume'])
    B.box('Fog volume',(0,0,70),(1040,1040,200),fog,lighting,0)
    # The reference is a wide panorama, so keep a true angular camera alongside the VR eye-height camera.
    hero=B.camera('Reference / 180 degree panorama',(0,0,1.2),(0,10,1.2),18,cameras)
    hero.data.type='PANO';hero.data.panorama_type='EQUIRECTANGULAR';hero.data.longitude_min=-math.pi/2;hero.data.longitude_max=math.pi/2
    hero.data.latitude_min=-math.pi/4;hero.data.latitude_max=math.pi/4
    eye=B.camera('VR / standing 1.7m',(0,0,1.7),(0,30,1.7),15,cameras)
    up=B.camera('VR / look up',(0,1,1.7),(0,1.05,50),16,cameras)
    route=B.camera('VR / approaching circle',(0,22,1.7),(0,48,2.5),21,cameras)
    staircase_detail=B.camera('Detail / integrated staircase',(10,34,6.3),(-1,46,2.2),26,cameras)
    parkour_detail=B.camera('Detail / irregular parkour',(4,-8,5.8),(16,15,3.0),21,cameras)
    circle_detail=B.camera('Detail / recessed circle',(8,18,5.3),(0,31,CIRCLE_LEVEL),24,cameras)
    rear_detail=B.camera('Detail / rear continuation',(8,-8,4.3),(-5,-43,3.8),21,cameras)
    east_detail=B.camera('Detail / east stair continuation',(14,16,4.8),(55,16,1.0),21,cameras)
    west_detail=B.camera('Detail / west low continuation',(-9,-1,2.1),(-95,1,1.0),18,cameras)
    carving_detail=B.camera('Detail / carved jump blocks',(-5.5,1.0,2.6),(-11.5,11,2.0),23,cameras)
    animation_detail=B.camera('Detail / native motion',(-29,25,19),(-25,44,16),24,cameras)
    directions=[]
    for angle in (0,90,180,270,360):
        a=math.radians(angle%360)
        cam=B.camera(f'View / {angle:03} degrees',(0,0,1.7),(30*math.sin(a),30*math.cos(a),1.7),18,cameras)
        directions.append((angle,cam))
    scene=bpy.context.scene;scene.camera=hero;scene.render.engine='CYCLES';scene.cycles.samples=64;scene.cycles.use_denoising=True
    scene.cycles.max_bounces=8;scene.cycles.transparent_max_bounces=12;scene.cycles.volume_bounces=1
    scene.render.resolution_x=1774;scene.render.resolution_y=887;scene.render.resolution_percentage=100
    scene.render.image_settings.file_format='PNG';scene.view_settings.view_transform='AgX';scene.view_settings.look='AgX - Medium High Contrast'
    scene.unit_settings.system='METRIC';scene['source']='Original q000 VR environment meshes with custom cast terrain and preview furniture; layout in native_manifest.json'
    scene['preview_limit']='Blender previews extracted multilayer masks; exact game materials are evaluated by REDengine.'
    pref=bpy.context.preferences.addons['cycles'].preferences;pref.compute_device_type='OPTIX';pref.get_devices()
    for d in pref.devices:d.use=d.type=='OPTIX'
    scene.cycles.device='GPU'
    scene.cycles.seed=2077
    common_period=math.lcm(*(int(t['period_seconds']) for g in ANIMATED for t in g['tracks']))
    scene.render.fps=24;scene.frame_start=1;scene.frame_end=24*common_period+1;scene.frame_set(1)
    # Portable font/image inputs and the source panorama for camera comparison.
    img=bpy.data.images.load(str(ROOT/'art/vr_lobby/references/panorama.png'));img.pack()
    for s in bpy.data.screens:
        for a in s.areas:
            if a.type=='VIEW_3D':a.spaces.active.region_3d.view_perspective='CAMERA';a.spaces.active.clip_end=800
    OUT.mkdir(parents=True,exist_ok=True)
    (OUT/'native_manifest.json').write_text(json.dumps({'version':1,'coordinate_system':'REDengine metres, Z up',
        'metrics':{'world_size_m':[WORLD_SIZE,WORLD_SIZE],'world_area_m2':WORLD_SIZE**2,
                   'viewpoint':[0,0,1.7],'circle_center':[0,CIRCLE_Y,CIRCLE_LEVEL],'circle_area_m2':CIRCLE_AREA,
                   'circle_radius_m':CIRCLE_RADIUS,'distance_to_circle_center_m':CIRCLE_Y,
                   'first_stairs_start_y_m':41,'first_stairs_top_y_m':46.2,'first_stairs_rise_m':1.2,
                   'circle_floor_level_m':CIRCLE_LEVEL,'microterrace_height_range_m':[.20,.30],
                   'recommended_native_anchor_lift_m':.35},
        'instances':MANIFEST,'custom_solids':CUSTOM,'parkour_routes':PARKOUR,
        'microterraces':MICRO_TERRACES,
        'ground_fill_groups':FILL_GROUPS,'animated_groups':ANIMATED,
        'animation_common_cycle_seconds':common_period,
        'floor_finish':{'style':'grey polished stone slabs','slab_size_m':3.0,'dark_joint_width_m':.018,
                        'native_striped_floor_removed':True},
        'continuations':{'forward':{'end_xy':[4,223],'stair_terraces':4},
                         'right':{'end_xy':[240,21],'stair_terraces':4},
                         'left':{'end_xy':[-248,5],'stair_terraces':3,'near_access':'low open passage'},
                         'rear':{'end_xy':[-12,-268],'stair_terraces':4}},
        'direction_views':{'origin':[0,0,1.7],'horizontal_fov_degrees':90,
                           'clockwise_from_forward':True,'angles':[0,90,180,270,360]}},indent=2),encoding='utf-8')
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'tutorial_lobby.blend'))
    print('NATIVE_INSTANCES',len(MANIFEST),flush=True)
    return [('panorama',hero,(1774,887)),('standing',eye,(1774,887)),('up',up,(1774,887)),
            ('route',route,(1774,887)),('staircase_detail',staircase_detail,(1600,900)),
            ('parkour_detail',parkour_detail,(1600,900)),('circle_detail',circle_detail,(1600,900)),
            ('rear_detail',rear_detail,(1600,900)),('east_detail',east_detail,(1600,900)),
            ('west_detail',west_detail,(1600,900)),('carving_detail',carving_detail,(1600,900)),
            ('animation_detail',animation_detail,(1600,900))]+[(f'view_{angle:03}',cam,(1600,900)) for angle,cam in directions]

if __name__=='__main__':
    cameras=build();s=bpy.context.scene
    preview='--preview' in sys.argv
    selected=set(sys.argv[sys.argv.index('--views')+1].split(',')) if '--views' in sys.argv else None
    if preview:s.cycles.samples=24;s.render.resolution_percentage=65
    for name,cam,resolution in cameras:
        if selected is not None and name not in selected:continue
        s.render.resolution_x,s.render.resolution_y=resolution
        s.camera=cam;s.render.filepath=str(OUT/(name+('.preview' if preview else '')+'.png'));bpy.ops.render.render(write_still=True)
    print('TUTORIAL_LOBBY_DONE',flush=True)
