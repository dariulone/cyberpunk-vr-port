"""Bounded simulator HUD motion check; read-only process access, exact pose restoration.

The installed and local DLL must match. No keys, focus changes, config edits or
game-memory writes. The separate simulator's documented pose channel supplies motion.
"""
import argparse
import json
import math
import os
from pathlib import Path
import struct
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT/'tools/roomscale_tests'), str(ROOT/'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve
from simulator_probe import read_status, send, await_pose


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--dll', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--game', type=Path, default=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077'))
    a = p.parse_args()
    if a.out.exists() or not a.out.parent.is_dir():
        raise RuntimeError('Fresh output under an existing parent required')
    log = (a.game/'red4ext/plugins/cyberpunkvrport/log/cyberpunkvrport.log').read_text(encoding='utf-8', errors='replace')
    if 'OpenXR runtime name="OpenXR Simulator Runtime"' not in log:
        raise RuntimeError('Current game must use OpenXR Simulator')
    symbols = resolve(a.pid, a.dll.resolve(), ['CyberpunkVR_HudPoseDebug', 'CyberpunkVR_HudPoseDebugSeq', 'CyberpunkVR_RuntimeDiagnostics'])
    directory = Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator'
    initial = read_status(directory)
    if time.time()-(directory/'runtime_status.json').stat().st_mtime > 5:
        raise RuntimeError('Simulator status is stale')
    h = initial['head_tracking']
    original = dict(h['position'], **{key:h[key] for key in ('yaw','pitch','roll')})
    reader = Reader(a.pid, a.game/'bin/x64/Cyberpunk2077.exe', 0, 0)
    if not struct.unpack('<i', reader.read(int(symbols['CyberpunkVR_RuntimeDiagnostics'], 16), 4))[0]:
        reader.close()
        raise RuntimeError('Runtime diagnostics are disabled; enable DEBUG or CyberpunkVR_RuntimeDiagnostics before this probe')
    address = int(symbols['CyberpunkVR_HudPoseDebug'], 16)
    sequence = int(symbols['CyberpunkVR_HudPoseDebugSeq'], 16)
    stop = threading.Event()
    rows, errors = [], []
    phase = 'baseline'

    def sample():
        for _ in range(100):
            before = struct.unpack('<I', reader.read(sequence, 4))[0]
            if before & 1:
                continue
            v = struct.unpack('<Qq4f3i9f', reader.read(address, 80))
            after = struct.unpack('<I', reader.read(sequence, 4))[0]
            if before == after and not after & 1:
                return dict(frame=v[0], display_time=v[1], head_yaw=v[2], panel_yaw=v[3], dt=v[4],
                            eye_error=v[5], mode=v[6], tracked=v[7], layers=v[8],
                            head=v[9:12], left=v[12:15], right=v[15:18], phase=phase)
        raise RuntimeError('HUD diagnostic remained busy')

    def record():
        try:
            while not stop.is_set():
                row = sample()
                if not rows or row['frame'] != rows[-1]['frame']:
                    rows.append(row)
                    if row['mode'] != 0 or not row['tracked'] or row['layers'] != 2:
                        raise RuntimeError('Expected tracked head-follow HUD with two angular layers')
                stop.wait(.002)
        except BaseException as error:
            errors.append(str(error))
            stop.set()

    current = dict(original)
    def sweep(target, label, duration=2):
        nonlocal current, phase
        phase = label
        start_pose = dict(current)
        start = time.monotonic()
        while True:
            if errors:
                raise RuntimeError(errors[-1])
            t = min(1, (time.monotonic()-start)/duration)
            u = t*t*(3-2*t)
            current = {key:start_pose[key]+(target[key]-start_pose[key])*u for key in original}
            send(directory, current)
            if t == 1:
                break
        await_pose(directory, target)

    result = dict(symbols=symbols, initial=initial, original=original, focus_changed=False, keys_sent=False)
    thread = threading.Thread(target=record, daemon=True)
    try:
        initial_sample = sample()
        if abs(math.remainder(initial_sample['head_yaw']-original['yaw'], 2*math.pi)) > .02:
            raise RuntimeError('Simulator status and live head yaw disagree before commands')
        thread.start()
        time.sleep(.6)
        for label, delta in [('right',1.4), ('left',-1.4)]:
            target = dict(original, yaw=original['yaw']+delta, pitch=original['pitch']+.12,
                          x=original['x']+.025*delta/1.4, y=original['y']-.025)
            sweep(target, label, 2.5)
            phase = label+'_held'
            time.sleep(1)
        sweep(original, 'return', 2.5)
        phase = 'settled'
        time.sleep(1.2)
    except BaseException as error:
        result['error'] = str(error)
    finally:
        try:
            send(directory, original)
            result['restored'] = await_pose(directory, original)
        except BaseException as error:
            result['cleanup_error'] = str(error)
        stop.set()
        if thread.is_alive():
            thread.join(5)
        reader.close()
    result['samples'] = rows
    result['sampler_errors'] = errors
    if rows:
        pairs = [(b,c) for b,c in zip(rows, rows[1:]) if c['frame'] == b['frame']+1]
        wrap = lambda x:math.remainder(x, 2*math.pi)
        settled = [r for r in rows if r['phase']=='settled']
        settled = settled[len(settled)//2:]
        stats = dict(samples=len(rows), consecutive_pairs=len(pairs),
            yaw_range_deg=math.degrees(max(r['head_yaw'] for r in rows)-min(r['head_yaw'] for r in rows)),
            panel_range_deg=math.degrees(max(r['panel_yaw'] for r in rows)-min(r['panel_yaw'] for r in rows)),
            max_eye_error_m=max(r['eye_error'] for r in rows),
            nonpositive_dt=sum(r['dt']<=0 for r in rows),
            max_clock_error_s=max((abs(c['dt']-min(.05,(c['display_time']-b['display_time'])*1e-9)) for b,c in pairs), default=0),
            max_panel_speed_rad_s=max((abs(wrap(c['panel_yaw']-b['panel_yaw']))/c['dt'] for b,c in pairs if c['dt']>0),default=0),
            settled_yaw_drift_deg=math.degrees(max((abs(wrap(r['panel_yaw']-settled[0]['panel_yaw'])) for r in settled),default=0)))
        result['stats'] = stats
        result['checks'] = dict(nonzero_motion=stats['yaw_range_deg']>150 and stats['panel_range_deg']>100,
            same_eye_directions=stats['max_eye_error_m']<1e-6,
            precise_clock=stats['max_clock_error_s']<1e-7 and stats['nonpositive_dt']==0,
            bounded_slew=stats['max_panel_speed_rad_s']<3.001,
            stable_when_held=stats['settled_yaw_drift_deg']<.001,
            coherent=not errors and len(pairs)>200,
            restored='restored' in result and 'cleanup_error' not in result)
    a.out.write_text(json.dumps(result, indent=2), encoding='utf-8')
    print(json.dumps({k:v for k,v in result.items() if k not in ('samples','symbols','initial','restored')}, indent=2))
    return 0 if result.get('checks') and all(result['checks'].values()) and not result.get('error') else 1


if __name__ == '__main__':
    raise SystemExit(main())
