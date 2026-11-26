//! The weapon hand's own pieces (namespace cvr::anim): the weapon in the left hand and the rig's mirror between the
//! hands (formerly src/Anim/LeftHanded.cpp), head aim (HeadAimWeapon.cpp), the animated-skeleton publish for the
//! reload recorder (FkRecorder.cpp), the ADS pose moved onto the sighting eye (AdsEyeAlign.cpp) and the non-VRIK ADS
//! muzzle stabilizer (AdsMuzzleStabilizer.cpp). Exported as cvr_anim_*; the C++ namespace forwards
//! (src/Anim/CharacterRigRust.cpp).
use super::camera::VRIK_ComputeCamModel;
use super::cyberware::cvr_cyberware_selected_kind;
use super::env::*;
use super::rig::*;
use super::snapshot::{SharedPose, VRIK_ResolveViewPos};
use super::*;
use crate::math::{dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec, quat_scale};
use crate::printf::format_g;
use crate::skeleton::equal_bone_name;
use core::ffi::{c_char, CStr};
use cvr_body::cmath::{acos, cos, sin, sqrt};

extern "C" {
    static mut g_hasLeftWeaponEquipped: bool;
    static mut g_hasRightWeaponEquipped: bool;
    static mut g_isAiming: bool;
    static mut g_VRSmokeLighterIdx: i32;
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRWeaponPsmState: f32;
    static mut g_VRWeaponRaiseTransition: i32;
    static mut g_VRAimInRemaining: f32;
    static mut CyberpunkVR_CarryLeft: i32;
    static mut CyberpunkVR_DualWieldUntil: u64;
    // probes and switches (src/Anim/CharacterRigState.cpp)
    static mut CyberpunkVR_DebugLeftHandedTwins: i32;
    static mut CyberpunkVR_DebugLeftHandedResidual: f32;
    static mut CyberpunkVR_DebugFingerFlex: [f32; 4];
    static mut CyberpunkVR_NonVrikAdsStabilizer: i32;
    static mut CyberpunkVR_DebugAdsStabApplies: u64;
    static mut CyberpunkVR_DebugAdsStabHipCaptures: u64;
}

const K_WEAPON_FLAG: usize = 144; // Utils/SharedSlots.hpp vrshared::kWeaponFlag
const K_ENTITY_SEQ: usize = 99; // vrshared::kEntitySeq

// ================================================================================================ the mirror

#[derive(Clone, Copy, Default)]
struct Mat3 {
    m: [[f32; 3]; 3],
}
fn from_quat(q: &Q) -> Mat3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    Mat3 {
        m: [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ],
    }
}
fn to_quat(r: &Mat3) -> Q {
    let m = &r.m;
    let trace = m[0][0] + m[1][1] + m[2][2];
    let mut q = [0.0f32; 4];
    if trace > 0.0 {
        let s = sqrt(trace + 1.0) * 2.0;
        q[3] = 0.25 * s;
        q[0] = (m[2][1] - m[1][2]) / s;
        q[1] = (m[0][2] - m[2][0]) / s;
        q[2] = (m[1][0] - m[0][1]) / s;
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2.0;
        q[3] = (m[2][1] - m[1][2]) / s;
        q[0] = 0.25 * s;
        q[1] = (m[0][1] + m[1][0]) / s;
        q[2] = (m[0][2] + m[2][0]) / s;
    } else if m[1][1] > m[2][2] {
        let s = sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2.0;
        q[3] = (m[0][2] - m[2][0]) / s;
        q[0] = (m[0][1] + m[1][0]) / s;
        q[1] = 0.25 * s;
        q[2] = (m[1][2] + m[2][1]) / s;
    } else {
        let s = sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2.0;
        q[3] = (m[1][0] - m[0][1]) / s;
        q[0] = (m[0][2] + m[2][0]) / s;
        q[1] = (m[1][2] + m[2][1]) / s;
        q[2] = 0.25 * s;
    }
    quat_norm(&mut q);
    q
}
fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut o = Mat3::default();
    for i in 0..3 {
        for j in 0..3 {
            o.m[i][j] = a.m[i][0] * b.m[0][j] + a.m[i][1] * b.m[1][j] + a.m[i][2] * b.m[2][j];
        }
    }
    o
}
fn transposed(a: &Mat3) -> Mat3 {
    let mut o = Mat3::default();
    for i in 0..3 {
        for j in 0..3 {
            o.m[i][j] = a.m[j][i];
        }
    }
    o
}
fn apply(a: &Mat3, v: &V3) -> V3 {
    let mut o = [0.0f32; 3];
    for i in 0..3 {
        o[i] = a.m[i][0] * v[0] + a.m[i][1] * v[1] + a.m[i][2] * v[2];
    }
    o
}
/// a * R * b on a rotation given as a quaternion: what every mirrored rotation here is.
fn conjugated(a: &Mat3, q: &Q, b: &Mat3) -> Q {
    to_quat(&mul(&mul(a, &from_quat(q)), b))
}

#[derive(Clone, Copy)]
struct Twin {
    right: i32,
    left: i32,
    s: Mat3,
    /// fingers: the twin of their parents, -1 = the hands (-2: the weapons have none)
    parent: i32,
}
const TWIN0: Twin = Twin { right: -1, left: -1, s: Mat3 { m: [[0.0; 3]; 3] }, parent: -1 };
const MAX_FINGERS: usize = 32;
static mut HANDS: Twin = TWIN0;
static mut FINGERS: [Twin; MAX_FINGERS] = [TWIN0; MAX_FINGERS];
static mut FINGER_COUNT: usize = 0;
static mut READY: bool = false;
static mut WEAPONS: Twin = TWIN0;
static mut WEAPONS_READY: bool = false;

