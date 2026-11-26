//! The body under the HMD: the pelvis and spine placed under the view, the legs solved back onto the animation's
//! feet, the bend into a floor reach, the seated and swimming head-anchored bodies, the ladder's upper body and
//! fingers, the clip's own body through a slide, a jump, a fall and a landing; and the shake probe.
use super::cache::VRIK_PublishBodyBones;
use super::env::*;
use super::rig::*;
use super::snapshot::SharedPose;
use super::state::*;
use super::types::*;
use super::*;
use crate::ladder;
use crate::math::{dist3, dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec, quat_scale};
use crate::skeleton::{spine_flex, CHEST_RETRACTION, NECK_FLEX, UPPER_NECK_FLEX};
use cvr_body::bend::{bend_weight, hinge_drop, smooth_range, MAX_BEND};
use cvr_body::cmath::{acos, atan2, clamp, cos, exp, expm1, hypot, max, min, remainder, sin, sqrt};
use cvr_camera::anchor::rotate_anchor_yaw;

const K_BEND_LIMIT_TIME: f32 = 0.35;
const K_BEND_EASE_TIME: f32 = 0.12;

/// A float of a 48-byte bone image, bit for bit.
#[inline(always)]
fn f32_at(b: &[u8; 48], idx: usize) -> f32 {
    f32::from_le_bytes([b[idx * 4], b[idx * 4 + 1], b[idx * 4 + 2], b[idx * 4 + 3]])
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_BeginBodySolve(boneBuf: *mut u8) {
    s_shoulderBase = [ShoulderBase::DEFAULT; 2];
    g_VRIKBodyEyeBound = false;
    g_VRIKVehicleUpperActive = false;
    s_bendLegOwned = [false; MAX_BONES];
    g_VRIKBodyBendAngle = 0.0;
    g_VRIKBodyBendPelvis = [0.0; 3];
    for saved in s_seatedRestore.iter() {
        if saved.buffer == boneBuf && saved.revision == s_rigRevision {
            for i in 0..saved.count.max(0) as usize {
                if same_local(boneBuf, saved.bones[i], &saved.solved[i]) {
                    restore_bone(boneBuf, saved.bones[i], &saved.native[i]);
                }
            }
            break;
        }
    }
    // Some pose passes feed our own output back in: undo only a complete match of what we last wrote here.
    for saved in s_bendLegRestore.iter() {
        if saved.buffer == boneBuf && saved.revision == s_rigRevision {
            if saved.vehicle {
                for i in 0..saved.count as usize {
                    if same_local(boneBuf, saved.bones[i], &saved.solved[i]) {
                        restore_bone(boneBuf, saved.bones[i], &saved.native[i]);
                    }
                }
                break;
            }
            let mut group = 0i32;
            while group < saved.count {
                let end = (group + 6).min(saved.count);
                let mut own_output = true;
                let mut i = group;
                while i < end && own_output {
                    let local = &*bone12(boneBuf, saved.bones[i as usize]);
                    for k in 0..12 {
                        if !local[k].is_finite() || (local[k] - saved.solved[i as usize][k]).abs() > 1e-6 {
                            own_output = false;
                            break;
                        }
                    }
                    i += 1;
                }
                if own_output {
                    for i in group..end {
                        restore_bone(boneBuf, saved.bones[i as usize], &saved.native[i as usize]);
                    }
                }
                group += 6;
            }
            break;
        }
    }
}

/// Generic 2-bone limb IK (hip -> knee -> foot), rotation-only; the knee bends toward poleDir.
pub unsafe fn solve_leg(boneBuf: *mut u8, up_idx: i32, mid_idx: i32, end_idx: i32, target: &V3, pole_dir: &V3) {
    if up_idx < 0 || mid_idx < 0 || end_idx < 0 || up_idx >= MAXB || mid_idx >= MAXB || end_idx >= MAXB {
        return;
    }
    let (u, m, e) = (up_idx as usize, mid_idx as usize, end_idx as usize);
    let hip = g_fkPos[u];
    let mut cur_up = sub3(&g_fkPos[m], &hip);
    let mut cur_lo = sub3(&g_fkPos[e], &g_fkPos[m]);
    let up_len = norm3(&mut cur_up);
    let lo_len = norm3(&mut cur_lo);
    if up_len < 1e-4 || lo_len < 1e-4 {
        return;
    }
    let mut to_target = sub3(target, &hip);
    let mut dist = norm3(&mut to_target);
    if dist < 1e-4 {
        return;
    }
    let max_len = up_len + lo_len;
    dist = clamp(dist, (up_len - lo_len).abs() + 1e-6, max_len * 0.999999);
    let mut pole = *pole_dir;
    let pd = dot3(&pole, &to_target);
    pole[0] -= to_target[0] * pd;
    pole[1] -= to_target[1] * pd;
    pole[2] -= to_target[2] * pd;
    if norm3(&mut pole) < 1e-3 {
        pole = [0.0, 0.0, 1.0];
    }
    let mut cos_hip = (up_len * up_len + dist * dist - lo_len * lo_len) / (2.0 * up_len * dist);
    if cos_hip < -1.0 {
        cos_hip = -1.0;
    }
    if cos_hip > 1.0 {
        cos_hip = 1.0;
    }
    let hip_ang = acos(cos_hip);
    let mut knee_pos = [0.0f32; 3];
    for k in 0..3 {
        knee_pos[k] = hip[k] + to_target[k] * (cos(hip_ang) * up_len) + pole[k] * (sin(hip_ang) * up_len);
    }
    let mut des_up = sub3(&knee_pos, &hip);
    norm3(&mut des_up);
    let d1 = quat_from_to(&cur_up, &des_up);
    let mut new_up = quat_mul(&d1, &g_fkRot[u]);
    quat_norm(&mut new_up);
    let up_p = g_VRBoneParent[u] as i32;
    let prot = if up_p >= 0 && up_p < MAXB { g_fkRot[up_p as usize] } else { ID };
    write_local_rot(boneBuf, up_idx, &prot, &new_up);
    let lo_base = quat_rotate_vec(&d1, &cur_lo);
    let mut des_lo = sub3(target, &knee_pos);
    norm3(&mut des_lo);
    let d2 = quat_from_to(&lo_base, &des_lo);
    let tmp = quat_mul(&d2, &d1);
    let mut new_lo = quat_mul(&tmp, &g_fkRot[m]);
    quat_norm(&mut new_lo);
    write_local_rot(boneBuf, mid_idx, &new_up, &new_lo);
}

/// Only the torso path while fitting the pose: full FK is needed just for the final writes.
unsafe fn evaluate_torso(buffer: *mut u8, flexion: f32, head_rotation: &Q) -> VrikTorsoCandidate {
    let mut pose = VrikTorsoCandidate::ZERO;
    let (hips, head) = (g_VRHipsIdx, g_VRHeadBoneIdx);
    let push = |pose: &mut VrikTorsoCandidate, b: i32| {
        pose.bones[pose.count as usize] = b;
        pose.count += 1;
    };
    push(&mut pose, hips);
    let spine_count = g_VRSpineCount.clamp(0, 8);
    for i in 0..spine_count {
        push(&mut pose, g_VRSpineIdx[i as usize]);
    }
    if g_VRNeckIdx >= 0 {
        push(&mut pose, g_VRNeckIdx);
    }
    if g_VRNeck1Idx >= 0 && pose.count < 11 {
        push(&mut pose, g_VRNeck1Idx);
    }
    if pose.count >= 12 {
        return pose;
    }
    push(&mut pose, head);
    for slot in 0..pose.count as usize {
        let bone = pose.bones[slot];
        if bone < 0 || bone >= VRIK_FKCount() || !s_referenceValid[bone as usize] {
            return pose;
        }
        if slot == 0 {
            let tilt = [-sin(flexion * 0.3), 0.0, 0.0, cos(flexion * 0.3)];
            pose.rotations[slot] = quat_mul(&tilt, &s_referenceModelRot[hips as usize]);
            continue;
        }
        let parent = g_VRBoneParent[bone as usize] as i32;
        let mut parent_slot = -1i32;
        for i in 0..slot {
            if pose.bones[i] == parent {
                parent_slot = i as i32;
            }
        }
        if parent_slot < 0 {
            return pose;
        }
        let ps = parent_slot as usize;
        let mut p = quat_rotate_vec(&pose.rotations[ps], &a3(bt(buffer, bone)));
        for k in 0..3 {
            p[k] += pose.positions[ps][k];
        }
        pose.positions[slot] = p;
        if bone == head {
            pose.rotations[slot] = quat_mul(head_rotation, &g_VRHeadReferenceModelRot);
            continue;
        }
        let (mut flex, mut share, mut hinge) = (0.0f32, 0.0f32, flexion);
        if slot as i32 <= spine_count {
            let i = slot as i32 - 1;
            let t = if spine_count > 1 { i as f32 / (spine_count - 1) as f32 } else { 1.0 };
            flex = spine_flex(i, spine_count);
            hinge = flexion * (0.85 + 0.15 * t);
        }
        if bone == g_VRNeckIdx {
            flex = NECK_FLEX;
            share = 0.25;
        }
        if bone == g_VRNeck1Idx {
            flex = UPPER_NECK_FLEX;
            share = 0.55;
        }
        let pitch = flex - hinge * (1.0 - share);
        let bend = [sin(pitch * 0.5), 0.0, 0.0, cos(pitch * 0.5)];
        let head_part = quat_scale(head_rotation, share);
        let combined = quat_mul(&head_part, &bend);
        pose.rotations[slot] = quat_mul(&combined, &s_referenceModelRot[bone as usize]);
    }
    let last = (pose.count - 1) as usize;
    let mut eye = quat_rotate_vec(&pose.rotations[last], &s_eyeInHead);
    for k in 0..3 {
        eye[k] += pose.positions[last][k];
    }
    pose.eye = eye;
    pose.valid = true;
    pose
}

#[inline(always)]
unsafe fn leg_joints() -> [i32; 6] {
    [g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, g_VRLeftUpLegIdx, g_VRLeftLegIdx, g_VRLeftFootIdx]
}

/// The legs as the animation has them (g_fkPos/g_fkRot must hold the native pose).
unsafe fn capture_native_legs() -> NativeLegs {
    let mut legs = NativeLegs::ZERO;
    let hips = g_VRHipsIdx;
    let j = leg_joints();
    if hips < 0 || hips >= VRIK_FKCount() {
        return legs;
    }
    for i in 0..6 {
        if j[i] < 0 || j[i] >= VRIK_FKCount() {
            return legs;
        }
    }
    legs.hips = g_fkPos[hips as usize];
    for side in 0..2 {
        let l = &j[side * 3..side * 3 + 3];
        let par = |b: i32| g_VRBoneParent[b as usize] as i32;
        if par(l[0]) != hips || par(l[1]) != l[0] || par(l[2]) != l[1] {
            return legs;
        }
        legs.foot[side] = g_fkPos[l[2] as usize];
        legs.footRot[side] = g_fkRot[l[2] as usize];
        let axis = sub3(&g_fkPos[l[2] as usize], &g_fkPos[l[0] as usize]);
        let knee = sub3(&g_fkPos[l[1] as usize], &g_fkPos[l[0] as usize]);
        let a2 = dot3(&axis, &axis);
        if !(a2 > 1e-6) {
            return legs;
        }
        let along = dot3(&knee, &axis) / a2;
        // the knee's own side of the hip-foot line, biased 2 cm ahead
        let mut pole = [knee[0] - axis[0] * along, knee[1] - axis[1] * along, knee[2] - axis[2] * along];
        pole[1] += 0.02;
        legs.pole[side] = pole;
        for k in 0..3 {
            if !legs.foot[side][k].is_finite() || !legs.pole[side][k].is_finite() {
                return legs;
            }
        }
        let toe = s_toeBone[side];
        if toe >= 0 && toe < VRIK_FKCount() {
            legs.toe[side] = g_fkPos[toe as usize];
            legs.toeValid[side] = legs.toe[side][0].is_finite() && legs.toe[side][1].is_finite();
        }
    }
    legs.valid = true;
    legs
}

/// THE KNEE OVER THE TOES (CyberpunkVR_SquatKneeMode).
unsafe fn knee_pole(legs: &NativeLegs, side: usize) -> V3 {
    let mut out = legs.pole[side];
    if CyberpunkVR_SquatKneeMode == 0 || !legs.toeValid[side] {
        return out;
    }
    let mut toe = [legs.toe[side][0] - legs.foot[side][0], legs.toe[side][1] - legs.foot[side][1], 0.0];
    let n = hypot(toe[0], toe[1]);
    if !(n > 1e-3) {
        return out;
    }
    toe[0] /= n;
    toe[1] /= n;
    let m = sqrt(out[0] * out[0] + out[1] * out[1] + out[2] * out[2]);
    if !(m > 1e-4) {
        return out;
    }
    let native = [out[0] / m, out[1] / m, out[2] / m];
    // toes never lead a knee more than a right angle off the animation's own knee
    let horizontal = hypot(native[0], native[1]);
    let agree = if horizontal > 1e-3 { (toe[0] * native[0] + toe[1] * native[1]) / horizontal } else { 1.0 };
    let w = smooth_range(agree, 0.0, 0.5) * (if CyberpunkVR_SquatKneeMode == 1 { 1.0 } else { 0.5 });
    for k in 0..3 {
        out[k] = native[k] + (toe[k] - native[k]) * w;
    }
    out
}

/// WHERE THE FOOT STANDS (CyberpunkVR_SquatFeetUnderHips): the animation's stance moved with the hips.
unsafe fn move_stance(legs: &mut NativeLegs, placed_hips: &V3) {
    if !legs.valid || CyberpunkVR_SquatFeetUnderHips == 0 {
        return;
    }
    let mv = [placed_hips[0] - legs.hips[0], placed_hips[1] - legs.hips[1]];
    if !mv[0].is_finite() || !mv[1].is_finite() {
        return;
    }
    for k in 0..2 {
        for side in 0..2 {
            legs.foot[side][k] += mv[k];
            legs.toe[side][k] += mv[k];
        }
        legs.hips[k] += mv[k];
    }
}

#[inline(always)]
fn foot_target(legs: &NativeLegs, side: usize) -> V3 {
    legs.foot[side]
}

/// A SQUAT STANDS SYMMETRIC (CyberpunkVR_SquatSymmetric): the feet level, at the animation's mean spread, turned out
/// alike, while the animation's feet stand still against its hips.
unsafe fn symmetric_stance(legs: &mut NativeLegs) {
    if !legs.valid || !legs.toeValid[0] || !legs.toeValid[1] {
        return;
    }
    let now = steady_ns();
    let mut rel = [[0.0f32; 2]; 2];
    for side in 0..2 {
        rel[side] = [legs.foot[side][0] - legs.hips[0], legs.foot[side][1] - legs.hips[1]];
    }
    if s_stancePrevValid {
        // duration<float>: the nanosecond count as a float, over 1e9
        let dt = (now - s_stancePrevTime) as f32 / 1_000_000_000.0;
        if dt > 0.25 {
            s_stanceMotion = 0.0;
        } else if dt > 1e-4 {
            let mut moved = 0.0f32;
            for side in 0..2 {
                moved = max(moved, hypot(rel[side][0] - s_stancePrev[side][0], rel[side][1] - s_stancePrev[side][1]));
            }
            s_stanceMotion = max(moved / dt, s_stanceMotion * exp(-dt / 0.3));
        }
    }
    s_stancePrev = rel;
    s_stancePrevValid = true;
    s_stancePrevTime = now;
    let w = if CyberpunkVR_SquatSymmetric != 0 { 1.0 - smooth_range(s_stanceMotion, 0.15, 0.40) } else { 0.0 };
    CyberpunkVR_DebugSquatStance[0] = s_stanceMotion;
    CyberpunkVR_DebugSquatStance[1] = w;
    if !(w > 1e-3) {
        return;
    }
    let deg = 0.01745329252f32;
    let mut angle = [0.0f32; 2];
    let mut outward = [0.0f32; 2];
    for side in 0..2 {
        angle[side] = atan2(legs.toe[side][0] - legs.foot[side][0], legs.toe[side][1] - legs.foot[side][1]);
        outward[side] = if side == 0 { angle[side] } else { -angle[side] }; // [0] right: +x is out
    }
    let turn_out = clamp((outward[0] + outward[1]) * 0.5, 10.0 * deg, 30.0 * deg);
    let half = clamp((legs.foot[0][0] - legs.foot[1][0]) * 0.5, 0.08, 0.25);
    let along = (legs.foot[0][1] + legs.foot[1][1]) * 0.5;
    for side in 0..2 {
        let sign = if side == 0 { 1.0f32 } else { -1.0 };
        let phi = remainder(angle[side] - sign * turn_out, 6.28318531) * w; // CCW about the vertical
        let (c, s) = (cos(phi), sin(phi));
        let turn = |v: &mut [f32]| {
            let (x, y) = (v[0], v[1]);
            v[0] = c * x - s * y;
            v[1] = s * x + c * y;
        };
        let mut toe = sub3(&legs.toe[side], &legs.foot[side]);
        turn(&mut toe);
        turn(&mut legs.pole[side]);
        let q = [0.0, 0.0, sin(phi * 0.5), cos(phi * 0.5)];
        legs.footRot[side] = quat_mul(&q, &legs.footRot[side]);
        legs.foot[side][0] += (legs.hips[0] + sign * half - legs.foot[side][0]) * w;
        legs.foot[side][1] += (along - legs.foot[side][1]) * w;
        for k in 0..3 {
            legs.toe[side][k] = legs.foot[side][k] + toe[k];
        }
    }
}

/// The restore entry for this buffer, filled with the six leg joints and their shadows as they are now.
unsafe fn save_legs(boneBuf: *mut u8, joints: &[i32; 6]) -> *mut BendLegRestore {
    let saved = restore_slot(boneBuf);
    let s = &mut *saved;
    s.buffer = boneBuf;
    s.revision = s_rigRevision;
    s.count = 6;
    s.vehicle = false;
    for i in 0..6 {
        s.bones[i] = joints[i];
        copy_bone(&mut s.native[i], boneBuf, joints[i]);
    }
    let mut bone = 0;
    while bone < VRIK_FKCount() && s.count < 12 {
        if s_bendLegShadowSource[bone as usize] >= 0 {
            let slot = s.count as usize;
            s.count += 1;
            s.bones[slot] = bone;
            copy_bone(&mut s.native[slot], boneBuf, bone);
        }
        bone += 1;
    }
    saved
}

unsafe fn apply_body_bend(
    boneBuf: *mut u8,
    mut angle: f32,
    head_rotation: &Q,
    squat_drop: f32,
    head_delta_model: Option<&V3>,
    mut limit_weight: f32,
    native: Option<&NativeLegs>,
) {
    // (run on at no angle while the limit fades out: see kBendLimitTime)
    if !angle.is_finite() || (angle <= 1e-5 && !(limit_weight > 0.0)) {
        return;
    }
    angle = clamp(angle, 0.0, MAX_BEND);
    limit_weight = smooth_range(if limit_weight.is_finite() { limit_weight } else { 1.0 }, 0.0, 1.0);
    // Keep the floor-reaching range, but do not amplify the onset of a bend.
    let gain = 1.0 + 0.2 * smooth_range(angle, 0.52359878, 1.04719755);
    let flexion = min(1.30899694, angle * gain);
    let (hips, head) = (g_VRHipsIdx, g_VRHeadBoneIdx);
    if hips < 0 || hips >= VRIK_FKCount() || !s_referenceValid[hips as usize] {
        return;
    }
    let hu = hips as usize;
    let joints = leg_joints();
    let mut legs = true;
    for bone in joints {
        legs = legs && bone >= 0 && bone < VRIK_FKCount();
    }
    if legs {
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            legs = legs
                && g_VRBoneParent[j[0] as usize] as i32 == hips
                && g_VRBoneParent[j[1] as usize] as i32 == j[0]
                && g_VRBoneParent[j[2] as usize] as i32 == j[1];
        }
    }
    let mut feet = [[0.0f32; 3]; 2];
    let mut foot_rot = [[0.0f32; 4]; 2];
    let mut length = [[0.0f32; 2]; 2];
    let mut saved: *mut BendLegRestore = core::ptr::null_mut();
    // how far the placed hips stand from the animation's: past 6-12 cm the feet are the animation's
    let mut own = 0.0f32;
    if let Some(n) = native {
        if n.valid {
            own = smooth_range((g_fkPos[hu][2] - n.hips[2]).abs(), 0.06, 0.12);
        }
    }
    if legs {
        saved = save_legs(boneBuf, &joints);
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            feet[side] = g_fkPos[j[2] as usize];
            foot_rot[side] = g_fkRot[j[2] as usize];
            if own > 0.0 {
                let n = native.unwrap();
                let target = foot_target(n, side);
                for k in 0..3 {
                    feet[side][k] += (target[k] - feet[side][k]) * own;
                }
                let q = n.footRot[side];
                let fr = foot_rot[side];
                let sign = if (q[0] * fr[0] + q[1] * fr[1] + q[2] * fr[2] + q[3] * fr[3]) < 0.0 { -1.0f32 } else { 1.0 };
                for k in 0..4 {
                    foot_rot[side][k] += (sign * q[k] - foot_rot[side][k]) * own;
                }
                quat_norm(&mut foot_rot[side]);
            }
            length[side][0] = dist3(&g_fkPos[j[0] as usize], &g_fkPos[j[1] as usize]);
            length[side][1] = dist3(&g_fkPos[j[1] as usize], &g_fkPos[j[2] as usize]);
        }
    }
    let weight = bend_weight(angle);
    let initial_pelvis = g_fkPos[hu];
    let preferred = [initial_pelvis[0], initial_pelvis[1] - 0.045 * weight, initial_pelvis[2] + 0.02 * weight - squat_drop];
    let mut head_bound = s_eyeReferenceValid && head_delta_model.is_some();
    if head_bound {
        for k in 0..3 {
            head_bound = head_bound && head_delta_model.unwrap()[k].is_finite();
        }
    }
    let mut eye_target = [0.0f32; 3];
    if head_bound {
        let hd = head_delta_model.unwrap();
        for k in 0..3 {
            eye_target[k] = s_neutralEyeModel[k] + hd[k] + initial_pelvis[k] - s_referenceModelPos[hu][k];
        }
    }
    let reach_of = |side: usize| {
        let (u, l) = (length[side][0], length[side][1]);
        min((u + l) * 0.999999, sqrt(u * u + l * l + 2.0 * u * l * cos(0.20 * weight)))
    };
    let fit = |candidate: &VrikTorsoCandidate, pelvis: &mut V3| -> f32 {
        if !candidate.valid {
            return 1e6;
        }
        for k in 0..3 {
            pelvis[k] = preferred[k];
            if head_bound {
                pelvis[k] += (eye_target[k] - candidate.eye[k] - pelvis[k]) * weight;
            }
        }
        if head_bound {
            // The body must not get ahead of or above the HMD that drives the view, faded over kBendLimitTime.
            let ahead_limit = eye_target[1] - candidate.eye[1] + 0.01;
            let above_limit = eye_target[2] - candidate.eye[2] + 0.015;
            if pelvis[1] > ahead_limit {
                pelvis[1] += (ahead_limit - pelvis[1]) * limit_weight;
            }
            if pelvis[2] > above_limit {
                pelvis[2] += (above_limit - pelvis[2]) * limit_weight;
            }
        }
        let mut excess = 0.0f32;
        if legs {
            for side in 0..2 {
                let reach = reach_of(side);
                let mut socket = quat_rotate_vec(&candidate.rotations[0], &a3(bt(boneBuf, joints[side * 3])));
                for k in 0..3 {
                    socket[k] += pelvis[k];
                }
                excess = max(excess, dist3(&socket, &feet[side]) - reach);
            }
        }
        excess
    };
    let mut selected = evaluate_torso(boneBuf, flexion, head_rotation);
    if !selected.valid {
        return;
    }
    let mut pelvis = [0.0f32; 3];
    let excess = fit(&selected, &mut pelvis);
    if head_bound && legs && excess > 1e-5 {
        // The closest feasible angle instead of sinking the body below the headset.
        let mut previous = flexion;
        let mut best_excess = excess;
        for step in 1..=32 {
            let trial_angle = flexion * (1.0 - step as f32 / 32.0);
            let trial = evaluate_torso(boneBuf, trial_angle, head_rotation);
            let mut trial_pelvis = [0.0f32; 3];
            let trial_excess = fit(&trial, &mut trial_pelvis);
            if trial_excess < best_excess {
                selected = trial;
                pelvis = trial_pelvis;
                best_excess = trial_excess;
            }
            if trial_excess <= 1e-5 {
                let (mut low, mut high) = (trial_angle, previous);
                selected = trial;
                pelvis = trial_pelvis;
                for _ in 0..10 {
                    let mid = (low + high) * 0.5;
                    let middle = evaluate_torso(boneBuf, mid, head_rotation);
                    let mut at = [0.0f32; 3];
                    if fit(&middle, &mut at) <= 1e-5 {
                        low = mid;
                        selected = middle;
                        pelvis = at;
                    } else {
                        high = mid;
                    }
                }
                break;
            }
            previous = trial_angle;
        }
    }
    let parent = g_VRBoneParent[hu] as i32;
    let place_pelvis = |pelvis: &V3, selected: &VrikTorsoCandidate| {
        write_local_pos(boneBuf, hips, &parent_pos(parent), &parent_rot(parent), pelvis);
        write_local_rot(boneBuf, hips, &parent_rot(parent), &selected.rotations[0]);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    };
    place_pelvis(&pelvis, &selected);
    if legs {
        let mut lower = 0.0f32;
        for side in 0..2 {
            let reach = reach_of(side);
            let hip = g_fkPos[joints[side * 3] as usize];
            let (dx, dy) = (hip[0] - feet[side][0], hip[1] - feet[side][1]);
            let max_z = feet[side][2] + sqrt(max(0.0, reach * reach - dx * dx - dy * dy));
            lower = min(lower, max_z - hip[2]);
        }
        pelvis[2] += lower;
        place_pelvis(&pelvis, &selected);
    }
    for slot in 1..selected.count as usize {
        let bone = selected.bones[slot];
        let par = g_VRBoneParent[bone as usize] as i32;
        write_local_rot(boneBuf, bone, &parent_rot(par), &selected.rotations[slot]);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }
    if head_bound {
        g_VRIKBodyEyeTarget = eye_target;
        let mut e = quat_rotate_vec(&g_fkRot[head as usize], &s_eyeInHead);
        for k in 0..3 {
            e[k] += g_fkPos[head as usize][k];
        }
        g_VRIKBodyEyeSolved = e;
        g_VRIKBodyEyeBound = true;
    }
    if legs {
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            let mut pole = [0.0f32, 1.0, 0.0];
            // there, the knee on the animation's side of the leg too (a crouch's knees splay out); else ahead
            if own > 0.0 {
                let n = knee_pole(native.unwrap(), side);
                let len = sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
                if len > 1e-4 {
                    for k in 0..3 {
                        pole[k] += (n[k] / len - pole[k]) * own;
                    }
                }
            }
            solve_leg(boneBuf, j[0], j[1], j[2], &feet[side], &pole);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
            write_local_rot(boneBuf, j[2], &g_fkRot[j[1] as usize], &foot_rot[side]);
            for k in 0..3 {
                s_bendLegOwned[j[k] as usize] = true;
            }
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }
    if !saved.is_null() {
        let s = &mut *saved;
        for i in 0..s.count as usize {
            copy_bone(&mut s.solved[i], boneBuf, s.bones[i]);
        }
    }
    g_VRIKBodyBendAngle = angle;
    for i in 0..3 {
        g_VRIKBodyBendPelvis[i] = g_fkPos[hu][i] - initial_pelvis[i];
    }
}

