//! NATIVE NVIDIA DLSS FRAME GENERATION THROUGH NGX (formerly src/Framegen/DlssFrameGen.cpp's): NGX initialised on the
//! device once (one instance per adapter for the whole process, shared with the game's own DLSS, never shut down) and
//! its frame-generation capability read; one feature per eye created into a list (its VRAM estimate through the
//! capability's callback) and released; its evaluation with the engine camera's row-vector matrices
//! (cvr_gpu::dlssg_matrices). The DLSS SDK's D3D12 helpers (nvsdk_ngx_helpers_dlssg_d3d.h, DLSS 310.9.1) set their
//! parameters by name; their sets are made here in the same order, the optional evaluation parameters' defaults included.
//!
//! NGX's entries: the C++ half's static loader when it lends them (`cvr_dlssg2_attach`); otherwise -- the Rust build --
//! the driver's NGX core the game has loaded (_nvngx.dll's D3D12 exports, as neural_render.rs takes them), its parameter
//! objects set and read through their vtable. The provider is whose DLSS-G runs the feature (main's 0.2.4): NVIDIA's
//! through NGX's core, or ANR FG's dll (anr_fg.rs) fed the same parameters.
use core::ffi::{c_char, c_void, CStr};
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use cvr_gpu::dlssg_matrices::{self, Matrices};
use cvr_gpu::frame_capture::CameraData;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

use crate::framegen_generator::DlssEvaluation;

type Result = u32;
pub type Parameter = *mut c_void;

/// NGX's entries (the C++ half's table).
#[repr(C)]
pub struct Ngx {
    /// NVSDK_NGX_D3D12_Init_with_ProjectID's order (the static loader's): the feature info before the version.
    pub init: unsafe extern "C" fn(*const c_char, i32, *const c_char, *const u16, *mut c_void, *const FeatureCommonInfo, i32) -> Result,
    pub capability: unsafe extern "C" fn(*mut Parameter) -> Result,
    pub allocate: unsafe extern "C" fn(*mut Parameter) -> Result,
    pub destroy: unsafe extern "C" fn(Parameter) -> Result,
    pub create: unsafe extern "C" fn(*mut c_void, i32, Parameter, *mut *mut c_void) -> Result,
    pub release: unsafe extern "C" fn(*mut c_void) -> Result,
    pub evaluate: unsafe extern "C" fn(*mut c_void, *const c_void, Parameter, *const c_void) -> Result,
    pub set_ui: unsafe extern "C" fn(Parameter, *const c_char, u32),
    pub set_f: unsafe extern "C" fn(Parameter, *const c_char, f32),
    pub set_resource: unsafe extern "C" fn(Parameter, *const c_char, *mut c_void),
    pub set_pointer: unsafe extern "C" fn(Parameter, *const c_char, *mut c_void),
    pub get_i: unsafe extern "C" fn(Parameter, *const c_char, *mut i32) -> Result,
    pub get_ui: unsafe extern "C" fn(Parameter, *const c_char, *mut u32) -> Result,
    pub get_pointer: unsafe extern "C" fn(Parameter, *const c_char, *mut *mut c_void) -> Result,
}
const _: () = assert!(core::mem::size_of::<Ngx>() == 14 * 8);
static NGX: AtomicPtr<Ngx> = AtomicPtr::new(null_mut());
/// # Safety
/// `ngx` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dlssg2_attach(ngx: *const Ngx) {
    NGX.store(ngx.cast_mut(), Ordering::Release);
}
fn ngx() -> Option<&'static Ngx> {
    // SAFETY: attached once with static storage.
    if let Some(n) = unsafe { NGX.load(Ordering::Acquire).as_ref() } {
        return Some(n);
    }
    driver_table()
}
/// The provider whose DLSS-G a feature runs (main's dlssg::Provider).
pub const PROVIDER_NVIDIA: u8 = 0;
pub const PROVIDER_ANR: u8 = 1;
fn table(provider: u8) -> Option<&'static Ngx> {
    if provider == PROVIDER_ANR {
        Some(&crate::anr_fg::TABLE)
    } else {
        ngx()
    }
}

// ---- the driver's NGX core (_nvngx.dll) as the table ----------------------------------------------------------------

