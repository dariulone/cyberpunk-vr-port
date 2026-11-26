//! THE OVERLAY'S SIDE OF THE LIVE CONTROLS (formerly src/Core/LiveControlsPoll.cpp's MakeLiveControlsUiState,
//! PersistLiveControlsUiState and SetLiveControlsUiState): the overlay edits a snapshot and hands it back, so a slider
//! dragged in the headset survives a restart. The snapshot is taken from the live state, applied to it with the same
//! clamps the ini path has, and written to vrport.ini -- the WHOLE file, every key, because a key left out is a key
//! deleted (the mistake that once ate xr_hand_predict).
//!
//! The subsystems' settings stay C++ structs inside the snapshot: taken, applied and written by the C++ half one
//! subsystem at a time (0 hud, 1 framegen, 2 foveated, 3 vrs, 4 hidden area, 5 overlay), into the same FILE.
#![allow(non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};

use cvr_crt::{cpp_clamp, format, Arg};
use cvr_settings::live_controls_ui_state::LiveControlsUiState;
use cvr_settings::haptic_route;
use cvr_settings::wheel_range::{bike_bar_lock_degrees, bike_body_lean_share, wheel_full_lock_degrees};
use cvr_settings::{lc, lc_set};

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    init_runtime_paths: extern "C" fn(),
    /// One subsystem's settings: taken from it into the snapshot, applied from the snapshot, written to a FILE.
    subsystem_get: extern "C" fn(i32, *mut LiveControlsUiState),
    subsystem_set: extern "C" fn(i32, *const LiveControlsUiState),
    subsystem_write: extern "C" fn(i32, *mut c_void, *const LiveControlsUiState),
    set_immersive_holsters: extern "C" fn(i32),
    set_body_hands_only: extern "C" fn(i32),
    set_mono_submit_enabled: extern "C" fn(bool),
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()): a default-constructed snapshot, filled and saved here.
    persist_live_controls: extern "C" fn(),
    live_control_path: *const c_char,
    last_live_control_write: *mut [u32; 2],
    draw_barrel_cross: *mut bool,
    vive_trackers_setting: *const i32,
    submit_chain_rev: *const i32,
}
/// The plugin's table, built here (formerly src/Core/LiveControlsPoll.cpp's): the subsystems' settings by their Rust
/// owners, the XR switches, the files' paths and stamps and the flags the C++ half keeps.
mod table {
    use super::{LiveControlsUiState, Runtime};
    use core::ffi::{c_char, c_void};
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

    unsafe extern "C" {
        static CyberpunkVR_HiddenAreaEnabled: AtomicU32;
        static CyberpunkVR_HiddenAreaRadius: AtomicU32;
        /// OpenXRManager::SetMonoSubmitEnabled (src/Runtimes/OpenXRManager.cpp).
        safe fn cvr_cpp_xr_set_mono_submit_enabled(enabled: bool);
        #[link_name = "?g_liveControlPath@@3PADA"]
        static g_liveControlPath: [c_char; 260];
        #[link_name = "?g_lastLiveControlWrite@@3U_FILETIME@@A"]
        static mut g_lastLiveControlWrite: [u32; 2];
        #[link_name = "?g_drawBarrelCross@overlay@@3_NA"]
        static mut g_drawBarrelCross: bool;
        #[link_name = "?g_viveTrackersSetting@@3HA"]
        static g_viveTrackersSetting: i32;
        #[link_name = "?g_submitChainRev@@3HA"]
        static g_submitChainRev: i32;
    }

    /// cvr::hidden_area::GetSettings: the exports, normalised.
    fn hidden_area() -> crate::hidden_area::Settings {
        // SAFETY: the plugin's exports.
        let (enabled, radius) = unsafe { (CyberpunkVR_HiddenAreaEnabled.load(Relaxed), CyberpunkVR_HiddenAreaRadius.load(Relaxed)) };
        crate::hidden_area::normalize(crate::hidden_area::Settings { enabled: enabled as i32, radius: f32::from_bits(radius) })
    }
    /// A subsystem's settings inside the snapshot, as its own type.
    unsafe fn at<T>(slot: *const u32) -> T {
        // SAFETY: the snapshot's field holds the subsystem's settings.
        unsafe { core::ptr::read_unaligned(slot.cast()) }
    }
    unsafe fn put<T>(slot: *mut u32, v: T) {
        // SAFETY: as above.
        unsafe { core::ptr::write_unaligned(slot.cast(), v) }
    }
    /// 0 hud, 1 framegen, 2 foveated, 3 vrs, 4 hidden area, 5 overlay: taken from the subsystem into the snapshot.
    extern "C" fn subsystem_get(which: i32, s: *mut LiveControlsUiState) {
        // SAFETY: the caller's snapshot.
        let s = unsafe { &mut *s };
        // SAFETY: each field holds that subsystem's settings.
        unsafe {
            match which {
                0 => {
                    cvr_settings::hud_layout::cvr_hud_layout_settings(s.hudElements.as_mut_ptr().cast(), cvr_settings::hud_layout::COUNT);
                }
                1 => cvr_gpu::framegen_core::cvr_framegen_get_settings(s.framegen.as_mut_ptr().cast()),
                2 => put(s.foveated.as_mut_ptr(), crate::foveated::cvr_foveated_get_settings()),
                3 => put(s.vrs.as_mut_ptr(), crate::fixed_vrs::cvr_vrs_get_settings()),
                4 => put(s.hiddenArea.as_mut_ptr(), hidden_area()),
                5 => cvr_body::vr_overlay::cvr_vrui_get_settings(s.overlay.as_mut_ptr().cast()),
                _ => {}
            }
        }
    }
    /// The same, applied from the snapshot.
    extern "C" fn subsystem_set(which: i32, s: *const LiveControlsUiState) {
        // SAFETY: the caller's snapshot.
        let s = unsafe { &*s };
        // SAFETY: each field holds that subsystem's settings.
        unsafe {
            match which {
                0 => cvr_settings::hud_layout::cvr_hud_layout_set_settings(s.hudElements.as_ptr().cast()),
                1 => cvr_gpu::framegen_core::cvr_framegen_set_settings(s.framegen.as_ptr().cast()),
                2 => crate::foveated::cvr_foveated_set_settings(at(s.foveated.as_ptr())),
                3 => crate::fixed_vrs::cvr_vrs_set_settings(at(s.vrs.as_ptr())),
                4 => {
                    let v = crate::hidden_area::normalize(at(s.hiddenArea.as_ptr()));
                    CyberpunkVR_HiddenAreaRadius.store(v.radius.to_bits(), Relaxed);
                    CyberpunkVR_HiddenAreaEnabled.store(v.enabled as u32, Relaxed);
                }
                5 => cvr_body::vr_overlay::cvr_vrui_set_settings(s.overlay.as_ptr().cast()),
                _ => {}
            }
        }
    }
    /// The same, written to vrport.ini's FILE.
    extern "C" fn subsystem_write(which: i32, file: *mut c_void, s: *const LiveControlsUiState) {
        // SAFETY: the caller's snapshot.
        let s = unsafe { &*s };
        // SAFETY: the caller's FILE; each field holds that subsystem's settings.
        unsafe {
            match which {
                0 => cvr_settings::hud_layout::cvr_hud_layout_save(file, s.hudElements.as_ptr().cast()),
                1 => {
                    let mut text = [0u8; 1024];
                    let n = cvr_gpu::framegen_core::cvr_framegen_settings_text(s.framegen.as_ptr().cast(), text.as_mut_ptr(),
                                                                               text.len());
                    if n < text.len() {
                        if let Ok(line) = core::ffi::CStr::from_bytes_until_nul(&text) {
                            cvr_crt::fputs_to(file, line);
                        }
                    }
                }
                2 => crate::foveated::cvr_foveated_write_settings(file, at(s.foveated.as_ptr())),
                3 => crate::vrs::cvr_vrs_write_settings(file, at(s.vrs.as_ptr())),
                4 => crate::hidden_area::cvr_hidden_area_write_settings(file, at(s.hiddenArea.as_ptr())),
                5 => cvr_body::vr_overlay::cvr_vrui_write_settings(file, s.overlay.as_ptr().cast()),
                _ => {}
            }
        }
    }
    extern "C" fn set_immersive_holsters(v: i32) {
        cvr_xr::switches::SWITCHES.immersive_holsters.store(v, Relaxed);
    }
    extern "C" fn set_body_hands_only(v: i32) {
        cvr_xr::switches::SWITCHES.body_hands_only.store(v, Relaxed);
    }
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()).
    extern "C" fn persist_live_controls() {
        // SAFETY: a zeroed snapshot (as the C++ half default-constructed it), filled whole by the make.
        let mut state: LiveControlsUiState = unsafe { core::mem::zeroed() };
        unsafe {
            super::cvr_make_live_controls_ui_state(&mut state);
            let _ = super::cvr_persist_live_controls_ui_state(&state);
        }
    }

    pub(super) static RT: Runtime = Runtime {
        init_runtime_paths: crate::runtime_paths::cvr_init_runtime_paths,
        subsystem_get,
        subsystem_set,
        subsystem_write,
        set_immersive_holsters,
        set_body_hands_only,
        set_mono_submit_enabled: cvr_cpp_xr_set_mono_submit_enabled,
        persist_live_controls,
        live_control_path: (&raw const g_liveControlPath).cast(),
        last_live_control_write: &raw mut g_lastLiveControlWrite,
        draw_barrel_cross: &raw mut g_drawBarrelCross,
        vive_trackers_setting: &raw const g_viveTrackersSetting,
        submit_chain_rev: &raw const g_submitChainRev,
    };
}
// SAFETY: process-lifetime globals and plain functions.
unsafe impl Sync for Runtime {}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const table::RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_live_controls_ui_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}