unsafe fn descends(mut bone: i32, ancestor: i32) -> bool {
    let mut guard = 0;
    while bone >= 0 && bone < MAXB && guard < MAXB {
        if bone == ancestor {
            return true;
        }
        bone = g_VRBoneParent[bone as usize] as i32;
        guard += 1;
    }
    false
}

/// The reflection between a right bone's reference frame and its left twin's, snapped to a signed permutation, with
/// the residual (the largest entry the solved matrix missed it by) and the positions' mirror error.
unsafe fn solve_twin(right: i32, left: i32) -> Option<(Twin, f32, f32)> {
    let (mut pr, mut qr, mut pl, mut ql) = ([0.0f32; 3], [0.0f32; 4], [0.0f32; 3], [0.0f32; 4]);
    if !VRIK_ReferenceModel(right, pr.as_mut_ptr(), qr.as_mut_ptr()) || !VRIK_ReferenceModel(left, pl.as_mut_ptr(), ql.as_mut_ptr()) {
        return None;
    }
    let mut ml = from_quat(&ql);
    for c in 0..3 {
        ml.m[0][c] = -ml.m[0][c]; // M * R_left
    }
    let raw = mul(&transposed(&from_quat(&qr)), &ml);
    let mut snap = Mat3::default();
    let mut used = [false; 3];
    for i in 0..3 {
        let mut best = 0;
        for j in 1..3 {
            if raw.m[i][j].abs() > raw.m[i][best].abs() {
                best = j;
            }
        }
        if used[best] {
            return None;
        }
        used[best] = true;
        snap.m[i][best] = if raw.m[i][best] < 0.0 { -1.0 } else { 1.0 };
    }
    let s = &snap.m;
    let det = s[0][0] * (s[1][1] * s[2][2] - s[1][2] * s[2][1]) - s[0][1] * (s[1][0] * s[2][2] - s[1][2] * s[2][0])
        + s[0][2] * (s[1][0] * s[2][1] - s[1][1] * s[2][0]);
    if det > 0.0 {
        return None;
    }
    let mut worst = 0.0f32;
    for i in 0..3 {
        for j in 0..3 {
            worst = worst.max((raw.m[i][j] - snap.m[i][j]).abs());
        }
    }
    let (dx, dy, dz) = (-pr[0] - pl[0], pr[1] - pl[1], pr[2] - pl[2]);
    Some((Twin { right, left, s: snap, parent: -1 }, worst, sqrt(dx * dx + dy * dy + dz * dz)))
}

unsafe fn parent_reflection(t: &Twin) -> Mat3 {
    if t.parent >= 0 {
        FINGERS[t.parent as usize].s
    } else {
        HANDS.s
    }
}

/// The only weapon in hand hangs on the left slot (not a carried gun, not an implanted arm).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_weapon_in_left_hand() -> bool {
    if !g_hasLeftWeaponEquipped || g_hasRightWeaponEquipped || CyberpunkVR_CarryLeft != 0 {
        return false;
    }
    let kind = cvr_cyberware_selected_kind();
    !(1..=4).contains(&kind)
}

