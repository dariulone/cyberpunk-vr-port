//! Hands on the steering wheel (formerly src/Anim/WheelGrab.cpp): bring a hand to where the driving animation holds
//! the wheel, squeeze, and that arm is handed back to the animation; release and it returns to the controller. The
//! hub is the horn, the hands' own rotation steers (crate::wheel), and the hand off the wheel gets the relaxed finger
//! pose latched on foot.
//!
//! main's since 2026-11 (0.2.4): ANYWHERE ON THE RIM (xr_wheel_anywhere, cars) -- a hand takes hold where it is and
//! rides the drawn wheel, on its steering_wheel slot when script gives it; A MOTORCYCLE'S BARS are pushed and pulled
//! (crate::wheel's handlebar) and the body's lean adds to them; AN OWNING HAND KEEPS THE WHEEL through the session's
//! paused input and a moment's lost tracking; a grip squeezed elsewhere and carried to the wheel is not the wheel's;
//! and the wheel publishes where the projected map hangs (cvr_body::vehicle_minimap).
use super::env::*;
use super::gestures::now_ms;
use super::snapshot::SharedPose;
use super::weapon_arm::cvr_anim_weapon_in_left_hand;
use super::*;
use crate::math::quat_norm;
use crate::math::{quat_conj, quat_mul, quat_rotate_vec};
use crate::wheel::{
    bike_bar_lock_degrees, car_input_for_cabin_angle, wheel_full_lock_for_vehicle, wheel_gamepad_axis, TrackedWheelSteering, WheelTrackingFrame,
    CAR_CABIN_WHEEL_LOCK_DEG,
};
use cvr_body::vehicle_minimap::{build_vehicle_minimap_pose, fresh_vehicle_minimap, VehicleMinimapAnchor};
use cvr_body::xr::{Quat as XQuat, Vec3 as XVec3};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// The steering settings of g_liveControls (src/Core/LiveControls), read through src/Anim/CharacterRigRust.cpp.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct WheelControls {
    pub grab: i32,
    pub radius: f32,
    pub steer_max_deg: f32,
    pub steer_dead_deg: f32,
    pub prediction: i32,
    pub prediction_ms: f32,
    pub horn: i32,
    pub horn_radius: f32,
}

