"""Offline tests of the benchmark harness (no game): py -3.13 -B -m unittest tools/vr_bench/test_vr_bench.py -v"""
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import trajectories  # noqa: E402
import vr_bench  # noqa: E402

BASE = dict(x=0.0, y=1.7, z=0.0, yaw=0.0, pitch=0.0, roll=0.0)


class Trajectories(unittest.TestCase):
    def test_every_trajectory_is_strictly_increasing_and_starts_at_base(self):
        for name in trajectories.TRAJECTORIES:
            keys = trajectories.keys(name, BASE)
            times = [k[0] for k in keys]
            self.assertTrue(all(b > a for a, b in zip(times, times[1:])), name)
            self.assertAlmostEqual(keys[0][1], BASE['x'], places=6)
            self.assertAlmostEqual(keys[0][4], BASE['yaw'], places=6, msg=name)

    def test_yaw_is_written_in_the_simulator_convention(self):
        # OpenXR yaw positive = left; the simulator's command yaw is the negated value (simxr.head).
        keys = trajectories.keys('snap90', BASE)
        held = [k for k in keys if 0.4 < k[0] < 0.9]
        self.assertTrue(held)
        for k in held:
            self.assertAlmostEqual(k[4], -math.pi / 2, places=4)

    def test_translation_follows_the_base_heading(self):
        base = dict(BASE, yaw=-math.pi / 2)            # turned 90 deg left (simulator yaw is negated)
        keys = trajectories.keys('translate_lr', base)
        peak = max(keys, key=lambda k: abs(k[1] - base['x']) + abs(k[3] - base['z']))
        # a sideways move after a left turn runs along world z, not x
        self.assertGreater(abs(peak[3] - base['z']), 0.2)
        self.assertLess(abs(peak[1] - base['x']), 1e-6)

    def test_text_header_and_line_count(self):
        text = trajectories.text('still', BASE, delay=0.3, ident=7)
        lines = text.strip().splitlines()
        self.assertEqual(lines[0], 'id 7')
        self.assertEqual(lines[1], 'delay 0.300')
        self.assertEqual(len(lines) - 2, int(8.0 * trajectories.RATE) + 1)
        self.assertEqual(len(lines[2].split()), 7)


class FrameStats(unittest.TestCase):
    def test_warm_up_needs_two_agreeing_windows(self):
        ramp = dict(frames=40, p50=30.0, clock=1300.0)
        hot = dict(frames=44, p50=22.5, clock=2040.0)
        self.assertFalse(vr_bench.steady(None, hot))
        self.assertFalse(vr_bench.steady(ramp, hot))                                   # the clock still ramping
        self.assertTrue(vr_bench.steady(hot, dict(frames=45, p50=22.3, clock=2032.0)))
        self.assertFalse(vr_bench.steady(hot, dict(frames=45, p50=22.3, clock=None)))   # no nvidia-smi rows
        self.assertFalse(vr_bench.steady(hot, dict(frames=3, p50=22.4, clock=2040.0)))  # too few frames

    def test_a_window_without_frames_is_a_stall(self):
        self.assertTrue(vr_bench.stalled(vr_bench.frame_stats([13.8], [13.8], 11.111), 7.0))
        self.assertTrue(vr_bench.stalled({}, 7.0))
        self.assertFalse(vr_bench.stalled(vr_bench.frame_stats([40.0] * 180, [40.0] * 180, 11.111), 7.0))
        self.assertFalse(vr_bench.stalled(dict(frames=12), 1.0))     # a short window needs at least 10 frames

    def test_percentiles_lows_and_period_misses(self):
        cpu = [11.0] * 980 + [30.0] * 20
        gpu = [9.0] * 1000
        s = vr_bench.frame_stats(cpu, gpu, 11.111)
        self.assertEqual(s['frames'], 1000)
        self.assertAlmostEqual(s['cpuP50'], 11.0)
        self.assertAlmostEqual(s['cpuP99'], 30.0)          # inside the 2% tail
        self.assertAlmostEqual(s['low1'], 1000 / 30.0)       # mean of the worst 1% = 30 ms
        self.assertEqual(s['overPeriod'], 20)
        self.assertEqual(s['overTwoPeriods'], 20)
        self.assertAlmostEqual(s['gpuP99'], 9.0)

    def test_empty(self):
        self.assertEqual(vr_bench.frame_stats([], [], 11.1), dict(frames=0))


