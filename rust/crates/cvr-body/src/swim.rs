//! Swimming by the arms (formerly include/Runtimes/SwimmingGesture.hpp and SwimmingMovement.hpp): the breaststroke
//! that drives the native forward input, the push down that ascends, the sprint button held through a boost, and the
//! swim stroke's heading turned to the head's.
use crate::cmath::{cos, exp, hypot, max, min, sin};
use crate::xr::{rotate_vector, Quat, Vec3};

/// PSMSwimming: Surface 1, Diving 2.
pub fn in_water(state: i32) -> bool {
    state == 1 || state == 2
}
pub fn state_fresh(owner: usize, current: usize, stamp: u64, now: u64, state: i32) -> bool {
    owner != 0 && owner == current && stamp != 0 && now >= stamp && now - stamp <= 250000 && in_water(state)
}
/// SwimmingTransitionEvents publishes Default for a moment while diving: physical submersion keeps the posture.
pub fn presence_fresh(owner: usize, current: usize, stamp: u64, now: u64, state: i32, water_context: bool) -> bool {
    owner != 0 && owner == current && stamp != 0 && now >= stamp && now - stamp <= 250000 && (in_water(state) || water_context)
}

/// Recenter-space metres, before weapon offsets.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub head: Vec3,
    pub hands: [Vec3; 2],
    pub orientation: Quat,
    pub sequence: u64,
    pub stamp_us: u64,
    pub origin: u64,
    pub valid: bool,
}
impl Default for Sample {
    fn default() -> Self {
        Sample { head: Vec3::default(), hands: [Vec3::default(); 2], orientation: Quat::default(), sequence: 0, stamp_us: 0, origin: 0, valid: false }
    }
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}
fn finite(a: Vec3) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}
fn clamp01(t: f32) -> f32 {
    // std::clamp(t, 0, 1): a NaN passes through
    if 1.0 < t {
        1.0
    } else if t < 0.0 {
        0.0
    } else {
        t
    }
}
fn smooth(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}
pub fn hand_in_tracking_space(head: Vec3, rotation: Quat, local: Vec3) -> Vec3 {
    let p = rotate_vector(rotation, local);
    Vec3 { x: head.x + p.x, y: head.y + p.y, z: head.z + p.z }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub forward: f32,
    pub stroke: bool,
    pub boost: bool,
    pub speed: f32,
}
/// Fast swimming latches SprintToggled: a normal stroke dips under the live 0.80 threshold until the fast state clears.
pub fn forward_action(forward: f32, boost: bool, fast: i32, manual_sprint: bool) -> f32 {
    if !boost && fast == 1 && !manual_sprint {
        min(forward, 0.75)
    } else {
        forward
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Reach,
    Pull,
    Recover,
}

/// The breaststroke: both hands reached forward, then pulled back and out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Breaststroke {
    phase: Phase,
    sequence: u64,
    stamp: u64,
    origin: u64,
    reach_since: u64,
    armed_at: u64,
    last_stroke: u64,
    began: u64,
    until: u64,
    start: [Vec3; 2],
    previous: [Vec3; 2],
    forward: Vec3,
    right: Vec3,
    out: [f32; 2],
    velocity: [f32; 2],
    speed: f32,
    progress: f32,
    fast_since: u64,
    slow_since: u64,
    driving: bool,
    boost: bool,
    provisional: bool,
}
impl Breaststroke {
    pub fn reset(&mut self) {
        *self = Breaststroke::default();
    }
    pub fn current_phase(&self) -> Phase {
        self.phase
    }
    pub fn speed(&self) -> f32 {
        self.speed
    }
    fn forward_at(&self, now: u64) -> f32 {
        if self.until == 0 || now >= self.until || now < self.began {
            return 0.0;
        }
        smooth((now - self.began) as f32 / 35000.0) * smooth((self.until - now) as f32 / if self.provisional { 60000.0 } else { 180000.0 })
    }
    fn output(&self, now: u64, stroke: bool) -> Motion {
        let forward = self.forward_at(now);
        Motion { forward, stroke, boost: forward > 0.0 && self.boost, speed: self.speed }
    }
    pub fn update(&mut self, p: &Sample, now: u64, allowed: bool) -> Motion {
        let o = p.orientation;
        let norm = o.x * o.x + o.y * o.y + o.z * o.z + o.w * o.w;
        let mut valid = allowed
            && p.valid
            && p.sequence != 0
            && p.origin != 0
            && p.stamp_us != 0
            && now >= p.stamp_us
            && now - p.stamp_us <= 250000
            && norm.is_finite()
            && norm > 0.5
            && norm < 1.5
            && finite(p.head);
        for hand in p.hands {
            valid = valid && finite(hand) && length(sub(hand, p.head)) < 2.0;
        }
        if !valid {
            self.reset();
            return Motion::default();
        }
        if self.origin != p.origin {
            self.reset();
            self.origin = p.origin;
        }
        if p.sequence == self.sequence {
            return self.output(now, false);
        }
        if self.stamp != 0 && (p.stamp_us <= self.stamp || p.stamp_us - self.stamp > 150000) {
            self.reset();
            self.origin = p.origin;
        }
        let dt = if self.stamp != 0 { (p.stamp_us - self.stamp) as f32 * 1e-6 } else { 0.0 };
        let mut movement = [Vec3::default(); 2];
        if self.stamp != 0 {
            for i in 0..2 {
                movement[i] = sub(p.hands[i], self.previous[i]);
                if length(movement[i]) > max(0.15, 5.0 * dt) {
                    self.reset();
                    self.origin = p.origin;
                    return Motion::default();
                }
            }
        }
        self.sequence = p.sequence;
        self.stamp = p.stamp_us;
        self.previous = p.hands;
        let mut forward = rotate_vector(p.orientation, Vec3 { x: 0.0, y: 0.0, z: -1.0 });
        let len = length(forward);
        if len < 0.01 {
            self.reset();
            return Motion::default();
        }
        forward = Vec3 { x: forward.x / len, y: forward.y / len, z: forward.z / len };
        let right = rotate_vector(p.orientation, Vec3 { x: 1.0, y: 0.0, z: 0.0 });
        let up = rotate_vector(p.orientation, Vec3 { x: 0.0, y: 1.0, z: 0.0 });
        let (left, r) = (sub(p.hands[0], p.head), sub(p.hands[1], p.head));
        let width = dot(sub(r, left), right);
        let reach = dot(left, forward) > 0.22
            && dot(r, forward) > 0.22
            && width > 0.06
            && width < 0.50
            && dot(left, right) < 0.12
            && dot(r, right) > -0.12
            && dot(left, up) > -0.85
            && dot(left, up) < 0.30
            && dot(r, up) > -0.85
            && dot(r, up) < 0.30
            && dot(sub(left, r), up).abs() < 0.30;
        let mut stroke = false;
        if self.phase == Phase::Recover && reach && now.wrapping_sub(self.last_stroke) > 250000 {
            self.phase = Phase::Reach;
        }
        if self.phase == Phase::Reach {
            if !reach {
                self.reach_since = 0;
            } else if self.reach_since == 0 {
                self.reach_since = p.stamp_us;
            } else if p.stamp_us.wrapping_sub(self.reach_since) >= 60000 {
                self.phase = Phase::Pull;
                self.armed_at = 0;
                self.forward = forward;
                self.right = right;
                self.driving = false;
                self.progress = 0.0;
                self.fast_since = 0;
                self.slow_since = 0;
                self.velocity = [0.0; 2];
                self.start = p.hands;
                self.out = [0.0; 2];
            }
        } else if self.phase == Phase::Pull {
            let mut pull = [0.0f32; 2];
            for i in 0..2 {
                let travel = sub(p.hands[i], self.start[i]);
                pull[i] = -dot(travel, self.forward);
                self.out[i] = max(self.out[i], dot(travel, self.right) * if i == 1 { 1.0 } else { -1.0 });
                if dt > 0.0 {
                    let speed = -dot(movement[i], self.forward) / dt;
                    self.velocity[i] += (speed - self.velocity[i]) * (1.0 - exp(-dt / 0.05));
                }
            }
            self.speed = max(0.0, (self.velocity[0] + self.velocity[1]) * 0.5);
            if self.armed_at == 0 && (pull[0] > 0.03 || pull[1] > 0.03 || self.out[0] > 0.03 || self.out[1] > 0.03) {
                self.armed_at = p.stamp_us;
            }
            let elapsed = if self.armed_at != 0 { (p.stamp_us - self.armed_at) as f32 * 1e-6 } else { 0.0 };
            let progress = (pull[0] + pull[1]) * 0.5;
            if elapsed > 2.2 || dot(forward, self.forward) < 0.5 || pull[0] < -0.08 || pull[1] < -0.08 {
                self.phase = Phase::Reach;
                self.reach_since = 0;
                if self.provisional {
                    self.until = self.until.min(now + 60000);
                }
            } else {
                let early = elapsed >= 0.04
                    && pull[0] > 0.035
                    && pull[1] > 0.035
                    && progress > 0.05
                    && (pull[0] - pull[1]).abs() < 0.12
                    && self.out[0] > 0.02
                    && self.out[1] > 0.02
                    && progress / elapsed > 0.15
                    && self.velocity[0] > 0.08
                    && self.velocity[1] > 0.08;
                let advancing = progress > self.progress + 0.002;
                if early && advancing {
                    if self.until <= now {
                        self.began = now;
                        self.boost = false;
                    }
                    let preview_end = now + 120000;
                    if preview_end > self.until {
                        self.until = preview_end;
                        self.provisional = true;
                    }
                    self.driving = true;
                    self.progress = progress;
                }
                if self.driving && self.speed >= 0.75 && min(self.velocity[0], self.velocity[1]) > 0.35 {
                    if self.fast_since == 0 {
                        self.fast_since = p.stamp_us;
                    }
                    if p.stamp_us.wrapping_sub(self.fast_since) >= 40000 {
                        self.boost = true;
                    }
                    self.slow_since = 0;
                } else if self.driving && early && self.speed < 0.45 {
                    if self.slow_since == 0 {
                        self.slow_since = p.stamp_us;
                    }
                    if p.stamp_us.wrapping_sub(self.slow_since) >= 80000 {
                        self.boost = false;
                    }
                    self.fast_since = 0;
                } else {
                    self.fast_since = 0;
                    self.slow_since = 0;
                }
                if elapsed >= 0.12
                    && pull[0] > 0.16
                    && pull[1] > 0.16
                    && progress > 0.20
                    && (pull[0] - pull[1]).abs() < 0.20
                    && self.out[0] > 0.045
                    && self.out[1] > 0.045
                    && progress / elapsed > 0.15
                {
                    stroke = true;
                    self.phase = Phase::Recover;
                    self.reach_since = 0;
                    self.last_stroke = now;
                    if self.until <= now {
                        self.began = now;
                        self.boost = false;
                    }
                    self.until = now + 1150000;
                    self.provisional = false;
                }
            }
        }
        self.output(now, stroke)
    }
}

