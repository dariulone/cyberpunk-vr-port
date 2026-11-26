//! The controllers read for the game (formerly the gameplay half of the [HANDS] block in
//! src/Runtimes/OpenXRFrameLoop.cpp): every hand's trigger, grip, stick and buttons read from its actions, mapped onto
//! XInput's button bits, the D-pad chord (the left stick held, the right stick picks the direction; on PSVR2 the
//! Triangle touch), the deferred L3 (sprint) emitted on a release that picked no direction, the Steam Frame's own
//! buttons, the menu button (PSVR2: the system button's tap and hold), and the snapshots published for the XInput hook.
//!
//! The poses stay in C++ for now (the same block, before this); so do the full-body calibration's trigger note, the
//! finger tracking's grip, and the overlay that owns the tracking these readings are written into.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::c_void;
use cvr_body::vr_overlay::{cvr_vrui_captures_input, cvr_vrui_update_tracking, cvr_vrui_visible};
use cvr_host::log;
use openxr_sys as xr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;
use xr::Handle as _;

/// VRControllerState (include/Runtimes/OpenXRManager.hpp), the snapshot the XInput hook merges.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControllerState {
    pub buttons: u16,
    pub left_trigger: f32,
    pub right_trigger: f32,
    /// The current physical pull per hand (the trigger's hysteresis sampled at the XR sync), 0 released.
    pub finger_pull: [u64; 2],
    pub left_thumb_x: f32,
    pub left_thumb_y: f32,
    pub right_thumb_x: f32,
    pub right_thumb_y: f32,
    pub left_grip: f32,
    pub right_grip: f32,
    pub left_hand_valid: bool,
    pub right_hand_valid: bool,
    pub psvr2_system_pressed: bool,
    pub psvr2_system_available: bool,
    pub psvr2_dpad_shift: bool,
    /// The physical left-ear L3 gesture, before handedness and the deferred L3's routing: the scanner's fresh presses
    /// (a monotonic count, so a short press between two XInput polls is not lost) and its click held.
    pub scanner_presses: u64,
    pub scanner_held: bool,
}
const _: () = assert!(core::mem::size_of::<ControllerState>() == 80);

/// cvr::vrui::Hand (include/Overlay/VrInteraction.hpp): what the overlay is told about a hand.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UiHand {
    pub aim: xr::Posef,
    pub valid: bool,
    pub stick_click: bool,
    pub trigger: f32,
    pub grip: f32,
    pub stick_y: f32,
}
const _: () = assert!(core::mem::size_of::<UiHand>() == 44);

/// The actions and spaces read (each 0 when not created).
/// How hard each grip is squeezed past closed, 0..1, side 0 left (main's g_xrGripForce): only Index controllers report it.
pub static GRIP_FORCE: [core::sync::atomic::AtomicU32; 2] = [core::sync::atomic::AtomicU32::new(0), core::sync::atomic::AtomicU32::new(0)];
/// cvr_xr_grip_force: VRGripForce's read.
#[no_mangle]
pub extern "C" fn cvr_xr_grip_force(side: i32) -> f32 {
    if (0..=1).contains(&side) { f32::from_bits(GRIP_FORCE[side as usize].load(core::sync::atomic::Ordering::Relaxed)) } else { 0.0 }
}
#[repr(C)]
pub struct InputActions {
    pub session: u64,
    pub local_space: u64,
    pub hand_paths: [u64; 2],
    pub hand_aim_spaces: [u64; 2],
    pub thumbstick: u64,
    pub trigger: u64,
    pub grip: u64,
    /// The squeeze's force past closed (Valve Index only; 0 when there is none).
    pub grip_force: u64,
    pub thumbstick_click: u64,
    pub primary: u64,
    pub secondary: u64,
    pub menu: u64,
    pub psvr2_triangle_touch: u64,
    /// include/Runtimes/SteamFrameInput.hpp SteamFrameActions: x, y, up, down, left, right, view, bumper.
    pub steam_frame: [u64; 8],
    /// The Touch controllers' thumbrest touch.
    pub thumbrest_touch: u64,
}

/// This frame's facts.
#[repr(C)]
pub struct InputFrame {
    /// The frame's predicted display time: the finger joints and the overlay's aims are located at it.
    pub time: i64,
    /// The hands' grip poses were located this frame.
    pub pose_valid: [bool; 2],
    /// xr_input_actions is on and the gameplay actions exist.
    pub gameplay_active: bool,
    pub psvr2: bool,
    /// xrSyncActions returned XR_SUCCESS.
    pub sync_ok: bool,
    /// The physical left grip relative to the raw head at the same instant (the scanner's ear zone, head axes).
    pub scanner_hand: [f32; 3],
}

