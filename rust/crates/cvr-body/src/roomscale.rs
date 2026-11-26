//! Roomscale accounting (formerly include/Runtimes/RoomscaleMovement.hpp), independent of OpenXR and of engine
//! pointers. Only planar displacement goes to physics; camera height, gravity and jump never enter it. Units are
//! tracking-space metres. The reasons for every rule (live traces and simulator runs) are at the C++ original's lines:
//! a follow starts from rest at the free-zone boundary, ends under the head and then stands still (or recentres a head
//! held still upright), a walk is one follow; the strict mode asks the whole lead every tick; the walk the rest of the
//! game sees is gated; a tracking jump is rebased, never walked; a wall's refusal stays the view's lead.
use crate::bend::{EYE_ABOVE_NECK, EYE_AHEAD_OF_NECK};
use crate::cmath::{atan2, clamp, exp, max, min, remainder, sqrt};
use crate::vec2::{dot, finite, rotate, v2, Vec2};

pub fn tracking_yaw(entity_yaw: f32, body_realign: f32) -> f32 {
    remainder(entity_yaw - body_realign, 6.28318530718)
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyTranslationFollow {
    velocity: Vec2,
    omega: f32,
    still_at: Vec2,
    still_for: f32,
    still_known: bool,
    following: bool,
}

impl Default for BodyTranslationFollow {
    fn default() -> Self {
        BodyTranslationFollow {
            velocity: Vec2::default(),
            omega: Self::FOLLOW_OMEGA,
            still_at: Vec2::default(),
            still_for: 0.0,
            still_known: false,
            following: false,
        }
    }
}

impl BodyTranslationFollow {
    pub const SETTLE: f32 = 0.004;
    pub const SETTLE_SPEED: f32 = 0.03;
    pub const RECENTRE: f32 = 0.03;
    pub const STILL_RADIUS: f32 = 0.015;
    pub const STILL_TIME: f32 = 1.0;
    pub const FOLLOW_OMEGA: f32 = 20.0;
    pub const RECENTRE_OMEGA: f32 = 12.0;

    pub fn reset(&mut self) {
        self.following = false;
        self.velocity = Vec2::default();
    }
    pub fn following(&self) -> bool {
        self.following
    }
    /// Where the neck is this tick (the hip hinge put back), for the standing recentre.
    pub fn head_motion(&mut self, neck: Vec2, upright: bool, dt: f32) {
        if !upright || !finite(neck) || !dt.is_finite() || dt <= 0.0 {
            self.still_for = 0.0;
            self.still_known = false;
            return;
        }
        let moved = neck - self.still_at;
        if !self.still_known || dot(moved, moved) > Self::STILL_RADIUS * Self::STILL_RADIUS {
            self.still_at = neck;
            self.still_known = true;
            self.still_for = 0.0;
            return;
        }
        self.still_for += dt;
    }
    pub fn head_still(&self) -> bool {
        self.still_for >= Self::STILL_TIME
    }
    /// The controller refused part of the last step: along it the body is at rest against the obstacle.
    pub fn block(&mut self, refused: Vec2) {
        let r2 = dot(refused, refused);
        if r2 < 1e-14 {
            return;
        }
        let along = dot(self.velocity, refused) / r2;
        if along > 0.0 {
            self.velocity = self.velocity - refused * along;
        }
    }
    pub fn step(&mut self, error: Vec2, radius: f32, dt: f32) -> Vec2 {
        if !finite(error) || !radius.is_finite() || !dt.is_finite() || dt <= 0.0 {
            self.reset();
            return Vec2::default();
        }
        if radius <= 0.0 {
            self.reset();
            return error;
        }
        let distance2 = dot(error, error);
        if !self.following {
            if distance2 > radius * radius {
                self.omega = Self::FOLLOW_OMEGA;
            } else if self.head_still() && distance2 > Self::RECENTRE * Self::RECENTRE {
                self.omega = Self::RECENTRE_OMEGA;
            } else {
                return Vec2::default();
            }
        } else if distance2 > radius * radius {
            self.omega = Self::FOLLOW_OMEGA;
        }
        self.following = true;
        if distance2 < 1e-12 {
            self.reset();
            return Vec2::default();
        }
        if distance2 <= Self::SETTLE * Self::SETTLE
            && dot(self.velocity, self.velocity) <= Self::SETTLE_SPEED * Self::SETTLE_SPEED
        {
            self.reset();
            return Vec2::default();
        }
        let omega = self.omega;
        let dt = min(dt, 0.05);
        let decay = exp(-omega * dt);
        let c = self.velocity - error * omega;
        let step = error - (error - c * dt) * decay;
        self.velocity = (self.velocity - c * (omega * dt)) * decay;
        if dot(step, error) < 0.0 {
            self.velocity = Vec2::default();
            return Vec2::default();
        }
        if dot(step, step) >= distance2 {
            return error;
        }
        step
    }
}

/// THE WALK THE REST OF THE GAME SEES (strict mode): the physical velocity smoothed over TAU, reported only as a walk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalWalkGate {
    smoothed: Vec2,
    slow_for: f32,
    walking: bool,
}

