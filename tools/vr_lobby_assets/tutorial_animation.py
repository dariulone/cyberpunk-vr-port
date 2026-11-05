"""Reconstruct q000's real TransformAnimator bindings and movement curves for Blender preview.

The native .ent path and its unmodified timeline parameters are retained for the REDengine authoring step.
No invented animation, generic sine bob or separate .anims/rig is needed for these entities.
"""
import bpy,json,math
from pathlib import Path
from mathutils import Vector,Quaternion

def build_group(name,entity_json,position,yaw,scale,col,prototype,fps=24):
    doc=json.loads(Path(entity_json).read_text(encoding='utf-8-sig'));root=doc['Data']['RootChunk'];handles={}
    def collect(v):
        if isinstance(v,dict):
            if 'HandleId' in v and 'Data' in v:handles[v['HandleId']]=v['Data']
            for x in v.values():collect(x)
        elif isinstance(v,list):
            for x in v:collect(x)
    collect(doc)
    def deref(v):
        if not v:return None
        return v.get('Data') or handles[v['HandleRefId']]
    def vector(v):return Vector(tuple(v[k] for k in ('X','Y','Z')))
    entity=bpy.data.objects.new(name,None);col.objects.link(entity);entity.location=position
    entity.rotation_euler.z=yaw;entity.scale=(scale,)*3
    components={};specs={};tracks=[];mesh_objects=[]
    for c in root['components']:
        key=c['name']['$value'];obj=bpy.data.objects.new(name+' / '+key,None);col.objects.link(obj)
        components[key]=obj;specs[key]=c
    for key,obj in components.items():
        c=specs[key];binding=deref(c.get('parentTransform'))
        obj.parent=components.get(binding['bindName']['$value'],entity) if binding else entity
        tr=c['localTransform'];p=tr['Position'];q=tr['Orientation']
        obj.location=tuple(p[k]['Bits']/131072 for k in ('x','y','z'))
        obj.rotation_mode='QUATERNION';obj.rotation_quaternion=Quaternion((q['r'],q['i'],q['j'],q['k']))
        if c['$type']=='entMeshComponent':
            path=c['mesh']['DepotPath']['$value'];asset=path.split('\\')[-1].removesuffix('.mesh')
            meshes,pivot,_=prototype(asset)
            vs=c.get('visualScale',{'X':1,'Y':1,'Z':1});obj.scale=tuple(vs[k] for k in ('X','Y','Z'))
            for i,mesh in enumerate(meshes):
                visual=bpy.data.objects.new(name+' / '+key+f' / mesh {i}',mesh);col.objects.link(visual)
                visual.parent=obj;visual.location=pivot
                visual['native_animated_mesh']=path;mesh_objects.append(visual.name)
        if c['$type']!='gameTransformAnimatorComponent':continue
        for animation in c['animations']:
            if not animation['autoStart']:continue
            segments=[]
            for item in animation['timeline']['items']:
                move=deref(item['impl']);assert move['$type']=='gameTransformAnimation_Move'
                movement=deref(move['movement'])['function']
                assert movement['easingType']=='EET_InOut' and movement['transitionType']=='EET_Cubic'
                start=deref(move['startPositionEvaluator']);target=deref(move['targetPositionEvaluator'])
                assert not start['offsetInWorldSpace'] and not target['offsetInWorldSpace']
                segments.append({'start':item['startTime'],'duration':item['duration'],
                                 'from':list(vector(start['offset'])),'to':list(vector(target['offset']))})
            period=max(s['start']+s['duration'] for s in segments);initial=obj.location.copy()
            def offset(t):
                t=t%period
                for s in segments:
                    if s['start']<=t<=s['start']+s['duration']:
                        u=(t-s['start'])/s['duration'];e=4*u*u*u if u<.5 else 1-(-2*u+2)**3/2
                        return Vector(s['from']).lerp(Vector(s['to']),e)
                raise ValueError('Unsupported gap in native movement timeline')
            frames=set(range(0,int(round(period*fps))+1,4));frames.add(int(round(period*fps)))
            for s in segments:frames.update((int(round(s['start']*fps)),int(round((s['start']+s['duration'])*fps))))
            for f in sorted(frames):
                obj.location=initial+offset(f/fps);obj.keyframe_insert(data_path='location',frame=f+1)
            action=obj.animation_data.action
            curves=action.fcurves if hasattr(action,'fcurves') else action.layers[0].strips[0].channelbag(obj.animation_data.action_slot).fcurves
            for curve in curves:
                for k in curve.keyframe_points:k.interpolation='LINEAR'
                if animation['looping']:curve.modifiers.new('CYCLES')
            obj.location=initial+offset(0)
            obj['native_animation_period_seconds']=period
            tracks.append({'object':obj.name,'native_component':key,'period_seconds':period,
                           'initial_position':list(initial),'easing':'Cubic InOut','looping':bool(animation['looping']),
                           'segments':segments})
    entity['native_entity_template']=Path(entity_json).name.removesuffix('.json')
    return {'name':name,'source_entity':Path(entity_json).name.removesuffix('.json'),
            'position':list(position),'yaw_degrees':math.degrees(yaw),'uniform_scale':scale,
            'scaled_entity_copy_required':scale!=1,'tracks':tracks,'mesh_objects':mesh_objects,
            'native_runtime_verified':False,'preview_fps':fps}
