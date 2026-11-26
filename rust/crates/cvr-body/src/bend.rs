//! THE ONE NECK and the bend tracker (formerly include/Anim/BodyBend.hpp). The eyes sit EyeAboveNeck over and
//! EyeAheadOfNeck ahead of the point the head turns about; roomscale walks that neck, the body turns about it, and the
//! tracker below follows it. Downward travel along an arc grows the hinge; translation at held height remains
//! walking; a vertical crouch alone does not become a forward bend.
use crate::cmath::{acos, asin, atan2, clamp, cos, hypot, max, min, remainder, sin, sqrt};

pub const EYE_ABOVE_NECK: f32 = 0.10;
pub const EYE_AHEAD_OF_NECK: f32 = 0.09;
pub const TURN_LEVER_SLACK: f32 = 0.04;
pub const BEND_REACH: f32 = 0.63;
pub const MAX_BEND: f32 = 1.22173048;
pub const BEND_SPAN_START: f32 = 0.24;
pub const BEND_SPAN_FULL: f32 = 0.33;
pub const BEND_DROP_START: f32 = 0.020;
pub const BEND_DROP_FULL: f32 = 0.048;
pub const FLOOR_REACH_DROP_START: f32 = 0.16;
pub const FLOOR_REACH_DROP_FULL: f32 = 0.29;
pub const BEND_WALK_BOB_DROP: f32 = 0.03;
pub const BEND_DROP_NOISE: f32 = 0.002;
pub const BEND_STILL_TRAVEL: f32 = 0.003;
pub const BEND_STEADY_UPDATES: i32 = 12;
pub const BEND_NOISE_BLEED: f32 = 0.02;