/// The snapshots (OpenXRManager's m_inputMutex, m_controllerState and m_rawControllerState, owned here): the one the
/// XInput hook reads, and the raw one from before the overlay's capture.
#[repr(C)]
pub struct InputShared {
    pub lock: crate::srw::SrwLock,
    pub controller: crate::srw::Guarded<ControllerState>,
    pub raw: crate::srw::Guarded<ControllerState>,
}
const NO_CONTROLLER: ControllerState = ControllerState {
    buttons: 0,
    left_trigger: 0.0,
    right_trigger: 0.0,
    finger_pull: [0; 2],
    left_thumb_x: 0.0,
    left_thumb_y: 0.0,
    right_thumb_x: 0.0,
    right_thumb_y: 0.0,
    left_grip: 0.0,
    right_grip: 0.0,
    left_hand_valid: false,
    right_hand_valid: false,
    psvr2_system_pressed: false,
    psvr2_system_available: false,
    psvr2_dpad_shift: false,
    scanner_presses: 0,
    scanner_held: false,
};
pub static INPUT: InputShared = InputShared {
    lock: crate::srw::SrwLock::new(),
    controller: crate::srw::Guarded::new(NO_CONTROLLER),
    raw: crate::srw::Guarded::new(NO_CONTROLLER),
};
const _: () = assert!(core::mem::size_of::<InputShared>() == 168);
#[no_mangle]
pub extern "C" fn cvr_xr_input_shared() -> *const InputShared {
    &INPUT
}
/// OpenXRManager::GetControllerState (`raw` false) and GetRawControllerState (true).
///
/// # Safety
/// `out` is a VRControllerState.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_controller_state(out: *mut ControllerState, raw: bool) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(out) = (unsafe { out.as_mut() }) else { return false };
    let g = INPUT.lock.lock();
    *out = if raw { *INPUT.raw.get(&g) } else { *INPUT.controller.get(&g) };
    true
}
/// OpenXRManager's m_fingers: each hand's pull, sampled under the input lock.
static FINGERS: Mutex<[cvr_input::trigger::FingerPull; 2]> = Mutex::new([cvr_input::trigger::FingerPull {
    trigger: cvr_input::trigger::WeaponTrigger { active: false, down: false, ready: false },
    serial: 0,
}; 2]);
/// The session is not focused any more: the PSVR2 system button and the D-pad shift are released; a stopped or
/// unfocused session may not publish another frame, so a held pull is released too.
pub fn focus_lost() {
    let g = INPUT.lock.lock();
    let c = INPUT.controller.get(&g);
    c.psvr2_system_available = false;
    c.psvr2_system_pressed = false;
    c.psvr2_dpad_shift = false;
    let mut fingers = FINGERS.lock().unwrap_or_else(|p| p.into_inner());
    for hand in 0..2 {
        c.finger_pull[hand] = fingers[hand].sample(false, 0.0);
    }
}

// XInput's XINPUT_GAMEPAD_* bits.
const XB_DPAD_UP: u16 = 0x0001;
const XB_DPAD_DOWN: u16 = 0x0002;
const XB_DPAD_LEFT: u16 = 0x0004;
const XB_DPAD_RIGHT: u16 = 0x0008;
const XB_START: u16 = 0x0010;
const XB_LEFT_THUMB: u16 = 0x0040;
const XB_RIGHT_THUMB: u16 = 0x0080;
const XB_LEFT_SHOULDER: u16 = 0x0100;
const XB_RIGHT_SHOULDER: u16 = 0x0200;
const XB_A: u16 = 0x1000;
const XB_B: u16 = 0x2000;
const XB_X: u16 = 0x4000;
const XB_Y: u16 = 0x8000;

/// One hand's gameplay inputs, as read from its actions (the grip after the finger tracking's say).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandInput {
    pub trigger: f32,
    pub grip: f32,
    pub stick_x: f32,
    pub stick_y: f32,
    pub click: bool,
    pub primary: bool,
    pub secondary: bool,
    /// The Steam Frame's separate bumper.
    pub bumper: bool,
    /// The left hand's PSVR2 Triangle touch (the D-pad shift).
    pub triangle: bool,
}
/// The thumbrests' touches this frame (left, right).
static THUMBRESTS: [core::sync::atomic::AtomicBool; 2] = [core::sync::atomic::AtomicBool::new(false), core::sync::atomic::AtomicBool::new(false)];

/// The chord's facts across the two hands of one frame (the left hand is mapped first).
#[derive(Clone, Copy, Debug, Default)]
pub struct Chord {
    pub left_stick_clicked: bool,
    pub triangle_shift: bool,
    pub dpad_used: bool,
    /// A Touch controller's thumbrest held past 0.2 s (Quest 3's D-pad shift).
    pub thumbrest_shift: bool,
    /// The thumbrest chord mirrored (a left-hander's traded sticks): the right thumbrest, the left stick's direction.
    pub mirrored: bool,
}

/// include/Hooks/Psvr2Input.hpp TriangleDpad: the stick's direction past half as the D-pad, the look zeroed for the
/// whole shift.
fn triangle_dpad(x: &mut f32, y: &mut f32, buttons: &mut u16) -> bool {
    let mut dpad = 0u16;
    if *y > 0.5 {
        dpad |= XB_DPAD_UP;
    }
    if *y < -0.5 {
        dpad |= XB_DPAD_DOWN;
    }
    if *x < -0.5 {
        dpad |= XB_DPAD_LEFT;
    }
    if *x > 0.5 {
        dpad |= XB_DPAD_RIGHT;
    }
    *buttons |= dpad;
    *x = 0.0;
    *y = 0.0;
    dpad != 0
}

