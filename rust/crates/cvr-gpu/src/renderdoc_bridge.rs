//! Coexisting with a resident RenderDoc capture layer (formerly src/Hooks/RenderDocBridge.cpp): the port's hooks of the
//! loader functions RenderDoc also hooks are routed through RenderDoc's serializer (the game's import of
//! D3D12CreateDevice when RenderDoc patched it, else the port's RenderDoc build's RENDERDOC_CPVR_GetHookedProcAddress),
//! a factory hook is chained behind RenderDoc's (RENDERDOC_CPVR_SetHookOriginal), and NVAPI is allowed through the
//! capture layer so the game's ray tracing loads. Without renderdoc.dll every answer is the plain one.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void, CStr};
use cvr_host::log;
use std::sync::Mutex;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleExW, GetProcAddress, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};

fn p(v: *const c_void) -> String {
    format!("{:016X}", v as usize)
}
/// Each function's routing logged once (at most 16 names).
fn should_log(name: &str) -> bool {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
    if seen.iter().any(|s| s == name) {
        return false;
    }
    if seen.len() < 16 {
        seen.push(name.to_string());
    }
    true
}
unsafe fn module(name: &CStr) -> Option<HMODULE> {
    GetModuleHandleA(PCSTR(name.as_ptr() as *const u8)).ok()
}
unsafe fn proc(module: HMODULE, name: &CStr) -> *mut c_void {
    GetProcAddress(module, PCSTR(name.as_ptr() as *const u8)).map_or(core::ptr::null_mut(), |f| f as *mut c_void)
}
/// The game's own import of `function`, when RenderDoc's module owns what it points at.
unsafe fn game_import_owned_by(function: &[u8], owner: HMODULE) -> Option<*mut c_void> {
    let exe = GetModuleHandleA(PCSTR::null()).ok()?.0 as usize;
    let nt = exe + *((exe + 0x3C) as *const u32) as usize;
    // IMAGE_OPTIONAL_HEADER64::DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]: VirtualAddress, Size.
    let dir_va = *((nt + 0x18 + 0x78) as *const u32) as usize;
    let dir_size = *((nt + 0x18 + 0x7C) as *const u32) as usize;
    if dir_va == 0 {
        return None;
    }
    let mut i = 0;
    while i < dir_size / 20 {
        let d = exe + dir_va + 20 * i;
        let (original_first, name, first) = (*(d as *const u32), *((d + 12) as *const u32), *((d + 16) as *const u32));
        if name == 0 {
            break;
        }
        i += 1;
        if original_first == 0 || first == 0 {
            continue;
        }
        let mut j = 0;
        loop {
            let data = *((exe + original_first as usize + 8 * j) as *const u64);
            if data == 0 {
                break;
            }
            let slot = exe + first as usize + 8 * j;
            j += 1;
            if data & (1u64 << 63) != 0 {
                continue;
            }
            if CStr::from_ptr((exe + data as usize + 2) as *const c_char).to_bytes() != function {
                continue;
            }
            let imported = *(slot as *const *mut c_void);
            let mut found = HMODULE::default();
            if GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(imported as *const u16),
                &mut found,
            )
            .is_ok()
                && found == owner
            {
                return Some(imported);
            }
        }
    }
    None
}