pub fn smooth_range(value: f32, from: f32, to: f32) -> f32 {
    let t = clamp((value - from) / (to - from), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn bend_weight(angle: f32) -> f32 {
    if angle.is_finite() {
        smooth_range(angle, 0.08726646, 0.43633231)
    } else {
        0.0
    }
}

pub fn hinge_drop(angle: f32) -> f32 {
    if angle.is_finite() {
        BEND_REACH * (1.0 - cos(clamp(angle, 0.0, MAX_BEND)))
    } else {
        0.0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BendSample {
    pub right: f32,
    pub forward: f32,
    pub angle: f32,
}

/// Same field order as the C++ class (BodyBend.hpp's private members).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendTracker {
    head: [f32; 3],
    lever: [f32; 3],
    neck: [f32; 3],
    accepted_neck: [f32; 3],
    right: f32,
    forward: f32,
    drop: f32,
    last_drop: f32,
    measured_angle: f32,
    squat_depth: f32,
    last_yaw: f32,
    have_yaw: bool,
    still_neck: [f32; 3],
    still_drop: f32,
    steady: i32,
    origin: u64,
    initialized: bool,
    sample: BendSample,
}

impl Default for BendTracker {
    fn default() -> Self {
        BendTracker {
            head: [0.0; 3],
            lever: [0.0; 3],
            neck: [0.0; 3],
            accepted_neck: [0.0; 3],
            right: 0.0,
            forward: 0.0,
            drop: 0.0,
            last_drop: 0.0,
            measured_angle: 0.0,
            squat_depth: 0.45,
            last_yaw: 0.0,
            have_yaw: false,
            still_neck: [0.0; 3],
            still_drop: 0.0,
            steady: 0,
            origin: 0,
            initialized: false,
            sample: BendSample::default(),
        }
    }
}

fn arc(loss: f32) -> f32 {
    let loss = clamp(loss, 0.0, BEND_REACH);
    min(BEND_REACH * sin(MAX_BEND), sqrt(max(0.0, loss * (2.0 * BEND_REACH - loss))))
}

fn loss(span: f32) -> f32 {
    BEND_REACH - sqrt(max(0.0, BEND_REACH * BEND_REACH - span * span))
}

impl BendTracker {
    pub fn neck_drop(&self) -> f32 {
        max(0.0, -self.neck[1])
    }
    pub fn measured_angle(&self) -> f32 {
        self.measured_angle
    }
    pub fn set_squat_depth(&mut self, depth: f32) {
        if depth.is_finite() {
            self.squat_depth = clamp(depth, 0.15, 0.55);
        }
    }

    pub fn update(&mut self, head: [f32; 3], q: [f32; 4], origin: u64, swimming: bool, walking: bool) -> BendSample {
        if swimming {
            *self = BendTracker::default();
            return BendSample::default();
        }
        for v in head {
            if !v.is_finite() {
                return self.sample;
            }
        }
        let mut norm = 0.0f32;
        for v in q {
            if !v.is_finite() {
                return self.sample;
            }
            norm += v * v;
        }
        if norm < 0.5 || norm > 1.5 {
            return self.sample;
        }
        if !self.initialized || self.origin != origin {
            *self = BendTracker::default();
            self.origin = origin;
            self.initialized = true;
        }
        let inv = 1.0 / sqrt(norm);
        let (x, y, z, w) = (q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv);
        let v = [0.0f32, EYE_ABOVE_NECK, -EYE_AHEAD_OF_NECK];
        let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
        let lever = [
            w * t[0] + y * t[2] - z * t[1],
            w * t[1] + z * t[0] - x * t[2],
            w * t[2] + x * t[1] - y * t[0],
        ];
        let mut raw = [0.0f32; 3];
        let mut expected = [0.0f32; 3];
        let mut travel = [0.0f32; 3];
        let (mut projection, mut expected_sq) = (0.0f32, 0.0f32);
        for i in 0..3 {
            raw[i] = head[i] - self.head[i];
            expected[i] = lever[i] - self.lever[i];
            projection += raw[i] * expected[i];
            expected_sq += expected[i] * expected[i];
        }
        let lever_weight =
            if expected_sq > 1e-10 { clamp((projection / expected_sq - 0.25) / 0.5, 0.0, 1.0) } else { 0.0 };
        let previous_neck = self.neck;
        for i in 0..3 {
            travel[i] = raw[i] - expected[i] * lever_weight;
            self.neck[i] += travel[i];
        }
        self.head = head;
        self.lever = lever;
        {
            let turned = |vx: f32, vy: f32, vz: f32| {
                let tx = 2.0 * (y * vz - z * vy);
                let ty = 2.0 * (z * vx - x * vz);
                let tz = 2.0 * (x * vy - y * vx);
                [vx + w * tx + y * tz - z * ty, vy + w * ty + z * tx - x * tz, vz + w * tz + x * ty - y * tx]
            };
            let f = turned(0.0, 0.0, -1.0);
            let u = turned(0.0, 1.0, 0.0);
            let (mut hx, mut hz) = (f[0], f[2]);
            if hx * hx + hz * hz < 0.09 {
                hx = if f[1] < 0.0 { u[0] } else { -u[0] };
                hz = if f[1] < 0.0 { u[2] } else { -u[2] };
            }
            let yaw = atan2(-hx, -hz);
            if self.have_yaw {
                let explained = TURN_LEVER_SLACK * remainder(yaw - self.last_yaw, 6.28318531).abs();
                let along = hypot(travel[0], travel[2]);
                if along > 1e-9 && explained > 0.0 {
                    let share = min(1.0, explained / along);
                    self.accepted_neck[0] += travel[0] * share;
                    self.accepted_neck[2] += travel[2] * share;
                }
            }
            self.last_yaw = yaw;
            self.have_yaw = true;
        }
        if lever[1].abs() < 0.0001 {
            self.neck[1] = head[1];
        }
        let drop = max(0.0, -self.neck[1]);
        if drop == self.last_drop && hypot(self.neck[0] - previous_neck[0], self.neck[2] - previous_neck[2]) > 1e-6 {
            self.right = self.sample.right;
            self.forward = self.sample.forward;
            self.drop = drop;
            self.accepted_neck = previous_neck;
        }
        self.last_drop = drop;
        if drop < BEND_DROP_NOISE {
            self.right *= 1.0 - BEND_NOISE_BLEED;
            self.forward *= 1.0 - BEND_NOISE_BLEED;
        }
        let delta_drop = drop - self.drop;
        let dx = self.neck[0] - self.accepted_neck[0];
        let df = -(self.neck[2] - self.accepted_neck[2]);
        let length = hypot(dx, df);
        let (mut right, mut forward) = (self.right, self.forward);
        let mut span = hypot(right, forward);
        if delta_drop > 0.0 && length > 1e-7 && (!walking || drop > BEND_WALK_BOB_DROP) {
            let growth = max(0.0, arc(loss(span) + delta_drop) - span);
            let fraction = min(length, growth) / length * smooth_range(delta_drop, 0.0, BEND_DROP_NOISE);
            right += dx * fraction;
            forward += df * fraction;
        } else if delta_drop < 0.0 && span > 1e-7 {
            let inward = max(0.0, -(dx * right + df * forward) / span);
            let shrink = max(0.0, span - arc(max(0.0, loss(span) + delta_drop)));
            let fraction = 1.0 - min(inward, shrink) / span;
            right *= fraction;
            forward *= fraction;
        }
        span = hypot(right, forward);
        let allowed = arc(if drop > 0.0 && delta_drop.abs() < BEND_DROP_NOISE { max(drop, self.drop) } else { drop });
        if span > allowed && span > 1e-7 {
            right *= allowed / span;
            forward *= allowed / span;
            span = allowed;
        }
        let neck = self.neck;
        let still = |s: &mut BendTracker| {
            s.steady = 0;
            s.still_neck = neck;
            s.still_drop = drop;
        };
        if delta_drop.abs() >= 0.015 || drop == 0.0 {
            self.right = right;
            self.forward = forward;
            self.drop = drop;
            self.accepted_neck = self.neck;
            still(self);
        } else if length > 0.18 {
            self.drop = drop;
            self.accepted_neck = self.neck;
            still(self);
        } else if hypot(self.neck[0] - self.still_neck[0], self.neck[2] - self.still_neck[2]) < BEND_STILL_TRAVEL
            && (drop - self.still_drop).abs() < BEND_STILL_TRAVEL
        {
            self.steady += 1;
            if self.steady >= BEND_STEADY_UPDATES {
                self.right = right;
                self.forward = forward;
                self.drop = drop;
                self.accepted_neck = self.neck;
                still(self);
            }
        } else {
            still(self);
        }
        let weight = smooth_range(span, BEND_SPAN_START, BEND_SPAN_FULL) * smooth_range(drop, BEND_DROP_START, BEND_DROP_FULL);
        let measured = asin(clamp(span / BEND_REACH, 0.0, 1.0)) * weight;
        self.measured_angle = measured;
        let down = clamp(2.0 * (y * z - w * x), -1.0, 1.0);
        let handover = clamp(self.squat_depth - 0.16, FLOOR_REACH_DROP_START, self.squat_depth - 0.05);
        let reach = smooth_range(down, 0.25, 0.75)
            * smooth_range(drop, FLOOR_REACH_DROP_START, FLOOR_REACH_DROP_FULL)
            * (1.0 - smooth_range(drop, handover, self.squat_depth));
        let inferred = acos(clamp(1.0 - min(drop, hinge_drop(MAX_BEND)) / BEND_REACH, 0.0, 1.0));
        self.sample = BendSample { right, forward, angle: max(measured, inferred * reach) };
        self.sample
    }
}
