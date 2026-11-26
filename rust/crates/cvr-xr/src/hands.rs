//! The hands' poses (formerly the pose half of the [HANDS] block in src/Runtimes/OpenXRFrameLoop.cpp): each hand's
//! grip located at the hands' instant (on the OpenXR Simulator turned up and rolled to a real controller's hold),
//! made relative to the head it is re-anchored on, the controller's own motion since the previous locate, and -- with
//! the gameplay actions -- where the controller points, as a yaw relative to the recenter base for hand-oriented
//! locomotion.
//!
//! The results land in OpenXRManager's own members (its hands, their motion, the aim-yaw filters, the yaw atomics),
//! under its hand lock, so every C++ reader is unchanged.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::cmath::{atan2, cos, sin, sqrt};
use cvr_body::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};
use cvr_host::log;
use openxr_sys as xr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use xr::Handle as _;

use crate::input::ControllerState;

/// XrPosef.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub orientation: Quat,
    pub position: Vec3,
}
impl Pose {
    fn from_xr(p: &xr::Posef) -> Pose {
        Pose {
            orientation: Quat { x: p.orientation.x, y: p.orientation.y, z: p.orientation.z, w: p.orientation.w },
            position: Vec3 { x: p.position.x, y: p.position.y, z: p.position.z },
        }
    }
}

/// The head of OpenXRHeadPose (include/Runtimes/OpenXRManager.hpp) that the loop writes.
#[repr(C)]
pub struct HandPose {
    pub pos: [f32; 3],
    pub ori: [f32; 4],
    pub valid: bool,
}

/// OpenXRManager::HandMotion: the change of the room-space pose since the previous locate.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HandMotion {
    pub linear: [f32; 3],
    pub angular: [f32; 3],
    pub valid: bool,
    pub prev_pose: Pose,
    pub prev_time: i64,
    pub origin: u64,
}
const _: () = assert!(core::mem::size_of::<HandMotion>() == 72);

/// OpenXRManager::TrackingAngleFilterState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AngleFilter {
    pub initialized: bool,
    pub angle_rad: f32,
}

/// This frame's handles and facts.
#[repr(C)]
pub struct HandsFrame {
    pub session: u64,
    pub local_space: u64,
    pub hand_pose_action: u64,
    pub hand_paths: [u64; 2],
    pub hand_spaces: [u64; 2],
    pub hand_aim_spaces: [u64; 2],
    /// The hands' locate instant.
    pub time: i64,
    /// The head the hands are made relative to (CyberpunkVR_HandRelToFilteredHead: the filtered one).
    pub head: Pose,
    /// The raw head at the hands' instant (the scanner's ear zone is measured from it).
    pub raw_head: Pose,
    /// The inverse of the recenter base's orientation.
    pub base_inv: Quat,
    /// The tracking origin's serial (a recenter moves the room under the hand: no motion across it).
    pub origin: u64,
    pub gameplay_active: bool,
    pub simulator: bool,
    /// This frame logs the hands (verbose, one frame in 120).
    pub log: bool,
}

/// OpenXRManager's members the hands land in.
#[repr(C)]
pub struct HandsOut {
    pub hands: [*mut HandPose; 2],
    pub motion: *mut HandMotion,
    pub aim_filter: *mut AngleFilter,
    /// std::atomic<float>[2], std::atomic<bool>[2].
    pub yaw_rel: *const AtomicU32,
    pub yaw_valid: *const AtomicBool,
    pub ctrl: *mut ControllerState,
    pub pose_valid: *mut bool,
    /// The physical left grip in the raw head's axes, [x, y, z] (left unchanged when it was not located).
    pub scanner_hand: *mut [f32; 3],
}

const K_PI: f32 = 3.1415926535;

