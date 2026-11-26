import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
import json
import hashlib

import numpy as np

from rifle_elbow_live import START, TARGET, matched_frame, measure_sample, place_wrist
from rifle_elbow_review import depth_patch, reconstruct, review_marks


class ElbowEvidence(unittest.TestCase):
    def test_depth_patch(self):
        with TemporaryDirectory() as t:
            raw=Path(t)/'depth.raw'
            grid=np.zeros((10,12),dtype='<f4')
            grid[3:6,5:8]=.04
            grid[3,5]=.2
            grid.tofile(raw)
            eye=dict(color=[24,20,28],depth=[12,10,41],jitter=[.25,-.25],
                     planes=[.02,16000],inverted=True,infinite=True)
            z=depth_patch(raw,eye,[12,8.5])
            self.assertEqual(z['texel'],[6,4])
            self.assertAlmostEqual(z['medianM'],.5)
            self.assertAlmostEqual(z['rangeM'][0],.1)
            eye['infinite']=False
            self.assertAlmostEqual(depth_patch(raw,eye,[12,8.5])['medianM'],.499985,places=5)

    def test_occlusion_preserves_missing_angles_and_binds_sources(self):
        with TemporaryDirectory() as t:
            out=Path(t)
            ph={}
            marks={'depthSightlines':{}}
            for i,label in enumerate(('before','after')):
                (out/label).mkdir()
                for side in ('L','R'): (out/label/(side+'.jpg')).write_bytes(b'fixture')
                ph[label]={'frame':{'eyes':[{},{}],'frame':i}}
                marks[label]=dict(joints=dict(shoulder=None,elbow=None,wrist=None),notes='occluded',
                                  surface={},frame=i,sha256=[hashlib.sha256(b'fixture').hexdigest()]*2)
            (out/'capture.json').write_text(json.dumps(dict(complete=True,phases=ph)))
            coords=out/'marks.json'
            coords.write_text(json.dumps(marks))
            rep=review_marks(out,coords)
            self.assertTrue(rep['reviewed'])
            self.assertFalse(rep['complete'])
            self.assertIsNone(rep['phases']['after']['visibleAngles'])
            (out/'after/R.jpg').write_bytes(b'other frame')
            with self.assertRaisesRegex(ValueError,'another eye image'):
                review_marks(out,coords)

    def test_observer_bias_and_coupled_controller_axes(self):
        # The observer moves back by almost a metre. Roll, yaw and arm scale mix all three columns.
        matrix=np.array([[.71,.08,.54],[-.62,.03,.79],[.06,.93,-.02]])
        offset=np.array([.42,-.95,-.18])
        def observe(pos): return TARGET+offset+matrix@(pos-START)
        journal=dict(placements=[],jacobians=[])
        pos=place_wrist(observe,journal,lambda:None)
        np.testing.assert_allclose(observe(pos),TARGET,atol=.010)
        np.testing.assert_allclose(journal['jacobians'][0]['rowsRFU_columnsXYZ'],matrix,atol=1e-10)
        self.assertLessEqual(len(journal['placements']),15)

    def test_frozen_forward_guide(self):
        journal=dict(placements=[],jacobians=[])
        def blocked(pos): return np.array([pos[0],-.250619,pos[1]])
        with self.assertRaisesRegex(RuntimeError,'constrained or unresponsive'):
            place_wrist(blocked,journal,lambda:None)
        self.assertEqual(len(journal['placements']),8)
        self.assertEqual(len(journal['jacobians']),1)

    def test_matched_pose_and_pole(self):
        # A rigid triangle can flip over its chord without changing the elbow opening.
        s, w = np.array([0.,0.,0.]), np.array([0.,.18,0.])
        up, down = np.array([.10,.09,.20]), np.array([.10,.09,-.20])
        def frame(e):
            eye=dict(pose=123,elbowMode=2,rightArmModel=np.concatenate([s,e,w]).tolist())
            return dict(eyes=[eye.copy(),eye.copy()])
        frames=[frame(down) for _ in range(3)]
        frames[1]['eyes'][0]['pose']=122
        index,_,angles=matched_frame(frames,2)
        self.assertEqual(index,0)
        self.assertAlmostEqual(angles['openingDeg'],measure_sample(s,w,up)['openingDeg'])
        self.assertLess(angles['poleElevationDeg'],-60.)
        for f in frames: f['eyes'][1]['elbowMode']=1
        with self.assertRaises(RuntimeError): matched_frame(frames,2)

    def test_stereo_points(self):
        eyes=[dict(position=[x,0,0],forward=[0,1,0],right=[1,0,0],up=[0,0,1],
                   fov=np.arctan(f).tolist(),color=[800,600,28]) for x,f in
              ((-.032,[-1.1,.9,.8,-.7]),(.032,[-.9,1.1,.8,-.7]))]
        p=np.array([.08,.8,-.05])
        pixels=[]
        for eye in eyes:
            x,z,y=p-np.array(eye['position'])
            l,r,u,d=np.tan(eye['fov'])
            pixels.append(np.array([(x/z-l)/(r-l)*800-.5,(u-y/z)/(u-d)*600-.5]))
        np.testing.assert_allclose(reconstruct(eyes,pixels),p,atol=1e-9)
        pixels[1][1]+=30
        with self.assertRaises(ValueError): reconstruct(eyes,pixels)


if __name__=='__main__':
    unittest.main()
