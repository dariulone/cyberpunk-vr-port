//! THE LAUNCHER'S AND THE LIVE SETTINGS' ANSWERS (formerly the getters and setters of src/Core/VrCore.cpp): the startup
//! sequence around the launcher (live controls, the launcher file, the dialog -- or without it the snap and the
//! startup wait), the fit to the runtime's largest eye, the launcher's setters that persist, the forced sizes the
//! window, the swapchain and the game's display modes take (two eyes wide under engine stereo), the field of view and
//! the live settings the other files ask for by these C names, and the game settings' resolution override.
//!
//! The C names are defined here, so the C++ callers link to them directly; the state stays the C++ half's (the
//! launcher's globals through cvr_settings::launcher, the live settings block through cvr_settings::live_controls_block,
//! the rest through the runtime below).
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::*};

use cvr_settings::{launcher, lc, lc_set};

extern "C" {
    fn CyberpunkVR_HeadsetDefaultFovDeg() -> f32;
    fn ApplyLauncherDebugGate();
}

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    /// OpenXRManager::GetRecommendedRenderTargetSize.
    recommended_size: extern "C" fn(*mut u32, *mut u32) -> bool,
    /// OpenXRManager::GetEyeImageLimit.
    eye_image_limit: extern "C" fn(*mut u32, *mut u32) -> bool,
    /// OpenXRManager::IsRuntimeSteamVR.
    runtime_steamvr: extern "C" fn() -> bool,
    /// OpenXRManager::GetRuntimeVerticalFovDeg.
    runtime_vertical_fov_deg: extern "C" fn() -> f32,
    /// LoadLauncherConfig / SaveLauncherConfig (the paths first, then cvr_settings::launcher).
    load_launcher_config: extern "C" fn(),
    save_launcher_config: extern "C" fn(i32, i32),
    /// cvr::sharpen::gameUpscaler.Amount.
    sharpen_amount: extern "C" fn(f32) -> f32,
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()).
    persist_live_controls: extern "C" fn(),
    ensure_live_control_file_exists: extern "C" fn(),
    poll_live_controls: extern "C" fn(),
    /// GetShotShared: the CET shared slots (null before they exist).
    shot_shared: extern "C" fn() -> *mut f32,
    normal_fov_override: *const f32,
    engine_horizontal_fov: *const f32,
    settings_res_ptr: *const usize,
    verbose_log: *const i32,
}
unsafe extern "C" {
    /// VrCore.cpp's: the game upscaler's sharpening amount (cvr::sharpen::gameUpscaler.Amount).
    safe fn cvr_cpp_sharpen_amount(requested: f32) -> f32;
}
/// OpenXRManager's getters and the live controls' and launcher's services, as the C++ table's lambdas did them.
mod services {
    use core::sync::atomic::Ordering::Relaxed;
    pub extern "C" fn recommended_size(width: *mut u32, height: *mut u32) -> bool {
        // SAFETY: the caller's outputs.
        unsafe { cvr_xr::view_configs::cvr_xr_recommended_render_target_size(width, height) }
    }
    /// GetEyeImageLimit: the largest image one eye can be given; false until Init has read it.
    pub extern "C" fn eye_image_limit(width: *mut u32, height: *mut u32) -> bool {
        // SAFETY: the manager's core block, alive for the process.
        let limit = unsafe { (*cvr_xr::core_handles::cvr_xr_core()).eye_image_limit };
        if width.is_null() || height.is_null() || limit[0] == 0 || limit[1] == 0 {
            return false;
        }
        // SAFETY: the caller's outputs.
        unsafe {
            *width = limit[0];
            *height = limit[1];
        }
        true
    }
    pub extern "C" fn runtime_steamvr() -> bool {
        cvr_xr::runtime_info::RUNTIME.steamvr.load(Relaxed)
    }
    pub extern "C" fn runtime_vertical_fov_deg() -> f32 {
        cvr_xr::runtime_info::RUNTIME.vertical_fov_deg.load()
    }
    /// LoadLauncherConfig / SaveLauncherConfig: the runtime paths, then the launcher's file.
    pub extern "C" fn load_launcher_config() {
        crate::runtime_paths::cvr_init_runtime_paths();
        cvr_settings::launcher::cvr_launcher_load();
    }
    pub extern "C" fn save_launcher_config(width: i32, height: i32) {
        crate::runtime_paths::cvr_init_runtime_paths();
        cvr_settings::launcher::cvr_launcher_save(width, height);
    }
    /// PersistLiveControlsUiState(MakeLiveControlsUiState()).
    pub extern "C" fn persist_live_controls() {
        // SAFETY: plain numbers and flags; the maker assigns the whole snapshot.
        let mut state: cvr_settings::live_controls_ui_state::LiveControlsUiState = unsafe { core::mem::zeroed() };
        // SAFETY: a whole snapshot, made then persisted.
        unsafe {
            crate::live_controls_ui::cvr_make_live_controls_ui_state(&mut state);
            crate::live_controls_ui::cvr_persist_live_controls_ui_state(&state);
        }
    }
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here (formerly VrCore.cpp's, attached at its static initialisation).
static RT: Runtime = Runtime {
    recommended_size: services::recommended_size,
    eye_image_limit: services::eye_image_limit,
    runtime_steamvr: services::runtime_steamvr,
    runtime_vertical_fov_deg: services::runtime_vertical_fov_deg,
    load_launcher_config: services::load_launcher_config,
    save_launcher_config: services::save_launcher_config,
    sharpen_amount: cvr_cpp_sharpen_amount,
    persist_live_controls: services::persist_live_controls,
    ensure_live_control_file_exists: crate::runtime_paths::cvr_ensure_live_control_file_exists,
    poll_live_controls: crate::live_controls_poll::cvr_poll_live_controls,
    shot_shared: crate::camera_takeover::cvr_get_shot_shared,
    normal_fov_override: (&raw const crate::vrcore_exports::g_normalFovOverrideValue).cast(),
    engine_horizontal_fov: (&raw const crate::vrcore_exports::g_engineHorizontalFovDeg).cast(),
    settings_res_ptr: (&raw const crate::vrcore_exports::g_settingsResPtr).cast(),
    verbose_log: (&raw const crate::vrcore_exports::g_verboseLog).cast(),
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
/// g_verboseLog, for the debug gate that writes it.
pub(crate) fn verbose_log_ptr() -> *mut i32 {
    rt().verbose_log as *mut i32
}
fn verbose() -> bool {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(rt().verbose_log) != 0 }
}

/// The launcher's ints (g_launcherWidth ...), as the C++ half keeps them.
struct L {
    width: *mut i32,
    height: *mut i32,
    hmd_type: *mut i32,
    debug: *mut i32,
    show: *mut i32,
    delay_ms: *mut i32,
}
fn l() -> L {
    let g =
        launcher::globals().expect("the launcher's globals are attached at static initialisation");
    L {
        width: g.width,
        height: g.height,
        hmd_type: g.hmd_type,
        debug: g.debug,
        show: g.show,
        delay_ms: g.delay_ms,
    }
}
fn save(width: i32, height: i32) {
    (rt().save_launcher_config)(width, height);
}
fn get(p: *mut i32) -> i32 {
    // SAFETY: a launcher global.
    unsafe { *p }
}
fn set(p: *mut i32, v: i32) {
    // SAFETY: as above.
    unsafe { *p = v }
}

// ---- the startup sequence -----------------------------------------------------------------------------------------

/// ClampRuntimeMode: 1 SteamVR, anything else the OpenXR default.
fn clamp_runtime_mode(value: i32) -> i32 {
    (value == 1) as i32
}

// ---- the launcher's picks (main's VrCore.cpp LauncherPicks) -----------------------------------------------------

/// What the dialog shows and sets before VR is chosen: the live poll also installs renderer patches, so until then
/// the dialog's switches are a preview, applied at the first PrepareStartupLiveControls once VR was chosen.
struct LauncherPicks {
    lobby: i32,
    interface_on: i32,
    physics: i32,
    runtime: i32,
    preview: bool,
    submitted: bool,
}
static PICKS: std::sync::Mutex<LauncherPicks> =
    std::sync::Mutex::new(LauncherPicks { lobby: 1, interface_on: 1, physics: 1, runtime: 0, preview: false, submitted: false });
fn picks() -> std::sync::MutexGuard<'static, LauncherPicks> {
    PICKS.lock().unwrap_or_else(|e| e.into_inner())
}
extern "C" {
    static mut CyberpunkVR_WeaponPlaneAsScene: i32;
}

#[no_mangle]
pub extern "C" fn GetLauncherVrEnabled() -> i32 {
    launcher::VR_ENABLED.load(Relaxed)
}
#[no_mangle]
pub extern "C" fn GetLauncherWeaponPlane() -> i32 {
    let pending = launcher::PENDING_WEAPON_PLANE.load(Relaxed);
    // SAFETY: an export, read whole.
    if pending >= 0 { pending } else { (unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_WeaponPlaneAsScene) } == 0) as i32 }
}
/// Start: desktop or VR and the weapon plane, saved with the launcher's file.
#[no_mangle]
pub extern "C" fn SetLauncherModes(vr: i32, plane: i32) {
    launcher::VR_ENABLED.store((vr != 0) as i32, Relaxed);
    launcher::PENDING_WEAPON_PLANE.store((plane != 0) as i32, Relaxed);
    picks().submitted = vr != 0;
    let g = l();
    save(get(g.width), get(g.height));
}