/// Separated hands raised toward chest level, then both pushed down: absolute tracking travel, so a moving head
/// invents no lift.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AscendStroke {
    origin: u64,
    sequence: u64,
    stamp: u64,
    ready_since: u64,
    started: u64,
    last_stroke: u64,
    until: u64,
    began: u64,
    start: [Vec3; 2],
    previous: [Vec3; 2],
    head_start: Vec3,
    armed: bool,
    recover: bool,
}
impl AscendStroke {
    pub fn reset(&mut self) {
        *self = AscendStroke::default();
    }
    fn output(&self, now: u64, stroke: bool) -> Motion {
        let strength = if self.until > now && now >= self.began {
            smooth((now - self.began) as f32 / 80000.0) * smooth((self.until - now) as f32 / 160000.0)
        } else {
            0.0
        };
        Motion { forward: strength, stroke, boost: false, speed: 0.0 }
    }
    pub fn update(&mut self, p: &Sample, now: u64, allowed: bool) -> Motion {
        let o = p.orientation;
        let norm = o.x * o.x + o.y * o.y + o.z * o.z + o.w * o.w;
        if !allowed
            || !p.valid
            || p.sequence == 0
            || p.origin == 0
            || p.stamp_us == 0
            || now < p.stamp_us
            || now - p.stamp_us > 250000
            || !finite(p.head)
            || !norm.is_finite()
            || norm < 0.5
            || norm > 1.5
        {
            self.reset();
            return Motion::default();
        }
        for hand in p.hands {
            if !finite(hand) || length(sub(hand, p.head)) > 2.0 {
                self.reset();
                return Motion::default();
            }
        }
        if self.origin != p.origin {
            self.reset();
            self.origin = p.origin;
        }
        if p.sequence == self.sequence {
            return self.output(now, false);
        }
        if self.stamp != 0 && (p.stamp_us <= self.stamp || p.stamp_us - self.stamp > 150000) {
            self.reset();
            self.origin = p.origin;
        }
        if self.stamp != 0 {
            for i in 0..2 {
                if length(sub(p.hands[i], self.previous[i])) > max(0.15, 5.0 * (p.stamp_us - self.stamp) as f32 * 1e-6) {
                    self.reset();
                    return Motion::default();
                }
            }
        }
        self.sequence = p.sequence;
        self.stamp = p.stamp_us;
        self.previous = p.hands;
        let width = length(sub(p.hands[1], p.hands[0]));
        let mut ready = width > 0.30 && width < 1.1 && (p.hands[0].y - p.hands[1].y).abs() < 0.20;
        for hand in p.hands {
            ready = ready && hand.y - p.head.y > -0.38 && hand.y - p.head.y < 0.30;
        }
        if self.recover && ready && now.wrapping_sub(self.last_stroke) > 250000 {
            self.recover = false;
        }
        if self.recover {
            return self.output(now, false);
        }
        if !self.armed {
            if !ready {
                self.ready_since = 0;
            } else if self.ready_since == 0 {
                self.ready_since = p.stamp_us;
            } else if p.stamp_us.wrapping_sub(self.ready_since) >= 60000 {
                self.armed = true;
                self.started = 0;
                self.head_start = p.head;
                self.start = p.hands;
            }
            return self.output(now, false);
        }
        let (mut down, mut relative_down) = ([0.0f32; 2], [0.0f32; 2]);
        let mut lateral = false;
        for i in 0..2 {
            let d = sub(p.hands[i], self.start[i]);
            down[i] = -d.y;
            relative_down[i] = down[i] + p.head.y - self.head_start.y;
            lateral = lateral || hypot(d.x, d.z) > 0.18;
        }
        if self.started == 0 && (down[0] > 0.03 || down[1] > 0.03) {
            self.started = p.stamp_us;
        }
        let elapsed = if self.started != 0 { (p.stamp_us - self.started) as f32 * 1e-6 } else { 0.0 };
        if lateral || elapsed > 1.8 || down[0] < -0.15 || down[1] < -0.15 {
            self.armed = false;
            self.ready_since = 0;
            return self.output(now, false);
        }
        if elapsed >= 0.12
            && down[0] > 0.22
            && down[1] > 0.22
            && relative_down[0] > 0.18
            && relative_down[1] > 0.18
            && (down[0] - down[1]).abs() < 0.16
            && (down[0] + down[1]) * 0.5 / elapsed > 0.20
        {
            self.armed = false;
            self.recover = true;
            self.ready_since = 0;
            self.last_stroke = now;
            if self.until <= now {
                self.began = now;
            }
            self.until = now + 800000;
            return self.output(now, true);
        }
        self.output(now, false)
    }
}

