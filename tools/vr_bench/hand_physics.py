"""Measure hand physics off/on in an already running disposable OpenXR Simulator session."""
import argparse
from contextlib import ExitStack
from datetime import datetime
import hashlib
import json
import math
from pathlib import Path
import statistics
import struct
import sys
import time

import vr_bench as bench
import storefront_post

sys.path[:0] = [str(bench.ROOT / 'tools' / name) for name in ('body_rotation_tests', 'sim_harness')]
from live_symbols import loaded_module
from live_types import describe
from simulator_probe import await_pose, read_status, send
from simxr import Sim

RATE = 10
SCALARS = ('stepsSeen', 'beforeUs', 'afterUs', 'fingerUs', 'fingerSolves', 'fingerCalls', 'fingerContacts', 'nearCycles')
SIDED = ('touchedSteps', 'worldSteps')
COUNTERS = (*SCALARS, *(n + side for n in SIDED for side in ('Left', 'Right')))
HANDS = 'cvr::physhands::'
RINGS = ('CyberpunkVR_FrameRingCpu', 'CyberpunkVR_FrameRingGpu')
SYMBOLS = (*RINGS, 'CyberpunkVR_FrameRingCount', 'CyberpunkVR_XrProjectionCycles',
           'CyberpunkVR_XrRepeatCycles', 'CyberpunkVR_PlayerEntityPos', 'CyberpunkVR_PlayerEntityValid',
           'g_liveControls', 'g_menuModeValue', 'g_isInVehicle', HANDS + 'live', *(HANDS + n for n in SCALARS + SIDED))

# Half-second keyframes, interpolated at fixed absolute deadlines. Yaw is a
# simulator-space offset in degrees. Hand positions are body offsets in metres.
REST = (0, -.22, -.38, -.30, .22, -.38, -.30)
KEYS = (REST, (4, -.29, -.28, -.39, .18, -.34, -.43),
        (10, -.35, -.22, -.48, .11, -.29, -.52), (4, -.25, -.30, -.40, .18, -.34, -.42),
        REST, (-4, -.16, -.34, -.44, .27, -.29, -.40),
        (-10, -.11, -.27, -.52, .35, -.22, -.48), REST, REST)


