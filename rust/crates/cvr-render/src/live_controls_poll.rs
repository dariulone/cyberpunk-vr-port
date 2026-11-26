//! READING THE LIVE CONTROLS, ONCE PER TICK, OFF THE GAME THREADS (formerly src/Core/LiveControlsPoll.cpp's poll): the
//! CET VRIK mod's recenter request, then vrport.ini whenever its write time changes -- the loop's variables seeded from
//! the live state, the ini's lines read over them (cvr_settings::cvr_settings_parse_live_controls), and the result
//! applied: the live settings block, the subsystems, the hundred-odd live keys the other files read, with their
//! clamps, the one-time defaults revision and the applies that run only on a change. The F7/F8 hotkeys and the
//! overlay's recenter request with it.
//!
//! READ THE FILE, NOT THE SOURCE DEFAULT: a value that looks wrong is checked against vrport.ini, not against a
//! seed here.
//!
//! The live keys are the C names the other files read; the subsystems' settings stay C++ structs, seeded and applied
//! through the runtime (indices as cvr_cpp_settings_parse_subsystem: 0 hud, 1 framegen, 2 foveated, 3 vrs, 4 hidden
//! area, 5 overlay). The overlay's UI state (MakeLiveControlsUiState, PersistLiveControlsUiState,
//! SetLiveControlsUiState) is still the C++ half's.
#![allow(non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering::*};

use cvr_crt::{cpp_clamp, format, Arg};
use cvr_settings::live_controls::LiveControlsValues;
use cvr_settings::haptic_route;
use cvr_settings::wheel_range::{bike_bar_lock_degrees, bike_body_lean_share, wheel_full_lock_degrees};
use cvr_settings::{lc, lc_set};

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    init_runtime_paths: extern "C" fn(),
    /// OpenXRManager::RequestRecenter.
    request_recenter: extern "C" fn(),
    set_immersive_holsters: extern "C" fn(i32),
    set_body_hands_only: extern "C" fn(i32),
    set_mono_submit_enabled: extern "C" fn(bool),
    use_threaded_submit: extern "C" fn() -> bool,
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()).
    persist_live_controls: extern "C" fn() -> bool,
    /// LiveControlsSubsystems: its size and alignment, constructed and seeded in place, applied one subsystem at a
    /// time, destroyed.
    subsystems_size: usize,
    subsystems_align: usize,
    subsystems_seed: extern "C" fn(*mut c_void),
    subsystems_apply: extern "C" fn(i32, *const c_void),
    subsystems_drop: extern "C" fn(*mut c_void),
    live_control_path: *const c_char,
    last_live_control_write: *mut [u32; 2],
    vrik_recenter_path: *const c_char,
    last_vrik_recenter_write: *mut [u32; 2],
    last_vrik_recenter_counter: *mut i32,
    vrik_settings_path: *const c_char,
    /// overlay::g_drawBarrelCross.
    draw_barrel_cross: *mut bool,
    /// xr_vive_trackers, kept so a save keeps it.
    vive_trackers_setting: *mut i32,
    /// xr_submit_chain_rev as the save writes it.
    submit_chain_rev: *mut i32,
    verbose_log: *const i32,
}
/// The table, built here (formerly src/Core/LiveControlsPoll.cpp's).
mod table {
    use super::Runtime;
    use core::ffi::c_char;
    use core::sync::atomic::Ordering::Relaxed;
    unsafe extern "C" {
        /// OpenXRManager::SetMonoSubmitEnabled / UseThreadedSubmit (src/Runtimes/OpenXRManager.cpp).
        safe fn cvr_cpp_xr_set_mono_submit_enabled(enabled: bool);
        safe fn cvr_cpp_xr_use_threaded_submit() -> bool;
        #[link_name = "?g_liveControlPath@@3PADA"]
        static g_liveControlPath: [c_char; 260];
        #[link_name = "?g_lastLiveControlWrite@@3U_FILETIME@@A"]
        static mut g_lastLiveControlWrite: [u32; 2];
        #[link_name = "?g_vrikRecenterPath@@3PADA"]
        static g_vrikRecenterPath: [c_char; 260];
        #[link_name = "?g_lastVrikRecenterWrite@@3U_FILETIME@@A"]
        static mut g_lastVrikRecenterWrite: [u32; 2];
        #[link_name = "?g_lastVrikRecenterCounter@@3HA"]
        static mut g_lastVrikRecenterCounter: i32;
        #[link_name = "?g_vrikSettingsPath@@3PADA"]
        static g_vrikSettingsPath: [c_char; 260];
        #[link_name = "?g_drawBarrelCross@overlay@@3_NA"]
        static mut g_drawBarrelCross: bool;
        #[link_name = "?g_viveTrackersSetting@@3HA"]
        static mut g_viveTrackersSetting: i32;
        #[link_name = "?g_submitChainRev@@3HA"]
        static mut g_submitChainRev: i32;
        #[link_name = "?g_verboseLog@@3HC"]
        static g_verboseLog: i32;
    }
    extern "C" fn set_immersive_holsters(v: i32) {
        cvr_xr::switches::SWITCHES.immersive_holsters.store(v, Relaxed);
    }
    extern "C" fn set_body_hands_only(v: i32) {
        cvr_xr::switches::SWITCHES.body_hands_only.store(v, Relaxed);
    }
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()).
    extern "C" fn persist_live_controls() -> bool {
        // SAFETY: a zeroed snapshot (as the C++ half default-constructed it), filled whole by the make.
        let mut state: cvr_settings::live_controls_ui_state::LiveControlsUiState = unsafe { core::mem::zeroed() };
        // SAFETY: a local snapshot.
        unsafe {
            crate::live_controls_ui::cvr_make_live_controls_ui_state(&mut state);
            crate::live_controls_ui::cvr_persist_live_controls_ui_state(&state)
        }
    }
    pub(super) static RT: Runtime = Runtime {
        init_runtime_paths: crate::runtime_paths::cvr_init_runtime_paths,
        request_recenter: cvr_xr::tracking::cvr_xr_request_recenter,
        set_immersive_holsters,
        set_body_hands_only,
        set_mono_submit_enabled: cvr_cpp_xr_set_mono_submit_enabled,
        use_threaded_submit: cvr_cpp_xr_use_threaded_submit,
        persist_live_controls,
        // sizeof / alignof(LiveControlsSubsystems): the Rust mirror's
        subsystems_size: core::mem::size_of::<super::SubsystemsValue>(),
        subsystems_align: core::mem::align_of::<super::SubsystemsValue>(),
        subsystems_seed: super::subsystems_seed,
        subsystems_apply: super::subsystems_apply,
        subsystems_drop: super::subsystems_drop,
        live_control_path: (&raw const g_liveControlPath).cast(),
        last_live_control_write: &raw mut g_lastLiveControlWrite,
        vrik_recenter_path: (&raw const g_vrikRecenterPath).cast(),
        last_vrik_recenter_write: &raw mut g_lastVrikRecenterWrite,
        last_vrik_recenter_counter: &raw mut g_lastVrikRecenterCounter,
        vrik_settings_path: (&raw const g_vrikSettingsPath).cast(),
        draw_barrel_cross: &raw mut g_drawBarrelCross,
        vive_trackers_setting: &raw mut g_viveTrackersSetting,
        submit_chain_rev: &raw mut g_submitChainRev,
        verbose_log: &raw const g_verboseLog,
    };
}
// SAFETY: process-lifetime globals and plain functions.
unsafe impl Sync for Runtime {}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const table::RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_live_controls_poll_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn log_c(text: std::ffi::CString) {
    log(&text.to_string_lossy());
}
fn verbose() -> bool {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(rt().verbose_log) != 0 }
}

// ---- the live keys (their definitions are the C++ half's, or Rust's under the same C names) -----------------------

extern "C" {
    /// OverlayArmLoadGuard's Rust half (imgui_overlay.rs).
    fn cvr_overlay_arm_load_guard(reason: *const c_char);
    static mut CyberpunkVR_ThreadedMonoSubmit: i32;
    static CyberpunkVR_MonoCascades: AtomicU32;
    static mut CyberpunkVR_MonoMode: i32;
    static mut CyberpunkVR_Afw: i32;
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
    static CyberpunkVR_SubmitChainDeferPresent: AtomicU32;
    static CyberpunkVR_GameThreadSkipRenderQueue: AtomicU32;
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
    static mut CyberpunkVR_VrcamPosFromMain: i32;
    static mut CyberpunkVR_DevCamInLocate: i32;
    static mut CyberpunkVR_LensHeadWrite: i32;
    static mut CyberpunkVR_InputDefaultInUi: i32;
    static mut CyberpunkVR_PopupMagBlockMs: i32;
    static mut CyberpunkVR_DevCamAnyName: i32;
    static mut CyberpunkVR_DevCamTolM: f32;
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
    static mut CyberpunkVR_InteractionAnchor: i32;
    static mut CyberpunkVR_HudClearance: i32;
    static mut CyberpunkVR_HudClearanceGrow: f32;
    static mut CyberpunkVR_HudClearanceHands: i32;
    static mut CyberpunkVR_HudClearanceHandsScale: f32;
    static mut CyberpunkVR_AutoRecenter: i32;
    static mut CyberpunkVR_FingerBones: i32;
    static mut CyberpunkVR_FingerSpread: f32;
    static mut CyberpunkVR_FingerSpreadOn: i32;
    static mut CyberpunkVR_AsymProjection: i32;
    static mut CyberpunkVR_AsymInnerGuard: f32;
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
    static mut CyberpunkVR_RecoilDownPistol: f32;
    static mut CyberpunkVR_RecoilDownRifle: f32;
    static mut CyberpunkVR_RecoilDownShotgun: f32;
    static mut CyberpunkVR_HandRecoilBackCm: f32;
    static mut CyberpunkVR_HandRecoilBackMaxCm: f32;
    static mut CyberpunkVR_TwoHandBackMul: f32;
    static mut CyberpunkVR_SniperRiseMul: f32;
    static mut CyberpunkVR_SniperBackMul: f32;
    static mut CyberpunkVR_RecoilScale: f32;
    static mut CyberpunkVR_Haptics: i32;
    static mut CyberpunkVR_HapticScale: f32;
    static mut CyberpunkVR_PistolBackMul: f32;
    static mut CyberpunkVR_RecoilClimbFrac: f32;
    static mut CyberpunkVR_RecoilClimbMaxDeg: f32;
    static mut CyberpunkVR_RecoilClimbMs: f32;
    static mut CyberpunkVR_HandRecoilReturnMs: f32;
    static mut CyberpunkVR_HandRecoilReturnPow: f32;
    static mut CyberpunkVR_ScannerSlots: [f32; 21];
    static mut CyberpunkVR_HandRelToFilteredHead: i32;
    static mut CyberpunkVR_HandLocatePerFrame: i32;
    static mut CyberpunkVR_VisionCsNode: u32;
    static mut CyberpunkVR_VisionCsOrd: i32;
    static mut CyberpunkVR_VisionCsDim: i32;
    static mut CyberpunkVR_VisionCsSwap: i32;
    static mut CyberpunkVR_WeaponPlaneAsScene: i32;
    static mut CyberpunkVR_HighlightAnyPlane: i32;
    static mut CyberpunkVR_PlaneBucketScene: i32;

    fn GetHmdTrackingSmooth() -> f32;
    fn SetHmdTrackingSmooth(value: f32);
    fn CyberpunkVR_StageReplayConfigure(enable: i32);
    fn CyberpunkVR_VrcamGbufferPartitionsConfigure(enable: i32);
    fn CyberpunkVR_ScopeGbufferPartitionsConfigure(enable: i32);
    fn CyberpunkVR_SubmitChainConfigure(mode: i32);
    fn cvr_weapon_plane_sync();
    fn cvr_highlight_any_plane_sync();
    fn cvr_plane_bucket_scene_sync();
    fn cvr_optics_source_get_stabilize() -> i32;
    fn cvr_optics_source_set_stabilize(percent: i32);
    fn cvr_reflex_set_mode(mode: i32);
    fn cvr_framegen_set_fps_cap(on: bool);
    fn cvr_engine_stereo_apply_mono_mode(mono: bool);
    fn cvr_fingergrip_grip_from_fingers() -> bool;
    fn cvr_fingergrip_set_grip_from_fingers(on: bool);
    fn cvr_stick_deadzone(value: f32) -> f32;
    fn cvr_stick_full_input(value: f32) -> f32;
    fn cvr_rotation_mode_from_flags(physical: i32, tracked: i32, hybrid: i32, head_led: i32) -> i32;
    fn cvr_crouch_threshold(value: f32) -> f32;
    fn cvr_resolve_vehicle_vrik_suspend_tier(on_foot: i32, vehicle: i32, explicit: bool) -> i32;
}

