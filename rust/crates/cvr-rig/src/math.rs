//! The solver's vector and quaternion arithmetic (formerly the VRIK_Quat*/VRIK_*3 helpers of
//! src/Anim/CharacterRig.cpp), operation for operation. Quaternions are i, j, k, r == x, y, z, w.
use cvr_body::cmath::{acos, cos, sin, sqrt};

pub type V3 = [f32; 3];
pub type Q = [f32; 4];

pub const IDENTITY: Q = [0.0, 0.0, 0.0, 1.0];

/// o = q * v * q^-1.
pub fn quat_rotate_vec(q: &Q, v: &V3) -> V3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [
        v[0] + q[3] * tx + (q[1] * tz - q[2] * ty),
        v[1] + q[3] * ty + (q[2] * tx - q[0] * tz),
        v[2] + q[3] * tz + (q[0] * ty - q[1] * tx),
    ]
}

/// Hamilton product a * b.
pub fn quat_mul(a: &Q, b: &Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn quat_conj(q: &Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}

pub fn quat_norm(q: &mut Q) {
    let n = sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    if n > 1e-8 {
        let inv = 1.0 / n;
        q[0] *= inv;
        q[1] *= inv;
        q[2] *= inv;
        q[3] *= inv;
    } else {
        *q = IDENTITY;
    }
}

pub fn dot3(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross3(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Normalises in place (when longer than 1e-8) and returns the length it had.
pub fn norm3(v: &mut V3) -> f32 {
    let n = sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    if n > 1e-8 {
        let inv = 1.0 / n;
        v[0] *= inv;
        v[1] *= inv;
        v[2] *= inv;
    }
    n
}

pub fn dist3(a: &V3, b: &V3) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    sqrt(dx * dx + dy * dy + dz * dz)
}

/// slerp(identity, q, t): a fraction t of the rotation q (a spine bend spread over several bones).
pub fn quat_scale(q: &Q, t: f32) -> Q {
    let mut qq = *q;
    if qq[3] < 0.0 {
        qq = [-qq[0], -qq[1], -qq[2], -qq[3]];
    }
    let s = sqrt(qq[0] * qq[0] + qq[1] * qq[1] + qq[2] * qq[2]);
    if s < 1e-6 {
        return IDENTITY;
    }
    let w = if qq[3] > 1.0 { 1.0 } else { qq[3] };
    let half = acos(w) * t;
    let (sn, cn) = (sin(half), cos(half));
    [qq[0] / s * sn, qq[1] / s * sn, qq[2] / s * sn, cn]
}

/// Shortest-arc unit quaternion rotating unit vector a onto unit vector b.
pub fn quat_from_to(a: &V3, b: &V3) -> Q {
    let d = dot3(a, b);
    if d >= 1.0 - 1e-6 {
        return IDENTITY;
    }
    if d <= -1.0 + 1e-6 {
        // Antiparallel: 180 degrees about any axis orthogonal to a.
        let ax = if a[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let mut axis = cross3(a, &ax);
        norm3(&mut axis);
        return [axis[0], axis[1], axis[2], 0.0];
    }
    let c = cross3(a, b);
    let mut o = [c[0], c[1], c[2], 1.0 + d];
    quat_norm(&mut o);
    o
}
