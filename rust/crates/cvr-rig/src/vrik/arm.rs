//! The arm: the two-bone solve onto the hand target (VRIK_SolveArm), the hand's stop and wrist lock, the controller's
//! hand target, and the arm's rest translations.
use super::env::*;
use super::rig::*;
use super::shoulder::*;
use super::state::*;
use super::*;
use crate::math::{cross3, dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec};
use cvr_body::cmath::{acos, atan2, clamp, cos, exp, sin, sqrt};

unsafe extern "C" {
    /// Anim/Natives' weapon flag and Anim/Recoil.cpp's class (2 rifle, 4 sniper): the raised rifle's elbow pole.
    static g_hasWeaponEquipped: bool;
    static CyberpunkVR_WeaponClass: i32;
}

/// The reload's owned hand (shared slot 162): the wrist a script claimed.
const SLOT_RELOAD_OWNED_HAND: usize = 162;
/// Per hand: the script's stop (model x y z, valid), the target as drawn (model x y z), whether the wrist is claimed.
#[no_mangle]
pub static mut CyberpunkVR_DebugHandStop: [[f32; 8]; 2] = [[0.0; 8]; 2];

/// std::remainder(float, float): x - n*y with n the quotient rounded to the nearest, ties to even (exact).
fn remainder(x: f32, y: f32) -> f32 {
    let (x, y) = (f64::from(x), f64::from(y));
    let n = (x / y).round_ties_even();
    (x - n * y) as f32
}

/// THE WRIST'S RANGE (main's CharacterRig.cpp). A hand is drawn with the controller's turn wherever the solve put the
/// forearm, so a hand held at a wall while the controller goes on could bend the wrist past any wrist. The turn of the
/// hand against the forearm, from the rig's own straight wrist, is split into its twist about the forearm (the forearm's
/// to carry) and its swing, and the swing held inside an ellipse: flexion 75 deg, extension 70, toward the thumb 25,
/// toward the little finger 35. The axes come from the reference hand: the forearm's line into it, across the knuckles,
/// and the palm's normal on the side the fingers close to. OFF BY DEFAULT: the forearm here is the arm solve's guess (the
/// elbow is not tracked), so a wrist the player really holds within its range read as out of it against the guessed
/// forearm. 1 turns the range back on.
#[no_mangle]
pub static mut CyberpunkVR_WristRange: i32 = 0;
#[derive(Clone, Copy)]
struct WristFrame {
    revision: u64,
    ok: bool,
    straight: Q,
    along: V3,
    across: V3,
    palm: V3,
}
static mut s_wristFrame: [WristFrame; 2] = [WristFrame { revision: !0u64, ok: false, straight: ID, along: ZERO3, across: ZERO3, palm: ZERO3 }; 2];
unsafe fn wrist_frame_of(side: usize, foreIdx: i32, handIdx: i32) -> bool {
    let w = &mut s_wristFrame[side];
    if w.revision == s_rigRevision {
        return w.ok;
    }
    w.revision = s_rigRevision;
    w.ok = false;
    let sd = side as i32;
    let index1 = VRIK_LadderFingerBone(sd, 6);
    let middle1 = VRIK_LadderFingerBone(sd, 7);
    let little1 = VRIK_LadderFingerBone(sd, 9);
    let middle2 = VRIK_LadderFingerBone(sd, 12);
    for b in [foreIdx, handIdx, index1, middle1, little1, middle2] {
        if b < 0 || b >= g_VRBoneCount || b >= MAXB || !s_referenceValid[b as usize] {
            return false;
        }
    }
    let (f, h) = (foreIdx as usize, handIdx as usize);
    let hq = s_referenceModelRot[h];
    let inv_f = quat_conj(&s_referenceModelRot[f]);
    let inv_h = quat_conj(&hq);
    w.straight = quat_mul(&inv_f, &hq);
    quat_norm(&mut w.straight);
    let mut a = sub3(&s_referenceModelPos[h], &s_referenceModelPos[f]);
    let mut d = sub3(&s_referenceModelPos[middle1 as usize], &s_referenceModelPos[h]);
    let mut t = sub3(&s_referenceModelPos[index1 as usize], &s_referenceModelPos[little1 as usize]);
    if norm3(&mut a) < 1e-4 || norm3(&mut d) < 1e-4 {
        return false;
    }
    let td = t[0] * d[0] + t[1] * d[1] + t[2] * d[2];
    for k in 0..3 {
        t[k] -= td * d[k];
    }
    if norm3(&mut t) < 1e-4 {
        return false;
    }
    let mut n = [d[1] * t[2] - d[2] * t[1], d[2] * t[0] - d[0] * t[2], d[0] * t[1] - d[1] * t[0]];
    if norm3(&mut n) < 1e-4 {
        return false;
    }
    // the side the fingers close to: the middle finger's proximal phalanx turned about its own -Z takes its next joint
    // there
    {
        let m = s_referenceModelRot[middle1 as usize];
        let inv_m = quat_conj(&m);
        let rel = sub3(&s_referenceModelPos[middle2 as usize], &s_referenceModelPos[middle1 as usize]);
        let local = quat_rotate_vec(&inv_m, &rel);
        // (-Z) x local
        let moved = [local[1], -local[0], 0.0];
        let out = quat_rotate_vec(&m, &moved);
        if out[0] * n[0] + out[1] * n[1] + out[2] * n[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
    }
    // all three in the hand's own frame, the swing axes square to the forearm's line
    w.along = quat_rotate_vec(&inv_h, &a);
    let mut tl = quat_rotate_vec(&inv_h, &t);
    let mut nl = quat_rotate_vec(&inv_h, &n);
    for v in [&mut tl, &mut nl] {
        let da = v[0] * w.along[0] + v[1] * w.along[1] + v[2] * w.along[2];
        for k in 0..3 {
            v[k] -= da * w.along[k];
        }
        if norm3(v) < 1e-4 {
            return false;
        }
    }
    w.across = tl;
    w.palm = nl;
    w.ok = true;
    true
}
unsafe fn hold_wrist_range(side: usize, foreIdx: i32, handIdx: i32, fore_model: &Q, hand_model: &mut Q) {
    if CyberpunkVR_WristRange == 0 || !wrist_frame_of(side, foreIdx, handIdx) {
        return;
    }
    let w = s_wristFrame[side];
    const DEG: f32 = 0.017_453_29;
    let inv_f = quat_conj(fore_model);
    let rel = quat_mul(&inv_f, hand_model);
    let inv_s = quat_conj(&w.straight);
    let mut dq = quat_mul(&inv_s, &rel);
    quat_norm(&mut dq);
    if dq[3] < 0.0 {
        dq = [-dq[0], -dq[1], -dq[2], -dq[3]];
    }
    // twist about the forearm's line, and the swing that is left (D = swing * twist)
    let p = dq[0] * w.along[0] + dq[1] * w.along[1] + dq[2] * w.along[2];
    let mut twist = [w.along[0] * p, w.along[1] * p, w.along[2] * p, dq[3]];
    let tn = sqrt(twist[0] * twist[0] + twist[1] * twist[1] + twist[2] * twist[2] + twist[3] * twist[3]);
    if tn < 1e-6 {
        return;
    }
    for c in twist.iter_mut() {
        *c /= tn;
    }
    let inv_t = quat_conj(&twist);
    let mut swing = quat_mul(&dq, &inv_t);
    quat_norm(&mut swing);
    if swing[3] < 0.0 {
        swing = [-swing[0], -swing[1], -swing[2], -swing[3]];
    }
    let sn = sqrt(swing[0] * swing[0] + swing[1] * swing[1] + swing[2] * swing[2]);
    if sn < 1e-6 {
        return;
    }
    let angle = 2.0 * atan2(sn, swing[3]);
    let v = [swing[0] / sn * angle, swing[1] / sn * angle, swing[2] / sn * angle];
    // about `across` +: the fingers go away from the palm's side (extension); about `palm` +: toward the thumb
    let mut flex = -(v[0] * w.across[0] + v[1] * w.across[1] + v[2] * w.across[2]);
    let mut dev = v[0] * w.palm[0] + v[1] * w.palm[1] + v[2] * w.palm[2];
    let flex_lim = (if flex > 0.0 { 75.0 } else { 70.0 }) * DEG;
    let dev_lim = (if dev > 0.0 { 25.0 } else { 35.0 }) * DEG;
    let e = (flex / flex_lim) * (flex / flex_lim) + (dev / dev_lim) * (dev / dev_lim);
    if e <= 1.0 {
        return;
    }
    let k = 1.0 / sqrt(e);
    flex *= k;
    dev *= k;
    let mut nv = [0.0f32; 3];
    for c in 0..3 {
        nv[c] = -w.across[c] * flex + w.palm[c] * dev;
    }
    let na = sqrt(nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]);
    let mut held = ID;
    if na > 1e-6 {
        let h = sin(0.5 * na) / na;
        held = [nv[0] * h, nv[1] * h, nv[2] * h, cos(0.5 * na)];
    }
    let d2 = quat_mul(&held, &twist);
    let r2 = quat_mul(&w.straight, &d2);
    *hand_model = quat_mul(fore_model, &r2);
    quat_norm(hand_model);
}

