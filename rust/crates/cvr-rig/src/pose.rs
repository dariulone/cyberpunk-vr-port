//! The engine's pose buffer and forward kinematics (formerly VRIK_ComputeFK, VRIK_WriteLocalRot/Pos and the
//! QsTransform offsets of src/Anim/CharacterRig.cpp). A bone is a 48-byte QsTransform: translation (Vector4) at +0,
//! rotation (x y z w) at +16, scale at +32. Parents precede children, so a prefix of the rig is self-contained.
use crate::math::{quat_conj, quat_mul, quat_norm, quat_rotate_vec, Q, V3};
use cvr_body::cmath::sqrt;

pub const BONE_STRIDE: usize = 48;
pub const TRANS_OFF: usize = 0;
pub const ROT_OFF: usize = 16;

/// A pose buffer the engine owns: `count` bones of 48 bytes. Every accessor is unchecked pointer arithmetic, as the
/// C++ was: the caller holds the player pose lock and has validated the buffer's size against the rig.
#[derive(Clone, Copy)]
pub struct PoseBuffer(pub *mut u8);

impl PoseBuffer {
    /// # Safety
    /// `bone` must be within the buffer.
    pub unsafe fn trans(self, bone: usize) -> *mut [f32; 3] {
        unsafe { self.0.add(bone * BONE_STRIDE + TRANS_OFF) as *mut [f32; 3] }
    }
    /// # Safety
    /// `bone` must be within the buffer.
    pub unsafe fn rot(self, bone: usize) -> *mut [f32; 4] {
        unsafe { self.0.add(bone * BONE_STRIDE + ROT_OFF) as *mut [f32; 4] }
    }
    /// # Safety
    /// `bone` must be within the buffer.
    pub unsafe fn local(self, bone: usize) -> *mut [f32; 12] {
        unsafe { self.0.add(bone * BONE_STRIDE) as *mut [f32; 12] }
    }
}

/// The FK pass's own normalisation: only when the squared norm is off by more than 2e-6 (a product of unit
/// quaternions is unit to float precision); a degenerate one becomes the identity.
#[inline]
pub fn normalise_if_needed(q: &mut Q) {
    let n2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if n2 > 1.0 - 2e-6 && n2 < 1.0 + 2e-6 {
        return;
    }
    if n2 > 1e-16 {
        let inv = 1.0 / sqrt(n2);
        q[0] *= inv;
        q[1] *= inv;
        q[2] *= inv;
        q[3] *= inv;
    } else {
        *q = [0.0, 0.0, 0.0, 1.0];
    }
}

/// Accumulates parent-local transforms into model space for the first `count` bones.
///
/// # Safety
/// `buffer` holds at least `count` bones; `parents`, `pos` and `rot` at least `count` entries.
pub unsafe fn compute_fk(buffer: PoseBuffer, count: usize, parents: &[i16], pos: &mut [V3], rot: &mut [Q]) {
    for i in 0..count {
        let l = unsafe { &*buffer.trans(i) };
        let b = unsafe { &*buffer.rot(i) };
        let (lx, ly, lz) = (l[0], l[1], l[2]);
        let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
        let p = parents[i] as i32;
        if p >= 0 && (p as usize) < i {
            let p = p as usize;
            let [ax, ay, az, aw] = rot[p];
            let mut r = [
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            ];
            normalise_if_needed(&mut r);
            rot[i] = r;
            let tx = 2.0 * (ay * lz - az * ly);
            let ty = 2.0 * (az * lx - ax * lz);
            let tz = 2.0 * (ax * ly - ay * lx);
            let pp = pos[p];
            pos[i] = [
                pp[0] + lx + aw * tx + (ay * tz - az * ty),
                pp[1] + ly + aw * ty + (az * tx - ax * tz),
                pp[2] + lz + aw * tz + (ax * ty - ay * tx),
            ];
        } else {
            let mut r = [bx, by, bz, bw];
            normalise_if_needed(&mut r);
            rot[i] = r;
            pos[i] = [lx, ly, lz];
        }
    }
}

/// A model-space rotation written back as the bone's local one, given its parent's model rotation.
///
/// # Safety
/// `bone` within the buffer.
pub unsafe fn write_local_rot(buffer: PoseBuffer, bone: usize, parent_model_rot: &Q, model_rot: &Q) {
    let mut local = quat_mul(&quat_conj(parent_model_rot), model_rot);
    quat_norm(&mut local);
    unsafe { *buffer.rot(bone) = local };
}

/// A model-space position written back as the bone's local translation.
///
/// # Safety
/// `bone` within the buffer.
pub unsafe fn write_local_pos(buffer: PoseBuffer, bone: usize, parent_model_pos: &V3, parent_model_rot: &Q, model_pos: &V3) {
    let delta = [model_pos[0] - parent_model_pos[0], model_pos[1] - parent_model_pos[1], model_pos[2] - parent_model_pos[2]];
    let local = quat_rotate_vec(&quat_conj(parent_model_rot), &delta);
    unsafe { *buffer.trans(bone) = local };
}
