//! The fingers from the controllers and the gloves (formerly src/Anim/HandGestures.cpp, namespace cvr::gestures):
//! each digit from its current local toward straight, then toward the native fist, from grip, trigger and the right
//! thumb; the baked per-hand corrections; finger tracking with spread. Exported under cvr_gestures_*; the C++
//! namespace forwards (src/Anim/CharacterRigRust.cpp).
use super::cyberware::{cvr_cyberware_selected_kind, cvr_cyberware_wire_deployment};
use super::env::*;
use super::reload::{cvr_anim_rest_finger_pose, cvr_anim_rest_finger_pose_right};
use super::rig::{cvr_vrik_spread_axis, cvr_vrik_thumb_reference, VRIK_ReferenceFingerRotation};
use super::*;
use crate::data::{CALIBRATED_GESTURES, NATIVE_FIST};
use crate::gestures::{calibration_targets, evaluate, from_tracking, CalibrationWeights, Targets};
use crate::math::{quat_conj, quat_mul, quat_norm};
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use cvr_body::cmath::{clamp, cos, exp, min, sin};

extern "C" {
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerName: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    static mut g_VRReloadFingerActive: [i32; 2];
    static mut CyberpunkVR_CarryLeft: i32;
    static g_VrikFrameEpoch: AtomicU32;
    // the gestures' own switches and probes (src/Anim/CharacterRigState.cpp)
    static mut CyberpunkVR_GestureMode: [i32; 2];
    static mut CyberpunkVR_GestureClosed: [[f32; 5]; 2];
    static mut CyberpunkVR_GestureStraight: [[f32; 5]; 2];
    static mut CyberpunkVR_FingerBones: i32;
    static mut CyberpunkVR_FingerSpread: f32;
    static mut CyberpunkVR_FingerSpreadOn: i32;
    /// The clock the hands' layers (gestures, cyberware) run by, in milliseconds; tests set it, null = GetTickCount64.
    static g_VRIKClockMs: Option<unsafe extern "C" fn() -> u64>;
    // Runtimes/FingerGrip.cpp, through src/Anim/CharacterRigRust.cpp
}

/// The hands' layers' clock: the test hook, or GetTickCount64.
pub unsafe fn now_ms() -> u64 {
    match g_VRIKClockMs {
        Some(clock) => clock(),
        None => GetTickCount64(),
    }
}

static INPUT: AtomicU64 = AtomicU64::new(0);
static EQUIPMENT: AtomicU64 = AtomicU64::new(0);

// ---- THE FINGERS AGAINST WHAT THE HAND MEETS (main's HandGestures.cpp, 2026-11): how far each finger may close, from
// the script's rays, and how far a surface presses its first joint back. A float in an atomic by its bits.
struct AtomicF32(AtomicU32);
impl AtomicF32 {
    const fn zero() -> Self {
        AtomicF32(AtomicU32::new(0))
    }
    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}
