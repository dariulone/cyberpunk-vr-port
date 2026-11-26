//! The XInput merge (formerly src/Hooks/XInput.cpp): the VR controllers ORed into gamepad 0 by patching the game's
//! XInputGetState imports, with the port's remaps for gameplay -- the grips as menu shoulders, the scanner at the left
//! ear and its one-handed navigation, the weapon trigger's two stages, dual wielding, discrete movement speeds, the
//! sprint detent, the crouch and dash on the right stick, snap turn, steering by the wheel, the gun in a car, the
//! vehicle exit held on X, the two-gun Y split and the port's weapon wheel. The reasoning behind every rule is the
//! C++ file's, kept there beside the exports and the registration; what follows is the same code, statement for
//! statement. What the merge asks of the rest of the plugin comes through src/Hooks/InputServices.cpp.
use super::cyberware::{cvr_cyberware_publish_grips, cvr_cyberware_selected_kind};
use super::env::{g_VRBind, g_VRLocomotionState, g_isInVehicle, log};
use super::gestures::cvr_gestures_publish_input;
use super::weapon_arm::cvr_anim_is_head_aim_weapon_active;
use super::wheel_grab::{cvr_anim_wheel_gamepad_steer, cvr_anim_wheel_steer_input};
use core::ffi::c_void;
use cvr_body::policy::{native_owns_turn, route_controller_turn, vehicle_button_policy, VehicleExitHold};
use cvr_input::dual_wield::DualWieldRapidFire;
use cvr_input::handedness::{mirrored_buttons, trades_buttons, trades_sticks};
use cvr_input::psvr2::SenseSystemButton;
use cvr_input::scanner::{ScannerRequest, SCANNER_HAND_PHYSICS};
use cvr_input::secondary::GameplaySecondary;
use cvr_input::stick::{analog_axis, at_full_travel, full_input, use_analog_movement, StickPulse};
use cvr_input::trigger::{FingerAction, FingerButton};
use cvr_input::wheel_hold::WheelHold;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// VRControllerState (include/Runtimes/OpenXRManager.hpp), field for field; InputServices.cpp checks the layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VrController {
    pub buttons: u16,
    pub left_trigger: f32,
    pub right_trigger: f32,
    /// The current physical pull per hand (the trigger's hysteresis at the XR sync), 0 released.
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
    /// The physical left-ear L3 gesture (cvr-xr input.rs): the scanner's fresh presses, counted, and its click held.
    pub scanner_presses: u64,
    pub scanner_held: bool,
}
const _: () = assert!(core::mem::size_of::<VrController>() == 80);
/// XINPUT_GAMEPAD.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gamepad {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}
/// XINPUT_STATE.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XState {
    pub packet: u32,
    pub pad: Gamepad,
}
/// The live controls the merge reads (g_liveControls), filled by super::live_reads::input_controls.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InputControls {
    pub xinput_hook: i32,
    pub left_handed: i32,
    pub left_handed_bindings: i32,
    pub ladder_grip_climb: i32,
    pub movement_speed_mode: i32,
    pub sprint_mode: i32,
    pub vehicle_gun_trigger: i32,
    pub wheel_horn: i32,
    pub disable_mouse_y: i32,
    pub snap_turn: i32,
    pub left_stick_deadzone: f32,
    pub right_stick_deadzone: f32,
    pub max_input_threshold: f32,
    pub vehicle_throttle_trim: f32,
    pub snap_turn_angle_deg: f32,
}
/// cvr::swimming::Input.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SwimInput {
    pub forward: f32,
    pub water: bool,
    pub ascend: f32,
    pub dive: bool,
    pub boost: bool,
}

const ERROR_SUCCESS: u32 = 0;
const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;
const LEFT_THUMB: u16 = 0x0040;
const RIGHT_THUMB: u16 = 0x0080;
const LEFT_SHOULDER: u16 = 0x0100;
const RIGHT_SHOULDER: u16 = 0x0200;
const K_LEFT_TRIGGER_ANALOG: i32 = 154;
const K_LEFT_GRIP_PRESSED: i32 = 155;
const K_DEBUG_LOG: i32 = 156;
const K_RIGHT_SECONDARY_BTN: i32 = 157;
const K_LEFT_SECONDARY_BTN: i32 = 158;
const K_RIGHT_STICK_CLICK: i32 = 159;
const K_RIGHT_TRIGGER_ANALOG: i32 = 160;
const K_TRIGGER_OVERRIDE: i32 = 161;
const K_DEVICE_SCREEN_OPEN: usize = 164;
const K_DUAL_WIELD_ACTIVE: i32 = 165;
const K_DUAL_WIELD_LEFT_FIRES: i32 = 2;
const K_DUAL_WIELD_RIGHT_FIRES: i32 = 4;
/// A brief request from rapid repeated breaks on both triggers (the game's Heywood Hurricane).
const K_DUAL_WIELD_HURRICANE: i32 = 8;
const K_DUAL_WIELD_Y_TAP: i32 = 57;
/// B PRESSES ON A POPUP IN THE CAR, counted for the driving mechanic (the radio list's own close never comes: B is held
/// out of the car, it is also the exit).
const K_VEHICLE_POPUP_BACK: i32 = 175;
const DPAD_LEFT: u16 = 0x0004;

/// INPUT with a KEYBDINPUT.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct KeyInput {
    pub(super) ty: u32,
    pub(super) _pad: u32,
    pub(super) vk: u16,
    pub(super) scan: u16,
    pub(super) flags: u32,
    pub(super) time: u32,
    pub(super) extra: usize,
    pub(super) _union_rest: [u8; 8],
}
const _: () = assert!(core::mem::size_of::<KeyInput>() == 40);

extern "C" {
    static CyberpunkVR_TestPadButtons: i32;
    static CyberpunkVR_InputDefaultInUi: i32;
    static CyberpunkVR_PopupMagBlockMs: i32;
    static CyberpunkVR_MoveTiers: i32;
    static CyberpunkVR_SprintHoldMs: i32;
    static CyberpunkVR_SprintFromCrouch: i32;
    static CyberpunkVR_LeftStickClicks: i32;
    static CyberpunkVR_SprintPulseMs: i32;
    static CyberpunkVR_SprintGapMs: i32;
    static CyberpunkVR_SprintGiveUpMs: i32;
    static CyberpunkVR_MoveWalkMax: f32;
    static CyberpunkVR_MoveWalkMag: f32;
    static CyberpunkVR_MoveWalkMargin: f32;
    static CyberpunkVR_LeftTriggerBlockUntil: u64;
    static CyberpunkVR_DualWieldUntil: u64;
    static CyberpunkVR_DualWieldInput: i32;
    static CyberpunkVR_DualWieldPressDelayMs: i32;
    static CyberpunkVR_DualWieldYHoldMs: i32;
    static CyberpunkVR_ScannerGesture: i32;
    static CyberpunkVR_ScannerToggle: i32;
    static CyberpunkVR_ScannerButtonToggle: i32;
    static CyberpunkVR_CrouchPulseMs: i32;
    static CyberpunkVR_TestPadLeft: AtomicU64;
    static CyberpunkVR_TestPadTriggers: AtomicU64;
    static CyberpunkVR_VehicleDialogConfirmOnA: i32;
    static CyberpunkVR_ScannerStickNav: i32;
    static CyberpunkVR_ScannerStickNavFire: f32;
    static CyberpunkVR_ScannerStickNavRearm: f32;
    static CyberpunkVR_ScannerFaceNav: i32;
    static CyberpunkVR_ScannerTriggerTag: i32;
    static CyberpunkVR_CarryLeft: i32;
    static CyberpunkVR_ScannerStickTab: i32;
    static CyberpunkVR_ScannerZoom: i32;
    static CyberpunkVR_ScannerZoomStick: f32;
    static CyberpunkVR_ScannerZoomRepeatMs: i32;
    static CyberpunkVR_ScannerPulseMs: i32;
    static CyberpunkVR_VehicleExitHoldMs: i32;
    static CyberpunkVR_DashStickUp: i32;
    static CyberpunkVR_SnapTurnStickFire: f32;
    static CyberpunkVR_SnapTurnStickRearm: f32;
    static CyberpunkVR_DashPulseMs: i32;
    static CyberpunkVR_LtLighterGate: i32;
    static mut CyberpunkVR_DebugWheelHornGestures: u64;
    static CyberpunkVR_WeaponClass: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_hasWeaponEquipped: bool;
    static g_VRSprintActive: i32;
    static g_wheelBlendRight: AtomicU32;
    static g_wheelBlendLeft: AtomicU32;
    static g_wheelHornMask: AtomicI32;
    // src/Hooks/InputServices.cpp (and the services the other hooks share)
    fn cvr_cpp_xr_set_shared_slot(slot: i32, value: f32);
    fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32;
    fn cvr_cpp_quest_suspend_manual_clue_input();
    fn cvr_cpp_quest_consume_manual_clue_button(pressed: bool, scanned: bool) -> bool;
    fn cvr_cpp_swimming_reset_input();
    fn cvr_cpp_swimming_update_input(controllers: *const VrController, manual_back: bool, out: *mut SwimInput);
    fn cvr_cpp_swimming_active() -> bool;
    fn cvr_cpp_swimming_sprint_input(wanted: bool) -> bool;
    fn cvr_cpp_ladder_reset_input();
    fn cvr_cpp_ladder_update_input(controllers: *const VrController);
    fn cvr_cpp_ladder_active() -> bool;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_reach_use_held() -> bool;
    fn cvr_cpp_cyberware_chord_dispatch();
    fn cvr_cpp_vrmenu_captures_buttons() -> bool;
    fn cvr_cpp_vrmenu_wheel_armed(now: u64) -> bool;
    fn cvr_cpp_vrmenu_publish_wheel_hold(pending: bool, held: bool);
    fn cvr_cpp_keypad_input_active() -> bool;
    fn cvr_cpp_keypad_publish_stick(x: f32, y: f32, deadzone: f32);
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_lobby_curtain() -> f32;
    fn cvr_cpp_lobby_intro_running() -> bool;
    fn cvr_cpp_in_braindance() -> bool;
    fn cvr_cpp_scene_stick_yaw_armed() -> bool;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_pose_vehicle_state() -> i32;
    fn cvr_cpp_shot_shared() -> *mut f32;
    fn cvr_cpp_take_pending_snap_yaw() -> i32;
    fn cvr_cpp_set_pending_snap_yaw(bits: i32);
    fn cvr_cpp_ui_popup_open() -> i32;
    fn cvr_cpp_ui_popup_closed_ms() -> u64;
    fn cvr_cpp_set_look_stick_x(x: f32);
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_is_driving() -> bool;
    fn cvr_cpp_game_window_in_front() -> bool;
    fn GetTickCount() -> u32;
    fn cvr_cpp_send_input(count: u32, inputs: *const c_void, size: i32) -> u32;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn ld(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}
/// VRIK_ClockQpc and its frequency (the tests' 1 MHz counter when they set one).
fn qpc_now() -> (i64, i64) {
    super::env::qpc()
}
/// VRIK_ClockMs (the tests' clock when they set one).
fn tick() -> u64 {
    // SAFETY: a function pointer the tests set, or GetTickCount64.
    unsafe { super::gestures::now_ms() }
}

fn float_to_short(v: f32) -> i16 {
    let v = if v > 1.0 {
        1.0
    } else if v < -1.0 {
        -1.0
    } else {
        v
    };
    if v >= 0.0 {
        (v * 32767.0) as i16
    } else {
        (v * 32768.0) as i16
    }
}
fn float_to_byte(v: f32) -> u8 {
    let v = if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    };
    (v * 255.0) as u8
}
fn apply_stick_deadzone(v: f32, dz: f32) -> f32 {
    let a = if v < 0.0 { -v } else { v };
    if a < dz {
        return 0.0;
    }
    let s = if v < 0.0 { -1.0 } else { 1.0 };
    s * (a - dz) / (1.0 - dz)
}

