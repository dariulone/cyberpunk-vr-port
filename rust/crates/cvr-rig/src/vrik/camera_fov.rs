//! CameraFov (formerly src/Hooks/CameraFov.cpp): the camera's field of view, written where the engine stores it
//! (`movss [rcx+410h],xmm3`). The engine's field is VERTICAL and the horizontal derives from the render aspect, so the
//! value aimed at is the horizontal the submit uses -- the panel-covering span of the two eyes, xr_force_fov, or under
//! asymmetric projection the main eye's window -- and the vertical that makes the engine derive exactly it is written.
//! The stub calls the callback (rcx = the camera state, xmm1 = the engine's value), then loads the override into xmm3
//! before the original store. The registration (Boot, 35), the debug globals of C++ linkage and the OpenXR getters'
//! services stay in src/Hooks/CameraFov.cpp.
use cvr_body::cmath::{atan, tan};
use cvr_crt::{format as cformat, Arg};
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use std::sync::atomic::{AtomicU64, Ordering};

/// cvr::projection::EyeWindow (tangents) and EngineProjection.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EyeWindow {
    l: f32,
    r: f32,
    d: f32,
    u: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EngineProjection {
    vfov_deg: f32,
    aspect: f32,
    shift_x: f32,
    shift_y: f32,
}
/// XrFovf.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Fov {
    left: f32,
    right: f32,
    up: f32,
    down: f32,
}

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_normalFovOverrideValue: f32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_AsymProjection: i32;
    fn GetForcedFov() -> f32;
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_launcher_size(width: *mut i32, height: *mut i32);
    fn cvr_cpp_xr_asym_eye_windows(out: *mut [EyeWindow; 2]) -> bool;
    fn cvr_cpp_xr_runtime_hfov() -> f32;
    fn cvr_cpp_xr_runtime_ipd() -> f32;
    /// The C++-linkage diagnostics the hub prints (src/Hooks/CameraFov.cpp).
    fn cvr_cpp_fov_publish(camera_state: usize, original: f32, horizontal: f32, lod: f32);
    /// OpenXRManager::GetCurrentEyeFov (cvr-xr views, through the C++ half).
    fn cvr_cpp_xr_eye_fov(eye: i32, out: *mut Fov) -> bool;
    fn cvr_projection_to_engine(w: *const EyeWindow, out: *mut EngineProjection);
    fn cvr_panel_covering_horizontal_fov_deg(left: *const Fov, right: *const Fov, aspect: f32) -> f32;
}

static HITS: AtomicU64 = AtomicU64::new(0);
static FOV_LOG: AtomicU64 = AtomicU64::new(0);
const D2R: f32 = 3.141_592_653_5_f32 / 180.0;

