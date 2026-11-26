//! THE TWO SOURCES FADE INTO EACH OTHER (formerly include/Runtimes/HybridBodyYaw.hpp). Upright the body follows the
//! head (a cone); looking down or bent it follows the hands (TrackedBodyYaw). Both run all the time and the body takes
//! the blend of their steps (FadeIn/FadeOut); the hands' heading is aligned to the body only while the head cone is
//! settled and the head looks over the hands; the head never stays more than NeckLimit off the hands' heading (a
//! rubber band, not a new alignment); a freeze and a short loss of the estimate keep the alignment. The reasons, with
//! the simulator runs that found them, are at the C++ original's lines.
use crate::cmath::{clamp, expm1, max, min, remainder};
use crate::follow_zone::BodyFollowZone;
use crate::tracked_yaw::YawEstimate;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationMode {
    Off = 0,
    HeadCone = 1,
    Tracked = 2,
    Hybrid = 3,
    /// The head-led turn (head_led.rs), the factory default since 2026-11-07.
    HeadLed = 4,
}

/// The older INI switches: an explicit tracked-only request takes priority over the factory default. HeadLed comes
/// before the hybrid: an ini written before 2026-11-07 says xr_hybrid_body_rotation=1 and has no head-led key, and
/// one written since says the head-led key.
pub fn rotation_mode_from_flags(physical: i32, tracked: i32, hybrid: i32, head_led: i32) -> RotationMode {
    if tracked != 0 {
        RotationMode::Tracked
    } else if head_led != 0 {
        RotationMode::HeadLed
    } else if hybrid != 0 {
        RotationMode::Hybrid
    } else if physical != 0 {
        RotationMode::HeadCone
    } else {
        RotationMode::Off
    }
}

