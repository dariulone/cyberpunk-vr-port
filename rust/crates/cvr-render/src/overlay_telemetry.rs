//! The compact ADS-camera panel (src/Overlay/OverlayDebugDraw.cpp, `DrawCompactAdsCameraTelemetry`): drawn with the
//! game running so the numbers can be read while aiming -- a window, not the background list, so it reaches both
//! eyes through the second-eye overlay pass. Off by default: an instrument, not a HUD. It shows whether the gameplay
//! camera is available, the aiming state (amber ON, green HIP), MAIN's live zoom and the final one, the shared slot's
//! diagnostic zoom, and -- once a hip-fire baseline is captured -- the camera's delta, peak and raw residual in cm.
//!
//! The Rust build draws it here; the C++ half lends its switch, its position and the shared slots through the
//! attached runtime.

use crate::eye_windows as ew;
use crate::imgui::{begin, end, set_next_window_bg_alpha, set_next_window_pos_pivot, text, text_colored, text_disabled, text_unformatted, Io};
use core::sync::atomic::{AtomicPtr, Ordering::Acquire};
use std::ffi::CString;

/// What the panel reads of the C++ half.
#[repr(C)]
pub struct TelemetryRuntime {
    /// OpenXRManager::GetSharedSlot.
    shared_slot: extern "C" fn(i32) -> f32,
    /// overlay::g_showCompactAdsTelemetry and its position.
    show: *const bool,
    x: *const f32,
    y: *const f32,
}
// SAFETY: the table holds plain functions and addresses of statics.
unsafe impl Sync for TelemetryRuntime {}
/// The table, built here (formerly OverlayDebugDraw.cpp's, attached at its static initialisation): the services are
/// eye_windows.rs's, the switches ImGuiOverlay.cpp's.
static RT: TelemetryRuntime = TelemetryRuntime {
    shared_slot: ew::shared_slot,
    show: &raw const ew::g_showCompactAdsTelemetry,
    x: &raw const ew::g_compactAdsTelemetryX,
    y: &raw const ew::g_compactAdsTelemetryY,
};
static RUNTIME: AtomicPtr<TelemetryRuntime> = AtomicPtr::new((&raw const RT).cast_mut());

/// Overlay/LiveControlsUi.hpp's AdsCameraTelemetryUiState.
#[repr(C)]
#[derive(Default)]
struct AdsCameraTelemetry {
    available: i32,
    aiming: i32,
    baseline_valid: i32,
    samples: u32,
    residual_right: f32,
    residual_forward: f32,
    residual_up: f32,
    delta_right: f32,
    delta_forward: f32,
    delta_up: f32,
    peak_right: f32,
    peak_forward: f32,
    peak_up: f32,
}

#[allow(non_upper_case_globals)]
extern "C" {
    fn GetAdsCameraTelemetryUiState(out: *mut AdsCameraTelemetry);
    static CyberpunkVR_MainAdsZoomFactor: f32;
}

/// ImGuiWindowFlags: NoDecoration | AlwaysAutoResize | NoSavedSettings | NoFocusOnAppearing | NoNav | NoInputs.
const FLAGS: i32 = 43 | (1 << 6) | 256 | (1 << 12) | ((1 << 16) | (1 << 17)) | ((1 << 9) | (1 << 16) | (1 << 17));
const SHARED_ZOOM_SLOT: i32 = 28;

fn line(s: String) -> CString {
    CString::new(s).unwrap_or_default()
}

/// DrawCompactAdsCameraTelemetry.
#[no_mangle]
pub extern "C" fn cvr_overlay_compact_ads_telemetry() {
    let runtime = RUNTIME.load(Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: attached once at load, from a static.
    let r = unsafe { &*runtime };
    // SAFETY: the overlay's switch and position.
    let (show, at_x, at_y) = unsafe { (*r.show, *r.x, *r.y) };
    if !show {
        return;
    }
    let mut t = AdsCameraTelemetry::default();
    // SAFETY: a plain call into a local.
    unsafe { GetAdsCameraTelemetryUiState(&mut t) };
    let display = Io::get().display_size();
    set_next_window_pos_pivot([display[0] * at_x, display[1] * at_y], [0.5, 0.5]);
    set_next_window_bg_alpha(0.72);
    if begin(c"ADS camera telemetry##compact", FLAGS) {
        if t.available == 0 {
            text_unformatted(c"ADS CAM  waiting for gameplay camera...");
        } else {
            let state = if t.aiming != 0 { [1.0, 0.78, 0.24, 1.0] } else { [0.45, 0.9, 0.55, 1.0] };
            // SAFETY: a C++ global.
            let main_zoom = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_MainAdsZoomFactor)) };
            let shared_zoom = (r.shared_slot)(SHARED_ZOOM_SLOT);
            let final_zoom = if main_zoom.is_finite() && main_zoom > 0.0 { main_zoom } else { 1.0 };
            text_colored(state, &line(format!("ADS CAM  {}", if t.aiming != 0 { "ON" } else { "HIP" })));
            text(&line(format!("zoom MAIN {main_zoom:.3}x   final {final_zoom:.3}x")));
            text_disabled(&line(format!("shared[28] {shared_zoom:.3}x   diagnostic only")));
            if t.baseline_valid == 0 {
                text_unformatted(c"Hold hip-fire briefly to capture baseline");
            } else {
                text(&line(format!(
                    "delta cm  R {:+6.2}  F {:+6.2}  U {:+6.2}",
                    t.delta_right * 100.0,
                    t.delta_forward * 100.0,
                    t.delta_up * 100.0
                )));
                text(&line(format!(
                    "peak  cm  R {:6.2}  F {:6.2}  U {:6.2}   n={}",
                    t.peak_right * 100.0,
                    t.peak_forward * 100.0,
                    t.peak_up * 100.0,
                    t.samples
                )));
                text_disabled(&line(format!(
                    "raw   cm  R {:+6.2}  F {:+6.2}  U {:+6.2}",
                    t.residual_right * 100.0,
                    t.residual_forward * 100.0,
                    t.residual_up * 100.0
                )));
            }
        }
    }
    end();
}
