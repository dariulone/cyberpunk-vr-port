//! The hands published to the VRIK solve, whole (formerly OpenXRManager::FlushHandsToShared and the getters of what it
//! published), and the external pose reset's gate.
//!
//! Once per frame -- from the native frame with its own head, or from Present with the XR thread's -- the hands are
//! made relative to ONE head reference (located for this frame's instant, or the XR thread's latest), the body's raw
//! tracking frame is taken beside them, the filters run (crate::publish), and the packet goes into the shared block
//! under its seqlock (crate::publish). What was published is kept, under the hand lock, for the gestures, the camera
//! and the body (GetNativeFrameHead, GetPublishedHandFrame, GetGestureHandFrame, GetBodyTrackingFrame).
//!
//! The state is owned here; in the Rust build OpenXRManager's members are references to it.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::tracked_yaw::TrackingFrame;
use cvr_body::vec2::Vec2;
use cvr_body::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};
use cvr_camera::anchor::{compose_view_translation, AnchorRecipe, AnchorVector};
use cvr_host::log;
use openxr_sys as xr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use xr::Handle as _;

use crate::calibration::AtomicF32;
use crate::hands::{HandPose, Pose};
use crate::srw::{Guarded, SrwLock};
use crate::tracking::{now_us, HeadPose, HANDS};

/// cvr::roomscale::TrackingResetGate: odd while a reset runs; a pose located at the current even serial is after it.
#[repr(C)]
pub struct ResetGate {
    pub serial: AtomicU64,
    pub applied: AtomicU64,
}
/// CyberpunkVR_BindPoseToImage while no external pose reset is pending (the reset asked only when binding), as the
/// capture binds a frame's pose to its image.
pub fn bind_pose_to_image(bind_export: i32, reset_pending: impl FnOnce() -> bool) -> bool {
    bind_export != 0 && !reset_pending()
}
impl ResetGate {
    pub fn serial(&self) -> u64 {
        self.serial.load(Ordering::Acquire)
    }
    pub fn pending(&self) -> bool {
        self.serial() != self.applied.load(Ordering::Acquire)
    }
}
/// m_externalResetMutex and m_externalPoseReset.
#[repr(C)]
pub struct ResetShared {
    pub lock: SrwLock,
    pub gate: ResetGate,
}
pub static RESET: ResetShared = ResetShared { lock: SrwLock::new(), gate: ResetGate { serial: AtomicU64::new(0), applied: AtomicU64::new(0) } };
#[no_mangle]
pub extern "C" fn cvr_xr_reset_shared() -> *const ResetShared {
    &RESET
}
/// OpenXRManager::ExternalPoseResetPending.
pub fn external_reset_pending() -> bool {
    RESET.gate.pending()
}
/// OpenXRManager::BeginExternalPoseReset: a runtime reset is in progress (the serial goes odd).
#[no_mangle]
pub extern "C" fn cvr_xr_begin_external_pose_reset() {
    let _g = RESET.lock.lock();
    RESET.gate.serial.fetch_add(1, Ordering::AcqRel);
}
/// OpenXRManager::EndExternalPoseReset: it is done (even again); the next base is captured before a pose is published.
#[no_mangle]
pub extern "C" fn cvr_xr_end_external_pose_reset() {
    {
        let _g = RESET.lock.lock();
        RESET.gate.serial.fetch_add(1, Ordering::Release);
    }
    log!("OpenXRManager: runtime Reset View completed; rebase before publishing another pose.\n");
}

/// cvr::tracking::HandPublicationGate: the native tick owns on-foot hands while its publication is fresh.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PublicationGate {
    pub stamp: u64,
    pub origin: u64,
}
impl PublicationGate {
    pub const MAX_AGE_US: u64 = 250000;
    pub fn native_owns(&self, now: u64, origin: u64, eligible: bool) -> bool {
        eligible && self.stamp != 0 && self.origin == origin && now >= self.stamp && now - self.stamp <= Self::MAX_AGE_US
    }
}

