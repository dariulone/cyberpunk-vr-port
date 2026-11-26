//! ANR FG (Apex Neural Rendering, github.com/apex-compute/ANR) AS A DLSS-G PROVIDER (main's 0.2.4
//! src/Framegen/AnrFrameGen.cpp): an nvngx_dlssg.dll of its own that exports NGX's D3D12 entry points. OptiScaler runs
//! it as a custom frame generation provider -- it loads the dll from custom/<name>/dlssg and sends the game's NGX calls
//! for the FrameGeneration feature to the dll's exports with the caller's own parameter table. The plugin does the same
//! with the package unchanged in its anr\dlssg folder: Init_Ext with that folder as the feature path,
//! PopulateParameters_Impl for the caps, and CreateFeature / EvaluateFeature with the DLSS-G helpers' own keys and
//! values (dlssg_native.rs), so every key and value is the one NVIDIA's DLSS-G gets.
//!
//! A provider dll has no parameter allocator: the table is the plugin's own (Parameters), an object with the vtable of
//! NGX's NVSDK_NGX_Parameter as MSVC lays it out (the measured order: neural_render.rs), with OptiScaler's semantics --
//! a value read as another numeric type is converted, a pointer is read only as a pointer (or as an unsigned 64-bit
//! integer), a missing key fails.
use core::ffi::{c_char, c_void, CStr};
use core::ptr::null_mut;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use windows::core::PCSTR;
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};

use crate::dlssg_native::{FeatureCommonInfo, Ngx, Parameter};

