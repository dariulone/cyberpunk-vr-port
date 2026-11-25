"""Bounded in-game VRS on/off comparison; restores the original INI in finally.

Stand still in a loaded save, with the headset on and all menus closed. Samples
the plugin's exported counters read-only; changes only xr_vrs in its watched INI.
This measures game Present intervals, not headset refresh rate or isolated GPU
cost. An uncontrolled change of scene invalidates a causal FPS comparison.
"""
import argparse
import json
from pathlib import Path
import re
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
MODE = re.compile(rb'(?m)^([ \t]*xr_vrs[ \t]*=[ \t]*)([012])([ \t]*\r?)$')


def set_mode(data, value):
    if len(MODE.findall(data)) != 1:
        raise ValueError('Expected exactly one plain xr_vrs setting')
    return MODE.sub(lambda m: m[1] + str(value).encode('ascii') + m[3], data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', required=True, type=int)
    parser.add_argument('--seconds', type=float, default=8)
    parser.add_argument('--game', type=Path, default=Path(r'C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk 2077'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not 2 <= args.seconds <= 15:
        parser.error('Use a sample length between 2 and 15 seconds')
    if args.out.exists():
        parser.error('Use a new output directory')
    sys.path[:0] = [str(ROOT/'tools/swimming_tests'), str(ROOT/'tools/roomscale_tests')]
    from live_symbols import resolve
    from live_read import Reader
    plugin = args.game/'red4ext/plugins/cyberpunkvrport'
    ini = plugin/'config/vrport.ini'
    original = ini.read_bytes()
    if set_mode(original, 1) != original or not re.search(rb'(?m)^xr_vrs_eye_tracked\s*=\s*1\s*$', original):
        raise RuntimeError('Start with eye-tracked VRS enabled at 2x2')
    names = ['CyberpunkVR_FixedVrsReport', 'CyberpunkVR_FixedVrsReportSeq', 'CyberpunkVR_PresentTiming']
    symbols = resolve(args.pid, plugin/'cyberpunkvrport.dll', names)
    reader = Reader(args.pid, args.game/'bin/x64/Cyberpunk2077.exe', 0, 0)
    args.out.mkdir(parents=True)
    (args.out/'vrport.before.ini').write_bytes(original)
    rows = []
    last_written = original
    result = {'pid': args.pid, 'dll_sha256': symbols['sha256'], 'rows': rows,
              'method': 'on/off/off/on; 3-second settling; game Present intervals'}

    def read(name, size):
        return reader.read(int(symbols[name], 16), size)

    def report():
        for _ in range(40):
            seq = read(names[1], 4)
            if struct.unpack('<I', seq)[0] & 1:
                time.sleep(.005)
                continue
            body = read(names[0], 1024).split(b'\0', 1)[0]
            if seq == read(names[1], 4):
                return json.loads(body)
        raise RuntimeError('VRS diagnostic remained busy')

    def timing():
        return struct.unpack('<8Q', read(names[2], 64))

    def wait_applied(mode):
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline:
            current = report()
            if current['requested'] == mode and current['applied'] == mode:
                return current
            time.sleep(.25)
        raise RuntimeError(f'Runtime did not apply xr_vrs={mode}')

    try:
        # Returning from the desktop or a blink can make the first sample stale.
        # Give the headset a bounded settling period before any setting change.
        deadline = time.monotonic() + 20
        while True:
            initial = report()
            result['initial'] = initial
            if initial['failed'] or not initial['supported']:
                raise RuntimeError('VRS support is required before testing')
            if initial['gaze_fresh']:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError('No fresh gaze within 20 seconds; keep headset on and close dashboards')
            time.sleep(.25)
        for label, mode in [('on', 1), ('off', 0), ('off', 0), ('on', 1)]:
            if ini.read_bytes() != last_written:
                raise RuntimeError('Configuration changed externally; comparison stopped')
            last_written = set_mode(last_written, mode)
            ini.write_bytes(last_written)
            wait_applied(mode)
            time.sleep(3)
            before = report()
            start = timing()
            gaze_samples = []
            deadline = time.monotonic() + args.seconds
            while time.monotonic() < deadline:
                time.sleep(min(.5, max(0, deadline-time.monotonic())))
                gaze_samples.append(report()['gaze_fresh'])
            finish = timing()
            after = report()
            frames = finish[0] - start[0]
            if frames <= 0 or not finish[6]:
                raise RuntimeError('Game stopped presenting frames')
            duration_ms = (finish[1]-start[1])*1000/finish[6]
            delta = lambda key: [after[key][i]-before[key][i] for i in range(2)]
            row = {'mode': label, 'frames': frames, 'duration_ms': duration_ms,
                   'frame_ms': duration_ms/frames, 'fps': frames*1000/duration_ms,
                   'vrs_draws': delta('draws'), 'scene_draws': delta('scene'),
                   'eligible_draws': delta('eligible'),
                   'fresh_gaze_fraction': sum(gaze_samples)/len(gaze_samples),
                   'before': before, 'after': after}
            rows.append(row)
            print(json.dumps({key: row[key] for key in ['mode', 'fps', 'frame_ms', 'vrs_draws', 'fresh_gaze_fraction']}), flush=True)
            (args.out/'result.json').write_text(json.dumps(result, indent=2))
            if mode and not sum(row['vrs_draws']):
                raise RuntimeError('No new foveated draws during the on sample; cumulative counters do not prove current activity')
            if mode and row['fresh_gaze_fraction'] < .8:
                raise RuntimeError('Insufficient fresh gaze during the on sample; comparison invalid')
            if not mode and sum(row['vrs_draws']):
                raise RuntimeError('VRS draws continued while off; comparison invalid')
        summary = {}
        for label in ('on', 'off'):
            group = [r for r in rows if r['mode'] == label]
            ms = sum(r['duration_ms'] for r in group)/sum(r['frames'] for r in group)
            summary[label] = {'frame_ms': ms, 'fps': 1000/ms,
                              'min_phase_fps': min(r['fps'] for r in group),
                              'max_phase_fps': max(r['fps'] for r in group)}
        summary['fps_change_percent'] = 100*(summary['on']['fps']/summary['off']['fps']-1)
        result['summary'] = summary
        print(json.dumps(summary, indent=2), flush=True)
    except BaseException as error:
        result['error'] = str(error)
        raise
    finally:
        try:
            current = ini.read_bytes()
            if current == last_written:
                ini.write_bytes(original)
                result['restoration'] = 'Original INI restored byte for byte'
            else:
                # Preserve other settings if the user saved the menu during the
                # test. Restore only the setting this probe temporarily changed.
                ini.write_bytes(set_mode(current, 1))
                result['restoration'] = 'Restored xr_vrs=1; preserved other external edits'
            result['restored_runtime'] = wait_applied(1)
        finally:
            reader.close()
            (args.out/'result.json').write_text(json.dumps(result, indent=2))
            print(result.get('restoration', 'Restoration needs checking'), flush=True)


if __name__ == '__main__':
    main()
