//! The XR runtime's Rust functions the solve calls by their names (rust/crates/cvr-xr; formerly through
//! src/Hooks/HookServices.cpp's cvr_cpp_xr_* adapters over OpenXRManager's forwarding methods). The calls whose
//! arguments the adapters passed on unchanged are declared where they are used; these keep the adapters' shapes.

use super::camera_link::HeadPose;
use super::xinput::VrController;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering::Relaxed};

/// cvr::tracking::FrameAimSnapshot.
#[repr(C)]
#[derive(Default)]
struct AimSnapshot {
    time: i64,
    stamp_us: u64,
    epoch: u64,
}
/// CvrXrSessionShared (rust/crates/cvr-xr/src/events.rs).
#[repr(C)]
struct SessionShared {
    stop_frame_thread: AtomicBool,
    running: AtomicBool,
    focused: AtomicBool,
    state: AtomicI32,
}
extern "C" {
    fn cvr_xr_acquire_frame_head_sample(out: *mut HeadPose, sequence: *mut u64, sampled_us: *mut u64, frame_time: *mut i64, prefer_native: bool) -> bool;
    fn cvr_xr_push_rendered_frame_pose(vrcam: bool, pose: *const HeadPose);
    fn cvr_xr_session_shared() -> *const SessionShared;
    fn cvr_xr_frame_aim_read(out: *mut AimSnapshot);
    fn cvr_xr_controller_state(out: *mut VrController, raw: bool) -> bool;
    fn cvr_xr_physical_crouch_requested() -> bool;
    fn cvr_xr_published_hand_frame(head: *mut HeadPose, hands: *mut HeadPose, which: i32, sequence: *mut u64, stamp_us: *mut u64) -> bool;
}
/// AcquireFrameHeadSample(out, nullptr, nullptr, nullptr, false): the XR cycle's own head sample.
pub unsafe fn acquire_frame_head(out: *mut HeadPose) -> bool {
    // SAFETY: the caller's pose; no other outputs.
    unsafe { cvr_xr_acquire_frame_head_sample(out, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), false) }
}
/// AcquireFrameHeadSample(out): the head sample, the native frame's preferred.
pub unsafe fn acquire_frame_head_sample(out: *mut HeadPose) -> bool {
    // SAFETY: as acquire_frame_head.
    unsafe { cvr_xr_acquire_frame_head_sample(out, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), true) }
}
/// PushRenderedFramePose / PushVrcamRenderedFramePose (nothing for a null pose).
pub unsafe fn push_rendered_frame_pose(pose: *const HeadPose, vrcam: bool) {
    if !pose.is_null() {
        // SAFETY: the caller's pose.
        unsafe { cvr_xr_push_rendered_frame_pose(vrcam, pose) }
    }
}
fn session() -> Option<&'static SessionShared> {
    // SAFETY: the XR runtime's process-lifetime state.
    unsafe { cvr_xr_session_shared().as_ref() }
}
/// IsSessionRunning.
pub unsafe fn session_running() -> bool {
    session().is_some_and(|s| s.running.load(Relaxed))
}
/// IsSessionFocused.
pub unsafe fn session_focused() -> bool {
    session().is_some_and(|s| s.focused.load(Relaxed))
}
fn aim() -> AimSnapshot {
    let mut snapshot = AimSnapshot::default();
    // SAFETY: a local snapshot.
    unsafe { cvr_xr_frame_aim_read(&mut snapshot) };
    snapshot
}
/// GetFrameAimEpoch.
pub unsafe fn frame_aim_epoch() -> u64 {
    aim().epoch
}
/// GetFrameAimTime.
pub unsafe fn frame_aim_time() -> i64 {
    aim().time
}
/// GetControllerState: the controllers as the game reads them (not raw).
pub unsafe fn controller_state(out: *mut VrController) -> bool {
    // SAFETY: the caller's snapshot.
    unsafe { cvr_xr_controller_state(out, false) }
}
/// PhysicalCrouchRequested.
pub unsafe fn physical_crouch_requested() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_xr_physical_crouch_requested() }
}
/// GetGestureHandFrame: the published hands' frame for gestures (`raw` the melee's raw hands); false without both
/// outputs.
pub unsafe fn gesture_hand_frame(head: *mut HeadPose, hands: *mut HeadPose, sequence: *mut u64, stamp: *mut u64, raw: bool) -> bool {
    if sequence.is_null() || stamp.is_null() {
        return false;
    }
    // SAFETY: the caller's outputs (two hands).
    unsafe { cvr_xr_published_hand_frame(head, hands, if raw { 2 } else { 1 }, sequence, stamp) }
}
