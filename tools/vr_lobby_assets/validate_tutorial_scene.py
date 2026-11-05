"""Run inside Blender with tutorial_lobby.blend open: solid topology and fixed-point view validation."""
import bpy,bmesh,json,math
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'art/vr_lobby/tutorial'
doc=json.loads((OUT/'native_manifest.json').read_text())
assert doc['metrics']['world_area_m2']==1_000_000
assert doc['metrics']['distance_to_circle_center_m']==30
assert abs(math.pi*doc['metrics']['circle_radius_m']**2-40)<1e-6
results=[]
for item in doc['custom_solids']:
    obj=bpy.data.objects[item['name']]
    bm=bmesh.new();bm.from_mesh(obj.data)
    bad=sum(not edge.is_manifold for edge in bm.edges);volume=bm.calc_volume(signed=False)
    assert bad==0,(item['name'],bad)
    assert volume>0,item['name']
    # A cutout may create a hole, but must never separate a roof from its supports.
    remaining=set(bm.verts);parts=0
    while remaining:
        parts+=1;stack=[remaining.pop()]
        while stack:
            vert=stack.pop()
            for edge in vert.link_edges:
                other=edge.other_vert(vert)
                if other in remaining:remaining.remove(other);stack.append(other)
    assert parts==1,(item['name'],'disconnected pieces',parts)
    results.append({'name':item['name'],'type':item['type'],'closed':True,'local_volume_m3':volume})
    bm.free()
names=[v['name'] for v in doc['custom_solids']];assert len(names)==len(set(names))
terraces=[v for v in doc['custom_solids'] if v['type']=='inset_terrace']
assert len(terraces)>=8 and all(v['integrated_stairs'] for v in terraces)
assert not any(v['type']=='cast_stairs' for v in doc['custom_solids'])
assert all(v['stair_run_m']<v['size'][1] for v in terraces)
assert all(v['stair_width_m']<v['size'][0] for v in terraces)
assert len([v for v in terraces if v['position'][1]<0])>=2
ts=[v for v in doc['custom_solids'] if v['type']=='inverted_t']
assert len({round(v['height_m'],3) for v in ts})>=12
positions={(round(v['position'][0],2),round(v['position'][1],2)) for v in ts}
assert not any((-x,y) in positions for x,y in positions if x!=0),'Mirrored T placements'
assert len([v for v in ts if v['position'][1]<0])>=6
assert len(doc['microterraces'])>=7
assert all(.20<=abs(v['top_height_m'])<=.30 for v in doc['microterraces'])
assert len({v['top_height_m'] for v in doc['microterraces']})>=5
assert doc['metrics']['circle_floor_level_m']==-.24
assert set(doc['continuations'])=={'forward','right','left','rear'}
assert all(v['stair_terraces']>=3 for v in doc['continuations'].values())
deps=bpy.context.evaluated_depsgraph_get()
def floor_at(x,y):
    hit,p,_,_,obj,_=bpy.context.scene.ray_cast(deps,Vector((x,y,.6)),Vector((0,0,-1)),distance=2)
    assert hit,(x,y)
    return p.z,obj.name
floor_samples={}
for key,x,y,expected in [('basin',1.5,30,-.24),('outer floor',0,20,.01),('west raised shelf',-16.5,27,.27),
                       ('front descent',0,22.85,-.115),('back ascent',0,38.6,-.115)]:
    z,name=floor_at(x,y);assert abs(z-expected)<.012,(key,z,name)
    floor_samples[key]={'z':z,'object':name}
for r in doc['parkour_routes']:
    assert max(j['edge_gap_m'] for j in r['jumps'])<1
    assert max(j['rise_m'] for j in r['jumps'])<=.75
assert doc['floor_finish']['native_striped_floor_removed']
assert not any('vr_architecture_floor_' in v['mesh'] for v in doc['instances'])
assert bpy.data.materials['Grey stone floor / reference slabs']['floor_finish']
assert sum(len(g['blocks']) for g in doc['ground_fill_groups'])>=60
cut_tests=[]
for item in (v for v in doc['custom_solids'] if v['type']=='parkour_block'):
    obj=bpy.data.objects[item['name']];tree=BVHTree.FromObject(obj,deps)
    w,d,h=item['size'];zmin=min(v.co.z for v in obj.data.vertices);zmax=max(v.co.z for v in obj.data.vertices)
    for x,y in [(0,0),(-.42*w,-.42*d),(.42*w,-.42*d),(.42*w,.42*d),(-.42*w,.42*d)]:
        low=tree.ray_cast(Vector((x,y,zmin-.1)),Vector((0,0,1)),h+1)[0]
        high=tree.ray_cast(Vector((x,y,zmax+.1)),Vector((0,0,-1)),h+1)[0]
        assert low is not None and abs(low.z-zmin)<.003,(item['name'],'base missing',x,y)
        assert high is not None and abs(high.z-zmax)<.003,(item['name'],'landing missing',x,y)
    assert item['cutouts'] and item['solid_base_thickness_m']>=.16
    cut_tests.append({'name':item['name'],'style':item['cutout_style'],'full_base_and_landing_samples':5})
animation_tests=[]
for group in doc['animated_groups']:
    for track in group['tracks']:
        obj=bpy.data.objects[track['object']];period=track['period_seconds'];fps=group['preview_fps']
        initial=Vector(track['initial_position']);target=initial+Vector(track['segments'][0]['to'])
        for t,expected in [(0,initial),(period/2,target),(period,initial),(period*1.5,target)]:
            bpy.context.scene.frame_set(round(t*fps)+1)
            assert (obj.location-expected).length<.002,(obj.name,t,tuple(obj.location),tuple(expected))
        animation_tests.append({'object':obj.name,'period_s':period,'loop_checked':True})
bpy.context.scene.frame_set(1)
assert doc['animation_common_cycle_seconds']==1440
views=[]
for angle in (0,90,180,270,360):
    cam=bpy.data.objects[f'View / {angle:03} degrees']
    assert (cam.location-bpy.data.objects['View / 000 degrees'].location).length<1e-6
    assert abs(cam.data.angle_x-math.pi/2)<1e-6
    views.append({'degrees':angle,'position':list(cam.location),'quaternion':list(cam.rotation_euler.to_quaternion())})
a=bpy.data.objects['View / 000 degrees'].matrix_world
b=bpy.data.objects['View / 360 degrees'].matrix_world
assert all(abs(a[i][j]-b[i][j])<1e-7 for i in range(4) for j in range(4))
report={'solid_count':len(results),'solids':results,'views':views,'same_pose_0_360':True,
        'integrated_stair_terraces':len(terraces),'no_mirrored_t_placements':True,
        'microterrace_heights_m':sorted({v['top_height_m'] for v in doc['microterraces']}),
        'depression_ray_checks':floor_samples,'four_direction_continuations':True,
        'carved_block_support_tests':cut_tests,'native_animation_tests':animation_tests,
        'stone_floor_everywhere':True,'added_ground_blocks':sum(len(g['blocks']) for g in doc['ground_fill_groups']),
        'runtime_parkour_tested':False}
(OUT/'geometry_validation.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print('VALIDATED',len(results),'closed solids; five fixed-position 90-degree views; 0/360 same pose')
