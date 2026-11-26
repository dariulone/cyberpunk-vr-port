//! The controllers' vibration (formerly src/Runtimes/Haptics.cpp, main's since 2026-11): callers on any thread ask for
//! an event, the XR frame thread plays it (Tick, after xrSyncActions).
//!
//! WHY ENVELOPES AND NOT FREQUENCIES. The controllers this port runs on answer frequency very differently, so the
//! frequency stays unspecified and events differ in strength, length and decay: each event is an envelope of up to three
//! segments (amplitude, milliseconds) from a live-tunable profile.
//!
//! ONE ENVELOPE PER HAND. A new event replaces what is playing unless that has a higher priority and is still running:
//! a belt tick never cuts a block's tail. A round is no profile any more: the earlier fork's strength/length model
//! (legacy_shot, from the weapon's kick) as a per-hand pulse, merged when two arrive together; vehicle audio rumble
//! (cvr-render vehicle_audio.rs) is the lowest of all and cannot interrupt a pulse or an envelope.
//!
//! The output goes where cvr_settings::haptic_route says: the PlayStation VR2 Toolkit bridge's channel when it was
//! chosen and found at the session's start, else OpenXR (cvr_cpp_haptics_play).
use super::reload_handoff::clock_qpc;
use super::weapon_arm::cvr_anim_weapon_in_left_hand;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use cvr_body::cmath::clamp;
use cvr_settings::haptic_route::{self, dual_toolkit_shot, legacy_shot, safe_gain, vehicle_fresh, PulseRequest, HAPTIC_ROUTE, PULSE0};
use std::sync::Mutex;

pub const LEFT: i32 = 1;
pub const RIGHT: i32 = 2;
const SEGS: usize = 3;
const PROFILE_COUNT: i32 = 17;
/// The row of a lodged blade sliding along the struck model.
const BLADE_SLIDE_ROW: i32 = 16;

extern "C" {
    static mut CyberpunkVR_Haptics: i32;
    static mut CyberpunkVR_HapticScale: f32;
    static mut CyberpunkVR_HapticProfiles: [[f32; 6]; 17];
    static mut CyberpunkVR_HapticStats: [i32; 8];
    static mut CyberpunkVR_TwoHandActive: i32;
    static mut CyberpunkVR_WeaponKickDeg: f32;
    static g_VRIKClockQpc: Option<unsafe extern "C" fn() -> i64>;
    #[link_name = "?g_isDriving@@3U?$atomic@_N@std@@A"]
    static g_isDriving: core::sync::atomic::AtomicBool;
    /// OpenXRManager::ApplyHaptic: the vibration on one hand through OpenXR (or the bridge); xrApplyHapticFeedback's
    /// result, -1 when there is nothing to vibrate.
    fn cvr_cpp_haptics_play(hand: i32, amplitude: f32, seconds: f32) -> i32;
    /// cvr::psvr2::Haptics().Pulse: one impact queued for the bridge's mailbox.
    fn cvr_cpp_psvr2_pulse(hand: i32, amplitude: f32, milliseconds: i32);
}
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

/// A lodged blade's slide, counted when it reached a hand's actuator: [0] left, [1] right.
#[no_mangle]
pub static CyberpunkVR_BladeSlideApplied: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

#[derive(Clone, Copy)]
struct Envelope {
    amp: [f32; SEGS],
    start: [i64; SEGS],
    end: [i64; SEGS],
    /// First segment not yet started; SEGS = idle.
    next: usize,
    priority: i32,
    row: i32,
    /// When the last segment ends.
    done: i64,
}
const ENVELOPE0: Envelope = Envelope { amp: [0.0; SEGS], start: [0; SEGS], end: [0; SEGS], next: SEGS, priority: 0, row: -1, done: 0 };

struct State {
    hand: [Envelope; 2],
    pulse: [PulseRequest; 2],
}
/// 0 left, 1 right.
static STATE: Mutex<State> = Mutex::new(State { hand: [ENVELOPE0; 2], pulse: [PULSE0; 2] });
static mut QPC_FREQ: i64 = 0;
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn qpc_freq() -> i64 {
    if g_VRIKClockQpc.is_some() {
        return 1_000_000;
    }
    if QPC_FREQ == 0 {
        let mut f = 0i64;
        super::env::QueryPerformanceFrequency(&mut f);
        QPC_FREQ = f;
    }
    QPC_FREQ
}
unsafe fn ms_to_qpc(ms: f32) -> i64 {
    (ms as f64 * 0.001 * qpc_freq() as f64) as i64
}

fn priority_of(row: i32) -> i32 {
    match row {
        4 | 11 | 12 | 13 | 15 | 16 => 1, // belt, menu, a swing, a blade's slide
        0..=3 => 2,                      // shots
        _ => 3,                          // block / parry / deflect / pin / hit
    }
}

