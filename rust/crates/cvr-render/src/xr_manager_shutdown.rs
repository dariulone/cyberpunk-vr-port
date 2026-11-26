//! OpenXRManager's Init and Shutdown and the plugin's unload (formerly Runtimes/OpenXRManager.cpp's and Main.cpp's):
//! the manager's init lock is a Rust mutex; Init constructs the manager (its C++ objects, through the frame objects'
//! service) before the runtime's own init; Shutdown stops the runtime, shuts the frame's objects down (the frame
//! generation, the stereo depth, the overlays and panels, the quads, the intro, the loading screen and the HUD channels)
//! and tears the runtime down, all under that lock.

use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::Mutex;

/// OpenXRManager::m_initMutex.
static INIT: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    /// Runtimes/OpenXRFrameLoop.cpp: the manager's objects as their Rust handles (OpenXRManager::Get() constructs it).
    fn cvr_cpp_xr_frame_objects(quads: *mut *mut c_void, framegen_overlay: *mut *mut c_void, settings_panel: *mut *mut c_void);
    fn cvr_foveated_dlss_shutdown();
    fn cvr_vrs_shutdown();
    fn cvr_xr_remove_simulator_recenter();
    fn cvr_xr_manager_init() -> bool;
    fn cvr_xr_manager_stop();
    fn cvr_xr_manager_teardown();
    fn cvr_framegen_set_depth_capture_enabled(value: bool);
    fn cvr_framegen_presenter_reset();
    fn cvr_xr_depth_poll();
    fn cvr_xr_framegen_overlay_shutdown(overlay: *mut c_void, wait: bool);
    fn cvr_vrui_panel_shutdown(panel: *mut c_void);
    fn cvr_gpu_hw_telemetry_stop();
    fn cvr_xr_hud_quad_shutdown(quad: *mut c_void);
    fn cvr_intro_overlay_shutdown();
    /// cvr::haptics::Clear (cvr-rig).
    fn cvr_haptics_clear();
    fn cvr_hud_panel_shutdown(channel: u32);
    fn cvr_overlay_set_device_and_queue(device: *mut c_void, queue: *mut c_void);
}

/// The manager's frame objects: its five quads (the HUD, interaction, Basilisk, surveillance and hit marker channels'),
/// the frame generation overlay and the settings panel.
fn frame_objects() -> ([*mut c_void; 5], *mut c_void, *mut c_void) {
    let mut quads = [null_mut(); 5];
    let (mut overlay, mut panel) = (null_mut(), null_mut());
    // SAFETY: room for each.
    unsafe { cvr_cpp_xr_frame_objects(quads.as_mut_ptr(), &mut overlay, &mut panel) };
    (quads, overlay, panel)
}

/// InitOpenXREarly's init (formerly cvr_cpp_xr_init -> OpenXRManager::Init): the manager constructed, then the
/// runtime's init under the init lock.
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_init() {
    let _ = frame_objects();
    let _init = INIT.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the runtime's own init.
    if unsafe { cvr_xr_manager_init() } {
        // The capture is shared; the haptics' tick routes the road's rumble through the chosen actuators.
        crate::vehicle_audio::start();
    }
}

/// OpenXRManager::Shutdown.
fn shutdown() {
    crate::vehicle_audio::stop();
    // SAFETY: the subsystems' own shutdowns, in the C++ order.
    unsafe {
        cvr_haptics_clear();
        cvr_foveated_dlss_shutdown();
        cvr_vrs_shutdown();
        cvr_xr_remove_simulator_recenter();
    }
    let _init = INIT.lock().unwrap_or_else(|p| p.into_inner());
    let (quads, overlay, panel) = frame_objects();
    // SAFETY: as above; the objects are the manager's, alive for the process.
    unsafe {
        cvr_xr_manager_stop();
        cvr_framegen_set_depth_capture_enabled(false);
        cvr_framegen_presenter_reset();
        cvr_xr_depth_poll();
        cvr_xr_framegen_overlay_shutdown(overlay, true);
        cvr_vrui_panel_shutdown(panel);
        cvr_gpu_hw_telemetry_stop();
        for quad in quads {
            cvr_xr_hud_quad_shutdown(quad);
        }
        cvr_xr_hud_quad_shutdown(cvr_xr::hud_quad::minimap_quad().cast());
        cvr_intro_overlay_shutdown();
        crate::loading_host::shutdown();
        // The HUD channels: Main, Interaction, Basilisk, Surveillance, HitMarker, VehicleMinimap.
        for channel in 0..6 {
            cvr_hud_panel_shutdown(channel);
        }
        // m_views.clear().
        *(*cvr_xr::views::cvr_xr_views_shared()).count.get() = 0;
        cvr_xr_manager_teardown();
    }
    crate::xr_capture::release_desktop_eye_heap();
}

/// The plugin's unload (formerly Main.cpp's): the overlay drops its device references, then the manager shuts down --
/// while D3D is still alive.
#[no_mangle]
pub extern "C" fn cvr_cpp_shutdown_overlay_and_xr() {
    // SAFETY: the overlay's own reset.
    unsafe { cvr_overlay_set_device_and_queue(null_mut(), null_mut()) };
    shutdown();
}