/// A live key, read as the C++ half reads it.
macro_rules! g {
    ($name:ident) => {{
        // SAFETY: a 4-byte scalar of the C++ half (or of Rust under its C name), alive for the process.
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}
/// A live key, written.
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

/// cvr::afw::ModeOf.
fn afw_mode_of(afw: bool, osr: bool) -> i32 {
    if osr {
        2
    } else if afw {
        1
    } else {
        0
    }
}
fn b(v: bool) -> i32 {
    v as i32
}

// ---- the CET VRIK mod's recenter request ----------------------------------------------------------------------------

pub(crate) fn file_time(path: *const c_char) -> Option<[u32; 2]> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesExA, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA,
    };
    let mut data: WIN32_FILE_ATTRIBUTE_DATA = unsafe { core::mem::zeroed() };
    // SAFETY: a C string path and the data written.
    let ok = unsafe {
        GetFileAttributesExA(
            path as *const u8,
            GetFileExInfoStandard,
            &mut data as *mut _ as *mut c_void,
        )
    };
    (ok != 0).then_some([
        data.ftLastWriteTime.dwLowDateTime,
        data.ftLastWriteTime.dwHighDateTime,
    ])
}

/// PollVrikRecenterRequest: the counter CET writes on a save load (OnGameAttached); a change recenters.
fn poll_vrik_recenter_request() {
    let r = rt();
    (r.init_runtime_paths)();
    let Some(time) = file_time(r.vrik_recenter_path) else {
        return;
    };
    // SAFETY: the C++ half's FILETIME.
    unsafe {
        if *r.last_vrik_recenter_write == time {
            return;
        }
        *r.last_vrik_recenter_write = time;
    }
    // SAFETY: the C++ half's path.
    let Some(mut file) = (unsafe { cvr_crt::File::open_shared(r.vrik_recenter_path) }) else {
        return;
    };
    let mut line = [0 as c_char; 64];
    let mut counter = -1i32;
    while file.gets_into(&mut line) {
        let mut v = 0i32;
        // SAFETY: a NUL-terminated line and the int written.
        if unsafe { cvr_crt::scan(line.as_ptr(), c"recenter=%d", &[cvr_crt::ptr(&mut v)]) } == 1 {
            counter = v;
            break;
        }
    }
    drop(file);
    if counter == 0 {
        return;
    }
    // The baseline was captured at startup (InitRuntimePaths); any later change is a fresh OnGameAttached.
    // SAFETY: the C++ half's int.
    unsafe {
        if counter != *r.last_vrik_recenter_counter {
            *r.last_vrik_recenter_counter = counter;
            (r.request_recenter)();
            // THE SAME SIGNAL MARKS THE LOAD for the overlay's pacing guard.
            cvr_overlay_arm_load_guard(c"save load".as_ptr());
            log(&format!(
                "VRIK recenter request (save load) -> recentering. counter={counter}\n"
            ));
        }
    }
}

// ---- the poll -------------------------------------------------------------------------------------------------------