/// How much each finger is held back from closing, so that zero (the start, and a hand long unreported) means free.
static CONTACT_HELD: [[AtomicF32; 5]; 2] = [const { [const { AtomicF32::zero() }; 5] }; 2];
static CONTACT_STAMP: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static CONTACT_ASKED: [[AtomicU64; 5]; 2] = [const { [const { AtomicU64::new(0) }; 5] }; 2];
static CONTACT_FORCE: [[AtomicBool; 5]; 2] = [const { [const { AtomicBool::new(false) }; 5] }; 2];
static CONTACT_BACK: [[AtomicF32; 5]; 2] = [const { [const { AtomicF32::zero() }; 5] }; 2];
static CONTACT_BACK_AT: [[AtomicU64; 5]; 2] = [const { [const { AtomicU64::new(0) }; 5] }; 2];
static GESTURE_WANTED: [[AtomicF32; 5]; 2] = [const { [const { AtomicF32::zero() }; 5] }; 2];
static mut CONTACT_WAITED: [[f32; 5]; 2] = [[0.0; 5]; 2];
const CONTACT_FRESH_MS: u64 = 400;
/// Where a finger's limit goes from what the rays last said, which no longer depends on the finger. Down quickly but not
/// in one frame, up only once the surface has stayed gone for a moment, and not at all for a change too small to be
/// anything but the ray (main's HandGestures.hpp StepFingerLimit).
pub fn step_finger_limit(limit: f32, wanted: f32, dt: f32, waited: &mut f32) -> f32 {
    const DEAD: f32 = 0.02;
    const FALL: f32 = 15.0;
    const RISE: f32 = 2.5;
    const WAIT: f32 = 0.12;
    if wanted < limit - DEAD {
        *waited = 0.0;
        return wanted.max(limit - FALL * dt);
    }
    if wanted > limit + DEAD {
        *waited += dt;
        return if *waited < WAIT { limit } else { wanted.min(limit + RISE * dt) };
    }
    *waited = 0.0;
    limit
}
fn tick() -> u64 {
    // SAFETY: a plain OS call.
    unsafe { GetTickCount64() }
}
/// FingerContact: how far a finger may close, 0 open .. 1 fist, worked out from the hand alone. With `hold` it closes
/// exactly that far: the fingers around a thing in the hand. Side 0 left, 1 right; digit 0 thumb .. 4 little finger. A
/// hand no report has come for lately is free again.
pub fn finger_contact(side: i32, digit: i32, limit: f32, hold: bool) {
    if !(0..=1).contains(&side) || !(0..=4).contains(&digit) || !limit.is_finite() {
        return;
    }
    let (s, d) = (side as usize, digit as usize);
    let limit = clamp(limit, 0.0, 1.0);
    let now = tick();
    let asked = CONTACT_ASKED[s][d].swap(now, Ordering::Relaxed);
    let was_held = CONTACT_FORCE[s][d].swap(hold, Ordering::Relaxed);
    // long unasked, the finger has been free; and letting go of a thing opens the hand at its own pace, not the surface's
    let free = asked == 0 || now - asked > CONTACT_FRESH_MS || (was_held && !hold);
    let was = if free { 1.0 } else { 1.0 - CONTACT_HELD[s][d].load() };
    // SAFETY: the script thread's own state.
    let next = if hold { limit } else { step_finger_limit(was, limit, min((now.wrapping_sub(asked)) as f32 * 0.001, 0.1), unsafe { &mut CONTACT_WAITED[s][d] }) };
    CONTACT_HELD[s][d].store(1.0 - next);
    CONTACT_STAMP[s].store(now, Ordering::Relaxed);
}
thread_local! {
    static BEND_LAST: core::cell::Cell<[[Option<std::time::Instant>; 5]; 2]> = const { core::cell::Cell::new([[None; 5]; 2]) };
}
/// FingerBend: how far a finger is pressed back past straight, radians at its first joint, by a surface it runs into
/// straight on. A real clock: the tick count moves in 16 ms steps, and the calls come 2 and 21 ms apart. It eases in and
/// out, and the rate is capped, so a wall found late does not snap a finger back.
pub fn finger_bend(side: i32, digit: i32, angle: f32) {
    if !(0..=1).contains(&side) || !(1..=4).contains(&digit) || !angle.is_finite() {
        return;
    }
    const RISE_TAU: f32 = 0.07;
    const FALL_TAU: f32 = 0.18;
    const MOST_RATE: f32 = 7.0;
    const MOST: f32 = 1.5;
    let (s, d) = (side as usize, digit as usize);
    let clock = std::time::Instant::now();
    let wanted = clamp(angle, 0.0, MOST);
    let now = tick();
    let asked = CONTACT_BACK_AT[s][d].swap(now, Ordering::Relaxed);
    let fresh = asked != 0 && now - asked <= CONTACT_FRESH_MS;
    let was = if fresh { CONTACT_BACK[s][d].load() } else { 0.0 };
    let last = BEND_LAST.with(|c| {
        let mut all = c.get();
        let last = all[s][d];
        all[s][d] = Some(clock);
        c.set(all);
        last
    });
    let dt = if fresh { min(last.map_or(0.0, |l| clock.duration_since(l).as_secs_f32()), 0.1) } else { 0.0 };
    let pull = (wanted - was) * (1.0 - exp(-dt / if wanted > was { RISE_TAU } else { FALL_TAU }));
    let next = was + clamp(pull, -MOST_RATE * dt, MOST_RATE * dt);
    CONTACT_BACK[s][d].store(next);
}
/// FingerClosed: the finger as drawn.
pub fn finger_closed(side: i32, digit: i32) -> f32 {
    if !(0..=1).contains(&side) || !(0..=4).contains(&digit) {
        return 0.0;
    }
    // SAFETY: an export read whole.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_GestureClosed[side as usize][digit as usize]) }
}
/// FingerWanted: the finger as it would be with nothing in its way.
pub fn finger_wanted(side: i32, digit: i32) -> f32 {
    if !(0..=1).contains(&side) || !(0..=4).contains(&digit) {
        return 0.0;
    }
    GESTURE_WANTED[side as usize][digit as usize].load()
}
/// PRESSED BACK, at the first joint and against the way this joint closes. Its closing is a turn on the parent's side,
/// so this is too.
fn press_back(side: usize, group: usize, suffix: &[u8], delta: &Q, now: u64, q: &mut Q) {
    if group == 0 || !suffix.starts_with(b"Hand") || suffix.last() != Some(&b'1') {
        return;
    }
    let back_at = CONTACT_BACK_AT[side][group].load(Ordering::Relaxed);
    let back = if back_at != 0 && now.wrapping_sub(back_at) <= CONTACT_FRESH_MS { CONTACT_BACK[side][group].load() } else { 0.0 };
    let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if back <= 0.001 || length <= 1e-4 {
        return;
    }
    // the short way round is the way it closes
    let sign = if delta[3] < 0.0 { 1.0 } else { -1.0 };
    let half = back * 0.5;
    let sine = sin(half) * sign / length;
    let turn = [delta[0] * sine, delta[1] * sine, delta[2] * sine, cos(half)];
    let mut result = quat_mul(&turn, q);
    quat_norm(&mut result);
    *q = result;
}

