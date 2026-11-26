//! The VRIK calibration (formerly most of src/Runtimes/OpenXRCalibration.cpp): the T-pose auto-calibration sampled on
//! the frame thread, the body taken from the player's height (include/Runtimes/BodyProportions.hpp), the wrist defaults
//! (include/Runtimes/WristBase.hpp), the eyes' height over the STAGE floor, and vrik_calibration.ini written and read
//! with the C runtime, as before, byte for byte.
//!
//! The calibration's state is owned here (`CalibrationShared`); in the Rust build OpenXRManager's members are
//! references to it, so its inline accessors and every C++ reader read this. The recenter base, the camera mount's
//! reset (the live controls' UI) and the hands' sample under the hand lock stay with OpenXRManager.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::{c_char, CStr};
use cvr_host::log;
use cvr_crt::{self as crt, Arg, File};
use openxr_sys as xr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use xr::Handle as _;

/// A float in an atomic, as std::atomic<float> is (4 bytes, the bits).
#[repr(transparent)]
pub struct AtomicF32(AtomicU32);
impl AtomicF32 {
    pub const fn zero() -> AtomicF32 {
        AtomicF32(AtomicU32::new(0))
    }
    pub const fn new(v: f32) -> AtomicF32 {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
    pub fn load_acquire(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Acquire))
    }
}

/// OpenXRManager's calibration members (include/Runtimes/OpenXRManager.hpp references these).
#[repr(C)]
pub struct CalibrationShared {
    pub calib_valid: AtomicI32,
    /// scaleR, scaleL, heightR, heightL, swingR, swingL, poleR, poleL, wrist R pitch/yaw/roll, wrist L pitch/yaw/roll.
    pub calib: [AtomicF32; 14],
    pub calib_ext_valid: AtomicI32,
    /// The shoulders from the eyes in body-frame OpenXR axes (right, up, back): right xyz, left xyz.
    pub calib_ext: [AtomicF32; 6],
    pub measure_valid: AtomicI32,
    pub user_height: AtomicF32,
    pub measured_eye_height: AtomicF32,
    pub first_setup_done: AtomicI32,
    pub user_arm_len_r: AtomicF32,
    pub user_arm_len_l: AtomicF32,
    pub user_eye_height: AtomicF32,
    pub cam_bake_offset: [AtomicF32; 3],
    /// 0 idle, 1 sampling, 2 done, 3 aborted.
    pub calib_state: AtomicI32,
    pub calib_seconds: AtomicF32,
    pub calib_progress: AtomicF32,
    /// The runtime's STAGE space (floor at y = 0), made on the first T-pose.
    pub stage_space: AtomicU64,
    pub stage_unavailable: AtomicBool,
}
const _: () = assert!(core::mem::offset_of!(CalibrationShared, stage_space) == 144);
const _: () = assert!(core::mem::size_of::<CalibrationShared>() == 160);

macro_rules! zeros {
    ($n:expr) => {{
        [const { AtomicF32::zero() }; $n]
    }};
}
static SHARED: CalibrationShared = CalibrationShared {
    calib_valid: AtomicI32::new(0),
    calib: zeros!(14),
    calib_ext_valid: AtomicI32::new(0),
    calib_ext: zeros!(6),
    measure_valid: AtomicI32::new(0),
    user_height: AtomicF32::zero(),
    measured_eye_height: AtomicF32::zero(),
    first_setup_done: AtomicI32::new(0),
    user_arm_len_r: AtomicF32::zero(),
    user_arm_len_l: AtomicF32::zero(),
    user_eye_height: AtomicF32::zero(),
    cam_bake_offset: zeros!(3),
    calib_state: AtomicI32::new(0),
    calib_seconds: AtomicF32::new(3.0),
    calib_progress: AtomicF32::zero(),
    stage_space: AtomicU64::new(0),
    stage_unavailable: AtomicBool::new(false),
};
#[no_mangle]
pub extern "C" fn cvr_xr_calibration() -> *const CalibrationShared {
    &SHARED
}

/// include/Runtimes/WristBase.hpp kWristBase: wRp wRy wRr wLp wLy wLr, degrees.
pub const WRIST_BASE: [f32; 6] = [0.0, -90.0, -21.0, -180.0, -90.0, -15.0];

