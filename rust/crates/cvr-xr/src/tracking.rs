//! The head's tracking state (OpenXRManager's head members, owned here; in the Rust build the C++ members are
//! references to these): the base-relative head pose the XR thread publishes and its velocity, the recenter request,
//! the recenter base under the render-pose lock (a lock both halves take, include/Utils/SrwMutex.hpp), and the
//! tracking origin's serial, bumped on every new base.
//!
//! And the head's samples under their own lock (SampleShared): the roomscale sample (the neck's planar position, its
//! origin, stamp and validity), the coherent head pose GetHeadPose hands out, and the sequential-sync pose.
//!
//! And what is done with them: the base taken for a frame (captured yaw-only on a recenter, the first pose or an
//! external reset), the base turned about the vertical (OpenXRManager::RotateBaseYaw), the head's yaw relative to it
//! (GetHmdYawRelToBody), the head published by the XR thread (pose, sample, velocity, loss), GetHeadPose.
use cvr_body::bend::BendSample;
use cvr_body::cmath::{atan2, cos, sin, sqrt};
use cvr_body::vec2::Vec2;
use cvr_body::xr::{multiply_quat, rotate_vector, Quat, Vec3};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::calibration::AtomicF32;
use crate::hands::{AngleFilter, HandMotion, Pose};
use crate::srw::{Guarded, SrwLock};

/// OpenXRManager's head members (include/Runtimes/OpenXRManager.hpp CvrXrHeadShared).
#[repr(C)]
pub struct HeadShared {
    pub pose_valid: AtomicBool,
    pub pos: [AtomicF32; 3],
    pub ori: [AtomicF32; 4],
    pub vel_valid: AtomicBool,
    pub ang_vel: [AtomicF32; 3],
    pub lin_vel: [AtomicF32; 3],
    pub recenter_requested: AtomicBool,
    /// m_renderPoseMutex: guards the base (and, in C++, the render-pose ring).
    pub render_pose_lock: SrwLock,
    pub base_pose_set: Guarded<bool>,
    pub base_pose: Guarded<Pose>,
    pub tracking_origin_serial: AtomicU64,
}
const _: () = assert!(core::mem::offset_of!(HeadShared, render_pose_lock) == 64);
const _: () = assert!(core::mem::offset_of!(HeadShared, base_pose) == 76);
const _: () = assert!(core::mem::size_of::<HeadShared>() == 112);

const ZERO_POSE: Pose = Pose { orientation: Quat { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: Vec3 { x: 0.0, y: 0.0, z: 0.0 } };
pub(crate) static HEAD: HeadShared = HeadShared {
    pose_valid: AtomicBool::new(false),
    pos: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    ori: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero(), AtomicF32::new(1.0)],
    vel_valid: AtomicBool::new(false),
    ang_vel: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    lin_vel: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    recenter_requested: AtomicBool::new(false),
    render_pose_lock: SrwLock::new(),
    base_pose_set: Guarded::new(false),
    base_pose: Guarded::new(ZERO_POSE),
    tracking_origin_serial: AtomicU64::new(0),
};
#[no_mangle]
pub extern "C" fn cvr_xr_head_shared() -> *const HeadShared {
    &HEAD
}

/// The base for this frame, under the render-pose lock: captured yaw-only from `head` (crate::head::yaw_only_base, its
/// log line, a new tracking origin) when there is none yet, a recenter was asked or an external reset is pending.
/// Returns whether it was captured now.
///
/// # Safety
/// `head` and `base` are XrPosef.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_take_base(head: *const Pose, external_reset_pending: bool, base: *mut Pose) -> bool {
    if head.is_null() || base.is_null() {
        return false;
    }
    let guard = HEAD.render_pose_lock.lock();
    let set = HEAD.base_pose_set.get(&guard);
    let pose = HEAD.base_pose.get(&guard);
    // !set || recenter || pending, short-circuiting as the C++ did: the request is consumed only with a base set.
    let reset = !*set || HEAD.recenter_requested.swap(false, Ordering::Relaxed) || external_reset_pending;
    if reset {
        // SAFETY: the caller's pose; the base written under the lock.
        unsafe { crate::head::cvr_xr_capture_base(head, pose) };
        *set = true;
        HEAD.tracking_origin_serial.fetch_add(1, Ordering::AcqRel);
    }
    // SAFETY: the caller's out pose.
    unsafe { base.write(*pose) };
    reset
}

/// OpenXRManager::RequestRecenter: the next frame captures a new base.
#[no_mangle]
pub extern "C" fn cvr_xr_request_recenter() {
    HEAD.recenter_requested.store(true, Ordering::Relaxed);
}