/// Two-bone arm IK in model space (F4VR/VRArmIK convention: xDir hand -> shoulder, the elbow from the cosine law at
/// the WRIST, the bend direction from the VRArmIK swivel model with its safety clamps, temporally smoothed; the hand
/// pinned onto the target within the segment caps).
#[no_mangle]
pub unsafe extern "C" fn VRIK_SolveArm(
    boneBuf: *mut u8,
    upperIdx: i32,
    foreIdx: i32,
    handIdx: i32,
    targetModel: *const f32,
    handModelRot: *const f32,
    bodyRight: *const f32,
    bodyUp: *const f32,
    bodyFwd: *const f32,
    poleAngleRad: f32,
    swingGain: f32,
    isLeft: bool,
    storeDbg: bool,
    ladderReach: bool,
) {
    // the hand packet this solve starts from (the rifle elbow fixture's, main's ce2a00cad)
    let arm_packet = g_handsStableSeq;
    let target = a3(targetModel);
    g_VRIKHandPin[if isLeft { 1 } else { 0 }] = target;
    if upperIdx < 0 || foreIdx < 0 || handIdx < 0 {
        return;
    }
    if upperIdx >= MAXB || foreIdx >= MAXB || handIdx >= MAXB {
        return;
    }
    let (up_u, fore_u, hand_u) = (upperIdx as usize, foreIdx as usize, handIdx as usize);
    let hand_rot = a4(handModelRot);
    let (body_right, body_up, body_fwd) = (a3(bodyRight), a3(bodyUp), a3(bodyFwd));

    // The free combat swivel mixed with the relaxed elbow policy; a wrist locked to a thing is turned by that thing,
    // not by the arm, so the swivel eases out while it is held.
    let family_blend = if g_VRBind != 4 || ladderReach || !s_referenceValid[fore_u] || !s_referenceValid[hand_u] {
        0.0f32
    } else if VRIK_UsesNativeCyberware() && (g_VRCyberwareFamily == 1 || g_VRCyberwareFamily == 2) {
        1.0
    } else if VRIK_UsesNativeCyberware() && g_VRCyberwareFamily == 3 {
        0.5
    } else {
        0.25
    };
    let combat_blend = family_blend * (1.0 - wrist_held_ease(if isLeft { 0 } else { 1 }));
    let combat_elbow = combat_blend > 0.0;

    let (sh, el, wr) = (g_fkPos[up_u], g_fkPos[fore_u], g_fkPos[hand_u]);
    let mut cur_up = sub3(&el, &sh);
    let mut cur_fore = sub3(&wr, &el);
    let mut up_len = norm3(&mut cur_up);
    let mut fore_len = norm3(&mut cur_fore);
    if up_len < 1e-4 || fore_len < 1e-4 {
        return;
    }

    // STABLE SEGMENT LENGTHS (anti-ratchet): the native rest lengths of the first solve, or the calibrated halves.
    let side_j = if isLeft { 1 } else { 0 };
    let arm = &mut ARM;
    if arm.length_revision[side_j] != s_rigRevision {
        arm.length_revision[side_j] = s_rigRevision;
        arm.rest_up_len[side_j] = 0.0;
        arm.rest_fore_len[side_j] = 0.0;
        g_vrikUpArmRestCap[side_j] = false;
    }
    if arm.rest_up_len[side_j] <= 0.0 {
        arm.rest_up_len[side_j] = up_len;
        arm.rest_fore_len[side_j] = fore_len;
    }
    {
        let ual = if isLeft { g_VRUserArmLenL } else { g_VRUserArmLenR };
        let calib_half = if ual >= 0.45 { ual * 0.5 } else { 0.0 };
        up_len = if calib_half > 0.0 { calib_half } else { arm.rest_up_len[side_j] };
        fore_len = if calib_half > 0.0 { calib_half } else { arm.rest_fore_len[side_j] };
    }
    // A hand closed on something: the two-hand hold's support hand, or a wrist the reload has locked to a grip.
    let support = isLeft != super::weapon_arm::cvr_anim_weapon_in_left_hand();
    let held_body = super::physhands::adjust::holds_body(if isLeft { 0 } else { 1 });
    let grip = CyberpunkVR_GripReach != 0
        && ((support && CyberpunkVR_TwoHandActive != 0) || g_VRHandRotLockOn[if isLeft { 0 } else { 1 }] != 0 || held_body);
    g_VRIKGrip[side_j] = grip;
    let girdle = reach_girdle(boneBuf, upperIdx, &target, &body_up, &body_fwd, up_len + fore_len, isLeft, ladderReach, grip);
    if girdle {
        for k in 0..3 {
            cur_up[k] = g_fkPos[fore_u][k] - g_fkPos[up_u][k];
            cur_fore[k] = g_fkPos[hand_u][k] - g_fkPos[fore_u][k];
        }
        norm3(&mut cur_up);
        norm3(&mut cur_fore);
    }
    // SHOULDER PROTRACTION for the legacy anchor modes, written ABSOLUTE against the captured rest translation.
    let mut sh_p = g_fkPos[up_u];
    {
        let par = g_VRBoneParent[up_u] as i32;
        if !girdle && par >= 0 && par < upperIdx {
            let tl = bt(boneBuf, upperIdx);
            if !g_vrikUpArmRestCap[side_j] {
                g_vrikUpArmRest[side_j] = a3(tl);
                g_vrikUpArmRestCap[side_j] = true;
            }
            let pu = par as usize;
            let mut base = quat_rotate_vec(&g_fkRot[pu], &g_vrikUpArmRest[side_j]);
            base[0] += g_fkPos[pu][0];
            base[1] += g_fkPos[pu][1];
            base[2] += g_fkPos[pu][2];
            let to_t = sub3(&target, &base);
            let dist = sqrt(to_t[0] * to_t[0] + to_t[1] * to_t[1] + to_t[2] * to_t[2]);
            let arm_l0 = up_len + fore_len;
            if dist > 1e-4 && arm_l0 > 1e-4 {
                let mut fwdness = (to_t[0] * body_fwd[0] + to_t[1] * body_fwd[1] + to_t[2] * body_fwd[2]) / dist;
                if fwdness < 0.0 {
                    fwdness = 0.0;
                }
                let mut need = (dist / arm_l0 - 0.90) * (1.0 / 0.12);
                if need < 0.0 {
                    need = 0.0;
                }
                if need > 1.0 {
                    need = 1.0;
                }
                let pr = need * fwdness;
                let out_s = if isLeft { 1.0f32 } else { -1.0 };
                let delta = [
                    (body_fwd[0] * 0.06 - out_s * body_right[0] * 0.015) * pr,
                    (body_fwd[1] * 0.06 - out_s * body_right[1] * 0.015) * pr,
                    (body_fwd[2] * 0.06 - out_s * body_right[2] * 0.015) * pr,
                ];
                sh_p = add3(&base, &delta);
                let pc = quat_conj(&g_fkRot[pu]);
                let ld = quat_rotate_vec(&pc, &delta);
                let rest = g_vrikUpArmRest[side_j];
                put3(tl, &[rest[0] + ld[0], rest[1] + ld[1], rest[2] + ld[2]]);
            }
        }
    }

    // Hand -> shoulder (F4VR convention; xDir is along this).
    let hand_to_shoulder = sub3(&sh_p, &target);
    let hs_len = sqrt(hand_to_shoulder[0] * hand_to_shoulder[0] + hand_to_shoulder[1] * hand_to_shoulder[1] + hand_to_shoulder[2] * hand_to_shoulder[2]);
    if hs_len < 1e-4 {
        return;
    }
    let x_dir = [hand_to_shoulder[0] / hs_len, hand_to_shoulder[1] / hs_len, hand_to_shoulder[2] / hs_len];

    // STRETCH instead of clamp (per F4VR): past the rest length both segments lengthen proportionally.
    let (mut up_l, mut fore_l) = (up_len, fore_len);
    if hs_len > up_l + fore_l {
        let diff = hs_len - up_l - fore_l;
        let ratio = fore_l / (fore_l + up_l);
        fore_l += ratio * diff;
        up_l += (1.0 - ratio) * diff;
    }
    // Hand far closer than |up-fore|: no solution; equalise to keep the solver stable.
    if hs_len < (up_l - fore_l).abs() + 1e-3 {
        let avg = (up_l + fore_l) * 0.5;
        up_l = avg;
        fore_l = avg;
    }
    // EXTENSION SNAP: within the last reach percents the segments ease toward exactly the distance.
    {
        let sum0 = up_l + fore_l;
        if sum0 > 1e-4 && hs_len < sum0 {
            let r = hs_len / sum0;
            let mut s = (r - 0.90) * (1.0 / 0.06);
            if s > 1.0 {
                s = 1.0;
            }
            if s > 0.0 {
                let f = 1.0 + (r - 1.0) * s;
                up_l *= f;
                fore_l *= f;
            }
        }
    }

    // ANATOMICAL TWIST RIG CONSTANTS, captured once per side from a visibly bent native elbow.
    let side_i = if isLeft { 1 } else { 0 };
    if arm.axis_revision[side_i] != s_rigRevision {
        arm.axis_revision[side_i] = s_rigRevision;
        arm.rig_cap[side_i] = false;
    }
    if !arm.rig_cap[side_i] {
        let mut cx = cross3(&cur_up, &cur_fore);
        let cl = sqrt(cx[0] * cx[0] + cx[1] * cx[1] + cx[2] * cx[2]);
        if cl > 0.17 {
            cx[0] /= cl;
            cx[1] /= cl;
            cx[2] /= cl;
            let uc = quat_conj(&g_fkRot[up_u]);
            let fc = quat_conj(&g_fkRot[fore_u]);
            let hc = quat_conj(&g_fkRot[hand_u]);
            arm.ax_loc_up[side_i] = quat_rotate_vec(&uc, &cur_up);
            arm.hg_loc_up[side_i] = quat_rotate_vec(&uc, &cx);
            arm.ax_loc_fore[side_i] = quat_rotate_vec(&fc, &cur_fore);
            arm.hg_loc_fore[side_i] = quat_rotate_vec(&fc, &cx);
            arm.palm_loc[side_i] = quat_rotate_vec(&hc, &cur_fore);
            arm.arm_rest[side_i] = cur_up;
            let d_uf = dot3(&cur_fore, &cur_up);
            let mut bnd = [cur_up[0] * d_uf - cur_fore[0], cur_up[1] * d_uf - cur_fore[1], cur_up[2] * d_uf - cur_fore[2]];
            if norm3(&mut bnd) > 1e-3 {
                arm.bend_rest[side_i] = bnd;
                arm.rig_cap[side_i] = true;
            }
        }
    }

    let side_sign = if isLeft { 1.0f32 } else { -1.0 };
    let back_ref = [-body_fwd[0], -body_fwd[1], -body_fwd[2]];
    let out_ref = [-side_sign * body_right[0], -side_sign * body_right[1], -side_sign * body_right[2]];
    let strip_forward_bend = |dir: &mut V3| {
        if combat_blend >= 1.0 {
            return;
        }
        let fc = dot3(dir, &body_fwd);
        if fc > 0.0 {
            dir[0] -= body_fwd[0] * fc;
            dir[1] -= body_fwd[1] * fc;
            dir[2] -= body_fwd[2] * fc;
        }
        *dir = project_perp(dir, &x_dir);
    };
    let mut o_perp = project_perp(&out_ref, &x_dir);

    // The VRArmIK swivel model: a synthetic symmetric neutral swung onto the target direction, deviated by the zone
    // model in body-normalised coordinates.
    let mut y_dir = [0.0f32; 3];
    let mut y_from_vrarmik = false;
    {
        let dir_n0 = [-x_dir[0], -x_dir[1], -x_dir[2]];
        let mut arm_rest = [
            -body_up[0] + 0.15 * body_fwd[0] + 0.10 * out_ref[0],
            -body_up[1] + 0.15 * body_fwd[1] + 0.10 * out_ref[1],
            -body_up[2] + 0.15 * body_fwd[2] + 0.10 * out_ref[2],
        ];
        norm3(&mut arm_rest);
        let mut bend_rest = [
            -0.90 * body_fwd[0] + 0.35 * out_ref[0] - 0.25 * body_up[0],
            -0.90 * body_fwd[1] + 0.35 * out_ref[1] - 0.25 * body_up[1],
            -0.90 * body_fwd[2] + 0.35 * out_ref[2] - 0.25 * body_up[2],
        ];
        norm3(&mut bend_rest);
        let qsw = quat_from_to(&arm_rest, &dir_n0);
        let rf = quat_rotate_vec(&qsw, &bend_rest);
        let mut ref_p = project_perp(&rf, &x_dir);
        if norm3(&mut ref_p) > 0.2 {
            let ax_len = up_len + fore_len;
            let v = [-hand_to_shoulder[0], -hand_to_shoulder[1], -hand_to_shoulder[2]];
            let nx = dot3(&v, &body_right) / ax_len;
            let ny = dot3(&v, &body_up) / ax_len;
            let nz = dot3(&v, &body_fwd) / ax_len;
            let inward = nx * (if isLeft { 1.0 } else { -1.0 });
            let mut dev = -60.0 * ny;
            let mut z_gate = 0.6 - nz;
            if z_gate < 0.0 {
                z_gate = 0.0;
            }
            dev += if ny > 0.0 { 70.0 * z_gate * ny } else { -100.0 * z_gate * (-ny) };
            let mut x_t = inward + 0.1;
            if x_t < 0.0 {
                x_t = 0.0;
            }
            dev += -50.0 * x_t;
            if dev > 25.0 {
                dev = 25.0;
            }
            if dev < -60.0 {
                dev = -60.0;
            }
            let a = dev * 0.01745329252 * (if isLeft { 1.0 } else { -1.0 });
            let cxv = cross3(&x_dir, &ref_p);
            let (ca, sa) = (cos(a), sin(a));
            y_dir = [ref_p[0] * ca + cxv[0] * sa, ref_p[1] * ca + cxv[1] * sa, ref_p[2] * ca + cxv[2] * sa];
            y_from_vrarmik = true;
        }
    }
    if !y_from_vrarmik {
        let mir = if isLeft { -1.0f32 } else { 1.0 };
        let dir_n = [-x_dir[0], -x_dir[1], -x_dir[2]];
        let d_c = [dot3(&dir_n, &body_right) * mir, dot3(&dir_n, &body_fwd), dot3(&dir_n, &body_up)];
        let f0 = [0.0f32, 0.0, -1.0];
        let mut t0 = [d_c[0], d_c[1] + 1.0, d_c[2]];
        if norm3(&mut t0) < 1e-3 {
            t0 = d_c;
        }
        let q0 = quat_from_to(&f0, &t0);
        let b_c = quat_rotate_vec(&q0, &[0.0, -1.0, 0.0]);
        let mut b = [
            body_right[0] * b_c[0] * mir + body_fwd[0] * b_c[1] + body_up[0] * b_c[2],
            body_right[1] * b_c[0] * mir + body_fwd[1] * b_c[1] + body_up[1] * b_c[2],
            body_right[2] * b_c[0] * mir + body_fwd[2] * b_c[1] + body_up[2] * b_c[2],
        ];
        b[0] += dir_n[0];
        b[1] += dir_n[1];
        b[2] += dir_n[2];
        // (the palm term, kPalmW, is disabled: rotating the wrist must not move the elbow)
        y_dir = project_perp(&b, &x_dir);
    }

    // CRITICAL anti-forward-bend clamp.
    strip_forward_bend(&mut y_dir);
    if norm3(&mut y_dir) < 0.3 {
        if norm3(&mut o_perp) > 0.3 {
            y_dir = o_perp;
        } else {
            let mut fb = [0.0f32, 0.0, 1.0];
            if x_dir[2].abs() > 0.9 {
                fb[0] = 1.0;
                fb[2] = 0.0;
            }
            y_dir = project_perp(&fb, &x_dir);
            norm3(&mut y_dir);
        }
    }

    // Cross-body swing toward the own side, faded off toward full extension and above the shoulder line.
    let arm_len = up_l + fore_l;
    let mut reach01 = if arm_len > 1e-4 { hs_len / arm_len } else { 0.0 };
    if reach01 > 1.0 {
        reach01 = 1.0;
    }
    let mut straight_fade = (reach01 - 0.92) * (1.0 / 0.08);
    if straight_fade < 0.0 {
        straight_fade = 0.0;
    }
    if straight_fade > 1.0 {
        straight_fade = 1.0;
    }
    let pole_weight = 1.0 - straight_fade;
    let shoulder_to_hand = [-hand_to_shoulder[0], -hand_to_shoulder[1], -hand_to_shoulder[2]];
    let lateral =
        (shoulder_to_hand[0] * body_right[0] + shoulder_to_hand[1] * body_right[1] + shoulder_to_hand[2] * body_right[2]) * side_sign;
    let cross_amount = if lateral > 0.0 { lateral / arm_len } else { 0.0 };
    if norm3(&mut o_perp) > 1e-3 && pole_weight > 0.0 {
        let mut bend_factor = (0.90 - reach01) * (1.0 / 0.35);
        if bend_factor < 0.0 {
            bend_factor = 0.0;
        }
        if bend_factor > 1.0 {
            bend_factor = 1.0;
        }
        let mut cross_gate = (cross_amount - 0.05) * (1.0 / 0.10);
        if cross_gate < 0.0 {
            cross_gate = 0.0;
        }
        if cross_gate > 1.0 {
            cross_gate = 1.0;
        }
        let up_amt = (shoulder_to_hand[0] * body_up[0] + shoulder_to_hand[1] * body_up[1] + shoulder_to_hand[2] * body_up[2]) / arm_len;
        let mut h_gate = 1.0 - (up_amt - 0.05) * (1.0 / 0.30);
        if h_gate < 0.0 {
            h_gate = 0.0;
        }
        if h_gate > 1.0 {
            h_gate = 1.0;
        }
        let mut f = swingGain * cross_gate * bend_factor * pole_weight * h_gate;
        if f < 0.0 {
            f = 0.0;
        }
        if f > 1.0 {
            f = 1.0;
        }
        for k in 0..3 {
            y_dir[k] = (1.0 - f) * y_dir[k] + f * o_perp[k];
        }
    }
    strip_forward_bend(&mut y_dir);
    norm3(&mut y_dir);

    // Optional swivel (elbow pole slider), faded by poleWeight.
    {
        let swivel_rad = poleAngleRad * pole_weight;
        if swivel_rad.abs() > 1e-5 {
            let (c, s) = (cos(swivel_rad), sin(swivel_rad));
            let cr = cross3(&x_dir, &y_dir);
            let ad = dot3(&x_dir, &y_dir);
            let mut y2 = [0.0f32; 3];
            for k in 0..3 {
                y2[k] = y_dir[k] * c + cr[k] * s + x_dir[k] * ad * (1.0 - c);
            }
            y_dir = y2;
            norm3(&mut y_dir);
        }
    }
    strip_forward_bend(&mut y_dir);
    if norm3(&mut y_dir) < 1e-3 {
        y_dir = project_perp(&back_ref, &x_dir);
        if norm3(&mut y_dir) < 1e-3 {
            y_dir = project_perp(&out_ref, &x_dir);
            norm3(&mut y_dir);
        }
    }

    // NEAR-EXTENSION RELAX: a nearly straight arm hangs its elbow sag down (and slightly back).
    {
        let mut rf = (reach01 - 0.88) * (1.0 / 0.10);
        if rf > 1.0 {
            rf = 1.0;
        }
        if rf > 0.0 {
            let dn = [-body_up[0] - 0.3 * body_fwd[0], -body_up[1] - 0.3 * body_fwd[1], -body_up[2] - 0.3 * body_fwd[2]];
            let mut dn_p = project_perp(&dn, &x_dir);
            if norm3(&mut dn_p) > 1e-3 {
                for k in 0..3 {
                    y_dir[k] = y_dir[k] * (1.0 - rf) + dn_p[k] * rf;
                }
                if norm3(&mut y_dir) < 1e-3 {
                    y_dir = dn_p;
                }
            }
        }
    }

    // F4VR: wristAngle = acos((fore^2 + hs^2 - up^2) / (2*fore*hs)), the triangle's angle AT THE HAND.
    let mut cos_wrist = (fore_l * fore_l + hs_len * hs_len - up_l * up_l) / (2.0 * fore_l * hs_len);
    if cos_wrist < -1.0 {
        cos_wrist = -1.0;
    }
    if cos_wrist > 1.0 {
        cos_wrist = 1.0;
    }
    let sin_wrist = sqrt(0.0f32.max(1.0 - cos_wrist * cos_wrist));
    let x_dist = cos_wrist * fore_l;
    let y_dist = sin_wrist * fore_l;

    let relaxed_elbow_policy = |bend: &mut V3| {
        if CyberpunkVR_VrikElbowPolicy == 0 || y_dist <= 1e-4 {
            return;
        }
        let arm_len_tot = up_l + fore_l;
        if arm_len_tot > 1e-3 {
            let mut w = 1.0 - hs_len / (0.5 * arm_len_tot);
            if w < 0.0 {
                w = 0.0;
            }
            if w > 1.0 {
                w = 1.0;
            }
            w *= 0.75;
            if w > 0.0 {
                let rest = [
                    0.30 * out_ref[0] - body_up[0] - 2.0 * body_fwd[0],
                    0.30 * out_ref[1] - body_up[1] - 2.0 * body_fwd[1],
                    0.30 * out_ref[2] - body_up[2] - 2.0 * body_fwd[2],
                ];
                let mut rp = project_perp(&rest, &x_dir);
                if norm3(&mut rp) > 1e-3 {
                    for k in 0..3 {
                        bend[k] = bend[k] * (1.0 - w) + rp[k] * w;
                    }
                    *bend = project_perp(bend, &x_dir);
                    if norm3(bend) < 1e-3 {
                        *bend = rp;
                    }
                }
            }
        }
        let hand_up = dot3(&target, &body_up);
        let sh_up = dot3(&sh_p, &body_up);
        if hand_up <= sh_up + 0.05 {
            // arm not raised above the shoulder
            let cap_up = if hand_up < sh_up { hand_up } else { sh_up };
            let mut u = project_perp(&body_up, &x_dir);
            let r = norm3(&mut u);
            if r > 1e-3 {
                let base_up = hand_up + x_dist * dot3(&x_dir, &body_up);
                let mut a_max = (cap_up - base_up) / (y_dist * r);
                // A HANGING ARM: the cap holds only as far as it can be met, and never turns an elbow forward.
                let can_meet = clamp((a_max + 1.0) / 0.5, 0.0, 1.0);
                a_max = 1.0 + (a_max - 1.0) * can_meet;
                if a_max < 1.0 {
                    let mut v = cross3(&x_dir, &u);
                    norm3(&mut v);
                    let a = dot3(bend, &u);
                    if a > a_max {
                        let was = *bend;
                        let b = dot3(bend, &v);
                        let mut b_new = sqrt(0.0f32.max(1.0 - a_max * a_max));
                        if b < 0.0 {
                            b_new = -b_new;
                        }
                        for k in 0..3 {
                            bend[k] = u[k] * a_max + v[k] * b_new;
                        }
                        strip_forward_bend(bend);
                        if norm3(bend) < 1e-3 {
                            *bend = was;
                        }
                    }
                }
            }
        }
    };
    let separate_elbow = |bend: &mut V3, weight: f32| {
        let mut o_p = project_perp(&out_ref, &x_dir);
        if norm3(&mut o_p) > 1e-3 {
            let min_out = 0.12f32; // ~sin(7 deg)
            let d_out = dot3(bend, &o_p);
            if d_out < min_out {
                let correction = weight * (min_out - d_out);
                for k in 0..3 {
                    bend[k] += o_p[k] * correction;
                }
                *bend = project_perp(bend, &x_dir);
                if norm3(bend) < 1e-3 {
                    *bend = o_p;
                }
            }
        }
    };

    if combat_elbow && s_referenceValid[fore_u] && s_referenceValid[hand_u] {
        // The elbow beside the fist: forearm continuation from the wrist's aim (not its roll), the point on the
        // elbow circle closest to it.
        let mut base_bend = y_dir;
        if combat_blend < 1.0 {
            separate_elbow(&mut base_bend, 1.0);
            relaxed_elbow_policy(&mut base_bend);
        }
        let mut continuation = sub3(&s_referenceModelPos[hand_u], &s_referenceModelPos[fore_u]);
        norm3(&mut continuation);
        let inverse_hand = quat_conj(&s_referenceModelRot[hand_u]);
        let local = quat_rotate_vec(&inverse_hand, &continuation);
        let aim = quat_rotate_vec(&hand_rot, &local);
        let mut guide = [0.0f32; 3];
        let mut relaxed = [0.0f32; 3];
        for k in 0..3 {
            guide[k] = -aim[k];
            relaxed[k] = -body_up[k] + 0.30 * out_ref[k];
        }
        guide = project_perp(&guide, &x_dir);
        relaxed = project_perp(&relaxed, &x_dir);
        let leverage = norm3(&mut guide);
        if norm3(&mut relaxed) < 1e-4 {
            relaxed = o_perp;
        }
        let weight = clamp((leverage - 0.10) / 0.55, 0.0, 1.0) * 0.90;
        for k in 0..3 {
            y_dir[k] = relaxed[k] * (1.0 - weight) + guide[k] * weight;
        }
        if norm3(&mut y_dir) < 1e-4 {
            y_dir = relaxed;
        }
        if combat_blend < 1.0 {
            separate_elbow(&mut y_dir, 1.0);
            norm3(&mut base_bend);
            norm3(&mut y_dir);
            let cross = cross3(&base_bend, &y_dir);
            let angle = atan2(dot3(&cross, &x_dir), dot3(&base_bend, &y_dir)) * combat_blend;
            let cross = cross3(&x_dir, &base_bend);
            for k in 0..3 {
                y_dir[k] = base_bend[k] * cos(angle) + cross[k] * sin(angle);
            }
            norm3(&mut y_dir);
        }
    }

    // A RAISED RIFLE brings the rear wrist close to the chest (main's 3b677ed26), where the outward pole projected onto
    // a mostly inward reach can point above the shoulder: a down/out pole is carried from a forward reach instead, then
    // swivelled on the same elbow circle; faded back to the normal swivel on extension, at the belt, or as the wrist
    // leaves the forward/inward chest region (fractions of the arm's length).
    let mut rifle_pole_weight = 0.0f32;
    let elbow_ab = cvr_host::rifle_elbow::mode();
    if !support
        && !grip
        && g_hasWeaponEquipped
        && g_VRBind == 4
        && !ladderReach
        && CyberpunkVR_VrikElbowPolicy != 0
        && (CyberpunkVR_WeaponClass == 2 || CyberpunkVR_WeaponClass == 4)
    {
        let close = clamp((0.60 - reach01) / 0.20, 0.0, 1.0);
        let raised = clamp((dot3(&shoulder_to_hand, &body_up) / arm_len + 0.45) / 0.20, 0.0, 1.0);
        let chest = clamp((lateral + dot3(&shoulder_to_hand, &body_fwd)) / (0.15 * arm_len), 0.0, 1.0);
        rifle_pole_weight = close * raised * chest;
        // the A/B fixture's 1: the swivel before main's fix
        if elbow_ab == 1 {
            rifle_pole_weight = 0.0;
        }
        if rifle_pole_weight > 0.0 {
            let mut reach_dir = [0.0f32; 3];
            let mut rest = [0.0f32; 3];
            for k in 0..3 {
                reach_dir[k] = -x_dir[k];
                rest[k] = 0.55 * out_ref[k] - body_up[k];
            }
            let swing = quat_from_to(&body_fwd, &reach_dir);
            let pole = quat_rotate_vec(&swing, &rest);
            let tangent = cross3(&x_dir, &y_dir);
            let turn = atan2(dot3(&pole, &tangent), dot3(&pole, &y_dir));
            let angle = remainder(turn + poleAngleRad, 6.2831853) * rifle_pole_weight;
            let (c, s) = (cos(angle), sin(angle));
            for k in 0..3 {
                y_dir[k] = y_dir[k] * c + tangent[k] * s;
            }
        }
    }

    // TEMPORAL SMOOTHING (anti-teleport): yDir smoothed over time (80 ms relaxed -> 25 ms combat), rate-capped.
    {
        if arm.y_revision[side_i] != s_rigRevision || arm.y_blend[side_i] != combat_blend {
            arm.y_revision[side_i] = s_rigRevision;
            arm.y_blend[side_i] = combat_blend;
            arm.y_init[side_i] = false;
            arm.y_t[side_i] = 0;
        }
        let (qn, qf) = qpc();
        let mut dt = 0.016f32;
        if arm.y_t[side_i] != 0 {
            dt = ((qn - arm.y_t[side_i]) as f64 / qf as f64) as f32;
            if dt < 0.0 {
                dt = 0.0;
            }
            if dt > 0.05 {
                dt = 0.05;
            }
        }
        arm.y_t[side_i] = qn;
        if !arm.y_init[side_i] {
            arm.y_sm[side_i] = y_dir;
            arm.y_init[side_i] = true;
        } else {
            let sm = arm.y_sm[side_i];
            let mut alpha = 1.0 - exp(-dt / (0.08 + (0.025 - 0.08) * combat_blend));
            let mut dotp = sm[0] * y_dir[0] + sm[1] * y_dir[1] + sm[2] * y_dir[2];
            if dotp < -1.0 {
                dotp = -1.0;
            }
            if dotp > 1.0 {
                dotp = 1.0;
            }
            let ang = acos(dotp);
            let max_step = (9.42477 + (25.13274 - 9.42477) * combat_blend) * dt;
            if ang > 1e-4 && ang * alpha > max_step {
                alpha = max_step / ang;
            }
            let mut ns = [
                sm[0] + (y_dir[0] - sm[0]) * alpha,
                sm[1] + (y_dir[1] - sm[1]) * alpha,
                sm[2] + (y_dir[2] - sm[2]) * alpha,
            ];
            if norm3(&mut ns) > 1e-3 {
                arm.y_sm[side_i] = ns;
            } else {
                // Exactly antiparallel lerp degenerated: snap (next frames smooth from here).
                arm.y_sm[side_i] = y_dir;
            }
        }
        let mut y_p = project_perp(&arm.y_sm[side_i], &x_dir);
        if norm3(&mut y_p) > 1e-3 {
            y_dir = y_p;
        }
    }

    separate_elbow(&mut y_dir, 1.0 - rifle_pole_weight);
    if !combat_elbow {
        relaxed_elbow_policy(&mut y_dir);
    }

    let new_elbow = [
        target[0] + x_dir[0] * x_dist + y_dir[0] * y_dist,
        target[1] + x_dir[1] * x_dist + y_dir[1] * y_dist,
        target[2] + x_dir[2] * x_dist + y_dir[2] * y_dist,
    ];
    if !isLeft && elbow_ab != 0 {
        // Diagnostic only: the controller's target and this solve's pole in body space, and the packet the completed
        // pose is kept for (cvr_host::rifle_elbow).
        let pose_id = if arm_packet == g_handsStableSeq && super::snapshot::SharedPose(115) == 2.0 { (1u64 << 63) | u64::from(arm_packet) } else { 0 };
        let radial = sub3(&new_elbow, &sh_p);
        let mut values = [0.0f32; 13];
        values[0] = elbow_ab as f32;
        values[1] = rifle_pole_weight;
        for (k, axis) in [&body_right, &body_fwd, &body_up].into_iter().enumerate() {
            values[2 + k] = dot3(&shoulder_to_hand, axis);
            values[5 + k] = dot3(&radial, axis);
        }
        values[8..12].copy_from_slice(&hand_rot);
        values[12] = CyberpunkVR_WeaponClass as f32;
        cvr_host::rifle_elbow::sample(elbow_ab, pose_id, values);
    }
    let mut des_up = sub3(&new_elbow, &sh_p);
    norm3(&mut des_up);
    let mut des_fore = sub3(&target, &new_elbow);
    norm3(&mut des_fore);

    // The desired hinge: desUp x desFore, or near full extension the pole plane (desUp x -yDir).
    let mut h_des = cross3(&des_up, &des_fore);
    {
        let hl = sqrt(h_des[0] * h_des[0] + h_des[1] * h_des[1] + h_des[2] * h_des[2]);
        if hl < 0.05 {
            let neg_y = [-y_dir[0], -y_dir[1], -y_dir[2]];
            h_des = cross3(&des_up, &neg_y);
            norm3(&mut h_des);
        } else {
            h_des[0] /= hl;
            h_des[1] /= hl;
            h_des[2] /= hl;
        }
    }

    // Upper arm: swing from the rig reference in the solved clavicle frame; the anatomical basis only limits the roll
    // (25 deg dead zone, eased, capped at 60).
    let mut swing_source = g_fkRot[up_u];
    let mut swing_axis = cur_up;
    let reference_parent = g_VRBoneParent[up_u] as i32;
    if s_referenceValid[up_u]
        && s_referenceValid[fore_u]
        && g_VRBoneParent[fore_u] as i32 == upperIdx
        && reference_parent >= 0
        && reference_parent < upperIdx
    {
        swing_source = quat_mul(&g_fkRot[reference_parent as usize], &s_referenceLocalRot[up_u]);
        quat_norm(&mut swing_source);
        swing_axis = quat_rotate_vec(&swing_source, &s_referenceLocalPos[fore_u]);
        norm3(&mut swing_axis);
    }
    let delta1 = quat_from_to(&swing_axis, &des_up);
    let mut swing_up = quat_mul(&delta1, &swing_source);
    quat_norm(&mut swing_up);
    let mut new_up_model = swing_up;
    if arm.rig_cap[side_i] {
        let basis = quat_align_two(&arm.ax_loc_up[side_i], &arm.hg_loc_up[side_i], &des_up, &h_des);
        let swc = [-swing_up[0], -swing_up[1], -swing_up[2], swing_up[3]];
        let mut d = quat_mul(&basis, &swc);
        quat_norm(&mut d);
        let tw = quat_extract_twist(&d, &des_up);
        let proj = tw[0] * des_up[0] + tw[1] * des_up[1] + tw[2] * des_up[2];
        let ang_s = 2.0 * atan2(proj, tw[3]); // signed twist vs plain swing
        let k_dead = 0.4363f32; // 25 deg dead zone
        let k_cap = 1.0472f32; // 60 deg max applied roll
        let mag = ang_s.abs();
        // std::fmin, the other operand for a NaN, as f32::min
        let eff = if mag <= k_dead { 0.0 } else { (mag - k_dead).min(k_cap) };
        if eff > 0.0 {
            let s = (if ang_s >= 0.0 { 1.0f32 } else { -1.0 }) * sin(eff * 0.5);
            let aa = [des_up[0] * s, des_up[1] * s, des_up[2] * s, cos(eff * 0.5)];
            new_up_model = quat_mul(&aa, &swing_up);
            quat_norm(&mut new_up_model);
        }
    }
    let up_parent = g_VRBoneParent[up_u] as i32;
    let up_parent_model = if up_parent >= 0 && up_parent < MAXB { g_fkRot[up_parent as usize] } else { ID };
    write_local_rot(boneBuf, upperIdx, &up_parent_model, &new_up_model);

    // Pronation shared with the wrist for skin and cyberware alike.
    let mut new_fore_model;
    if arm.rig_cap[side_i] {
        new_fore_model = quat_align_two(&arm.ax_loc_fore[side_i], &arm.hg_loc_fore[side_i], &des_fore, &h_des);
    } else {
        // Until the hinge can be identified, transport the native forearm.
        let native_delta = quat_from_to(&cur_up, &des_up);
        let fore_base = quat_rotate_vec(&native_delta, &cur_fore);
        let delta2 = quat_from_to(&fore_base, &des_fore);
        let tmp = quat_mul(&delta2, &native_delta);
        new_fore_model = quat_mul(&tmp, &g_fkRot[fore_u]);
        quat_norm(&mut new_fore_model);
    }
    if arm.rig_cap[side_i] {
        let inverse = quat_conj(&new_fore_model);
        let mut relative = quat_mul(&hand_rot, &inverse);
        quat_norm(&mut relative);
        let twist = quat_extract_twist(&relative, &des_fore);
        let projected = twist[0] * des_fore[0] + twist[1] * des_fore[1] + twist[2] * des_fore[2];
        let angle = 2.0 * atan2(projected, twist[3]);
        let applied = clamp(angle * 0.25, -0.7854, 0.7854);
        let sine = sin(applied * 0.5);
        let roll = [des_fore[0] * sine, des_fore[1] * sine, des_fore[2] * sine, cos(applied * 0.5)];
        new_fore_model = quat_mul(&roll, &new_fore_model);
        quat_norm(&mut new_fore_model);
    }
    write_local_rot(boneBuf, foreIdx, &new_up_model, &new_fore_model);

    // DISTRIBUTED STRETCH (the elbow half of the hand pin), within anatomical caps.
    let mut elbow_w = new_elbow;
    {
        let rel_u = sub3(&new_elbow, &sh_p);
        let rl_u = sqrt(rel_u[0] * rel_u[0] + rel_u[1] * rel_u[1] + rel_u[2] * rel_u[2]);
        if rl_u > 1e-4 {
            let mut cl_u = rl_u;
            let (lo_u, hi_u) = (up_len * 0.85, up_len * 1.15);
            if cl_u < lo_u {
                cl_u = lo_u;
            }
            if cl_u > hi_u {
                cl_u = hi_u;
            }
            for k in 0..3 {
                elbow_w[k] = sh_p[k] + rel_u[k] * (cl_u / rl_u);
            }
            write_local_pos(boneBuf, foreIdx, &sh_p, &new_up_model, &elbow_w);
        }
    }

    // Hand orientation written local to the new forearm, inside a wrist's range -- but not a hand on a weapon or locked
    // to a grip: there the hand is the aim, and the forearm is the one that gives.
    {
        let mut hand_held = hand_rot;
        if !g_hasWeaponEquipped && !grip {
            hold_wrist_range(if isLeft { 0 } else { 1 }, foreIdx, handIdx, &new_fore_model, &mut hand_held);
        }
        write_local_rot(boneBuf, handIdx, &new_fore_model, &hand_held);
        // the farthest the hand pin below puts the hand from this shoulder: both segments at their stretch caps
        super::physhands::pose::note_forearm(if isLeft { 0 } else { 1 }, &elbow_w, &target, &hand_held, Some(&sh_p), 1.15 * (up_len + fore_len));
    }

    // HAND PIN (hand == target exactly, the forearm segment within 0.7..1.15 of its rest length).
    {
        let rel = sub3(&target, &elbow_w);
        let rl = sqrt(rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]);
        if rl > 1e-4 {
            let mut cl = rl;
            let (lo, hi) = (fore_len * 0.7, fore_len * 1.15);
            // what the caps leave between the wrist and its target -- the thorax turn's input (VRIK_GripTorso)
            g_VRIKShort[side_j] = rl - hi;
            CyberpunkVR_DebugGripShortMm[side_j] = (rl - hi) * 1000.0;
            // The weighted body is already driven to this wrist's grip point. A second positional cap would separate
            // the fingers from the part whenever wrist rotation changes the elbow solve.
            cl = cvr_physhands::held_grip::pinned_forearm_length(
                cl,
                lo,
                hi,
                held_body || super::physhands::blade::blade_attachment_matches(if isLeft { 0 } else { 1 }, &target),
            );
            let pin = [elbow_w[0] + rel[0] * (cl / rl), elbow_w[1] + rel[1] * (cl / rl), elbow_w[2] + rel[2] * (cl / rl)];
            write_local_pos(boneBuf, handIdx, &elbow_w, &new_fore_model, &pin);
            g_VRIKHandPin[side_j] = pin;
        }
    }

    update_upper_arm_deformation(boneBuf, foreIdx, isLeft);
    VRIK_UpdateForearmDeformation(boneBuf, foreIdx, handIdx, isLeft);

    if storeDbg {
        let l = if isLeft { &mut g_VRIKDbgLocalL } else { &mut g_VRIKDbgLocal };
        *l = [lateral, cross_amount, reach01, 0.0];
        if isLeft {
            g_VRIKDbgTargetL = target;
            g_VRIKDbgShoulderL = sh_p;
            g_VRIKDbgElbowL = new_elbow;
            g_VRIKDbgLensL = [up_l, fore_l];
        } else {
            g_VRIKDbgTarget = target;
            g_VRIKDbgShoulder = sh_p;
            g_VRIKDbgElbow = new_elbow;
            g_VRIKDbgLens = [up_l, fore_l];
        }
    }
}