extern "C" {
    static mut g_hasWeaponEquipped: bool;
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    // What the rest of the plugin reads (src/Anim/CharacterRigState.cpp): std::atomic<float> / <int>.
    static g_wheelBlendRight: AtomicU32;
    static g_wheelBlendLeft: AtomicU32;
    static g_wheelSteer: AtomicU32;
    static g_wheelSteerDeg: AtomicU32;
    static g_wheelHornMask: AtomicI32;
    /// The wheel's microsecond clock for tests; null = XrDiagNowUs (QPC microseconds).
    static g_VRIKClockUs: Option<unsafe extern "C" fn() -> u64>;
    /// The wheel's tracking sample for tests; null = OpenXRManager::GetGestureHandFrame.
    static g_VRIKWheelFrame: Option<unsafe extern "C" fn(*mut WheelTrackingFrame) -> bool>;
    fn cvr_cpp_wheel_driving() -> bool;
    /// g_menuModeValue != 0 || OverlayIsVisible().
    fn cvr_cpp_wheel_ui_blocks() -> bool;
    fn cvr_cpp_wheel_tracking_frame(out: *mut WheelTrackingFrame) -> bool;
    /// CurrentMountedVehicleIdentity (cvr-ffi player_camera_state.rs).
    fn cvr_mounted_vehicle_identity() -> usize;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    /// OpenXRManager::GetRecenterBase's mirror (cvr-xr tracking.rs): orientation x y z w, position x y z.
    fn cvr_xr_base_mirror() -> *const [AtomicU32; 7];
    static g_menuModeValue: i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

/// Test read-out of the steering, per XInput sample: output, hands' angle, bike lean, grab bits | epoch << 2, control
/// state, suspended samples so far, engaged right, engaged left.
#[no_mangle]
pub static mut CyberpunkVR_WheelSteerDebug: [f32; 8] = [0.0; 8];

const SLOT_COUNT: i32 = 256;
const LEFT_GRIP_PRESSED: i32 = 155;
const WHEEL_ARMED_MASK: usize = 163;
const ARMED_RIGHT_BIT: i32 = 1;
const ARMED_LEFT_BIT: i32 = 2;

const HORN_RADIUS_DEFAULT: f32 = 0.12;
const HORN_RADIUS_MIN: f32 = 0.04;
const HORN_RADIUS_MAX: f32 = 0.30;
const HORN_HYSTERESIS: f32 = 0.03;
const ENGAGE_SEC: f32 = 0.16;
const RELEASE_SEC: f32 = 0.11;
const FULL_BLEND: f32 = 0.995;

#[derive(Clone, Copy)]
struct WheelHand {
    blend: f32,
    engaged: bool,
    at_wheel: bool,
    at_hub: bool,
    grip_prev: bool,
    /// The grip was already held when the hand came to the wheel: it belongs elsewhere.
    grip_foreign: bool,
    /// GetTickCount64 when this controller's pose went invalid, 0 while tracked.
    untracked_since: u64,
    target_valid: bool,
    target: V3,
    anim_pos: V3,
    anim_rot: Q,
    anim_valid: bool,
    target_frame: u64,
    /// The animated hand with nothing held: how a hand lies on the rim.
    ref_valid: bool,
    ref_pos: V3,
    ref_rot: Q,
    /// This grab keeps to the rim where it took hold: radians round the wheel from ref_pos then, how far the drawn wheel
    /// had turned then, where on the rim (radians from 3 o'clock).
    on_rim: bool,
    rim_turn: f32,
    rim_wheel: f32,
    rim_angle0: f32,
    /// The hand's place on the cabin wheel, in its slot's frame, taken this grab.
    welded: bool,
    weld_pos: V3,
    weld_rot: Q,
}
const HAND0: WheelHand = WheelHand {
    blend: 0.0,
    engaged: false,
    at_wheel: false,
    at_hub: false,
    grip_prev: false,
    grip_foreign: false,
    untracked_since: 0,
    target_valid: false,
    target: [0.0; 3],
    anim_pos: [0.0; 3],
    anim_rot: ID,
    anim_valid: false,
    target_frame: 0,
    ref_valid: false,
    ref_pos: [0.0; 3],
    ref_rot: ID,
    on_rim: false,
    rim_turn: 0.0,
    rim_wheel: 0.0,
    rim_angle0: 0.0,
    welded: false,
    weld_pos: [0.0; 3],
    weld_rot: ID,
};
/// [0] right, [1] left.
static mut WHEEL: [WheelHand; 2] = [HAND0; 2];
/// The hub (midpoint of the two animated hands), frozen for the whole grab, and the wheel's span as animated.
static mut CENTER: V3 = [0.0; 3];
static mut CENTER_VALID: bool = false;
static mut SPAN: f32 = 0.0;
struct Steer {
    steering: TrackedWheelSteering,
    sampled_grab_epoch: u64,
}
static STEER: Mutex<Steer> = Mutex::new(Steer {
    steering: TrackedWheelSteering::new_const(),
    sampled_grab_epoch: 0,
});
static mut WHEEL_FRAME: u64 = 0;
/// Low two bits ownership, the rest a grab epoch.
static GRAB_STATE: AtomicU64 = AtomicU64::new(0);
static PHYSICAL_RADIUS: AtomicU32 = AtomicU32::new(0x3E42_8F5C); // 0.19f
/// Where on the rim each hand holds (radians from 3 o'clock), for the steering.
static RIM_ANGLE: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

// ---- the map over the hood: the anchor, published each solve, read by the HUD
static MINIMAP: Mutex<(VehicleMinimapAnchor, VehicleMinimapAnchor)> =
    Mutex::new((VehicleMinimapAnchor { orientation: XQuat::IDENTITY, position: XVec3 { x: 0.0, y: 0.0, z: 0.0 }, stamp: 0, origin: 0, vehicle: 0, bike: false },
                VehicleMinimapAnchor { orientation: XQuat::IDENTITY, position: XVec3 { x: 0.0, y: 0.0, z: 0.0 }, stamp: 0, origin: 0, vehicle: 0, bike: false }));
static mut MINIMAP_VEHICLE: usize = 0;

// ---- the cabin wheel itself (SetVRWheelCabin): model space, and when script last said it
struct Cabin {
    pos: V3,
    rot: Q,
    at: u64,
}
static CABIN: Mutex<Cabin> = Mutex::new(Cabin { pos: [0.0; 3], rot: ID, at: 0 });
const CABIN_FRESH_MS: u64 = 250;

// ---- the drawn wheel's turn (the line between the animation's hands, added up frame by frame)
const WHEEL_JUMP: f32 = 0.35; // radians in a frame
static mut WHEEL_TURN: f32 = 0.0;
static mut WHEEL_LINE: f32 = 0.0;
static mut WHEEL_LINE_VALID: bool = false;
static mut TURN_SEEN: f32 = 0.0;
static mut STEER_SEEN: f32 = 0.0;
static mut ANY_HELD: bool = false;

/// THE HEADSET'S INPUT IS AWAY: the XR session is not focused (a runtime overlay, a long hitch with the compositor's own
/// scene up), or got the focus back a moment ago. Its actions are inactive then -- grips read released, poses invalid --
/// and that is not the player letting go: an owning hand keeps the wheel until the session has had its focus back for
/// REFOCUS_MS (reported 2026-11-17 on a motorcycle: after freezes the steering went neutral and stayed wrong).
const REFOCUS_MS: u64 = 300;
static FOCUSED_SINCE: AtomicU64 = AtomicU64::new(0);
unsafe fn input_away() -> bool {
    if !super::xr_calls::session_focused() {
        FOCUSED_SINCE.store(0, Ordering::Relaxed);
        return true;
    }
    let now = tick_ms();
    let since = match FOCUSED_SINCE.load(Ordering::Relaxed) {
        0 => {
            FOCUSED_SINCE.store(now, Ordering::Relaxed);
            now
        }
        s => s,
    };
    now - since < REFOCUS_MS
}
/// A CONTROLLER OUT OF TRACKING FOR A MOMENT with its grip still closed is not a release either: the owning hand keeps
/// the wheel for TRACK_GAP_MS, the steering holding its turn meanwhile.
const TRACK_GAP_MS: u64 = 1000;
unsafe fn hand_tracked(w: &mut WheelHand, h: usize, keep: bool) -> bool {
    let pose = SharedPose(if h == 0 { 8 } else { 0 }) > 0.0;
    let now = tick_ms();
    if pose {
        w.untracked_since = 0;
    } else if w.untracked_since == 0 {
        w.untracked_since = now;
    }
    pose || keep || (w.engaged && now - w.untracked_since < TRACK_GAP_MS)
}

// ANYWHERE ON THE RIM. The wheel is a circle: its centre and radius from the two animated hands, in a plane through the
// line between them, leaning back from upright -- a fixed look at a car's wheel rather than a measurement.
const WHEEL_LEAN: f32 = 25.0 * 0.017_453_292_52;
const RIM_BAND: f32 = 0.11; // m from the rim, in the wheel's plane
const RIM_DEPTH: f32 = 0.14; // m in front of or behind it
const RIM_CENTRE: f32 = 0.03; // m: a hand this near the hub has no direction round it

#[derive(Clone, Copy, Default)]
struct WheelFrame {
    across: V3,
    up: V3,
    normal: V3,
}
fn dot3(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
unsafe fn wheel_frame_now() -> Option<WheelFrame> {
    if !CENTER_VALID || !WHEEL[0].ref_valid || !WHEEL[1].ref_valid {
        return None;
    }
    let mut a = [WHEEL[0].ref_pos[0] - WHEEL[1].ref_pos[0], WHEEL[0].ref_pos[1] - WHEEL[1].ref_pos[1], 0.0];
    // level: the animation's hands need not be, and model Z is the car's up
    let len = (a[0] * a[0] + a[1] * a[1]).sqrt();
    if len < 0.1 {
        return None;
    }
    a[0] /= len;
    a[1] /= len;
    // model space: X right, Y forward, Z up. The face of the wheel looks back at the driver and a little up.
    let forward = [-a[1], a[0]];
    let (c, s) = (WHEEL_LEAN.cos(), WHEEL_LEAN.sin());
    let n = [-forward[0] * c, -forward[1] * c, s];
    Some(WheelFrame {
        across: a,
        normal: n,
        up: [n[1] * a[2] - n[2] * a[1], n[2] * a[0] - n[0] * a[2], n[0] * a[1] - n[1] * a[0]],
    })
}
/// Where a point is on the wheel: radians from 3 o'clock anticlockwise, how far from the hub in its plane, how far off.
unsafe fn wheel_coords(f: &WheelFrame, p: &V3) -> (f32, f32, f32) {
    let d = [p[0] - CENTER[0], p[1] - CENTER[1], p[2] - CENTER[2]];
    let (x, y) = (dot3(&d, &f.across), dot3(&d, &f.up));
    ((y).atan2(x), (x * x + y * y).sqrt(), dot3(&d, &f.normal))
}
unsafe fn rim_grab_mode() -> bool {
    cvr_settings::lc!(xrWheelAnywhere) != 0 && MOUNTED_CAR.load(Ordering::Relaxed)
}
/// HOW FAR THE DRAWN WHEEL HAS TURNED (not the controller): the turn of the line between the animation's two hands,
/// added up frame by frame; a jump is the animation changing its grip.
unsafe fn track_drawn_wheel() {
    let Some(frame) = (if WHEEL[0].anim_valid && WHEEL[1].anim_valid { wheel_frame_now() } else { None }) else {
        WHEEL_LINE_VALID = false;
        return;
    };
    let d = [WHEEL[0].anim_pos[0] - WHEEL[1].anim_pos[0], WHEEL[0].anim_pos[1] - WHEEL[1].anim_pos[1], WHEEL[0].anim_pos[2] - WHEEL[1].anim_pos[2]];
    let (x, y) = (dot3(&d, &frame.across), dot3(&d, &frame.up));
    // one hand off the wheel in this pose (a weapon drawn): the line says nothing
    if x * x + y * y < 0.15 * 0.15 {
        WHEEL_LINE_VALID = false;
        return;
    }
    let line = y.atan2(x);
    if WHEEL_LINE_VALID {
        let mut step = line - WHEEL_LINE;
        if step > 3.141_592_65 {
            step -= 6.283_185_3;
        } else if step < -3.141_592_65 {
            step += 6.283_185_3;
        }
        if step.abs() < WHEEL_JUMP {
            WHEEL_TURN += step;
        }
    }
    WHEEL_LINE = line;
    WHEEL_LINE_VALID = true;
}

/// PublishMinimapAnchor: where the map hangs, from the hub and the camera pair whose XR pose produced this exact camera
/// rotation (not the current HMD sample; world translation cancels in the pair). HELD WHILE THE CAMERA CANNOT BE PAIRED:
/// with the head turned far the pair found its XR pose only now and then, the projection went off and on several times a
/// second (2026-11-14) -- the last one made for this vehicle, in this tracking space, stands until a new one is.
unsafe fn publish_minimap_anchor() {
    let mut value = VehicleMinimapAnchor::default();
    let vehicle = cvr_mounted_vehicle_identity();
    let bike = !MOUNTED_CAR.load(Ordering::Relaxed) && MOUNTED_BIKE.load(Ordering::Relaxed);
    let origin_now = cvr_cpp_xr_tracking_origin_serial();
    // why there is no fresh anchor (CyberpunkVR_VehicleMinimapWhy, a line each time the reason changes)
    let why = if !cvr_cpp_wheel_driving() {
        1
    } else if !map_vehicle() {
        2
    } else if vehicle == 0 {
        3
    } else if vehicle != MINIMAP_VEHICLE {
        4
    } else if !CENTER_VALID {
        5
    } else if menu_mode() {
        6
    } else {
        0
    };
    if why != 0 {
        note_minimap_why(why);
    }
    if why == 0 {
        let mut pair = VrikTransformSnapshot::default();
        let mut head = super::camera_link::HeadPose::default();
        let (mut age, mut ties) = (0u32, 0u32);
        // The script-pushed camera/entity snapshot (VRIK_ReadTransformSnapshot, as main's WheelGrab.cpp reads it): it
        // is published in a vehicle too. The native camera pair is not -- patch_camera publishes it on foot only -- so
        // read from there the map never had an anchor in a car ("the camera's XR pose not paired" every frame).
        if super::calibration::cvr_vrik_read_transform_snapshot(&mut pair)
            && pair.valid != 0
            && super::camera_link::cvr_camera_cam_write_record_find(&pair.camQuat, &mut head, &mut age, &mut ties)
            && age <= 2
            && head.valid
            && head.origin_serial == origin_now
        {
            let inverse = quat_conj(&pair.entityQuat);
            let camera = quat_rotate_vec(&inverse, &pair.cameraMinusEntity);
            let rotation = quat_mul(&inverse, &pair.camQuat);
            let base = &*cvr_xr_base_mirror();
            let b = |i: usize| f32::from_bits(base[i].load(Ordering::Relaxed));
            let world_scale = super::camera_math::cvr_camera_world_scale();
            if let Some((q, p)) = build_vehicle_minimap_pose(
                CENTER,
                camera,
                rotation,
                XQuat { x: head.ori[0], y: head.ori[1], z: head.ori[2], w: head.ori[3] },
                XVec3 { x: head.pos[0], y: head.pos[1], z: head.pos[2] },
                XQuat { x: b(0), y: b(1), z: b(2), w: b(3) },
                XVec3 { x: b(4), y: b(5), z: b(6) },
                world_scale,
                bike,
            ) {
                value = VehicleMinimapAnchor { orientation: q, position: p, stamp: tick_ms(), origin: head.origin_serial, vehicle, bike };
                note_minimap_why(0);
            } else {
                note_minimap_why(8);
            }
        } else {
            note_minimap_why(7);
        }
    }
    let mut m = MINIMAP.lock().unwrap_or_else(|e| e.into_inner());
    let held = &mut m.1;
    if value.stamp != 0 {
        *held = value;
    } else if held.stamp != 0 && held.vehicle == vehicle && held.bike == bike && held.origin == origin_now {
        value = *held;
        value.stamp = tick_ms();
    }
    m.0 = value;
}

/// Why the projected map has no fresh anchor: 0 it has one, 1 not driving, 2 not a car or a bike, 3 no mounted vehicle,
/// 4 the vehicle changed this solve, 5 no wheel centre yet (the hands' animated wheel span not seen), 6 a menu, 7 the
/// camera's XR pose not paired, 8 the pose not finite or too far.
#[no_mangle]
pub static CyberpunkVR_VehicleMinimapWhy: AtomicU32 = AtomicU32::new(0);
fn note_minimap_why(why: u32) {
    let before = CyberpunkVR_VehicleMinimapWhy.swap(why, Ordering::Relaxed);
    if before != why && why >= 2 {
        static SAID: AtomicU32 = AtomicU32::new(0);
        if SAID.fetch_or(1 << why, Ordering::Relaxed) & (1 << why) == 0 {
            const WHY: [&str; 9] = ["", "", "not a car or a bike", "no mounted vehicle", "the vehicle changed", "no wheel centre (the animated hands' wheel span not seen)", "a menu", "the camera's XR pose not paired", "the pose not finite or too far"];
            super::env::log(&format!("[wheel] the projected map has no anchor: {} ({why})\n", WHY[why as usize]));
        }
    }
}
/// WheelReadMinimapAnchor: a fresh cabin- or bars-locked anchor (a car or a bike); false for transitions, stale poses or
/// a recenter.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_read_minimap_anchor(origin: u64, out: *mut VehicleMinimapAnchor) -> bool {
    if out.is_null() || !cvr_cpp_wheel_driving() || !map_vehicle() || menu_mode() {
        return false;
    }
    let m = MINIMAP.lock().unwrap_or_else(|e| e.into_inner());
    if !fresh_vehicle_minimap(&m.0, tick_ms(), origin, cvr_mounted_vehicle_identity()) {
        return false;
    }
    *out = m.0;
    true
}

/// WheelSetCabinSlot: the car's own cabin wheel (its steering_wheel slot) in the player's model space, from script each
/// frame while a hand holds the rim. False for a non-finite pose.
pub fn set_cabin_slot(pos: V3, rot: Q) -> bool {
    if !(pos[0] + pos[1] + pos[2] + rot[0] + rot[1] + rot[2] + rot[3]).is_finite() {
        return false;
    }
    let n = (rot[0] * rot[0] + rot[1] * rot[1] + rot[2] * rot[2] + rot[3] * rot[3]).sqrt();
    if n < 0.5 {
        return false;
    }
    let mut c = CABIN.lock().unwrap_or_else(|e| e.into_inner());
    c.pos = pos;
    c.rot = [rot[0] / n, rot[1] / n, rot[2] / n, rot[3] / n];
    c.at = tick_ms();
    true
}
#[no_mangle]
pub extern "C" fn cvr_anim_wheel_set_mounted_bike(bike: bool) {
    MOUNTED_BIKE.store(bike, Ordering::Relaxed);
}
/// What the last refresh saw: the player is on a motorcycle.
#[no_mangle]
pub extern "C" fn cvr_anim_wheel_mounted_bike() -> bool {
    MOUNTED_BIKE.load(Ordering::Relaxed)
}
static WHEEL_STAMP_MS: AtomicU64 = AtomicU64::new(0);
/// std::bit_cast<uint64_t>(GamepadProfile{inner, outer}): inner the low half.
static PAD_PROFILE: AtomicU64 = AtomicU64::new(0x3F66_6666_3EB3_3333); // {0.35f, 0.9f}
static MOUNTED_CAR: AtomicBool = AtomicBool::new(false);
/// A bike: the projected map hangs over its bars, and the hands steer by pushing and pulling the bars.
static MOUNTED_BIKE: AtomicBool = AtomicBool::new(false);
fn map_vehicle() -> bool {
    MOUNTED_CAR.load(Ordering::Relaxed) || MOUNTED_BIKE.load(Ordering::Relaxed)
}
fn menu_mode() -> bool {
    // SAFETY: an export of the C++ half, read whole.
    unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) != 0 }
}
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { GetTickCount64() }
}
static mut RELAX_ROT: [[[f32; 4]; 32]; 2] = [[[0.0; 4]; 32]; 2];
static mut RELAX_HAVE: [bool; 2] = [false; 2];

