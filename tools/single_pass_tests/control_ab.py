"""Same-launch A/B of one exported uint32 control: frame rate and thread load, ABBA order.

Interleaving A,B,B,A (repeated) cancels slow drift in the scene (streaming, crowd, weather).
The control is restored to its original value in every case. Use with an owned launch created by
geometry_cost_live.py (scene chosen in between, e.g. by a CET teleport).
"""
import argparse
import json
from pathlib import Path
import statistics
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests'), str(HERE)]


def order(rounds):
    return ['A', 'B', 'B', 'A'] * rounds


TIMING = 'CyberpunkVR_PresentTiming'
TIMING_KEYS = ('presents', 'interval', 'gpuFinish', 'xrPump', 'dxgiPresent', 'hook')


def present_timing(before, after):
    """Mean milliseconds per game Present from two snapshots of CyberpunkVR_PresentTiming.
    `workMs` = interval minus the waits inside the Present hook: the frame's time outside them."""
    frequency = after[6] or before[6]
    count = after[0] - before[0]
    if not frequency or count <= 0:
        return None
    out = {key: (after[i] - before[i]) * 1000 / frequency / count for i, key in enumerate(TIMING_KEYS) if i}
    out['presents'] = count
    out['workMs'] = out['interval'] - out['gpuFinish'] - out['xrPump'] - out['dxgiPresent']
    out['outsideHookMs'] = out['interval'] - out['hook']
    return out


def _median(values):
    values = [v for v in values if v]
    return statistics.median(values) if values else None


def summarize(rows):
    """FPS plus frame-time costs. A change that removes CPU work can leave FPS flat when the frame
    is paced by something else (GPU, compositor); the CPU milliseconds the render workers and the
    game thread spend per frame (load x frame time) still show it."""
    groups = {label: [row for row in rows if row['label'] == label] for label in ('A', 'B')}
    out = {}
    for label, items in groups.items():
        fps = [row['frameRate']['fps'] for row in items]
        frame_ms = [1000 / f for f in fps]
        out[label] = dict(intervals=len(items), fpsMean=statistics.mean(fps), fpsMin=min(fps), fpsMax=max(fps),
                          frameMs=1000 / statistics.mean(fps),
                          gameThread=statistics.mean(row['threads']['gameThread'] or 0 for row in items),
                          workersTotal=statistics.mean(row['threads']['workersTotal'] for row in items),
                          gpuMs=_median(row['frameRate'].get('medianGpuMs') for row in items),
                          cpuMs=_median(row['frameRate'].get('medianFrameIntervalMs') for row in items),
                          workerCpuMsPerFrame=statistics.mean(row['threads']['workersTotal'] / 100 * ms
                                                              for row, ms in zip(items, frame_ms)),
                          gameThreadMsPerFrame=statistics.mean((row['threads']['gameThread'] or 0) / 100 * ms
                                                               for row, ms in zip(items, frame_ms)))
        timings = [row['present'] for row in items if row.get('present')]
        if timings:
            out[label]['present'] = {key: statistics.mean(t[key] for t in timings)
                                     for key in ('interval', 'gpuFinish', 'xrPump', 'dxgiPresent', 'hook', 'workMs', 'outsideHookMs')}
    a, b = out['A'], out['B']
    out['deltaFrameMs'] = b['frameMs'] - a['frameMs']
    out['deltaPercentFps'] = 100 * (b['fpsMean'] / a['fpsMean'] - 1)
    out['deltaWorkerCpuMsPerFrame'] = b['workerCpuMsPerFrame'] - a['workerCpuMsPerFrame']
    out['deltaGameThreadMsPerFrame'] = b['gameThreadMsPerFrame'] - a['gameThreadMsPerFrame']
    if a['gpuMs'] and b['gpuMs']:
        out['deltaGpuMs'] = b['gpuMs'] - a['gpuMs']
    if a.get('present') and b.get('present'):
        out['deltaPresent'] = {key: b['present'][key] - a['present'][key] for key in a['present']}
    # Ranges that do not overlap are the minimum evidence for a real difference.
    out['separated'] = b['fpsMin'] > a['fpsMax'] or a['fpsMin'] > b['fpsMax']
    worker = {label: [row['threads']['workersTotal'] / 100 * 1000 / row['frameRate']['fps'] for row in items]
              for label, items in groups.items()}
    out['workerCpuSeparated'] = min(worker['A']) > max(worker['B']) or min(worker['B']) > max(worker['A'])
    return out


