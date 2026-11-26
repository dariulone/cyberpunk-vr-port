//! One head per frame (formerly OpenXRManager's FrameAim, its cycle-head latch, LocateHeadPoseAt and
//! AcquireFrameHeadSample): the instant the XR cycle aimed this frame at (published with its stamp and an epoch, read
//! as one tuple), carried forward to now for consumers off the XR cycle; the pose that cycle located, as a flat latch;
//! the head located at an instant, base-relative and through the predict filter; and the ONE head sample per aim epoch
//! that the camera, the solve and the label all take.
//!
//! The state is owned here; in the Rust build OpenXRManager's members are references to it.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::calibration::AtomicF32;
use crate::hands::Pose;
use crate::srw::{Guarded, SrwLock};
use crate::tracking::{now_us, HeadPose};

/// The XR session's handles the head is located with (set when the spaces are made, cleared at teardown).
pub static SESSION: AtomicU64 = AtomicU64::new(0);
pub static LOCAL_SPACE: AtomicU64 = AtomicU64::new(0);
pub static VIEW_SPACE: AtomicU64 = AtomicU64::new(0);
/// The session's, the tracking (LOCAL or STAGE) space's and the view space's handles, in that order (the frame loop
/// and the hands' publication read them here; OpenXRManager's members hold the same handles).
pub fn session_handles() -> (u64, u64, u64) {
    (SESSION.load(Ordering::Acquire), LOCAL_SPACE.load(Ordering::Acquire), VIEW_SPACE.load(Ordering::Acquire))
}

/// cvr::tracking::FrameAimSnapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AimSnapshot {
    pub time: i64,
    pub stamp_us: u64,
    pub epoch: u64,
}
/// The frame's head sample (m_frameSample* under m_frameSampleMutex).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SampleState {
    pub epoch: u64,
    pub sequence: u64,
    pub stamp_us: u64,
    pub pose: HeadPose,
    pub aim: i64,
}
/// OpenXRManager's frame members (include/Runtimes/OpenXRManager.hpp CvrXrFrameShared).
#[repr(C)]
pub struct FrameShared {
    pub aim_lock: SrwLock,
    pub aim: Guarded<AimSnapshot>,
    pub cycle_valid: AtomicBool,
    pub cycle_ori: [AtomicF32; 4],
    pub cycle_pos: [AtomicF32; 3],
    pub sample_lock: SrwLock,
    pub sample: Guarded<SampleState>,
}
const _: () = assert!(core::mem::offset_of!(FrameShared, sample_lock) == 64 && core::mem::size_of::<FrameShared>() == 168);
pub static FRAME: FrameShared = FrameShared {
    aim_lock: SrwLock::new(),
    aim: Guarded::new(AimSnapshot { time: 0, stamp_us: 0, epoch: 0 }),
    cycle_valid: AtomicBool::new(false),
    cycle_ori: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero(), AtomicF32::new(1.0)],
    cycle_pos: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    sample_lock: SrwLock::new(),
    sample: Guarded::new(SampleState { epoch: !0, sequence: 0, stamp_us: 0, pose: HeadPose::EMPTY, aim: 0 }),
};
#[no_mangle]
pub extern "C" fn cvr_xr_frame_shared() -> *const FrameShared {
    &FRAME
}

/// FrameAim::Publish: a new target, its stamp and the next epoch, as one tuple.
#[no_mangle]
pub extern "C" fn cvr_xr_frame_aim_publish(time: i64, stamp_us: u64) {
    let guard = FRAME.aim_lock.lock();
    let a = FRAME.aim.get(&guard);
    *a = AimSnapshot { time, stamp_us, epoch: a.epoch + 1 };
}
/// FrameAim::Read.
pub fn aim() -> AimSnapshot {
    let guard = FRAME.aim_lock.lock();
    *FRAME.aim.get(&guard)
}
/// FrameAim::Read for the C++ half.
///
/// # Safety
/// `out` is a FrameAimSnapshot.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_frame_aim_read(out: *mut AimSnapshot) {
    // SAFETY: the caller's snapshot.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = aim();
    }
}
/// OpenXRManager::GetFrameAimTimeNow: the last cycle's aim carried forward by the time since its stamp (at most
/// 40 ms: past a couple of frames the stamp is stale), so a consumer off the XR cycle reads a continuous instant.
#[no_mangle]
pub extern "C" fn cvr_xr_frame_aim_time_now() -> i64 {
    let a = aim();
    if a.time <= 0 || a.stamp_us == 0 {
        return a.time;
    }
    let now = now_us();
    let d = now.saturating_sub(a.stamp_us);
    a.time + (d.min(40000) * 1000) as i64
}