/// Down and up of one extended key in one call: the game consumes raw input per frame.
unsafe fn send_key(vk: u16) {
    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_EXTENDEDKEY: u32 = 0x1;
    const KEYEVENTF_KEYUP: u32 = 0x2;
    let down = KeyInput { ty: INPUT_KEYBOARD, _pad: 0, vk, scan: 0, flags: KEYEVENTF_EXTENDEDKEY, time: 0, extra: 0, _union_rest: [0; 8] };
    let up = KeyInput { flags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, ..down };
    let keys = [down, up];
    cvr_cpp_send_input(2, keys.as_ptr() as *const c_void, core::mem::size_of::<KeyInput>() as i32);
}
/// The scanner's zoom (VK_END in, VK_DELETE out: never VK_INSERT, the overlay's toggle).
unsafe fn send_zoom_key(zoom_in: bool) {
    send_key(if zoom_in { 0x23 } else { 0x2E });
}
/// The hack list (the arrows: not the D-pad, which also zooms).
unsafe fn send_list_key(up: bool) {
    send_key(if up { 0x26 } else { 0x28 });
}

// ---- the published game state other parts of the plugin read (cvr::input::*)

static WEAPON_TRIGGER_ENABLED: AtomicBool = AtomicBool::new(false);
static WEAPON_TRIGGER_STAMP: AtomicU64 = AtomicU64::new(0);
static FINGER_ACTION: AtomicU64 = AtomicU64::new(0);
static SCANNER_VISION: AtomicU64 = AtomicU64::new(0);
static SPRINT_INPUT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// PrimaryWeaponRevision (main's Hooks/LocateCamera.cpp): bumped by the player state's refresh (cvr-ffi
/// player_camera_state.rs) whenever the weapon in the primary hand changes, so a trigger held through an equip never
/// presses for the new weapon.
pub static PRIMARY_WEAPON_REVISION: AtomicU64 = AtomicU64::new(0);
/// The physics hands' finger-layer trace (main's cvr::physhands::Tracing, the PhysHands settings' trace=1): the right
/// grip's published reason is logged while it is on. Set by the physics hands' settings.
pub static PHYSHANDS_TRACE: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn cvr_input_publish_scanner_vision(state: i32) {
    SCANNER_HAND_PHYSICS.publish(state);
    SCANNER_VISION.store((tick() << 1) | (state == 1) as u64, Ordering::Release);
}
/// The world's time dilation (main's 0.2.4 SetVRWorldTimeScale): world slow motion holds the tracked hands' physics
/// like the scanner does -- independent reasons, leaving one never resumes physics the other still holds.
#[no_mangle]
pub extern "C" fn cvr_input_world_time_scale(scale: f32) {
    SCANNER_HAND_PHYSICS.time_scale(scale);
}
/// CurrentFingerAction (VRCurrentFingerAction): the weapon finger's fresh pull for the script side's semi-automatic
/// fire, 0 without one (the trigger route off, the session unfocused).
#[no_mangle]
pub extern "C" fn cvr_input_current_finger_action() -> u64 {
    // SAFETY: the XR manager's focus flag.
    if !cvr_input_weapon_trigger_enabled() || !unsafe { super::xr_calls::session_focused() } {
        return 0;
    }
    FINGER_ACTION.load(Ordering::Acquire)
}
#[no_mangle]
pub extern "C" fn cvr_input_scanner_vision() -> i32 {
    let sample = SCANNER_VISION.load(Ordering::Acquire);
    let (stamp, now) = (sample >> 1, tick());
    if stamp != 0 && now >= stamp && now - stamp <= 350 {
        (sample & 1) as i32
    } else {
        -1
    }
}
#[no_mangle]
pub extern "C" fn cvr_input_weapon_trigger_enabled() -> bool {
    WEAPON_TRIGGER_ENABLED.load(Ordering::Relaxed) && tick().wrapping_sub(WEAPON_TRIGGER_STAMP.load(Ordering::Relaxed)) < 250
}

// ---- the merge's state (the C++ function's statics)

#[derive(Default)]
struct Merge {
    snap_armed_dir: i32,
    sense: SenseSystemButton,
    packet_prev: Gamepad,
    packet_valid: bool,
    packet_seq: u32,
    grip_shoulder_armed: [bool; 2],
    weapon_trigger: FingerButton,
    finger_action: FingerAction,
    secondary: GameplaySecondary,
    scanner_presses_seen: u64,
    scanner_latch: bool,
    request: ScannerRequest,
    lb_was: bool,
    btn_latch: bool,
    was_takeover: bool,
    btn_request: ScannerRequest,
    right_grip_why: i32,
    x_was_down: bool,
    r3_was_down: bool,
    rt_was_down: bool,
    up_until: u64,
    down_until: u64,
    apply_until: u64,
    tag_until: u64,
    tab_until: u64,
    dual_left: bool,
    dual_right: bool,
    dual_hold_until: u64,
    dual_break: [FingerButton; 2],
    dual_rapid: DualWieldRapidFire,
    nav_armed_dir: i32,
    walking: bool,
    detent_ms: f64,
    phase_ms: f64,
    ask_ms: f64,
    gave_up: bool,
    sprint_qpc: i64,
    clicks_seen: i32,
    click_sprint: bool,
    pushed_since_click: bool,
    rest_ms: f64,
    lt_zoom_was: bool,
    next_step_ms: u64,
    crouch_pulse: StickPulse,
    dash_armed_dir: i32,
    dash_until: u64,
    a_was: bool,
    exit_hold: VehicleExitHold,
    b_prev: bool,
    popup_backs: f32,
    scan_output: u64,
    last_hold: bool,
    y_down_at: u64,
    y_pass: bool,
    y_taps: f32,
    wheel_hold: WheelHold,
    psvr2_logged: u16,
}
static MERGE: LazyLock<Mutex<Merge>> = LazyLock::new(|| Mutex::new(Merge { clicks_seen: -1, right_grip_why: -1, ..Merge::default() }));

