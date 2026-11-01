"""Upper bound of CPU single-pass: frame rate and thread load with the second view on/off.

Single-pass can at most remove the second view's own preparation and recording. Turning the
VRCAM render-to-texture component off (the plugin's own bridge request, bridge/vrcam_enable.txt)
removes ALL of it: CPU culling/proxy/graph work and its GPU frame. The difference between the
intervals therefore bounds what any CPU/GPU sharing of the second view could gain in this scene.
The left eye shows MAIN while the view is off. Re-enabling the render-to-texture component at
runtime does NOT resume the view (measured in launch city-803), so main_only is the LAST interval
of a launch and the request file is restored to 1 for the next launch only.
"""
import argparse
import json
from pathlib import Path
import statistics
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'), str(HERE)]
import geometry_cost_report as cost  # noqa: E402
from thread_cpu_sample import measure as thread_measure  # noqa: E402

BRIDGE = 'bin/x64/plugins/cyber_engine_tweaks/mods/CyberpunkVRPort_Stereo/bridge'
INTERVALS = (('both_views', '1'), ('both_views_repeat', '1'), ('main_only', '0'))


def summarize_threads(value):
    rows = value['threads']
    game = [row for row in rows if row['name'] == 'GameThread']
    workers = [row for row in rows if row['name'].startswith('redDispatcher')]
    return dict(gameThread=game[0]['percentOfCore'] if game else None,
                workersTotal=sum(row['percentOfCore'] for row in workers),
                workersMean=statistics.mean(row['percentOfCore'] for row in workers) if workers else None,
                processTotal=value['total'])


def bound(results):
    """Gain bound: removing the whole second view vs keeping it (mean of both 'on' intervals)."""
    by = {row['label']: row for row in results}
    on = [by[name]['frameRate']['fps'] for name in ('both_views', 'both_views_repeat')]
    off = by['main_only']['frameRate']['fps']
    mean_on = statistics.mean(on)
    return dict(fpsBothViews=mean_on, fpsMainOnly=off, driftFps=max(on) - min(on),
                secondViewFrameCostMs=1000 / mean_on - 1000 / off if off > 0 and mean_on > 0 else None,
                maxGainPercent=100 * (off / mean_on - 1) if mean_on > 0 else None,
                gameThreadBothViews=statistics.mean(by[n]['threads']['gameThread'] or 0 for n in ('both_views', 'both_views_repeat')),
                gameThreadMainOnly=by['main_only']['threads']['gameThread'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, required=True, help='geometry_cost_live.py state of the owned launch')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=6)
    parser.add_argument('--settle', type=float, default=4)
    parser.add_argument('--stacks', type=float, default=0, help='Also sample GameThread call stacks for this many seconds per interval')
    args = parser.parse_args()
    import launch_to_save as boot
    from live_read import Reader
    from live_symbols import resolve
    import geometry_cost_live as live
    state = live.load_state(args.state)
    pid = state['pid']
    game = boot.DEFAULT_GAME
    check = live.checker(pid, state['started'], state['baselineReporters'])
    request = game / BRIDGE / 'vrcam_enable.txt'
    if request.read_text().strip() != '1':
        raise RuntimeError('Second view is not enabled at the start')
    symbols = resolve(pid, game / live.DLL, list(cost.NAMES) + ['CyberpunkVR_DebugVrcamNodeHits'])
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    probe = cost.LiveProbe(reader, symbols, pid, check)
    ini = game / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
    original_ini = ini.read_bytes()
    overlay = next(line for line in original_ini.splitlines() if line.startswith(b'xr_framegen_overlay='))
    modified = original_ini.replace(overlay, b'xr_framegen_overlay=1', 1)
    if b'xr_framegen=0' not in original_ini.splitlines():
        raise RuntimeError('Frame generation must stay off')
    results = []
    try:
        probe.write32('CyberpunkVR_RuntimeDiagnostics', 1)
        ini.write_bytes(modified)
        time.sleep(3)
        for label, value in INTERVALS:
            request.write_text(value)
            time.sleep(args.settle)
            hits = probe.read64('CyberpunkVR_DebugVrcamNodeHits')
            rows = probe.frames(args.seconds)
            threads = thread_measure(pid, 4)
            hits = probe.read64('CyberpunkVR_DebugVrcamNodeHits') - hits
            image = live.screenshot(args.out.with_name(f'{args.out.stem}-{label}.bmp'), check)
            stacks = None
            if args.stacks:
                import thread_stack_sampler as sampler
                tid = sampler.find_thread(pid, 'GameThread')
                raw, mods, failures, suspended, symbols = sampler.sample_stacks(pid, tid, args.stacks, 50, 64)
                named = sampler.symbolize(raw, mods, sampler.pdata_starts(game / 'bin/x64/Cyberpunk2077.exe'), 'Cyberpunk2077.exe', symbols)
                stacks = dict(failures=failures, suspendedFraction=suspended / args.stacks, **sampler.aggregate(named),
                              stacks=[' <- '.join(s[:32]) for s in named])
            results.append(dict(label=label, request=value, vrcamNodeHits=hits, frameRate=cost.frame_rate(rows),
                                threads=summarize_threads(threads), topThreads=threads['threads'][:20], image=image, gameThreadStacks=stacks))
            print(json.dumps(dict(label=label, fps=round(results[-1]['frameRate']['fps'], 2), vrcamNodeHits=hits,
                                  threads=results[-1]['threads']), indent=1), flush=True)
    finally:
        request.write_text('1')
        current = ini.read_bytes()
        ini.write_bytes(original_ini if current == modified else current.replace(b'xr_framegen_overlay=1', overlay, 1))
        cleanup = probe.restore()
        reader.close()
        args.out.write_text(json.dumps(dict(pid=pid, cleanup=cleanup, results=results), indent=2) + '\n', encoding='utf-8')
    hits = [row['vrcamNodeHits'] for row in results]
    if len(hits) != 3 or hits[0] == 0 or hits[1] == 0 or hits[2] != 0:
        raise RuntimeError(f'Second view did not follow the request: {hits}')
    value = dict(pid=pid, bound=bound(results), cleanup=cleanup, results=results)
    args.out.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(value['bound'], indent=2), flush=True)


if __name__ == '__main__':
    main()
