"""Offline tests for the same-session hand physics benchmark."""
from contextlib import ExitStack
import json
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import hand_physics as hp


class BenchmarkTests(unittest.TestCase):
    def test_same_trace_per_window(self):
        base = dict(x=1, y=1.7, z=-2, yaw=.2, pitch=-.12, roll=.1)
        frames = hp.trace(base, 2)
        self.assertEqual(len(frames), 80)
        self.assertEqual(frames, hp.trace(base, 2))
        self.assertEqual([f['t'] for f in frames], [i / hp.RATE for i in range(80)])
        for i, frame in enumerate(frames[:40]):
            for key in ('head', 'left', 'right'):
                self.assertEqual(frame[key], frames[40 + i][key])
        for key in ('head', 'left', 'right'):
            self.assertNotEqual(frames[0][key], frames[10][key])
        self.assertEqual(frames[0]['head'], base)
        self.assertEqual(frames[-1]['head'], base)

    def test_ring_reset_overwrite_and_wrap(self):
        game = Mock()
        game.ring.side_effect = lambda name, a, b: [10.] * (b - a)
        game.ring_counts.return_value = (8195, 8193)
        self.assertEqual([len(v) for v in hp.read_window(game, (8190, 8189), (8195, 8193))], [5, 4])
        for start, end in [((0, 0), (0, 1)), ((50, 20), (4, 19)), ((0, 0), (8193, 100))]:
            with self.assertRaises(RuntimeError):
                hp.read_window(game, start, end)
        game.ring_counts.return_value = (16384, 8193)
        with self.assertRaisesRegex(RuntimeError, 'overwritten'):
            hp.read_window(game, (8190, 8189), (8195, 8193))

    def test_resettable_physics_counters(self):
        samples = [v + [0] * (len(hp.COUNTERS) - len(v)) for v in
                   [[0]*6, [4, 100, 40, 60, 2, 7], [1, 3, 3, 3, 1, 1], [3, 23, 13, 23, 2, 4]]]
        cost = hp.physics_cost(samples)
        self.assertEqual(cost['resetIntervals'], 1)
        self.assertEqual(cost['beforeUsPerStep'], 20)
        self.assertAlmostEqual(cost['fingerUsPerSolve'], 80/3)
        self.assertAlmostEqual(cost['fingerCallsPerSolve'], 10/3)

    def test_only_complete_matched_blocks_contribute(self):
        def row(v):
            stats = hp.bench.frame_stats([20. if v == 'A' else 25.] * 100, [19.] * 100, 11.1)
            return dict(variant=v, stats=stats, issues=[], deliverySeconds=[[i/10, i/10+.02] for i in range(8)])
        rows = [row(v) for v in 'ABBAABBAABBAAB']
        rows[5]['issues'].append('actor inactive')
        for r in rows[9:11]:
            r['deliverySeconds'] = [[s, c+.04] for s, c in r['deliverySeconds']]
        result = hp.paired_results(rows)
        self.assertEqual([b['block'] for b in result['onMinusOff']], [1])
        self.assertEqual([b['block'] for b in result['excluded']], [2, 3, 4])
        self.assertEqual(result['medianDelta']['fps'], -10.)
        self.assertEqual(result['medianDelta']['cpuMean'], 5.)

    def test_session_restored_after_interrupt(self):
        with tempfile.TemporaryDirectory() as tmp, ExitStack() as mocks:
            root = Path(tmp)
            game = Mock()
            game.i32.return_value = 1
            game.addr.return_value = 128
            game.symbols = {}
            game.read.side_effect = lambda a, n: bytes(n)
            game.ring_counts.side_effect = [(0, 0), (45, 45), (46, 46)]
            game.ring.return_value = [22.] * 45
            sim = Mock(version=3)
            sim.dll = root / 'runtime.dll'
            sim.h = 27
            head = dict(position=dict(x=0., y=1.7, z=0.), yaw=.12, pitch=.1, roll=0.)
            saved_head = {**head['position'], **{k: head[k] for k in ('yaw','pitch','roll')}}
            layout = dict(sha256='test', LiveControls=dict(fields={k:dict(offset=i*4) for i,k in
                                                                  enumerate(('xrHandPhysics','xrHandCollision'))}))
            for name, value in dict(loaded_module=Mock(return_value=root/'test.dll'), describe=Mock(return_value=layout),
                                    read_status=Mock(return_value=dict(head_tracking=head, target_fps=90)),
                                    Sim=Mock(return_value=sim), on_foot=Mock(return_value=True),
                                    position=Mock(return_value=(0., 0., 0.)),
                                    window=Mock(side_effect=KeyboardInterrupt())).items():
                mocks.enter_context(patch.object(hp, name, value))
            mocks.enter_context(patch.object(hp.bench, 'Game', return_value=game))
            mocks.enter_context(patch.object(hp.bench, 'GpuSampler'))
            mocks.enter_context(patch.object(hp.time, 'sleep'))
            head_restore = mocks.enter_context(patch.object(hp, 'send'))
            mocks.enter_context(patch.object(hp, 'await_pose'))
            out = root/'results'
            with self.assertRaises(KeyboardInterrupt):
                hp.run(SimpleNamespace(pid=123, case='reach', save='test', blocks=1, cycles=1, out=out))
            head_restore.assert_called_once_with(hp.bench.SIM, saved_head)
            game.set32.assert_called_with('physicsMaster', 1)
            sim.restore.assert_called_once_with()
            game.close.assert_called_once_with()
            self.assertIn('KeyboardInterrupt', json.loads((out/'hand-physics.json').read_bytes())['error'])

    def test_post_block_requires_matched_starts_and_paths(self):
        def row(variant, start, middle):
            body=dict(foot=(middle,0.,0.),quat=(0.,0.,0.,1.))
            return dict(variant=variant, issues=[], deliverySeconds=[[0.,.02]],
                        postStart=dict(foot=(start,0.,0.)), postSamples=[dict(body=body)],
                        stats=hp.bench.frame_stats([20.]*100,[19.]*100,11.1))
        rows=[row(v,0.,0.) for v in 'ABBA']
        self.assertEqual(len(hp.paired_results(rows)['onMinusOff']),1)
        rows[1]=row('B',.021,.01)
        self.assertIn('paired starting feet differ by over 2 cm',hp.paired_results(rows)['excluded'][0]['reasons'])
        rows[1]=row('B',.01,.031)
        self.assertIn('paired foot paths differ by over 3 cm',hp.paired_results(rows)['excluded'][0]['reasons'])


if __name__ == '__main__':
    unittest.main()