/// ChooseVRLaunch, at the plugin's Load before it registers or hooks anything: the launcher's file, then the dialog
/// (when shown) over the few live keys it displays. False: the desktop game was chosen.
pub fn choose_vr_launch() -> bool {
    crate::runtime_paths::cvr_init_runtime_paths();
    (rt().load_launcher_config)();
    if get(l().show) == 0 {
        return launcher::VR_ENABLED.load(Relaxed) != 0;
    }
    // Read just the controls displayed by the dialog. The live poll also installs renderer patches, so it must wait
    // until VR has actually been selected.
    let path = crate::runtime_paths::live_control_path();
    // SAFETY: the C++ half's path; the C runtime's file.
    if let Some(mut file) = unsafe { cvr_crt::File::open_shared(path) } {
        let mut line = [0 as core::ffi::c_char; 256];
        while file.gets_into(&mut line) {
            let key = |format: &core::ffi::CStr| {
                let mut v = 0i32;
                // SAFETY: a C string line and an int slot.
                (unsafe { cvr_crt::scan(line.as_ptr(), format, &[cvr_crt::ptr(&mut v)]) } == 1).then_some(v)
            };
            if let Some(v) = key(c"xr_weapon_plane_as_scene = %d") {
                // SAFETY: an export, written whole before anything reads it.
                unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_WeaponPlaneAsScene, (v != 0) as i32) };
            }
            let mut p = picks();
            if let Some(v) = key(c"xr_lobby = %d") {
                p.lobby = (v != 0) as i32;
            }
            if let Some(v) = key(c"xr_vr_interface = %d") {
                p.interface_on = (v != 0) as i32;
            }
            if let Some(v) = key(c"xr_hand_physics = %d") {
                p.physics = (v != 0) as i32;
            }
            if let Some(v) = key(c"xr_runtime = %d") {
                p.runtime = clamp_runtime_mode(v);
            }
        }
    }
    picks().preview = true;
    cvr_settings::launcher_dialog::cvr_launcher_show();
    picks().preview = false;
    launcher::VR_ENABLED.load(Relaxed) != 0
}
#[no_mangle]
pub extern "C" fn ChooseVRLaunch() -> bool {
    choose_vr_launch()
}

