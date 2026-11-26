//! The external view (formerly include/Camera/ExternalViewMath.hpp and ExternalViewLease.hpp): a detached camera the
//! director selects as MAIN -- its fixed-point pose, the head composed onto it, the second eye beside it, a blend of
//! several, and the lease that hands a completed MAIN to the second eye's camera.
use cvr_body::cmath::{atan2, cos, hypot, sin};
use cvr_body::xr::{multiply_quat, rotate_vector, Quat, Vec3};

/// The engine's camera pose: 17-bit fixed-point position and its rotation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExternalViewPose {
    pub position: [i32; 3],
    pub rotation: Quat,
}
impl Default for ExternalViewPose {
    fn default() -> Self {
        ExternalViewPose { position: [0; 3], rotation: Quat::IDENTITY }
    }
}

/// The unit direction from `origin` to `target`, in double precision.
pub fn aim_at_point(origin: Vec3, target: Vec3) -> Option<Vec3> {
    let (x, y, z) = (target.x as f64 - origin.x as f64, target.y as f64 - origin.y as f64, target.z as f64 - origin.z as f64);
    let length = (x * x + y * y + z * z).sqrt();
    if !length.is_finite() || length < 1e-6 {
        return None;
    }
    Some(Vec3 { x: (x / length) as f32, y: (y / length) as f32, z: (z / length) as f32 })
}
pub fn normalize_external(q: &mut Quat) -> bool {
    let n = q.x as f64 * q.x as f64 + q.y as f64 * q.y as f64 + q.z as f64 * q.z as f64 + q.w as f64 * q.w as f64;
    if !n.is_finite() || n < 0.5 || n > 1.5 {
        return false;
    }
    let s = (1.0 / n.sqrt()) as f32;
    *q = Quat { x: q.x * s, y: q.y * s, z: q.z * s, w: q.w * s };
    true
}
/// The camera's forward (+Y) laid flat, unit length.
pub fn planar_external_forward(mut q: Quat) -> Option<Vec3> {
    if !normalize_external(&mut q) {
        return None;
    }
    let forward = rotate_vector(q, Vec3 { x: 0.0, y: 1.0, z: 0.0 });
    let length = hypot(forward.x, forward.y);
    if !length.is_finite() || length < 0.05 {
        return None;
    }
    Some(Vec3 { x: forward.x / length, y: forward.y / length, z: 0.0 })
}
/// Moves the pose by `delta` metres, rounded to the fixed point; false (and the pose untouched) out of range.
pub fn shift_external(pose: &mut ExternalViewPose, delta: Vec3) -> bool {
    let mut position = pose.position;
    for (i, v) in [delta.x, delta.y, delta.z].into_iter().enumerate() {
        if !v.is_finite() {
            return false;
        }
        let shifted = position[i] as f64 + (v as f64 * 131072.0).round();
        if shifted < i32::MIN as f64 || shifted > i32::MAX as f64 {
            return false;
        }
        position[i] = shifted as i32;
    }
    pose.position = position;
    true
}
/// The eye `offset` metres along the camera's right.
pub fn external_eye(pose: &mut ExternalViewPose, offset: f32) -> bool {
    if !offset.is_finite() || offset.abs() > 1.0 {
        return false;
    }
    let right = rotate_vector(pose.rotation, Vec3 { x: 1.0, y: 0.0, z: 0.0 });
    shift_external(pose, Vec3 { x: right.x * offset, y: right.y * offset, z: right.z * offset })
}
/// The head (OpenXR axes, `head_position` / `head_orientation`) composed onto the external camera: the head's
/// translation turned by the camera's yaw and scaled, its rotation after the camera's, then MAIN's eye offset.
pub fn compose_external(base: &ExternalViewPose, head_orientation: Quat, head_position: Vec3, scale: f32, main_eye_offset: f32) -> Option<ExternalViewPose> {
    let mut result = *base;
    let mut h = head_orientation;
    if !normalize_external(&mut result.rotation) || !normalize_external(&mut h) || !scale.is_finite() || scale <= 0.0 || scale > 100.0 {
        return None;
    }
    let q = result.rotation;
    let yaw = atan2(2.0 * (q.w * q.z + q.x * q.y), 1.0 - 2.0 * (q.y * q.y + q.z * q.z));
    let (c, s) = (cos(yaw), sin(yaw));
    let delta = Vec3 {
        x: (c * head_position.x + s * head_position.z) * scale,
        y: (s * head_position.x - c * head_position.z) * scale,
        z: head_position.y * scale,
    };
    result.rotation = multiply_quat(result.rotation, Quat { x: h.x, y: -h.z, z: h.y, w: h.w });
    if !normalize_external(&mut result.rotation) || !shift_external(&mut result, delta) || !external_eye(&mut result, main_eye_offset) {
        return None;
    }
    Some(result)
}