/// include/Runtimes/BodyProportions.hpp.
pub mod body {
    pub const EYE_HEIGHT: f32 = 0.936;
    pub const SHOULDER_HEIGHT: f32 = 0.818;
    pub const UPPER_ARM: f32 = 0.186;
    pub const FOREARM: f32 = 0.146;
    pub const SHOULDER_HALF_WIDTH: f32 = 0.105;
    pub const SHOULDER_BEHIND_EYES: f32 = 0.05;
    pub const MIN_HEIGHT: f32 = 1.40;
    pub const MAX_HEIGHT: f32 = 2.20;
    pub struct Proportions {
        pub height: f32,
        pub eye_height: f32,
        pub shoulder_below_eyes: f32,
        pub shoulder_half_width: f32,
        pub arm_length: f32,
    }
    pub fn from_height(height: f32) -> Proportions {
        Proportions {
            height,
            eye_height: EYE_HEIGHT * height,
            shoulder_below_eyes: (EYE_HEIGHT - SHOULDER_HEIGHT) * height,
            shoulder_half_width: SHOULDER_HALF_WIDTH * height,
            arm_length: (UPPER_ARM + FOREARM) * height,
        }
    }
}

/// std::clamp.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    crt::cpp_clamp(v, lo, hi)
}

/// OpenXRManager::SetVRHandCalib.
pub fn set_hand_calib(v: [f32; 14]) {
    for (slot, value) in SHARED.calib.iter().zip(v) {
        slot.store(value);
    }
    SHARED.calib_valid.store(1, Ordering::Relaxed);
}
/// OpenXRManager::SetShoulderAnatomical.
pub fn set_shoulders(e: [f32; 6]) {
    for (slot, value) in SHARED.calib_ext.iter().zip(e) {
        slot.store(value);
    }
    SHARED.calib_ext_valid.store(1, Ordering::Relaxed);
}
/// OpenXRManager::SetCameraOffset.
pub fn set_camera_offset(c: [f32; 3]) {
    for (slot, value) in SHARED.cam_bake_offset.iter().zip(c) {
        slot.store(value);
    }
}

#[cfg(not(test))]
extern "system" {
    fn xrCreateReferenceSpace(session: xr::Session, info: *const xr::ReferenceSpaceCreateInfo, space: *mut xr::Space) -> xr::Result;
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}
#[cfg(not(test))]
extern "C" {
    /// vrik_calibration.ini in the plugin's config folder (Core/Paths.hpp).
    fn cvr_cpp_calibration_path(out: *mut c_char, size: usize);
    /// OpenXRManager::GetFrameAimTimeNow.
    /// OpenXRManager::BakeCameraOffset: the camera mount back to the neutral neck (and saved).
    fn cvr_cpp_xr_bake_camera_offset();
}

/// The T-pose's accumulators (the frame thread's own).
struct Sampling {
    start: f64,
    arm_span_max: f32,
    hmd_height_sum: f32,
    samples: i32,
    right_sum: [f32; 3],
    left_sum: [f32; 3],
    eye_floor_sum: f32,
    eye_floor_count: i32,
}
static SAMPLING: Mutex<Sampling> = Mutex::new(Sampling {
    start: 0.0,
    arm_span_max: 0.0,
    hmd_height_sum: 0.0,
    samples: 0,
    right_sum: [0.0; 3],
    left_sum: [0.0; 3],
    eye_floor_sum: 0.0,
    eye_floor_count: 0,
});