impl PhysicalWalkGate {
    pub const TAU: f32 = 0.15;
    pub const WALK_ON: f32 = 0.30;
    pub const WALK_OFF: f32 = 0.12;
    pub const WALK_OFF_HOLD: f32 = 0.25;

    pub fn reset(&mut self) {
        *self = PhysicalWalkGate::default();
    }
    pub fn update(&mut self, velocity: Vec2, dt: f32) {
        if !finite(velocity) || !dt.is_finite() || dt <= 0.0 {
            self.reset();
            return;
        }
        let dt = min(dt, 0.1);
        self.smoothed = self.smoothed + (velocity - self.smoothed) * (1.0 - exp(-dt / Self::TAU));
        let speed2 = dot(self.smoothed, self.smoothed);
        if !self.walking {
            if speed2 > Self::WALK_ON * Self::WALK_ON {
                self.walking = true;
                self.slow_for = 0.0;
            }
        } else if speed2 < Self::WALK_OFF * Self::WALK_OFF {
            self.slow_for += dt;
            if self.slow_for >= Self::WALK_OFF_HOLD {
                self.walking = false;
                self.slow_for = 0.0;
            }
        } else {
            self.slow_for = 0.0;
        }
    }
    pub fn walking(&self) -> bool {
        self.walking
    }
    pub fn reported(&self) -> Vec2 {
        if self.walking {
            self.smoothed
        } else {
            Vec2::default()
        }
    }
}

/// Heading of OpenXR -Z projected onto the floor; None near vertical or inverted poses.
pub fn head_yaw(x: f32, y: f32, z: f32, w: f32) -> Option<f32> {
    let n2 = x * x + y * y + z * z + w * w;
    if !n2.is_finite() || n2 < 0.5 || n2 > 1.5 {
        return None;
    }
    let inv = 1.0 / sqrt(n2);
    let (x, y, z, w) = (x * inv, y * inv, z * inv, w * inv);
    if 1.0 - 2.0 * (x * x + z * z) < 0.1 {
        return None;
    }
    let hx = 2.0 * (w * y + x * z);
    let hz = 1.0 - 2.0 * (x * x + y * y);
    if hx * hx + hz * hz < 0.0001 {
        return None;
    }
    Some(atan2(hx, hz))
}

/// THE NECK, NOT THE EYES: the lever's planar part (x right, y forward) for a head orientation; zero for one that is
/// not an orientation.
pub fn eye_from_neck(x: f32, y: f32, z: f32, w: f32) -> Vec2 {
    let n2 = x * x + y * y + z * z + w * w;
    if !n2.is_finite() || n2 < 0.5 || n2 > 1.5 {
        return Vec2::default();
    }
    let inv = 1.0 / sqrt(n2);
    let (x, y, z, w) = (x * inv, y * inv, z * inv, w * inv);
    let v = [0.0f32, EYE_ABOVE_NECK, -EYE_AHEAD_OF_NECK];
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    let rx = v[0] + w * t[0] + (y * t[2] - z * t[1]);
    let rz = v[2] + w * t[2] + (x * t[1] - y * t[0]);
    v2(rx, -rz)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub head: Vec2,
    pub sequence: u64,
    pub origin: u64,
    pub stamp_us: u64,
    pub valid: bool,
    pub runtime_head: Vec2,
    pub runtime_head_known: bool,
    pub eye_from_neck: Vec2,
    pub bend: Vec2,
    pub upright: bool,
}

