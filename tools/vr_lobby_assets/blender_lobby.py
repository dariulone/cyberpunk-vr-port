"""Build the reference lobby as an editable, metre-scale Blender scene.

blender --background --python tools/vr_lobby_assets/blender_lobby.py -- --preview
blender --background --python tools/vr_lobby_assets/blender_lobby.py -- --render

No panorama is projected onto the environment. Architecture, furniture, trim and UI lettering are geometry.
Reference pictures are packed into a disabled reference collection for comparison.
"""
import argparse
import json
import math
from pathlib import Path
import random
import sys

import bpy
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'art/vr_lobby'
RNG=random.Random(20771027)
TAU=2*math.pi


def collection(name):
    c=bpy.data.collections.new(name);bpy.context.scene.collection.children.link(c);return c


def move(obj,col):
    for c in list(obj.users_collection):c.objects.unlink(obj)
    col.objects.link(obj)
    return obj


def material(name,color,rough=.5,metal=0,emission=0):
    m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
    p=m.node_tree.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value=(*color,1)
    p.inputs['Roughness'].default_value=rough;p.inputs['Metallic'].default_value=metal
    p.inputs['Emission Color'].default_value=(*color,1);p.inputs['Emission Strength'].default_value=emission
    return m


def stone(name,base,rough=.5,polish=False):
    m=material(name,base,rough,.16 if polish else .04)
    n=m.node_tree.nodes;l=m.node_tree.links;p=n.get('Principled BSDF')
    tex=n.new('ShaderNodeTexCoord')
    noise=n.new('ShaderNodeTexNoise');noise.inputs['Scale'].default_value=2.9;noise.inputs['Detail'].default_value=5.0
    l.new(tex.outputs['Position'],noise.inputs['Vector']) if 'Position' in tex.outputs else l.new(tex.outputs['Object'],noise.inputs['Vector'])
    ramp=n.new('ShaderNodeValToRGB')
    ramp.color_ramp.elements[0].position=.18;ramp.color_ramp.elements[0].color=(*[x*.76 for x in base],1)
    ramp.color_ramp.elements[1].position=.82;ramp.color_ramp.elements[1].color=(*[x*1.22 for x in base],1)
    l.new(noise.outputs['Fac'],ramp.inputs[0]);l.new(ramp.outputs[0],p.inputs['Base Color'])
    fine=n.new('ShaderNodeTexNoise');fine.inputs['Scale'].default_value=46;fine.inputs['Detail'].default_value=3
    l.new(tex.outputs['Object'],fine.inputs['Vector'])
    bump=n.new('ShaderNodeBump');bump.inputs['Strength'].default_value=.18 if polish else .25
    bump.inputs['Distance'].default_value=.011 if polish else .022
    l.new(fine.outputs['Fac'],bump.inputs['Height']);l.new(bump.outputs['Normal'],p.inputs['Normal'])
    rr=n.new('ShaderNodeMapRange');rr.inputs['From Min'].default_value=0;rr.inputs['From Max'].default_value=1
    rr.inputs['To Min'].default_value=rough-.09;rr.inputs['To Max'].default_value=rough+.1
    l.new(noise.outputs['Fac'],rr.inputs[0]);l.new(rr.outputs[0],p.inputs['Roughness'])
    # Use the game's authored concrete at a metre scale, instead of blurry procedural marble.
    texture_root=ROOT/'build/vr-lobby-20261027/concrete_png/surfaces/materials/concrete'
    diffuse=texture_root/('concrete_smooth/concrete_smooth_01_300_d.png' if polish else 'concrete_new/concrete_new_clean_01_300_d.png')
    if diffuse.exists():
        img=bpy.data.images.load(str(diffuse),check_existing=True);img.pack()
        im=n.new('ShaderNodeTexImage');im.image=img;im.projection='BOX';im.projection_blend=.1
        sc=n.new('ShaderNodeVectorMath');sc.operation='SCALE';sc.inputs[3].default_value=1/3
        l.new(tex.outputs['Object'],sc.inputs[0]);l.new(sc.outputs[0],im.inputs['Vector'])
        mul=n.new('ShaderNodeMixRGB');mul.blend_type='MULTIPLY';mul.inputs[0].default_value=1
        mul.inputs[2].default_value=(*[v*2.0 for v in base],1);l.new(im.outputs['Color'],mul.inputs[1])
        l.new(mul.outputs[0],p.inputs['Base Color']);l.new(im.outputs['Color'],bump.inputs['Height'])
        bump.inputs['Distance'].default_value=.018 if polish else .025
    return m