/// The loop's initial values: the live state an ini without the key leaves alone.
fn seed(v: &mut LiveControlsValues) {
    v.xrWeaponLaserDot = false;
    v.xrHeadOffsetX = 0.0;
    v.xrHeadOffsetY = 0.0;
    v.xrHeadOffsetZ = 0.0;
    v.xrRecenter = 0;
    v.xrMonoSubmit = 1;
    v.xrThreadedSubmit = g!(CyberpunkVR_ThreadedMonoSubmit);
    v.xrWindowWidth = 0;
    v.xrWindowHeight = 0;
    v.xrForceFov = 0.0;
    v.xrMenuRect = 0;
    v.xrMenuFov = 65.4;
    v.xrMenuFollowDeg = 60.0;
    v.xrHudPanel = 1;
    v.xrHudFollowDeg = 60.0;
    v.xrHudFov = 65.4;
    v.xrHudDistance = 1.5;
    v.xrInteractionPanel = 1;
    v.xrInteractionFollowDeg = 0.0;
    v.xrInteractionDistance = 2.0;
    v.xrInteractionFov = 75.0;
    v.xrLootFollowDeg = 0.0;
    v.xrHudFollowMode = 0;
    v.xrHudStereoDepth = 1;
    v.xrHudBrightness = 1.0;
    v.xrHudShadow = 1.0;
    v.xrHudGlow = 1.0;
    // SAFETY: the C++ half's atomic.
    v.monoCascades = unsafe { CyberpunkVR_MonoCascades.load(Relaxed) };
    v.monoMode = g!(CyberpunkVR_MonoMode);
    v.afw = b(g!(CyberpunkVR_Afw) == 1);
    v.osr = b(g!(CyberpunkVR_Afw) == 2);
    // SAFETY: Rust's own functions under their C names.
    v.scopeStabilize = unsafe { cvr_optics_source_get_stabilize() };
    v.nvidiaReflex = 0;
    v.fpsCap = 1;
    v.lobby = 1;
    v.lobbyAmbient = 1;
    v.xrPitchSign = 1.0;
    v.xrPitchScale = 1.35;
    v.xrSyncSequential = 1;
    v.xr3DofMovement = 0;
    v.xrFirstLaunch = 1;
    v.xrMotionPredictMs = 0.0;
    v.xrStereoScale = 1.0;
    v.xrWorldScale = 1.0;
    v.xrIpdScale = 1.0;
    v.xrSharpness = 0.0;
    v.xrSharpmix = 0.0;
    v.xrReuseLastFrame = 0;
    v.xrPairLock = 0;
    v.xrRenderPoseSubmit = 1;
    v.xrPoseLag = 1;
    v.xrRuntime = 0;
    v.xrDepthSubmit = 1;
    v.xrMovementControl = lc!(xrMovementControl);
    v.xrDisableMouseY = lc!(xrDisableMouseY);
    v.xrXInputHook = if lc!(xrXInputHook) != 0 {
        lc!(xrXInputHook)
    } else {
        1
    };
    v.xrSnapTurn = lc!(xrSnapTurn);
    // SAFETY: the C++ half's accessor.
    v.xrHmdSmooth = unsafe { GetHmdTrackingSmooth() };
    v.xrHandLerp = g!(CyberpunkVR_HandLerpSpeed);
    v.xrLerpPistol = g!(CyberpunkVR_HandLerpPistol);
    v.xrLerpRifle = g!(CyberpunkVR_HandLerpRifle);
    v.xrLerpShotgun = g!(CyberpunkVR_HandLerpShotgun);
    v.xrLerpSniper = g!(CyberpunkVR_HandLerpSniper);
    v.xrLerpMelee = g!(CyberpunkVR_HandLerpMelee);
    v.xrLerpFramegen = g!(CyberpunkVR_HandLerpFramegenScale);
    v.xrGradeMirrorMask = g!(CyberpunkVR_GradeMirrorMask);
    v.xrEnvExtraMask = g!(CyberpunkVR_EnvExtraMask);
    v.xrWaterMirror = g!(CyberpunkVR_WaterMirror);
    v.xrWaterAssign = g!(CyberpunkVR_WaterAssign);
    v.xrViewDataDiff = g!(CyberpunkVR_ViewDataDiff);
    v.xrLutSrcLend = g!(CyberpunkVR_LutSrcLend);
    v.xrSceneYawDps = g!(CyberpunkVR_SceneStickYawDps);
    v.xrSceneYawTier = g!(CyberpunkVR_SceneStickYawTier);
    v.xrBlockDiff = g!(CyberpunkVR_BlockDiff);
    v.xrBlockDiffNode = g!(CyberpunkVR_BlockDiffNode);
    v.xrBlockDiffSize = g!(CyberpunkVR_BlockDiffSize);
    v.xrRenderMaskGrant = g!(CyberpunkVR_RenderMaskGrant);
    v.xrStageReplay = g!(CyberpunkVR_StageReplaySetting);
    v.xrGbufferPartitions = g!(CyberpunkVR_VrcamGbufferPartitionsSetting);
    v.xrScopeGbufferPartitions = g!(CyberpunkVR_ScopeGbufferPartitionsSetting);
    v.xrScopeGraphDag = b(g!(CyberpunkVR_ScopeGraphDag) != 0);
    v.xrSubmitChain = g!(CyberpunkVR_SubmitChainSetting);
    v.xrDeferPresent = g!(CyberpunkVR_DeferPresentSetting);
    v.xrGtSkipRenderQueue = g!(CyberpunkVR_GtSkipRenderQueueSetting);
    v.xrSubmitChainRev = 0;
    v.xrViewDataFixMask = g!(CyberpunkVR_ViewDataFixMask);
    v.xrEnvMirrorMask = g!(CyberpunkVR_EnvMirrorMask);
    v.xrBdCamDirty = g!(CyberpunkVR_BdCamDirty);
    v.xrBdPushTransform = g!(CyberpunkVR_BdPushTransform);
    v.xrBdMainPosScene = g!(CyberpunkVR_BdMainPosFromScene);
    v.xrBdPushBase = g!(CyberpunkVR_BdPushBase);
    v.xrBdOneComp = g!(CyberpunkVR_BdOneComposition);
    v.xrBdEditorAlign = g!(CyberpunkVR_BdEditorAlign);
    v.xrBdBaseFromLocate = g!(CyberpunkVR_BdBaseFromLocate);
    v.xrCompLendSet = g!(CyberpunkVR_CompLendSet);
    v.xrFinalGrab = g!(CyberpunkVR_FinalGrab);
    v.xrStableSrgbView = g!(CyberpunkVR_StableSrgbView);
    v.xrBdQuatWriteSite = g!(CyberpunkVR_BdQuatFromWriteSite);
    v.xrVrcamPosFromMain = g!(CyberpunkVR_VrcamPosFromMain);
    v.xrDevCamInLocate = g!(CyberpunkVR_DevCamInLocate);
    v.xrLensHeadWrite = g!(CyberpunkVR_LensHeadWrite);
    v.xrInputDefaultInUi = g!(CyberpunkVR_InputDefaultInUi);
    v.xrPopupMagBlockMs = g!(CyberpunkVR_PopupMagBlockMs);
    v.xrDevCamAnyName = g!(CyberpunkVR_DevCamAnyName);
    v.xrDevCamTolM = g!(CyberpunkVR_DevCamTolM);
    v.xrBdIpdInLocate = g!(CyberpunkVR_BdIpdInLocate);
    v.xrBdQuatFromBuffer = g!(CyberpunkVR_BdQuatFromBuffer);
    v.xrCapGrant = g!(CyberpunkVR_CapGrant);
    v.xrForceVrcamCam = g!(CyberpunkVR_ForceVrcamCam);
    v.xrVrcamCamFields = g!(CyberpunkVR_VrcamCamFields);
    v.xrRunViewParams = g!(CyberpunkVR_RunViewParams);
    v.xrExpoProbe = g!(CyberpunkVR_ExpoProbe);
    v.xrIndirectCensus = g!(CyberpunkVR_IndirectCensus);
    v.xrNoStateLies = g!(CyberpunkVR_NoStateLies);
    v.xrStableCopy = g!(CyberpunkVR_StableCopy);
    v.xrDistantReuse = g!(CyberpunkVR_DistantReuseMode);
    v.xrLocalShadowReuse = g!(CyberpunkVR_LocalShadowReuseMode);
    v.xrGiReuse = g!(CyberpunkVR_GiReuseMode);
    v.xrProbeReuse = g!(CyberpunkVR_ProbeReuseMode);
    v.xrExpoMirror = g!(CyberpunkVR_ExpoMirror);
    v.xrExpoFieldMask = g!(CyberpunkVR_ExpoFieldMask);
    v.xrCompLendScoped = g!(CyberpunkVR_CompLendScoped);
    v.xrLightContent = g!(CyberpunkVR_LightContent);
    v.xrFixLod = g!(CyberpunkVR_FixLodEnable);
    v.xrHudTo2 = g!(CyberpunkVR_HudToSecondEye);
    v.xrInteractionAnchor = g!(CyberpunkVR_InteractionAnchor);
    v.xrHudClearance = g!(CyberpunkVR_HudClearance);
    v.xrHudClearanceGrow = g!(CyberpunkVR_HudClearanceGrow);
    v.xrHudClearanceHands = g!(CyberpunkVR_HudClearanceHands);
    v.xrHudClearanceHandsScale = g!(CyberpunkVR_HudClearanceHandsScale);
    v.xrAutoRecenter = g!(CyberpunkVR_AutoRecenter);
    v.xrFingerBones = g!(CyberpunkVR_FingerBones);
    // SAFETY: Rust's own function under its C name.
    v.xrGripFromFingers = b(unsafe { cvr_fingergrip_grip_from_fingers() });
    v.xrFingerSpread = g!(CyberpunkVR_FingerSpread);
    v.xrFingerSpreadOn = g!(CyberpunkVR_FingerSpreadOn);
    v.xrAsym = g!(CyberpunkVR_AsymProjection);
    v.xrAsymGuard = g!(CyberpunkVR_AsymInnerGuard);
    v.xrVisionTo2 = g!(CyberpunkVR_VisionToSecondEye);
    v.xrHudBd = g!(CyberpunkVR_HudInBraindance);
    v.xrCompGroup = g!(CyberpunkVR_VrcamCompositionGroup);
    v.xrTwoHandRadius = g!(CyberpunkVR_TwoHandRadius);
    v.xrTwoHandAim = g!(CyberpunkVR_TwoHandAim);
    v.xrTwoHandBreak = g!(CyberpunkVR_TwoHandBreakRadius);
    v.xrTwoHandAimRelease = g!(CyberpunkVR_TwoHandAimReleaseMs);
    v.xrCarryRadius = g!(CyberpunkVR_CarryGripRadius);
    v.xrRifleRise = g!(CyberpunkVR_RifleRiseMul);
    v.xrRifleBack = g!(CyberpunkVR_RifleBackMul);
    v.xrShotgunRise = g!(CyberpunkVR_ShotgunRiseMul);
    v.xrShotgunBack = g!(CyberpunkVR_ShotgunBackMul);
    v.xrDownPistol = g!(CyberpunkVR_RecoilDownPistol);
    v.xrDownRifle = g!(CyberpunkVR_RecoilDownRifle);
    v.xrDownShotgun = g!(CyberpunkVR_RecoilDownShotgun);
    v.xrBackCm = g!(CyberpunkVR_HandRecoilBackCm);
    v.xrBackMaxCm = g!(CyberpunkVR_HandRecoilBackMaxCm);
    v.xrTwoHandBack = g!(CyberpunkVR_TwoHandBackMul);
    v.xrSniperRise = g!(CyberpunkVR_SniperRiseMul);
    v.xrSniperBack = g!(CyberpunkVR_SniperBackMul);
    v.xrRecoilScale = g!(CyberpunkVR_RecoilScale);
    v.xrHaptics = g!(CyberpunkVR_Haptics);
    v.xrHapticScale = g!(CyberpunkVR_HapticScale);
    v.xrPistolBack = g!(CyberpunkVR_PistolBackMul);
    v.xrClimbFrac = g!(CyberpunkVR_RecoilClimbFrac);
    v.xrClimbMax = g!(CyberpunkVR_RecoilClimbMaxDeg);
    v.xrClimbMs = g!(CyberpunkVR_RecoilClimbMs);
    v.xrReturnMs = g!(CyberpunkVR_HandRecoilReturnMs);
    v.xrReturnPow = g!(CyberpunkVR_HandRecoilReturnPow);
    v.xrScannerSlots = g!(CyberpunkVR_ScannerSlots);
    v.xrHandRelFiltered = g!(CyberpunkVR_HandRelToFilteredHead);
    v.xrHandPerFrame = g!(CyberpunkVR_HandLocatePerFrame);
    v.xrSnapTurnAngleDeg = if lc!(xrSnapTurnAngleDeg) > 0.0 {
        lc!(xrSnapTurnAngleDeg)
    } else {
        30.0
    };
    v.xrMovementSource = lc!(xrMovementSource);
    v.xrMovementSpeedMode = 0;
    v.xrSprintMode = 0;
    v.xrLeftStickDeadzone = 0.15;
    v.xrRightStickDeadzone = 0.15;
    v.xrMaxInputThreshold = 0.90;
    v.xrXInputInstall = lc!(xrXInputInstall);
    v.xrInputActions = lc!(xrInputActions);
    v.xrMonoXQueueWait = lc!(xrMonoXQueueWait);
    v.xrSnapTurnPulseMs = if lc!(xrSnapTurnPulseMs) > 0 {
        lc!(xrSnapTurnPulseMs)
    } else {
        30
    };
    v.xrMonoDepthCapture = lc!(xrMonoDepthCapture);
    v.xrSnapTurnYawIndex = if lc!(xrSnapTurnYawIndex) >= 0 && lc!(xrSnapTurnYawIndex) <= 3 {
        lc!(xrSnapTurnYawIndex)
    } else {
        1
    };
    v.xrImmersiveHolsters = lc!(xrImmersiveHolsters);
    v.xrBodyHandsOnly = lc!(xrBodyHandsOnly);
    v.xrPhysicalBodyRotation = lc!(xrPhysicalBodyRotation);
    v.xrTrackedBodyRotation = lc!(xrTrackedBodyRotation);
    v.xrHybridBodyRotation = lc!(xrHybridBodyRotation);
    v.xrRoomscaleMovement = lc!(xrRoomscaleMovement);
    v.xrPhysicalCrouch = lc!(xrPhysicalCrouch);
    v.xrPhysicalCrouchDepth = lc!(xrPhysicalCrouchDepth);
    v.xrBreaststrokeSwim = lc!(xrBreaststrokeSwim);
    v.xrLadderGripClimb = lc!(xrLadderGripClimb);
    v.xrLeftHanded = lc!(xrLeftHanded);
    v.xrLeftHandedBindings = lc!(xrLeftHandedBindings);
    v.xrLadderAutoFinish = lc!(xrLadderAutoFinish);
    v.xrLadderFinishDistance = lc!(xrLadderFinishDistance);
    v.xrBodyFreeLookDeg = lc!(xrBodyFreeLookDeg);
    v.xrBodyFreeLookDownDeg = lc!(xrBodyFreeLookDownDeg);
    v.xrBodyFreeLookSwimDeg = lc!(xrBodyFreeLookSwimDeg);
    v.xrBodyMoveRadius = lc!(xrBodyMoveRadius);
    v.xrSceneCamComfortDeg = lc!(xrSceneCamComfortDeg);
    v.xrCutsceneSuspendTier = lc!(xrCutsceneSuspendTier);
    v.xrVehicleCutsceneSuspendTier = lc!(xrVehicleCutsceneSuspendTier);
    v.vehicleCutsceneTierConfigured = false;
    v.xrVehHeadOffsetX = lc!(xrVehHeadOffsetX);
    v.xrVehHeadOffsetY = lc!(xrVehHeadOffsetY);
    v.xrVehHeadOffsetZ = lc!(xrVehHeadOffsetZ);
    v.xrWheelGrab = lc!(xrWheelGrab);
    v.xrMenuGesture = lc!(xrMenuGesture);
    v.xrVrWeaponWheel = lc!(xrVrWeaponWheel);
    v.xrScopeRrFullResolution = b(crate::scope_resolution::FULL_RESOLUTION.load(Relaxed));
    v.xrPsvr2HapticBackend = 2; // Auto: native unless a compatible bridge is running.
    v.xrOpenXrHapticGain = haptic_route::legacy_gain();
    v.xrOpenXrVehicleHapticGain = haptic_route::vehicle_gain();
    v.xrVehicleAudioChannels = haptic_route::vehicle_audio_channels();
    v.xrVehicleAudioRequireGrip = haptic_route::vehicle_audio_require_grip() as i32;
    v.xrReloadGlow = lc!(xrReloadGlow);
    v.xrHeadLedBodyRotation = lc!(xrHeadLedBodyRotation);
    v.xrBikeHeadOffsetX = lc!(xrBikeHeadOffsetX);
    v.xrBikeHeadOffsetY = lc!(xrBikeHeadOffsetY);
    v.xrBikeHeadOffsetZ = lc!(xrBikeHeadOffsetZ);
    v.xrVrInterface = lc!(xrVrInterface);
    v.xrHandPhysics = lc!(xrHandPhysics);
    v.xrHandCollision = lc!(xrHandCollision);
    v.xrHandCollisionVehicle = lc!(xrHandCollisionVehicle);
    v.xrHandPush = lc!(xrHandPush);
    v.xrFingerContact = lc!(xrFingerContact);
    v.xrPropGrab = lc!(xrPropGrab);
    v.xrCarryPicture = lc!(xrCarryPicture);
    v.xrCarryLead = lc!(xrCarryLead);
    v.xrContactMargin = lc!(xrContactMargin);
    v.xrWheelAnywhere = lc!(xrWheelAnywhere);
    v.xrBikeBarLockDeg = bike_bar_lock_degrees(lc!(xrBikeBarLockDeg));
    v.xrBikeBodyLean = bike_body_lean_share(lc!(xrBikeBodyLean));
    v.savedPlaneAsScene = -1;
    v.xrWheelRadius = if lc!(xrWheelRadius) > 0.0 {
        lc!(xrWheelRadius)
    } else {
        0.28
    };
    v.xrWheelSteerMaxDeg = wheel_full_lock_degrees(lc!(xrWheelSteerMaxDeg));
    v.xrWheelSteerDeadDeg = if lc!(xrWheelSteerDeadDeg) >= 0.0 {
        lc!(xrWheelSteerDeadDeg)
    } else {
        0.0
    };
    v.xrWheelPrediction = lc!(xrWheelPrediction);
    v.xrWheelPredictionMs = lc!(xrWheelPredictionMs);
    v.xrWheelHorn = lc!(xrWheelHorn);
    v.xrWheelHornRadius = if lc!(xrWheelHornRadius) > 0.0 {
        lc!(xrWheelHornRadius)
    } else {
        0.12
    };
    v.xrVehicleGunTrigger = lc!(xrVehicleGunTrigger);
    v.xrVehicleThrottleTrim = if lc!(xrVehicleThrottleTrim) > 0.0 {
        lc!(xrVehicleThrottleTrim)
    } else {
        0.5
    };
    // SAFETY: the C++ half's int.
    v.g_viveTrackersSetting = unsafe { *rt().vive_trackers_setting };
    v.CyberpunkVR_VisionCsNode = g!(CyberpunkVR_VisionCsNode);
    v.CyberpunkVR_VisionCsOrd = g!(CyberpunkVR_VisionCsOrd);
    v.CyberpunkVR_VisionCsDim = g!(CyberpunkVR_VisionCsDim);
    v.CyberpunkVR_VisionCsSwap = g!(CyberpunkVR_VisionCsSwap);
    v.CyberpunkVR_WeaponPlaneAsScene = g!(CyberpunkVR_WeaponPlaneAsScene);
    v.CyberpunkVR_HighlightAnyPlane = g!(CyberpunkVR_HighlightAnyPlane);
    v.CyberpunkVR_PlaneBucketScene = g!(CyberpunkVR_PlaneBucketScene);
}

