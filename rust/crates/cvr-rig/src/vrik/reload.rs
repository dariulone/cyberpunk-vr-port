//! The hands' finger layers outside the solve (formerly src/Anim/ReloadPose.cpp): the left hand's and the right hand's
//! resting fingers, recorded once on command and replayed while the other hand holds the weapon; the reload's finger
//! poses, blended onto the live fingers; the right hand's grip, learned while it is worn and offered back while the
//! left hand carries the weapon. Poses are parent-local rotations straight out of the pose buffer; the files are keyed
//! by bone name (`F <bone> qx qy qz qw`).
use super::env::*;
use super::*;
use core::ffi::{c_char, CStr};
use crate::printf::format_g9;
use cvr_body::cmath::sqrt;

extern "C" {
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerName: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    static mut g_VRRestFingerRot: [[f32; 4]; 32];
    static mut g_VRRestFingerHave: i32;
    static mut g_VRRestFingerCount: i32;
    static mut g_VRRestFingerApply: i32;
    static mut g_VRReloadFingerActive: [i32; 2];
    static mut g_VRReloadFingerRot: [[[f32; 4]; 32]; 2];
    static mut g_VRReloadFingerSet: [[i32; 32]; 2];
    static mut g_VRReloadFingerBlend: [f32; 2];
    static mut g_hasWeaponEquipped: bool;
    static mut CyberpunkVR_DualWieldUntil: u64;
    static mut CyberpunkVR_CarryLeft: i32;
    static mut CyberpunkVR_CarryGripBlend: f32;
    // the layers' own switches and probes (src/Anim/CharacterRigState.cpp)
    static mut CyberpunkVR_DebugRestFingerHave: i32;
    static mut CyberpunkVR_DebugRestFingerCaps: u64;
    static mut CyberpunkVR_DebugRestFingerRefused: i32;
    static mut CyberpunkVR_DebugRestFingerSaved: i32;
    static mut CyberpunkVR_DebugRestFingerLoadTried: i32;
    static mut CyberpunkVR_DebugRestFingerLoaded: i32;
    static mut CyberpunkVR_RestFingerCaptureReq: i32;
    static mut CyberpunkVR_RestFingerSaveReq: i32;
    static mut CyberpunkVR_DebugRestApplyCalls: u64;
    static mut CyberpunkVR_DebugRestBones: i32;
    static mut CyberpunkVR_DebugRestGate: i32;
    static mut CyberpunkVR_RestFingerForce: i32;
    static mut CyberpunkVR_RestFingerCaptureReqR: i32;
    static mut CyberpunkVR_RestFingerSaveReqR: i32;
    static mut CyberpunkVR_RestFingerApplyR: i32;
    static mut CyberpunkVR_RestFingerForceR: i32;
    static mut CyberpunkVR_DebugRestFingerHaveR: i32;
    static mut CyberpunkVR_DebugRestFingerSavedR: i32;
    static mut CyberpunkVR_DebugRestFingerRefusedR: i32;
    static mut CyberpunkVR_DebugRestFingerCapsR: i32;
    static mut CyberpunkVR_DebugRestBonesR: i32;
    static mut CyberpunkVR_DebugRestGateR: i32;
    static mut CyberpunkVR_DebugRestFingerLoadTriedR: i32;
    static mut CyberpunkVR_CarryGripApply: i32;
    static mut CyberpunkVR_DebugCarryGripHave: i32;
    static mut CyberpunkVR_DebugCarryGripSnaps: i32;
    static mut CyberpunkVR_DebugCarryGripBones: i32;
    fn cvr_cpp_vrik_grip_path(name: *const c_char, out: *mut c_char, size: usize) -> bool;
}

const REST_GRIP_INI: &str = "CyberpunkVR_RestGrip_Left.ini";
const REST_GRIP_INI_R: &str = "CyberpunkVR_RestGrip_Right.ini";

static mut g_restRotR: [[f32; 4]; 32] = [[0.0; 4]; 32];
static mut g_restCountR: i32 = 0;
static mut g_restHaveR: i32 = 0;
static mut g_gripRotR: [[f32; 4]; 32] = [[0.0; 4]; 32];
static mut g_gripCountR: i32 = 0;
static mut g_gripHaveR: i32 = 0;

#[inline(always)]
unsafe fn diagnostics() -> bool {
    CyberpunkVR_RuntimeDiagnostics != 0
}

