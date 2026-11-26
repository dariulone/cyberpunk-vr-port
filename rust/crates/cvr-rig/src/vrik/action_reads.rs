//! The state machines' input reads (formerly src/Natives/ActionReads.cpp): gamestateMachineGameScriptInterface's
//! GetActionValue and IsActionJustPressed, with the player's machines given the gestures -- the ladder's climbing axis
//! on a resting MoveY, the swimming strokes on MoveY/ToggleSprint/Jump/Dive. They were Lua overrides (the Ladder and
//! Swimming mods): ~10 calls a tick, made on the world tick's workers, each through the Lua host's whole override path
//! and its state lock. Here the function objects are swapped with native ones the way the host's overrides swap them:
//! the original moves into the object made here and is called through it, so a mod's own Override of the same function
//! still chains over this one. The Lua host (src/Lua/cet/scripting/Scripting.cpp) installs them (CvrInstallActionReads).
use super::env::log;
use super::ladder_input::cvr_ladder_native_action;
use super::swimming_input::{cvr_swimming_is_player, cvr_swimming_script_action};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use red4ext_abi::cname::CName;
use red4ext_abi::function::{function_flags, BaseFunction, ClassFunction, NativeFn};
use red4ext_abi::rtti::{Class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use std::ffi::CString;

/// The originals, after the swap.
static ACTION_VALUE: AtomicPtr<BaseFunction> = AtomicPtr::new(core::ptr::null_mut());
static JUST_PRESSED: AtomicPtr<BaseFunction> = AtomicPtr::new(core::ptr::null_mut());
/// StateGameScriptInterface.owner, a WeakHandle<GameObject>.
static OWNER_OFFSET: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

unsafe fn owner(context: *mut c_void) -> usize {
    let offset = OWNER_OFFSET.load(Ordering::Acquire);
    if context.is_null() || offset == 0 {
        return 0;
    }
    *((context as *const u8).add(offset as usize) as *const usize)
}
/// The original with this one argument, without the argument's bytecode running twice.
unsafe fn call_original(function: *mut BaseFunction, context: *mut c_void, name: &mut CName, out: *mut c_void) -> bool {
    let (Some(f), Some(rtti)) = (function.as_ref(), RttiSystem::get()) else {
        return false;
    };
    let param = f
        .params
        .as_slice()
        .first()
        .and_then(|p| p.as_ref())
        .map_or(core::ptr::null(), |p| p.ty as *const _);
    let returns = f.return_type.as_ref().map_or(core::ptr::null(), |p| p.ty as *const _);
    let mut args = [StackType {
        ty: param,
        value: name as *mut CName as *mut c_void,
    }];
    let mut result = StackType { ty: returns, value: out };
    let Some(mut stack) = CStack::new(rtti, context, &mut args, &mut result) else {
        return false;
    };
    let Some(address) = f.execute_address() else { return false };
    type Execute = unsafe extern "C" fn(*mut BaseFunction, *mut CStack, *mut c_void) -> u8;
    core::mem::transmute::<usize, Execute>(address)(function, &mut stack, core::ptr::null_mut()) != 0
}

unsafe extern "C" fn action_value(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut name = CName::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut name);
        frame.finish_params();
    }
    let mut value = 0.0f32;
    call_original(ACTION_VALUE.load(Ordering::Acquire), context, &mut name, &mut value as *mut f32 as *mut c_void);
    let owner = owner(context);
    // On a ladder the climbing axis stands in for a stick at rest (the stick still wins once pushed).
    if name == CName::hashed("MoveY") && value.abs() <= 0.15 && cvr_swimming_is_player(owner) {
        let axis = cvr_ladder_native_action();
        if axis != 0.0 {
            value = axis;
        }
    }
    let added = cvr_swimming_script_action(owner, name.0, false);
    if !out.is_null() {
        *(out as *mut f32) = if added > 0.0 && value < added { added } else { value };
    }
}
unsafe extern "C" fn just_pressed(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut name = CName::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut name);
        frame.finish_params();
    }
    let mut pressed = 0u8;
    call_original(JUST_PRESSED.load(Ordering::Acquire), context, &mut name, &mut pressed as *mut u8 as *mut c_void);
    if !out.is_null() {
        *(out as *mut bool) = pressed != 0 || cvr_swimming_script_action(owner(context), name.0, true) > 0.0;
    }
}

/// The class's native member function `name` swapped with one calling `handler`; the returned object holds the original.
unsafe fn swap(class: &Class, name: &str, handler: NativeFn) -> *mut BaseFunction {
    let Some(real) = class.function(CName::hashed(name)) else {
        return core::ptr::null_mut();
    };
    let real = real as *const BaseFunction as *mut BaseFunction;
    let r = &*real;
    if r.flags & function_flags::IS_NATIVE == 0 || r.flags & function_flags::IS_STATIC != 0 || r.params.len() != 1 || r.return_type.is_null() {
        return core::ptr::null_mut();
    }
    let (Ok(full), Ok(short)) = (CString::new(r.full_name.text()), CString::new(name)) else {
        return core::ptr::null_mut();
    };
    let Some(ours) = ClassFunction::create(class, &full, &short, handler, r.flags) else {
        return core::ptr::null_mut();
    };
    ours.parent = (*(real as *const ClassFunction)).parent;
    if !ours.base.take_signature(r) {
        return core::ptr::null_mut();
    }
    let ours = ours as *mut ClassFunction as *mut u8;
    let real = real as *mut u8;
    let mut held = [0u8; core::mem::size_of::<ClassFunction>()];
    core::ptr::copy_nonoverlapping(real, held.as_mut_ptr(), held.len());
    core::ptr::copy_nonoverlapping(ours, real, held.len());
    core::ptr::copy_nonoverlapping(held.as_ptr(), ours, held.len());
    ours as *mut BaseFunction
}

fn active() -> bool {
    !ACTION_VALUE.load(Ordering::Acquire).is_null() && !JUST_PRESSED.load(Ordering::Acquire).is_null()
}
/// Once, from the Lua host's start: false when either read could not be swapped.
#[no_mangle]
pub unsafe extern "C" fn CvrInstallActionReads() -> bool {
    if DONE.swap(true, Ordering::AcqRel) {
        return active();
    }
    let class = RttiSystem::get().and_then(|r| r.get_class(CName::hashed("gamestateMachineGameScriptInterface")));
    let Some(class) = class else {
        log("[input] state machine action reads: class missing, the gestures reach native locomotion only\n");
        return false;
    };
    if let Some(owner) = class.property(CName::hashed("owner")) {
        OWNER_OFFSET.store(owner.value_offset, Ordering::Release);
    }
    ACTION_VALUE.store(swap(class, "GetActionValue", action_value), Ordering::Release);
    JUST_PRESSED.store(swap(class, "IsActionJustPressed", just_pressed), Ordering::Release);
    let status = |f: &AtomicPtr<BaseFunction>| if f.load(Ordering::Acquire).is_null() { "FAILED" } else { "ok" };
    log(&format!(
        "[input] state machine action reads native: GetActionValue {}, IsActionJustPressed {}, owner +0x{:X}\n",
        status(&ACTION_VALUE),
        status(&JUST_PRESSED),
        OWNER_OFFSET.load(Ordering::Acquire)
    ));
    active()
}
/// cvr::swimming::ScriptHooksActive (src/Natives/ActionReads.cpp forwards).
#[no_mangle]
pub extern "C" fn cvr_action_reads_active() -> bool {
    active()
}