/// LiveControlsSubsystems (include/Core/LiveControlsParse.hpp) as Rust lays it out: the HUD's element layout, frame
/// generation's, foveated DLSS's, fixed VRS's, the hidden area's and the VR overlay's settings (976 bytes, 4-aligned, as
/// the C++ struct; formerly built and applied by src/Core/LiveControlsPoll.cpp's services).
#[repr(C)]
pub struct SubsystemsValue {
    pub hud_elements: [cvr_settings::hud_layout::ElementSettings; cvr_settings::hud_layout::COUNT],
    pub framegen: cvr_gpu::framegen_core::Settings,
    pub foveated: crate::foveated::Settings,
    pub vrs: crate::vrs::Settings,
    pub hidden_area: crate::hidden_area::Settings,
    pub overlay: cvr_body::vr_overlay::Settings,
}
const _: () = assert!(
    core::mem::size_of::<SubsystemsValue>() == 976
        && core::mem::align_of::<SubsystemsValue>() == 4
        && cvr_settings::hud_layout::COUNT == 41
);

#[allow(non_upper_case_globals)]
unsafe extern "C" {
}

/// cvr::hidden_area::SetSettings (formerly src/Render/HiddenArea.cpp's): normalized, into its exports.
fn hidden_area_set_settings(s: crate::hidden_area::Settings) {
    let s = crate::hidden_area::cvr_hidden_area_normalize(s);
    crate::typed_moved_exports::CyberpunkVR_HiddenAreaRadius.store(s.radius.to_bits(), Relaxed);
    crate::used_moved_exports_rest::CyberpunkVR_HiddenAreaEnabled.store(s.enabled as u32, Relaxed);
}
/// `new (p) LiveControlsSubsystems{}` -- every member at its default -- with frame generation's and the VR overlay's
/// current settings.
extern "C" fn subsystems_seed(p: *mut c_void) {
    let mut framegen = cvr_gpu::framegen_core::Settings::default();
    cvr_gpu::framegen_core::cvr_framegen_get_settings(&mut framegen);
    let mut overlay = cvr_body::vr_overlay::Settings::default();
    // SAFETY: a local output.
    unsafe { cvr_body::vr_overlay::cvr_vrui_get_settings(&mut overlay) };
    let value = SubsystemsValue {
        hud_elements: [cvr_settings::hud_layout::ElementSettings::DEFAULT; cvr_settings::hud_layout::COUNT],
        framegen,
        foveated: crate::foveated::Settings::default(),
        vrs: crate::vrs::Settings::default(),
        hidden_area: crate::hidden_area::Settings { enabled: 0, radius: crate::hidden_area::DEFAULT_RADIUS },
        overlay,
    };
    // SAFETY: the caller's buffer of the struct's size and alignment.
    unsafe { p.cast::<SubsystemsValue>().write(value) };
}

/// One subsystem's settings applied: 0 the HUD's layout, 1 frame generation, 2 foveated DLSS, 3 fixed VRS, 4 the hidden
/// area, 5 the VR overlay.
extern "C" fn subsystems_apply(which: i32, p: *const c_void) {
    // SAFETY: a seeded value.
    let s = unsafe { &*p.cast::<SubsystemsValue>() };
    // SAFETY: the subsystems' own setters over this value's members.
    unsafe {
        match which {
            0 => cvr_settings::hud_layout::cvr_hud_layout_set_settings(s.hud_elements.as_ptr()),
            1 => cvr_gpu::framegen_core::cvr_framegen_set_settings(&s.framegen),
            2 => crate::foveated::cvr_foveated_set_settings(s.foveated),
            3 => crate::fixed_vrs::cvr_vrs_set_settings(s.vrs),
            4 => hidden_area_set_settings(s.hidden_area),
            5 => cvr_body::vr_overlay::cvr_vrui_set_settings(&s.overlay),
            _ => {}
        }
    }
}

/// The value's destructor: its members are plain data.
extern "C" fn subsystems_drop(_: *mut c_void) {}

/// One settings line parsed into one subsystem's settings (indices as subsystems_apply's); false when it is not that
/// subsystem's (formerly src/Core/LiveControlsParse.cpp's; tools/vrik_tests keep that one).
///
/// # Safety
/// `line` is NUL-terminated or null; `s` a seeded value or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_settings_parse_subsystem(which: i32, line: *const c_char, s: *mut c_void) -> bool {
    if line.is_null() || s.is_null() {
        return false;
    }
    // SAFETY: as documented.
    unsafe {
        let s = &mut *s.cast::<SubsystemsValue>();
        match which {
            0 => cvr_settings::hud_layout::cvr_hud_layout_parse(line, s.hud_elements.as_mut_ptr()),
            1 => cvr_gpu::framegen_core::cvr_framegen_parse_setting(line, &mut s.framegen),
            2 => crate::foveated::cvr_foveated_parse_setting(line, &mut s.foveated),
            3 => crate::vrs::cvr_vrs_parse_setting(line, &mut s.vrs),
            4 => crate::hidden_area::cvr_hidden_area_parse_setting(line, &mut s.hidden_area),
            5 => cvr_body::vr_overlay::cvr_vrui_parse_setting(line, &mut s.overlay),
            _ => false,
        }
    }
}

/// The C++ half's LiveControlsSubsystems, constructed and seeded in a buffer of its size, destroyed on drop.
struct Subsystems {
    ptr: *mut u8,
    layout: std::alloc::Layout,
}
impl Subsystems {
    fn new() -> Option<Subsystems> {
        let r = rt();
        let layout = std::alloc::Layout::from_size_align(
            r.subsystems_size.max(1),
            r.subsystems_align.max(1),
        )
        .ok()?;
        // SAFETY: a non-zero layout.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            return None;
        }
        (r.subsystems_seed)(ptr as *mut c_void);
        Some(Subsystems { ptr, layout })
    }
    fn apply(&self, which: i32) {
        (rt().subsystems_apply)(which, self.ptr as *const c_void);
    }
}
impl Drop for Subsystems {
    fn drop(&mut self) {
        (rt().subsystems_drop)(self.ptr as *mut c_void);
        // SAFETY: allocated in new with this layout.
        unsafe { std::alloc::dealloc(self.ptr, self.layout) };
    }
}
const HUD: i32 = 0;
const FRAMEGEN: i32 = 1;
const FOVEATED: i32 = 2;
const VRS: i32 = 3;
const HIDDEN_AREA: i32 = 4;
const OVERLAY: i32 = 5;

/// The revision of the submit chain's defaults an ini has (see the C++ half's note at kSubmitChainRev).
const SUBMIT_CHAIN_REV: i32 = 1;
static SUBMIT_CHAIN_REV_WRITE: AtomicBool = AtomicBool::new(false);
static APPLIED_STAGE_REPLAY: AtomicI32 = AtomicI32::new(-1);
static APPLIED_GBUFFER_PARTITIONS: AtomicI32 = AtomicI32::new(-1);
static APPLIED_SUBMIT_CHAIN: AtomicI32 = AtomicI32::new(-1);
static APPLIED_DEFER_PRESENT: AtomicI32 = AtomicI32::new(-1);
static APPLIED_GT_SKIP_RENDER_QUEUE: AtomicI32 = AtomicI32::new(-1);

/// PollLiveControls.
#[no_mangle]
pub extern "C" fn cvr_poll_live_controls() {
    let r = rt();
    (r.init_runtime_paths)();
    poll_vrik_recenter_request();

    let Some(time) = file_time(r.live_control_path) else {
        return;
    };
    // SAFETY: the C++ half's FILETIME.
    unsafe {
        if *r.last_live_control_write == time {
            return;
        }
        *r.last_live_control_write = time;
    }

    // The loop's variables, seeded from the live state, then the ini's lines.
    // SAFETY: plain numbers and flags; zero is what `LiveControlsValues values{}` leaves.
    let mut v: LiveControlsValues = unsafe { core::mem::zeroed() };
    let Some(subsystems) = Subsystems::new() else {
        return;
    };
    seed(&mut v);
    // SAFETY: the C++ half's path, the values and the subsystems.
    if !unsafe {
        cvr_settings::cvr_settings_parse_live_controls(
            r.live_control_path,
            &mut v,
            subsystems.ptr as *mut c_void,
        )
    } {
        return;
    }
    apply(&mut v, &subsystems);
}