/// HAND STOP: a hand position resolved by physics, pushed in from script (model space); index 0 LEFT, 1 RIGHT. The
/// target before any stop is published first, and the wrist lock replaces the hand's rotation.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ApplyHandStop(side: i32, target: *mut f32, handRot: *mut f32) {
    if !(0..=1).contains(&side) || target.is_null() {
        return;
    }
    let s = side as usize;
    // THE WRIST A SCRIPT CLAIMED (shared slot 162: the reload's slide glue and magazine magnet, a holster's hold, the
    // grenade's pin): its stop is drawn even with the hand's own body on. The body's place is where the hand starts
    // from, the script's stop is where it goes (the pull below), its rotation lock kept over the body's turn.
    let owned_slot = if g_pSharedHands.is_null() { -1.0 } else { *g_pSharedHands.add(SLOT_RELOAD_OWNED_HAND) };
    let claimed = owned_slot > -0.5 && (owned_slot + 0.5) as i32 == side;
    let scanner = cvr_input::scanner::SCANNER_HAND_PHYSICS.scanner_active();
    let slow_physics = cvr_input::scanner::SCANNER_HAND_PHYSICS.active();
    // published unconditionally, so the loop has a target even while no hold is active
    g_VRHandRawModel[s] = a3(target);
    if !handRot.is_null() {
        g_VRHandRawRot[s] = a4(handRot);
        // the reload grip's wrist lock: the solve gets the recorded orientation instead of the controller's
        if (!slow_physics || (claimed && !scanner)) && g_VRHandRotLockOn[s] != 0 {
            put4(handRot, &g_VRHandRotLock[s]);
        }
    }
    g_VRHandRawValid[s] = 1;
    let debug = &mut *(&raw mut CyberpunkVR_DebugHandStop[s]);
    // Keep raw tracking and the next physics target live, but never draw a slowed physics body. Explicit
    // reload/magazine ownership survives non-scanner slow motion; scanner releases all contacts.
    if slow_physics && (scanner || !claimed) {
        g_VRHandStopValid[s] = 0;
        g_VRHandRotLockOn[s] = 0;
        if !handRot.is_null() {
            super::physhands::pose::note_target(s, &a3(target), &a4(handRot));
        }
        debug[4..7].copy_from_slice(&a3(target));
        debug[3] = 0.0;
        debug[7] = 0.0;
        return;
    }
    // Raw tracking above is the input even during contact. The virtual wrist servo follows a bounded, compliant
    // target; PhysX can still refuse that target. Without physical hands the IK solve uses the same target directly.
    if !claimed && !slow_physics {
        super::blade_pose::apply_pose(side, target);
    }
    // THE HAND'S OWN BODY (physhands): tracking goes to it, and the drawn hand comes back from where PhysX has it.
    // While it does, the script's stops are not added on top: the body already met whatever they would.
    if !handRot.is_null() {
        let mut t = a3(target);
        let mut r = a4(handRot);
        super::physhands::pose::note_target(s, &t, &r);
        let adjusted = !slow_physics && super::physhands::adjust::adjust(s, &mut t, Some(&mut r));
        // The serial-bound point owns the final wrist even when the ordinary hand readout skips a frame. Raw tracking
        // above still drives PhysX.
        let pinned = !slow_physics && !claimed && super::physhands::blade::apply_blade_attachment(side, &mut t, &r);
        put3(target, &t);
        put4(handRot, &r);
        if adjusted || pinned {
            if !(claimed && g_VRHandStopValid[s] != 0) {
                debug[4..7].copy_from_slice(&t);
                debug[3] = if g_VRHandStopValid[s] != 0 { 1.0 } else { 0.0 };
                debug[7] = if claimed { 1.0 } else { 0.0 };
                return;
            }
            if g_VRHandRotLockOn[s] != 0 {
                put4(handRot, &g_VRHandRotLock[s]);
            }
        }
    }
    debug[0..3].copy_from_slice(&g_VRHandStopModel[s]);
    debug[3] = if g_VRHandStopValid[s] != 0 { 1.0 } else { 0.0 };
    debug[7] = if claimed { 1.0 } else { 0.0 };
    debug[4..7].copy_from_slice(&a3(target));
    if g_VRHandStopValid[s] == 0 {
        return;
    }
    let t = a3(target);
    let d = sub3(&g_VRHandStopModel[s], &t);
    let len = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    let dead = g_VRHandStopDeadband;
    if len <= dead || len > 1.5 {
        return; // 1.5 m: the body is lost, not blocked -- ignore it
    }
    let k = (len - dead) / len;
    let out = [t[0] + d[0] * k, t[1] + d[1] * k, t[2] + d[2] * k];
    put3(target, &out);
    debug[4..7].copy_from_slice(&out);
}