class Profiles(unittest.TestCase):
    def test_eye_classification(self):
        right = {1536}
        self.assertEqual(vr_bench.eye_of(0x100000 | (2 << 16) | 1536, right), 'R')
        self.assertEqual(vr_bench.eye_of(0x100000 | (1 << 16) | 1536, right), 'L')
        self.assertEqual(vr_bench.eye_of(0x100000 | (1 << 16) | 3072, right), 'F')
        self.assertEqual(vr_bench.eye_of(0, right), 'C')
        self.assertEqual(vr_bench.eye_of(3, right), 'C')            # a cascade index, not a viewport
        self.assertEqual(vr_bench.eye_of(0x100000 | (3 << 16) | 64, right), 'O')

    def test_summary_is_per_frame_and_splits_eyes(self):
        g = lambda node, detail, ms: dict(node=node, side=0, pso=0, detail=detail, sum_ms=ms, ranges=2, commands=4,
                                          lists=1, max_ms=ms)
        left, rightd, whole = 0x100000 | (1 << 16) | 1536, 0x100000 | (2 << 16) | 1536, 0x100000 | (1 << 16) | 3072
        profiles = [dict(frames=4, groups=[g(0x10, left, 4.0), g(0x10, rightd, 4.4), g(0x20, whole, 2.0),
                                           g(0x30, 0, 1.2)])] * 2
        s = vr_bench.summarize_profiles(profiles, {0x10: 'GBuffer', 0x20: 'Post', 0x30: 'Cs'})
        self.assertEqual(s['frames'], 8)
        rows = {r['name']: r for r in s['nodes']}
        self.assertAlmostEqual(rows['GBuffer']['L'], 1.0)
        self.assertAlmostEqual(rows['GBuffer']['R'], 1.1)
        self.assertAlmostEqual(rows['Post']['F'], 0.5)
        self.assertAlmostEqual(rows['Cs']['C'], 0.3)
        self.assertAlmostEqual(s['totals']['all'], 2.9)


class Counters(unittest.TestCase):
    def test_array_counters_name_their_index(self):
        self.assertEqual(vr_bench.counter_symbol('CyberpunkVR_SinglePassCounts[3]'), ('CyberpunkVR_SinglePassCounts', 3))
        self.assertEqual(vr_bench.counter_symbol('CyberpunkVR_XrRepeatCycles'), ('CyberpunkVR_XrRepeatCycles', 0))


class Pairs(unittest.TestCase):
    def test_left_right_batches_are_paired_and_the_camera_difference_found(self):
        def draw(seq, vpx, camera, args=(36, 1, 0, 0, 0)):
            params, types = [0] * vr_bench.PAIR_PARAMS, [0] * vr_bench.PAIR_PARAMS
            params[1], params[3] = camera, 0x5000 + (vpx > 0) * 0x40
            types[1], types[3] = 2, 1
            return dict(list=7, pso=11, root=13, ib=17, vb=19, params=params, types=types, node=0x33DDA4,
                        serial=100, kind=2, vpx=vpx, vpw=1197, args=list(args), seq=seq)
        rows = [draw(0, 0, 0x1000), draw(1, 0, 0x1000, (48, 1, 0, 0, 0)),
                draw(2, 1197, 0x1400), draw(3, 1197, 0x1400, (48, 1, 0, 0, 0)),
                draw(4, 0, 0x1000), draw(5, 1197, 0x1400)]
        a = vr_bench.analyze_pairs(rows)
        self.assertEqual(a['pairs'], 2)
        self.assertEqual(a['pairsEqual'], 2)
        self.assertEqual(a['compared'], 3)
        self.assertEqual(a['fieldDiff'], {})                     # same pipeline, buffers, arguments
        self.assertEqual(a['paramDiff'], {'1:cbv': 3, '3:table': 3})   # the camera block and a per-eye table
        # Inside each eye's batch table 3 stays one handle: both eyes' tables are known when the right batch starts.
        self.assertEqual(a['withinBatch']['L3'], dict(batches=1, draws=2, changes=0))
        self.assertEqual(a['withinBatch']['R3'], dict(batches=1, draws=2, changes=0))
        self.assertEqual(a['cameraDelta'], {'1024': 3})
        # The right camera address is first bound in the first right batch; in the second pair it was known already.
        self.assertEqual(a['rightKnownChecked'], 3)
        self.assertEqual(a['rightKnownBeforeLeft'], 1)
        self.assertEqual(a['eyes'], {'L': 3, 'R': 3})


