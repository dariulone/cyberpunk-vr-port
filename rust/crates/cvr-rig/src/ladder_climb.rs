//! Climbing a ladder by the hands (formerly include/Runtimes/LadderClimbing.hpp and Anim/LadderRungWrist.hpp): the
//! ladder's rails, rungs and finisher rails, the closest grip for a hand, and the climber that turns two gripping hands'
//! pull into the native ladder input -- with the intentional pull near the top that hands the exit to the game.
use crate::ladder::HANDS;
use cvr_body::cmath::{clamp, max, min};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl core::ops::Add for Vec {
    type Output = Vec;
    fn add(self, b: Vec) -> Vec {
        Vec { x: self.x + b.x, y: self.y + b.y, z: self.z + b.z }
    }
}
impl core::ops::Sub for Vec {
    type Output = Vec;
    fn sub(self, b: Vec) -> Vec {
        Vec { x: self.x - b.x, y: self.y - b.y, z: self.z - b.z }
    }
}
impl core::ops::Mul<f32> for Vec {
    type Output = Vec;
    fn mul(self, k: f32) -> Vec {
        Vec { x: self.x * k, y: self.y * k, z: self.z * k }
    }
}
pub fn dot(a: Vec, b: Vec) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
pub fn length(a: Vec) -> f32 {
    dot(a, a).sqrt()
}
pub fn finite(a: Vec) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}
pub fn cross(a: Vec, b: Vec) -> Vec {
    Vec { x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x }
}
pub fn normalize(a: &mut Vec) -> bool {
    let n = length(*a);
    if !finite(*a) || n < 0.001 {
        return false;
    }
    *a = *a * (1.0 / n);
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
impl Default for Rotation {
    fn default() -> Self {
        Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
}
impl core::ops::Mul for Rotation {
    type Output = Rotation;
    fn mul(self, b: Rotation) -> Rotation {
        let a = self;
        Rotation {
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        }
    }
}
impl Rotation {
    pub fn inverse(self) -> Rotation {
        Rotation { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }
    pub fn rotate(self, v: Vec) -> Vec {
        let q = self * Rotation { x: v.x, y: v.y, z: v.z, w: 0.0 } * self.inverse();
        Vec { x: q.x, y: q.y, z: q.z }
    }
    pub fn normalize(&mut self) -> bool {
        let n = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if !n.is_finite() || n < 0.001 {
            return false;
        }
        self.x /= n;
        self.y /= n;
        self.z /= n;
        self.w /= n;
        true
    }
}
pub fn from_axes(x: Vec, y: Vec, z: Vec) -> Rotation {
    let trace = x.x + y.y + z.z;
    let mut q = if trace > 0.0 {
        let s = 2.0 * (trace + 1.0).sqrt();
        Rotation { x: (y.z - z.y) / s, y: (z.x - x.z) / s, z: (x.y - y.x) / s, w: s * 0.25 }
    } else if x.x > y.y && x.x > z.z {
        let s = 2.0 * (1.0 + x.x - y.y - z.z).sqrt();
        Rotation { x: s * 0.25, y: (y.x + x.y) / s, z: (z.x + x.z) / s, w: (y.z - z.y) / s }
    } else if y.y > z.z {
        let s = 2.0 * (1.0 + y.y - x.x - z.z).sqrt();
        Rotation { x: (y.x + x.y) / s, y: s * 0.25, z: (z.y + y.z) / s, w: (z.x - x.z) / s }
    } else {
        let s = 2.0 * (1.0 + z.z - x.x - y.y).sqrt();
        Rotation { x: (z.x + x.z) / s, y: (z.y + y.z) / s, z: s * 0.25, w: (x.y - y.x) / s }
    };
    q.normalize();
    q
}
pub fn palm_offset(side: usize) -> Vec {
    let p = HANDS[side].contact;
    Vec { x: p[0], y: p[1], z: p[2] }
}
pub fn on_ladder(detailed: i32) -> bool {
    (10..=12).contains(&detailed)
}
pub fn fresh(owner: usize, current: usize, stamp: u64, now: u64, detailed: i32) -> bool {
    owner != 0 && owner == current && stamp != 0 && now >= stamp && now - stamp <= 250000 && on_ladder(detailed)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    pub a: Vec,
    pub b: Vec,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub position: Vec,
    pub normal: Vec,
    pub up: Vec,
    pub right: Vec,
    pub height: f32,
    pub top_step: f32,
    pub half_width: f32,
    pub rung_step: f32,
    pub first_rung: f32,
    /// The top of the ladder's highest module, measured from its meshes (0 = not known).
    pub measured_top: f32,
    pub top_rails: [Segment; 10],
    pub top_rail_count: i32,
    pub identity: u64,
    pub valid: bool,
}
impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            position: Vec::default(),
            normal: Vec::default(),
            up: Vec::default(),
            right: Vec::default(),
            height: 0.0,
            top_step: 0.0,
            half_width: 0.30,
            rung_step: 0.40,
            first_rung: 0.40,
            measured_top: 0.0,
            top_rails: [Segment::default(); 10],
            top_rail_count: 0,
            identity: 0,
            valid: false,
        }
    }
}
impl Geometry {
    pub fn prepare(&mut self) -> bool {
        self.valid = false;
        if !finite(self.position) || !normalize(&mut self.up) || !normalize(&mut self.normal) || !self.height.is_finite() || self.height < 0.5 || self.height > 100.0 {
            return false;
        }
        self.normal = self.normal - self.up * dot(self.normal, self.up);
        if !normalize(&mut self.normal) {
            return false;
        }
        self.right = cross(self.up, self.normal);
        if !normalize(&mut self.right) {
            return false;
        }
        self.valid = self.half_width > 0.1
            && self.half_width < 1.0
            && self.rung_step > 0.1
            && self.rung_step < 1.0
            && self.first_rung.is_finite()
            && self.top_step.is_finite()
            && self.top_step.abs() <= 1.0
            && self.identity != 0;
        self.valid
    }
    /// The exit plane is one step below the physical top; the highest module's measured top is the last rung's height
    /// when it is known.
    pub fn physical_top(&self) -> f32 {
        if self.measured_top > 0.0 {
            self.measured_top
        } else {
            self.height - min(self.top_step, 0.0)
        }
    }
    pub fn set_measured_top(&mut self, top: f32) -> bool {
        if !self.valid || !top.is_finite() || top < self.height - 1.0 || top > self.height + 3.0 || top > 100.0 {
            return false;
        }
        self.measured_top = top;
        true
    }
    pub fn set_top_rails(&mut self, origin: Vec, mut x: Vec, mut y: Vec, mut z: Vec) -> bool {
        if !self.valid
            || !finite(origin)
            || !normalize(&mut x)
            || !normalize(&mut y)
            || !normalize(&mut z)
            || dot(z, self.up) < 0.98
            || dot(x, self.right).abs() < 0.98
            || dot(y, self.normal).abs() < 0.98
        {
            return false;
        }
        let delta = origin - self.position;
        if (dot(delta, self.up) - self.physical_top()).abs() > 0.25 || dot(delta, self.right).abs() > 0.15 || dot(delta, self.normal).abs() > 0.4 {
            return false;
        }
        // generic_ladder_f_finisher_rail: two U-shaped rails, centre lines measured from its mesh.
        self.top_rail_count = 0;
        let point = |p: Vec| origin + x * p.x + y * p.y + z * p.z;
        let v = |a: f32, b: f32, c: f32| Vec { x: a, y: b, z: c };
        for side in [-1.0f32, 1.0] {
            let (outer, inner) = (side * 0.4023, side * 0.287);
            let segs = [
                (v(inner, 0.1235, 0.013), v(outer, 0.1235, 0.08)),
                (v(outer, 0.1235, 0.08), v(outer, 0.1235, 1.035)),
                (v(outer, 0.1235, 1.035), v(outer, -0.1579, 1.035)),
                (v(outer, -0.1579, 1.035), v(outer, -0.1579, 0.08)),
                (v(outer, -0.1579, 0.08), v(inner, -0.1579, 0.013)),
            ];
            for (a, b) in segs {
                self.top_rails[self.top_rail_count as usize] = Segment { a: point(a), b: point(b) };
                self.top_rail_count += 1;
            }
        }
        true
    }
}