extern "C" {
    static mut CyberpunkVR_ThreadedMonoSubmit: i32;
    static CyberpunkVR_MonoCascades: AtomicU32;
    static mut CyberpunkVR_MonoMode: i32;
    static mut CyberpunkVR_Afw: i32;
    static mut CyberpunkVR_FingerBones: i32;
    static mut CyberpunkVR_FingerSpreadOn: i32;
    static mut CyberpunkVR_FingerSpread: f32;
    static mut CyberpunkVR_HudClearance: i32;
    static mut CyberpunkVR_HudClearanceGrow: f32;
    static mut CyberpunkVR_HudClearanceHands: i32;
    static mut CyberpunkVR_HudClearanceHandsScale: f32;
    static mut CyberpunkVR_InteractionAnchor: i32;
    static mut CyberpunkVR_AsymProjection: i32;
    static mut CyberpunkVR_AsymInnerGuard: f32;
    static mut CyberpunkVR_Haptics: i32;
    static mut CyberpunkVR_HapticScale: f32;
    static mut CyberpunkVR_HandLerpSpeed: f32;
    static mut CyberpunkVR_HandLerpPistol: f32;
    static mut CyberpunkVR_HandLerpRifle: f32;
    static mut CyberpunkVR_HandLerpShotgun: f32;
    static mut CyberpunkVR_HandLerpSniper: f32;
    static mut CyberpunkVR_HandLerpMelee: f32;
    static mut CyberpunkVR_HandLerpFramegenScale: f32;
    static mut CyberpunkVR_GradeMirrorMask: u32;
    static mut CyberpunkVR_EnvExtraMask: u32;
    static mut CyberpunkVR_WaterMirror: u32;
    static mut CyberpunkVR_WaterAssign: u32;
    static mut CyberpunkVR_ViewDataDiff: i32;
    static mut CyberpunkVR_LutSrcLend: u32;
    static mut CyberpunkVR_SceneStickYawDps: f32;
    static mut CyberpunkVR_SceneStickYawTier: i32;
    static mut CyberpunkVR_BlockDiff: i32;
    static mut CyberpunkVR_BlockDiffNode: u32;
    static mut CyberpunkVR_BlockDiffSize: u32;
    static mut CyberpunkVR_RenderMaskGrant: u32;
    static mut CyberpunkVR_StageReplaySetting: i32;
    static mut CyberpunkVR_VrcamGbufferPartitionsSetting: i32;
    static mut CyberpunkVR_ScopeGbufferPartitionsSetting: i32;
    static mut CyberpunkVR_ScopeGraphDag: u32;
    static mut CyberpunkVR_SubmitChainSetting: i32;
    static mut CyberpunkVR_DeferPresentSetting: i32;
    static mut CyberpunkVR_GtSkipRenderQueueSetting: i32;
    static mut CyberpunkVR_ViewDataFixMask: u32;
    static mut CyberpunkVR_EnvMirrorMask: u32;
    static mut CyberpunkVR_BdCamDirty: i32;
    static mut CyberpunkVR_BdPushTransform: i32;
    static mut CyberpunkVR_BdMainPosFromScene: i32;
    static mut CyberpunkVR_BdPushBase: i32;
    static mut CyberpunkVR_BdOneComposition: i32;
    static mut CyberpunkVR_BdEditorAlign: i32;
    static mut CyberpunkVR_BdBaseFromLocate: i32;
    static mut CyberpunkVR_CompLendSet: u32;
    static mut CyberpunkVR_FinalGrab: i32;
    static mut CyberpunkVR_StableSrgbView: i32;
    static mut CyberpunkVR_BdQuatFromWriteSite: i32;
    static mut CyberpunkVR_DevCamTolM: f32;
    static mut CyberpunkVR_DevCamAnyName: i32;
    static mut CyberpunkVR_DevCamInLocate: i32;
    static mut CyberpunkVR_LensHeadWrite: i32;
    static mut CyberpunkVR_InputDefaultInUi: i32;
    static mut CyberpunkVR_PopupMagBlockMs: i32;
    static mut CyberpunkVR_VrcamPosFromMain: i32;
    static mut CyberpunkVR_VisionCsNode: u32;
    static mut CyberpunkVR_VisionCsSwap: i32;
    static mut CyberpunkVR_VisionCsDim: i32;
    static mut CyberpunkVR_VisionCsOrd: i32;
    static mut CyberpunkVR_WeaponPlaneAsScene: i32;
    static mut CyberpunkVR_HighlightAnyPlane: i32;
    static mut CyberpunkVR_PlaneBucketScene: i32;
    static mut CyberpunkVR_BdIpdInLocate: i32;
    static mut CyberpunkVR_BdQuatFromBuffer: i32;
    static mut CyberpunkVR_CapGrant: u32;
    static mut CyberpunkVR_ForceVrcamCam: u32;
    static mut CyberpunkVR_VrcamCamFields: u32;
    static mut CyberpunkVR_RunViewParams: u32;
    static mut CyberpunkVR_ExpoProbe: i32;
    static mut CyberpunkVR_IndirectCensus: i32;
    static mut CyberpunkVR_NoStateLies: i32;
    static mut CyberpunkVR_StableCopy: i32;
    static mut CyberpunkVR_DistantReuseMode: u32;
    static mut CyberpunkVR_LocalShadowReuseMode: u32;
    static mut CyberpunkVR_GiReuseMode: u32;
    static mut CyberpunkVR_ProbeReuseMode: u32;
    static mut CyberpunkVR_ExpoMirror: i32;
    static mut CyberpunkVR_ExpoFieldMask: u32;
    static mut CyberpunkVR_CompLendScoped: u32;
    static mut CyberpunkVR_LightContent: i32;
    static mut CyberpunkVR_FixLodEnable: i32;
    static mut CyberpunkVR_HudToSecondEye: i32;
    static mut CyberpunkVR_AutoRecenter: i32;
    static mut CyberpunkVR_VisionToSecondEye: i32;
    static mut CyberpunkVR_HudInBraindance: i32;
    static mut CyberpunkVR_VrcamCompositionGroup: u32;
    static mut CyberpunkVR_TwoHandRadius: f32;
    static mut CyberpunkVR_TwoHandAim: i32;
    static mut CyberpunkVR_TwoHandBreakRadius: f32;
    static mut CyberpunkVR_TwoHandAimReleaseMs: f32;
    static mut CyberpunkVR_CarryGripRadius: f32;
    static mut CyberpunkVR_RifleRiseMul: f32;
    static mut CyberpunkVR_RifleBackMul: f32;
    static mut CyberpunkVR_ShotgunRiseMul: f32;
    static mut CyberpunkVR_ShotgunBackMul: f32;
    static mut CyberpunkVR_SniperRiseMul: f32;
    static mut CyberpunkVR_SniperBackMul: f32;
    static mut CyberpunkVR_RecoilScale: f32;
    static mut CyberpunkVR_PistolBackMul: f32;
    static mut CyberpunkVR_RecoilClimbFrac: f32;
    static mut CyberpunkVR_RecoilClimbMaxDeg: f32;
    static mut CyberpunkVR_RecoilClimbMs: f32;
    static mut CyberpunkVR_HandRecoilReturnMs: f32;
    static mut CyberpunkVR_HandRecoilReturnPow: f32;
    static mut CyberpunkVR_RecoilDownPistol: f32;
    static mut CyberpunkVR_RecoilDownRifle: f32;
    static mut CyberpunkVR_RecoilDownShotgun: f32;
    static mut CyberpunkVR_HandRecoilBackCm: f32;
    static mut CyberpunkVR_HandRecoilBackMaxCm: f32;
    static mut CyberpunkVR_TwoHandBackMul: f32;
    static mut CyberpunkVR_ScannerSlots: [f32; 21];

    fn GetHmdTrackingSmooth() -> f32;
    fn cvr_optics_source_get_stabilize() -> i32;
    fn cvr_optics_source_set_stabilize(percent: i32);
    fn cvr_reflex_get_mode() -> i32;
    fn cvr_reflex_set_mode(mode: i32);
    fn cvr_reflex_normalize_mode(mode: i32) -> i32;
    fn cvr_framegen_fps_cap() -> bool;
    fn cvr_framegen_set_fps_cap(on: bool);
    fn cvr_engine_stereo_apply_mono_mode(mono: bool);
    fn cvr_engine_stereo_requested() -> bool;
    fn cvr_fingergrip_grip_from_fingers() -> bool;
    fn cvr_fingergrip_set_grip_from_fingers(on: bool);
    fn cvr_stick_deadzone(value: f32) -> f32;
    fn cvr_stick_full_input(value: f32) -> f32;
    fn cvr_rotation_mode_from_flags(physical: i32, tracked: i32, hybrid: i32, head_led: i32) -> i32;
    fn cvr_crouch_threshold(value: f32) -> f32;
    fn cvr_resolve_vehicle_vrik_suspend_tier(on_foot: i32, vehicle: i32, explicit: bool) -> i32;
}