/// The parent-local rotations of a hand's finger list as the buffer holds them; slots past the list (or out of range)
/// are the identity.
unsafe fn capture(boneBuf: *mut u8, idx: &[i32; 32], cnt: i32, out: &mut [[f32; 4]; 32]) {
    for k in 0..32 {
        let bi = if (k as i32) < cnt { idx[k] } else { -1 };
        if bi < 0 || bi >= MAXB {
            out[k] = [0.0, 0.0, 0.0, 1.0];
            continue;
        }
        out[k] = a4(br(boneBuf, bi));
    }
}

/// A recorded pose written flat onto the first `n` fingers; an all-zero slot was never recorded. Returns the count.
unsafe fn replay(boneBuf: *mut u8, idx: &[i32; 32], n: i32, rot: &[[f32; 4]; 32]) -> i32 {
    let mut wrote = 0;
    let mut k = 0;
    while k < n && k < 32 {
        let bi = idx[k as usize];
        k += 1;
        if bi < 0 || bi >= MAXB {
            continue;
        }
        let r = rot[(k - 1) as usize];
        if r[0] == 0.0 && r[1] == 0.0 && r[2] == 0.0 && r[3] == 0.0 {
            continue; // never recorded
        }
        put4(br(boneBuf, bi), &r);
        wrote += 1;
    }
    wrote
}

/// THE LEFT HAND AT REST: captured on request with empty hands, replayed while a weapon is out (cvr::anim).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_rest_finger_pose(boneBuf: *mut u8, force: bool) {
    let cnt = g_VRSmokeFingerCountL;
    let idx = &g_VRSmokeFingerIdxL;
    if cnt <= 0 {
        return;
    }
    // CAPTURE, on request; a refusal names itself (1 weapon, 2 reload grip, 4 cigarette, 8 vehicle).
    if CyberpunkVR_RestFingerCaptureReq != 0 {
        let mut why = 0;
        if g_hasWeaponEquipped {
            why |= 1;
        }
        if g_VRReloadFingerActive[0] != 0 {
            why |= 2;
        }
        if g_VRSmokeFingerActiveL != 0 {
            why |= 4;
        }
        if g_isInVehicle {
            why |= 8;
        }
        if why != 0 {
            CyberpunkVR_RestFingerCaptureReq = 0;
            CyberpunkVR_DebugRestFingerRefused = why;
            return;
        }
        capture(boneBuf, idx, cnt, &mut g_VRRestFingerRot);
        g_VRRestFingerCount = cnt;
        g_VRRestFingerHave = 1;
        CyberpunkVR_DebugRestFingerHave = 1;
        if diagnostics() {
            CyberpunkVR_DebugRestFingerCaps = CyberpunkVR_DebugRestFingerCaps.wrapping_add(1);
        }
        CyberpunkVR_DebugRestFingerRefused = 0;
        CyberpunkVR_RestFingerCaptureReq = 0;
        CyberpunkVR_RestFingerSaveReq = 1; // written to disk off this thread; see RestFingerTick
        return;
    }
    // THE GATE, PUBLISHED: 1 weapon seen, 2 pose recorded, 4 apply on, 8 finger list resolved, 16 dual wield.
    CyberpunkVR_DebugRestGate = (if g_hasWeaponEquipped { 1 } else { 0 })
        | (if g_VRRestFingerHave != 0 { 2 } else { 0 })
        | (if g_VRRestFingerApply != 0 { 4 } else { 0 })
        | (if cnt > 0 { 8 } else { 0 });
    if !g_hasWeaponEquipped && CyberpunkVR_RestFingerForce == 0 && !force {
        return;
    }
    if g_VRRestFingerHave == 0 || (g_VRRestFingerApply == 0 && !force) {
        return;
    }
    // LEFT-HANDED this is the hand holding the weapon.
    if super::weapon_arm::cvr_anim_weapon_in_left_hand() && !force {
        return;
    }
    // DUAL WIELDING: this hand holds the second gun.
    if GetTickCount64() < core::ptr::read_volatile(&raw const CyberpunkVR_DualWieldUntil) && !force {
        CyberpunkVR_DebugRestGate |= 16;
        return;
    }
    // ON THE STEERING WHEEL (main's fc750f1ed): this hand holds the rim in the driving animation's grip; the resting
    // fingers would open it off the wheel while the other hand holds the gun. 32 in the gate.
    if !force && g_isInVehicle && on_wheel(&super::shared_globals::g_wheelBlendLeft) {
        CyberpunkVR_DebugRestGate |= 32;
        return;
    }
    if diagnostics() {
        CyberpunkVR_DebugRestApplyCalls = CyberpunkVR_DebugRestApplyCalls.wrapping_add(1);
    }
    // The recorded count decides, not the live one.
    let n = if g_VRRestFingerCount > 0 && g_VRRestFingerCount < cnt { g_VRRestFingerCount } else { cnt };
    CyberpunkVR_DebugRestBones = replay(boneBuf, idx, n, &g_VRRestFingerRot);
}

