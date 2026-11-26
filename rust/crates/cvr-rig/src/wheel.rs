//! Steering with the hands, operation for operation as include/Anim/WheelRange.hpp, WheelSteering.hpp,
//! WheelPrediction.hpp and TrackedWheelSteering.hpp: the full lock a hand travel maps to, a car's cabin-wheel curve,
//! the signed angle integrated from the hands' vector, the conservative feed-forward lead, and the wheel held through
//! tracking gaps.
use cvr_body::cmath::{atan2, clamp, cos, hypot, min, sin, sqrt};
use cvr_body::xr::{rotate_vector, Quat, Vec3};

pub use cvr_settings::wheel_range::{
    bike_bar_lock_degrees, bike_body_lean_share, bike_lean_degrees, wheel_full_lock_degrees, BIKE_BAR_LOCK_DEFAULT,
    BIKE_BODY_LEAN_DEFAULT, WHEEL_FULL_LOCK_DEFAULT, WHEEL_FULL_LOCK_MAX, WHEEL_FULL_LOCK_MIN,
};

pub const CAR_CABIN_GAIN: f32 = 2.5;
pub const CAR_CABIN_LINEAR_DEG: f32 = 89.9 * CAR_CABIN_GAIN / 2.0;
pub const CAR_CABIN_CUBIC_DEG: f32 = 40.1 * CAR_CABIN_GAIN / 2.0;
pub const CAR_CABIN_WHEEL_LOCK_DEG: f32 = CAR_CABIN_LINEAR_DEG + CAR_CABIN_CUBIC_DEG;

/// The cabin wheel's angle for a post-deadzone steering input n.
pub fn car_cabin_angle_for_input(n: f32) -> f32 {
    if !n.is_finite() {
        return 0.0;
    }
    let a = clamp(n.abs(), 0.0, 1.0);
    (CAR_CABIN_LINEAR_DEG * a + CAR_CABIN_CUBIC_DEG * a * a * a).copysign(n)
}

/// The input that stands the cabin wheel at `degrees`: Newton from the linear guess, four steps.
pub fn car_input_for_cabin_angle(degrees: f32) -> f32 {
    if !degrees.is_finite() {
        return 0.0;
    }
    let target = min(degrees.abs(), CAR_CABIN_WHEEL_LOCK_DEG);
    let mut n = target / CAR_CABIN_WHEEL_LOCK_DEG;
    for _ in 0..4 {
        let error = CAR_CABIN_LINEAR_DEG * n + CAR_CABIN_CUBIC_DEG * n * n * n - target;
        n -= error / (CAR_CABIN_LINEAR_DEG + 3.0 * CAR_CABIN_CUBIC_DEG * n * n);
    }
    clamp(n, 0.0, 1.0).copysign(degrees)
}

/// A car's wheel turns with the hands one to one; bikes and other mounts keep the setting (30..120, else 90).
pub fn wheel_full_lock_for_vehicle(value: f32, car: bool) -> f32 {
    if car {
        CAR_CABIN_WHEEL_LOCK_DEG
    } else if value.is_finite() && value >= 30.0 && value <= 120.0 {
        value
    } else {
        90.0
    }
}