def box(name,loc,size,mat,col,bevel=.025,parent=None,rotation=None):
    hx,hy,hz=(s/2 for s in size)
    verts=[(-hx,-hy,-hz),(hx,-hy,-hz),(hx,hy,-hz),(-hx,hy,-hz),
           (-hx,-hy,hz),(hx,-hy,hz),(hx,hy,hz),(-hx,hy,hz)]
    faces=[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]
    mesh=bpy.data.meshes.new(name);mesh.from_pydata(verts,[],faces)
    o=bpy.data.objects.new(name,mesh);col.objects.link(o);o.location=loc
    if rotation:o.rotation_euler=rotation
    if mat:o.data.materials.append(mat)
    if bevel:
        mod=o.modifiers.new('Machined edges','BEVEL');mod.width=bevel;mod.segments=2
        mod=o.modifiers.new('Weighted corner normals','WEIGHTED_NORMAL')
    if parent:o.parent=parent;o.location=loc
    o['units']='metres';return o


def empty(name,loc,col,yaw=0):
    o=bpy.data.objects.new(name,None);col.objects.link(o);o.location=loc;o.rotation_euler.z=yaw;return o


def cylinder(name,loc,radius,depth,mat,col,parent=None,rotation=None,vertices=64):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=radius,depth=depth,location=loc)
    o=bpy.context.object;o.name=name;o.data.materials.append(mat)
    if rotation:o.rotation_euler=rotation
    if parent:o.parent=parent;o.location=loc
    mod=o.modifiers.new('Soft rim','BEVEL');mod.width=.008;mod.segments=3
    o.modifiers.new('Weighted normals','WEIGHTED_NORMAL');move(o,col);return o


def line(name,points,radius,mat,col,parent=None):
    curve=bpy.data.curves.new(name,'CURVE');curve.dimensions='3D';curve.resolution_u=1
    curve.bevel_depth=radius;curve.bevel_resolution=2
    spline=curve.splines.new('POLY');spline.points.add(len(points)-1)
    for p,v in zip(spline.points,points):p.co=(*v,1)
    obj=bpy.data.objects.new(name,curve);col.objects.link(obj);curve.materials.append(mat)
    if parent:obj.parent=parent
    return obj


def ring(name,center,radius,thickness,mat,col,z=0,start=0,end=TAU,parent=None):
    steps=max(12,int((end-start)*radius*10))
    return line(name,[(center[0]+radius*math.cos(start+(end-start)*i/steps),
                       center[1]+radius*math.sin(start+(end-start)*i/steps),z) for i in range(steps+1)],thickness,mat,col,parent)


def text(name,body,loc,size,mat,col,parent=None,font=None,align='LEFT'):
    curve=bpy.data.curves.new(name,'FONT');curve.body=body;curve.size=size;curve.align_x=align
    curve.extrude=.0002;curve.resolution_u=5
    if font:curve.font=font
    obj=bpy.data.objects.new(name,curve);col.objects.link(obj);obj.location=loc
    obj.rotation_euler=(math.pi/2,0,0);curve.materials.append(mat)
    if parent:obj.parent=parent
    return obj


def area(name,pos,power,color,size,target,col,shape='DISK',size_y=None):
    d=bpy.data.lights.new(name,'AREA');d.energy=power;d.color=color;d.shape=shape;d.size=size
    if size_y is not None:d.size_y=size_y
    o=bpy.data.objects.new(name,d);col.objects.link(o);o.location=pos
    o.rotation_euler=(Vector(target)-o.location).to_track_quat('-Z','Y').to_euler();return o


def strip(name,pos,length,col,parent=None,axis='X',power=0):
    size=(length,.022,.024) if axis=='X' else (.022,length,.024)
    return box(name,pos,size,AMBER,col,.005,parent)


def monolith(name,x,y,z,w,d,h,col,lit=True,yaw=0):
    o=box(name,(x,y,z+h/2),(w,d,h),CONCRETE,col,.035,rotation=(0,0,yaw))
    o['game_role']='static_architecture'
    if lit:
        p=empty(name+' / trim',(x,y,z),col,yaw)
        box(name+' / recessed channel',(0,-d/2-.008,.07),(w*.9,.03,.065),METAL,col,.006,p)
        strip(name+' / amber seam',(0,-d/2-.03,.065),w*.88,col,p)
        if w>4 and RNG.random()<.55:
            box(name+' / vertical inlay',(-w*.32,-d/2-.012,h*.43),(.027,.025,h*.75),AMBER,col,.005,p)
    return o


def steps(name,x,y,width,count,rise,run,col,yaw=0,base=0):
    p=empty(name,(x,y,base),col,yaw)
    for k in range(count):
        h=(k+1)*rise
        box(name+f' {k+1:02}',(0,(k+.5)*run,h/2),(width,run-.012,h),CONCRETE,col,.02,p)
        if k in (0,count-1):strip(name+' / tread light',(0,k*run+.035,h-.015),width*.94,col,p)


def camera(name,pos,target,lens,col):
    d=bpy.data.cameras.new(name);o=bpy.data.objects.new(name,d);col.objects.link(o);o.location=pos
    o.rotation_euler=(Vector(target)-o.location).to_track_quat('-Z','Y').to_euler()
    d.lens=lens;d.clip_start=.03;d.clip_end=800;return o