/// WeaponLeft while the weapon is in the left hand, WeaponRight otherwise; -1 until the rig is armed.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_held_weapon_bone() -> i32 {
    if cvr_anim_weapon_in_left_hand() {
        g_VRSmokeLighterIdx
    } else {
        g_VRSmokeCigIdx
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_left_handed_arm_rig(names: *const *const c_char, count: i32) {
    READY = false;
    FINGER_COUNT = 0;
    HANDS = TWIN0;
    WEAPONS = TWIN0;
    WEAPONS_READY = false;
    if names.is_null() || count <= 0 {
        return;
    }
    let count = count.min(MAXB);
    let name = |i: i32| -> Option<&[u8]> {
        let p = *names.offset(i as isize);
        if p.is_null() {
            None
        } else {
            Some(CStr::from_ptr(p).to_bytes())
        }
    };
    let find = |wanted: &str| -> i32 {
        for i in 0..count {
            if let Some(n) = name(i) {
                if equal_bone_name(n, wanted.as_bytes()) {
                    return i;
                }
            }
        }
        -1
    };
    let (mut worst, mut worst_mirror) = (0.0f32, 0.0f32);
    let mut solve = |right: i32, left: i32| -> Option<Twin> {
        if right < 0 || left < 0 {
            return None;
        }
        let (t, residual, mirror) = solve_twin(right, left)?;
        // 0.2 is a frame eleven degrees off any mirror; 3 cm a twin that is not where the other one is reflected
        if residual > 0.2 || mirror > 0.03 {
            return None;
        }
        worst = worst.max(residual);
        worst_mirror = worst_mirror.max(mirror);
        Some(t)
    };
    let (right_hand, left_hand) = (find("RightHand"), find("LeftHand"));
    match solve(right_hand, left_hand) {
        Some(t) => HANDS = t,
        None => {
            log("[left-handed] rig has no mirrored hands: the support hand keeps its own holds\n");
            return;
        }
    }
    // The fingers: every Right* deform bone under the right hand (no '_', no *End), each with its Left* twin.
    for bone in 0..count {
        if FINGER_COUNT >= MAX_FINGERS {
            break;
        }
        let Some(n) = name(bone) else { continue };
        if bone == right_hand
            || n.len() < 5
            || !equal_bone_name(&n[..5], b"Right")
            || n.contains(&b'_')
            || n.windows(3).any(|w| w == b"End")
            || !descends(bone, right_hand)
        {
            continue;
        }
        let mut twin_name = b"Left".to_vec();
        twin_name.extend_from_slice(&n[5..]);
        twin_name.truncate(95);
        let twin = find(&String::from_utf8_lossy(&twin_name));
        if twin < 0 || !descends(twin, left_hand) {
            continue;
        }
        let Some(mut t) = solve(bone, twin) else { continue };
        let parent_right = g_VRBoneParent[bone as usize] as i32;
        t.parent = -1;
        if parent_right != right_hand {
            for k in 0..FINGER_COUNT {
                if FINGERS[k].right == parent_right {
                    t.parent = k as i32;
                }
            }
            if t.parent < 0 || FINGERS[t.parent as usize].left != g_VRBoneParent[twin as usize] as i32 {
                continue;
            }
        } else if g_VRBoneParent[twin as usize] as i32 != left_hand {
            continue;
        }
        FINGERS[FINGER_COUNT] = t;
        FINGER_COUNT += 1;
    }
    // The weapon bones hang off the hands (or a finger of them): the same test of their parents.
    let (right_weapon, left_weapon) = (find("WeaponRight"), find("WeaponLeft"));
    let (mut weapon_residual, mut weapon_mirror) = (0.0f32, 0.0f32);
    if right_weapon >= 0 && left_weapon >= 0 {
        if let Some((t, residual, mirror)) = solve_twin(right_weapon, left_weapon) {
            weapon_residual = residual;
            weapon_mirror = mirror;
            WEAPONS = t;
            if residual <= 0.2 && mirror <= 0.03 {
                let (parent_right, parent_left) =
                    (g_VRBoneParent[right_weapon as usize] as i32, g_VRBoneParent[left_weapon as usize] as i32);
                WEAPONS.parent = -2;
                if parent_right == right_hand && parent_left == left_hand {
                    WEAPONS.parent = -1;
                }
                for k in 0..FINGER_COUNT {
                    if FINGERS[k].right == parent_right && FINGERS[k].left == parent_left {
                        WEAPONS.parent = k as i32;
                    }
                }
                WEAPONS_READY = WEAPONS.parent >= -1;
            }
        }
    }
    if !WEAPONS_READY {
        log(&format!(
            "[left-handed] no mirrored weapon bones (residual {:.3}, mirror {:.1} mm): props stay in their own hand\n",
            weapon_residual,
            weapon_mirror * 1000.0
        ));
    }
    READY = true;
    CyberpunkVR_DebugLeftHandedTwins = FINGER_COUNT as i32 + 1;
    CyberpunkVR_DebugLeftHandedResidual = worst;
    if !cvr_host::log::verbose() {
        return;
    }
    let h = HANDS.s.m;
    let g = |v: f32| format_g(v as f64, 6);
    log(&format!(
        "[left-handed] twins: hands, {} fingers; residual {:.3}, mirror {:.1} mm; S hand [{} {} {}|{} {} {}|{} {} {}]\n",
        FINGER_COUNT,
        worst,
        worst_mirror * 1000.0,
        g(h[0][0]),
        g(h[0][1]),
        g(h[0][2]),
        g(h[1][0]),
        g(h[1][1]),
        g(h[1][2]),
        g(h[2][0]),
        g(h[2][1]),
        g(h[2][2])
    ));
}

/// Each hand's mean finger curl off the reference pose (CyberpunkVR_DebugFingerFlex).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_note_finger_flex(boneBuf: *const u8, drawn: bool) {
    if boneBuf.is_null() || !READY || FINGER_COUNT == 0 {
        return;
    }
    for side in 0..2 {
        let (mut sum, mut n) = (0.0f32, 0);
        for k in 0..FINGER_COUNT {
            let bone = if side == 0 { FINGERS[k].left } else { FINGERS[k].right };
            let (mut p, mut r) = ([0.0f32; 3], [0.0f32; 4]);
            if !VRIK_ReferenceLocal(bone, p.as_mut_ptr(), r.as_mut_ptr()) {
                continue;
            }
            let q = a4(br(boneBuf as *mut u8, bone));
            let d = (q[0] * r[0] + q[1] * r[1] + q[2] * r[2] + q[3] * r[3]).abs();
            sum += 2.0 * acos(1.0f32.min(d)) * 57.29578;
            n += 1;
        }
        if n > 0 {
            CyberpunkVR_DebugFingerFlex[(if drawn { 2 } else { 0 }) + side] = sum / n as f32;
        }
    }
}

/// A hold captured with the weapon in the right hand, as the same hold with the weapon in the left (in place).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_mirror_held_hand(pos: *mut f32, rot: *mut f32) {
    if !READY || pos.is_null() || rot.is_null() {
        return;
    }
    let st = transposed(&HANDS.s);
    let p = apply(&st, &a3(pos));
    let q = conjugated(&st, &a4(rot), &st);
    put3(pos, &p);
    put4(rot, &q);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_mirror_finger_to_right(leftBone: i32, rot: *const f32, rightBone: *mut i32, out: *mut f32) -> bool {
    if !READY || rot.is_null() || rightBone.is_null() || out.is_null() {
        return false;
    }
    for k in 0..FINGER_COUNT {
        let t = FINGERS[k];
        if t.left != leftBone {
            continue;
        }
        // local_right = S_parent * local_left * S^T
        put4(out, &conjugated(&parent_reflection(&t), &a4(rot), &transposed(&t.s)));
        *rightBone = t.right;
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_mirror_weapon_slot_to_right(pos: *const f32, rot: *const f32, outPos: *mut f32, outRot: *mut f32) -> bool {
    if !READY || !WEAPONS_READY || pos.is_null() || rot.is_null() || outPos.is_null() || outRot.is_null() {
        return false;
    }
    let parent = parent_reflection(&WEAPONS);
    put3(outPos, &apply(&parent, &a3(pos)));
    put4(outRot, &conjugated(&parent, &a4(rot), &transposed(&WEAPONS.s)));
    true
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_guns_in_hand() -> bool {
    g_hasLeftWeaponEquipped && g_hasRightWeaponEquipped && GetTickCount64() < core::ptr::read_volatile(&raw const CyberpunkVR_DualWieldUntil)
}

// ================================================================================================ head aim

/// The weapon-aim toggle OFF, a weapon out and its muzzle published (single-float flags, read directly).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_is_head_aim_weapon_active() -> bool {
    let sh = g_pSharedHands;
    !sh.is_null() && *sh.add(58) <= 0.5 && *sh.add(K_WEAPON_FLAG) > 0.5 && *sh.add(27) > 0.5
}

/// The latched view orientation as the weapon's model rotation, through the hand (the grip stays as animated).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_apply_head_aim_weapon_orientation(boneBuf: *mut u8) {
    if !cvr_anim_is_head_aim_weapon_active() {
        return;
    }
    let weapon_idx = cvr_anim_held_weapon_bone();
    if weapon_idx < 0 || weapon_idx >= VRIK_FKCount() || !g_viewPktValid {
        return;
    }
    let mut view_q = [g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3]];
    quat_norm(&mut view_q);
    let mut ent_q = [g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR];
    quat_norm(&mut ent_q);
    let inv_ent = quat_conj(&ent_q);
    let mut view_model = quat_mul(&inv_ent, &view_q);
    quat_norm(&mut view_model);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    cvr_anim_write_weapon_model_rot_via_hand(boneBuf, weapon_idx, view_model.as_ptr(), core::ptr::null());
}