/// What the publication keeps (OpenXRManager's m_handSample*, m_handPublicationGate, m_published*, m_bodyTrackingFrame
/// and m_sharedSeq), all under the hand lock.
#[repr(C)]
pub struct PublishedData {
    /// The head taken in the same locked step as the XR thread's controller poses.
    pub sample_head_pos: [f32; 3],
    pub sample_head_valid: bool,
    pub sample_origin: u64,
    pub gate: PublicationGate,
    pub head: HeadPose,
    pub hands: [HeadPose; 2],
    /// The same sample before the weapon hand's offsets, and the ordinary melee's before the visual filter.
    pub gesture: [HeadPose; 2],
    pub raw_melee: [HeadPose; 2],
    pub sequence: u64,
    pub stamp_us: u64,
    pub aim: i64,
    pub consumed: Vec2,
    pub body_frame: TrackingFrame,
    pub shared_seq: u32,
}
/// OpenXRManager's publication members: the sample's stamp (an atomic of its own) and the rest under the hand lock.
#[repr(C)]
pub struct PublishedShared {
    pub sample_ms: AtomicF32,
    pub data: Guarded<PublishedData>,
}
const _: () = assert!(core::mem::offset_of!(PublishedData, head) == 40 && core::mem::offset_of!(PublishedData, body_frame) == 520);
const _: () = assert!(core::mem::size_of::<PublishedData>() == 640 && core::mem::size_of::<PublishedShared>() == 648);
const NO_FRAME: TrackingFrame = TrackingFrame {
    head: Quat::IDENTITY,
    head_position: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
    hands: [Vec3 { x: 0.0, y: 0.0, z: 0.0 }; 2],
    rotations: [Quat::IDENTITY; 2],
    valid: false,
    tracked: [false; 2],
    sequence: 0,
    stamp_us: 0,
    origin: 0,
};
pub static PUBLISHED: PublishedShared = PublishedShared {
    sample_ms: AtomicF32::zero(),
    data: Guarded::new(PublishedData {
        sample_head_pos: [0.0; 3],
        sample_head_valid: false,
        sample_origin: 0,
        gate: PublicationGate { stamp: 0, origin: 0 },
        head: HeadPose::EMPTY,
        hands: [HeadPose::EMPTY; 2],
        gesture: [HeadPose::EMPTY; 2],
        raw_melee: [HeadPose::EMPTY; 2],
        sequence: 0,
        stamp_us: 0,
        aim: 0,
        consumed: Vec2 { x: 0.0, y: 0.0 },
        body_frame: NO_FRAME,
        shared_seq: 0,
    }),
};
#[no_mangle]
pub extern "C" fn cvr_xr_published_shared() -> *const PublishedShared {
    &PUBLISHED
}

/// What one publication reads besides the heads (the session's and its spaces' handles are read in crate::frame and
/// crate::session): the Present path's moving prediction (GetFrameAimTimeNow, taken only without a frame head), the
/// live controls' body rotation mode and whether frame generation runs.
pub struct FlushContext {
    pub aim_now: i64,
    /// g_liveControls.xrBodyRotationMode.
    pub body_rotation_mode: i32,
    pub framegen: bool,
}
impl FlushContext {
    pub fn of(frame_head: bool, aim_now: impl FnOnce() -> i64, body_rotation_mode: i32, framegen: bool) -> FlushContext {
        FlushContext { aim_now: if frame_head { 0 } else { aim_now() }, body_rotation_mode, framegen }
    }
}

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_HandLocatePerFrame: i32;
    static CyberpunkVR_HeadTranslationInPatch: i32;
    static CyberpunkVR_CoherentHandAnchor: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugHandPerFrameLocates: u64;
    static CyberpunkVR_NativeHandPublishes: AtomicU64;
    static CyberpunkVR_PresentHandPublishSkipped: AtomicU64;
    static CyberpunkVR_FrameHandReferenceRejected: AtomicU64;
    /// rust/crates/cvr-rig/src/vrik/roomscale_move.rs.
    fn cvr_roomscale_pose_frame_allowed() -> bool;
    fn cvr_roomscale_camera_consumed_at(origin: u64) -> Vec2;
    /// rust/crates/cvr-rig/src/vrik/camera_link.rs.
    fn cvr_camera_anchor_recipe_read(recipe: *mut AnchorRecipe) -> bool;
    /// rust/crates/cvr-ffi/src/body.rs.
    fn cvr_needs_body_tracking(mode: i32) -> bool;
}
#[cfg(not(test))]
extern "system" {
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}

#[cfg(not(test))]
fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 }
}