/// Passenger combat and swimming: the pelvis moved to fit the neutral upper posture under the HMD centre; seated, the
/// native foot contacts kept.
unsafe fn place_head_anchored_body(boneBuf: *mut u8, camera_target: *const f32, head_rotation: *const f32, head_idx: i32, vehicle: bool) -> bool {
    if boneBuf.is_null()
        || camera_target.is_null()
        || head_rotation.is_null()
        || !g_VRHeadReferenceValid
        || !s_eyeReferenceValid
        || head_idx < 0
        || head_idx >= g_VRBoneCount
        || g_VRSpineCount <= 0
    {
        return false;
    }
    let camera_target = a3(camera_target);
    for k in 0..3 {
        if !camera_target[k].is_finite() {
            return false;
        }
    }
    let head_rotation = a4(head_rotation);
    let mut norm = 0.0f32;
    for k in 0..4 {
        if !head_rotation[k].is_finite() {
            return false;
        }
        norm += head_rotation[k] * head_rotation[k];
    }
    if norm < 0.5 || norm > 1.5 {
        return false;
    }
    let mut head_q = quat_mul(&head_rotation, &g_VRHeadReferenceModelRot);
    quat_norm(&mut head_q);
    let hips = g_VRHipsIdx;
    let mut chain = [0i32; 12];
    let mut count = 0usize;
    let mut bone = head_idx;
    while bone >= 0 && bone < g_VRBoneCount && count < 12 {
        if !s_referenceValid[bone as usize] {
            return false;
        }
        chain[count] = bone;
        count += 1;
        if bone == hips {
            break;
        }
        bone = g_VRBoneParent[bone as usize] as i32;
    }
    if count < 3 || chain[count - 1] != hips {
        return false;
    }
    chain[..count].reverse();
    let joints = leg_joints();
    if vehicle {
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            for k in 0..3 {
                if j[k] < 0 || j[k] >= VRIK_FKCount() {
                    return false;
                }
            }
            if g_VRBoneParent[j[0] as usize] as i32 != hips
                || g_VRBoneParent[j[1] as usize] as i32 != j[0]
                || g_VRBoneParent[j[2] as usize] as i32 != j[1]
            {
                return false;
            }
        }
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let forward = [0.0f32, 1.0, 0.0];
    let mut facing = quat_rotate_vec(&head_rotation, &forward);
    facing[2] = 0.0;
    if norm3(&mut facing) < 1e-4 {
        let right = quat_rotate_vec(&head_rotation, &[1.0, 0.0, 0.0]);
        facing = [-right[1], right[0], 0.0];
        if norm3(&mut facing) < 1e-4 {
            return false;
        }
    }
    // Swimming has its own physical body-yaw cone: the upper body does not follow every head turn there.
    let yaw = if vehicle { atan2(-facing[0], facing[1]) } else { 0.0 };
    if !vehicle {
        facing = [0.0, 1.0, 0.0];
    }
    let heading = [0.0, 0.0, sin(yaw * 0.5), cos(yaw * 0.5)];
    let inverse_heading = quat_conj(&heading);
    let mut relative_head = quat_mul(&inverse_heading, &head_rotation);
    quat_norm(&mut relative_head);
    let camera_from_eyes = rotate_anchor_yaw(s_neckCameraEyeOffset, yaw);
    let eye_target = [camera_target[0] - camera_from_eyes.x, camera_target[1] - camera_from_eyes.y, camera_target[2] - camera_from_eyes.z];

    let mut positions = [[0.0f32; 3]; 12];
    let mut rotations = [[0.0f32; 4]; 12];
    rotations[0] = if vehicle { g_fkRot[hips as usize] } else { s_referenceModelRot[hips as usize] };
    for i in 1..count {
        let bone = chain[i];
        let mut p = quat_rotate_vec(&rotations[i - 1], &s_referenceLocalPos[bone as usize]);
        for k in 0..3 {
            p[k] += positions[i - 1][k];
        }
        positions[i] = p;
        if bone == head_idx {
            rotations[i] = head_q;
            continue;
        }
        let (mut flex, mut head_share) = (0.0f32, 0.0f32);
        for c in 0..g_VRSpineCount {
            if g_VRSpineIdx[c as usize] == bone {
                flex = spine_flex(c, g_VRSpineCount);
                if c == g_VRSpineCount - 1 {
                    for k in 0..3 {
                        positions[i][k] -= facing[k] * CHEST_RETRACTION;
                    }
                }
            }
        }
        if bone == g_VRNeckIdx {
            flex = NECK_FLEX;
            head_share = 0.25;
        }
        if bone == g_VRNeck1Idx {
            flex = UPPER_NECK_FLEX;
            head_share = 0.55;
        }
        let bend = [sin(flex * 0.5), 0.0, 0.0, cos(flex * 0.5)];
        let partial = quat_scale(&relative_head, head_share);
        let tilted = quat_mul(&partial, &bend);
        let world = quat_mul(&heading, &tilted);
        rotations[i] = quat_mul(&world, &s_referenceModelRot[bone as usize]);
        quat_norm(&mut rotations[i]);
    }
    let eye = quat_rotate_vec(&head_q, &s_eyeInHead);
    let mut pelvis = [0.0f32; 3];
    for k in 0..3 {
        pelvis[k] = eye_target[k] - positions[count - 1][k] - eye[k];
    }
    let mut feet = [[0.0f32; 3]; 2];
    let mut foot_rot = [[0.0f32; 4]; 2];
    let mut poles = [[0.0f32; 3]; 2];
    let saved = if vehicle { save_vehicle_lower(boneBuf) } else { core::ptr::null_mut() };
    if vehicle {
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            feet[side] = g_fkPos[j[2] as usize];
            foot_rot[side] = g_fkRot[j[2] as usize];
            poles[side] = sub3(&g_fkPos[j[1] as usize], &g_fkPos[j[0] as usize]);
            if norm3(&mut poles[side]) < 1e-6 {
                poles[side] = forward;
            }
        }
    }
    for i in 0..count {
        let bone = chain[i];
        let parent = g_VRBoneParent[bone as usize] as i32;
        let target = add3(&pelvis, &positions[i]);
        write_local_pos(boneBuf, bone, &parent_pos(parent), &parent_rot(parent), &target);
        write_local_rot(boneBuf, bone, &parent_rot(parent), &rotations[i]);
        g_fkPos[bone as usize] = target;
        g_fkRot[bone as usize] = rotations[i];
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    if vehicle {
        for side in 0..2 {
            let j = &joints[side * 3..side * 3 + 3];
            solve_leg(boneBuf, j[0], j[1], j[2], &feet[side], &poles[side]);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
            write_local_rot(boneBuf, j[2], &g_fkRot[j[1] as usize], &foot_rot[side]);
            for k in 0..3 {
                s_bendLegOwned[j[k] as usize] = true;
            }
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }
    if !saved.is_null() {
        let s = &mut *saved;
        for i in 0..s.count as usize {
            copy_bone(&mut s.solved[i], boneBuf, s.bones[i]);
        }
    }
    g_VRIKBodyEyeTarget = eye_target;
    let mut e = quat_rotate_vec(&g_fkRot[head_idx as usize], &s_eyeInHead);
    for k in 0..3 {
        e[k] += g_fkPos[head_idx as usize][k];
    }
    g_VRIKBodyEyeSolved = e;
    g_VRIKBodyEyeBound = true;
    g_VRIKVehicleUpperActive = vehicle;
    VRIK_PublishBodyBones();
    true
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_PlaceVehicleCombatUpper(boneBuf: *mut u8, cameraTarget: *const f32, headRotation: *const f32, headIdx: i32) -> bool {
    place_head_anchored_body(boneBuf, cameraTarget, headRotation, headIdx, true)
}

/// SEATED IN A CAR OR ON A MOTORCYCLE the spine bends with the tracked head (main's cccb014f7): the authored car / bike
/// posture is kept and only the difference between its head orientation and the HMD's is spread over the upper chain;
/// the eye follows this packet's VR displacement from the authored seat camera, by a bounded CCD that keeps every
/// segment's native length (the seat owns the pelvis); native wheel / handlebar hands keep their contact.
#[no_mangle]
pub unsafe extern "C" fn VRIK_PlaceSeatedUpper(boneBuf: *mut u8, cameraTarget: *const f32, cameraBase: *const f32, headRotation: *const f32,
                                               headIdx: i32, keepRightHand: bool, keepLeftHand: bool) -> bool {
    let hips = g_VRHipsIdx;
    if boneBuf.is_null() || cameraTarget.is_null() || cameraBase.is_null() || headRotation.is_null() || !s_eyeReferenceValid
        || !g_VRHeadReferenceValid || hips < 0 || hips >= g_VRBoneCount || headIdx < 0 || headIdx >= g_VRBoneCount
        || g_VRSpineCount <= 0
    {
        return false;
    }
    let (ct, cb, hr) = (a3(cameraTarget), a3(cameraBase), a4(headRotation));
    if ct.iter().chain(cb.iter()).any(|v| !v.is_finite()) {
        return false;
    }
    if hr.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let norm: f32 = hr.iter().map(|v| v * v).sum();
    if !(0.5..=1.5).contains(&norm) {
        return false;
    }
    let mut chain = [0i32; 12];
    let mut count = 0usize;
    let mut bone = headIdx;
    while bone >= 0 && bone < g_VRBoneCount && bone != hips && count < 12 {
        chain[count] = bone;
        count += 1;
        bone = g_VRBoneParent[bone as usize] as i32;
    }
    if count == 0 || g_VRBoneParent[chain[count - 1] as usize] as i32 != hips {
        return false;
    }
    chain[..count].reverse();
    let saved = save_seated_pose(boneBuf);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let hu = hips as usize;
    let hd = headIdx as usize;
    let mut native_eyes = quat_rotate_vec(&g_fkRot[hd], &s_eyeInHead);
    for k in 0..3 {
        native_eyes[k] += g_fkPos[hd][k];
    }
    let hands = [g_VRRightBoneIdx, g_VRLeftBoneIdx];
    let keep = [keepRightHand, keepLeftHand];
    let mut hand_position = [[0.0f32; 3]; 2];
    let mut hand_rotation = [[0.0f32; 4]; 2];
    for side in 0..2 {
        if keep[side] && hands[side] >= 0 && hands[side] < VRIK_FKCount() {
            hand_position[side] = g_fkPos[hands[side] as usize];
            hand_rotation[side] = g_fkRot[hands[side] as usize];
        }
    }
    let mut head_q = quat_mul(&hr, &g_VRHeadReferenceModelRot);
    quat_norm(&mut head_q);
    let mut head_delta = quat_mul(&head_q, &quat_conj(&g_fkRot[hd]));
    quat_norm(&mut head_delta);
    let mut local = [[0.0f32; 4]; 12];
    let mut base = [[0.0f32; 4]; 12];
    let mut offset = [[0.0f32; 3]; 12];
    let mut position = [[0.0f32; 3]; 12];
    let mut rotation = [[0.0f32; 4]; 12];
    let mut limit = [0.0f32; 12];
    for i in 0..count {
        let bone = chain[i];
        let mut share = 0.0f32;
        offset[i] = a3(bt(boneBuf, bone));
        for spine in 0..g_VRSpineCount {
            if g_VRSpineIdx[spine as usize] == bone {
                share = 0.15 * (spine + 1) as f32 / g_VRSpineCount as f32;
                limit[i] = 0.14 + 0.21 * spine as f32 / 1.max(g_VRSpineCount - 1) as f32;
            }
        }
        if bone == g_VRNeckIdx {
            (share, limit[i]) = (0.35, 0.20);
        }
        if bone == g_VRNeck1Idx {
            (share, limit[i]) = (0.65, 0.25);
        }
        if bone == headIdx {
            share = 1.0;
        }
        let part = quat_scale(&head_delta, share);
        rotation[i] = quat_mul(&part, &g_fkRot[bone as usize]);
        quat_norm(&mut rotation[i]);
        let inverse = quat_conj(if i != 0 { &rotation[i - 1] } else { &g_fkRot[hu] });
        local[i] = quat_mul(&inverse, &rotation[i]);
        quat_norm(&mut local[i]);
        base[i] = local[i];
    }
    let evaluate = |position: &mut [[f32; 3]; 12], rotation: &mut [[f32; 4]; 12], local: &[[f32; 4]; 12]| {
        for i in 0..count {
            let q = if i != 0 { rotation[i - 1] } else { g_fkRot[hu] };
            let p = if i != 0 { position[i - 1] } else { g_fkPos[hu] };
            let mut at = quat_rotate_vec(&q, &offset[i]);
            for k in 0..3 {
                at[k] += p[k];
            }
            position[i] = at;
            rotation[i] = quat_mul(&q, &local[i]);
            quat_norm(&mut rotation[i]);
        }
    };
    evaluate(&mut position, &mut rotation, &local);
    let eye = quat_rotate_vec(&head_q, &s_eyeInHead);
    // vehicle cameras do not use the on-foot neck bake: this packet's VR displacement from the authored seat camera,
    // keeping the car's or bike's own neutral posture even with its camera off V
    let mut target = [0.0f32; 3];
    for k in 0..3 {
        g_VRIKBodyEyeTarget[k] = native_eyes[k] + ct[k] - cb[k];
        target[k] = g_VRIKBodyEyeTarget[k] - eye[k];
    }
    let error_squared = |position: &[[f32; 3]; 12]| -> f32 {
        let mut e = 0.0;
        for k in 0..3 {
            let d = position[count - 1][k] - target[k];
            e += d * d;
        }
        e
    };
    let mut previous_error = error_squared(&position);
    let mut best_error = previous_error;
    let mut best = local;
    for _pass in 0..24 {
        let mut i = count as i32 - 2;
        while i >= 0 {
            let iu = i as usize;
            if limit[iu] > 0.0 {
                let mut current = [0.0f32; 3];
                let mut desired = [0.0f32; 3];
                for k in 0..3 {
                    current[k] = position[count - 1][k] - position[iu][k];
                    desired[k] = target[k] - position[iu][k];
                }
                if norm3(&mut current) >= 0.01 && norm3(&mut desired) >= 0.01 {
                    let delta = quat_from_to(&current, &desired);
                    let step = quat_scale(&delta, 0.65);
                    let model = quat_mul(&step, &rotation[iu]);
                    let inverse = quat_conj(if iu != 0 { &rotation[iu - 1] } else { &g_fkRot[hu] });
                    let next = quat_mul(&inverse, &model);
                    let mut relative = quat_mul(&next, &quat_conj(&base[iu]));
                    quat_norm(&mut relative);
                    let angle = 2.0 * acos(clamp(relative[3].abs(), 0.0, 1.0));
                    let limited = quat_scale(&relative, if angle > limit[iu] { limit[iu] / angle } else { 1.0 });
                    local[iu] = quat_mul(&limited, &base[iu]);
                    quat_norm(&mut local[iu]);
                    evaluate(&mut position, &mut rotation, &local);
                }
            }
            i -= 1;
        }
        let error = error_squared(&position);
        if error < best_error {
            best_error = error;
            best = local;
        }
        if error < 1e-6 || (previous_error - error).abs() < 1e-9 {
            break;
        }
        previous_error = error;
    }
    local = best;
    evaluate(&mut position, &mut rotation, &local);
    let inverse = quat_conj(if count > 1 { &rotation[count - 2] } else { &g_fkRot[hu] });
    local[count - 1] = quat_mul(&inverse, &head_q);
    for i in 0..count {
        put4(br(boneBuf, chain[i]), &local[i]);
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    g_VRIKVehicleUpperActive = true;
    // native wheel / handlebar hands keep their contact while the chest moves
    let (mut right, mut up, mut forward) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    if super::cache::VRIK_BodyAxesFromRig(right.as_mut_ptr(), up.as_mut_ptr(), forward.as_mut_ptr(), true) {
        for side in 0..2 {
            if keep[side] && hands[side] >= 0 && hands[side] < VRIK_FKCount() {
                super::arm::VRIK_SolveArm(boneBuf, if side != 0 { g_VRLeftUpperArmIdx } else { g_VRRightUpperArmIdx },
                    if side != 0 { g_VRLeftForeArmIdx } else { g_VRRightForeArmIdx }, hands[side], hand_position[side].as_ptr(),
                    hand_rotation[side].as_ptr(), right.as_ptr(), up.as_ptr(), forward.as_ptr(), 0.0, 1.0, side != 0, false, false);
            }
        }
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let mut solved = quat_rotate_vec(&g_fkRot[hd], &s_eyeInHead);
    for k in 0..3 {
        solved[k] += g_fkPos[hd][k];
    }
    g_VRIKBodyEyeSolved = solved;
    g_VRIKBodyEyeBound = true;
    note_seated_pose(saved, boneBuf);
    VRIK_PublishBodyBones();
    true
}

/// No tuning arguments in gameplay: the baked, approved rung pose. Explicit arguments for offline authoring.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ApplyLadderFingers(boneBuf: *mut u8, side: i32, kind: i32, curls: *const f32, adjustments: *const [f32; 6]) {
    if boneBuf.is_null() || !(0..=1).contains(&side) || !(1..=3).contains(&kind) {
        return;
    }
    let (mut curls, mut adjustments) = (curls, adjustments);
    if kind == 2 && curls.is_null() {
        curls = ladder::RUNG_CURL.as_ptr();
        adjustments = ladder::RUNG_ADJUST[side as usize].as_ptr();
    }
    for slot in 0..19 {
        let bone = s_ladderFingers[side as usize][slot];
        if bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
            continue;
        }
        let finger = &ladder::FINGERS[slot];
        let q = br(boneBuf, bone);
        if kind != 2 || curls.is_null() {
            put4(q, &finger.rotation[side as usize]);
            continue;
        }
        let value = *curls.add(finger.group);
        let amount = clamp(if value.is_finite() { value } else { 1.0 }, 0.0, 1.5);
        let reference = s_referenceLocalRot[bone as usize];
        let inverse = quat_conj(&reference);
        let delta = quat_mul(&finger.rotation[side as usize], &inverse);
        let part = quat_scale(&delta, amount);
        let mut r = quat_mul(&part, &reference);
        quat_norm(&mut r);
        put4(q, &r);
        if !adjustments.is_null() {
            let angles = ladder::finger_adjustment_angles(finger.suffix, finger.group, Some(&*adjustments.add(finger.group)));
            for axis in 0..3 {
                if angles[axis] != 0.0 {
                    let half = angles[axis] * 0.00872664626;
                    let mut offset = [0.0, 0.0, 0.0, cos(half)];
                    offset[axis] = sin(half);
                    let mut result = quat_mul(&a4(q), &offset);
                    quat_norm(&mut result);
                    put4(q, &result);
                }
            }
        }
    }
}

/// The ladder: the native pelvis and legs kept, the spine fitted (bounded CCD) to the HMD's position, the head
/// following its orientation.
#[no_mangle]
pub unsafe extern "C" fn VRIK_PlaceLadderUpper(boneBuf: *mut u8, cameraTarget: *const f32, headRotation: *const f32, headIdx: i32) {
    let hips = g_VRHipsIdx;
    if boneBuf.is_null()
        || headRotation.is_null()
        || hips < 0
        || hips >= g_VRBoneCount
        || headIdx < 0
        || headIdx >= g_VRBoneCount
        || !s_referenceValid[hips as usize]
        || !g_VRHeadReferenceValid
    {
        return;
    }
    let head_rotation = a4(headRotation);
    s_squatEMA = 0.0;
    s_squatInit = false;
    s_vrSharedSquatDrop = 0.0;
    s_bendLimitW = 0.0;
    s_bendLimitUs = 0;
    s_bendEased = 0.0;
    g_VRIKBodyBendAngle = 0.0;
    g_VRIKBodyBendPelvis = [0.0; 3];
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let hu = hips as usize;
    let inverse = quat_conj(&s_referenceModelRot[hu]);
    let hip_delta = quat_mul(&g_fkRot[hu], &inverse);
    let inverse_delta = quat_conj(&hip_delta);
    let relative_head = quat_mul(&inverse_delta, &head_rotation);
    let mut chain = [0i32; 12];
    let mut count = 0usize;
    let mut bone = headIdx;
    while bone >= 0 && bone < g_VRBoneCount && bone != hips && count < 12 {
        chain[count] = bone;
        count += 1;
        bone = g_VRBoneParent[bone as usize] as i32;
    }
    if count == 0 || g_VRBoneParent[chain[count - 1] as usize] as i32 != hips {
        return;
    }
    chain[..count].reverse();
    for i in 0..count {
        if !s_referenceValid[chain[i] as usize] {
            return;
        }
    }
    let mut local = [[0.0f32; 4]; 12];
    let mut base_local = [[0.0f32; 4]; 12];
    let mut positions = [[0.0f32; 3]; 12];
    let mut rotations = [[0.0f32; 4]; 12];
    let mut limits = [0.0f32; 12];
    // Every solve starts from the same anatomical chain attached to the native pelvis.
    for i in 0..count {
        let bone = chain[i];
        let (mut share, mut flex) = (0.0f32, 0.0f32);
        for spine in 0..g_VRSpineCount {
            if g_VRSpineIdx[spine as usize] == bone {
                flex = spine_flex(spine, g_VRSpineCount);
                share = 0.15 * (spine + 1) as f32 / g_VRSpineCount as f32;
                limits[i] = 0.14 + 0.21 * spine as f32 / 1.max(g_VRSpineCount - 1) as f32;
            }
        }
        if bone == g_VRNeckIdx {
            share = 0.35;
            flex = NECK_FLEX;
        }
        if bone == g_VRNeck1Idx {
            share = 0.65;
            flex = UPPER_NECK_FLEX;
        }
        if bone == headIdx {
            share = 1.0;
            flex = 0.0;
        }
        let bend = [sin(flex * 0.5), 0.0, 0.0, cos(flex * 0.5)];
        let part = quat_scale(&relative_head, share);
        let tilt = quat_mul(&part, &bend);
        let heading = quat_mul(&hip_delta, &tilt);
        let model = quat_mul(&heading, &s_referenceModelRot[bone as usize]);
        let inv = quat_conj(if i != 0 { &rotations[i - 1] } else { &g_fkRot[hu] });
        local[i] = quat_mul(&inv, &model);
        quat_norm(&mut local[i]);
        base_local[i] = local[i];
        rotations[i] = model;
    }
    let evaluate = |positions: &mut [[f32; 3]; 12], rotations: &mut [[f32; 4]; 12], local: &[[f32; 4]; 12]| {
        for i in 0..count {
            let parent_q = if i != 0 { rotations[i - 1] } else { g_fkRot[hu] };
            let parent_p = if i != 0 { positions[i - 1] } else { g_fkPos[hu] };
            let mut p = quat_rotate_vec(&parent_q, &s_referenceLocalPos[chain[i] as usize]);
            for k in 0..3 {
                p[k] += parent_p[k];
            }
            positions[i] = p;
            rotations[i] = quat_mul(&parent_q, &local[i]);
            quat_norm(&mut rotations[i]);
        }
    };
    evaluate(&mut positions, &mut rotations, &local);
    let fit = !cameraTarget.is_null()
        && s_eyeReferenceValid
        && (*cameraTarget).is_finite()
        && (*cameraTarget.add(1)).is_finite()
        && (*cameraTarget.add(2)).is_finite();
    let mut head_q = quat_mul(&head_rotation, &s_referenceModelRot[headIdx as usize]);
    quat_norm(&mut head_q);
    if fit {
        // The ladder keeps the native body heading: its camera mount is in model axes, as in AnchorRecipe.
        let mount = s_neckCameraEyeOffset;
        let eye = quat_rotate_vec(&head_q, &s_eyeInHead);
        let ct = a3(cameraTarget);
        g_VRIKBodyEyeTarget = [ct[0] - mount.x, ct[1] - mount.y, ct[2] - mount.z];
        let target = sub3(&g_VRIKBodyEyeTarget, &eye);
        // Bounded CCD over the spine joints (not the pelvis or neck), the upper thoracic ones allowed more.
        for _pass in 0..8 {
            let mut i = count as i32 - 2;
            while i >= 0 {
                let iu = i as usize;
                if limits[iu] > 0.0 {
                    let mut current = sub3(&positions[count - 1], &positions[iu]);
                    let mut desired = sub3(&target, &positions[iu]);
                    if norm3(&mut current) < 0.01 || norm3(&mut desired) < 0.01 {
                        i -= 1;
                        continue;
                    }
                    let delta = quat_from_to(&current, &desired);
                    let step = quat_scale(&delta, 0.65);
                    let model = quat_mul(&step, &rotations[iu]);
                    let inv = quat_conj(if iu != 0 { &rotations[iu - 1] } else { &g_fkRot[hu] });
                    let next = quat_mul(&inv, &model);
                    let base_inverse = quat_conj(&base_local[iu]);
                    let mut relative = quat_mul(&next, &base_inverse);
                    quat_norm(&mut relative);
                    let angle = 2.0 * acos(clamp(relative[3].abs(), 0.0, 1.0));
                    let limited = quat_scale(&relative, if angle > limits[iu] { limits[iu] / angle } else { 1.0 });
                    local[iu] = quat_mul(&limited, &base_local[iu]);
                    quat_norm(&mut local[iu]);
                    evaluate(&mut positions, &mut rotations, &local);
                }
                i -= 1;
            }
        }
    }
    // The head still follows the HMD's orientation exactly while the spine fits its position.
    let inv = quat_conj(if count > 1 { &rotations[count - 2] } else { &g_fkRot[hu] });
    local[count - 1] = quat_mul(&inv, &head_q);
    for i in 0..count {
        put3(bt(boneBuf, chain[i]), &s_referenceLocalPos[chain[i] as usize]);
        put4(br(boneBuf, chain[i]), &local[i]);
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let mut e = quat_rotate_vec(&g_fkRot[headIdx as usize], &s_eyeInHead);
    for k in 0..3 {
        e[k] += g_fkPos[headIdx as usize][k];
    }
    g_VRIKBodyEyeSolved = e;
    g_VRIKBodyEyeBound = fit;
    // Root, Hips and every leg local remain exactly as the native ladder animation supplied them.
    VRIK_PublishBodyBones();
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_PlaceSwimmingBody(boneBuf: *mut u8, cameraTarget: *const f32, headRotation: *const f32, headIdx: i32) -> bool {
    VRIK_BeginBodySolve(boneBuf);
    s_squatEMA = 0.0;
    s_squatInit = false;
    s_vrSharedSquatDrop = 0.0;
    s_bendLimitW = 0.0;
    s_bendLimitUs = 0;
    s_bendEased = 0.0;
    // Fit from the actual eye centre, including the water camera's translation.
    place_head_anchored_body(boneBuf, cameraTarget, headRotation, headIdx, false)
}

/// The clip's torso for this buffer: read now, or -- the buffer still holding this frame's output -- as read before.
unsafe fn clip_torso(boneBuf: *mut u8, bones: &[i32]) -> *mut NativeTorso {
    let count = bones.len() as i32;
    if count <= 0 || count > 12 || s_referenceCameraBone < 0 || s_referenceCameraBone >= VRIK_FKCount() {
        return core::ptr::null_mut();
    }
    for t in s_nativeTorso.iter_mut() {
        if t.buffer == boneBuf as *const u8
            && t.revision == s_rigRevision
            && t.count == count
            && t.bones[0] == bones[0]
            && *bone_bytes(boneBuf, bones[0]) == t.solved[0]
        {
            CyberpunkVR_DebugClipBody[11] = 1.0;
            return t;
        }
    }
    let mut t: *mut NativeTorso = core::ptr::null_mut();
    for e in s_nativeTorso.iter_mut() {
        if e.buffer == boneBuf as *const u8 {
            t = e;
            break;
        }
    }
    if t.is_null() {
        t = &mut s_nativeTorso[s_nativeTorsoNext as usize];
        s_nativeTorsoNext = (s_nativeTorsoNext + 1) % 4;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let c = g_fkPos[s_referenceCameraBone as usize];
    if !c[0].is_finite() || !c[1].is_finite() || !c[2].is_finite() {
        return core::ptr::null_mut();
    }
    let t = &mut *t;
    t.buffer = boneBuf;
    t.revision = s_rigRevision;
    t.count = count;
    for i in 0..count as usize {
        t.bones[i] = bones[i];
        t.native[i] = *bone_bytes(boneBuf, bones[i]);
        t.solved[i] = *bone_bytes(boneBuf, bones[i]);
    }
    t.camera = c;
    t.hips = g_fkPos[bones[0] as usize];
    CyberpunkVR_DebugClipBody[11] = 0.0;
    t
}

/// THE MOVES THAT ARE THE CLIP'S: a slide (detailed 5, or plain 10), a fall (14), the jumps (18..22) and the
/// landings (23..25).
fn clip_move(detailed: i32, plain: i32) -> bool {
    plain == 10 || detailed == 5 || detailed == 14 || (18..=25).contains(&detailed)
}

/// How much of the body is the clip's, eased over about 80 ms; a move holds it until its camera bone is back near
/// standing height. `engaged` is true on the solve the clip's body comes in.
unsafe fn clip_body_weight(clip: *const NativeTorso, engaged: &mut bool) -> f32 {
    let now = steady_us();
    let dt = if s_clipBodyAtUs != 0 { clamp((now - s_clipBodyAtUs) as f32 * 1e-6, 0.0, 0.1) } else { 0.0 };
    s_clipBodyAtUs = now;
    // Never under full-body tracking: the trackers move the body through those moves as the player makes them.
    let clip_moves = CyberpunkVR_ClipBodyMoves != 0 && !cvr_cpp_vrik_fbt_body_active();
    let moving = clip_moves && !clip.is_null() && clip_move(g_VRLocomotionDetailed, g_VRLocomotionState);
    *engaged = moving && !s_clipLatched && s_clipBodyWeight <= 0.0;
    if moving {
        s_clipLatched = true;
    }
    let mut want = 0.0f32;
    if s_clipLatched && !clip.is_null() && clip_moves {
        let drop = s_referenceCameraHeight - (*clip).camera[2];
        want = if moving { 1.0 } else { clamp((drop - 0.05) / 0.25, 0.0, 1.0) };
    }
    if !moving && want <= 0.0 {
        s_clipLatched = false;
    }
    s_clipBodyWeight += (want - s_clipBodyWeight) * min(1.0, dt / 0.08);
    if (want - s_clipBodyWeight).abs() < 1e-3 {
        s_clipBodyWeight = want;
    }
    s_clipBodyWeight
}

fn quat_nlerp(a: &Q, b: &Q, t: f32) -> Q {
    let s = if (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]) < 0.0 { -1.0f32 } else { 1.0 };
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        out[k] = a[k] + (s * b[k] - a[k]) * t;
    }
    quat_norm(&mut out);
    out
}

/// THE FEET STAY WHERE THE ANIMATION PUTS THEM WHEN THE HIPS GO DOWN: in a squat (faded in over its first 3 cm), and
/// wherever the placed hips stand 6-12 cm above the animation's, the legs solved onto the native feet.
unsafe fn plant_feet(boneBuf: *mut u8, legs: &NativeLegs, squat_drop: f32, clip_weight: f32) {
    if !legs.valid || g_VRHipsIdx < 0 || g_VRHipsIdx >= VRIK_FKCount() {
        return;
    }
    let lift = g_fkPos[g_VRHipsIdx as usize][2] - legs.hips[2];
    let mut weight = max(smooth_range(squat_drop, 0.0, 0.03), smooth_range(lift, 0.06, 0.12));
    weight *= 1.0 - clamp(if clip_weight.is_finite() { clip_weight } else { 0.0 }, 0.0, 1.0);
    if !(weight > 1e-3) {
        return;
    }
    weight = min(weight, 1.0);
    let j = leg_joints();
    // the native legs kept for a pass that feeds this output back in (VRIK_BeginBodySolve), as the bend keeps them
    let saved = save_legs(boneBuf, &j);
    for side in 0..2 {
        let l = &j[side * 3..side * 3 + 3];
        let mut before = [[0.0f32; 4]; 3];
        for i in 0..3 {
            before[i] = a4(br(boneBuf, l[i]));
        }
        let target = foot_target(legs, side);
        let pole = knee_pole(legs, side);
        solve_leg(boneBuf, l[0], l[1], l[2], &target, &pole);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        write_local_rot(boneBuf, l[2], &g_fkRot[l[1] as usize], &legs.footRot[side]);
        if weight < 1.0 {
            for i in 0..3 {
                let rot = br(boneBuf, l[i]);
                let q = quat_nlerp(&before[i], &a4(rot), weight);
                put4(rot, &q);
            }
        }
        for k in 0..3 {
            s_bendLegOwned[l[k] as usize] = true;
        }
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }
    let s = &mut *saved;
    for i in 0..s.count as usize {
        copy_bone(&mut s.solved[i], boneBuf, s.bones[i]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_PlaceBodyUnderHMD(
    boneBuf: *mut u8,
    camModelPos: *const f32,
    camModelRot: *const f32,
    headIdx: i32,
    _bodyFwd: *const f32,
    pairedHeadDeltaModel: *const f32,
    bodyOrbitModel: *const f32,
) {
    if headIdx < 0 || headIdx >= g_VRBoneCount || !g_VRHeadReferenceValid {
        return;
    }
    VRIK_BeginBodySolve(boneBuf);
    let cam_pos = a3(camModelPos);
    let cam_rot = a4(camModelRot);
    // the angle eased (kBendEaseTime), and the bend's limit on the pelvis faded (kBendLimitTime)
    let bend_angle;
    {
        let raw = if SharedPose(115) == 2.0 { SharedPose(114) } else { 0.0 };
        let now = match g_VRBendLimitClockUs {
            Some(clock) => clock(),
            None => steady_us(),
        };
        let dt = if s_bendLimitUs != 0 { clamp((now - s_bendLimitUs) as f32 * 1e-6, 0.0, 0.1) } else { 0.0 };
        s_bendLimitUs = now;
        if !raw.is_finite() {
            s_bendEased = 0.0;
        } else {
            s_bendEased += (raw - s_bendEased) * (if dt > 0.0 { -expm1(-dt / K_BEND_EASE_TIME) } else { 1.0 });
            if raw <= 1e-5 && s_bendEased < 1e-4 {
                s_bendEased = 0.0;
            }
        }
        bend_angle = s_bendEased;
        let rate = dt / K_BEND_LIMIT_TIME;
        s_bendLimitW = if bend_angle > 1e-5 { min(1.0, s_bendLimitW + rate) } else { max(0.0, s_bendLimitW - rate) };
    }
    let hinge = hinge_drop(bend_angle);
    let bend_placed = bend_angle > 1e-5 || s_bendLimitW > 0.0;
    // Real-life SQUAT from the neck-pivot height (shared[90], else the raw HMD height [89]) past the deadzone.
    let mut squat_drop = 0.0f32;
    if !g_pSharedHands.is_null() {
        let mut hy = SharedPose(90);
        if hy == 0.0 {
            hy = SharedPose(89);
        }
        let ordinary_drop = -hy - g_VRSquatThreshold - hinge;
        // During a confirmed floor reach: the neutral 8 cm neck-to-eye height removed, a small crouch threshold.
        let reach_drop = -(hy + 0.08) - 0.02 - hinge;
        let reach_weight = bend_weight(bend_angle);
        let drop = ordinary_drop + (reach_drop - ordinary_drop) * reach_weight;
        if drop > 0.0 {
            squat_drop = drop;
        }
        if squat_drop > 0.7 {
            squat_drop = 0.7;
        }
    }
    // Squat deadzone (2 cm) + EMA (alpha 0.25), shared with the arm anchor through s_vrSharedSquatDrop.
    {
        let (k_squat_dead, k_squat_a) = (0.02f32, 0.25f32);
        if !s_squatInit {
            s_squatEMA = squat_drop;
            s_squatInit = true;
        } else if squat_drop == 0.0 || (squat_drop - s_squatEMA).abs() > k_squat_dead {
            s_squatEMA += (squat_drop - s_squatEMA) * k_squat_a;
            if squat_drop == 0.0 && s_squatEMA < 0.001 {
                s_squatEMA = 0.0;
            }
        }
        squat_drop = s_squatEMA;
    }
    s_vrSharedSquatDrop = squat_drop;
    let mut chain = [0i32; 10];
    let mut chain_n = 0usize;
    let spine_count = g_VRSpineCount.clamp(0, 8);
    for i in 0..spine_count {
        chain[chain_n] = g_VRSpineIdx[i as usize];
        chain_n += 1;
    }
    if g_VRNeckIdx >= 0 {
        chain[chain_n] = g_VRNeckIdx;
        chain_n += 1;
    }
    if g_VRNeck1Idx >= 0 && chain_n < 10 {
        chain[chain_n] = g_VRNeck1Idx;
        chain_n += 1;
    }
    // Own the pelvis, keeping every native leg local transform.
    let hips = g_VRHipsIdx;
    // The clip's torso, read before anything below writes it.
    let mut torso = [0i32; 12];
    let mut torso_n = 0usize;
    if hips >= 0 && hips < g_VRBoneCount {
        torso[torso_n] = hips;
        torso_n += 1;
    }
    let mut c = 0;
    while c < chain_n && torso_n < 12 && torso_n > 0 {
        torso[torso_n] = chain[c];
        torso_n += 1;
        c += 1;
    }
    let clip = clip_torso(boneBuf, &torso[..torso_n]);
    // Where the animation has the feet, before anything below moves the hips: under the animation's hips.
    let mut native_legs;
    if !clip.is_null() && hips >= 0 && (*clip).bones[0] == hips {
        let mine = *bone_bytes(boneBuf, hips);
        *bone_bytes(boneBuf, hips) = (*clip).native[0];
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        native_legs = capture_native_legs();
        *bone_bytes(boneBuf, hips) = mine;
    } else {
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        native_legs = capture_native_legs();
    }
    symmetric_stance(&mut native_legs);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let mut clip_engaged = false;
    let clip_w = clip_body_weight(clip, &mut clip_engaged);
    let orbit = if !bodyOrbitModel.is_null() && (*bodyOrbitModel).is_finite() && (*bodyOrbitModel.add(1)).is_finite() {
        Some([*bodyOrbitModel, *bodyOrbitModel.add(1)])
    } else {
        None
    };
    if hips >= 0 && hips < g_VRBoneCount && s_referenceValid[hips as usize] && s_referenceCameraValid {
        let hu = hips as usize;
        let parent = g_VRBoneParent[hu] as i32;
        // Bent-leg contacts are captured at native standing height first; physical descent is applied in the IK.
        let mut target = [
            s_referenceModelPos[hu][0],
            s_referenceModelPos[hu][1],
            s_referenceModelPos[hu][2] + cam_pos[2] - s_referenceCameraHeight - (if bend_placed { 0.0 } else { squat_drop }),
        ];
        // The avatar placed about the optical centre in model space.
        if let Some(o) = orbit {
            target[0] += o[0];
            target[1] += o[1];
        }
        write_local_pos(boneBuf, hips, &parent_pos(parent), &parent_rot(parent), &target);
        write_local_rot(boneBuf, hips, &parent_rot(parent), &s_referenceModelRot[hu]);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        move_stance(&mut native_legs, &g_fkPos[hu]);
    }
    let spine_root = if spine_count != 0 { g_VRSpineIdx[0] } else { -1 };
    if spine_root >= 0 && spine_root < g_VRBoneCount && s_referenceValid[spine_root as usize] {
        // Reference translations make repeated solves idempotent.
        for c in 0..chain_n {
            let bone = chain[c];
            if bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
                continue;
            }
            put3(bt(boneBuf, bone), &s_referenceLocalPos[bone as usize]);
        }
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        for c in 0..chain_n {
            let bone = chain[c];
            if bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
                continue;
            }
            // Rounded upper spine and a small chest retraction.
            let (mut flex, mut head_share) = (0.0f32, 0.0f32);
            if (c as i32) < spine_count {
                flex = spine_flex(c as i32, spine_count);
            }
            if bone == g_VRNeckIdx {
                flex = NECK_FLEX;
                head_share = 0.25;
            }
            if bone == g_VRNeck1Idx {
                flex = UPPER_NECK_FLEX;
                head_share = 0.55;
            }
            let bend = [sin(flex * 0.5), 0.0, 0.0, cos(flex * 0.5)];
            let head_part = quat_scale(&cam_rot, head_share);
            let bend_head = quat_mul(&head_part, &bend);
            let model_rot = quat_mul(&bend_head, &s_referenceModelRot[bone as usize]);
            let parent = g_VRBoneParent[bone as usize] as i32;
            if c as i32 == spine_count - 1 {
                let p = g_fkPos[bone as usize];
                let chest = [p[0], p[1] - CHEST_RETRACTION, p[2]];
                write_local_pos(boneBuf, bone, &parent_pos(parent), &parent_rot(parent), &chest);
            }
            write_local_rot(boneBuf, bone, &parent_rot(parent), &model_rot);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }
    // A SLIDE, A JUMP, A FALL AND A LANDING ARE THE CLIP'S BODY, where the clip has it about the player, eased.
    if clip_engaged && !clip.is_null() && hips >= 0 && s_referenceValid[hips as usize] {
        s_clipOffset[0] = s_referenceModelPos[hips as usize][0] - (*clip).hips[0];
        s_clipOffset[1] = s_referenceModelPos[hips as usize][1] - (*clip).hips[1];
    }
    if !clip.is_null() && clip_w > 0.0 {
        let clip = &*clip;
        let mut shift = [s_clipOffset[0], s_clipOffset[1], 0.0];
        if let Some(o) = orbit {
            shift[0] += o[0];
            shift[1] += o[1];
        }
        for i in 0..clip.count as usize {
            let bone = clip.bones[i];
            let pos = bt(boneBuf, bone);
            let rot = br(boneBuf, bone);
            let n = &clip.native[i];
            let native_pos = [f32_at(n, 0), f32_at(n, 1), f32_at(n, 2)];
            let native_rot = [f32_at(n, 4), f32_at(n, 5), f32_at(n, 6), f32_at(n, 7)];
            let mut target = native_pos;
            if i == 0 {
                // the shift in the hips' parent frame
                let parent = g_VRBoneParent[bone as usize] as i32;
                let mut local = shift;
                if parent >= 0 {
                    let inv = quat_conj(&g_fkRot[parent as usize]);
                    local = quat_rotate_vec(&inv, &shift);
                }
                for k in 0..3 {
                    target[k] += local[k];
                }
            }
            for k in 0..3 {
                *pos.add(k) += (target[k] - *pos.add(k)) * clip_w;
            }
            let q = quat_nlerp(&a4(rot), &native_rot, clip_w);
            put4(rot, &q);
        }
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }
    CyberpunkVR_DebugClipBody[0] = cam_pos[0];
    CyberpunkVR_DebugClipBody[1] = cam_pos[1];
    CyberpunkVR_DebugClipBody[2] = cam_pos[2];
    if !clip.is_null() {
        CyberpunkVR_DebugClipBody[3..6].copy_from_slice(&(*clip).camera);
        CyberpunkVR_DebugClipBody[6..9].copy_from_slice(&(*clip).hips);
    }
    CyberpunkVR_DebugClipBody[9] = clip_w;
    CyberpunkVR_DebugClipBody[10] = g_VRLocomotionDetailed as f32;
    // A camera's axes are not a skull bone's axes: preserve the reference basis.
    let head_model_rot = quat_mul(&cam_rot, &g_VRHeadReferenceModelRot);
    let hp = g_VRBoneParent[headIdx as usize] as i32;
    write_local_rot(boneBuf, headIdx, &parent_rot(hp), &head_model_rot);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());

    // standing (and walking) the native legs stay exactly as animated
    if !bend_placed {
        plant_feet(boneBuf, &native_legs, squat_drop, clip_w);
    }
    let head_delta = if pairedHeadDeltaModel.is_null() { None } else { Some(a3(pairedHeadDeltaModel)) };
    apply_body_bend(boneBuf, bend_angle, &cam_rot, squat_drop, head_delta.as_ref(), s_bendLimitW, Some(&native_legs));
    VRIK_PublishBodyBones();
    if !clip.is_null() {
        let t = &mut *clip;
        t.solved[0] = *bone_bytes(boneBuf, t.bones[0]);
    }
    // Camera alignment comes from the immutable neutral-neck mount: retire the old animated-head EMA.
    if !g_pSharedHands.is_null() {
        for slot in 116..=119 {
            *g_pSharedHands.add(slot) = 0.0;
        }
    }
    g_VRIKDbgChest = g_fkPos[headIdx as usize];
    g_VRIKDbgChestTgt = g_fkPos[headIdx as usize];
}

/// Swing-twist: the TWIST of q about the unit axis a; identity when the projection degenerates.
#[no_mangle]
pub unsafe extern "C" fn VRIK_TwistAbout(q: *const f32, a: *const f32, outT: *mut f32) {
    let (q, a) = (a4(q), a3(a));
    let d = q[0] * a[0] + q[1] * a[1] + q[2] * a[2];
    let mut t = [a[0] * d, a[1] * d, a[2] * d, q[3]];
    let mut n = t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3];
    if n < 1e-10 {
        put4(outT, &ID);
        return;
    }
    n = 1.0 / sqrt(n);
    for v in t.iter_mut() {
        *v *= n;
    }
    put4(outT, &t);
}

/// 3 mm per sample is ~0.15 m/s at 52 fps: slow enough that timing jitter contributes microns.
const K_SHAKE_SLOW_MM: f32 = 3.0;

/// The second difference of a hand position per stage (0 ctrl, 1 anchor, 2 target, 3 xr), peak per window.
#[no_mangle]
pub unsafe extern "C" fn VRIK_NoteShake(hand: i32, stage: i32, pos: *const f32) {
    if !(0..=1).contains(&hand) || !(0..=3).contains(&stage) || pos.is_null() {
        return;
    }
    let (hu, su) = (hand as usize, stage as usize);
    let p = a3(pos);
    let h = &mut g_vrikShakeHist[hu][su];
    if h.n >= 2 {
        let (mut acc2, mut step2) = (0.0f32, 0.0f32);
        for i in 0..3 {
            let d2 = p[i] - 2.0 * h.p1[i] + h.p2[i];
            acc2 += d2 * d2;
            let d1 = p[i] - h.p1[i];
            step2 += d1 * d1;
        }
        let mm = sqrt(acc2) * 1000.0;
        let step_mm = sqrt(step2) * 1000.0;
        if mm < 500.0 {
            if mm > g_VrikShakePeakMm[hu][su] {
                g_VrikShakePeakMm[hu][su] = mm;
            }
            if step_mm < K_SHAKE_SLOW_MM {
                if mm > g_VrikShakeSlowMm[hu][su] {
                    g_VrikShakeSlowMm[hu][su] = mm;
                }
                g_VrikShakeSlowN[hu][su] += 1;
            }
        }
    }
    for i in 0..3 {
        h.p2[i] = h.p1[i];
        h.p1[i] = p[i];
    }
    if h.n < 2 {
        h.n += 1;
    }
}
