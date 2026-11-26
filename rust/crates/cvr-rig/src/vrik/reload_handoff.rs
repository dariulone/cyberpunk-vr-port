//! The native reload, played by both arms (formerly src/Anim/ReloadHandOff.cpp): the arms keep being solved and
//! only their target moves, from the controller to the animated hand re-placed on the solved chest and back, eased;
//! the hands start back before the reload can end.
use super::env::*;
use super::*;
use crate::math::{quat_conj, quat_mul, quat_norm, quat_rotate_vec};
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use cvr_body::cmath::clamp;

extern "C" {
    static mut g_hasWeaponEquipped: bool;
    static mut CyberpunkVR_VrikReloadHandOff: i32;
    static mut CyberpunkVR_ReloadHandOffInSec: f32;
    static mut CyberpunkVR_ReloadHandOffOutSec: f32;
    static mut CyberpunkVR_ReloadHandOffLeadSec: f32;
    static mut CyberpunkVR_ReloadHandOffStats: [f32; 8];
    /// The hands' layers' QPC (ticks at 1 MHz) for tests; null = QueryPerformanceCounter.
    static g_VRIKClockQpc: Option<unsafe extern "C" fn() -> i64>;
}

/// VRIK_ClockQpc: the test hook, or QueryPerformanceCounter.
pub unsafe fn clock_qpc() -> i64 {
    match g_VRIKClockQpc {
        Some(clock) => clock(),
        None => {
            let mut c = 0i64;
            QueryPerformanceCounter(&mut c);
            c
        }
    }
}
/// VRIK_ClockQpcFrequency: 1 MHz under the test hook, else QueryPerformanceFrequency.
pub unsafe fn clock_qpc_frequency() -> f64 {
    if g_VRIKClockQpc.is_some() {
        return 1e6;
    }
    let mut f = 0i64;
    QueryPerformanceFrequency(&mut f);
    f as f64
}
unsafe fn qpc_seconds(ticks: i64) -> f64 {
    let freq = clock_qpc_frequency();
    if freq > 0.0 {
        ticks as f64 / freq
    } else {
        0.0
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static FINISH_IN: AtomicU32 = AtomicU32::new(0xBF80_0000); // -1.0f
static ENTER_QPC: AtomicI64 = AtomicI64::new(0);
static LAST_QPC: AtomicI64 = AtomicI64::new(0);

#[derive(Clone, Copy)]
struct AnimHand {
    chest: i32,
    pos: V3,
    rot: Q,
    valid: bool,
}
const HAND0: AnimHand = AnimHand { chest: -1, pos: [0.0; 3], rot: ID, valid: false };
/// 0 right, 1 left -- the wheel's numbering.
static mut HAND: [AnimHand; 2] = [HAND0; 2];
/// The linear ramp; the weight is its ease.
static mut BLEND: f32 = 0.0;
static mut WEIGHT: f32 = 0.0;
/// The ramp's slope at the last update, per second.
static mut RATE: f32 = 0.0;

fn ease(b: f32) -> f32 {
    b * b * (3.0 - 2.0 * b)
}

/// The bone both clavicles hang from: the upper arm's parent is the clavicle, its parent the chest.
unsafe fn chest_of(upper_arm: i32) -> i32 {
    if !(0..MAXB).contains(&upper_arm) {
        return -1;
    }
    let clavicle = g_VRBoneParent[upper_arm as usize] as i32;
    if !(0..MAXB).contains(&clavicle) {
        return -1;
    }
    let chest = g_VRBoneParent[clavicle as usize] as i32;
    if (0..MAXB).contains(&chest) {
        chest
    } else {
        -1
    }
}

unsafe fn capture(h: &mut AnimHand, hand_idx: i32, upper_arm: i32) {
    h.valid = false;
    h.chest = chest_of(upper_arm);
    if h.chest < 0 || !(0..MAXB).contains(&hand_idx) {
        return;
    }
    let (cu, hu) = (h.chest as usize, hand_idx as usize);
    let inv = quat_conj(&g_fkRot[cu]);
    let d = sub3(&g_fkPos[hu], &g_fkPos[cu]);
    h.pos = quat_rotate_vec(&inv, &d);
    h.rot = quat_mul(&inv, &g_fkRot[hu]);
    quat_norm(&mut h.rot);
    h.valid = (h.pos[0] + h.pos[1] + h.pos[2] + h.rot[0] + h.rot[1] + h.rot[2] + h.rot[3]).is_finite();
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_hand_off_set(active: bool, finishInSec: f32) {
    if active {
        let f = if finishInSec.is_finite() { finishInSec } else { -1.0 };
        FINISH_IN.store(f.to_bits(), Ordering::Relaxed);
        ENTER_QPC.store(clock_qpc(), Ordering::Relaxed);
        CyberpunkVR_ReloadHandOffStats[7] += 1.0;
    }
    ACTIVE.store(active, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_hand_off_capture() {
    capture(&mut HAND[0], g_VRRightBoneIdx, g_VRRightUpperArmIdx);
    capture(&mut HAND[1], g_VRLeftBoneIdx, g_VRLeftUpperArmIdx);
    if HAND[0].valid {
        CyberpunkVR_ReloadHandOffStats[4..7].copy_from_slice(&HAND[0].pos);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_hand_off_update() {
    let now = clock_qpc();
    let last = LAST_QPC.load(Ordering::Relaxed);
    let mut dt = if last != 0 { qpc_seconds(now - last) as f32 } else { 0.016 };
    dt = clamp(dt, 0.0, 0.1);
    let active = ACTIVE.load(Ordering::Acquire);
    let finish_in = f32::from_bits(FINISH_IN.load(Ordering::Relaxed));
    let elapsed = if active { qpc_seconds(now - ENTER_QPC.load(Ordering::Relaxed)) as f32 } else { 0.0 };
    // A reload whose exit never reaches us must not keep the arms.
    let overdue = if finish_in > 0.0 { elapsed > finish_in + 1.0 } else { elapsed > 10.0 };
    let wanted = active
        && !overdue
        && CyberpunkVR_VrikReloadHandOff != 0
        && g_hasWeaponEquipped
        && !(finish_in > 0.0 && elapsed >= finish_in - CyberpunkVR_ReloadHandOffLeadSec);
    let ramp = if wanted { CyberpunkVR_ReloadHandOffInSec } else { CyberpunkVR_ReloadHandOffOutSec };
    let step = if ramp > 0.001 { dt / ramp } else { 1.0 };
    BLEND = clamp(BLEND + if wanted { step } else { -step }, 0.0, 1.0);
    WEIGHT = ease(BLEND);
    let moving = if wanted { BLEND < 1.0 } else { BLEND > 0.0 };
    RATE = if moving && ramp > 0.001 { (if wanted { 1.0 } else { -1.0 }) / ramp } else { 0.0 };
    LAST_QPC.store(now, Ordering::Relaxed);
    CyberpunkVR_ReloadHandOffStats[0] = WEIGHT;
    CyberpunkVR_ReloadHandOffStats[1] = if wanted { 1.0 } else { 0.0 };
    CyberpunkVR_ReloadHandOffStats[2] = elapsed;
    CyberpunkVR_ReloadHandOffStats[3] = if active { finish_in } else { -1.0 };
}

/// The ramp extrapolated to the moment of the call, so every pass of a tick gets its own weight.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_hand_off_weight() -> f32 {
    if RATE == 0.0 {
        return WEIGHT;
    }
    let last = LAST_QPC.load(Ordering::Relaxed);
    let ahead = if last != 0 { clamp(qpc_seconds(clock_qpc() - last) as f32, 0.0, 0.1) } else { 0.0 };
    ease(clamp(BLEND + RATE * ahead, 0.0, 1.0))
}

/// The IK target moved toward the animated hand, re-placed on the solved chest. hand 0 right, 1 left.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_reload_hand_off_blend_target(hand: i32, target: *mut f32, handRot: *mut f32) {
    if !(0..=1).contains(&hand) || target.is_null() || handRot.is_null() || WEIGHT <= 0.0 {
        return;
    }
    let h = HAND[hand as usize];
    if !h.valid || h.chest < 0 {
        return;
    }
    let cu = h.chest as usize;
    let mut anim_pos = quat_rotate_vec(&g_fkRot[cu], &h.pos);
    for k in 0..3 {
        anim_pos[k] += g_fkPos[cu][k];
    }
    let mut anim_rot = quat_mul(&g_fkRot[cu], &h.rot);
    quat_norm(&mut anim_rot);
    let w = WEIGHT;
    for k in 0..3 {
        *target.add(k) += (anim_pos[k] - *target.add(k)) * w;
    }
    let r = a4(handRot);
    if r[0] * anim_rot[0] + r[1] * anim_rot[1] + r[2] * anim_rot[2] + r[3] * anim_rot[3] < 0.0 {
        for c in anim_rot.iter_mut() {
            *c = -*c;
        }
    }
    let mut out = r;
    for k in 0..4 {
        out[k] += (anim_rot[k] - out[k]) * w;
    }
    quat_norm(&mut out);
    put4(handRot, &out);
}