impl Merge {
    fn publish_packet(&mut self, state: &mut XState) {
        // Axes matter too: a swimming glide must publish its final zero.
        if !self.packet_valid || self.packet_prev != state.pad {
            self.packet_prev = state.pad;
            self.packet_valid = true;
            self.packet_seq = self.packet_seq.wrapping_add(1);
        }
        state.packet = self.packet_seq;
    }
    /// The right grip's published flag (shared [49]) and why it is what it is, logged when that changes (the physics
    /// hands' trace): 0 released, 1 a ladder, 2 the cyberware, 3 held, 4 a VR UI capture, 5 a menu or braindance, 6
    /// the VR menu's capture.
    fn note_right_grip(&mut self, why: i32, raw: f32) {
        if why == self.right_grip_why {
            return;
        }
        if PHYSHANDS_TRACE.load(Ordering::Relaxed) {
            log(&format!("[xinput] right grip published {} ({}, the controller's {:.2})\n", if why == 3 { "held" } else { "off" }, why, raw));
        }
        self.right_grip_why = why;
    }
    /// The trigger route closed (the hook off, no controllers, a capture, a menu): no press survives it.
    fn close_trigger_route(&mut self) {
        self.secondary.reset();
        self.weapon_trigger = FingerButton::default();
        WEAPON_TRIGGER_ENABLED.store(false, Ordering::Relaxed);
    }
    fn sense_event(&mut self, pressed: bool, available: bool) -> u16 {
        let event = self.sense.update(pressed, tick(), available);
        if event != 0 && self.psvr2_logged & event == 0 {
            self.psvr2_logged |= event;
            log(&format!("XInput[PSVR2]: SystemButton mapped to {}.\n", if event == 0x10 { "Start/Pause" } else { "Back/Select" }));
        }
        event
    }
}

fn mirrored(s: VrController, bindings: i32) -> VrController {
    let mut out = s;
    out.buttons = mirrored_buttons(s.buttons, trades_buttons(bindings), trades_sticks(bindings));
    out.left_trigger = s.right_trigger;
    out.right_trigger = s.left_trigger;
    if trades_sticks(bindings) {
        out.left_thumb_x = s.right_thumb_x;
        out.left_thumb_y = s.right_thumb_y;
        out.right_thumb_x = s.left_thumb_x;
        out.right_thumb_y = s.left_thumb_y;
    }
    out.left_grip = s.right_grip;
    out.right_grip = s.left_grip;
    out.left_hand_valid = s.right_hand_valid;
    out.right_hand_valid = s.left_hand_valid;
    out
}

