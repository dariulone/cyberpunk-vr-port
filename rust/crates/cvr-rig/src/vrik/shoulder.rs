//! The shoulder girdle: the thorax turn into a grip, the shoulder's anchor under the view, the girdle's reach, the
//! skin helpers of the upper arm and forearm, and the basis helpers the arm's twist uses.
use super::cache::{torso_carry, VRIK_ResetShoulderReference};
use super::env::*;
use super::rig::*;
use super::state::*;
use super::*;
use crate::math::{cross3, dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec, quat_scale};
use cvr_body::bend::smooth_range;
use cvr_body::cmath::{acos, atan2, clamp, cos, max, min, sin, sqrt};

/// THE THORAX TURNS INTO A GRIP THE ARM CANNOT REACH (CyberpunkVR_GripReach): the support wrist's shortfall from the
/// last solve drives a turn of the upper spine about the body's vertical, the two top spine bones carrying it 40/60.
#[no_mangle]
pub unsafe extern "C" fn VRIK_GripTorso(boneBuf: *mut u8, bodyRight: *const f32, bodyUp: *const f32) {
    g_VRIKTorsoOn = false;
    g_VRIKTorsoShift = [0.0; 3];
    // elapsed time, measured here: this runs once per fresh solve, and the passes in between replay it
    let mut dt = 0.016f32;
    {
        let (now, freq) = qpc();
        s_gripTorsoFreq = freq;
        if s_gripTorsoPrev != 0 && s_gripTorsoFreq != 0 {
            dt = ((now - s_gripTorsoPrev) as f64 / s_gripTorsoFreq as f64) as f32;
        }
        s_gripTorsoPrev = now;
        if !(dt > 0.0) || dt > 0.1 {
            dt = 0.016;
        }
    }
    let max_turn = clamp(CyberpunkVR_GripTorsoMaxDeg, 0.0, 35.0) * 0.01745329252;
    // The support hand's side: the left one, or the right one while the weapon is in the left hand.
    let mirrored = super::weapon_arm::cvr_anim_weapon_in_left_hand();
    let support_j = if mirrored { 0 } else { 1 };
    if CyberpunkVR_GripReach != 0 && g_VRIKGrip[support_j] && !g_VRIKVehicleUpperActive {
        // the shoulder sits ~22 cm off the turn's pivot; settles in about a sixth of a second, slew-limited
        let rate = clamp((g_VRIKShort[support_j] + 0.01) / 0.22 * 6.0, -1.5, 1.5);
        s_turn = clamp(s_turn + rate * dt, 0.0, max_turn);
    } else {
        s_turn -= s_turn * min(1.0, dt / 0.20);
        if s_turn < 1e-4 {
            s_turn = 0.0;
        }
    }
    CyberpunkVR_DebugGripTorsoDeg = s_turn * 57.2957795;
    let count = g_VRSpineCount.clamp(0, 8);
    let upper = if mirrored { g_VRRightUpperArmIdx } else { g_VRLeftUpperArmIdx };
    if s_turn <= 0.0 || count < 2 || upper < 0 || upper >= g_VRBoneCount || bodyRight.is_null() || bodyUp.is_null() {
        return;
    }
    let bones = [g_VRSpineIdx[(count - 2) as usize], g_VRSpineIdx[(count - 1) as usize]];
    let share = [0.4f32, 0.6];
    // about -up turns the left shoulder forward (about +up the right one); about -right leans the chest over the reach
    let turn = if mirrored { 1.0f32 } else { -1.0 };
    let (r, u) = (a3(bodyRight), a3(bodyUp));
    let mut axis = [turn * u[0] - 0.33 * r[0], turn * u[1] - 0.33 * r[1], turn * u[2] - 0.33 * r[2]];
    if norm3(&mut axis) < 1e-4 {
        return;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    for i in 0..2 {
        let bone = bones[i];
        if bone < 0 || bone >= g_VRBoneCount {
            return;
        }
        let half = s_turn * share[i] * 0.5;
        let sn = sin(half);
        let q = [axis[0] * sn, axis[1] * sn, axis[2] * sn, cos(half)];
        // kept, so the shoulder's ANCHOR can be carried by the same turn
        g_VRIKTorsoQ[i] = q;
        g_VRIKTorsoP[i] = g_fkPos[bone as usize];
        let mut model = quat_mul(&q, &g_fkRot[bone as usize]);
        quat_norm(&mut model);
        let parent = g_VRBoneParent[bone as usize] as i32;
        write_local_rot(boneBuf, bone, &parent_rot(parent), &model);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }
    g_VRIKTorsoBones = bones;
    g_VRIKTorsoOn = true;
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_AnchorShoulder(
    boneBuf: *mut u8,
    upperIdx: i32,
    anchor: *const f32,
    isLeft: bool,
    bodyRight: *const f32,
    bodyUp: *const f32,
    halfWidth: f32,
    outJoint: *mut f32,
    neckAnchor: bool,
) {
    let mut half = halfWidth.abs();
    if half < 0.13 {
        half = 0.13;
    }
    if half > 0.19 {
        half = 0.19;
    }
    let drop = 0.17f32;
    let side = if isLeft { -1.0f32 } else { 1.0 };
    let (anchor, body_right, body_up) = (a3(anchor), a3(bodyRight), a3(bodyUp));
    let mut desired = [
        anchor[0] + body_right[0] * (side * half) - body_up[0] * drop,
        anchor[1] + body_right[1] * (side * half) - body_up[1] * drop,
        anchor[2] + body_right[2] * (side * half) - body_up[2] * drop,
    ];
    if neckAnchor
        && upperIdx >= 0
        && upperIdx < g_VRBoneCount
        && g_VRNeckIdx >= 0
        && g_VRNeckIdx < g_VRBoneCount
        && s_referenceValid[upperIdx as usize]
        && s_referenceValid[g_VRNeckIdx as usize]
    {
        // The body anchor is the neck: the rig's own neck-to-shoulder height and depth with the calibrated width.
        let forward = cross3(&body_up, &body_right);
        let (up, nk) = (s_referenceModelPos[upperIdx as usize], s_referenceModelPos[g_VRNeckIdx as usize]);
        let height = up[2] - nk[2];
        let depth = up[1] - nk[1];
        for k in 0..3 {
            desired[k] = anchor[k] + body_right[k] * (side * half) + body_up[k] * height + forward[k] * depth;
        }
    }
    // ...carried with the thorax while a grip has turned it (VRIK_GripTorso), the support shoulder only.
    let turned = isLeft != super::weapon_arm::cvr_anim_weapon_in_left_hand() && g_VRIKTorsoOn;
    if turned {
        let mut carried = desired;
        torso_carry(&mut carried);
        for k in 0..3 {
            g_VRIKTorsoShift[k] = carried[k] - desired[k];
            desired[k] = carried[k];
        }
        CyberpunkVR_DebugGripReach[0] = sqrt(dot3(&g_VRIKTorsoShift, &g_VRIKTorsoShift)) * 1000.0;
    }
    let clavi = if (0..MAXB).contains(&upperIdx) { g_VRBoneParent[upperIdx as usize] as i32 } else { -1 };
    let dbg = if isLeft { 1 } else { 0 };
    g_VRIKDbgClav[dbg][0] = desired[0];
    g_VRIKDbgClav[dbg][1] = desired[1];
    g_VRIKDbgClav[dbg][2] = desired[2];
    g_VRIKDbgClav[dbg][6] = 0.0;
    g_VRIKDbgClav[dbg][7] = 0.0;
    if (0..MAXB).contains(&clavi) {
        // Start the clavicle aim at the rig's actual pivot/basis.
        VRIK_ResetShoulderReference(boneBuf, upperIdx);
        let pv = g_fkPos[clavi as usize];
        let up = g_fkPos[upperIdx as usize];
        let mut cur = [up[0] - pv[0], up[1] - pv[1], up[2] - pv[2]];
        let mut des = [desired[0] - pv[0], desired[1] - pv[1], desired[2] - pv[2]];
        if norm3(&mut cur) > 1e-4 && norm3(&mut des) > 1e-4 {
            let mut d4 = quat_from_to(&cur, &des);
            if d4[3] < 0.0 {
                d4 = [-d4[0], -d4[1], -d4[2], -d4[3]];
            }
            // std::fmin: the other operand for a NaN, as f32::min
            let ang = 2.0 * acos(1.0f32.min(d4[3]));
            let k_max_clav = 1.3090f32; // 75 deg
            let mut applied = ang;
            if ang > k_max_clav && ang > 1e-4 {
                d4 = quat_scale(&d4, k_max_clav / ang);
                applied = k_max_clav;
            }
            g_VRIKDbgClav[dbg][6] = ang * 57.29578;
            g_VRIKDbgClav[dbg][7] = applied * 57.29578;
            let mut nm = quat_mul(&d4, &g_fkRot[clavi as usize]);
            quat_norm(&mut nm);
            let pp = g_VRBoneParent[clavi as usize] as i32;
            let prot = if pp >= 0 && pp < MAXB { g_fkRot[pp as usize] } else { ID };
            write_local_rot(boneBuf, clavi, &prot, &nm);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }
    let mut joint = g_fkPos[upperIdx as usize];
    if turned {
        for k in 0..3 {
            joint[k] -= g_VRIKTorsoShift[k];
        }
    }
    put3(outJoint, &joint);
    if clavi >= 0 && clavi < upperIdx {
        let base = &mut s_shoulderBase[dbg];
        base.buffer = boneBuf;
        base.revision = s_rigRevision;
        base.upper = upperIdx;
        base.clavicle = clavi;
        base.clavicleLocalRot = a4(br(boneBuf, clavi));
        base.upperLocalPos = a3(bt(boneBuf, upperIdx));
    }
    g_VRIKDbgClav[dbg][3] = joint[0];
    g_VRIKDbgClav[dbg][4] = joint[1];
    g_VRIKDbgClav[dbg][5] = joint[2];
    if !isLeft {
        // Trace probes: right shoulder joint (model) + hips MODEL yaw.
        g_VRIKDbgShModel = joint;
        let hb = g_VRHipsIdx;
        if (0..MAXB).contains(&hb) {
            let q = g_fkRot[hb as usize];
            let ax_x = 1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2]);
            let ax_y = 2.0 * (q[0] * q[1] + q[2] * q[3]);
            g_VRIKDbgHipsYaw = atan2(ax_y, ax_x) * 57.29578;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_UpdateForearmDeformation(boneBuf: *mut u8, foreIdx: i32, handIdx: i32, left: bool) {
    if VRIK_UsesNativeCyberware() {
        return;
    }
    if foreIdx < 0
        || handIdx < 0
        || foreIdx >= g_VRBoneCount
        || handIdx >= g_VRBoneCount
        || g_VRBoneParent[handIdx as usize] as i32 != foreIdx
        || !s_referenceValid[handIdx as usize]
    {
        return;
    }
    let mut axis = s_referenceLocalPos[handIdx as usize];
    let rest_length = norm3(&mut axis);
    if rest_length < 1e-4 {
        return;
    }
    let hand_t = a3(bt(boneBuf, handIdx));
    let hand_r = a4(br(boneBuf, handIdx));
    let inverse = quat_conj(&s_referenceLocalRot[handIdx as usize]);
    let mut delta = quat_mul(&hand_r, &inverse);
    quat_norm(&mut delta);
    let mut twist = quat_extract_twist(&delta, &axis);
    if twist[3] < 0.0 {
        for v in twist.iter_mut() {
            *v = -*v;
        }
    }
    let angle = clamp(2.0 * atan2(dot3(&[twist[0], twist[1], twist[2]], &axis), twist[3]), -2.0944, 2.0944);
    let length = sqrt(dot3(&hand_t, &hand_t));
    let weights = [0.2f32, 0.4, 0.6];
    let helpers = if left { g_VRForeTwistL } else { g_VRForeTwistR };
    for slot in 0..3 {
        let bone = helpers[slot];
        if bone < 0 || bone >= g_VRBoneCount || g_VRBoneParent[bone as usize] as i32 != foreIdx || !s_referenceValid[bone as usize] {
            continue;
        }
        let half = angle * weights[slot] * 0.5;
        let s = sin(half);
        let roll = [axis[0] * s, axis[1] * s, axis[2] * s, cos(half)];
        let mut q = quat_mul(&roll, &s_referenceLocalRot[bone as usize]);
        quat_norm(&mut q);
        put4(br(boneBuf, bone), &q);
        // Calibrated segment length changes must also move its skin helpers: keep their radial offset and scale only
        // the distance along the segment.
        let rp = s_referenceLocalPos[bone as usize];
        let along = dot3(&rp, &axis);
        let t = bt(boneBuf, bone);
        for k in 0..3 {
            *t.add(k) = rp[k] + axis[k] * along * (length / rest_length - 1.0);
        }
    }
}

/// Quaternion from an orthonormal basis built on X (primary) + a Y hint (Gram-Schmidt).
pub fn quat_from_axes(x: &V3, y_hint: &V3) -> Q {
    let mut xx = *x;
    norm3(&mut xx);
    let mut y = *y_hint;
    let mut d = dot3(&y, &xx);
    y[0] -= xx[0] * d;
    y[1] -= xx[1] * d;
    y[2] -= xx[2] * d;
    if norm3(&mut y) < 1e-5 {
        y = [xx[1], xx[2], xx[0]];
        d = dot3(&y, &xx);
        y[0] -= xx[0] * d;
        y[1] -= xx[1] * d;
        y[2] -= xx[2] * d;
        norm3(&mut y);
    }
    let z = cross3(&xx, &y);
    let (m00, m01, m02) = (xx[0], y[0], z[0]);
    let (m10, m11, m12) = (xx[1], y[1], z[1]);
    let (m20, m21, m22) = (xx[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    let mut q = [0.0f32; 4];
    if tr > 0.0 {
        let s = sqrt(tr + 1.0) * 2.0;
        q[3] = 0.25 * s;
        q[0] = (m21 - m12) / s;
        q[1] = (m02 - m20) / s;
        q[2] = (m10 - m01) / s;
    } else if m00 > m11 && m00 > m22 {
        let s = sqrt(1.0 + m00 - m11 - m22) * 2.0;
        q[3] = (m21 - m12) / s;
        q[0] = 0.25 * s;
        q[1] = (m01 + m10) / s;
        q[2] = (m02 + m20) / s;
    } else if m11 > m22 {
        let s = sqrt(1.0 + m11 - m00 - m22) * 2.0;
        q[3] = (m02 - m20) / s;
        q[0] = (m01 + m10) / s;
        q[1] = 0.25 * s;
        q[2] = (m12 + m21) / s;
    } else {
        let s = sqrt(1.0 + m22 - m00 - m11) * 2.0;
        q[3] = (m10 - m01) / s;
        q[0] = (m02 + m20) / s;
        q[1] = (m12 + m21) / s;
        q[2] = 0.25 * s;
    }
    quat_norm(&mut q);
    q
}

/// Model rotation R with R*aLoc = aW and R*hLoc = hW: Basis(aW,hW) * Basis(aLoc,hLoc)^-1.
pub fn quat_align_two(a_loc: &V3, h_loc: &V3, a_w: &V3, h_w: &V3) -> Q {
    let qw = quat_from_axes(a_w, h_w);
    let ql = quat_from_axes(a_loc, h_loc);
    let qlc = [-ql[0], -ql[1], -ql[2], ql[3]];
    let mut out = quat_mul(&qw, &qlc);
    quat_norm(&mut out);
    out
}

/// Swing/twist decomposition: the twist component of q around a unit axis (w made non-negative).
pub fn quat_extract_twist(q: &Q, axis: &V3) -> Q {
    let d = q[0] * axis[0] + q[1] * axis[1] + q[2] * axis[2];
    let mut t = [axis[0] * d, axis[1] * d, axis[2] * d, q[3]];
    let n = sqrt(t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3]);
    if n < 1e-6 {
        return ID;
    }
    t[0] /= n;
    t[1] /= n;
    t[2] /= n;
    t[3] /= n;
    if t[3] < 0.0 {
        t = [-t[0], -t[1], -t[2], -t[3]];
    }
    t
}

/// The girdle's share of a reach: the clavicle swings and the glenohumeral joint glides toward a far target, more
/// for a grip that has to arrive. Starts from this solve's calibrated clavicle aim (VRIK_AnchorShoulder).
pub unsafe fn reach_girdle(
    buffer: *mut u8,
    upper: i32,
    target: &V3,
    up: &V3,
    forward: &V3,
    arm_length: f32,
    left: bool,
    ladder_reach: bool,
    grip: bool,
) -> bool {
    let side = if left { 1 } else { 0 };
    let base = s_shoulderBase[side];
    g_VRIKGirdleReach[side] = [0.0; 4];
    if base.buffer != buffer || base.revision != s_rigRevision || base.upper != upper || base.clavicle < 0 || arm_length < 0.1 {
        return false;
    }
    let clavicle = base.clavicle;
    // Always start at this solve's calibrated clavicle aim, not our last reach.
    put4(br(buffer, clavicle), &base.clavicleLocalRot);
    put3(bt(buffer, upper), &base.upperLocalPos);
    g_vrikUpArmRest[side] = base.upperLocalPos;
    g_vrikUpArmRestCap[side] = true;
    VRIK_ComputeFK(buffer, VRIK_FKCount());
    let start = g_fkPos[upper as usize];
    let mut dir = sub3(target, &start);
    let distance = norm3(&mut dir);
    // A GRIP has to arrive: on foot only; the ladder and the seat keep their own profiles.
    let g = grip && !ladder_reach && !g_VRIKVehicleUpperActive;
    let weight = if g {
        smooth_range(distance / arm_length, 0.84, 1.06)
    } else {
        smooth_range(distance / arm_length, if ladder_reach { 0.88 } else { 0.94 }, if ladder_reach { 1.10 } else { 1.14 })
    };
    let front = clamp(dot3(&dir, forward), -0.35, 1.0);
    let rise = clamp(dot3(&dir, up), -0.2, 1.0);
    let protract = if ladder_reach {
        0.06
    } else if g_VRIKVehicleUpperActive {
        0.07
    } else if g {
        clamp(CyberpunkVR_GripProtract, 0.0, 0.12)
    } else {
        0.035
    };
    let overhead = smooth_range(rise, 0.15, 0.9);
    let downward = if !ladder_reach && !g_VRIKVehicleUpperActive { smooth_range(-dot3(&dir, up), 0.3, 0.9) } else { 0.0 };
    let elevate = if ladder_reach {
        0.035
    } else if g_VRIKVehicleUpperActive {
        0.03
    } else {
        0.015 + 0.085 * overhead
    };
    let mut max_clavicle = if ladder_reach {
        0.34906585
    } else if g_VRIKVehicleUpperActive {
        0.38397244
    } else {
        0.20943951 + 0.40142573 * overhead + 0.17453293 * downward
    };
    if g {
        max_clavicle = max(max_clavicle, 0.40142573); // 23 deg of forward swing
    }
    let shoulder_glide = if ladder_reach {
        0.022
    } else if g {
        0.03
    } else {
        0.015 + 0.02 * downward
    };
    let blade_share = if ladder_reach {
        0.75
    } else if g {
        0.8
    } else {
        0.6
    };
    // ...and the share of the reach that runs ACROSS the body, which a protracting scapula follows round the ribs
    let mut across = [0.0f32; 3];
    if g {
        for k in 0..3 {
            across[k] = dir[k] - forward[k] * dot3(&dir, forward) - up[k] * dot3(&dir, up);
        }
    }
    let cpos = g_fkPos[clavicle as usize];
    let mut desired = [0.0f32; 3];
    let mut current = [0.0f32; 3];
    let mut arc = [0.0f32; 3];
    for k in 0..3 {
        desired[k] = start[k]
            + weight * (forward[k] * protract * front + across[k] * protract * 0.4 + up[k] * (elevate * rise - 0.045 * downward));
        current[k] = start[k] - cpos[k];
        arc[k] = desired[k] - cpos[k];
    }
    let mut rotation_delta = ID;
    if norm3(&mut current) > 1e-4 && norm3(&mut arc) > 1e-4 {
        rotation_delta = quat_from_to(&current, &arc);
        if rotation_delta[3] < 0.0 {
            for q in rotation_delta.iter_mut() {
                *q = -*q;
            }
        }
        let mut angle = 2.0 * acos(clamp(rotation_delta[3], 0.0, 1.0));
        if angle > max_clavicle {
            rotation_delta = quat_scale(&rotation_delta, max_clavicle / angle);
            angle = max_clavicle;
        }
        let mut model = quat_mul(&rotation_delta, &g_fkRot[clavicle as usize]);
        quat_norm(&mut model);
        let parent = g_VRBoneParent[clavicle as usize] as i32;
        write_local_rot(buffer, clavicle, &parent_rot(parent), &model);
        VRIK_ComputeFK(buffer, VRIK_FKCount());
        g_VRIKGirdleReach[side][1] = angle * 57.2957795;
    }
    // A small glenohumeral translation supplements the clavicle's arc.
    let mut joint = [0.0f32; 3];
    for k in 0..3 {
        joint[k] = g_fkPos[upper as usize][k] + dir[k] * (shoulder_glide * weight);
    }
    write_local_pos(buffer, upper, &g_fkPos[clavicle as usize], &g_fkRot[clavicle as usize], &joint);
    VRIK_ComputeFK(buffer, VRIK_FKCount());
    let movement = sub3(&g_fkPos[upper as usize], &start);
    let blade_rotation = quat_scale(&rotation_delta, blade_share);
    for slot in 0..s_scapulaCount[side] as usize {
        let bone = s_scapula[side][slot];
        let parent = g_VRBoneParent[bone as usize] as i32;
        if parent < 0 || parent >= bone {
            continue;
        }
        let (pp, pr) = (g_fkPos[parent as usize], g_fkRot[parent as usize]);
        let mut position = quat_rotate_vec(&pr, &s_referenceLocalPos[bone as usize]);
        for k in 0..3 {
            position[k] += pp[k] + movement[k] * blade_share;
        }
        let rotation = quat_mul(&pr, &s_referenceLocalRot[bone as usize]);
        let mut model = quat_mul(&blade_rotation, &rotation);
        quat_norm(&mut model);
        write_local_pos(buffer, bone, &pp, &pr, &position);
        write_local_rot(buffer, bone, &pr, &model);
    }
    VRIK_ComputeFK(buffer, VRIK_FKCount());
    g_VRIKGirdleReach[side][0] = weight;
    g_VRIKGirdleReach[side][2] = sqrt(dot3(&movement, &movement));
    g_VRIKGirdleReach[side][3] = g_VRIKGirdleReach[side][2] * blade_share;
    if left {
        CyberpunkVR_DebugGripReach[1] = weight;
        CyberpunkVR_DebugGripReach[2] = g_VRIKGirdleReach[side][1];
        CyberpunkVR_DebugGripReach[3] = g_VRIKGirdleReach[side][2] * 1000.0;
        CyberpunkVR_DebugGripReach[4] = distance / arm_length;
        CyberpunkVR_DebugGripReach[5] = if g { 1.0 } else { 0.0 };
    }
    true
}

pub unsafe fn update_upper_arm_deformation(buffer: *mut u8, fore: i32, left: bool) {
    if fore < 0 || fore >= g_VRBoneCount || !s_referenceValid[fore as usize] {
        return;
    }
    let mut axis = s_referenceLocalPos[fore as usize];
    let rest = norm3(&mut axis);
    if rest < 1e-4 {
        return;
    }
    let end = a3(bt(buffer, fore));
    let scale = sqrt(dot3(&end, &end)) / rest;
    for bone in s_upperSkin[if left { 1 } else { 0 }] {
        if bone >= 0 && s_referenceValid[bone as usize] {
            let rp = s_referenceLocalPos[bone as usize];
            let along = dot3(&rp, &axis);
            let t = bt(buffer, bone);
            for k in 0..3 {
                *t.add(k) = rp[k] + axis[k] * along * (scale - 1.0);
            }
        }
    }
}

/// How far a hand is held by what it holds (a wrist lock), eased so the elbow swings over instead of jumping.
pub unsafe fn wrist_held_ease(side: usize) -> f32 {
    let (now, freq) = qpc();
    let dt = if s_wristAt[side] != 0 { ((now - s_wristAt[side]) as f64 / freq as f64) as f32 } else { 0.0 };
    s_wristAt[side] = now;
    let want = if g_VRHandRotLockOn[side] != 0 || super::physhands::adjust::holds_body(side) { 1.0f32 } else { 0.0 };
    let step = clamp(dt, 0.0, 0.05) / 0.15;
    s_wristEase[side] += clamp(want - s_wristEase[side], -step, step);
    s_wristEase[side]
}
