"""Offline geometry and load-acceptance controls for the storefront benchmark."""
from copy import deepcopy
import math
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import storefront_post as post

GENERATOR = Path(__file__).resolve().parents[2]/'build/vr-bench-post/Release/post_trace.exe'


class PostTests(unittest.TestCase):
    @unittest.skipUnless(GENERATOR.is_file(), 'build post_trace to check the native trajectory')
    def test_native_calibration_targets_post_on_rotated_axes(self):
        # XR +x points to game +y, XR +y points up, and XR +z points to game +x.
        origin = (-1436.91, 1259.60, 24.48)
        probes = [origin, (origin[0],origin[1]+.1,origin[2]), (origin[0],origin[1],origin[2]+.1),
                  (origin[0]+.1,origin[1],origin[2])]
        head = dict(x=0,y=1.7,z=0,yaw=.2,pitch=-.1,roll=0)
        frames = post.generate(GENERATOR,head,2,post.FOOT,probes)
        self.assertEqual([f['t'] for f in frames], [i/10 for i in range(80)])
        self.assertEqual(frames[0]['right'], list(post.RIGHT))
        for i in range(40):
            for k in ('left','right','head','phase'):
                self.assertEqual(frames[i][k],frames[40+i][k])
        for f in frames:
            if f['phase'] != 'press':
                continue
            x,y,z = [v-base for v,base in zip(f['right'],post.RIGHT)]
            world = (origin[0]+z,origin[1]+x,origin[2]+y)
            self.assertAlmostEqual(math.dist(world[:2],post.CONTACT_PATCH[:2]),.055,places=6)
            self.assertAlmostEqual(world[2],post.CONTACT_PATCH[2],places=6)
            self.assertEqual(f['head'],head)
        bound = (-1437.12,1259.34,24.65)
        target = (-1437.13,1259.32,24.65)
        tilted=post.generate(GENERATOR,head,1,post.FOOT,probes,bound,target,(.06,.035,-.03,35.))
        plateau=[f for f in tilted if f['phase']=='press']
        self.assertEqual(len({tuple(f['right']) for f in plateau}),1)
        self.assertEqual(len({tuple(f['rightQuat']) for f in plateau}),1)
        self.assertGreater(abs(plateau[0]['rightQuat'][0]),.25)
        with self.assertRaisesRegex(RuntimeError,'calibration'):
            post.generate(GENERATOR,head,1,post.FOOT,[origin]*4)

    def samples(self, cycles=2):
        frames, data = [], []
        for i in range(40*cycles):
            pressed = 12 <= i%40 < 24
            frames.append(dict(phase='press' if pressed else 'clear' if i%40>=32 else 'travel'))
            step = dict(serial=i+1,onWorld=pressed,kind=1,depthM=.002,inwardRefusedM=.01,wrist=post.POST,
                        tracked=post.POST,target=post.POST)
            data.append(dict(step=step,problem=None,near=dict(postBoundsGapM=.03)))
        return frames, data

    def test_each_cycle_needs_static_load_and_withdrawal(self):
        frames, samples = self.samples()
        def issues(s):
            return post.assess(frames,s,True,dict(worldStepsRight=100))[1]
        self.assertEqual(issues(samples),[])
        mismatched=deepcopy(samples)
        for s in mismatched[12:24]:
            s['near']['sameColliderBounds']=False
        self.assertTrue(issues(mismatched))
        transient=deepcopy(samples)
        for s in transient[16:24]:
            s['step']['inwardRefusedM']=.001
        self.assertTrue(issues(transient))
        for field,value in [('kind',2),('kind',3),('onWorld',False),('depthM',0.),
                            ('inwardRefusedM',.001),('wrist',post.FOOT)]:
            case=deepcopy(samples)
            for s in case[52:64]:
                s['step'][field]=value
            self.assertTrue(issues(case),field)
        case=deepcopy(samples)
        for s in case[12:24]:
            s['near']['postBoundsGapM']=1.
        self.assertTrue(issues(case))
        for s in samples[72:]:
            s['step']['onWorld']=True
        self.assertTrue(issues(samples))

    def test_world_age_identity_and_counter_controls(self):
        frames,samples=self.samples(1)
        samples[3]['problem']='local list stale'
        samples[5]['step']['serial']=samples[4]['step']['serial']
        _,issues=post.assess(frames,samples,True,dict(worldStepsRight=0))
        self.assertIn('local list stale',issues)
        self.assertTrue(any('serial' in v for v in issues))
        self.assertTrue(any('world-contact steps' in v for v in issues))
        _,issues=post.assess(frames,samples,False,dict(worldStepsRight=4))
        self.assertTrue(any('physics off' in v for v in issues))

    def test_station_facing(self):
        def quat(deg):
            return (0,0,math.sin(math.radians(deg)/2),math.cos(math.radians(deg)/2))
        self.assertLess(post.heading_error(quat(146),post.FORWARD),1)
        self.assertGreater(post.heading_error(quat(-34),post.FORWARD),170)

    def test_staging_vs_probe_drift(self):
        fixture=object.__new__(post.Post)
        fixture.game=Mock()
        fixture.scene=fixture.game.u64.return_value=100
        fixture.game.read.return_value=b'\0'
        fixture.owner=fixture.probe_foot=fixture.yaw=None
        # post-ab03 started 38 mm away and moved another 48 mm under pressure.
        initial=(-1436.7328,1259.7760,23.09)
        body=dict(foot=(-1436.6982,1259.8088,23.09),owner=18,ageMs=12,
                  quat=(0,0,math.sin(math.radians(73)),math.cos(math.radians(73))))
        self.assertTrue(fixture.placement(body))
        fixture.owner=18
        fixture.probe_foot=initial
        self.assertIsNone(fixture.placement(body))
        body['owner']=19
        self.assertTrue(fixture.placement(body))

    @unittest.skipUnless(GENERATOR.is_file(), 'build post_trace for the bind test')
    def test_calibration_requires_two_settled_successes(self):
        fixture=object.__new__(post.Post)
        sim=Mock()
        origin=(-1437.05,1259.54,24.55)
        body=dict(foot=post.FOOT,owner=12,quat=(0.,0.,.95,.31))
        sample=dict(body=body,step=dict(tracked=origin))
        fixture.retract=Mock(return_value=sample)
        def ready(enabled):
            offset=sim.hand.call_args.kwargs['pos']
            return dict(body=body,step=dict(tracked=[origin[k]+offset[k]-post.RIGHT[k] for k in range(3)]))
        fixture.ready=ready
        _,cycle=self.samples(1)
        for s in cycle:
            s['step']['wrist']=post.CONTACT_PATCH
            s['step']['target']=post.CONTACT_PATCH
            s['near']['bounds']=[post.POST,post.POST]
        attempts=[]
        for index in range(4):  # locate, fail after approach, pass, confirm
            rows=deepcopy(cycle)
            if index==1:
                for s in rows[16:24]:
                    s['step']['inwardRefusedM']=.001
            attempts.extend(rows)
        fixture.sample=Mock(side_effect=attempts)
        head=dict(x=0,y=1.7,z=0,yaw=0,pitch=0,roll=0)
        native_counts=iter(range(200))
        with patch.object(post.time,'sleep'):
            frames, info=fixture.calibrate(sim,head,2,GENERATOR,Mock(),lambda:next(native_counts),Mock(),2)
        self.assertTrue(info['qualified'])
        self.assertEqual([a['cycles'][0]['steadyRun'] for a in info['attempts']],[8,0,8,8])
        self.assertEqual(info['attempts'][-2]['frames'],info['attempts'][-1]['frames'])
        self.assertEqual(frames[:40],info['attempts'][-1]['frames'])


if __name__ == '__main__':
    unittest.main()
