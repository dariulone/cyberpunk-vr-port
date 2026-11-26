//! The head's pose on the XR thread (formerly the head half of OpenXRFrameLoop.cpp's located-head block): the recenter
//! base captured yaw-only (the runtime's own recenter semantics: around gravity, never the head's pitch or roll), and
//! the head filters -- none by default (a VR view must be raw), the adaptive one (xr_hmd_smooth), or mode 3, the same
//! fixed time-constant lerp the view path uses, so the view and the hands lag alike.
//!
//! The locks, the members the results land in and the publications stay with OpenXRManager for now.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::cmath::{acos, atan2, cos, exp, sin, sqrt};
use cvr_body::xr::{nlerp_quat, rotate_vector, Quat, Vec3};
use cvr_host::log;
use std::sync::Mutex;

use crate::hands::{adaptive_follow, Pose};

/// The recenter base from the head: its position, and its heading alone. Looking straight up or down, the head's up
/// vector gives the heading instead. Returns the base and its yaw (radians).
pub fn yaw_only_base(head: &Pose) -> (Pose, f32) {
    let q = head.orientation;
    let fwd = rotate_vector(q, Vec3 { x: 0.0, y: 0.0, z: -1.0 });
    let (mut hx, mut hz) = (fwd.x, fwd.z);
    if hx * hx + hz * hz < 1e-6 {
        let up = rotate_vector(q, Vec3 { x: 0.0, y: 1.0, z: 0.0 });
        hx = if fwd.y < 0.0 { up.x } else { -up.x };
        hz = if fwd.y < 0.0 { up.z } else { -up.z };
    }
    let yaw = atan2(-hx, -hz);
    let base = Pose { orientation: Quat { x: 0.0, y: sin(yaw * 0.5), z: 0.0, w: cos(yaw * 0.5) }, position: head.position };
    (base, yaw)
}

/// The base captured (a recenter, the first pose, an external reset), logged as OpenXRManager logged it.
///
/// # Safety
/// `head` and `out` are XrPosef.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_base(head: *const Pose, out: *mut Pose) {
    // SAFETY: the caller's poses.
    let (Some(head), Some(out)) = (unsafe { head.as_ref() }, unsafe { out.as_mut() }) else { return };
    let (base, yaw) = yaw_only_base(head);
    *out = base;
    // up.y: about +1 for a level space, about -1 for one upside down (the tracking space in InitGraphics).
    let up = rotate_vector(head.orientation, Vec3 { x: 0.0, y: 1.0, z: 0.0 });
    log!(
        "OpenXRManager: Base pose captured (yaw-only, {:.1} deg; head up.y {:.2}, y {:.2} m).\n",
        yaw * 57.29578,
        up.y,
        head.position.y
    );
}

/// OpenXRManager::TrackingPoseFilterState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseFilter {
    pub initialized: bool,
    pub position: Vec3,
    pub orientation: Quat,
}
const _: () = assert!(core::mem::size_of::<PoseFilter>() == 32);
impl PoseFilter {
    /// `TrackingPoseFilterState{}`: all zeros.
    pub const NEW: PoseFilter =
        PoseFilter { initialized: false, position: Vec3 { x: 0.0, y: 0.0, z: 0.0 }, orientation: Quat { x: 0.0, y: 0.0, z: 0.0, w: 0.0 } };
}

fn quat_angle(a: Quat, b: Quat) -> f32 {
    let mut dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
    if dot < 0.0 {
        dot = -dot;
    }
    if dot > 1.0 {
        dot = 1.0;
    }
    2.0 * acos(dot)
}

/// The frame loop's filterTrackingPose: position and orientation each follow by adaptiveFollow of their own delta.
pub fn filter_pose(s: &mut PoseFilter, raw: &Pose, strength: f32, quiet_pos: f32, release_pos: f32, quiet_ang: f32, release_ang: f32) -> Pose {
    if !s.initialized || strength <= 0.001 {
        s.initialized = true;
        s.position = raw.position;
        s.orientation = raw.orientation;
        return *raw;
    }
    let dx = raw.position.x - s.position.x;
    let dy = raw.position.y - s.position.y;
    let dz = raw.position.z - s.position.z;
    let pos_delta = sqrt(dx * dx + dy * dy + dz * dz);
    let ang_delta = quat_angle(s.orientation, raw.orientation);
    let pos_t = adaptive_follow(strength, pos_delta, quiet_pos, release_pos);
    let ang_t = adaptive_follow(strength, ang_delta, quiet_ang, release_ang);
    s.position.x += dx * pos_t;
    s.position.y += dy * pos_t;
    s.position.z += dz * pos_t;
    s.orientation = nlerp_quat(s.orientation, raw.orientation, ang_t);
    Pose { orientation: s.orientation, position: s.position }
}

