//! The physical crouch (formerly include/Runtimes/PhysicalCrouch.hpp): a height loss the measured hinge does not
//! explain, held past a threshold for 80 ms, with hysteresis (out at 60 % of the threshold).
use crate::bend::{BendSample, BEND_REACH, MAX_BEND};
use crate::cmath::{clamp, hypot, max, min, sin, sqrt};

pub fn crouch_threshold(value: f32) -> f32 {
    if value.is_finite() {
        clamp(value, 0.15, 0.55)
    } else {
        0.45
    }
}

/// Only measured horizontal hinge travel explains away a height loss; gaze cannot turn a squat into a bend.
pub fn crouch_depth(neck_drop: f32, bend: &BendSample) -> f32 {
    let span = min(hypot(bend.right, bend.forward), BEND_REACH * sin(MAX_BEND));
    let hinge = BEND_REACH - sqrt(max(0.0, BEND_REACH * BEND_REACH - span * span));
    max(0.0, neck_drop - hinge)
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalCrouch {
    origin: u64,
    owner: u64,
    stamp: u64,
    candidate_since: u64,
    active: bool,
    candidate: bool,
}

impl PhysicalCrouch {
    pub fn reset(&mut self) {
        *self = PhysicalCrouch::default();
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn update(&mut self, depth: f32, threshold: f32, now: u64, origin: u64, owner: u64, allowed: bool) -> bool {
        if !allowed || origin == 0 || owner == 0 || now == 0 || !depth.is_finite() || depth > 1.5 {
            self.reset();
            return false;
        }
        if origin != self.origin
            || owner != self.owner
            || (self.stamp != 0 && (now < self.stamp || now - self.stamp > 150000))
        {
            self.reset();
            self.origin = origin;
            self.owner = owner;
        }
        self.stamp = now;
        let enter = crouch_threshold(threshold);
        let candidate = if self.active { depth > enter * 0.60 } else { depth >= enter };
        if candidate != self.candidate {
            self.candidate = candidate;
            self.candidate_since = now;
        }
        if candidate != self.active && now - self.candidate_since >= 80000 {
            self.active = candidate;
        }
        self.active
    }
}