/// The 1 KiB block the VRIK plugin reads (CyberpunkVR_Hands_Shared), mapped once.
#[cfg(not(test))]
pub fn shared_block() -> *mut f32 {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Memory::{CreateFileMappingA, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE};
    static BLOCK: AtomicPtr<f32> = AtomicPtr::new(core::ptr::null_mut());
    let p = BLOCK.load(Ordering::Acquire);
    if !p.is_null() {
        return p;
    }
    // SAFETY: a named page-file mapping of 1 KiB; the view lives as long as the process.
    unsafe {
        let mapping = CreateFileMappingA(INVALID_HANDLE_VALUE, core::ptr::null(), PAGE_READWRITE, 0, 1024, c"CyberpunkVR_Hands_Shared".as_ptr().cast());
        if mapping.is_null() {
            return core::ptr::null_mut();
        }
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 1024).Value.cast::<f32>();
        if view.is_null() {
            return core::ptr::null_mut();
        }
        match BLOCK.compare_exchange(core::ptr::null_mut(), view, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => view,
            Err(other) => other,
        }
    }
}

/// include/Utils/XrMath.hpp RelativePose: `pose` in `reference`'s frame.
pub fn relative_pose(reference: &Pose, pose: &Pose) -> Pose {
    let inv = conjugate_quat(reference.orientation);
    let d = Vec3 {
        x: pose.position.x - reference.position.x,
        y: pose.position.y - reference.position.y,
        z: pose.position.z - reference.position.z,
    };
    Pose { orientation: multiply_quat(inv, pose.orientation), position: rotate_vector(inv, d) }
}
fn pose_of(p: &xr::Posef) -> Pose {
    Pose {
        orientation: Quat { x: p.orientation.x, y: p.orientation.y, z: p.orientation.z, w: p.orientation.w },
        position: Vec3 { x: p.position.x, y: p.position.y, z: p.position.z },
    }
}

/// OpenXRManager::GetCoherentViewAnchor with a reference head: the view anchor built from this head's position and the
/// camera's recipe, the roomscale already consumed taken off.
#[cfg(not(test))]
fn coherent_view_anchor(reference: &HeadPose, consumed: Vec2) -> Option<[f32; 3]> {
    let mut anchor = AnchorRecipe::default();
    // SAFETY: the exported switch; the camera link's recipe read into a local.
    let ok = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_CoherentHandAnchor)) != 0 }
        && reference.valid
        && unsafe { cvr_camera_anchor_recipe_read(&mut anchor) };
    if !ok {
        return None;
    }
    let sc = anchor.scale;
    let [x, y, z] = reference.pos;
    let residual = AnchorVector { x: (x - consumed.x) * sc, y: (-z - consumed.y) * sc, z: y * sc };
    let d = compose_view_translation(residual, &anchor, anchor.tracking_yaw, anchor.model_yaw);
    Some([d.x, d.y, d.z])
}

