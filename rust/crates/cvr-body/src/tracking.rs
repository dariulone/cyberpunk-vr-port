//! Tracking's small rules (formerly include/Runtimes/{TrackingFilter,RoomscaleTracking,TrackingReset,
//! HandPublication,FrameAim}.hpp): the exact first-order follow, when head tracking is usable, the reset gate, which
//! side publishes the hands, and the frame aim published as one snapshot.
use crate::cmath::expm1;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Exact first-order response for elapsed seconds: subdividing an interval changes nothing, no snap at dt = 1/rate.
pub fn exponential_follow(rate: f32, dt: f32) -> f32 {
    if !rate.is_finite() || !dt.is_finite() || rate <= 0.0 || dt <= 0.0 {
        return 0.0;
    }
    -expm1(-rate * dt)
}

/// XrSessionState values and XrSpaceLocationFlags bits this needs.
pub const XR_SESSION_STATE_VISIBLE: i32 = 4;
pub const XR_SESSION_STATE_FOCUSED: i32 = 5;
pub const XR_SPACE_LOCATION_ORIENTATION_VALID_BIT: u64 = 0x1;
pub const XR_SPACE_LOCATION_POSITION_VALID_BIT: u64 = 0x2;
pub const XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT: u64 = 0x4;
pub const XR_SPACE_LOCATION_POSITION_TRACKED_BIT: u64 = 0x8;

/// FOCUS gates action input, not the validity of head tracking (the simulator updates poses while VISIBLE).
pub const fn head_tracking_usable(state: i32, flags: u64) -> bool {
    const REQUIRED: u64 = XR_SPACE_LOCATION_POSITION_VALID_BIT
        | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT
        | XR_SPACE_LOCATION_POSITION_TRACKED_BIT
        | XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT;
    (state == XR_SESSION_STATE_VISIBLE || state == XR_SESSION_STATE_FOCUSED) && (flags & REQUIRED) == REQUIRED
}

/// A recenter in progress: the serial is odd while it runs; a pose located at an even serial equal to the current
/// one is from after it.
#[derive(Debug, Default)]
pub struct TrackingResetGate {
    serial: AtomicU64,
    applied: AtomicU64,
}

impl TrackingResetGate {
    pub fn begin(&self) {
        self.serial.fetch_add(1, Ordering::AcqRel);
    }
    pub fn end(&self) {
        self.serial.fetch_add(1, Ordering::Release);
    }
    pub fn serial(&self) -> u64 {
        self.serial.load(Ordering::Acquire)
    }
    pub fn pending(&self) -> bool {
        self.serial() != self.applied.load(Ordering::Acquire)
    }
    pub fn ready(&self, located_at: u64) -> bool {
        located_at & 1 == 0 && located_at == self.serial()
    }
    pub fn applied(&self, located_at: u64) {
        self.applied.store(located_at, Ordering::Release);
    }
}

/// The native tick owns on-foot hands; Present resumes publication when native movement stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HandPublicationGate {
    stamp: u64,
    origin: u64,
}

impl HandPublicationGate {
    pub const MAX_AGE_US: u64 = 250000;
    pub fn native_owns(&self, now: u64, origin: u64, eligible: bool) -> bool {
        eligible && self.stamp != 0 && self.origin == origin && now >= self.stamp && now - self.stamp <= Self::MAX_AGE_US
    }
    pub fn published_native(&mut self, now: u64, origin: u64) {
        self.stamp = now;
        self.origin = origin;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameAimSnapshot {
    pub time: i64,
    pub stamp_us: u64,
    pub epoch: u64,
}

/// The target time, publication time and identity of one update, read together.
#[derive(Debug, Default)]
pub struct FrameAim {
    value: Mutex<FrameAimSnapshot>,
}

impl FrameAim {
    pub fn publish(&self, time: i64, stamp_us: u64) {
        let mut v = self.value.lock().unwrap_or_else(|p| p.into_inner());
        *v = FrameAimSnapshot { time, stamp_us, epoch: v.epoch + 1 };
    }
    pub fn read(&self) -> FrameAimSnapshot {
        *self.value.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gates() {
        assert!(head_tracking_usable(XR_SESSION_STATE_VISIBLE, 0xF));
        assert!(!head_tracking_usable(XR_SESSION_STATE_VISIBLE, 0x7));
        assert!(!head_tracking_usable(3, 0xF));
        let g = TrackingResetGate::default();
        assert!(!g.pending() && g.ready(0));
        g.begin();
        assert!(g.pending() && !g.ready(1));
        g.end();
        assert!(g.ready(2) && g.pending());
        g.applied(2);
        assert!(!g.pending());
        let a = FrameAim::default();
        a.publish(5, 6);
        a.publish(7, 8);
        assert_eq!(a.read(), FrameAimSnapshot { time: 7, stamp_us: 8, epoch: 2 });
        let mut h = HandPublicationGate::default();
        assert!(!h.native_owns(10, 1, true));
        h.published_native(10, 1);
        assert!(h.native_owns(10, 1, true) && !h.native_owns(10, 2, true) && !h.native_owns(250011, 1, true));
    }
}