static DRIVER_INIT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static DRIVER: std::sync::OnceLock<Ngx> = std::sync::OnceLock::new();
/// NVSDK_NGX_Parameter's measured vtable slots (neural_render.rs).
unsafe fn slot(p: Parameter, offset: usize) -> usize {
    // SAFETY: a live parameter object of NGX's.
    unsafe { *((*(p as *const *const u8)).add(offset) as *const usize) }
}
unsafe extern "C" fn vt_set_ui(p: Parameter, n: *const c_char, v: u32) {
    let f: unsafe extern "C" fn(Parameter, *const c_char, u32) = unsafe { core::mem::transmute(slot(p, 0x18)) };
    unsafe { f(p, n, v) }
}
unsafe extern "C" fn vt_set_f(p: Parameter, n: *const c_char, v: f32) {
    let f: unsafe extern "C" fn(Parameter, *const c_char, f32) = unsafe { core::mem::transmute(slot(p, 0x30)) };
    unsafe { f(p, n, v) }
}
unsafe extern "C" fn vt_set_resource(p: Parameter, n: *const c_char, v: *mut c_void) {
    let f: unsafe extern "C" fn(Parameter, *const c_char, *mut c_void) = unsafe { core::mem::transmute(slot(p, 0x08)) };
    unsafe { f(p, n, v) }
}
unsafe extern "C" fn vt_set_pointer(p: Parameter, n: *const c_char, v: *mut c_void) {
    let f: unsafe extern "C" fn(Parameter, *const c_char, *mut c_void) = unsafe { core::mem::transmute(slot(p, 0x00)) };
    unsafe { f(p, n, v) }
}
unsafe extern "C" fn vt_get_i(p: Parameter, n: *const c_char, out: *mut i32) -> Result {
    let f: unsafe extern "C" fn(Parameter, *const c_char, *mut i32) -> Result = unsafe { core::mem::transmute(slot(p, 0x60)) };
    unsafe { f(p, n, out) }
}
unsafe extern "C" fn vt_get_ui(p: Parameter, n: *const c_char, out: *mut u32) -> Result {
    let f: unsafe extern "C" fn(Parameter, *const c_char, *mut u32) -> Result = unsafe { core::mem::transmute(slot(p, 0x58)) };
    unsafe { f(p, n, out) }
}
unsafe extern "C" fn vt_get_pointer(p: Parameter, n: *const c_char, out: *mut *mut c_void) -> Result {
    let f: unsafe extern "C" fn(Parameter, *const c_char, *mut *mut c_void) -> Result = unsafe { core::mem::transmute(slot(p, 0x40)) };
    unsafe { f(p, n, out) }
}
/// The static loader's Init_with_ProjectID on the core's own export, whose version comes before the feature info (as
/// NVSDK_NGX_D3D12_Init_Ext's does).
unsafe extern "C" fn driver_init(
    project: *const c_char,
    engine: i32,
    engine_version: *const c_char,
    path: *const u16,
    device: *mut c_void,
    info: *const FeatureCommonInfo,
    version: i32,
) -> Result {
    type InitProjectId = unsafe extern "C" fn(*const c_char, i32, *const c_char, *const u16, *mut c_void, i32, *const FeatureCommonInfo) -> Result;
    let f = DRIVER_INIT.load(Ordering::Acquire);
    if f.is_null() {
        return RESULT_FAIL;
    }
    // SAFETY: the core's export of that signature.
    unsafe { core::mem::transmute::<*mut c_void, InitProjectId>(f)(project, engine, engine_version, path, device, version, info) }
}
/// The table on the driver's NGX core once the game has loaded it (None before).
fn driver_table() -> Option<&'static Ngx> {
    if let Some(t) = DRIVER.get() {
        return Some(t);
    }
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    // SAFETY: module and export lookups.
    unsafe {
        let core = [windows::core::w!("_nvngx.dll"), windows::core::w!("nvngx.dll")]
            .iter()
            .find_map(|&name| GetModuleHandleW(name).ok())?;
        let export = |name: &CStr| GetProcAddress(core, windows::core::PCSTR(name.as_ptr().cast())).map_or(0usize, |f| f as usize);
        let names = [
            c"NVSDK_NGX_D3D12_Init_ProjectID",
            c"NVSDK_NGX_D3D12_GetCapabilityParameters",
            c"NVSDK_NGX_D3D12_AllocateParameters",
            c"NVSDK_NGX_D3D12_DestroyParameters",
            c"NVSDK_NGX_D3D12_CreateFeature",
            c"NVSDK_NGX_D3D12_ReleaseFeature",
            c"NVSDK_NGX_D3D12_EvaluateFeature",
        ];
        let f = names.map(export);
        if f.iter().any(|&a| a == 0) {
            return None;
        }
        DRIVER_INIT.store(f[0] as *mut c_void, Ordering::Release);
        let table = Ngx {
            init: driver_init,
            capability: core::mem::transmute::<usize, unsafe extern "C" fn(*mut Parameter) -> Result>(f[1]),
            allocate: core::mem::transmute::<usize, unsafe extern "C" fn(*mut Parameter) -> Result>(f[2]),
            destroy: core::mem::transmute::<usize, unsafe extern "C" fn(Parameter) -> Result>(f[3]),
            create: core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void, i32, Parameter, *mut *mut c_void) -> Result>(f[4]),
            release: core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void) -> Result>(f[5]),
            evaluate: core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void, *const c_void, Parameter, *const c_void) -> Result>(f[6]),
            set_ui: vt_set_ui,
            set_f: vt_set_f,
            set_resource: vt_set_resource,
            set_pointer: vt_set_pointer,
            get_i: vt_get_i,
            get_ui: vt_get_ui,
            get_pointer: vt_get_pointer,
        };
        Some(DRIVER.get_or_init(|| table))
    }
}

