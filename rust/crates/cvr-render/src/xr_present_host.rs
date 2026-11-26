//! The Present's XR host and the mono frame's capture (formerly src/Runtimes/OpenXRPresent.cpp's OnPresent and
//! src/Runtimes/OpenXRCapture.cpp's CaptureMonoPresentedFrame and EnsureMonoCaptureResource): the host cvr-xr's
//! present.rs runs the present with -- the camera offset, the capture, the subsystems' ticks -- and the capture's
//! callbacks into xr_capture.rs. The game's device and queue and the capture's ring are cvr-xr's blocks; the loading
//! screen (its placement and caption) and the HUD channels' capture stay the C++ half's services.

use core::ffi::c_void;
use core::sync::atomic::Ordering::Relaxed;
use cvr_camera::projection::EyeWindow;
use openxr_sys as xr;
use std::sync::Mutex;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC;

use crate::xr_capture::CaptureFrameHostIn;

/// cvr-xr present.rs's PresentHost (its abi module's), as the manager filled it.
#[repr(C)]
struct PresentHost {
    ctx: *mut c_void,
    camera_offset: unsafe extern "C" fn(*mut c_void, *mut f32),
    #[allow(clippy::type_complexity)]
    capture_mono: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *const D3D12_RESOURCE_DESC,
        u64,
        *const xr::Posef,
        *const xr::Fovf,
        *const bool,
        *const xr::Posef,
    ) -> bool,
    ticks: unsafe extern "C" fn(*mut c_void),
}
#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// cvr-xr's present.rs.
    fn cvr_xr_on_present(host: *const PresentHost, swap_chain: *mut c_void);
    // the C++ half's services
    /// OpenXRManager::GetCameraOffset (src/Hooks/HookServices.cpp).
    fn cvr_cpp_xr_camera_offset(out: *mut f32);
    /// The HUD channels' capture (main, interaction, Basilisk, surveillance, hit marker) (src/Runtimes/OpenXRPresent.cpp).
    fn cvr_cpp_xr_hud_capture(device: *mut c_void, queue: *mut c_void);
    /// volatile int g_verboseLog (src/Core/VrCore.cpp).
    #[link_name = "?g_verboseLog@@3HC"]
    static g_verboseLog: i32;
}

/// The game's device and queue (cvr-xr's submit block).
fn device_queue() -> (*mut c_void, *mut c_void) {
    // SAFETY: cvr-xr's block, alive for the process.
    let shared = unsafe { &*cvr_xr::submit_state::cvr_xr_submit_shared() };
    // SAFETY: plain pointers the init writes once.
    unsafe { (*shared.device.get(), *shared.queue.get()) }
}