/// One hand onto the snapshot (the D-pad chord comes after, `dpad_chord`).
pub fn map_hand(ctrl: &mut ControllerState, hand: usize, input: &HandInput, chord: &mut Chord) {
    if hand == 0 {
        ctrl.left_trigger = input.trigger;
        ctrl.left_grip = input.grip;
        ctrl.left_thumb_x = input.stick_x;
        ctrl.left_thumb_y = input.stick_y;
        if input.primary {
            ctrl.buttons |= XB_X;
        }
        if input.secondary {
            ctrl.buttons |= XB_Y;
        }
        // Only the Frame's separate bumper is LB: the grip stays for magazines and gestures.
        if input.bumper {
            ctrl.buttons |= XB_LEFT_SHOULDER;
        }
        // The left stick's click is the D-pad's modifier; its own L3 is deferred (`DeferredL3`).
        chord.left_stick_clicked = input.click;
        chord.triangle_shift = input.triangle;
    } else {
        ctrl.right_trigger = input.trigger;
        ctrl.right_grip = input.grip;
        ctrl.right_thumb_x = input.stick_x;
        ctrl.right_thumb_y = input.stick_y;
        if input.click {
            ctrl.buttons |= XB_RIGHT_THUMB;
        }
        if input.primary {
            ctrl.buttons |= XB_A;
        }
        if input.secondary {
            ctrl.buttons |= XB_B;
        }
        if input.bumper {
            ctrl.buttons |= XB_RIGHT_SHOULDER;
        }
    }
}

/// The D-pad chord, once the left click's owner is known (main resolves it first: a scanner click must not also send
/// a D-pad direction): the right stick picks the direction; its axes are zeroed for the whole hold so the snap turn and
/// the camera cannot fire during the selection. The right grip stays the holsters'. `left_free` is the left click not
/// the scanner's nor a grenade's.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // the NaN falls to the default, as in C++
pub fn dpad_chord(ctrl: &mut ControllerState, chord: &mut Chord, chord_stick: f32, scanner_claim: bool, left_free: bool) {
    if !scanner_claim && chord.thumbrest_shift && chord.mirrored {
        // a left-hander's traded sticks: the right thumbrest held, the left stick picks the direction
        chord.dpad_used = triangle_dpad(&mut ctrl.left_thumb_x, &mut ctrl.left_thumb_y, &mut ctrl.buttons);
    } else if !scanner_claim && (chord.triangle_shift || chord.thumbrest_shift) {
        chord.dpad_used = triangle_dpad(&mut ctrl.right_thumb_x, &mut ctrl.right_thumb_y, &mut ctrl.buttons);
    } else if chord.left_stick_clicked && !scanner_claim && left_free {
        let threshold = if !(chord_stick > 0.05) || chord_stick > 1.0 { 0.90 } else { chord_stick };
        let (sx, sy) = (ctrl.right_thumb_x, ctrl.right_thumb_y);
        if sy > threshold {
            ctrl.buttons |= XB_DPAD_UP;
            chord.dpad_used = true;
        }
        if sy < -threshold {
            ctrl.buttons |= XB_DPAD_DOWN;
            chord.dpad_used = true;
        }
        if sx < -threshold {
            ctrl.buttons |= XB_DPAD_LEFT;
            chord.dpad_used = true;
        }
        if sx > threshold {
            ctrl.buttons |= XB_DPAD_RIGHT;
            chord.dpad_used = true;
        }
        ctrl.right_thumb_x = 0.0;
        ctrl.right_thumb_y = 0.0;
    }
}