/// OpenXRManager::EnsureCalibDefaults: the wrist and elbow defaults when never set (else finalising reads zeros and
/// publishes identity wrists).
#[no_mangle]
pub extern "C" fn cvr_xr_ensure_calib_defaults() {
    let c = &SHARED.calib;
    if c[0].load() == 0.0 && c[1].load() == 0.0 && c[9].load() == 0.0 {
        let defaults = [1.05, 1.06, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
        for (i, v) in defaults.into_iter().enumerate() {
            c[i].store(v);
        }
        for (i, v) in WRIST_BASE.into_iter().enumerate() {
            c[8 + i].store(v);
        }
        log!("Auto-calibration: initialised wrist/elbow defaults (the baked wrist base).\n");
    }
}

/// OpenXRManager::StartAutoCalibration.
#[no_mangle]
pub extern "C" fn cvr_xr_start_auto_calibration(secs: f32) {
    SHARED.calib_seconds.store(secs);
    SHARED.calib_progress.store(0.0);
    {
        let mut s = SAMPLING.lock().unwrap_or_else(|p| p.into_inner());
        s.arm_span_max = 0.0;
        s.hmd_height_sum = 0.0;
        s.samples = 0;
        s.right_sum = [0.0; 3];
        s.left_sum = [0.0; 3];
        s.eye_floor_sum = 0.0;
        s.eye_floor_count = 0;
        s.start = 0.0;
    }
    cvr_xr_ensure_calib_defaults();
    SHARED.calib_state.store(1, Ordering::Relaxed);
    log!("Auto-calibration: started ({:.1}s T-pose sample). Stretch arms STRAIGHT OUT to the sides.\n", secs);
}

/// OpenXRManager::LocateEyesOverFloor: the view in the runtime's STAGE space, whose floor is y = 0 (the session's
/// LOCAL space starts wherever it started). The STAGE space is made the first time; a runtime without one says so once.
///
/// # Safety
/// `session` and `view` are live or 0.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_locate_eyes_over_floor(session: u64, view: u64, time: i64, height: *mut f32) -> bool {
    if height.is_null() || SHARED.stage_unavailable.load(Ordering::Relaxed) || session == 0 || view == 0 {
        return false;
    }
    // SAFETY (whole body): the live session and view; the space made here is OpenXRManager's (destroyed at shutdown).
    unsafe {
        if SHARED.stage_space.load(Ordering::Relaxed) == 0 {
            let info = xr::ReferenceSpaceCreateInfo {
                ty: xr::StructureType::REFERENCE_SPACE_CREATE_INFO,
                next: core::ptr::null(),
                reference_space_type: xr::ReferenceSpaceType::STAGE,
                pose_in_reference_space: xr::Posef {
                    orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                    position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 },
                },
            };
            let mut space = xr::Space::NULL;
            if xrCreateReferenceSpace(xr::Session::from_raw(session), &info, &mut space).into_raw() < 0 {
                SHARED.stage_space.store(0, Ordering::Relaxed);
                SHARED.stage_unavailable.store(true, Ordering::Relaxed);
                log!("Auto-calibration: the runtime has no STAGE space -- the eyes' height over the floor is not measured.\n");
                return false;
            }
            SHARED.stage_space.store(space.into_raw(), Ordering::Relaxed);
        }
        if time <= 0 {
            return false;
        }
        let mut location: xr::SpaceLocation = core::mem::zeroed();
        location.ty = xr::StructureType::SPACE_LOCATION;
        let res = xrLocateSpace(
            xr::Space::from_raw(view),
            xr::Space::from_raw(SHARED.stage_space.load(Ordering::Relaxed)),
            xr::Time::from_nanos(time),
            &mut location,
        );
        if res.into_raw() < 0 || !location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID) {
            return false;
        }
        let h = location.pose.position.y;
        *height = h;
        h.is_finite() && h > 0.5 && h < 2.5
    }
}

#[cfg(not(test))]
fn now_seconds() -> f64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut now, mut freq) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut freq);
    }
    now as f64 / freq as f64
}