class Timeline(unittest.TestCase):
    def test_busy_is_the_union_of_queue_intervals_per_present_window(self):
        qpcf, freq = 10_000_000, 1_000_000             # QPC 10 MHz, GPU timestamps 1 MHz, both from zero
        calib = (freq, 0, 0, freq, 0, 0, qpcf, 0)
        present = lambda ms: (2, 0, int(ms * qpcf / 1000), 0, 0, 0)
        batch = lambda kind, a, b: (kind | (3 << 8), 0, 0, 0, int(a * freq / 1000), int(b * freq / 1000))
        records = [present(0), batch(0, 1, 5), batch(1, 4, 8), present(10), batch(0, 12, 15), present(20),
                   batch(0, 19, 23), present(30)]
        t = vr_bench.timeline_frames(records, calib)
        self.assertEqual(t['frames'], 3)
        self.assertAlmostEqual(t['busyMax'], 7.0, places=6)              # 1..8 ms, the overlap counted once
        self.assertAlmostEqual(t['windowP50'], 10.0, places=6)
        busy = sorted(w['busy'] for w in t['worst'])
        # [10,20): 12..15 and 19..20 = 4 ms; [20,30): 20..23 = 3 ms -- a batch is split at the Present it spans
        self.assertEqual(busy, [3.0, 4.0, 7.0])

    def test_idle_is_split_into_starved_and_held(self):
        qpcf, freq = 10_000_000, 1_000_000
        calib = [freq, 0, 0, freq, 0, 0, qpcf, 0]
        q = lambda ms: int(ms * qpcf / 1000)
        g = lambda ms: int(ms * freq / 1000)
        present = lambda ms: [2, 0, q(ms), q(ms), 0, 0]
        batch = lambda call, a, b: [0 | (1 << 8), 7, q(call), q(call) + 1, g(a), g(b)]
        # Frames [10,20) and [20,30) are whole (the analyser drops the first and last). In each: busy 11..15, then a
        # 2 ms gap whose next batch was called at 16 (after the GPU went idle at 15: starved), then 17..18, then a
        # 1 ms gap whose next batch was called at 12 (before: held), then 19..20.
        records = [present(0), batch(0, 0, 9)]
        for base in (10, 20, 30):
            records += [present(base), batch(base, base + 1, base + 5), batch(base + 6, base + 7, base + 8),
                        batch(base + 2, base + 9, base + 10)]
        records.append(present(40))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 't.json'
            path.write_text(json.dumps(dict(calib=calib, records=records)), encoding='utf-8')
            idle = vr_bench.timeline_idle(path)
        self.assertEqual(idle['frames'], 2)
        self.assertAlmostEqual(idle['busy'], 6.0, places=3)
        self.assertAlmostEqual(idle['starved'], 1.0 + 2.0, places=3)   # the 1 ms after the Present (called at it) too
        self.assertAlmostEqual(idle['held'], 1.0, places=3)