impl Default for Sample {
    fn default() -> Self {
        Sample {
            head: Vec2::default(),
            sequence: 0,
            origin: 0,
            stamp_us: 0,
            valid: false,
            runtime_head: Vec2::default(),
            runtime_head_known: false,
            eye_from_neck: Vec2::default(),
            bend: Vec2::default(),
            upright: true,
        }
    }
}

/// The runtime packet qualifies tracking; the displacement comes from the same latched position the body renders.
pub fn movement_frame_sample(tracking: &Sample, frame: &Sample) -> Sample {
    let mut result = *frame;
    result.valid = tracking.valid && frame.valid && finite(tracking.head) && tracking.origin == frame.origin;
    result.origin = tracking.origin;
    result.stamp_us = tracking.stamp_us.min(frame.stamp_us);
    result.runtime_head = tracking.head;
    result.runtime_head_known = true;
    result
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Applied = 0,
    Baseline = 1,
    Suspended = 2,
    TrackingInvalid = 3,
    Stale = 4,
    Duplicate = 5,
    Discontinuity = 6,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub raw: Vec2,
    pub world: Vec2,
    pub yaw: f32,
    pub scale: f32,
    pub dt: f32,
    pub origin: u64,
    pub ticket: u64,
    pub reason: Reason,
}