/// OpenXRManager::GetCycleHeadPoseLocal: the pose the last XR cycle located, in LOCAL space.
pub fn cycle_head_local() -> Option<Pose> {
    if !FRAME.cycle_valid.load(Ordering::Acquire) {
        return None;
    }
    let pz = FRAME.cycle_pos[2].load_acquire();
    Some(Pose {
        orientation: Quat { x: FRAME.cycle_ori[0].load(), y: FRAME.cycle_ori[1].load(), z: FRAME.cycle_ori[2].load(), w: FRAME.cycle_ori[3].load() },
        position: Vec3 { x: FRAME.cycle_pos[0].load(), y: FRAME.cycle_pos[1].load(), z: pz },
    })
}

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_PoseFromCycle: i32;
    static CyberpunkVR_PredictFilter: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugAimLagLastUs: u32;
    static mut CyberpunkVR_DebugAimLagMinUs: u32;
    static mut CyberpunkVR_DebugAimLagMaxUs: u32;
    static mut CyberpunkVR_DebugAimLagSumUs: u64;
    static mut CyberpunkVR_DebugAimLagCount: u64;
    static mut CyberpunkVR_DebugAimLagHist: [u32; 12];
    fn GetHmdTrackingSmooth() -> f32;
}
#[cfg(not(test))]
extern "system" {
    fn xrLocateSpace(space: openxr_sys::Space, base: openxr_sys::Space, time: openxr_sys::Time, location: *mut openxr_sys::SpaceLocation) -> openxr_sys::Result;
}

/// The aim lag since SetFrameAimTime stamped the cycle (diagnostics: last, min, max, sum, count, 2 ms bins).
#[cfg(not(test))]
unsafe fn note_aim_lag() {
    let stamp = aim().stamp_us;
    if stamp == 0 {
        return;
    }
    let lag = now_us().saturating_sub(stamp) as u32;
    // SAFETY: the C++ half's diagnostic exports, written as it wrote them (one writer at a time is not promised there
    // either: they are read-outs).
    unsafe {
        use core::ptr::{addr_of_mut, read_volatile as rd, write_volatile as wr};
        wr(addr_of_mut!(CyberpunkVR_DebugAimLagLastUs), lag);
        if lag < rd(addr_of_mut!(CyberpunkVR_DebugAimLagMinUs)) {
            wr(addr_of_mut!(CyberpunkVR_DebugAimLagMinUs), lag);
        }
        if lag > rd(addr_of_mut!(CyberpunkVR_DebugAimLagMaxUs)) {
            wr(addr_of_mut!(CyberpunkVR_DebugAimLagMaxUs), lag);
        }
        wr(addr_of_mut!(CyberpunkVR_DebugAimLagSumUs), rd(addr_of_mut!(CyberpunkVR_DebugAimLagSumUs)).wrapping_add(lag as u64));
        wr(addr_of_mut!(CyberpunkVR_DebugAimLagCount), rd(addr_of_mut!(CyberpunkVR_DebugAimLagCount)).wrapping_add(1));
        let bin = ((lag / 2000) as usize).min(11);
        let hist = addr_of_mut!(CyberpunkVR_DebugAimLagHist).cast::<u32>().add(bin);
        wr(hist, rd(hist).wrapping_add(1));
    }
}