// ================================================================================================ the recorder

/// The engine's ANIMATED bones published (VRIK off), for authoring reload poses.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_publish_animated_fk(boneBuf: *mut u8) {
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let fk_n = VRIK_FKCount();
    let lim = if fk_n < MAXB { fk_n } else { MAXB };
    for i in 0..lim.max(0) as usize {
        g_VRFKSnapPos[i] = g_fkPos[i];
        g_VRFKSnapRot[i] = g_fkRot[i];
    }
    g_VRFKSnapCount = lim;
}

// ================================================================================================ ADS on the eye

/// A desired WEAPON model rotation applied through its parent hand; the authored grip (the weapon's local, or the
/// override) is divided out and left alone.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_write_weapon_model_rot_via_hand(
    boneBuf: *mut u8,
    weaponIdx: i32,
    desiredWeaponModel: *const f32,
    weaponLocalOverride: *const f32,
) -> bool {
    if weaponIdx < 0 || weaponIdx >= VRIK_FKCount() {
        return false;
    }
    let hand_idx = g_VRBoneParent[weaponIdx as usize] as i32;
    if hand_idx < 0 || hand_idx >= VRIK_FKCount() || (hand_idx != g_VRRightBoneIdx && hand_idx != g_VRLeftBoneIdx) {
        return false;
    }
    let hand_parent = g_VRBoneParent[hand_idx as usize] as i32;
    if hand_parent < 0 || hand_parent >= hand_idx {
        return false;
    }
    let mut weapon_local = if weaponLocalOverride.is_null() { a4(br(boneBuf, weaponIdx)) } else { a4(weaponLocalOverride) };
    quat_norm(&mut weapon_local);
    let inv_weapon_local = quat_conj(&weapon_local);
    let mut desired_hand_model = quat_mul(&a4(desiredWeaponModel), &inv_weapon_local);
    quat_norm(&mut desired_hand_model);
    write_local_rot(boneBuf, hand_idx, &g_fkRot[hand_parent as usize], &desired_hand_model);
    true
}

#[derive(Clone, Copy)]
struct AimArmPose {
    bone_buf: *mut u8,
    tick: f32,
    bone: [i32; 6],
    local_rot: [Q; 6],
    raw_pos: [V3; 6],
    raw_rot: [Q; 6],
    target_hand: [V3; 2],
    target_elbow: [V3; 2],
    target_support_rot: Q,
    valid: bool,
}
const AIM0: AimArmPose = AimArmPose {
    bone_buf: core::ptr::null_mut(),
    tick: -1.0,
    bone: [-1; 6],
    local_rot: [[0.0; 4]; 6],
    raw_pos: [[0.0; 3]; 6],
    raw_rot: [[0.0; 4]; 6],
    target_hand: [[0.0; 3]; 2],
    target_elbow: [[0.0; 3]; 2],
    target_support_rot: [0.0, 0.0, 0.0, 1.0],
    valid: false,
};
static mut AIM_ARM_POSE: [AimArmPose; 4] = [AIM0; 4];
static mut PREV_HEAD_AIM: bool = false;
static mut PREV_AIMING: bool = false;
static mut HEAD_ANCHOR_VALID: bool = false;
static mut NON_VRIK_ANCHOR_VALID: bool = false;
static mut HEAD_CENTRE_OFFSET_CAM: V3 = [0.0; 3];
static mut NON_VRIK_EYE_OFFSET_CAM: V3 = [0.0; 3];

