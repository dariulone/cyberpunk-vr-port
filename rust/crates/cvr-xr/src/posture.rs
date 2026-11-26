//! The body's posture with each frame's head sample (formerly src/Runtimes/PhysicalCrouch.cpp): the hip hinge
//! (cvr-body's BendTracker, walking bobs read as steps while the body follows the head), the crouch's depth after the
//! measured hinge, and the physical crouch -- held only on positional tracking actually tracked, in a context that
//! allows it (the switch, the player, a pose frame, not swimming or on a ladder) and while the game's crouch bridge
//! answers. And PhysicalCrouchRequested: the crouch of a fresh frame sample, for that bridge.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::bend::BendTracker;
use cvr_body::crouch::{crouch_depth, crouch_threshold, PhysicalCrouch};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::tracking::HeadPose;

/// std::chrono::steady_clock microseconds (QPC, divided without overflow as MSVC's clock does).
pub fn steady_us() -> u64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut t, mut f) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut t);
    }
    if f <= 0 {
        return 0;
    }
    let (t, f) = (t as u64, f as u64);
    (t / f) * 1_000_000 + (t % f) * 1_000_000 / f
}

fn fresh(now: u64, stamp: u64, max_age: u64) -> bool {
    stamp != 0 && now >= stamp && now - stamp <= max_age
}

struct Posture {
    bend: BendTracker,
    crouch: PhysicalCrouch,
}
static POSTURE: Mutex<Option<Posture>> = Mutex::new(None);
/// When the game's crouch bridge last said it is ready (0: not).
static BRIDGE_STAMP_US: AtomicU64 = AtomicU64::new(0);

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    static mut CyberpunkVR_PhysicalCrouchDepth: f32;
    static mut CyberpunkVR_PhysicalCrouchDesired: i32;
    static CyberpunkVR_BendWalkMode: i32;
    /// g_liveControls.xrPhysicalCrouch and xrPhysicalCrouchDepth.
    fn cvr_cpp_live_physical_crouch(enabled: *mut i32, depth: *mut f32);
    /// rust/crates/cvr-rig: the room-scale state, the swim, the ladder.
    fn cvr_roomscale_player_identity() -> usize;
    fn cvr_roomscale_pose_frame_allowed() -> bool;
    fn cvr_roomscale_body_following() -> bool;
    fn cvr_swimming_active() -> bool;
    fn cvr_ladder_active() -> bool;
}
#[cfg(not(test))]
extern "system" {
    fn xrLocateSpace(space: openxr_sys::Space, base: openxr_sys::Space, time: openxr_sys::Time, location: *mut openxr_sys::SpaceLocation) -> openxr_sys::Result;
}

#[cfg(not(test))]
fn live() -> (bool, f32) {
    let (mut enabled, mut depth) = (0i32, 0.0f32);
    // SAFETY: two out-parameters.
    unsafe { cvr_cpp_live_physical_crouch(&mut enabled, &mut depth) };
    (enabled != 0, depth)
}
/// CrouchContext: the switch on, a player, a pose frame, neither swimming nor on a ladder.
#[cfg(not(test))]
fn crouch_context(enabled: bool) -> bool {
    // SAFETY: the room-scale state's own queries.
    enabled && unsafe { cvr_roomscale_player_identity() != 0 && cvr_roomscale_pose_frame_allowed() && !cvr_swimming_active() && !cvr_ladder_active() }
}

/// OpenXRManager::SetPhysicalCrouchBridgeReady.
#[no_mangle]
pub extern "C" fn cvr_xr_set_physical_crouch_bridge_ready(ready: bool) {
    BRIDGE_STAMP_US.store(if ready { steady_us() } else { 0 }, Ordering::Release);
}