/// OpenXRManager::LocateHeadPoseAt: the head at `display_time` (or the cycle's own locate, CyberpunkVR_PoseFromCycle),
/// base-relative like GetHeadPose's, through the predict filter. Across an external reset, GetHeadPose instead.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_locate_head_pose_at(display_time: i64, out: *mut HeadPose) -> bool {
    use openxr_sys as xr;
    use xr::Handle as _;
    // SAFETY (whole body): the caller's pose; the live session's spaces.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        let (session, view, local) = (SESSION.load(Ordering::Acquire), VIEW_SPACE.load(Ordering::Acquire), LOCAL_SPACE.load(Ordering::Acquire));
        if session == 0 || view == 0 || local == 0 {
            return false;
        }
        if display_time <= 0 || !crate::events::session_running() {
            return false;
        }
        let reset = &crate::published::RESET;
        let located_at = reset.gate.serial();
        if crate::published::external_reset_pending() {
            return crate::tracking::cvr_xr_get_head_pose(out);
        }
        let raw = if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_PoseFromCycle)) != 0 {
            // FIXED PHASE: the pose the XR cycle located right after xrWaitFrame.
            let Some(p) = cycle_head_local() else { return false };
            p
        } else {
            let mut loc: xr::SpaceLocation = core::mem::zeroed();
            loc.ty = xr::StructureType::SPACE_LOCATION;
            if xrLocateSpace(xr::Space::from_raw(view), xr::Space::from_raw(local), xr::Time::from_nanos(display_time), &mut loc).into_raw() < 0 {
                return false;
            }
            if !loc.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID) {
                return false;
            }
            Pose {
                orientation: Quat { x: loc.pose.orientation.x, y: loc.pose.orientation.y, z: loc.pose.orientation.z, w: loc.pose.orientation.w },
                position: Vec3 { x: loc.pose.position.x, y: loc.pose.position.y, z: loc.pose.position.z },
            }
        };
        let _reset_guard = reset.lock.lock();
        if !(located_at & 1 == 0 && located_at == reset.gate.serial()) || crate::published::external_reset_pending() {
            return crate::tracking::cvr_xr_get_head_pose(out);
        }
        // The frame loop's recenter transform, so this pose is interchangeable with GetHeadPose's.
        let Some((base, origin)) = crate::tracking::base_and_origin() else { return false };
        out.origin_serial = origin;
        let inv = conjugate_quat(base.orientation);
        let rel_pos = rotate_vector(
            inv,
            Vec3 { x: raw.position.x - base.position.x, y: raw.position.y - base.position.y, z: raw.position.z - base.position.z },
        );
        let rel_ori = multiply_quat(inv, raw.orientation);
        if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 {
            note_aim_lag();
        }
        let mut pose = Pose { orientation: rel_ori, position: rel_pos };
        if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_PredictFilter)) != 0 {
            crate::head::cvr_xr_predict_filter(&mut pose, out.origin_serial, now_us(), GetHmdTrackingSmooth());
        }
        out.valid = true;
        out.pos = [pose.position.x, pose.position.y, pose.position.z];
        out.ori = [pose.orientation.x, pose.orientation.y, pose.orientation.z, pose.orientation.w];
        true
    }
}

/// OpenXRManager::AcquireFrameHeadSample: the ONE head sample of this aim epoch (the native frame's own head first,
/// when it has one), located at the aim (or the XR thread's latest), with its body posture, a sequence, a stamp and the
/// aim it was located at (crate::posture).
///
/// # Safety
/// `out` is an OpenXRHeadPose; the others are null or valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_acquire_frame_head_sample(out: *mut HeadPose, sequence: *mut u64, sampled_us: *mut u64, frame_time: *mut i64, prefer_native: bool) -> bool {
    // SAFETY (whole body): the caller's outputs.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        if crate::published::external_reset_pending() {
            return false;
        }
        // The camera and the solve take the head published with the native controller packet.
        if prefer_native && crate::published::cvr_xr_native_frame_head(out, sequence, sampled_us, frame_time, core::ptr::null_mut()) {
            return true;
        }
        let guard = FRAME.sample_lock.lock();
        let s = FRAME.sample.get(&guard);
        let a = aim();
        let give = |s: &SampleState, out: &mut HeadPose| {
            *out = s.pose;
            if let Some(v) = sequence.as_mut() {
                *v = s.sequence;
            }
            if let Some(v) = sampled_us.as_mut() {
                *v = s.stamp_us;
            }
            if let Some(v) = frame_time.as_mut() {
                *v = s.aim;
            }
        };
        if s.epoch == a.epoch && s.pose.valid && s.pose.origin_serial == crate::tracking::origin_serial() {
            give(s, out);
            return true;
        }
        let mut p = HeadPose::EMPTY;
        let mut ok = a.time > 0 && cvr_xr_locate_head_pose_at(a.time, &mut p) && p.valid;
        let located_at_aim = ok;
        if !ok {
            ok = crate::tracking::cvr_xr_get_head_pose(&mut p) && p.valid; // pre-session / locate failure
        }
        if !ok || crate::published::external_reset_pending() || p.origin_serial != crate::tracking::origin_serial() {
            return false;
        }
        if s.epoch != a.epoch || !s.pose.valid || s.pose.origin_serial != p.origin_serial {
            crate::posture::cvr_xr_update_body_posture(&mut p);
            s.pose = p;
            s.epoch = a.epoch;
            s.sequence += 1;
            s.stamp_us = now_us();
            s.aim = if located_at_aim { a.time } else { 0 };
        }
        give(s, out);
        out.valid
    }
}

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    /// The neck roomscale walks (frame head, runtime head), the eye's lever and the head's yaw, the head's x and -z.
    static mut CyberpunkVR_DebugRoomscaleNeck: [f32; 9];
    fn Get3DofMovement() -> i32;
    fn cvr_swimming_active() -> bool;
    fn cvr_roomscale_camera_consumed_at(origin: u64) -> cvr_body::vec2::Vec2;
}