fn f(v: f32, precision: &core::ffi::CStr) -> String {
    cformat(precision, &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}

/// rcx = the camera state, xmm1 = the engine's own (vertical) fov.
#[no_mangle]
pub unsafe extern "C" fn OnNormalFovHookCallback(camera_state: usize, original_fov: f32) {
    let hits = HITS.fetch_add(1, Ordering::Relaxed) + 1;
    let forced = GetForcedFov();
    let (mut lw, mut lh) = (0i32, 0i32);
    cvr_cpp_launcher_size(&mut lw, &mut lh);
    let aspect = if lw > 1 && lh > 1 { lw as f32 / lh as f32 } else { 1.0 };
    // ASYMMETRIC PROJECTION: the vertical is the main eye's window span; each eye's aspect and shift on top of it.
    let mut windows = [EyeWindow::default(); 2];
    if CyberpunkVR_AsymProjection == 1 && !(forced > 1.0 && forced < 170.0) && cvr_cpp_xr_asym_eye_windows(&mut windows) {
        let main = if CyberpunkVR_MainIsRightEye != 0 { 1 } else { 0 };
        let mut p = EngineProjection::default();
        cvr_projection_to_engine(&windows[main], &mut p);
        if p.vfov_deg > 1.0 && p.vfov_deg < 179.0 {
            core::ptr::write_volatile(&raw mut g_normalFovOverrideValue, p.vfov_deg);
            let horizontal = 2.0 * atan(0.5 * (windows[main].r - windows[main].l)) / D2R;
            cvr_cpp_fov_publish(camera_state, original_fov, horizontal, p.vfov_deg + 30.0);
            return;
        }
    }
    let mut target_h = 0.0f32;
    if forced > 1.0 && forced < 170.0 {
        target_h = forced; // xr_force_fov has always meant the horizontal
    } else {
        let (mut lf, mut rf) = (Fov::default(), Fov::default());
        if cvr_cpp_xr_eye_fov(0, &mut lf) && cvr_cpp_xr_eye_fov(1, &mut rf) {
            // Size to COVER the panel (the submit recentres the frustum but never rotates the pose).
            target_h = cvr_panel_covering_horizontal_fov_deg(&lf, &rf, aspect);
        }
        if !(target_h > 1.0 && target_h < 170.0) {
            target_h = cvr_cpp_xr_runtime_hfov();
        }
    }
    let mut vertical = original_fov;
    let horizontal;
    if target_h > 1.0 && target_h < 170.0 && aspect > 0.05 && aspect < 20.0 {
        vertical = 2.0 * atan(tan(target_h * 0.5 * D2R) / aspect) / D2R;
        horizontal = target_h;
    } else {
        // Before XR is up: the engine's own value, and the horizontal it implies.
        horizontal = 2.0 * atan(tan(original_fov * 0.5 * D2R) * aspect) / D2R;
    }
    if !(vertical > 1.0 && vertical < 179.0) {
        vertical = original_fov;
    }
    core::ptr::write_volatile(&raw mut g_normalFovOverrideValue, vertical);
    if FOV_LOG.load(Ordering::Relaxed) < 20 {
        FOV_LOG.fetch_add(1, Ordering::Relaxed);
        super::env::log(&format!(
            "NormalFOV: original={} xr_force_fov={} targetH={} aspect={} ({}x{}) -> wroteV={} derivedH={}\n",
            f(original_fov, c"%.3f"),
            f(forced, c"%.3f"),
            f(target_h, c"%.3f"),
            f(aspect, c"%.5f"),
            lw,
            lh,
            f(vertical, c"%.3f"),
            f(horizontal, c"%.3f")
        ));
    }
    // LOD cone: a bit wider than the fov used (printed only: no LOD site reads it).
    let lod = vertical + 30.0;
    cvr_cpp_fov_publish(camera_state, original_fov, horizontal, lod);
    if cvr_cpp_verbose_log() != 0 && hits % 600 == 1 {
        let stored_h = read::<f32>(camera_state.wrapping_add(0x410)).unwrap_or(0.0);
        let stored_v = read::<f32>(camera_state.wrapping_add(0x414)).unwrap_or(0.0);
        super::env::log(&format!(
            "NormalFOV hook: state={:016X} original={} desired={} storedH={} storedV={} lodFov={} runtimeHFov={} runtimeIPD={}\n",
            camera_state,
            f(original_fov, c"%.6f"),
            f(vertical, c"%.6f"),
            f(stored_h, c"%.6f"),
            f(stored_v, c"%.6f"),
            f(lod, c"%.6f"),
            f(cvr_cpp_xr_runtime_hfov(), c"%.6f"),
            f(cvr_cpp_xr_runtime_ipd(), c"%.6f")
        ));
    }
}

/// movss [rcx+410h],xmm3 / mov rdx,[rcx+360h]: the store of the camera's fov.
const PATTERN: &[u8] = b"\xF3\x0F\x11\x99\x10\x04\x00\x00\x48\x8B\x91\x60\x03\x00\x00";
const MASK: &[u8] = b"xxxxxxxxxxxxxxx";
const REPLACE: usize = 15;

/// Installed from src/Hooks/CameraFov.cpp's registration (Boot, 35).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_camera_fov_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else {
        return false;
    };
    let Some(tramp) = allocate(found, 512) else { return false };
    let mut c = Code::new(tramp);
    c.volatile_call(4, &[0x48, 0x89, 0xC9, 0x0F, 0x28, 0xCB], OnNormalFovHookCallback as *const () as u64) // mov rcx,rcx / movaps xmm1,xmm3
        .mov_rax(&raw const g_normalFovOverrideValue as u64)
        .bytes(&[0xF3, 0x0F, 0x10, 0x18]) // movss xmm3,[rax]
        .bytes(PATTERN)
        .jmp(found + REPLACE);
    c.commit();
    patch_jump(found, REPLACE, tramp);
    true
}