/// OpenXRManager::FlushHandsToShared. `frame_head` is the native frame's head (with `frame_time` and the roomscale it
/// consumed), or null for Present's publication from the XR thread's head.
///
/// # Safety
/// The pointers are the caller's (null where optional).
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_flush_hands(frame_head: *const HeadPose, frame_time: i64, frame_consumed: *const Vec2) {
    let ctx = &FlushContext::of(
        !frame_head.is_null(),
        || crate::frame::cvr_xr_frame_aim_time_now(),
        cvr_settings::lc!(xrBodyRotationMode),
        cvr_gpu::framegen_core::cvr_framegen_enabled(),
    );
    // SAFETY (whole body): the caller's heads; the shared block mapped above; the runtime's spaces.
    unsafe {
        let frame_head = frame_head.as_ref();
        let hand_guard = HANDS.lock.lock();
        let block = shared_block();
        if block.is_null() {
            return;
        }
        let published = PUBLISHED.data.get(&hand_guard);
        let publish_us = now_us();
        let origin = crate::tracking::origin_serial();
        if frame_head.is_none() && published.gate.native_owns(publish_us, origin, cvr_roomscale_pose_frame_allowed()) {
            if diagnostics() {
                CyberpunkVR_PresentHandPublishSkipped.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        let mut reference = HeadPose::EMPTY;
        match frame_head {
            Some(h) => reference = *h,
            None => {
                crate::tracking::cvr_xr_get_head_pose(&mut reference);
            }
        }
        if !reference.valid || reference.origin_serial != origin || (frame_head.is_some() && frame_time <= 0) || external_reset_pending() {
            if diagnostics() {
                CyberpunkVR_FrameHandReferenceRejected.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        let consumed = match frame_consumed.as_ref() {
            Some(c) => *c,
            None => cvr_roomscale_camera_consumed_at(reference.origin_serial),
        };
        let Some(base) = crate::tracking::base_for(reference.origin_serial) else { return };

        // ONE head reference for localisation, filtering and publication.
        let ref_ori = Quat { x: reference.ori[0], y: reference.ori[1], z: reference.ori[2], w: reference.ori[3] };
        let hp = rotate_vector(base.orientation, Vec3 { x: reference.pos[0], y: reference.pos[1], z: reference.pos[2] });
        let head_anchor = Pose {
            orientation: multiply_quat(base.orientation, ref_ori),
            position: Vec3 { x: base.position.x + hp.x, y: base.position.y + hp.y, z: base.position.z + hp.z },
        };
        let mut src = *HANDS.hands.get(&hand_guard);
        // Cached offsets belong to the XR thread's reference: a native frame gets fresh controllers or none.
        if frame_head.is_some() {
            src[0].valid = false;
            src[1].valid = false;
        }
        let mut body_frame = NO_FRAME;
        let per_frame = frame_head.is_some() || core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HandLocatePerFrame)) != 0;
        let (session, local_space, view_space) = crate::frame::session_handles();
        if per_frame && session != 0 && local_space != 0 && crate::events::session_running() {
            let aim = if frame_head.is_some() { frame_time } else { ctx.aim_now };
            if aim > 0 {
                let hand_spaces = crate::session::HAND_SPACES.lock().unwrap_or_else(|p| p.into_inner()).0;
                let local = xr::Space::from_raw(local_space);
                let time = xr::Time::from_nanos(aim);
                let tracked = xr::SpaceLocationFlags::POSITION_TRACKED | xr::SpaceLocationFlags::ORIENTATION_TRACKED;
                let valid = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
                let mut body_head = Pose::default();
                if cvr_needs_body_tracking(ctx.body_rotation_mode) && frame_head.is_some() {
                    // Raw head tracking at the SAME aim time as the raw controllers.
                    let mut loc: xr::SpaceLocation = core::mem::zeroed();
                    loc.ty = xr::StructureType::SPACE_LOCATION;
                    let res = xrLocateSpace(xr::Space::from_raw(view_space), local, time, &mut loc);
                    body_frame.valid = res.into_raw() >= 0 && loc.location_flags.contains(valid | tracked);
                    body_head = pose_of(&loc.pose);
                    body_frame.origin = reference.origin_serial;
                    let inv = conjugate_quat(base.orientation);
                    body_frame.head = multiply_quat(inv, body_head.orientation);
                    body_frame.head_position = rotate_vector(
                        inv,
                        Vec3 {
                            x: body_head.position.x - base.position.x,
                            y: body_head.position.y - base.position.y,
                            z: body_head.position.z - base.position.z,
                        },
                    );
                }
                for h in 0..2 {
                    if hand_spaces[h] == 0 {
                        continue;
                    }
                    let mut loc: xr::SpaceLocation = core::mem::zeroed();
                    loc.ty = xr::StructureType::SPACE_LOCATION;
                    if xrLocateSpace(xr::Space::from_raw(hand_spaces[h]), local, time, &mut loc).into_raw() < 0 {
                        continue;
                    }
                    let mut pose = pose_of(&loc.pose);
                    pose.orientation = crate::hands::simulator_grip_fix(pose.orientation, h as i32, crate::runtime_info::RUNTIME.simulator());
                    if !loc.location_flags.contains(valid) {
                        continue;
                    }
                    if body_frame.valid {
                        let raw = relative_pose(&body_head, &pose);
                        body_frame.hands[h] = raw.position;
                        body_frame.rotations[h] = raw.orientation;
                        body_frame.tracked[h] = loc.location_flags.contains(tracked);
                    }
                    let local_pose = relative_pose(&head_anchor, &pose);
                    let s = &mut src[h];
                    s.valid = true;
                    s.pos = [local_pose.position.x, local_pose.position.y, local_pose.position.z];
                    s.ori = [local_pose.orientation.x, local_pose.orientation.y, local_pose.orientation.z, local_pose.orientation.w];
                    if diagnostics() {
                        let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugHandPerFrameLocates);
                        core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
                    }
                }
            }
        }
        // Ordinary melee measures the grip's own speed before the visual smoothing.
        let raw_melee = src;
        let mut pair = [HandPose { pos: src[0].pos, ori: src[0].ori, valid: src[0].valid }, HandPose { pos: src[1].pos, ori: src[1].ori, valid: src[1].valid }];
        crate::publish::filter_hands(&mut pair, &head_anchor, reference.origin_serial, frame_head.is_some(), ctx.framegen, now_us());
        for h in 0..2 {
            src[h].pos = pair[h].pos;
            src[h].ori = pair[h].ori;
        }
        if reference.origin_serial != crate::tracking::origin_serial() || external_reset_pending() {
            return;
        }

        // The packet, under its seqlock.
        let mut packet = crate::publish::Packet {
            weapon_euler: crate::weapon::WEAPON.euler(),
            weapon_offset: crate::weapon::WEAPON.offset(),
            anchor_mode: 0,
            anchor: [0.0; 3],
            stamp: 0.0,
            head_ori: reference.ori,
            head_pos: reference.pos,
        };
        if frame_head.is_some() && core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HeadTranslationInPatch)) != 0 {
            packet.anchor_mode = 2;
            packet.anchor = [consumed.x, consumed.y, reference.body_bend.angle];
        } else if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HeadTranslationInPatch)) == 0 {
            if let Some(a) = coherent_view_anchor(&reference, consumed) {
                packet.anchor_mode = 1;
                packet.anchor = a;
            }
        }
        packet.stamp = if frame_head.is_some() { (crate::tracking::now_ms() % 100000.0) as f32 } else { PUBLISHED.sample_ms.load() };
        let mut right = [0.0f32; 7];
        let end = crate::publish::cvr_xr_write_hand_packet(block, &pair[0], &pair[1], &packet, &mut published.shared_seq, &mut right);

        published.head = reference;
        published.gesture = [src[0], src[1]];
        published.raw_melee = raw_melee;
        published.hands = [src[0], src[1]];
        published.hands[1].pos = [right[0], right[1], right[2]];
        published.hands[1].ori = [right[3], right[4], right[5], right[6]];
        published.sequence = end as u64;
        published.stamp_us = now_us();
        body_frame.sequence = published.sequence;
        body_frame.stamp_us = published.stamp_us;
        published.body_frame = body_frame;
        published.aim = if frame_head.is_some() { frame_time } else { 0 };
        published.consumed = consumed;
        if frame_head.is_some() {
            published.gate.stamp = now_us();
            published.gate.origin = reference.origin_serial;
            if diagnostics() {
                let count = CyberpunkVR_NativeHandPublishes.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 300 == 1 {
                    log!(
                        "HandFrame: native={} presentSkipped={} rejected={} filter=exponential\n",
                        count,
                        CyberpunkVR_PresentHandPublishSkipped.load(Ordering::Relaxed),
                        CyberpunkVR_FrameHandReferenceRejected.load(Ordering::Relaxed)
                    );
                }
            }
        }
    }
}