/// OpenXRManager::GetRoomscaleSample: the raw XR sample qualifies tracking; the displacement is the camera's own
/// latch (the frame's head sample), the neck walked rather than the eyes, a hip hinge left out (it is no step), with
/// whether the player stands upright and looks ahead. The frame head and its aim go to the caller when valid.
///
/// # Safety
/// `out` is a cvr::roomscale::Sample; `frame_head`/`frame_time` null or valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_get_roomscale_sample(out: *mut crate::tracking::RoomscaleSample, frame_head: *mut HeadPose, frame_time: *mut i64) -> bool {
    use cvr_body::cmath::{atan2, hypot};
    use cvr_body::vec2::Vec2;
    // SAFETY (whole body): the caller's outputs; the C++ half's read-out slots.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        if let Some(h) = frame_head.as_mut() {
            *h = HeadPose::EMPTY;
        }
        if let Some(t) = frame_time.as_mut() {
            *t = 0;
        }
        let running = || crate::events::session_running();
        let mut tracking = crate::tracking::roomscale_sample();
        if !tracking.valid || !running() || crate::published::external_reset_pending() {
            *out = tracking;
            out.valid = false;
            return false;
        }
        // The native tick starts a NEW pose transaction: it samples XR, not the preceding tick's controller packet.
        let mut head = HeadPose::EMPTY;
        let (mut sequence, mut sampled_us, mut aim) = (0u64, 0u64, 0i64);
        let have_frame = cvr_xr_acquire_frame_head_sample(&mut head, &mut sequence, &mut sampled_us, &mut aim, false);
        tracking = crate::tracking::roomscale_sample();
        tracking.valid = tracking.valid && running() && !crate::published::external_reset_pending();
        if cvr_swimming_active() {
            head.body_bend = cvr_body::bend::BendSample { right: 0.0, forward: 0.0, angle: 0.0 };
        }
        let lever = if Get3DofMovement() != 0 { Vec2 { x: 0.0, y: 0.0 } } else { cvr_body::roomscale::eye_from_neck(head.ori[0], head.ori[1], head.ori[2], head.ori[3]) };
        let bend = head.body_bend;
        let [ox, oy, oz, ow] = head.ori;
        // the gaze's vertical part, sin(pitch); 5 cm of crouch is still standing
        let up = 2.0 * (ow * ox - oy * oz);
        let frame = crate::tracking::RoomscaleSample {
            head: Vec2 { x: head.pos[0] - bend.right - lever.x, y: -head.pos[2] - bend.forward - lever.y },
            sequence,
            origin: head.origin_serial,
            stamp_us: sampled_us,
            valid: have_frame && head.valid,
            runtime_head: Vec2 { x: 0.0, y: 0.0 },
            runtime_head_known: false,
            eye_from_neck: lever,
            bend: Vec2 { x: bend.right, y: bend.forward },
            upright: head.crouch_depth < 0.05 && hypot(bend.right, bend.forward) < 0.03 && up.abs() < 0.342,
        };
        *out = cvr_body::roomscale::movement_frame_sample(&tracking, &frame);
        let slots = core::ptr::addr_of_mut!(CyberpunkVR_DebugRoomscaleNeck).cast::<f32>();
        let neck = [frame.head.x, frame.head.y, tracking.head.x, tracking.head.y, lever.x, lever.y, 2.0 * atan2(oy, ow), head.pos[0], -head.pos[2]];
        for (k, v) in neck.into_iter().enumerate() {
            core::ptr::write_volatile(slots.add(k), v);
        }
        if out.valid {
            if let Some(h) = frame_head.as_mut() {
                *h = head;
            }
            if let Some(t) = frame_time.as_mut() {
                *t = aim;
            }
        }
        out.valid
    }
}

