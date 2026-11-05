"""Bounded fixed-foveation comparison using simulator poses and read-only diagnostics."""
import argparse
import ctypes as c
import json
import math
from pathlib import Path
import shutil
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT/'tools/quickboot_tests'), str(ROOT/'tools/swimming_tests'), str(ROOT/'tools/roomscale_tests')]
import launch_to_save as boot
from live_symbols import resolve
from live_read import Reader
from simulator_probe import send, read_status

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--pid', type=int, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--smoke', action='store_true')
p.add_argument('--framegen', action='store_true', help='Run the smoke comparison with frame generation enabled')
p.add_argument('--width', type=float, default=70.0)
p.add_argument('--height', type=float, default=65.0)
p.add_argument('--offset-x', type=float, default=0.0)
p.add_argument('--offset-y', type=float, default=-10.0)
p.add_argument('--debounce', action='store_true', help='Check that rapid INI changes coalesce into one applied region')
p.add_argument('--motion', type=float, default=8.0, help='Seconds of smooth simulated head movement (0 disables)')
a = p.parse_args()
if a.out.exists():
    raise RuntimeError('Use a fresh output directory')
a.out.mkdir(parents=True)
game = boot.DEFAULT_GAME
ini = game/'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
sim = Path.home()/'AppData/Local/OpenXR-Simulator'
mod = game/'bin/x64/plugins/cyber_engine_tweaks/mods/CyberpunkVRPort_QuickBoot'
original = ini.read_bytes()
(a.out/'vrport.before.ini').write_bytes(original)
initial = json.loads((mod/'quickboot_status.json').read_text())
if initial['phase'] != 'world_ready' or time.time()-initial['updatedAt'] > 3:
    raise RuntimeError('Game is not freshly confirmed in the world')
sim_state = read_status(sim)
if time.time()-(sim/'runtime_status.json').stat().st_mtime > 3:
    raise RuntimeError('Simulator status is stale')
head = sim_state['head_tracking']
pose = {**head['position'], **{k:head[k] for k in ('yaw','pitch','roll')}}
dll = ROOT/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll'
symbols = resolve(a.pid, dll, ['CyberpunkVR_FoveatedReport','CyberpunkVR_FoveatedReportSeq'])
address, sequence = [int(symbols[k],16) for k in ('CyberpunkVR_FoveatedReport','CyberpunkVR_FoveatedReportSeq')]
reader = Reader(a.pid, game/'bin/x64/Cyberpunk2077.exe', address, sequence)
baseline = boot.reporter_pids() | boot.reporter_window_pids()
started = time.time()-180
boot.kernel32.GetExitCodeProcess.argtypes = [c.c_void_p,c.POINTER(c.c_ulong)]
last_written = original
rows = []

def check():
    code = c.c_ulong()
    if not boot.kernel32.GetExitCodeProcess(reader.handle,c.byref(code)) or code.value != 259:
        boot.fail_for_crash(a.pid,started,baseline,'Foveated test game exited')
    if boot.crash_report(a.pid,started) or boot.new_reporters(baseline):
        boot.fail_for_crash(a.pid,started,baseline,'Foveated test crash')

def report():
    for _ in range(10):
        seq = struct.unpack('<I',reader.read(sequence,4))[0]
        if seq & 1:
            continue
        body = reader.read(address,32768).split(b'\0',1)[0]
        if seq == struct.unpack('<I',reader.read(sequence,4))[0]:
            return json.loads(body) if body else {}
    raise RuntimeError('Foveation snapshot busy')

def settings(mode, width, height, boundary, offset_x=None, offset_y=None):
    global last_written
    values = {'xr_foveated_dlss':str(mode),'xr_foveated_width':str(width),'xr_foveated_height':str(height),
              'xr_foveated_offset_x':str(a.offset_x if offset_x is None else offset_x),
              'xr_foveated_offset_y':str(a.offset_y if offset_y is None else offset_y),
              'xr_foveated_boundary':str(int(boundary)),'xr_framegen':str(int(a.framegen)),'xr_framegen_overlay':'1'}
    lines = ini.read_text(encoding='utf-8').splitlines()
    for i,line in enumerate(lines):
        key = line.split('=',1)[0].strip()
        if key in values:
            lines[i] = key+'='+values.pop(key)
    lines.extend(key+'='+value for key,value in values.items())
    last_written = ('\n'.join(lines)+'\n').encode('utf-8')
    ini.write_bytes(last_written)

def wait(seconds):
    end = time.monotonic()+seconds
    samples = []
    while time.monotonic() < end:
        check()
        snapshot = report()
        if snapshot:
            samples.append(snapshot)
        time.sleep(.5)
    return samples

def screenshot(label):
    requested = time.time()
    (sim/'screenshot_request.json').write_text(json.dumps({'eye':'both','layer':'all'}))
    end = time.monotonic()+8
    while time.monotonic() < end:
        check()
        shot = sim/'screenshot.bmp'
        completed = sim/'screenshot_status.json'
        if shot.exists() and completed.exists() and completed.stat().st_mtime >= requested:
            try:
                shutil.copy2(shot,a.out/(label+'.bmp'))
                return
            except PermissionError:
                pass
        time.sleep(.2)
    raise RuntimeError('Screenshot timed out')