fn apply(v: &mut LiveControlsValues, sub: &Subsystems) {
    let r = rt();
    // The globals the loop writes directly.
    // SAFETY: the C++ half's int.
    unsafe { *r.vive_trackers_setting = v.g_viveTrackersSetting };
    s!(CyberpunkVR_VisionCsNode, v.CyberpunkVR_VisionCsNode);
    s!(CyberpunkVR_VisionCsOrd, v.CyberpunkVR_VisionCsOrd);
    s!(CyberpunkVR_VisionCsDim, v.CyberpunkVR_VisionCsDim);
    s!(CyberpunkVR_VisionCsSwap, v.CyberpunkVR_VisionCsSwap);
    s!(
        CyberpunkVR_HighlightAnyPlane,
        v.CyberpunkVR_HighlightAnyPlane
    );
    s!(CyberpunkVR_PlaneBucketScene, v.CyberpunkVR_PlaneBucketScene);

    let prev_recenter = lc!(xrRecenter);
    let prev_mono_submit = lc!(xrMonoSubmit);
    let changed = lc!(xrHeadOffsetX) != v.xrHeadOffsetX
        || lc!(xrHeadOffsetY) != v.xrHeadOffsetY
        || lc!(xrHeadOffsetZ) != v.xrHeadOffsetZ
        || lc!(xrRecenter) != v.xrRecenter
        || lc!(xrMonoSubmit) != v.xrMonoSubmit
        || lc!(xrForceFov) != v.xrForceFov
        || lc!(xrMenuRect) != v.xrMenuRect
        || lc!(xrMenuFov) != v.xrMenuFov
        || lc!(xrMenuFollowDeg) != v.xrMenuFollowDeg
        || lc!(xrHudPanel) != v.xrHudPanel
        || lc!(xrHudFollowDeg) != v.xrHudFollowDeg
        || lc!(xrHudFov) != v.xrHudFov
        || lc!(xrHudDistance) != v.xrHudDistance
        || lc!(xr3DofMovement) != v.xr3DofMovement
        || lc!(xrFirstLaunch) != v.xrFirstLaunch
        || lc!(xrMotionPredictMs) != v.xrMotionPredictMs
        || lc!(xrStereoScale) != v.xrStereoScale
        || lc!(xrWorldScale) != v.xrWorldScale
        || lc!(xrIpdScale) != v.xrIpdScale
        || lc!(xrSharpness) != v.xrSharpness
        || lc!(xrSharpmix) != v.xrSharpmix
        || lc!(xrReuseLastFrame) != v.xrReuseLastFrame
        || lc!(xrPairLock) != v.xrPairLock
        || lc!(xrRenderPoseSubmit) != v.xrRenderPoseSubmit
        || lc!(xrRuntime) != v.xrRuntime
        || lc!(xrDepthSubmit) != v.xrDepthSubmit;

    // SAFETY: the C++ half's bool.
    unsafe { core::ptr::write_volatile(r.draw_barrel_cross, v.xrWeaponLaserDot) };
    lc_set!(xrHeadOffsetX, v.xrHeadOffsetX);
    lc_set!(xrHeadOffsetY, v.xrHeadOffsetY);
    lc_set!(xrHeadOffsetZ, v.xrHeadOffsetZ);
    lc_set!(xrRecenter, v.xrRecenter);
    lc_set!(xrMonoSubmit, v.xrMonoSubmit);
    lc_set!(xrForceFov, v.xrForceFov);
    lc_set!(xrMenuRect, v.xrMenuRect);
    lc_set!(xrMenuFov, v.xrMenuFov);
    lc_set!(xrMenuFollowDeg, v.xrMenuFollowDeg);
    sub.apply(FRAMEGEN);
    sub.apply(FOVEATED);
    sub.apply(VRS);
    sub.apply(HIDDEN_AREA);
    // SAFETY: the C++ half's atomic; Rust's own functions under their C names.
    unsafe {
        CyberpunkVR_MonoCascades.store(v.monoCascades, Relaxed);
        cvr_engine_stereo_apply_mono_mode(v.monoMode != 0);
    }
    // On or off takes effect at the next launch, AFW <-> OSR at once.
    s!(CyberpunkVR_Afw, afw_mode_of(v.afw != 0, v.osr != 0));
    if crate::afw::cvr_afw_launch() {
        crate::afw::cvr_afw_set_mode(g!(CyberpunkVR_Afw));
    }
    // SAFETY: Rust's own functions under their C names.
    unsafe {
        cvr_optics_source_set_stabilize(v.scopeStabilize);
        cvr_reflex_set_mode(v.nvidiaReflex);
        cvr_framegen_set_fps_cap(v.fpsCap != 0);
    }
    crate::menu_lobby::cvr_menu_lobby_set_enabled(v.lobby != 0);
    crate::menu_lobby::cvr_menu_lobby_set_ambient(v.lobbyAmbient != 0);
    sub.apply(OVERLAY);
    lc_set!(xrHudPanel, b(v.xrHudPanel != 0));
    lc_set!(xrHudFollowMode, b(v.xrHudFollowMode == 1));
    lc_set!(xrHudStereoDepth, b(v.xrHudStereoDepth != 0));
    lc_set!(
        xrHudBrightness,
        if v.xrHudBrightness >= 0.25 && v.xrHudBrightness <= 3.0 {
            v.xrHudBrightness
        } else {
            1.0
        }
    );
    lc_set!(
        xrHudShadow,
        if v.xrHudShadow >= 0.0 && v.xrHudShadow <= 2.0 {
            v.xrHudShadow
        } else {
            1.0
        }
    );
    lc_set!(
        xrHudGlow,
        if v.xrHudGlow >= 0.0 && v.xrHudGlow <= 2.0 {
            v.xrHudGlow
        } else {
            1.0
        }
    );
    sub.apply(HUD);
    lc_set!(
        xrHudFollowDeg,
        if v.xrHudFollowDeg >= 5.0 && v.xrHudFollowDeg <= 90.0 {
            v.xrHudFollowDeg
        } else {
            60.0
        }
    );
    lc_set!(xrInteractionPanel, b(v.xrInteractionPanel != 0));
    lc_set!(
        xrInteractionFollowDeg,
        if v.xrInteractionFollowDeg >= 0.0 && v.xrInteractionFollowDeg <= 90.0 {
            v.xrInteractionFollowDeg
        } else {
            0.0
        }
    );
    lc_set!(
        xrLootFollowDeg,
        if v.xrLootFollowDeg >= 0.0 && v.xrLootFollowDeg <= 90.0 {
            v.xrLootFollowDeg
        } else {
            0.0
        }
    );
    lc_set!(
        xrInteractionDistance,
        if v.xrInteractionDistance >= 0.5 && v.xrInteractionDistance <= 5.0 {
            v.xrInteractionDistance
        } else {
            2.0
        }
    );
    lc_set!(
        xrInteractionFov,
        if v.xrInteractionFov >= 30.0 && v.xrInteractionFov <= 120.0 {
            v.xrInteractionFov
        } else {
            75.0
        }
    );
    lc_set!(
        xrHudFov,
        if v.xrHudFov >= 30.0 && v.xrHudFov <= 120.0 {
            v.xrHudFov
        } else {
            65.4
        }
    );
    lc_set!(
        xrHudDistance,
        if v.xrHudDistance >= 0.5 && v.xrHudDistance <= 5.0 {
            v.xrHudDistance
        } else {
            1.5
        }
    );
    lc_set!(xr3DofMovement, v.xr3DofMovement);
    lc_set!(xrFirstLaunch, b(v.xrFirstLaunch != 0));
    lc_set!(
        xrMotionPredictMs,
        if v.xrMotionPredictMs >= 0.0 {
            v.xrMotionPredictMs
        } else {
            0.0
        }
    );
    lc_set!(
        xrStereoScale,
        if v.xrStereoScale < 0.0 {
            0.0
        } else if v.xrStereoScale > 10.0 {
            10.0
        } else {
            v.xrStereoScale
        }
    );
    lc_set!(
        xrWorldScale,
        if v.xrWorldScale < 0.05 {
            0.05
        } else if v.xrWorldScale > 20.0 {
            20.0
        } else {
            v.xrWorldScale
        }
    );
    lc_set!(
        xrIpdScale,
        if v.xrIpdScale < 0.0 {
            0.0
        } else if v.xrIpdScale > 5.0 {
            5.0
        } else {
            v.xrIpdScale
        }
    );
    lc_set!(
        xrSharpness,
        if v.xrSharpness < 0.0 {
            0.0
        } else if v.xrSharpness > 1.0 {
            1.0
        } else {
            v.xrSharpness
        }
    );
    lc_set!(
        xrSharpmix,
        if v.xrSharpmix < 0.0 {
            0.0
        } else if v.xrSharpmix > 1.0 {
            1.0
        } else {
            v.xrSharpmix
        }
    );
    lc_set!(xrReuseLastFrame, b(v.xrReuseLastFrame != 0));
    lc_set!(xrPairLock, b(v.xrPairLock != 0));
    lc_set!(xrRenderPoseSubmit, b(v.xrRenderPoseSubmit != 0));
    lc_set!(xrPoseLag, v.xrPoseLag);
    lc_set!(xrRuntime, b(v.xrRuntime == 1));
    lc_set!(xrDepthSubmit, b(v.xrDepthSubmit != 0));
    // xrMovementSource is the authoritative locomotion mode (0..3); the legacy xrMovementControl mirrors it for old
    // configs (0 = Game, anything else means VR-driven, so legacy 1).
    if v.xrMovementSource < 0 || v.xrMovementSource > 3 {
        v.xrMovementSource = b(v.xrMovementControl != 0);
    }
    lc_set!(xrMovementSource, v.xrMovementSource);
    lc_set!(xrMovementSpeedMode, b(v.xrMovementSpeedMode == 1));
    lc_set!(xrSprintMode, b(v.xrSprintMode == 1));
    // SAFETY: Rust's own functions under their C names.
    let (left, right, full) = unsafe {
        (
            cvr_stick_deadzone(v.xrLeftStickDeadzone),
            cvr_stick_deadzone(v.xrRightStickDeadzone),
            cvr_stick_full_input(v.xrMaxInputThreshold),
        )
    };
    lc_set!(xrLeftStickDeadzone, left);
    lc_set!(xrRightStickDeadzone, right);
    lc_set!(xrMaxInputThreshold, full);
    lc_set!(xrMovementControl, b(v.xrMovementSource != 0));
    lc_set!(xrPhysicalBodyRotation, b(v.xrPhysicalBodyRotation != 0));
    lc_set!(xrTrackedBodyRotation, b(v.xrTrackedBodyRotation != 0));
    lc_set!(xrHybridBodyRotation, b(v.xrHybridBodyRotation != 0));
    lc_set!(xrHeadLedBodyRotation, b(v.xrHeadLedBodyRotation != 0));
    // SAFETY: as above.
    let value = unsafe {
        cvr_rotation_mode_from_flags(
            v.xrPhysicalBodyRotation,
            v.xrTrackedBodyRotation,
            v.xrHybridBodyRotation,
            v.xrHeadLedBodyRotation,
        )
    };
    lc_set!(xrBodyRotationMode, value);
    lc_set!(xrRoomscaleMovement, b(v.xrRoomscaleMovement != 0));
    lc_set!(xrPhysicalCrouch, b(v.xrPhysicalCrouch != 0));
    // SAFETY: as above.
    let value = unsafe { cvr_crouch_threshold(v.xrPhysicalCrouchDepth) };
    lc_set!(xrPhysicalCrouchDepth, value);
    lc_set!(xrBreaststrokeSwim, b(v.xrBreaststrokeSwim != 0));
    lc_set!(xrLadderGripClimb, b(v.xrLadderGripClimb != 0));
    lc_set!(xrLeftHanded, b(v.xrLeftHanded != 0));
    lc_set!(xrLeftHandedBindings, v.xrLeftHandedBindings.clamp(0, 2));
    lc_set!(xrLadderAutoFinish, b(v.xrLadderAutoFinish != 0));
    lc_set!(
        xrLadderFinishDistance,
        cpp_clamp(
            if v.xrLadderFinishDistance.is_finite() {
                v.xrLadderFinishDistance
            } else {
                1.0
            },
            0.2,
            1.2
        )
    );
    lc_set!(xrBodyFreeLookDeg, cpp_clamp(v.xrBodyFreeLookDeg, 0.0, 60.0));
    lc_set!(
        xrBodyFreeLookDownDeg,
        cpp_clamp(
            if v.xrBodyFreeLookDownDeg.is_finite() {
                v.xrBodyFreeLookDownDeg
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
            if v.xrBodyFreeLookSwimDeg.is_finite() {
                v.xrBodyFreeLookSwimDeg
            } else {
                5.0
            },
            0.0,
            90.0
        )
    );
    lc_set!(xrBodyMoveRadius, cpp_clamp(v.xrBodyMoveRadius, 0.0, 0.30));
    lc_set!(
        xrSceneCamComfortDeg,
        cpp_clamp(
            if v.xrSceneCamComfortDeg.is_finite() {
                v.xrSceneCamComfortDeg
            } else {
                30.0
            },
            0.0,
            45.0
        )
    );
    lc_set!(
        xrCutsceneSuspendTier,
        if v.xrCutsceneSuspendTier < -1 {
            -1
        } else if v.xrCutsceneSuspendTier > 4 {
            4
        } else {
            v.xrCutsceneSuspendTier
        }
    );
    // SAFETY: as above.
    let value = unsafe {
        cvr_resolve_vehicle_vrik_suspend_tier(
            v.xrCutsceneSuspendTier,
            v.xrVehicleCutsceneSuspendTier,
            v.vehicleCutsceneTierConfigured,
        )
    };
    lc_set!(xrVehicleCutsceneSuspendTier, value);
    lc_set!(xrDisableMouseY, b(v.xrDisableMouseY != 0));
    lc_set!(xrXInputHook, b(v.xrXInputHook != 0));
    lc_set!(xrSnapTurn, b(v.xrSnapTurn != 0));
    lc_set!(
        xrSnapTurnAngleDeg,
        if v.xrSnapTurnAngleDeg > 0.0 {
            v.xrSnapTurnAngleDeg
        } else {
            30.0
        }
    );
    lc_set!(xrXInputInstall, b(v.xrXInputInstall != 0));
    lc_set!(xrInputActions, b(v.xrInputActions != 0));
    lc_set!(xrMonoXQueueWait, b(v.xrMonoXQueueWait != 0));
    lc_set!(
        xrSnapTurnPulseMs,
        if v.xrSnapTurnPulseMs > 0 {
            v.xrSnapTurnPulseMs
        } else {
            30
        }
    );
    lc_set!(xrMonoDepthCapture, b(v.xrMonoDepthCapture != 0));
    lc_set!(
        xrSnapTurnYawIndex,
        if v.xrSnapTurnYawIndex >= 0 && v.xrSnapTurnYawIndex <= 3 {
            v.xrSnapTurnYawIndex
        } else {
            1
        }
    );
    lc_set!(xrImmersiveHolsters, b(v.xrImmersiveHolsters != 0));
    (r.set_immersive_holsters)(lc!(xrImmersiveHolsters));
    lc_set!(xrBodyHandsOnly, b(v.xrBodyHandsOnly != 0));
    (r.set_body_hands_only)(lc!(xrBodyHandsOnly));
    lc_set!(xrReloadGlow, b(v.xrReloadGlow != 0));
    cvr_xr::switches::SWITCHES.reload_glow.store(lc!(xrReloadGlow), Relaxed);
    // Clamped wider than the slider (+/-0.50) but still bounded: a metre of head offset is a typo, not a setting.
    let clamp_veh_off = |v: f32| {
        if v < -1.0 {
            -1.0
        } else if v > 1.0 {
            1.0
        } else {
            v
        }
    };
    lc_set!(xrVehHeadOffsetX, clamp_veh_off(v.xrVehHeadOffsetX));
    lc_set!(xrVehHeadOffsetY, clamp_veh_off(v.xrVehHeadOffsetY));
    lc_set!(xrVehHeadOffsetZ, clamp_veh_off(v.xrVehHeadOffsetZ));
    lc_set!(xrBikeHeadOffsetX, clamp_veh_off(v.xrBikeHeadOffsetX));
    lc_set!(xrBikeHeadOffsetY, clamp_veh_off(v.xrBikeHeadOffsetY));
    lc_set!(xrBikeHeadOffsetZ, clamp_veh_off(v.xrBikeHeadOffsetZ));
    lc_set!(xrWheelGrab, b(v.xrWheelGrab != 0));
    lc_set!(xrMenuGesture, b(v.xrMenuGesture != 0));
    lc_set!(xrVrWeaponWheel, b(v.xrVrWeaponWheel != 0));
    lc_set!(xrVrInterface, b(v.xrVrInterface != 0));
    lc_set!(xrHandPhysics, b(v.xrHandPhysics != 0));
    lc_set!(xrHandCollision, b(v.xrHandCollision != 0));
    lc_set!(xrHandCollisionVehicle, b(v.xrHandCollisionVehicle != 0));
    lc_set!(xrHandPush, b(v.xrHandPush != 0));
    lc_set!(xrFingerContact, b(v.xrFingerContact != 0));
    lc_set!(xrPropGrab, b(v.xrPropGrab != 0));
    lc_set!(xrCarryPicture, b(v.xrCarryPicture != 0));
    lc_set!(xrCarryLead, if v.xrCarryLead < 0.0 { 0.0 } else if v.xrCarryLead > 0.1 { 0.1 } else { v.xrCarryLead });
    lc_set!(xrContactMargin, if v.xrContactMargin < 0.0 { 0.0 } else if v.xrContactMargin > 0.05 { 0.05 } else { v.xrContactMargin });
    lc_set!(xrWheelAnywhere, b(v.xrWheelAnywhere != 0));
    // Clamped, not trusted: below ~8 cm the grab is unreachable for a hand you cannot see; above 60 cm every grip in a
    // car is a grab.
    lc_set!(
        xrWheelRadius,
        if v.xrWheelRadius < 0.08 {
            0.08
        } else if v.xrWheelRadius > 0.60 {
            0.60
        } else {
            v.xrWheelRadius
        }
    );
    lc_set!(
        xrWheelSteerMaxDeg,
        wheel_full_lock_degrees(v.xrWheelSteerMaxDeg)
    );
    lc_set!(xrBikeBarLockDeg, bike_bar_lock_degrees(v.xrBikeBarLockDeg));
    lc_set!(xrBikeBodyLean, bike_body_lean_share(v.xrBikeBodyLean));
    // The deadzone stays well under the smallest full-lock angle (30 deg).
    lc_set!(
        xrWheelSteerDeadDeg,
        if v.xrWheelSteerDeadDeg < 0.0 {
            0.0
        } else if v.xrWheelSteerDeadDeg > 20.0 {
            20.0
        } else {
            v.xrWheelSteerDeadDeg
        }
    );
    lc_set!(xrWheelPrediction, b(v.xrWheelPrediction != 0));
    lc_set!(
        xrWheelPredictionMs,
        if v.xrWheelPredictionMs.is_finite() {
            cpp_clamp(v.xrWheelPredictionMs, 0.0, 8.0)
        } else {
            0.0
        }
    );
    lc_set!(xrWheelHorn, b(v.xrWheelHorn != 0));
    // Below 4 cm the hub is unhittable; above 30 cm it swallows the rim and every reach for the wheel honks.
    lc_set!(
        xrWheelHornRadius,
        if v.xrWheelHornRadius < 0.04 {
            0.04
        } else if v.xrWheelHornRadius > 0.30 {
            0.30
        } else {
            v.xrWheelHornRadius
        }
    );
    lc_set!(xrVehicleGunTrigger, b(v.xrVehicleGunTrigger != 0));
    lc_set!(
        xrVehicleThrottleTrim,
        if v.xrVehicleThrottleTrim < 0.05 {
            0.05
        } else if v.xrVehicleThrottleTrim > 3.0 {
            3.0
        } else {
            v.xrVehicleThrottleTrim
        }
    );
    // SAFETY: the C++ half's setter.
    unsafe { SetHmdTrackingSmooth(v.xrHmdSmooth) };
    // 100, not 30: melee wants 40 and the old ceiling silently cut the number that was asked for.
    s!(
        CyberpunkVR_HandLerpSpeed,
        if v.xrHandLerp < 0.0 {
            0.0
        } else if v.xrHandLerp > 100.0 {
            100.0
        } else {
            v.xrHandLerp
        }
    );
    // Same clamp as the base; 0 stays 0, which is how a class asks for the base.
    let clamp_lerp = |v: f32| {
        if v < 0.0 {
            0.0
        } else if v > 100.0 {
            100.0
        } else {
            v
        }
    };
    s!(CyberpunkVR_HandLerpPistol, clamp_lerp(v.xrLerpPistol));
    s!(CyberpunkVR_HandLerpRifle, clamp_lerp(v.xrLerpRifle));
    s!(CyberpunkVR_HandLerpShotgun, clamp_lerp(v.xrLerpShotgun));
    s!(CyberpunkVR_HandLerpSniper, clamp_lerp(v.xrLerpSniper));
    s!(CyberpunkVR_HandLerpMelee, clamp_lerp(v.xrLerpMelee));
    s!(
        CyberpunkVR_HandLerpFramegenScale,
        if v.xrLerpFramegen.is_finite() {
            cpp_clamp(v.xrLerpFramegen, 0.0, 4.0)
        } else {
            1.10
        }
    );
    // 26 BITS, AND THE NUMBER COMES FROM THE TABLE (kGradeMirrorOff); whenever it grows, this grows with it.
    s!(
        CyberpunkVR_GradeMirrorMask,
        v.xrGradeMirrorMask & 0x3FF_FFFF
    );
    s!(CyberpunkVR_EnvExtraMask, v.xrEnvExtraMask & 0x1F_FFFF); // 21 measured object slots
    s!(CyberpunkVR_WaterMirror, v.xrWaterMirror & 0xF); // 4 groups in kWaterMirror
    s!(CyberpunkVR_WaterAssign, (v.xrWaterAssign != 0) as u32);
    s!(CyberpunkVR_ViewDataDiff, v.xrViewDataDiff);
    s!(CyberpunkVR_LutSrcLend, v.xrLutSrcLend);
    s!(CyberpunkVR_SceneStickYawDps, v.xrSceneYawDps);
    s!(CyberpunkVR_SceneStickYawTier, v.xrSceneYawTier);
    s!(CyberpunkVR_BlockDiff, v.xrBlockDiff);
    s!(CyberpunkVR_BlockDiffNode, v.xrBlockDiffNode);
    s!(CyberpunkVR_BlockDiffSize, v.xrBlockDiffSize);
    // 27 categories exist; anything above them is dropped rather than reinterpreted.
    s!(
        CyberpunkVR_RenderMaskGrant,
        v.xrRenderMaskGrant & 0x07FF_FFFF
    );
    s!(CyberpunkVR_StageReplaySetting, b(v.xrStageReplay != 0));
    if APPLIED_STAGE_REPLAY.load(Relaxed) != g!(CyberpunkVR_StageReplaySetting) {
        // SAFETY: the C++ half's configure.
        unsafe { CyberpunkVR_StageReplayConfigure(g!(CyberpunkVR_StageReplaySetting)) };
        APPLIED_STAGE_REPLAY.store(g!(CyberpunkVR_StageReplaySetting), Relaxed);
    }
    s!(
        CyberpunkVR_VrcamGbufferPartitionsSetting,
        b(v.xrGbufferPartitions != 0)
    );
    if APPLIED_GBUFFER_PARTITIONS.load(Relaxed) != g!(CyberpunkVR_VrcamGbufferPartitionsSetting) {
        // SAFETY: as above.
        unsafe {
            CyberpunkVR_VrcamGbufferPartitionsConfigure(g!(
                CyberpunkVR_VrcamGbufferPartitionsSetting
            ))
        };
        APPLIED_GBUFFER_PARTITIONS.store(g!(CyberpunkVR_VrcamGbufferPartitionsSetting), Relaxed);
    }
    s!(
        CyberpunkVR_ScopeGbufferPartitionsSetting,
        b(v.xrScopeGbufferPartitions != 0)
    );
    // SAFETY: as above.
    unsafe {
        CyberpunkVR_ScopeGbufferPartitionsConfigure(g!(CyberpunkVR_ScopeGbufferPartitionsSetting))
    };
    s!(CyberpunkVR_ScopeGraphDag, (v.xrScopeGraphDag != 0) as u32);
    crate::scope_resolution::FULL_RESOLUTION.store(v.xrScopeRrFullResolution != 0, Relaxed);
    if v.xrSubmitChainRev < SUBMIT_CHAIN_REV {
        v.xrSubmitChain = 1;
        v.xrDeferPresent = 1;
        SUBMIT_CHAIN_REV_WRITE.store(true, Relaxed);
        log(&format!(
            "Live controls: xr_submit_chain_rev {} -> {SUBMIT_CHAIN_REV}: xr_submit_chain=1, xr_defer_present=1.\n",
            v.xrSubmitChainRev
        ));
    }
    // SAFETY: the C++ half's int.
    unsafe { *r.submit_chain_rev = SUBMIT_CHAIN_REV };
    s!(CyberpunkVR_DeferPresentSetting, v.xrDeferPresent);
    if APPLIED_DEFER_PRESENT.load(Relaxed) != g!(CyberpunkVR_DeferPresentSetting) {
        // SAFETY: the C++ half's atomic.
        unsafe {
            CyberpunkVR_SubmitChainDeferPresent
                .store(g!(CyberpunkVR_DeferPresentSetting) as u32, Relaxed)
        };
        APPLIED_DEFER_PRESENT.store(g!(CyberpunkVR_DeferPresentSetting), Relaxed);
    }
    s!(CyberpunkVR_GtSkipRenderQueueSetting, v.xrGtSkipRenderQueue);
    if APPLIED_GT_SKIP_RENDER_QUEUE.load(Relaxed) != g!(CyberpunkVR_GtSkipRenderQueueSetting) {
        // SAFETY: as above.
        unsafe {
            CyberpunkVR_GameThreadSkipRenderQueue
                .store(g!(CyberpunkVR_GtSkipRenderQueueSetting) as u32, Relaxed)
        };
        APPLIED_GT_SKIP_RENDER_QUEUE.store(g!(CyberpunkVR_GtSkipRenderQueueSetting), Relaxed);
    }
    s!(CyberpunkVR_SubmitChainSetting, v.xrSubmitChain);
    if APPLIED_SUBMIT_CHAIN.load(Relaxed) != g!(CyberpunkVR_SubmitChainSetting) {
        // SAFETY: Rust's own configure under its C name.
        unsafe { CyberpunkVR_SubmitChainConfigure(g!(CyberpunkVR_SubmitChainSetting)) };
        APPLIED_SUBMIT_CHAIN.store(g!(CyberpunkVR_SubmitChainSetting), Relaxed);
    }
    s!(CyberpunkVR_ViewDataFixMask, v.xrViewDataFixMask);
    s!(CyberpunkVR_EnvMirrorMask, v.xrEnvMirrorMask);
    s!(CyberpunkVR_BdCamDirty, v.xrBdCamDirty);
    s!(CyberpunkVR_BdPushTransform, v.xrBdPushTransform);
    s!(CyberpunkVR_BdMainPosFromScene, v.xrBdMainPosScene);
    s!(CyberpunkVR_BdPushBase, v.xrBdPushBase);
    s!(CyberpunkVR_BdOneComposition, v.xrBdOneComp);
    s!(CyberpunkVR_BdEditorAlign, v.xrBdEditorAlign);
    s!(CyberpunkVR_BdBaseFromLocate, v.xrBdBaseFromLocate);
    s!(CyberpunkVR_CompLendSet, v.xrCompLendSet);
    s!(CyberpunkVR_FinalGrab, v.xrFinalGrab);
    s!(CyberpunkVR_StableSrgbView, v.xrStableSrgbView);
    s!(CyberpunkVR_BdQuatFromWriteSite, v.xrBdQuatWriteSite);
    // Only the three modes exist: 0 off, 1 the turret's route for an UNCLAIMED camera only, 2 for every takeover.
    s!(
        CyberpunkVR_DevCamInLocate,
        if v.xrDevCamInLocate < 0 {
            0
        } else if v.xrDevCamInLocate > 2 {
            2
        } else {
            v.xrDevCamInLocate
        }
    );
    s!(CyberpunkVR_LensHeadWrite, b(v.xrLensHeadWrite != 0));
    s!(CyberpunkVR_InputDefaultInUi, b(v.xrInputDefaultInUi != 0));
    s!(
        CyberpunkVR_PopupMagBlockMs,
        if v.xrPopupMagBlockMs >= 0 && v.xrPopupMagBlockMs <= 10000 {
            v.xrPopupMagBlockMs
        } else {
            1000
        }
    );
    s!(CyberpunkVR_DevCamAnyName, b(v.xrDevCamAnyName != 0));
    s!(
        CyberpunkVR_DevCamTolM,
        if v.xrDevCamTolM > 0.05 && v.xrDevCamTolM < 100.0 {
            v.xrDevCamTolM
        } else {
            1.5
        }
    );
    s!(
        CyberpunkVR_VrcamPosFromMain,
        if v.xrVrcamPosFromMain >= 0 && v.xrVrcamPosFromMain <= 3 {
            v.xrVrcamPosFromMain
        } else {
            0
        }
    );
    s!(CyberpunkVR_BdIpdInLocate, b(v.xrBdIpdInLocate != 0));
    s!(CyberpunkVR_BdQuatFromBuffer, b(v.xrBdQuatFromBuffer != 0));
    s!(CyberpunkVR_CapGrant, v.xrCapGrant);
    s!(CyberpunkVR_ForceVrcamCam, v.xrForceVrcamCam);
    s!(CyberpunkVR_VrcamCamFields, v.xrVrcamCamFields);
    s!(CyberpunkVR_RunViewParams, v.xrRunViewParams);
    s!(CyberpunkVR_ExpoProbe, v.xrExpoProbe);
    s!(CyberpunkVR_IndirectCensus, v.xrIndirectCensus);
    s!(CyberpunkVR_NoStateLies, v.xrNoStateLies);
    s!(CyberpunkVR_StableCopy, v.xrStableCopy);
    s!(CyberpunkVR_DistantReuseMode, v.xrDistantReuse);
    s!(CyberpunkVR_LocalShadowReuseMode, v.xrLocalShadowReuse);
    s!(CyberpunkVR_GiReuseMode, v.xrGiReuse);
    s!(CyberpunkVR_ProbeReuseMode, v.xrProbeReuse);
    s!(CyberpunkVR_ExpoMirror, v.xrExpoMirror);
    s!(CyberpunkVR_ExpoFieldMask, v.xrExpoFieldMask);

    s!(CyberpunkVR_CompLendScoped, v.xrCompLendScoped);
    s!(CyberpunkVR_LightContent, v.xrLightContent);
    s!(CyberpunkVR_FixLodEnable, v.xrFixLod);
    s!(CyberpunkVR_HudToSecondEye, v.xrHudTo2);
    s!(CyberpunkVR_InteractionAnchor, v.xrInteractionAnchor);
    s!(CyberpunkVR_HudClearance, v.xrHudClearance);
    s!(CyberpunkVR_HudClearanceGrow, v.xrHudClearanceGrow);
    s!(CyberpunkVR_HudClearanceHands, v.xrHudClearanceHands);
    s!(
        CyberpunkVR_HudClearanceHandsScale,
        v.xrHudClearanceHandsScale
    );
    s!(CyberpunkVR_AutoRecenter, v.xrAutoRecenter);
    s!(CyberpunkVR_FingerBones, v.xrFingerBones);
    // SAFETY: Rust's own function under its C name.
    unsafe { cvr_fingergrip_set_grip_from_fingers(v.xrGripFromFingers != 0) };
    s!(CyberpunkVR_FingerSpread, v.xrFingerSpread);
    s!(CyberpunkVR_FingerSpreadOn, v.xrFingerSpreadOn);
    s!(CyberpunkVR_AsymProjection, v.xrAsym);
    s!(CyberpunkVR_AsymInnerGuard, v.xrAsymGuard);
    s!(CyberpunkVR_VisionToSecondEye, v.xrVisionTo2);
    s!(CyberpunkVR_HudInBraindance, v.xrHudBd);
    s!(CyberpunkVR_VrcamCompositionGroup, v.xrCompGroup);
    s!(
        CyberpunkVR_TwoHandRadius,
        if v.xrTwoHandRadius < 0.02 {
            0.02
        } else if v.xrTwoHandRadius > 0.30 {
            0.30
        } else {
            v.xrTwoHandRadius
        }
    );
    s!(
        CyberpunkVR_TwoHandAim,
        if v.xrTwoHandAim < 0 {
            0
        } else if v.xrTwoHandAim > 2 {
            2
        } else {
            v.xrTwoHandAim
        }
    );
    s!(
        CyberpunkVR_TwoHandBreakRadius,
        if v.xrTwoHandBreak <= 0.0 {
            0.0
        } else if v.xrTwoHandBreak > 0.60 {
            0.60
        } else {
            v.xrTwoHandBreak
        }
    );
    s!(
        CyberpunkVR_TwoHandAimReleaseMs,
        if v.xrTwoHandAimRelease < 20.0 {
            20.0
        } else if v.xrTwoHandAimRelease > 2000.0 {
            2000.0
        } else {
            v.xrTwoHandAimRelease
        }
    );
    s!(
        CyberpunkVR_CarryGripRadius,
        if v.xrCarryRadius < 0.03 {
            0.03
        } else if v.xrCarryRadius > 0.30 {
            0.30
        } else {
            v.xrCarryRadius
        }
    );
    let range = |v: f32, lo: f32, hi: f32| {
        if v < lo {
            lo
        } else if v > hi {
            hi
        } else {
            v
        }
    };
    s!(CyberpunkVR_RifleRiseMul, range(v.xrRifleRise, 0.0, 2.0));
    s!(CyberpunkVR_RifleBackMul, range(v.xrRifleBack, 0.0, 6.0));
    s!(CyberpunkVR_ShotgunRiseMul, range(v.xrShotgunRise, 0.0, 2.0));
    s!(CyberpunkVR_ShotgunBackMul, range(v.xrShotgunBack, 0.0, 6.0));
    s!(
        CyberpunkVR_RecoilDownPistol,
        range(v.xrDownPistol, 0.0, 1.0)
    );
    s!(CyberpunkVR_RecoilDownRifle, range(v.xrDownRifle, 0.0, 1.0));
    s!(
        CyberpunkVR_RecoilDownShotgun,
        range(v.xrDownShotgun, 0.0, 1.0)
    );
    s!(CyberpunkVR_HandRecoilBackCm, range(v.xrBackCm, 0.0, 20.0));
    s!(
        CyberpunkVR_HandRecoilBackMaxCm,
        range(v.xrBackMaxCm, 0.0, 30.0)
    );
    s!(CyberpunkVR_TwoHandBackMul, range(v.xrTwoHandBack, 0.0, 1.0));
    s!(CyberpunkVR_SniperRiseMul, range(v.xrSniperRise, 0.0, 4.0));
    s!(CyberpunkVR_SniperBackMul, range(v.xrSniperBack, 0.0, 8.0));
    s!(CyberpunkVR_RecoilScale, range(v.xrRecoilScale, 0.0, 2.0));
    s!(CyberpunkVR_Haptics, v.xrHaptics);
    s!(CyberpunkVR_HapticScale, range(v.xrHapticScale, 0.0, 2.0));
    haptic_route::HAPTIC_ROUTE.request(v.xrPsvr2HapticBackend);
    haptic_route::set_legacy_gain(v.xrOpenXrHapticGain);
    haptic_route::set_vehicle_gain(v.xrOpenXrVehicleHapticGain);
    haptic_route::set_vehicle_audio_channels(v.xrVehicleAudioChannels);
    haptic_route::set_vehicle_audio_require_grip(v.xrVehicleAudioRequireGrip != 0);
    s!(CyberpunkVR_PistolBackMul, range(v.xrPistolBack, 0.0, 4.0));
    s!(CyberpunkVR_RecoilClimbFrac, range(v.xrClimbFrac, 0.0, 2.0));
    s!(
        CyberpunkVR_RecoilClimbMaxDeg,
        range(v.xrClimbMax, 0.0, 45.0)
    );
    s!(CyberpunkVR_RecoilClimbMs, range(v.xrClimbMs, 50.0, 5000.0));
    s!(
        CyberpunkVR_HandRecoilReturnMs,
        range(v.xrReturnMs, 40.0, 800.0)
    );
    s!(
        CyberpunkVR_HandRecoilReturnPow,
        range(v.xrReturnPow, 0.0, 1.5)
    );
    // Clamped to one screen either way, and to a scale that leaves something on screen.
    let mut slots = g!(CyberpunkVR_ScannerSlots);
    for i in 0..7 {
        let (sx, sy, ss) = (
            v.xrScannerSlots[i * 3],
            v.xrScannerSlots[i * 3 + 1],
            v.xrScannerSlots[i * 3 + 2],
        );
        slots[i * 3] = range(sx, -1920.0, 1920.0);
        slots[i * 3 + 1] = range(sy, -1080.0, 1080.0);
        slots[i * 3 + 2] = range(ss, 0.10, 5.0);
    }
    s!(CyberpunkVR_ScannerSlots, slots);
    s!(CyberpunkVR_HandRelToFilteredHead, v.xrHandRelFiltered);
    s!(CyberpunkVR_HandLocatePerFrame, v.xrHandPerFrame);
    // Keep the CET-facing bridge file in sync with vrport.ini.
    cvr_write_vrik_settings_file();
    if prev_recenter == 0 && v.xrRecenter != 0 {
        (r.request_recenter)();
        log("OpenXR recenter requested.\n");
    }

    if prev_mono_submit != v.xrMonoSubmit {
        (r.set_mono_submit_enabled)(v.xrMonoSubmit != 0);
        log(&format!(
            "OpenXR mono submit {}.\n",
            if v.xrMonoSubmit != 0 {
                "enabled"
            } else {
                "disabled"
            }
        ));
    }

    // A plain global rather than a live-controls field: producer and consumer are both inside this plugin.
    if g!(CyberpunkVR_ThreadedMonoSubmit) != v.xrThreadedSubmit {
        s!(CyberpunkVR_ThreadedMonoSubmit, v.xrThreadedSubmit);
        let how = if v.xrThreadedSubmit == 0 {
            "INLINE pump (forced)"
        } else if v.xrThreadedSubmit > 0 {
            "submit THREAD (forced)"
        } else {
            "auto"
        };
        log(&format!(
            "OpenXR submit owner: xr_threaded_submit={} -> {how}; resolved now = {}\n",
            v.xrThreadedSubmit,
            if (r.use_threaded_submit)() {
                "submit thread"
            } else {
                "inline pump"
            }
        ));
    }

    // The weapon plane: the launcher's pick until the ini has saved it, else the ini's own key.
    let pending_plane = cvr_settings::launcher::PENDING_WEAPON_PLANE.load(Relaxed);
    if pending_plane >= 0 {
        s!(CyberpunkVR_WeaponPlaneAsScene, b(pending_plane == 0));
    } else if v.savedPlaneAsScene >= 0 {
        s!(CyberpunkVR_WeaponPlaneAsScene, v.savedPlaneAsScene);
    }
    // Applies or reverts the plane-routing bytes to match the keys just parsed (a no-op unless they changed).
    // SAFETY: Rust's own syncs under their C names.
    unsafe {
        cvr_weapon_plane_sync();
        cvr_highlight_any_plane_sync();
        cvr_plane_bucket_scene_sync();
    }

    let save_plane = pending_plane >= 0 && v.savedPlaneAsScene != g!(CyberpunkVR_WeaponPlaneAsScene);
    let mut saved = true;
    if SUBMIT_CHAIN_REV_WRITE.swap(false, Relaxed) || save_plane {
        saved = (r.persist_live_controls)();
    }
    // Saved into the ini: the launcher's file forgets the pick (SaveLauncherConfig(g_launcherWidth, g_launcherHeight)).
    if saved && pending_plane >= 0 {
        cvr_settings::launcher::PENDING_WEAPON_PLANE.store(-1, Relaxed);
        cvr_settings::launcher::cvr_launcher_save(0, 0);
    }

    if changed && verbose() {
        log_c(format(
            c"Live controls updated: xr_head_offset=(%.4f,%.4f,%.4f) xr_recenter=%d xr_mono_submit=%d xr_force_fov=%.3f xr_menu_rect=%d xr_menu_fov=%.3f xr_3dof_movement=%d xr_motion_predict_ms=%.2f xr_stereo_scale=%.3f xr_render_pose_submit=%d xr_runtime=%d\n",
            &[
                Arg::F(lc!(xrHeadOffsetX) as f64),
                Arg::F(lc!(xrHeadOffsetY) as f64),
                Arg::F(lc!(xrHeadOffsetZ) as f64),
                Arg::I(lc!(xrRecenter)),
                Arg::I(lc!(xrMonoSubmit)),
                Arg::F(lc!(xrForceFov) as f64),
                Arg::I(lc!(xrMenuRect)),
                Arg::F(lc!(xrMenuFov) as f64),
                Arg::I(lc!(xr3DofMovement)),
                Arg::F(lc!(xrMotionPredictMs) as f64),
                Arg::F(lc!(xrStereoScale) as f64),
                Arg::I(lc!(xrRenderPoseSubmit)),
                Arg::I(lc!(xrRuntime)),
            ],
        ));
        if lc!(xrRuntime) != 0 {
            log(&format!(
                "Live controls: xr_runtime={} will apply on next startup before OpenXR init.\n",
                lc!(xrRuntime)
            ));
        }
    }
}

/// WriteVrikSettingsFile: the mouse-Y flag for the CET VRIK mod, which reads it from its own folder.
#[no_mangle]
pub extern "C" fn cvr_write_vrik_settings_file() {
    let r = rt();
    (r.init_runtime_paths)();
    let v = b(lc!(xrDisableMouseY) != 0);
    // Only the CET VRIK mod reads it: a clean install of the Rust build has no folder of it (its VRIK is the plugin's),
    // nothing is written then.
    // SAFETY: the C++ half's NUL-terminated path.
    let folder = unsafe { core::ffi::CStr::from_ptr(r.vrik_settings_path) }
        .to_str()
        .ok()
        .and_then(|p| std::path::Path::new(p).parent().map(std::path::Path::is_dir));
    if folder == Some(false) {
        return;
    }
    // SAFETY: the C++ half's path.
    let Some(mut file) = (unsafe { cvr_crt::File::open_shared_mode(r.vrik_settings_path, c"w") })
    else {
        // SAFETY: as above.
        let path = unsafe { core::ffi::CStr::from_ptr(r.vrik_settings_path) };
        log(&format!(
            "VRIK bridge: FAILED to open {} for write\n",
            path.to_string_lossy()
        ));
        return;
    };
    file.puts(&format(c"disable_mouse_y=%d\n", &[Arg::I(v)]));
    drop(file);
    static LAST_LOGGED: AtomicI32 = AtomicI32::new(-1);
    if v != LAST_LOGGED.load(Relaxed) {
        LAST_LOGGED.store(v, Relaxed);
        if verbose() {
            // SAFETY: as above.
            let path = unsafe { core::ffi::CStr::from_ptr(r.vrik_settings_path) };
            log(&format!(
                "VRIK bridge: disable_mouse_y={v} -> {}\n",
                path.to_string_lossy()
            ));
        }
    }
}

/// RequestLiveControlsRecenter: the overlay's recenter button.
#[no_mangle]
pub extern "C" fn RequestLiveControlsRecenter() {
    lc_set!(xrRecenter, 0);
    (rt().request_recenter)();
    log("ImGui: OpenXR recenter requested.\n");
}

/// PollHotkeys: F7 recenters, F8 toggles the small menu rectangle.
#[no_mangle]
pub extern "C" fn cvr_poll_hotkeys() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F7, VK_F8};
    static F7_WAS_DOWN: AtomicBool = AtomicBool::new(false);
    static F8_WAS_DOWN: AtomicBool = AtomicBool::new(false);
    // SAFETY: plain key state queries.
    let (f7, f8) = unsafe {
        (
            GetAsyncKeyState(VK_F7 as i32) as u16 & 0x8000 != 0,
            GetAsyncKeyState(VK_F8 as i32) as u16 & 0x8000 != 0,
        )
    };
    if f7 && !F7_WAS_DOWN.load(Relaxed) {
        (rt().request_recenter)();
        log("Hotkey F7: OpenXR recenter requested.\n");
    }
    if f8 && !F8_WAS_DOWN.load(Relaxed) {
        lc_set!(xrMenuRect, if lc!(xrMenuRect) != 0 { 0 } else { 1 });
        log(&format!(
            "Hotkey F8: xr_menu_rect={} ({}).\n",
            lc!(xrMenuRect),
            if lc!(xrMenuRect) != 0 {
                "small HMD rectangle"
            } else {
                "full HMD rectangle"
            }
        ));
    }
    F7_WAS_DOWN.store(f7, Relaxed);
    F8_WAS_DOWN.store(f8, Relaxed);
}