/// ToggleSprint is LShift/L3: kept pressed through a boost, not pressed again when fast swimming was already on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SprintButton {
    active: bool,
    down: bool,
    saw_fast: bool,
    release_until: u64,
}
impl SprintButton {
    pub fn reset(&mut self) {
        *self = SprintButton::default();
    }
    pub fn update(&mut self, wanted: bool, fast: i32, now: u64) -> bool {
        if !wanted {
            self.reset();
            return false;
        }
        if !self.active {
            self.active = true;
            self.down = fast != 1;
            self.saw_fast = fast == 1;
        }
        if fast == 1 {
            self.saw_fast = true;
            return self.down;
        }
        if fast == 0 && self.saw_fast && self.down {
            self.down = false;
            self.saw_fast = false;
            self.release_until = now + 60000;
        } else if !self.down && now >= self.release_until {
            self.down = true;
        }
        self.down
    }
}

/// The swim stroke's heading turned to the head's, its horizontal distance kept (diving already carries the pitch in z).
pub fn redirect(native: [f32; 3], head_forward: [f32; 3], tracking_yaw: f32) -> [f32; 3] {
    let finite3 = |v: [f32; 3]| v.iter().all(|c| c.is_finite());
    if !finite3(native) || !tracking_yaw.is_finite() {
        return native;
    }
    let mut d = [head_forward[0], -head_forward[2], 0.0];
    let norm = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if !finite3(d) || norm < 1e-8 {
        return native;
    }
    let k = 1.0 / norm.sqrt();
    d = [d[0] * k, d[1] * k, d[2] * k];
    let (c, s) = (cos(tracking_yaw), sin(tracking_yaw));
    d = [c * d[0] - s * d[1], s * d[0] + c * d[1], d[2]];
    let distance = hypot(native[0], native[1]);
    [distance * d[0], distance * d[1], native[2]]
}