struct SightingEye {
    model: V3,
    view_model: Q,
    cam_model_pos: V3,
    centre_model_rot: Q,
}
/// The sighting eye (side +1 right, -1 left) in model space, the view rotation, the camera's model position and the
/// CENTRED camera rotation with the HMD divided out.
unsafe fn current_sighting_eye(side: f32) -> Option<SightingEye> {
    let mut cam_pos = [0.0f32; 3];
    let mut unused_cam_rot = [0.0f32; 4];
    if !g_viewPktValid
        || !VRIK_ComputeCamModel(
            cam_pos.as_mut_ptr(),
            unused_cam_rot.as_mut_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    {
        return None;
    }
    let mut head_world = [0.0f32; 3];
    if !VRIK_ResolveViewPos(head_world.as_mut_ptr()) {
        return None;
    }
    let mut view_q = [g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3]];
    quat_norm(&mut view_q);
    let right_world = quat_rotate_vec(&view_q, &[1.0, 0.0, 0.0]);
    let half_ipd = SharedPose(95) * side;
    let world = [
        head_world[0] + right_world[0] * half_ipd,
        head_world[1] + right_world[1] * half_ipd,
        head_world[2] + right_world[2] * half_ipd,
    ];
    let mut ent_q = [g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR];
    quat_norm(&mut ent_q);
    let inv_ent = quat_conj(&ent_q);
    let eye_delta_world = [world[0] - g_VREntityPosX, world[1] - g_VREntityPosY, world[2] - g_VREntityPosZ];
    let model = quat_rotate_vec(&inv_ent, &eye_delta_world);
    let mut view_model = quat_mul(&inv_ent, &view_q);
    quat_norm(&mut view_model);
    // Divide out the exact raw HMD quaternion published beside this view, axis-mapped as the compose maps it.
    let mut hmd_game = [g_viewPkt[13], -g_viewPkt[15], g_viewPkt[14], g_viewPkt[16]];
    if (hmd_game[0] * hmd_game[0] + hmd_game[1] * hmd_game[1] + hmd_game[2] * hmd_game[2] + hmd_game[3] * hmd_game[3]) < 1e-6 {
        hmd_game = ID;
    } else {
        quat_norm(&mut hmd_game);
    }
    let inv_hmd_game = quat_conj(&hmd_game);
    let centre_world = quat_mul(&view_q, &inv_hmd_game);
    let mut centre = quat_mul(&inv_ent, &centre_world);
    quat_norm(&mut centre);
    Some(SightingEye { model, view_model, cam_model_pos: cam_pos, centre_model_rot: centre })
}

/// Rotation-only two-bone solve for an authored arm pose: segment translations untouched.
unsafe fn solve_aim_arm(
    boneBuf: *mut u8,
    upper: i32,
    fore: i32,
    hand: i32,
    raw_pos: &[V3],
    raw_rot: &[Q],
    target_hand: &V3,
    elbow_hint: &V3,
    target_hand_rot: &Q,
) {
    let mut up_vec = sub3(&raw_pos[1], &raw_pos[0]);
    let mut fore_vec = sub3(&raw_pos[2], &raw_pos[1]);
    let up_len = norm3(&mut up_vec);
    let fore_len = norm3(&mut fore_vec);
    let mut to_hand = sub3(target_hand, &raw_pos[0]);
    let dist = norm3(&mut to_hand);
    if up_len < 1e-4 || fore_len < 1e-4 || dist < 1e-4 {
        return;
    }
    let min_d = (up_len - fore_len).abs() + 1e-4;
    let max_d = up_len + fore_len - 1e-4;
    let mut reach = dist;
    if reach < min_d {
        reach = min_d;
    }
    if reach > max_d {
        reach = max_d;
    }
    let along = (up_len * up_len - fore_len * fore_len + reach * reach) / (2.0 * reach);
    let height = sqrt(0.0f32.max(up_len * up_len - along * along));
    let line_point = [raw_pos[0][0] + to_hand[0] * along, raw_pos[0][1] + to_hand[1] * along, raw_pos[0][2] + to_hand[2] * along];
    let mut bend = sub3(elbow_hint, &line_point);
    let mut proj = dot3(&bend, &to_hand);
    for k in 0..3 {
        bend[k] -= to_hand[k] * proj;
    }
    if norm3(&mut bend) < 1e-4 {
        // The hint collapsed onto the shoulder-to-hand line: the authored elbow's own bend.
        bend = sub3(&raw_pos[1], &line_point);
        proj = dot3(&bend, &to_hand);
        for k in 0..3 {
            bend[k] -= to_hand[k] * proj;
        }
        if norm3(&mut bend) < 1e-4 {
            return;
        }
    }
    let new_elbow = [line_point[0] + bend[0] * height, line_point[1] + bend[1] * height, line_point[2] + bend[2] * height];
    let mut desired_up = sub3(&new_elbow, &raw_pos[0]);
    norm3(&mut desired_up);
    let mut desired_fore = sub3(target_hand, &new_elbow);
    norm3(&mut desired_fore);
    let d_up = quat_from_to(&up_vec, &desired_up);
    let mut new_up = quat_mul(&d_up, &raw_rot[0]);
    quat_norm(&mut new_up);
    let d_fore = quat_from_to(&fore_vec, &desired_fore);
    let mut new_fore = quat_mul(&d_fore, &raw_rot[1]);
    quat_norm(&mut new_fore);
    let up_parent = g_VRBoneParent[upper as usize] as i32;
    let prot = if up_parent >= 0 && up_parent < VRIK_FKCount() { g_fkRot[up_parent as usize] } else { ID };
    write_local_rot(boneBuf, upper, &prot, &new_up);
    write_local_rot(boneBuf, fore, &new_up, &new_fore);
    write_local_rot(boneBuf, hand, &new_fore, target_hand_rot);
}