/// PrepareStartupLiveControls: the live controls, then once the launcher's picks (when VR was chosen in the dialog)
/// -- or without the dialog the snap of a pick the ladders retired and the startup wait.
#[no_mangle]
pub extern "C" fn PrepareStartupLiveControls() {
    static INITIALIZED: AtomicBool = AtomicBool::new(false);
    let r = rt();
    (r.ensure_live_control_file_exists)();
    (r.poll_live_controls)();
    if INITIALIZED.swap(true, SeqCst) {
        return;
    }
    let submitted = {
        let p = picks();
        p.submitted.then_some((p.lobby, p.interface_on, p.physics, p.runtime))
    };
    if let Some((lobby, interface_on, physics, runtime)) = submitted {
        crate::menu_lobby::set_enabled(lobby != 0);
        lc_set!(xrVrInterface, interface_on);
        lc_set!(xrHandPhysics, physics);
        lc_set!(xrRuntime, runtime);
        (r.persist_live_controls)();
        picks().submitted = false;
    }
    let g = l();
    if get(g.show) != 0 {
        return;
    }
    // A pick from a reshaped ladder names a VRCAM component that no longer exists, and an entry only engine stereo
    // offers has none: no second eye. The dialog would move it; without the dialog it moves here.
    let (saved_w, saved_h) = (get(g.width), get(g.height));
    let keep_custom = launcher::CUSTOM_RES.load(Relaxed) != 0 && crate::engine_stereo::enabled();
    let snapped = if keep_custom {
        None
    } else {
        cvr_settings::launcher_presets::snap(get(g.hmd_type), saved_w, saved_h, crate::engine_stereo::enabled())
    };
    if let Some((w, h)) = snapped {
        set(g.width, w);
        set(g.height, h);
        log(&format!(
            "Launcher: {saved_w}x{saved_h} is not in HMD type {}'s ladder -> {w}x{h} (nearest in cost)\n",
            get(g.hmd_type)
        ));
        save(w, h);
    }
    log(&format!(
        "Launcher skipped (show_launcher=0): {}x{}, HMD type {}; startup wait {} ms\n",
        get(g.width),
        get(g.height),
        get(g.hmd_type),
        get(g.delay_ms)
    ));
    cvr_settings::launcher_dialog::cvr_launcher_wait_startup(get(g.delay_ms) as u32);
}