/// The legacy model-space hand target: controller in the recenter/body frame less the calibrated shoulder, axis
/// swapped and scaled, anchored at the shoulder.
#[no_mangle]
pub unsafe extern "C" fn VRIK_BuildHandTarget(
    shoulderModelPos: *const f32,
    calibratedShoulderBody: *const f32,
    hmdRel: *const f32,
    vrPos: *const f32,
    vrQuat: *const f32,
    wristCorr: *const f32,
    scale: f32,
    off: *const f32,
    outTarget: *mut f32,
    outHandRot: *mut f32,
) {
    let s = scale;
    let mut hq = if hmdRel.is_null() { ID } else { a4(hmdRel) };
    if (hq[0] * hq[0] + hq[1] * hq[1] + hq[2] * hq[2] + hq[3] * hq[3]) > 1e-4 {
        quat_norm(&mut hq);
    } else {
        hq = ID;
    }
    let controller_body = quat_rotate_vec(&hq, &a3(vrPos));
    let shoulder_body = if calibratedShoulderBody.is_null() { ZERO3 } else { a3(calibratedShoulderBody) };
    let hfs = sub3(&controller_body, &shoulder_body);
    let map_local = [hfs[0] * s, -hfs[2] * s, hfs[1] * s];
    let (sp, of) = (a3(shoulderModelPos), a3(off));
    put3(
        outTarget,
        &[sp[0] + map_local[0] + of[0], sp[1] + map_local[1] + of[1], sp[2] + map_local[2] + of[2]],
    );
    let mut base_quat = quat_mul(&hq, &a4(vrQuat));
    quat_norm(&mut base_quat);
    let map_quat = [base_quat[0], -base_quat[2], base_quat[1], base_quat[3]];
    let mut r = quat_mul(&map_quat, &a4(wristCorr));
    quat_norm(&mut r);
    put4(outHandRot, &r);
}