/// The T-pose's result from its accumulators (the finalisation of OpenXRManager::TickAutoCalibration), or None when
/// the span is implausible. Returns the shoulders, the arm length per hand, the eyes' height and the averages logged.
pub struct TPose {
    pub shoulders: [f32; 6],
    pub arm_r: f32,
    pub arm_l: f32,
    pub eye_height: f32,
    pub avg_r: [f32; 3],
    pub avg_l: [f32; 3],
}
fn finalise(s: &Sampling) -> Option<TPose> {
    let arm_span = s.arm_span_max;
    if arm_span < 0.5 || arm_span > 2.5 || s.samples == 0 {
        return None;
    }
    let inv_n = 1.0 / s.samples as f32;
    let avg_r = [s.right_sum[0] * inv_n, s.right_sum[1] * inv_n, s.right_sum[2] * inv_n];
    let avg_l = [s.left_sum[0] * inv_n, s.left_sum[1] * inv_n, s.left_sum[2] * inv_n];
    // The shoulders, body-frame OpenXR axes (X right, Y up, Z back), from the same coordinates the hands are drawn in.
    let mut shoulder_half = body::SHOULDER_HALF_WIDTH * arm_span;
    if shoulder_half < 0.10 {
        shoulder_half = 0.10;
    }
    if shoulder_half > 0.20 {
        shoulder_half = 0.20;
    }
    let right_sign = if avg_r[0] >= avg_l[0] { 1.0 } else { -1.0 };
    let rx = shoulder_half * right_sign;
    let lx = -shoulder_half * right_sign;
    // One height for both: the higher T-pose hand, so a tired arm does not pull its shoulder down.
    let mut shoulder_y = if avg_r[1] > avg_l[1] { avg_r[1] } else { avg_l[1] };
    if shoulder_y > -0.12 {
        shoulder_y = -0.12;
    }
    if shoulder_y < -0.30 {
        shoulder_y = -0.30;
    }
    let back = body::SHOULDER_BEHIND_EYES;
    // The arm from the SPAN (the best fully extended frame), the same for both: (span - shoulders) / 2.
    let mut span_arm = (arm_span - 2.0 * shoulder_half) * 0.5;
    if span_arm < 0.40 {
        span_arm = 0.40;
    }
    if span_arm > 0.85 {
        span_arm = 0.85;
    }
    // The eyes over the FLOOR (STAGE space); the LOCAL heights summed are over the session's start.
    let eye_height = if s.eye_floor_count > 0 { s.eye_floor_sum / s.eye_floor_count as f32 } else { 0.0 };
    Some(TPose { shoulders: [rx, shoulder_y, back, lx, shoulder_y, back], arm_r: span_arm, arm_l: span_arm, eye_height, avg_r, avg_l })
}

/// OpenXRManager::TickAutoCalibration, from the frame thread: one sample, and at the end the result published, saved,
/// the camera mount reset and a recenter asked.
///
/// # Safety
/// `session` and `view` are live or 0.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_tick_auto_calibration(session: u64, view: u64) {
    if SHARED.calib_state.load(Ordering::Relaxed) != 1 {
        return;
    }
    let t = now_seconds();
    let mut s = SAMPLING.lock().unwrap_or_else(|p| p.into_inner());
    if s.start == 0.0 {
        s.start = t;
    }
    let elapsed = (t - s.start) as f32;
    let total = SHARED.calib_seconds.load();
    let mut prog = if total > 0.0 { elapsed / total } else { 1.0 };
    if prog > 1.0 {
        prog = 1.0;
    }
    SHARED.calib_progress.store(prog);

    // Both hands, and the head's height, in one step under the hand lock.
    let sample = {
        let guard = crate::tracking::HANDS.lock.lock();
        let h = crate::tracking::HANDS.hands.get(&guard);
        (h[0].valid && h[1].valid).then(|| (h[0].pos, h[1].pos, crate::tracking::head_y()))
    };
    if let Some((left, right, hmd_y)) = sample {
        let (dx, dy, dz) = (right[0] - left[0], right[1] - left[1], right[2] - left[2]);
        let span = (dx * dx + dy * dy + dz * dz).sqrt();
        if span > s.arm_span_max {
            s.arm_span_max = span;
        }
        // Where each hand actually sits: the X sign tells which is right (swapped controllers are detected).
        for k in 0..3 {
            s.right_sum[k] += right[k];
            s.left_sum[k] += left[k];
        }
        s.hmd_height_sum += hmd_y;
        s.samples += 1;
    }
    let mut eyes = 0.0f32;
    // SAFETY: the caller's handles; the frame's aim time.
    if unsafe { cvr_xr_locate_eyes_over_floor(session, view, crate::frame::cvr_xr_frame_aim_time_now(), &mut eyes) } {
        s.eye_floor_sum += eyes;
        s.eye_floor_count += 1;
    }
    if elapsed < total {
        return;
    }
    let arm_span = s.arm_span_max;
    let Some(p) = finalise(&s) else {
        log!("Auto-calibration: armSpan {:.3}m out of plausible range \u{2014} aborting.\n", arm_span);
        SHARED.calib_state.store(3, Ordering::Relaxed);
        return;
    };
    drop(s);
    set_shoulders(p.shoulders);
    SHARED.measured_eye_height.store(p.eye_height);
    SHARED.user_arm_len_r.store(p.arm_r);
    SHARED.user_arm_len_l.store(p.arm_l);
    SHARED.user_eye_height.store(p.eye_height);
    SHARED.measure_valid.store(1, Ordering::Relaxed);
    // The reach scale stays neutral (the bones are scaled to the measured arm instead); swing, pole and the wrists
    // are kept -- the T-pose measures the anatomy only.
    let c = &SHARED.calib;
    let (mut swing_r, mut swing_l) = (c[4].load(), c[5].load());
    if swing_r == 0.0 && swing_l == 0.0 {
        swing_r = 1.0;
        swing_l = 1.0;
    }
    set_hand_calib([
        1.0,
        1.0,
        0.0,
        0.0,
        swing_r,
        swing_l,
        c[6].load(),
        c[7].load(),
        c[8].load(),
        c[9].load(),
        c[10].load(),
        c[11].load(),
        c[12].load(),
        c[13].load(),
    ]);
    cvr_xr_save_calibration();
    let e = p.shoulders;
    log!("Auto-calibration DONE.\n");
    log!("  armSpan = {:.3}m  armLenR/L = {:.3}/{:.3}m  eyeHeight = {:.3}m\n", arm_span, p.arm_r, p.arm_l, p.eye_height);
    log!("  ctrl R world-local avg = ({:.3}, {:.3}, {:.3})\n", p.avg_r[0], p.avg_r[1], p.avg_r[2]);
    log!("  ctrl L world-local avg = ({:.3}, {:.3}, {:.3})\n", p.avg_l[0], p.avg_l[1], p.avg_l[2]);
    log!("  shoulder R = ({:.3}, {:.3}, {:.3})\n", e[0], e[1], e[2]);
    log!("  shoulder L = ({:.3}, {:.3}, {:.3})\n", e[3], e[4], e[5]);
    // SAFETY: OpenXRManager's own methods.
    unsafe {
        // The player stood straight in the T-pose: the camera mount is reset to the neutral neck.
        cvr_cpp_xr_bake_camera_offset();
        // The body's forward is the one the T-pose faced.
        crate::tracking::cvr_xr_request_recenter();
    }
    // A height already given decides the arms and the shoulders; the T-pose keeps which way is right, the eyes, the camera.
    let height = SHARED.user_height.load();
    if height > 0.0 {
        cvr_xr_apply_user_height(height);
    }
    SHARED.calib_state.store(2, Ordering::Relaxed);
    SHARED.calib_progress.store(1.0);
}

