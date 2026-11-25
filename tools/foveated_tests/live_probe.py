"""Bounded fixed-foveation comparison using simulator poses and read-only diagnostics."""
import argparse
import ctypes as c
from ctypes import wintypes as w
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
p.add_argument('--eye-gaze', action='store_true', help='Drive synthetic OpenXR eye gaze (pursuit, saccade, blink hold, dropout) via exported test controls')
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
symbol_names = ['CyberpunkVR_FoveatedReport','CyberpunkVR_FoveatedReportSeq']
if a.eye_gaze:
    symbol_names += ['CyberpunkVR_TestEyeGazeEnable','CyberpunkVR_TestEyeGazeTanX','CyberpunkVR_TestEyeGazeTanY']
symbols = resolve(a.pid, dll, symbol_names)
address, sequence = [int(symbols[k],16) for k in ('CyberpunkVR_FoveatedReport','CyberpunkVR_FoveatedReportSeq')]
reader = Reader(a.pid, game/'bin/x64/Cyberpunk2077.exe', address, sequence)
write_handle = None
kernel_rw = None
if a.eye_gaze:
    gaze_enable_addr, gaze_tan_x_addr, gaze_tan_y_addr = [
        int(symbols[k],16) for k in ('CyberpunkVR_TestEyeGazeEnable','CyberpunkVR_TestEyeGazeTanX','CyberpunkVR_TestEyeGazeTanY')
    ]
    kernel_rw = c.WinDLL('kernel32', use_last_error=True)
    kernel_rw.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel_rw.OpenProcess.restype = w.HANDLE
    kernel_rw.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel_rw.WriteProcessMemory.restype = w.BOOL
    kernel_rw.CloseHandle.argtypes = [w.HANDLE]
    kernel_rw.CloseHandle.restype = w.BOOL
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

def write_mem(addr, data):
    if not write_handle:
        raise RuntimeError('Eye-gaze write handle is not open')
    written = c.c_size_t()
    if not kernel_rw.WriteProcessMemory(write_handle, addr, data, len(data), c.byref(written)) or written.value != len(data):
        raise c.WinError(c.get_last_error())

def set_eye_gaze(enable, tan_x=0.0, tan_y=0.0):
    if enable == 0:
        write_mem(gaze_enable_addr, struct.pack('<i', 0))
    if gaze_tan_y_addr == gaze_tan_x_addr + 4:
        write_mem(gaze_tan_x_addr, struct.pack('<ff', float(tan_x), float(tan_y)))
    else:
        write_mem(gaze_tan_x_addr, struct.pack('<f', float(tan_x)))
        write_mem(gaze_tan_y_addr, struct.pack('<f', float(tan_y)))
    if enable != 0:
        write_mem(gaze_enable_addr, struct.pack('<i', int(enable)))

def settings(mode, width, height, boundary, offset_x=None, offset_y=None, eye_tracked=False):
    global last_written
    values = {'xr_foveated_dlss':str(mode),'xr_foveated_width':str(width),'xr_foveated_height':str(height),
              'xr_foveated_offset_x':str(a.offset_x if offset_x is None else offset_x),
              'xr_foveated_offset_y':str(a.offset_y if offset_y is None else offset_y),
              'xr_foveated_boundary':str(int(boundary)),
              'xr_foveated_eye_tracked':str(int(eye_tracked)),
              'xr_framegen':str(int(a.framegen)),'xr_framegen_overlay':'1'}
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