/// NVSDK_NGX_FeatureCommonInfo.
#[repr(C)]
pub struct FeatureCommonInfo {
    path: *const *const u16,
    path_count: u32,
    internal: *mut c_void,
    logging: Option<unsafe extern "C" fn(*const c_char, i32, i32)>,
    minimum_logging: i32,
    disable_other_sinks: bool,
}
impl FeatureCommonInfo {
    pub fn new(path: *const *const u16, path_count: u32, logging: Option<unsafe extern "C" fn(*const c_char, i32, i32)>, minimum_logging: i32) -> Self {
        FeatureCommonInfo { path, path_count, internal: null_mut(), logging, minimum_logging, disable_other_sinks: false }
    }
}
const _: () = assert!(core::mem::size_of::<FeatureCommonInfo>() == 40);

/// NVSDK_NGX_ENGINE_TYPE_CUSTOM, NVSDK_NGX_Version_API, NVSDK_NGX_Feature_FrameGeneration, the logging levels.
const ENGINE_CUSTOM: i32 = 0;
const VERSION_API: i32 = 0x15;
const FEATURE_FRAME_GENERATION: i32 = 11;
const LOGGING_OFF: i32 = 0;
const LOGGING_ON: i32 = 1;
/// NVSDK_NGX_Result_Fail | 12.
const RESULT_FAIL: u32 = 0xBAD0_0000;
const FAIL_OUT_OF_DATE: u32 = RESULT_FAIL | 12;
fn failed(result: Result) -> bool {
    result & 0xFFF0_0000 == RESULT_FAIL
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
}

fn log(text: &str) {
    cvr_host::log::write(text);
}
/// NgxLog: NGX's own messages.
unsafe extern "C" fn ngx_log(message: *const c_char, _: i32, _: i32) {
    if !message.is_null() {
        // SAFETY: NGX's NUL-terminated message.
        log(&format!("[ngx] {}", unsafe { CStr::from_ptr(message) }.to_string_lossy()));
    }
}

/// Directory: the module's folder ("." without a separator).
fn directory(module: windows::Win32::Foundation::HMODULE) -> Vec<u16> {
    let mut path = [0u16; 260];
    // SAFETY: a local buffer.
    let n = unsafe { windows::Win32::System::LibraryLoader::GetModuleFileNameW(Some(module), &mut path) } as usize;
    let text = &path[..n.min(path.len())];
    let mut out = match text.iter().rposition(|&c| c == u16::from(b'\\') || c == u16::from(b'/')) {
        Some(slash) => text[..slash].to_vec(),
        None => vec![u16::from(b'.')],
    };
    out.push(0);
    out
}

struct State {
    initialized: usize,
    available: bool,
    /// Why DLSS-G is not available (empty when it is).
    reason: &'static str,
    /// An initialisation failure's text.
    failure: String,
}
const NOT_BUILT: &str = "NVIDIA DLSS SDK was not built into this plugin";
static STATE: Mutex<State> = Mutex::new(State { initialized: 0, available: false, reason: NOT_BUILT, failure: String::new() });
thread_local! {
    static REASON: core::cell::RefCell<std::ffi::CString> = core::cell::RefCell::new(std::ffi::CString::default());
}
/// Reason: why DLSS-G (or ANR FG with PROVIDER_ANR) is not available, a copy valid on this thread until its next call.
pub fn reason_of(provider: u8) -> *const c_char {
    let text = if provider == PROVIDER_ANR {
        Ok(crate::anr_fg::reason())
    } else {
        STATE.lock().map(|s| if s.reason.is_empty() && !s.failure.is_empty() { s.failure.clone() } else { s.reason.to_owned() })
    };
    let text = std::ffi::CString::new(text.unwrap_or_default()).unwrap_or_default();
    REASON.with(|copy| {
        *copy.borrow_mut() = text;
        copy.borrow().as_ptr()
    })
}

