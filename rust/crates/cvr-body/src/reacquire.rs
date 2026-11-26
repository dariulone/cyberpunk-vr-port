//! A HAND COMING BACK INTO TRACKING GLIDES TO WHERE IT IS (formerly include/Runtimes/TrackingReacquire.hpp). On the
//! first pose after a gap, the difference between the pose last published and the new one is kept as an offset,
//! published scaled by a weight brought from 1 to 0 by a critically damped spring (BONELAB's OpenControllerRig blend,
//! Unity's SmoothDamp): the hand follows the controller from the first frame, the jump spread over a few tenths.
use crate::cmath::{acos, sqrt};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPose {
    pub p: [f32; 3],
    /// x, y, z, w
    pub q: [f32; 4],
}

impl Default for HandPose {
    fn default() -> Self {
        HandPose { p: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0] }
    }
}

/// Unity's Mathf.SmoothDamp (Game Programming Gems 4, 1.10).
pub fn smooth_damp(current: f32, target: f32, velocity: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let smooth_time = if smooth_time > 1e-4 { smooth_time } else { 1e-4 };
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * decay;
    let mut out = target + (change + temp) * decay;
    if (target - current > 0.0) == (out > target) {
        out = target;
        *velocity = if dt > 0.0 { (out - target) / dt } else { 0.0 };
    }
    out
}

fn multiply(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn conjugate(q: &[f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// From the identity towards `q` by `t`, normalised.
fn nlerp(q: &[f32; 4], t: f32) -> [f32; 4] {
    let r = [q[0] * t, q[1] * t, q[2] * t, 1.0 + (q[3] - 1.0) * t];
    let n = sqrt(r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]);
    core::array::from_fn(|i| if n > 1e-6 { r[i] / n } else if i == 3 { 1.0 } else { 0.0 })
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reacquire {
    last: HandPose,
    offset: HandPose,
    have: bool,
    lost: bool,
    blend: f32,
    velocity: f32,
    gap: f32,
}

impl Reacquire {
    pub const SMOOTH_TIME: f32 = 0.12;
    pub const NOTICEABLE: f32 = 0.002;
    pub const NOTICEABLE_ANGLE: f32 = 0.01;
    pub const MIN_GAP: f32 = 0.1;

    pub fn reset(&mut self) {
        *self = Reacquire::default();
    }

    pub fn step(&mut self, tracked: &HandPose, valid: bool, dt: f32) -> HandPose {
        if !valid {
            if self.have && !self.lost {
                self.gap = 0.0;
            }
            self.lost = self.have;
            self.gap += dt;
            return self.last;
        }
        if self.lost && self.gap < Self::MIN_GAP {
            self.lost = false;
        }
        if self.lost {
            self.lost = false;
            for i in 0..3 {
                self.offset.p[i] = self.last.p[i] - tracked.p[i];
            }
            self.offset.q = multiply(&self.last.q, &conjugate(&tracked.q));
            if self.offset.q[3] < 0.0 {
                for c in &mut self.offset.q {
                    *c = -*c;
                }
            }
            let o = &self.offset.p;
            let distance = sqrt(o[0] * o[0] + o[1] * o[1] + o[2] * o[2]);
            let angle = 2.0 * acos(1.0f32.min(self.offset.q[3])); // std::fmin
            self.blend =
                if distance > Self::NOTICEABLE || angle > Self::NOTICEABLE_ANGLE { 1.0 } else { 0.0 };
            self.velocity = 0.0;
        }
        let mut out = *tracked;
        if self.blend > 0.0 {
            self.blend = if dt > 0.0 {
                smooth_damp(self.blend, 0.0, &mut self.velocity, Self::SMOOTH_TIME, dt)
            } else {
                self.blend
            };
            if self.blend <= 0.01 {
                self.blend = 0.0;
            }
            for i in 0..3 {
                out.p[i] = tracked.p[i] + self.offset.p[i] * self.blend;
            }
            let part = nlerp(&self.offset.q, self.blend);
            out.q = multiply(&part, &tracked.q);
        }
        self.last = out;
        self.have = true;
        out
    }

    pub fn blend(&self) -> f32 {
        self.blend
    }
}