/// OpenXRManager::RotateBaseYaw: the base turned about the vertical, so the head's base-relative yaw shifts by
/// -radians (the body realign's pair with an equal heading injection).
#[no_mangle]
pub extern "C" fn cvr_xr_rotate_base_yaw(radians: f32) {
    if radians == 0.0 {
        return;
    }
    let guard = HEAD.render_pose_lock.lock();
    if !*HEAD.base_pose_set.get(&guard) {
        return;
    }
    let base = HEAD.base_pose.get(&guard);
    let h = radians * 0.5;
    let mut q = multiply_quat(base.orientation, Quat { x: 0.0, y: sin(h), z: 0.0, w: cos(h) });
    let n = sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    if n > 1e-8 {
        let inv = 1.0 / n;
        q.x *= inv;
        q.y *= inv;
        q.z *= inv;
        q.w *= inv;
    }
    base.orientation = q;
}

/// The tracking origin's serial (GetTrackingOriginSerial).
pub fn origin_serial() -> u64 {
    HEAD.tracking_origin_serial.load(Ordering::Acquire)
}
/// The recenter base, when one is set and `origin` is still the current one (read under the render-pose lock).
pub fn base_for(origin: u64) -> Option<Pose> {
    let guard = HEAD.render_pose_lock.lock();
    (*HEAD.base_pose_set.get(&guard) && origin == origin_serial()).then(|| *HEAD.base_pose.get(&guard))
}
/// The recenter base and the current origin, read together under the render-pose lock; none before a base is set.
pub fn base_and_origin() -> Option<(Pose, u64)> {
    let guard = HEAD.render_pose_lock.lock();
    (*HEAD.base_pose_set.get(&guard)).then(|| (*HEAD.base_pose.get(&guard), origin_serial()))
}
/// XrDiagNowMs: QPC milliseconds as a double.
pub fn now_ms() -> f64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut t, mut f) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut t);
    }
    if f == 0 {
        return 0.0;
    }
    t as f64 * 1000.0 / f as f64
}

/// The XR thread's roomscale sample, copied under its lock.
pub fn roomscale_sample() -> RoomscaleSample {
    let guard = SAMPLE.lock.lock();
    *SAMPLE.roomscale.get(&guard)
}

/// The sequential-sync pose is valid (m_syncedPoseValid).
pub fn synced_pose_valid() -> bool {
    SAMPLE.synced_valid.load(Ordering::Relaxed)
}

/// The published head's height (m_posY).
pub fn head_y() -> f32 {
    HEAD.pos[1].load()
}

/// OpenXRManager::GetHmdYawRelToBody: the published head's heading about the vertical.
#[no_mangle]
pub extern "C" fn cvr_xr_hmd_yaw_rel_to_body() -> f32 {
    let [x, y, z, w] = [HEAD.ori[0].load(), HEAD.ori[1].load(), HEAD.ori[2].load(), HEAD.ori[3].load()];
    atan2(2.0 * (w * y + x * z), 1.0 - 2.0 * (y * y + z * z))
}

/// OpenXRHeadPose (include/Runtimes/OpenXRManager.hpp), whole.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadPose {
    pub pos: [f32; 3],
    pub ori: [f32; 4],
    pub valid: bool,
    pub origin_serial: u64,
    pub body_bend: BendSample,
    pub crouch_depth: f32,
    pub physical_crouch: bool,
    /// AFW's eye for MAIN, -1 when not placed.
    pub afw_eye: i8,
}
const _: () = assert!(core::mem::size_of::<HeadPose>() == 64 && core::mem::offset_of!(HeadPose, body_bend) == 40);
impl HeadPose {
    /// `OpenXRHeadPose{}`: zeros, but afwEye's own default.
    pub const EMPTY: HeadPose = HeadPose {
        pos: [0.0; 3],
        ori: [0.0; 4],
        valid: false,
        origin_serial: 0,
        body_bend: BendSample { right: 0.0, forward: 0.0, angle: 0.0 },
        crouch_depth: 0.0,
        physical_crouch: false,
        afw_eye: -1,
    };
}

/// cvr::roomscale::Sample (include/Runtimes/RoomscaleMovement.hpp), cvr-body's mirror.
pub type RoomscaleSample = cvr_body::roomscale::Sample;
const _: () = assert!(core::mem::size_of::<RoomscaleSample>() == 72 && core::mem::offset_of!(RoomscaleSample, upright) == 64);
const V0: Vec2 = Vec2 { x: 0.0, y: 0.0 };

