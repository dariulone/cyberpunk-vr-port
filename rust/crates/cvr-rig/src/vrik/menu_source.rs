//! The native menu source, observed at startup (formerly src/Hooks/MenuMode.cpp; CVR_STARTUP_DIAGNOSTICS only):
//! which object the engine reads its menu mode from on the frame path (the handle getter sub_1404C259C called from
//! 0x4C32E8, its owner and mode from sub_1404C278C called from 0x4C32FC), published under a sequence counter, and the
//! menu controllers' RTTI listed once. Both detours are installed by src/Hooks/MenuMode.cpp's registrations, only when
//! the diagnostics are asked for and the code hashes to the 2.5 bytes.
use super::env::GetTickCount64;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::function::Property;
use red4ext_abi::rtti::{Class, RttiSystem};
use std::sync::Mutex;

/// The published row (src/Hooks/MenuMode.cpp's MenuSource, 168 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MenuSource {
    tick: u64,
    object: u64,
    ty: u32,
    getter: u32,
    prefix: [u8; 128],
    owner: u64,
    owner_type: u32,
    mode: i32,
}
const _: () = assert!(core::mem::size_of::<MenuSource>() == 168);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_MenuSourceSeq: AtomicU32;
    static mut CyberpunkVR_MenuSource: MenuSource;
    static CyberpunkVR_MenuMethodsLength: AtomicU32;
    static mut CyberpunkVR_MenuMethods: [u8; 32768];
}

/// The originals the detour registry fills (src/Stereo/DetourRegistry.hpp).
#[no_mangle]
pub static mut cvr_menu_source_get_original: usize = 0;
#[no_mangle]
pub static mut cvr_menu_mode_get_original: usize = 0;

static SOURCE_LOCK: Mutex<()> = Mutex::new(());

