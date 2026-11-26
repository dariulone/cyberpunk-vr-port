//! The hidden area's runtime table, built here (formerly src/Render/HiddenArea.cpp's, attached at its static
//! initialisation): the exports, the eye's frustum (the runtime FOV and the window the engine renders for it), the menu,
//! engine stereo and the render target's aspect.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64};

use crate::hidden_area::Fov;
use crate::hidden_area_runtime::{cvr_ha_frustum, BarrierRecord, ClearRecord, Runtime};

extern "C" {
    static CyberpunkVR_HiddenAreaEnabled: AtomicU32;
    static CyberpunkVR_HiddenAreaRadius: AtomicU32;
    static CyberpunkVR_HiddenAreaNode: AtomicU32;
    static CyberpunkVR_HiddenAreaLog: AtomicU32;
    static mut CyberpunkVR_HiddenAreaClearLog: [ClearRecord; 32];
    static CyberpunkVR_HiddenAreaMasked: [AtomicU64; 2];
    static CyberpunkVR_HiddenAreaSky: AtomicU32;
    static CyberpunkVR_HiddenAreaSkyNode: AtomicU32;
    static mut CyberpunkVR_HiddenAreaBarrierLog: [BarrierRecord; 32];
    static CyberpunkVR_HiddenAreaSkyResets: [AtomicU64; 2];
    static CyberpunkVR_HiddenAreaNearResets: [AtomicU64; 2];
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_AsymInnerGuard: f32;
    fn GetMenuMode() -> i32;
    #[link_name = "?g_launcherWidth@@3HA"]
    static g_launcherWidth: AtomicI32;
    #[link_name = "?g_launcherHeight@@3HA"]
    static g_launcherHeight: AtomicI32;
}

/// Frustum: MAIN is side 0, and which physical eye it is follows the stereo assignment. The eye's runtime FOV, its
/// asymmetric window and the other eye's FOV (its inner guard) when the projection is on, or the game's symmetric
/// render FOV; the arithmetic is cvr_ha_frustum's.
unsafe extern "C" fn frustum(side: i32, fov: *mut Fov, rendered: *mut Fov) -> bool {
    // SAFETY: an export of the plugin.
    let main_right = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_MainIsRightEye) } != 0;
    let eye = if (side == 0) == main_right { 1 } else { 0 };
    let (mut own, mut other) = ([0.0f32; 4], [0.0f32; 4]);
    // SAFETY: XrFovf is four floats, written by the getter.
    let fov_known = unsafe { cvr_xr::views::cvr_xr_eye_fov(eye, own.as_mut_ptr().cast()) };
    let windows = if fov_known && crate::eye_windows::projection_active() { crate::eye_windows::eye_windows(true) } else { None };
    // SAFETY: as above.
    let other_known = windows.is_some() && unsafe { cvr_xr::views::cvr_xr_eye_fov(1 - eye, other.as_mut_ptr().cast()) };
    let window = windows.map(|w| [w[eye as usize].l, w[eye as usize].r, w[eye as usize].d, w[eye as usize].u]);
    let at = |known: bool, v: &[f32; 4]| if known { v as *const [f32; 4] } else { core::ptr::null() };
    // SAFETY: the pointers are locals or null; the outputs the caller's.
    unsafe {
        cvr_ha_frustum(
            at(fov_known, &own),
            window.as_ref().map_or(core::ptr::null(), |w| w as *const [f32; 4]),
            core::ptr::read_volatile(&raw const CyberpunkVR_AsymInnerGuard),
            at(other_known, &other),
            crate::vr_core_settings::GetGameRenderFovDeg(),
            crate::vr_core_settings::GetGameRenderVerticalFovDeg(),
            fov,
            rendered,
        )
    }
}
unsafe extern "C" fn menu_mode() -> bool {
    // SAFETY: a plain getter of the plugin.
    unsafe { GetMenuMode() != 0 }
}
unsafe extern "C" fn engine_stereo() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
}
/// The launcher's width over its height, 1 when unknown.
unsafe extern "C" fn aspect() -> f32 {
    use core::sync::atomic::Ordering::Relaxed;
    // SAFETY: globals of the plugin.
    let (w, h) = unsafe { (g_launcherWidth.load(Relaxed), g_launcherHeight.load(Relaxed)) };
    if h > 0 {
        w as f32 / h as f32
    } else {
        1.0
    }
}

pub static RT: Runtime = Runtime {
    enabled: &raw const CyberpunkVR_HiddenAreaEnabled,
    radius: &raw const CyberpunkVR_HiddenAreaRadius,
    node: &raw const CyberpunkVR_HiddenAreaNode,
    log: &raw const CyberpunkVR_HiddenAreaLog,
    clear_log: (&raw mut CyberpunkVR_HiddenAreaClearLog).cast(),
    masked: (&raw const CyberpunkVR_HiddenAreaMasked).cast(),
    sky: &raw const CyberpunkVR_HiddenAreaSky,
    sky_node: &raw const CyberpunkVR_HiddenAreaSkyNode,
    barrier_log: (&raw mut CyberpunkVR_HiddenAreaBarrierLog).cast(),
    sky_resets: (&raw const CyberpunkVR_HiddenAreaSkyResets).cast(),
    near_resets: (&raw const CyberpunkVR_HiddenAreaNearResets).cast(),
    frustum,
    menu_mode,
    engine_stereo,
    aspect,
};
