//! OpenXRManager's camera mount and HUD capture services (formerly Runtimes/OpenXRCalibration.cpp's GetCameraOffset,
//! BakeCameraOffset and SaveCalibrationToFile, HookServices.cpp's cvr_cpp_xr_camera_offset and OpenXRPresent.cpp's
//! cvr_cpp_xr_hud_capture): the camera offset (the neutral neck mount plus the stored trim) and its reset, as the
//! overlay's calibration panel computes them (overlay_panels.rs), and the HUD channels' capture with the live style.

use core::ffi::c_void;

unsafe extern "C" {
    /// The optics probe's capture (cvr-gpu's optics_probe.rs).
    fn cvr_optics_probe_capture(device: *mut c_void, queue: *mut c_void);
    /// A HUD channel's capture with its brightness, shadow and glow (cvr-gpu's hud_panel.rs).
    fn cvr_hud_panel_capture(device: *mut c_void, queue: *mut c_void, channel: u32, style: *const [f32; 3]);
}

/// The camera offset the locate camera and the present apply: the neutral neck mount plus the stored trim.
///
/// # Safety
/// `out` holds three floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_camera_offset(out: *mut f32) {
    crate::overlay_panels::xr::get_camera_offset(out);
}

/// The camera mount back to the neutral neck: the trim cleared, the live head offsets zeroed and saved, the
/// calibration saved.
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_bake_camera_offset() {
    crate::overlay_panels::xr::bake_camera_offset();
}

/// The HUD channels' capture for the present (Main first, with the optics probe's capture before it; then Interaction,
/// Basilisk, Surveillance, HitMarker and VehicleMinimap), each with the live brightness, shadow and glow.
///
/// # Safety
/// The game's device and queue.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_hud_capture(device: *mut c_void, queue: *mut c_void) {
    for channel in 0..6u32 {
        // SAFETY: the game's device and queue; the live controls block, alive for the process.
        unsafe {
            if channel == 0 {
                cvr_optics_probe_capture(device, queue);
            }
            let controls = &*cvr_settings::live_controls_block::block();
            let style = [controls.xrHudBrightness, controls.xrHudShadow, controls.xrHudGlow];
            cvr_hud_panel_capture(device, queue, channel, &style);
        }
    }
}

/// g_hmdTrackingSmooth (formerly include/Runtimes/OpenXRInternal.hpp's inline atomic, 0.35): the HMD tracking's
/// smoothing setting the live controls keep (the camera uses the runtime's predicted pose without it).
static HMD_TRACKING_SMOOTH: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0.35f32.to_bits());
/// GetHmdTrackingSmooth.
#[no_mangle]
pub extern "C" fn GetHmdTrackingSmooth() -> f32 {
    f32::from_bits(HMD_TRACKING_SMOOTH.load(core::sync::atomic::Ordering::Relaxed))
}
/// SetHmdTrackingSmooth: clamped to [0, 1].
#[no_mangle]
pub extern "C" fn SetHmdTrackingSmooth(value: f32) {
    let clamped = if value < 0.0 { 0.0 } else if value > 1.0 { 1.0 } else { value };
    HMD_TRACKING_SMOOTH.store(clamped.to_bits(), core::sync::atomic::Ordering::Relaxed);
}

/// The VR menu's live controls (formerly Natives/VrMenu.cpp's): the menu gesture and the VR weapon wheel.
///
/// # Safety
/// Both outputs are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_menu_controls(gesture: *mut i32, weapon_wheel: *mut i32) {
    // SAFETY: the live controls block, alive for the process; the caller's outputs.
    unsafe {
        let controls = &*cvr_settings::live_controls_block::block();
        *gesture = controls.xrMenuGesture;
        *weapon_wheel = controls.xrVrWeaponWheel;
    }
}

/// The live world, IPD and stereo scales (formerly Camera/CameraServices.cpp's).
///
/// # Safety
/// `out` holds three floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_camera_scales(out: *mut f32) {
    // SAFETY: the live controls block, alive for the process; the caller's three floats.
    unsafe {
        let controls = &*cvr_settings::live_controls_block::block();
        *out = controls.xrWorldScale;
        *out.add(1) = controls.xrIpdScale;
        *out.add(2) = controls.xrStereoScale;
    }
}