def world_setup():
    world=bpy.data.worlds.new('Blue cloud chamber');world.use_nodes=True;bpy.context.scene.world=world
    n=world.node_tree.nodes;l=world.node_tree.links;n.clear()
    out=n.new('ShaderNodeOutputWorld');bg=n.new('ShaderNodeBackground');bg.inputs['Strength'].default_value=.4
    tex=n.new('ShaderNodeTexCoord');sep=n.new('ShaderNodeSeparateXYZ');l.new(tex.outputs['Normal'],sep.inputs[0])
    ramp=n.new('ShaderNodeValToRGB');ramp.color_ramp.elements[0].position=0
    ramp.color_ramp.elements[0].color=(.18,.42,.75,1);ramp.color_ramp.elements[1].position=1
    ramp.color_ramp.elements[1].color=(.025,.18,.55,1)
    flip=n.new('ShaderNodeMath');flip.operation='ABSOLUTE';l.new(sep.outputs['Z'],flip.inputs[0]);l.new(flip.outputs[0],ramp.inputs[0])
    scale=n.new('ShaderNodeVectorMath');scale.operation='MULTIPLY';scale.inputs[1].default_value=(1.8,1.8,3.2)
    l.new(tex.outputs['Normal'],scale.inputs[0])
    clouds=n.new('ShaderNodeTexNoise');clouds.inputs['Scale'].default_value=3.1;clouds.inputs['Detail'].default_value=5
    clouds.inputs['Roughness'].default_value=.68;l.new(scale.outputs[0],clouds.inputs['Vector'])
    cr=n.new('ShaderNodeValToRGB');cr.color_ramp.elements[0].position=.46;cr.color_ramp.elements[1].position=.69
    l.new(clouds.outputs['Fac'],cr.inputs[0]);mix=n.new('ShaderNodeMixRGB');mix.blend_type='MIX'
    l.new(cr.outputs[0],mix.inputs[0]);l.new(ramp.outputs[0],mix.inputs[1]);mix.inputs[2].default_value=(.45,.68,1.0,1)
    l.new(mix.outputs[0],bg.inputs[0]);l.new(bg.outputs[0],out.inputs['Surface'])


def hologram_material():
    m=bpy.data.materials.new('Amber construct / contour and mesh');m.use_nodes=True
    n=m.node_tree.nodes;l=m.node_tree.links;n.clear()
    out=n.new('ShaderNodeOutputMaterial');trans=n.new('ShaderNodeBsdfTransparent')
    glow=n.new('ShaderNodeEmission');glow.inputs[0].default_value=(1,.38,.055,1);glow.inputs[1].default_value=1.25
    facing=n.new('ShaderNodeLayerWeight');facing.inputs['Blend'].default_value=.35
    ramp=n.new('ShaderNodeValToRGB');ramp.color_ramp.elements[0].color=(.018,.018,.018,1)
    ramp.color_ramp.elements[0].position=.12;ramp.color_ramp.elements[1].position=.94
    ramp.color_ramp.elements[1].color=(.9,.9,.9,1)
    l.new(facing.outputs['Facing'],ramp.inputs[0])
    wire=n.new('ShaderNodeWireframe');wire.use_pixel_size=False;wire.inputs[0].default_value=.0003
    add=n.new('ShaderNodeMath');add.operation='ADD';l.new(ramp.outputs[0],add.inputs[0]);l.new(wire.outputs[0],add.inputs[1])
    mix=n.new('ShaderNodeMixShader');l.new(add.outputs[0],mix.inputs[0]);l.new(trans.outputs[0],mix.inputs[1]);l.new(glow.outputs[0],mix.inputs[2])
    l.new(mix.outputs[0],out.inputs['Surface']);return m


