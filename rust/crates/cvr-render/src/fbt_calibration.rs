//! The full-body tracking calibration in the game (formerly src/Fbt/Game/Calibration.cpp): the shared calibration (the
//! IK library's `Calibrator`, the same as the FBT IK lab's) run on the devices cvr-xr's trackers locate, in three
//! instants -- step 1 the head looking ahead at the horizon line (a pull of both triggers with the gaze within 8 deg
//! of level), step 2 the body (a pull: the capture, its body frame turned onto the feet and taken over them), step 3
//! the T-pose on the cross (held 4 s: the widest span's arm length, V's scale). Each step is a pull of both triggers
//! after they were released together, so the pull that pressed the menu button does not commit at once.
//!
//! A calibration belongs to the scene it started in: while it runs the game gets no triggers, grips or face buttons
//! and the VR menu stays shut, so it is cancelled when the player changes (a load), when a game menu stays open for
//! 2 s, and after 2 minutes without a step taken.
//!
//! The cross's frame, size and marks and step 2's feet frame are cvr-body's fbt_cross; the file is cvr-settings'
//! fbt_calibration_file. The IK library's skeleton and calibrator (C++ classes) stay the .cpp's behind four services,
//! where the body solve (Body.cpp's `Rig`) also takes them.

use crate::overlay_fbt_helpers::CrossView;
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use cvr_body::fbt_cross::{self as fx, Cross, Q4, V3};
use cvr_settings::fbt_calibration_file::Calibration;
use cvr_xr::trackers::Pose;
use openxr_sys::{self as xr, Handle};
use std::cell::RefCell;
use std::ffi::CString;
use std::sync::{Mutex, MutexGuard};

/// VrikFbtReference (include/Anim/CharacterRig.hpp; cvr-rig's rig.rs).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Reference {
    pub(crate) valid: bool,
    pub(crate) female: bool,
    pub(crate) eye: [f32; 3],
    pub(crate) eye_in_head: [f32; 3],
    pub(crate) hips_pos: [f32; 3],
    pub(crate) hips_rot: [f32; 4],
    pub(crate) head_pos: [f32; 3],
    pub(crate) head_rot: [f32; 4],
    pub(crate) foot_pos: [[f32; 3]; 2],
    pub(crate) foot_rot: [[f32; 4]; 2],
    pub(crate) upper_arm_pos: [[f32; 3]; 2],
    pub(crate) fore_arm_pos: [[f32; 3]; 2],
    pub(crate) hand_pos: [[f32; 3]; 2],
}

/// wrpuppets::fbt::RoomPose: a device in room space (STAGE in V's model axes: x right, y forward, z up).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RoomPose {
    pub valid: bool,
    pub pos: V3,
    pub rot: Q4,
    /// A hand: the hand-tracking skeleton's wrist joint, not a grip pose (never here).
    pub wrist: bool,
}
const _: () = assert!(
    core::mem::size_of::<RoomPose>() == 36
        && core::mem::offset_of!(RoomPose, rot) == 16
        && core::mem::offset_of!(RoomPose, wrist) == 32
);
impl Default for RoomPose {
    fn default() -> Self {
        RoomPose { valid: false, pos: [0.0; 3], rot: [0.0, 0.0, 0.0, 1.0], wrist: false }
    }
}

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// cvr-rig's: V's reference pose, and the rig file the IK library reads for her.
    fn VRIK_ReadFbtReference(out: *mut Reference) -> bool;
    fn VRIK_FbtRigPath(female: bool, out: *mut c_char, size: usize) -> bool;
    /// roomscale::PlayerIdentity (cvr-rig's roomscale_move.rs).
    fn cvr_roomscale_player_identity() -> usize;
    /// volatile int g_menuModeValue (C linkage; cvr-rig's menu_mode.rs).
    static g_menuModeValue: i32;
    /// The plugin's folder `folder` (Core/Paths.hpp order: 4 config), narrow, with its trailing separator.
    fn cvr_paths_dir_a(folder: u32) -> *const c_char;
    // The IK library's skeleton and calibrator (src/Fbt/Game/Calibration.cpp)
    /// V's skeleton read from `path` and the calibrator set over it: its bone count; -1 unreadable (why in `error`,
    /// the skeleton kept), -2 lacking the bones the calibration needs.
    fn cvr_cpp_fbt_load_rig(path: *const c_char, error: *mut c_char, size: usize) -> i32;
    /// Calibrator::AvatarArmLength.
    safe fn cvr_cpp_fbt_avatar_arm(side: i32) -> f32;
    /// Calibrator::MeasureArmsOut.
    fn cvr_cpp_fbt_measure_arms_out(c: *mut Calibration, head: *const RoomPose, devices: *const RoomPose) -> bool;
    /// Calibrator::Capture.
    fn cvr_cpp_fbt_capture(head: *const RoomPose, devices: *const RoomPose, out: *mut Calibration, why: *mut *const c_char) -> bool;
}
extern "system" {
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}