#[derive(Clone, Copy, Debug)]
pub struct ExternalViewInput {
    pub pose: ExternalViewPose,
    pub weight: f32,
}
/// The director's weighted blend: positions averaged, rotations summed on the first one's hemisphere and normalised.
pub fn blend_external(input: &[ExternalViewInput]) -> Option<ExternalViewPose> {
    if input.is_empty() {
        return None;
    }
    let mut sum = 0.0f64;
    let mut position = [0.0f64; 3];
    let mut rotation = Quat::ZERO;
    let mut reference = Quat::ZERO;
    let mut have = false;
    for value in input {
        if !value.weight.is_finite() || value.weight < 0.0 {
            return None;
        }
        if value.weight == 0.0 {
            continue;
        }
        let mut q = value.pose.rotation;
        if !normalize_external(&mut q) {
            return None;
        }
        if !have {
            reference = q;
            have = true;
        }
        let sign = if q.x * reference.x + q.y * reference.y + q.z * reference.z + q.w * reference.w < 0.0 { -1.0f32 } else { 1.0 };
        let weight = if input.len() == 1 { 1.0f32 } else { value.weight };
        sum += weight as f64;
        for (i, p) in position.iter_mut().enumerate() {
            *p += value.pose.position[i] as f64 * weight as f64;
        }
        rotation.x += q.x * weight * sign;
        rotation.y += q.y * weight * sign;
        rotation.z += q.z * weight * sign;
        rotation.w += q.w * weight * sign;
    }
    if !have || (sum - 1.0).abs() > 1e-5 {
        return None;
    }
    let norm = rotation.x as f64 * rotation.x as f64
        + rotation.y as f64 * rotation.y as f64
        + rotation.z as f64 * rotation.z as f64
        + rotation.w as f64 * rotation.w as f64;
    if !norm.is_finite() || norm < 1e-8 {
        return None;
    }
    let inverse = (1.0 / norm.sqrt()) as f32;
    let mut result = ExternalViewPose {
        position: [0; 3],
        rotation: Quat { x: rotation.x * inverse, y: rotation.y * inverse, z: rotation.z * inverse, w: rotation.w * inverse },
    };
    for (i, p) in position.iter().enumerate() {
        if *p < i32::MIN as f64 || *p > i32::MAX as f64 {
            return None;
        }
        result.position[i] = p.round() as i64 as i32;
    }
    Some(result)
}

/// Whose external view a lease is: MAIN's placed camera, the second eye's camera, the tracking origin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExternalViewKey {
    pub player_camera: usize,
    pub eye_camera: usize,
    pub origin: u64,
}
/// MAIN owns the lifetime, exactly like the device-camera handoff. A missing update never selects the player's cabin
/// camera. A value snapshot, not a retained engine camera pointer: explicit FPP/device transitions clear it, owner or
/// origin changes reject it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExternalViewLease<T> {
    pub value: T,
    pub key: ExternalViewKey,
    pub stamp: u64,
}
impl<T: Copy> ExternalViewLease<T> {
    pub fn clear(&mut self) {
        self.stamp = 0;
    }
    pub fn publish(&mut self, next: T, owner: ExternalViewKey, now: u64) {
        self.value = next;
        self.key = owner;
        self.stamp = now;
    }
    pub fn read(&self, owner: ExternalViewKey, now: u64) -> Option<T> {
        if self.stamp == 0 || owner != self.key || now < self.stamp {
            return None;
        }
        Some(self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eye_shift_is_along_the_right() {
        let mut p = ExternalViewPose::default();
        assert!(external_eye(&mut p, 0.032));
        assert_eq!(p.position, [(0.032f64 * 131072.0).round() as i32, 0, 0]);
        assert!(!external_eye(&mut p, 2.0), "an offset past a metre is refused");
    }
    #[test]
    fn blend_of_one_is_itself() {
        let a = ExternalViewPose { position: [10, -20, 30], rotation: Quat::IDENTITY };
        let b = blend_external(&[ExternalViewInput { pose: a, weight: 0.7 }]).unwrap();
        assert_eq!(b.position, a.position);
        assert!(blend_external(&[ExternalViewInput { pose: a, weight: 0.5 }, ExternalViewInput { pose: a, weight: 0.4 }]).is_none());
    }
    #[test]
    fn lease_rejects_another_owner() {
        let mut l = ExternalViewLease::<u32>::default();
        let k = ExternalViewKey { player_camera: 1, eye_camera: 2, origin: 3 };
        l.publish(7, k, 100);
        assert_eq!(l.read(k, 150), Some(7));
        assert_eq!(l.read(ExternalViewKey { origin: 4, ..k }, 150), None);
        assert_eq!(l.read(k, 50), None, "a stamp from the future");
        l.clear();
        assert_eq!(l.read(k, 150), None);
    }
}