fn store_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

unsafe fn controls() -> WheelControls {
    let mut c = WheelControls::default();
    super::live_reads::wheel_controls(&mut c);
    c
}

/// XrDiagNowUs (QPC microseconds), or the test clock.
unsafe fn now_us() -> u64 {
    if let Some(clock) = g_VRIKClockUs {
        return clock();
    }
    let mut f = 0i64;
    QueryPerformanceFrequency(&mut f);
    if f == 0 {
        return 0;
    }
    let mut t = 0i64;
    QueryPerformanceCounter(&mut t);
    (t.wrapping_mul(1_000_000) / f) as u64
}

unsafe fn tracking_frame(out: &mut WheelTrackingFrame) -> bool {
    match g_VRIKWheelFrame {
        Some(source) => source(out),
        None => cvr_cpp_wheel_tracking_frame(out),
    }
}

/// RAW slot read, for the two GRIPS only ([49] right, [155] left: past the hands' seqlock snapshot).
unsafe fn wheel_slot(i: i32) -> f32 {
    if !g_pSharedHands.is_null() && (0..SLOT_COUNT).contains(&i) {
        *g_pSharedHands.add(i as usize)
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn cvr_anim_wheel_set_mounted_car(car: bool) {
    MOUNTED_CAR.store(car, Ordering::Relaxed);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_capture_anim(hand: i32, handIdx: i32) {
    if !(0..=1).contains(&hand) {
        return;
    }
    let w = &mut WHEEL[hand as usize];
    w.anim_valid = false;
    if !(0..MAXB).contains(&handIdx) {
        return;
    }
    w.anim_pos = g_fkPos[handIdx as usize];
    w.anim_rot = g_fkRot[handIdx as usize];
    w.anim_valid = true;
}

/// One state update per FRESH solve: proximity, the grab, the blends, the hub and the horn.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_update(dtSec: f32) {
    WHEEL_FRAME = WHEEL_FRAME.wrapping_add(1);
    let c = controls();
    let enabled = c.grab != 0;
    let driving = cvr_cpp_wheel_driving();
    let vehicle = if driving && map_vehicle() { cvr_mounted_vehicle_identity() } else { 0 };
    if vehicle != MINIMAP_VEHICLE {
        MINIMAP_VEHICLE = vehicle;
        CENTER_VALID = false;
        for w in WHEEL.iter_mut() {
            w.ref_valid = false;
        }
    }
    let mut radius = c.radius;
    if !(radius > 0.05) || radius > 1.0 {
        radius = 0.28;
    }
    let mut dt = dtSec;
    if dt < 0.0 {
        dt = 0.0;
    }
    if dt > 0.10 {
        dt = 0.10;
    }
    // A DRAWN WEAPON OWNS THE GUN HAND.
    let weapon_out = g_hasWeaponEquipped;
    let gun_hand = if cvr_anim_weapon_in_left_hand() { 1 } else { 0 };
    track_drawn_wheel();

    let mut armed_mask = 0i32;
    let away = input_away();
    for h in 0..2usize {
        let w = &mut WHEEL[h];
        let keep = away && w.engaged; // an owning hand through the session's paused input
        let tracked = hand_tracked(w, h, keep);
        let hand_blocked = (h == gun_hand && weapon_out) || !tracked;
        if !tracked {
            w.target_valid = false;
        }
        let grip = keep || if h == 0 { wheel_slot(49) > 0.5 } else { wheel_slot(LEFT_GRIP_PRESSED) > 0.5 };
        w.at_wheel = false;
        let frame = if rim_grab_mode() && SPAN > 0.15 { wheel_frame_now() } else { None };
        let mut rim_angle = 0.0f32;
        if enabled && driving && !hand_blocked && w.anim_valid && w.target_valid {
            if let Some(f) = &frame {
                let (angle, radial, depth) = wheel_coords(f, &w.target);
                rim_angle = angle;
                w.at_wheel = (radial - SPAN * 0.5).abs() <= RIM_BAND && depth.abs() <= RIM_DEPTH;
                if grip && !w.grip_prev && !w.engaged {
                    super::env::log(&format!(
                        "[wheel] {} grip {} the rim: {:.0} deg, {:.3} m from the hub (rim {:.3}), {:+.3} m off its face\n",
                        if h == 0 { "right" } else { "left" },
                        if w.at_wheel { "on" } else { "off" },
                        angle * 57.295_779_5,
                        radial,
                        SPAN * 0.5,
                        depth
                    ));
                }
            } else {
                let dx = w.target[0] - w.anim_pos[0];
                let dy = w.target[1] - w.anim_pos[1];
                let dz = w.target[2] - w.anim_pos[2];
                w.at_wheel = (dx * dx + dy * dy + dz * dz) <= (radius * radius);
            }
        }
        if !enabled || !driving || hand_blocked {
            w.engaged = false;
        } else if w.engaged {
            w.engaged = grip; // the grip alone holds it
        } else if grip && !w.grip_prev && w.at_wheel {
            w.engaged = true; // a fresh press AT the wheel
            w.on_rim = frame.is_some();
            w.welded = false;
            if let Some(f) = &frame {
                RIM_ANGLE[h].store(rim_angle.to_bits(), Ordering::Relaxed);
                let (ref_angle, _, _) = wheel_coords(f, &w.ref_pos);
                w.rim_turn = rim_angle - ref_angle;
                w.rim_wheel = WHEEL_TURN;
                w.rim_angle0 = rim_angle;
            }
        }
        // Kept up to date even while blocked: a grip still held when the weapon is holstered is not a fresh press.
        w.grip_prev = grip;
        // A GRIP CARRIED TO THE WHEEL IS NOT THE WHEEL'S: squeezed away from it (a magazine from the belt, an item)
        // and brought across the rim, it stays what it was until it is let go.
        w.grip_foreign = grip && !w.engaged && (w.grip_foreign || !w.at_wheel);

        let step = dt / (if w.engaged { ENGAGE_SEC } else { RELEASE_SEC });
        if w.engaged {
            w.blend += step;
            if w.blend > 1.0 {
                w.blend = 1.0;
            }
        } else {
            w.blend -= step;
            if w.blend < 0.0 {
                w.blend = 0.0;
            }
        }
        // ARMED = the grip is not a gameplay button right now -- but not over a grip already held elsewhere: the
        // reload dropped its magazine at the wheel when this took it.
        if w.engaged || (w.at_wheel && !w.grip_foreign) {
            armed_mask |= if h == 0 { ARMED_RIGHT_BIT } else { ARMED_LEFT_BIT };
        }
    }

    // What the setting "full lock" is worth against the drawn wheel, for whoever is matching the two.
    if WHEEL[0].engaged || WHEEL[1].engaged {
        if !ANY_HELD {
            ANY_HELD = true;
            TURN_SEEN = 0.0;
            STEER_SEEN = 0.0;
            WHEEL_TURN = 0.0;
            WHEEL[0].rim_wheel = 0.0;
            WHEEL[1].rim_wheel = 0.0;
        }
        TURN_SEEN = TURN_SEEN.max(WHEEL_TURN.abs());
        STEER_SEEN = STEER_SEEN.max(f32::from_bits(g_wheelSteerDeg.load(Ordering::Relaxed)).abs());
    } else if ANY_HELD {
        ANY_HELD = false;
        if STEER_SEEN > 20.0 {
            super::env::log(&format!(
                "[wheel] let go: the drawn wheel turned at most {:.0} deg, the hands {:.0} deg\n",
                TURN_SEEN * 57.295_779_5,
                STEER_SEEN
            ));
        }
    }

    // WHEEL CENTRE: tracked while nothing is held, frozen for a grab.
    if !WHEEL[0].engaged && !WHEEL[1].engaged {
        if WHEEL[0].anim_valid && WHEEL[1].anim_valid {
            let (a, b) = (WHEEL[0].anim_pos, WHEEL[1].anim_pos);
            let sx = a[0] - b[0];
            let sy = a[1] - b[1];
            let sz = a[2] - b[2];
            let span = (sx * sx + sy * sy + sz * sz).sqrt();
            if span > 0.15 {
                CENTER = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
                SPAN = span;
                CENTER_VALID = true;
                for w in WHEEL.iter_mut() {
                    w.ref_pos = w.anim_pos;
                    w.ref_rot = w.anim_rot;
                    w.ref_valid = true;
                }
            }
        }
        GRAB_STATE.fetch_and(!3u64, Ordering::Release);
        store_f32(&g_wheelSteer, 0.0);
        store_f32(&g_wheelSteerDeg, 0.0);
    }

    publish_minimap_anchor();

    // HORN: a hand at the hub, not grabbing, not the gun hand.
    let mut horn_mask = 0i32;
    {
        let horn_enabled = c.horn != 0;
        let mut horn_r = c.horn_radius;
        if !(horn_r >= HORN_RADIUS_MIN) || horn_r > HORN_RADIUS_MAX {
            horn_r = HORN_RADIUS_DEFAULT;
        }
        let r_in = horn_r;
        let r_out = horn_r + HORN_HYSTERESIS;
        for h in 0..2usize {
            let w = &mut WHEEL[h];
            let mut at = false;
            let horn_blocked = h == gun_hand && weapon_out;
            if horn_enabled && driving && !horn_blocked && CENTER_VALID && w.target_valid && !w.engaged {
                let dx = w.target[0] - CENTER[0];
                let dy = w.target[1] - CENTER[1];
                let dz = w.target[2] - CENTER[2];
                let d2 = dx * dx + dy * dy + dz * dz;
                let r = if w.at_hub { r_out } else { r_in };
                at = d2 <= r * r;
            }
            w.at_hub = at;
            if at {
                horn_mask |= if h == 0 { ARMED_RIGHT_BIT } else { ARMED_LEFT_BIT };
            }
        }
    }

    store_f32(&g_wheelBlendRight, WHEEL[0].blend);
    store_f32(&g_wheelBlendLeft, WHEEL[1].blend);
    g_wheelHornMask.store(horn_mask, Ordering::Relaxed);
    // The grips holding the wheel are not gameplay buttons for the CET mods.
    if !g_pSharedHands.is_null() {
        *g_pSharedHands.add(WHEEL_ARMED_MASK) = armed_mask as f32;
    }
}

/// Ownership after both arm blocks: a NEW grab is a grip closing on the wheel with neither hand on it.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_publish_grab() {
    let mut held = 0u64;
    for h in 0..2 {
        if WHEEL[h].engaged && WHEEL[h].target_valid {
            held |= 1 << h;
        }
    }
    let previous = GRAB_STATE.load(Ordering::Relaxed);
    let epoch = (previous >> 2) + if (previous & 3) == 0 && held != 0 { 1 } else { 0 };
    if SPAN > 0.15 {
        store_f32(&PHYSICAL_RADIUS, SPAN * 0.5);
    }
    WHEEL_STAMP_MS.store(now_ms(), Ordering::Relaxed);
    GRAB_STATE.store((epoch << 2) | held, Ordering::Release);
}

/// A camera miss does not end a tracked grip: retain or release the old grabs, never start one.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_maintain_grab() {
    let allowed = controls().grab != 0 && cvr_cpp_wheel_driving();
    let gun_hand = if cvr_anim_weapon_in_left_hand() { 1 } else { 0 };
    let away = input_away();
    for h in 0..2usize {
        let w = &mut WHEEL[h];
        let keep = away && w.engaged; // see input_away
        let grip = keep || wheel_slot(if h == 0 { 49 } else { LEFT_GRIP_PRESSED }) > 0.5;
        let tracked = hand_tracked(w, h, keep);
        if !allowed || !grip || !tracked || (h == gun_hand && g_hasWeaponEquipped) {
            w.engaged = false;
        }
        w.grip_prev = grip;
        if !w.engaged {
            w.blend = 0.0;
        }
    }
    store_f32(&g_wheelBlendRight, WHEEL[0].blend);
    store_f32(&g_wheelBlendLeft, WHEEL[1].blend);
    cvr_anim_wheel_publish_grab();
}

