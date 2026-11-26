//! THE EYES' RENDERED WINDOWS AND THE OVERLAY'S PROJECTION WINDOW (formerly OpenXRManager::GetAsymmetricEyeWindows,
//! src/Runtimes/OpenXRManager.cpp, and overlay::GetOverlayProjWindow, src/Overlay/OverlayProjection.cpp): both eyes'
//! windows from the located views (or the sequential sync's eyes), under the views' lock; the window the overlay
//! projects through -- the main eye's asymmetric window when the projection is on, else the game's (or the runtime's)
//! horizontal FOV; and the services the crosshair, left-gun dot and ADS telemetry panels take from them.
#![allow(non_upper_case_globals)]
use core::sync::atomic::Ordering::Relaxed;

use cvr_body::xr::Quat;
use cvr_camera::overlay_projection::{cvr_ovp_project, cvr_ovp_window, Window};
use cvr_camera::projection::{windows, EyeWindow, Fov};
use cvr_xr::views::VIEWS;

extern "C" {
    static CyberpunkVR_AsymProjection: i32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_MainAdsZoomFactor: f32;
}
fn read<T: Copy>(v: *const T) -> T {
    // SAFETY: exports of the plugin, alive for the process.
    unsafe { core::ptr::read_volatile(v) }
}

/// cvr::projection::Active.
pub fn projection_active() -> bool {
    read(&raw const CyberpunkVR_AsymProjection) == 1
}

macro_rules! fov {
    ($f:expr) => {
        Fov { angle_left: $f.angle_left, angle_right: $f.angle_right, angle_up: $f.angle_up, angle_down: $f.angle_down }
    };
}
macro_rules! quat {
    ($q:expr) => {
        Quat { x: $q.x, y: $q.y, z: $q.z, w: $q.w }
    };
}

/// GetAsymmetricEyeWindows: the sequential sync's eyes when it runs and has a pose, else the located views (two at
/// least); None when the frusta are not usable.
pub fn eye_windows(fit_target: bool) -> Option<[EyeWindow; 2]> {
    let _held = VIEWS.lock.lock();
    let synced = crate::vr_core_settings::GetSyncSequential() != 0 && cvr_xr::tracking::synced_pose_valid();
    // SAFETY: read under the views' lock (m_viewMutex), as the C++ half does.
    let (count, views, poses, fovs) =
        unsafe { (*VIEWS.count.get(), &*VIEWS.views.get(), &*VIEWS.synced_poses.get(), &*VIEWS.synced_fovs.get()) };
    if !synced && count < 2 {
        return None;
    }
    let (fov, ori) = if synced {
        ([fov!(fovs[0]), fov!(fovs[1])], [quat!(poses[0].orientation), quat!(poses[1].orientation)])
    } else {
        ([fov!(views[0].fov), fov!(views[1].fov)], [quat!(views[0].pose.orientation), quat!(views[1].pose.orientation)])
    };
    let (guard, target) = crate::asym_projection::window_params(fit_target);
    windows(&fov, Some(&ori), guard, target)
}

/// OpenXRManager::GetAsymmetricEyeWindows for the C++ half: both windows into `out`, written only when usable.
///
/// # Safety
/// `out` is two writable windows or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_asym_eye_windows(out: *mut EyeWindow, fit_target: bool) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(w) = eye_windows(fit_target) else {
        return false;
    };
    // SAFETY: the caller's two windows.
    unsafe { core::ptr::copy_nonoverlapping(w.as_ptr(), out, 2) };
    true
}

/// GetOverlayProjWindow: the overlay's projection window for a display, None for one too small or no window.
pub fn overlay_window(width: f32, height: f32) -> Option<Window> {
    if width <= 1.0 || height <= 1.0 {
        return None;
    }
    // The main eye's window (left, right, up, down) when the projection is on and the frusta are usable.
    let asym = if projection_active() {
        eye_windows(true).map(|w| {
            let w = w[if read(&raw const CyberpunkVR_MainIsRightEye) != 0 { 1 } else { 0 }];
            [w.l, w.r, w.u, w.d]
        })
    } else {
        None
    };
    let eye: *const [f32; 4] = asym.as_ref().map_or(core::ptr::null(), |w| w);
    let game = if eye.is_null() { crate::vr_core_settings::GetGameRenderFovDeg() } else { 0.0 };
    let runtime = if !eye.is_null() || game > 1.0 { 0.0 } else { cvr_xr::runtime_info::RUNTIME.horizontal_fov_deg.load() };
    let mut out = Window::default();
    // SAFETY: the window (or null) and the result are locals.
    unsafe { cvr_ovp_window(width, height, eye, game, runtime, read(&raw const CyberpunkVR_MainAdsZoomFactor), &mut out) }
        .then_some(out)
}