/// OpenXRManager::GetNativeFrameHead: the head published with the native frame's hands, while the native tick owns
/// them (the sequence marked with bit 63).
///
/// # Safety
/// `out` is an OpenXRHeadPose; the others are null or valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_native_frame_head(out: *mut HeadPose, sequence: *mut u64, stamp_us: *mut u64, aim: *mut i64, consumed: *mut Vec2) -> bool {
    // SAFETY: the caller's outputs.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        if !cvr_roomscale_pose_frame_allowed() || external_reset_pending() {
            return false;
        }
        let origin = crate::tracking::origin_serial();
        let guard = HANDS.lock.lock();
        let p = PUBLISHED.data.get(&guard);
        if !p.gate.native_owns(now_us(), origin, true) || !p.head.valid || p.head.origin_serial != origin || p.aim <= 0 {
            return false;
        }
        *out = p.head;
        if let Some(s) = sequence.as_mut() {
            *s = p.sequence | (1u64 << 63);
        }
        if let Some(s) = stamp_us.as_mut() {
            *s = p.stamp_us;
        }
        if let Some(a) = aim.as_mut() {
            *a = p.aim;
        }
        if let Some(c) = consumed.as_mut() {
            *c = p.consumed;
        }
        true
    }
}

/// The publication fresh enough to use (under a quarter second, this origin): its head and hands, as GetPublishedHandFrame
/// (`which` 0: as published, with the weapon offset) and GetGestureHandFrame (1: before the offsets, 2: the raw melee
/// hands, with the sequence and stamp) read them.
///
/// # Safety
/// `head` is an OpenXRHeadPose, `hands` two of them; `sequence` and `stamp_us` are null or valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_published_hand_frame(head: *mut HeadPose, hands: *mut HeadPose, which: i32, sequence: *mut u64, stamp_us: *mut u64) -> bool {
    if head.is_null() || hands.is_null() || external_reset_pending() {
        return false;
    }
    let origin = crate::tracking::origin_serial();
    let guard = HANDS.lock.lock();
    let p = PUBLISHED.data.get(&guard);
    let now = now_us();
    if !p.head.valid || p.head.origin_serial != origin || p.stamp_us == 0 || now < p.stamp_us || now - p.stamp_us > 250000 {
        return false;
    }
    let pair = match which {
        1 => p.gesture,
        2 => p.raw_melee,
        _ => p.hands,
    };
    // SAFETY: the caller's outputs.
    unsafe {
        *head = p.head;
        *hands = pair[0];
        *hands.add(1) = pair[1];
        if let Some(s) = sequence.as_mut() {
            *s = p.sequence;
        }
        if let Some(s) = stamp_us.as_mut() {
            *s = p.stamp_us;
        }
    }
    true
}