/// Bit 2: the wheel is available in the driver's seat; bits 0/1: fresh right/left grip ownership.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_control_state() -> i32 {
    if !cvr_cpp_wheel_driving() || controls().grab == 0 || cvr_cpp_wheel_ui_blocks() {
        return 0;
    }
    let grab = GRAB_STATE.load(Ordering::Acquire);
    let stamp = WHEEL_STAMP_MS.load(Ordering::Relaxed);
    let now = now_ms();
    4 | if stamp != 0 && now >= stamp && now - stamp <= 250 {
        (grab & 3) as i32
    } else {
        0
    }
}

/// WheelHoldsCar: the player drives a car with a hand owning its wheel (a paused publication -- a hitch -- still counts).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_holds_car() -> bool {
    MOUNTED_CAR.load(Ordering::Relaxed) && (cvr_anim_wheel_control_state() & 4) != 0 && (GRAB_STATE.load(Ordering::Acquire) & 3) != 0
}

/// The latest coherent raw tracking, sampled at input-poll time.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_steer_input() -> f32 {
    let state = cvr_anim_wheel_control_state();
    let grab = GRAB_STATE.load(Ordering::Acquire);
    // Held, but the animation has not published for a while: the grab is only paused.
    if (state & 4) != 0 && (state & 3) == 0 && (grab & 3) != 0 {
        let mut s = STEER.lock().unwrap_or_else(|e| e.into_inner());
        if s.sampled_grab_epoch == grab >> 2 {
            let mut out = s.steering.suspend(now_us());
            if MOUNTED_CAR.load(Ordering::Relaxed) {
                out = car_input_for_cabin_angle(out * CAR_CABIN_WHEEL_LOCK_DEG);
            }
            store_f32(&g_wheelSteer, out);
            CyberpunkVR_WheelSteerDebug[0] = out;
            CyberpunkVR_WheelSteerDebug[3] = grab as f32;
            CyberpunkVR_WheelSteerDebug[4] = state as f32;
            CyberpunkVR_WheelSteerDebug[5] += 1.0;
            return out;
        }
    }
    let mut frame = WheelTrackingFrame::default();
    if (state & 3) != 0 {
        let mut sample = WheelTrackingFrame::default();
        if tracking_frame(&mut sample) {
            frame = sample;
        }
    }
    let c = controls();
    let mut s = STEER.lock().unwrap_or_else(|e| e.into_inner());
    if s.sampled_grab_epoch != grab >> 2 {
        s.steering.reset();
        s.sampled_grab_epoch = grab >> 2;
    }
    let car = MOUNTED_CAR.load(Ordering::Relaxed);
    let bike = MOUNTED_BIKE.load(Ordering::Relaxed);
    // where on the rim a hand holds is a car wheel's; bars have none
    let rim = |h: usize| if rim_grab_mode() && !bike { f32::from_bits(RIM_ANGLE[h].load(Ordering::Relaxed)) } else { f32::NAN };
    let mut out = s.steering.update(
        (state as u64 & grab & 3) as u8,
        &frame,
        now_us(),
        f32::from_bits(PHYSICAL_RADIUS.load(Ordering::Relaxed)),
        if bike { bike_bar_lock_degrees(cvr_settings::lc!(xrBikeBarLockDeg)) } else { wheel_full_lock_for_vehicle(c.steer_max_deg, car) },
        c.steer_dead_deg,
        if c.prediction != 0 { c.prediction_ms } else { 0.0 },
        rim(0),
        rim(1),
        bike,
        cvr_settings::lc!(xrBikeBodyLean),
    );
    if car {
        out = car_input_for_cabin_angle(out * CAR_CABIN_WHEEL_LOCK_DEG);
    }
    store_f32(&g_wheelSteer, out);
    store_f32(&g_wheelSteerDeg, s.steering.angle());
    CyberpunkVR_WheelSteerDebug[0] = out;
    CyberpunkVR_WheelSteerDebug[1] = s.steering.angle();
    CyberpunkVR_WheelSteerDebug[2] = s.steering.lean();
    CyberpunkVR_WheelSteerDebug[3] = grab as f32;
    CyberpunkVR_WheelSteerDebug[4] = state as f32;
    CyberpunkVR_WheelSteerDebug[6] = if WHEEL[0].engaged { 1.0 } else { 0.0 };
    CyberpunkVR_WheelSteerDebug[7] = if WHEEL[1].engaged { 1.0 } else { 0.0 };
    out
}