/// Available: DLSS-G on this device (NGX initialised and its capability read on a new device).
///
/// # Safety
/// `device` is a live device or null.
pub unsafe fn available(device: *mut c_void, provider: u8) -> bool {
    if provider == PROVIDER_ANR {
        // SAFETY: as documented.
        return unsafe { crate::anr_fg::available(device) };
    }
    let Some(n) = ngx() else {
        if let Ok(mut s) = STATE.lock() {
            s.reason = "NVIDIA's NGX core is not loaded (the game's DLSS is off)";
        }
        return false;
    };
    let Ok(mut s) = STATE.lock() else { return false };
    if device.is_null() {
        return false;
    }
    if device as usize == s.initialized {
        return s.available;
    }
    s.initialized = device as usize;
    s.available = false;
    // NGX is one instance per adapter for the whole process, shared with the game's own DLSS.
    // It is never shut down here: NVSDK_NGX_D3D12_Shutdown1 would take the game's features down too.
    let mut module = windows::Win32::Foundation::HMODULE::default();
    // SAFETY: this module's own address.
    let _ = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleExW(
            windows::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | windows::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::PCWSTR(available as *const () as *const u16),
            &mut module,
        )
    };
    // nvngx_dlssg.dll ships with the game next to its executable; the plugin folder is searched too.
    let (game, plugin) = (directory(windows::Win32::Foundation::HMODULE::default()), directory(module));
    let paths = [game.as_ptr(), plugin.as_ptr()];
    // SAFETY: the exported switch.
    let diagnostics = unsafe { CyberpunkVR_RuntimeDiagnostics } != 0;
    let info = FeatureCommonInfo {
        path: paths.as_ptr(),
        path_count: 2,
        internal: null_mut(),
        logging: Some(ngx_log),
        minimum_logging: if diagnostics { LOGGING_ON } else { LOGGING_OFF },
        disable_other_sinks: false,
    };
    // NGX writes its logs to the application data path: the plugin's log folder.
    let mut logs: Vec<u16> = cvr_host::paths::dir(cvr_host::paths::Folder::Log).as_os_str().encode_wide_vec();
    logs.push(0);
    // SAFETY: NUL-terminated strings, the live device and the info above.
    let result = unsafe {
        (n.init)(c"5f0c9a3e-7d24-4b8e-9c61-2a7e8d4f1b30".as_ptr(), ENGINE_CUSTOM, c"CyberpunkVR".as_ptr(), logs.as_ptr(), device, &info, VERSION_API)
    };
    if failed(result) {
        s.failure = format!("NGX initialization failed (0x{result:08X})");
        s.reason = "";
        log(&format!("[framegen] DLSS-G: {}\n", s.failure));
        return false;
    }
    let mut caps: Parameter = null_mut();
    // SAFETY: an output parameter.
    if failed(unsafe { (n.capability)(&mut caps) }) || caps.is_null() {
        s.reason = "NGX capability query failed";
        return false;
    }
    let (mut supported, mut needs_driver, mut init_result) = (0i32, 0i32, 0i32);
    let (mut frames, mut major, mut minor) = (0u32, 0u32, 0u32);
    // SAFETY: the capability parameters and local outputs.
    unsafe {
        (n.get_i)(caps, c"FrameGeneration.Available".as_ptr(), &mut supported);
        (n.get_i)(caps, c"FrameGeneration.NeedsUpdatedDriver".as_ptr(), &mut needs_driver);
        (n.get_i)(caps, c"FrameGeneration.FeatureInitResult".as_ptr(), &mut init_result);
        (n.get_ui)(caps, c"DLSSG.MultiFrameCountMax".as_ptr(), &mut frames);
        (n.get_ui)(caps, c"FrameGeneration.MinDriverVersionMajor".as_ptr(), &mut major);
        (n.get_ui)(caps, c"FrameGeneration.MinDriverVersionMinor".as_ptr(), &mut minor);
        (n.destroy)(caps);
    }
    log(&format!(
        "[framegen] DLSS-G: available={supported} frames={frames} init=0x{:08X} needsDriver={needs_driver} minDriver={major}.{minor}\n",
        init_result as u32
    ));
    if supported == 0 {
        s.reason = if needs_driver != 0 {
            "DLSS Frame Generation needs a newer NVIDIA driver"
        } else {
            "DLSS Frame Generation is not supported on this GPU (RTX 40/50 required)"
        };
        return false;
    }
    s.available = true;
    s.reason = "";
    s.failure.clear();
    true
}