/// The right hand's mirror: replayed while the weapon is in the LEFT hand (carried there, or held left-handed).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_rest_finger_pose_right(boneBuf: *mut u8, force: bool) {
    let cnt = g_VRSmokeFingerCount;
    let idx = &g_VRSmokeFingerIdx;
    if cnt <= 0 {
        return;
    }
    if CyberpunkVR_RestFingerCaptureReqR != 0 {
        let mut why = 0;
        if g_hasWeaponEquipped {
            why |= 1;
        }
        if g_VRSmokeFingerActive != 0 {
            why |= 4;
        }
        if why != 0 {
            CyberpunkVR_DebugRestFingerRefusedR = why;
            CyberpunkVR_RestFingerCaptureReqR = 0;
            return;
        }
        capture(boneBuf, idx, cnt, &mut g_restRotR);
        g_restCountR = cnt;
        g_restHaveR = 1;
        CyberpunkVR_DebugRestFingerHaveR = 1;
        if diagnostics() {
            CyberpunkVR_DebugRestFingerCapsR = CyberpunkVR_DebugRestFingerCapsR.wrapping_add(1);
        }
        CyberpunkVR_DebugRestFingerRefusedR = 0;
        CyberpunkVR_RestFingerCaptureReqR = 0;
        CyberpunkVR_RestFingerSaveReqR = 1;
        return;
    }
    // 1 carried in the left hand, 2 pose recorded, 4 apply on, 8 finger list resolved
    CyberpunkVR_DebugRestGateR = (if CyberpunkVR_CarryLeft != 0 { 1 } else { 0 })
        | (if g_restHaveR != 0 { 2 } else { 0 })
        | (if CyberpunkVR_RestFingerApplyR != 0 { 4 } else { 0 })
        | (if cnt > 0 { 8 } else { 0 });
    if CyberpunkVR_CarryLeft == 0 && !super::weapon_arm::cvr_anim_weapon_in_left_hand() && CyberpunkVR_RestFingerForceR == 0 && !force {
        return;
    }
    if g_restHaveR == 0 || (CyberpunkVR_RestFingerApplyR == 0 && !force) {
        return;
    }
    // On the steering wheel (left-handed: the gun in the left hand, this one drives): the animation's grip stays.
    if !force && g_isInVehicle && on_wheel(&super::shared_globals::g_wheelBlendRight) {
        return;
    }
    let n = if g_restCountR > 0 && g_restCountR < cnt { g_restCountR } else { cnt };
    CyberpunkVR_DebugRestBonesR = replay(boneBuf, idx, n, &g_restRotR);
}

/// A hand at least half blended onto the steering wheel (wheel_grab.rs's blend, f32 bits).
fn on_wheel(blend: &core::sync::atomic::AtomicU32) -> bool {
    f32::from_bits(blend.load(core::sync::atomic::Ordering::Relaxed)) >= 0.5
}

/// The nlerp onto the nearer hemisphere every finger preview uses: `b` of the target over what is in the buffer.
#[inline(always)]
fn nlerp_onto(q: &mut Q, t: &Q, b: f32) -> bool {
    let dot = q[0] * t[0] + q[1] * t[1] + q[2] * t[2] + q[3] * t[3];
    let s = if dot < 0.0 { -b } else { b };
    let n = [q[0] * (1.0 - b) + t[0] * s, q[1] * (1.0 - b) + t[1] * s, q[2] * (1.0 - b) + t[2] * s, q[3] * (1.0 - b) + t[3] * s];
    let nl = sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2] + n[3] * n[3]);
    if nl > 1e-6 {
        *q = [n[0] / nl, n[1] / nl, n[2] / nl, n[3] / nl];
        return true;
    }
    false
}