#[no_mangle]
pub extern "C" fn cvr_anim_wheel_set_gamepad_profile(inner: f32, outer: f32) -> bool {
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || inner >= 0.5 || outer > 1.0 || outer <= inner + 0.01 {
        return false;
    }
    PAD_PROFILE.store(inner.to_bits() as u64 | ((outer.to_bits() as u64) << 32), Ordering::Relaxed);
    true
}

#[no_mangle]
pub extern "C" fn cvr_anim_wheel_gamepad_steer(normalized: f32) -> f32 {
    let p = PAD_PROFILE.load(Ordering::Relaxed);
    wheel_gamepad_axis(normalized, f32::from_bits(p as u32), f32::from_bits((p >> 32) as u32))
}

/// The IK target blended toward the animated hand; a no-op at blend 0.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_blend_target(hand: i32, target: *mut f32, handRot: *mut f32) {
    if !(0..=1).contains(&hand) || target.is_null() || handRot.is_null() {
        return;
    }
    let w = &mut WHEEL[hand as usize];
    if w.blend <= 0.0 || !w.anim_valid {
        return;
    }
    let b = w.blend;
    let mut goal_pos = w.anim_pos;
    let mut goal_rot = w.anim_rot;
    let frame = if w.on_rim && w.ref_valid { wheel_frame_now() } else { None };
    if let Some(f) = frame {
        // the hand, round the hub from where the animation has it to where it took hold, and on with the wheel since
        let (angle, radial, _) = wheel_coords(&f, &w.target);
        // where the controller really is, for the steering when the other hand lets go
        if radial > RIM_CENTRE {
            RIM_ANGLE[hand as usize].store(angle.to_bits(), Ordering::Relaxed);
        }
        // FROM THE ANIMATED HAND AS IT IS NOW, turned only by how far round the rim from it the hand took hold (the
        // whole wheel turn about our guess at its axis slid the hand 1 cm every 30 degrees, 2026-11-14). The old way
        // while the animated hand is not on the rim (a one-handed pose with a weapon out).
        let (anim_angle, anim_radial, _) = wheel_coords(&f, &w.anim_pos);
        let from_anim = anim_radial > SPAN * 0.25;
        let mut turn = if from_anim { w.rim_angle0 + (WHEEL_TURN - w.rim_wheel) - anim_angle } else { w.rim_turn + WHEEL_TURN - w.rim_wheel };
        if from_anim {
            turn = cvr_body::cmath::remainder(turn, 6.283_185_3);
        }
        let base = if from_anim { w.anim_pos } else { w.ref_pos };
        let (half_sin, half_cos) = ((turn * 0.5).sin(), (turn * 0.5).cos());
        let q = [f.normal[0] * half_sin, f.normal[1] * half_sin, f.normal[2] * half_sin, half_cos];
        let d = [base[0] - CENTER[0], base[1] - CENTER[1], base[2] - CENTER[2]];
        // v + 2w(q x v) + 2 q x (q x v)
        let t = [2.0 * (q[1] * d[2] - q[2] * d[1]), 2.0 * (q[2] * d[0] - q[0] * d[2]), 2.0 * (q[0] * d[1] - q[1] * d[0])];
        goal_pos = [
            CENTER[0] + d[0] + half_cos * t[0] + q[1] * t[2] - q[2] * t[1],
            CENTER[1] + d[1] + half_cos * t[1] + q[2] * t[0] - q[0] * t[2],
            CENTER[2] + d[2] + half_cos * t[2] + q[0] * t[1] - q[1] * t[0],
        ];
        let r = if from_anim { w.anim_rot } else { w.ref_rot };
        goal_rot = [
            q[3] * r[0] + q[0] * r[3] + q[1] * r[2] - q[2] * r[1],
            q[3] * r[1] - q[0] * r[2] + q[1] * r[3] + q[2] * r[0],
            q[3] * r[2] + q[0] * r[1] - q[1] * r[0] + q[2] * r[3],
            q[3] * r[3] - q[0] * r[0] - q[1] * r[1] - q[2] * r[2],
        ];
        // ON THE CABIN WHEEL ITSELF, when script gives its slot: where the hand took hold is kept in the slot's frame
        // and rides it (the turn about the guessed axis left it 14-30 mm off the drawn wheel at 90 degrees, 2026-11-14).
        let slot = {
            let c = CABIN.lock().unwrap_or_else(|e| e.into_inner());
            (c.at != 0 && tick_ms() - c.at < CABIN_FRESH_MS).then_some((c.pos, c.rot))
        };
        if let Some((sp, sq)) = slot {
            let iq = [-sq[0], -sq[1], -sq[2], sq[3]];
            if !w.welded {
                let d2 = [goal_pos[0] - sp[0], goal_pos[1] - sp[1], goal_pos[2] - sp[2]];
                w.weld_pos = quat_rotate_vec(&iq, &d2);
                w.weld_rot = quat_mul(&iq, &goal_rot);
                w.welded = true;
            }
            let o = quat_rotate_vec(&sq, &w.weld_pos);
            goal_pos = [sp[0] + o[0], sp[1] + o[1], sp[2] + o[2]];
            goal_rot = quat_mul(&sq, &w.weld_rot);
        }
    }
    for k in 0..3 {
        *target.add(k) += (goal_pos[k] - *target.add(k)) * b;
    }
    let from = a4(handRot);
    let mut to = goal_rot;
    if from[0] * to[0] + from[1] * to[1] + from[2] * to[2] + from[3] * to[3] < 0.0 {
        for c in to.iter_mut() {
            *c = -*c;
        }
    }
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        out[k] = from[k] + (to[k] - from[k]) * b;
    }
    quat_norm(&mut out);
    put4(handRot, &out);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_store_target(hand: i32, target: *const f32) {
    if !(0..=1).contains(&hand) || target.is_null() {
        return;
    }
    let w = &mut WHEEL[hand as usize];
    w.target = a3(target);
    w.target_valid = true;
    w.target_frame = WHEEL_FRAME;
}

