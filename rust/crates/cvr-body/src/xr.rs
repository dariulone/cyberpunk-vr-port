//! OpenXR's vector and quaternion, and the quaternion math of include/Utils/XrMath.hpp, operation for operation.
use crate::cmath::sqrt;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
    /// `XrQuaternionf{}`: all zero, as a value-initialised C++ member is.
    pub const ZERO: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 0.0 };
}

impl Default for Quat {
    fn default() -> Self {
        Quat::ZERO
    }
}

pub const fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

pub fn multiply_quat(a: Quat, b: Quat) -> Quat {
    Quat {
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}

pub fn conjugate_quat(q: Quat) -> Quat {
    Quat { x: -q.x, y: -q.y, z: -q.z, w: q.w }
}

pub fn nlerp_quat(a: Quat, b: Quat, t: f32) -> Quat {
    let (mut bx, mut by, mut bz, mut bw) = (b.x, b.y, b.z, b.w);
    let dot = a.x * bx + a.y * by + a.z * bz + a.w * bw;
    if dot < 0.0 {
        bx = -bx;
        by = -by;
        bz = -bz;
        bw = -bw;
    }
    let mut out = Quat { x: a.x + (bx - a.x) * t, y: a.y + (by - a.y) * t, z: a.z + (bz - a.z) * t, w: a.w + (bw - a.w) * t };
    let norm = sqrt(out.x * out.x + out.y * out.y + out.z * out.z + out.w * out.w);
    if norm > 1e-8 {
        let inv = 1.0 / norm;
        out.x *= inv;
        out.y *= inv;
        out.z *= inv;
        out.w *= inv;
    } else {
        out = a;
    }
    out
}

pub fn rotate_vector(q: Quat, v: Vec3) -> Vec3 {
    let pure = Quat { x: v.x, y: v.y, z: v.z, w: 0.0 };
    let r = multiply_quat(multiply_quat(q, pure), conjugate_quat(q));
    Vec3 { x: r.x, y: r.y, z: r.z }
}