def eye_gaze_test():
    def by_id(snap):
        return {v['id']: v for v in snap.get('view_details', [])}

    # 1. Enable eye-tracked mode at 50x50 (+-25% travel room on both axes) and center gaze.
    settings(1, 50, 50, True, eye_tracked=True)
    set_eye_gaze(1, 0.0, 0.0)
    center_samples = wait(1.5)
    center = report()
    deadline = time.monotonic() + 1.5
    while (not center.get('eye_tracked') or center.get('gaze_state') != 4 or len(center.get('view_details', [])) != 2) and time.monotonic() < deadline:
        center_samples.extend(wait(0.5))
        center = report()
    api = center.get('apis', [{}, {}])[1] if center.get('apis') else {}
    if not center.get('eye_tracked') or center.get('gaze_state') != 4:
        raise RuntimeError(f'Eye tracking did not become Active: eye_tracked={center.get("eye_tracked")} gaze_state={center.get("gaze_state")}')
    if api.get('state') != 'Active' or len(center.get('view_details', [])) != 2:
        raise RuntimeError('Foveated DLSS views not active during eye-gaze initialization')
    baseline_failures = api.get('failures', 0)

    # 2. Smooth pursuit from TanX = -0.25 to +0.25 without NGX failures or history resets.
    for i in range(1, 11):
        set_eye_gaze(1, -0.25 * (i / 10.0), 0.0)
        time.sleep(0.03)
    center_samples.extend(wait(0.8))
    pursuit_start = report()
    pursuit_samples = [pursuit_start]
    next_sample = time.monotonic() + 0.5
    steps = 60
    for i in range(1, steps + 1):
        set_eye_gaze(1, -0.25 + 0.50 * (i / steps), 0.0)
        if time.monotonic() >= next_sample:
            check()
            snap = report()
            if snap:
                pursuit_samples.append(snap)
            next_sample = time.monotonic() + 0.5
        time.sleep(0.05)
    pursuit_samples.extend(wait(0.8))
    pursuit_end = report()
    pursuit_samples.append(pursuit_end)

    if len(pursuit_end.get('view_details', [])) != 2 or pursuit_end['apis'][1]['failures'] != baseline_failures:
        raise RuntimeError('NGX evaluation failure during smooth gaze pursuit')
    start_views = by_id(pursuit_start)
    end_views = by_id(pursuit_end)
    for vid in (0, 1):
        if vid not in start_views or vid not in end_views:
            raise RuntimeError('Missing eye view during smooth gaze pursuit')
        if end_views[vid]['evaluations'] <= start_views[vid]['evaluations'] or end_views[vid]['resets'] != start_views[vid]['resets']:
            raise RuntimeError(f'DLSS history stopped or reset on view {vid} during smooth pursuit')
        xs = [by_id(s)[vid]['crop_x'] for s in pursuit_samples if vid in by_id(s)]
        if any(xs[j] < xs[j - 1] for j in range(1, len(xs))) or end_views[vid]['crop_x'] <= start_views[vid]['crop_x']:
            raise RuntimeError(f'crop_x did not increase monotonically on view {vid} during smooth pursuit: {xs}')

    # 3. Large saccade step: settle at top-right extreme, then jump across the diagonal to bottom-left.
    # At 50x50, travel range is [-25%, +25%] (50% of renderW = 100% of center.inputW > 60% saccade threshold).
    set_eye_gaze(1, 0.80, 0.65)
    pre_saccade_samples = wait(0.8)
    pre_saccade = report()
    if len(pre_saccade.get('view_details', [])) != 2 or pre_saccade['apis'][1]['failures'] != baseline_failures:
        raise RuntimeError('NGX evaluation failure while settling before gaze saccade')
    if pre_saccade.get('gaze_state') != 4:
        raise RuntimeError(f'Expected gaze_state=4 before saccade, got {pre_saccade.get("gaze_state")}')
    set_eye_gaze(1, -0.80, -0.65)
    saccade_samples = wait(1.0)
    saccade = report()
    if len(saccade.get('view_details', [])) != 2 or saccade['apis'][1]['failures'] != baseline_failures:
        raise RuntimeError('NGX evaluation failure after gaze saccade step')
    if saccade.get('gaze_state') != 4:
        raise RuntimeError(f'Expected gaze_state=4 after saccade, got {saccade.get("gaze_state")}')
    pre_saccade_views = by_id(pre_saccade)
    saccade_views = by_id(saccade)
    for vid in (0, 1):
        if saccade_views[vid]['crop_x'] >= pre_saccade_views[vid]['crop_x']:
            raise RuntimeError(f'Saccade (-0.80, -0.65) did not shift crop_x left on view {vid}')
        if saccade_views[vid]['crop_y'] <= pre_saccade_views[vid]['crop_y']:
            raise RuntimeError(f'Saccade (-0.80, -0.65) did not shift crop_y down on view {vid}')
        reset_delta = saccade_views[vid]['resets'] - pre_saccade_views[vid]['resets']
        if not (1 <= reset_delta <= 2):
            raise RuntimeError(
                f'Expected 1..2 saccade resets on view {vid}: '
                f'before={pre_saccade_views[vid]["resets"]}, after={saccade_views[vid]["resets"]}'
            )

    # 4. Short blink (Enable=2 for ~0.18 s <= 350 ms hold): crop must stay at pre-blink position.
    pre_blink = {vid: (v['crop_x'], v['crop_y']) for vid, v in saccade_views.items()}
    set_eye_gaze(2, -0.80, -0.65)
    blink_samples = []
    blink_end = time.monotonic() + 0.18
    while time.monotonic() < blink_end:
        time.sleep(0.06)
        check()
        snap = report()
        if snap:
            blink_samples.append(snap)
    blink = report()
    if blink['apis'][1]['failures'] != baseline_failures or blink.get('gaze_state') not in (3, 4):
        raise RuntimeError(f'Unexpected state during short blink hold: gaze_state={blink.get("gaze_state")}')
    for vid, v in by_id(blink).items():
        if (v['crop_x'], v['crop_y']) != pre_blink[vid]:
            raise RuntimeError(f'Short blink snapped crop on view {vid}: {(v["crop_x"], v["crop_y"])} != {pre_blink[vid]}')

    # 5. Extended dropout (Enable=2 for ~1.5 s): decays to Inactive (2) and configured fallback offsets.
    dropout_samples = wait(1.5)
    dropout = report()
    deadline = time.monotonic() + 1.5
    while (dropout.get('gaze_state') != 2 or any(
        abs(v['offset_x_percent'] - a.offset_x) > 0.25 or abs(v['offset_y_percent'] - a.offset_y) > 0.25
        for v in dropout.get('view_details', [])
    )) and time.monotonic() < deadline:
        dropout_samples.extend(wait(0.5))
        dropout = report()
    set_eye_gaze(0, 0.0, 0.0)
    if dropout.get('gaze_state') != 2 or dropout['apis'][1]['failures'] != baseline_failures or len(dropout.get('view_details', [])) != 2:
        raise RuntimeError(f'Extended dropout did not reach Inactive fallback cleanly: gaze_state={dropout.get("gaze_state")}')
    for vid, v in by_id(dropout).items():
        if abs(v['offset_x_percent'] - a.offset_x) > 0.25 or abs(v['offset_y_percent'] - a.offset_y) > 0.25:
            raise RuntimeError(f'Fallback offset mismatch on view {vid}: ({v["offset_x_percent"]}, {v["offset_y_percent"]})')

    (a.out / 'eye-gaze.json').write_text(json.dumps({
        'center': center,
        'center_samples': center_samples,
        'pursuit_start': pursuit_start,
        'pursuit_samples': pursuit_samples,
        'pursuit_end': pursuit_end,
        'pre_saccade_samples': pre_saccade_samples,
        'pre_saccade': pre_saccade,
        'saccade': saccade,
        'saccade_samples': saccade_samples,
        'blink_samples': blink_samples,
        'blink': blink,
        'dropout_samples': dropout_samples,
        'dropout': dropout,
    }, indent=2))
    print('PASS eye-tracked OpenXR gaze: pursuit, saccade, blink hold, and fallback decay', flush=True)

try:
    if a.eye_gaze:
        write_handle = kernel_rw.OpenProcess(0x0028, False, a.pid)
        if not write_handle:
            raise c.WinError(c.get_last_error())
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
    if a.eye_gaze:
        eye_gaze_test()
    print('PASS fixed foveated simulation stages',flush=True)
finally:
    if write_handle:
        try:
            set_eye_gaze(0, 0.0, 0.0)
        except Exception as error:
            print('Eye-gaze override restore:', error, flush=True)
        kernel_rw.CloseHandle(write_handle)
        write_handle = None
    if ini.read_bytes() == last_written:
        ini.write_bytes(original)
    try:
        send(sim,pose)
    except Exception as error:
        print('Pose restore:',error,flush=True)
    reader.close()
    (a.out/'results.json').write_text(json.dumps({'pid':a.pid,'symbols':symbols,'stages':rows},indent=2))
    print('Original settings and simulator pose restored',flush=True)
