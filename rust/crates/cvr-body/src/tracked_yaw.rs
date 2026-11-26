//! The torso's yaw from the head and both hands (formerly include/Runtimes/TrackedBodyYaw.hpp). All three devices
//! belong to one locate time and origin; the hands are raw HMD-local positions, before weapon offsets or hand
//! interpolation. An estimate of the torso's heading, not a waist tracker. The reasons behind every gate are written
//! at the C++ original's lines (git history of that header); the arithmetic here is the same, operation for operation.
use crate::cmath::{atan2, clamp, cos, expm1, hypot, max, min, remainder, sin, sqrt};
use crate::prediction::BodyYawPrediction;
use crate::xr::{conjugate_quat, multiply_quat, nlerp_quat, rotate_vector, v3, Quat, Vec3};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackingFrame {
    pub head: Quat,
    /// Recentered tracking-base coordinates.
    pub head_position: Vec3,
    pub hands: [Vec3; 2],
    pub rotations: [Quat; 2],
    pub valid: bool,
    pub tracked: [bool; 2],
    pub sequence: u64,
    pub stamp_us: u64,
    pub origin: u64,
}

impl Default for TrackingFrame {
    fn default() -> Self {
        TrackingFrame {
            head: Quat::IDENTITY,
            head_position: Vec3::default(),
            hands: [Vec3::default(); 2],
            rotations: [Quat::IDENTITY; 2],
            valid: false,
            tracked: [false; 2],
            sequence: 0,
            stamp_us: 0,
            origin: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct YawEstimate {
    pub yaw: f32,
    pub confidence: f32,
    pub valid: bool,
}

const fn est(yaw: f32, confidence: f32, valid: bool) -> YawEstimate {
    YawEstimate { yaw, confidence, valid }
}

pub fn tracking_frame_fresh(frame: &TrackingFrame, current_origin: u64, now_us: u64) -> bool {
    frame.valid
        && frame.origin == current_origin
        && current_origin != 0
        && frame.sequence != 0
        && frame.stamp_us != 0
        && now_us >= frame.stamp_us
        && now_us - frame.stamp_us <= 150000
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Observation {
    hand: [Vec3; 2],
    world_hand: [Vec3; 2],
    rotation: [Quat; 2],
    head_rotation: Quat,
}

/// The last update's gates, for the debug export (bit meanings at the C++ header's `Trace`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trace {
    pub delta: f32,
    pub wrist_mean: f32,
    pub head_delta: f32,
    pub head_lead: f32,
    pub accepted: f32,
    pub evidence: f32,
    pub residual: f32,
    pub target: f32,
    pub bits: u32,
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackedBodyYaw {
    anchor: Observation,
    filtered: Observation,
    motion_witness: Observation,
    origin: u64,
    sequence: u64,
    stamp: u64,
    acquire_start: u64,
    rebase_until: u64,
    anchor_yaw: f32,
    target: f32,
    output: f32,
    evidence: f32,
    head_lead: f32,
    kept_stamp: u64,
    kept_anchor: bool,
    have_anchor: bool,
    have_filter: bool,
    absolute_initial: bool,
    have_motion_witness: bool,
    rigid_held: bool,
    pub trace: Trace,
    prediction: BodyYawPrediction,
}

impl Default for TrackedBodyYaw {
    fn default() -> Self {
        TrackedBodyYaw {
            anchor: Observation::default(),
            filtered: Observation::default(),
            motion_witness: Observation::default(),
            origin: 0,
            sequence: 0,
            stamp: 0,
            acquire_start: 0,
            rebase_until: 0,
            anchor_yaw: 0.0,
            target: 0.0,
            output: 0.0,
            evidence: 0.0,
            head_lead: 0.0,
            kept_stamp: 0,
            kept_anchor: false,
            have_anchor: false,
            have_filter: false,
            absolute_initial: true,
            have_motion_witness: false,
            rigid_held: false,
            trace: Trace::default(),
            prediction: BodyYawPrediction::default(),
        }
    }
}

const PI: f32 = 3.14159265359;
const HEAD_LEAD_SLACK: f32 = 0.05;

fn wrap(a: f32) -> f32 {
    remainder(a, 2.0 * PI)
}
fn newer(a: u64, b: u64) -> bool {
    if a <= u32::MAX as u64 && b <= u32::MAX as u64 {
        return ((a as u32).wrapping_sub(b as u32) as i32) > 0;
    }
    a > b
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x - b.x, a.y - b.y, a.z - b.z)
}
fn mid(a: Vec3, b: Vec3) -> Vec3 {
    v3((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5)
}
fn radius(a: Vec3) -> f32 {
    hypot(a.x, a.z)
}
fn length(a: Vec3) -> f32 {
    sqrt(a.x * a.x + a.y * a.y + a.z * a.z)
}
fn finite(a: Vec3) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}
fn normalize(q: &mut Quat) -> bool {
    let n = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if !n.is_finite() || n < 0.5 || n > 1.5 {
        return false;
    }
    let inv = 1.0 / sqrt(n);
    q.x *= inv;
    q.y *= inv;
    q.z *= inv;
    q.w *= inv;
    true
}
fn rotation_yaw(from: Quat, to: Quat) -> f32 {
    let q = multiply_quat(to, conjugate_quat(from));
    wrap(2.0 * atan2(q.y, q.w))
}
fn angle(from: Vec3, to: Vec3) -> f32 {
    atan2(from.z * to.x - from.x * to.z, from.x * to.x + from.z * to.z)
}
fn rotate(p: Vec3, a: f32) -> Vec3 {
    let (c, s) = (cos(a), sin(a));
    v3(c * p.x + s * p.z, p.y, -s * p.x + c * p.z)
}
fn decode(frame: &TrackingFrame, out: &mut Observation) -> bool {
    if !frame.valid || !frame.tracked[0] || !frame.tracked[1] {
        return false;
    }
    if !finite(frame.head_position) || length(frame.head_position) > 1000.0 {
        return false;
    }
    let mut q = frame.head;
    if !normalize(&mut q) {
        return false;
    }
    out.head_rotation = q;
    for h in 0..2 {
        if !finite(frame.hands[h]) || length(frame.hands[h]) < 0.10 || length(frame.hands[h]) > 1.6 {
            return false;
        }
        out.hand[h] = rotate_vector(q, sub(frame.hands[h], v3(0.0, -0.10, 0.08)));
        let relative = rotate_vector(q, frame.hands[h]);
        out.world_hand[h] = v3(
            frame.head_position.x + relative.x,
            frame.head_position.y + relative.y,
            frame.head_position.z + relative.z,
        );
        let mut hand_rotation = frame.rotations[h];
        if !normalize(&mut hand_rotation) {
            return false;
        }
        out.rotation[h] = multiply_quat(q, hand_rotation);
    }
    true
}
fn turn_across_gap(before: &Observation, after: &Observation, turn: &mut f32) -> bool {
    let w0 = rotation_yaw(before.rotation[0], after.rotation[0]);
    let w1 = rotation_yaw(before.rotation[1], after.rotation[1]);
    let mean = w0 + wrap(w1 - w0) * 0.5;
    if wrap(w0 - w1).abs() > 0.10 + 0.15 * mean.abs() {
        return false;
    }
    for h in 0..2 {
        if length(sub(after.hand[h], rotate(before.hand[h], mean))) > 0.12 {
            return false;
        }
    }
    *turn = mean;
    true
}
fn rest(p: &Observation) -> bool {
    p.hand[0].y < -0.35
        && p.hand[1].y < -0.35
        && (p.hand[0].y - p.hand[1].y).abs() < 0.25
        && radius(sub(p.hand[1], p.hand[0])) > 0.28
        && radius(p.hand[0]) < 0.65
        && radius(p.hand[1]) < 0.65
}

impl TrackedBodyYaw {
    fn anchor_to(&mut self, value: &Observation, continuous: bool) {
        let lead = if continuous && self.have_anchor {
            clamp(
                wrap(self.head_lead + rotation_yaw(self.anchor.head_rotation, value.head_rotation)
                    - wrap(self.target - self.anchor_yaw)),
                -1.6,
                1.6,
            )
        } else {
            0.0
        };
        if continuous {
            self.head_lead = lead;
        }
        self.anchor = *value;
        self.anchor_yaw = self.target;
        self.have_anchor = true;
        self.motion_witness = *value;
        self.have_motion_witness = true;
        self.rebase_until = if continuous { self.stamp + 50000 } else { 0 };
        if !continuous {
            self.evidence = 0.0;
            self.head_lead = 0.0;
            self.prediction.suspend();
        }
    }

    fn reshape(&mut self, value: &Observation) {
        let lead = clamp(
            wrap(self.head_lead + rotation_yaw(self.anchor.head_rotation, value.head_rotation)
                - wrap(self.target - self.anchor_yaw)),
            -1.6,
            1.6,
        );
        self.anchor_to(value, false);
        self.head_lead = lead;
    }

    pub fn reset(&mut self, seed: f32, use_absolute_initial: bool) {
        *self = TrackedBodyYaw::default();
        self.absolute_initial = use_absolute_initial;
        let s = if seed.is_finite() { wrap(seed) } else { 0.0 };
        self.target = s;
        self.output = s;
        self.anchor_yaw = s;
        self.prediction.reset(self.output);
    }

    pub fn suspend(&mut self) {
        self.trace.bits |= 1 << 27;
        if self.have_anchor {
            self.kept_anchor = true;
            self.kept_stamp = self.stamp;
        }
        self.have_anchor = false;
        self.have_filter = false;
        self.rigid_held = false;
        self.stamp = 0;
        self.sequence = 0;
        self.evidence = 0.0;
        self.target = self.output;
        self.prediction.reset(self.output);
    }

    pub fn ready(&self) -> bool {
        self.have_anchor
    }

    pub fn predict(&self, now_us: u64) -> YawEstimate {
        if !self.have_anchor || self.stamp == 0 || now_us < self.stamp || now_us - self.stamp > 150000 {
            return est(self.output, 0.0, false);
        }
        est(
            self.prediction.sample((now_us - self.stamp) as f32 * 1e-6),
            min(1.0, self.evidence / 0.045),
            true,
        )
    }

    pub fn update(&mut self, frame: &TrackingFrame) -> YawEstimate {
        if self.origin != 0 && frame.origin < self.origin {
            return est(self.output, 0.0, false);
        }
        if self.origin != 0 && frame.origin != self.origin {
            let absolute = self.absolute_initial;
            self.reset(0.0, absolute);
        }
        self.origin = frame.origin;
        let mut current = Observation::default();
        if frame.origin == 0 || frame.stamp_us == 0 || frame.sequence == 0 || !decode(frame, &mut current) {
            self.suspend();
            return est(self.output, 0.0, false);
        }
        let unfiltered = current;
        if self.stamp != 0 && (frame.stamp_us <= self.stamp || !newer(frame.sequence, self.sequence)) {
            return est(self.output, 0.0, true);
        }
        let dt = if self.stamp != 0 { (frame.stamp_us - self.stamp) as f32 * 1e-6 } else { 0.0 };
        if dt > 0.20 {
            // A FREEZE: both wrists turned by one yaw and both hands moved round the neck with them is the torso
            // turned, carried across the hitch.
            let mut turn = 0.0;
            if self.have_anchor && self.have_filter && turn_across_gap(&self.filtered, &current, &mut turn) {
                self.trace.bits = 1 << 28;
                self.output = wrap(self.target + turn);
                self.target = self.output;
                self.prediction.reset(self.output);
                self.stamp = frame.stamp_us;
                self.sequence = frame.sequence;
                self.rigid_held = false;
                self.filtered = current;
                self.anchor_to(&current, true);
                return est(self.output, 1.0, true);
            }
            self.suspend();
        }
        self.stamp = frame.stamp_us;
        self.sequence = frame.sequence;
        if !self.have_filter {
            self.filtered = current;
            self.have_filter = true;
            self.acquire_start = self.stamp;
        } else {
            let jump_limit = 25.0 * max(dt, 0.0) + 0.08;
            let head_jump = rotation_yaw(self.filtered.head_rotation, current.head_rotation);
            let wrist_jump = [
                rotation_yaw(self.filtered.rotation[0], current.rotation[0]),
                rotation_yaw(self.filtered.rotation[1], current.rotation[1]),
            ];
            if head_jump.abs() > jump_limit && wrist_jump[0].abs() > jump_limit && wrist_jump[1].abs() > jump_limit {
                // THE WHOLE PLAYER TURNED AT ONCE: carried when rigid and confirmed by the next sample.
                let tolerance = 0.10 + 0.15 * head_jump.abs();
                let rigid = self.have_anchor
                    && wrap(wrist_jump[0] - head_jump).abs() < tolerance
                    && wrap(wrist_jump[1] - head_jump).abs() < tolerance;
                self.trace.bits = 1 << 26;
                if !rigid {
                    self.suspend();
                    return est(self.output, 0.0, false);
                }
                if !self.rigid_held {
                    self.rigid_held = true;
                    return est(self.output, 0.0, true);
                }
                self.rigid_held = false;
                self.output = wrap(self.target + head_jump);
                self.target = self.output;
                self.prediction.reset(self.output);
                self.filtered = current;
                self.anchor_to(&current, true);
                return est(self.output, 1.0, true);
            }
            self.rigid_held = false;
            for h in 0..2 {
                if length(sub(current.hand[h], self.filtered.hand[h])) > 0.5 {
                    self.suspend();
                    return est(self.output, 0.0, false);
                }
                let a = -expm1(-min(dt, 0.1) / 0.025);
                let f = &mut self.filtered;
                f.hand[h].x += (current.hand[h].x - f.hand[h].x) * a;
                f.hand[h].y += (current.hand[h].y - f.hand[h].y) * a;
                f.hand[h].z += (current.hand[h].z - f.hand[h].z) * a;
                f.world_hand[h].x += (current.world_hand[h].x - f.world_hand[h].x) * a;
                f.world_hand[h].y += (current.world_hand[h].y - f.world_hand[h].y) * a;
                f.world_hand[h].z += (current.world_hand[h].z - f.world_hand[h].z) * a;
            }
            let rot_alpha = if self.have_anchor { 1.0 } else { -expm1(-min(dt, 0.1) / 0.025) };
            self.filtered.head_rotation = nlerp_quat(self.filtered.head_rotation, current.head_rotation, rot_alpha);
            self.filtered.rotation[0] = nlerp_quat(self.filtered.rotation[0], current.rotation[0], rot_alpha);
            self.filtered.rotation[1] = nlerp_quat(self.filtered.rotation[1], current.rotation[1], rot_alpha);
        }
        let current = self.filtered;
        if !self.have_anchor {
            self.trace.bits = 1 << 24;
            if self.stamp - self.acquire_start < 120000 {
                return est(self.output, 0.0, true);
            }
            let centre = mid(current.hand[0], current.hand[1]);
            let carry = self.kept_anchor
                && self.kept_stamp != 0
                && self.stamp > self.kept_stamp
                && self.stamp - self.kept_stamp < 1500000;
            self.kept_anchor = false;
            if carry {
                self.reshape(&current);
                return est(self.output, 0.0, true);
            }
            if self.absolute_initial && rest(&current) && radius(centre) < 0.14 {
                let line = sub(current.hand[1], current.hand[0]);
                let yaw = atan2(-line.z, line.x);
                if wrap(yaw - self.output).abs() <= PI * 0.5 + 0.00001 {
                    self.target = yaw;
                }
            }
            self.anchor_to(&current, false);
            return est(self.output, 0.0, true);
        }
        let anchor = self.anchor;
        let before = sub(anchor.hand[1], anchor.hand[0]);
        let after = sub(current.hand[1], current.hand[0]);
        let use_line = radius(before) > 0.14 && radius(after) > 0.14;
        let old_axis = if use_line { before } else { mid(anchor.hand[0], anchor.hand[1]) };
        let new_axis = if use_line { after } else { mid(current.hand[0], current.hand[1]) };
        if radius(old_axis) < 0.12 || radius(new_axis) < 0.12 {
            self.trace.bits = 1 << 25;
            self.reshape(&current);
            return est(self.output, 0.0, true);
        }
        let delta = angle(old_axis, new_axis);
        let mut hand_delta = [0.0f32; 2];
        let mut residual = 0.0f32;
        let mut radial = true;
        for h in 0..2 {
            radial &= radius(anchor.hand[h]) > 0.10 && radius(current.hand[h]) > 0.10;
            hand_delta[h] = angle(anchor.hand[h], current.hand[h]);
            residual = max(residual, length(sub(current.hand[h], rotate(anchor.hand[h], delta))));
        }
        let magnitude = max(hand_delta[0].abs(), hand_delta[1].abs());
        let agreement = if magnitude > 0.001 { min(hand_delta[0].abs(), hand_delta[1].abs()) / magnitude } else { 1.0 };
        let head_delta = rotation_yaw(anchor.head_rotation, current.head_rotation);
        let accepted = wrap(self.target - self.anchor_yaw);
        let head_lead = self.head_lead;
        let closes_on_head = |turn: f32| {
            let to_head = wrap(head_lead + head_delta - accepted);
            let moved = wrap(turn - accepted);
            moved * to_head > 0.0 && moved.abs() <= to_head.abs() + HEAD_LEAD_SLACK
        };
        let same_hands = hand_delta[0] * hand_delta[1] >= 0.0
            && agreement > 0.55
            && wrap(hand_delta[0] - delta).abs() < 0.008 + 0.30 * delta.abs()
            && wrap(hand_delta[1] - delta).abs() < 0.008 + 0.30 * delta.abs();
        let head_supports =
            (head_delta * delta >= 0.0 && head_delta.abs() >= 0.40 * delta.abs()) || closes_on_head(delta);
        let mut orientations_agree = true;
        let mut one_orientation_agrees = false;
        let mut wrist_delta = [0.0f32; 2];
        for h in 0..2 {
            let a = rotation_yaw(anchor.rotation[h], current.rotation[h]);
            wrist_delta[h] = a;
            let agrees = a * delta >= 0.0
                && a.abs() >= 0.40 * delta.abs()
                && wrap(a - delta).abs() < 0.025 + 0.35 * delta.abs();
            orientations_agree &= agrees;
            one_orientation_agrees |= agrees;
        }
        let rest_supports = use_line && rest(&anchor) && rest(&current) && orientations_agree;
        let wrist_mean = wrist_delta[0] + wrap(wrist_delta[1] - wrist_delta[0]) * 0.5;
        let wrist_max = max(wrist_delta[0].abs(), wrist_delta[1].abs());
        let mut wrist_consensus = wrist_delta[0] * wrist_delta[1] >= 0.0
            && (wrist_max < 0.001 || min(wrist_delta[0].abs(), wrist_delta[1].abs()) > 0.55 * wrist_max)
            && wrap(wrist_delta[0] - wrist_delta[1]).abs() < 0.015 + 0.25 * wrist_mean.abs()
            && ((head_delta * wrist_mean >= 0.0 && head_delta.abs() >= 0.40 * wrist_mean.abs())
                || closes_on_head(wrist_mean));
        let rotations_confirm = wrist_consensus && wrist_max > 0.003;
        let consensus_before = wrist_consensus;
        let rot_head_support = head_delta * wrist_mean >= 0.0 && head_delta.abs() >= 0.40 * wrist_mean.abs();
        let rot_closes_on_head = closes_on_head(wrist_mean);
        let (mut witness_hand, mut witness_world, mut witness_motion) = (true, true, true);
        let raw_axis = if use_line {
            sub(unfiltered.hand[1], unfiltered.hand[0])
        } else {
            mid(unfiltered.hand[0], unfiltered.hand[1])
        };
        let raw_geometry_supports = wrap(angle(old_axis, raw_axis) - wrist_mean).abs() < 0.030;
        for h in 0..2 {
            let expected = length(sub(rotate(anchor.hand[h], wrist_mean), anchor.hand[h]));
            let witness = if self.stamp < self.rebase_until
                || self.prediction.rate().abs() > 0.174532925
                || (raw_geometry_supports && self.evidence >= 0.045)
            {
                &unfiltered
            } else {
                &current
            };
            witness_hand &= length(sub(witness.hand[h], anchor.hand[h])) >= min(expected * 0.15, 0.006);
            wrist_consensus &= length(sub(witness.hand[h], anchor.hand[h])) >= min(expected * 0.15, 0.006);
            witness_world &= length(sub(witness.world_hand[h], anchor.world_hand[h])) >= min(expected * 0.40, 0.012);
            wrist_consensus &= length(sub(witness.world_hand[h], anchor.world_hand[h])) >= min(expected * 0.40, 0.012);
            if self.have_motion_witness && !raw_geometry_supports {
                let mw = &self.motion_witness;
                let fresh_yaw = rotation_yaw(mw.rotation[h], unfiltered.rotation[h]);
                let orbit = sub(rotate(mw.hand[h], fresh_yaw), mw.hand[h]);
                let travel = sub(unfiltered.world_hand[h], mw.world_hand[h]);
                let fresh_orbit = length(orbit);
                let along = travel.x * orbit.x + travel.y * orbit.y + travel.z * orbit.z;
                let active_gesture = length(travel) > 0.012;
                witness_motion &= along >= fresh_orbit * min(fresh_orbit * 0.40, 0.012) || active_gesture;
                wrist_consensus &= along >= fresh_orbit * min(fresh_orbit * 0.40, 0.012) || active_gesture;
            }
        }
        let mut position_motion = true;
        for h in 0..2 {
            let expected = length(sub(rotate(anchor.hand[h], delta), anchor.hand[h]));
            position_motion &= length(sub(current.world_hand[h], anchor.world_hand[h])) >= min(expected * 0.15, 0.006);
        }
        let mut at_reference = wrist_max < 0.003;
        for h in 0..2 {
            at_reference &= length(sub(unfiltered.hand[h], rotate(anchor.hand[h], wrist_mean))) < 0.012;
        }
        let use_wrists = (rotations_confirm && wrist_consensus) || at_reference;
        let coherent = use_wrists
            || (!rotations_confirm
                && radial
                && same_hands
                && residual < 0.035
                && position_motion
                && one_orientation_agrees
                && (use_line || orientations_agree)
                && (head_supports || rest_supports));
        let turn = if use_wrists { wrist_mean } else { delta };
        self.trace = Trace {
            delta,
            wrist_mean,
            head_delta,
            head_lead: self.head_lead,
            accepted,
            evidence: self.evidence,
            residual,
            target: self.target,
            bits: (coherent as u32)
                | (use_wrists as u32) << 1
                | (rotations_confirm as u32) << 2
                | (wrist_consensus as u32) << 3
                | (same_hands as u32) << 4
                | (head_supports as u32) << 5
                | (rest_supports as u32) << 6
                | (position_motion as u32) << 7
                | (radial as u32) << 8
                | (raw_geometry_supports as u32) << 9
                | (at_reference as u32) << 10
                | (orientations_agree as u32) << 11
                | (one_orientation_agrees as u32) << 12
                | (use_line as u32) << 13
                | (consensus_before as u32) << 14
                | (witness_hand as u32) << 15
                | (witness_world as u32) << 16
                | (witness_motion as u32) << 17
                | (rot_head_support as u32) << 18
                | (rot_closes_on_head as u32) << 19,
        };
        if coherent {
            self.evidence += max(0.0, dt);
            if self.evidence >= 0.045 {
                self.target = wrap(self.anchor_yaw + turn);
                self.motion_witness = unfiltered;
                self.have_motion_witness = true;
            }
        } else {
            if !(rotations_confirm && raw_geometry_supports && self.evidence >= 0.045) {
                self.evidence = 0.0;
            }
            let mut raw_residual = 0.0f32;
            for h in 0..2 {
                raw_residual = max(raw_residual, length(sub(unfiltered.hand[h], rotate(anchor.hand[h], delta))));
            }
            let shape_changed = residual > 0.06 && raw_residual > 0.06;
            let span_changed = use_line
                && (radius(after) - radius(before)).abs() > 0.10
                && (radius(sub(unfiltered.hand[1], unfiltered.hand[0])) - radius(before)).abs() > 0.10;
            if !rotations_confirm && (shape_changed || span_changed) {
                self.reshape(&current);
                self.trace.bits |= 1 << 21;
            }
        }
        self.output = self.prediction.update(self.target, dt, coherent && self.evidence >= 0.045);
        if coherent && self.evidence >= 0.045 && turn.abs() > 1.05 {
            let raw_turn = if use_wrists { turn } else { angle(old_axis, raw_axis) };
            let lead = clamp(
                wrap(self.head_lead + rotation_yaw(self.anchor.head_rotation, unfiltered.head_rotation) - raw_turn),
                -1.6,
                1.6,
            );
            self.target = wrap(self.anchor_yaw + raw_turn);
            self.filtered = unfiltered;
            self.anchor_to(&unfiltered, true);
            self.head_lead = lead;
            self.trace.bits |= 1 << 20;
        }
        est(self.output, if coherent { min(1.0, self.evidence / 0.045) } else { 0.0 }, true)
    }
}
