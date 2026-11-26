//! The asymmetric projection's, engine stereo's and Streamline's runtime tables built in Rust (formerly
//! src/Stereo/AsymProjection.cpp's, src/Stereo/EngineStereo.cpp's and src/Hooks/Ngx.cpp's, attached at their static
//! initialisation): the exports they read and write, the eyes' windows, the main-view check, alternate frame warping,
//! the VRCAM switch, the ink layers' kinds, the SDK's arming, the frame generation inputs and the lens mask's tag.
#![allow(non_upper_case_globals)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

use cvr_camera::projection::EyeWindow;
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::object_class;

use crate::engine_stereo_dlss::SetTagFn;
use crate::{asym_projection, engine_stereo, ngx};

extern "C" {
    // the asymmetric projection's
    static CyberpunkVR_AsymProjection: i32;
    static CyberpunkVR_AsymInnerGuard: f32;
    static CyberpunkVR_AsymTest: [f32; 8];
    static mut CyberpunkVR_DebugAsymPrepare: [u64; 3];
    static mut CyberpunkVR_DebugAsymView: [f32; 16];
    static mut CyberpunkVR_DebugAsymPlanes: [f32; 64];
    static mut CyberpunkVR_DebugAsymBuilt: [f32; 32];
    static mut CyberpunkVR_DebugAsymFovTerms: [f32; 4];
    static mut CyberpunkVR_DebugAsymWindow: [f32; 8];
    static mut CyberpunkVR_DebugAsymDescs: [f32; 64];
    static mut CyberpunkVR_DebugAsymPlanePtr: [u32; 8];
    static CyberpunkVR_AsymCascadeUnion: i32;
    static mut CyberpunkVR_DebugAsymCascadeFits: [u64; 3];
    static mut CyberpunkVR_DebugAsymCascadeWindow: [f32; 8];
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    // engine stereo's
    static CyberpunkVR_EngineStereoEyeWindows: i32;
    static CyberpunkVR_EngineStereoEyeJitterWidth: i32;
    static CyberpunkVR_EngineStereoUiOneEye: i32;
    static CyberpunkVR_EngineStereoLoadingPanel: i32;
    static mut CyberpunkVR_DebugEngineStereoFrames: [u64; 2];
    static mut CyberpunkVR_DebugEngineStereoValues: [f32; 3];
    static mut CyberpunkVR_DebugEngineStereoEyeBuilds: [u64; 2];
    static mut CyberpunkVR_DebugEngineStereoEyes: [[f32; 7]; 2];
    static mut CyberpunkVR_DebugEngineStereoEyeJitter: [[f32; 4]; 2];
    static mut CyberpunkVR_DebugEngineStereoPlaneEyes: [u64; 2];
    static mut CyberpunkVR_DebugEngineStereoViewEyes: [[[f32; 6]; 3]; 2];
    static mut CyberpunkVR_DebugEngineStereoUiResizes: [u64; 2];
    static mut CyberpunkVR_DebugEngineStereoUiSize: [i32; 4];
    static mut CyberpunkVR_DebugEngineStereoUiTicks: u64;
    static mut CyberpunkVR_DebugEngineStereoLoadingTicks: u64;
    static mut CyberpunkVR_DebugEngineStereoLoadingLayer: [i32; 2];
    static mut CyberpunkVR_EngineStereo: i32;
    static mut CyberpunkVR_MonoMode: i32;
    static mut CyberpunkVR_MonoView: i32;
    static mut CyberpunkVR_Afw: i32;
    static mut CyberpunkVR_IpdInWorldPos: i32;
    /// The VRCAM switch (src/Stereo/SyncStereo.cpp).
    fn CyberpunkVR_SetVrcamEnabled(on: u32);
    #[link_name = "?g_launcherWidth@@3HA"]
    static g_launcherWidth: AtomicI32;
    #[link_name = "?g_launcherHeight@@3HA"]
    static g_launcherHeight: AtomicI32;
    // Streamline's
    static CyberpunkVR_NgxLegacyCapture: u32;
    /// g_verboseLog (C++ linkage).
    #[link_name = "?g_verboseLog@@3HC"]
    static g_verboseLog: i32;
}