/// The head's samples (include/Runtimes/OpenXRManager.hpp CvrXrSampleShared): m_roomscaleSampleMutex and what it
/// guards, and the sequential-sync pose's atomics.
#[repr(C)]
pub struct SampleShared {
    pub lock: SrwLock,
    pub roomscale: Guarded<RoomscaleSample>,
    pub coherent_head: Guarded<HeadPose>,
    pub synced_valid: AtomicBool,
    pub synced_pos: [AtomicF32; 3],
    pub synced_ori: [AtomicF32; 4],
}
const _: () = assert!(core::mem::offset_of!(SampleShared, coherent_head) == 80 && core::mem::size_of::<SampleShared>() == 176);
static SAMPLE: SampleShared = SampleShared {
    lock: SrwLock::new(),
    roomscale: Guarded::new(RoomscaleSample {
        head: V0,
        sequence: 0,
        origin: 0,
        stamp_us: 0,
        valid: false,
        runtime_head: V0,
        runtime_head_known: false,
        eye_from_neck: V0,
        bend: V0,
        upright: true,
    }),
    coherent_head: Guarded::new(HeadPose::EMPTY),
    synced_valid: AtomicBool::new(false),
    synced_pos: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    synced_ori: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero(), AtomicF32::new(1.0)],
};
#[no_mangle]
pub extern "C" fn cvr_xr_sample_shared() -> *const SampleShared {
    &SAMPLE
}

/// XrDiagNowUs: QPC microseconds (the multiplication wrapping as MSVC's does).
pub fn now_us() -> u64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut t, mut f) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut t);
    }
    if f == 0 {
        return 0;
    }
    (t.wrapping_mul(1_000_000) / f) as u64
}

#[cfg(not(test))]
extern "C" {
    fn Get3DofMovement() -> i32;
    fn GetSyncSequential() -> i32;
}

/// The XR thread's roomscale sample from the base-relative head: the neck's planar position (the eyes' lever taken
/// off, as GetRoomscaleSample's frame head is), a new sequence, the origin, the stamp, whether tracking is usable.
///
/// # Safety
/// `rel` is an XrPosef.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_publish_head_sample(rel: *const Pose, session_state: i32, location_flags: u64) {
    // SAFETY: the caller's pose; a plain C getter.
    let (Some(rel), three_dof) = (unsafe { rel.as_ref() }, unsafe { Get3DofMovement() } != 0) else { return };
    let o = rel.orientation;
    let lever = if three_dof { V0 } else { cvr_body::roomscale::eye_from_neck(o.x, o.y, o.z, o.w) };
    let guard = SAMPLE.lock.lock();
    let s = SAMPLE.roomscale.get(&guard);
    s.head = Vec2 { x: rel.position.x - lever.x, y: -rel.position.z - lever.y };
    s.sequence += 1;
    s.origin = HEAD.tracking_origin_serial.load(Ordering::Acquire);
    s.stamp_us = now_us();
    s.valid = cvr_body::tracking::head_tracking_usable(session_state, location_flags);
}

/// The head the XR thread hands out (filtered, base-relative): the atomics and the coherent pose GetHeadPose reads.
///
/// # Safety
/// `filtered` is an XrPosef.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_publish_head(filtered: *const Pose) {
    // SAFETY: the caller's pose.
    let Some(f) = (unsafe { filtered.as_ref() }) else { return };
    let (p, o) = (f.position, f.orientation);
    for (slot, v) in HEAD.pos.iter().zip([p.x, p.y, p.z]) {
        slot.store(v);
    }
    for (slot, v) in HEAD.ori.iter().zip([o.x, o.y, o.z, o.w]) {
        slot.store(v);
    }
    HEAD.pose_valid.store(true, Ordering::Relaxed);
    let guard = SAMPLE.lock.lock();
    *SAMPLE.coherent_head.get(&guard) = HeadPose {
        pos: [p.x, p.y, p.z],
        ori: [o.x, o.y, o.z, o.w],
        valid: true,
        origin_serial: HEAD.tracking_origin_serial.load(Ordering::Acquire),
        ..HeadPose::EMPTY
    };
}

