//! The monowire's line (formerly include/Anim/CyberwareDynamics.hpp): a free-ended filament anchored at the emitter,
//! world-space inertia, gravity and inextensible segments, shared by the rendered rig and the combat geometry. The
//! owner steps it once per tracking/animation epoch, never per graph pass. Operation for operation as the C++.
use cvr_body::cmath::{clamp, exp, max, min, sqrt};
use core::ops::{Add, Mul, Sub};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
pub const fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}
impl Add for Vec3 {
    type Output = Vec3;
    #[inline(always)]
    fn add(self, b: Vec3) -> Vec3 {
        v3(self.x + b.x, self.y + b.y, self.z + b.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    #[inline(always)]
    fn sub(self, b: Vec3) -> Vec3 {
        v3(self.x - b.x, self.y - b.y, self.z - b.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    #[inline(always)]
    fn mul(self, s: f32) -> Vec3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
#[inline(always)]
pub fn length(v: Vec3) -> f32 {
    sqrt(v.x * v.x + v.y * v.y + v.z * v.z)
}
#[inline(always)]
pub fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}
#[inline(always)]
pub fn unit(v: Vec3) -> Vec3 {
    let n = length(v);
    if n > 1e-6 {
        v * (1.0 / n)
    } else {
        v3(0.0, 1.0, 0.0)
    }
}
#[inline(always)]
pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub position: Vec3,
    pub normal: Vec3,
    pub valid: bool,
}

pub const COUNT: usize = 46;
const _: () = assert!(core::mem::size_of::<Wire>() == 2440);
pub const REACH: f32 = 5.0;

/// The C++ struct's layout (cvr::cyberware::Wire), so the two can be stepped on the same state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Wire {
    pub point: [Vec3; COUNT],
    pub velocity: [Vec3; COUNT],
    pub planes: [Plane; COUNT],
    pub last_anchor: Vec3,
    pub last_direction: Vec3,
    pub motion: Vec3,
    pub deployed: f32,
    pub swing_tension: f32,
    pub valid: bool,
}
impl Default for Wire {
    fn default() -> Self {
        Self::ZERO
    }
}
impl Wire {
    pub const ZERO: Wire = Wire {
        point: [v3(0.0, 0.0, 0.0); COUNT],
        velocity: [v3(0.0, 0.0, 0.0); COUNT],
        planes: [Plane { position: v3(0.0, 0.0, 0.0), normal: v3(0.0, 0.0, 0.0), valid: false }; COUNT],
        last_anchor: v3(0.0, 0.0, 0.0),
        last_direction: v3(0.0, 0.0, 0.0),
        motion: v3(0.0, 0.0, 0.0),
        deployed: 0.0,
        swing_tension: 0.0,
        valid: false,
    };
    pub fn reset(&mut self) {
        *self = Self::ZERO;
    }

    /// carry: how far the body itself moved since the last step; the whole line moves with it first.
    pub fn step(&mut self, anchor: Vec3, direction: Vec3, grip: bool, dt: f32, length_in: f32, carry: Vec3) {
        if !finite(anchor) || !finite(direction) || !dt.is_finite() || !length_in.is_finite() {
            self.reset();
            return;
        }
        const N: i32 = COUNT as i32;
        let length = clamp(length_in, 0.2, 8.0);
        let direction = unit(direction);
        if self.valid && finite(carry) {
            for p in self.point.iter_mut() {
                *p = *p + carry;
            }
            self.last_anchor = self.last_anchor + carry;
        }
        if !self.valid || length_of(anchor - self.last_anchor) > 1.0 || dt > 0.15 {
            for i in 0..COUNT {
                self.point[i] = anchor + direction * (self.deployed * length * i as f32 / (N - 1) as f32);
                self.velocity[i] = Vec3::default();
            }
            self.last_anchor = anchor;
            self.last_direction = direction;
            self.motion = Vec3::default();
            self.swing_tension = 0.0;
            self.valid = true;
        }
        let anchor_before = if self.valid { self.last_anchor } else { anchor };
        let sample_dt = max(dt, 0.001);
        // Motion at the line's middle captures both arm sweeps and wrist flicks.
        let sample_motion = (anchor - self.last_anchor + (direction - self.last_direction) * (length * 0.5)) * (1.0 / sample_dt);
        let motion_blend = 1.0 - exp(-min(dt, 0.05) / 0.04);
        self.motion = self.motion + (sample_motion - self.motion) * motion_blend;
        let tension_target = clamp((length_of(self.motion) - 0.6) / 2.4, 0.0, 1.0);
        let tension_blend = 1.0 - exp(-min(dt, 0.05) / (if tension_target > self.swing_tension { 0.04 } else { 0.18 }));
        self.swing_tension += (tension_target - self.swing_tension) * tension_blend;
        let lateral = self.motion - direction * dot(self.motion, direction);
        let lateral_speed = length_of(lateral);
        let bow = if lateral_speed > 1e-5 {
            lateral * (-min(length * 0.12, lateral_speed * 0.10) * self.swing_tension / lateral_speed)
        } else {
            Vec3::default()
        };
        self.last_anchor = anchor;
        self.last_direction = direction;
        let dt = clamp(dt, 0.0, 0.05);
        let steps = 1.max((dt * 240.0).ceil() as i32);
        let h = dt / steps as f32;
        if h < 1e-6 {
            return;
        }
        for step in 0..steps {
            let pin = anchor_before + (anchor - anchor_before) * ((step + 1) as f32 / steps as f32);
            let previous_deployed = self.deployed;
            self.deployed = clamp(self.deployed + (if grip { 5.0 } else { -8.0 }) * h, 0.0, 1.0);
            if self.deployed <= 0.0001 {
                self.point = [anchor; COUNT];
                self.velocity = [Vec3::default(); COUNT];
                continue;
            }
            if previous_deployed <= 0.0001 {
                for i in 0..COUNT {
                    self.point[i] = anchor + direction * (length * self.deployed * i as f32 / (N - 1) as f32);
                }
                self.velocity = [Vec3::default(); COUNT];
            } else if self.deployed != previous_deployed {
                let ratio = self.deployed / previous_deployed;
                for i in 1..COUNT {
                    self.point[i] = anchor + (self.point[i] - anchor) * ratio;
                }
            }
            let segment = length * self.deployed / (N - 1) as f32;
            let before = self.point;
            self.point[0] = pin;
            for i in 1..COUNT {
                // A light filament must retain a flick's momentum.
                self.velocity[i] = self.velocity[i] * exp(-0.35 * h);
                if grip && self.deployed > previous_deployed {
                    self.velocity[i] = self.velocity[i] + direction * (12.0 * h * i as f32 / (N - 1) as f32);
                }
                // A supported filament: shape memory keeps the proximal section ahead of the wrist.
                let along = i as f32 / (N - 1) as f32;
                let support = clamp((0.85 - along) / 0.20, 0.0, 1.0);
                let stiffness = 300.0 * (support * support + (1.0 - support * support) * self.swing_tension);
                let curve = 16.0 * along * along * (1.0 - along) * (1.0 - along);
                let rest = pin + direction * (segment * i as f32) + bow * (curve * self.deployed);
                self.velocity[i] = self.velocity[i] + (rest - self.point[i]) * (stiffness * h);
                self.velocity[i] = self.velocity[i] * exp(-2.0 * sqrt(stiffness) * h);
                self.velocity[i].z -= 9.81 * h;
                self.point[i] = self.point[i] + self.velocity[i] * h;
            }
            // Symmetric passes avoid a permanent stretch gradient at the hand.
            for iteration in 0..24 {
                for j in 1..COUNT {
                    let i = if iteration & 1 != 0 { COUNT - j } else { j };
                    let mut delta = self.point[i] - self.point[i - 1];
                    let n = length_of(delta);
                    delta = if n > 1e-6 { delta * (1.0 / n) } else { direction };
                    let correction = delta * (n - segment);
                    if i == 1 {
                        self.point[i] = self.point[i] - correction;
                    } else {
                        self.point[i - 1] = self.point[i - 1] + correction * 0.5;
                        self.point[i] = self.point[i] - correction * 0.5;
                    }
                }
                // Long-range, tension-only constraints carry a hand impulse through the chain.
                for i in 2..COUNT {
                    let delta = self.point[i] - pin;
                    let n = length_of(delta);
                    let limit = segment * i as f32;
                    if n > limit {
                        self.point[i] = pin + delta * (limit / n);
                    }
                }
                self.point[0] = pin;
                // The first short segment exits along the wrist.
                self.point[1] = pin + direction * segment;
                for i in 1..COUNT {
                    if self.planes[i].valid {
                        let plane = self.planes[i];
                        let delta = self.point[i] - plane.position;
                        let distance = dot(delta, plane.normal);
                        let tangent = delta - plane.normal * distance;
                        if distance < 0.006 && length_of(tangent) < 0.65 {
                            self.point[i] = self.point[i] + plane.normal * (0.006 - distance);
                        }
                    }
                }
            }
            for i in 1..COUNT {
                self.velocity[i] = (self.point[i] - before[i]) * (1.0 / h);
                let speed = length_of(self.velocity[i]);
                if speed > 35.0 {
                    self.velocity[i] = self.velocity[i] * (35.0 / speed);
                }
                let plane = self.planes[i];
                if plane.valid && dot(self.point[i] - plane.position, plane.normal) < 0.012 {
                    let into = dot(self.velocity[i], plane.normal);
                    if into < 0.0 {
                        self.velocity[i] = self.velocity[i] - plane.normal * into;
                    }
                    let tangent = self.velocity[i] - plane.normal * dot(self.velocity[i], plane.normal);
                    let tangent_speed = length_of(tangent);
                    if tangent_speed > 1e-6 {
                        self.velocity[i] = self.velocity[i] - tangent * (min(tangent_speed, 0.04 * 9.81 * h) / tangent_speed);
                    }
                }
            }
        }
    }
}

#[inline(always)]
fn length_of(v: Vec3) -> f32 {
    length(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pays_out_and_retracts() {
        let mut w = Wire::default();
        let anchor = v3(0.0, 0.0, 1.0);
        for _ in 0..30 {
            w.step(anchor, v3(0.0, 1.0, 0.0), true, 0.016, 3.0, Vec3::default());
        }
        assert!(w.valid && w.deployed == 1.0);
        let tip = length(w.point[COUNT - 1] - anchor);
        assert!(tip > 2.5 && tip <= 3.0001, "{tip}");
        for _ in 0..30 {
            w.step(anchor, v3(0.0, 1.0, 0.0), false, 0.016, 3.0, Vec3::default());
        }
        assert_eq!(w.deployed, 0.0);
    }
}