/// The deferred L3 (sprint) and the scanner's click: the raw left click's owners (cvr_input::scanner).
struct ClickOwners {
    deferred: cvr_input::scanner::DeferredLeftStickClick,
    scanner: cvr_input::scanner::ScannerClick,
    scanner_tick: u64,
    grenade: cvr_input::grenade_stick::GrenadeStick,
}
static CLICKS: Mutex<Option<ClickOwners>> = Mutex::new(None);
/// The combat gadget's chord (cvr_input::gadget_hold) and whether it holds RB now.
static GADGET: Mutex<cvr_input::gadget_hold::GadgetHold> = Mutex::new(cvr_input::gadget_hold::GadgetHold::new());
static GADGET_RB: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
extern "system" {
    fn xrGetActionStateFloat(session: xr::Session, info: *const xr::ActionStateGetInfo, state: *mut xr::ActionStateFloat) -> xr::Result;
    fn xrGetActionStateBoolean(session: xr::Session, info: *const xr::ActionStateGetInfo, state: *mut xr::ActionStateBoolean) -> xr::Result;
    fn xrGetActionStateVector2f(session: xr::Session, info: *const xr::ActionStateGetInfo, state: *mut xr::ActionStateVector2f) -> xr::Result;
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}
#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    /// The D-pad chord's stick threshold (xr_dpad_chord_stick).
    static CyberpunkVR_DpadChordStick: f32;
    /// Counted for the XInput hook, which holds a one-frame L3 until its next poll (src/Hooks/XInput.cpp).
    static CyberpunkVR_LeftStickClicks: AtomicI32;
    /// The full-body calibration's steps are a pull of both triggers.
    fn cvr_cpp_fbt_note_trigger(hand: i32, value: f32);
    /// rust/crates/cvr-rig/src/vrik/fingergrip.rs: the fingers' grip (-1 untracked) and whether xr_grip_from_fingers
    /// takes the grip from them.
    fn cvr_fingergrip_evaluate(hand: i32, base: u64, time: i64) -> i32;
    fn cvr_fingergrip_grip_from_fingers() -> bool;
    /// rust/crates/cvr-rig/src/vrik/cyberware_chord.rs.
    fn cvr_cyberware_chord_update(l3: bool, grip: f32, r3: bool, input_active: bool) -> bool;
    /// The game's menu mode (cvr-render cpp_services.rs), the lobby's world shown (menu_lobby.rs), the VR menu holding
    /// the buttons (cvr-rig vr_menu).
    fn GetMenuMode() -> i32;
    fn cvr_menu_lobby_world_shown() -> bool;
    fn cvr_vrmenu_captures_buttons() -> bool;
    /// Mounted in a vehicle (cvr-render vrcore_exports): a hand on the wheel, RB the radio's.
    static g_isInVehicle: bool;
    /// The scanner gesture's switch and its ear zone (cvr-render input_exports.rs).
    static CyberpunkVR_ScannerGesture: i32;
    static CyberpunkVR_ScannerEarX: f32;
    static CyberpunkVR_ScannerEarY: f32;
    static CyberpunkVR_ScannerEarZ: f32;
    static CyberpunkVR_ScannerEarRadius: f32;
    static CyberpunkVR_ScannerEarMargin: f32;
    static CyberpunkVR_ScannerEnterMs: i32;
    static CyberpunkVR_ScannerExitMs: i32;
}

#[cfg(not(test))]
fn get_info(action: u64, path: u64) -> xr::ActionStateGetInfo {
    xr::ActionStateGetInfo {
        ty: xr::StructureType::ACTION_STATE_GET_INFO,
        next: core::ptr::null(),
        action: xr::Action::from_raw(action),
        subaction_path: xr::Path::from_raw(path),
    }
}
/// The action's value when it is active, else 0; and whether it was active.
#[cfg(not(test))]
unsafe fn read_float_active(session: xr::Session, action: u64, path: u64) -> (f32, bool) {
    let mut st = xr::ActionStateFloat {
        ty: xr::StructureType::ACTION_STATE_FLOAT,
        next: core::ptr::null_mut(),
        current_state: 0.0,
        changed_since_last_sync: xr::FALSE,
        last_change_time: xr::Time::from_nanos(0),
        is_active: xr::FALSE,
    };
    // SAFETY: the caller's live session.
    let ok = unsafe { xrGetActionStateFloat(session, &get_info(action, path), &mut st) }.into_raw() >= 0;
    if ok && st.is_active.into_raw() != 0 {
        (st.current_state, true)
    } else {
        (0.0, false)
    }
}
/// The boolean action's state and whether it was active.
#[cfg(not(test))]
unsafe fn read_bool_active(session: xr::Session, action: u64, path: u64) -> (bool, bool) {
    let mut st = xr::ActionStateBoolean {
        ty: xr::StructureType::ACTION_STATE_BOOLEAN,
        next: core::ptr::null_mut(),
        current_state: xr::FALSE,
        changed_since_last_sync: xr::FALSE,
        last_change_time: xr::Time::from_nanos(0),
        is_active: xr::FALSE,
    };
    // SAFETY: the caller's live session.
    let ok = unsafe { xrGetActionStateBoolean(session, &get_info(action, path), &mut st) }.into_raw() >= 0;
    let active = ok && st.is_active.into_raw() != 0;
    (active && st.current_state.into_raw() != 0, active)
}
/// The action's value when it is active, else 0.
#[cfg(not(test))]
unsafe fn read_float(session: xr::Session, action: u64, path: u64) -> f32 {
    let mut st = xr::ActionStateFloat {
        ty: xr::StructureType::ACTION_STATE_FLOAT,
        next: core::ptr::null_mut(),
        current_state: 0.0,
        changed_since_last_sync: xr::FALSE,
        last_change_time: xr::Time::from_nanos(0),
        is_active: xr::FALSE,
    };
    // SAFETY: the caller's live session.
    let ok = unsafe { xrGetActionStateFloat(session, &get_info(action, path), &mut st) }.into_raw() >= 0;
    if ok && st.is_active.into_raw() != 0 {
        st.current_state
    } else {
        0.0
    }
}
#[cfg(not(test))]
unsafe fn read_bool(session: xr::Session, action: u64, path: u64) -> bool {
    let mut st = xr::ActionStateBoolean {
        ty: xr::StructureType::ACTION_STATE_BOOLEAN,
        next: core::ptr::null_mut(),
        current_state: xr::FALSE,
        changed_since_last_sync: xr::FALSE,
        last_change_time: xr::Time::from_nanos(0),
        is_active: xr::FALSE,
    };
    // SAFETY: the caller's live session.
    let ok = unsafe { xrGetActionStateBoolean(session, &get_info(action, path), &mut st) }.into_raw() >= 0;
    ok && st.is_active.into_raw() != 0 && st.current_state.into_raw() != 0
}
#[cfg(not(test))]
unsafe fn read_vec2(session: xr::Session, action: u64, path: u64) -> (f32, f32) {
    let mut st = xr::ActionStateVector2f {
        ty: xr::StructureType::ACTION_STATE_VECTOR2F,
        next: core::ptr::null_mut(),
        current_state: xr::Vector2f { x: 0.0, y: 0.0 },
        changed_since_last_sync: xr::FALSE,
        last_change_time: xr::Time::from_nanos(0),
        is_active: xr::FALSE,
    };
    // SAFETY: the caller's live session.
    let ok = unsafe { xrGetActionStateVector2f(session, &get_info(action, path), &mut st) }.into_raw() >= 0;
    if ok && st.is_active.into_raw() != 0 {
        (st.current_state.x, st.current_state.y)
    } else {
        (0.0, 0.0)
    }
}