/// Mode 3's lerp: its state across frames.
#[derive(Clone, Copy, Debug)]
pub struct Lerp {
    init: bool,
    last_us: u64,
    p: Vec3,
    q: Quat,
}
impl Lerp {
    pub const NEW: Lerp = Lerp { init: false, last_us: 0, p: Vec3 { x: 0.0, y: 0.0, z: 0.0 }, q: Quat::IDENTITY };
    /// 1 - exp(-speed dt) of the way to the head (dt from the clock, at most 0.1 s; speed at least 0.1).
    pub fn step(&mut self, head: &Pose, now_us: u64, speed: f32, base_reset: bool) -> Pose {
        if !self.init {
            self.init = true;
            self.p = head.position;
            self.q = head.orientation;
        } else {
            let mut dt = 0.0139f32;
            if self.last_us != 0 && now_us > self.last_us {
                dt = (now_us - self.last_us) as f32 * 1e-6;
            }
            if dt > 0.100 {
                dt = 0.100;
            }
            let speed = if speed < 0.1 { 0.1 } else { speed };
            let t = 1.0 - exp(-speed * dt);
            self.p.x += (head.position.x - self.p.x) * t;
            self.p.y += (head.position.y - self.p.y) * t;
            self.p.z += (head.position.z - self.p.z) * t;
            self.q = nlerp_quat(self.q, head.orientation, t);
        }
        self.last_us = now_us;
        if base_reset {
            self.p = head.position;
            self.q = head.orientation;
        }
        Pose { orientation: self.q, position: self.p }
    }
}
static LERP: Mutex<Lerp> = Mutex::new(Lerp::NEW);

/// The head filter for this frame. `mode` is CyberpunkVR_HeadFilter (0 none, 3 the view's lerp, otherwise the adaptive
/// filter at `smooth`); `state` is OpenXRManager's adaptive filter, reset whenever it is not the one in use.
///
/// # Safety
/// `head` and `out` are XrPosef (the same one is fine), `state` OpenXRManager's filter state.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_filter_head(
    head: *const Pose,
    base_reset: bool,
    mode: i32,
    smooth: f32,
    lerp_speed: f32,
    now_us: u64,
    state: *mut PoseFilter,
    out: *mut Pose,
) {
    if head.is_null() || out.is_null() {
        return;
    }
    // SAFETY: the caller's poses (`out` may be `head`: read first) and filter.
    let head = unsafe { head.read() };
    let Some(state) = (unsafe { state.as_mut() }) else { return };
    let filtered = if mode == 3 {
        let p = LERP.lock().unwrap_or_else(|p| p.into_inner()).step(&head, now_us, lerp_speed, base_reset);
        state.initialized = false;
        p
    } else if mode != 0 {
        filter_pose(state, &head, smooth, 0.0012, 0.0080, 0.0035, 0.0350)
    } else {
        state.initialized = false;
        head
    };
    // SAFETY: the caller's pose.
    unsafe { out.write(filtered) };
}

