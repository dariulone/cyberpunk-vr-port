//! The XR manager's frame objects (formerly members of the C++ OpenXRManager singleton, handed to the Rust cycle by
//! src/Runtimes/OpenXRFrameLoop.cpp's cvr_cpp_xr_frame_objects): the five HUD quads, the frame generation's stats overlay
//! and the settings panel. Made on first use in the C++ members' order (the overlay, the quads by channel, the panel);
//! destroyed at the DLL's exit in the reverse order, followed by the stereo depth renderer's shutdown that the manager's
//! depth member ran (the singleton's destructor, registered the same way when it was first built).
use core::ffi::c_void;
use std::sync::OnceLock;

unsafe extern "C" {
    fn cvr_xr_framegen_overlay_new() -> *mut c_void;
    fn cvr_xr_framegen_overlay_free(overlay: *mut c_void);
    fn cvr_xr_hud_quad_new(channel: u32) -> *mut c_void;
    fn cvr_xr_hud_quad_free(quad: *mut c_void);
    fn cvr_vrui_panel_new() -> *mut c_void;
    fn cvr_vrui_panel_free(panel: *mut c_void);
    /// depth_layer.rs: the stereo depth renderer freed.
    fn cvr_xr_depth_shutdown();
    /// The CRT's: runs `callback` at the DLL's exit, after what was registered later.
    fn atexit(callback: extern "C" fn()) -> i32;
}

struct Objects {
    quads: [usize; 5],
    /// cvr::hud::Channel::VehicleMinimap's, registered with cvr-xr's layers (main's m_vehicleMinimapQuad).
    minimap: usize,
    framegen_overlay: usize,
    settings_panel: usize,
}
static OBJECTS: OnceLock<Objects> = OnceLock::new();

fn objects() -> &'static Objects {
    OBJECTS.get_or_init(|| {
        // SAFETY: the constructors' allocations, owned here until the DLL's exit.
        unsafe {
            let framegen_overlay = cvr_xr_framegen_overlay_new() as usize;
            // cvr::hud::Channel: Main, Interaction, Basilisk, Surveillance, HitMarker.
            let quads = [0u32, 1, 2, 3, 4].map(|channel| cvr_xr_hud_quad_new(channel) as usize);
            let minimap = cvr_xr_hud_quad_new(cvr_xr::hud_quad::VEHICLE_MINIMAP) as usize;
            cvr_xr::hud_quad::set_minimap_quad(minimap as *mut cvr_xr::hud_quad::Quad);
            let settings_panel = cvr_vrui_panel_new() as usize;
            atexit(destroy);
            Objects { quads, minimap, framegen_overlay, settings_panel }
        }
    })
}

extern "C" fn destroy() {
    let Some(objects) = OBJECTS.get() else { return };
    // SAFETY: the objects made above, freed once at the DLL's exit.
    unsafe {
        cvr_vrui_panel_free(objects.settings_panel as *mut c_void);
        cvr_xr::hud_quad::set_minimap_quad(core::ptr::null_mut());
        cvr_xr_hud_quad_free(objects.minimap as *mut c_void);
        for &quad in objects.quads.iter().rev() {
            cvr_xr_hud_quad_free(quad as *mut c_void);
        }
        cvr_xr_framegen_overlay_free(objects.framegen_overlay as *mut c_void);
        cvr_xr_depth_shutdown();
    }
}

/// The frame objects for the cycle (xr_manager_loop.rs) and the shutdown (xr_manager_shutdown.rs).
///
/// # Safety
/// `quads` has room for five pointers; the other two are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_frame_objects(quads: *mut *mut c_void, framegen_overlay: *mut *mut c_void, settings_panel: *mut *mut c_void) {
    let objects = objects();
    // SAFETY: the caller's out-pointers.
    unsafe {
        for (q, &quad) in objects.quads.iter().enumerate() {
            *quads.add(q) = quad as *mut c_void;
        }
        *framegen_overlay = objects.framegen_overlay as *mut c_void;
        *settings_panel = objects.settings_panel as *mut c_void;
    }
}