trait EncodeWide {
    fn encode_wide_vec(&self) -> Vec<u16>;
}
impl EncodeWide for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        std::os::windows::ffi::OsStrExt::encode_wide(self).collect()
    }
}

/// A feature: its handle, its parameters and its VRAM estimate.
struct Feature {
    handle: *mut c_void,
    parameters: Parameter,
    bytes: u64,
    provider: u8,
}
fn delete(n: &Ngx, feature: Box<Feature>) {
    // SAFETY: the feature's own handle and parameters.
    unsafe {
        if !feature.handle.is_null() {
            (n.release)(feature.handle);
        }
        if !feature.parameters.is_null() {
            (n.destroy)(feature.parameters);
        }
    }
}
fn set_ui(n: &Ngx, p: Parameter, name: &CStr, value: u32) {
    // SAFETY: live parameters and a NUL-terminated name.
    unsafe { (n.set_ui)(p, name.as_ptr(), value) };
}
fn set_f(n: &Ngx, p: Parameter, name: &CStr, value: f32) {
    // SAFETY: as set_ui.
    unsafe { (n.set_f)(p, name.as_ptr(), value) };
}
fn set_resource(n: &Ngx, p: Parameter, name: &CStr, value: *mut c_void) {
    // SAFETY: as set_ui; a live resource or null.
    unsafe { (n.set_resource)(p, name.as_ptr(), value) };
}
fn set_pointer(n: &Ngx, p: Parameter, name: &CStr, value: *const c_void) {
    // SAFETY: as set_ui; the value outlives the evaluation.
    unsafe { (n.set_pointer)(p, name.as_ptr(), value.cast_mut()) };
}

