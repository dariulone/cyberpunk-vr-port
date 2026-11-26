//! THE HEAD-LED BODY TURN (main's include/Runtimes/HeadLedBodyTurn.hpp, the factory default since 2026-11-07). The head
//! alone decides the body's yaw; the controllers take no part. The turn is VRExpansionPlugin's
//! UParentRelativeAttachmentComponent::GetCalculatedRotation (open source, ported line for line with FMath::FixedTurn),
//! with a separate tolerance and speed while the head is pitched below a look-down angle:
//!   - within the tolerance of where the body last re-aimed, the body stays (or finishes a turn already under way);
//!   - past it, the body turns at the speed toward the head of that frame.
//!
//! Upright the default is a 10 degree cone; looking down (more than 30 degrees) the body stays until the head has turned
//! 70 degrees, then turns to where the head was then. TIGHT is the same turn with a 1 degree upright tolerance at 150
//! deg/s (the reference the tests hold the port to), FOLLOW_HEAD the 1/1 at 500 deg/s one.
use crate::cmath::fmod;

/// std::fabs (exact).
fn fabs(x: f32) -> f32 {
    x.abs()
}
/// std::fmin: the number when the other is a NaN, as f32::min.
fn fmin(a: f32, b: f32) -> f32 {
    a.min(b)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyTurnConfig {
    pub upper_tolerance_deg: f32,
    pub lower_tolerance_deg: f32,
    pub upper_speed_dps: f32,
    pub lower_speed_dps: f32,
    /// Upright, a turn under way follows the head through to it. GetCalculatedRotation re-aims only on the frame the
    /// head leaves the tolerance, at the head of THAT frame, and a head that turns under the tolerance a frame does so
    /// every so often: with 1 degree the body ends within a degree, with a 10 degree cone it stopped up to 10 degrees
    /// short, wherever the last re-aim fell (in game: 6.6 of a 30 degree turn). Following through, a turn begun by the
    /// head leaving the cone goes on after the head until the body is under it (SETTLE_DEG) and the head has stopped
    /// (under SETTLE_DPS) -- only then is the cone centred there again.
    pub follow_through_upright: bool,
}

pub const TIGHT_BODY_TURN_CONFIG: BodyTurnConfig =
    BodyTurnConfig { upper_tolerance_deg: 1.0, lower_tolerance_deg: 70.0, upper_speed_dps: 150.0, lower_speed_dps: 150.0, follow_through_upright: false };
/// The default: a 10 degree upright cone, 70 looking down, 300 deg/s both ways, turns followed through.
pub const DEFAULT_BODY_TURN_CONFIG: BodyTurnConfig =
    BodyTurnConfig { upper_tolerance_deg: 10.0, lower_tolerance_deg: 70.0, upper_speed_dps: 300.0, lower_speed_dps: 300.0, follow_through_upright: true };
pub const FOLLOW_HEAD_TURN_CONFIG: BodyTurnConfig =
    BodyTurnConfig { upper_tolerance_deg: 1.0, lower_tolerance_deg: 1.0, upper_speed_dps: 500.0, lower_speed_dps: 500.0, follow_through_upright: false };
pub const SETTLE_DEG: f32 = 0.5;
pub const SETTLE_DPS: f32 = 10.0;
/// Camera pitch, degrees, up positive.
pub const LOOK_DOWN_ANGLE_DEG: f32 = -30.0;

/// FRotator::ClampAxis: [0, 360).
pub fn turn_clamp_axis(a: f32) -> f32 {
    let mut a = fmod(a, 360.0);
    if a < 0.0 {
        a += 360.0;
    }
    a
}
/// FMath::FindDeltaAngleDegrees: the shortest signed angle from a1 to a2.
pub fn turn_delta_angle(a1: f32, a2: f32) -> f32 {
    let mut d = a2 - a1;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}
/// FMath::FixedTurn.
pub fn turn_fixed(current: f32, desired: f32, delta_rate: f32) -> f32 {
    if delta_rate == 0.0 {
        return turn_clamp_axis(current);
    }
    if delta_rate >= 360.0 {
        return turn_clamp_axis(desired);
    }
    let mut result = turn_clamp_axis(current);
    let current = result;
    let desired = turn_clamp_axis(desired);
    if current > desired {
        if current - desired < 180.0 {
            result -= fmin(current - desired, fabs(delta_rate));
        } else {
            result += fmin(desired + 360.0 - current, fabs(delta_rate));
        }
    } else if desired - current < 180.0 {
        result += fmin(desired - current, fabs(delta_rate));
    } else {
        result -= fmin(current + 360.0 - desired, fabs(delta_rate));
    }
    turn_clamp_axis(result)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadLedBodyTurn {
    last_rot: f32,
    last_lerp_val: f32,
    lerp_target: f32,
    output: f32,
    was_set_once: bool,
    seeded: bool,
    turning: bool,
    have_prev_head: bool,
    prev_head: f32,
}

impl HeadLedBodyTurn {
    /// ResetRotValues: the next update starts from the body as it stands (after a snap or stick turn, a reset).
    pub fn reset(&mut self) {
        self.was_set_once = false;
        self.seeded = false;
        self.turning = false;
        self.have_prev_head = false;
    }
    /// Starts from the body's current yaw (degrees) instead of VRExpansion's zero: the port's body already has a heading.
    pub fn seed(&mut self, body_yaw_deg: f32) {
        let a = turn_clamp_axis(body_yaw_deg);
        self.last_rot = a;
        self.last_lerp_val = a;
        self.lerp_target = a;
        self.was_set_once = true;
        self.seeded = true;
        self.turning = false;
        self.have_prev_head = false;
    }
    pub fn seeded(&self) -> bool {
        self.seeded
    }
    pub fn body_yaw_deg(&self) -> f32 {
        self.last_lerp_val
    }
    pub fn output(&self) -> f32 {
        self.output
    }

    /// head_yaw_deg: the head's yaw relative to the character (degrees); head_pitch_deg: up positive; dt seconds.
    /// Returns the body's yaw (degrees, [0, 360)): GetCalculatedRotation with the tolerance and speed of the pitch.
    pub fn step(&mut self, head_yaw_deg: f32, head_pitch_deg: f32, dt: f32, config: &BodyTurnConfig, look_down_angle_deg: f32) -> f32 {
        let looking_down = head_pitch_deg < look_down_angle_deg;
        let yaw_tolerance = if looking_down { config.lower_tolerance_deg } else { config.upper_tolerance_deg };
        let lerp_speed = if looking_down { config.lower_speed_dps } else { config.upper_speed_dps };
        let lerp_transition = lerp_speed != 0.0;
        let head_rate =
            if self.have_prev_head && dt > 0.0 { fabs(turn_delta_angle(self.prev_head, head_yaw_deg)) / dt } else { 0.0 };
        self.prev_head = head_yaw_deg;
        self.have_prev_head = true;
        let follow_through = config.follow_through_upright && !looking_down;
        if !follow_through {
            self.turning = false;
        }
        if self.turning {
            // the follow-through (BodyTurnConfig::follow_through_upright)
            self.lerp_target = turn_clamp_axis(head_yaw_deg);
            self.last_rot = self.lerp_target;
            self.last_lerp_val = turn_fixed(self.last_lerp_val, self.lerp_target, lerp_speed * dt);
            if fabs(turn_delta_angle(self.last_lerp_val, self.lerp_target)) < SETTLE_DEG && head_rate < SETTLE_DPS {
                self.last_lerp_val = self.lerp_target;
                self.turning = false;
            }
            self.output = self.last_lerp_val;
            return self.output;
        }
        let final_rot;
        if fabs(turn_delta_angle(head_yaw_deg, self.last_rot)) < yaw_tolerance {
            if !self.was_set_once {
                self.last_rot = turn_clamp_axis(head_yaw_deg);
                self.last_lerp_val = self.last_rot;
                self.lerp_target = self.last_rot;
                self.was_set_once = true;
            }
            if lerp_transition && fabs(self.last_lerp_val - self.lerp_target) > 1e-8 {
                self.last_lerp_val = turn_fixed(self.last_lerp_val, self.lerp_target, lerp_speed * dt);
                final_rot = self.last_lerp_val;
            } else {
                final_rot = self.last_rot;
                self.last_lerp_val = self.last_rot;
            }
        } else {
            if yaw_tolerance > 1e-8 {
                if follow_through {
                    self.turning = true;
                }
                self.lerp_target = turn_clamp_axis(head_yaw_deg);
                self.last_lerp_val = turn_fixed(self.last_lerp_val, self.lerp_target, lerp_speed * dt);
                final_rot = self.last_lerp_val;
            } else {
                final_rot = turn_clamp_axis(head_yaw_deg);
            }
            self.last_rot = turn_clamp_axis(head_yaw_deg);
        }
        // (GetCalculatedRotation returns final_rot; with a zero tolerance it does not keep it in last_lerp_val)
        self.output = final_rot;
        final_rot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_cone_holds_then_follows_through() {
        let mut t = HeadLedBodyTurn::default();
        t.seed(0.0);
        // within the 10 degree cone the body stays
        let mut out = 0.0;
        for _ in 0..30 {
            out = t.step(8.0, 0.0, 1.0 / 90.0, &DEFAULT_BODY_TURN_CONFIG, LOOK_DOWN_ANGLE_DEG);
        }
        assert_eq!(out, 0.0);
        // past it the body turns at 300 deg/s and follows through to the head
        for _ in 0..90 {
            out = t.step(40.0, 0.0, 1.0 / 90.0, &DEFAULT_BODY_TURN_CONFIG, LOOK_DOWN_ANGLE_DEG);
        }
        assert!((out - 40.0).abs() < 1e-3, "{out}");
    }

    #[test]
    fn looking_down_needs_seventy_degrees() {
        let mut t = HeadLedBodyTurn::default();
        t.seed(0.0);
        let mut out = 0.0;
        for _ in 0..60 {
            out = t.step(60.0, -45.0, 1.0 / 90.0, &DEFAULT_BODY_TURN_CONFIG, LOOK_DOWN_ANGLE_DEG);
        }
        assert_eq!(out, 0.0);
        for _ in 0..90 {
            out = t.step(80.0, -45.0, 1.0 / 90.0, &DEFAULT_BODY_TURN_CONFIG, LOOK_DOWN_ANGLE_DEG);
        }
        assert!((out - 80.0).abs() < 1e-3, "{out}");
    }

    #[test]
    fn fixed_turn_takes_the_short_way() {
        assert!((turn_fixed(350.0, 10.0, 5.0) - 355.0).abs() < 1e-4);
        assert!((turn_fixed(10.0, 350.0, 5.0) - 5.0).abs() < 1e-4);
        assert_eq!(turn_fixed(10.0, 350.0, 400.0), 350.0);
        assert_eq!(turn_delta_angle(350.0, 10.0), 20.0);
    }
}