/// The authored arm pose recorded for this tick and eye-anchored hand/elbow targets computed from it.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_prepare_aim_arm_targets(boneBuf: *mut u8) {
    let head_aim = cvr_anim_is_head_aim_weapon_active();
    let sh = g_pSharedHands;
    let non_vrik = !sh.is_null() && g_VRBind <= 0 && CyberpunkVR_NonVrikAdsStabilizer != 0 && *sh.add(K_WEAPON_FLAG) > 0.5;
    let aiming = g_isAiming;
    let active = aiming && (head_aim || non_vrik);
    let mut pose: *mut AimArmPose = core::ptr::null_mut();
    for entry in AIM_ARM_POSE.iter_mut() {
        entry.valid = false;
        if entry.bone_buf == boneBuf {
            pose = entry;
            break;
        }
        if pose.is_null() && entry.bone_buf.is_null() {
            pose = entry;
        }
    }
    if !active || pose.is_null() {
        if !aiming {
            HEAD_ANCHOR_VALID = false;
            NON_VRIK_ANCHOR_VALID = false;
        }
        PREV_HEAD_AIM = head_aim;
        PREV_AIMING = aiming;
        return;
    }
    let pose = &mut *pose;
    if pose.bone_buf != boneBuf {
        pose.bone_buf = boneBuf;
        pose.tick = -1.0;
    }
    let left_weapon = cvr_anim_weapon_in_left_hand();
    let right = [g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx];
    let left = [g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx];
    let (weapon_arm, support_arm) = if left_weapon { (left, right) } else { (right, left) };
    let bone = [weapon_arm[0], weapon_arm[1], weapon_arm[2], support_arm[0], support_arm[1], support_arm[2]];
    for b in bone {
        if b < 0 || b >= VRIK_FKCount() {
            return;
        }
    }
    // THE TICK: the first pass records the authored rotations, the repeats restore them.
    let tick = *sh.add(K_ENTITY_SEQ);
    let mut same_pose = pose.tick == tick;
    for i in 0..6 {
        same_pose = same_pose && pose.bone[i] == bone[i];
    }
    if same_pose {
        for i in 0..6 {
            put4(br(boneBuf, bone[i]), &pose.local_rot[i]);
        }
    } else {
        for i in 0..6 {
            pose.bone[i] = bone[i];
            pose.local_rot[i] = a4(br(boneBuf, bone[i]));
        }
        pose.tick = tick;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    for i in 0..6 {
        pose.raw_pos[i] = g_fkPos[bone[i] as usize];
        pose.raw_rot[i] = g_fkRot[bone[i] as usize];
    }
    let eye_side = if left_weapon { -1.0f32 } else { 1.0 };
    let Some(eye) = current_sighting_eye(eye_side) else { return };
    let (cam_pos, centre_rot, view_model) = (eye.cam_model_pos, eye.centre_model_rot, eye.view_model);
    let inv_centre = quat_conj(&centre_rot);
    let mut live_delta = quat_mul(&view_model, &inv_centre);
    quat_norm(&mut live_delta);
    // The eye and the cyclopean point it orbits, in the centred camera's frame.
    let eye_from_centre = sub3(&eye.model, &cam_pos);
    let eye_offset_cam = quat_rotate_vec(&inv_centre, &eye_from_centre);
    let mut eye_side_offset = quat_rotate_vec(&view_model, &[1.0, 0.0, 0.0]);
    let half_ipd = SharedPose(95) * eye_side;
    for k in 0..3 {
        eye_side_offset[k] *= half_ipd;
    }
    let head_centre = sub3(&eye.model, &eye_side_offset);
    let head_from_cam = sub3(&head_centre, &cam_pos);
    let head_offset_cam = quat_rotate_vec(&inv_centre, &head_from_cam);
    if head_aim && (!PREV_HEAD_AIM || !PREV_AIMING || !HEAD_ANCHOR_VALID) {
        HEAD_CENTRE_OFFSET_CAM = head_offset_cam;
        HEAD_ANCHOR_VALID = true;
    }
    if non_vrik && aiming && (!PREV_AIMING || PREV_HEAD_AIM || !NON_VRIK_ANCHOR_VALID) {
        NON_VRIK_EYE_OFFSET_CAM = eye_offset_cam;
        NON_VRIK_ANCHOR_VALID = true;
    }
    if !head_aim {
        HEAD_ANCHOR_VALID = false;
    }
    if !aiming {
        NON_VRIK_ANCHOR_VALID = false;
    }
    PREV_HEAD_AIM = head_aim;
    PREV_AIMING = aiming;
    let mut anchor = [0.0f32; 3];
    let delta: Q;
    if head_aim && HEAD_ANCHOR_VALID {
        // Head aim: the latched head centre plus the LIVE sighting-eye orbit; the arms rotate with the head.
        let head_off = quat_rotate_vec(&centre_rot, &HEAD_CENTRE_OFFSET_CAM);
        for k in 0..3 {
            anchor[k] = cam_pos[k] + head_off[k] + eye_side_offset[k];
        }
        delta = live_delta;
    } else if non_vrik && NON_VRIK_ANCHOR_VALID {
        // Hand aim: the game owns the direction; only the translation moves onto the eye.
        let eye_off = quat_rotate_vec(&centre_rot, &NON_VRIK_EYE_OFFSET_CAM);
        for k in 0..3 {
            anchor[k] = cam_pos[k] + eye_off[k];
        }
        delta = ID;
    } else {
        return;
    }
    let (hand_slot, elbow_slot) = ([2usize, 5], [1usize, 4]);
    for side in 0..2 {
        let rel_h = sub3(&pose.raw_pos[hand_slot[side]], &cam_pos);
        let rel_e = sub3(&pose.raw_pos[elbow_slot[side]], &cam_pos);
        let rot_h = quat_rotate_vec(&delta, &rel_h);
        let rot_e = quat_rotate_vec(&delta, &rel_e);
        for k in 0..3 {
            pose.target_hand[side][k] = anchor[k] + rot_h[k];
            pose.target_elbow[side][k] = anchor[k] + rot_e[k];
        }
    }
    pose.target_support_rot = quat_mul(&delta, &pose.raw_rot[5]);
    quat_norm(&mut pose.target_support_rot);
    pose.valid = true;
}