/// OpenXRManager::OnPresent.
pub fn on_present(swap_chain: *mut c_void) {
    // CVR_HITCH_TRACE: a game frame far longer than its pace (the interval between the game's Presents) and the port's
    // own Present work over 6 ms are logged
    static LAST: std::sync::Mutex<f64> = std::sync::Mutex::new(0.0);
    let span = cvr_host::hitch::start();
    if span != 0.0 {
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        if *last != 0.0 && (span - *last) * 1000.0 >= 30.0 {
            cvr_host::log!("[hitch] q={:.4} game frame {:.1} ms between Presents (Present at q={span:.4})
", *last, (span - *last) * 1000.0);
        }
        *last = span;
    }
    let host = PresentHost { ctx: core::ptr::null_mut(), camera_offset, capture_mono, ticks };
    // SAFETY: a host of this module's functions; the game's swapchain (or null).
    unsafe { cvr_xr_on_present(&host, swap_chain) };
    cvr_host::hitch::end(span, 6.0, || "the port's Present work (OpenXRManager::OnPresent)".to_string());
}
unsafe extern "C" fn camera_offset(_: *mut c_void, out: *mut f32) {
    // SAFETY: the present's three floats.
    unsafe { cvr_cpp_xr_camera_offset(out) };
}
/// The foveated rendering's and the VRS's ticks, then the HUD channels' capture.
unsafe extern "C" fn ticks(_: *mut c_void) {
    let (device, queue) = device_queue();
    let phase = cvr_xr::xr_trace::phase_start();
    let span = cvr_host::hitch::start();
    crate::foveated_dlss::cvr_foveated_dlss_tick();
    // SAFETY: the game's device and queue (or null, which the tick checks).
    unsafe {
        crate::fixed_vrs::cvr_vrs_tick(device, queue);
        cvr_host::hitch::end(span, 3.0, || "foveated DLSS + VRS ticks at Present".to_string());
        cvr_xr::xr_trace::phase_end(cvr_xr::xr_trace::PHASE_TICKS, phase);
        let phase = cvr_xr::xr_trace::phase_start();
        let span = cvr_host::hitch::start();
        cvr_cpp_xr_hud_capture(device, queue);
        cvr_host::hitch::end(span, 3.0, || "HUD channels' capture at Present".to_string());
        cvr_xr::xr_trace::phase_end(cvr_xr::xr_trace::PHASE_HUD, phase);
    }
}

/// m_captureMutex: one capture at a time.
static CAPTURE: Mutex<()> = Mutex::new(());

/// OpenXRManager::EnsureMonoCaptureResource: the capture's ring and the eye's pool for this source.
fn ensure_mono_capture_resource(desc: &D3D12_RESOURCE_DESC) -> bool {
    let (device, queue) = device_queue();
    if device.is_null() || queue.is_null() {
        return false;
    }
    if desc.Width == 0 || desc.Height == 0 || desc.Format.0 == 0 {
        return false;
    }
    // SAFETY: cvr-xr's ring block, written under the capture's lock.
    let ring = unsafe { &mut *cvr_xr::capture_ring::cvr_xr_capture_ring() };
    // SAFETY: the game's device and the ring's slots.
    unsafe {
        crate::xr_capture_state::cvr_xr_capture_ensure_ring(
            device,
            ring.allocators.as_mut_ptr().cast(),
            ring.lists.as_mut_ptr().cast(),
            (&raw mut ring.fence).cast(),
            (&raw mut ring.fence_event).cast(),
        ) && crate::xr_capture_state::cvr_xr_capture_ensure_pool(device, desc)
    }
}

extern "C" fn ensure_resources(_: *mut c_void, eye: *const D3D12_RESOURCE_DESC) -> bool {
    // SAFETY: the capture's eye description.
    ensure_mono_capture_resource(unsafe { &*eye })
}
extern "C" fn loading_end(_: *mut c_void) {
    crate::loading_host::end();
}
extern "C" fn loading_begin(_: *mut c_void, notice: bool, opacity: f32, hold: bool, head: *const xr::Posef) {
    // SAFETY: the capture's head pose.
    unsafe { crate::loading_host::begin(notice, opacity, hold, head) };
}
#[allow(clippy::too_many_arguments)]
extern "C" fn loading_record(
    _: *mut c_void,
    target: *mut c_void,
    view: i32,
    rect: *const RECT,
    pose: *const xr::Posef,
    fov: *const xr::Fovf,
    window: *mut c_void,
    window_view: i32,
    uv: *const f32,
    aspect: f32,
) -> bool {
    // the capture's current list: the ring's, at its allocator index
    // SAFETY: cvr-xr's ring block, read under the capture's lock.
    let ring = unsafe { &*cvr_xr::capture_ring::cvr_xr_capture_ring() };
    let list = ring.lists[ring.allocator_index as usize % ring.lists.len()];
    // SAFETY: the capture's arguments and its list.
    unsafe { crate::loading_host::record(list.cast(), target, view, rect, pose, fov, window, window_view, uv, aspect) }
}
extern "C" fn hud_params(_: *mut c_void, out: *mut [f32; crate::xr_capture::HUD_PARAMS]) {
    // SAFETY: the capture's parameter block, the size of ColorBlit::HudParams.
    unsafe { crate::hud_composite::cvr_hud_composite_params(out.cast()) };
}
extern "C" fn eye1_fov(_: *mut c_void, out: *mut xr::Fovf) -> bool {
    // SAFETY: the capture's FOV.
    unsafe { cvr_xr::views::cvr_xr_eye_fov(1, out) }
}
extern "C" fn asym_windows(_: *mut c_void, out: *mut [EyeWindow; 2]) -> bool {
    // GetAsymmetricEyeWindows' default: fitted to the target
    // SAFETY: the capture's two windows.
    unsafe { crate::eye_windows::cvr_xr_asym_eye_windows(out.cast(), true) }
}

/// OpenXRManager::CaptureMonoPresentedFrame.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn capture_mono(
    _: *mut c_void,
    back_buffer: *mut c_void,
    desc: *const D3D12_RESOURCE_DESC,
    serial: u64,
    poses: *const xr::Posef,
    fovs: *const xr::Fovf,
    has_view: *const bool,
    head_centers: *const xr::Posef,
) -> bool {
    // SAFETY: the present's two views of everything.
    let (poses, fovs, has_view, head_centers) = unsafe {
        (
            core::slice::from_raw_parts(poses, 2),
            core::slice::from_raw_parts(fovs, 2),
            core::slice::from_raw_parts(has_view, 2),
            core::slice::from_raw_parts(head_centers, 2),
        )
    };
    if back_buffer.is_null() || !has_view[0] || !has_view[1] {
        crate::menu_lobby::trace_boot_skip("no view");
        // CVR_DIAGNOSTIC: only with the runtime diagnostics on
        // SAFETY: an exported switch.
        if unsafe { core::ptr::read_volatile(&raw const crate::declared_moved_exports::CyberpunkVR_RuntimeDiagnostics) } != 0 {
            crate::used_moved_exports::CyberpunkVR_DebugCapSkipNoView.fetch_add(1, Relaxed);
        }
        return false;
    }
    let _lock = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    // a runtime pose reset in progress: no picture is labelled with a pose it is about to invalidate (8255f379b)
    if cvr_xr::published::external_reset_pending() {
        crate::menu_lobby::trace_boot_skip("pose reset pending");
        return false;
    }
    let mut writes = [0usize; 2];
    let input = CaptureFrameHostIn {
        back_buffer,
        source_desc: desc,
        writes: writes.as_mut_ptr(),
        ctx: core::ptr::null_mut(),
        serial,
        poses: [poses[0], poses[1]],
        head_centers: [head_centers[0], head_centers[1]],
        fovs: [fovs[0], fovs[1]],
        has_view: [has_view[0], has_view[1]],
        ensure_resources,
        loading_end,
        loading_begin,
        loading_record,
        hud_params,
        eye1_fov,
        asym_windows,
    };
    // SAFETY: the capture's input, alive for the call.
    let captured = unsafe { crate::xr_capture::cvr_xr_capture_frame(&input) };
    // SAFETY: the plugin's verbose switch.
    if captured && unsafe { core::ptr::read_volatile(&raw const g_verboseLog) } != 0 && serial % 300 == 1 {
        cvr_host::log::write(&format!("OpenXRManager: Mono frame captured. serial={serial}\n"));
    }
    // the write leases' release (RustWriteRelease's destructor), before the lock's
    // SAFETY: the capture's own write leases.
    unsafe { crate::xr_capture_state::cvr_xr_capture_end_writes(writes.as_mut_ptr()) };
    captured
}