/// GetOverlayProjWindow for the C++ half (its centre zeroing and early size check stay there).
///
/// # Safety
/// `out` is a writable window.
#[no_mangle]
pub unsafe extern "C" fn cvr_overlay_proj_window(width: f32, height: f32, out: *mut Window) -> bool {
    let Some(w) = overlay_window(width, height) else {
        return false;
    };
    // SAFETY: the caller's window.
    unsafe { *out = w };
    true
}

// ---- the panels' services (formerly the C++ tables' lambdas)
/// OpenXRManager::GetSharedSlot: the shared slots' block the present keeps, 0 outside it.
pub extern "C" fn shared_slot(i: i32) -> f32 {
    let p = cvr_xr::switches::SWITCHES.shared_hands.load(Relaxed);
    if p.is_null() || !(0..256).contains(&i) {
        return 0.0;
    }
    // SAFETY: the present's 256-slot block.
    unsafe { *p.add(i as usize) }
}
/// engine_stereo::Enabled.
pub extern "C" fn engine_stereo() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
}
/// engine_stereo::OnePicture: engine stereo showing one picture (not rendering, or the loading screen).
pub extern "C" fn one_picture() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
        && (!crate::engine_stereo::cvr_engine_stereo_rendering() || crate::engine_stereo::cvr_engine_stereo_loading_shown())
}
/// projection::Active() && GetAsymmetricEyeWindows (fitting the target).
pub extern "C" fn asym_windows(out: *mut [EyeWindow; 2]) -> bool {
    if !projection_active() {
        return false;
    }
    let Some(w) = eye_windows(true) else {
        return false;
    };
    // SAFETY: the caller's two windows.
    unsafe { *out = w };
    true
}
/// ProjectHeadSpacePointToScreen into two floats, written only when it projects.
pub extern "C" fn project(x: f32, y: f32, z: f32, width: f32, height: f32, out: *mut [f32; 2]) -> bool {
    let Some(w) = overlay_window(width, height) else {
        return false;
    };
    let mut screen = [0.0f32; 2];
    // SAFETY: locals.
    if !unsafe { cvr_ovp_project(false, core::ptr::null(), x, y, z, width, height, &w, screen.as_mut_ptr()) } {
        return false;
    }
    // SAFETY: the caller's two floats.
    unsafe { *out = screen };
    true
}
/// GetOverlayProjTans.
pub extern "C" fn proj_tans(width: f32, height: f32, tan_x: *mut f32, tan_y: *mut f32) -> bool {
    let Some(w) = overlay_window(width, height) else {
        return false;
    };
    // SAFETY: the caller's two floats.
    unsafe {
        *tan_x = w.tan_x;
        *tan_y = w.tan_y;
    }
    true
}
/// OpenXRManager::GetRuntimeIpd.
pub extern "C" fn runtime_ipd() -> f32 {
    cvr_xr::runtime_info::RUNTIME.ipd.load()
}

// ---- the overlay's own switches (ImGuiOverlay.cpp's)
extern "C" {
    #[link_name = "?g_drawBarrelCross@overlay@@3_NA"]
    pub static g_drawBarrelCross: bool;
    #[link_name = "?g_showCompactAdsTelemetry@overlay@@3_NA"]
    pub static g_showCompactAdsTelemetry: bool;
    #[link_name = "?g_compactAdsTelemetryX@overlay@@3MA"]
    pub static g_compactAdsTelemetryX: f32;
    #[link_name = "?g_compactAdsTelemetryY@overlay@@3MA"]
    pub static g_compactAdsTelemetryY: f32;
}
