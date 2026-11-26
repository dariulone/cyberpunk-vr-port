//! The first-launch setup's natives (formerly src/Natives/BodySetup.cpp) and the VR overlay's (formerly
//! src/Natives/VrOverlay.cpp).
//!
//! The setup walks a new player through the hand that holds the weapon, the T-pose and the player's height
//! (mods/cet/CyberpunkVRPort_Lobby modules/setup.lua). The calibration is cvr-xr's (`calibration`): the T-pose measures
//! which way is right, the eyes' height over the floor and the arms' span; the height gives the arms and the shoulders
//! their anatomical lengths. The switches the setup sets (xr_left_handed, xr_left_handed_bindings, xr_body_hands_only)
//! are saved as the settings panel saves them, through BodySetup.cpp's adapter over the C++ UI state. The overlay's
//! natives bridge the VR menu of the C++ overlay (src/Overlay/VrOverlay.cpp).
use crate::native_args::args;
use core::ffi::c_void;
use cvr_settings::live_controls_ui_state::LiveControlsUiState;
use cvr_xr::calibration::{self, body};
use red4ext_abi::stack::StackFrame;
use std::sync::atomic::Ordering::Relaxed;

extern "C" {
    /// The live controls' UI state: read (src/Core/LiveControlsPoll.cpp), and set and saved as the settings panel saves
    /// it (rust/crates/cvr-render/src/live_controls_ui.rs).
    fn GetLiveControlsUiState(out: *mut LiveControlsUiState);
    fn SetLiveControlsUiState(state: *const LiveControlsUiState, persist_to_file: i32);
}
const LEFT_HANDED: i32 = 0;
const LEFT_HANDED_BINDINGS: i32 = 1;
const BODY_HANDS_ONLY: i32 = 2;
/// One of the setup's switches set in the live controls' UI state and saved as the settings panel saves it (formerly
/// src/Natives/BodySetup.cpp's cvr_cpp_setup_switch_set): 0 xr_left_handed, 1 xr_left_handed_bindings, 2
/// xr_body_hands_only.
unsafe fn setup_switch_set(which: i32, value: i32) {
    // SAFETY: plain data, zeroed as the .cpp's `LiveControlsUiState state{}`.
    let mut state: LiveControlsUiState = unsafe { core::mem::zeroed() };
    // SAFETY: a local state.
    unsafe { GetLiveControlsUiState(&mut state) };
    match which {
        LEFT_HANDED => state.xrLeftHanded = value,
        LEFT_HANDED_BINDINGS => state.xrLeftHandedBindings = value,
        BODY_HANDS_ONLY => state.xrBodyHandsOnly = value,
        _ => return,
    }
    // SAFETY: the state read above, changed in one switch.
    unsafe { SetLiveControlsUiState(&state, 1) };
}
/// g_liveControls' switch: 1 xrLeftHandedBindings, 2 xrBodyHandsOnly (formerly cvr_cpp_setup_switch).
fn setup_switch(which: i32) -> i32 {
    use cvr_settings::lc;
    match which {
        LEFT_HANDED_BINDINGS => lc!(xrLeftHandedBindings),
        BODY_HANDS_ONLY => lc!(xrBodyHandsOnly),
        _ => 0,
    }
}

fn shared() -> &'static calibration::CalibrationShared {
    // SAFETY: the calibration's static state.
    unsafe { &*calibration::cvr_xr_calibration() }
}
/// # Safety
/// `out` is null or the native's result slot of type T.
unsafe fn put<T>(out: *mut c_void, value: T) {
    if !out.is_null() {
        unsafe { *(out as *mut T) = value };
    }
}