/// OpenXRManager::UpdateBodyPosture: the bend, the crouch's depth and the physical crouch for a new frame sample.
///
/// # Safety
/// `p` is an OpenXRHeadPose.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_update_body_posture(p: *mut HeadPose) {
    use openxr_sys as xr;
    use xr::Handle as _;
    // SAFETY (whole body): the caller's pose; the live session's spaces; the C++ half's read-outs written as it did.
    unsafe {
        let Some(p) = p.as_mut() else { return };
        let (enabled, depth_setting) = live();
        let mut guard = POSTURE.lock().unwrap_or_else(|e| e.into_inner());
        let st = guard.get_or_insert_with(|| Posture { bend: BendTracker::default(), crouch: PhysicalCrouch::default() });
        st.bend.set_squat_depth(crouch_threshold(depth_setting));
        let swimming_or_ladder = cvr_swimming_active() || cvr_ladder_active();
        let walking = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_BendWalkMode)) != 0 && cvr_roomscale_body_following();
        p.body_bend = st.bend.update(p.pos, p.ori, p.origin_serial, swimming_or_ladder, walking);
        p.crouch_depth = crouch_depth(st.bend.neck_drop(), &p.body_bend);
        let now = steady_us();
        let norm = p.ori.iter().map(|v| v * v).sum::<f32>();
        let valid = p.valid && p.pos.iter().all(|v| v.is_finite()) && norm.is_finite() && norm > 0.5 && norm < 1.5;
        let mut allowed = valid && crouch_context(enabled) && fresh(now, BRIDGE_STAMP_US.load(Ordering::Acquire), 500000);
        if allowed {
            // A cached view pose can stay valid after positional tracking is lost: it must not refresh a held crouch.
            let aim = crate::frame::cvr_xr_frame_aim_time_now();
            let (view, local) = (crate::frame::VIEW_SPACE.load(Ordering::Acquire), crate::frame::LOCAL_SPACE.load(Ordering::Acquire));
            let required = xr::SpaceLocationFlags::POSITION_VALID
                | xr::SpaceLocationFlags::ORIENTATION_VALID
                | xr::SpaceLocationFlags::POSITION_TRACKED
                | xr::SpaceLocationFlags::ORIENTATION_TRACKED;
            let mut loc: xr::SpaceLocation = core::mem::zeroed();
            loc.ty = xr::StructureType::SPACE_LOCATION;
            allowed = crate::events::session_running()
                && aim > 0
                && view != 0
                && local != 0
                && xrLocateSpace(xr::Space::from_raw(view), xr::Space::from_raw(local), xr::Time::from_nanos(aim), &mut loc).into_raw() >= 0
                && loc.location_flags.contains(required);
        }
        p.physical_crouch = st.crouch.update(p.crouch_depth, depth_setting, now, p.origin_serial, cvr_roomscale_player_identity() as u64, allowed);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(CyberpunkVR_PhysicalCrouchDepth), p.crouch_depth);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(CyberpunkVR_PhysicalCrouchDesired), if p.physical_crouch { 1 } else { 0 });
    }
}

/// OpenXRManager::PhysicalCrouchRequested: the crouch of a fresh frame sample of this origin, in a crouch context
/// with the bridge answering.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_physical_crouch_requested() -> bool {
    let now = steady_us();
    if !crouch_context(live().0) || !fresh(now, BRIDGE_STAMP_US.load(Ordering::Acquire), 500000) {
        return false;
    }
    let guard = crate::frame::FRAME.sample_lock.lock();
    let s = crate::frame::FRAME.sample.get(&guard);
    s.pose.valid && s.pose.origin_serial == crate::tracking::origin_serial() && fresh(now, s.stamp_us, 150000) && s.pose.physical_crouch
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freshness_and_the_steady_clock() {
        assert!(fresh(1_000, 900, 150));
        assert!(!fresh(1_000, 0, 150), "never stamped");
        assert!(!fresh(1_000, 1_001, 150), "a stamp from the future");
        assert!(!fresh(1_000, 800, 150), "too old");
        let a = steady_us();
        let b = steady_us();
        assert!(a > 0 && b >= a);
    }
}