/// THE RUNTIME TAKES NO LARGER AN EYE THAN IT SAYS: once it has answered, before the game's swapchain is sized from the
/// pick, the pick is fitted to its largest eye for this launch (the saved pick stays).
static EYE_SIZE_DECIDED: AtomicBool = AtomicBool::new(false);
#[no_mangle]
pub extern "C" fn FitLauncherResolutionToRuntime() {
    let g = l();
    let (mut max_w, mut max_h) = (0u32, 0u32);
    let (w, h) = (get(g.width), get(g.height));
    // a custom size is scaled into the limit at its own aspect (-1: no ladder to pick from)
    let ladder = if launcher::CUSTOM_RES.load(Relaxed) != 0 { -1 } else { get(g.hmd_type) };
    if (rt().eye_image_limit)(&mut max_w, &mut max_h) {
        if let Some((fw, fh)) = cvr_settings::launcher_presets::fit(
            ladder,
            max_w as i32,
            max_h as i32,
            w,
            h,
            crate::engine_stereo::enabled(),
        ) {
            set(g.width, fw);
            set(g.height, fh);
            log(&format!(
                "Launcher: {w}x{h} is larger than the runtime takes for one eye ({max_w}x{max_h}) -> {fw}x{fh} for this launch\n"
            ));
        }
    }
    EYE_SIZE_DECIDED.store(true, Release);
}
/// One eye's render size for this launch once it is decided (the pick, fitted to the runtime); 0 before.
///
/// # Safety
/// `width` and `height` are writable or null.
#[no_mangle]
pub unsafe extern "C" fn GetLaunchEyeSize(width: *mut i32, height: *mut i32) -> i32 {
    if !EYE_SIZE_DECIDED.load(Acquire) || width.is_null() || height.is_null() {
        return 0;
    }
    let g = l();
    // SAFETY: the caller's slots.
    unsafe {
        *width = get(g.width);
        *height = get(g.height);
    }
    1
}