/// The two arms solved onto the prepared targets; the weapon hand keeps what the weapon writer gave it.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_solve_prepared_aim_arms(boneBuf: *mut u8) {
    let mut pose: Option<AimArmPose> = None;
    for entry in AIM_ARM_POSE.iter() {
        if entry.bone_buf == boneBuf {
            pose = Some(*entry);
            break;
        }
    }
    let Some(pose) = pose else { return };
    if !pose.valid {
        return;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let weapon_hand_rot = g_fkRot[pose.bone[2] as usize];
    solve_aim_arm(
        boneBuf,
        pose.bone[0],
        pose.bone[1],
        pose.bone[2],
        &pose.raw_pos[0..3],
        &pose.raw_rot[0..3],
        &pose.target_hand[0],
        &pose.target_elbow[0],
        &weapon_hand_rot,
    );
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    solve_aim_arm(
        boneBuf,
        pose.bone[3],
        pose.bone[4],
        pose.bone[5],
        &pose.raw_pos[3..6],
        &pose.raw_rot[3..6],
        &pose.target_hand[1],
        &pose.target_elbow[1],
        &pose.target_support_rot,
    );
}

// ================================================================================================ the stabilizer

#[derive(Clone, Copy)]
struct RawWeaponPoseCache {
    bone_buf: *mut u8,
    tick: f32,
    weapon_idx: i32,
    hand_idx: i32,
    weapon_local_rot: Q,
    hand_local_rot: Q,
}
const RAW0: RawWeaponPoseCache =
    RawWeaponPoseCache { bone_buf: core::ptr::null_mut(), tick: -1.0, weapon_idx: -1, hand_idx: -1, weapon_local_rot: ID, hand_local_rot: ID };
struct Stabilizer {
    hip_aim_heading: V3,
    total_model_correction: Q,
    frozen_weapon_local_correction: Q,
    last_tick: f32,
    hip_aim_valid: bool,
    prev_aiming: bool,
    ads_correction_frozen: bool,
    ads_saw_aim_in: bool,
    freeze_local_correction_pending: bool,
    ads_transition_ticks: i32,
    ads_converged_ticks: i32,
    raw_pose_cache: [RawWeaponPoseCache; 4],
    raw_pose_replace: u32,
}
static mut STAB: Stabilizer = Stabilizer {
    hip_aim_heading: [0.0, 1.0, 0.0],
    total_model_correction: ID,
    frozen_weapon_local_correction: ID,
    last_tick: -1.0,
    hip_aim_valid: false,
    prev_aiming: false,
    ads_correction_frozen: false,
    ads_saw_aim_in: false,
    freeze_local_correction_pending: false,
    ads_transition_ticks: 0,
    ads_converged_ticks: 0,
    raw_pose_cache: [RAW0; 4],
    raw_pose_replace: 0,
};

