//! The lobby's natives (formerly src/Natives/EngineStereoMenu.cpp), registered by cvr-ffi's natives table:
//!
//!   VREngineStereoMenuShown() / VREngineStereoMenuClosed()  the main menu's controller was initialised / uninitialised
//!       (mods/redscript/CyberpunkVRPort_EngineStereo): engine stereo starts at the first and stays on; both say
//!       whether the main menu is up (the engine-stereo SDK and the lobby). Outside engine stereo they do nothing.
//!   VRLobbyActive() -> Bool        the main menu is shown in its world, so the script hides the menu's backdrop
//!   VRLobbySuspend(on)             the lobby steps aside for one of the game's own main menu screens, and takes it back
//!   VRLobbyCurtain(closure)        how far the eyes are closed while the lobby comes up, 0 open .. 1 shut
//!   VRLobbyAmbient() -> Bool       the lobby's ambience on (xr_lobby_ambient)
//!   VRSetLobbyAmbient(on)          switched from the lobby menu's button and saved as the settings panel saves it
use core::ffi::c_void;
use red4ext_abi::stack::StackFrame;

use crate::{engine_stereo, engine_stereo_sdk, menu_lobby};

extern "C" {
    /// The settings panel's save of xr_lobby_ambient (src/Natives/EngineStereoMenu.cpp: LiveControlsUi state).
    fn cvr_cpp_set_lobby_ambient(on: bool);
}

/// The frame's parameters read, then the frame finished (`++frame->code`).
unsafe fn finish(frame: *mut StackFrame) {
    // SAFETY: the VM's frame.
    if let Some(f) = unsafe { frame.as_mut() } {
        f.finish_params();
    }
}
unsafe fn param<T: Default>(frame: *mut StackFrame) -> T {
    let mut v = T::default();
    // SAFETY: the VM's frame, a parameter of type T.
    if let Some(f) = unsafe { frame.as_mut() } {
        unsafe { f.get_parameter(&mut v) };
    }
    v
}
unsafe fn put<T>(out: *mut c_void, v: T) {
    if !out.is_null() {
        // SAFETY: the VM's return slot of type T.
        unsafe { *(out as *mut T) = v };
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREngineStereoMenuShown(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame.
    unsafe { finish(frame) };
    if engine_stereo::enabled() {
        // SAFETY: the SDK's main-menu note.
        unsafe { engine_stereo_sdk::cvr_engine_stereo_sdk_note_main_menu() };
    }
    menu_lobby::note_main_menu();
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREngineStereoMenuClosed(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame; the SDK's note.
    unsafe {
        finish(frame);
        menu_lobby::note_main_menu_closed();
        engine_stereo_sdk::cvr_engine_stereo_sdk_note_main_menu_closed();
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyActive(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame and its Bool return slot.
    unsafe {
        finish(frame);
        put(out, menu_lobby::active());
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbySuspend(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame, a Bool parameter.
    let on: bool = unsafe { param(frame) };
    unsafe { finish(frame) };
    menu_lobby::suspend(on);
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyCurtain(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame, a Float parameter.
    let closure: f32 = unsafe { param(frame) };
    unsafe { finish(frame) };
    menu_lobby::set_curtain(closure);
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyAmbient(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame and its Bool return slot.
    unsafe {
        finish(frame);
        put(out, menu_lobby::ambient());
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSetLobbyAmbient(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    // SAFETY: the VM's frame, a Bool parameter; the C++ half's settings save.
    unsafe {
        let mut on = true;
        if let Some(f) = frame.as_mut() {
            f.get_parameter(&mut on);
        }
        finish(frame);
        cvr_cpp_set_lobby_ambient(on);
    }
}
