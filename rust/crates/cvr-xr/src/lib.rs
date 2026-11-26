//! The port's OpenXR (stage 4, docs/rust-migration-plan.md). src/Runtimes/OpenXRManager.cpp hands its parts over one
//! at a time; the C++ class keeps the handles it is given and the parts not yet here.
//!
//!   instance  the instance and the system: the optional extensions, the runtime's identity (SteamVR, Virtual
//!             Desktop, the OpenXR Simulator, PSVR2), the system's limits and the views' recommended sizes
//!   actions   the gameplay action set, its actions and every interaction profile's suggested bindings
//!   session   the session on the game's D3D12 device and queue, the tracking and view spaces, the hands' spaces
//!   events    the session's life: its events, begun and ended, presence recenters; it owns the session's state
//!   teardown  the haptic pulse, and the spaces, session, action set and instance destroyed at shutdown
//!   hud       the panels' yaw-follow: the head's heading, the display clock, the free-look cone and its catch-up
//!   hud_quad  a HUD channel's quad layer: its swapchain, the canvas copies, the panel's placement
//!   head      the head on the XR thread: the yaw-only recenter base, the head filters
//!   hands     the hands' poses: located, made head-relative, their motion, where they point (the aim yaw)
//!   simulator_recenter  the OpenXR Simulator's Reset View bracketed as an external pose reset
//!   calibration  the VRIK calibration: the T-pose, the body from the height, vrik_calibration.ini; it owns its state
//!   frame     one head per frame: the aim instant, the cycle's latch, LocateHeadPoseAt, AcquireFrameHeadSample
//!   frame_identity  one identity per frame (owned here): the frame slots, the display-time fit, the rendered-frame
//!             queues, the render-head ring, the per-eye render poses
//!   posture   the body's posture with each frame sample: the hinge, the crouch's depth, the physical crouch
//!   published the hands published whole (FlushHandsToShared), what was published, the external reset's gate
//!   publish   the published hands' filters (reacquire, UEVR's lerp) and the seqlock packet for the VRIK solve
//!   present_diag  Present's diagnostics: the game's frame interval bucketed, the two-second VRIK rate census
//!   runtime_select  xr_runtime=1: SteamVR's manifest found and XR_RUNTIME_JSON set before the instance
//!   resubmit  the last accepted frame sent once more for a display cycle with nothing new
//!   menu      the menu panel's pose: anchored ahead, lazy yaw-follow, held when the head drops (owned here)
//!   mono_eyes the eyes a presented mono frame is submitted with: the rendered centre, the slot's eye offsets, the FOV
//!   swapchains the eye swapchains: their format, images, render-target views, the submit's allocators, lists, fence
//!   tracking  the head's tracking state (owned here): the published head, the recenter base and its lock, the origin
//!   views     the eyes' views (owned here) and their getters: an eye's offset from the centre, its FOV; the cycle's
//!             xrLocateViews with the runtime's FOVs, IPD and gaze centre
//!   submit_thread  who drives the frame loop (the inline pump or the submit thread), and the submit thread
//!   cycle     the XR cycle's glue: the hands and the controllers of one cycle (sync, trackers, hands, controllers)
//!   xr_rates  the XR loop's rate reports ([xrrate], [xrloop], [xrcadence], [xrcycle], [xrorder] and the deep ones)
//!   srw       the lock both halves take (a slim reader/writer lock)
//!   input     the controllers read for the game: the actions mapped onto XInput, the D-pad chord, the deferred L3
//!
//! The OpenXR loader is linked into the DLL from source (CMakeLists.txt, OpenXR-SDK release-1.0.34); the functions here
//! resolve against it at the DLL's link.
pub mod actions;
pub mod calibration;
pub mod core_handles;
pub mod cycle;
pub mod events;
pub mod frame;
// The frame generation's statistics panel as an XR quad (formerly src/Framegen/StatsOverlay.cpp's Prepare and Shutdown).
pub mod framegen_overlay;
pub mod frame_identity;
pub mod haptic_bridge;
pub mod hands;
pub mod head;
/// The panels' yaw-follow lives in cvr-body (pure, so the test benches link it without the DLL).
pub use cvr_body::hud;
pub mod hud_natives;
pub mod hud_quad;
pub mod input;
pub mod manager_init;
pub mod menu;
pub mod mono_eyes;
pub mod posture;
pub mod present;
pub mod presented;
pub mod submit_state;
pub mod weapon;
pub mod runtime_info;
pub mod runtime_queue;
pub mod xr_trace;
pub mod view_configs;
pub mod gaze;
pub mod capture_ring;
pub mod switches;
pub mod present_diag;
pub mod publish;
pub mod published;
pub mod resubmit;
pub mod runtime_select;
pub mod instance;
pub mod session;
pub mod simulator_recenter;
pub mod srw;
pub mod submit_thread;
pub mod swapchains;
pub mod tracking;
pub mod trackers;
pub mod views;
pub mod vr_panel;
pub mod xr_rates;
pub mod teardown;
pub use instance::{classify_runtime, is_psvr2_system, InstanceSetup};

/// CyberpunkVR_RuntimeDiagnostics (include/Utils/DebugGate.hpp): the diagnostic snapshots and counters are on.
#[cfg(not(test))]
pub fn present_diag_enabled() -> bool {
    extern "C" {
        static CyberpunkVR_RuntimeDiagnostics: i32;
    }
    // SAFETY: a plain C int the launcher's gate sets.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 }
}

/// The unit tests' stand-ins for what the DLL links from elsewhere: the C++ subsystems' settings parser (cvr-settings
/// names it) and the OpenXR loader's swapchain calls the VR panel makes (no runtime in a test: each fails).
#[cfg(test)]
mod test_stubs {
    use core::ffi::{c_char, c_void};
    use openxr_sys as xr;
    #[no_mangle]
    extern "C" fn cvr_cpp_settings_parse_subsystem(_which: i32, _line: *const c_char, _s: *mut c_void) -> bool {
        false
    }
    macro_rules! no_runtime {
        ($($name:ident),*) => {$(
            #[no_mangle]
            extern "system" fn $name() -> xr::Result {
                xr::Result::ERROR_RUNTIME_FAILURE
            }
        )*};
    }
    no_runtime!(
        xrEnumerateSwapchainFormats,
        xrCreateSwapchain,
        xrEnumerateSwapchainImages,
        xrDestroySwapchain,
        xrAcquireSwapchainImage,
        xrWaitSwapchainImage,
        xrReleaseSwapchainImage
    );
}