fn base() -> usize {
    cvr_engine::code::image_base()
}
fn read_into(at: usize, out: &mut [u8]) -> bool {
    at != 0 && cvr_engine::code::read_bytes(at, out)
}
fn read_u64(at: usize) -> Option<u64> {
    if at == 0 {
        return None;
    }
    cvr_engine::code::read::<u64>(at)
}
fn rva32(address: u64) -> u32 {
    let b = base() as u64;
    if address >= b && address - b <= u32::MAX as u64 {
        (address - b) as u32
    } else {
        0
    }
}
fn diagnostics_wanted() -> bool {
    std::env::var_os("CVR_STARTUP_DIAGNOSTICS").is_some()
}
fn code_hash(rva: usize, len: usize) -> Option<u64> {
    let mut code = vec![0u8; len];
    if !read_into(base() + rva, &mut code) {
        return None;
    }
    let mut h = 14695981039346656037u64;
    for b in code {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    Some(h)
}

/// 2.5 sub_1404C259C, all 0xBF bytes (the handle getter at [object+0x1E8/0x1F0]).
#[no_mangle]
pub extern "C" fn cvr_menu_source_enabled() -> bool {
    diagnostics_wanted() && code_hash(0x4C259C, 0xBF) == Some(0x2F88BC5D4F5787EB)
}
/// 2.5 sub_1404C278C: the same code as 2.31's, its rip-relative operands moved.
#[no_mangle]
pub extern "C" fn cvr_menu_mode_getter_enabled() -> bool {
    diagnostics_wanted() && code_hash(0x4C278C, 0x71) == Some(0x9B2A220D3C65DC17)
}

/// The menu controllers' functions and properties and PauseMenuAction's values, once.
unsafe fn observe_menu_methods() {
    if CyberpunkVR_MenuMethodsLength.load(Ordering::Relaxed) != 0 {
        return;
    }
    let Some(rtti) = RttiSystem::get() else { return };
    if rtti.get_class(CName::hashed("SingleplayerMenuGameController")).is_none() {
        return;
    }
    let mut text = String::new();
    for name in [
        "SingleplayerMenuGameController",
        "PauseMenuListItemData",
        "inkLoadingLayer",
        "inkInitialLoadingControllerSupervisor",
    ] {
        let mut ty: Option<&Class> = rtti.get_class(CName::hashed(name));
        let mut depth = 0;
        while let Some(class) = ty {
            if depth >= 3 {
                break;
            }
            text.push_str("CLASS ");
            text.push_str(class.class_name().text());
            text.push('\n');
            for f in class.funcs().iter().filter_map(|&f| f.as_ref()) {
                text.push_str(f.full_name.text());
                text.push_str(" params=");
                text.push_str(&f.params.len().to_string());
                text.push('\n');
            }
            if depth == 0 {
                for p in class_properties(class) {
                    let Some(p) = p.as_ref() else { continue };
                    let Some(t) = p.ty.as_ref() else { continue };
                    text.push_str("PROPERTY ");
                    text.push_str(p.name.text());
                    text.push(' ');
                    text.push_str(t.name().text());
                    text.push('\n');
                }
            }
            ty = class.parent();
            depth += 1;
        }
    }
    if let Some((hashes, values)) = enum_lists(rtti, CName::hashed("PauseMenuAction")) {
        for (h, v) in hashes.iter().zip(values.iter()) {
            text.push_str("ACTION ");
            text.push_str(h.text());
            text.push(' ');
            text.push_str(&v.to_string());
            text.push('\n');
        }
    }
    let methods = &mut *(&raw mut CyberpunkVR_MenuMethods);
    if text.len() >= methods.len() {
        return;
    }
    methods[..text.len()].copy_from_slice(text.as_bytes());
    CyberpunkVR_MenuMethodsLength.store(text.len() as u32, Ordering::Release);
}
/// CClass::GetProperties: the class's properties, the parents' included (the engine's own walk).
unsafe fn class_properties(class: &Class) -> Vec<*mut Property> {
    static GET: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(red4ext_abi::hashes::CClass_GetProperties);
    type F = unsafe extern "C" fn(*const Class, *mut DynArray<*mut Property>) -> *mut Property;
    let Some(f) = GET.func::<F>() else { return Vec::new() };
    let mut list = DynArray::<*mut Property>::default();
    f(class, &mut list);
    let out = list.as_slice().to_vec();
    list.free();
    out
}
/// CRTTISystem::GetEnum (+0x18): the enum's hashList (+0x28) and valueList (+0x38).
unsafe fn enum_lists(rtti: &RttiSystem, name: CName) -> Option<(Vec<CName>, Vec<i64>)> {
    type F = unsafe extern "C" fn(*const RttiSystem, CName) -> *const u8;
    let e = red4ext_abi::rtti::vfunc::<F>(rtti as *const RttiSystem as *const c_void, 0x18)(rtti, name);
    if e.is_null() {
        return None;
    }
    let hashes = (*(e.add(0x28) as *const DynArray<CName>)).as_slice().to_vec();
    let values = (*(e.add(0x38) as *const DynArray<i64>)).as_slice().to_vec();
    Some((hashes, values))
}

type SourceGet = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
type ModeGet = unsafe extern "C" fn(*mut c_void) -> i32;

#[no_mangle]
pub unsafe extern "C" fn cvr_menu_source_get_at(system: *mut c_void, output: *mut c_void, return_address: usize) -> *mut c_void {
    let result = core::mem::transmute::<usize, SourceGet>(cvr_menu_source_get_original)(system, output);
    if return_address != base() + 0x4C32E8 {
        return result;
    }
    let mut row = MenuSource {
        tick: GetTickCount64(),
        object: 0,
        ty: 0,
        getter: 0,
        prefix: [0; 128],
        owner: 0,
        owner_type: 0,
        mode: -1,
    };
    if let Some(object) = read_u64(output as usize) {
        row.object = object;
        if object != 0 {
            if let Some(ty) = read_u64(object as usize) {
                if let Some(getter) = read_u64(ty as usize + 0x18) {
                    row.ty = rva32(ty);
                    row.getter = rva32(getter);
                    read_into(object as usize, &mut row.prefix);
                }
            }
        }
    }
    let _guard = SOURCE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    observe_menu_methods();
    CyberpunkVR_MenuSourceSeq.fetch_add(1, Ordering::SeqCst);
    CyberpunkVR_MenuSource = row;
    CyberpunkVR_MenuSourceSeq.fetch_add(1, Ordering::SeqCst);
    result
}
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_mode_get_at(source: *mut c_void, return_address: usize) -> i32 {
    let result = core::mem::transmute::<usize, ModeGet>(cvr_menu_mode_get_original)(source);
    if return_address == base() + 0x4C32FC {
        let owner = read_u64(source as usize + 8).unwrap_or(0);
        let ty = if owner != 0 { read_u64(owner as usize).unwrap_or(0) } else { 0 };
        let _guard = SOURCE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        CyberpunkVR_MenuSourceSeq.fetch_add(1, Ordering::SeqCst);
        let row = &mut *(&raw mut CyberpunkVR_MenuSource);
        if row.object == source as u64 {
            row.owner = owner;
            row.owner_type = rva32(ty);
            row.mode = result;
        }
        CyberpunkVR_MenuSourceSeq.fetch_add(1, Ordering::SeqCst);
    }
    result
}

// ---- the entries: the return address as one more argument ----
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_source_get_entry() {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym cvr_menu_source_get_at);
}
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_mode_get_entry() {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym cvr_menu_mode_get_at);
}