/// The vanilla ADS animation's muzzle direction corrected toward the Ready pose's aim (non-VRIK hand aim).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_apply_non_vrik_ads_muzzle_stabilizer(boneBuf: *mut u8) {
    let s = &mut STAB;
    let sh = g_pSharedHands;
    if sh.is_null()
        || cvr_anim_is_head_aim_weapon_active()
        || g_VRBind > 0
        || CyberpunkVR_NonVrikAdsStabilizer == 0
        || *sh.add(K_WEAPON_FLAG) <= 0.5
        || *sh.add(27) <= 0.5
    {
        s.total_model_correction = ID;
        s.frozen_weapon_local_correction = ID;
        s.prev_aiming = false;
        s.ads_correction_frozen = false;
        s.freeze_local_correction_pending = false;
        return;
    }
    let weapon_idx = cvr_anim_held_weapon_bone();
    if weapon_idx < 0 || weapon_idx >= VRIK_FKCount() {
        return;
    }
    let mut muzzle = [*sh.add(24), *sh.add(25), *sh.add(26)];
    if norm3(&mut muzzle) < 0.5 {
        return;
    }
    let mut ent_q = [g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR];
    quat_norm(&mut ent_q);
    // The heading from the packet latched for THIS solve.
    if !g_viewPktValid {
        return;
    }
    let heading = g_viewPkt[8];
    let aiming = g_isAiming;
    let tick = *sh.add(K_ENTITY_SEQ);
    if tick != s.last_tick {
        s.last_tick = tick;
        let weapon_psm = g_VRWeaponPsmState.round() as i32;
        let weapon_psm_ready = weapon_psm == 5;
        let safe_to_ready = g_VRWeaponRaiseTransition != 0;
        let aim_in_running = g_VRAimInRemaining > 0.001;
        if aiming && !s.prev_aiming {
            s.total_model_correction = ID;
            s.frozen_weapon_local_correction = ID;
            s.ads_correction_frozen = false;
            s.ads_saw_aim_in = aim_in_running;
            s.freeze_local_correction_pending = false;
            s.ads_transition_ticks = 0;
            s.ads_converged_ticks = 0;
        } else if !aiming && s.prev_aiming {
            s.ads_correction_frozen = false;
            s.ads_saw_aim_in = false;
            s.freeze_local_correction_pending = false;
            s.ads_transition_ticks = 0;
            s.ads_converged_ticks = 0;
        }
        if !aiming {
            s.total_model_correction = ID;
            // Weapon PSM 5 is the ranged Ready state: the reference is the barrel dot there, nowhere else.
            if weapon_psm_ready && !safe_to_ready {
                let (hs, hc) = (sin(heading * 0.5), cos(heading * 0.5));
                let inv_heading_q = [0.0, 0.0, -hs, hc];
                let mut local = quat_rotate_vec(&inv_heading_q, &muzzle);
                if norm3(&mut local) > 0.5 {
                    s.hip_aim_heading = local;
                    s.hip_aim_valid = true;
                    if CyberpunkVR_RuntimeDiagnostics != 0 {
                        CyberpunkVR_DebugAdsStabHipCaptures = CyberpunkVR_DebugAdsStabHipCaptures.wrapping_add(1);
                    }
                }
            }
        } else if s.hip_aim_valid && !s.ads_correction_frozen {
            if aim_in_running {
                s.ads_saw_aim_in = true;
            }
            let heading_q = [0.0, 0.0, sin(heading * 0.5), cos(heading * 0.5)];
            let mut desired_world = quat_rotate_vec(&heading_q, &s.hip_aim_heading);
            norm3(&mut desired_world);
            let error_world = quat_from_to(&muzzle, &desired_world);
            let mut alignment = muzzle[0] * desired_world[0] + muzzle[1] * desired_world[1] + muzzle[2] * desired_world[2];
            if alignment > 1.0 {
                alignment = 1.0;
            }
            if alignment < -1.0 {
                alignment = -1.0;
            }
            let error_radians = acos(alignment);
            s.ads_transition_ticks += 1;
            if error_radians < 0.2 * 0.01745329252 {
                if s.ads_converged_ticks < 1000 {
                    s.ads_converged_ticks += 1;
                }
            } else {
                s.ads_converged_ticks = 0;
            }
            // 35% of the error per tick, in model space (entity-conjugated), accumulated and clamped.
            let step_world = quat_scale(&error_world, 0.35);
            let inv_ent = quat_conj(&ent_q);
            let tmp = quat_mul(&inv_ent, &step_world);
            let mut step_model = quat_mul(&tmp, &ent_q);
            quat_norm(&mut step_model);
            let mut next = quat_mul(&step_model, &s.total_model_correction);
            quat_norm(&mut next);
            const K_MAX_CORRECTION_RADIANS: f32 = 15.0 * 0.01745329252;
            let mut w = next[3].abs();
            if w > 1.0 {
                w = 1.0;
            }
            let angle = 2.0 * acos(w);
            if angle > K_MAX_CORRECTION_RADIANS {
                next = quat_scale(&next, K_MAX_CORRECTION_RADIANS / angle);
            }
            s.total_model_correction = next;
            // AimInTimeRemaining ends the transition; convergence is the fallback for weapons without one.
            if (s.ads_saw_aim_in && !aim_in_running) || (!s.ads_saw_aim_in && s.ads_transition_ticks >= 20 && s.ads_converged_ticks >= 3) {
                s.ads_correction_frozen = true;
                s.freeze_local_correction_pending = true;
            }
        }
        s.prev_aiming = aiming;
    }
    if !aiming || !s.hip_aim_valid {
        return;
    }
    // ALWAYS COMPOSE FROM THE TICK'S RAW ROTATION.
    let mut raw_pose: *mut RawWeaponPoseCache = core::ptr::null_mut();
    for entry in s.raw_pose_cache.iter_mut() {
        if entry.bone_buf == boneBuf {
            raw_pose = entry;
            break;
        }
    }
    if raw_pose.is_null() {
        let slot = (s.raw_pose_replace & 3) as usize;
        s.raw_pose_replace = s.raw_pose_replace.wrapping_add(1);
        raw_pose = &mut s.raw_pose_cache[slot];
        (*raw_pose).bone_buf = boneBuf;
        (*raw_pose).tick = -1.0;
    }
    let raw_pose = &mut *raw_pose;
    let hand_idx = if cvr_anim_weapon_in_left_hand() { g_VRLeftBoneIdx } else { g_VRRightBoneIdx };
    if hand_idx < 0 || hand_idx >= VRIK_FKCount() || g_VRBoneParent[weapon_idx as usize] as i32 != hand_idx {
        return;
    }
    if raw_pose.tick != tick || raw_pose.weapon_idx != weapon_idx || raw_pose.hand_idx != hand_idx {
        raw_pose.weapon_local_rot = a4(br(boneBuf, weapon_idx));
        raw_pose.hand_local_rot = a4(br(boneBuf, hand_idx));
        quat_norm(&mut raw_pose.weapon_local_rot);
        quat_norm(&mut raw_pose.hand_local_rot);
        raw_pose.tick = tick;
        raw_pose.weapon_idx = weapon_idx;
        raw_pose.hand_idx = hand_idx;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let hand_parent = g_VRBoneParent[hand_idx as usize] as i32;
    if hand_parent < 0 || hand_parent >= hand_idx {
        return;
    }
    let mut raw_hand_model = quat_mul(&g_fkRot[hand_parent as usize], &raw_pose.hand_local_rot);
    quat_norm(&mut raw_hand_model);
    let mut raw_model = quat_mul(&raw_hand_model, &raw_pose.weapon_local_rot);
    quat_norm(&mut raw_model);
    let mut corrected_model;
    if s.ads_correction_frozen && !s.freeze_local_correction_pending {
        // Frozen: the correction rides in the WEAPON's frame.
        corrected_model = quat_mul(&raw_model, &s.frozen_weapon_local_correction);
    } else {
        corrected_model = quat_mul(&s.total_model_correction, &raw_model);
        if s.freeze_local_correction_pending {
            let inv_raw_model = quat_conj(&raw_model);
            s.frozen_weapon_local_correction = quat_mul(&inv_raw_model, &corrected_model);
            quat_norm(&mut s.frozen_weapon_local_correction);
            s.freeze_local_correction_pending = false;
        }
    }
    quat_norm(&mut corrected_model);
    let weapon_local = raw_pose.weapon_local_rot;
    cvr_anim_write_weapon_model_rot_via_hand(boneBuf, weapon_idx, corrected_model.as_ptr(), weapon_local.as_ptr());
    if CyberpunkVR_RuntimeDiagnostics != 0 {
        CyberpunkVR_DebugAdsStabApplies = CyberpunkVR_DebugAdsStabApplies.wrapping_add(1);
    }
}