/// The XInputGetState the game's imports now point at: the real one first, then the VR merge into pad 0.
#[no_mangle]
pub unsafe extern "C" fn cvr_xinput_merge(user: u32, state: *mut XState, real_result: u32) -> u32 {
    let mut r = real_result;
    if state.is_null() || user != 0 {
        return r;
    }
    let mut m = MERGE.lock().unwrap_or_else(|e| e.into_inner());
    let lc = {
        let mut c = InputControls::default();
        super::live_reads::input_controls(&mut c);
        c
    };
    // The road wheels' curve for the car whose wheel a hand holds (steer_curve.rs). Ahead of every early return below:
    // a menu, an unfocused session or the hook switched off must still put the car's own values back.
    super::steer_curve::tick(lc.xinput_hook != 0 && super::wheel_grab::cvr_anim_wheel_holds_car());
    let st = &mut *state;
    let mut vr = VrController::default();
    if lc.xinput_hook == 0 || !super::xr_calls::controller_state(&mut vr) {
        m.close_trigger_route();
        cvr_cpp_xr_set_shared_slot(K_RIGHT_SECONDARY_BTN, 0.0);
        m.sense_event(false, false);
        cvr_cpp_quest_suspend_manual_clue_input();
        cvr_cpp_swimming_reset_input();
        cvr_cpp_ladder_reset_input();
        return r;
    }
    vr.buttons |= (CyberpunkVR_TestPadButtons & 0xFFFF) as u16;
    if CyberpunkVR_TestPadButtons & 0x10000 != 0 {
        vr.left_grip = 1.0;
    }
    if CyberpunkVR_TestPadButtons & 0x20000 != 0 {
        vr.right_grip = 1.0;
    }
    // The bounded test stick: X and Y in the low DWORD, a GetTickCount deadline in the high one (ten seconds at most).
    let test = CyberpunkVR_TestPadLeft.load(Ordering::Relaxed);
    if test != 0 {
        let remaining = ((test >> 32) as u32).wrapping_sub(GetTickCount()) as i32;
        if remaining > 0 && remaining <= 10000 {
            vr.left_thumb_x = (test & 0xFFFF) as u16 as i16 as f32 / 32767.0;
            vr.left_thumb_y = ((test >> 16) & 0xFFFF) as u16 as i16 as f32 / 32767.0;
        }
    }
    // The bounded test triggers, the same way (the left in bits 0-7, the right in bits 8-15).
    let test = CyberpunkVR_TestPadTriggers.load(Ordering::Relaxed);
    if test != 0 {
        let remaining = ((test >> 32) as u32).wrapping_sub(GetTickCount()) as i32;
        if remaining > 0 && remaining <= 10000 {
            vr.left_trigger = vr.left_trigger.max((test & 0xFF) as f32 / 255.0);
            vr.right_trigger = vr.right_trigger.max(((test >> 8) & 0xFF) as f32 / 255.0);
        }
    }
    // PSVR2: the Sense controller's Create button, tap = Start, hold = Back.
    let available = vr.psvr2_system_available && !cvr_cpp_vrui_captures_input();
    vr.buttons |= m.sense_event(vr.psvr2_system_pressed, available);
    // LEFT-HANDED, THE CONTROLLERS TRADE ROLES HERE, once: from here `vr` is what they mean to the game, `physical`
    // the two controllers as they are. The scanner always uses the physical left hand.
    let mut physical = vr;
    let fresh_scanner_presses = physical.scanner_presses.saturating_sub(m.scanner_presses_seen);
    m.scanner_presses_seen = physical.scanner_presses;
    let left_handed = lc.left_handed != 0;
    if left_handed {
        vr = mirrored(vr, lc.left_handed_bindings);
    }
    if cvr_cpp_reach_use_held() {
        vr.buttons |= 0x4000;
    }
    cvr_cpp_cyberware_chord_dispatch();

    if r != ERROR_SUCCESS {
        *st = XState::default();
        r = ERROR_SUCCESS;
    }
    if vr.psvr2_dpad_shift {
        st.pad.thumb_rx = 0;
        st.pad.thumb_ry = 0;
        vr.right_thumb_x = 0.0;
        vr.right_thumb_y = 0.0;
        m.snap_armed_dir = 0;
        cvr_cpp_take_pending_snap_yaw();
    }
    if cvr_cpp_quest_consume_manual_clue_button(((vr.buttons | st.pad.buttons) & 0x4000) != 0, true) {
        vr.buttons &= !0x4000;
        st.pad.buttons &= !0x4000;
    }
    if cvr_cpp_vrui_captures_input() {
        m.close_trigger_route();
        m.scanner_latch = false;
        st.pad = Gamepad::default();
        cvr_cpp_set_look_stick_x(0.0);
        cvr_cpp_swimming_reset_input();
        cvr_cpp_ladder_reset_input();
        for slot in [30, 49, K_RIGHT_SECONDARY_BTN, K_LEFT_SECONDARY_BTN, K_RIGHT_STICK_CLICK, K_LEFT_GRIP_PRESSED, K_RIGHT_TRIGGER_ANALOG, K_LEFT_TRIGGER_ANALOG] {
            cvr_cpp_xr_set_shared_slot(slot, 0.0);
        }
        m.note_right_grip(4, physical.right_grip);
        m.publish_packet(st);
        return r;
    }
    let hand_input = physical;
    let menu_capture = cvr_cpp_vrmenu_captures_buttons();
    if menu_capture {
        vr.buttons &= 0x00FF;
        vr.left_grip = 0.0;
        vr.right_grip = 0.0;
        physical.buttons &= 0x00FF;
        physical.left_grip = 0.0;
        physical.right_grip = 0.0;
    }
    let menu_mask = |st: &mut XState| {
        if !menu_capture {
            return;
        }
        st.pad.left_trigger = 0;
        st.pad.right_trigger = 0;
        st.pad.buttons &= 0x00FF;
    };
    let mut swim = SwimInput::default();
    // Through the service (rust/crates/cvr-render/src/cpp_services.rs in the plugin): vrik_tests' bench records it.
    cvr_cpp_swimming_update_input(&vr, vr.left_thumb_y < -0.2 || st.pad.thumb_ly < -6553, &mut swim);
    let in_water = cvr_cpp_swimming_active();
    cvr_cpp_ladder_update_input(&physical);
    let on_ladder = cvr_cpp_ladder_active() && lc.ladder_grip_climb != 0;

    let ui_popup_open = cvr_cpp_ui_popup_open() != 0;
    let port_owned: u16 = 0x0080 | if g_hasWeaponEquipped && !ui_popup_open { 0x2000 } else { 0 };
    let keypad_input = cvr_cpp_keypad_input_active();
    let menu_mode = cvr_cpp_menu_mode();
    let gameplay_screen = menu_mode == 0 && !keypad_input;
    let bd_active = cvr_cpp_braindance_active();
    cvr_gestures_publish_input(
        hand_input.left_grip,
        hand_input.right_grip,
        hand_input.left_trigger,
        hand_input.right_trigger,
        (hand_input.buttons & 0x0080) != 0,
        (gameplay_screen && !ui_popup_open && !bd_active) || cvr_cpp_lobby_world_shown(),
    );
    let mounted = g_isInVehicle;
    let vehicle_state = cvr_cpp_pose_vehicle_state();
    let vehicle_buttons = vehicle_button_policy(mounted, vehicle_state);
    let mut owned_now: u16 = 0;
    if gameplay_screen {
        owned_now = if vehicle_buttons.owned != 0 { vehicle_buttons.owned } else { port_owned };
    }
    st.pad.buttons |= vr.buttons & !owned_now;

    // MENU-ONLY: the grips stand in for the shoulders, a squeeze that starts inside the menu.
    let in_braindance = cvr_cpp_in_braindance();
    let mut menu_open_for_grip_shoulders = (menu_mode != 0 || in_braindance) && !cvr_cpp_lobby_world_shown();
    if !menu_open_for_grip_shoulders && !cvr_cpp_lobby_world_shown() {
        let sh = cvr_cpp_shot_shared();
        if !sh.is_null() && core::ptr::read_volatile((sh as *const u32).add(81)) != 0 {
            menu_open_for_grip_shoulders = true;
        }
    }
    let mut grip_shoulders: u16 = 0;
    {
        let grips = [physical.left_grip, physical.right_grip];
        for h in 0..2 {
            if !menu_open_for_grip_shoulders || menu_capture {
                m.grip_shoulder_armed[h] = false;
            } else if grips[h] < 0.5 {
                m.grip_shoulder_armed[h] = true;
            }
        }
        if m.grip_shoulder_armed[1] && physical.right_grip >= 0.7 {
            grip_shoulders |= RIGHT_SHOULDER;
        }
        if m.grip_shoulder_armed[0] && physical.left_grip >= 0.7 {
            grip_shoulders |= LEFT_SHOULDER;
        }
    }

    // THE VANILLA PAD, WHILE A MENU OR A BRAINDANCE IS UP.
    if keypad_input || (CyberpunkVR_InputDefaultInUi != 0 && (menu_mode != 0 || in_braindance || bd_active)) {
        m.close_trigger_route();
        m.scanner_latch = false;
        st.pad.buttons |= vr.buttons;
        if !keypad_input && (in_braindance || !bd_active) {
            st.pad.buttons |= grip_shoulders;
        }
        let lt_ui = float_to_byte(vr.left_trigger);
        let rt_ui = float_to_byte(vr.right_trigger);
        if lt_ui > st.pad.left_trigger {
            st.pad.left_trigger = lt_ui;
        }
        if rt_ui > st.pad.right_trigger {
            st.pad.right_trigger = rt_ui;
        }
        let (mut left_x, mut left_y) = (vr.left_thumb_x, vr.left_thumb_y);
        if keypad_input {
            let pad_x = st.pad.thumb_lx as f32 / 32767.0;
            let pad_y = st.pad.thumb_ly as f32 / 32767.0;
            if pad_x.abs() > left_x.abs() {
                left_x = pad_x;
            }
            if pad_y.abs() > left_y.abs() {
                left_y = pad_y;
            }
            cvr_cpp_keypad_publish_stick(left_x, left_y, lc.left_stick_deadzone);
            left_x = 0.0;
            left_y = 0.0;
            st.pad.thumb_lx = 0;
            st.pad.thumb_ly = 0;
        }
        if left_x.abs() > (st.pad.thumb_lx as f32 / 32767.0).abs() {
            st.pad.thumb_lx = float_to_short(left_x);
        }
        if left_y.abs() > (st.pad.thumb_ly as f32 / 32767.0).abs() {
            st.pad.thumb_ly = float_to_short(left_y);
        }
        if vr.right_thumb_x.abs() > (st.pad.thumb_rx as f32 / 32767.0).abs() {
            st.pad.thumb_rx = float_to_short(vr.right_thumb_x);
        }
        cvr_cpp_set_look_stick_x(st.pad.thumb_rx as f32 / 32767.0);
        if vr.right_thumb_y.abs() > (st.pad.thumb_ry as f32 / 32767.0).abs() {
            st.pad.thumb_ry = float_to_short(vr.right_thumb_y);
        }
        // THE LOBBY: the triggers are its hands', the right stick fully down crouches, nothing moves until it is there.
        if cvr_cpp_lobby_world_shown() {
            st.pad.left_trigger = 0;
            st.pad.right_trigger = 0;
            if cvr_cpp_lobby_curtain() > 0.0 || cvr_cpp_lobby_intro_running() {
                st.pad.thumb_lx = 0;
                st.pad.thumb_ly = 0;
                st.pad.thumb_rx = 0;
                st.pad.thumb_ry = 0;
                cvr_cpp_set_look_stick_x(0.0);
            } else if at_full_travel(vr.right_thumb_y, use_analog_movement(lc.movement_speed_mode, false), full_input(lc.max_input_threshold), true) {
                st.pad.thumb_ry = 0;
                st.pad.buttons |= RIGHT_THUMB;
            }
        }
        cvr_cpp_xr_set_shared_slot(30, 0.0);
        cvr_cpp_xr_set_shared_slot(49, 0.0);
        m.note_right_grip(5, physical.right_grip);
        cvr_cpp_xr_set_shared_slot(K_RIGHT_SECONDARY_BTN, 0.0);
        cvr_cpp_xr_set_shared_slot(K_LEFT_SECONDARY_BTN, 0.0);
        cvr_cpp_xr_set_shared_slot(K_RIGHT_STICK_CLICK, 0.0);
        cvr_cpp_xr_set_shared_slot(K_LEFT_GRIP_PRESSED, 0.0);
        cvr_cpp_xr_set_shared_slot(K_RIGHT_TRIGGER_ANALOG, physical.right_trigger);
        cvr_cpp_xr_set_shared_slot(K_LEFT_TRIGGER_ANALOG, physical.left_trigger);
        menu_mask(st);
        m.publish_packet(st);
        return r;
    }

    st.pad.buttons |= grip_shoulders;
    let lt = float_to_byte(vr.left_trigger);
    let mut rt = float_to_byte(vr.right_trigger);
    // NOT WHILE A DEVICE CAMERA IS TAKEN OVER: V holds no weapon of her own in a camera or the q303 SniperNest, so the
    // gate read "empty hands" and swallowed the nest's zoom (LT), 2026-11-12.
    let device_takeover = cvr_cpp_device_cam_active();
    let lt_claimed_by_lighter = CyberpunkVR_LtLighterGate != 0 && !g_hasWeaponEquipped && !g_isInVehicle && !device_takeover;
    if !lt_claimed_by_lighter && lt > st.pad.left_trigger {
        st.pad.left_trigger = lt;
    }
    if tick() < CyberpunkVR_LeftTriggerBlockUntil {
        st.pad.left_trigger = 0;
    }

    // ---- SCANNER: the physical left hand at the left ear + a fresh L3 (cvr-xr counts the presses before the click's
    // D-pad and deferred-sprint conversion, so a short press between two polls is not lost).
    let cyberware_kind = cvr_cyberware_selected_kind();
    let head_aim = cvr_anim_is_head_aim_weapon_active();
    let physical_cyberware = ((1..=3).contains(&cyberware_kind) || cyberware_kind == 5) && menu_mode == 0 && !head_aim;
    let wire_owns_grip = physical_cyberware && cyberware_kind == 3 && !on_ladder;
    let scanner_vision = cvr_input_scanner_vision();
    let scanner_active = scanner_vision == 1 && menu_mode == 0;
    let firearm_trigger = g_VRBind == 4
        && gameplay_screen
        && !ui_popup_open
        && !g_isInVehicle
        && !menu_capture
        && !device_takeover
        && !in_braindance
        && !scanner_active
        && !bd_active
        && g_hasWeaponEquipped
        && (1..=4).contains(&CyberpunkVR_WeaponClass)
        && cyberware_kind != 4
        && !cvr_anim_is_head_aim_weapon_active();
    // ONE PRESS PER PHYSICAL PULL of the weapon hand's finger, its owner the weapon and the hand.
    let weapon_source = (PRIMARY_WEAPON_REVISION.load(Ordering::Relaxed) << 1) | left_handed as u64;
    let fire_held = m.weapon_trigger.poll(firearm_trigger, weapon_source, physical.finger_pull[if left_handed { 0 } else { 1 }]);
    if firearm_trigger {
        rt = if fire_held { 255 } else { 0 };
    }
    cvr_cyberware_publish_grips(wire_owns_grip && physical.left_grip > 0.5, wire_owns_grip && physical.right_grip > 0.5);
    let mut scanner_hold;
    {
        let enabled = CyberpunkVR_ScannerGesture != 0 && gameplay_screen && !menu_capture && !cvr_cpp_lobby_world_shown();
        if !enabled || CyberpunkVR_ScannerToggle == 0 {
            m.scanner_latch = false;
        } else if fresh_scanner_presses & 1 != 0 {
            m.scanner_latch = !m.scanner_latch;
        }
        let mut latch = m.scanner_latch;
        m.request.update(&mut latch, scanner_vision, tick());
        m.scanner_latch = latch;
        scanner_hold = enabled && if CyberpunkVR_ScannerToggle != 0 { m.scanner_latch } else { physical.scanner_held };
    }
    // ---- THE SCANNER ON THE LB BUTTON, AS A TOGGLE (2026-11-12, the user: the scanner should toggle). The game's LB is
    // Vision_Hold_Button, a hold; a button bound to LB (the Frame's bumper) went straight through as the raw hold. One
    // press opens, the next closes: the raw LB is taken out of the merge and asserted from the latch. On foot the
    // gesture's request check releases a press the game did not take; in a device takeover (the q303 SniperNest) the
    // Vision state is not trusted to report the device's scanner, so only a second press or leaving the takeover ends it.
    {
        let eligible = CyberpunkVR_ScannerButtonToggle != 0 && gameplay_screen && !ui_popup_open && !g_isInVehicle && menu_mode == 0
            && !cvr_cpp_lobby_world_shown();
        let lb = vr.buttons & LEFT_SHOULDER != 0;
        if !eligible || device_takeover != m.was_takeover {
            m.btn_latch = false;
        } else if lb && !m.lb_was {
            m.btn_latch = !m.btn_latch;
        }
        m.lb_was = lb;
        m.was_takeover = device_takeover;
        if !device_takeover {
            let mut latch = m.btn_latch;
            m.btn_request.update(&mut latch, scanner_vision, tick());
            m.btn_latch = latch;
        } else {
            let mut none = false;
            m.btn_request.update(&mut none, 0, 0);
        }
        if eligible {
            st.pad.buttons &= !LEFT_SHOULDER;
            scanner_hold = scanner_hold || m.btn_latch;
        }
    }

    // ---- THE SCANNER, WORKED WITH THE HAND THAT RAISED IT
    let (scan_dpad_up, scan_dpad_down, scan_apply, scan_tag, scan_tab);
    {
        let now = tick();
        let pulse_ms = if CyberpunkVR_ScannerPulseMs > 0 { CyberpunkVR_ScannerPulseMs as u64 } else { 90 };
        if scanner_active && CyberpunkVR_ScannerFaceNav != 0 {
            let x_down = vr.buttons & 0x4000 != 0;
            if x_down && !m.x_was_down {
                m.apply_until = now + pulse_ms;
            }
            m.x_was_down = x_down;
            if CyberpunkVR_ScannerTriggerTag != 0 {
                let rt_down = if m.rt_was_down { vr.right_trigger > 0.35 } else { vr.right_trigger > 0.65 };
                if rt_down && !m.rt_was_down {
                    m.tag_until = now + pulse_ms;
                }
                m.rt_was_down = rt_down;
            } else {
                m.rt_was_down = false;
            }
            if CyberpunkVR_ScannerStickTab != 0 {
                let r3_down = vr.buttons & 0x0080 != 0;
                if r3_down && !m.r3_was_down {
                    m.tab_until = now + pulse_ms;
                }
                m.r3_was_down = r3_down;
            } else {
                m.r3_was_down = false;
            }
            st.pad.buttons &= !(0x4000 | 0x8000);
        } else {
            m.x_was_down = false;
            m.r3_was_down = false;
            m.rt_was_down = false;
        }
        scan_dpad_up = now < m.up_until;
        scan_dpad_down = now < m.down_until;
        scan_apply = now < m.apply_until;
        scan_tag = now < m.tag_until;
        scan_tab = now < m.tab_until;
    }

    cvr_cpp_xr_set_shared_slot(30, if vr.right_trigger > 0.5 { 1.0 } else { 0.0 });
    cvr_cpp_xr_set_shared_slot(K_RIGHT_TRIGGER_ANALOG, physical.right_trigger);
    cvr_cpp_xr_set_shared_slot(49, if !on_ladder && !wire_owns_grip && physical.right_grip > 0.5 { 1.0 } else { 0.0 });
    m.note_right_grip(
        if menu_capture && hand_input.right_grip > 0.5 {
            6
        } else if physical.right_grip <= 0.5 {
            0
        } else if on_ladder {
            1
        } else if wire_owns_grip {
            2
        } else {
            3
        },
        hand_input.right_grip,
    );
    cvr_cpp_xr_set_shared_slot(K_LEFT_TRIGGER_ANALOG, physical.left_trigger);
    cvr_cpp_xr_set_shared_slot(K_LEFT_GRIP_PRESSED, if !on_ladder && !wire_owns_grip && physical.left_grip > 0.5 { 1.0 } else { 0.0 });
    {
        // THE MAGAZINE DROP, AND THE SECOND AFTER A POPUP CLOSES.
        let closed_ms = cvr_cpp_ui_popup_closed_ms();
        let block_ms = if CyberpunkVR_PopupMagBlockMs > 0 { CyberpunkVR_PopupMagBlockMs } else { 0 };
        let just_closed = closed_ms != 0 && tick().wrapping_sub(closed_ms) < block_ms as u64;
        // In menus B backs out; in a vehicle the raw VR B stays off the game's ExitVehicle binding. The physical reload
        // reads the logical B, published only while the controller using it has been up since a change of owner.
        let left_button = left_handed && trades_buttons(lc.left_handed_bindings);
        let eligible = gameplay_screen
            && !ui_popup_open
            && !just_closed
            && !menu_capture
            && (!mounted || g_hasWeaponEquipped)
            && !scanner_active
            && !in_braindance
            && !device_takeover
            && !bd_active
            && !on_ladder;
        let held = m.secondary.update(eligible, vr.buttons & 0x2000 != 0, if left_button { 0 } else { 1 });
        cvr_cpp_xr_set_shared_slot(K_RIGHT_SECONDARY_BTN, if held { 1.0 } else { 0.0 });
    }
    cvr_cpp_xr_set_shared_slot(K_LEFT_SECONDARY_BTN, if vr.buttons & 0x8000 != 0 { 1.0 } else { 0.0 });
    cvr_cpp_xr_set_shared_slot(K_RIGHT_STICK_CLICK, if vr.buttons & 0x0080 != 0 { 1.0 } else { 0.0 });
    cvr_cpp_xr_set_shared_slot(K_DEBUG_LOG, if cvr_cpp_verbose_log() != 0 { 1.0 } else { 0.0 });
    // Melee RT IMPULSE: a frame countdown the CET mod raises on a detected VR swing.
    let melee_impulse = cvr_cpp_xr_get_shared_slot(29);
    if melee_impulse > 0.5 {
        st.pad.right_trigger = 255;
        cvr_cpp_xr_set_shared_slot(29, melee_impulse - 1.0);
    } else if rt > st.pad.right_trigger {
        st.pad.right_trigger = rt;
    }
    // THE PORT'S OWN SAY OVER THE TRIGGER: 1 swallow, 2 press fully.
    let trg_mode = cvr_cpp_xr_get_shared_slot(K_TRIGGER_OVERRIDE);
    if trg_mode > 1.5 {
        st.pad.right_trigger = if !firearm_trigger || fire_held { 255 } else { 0 };
    } else if trg_mode > 0.5 {
        st.pad.right_trigger = 0;
    }
    if CyberpunkVR_CarryLeft != 0 {
        st.pad.right_trigger = 0;
    }
    // NONE OF THE THREE WHILE A DEVICE CAMERA IS TAKEN OVER: V's own hands hold nothing there, and the q303 SniperNest
    // fires its rifle on RT and zooms on LT (2026-11-12: RT never reached the game).
    if physical_cyberware && !device_takeover {
        st.pad.right_trigger = 0;
    }
    if cyberware_kind == 4 && gameplay_screen && !g_isInVehicle && !device_takeover {
        st.pad.right_trigger = float_to_byte(vr.left_trigger);
        st.pad.left_trigger = 0;
    } else if g_VRBind == 4 && gameplay_screen && !g_isInVehicle && !g_hasWeaponEquipped && !device_takeover {
        st.pad.right_trigger = 0; // empty-hand RT is a finger gesture
    }
    if scanner_active && CyberpunkVR_ScannerTriggerTag != 0 {
        st.pad.right_trigger = 0;
    }

    // DUAL WIELDING: the left trigger fires the left gun, each breaking where the other does.
    let dual_wield = CyberpunkVR_DualWieldInput != 0
        && tick() < CyberpunkVR_DualWieldUntil
        && gameplay_screen
        && !ui_popup_open
        && !g_isInVehicle
        && g_hasWeaponEquipped
        && !menu_capture
        && !scanner_active;
    {
        let left = m.dual_break[0].poll(dual_wield, weapon_source, physical.finger_pull[0]);
        let right = m.dual_break[1].poll(dual_wield, weapon_source, physical.finger_pull[1]);
        let hurricane = m.dual_rapid.update(dual_wield, left, right, tick());
        if (left && !m.dual_left) || (right && !m.dual_right) {
            m.dual_hold_until = tick() + CyberpunkVR_DualWieldPressDelayMs.max(0) as u64;
        }
        m.dual_left = left;
        m.dual_right = right;
        if dual_wield {
            st.pad.left_trigger = 0;
            let fire = if left || right { 255 } else { st.pad.right_trigger };
            st.pad.right_trigger = if tick() < m.dual_hold_until { 0 } else { fire };
        }
        let v = if dual_wield {
            (1 | if left { K_DUAL_WIELD_LEFT_FIRES } else { 0 }
                | if right { K_DUAL_WIELD_RIGHT_FIRES } else { 0 }
                | if hurricane { K_DUAL_WIELD_HURRICANE } else { 0 }) as f32
        } else {
            0.0
        };
        cvr_cpp_xr_set_shared_slot(K_DUAL_WIELD_ACTIVE, v);
    }

    // Left stick = locomotion.
    let analog_movement = use_analog_movement(lc.movement_speed_mode, g_isInVehicle);
    let full = full_input(lc.max_input_threshold);
    let scanner_ly = apply_stick_deadzone(vr.left_thumb_y, 0.12);
    let mut lx = if analog_movement { analog_axis(vr.left_thumb_x, lc.left_stick_deadzone, full) } else { apply_stick_deadzone(vr.left_thumb_x, 0.12) };
    let mut ly = if analog_movement { analog_axis(vr.left_thumb_y, lc.left_stick_deadzone, full) } else { scanner_ly };
    // THE HACK LIST ON THE LEFT STICK, UP AND DOWN, TO THE STOP (a key, edge-triggered with a re-arm).
    if scanner_active && CyberpunkVR_ScannerFaceNav != 0 && CyberpunkVR_ScannerStickNav != 0 && !g_isInVehicle && menu_mode == 0 {
        let mut nav_fire = CyberpunkVR_ScannerStickNavFire;
        if !(nav_fire > 0.05) || nav_fire > 1.0 {
            nav_fire = 0.90;
        }
        let mut nav_rearm = CyberpunkVR_ScannerStickNavRearm;
        if !(nav_rearm >= 0.0) || nav_rearm >= nav_fire {
            nav_rearm = nav_fire * 0.55;
        }
        let mut nav_dir = 0;
        if scanner_ly > nav_fire {
            nav_dir = 1;
        } else if scanner_ly < -nav_fire {
            nav_dir = -1;
        }
        if scanner_ly.abs() < nav_rearm {
            m.nav_armed_dir = 0;
        }
        if nav_dir != 0 && nav_dir != m.nav_armed_dir {
            m.nav_armed_dir = nav_dir;
            send_list_key(nav_dir > 0);
        }
        if nav_dir != 0 {
            ly = 0.0;
        }
    }
    let ly_detent = if ly == 0.0 {
        0.0
    } else if analog_movement {
        vr.left_thumb_y
    } else {
        ly
    };
    // ONE SPEED PER PUSH, scaled as a 2D vector; not in a vehicle.
    {
        let mag = (lx * lx + ly * ly).sqrt();
        if !analog_movement && CyberpunkVR_MoveTiers != 0 && !g_isInVehicle && mag > 1e-4 {
            let mut out_mag = 1.0f32;
            let mut walk_max = CyberpunkVR_MoveWalkMax;
            if !(walk_max > 0.0) || walk_max > 0.95 {
                walk_max = 0.0;
            }
            if walk_max > 0.0 {
                let mut margin = CyberpunkVR_MoveWalkMargin;
                if !(margin >= 0.0) || margin > 0.30 {
                    margin = 0.06;
                }
                m.walking = mag <= if m.walking { walk_max + margin } else { walk_max };
                if m.walking {
                    out_mag = CyberpunkVR_MoveWalkMag;
                    if !(out_mag > 0.05) || out_mag > 1.0 {
                        out_mag = 0.35;
                    }
                }
            }
            let k = out_mag / mag;
            lx *= k;
            ly *= k;
        } else if mag <= 1e-4 || analog_movement {
            m.walking = false;
        }
    }
    if lx.abs() > (st.pad.thumb_lx as f32 / 32767.0).abs() {
        st.pad.thumb_lx = float_to_short(lx);
    }
    if ly.abs() > (st.pad.thumb_ly as f32 / 32767.0).abs() {
        st.pad.thumb_ly = float_to_short(ly);
    }

    // WHEEL STEERING: assignment, gated on driving.
    {
        let b_r = ld(&g_wheelBlendRight);
        let b_l = ld(&g_wheelBlendLeft);
        if cvr_cpp_is_driving() && (b_r > 0.01 || b_l > 0.01) {
            let mut steer = cvr_anim_wheel_steer_input();
            if steer > 1.0 {
                steer = 1.0;
            }
            if steer < -1.0 {
                steer = -1.0;
            }
            st.pad.thumb_lx = float_to_short(cvr_anim_wheel_gamepad_steer(steer));
        }
    }
    // THE HORN HAS NO BUTTON ANY MORE: counted, not pressed.
    if cvr_cpp_is_driving() && lc.wheel_horn != 0 && g_wheelHornMask.load(Ordering::Relaxed) != 0 && diagnostics() {
        CyberpunkVR_DebugWheelHornGestures += 1;
    }

    // DRIVING WITH A GUN OUT: the trigger fires through RB and THE LEFT STICK DRIVES (2026-11-17, replacing the latched
    // throttle and its trim): forward is the throttle, back the brake, then reverse, both in proportion to the push;
    // stick X steers as always unless a hand holds the wheel (the wheel assigns X above). Assignment, not max(): the
    // trigger is the gun's now and must not leak into the throttle. Stick Y is consumed: in a vehicle its click is
    // Vehicle_Autodrive, which the full-forward gesture asserts as L3.
    let veh_gun_mode = cvr_cpp_is_driving() && g_hasWeaponEquipped && lc.vehicle_gun_trigger != 0;
    if veh_gun_mode {
        let forward = ly.clamp(0.0, 1.0);
        let back = (-ly).clamp(0.0, 1.0);
        st.pad.right_trigger = (forward * 255.0 + 0.5) as u8;
        let brake = (back * 255.0 + 0.5) as u8;
        if brake > st.pad.left_trigger {
            st.pad.left_trigger = brake;
        }
        st.pad.thumb_ly = 0;
        // RB is the ranged attack: the port's trigger override still decides (a revolver with its cylinder out).
        let fire_held = if trg_mode > 1.5 {
            true
        } else if trg_mode > 0.5 {
            false
        } else {
            vr.right_trigger > 0.5
        };
        if fire_held {
            st.pad.buttons |= RIGHT_SHOULDER;
        }
    }
    // THE RADIO ON D-PAD LEFT. Seated in a car the game puts the CrystalCoat paint selector (and the lights and siren) on
    // a held D-pad left, while the radio is RB (a tap toggles or cycles, a 0.3 s hold opens the station list) -- a
    // button no Touch controller produces in a vehicle. So in a car D-pad left IS the radio; with a gun out in the
    // driver seat RB is the shot, and the D-pad is still taken from the paint with nothing pressed for it. Passenger
    // window combat keeps the game's own buttons. With a list open (the radio's own) D-pad left is the list's volume.
    if gameplay_screen && mounted && vehicle_state != 2 && !ui_popup_open && st.pad.buttons & DPAD_LEFT != 0 {
        st.pad.buttons &= !DPAD_LEFT;
        if !veh_gun_mode {
            st.pad.buttons |= RIGHT_SHOULDER;
        }
    }

    // SPRINT: the stick held at the stop (or the click mode), driven as a held L3 with a brief release for re-entry.
    let want_sprint;
    {
        let (now_s, freq_s) = qpc_now();
        let mut dt_ms = 0.0f64;
        if m.sprint_qpc != 0 && freq_s > 0 {
            dt_ms = (now_s - m.sprint_qpc) as f64 * 1000.0 / freq_s as f64;
        }
        m.sprint_qpc = now_s;
        if dt_ms < 0.0 {
            dt_ms = 0.0;
        }
        if dt_ms > 100.0 {
            dt_ms = 100.0;
        }
        let loco = g_VRLocomotionState;
        let crouched = loco == 1 || loco == 12 || loco == 13;
        let crouch_blocks = (CyberpunkVR_SprintFromCrouch == 0 && crouched) || super::xr_calls::physical_crouch_requested();
        let full_forward = at_full_travel(ly_detent, analog_movement, full, false);
        let detent = full_forward && !g_isInVehicle && !crouch_blocks;
        if detent {
            m.detent_ms += dt_ms;
        } else {
            m.detent_ms = 0.0;
        }
        let hold_ms = if CyberpunkVR_SprintHoldMs >= 0 { CyberpunkVR_SprintHoldMs as f64 } else { 200.0 };
        let mut want = detent && m.detent_ms >= hold_ms;
        {
            const CLICK_WAIT_MS: f64 = 600.0;
            const CLICK_REST_MS: f64 = 100.0;
            let clicks = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_LeftStickClicks));
            let clicked = m.clicks_seen >= 0 && clicks != m.clicks_seen;
            m.clicks_seen = clicks;
            if lc.sprint_mode == 1 {
                let pushed = if analog_movement { full_forward } else { lx * lx + ly * ly > 1e-4 };
                if clicked && !m.click_sprint {
                    m.click_sprint = true;
                    m.pushed_since_click = false;
                    m.rest_ms = 0.0;
                }
                if pushed {
                    m.pushed_since_click = true;
                    m.rest_ms = 0.0;
                } else {
                    m.rest_ms += dt_ms;
                }
                if g_isInVehicle || crouch_blocks || m.rest_ms > if m.pushed_since_click { CLICK_REST_MS } else { CLICK_WAIT_MS } {
                    m.click_sprint = false;
                }
                want = m.click_sprint && pushed;
            } else {
                m.click_sprint = false;
            }
        }
        let sprint_flag = g_VRSprintActive;
        let have_feedback = sprint_flag >= 0;
        let sprinting = have_feedback && sprint_flag != 0;
        if swim.water {
            want_sprint = cvr_cpp_swimming_sprint_input(want);
            m.phase_ms = 0.0;
            m.ask_ms = 0.0;
            m.gave_up = false;
        } else if !want {
            want_sprint = false;
            m.phase_ms = 0.0;
            m.ask_ms = 0.0;
            m.gave_up = false;
        } else if !have_feedback || sprinting || m.gave_up {
            want_sprint = true;
            m.phase_ms = 0.0;
        } else {
            m.ask_ms += dt_ms;
            let give_up = if CyberpunkVR_SprintGiveUpMs > 0 { CyberpunkVR_SprintGiveUpMs as f64 } else { 1500.0 };
            if m.ask_ms > give_up {
                m.gave_up = true;
                want_sprint = true;
            } else {
                let hold = if CyberpunkVR_SprintPulseMs > 0 { CyberpunkVR_SprintPulseMs as f64 } else { 240.0 };
                let gap = if CyberpunkVR_SprintGapMs > 0 { CyberpunkVR_SprintGapMs as f64 } else { 60.0 };
                m.phase_ms += dt_ms;
                let period = hold + gap;
                while m.phase_ms >= period {
                    m.phase_ms -= period;
                }
                want_sprint = m.phase_ms < hold;
            }
        }
    }
    {
        let sf = g_VRSprintActive;
        SPRINT_INPUT_ACTIVE.store(if sf >= 0 { sf != 0 } else { want_sprint }, Ordering::Relaxed);
    }

    // Right stick = camera turn / pitch.
    let legacy_rx = apply_stick_deadzone(vr.right_thumb_x, 0.18);
    let legacy_ry = apply_stick_deadzone(vr.right_thumb_y, 0.18);
    let mut rx = if analog_movement { analog_axis(vr.right_thumb_x, lc.right_stick_deadzone, full) } else { legacy_rx };
    let mut ry = if analog_movement { analog_axis(vr.right_thumb_y, lc.right_stick_deadzone, full) } else { legacy_ry };
    // The snap turn reads the raw travel, not the deadzoned camera axis.
    let mut snap_stick_x = vr.right_thumb_x;
    // A DEVICE SCREEN IS UP: the right stick's Y is the game's list navigation.
    let mut device_screen = false;
    let sh_dev = cvr_cpp_shot_shared();
    if !sh_dev.is_null() && core::ptr::read_volatile((sh_dev as *const u32).add(K_DEVICE_SCREEN_OPEN)) != 0 {
        device_screen = true;
    }
    // THE SCANNER'S ZOOM TAKES THE RIGHT STICK, before the crouch, the dash, the pitch and the snap turn.
    {
        let now = tick();
        let lt_down = if m.lt_zoom_was { vr.left_trigger > 0.35 } else { vr.left_trigger > 0.60 };
        m.lt_zoom_was = lt_down;
        let armed = scanner_active && CyberpunkVR_ScannerZoom != 0 && lt_down && !g_isInVehicle && menu_mode == 0;
        if armed {
            let mut th = CyberpunkVR_ScannerZoomStick;
            if !(th > 0.05) || th > 1.0 {
                th = 0.50;
            }
            let rep = if CyberpunkVR_ScannerZoomRepeatMs > 0 { CyberpunkVR_ScannerZoomRepeatMs } else { 200 };
            if legacy_ry > th || legacy_ry < -th {
                if now >= m.next_step_ms {
                    send_zoom_key(legacy_ry > 0.0);
                    m.next_step_ms = now + rep as u64;
                }
            } else {
                m.next_step_ms = 0;
            }
            rx = 0.0;
            ry = 0.0;
            snap_stick_x = 0.0; // the scanner's zoom owns the raw snap travel too
        } else {
            m.next_step_ms = 0;
        }
    }
    let device_cam = cvr_cpp_device_cam_active();
    let stick_y = if analog_movement { vr.right_thumb_y } else { ry };
    let full_down = at_full_travel(stick_y, analog_movement, full, true);
    let full_up = at_full_travel(stick_y, analog_movement, full, false);
    let crouch_allowed = !in_water && !g_isInVehicle && !device_screen && !scanner_active && !device_cam;
    let want_crouch = full_down && crouch_allowed;
    if want_crouch {
        ry = 0.0;
    }
    // R3 is a tap per push, not a level: ToggleCrouch only reacts to a tap, so a held stick stood still.
    let crouch_pulse = m.crouch_pulse.update(
        full_down,
        if analog_movement { -vr.right_thumb_y } else { -legacy_ry },
        tick(),
        if CyberpunkVR_CrouchPulseMs > 0 { CyberpunkVR_CrouchPulseMs as u32 } else { 100 },
        crouch_allowed,
    );
    // DASH on the right stick fully up, edge-triggered with a re-arm.
    let dash_pulse;
    {
        let allow_dash = CyberpunkVR_DashStickUp != 0 && !in_water && !scanner_active && !device_screen && !device_cam && !g_isInVehicle && menu_mode == 0;
        if allow_dash {
            let now = tick();
            if full_up {
                if m.dash_armed_dir == 0 {
                    m.dash_armed_dir = 1;
                    let ms = if CyberpunkVR_DashPulseMs > 0 { CyberpunkVR_DashPulseMs } else { 100 };
                    m.dash_until = now + ms as u64;
                }
            } else if legacy_ry < 0.50 {
                m.dash_armed_dir = 0;
            }
            dash_pulse = now < m.dash_until;
        } else {
            m.dash_armed_dir = 0;
            m.dash_until = 0;
            dash_pulse = false;
        }
    }
    if full_up && !device_screen {
        ry = 0.0;
    }
    if lc.disable_mouse_y != 0 && !device_screen {
        ry = 0.0;
    }
    // Snap turn: not in a device camera, not in a scene the port's own yaw turns, not where the native heading owns it.
    let snap_enabled = lc.snap_turn != 0
        && !device_cam
        && !cvr_cpp_scene_stick_yaw_armed()
        && !native_owns_turn(g_isInVehicle, cvr_cpp_pose_vehicle_state(), cvr_cpp_ladder_active());
    let turn = route_controller_turn(
        snap_stick_x,
        rx,
        snap_enabled,
        &mut m.snap_armed_dir,
        lc.snap_turn_angle_deg,
        CyberpunkVR_SnapTurnStickFire,
        CyberpunkVR_SnapTurnStickRearm,
    );
    if turn.clear_pending {
        cvr_cpp_take_pending_snap_yaw();
    } else if turn.snap_degrees != 0.0 {
        cvr_cpp_set_pending_snap_yaw(turn.snap_degrees.to_bits() as i32);
    }
    rx = turn.axis;
    if rx.abs() > (st.pad.thumb_rx as f32 / 32767.0).abs() {
        st.pad.thumb_rx = float_to_short(rx);
    }
    cvr_cpp_set_look_stick_x(st.pad.thumb_rx as f32 / 32767.0);
    if ry.abs() > (st.pad.thumb_ry as f32 / 32767.0).abs() {
        st.pad.thumb_ry = float_to_short(ry);
    }

    // The stick gestures and the scanner's buttons.
    let mut synth: u16 = 0;
    if want_sprint {
        synth |= LEFT_THUMB;
    }
    if crouch_pulse {
        synth |= RIGHT_THUMB;
    }
    if dash_pulse {
        synth |= 0x2000;
    }
    if scanner_hold {
        synth |= 0x0100;
    }
    if scan_dpad_up {
        synth |= 0x0001;
    }
    if scan_dpad_down {
        synth |= 0x0002;
    }
    if scan_apply {
        synth |= 0x4000;
    }
    if scan_tag {
        synth |= 0x0080;
    }
    if scan_tab {
        synth |= 0x0200;
    }
    // A CONFIRMS A DIALOGUE LINE WHILE MOUNTED, on the edge.
    if CyberpunkVR_VehicleDialogConfirmOnA != 0 {
        let a_down = vehicle_buttons.confirm_on_a && gameplay_screen && vr.buttons & 0x1000 != 0;
        if a_down && !m.a_was {
            synth |= 0x4000;
        }
        m.a_was = a_down;
    }
    // X GETS YOU OUT OF THE CAR, HELD.
    {
        let need = if CyberpunkVR_VehicleExitHoldMs > 0 { CyberpunkVR_VehicleExitHoldMs as u64 } else { 400 };
        if m.exit_hold.update(vehicle_buttons.exit_on_x && gameplay_screen, vr.buttons & 0x4000 != 0, tick(), need) {
            synth |= 0x2000;
        }
    }
    // B CLOSES A POPUP IN THE CAR, THROUGH THE DRIVING MECHANIC. The radio list (D-pad left) closes on `cancel`, which
    // shares the pad's B with ExitVehicle_Button -- B stays held out of the car; Start is no way round it (the list
    // closes on the press and the pause menu opens on the release, measured). Each B press that starts with a popup up
    // is counted, and the mechanic closes the list on the change.
    {
        let b_down = vr.buttons & 0x2000 != 0;
        if b_down && !m.b_prev && mounted && gameplay_screen && ui_popup_open && vehicle_buttons.owned & 0x2000 != 0 {
            m.popup_backs = if m.popup_backs >= 1.0e6 { 1.0 } else { m.popup_backs + 1.0 };
            cvr_cpp_xr_set_shared_slot(K_VEHICLE_POPUP_BACK, m.popup_backs);
        }
        m.b_prev = b_down;
    }
    st.pad.buttons |= synth;
    if diagnostics() {
        let t = tick();
        if scanner_hold != m.last_hold || (scanner_hold && t.wrapping_sub(m.scan_output) >= 250) {
            m.scan_output = t;
            m.last_hold = scanner_hold;
            log(&format!(
                "[scanner-output] requested={} vision={} buttons={:04X} grip={:.3} foreground={}\n",
                scanner_hold as i32, scanner_vision, st.pad.buttons, vr.left_grip, cvr_cpp_game_window_in_front() as i32
            ));
        }
    }
    // Y WHILE TWO GUNS ARE IN HAND: a tap drops the left gun's magazine, a hold is the game's Y again.
    {
        let y_down = vr.buttons & 0x8000 != 0;
        let now = tick();
        if dual_wield && y_down {
            if m.y_down_at == 0 {
                m.y_down_at = now;
            }
            let hold_ms = CyberpunkVR_DualWieldYHoldMs.max(0);
            if now.wrapping_sub(m.y_down_at) >= hold_ms as u64 {
                m.y_pass = true;
            }
        } else {
            if dual_wield && m.y_down_at != 0 && !m.y_pass {
                m.y_taps += 1.0;
                cvr_cpp_xr_set_shared_slot(K_DUAL_WIELD_Y_TAP, m.y_taps);
            }
            m.y_down_at = 0;
            m.y_pass = false;
        }
        if dual_wield && !m.y_pass {
            st.pad.buttons &= !0x8000;
        }
    }
    // THE PORT'S WEAPON WHEEL TAKES THE HOLD OF Y.
    {
        let now = tick();
        let armed = cvr_cpp_vrmenu_wheel_armed(now) && gameplay_screen && !ui_popup_open && !menu_capture && !scanner_active && !bd_active;
        let wheel = m.wheel_hold.step(st.pad.buttons & 0x8000 != 0, armed, now);
        st.pad.buttons = (st.pad.buttons & !0x8000) | if wheel.game_y { 0x8000 } else { 0 };
        cvr_cpp_vrmenu_publish_wheel_hold(wheel.pending, wheel.held);
    }
    menu_mask(st);
    // The weapon finger's fresh pull for the script side's semi-automatic fire: only a pull the game saw as a full
    // press of a firearm's trigger, published for 100 ms from its start, never one a block consumed.
    let pull = physical.finger_pull[if left_handed { 0 } else { 1 }];
    let token = if fire_held { (pull << 1) | left_handed as u64 } else { 0 };
    let now = tick();
    let finger_allowed = firearm_trigger && !dual_wield && fire_held && st.pad.right_trigger == 255;
    FINGER_ACTION.store(m.finger_action.sample(token, finger_allowed, now), Ordering::Release);
    WEAPON_TRIGGER_STAMP.store(now, Ordering::Relaxed);
    WEAPON_TRIGGER_ENABLED.store(firearm_trigger, Ordering::Relaxed);
    m.publish_packet(st);
    r
}