/// OpenXRManager::ApplyUserHeight: the arms and the shoulders from the player's height, saved.
#[no_mangle]
pub extern "C" fn cvr_xr_apply_user_height(meters: f32) {
    if !meters.is_finite() {
        return;
    }
    let height = clamp(meters, body::MIN_HEIGHT, body::MAX_HEIGHT);
    let b = body::from_height(height);
    cvr_xr_ensure_calib_defaults();
    // Which way is right: what the last T-pose found (swapped controllers), else the runtime's +X.
    let right = if SHARED.calib_ext[0].load() < 0.0 { -1.0 } else { 1.0 };
    let half = clamp(b.shoulder_half_width, 0.10, 0.20);
    let drop = clamp(b.shoulder_below_eyes, 0.12, 0.30);
    let back = body::SHOULDER_BEHIND_EYES;
    set_shoulders([half * right, -drop, back, -half * right, -drop, back]);
    let arm = clamp(b.arm_length, 0.45, 0.85);
    SHARED.user_arm_len_r.store(arm);
    SHARED.user_arm_len_l.store(arm);
    if SHARED.user_eye_height.load() <= 0.0 {
        SHARED.user_eye_height.store(b.eye_height);
    }
    SHARED.measure_valid.store(1, Ordering::Relaxed);
    SHARED.user_height.store(height);
    SHARED.first_setup_done.store(1, Ordering::Relaxed);
    cvr_xr_save_calibration();
    log!(
        "Body from the height {:.2} m: arms {:.3} m (shoulder to wrist), shoulders {:.3} m apart and {:.3} m under the eyes\n",
        height,
        arm,
        2.0 * half,
        drop
    );
}

#[cfg(not(test))]
fn calibration_path() -> [c_char; 260] {
    let mut path = [0 as c_char; 260];
    // SAFETY: a buffer of the size passed.
    unsafe { cvr_cpp_calibration_path(path.as_mut_ptr(), path.len()) };
    path
}
fn path_text(path: &[c_char; 260]) -> String {
    // SAFETY: NUL-terminated by the C++ half (or all zeros).
    unsafe { CStr::from_ptr(path.as_ptr()) }.to_string_lossy().into_owned()
}

