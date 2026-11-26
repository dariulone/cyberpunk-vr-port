//! The rig: its policy and reference pose (VRIK_ConfigureRigPolicy), the reference readers, the calibration hold,
//! forward kinematics and the local writes, and the quaternion helpers the C++ half calls by name.
use super::env::*;
use super::state::*;
use super::cyberware::VRIK_ConfigureCyberwareRig;
use super::snapshot::VRIK_RemapAxis;
use super::types::*;
use super::*;
use crate::ladder;
use crate::math::{cross3, dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec};
use crate::skeleton::{equal_bone_name, forearm_twist_slot, is_primary_spine, is_rig_control, spine_flex, starts_bone_name};
use crate::skeleton::{CHEST_RETRACTION, NECK_FLEX, UPPER_NECK_FLEX};
use cvr_body::cmath::{cos, sin, sqrt};
use cvr_camera::anchor::{av, neck_camera_eye_offset, neck_camera_offset, AnchorVector};
use core::ffi::{c_char, CStr};
use std::fmt::Write as _;

// ---- the helpers, by name, for the C++ half (Anim/VrikHook.hpp, Anim/CharacterRig.hpp). Pointer for pointer as the
// ---- C++ wrote them: an output that is also an input is read and written in the same order.

#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatRotateVec(q: *const f32, v: *const f32, o: *mut f32) {
    let tx = 2.0 * (*q.add(1) * *v.add(2) - *q.add(2) * *v.add(1));
    let ty = 2.0 * (*q.add(2) * *v - *q * *v.add(2));
    let tz = 2.0 * (*q * *v.add(1) - *q.add(1) * *v);
    *o = *v + *q.add(3) * tx + (*q.add(1) * tz - *q.add(2) * ty);
    *o.add(1) = *v.add(1) + *q.add(3) * ty + (*q.add(2) * tx - *q * tz);
    *o.add(2) = *v.add(2) + *q.add(3) * tz + (*q * ty - *q.add(1) * tx);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatMul(a: *const f32, b: *const f32, o: *mut f32) {
    *o = *a.add(3) * *b + *a * *b.add(3) + *a.add(1) * *b.add(2) - *a.add(2) * *b.add(1);
    *o.add(1) = *a.add(3) * *b.add(1) - *a * *b.add(2) + *a.add(1) * *b.add(3) + *a.add(2) * *b;
    *o.add(2) = *a.add(3) * *b.add(2) + *a * *b.add(1) - *a.add(1) * *b + *a.add(2) * *b.add(3);
    *o.add(3) = *a.add(3) * *b.add(3) - *a * *b - *a.add(1) * *b.add(1) - *a.add(2) * *b.add(2);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatConj(q: *const f32, o: *mut f32) {
    *o = -*q;
    *o.add(1) = -*q.add(1);
    *o.add(2) = -*q.add(2);
    *o.add(3) = *q.add(3);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatNorm(q: *mut f32) {
    let mut v = a4(q);
    quat_norm(&mut v);
    put4(q, &v);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_Dot3(a: *const f32, b: *const f32) -> f32 {
    *a * *b + *a.add(1) * *b.add(1) + *a.add(2) * *b.add(2)
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_Cross3(a: *const f32, b: *const f32, o: *mut f32) {
    *o = *a.add(1) * *b.add(2) - *a.add(2) * *b.add(1);
    *o.add(1) = *a.add(2) * *b - *a * *b.add(2);
    *o.add(2) = *a * *b.add(1) - *a.add(1) * *b;
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_Norm3(v: *mut f32) -> f32 {
    let mut x = a3(v);
    let n = norm3(&mut x);
    put3(v, &x);
    n
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatScale(q: *const f32, t: f32, o: *mut f32) {
    let r = crate::math::quat_scale(&a4(q), t);
    put4(o, &r);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_QuatFromTo(a: *const f32, b: *const f32, o: *mut f32) {
    let r = quat_from_to(&a3(a), &a3(b));
    put4(o, &r);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_NoteHandAim(pin: i32, target: *const f32, rotation: *const f32) {
    if !(0..=1).contains(&pin) {
        return;
    }
    let aim = &mut g_VRIKHandAim[pin as usize];
    aim[7] = 0.0;
    for k in 0..3 {
        aim[k] = *target.add(k);
    }
    for k in 0..4 {
        aim[3 + k] = *rotation.add(k);
    }
    aim[7] = 1.0;
}

// ---- forward kinematics and the local writes

/// FK walk length: only the prefix of the rig the solver reads (parents precede children); the full bone count
/// until the resolve publishes the prefix.
#[no_mangle]
pub unsafe extern "C" fn VRIK_FKCount() -> i32 {
    let n = g_VRFKCount;
    if n > 0 {
        n
    } else {
        g_VRBoneCount
    }
}

/// The FK pass's own normalisation: only when the squared norm is off by more than 2e-6.
#[inline(always)]
fn normalise_if_needed(q: &mut Q) {
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
        *q = ID;
    }
}

/// Accumulates parent-local transforms into model space (g_fkPos, g_fkRot). Requires parent < child.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ComputeFK(boneBuf: *mut u8, count: i32) {
    let count = if count > MAXB { MAXB } else { count };
    for i in 0..count {
        let lt = bt(boneBuf, i);
        let lr = br(boneBuf, i);
        let (lx, ly, lz) = (*lt, *lt.add(1), *lt.add(2));
        let (bx, by, bz, bw) = (*lr, *lr.add(1), *lr.add(2), *lr.add(3));
        let iu = i as usize;
        let p = g_VRBoneParent[iu] as i32;
        if p >= 0 && p < i {
            let pu = p as usize;
            let [ax, ay, az, aw] = g_fkRot[pu];
            let mut r = [
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            ];
            normalise_if_needed(&mut r);
            g_fkRot[iu] = r;
            let tx = 2.0 * (ay * lz - az * ly);
            let ty = 2.0 * (az * lx - ax * lz);
            let tz = 2.0 * (ax * ly - ay * lx);
            let pp = g_fkPos[pu];
            g_fkPos[iu] = [
                pp[0] + lx + aw * tx + (ay * tz - az * ty),
                pp[1] + ly + aw * ty + (az * tx - ax * tz),
                pp[2] + lz + aw * tz + (ax * ty - ay * tx),
            ];
        } else {
            let mut r = [bx, by, bz, bw];
            normalise_if_needed(&mut r);
            g_fkRot[iu] = r;
            g_fkPos[iu] = [lx, ly, lz];
        }
    }
}

/// A model-space rotation written back as the bone's local one: parentModel^-1 * modelRot.
pub unsafe fn write_local_rot(boneBuf: *mut u8, idx: i32, parent_model_rot: &Q, model_rot: &Q) {
    let p_inv = quat_conj(parent_model_rot);
    let mut local = quat_mul(&p_inv, model_rot);
    quat_norm(&mut local);
    put4(br(boneBuf, idx), &local);
}
pub unsafe fn write_local_pos(boneBuf: *mut u8, idx: i32, parent_model_pos: &V3, parent_model_rot: &Q, model_pos: &V3) {
    let delta = [model_pos[0] - parent_model_pos[0], model_pos[1] - parent_model_pos[1], model_pos[2] - parent_model_pos[2]];
    let p_inv = quat_conj(parent_model_rot);
    let local = quat_rotate_vec(&p_inv, &delta);
    put3(bt(boneBuf, idx), &local);
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_WriteLocalRot(boneBuf: *mut u8, idx: i32, parentModelRot: *const f32, modelRot: *const f32) {
    write_local_rot(boneBuf, idx, &a4(parentModelRot), &a4(modelRot));
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_WriteLocalPos(
    boneBuf: *mut u8,
    idx: i32,
    parentModelPos: *const f32,
    parentModelRot: *const f32,
    modelPos: *const f32,
) {
    write_local_pos(boneBuf, idx, &a3(parentModelPos), &a4(parentModelRot), &a3(modelPos));
}
/// The parent's model rotation, or the identity for a root (`parent>=0 ? g_fkRot[parent] : id`).
#[inline(always)]
pub unsafe fn parent_rot(parent: i32) -> Q {
    if parent >= 0 {
        g_fkRot[parent as usize]
    } else {
        ID
    }
}
#[inline(always)]
pub unsafe fn parent_pos(parent: i32) -> V3 {
    if parent >= 0 {
        g_fkPos[parent as usize]
    } else {
        ZERO3
    }
}

// ---- small shared pieces

/// A bone's twelve local floats equal `value` to 1e-6, all finite: what we left in this buffer.
pub unsafe fn same_local(buffer: *mut u8, bone: i32, value: &[f32; 12]) -> bool {
    let local = &*bone12(buffer, bone);
    for k in 0..12 {
        if !local[k].is_finite() || (local[k] - value[k]).abs() > 1e-6 {
            return false;
        }
    }
    true
}

/// The seated lower body (legs, their shadows, the pelvis and its shadow) as the animation had it.
pub unsafe fn save_vehicle_lower(buffer: *mut u8) -> *mut BendLegRestore {
    let mut bones = [0i32; 14];
    bones[..6].copy_from_slice(&[g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, g_VRLeftUpLegIdx, g_VRLeftLegIdx, g_VRLeftFootIdx]);
    let mut count = 6usize;
    let mut bone = 0;
    while bone < VRIK_FKCount() && count < 12 {
        if s_bendLegShadowSource[bone as usize] >= 0 {
            bones[count] = bone;
            count += 1;
        }
        bone += 1;
    }
    bones[count] = g_VRHipsIdx;
    count += 1;
    let mut bone = 0;
    while bone < VRIK_FKCount() && count < 14 {
        if g_VRShadowSource[bone as usize] == g_VRHipsIdx {
            bones[count] = bone;
            count += 1;
        }
        bone += 1;
    }
    let saved = restore_slot(buffer);
    let s = &mut *saved;
    let previous = s.buffer == buffer && s.revision == s_rigRevision;
    for i in 0..count {
        if !previous || i as i32 >= s.count || s.bones[i] != bones[i] || !same_local(buffer, bones[i], &s.solved[i]) {
            copy_bone(&mut s.native[i], buffer, bones[i]);
        }
        s.bones[i] = bones[i];
    }
    s.buffer = buffer;
    s.revision = s_rigRevision;
    s.count = count as i32;
    s.vehicle = true;
    saved
}

/// The seated upper body's bones: the spine, the neck and head, each arm with its girdle and twists, and their shadows.
pub unsafe fn seated_bones(bones: &mut [i32; 64]) -> usize {
    let mut included = [false; MAX_BONES];
    let mut count = 0usize;
    fn put(included: &mut [bool; MAX_BONES], count: &mut usize, bones: &mut [i32; 64], bone: i32, limit: i32) {
        if bone < 0 || bone >= limit || included[bone as usize] || *count >= 64 {
            return;
        }
        included[bone as usize] = true;
        bones[*count] = bone;
        *count += 1;
    }
    let limit = VRIK_FKCount();
    let mut add = |bone: i32, bones: &mut [i32; 64], included: &mut [bool; MAX_BONES]| put(included, &mut count, bones, bone, limit);
    for i in 0..g_VRSpineCount.clamp(0, 8) {
        add(g_VRSpineIdx[i as usize], bones, &mut included);
    }
    add(g_VRNeckIdx, bones, &mut included);
    add(g_VRNeck1Idx, bones, &mut included);
    add(g_VRHeadBoneIdx, bones, &mut included);
    for bone in [g_VRRightUpperArmIdx, g_VRLeftUpperArmIdx] {
        if bone >= 0 && bone < VRIK_FKCount() {
            add(g_VRBoneParent[bone as usize] as i32, bones, &mut included);
        }
        add(bone, bones, &mut included);
    }
    add(g_VRRightForeArmIdx, bones, &mut included);
    add(g_VRLeftForeArmIdx, bones, &mut included);
    add(g_VRRightBoneIdx, bones, &mut included);
    add(g_VRLeftBoneIdx, bones, &mut included);
    for i in 0..3 {
        add(g_VRForeTwistR[i], bones, &mut included);
        add(g_VRForeTwistL[i], bones, &mut included);
    }
    for bone in 0..VRIK_FKCount() {
        let source = g_VRShadowSource[bone as usize];
        if source >= 0 && (source as usize) < MAX_BONES && included[source as usize] {
            add(bone, bones, &mut included);
        }
    }
    count
}
/// The seated pose as the animation left it in this buffer -- kept from the last pass where the bone still holds our
/// own output.
pub unsafe fn save_seated_pose(buffer: *mut u8) -> *mut SeatedPoseRestore {
    let mut bones = [0i32; 64];
    let count = seated_bones(&mut bones);
    let mut slot = None;
    for (i, entry) in s_seatedRestore.iter().enumerate() {
        if entry.buffer == buffer {
            slot = Some(i);
            break;
        }
    }
    let i = slot.unwrap_or_else(|| {
        let i = (s_seatedRestoreCursor % 8) as usize;
        s_seatedRestoreCursor = s_seatedRestoreCursor.wrapping_add(1);
        i
    });
    let saved = &mut s_seatedRestore[i];
    let previous = saved.buffer == buffer && saved.revision == s_rigRevision;
    for k in 0..count {
        if !previous || k as i32 >= saved.count || saved.bones[k] != bones[k] || !same_local(buffer, bones[k], &saved.solved[k]) {
            copy_bone(&mut saved.native[k], buffer, bones[k]);
        }
        saved.bones[k] = bones[k];
    }
    (saved.buffer, saved.revision, saved.count) = (buffer, s_rigRevision, count as i32);
    saved
}
/// What we left in the seated bones.
pub unsafe fn note_seated_pose(saved: *mut SeatedPoseRestore, buffer: *const u8) {
    let Some(saved) = saved.as_mut() else { return };
    for k in 0..saved.count.max(0) as usize {
        copy_bone(&mut saved.solved[k], buffer as *mut u8, saved.bones[k]);
    }
}

/// This buffer's restore entry, or the next one round the ring.
pub unsafe fn restore_slot(buffer: *mut u8) -> *mut BendLegRestore {
    for entry in s_bendLegRestore.iter_mut() {
        if entry.buffer == buffer {
            return entry;
        }
    }
    let slot = (s_bendRestoreCursor % 8) as usize;
    s_bendRestoreCursor = s_bendRestoreCursor.wrapping_add(1);
    &mut s_bendLegRestore[slot]
}

/// memcpy(dst, boneBuf + bone*48, 48).
#[inline(always)]
pub unsafe fn copy_bone(dst: &mut [f32; 12], buffer: *mut u8, bone: i32) {
    *dst = *bone12(buffer, bone);
}
/// memcpy(boneBuf + bone*48, src, 48).
#[inline(always)]
pub unsafe fn restore_bone(buffer: *mut u8, bone: i32, src: &[f32; 12]) {
    *bone12(buffer, bone) = *src;
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_UsesNativeCyberware() -> bool {
    CyberpunkVR_NativeCyberwarePasses != 0 && g_VRCyberwareFamily != 0
}

// ---- the reference readers

#[no_mangle]
pub unsafe extern "C" fn cvr_vrik_read_neck_camera_mount(offset: *mut AnchorVector) -> bool {
    let _lock = NECK_MOUNT_LOCK.lock();
    if offset.is_null() || !s_neckMountValid {
        return false;
    }
    *offset = s_neckMount;
    true
}
#[no_mangle]
pub unsafe extern "C" fn cvr_vrik_read_neck_camera_eye_offset(offset: *mut AnchorVector) -> bool {
    let _lock = NECK_MOUNT_LOCK.lock();
    if offset.is_null() || !s_eyeReferenceValid {
        return false;
    }
    *offset = s_neckCameraEyeOffset;
    true
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_FbtRigPath(female: bool, out: *mut c_char, size: usize) -> bool {
    let _lock = NECK_MOUNT_LOCK.lock();
    let p = &s_fbtRigPath[if female { 1 } else { 0 }];
    if out.is_null() || size == 0 || p[0] == 0 {
        return false;
    }
    let len = p.iter().position(|&c| c == 0).unwrap_or(p.len());
    if len + 1 > size {
        // strcpy_s: the destination too small is the invalid-parameter path; the caller gets an empty string
        *out = 0;
        return true;
    }
    core::ptr::copy_nonoverlapping(p.as_ptr() as *const c_char, out, len);
    *out.add(len) = 0;
    true
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReadFbtReference(out: *mut VrikFbtReference) -> bool {
    let _lock = NECK_MOUNT_LOCK.lock();
    if out.is_null() || !s_fbtReference.valid {
        return false;
    }
    *out = s_fbtReference;
    true
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReferenceFingerRotation(bone: i32, rotation: *mut f32) -> bool {
    if rotation.is_null() || bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
        return false;
    }
    put4(rotation, &s_referenceLocalRot[bone as usize]);
    true
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReferenceLocal(bone: i32, position: *mut f32, rotation: *mut f32) -> bool {
    if position.is_null() || rotation.is_null() || bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
        return false;
    }
    put3(position, &s_referenceLocalPos[bone as usize]);
    put4(rotation, &s_referenceLocalRot[bone as usize]);
    true
}
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReferenceModel(bone: i32, position: *mut f32, rotation: *mut f32) -> bool {
    if position.is_null() || rotation.is_null() || bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
        return false;
    }
    put3(position, &s_referenceModelPos[bone as usize]);
    put4(rotation, &s_referenceModelRot[bone as usize]);
    true
}
/// The ladder profile's finger bones of a hand: 0 left, 1 right; -1 when the rig has none.
#[no_mangle]
pub unsafe extern "C" fn VRIK_LadderFingerBone(side: i32, slot: i32) -> i32 {
    if !(0..=1).contains(&side) || !(0..19).contains(&slot) {
        return -1;
    }
    s_ladderFingers[side as usize][slot as usize]
}
#[inline(always)]
unsafe fn ref_ok(bone: i32) -> bool {
    bone >= 0 && bone < g_VRBoneCount && s_referenceValid[bone as usize]
}

/// cvr::gestures::ThumbReference (Anim/HandGestures.hpp).
#[no_mangle]
pub unsafe extern "C" fn cvr_vrik_thumb_reference(side: i32, rotation: *mut f32) -> bool {
    if !(0..=1).contains(&side) || rotation.is_null() {
        return false;
    }
    let f = &s_ladderFingers[side as usize];
    let (root, tip, index, index_tip) = (f[0], f[10], f[1], f[15]);
    for bone in [root, tip, index, index_tip] {
        if !ref_ok(bone) {
            return false;
        }
    }
    let parent = g_VRBoneParent[root as usize] as i32;
    if parent < 0 || !s_referenceValid[parent as usize] {
        return false;
    }
    let p = &s_referenceModelPos;
    let from = sub3(&p[tip as usize], &p[root as usize]);
    let to = sub3(&p[index_tip as usize], &p[index as usize]);
    let inverse = quat_conj(&s_referenceModelRot[parent as usize]);
    let mut a = quat_rotate_vec(&inverse, &from);
    let mut b = quat_rotate_vec(&inverse, &to);
    if dot3(&a, &a) < 1e-8 || dot3(&b, &b) < 1e-8 {
        return false;
    }
    norm3(&mut a);
    norm3(&mut b);
    let delta = quat_from_to(&a, &b);
    let mut r = quat_mul(&delta, &s_referenceLocalRot[root as usize]);
    quat_norm(&mut r);
    put4(rotation, &r);
    true
}

/// cvr::gestures::SpreadAxis (Anim/HandGestures.hpp).
#[no_mangle]
pub unsafe extern "C" fn cvr_vrik_spread_axis(side: i32, digit: i32, bone: *mut i32, axis: *mut f32) -> bool {
    if !(0..=1).contains(&side) || bone.is_null() || axis.is_null() || !(0..=4).contains(&digit) || digit == 2 {
        return false;
    }
    let slot = &s_ladderFingers[side as usize];
    let d = digit as usize;
    let root = if digit == 0 { slot[0] } else { slot[5 + d] };
    let next = if digit == 0 { slot[10] } else { slot[10 + d] };
    let ref_from = if digit == 0 { slot[1] } else { slot[7] };
    let ref_to = if digit == 0 { slot[15] } else { slot[12] };
    let (index_knuckle, little_knuckle, palm) = (slot[6], slot[9], slot[2]);
    for b in [root, next, ref_from, ref_to, index_knuckle, little_knuckle, palm] {
        if !ref_ok(b) {
            return false;
        }
    }
    let parent = g_VRBoneParent[root as usize] as i32;
    if parent < 0 || !s_referenceValid[parent as usize] {
        return false;
    }
    let p = &s_referenceModelPos;
    let dir = sub3(&p[next as usize], &p[root as usize]);
    let rf = sub3(&p[ref_to as usize], &p[ref_from as usize]);
    let across = sub3(&p[little_knuckle as usize], &p[index_knuckle as usize]);
    let to_palm = sub3(&p[palm as usize], &p[index_knuckle as usize]);
    let mut normal = [
        across[1] * to_palm[2] - across[2] * to_palm[1],
        across[2] * to_palm[0] - across[0] * to_palm[2],
        across[0] * to_palm[1] - across[1] * to_palm[0],
    ];
    let turn = [rf[1] * dir[2] - rf[2] * dir[1], rf[2] * dir[0] - rf[0] * dir[2], rf[0] * dir[1] - rf[1] * dir[0]];
    if dot3(&normal, &normal) < 1e-12 {
        return false;
    }
    norm3(&mut normal);
    // Turning dir further around +normal increases its angle from ref when ref x dir points along +normal.
    if dot3(&turn, &normal) < 0.0 {
        for v in normal.iter_mut() {
            *v = -*v;
        }
    }
    let inverse = quat_conj(&s_referenceModelRot[parent as usize]);
    let mut out = quat_rotate_vec(&inverse, &normal);
    if dot3(&out, &out) < 1e-12 {
        return false;
    }
    norm3(&mut out);
    put3(axis, &out);
    *bone = root;
    true
}

// ---- the calibration hold

#[no_mangle]
pub unsafe extern "C" fn VRIK_HoldReferencePose(boneBuf: *mut u8) {
    if boneBuf.is_null() || s_holdCount <= 0 {
        return;
    }
    let n = s_holdCount.min(g_VRBoneCount);
    let mut keep = false;
    for i in 0..n as usize {
        keep |= s_holdKeepModel[i];
    }
    if keep {
        VRIK_ComputeFK(boneBuf, n);
        for i in 0..n as usize {
            if s_holdKeepModel[i] {
                s_holdKeepPos[i] = g_fkPos[i];
                s_holdKeepRot[i] = g_fkRot[i];
            }
        }
    }
    for i in 0..n {
        if s_holdBone[i as usize] {
            put3(bt(boneBuf, i), &s_referenceLocalPos[i as usize]);
            put4(br(boneBuf, i), &s_referenceLocalRot[i as usize]);
        }
    }
    if keep {
        VRIK_ComputeFK(boneBuf, n);
        for i in 0..n {
            let iu = i as usize;
            if s_holdKeepModel[iu] {
                let parent = g_VRBoneParent[iu] as usize;
                write_local_rot(boneBuf, i, &g_fkRot[parent], &s_holdKeepRot[iu]);
                write_local_pos(boneBuf, i, &g_fkPos[parent], &g_fkRot[parent], &s_holdKeepPos[iu]);
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_MarkLegOwned(bone: i32) {
    if (0..MAXB).contains(&bone) {
        s_bendLegOwned[bone as usize] = true;
    }
}

// ---- the rig's policy and reference pose

/// names[i] as bytes; None for a null name.
#[inline(always)]
unsafe fn name<'a>(names: *const *const c_char, i: i32) -> Option<&'a [u8]> {
    let p = *names.offset(i as isize);
    if p.is_null() {
        None
    } else {
        Some(CStr::from_ptr(p).to_bytes())
    }
}

/// Runtime name/parent tables, never hardcoded indices; reference transforms in the engine's 48-byte local layout.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ConfigureRigPolicy(
    names: *const *const c_char,
    count: i32,
    reference: *const u8,
    referenceCount: i32,
    female: bool,
) {
    let count = count.clamp(0, MAXB);
    // the physics hands' skin (cvr-physhands hand_skin) is a woman's or a man's
    super::shared_globals::g_VRBodyFemale.store(i32::from(female), core::sync::atomic::Ordering::Relaxed);
    g_VRCyberwareFamily = 0;
    for i in 0..count {
        if let Some(n) = name(names, i) {
            if equal_bone_name(n, b"l_plate_up_a") {
                g_VRCyberwareFamily = 1;
            }
            if equal_bone_name(n, b"l_wristSuspension01_JNT") {
                g_VRCyberwareFamily = 2;
            }
            if equal_bone_name(n, b"l_nanoWire01_grp") {
                g_VRCyberwareFamily = 3;
            }
            if equal_bone_name(n, b"launcher_base") {
                g_VRCyberwareFamily = 4;
            }
        }
    }
    VRIK_ConfigureCyberwareRig(names, count, reference, referenceCount);
    // A meta-rig can change when arms/cyberware are replaced: neither indices nor captured anatomical axes from the
    // previous layout may survive it.
    let mut fingerprint: u64 = 14695981039346656037;
    let mut hash_byte = |b: u8| fingerprint = (fingerprint ^ b as u64).wrapping_mul(1099511628211);
    hash_byte(if female { 1 } else { 0 });
    for i in 0..count {
        if let Some(n) = name(names, i) {
            for &c in n {
                hash_byte(c);
            }
        }
        hash_byte(0);
        let parent = g_VRBoneParent[i as usize];
        hash_byte(parent as u8);
        hash_byte((parent >> 8) as u8);
        if !reference.is_null() && i < referenceCount {
            for b in 0..12 {
                hash_byte(*reference.offset(i as isize * 48 + b));
            }
            for b in 16..32 {
                hash_byte(*reference.offset(i as isize * 48 + b));
            }
        }
    }
    if fingerprint != s_rigFingerprint {
        s_rigFingerprint = fingerprint;
        s_rigRevision = s_rigRevision.wrapping_add(1);
    }
    g_VRForeTwistR = [-1; 3];
    g_VRForeTwistL = [-1; 3];
    s_scapulaCount = [0; 2];
    s_upperSkin = [[-1; 2]; 2];
    s_shoulderBase = [ShoulderBase::DEFAULT; 2];
    s_ladderFingers = [[-1; 19]; 2];
    for bone in 0..count {
        let Some(nm) = name(names, bone) else { continue };
        for side in 0..2usize {
            let prefix: &[u8] = if side == 1 { b"Right" } else { b"Left" };
            if !starts_bone_name(nm, prefix) {
                continue;
            }
            let hand = if side == 1 { g_VRRightBoneIdx } else { g_VRLeftBoneIdx };
            let mut ancestor = bone;
            let mut guard = 0;
            while ancestor >= 0 && ancestor < count && ancestor != hand && guard < count {
                ancestor = g_VRBoneParent[ancestor as usize] as i32;
                guard += 1;
            }
            if ancestor != hand {
                continue;
            }
            for slot in 0..19 {
                if equal_bone_name(&nm[prefix.len()..], ladder::FINGERS[slot].suffix.as_bytes()) {
                    s_ladderFingers[side][slot] = bone;
                }
            }
        }
    }
    for i in 0..count {
        let Some(nm) = name(names, i) else { continue };
        for left in [false, true] {
            let slot = forearm_twist_slot(nm, left);
            let fore = if left { g_VRLeftForeArmIdx } else { g_VRRightForeArmIdx };
            // Supported helpers are siblings parented to the forearm; the topology also rejects stale names.
            if let Some(slot) = slot {
                if fore >= 0 && fore < count && g_VRBoneParent[i as usize] as i32 == fore {
                    if left {
                        g_VRForeTwistL[slot] = i;
                    } else {
                        g_VRForeTwistR[slot] = i;
                    }
                }
            }
        }
    }
    // The toes by name, under their own foot.
    s_toeBone = [-1, -1];
    for i in 0..count {
        let Some(nm) = name(names, i) else { continue };
        let side = if equal_bone_name(nm, b"RightToeBase") {
            0
        } else if equal_bone_name(nm, b"LeftToeBase") {
            1
        } else {
            -1
        };
        if side < 0 {
            continue;
        }
        let foot = if side == 0 { g_VRRightFootIdx } else { g_VRLeftFootIdx };
        let mut ancestor = g_VRBoneParent[i as usize] as i32;
        let mut guard = 0;
        while ancestor >= 0 && ancestor < count && {
            let g = guard;
            guard += 1;
            g < 8
        } {
            if ancestor == foot {
                s_toeBone[side as usize] = i;
                break;
            }
            ancestor = g_VRBoneParent[ancestor as usize] as i32;
        }
    }
    g_VRUpperOwned = [false; MAX_BONES];
    g_VRShadowSource = [-1; MAX_BONES];
    s_bendLegShadowSource = [-1; MAX_BONES];
    s_bendLegOwned = [false; MAX_BONES];
    s_referenceValid = [false; MAX_BONES];
    g_VRHeadReferenceValid = false;
    g_solveCacheN = 0;
    g_solveCacheModel = false;
    g_solveCacheTick = 0xFFFF_FFFF;
    for i in 0..count {
        let Some(nm) = name(names, i) else { continue };
        if is_rig_control(nm) {
            continue;
        }
        if equal_bone_name(nm, b"Hips") {
            g_VRUpperOwned[i as usize] = true;
        }
        let mut ancestor = i;
        let mut guard = 0;
        while ancestor >= 0 && ancestor < count && {
            let g = guard;
            guard += 1;
            g < count
        } {
            if let Some(an) = name(names, ancestor) {
                if is_primary_spine(an) {
                    g_VRUpperOwned[i as usize] = true;
                    break;
                }
            }
            let parent = g_VRBoneParent[ancestor as usize] as i32;
            if parent >= ancestor {
                break;
            }
            ancestor = parent;
        }
    }
    for i in 0..count {
        let Some(nm) = name(names, i) else { continue };
        if !starts_bone_name(nm, b"shadow_") {
            continue;
        }
        let tail = &nm[7..];
        for source in 0..count {
            if g_VRUpperOwned[source as usize] {
                if let Some(sn) = name(names, source) {
                    if equal_bone_name(tail, sn) {
                        g_VRShadowSource[i as usize] = source;
                        break;
                    }
                }
            }
        }
        for source in [g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, g_VRLeftUpLegIdx, g_VRLeftLegIdx, g_VRLeftFootIdx] {
            if source >= 0 && source < count {
                if let Some(sn) = name(names, source) {
                    if equal_bone_name(tail, sn) {
                        s_bendLegShadowSource[i as usize] = source;
                        break;
                    }
                }
            }
        }
    }
    // Do not use global working FK arrays while arming a rig.
    let reference_count = if !reference.is_null() { referenceCount.clamp(0, count) } else { 0 };
    for i in 0..reference_count {
        let iu = i as usize;
        let q = a4(reference.offset(i as isize * 48 + 16) as *const f32);
        let norm = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        let parent = g_VRBoneParent[iu] as i32;
        if !norm.is_finite() || norm < 0.5 || norm > 1.5 || parent >= i || (parent >= 0 && !s_referenceValid[parent as usize]) {
            continue;
        }
        s_referenceLocalRot[iu] = q;
        quat_norm(&mut s_referenceLocalRot[iu]);
        let p = a3(reference.offset(i as isize * 48) as *const f32);
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            continue;
        }
        s_referenceLocalPos[iu] = p;
        if parent >= 0 {
            let pu = parent as usize;
            s_referenceModelRot[iu] = quat_mul(&s_referenceModelRot[pu], &s_referenceLocalRot[iu]);
            let r = quat_rotate_vec(&s_referenceModelRot[pu], &s_referenceLocalPos[iu]);
            s_referenceModelPos[iu] = r;
            for k in 0..3 {
                s_referenceModelPos[iu][k] += s_referenceModelPos[pu][k];
            }
        } else {
            s_referenceModelRot[iu] = s_referenceLocalRot[iu];
            s_referenceModelPos[iu] = s_referenceLocalPos[iu];
        }
        quat_norm(&mut s_referenceModelRot[iu]);
        s_referenceValid[iu] = true;
    }
    let head = g_VRHeadBoneIdx;
    if head >= 0 && head < reference_count && s_referenceValid[head as usize] {
        g_VRHeadReferenceModelRot = s_referenceModelRot[head as usize];
        g_VRHeadReferenceValid = true;
    }
    // Resolve the camera and eyes from this rig, then evaluate the same neutral spine posture used below. The upper
    // neck is sampled with a neutral HMD.
    let (mut camera, mut left_eye, mut right_eye) = (-1i32, -1i32, -1i32);
    for i in 0..count {
        if let Some(nm) = name(names, i) {
            if equal_bone_name(nm, b"Torso_fppCamera_Control_GRP") {
                camera = i;
            }
            if equal_bone_name(nm, b"l_J_eye_JNT") || (left_eye < 0 && equal_bone_name(nm, b"LeftEye")) {
                left_eye = i;
            }
            if equal_bone_name(nm, b"r_J_eye_JNT") || (right_eye < 0 && equal_bone_name(nm, b"RightEye")) {
                right_eye = i;
            }
        }
    }
    s_neutralPos = [[0.0; 3]; MAX_BONES];
    s_neutralRot = [[0.0; 4]; MAX_BONES];
    let (np, nr) = (&mut s_neutralPos, &mut s_neutralRot);
    for i in 0..reference_count {
        let iu = i as usize;
        if !s_referenceValid[iu] {
            continue;
        }
        let parent = g_VRBoneParent[iu] as i32;
        if parent >= 0 {
            let pu = parent as usize;
            np[iu] = quat_rotate_vec(&nr[pu], &s_referenceLocalPos[iu]);
            for k in 0..3 {
                np[iu][k] += np[pu][k];
            }
            nr[iu] = quat_mul(&nr[pu], &s_referenceLocalRot[iu]);
        } else {
            np[iu] = s_referenceLocalPos[iu];
            nr[iu] = s_referenceLocalRot[iu];
        }
        let mut flex = 0.0f32;
        let mut over = false;
        let mut c = 0;
        while c < g_VRSpineCount && c < 8 {
            if g_VRSpineIdx[c as usize] == i {
                flex = spine_flex(c, g_VRSpineCount);
                over = true;
                if c == g_VRSpineCount - 1 {
                    np[iu][1] -= CHEST_RETRACTION;
                }
            }
            c += 1;
        }
        if i == g_VRNeckIdx {
            flex = NECK_FLEX;
            over = true;
        }
        if i == g_VRNeck1Idx {
            flex = UPPER_NECK_FLEX;
            over = true;
        }
        if i == head {
            flex = 0.0;
            over = true;
        }
        if over {
            let bend = [sin(flex * 0.5), 0.0, 0.0, cos(flex * 0.5)];
            nr[iu] = quat_mul(&bend, &s_referenceModelRot[iu]);
        }
    }
    let neck = if g_VRNeck1Idx >= 0 { g_VRNeck1Idx } else { g_VRNeckIdx };
    s_referenceCameraValid = camera >= 0 && camera < reference_count && s_referenceValid[camera as usize];
    s_referenceCameraHeight = if s_referenceCameraValid { s_referenceModelPos[camera as usize][2] } else { 0.0 };
    s_referenceCameraBone = if s_referenceCameraValid { camera } else { -1 };
    let _lock = NECK_MOUNT_LOCK.lock();
    let inside = |b: i32| b >= 0 && b < reference_count;
    s_neckMountValid = inside(neck)
        && inside(camera)
        && inside(left_eye)
        && inside(right_eye)
        && s_referenceValid[neck as usize]
        && s_referenceValid[camera as usize]
        && s_referenceValid[left_eye as usize]
        && s_referenceValid[right_eye as usize];
    if s_neckMountValid {
        let nk = np[neck as usize];
        let cam = s_referenceModelPos[camera as usize];
        s_neckMount = neck_camera_offset(
            av(nk[0], nk[1], nk[2]),
            0.5 * (np[left_eye as usize][2] + np[right_eye as usize][2]),
            av(cam[0], cam[1], cam[2]),
            female,
        );
    }
    s_eyeReferenceValid = s_neckMountValid && head >= 0 && head < reference_count && s_referenceValid[head as usize];
    if s_eyeReferenceValid {
        let inverse = quat_conj(&nr[head as usize]);
        let mut delta = [0.0f32; 3];
        for k in 0..3 {
            s_neutralEyeModel[k] = 0.5 * (np[left_eye as usize][k] + np[right_eye as usize][k]);
            delta[k] = s_neutralEyeModel[k] - np[head as usize][k];
        }
        s_eyeInHead = quat_rotate_vec(&inverse, &delta);
        let nk = np[neck as usize];
        s_neckCameraEyeOffset = neck_camera_eye_offset(
            av(nk[0], nk[1], nk[2]),
            av(s_neutralEyeModel[0], s_neutralEyeModel[1], s_neutralEyeModel[2]),
            female,
        );
    }
    {
        // Full-body tracking calibration reads the plain reference pose, not the neutral posture above.
        let mut fbt = VrikFbtReference::ZERO;
        fbt.female = female;
        let hips = g_VRHipsIdx;
        let feet = [g_VRLeftFootIdx, g_VRRightFootIdx];
        let uppers = [g_VRLeftUpperArmIdx, g_VRRightUpperArmIdx];
        let fores = [g_VRLeftForeArmIdx, g_VRRightForeArmIdx];
        let ok = |i: i32| i >= 0 && i < reference_count && s_referenceValid[i as usize];
        fbt.valid = ok(hips)
            && ok(head)
            && ok(left_eye)
            && ok(right_eye)
            && ok(feet[0])
            && ok(feet[1])
            && ok(uppers[0])
            && ok(uppers[1])
            && ok(fores[0])
            && ok(fores[1]);
        if fbt.valid {
            let mp = &s_referenceModelPos;
            let mr = &s_referenceModelRot;
            let inverse = quat_conj(&mr[head as usize]);
            let mut delta = [0.0f32; 3];
            for k in 0..3 {
                fbt.eye[k] = 0.5 * (mp[left_eye as usize][k] + mp[right_eye as usize][k]);
                delta[k] = fbt.eye[k] - mp[head as usize][k];
            }
            fbt.eyeInHead = quat_rotate_vec(&inverse, &delta);
            fbt.hipsPos = mp[hips as usize];
            fbt.hipsRot = mr[hips as usize];
            fbt.headPos = mp[head as usize];
            fbt.headRot = mr[head as usize];
            for side in 0..2 {
                fbt.footPos[side] = mp[feet[side] as usize];
                fbt.footRot[side] = mr[feet[side] as usize];
                fbt.upperArmPos[side] = mp[uppers[side] as usize];
                fbt.foreArmPos[side] = mp[fores[side] as usize];
                let hand_bone = if side == 0 { g_VRLeftBoneIdx } else { g_VRRightBoneIdx };
                if ok(hand_bone) {
                    fbt.handPos[side] = mp[hand_bone as usize];
                }
            }
        }
        s_fbtReference = fbt;
        // The calibration hold: Hips and below, minus the fppCamera chain (the view hangs off it).
        let n = reference_count.min(MAXB);
        let (mut held, mut kept) = (0, 0);
        for i in 0..n {
            let iu = i as usize;
            let parent = g_VRBoneParent[iu] as i32;
            let camera_bone = name(names, i).is_some_and(|nm| starts_bone_name(nm, b"Torso_fppCamera"));
            s_holdBone[iu] = s_referenceValid[iu] && !camera_bone && (i == hips || (parent >= 0 && s_holdBone[parent as usize]));
            s_holdKeepModel[iu] = camera_bone && parent >= 0 && s_holdBone[parent as usize];
            held += s_holdBone[iu] as i32;
            kept += s_holdKeepModel[iu] as i32;
        }
        s_holdCount = if hips >= 0 { n } else { 0 };
        let mut chain = camera;
        let mut ancestors = String::new();
        while chain >= 0 && ancestors.len() < 200 {
            if let Some(nm) = name(names, chain) {
                ancestors.push_str(&String::from_utf8_lossy(nm));
                ancestors.push_str(" < ");
            }
            chain = g_VRBoneParent[chain as usize] as i32;
        }
        if cvr_host::log::verbose() {
            log(&format!(
                "[fbt-calib] reference hold: {held} bones under Hips, {kept} fppCamera bones kept in place; camera chain: {ancestors}\n"
            ));
        }
        // V's skeleton for offline IK tools, once per rig and size per session (fbt_rig_<male|female>_<bones>.txt).
        let dump_key = n * 2 + if female { 1 } else { 0 };
        if !s_fbtDumped.contains(&dump_key) && cvr_host::log::verbose() {
            s_fbtDumped.push(dump_key);
            let file = format!("fbt_rig_{}_{}.txt\0", if female { "female" } else { "male" }, n);
            let mut path = [0 as c_char; 260];
            if cvr_cpp_vrik_config_file(file.as_ptr() as *const c_char, path.as_mut_ptr(), path.len()) && path[0] != 0 {
                let path_str = CStr::from_ptr(path.as_ptr()).to_string_lossy().into_owned();
                let roles: [(&str, i32); 19] = [
                    ("hips", hips),
                    ("neck", g_VRNeckIdx),
                    ("neck1", g_VRNeck1Idx),
                    ("head", head),
                    ("left_eye", left_eye),
                    ("right_eye", right_eye),
                    ("camera", camera),
                    ("left_upperarm", g_VRLeftUpperArmIdx),
                    ("left_forearm", g_VRLeftForeArmIdx),
                    ("left_hand", g_VRLeftBoneIdx),
                    ("right_upperarm", g_VRRightUpperArmIdx),
                    ("right_forearm", g_VRRightForeArmIdx),
                    ("right_hand", g_VRRightBoneIdx),
                    ("left_upleg", g_VRLeftUpLegIdx),
                    ("left_leg", g_VRLeftLegIdx),
                    ("left_foot", g_VRLeftFootIdx),
                    ("right_upleg", g_VRRightUpLegIdx),
                    ("right_leg", g_VRRightLegIdx),
                    ("right_foot", g_VRRightFootIdx),
                ];
                let mut text = String::new();
                text.push_str("# V's skeleton, reference pose (Anim/CharacterRig.cpp). Model space: x right, y forward, z up, metres.\n");
                text.push_str("# roles: name index\n");
                for (r, idx) in roles {
                    let _ = writeln!(text, "role {r} {idx}");
                }
                let mut c = 0;
                while c < g_VRSpineCount && c < 8 {
                    let _ = writeln!(text, "role spine{} {}", c, g_VRSpineIdx[c as usize]);
                    c += 1;
                }
                // The hands' thumb joint and end, which the shared IK takes the hand's axes from (Fbt/FbtIk.cpp).
                let hand_roles = [
                    ("left_thumb1", "LeftHandThumb1"),
                    ("left_hand_end", "LeftHandEnd"),
                    ("right_thumb1", "RightHandThumb1"),
                    ("right_hand_end", "RightHandEnd"),
                ];
                for (role, bone) in hand_roles {
                    for i in 0..n {
                        if name(names, i) == Some(bone.as_bytes()) {
                            let _ = writeln!(text, "role {role} {i}");
                            break;
                        }
                    }
                }
                text.push_str("# bone: index parent name  local pos xyz  local rot xyzw  model pos xyz  model rot xyzw\n");
                for i in 0..n {
                    let iu = i as usize;
                    if !s_referenceValid[iu] {
                        continue;
                    }
                    let (lp, lr) = (s_referenceLocalPos[iu], s_referenceLocalRot[iu]);
                    let (mp, mr) = (s_referenceModelPos[iu], s_referenceModelRot[iu]);
                    let nm = name(names, i).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_else(|| "?".into());
                    let _ = writeln!(
                        text,
                        "bone {} {} {}  {:.6} {:.6} {:.6}  {:.7} {:.7} {:.7} {:.7}  {:.6} {:.6} {:.6}  {:.7} {:.7} {:.7} {:.7}",
                        i, g_VRBoneParent[iu], nm, lp[0], lp[1], lp[2], lr[0], lr[1], lr[2], lr[3], mp[0], mp[1], mp[2], mr[0],
                        mr[1], mr[2], mr[3]
                    );
                }
                // fopen(path, "w"): a text-mode stream, every '\n' written as CRLF.
                if std::fs::write(&path_str, text.replace('\n', "\r\n")).is_ok() {
                    log(&format!("[fbt-calib] skeleton written to {path_str} ({n} bones)\n"));
                    let slot = &mut s_fbtRigPath[if female { 1 } else { 0 }];
                    let bytes = path_str.as_bytes();
                    let len = bytes.len().min(slot.len() - 1);
                    slot[..len].copy_from_slice(&bytes[..len]);
                    slot[len] = 0;
                }
            }
        }
    }
    drop(_lock);
    let chest = if g_VRSpineCount > 0 { g_VRSpineIdx[(g_VRSpineCount - 1) as usize] } else { -1 };
    for i in 0..count {
        let Some(nm) = name(names, i) else { continue };
        if !s_referenceValid[i as usize] {
            continue;
        }
        for side in 0..2usize {
            let left = side == 1;
            let upper = if left { g_VRLeftUpperArmIdx } else { g_VRRightUpperArmIdx };
            let parent = g_VRBoneParent[i as usize] as i32;
            if parent == chest
                && starts_bone_name(nm, if left { b"l_scapula_" } else { b"r_scapula_" })
                && s_scapulaCount[side] < 4
            {
                s_scapula[side][s_scapulaCount[side] as usize] = i;
                s_scapulaCount[side] += 1;
            }
            if parent == upper {
                for slot in 0..2 {
                    let n: &[u8] = match (left, slot) {
                        (true, 0) => b"l_SHL_0_JNT",
                        (true, _) => b"l_SHL_1_JNT",
                        (false, 0) => b"r_SHL_0_JNT",
                        (false, _) => b"r_SHL_1_JNT",
                    };
                    if equal_bone_name(nm, n) {
                        s_upperSkin[side][slot] = i;
                    }
                }
            }
        }
    }
    // The left-handed weapon's mirror, from the same reference pose.
    super::weapon_arm::cvr_anim_left_handed_arm_rig(names, count);
}

// ---- the legacy hand write and the cyberware probe

/// One VR hand into the destination bone buffer (head-relative when the bind says so).
#[no_mangle]
pub unsafe extern "C" fn VRIK_WriteHand(
    boneBuf: *mut u8,
    bIdx: i32,
    headPos: *const f32,
    headQuat: *const f32,
    headOk: bool,
    vrPos: *const f32,
    vrQuat: *const f32,
    writeRot: bool,
) {
    if bIdx < 0 {
        return;
    }
    let s = g_VRBindScale;
    let mut local = [0.0f32; 3];
    VRIK_RemapAxis(g_VRBindAxis, vrPos, local.as_mut_ptr());
    local[0] *= s;
    local[1] *= s;
    local[2] *= s;
    let mut pos;
    if g_VRUseHeadRelative != 0 && headOk {
        let rotated = quat_rotate_vec(&a4(headQuat), &local);
        pos = [*headPos + rotated[0], *headPos.add(1) + rotated[1], *headPos.add(2) + rotated[2]];
    } else {
        pos = local;
    }
    pos[0] += g_VRBindOffX;
    pos[1] += g_VRBindOffY;
    pos[2] += g_VRBindOffZ;
    put3(bt(boneBuf, bIdx), &pos);
    if writeRot {
        // VR->game axis swap, same as the gizmo's mapLocalQuat: (i, -k, j, r).
        let local_quat = [*vrQuat, -*vrQuat.add(2), *vrQuat.add(1), *vrQuat.add(3)];
        let r = br(boneBuf, bIdx);
        if g_VRUseHeadRelative != 0 && headOk {
            VRIK_QuatMul(headQuat, local_quat.as_ptr(), r);
        } else {
            put4(r, &local_quat);
        }
    }
}

/// Read-only diagnostic of the downstream result of a native cyberware pass.
#[no_mangle]
pub unsafe extern "C" fn VRIK_PublishNativeCyberwarePose(buffer: *mut u8, graph: u64) {
    for i in 0..16 {
        if g_VRCyberGraphHashes[i] == 0 || g_VRCyberGraphHashes[i] == graph {
            g_VRCyberGraphHashes[i] = graph;
            g_VRCyberGraphCalls[i] = g_VRCyberGraphCalls[i].wrapping_add(1);
            break;
        }
    }
    if g_VRDiagCapture == 0 {
        return;
    }
    let count = VRIK_FKCount();
    VRIK_ComputeFK(buffer, count);
    for i in 0..count.max(0) as usize {
        g_VRFKSnapPos[i] = g_fkPos[i];
        g_VRFKSnapRot[i] = g_fkRot[i];
    }
    g_VRFKSnapCount = count;
    super::shared_globals::g_VRFKSnapSeq.fetch_add(1, core::sync::atomic::Ordering::Release);
    super::frame_order::note(b'K');
}

// The cross product the C++ inlined at its call sites, by value.
#[inline(always)]
pub fn cross(a: &V3, b: &V3) -> V3 {
    cross3(a, b)
}