static TRIANGLE_LOGGED: AtomicBool = AtomicBool::new(false);
static MENU_LOGGED: AtomicBool = AtomicBool::new(false);

/// The controllers for this frame: read, mapped, chorded, published. `ctrl` comes with the hands' validity set;
/// `tracking` is the overlay's cvr::vrui::Tracking, whose `hands` are `ui_hands`.
///
/// # Safety
/// The handles are live or 0; the pointers are OpenXRManager's for this frame, called under its hand lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_sample_controllers(
    a: *const InputActions,
    frame: *const InputFrame,
    ctrl: *mut ControllerState,
    tracking: *mut c_void,
    ui_hands: *mut UiHand,
) {
    // SAFETY (whole body): the caller's structures for this frame; the live session's actions and spaces.
    unsafe {
        let (Some(a), Some(f), Some(ctrl)) = (a.as_ref(), frame.as_ref(), ctrl.as_mut()) else {
            return;
        };
        let ui = core::slice::from_raw_parts_mut(ui_hands, 2);
        let session = xr::Session::from_raw(a.session);
        let mut chord = Chord::default();
        let mut trigger_active = [false; 2];
        let mut left_click_valid = false;
        if f.gameplay_active {
            for (i, u) in ui.iter_mut().enumerate() {
                let path = a.hand_paths[i];
                let (trigger, active) = read_float_active(session, a.trigger, path);
                trigger_active[i] = active;
                cvr_cpp_fbt_note_trigger(i as i32, trigger);
                let mut grip = read_float(session, a.grip, path);
                GRIP_FORCE[i].store(if a.grip_force != 0 { read_float(session, a.grip_force, path) } else { 0.0 }.to_bits(), core::sync::atomic::Ordering::Relaxed);
                let fingers = finger_grip(i as i32, a.local_space, f.time);
                if fingers >= 0 {
                    grip = if fingers != 0 { 1.0 } else { 0.0 };
                }
                let (stick_x, stick_y) = read_vec2(session, a.thumbstick, path);
                let (click, click_active) = read_bool_active(session, a.thumbstick_click, path);
                if i == 0 {
                    left_click_valid = click_active;
                }
                u.stick_click = click;
                u.trigger = trigger;
                u.grip = grip;
                u.stick_y = stick_y;
                u.valid = f.pose_valid[i];
                if (vrui_pointing() || click) && a.hand_aim_spaces[i] != 0 {
                    u.valid = false;
                    let mut aim: xr::SpaceLocation = core::mem::zeroed();
                    aim.ty = xr::StructureType::SPACE_LOCATION;
                    let need = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
                    let res = xrLocateSpace(
                        xr::Space::from_raw(a.hand_aim_spaces[i]),
                        xr::Space::from_raw(a.local_space),
                        xr::Time::from_nanos(f.time),
                        &mut aim,
                    );
                    if res.into_raw() >= 0 && aim.location_flags.contains(need) {
                        u.aim = aim.pose;
                        u.valid = true;
                    }
                }
                let primary = read_bool(session, a.primary, path);
                let secondary = read_bool(session, a.secondary, path);
                // bound only on the Steam Frame controllers' own profile
                let (bumper, native) = if a.steam_frame[7] != 0 { read_bool_active(session, a.steam_frame[7], path) } else { (false, false) };
                if i == 1 {
                    cvr_input::modifier::NATIVE_PAD.store(native, Ordering::Relaxed);
                }
                let triangle = i == 0 && f.psvr2 && a.psvr2_triangle_touch != 0 && read_bool(session, a.psvr2_triangle_touch, path);
                if triangle && !TRIANGLE_LOGGED.swap(true, Ordering::Relaxed) {
                    log!("OpenXRManager[PSVR2]: Triangle-touch D-pad modifier active.\n");
                }
                let thumbrest = a.thumbrest_touch != 0 && read_bool(session, a.thumbrest_touch, path);
                THUMBRESTS[i].store(thumbrest, Ordering::Relaxed);
                let input = HandInput { trigger, grip, stick_x, stick_y, click, primary, secondary, bumper, triangle };
                map_hand(ctrl, i, &input, &mut chord);
            }
        }

        // The overlay reads the RAW clicks first: L3+R3 takes priority over the cyberware even with the left grip held.
        cvr_vrui_update_tracking(tracking.cast(), cvr_fingergrip_grip_from_fingers());
        let right_clicked = ui[1].stick_click;
        let focused = crate::events::session_state() == xr::SessionState::FOCUSED;
        let menu = GetMenuMode() != 0;
        let captured = cvr_vrui_captures_input() || cvr_vrmenu_captures_buttons();
        let mut owners = CLICKS.lock().unwrap_or_else(|p| p.into_inner());
        let owners = owners.get_or_insert_with(|| ClickOwners {
            deferred: Default::default(),
            scanner: Default::default(),
            scanner_tick: 0,
            grenade: Default::default(),
        });
        // A pin-type belt prop held: its controller's fresh click is the grenade's (the script side picks the hand).
        let grenade_owner = cvr_input::grenade_stick::GRENADE_STICK_HAND.load(Ordering::Relaxed);
        let grenade_input = f.gameplay_active && f.sync_ok && focused && !menu && !cvr_menu_lobby_world_shown() && !captured;
        let grenade_claim = owners.grenade.update(grenade_owner, grenade_input, chord.left_stick_clicked, right_clicked);
        if grenade_claim == 1 {
            ctrl.buttons &= !XB_RIGHT_THUMB;
        }
        // THE SCANNER'S CLICK: the physical left hand at the left ear and a fresh left-stick click, before the click's
        // D-pad and deferred-sprint conversion.
        let tick = windows_sys::Win32::System::SystemInformation::GetTickCount64();
        let scanner_dt = if owners.scanner_tick != 0 && tick >= owners.scanner_tick { (tick - owners.scanner_tick) as f64 } else { 0.0 };
        owners.scanner_tick = tick;
        let scanner_enabled = core::ptr::read_volatile(&raw const CyberpunkVR_ScannerGesture) != 0
            && f.gameplay_active
            && f.sync_ok
            && left_click_valid
            && cvr_settings::lc!(xrXInputHook) != 0
            && !menu
            && !cvr_menu_lobby_world_shown()
            && !captured
            && grenade_claim != 0
            && !right_clicked;
        let zone = cvr_input::scanner::ScannerEarZone {
            x: core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEarX),
            y: core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEarY),
            z: core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEarZ),
            radius: core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEarRadius),
            margin: core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEarMargin),
        };
        let [sx, sy, sz] = f.scanner_hand;
        let scanner_claim = owners.scanner.update(
            scanner_enabled,
            chord.left_stick_clicked,
            f.sync_ok && ctrl.left_hand_valid,
            sx,
            sy,
            sz,
            scanner_dt,
            &zone,
            core::ptr::read_volatile(&raw const CyberpunkVR_ScannerEnterMs),
            core::ptr::read_volatile(&raw const CyberpunkVR_ScannerExitMs),
        );
        ctrl.scanner_presses = owners.scanner.presses();
        ctrl.scanner_held = scanner_enabled && owners.scanner.held();
        // THE MODIFIER (cvr_input::modifier): the left thumbrest held past 0.2 s on a Touch controller shifts the right
        // stick to the D-pad; elsewhere the left stick held in does (below).
        let rests = [THUMBRESTS[0].load(Ordering::Relaxed), THUMBRESTS[1].load(Ordering::Relaxed)];
        // CVR_TEST_THUMBREST=1 (a development switch): the thumbrest scheme on a controller without one (the
        // simulator), for the HUD's hints
        static TEST_THUMBREST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *TEST_THUMBREST.get_or_init(|| std::env::var("CVR_TEST_THUMBREST").is_ok_and(|v| v == "1")) {
            cvr_input::modifier::THUMBREST_SEEN.store(true, Ordering::Relaxed);
        }
        // a left-hander whose sticks are traded (xr_left_handed_bindings 2) holds the mirrored chord: the right
        // thumbrest, the left stick (the stick-click scheme stays on the left click: its deferral is the left stick's)
        let thumbrest = !f.psvr2 && cvr_input::modifier::thumbrest_controller();
        chord.mirrored = thumbrest && cvr_settings::lc!(xrLeftHanded) != 0 && cvr_settings::lc!(xrLeftHandedBindings) >= 2;
        cvr_input::modifier::publish_side(rests, u8::from(chord.mirrored), chord.left_stick_clicked, tick);
        cvr_input::modifier::SCHEME.store(
            if f.psvr2 {
                cvr_input::modifier::SCHEME_TRIANGLE
            } else if thumbrest {
                cvr_input::modifier::SCHEME_THUMBREST
            } else {
                cvr_input::modifier::SCHEME_CLICK
            },
            Ordering::Relaxed,
        );
        chord.thumbrest_shift = f.gameplay_active && thumbrest && cvr_input::modifier::thumbrest_held(tick) && !captured;
        // The click's owner resolved before either chord sees L3.
        dpad_chord(ctrl, &mut chord, CyberpunkVR_DpadChordStick, scanner_claim, grenade_claim != 0);
        // THE COMBAT GADGET (the pad's RB): the right grip held and the right stick pressed in for half a second, RB
        // until either lets go; the click is withheld from the game from its press, and given back as a short R3 when
        // it was let go before that (the slide's release with the grip held).
        {
            let allowed = f.gameplay_active
                && f.sync_ok
                && focused
                && !menu
                && !captured
                && !cvr_menu_lobby_world_shown()
                && grenade_claim != 1
                // L3 + R3 (the overlay) and PSVR2's Triangle + R3 are their own chords
                && !chord.left_stick_clicked
                && !chord.triangle_shift
                && !core::ptr::read_volatile(&raw const g_isInVehicle);
            let mut gadget = GADGET.lock().unwrap_or_else(|p| p.into_inner());
            let g = gadget.step(right_clicked, ctrl.right_grip, allowed, tick);
            if g.claimed {
                ctrl.buttons &= !XB_RIGHT_THUMB;
            }
            if g.rb {
                ctrl.buttons |= XB_RIGHT_SHOULDER;
            }
            if g.tap {
                ctrl.buttons |= XB_RIGHT_THUMB;
            }
            if g.rb != GADGET_RB.swap(g.rb, Ordering::Relaxed) {
                log!("[gadget] the combat gadget's chord (right grip + right stick held): RB {}\n", if g.rb { "held" } else { "released" });
            }
        }
        let claim = cvr_cyberware_chord_update(chord.left_stick_clicked, ctrl.left_grip, right_clicked,
            f.gameplay_active && !scanner_claim && grenade_claim < 0);
        if claim {
            ctrl.left_grip = 0.0;
            ctrl.buttons &= !0x000f; // no D-pad from the same chord
        }
        // THE DEFERRED L3 (sprint): emitted on a release no other owner used.
        let overlay_chord = cvr_settings::lc!(xrVrInterface) != 0 && ui[0].stick_click && right_clicked;
        let other_owner = grenade_claim == 0 || scanner_claim || claim || overlay_chord || chord.dpad_used || cvr_input::modifier::SWIPE_USED_CLICK.load(Ordering::Relaxed);
        if owners.deferred.update(chord.left_stick_clicked, other_owner) {
            ctrl.buttons |= XB_LEFT_THUMB;
            CyberpunkVR_LeftStickClicks.fetch_add(1, Ordering::SeqCst);
        }
        // seated, the click held on is the game's own L3 (the autodrive's hold)
        let driving = f.gameplay_active && f.sync_ok && focused && !menu && !captured && core::ptr::read_volatile(&raw const g_isInVehicle);
        let held_l3 = owners.deferred.hold(driving, tick);
        if held_l3 {
            ctrl.buttons |= XB_LEFT_THUMB;
        }
        // said as the hold starts, its first few times a session
        static HELD_L3: AtomicBool = AtomicBool::new(false);
        static HELD_L3_SAID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        if held_l3 && !HELD_L3.swap(true, Ordering::Relaxed) && HELD_L3_SAID.fetch_add(1, Ordering::Relaxed) < 4 {
            log!("[input] in the driver's seat the left stick's click held on: the game's own L3 held (its autodrive hold)
");
        } else if !held_l3 {
            HELD_L3.store(false, Ordering::Relaxed);
        }
        let scanner_presses = owners.scanner.presses();

        let global = |action: u64| action != 0 && read_bool(session, action, 0);
        if f.gameplay_active {
            // The Frame's extra buttons obey the overlay's capture like the Touch's; Menu/Start keeps its escape.
            const FRAME_BITS: [u16; 7] = [XB_X, XB_Y, XB_DPAD_UP, XB_DPAD_DOWN, XB_DPAD_LEFT, XB_DPAD_RIGHT, 0x0020];
            for (k, bit) in FRAME_BITS.iter().enumerate() {
                if global(a.steam_frame[k]) {
                    ctrl.buttons |= bit;
                }
            }
        }
        {
            let g = INPUT.lock.lock();
            *INPUT.raw.get(&g) = *ctrl;
        }
        if cvr_vrui_captures_input() {
            *ctrl = ControllerState::default();
            ctrl.scanner_presses = scanner_presses;
        }
        if f.gameplay_active {
            let menu_button = global(a.menu);
            if f.psvr2 {
                // L3+R3 belongs to the overlay, not to this menu fallback.
                let chorded = chord.triangle_shift && right_clicked && !chord.left_stick_clicked;
                ctrl.psvr2_system_available = f.sync_ok && focused;
                ctrl.psvr2_system_pressed = menu_button || (chorded && !cvr_vrui_captures_input());
                ctrl.psvr2_dpad_shift = chord.triangle_shift && !cvr_vrui_captures_input();
                if chorded {
                    ctrl.buttons &= !XB_RIGHT_THUMB;
                }
                if menu_button && !MENU_LOGGED.swap(true, Ordering::Relaxed) {
                    log!("OpenXRManager[PSVR2]: Create SystemButton active (tap=Start, hold=Back).\n");
                }
            } else if menu_button {
                ctrl.buttons |= XB_START; // the other headsets unchanged
            }
        }
        // The pulls sampled under the input lock, from the published state (with the gameplay actions or without).
        let g = INPUT.lock.lock();
        let finger_input = f.gameplay_active && f.sync_ok && focused && !menu && !captured;
        let published = INPUT.controller.get(&g);
        if f.gameplay_active {
            *published = *ctrl;
        }
        let mut fingers = FINGERS.lock().unwrap_or_else(|p| p.into_inner());
        published.finger_pull[0] = fingers[0].sample(finger_input && trigger_active[0], ctrl.left_trigger);
        published.finger_pull[1] = fingers[1].sample(finger_input && trigger_active[1], ctrl.right_trigger);
    }
}