/// The physical crouch's live switch and depth (formerly Runtimes/PhysicalCrouch.cpp's).
///
/// # Safety
/// Both outputs are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_live_physical_crouch(enabled: *mut i32, depth: *mut f32) {
    // SAFETY: as above.
    unsafe {
        let controls = &*cvr_settings::live_controls_block::block();
        *enabled = controls.xrPhysicalCrouch;
        *depth = controls.xrPhysicalCrouchDepth;
    }
}

/// gaze::Read: the published gaze centre {tangentY, ndcY} while the head looks, else the zero centre.
fn gaze_center() -> (f32, f32) {
    // SAFETY: the gaze's static block, alive for the process.
    let g = unsafe { &*cvr_xr::gaze::cvr_xr_gaze_shared() };
    if !g.head_look.load(core::sync::atomic::Ordering::Relaxed) {
        return (0.0, 0.0);
    }
    let bits = g.center_bits.load(core::sync::atomic::Ordering::Acquire);
    (f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32))
}
/// The gaze centre's vertical tangent (formerly Hooks/HookServices.cpp's).
#[no_mangle]
pub extern "C" fn cvr_cpp_gaze_tangent_y() -> f32 {
    gaze_center().0
}
/// The head-look switch (formerly Natives/HudPanel.cpp's).
#[no_mangle]
pub extern "C" fn cvr_cpp_gaze_head_look(on: bool) {
    // SAFETY: as above.
    unsafe { &*cvr_xr::gaze::cvr_xr_gaze_shared() }.head_look.store(on, core::sync::atomic::Ordering::Relaxed);
}
/// gaze::CanvasOffset of the current centre (formerly Natives/HudPanel.cpp's): the canvas shifted to the gaze's height.
#[no_mangle]
pub extern "C" fn cvr_cpp_gaze_canvas_offset(height: f32, design_scale: f32) -> f32 {
    if height > 0.0 && design_scale > 0.0 { -gaze_center().1 * height * 0.5 / design_scale } else { 0.0 }
}

// ---- sharpen::UpscalerGate (formerly include/Render/SharpenPolicy.hpp's inline gameUpscaler): whether the game's own
// upscaler is DLSS, and its quality, so CAS sharpening stays off over DLSS without a known non-DLAA quality.
const UPSCALER_SELECTED: u32 = 1;
const UPSCALER_KNOWN: u32 = 2;
const UPSCALER_DLAA: u32 = 4;
static UPSCALER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// UpscalerGate::SelectDlss.
#[no_mangle]
pub extern "C" fn cvr_cpp_select_dlss(selected: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    if selected {
        UPSCALER.fetch_or(UPSCALER_SELECTED, Relaxed);
    } else {
        UPSCALER.store(0, Relaxed);
    }
}
/// UpscalerGate::ObserveGameQuality: the original game's NGX evaluation of the main view (never the private foveation
/// passes, whose periphery intentionally uses DLAA).
#[no_mangle]
pub extern "C" fn cvr_cpp_observe_game_quality(view: u32, quality: u32) {
    if view != 1 {
        return;
    }
    let state = UPSCALER_SELECTED
        | if quality <= 5 { UPSCALER_KNOWN } else { 0 }
        | if quality == 5 { UPSCALER_DLAA } else { 0 };
    UPSCALER.store(state, core::sync::atomic::Ordering::Relaxed);
}
/// UpscalerGate::BlocksCas.
#[no_mangle]
pub extern "C" fn cvr_cpp_blocks_cas() -> bool {
    let value = UPSCALER.load(core::sync::atomic::Ordering::Relaxed);
    value & UPSCALER_SELECTED != 0 && (value & UPSCALER_KNOWN == 0 || value & UPSCALER_DLAA != 0)
}
/// UpscalerGate::Amount: the requested sharpening clamped to [0, 1], none while CAS is blocked or not finite.
#[no_mangle]
pub extern "C" fn cvr_cpp_sharpen_amount(requested: f32) -> f32 {
    if cvr_cpp_blocks_cas() || !requested.is_finite() { 0.0 } else { requested.clamp(0.0, 1.0) }
}