/// The head's velocity rotated into the base frame (so GetHeadPose's readers can predict), or none.
///
/// # Safety
/// The vectors are XrVector3f.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_publish_head_velocity(base_inv: Quat, angular: *const Vec3, linear: *const Vec3, valid: bool) {
    // SAFETY: the caller's vectors.
    match (valid, unsafe { angular.as_ref() }, unsafe { linear.as_ref() }) {
        (true, Some(a), Some(l)) => {
            let a = rotate_vector(base_inv, *a);
            let l = rotate_vector(base_inv, *l);
            for (slot, v) in HEAD.ang_vel.iter().zip([a.x, a.y, a.z]) {
                slot.store(v);
            }
            for (slot, v) in HEAD.lin_vel.iter().zip([l.x, l.y, l.z]) {
                slot.store(v);
            }
            HEAD.vel_valid.store(true, Ordering::Relaxed);
        }
        _ => HEAD.vel_valid.store(false, Ordering::Relaxed),
    }
}

/// The head was not located this cycle: no velocity, no roomscale sample, and -- when no reset is in the way -- no
/// coherent head.
#[no_mangle]
pub extern "C" fn cvr_xr_head_lost(reset_stable: bool) {
    HEAD.vel_valid.store(false, Ordering::Relaxed);
    let guard = SAMPLE.lock.lock();
    SAMPLE.roomscale.get(&guard).valid = false;
    if reset_stable {
        SAMPLE.coherent_head.get(&guard).valid = false;
    }
}

/// OpenXRManager::GetHeadPose: the sequential-sync pose when that mode has one, else the coherent head; valid only
/// while the session runs; no position in 3DoF movement.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_get_head_pose(out: *mut HeadPose) -> bool {
    // SAFETY: the caller's pose; plain C getters.
    let (Some(out), sync, three_dof) = (unsafe { out.as_mut() }, unsafe { GetSyncSequential() } != 0, unsafe { Get3DofMovement() } != 0) else {
        return false;
    };
    out.origin_serial = HEAD.tracking_origin_serial.load(Ordering::Acquire);
    let synced = sync && SAMPLE.synced_valid.load(Ordering::Relaxed);
    if synced {
        out.valid = true;
        out.pos = core::array::from_fn(|i| SAMPLE.synced_pos[i].load());
        out.ori = core::array::from_fn(|i| SAMPLE.synced_ori[i].load());
    } else {
        // valid, the pose and the rest of it from the coherent head (the atomics' values are overwritten).
        let guard = SAMPLE.lock.lock();
        *out = *SAMPLE.coherent_head.get(&guard);
    }
    out.valid = out.valid && crate::events::session_running();
    if three_dof {
        out.pos = [0.0; 3];
    }
    out.valid
}

/// The hands' state (include/Runtimes/OpenXRManager.hpp CvrXrHandsShared): m_handMutex, the head-relative hands it
/// guards, their motion and aim-yaw filters, and the aim yaws for hand-oriented locomotion.
#[repr(C)]
pub struct HandsShared {
    pub lock: SrwLock,
    pub hands: Guarded<[HeadPose; 2]>,
    pub motion: Guarded<[HandMotion; 2]>,
    pub aim_filter: Guarded<[AngleFilter; 2]>,
    pub yaw_rel: [AtomicF32; 2],
    pub yaw_valid: [AtomicBool; 2],
}
const _: () = assert!(core::mem::offset_of!(HandsShared, motion) == 136 && core::mem::offset_of!(HandsShared, yaw_rel) == 296);
const _: () = assert!(core::mem::size_of::<HandsShared>() == 312);
const NO_MOTION: HandMotion =
    HandMotion { linear: [0.0; 3], angular: [0.0; 3], valid: false, prev_pose: ZERO_POSE, prev_time: 0, origin: 0 };
pub static HANDS: HandsShared = HandsShared {
    lock: SrwLock::new(),
    hands: Guarded::new([HeadPose::EMPTY; 2]),
    motion: Guarded::new([NO_MOTION; 2]),
    aim_filter: Guarded::new([AngleFilter { initialized: false, angle_rad: 0.0 }; 2]),
    yaw_rel: [AtomicF32::zero(), AtomicF32::zero()],
    yaw_valid: [AtomicBool::new(false), AtomicBool::new(false)],
};
#[no_mangle]
pub extern "C" fn cvr_xr_hands_shared() -> *const HandsShared {
    &HANDS
}

/// OpenXRManager::GetHandPose: one hand, head-relative, as the XR thread last located it.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_get_hand_pose(hand: i32, out: *mut HeadPose) -> bool {
    // SAFETY: the caller's pose.
    let Some(out) = (unsafe { out.as_mut() }) else { return false };
    if !(0..=1).contains(&hand) {
        return false;
    }
    let guard = HANDS.lock.lock();
    *out = HANDS.hands.get(&guard)[hand as usize];
    out.valid
}