unsafe fn scale() -> f32 {
    safe_gain(core::ptr::read_volatile(&raw const CyberpunkVR_HapticScale), 1.0)
}

unsafe fn queue(row: i32, hand_mask: i32, mul: f32) {
    if !(0..PROFILE_COUNT).contains(&row) {
        return;
    }
    if CyberpunkVR_Haptics == 0 {
        CyberpunkVR_HapticStats[5] += 1;
        return;
    }
    let scale = scale() * mul;
    if scale <= 0.0 {
        return;
    }
    let p = CyberpunkVR_HapticProfiles[row as usize];
    let now = clock_qpc();
    let pri = priority_of(row);
    let mut s = state();
    for h in 0..2usize {
        if hand_mask & (if h == 0 { LEFT } else { RIGHT }) == 0 {
            continue;
        }
        let e = &mut s.hand[h];
        if now < e.done && e.priority > pri {
            continue;
        }
        let mut n = ENVELOPE0;
        let mut t = now;
        for seg in 0..SEGS {
            let ms = clamp(p[seg * 2 + 1], 0.0, 500.0);
            n.amp[seg] = clamp(p[seg * 2] * scale, 0.0, 1.0);
            n.start[seg] = t;
            n.end[seg] = t + ms_to_qpc(ms);
            t = n.end[seg];
        }
        n.next = 0;
        n.priority = pri;
        n.row = row;
        n.done = t;
        *e = n;
    }
    drop(s);
    CyberpunkVR_HapticStats[0] += 1;
    CyberpunkVR_HapticStats[4] = row;
}

/// One pulse on one hand (0 left, 1 right), merged with one already pending.
///
/// # Safety
/// Reads the haptics' exports.
pub unsafe fn pulse(hand: i32, amplitude: f32, duration_ms: i32) {
    if !(0..=1).contains(&hand) || CyberpunkVR_Haptics == 0 {
        return;
    }
    state().pulse[hand as usize].merge(amplitude, duration_ms, GetTickCount64());
}

/// Everything pending or playing dropped (the switch off, the session's end).
#[no_mangle]
pub unsafe extern "C" fn cvr_haptics_clear() {
    let mut s = state();
    s.hand = [ENVELOPE0; 2];
    s.pulse = [PULSE0; 2];
    drop(s);
    haptic_route::clear_vehicle_haptics();
}

unsafe fn shot_for_hand(hand: i32, kick: f32, support_hand_on_gun: bool) {
    if HAPTIC_ROUTE.toolkit() || !(0..=1).contains(&hand) {
        return;
    }
    CyberpunkVR_HapticStats[6] += 1;
    let shot = legacy_shot(kick);
    pulse(hand, shot.amplitude, shot.duration_ms);
    if support_hand_on_gun {
        pulse(1 - hand, shot.amplitude * 0.55, shot.duration_ms);
    }
}

/// One round left the barrel: weaponClass 1 handgun, 2 rifle/SMG/LMG, 3 shotgun, 4 sniper; `hand` the hand holding that
/// gun (1 right, 0 left); `kick` the gun's (0 or less: the weapon in hand's); `dual` a gun in each hand. With the
/// Toolkit bridge the bridge's adaptive triggers own a single gun's recoil, and only a dual wield's impacts are sent.
#[no_mangle]
pub unsafe extern "C" fn cvr_haptics_shot(weaponClass: i32, supportHandOnGun: bool, hand: i32, kick: f32, dual: bool) {
    if !(1..=4).contains(&weaponClass) || !(0..=1).contains(&hand) {
        return; // melee and "unknown" fire no rounds of their own
    }
    let kick = if kick > 0.0 { kick } else { CyberpunkVR_WeaponKickDeg };
    if HAPTIC_ROUTE.toolkit() {
        if dual && CyberpunkVR_Haptics != 0 && super::xr_calls::session_running() {
            let p = dual_toolkit_shot(kick);
            let amp = (p.amplitude * scale()).min(1.0);
            if amp > 0.0 {
                cvr_cpp_psvr2_pulse(hand, amp, p.duration_ms);
            }
        }
        return;
    }
    shot_for_hand(hand, kick, !dual && supportHandOnGun);
}