/// kCalibrationLevelDeg: step 1 counts only with the gaze this close to level.
const LEVEL_DEG: f32 = fx::LEVEL_DEG;
/// kCrossHoldSeconds.
const HOLD_SECONDS: f32 = 4.0;
/// The refusals' text slots (kRefuseLevel, kRefuseArm, kRefuseTracking) and the slots' count.
const REFUSE_LEVEL: i32 = 5;
const REFUSE_ARM: i32 = 6;
const REFUSE_TRACKING: i32 = 7;
const TEXT_SLOTS: i32 = 8;
const TRIGGER_ON: f32 = 0.8;
const TRIGGER_OFF: f32 = 0.2;
// The calibration's devices (wrpuppets::fbt::CalDevice) and the trackers' roles (cvr::trackers::Role).
const CAL_WAIST: usize = 0;
const CAL_LEFT_FOOT: usize = 1;
const CAL_RIGHT_FOOT: usize = 2;
const CAL_LEFT_HAND: usize = 3;
const CAL_RIGHT_HAND: usize = 4;
const CAL_COUNT: usize = 5;
const WAIST: usize = 0;
const LEFT_FOOT: usize = 1;
const RIGHT_FOOT: usize = 2;
const MAX_PATH: usize = 260;

/// wrpuppets::fbt::Calibration's default (`Calibration c{}`): scale 1, the rotations identity.
const CALIBRATION_DEFAULT: Calibration = Calibration {
    done: false,
    scale: 1.0,
    player_eye_height: 0.0,
    avatar_eye_height: 0.0,
    origin: [0.0; 3],
    yaw_inverse: [0.0, 0.0, 0.0, 1.0],
    head_rot_inverse: [0.0, 0.0, 0.0, 1.0],
    valid: [false; 5],
    offset: [[0.0; 3]; 5],
    rot_inverse: [[0.0, 0.0, 0.0, 1.0]; 5],
    arm_length: [0.0; 2],
};
const IDENTITY: xr::Posef = xr::Posef {
    orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
    position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 },
};
const CROSS_ZERO: Cross = Cross { spot: [0.0; 3], fwd: [0.0; 3], right: [0.0; 3], base: [0.0; 3], tip_y: 0.0, bar_y: 0.0, half_span: 0.0 };

/// 0 off, 1 head (look ahead), 2 body (look down), 3 arms (the T-pose on the cross).
static STEP: AtomicI32 = AtomicI32::new(0);
/// The last pull not taken: which message (a refusal's slot), until when -- shown under the step's text.
static REFUSED: AtomicI32 = AtomicI32::new(0);
static REFUSED_UNTIL: AtomicU64 = AtomicU64::new(0);
/// The player the calibration started with, when the last step was taken, since when a game menu is open.
static PLAYER: AtomicUsize = AtomicUsize::new(0);
static LAST_PROGRESS: AtomicU64 = AtomicU64::new(0);
static MENU_SINCE: AtomicU64 = AtomicU64::new(0);
/// The triggers' values (f32 bits), and whether both were released since the last pull.
static TRIGGER: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static ARMED: AtomicBool = AtomicBool::new(false);
static CALIBRATION: Mutex<Calibration> = Mutex::new(CALIBRATION_DEFAULT);

/// Step 3's cross and the last frame's view of it (for the helpers).
struct CrossState {
    cross: Cross,
    view: CrossView,
}
static CROSS: Mutex<CrossState> = Mutex::new(CrossState { cross: CROSS_ZERO, view: CrossView::EMPTY });

