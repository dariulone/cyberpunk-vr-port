//! include/Natives/HandReferenceFrame.hpp: the frame a hand's pose is taken in -- an entity's place and turn, its owner
//! and the scene origin it was read against -- and the motion between two frames of the same owner.
use core::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Rotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
impl Default for Rotation {
    fn default() -> Rotation {
        Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
}
impl Vector {
    pub const fn new(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }
    pub fn from_slice(v: &[f32]) -> Vector {
        Vector { x: v[0], y: v[1], z: v[2] }
    }
    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}
impl Rotation {
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Rotation {
        Rotation { x, y, z, w }
    }
    pub fn from_slice(q: &[f32]) -> Rotation {
        Rotation { x: q[0], y: q[1], z: q[2], w: q[3] }
    }
    pub fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}
impl Add for Vector {
    type Output = Vector;
    fn add(self, b: Vector) -> Vector {
        Vector { x: self.x + b.x, y: self.y + b.y, z: self.z + b.z }
    }
}
impl Sub for Vector {
    type Output = Vector;
    fn sub(self, b: Vector) -> Vector {
        Vector { x: self.x - b.x, y: self.y - b.y, z: self.z - b.z }
    }
}
impl Mul<f32> for Vector {
    type Output = Vector;
    fn mul(self, s: f32) -> Vector {
        Vector { x: self.x * s, y: self.y * s, z: self.z * s }
    }
}
pub fn length_squared(a: Vector) -> f32 {
    a.x * a.x + a.y * a.y + a.z * a.z
}
pub fn finite(a: Vector) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}
pub fn conjugate(q: Rotation) -> Rotation {
    Rotation { x: -q.x, y: -q.y, z: -q.z, w: q.w }
}
pub fn multiply(a: Rotation, b: Rotation) -> Rotation {
    Rotation {
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}
pub fn normalize(q: &mut Rotation) -> bool {
    let n = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if !n.is_finite() || n < 1e-8 {
        return false;
    }
    let s = 1.0 / n.sqrt();
    q.x *= s;
    q.y *= s;
    q.z *= s;
    q.w *= s;
    true
}
pub fn rotate(q: Rotation, p: Vector) -> Vector {
    let t = Vector { x: 2.0 * (q.y * p.z - q.z * p.y), y: 2.0 * (q.z * p.x - q.x * p.z), z: 2.0 * (q.x * p.y - q.y * p.x) };
    Vector {
        x: p.x + q.w * t.x + q.y * t.z - q.z * t.y,
        y: p.y + q.w * t.y + q.z * t.x - q.x * t.z,
        z: p.z + q.w * t.z + q.x * t.y - q.y * t.x,
    }
}
/// An avatar's frame: where its entity is and how it is turned, whose it is and which scene origin it was read against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Reference {
    pub position: Vector,
    pub rotation: Rotation,
    pub owner: usize,
    pub origin: u64,
    pub valid: bool,
}
pub fn make(p: Vector, mut q: Rotation, owner: usize, origin: u64) -> Reference {
    let valid = owner != 0 && finite(p) && normalize(&mut q);
    Reference { position: p, rotation: q, owner, origin, valid }
}
pub fn point(frame: &Reference, local: Vector) -> Vector {
    frame.position + rotate(frame.rotation, local)
}
/// FromView: the same sample's HMD orientation and positional contribution removed from the rendered view. Animated torso
/// yaw is merely the model-space basis; it must never turn a physics hand when the tracked controller did not turn.
pub fn from_view(entity: &Reference, view_model: Vector, mut view_model_rotation: Rotation, head_delta_model: Vector, mut mapped_head: Rotation) -> Reference {
    if !entity.valid || !finite(head_delta_model) || !normalize(&mut view_model_rotation) || !normalize(&mut mapped_head) {
        return Reference::default();
    }
    make(
        point(entity, view_model - head_delta_model),
        multiply(multiply(entity.rotation, view_model_rotation), conjugate(mapped_head)),
        entity.owner,
        entity.origin,
    )
}
/// The motion from one frame to the next: a point, a direction and a turn carried along.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub from: Vector,
    pub to: Vector,
    pub turn: Rotation,
    pub valid: bool,
}
impl Motion {
    pub fn point(&self, p: Vector) -> Vector {
        if self.valid {
            self.to + rotate(self.turn, p - self.from)
        } else {
            p
        }
    }
    pub fn direction(&self, v: Vector) -> Vector {
        if self.valid {
            rotate(self.turn, v)
        } else {
            v
        }
    }
    pub fn orientation(&self, q: Rotation) -> Rotation {
        if self.valid {
            multiply(self.turn, q)
        } else {
            q
        }
    }
}
/// Between: the motion of one owner's frame from `before` to `after` (none across owners, origins, or a jump past
/// `max_travel`).
pub fn between(before: &Reference, after: &Reference, max_travel: f32) -> Motion {
    if !before.valid
        || !after.valid
        || before.owner != after.owner
        || before.origin != after.origin
        || !finite(before.position)
        || !finite(after.position)
        || !max_travel.is_finite()
        || max_travel <= 0.0
        || length_squared(after.position - before.position) > max_travel * max_travel
    {
        return Motion::default();
    }
    let mut turn = multiply(after.rotation, conjugate(before.rotation));
    if !normalize(&mut turn) {
        return Motion::default();
    }
    Motion { from: before.position, to: after.position, turn, valid: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn yaw(deg: f32) -> Rotation {
        let h = deg.to_radians() * 0.5;
        Rotation::new(0.0, 0.0, h.sin(), h.cos())
    }
    #[test]
    fn a_frame_carries_points_and_turns_with_its_owner() {
        let a = make(Vector::new(1.0, 2.0, 0.0), yaw(0.0), 7, 1);
        let b = make(Vector::new(2.0, 2.0, 0.0), yaw(90.0), 7, 1);
        let m = between(&a, &b, 5.0);
        assert!(m.valid);
        let p = m.point(Vector::new(2.0, 2.0, 0.0));
        assert!((p - Vector::new(2.0, 3.0, 0.0)).x.abs() < 1e-5 && (p.y - 3.0).abs() < 1e-5);
        assert!(!between(&a, &make(b.position, b.rotation, 8, 1), 5.0).valid, "another owner");
        assert!(!between(&a, &make(b.position, b.rotation, 7, 2), 5.0).valid, "another origin");
        assert!(!between(&a, &b, 0.5).valid, "a jump");
        assert!(!make(a.position, a.rotation, 0, 1).valid, "no owner");
    }
    #[test]
    fn the_view_without_the_head_is_the_entity_turned_by_the_view() {
        let e = make(Vector::new(0.0, 0.0, 0.0), yaw(30.0), 1, 0);
        let f = from_view(&e, Vector::new(0.0, 0.0, 1.6), yaw(10.0), Vector::new(0.0, 0.0, 0.1), yaw(10.0));
        assert!(f.valid);
        assert!((f.position.z - 1.5).abs() < 1e-6);
        let d = multiply(f.rotation, conjugate(yaw(30.0)));
        assert!(d.w.abs() > 0.99999);
    }
}
