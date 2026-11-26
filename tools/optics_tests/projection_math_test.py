"""Independent projective-geometry checks for the per-eye glass sampling centre."""
import unittest
import math
import re
from pathlib import Path
import numpy as np

class LensProjection(unittest.TestCase):
    def test_short_eye_ray_is_interpolated_before_world_origin(self):
        # The optics sits centimetres from the eye, hundreds of metres from
        # world zero. Preserve ray precision across subpixel barycentric shifts.
        eye=np.array([-666.247,845.694,65.82],dtype=np.float32)
        origin=np.array([-704,832,64],dtype=np.float32)
        rays=np.array([[-.03,.05,-.015],[.03,.05,-.015],[0,.05,.015]],dtype=np.float32)
        ip=(eye-origin)+rays
        world=origin+ip
        local=ip+(origin-eye)
        old=[];new=[]
        for b in np.linspace(.02,.98,1001,dtype=np.float32):
            weights=np.array([(1-b)*.5,b,(1-b)*.5],dtype=np.float32)
            expected=weights.astype(float)@local.astype(float)
            old.append(np.max(np.abs(weights@world-eye-expected)))
            new.append(np.max(np.abs(weights@local-expected)))
        self.assertLess(max(new),max(old)/1000)

    def test_camera_image_and_reticle_field_at_five_centimetres(self):
        root=Path(__file__).resolve().parents[2]
        policy=(root/'mods/cet/CyberpunkVRPort_Optics/optics/policy.lua').read_text()
        shader=(root/'src/Shaders/sight_reflex_ps.hlsl').read_text()
        coverage=float(re.search(r'imageCoverageDegrees\s*=\s*([\d.]+)',policy)[1])
        half_slope=float(re.search(r'imageHalfSlope\s*=\s*([\d.]+)f',shader)[1])
        self.assertAlmostEqual(math.tan(math.radians(coverage/2)),half_slope,places=8)
        # Grad's native aperture is 67.83 mm wide. The sensor must cover it
        # at the requested 50 mm eye relief without a rectangular black border.
        self.assertLess(.06783/2/.05,half_slope)
        for magnification in (3.15,3.9,4.0,4.4,8.0):
            sensor_half_slope=math.tan(math.atan(half_slope/magnification))
            for angle in (-.1,0,.1):
                uv_offset=math.tan(angle)/(2*sensor_half_slope)
                apparent_slope=2*half_slope*uv_offset
                self.assertAlmostEqual(apparent_slope,magnification*math.tan(angle),places=8)

    def test_perspective_lens_center_and_zoom(self):
        for eye_shift in (-160,0,160):
            for tilt in (-.6,0,.6):
                # Full homography, including perspective and asymmetric eye offset.
                h=np.array([[210,17,930+eye_shift],[-11,140,650],[tilt,.2,1.4]])
                inverse=np.linalg.inv(h)
                projected=h@np.array([.5,.5,1]);expected=projected[:2]/projected[2]
                jacobian=np.stack([inverse[0,:2]-.5*inverse[2,:2],inverse[1,:2]-.5*inverse[2,:2]])
                for u,v in ((.1,.2),(.8,.1),(.2,.9),(.8,.8)):
                    clip=h@np.array([u,v,1]);pixel=clip[:2]/clip[2]
                    q=(np.array([u,v])-.5)/clip[2]
                    center=pixel-np.linalg.solve(jacobian,q)
                    np.testing.assert_allclose(center,expected,rtol=1e-12,atol=1e-10)
                    for magnification in (1,1.75,3.99):
                        sample=pixel-(pixel-center)*(1-1/magnification)
                        # A source feature at sample must appear M times farther
                        # from the projected lens centre, in either eye.
                        np.testing.assert_allclose((sample-center)*magnification,pixel-center,atol=1e-10)
                        if magnification==1:np.testing.assert_array_equal(sample,pixel)

if __name__=='__main__':unittest.main()