/// RELOAD FINGER POSE: the free hand's fingers into a grip while it holds a weapon part; 0..1 preview blend.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_finger_pose(boneBuf: *mut u8) {
    for hnd in 0..2 {
        if g_VRReloadFingerActive[hnd] == 0 {
            continue;
        }
        if super::two_hand::two_hand_owns_reload_finger(hnd) {
            continue;
        }
        let cnt = if hnd == 0 { g_VRSmokeFingerCountL } else { g_VRSmokeFingerCount };
        let idx = if hnd == 0 { &g_VRSmokeFingerIdxL } else { &g_VRSmokeFingerIdx };
        let b = g_VRReloadFingerBlend[hnd];
        let mut k = 0;
        while k < cnt && k < 32 {
            let ku = k as usize;
            k += 1;
            if g_VRReloadFingerSet[hnd][ku] == 0 {
                continue;
            }
            let bi = idx[ku];
            if bi < 0 || bi >= MAXB {
                continue;
            }
            let qp = br(boneBuf, bi);
            let t = g_VRReloadFingerRot[hnd][ku];
            if b >= 0.999 {
                put4(qp, &t);
            } else if b > 0.001 {
                let mut q = a4(qp);
                if nlerp_onto(&mut q, &t, b) {
                    put4(qp, &q);
                }
            }
        }
    }
}

/// THE RIGHT HAND'S GRIP, remembered as it is worn and offered back (faded by CyberpunkVR_CarryGripBlend) while the
/// left hand carries the weapon.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_carry_grip_pose_right(boneBuf: *mut u8) {
    let cnt = g_VRSmokeFingerCount;
    let idx = &g_VRSmokeFingerIdx;
    if cnt <= 0 {
        return;
    }
    // LEARN IT, while the hand is really holding the weapon.
    if g_hasWeaponEquipped
        && CyberpunkVR_CarryLeft == 0
        && !super::weapon_arm::cvr_anim_weapon_in_left_hand()
        && g_VRSmokeFingerActive == 0
        && g_VRReloadFingerActive[1] == 0
    {
        let mut k = 0;
        while k < cnt && k < 32 {
            let bi = idx[k as usize];
            if bi >= 0 && bi < MAXB {
                g_gripRotR[k as usize] = a4(br(boneBuf, bi));
            }
            k += 1;
        }
        g_gripCountR = cnt;
        g_gripHaveR = 1;
        CyberpunkVR_DebugCarryGripHave = 1;
        if diagnostics() {
            CyberpunkVR_DebugCarryGripSnaps = CyberpunkVR_DebugCarryGripSnaps.wrapping_add(1);
        }
        return;
    }
    if g_gripHaveR == 0 || CyberpunkVR_CarryGripApply == 0 {
        return;
    }
    let mut b = CyberpunkVR_CarryGripBlend;
    if b <= 0.001 {
        return;
    }
    if b > 1.0 {
        b = 1.0;
    }
    let n = if g_gripCountR > 0 && g_gripCountR < cnt { g_gripCountR } else { cnt };
    let mut wrote = 0;
    let mut k = 0;
    while k < n && k < 32 {
        let ku = k as usize;
        k += 1;
        let bi = idx[ku];
        if bi < 0 || bi >= MAXB {
            continue;
        }
        let tq = g_gripRotR[ku];
        if tq[0] == 0.0 && tq[1] == 0.0 && tq[2] == 0.0 && tq[3] == 0.0 {
            continue;
        }
        let qp = br(boneBuf, bi);
        if b >= 0.999 {
            put4(qp, &tq);
            wrote += 1;
            continue;
        }
        let mut q = a4(qp);
        if nlerp_onto(&mut q, &tq, b) {
            put4(qp, &q);
            wrote += 1;
        }
    }
    CyberpunkVR_DebugCarryGripBones = wrote;
}

// ---- the files, off the animation thread

pub(super) unsafe fn grip_path(name: &str) -> Option<String> {
    let mut c = Vec::with_capacity(name.len() + 1);
    c.extend_from_slice(name.as_bytes());
    c.push(0);
    let mut out = [0 as c_char; 520];
    if !cvr_cpp_vrik_grip_path(c.as_ptr() as *const c_char, out.as_mut_ptr(), out.len()) || out[0] == 0 {
        return None;
    }
    Some(CStr::from_ptr(out.as_ptr()).to_string_lossy().into_owned())
}

unsafe fn finger_name(names: &[[c_char; 48]; 32], k: usize) -> &[u8] {
    CStr::from_ptr(names[k].as_ptr()).to_bytes()
}

unsafe fn save(path: Option<String>, header: &str, names: &[[c_char; 48]; 32], rot: &[[f32; 4]; 32], count: i32) -> bool {
    let Some(path) = path else { return false };
    let mut text = String::from(header);
    let mut k = 0;
    while k < count && k < 32 {
        let nm = finger_name(names, k as usize);
        if !nm.is_empty() {
            let r = rot[k as usize];
            text.push_str(&format!(
                "F {} {} {} {} {}\n",
                String::from_utf8_lossy(nm),
                format_g9(r[0]),
                format_g9(r[1]),
                format_g9(r[2]),
                format_g9(r[3])
            ));
        }
        k += 1;
    }
    // fopen(path, "w"): a text-mode stream
    std::fs::write(path, text.replace('\n', "\r\n")).is_ok()
}

