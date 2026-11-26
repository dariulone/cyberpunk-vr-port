//! The panels' yaw-follow (formerly include/Runtimes/HudFollow.hpp): the head's heading, the display timeline's step,
//! and the yaw a menu or HUD panel holds -- still inside a free-look cone, catching up past it (or, with the delayed
//! catch-up, after three seconds at rest inside it). In the Rust build the header's structs keep their layout and call
//! these.
use crate::cmath::{atan2, clamp, remainder, sqrt};

/// cvr::hud::HeadYaw: the heading of an orientation; `fallback` for one that is not an orientation or looks almost
/// straight up or down.
#[no_mangle]
pub extern "C" fn cvr_hud_head_yaw(x: f32, y: f32, z: f32, w: f32, fallback: f32) -> f32 {
    let n = x * x + y * y + z * z + w * w;
    if !n.is_finite() || n < 0.5 || n > 1.5 {
        return fallback;
    }
    let inv = 1.0 / sqrt(n);
    let (x, y, z, w) = (x * inv, y * inv, z * inv, w * inv);
    let fx = 2.0 * (w * y + x * z);
    let fz = 1.0 - 2.0 * (x * x + y * y);
    if fx * fx + fz * fz < 0.01 {
        return fallback;
    }
    atan2(fx, fz)
}

/// cvr::hud::DisplayClock: the runtime's nanosecond display timeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayClock {
    pub last: i64,
    pub elapsed: f32,
}
impl DisplayClock {
    /// The seconds since the last step (0 on the first), at most 0.05; `elapsed` keeps them unclamped.
    pub fn step(&mut self, time: i64) -> f32 {
        self.elapsed = 0.0;
        if time <= 0 {
            return 0.0;
        }
        self.elapsed = if self.last != 0 && time > self.last { ((time - self.last) as f64 * 1e-9) as f32 } else { 0.0 };
        self.last = time;
        clamp(self.elapsed, 0.0, 0.05)
    }
}
/// DisplayClock::Step.
///
/// # Safety
/// `clock` is a cvr::hud::DisplayClock.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_clock_step(clock: *mut DisplayClock, time: i64) -> f32 {
    // SAFETY: the caller's clock.
    unsafe { clock.as_mut() }.map_or(0.0, |c| c.step(time))
}

/// cvr::hud::Follow: yaw only; a completed catch-up starts a fresh free zone.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Follow {
    pub yaw: f32,
    pub valid: bool,
    pub following: bool,
    pub delay_pending: bool,
    pub still_yaw: f32,
    pub still_seconds: f64,
}
const _: () = assert!(core::mem::size_of::<Follow>() == 24 && core::mem::offset_of!(Follow, still_seconds) == 16);
fn wrap(a: f32) -> f32 {
    remainder(a, 6.28318530718)
}
impl Follow {
    pub fn reset_delay(&mut self) {
        self.delay_pending = false;
        self.still_seconds = 0.0;
    }
    pub fn update(&mut self, head: f32, cone: f32, dt: f32, delayed_catchup: bool) -> f32 {
        if !head.is_finite() {
            self.reset_delay();
            return self.yaw;
        }
        if !self.valid {
            self.yaw = head;
            self.valid = true;
            self.following = false;
            self.reset_delay();
            return self.yaw;
        }
        let delta = wrap(head - self.yaw);
        if delta.abs() > cone {
            self.following = true;
        }
        // Rest is measured against a fixed heading, 0.5 deg of jitter allowed; turning restarts the 3 s wait.
        const MIN_DELAY_ANGLE: f32 = 0.1745329252;
        const REST_TOLERANCE: f32 = 0.00872664626;
        let delay_range = delayed_catchup && delta.abs() > MIN_DELAY_ANGLE && delta.abs() < cone;
        if self.following || !delay_range || !dt.is_finite() || dt <= 0.0 || dt > 0.25 {
            self.reset_delay();
        } else if !self.delay_pending || wrap(head - self.still_yaw).abs() > REST_TOLERANCE {
            self.delay_pending = true;
            self.still_yaw = head;
            self.still_seconds = 0.0;
        } else {
            self.still_seconds += dt as f64;
            if self.still_seconds >= 3.0 {
                self.following = true;
                self.reset_delay();
            }
        }
        if self.following {
            let step = 3.0 * if dt.is_finite() { clamp(dt, 0.0, 0.05) } else { 0.0 };
            self.yaw = wrap(self.yaw + clamp(delta, -step, step));
            if wrap(head - self.yaw).abs() <= 0.034906585 {
                self.following = false;
            }
        }
        self.yaw
    }
}
/// Follow::Update.
///
/// # Safety
/// `follow` is a cvr::hud::Follow.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_follow_update(follow: *mut Follow, head: f32, cone: f32, dt: f32, delayed_catchup: bool) -> f32 {
    // SAFETY: the caller's follow state.
    unsafe { follow.as_mut() }.map_or(head, |f| f.update(head, cone, dt, delayed_catchup))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_panel_holds_inside_the_cone_and_catches_up_past_it() {
        let mut f = Follow::default();
        assert_eq!(f.update(0.0, 0.5, 0.01, false), 0.0, "the first heading seeds");
        assert_eq!(f.update(0.3, 0.5, 0.01, false), 0.0, "inside the cone: held");
        let y = f.update(1.0, 0.5, 0.05, false);
        assert!(f.following && (y - 0.15).abs() < 1e-6, "past it: 3 rad/s * 0.05 s = {y}");
        // the delayed catch-up: three seconds at rest inside the cone
        let mut f = Follow::default();
        f.update(0.0, 0.5, 0.01, true);
        let steps = (0..400).position(|_| {
            f.update(0.3, 0.5, 0.01, true);
            f.following
        });
        assert!(matches!(steps, Some(299..=302)), "about 3 s still: {steps:?}");
    }
    #[test]
    fn the_clock_and_the_heading() {
        let mut c = DisplayClock::default();
        assert_eq!(c.step(1_000_000_000), 0.0);
        assert!((c.step(1_011_000_000) - 0.011).abs() < 1e-6);
        assert_eq!(c.step(2_000_000_000), 0.05, "clamped");
        assert!((c.elapsed - 0.989).abs() < 1e-5, "elapsed keeps the unclamped step");
        assert!((cvr_hud_head_yaw(0.0, (0.2f32).sin(), 0.0, (0.2f32).cos(), 9.0) - 0.4).abs() < 1e-5);
        assert_eq!(cvr_hud_head_yaw(0.0, 0.0, 0.0, 0.0, 9.0), 9.0);
    }
}