def _write_plane_capture(directory, w=192, h=160, ipd=0.064, distance=5.0, jitter=(0.0, 0.0)):
    """Two eyes looking down -z at a textured wall at z = -distance; reversed depth, asymmetric windows."""
    n, f = 0.1, 1000.0
    fov = [(-0.9, 0.7, 0.8, -0.85), (-0.7, 0.9, 0.8, -0.85)]   # left / right windows (radians), nose-side narrower
    eyes = []
    for e in range(2):
        px = (-0.5 if e == 0 else 0.5) * ipd
        l, r, u, d = [math.tan(a) for a in fov[e]]
        depth, color = bytearray(), bytearray()
        for y in range(h):
            ty = u - (u - d) * (y + 0.5) / h
            for x in range(w):
                tx = l + (r - l) * (x + 0.5) / w
                z = distance                                   # every ray hits the wall at view depth `distance`
                wx, wy = px + tx * z, 1.7 + ty * z
                depth += struct.pack('<f', n * (f - z) / ((f - n) * z))
                shade = int(128 + 90 * math.sin(wx * 2.5) * math.cos(wy * 1.7))
                color += bytes((shade, 255 - shade, 128, 255))
        (directory / f'f0_e{e}_depth.raw').write_bytes(bytes(depth))
        (directory / f'f0_e{e}_color.raw').write_bytes(bytes(color))
        (directory / f'f0_e{e}_motion.raw').write_bytes(bytes(w * h * 4))
        eyes.append(dict(color=[w, h, 28], depth=[w, h, 41], motion=[w, h, 34], jitter=list(jitter),
                         motionScale=[1, 1], position=[px, 1.7, 0.0], up=[0, 1, 0], right=[1, 0, 0],
                         forward=[0, 0, -1], planes=[n, f, 1.6, 1.0], inverted=1, infinite=0, jittered=0, pose=1,
                         qpc=1, orientation=[0, 0, 0, 1], fov=list(fov[e])))
    (directory / 'capture.json').write_text(json.dumps(dict(frames=[dict(frame=1, eyes=eyes)])), encoding='utf-8')