const RESULT_SUCCESS: u32 = 1;
const RESULT_FAIL: u32 = 0xBAD0_0000;
const VERSION_API: i32 = 0x15;
const LOGGING_OFF: i32 = 0;
const LOGGING_ON: i32 = 1;
fn failed(result: u32) -> bool {
    result & 0xFFF0_0000 == RESULT_FAIL
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

// ---- the parameter table (NVSDK_NGX_Parameter with OptiScaler's semantics) -----------------------------------------

#[derive(Clone, Copy)]
enum Value {
    Ull(u64),
    F(f32),
    D(f64),
    Ui(u32),
    I(i32),
    Ptr(*mut c_void),
}
impl Value {
    fn number(self) -> Option<f64> {
        Some(match self {
            Value::Ull(v) => v as f64,
            Value::F(v) => v as f64,
            Value::D(v) => v,
            Value::Ui(v) => v as f64,
            Value::I(v) => v as f64,
            Value::Ptr(_) => return None,
        })
    }
}

/// The vtable's entries (functions).
#[repr(transparent)]
struct VTable([*const (); 17]);
// SAFETY: plain function addresses.
unsafe impl Sync for VTable {}

/// The table: its vtable first, as the provider's C++ calls it.
#[repr(C)]
pub struct Parameters {
    vtable: *const VTable,
    values: Mutex<HashMap<CString, Value>>,
}
fn name_of(name: *const c_char) -> CString {
    if name.is_null() {
        CString::default()
    } else {
        // SAFETY: the caller's NUL-terminated name.
        unsafe { CStr::from_ptr(name) }.to_owned()
    }
}
/// # Safety
/// `this` is one of ours.
unsafe fn put(this: *mut Parameters, name: *const c_char, value: Value) {
    // SAFETY: as the caller vouches.
    if let Some(p) = unsafe { this.as_ref() } {
        p.values.lock().unwrap_or_else(|e| e.into_inner()).insert(name_of(name), value);
    }
}
/// # Safety
/// As put.
unsafe fn find(this: *const Parameters, name: *const c_char) -> Option<Value> {
    // SAFETY: as the caller vouches.
    let p = unsafe { this.as_ref() }?;
    p.values.lock().unwrap_or_else(|e| e.into_inner()).get(&name_of(name)).copied()
}

unsafe extern "C" fn set_ptr(t: *mut Parameters, n: *const c_char, v: *mut c_void) {
    unsafe { put(t, n, Value::Ptr(v)) }
}
unsafe extern "C" fn set_d3d(t: *mut Parameters, n: *const c_char, v: *mut c_void) {
    unsafe { put(t, n, Value::Ptr(v)) }
}
unsafe extern "C" fn set_ui(t: *mut Parameters, n: *const c_char, v: u32) {
    unsafe { put(t, n, Value::Ui(v)) }
}
unsafe extern "C" fn set_i(t: *mut Parameters, n: *const c_char, v: i32) {
    unsafe { put(t, n, Value::I(v)) }
}
unsafe extern "C" fn set_d(t: *mut Parameters, n: *const c_char, v: f64) {
    unsafe { put(t, n, Value::D(v)) }
}
unsafe extern "C" fn set_f(t: *mut Parameters, n: *const c_char, v: f32) {
    unsafe { put(t, n, Value::F(v)) }
}
unsafe extern "C" fn set_ull(t: *mut Parameters, n: *const c_char, v: u64) {
    unsafe { put(t, n, Value::Ull(v)) }
}
unsafe extern "C" fn get_ptr(t: *mut Parameters, n: *const c_char, out: *mut *mut c_void) -> u32 {
    match (unsafe { find(t, n) }, out.is_null()) {
        (Some(Value::Ptr(p)), false) => {
            unsafe { *out = p };
            RESULT_SUCCESS
        }
        _ => RESULT_FAIL,
    }
}
macro_rules! getter {
    ($name:ident, $t:ty) => {
        unsafe extern "C" fn $name(t: *mut Parameters, n: *const c_char, out: *mut $t) -> u32 {
            let Some(v) = (unsafe { find(t, n) }) else { return RESULT_FAIL };
            if out.is_null() {
                return RESULT_FAIL;
            }
            match v.number() {
                Some(x) => unsafe { *out = x as $t },
                None => return RESULT_FAIL,
            }
            RESULT_SUCCESS
        }
    };
}
getter!(get_ui, u32);
getter!(get_i, i32);
getter!(get_d, f64);
getter!(get_f, f32);
unsafe extern "C" fn get_ull(t: *mut Parameters, n: *const c_char, out: *mut u64) -> u32 {
    let Some(v) = (unsafe { find(t, n) }) else { return RESULT_FAIL };
    if out.is_null() {
        return RESULT_FAIL;
    }
    let value = match v {
        // a pointer reads as an unsigned 64-bit integer
        Value::Ptr(p) => p as u64,
        Value::Ull(x) => x,
        other => other.number().map_or(0, |x| x as u64),
    };
    unsafe { *out = value };
    RESULT_SUCCESS
}
unsafe extern "C" fn reset(t: *mut Parameters) {
    if let Some(p) = unsafe { t.as_ref() } {
        p.values.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// NVSDK_NGX_Parameter's vtable as MSVC lays out its overloads (measured, neural_render.rs): Set(void*), Set(D3D12),
/// Set(D3D11), Set(uint), Set(int), Set(double), Set(float), Set(ull), the Gets in the same order, Reset.
static VTABLE: VTable = VTable([
    set_ptr as *const (),
    set_d3d as *const (),
    set_d3d as *const (),
    set_ui as *const (),
    set_i as *const (),
    set_d as *const (),
    set_f as *const (),
    set_ull as *const (),
    get_ptr as *const (),
    get_ptr as *const (),
    get_ptr as *const (),
    get_ui as *const (),
    get_i as *const (),
    get_d as *const (),
    get_f as *const (),
    get_ull as *const (),
    reset as *const (),
]);

unsafe extern "C" fn allocate(out: *mut Parameter) -> u32 {
    if out.is_null() {
        return RESULT_FAIL;
    }
    let p = Box::new(Parameters { vtable: &VTABLE, values: Mutex::new(HashMap::new()) });
    // SAFETY: the caller's output.
    unsafe { *out = Box::into_raw(p).cast() };
    RESULT_SUCCESS
}
unsafe extern "C" fn destroy(p: Parameter) -> u32 {
    if !p.is_null() {
        // SAFETY: one of allocate's.
        drop(unsafe { Box::from_raw(p.cast::<Parameters>()) });
    }
    RESULT_SUCCESS
}

// ---- the provider's exports ------------------------------------------------------------------------------------------

type InitExtFn = unsafe extern "C" fn(u64, *const u16, *mut c_void, i32, *const FeatureCommonInfo) -> u32;
type CreateFn = unsafe extern "C" fn(*mut c_void, i32, Parameter, *mut *mut c_void) -> u32;
type EvaluateFn = unsafe extern "C" fn(*mut c_void, *const c_void, Parameter, *const c_void) -> u32;
type ReleaseFn = unsafe extern "C" fn(*mut c_void) -> u32;
type PopulateFn = unsafe extern "C" fn(Parameter) -> u32;

struct Provider {
    dll: usize,
    init_ext: Option<InitExtFn>,
    create: Option<CreateFn>,
    evaluate: Option<EvaluateFn>,
    release: Option<ReleaseFn>,
    populate: Option<PopulateFn>,
    initialized: usize,
    available: bool,
    reason: String,
}
static PROVIDER: Mutex<Provider> = Mutex::new(Provider {
    dll: 0,
    init_ext: None,
    create: None,
    evaluate: None,
    release: None,
    populate: None,
    initialized: 0,
    available: false,
    reason: String::new(),
});
fn provider() -> std::sync::MutexGuard<'static, Provider> {
    PROVIDER.lock().unwrap_or_else(|e| e.into_inner())
}

/// The plugin's folder\anr\dlssg (the package as Nvngx_Custom loads it: the dll there, the files it reads beside it).
fn folder() -> std::path::PathBuf {
    cvr_host::paths::module_folder().unwrap_or_else(|| std::path::PathBuf::from(".")).join("anr").join("dlssg")
}
fn wide(p: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn load(p: &mut Provider) -> bool {
    if p.dll != 0 {
        return true;
    }
    let path = folder().join("nvngx_dlssg.dll");
    if !path.exists() {
        p.reason = "ANR FG is not installed (anr\\dlssg\\nvngx_dlssg.dll beside the plugin)".into();
        return false;
    }
    let w = wide(&path);
    // SAFETY: a NUL-terminated path; the provider dll's own DllMain.
    let dll = match unsafe { LoadLibraryExW(windows::core::PCWSTR(w.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) } {
        Ok(m) => m,
        Err(e) => {
            p.reason = format!("ANR FG could not be loaded (error {})", e.code().0 as u32 & 0xFFFF);
            return false;
        }
    };
    let export = |name: &CStr| -> usize {
        // SAFETY: the loaded module's export lookup.
        unsafe { GetProcAddress(dll, PCSTR(name.as_ptr().cast())) }.map_or(0, |f| f as usize)
    };
    let (init, create, evaluate, release, populate) = (
        export(c"NVSDK_NGX_D3D12_Init_Ext"),
        export(c"NVSDK_NGX_D3D12_CreateFeature"),
        export(c"NVSDK_NGX_D3D12_EvaluateFeature"),
        export(c"NVSDK_NGX_D3D12_ReleaseFeature"),
        export(c"NVSDK_NGX_D3D12_PopulateParameters_Impl"),
    );
    if init == 0 || create == 0 || evaluate == 0 || release == 0 {
        p.reason = "ANR FG's nvngx_dlssg.dll lacks the NGX D3D12 entry points".into();
        // SAFETY: ours.
        let _ = unsafe { FreeLibrary(dll) };
        return false;
    }
    // SAFETY: the exports of the NGX signatures (public NGX ABI).
    unsafe {
        p.init_ext = Some(core::mem::transmute::<usize, InitExtFn>(init));
        p.create = Some(core::mem::transmute::<usize, CreateFn>(create));
        p.evaluate = Some(core::mem::transmute::<usize, EvaluateFn>(evaluate));
        p.release = Some(core::mem::transmute::<usize, ReleaseFn>(release));
        p.populate = (populate != 0).then(|| core::mem::transmute::<usize, PopulateFn>(populate));
    }
    p.dll = dll.0 as usize;
    true
}

unsafe extern "C" fn provider_log(message: *const c_char, _: i32, _: i32) {
    if !message.is_null() {
        // SAFETY: the provider's NUL-terminated message.
        log(&format!("[anr-fg] {}", unsafe { CStr::from_ptr(message) }.to_string_lossy()));
    }
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
}

/// Available: ANR FG on this device (the provider loaded and initialised once per device, its caps read).
///
/// # Safety
/// `device` is a live device or null.
pub unsafe fn available(device: *mut c_void) -> bool {
    let mut p = provider();
    if device.is_null() {
        return false;
    }
    if device as usize == p.initialized {
        return p.available;
    }
    p.initialized = device as usize;
    p.available = false;
    if !load(&mut p) {
        log(&format!("[framegen] ANR FG: {}\n", p.reason));
        return false;
    }
    // OptiScaler forwards the game's own D3D12_Init_Ext with its feature info: the feature dll's folder first.
    let dir = wide(&folder());
    let paths = [dir.as_ptr()];
    // SAFETY: the exported switch.
    let diagnostics = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0;
    let info = FeatureCommonInfo::new(paths.as_ptr(), 1, Some(provider_log), if diagnostics { LOGGING_ON } else { LOGGING_OFF });
    let logs = wide(&cvr_host::paths::dir(cvr_host::paths::Folder::Log));
    let init = p.init_ext.expect("loaded");
    // SAFETY: NUL-terminated strings, the live device and the info above.
    let result = unsafe { init(0x4379_5650, logs.as_ptr(), device, VERSION_API, &info) };
    if failed(result) {
        p.reason = format!("ANR FG initialization failed (0x{result:08X})");
        log(&format!("[framegen] {}\n", p.reason));
        return false;
    }
    let (mut supported, mut frames) = (1i32, 0u32);
    if let Some(populate) = p.populate {
        let mut caps: Parameter = null_mut();
        // SAFETY: our table, made and destroyed here.
        unsafe {
            allocate(&mut caps);
            if !failed(populate(caps)) {
                let this = caps.cast::<Parameters>();
                if failed(get_i(this, c"FrameGeneration.Available".as_ptr(), &mut supported)) {
                    supported = 1;
                }
                get_ui(this, c"DLSSG.MultiFrameCountMax".as_ptr(), &mut frames);
            }
            destroy(caps);
        }
    }
    log(&format!("[framegen] ANR FG: loaded, available={supported} frames={frames}\n"));
    if supported == 0 {
        p.reason = "ANR FG reports frame generation unavailable on this GPU".into();
        return false;
    }
    p.available = true;
    p.reason.clear();
    true
}

/// Reason: why ANR FG is not available ("ANR FG was not looked for yet" before the first try).
pub fn reason() -> String {
    let p = provider();
    if p.initialized == 0 && p.reason.is_empty() {
        "ANR FG was not looked for yet".into()
    } else {
        p.reason.clone()
    }
}

// The table dlssg_native's helpers run on: the provider's feature calls and the plugin's own parameter table.
unsafe extern "C" fn no_init(_: *const c_char, _: i32, _: *const c_char, _: *const u16, _: *mut c_void, _: *const FeatureCommonInfo, _: i32) -> u32 {
    RESULT_FAIL
}
unsafe extern "C" fn no_capability(_: *mut Parameter) -> u32 {
    RESULT_FAIL
}
unsafe extern "C" fn create(list: *mut c_void, feature: i32, p: Parameter, handle: *mut *mut c_void) -> u32 {
    match provider().create {
        // SAFETY: the provider's export with the caller's arguments.
        Some(f) => unsafe { f(list, feature, p, handle) },
        None => RESULT_FAIL,
    }
}
unsafe extern "C" fn release(handle: *mut c_void) -> u32 {
    match (provider().release, handle.is_null()) {
        // SAFETY: as create.
        (Some(f), false) => unsafe { f(handle) },
        _ => RESULT_FAIL,
    }
}
unsafe extern "C" fn evaluate(list: *mut c_void, handle: *const c_void, p: Parameter, callback: *const c_void) -> u32 {
    match provider().evaluate {
        // SAFETY: as create.
        Some(f) => unsafe { f(list, handle, p, callback) },
        None => RESULT_FAIL,
    }
}
unsafe extern "C" fn t_set_ui(p: Parameter, n: *const c_char, v: u32) {
    unsafe { set_ui(p.cast(), n, v) }
}
unsafe extern "C" fn t_set_f(p: Parameter, n: *const c_char, v: f32) {
    unsafe { set_f(p.cast(), n, v) }
}
unsafe extern "C" fn t_set_resource(p: Parameter, n: *const c_char, v: *mut c_void) {
    unsafe { set_d3d(p.cast(), n, v) }
}
unsafe extern "C" fn t_set_pointer(p: Parameter, n: *const c_char, v: *mut c_void) {
    unsafe { set_ptr(p.cast(), n, v) }
}
unsafe extern "C" fn t_get_i(p: Parameter, n: *const c_char, out: *mut i32) -> u32 {
    unsafe { get_i(p.cast(), n, out) }
}
unsafe extern "C" fn t_get_ui(p: Parameter, n: *const c_char, out: *mut u32) -> u32 {
    unsafe { get_ui(p.cast(), n, out) }
}
unsafe extern "C" fn t_get_pointer(p: Parameter, n: *const c_char, out: *mut *mut c_void) -> u32 {
    unsafe { get_ptr(p.cast(), n, out) }
}
/// The int-valued set the provider proxy makes before each evaluation (DLSSG.ShowDebug).
///
/// # Safety
/// `p` is one of ours.
pub unsafe fn set_int(p: Parameter, name: &CStr, v: i32) {
    unsafe { set_i(p.cast(), name.as_ptr(), v) }
}

pub static TABLE: Ngx = Ngx {
    init: no_init,
    capability: no_capability,
    allocate,
    destroy,
    create,
    release,
    evaluate,
    set_ui: t_set_ui,
    set_f: t_set_f,
    set_resource: t_set_resource,
    set_pointer: t_set_pointer,
    get_i: t_get_i,
    get_ui: t_get_ui,
    get_pointer: t_get_pointer,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_converts_numbers_and_keeps_pointers_as_pointers() {
        // SAFETY: our own table through its vtable's functions.
        unsafe {
            let mut p: Parameter = null_mut();
            assert_eq!(allocate(&mut p), RESULT_SUCCESS);
            let t = p.cast::<Parameters>();
            set_ui(t, c"Width".as_ptr(), 1920);
            set_f(t, c"Scale".as_ptr(), 1.5);
            let mut x = 7u32;
            set_ptr(t, c"Buffer".as_ptr(), (&mut x as *mut u32).cast());
            let (mut i, mut f, mut ull) = (0i32, 0f32, 0u64);
            assert_eq!(get_i(t, c"Width".as_ptr(), &mut i), RESULT_SUCCESS);
            assert_eq!(i, 1920);
            assert_eq!(get_f(t, c"Scale".as_ptr(), &mut f), RESULT_SUCCESS);
            assert_eq!(f, 1.5);
            let mut ui = 0u32;
            assert_eq!(get_ui(t, c"Scale".as_ptr(), &mut ui), RESULT_SUCCESS);
            assert_eq!(ui, 1);
            // a pointer: not a number, but an unsigned 64-bit integer and a pointer
            assert_eq!(get_i(t, c"Buffer".as_ptr(), &mut i), RESULT_FAIL);
            assert_eq!(get_ull(t, c"Buffer".as_ptr(), &mut ull), RESULT_SUCCESS);
            assert_eq!(ull, (&mut x as *mut u32) as u64);
            let mut out: *mut c_void = null_mut();
            assert_eq!(get_ptr(t, c"Width".as_ptr(), &mut out), RESULT_FAIL);
            assert_eq!(get_ptr(t, c"Missing".as_ptr(), &mut out), RESULT_FAIL);
            // the vtable's slots as the provider calls them
            let vt = *(p as *const *const [usize; 17]);
            let set: unsafe extern "C" fn(Parameter, *const c_char, u32) = core::mem::transmute((*vt)[3]);
            set(p, c"Height".as_ptr(), 1080);
            let get: unsafe extern "C" fn(Parameter, *const c_char, *mut u32) -> u32 = core::mem::transmute((*vt)[11]);
            assert_eq!(get(p, c"Height".as_ptr(), &mut ui), RESULT_SUCCESS);
            assert_eq!(ui, 1080);
            reset(t);
            assert_eq!(get_ui(t, c"Height".as_ptr(), &mut ui), RESULT_FAIL);
            destroy(p);
        }
    }
}