/// The file's text, line by line as OpenXRManager::SaveCalibrationToFile printed it (version 4: the wrists as offsets
/// from the baked base).
pub fn calibration_lines() -> Vec<std::ffi::CString> {
    let f = |a: &AtomicF32| Arg::F(a.load() as f64);
    let c = &SHARED.calib;
    let e = &SHARED.calib_ext;
    let cb = &SHARED.cam_bake_offset;
    let w: Vec<Arg> = (0..6).map(|i| Arg::F((c[8 + i].load() - WRIST_BASE[i]) as f64)).collect();
    vec![
        crt::format(c"# CyberpunkVRPort VRIK auto-calibration\n", &[]),
        crt::format(c"version=4\n", &[]),
        crt::format(c"scaleR=%.4f\nscaleL=%.4f\n", &[f(&c[0]), f(&c[1])]),
        crt::format(c"heightR=%.4f\nheightL=%.4f\n", &[f(&c[2]), f(&c[3])]),
        crt::format(c"swingR=%.4f\nswingL=%.4f\n", &[f(&c[4]), f(&c[5])]),
        crt::format(c"poleR=%.4f\npoleL=%.4f\n", &[f(&c[6]), f(&c[7])]),
        crt::format(c"wRp=%.4f\nwRy=%.4f\nwRr=%.4f\n", &w[0..3]),
        crt::format(c"wLp=%.4f\nwLy=%.4f\nwLr=%.4f\n", &w[3..6]),
        crt::format(c"shoulderRX=%.4f\nshoulderRY=%.4f\nshoulderRZ=%.4f\n", &[f(&e[0]), f(&e[1]), f(&e[2])]),
        crt::format(c"shoulderLX=%.4f\nshoulderLY=%.4f\nshoulderLZ=%.4f\n", &[f(&e[3]), f(&e[4]), f(&e[5])]),
        crt::format(c"cameraMountVersion=1\n", &[]),
        crt::format(c"camBakeX=%.4f\ncamBakeY=%.4f\ncamBakeZ=%.4f\n", &[f(&cb[0]), f(&cb[1]), f(&cb[2])]),
        crt::format(
            c"userArmLenR=%.4f\nuserArmLenL=%.4f\nuserEyeHeight=%.4f\n",
            &[f(&SHARED.user_arm_len_r), f(&SHARED.user_arm_len_l), f(&SHARED.user_eye_height)],
        ),
        crt::format(
            c"userHeight=%.4f\nfirstSetup=%d\n",
            &[f(&SHARED.user_height), Arg::I(SHARED.first_setup_done.load(Ordering::Relaxed))],
        ),
    ]
}

/// OpenXRManager::SaveCalibrationToFile: vrik_calibration.ini, in text mode as before.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_save_calibration() -> bool {
    let path = calibration_path();
    // SAFETY: a C string path.
    let Some(mut file) = (unsafe { File::open(path.as_ptr(), c"w") }) else {
        log!("SaveCalibration: failed to open {}\n", path_text(&path));
        return false;
    };
    for line in calibration_lines() {
        file.puts(&line);
    }
    drop(file);
    log!("Calibration saved -> {}\n", path_text(&path));
    true
}
#[cfg(test)]
pub extern "C" fn cvr_xr_save_calibration() -> bool {
    true
}

