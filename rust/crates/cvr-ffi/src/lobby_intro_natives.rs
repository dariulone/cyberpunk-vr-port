//! The lobby intro's natives (formerly src/Lobby/Intro.cpp), registered by natives.rs:
//!
//!   VRLobbyIntro(component, distance, height, opacity, blackPoint) -> Bool   the intro's picture on a WorldWidgetComponent
//!   VRIntroTakeBootNotice(fade) -> Bool    the intro takes the boot's notice over, fading it out over `fade` seconds
//!   VRIntroHum(on, volume)                 the lobby's hum on or off
use core::ffi::c_void;

use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::object_class;
use red4ext_abi::stack::StackFrame;

use crate::native_args::args;

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRIntroTakeBootNotice(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let fade = a.get::<f32>();
    a.finish();
    let taken = cvr_render::intro_boot::cvr_intro_boot_take_notice(fade);
    if !out.is_null() {
        // SAFETY: the VM's Bool return slot.
        unsafe { *(out as *mut bool) = taken };
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRIntroHum(_context: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let on = a.get::<bool>();
    let volume = a.get::<f32>();
    a.finish();
    cvr_render::lobby_tables::hum(on, volume);
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyIntro(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let distance = a.get::<f32>();
    let height = a.get::<f32>();
    let opacity = a.get::<f32>();
    let black_point = a.get::<f32>();
    a.finish();
    // component->GetType()->IsA(WorldWidgetComponent)
    let widget = if !component.instance.is_null()
        // SAFETY: the live component the argument's handle holds.
        && unsafe { object_class(component.instance) }.is_some_and(|c| c.is_a(CName::hashed("WorldWidgetComponent")))
    {
        component.instance
    } else {
        core::ptr::null_mut()
    };
    let shown = cvr_render::lobby_intro::cvr_intro_set(widget, distance, height, opacity, black_point);
    if !out.is_null() {
        // SAFETY: the VM's Bool return slot.
        unsafe { *(out as *mut bool) = shown };
    }
}