/// cvr::RenderDocResolveHookTarget: where a hook of `function` in `module` must go so RenderDoc's capture sees the
/// port's objects.
#[no_mangle]
pub unsafe extern "C" fn cvr_renderdoc_resolve_hook_target(module: *mut c_void, function: *const c_char) -> *mut c_void {
    if module.is_null() || function.is_null() {
        return core::ptr::null_mut();
    }
    let name = CStr::from_ptr(function);
    let text = name.to_string_lossy();
    let raw = proc(HMODULE(module), name);
    let Some(renderdoc) = module_named(c"renderdoc.dll") else { return raw };
    if name.to_bytes() == b"D3D12CreateDevice" {
        if let Some(imported) = game_import_owned_by(name.to_bytes(), renderdoc) {
            if should_log(&text) {
                log!("[RenderDoc] '{text}' routed through game import {} (loader {})\n", p(imported), p(raw));
            }
            return imported;
        }
    }
    let resolve = proc(renderdoc, c"RENDERDOC_CPVR_GetHookedProcAddress");
    if resolve.is_null() {
        if should_log(&text) {
            log!(
                "[RenderDoc] renderdoc.dll is resident but RENDERDOC_CPVR_GetHookedProcAddress is missing -- '{text}' stays raw, so captures will not see our wrapped objects. Use the renderdoc.dll built from renderdoc-src.\n"
            );
        }
        return raw;
    }
    type Resolve = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
    let hooked = core::mem::transmute::<*mut c_void, Resolve>(resolve)(module, function);
    if hooked.is_null() {
        return raw; // RenderDoc does not hook this one
    }
    if should_log(&text) {
        log!("[RenderDoc] '{text}' routed through the serializer {} (loader {})\n", p(hooked), p(raw));
    }
    hooked
}
unsafe fn module_named(name: &CStr) -> Option<HMODULE> {
    module(name)
}
/// cvr::RenderDocResolveRawTarget: the loader's own export.
#[no_mangle]
pub unsafe extern "C" fn cvr_renderdoc_resolve_raw_target(module: *mut c_void, function: *const c_char) -> *mut c_void {
    if module.is_null() || function.is_null() {
        return core::ptr::null_mut();
    }
    proc(HMODULE(module), CStr::from_ptr(function))
}
/// cvr::RenderDocChainBehindHook: RenderDoc calls our trampoline as its original of `module!function`.
#[no_mangle]
pub unsafe extern "C" fn cvr_renderdoc_chain_behind_hook(module: *const c_char, function: *const c_char, trampoline: *mut c_void) -> bool {
    if module.is_null() || function.is_null() || trampoline.is_null() {
        return false;
    }
    let Some(renderdoc) = module_named(c"renderdoc.dll") else { return false };
    let (m, f) = (CStr::from_ptr(module).to_string_lossy(), CStr::from_ptr(function).to_string_lossy());
    let set = proc(renderdoc, c"RENDERDOC_CPVR_SetHookOriginal");
    if set.is_null() {
        if should_log(&f) {
            log!(
                "[RenderDoc] RENDERDOC_CPVR_SetHookOriginal is missing -- '{m}!{f}' cannot be chained, so the capture layer will not see the factory. Use the renderdoc.dll built from renderdoc-src.\n"
            );
        }
        return false;
    }
    type SetOriginal = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_void) -> bool;
    let ok = core::mem::transmute::<*mut c_void, SetOriginal>(set)(module, function, trampoline);
    log!(
        "[RenderDoc] chained behind '{m}!{f}': {} (our trampoline {})\n",
        if ok { "ok" } else { "not hooked by RenderDoc" },
        p(trampoline)
    );
    ok
}
/// cvr::RenderDocAllowNvApi: capture option 12 (AllowUnsupportedVendorExtensions) set to NVIDIA's vendor id, so NVAPI
/// passes the capture layer and the game's ray tracing loads.
#[no_mangle]
pub unsafe extern "C" fn cvr_renderdoc_allow_nvapi() -> bool {
    let Some(renderdoc) = module_named(c"renderdoc.dll") else { return false };
    let get_api = proc(renderdoc, c"RENDERDOC_GetAPI");
    if get_api.is_null() {
        log!("[RenderDoc] renderdoc.dll is resident but RENDERDOC_GetAPI is missing\n");
        return false;
    }
    type GetApi = unsafe extern "C" fn(i32, *mut *mut c_void) -> i32;
    let mut api: *mut c_void = core::ptr::null_mut();
    if core::mem::transmute::<*mut c_void, GetApi>(get_api)(10600, &mut api) != 1 || api.is_null() {
        log!("[RenderDoc] RENDERDOC_GetAPI(1.6.0) failed -- NVAPI stays disabled and the game will report that ray tracing could not load\n");
        return false;
    }
    // The API table: GetAPIVersion, then SetCaptureOptionU32.
    let set_option = *((api as usize + 8) as *const *mut c_void);
    if set_option.is_null() {
        return false;
    }
    type SetOption = unsafe extern "C" fn(i32, u32) -> i32;
    let ok = core::mem::transmute::<*mut c_void, SetOption>(set_option)(12, 0x10DE);
    log!(
        "[RenderDoc] NVAPI vendor extension {} (capture option 12 = 0x10DE)\n",
        if ok != 0 { "ENABLED" } else { "REJECTED" }
    );
    ok != 0
}