/// OpenXRManager::GetBodyTrackingFrame: the raw body frame published with the hands, while the roomscale sample and
/// it are fresh for this origin.
///
/// # Safety
/// `out` is a cvr::body::TrackingFrame.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_body_tracking_frame(out: *mut cvr_body::tracked_yaw::TrackingFrame) -> bool {
    if out.is_null() || crate::published::external_reset_pending() {
        return false;
    }
    let origin = crate::tracking::origin_serial();
    let rs = crate::tracking::roomscale_sample();
    let now = now_us();
    if !rs.valid || rs.origin != origin || now < rs.stamp_us || now - rs.stamp_us > 150000 {
        return false;
    }
    let guard = crate::tracking::HANDS.lock.lock();
    let frame = crate::published::PUBLISHED.data.get(&guard).body_frame;
    if !cvr_body::tracked_yaw::tracking_frame_fresh(&frame, origin, now_us()) {
        return false;
    }
    // SAFETY: the caller's frame.
    unsafe { *out = frame };
    true
}

/// OpenXRManager::ResolveRenderedPose: a base-relative head back into the tracking space, by the base of its origin.
///
/// # Safety
/// `pose` is an OpenXRHeadPose, `out` an XrPosef.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_resolve_rendered_pose(pose: *const HeadPose, out: *mut Pose) -> bool {
    // SAFETY: the caller's poses.
    let (Some(p), Some(out)) = (unsafe { pose.as_ref() }, unsafe { out.as_mut() }) else { return false };
    let Some(r) = resolve_rendered(p) else { return false };
    *out = r;
    true
}
/// ResolveRenderedPose's body: none for an invalid pose, during an external reset, or without the base of its origin.
pub fn resolve_rendered(p: &HeadPose) -> Option<Pose> {
    if !p.valid || crate::published::external_reset_pending() {
        return None;
    }
    let base = crate::tracking::base_for(p.origin_serial)?;
    let d = rotate_vector(base.orientation, Vec3 { x: p.pos[0], y: p.pos[1], z: p.pos[2] });
    Some(Pose {
        orientation: multiply_quat(base.orientation, Quat { x: p.ori[0], y: p.ori[1], z: p.ori[2], w: p.ori[3] }),
        position: Vec3 { x: base.position.x + d.x, y: base.position.y + d.y, z: base.position.z + d.z },
    })
}

/// OpenXRManager::AcquireCameraPoseFrame: the native frame's head and its consumed roomscale, else the frame's head
/// sample and the camera's consumed roomscale for its origin.
///
/// # Safety
/// `head` is an OpenXRHeadPose, `consumed` a Vec2; `sequence` null or valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_acquire_camera_pose_frame(head: *mut HeadPose, consumed: *mut cvr_body::vec2::Vec2, sequence: *mut u64) -> bool {
    // SAFETY: the caller's outputs.
    unsafe {
        if head.is_null() || consumed.is_null() {
            return false;
        }
        if crate::published::cvr_xr_native_frame_head(head, sequence, core::ptr::null_mut(), core::ptr::null_mut(), consumed) {
            return true;
        }
        if !cvr_xr_acquire_frame_head_sample(head, sequence, core::ptr::null_mut(), core::ptr::null_mut(), false) {
            return false;
        }
        *consumed = cvr_roomscale_camera_consumed_at((*head).origin_serial);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_aim_is_one_tuple_with_an_epoch() {
        let before = aim().epoch;
        cvr_xr_frame_aim_publish(5_000, 7);
        let a = aim();
        assert_eq!((a.time, a.stamp_us, a.epoch), (5_000, 7, before + 1));
    }
    #[test]
    fn the_session_handles_in_order() {
        assert_eq!(session_handles(), (0, 0, 0));
        SESSION.store(11, Ordering::Release);
        LOCAL_SPACE.store(22, Ordering::Release);
        VIEW_SPACE.store(33, Ordering::Release);
        let handles = session_handles();
        for h in [&SESSION, &LOCAL_SPACE, &VIEW_SPACE] {
            h.store(0, Ordering::Release);
        }
        assert_eq!(handles, (11, 22, 33));
    }
}