/// True once the arm is entirely the animation's.
/// g_wheelBlendLeft / g_wheelBlendRight: 0 the arm IK drives the hand, 1 the wheel's animation does; `hand` 0 left,
/// 1 right (the haptics' order).
pub fn cvr_anim_wheel_blend(hand: i32) -> f32 {
    let index = match hand {
        0 => 1,
        1 => 0,
        _ => return 0.0,
    };
    // SAFETY: one float of the wheel's state, read whole (a frame late at worst, as the C++ atomic was).
    unsafe { core::ptr::read_volatile(&raw const WHEEL[index].blend) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_hands_off(hand: i32) -> bool {
    // A hand on the rim is solved to its place there, so the arm stays ours.
    (0..=1).contains(&hand) && WHEEL[hand as usize].blend >= FULL_BLEND && !WHEEL[hand as usize].on_rim
}

/// OPEN HANDS IN A CAR: the finger locals latched on foot with empty hands, replayed on the hand not on the wheel.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_fingers(boneBuf: *mut u8) {
    if boneBuf.is_null() {
        return;
    }
    let in_vehicle = g_isInVehicle;
    let weapon = g_hasWeaponEquipped;
    for h in 0..2usize {
        let count = if h == 0 { g_VRSmokeFingerCount } else { g_VRSmokeFingerCountL };
        let idx = if h == 0 { &g_VRSmokeFingerIdx } else { &g_VRSmokeFingerIdxL };
        if count <= 0 {
            continue;
        }
        let smoking = if h == 0 { g_VRSmokeFingerActive != 0 } else { g_VRSmokeFingerActiveL != 0 };
        let grip = if h == 0 { wheel_slot(49) > 0.5 } else { wheel_slot(LEFT_GRIP_PRESSED) > 0.5 };
        let n = (count as usize).min(32);
        if !in_vehicle {
            // CAPTURE: empty-handed and not squeezing.
            if !weapon && !smoking && !grip {
                for k in 0..n {
                    let bi = idx[k];
                    if !(0..MAXB).contains(&bi) {
                        continue;
                    }
                    RELAX_ROT[h][k] = a4(br(boneBuf, bi));
                }
                RELAX_HAVE[h] = true;
            }
            continue;
        }
        // APPLY: seated, this hand not (mostly) on the wheel, nothing else claiming the fingers.
        // Not a hand whose fingers are measured: those are already on it.
        if !weapon && !smoking && RELAX_HAVE[h] && WHEEL[h].blend < 0.5 && !super::gestures::owns_hand(1 - h as i32) {
            for k in 0..n {
                let bi = idx[k];
                if !(0..MAXB).contains(&bi) {
                    continue;
                }
                put4(br(boneBuf, bi), &RELAX_ROT[h][k]);
            }
        }
    }
}

/// Drops the whole thing, published state included.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_wheel_reset() {
    let mut s = STEER.lock().unwrap_or_else(|e| e.into_inner());
    s.steering.reset();
    WHEEL_FRAME = 0;
    let epoch = (GRAB_STATE.load(Ordering::Relaxed) >> 2) + 1;
    GRAB_STATE.store(epoch << 2, Ordering::Release);
    WHEEL_STAMP_MS.store(0, Ordering::Relaxed);
    for w in WHEEL.iter_mut() {
        w.blend = 0.0;
        w.engaged = false;
        w.at_wheel = false;
        w.at_hub = false;
        w.grip_prev = false;
        w.target_valid = false;
        w.anim_valid = false;
        w.target_frame = 0;
        w.ref_valid = false;
        w.on_rim = false;
    }
    CENTER_VALID = false;
    MINIMAP_VEHICLE = 0;
    MINIMAP.lock().unwrap_or_else(|e| e.into_inner()).0 = VehicleMinimapAnchor::default();
    SPAN = 0.0;
    WHEEL_LINE_VALID = false;
    WHEEL_TURN = 0.0;
    ANY_HELD = false;
    store_f32(&g_wheelBlendRight, 0.0);
    store_f32(&g_wheelBlendLeft, 0.0);
    store_f32(&g_wheelSteer, 0.0);
    store_f32(&g_wheelSteerDeg, 0.0);
    g_wheelHornMask.store(0, Ordering::Relaxed);
    if !g_pSharedHands.is_null() {
        *g_pSharedHands.add(WHEEL_ARMED_MASK) = 0.0;
    }
    drop(s);
}