// ---- the launcher's setters and getters ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn SetWindowResolutionAndPersist(width: i32, height: i32) {
    save(width, height);
}
#[no_mangle]
pub extern "C" fn SetHmdTypeAndPersist(hmd_type: i32) {
    let g = l();
    set(g.hmd_type, hmd_type);
    save(get(g.width), get(g.height));
}
#[no_mangle]
pub extern "C" fn GetLauncherDebug() -> i32 {
    get(l().debug)
}
/// 1 when this launch renders both eyes through engine stereo (the ladder entries without VRCAM assets are offered only
/// then).
#[no_mangle]
pub extern "C" fn GetLauncherEngineStereo() -> i32 {
    crate::engine_stereo::enabled() as i32
}
/// The DEBUG tick-box, persisted and the gate re-armed on the spot (the startup gate ran before the file was read).
#[no_mangle]
pub extern "C" fn SetLauncherDebugAndPersist(on: i32) {
    let g = l();
    set(g.debug, (on != 0) as i32);
    save(get(g.width), get(g.height));
    if !picks().preview {
        // SAFETY: the C++ half's gate.
        unsafe { ApplyLauncherDebugGate() };
    }
}
/// The launcher's "custom size" flag (custom_res in vrport-launcher.ini), saved with the next save.
#[no_mangle]
pub extern "C" fn GetLauncherCustomResolution() -> i32 {
    launcher::CUSTOM_RES.load(Relaxed)
}
#[no_mangle]
pub extern "C" fn SetLauncherCustomResolution(on: i32) {
    launcher::CUSTOM_RES.store((on != 0) as i32, Relaxed);
}
/// The VR interface, hand physics and the VR lobby: the preview's while the dialog shows before VR is chosen, else the
/// live setting, persisted. The launcher closes before the main menu opens, so a change takes effect in this launch.
#[no_mangle]
pub extern "C" fn GetLauncherInterface() -> i32 {
    let p = picks();
    if p.preview { p.interface_on } else { (lc!(xrVrInterface) != 0) as i32 }
}
#[no_mangle]
pub extern "C" fn SetLauncherInterfaceAndPersist(on: i32) {
    {
        let mut p = picks();
        if p.preview {
            p.interface_on = (on != 0) as i32;
            return;
        }
    }
    lc_set!(xrVrInterface, (on != 0) as i32);
    (rt().persist_live_controls)();
}
#[no_mangle]
pub extern "C" fn GetLauncherHandPhysics() -> i32 {
    let p = picks();
    if p.preview { p.physics } else { (lc!(xrHandPhysics) != 0) as i32 }
}
#[no_mangle]
pub extern "C" fn SetLauncherHandPhysicsAndPersist(on: i32) {
    {
        let mut p = picks();
        if p.preview {
            p.physics = (on != 0) as i32;
            return;
        }
    }
    lc_set!(xrHandPhysics, (on != 0) as i32);
    (rt().persist_live_controls)();
}
#[no_mangle]
pub extern "C" fn GetLauncherLobby() -> i32 {
    let p = picks();
    if p.preview { p.lobby } else { crate::menu_lobby::enabled() as i32 }
}
#[no_mangle]
pub extern "C" fn SetLauncherLobbyAndPersist(on: i32) {
    {
        let mut p = picks();
        if p.preview {
            p.lobby = (on != 0) as i32;
            return;
        }
    }
    crate::menu_lobby::set_enabled(on != 0);
    (rt().persist_live_controls)();
}
#[no_mangle]
pub extern "C" fn GetCurrentHmdType() -> i32 {
    get(l().hmd_type)
}
/// The runtime choice into vrport.ini, applied on the OpenXR init that follows the launcher.
#[no_mangle]
pub extern "C" fn SetRuntimeModeAndPersist(mode: i32) {
    {
        let mut p = picks();
        if p.preview {
            p.runtime = clamp_runtime_mode(mode);
            return;
        }
    }
    lc_set!(xrRuntime, clamp_runtime_mode(mode));
    (rt().persist_live_controls)();
}
#[no_mangle]
pub extern "C" fn GetCurrentWindowWidth() -> i32 {
    get(l().width)
}
#[no_mangle]
pub extern "C" fn GetCurrentWindowHeight() -> i32 {
    get(l().height)
}

// ---- the forced sizes ---------------------------------------------------------------------------------------------

