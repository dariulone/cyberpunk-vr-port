//! The body's heading at the native movement adapter (formerly src/Hooks/BodyHeading.cpp): each motion request is
//! fed the follower's step (PhysicalMoveController::GetDeltaTransform, upstream of the transform write -- the
//! camera-input fields and the provider's velocity getter stay intact), and only a matching, actually accepted
//! request commits the independent view reference (the adapter's apply). Outside on-foot VRIK the native heading is
//! the fallback.
//!
//! The exports stay in src/Hooks/BodyHeading.cpp with its registration; the C++ API (Hooks/BodyHeading.hpp)
//! forwards here (src/Hooks/RoomscaleRust.cpp).
use super::body_yaw_follow::cvr_body_yaw_follow_step;
use super::env::log;
use super::roomscale_move::{
    cvr_roomscale_is_player_movement, cvr_roomscale_is_player_provider, cvr_roomscale_physical_body_heading_owned,
    cvr_roomscale_player_identity, PHYSICAL_ADAPTER_VTABLE,
};
use core::cell::Cell;
use core::ffi::c_void;
use cvr_body::cmath::{atan2, cos, sin};
use cvr_body::native_heading::NativeBodyHeading;
use cvr_engine::code::{expect, read, Pattern};
use cvr_engine::hooks::{install_chain, Detour};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

extern "C" {
    static mut CyberpunkVR_BodyHeadingHooksReady: i32;
    static mut CyberpunkVR_BodyHeadingCommits: u64;
    static mut CyberpunkVR_BodyHeadingRequests: u64;
    static mut CyberpunkVR_BodyTrackingYawRad: f32;
    static CyberpunkVR_DebugBodyHeadingSkip: i32;
    static mut CyberpunkVR_BodyYawRealignRad: f32;
    static mut CyberpunkVR_DebugBodyFollowOffsetDeg: f32;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
}

const APPLY: usize = 0xA99838;
const APPLY_BYTES: &str = "48 89 5C 24 08 48 89 7C 24 10 55 48 8B EC 48 83 EC 70";
const MOTION: usize = 0xE3C3A0;
const MOTION_BYTES: &str = "40 53 48 83 EC 30 0F 28 05 F3 F3 97 02 49 8B D8 0F 11 02 F3 0F 10 89 A4 00 00 00";

type ApplyFn = unsafe extern "system" fn(*mut c_void, *const f32, *const f32, u8) -> bool;
type MotionFn = unsafe extern "system" fn(*mut c_void, *mut f32, *mut f32);
static mut ORIG_APPLY: *mut c_void = core::ptr::null_mut();
static mut ORIG_MOTION: *mut c_void = core::ptr::null_mut();
static BASE: AtomicUsize = AtomicUsize::new(0);
static HEADING: Mutex<NativeBodyHeading> = Mutex::new(unsafe { core::mem::zeroed() });

fn heading() -> std::sync::MutexGuard<'static, NativeBodyHeading> {
    HEADING.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Default)]
struct PendingTurn {
    adapter: usize,
    owner: usize,
    origin: u64,
    native_turn: f32,
    rotation: [f32; 4],
    valid: bool,
}
thread_local! {
    static PENDING: Cell<PendingTurn> = const { Cell::new(PendingTurn { adapter: 0, owner: 0, origin: 0, native_turn: 0.0, rotation: [0.0; 4], valid: false }) };
}

fn player_adapter(adapter: usize) -> bool {
    let Some(state) = read::<usize>(adapter + 0x18) else { return false };
    cvr_roomscale_is_player_movement(state as *const c_void)
        && read::<usize>(state + 0x160) == Some(adapter)
        && read::<usize>(state + 0xA8).is_some_and(|p| cvr_roomscale_is_player_provider(p as *const c_void))
}
fn yaw_of(q: &[f32; 4]) -> Option<f32> {
    let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !n.is_finite() || n < 0.5 || n > 1.5 {
        return None;
    }
    Some(atan2(2.0 * (q[3] * q[2] + q[0] * q[1]), 1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2])))
}

/// The provider's motion request (vtable+20): the follower's step turned into the requested rotation, the request
/// noted for the commit.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_motion(provider: *mut c_void, position: *mut f32, rotation: *mut f32) {
    let original: MotionFn = core::mem::transmute(ORIG_MOTION);
    original(provider, position, rotation);
    PENDING.with(|p| p.set(PendingTurn::default()));
    if CyberpunkVR_BodyHeadingHooksReady == 0 || !cvr_roomscale_is_player_provider(provider) || !cvr_roomscale_physical_body_heading_owned() {
        return;
    }
    let Some(state) = read::<usize>(provider as usize + 0x10) else { return };
    let Some(adapter) = read::<usize>(state + 0x160) else { return };
    if adapter == 0 || read::<usize>(adapter) != Some(BASE.load(Ordering::Relaxed) + PHYSICAL_ADAPTER_VTABLE) || !player_adapter(adapter) {
        return;
    }
    let owner = cvr_roomscale_player_identity();
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let Some(before) = read::<[f32; 4]>(adapter + 0x30).and_then(|q| yaw_of(&q)) else { return };
    let r = [*rotation, *rotation.add(1), *rotation.add(2), *rotation.add(3)];
    let Some(native_turn) = yaw_of(&r) else { return };
    let offset = {
        let mut h = heading();
        h.observe(owner, origin, before);
        h.offset()
    };
    let step = if CyberpunkVR_DebugBodyHeadingSkip != 0 { 0.0 } else { cvr_body_yaw_follow_step(offset) };
    let (s, c) = (sin(step * 0.5), cos(step * 0.5));
    let delta = [c * r[0] - s * r[1], c * r[1] + s * r[0], c * r[2] + s * r[3], c * r[3] - s * r[2]];
    if step != 0.0 {
        core::ptr::copy_nonoverlapping(delta.as_ptr(), rotation, 4);
    }
    let rotation_now = [*rotation, *rotation.add(1), *rotation.add(2), *rotation.add(3)];
    PENDING.with(|p| p.set(PendingTurn { adapter, owner, origin, native_turn, rotation: rotation_now, valid: true }));
    CyberpunkVR_BodyHeadingRequests += 1;
}