/// The settings Present publishes beside the calibration.
#[repr(C)]
pub struct PresentSlots {
    pub hand_tracking_mode: i32,
    pub weapon_aim_enable: i32,
    pub immersive_holsters: i32,
    pub body_hands_only: i32,
    /// GetCameraOffset: the neck mount plus the stored camera trim.
    pub camera_offset: [f32; 3],
    pub log_diag_request: i32,
    /// 1 = the reload highlight on; [174] is 1 when it is OFF (inverted like [23]: the zeroed block means on).
    pub reload_glow: i32,
}
/// The shared slots refreshed each Present (formerly the head of OpenXRManager::OnPresent), under the hand lock: the
/// hand-tracking mode [32], weapon aim [58], holsters [23], hands-only body [173], the muzzle mode [59], the shoulders
/// [70..76], the measured anatomy [77..80], the camera trim [91..93], the hand calibration [33..47] and the diagnostic
/// request [48]. Returns the block (OpenXRManager keeps it for GetSharedSlot), or null.
///
/// # Safety
/// `s` is valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_publish_present_slots(s: *const PresentSlots) -> *mut f32 {
    let block = shared_block();
    // SAFETY: the caller's settings.
    let Some(s) = (unsafe { s.as_ref() }) else { return block };
    if block.is_null() {
        return block;
    }
    let calib = crate::calibration::cvr_xr_calibration();
    let _guard = HANDS.lock.lock();
    // SAFETY: the mapped 256-float block; the Rust-owned calibration; volatile stores as the C++ wrote them.
    unsafe {
        let c = &*calib;
        let slot = |i: usize, v: f32| core::ptr::write_volatile(block.add(i), v);
        slot(32, s.hand_tracking_mode as f32);
        slot(58, s.weapon_aim_enable as f32);
        slot(23, if s.immersive_holsters != 0 { 0.0 } else { 1.0 });
        slot(173, if s.body_hands_only != 0 { 1.0 } else { 0.0 });
        slot(174, if s.reload_glow != 0 { 0.0 } else { 1.0 });
        slot(59, 5.0); // the game's muzzle transform
        if c.calib_ext_valid.load(Ordering::Relaxed) != 0 {
            for i in 0..6 {
                slot(70 + i, c.calib_ext[i].load());
            }
            slot(76, 1.0);
        }
        if c.measure_valid.load(Ordering::Relaxed) != 0 {
            slot(77, c.user_arm_len_r.load());
            slot(78, c.user_arm_len_l.load());
            slot(79, c.user_eye_height.load());
            slot(80, 1.0);
        }
        slot(91, s.camera_offset[0]);
        slot(92, s.camera_offset[1]);
        slot(93, s.camera_offset[2]);
        slot(33, c.calib_valid.load(Ordering::Relaxed) as f32);
        for i in 0..14 {
            slot(34 + i, c.calib[i].load());
        }
        slot(48, s.log_diag_request as f32);
    }
    block
}

#[cfg(test)]
mod flush_context_tests {
    //! The aim is the Present path's only: a frame head brings its own time.
    use super::FlushContext;

    #[test]
    fn the_aim_only_without_a_frame_head() {
        let with = FlushContext::of(true, || panic!("the aim is not read with a frame head"), 2, true);
        assert_eq!((with.aim_now, with.body_rotation_mode, with.framegen), (0, 2, true));
        let without = FlushContext::of(false, || 5_000, 0, false);
        assert_eq!((without.aim_now, without.body_rotation_mode, without.framegen), (5_000, 0, false));
    }

    #[test]
    fn the_pose_bound_only_without_a_pending_reset() {
        assert!(!super::bind_pose_to_image(0, || panic!("the reset is not asked when unbound")));
        assert!(super::bind_pose_to_image(1, || false));
        assert!(!super::bind_pose_to_image(1, || true));
    }
}