// ---- the asymmetric projection
/// OpenXRManager::GetAsymmetricEyeWindows.
extern "C" fn eye_windows(out: *mut EyeWindow, fit_target: bool) -> bool {
    // SAFETY: the caller's two windows.
    unsafe { crate::eye_windows::cvr_xr_asym_eye_windows(out, fit_target) }
}
/// cvr::detail::is_main_view, a fault caught (the C++ __try): false on a fault.
extern "C" fn is_main_view(view: *const u8) -> bool {
    let cached = crate::moved_globals::g_main_view_ctx.load(Ordering::Acquire);
    // SAFETY: an engine view the caller found, read inside the SEH adapter.
    unsafe {
        cvr_engine::code::guarded_call3(crate::main_view_check::cvr_ss_is_main_view as *const () as usize,
                                        view as *mut c_void, cached as *mut c_void, core::ptr::null_mut())
    }
    .unwrap_or(false)
}

pub static ASYM: asym_projection::Runtime = asym_projection::Runtime {
    mode: &raw const CyberpunkVR_AsymProjection,
    inner_guard: &raw const CyberpunkVR_AsymInnerGuard,
    test: (&raw const CyberpunkVR_AsymTest).cast(),
    debug_prepare: (&raw mut CyberpunkVR_DebugAsymPrepare).cast(),
    debug_view: (&raw mut CyberpunkVR_DebugAsymView).cast(),
    debug_planes: (&raw mut CyberpunkVR_DebugAsymPlanes).cast(),
    debug_built: (&raw mut CyberpunkVR_DebugAsymBuilt).cast(),
    debug_fov_terms: (&raw mut CyberpunkVR_DebugAsymFovTerms).cast(),
    debug_window: (&raw mut CyberpunkVR_DebugAsymWindow).cast(),
    debug_descs: (&raw mut CyberpunkVR_DebugAsymDescs).cast(),
    debug_plane_ptr: (&raw mut CyberpunkVR_DebugAsymPlanePtr).cast(),
    cascade_union: &raw const CyberpunkVR_AsymCascadeUnion,
    debug_cascade_fits: (&raw mut CyberpunkVR_DebugAsymCascadeFits).cast(),
    debug_cascade_window: (&raw mut CyberpunkVR_DebugAsymCascadeWindow).cast(),
    main_is_right_eye: &raw const CyberpunkVR_MainIsRightEye,
    runtime_diagnostics: &raw const CyberpunkVR_RuntimeDiagnostics,
    vrcam_key: &raw const crate::moved_globals::g_vrcam_ctx_key,
    eye_windows,
    is_main_view,
};

// ---- engine stereo
/// cvr::afw::SetLaunch(turns != 0) and SetMode(turns).
extern "C" fn afw_apply(turns: i32) {
    crate::afw::cvr_afw_set_launch(turns != 0);
    crate::afw::cvr_afw_set_mode(turns);
}
extern "C" fn set_vrcam_enabled(on: u32) {
    // SAFETY: the plugin's switch.
    unsafe { CyberpunkVR_SetVrcamEnabled(on) }
}
/// An ink layer's kind by its class: the HUD's and the game notifications' 1, the loading screen's 2, else 0.
extern "C" fn layer_kind(layer: *mut c_void) -> i32 {
    // SAFETY: a live ink layer the caller found (layer->GetType()->GetName()).
    let Some(class) = (unsafe { object_class(layer) }) else { return 0 };
    let name = class.class_name();
    if name == CName::hashed("inkHUDLayer") || name == CName::hashed("inkGameNotificationsLayer") {
        1
    } else if name == CName::hashed("inkLoadingLayer") {
        2
    } else {
        0
    }
}