@unittest.skipUnless(vr_bench.STEREO_CHECK.exists(), 'stereo_check.exe not built')
class StereoCheck(unittest.TestCase):
    def run_check(self, **kw):
        with tempfile.TemporaryDirectory() as tmp:
            capture, out = Path(tmp) / 'cap', Path(tmp) / 'out'
            capture.mkdir()
            _write_plane_capture(capture, **kw)
            result = subprocess.run([str(vr_bench.STEREO_CHECK), str(capture), str(out), '--jitter', 'none'],
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            value = json.loads(result.stdout)
            for name in ('L.jpg', 'R.jpg', 'SBS.jpg', 'heat_L.png', 'heat_R.png', 'stereo.json'):
                self.assertTrue((out / name).exists(), name)
            return value

    def test_a_consistent_pair_agrees(self):
        v = self.run_check()
        self.assertAlmostEqual(v['separation'], 0.064, places=4)
        for way in ('leftToRight', 'rightToLeft'):
            s = v[way]
            self.assertGreater(s['geometricAgreement'], 0.99, way)
            self.assertLess(s['meanDiff'], 2.0, way)               # bilinear resampling only
            self.assertLess(s['depthErrorP95'], 1e-3, way)
            self.assertGreater(s['outside'], 0, way)                # the outer strip only one eye sees

    def test_a_wrong_separation_is_caught(self):
        # Colour drawn for a 64 mm pair but the right camera claiming to be 0.6 m off: the reprojection lands half a
        # texture period away.
        with tempfile.TemporaryDirectory() as tmp:
            capture, out = Path(tmp) / 'cap', Path(tmp) / 'out'
            capture.mkdir()
            _write_plane_capture(capture)
            cap = json.loads((capture / 'capture.json').read_text())
            cap['frames'][0]['eyes'][1]['position'][0] = 0.6
            (capture / 'capture.json').write_text(json.dumps(cap))
            result = subprocess.run([str(vr_bench.STEREO_CHECK), str(capture), str(out), '--jitter', 'none'],
                                    capture_output=True, text=True, timeout=60)
            v = json.loads(result.stdout)
            self.assertGreater(v['leftToRight']['meanDiff'], 15.0)

    def test_compare_finds_nothing_in_a_run_against_itself_and_a_changed_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            outs = []
            for name, distance in (('a', 5.0), ('b', 2.0)):
                capture, out = Path(tmp) / f'cap_{name}', Path(tmp) / f'out_{name}'
                capture.mkdir()
                _write_plane_capture(capture, distance=distance)
                subprocess.run([str(vr_bench.STEREO_CHECK), str(capture), str(out), '--jitter', 'none'],
                               capture_output=True, text=True, timeout=60, check=True)
                outs.append(out)

            def compare(x, y, where):
                result = subprocess.run([str(vr_bench.STEREO_CHECK), '--compare', str(x), str(y), str(Path(tmp) / where)],
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue((Path(tmp) / where / 'diff_L.png').exists())
                return json.loads(result.stdout)

            same = compare(outs[0], outs[0], 'same')
            for eye in ('L', 'R'):
                self.assertEqual(same[eye]['mean'], 0)
                self.assertEqual(same[eye]['over16'], 0)
                self.assertEqual(len(same[eye]['grid']), 16)
            changed = compare(outs[0], outs[1], 'changed')
            for eye in ('L', 'R'):
                self.assertGreater(changed[eye]['mean'], 10)
                self.assertGreater(changed[eye]['over16'], 0.2)


class PipelineStats(unittest.TestCase):
    def test_per_eye_sums_per_frame_and_the_clipped_share(self):
        left = 0x100000 | (1 << 16) | 1197
        right = 0x100000 | (2 << 16) | 1197
        ps = lambda c_inv, c_prim, vs: [0, c_inv, vs, 0, 0, c_inv, c_prim, 10, 0, 0, 0]
        groups = [dict(detail=left, ps=ps(1000, 600, 2000)), dict(detail=left, ps=ps(1000, 800, 2000)),
                  dict(detail=right, ps=ps(2000, 1000, 4000)), dict(detail=right)]   # a group without stats
        eyes = vr_bench.pipeline_stats_summary(groups, frames=2)
        self.assertEqual(sorted(eyes), ['L', 'R'])
        self.assertAlmostEqual(eyes['L']['cInvocations'], 1000)
        self.assertAlmostEqual(eyes['L']['clippedShare'], 0.3)
        self.assertAlmostEqual(eyes['R']['clippedShare'], 0.5)
        self.assertAlmostEqual(eyes['R']['vsInvocations'], 2000)


class DrawOcclusion(unittest.TestCase):
    def test_shares_per_eye(self):
        occ = {'1': [400, 100, 50, 150, 100], '2': [400, 140, 60, 120, 80], '3': [0, 0, 0, 0, 0]}
        eyes = vr_bench.draw_occlusion_summary(occ, frames=4)
        self.assertEqual(sorted(eyes), ['L', 'R'])
        self.assertAlmostEqual(eyes['L']['drawsPerFrame'], 100)
        self.assertAlmostEqual(eyes['L']['zeroShare'], 0.25)
        self.assertAlmostEqual(eyes['R']['zeroShare'], 0.35)
        self.assertAlmostEqual(eyes['R']['moreShare'], 0.2)


class EtwCpu(unittest.TestCase):
    def test_samples_are_counted_per_module_and_thread_inside_the_window(self):
        import etw_cpu

        def line(name, kind, clock, *data):   # Event Name, Type, 14 header columns, Clock-Time, Kernel, User, data
            return f'{name:>12}, {kind:>10}, ' + ', '.join(['0'] * 14) + f', {clock}, 0, 0, ' + ', '.join(data) + '\n'
        rows = ['Event Name, Type\n',
                line('Process', 'DCStart', 1, '0xFFFF', '0x1F4', '0x4', '1', '"\\\\PC\\u"', '"Cyberpunk2077.exe"', '""'),
                line('Image', 'DCStart', 1, '0x140000000', '0x1000000', '500', '0', '"\\Device\\X\\Cyberpunk2077.exe"'),
                line('Image', 'Load', 1, '0x7FF800000000', '0x100000', '500', '0', '"\\Device\\X\\cyberpunkvrport.dll"'),
                line('Image', 'DCStart', 1, '0xFFFFF80000000000', '0x1000000', '0', '0', '"\\SystemRoot\\ntoskrnl.exe"'),
                line('Thread', 'DCStart', 1, '0x1F4', '0x10', '0x0'),
                line('Thread', 'DCStart', 1, '0x1F4', '0x11', '0x0'),
                line('Thread', 'DCStart', 1, '0x99', '0x12', '0x0'),
                line('Thread', 'SetName', 1, '0x1F4', '0x10', '"GameThread"'),
                line('Thread', 'SetName', 1, '0x1F4', '0x11', '"redDispatcher7"')]
        samples = [(100, '0x140001000', 16)] * 3 + [(100, '0x7FF800000010', 17)] * 2 + \
                  [(100, '0xFFFFF80000000100', 17)] + [(100, '0x140001000', 18)] + [(500, '0x140001000', 16)] * 4
        rows += [line('PerfInfo', 'SampleProf', t, ip, str(tid), '1', '72') for t, ip, tid in samples]
        with tempfile.TemporaryDirectory() as tmp:
            csv = Path(tmp) / 't.csv'
            csv.write_text(''.join(rows), encoding='utf-8')
            functions = Path(tmp) / 'functions.tsv'
            functions.write_text('800\t900\tsub_140000800\n1000\t1080\tjobWait\n', encoding='utf-8')
            v = etw_cpu.count(csv, process='Cyberpunk2077.exe', window=(50, 200), exe_functions=functions)
        self.assertEqual(v['exeFunctions'], [('1000', 'jobWait', 3)])   # exe samples by IDA function (rva 0x1000)
        self.assertEqual(v['exeByThread'], [('GameThread', '1000', 'jobWait', 3)])
        self.assertEqual(v['pid'], 0x1F4)
        self.assertEqual(v['samples'], 6)                 # the other process's thread and the late samples left out
        self.assertEqual(dict(v['modules']), {'Cyberpunk2077.exe': 3, 'cyberpunkvrport.dll': 2, 'ntoskrnl.exe': 1})
        self.assertEqual(dict(v['threads']), {'GameThread': 3, 'redDispatcher#': 3})
        self.assertAlmostEqual(v['seconds'], 150 / 1e7)


class Settings(unittest.TestCase):
    def test_list_option_takes_value_and_index_and_is_restored(self):
        data = dict(version=1, data=[dict(group_name='/graphics/advanced', options=[
            dict(name='ScreenSpaceReflectionsQuality', type='string_list', value='Off', index=0,
                 values=['Off', 'Low', 'Medium', 'High', 'Ultra', 'Insane']),
            dict(name='ContactShadows', type='bool', value=True),
            dict(name='DLSS', type='string_list', is_dynamic=True, value='Balanced', index=3)])])
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'UserSettings.json'
            path.write_text(json.dumps(data), encoding='utf-8')
            saved = vr_bench.USER_SETTINGS
            vr_bench.USER_SETTINGS = path
            try:
                original = vr_bench.apply_settings(['ScreenSpaceReflectionsQuality=High', 'ContactShadows=0'])
                changed = json.loads(path.read_text(encoding='utf-8'))['data'][0]['options']
                self.assertEqual((changed[0]['value'], changed[0]['index']), ('High', 3))
                self.assertIs(changed[1]['value'], False)
                with self.assertRaises(SystemExit):
                    vr_bench.apply_settings(['ScreenSpaceReflectionsQuality=Extreme'])
                vr_bench.apply_settings(['DLSS=Ultra Performance#5'])
                dlss = json.loads(path.read_text(encoding='utf-8'))['data'][0]['options'][2]
                self.assertEqual((dlss['value'], dlss['index']), ('Ultra Performance', 5))
                with self.assertRaises(SystemExit):
                    vr_bench.apply_settings(['DLSS=Performance'])
                vr_bench.restore_settings(original)
                self.assertEqual(json.loads(path.read_text(encoding='utf-8')), data)
            finally:
                vr_bench.USER_SETTINGS = saved


class Telegram(unittest.TestCase):
    def test_no_config_is_a_quiet_no_op(self):
        import telegram
        saved = telegram.CONFIG
        try:
            telegram.CONFIG = Path(tempfile.gettempdir()) / 'does-not-exist' / 'telegram.json'
            self.assertFalse(telegram.send('x'))
        finally:
            telegram.CONFIG = saved


if __name__ == '__main__':
    unittest.main()