/// The desired post-deadzone axis encoded in the game's inner/outer stick range.
pub fn wheel_gamepad_axis(value: f32, inner: f32, outer: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let (mut inner, mut outer) = (inner, outer);
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || inner >= 0.5 || outer > 1.0 || outer <= inner + 0.01 {
        inner = 0.35;
        outer = 0.9;
    }
    let magnitude = clamp(value.abs(), 0.0, 1.0);
    if magnitude > 0.0 {
        (inner + (outer - inner) * magnitude).copysign(value)
    } else {
        0.0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelPoint {
    pub x: f32,
    pub y: f32,
}

const DEGREES: f32 = 57.295_779_513;
const PI: f32 = 3.141_592_65;

fn finite_point(p: WheelPoint) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn sub(a: WheelPoint, b: WheelPoint) -> WheelPoint {
    WheelPoint { x: a.x - b.x, y: a.y - b.y }
}
fn length(p: WheelPoint) -> f32 {
    hypot(p.x, p.y)
}

/// Calibrate at each grab/hand change, then integrate signed angle changes (WheelSteering).
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelSteering {
    held: u8,
    valid: bool,
    angle: f32,
    pivot: WheelPoint,
    previous: WheelPoint,
}

impl WheelSteering {
    pub const fn new() -> Self {
        WheelSteering { held: 0, valid: false, angle: 0.0, pivot: WheelPoint { x: 0.0, y: 0.0 }, previous: WheelPoint { x: 0.0, y: 0.0 } }
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn angle(&self) -> f32 {
        self.angle
    }
    /// `rim_right` / `rim_left`: where on the rim a hand has hold, in radians from 3 o'clock anticlockwise, or a NaN for
    /// the old assumption -- a hand where the driving animation has it: 3 and 9 o'clock, turned with the wheel. With the
    /// hand at the top, a hub assumed beside it read a sideways pull as hardly any turn.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, held: u8, right: WheelPoint, left: WheelPoint, radius: f32, max_degrees: f32,
                  dead_degrees: f32, rim_right: f32, rim_left: f32) -> f32 {
        let held = held & 3;
        if held == 0 || ((held & 1) != 0 && !finite_point(right)) || ((held & 2) != 0 && !finite_point(left)) {
            self.reset();
            return 0.0;
        }
        let radius = if radius.is_finite() { clamp(radius, 0.08, 0.45) } else { 0.19 };
        let limit = wheel_full_lock_degrees(max_degrees);
        let rebase = held != self.held || !self.valid;
        if rebase {
            self.held = held;
            if held != 3 {
                let hand = if held == 1 { right } else { left };
                let rim = if held == 1 { rim_right } else { rim_left };
                let angle = if rim.is_finite() { rim } else { (if held == 1 { 0.0 } else { PI }) - self.angle / DEGREES };
                self.pivot = WheelPoint { x: hand.x - radius * cos(angle), y: hand.y - radius * sin(angle) };
            }
        }
        let v = if held == 3 { sub(right, left) } else { sub(if held == 1 { right } else { left }, self.pivot) };
        let len = length(v);
        let minimum = if held == 3 { 0.08 } else { 0.04 };
        if len < minimum || !len.is_finite() {
            self.valid = false;
            return Self::output(self.angle, limit, dead_degrees);
        }
        let previous_length = length(self.previous);
        let cross = self.previous.x * v.y - self.previous.y * v.x;
        let dot = self.previous.x * v.x + self.previous.y * v.y;
        let delta = -atan2(cross, dot) * DEGREES;
        if !rebase && previous_length > 0.0 && len < previous_length * 2.0 && previous_length < len * 2.0 && delta.abs() < 150.0 {
            self.angle += delta;
        }
        self.previous = v;
        self.valid = true;
        Self::output(self.angle, limit, dead_degrees)
    }
    pub fn output(angle: f32, max_degrees: f32, dead_degrees: f32) -> f32 {
        let limit = wheel_full_lock_degrees(max_degrees);
        let mut dead = if dead_degrees.is_finite() && dead_degrees >= 0.0 && dead_degrees <= 20.0 { dead_degrees } else { 1.5 };
        dead = min(dead, limit - 5.0);
        let n = clamp((angle.abs() - dead) / (limit - dead), 0.0, 1.0);
        if n <= 0.0 {
            return 0.0;
        }
        n.copysign(angle)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PredictionPoint {
    angle: f32,
    stamp: u64,
}

/// A small feed-forward term from four observations of a continuing turn (WheelPrediction).
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelPrediction {
    points: [PredictionPoint; 4],
    count: usize,
    velocity: f32,
}

impl WheelPrediction {
    pub const MAX_MS: f32 = 8.0;
    pub const MAX_LEAD_DEGREES: f32 = 0.5;
    pub const fn new() -> Self {
        WheelPrediction { points: [PredictionPoint { angle: 0.0, stamp: 0 }; 4], count: 0, velocity: 0.0 }
    }
    pub fn clamp_ms(value: f32) -> f32 {
        if value.is_finite() {
            clamp(value, 0.0, Self::MAX_MS)
        } else {
            0.0
        }
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn push(&mut self, angle: f32, stamp: u64) {
        if !angle.is_finite() || stamp == 0 {
            self.reset();
            return;
        }
        if self.count != 0 {
            let last = self.points[self.count - 1].stamp;
            if stamp <= last || stamp - last > 75000 {
                self.reset();
            }
        }
        if self.count != 0 && stamp - self.points[self.count - 1].stamp < 1000 {
            self.points[self.count - 1] = PredictionPoint { angle, stamp };
            self.velocity = 0.0;
            return;
        }
        if self.count == self.points.len() {
            for i in 1..self.count {
                self.points[i - 1] = self.points[i];
            }
            self.count -= 1;
        }
        self.points[self.count] = PredictionPoint { angle, stamp };
        self.count += 1;
        self.velocity = 0.0;
        if self.count < 4 {
            return;
        }
        let mut speed = 720.0f32;
        let mut direction = 0i32;
        for i in 1..self.count {
            let delta = self.points[i].angle - self.points[i - 1].angle;
            let dt = (self.points[i].stamp - self.points[i - 1].stamp) as f32 * 1e-6;
            if delta.abs() < 0.002 || dt <= 0.0 {
                return;
            }
            let v = delta / dt;
            if v.abs() < 3.0 || v.abs() > 720.0 {
                return;
            }
            let sign = if v > 0.0 { 1 } else { -1 };
            if direction != 0 && sign != direction {
                return;
            }
            direction = sign;
            speed = min(speed, v.abs());
        }
        self.velocity = direction as f32 * speed;
    }
    pub fn angle(&self, physical: f32, now: u64, horizon_ms: f32, limit: f32, dead: f32) -> f32 {
        let ms = Self::clamp_ms(horizon_ms);
        if ms == 0.0 || self.velocity == 0.0 || self.count < 4 || !physical.is_finite() {
            return physical;
        }
        let stamp = self.points[self.count - 1].stamp;
        let interval = stamp.wrapping_sub(self.points[self.count - 2].stamp);
        let lease = interval.wrapping_mul(2).clamp(25000, 60000);
        if now < stamp || now - stamp > lease {
            return physical;
        }
        let limit = wheel_full_lock_degrees(limit);
        let dead = if dead.is_finite() && dead >= 0.0 && dead <= 20.0 { dead } else { 1.5 };
        let envelope = clamp(min(physical.abs() - dead, limit - physical.abs()) / 2.0, 0.0, 1.0);
        let lead = clamp(self.velocity * ms * 0.001, -Self::MAX_LEAD_DEGREES, Self::MAX_LEAD_DEGREES) * envelope;
        physical + lead
    }
}

/// One tracking sample for the wheel (TrackedWheelSteering.hpp's WheelTrackingFrame, the same layout): the head pose,
/// both hands head-relative (right, then left), its sequence, recenter origin and stamp.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelTrackingFrame {
    pub head_orientation: Quat,
    pub head_position: Vec3,
    pub hands: [Vec3; 2],
    pub sequence: u64,
    pub origin: u64,
    pub stamp_us: u64,
    pub valid_hands: u8,
    pub head_valid: bool,
}

impl Default for WheelTrackingFrame {
    fn default() -> Self {
        WheelTrackingFrame {
            head_orientation: Quat::IDENTITY,
            head_position: Vec3::default(),
            hands: [Vec3::default(); 2],
            sequence: 0,
            origin: 0,
            stamp_us: 0,
            valid_hands: 0,
            head_valid: false,
        }
    }
}

fn finite3(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// THE WHEEL THE HANDS HOLD, through gaps (TrackedWheelSteering).
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackedWheelSteering {
    wheel: WheelSteering,
    prediction: WheelPrediction,
    horizon: f32,
    last: f32,
    sequence: u64,
    origin: u64,
    stamp: u64,
    fresh: u64,
    held: u8,
    lean_degrees: f32,
    lean_neck: f32,
    lean_valid: bool,
}

/// Estimated, from between the eyes.
const NECK_BELOW_M: f32 = 0.10;
const NECK_BEHIND_M: f32 = 0.10;

impl TrackedWheelSteering {
    pub const STALE_US: u64 = 250_000;
    pub const HOLD_US: u64 = 2_000_000;
    pub const fn new_const() -> Self {
        TrackedWheelSteering {
            wheel: WheelSteering::new(),
            prediction: WheelPrediction::new(),
            horizon: 0.0,
            last: 0.0,
            sequence: 0,
            origin: 0,
            stamp: 0,
            fresh: 0,
            held: 0,
            lean_degrees: 0.0,
            lean_neck: 0.0,
            lean_valid: false,
        }
    }
    /// The body's lean into a bike's turn, as degrees of steering.
    pub fn lean(&self) -> f32 {
        self.lean_degrees
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn angle(&self) -> f32 {
        self.wheel.angle()
    }
    pub fn suspend(&mut self, now: u64) -> f32 {
        self.prediction.reset();
        if self.fresh != 0 && now >= self.fresh && now - self.fresh <= Self::HOLD_US {
            self.last
        } else {
            0.0
        }
    }
    /// `rim_right` / `rim_left` as WheelSteering::update's (NaN: the animation's grip); `handlebar` a motorcycle's bars,
    /// `body_lean` the share of full lock a sideways lean adds (xr_bike_body_lean).
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, held: u8, frame: &WheelTrackingFrame, now: u64, radius: f32, limit: f32, dead: f32,
                  prediction_ms: f32, rim_right: f32, rim_left: f32, handlebar: bool, body_lean: f32) -> f32 {
        let held = held & 3;
        if held == 0 {
            self.reset();
            return 0.0;
        }
        let tracked = held & frame.valid_hands;
        let q = frame.head_orientation;
        let norm = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
        if tracked == 0 || !frame.head_valid || frame.origin == 0 || frame.sequence == 0 || frame.stamp_us == 0
            || now < frame.stamp_us || now - frame.stamp_us > Self::STALE_US || !finite3(frame.head_position)
            || !norm.is_finite() || norm < 0.5 || norm > 1.5
        {
            return self.suspend(now);
        }
        if self.origin != 0 && self.origin != frame.origin {
            self.reset();
        } else if self.sequence != frame.sequence && self.stamp != 0 && frame.stamp_us <= self.stamp {
            return self.suspend(now);
        }
        self.origin = frame.origin;
        let horizon = WheelPrediction::clamp_ms(prediction_ms);
        if self.held != tracked || horizon != self.horizon {
            self.prediction.reset();
            self.horizon = horizon;
        }
        if self.sequence == frame.sequence && self.held == tracked {
            let out = self.output(now, limit, dead);
            return self.keep(now, out);
        }
        let scale = 1.0 / sqrt(norm);
        let rotation = Quat { x: q.x * scale, y: q.y * scale, z: q.z * scale, w: q.w * scale };
        let mut points = [WheelPoint::default(); 2];
        for (i, point) in points.iter_mut().enumerate() {
            if tracked & (1 << i) != 0 {
                let local = frame.hands[i];
                if !finite3(local) || local.x * local.x + local.y * local.y + local.z * local.z > 4.0 {
                    return self.suspend(now);
                }
                let world = rotate_vector(rotation, local);
                // A motorcycle's bars turn about the vertical, so the same turn is measured seen from above:
                // right/forward (OpenXR -Z is forward). Right hand pushed forward, left pulled back is a turn left.
                *point = WheelPoint {
                    x: frame.head_position.x + world.x,
                    y: if handlebar { -(frame.head_position.z + world.z) } else { frame.head_position.y + world.y },
                };
            }
        }
        self.sequence = frame.sequence;
        self.stamp = frame.stamp_us;
        self.held = tracked;
        // THE BODY LEANING INTO A BIKE'S TURN, measured from where it was at the grab. At the neck and not the headset:
        // the eyes swing round it, and looking 60 degrees aside moved the headset about 9 cm sideways.
        self.lean_degrees = 0.0;
        if handlebar {
            let neck = frame.head_position.x + rotate_vector(rotation, Vec3 { x: 0.0, y: -NECK_BELOW_M, z: NECK_BEHIND_M }).x;
            if !self.lean_valid {
                self.lean_neck = neck;
                self.lean_valid = true;
            }
            self.lean_degrees = bike_lean_degrees(neck - self.lean_neck, body_lean, wheel_full_lock_degrees(limit));
        }
        self.wheel.update(tracked, points[0], points[1], radius, limit, dead, rim_right, rim_left);
        if self.horizon == 0.0 {
            let out = WheelSteering::output(self.angle() + self.lean_degrees, limit, dead);
            return self.keep(now, out);
        }
        self.prediction.push(self.angle(), frame.stamp_us);
        let out = self.output(now, limit, dead);
        self.keep(now, out)
    }
    fn keep(&mut self, now: u64, out: f32) -> f32 {
        self.fresh = now;
        self.last = out;
        out
    }
    fn output(&self, now: u64, limit: f32, dead: f32) -> f32 {
        WheelSteering::output(self.prediction.angle(self.angle(), now, self.horizon, limit, dead) + self.lean_degrees, limit, dead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_layout_is_the_cpp_one() {
        assert_eq!(core::mem::size_of::<WheelTrackingFrame>(), 88);
        assert_eq!(core::mem::offset_of!(WheelTrackingFrame, hands), 28);
        assert_eq!(core::mem::offset_of!(WheelTrackingFrame, sequence), 56);
        assert_eq!(core::mem::offset_of!(WheelTrackingFrame, valid_hands), 80);
    }

    #[test]
    fn cabin_curve_inverts() {
        for i in 0..=20 {
            let n = i as f32 / 20.0;
            let back = car_input_for_cabin_angle(car_cabin_angle_for_input(n));
            assert!((back - n).abs() < 1e-4, "{n} -> {back}");
        }
        assert_eq!(car_input_for_cabin_angle(f32::NAN), 0.0);
    }

    #[test]
    fn two_hands_turn_the_wheel() {
        let mut w = WheelSteering::default();
        let r0 = WheelPoint { x: 0.19, y: 0.0 };
        let l0 = WheelPoint { x: -0.19, y: 0.0 };
        assert_eq!(w.update(3, r0, l0, 0.19, 90.0, 0.0, f32::NAN, f32::NAN), 0.0);
        let a = 0.5f32;
        let r1 = WheelPoint { x: 0.19 * a.cos(), y: -0.19 * a.sin() };
        let l1 = WheelPoint { x: -0.19 * a.cos(), y: 0.19 * a.sin() };
        let out = w.update(3, r1, l1, 0.19, 90.0, 0.0, f32::NAN, f32::NAN);
        assert!((w.angle() - a.to_degrees()).abs() < 1e-3, "{}", w.angle());
        assert!(out > 0.3 && out < 0.33, "{out}");
    }

    #[test]
    fn one_hand_at_the_top_of_the_rim() {
        // The right hand at 12 o'clock pulled right: a hub assumed beside it (3 o'clock) read hardly any turn.
        let mut w = WheelSteering::default();
        let top = WheelPoint { x: 0.0, y: 0.19 };
        w.update(1, top, WheelPoint::default(), 0.19, 90.0, 0.0, core::f32::consts::FRAC_PI_2, f32::NAN);
        let a = 0.3f32;
        let moved = WheelPoint { x: 0.19 * a.sin(), y: 0.19 * a.cos() };
        w.update(1, moved, WheelPoint::default(), 0.19, 90.0, 0.0, core::f32::consts::FRAC_PI_2, f32::NAN);
        assert!((w.angle() - a.to_degrees()).abs() < 1e-2, "{}", w.angle());
    }

    #[test]
    fn gamepad_axis_encodes_the_range() {
        assert_eq!(wheel_gamepad_axis(0.0, 0.35, 0.9), 0.0);
        assert!((wheel_gamepad_axis(1.0, 0.35, 0.9) - 0.9).abs() < 1e-6);
        assert!((wheel_gamepad_axis(-0.5, 0.6, 0.9) + 0.625).abs() < 1e-6, "an invalid profile falls back");
    }
}