macro_rules! g {
    ($name:ident) => {{
        // SAFETY: a scalar of the C++ half (or of Rust under its C name), alive for the process.
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}
macro_rules! s {
    ($name:ident, $value:expr) => {{
        let value = $value;
        // SAFETY: as g!.
        #[allow(unused_unsafe)]
        unsafe {
            core::ptr::write_volatile(&raw mut $name, value)
        }
    }};
}
fn b(v: bool) -> i32 {
    v as i32
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

const HUD: i32 = 0;
const FRAMEGEN: i32 = 1;
const FOVEATED: i32 = 2;
const VRS: i32 = 3;
const HIDDEN_AREA: i32 = 4;
const OVERLAY: i32 = 5;

// ---- the snapshot -----------------------------------------------------------------------------------------------------

/// MakeLiveControlsUiState, into a snapshot the C++ half default-constructed.
///
/// # Safety
/// `state` is a LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_make_live_controls_ui_state(state: *mut LiveControlsUiState) {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return;
    };
    let r = rt();
    // SAFETY: the C++ half's bool.
    st.xrWeaponLaserDot = unsafe { core::ptr::read_volatile(r.draw_barrel_cross) };
    st.xrHeadOffsetX = lc!(xrHeadOffsetX);
    st.xrHeadOffsetY = lc!(xrHeadOffsetY);
    st.xrHeadOffsetZ = lc!(xrHeadOffsetZ);
    st.xrRecenter = lc!(xrRecenter);
    st.xrMonoSubmit = lc!(xrMonoSubmit);
    st.xrForceFov = lc!(xrForceFov);
    st.xrMenuRect = lc!(xrMenuRect);
    st.xrMenuFov = lc!(xrMenuFov);
    st.xrMenuFollowDeg = lc!(xrMenuFollowDeg);
    st.xrHudPanel = lc!(xrHudPanel);
    st.xrHudFollowDeg = lc!(xrHudFollowDeg);
    st.xrInteractionPanel = lc!(xrInteractionPanel);
    st.xrInteractionFollowDeg = lc!(xrInteractionFollowDeg);
    st.xrLootFollowDeg = lc!(xrLootFollowDeg);
    st.xrInteractionDistance = lc!(xrInteractionDistance);
    st.xrInteractionFov = lc!(xrInteractionFov);
    st.xrHudFov = lc!(xrHudFov);
    st.xrHudDistance = lc!(xrHudDistance);
    st.xrHudFollowMode = lc!(xrHudFollowMode);
    st.xrHudStereoDepth = lc!(xrHudStereoDepth);
    // SAFETY: Rust's own function under its C name.
    st.xrGripFromFingers = b(unsafe { cvr_fingergrip_grip_from_fingers() });
    st.xrFingerBones = g!(CyberpunkVR_FingerBones);
    st.xrFingerSpreadOn = g!(CyberpunkVR_FingerSpreadOn);
    st.xrFingerSpread = g!(CyberpunkVR_FingerSpread);
    st.xrHudClearance = g!(CyberpunkVR_HudClearance);
    st.xrHudClearanceGrow = g!(CyberpunkVR_HudClearanceGrow);
    st.xrHudClearanceHands = g!(CyberpunkVR_HudClearanceHands);
    st.xrHudClearanceHandsScale = g!(CyberpunkVR_HudClearanceHandsScale);
    st.xrInteractionAnchor = g!(CyberpunkVR_InteractionAnchor);
    st.xrHudBrightness = lc!(xrHudBrightness);
    st.xrHudShadow = lc!(xrHudShadow);
    st.xrHudGlow = lc!(xrHudGlow);
    (r.subsystem_get)(HUD, st);
    (r.subsystem_get)(FRAMEGEN, st);
    (r.subsystem_get)(FOVEATED, st);
    (r.subsystem_get)(VRS, st);
    (r.subsystem_get)(HIDDEN_AREA, st);
    // SAFETY: the C++ half's atomic; Rust's own functions under their C names.
    unsafe {
        st.monoCascades = CyberpunkVR_MonoCascades.load(Relaxed) as i32;
        st.monoMode = b(g!(CyberpunkVR_MonoMode) != 0);
        st.afw = b(g!(CyberpunkVR_Afw) == 1);
        st.osr = b(g!(CyberpunkVR_Afw) == 2);
        st.scopeStabilize = cvr_optics_source_get_stabilize();
        st.nvidiaReflex = cvr_reflex_get_mode();
        st.fpsCap = b(cvr_framegen_fps_cap());
    }
    st.lobby = b(crate::menu_lobby::cvr_menu_lobby_enabled());
    st.lobbyAmbient = b(crate::menu_lobby::cvr_menu_lobby_ambient());
    (r.subsystem_get)(OVERLAY, st);
    st.xr3DofMovement = lc!(xr3DofMovement);
    st.xrFirstLaunch = lc!(xrFirstLaunch);
    st.xrMotionPredictMs = lc!(xrMotionPredictMs);
    st.xrStereoScale = lc!(xrStereoScale);
    st.xrWorldScale = lc!(xrWorldScale);
    st.xrIpdScale = lc!(xrIpdScale);
    st.xrSharpness = lc!(xrSharpness);
    st.xrSharpmix = lc!(xrSharpmix);
    st.xrReuseLastFrame = lc!(xrReuseLastFrame);
    st.xrPairLock = lc!(xrPairLock);
    st.xrRenderPoseSubmit = lc!(xrRenderPoseSubmit);
    st.xrPoseLag = lc!(xrPoseLag);
    st.xrRuntime = lc!(xrRuntime);
    st.xrMovementControl = lc!(xrMovementControl);
    st.xrDisableMouseY = lc!(xrDisableMouseY);
    st.xrXInputHook = lc!(xrXInputHook);
    st.xrSnapTurn = lc!(xrSnapTurn);
    st.xrSnapTurnAngleDeg = lc!(xrSnapTurnAngleDeg);
    st.xrMovementSource = lc!(xrMovementSource);
    st.xrMovementSpeedMode = lc!(xrMovementSpeedMode);
    st.xrSprintMode = lc!(xrSprintMode);
    st.xrLeftStickDeadzone = lc!(xrLeftStickDeadzone);
    st.xrRightStickDeadzone = lc!(xrRightStickDeadzone);
    st.xrMaxInputThreshold = lc!(xrMaxInputThreshold);
    // cvr::body::RotationMode: Off, HeadCone, Tracked, Hybrid, HeadLed.
    let body_mode = lc!(xrBodyRotationMode);
    st.xrPhysicalBodyRotation = b(body_mode == 1);
    st.xrTrackedBodyRotation = b(body_mode == 2);
    st.xrHybridBodyRotation = b(body_mode == 3);
    st.xrHeadLedBodyRotation = b(body_mode == 4);
    st.xrPsvr2HapticBackend = haptic_route::HAPTIC_ROUTE.requested();
    st.xrOpenXrHapticGain = haptic_route::legacy_gain();
    st.xrOpenXrVehicleHapticGain = haptic_route::vehicle_gain();
    st.xrVehicleAudioChannels = haptic_route::vehicle_audio_channels();
    st.xrVehicleAudioRequireGrip = haptic_route::vehicle_audio_require_grip();
    st.xrRoomscaleMovement = lc!(xrRoomscaleMovement);
    st.xrPhysicalCrouch = lc!(xrPhysicalCrouch);
    st.xrPhysicalCrouchDepth = lc!(xrPhysicalCrouchDepth);
    st.xrAsymProjection = b(g!(CyberpunkVR_AsymProjection) == 1);
    st.xrHaptics = b(g!(CyberpunkVR_Haptics) != 0);
    st.xrHapticScale = g!(CyberpunkVR_HapticScale);
    st.xrBreaststrokeSwim = lc!(xrBreaststrokeSwim);
    st.xrLadderGripClimb = lc!(xrLadderGripClimb);
    st.xrLeftHanded = lc!(xrLeftHanded);
    st.xrLeftHandedBindings = lc!(xrLeftHandedBindings);
    st.xrLadderAutoFinish = lc!(xrLadderAutoFinish);
    st.xrLadderFinishDistance = lc!(xrLadderFinishDistance);
    st.xrBodyFreeLookDeg = lc!(xrBodyFreeLookDeg);
    st.xrBodyFreeLookDownDeg = lc!(xrBodyFreeLookDownDeg);
    st.xrBodyFreeLookSwimDeg = lc!(xrBodyFreeLookSwimDeg);
    st.xrBodyMoveRadius = lc!(xrBodyMoveRadius);
    st.xrSceneCamComfortDeg = lc!(xrSceneCamComfortDeg);
    st.xrCutsceneSuspendTier = lc!(xrCutsceneSuspendTier);
    st.xrVehicleCutsceneSuspendTier = lc!(xrVehicleCutsceneSuspendTier);
    st.xrXInputInstall = lc!(xrXInputInstall);
    st.xrInputActions = lc!(xrInputActions);
    st.xrMonoXQueueWait = lc!(xrMonoXQueueWait);
    st.xrMonoDepthCapture = lc!(xrMonoDepthCapture);
    st.xrSnapTurnPulseMs = lc!(xrSnapTurnPulseMs);
    st.xrImmersiveHolsters = lc!(xrImmersiveHolsters);
    st.xrBodyHandsOnly = lc!(xrBodyHandsOnly);
    st.xrVehHeadOffsetX = lc!(xrVehHeadOffsetX);
    st.xrVehHeadOffsetY = lc!(xrVehHeadOffsetY);
    st.xrVehHeadOffsetZ = lc!(xrVehHeadOffsetZ);
    st.xrWheelGrab = lc!(xrWheelGrab);
    st.xrMenuGesture = lc!(xrMenuGesture);
    st.xrVrWeaponWheel = lc!(xrVrWeaponWheel);
    st.xrWheelRadius = lc!(xrWheelRadius);
    st.xrWheelSteerMaxDeg = lc!(xrWheelSteerMaxDeg);
    st.xrWheelSteerDeadDeg = lc!(xrWheelSteerDeadDeg);
    st.xrWheelPrediction = lc!(xrWheelPrediction);
    st.xrWheelPredictionMs = lc!(xrWheelPredictionMs);
    st.xrWheelHorn = lc!(xrWheelHorn);
    st.xrWheelHornRadius = lc!(xrWheelHornRadius);
    st.xrVehicleGunTrigger = lc!(xrVehicleGunTrigger);
    st.xrVehicleThrottleTrim = lc!(xrVehicleThrottleTrim);
    st.xrReloadGlow = lc!(xrReloadGlow);
    st.xrBikeHeadOffsetX = lc!(xrBikeHeadOffsetX);
    st.xrBikeHeadOffsetY = lc!(xrBikeHeadOffsetY);
    st.xrBikeHeadOffsetZ = lc!(xrBikeHeadOffsetZ);
    st.xrVrInterface = lc!(xrVrInterface);
    st.xrHandPhysics = lc!(xrHandPhysics);
    st.xrHandCollision = lc!(xrHandCollision);
    st.xrHandCollisionVehicle = lc!(xrHandCollisionVehicle);
    st.xrHandPush = lc!(xrHandPush);
    st.xrFingerContact = lc!(xrFingerContact);
    st.xrPropGrab = lc!(xrPropGrab);
    st.xrDebugView = lc!(xrDebugView);
    st.xrCarryPicture = lc!(xrCarryPicture);
    st.xrCarryLead = lc!(xrCarryLead);
    st.xrContactMargin = lc!(xrContactMargin);
    st.xrWheelAnywhere = lc!(xrWheelAnywhere);
    st.xrBikeBarLockDeg = lc!(xrBikeBarLockDeg);
    st.xrBikeBodyLean = lc!(xrBikeBodyLean);
}

// ---- the save -------------------------------------------------------------------------------------------------------

/// The file the save writes, as the C++ half wrote it: `fprintf` into a text-mode FILE.
struct Out(cvr_crt::File);
impl Out {
    fn w(&mut self, fmt: &core::ffi::CStr, args: &[Arg]) {
        self.0.puts(&format(fmt, args));
    }
    fn i(&mut self, fmt: &core::ffi::CStr, v: i32) {
        self.w(fmt, &[Arg::I(v)]);
    }
    fn u(&mut self, fmt: &core::ffi::CStr, v: u32) {
        self.w(fmt, &[Arg::U(v as u64)]);
    }
    fn f(&mut self, fmt: &core::ffi::CStr, v: f32) {
        self.w(fmt, &[Arg::F(v as f64)]);
    }
}

/// PersistLiveControlsUiState: vrport.ini rewritten whole from the snapshot and the live keys; false when the file
/// could not be opened or written.
///
/// # Safety
/// `state` is a LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_persist_live_controls_ui_state(state: *const LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_ref() }) else {
        return false;
    };
    let r = rt();
    (r.init_runtime_paths)();
    // SAFETY: the C++ half's path.
    let Some(file) = (unsafe { cvr_crt::File::open_shared_mode(r.live_control_path, c"w") }) else {
        return false;
    };
    let mut o = Out(file);
    let sub = |o: &mut Out, which: i32| (r.subsystem_write)(which, o.0.as_ptr(), st);

    o.i(c"xr_weapon_laser_dot=%d\n", b(st.xrWeaponLaserDot));
    o.f(c"xr_head_offset_x=%.4f\n", st.xrHeadOffsetX);
    o.f(c"xr_head_offset_y=%.4f\n", st.xrHeadOffsetY);
    o.f(c"xr_head_offset_z=%.4f\n", st.xrHeadOffsetZ);
    o.w(c"xr_recenter=0\n", &[]);
    o.i(c"xr_mono_submit=%d\n", b(st.xrMonoSubmit != 0));
    // From the global because it is not a UI control -- written all the same: a key left out is a key deleted.
    o.i(
        c"xr_threaded_submit=%d\n",
        g!(CyberpunkVR_ThreadedMonoSubmit),
    );
    o.f(c"xr_force_fov=%.3f\n", st.xrForceFov);
    o.i(c"xr_menu_rect=%d\n", b(st.xrMenuRect != 0));
    o.f(c"xr_menu_fov=%.3f\n", st.xrMenuFov);
    o.f(
        c"xr_menu_follow_deg=%.3f\n",
        if st.xrMenuFollowDeg >= 5.0 {
            st.xrMenuFollowDeg
        } else {
            60.0
        },
    );
    sub(&mut o, FRAMEGEN);
    sub(&mut o, FOVEATED);
    sub(&mut o, VRS);
    sub(&mut o, HIDDEN_AREA);
    o.i(c"xr_mono_cascades=%d\n", b(st.monoCascades != 0));
    o.i(c"xr_mono_mode=%d\n", b(st.monoMode != 0));
    o.i(c"xr_afw=%d\n", b(st.afw != 0));
    o.i(c"xr_osr=%d\n", b(st.osr != 0));
    o.i(c"xr_scope_stabilize=%d\n", st.scopeStabilize);
    // SAFETY: Rust's own function under its C name.
    o.i(c"xr_nvidia_reflex=%d\n", unsafe {
        cvr_reflex_normalize_mode(st.nvidiaReflex)
    });
    o.i(c"xr_fps_cap=%d\n", b(st.fpsCap != 0));
    o.i(c"xr_lobby=%d\n", b(st.lobby != 0));
    o.i(c"xr_lobby_ambient=%d\n", b(st.lobbyAmbient != 0));
    sub(&mut o, OVERLAY);
    o.w(
        c"xr_hud_panel=%d\nxr_hud_follow_deg=%.3f\nxr_hud_fov=%.3f\nxr_hud_distance=%.3f\n",
        &[
            Arg::I(st.xrHudPanel),
            Arg::F(st.xrHudFollowDeg as f64),
            Arg::F(st.xrHudFov as f64),
            Arg::F(st.xrHudDistance as f64),
        ],
    );
    o.w(
        c"xr_interaction_panel=%d\nxr_interaction_follow_deg=%.3f\nxr_interaction_distance=%.3f\nxr_interaction_fov=%.3f\n",
        &[
            Arg::I(st.xrInteractionPanel),
            Arg::F(st.xrInteractionFollowDeg as f64),
            Arg::F(st.xrInteractionDistance as f64),
            Arg::F(st.xrInteractionFov as f64),
        ],
    );
    o.f(c"xr_loot_follow_deg=%.3f\n", st.xrLootFollowDeg);
    o.w(
        c"xr_hud_follow_mode=%d\nxr_hud_stereo_depth=%d\nxr_hud_brightness=%.3f\nxr_hud_shadow=%.3f\nxr_hud_glow=%.3f\n",
        &[
            Arg::I(st.xrHudFollowMode),
            Arg::I(st.xrHudStereoDepth),
            Arg::F(st.xrHudBrightness as f64),
            Arg::F(st.xrHudShadow as f64),
            Arg::F(st.xrHudGlow as f64),
        ],
    );
    sub(&mut o, HUD);
    o.i(c"xr_3dof_movement=%d\n", b(st.xr3DofMovement != 0));
    // Not a control, but it MUST be written back: left out, the next launch would read the default 1 and re-install
    // the shipped settings over the player's.
    o.i(c"first_launch=%d\n", b(st.xrFirstLaunch != 0));
    o.f(c"xr_motion_predict_ms=%.2f\n", st.xrMotionPredictMs);
    o.f(c"xr_stereo_scale=%.3f\n", st.xrStereoScale);
    o.f(c"xr_world_scale=%.3f\n", st.xrWorldScale);
    o.f(c"xr_ipd_scale=%.3f\n", st.xrIpdScale);
    o.f(c"xr_sharpness=%.3f\n", st.xrSharpness);
    o.f(c"xr_sharpmix=%.3f\n", st.xrSharpmix);
    o.i(c"xr_reuse_last_frame=%d\n", b(st.xrReuseLastFrame != 0));
    o.i(c"xr_pair_lock=%d\n", b(st.xrPairLock != 0));
    o.i(c"xr_render_pose_submit=%d\n", b(st.xrRenderPoseSubmit != 0));
    o.i(c"xr_depth_submit=%d\n", b(lc!(xrDepthSubmit) != 0));
    o.i(c"xr_pose_lag=%d\n", st.xrPoseLag);
    o.i(c"xr_runtime=%d\n", b(st.xrRuntime == 1));
    // SAFETY: the C++ half's accessor.
    o.f(c"xr_hmd_smooth=%.3f\n", unsafe { GetHmdTrackingSmooth() });
    o.f(c"xr_hand_lerp_speed=%.3f\n", g!(CyberpunkVR_HandLerpSpeed));
    o.f(
        c"xr_hand_lerp_pistol=%.3f\n",
        g!(CyberpunkVR_HandLerpPistol),
    );
    o.f(c"xr_hand_lerp_rifle=%.3f\n", g!(CyberpunkVR_HandLerpRifle));
    o.f(
        c"xr_hand_lerp_shotgun=%.3f\n",
        g!(CyberpunkVR_HandLerpShotgun),
    );
    o.f(
        c"xr_hand_lerp_sniper=%.3f\n",
        g!(CyberpunkVR_HandLerpSniper),
    );
    o.f(c"xr_hand_lerp_melee=%.3f\n", g!(CyberpunkVR_HandLerpMelee));
    o.f(
        c"xr_hand_lerp_framegen_scale=%.3f\n",
        g!(CyberpunkVR_HandLerpFramegenScale),
    );
    o.u(
        c"xr_grade_mirror_mask=%X\n",
        g!(CyberpunkVR_GradeMirrorMask),
    );
    o.u(c"xr_env_extra_mask=%u\n", g!(CyberpunkVR_EnvExtraMask));
    o.u(c"xr_water_mirror=%u\n", g!(CyberpunkVR_WaterMirror));
    o.u(c"xr_water_assign=%u\n", g!(CyberpunkVR_WaterAssign));
    o.i(c"xr_viewdata_diff=%d\n", g!(CyberpunkVR_ViewDataDiff));
    o.u(c"xr_lut_src_lend=%u\n", g!(CyberpunkVR_LutSrcLend));
    o.f(
        c"xr_scene_stick_yaw_dps=%.2f\n",
        g!(CyberpunkVR_SceneStickYawDps),
    );
    o.i(
        c"xr_scene_stick_yaw_tier=%d\n",
        g!(CyberpunkVR_SceneStickYawTier),
    );
    o.i(c"xr_block_diff=%d\n", g!(CyberpunkVR_BlockDiff));
    o.u(c"xr_block_diff_node=%X\n", g!(CyberpunkVR_BlockDiffNode));
    o.u(c"xr_block_diff_size=%u\n", g!(CyberpunkVR_BlockDiffSize));
    o.u(
        c"xr_render_mask_grant=%u\n",
        g!(CyberpunkVR_RenderMaskGrant),
    );
    o.i(c"xr_stage_replay=%d\n", g!(CyberpunkVR_StageReplaySetting));
    o.i(
        c"xr_vrcam_gbuffer_partitions=%d\n",
        g!(CyberpunkVR_VrcamGbufferPartitionsSetting),
    );
    o.i(
        c"xr_scope_gbuffer_partitions=%d\n",
        g!(CyberpunkVR_ScopeGbufferPartitionsSetting),
    );
    o.i(
        c"xr_scope_graph_dag=%d\n",
        b(g!(CyberpunkVR_ScopeGraphDag) != 0),
    );
    o.i(c"xr_scope_rr_full_resolution=%d\n", b(crate::scope_resolution::FULL_RESOLUTION.load(Relaxed)));
    o.i(c"xr_submit_chain=%d\n", g!(CyberpunkVR_SubmitChainSetting));
    o.i(
        c"xr_defer_present=%d\n",
        g!(CyberpunkVR_DeferPresentSetting),
    );
    o.i(
        c"xr_gt_skip_render_queue=%d\n",
        g!(CyberpunkVR_GtSkipRenderQueueSetting),
    );
    // SAFETY: the C++ half's int.
    o.i(c"xr_submit_chain_rev=%d\n", unsafe {
        core::ptr::read_volatile(r.submit_chain_rev)
    });
    o.u(
        c"xr_viewdata_fix_mask=%X\n",
        g!(CyberpunkVR_ViewDataFixMask),
    );
    o.u(c"xr_env_mirror_mask=%X\n", g!(CyberpunkVR_EnvMirrorMask));
    o.i(c"xr_bd_cam_dirty=%d\n", g!(CyberpunkVR_BdCamDirty));
    o.i(
        c"xr_bd_push_transform=%d\n",
        g!(CyberpunkVR_BdPushTransform),
    );
    o.i(
        c"xr_bd_main_pos_from_scene=%d\n",
        g!(CyberpunkVR_BdMainPosFromScene),
    );
    o.i(c"xr_bd_push_base=%d\n", g!(CyberpunkVR_BdPushBase));
    o.i(
        c"xr_bd_one_composition=%d\n",
        g!(CyberpunkVR_BdOneComposition),
    );
    o.i(c"xr_bd_editor_align=%d\n", g!(CyberpunkVR_BdEditorAlign));
    o.i(
        c"xr_bd_base_from_locate=%d\n",
        g!(CyberpunkVR_BdBaseFromLocate),
    );
    o.u(c"xr_comp_lend_set=%u\n", g!(CyberpunkVR_CompLendSet));
    o.i(c"xr_final_grab=%d\n", g!(CyberpunkVR_FinalGrab));
    o.i(c"xr_stable_srgb_view=%d\n", g!(CyberpunkVR_StableSrgbView));
    o.i(
        c"xr_bd_quat_from_write_site=%d\n",
        g!(CyberpunkVR_BdQuatFromWriteSite),
    );
    o.f(c"xr_dev_cam_tol_m=%.2f\n", g!(CyberpunkVR_DevCamTolM));
    o.i(c"xr_dev_cam_any_name=%d\n", g!(CyberpunkVR_DevCamAnyName));
    o.i(c"xr_dev_cam_in_locate=%d\n", g!(CyberpunkVR_DevCamInLocate));
    o.i(c"xr_lens_head_write=%d\n", g!(CyberpunkVR_LensHeadWrite));
    o.i(
        c"xr_input_default_in_ui=%d\n",
        g!(CyberpunkVR_InputDefaultInUi),
    );
    o.i(
        c"xr_popup_mag_block_ms=%d\n",
        g!(CyberpunkVR_PopupMagBlockMs),
    );
    o.i(
        c"xr_vrcam_pos_from_main=%d\n",
        g!(CyberpunkVR_VrcamPosFromMain),
    );
    // Hex: the value is an RVA, read straight off a log line printed in hex.
    o.u(c"xr_vision_cs_node=%X\n", g!(CyberpunkVR_VisionCsNode));
    o.i(c"xr_vision_cs_swap=%d\n", g!(CyberpunkVR_VisionCsSwap));
    o.i(c"xr_vision_cs_dim=%d\n", g!(CyberpunkVR_VisionCsDim));
    o.i(c"xr_vision_cs_ord=%d\n", g!(CyberpunkVR_VisionCsOrd));
    o.i(
        c"xr_weapon_plane_as_scene=%d\n",
        g!(CyberpunkVR_WeaponPlaneAsScene),
    );
    o.i(
        c"xr_highlight_any_plane=%d\n",
        g!(CyberpunkVR_HighlightAnyPlane),
    );
    o.i(
        c"xr_plane_bucket_scene=%d\n",
        g!(CyberpunkVR_PlaneBucketScene),
    );
    o.i(c"xr_bd_ipd_in_locate=%d\n", g!(CyberpunkVR_BdIpdInLocate));
    o.i(
        c"xr_bd_quat_from_buffer=%d\n",
        g!(CyberpunkVR_BdQuatFromBuffer),
    );
    o.u(c"xr_cap_grant=%u\n", g!(CyberpunkVR_CapGrant));
    o.u(c"xr_force_vrcam_cam=%u\n", g!(CyberpunkVR_ForceVrcamCam));
    o.u(c"xr_vrcam_cam_fields=%X\n", g!(CyberpunkVR_VrcamCamFields));
    o.u(c"xr_run_view_params=%u\n", g!(CyberpunkVR_RunViewParams));
    o.i(c"xr_expo_probe=%d\n", g!(CyberpunkVR_ExpoProbe));
    o.i(c"xr_indirect_census=%d\n", g!(CyberpunkVR_IndirectCensus));
    o.i(c"xr_no_state_lies=%d\n", g!(CyberpunkVR_NoStateLies));
    o.i(c"xr_stable_copy=%d\n", g!(CyberpunkVR_StableCopy));
    o.u(c"xr_distant_reuse=%u\n", g!(CyberpunkVR_DistantReuseMode));
    o.u(
        c"xr_local_shadow_reuse=%u\n",
        g!(CyberpunkVR_LocalShadowReuseMode),
    );
    o.u(c"xr_gi_reuse=%u\n", g!(CyberpunkVR_GiReuseMode));
    o.u(c"xr_probe_reuse=%u\n", g!(CyberpunkVR_ProbeReuseMode));
    o.i(c"xr_expo_mirror=%d\n", g!(CyberpunkVR_ExpoMirror));
    o.u(c"xr_expo_field_mask=%u\n", g!(CyberpunkVR_ExpoFieldMask));
    o.u(c"xr_comp_lend_scoped=%u\n", g!(CyberpunkVR_CompLendScoped));
    o.i(c"xr_light_content=%d\n", g!(CyberpunkVR_LightContent));
    o.i(c"xr_fix_lod=%d\n", g!(CyberpunkVR_FixLodEnable));
    o.i(c"xr_hud_to_second_eye=%d\n", g!(CyberpunkVR_HudToSecondEye));
    // SAFETY: the C++ half's int.
    o.i(c"xr_vive_trackers=%d\n", unsafe {
        core::ptr::read_volatile(r.vive_trackers_setting)
    });
    o.i(
        c"xr_interaction_anchor=%d\n",
        b(st.xrInteractionAnchor != 0),
    );
    o.i(c"xr_hud_clearance=%d\n", b(st.xrHudClearance != 0));
    o.f(c"xr_hud_clearance_grow=%.2f\n", st.xrHudClearanceGrow);
    o.i(
        c"xr_hud_clearance_hands=%d\n",
        b(st.xrHudClearanceHands != 0),
    );
    o.f(
        c"xr_hud_clearance_hands_scale=%.2f\n",
        st.xrHudClearanceHandsScale,
    );
    o.i(c"xr_auto_recenter=%d\n", g!(CyberpunkVR_AutoRecenter));
    o.i(c"xr_finger_bones=%d\n", b(st.xrFingerBones != 0));
    o.i(c"xr_grip_from_fingers=%d\n", b(st.xrGripFromFingers != 0));
    o.f(c"xr_finger_spread=%.2f\n", st.xrFingerSpread);
    o.i(
        c"xr_finger_spread_enabled=%d\n",
        b(st.xrFingerSpreadOn != 0),
    );
    o.i(
        c"xr_asym_projection=%d\n",
        b(g!(CyberpunkVR_AsymProjection) == 1),
    );
    o.f(
        c"xr_asym_inner_guard=%.2f\n",
        g!(CyberpunkVR_AsymInnerGuard),
    );
    // Read once at start; written as the ini asked for it (the mono mode turns it off for a launch, not for good).
    // SAFETY: Rust's own function under its C name.
    o.i(
        c"xr_engine_stereo=%d\n",
        b(unsafe { cvr_engine_stereo_requested() }),
    );
    o.i(
        c"xr_vision_to_second_eye=%d\n",
        g!(CyberpunkVR_VisionToSecondEye),
    );
    o.i(
        c"xr_hud_in_braindance=%d\n",
        g!(CyberpunkVR_HudInBraindance),
    );
    o.u(
        c"xr_vrcam_composition=%u\n",
        g!(CyberpunkVR_VrcamCompositionGroup),
    );
    o.f(c"xr_two_hand_radius=%.3f\n", g!(CyberpunkVR_TwoHandRadius));
    o.i(c"xr_two_hand_aim=%d\n", g!(CyberpunkVR_TwoHandAim));
    o.f(
        c"xr_two_hand_break=%.3f\n",
        g!(CyberpunkVR_TwoHandBreakRadius),
    );
    o.f(
        c"xr_two_hand_aim_release_ms=%.1f\n",
        g!(CyberpunkVR_TwoHandAimReleaseMs),
    );
    o.f(c"xr_carry_radius=%.3f\n", g!(CyberpunkVR_CarryGripRadius));
    o.f(c"xr_recoil_rifle_rise=%.3f\n", g!(CyberpunkVR_RifleRiseMul));
    o.f(c"xr_recoil_rifle_back=%.3f\n", g!(CyberpunkVR_RifleBackMul));
    o.f(
        c"xr_recoil_shotgun_rise=%.3f\n",
        g!(CyberpunkVR_ShotgunRiseMul),
    );
    o.f(
        c"xr_recoil_shotgun_back=%.3f\n",
        g!(CyberpunkVR_ShotgunBackMul),
    );
    o.f(
        c"xr_recoil_sniper_rise=%.3f\n",
        g!(CyberpunkVR_SniperRiseMul),
    );
    o.f(
        c"xr_recoil_sniper_back=%.3f\n",
        g!(CyberpunkVR_SniperBackMul),
    );
    o.f(c"xr_recoil_scale=%.3f\n", g!(CyberpunkVR_RecoilScale));
    o.i(c"xr_psvr2_haptic_backend=%d\n", haptic_route::HAPTIC_ROUTE.requested());
    o.f(c"xr_openxr_haptic_gain=%.3f\n", haptic_route::legacy_gain());
    o.f(c"xr_openxr_vehicle_haptic_gain=%.3f\n", haptic_route::vehicle_gain());
    o.i(c"xr_vehicle_audio_channels=%d\n", haptic_route::vehicle_audio_channels());
    o.i(c"xr_vehicle_audio_require_grip=%d\n", haptic_route::vehicle_audio_require_grip() as i32);
    o.i(c"xr_haptics=%d\n", g!(CyberpunkVR_Haptics));
    o.f(c"xr_haptic_scale=%.3f\n", g!(CyberpunkVR_HapticScale));
    o.f(
        c"xr_recoil_pistol_back=%.3f\n",
        g!(CyberpunkVR_PistolBackMul),
    );
    o.f(c"xr_recoil_climb=%.3f\n", g!(CyberpunkVR_RecoilClimbFrac));
    o.f(
        c"xr_recoil_climb_max_deg=%.3f\n",
        g!(CyberpunkVR_RecoilClimbMaxDeg),
    );
    o.f(c"xr_recoil_climb_ms=%.1f\n", g!(CyberpunkVR_RecoilClimbMs));
    o.f(
        c"xr_recoil_return_ms=%.1f\n",
        g!(CyberpunkVR_HandRecoilReturnMs),
    );
    o.f(
        c"xr_recoil_return_pow=%.3f\n",
        g!(CyberpunkVR_HandRecoilReturnPow),
    );
    o.f(
        c"xr_recoil_down_pistol=%.3f\n",
        g!(CyberpunkVR_RecoilDownPistol),
    );
    o.f(
        c"xr_recoil_down_rifle=%.3f\n",
        g!(CyberpunkVR_RecoilDownRifle),
    );
    o.f(
        c"xr_recoil_down_shotgun=%.3f\n",
        g!(CyberpunkVR_RecoilDownShotgun),
    );
    o.f(
        c"xr_recoil_back_cm=%.3f\n",
        g!(CyberpunkVR_HandRecoilBackCm),
    );
    o.f(
        c"xr_recoil_back_max_cm=%.3f\n",
        g!(CyberpunkVR_HandRecoilBackMaxCm),
    );
    o.f(
        c"xr_recoil_twohand_back=%.3f\n",
        g!(CyberpunkVR_TwoHandBackMul),
    );
    // The scanner's movable pieces, x,y,scale each, in 1920x1080 design pixels (the in-game editor writes them).
    let slots = g!(CyberpunkVR_ScannerSlots);
    for (i, key) in [
        c"xr_scanner_frame",
        c"xr_scanner_details",
        c"xr_scanner_hacks",
        c"xr_scanner_hint",
        c"xr_scanner_memory",
        c"xr_scanner_scripts",
        c"xr_scanner_desc",
    ]
    .iter()
    .enumerate()
    {
        o.w(
            c"%s=%.1f,%.1f,%.3f\n",
            &[
                Arg::S(key.as_ptr()),
                Arg::F(slots[i * 3] as f64),
                Arg::F(slots[i * 3 + 1] as f64),
                Arg::F(slots[i * 3 + 2] as f64),
            ],
        );
    }
    o.i(c"xr_movement_control=%d\n", b(st.xrMovementControl != 0));
    o.i(c"xr_disable_mouse_y=%d\n", b(st.xrDisableMouseY != 0));
    o.i(c"xr_xinput_hook=%d\n", b(st.xrXInputHook != 0));
    o.i(c"xr_snap_turn=%d\n", b(st.xrSnapTurn != 0));
    o.f(
        c"xr_snap_turn_angle_deg=%.2f\n",
        if st.xrSnapTurnAngleDeg > 0.0 {
            st.xrSnapTurnAngleDeg
        } else {
            30.0
        },
    );
    o.i(
        c"xr_movement_source=%d\n",
        if st.xrMovementSource < 0 {
            0
        } else if st.xrMovementSource > 3 {
            3
        } else {
            st.xrMovementSource
        },
    );
    o.i(c"xr_sprint_mode=%d\n", b(st.xrSprintMode == 1));
    // SAFETY: Rust's own functions under their C names.
    let (left, right, full) = unsafe {
        (
            cvr_stick_deadzone(st.xrLeftStickDeadzone),
            cvr_stick_deadzone(st.xrRightStickDeadzone),
            cvr_stick_full_input(st.xrMaxInputThreshold),
        )
    };
    o.w(
        c"xr_movement_speed_mode=%d\nxr_left_stick_deadzone=%.3f\nxr_right_stick_deadzone=%.3f\nxr_max_input_threshold=%.3f\n",
        &[Arg::I(b(st.xrMovementSpeedMode == 1)), Arg::F(left as f64), Arg::F(right as f64), Arg::F(full as f64)],
    );
    o.i(
        c"xr_physical_body_rotation=%d\n",
        b(st.xrPhysicalBodyRotation != 0),
    );
    o.i(
        c"xr_tracked_body_rotation=%d\n",
        b(st.xrTrackedBodyRotation != 0),
    );
    o.i(
        c"xr_hybrid_body_rotation=%d\n",
        b(st.xrHybridBodyRotation != 0),
    );
    o.i(c"xr_head_led_body_rotation=%d\n", b(st.xrHeadLedBodyRotation != 0));
    o.i(
        c"xr_roomscale_movement=%d\n",
        b(st.xrRoomscaleMovement != 0),
    );
    o.i(c"xr_physical_crouch=%d\n", b(st.xrPhysicalCrouch != 0));
    // SAFETY: as above.
    o.f(c"xr_physical_crouch_depth=%.3f\n", unsafe {
        cvr_crouch_threshold(st.xrPhysicalCrouchDepth)
    });
    o.i(c"xr_breaststroke_swim=%d\n", b(st.xrBreaststrokeSwim != 0));
    o.i(c"xr_ladder_grip_climb=%d\n", b(st.xrLadderGripClimb != 0));
    o.i(c"xr_left_handed=%d\n", b(st.xrLeftHanded != 0));
    o.i(
        c"xr_left_handed_bindings=%d\n",
        st.xrLeftHandedBindings.clamp(0, 2),
    );
    o.w(
        c"xr_ladder_auto_finish=%d\nxr_ladder_finish_distance=%.3f\n",
        &[
            Arg::I(b(st.xrLadderAutoFinish != 0)),
            Arg::F(st.xrLadderFinishDistance as f64),
        ],
    );
    o.f(c"xr_body_free_look_deg=%.3f\n", st.xrBodyFreeLookDeg);
    o.f(
        c"xr_body_free_look_down_deg=%.3f\n",
        st.xrBodyFreeLookDownDeg,
    );
    o.f(
        c"xr_body_free_look_swim_deg=%.3f\n",
        st.xrBodyFreeLookSwimDeg,
    );
    o.f(c"xr_body_move_radius=%.4f\n", st.xrBodyMoveRadius);
    o.f(c"xr_scene_cam_comfort_deg=%.1f\n", st.xrSceneCamComfortDeg);
    o.i(
        c"xr_cutscene_suspend_tier=%d\n",
        if st.xrCutsceneSuspendTier < -1 {
            -1
        } else if st.xrCutsceneSuspendTier > 4 {
            4
        } else {
            st.xrCutsceneSuspendTier
        },
    );
    // SAFETY: as above.
    let vehicle_tier = unsafe {
        cvr_resolve_vehicle_vrik_suspend_tier(
            st.xrCutsceneSuspendTier,
            st.xrVehicleCutsceneSuspendTier,
            true,
        )
    };
    o.i(c"xr_vehicle_cutscene_suspend_tier=%d\n", vehicle_tier);
    o.i(c"xr_xinput_install=%d\n", b(st.xrXInputInstall != 0));
    o.i(c"xr_input_actions=%d\n", b(st.xrInputActions != 0));
    o.i(c"xr_mono_xqueue_wait=%d\n", b(st.xrMonoXQueueWait != 0));
    o.i(c"xr_mono_depth_capture=%d\n", b(st.xrMonoDepthCapture != 0));
    o.i(
        c"xr_snap_turn_pulse_ms=%d\n",
        if st.xrSnapTurnPulseMs > 0 {
            st.xrSnapTurnPulseMs
        } else {
            30
        },
    );
    o.i(
        c"xr_immersive_holsters=%d\n",
        b(st.xrImmersiveHolsters != 0),
    );
    o.i(c"xr_body_hands_only=%d\n", b(st.xrBodyHandsOnly != 0));
    o.i(c"xr_reload_glow=%d\n", b(st.xrReloadGlow != 0));
    o.f(c"xr_veh_head_offset_x=%.4f\n", st.xrVehHeadOffsetX);
    o.f(c"xr_veh_head_offset_y=%.4f\n", st.xrVehHeadOffsetY);
    o.f(c"xr_veh_head_offset_z=%.4f\n", st.xrVehHeadOffsetZ);
    o.f(c"xr_bike_head_offset_x=%.4f\n", st.xrBikeHeadOffsetX);
    o.f(c"xr_bike_head_offset_y=%.4f\n", st.xrBikeHeadOffsetY);
    o.f(c"xr_bike_head_offset_z=%.4f\n", st.xrBikeHeadOffsetZ);
    o.i(c"xr_wheel_grab=%d\n", b(st.xrWheelGrab != 0));
    o.i(c"xr_menu_gesture=%d\n", b(st.xrMenuGesture != 0));
    o.i(c"xr_vr_interface=%d\n", b(st.xrVrInterface != 0));
    o.i(c"xr_hand_physics=%d\n", b(st.xrHandPhysics != 0));
    o.i(c"xr_vr_weapon_wheel=%d\n", b(st.xrVrWeaponWheel != 0));
    o.i(c"xr_hand_collision=%d\n", b(st.xrHandCollision != 0));
    o.i(c"xr_hand_collision_vehicle=%d\n", b(st.xrHandCollisionVehicle != 0));
    o.i(c"xr_hand_push=%d\n", b(st.xrHandPush != 0));
    o.i(c"xr_finger_contact=%d\n", b(st.xrFingerContact != 0));
    o.i(c"xr_prop_grab=%d\n", b(st.xrPropGrab != 0));
    o.i(c"xr_carry_picture=%d\n", b(st.xrCarryPicture != 0));
    o.f(c"xr_carry_lead=%.3f\n", st.xrCarryLead);
    o.f(c"xr_contact_margin=%.3f\n", st.xrContactMargin);
    o.i(c"xr_wheel_anywhere=%d\n", b(st.xrWheelAnywhere != 0));
    o.f(
        c"xr_wheel_radius=%.3f\n",
        if st.xrWheelRadius > 0.0 {
            st.xrWheelRadius
        } else {
            0.28
        },
    );
    o.f(
        c"xr_wheel_steer_max_deg=%.1f\n",
        wheel_full_lock_degrees(st.xrWheelSteerMaxDeg),
    );
    o.f(c"xr_bike_bar_lock_deg=%.1f\n", bike_bar_lock_degrees(st.xrBikeBarLockDeg));
    o.f(c"xr_bike_body_lean=%.2f\n", bike_body_lean_share(st.xrBikeBodyLean));
    o.f(
        c"xr_wheel_steer_dead_deg=%.1f\n",
        if st.xrWheelSteerDeadDeg >= 0.0 {
            st.xrWheelSteerDeadDeg
        } else {
            1.5
        },
    );
    o.i(c"xr_wheel_prediction=%d\n", b(st.xrWheelPrediction != 0));
    o.f(
        c"xr_wheel_prediction_ms=%.1f\n",
        if st.xrWheelPredictionMs.is_finite() {
            cpp_clamp(st.xrWheelPredictionMs, 0.0, 8.0)
        } else {
            0.0
        },
    );
    o.i(c"xr_wheel_horn2=%d\n", b(st.xrWheelHorn != 0));
    o.f(
        c"xr_wheel_horn_radius=%.3f\n",
        if st.xrWheelHornRadius > 0.0 {
            st.xrWheelHornRadius
        } else {
            0.12
        },
    );
    o.i(
        c"xr_vehicle_gun_trigger=%d\n",
        b(st.xrVehicleGunTrigger != 0),
    );
    o.f(
        c"xr_vehicle_throttle_trim=%.2f\n",
        if st.xrVehicleThrottleTrim > 0.0 {
            st.xrVehicleThrottleTrim
        } else {
            0.5
        },
    );
    let written = o.0.close();

    if let Some(time) = crate::live_controls_poll::file_time(r.live_control_path) {
        // SAFETY: the C++ half's FILETIME.
        unsafe { *r.last_live_control_write = time };
    }
    written
}