pub fn needs_body_tracking(mode: RotationMode) -> bool {
    mode == RotationMode::Tracked || mode == RotationMode::Hybrid
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HybridInput {
    pub head_yaw: f32,
    pub head_down_degrees: f32,
    pub body_bend_radians: f32,
    pub body_offset: f32,
    pub cone_degrees: f32,
    pub estimate: YawEstimate,
    pub yaw_defined: bool,
    pub estimate_ready: bool,
    pub stamp_us: u64,
    pub origin: u64,
    /// The head cone while the tracked source takes over (xr_body_free_look_down_deg).
    pub lowered_cone_degrees: f32,
}

impl Default for HybridInput {
    fn default() -> Self {
        HybridInput {
            head_yaw: 0.0,
            head_down_degrees: 0.0,
            body_bend_radians: 0.0,
            body_offset: 0.0,
            cone_degrees: 10.0,
            estimate: YawEstimate::default(),
            yaw_defined: true,
            estimate_ready: false,
            stamp_us: 0,
            origin: 0,
            lowered_cone_degrees: 30.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HybridCommand {
    pub target_yaw: f32,
    pub cone_degrees: f32,
    pub tracked: bool,
    pub tracked_weight: f32,
    pub hands_yaw: f32,
    pub neck_pull: f32,
    pub alignment: f32,
    pub estimate: bool,
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HybridBodyYaw {
    head_zone: BodyFollowZone,
    origin: u64,
    stamp: u64,
    alignment: f32,
    weight: f32,
    neck_pull: f32,
    hands_hold: f32,
    beyond_for: f32,
    lost_for: f32,
    tracked: bool,
    have_source: bool,
    aligned: bool,
    have_hold: bool,
}

fn wrap(a: f32) -> f32 {
    remainder(a, 6.28318530718)
}

impl HybridBodyYaw {
    pub const DOWN_ENTER: f32 = 10.0;
    pub const DOWN_EXIT: f32 = 8.0;
    pub const BEND_ENTER: f32 = 5.0;
    pub const BEND_EXIT: f32 = 3.0;
    pub const FADE_IN: f32 = 0.08;
    pub const FADE_OUT: f32 = 0.30;
    pub const NECK_LIMIT: f32 = 1.1344640;
    pub const NECK_GAIN: f32 = 10.0;
    pub const NECK_RELEASE: f32 = 3.0;
    pub const AGREE_WINDOW: f32 = 0.26179939;
    pub const AGREE_TAU: f32 = 1.0;
    pub const NECK_REFIT: f32 = 1.4835299;
    pub const REFIT_AFTER: f32 = 0.4;
    pub const LOST_REFIT_AFTER: f32 = 1.5;

    pub fn reset(&mut self) {
        *self = HybridBodyYaw::default();
    }

    /// keep_alignment: a freeze of our own updates (the hands' estimate runs on); otherwise the source is gone (a
    /// scene, a menu) and the hands are aligned afresh when it comes back.
    pub fn suspend(&mut self, keep_alignment: bool) {
        self.stamp = 0;
        if !keep_alignment {
            self.aligned = false;
        }
        self.beyond_for = 0.0;
        self.head_zone.reset();
    }

    pub fn tracked(&self) -> bool {
        self.tracked
    }
    pub fn tracked_weight(&self) -> f32 {
        self.weight
    }

    pub fn step(&mut self, input: &HybridInput) -> HybridCommand {
        let mut body = if input.body_offset.is_finite() { input.body_offset } else { 0.0 };
        let cone = clamp(if input.cone_degrees.is_finite() { input.cone_degrees } else { 10.0 }, 0.0, 60.0);
        let lowered =
            clamp(if input.lowered_cone_degrees.is_finite() { input.lowered_cone_degrees } else { 30.0 }, cone, 90.0);
        let mut hands_target = body;
        let mut have_estimate = false;
        let command = |s: &HybridBodyYaw, target: f32, hands_target: f32, have_estimate: bool| {
            let c = cone + (lowered - cone) * s.weight;
            HybridCommand {
                target_yaw: target,
                cone_degrees: if s.tracked { 0.0 } else { c },
                tracked: s.tracked,
                tracked_weight: s.weight,
                hands_yaw: hands_target,
                neck_pull: s.neck_pull,
                alignment: s.alignment,
                estimate: have_estimate,
            }
        };
        if input.origin == 0
            || input.stamp_us == 0
            || !input.head_down_degrees.is_finite()
            || !input.body_bend_radians.is_finite()
        {
            return command(self, body, hands_target, have_estimate);
        }
        if self.origin != 0 && input.origin < self.origin {
            return command(self, body, hands_target, have_estimate);
        }
        if self.origin != 0 && input.origin != self.origin {
            self.reset();
            body = 0.0;
        }
        self.origin = input.origin;
        if self.stamp != 0 && input.stamp_us <= self.stamp {
            return command(self, body, hands_target, have_estimate);
        }
        let dt = if self.stamp != 0 { (input.stamp_us - self.stamp) as f32 * 1e-6 } else { 0.0 };
        if dt > 0.25 {
            self.suspend(true);
        }
        self.stamp = input.stamp_us;

        let bend = input.body_bend_radians * 57.295779513;
        let mut next = self.tracked;
        if input.head_down_degrees >= Self::DOWN_ENTER || bend >= Self::BEND_ENTER {
            next = true;
        } else if input.head_down_degrees <= Self::DOWN_EXIT && bend <= Self::BEND_EXIT {
            next = false;
        }
        if !self.have_source {
            self.tracked = next;
            self.have_source = true;
            self.weight = if next { 1.0 } else { 0.0 };
            self.head_zone.reset();
        }
        self.tracked = next;
        let want = if self.tracked { 1.0 } else { 0.0 };
        if dt > 0.0 {
            self.weight += (want - self.weight)
                * -expm1(-min(dt, 0.1) / if want > self.weight { Self::FADE_IN } else { Self::FADE_OUT });
            if (want - self.weight).abs() < 0.01 {
                self.weight = want;
            }
        }

        let estimate = input.estimate_ready && input.estimate.valid && input.estimate.yaw.is_finite();
        have_estimate = estimate;
        let head = input.yaw_defined && input.head_yaw.is_finite();
        if estimate {
            let fit = wrap(body - input.estimate.yaw);
            let under_head = head && wrap(input.head_yaw - body).abs() <= cone * 0.01745329252 + 1e-4;
            self.lost_for = 0.0;
            if !self.aligned {
                self.alignment = fit;
                self.aligned = true;
                self.neck_pull = 0.0;
                self.beyond_for = 0.0;
            } else if under_head && self.weight < 1.0 && !self.head_zone.following() {
                let off = wrap(input.head_yaw - input.estimate.yaw - self.alignment).abs();
                let mut share = 0.0f32;
                if off <= Self::AGREE_WINDOW {
                    self.beyond_for = 0.0;
                    share = if dt > 0.0 { -expm1(-min(dt, 0.1) / Self::AGREE_TAU) } else { 0.0 };
                } else if off > Self::NECK_REFIT {
                    self.beyond_for += max(dt, 0.0);
                    share = if self.beyond_for >= Self::REFIT_AFTER { 1.0 } else { 0.0 };
                } else {
                    self.beyond_for = 0.0;
                }
                share *= 1.0 - self.weight;
                self.alignment = wrap(self.alignment + wrap(fit - self.alignment) * share);
                self.neck_pull *= 1.0 - share;
            } else {
                self.beyond_for = 0.0;
            }
        } else {
            self.beyond_for = 0.0;
            self.lost_for += max(dt, 0.0);
            if self.lost_for > Self::LOST_REFIT_AFTER {
                self.aligned = false;
            }
        }
        if estimate {
            hands_target = wrap(input.estimate.yaw + self.alignment);
            self.hands_hold = hands_target;
            self.have_hold = true;
        } else {
            if !self.have_hold {
                self.hands_hold = body;
                self.have_hold = true;
            }
            hands_target = self.hands_hold;
        }
        if head && dt > 0.0 {
            let gap = wrap(input.head_yaw - hands_target);
            let needed = max(0.0, gap.abs() - Self::NECK_LIMIT).copysign(gap);
            let grow = needed * self.neck_pull < 0.0 || needed.abs() > self.neck_pull.abs();
            let change = (needed - self.neck_pull)
                * -expm1(-min(dt, 0.1) * if grow { Self::NECK_GAIN } else { Self::NECK_RELEASE });
            let most = 12.566371 * dt;
            self.neck_pull += clamp(change, -most, most);
            if self.neck_pull.abs() < 1e-5 && needed == 0.0 {
                self.neck_pull = 0.0;
            }
        } else if !head {
            self.neck_pull = 0.0;
        }
        let hand_step = wrap(hands_target + self.neck_pull - body);

        let mut cone_step = 0.0f32;
        if head && self.weight < 1.0 {
            cone_step = self.head_zone.step(
                wrap(input.head_yaw - body),
                (cone + (lowered - cone) * self.weight) * 0.01745329252,
                dt,
            );
        } else {
            self.head_zone.reset();
        }
        let mut step = (1.0 - self.weight) * cone_step + self.weight * hand_step;
        let limit = 12.566371 * max(0.0, dt);
        step = clamp(step, -limit, limit);
        command(self, wrap(body + step), hands_target, have_estimate)
    }
}