/// kind 1 rail, 2 rung, 3 top rail.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Contact {
    pub point: Vec,
    pub distance: f32,
    pub kind: i32,
    pub axis: Vec,
}
/// std::round: half away from zero (f32::round), on the float.
pub fn closest(g: &Geometry, world: Vec) -> Contact {
    if !g.valid || !finite(world) {
        return Contact::default();
    }
    let d = world - g.position;
    let (x, z) = (dot(d, g.right), dot(d, g.up));
    let rail = if x < 0.0 { -g.half_width } else { g.half_width };
    let rail_point = g.position + g.right * rail + g.up * clamp(z, 0.0, g.physical_top()) + g.normal * 0.025;
    let rung = g.first_rung + ((z - g.first_rung) / g.rung_step).round() * g.rung_step;
    let mut result = Contact { point: rail_point, distance: length(world - rail_point), kind: 1, axis: g.up };
    if rung >= 0.0 && rung <= g.physical_top() + 0.001 {
        let point = g.position + g.right * clamp(x, -g.half_width, g.half_width) + g.up * rung + g.normal * 0.025;
        let distance = length(world - point);
        if distance < result.distance {
            result = Contact { point, distance, kind: 2, axis: g.right };
        }
    }
    for i in 0..g.top_rail_count as usize {
        let line = g.top_rails[i];
        let span = line.b - line.a;
        let point = line.a + span * clamp(dot(world - line.a, span) / max(dot(span, span), 1e-8), 0.0, 1.0) + g.normal * 0.025;
        let distance = length(world - point);
        if distance < result.distance {
            let mut axis = span;
            normalize(&mut axis);
            if dot(axis, g.up) < 0.0 {
                axis = axis * -1.0;
            }
            result = Contact { point, distance, kind: 3, axis };
        }
    }
    result
}
pub fn grip_rotation(g: &Geometry, contact: &Contact, side: usize) -> Rotation {
    let mut up = contact.axis;
    // Opposite thumbs point toward the centre on a horizontal rung.
    if contact.kind == 2 && side == 1 {
        up = up * -1.0;
    }
    let mut forward = g.normal * -1.0;
    forward = forward - up * dot(forward, up);
    if !normalize(&mut forward) {
        forward = g.up * -1.0;
        forward = forward - up * dot(forward, up);
        normalize(&mut forward);
    }
    let mut right = cross(forward, up);
    normalize(&mut right);
    forward = cross(up, right);
    normalize(&mut forward);
    let q = HANDS[side].rotation;
    from_axes(right, forward, up) * Rotation { x: q[0], y: q[1], z: q[2], w: q[3] }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub tracking: [Vec; 2],
    pub world: [Vec; 2],
    pub tracking_up: Vec,
    pub grip: [f32; 2],
    pub height: f32,
    pub finish_distance: f32,
    pub auto_finish: bool,
    pub hand_valid: [bool; 2],
    pub valid: bool,
    pub sequence: u64,
    pub origin: u64,
    pub stamp: u64,
}
impl Default for Frame {
    fn default() -> Self {
        Frame {
            tracking: [Vec::default(); 2],
            world: [Vec::default(); 2],
            tracking_up: Vec { x: 0.0, y: 0.0, z: 1.0 },
            grip: [0.0; 2],
            height: 0.0,
            finish_distance: 1.0,
            auto_finish: true,
            hand_valid: [false; 2],
            valid: false,
            sequence: 0,
            origin: 0,
            stamp: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Climber {
    down: [bool; 2],
    held: [bool; 2],
    rotation: [Rotation; 2],
    kind: [i32; 2],
    previous: [Vec; 2],
    anchor: [Vec; 2],
    target: f32,
    debt: f32,
    finishing: bool,
    up_intent: f32,
    best_height: f32,
    finish_start: u64,
    progress_stamp: u64,
    sequence: u64,
    stamp: u64,
    origin: u64,
    geometry: u64,
}
impl Climber {
    pub fn reset(&mut self) {
        *self = Climber::default();
    }
    pub fn held(&self, side: i32) -> bool {
        (0..2).contains(&side) && self.held[side as usize]
    }
    pub fn anchor(&self, side: usize) -> Vec {
        self.anchor[side]
    }
    pub fn kind(&self, side: usize) -> i32 {
        self.kind[side]
    }
    pub fn debt(&self) -> f32 {
        self.debt
    }
    pub fn finishing(&self) -> bool {
        self.finishing
    }
    /// The rail touches the palm, not the wrist joint IK targets.
    pub fn constrain(&self, side: i32, body_position: Vec, mut body_rotation: Rotation, target: &mut Vec, hand_rotation: &mut Rotation) -> bool {
        if !self.held(side) || !finite(body_position) || !body_rotation.normalize() || !hand_rotation.normalize() {
            return false;
        }
        let s = side as usize;
        let inverse = body_rotation.inverse();
        *target = inverse.rotate(self.anchor[s] - self.rotation[s].rotate(palm_offset(s)) - body_position);
        *hand_rotation = inverse * self.rotation[s];
        true
    }
    fn release(&mut self, f: &Frame) {
        self.reset();
        self.target = f.height;
        for i in 0..2 {
            self.down[i] = f.grip[i] > 0.35;
            self.previous[i] = f.tracking[i];
        }
    }
    pub fn update(&mut self, g: &Geometry, f: &Frame, now: u64, allowed: bool) -> f32 {
        let valid = allowed
            && g.valid
            && f.valid
            && f.sequence != 0
            && f.origin != 0
            && f.stamp != 0
            && now >= f.stamp
            && now - f.stamp <= 250000
            && f.height.is_finite()
            && finite(f.tracking_up);
        if !valid {
            self.release(f);
            return 0.0;
        }
        if self.origin != f.origin || self.geometry != g.identity || (self.stamp != 0 && (f.stamp < self.stamp || f.stamp - self.stamp > 150000)) {
            self.release(f);
            self.origin = f.origin;
            self.geometry = g.identity;
        }
        let fresh = f.sequence != self.sequence;
        let dt = if self.stamp != 0 && f.stamp > self.stamp { (f.stamp - self.stamp) as f32 * 1e-6 } else { 0.02 };
        let previous_hands = self.held[0] as i32 + self.held[1] as i32;
        let mut pull = 0.0f32;
        let mut moving_hands = 0i32;
        for side in 0..2 {
            let (was_down, was_held) = (self.down[side], self.held[side]);
            if f.grip[side] <= 0.35 {
                self.down[side] = false;
            } else if f.grip[side] >= 0.65 {
                self.down[side] = true;
            }
            if !self.down[side] {
                self.held[side] = false;
                self.kind[side] = 0;
            }
            if !f.hand_valid[side] || !finite(f.tracking[side]) || !finite(f.world[side]) {
                self.held[side] = false;
                continue;
            }
            if !was_down && self.down[side] {
                let contact = closest(g, f.world[side]);
                if contact.kind != 0 && contact.distance <= 0.11 {
                    self.held[side] = true;
                    self.anchor[side] = contact.point;
                    self.kind[side] = contact.kind;
                    self.rotation[side] = grip_rotation(g, &contact, side);
                }
            }
            if self.held[side] && length(f.world[side] - self.anchor[side]) > 0.65 {
                self.held[side] = false;
                self.kind[side] = 0;
            }
            if fresh && was_held && self.held[side] {
                let delta = f.tracking[side] - self.previous[side];
                if length(delta) > max(0.18, 4.0 * dt) {
                    self.held[side] = false;
                    self.kind[side] = 0;
                } else {
                    pull -= dot(delta, f.tracking_up);
                    moving_hands += 1;
                }
            }
            if fresh {
                self.previous[side] = f.tracking[side];
            }
        }
        let hands = self.held[0] as i32 + self.held[1] as i32;
        let was_finishing = self.finishing;
        let signed_pull = if moving_hands != 0 { pull / moving_hands as f32 } else { 0.0 };
        let remaining = g.height - f.height;
        let finish_distance = clamp(f.finish_distance, 0.2, 1.2);
        // THE FLOOR AT THE TOP CAN LIE ABOVE THE EXIT PLANE: by -topStep.
        let floor_above = max(0.0, -min(g.top_step, 0.0));
        if !f.auto_finish || remaining > finish_distance || remaining < -(floor_above + 0.35) || (!f.hand_valid[0] && !f.hand_valid[1]) || signed_pull < -0.008 {
            self.finishing = false;
            self.up_intent = 0.0;
        } else if !self.finishing {
            self.up_intent = clamp(self.up_intent + signed_pull, 0.0, 0.08);
            if hands != 0 && self.up_intent >= 0.05 {
                self.finishing = true;
                self.finish_start = now;
                self.progress_stamp = now;
                self.best_height = f.height;
            }
        }
        if self.finishing {
            if f.height > self.best_height + 0.01 {
                self.best_height = f.height;
                self.progress_stamp = now;
            }
            if now.wrapping_sub(self.finish_start) > 4000000 || now.wrapping_sub(self.progress_stamp) > 1200000 {
                self.finishing = false;
                self.up_intent = 0.0;
            }
        }
        if was_finishing || self.finishing {
            self.target = f.height;
            self.debt = 0.0;
        }
        if hands == 0 {
            self.target = f.height;
            self.debt = 0.0;
        } else {
            if previous_hands == 0 {
                self.target = f.height;
            }
            if moving_hands != 0 {
                self.target += pull / moving_hands as f32;
            }
            self.target = clamp(self.target, f.height - 0.30, f.height + 0.30);
            self.debt = self.target - f.height;
        }
        self.sequence = f.sequence;
        if fresh {
            self.stamp = f.stamp;
        }
        if self.finishing {
            return 0.65;
        }
        if hands == 0 || self.debt.abs() < 0.002 {
            return 0.0;
        }
        clamp(self.debt / max(0.12, 3.0 * min(dt, 0.15)), -0.75, 0.75)
    }
}

/// A rail's native wrist sits above the grip when rotated onto a rung: the approved 25 degree pitch, then the 2 cm
/// downward grip calibration in ladder axes.
pub fn correct_rung_wrist(side: i32, mut rung_axis: Vec, mut ladder_up: Vec, wrist: &mut Vec, rotation: &mut Rotation) {
    if !(0..=1).contains(&side) || !normalize(&mut rung_axis) || !normalize(&mut ladder_up) {
        return;
    }
    let palm = palm_offset(side as usize);
    let contact = *wrist + rotation.rotate(palm);
    let half_angle = 25.0f32 * 0.008_726_646_26;
    let s = cvr_body::cmath::sin(half_angle);
    let pitch = Rotation { x: rung_axis.x * s, y: rung_axis.y * s, z: rung_axis.z * s, w: cvr_body::cmath::cos(half_angle) };
    *rotation = pitch * *rotation;
    rotation.normalize();
    *wrist = contact - rotation.rotate(palm) - ladder_up * 0.02;
}
