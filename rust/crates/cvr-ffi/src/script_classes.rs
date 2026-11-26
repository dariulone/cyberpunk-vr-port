//! Script classes and methods the Rust build defines itself (formerly mods/redscript/CyberpunkVRPort_Engine): what the
//! engine makes or calls by name -- the state machine's state classes and transitions, a journal callback's listener, a
//! scriptable service, an ink controller -- made the way the script loader makes redscript's. The loader creates a
//! scripted class with CRTTISystem::CreateScriptedClass (vtable +0xE0: the class registered, its parent and the
//! scripted flag set, size and value holder still zero) and fills its functions; its last step, InitializeScriptRuntime
//! (vtable +0xF8), lays every class out (size and value holder from the parent, the flattened property lists, the
//! functions' maps, their frames, the event listeners). So ours are made right before that step, in its hook, and the
//! engine takes them as its own: a ScriptableService of ours is made with the others, a state class is found by the
//! state machine's loader, a controller by its widget library.
//!
//! Their functions are natives answered by Rust (script_hooks::define). A class a script already defines (an older
//! script cache, a third-party mod) is left as it is; so is a method a class already has.
use crate::script_hooks::{HookFn, Sig};
use core::ffi::c_void;
use red4ext_abi::api::{PluginHandle, Sdk};
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::{Class, RttiSystem};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A function a class gets: its name, signature, flags (script_hooks::Sig; function_flags) and Rust's answer.
pub(crate) struct Method {
    pub name: &'static str,
    pub sig: Sig,
    pub flags: u32,
    pub hook: HookFn,
}
/// A scripted class Rust defines: its name, its parent's (native or script name) and its functions.
pub(crate) struct ClassDef {
    pub name: &'static str,
    pub parent: &'static str,
    pub methods: &'static [Method],
}
/// Functions an existing class gets (redscript's @addMethod).
pub(crate) struct Added {
    pub class: &'static str,
    pub methods: &'static [Method],
}

/// The flags of a redscript `func` (public, final) and of a `cb func` (public, event).
pub(crate) const FUNC: u32 = 0x204;
pub(crate) const VIRTUAL: u32 = 0x200;
pub(crate) const CB: u32 = 0x208;

fn classes() -> Vec<&'static ClassDef> {
    let mut v: Vec<&'static ClassDef> = Vec::new();
    v.extend(crate::mechanics::launcher_ready::CLASSES.iter());
    v.extend(crate::mechanics::journal_watch::CLASSES.iter());
    v.extend(crate::mechanics::engine_services::CLASSES.iter());
    v.extend(crate::mechanics::arm_b::CLASSES.iter());
    v
}
fn added() -> Vec<&'static Added> {
    crate::mechanics::launcher_ready::ADDED.iter().collect()
}

/// CClass::Flags: isScriptedClass.
const SCRIPTED_CLASS: u32 = 1 << 2;
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicBool = AtomicBool::new(false);
type InitRuntime = unsafe extern "C" fn(*mut c_void) -> usize;