/// The adapter's apply: never alters a native request or its destination; a matching, accepted request commits.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_apply(adapter: *mut c_void, position: *const f32, rotation: *const f32, flag: u8) -> bool {
    let original: ApplyFn = core::mem::transmute(ORIG_APPLY);
    let pending = PENDING.with(|p| p.get());
    let bits = |v: &[f32; 4]| v.map(f32::to_bits);
    let matching = pending.valid
        && pending.adapter == adapter as usize
        && pending.owner == cvr_roomscale_player_identity()
        && bits(&pending.rotation) == bits(&[*rotation, *rotation.add(1), *rotation.add(2), *rotation.add(3)]);
    if matching {
        PENDING.with(|p| {
            let mut v = p.get();
            v.valid = false;
            p.set(v);
        });
    }
    let applied = original(adapter, position, rotation, flag);
    if matching && applied {
        if let Some(after) = read::<[f32; 4]>(adapter as usize + 0x30).and_then(|q| yaw_of(&q)) {
            let mut h = heading();
            if h.matches(pending.owner, pending.origin) {
                h.commit(after, pending.native_turn);
                CyberpunkVR_BodyTrackingYawRad = h.view();
                CyberpunkVR_BodyYawRealignRad = h.offset();
                CyberpunkVR_DebugBodyFollowOffsetDeg = h.offset() * 57.295_779_5;
                CyberpunkVR_BodyHeadingCommits += 1;
            }
        }
    }
    applied
}

/// The image base and the two originals: the install's, or the tests' fakes.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_bind(base: usize, apply: *mut c_void, motion: *mut c_void) {
    BASE.store(base, Ordering::Relaxed);
    ORIG_APPLY = apply;
    ORIG_MOTION = motion;
}

/// Installed from src/Hooks/BodyHeading.cpp's registration (Boot, 79).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_body_heading_install() -> bool {
    let base = cvr_engine::code::image_base();
    BASE.store(base, Ordering::Relaxed);
    let site = |rva: usize, bytes: &str| Pattern::parse(bytes).is_some_and(|p| expect(base + rva, &p));
    if base == 0 || !site(APPLY, APPLY_BYTES) || !site(MOTION, MOTION_BYTES) {
        log("BodyHeading: native ABI mismatch; hooks not installed.\n");
        return false;
    }
    let detours = [
        Detour {
            name: "AdapterApply",
            target: APPLY,
            absolute: false,
            detour: cvr_body_heading_apply as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_APPLY),
            expect: Some(APPLY_BYTES),
        },
        Detour {
            name: "ProviderMotion",
            target: MOTION,
            absolute: false,
            detour: cvr_body_heading_motion as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_MOTION),
            expect: Some(MOTION_BYTES),
        },
    ];
    if install_chain("body-heading", &detours).is_err() {
        return false;
    }
    CyberpunkVR_BodyHeadingHooksReady = 1;
    log("BodyHeading: native body motion source and commit observer installed (2.31).\n");
    true
}

// ---- the API (Hooks/BodyHeading.hpp)

#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_ready() -> bool {
    CyberpunkVR_BodyHeadingHooksReady != 0
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_centre_yaw(fallback: f32) -> f32 {
    if !cvr_roomscale_physical_body_heading_owned() {
        return fallback;
    }
    let owner = cvr_roomscale_player_identity();
    let h = heading();
    if h.owned_by(owner) {
        h.centre_yaw()
    } else {
        fallback
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_tracking_yaw(fallback: f32) -> f32 {
    let mut yaw = 0.0f32;
    if cvr_body_read_tracking_yaw(&mut yaw) {
        yaw
    } else {
        fallback
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_read_tracking_yaw(yaw: *mut f32) -> bool {
    if yaw.is_null() {
        return false;
    }
    if !cvr_roomscale_physical_body_heading_owned() {
        cvr_body_heading_reset();
        return false;
    }
    let owner = cvr_roomscale_player_identity();
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let h = heading();
    if !h.matches(owner, origin) {
        return false;
    }
    *yaw = h.view();
    true
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_offset() -> f32 {
    if !cvr_roomscale_physical_body_heading_owned() {
        return 0.0;
    }
    let owner = cvr_roomscale_player_identity();
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let h = heading();
    if h.matches(owner, origin) {
        h.offset()
    } else {
        0.0
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_heading_reset() {
    heading().reset();
    CyberpunkVR_BodyTrackingYawRad = 0.0;
    CyberpunkVR_BodyYawRealignRad = 0.0;
    CyberpunkVR_DebugBodyFollowOffsetDeg = 0.0;
}