/// What a calibration file sets, applied over the current values as OpenXRManager::LoadCalibrationFromFile did.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub calib: [f32; 14],
    pub ext: [f32; 6],
    pub camera: [f32; 3],
}
/// One `key=value` line into the load (`%31[^=]=%f`), the values on the shared block written as they are read.
fn load_line(line: *const c_char, l: &mut Loaded, version: &mut i32, camera_version: &mut i32, wrist: &mut [f32; 6], seen: &mut [bool; 6]) {
    let mut key = [0 as c_char; 32];
    let mut val = 0.0f32;
    // SAFETY: a C string line; a 32-byte key (its size in the slot after it) and a float.
    let n = unsafe { crt::scan(line, c"%31[^=]=%f", &[key.as_mut_ptr().cast(), 32usize as *mut core::ffi::c_void, crt::ptr(&mut val)]) };
    if n != 2 {
        return;
    }
    // SAFETY: the scan NUL-terminated the key.
    let key = unsafe { CStr::from_ptr(key.as_ptr()) }.to_bytes();
    let is = |name: &str| key == name.as_bytes();
    if is("version") {
        *version = val as i32;
    }
    if is("cameraMountVersion") {
        *camera_version = val as i32;
    }
    const M: [&str; 8] = ["scaleR", "scaleL", "heightR", "heightL", "swingR", "swingL", "poleR", "poleL"];
    const W: [&str; 6] = ["wRp", "wRy", "wRr", "wLp", "wLy", "wLr"];
    const E: [&str; 6] = ["shoulderRX", "shoulderRY", "shoulderRZ", "shoulderLX", "shoulderLY", "shoulderLZ"];
    const C: [&str; 3] = ["camBakeX", "camBakeY", "camBakeZ"];
    for (i, name) in M.iter().enumerate() {
        if is(name) {
            l.calib[i] = val;
        }
    }
    for (i, name) in W.iter().enumerate() {
        if is(name) {
            wrist[i] = val;
            seen[i] = true;
        }
    }
    for (i, name) in E.iter().enumerate() {
        if is(name) {
            l.ext[i] = val;
        }
    }
    for (i, name) in C.iter().enumerate() {
        if is(name) {
            l.camera[i] = val;
        }
    }
    if is("userArmLenR") && (0.45..=0.95).contains(&val) {
        SHARED.user_arm_len_r.store(val);
    }
    if is("userArmLenL") && (0.45..=0.95).contains(&val) {
        SHARED.user_arm_len_l.store(val);
    }
    if is("userEyeHeight") && val > 0.0 && val <= 2.2 {
        SHARED.user_eye_height.store(val);
    }
    if is("userHeight") && (body::MIN_HEIGHT..=body::MAX_HEIGHT).contains(&val) {
        SHARED.user_height.store(val);
    }
    if is("firstSetup") {
        SHARED.first_setup_done.store(if val != 0.0 { 1 } else { 0 }, Ordering::Relaxed);
    }
}
/// The migrations after the lines: feet-based camera bakes dropped, version 1 heights zeroed, the wrists from offsets
/// (version 4) or absolute angles, version 2's scales and shoulder heights brought into range.
fn migrate(l: &mut Loaded, version: i32, camera_version: i32, wrist: &[f32; 6], seen: &[bool; 6]) {
    if camera_version < 1 {
        l.camera = [0.0; 3];
    }
    if version < 2 {
        l.calib[2] = 0.0;
        l.calib[3] = 0.0;
    }
    for i in 0..6 {
        if seen[i] {
            l.calib[8 + i] = if version >= 4 { WRIST_BASE[i] + wrist[i] } else { wrist[i] };
        }
    }
    if version < 3 {
        if l.calib[0] < 0.90 || l.calib[0] > 1.25 {
            l.calib[0] = 1.05;
        }
        if l.calib[1] < 0.90 || l.calib[1] > 1.25 {
            l.calib[1] = 1.06;
        }
        let mut shoulder_y = if l.ext[1] > l.ext[4] { l.ext[1] } else { l.ext[4] };
        if shoulder_y > -0.12 {
            shoulder_y = -0.12;
        }
        if shoulder_y < -0.30 {
            shoulder_y = -0.30;
        }
        l.ext[1] = shoulder_y;
        l.ext[4] = shoulder_y;
    }
}
fn current() -> Loaded {
    Loaded {
        calib: core::array::from_fn(|i| SHARED.calib[i].load()),
        ext: core::array::from_fn(|i| SHARED.calib_ext[i].load()),
        camera: core::array::from_fn(|i| SHARED.cam_bake_offset[i].load()),
    }
}