/// What the steps keep between frames (the trackers' thread alone).
struct Tick {
    /// Step 1's head, looking ahead at the horizon line: the capture's eye height, head reference and origin come from
    /// it, not from the head at step 2's pull -- that step looks down at the trackers, and a head pitched 60-70 deg
    /// drops the eyes some 20 cm and moves them forward.
    level_head: xr::Posef,
    /// When all three marks were reached (0 = not held), and the widest span's arm length over this hold (0 = none).
    hold_start: u64,
    best_arm: f32,
}
static TICK: Mutex<Tick> = Mutex::new(Tick { level_head: IDENTITY, hold_start: 0, best_arm: 0.0 });

/// V's skeleton as read: whether the calibrator is ready, the rig file it came from.
struct Rig {
    ready: bool,
    path: [u8; MAX_PATH],
}
static RIG: Mutex<Rig> = Mutex::new(Rig { ready: false, path: [0; MAX_PATH] });
/// The rig's absence, logged at most every 10 s (the body solve asks every frame).
static RIG_ABSENT_LOG: AtomicU64 = AtomicU64::new(0);

/// The steps' instructions and the refusals, until the game's translations replace them (VRFbtSetCalibrationText).
const DEFAULT_TEXT: [&CStr; TEXT_SLOTS as usize] = [
    c"",
    c"Step 1: look ahead at the horizon line, then press both triggers",
    c"Step 2: arms down, look down, then press both triggers",
    c"Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends",
    c"",
    c"Not taken: look straight at the horizon line until it turns green, then press again",
    c"Not taken: stretch both arms straight out along the cross, then hold still again",
    c"Not taken: stand straight with the headset, trackers and controllers tracked, then press again",
];
static TEXTS: Mutex<[Option<CString>; TEXT_SLOTS as usize]> = Mutex::new([const { None }; TEXT_SLOTS as usize]);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn now() -> u64 {
    // SAFETY: plain Win32 call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn player_identity() -> usize {
    // SAFETY: cvr-rig's atomic read.
    unsafe { cvr_roomscale_player_identity() }
}
fn refuse(message: i32) {
    REFUSED.store(message, Relaxed);
    REFUSED_UNTIL.store(now() + 3000, Relaxed);
}
fn accept() {
    REFUSED_UNTIL.store(0, Relaxed);
}
fn progress() {
    LAST_PROGRESS.store(now(), Relaxed);
}
fn trigger(hand: usize) -> f32 {
    f32::from_bits(TRIGGER[hand].load(Relaxed))
}
fn ok(valid: bool, missing: &'static str) -> &'static str {
    if valid { "ok" } else { missing }
}

/// V's reference pose, once her rig is read.
pub(crate) fn read_reference() -> Option<Reference> {
    // SAFETY: plain data, all zero; the read fills it whole when it succeeds.
    let mut r: Reference = unsafe { core::mem::zeroed() };
    // SAFETY: a local to fill.
    unsafe { VRIK_ReadFbtReference(&mut r) }.then_some(r)
}

/// ToRoom: an OpenXR STAGE pose in V's model axes (x right, y forward, z up), its rotation normalized as the IK
/// library's Normalize does.
fn to_room(valid: bool, p: &xr::Posef) -> RoomPose {
    let o = &p.orientation;
    let q = [o.x, -o.z, o.y, o.w];
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let rot = if l > 1e-9 { [q[0] / l, q[1] / l, q[2] / l, q[3] / l] } else { [0.0, 0.0, 0.0, 1.0] };
    RoomPose { valid, pos: [p.position.x, -p.position.z, p.position.y], rot, wrist: false }
}
/// The calibration's devices from the trackers and the controllers.
fn devices(poses: &[Pose; 3], hands: &[Pose; 2]) -> [RoomPose; CAL_COUNT] {
    let mut d = [RoomPose::default(); CAL_COUNT];
    d[CAL_WAIST] = to_room(poses[WAIST].stage_valid, &poses[WAIST].stage);
    d[CAL_LEFT_FOOT] = to_room(poses[LEFT_FOOT].stage_valid, &poses[LEFT_FOOT].stage);
    d[CAL_RIGHT_FOOT] = to_room(poses[RIGHT_FOOT].stage_valid, &poses[RIGHT_FOOT].stage);
    d[CAL_LEFT_HAND] = to_room(hands[0].stage_valid, &hands[0].stage);
    d[CAL_RIGHT_HAND] = to_room(hands[1].stage_valid, &hands[1].stage);
    d
}
/// The head in STAGE, when both its position and orientation are tracked.
fn locate_head(view: u64, stage: u64, time: i64) -> Option<xr::Posef> {
    if stage == 0 || view == 0 {
        return None;
    }
    // SAFETY: plain data; the type set below.
    let mut loc: xr::SpaceLocation = unsafe { core::mem::zeroed() };
    loc.ty = xr::StructureType::SPACE_LOCATION;
    // SAFETY: the session's spaces and a local location.
    let r = unsafe { xrLocateSpace(xr::Space::from_raw(view), xr::Space::from_raw(stage), xr::Time::from_nanos(time), &mut loc) };
    let need = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
    (r.into_raw() >= 0 && loc.location_flags.contains(need)).then_some(loc.pose)
}
fn pos3(p: &xr::Posef) -> V3 {
    [p.position.x, p.position.y, p.position.z]
}
fn rot4(p: &xr::Posef) -> Q4 {
    [p.orientation.x, p.orientation.y, p.orientation.z, p.orientation.w]
}
fn arm_scale(c: &Calibration, side: usize) -> f32 {
    let reference = cvr_cpp_fbt_avatar_arm(side as i32);
    if c.arm_length[side] > 0.0 && reference > 0.0 { c.arm_length[side] / reference } else { 1.0 }
}
fn c_text(p: *const c_char) -> String {
    // SAFETY: a terminated string (or null).
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

fn log_reference(v: &Reference) {
    let elevation = |side: usize| -fx::arm_elevation_deg(v.upper_arm_pos[side], v.fore_arm_pos[side]);
    log(&format!(
        "[fbt-calib] V reference ({} rig, model space x right y fwd z up): eye=({:+.3} {:+.3} {:+.3}) hips=({:+.3} {:+.3} {:+.3}) \
         feet=({:+.3} {:+.3} {:+.3})/({:+.3} {:+.3} {:+.3}); arms below horizontal {:.1} / {:.1} deg (0 = T pose)\n",
        if v.female { "female" } else { "male" },
        v.eye[0],
        v.eye[1],
        v.eye[2],
        v.hips_pos[0],
        v.hips_pos[1],
        v.hips_pos[2],
        v.foot_pos[0][0],
        v.foot_pos[0][1],
        v.foot_pos[0][2],
        v.foot_pos[1][0],
        v.foot_pos[1][1],
        v.foot_pos[1][2],
        elevation(0),
        elevation(1)
    ));
}

/// The calibration written to config\fbt_calibration.ini.
fn save(c: &Calibration) {
    // SAFETY: a terminated folder kept for the process.
    let mut bytes = unsafe { CStr::from_ptr(cvr_paths_dir_a(4)) }.to_bytes().to_vec();
    bytes.extend_from_slice(b"fbt_calibration.ini");
    let shown = String::from_utf8_lossy(&bytes).into_owned();
    let Ok(path) = CString::new(bytes) else { return };
    if !cvr_settings::fbt_calibration_file::save(&path, c) {
        log(&format!("[fbt-calib] could not write {shown}\n"));
        return;
    }
    log(&format!("[fbt-calib] saved {shown}\n"));
}

/// V's skeleton and the calibrator over it, read once per rig file; false while her rig is not loaded or unreadable.
fn prepare_calibrator() -> bool {
    let mut rig = lock(&RIG);
    let mut path = [0u8; MAX_PATH];
    let found = match read_reference() {
        // SAFETY: a local of MAX_PATH bytes.
        Some(r) => unsafe { VRIK_FbtRigPath(r.female, path.as_mut_ptr().cast(), MAX_PATH) },
        None => false,
    };
    if !found {
        if now().wrapping_sub(RIG_ABSENT_LOG.load(Relaxed)) > 10000 {
            RIG_ABSENT_LOG.store(now(), Relaxed);
            log("[fbt-calib] V's rig is not loaded yet\n");
        }
        return false;
    }
    let len = path.iter().position(|&b| b == 0).unwrap_or(MAX_PATH - 1);
    let shown = String::from_utf8_lossy(&path[..len]).into_owned();
    if rig.ready && rig.path[..] == path[..] {
        return true;
    }
    let mut error = [0u8; 512];
    // SAFETY: a terminated path and a local for the error.
    let bones = unsafe { cvr_cpp_fbt_load_rig(path.as_ptr().cast(), error.as_mut_ptr().cast(), error.len()) };
    if bones == -1 {
        let why = c_text(error.as_ptr().cast());
        log(&format!("[fbt-calib] cannot read {shown}: {why}\n"));
        return false;
    }
    if bones < 0 {
        log(&format!("[fbt-calib] {shown} lacks bones the calibration needs\n"));
        rig.ready = false;
        return false;
    }
    rig.path = path;
    rig.ready = true;
    log(&format!("[fbt-calib] V's skeleton: {shown} ({bones} bones)\n"));
    true
}

/// STEP 3, THE CROSS. Set when step 2 is taken: centred on the midpoint between the feet trackers and square to the
/// line between them, V's proportions at the player's size (cvr-body's fbt_cross); false without V's reference.
fn set_up_cross(c: &Calibration, left_foot: &Pose, right_foot: &Pose, t: &mut Tick) -> bool {
    let Some(r) = read_reference() else { return false };
    if c.avatar_eye_height <= 0.1 || c.player_eye_height <= 0.1 {
        return false;
    }
    let feet = [left_foot.stage_valid.then(|| pos3(&left_foot.stage)), right_foot.stage_valid.then(|| pos3(&right_foot.stage))];
    let Some(frame) = fx::cross_frame(pos3(&t.level_head), rot4(&t.level_head), feet) else { return false };
    const FROM: [&str; 3] = ["the head (no feet trackers)", "the feet (too close for their line: the gaze's heading)", "the feet"];
    let from = FROM[(frame.from as usize).min(2)];
    if left_foot.stage_valid && right_foot.stage_valid {
        log(&format!("[fbt-calib] step 3 cross from {from}: step 1's head was {:+.3} m sideways of the feet's midpoint\n", frame.off));
    } else {
        log(&format!("[fbt-calib] step 3 cross from {from}\n"));
    }
    let shoulders_z = [r.upper_arm_pos[0][2], r.upper_arm_pos[1][2]];
    let shoulders_x = [r.upper_arm_pos[0][0], r.upper_arm_pos[1][0]];
    let arms = [cvr_cpp_fbt_avatar_arm(0), cvr_cpp_fbt_avatar_arm(1)];
    let (base, tip_y, bar_y, half_span) = fx::cross_dims(&frame, c.player_eye_height, c.avatar_eye_height, shoulders_z, shoulders_x, arms);
    let x = Cross { spot: frame.spot, fwd: frame.fwd, right: frame.right, base, tip_y, bar_y, half_span };
    {
        let mut g = lock(&CROSS);
        g.cross = x;
        g.view = CrossView::EMPTY;
    }
    t.hold_start = 0;
    t.best_arm = 0.0;
    log(&format!(
        "[fbt-calib] step 3 cross: {:.2} m ahead, top {:.3} m, arms at {:.3} m, half span {:.3} m (V's at the player's size)\n",
        fx::CROSS_AHEAD,
        x.tip_y,
        x.bar_y,
        x.half_span
    ));
    true
}

/// Step 3 each frame: the marks, the hold's countdown, the widest span's arm length, and the arms taken when the hold
/// ends.
fn cross_tick(view: u64, stage: u64, time: i64, hands: &[Pose; 2], devices: &[RoomPose; CAL_COUNT], t: &mut Tick) {
    let x = lock(&CROSS).cross;
    let mut out = CrossView::EMPTY;
    out.base = x.base;
    out.right = x.right;
    out.tip_y = x.tip_y;
    out.bar_y = x.bar_y;
    out.half_span = x.half_span;
    let head = locate_head(view, stage, time);
    fx::cross_marks(
        &x,
        head.map(|p| (pos3(&p), rot4(&p))),
        [hands[0].stage_valid.then(|| pos3(&hands[0].stage)), hands[1].stage_valid.then(|| pos3(&hands[1].stage))],
        &mut out.dot,
        &mut out.state,
    );
    let all = out.state == [2, 2, 2];
    let now = now();
    if !all {
        if t.hold_start != 0 {
            log(&format!(
                "[fbt-calib] step 3: the pose was left (head {}, left hand {}, right hand {}): the countdown restarts\n",
                out.state[0], out.state[1], out.state[2]
            ));
        }
        t.hold_start = 0;
        t.best_arm = 0.0;
    } else {
        if t.hold_start == 0 {
            t.hold_start = now;
            log("[fbt-calib] step 3: on the cross, countdown\n");
        }
        progress();
        // The widest span over the hold: the arms' length out of the hands' span at the shoulders' height.
        let mut c = *lock(&CALIBRATION);
        if let Some(h) = head {
            let room = to_room(true, &h);
            // SAFETY: a local calibration, the head and the five devices.
            if unsafe { cvr_cpp_fbt_measure_arms_out(&mut c, &room, devices.as_ptr()) } && c.arm_length[0] > t.best_arm {
                t.best_arm = c.arm_length[0];
            }
        }
        let held = now.wrapping_sub(t.hold_start) as f32 / 1000.0;
        let left = HOLD_SECONDS - held;
        out.countdown = if 0.0 < left { left } else { 0.0 };
        if held >= HOLD_SECONDS {
            t.hold_start = 0;
            if t.best_arm <= 0.0 {
                log(
                    "[fbt-calib] step 3: the span between the hands was never a T-pose's (hands not side by side at the \
                     shoulders, or the arms' length out of 0.6..1.5 x V's): hold it again\n",
                );
                refuse(REFUSE_ARM);
            } else {
                {
                    let mut g = lock(&CALIBRATION);
                    g.arm_length = [t.best_arm; 2];
                    c = *g;
                }
                accept();
                log(&format!(
                    "[fbt-calib] step 3 captured: arms {:.3} m each (V's scale, from the widest span) -> V's arms x {:.3} / x {:.3}. \
                     Calibrated\n",
                    t.best_arm,
                    arm_scale(&c, 0),
                    arm_scale(&c, 1)
                ));
                save(&c);
                STEP.store(0, Relaxed);
                log("[fbt-calib] calibration mode off\n");
            }
            t.best_arm = 0.0;
        }
    }
    lock(&CROSS).view = out;
}

fn step_tick(view: u64, stage: u64, time: i64, poses: &[Pose; 3], hands: &[Pose; 2], t: &mut Tick) {
    let step = STEP.load(Relaxed);
    if step == 3 {
        // the T-pose: every frame, no pull
        cross_tick(view, stage, time, hands, &devices(poses, hands), t);
        return;
    }
    let (l, r) = (trigger(0), trigger(1));
    if !ARMED.load(Relaxed) {
        ARMED.store(l < TRIGGER_OFF && r < TRIGGER_OFF, Relaxed);
        return;
    }
    if l < TRIGGER_ON || r < TRIGGER_ON {
        return;
    }
    ARMED.store(false, Relaxed); // one pull, one action
    let Some(head_stage) = locate_head(view, stage, time) else {
        log("[fbt-calib] head not tracked in STAGE: pull again\n");
        refuse(REFUSE_TRACKING);
        return;
    };
    let devices = devices(poses, hands);
    if step == 1 {
        let pitch = fx::head_pitch_deg(rot4(&head_stage));
        if pitch.abs() > LEVEL_DEG {
            log(&format!(
                "[fbt-calib] step 1: the gaze is {pitch:.0} deg off level -- look at the horizon line (it turns green), pull again\n"
            ));
            refuse(REFUSE_LEVEL);
            return;
        }
        t.level_head = head_stage;
        accept();
        progress();
        log(&format!(
            "[fbt-calib] step 1 captured: eye height {:.3} m, gaze {pitch:+.1} deg. Now arms along the body, look down, both triggers\n",
            head_stage.position.y
        ));
        STEP.store(2, Relaxed);
        return;
    }
    if step == 2 {
        if !prepare_calibrator() {
            return;
        }
        // THE BODY FACES THE WAY THE FEET FACE, AND STANDS OVER THEM: with both feet tracked and apart, the capture
        // takes step 1's head as looking straight along the feet (its yaw turned onto theirs; pitch and roll kept) and
        // over their midpoint sideways; forwards and in height it stays as step 1 took it.
        let mut head = to_room(true, &t.level_head);
        if devices[CAL_LEFT_FOOT].valid && devices[CAL_RIGHT_FOOT].valid {
            let frame = fx::feet_frame(head.rot, head.pos, devices[CAL_LEFT_FOOT].pos, devices[CAL_RIGHT_FOOT].pos);
            head.rot = frame.rot;
            head.pos = frame.pos;
            if frame.turned {
                log(&format!(
                    "[fbt-calib] step 2: the gaze was {:+.1} deg off the feet's heading: the body faces the feet\n",
                    -frame.turn * 57.29578f32
                ));
            }
            if frame.shifted {
                log(&format!(
                    "[fbt-calib] step 2: the head was {:+.3} m sideways of the feet's midpoint (- left): taken over the feet\n",
                    -frame.side
                ));
            }
        }
        let mut c = CALIBRATION_DEFAULT;
        let mut why: *const c_char = core::ptr::null();
        // SAFETY: the head, the five devices and locals for the capture.
        if !unsafe { cvr_cpp_fbt_capture(&head, devices.as_ptr(), &mut c, &mut why) } {
            let why = if why.is_null() { "?".to_owned() } else { c_text(why) };
            log(&format!("[fbt-calib] step 2 not captured: {why}; pull again\n"));
            refuse(REFUSE_TRACKING);
            return;
        }
        accept();
        progress();
        c.arm_length = [0.0; 2]; // a hanging length is not a scale: step 3 measures the arms
        *lock(&CALIBRATION) = c;
        log(&format!(
            "[fbt-calib] step 2 captured: player eye height {:.3} m, V's {:.3} m -> scale {:.4}; waist {}, feet {}/{}, hands {}/{}. \
             Now the T-pose on the cross\n",
            c.player_eye_height,
            c.avatar_eye_height,
            c.scale,
            ok(c.valid[CAL_WAIST], "MISSING"),
            ok(c.valid[CAL_LEFT_FOOT], "MISSING"),
            ok(c.valid[CAL_RIGHT_FOOT], "MISSING"),
            ok(c.valid[CAL_LEFT_HAND], "missing"),
            ok(c.valid[CAL_RIGHT_HAND], "missing")
        ));
        if !set_up_cross(&c, &poses[LEFT_FOOT], &poses[RIGHT_FOOT], t) {
            log("[fbt-calib] V's reference is not available for the cross: arms not measured, V's kept. Calibrated\n");
            save(&c);
            STEP.store(0, Relaxed);
            return;
        }
        STEP.store(3, Relaxed);
    }
}

/// CalibrationTick: the frame's step while a calibration runs (cvr-xr's trackers call it), or its cancellation.
///
/// # Safety
/// `trackers` holds the three roles' poses and `hands` the two controllers'.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_fbt_calibration_tick(view: u64, stage: u64, time: i64, trackers: *const Pose, hands: *const Pose) {
    let now = now();
    let player = player_identity();
    // SAFETY: cvr-rig's volatile int.
    let menu = unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) } != 0;
    let since = MENU_SINCE.load(Relaxed);
    let since = if menu { if since != 0 { since } else { now } } else { 0 };
    MENU_SINCE.store(since, Relaxed);
    let why = if player != PLAYER.load(Relaxed) {
        Some("the player changed (a load or the main menu)")
    } else if since != 0 && now.wrapping_sub(since) > 2000 {
        Some("a game menu is open")
    } else if now.wrapping_sub(LAST_PROGRESS.load(Relaxed)) > 120000 {
        Some("2 minutes without a step")
    } else {
        None
    };
    if let Some(why) = why {
        log(&format!("[fbt-calib] calibration cancelled: {why}\n"));
        cvr_cpp_fbt_cancel_calibration();
        return;
    }
    if trackers.is_null() || hands.is_null() {
        return;
    }
    // SAFETY: the caller's poses.
    let (poses, hands) = unsafe { (&*trackers.cast::<[Pose; 3]>(), &*hands.cast::<[Pose; 2]>()) };
    step_tick(view, stage, time, poses, hands, &mut lock(&TICK));
}