fn normalize_angle(mut angle: f32) -> f32 {
    while angle > K_PI {
        angle -= 2.0 * K_PI;
    }
    while angle < -K_PI {
        angle += 2.0 * K_PI;
    }
    angle
}
fn smooth_step01(v: f32) -> f32 {
    let x = if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    };
    x * x * (3.0 - 2.0 * x)
}
/// The frame loop's adaptiveFollow: still, a fraction 1/(1+20 strength); moving past `release`, all of it.
pub fn adaptive_follow(strength: f32, delta: f32, quiet: f32, release: f32) -> f32 {
    if strength <= 0.001 || release <= quiet {
        return 1.0;
    }
    let still_follow = 1.0 / (1.0 + 20.0 * strength);
    let motion = smooth_step01((delta - quiet) / (release - quiet));
    still_follow + (1.0 - still_follow) * motion
}
/// The frame loop's filterTrackingAngle.
pub fn filter_angle(state: &mut AngleFilter, raw: f32, strength: f32, quiet: f32, release: f32) -> f32 {
    let raw = normalize_angle(raw);
    if !state.initialized || strength <= 0.001 {
        state.initialized = true;
        state.angle_rad = raw;
        return raw;
    }
    let delta = normalize_angle(raw - state.angle_rad);
    let t = adaptive_follow(strength, delta.abs(), quiet, release);
    state.angle_rad = normalize_angle(state.angle_rad + delta * t);
    state.angle_rad
}

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    /// The simulator's grip turned up about its own X by this many degrees (0: off).
    static CyberpunkVR_SimGripPitchDeg: f32;
    /// ...and rolled about its pointing axis per hand, before the pitch.
    static CyberpunkVR_SimGripRollDeg: [f32; 2];
    /// The shake census (rust/crates/cvr-rig/src/vrik/body.rs): stage 3 is the XR thread's own sample.
    fn VRIK_NoteShake(hand: i32, stage: i32, pos: *const f32);
}

/// OpenXRManager::ApplySimulatorGripFix: the OpenXR Simulator's controllers point along their grip -Z, a real
/// controller's grip runs through the fist; on the simulator only, the grip is rolled and turned up to match.
#[cfg(not(test))]
pub fn simulator_grip_fix(orientation: Quat, hand: i32, simulator: bool) -> Quat {
    if !simulator {
        return orientation;
    }
    // SAFETY: the C++ half's tuning globals, read as it reads them.
    let (roll, deg) = unsafe {
        (
            if hand == 0 || hand == 1 { core::ptr::read_volatile(&CyberpunkVR_SimGripRollDeg[hand as usize]) } else { 0.0 },
            core::ptr::read_volatile(&raw const CyberpunkVR_SimGripPitchDeg),
        )
    };
    let mut q = orientation;
    if roll != 0.0 {
        let h = roll * 0.5 * 3.14159265 / 180.0;
        q = multiply_quat(q, Quat { x: 0.0, y: 0.0, z: sin(h), w: cos(h) });
    }
    if deg == 0.0 {
        return q;
    }
    let h = deg * 0.5 * 3.14159265 / 180.0;
    multiply_quat(q, Quat { x: sin(h), y: 0.0, z: 0.0, w: cos(h) })
}
/// The fix for OpenXRManager's other locate of the grips (the per-frame body frame).
///
/// # Safety
/// `pose` is an XrPosef.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_simulator_grip_fix(pose: *mut Pose, hand: i32, simulator: bool) {
    // SAFETY: the caller's pose.
    if let Some(p) = unsafe { pose.as_mut() } {
        p.orientation = simulator_grip_fix(p.orientation, hand, simulator);
    }
}

/// The controller's own motion: the room-space change since the previous locate, on the XR thread's clock, in the
/// head's axes. Not the runtime's XrSpaceVelocity: the OpenXR Simulator flags its velocity valid and reports zero.
pub fn update_motion(m: &mut HandMotion, pose: &Pose, time: i64, origin: u64, head_inv: Quat) {
    if origin != m.origin {
        m.prev_time = 0;
    }
    m.origin = origin;
    let dt = if m.prev_time != 0 && time > m.prev_time { (time - m.prev_time) as f32 * 1e-9 } else { 0.0 };
    let (mut lin, mut ang) = (Vec3::default(), Vec3::default());
    let mut ok = false;
    if dt > 0.0 && dt < 0.1 {
        let p = &pose.position;
        let q = &m.prev_pose.position;
        lin = Vec3 { x: (p.x - q.x) / dt, y: (p.y - q.y) / dt, z: (p.z - q.z) / dt };
        // q = dq * prev: dq's axis-angle over dt, in the room's axes
        let mut dq = multiply_quat(pose.orientation, conjugate_quat(m.prev_pose.orientation));
        if dq.w < 0.0 {
            dq = Quat { x: -dq.x, y: -dq.y, z: -dq.z, w: -dq.w };
        }
        let s = sqrt(dq.x * dq.x + dq.y * dq.y + dq.z * dq.z);
        let rate = if s > 1e-7 { 2.0 * atan2(s, dq.w) / (s * dt) } else { 0.0 };
        ang = Vec3 { x: dq.x * rate, y: dq.y * rate, z: dq.z * rate };
        ok = true;
    }
    m.prev_pose = *pose;
    m.prev_time = time;
    m.valid = ok;
    if ok {
        let l = rotate_vector(head_inv, lin);
        let a = rotate_vector(head_inv, ang);
        m.linear = [l.x, l.y, l.z];
        m.angular = [a.x, a.y, a.z];
    }
}