/// `F <bone> qx qy qz qw` lines matched by name against the hand's finger list; returns the matches.
unsafe fn load(path: Option<String>, names: &[[c_char; 48]; 32], count: i32, rot: &mut [[f32; 4]; 32]) -> Option<i32> {
    let text = std::fs::read(path?).ok()?;
    let mut hit = 0;
    for raw in text.split(|&b| b == b'\n') {
        let line = raw;
        if line.is_empty() || line[0] == b'#' || line[0] == b'\r' {
            continue;
        }
        let s = String::from_utf8_lossy(line);
        let mut it = s.split_ascii_whitespace();
        if it.next() != Some("F") {
            continue;
        }
        let Some(nm) = it.next() else { continue };
        let nm = &nm.as_bytes()[..nm.len().min(63)];
        let mut v = [0.0f32; 4];
        let mut ok = true;
        for slot in v.iter_mut() {
            match it.next().and_then(|t| t.parse::<f32>().ok()) {
                Some(x) => *slot = x,
                None => ok = false,
            }
        }
        if !ok {
            continue;
        }
        let mut k = 0;
        while k < count && k < 32 {
            if finger_name(names, k as usize) == nm {
                rot[k as usize] = v;
                hit += 1;
                break;
            }
            k += 1;
        }
    }
    Some(hit)
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_rest_finger_tick() {
    if CyberpunkVR_RestFingerSaveReq != 0 {
        CyberpunkVR_RestFingerSaveReq = 0;
        let header = "# CyberpunkVR rest grip pose v1 (auto-generated by VRRestFingerCapture)\n\
                      # The LEFT hand's resting fingers, one frame of the game's own empty-handed\n\
                      # animation. Replayed while a weapon is out, where the game would otherwise\n\
                      # pose this hand as the support half of a two-handed grip.\n\
                      # F <bone> qx qy qz qw           finger: parent-local rotation only\n";
        CyberpunkVR_DebugRestFingerSaved =
            if save(grip_path(REST_GRIP_INI), header, &g_VRSmokeFingerNameL, &g_VRRestFingerRot, g_VRRestFingerCount) { 1 } else { -1 };
        return;
    }
    // LOAD once the resolver has names to match against.
    if g_VRRestFingerHave != 0 || CyberpunkVR_DebugRestFingerLoadTried != 0 {
        return;
    }
    if g_VRSmokeFingerCountL <= 0 {
        return;
    }
    CyberpunkVR_DebugRestFingerLoadTried = 1;
    if let Some(hit) = load(grip_path(REST_GRIP_INI), &g_VRSmokeFingerNameL, g_VRSmokeFingerCountL, &mut g_VRRestFingerRot) {
        if hit > 0 {
            g_VRRestFingerCount = g_VRSmokeFingerCountL;
            g_VRRestFingerHave = 1;
            CyberpunkVR_DebugRestFingerHave = 1;
            CyberpunkVR_DebugRestFingerLoaded = hit;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_rest_finger_right_tick() {
    if CyberpunkVR_RestFingerSaveReqR != 0 {
        CyberpunkVR_RestFingerSaveReqR = 0;
        let header = "# CyberpunkVR rest grip pose v1, RIGHT hand (VRRestFingerCaptureRight)\n\
                      # One frame of the game's own empty-handed animation. Replayed while the\n\
                      # weapon is carried in the LEFT hand, where this one would otherwise be\n\
                      # posed around a grip it is no longer holding.\n\
                      # F <bone> qx qy qz qw           finger: parent-local rotation only\n";
        CyberpunkVR_DebugRestFingerSavedR =
            if save(grip_path(REST_GRIP_INI_R), header, &g_VRSmokeFingerName, &g_restRotR, g_restCountR) { 1 } else { -1 };
        return;
    }
    if g_restHaveR != 0 || CyberpunkVR_DebugRestFingerLoadTriedR != 0 {
        return;
    }
    if g_VRSmokeFingerCount <= 0 {
        return;
    }
    CyberpunkVR_DebugRestFingerLoadTriedR = 1;
    if let Some(hit) = load(grip_path(REST_GRIP_INI_R), &g_VRSmokeFingerName, g_VRSmokeFingerCount, &mut g_restRotR) {
        if hit > 0 {
            g_restCountR = g_VRSmokeFingerCount;
            g_restHaveR = 1;
            CyberpunkVR_DebugRestFingerHaveR = 1;
        }
    }
}