/// The predicted pose's filter (CyberpunkVR_PredictFilter, formerly in OpenXRManager::LocateHeadPoseAt): its own
/// state, on the game's threads at the game's rate. 1 the adaptive follow at xr_hmd_smooth; 2 a dead band (held below
/// the threshold, taken whole at or above it -- no fraction, so no speed-dependent lag); 3 one time constant
/// (CyberpunkVR_PoseLerpSpeed) for position and orientation alike. A new tracking origin re-seeds it.
#[derive(Clone, Copy, Debug)]
pub struct PredictFilter {
    init: bool,
    pos: Vec3,
    ori: Quat,
    origin: u64,
    last_us: u64,
}
/// The dead band's counts (held/stepped, position/orientation), the diagnostics' when they are on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Deadband {
    pub held_pos: bool,
    pub held_ori: bool,
}
impl PredictFilter {
    pub const NEW: PredictFilter =
        PredictFilter { init: false, pos: Vec3 { x: 0.0, y: 0.0, z: 0.0 }, ori: Quat::IDENTITY, origin: 0, last_us: 0 };
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, rel: &Pose, origin: u64, now_us: u64, mode: i32, strength: f32, lerp_speed: f32, band_mm: f32, band_deg: f32) -> (Pose, Option<Deadband>) {
        let mut band = None;
        // tracking::RebaseTrackingFilter: a recenter is a change of coordinates, not travel to a new target.
        let rebased = if self.init && self.origin == origin {
            false
        } else {
            self.init = true;
            self.origin = origin;
            self.last_us = now_us;
            self.pos = rel.position;
            self.ori = rel.orientation;
            true
        };
        if rebased || (mode == 1 && strength <= 0.001) {
            self.init = true;
            self.pos = rel.position;
            self.ori = rel.orientation;
        } else if mode == 3 {
            let mut dt = 0.0139f32;
            if self.last_us != 0 && now_us > self.last_us {
                dt = (now_us - self.last_us) as f32 * 1e-6;
            }
            self.last_us = now_us;
            if dt > 0.100 {
                dt = 0.100;
            }
            let speed = if lerp_speed < 0.1 { 0.1 } else { lerp_speed };
            let t = 1.0 - exp(-speed * dt);
            self.pos.x += (rel.position.x - self.pos.x) * t;
            self.pos.y += (rel.position.y - self.pos.y) * t;
            self.pos.z += (rel.position.z - self.pos.z) * t;
            self.ori = nlerp_quat(self.ori, rel.orientation, t);
        } else if mode == 2 {
            let dx = rel.position.x - self.pos.x;
            let dy = rel.position.y - self.pos.y;
            let dz = rel.position.z - self.pos.z;
            let pos_delta = sqrt(dx * dx + dy * dy + dz * dz);
            let held_pos = pos_delta < band_mm * 0.001;
            if !held_pos {
                self.pos = rel.position;
            }
            let ang_delta = quat_angle(self.ori, rel.orientation);
            let held_ori = ang_delta < band_deg * 0.01745329252;
            if !held_ori {
                self.ori = rel.orientation;
            }
            band = Some(Deadband { held_pos, held_ori });
        } else {
            // 1/(1+20 strength) of the step while still, the whole step past the release threshold.
            let follow = |delta: f32, quiet: f32, release: f32| {
                if release <= quiet {
                    return 1.0;
                }
                let mut t = (delta - quiet) / (release - quiet);
                if t < 0.0 {
                    t = 0.0;
                }
                if t > 1.0 {
                    t = 1.0;
                }
                let motion = t * t * (3.0 - 2.0 * t);
                let still = 1.0 / (1.0 + 20.0 * strength);
                still + (1.0 - still) * motion
            };
            let dx = rel.position.x - self.pos.x;
            let dy = rel.position.y - self.pos.y;
            let dz = rel.position.z - self.pos.z;
            let pos_delta = sqrt(dx * dx + dy * dy + dz * dz);
            let ang_delta = quat_angle(self.ori, rel.orientation);
            let pos_t = follow(pos_delta, 0.0012, 0.0080);
            let ang_t = follow(ang_delta, 0.0035, 0.0350);
            self.pos.x += dx * pos_t;
            self.pos.y += dy * pos_t;
            self.pos.z += dz * pos_t;
            self.ori = nlerp_quat(self.ori, rel.orientation, ang_t);
        }
        (Pose { orientation: self.ori, position: self.pos }, band)
    }
}
static PREDICT: Mutex<PredictFilter> = Mutex::new(PredictFilter::NEW);

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_PredictFilter: i32;
    static CyberpunkVR_PoseLerpSpeed: f32;
    static CyberpunkVR_DeadbandPosMm: f32;
    static CyberpunkVR_DeadbandAngDeg: f32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugDeadbandHeldPos: u64;
    static mut CyberpunkVR_DebugDeadbandHeldOri: u64;
    static mut CyberpunkVR_DebugDeadbandStepPos: u64;
    static mut CyberpunkVR_DebugDeadbandStepOri: u64;
}