/// OpenXRManager::LoadCalibrationFromFile, at the frame loop's start.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_load_calibration() -> bool {
    let path = calibration_path();
    // SAFETY: a C string path.
    let Some(mut file) = (unsafe { File::open(path.as_ptr(), c"r") }) else { return false };
    let mut l = current();
    let (mut version, mut camera_version) = (0, 0);
    let (mut wrist, mut seen) = ([0.0f32; 6], [false; 6]);
    let mut line = [0 as c_char; 128];
    while file.gets(&mut line) {
        load_line(line.as_ptr(), &mut l, &mut version, &mut camera_version, &mut wrist, &mut seen);
    }
    drop(file);
    migrate(&mut l, version, camera_version, &wrist, &seen);
    set_hand_calib(l.calib);
    set_shoulders(l.ext);
    set_camera_offset(l.camera);
    log!("Calibration loaded <- {}\n", path_text(&path));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_file_round_trips_through_its_lines() {
        set_hand_calib([1.05, 1.06, 0.0, 0.0, 1.0, 1.0, 0.1, -0.2, 3.0, -90.0, -21.0, -180.0, -95.5, -15.0]);
        set_shoulders([0.15, -0.2, 0.05, -0.15, -0.2, 0.05]);
        set_camera_offset([0.01, 0.0, -0.02]);
        let text: Vec<u8> = calibration_lines().iter().flat_map(|l| l.to_bytes().to_vec()).collect();
        let text = String::from_utf8(text).unwrap();
        assert!(text.starts_with("# CyberpunkVRPort VRIK auto-calibration\nversion=4\nscaleR=1.0500\nscaleL=1.0600\n"), "{text}");
        assert!(text.contains("wRp=3.0000\nwRy=0.0000\nwRr=0.0000\nwLp=0.0000\nwLy=-5.5000\nwLr=0.0000\n"), "{text}");
        let mut l = Loaded { calib: [0.0; 14], ext: [0.0; 6], camera: [0.0; 3] };
        let (mut version, mut camera_version, mut wrist, mut seen) = (0, 0, [0.0f32; 6], [false; 6]);
        for line in text.lines() {
            let c = std::ffi::CString::new(format!("{line}\n")).unwrap();
            load_line(c.as_ptr(), &mut l, &mut version, &mut camera_version, &mut wrist, &mut seen);
        }
        migrate(&mut l, version, camera_version, &wrist, &seen);
        assert_eq!((version, camera_version), (4, 1));
        assert_eq!(l.calib, [1.05, 1.06, 0.0, 0.0, 1.0, 1.0, 0.1, -0.2, 3.0, -90.0, -21.0, -180.0, -95.5, -15.0]);
        assert_eq!(l.ext, [0.15, -0.2, 0.05, -0.15, -0.2, 0.05]);
        assert_eq!(l.camera, [0.01, 0.0, -0.02]);
    }
    #[test]
    fn an_old_file_is_migrated() {
        let mut l = Loaded { calib: [2.0, 0.5, 0.3, 0.3, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], ext: [0.1, -0.05, 0.0, -0.1, -0.4, 0.0], camera: [0.2, 0.2, 0.2] };
        migrate(&mut l, 1, 0, &[10.0, 0.0, 0.0, 0.0, 0.0, 0.0], &[true, false, false, false, false, false]);
        assert_eq!(&l.calib[..4], &[1.05, 1.06, 0.0, 0.0]);
        assert_eq!(l.calib[8], 10.0, "absolute before version 4");
        assert_eq!((l.ext[1], l.ext[4]), (-0.12, -0.12));
        assert_eq!(l.camera, [0.0; 3]);
    }
    #[test]
    fn the_tpose_takes_the_span_and_the_higher_hand() {
        let s = Sampling {
            start: 0.0,
            arm_span_max: 1.7,
            hmd_height_sum: 0.0,
            samples: 2,
            right_sum: [1.6, -0.4, 0.0],
            left_sum: [-1.6, -0.5, 0.0],
            eye_floor_sum: 3.2,
            eye_floor_count: 2,
        };
        let p = finalise(&s).unwrap();
        let half = 0.105f32 * 1.7;
        assert_eq!(p.shoulders, [half, -0.2, 0.05, -half, -0.2, 0.05]);
        assert_eq!(p.arm_r, (1.7 - 2.0 * half) * 0.5);
        assert_eq!(p.eye_height, 1.6);
        assert!(finalise(&Sampling { arm_span_max: 0.3, ..s }).is_none());
    }
}

/// The frame loop's first entry (FrameThreadMain): said once in the log, and the calibration read from its file --
/// or, without one, the arms' defaults and the wrists on the baked base.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_frame_loop_startup() {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    log!("OpenXRManager: Inline frame pump started.\n");
    if !cvr_xr_load_calibration() {
        let c = &SHARED.calib;
        c[0].store(1.05);
        c[1].store(1.06);
        c[4].store(1.0);
        c[5].store(1.0);
        for (i, v) in WRIST_BASE.into_iter().enumerate() {
            c[8 + i].store(v);
        }
    }
}