def motion_test(label):
    before = report()
    start = time.monotonic()
    samples = []
    next_sample = start
    while time.monotonic()-start < a.motion:
        elapsed = time.monotonic()-start
        send(sim,{**pose,'yaw':pose['yaw']+.15*math.sin(elapsed*math.pi/2),
                  'pitch':pose['pitch']+.04*math.sin(elapsed*math.pi)})
        if time.monotonic() >= next_sample:
            check()
            samples.append(report())
            next_sample = time.monotonic()+.5
        time.sleep(.02)
    send(sim,pose)
    samples.extend(wait(1))
    after = report()
    (a.out/(label+'-motion.json')).write_text(json.dumps({'before':before,'samples':samples,'after':after},indent=2))
    first = {v['id']:v for v in before.get('view_details',[])}
    for view in after.get('view_details',[]):
        old = first[view['id']]
        if view['evaluations'] <= old['evaluations'] or view['resets'] != old['resets']:
            raise RuntimeError('DLSS history stopped or reset during head movement')
    if len(after.get('view_details',[])) != 2 or after['apis'][1]['failures'] != before['apis'][1]['failures']:
        raise RuntimeError('Foveated views failed during head movement')
    print(f'{label}: smooth head movement passed, both histories advanced without resets',flush=True)

def debounce_test():
    def geometry(value):
        return [(v['id'],v['crop_x'],v['crop_y'],v['crop_width'],v['crop_height']) for v in value.get('view_details',[])]
    baseline = geometry(report())
    samples = []
    for i in range(8):
        settings(1,45+i,50,True,-10,10)
        samples.extend(wait(.2))
        if geometry(report()) != baseline:
            raise RuntimeError('Region changed before rapid slider updates settled')
    settings(1,60,50,True,-10,20)
    samples.extend(wait(.5))
    if geometry(report()) != baseline:
        raise RuntimeError('Region applied before one-second settling delay')
    samples.extend(wait(2))
    final = report()
    (a.out/'debounce.json').write_text(json.dumps({'baseline':baseline,'samples':samples,'final':final},indent=2))
    if len(final.get('view_details',[])) != 2 or final['apis'][1]['failures']:
        raise RuntimeError('Shifted-region evaluation failed')
    for view in final['view_details']:
        if view['offset_x_percent'] != -10 or view['offset_y_percent'] != 20:
            raise RuntimeError('Final region offset was not applied')
        if view['crop_y'] <= (view['output_height']-view['crop_height'])/2:
            raise RuntimeError('Positive Y did not lower the region')
    screenshot('shifted-down')
    print('PASS rapid settings deferred; final region shifted left/down in both eyes',flush=True)

try:
    stages = [('quality-boundary',1,a.width,a.height,True)]
    if not a.smoke:
        stages += [('balanced',2,a.width,a.height,False),('performance',3,40,40,False),('off',0,a.width,a.height,False)]
    for label,mode,width,height,boundary in stages:
        settings(mode,width,height,boundary)
        print(f'{label}: warming up',flush=True)
        samples = wait(20)
        value = report()
        (a.out/(label+'.json')).write_text(json.dumps(value,indent=2))
        screenshot(label)
        api = value.get('apis',[{},{}])[1] if value.get('apis') else {}
        row = {'stage':label,'mode':mode,'width':width,'height':height,'api':api,'views':value.get('view_details',[]),'samples':samples}
        rows.append(row)
        print(json.dumps({k:row[k] for k in ('stage','api','views')}),flush=True)
        if mode:
            if value.get('backend') != 'native' or api.get('state') != 'Active' or api.get('active',0) == 0:
                raise RuntimeError('Backend is not processing foveated DLSS')
            if api.get('failures',0):
                raise RuntimeError('Backend reported an evaluation failure')
            views = [v for v in value.get('view_details',[]) if v.get('evaluations',0)>0]
            if len(views) < 2:
                raise RuntimeError('Two independent DLSS views were not observed')
            if a.motion > 0 and (a.smoke or label == 'balanced'):
                motion_test(label)
            if a.debounce and label == 'quality-boundary':
                debounce_test()
            if not a.smoke and label == 'balanced':
                for suffix,offset in [('left',-.15),('right',.15),('center',0)]:
                    send(sim,{**pose,'yaw':pose['yaw']+offset})
                    wait(4)
                    screenshot('balanced-'+suffix)
        elif api.get('state') != 'Off' or value.get('view_details'):
            raise RuntimeError('Off did not return to the native full-frame path')
    print('PASS fixed foveated simulation stages',flush=True)
finally:
    if ini.read_bytes() == last_written:
        ini.write_bytes(original)
    try:
        send(sim,pose)
    except Exception as error:
        print('Pose restore:',error,flush=True)
    reader.close()
    (a.out/'results.json').write_text(json.dumps({'pid':a.pid,'symbols':symbols,'stages':rows},indent=2))
    print('Original settings and simulator pose restored',flush=True)
