"""Dimensioned layout from the same manifest used by the Blender/native lobby authoring."""
import html,json,math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'art/vr_lobby/tutorial'
doc=json.loads((OUT/'native_manifest.json').read_text())
parts=['<svg xmlns="http://www.w3.org/2000/svg" width="1400" height="820" viewBox="0 0 1400 820">',
       '<rect width="1400" height="820" fill="#111923"/>',
       '<style>text{font-family:Arial,sans-serif;fill:#d5e5ef} .muted{fill:#8296a5}</style>',
       '<defs><marker id="arrow" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="#f9be56"/></marker></defs>']
def text(x,y,s,size=18,color=None):
    parts.append(f'<text x="{x}" y="{y}" font-size="{size}"'+(f' style="fill:{color}"' if color else '')+'>'+html.escape(s)+'</text>')
def line(x1,y1,x2,y2,col='#f9be56',dim=False):
    parts.append(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{col}" stroke-width="1.5"'+(' marker-start="url(#arrow)" marker-end="url(#arrow)"' if dim else '')+'/>')
text(38,43,'VR-ЛОББИ · РАЗМЕРЫ И РАССТАНОВКА',25)
text(38,74,'Оригинальные блоки туториала Cyberpunk · масштаб в метрах',16,'#8296a5')
def panel(origin,scale,bounds,label):
    ox,oy=origin;x0,x1,y0,y1=bounds
    def xy(x,y):return ox+(x-x0)*scale,oy+(y1-y)*scale
    a,b=xy(x0,y1),xy(x1,y0)
    parts.append(f'<rect x="{a[0]}" y="{a[1]}" width="{b[0]-a[0]}" height="{b[1]-a[1]}" fill="#182430" stroke="#385165"/>')
    objects=list(doc['instances'])
    for item in doc.get('custom_solids',[]):
        size=item.get('size')
        if size is None:
            size=[item['width_m'],item.get('depth_m',item.get('run_m')),item.get('height_m',item.get('rise_m'))]
        objects.append({'name':item['name'],'authoring_center_bottom':item['position'],
                        'dimensions':size,'rotation_degrees':[0,0,item.get('yaw_degrees',0)],'custom':True,'kind':item['type']})
    for item in objects:
        x,y,z=item['authoring_center_bottom'];w,d,h=item['dimensions']
        if not (x0<=x<=x1 and y0<=y<=y1):continue
        px,py=xy(x,y)
        color='#6b91ac' if z>4 else '#506573'
        if 'landing' in item['name']:color='#727b75'
        if 'stairs' in item['name']:color='#dda855'
        if 'staircase' in item['name']:color='#dda855'
        if 'inverted-T' in item['name']:color='#7ba88e'
        if 'jump block' in item['name']:color='#a0cc7a'
        if item.get('kind')=='inset_terrace':color='#ba985f'
        if item.get('kind') in ('inverted_t','t_block_stack'):color='#7ba88e'
        if item.get('kind') in ('micro_terrain','ground_with_basin_cutout'):continue
        parts.append(f'<rect x="{px-w*scale/2}" y="{py-d*scale/2}" width="{max(.9,w*scale)}" height="{max(.9,d*scale)}" fill="{color}" opacity=".7" transform="rotate({-item["rotation_degrees"][2]} {px} {py})"/>')
    for region in doc.get('microterraces',[]):
        points=' '.join(f'{xy(x,y)[0]},{xy(x,y)[1]}' for x,y in region['polygon_xy'])
        parts.append(f'<polygon points="{points}" fill="#496e84" stroke="#8bc0d7" stroke-width="1"/>')
    text(ox,oy-15,label,20)
    cx,cy=xy(0,doc['metrics']['circle_center'][1]);r=doc['metrics']['circle_radius_m']*scale
    parts.append(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="#f9be56" stroke-width="2"/>')
    px,py=xy(0,0);parts.append(f'<path d="M{px} {py-7}l-5 12h10z" fill="#72f0ca"/>')
    return xy
site=panel((38,140),.58,(-500,500,-500,500),'Весь участок · 1 км²')
line(38,747,618,747,dim=True);text(275,775,'1000 м',17)
near=panel((730,140),6.5,(-40,40,-5,85),'Площадка и первые подъёмы')
x0,y0=near(0,0);xc,yc=near(0,30);xd,_=near(-4.8,0)
line(xd,y0,xd,yc,dim=True);text(xd-68,(y0+yc)/2,'30 м',19,'#f9be56')
text(xc+36,yc-10,'Круг: 40 м²',17,'#f9be56');text(xc+36,yc+14,'Ø 7,14 м · уровень −24 см',14,'#f9be56')
text(x0+12,y0+24,'Точка обзора',16,'#72f0ca')
xs,ys=near(4.8,44);text(xs+12,ys,'Лестница внутри площадки',16);text(xs+12,ys+22,'41–46,2 м · подъём 1,2 м',13,'#8296a5')
text(730,775,'Далее — новые площадки, блоки и слои тумана',16,'#8296a5')
parts.append('</svg>');(OUT/'layout.svg').write_text('\n'.join(parts),encoding='utf-8')
assert doc['metrics']['world_area_m2']==1_000_000
assert abs(math.pi*doc['metrics']['circle_radius_m']**2-40)<1e-6
assert doc['metrics']['distance_to_circle_center_m']==30
print(json.dumps({'plan':str(OUT/'layout.svg'),'native_instances':len(doc['instances']),'metrics':doc['metrics']},ensure_ascii=False))