#[derive(Clone, Copy)]
struct State {
    pose: Targets,
    calibration: CalibrationWeights,
    epoch: u32,
    stamp: u64,
}
const STATE0: State = State { pose: Targets { closed: [0.0; 5], straight: [0.0; 5] }, calibration: [0.0; 4], epoch: !0, stamp: 0 };
static mut STATE: [State; 2] = [STATE0; 2];

#[derive(Clone, Copy)]
struct GloveState {
    pose: Targets,
    splay: [f32; 4],
    splay_valid: bool,
    epoch: u32,
    stamp: u64,
}
const GLOVE0: GloveState =
    GloveState { pose: Targets { closed: [0.0; 5], straight: [0.0; 5] }, splay: [0.5; 4], splay_valid: false, epoch: !0, stamp: 0 };
static mut GLOVES: [GloveState; 2] = [GLOVE0; 2];

fn byte(v: f32) -> u64 {
    (clamp(if v.is_finite() { v } else { 0.0 }, 0.0, 1.0) * 255.0 + 0.5) as u64
}

fn blend(a: &Q, b: &Q, t: f32) -> Q {
    let sign = if (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]) < 0.0 { -1.0f32 } else { 1.0 };
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        out[k] = a[k] * (1.0 - t) + b[k] * t * sign;
    }
    quat_norm(&mut out);
    out
}