/// The predicted head (base-relative) through the filter, in place.
///
/// # Safety
/// `rel` is an XrPosef.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_predict_filter(rel: *mut Pose, origin: u64, now_us: u64, strength: f32) {
    // SAFETY (whole body): the caller's pose; the C++ half's live tuning and its diagnostic counters, read and bumped
    // as it did.
    unsafe {
        let Some(rel) = rel.as_mut() else { return };
        let rd = |p: *const f32| core::ptr::read_volatile(p);
        let mode = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_PredictFilter));
        let (pose, band) = PREDICT.lock().unwrap_or_else(|p| p.into_inner()).step(
            rel,
            origin,
            now_us,
            mode,
            strength,
            rd(core::ptr::addr_of!(CyberpunkVR_PoseLerpSpeed)),
            rd(core::ptr::addr_of!(CyberpunkVR_DeadbandPosMm)),
            rd(core::ptr::addr_of!(CyberpunkVR_DeadbandAngDeg)),
        );
        if let Some(b) = band {
            if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 {
                let bump = |p: *mut u64| core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
                bump(if b.held_pos { core::ptr::addr_of_mut!(CyberpunkVR_DebugDeadbandHeldPos) } else { core::ptr::addr_of_mut!(CyberpunkVR_DebugDeadbandStepPos) });
                bump(if b.held_ori { core::ptr::addr_of_mut!(CyberpunkVR_DebugDeadbandHeldOri) } else { core::ptr::addr_of_mut!(CyberpunkVR_DebugDeadbandStepOri) });
            }
        }
        *rel = pose;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn yaw_quat(yaw: f32) -> Quat {
        Quat { x: 0.0, y: (yaw * 0.5).sin(), z: 0.0, w: (yaw * 0.5).cos() }
    }
    #[test]
    fn the_base_keeps_the_heading_and_drops_the_tilt() {
        // turned 0.7 rad and pitched down 0.4: the base is the 0.7 turn alone
        let yaw = yaw_quat(0.7);
        let pitch = Quat { x: (-0.2f32).sin(), y: 0.0, z: 0.0, w: (-0.2f32).cos() };
        let head = Pose { orientation: cvr_body::xr::multiply_quat(yaw, pitch), position: Vec3 { x: 1.0, y: 1.7, z: -2.0 } };
        let (base, y) = yaw_only_base(&head);
        assert!((y - 0.7).abs() < 1e-5, "{y}");
        assert_eq!(base.position, head.position);
        assert!(base.orientation.x == 0.0 && base.orientation.z == 0.0);
    }
    #[test]
    fn looking_straight_down_the_heading_comes_from_up() {
        let yaw = yaw_quat(-1.2);
        let down = Quat { x: (-core::f32::consts::FRAC_PI_4).sin(), y: 0.0, z: 0.0, w: (-core::f32::consts::FRAC_PI_4).cos() };
        let head = Pose { orientation: cvr_body::xr::multiply_quat(yaw, down), position: Vec3::default() };
        let (_, y) = yaw_only_base(&head);
        assert!((y + 1.2).abs() < 1e-4, "{y}");
    }
    #[test]
    fn the_predict_filter_seeds_bands_and_rebases() {
        let mut f = PredictFilter::NEW;
        let at = |x: f32| Pose { orientation: Quat::IDENTITY, position: Vec3 { x, y: 1.7, z: 0.0 } };
        assert_eq!(f.step(&at(0.0), 1, 1_000, 2, 0.35, 30.0, 0.8, 0.05).0.position.x, 0.0);
        // under the band: held
        let (p, b) = f.step(&at(0.0005), 1, 2_000, 2, 0.35, 30.0, 0.8, 0.05);
        assert_eq!((p.position.x, b), (0.0, Some(Deadband { held_pos: true, held_ori: true })));
        // at it: taken whole
        assert_eq!(f.step(&at(0.001), 1, 3_000, 2, 0.35, 30.0, 0.8, 0.05).0.position.x, 0.001);
        // a new origin seeds
        assert_eq!(f.step(&at(5.0), 2, 4_000, 1, 0.35, 30.0, 0.8, 0.05).0.position.x, 5.0);
        // mode 1 still: 1/(1+7) of a small step
        let p = f.step(&at(5.001), 2, 5_000, 1, 0.35, 30.0, 0.8, 0.05).0.position.x;
        assert!((p - (5.0 + 0.001 / 8.0)).abs() < 1e-6, "{p}");
    }
    #[test]
    fn mode_three_lerps_and_snaps_on_a_reset() {
        let mut l = Lerp::NEW;
        let a = Pose { orientation: Quat::IDENTITY, position: Vec3 { x: 0.0, y: 0.0, z: 0.0 } };
        let b = Pose { orientation: Quat::IDENTITY, position: Vec3 { x: 1.0, y: 0.0, z: 0.0 } };
        assert_eq!(l.step(&a, 1_000, 30.0, false).position.x, 0.0);
        let half = l.step(&b, 11_000, 30.0, false).position.x;
        assert!(half > 0.2 && half < 0.3, "{half}"); // 1 - exp(-0.3)
        assert_eq!(l.step(&a, 21_000, 30.0, true).position.x, 0.0, "a recenter snaps to the head");
    }
}

