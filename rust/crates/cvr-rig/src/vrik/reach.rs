//! The hand's reach in the game (formerly src/Natives/Reach.cpp; include/Natives/Reach.hpp): the point the targeting
//! looks at while a free hand reaches for a thing, and the interaction button the hand presses. The script module
//! publishes both (VRReachAim, VRReachUse); the aim point is current for 150 ms, a press for its own short span.
//! VRHandHolds answers whether a hand is busy: the port's own grip, or the support hand of a two-handed weapon.
use super::env::GetTickCount64;
use super::gestures::cvr_gestures_port_holds;
use super::weapon_arm::cvr_anim_weapon_in_left_hand;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

extern "C" {
    static CyberpunkVR_TwoHandActive: i32;
}

/// kStaleMs: the aim point's life.
const STALE_MS: u64 = 150;

fn tick() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { GetTickCount64() }
}

struct Reach {
    point: [f32; 3],
    point_tick: u64,
    use_until: u64,
}
static REACH: Mutex<Reach> = Mutex::new(Reach {
    point: [0.0; 3],
    point_tick: 0,
    use_until: 0,
});
fn reach() -> std::sync::MutexGuard<'static, Reach> {
    REACH.lock().unwrap_or_else(|p| p.into_inner())
}

/// RED4ext::Vector4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

/// VRReachAim's work for the plugin's own callers: the look-at ray turned onto a point (W below 0.5 clears it).
pub fn set_aim_point(point: [f32; 4]) -> bool {
    set_aim(Vector4 { x: point[0], y: point[1], z: point[2], w: point[3] })
}
/// VRReachUse's work for the plugin's own callers: the interaction button held that long.
pub fn set_use_seconds(seconds: f32) -> bool {
    set_use(seconds)
}
fn set_aim(point: Vector4) -> bool {
    let mut r = reach();
    if point.w < 0.5 || !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
        r.point_tick = 0;
        return false;
    }
    r.point = [point.x, point.y, point.z];
    r.point_tick = tick();
    true
}
fn set_use(seconds: f32) -> bool {
    let mut r = reach();
    if !(seconds > 0.0) {
        r.use_until = 0;
        return false;
    }
    r.use_until = tick() + (seconds.clamp(0.05, 2.0) * 1000.0) as u64;
    true
}

/// AimPoint: the point while the script gives one.
pub fn aim_point(point: &mut [f32; 3]) -> bool {
    let r = reach();
    if r.point_tick == 0 || tick().wrapping_sub(r.point_tick) > STALE_MS {
        return false;
    }
    *point = r.point;
    true
}
/// UseHeld: the hand holds the interaction button down.
pub fn use_held() -> bool {
    let r = reach();
    r.use_until != 0 && tick() < r.use_until
}
/// HandHolds: the port's own grip on `side`, or the support hand of an active two-handed grip.
pub fn hand_holds(side: i32) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    // SAFETY: the Rust rig's exports and the C++ half's two-hand flag.
    unsafe {
        let support = if cvr_anim_weapon_in_left_hand() { 1 } else { 0 };
        let s = side as usize;
        cvr_gestures_port_holds(side)
            || (CyberpunkVR_TwoHandActive != 0 && side == support)
            || (super::physhands::heldbody::holding_mask() & (1 << side)) != 0
            || super::physhands::adjust::holds_body(s)
            || super::physhands::grab::holds_on(s)
            || super::physhands::vehicle_hands::busy(s)
    }
}

/// VRReachAim(point: Vector4) -> Bool (W below 0.5 or a non-finite point clears it).
#[no_mangle]
pub unsafe extern "C" fn VRReachAim(_this: *mut core::ffi::c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut point = Vector4::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut point);
        frame.finish_params();
    }
    let aimed = set_aim(point);
    if !out.is_null() {
        *out = aimed;
    }
}
/// VRReachUse(seconds: Float) -> Bool (the press, 0.05..2 s; 0 or less releases it).
#[no_mangle]
pub unsafe extern "C" fn VRReachUse(_this: *mut core::ffi::c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut seconds = 0.0f32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut seconds);
        frame.finish_params();
    }
    let held = set_use(seconds);
    if !out.is_null() {
        *out = held;
    }
}
/// VRHandHolds(side: Int32) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRHandHolds(_this: *mut core::ffi::c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut side = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.finish_params();
    }
    let holds = hand_holds(side);
    if !out.is_null() {
        *out = holds;
    }
}

// The header's forwarders (Natives/Reach.hpp in the Rust build).

#[no_mangle]
pub unsafe extern "C" fn cvr_reach_aim_point(point: *mut f32) -> bool {
    let mut p = [0.0f32; 3];
    if point.is_null() || !aim_point(&mut p) {
        return false;
    }
    core::ptr::copy_nonoverlapping(p.as_ptr(), point, 3);
    true
}
#[no_mangle]
pub extern "C" fn cvr_reach_use_held() -> bool {
    use_held()
}
#[no_mangle]
pub extern "C" fn cvr_reach_hand_holds(side: i32) -> bool {
    hand_holds(side)
}