/// A scripted event (VRHapticEvent) on the hands in handMask.
#[no_mangle]
pub unsafe extern "C" fn cvr_haptics_play(event: i32, handMask: i32) {
    let row = match event {
        1 => 4,   // belt touch
        2 => 9,   // grenade pin
        3 => 10,  // grenade pin, the holding hand
        10 => 5,  // melee block
        11 => 6,  // melee parry
        12 => 7,  // bullet block
        13 => 8,  // bullet deflect
        20 => 11, // menu click
        21 => 12, // menu hover
        22 => 13, // menu slider step
        14 => 14, // melee hit
        15 => 15, // melee swing
        16 => 16, // a blade's slide
        _ => return,
    };
    // A BLOCK IS TAKEN BY BOTH HANDS when both are on the weapon.
    let mut mask = handMask;
    if ((5..=8).contains(&row) || row == 14 || row == 15 || row == 16) && CyberpunkVR_TwoHandActive != 0 {
        mask |= if cvr_anim_weapon_in_left_hand() { RIGHT } else { LEFT };
    }
    CyberpunkVR_HapticStats[7] += 1;
    queue(row, mask, 1.0);
}

/// XR frame thread only: start whatever segment, pulse or rumble is due, on the route's actuators.
#[no_mangle]
pub unsafe extern "C" fn cvr_haptics_tick() {
    if CyberpunkVR_Haptics == 0 || scale() <= 0.0 {
        cvr_haptics_clear();
        return;
    }
    // (amplitude, seconds, a blade's slide)
    let mut due = [(0.0f32, 0.0f32, false); 2];
    let toolkit = HAPTIC_ROUTE.toolkit();
    {
        let mut s = state();
        let now = clock_qpc();
        let now_ms = GetTickCount64();
        let gain = haptic_route::legacy_gain() * scale();
        let (vehicle, vehicle_ms) = haptic_route::vehicle_haptics();
        for h in 0..2usize {
            let pulse = s.pulse[h];
            s.pulse[h] = PULSE0;
            let e = &mut s.hand[h];
            // A block/parry takes precedence. Merge simultaneous swings/impacts, and keep their lifetime intact instead
            // of overwriting with engine rumble.
            if pulse.fresh(now_ms) && gain > 0.0 && !(now < e.done && e.priority > 2) {
                *e = ENVELOPE0;
                e.amp[0] = (pulse.amplitude * gain).min(1.0);
                e.start[0] = now;
                e.end[0] = now + ms_to_qpc(pulse.duration_ms as f32);
                e.start[1] = e.end[0];
                e.end[1] = e.end[0];
                e.start[2] = e.end[0];
                e.end[2] = e.end[0];
                e.next = 0;
                e.priority = 2;
                e.done = e.end[0];
                CyberpunkVR_HapticStats[0] += 1;
            }
            let mut started: i32 = -1;
            while e.next < SEGS && e.start[e.next] <= now {
                started = e.next as i32;
                e.next += 1;
            }
            if started >= 0 && e.amp[started as usize] > 0.0 {
                let left = (e.end[started as usize] - now) as f64 / qpc_freq() as f64;
                if left > 0.003 {
                    due[h] = (e.amp[started as usize], left as f32, e.row == BLADE_SLIDE_ROW);
                }
            }
            if now >= e.done && haptic_route::vehicle_gain() > 0.0 && vehicle_fresh(g_isDriving.load(Relaxed), now_ms, vehicle_ms) {
                let held = super::wheel_grab::cvr_anim_wheel_blend(h as i32) > 0.01;
                let audio = match haptic_route::vehicle_audio_channels() {
                    1 => vehicle[0].max(vehicle[1]),
                    2 => vehicle[0],
                    3 => vehicle[1],
                    _ => vehicle[h],
                };
                let amplitude = if haptic_route::vehicle_audio_require_grip() && !held { 0.0 } else { (audio * gain).min(1.0) };
                // The bridge's v1 mailbox delivers one hand per frame: cover two frames even at 30 FPS rather than
                // alternate silent gaps.
                if amplitude > 0.01 {
                    due[h] = (amplitude, if toolkit { 0.090 } else { 0.045 }, false);
                }
            }
        }
    }
    for (h, &(amp, sec, blade)) in due.iter().enumerate() {
        if amp <= 0.0 {
            continue;
        }
        if toolkit {
            cvr_cpp_psvr2_pulse(h as i32, amp, ((sec * 1000.0 + 0.5) as i32).clamp(1, 1000));
            CyberpunkVR_HapticStats[if h == 0 { 1 } else { 2 }] += 1;
            if blade {
                CyberpunkVR_BladeSlideApplied[h].fetch_add(1, Relaxed);
            }
            continue;
        }
        let r = cvr_cpp_haptics_play(h as i32, amp, sec);
        CyberpunkVR_HapticStats[3] = r;
        if r == 0 {
            CyberpunkVR_HapticStats[if h == 0 { 1 } else { 2 }] += 1;
            if blade {
                CyberpunkVR_BladeSlideApplied[h].fetch_add(1, Relaxed);
            }
        } else {
            CyberpunkVR_HapticStats[5] += 1;
        }
    }
}