/// VRFirstSetupNeeded() -> Bool: the setup has never been through on this install.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRFirstSetupNeeded(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, shared().first_setup_done.load(Relaxed) == 0) };
}
/// VRSetLeftHanded(left): the hand that holds the weapon -- xr_left_handed, saved as the settings panel saves it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSetLeftHanded(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let left = a.get::<bool>();
    a.finish();
    unsafe { setup_switch_set(LEFT_HANDED, left as i32) };
    unsafe { put(out, true) };
}
/// VRSetLeftHandedBindings(mode): how far the controllers trade left-handed -- xr_left_handed_bindings (0 the
/// right-handed buttons and sticks, 1 the left stick walks with the buttons mirrored, 2 everything mirrored).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSetLeftHandedBindings(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let a = unsafe { args(frame) };
    // The parameter's default when the VM gives none: 2, as the C++ initialised it.
    let mode = {
        let mut v = 2i32;
        unsafe { a.0.get_parameter(&mut v) };
        v
    };
    a.finish();
    unsafe { setup_switch_set(LEFT_HANDED_BINDINGS, mode.clamp(0, 2)) };
    unsafe { put(out, true) };
}
/// VRLeftHandedBindings() -> Int32: xr_left_handed_bindings.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLeftHandedBindings(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, setup_switch(LEFT_HANDED_BINDINGS)) };
}
/// VRSetBodyHandsOnly(on): the body drawn whole or as its hands alone -- xr_body_hands_only.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSetBodyHandsOnly(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let on = a.get::<bool>();
    a.finish();
    unsafe { setup_switch_set(BODY_HANDS_ONLY, on as i32) };
    unsafe { put(out, true) };
}
/// VRBodyHandsOnly() -> Bool: the body is drawn as its hands alone.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRBodyHandsOnly(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, setup_switch(BODY_HANDS_ONLY) != 0) };
}
/// VRCalibrationStart(seconds): the T-pose sample begins (arms straight out to the sides, standing straight).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRCalibrationStart(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let a = unsafe { args(frame) };
    let mut seconds = {
        let mut v = 4.0f32;
        unsafe { a.0.get_parameter(&mut v) };
        v
    };
    a.finish();
    if !seconds.is_finite() {
        seconds = 4.0;
    }
    calibration::cvr_xr_start_auto_calibration(seconds.clamp(1.0, 10.0));
    unsafe { put(out, true) };
}
/// VRCalibrationState() -> Int32: 0 idle, 1 sampling, 2 done, 3 failed (no T-pose seen).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRCalibrationState(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, shared().calib_state.load(Relaxed)) };
}
/// VRCalibrationProgress() -> Float: 0 .. 1 through the sample.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRCalibrationProgress(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, shared().calib_progress.load()) };
}
/// The height to offer: the one given before, else the one whose eyes are as high as the last T-pose measured them
/// (to the centimetre), else 1.75 m.
pub fn suggested_height(given: f32, eyes: f32) -> f32 {
    let height = if given > 0.0 {
        given
    } else if eyes > 0.0 {
        // cvr::body::HeightFromEyes, rounded to the centimetre as std::round does (half away from zero).
        (eyes / body::EYE_HEIGHT * 100.0).round() / 100.0
    } else {
        1.75
    };
    // std::clamp: a NaN passes through.
    if height < body::MIN_HEIGHT {
        body::MIN_HEIGHT
    } else if body::MAX_HEIGHT < height {
        body::MAX_HEIGHT
    } else {
        height
    }
}
/// VRSuggestedHeight() -> Float.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSuggestedHeight(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    let s = shared();
    unsafe {
        put(
            out,
            suggested_height(s.user_height.load(), s.measured_eye_height.load()),
        )
    };
}
/// VRSetPlayerHeight(meters): the body's proportions from it, saved; the setup is through.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSetPlayerHeight(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let meters = a.get::<f32>();
    a.finish();
    let ok = meters.is_finite() && meters > 0.0;
    if ok {
        calibration::cvr_xr_apply_user_height(meters);
    }
    unsafe { put(out, ok) };
}
/// VRPlayerHeight() -> Float: the height given, 0 if none.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRPlayerHeight(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, shared().user_height.load()) };
}

/// VROverlayCommand() -> Int32: the VR menu's next command (1 open the panel, 2 close it, 100+ a game action).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VROverlayCommand(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    unsafe { put(out, cvr_body::vr_overlay::cvr_vrui_poll_command()) };
}
/// VROverlayMenu(ready, inGame, labels) -> Bool: the game's state and the menu's labels handed to the VR menu; whether
/// it is visible.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VROverlayMenu(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let ready = a.get::<bool>();
    let in_game = a.get::<bool>();
    let labels = a.text();
    a.finish();
    // CString::c_str(): the text up to its first NUL.
    let labels =
        std::ffi::CString::new(labels.split('\0').next().unwrap_or_default()).unwrap_or_default();
    unsafe { cvr_body::vr_overlay::cvr_vrui_bridge_update(ready, in_game, labels.as_ptr()) };
    unsafe { put(out, cvr_body::vr_overlay::cvr_vrui_visible()) };
}
/// VROverlayToggle(): the VR menu shown or hidden.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VROverlayToggle(
    _context: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    cvr_body::vr_overlay::cvr_vrui_toggle();
}
