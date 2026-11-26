//! The on-foot body heading follower (formerly include/Runtimes/BodyYawFollower.hpp). The offset belongs to an active
//! follower, never to a suspended scene: a menu only stops `step`, it does not change ownership of the heading.
use crate::cmath::{max, remainder};
use crate::follow_zone::BodyFollowZone;

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyYawFollower {
    zone: BodyFollowZone,
    stamp: u64,
    origin: u64,
    offset: f32,
    enabled: bool,
}

impl BodyYawFollower {
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.reset();
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn offset(&self) -> f32 {
        self.offset
    }
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.stamp = 0;
        self.zone.reset();
    }
    /// Feedback from the native adapter's committed quaternion: a missed request must not become a fictitious applied
    /// turn in the next solve.
    pub fn observe(&mut self, offset: f32, origin: u64) {
        if !offset.is_finite() {
            return;
        }
        if origin != self.origin {
            self.origin = origin;
            self.reset();
        }
        self.offset = remainder(offset, 6.28318530718);
    }
    /// The body's yaw is a model's own output (the head-led turn): set it, no zone. Returns the step (main's Set).
    pub fn set(&mut self, body_yaw: f32, stamp: u64, origin: u64) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        if origin != self.origin {
            self.origin = origin;
            self.reset();
            return 0.0;
        }
        if !body_yaw.is_finite() {
            return 0.0;
        }
        if self.stamp != 0 && stamp <= self.stamp {
            return 0.0;
        }
        self.stamp = stamp;
        let step = remainder(body_yaw - self.offset, 6.28318530718);
        self.offset = remainder(self.offset + step, 6.28318530718);
        step
    }
    /// The seconds since the last applied update (the head-led turn steps with it).
    pub fn since_last(&self, stamp: u64) -> f32 {
        if self.stamp != 0 && stamp > self.stamp { (stamp - self.stamp) as f32 * 0.000001 } else { 0.011111111 }
    }
    pub fn step(&mut self, head_yaw: f32, cone: f32, stamp: u64, origin: u64) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        if origin != self.origin {
            self.origin = origin;
            self.reset();
            return 0.0;
        }
        if !head_yaw.is_finite() || !cone.is_finite() {
            return 0.0;
        }
        if self.stamp != 0 && stamp <= self.stamp {
            return 0.0;
        }
        let dt = if self.stamp != 0 { (stamp - self.stamp) as f32 * 0.000001 } else { 0.011111111 };
        if self.stamp != 0 && stamp - self.stamp > 250000 {
            self.zone.reset();
        }
        self.stamp = stamp;
        let residual = remainder(head_yaw - self.offset, 6.28318530718);
        let step = self.zone.step(residual, max(cone, 0.0), dt);
        self.offset = remainder(self.offset + step, 6.28318530718);
        step
    }
}