/// The baked corrections of this finger, weighted by the gesture blend, applied on the rotation's right.
pub fn apply_calibration(side: i32, suffix: &[u8], weights: &CalibrationWeights, rotation: &mut Q) {
    if !(0..=1).contains(&side) {
        return;
    }
    let mut delta = [0.0f32, 0.0, 0.0, 1.0];
    let mut changed = false;
    for entry in CALIBRATED_GESTURES.iter() {
        let weight = weights[entry.pose];
        if weight <= 0.0 || entry.hands & (1u32 << side) == 0 || entry.suffix.as_bytes() != suffix {
            continue;
        }
        for i in 0..4 {
            delta[i] += weight * (entry.delta[i] - if i == 3 { 1.0 } else { 0.0 });
        }
        changed = true;
    }
    if !changed {
        return;
    }
    quat_norm(&mut delta);
    let mut result = quat_mul(rotation, &delta);
    quat_norm(&mut result);
    *rotation = result;
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_apply_calibration(side: i32, suffix: *const c_char, weights: *const f32, rotation: *mut f32) {
    if suffix.is_null() || rotation.is_null() || weights.is_null() {
        return;
    }
    let w = [*weights, *weights.add(1), *weights.add(2), *weights.add(3)];
    let mut r = a4(rotation);
    apply_calibration(side, CStr::from_ptr(suffix).to_bytes(), &w, &mut r);
    put4(rotation, &r);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_publish_input(lg: f32, rg: f32, lt: f32, rt: f32, thumb: bool, enabled: bool) {
    let now = now_ms() & 0x3fff_ffff;
    INPUT.store(
        byte(lg) | (byte(rg) << 8) | (byte(lt) << 16) | (byte(rt) << 24) | ((thumb as u64) << 32) | ((enabled as u64) << 33) | (now << 34),
        Ordering::Release,
    );
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_publish_equipment(mask: i32) {
    EQUIPMENT.store((now_ms() << 2) | ((mask & 3) as u64), Ordering::Release);
}

/// One hand's digits: from the bone's current local toward straight, then toward the native fist, per digit group.
/// `press_back_at`: the time to press a finger's first joint back by what a surface asks (finger_bend), 0 for none.
unsafe fn pose_digits(buffer: *mut u8, side: i32, pose: &Targets, calibration: Option<&CalibrationWeights>, posed: &mut [i32; 5], thumb_rig_open: bool, press_back_at: u64) {
    let count = if side == 0 { g_VRSmokeFingerCountL } else { g_VRSmokeFingerCount };
    let indices = if side == 0 { &g_VRSmokeFingerIdxL } else { &g_VRSmokeFingerIdx };
    let names = if side == 0 { &g_VRSmokeFingerNameL } else { &g_VRSmokeFingerName };
    let prefix = if side == 0 { 4 } else { 5 };
    let s = side as usize;
    for k in 0..count.min(32).max(0) as usize {
        let bone = indices[k];
        let mut reference = [0.0f32; 4];
        let name = CStr::from_ptr(names[k].as_ptr()).to_bytes();
        if !VRIK_ReferenceFingerRotation(bone, reference.as_mut_ptr()) || name.len() <= prefix {
            continue;
        }
        let suffix = &name[prefix..];
        for fist in NATIVE_FIST.iter() {
            if suffix == fist.suffix.as_bytes() {
                let q = br(buffer, bone);
                let rest = a4(q);
                let inverse = quat_conj(&fist.reference[s]);
                let delta = quat_mul(&fist.closed[s], &inverse);
                let mut closed = quat_mul(&delta, &reference);
                quat_norm(&mut closed);
                let mut straight_goal = reference;
                if !thumb_rig_open && fist.suffix == "InHandThumb" {
                    cvr_vrik_thumb_reference(side, straight_goal.as_mut_ptr());
                }
                let straight = blend(&rest, &straight_goal, pose.straight[fist.group]);
                let mut out = blend(&straight, &closed, pose.closed[fist.group]);
                if press_back_at != 0 {
                    press_back(s, fist.group, fist.suffix.as_bytes(), &delta, press_back_at, &mut out);
                }
                put4(q, &out);
                posed[fist.group] += 1;
                break;
            }
        }
        if let Some(c) = calibration {
            let q = br(buffer, bone);
            let mut r = a4(q);
            apply_calibration(side, suffix, c, &mut r);
            put4(q, &r);
        }
    }
}

/// FINGER SPREAD: the middle finger stays, the others turn sideways in the palm plane (relative to the rig's own pose
/// at a neutral 0.5), degrees per unit of splay times xr_finger_spread, clamped.
const SPLAY_NEUTRAL: f32 = 0.5;
const SPLAY_DEG: [f32; 5] = [19.6, 14.7, 0.0, 11.76, 11.76];
unsafe fn spread_digits(buffer: *mut u8, side: i32, splay: &[f32; 4], strength: f32) {
    let mut degrees = [0.0f32; 5];
    degrees[0] = (splay[0] - SPLAY_NEUTRAL) * SPLAY_DEG[0];
    degrees[1] = (splay[1] - SPLAY_NEUTRAL) * SPLAY_DEG[1];
    degrees[3] = (splay[2] - SPLAY_NEUTRAL) * SPLAY_DEG[3];
    degrees[4] = degrees[3] + (splay[3] - SPLAY_NEUTRAL) * SPLAY_DEG[4];
    for digit in [0usize, 1, 3, 4] {
        let limit = if digit == 0 { 40.0f32 } else { 30.0 };
        let angle = clamp(degrees[digit] * strength, -limit, limit) * 0.01745329252;
        let mut bone = -1i32;
        let mut axis = [0.0f32; 3];
        if angle.abs() < 1e-4 || !cvr_vrik_spread_axis(side, digit as i32, &mut bone, axis.as_mut_ptr()) {
            continue;
        }
        let h = sin(angle * 0.5);
        let turn = [axis[0] * h, axis[1] * h, axis[2] * h, cos(angle * 0.5)];
        let q = br(buffer, bone);
        let mut out = quat_mul(&turn, &a4(q));
        quat_norm(&mut out);
        put4(q, &out);
    }
}

/// cvr::gestures::OwnsHand: finger tracking poses this side's fingers (0 right, 1 left; CyberpunkVR_GestureMode).
pub fn owns_hand(side: i32) -> bool {
    // SAFETY: an export read whole.
    (0..2).contains(&side) && unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_GestureMode[side as usize]) } != 0
}
#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_apply(buffer: *mut u8) {
    CyberpunkVR_GestureMode = [0, 0];
    if buffer.is_null() || g_VRBind != 4 || g_isInVehicle {
        return;
    }
    let controls = INPUT.load(Ordering::Acquire);
    let held = EQUIPMENT.load(Ordering::Acquire);
    let now = now_ms();
    if (controls >> 33) & 1 == 0
        || (now.wrapping_sub(controls >> 34) & 0x3fff_ffff) > 250
        || held == 0
        || now < (held >> 2)
        || now - (held >> 2) > 500
    {
        return;
    }
    let kind = cvr_cyberware_selected_kind();
    let epoch = g_VrikFrameEpoch.load(Ordering::Relaxed);
    let mut unused = [0.0f32; 5];
    for side in 0..2i32 {
        let s = side as usize;
        // Gloves drive this hand in ApplyGloves, after every other finger layer.
        if CyberpunkVR_FingerBones != 0 && super::fingergrip::cvr_fingergrip_tracked_curl(side, unused.as_mut_ptr()) {
            STATE[s] = STATE0;
            continue;
        }
        let owned = ((held >> side) & 1) != 0 || cvr_gestures_port_holds(side);
        if owned {
            STATE[s] = STATE0;
            CyberpunkVR_GestureMode[s] = 0;
            continue;
        }
        let grip = ((controls >> (side * 8)) & 255) as f32 / 255.0;
        let trigger = ((controls >> (16 + side * 8)) & 255) as f32 / 255.0;
        // R3 belongs only to the right hand. L3 remains the game's ability.
        let thumb = side == 1 && ((controls >> 32) & 1) != 0;
        let wire = kind == 3;
        let st = &mut STATE[s];
        if st.epoch != epoch {
            let mut wanted = evaluate(grip, trigger, thumb, wire, cvr_cyberware_wire_deployment(side));
            for digit in 0..5 {
                GESTURE_WANTED[s][digit].store(wanted.closed[digit]);
            }
            let contact_at = CONTACT_STAMP[s].load(Ordering::Relaxed);
            let mut may_close = [1.0f32; 5];
            if !wire && contact_at != 0 && now >= contact_at && now - contact_at <= CONTACT_FRESH_MS {
                for digit in 0..5 {
                    may_close[digit] = 1.0 - CONTACT_HELD[s][digit].load();
                    wanted.closed[digit] = if CONTACT_FORCE[s][digit].load(Ordering::Relaxed) { may_close[digit] } else { min(wanted.closed[digit], may_close[digit]) };
                }
            }
            let calibration = calibration_targets(grip, trigger, thumb, wire);
            let dt = if st.stamp != 0 && now >= st.stamp { min((now - st.stamp) as f32 * 0.001, 0.05) } else { 0.016 };
            let b = if wire { 1.0 } else { 1.0 - exp(-dt / 0.04) };
            for digit in 0..5 {
                st.pose.closed[digit] += (wanted.closed[digit] - st.pose.closed[digit]) * b;
                // a surface moves a finger now, without the pace of a hand opening
                st.pose.closed[digit] = min(st.pose.closed[digit], may_close[digit]);
                st.pose.straight[digit] += (wanted.straight[digit] - st.pose.straight[digit]) * b;
                CyberpunkVR_GestureClosed[s][digit] = st.pose.closed[digit];
                CyberpunkVR_GestureStraight[s][digit] = st.pose.straight[digit];
            }
            for i in 0..st.calibration.len() {
                st.calibration[i] += (calibration[i] - st.calibration[i]) * b;
            }
            st.stamp = now;
            st.epoch = epoch;
        }
        CyberpunkVR_GestureMode[s] = if wire {
            6
        } else if thumb && grip > 0.5 {
            5
        } else if grip > 0.5 {
            if trigger > 0.5 {
                3
            } else {
                2
            }
        } else if trigger > 0.5 {
            4
        } else {
            1
        };
        // Captured rest locals are the baseline, below reload/held-object layers.
        if side == 0 {
            cvr_anim_rest_finger_pose(buffer, true);
        } else {
            cvr_anim_rest_finger_pose_right(buffer, true);
        }
        let mut posed = [0i32; 5];
        let (pose, calibration) = (STATE[s].pose, STATE[s].calibration);
        pose_digits(buffer, side, &pose, Some(&calibration), &mut posed, false, now);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_apply_gloves(buffer: *mut u8) {
    if buffer.is_null() || g_VRBind != 4 || CyberpunkVR_FingerBones == 0 {
        return;
    }
    let now = now_ms();
    let epoch = g_VrikFrameEpoch.load(Ordering::Relaxed);
    for side in 0..2i32 {
        let s = side as usize;
        let mut curl = [0.0f32; 5];
        if !super::fingergrip::cvr_fingergrip_tracked_curl(side, curl.as_mut_ptr()) {
            GLOVES[s] = GLOVE0;
            continue;
        }
        let g = &mut GLOVES[s];
        if g.epoch != epoch {
            let wanted = from_tracking(&curl);
            let dt = if g.stamp != 0 && now >= g.stamp { min((now - g.stamp) as f32 * 0.001, 0.05) } else { 0.016 };
            let b = if g.stamp != 0 { 1.0 - exp(-dt / 0.04) } else { 1.0 };
            for digit in 0..5 {
                g.pose.closed[digit] += (wanted.closed[digit] - g.pose.closed[digit]) * b;
                g.pose.straight[digit] += (wanted.straight[digit] - g.pose.straight[digit]) * b;
            }
            g.stamp = now;
            g.epoch = epoch;
        }
        let mut posed = [0i32; 5];
        let pose = GLOVES[s].pose;
        pose_digits(buffer, side, &pose, None, &mut posed, true, 0); // the open thumb: the rig's own pose
        let mut splay = [0.0f32; 4];
        let g = &mut GLOVES[s];
        if CyberpunkVR_FingerSpreadOn != 0 && CyberpunkVR_FingerSpread > 0.0 && super::fingergrip::cvr_fingergrip_tracked_splay(side, splay.as_mut_ptr()) {
            let b = if g.splay_valid { 1.0 - exp(-0.016 / 0.06) } else { 1.0 };
            for k in 0..4 {
                g.splay[k] += (splay[k] - g.splay[k]) * b;
            }
            g.splay_valid = true;
            let sp = g.splay;
            spread_digits(buffer, side, &sp, min(CyberpunkVR_FingerSpread, 2.0));
        } else {
            g.splay_valid = false;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_owns_hand(side: i32) -> bool {
    (0..2).contains(&side) && CyberpunkVR_GestureMode[side as usize] != 0
}

#[no_mangle]
pub unsafe extern "C" fn cvr_gestures_port_holds(side: i32) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    (side == 0 && CyberpunkVR_CarryLeft != 0)
        || g_VRReloadFingerActive[side as usize] != 0
        || (if side == 0 { g_VRSmokeFingerActiveL } else { g_VRSmokeFingerActive }) != 0
}

// ---- the natives of the fingers against what the hand meets (main's VrMenu.cpp)
macro_rules! finger_params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
type FingerFrame = red4ext_abi::stack::StackFrame;
unsafe fn finger_put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
extern "C" {
    fn cvr_xr_grip_force(side: i32) -> f32;
}
/// VRFingerContact(side, digit, limit, hold) -> Bool: how far that finger may close (0 open .. 1 fist); with hold it
/// closes exactly that far.
#[no_mangle]
pub unsafe extern "C" fn VRFingerContact(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut bool, _: i64) {
    let (mut side, mut digit, mut limit, mut hold) = (0i32, 0i32, 1.0f32, false);
    finger_params!(frame, side, digit, limit, hold);
    finger_contact(side, digit, limit, hold);
    finger_put(out, true);
}
/// VRFingerBend(side, digit, angle) -> Bool: how far a surface presses that finger back past straight (radians).
#[no_mangle]
pub unsafe extern "C" fn VRFingerBend(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut bool, _: i64) {
    let (mut side, mut digit, mut angle) = (0i32, 0i32, 0.0f32);
    finger_params!(frame, side, digit, angle);
    finger_bend(side, digit, angle);
    finger_put(out, true);
}
/// VRFingerClosed(side, digit) -> Float: the finger as drawn.
#[no_mangle]
pub unsafe extern "C" fn VRFingerClosed(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut f32, _: i64) {
    let (mut side, mut digit) = (0i32, 0i32);
    finger_params!(frame, side, digit);
    finger_put(out, finger_closed(side, digit));
}
/// VRFingerWanted(side, digit) -> Float: the finger as it would be with nothing in its way.
#[no_mangle]
pub unsafe extern "C" fn VRFingerWanted(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut f32, _: i64) {
    let (mut side, mut digit) = (0i32, 0i32);
    finger_params!(frame, side, digit);
    finger_put(out, finger_wanted(side, digit));
}
/// VRCarryLead() -> Float: xr_carry_lead.
#[no_mangle]
pub unsafe extern "C" fn VRCarryLead(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut f32, _: i64) {
    finger_params!(frame);
    finger_put(out, cvr_settings::lc!(xrCarryLead));
}
/// VRHandContactMargin() -> Float: xr_contact_margin.
#[no_mangle]
pub unsafe extern "C" fn VRHandContactMargin(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut f32, _: i64) {
    finger_params!(frame);
    finger_put(out, cvr_settings::lc!(xrContactMargin));
}
/// VRGripForce(side) -> Float: how hard a grip is squeezed past closed, 0..1, side 0 left. Only Index controllers
/// report it.
#[no_mangle]
pub unsafe extern "C" fn VRGripForce(_this: *mut core::ffi::c_void, frame: *mut FingerFrame, out: *mut f32, _: i64) {
    let mut side = 0i32;
    finger_params!(frame, side);
    finger_put(out, cvr_xr_grip_force(side));
}