def build():
    global CONCRETE,METAL,AMBER
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    for c in list(bpy.data.collections):
        if c.name=='Collection':bpy.data.collections.remove(c)
    floor=collection('01 / Plaza and inlays');platforms=collection('02 / Terraces and stairs')
    city=collection('03 / Grounded monoliths');floating=collection('04 / Floating monoliths')
    ceiling=collection('05 / Broken ceiling ring');console=collection('06 / Menu terminal')
    desk=collection('07 / Object table');pod=collection('08 / Shell alcove');props=collection('09 / Table objects')
    lighting=collection('10 / Atmosphere and lighting');cameras=collection('11 / Cameras');refs=collection('12 / References (not rendered)')
    CONCRETE=stone('Architectural concrete / cool grey',(.085,.095,.11),.50)
    DARK=stone('Basalt cabinet / dark grey',(.036,.046,.057),.40)
    FLOOR=stone('Polished stone / floor',(.055,.065,.078),.27,True)
    METAL=material('Graphite anodized metal',(.018,.023,.03),.3,.74)
    GOLD=material('Brushed champagne trim',(.38,.23,.075),.26,.82)
    AMBER=material('Amber light / steady', (1,.47,.105),.35,.1,6)
    LETTER=material('Warm white lettering',(.73,.72,.64),.5,0,.65)
    DIM=material('Disabled lettering',(.12,.15,.16),.7,0,.15)
    SCREEN=material('Terminal glass',(.002,.003,.004),.35,0)
    SCREEN.node_tree.nodes.get('Principled BSDF').inputs['Specular IOR Level'].default_value=.08
    RED=material('Selected menu row',(.055,.009,.005),.4,0,.5)
    BLACK=material('Stone joints',(.006,.01,.014),.64)
    font=bpy.data.fonts.load('C:/Windows/Fonts/ARIALN.TTF')
    bold=bpy.data.fonts.load('C:/Windows/Fonts/ARIALNB.TTF')
    italic=bpy.data.fonts.load('C:/Windows/Fonts/arialbi.ttf')
    # Ground and separated large flagstones.
    box('Substrate',(0,12,-.22),(320,320,.4),DARK,floor,0)
    for ix in range(-12,13):
        for iy in range(-8,19):
            o=box(f'Slab {ix:+03} {iy:+03}',(ix*2.4,iy*2.4,-.035),(2.382,2.382,.07),FLOOR,floor,.008)
            o['game_collision']='floor'
    for r,w in [(1.1,.014),(3.0,.008),(3.07,.006),(6.6,.006)]:ring('Circular inlay',(0,6.8),r,w,GOLD if r==3.07 else BLACK,floor,.006)
    cylinder('Centre inset',(0,6.8,.005),1.07,.018,DARK,floor,vertices=96)
    # Raised square islands. The breaks between them form the four walkable stair approaches.
    for x,y,w,d,h in [(-9,8,9,10,.62),(9,8,9,10,.62),(-9,21,10,12,1.2),(9,21,10,12,1.2),(0,28,8,12,1.2),(-13,-6,8,8,.58),(13,-5,8,10,.58)]:
        monolith('Terrace',x,y,0,w,d,h,platforms)
        box('Terrace cap',(x,y,h+.045),(w+.14,d+.14,.09),CONCRETE,platforms,.025)
    steps('Central approach',0,14,5.4,6,.2,.43,platforms)
    steps('West approach',-4.4,5.2,3.8,4,.155,.4,platforms,math.pi/2)
    steps('East approach',4.4,8,3.8,4,.155,.4,platforms,-math.pi/2)
    steps('Rear approach',0,-8,4.8,4,.155,.4,platforms,math.pi)
    # Manual near composition: low layered islands, clear central sightline, NO wall of full-height pillars.
    anchors=[(-8,7,3.2,2.8,2.4),(-13,17,3.5,3.7,3.6),(10.5,12,3.3,3.5,2.6),
             (-7,29,3.0,3.4,3.2),(9,28,3.2,3.7,3.6),(-20,6,3.3,4.7,3.4),
             (20,7,3,4.2,3.8),(-19,31,3.4,4,4),(19,32,3.2,3.3,4.0)]
    for i,(x,y,w,d,h) in enumerate(anchors):
        monolith(f'Courtyard tower {i:02}',x,y,.65,w,d,h,city)
        if i%2==0:monolith(f'Courtyard offset {i:02}',x+.7,y+.2,h+.65,w*.78,d*.7,2.0,city)
        monolith(f'Low block {i:02}',x+(-2.3 if x<0 else 2.3),y-1,.65,2.4,2.5,.7,city)
    # Repeat into the distance: each layer remains separated by air. Far rows become silhouettes in fog.
    # Shared mesh data keeps the authored infinity inexpensive to navigate in Blender.
    for row,y in enumerate((42,60,83,111,146,188,238,298,368)):
        for col,x in enumerate(range(-90,91,12)):
            h=RNG.uniform(1.5,4.8)
            if abs(x)<8:continue
            monolith(f'Distant base {row:02} {col:02}',x+RNG.uniform(-1,1),y,0,RNG.uniform(3,6),RNG.uniform(3,6),h,city,lit=row<4)
    # Floating near/mid blocks are kept over the circulation area, never at head height.
    fixed=[(-8,9,5.6,3.2,3.0,4.8),(-4.4,17,8.2,3.0,3.2,4.2),(3.0,24,6.0,3.3,3.4,3.5),
           (9,11,6.5,3.1,3.8,4.8),(-12,19,10.5,3.8,3.1,5.0),(13,23,8,4,3.6,5),
           (-1,31,10,3.5,4,4.5),(-19,3,9,4,5,6),(17,5,10,4,5,7)]
    for i,(x,y,z,w,d,h) in enumerate(fixed):monolith(f'Floating hero {i:02}',x,y,z,w,d,h,floating)
    for row,y in enumerate((24,39,57,78,103,134,172,217,270,332,405)):
        for col,x in enumerate(range(-90,91,12)):
            for tier in range(3):
                xx=x+RNG.uniform(-2.0,2.0); yy=y+RNG.uniform(-2,2)
                z=4.5+tier*11+RNG.uniform(-1,2)
                if row==0 and abs(xx)<4 and tier==0:continue
                monolith(f'Infinity row {row:02} column {col:02} tier {tier}',xx,yy,z,
                         RNG.uniform(3.0,5.8),RNG.uniform(3,6),RNG.uniform(3.2,7.0),floating,lit=row<6 and (col+tier)%3!=0)
    for i in range(75):
        a=RNG.uniform(math.pi,TAU);r=RNG.uniform(20,90)
        monolith(f'Behind viewer {i:03}',r*math.cos(a),r*math.sin(a),RNG.uniform(6,30),3.6,4.0,RNG.uniform(4,12),floating,lit=i%3==0)
    # Tall forms converge toward an open blue skylight from the upward camera.
    for i in range(26):
        a=i*TAU/26; r=RNG.uniform(14.5,26)
        monolith(f'Upper spire {i:02}',r*math.cos(a),7+r*math.sin(a),RNG.uniform(29,46),RNG.uniform(2.5,5),RNG.uniform(3,5.5),RNG.uniform(12,32),floating,lit=i%4==0)
    # Actual curved wedges, with a lower bevel, rather than a flat rectangular ceiling.
    for k in range(36):
        if k in (1,7,8,16,23,24,31):continue
        a0=k*TAU/36+.007;a1=(k+1)*TAU/36-.007
        verts=[]
        for z in (10.8,12.0):
            for r in (13.5,23.0):
                for j in range(5):
                    a=a0+(a1-a0)*j/4;verts.append((r*math.cos(a),7+r*math.sin(a),z))
        faces=[]
        for j in range(4):
            faces.extend([(j,j+5,j+6,j+1),(10+j,11+j,16+j,15+j),(j,j+1,11+j,10+j),(5+j,15+j,16+j,6+j)])
        faces.extend([(0,10,15,5),(4,9,19,14)])
        mesh=bpy.data.meshes.new('Ceiling wedge');mesh.from_pydata(verts,[],faces);mesh.materials.append(CONCRETE)
        obj=bpy.data.objects.new(f'Ceiling arc {k:02}',mesh);ceiling.objects.link(obj)
        mod=obj.modifiers.new('Stone edge bevel','BEVEL');mod.width=.025;mod.segments=2
        if k%3==0:ring('Ceiling underside light',(0,7),14,.026,AMBER,ceiling,10.78,a0+.02,a1-.02)
    # Terminal: local -Y is its face. All screen items are on a genuinely tilted assembly.
    c=empty('TERMINAL / move as one',(-2.1,.3,0),console,math.radians(12))
    box('Terminal plinth',(0,0,.075),(1.75,1.0,.15),METAL,console,.03,c)
    box('Terminal cabinet',(0,0,.53),(1.58,.92,.9),DARK,console,.025,c)
    box('Terminal top',(0,0,1.0),(1.70,1.04,.10),METAL,console,.022,c)
    strip('Terminal toe light',(0,-.472,.16),1.47,console,c)
    for x in (-.65,.65):box('Cabinet vertical seam',(x,-.464,.57),(.012,.008,.72),BLACK,console,0,c)
    cylinder('Light control rim',(.47,-.492,.65),.145,.035,GOLD,console,c,(math.pi/2,0,0))
    cylinder('Light control dial',(.47,-.52,.65),.115,.052,METAL,console,c,(math.pi/2,0,0))
    text('Light label','СВЕТ',(.67,-.47,.61),.058,LETTER,console,c,font)
    stand=box('Screen support',(0,.22,1.15),(.35,.18,.26),METAL,console,.02,c)
    screen=empty('SCREEN / 12 degree recline',(0,.04,1.95),console);screen.parent=c
    screen.rotation_euler.x=math.radians(-12)
    screen.scale=(1.22,1,1.25)
    box('Screen outer frame',(0,0,0),(1.34,.09,1.35),METAL,console,.025,screen)
    box('Screen inset',(0,-.051,0),(1.23,.012,1.23),SCREEN,console,.014,screen)
    for x in (-.59,.59):box('Screen gold side rail',(x,-.061,0),(.009,.004,1.16),GOLD,console,.001,screen)
    for z in (-.58,.58):box('Screen gold end rail',(0,-.061,z),(1.18,.004,.009),GOLD,console,.001,screen)
    logo=text('Cyberpunk logo','Cyberpunk',(0,-.065,.365),.155,AMBER,console,screen,italic,'CENTER');logo.data.shear=.16
    text('2077','2 0 7 7',(0,-.065,.288),.044,LETTER,console,screen,font,'CENTER')
    text('Buffer heading','Б У Ф Е Р',(0,-.065,.165),.052,LETTER,console,screen,font,'CENTER')
    buttons=['ПРОДОЛЖИТЬ','НОВАЯ ИГРА','ЗАГРУЗИТЬ ИГРУ','НАСТРОЙКИ','НАСТРОЙКИ VR','ВЫХОД']
    for i,label in enumerate(buttons):
        z=.045-i*.093
        box(f'Button {i+1:02} / {label}',(0,-.065,z),(.98,.004,.081),RED if i==1 else SCREEN,console,.005,screen)
        text('Button label / '+label,label,(-.43,-.069,z-.018),.044,DIM if i==0 else LETTER,console,screen,font)
        if i==1:
            for zz in (z-.042,z+.042):box('Active row rim',(0,-.071,zz),(.986,.004,.003),AMBER,console,0,screen)
            for xx in (-.492,.492):box('Active row end',(xx,-.071,z),(.003,.004,.084),AMBER,console,0,screen)
    # Close-range work table, shallow tray and separate props.
    t=empty('TABLE / move as one',(-2.8,-1.35,0),desk,math.radians(18))
    box('Table foot',(0,0,.08),(2.17,1.23,.16),METAL,desk,.035,t)
    box('Table pedestal',(0,0,.45),(1.95,1.05,.78),DARK,desk,.025,t)
    box('Table top',(0,0,.89),(2.35,1.34,.13),FLOOR,desk,.035,t)
    box('Tray bottom',(.32,-.43,.968),(1.30,.35,.035),METAL,desk,.008,t)
    for x in (-.35,.99):box('Tray side',(x,-.43,1.0),(.04,.40,.095),METAL,desk,.006,t)
    box('Tray back',(.32,-.24,1.0),(1.38,.04,.095),METAL,desk,.006,t)
    box('Tray front',(.32,-.63,.99),(1.38,.04,.075),METAL,desk,.006,t)
    box('Reset button',(-.25,-.662,.99),(.075,.02,.065),GOLD,desk,.006,t)
    text('Reset label','ВЕРНУТЬ ПРЕДМЕТЫ',(-.12,-.656,.965),.053,LETTER,desk,t,font)
    box('Table front light',(-.73,-.545,.59),(.48,.024,.042),AMBER,desk,.006,t)
    box('Table side light',(-.987,0,.57),(.025,.78,.044),AMBER,desk,.006,t)
    bpy.ops.mesh.primitive_uv_sphere_add(segments=48,ring_count=24,radius=.16)
    ball=move(bpy.context.object,props);ball.name='PROP / black sphere';ball.parent=t;ball.location=(-.67,.12,1.12);ball.data.materials.append(stone('Black ceramic',(.015,.019,.023),.28,True))
    for f in ball.data.polygons:f.use_smooth=True
    box('PROP / amber cube',(-.2,.24,1.095),(.26,.26,.28),GOLD,props,.012,t,rotation=(0,0,.12))
    cylinder('PROP / drink can',(.32,.3,1.13),.105,.35,METAL,props,t)
    for z in (.96,1.302):cylinder('Can rolled rim',(.32,.3,z),.109,.018,GOLD,props,t)
    text('Can label','NC',(.25,.198,1.12),.07,LETTER,props,t,bold)
    for i,(x,y,angle) in enumerate(((.12,-.05,.12),(.62,-.02,-.13))):
        card=empty(f'PROP / data shard {i+1}',(x,y,.98),props,angle);card.parent=t
        box('Shard body',(0,0,0),(.22,.135,.026),METAL,props,.007,card)
        for k in range(4):box('Shard contact',(-.064+k*.028,.045,.015),(.013,.027,.002),GOLD,props,0,card)
        line('Shard etched trace',[(-.06,-.035,.014),(-.02,.008,.014),(.07,.008,.014)],.002,AMBER,props,card)
    # Shell niche, a life-size transparent construct against a dark backing.
    p=empty('SHELL ALCOVE / move as one',(4.4,2.4,0),pod,math.radians(-18))
    box('Shell foundation',(0,.30,.115),(2.85,2.0,.23),DARK,pod,.03,p)
    box('Shell rear wall',(0,.91,2.22),(2.55,.28,4.0),CONCRETE,pod,.03,p)
    for x in (-.91,.91):box('Shell side column',(x,.24,2.16),(.68,1.58,3.9),CONCRETE,pod,.03,p)
    box('Shell lintel',(0,.24,3.74),(2.55,1.58,.70),CONCRETE,pod,.03,p)
    box('Inner backing',(0,.745,1.82),(1.17,.035,3.15),METAL,pod,.012,p)
    for x in (-.59,.59):
        box('Shell dark inner jamb',(x,.1,1.85),(.09,1.10,3.20),METAL,pod,.01,p)
        box('Shell amber upright',(x,-.47,1.85),(.019,.018,3.17),AMBER,pod,.004,p)
    box('Shell upper light',(0,-.46,3.40),(1.20,.026,.027),AMBER,pod,.004,p)
    box('Shell lower light',(0,-.49,.25),(1.20,.045,.028),AMBER,pod,.004,p)
    box('Shell floor plate',(0,.05,.255),(1.13,1.23,.045),GOLD,pod,.012,p)
    box('Shell label slab',(1.27,-.58,2.12),(.92,.15,.87),DARK,pod,.025,p)
    text('Shell title','ОБОЛОЧКА',(.91,-.662,2.26),.080,LETTER,pod,p,font)
    box('Shell label rule',(1.25,-.662,2.16),(.69,.008,.005),GOLD,pod,0,p)
    text('Shell status','ОЖИДАНИЕ СВЯЗИ',(.91,-.665,2.03),.048,LETTER,pod,p,font)
    # Interlocked masses around the niche, from the reference.
    box('Shell high block',(-.4,1.5,4.12),(3.2,2.9,2.6),CONCRETE,pod,.04,p)
    box('Shell left offset',(-1.25,1.0,3.10),(1.1,2.8,2.7),CONCRETE,pod,.03,p)
    box('Shell low side block',(2.05,1.0,.65),(2.2,2.5,1.3),CONCRETE,pod,.025,p)
    box('Shell mass seam',(0.36,-.012,4.4),(.022,.025,1.4),AMBER,pod,.004,p)
    body_glb=list((ROOT/'build/lobby-blender/poses').rglob('mannequin_b_full_d.glb'))
    if not body_glb:raise RuntimeError('Export mannequin_b_full_d.mesh to build/lobby-blender/poses first')
    before=set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(body_glb[0]))
    imported=[o for o in bpy.data.objects if o not in before]
    body=empty('Construct / 2.15m',(0,.16,.29),pod);body.parent=p
    holo=hologram_material()
    meshes=[o for o in imported if o.type=='MESH']
    # Freeze skinning BEFORE removing the imported rig; proxy vertices are stored in bone-local frames.
    bpy.context.view_layer.update();deps=bpy.context.evaluated_depsgraph_get()
    frozen=[]
    for o in meshes:
        mesh=bpy.data.meshes.new_from_object(o.evaluated_get(deps),depsgraph=deps)
        mesh.transform(o.matrix_world);frozen.append(mesh)
    for o in imported:bpy.data.objects.remove(o,do_unlink=True)
    for mesh in frozen:
        o=bpy.data.objects.new('Construct surface',mesh);pod.objects.link(o)
        o.parent=body;o.scale=(1.19,1.19,1.19);o.rotation_euler.z=math.pi
        mesh.materials.clear();mesh.materials.append(holo)
        for f in mesh.polygons:f.material_index=0;f.use_smooth=True
    # Blue atmospheric depth, warm accents are deliberately local to keep the floor cool.
    world_setup()
    area('Open sky softbox',(0,9,52),85000,(.85,.91,1),48,(0,9,0),lighting)
    area('Cool front fill',(0,-22,13),4500,(.70,.80,1),22,(0,8,3),lighting)
    area('Warm horizon bounce',(-26,5,9),5500,(1,.66,.37),18,(0,12,5),lighting)
    area('Infinity blue atmospheric backlight',(0,85,18),190000,(.22,.56,1),65,(0,0,12),lighting)
    area('Pod downlight',(4.4,2.15,3.4),75,(1,.52,.16),.8,(4.4,2.6,1),lighting)
    area('Pod floor bounce',(4.3,1.7,.35),32,(1,.42,.1),.7,(4.3,2.4,1.8),lighting)
    area('Table concealed light',(-3.8,-2.3,.58),20,(1,.48,.12),.8,(-4,-3,.1),lighting)
    for x,y in [(-5,4),(5,4),(-7,13),(7,16)]:
        area('Terrace amber wash',(x,y,.65),90,(1,.47,.14),2.0,(x,y-1.3,0),lighting)
    fog=bpy.data.materials.new('Blue haze / depth');fog.use_nodes=True;n=fog.node_tree.nodes;n.clear()
    vo=n.new('ShaderNodeVolumePrincipled');vo.inputs['Density'].default_value=.003
    vo.inputs['Color'].default_value=(.52,.69,.88,1);vo.inputs['Anisotropy'].default_value=.2
    output=n.new('ShaderNodeOutputMaterial');fog.node_tree.links.new(vo.outputs['Volume'],output.inputs['Volume'])
    geo=n.new('ShaderNodeNewGeometry');height=n.new('ShaderNodeSeparateXYZ');fade=n.new('ShaderNodeMapRange')
    fade.inputs['From Min'].default_value=12;fade.inputs['From Max'].default_value=80
    fade.inputs['To Min'].default_value=.010;fade.inputs['To Max'].default_value=.0009;fade.clamp=True
    fog.node_tree.links.new(geo.outputs['Position'],height.inputs[0]);fog.node_tree.links.new(height.outputs['Z'],fade.inputs[0])
    fog.node_tree.links.new(fade.outputs[0],vo.inputs['Density'])
    box('Atmospheric volume',(0,180,65),(700,700,180),fog,lighting,0)
    hero=camera('01 / Panorama',(0,-5.4,1.6),(0,8,1.85),14.0,cameras)
    close=camera('02 / Terminal and shell',(-.2,-4.0,1.65),(.30,5.5,1.9),16.5,cameras)
    top=camera('03 / Upward reference',(0,3,1.72),(0,3.4,45),16,cameras)
    pano=camera('04 / VR panorama',(0,-3,1.72),(0,8,1.72),20,cameras)
    pano.data.type='PANO';pano.data.panorama_type='EQUIRECTANGULAR'
    for name in ('panorama','closeup','ceiling'):
        image=bpy.data.images.load(str(OUT/'references'/(name+'.png')));image.pack()
        obj=bpy.data.objects.new('Reference / '+name,None);obj.empty_display_type='IMAGE';obj.data=image
        refs.objects.link(obj);obj.location=(0,0,-50);obj.hide_render=True
    refs.hide_render=True;refs.hide_viewport=True
    scene=bpy.context.scene;scene.camera=hero;scene.unit_settings.system='METRIC';scene.unit_settings.scale_length=1
    scene.render.engine='CYCLES';scene.cycles.samples=96;scene.cycles.use_denoising=True
    scene.cycles.max_bounces=8;scene.cycles.transparent_max_bounces=12;scene.cycles.volume_bounces=1
    scene.cycles.volume_step_rate=2
    scene.render.resolution_x=1774;scene.render.resolution_y=887;scene.render.resolution_percentage=100
    scene.render.image_settings.file_format='PNG'
    scene.view_settings.view_transform='AgX';scene.view_settings.look='AgX - Medium High Contrast';scene.view_settings.exposure=0
    scene.render.film_transparent=False
    scene['reference']='references/panorama.png';scene['scale']='Metres, X right, Y forward, Z up'
    scene['status']='Blender art scene. Engine conversion and interaction integration are separate.'
    if hasattr(scene,'compositing_node_group'):
        tree=bpy.data.node_groups.new('Lobby compositing','CompositorNodeTree');scene.compositing_node_group=tree
        tree.interface.new_socket(name='Image',in_out='OUTPUT',socket_type='NodeSocketColor')
        output_type='NodeGroupOutput'
    else:
        scene.use_nodes=True;tree=scene.node_tree;output_type='CompositorNodeComposite'
    n=tree.nodes;n.clear();rl=n.new('CompositorNodeRLayers');gl=n.new('CompositorNodeGlare')
    if 'Type' in gl.inputs:
        gl.inputs['Type'].default_value='Fog Glow';gl.inputs['Quality'].default_value='Medium'
        gl.inputs['Threshold'].default_value=1.5;gl.inputs['Strength'].default_value=.3
    else:
        gl.glare_type='FOG_GLOW';gl.quality='MEDIUM';gl.threshold=1.5
    comp=n.new(output_type);tree.links.new(rl.outputs['Image'],gl.inputs['Image']);tree.links.new(gl.outputs['Image'],comp.inputs['Image'])
    # Start the .blend in camera view, with a useful material-preview workspace.
    for screen in bpy.data.screens:
        for area_ in screen.areas:
            if area_.type=='VIEW_3D':
                area_.spaces.active.region_3d.view_perspective='CAMERA'
                area_.spaces.active.clip_end=800
    bpy.ops.object.select_all(action='DESELECT')
    hero.select_set(True);bpy.context.view_layer.objects.active=hero
    try:
        pref=bpy.context.preferences.addons['cycles'].preferences;pref.compute_device_type='OPTIX';pref.get_devices()
        for d in pref.devices:d.use=d.type=='OPTIX'
        scene.cycles.device='GPU'
        print('RENDER_DEVICES',[(d.name,d.type,d.use) for d in pref.devices],flush=True)
    except Exception as exc:
        scene.cycles.device='CPU';print('CPU_RENDER',exc,flush=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'lobby.blend'))
    print('SCENE_READY',len(bpy.data.objects),'objects',flush=True)
    return hero,close,top,pano


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--preview',action='store_true');parser.add_argument('--render',action='store_true')
    args=parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [])
    OUT.mkdir(parents=True,exist_ok=True);(OUT/'renders').mkdir(exist_ok=True)
    cams=build();scene=bpy.context.scene
    if args.preview or args.render:
        scene.cycles.samples=24 if args.preview else 96
        if args.preview:scene.render.resolution_percentage=65
        for name,cam in zip(('panorama','closeup','ceiling'),cams):
            if args.preview and name!='panorama':continue
            scene.render.resolution_x,scene.render.resolution_y={'panorama':(1774,887),'closeup':(1672,941),'ceiling':(1200,800)}[name]
            scene.camera=cam;scene.render.filepath=str(OUT/'renders'/(name+('.preview' if args.preview else '')+'.png'))
            bpy.ops.render.render(write_still=True)
        scene.camera=cams[0]
    print('LOBBY_DONE',str(OUT/'lobby.blend'),flush=True)


if __name__=='__main__':main()