/// The arm's rest translations, captured the first time each bone is seen for this track buffer and bone count.
unsafe fn arm_rest_trans(boneBuf: *mut u8, trackBuf: usize, boneCount: i32, idx: i32, out: &mut V3) -> bool {
    if boneBuf.is_null() || idx < 0 || idx >= MAXB || idx >= boneCount {
        return false;
    }
    if trackBuf != s_armRestTrackBuf || boneCount != s_armRestBoneCount {
        s_armRestValid = [false; MAX_BONES];
        s_armRestTrackBuf = trackBuf;
        s_armRestBoneCount = boneCount;
    }
    if !s_armRestValid[idx as usize] {
        s_armRestTrans[idx as usize] = a3(bt(boneBuf, idx));
        s_armRestValid[idx as usize] = true;
    }
    *out = s_armRestTrans[idx as usize];
    true
}

/// Each arm segment given half the player's T-pose shoulder->wrist length, along its rest direction.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ScaleArmBonesFromRest(
    boneBuf: *mut u8,
    trackBuf: usize,
    boneCount: i32,
    foreIdx: i32,
    handIdx: i32,
    userArmLen: f32,
) {
    // (two comparisons, as the C++: a NaN passes them)
    if userArmLen < 0.45 || userArmLen > 0.85 {
        return;
    }
    let (mut fore_rest, mut hand_rest) = (ZERO3, ZERO3);
    if !arm_rest_trans(boneBuf, trackBuf, boneCount, foreIdx, &mut fore_rest) {
        return;
    }
    if !arm_rest_trans(boneBuf, trackBuf, boneCount, handIdx, &mut hand_rest) {
        return;
    }
    let upper_len = sqrt(fore_rest[0] * fore_rest[0] + fore_rest[1] * fore_rest[1] + fore_rest[2] * fore_rest[2]);
    let fore_len = sqrt(hand_rest[0] * hand_rest[0] + hand_rest[1] * hand_rest[1] + hand_rest[2] * hand_rest[2]);
    if upper_len < 0.02 || fore_len < 0.02 {
        return;
    }
    // Even 50/50 split (the measured real bicep/forearm are near-equal).
    let target_upper = userArmLen * 0.50;
    let target_fore = userArmLen * 0.50;
    let mut upper_scale = target_upper / upper_len;
    let mut fore_scale = target_fore / fore_len;
    if upper_scale < 0.65 {
        upper_scale = 0.65;
    }
    if upper_scale > 1.45 {
        upper_scale = 1.45;
    }
    if fore_scale < 0.65 {
        fore_scale = 0.65;
    }
    if fore_scale > 1.45 {
        fore_scale = 1.45;
    }
    put3(bt(boneBuf, foreIdx), &[fore_rest[0] * upper_scale, fore_rest[1] * upper_scale, fore_rest[2] * upper_scale]);
    put3(bt(boneBuf, handIdx), &[hand_rest[0] * fore_scale, hand_rest[1] * fore_scale, hand_rest[2] * fore_scale]);
}

/// PUT THE ARM'S TRANSLATIONS BACK for an arm handed to the driving animation: the segment lengths and the shoulder
/// protraction are writes the engine does not revert.
#[no_mangle]
pub unsafe extern "C" fn VRIK_RestoreArmRestTrans(
    boneBuf: *mut u8,
    trackBuf: usize,
    boneCount: i32,
    upperIdx: i32,
    foreIdx: i32,
    handIdx: i32,
    isLeft: bool,
) {
    let mut rest = ZERO3;
    if (0..MAXB).contains(&foreIdx) && arm_rest_trans(boneBuf, trackBuf, boneCount, foreIdx, &mut rest) {
        put3(bt(boneBuf, foreIdx), &rest);
    }
    if (0..MAXB).contains(&handIdx) && arm_rest_trans(boneBuf, trackBuf, boneCount, handIdx, &mut rest) {
        put3(bt(boneBuf, handIdx), &rest);
    }
    let side = if isLeft { 1 } else { 0 };
    if (0..MAXB).contains(&upperIdx) && g_vrikUpArmRestCap[side] {
        put3(bt(boneBuf, upperIdx), &g_vrikUpArmRest[side]);
    }
}