/// OpenXRManager::PushRenderHeadPose (formerly Hooks/HookServices.cpp's service): the pose a frame is rendered with,
/// labelled with the present count.
///
/// # Safety
/// `pose` is an OpenXRHeadPose or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_push_render_head_pose(pose: *const c_void) {
    if pose.is_null() {
        return;
    }
    // SAFETY: the presented block, alive for the process; the caller's pose.
    unsafe {
        let present = (*cvr_xr::presented::cvr_xr_presented_shared()).present_count.load(core::sync::atomic::Ordering::Relaxed);
        cvr_xr::frame_identity::cvr_xr_push_render_head_pose(pose.cast(), present);
    }
}
/// OpenXRManager::GetCurrentRenderEyeIndex: nothing in the Rust build sets the manager's render eye index, which stays
/// 0 (formerly Hooks/HookServices.cpp's service).
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_render_eye_index() -> i32 {
    0
}
/// OpenXRManager::ExternalPoseResetPending (formerly Camera/CameraServices.cpp's service).
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_external_pose_reset_pending() -> bool {
    cvr_xr::published::external_reset_pending()
}

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// src/Core/VrCore.cpp: `void* volatile g_dbgFovCamState` (the FOV hub's camera state, for a debugger).
    #[link_name = "?g_dbgFovCamState@@3REAXEA"]
    static mut g_dbgFovCamState: *mut c_void;
    /// `volatile float g_dbgLastOriginalFov` (cpp_services.rs's).
    #[link_name = "?g_dbgLastOriginalFov@@3MC"]
    static mut g_dbgLastOriginalFov: f32;
}
/// The FOV hub's diagnostics (formerly Hooks/CameraFov.cpp's): the original FOV, the camera state, the engine's
/// horizontal FOV. (The LOD FOV the C++ kept beside them was never read.)
#[no_mangle]
pub extern "C" fn cvr_cpp_fov_publish(camera_state: usize, original: f32, horizontal: f32, _lod: f32) {
    // SAFETY: the globals' plain stores, as the C++ wrote them.
    unsafe {
        core::ptr::write_volatile(&raw mut g_dbgLastOriginalFov, original);
        core::ptr::write_volatile(&raw mut g_dbgFovCamState, camera_state as *mut c_void);
    }
    crate::vrcore_exports::g_engineHorizontalFovDeg.store(horizontal.to_bits(), core::sync::atomic::Ordering::Relaxed);
}

/// The remote camera's gates by address (formerly Natives/RemoteCamera.cpp's; remote_camera.rs's State).
#[repr(C)]
pub struct RemoteCameraState {
    remote_cam_on: *const core::sync::atomic::AtomicI32,
    remote_cam_pos_fp: *const [core::sync::atomic::AtomicI32; 3],
    player_cam_on: *const core::sync::atomic::AtomicI32,
    player_cam_pos_fp: *const [core::sync::atomic::AtomicI32; 3],
    bd_scene_fov: *const core::sync::atomic::AtomicU32,
    takeover_entity_id: *const core::sync::atomic::AtomicU64,
    bd_want_fov_milli: *const core::sync::atomic::AtomicI32,
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_remote_camera_state(out: *mut RemoteCameraState) {
    use crate::vrcore_exports as v;
    // SAFETY: the caller's output; the statics live for the process.
    unsafe {
        *out = RemoteCameraState {
            remote_cam_on: &v::g_remoteCamOn,
            remote_cam_pos_fp: &v::g_remoteCamPosFP,
            player_cam_on: &v::g_playerCamOn,
            player_cam_pos_fp: &v::g_playerCamPosFP,
            bd_scene_fov: &v::g_bdSceneFov,
            takeover_entity_id: &v::g_takeoverEntityId,
            bd_want_fov_milli: &v::g_bdWantFovMilli,
        };
    }
}
