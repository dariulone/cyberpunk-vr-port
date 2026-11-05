"""WorldBuilder/CETBridge authoring and OpenXR Simulator inspection, without window focus.

  py -3.13 -B tools/vr_lobby_assets/control.py editor request.json
  py -3.13 -B tools/vr_lobby_assets/control.py survey --out scratch/lobby-survey

The same structured requests work through WolvenKit's live_execute_lua MCP tool:
GetMod('CyberpunkVRPort_Lobby').editor({op='list'})
"""
import argparse
import json
import os
from pathlib import Path
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
BRIDGE = GAME / 'bin/x64/plugins/cyber_engine_tweaks/mods/CETBridge'
SIM = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'


def execute(code, timeout=12):
    request = BRIDGE / 'command.json'
    if request.exists():
        raise RuntimeError('CETBridge has another pending request')
    uid = str(uuid.uuid4())
    temporary = BRIDGE / (uid + '.writing')
    temporary.write_text(json.dumps({'id': uid, 'type': 'exec', 'code': code}), encoding='utf-8')
    os.replace(temporary, request)
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            result = json.loads((BRIDGE / 'response.json').read_text(encoding='utf-8'))
        except (OSError, json.JSONDecodeError):
            result = {}
        if result.get('id') == uid:
            if not result.get('ok'):
                raise RuntimeError(result)
            return result.get('result')
        time.sleep(0.04)
    raise TimeoutError('CETBridge did not complete the request; it was not retried')


def literal(text):
    eq = '='
    while ']' + eq + ']' in text:
        eq += '='
    return '[' + eq + '[' + text + ']' + eq + ']'


def survey(out):
    import math
    from PIL import Image
    sys.path.insert(0, str(ROOT / 'tools/roomscale_tests'))
    from simulator_probe import read_status, send, await_pose
    out.mkdir(parents=True, exist_ok=True)
    head = read_status(SIM)['head_tracking']
    original = {**head['position'], **{k: head[k] for k in ('yaw', 'pitch', 'roll')}}
    shots = []
    try:
        for name, yaw, pitch in [('front',0,0),('left',45,0),('right',-45,0),('back',180,0),('up',0,78)]:
            pose = {**original, 'yaw': original['yaw']+math.radians(yaw), 'pitch': math.radians(pitch)}
            send(SIM, pose)
            await_pose(SIM, pose)
            time.sleep(.5)
            requested = time.time()
            request = SIM / 'screenshot_request.json'
            if request.exists():
                raise RuntimeError('Another screenshot is pending')
            request.write_text(json.dumps({'eye':'left','layer':'all'}), encoding='ascii')
            deadline = time.monotonic()+8
            while time.monotonic()<deadline:
                status = SIM / 'screenshot_status.json'
                if status.exists() and status.stat().st_mtime >= requested:
                    with Image.open(SIM/'screenshot.bmp') as image:
                        image.save(out/(name+'.png'))
                    shots.append({'name':name,'pose':pose,'capture':json.loads(status.read_text())})
                    break
                time.sleep(.04)
            else:
                raise TimeoutError('Simulator did not capture')
    finally:
        send(SIM, original)
        await_pose(SIM, original)
        (out/'survey.json').write_text(json.dumps({'original':original,'shots':shots}, indent=2), encoding='utf-8')
    return {'out':str(out.resolve()),'shots':len(shots),'pose_restored':True}


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    p=sub.add_parser('editor');p.add_argument('request',type=Path)
    p=sub.add_parser('survey');p.add_argument('--out',required=True,type=Path)
    p=sub.add_parser('lua');p.add_argument('file',type=Path)
    args=parser.parse_args()
    if args.command=='survey':
        print(json.dumps(survey(args.out)))
    elif args.command=='lua':
        print(execute(args.file.read_text(encoding='utf-8-sig')))
    else:
        req=json.loads(args.request.read_text(encoding='utf-8-sig'))
        source='local r=json.decode('+literal(json.dumps(req,ensure_ascii=False))+'); print(json.encode(GetMod("CyberpunkVRPort_Lobby").editor(r)))'
        print(execute(source))
