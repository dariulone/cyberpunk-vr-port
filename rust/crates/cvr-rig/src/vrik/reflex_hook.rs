//! NVIDIA Reflex (formerly src/Hooks/Reflex.cpp): the game's own slReflexSetOptions wrapper is detoured, the port's
//! latency mode goes into the game's options block while the XR session runs (`crate::reflex`), and on request the
//! Streamline latency report is collected right after the native call, on the same render thread and inside the
//! engine's Streamline critical section (GetState is not thread safe). No sampling thread, no waits.
use crate::reflex::{known_options, normalize_mode, override_options, Options, TimingState, DEFAULT_MODE};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use cvr_engine::hooks::{install_chain, Detour};
use std::fmt::Write as _;
use std::sync::Mutex;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetTickCount64() -> u64;
}

static SELECTED_MODE: AtomicI32 = AtomicI32::new(DEFAULT_MODE);
static APPLIED_MODE: AtomicI32 = AtomicI32::new(-2);
/// idle / requested / collecting.
static TIMING_REQUEST: AtomicU32 = AtomicU32::new(0);
static REPORT: Mutex<Option<String>> = Mutex::new(None);
const NOT_REQUESTED: &str = "{\"pending\":false,\"status\":\"not_requested\"}";

type SetOptionsFn = unsafe extern "C" fn(*const Options) -> i32;
static mut ORIG_SET_OPTIONS: *mut c_void = core::ptr::null_mut();

fn diagnostics() -> bool {
    // SAFETY: the C++ half's switch.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}

fn capture_timing() -> String {
    if !diagnostics() {
        return "{\"error\":\"diagnostics_disabled\"}".into();
    }
    type FeatureFn = unsafe extern "C" fn(u32, *const u8, *mut *mut c_void) -> i32;
    type StateFn = unsafe extern "C" fn(*mut TimingState) -> i32;
    let name: Vec<u16> = "sl.interposer.dll\0".encode_utf16().collect();
    // SAFETY: plain module and export lookups.
    let feature = unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        if module.is_null() {
            core::ptr::null_mut()
        } else {
            GetProcAddress(module, c"slGetFeatureFunction".as_ptr().cast())
        }
    };
    if feature.is_null() {
        return "{\"error\":\"streamline_unavailable\"}".into();
    }
    // SAFETY: slGetFeatureFunction's signature.
    let feature: FeatureFn = unsafe { core::mem::transmute::<*mut c_void, FeatureFn>(feature) };
    let mut state_fn: *mut c_void = core::ptr::null_mut();
    // SAFETY: Streamline writes the feature function (Reflex is feature 3).
    let lookup = unsafe { feature(3, c"slReflexGetState".as_ptr().cast(), &mut state_fn) };
    if lookup != 0 || state_fn.is_null() {
        return "{\"error\":\"reflex_unavailable\"}".into();
    }
    // SAFETY: slReflexGetState's signature.
    let state_fn: StateFn = unsafe { core::mem::transmute::<*mut c_void, StateFn>(state_fn) };
    let mut state = Box::<TimingState>::default();
    // SAFETY: a v1 state block Streamline fills.
    let result = unsafe { state_fn(&mut *state) };
    let mut out = String::new();
    // SAFETY: a plain clock read.
    let stamp = unsafe { GetTickCount64() };
    let _ = write!(
        out,
        "{{\"pending\":false,\"result\":{},\"available\":{},\"appliedMode\":{},\"stamp\":{},\"frames\":[",
        result,
        state.latency_report_available,
        APPLIED_MODE.load(Ordering::Relaxed),
        stamp
    );
    let mut first = true;
    if result == 0 && state.latency_report_available {
        for f in state.frames.iter().filter(|f| f.frame_id != 0) {
            if !first {
                out.push(',');
            }
            first = false;
            let _ = write!(
                out,
                "{{\"id\":{},\"input\":{},\"simStart\":{},\"simEnd\":{},\"renderStart\":{},\"renderEnd\":{},\"presentStart\":{},\"presentEnd\":{},\
                 \"driverStart\":{},\"driverEnd\":{},\"queueStart\":{},\"queueEnd\":{},\"gpuStart\":{},\"gpuEnd\":{},\"gpuActiveUs\":{},\"gpuFrameUs\":{}}}",
                f.frame_id,
                f.input,
                f.sim_start,
                f.sim_end,
                f.render_start,
                f.render_end,
                f.present_start,
                f.present_end,
                f.driver_start,
                f.driver_end,
                f.queue_start,
                f.queue_end,
                f.gpu_start,
                f.gpu_end,
                f.gpu_active_us,
                f.gpu_frame_us
            );
        }
    }
    out.push_str("]}");
    out
}

unsafe extern "C" fn set_options(options: *const Options) -> i32 {
    let mode = SELECTED_MODE.load(Ordering::Relaxed);
    let source = options.as_ref();
    let known = source.is_some_and(known_options);
    let adjusted = source.filter(|_| known).and_then(|s| override_options(s, mode, super::xr_calls::session_running()));
    let submitted: *const Options = adjusted.as_ref().map_or(options, |a| a as *const Options);
    let original: SetOptionsFn = core::mem::transmute(ORIG_SET_OPTIONS);
    let result = original(submitted);
    if known && result == 0 {
        APPLIED_MODE.store((*submitted).mode, Ordering::Relaxed);
    }
    if TIMING_REQUEST.load(Ordering::Acquire) == 1 && TIMING_REQUEST.compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        let report = capture_timing();
        *REPORT.lock().unwrap_or_else(|p| p.into_inner()) = Some(report);
        TIMING_REQUEST.store(0, Ordering::Release);
    }
    result
}

/// Installed from src/Hooks/Reflex.cpp's registration (Boot, 89): the native slReflexSetOptions wrapper, refused on
/// any other code.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_reflex_install() -> bool {
    let detours = [Detour {
        name: "slReflexSetOptions wrapper",
        target: 0x22904f0,
        absolute: false,
        detour: set_options as *const c_void,
        original: core::ptr::addr_of_mut!(ORIG_SET_OPTIONS),
        expect: Some("40 53 48 83 EC 20 48 8B 05 63 42 0A 02 48 8B D9"),
    }];
    install_chain("nvidia-reflex", &detours).is_ok()
}

#[no_mangle]
pub extern "C" fn cvr_reflex_get_mode() -> i32 {
    SELECTED_MODE.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_reflex_set_mode(mode: i32) {
    SELECTED_MODE.store(normalize_mode(mode), Ordering::Relaxed);
}
#[no_mangle]
pub extern "C" fn cvr_reflex_get_applied_mode() -> i32 {
    APPLIED_MODE.load(Ordering::Relaxed)
}
/// TimingReport: the last report (`request`: ask the renderer's next SetOptions for a new one). The text is copied into
/// `out` by `sink`, the caller's string.
#[no_mangle]
pub unsafe extern "C" fn cvr_reflex_timing_report(request: bool, sink: unsafe extern "C" fn(*mut c_void, *const u8, usize), out: *mut c_void) {
    let text = if !diagnostics() {
        "{\"error\":\"diagnostics_disabled\"}".to_string()
    } else {
        let mut report = REPORT.lock().unwrap_or_else(|p| p.into_inner());
        if request && TIMING_REQUEST.load(Ordering::Acquire) == 0 {
            *report = Some("{\"pending\":true}".into());
            TIMING_REQUEST.store(1, Ordering::Release);
        }
        report.clone().unwrap_or_else(|| NOT_REQUESTED.into())
    };
    sink(out, text.as_ptr(), text.len());
}