/// The sprint as the snap-event machinery reads it (the game's own state when there is one, else the intent).
#[no_mangle]
pub extern "C" fn cvr_xinput_sprint_input_active() -> bool {
    SPRINT_INPUT_ACTIVE.load(Ordering::Relaxed)
}

// ---- the hook: every loaded module's XInputGetState import slot

type GetStateFn = unsafe extern "system" fn(u32, *mut XState) -> u32;
static mut REAL_GET_STATE: Option<GetStateFn> = None;

unsafe extern "system" fn hooked_get_state(user: u32, state: *mut XState) -> u32 {
    let r = match REAL_GET_STATE {
        Some(real) => real(user, state),
        None => ERROR_DEVICE_NOT_CONNECTED,
    };
    cvr_xinput_merge(user, state, r)
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn K32EnumProcessModules(process: *mut c_void, modules: *mut *mut c_void, bytes: u32, needed: *mut u32) -> i32;
}

/// The module's XInputGetState import slots (by name, any xinput*.dll) pointed at the hook; the first one found
/// keeps the function it chains to. The number patched.
unsafe fn patch_iat(module: *mut c_void, hook: usize, original: &mut usize) -> i32 {
    let base = module as usize;
    if base == 0 || *(base as *const u16) != 0x5A4D {
        return 0;
    }
    let nt = base + *((base + 0x3C) as *const u32) as usize;
    if *(nt as *const u32) != 0x0000_4550 {
        return 0;
    }
    // IMAGE_NT_HEADERS64: OptionalHeader at +24, DataDirectory at +112 of it, the import entry is [1].
    let dir = nt + 24 + 112 + 8;
    let (va, size) = (*(dir as *const u32), *((dir + 4) as *const u32));
    if va == 0 || size == 0 {
        return 0;
    }
    let mut patched = 0;
    let mut imp = base + va as usize;
    loop {
        // IMAGE_IMPORT_DESCRIPTOR: OriginalFirstThunk, TimeDateStamp, ForwarderChain, Name, FirstThunk
        let name = *((imp + 12) as *const u32);
        if name == 0 {
            break;
        }
        let dll = core::ffi::CStr::from_ptr((base + name as usize) as *const core::ffi::c_char).to_bytes();
        let original_first = *(imp as *const u32);
        let first = *((imp + 16) as *const u32);
        if dll.len() >= 6 && dll[..6].eq_ignore_ascii_case(b"xinput") && original_first != 0 && first != 0 {
            let mut name_thunk = base + original_first as usize;
            let mut iat_thunk = base + first as usize;
            loop {
                let data = *(name_thunk as *const u64);
                if data == 0 {
                    break;
                }
                if data & 0x8000_0000_0000_0000 == 0 {
                    let ibn = core::ffi::CStr::from_ptr((base + data as usize + 2) as *const core::ffi::c_char);
                    if ibn.to_bytes() == b"XInputGetState" {
                        let slot = iat_thunk as *mut usize;
                        if *slot != hook {
                            let mut old = 0u32;
                            if VirtualProtect(slot as *mut c_void, 8, 0x04, &mut old) != 0 {
                                if *original == 0 {
                                    *original = *slot; // chain whatever was there
                                }
                                *slot = hook;
                                VirtualProtect(slot as *mut c_void, 8, old, &mut old);
                                patched += 1;
                            }
                        }
                    }
                }
                name_thunk += 8;
                iat_thunk += 8;
            }
        }
        imp += 20;
    }
    patched
}

