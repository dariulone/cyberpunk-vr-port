"""Make an offline, reviewable gallery from measured simulator captures."""
import argparse
import html
import json
import shutil
from pathlib import Path


PAGE = r'''<!doctype html><html lang="ru"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cyberpunk VR — прицелы</title>
<style>
:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#10151b;color:#e9eef3}
body{margin:0}header{padding:22px 28px;border-bottom:1px solid #35414c}h1{font-size:25px;margin:0 0 8px}
p{margin:6px 0;color:#b7c4ce}main{padding:22px 28px;display:grid;grid-template-columns:repeat(auto-fit,minmax(290px,1fr));gap:18px}
button,select{font:inherit;color:inherit;background:#263440;border:1px solid #526675;border-radius:6px;padding:7px 12px;cursor:pointer}
.card{padding:0;text-align:left;overflow:hidden;background:#19232d;border:1px solid #354655;box-shadow:0 4px 14px #0004}
.card img{display:block;width:200%;transform:translateX(-50%)}.preview{overflow:hidden;aspect-ratio:1.247;background:#080c10}
.meta{padding:12px}.meta strong{display:block}.meta span{color:#93a9ba;font-size:13px;display:block;margin-top:5px}
dialog{width:min(1400px,94vw);max-width:none;border:1px solid #708698;border-radius:10px;background:#121a22;padding:16px}
dialog::backdrop{background:#000d}.toolbar{display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:12px}
.toolbar h2{font-size:18px;flex:1;margin:0}.viewer{width:100%;height:min(74vh,840px);background:#060a0e;display:block;touch-action:none}
small{display:block;margin-top:10px;color:#a7bac9}a{color:#8cdde8}input{accent-color:#65d5d1}
</style><header><h1>Cyberpunk VR — галерея прицелов</h1>
<p>Контрольные снимки: ясная погода, 12:00. Прицел в __DISTANCE__ см от правого глаза, laser dot = 0.</p>
<p>__NOTE__</p><p>Нажмите на карточку. В просмотре доступны линза крупным планом, правый глаз и стереопара.</p></header>
<main id="cards"></main><dialog id="viewer"><div class="toolbar"><button id="prev" aria-label="Предыдущий">←</button>
<button id="next" aria-label="Следующий">→</button><h2 id="title"></h2><select id="mode" aria-label="Вид">
<option value="lens">Линза</option><option value="right">Правый глаз</option><option value="stereo">Оба глаза</option><option value="source">Камера прицела</option></select>
<label>Масштаб <input id="zoom" type="range" min="1" max="4" step="0.1" value="1"></label><button id="close">Закрыть</button></div>
<canvas id="canvas" class="viewer"></canvas><small id="detail"></small><small><a id="original" target="_blank">Открыть исходный снимок</a> · Стрелки — смена прицела · Esc — закрыть</small></dialog>
<script>
const entries=__ENTRIES__;let selected=0,bitmap=new Image();const $=id=>document.getElementById(id);
entries.forEach((e,i)=>{const b=document.createElement('button');b.className='card';const p=document.createElement('div');p.className='preview';
const im=document.createElement('img');im.src=e.image;im.alt=e.title;im.loading='lazy';p.append(im);b.append(p);const m=document.createElement('div');m.className='meta';
const t=document.createElement('strong');t.textContent=e.title;const d=document.createElement('span');d.textContent=e.zoomLabel+' · '+e.record;m.append(t,d);b.append(m);
b.onclick=()=>show(i);$('cards').append(b)});
function show(i){selected=(i+entries.length)%entries.length;const e=entries[selected];$('title').textContent=e.title+' — '+e.zoomLabel;
$('detail').textContent=e.note||'';$('zoom').value=1;const option=$('mode').querySelector('[value="source"]');option.disabled=!e.sourceImage;
if(!e.sourceImage&&$('mode').value==='source')$('mode').value='lens';loadImage();if(!$('viewer').open)$('viewer').showModal()}
function loadImage(){const e=entries[selected],url=$('mode').value==='source'?e.sourceImage:e.image;const next=new Image();bitmap=next;
$('original').href=url;$('detail').textContent=$('mode').value==='source'?(e.sourceNote||'Исходная текстура камеры прицела.'):(e.note||'');
next.onload=()=>{if(bitmap===next)draw()};next.src=url}
function draw(){if(!bitmap.complete||!bitmap.naturalWidth)return;const c=$('canvas'),r=c.getBoundingClientRect(),dpr=window.devicePixelRatio||1;
c.width=Math.round(r.width*dpr);c.height=Math.round(r.height*dpr);const x=c.getContext('2d');x.fillStyle='#060a0e';x.fillRect(0,0,c.width,c.height);
const w=bitmap.naturalWidth,h=bitmap.naturalHeight,mode=$('mode').value,e=entries[selected];
const crop=e.lensCrop||[.57,.23,.37,.52];let rect=mode==='lens'?[crop[0]*w,crop[1]*h,crop[2]*w,crop[3]*h]:mode==='right'?[w*.5,0,w*.5,h]:[0,0,w,h];
const zoom=Number($('zoom').value),cx=rect[0]+rect[2]/2,cy=rect[1]+rect[3]/2;rect[2]/=zoom;rect[3]/=zoom;rect[0]=cx-rect[2]/2;rect[1]=cy-rect[3]/2;
const scale=Math.min(c.width/rect[2],c.height/rect[3]),dw=rect[2]*scale,dh=rect[3]*scale;x.drawImage(bitmap,...rect,(c.width-dw)/2,(c.height-dh)/2,dw,dh)}
$('prev').onclick=()=>show(selected-1);$('next').onclick=()=>show(selected+1);$('close').onclick=()=>$('viewer').close();$('mode').onchange=loadImage;$('zoom').oninput=draw;
window.addEventListener('resize',draw);window.addEventListener('keydown',e=>{if(!$('viewer').open)return;if(e.key==='ArrowRight')show(selected+1);if(e.key==='ArrowLeft')show(selected-1)});
</script></html>'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--run', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--catalog', type=Path, help='Current capture catalog; images are read beside it, without stale fallbacks.')
    parser.add_argument('--note', default='Рабочая галерея для проверки; отдельные модели ещё дорабатываются.')
    parser.add_argument('--distance-cm', type=float, default=5, help='Must match every measured capture in the catalog.')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    images = args.out/'images'
    images.mkdir(exist_ok=True)
    catalog=args.catalog or args.run/'catalog-gallery-v2/catalog.json'
    rows = json.loads(catalog.read_text(encoding='utf-8'))
    entries = []
    for row in rows:
        distance=row.get('distance',.15)
        if abs(distance*100-args.distance_cm)>.1:
            raise ValueError('Capture distance does not match gallery label: '+str(row.get('image')))
        scope = row.get('scope')
        record = scope or row['weapon']
        key = scope.removeprefix('Items.w_att_scope_') if scope else record.removeprefix('Items.Preset_').removesuffix('_Default')
        filename = row.get('image') or (scope.split('.')[-1]+'.png' if scope else key+'.png')
        original = args.run/'catalog-gallery-v2'/filename
        refined = args.run/'refined-gallery'/(key+'.png')
        source = catalog.parent/filename if args.catalog else (refined if refined.exists() else original)
        if not source.exists():
            raise FileNotFoundError(source)
        target = images/filename
        shutil.copy2(source, target)
        magnification = row.get('opticsFinal', {}).get('zoom', row['zoom'])
        title = row.get('title') or ''
        if not title or title.startswith('LocKey'):
            title = key.replace('_', ' ').title() if scope else {'Tech_Sniper_Rifle': 'Rasetsu'}.get(key, key)
        label = ('%g×' % magnification) if key != 'Ashura' else 'Штатный вид'
        path=row.get('path',row.get('opticsFinal',{}).get('path'))
        note = 'Боковой дисплей и штатная система наведения сохранены по запросу.' if key == 'Ashura' else ''
        if path=='camera':note=row.get('reticleNote','Отдельный рендер. Видимость прицельных отметок ещё дорабатывается.')
        elif path=='shader':note='Увеличение внутри линзы через шейдер.'
        elif path=='collimator':note='Коллиматор без увеличения: 1×.'
        entry=dict(title=title,record=record,zoomLabel=label,path=path,note=note,image='images/'+filename)
        entry['distanceCm']=distance*100
        if row.get('lensCrop'):entry['lensCrop']=row['lensCrop']
        camera_source=catalog.parent/(Path(filename).stem+'-source-rgb.png')
        if path=='camera' and camera_source.exists():
            shutil.copy2(camera_source,images/camera_source.name)
            entry['sourceImage']='images/'+camera_source.name
            if (catalog.parent/(Path(filename).stem+'-source-linear.npy')).exists():
                entry['sourceNote']='Предпросмотр линейного HDR до тональной обработки. Яркие значения обрезаны только для этого диагностического PNG; итоговый вид смотрите в режиме «Линза».'
        entries.append(entry)
    (args.out/'manifest.json').write_text(json.dumps(entries,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    payload=json.dumps(entries,ensure_ascii=False).replace('<','\\u003c')
    (args.out/'index.html').write_text(PAGE.replace('__NOTE__',html.escape(args.note)).replace('__DISTANCE__','%g'%args.distance_cm).replace('__ENTRIES__',payload),encoding='utf-8')
    print('Saved %d captured scopes to %s' % (len(entries),args.out))


if __name__=='__main__':
    main()