/// OpenXRManager::GetHandMotion: the controller's own motion, while it and its hand are valid.
///
/// # Safety
/// `linear` and `angular` hold three floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_get_hand_motion(hand: i32, linear: *mut f32, angular: *mut f32) -> bool {
    if linear.is_null() || angular.is_null() || !(0..=1).contains(&hand) {
        return false;
    }
    let guard = HANDS.lock.lock();
    let m = HANDS.motion.get(&guard)[hand as usize];
    if !m.valid || !HANDS.hands.get(&guard)[hand as usize].valid {
        return false;
    }
    // SAFETY: the caller's three-float arrays.
    unsafe {
        core::ptr::copy_nonoverlapping(m.linear.as_ptr(), linear, 3);
        core::ptr::copy_nonoverlapping(m.angular.as_ptr(), angular, 3);
    }
    true
}

/// OpenXRManager::GetHandYawRelToBody: where the controller points, relative to the base; the head's heading while
/// the controller is not tracked (so locomotion does not snap to 0 mid-step).
#[no_mangle]
pub extern "C" fn cvr_xr_hand_yaw_rel_to_body(side: i32) -> f32 {
    if !(0..=1).contains(&side) {
        return 0.0;
    }
    if !HANDS.yaw_valid[side as usize].load(Ordering::Relaxed) {
        return cvr_xr_hmd_yaw_rel_to_body();
    }
    HANDS.yaw_rel[side as usize].load()
}

/// OpenXRManager::GetBodyYawFromHands: the chest's heading from the line between the hands (rotated into the base
/// frame by the head), the line turned +90 deg about the vertical; none with a hand untracked or the hands together.
///
/// # Safety
/// `out` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_body_yaw_from_hands(out: *mut f32) -> bool {
    if out.is_null() {
        return false;
    }
    let (l, r) = {
        let guard = HANDS.lock.lock();
        let h = HANDS.hands.get(&guard);
        (h[0], h[1])
    };
    if !l.valid || !r.valid {
        return false;
    }
    let rel = Quat { x: HEAD.ori[0].load(), y: HEAD.ori[1].load(), z: HEAD.ori[2].load(), w: HEAD.ori[3].load() };
    let pl = rotate_vector(rel, Vec3 { x: l.pos[0], y: l.pos[1], z: l.pos[2] });
    let pr = rotate_vector(rel, Vec3 { x: r.pos[0], y: r.pos[1], z: r.pos[2] });
    let dx = pr.x - pl.x;
    let dz = pr.z - pl.z;
    if dx * dx + dz * dz < 0.12 * 0.12 {
        return false;
    }
    // SAFETY: the caller's float.
    unsafe { *out = atan2(-dz, dx) };
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_base_turns_about_the_vertical() {
        {
            let g = HEAD.render_pose_lock.lock();
            *HEAD.base_pose_set.get(&g) = true;
            *HEAD.base_pose.get(&g) = Pose { orientation: Quat::IDENTITY, position: Vec3::default() };
        }
        cvr_xr_rotate_base_yaw(0.5);
        let g = HEAD.render_pose_lock.lock();
        let q = HEAD.base_pose.get(&g).orientation;
        assert!((q.y - 0.25f32.sin()).abs() < 1e-6 && (q.w - 0.25f32.cos()).abs() < 1e-6, "{q:?}");
    }
}

/// The recenter base mirrored for the submit path (OpenXRManager's m_baseOri*/m_basePos*, GetRecenterBase): the base
/// a rendered pose is put back into LOCAL space by. Written by the XR thread with each located head.
#[repr(C)]
pub struct BaseMirror {
    pub ori: [AtomicF32; 4],
    pub pos: [AtomicF32; 3],
}
const _: () = assert!(core::mem::size_of::<BaseMirror>() == 28);
pub static BASE_MIRROR: BaseMirror = BaseMirror {
    ori: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero(), AtomicF32::new(1.0)],
    pos: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
};
#[no_mangle]
pub extern "C" fn cvr_xr_base_mirror() -> *const BaseMirror {
    &BASE_MIRROR
}
/// PublishRecenterBase.
pub fn publish_base_mirror(b: &Pose) {
    let m = &BASE_MIRROR;
    m.ori[0].store(b.orientation.x);
    m.ori[1].store(b.orientation.y);
    m.ori[2].store(b.orientation.z);
    m.ori[3].store(b.orientation.w);
    m.pos[0].store(b.position.x);
    m.pos[1].store(b.position.y);
    m.pos[2].store(b.position.z);
}