/// The yaw of a rotation about the vertical (the HMD's GetHmdYawRelToBody convention).
pub fn yaw_of(q: Quat) -> f32 {
    atan2(2.0 * (q.w * q.y + q.x * q.z), 1.0 - 2.0 * (q.y * q.y + q.z * q.z))
}

#[cfg(not(test))]
extern "system" {
    fn xrGetActionStatePose(session: xr::Session, info: *const xr::ActionStateGetInfo, state: *mut xr::ActionStatePose) -> xr::Result;
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}

/// Both hands for this frame.
///
/// # Safety
/// The handles are live or 0; `out` points into OpenXRManager, whose hand lock the caller holds.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_locate_hands(f: *const HandsFrame, out: *const HandsOut) {
    // SAFETY (whole body): the caller's frame and members; the live session's actions and spaces.
    unsafe {
        let (Some(f), Some(o)) = (f.as_ref(), out.as_ref()) else { return };
        let session = xr::Session::from_raw(f.session);
        let local = xr::Space::from_raw(f.local_space);
        let time = xr::Time::from_nanos(f.time);
        let head_inv = conjugate_quat(f.head.orientation);
        for i in 0..2 {
            let info = xr::ActionStateGetInfo {
                ty: xr::StructureType::ACTION_STATE_GET_INFO,
                next: core::ptr::null(),
                action: xr::Action::from_raw(f.hand_pose_action),
                subaction_path: xr::Path::from_raw(f.hand_paths[i]),
            };
            let mut state = xr::ActionStatePose { ty: xr::StructureType::ACTION_STATE_POSE, next: core::ptr::null_mut(), is_active: xr::FALSE };
            let pose_res = xrGetActionStatePose(session, &info, &mut state);
            if f.log {
                log!("OpenXRManager[Hands]: eye={} poseRes={} isActive={}\n", i, pose_res.into_raw(), state.is_active.into_raw());
            }
            let hand = &mut *o.hands[i];
            let motion = &mut *o.motion.add(i);
            hand.valid = false;
            let mut pose_valid = false;
            if state.is_active.into_raw() != 0 {
                let mut loc: xr::SpaceLocation = core::mem::zeroed();
                loc.ty = xr::StructureType::SPACE_LOCATION;
                let loc_res = xrLocateSpace(xr::Space::from_raw(f.hand_spaces[i]), local, time, &mut loc);
                let mut pose = Pose::from_xr(&loc.pose);
                if loc_res.into_raw() >= 0 {
                    pose.orientation = simulator_grip_fix(pose.orientation, i as i32, f.simulator);
                }
                if f.log {
                    log!("OpenXRManager[Hands]: eye={} locRes={} flags=0x{:X}\n", i, loc_res.into_raw(), loc.location_flags.into_raw());
                }
                let need = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
                if loc_res.into_raw() >= 0 && loc.location_flags.contains(need) {
                    // Relative to the head it is re-anchored on, at the instant it was located.
                    let world = Vec3 {
                        x: pose.position.x - f.head.position.x,
                        y: pose.position.y - f.head.position.y,
                        z: pose.position.z - f.head.position.z,
                    };
                    let rel_pos = rotate_vector(head_inv, world);
                    let rel_ori = multiply_quat(head_inv, pose.orientation);
                    // The shake census, stage 3: the head-relative position on the XR thread's uniform clock.
                    let raw = [rel_pos.x, rel_pos.y, rel_pos.z];
                    VRIK_NoteShake(i as i32, 3, raw.as_ptr());
                    // RAW: the one filter that remains is where the arms read the pose (FlushHandsToShared).
                    hand.pos = raw;
                    hand.ori = [rel_ori.x, rel_ori.y, rel_ori.z, rel_ori.w];
                    hand.valid = true;
                    pose_valid = true;
                    if i == 0 && !o.scanner_hand.is_null() {
                        // The scanner's geometry: the raw head and the physical left grip at this same instant.
                        let r = &f.raw_head;
                        let s = rotate_vector(
                            cvr_body::xr::conjugate_quat(r.orientation),
                            Vec3 { x: pose.position.x - r.position.x, y: pose.position.y - r.position.y, z: pose.position.z - r.position.z },
                        );
                        *o.scanner_hand = [s.x, s.y, s.z];
                    }
                    update_motion(motion, &pose, f.time, f.origin, head_inv);
                }
            }
            if !pose_valid {
                motion.valid = false;
                motion.prev_time = 0;
            }
            *o.pose_valid.add(i) = pose_valid;
            if !f.gameplay_active {
                continue;
            }
            if let Some(ctrl) = o.ctrl.as_mut() {
                if i == 0 {
                    ctrl.left_hand_valid = pose_valid;
                } else {
                    ctrl.right_hand_valid = pose_valid;
                }
            }
            // Where the controller POINTS (the aim pose), not where the palm faces: the grip's -Z is mirrored between
            // the hands. Its yaw relative to the recenter base is hand-oriented locomotion's direction.
            let filter = &mut *o.aim_filter.add(i);
            let mut aim_yaw_valid = false;
            if pose_valid && f.hand_aim_spaces[i] != 0 {
                let mut aim: xr::SpaceLocation = core::mem::zeroed();
                aim.ty = xr::StructureType::SPACE_LOCATION;
                let res = xrLocateSpace(xr::Space::from_raw(f.hand_aim_spaces[i]), local, time, &mut aim);
                if res.into_raw() >= 0 && aim.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID) {
                    let q = multiply_quat(f.base_inv, Pose::from_xr(&aim.pose).orientation);
                    // LOCOMOTION, not rendering: stick feel against direction wander.
                    const AIM_YAW_SMOOTH: f32 = 0.20;
                    let yaw = filter_angle(filter, yaw_of(q), AIM_YAW_SMOOTH, 0.0040, 0.0800);
                    (*o.yaw_rel.add(i)).store(yaw.to_bits(), Ordering::Relaxed);
                    (*o.yaw_valid.add(i)).store(true, Ordering::Relaxed);
                    aim_yaw_valid = true;
                }
            }
            if !aim_yaw_valid {
                filter.initialized = false;
                (*o.yaw_valid.add(i)).store(false, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_angle_filter_holds_still_and_follows_motion() {
        let mut f = AngleFilter::default();
        assert_eq!(filter_angle(&mut f, 0.5, 0.2, 0.004, 0.08), 0.5, "the first sample seeds it");
        // a tiny jitter is mostly held: 1/(1+4) of it
        let held = filter_angle(&mut f, 0.502, 0.2, 0.004, 0.08);
        assert!((held - 0.5004).abs() < 1e-6, "{held}");
        // a turn past release is followed whole
        assert!((filter_angle(&mut f, 1.0, 0.2, 0.004, 0.08) - 1.0).abs() < 1e-6);
        // across the seam the shorter way, wrapped
        let mut f = AngleFilter { initialized: true, angle_rad: 3.1 };
        let wrapped = filter_angle(&mut f, -3.1, 0.2, 0.004, 0.08);
        assert!(wrapped < -3.0 && wrapped > -K_PI, "{wrapped}");
    }
    #[test]
    fn motion_needs_two_locates_and_resets_across_a_recenter() {
        let mut m = HandMotion::default();
        let id = Quat::IDENTITY;
        let p0 = Pose { orientation: id, position: Vec3 { x: 0.0, y: 0.0, z: 0.0 } };
        let p1 = Pose { orientation: id, position: Vec3 { x: 0.1, y: 0.0, z: 0.0 } };
        update_motion(&mut m, &p0, 1_000_000_000, 7, id);
        assert!(!m.valid);
        update_motion(&mut m, &p1, 1_010_000_000, 7, id);
        assert!(m.valid);
        assert!((m.linear[0] - 10.0).abs() < 1e-3, "{:?}", m.linear);
        update_motion(&mut m, &p0, 1_020_000_000, 8, id);
        assert!(!m.valid, "a recentre moves the room under the hand");
    }
}