/// The finger tracking's verdict on the grip (formerly the frame loop's cvr_cpp_finger_grip): -1 for the grip sensor's
/// own value, otherwise 0 (open) or 1 (fist, thumbs-up) when xr_grip_from_fingers takes the grip from the fingers.
#[cfg(not(test))]
unsafe fn finger_grip(hand: i32, local_space: u64, time: i64) -> i32 {
    // SAFETY: cvr-rig's finger tracking with the session's space.
    let fingers = unsafe { cvr_fingergrip_evaluate(hand, local_space, time) };
    // SAFETY: a plain switch read.
    if fingers >= 0 && unsafe { cvr_fingergrip_grip_from_fingers() } {
        fingers
    } else {
        -1
    }
}
/// The overlay is shown or takes the input: the hands' aims are located for its pointer.
fn vrui_pointing() -> bool {
    cvr_vrui_visible() || cvr_vrui_captures_input()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hand(stick_x: f32, stick_y: f32, click: bool) -> HandInput {
        HandInput { stick_x, stick_y, click, ..Default::default() }
    }
    #[test]
    fn the_left_click_turns_the_right_stick_into_the_dpad() {
        let mut c = ControllerState::default();
        let mut chord = Chord::default();
        map_hand(&mut c, 0, &hand(0.3, -0.2, true), &mut chord);
        map_hand(&mut c, 1, &hand(0.95, 0.5, false), &mut chord);
        dpad_chord(&mut c, &mut chord, 0.0, false, true);
        assert_eq!(c.buttons, XB_DPAD_RIGHT);
        assert!(chord.dpad_used);
        assert_eq!((c.right_thumb_x, c.right_thumb_y), (0.0, 0.0), "the look is held still for the whole chord");
        assert_eq!((c.left_thumb_x, c.left_thumb_y), (0.3, -0.2));
        // below the threshold: nothing picked, the look still zeroed
        let mut c = ControllerState::default();
        let mut chord = Chord::default();
        map_hand(&mut c, 0, &hand(0.0, 0.0, true), &mut chord);
        map_hand(&mut c, 1, &hand(0.5, 0.5, false), &mut chord);
        dpad_chord(&mut c, &mut chord, 0.9, false, true);
        assert_eq!(c.buttons, 0);
        assert!(!chord.dpad_used);
        assert_eq!(c.right_thumb_x, 0.0);
        // the scanner's click (or a grenade's) is no D-pad modifier: the look stays
        let mut c = ControllerState::default();
        let mut chord = Chord::default();
        map_hand(&mut c, 0, &hand(0.0, 0.0, true), &mut chord);
        map_hand(&mut c, 1, &hand(0.95, 0.0, false), &mut chord);
        dpad_chord(&mut c, &mut chord, 0.9, true, true);
        assert_eq!((c.buttons, c.right_thumb_x), (0, 0.95));
        dpad_chord(&mut c, &mut chord, 0.9, false, false);
        assert_eq!((c.buttons, c.right_thumb_x), (0, 0.95));
    }
    #[test]
    fn without_the_chord_the_buttons_are_xinputs() {
        let mut c = ControllerState::default();
        let mut chord = Chord::default();
        let left = HandInput { primary: true, secondary: true, bumper: true, trigger: 0.5, grip: 0.25, ..Default::default() };
        let right = HandInput { click: true, primary: true, bumper: true, stick_x: 0.7, ..Default::default() };
        map_hand(&mut c, 0, &left, &mut chord);
        map_hand(&mut c, 1, &right, &mut chord);
        dpad_chord(&mut c, &mut chord, 0.9, false, true);
        assert_eq!(c.buttons, XB_X | XB_Y | XB_LEFT_SHOULDER | XB_RIGHT_THUMB | XB_A | XB_RIGHT_SHOULDER);
        assert_eq!((c.left_trigger, c.left_grip, c.right_thumb_x), (0.5, 0.25, 0.7));
    }
    #[test]
    fn the_triangle_shift_picks_past_half() {
        let mut c = ControllerState::default();
        let mut chord = Chord::default();
        map_hand(&mut c, 0, &HandInput { triangle: true, ..Default::default() }, &mut chord);
        map_hand(&mut c, 1, &hand(-0.6, 0.1, false), &mut chord);
        dpad_chord(&mut c, &mut chord, 0.9, false, true);
        assert_eq!(c.buttons, XB_DPAD_LEFT);
        assert!(chord.dpad_used && chord.triangle_shift);
        assert_eq!(c.right_thumb_x, 0.0);
    }
}