// ---- the instant this cycle locates at

/// The aim's lead state across cycles (FrameThreadMain's static PipelinePrediction and what it was last reset for).
struct LocateAim {
    prediction: cvr_camera::projection::PipelinePrediction,
    origin: u64,
    framegen: bool,
}
static LOCATE_AIM: std::sync::Mutex<LocateAim> =
    std::sync::Mutex::new(LocateAim { prediction: cvr_camera::projection::PipelinePrediction { lead_ns: 0.0, last_display: 0 }, origin: 0, framegen: false });

/// What one cycle aims at: the instant every locate of the cycle uses, and the serial of the present that will show
/// the frame the game is building now.
#[repr(C)]
pub struct LocateTarget {
    pub time: i64,
    pub building_serial: u64,
}

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_EnginePipelineDepth: i32;
    static CyberpunkVR_PosePredictMode: i32;
    static CyberpunkVR_PosePredictScale: f32;
    static CyberpunkVR_PoseAimScale: f32;
    static mut CyberpunkVR_DebugFitUsed: u64;
    static mut CyberpunkVR_DebugFitMissed: u64;
    static mut CyberpunkVR_DebugFitSlopeUs: u32;
    static mut CyberpunkVR_PoseTargetDebug: [i64; 5];
}

/// ONE instant for every locate in this cycle, aimed at the frame that will USE the pose (FrameThreadMain): mode 2 the
/// display-time fit's slot for the frame being built, through the bounded, slewed lead (one continuous expression,
/// never a switch between two formulas); mode 1 the display time plus a scaled period; mode 0 the display time. A new
/// tracking origin or frame-generation cadence discards the lead and the fit. Published as the frame's aim time.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_cycle_locate_target(display: i64, period: i64, present_count: u64, framegen: bool) -> LocateTarget {
    use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
    // SAFETY: the panel's knobs and the C++ half's counters; only the frame thread writes these.
    unsafe {
        let diag = read_volatile(addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0;
        let depth = read_volatile(addr_of!(CyberpunkVR_EnginePipelineDepth)).clamp(0, 3);
        let building_serial = present_count + 1 + depth as u64;
        let mut aim = LOCATE_AIM.lock().unwrap_or_else(|p| p.into_inner());
        let origin = crate::tracking::origin_serial();
        if aim.origin != origin || aim.framegen != framegen {
            aim.prediction.reset();
            aim.origin = origin;
            aim.framegen = framegen;
            crate::frame_identity::cvr_xr_fit_reset();
        }
        let mut time = display;
        match read_volatile(addr_of!(CyberpunkVR_PosePredictMode)) {
            2 => {
                let mut fitted = 0i64;
                let valid = crate::frame_identity::cvr_xr_fit_predict_display_time(building_serial, &mut fitted);
                if diag {
                    let c = if valid { addr_of_mut!(CyberpunkVR_DebugFitUsed) } else { addr_of_mut!(CyberpunkVR_DebugFitMissed) };
                    write_volatile(c, read_volatile(c).wrapping_add(1));
                }
                write_volatile(addr_of_mut!(CyberpunkVR_DebugFitSlopeUs), (crate::frame_identity::cvr_xr_fit_slope_ns() / 1000.0) as u32);
                time = aim.prediction.target(display, period, fitted, valid, read_volatile(addr_of!(CyberpunkVR_PoseAimScale)));
                if diag {
                    write_volatile(addr_of_mut!(CyberpunkVR_PoseTargetDebug), [display, time, fitted, building_serial as i64, valid as i64]);
                }
            }
            1 => time = display + (period as f32 * read_volatile(addr_of!(CyberpunkVR_PosePredictScale))) as i64,
            _ => {}
        }
        cvr_xr_frame_aim_publish(time, now_us());
        LocateTarget { time, building_serial }
    }
}