// ---- the overlay's edits ----------------------------------------------------------------------------------------------

/// SetLiveControlsUiState: the overlay's snapshot applied with the ini path's clamps, and saved when asked.
///
/// # Safety
/// `state` is null or a LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn SetLiveControlsUiState(
    state: *const LiveControlsUiState,
    persist_to_file: i32,
) {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_ref() }) else {
        return;
    };
    let r = rt();
    (r.subsystem_set)(FRAMEGEN, st);
    (r.subsystem_set)(FOVEATED, st);
    (r.subsystem_set)(VRS, st);
    (r.subsystem_set)(HIDDEN_AREA, st);
    haptic_route::HAPTIC_ROUTE.request(st.xrPsvr2HapticBackend);
    haptic_route::set_legacy_gain(st.xrOpenXrHapticGain);
    haptic_route::set_vehicle_gain(st.xrOpenXrVehicleHapticGain);
    haptic_route::set_vehicle_audio_channels(st.xrVehicleAudioChannels);
    haptic_route::set_vehicle_audio_require_grip(st.xrVehicleAudioRequireGrip);
    // SAFETY: the C++ half's atomic; Rust's own functions under their C names.
    unsafe {
        CyberpunkVR_MonoCascades.store(b(st.monoCascades != 0) as u32, Relaxed);
        cvr_engine_stereo_apply_mono_mode(st.monoMode != 0);
    }
    s!(
        CyberpunkVR_Afw,
        if st.osr != 0 {
            2
        } else if st.afw != 0 {
            1
        } else {
            0
        }
    );
    if crate::afw::cvr_afw_launch() {
        crate::afw::cvr_afw_set_mode(g!(CyberpunkVR_Afw));
    }
    // SAFETY: as above.
    unsafe {
        cvr_optics_source_set_stabilize(st.scopeStabilize);
        cvr_reflex_set_mode(st.nvidiaReflex);
        cvr_framegen_set_fps_cap(st.fpsCap != 0);
    }
    crate::menu_lobby::cvr_menu_lobby_set_enabled(st.lobby != 0);
    crate::menu_lobby::cvr_menu_lobby_set_ambient(st.lobbyAmbient != 0);
    (r.subsystem_set)(OVERLAY, st);

    let prev_mono = lc!(xrMonoSubmit);

    // SAFETY: the C++ half's bool.
    unsafe { core::ptr::write_volatile(r.draw_barrel_cross, st.xrWeaponLaserDot) };
    lc_set!(xrHeadOffsetX, st.xrHeadOffsetX);
    lc_set!(xrHeadOffsetY, st.xrHeadOffsetY);
    lc_set!(xrHeadOffsetZ, st.xrHeadOffsetZ);
    lc_set!(xrRecenter, 0);
    lc_set!(xrMonoSubmit, b(st.xrMonoSubmit != 0));
    lc_set!(
        xrForceFov,
        if st.xrForceFov > 0.0 {
            st.xrForceFov
        } else {
            0.0
        }
    );
    lc_set!(xrHudPanel, b(st.xrHudPanel != 0));
    lc_set!(xrHudFollowMode, b(st.xrHudFollowMode == 1));
    lc_set!(xrHudStereoDepth, b(st.xrHudStereoDepth != 0));
    // SAFETY: as above.
    unsafe { cvr_fingergrip_set_grip_from_fingers(st.xrGripFromFingers != 0) };
    s!(CyberpunkVR_FingerBones, b(st.xrFingerBones != 0));
    s!(
        CyberpunkVR_FingerSpread,
        cpp_clamp(st.xrFingerSpread, 0.0, 2.0)
    );
    s!(CyberpunkVR_FingerSpreadOn, b(st.xrFingerSpreadOn != 0));
    s!(CyberpunkVR_HudClearance, b(st.xrHudClearance != 0));
    s!(
        CyberpunkVR_HudClearanceGrow,
        cpp_clamp(st.xrHudClearanceGrow, 0.0, 1.0)
    );
    s!(
        CyberpunkVR_HudClearanceHands,
        b(st.xrHudClearanceHands != 0)
    );
    s!(
        CyberpunkVR_HudClearanceHandsScale,
        cpp_clamp(st.xrHudClearanceHandsScale, 1.0, 3.0)
    );
    s!(
        CyberpunkVR_InteractionAnchor,
        b(st.xrInteractionAnchor != 0)
    );
    lc_set!(
        xrHudBrightness,
        if st.xrHudBrightness >= 0.25 && st.xrHudBrightness <= 3.0 {
            st.xrHudBrightness
        } else {
            1.0
        }
    );
    lc_set!(
        xrHudShadow,
        if st.xrHudShadow >= 0.0 && st.xrHudShadow <= 2.0 {
            st.xrHudShadow
        } else {
            1.0
        }
    );
    lc_set!(
        xrHudGlow,
        if st.xrHudGlow >= 0.0 && st.xrHudGlow <= 2.0 {
            st.xrHudGlow
        } else {
            1.0
        }
    );
    (r.subsystem_set)(HUD, st);
    lc_set!(
        xrHudFollowDeg,
        if st.xrHudFollowDeg >= 5.0 && st.xrHudFollowDeg <= 90.0 {
            st.xrHudFollowDeg
        } else {
            60.0
        }
    );
    lc_set!(xrInteractionPanel, b(st.xrInteractionPanel != 0));
    lc_set!(
        xrInteractionFollowDeg,
        if st.xrInteractionFollowDeg >= 0.0 && st.xrInteractionFollowDeg <= 90.0 {
            st.xrInteractionFollowDeg
        } else {
            0.0
        }
    );
    lc_set!(
        xrLootFollowDeg,
        if st.xrLootFollowDeg >= 0.0 && st.xrLootFollowDeg <= 90.0 {
            st.xrLootFollowDeg
        } else {
            0.0
        }
    );
    lc_set!(
        xrInteractionDistance,
        if st.xrInteractionDistance >= 0.5 && st.xrInteractionDistance <= 5.0 {
            st.xrInteractionDistance
        } else {
            2.0
        }
    );
    lc_set!(
        xrInteractionFov,
        if st.xrInteractionFov >= 30.0 && st.xrInteractionFov <= 120.0 {
            st.xrInteractionFov
        } else {
            75.0
        }
    );
    lc_set!(
        xrHudFov,
        if st.xrHudFov >= 30.0 && st.xrHudFov <= 120.0 {
            st.xrHudFov
        } else {
            65.4
        }
    );
    lc_set!(
        xrHudDistance,
        if st.xrHudDistance >= 0.5 && st.xrHudDistance <= 5.0 {
            st.xrHudDistance
        } else {
            1.5
        }
    );
    lc_set!(xrMenuRect, b(st.xrMenuRect != 0));
    lc_set!(
        xrMenuFov,
        if st.xrMenuFov > 1.0 {
            st.xrMenuFov
        } else {
            65.0
        }
    );
    lc_set!(
        xrMenuFollowDeg,
        if st.xrMenuFollowDeg >= 5.0 && st.xrMenuFollowDeg <= 90.0 {
            st.xrMenuFollowDeg
        } else {
            60.0
        }
    );
    lc_set!(xr3DofMovement, b(st.xr3DofMovement != 0));
    lc_set!(xrFirstLaunch, b(st.xrFirstLaunch != 0));
    lc_set!(
        xrMotionPredictMs,
        if st.xrMotionPredictMs >= 0.0 {
            st.xrMotionPredictMs
        } else {
            0.0
        }
    );
    lc_set!(
        xrStereoScale,
        if st.xrStereoScale < 0.0 {
            0.0
        } else if st.xrStereoScale > 10.0 {
            10.0
        } else {
            st.xrStereoScale
        }
    );
    lc_set!(
        xrWorldScale,
        if st.xrWorldScale < 0.05 {
            0.05
        } else if st.xrWorldScale > 20.0 {
            20.0
        } else {
            st.xrWorldScale
        }
    );
    lc_set!(
        xrIpdScale,
        if st.xrIpdScale < 0.0 {
            0.0
        } else if st.xrIpdScale > 5.0 {
            5.0
        } else {
            st.xrIpdScale
        }
    );
    lc_set!(
        xrSharpness,
        if st.xrSharpness < 0.0 {
            0.0
        } else if st.xrSharpness > 1.0 {
            1.0
        } else {
            st.xrSharpness
        }
    );
    lc_set!(
        xrSharpmix,
        if st.xrSharpmix < 0.0 {
            0.0
        } else if st.xrSharpmix > 1.0 {
            1.0
        } else {
            st.xrSharpmix
        }
    );
    lc_set!(xrReuseLastFrame, b(st.xrReuseLastFrame != 0));
    lc_set!(xrPairLock, b(st.xrPairLock != 0));
    lc_set!(xrRenderPoseSubmit, b(st.xrRenderPoseSubmit != 0));
    lc_set!(xrPoseLag, st.xrPoseLag);
    lc_set!(xrRuntime, b(st.xrRuntime == 1));
    let source = if st.xrMovementSource < 0 || st.xrMovementSource > 3 {
        b(st.xrMovementControl != 0)
    } else {
        st.xrMovementSource
    };
    lc_set!(xrMovementSource, source);
    lc_set!(xrMovementControl, b(source != 0));
    lc_set!(xrPhysicalBodyRotation, b(st.xrPhysicalBodyRotation != 0));
    lc_set!(xrMovementSpeedMode, b(st.xrMovementSpeedMode == 1));
    lc_set!(xrSprintMode, b(st.xrSprintMode == 1));
    // SAFETY: as above.
    let (left, right, full) = unsafe {
        (
            cvr_stick_deadzone(st.xrLeftStickDeadzone),
            cvr_stick_deadzone(st.xrRightStickDeadzone),
            cvr_stick_full_input(st.xrMaxInputThreshold),
        )
    };
    lc_set!(xrLeftStickDeadzone, left);
    lc_set!(xrRightStickDeadzone, right);
    lc_set!(xrMaxInputThreshold, full);
    lc_set!(xrTrackedBodyRotation, b(st.xrTrackedBodyRotation != 0));
    lc_set!(xrHybridBodyRotation, b(st.xrHybridBodyRotation != 0));
    lc_set!(xrHeadLedBodyRotation, b(st.xrHeadLedBodyRotation != 0));
    // SAFETY: as above.
    let mode = unsafe {
        cvr_rotation_mode_from_flags(
            st.xrPhysicalBodyRotation,
            st.xrTrackedBodyRotation,
            st.xrHybridBodyRotation,
            st.xrHeadLedBodyRotation,
        )
    };
    lc_set!(xrBodyRotationMode, mode);
    lc_set!(xrRoomscaleMovement, b(st.xrRoomscaleMovement != 0));
    lc_set!(xrPhysicalCrouch, b(st.xrPhysicalCrouch != 0));
    // SAFETY: as above.
    let depth = unsafe { cvr_crouch_threshold(st.xrPhysicalCrouchDepth) };
    lc_set!(xrPhysicalCrouchDepth, depth);
    // Only on a real change: mode 2 is the probes' test projection and must survive an unrelated edit.
    if (st.xrAsymProjection != 0) != (g!(CyberpunkVR_AsymProjection) == 1) {
        s!(CyberpunkVR_AsymProjection, b(st.xrAsymProjection != 0));
    }
    s!(CyberpunkVR_Haptics, b(st.xrHaptics != 0));
    s!(
        CyberpunkVR_HapticScale,
        if st.xrHapticScale.is_finite() {
            cpp_clamp(st.xrHapticScale, 0.0, 2.0)
        } else {
            1.0
        }
    );
    lc_set!(xrBreaststrokeSwim, b(st.xrBreaststrokeSwim != 0));
    lc_set!(xrLadderGripClimb, b(st.xrLadderGripClimb != 0));
    lc_set!(xrLeftHanded, b(st.xrLeftHanded != 0));
    lc_set!(xrLeftHandedBindings, st.xrLeftHandedBindings.clamp(0, 2));
    lc_set!(xrLadderAutoFinish, b(st.xrLadderAutoFinish != 0));
    lc_set!(
        xrLadderFinishDistance,
        cpp_clamp(
            if st.xrLadderFinishDistance.is_finite() {
                st.xrLadderFinishDistance
            } else {
                1.0
            },
            0.2,
            1.2
        )
    );
    lc_set!(
        xrBodyFreeLookDeg,
        cpp_clamp(st.xrBodyFreeLookDeg, 0.0, 60.0)
    );
    lc_set!(
        xrBodyFreeLookDownDeg,
        cpp_clamp(
            if st.xrBodyFreeLookDownDeg.is_finite() {
                st.xrBodyFreeLookDownDeg
            } else {
                30.0
            },
            0.0,
            90.0
        )
    );
    lc_set!(
        xrBodyFreeLookSwimDeg,
        cpp_clamp(
            if st.xrBodyFreeLookSwimDeg.is_finite() {
                st.xrBodyFreeLookSwimDeg
            } else {
                5.0
            },
            0.0,
            90.0
        )
    );
    lc_set!(xrBodyMoveRadius, cpp_clamp(st.xrBodyMoveRadius, 0.0, 0.30));
    lc_set!(
        xrSceneCamComfortDeg,
        cpp_clamp(
            if st.xrSceneCamComfortDeg.is_finite() {
                st.xrSceneCamComfortDeg
            } else {
                30.0
            },
            0.0,
            45.0
        )
    );
    lc_set!(
        xrCutsceneSuspendTier,
        if st.xrCutsceneSuspendTier < -1 {
            -1
        } else if st.xrCutsceneSuspendTier > 4 {
            4
        } else {
            st.xrCutsceneSuspendTier
        }
    );
    // SAFETY: as above.
    let vehicle_tier = unsafe {
        cvr_resolve_vehicle_vrik_suspend_tier(
            st.xrCutsceneSuspendTier,
            st.xrVehicleCutsceneSuspendTier,
            true,
        )
    };
    lc_set!(xrVehicleCutsceneSuspendTier, vehicle_tier);
    lc_set!(xrDisableMouseY, b(st.xrDisableMouseY != 0));
    lc_set!(xrXInputHook, b(st.xrXInputHook != 0));
    lc_set!(xrSnapTurn, b(st.xrSnapTurn != 0));
    lc_set!(
        xrSnapTurnAngleDeg,
        if st.xrSnapTurnAngleDeg > 0.0 {
            st.xrSnapTurnAngleDeg
        } else {
            30.0
        }
    );
    lc_set!(xrXInputInstall, b(st.xrXInputInstall != 0));
    lc_set!(xrInputActions, b(st.xrInputActions != 0));
    lc_set!(xrMonoXQueueWait, b(st.xrMonoXQueueWait != 0));
    lc_set!(xrMonoDepthCapture, b(st.xrMonoDepthCapture != 0));
    lc_set!(
        xrSnapTurnPulseMs,
        if st.xrSnapTurnPulseMs > 0 {
            st.xrSnapTurnPulseMs
        } else {
            30
        }
    );
    lc_set!(xrImmersiveHolsters, b(st.xrImmersiveHolsters != 0));
    (r.set_immersive_holsters)(lc!(xrImmersiveHolsters));
    lc_set!(xrBodyHandsOnly, b(st.xrBodyHandsOnly != 0));
    (r.set_body_hands_only)(lc!(xrBodyHandsOnly));
    lc_set!(xrReloadGlow, b(st.xrReloadGlow != 0));
    cvr_xr::switches::SWITCHES.reload_glow.store(lc!(xrReloadGlow), Relaxed);
    // In-vehicle head offset: the same bound as the ini path, for the same reason.
    let cl = |v: f32| {
        if v < -1.0 {
            -1.0
        } else if v > 1.0 {
            1.0
        } else {
            v
        }
    };
    lc_set!(xrVehHeadOffsetX, cl(st.xrVehHeadOffsetX));
    lc_set!(xrVehHeadOffsetY, cl(st.xrVehHeadOffsetY));
    lc_set!(xrVehHeadOffsetZ, cl(st.xrVehHeadOffsetZ));
    lc_set!(xrBikeHeadOffsetX, cl(st.xrBikeHeadOffsetX));
    lc_set!(xrBikeHeadOffsetY, cl(st.xrBikeHeadOffsetY));
    lc_set!(xrBikeHeadOffsetZ, cl(st.xrBikeHeadOffsetZ));
    // DRIVING: the ini path's clamps -- the two entry points must not disagree about what a valid value is.
    lc_set!(xrWheelGrab, b(st.xrWheelGrab != 0));
    lc_set!(xrMenuGesture, b(st.xrMenuGesture != 0));
    lc_set!(xrVrWeaponWheel, b(st.xrVrWeaponWheel != 0));
    lc_set!(xrVrInterface, b(st.xrVrInterface != 0));
    lc_set!(xrHandPhysics, b(st.xrHandPhysics != 0));
    lc_set!(xrHandCollision, b(st.xrHandCollision != 0));
    lc_set!(xrHandCollisionVehicle, b(st.xrHandCollisionVehicle != 0));
    lc_set!(xrHandPush, b(st.xrHandPush != 0));
    lc_set!(xrFingerContact, b(st.xrFingerContact != 0));
    lc_set!(xrPropGrab, b(st.xrPropGrab != 0));
    lc_set!(xrDebugView, b(st.xrDebugView != 0));
    lc_set!(xrCarryPicture, b(st.xrCarryPicture != 0));
    lc_set!(xrCarryLead, if st.xrCarryLead < 0.0 { 0.0 } else if st.xrCarryLead > 0.1 { 0.1 } else { st.xrCarryLead });
    lc_set!(xrContactMargin, if st.xrContactMargin < 0.0 { 0.0 } else if st.xrContactMargin > 0.05 { 0.05 } else { st.xrContactMargin });
    lc_set!(xrWheelAnywhere, b(st.xrWheelAnywhere != 0));
    lc_set!(xrWheelHorn, b(st.xrWheelHorn != 0));
    lc_set!(xrVehicleGunTrigger, b(st.xrVehicleGunTrigger != 0));
    let rr = st.xrWheelRadius;
    lc_set!(
        xrWheelRadius,
        if rr < 0.08 {
            0.08
        } else if rr > 0.60 {
            0.60
        } else {
            rr
        }
    );
    lc_set!(
        xrWheelSteerMaxDeg,
        wheel_full_lock_degrees(st.xrWheelSteerMaxDeg)
    );
    lc_set!(xrBikeBarLockDeg, bike_bar_lock_degrees(st.xrBikeBarLockDeg));
    lc_set!(xrBikeBodyLean, bike_body_lean_share(st.xrBikeBodyLean));
    let d = st.xrWheelSteerDeadDeg;
    lc_set!(
        xrWheelSteerDeadDeg,
        if d < 0.0 {
            0.0
        } else if d > 20.0 {
            20.0
        } else {
            d
        }
    );
    lc_set!(xrWheelPrediction, b(st.xrWheelPrediction != 0));
    lc_set!(
        xrWheelPredictionMs,
        if st.xrWheelPredictionMs.is_finite() {
            cpp_clamp(st.xrWheelPredictionMs, 0.0, 8.0)
        } else {
            0.0
        }
    );
    let hr = st.xrWheelHornRadius;
    lc_set!(
        xrWheelHornRadius,
        if hr < 0.04 {
            0.04
        } else if hr > 0.30 {
            0.30
        } else {
            hr
        }
    );
    let tt = st.xrVehicleThrottleTrim;
    lc_set!(
        xrVehicleThrottleTrim,
        if tt < 0.05 {
            0.05
        } else if tt > 3.0 {
            3.0
        } else {
            tt
        }
    );
    // Publish the mouse-Y flag for the CET VRIK mod.
    crate::live_controls_poll::cvr_write_vrik_settings_file();

    if prev_mono != lc!(xrMonoSubmit) {
        (r.set_mono_submit_enabled)(lc!(xrMonoSubmit) != 0);
        log(&std::format!(
            "ImGui: OpenXR mono submit {}.\n",
            if lc!(xrMonoSubmit) != 0 {
                "enabled"
            } else {
                "disabled"
            }
        ));
    }
    if st.xrRecenter != 0 {
        crate::live_controls_poll::RequestLiveControlsRecenter();
    }

    if persist_to_file != 0 {
        (r.persist_live_controls)();
    }
}