/// Create: the eye's feature (NGX's creation work recorded into the list; usable once that list executed), null on a
/// failure.
///
/// # Safety
/// `list` is a live command list.
#[allow(clippy::too_many_arguments)]
pub unsafe fn create(
    list: *mut c_void,
    width: u32,
    height: u32,
    render_width: u32,
    render_height: u32,
    backbuffer: DXGI_FORMAT,
    motion: DXGI_FORMAT,
    depth: DXGI_FORMAT,
    provider: u8,
) -> *mut c_void {
    let Some(n) = table(provider) else { return null_mut() };
    let anr = provider == PROVIDER_ANR;
    let name = if anr { "ANR FG" } else { "DLSS-G" };
    let mut feature = Box::new(Feature { handle: null_mut(), parameters: null_mut(), bytes: 0, provider });
    // SAFETY: an output parameter.
    if failed(unsafe { (n.allocate)(&mut feature.parameters) }) {
        delete(n, feature);
        return null_mut();
    }
    // NGX_D3D12_CREATE_DLSSG: nodes 1 and 1, no dynamic resolution.
    let p = feature.parameters;
    set_ui(n, p, c"CreationNodeMask", 1);
    set_ui(n, p, c"VisibilityNodeMask", 1);
    set_ui(n, p, c"Width", width);
    set_ui(n, p, c"Height", height);
    set_ui(n, p, c"DLSSG.BackbufferFormat", backbuffer.0 as u32);
    set_ui(n, p, c"DLSSG.InternalWidth", render_width);
    set_ui(n, p, c"DLSSG.InternalHeight", render_height);
    set_ui(n, p, c"DLSSG.DynamicResolution", 0);
    // SAFETY: the live list, the parameters and the output handle.
    let result = unsafe { (n.create)(list, FEATURE_FRAME_GENERATION, p, &mut feature.handle) };
    if failed(result) || feature.handle.is_null() {
        log(&format!(
            "[framegen] {name}: feature {width}x{height} (render {render_width}x{render_height}) creation failed 0x{result:08X}\n"
        ));
        feature.handle = null_mut();
        delete(n, feature);
        return null_mut();
    }
    if anr {
        log(&format!("[framegen] ANR FG: feature {width}x{height} render {render_width}x{render_height} created\n"));
        return Box::into_raw(feature).cast();
    }
    let mut caps: Parameter = null_mut();
    // SAFETY: an output parameter.
    if !failed(unsafe { (n.capability)(&mut caps) }) && !caps.is_null() {
        let mut bytes = 0usize;
        if !failed(estimate_vram(n, caps, render_width, render_height, width, height, backbuffer, motion, depth, &mut bytes)) {
            feature.bytes = bytes as u64;
        }
        // SAFETY: the capability parameters.
        unsafe { (n.destroy)(caps) };
    }
    log(&format!(
        "[framegen] DLSS-G: feature {width}x{height} render {render_width}x{render_height} created, estimated {:.1} MiB\n",
        feature.bytes as f64 / 1048576.0
    ));
    Box::into_raw(feature).cast()
}
type EstimateVram = unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, *mut usize) -> Result;
/// NGX_D3D12_ESTIMATE_VRAM_DLSSG (no HUD-less or UI buffers).
#[allow(clippy::too_many_arguments)]
fn estimate_vram(
    n: &Ngx,
    caps: Parameter,
    mvec_depth_width: u32,
    mvec_depth_height: u32,
    color_width: u32,
    color_height: u32,
    color: DXGI_FORMAT,
    motion: DXGI_FORMAT,
    depth: DXGI_FORMAT,
    bytes: &mut usize,
) -> Result {
    let mut callback: *mut c_void = null_mut();
    // SAFETY: the capability parameters and a local output.
    unsafe { (n.get_pointer)(caps, c"DLSSG.EstimateVRAMCallback".as_ptr(), &mut callback) };
    if callback.is_null() {
        return FAIL_OUT_OF_DATE;
    }
    // SAFETY: NGX's callback of this type.
    let callback: EstimateVram = unsafe { core::mem::transmute::<*mut c_void, EstimateVram>(callback) };
    // SAFETY: as documented by the SDK; a local output.
    let result = unsafe {
        callback(mvec_depth_width, mvec_depth_height, color_width, color_height, color.0 as u32, motion.0 as u32, depth.0 as u32, 0, 0, bytes)
    };
    if failed(result) {
        result
    } else {
        1
    }
}
/// FeatureDeleter.
///
/// # Safety
/// `feature` is create's or null.
pub unsafe fn free(feature: *mut c_void) {
    if feature.is_null() {
        return;
    }
    // SAFETY: as documented.
    let feature = unsafe { Box::from_raw(feature.cast::<Feature>()) };
    match table(feature.provider) {
        Some(n) => delete(n, feature),
        None => drop(feature),
    }
}
/// Bytes: the feature's VRAM estimate (0 for null).
///
/// # Safety
/// As free.
pub unsafe fn bytes(feature: *const c_void) -> u64 {
    // SAFETY: as documented.
    unsafe { feature.cast::<Feature>().as_ref() }.map_or(0, |f| f.bytes)
}