// The script bridge's natives (formerly src/Natives/Driving.cpp), registered by src/Natives/Registration.cpp.

/// GetVRWheelControlState() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn GetVRWheelControlState(_this: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut i32, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    if !out.is_null() {
        *out = cvr_anim_wheel_control_state();
    }
}
/// SetVRWheelCabin(pos: Vector4, rot: Quaternion) -> Bool: the car's steering_wheel slot in the player's model space
/// (the Driving mechanic, each frame while a hand holds the rim).
#[no_mangle]
pub unsafe extern "C" fn SetVRWheelCabin(_this: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut bool, _unused: i64) {
    let (mut pos, mut rot) = ([0.0f32; 4], [0.0f32; 4]);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut pos);
        frame.get_parameter(&mut rot);
        frame.finish_params();
    }
    let accepted = set_cabin_slot([pos[0], pos[1], pos[2]], rot);
    if !out.is_null() {
        *out = accepted;
    }
}
/// SetVRWheelGamepadProfile(inner: Float, outer: Float) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn SetVRWheelGamepadProfile(_this: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut bool, _unused: i64) {
    let (mut inner, mut outer) = (0.0f32, 0.0f32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut inner);
        frame.get_parameter(&mut outer);
        frame.finish_params();
    }
    let accepted = cvr_anim_wheel_set_gamepad_profile(inner, outer);
    if !out.is_null() {
        *out = accepted;
    }
}