def read_timing(reader, probe):
    import struct
    return struct.unpack('<8Q', reader.read(probe.address(TIMING), 64))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--control', required=True)
    parser.add_argument('--a', type=int, required=True)
    parser.add_argument('--b', type=int, required=True)
    parser.add_argument('--rounds', type=int, default=2)
    parser.add_argument('--seconds', type=float, default=6)
    parser.add_argument('--settle', type=float, default=2)
    args = parser.parse_args()
    import geometry_cost_live as live
    import geometry_cost_report as cost
    import launch_to_save as boot
    from live_read import Reader
    from live_symbols import resolve
    from second_view_cost import summarize_threads
    from thread_cpu_sample import measure as thread_measure
    state = live.load_state(args.state)
    pid = state['pid']
    game = boot.DEFAULT_GAME
    check = live.checker(pid, state['started'], state['baselineReporters'])
    symbols = resolve(pid, game / live.DLL, list(cost.NAMES) + [args.control, TIMING])
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    probe = cost.LiveProbe(reader, symbols, pid, check)
    original = probe.read32(args.control)
    ini = game / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
    original_ini = ini.read_bytes()
    overlay = next(line for line in original_ini.splitlines() if line.startswith(b'xr_framegen_overlay='))
    modified = original_ini.replace(overlay, b'xr_framegen_overlay=1', 1)
    rows = []
    try:
        probe.write32('CyberpunkVR_RuntimeDiagnostics', 1)
        ini.write_bytes(modified)
        time.sleep(3)
        for label in order(args.rounds):
            probe.write32(args.control, args.a if label == 'A' else args.b)
            time.sleep(args.settle)
            timing_before = read_timing(reader, probe)
            frames = probe.frames(args.seconds)
            timing = present_timing(timing_before, read_timing(reader, probe))
            threads = thread_measure(pid, 3)
            rows.append(dict(label=label, value=probe.read32(args.control), frameRate=cost.frame_rate(frames),
                             threads=summarize_threads(threads), present=timing))
            rate = rows[-1]['frameRate']
            print(json.dumps(dict(label=label, fps=round(rate['fps'], 2), frameMs=round(1000 / rate['fps'], 3),
                                  gpuMs=rate.get('medianGpuMs'), cpuMs=rate.get('medianFrameIntervalMs'),
                                  gameThread=round(rows[-1]['threads']['gameThread'] or 0, 1),
                                  workers=round(rows[-1]['threads']['workersTotal'], 1),
                                  workerMsPerFrame=round(rows[-1]['threads']['workersTotal'] / 100 * 1000 / rate['fps'], 2),
                                  present={k: round(v, 3) for k, v in (timing or {}).items()})), flush=True)
    finally:
        probe.write32(args.control, original)
        current = ini.read_bytes()
        ini.write_bytes(original_ini if current == modified else current.replace(b'xr_framegen_overlay=1', overlay, 1))
        restored = probe.read32(args.control)
        cleanup = probe.restore()
        reader.close()
        value = dict(pid=pid, control=args.control, a=args.a, b=args.b, original=original, restored=restored,
                     cleanup=cleanup, rows=rows)
        if len(rows) == 4 * args.rounds:
            value['summary'] = summarize(rows)
        args.out.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    if restored != original:
        raise RuntimeError('Control was not restored')
    print(json.dumps(value.get('summary'), indent=2), flush=True)


if __name__ == '__main__':
    main()