/// The runtime's recommended render size (0 before it answers).
#[no_mangle]
pub extern "C" fn cvr_forced_render_size(width: bool) -> u32 {
    let (mut w, mut h) = (0u32, 0u32);
    if (rt().recommended_size)(&mut w, &mut h) {
        let v = if width { w } else { h };
        if v > 0 {
            return v;
        }
    }
    0
}
/// g_launcherWidth is ONE EYE; engine stereo draws both side by side in one target.
fn eyes_across() -> u32 {
    if crate::engine_stereo::enabled() {
        2
    } else {
        1
    }
}
fn forced_window_width() -> u32 {
    let w = get(l().width);
    if w > 0 {
        w as u32 * eyes_across()
    } else {
        cvr_forced_render_size(true) * eyes_across()
    }
}
fn forced_window_height() -> u32 {
    let h = get(l().height);
    if h > 0 {
        h as u32
    } else {
        cvr_forced_render_size(false)
    }
}
#[no_mangle]
pub extern "C" fn GetForcedSwapchainWidth() -> u32 {
    let w = get(l().width);
    if w > 0 {
        w as u32 * eyes_across()
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn GetForcedSwapchainHeight() -> u32 {
    let h = get(l().height);
    if h > 0 {
        h as u32
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn GetForcedDisplayModeWidth() -> u32 {
    forced_window_width()
}
#[no_mangle]
pub extern "C" fn GetForcedDisplayModeHeight() -> u32 {
    forced_window_height()
}
#[no_mangle]
pub extern "C" fn GetForcedWindowWidth() -> u32 {
    forced_window_width()
}
#[no_mangle]
pub extern "C" fn GetForcedWindowHeight() -> u32 {
    forced_window_height()
}

// ---- the live settings' answers -----------------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn GetDisableRoll() -> i32 {
    0
}
/// The ini's number always wins; only when it leaves this at 0 does the per-headset measured default apply.
#[no_mangle]
pub extern "C" fn GetForcedFov() -> f32 {
    let from_ini = lc!(xrForceFov);
    if from_ini > 1.0 && from_ini < 170.0 {
        return from_ini;
    }
    // SAFETY: the C++ half's function.
    unsafe { CyberpunkVR_HeadsetDefaultFovDeg() }
}
#[no_mangle]
pub extern "C" fn GetMenuFov() -> f32 {
    lc!(xrMenuFov)
}
#[no_mangle]
pub extern "C" fn GetMenuFollowDeg() -> f32 {
    let v = lc!(xrMenuFollowDeg);
    if (5.0..=90.0).contains(&v) {
        v
    } else {
        60.0
    }
}
#[no_mangle]
pub extern "C" fn GetMenuRectMode() -> i32 {
    lc!(xrMenuRect)
}
#[no_mangle]
pub extern "C" fn GetSyncSequential() -> i32 {
    if (rt().runtime_steamvr)() {
        return 1;
    }
    (lc!(xrRuntime) == 1) as i32
}
#[no_mangle]
pub extern "C" fn Get3DofMovement() -> i32 {
    lc!(xr3DofMovement)
}
#[no_mangle]
pub extern "C" fn GetMotionPredictMs() -> f32 {
    lc!(xrMotionPredictMs)
}
#[no_mangle]
pub extern "C" fn GetRenderPoseSubmit() -> i32 {
    lc!(xrRenderPoseSubmit)
}
#[no_mangle]
pub extern "C" fn GetDepthSubmit() -> i32 {
    lc!(xrDepthSubmit)
}
#[no_mangle]
pub extern "C" fn GetPoseLag() -> i32 {
    lc!(xrPoseLag)
}
#[no_mangle]
pub extern "C" fn GetVrSharpness() -> f32 {
    lc!(xrSharpness)
}
#[no_mangle]
pub extern "C" fn GetVrSharpmix() -> f32 {
    (rt().sharpen_amount)(lc!(xrSharpmix))
}
#[no_mangle]
pub extern "C" fn GetReuseLastFrameOutput() -> i32 {
    lc!(xrReuseLastFrame)
}
#[no_mangle]
pub extern "C" fn GetXrRuntimeMode() -> i32 {
    {
        let p = picks();
        if p.preview || p.submitted {
            return p.runtime;
        }
    }
    lc!(xrRuntime)
}
#[no_mangle]
pub extern "C" fn GetInputActionsEnabled() -> i32 {
    (lc!(xrInputActions) != 0) as i32
}
#[no_mangle]
pub extern "C" fn GetMonoXQueueWait() -> i32 {
    (lc!(xrMonoXQueueWait) != 0) as i32
}
#[no_mangle]
pub extern "C" fn GetSnapTurnPulseMs() -> i32 {
    let v = lc!(xrSnapTurnPulseMs);
    if v > 0 {
        v
    } else {
        30
    }
}
#[no_mangle]
pub extern "C" fn GetMonoDepthCapture() -> i32 {
    (lc!(xrMonoDepthCapture) != 0) as i32
}
#[no_mangle]
pub extern "C" fn GetSnapTurnYawIndex() -> i32 {
    let v = lc!(xrSnapTurnYawIndex);
    if (0..=3).contains(&v) {
        v
    } else {
        1
    }
}

// ---- the field of view ----------------------------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn GetGameRenderFovDeg() -> f32 {
    // SAFETY: the C++ half's volatile float.
    let f = unsafe { core::ptr::read_volatile(rt().engine_horizontal_fov) };
    if f > 1.0 && f < 170.0 {
        f
    } else {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn GetGameRenderVerticalFovDeg() -> f32 {
    // SAFETY: as above.
    let f = unsafe { core::ptr::read_volatile(rt().normal_fov_override) };
    if f > 1.0 && f < 179.0 {
        f
    } else {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn GetFovOverscan() -> f32 {
    1.0
}
/// The vertical fov to render: the runtime's, times the overscan.
#[no_mangle]
pub extern "C" fn GetTargetRenderVfovDegC() -> f32 {
    let vfov = (rt().runtime_vertical_fov_deg)();
    if !(vfov > 1.0 && vfov < 175.0) {
        return 0.0;
    }
    let mut os = GetFovOverscan();
    if !(1.0..=2.0).contains(&os) {
        os = 1.3;
    }
    let t = vfov * os;
    if t > 1.0 && t < 178.0 {
        t
    } else {
        vfov
    }
}

// ---- the game settings' resolution --------------------------------------------------------------------------------

/// ApplySettingsResolutionOverride: the window's forced size into the game's settings object (+0xB0/+0xB4), suspended
/// while the map is open (shared slot 81).
#[no_mangle]
pub extern "C" fn cvr_apply_settings_resolution_override(settings: usize) {
    let (w, h) = (forced_window_width(), forced_window_height());
    if settings == 0 || w == 0 || h == 0 {
        return;
    }
    static LAST_MAP_FLAG: AtomicU32 = AtomicU32::new(0xFFFF_FFFF);
    let shared = (rt().shot_shared)();
    // SAFETY: the shared slots, 4-byte cells.
    let map_flag = if shared.is_null() {
        0
    } else {
        unsafe { core::ptr::read_volatile((shared as *const u32).add(81)) }
    };
    if LAST_MAP_FLAG.swap(map_flag, Relaxed) != map_flag && verbose() {
        log(&format!(
            "ApplySettingsResOverride: mapFlag[81]={map_flag} -> {}\n",
            if map_flag != 0 {
                "SUSPEND resolution override (map open)"
            } else {
                "apply square"
            }
        ));
    }
    if map_flag != 0 {
        return;
    }
    let _ = cvr_engine::code::write::<u32>(settings + 0xB0, w);
    let _ = cvr_engine::code::write::<u32>(settings + 0xB4, h);
}
/// ApplyKnownResolutionOverrides: the last settings object seen, again (the worker thread, every pass).
#[no_mangle]
pub extern "C" fn cvr_apply_known_resolution_overrides() {
    // SAFETY: the C++ half's volatile pointer.
    let settings = unsafe { core::ptr::read_volatile(rt().settings_res_ptr) };
    if settings != 0 {
        cvr_apply_settings_resolution_override(settings);
    }
}