pub static ENGINE_STEREO: engine_stereo::Runtime = engine_stereo::Runtime {
    eye_windows: &raw const CyberpunkVR_EngineStereoEyeWindows,
    eye_jitter_width: &raw const CyberpunkVR_EngineStereoEyeJitterWidth,
    ui_one_eye: &raw const CyberpunkVR_EngineStereoUiOneEye,
    loading_panel: &raw const CyberpunkVR_EngineStereoLoadingPanel,
    debug_frames: (&raw mut CyberpunkVR_DebugEngineStereoFrames).cast(),
    debug_values: (&raw mut CyberpunkVR_DebugEngineStereoValues).cast(),
    debug_eye_builds: (&raw mut CyberpunkVR_DebugEngineStereoEyeBuilds).cast(),
    debug_eyes: (&raw mut CyberpunkVR_DebugEngineStereoEyes).cast(),
    debug_eye_jitter: (&raw mut CyberpunkVR_DebugEngineStereoEyeJitter).cast(),
    debug_plane_eyes: (&raw mut CyberpunkVR_DebugEngineStereoPlaneEyes).cast(),
    debug_view_eyes: (&raw mut CyberpunkVR_DebugEngineStereoViewEyes).cast(),
    debug_ui_resizes: (&raw mut CyberpunkVR_DebugEngineStereoUiResizes).cast(),
    debug_ui_size: (&raw mut CyberpunkVR_DebugEngineStereoUiSize).cast(),
    debug_ui_ticks: &raw mut CyberpunkVR_DebugEngineStereoUiTicks,
    debug_loading_ticks: &raw mut CyberpunkVR_DebugEngineStereoLoadingTicks,
    debug_loading_layer: (&raw mut CyberpunkVR_DebugEngineStereoLoadingLayer).cast(),
    launcher_width: (&raw const g_launcherWidth).cast(),
    launcher_height: (&raw const g_launcherHeight).cast(),
    engine_stereo: &raw mut CyberpunkVR_EngineStereo,
    mono_mode: &raw mut CyberpunkVR_MonoMode,
    mono_view: &raw mut CyberpunkVR_MonoView,
    afw: &raw mut CyberpunkVR_Afw,
    ipd_in_world_pos: &raw mut CyberpunkVR_IpdInWorldPos,
    afw_apply,
    afw_launch: crate::afw::cvr_afw_launch,
    set_vrcam_enabled,
    layer_kind,
    armed: crate::engine_stereo_sdk::cvr_engine_stereo_sdk_armed,
};

// ---- Streamline
/// cvr::optics::mask::SetTag with this thread's view side: the scope image's history mask rides with the velocity tag,
/// then Streamline's own slSetTag (the list's original ClearRenderTargetView for the mask's clear).
unsafe extern "C" fn mask_set_tag(viewport: *const c_void, tags: *const c_void, count: u32, list: *mut c_void,
                                  original: SetTagFn) -> u32 {
    let clear = if list.is_null() {
        0
    } else {
        crate::list_vtable::entry(list).map_or(0, |e| e.tail[crate::list_detours::tail::CLEAR_RTV])
    };
    // SAFETY: Streamline's arguments, passed on.
    unsafe {
        cvr_gpu::optics::cvr_optics_mask_set_tag(viewport, tags, count, list, Some(original),
                                                 crate::render_tls::view_side(), clear)
    }
}

// SAFETY: process-lifetime globals and plain functions.
unsafe impl Sync for ngx::Runtime {}
pub static NGX: ngx::Runtime = ngx::Runtime {
    verbose_log: &raw const g_verboseLog,
    legacy_capture: &raw const CyberpunkVR_NgxLegacyCapture,
    framegen_record_tags: crate::framegen_inputs::cvr_framegen_record_tags,
    framegen_install_input_hooks: crate::framegen_inputs::cvr_framegen_install_input_hooks,
    mask_set_tag,
};
