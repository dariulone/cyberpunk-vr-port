"""Label the driver's original controller artwork without changing its pixels."""
import argparse
import base64
import html
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--driver', type=Path, default=Path('D:/build/CyberFinger_SteamVRFusion'))
args = parser.parse_args()
out = ROOT / 'docs' / 'controls'
out.mkdir(parents=True, exist_ok=True)
# Control anchors from the driver's docs/manual/manual.html, in source pixels.
anchors = [
    [(440,260,290,140),(646,318,800,330),(570,472,640,600),
     (482,466,490,610),(568,1058,800,1020),(564,1238,800,1280)],
    [(780,191,940,80),(621,252,470,200),(671,381,610,500),
     (744,383,760,515),(698,978,1050,930),(695,1131,1050,1185)],
]
names = ['STICK','TRIGGER','GRAB','MENU','BLACK WRIST','PINK WRIST']
contexts = {
 'Gameplay': [
    ['Move. Hold fully forward 0.2 s to sprint. Click and release to jump.', 'Fire weapon.',
     'Toggle scanner.', 'Tap: next weapon. Hold: weapon wheel.',
     'Tap: eject magazine. Hold: pause.', 'SteamVR dashboard.'],
    ['Turn. Up/down for dash/crouch. Click to jump, hold to charge.', 'Prompt: Take / Interact. Otherwise aim.',
     'Eat / use at prompts. Rack a gun or toggle fists / melee weapon.', 'Tap: inventory. Hold: OS cyberware.',
     'Tap: quick reload. Hold: heal.', 'Open / close VR menu.'],
 ],
 'Cyberpunk menus': [
    ['Navigate highlights in four directions.', 'A: select / click this hand\'s laser target.',
     'X: secondary action; hold when prompted.', 'B: Back / exit on release.',
     'Use Menu for Back.', 'SteamVR dashboard.'],
    ['Left/right: previous / next tab.', 'A: select / click this hand\'s laser target.',
     'X: secondary action; hold when prompted.', 'B: Back / exit on release.',
     'Use Grip for X.', 'Open / close VR menu.'],
 ],
 'VR menus / F10': [
    ['Navigate highlights in four directions.', 'Point and select with this hand.',
     'Context; hold F10 title to move.', 'Back / cancel / close on release.',
     'Tap: eject magazine. Hold: pause.', 'SteamVR dashboard.'],
    ['Left/right: tabs. Up/down: VR list scroll.', 'Point and select with this hand.',
     'Context; hold F10 title to move.', 'Back / cancel / close on release.',
     'Use Menu for Back; hold: heal.', 'Open / close VR menu.'],
 ],
 'Conversation choices': [
    ['Move where dialogue allows.', 'Weapon trigger; no dialogue selection.',
     'Toggle scanner.', 'Tap: next weapon. Hold: weapon wheel.',
     'Tap: eject magazine. Hold: pause.', 'SteamVR dashboard.'],
    ['Up/down: choices. Left/right: turn.', 'Confirm choice; normal aim/fire is paused.',
     'Slide release / rack / melee.', 'Tap: inventory. Hold: OS cyberware.',
     'Tap: quick reload. Hold: heal.', 'Open / close VR menu.'],
 ],
 'Map / journal': [
    ['Map: up/down zoom. Journal: highlights.', 'Select / click the left laser target.',
     'Map: right-click waypoint. Journal: X.', 'Close / Back on release.',
     'Use Menu for Back.', 'SteamVR dashboard.'],
    ['Left/right to switch tabs.', 'Map: jump to V. Journal: sort (Tab).',
     'Map: open journal. Journal: X.', 'Close / Back on release.',
     'Use Menu for Back.', 'Open / close VR menu.'],
 ],
 'Computer': [
    ['Movement hand; idle while using screen.', 'Use the right hand to click.',
     'Use right Grip for computer Back.', 'Exit computer on release.',
     'Use Menu to exit.', 'SteamVR dashboard.'],
    ['Up/down: scroll the computer.', 'Aim hand as mouse. Trigger: click.',
     'Back within the computer.', 'Exit computer on release.',
     'Use Menu to exit.', 'Open / close VR menu.'],
 ],
 'Apartment picture frame': [
    ['Navigate highlights in four directions.', 'Laser click / confirm focused photo.',
     'Secondary / remove photo.', 'Close / cancel on release.',
     'Use Menu for Back.', 'SteamVR dashboard.'],
    ['Left/right: zoom out/in.', 'Laser click / confirm focused photo.',
     'Secondary / remove photo.', 'Close / cancel on release.',
     'Use Menu for Back.', 'Open / close VR menu.'],
 ],
}
notes = {
 'Gameplay': 'Hold left stick click and push right stick for the D-pad. Hold both Menu buttons to toggle HUD. Close fingers near an object to grab.',
 'Cyberpunk menus': 'Includes wardrobe. Either hand: Trigger = A, Menu = B, Grip = X. Both Menu held: toggle HUD.',
 'VR menus / F10': 'Trigger selects. Grip opens context actions. Menu goes Back. Hold both Menu buttons: toggle HUD.',
 'Conversation choices': 'HUD shows automatically. Move and turn where the game permits; right up/down cannot dash or crouch.',
 'Map / journal': 'Left weapon preset shown. Map/journal hand roles swap with weapon handedness. Both Menu: toggle HUD.',
 'Computer': 'Mouse hand is opposite movement stick. Head movement never moves the cursor. Menu exits; Grip goes back.',
 'Apartment picture frame': 'Stationary panel: move your head freely. Laser + Trigger or stick highlights. Both Menu: toggle HUD.',
}
def text(x,y,value,cls='label',ident=''):
    return f'<text x="{x}" y="{y}" class="{cls}" id="{ident}">{html.escape(value)}</text>'