/// Evaluate: the interpolated frame into `output` (inputs in NON_PIXEL_SHADER_RESOURCE, output in UNORDERED_ACCESS;
/// NGX binds its own heaps and root signature, so the caller re-binds its own after this).
///
/// # Safety
/// `feature` is create's or null; `list` a live list; `e` the generator's evaluation or null.
pub(crate) unsafe fn evaluate(feature: *mut c_void, list: *mut c_void, e: *const DlssEvaluation) -> bool {
    // SAFETY: as documented.
    let (Some(feature), Some(e)) = (unsafe { feature.cast::<Feature>().as_ref() }, unsafe { e.as_ref() }) else { return false };
    let Some(n) = table(feature.provider) else { return false };
    if feature.handle.is_null() || e.camera.is_null() {
        return false;
    }
    // SAFETY: the generator's cameras.
    let (camera, previous): (&CameraData, Option<&CameraData>) = unsafe { (&*e.camera, if e.reset { None } else { e.previous.as_ref() }) };
    let mut matrices = Matrices { view_to_clip: [0.0; 16], clip_to_view: [0.0; 16], clip_to_prev_clip: [0.0; 16], prev_clip_to_clip: [0.0; 16] };
    if !dlssg_matrices::build(camera, previous, &mut matrices) {
        return false;
    }
    // The optional evaluation parameters (their defaults: one frame at index 1, 40 for the depth separation).
    let clip_to_lens = dlssg_matrices::identity();
    let c = camera;
    let jitter = if c.jittered { c.jitter } else { [0.0; 2] };
    // Raw engine vector * motionScale is a fraction of the image; NGX wants pixels of the motion grid.
    let mvec_scale = [c.motion_scale[0] * e.motion_width as f32, c.motion_scale[1] * e.motion_height as f32];
    let p = feature.parameters;
    // NGX_D3D12_EVALUATE_DLSSG's sets.
    set_resource(n, p, c"DLSSG.Backbuffer", e.backbuffer);
    set_resource(n, p, c"DLSSG.MVecs", e.motion);
    set_resource(n, p, c"DLSSG.Depth", e.depth);
    set_resource(n, p, c"DLSSG.HUDLess", null_mut());
    set_resource(n, p, c"DLSSG.UI", null_mut());
    set_resource(n, p, c"DLSSG.UIAlpha", null_mut());
    set_resource(n, p, c"DLSSG.BidirectionalDistortionField", null_mut());
    set_resource(n, p, c"DLSSG.OutputInterpolated", e.output);
    set_resource(n, p, c"DLSSG.OutputReal", null_mut());
    set_resource(n, p, c"DLSSG.OutputDisableInterpolation", null_mut());
    set_ui(n, p, c"DLSSG.MultiFrameCount", 1);
    set_ui(n, p, c"DLSSG.MultiFrameIndex", 1);
    set_pointer(n, p, c"DLSSG.CameraViewToClip", matrices.view_to_clip.as_ptr().cast());
    set_pointer(n, p, c"DLSSG.ClipToCameraView", matrices.clip_to_view.as_ptr().cast());
    set_pointer(n, p, c"DLSSG.ClipToLensClip", clip_to_lens.as_ptr().cast());
    set_pointer(n, p, c"DLSSG.ClipToPrevClip", matrices.clip_to_prev_clip.as_ptr().cast());
    set_pointer(n, p, c"DLSSG.PrevClipToClip", matrices.prev_clip_to_clip.as_ptr().cast());
    set_f(n, p, c"DLSSG.JitterOffsetX", jitter[0]);
    set_f(n, p, c"DLSSG.JitterOffsetY", jitter[1]);
    set_f(n, p, c"DLSSG.MvecScaleX", mvec_scale[0]);
    set_f(n, p, c"DLSSG.MvecScaleY", mvec_scale[1]);
    set_f(n, p, c"DLSSG.CameraPinholeOffsetX", 0.0);
    set_f(n, p, c"DLSSG.CameraPinholeOffsetY", 0.0);
    set_f(n, p, c"DLSSG.CameraPosX", c.position[0]);
    set_f(n, p, c"DLSSG.CameraPosY", c.position[1]);
    set_f(n, p, c"DLSSG.CameraPosZ", c.position[2]);
    set_f(n, p, c"DLSSG.CameraUpX", c.up[0]);
    set_f(n, p, c"DLSSG.CameraUpY", c.up[1]);
    set_f(n, p, c"DLSSG.CameraUpZ", c.up[2]);
    set_f(n, p, c"DLSSG.CameraRightX", c.right[0]);
    set_f(n, p, c"DLSSG.CameraRightY", c.right[1]);
    set_f(n, p, c"DLSSG.CameraRightZ", c.right[2]);
    set_f(n, p, c"DLSSG.CameraFwdX", c.forward[0]);
    set_f(n, p, c"DLSSG.CameraFwdY", c.forward[1]);
    set_f(n, p, c"DLSSG.CameraFwdZ", c.forward[2]);
    set_f(n, p, c"DLSSG.CameraNear", c.near_plane);
    set_f(n, p, c"DLSSG.CameraFar", c.far_plane);
    set_f(n, p, c"DLSSG.CameraFOV", c.vertical_fov);
    set_f(n, p, c"DLSSG.CameraAspectRatio", c.aspect);
    set_ui(n, p, c"DLSSG.ColorBuffersHDR", 0);
    set_ui(n, p, c"DLSSG.DepthInverted", u32::from(c.inverted));
    set_ui(n, p, c"DLSSG.CameraMotionIncluded", 1);
    set_ui(n, p, c"DLSSG.Reset", u32::from(e.reset));
    set_ui(n, p, c"DLSSG.AutomodeOverrideReset", 0);
    set_ui(n, p, c"DLSSG.NotRenderingGameFrames", 0);
    set_ui(n, p, c"DLSSG.OrthoProjection", 0);
    set_f(n, p, c"DLSSG.MvecInvalidValue", f32::MAX);
    set_ui(n, p, c"DLSSG.MvecDilated", 0);
    set_ui(n, p, c"DLSSG.MenuDetectionEnabled", 0);
    for (name, value) in [
        (c"DLSSG.MVecsSubrectBaseX", 0),
        (c"DLSSG.MVecsSubrectBaseY", 0),
        (c"DLSSG.MVecsSubrectWidth", e.motion_width),
        (c"DLSSG.MVecsSubrectHeight", e.motion_height),
        (c"DLSSG.DepthSubrectBaseX", 0),
        (c"DLSSG.DepthSubrectBaseY", 0),
        (c"DLSSG.DepthSubrectWidth", e.depth_width),
        (c"DLSSG.DepthSubrectHeight", e.depth_height),
        (c"DLSSG.HUDLessSubrectBaseX", 0),
        (c"DLSSG.HUDLessSubrectBaseY", 0),
        (c"DLSSG.HUDLessSubrectWidth", 0),
        (c"DLSSG.HUDLessSubrectHeight", 0),
        (c"DLSSG.UISubrectBaseX", 0),
        (c"DLSSG.UISubrectBaseY", 0),
        (c"DLSSG.UISubrectWidth", 0),
        (c"DLSSG.UISubrectHeight", 0),
        (c"DLSSG.UIAlphaSubrectBaseX", 0),
        (c"DLSSG.UIAlphaSubrectBaseY", 0),
        (c"DLSSG.UIAlphaSubrectWidth", 0),
        (c"DLSSG.UIAlphaSubrectHeight", 0),
        (c"DLSSG.BidirectionalDistortionFieldSubrectBaseX", 0),
        (c"DLSSG.BidirectionalDistortionFieldSubrectBaseY", 0),
        (c"DLSSG.BidirectionalDistortionFieldSubrectWidth", 0),
        (c"DLSSG.BidirectionalDistortionFieldSubrectHeight", 0),
        (c"DLSSG.BidirectionalDistortionFieldLowPrecision.IsLowPrecision", 0),
    ] {
        set_ui(n, p, name, value);
    }
    set_f(n, p, c"DLSSG.BidirectionalDistortionFieldLowPrecision.Bias", 0.0);
    set_f(n, p, c"DLSSG.BidirectionalDistortionFieldLowPrecision.Scale", 0.0);
    set_f(n, p, c"DLSSG.MinRelativeLinearDepthObjectSeparation", 40.0);
    for (name, value) in [
        (c"DLSSG.InputBackbufferSubrectBaseX", 0),
        (c"DLSSG.InputBackbufferSubrectBaseY", 0),
        (c"DLSSG.InputBackbufferSubrectWidth", e.width),
        (c"DLSSG.InputBackbufferSubrectHeight", e.height),
        (c"DLSSG.OutputInterpolatedSubrectBaseX", 0),
        (c"DLSSG.OutputInterpolatedSubrectBaseY", 0),
        (c"DLSSG.OutputInterpolatedSubrectWidth", e.width),
        (c"DLSSG.OutputInterpolatedSubrectHeight", e.height),
        (c"DLSSG.OutputRealSubrectBaseX", 0),
        (c"DLSSG.OutputRealSubrectBaseY", 0),
        (c"DLSSG.OutputRealSubrectWidth", 0),
        (c"DLSSG.OutputRealSubrectHeight", 0),
    ] {
        set_ui(n, p, name, value);
    }
    if feature.provider == PROVIDER_ANR {
        // What OptiScaler's provider proxy sets before every evaluation (Nvngx_DllProxy, its defaults).
        // SAFETY: the table is ours.
        unsafe { crate::anr_fg::set_int(p, c"DLSSG.ShowDebug", 0) };
        set_ui(n, p, c"DLSSG.DispatchFlags", 0x1000_0000); // IGNORE_UI_TEXTURE
    }
    // SAFETY: the live list, the feature's handle and parameters; no progress callback.
    let result = unsafe { (n.evaluate)(list, feature.handle, p, null()) };
    if failed(result) {
        static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        if LOGGED.fetch_add(1, Ordering::SeqCst) < 4 {
            let name = if feature.provider == PROVIDER_ANR { "ANR FG" } else { "DLSS-G" };
            log(&format!("[framegen] {name}: evaluate failed 0x{result:08X}\n"));
        }
        return false;
    }
    true
}