// ---- the XR cycle's head

/// OpenXRManager's m_headFilterState: the adaptive head filter, XR frame thread only.
static HEAD_FILTER: Mutex<PoseFilter> = Mutex::new(PoseFilter::NEW);

/// What FrameThreadMain hands the cycle's head locate.
#[repr(C)]
pub struct CycleHeadIn {
    pub locate_time: i64,
    pub hand_locate_time: i64,
    /// The external reset's serial read before this cycle's locates.
    pub reset_serial: u64,
    pub building_serial: u64,
    pub session_state: i32,
}
/// What the cycle's head locate gives back for the hands, the velocity and the layers.
#[repr(C)]
pub struct CycleHead {
    /// The head was located with position and orientation, and no external reset moved under it.
    pub located: bool,
    pub reset_stable: bool,
    /// xrLocateSpace's pose as it came (LOCAL space), whatever the outcome.
    pub location: Pose,
    pub base: Pose,
    pub base_inv: Quat,
    /// The base-relative head through this cycle's filter.
    pub filtered: Pose,
    /// The head at the hands' instant (the located head when that is the same instant or its locate fails).
    pub hand_head: Pose,
    pub angular_velocity: Vec3,
    pub linear_velocity: Vec3,
    pub velocity_flags: u64,
}
const _: () = assert!(core::mem::offset_of!(CycleHead, location) == 4 && core::mem::offset_of!(CycleHead, velocity_flags) == 160);

#[cfg(not(test))]
#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_HeadFilter: i32;
    fn GetHmdTrackingSmooth() -> f32;
}
#[cfg(not(test))]
extern "system" {
    fn xrLocateSpace(space: openxr_sys::Space, base: openxr_sys::Space, time: openxr_sys::Time, location: *mut openxr_sys::SpaceLocation) -> openxr_sys::Result;
}