parts = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1600" height="1370" viewBox="0 0 1600 1370" role="img" aria-labelledby="title desc">',
 '<title id="title">CyberFinger controls in Cyberpunk VR</title>',
 '<desc id="desc">Numbered original left and right controller illustrations with context-specific controls. Left-handed weapon preset.</desc>',
 '<style>text{font-family:Segoe UI,Arial,sans-serif;fill:#e7edf5}.heading{font-size:40px;font-weight:700}.sub{font-size:22px;fill:#a8c5d9}.hand{font-size:25px;font-weight:700;fill:#63e6e7}.label{font-size:22px}.key{font-size:18px;fill:#63e6e7;font-weight:700}.number{font-size:23px;fill:#071322;font-weight:700;text-anchor:middle}</style>',
 '<rect width="1600" height="1370" fill="#081321"/>',
 text(40,60,'CYBERFINGER / CYBERPUNK VR','heading'),
 text(40,100,'Gameplay | Left-handed weapon preset | Full-forward sprint','sub','context-heading')]
for hand in range(2):
    x = 32 + hand*784
    parts += [f'<rect x="{x}" y="126" width="752" height="1134" rx="20" fill="#102237"/>',
              text(x+26,167,('LEFT' if hand==0 else 'RIGHT')+' HAND','hand')]
    side = 'left' if hand==0 else 'right'
    image = args.driver / 'resources' / 'icons' / f'steamvr_cybrfngr_{side}_transp.png'
    data = base64.b64encode(image.read_bytes()).decode('ascii')
    px,py = (165,185) if hand==0 else (915,202)
    w,h = (1145,1374) if hand==0 else (1254,1254)
    scale=.4
    parts.append(f'<image x="{px}" y="{py}" width="{w*scale}" height="{h*scale}" xlink:href="data:image/png;base64,{data}"/>')
    for i,(tx,ty,bx,by) in enumerate(anchors[hand]):
        tx,ty,bx,by=px+tx*scale,py+ty*scale,px+bx*scale,py+by*scale
        parts += [f'<path d="M {tx} {ty} L {bx} {by}" stroke="#63e6e7" stroke-width="3"/>',
                  f'<circle cx="{tx}" cy="{ty}" r="5" fill="#63e6e7"/>',
                  f'<circle cx="{bx}" cy="{by}" r="19" fill="#63e6e7" stroke="#081321" stroke-width="3"/>',
                  text(bx,by+8,str(i+1),'number')]
    for i,name in enumerate(names):
        y=800+i*76
        parts += [text(x+24,y,f'{i+1}  {name}','key'),
                  text(x+24,y+28,contexts['Gameplay'][hand][i],'label',f'action-{hand}-{i}')]
parts += [text(40,1302,notes['Gameplay'],'sub','context-note'),
          text(40,1342,'Black wrist buttons follow weapon hand: eject / pause on the weapon side, quick reload / heal on the other.','sub'),'</svg>']
svg='\n'.join(parts)
(out/'cyberfinger-controls.svg').write_text(svg,encoding='utf-8')
page='''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>CyberFinger controls — Cyberpunk VR</title>
<style>body{margin:0;background:#081321;color:#e7edf5;font:18px system-ui}nav{padding:16px;display:flex;gap:12px;flex-wrap:wrap}button{padding:12px 22px;background:#102237;color:#63e6e7;border:1px solid #63e6e7;border-radius:8px;cursor:pointer;font:inherit}button[aria-pressed=true]{background:#63e6e7;color:#081321}svg{width:100%;height:auto;max-width:1600px;display:block;margin:auto}footer{padding:20px}a{color:#63e6e7}</style>
<nav aria-label="Control context">'''
for index,key in enumerate(contexts):
    page+=f'<button aria-pressed="{str(index==0).lower()}" data-context="{html.escape(key)}">{html.escape(key)}</button>'
page+='</nav>'+svg+'<footer>New plugin and v2 binding required. <a href="../../README_CYBRFNGR.md">Quick reference</a> · <a href="ARTWORK.md">Artwork attribution</a></footer>'
page+='<script>const contexts='+json.dumps(contexts)+';const notes='+json.dumps(notes)+''';
document.querySelectorAll('button').forEach(button=>button.addEventListener('click',()=>{
  const key=button.dataset.context;
  document.querySelectorAll('button').forEach(b=>b.setAttribute('aria-pressed',String(b===button)));
  document.getElementById('context-heading').textContent=key+' | Left-handed weapon preset'+(key==='Gameplay'?' | Full-forward sprint':'');
  document.getElementById('context-note').textContent=notes[key];
  contexts[key].forEach((rows,hand)=>rows.forEach((value,i)=>document.getElementById('action-'+hand+'-'+i).textContent=value));
}));</script></html>'''
(out/'cyberfinger-controls.html').write_text(page,encoding='utf-8')
(out/'ARTWORK-LICENSE.txt').write_bytes((args.driver/'LICENSE').read_bytes())
print('Wrote controller SVG, interactive HTML and original artwork license to', out)