impl Default for Step {
    fn default() -> Self {
        Step {
            raw: Vec2::default(),
            world: Vec2::default(),
            yaw: 0.0,
            scale: 0.0,
            dt: 0.0,
            origin: 0,
            ticket: 0,
            reason: Reason::Suspended,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeadLimit {
    pub eye_base: Vec2,
    pub radius: f32,
}

impl Default for LeadLimit {
    fn default() -> Self {
        LeadLimit { eye_base: Vec2::default(), radius: f32::INFINITY }
    }
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Movement {
    previous: Vec2,
    consumed: Vec2,
    pending: Vec2,
    reach: f32,
    follow: BodyTranslationFollow,
    previous_runtime: Vec2,
    origin: u64,
    sequence: u64,
    tick_us: u64,
    ticket: u64,
    completed: u64,
    active: bool,
    have_head: bool,
    runtime_known: bool,
    rebasing_filtered_jump: bool,
    /// The eye's lever off the neck at the last move (tick_eye).
    eye_from_neck: Vec2,
}

impl Default for Movement {
    fn default() -> Self {
        Movement {
            previous: Vec2::default(),
            consumed: Vec2::default(),
            pending: Vec2::default(),
            reach: -1.0,
            follow: BodyTranslationFollow::default(),
            previous_runtime: Vec2::default(),
            origin: 0,
            sequence: 0,
            tick_us: 0,
            ticket: 0,
            completed: 0,
            active: false,
            have_head: false,
            runtime_known: false,
            rebasing_filtered_jump: false,
            eye_from_neck: Vec2::default(),
        }
    }
}

impl Movement {
    pub const MAX_AGE_US: u64 = 250000;
    pub const MAX_STEP_METRES: f32 = 0.5;
    pub const STRICT_REST: f32 = 0.0001;
    pub const LEAD_EASE_BACK: f32 = 0.10;
    pub const REFUSED_MIN: f32 = 0.0005;

    pub fn reset(&mut self) {
        *self = Movement::default();
    }
    pub fn suspend(&mut self) {
        self.active = false;
        self.follow.reset();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin(&mut self, sample: &Sample, now_us: u64, dt: f32, scale: f32, tracking_yaw: f32, allowed: bool,
        free_radius_metres: f32, strict: bool) -> Step {
        let mut step = Step::default();
        if sample.origin != self.origin {
            self.reset();
            self.origin = sample.origin;
        }
        step.origin = self.origin;
        self.ticket += 1;
        step.ticket = self.ticket;
        let gap = self.tick_us != 0 && (now_us < self.tick_us || now_us - self.tick_us > Self::MAX_AGE_US);
        self.tick_us = now_us;
        if !sample.valid
            || sample.sequence == 0
            || !finite(sample.head)
            || !scale.is_finite()
            || scale < 0.05
            || scale > 20.0
            || !tracking_yaw.is_finite()
        {
            self.active = false;
            step.reason = Reason::TrackingInvalid;
            return step;
        }
        if now_us < sample.stamp_us || now_us - sample.stamp_us > Self::MAX_AGE_US {
            self.active = false;
            step.reason = Reason::Stale;
            return step;
        }
        if !allowed || !dt.is_finite() || dt <= 0.000001 || dt > 0.25 {
            self.previous = sample.head;
            self.have_head = true;
            self.sequence = sample.sequence;
            self.active = false;
            step.reason = Reason::Suspended;
            return step;
        }
        if !self.active || gap {
            self.follow.reset();
            self.previous = sample.head;
            self.have_head = true;
            self.sequence = sample.sequence;
            self.active = true;
            self.previous_runtime = sample.runtime_head;
            self.runtime_known = sample.runtime_head_known;
            self.rebasing_filtered_jump = false;
            step.reason = Reason::Baseline;
            return step;
        }
        let chasing = if strict {
            dot(self.pending * scale, self.pending * scale) > Self::STRICT_REST * Self::STRICT_REST
        } else {
            self.follow.following()
        };
        if sample.sequence < self.sequence || (sample.sequence == self.sequence && !chasing) {
            step.reason = Reason::Duplicate;
            return step;
        }
        let delta = if sample.sequence == self.sequence { Vec2::default() } else { sample.head - self.previous };
        self.previous = sample.head;
        self.have_head = true;
        self.sequence = sample.sequence;
        if finite(sample.eye_from_neck) {
            self.eye_from_neck = sample.eye_from_neck;
        }
        let metres = delta * scale;
        self.follow.head_motion((sample.head + sample.bend) * scale, sample.upright, dt);
        let runtime_delta = (sample.runtime_head - self.previous_runtime) * scale;
        let runtime_jump = sample.runtime_head_known
            && self.runtime_known
            && dot(runtime_delta, runtime_delta) > Self::MAX_STEP_METRES * Self::MAX_STEP_METRES;
        self.previous_runtime = sample.runtime_head;
        self.runtime_known = sample.runtime_head_known;
        if runtime_jump {
            self.rebasing_filtered_jump = true;
        }
        if dot(metres, metres) > Self::MAX_STEP_METRES * Self::MAX_STEP_METRES || self.rebasing_filtered_jump {
            self.follow.reset();
            self.consumed = self.consumed + delta;
            let remaining = (sample.runtime_head - sample.head - sample.bend) * scale;
            if !sample.runtime_head_known || dot(remaining, remaining) <= 0.000001 {
                self.rebasing_filtered_jump = false;
            }
            step.reason = Reason::Discontinuity;
            return step;
        }
        self.pending = self.pending + delta;
        if strict {
            self.follow.reset();
            step.raw = self.pending;
        } else {
            step.raw = self.follow.step(self.pending * scale, free_radius_metres, dt) * (1.0 / scale);
        }
        step.world = rotate(step.raw * scale, tracking_yaw);
        step.yaw = tracking_yaw;
        step.scale = scale;
        step.dt = dt;
        step.reason = Reason::Applied;
        step
    }

    /// The controller carried out the whole request.
    pub fn complete_all(&mut self, step: &Step) {
        self.complete(step, step.world, LeadLimit::default());
    }

    pub fn complete(&mut self, step: &Step, accepted_world: Vec2, limit: LeadLimit) {
        if step.origin != self.origin || step.ticket != self.ticket || self.completed == step.ticket {
            return;
        }
        self.completed = step.ticket;
        let mut accepted = step.raw;
        let refused_world = step.world - accepted_world;
        if finite(accepted_world) && step.scale > 0.0 && dot(refused_world, refused_world) > 1e-14 {
            accepted = step.raw - rotate(refused_world, -step.yaw) * (1.0 / step.scale);
        }
        self.consumed = self.consumed + accepted;
        if step.reason != Reason::Applied {
            self.reach = -1.0;
            return;
        }
        self.pending = self.pending - accepted;
        if finite(refused_world) && dot(refused_world, refused_world) > Self::REFUSED_MIN * Self::REFUSED_MIN {
            self.follow.block(step.raw - accepted);
        }
        if !limit.radius.is_finite() || !finite(limit.eye_base) || step.scale <= 0.0 {
            self.reach = -1.0;
            return;
        }
        let base = limit.eye_base + rotate((self.previous - self.consumed - self.pending) * step.scale, step.yaw);
        let mut eye = base + rotate(self.pending * step.scale, step.yaw);
        let mut reach = sqrt(dot(eye, eye));
        let blocked = finite(refused_world) && dot(refused_world, refused_world) > Self::REFUSED_MIN * Self::REFUSED_MIN;
        if blocked {
            let mut bound = max(max(limit.radius, 0.0), sqrt(dot(base, base)));
            let dt = if step.dt.is_finite() { clamp(step.dt, 0.0, 0.05) } else { 0.0 };
            if self.reach >= 0.0 {
                bound = max(bound, self.reach - Self::LEAD_EASE_BACK * dt);
            }
            if reach > bound {
                let excess = rotate(eye * (1.0 - bound / reach), -step.yaw) * (1.0 / step.scale);
                self.consumed = self.consumed + excess;
                self.pending = self.pending - excess;
                eye = eye * (bound / reach);
                reach = bound;
            }
        }
        let _ = eye;
        self.reach = reach;
    }

    pub fn consumed(&self, origin: u64) -> Vec2 {
        if origin == self.origin {
            self.consumed
        } else {
            Vec2::default()
        }
    }

    /// THE NECK AT REST, in the tracked residual's frame; None until a head has been sampled.
    pub fn rest_neck(&self, origin: u64) -> Option<Vec2> {
        if self.origin == 0 || origin != self.origin || !self.have_head {
            return None;
        }
        let neck = self.previous - self.consumed - self.pending;
        finite(neck).then_some(neck)
    }

    /// THE EYE AS THE LAST MOVE SAW IT, in the tracked residual's frame (the head less consumed): where the camera was
    /// when the capsule was last placed (main's 0.2.3 TickEye). The rendered camera has moved on since by the head's
    /// motion since that sample; the visible body follows that too (pose_hook.rs, CyberpunkVR_BodyUnderRenderedHead).
    pub fn tick_eye(&self, origin: u64) -> Option<Vec2> {
        if self.origin == 0 || origin != self.origin || !self.have_head {
            return None;
        }
        let eye = self.previous - self.consumed - self.pending + self.eye_from_neck;
        finite(eye).then_some(eye)
    }

    pub fn following(&self) -> bool {
        self.active && self.follow.following()
    }
    pub fn pending(&self) -> Vec2 {
        self.pending
    }
}

/// The part of the physical displacement the controller carried out, for the ledger (at most the loss itself).
pub fn accepted_displacement(native: Vec2, physical: Vec2, resolved: Vec2) -> Vec2 {
    if !finite(native) || !finite(physical) || !finite(resolved) {
        return physical;
    }
    let lost = native + physical - resolved;
    let lost2 = dot(lost, lost);
    if lost2 <= 1e-12 {
        return physical;
    }
    physical - lost * clamp(dot(physical, lost) / lost2, 0.0, 1.0)
}

pub fn physical_velocity(native: Vec2, physical: Vec2, resolved_velocity: Vec2, dt: f32) -> Vec2 {
    if !finite(native)
        || !finite(physical)
        || !finite(resolved_velocity)
        || !dt.is_finite()
        || dt <= 0.000001
        || dot(physical, physical) < 1e-14
    {
        return Vec2::default();
    }
    if dot(native, native) < 1e-12 {
        return resolved_velocity;
    }
    let lost = native + physical - resolved_velocity * dt;
    let lost2 = dot(lost, lost);
    let mut accepted = physical;
    if lost2 > 1e-8 {
        accepted = physical - lost * clamp(dot(physical, lost) / lost2, 0.0, 1.0);
    }
    accepted * (1.0 / dt)
}