/// Installed from src/Hooks/XInput.cpp's registration (Boot, 100, when xr_xinput_install is on).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_xinput_install() -> bool {
    let mut xi = GetModuleHandleA(c"XInput1_4.dll".as_ptr() as *const u8);
    if xi.is_null() {
        xi = LoadLibraryA(c"XInput1_4.dll".as_ptr() as *const u8);
    }
    if xi.is_null() {
        xi = LoadLibraryA(c"XInput1_3.dll".as_ptr() as *const u8);
    }
    if xi.is_null() {
        xi = LoadLibraryA(c"xinput9_1_0.dll".as_ptr() as *const u8);
    }
    let hook = hooked_get_state as *const () as usize;
    let mut original = 0usize;
    let mut patched = 0;
    let exe = GetModuleHandleW(core::ptr::null());
    if !exe.is_null() {
        patched += patch_iat(exe, hook, &mut original);
    }
    let mut modules = [core::ptr::null_mut::<c_void>(); 512];
    let mut needed = 0u32;
    if K32EnumProcessModules(GetCurrentProcess(), modules.as_mut_ptr(), (modules.len() * 8) as u32, &mut needed) != 0 {
        let n = ((needed / 8) as usize).min(512);
        for &module in &modules[..n] {
            patched += patch_iat(module, hook, &mut original);
        }
    }
    if patched > 0 && original != 0 {
        REAL_GET_STATE = Some(core::mem::transmute::<usize, GetStateFn>(original));
        log(&format!("XInput: IAT hook installed ({} slot(s) patched, real={:016X})\n", patched, original));
        return true;
    }
    // No import slot (resolved dynamically or by ordinal): keep a real pointer, fail soft.
    if !xi.is_null() && REAL_GET_STATE.is_none() {
        let f = GetProcAddress(xi, c"XInputGetState".as_ptr() as *const u8);
        if !f.is_null() {
            REAL_GET_STATE = Some(core::mem::transmute::<*mut c_void, GetStateFn>(f));
        }
    }
    log(&format!("XInput: no XInputGetState import slot found (patched={}) -- controller input unavailable\n", patched));
    false
}