/// CalibrationAvailable: in game (a save loaded), never in the VR lobby.
pub(crate) extern "C" fn calibration_available() -> bool {
    !crate::menu_lobby::cvr_menu_lobby_active() && player_identity() != 0
}
/// StartCalibration: step 1, with V's skeleton read now so a missing rig shows before the first pull.
pub(crate) extern "C" fn start_calibration() {
    if !calibration_available() {
        log("[fbt-calib] not started: the full-body calibration runs in game only (load a save)\n");
        return;
    }
    if !cvr_xr::trackers::cvr_trackers_running() {
        log("[fbt-calib] Vive trackers are off (xr_vive_trackers=0): nothing to calibrate\n");
        return;
    }
    ARMED.store(false, Relaxed);
    accept();
    PLAYER.store(player_identity(), Relaxed);
    MENU_SINCE.store(0, Relaxed);
    progress();
    prepare_calibrator();
    STEP.store(1, Relaxed);
    log("[fbt-calib] calibration step 1: look ahead at the horizon line; release then pull both triggers\n");
    match read_reference() {
        Some(v) => log_reference(&v),
        None => log("[fbt-calib] V's reference pose is not available yet\n"),
    }
}
/// CancelCalibration: off; in step 3 the calibration step 2 took is saved (an arm not measured keeps V's length).
#[no_mangle]
pub extern "C" fn cvr_cpp_fbt_cancel_calibration() {
    let step = STEP.swap(0, Relaxed);
    if step == 1 || step == 2 {
        log("[fbt-calib] calibration cancelled\n");
    }
    if step == 3 {
        let c = *lock(&CALIBRATION);
        save(&c);
        log("[fbt-calib] arms step cancelled: an arm not measured keeps V's length\n");
    }
}
/// Calibrating.
#[no_mangle]
pub extern "C" fn cvr_cpp_fbt_calibrating() -> bool {
    STEP.load(Relaxed) != 0
}
/// CalibrationStep: 0 off, 1 head, 2 body, 3 arms.
#[no_mangle]
pub extern "C" fn cvr_cpp_fbt_calibration_step() -> i32 {
    STEP.load(Relaxed)
}
/// NoteTrigger: a controller's trigger (0 left, 1 right) for the steps' pulls.
#[no_mangle]
pub extern "C" fn cvr_cpp_fbt_note_trigger(hand: i32, value: f32) {
    if hand == 0 || hand == 1 {
        TRIGGER[hand as usize].store(value.to_bits(), Relaxed);
    }
}
/// ReadCross: the last frame's cross while step 3 runs.
pub(crate) extern "C" fn read_cross(out: *mut CrossView) -> bool {
    if STEP.load(Relaxed) != 3 || out.is_null() {
        return false;
    }
    let g = lock(&CROSS);
    // SAFETY: the caller's cross.
    unsafe { *out = g.view };
    true
}
fn step_text(slot: i32) -> CString {
    if !(1..TEXT_SLOTS).contains(&slot) {
        return CString::default();
    }
    let texts = lock(&TEXTS);
    texts[slot as usize].clone().unwrap_or_else(|| DEFAULT_TEXT[slot as usize].to_owned())
}
fn refusal_text() -> CString {
    if !cvr_cpp_fbt_calibrating() || now() >= REFUSED_UNTIL.load(Relaxed) {
        return CString::default();
    }
    step_text(REFUSED.load(Relaxed))
}
thread_local! {
    static HELD: RefCell<[CString; 2]> = RefCell::new([CString::default(), CString::default()]);
}
/// The step's text, or the refusal for a negative slot, held per thread until the next call.
pub(crate) extern "C" fn text(slot: i32) -> *const c_char {
    let s = if slot < 0 { refusal_text() } else { step_text(slot) };
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        let i = if slot < 0 { 1 } else { 0 };
        held[i] = s;
        held[i].as_ptr()
    })
}
/// SetStepText: the game's translation of a step's instruction or a refusal.
///
/// # Safety
/// `text` is a C string or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_set_step_text(slot: i32, text: *const c_char) {
    // SAFETY: the caller's string.
    if !(1..TEXT_SLOTS).contains(&slot) || text.is_null() || unsafe { *text } == 0 {
        return;
    }
    // SAFETY: as above.
    let s = unsafe { CStr::from_ptr(text) }.to_owned();
    lock(&TEXTS)[slot as usize] = Some(s);
}
/// ReadCalibration: the calibration once step 2 took one.
///
/// # Safety
/// `out` is a calibration or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_read_calibration(out: *mut Calibration) -> bool {
    let c = lock(&CALIBRATION);
    if out.is_null() || !c.done {
        return false;
    }
    // SAFETY: the caller's calibration.
    unsafe { *out = *c };
    true
}
/// The calibrator ready for V's current rig (Body.cpp's Rig, under its own lock).
#[no_mangle]
pub extern "C" fn cvr_fbt_prepare_calibrator() -> bool {
    prepare_calibrator()
}
/// ToRoom for the body solve.
///
/// # Safety
/// `pose` is a pose, `out` a room pose.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_to_room(valid: bool, pose: *const xr::Posef, out: *mut RoomPose) {
    // SAFETY: as documented.
    unsafe { *out = to_room(valid, &*pose) };
}