/// FrameThreadMain's head for the cycle: the view space located at the cycle's instant (with its velocity) and, when the
/// hands aim elsewhere, at theirs; under the external-reset lock the base for the frame (taken yaw-only on a recenter,
/// the first pose or an external reset), mirrored for the submit path; the base-relative head published as the
/// roomscale sample, filtered, stored with the views in the frame's slot and published as the head; the reset marked
/// applied. A lost head drops the filter and marks the head lost.
///
/// # Safety
/// `input` and `out` are valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_locate_cycle_head(input: *const CycleHeadIn, out: *mut CycleHead) {
    use core::sync::atomic::Ordering;
    use openxr_sys as xr;
    use xr::Handle;
    // SAFETY: the caller's input and output.
    let (Some(inp), Some(out)) = (unsafe { input.as_ref() }, unsafe { out.as_mut() }) else { return };
    let view = xr::Space::from_raw(crate::frame::VIEW_SPACE.load(Ordering::Acquire));
    let local = xr::Space::from_raw(crate::frame::LOCAL_SPACE.load(Ordering::Acquire));
    let pose_of = |p: &xr::Posef| Pose {
        orientation: Quat { x: p.orientation.x, y: p.orientation.y, z: p.orientation.z, w: p.orientation.w },
        position: Vec3 { x: p.position.x, y: p.position.y, z: p.position.z },
    };
    let vec_of = |v: &xr::Vector3f| Vec3 { x: v.x, y: v.y, z: v.z };
    let need = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
    // SAFETY: zeroed OpenXR structs with their types set, chained as the runtime expects.
    let (location, located_ok, velocity) = unsafe {
        let mut velocity: xr::SpaceVelocity = core::mem::zeroed();
        velocity.ty = xr::StructureType::SPACE_VELOCITY;
        let mut location: xr::SpaceLocation = core::mem::zeroed();
        location.ty = xr::StructureType::SPACE_LOCATION;
        location.next = (&mut velocity as *mut xr::SpaceVelocity).cast();
        let res = xrLocateSpace(view, local, xr::Time::from_nanos(inp.locate_time), &mut location);
        location.next = core::ptr::null_mut();
        (location, res.into_raw() >= 0, velocity)
    };
    // THE HEAD AT THE HANDS' INSTANT: a head-relative hand is only meaningful against the head of the same instant.
    let mut hand_head = location.pose;
    if inp.hand_locate_time != inp.locate_time {
        // SAFETY: as above.
        unsafe {
            let mut h: xr::SpaceLocation = core::mem::zeroed();
            h.ty = xr::StructureType::SPACE_LOCATION;
            if xrLocateSpace(view, local, xr::Time::from_nanos(inp.hand_locate_time), &mut h).into_raw() >= 0 && h.location_flags.contains(need) {
                hand_head = h.pose;
            }
        }
    }
    out.location = pose_of(&location.pose);
    out.hand_head = pose_of(&hand_head);
    out.angular_velocity = vec_of(&velocity.angular_velocity);
    out.linear_velocity = vec_of(&velocity.linear_velocity);
    out.velocity_flags = velocity.velocity_flags.into_raw();

    // A reset between the locate and the publication invalidates this sample.
    let reset = &crate::published::RESET;
    let guard = reset.lock.lock();
    let serial = reset.gate.serial();
    out.reset_stable = inp.reset_serial & 1 == 0 && inp.reset_serial == serial;
    out.located = out.reset_stable
        && located_ok
        && location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
        && location.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
    if !out.located {
        drop(guard);
        HEAD_FILTER.lock().unwrap_or_else(|p| p.into_inner()).initialized = false;
        crate::tracking::cvr_xr_head_lost(out.reset_stable);
        return;
    }
    let mut base = Pose::default();
    // SAFETY: plain poses.
    let base_reset = unsafe { crate::tracking::cvr_xr_take_base(&out.location, reset.gate.pending(), &mut base) };
    crate::tracking::publish_base_mirror(&base);
    let base_inv = cvr_body::xr::conjugate_quat(base.orientation);
    let p = out.location.position;
    let rel = Pose {
        orientation: cvr_body::xr::multiply_quat(base_inv, out.location.orientation),
        position: rotate_vector(base_inv, Vec3 { x: p.x - base.position.x, y: p.y - base.position.y, z: p.z - base.position.z }),
    };
    // SAFETY: plain values; the C getters are plain.
    unsafe {
        crate::tracking::cvr_xr_publish_head_sample(&rel, inp.session_state, location.location_flags.into_raw());
        let mut filter = HEAD_FILTER.lock().unwrap_or_else(|p| p.into_inner());
        if base_reset {
            filter.initialized = false;
            let hands = &crate::tracking::HANDS;
            let g = hands.lock.lock();
            for f in hands.aim_filter.get(&g).iter_mut() {
                f.initialized = false;
            }
            drop(g);
            crate::menu::reset_anchor();
        }
        let mut filtered = rel;
        cvr_xr_filter_head(
            &rel,
            base_reset,
            core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HeadFilter)),
            GetHmdTrackingSmooth(),
            core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_PoseLerpSpeed)),
            crate::tracking::now_us(),
            &mut *filter,
            &mut filtered,
        );
        drop(filter);
        // Everything this frame will need, stored together and stamped with the present that will show it. The views
        // are this thread's own (it alone locates them).
        let views = &crate::views::VIEWS;
        let count = *views.count.get();
        if count >= 2 {
            crate::frame_identity::cvr_xr_publish_frame_slot(
                inp.building_serial,
                inp.locate_time,
                (*views.views.get()).as_ptr(),
                count,
                &location.pose,
            );
        }
        crate::tracking::cvr_xr_publish_head(&filtered);
        out.base = base;
        out.base_inv = base_inv;
        out.filtered = filtered;
    }
    reset.gate.applied.store(inp.reset_serial, Ordering::Release);
    drop(guard);
}
