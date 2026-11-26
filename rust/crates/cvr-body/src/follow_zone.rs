//! THE BODY FOLLOWS THE HEAD CONTINUOUSLY (formerly include/Runtimes/BodyFollowZone.hpp). Head tracking is never
//! filtered here; this turns only the body. Its turn RATE is a continuous function of how far the head is from it:
//! pulled hard past the free-look cone (kLeashGain per radian beyond it), then -- once a turn has started -- toward
//! the head at kCentreGain per radian of the whole gap, a gain that grows in over kCentreRamp; the rate itself eases
//! over kRateTau, but a cone widening over the head (looking down) stops a turn at once. A radius of zero takes the
//! whole gap.
use crate::cmath::{expm1, max, min};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyFollowZone {
    pub rate: f32,
    pub blend: f32,
    pub previous_radius: f32,
    pub turning: bool,
}

impl BodyFollowZone {
    pub const CENTRE_DEG: f32 = 0.05;
    pub const CENTRE_GAIN: f32 = 5.0;
    pub const CENTRE_RAMP: f32 = 0.35;
    pub const LEASH_GAIN: f32 = 14.0;
    pub const MAX_RATE: f32 = 6.2831853;
    pub const RATE_TAU: f32 = 0.05;

    pub fn reset(&mut self) {
        self.rate = 0.0;
        self.blend = 0.0;
        self.turning = false;
        self.previous_radius = 0.0;
    }

    pub fn following(&self) -> bool {
        self.turning || self.rate.abs() > 1e-3
    }

    /// The body's yaw step toward the head. residual = head - body (radians, wrapped), radius = the free-look cone
    /// (radians), dt in seconds. Never past the head.
    pub fn step(&mut self, residual: f32, radius: f32, dt: f32) -> f32 {
        if !residual.is_finite() || !radius.is_finite() || !dt.is_finite() || dt <= 0.0 {
            self.reset();
            return 0.0;
        }
        if radius <= 0.0 {
            self.reset();
            return residual;
        }
        let dt = min(dt, 0.25);
        let gap = residual.abs();
        let centre = Self::CENTRE_DEG * 0.01745329252;
        let widened = radius > self.previous_radius + 1e-5;
        self.previous_radius = radius;
        if gap > radius {
            self.turning = true;
        } else if widened {
            self.turning = false;
            self.rate = 0.0;
        }
        if gap <= centre {
            self.turning = false;
        }
        self.blend = if self.turning { min(1.0, self.blend + dt / Self::CENTRE_RAMP) } else { 0.0 };
        let mut target = Self::LEASH_GAIN * max(0.0, gap - radius)
            + if self.turning { self.blend * Self::CENTRE_GAIN * gap } else { 0.0 };
        target = min(target, Self::MAX_RATE) * if residual < 0.0 { -1.0 } else { 1.0 };
        self.rate += (target - self.rate) * -expm1(-dt / Self::RATE_TAU);
        let mut step = self.rate * dt;
        if step * residual <= 0.0 {
            if gap <= centre {
                self.rate = 0.0;
            }
            return 0.0;
        }
        if step.abs() > gap {
            step = residual;
            self.rate = 0.0;
        }
        step
    }
}