/// The script loader's last step: ours first, then the engine's.
unsafe extern "C" fn init_runtime(rtti: *mut c_void) -> usize {
    if !DONE.swap(true, Ordering::AcqRel) {
        let _ = std::panic::catch_unwind(define_all);
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    // SAFETY: the engine's InitializeScriptRuntime (RED4ext's trampoline) with its own registry.
    unsafe { core::mem::transmute::<usize, InitRuntime>(original)(rtti) }
}

/// At the plugin's start: InitializeScriptRuntime hooked through RED4ext (its address from the registry's vtable).
///
/// # Safety
/// RED4ext's plugin handle and SDK, from Main's load.
pub(crate) unsafe fn install(handle: PluginHandle, sdk: *const Sdk) -> bool {
    let Some(rtti) = RttiSystem::get() else { return false };
    // SAFETY: the registry's vtable; the SDK's hooking table, from the loader.
    unsafe {
        let vtable = *(rtti as *const RttiSystem as *const *const usize);
        let target = *vtable.add(0xF8 / 8);
        let Some(hooking) = sdk.as_ref().and_then(|s| s.hooking.as_ref()) else { return false };
        let ok = (hooking.attach)(handle, target as *mut c_void, init_runtime as *mut c_void, ORIGINAL.as_ptr() as *mut *mut c_void);
        ok && ORIGINAL.load(Ordering::Acquire) != 0
    }
}

/// CRTTISystem::CreateScriptedClass(name, flags, parent): the class, registered (an existing one when the name has one).
fn create(rtti: &RttiSystem, name: &str, parent: &Class) -> Option<&'static Class> {
    type F = unsafe extern "C" fn(*const RttiSystem, CName, u32, *const Class) -> *const Class;
    let cname = CName::add(&std::ffi::CString::new(name).ok()?);
    // SAFETY: the registry's vtable slot +0xE0 (RTTISystem.hpp), returning the class it made.
    unsafe {
        let vtable = *(rtti as *const RttiSystem as *const *const usize);
        let f = core::mem::transmute::<usize, F>(*vtable.add(0xE0 / 8));
        let made = f(rtti, cname, SCRIPTED_CLASS, parent);
        made.as_ref().or_else(|| rtti.get_class(cname))
    }
}

fn define_methods(class: &'static Class, methods: &[Method]) -> usize {
    methods.iter().filter(|m| crate::script_hooks::define(class, m.name, m.sig, m.flags, m.hook)).count()
}

fn define_all() {
    let Some(rtti) = RttiSystem::get() else { return };
    let (mut made, mut defined, mut total) = (0, 0, 0);
    let all = classes();
    for def in &all {
        total += def.methods.len();
        if rtti.get_class(CName::hashed(def.name)).is_some() {
            cvr_host::log!("[script-classes] {}: a script defines it already, left as it is\n", def.name);
            continue;
        }
        let Some(parent) = crate::script_call::class_any(def.parent) else {
            cvr_host::log!("[script-classes] {}: no parent class {}\n", def.name, def.parent);
            continue;
        };
        let Some(class) = create(rtti, def.name, parent) else {
            cvr_host::log!("[script-classes] {}: CreateScriptedClass made nothing\n", def.name);
            continue;
        };
        made += 1;
        defined += define_methods(class, def.methods);
    }
    let adds = added();
    for add in &adds {
        total += add.methods.len();
        let Some(class) = crate::script_call::class_any(add.class) else {
            cvr_host::log!("[script-classes] {}: no such class for its methods\n", add.class);
            continue;
        };
        defined += define_methods(class, add.methods);
    }
    cvr_host::log!(
        "[script-classes] {made} of {} classes and {defined} of {total} methods defined from Rust before InitializeScriptRuntime\n",
        all.len()
    );
}

/// Once, at the first Running frame: what the engine made of the classes (InitializeScriptRuntime laid them out: the
/// size and value holder from the parent, the function maps).
pub(crate) fn report() {
    let Some(rtti) = RttiSystem::get() else { return };
    let mut lines = Vec::new();
    for def in classes() {
        match rtti.get_class(CName::hashed(def.name)) {
            Some(c) => {
                let at = c as *const Class as usize;
                // SAFETY: a registered class's size (+0x68), value holder (+0x6C) and functions-by-name count (+0x80).
                let (size, holder, by_name) = unsafe { (*((at + 0x68) as *const u32), *((at + 0x6C) as *const u32), *((at + 0x80) as *const u32)) };
                lines.push(format!("{} size 0x{size:X} holder 0x{holder:X} functions {} by name {by_name}", def.name, c.funcs().len()));
            }
            None => lines.push(format!("{} missing", def.name)),
        }
    }
    cvr_host::log!("[script-classes] {}\n", lines.join("; "));
}