def trace(base, cycles):
    frames = []
    for tick in range(cycles * (len(KEYS) - 1) * (RATE // 2)):
        segment, step = divmod(tick, RATE // 2)
        i = segment % (len(KEYS) - 1)
        yaw, *xyz = [a + (b - a) * step / (RATE // 2) for a, b in zip(KEYS[i], KEYS[i + 1])]
        frames.append(dict(t=tick / RATE, head=dict(base, yaw=base['yaw'] + math.radians(yaw)),
                           left=xyz[:3], right=xyz[3:]))
    return frames


def settings_digest(paths):
    return {str(p): hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None for p in paths}


def phase_state(game):
    return (game.i32('physicsMaster'), tuple(game.read(game.addr(HANDS + 'live'), 2)))


def wait_for_phase(game, enabled):
    end = time.perf_counter() + 10
    while phase_state(game) != (enabled, (enabled, enabled)):
        if time.perf_counter() >= end:
            raise RuntimeError(f'Physics master and both live hand actors failed to settle to {enabled}')
        time.sleep(.05)


def pose(sim, frame):
    for side in ('left', 'right'):
        sim.hand(side, pos=frame[side], quat=frame.get(side+'Quat',(0,0,0,1)), mode='body')
    # The regular simulator consumes head_pose_command in xrWaitFrame.
    send(bench.SIM, frame['head'])


def position(game):
    return struct.unpack('<3f', game.read(game.addr('CyberpunkVR_PlayerEntityPos'), 12))


def on_foot(game):
    return (game.i32('g_menuModeValue') == 0 and game.i32('CyberpunkVR_PlayerEntityValid') != 0
            and game.read(game.addr('g_isInVehicle'), 1) == b'\0')


def sample_counters(game):
    return ([game.u64(HANDS + name) for name in SCALARS] +
            [game.u64(HANDS + name, side) for name in SIDED for side in (0, 1)])


def physics_cost(samples):
    # The physics worker drains these counters on its own logging interval.
    # Retain the snapshots and exclude intervals crossing an observed reset.
    sums = [0] * len(COUNTERS)
    resets = 0
    for first, last in zip(samples, samples[1:]):
        delta = [b - a for a, b in zip(first, last)]
        if min(delta) < 0:
            resets += 1
        else:
            sums = [a + b for a, b in zip(sums, delta)]
    steps, before, after, fingers, solves = sums[:5]
    return dict(zip(COUNTERS, sums), resetIntervals=resets,
                beforeUsPerStep=before / steps if steps else None,
                afterUsPerStep=after / steps if steps else None,
                fingerUsPerSolve=fingers / solves if solves else None,
                fingerCallsPerSolve=sums[5] / solves if solves else None)


def read_window(game, start, end):
    values = []
    for name, a, b in zip(RINGS, start, end):
        if not 0 < b - a <= 8192:
            raise RuntimeError(f'{name}: missing samples, counter reset or ring overrun ({a} to {b}). '
                               'Check the FPS overlay and GPU timing.')
        samples = game.ring(name, a, b)
        if not all(math.isfinite(v) and v > 0 for v in samples):
            raise RuntimeError(f'{name}: invalid frame times')
        values.append(samples)
    if any(now < b or now - a > 8192 for now, a, b in zip(game.ring_counts(), start, end)):
        raise RuntimeError('Frame ring reset or overwritten while reading')
    return values


def window(game, sim, frames, enabled, anchor, hz, clocks, post=None):
    game.set32('physicsMaster', enabled)
    pose(sim, frames[0])
    wait_for_phase(game, enabled)
    time.sleep(1.5)
    post_start = None
    if post:
        post_start = post.timed_start(sim,frames[0]['head'],pose,enabled)
    delivery = []
    contacts = []
    counters = [sample_counters(game)]
    xr0 = bench.xr_cycles(game)
    issues = set()
    max_drift = 0
    observed_head = []
    last_status_frame = read_status(bench.SIM)['frame_count']
    gpu_start = len(clocks.rows)
    start = game.ring_counts()
    begin = time.perf_counter()
    for i, frame in enumerate(frames):
        time.sleep(max(0, begin + frame['t'] - time.perf_counter()))
        sent = time.perf_counter() - begin
        pose(sim, frame)
        consumed = time.perf_counter() - begin
        delivery.append([sent, consumed])
        if consumed - frame['t'] >= 1 / RATE:
            issues.add('pose missed its delivery interval')
        if phase_state(game) != (enabled, (enabled, enabled)) or not on_foot(game):
            issues.add('master, hand actors or gameplay state changed')
        for side in ('left', 'right'):
            mode, xyz, rot = sim.pose(side)
            if mode != 'body' or math.dist(xyz, frame[side]) > .001 or math.dist(rot, frame.get(side+'Quat',(0,0,0,1))) > .001:
                issues.add('simulator controller pose overwritten')
        max_drift = max(max_drift, math.dist(position(game), anchor))
        counters.append(sample_counters(game))
        if post:
            contacts.append(post.sample(enabled))
        # runtime_status is published less frequently than pose commands.
        # Record fresh observations only. A status update can precede this tick's command.
        status = bench.sim_status()
        if 'frame_count' in status and status['frame_count'] != last_status_frame:
            last_status_frame = status['frame_count']
            head = status['head_tracking']
            observed_head.append(dict(t=time.perf_counter() - begin, frame=last_status_frame, pose=head))
            recent = frames[max(0, i - 2):i + 1]
            if (min(abs(math.remainder(head['yaw'] - f['head']['yaw'], math.tau)) for f in recent) > .003
                    or any(abs(head[k] - frame['head'][k]) > .003 for k in ('pitch', 'roll'))
                    or any(abs(head['position'][k] - frame['head'][k]) > .003 for k in ('x', 'y', 'z'))):
                issues.add('observed head diverged from commanded trace')
    duration = len(frames) / RATE
    time.sleep(max(0, begin + duration - time.perf_counter()))
    end = game.ring_counts()
    elapsed = time.perf_counter() - begin
    gpu_clocks = [r[2] for r in clocks.rows[gpu_start:] if len(r) > 2]
    xr1 = bench.xr_cycles(game)
    cpu, gpu = read_window(game, start, end)
    stats = bench.frame_stats(cpu, gpu, 1000 / hz)
    if bench.stalled(stats, duration) or abs(sum(cpu) / 1000 - elapsed) > max(.15, .05 * duration):
        issues.add('real-frame coverage or cadence outside measurement window')
    if len(gpu) < 10:
        issues.add('fewer than ten GPU samples')
    if max_drift > .10:
        issues.add('player moved over 10 cm from initial save position')
    if not any(abs(o['pose']['yaw'] - frames[0]['head']['yaw']) > .02 for o in observed_head):
        issues.add('HMD motion unconfirmed in simulator status')
    if read_status(bench.SIM)['target_fps'] != hz:
        issues.add('display rate changed')
    if xr0 is None or xr1 is None:
        raise RuntimeError('XR display counters unavailable')
    cycles, repeats = (b - a for a, b in zip(xr0, xr1))
    if cycles <= 0 or not 0 <= repeats <= cycles:
        issues.add('XR display counters invalid')
    cost = physics_cost(counters)
    extra = {}
    if post:
        cycles_loaded, problems = storefront_post.assess(frames, contacts, enabled, cost)
        issues.update(problems)
        extra.update(postStart=post_start, postCycles=cycles_loaded, postSamples=contacts)
    return dict(variant='B' if enabled else 'A', seconds=elapsed, ringStart=start, ringEnd=end,
                cpuMs=cpu, gpuMs=gpu, stats=stats, displayCycles=cycles, repeatCycles=repeats,
                playerDriftM=max_drift, deliverySeconds=delivery, observedHead=observed_head, counterSamples=counters,
                physicsCost=cost, gpuSmClockMHz=gpu_clocks, issues=sorted(issues), **extra)


def paired_results(rows):
    blocks = []
    excluded = []
    for first in range(0, len(rows), 4):
        group = rows[first:first + 4]
        reasons = sorted({reason for r in group for reason in r['issues']})
        if [r['variant'] for r in group] != list('ABBA'):
            reasons.append('incomplete ABBA block')
        else:
            # Times are relative to the same planned key. Compare like keys
            # across arms, including the delay for the simulator to consume the pose.
            skew = sorted(abs((a[1] + d[1] - b[1] - c[1]) / 2)
                          for a, b, c, d in zip(*(r['deliverySeconds'] for r in group), strict=True))
            if skew[int(.95 * (len(skew) - 1))] > .025:
                reasons.append('paired head delivery p95 skew over 25 ms')
            if 'postSamples' in group[0]:
                for a,b in ((group[0],group[1]),(group[3],group[2])):
                    if math.dist(a['postStart']['foot'],b['postStart']['foot'])>.02:
                        reasons.append('paired starting feet differ by over 2 cm')
                    paired=zip(a['postSamples'],b['postSamples'],strict=True)
                    if any(math.dist(x['body']['foot'],y['body']['foot'])>.03 for x,y in paired):
                        reasons.append('paired foot paths differ by over 3 cm')
                    if any(storefront_post.heading_error(x['body']['quat'],
                           storefront_post.forward(y['body']['quat']))>2 for x,y in
                           zip(a['postSamples'],b['postSamples'],strict=True)):
                        reasons.append('paired headings differ by over 2 degrees')
        if reasons:
            excluded.append(dict(block=first // 4 + 1, reasons=reasons))
            continue
        off = [r['stats'] for r in group if r['variant'] == 'A']
        on = [r['stats'] for r in group if r['variant'] == 'B']
        blocks.append({'block': first // 4 + 1, **{
            key: statistics.mean(v[key] for v in on) - statistics.mean(v[key] for v in off)
            for key in ('fps', 'cpuMean', 'cpuP95', 'cpuP99', 'gpuMean', 'gpuP95', 'low1')}})
    return dict(onMinusOff=blocks, excluded=excluded, medianDelta={k: statistics.median(b[k] for b in blocks)
                for k in blocks[0] if k != 'block'} if blocks else {})


def run(args):
    exe = loaded_module(args.pid, 'Cyberpunk2077.exe')
    dll = loaded_module(args.pid, 'cyberpunkvrport.dll')
    runtime = loaded_module(args.pid, 'openxr_simulator.dll')
    if not all((exe, dll, runtime)):
        raise RuntimeError('PID must be a running Cyberpunk 2077 OpenXR Simulator session')
    post_case = args.case == 'storefront-post'
    if post_case and not args.post_generator.is_file():
        raise RuntimeError(f'Build post_trace before using storefront-post: {args.post_generator}')
    layout = describe(dll, storefront_post.TYPES if post_case else ())
    fields = layout['LiveControls']['fields']
    paths = [dll.parent / 'config' / name for name in ('vrport.ini', 'physhands.ini')] + [bench.USER_SETTINGS]
    initial_settings = settings_digest(paths)
    status = read_status(bench.SIM)
    head = status['head_tracking']
    base = {**head['position'], **{k: head[k] for k in ('yaw', 'pitch', 'roll')}}
    hz = float(status['target_fps'])
    if not math.isfinite(hz) or hz <= 0:
        raise RuntimeError('Simulator display rate unavailable')
    frames = [] if post_case else trace(base, args.cycles)
    report = dict(pid=args.pid, case=args.case, saveLabel=args.save, executable=str(exe), plugin=layout['sha256'],
                  simulator=settings_digest([runtime]), settings=initial_settings, displayHz=hz,
                  portOptions=bench.ini_values({'xr_framegen', 'xr_fps_cap', 'xr_framegen_pace',
                                                'xr_framegen_overlay', 'xr_hand_collision'}, paths[0]),
                  initialHead=base, rate=RATE, trace=frames, counterNames=COUNTERS, warmup=[], rows=[])
    args.out.mkdir(parents=True, exist_ok=False)
    path = args.out / 'hand-physics.json'

    def save():
        tmp = path.with_suffix('.tmp')
        tmp.write_text(json.dumps(report, allow_nan=False), encoding='utf-8')
        tmp.replace(path)

    try:
        with ExitStack() as cleanup:
            game = bench.Game(args.pid, dll=dll, symbols=SYMBOLS + (storefront_post.SYMBOLS if post_case else ()))
            cleanup.callback(game.close)
            post = storefront_post.Post(game,layout) if post_case else None
            if post:
                post.restage_wait = args.post_restage_wait
            if post and (error := post.placement(post.body())):
                raise RuntimeError(error)
            for alias, field in [('physicsMaster', 'xrHandPhysics'), ('collision', 'xrHandCollision')]:
                game.symbols[alias] = hex(game.addr('g_liveControls') + fields[field]['offset'])
            original = game.i32('physicsMaster')
            if original not in (0, 1) or game.i32('collision') != 1 or not on_foot(game):
                raise RuntimeError('Enter gameplay with hand collision enabled')
            sim = Sim(args.pid, focus_away=False)
            cleanup.callback(game.k.CloseHandle, sim.h)
            cleanup.callback(sim.restore)
            cleanup.callback(await_pose, bench.SIM, base)
            cleanup.callback(send, bench.SIM, base)
            cleanup.callback(game.set32, 'physicsMaster', original)
            sim.hands_follow_head(not post_case)
            anchor = position(game)
            report.update(originalMaster=original, anchor=anchor, harness=sim.version)
            # Fail early if the already-loaded session is not publishing real-frame metrics.
            if not (post_case and args.calibration_only):
                start = game.ring_counts()
                time.sleep(1)
                read_window(game, start, game.ring_counts())
            if post:
                if sim.version < 3 or not sim.body_state:
                    raise RuntimeError('Post trace requires the simulator body-yaw harness')
                sim.body_yaw(0)
                game.set32('physicsMaster', 1)
                wait_for_phase(game, 1)
                def record_calibration(info):
                    report['postCalibration'] = info
                    save()
                    trial = info['attempts'][-1]
                    print(f"post {trial['label']}  dwell {trial['cycles'][0]['steadyRun']}  "
                          f"released {trial['cycles'][0]['released']}  {', '.join(trial['issues']) or 'OK'}", flush=True)
                frames, calibration = post.calibrate(sim,base,args.cycles,args.post_generator,pose,
                                                     lambda: game.u64(HANDS+'worldSteps',1),record_calibration,
                                                     args.post_attempts)
                anchor = post.timed_foot
                report.update(trace=frames, postCalibration=calibration, anchor=anchor)
                if args.calibration_only:
                    return 0
            clocks = bench.GpuSampler()
            clocks.start()
            cleanup.callback(clocks.stop)
            plan = [('warmup', v) for v in (0, 1)] + [('rows', v) for v in (0, 1, 1, 0) * args.blocks]
            for kind, enabled in plan:
                row = window(game, sim, frames, enabled, anchor, hz, clocks, post)
                report[kind].append(row)
                if settings_digest(paths) != initial_settings:
                    row['issues'].append('configuration fingerprint changed')
                    raise RuntimeError('Game or port configuration changed during the benchmark')
                save()
                print(f"{kind} {row['variant']}  {row['stats']['fps']:.2f} real FPS  "
                      f"CPU p95 {row['stats']['cpuP95']:.2f} ms  GPU p95 {row['stats']['gpuP95']:.2f} ms  "
                      f"{', '.join(row['issues']) or 'OK'}", flush=True)
                if post and kind == 'warmup' and enabled and any(c['steadyRun']<3 or c['released']<2 for c in row['postCycles']):
                    raise RuntimeError('Post warmup did not establish repeatable loaded contact and release; see postSamples')
    except BaseException as exc:
        report['error'] = f'{type(exc).__name__}: {exc}'
        raise
    finally:
        report['paired'] = paired_results(report['rows'])
        save()
        print(path, flush=True)
    print('Median full-block change, physics on minus off:', report['paired']['medianDelta'])
    return 0 if len(report['paired']['onMinusOff']) == args.blocks else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True, help='existing gameplay process')
    parser.add_argument('--save', required=True, help='label of the already loaded save')
    parser.add_argument('--blocks', type=int, default=3, help='number of complete ABBA blocks')
    parser.add_argument('--cycles', type=int, default=2, help='four-second motion cycles per window')
    parser.add_argument('--case', choices=('reach', 'storefront-post'), default='reach')
    parser.add_argument('--calibration-only', action='store_true', help='qualify two separate post presses and exit before ABBA')
    parser.add_argument('--post-attempts', type=int, choices=range(1,8), default=5,
                        help='maximum four-second refinement profiles before giving up')
    parser.add_argument('--post-restage-wait', type=float, default=0.,
                        help='seconds to allow manual repositioning before a mismatched window, with hands retracted')
    parser.add_argument('--post-generator', type=Path,
                        default=bench.ROOT/'build/vr-bench-post/Release/post_trace.exe')
    parser.add_argument('--out', type=Path, default=bench.OUT_ROOT / datetime.now().strftime('%Y%m%d-%H%M%S-hand-physics'))
    args = parser.parse_args()
    if args.calibration_only and args.case != 'storefront-post':
        parser.error('--calibration-only requires --case storefront-post')
    if not 0 <= args.post_restage_wait <= 120:
        parser.error('--post-restage-wait must be between 0 and 120 seconds')
    if args.pid <= 0 or args.blocks < 1 or not 1 <= args.cycles <= 8:
        parser.error('use a positive PID and block count, and 1 to 8 cycles')
    return run(args)


if __name__ == '__main__':
    sys.exit(main())
