//! THE VISIBLE BODY'S DEAD ZONES (main's include/Runtimes/BodyDeadZones.hpp, 0.2.3; off by default:
//! CyberpunkVR_BodyDeadZones). The capsule stands under the head every tick (strict roomscale); with the dead zones on,
//! the body the player sees is placed apart from it:
//!   looking down     = camera pitch < -30
//!   inner distance   = distance2d(body, camera)
//!   body to camera   = normal(camera - body)  (2D)
//!   distance         = distance2d(body + (looking down ? body to camera : body forward) * xy shift, camera)
//!   in the dead zone = upright: distance < 9 cm
//!                      looking down: (acos(camera forward 2D . body to camera) <= 60 || pitch <= -75)
//!                                    && inner distance >= 12 cm && distance < 30 cm
//!   breached: inner breached (inner distance < 12 and looking down): body += body to camera * (inner distance - 12)
//!             else: body = camera - body forward * xy shift (the neck under the camera)
//!   otherwise the body stays where it stands -- moving with the capsule while the stick moves it, which in the tracking
//!   space the head lives in is standing still too.
//! xy shift is the camera's horizontal reach ahead of the neck (the neck lever, eye_from_neck). Everything is in the
//! tracking space's axes, relative to the NECK the capsule stands under: the body's offset from the capsule. Tracking
//! metres times scale = game metres.
use crate::cmath::{acos, clamp, exp, min, sqrt};
use crate::vec2::{dot, finite, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZoneState {
    pub looking_down: bool,
    pub breached: bool,
    pub inner: bool,
    pub distance: f32,
    pub inner_distance: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BodyDeadZones {
    offset: Vec2,
    shown: Vec2,
    shown_velocity: Vec2,
    state: ZoneState,
    valid: bool,
}

impl BodyDeadZones {
    /// upright
    pub const UPPER_RADIUS: f32 = 0.09;
    /// looking down
    pub const LOWER_RADIUS: f32 = 0.30;
    /// looking down: the sector ahead
    pub const LOWER_HALF_ANGLE_DEG: f32 = 60.0;
    /// looking down: kept off the camera
    pub const INNER_RADIUS: f32 = 0.12;
    pub const LOOK_DOWN_DEG: f32 = -30.0;
    /// looking straight down: any direction counts as ahead
    pub const STRAIGHT_DOWN_DEG: f32 = -75.0;
    /// THE PLACING IS SHOWN EASED, NOT AS A JUMP: a 9 cm (looking down 30 cm) move in one frame of a body whose legs are in
    /// view reads as a teleport. The zones decide as above; what is SHOWN goes to the new place on a critically damped
    /// spring (no jump in the first frame, about 95 % in 0.3 s). 1/s; 0 shows the jump.
    pub const SHOW_OMEGA: f32 = 16.0;

    pub fn reset(&mut self) {
        *self = BodyDeadZones::default();
    }
    pub fn valid(&self) -> bool {
        self.valid
    }
    /// The body less the neck, tracking metres (the zones' decision).
    pub fn offset(&self) -> Vec2 {
        self.offset
    }
    /// The same, as shown: eased to the offset.
    pub fn shown(&self) -> Vec2 {
        self.shown
    }
    pub fn last(&self) -> ZoneState {
        self.state
    }

    /// `neck_moved`: how far the neck -- the capsule -- really moved this tick (tracking metres, what the controller
    /// accepted); the body stays in the world, so its offset from the neck shrinks by that. `eye_from_neck`: the camera
    /// relative to the neck (tracking metres, horizontal). `body_forward`: the body's forward in tracking axes (unit).
    /// `pitch_deg`: the camera's pitch, up positive. `scale`: game metres per tracking metre. `dt` (s) and `show_omega`
    /// (1/s) ease what is shown; either 0 shows the offset as it is.
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, neck_moved: Vec2, eye_from_neck: Vec2, body_forward: Vec2, pitch_deg: f32, scale: f32, dt: f32, show_omega: f32) -> Vec2 {
        if !finite(neck_moved) || !finite(eye_from_neck) || !finite(body_forward) || !pitch_deg.is_finite() || !scale.is_finite() || scale <= 0.0 {
            self.reset();
            return Vec2::default();
        }
        if !self.valid {
            // the first tick: under the neck
            self.offset = Vec2::default();
            self.shown = Vec2::default();
            self.shown_velocity = Vec2::default();
            self.valid = true;
        }
        self.offset = self.offset - neck_moved;
        // the shown body stands in the world too
        self.shown = self.shown - neck_moved;
        // game metres from here on
        let body = self.offset * scale;
        let camera = eye_from_neck * scale;
        let xy_shift = sqrt(dot(camera, camera));
        let camera_forward = if xy_shift > 1e-6 { camera * (1.0 / xy_shift) } else { body_forward };
        let mut s = ZoneState { looking_down: pitch_deg < Self::LOOK_DOWN_DEG, ..Default::default() };
        let to_camera = camera - body;
        s.inner_distance = sqrt(dot(to_camera, to_camera));
        let body_to_camera = if s.inner_distance > 1e-6 { to_camera * (1.0 / s.inner_distance) } else { Vec2::default() };
        let reach = body + (if s.looking_down { body_to_camera } else { body_forward }) * xy_shift - camera;
        s.distance = sqrt(dot(reach, reach));
        let in_zone = if s.looking_down {
            let cosine = clamp(dot(camera_forward, body_to_camera), -1.0, 1.0);
            let ahead = acos(cosine) * 57.295_78 <= Self::LOWER_HALF_ANGLE_DEG || pitch_deg <= Self::STRAIGHT_DOWN_DEG;
            ahead && s.inner_distance >= Self::INNER_RADIUS && s.distance < Self::LOWER_RADIUS
        } else {
            s.distance < Self::UPPER_RADIUS
        };
        s.breached = !in_zone;
        if s.breached {
            s.inner = s.inner_distance < Self::INNER_RADIUS && s.looking_down;
            let placed = if s.inner {
                body + body_to_camera * (s.inner_distance - Self::INNER_RADIUS)
            } else {
                camera - body_forward * xy_shift
            };
            self.offset = placed * (1.0 / scale);
        }
        self.state = s;
        self.show(dt, show_omega);
        self.offset
    }

    /// The exact critically damped response toward the offset (as BodyTranslationFollow's): no jump, no overshoot.
    fn show(&mut self, dt: f32, omega: f32) {
        if !dt.is_finite() || !omega.is_finite() || dt <= 0.0 || omega <= 0.0 {
            self.shown = self.offset;
            self.shown_velocity = Vec2::default();
            return;
        }
        let dt = min(dt, 0.05);
        let error = self.offset - self.shown;
        if dot(error, error) < 1e-10 && dot(self.shown_velocity, self.shown_velocity) < 1e-8 {
            self.shown = self.offset;
            self.shown_velocity = Vec2::default();
            return;
        }
        let decay = exp(-omega * dt);
        let c = self.shown_velocity - error * omega;
        self.shown = self.shown + (error - (error - c * dt) * decay);
        self.shown_velocity = (self.shown_velocity - c * (omega * dt)) * decay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec2::v2;

    const FORWARD: Vec2 = v2(0.0, 1.0);
    const LEVER: Vec2 = v2(0.0, 0.09);

    #[test]
    fn upright_the_body_stays_within_nine_centimetres() {
        let mut z = BodyDeadZones::default();
        assert_eq!(z.step(v2(0.0, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.0, 0.0), v2(0.0, 0.0));
        // the neck steps 5 cm right: the body stays in the world (offset -5 cm), still inside the zone
        let o = z.step(v2(0.05, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.0, 0.0);
        assert!((o.x + 0.05).abs() < 1e-6 && !z.last().breached);
        // another 5 cm: 10 cm off, breached -- placed back under the neck
        let o = z.step(v2(0.05, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.0, 0.0);
        assert!(z.last().breached && o.x.abs() < 1e-6 && o.y.abs() < 1e-6, "{o:?}");
    }

    #[test]
    fn looking_down_keeps_the_body_off_the_camera() {
        let mut z = BodyDeadZones::default();
        z.step(v2(0.0, 0.0), LEVER, FORWARD, -50.0, 1.0, 0.0, 0.0);
        // the camera's reach is 9 cm, inside the 12 cm inner radius: placed back along the body-to-camera line
        assert!(z.last().looking_down && z.last().breached && z.last().inner);
        let d = z.offset() - LEVER;
        assert!((sqrt(dot(d, d)) - BodyDeadZones::INNER_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn the_shown_body_eases_and_a_bad_input_resets() {
        let mut z = BodyDeadZones::default();
        z.step(v2(0.0, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.011, BodyDeadZones::SHOW_OMEGA);
        z.step(v2(0.2, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.011, BodyDeadZones::SHOW_OMEGA);
        // placed under the neck at once, shown on its way there
        assert!(z.offset().x.abs() < 1e-6 && z.shown().x < -0.05);
        for _ in 0..60 {
            z.step(v2(0.0, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.011, BodyDeadZones::SHOW_OMEGA);
        }
        assert!(z.shown().x.abs() < 1e-3);
        assert_eq!(z.step(v2(f32::NAN, 0.0), LEVER, FORWARD, 0.0, 1.0, 0.0, 0.0), v2(0.0, 0.0));
        assert!(!z.valid());
    }
}
