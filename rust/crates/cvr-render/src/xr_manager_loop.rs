//! OpenXRManager's frame loop (formerly src/Runtimes/OpenXRFrameLoop.cpp's FrameThreadMain, PumpInlineFrame and
//! ReportXrFrameRates and src/Runtimes/OpenXRManager.cpp's SubmitThreadMain, SubmitThreadThunk and InitGraphics): the
//! cycle's host (the HUD quads, the frame generation overlay and the settings panel the manager holds, the PSVR2
//! heartbeat), the inline pump, the submit thread's host and the graphics binding. The loop itself is cvr-xr's and
//! xr_frame_loop.rs's; the manager's members are the C++ half's services.

use core::ffi::c_void;
use core::sync::atomic::Ordering::Relaxed;

use crate::xr_frame_loop::FrameHost;
use cvr_xr::submit_thread::SubmitHost;

/// FrameLoopOwner::Inline.
const INLINE: i32 = 1;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// volatile int g_verboseLog (src/Core/VrCore.cpp).
    #[link_name = "?g_verboseLog@@3HC"]
    static g_verboseLog: i32;
    // the C++ half's services
    /// The manager's five HUD quads (main, interaction, Basilisk, surveillance, hit marker), its frame generation
    /// overlay and its settings panel, as their Rust handles (src/Runtimes/OpenXRFrameLoop.cpp).
    fn cvr_cpp_xr_frame_objects(quads: *mut *mut c_void, framegen_overlay: *mut *mut c_void, settings_panel: *mut *mut c_void);
    /// cvr::psvr2::Haptics().Heartbeat().
    safe fn cvr_cpp_psvr2_heartbeat();
}

extern "C" fn heartbeat(_: *mut c_void) {
    // The bridge's heartbeat only while it owns the actuators (main's OpenXRFrameLoop).
    if cvr_settings::haptic_route::HAPTIC_ROUTE.toolkit() {
        cvr_cpp_psvr2_heartbeat();
    }
}
/// OpenXRManager::FrameThreadMain: one cycle with the manager's objects.
pub fn frame_thread_main() -> u32 {
    cvr_host::name_this_thread("CVR XR frame loop");
    let mut host = FrameHost {
        ctx: core::ptr::null_mut(),
        verbose: &raw const g_verboseLog,
        quads: [core::ptr::null_mut(); 5],
        framegen_overlay: core::ptr::null_mut(),
        settings_panel: core::ptr::null_mut(),
        heartbeat,
    };
    // SAFETY: the host's slots to fill.
    unsafe { cvr_cpp_xr_frame_objects(host.quads.as_mut_ptr(), &mut host.framegen_overlay, &mut host.settings_panel) };
    // SAFETY: a host of the manager's objects, alive for the cycle.
    unsafe { crate::xr_frame_loop::cvr_xr_frame_thread_main(&host) }
}
/// OpenXRManager::PumpInlineFrame: the submit thread's turn when it owns the loop, else one cycle here.
pub fn pump_inline_frame() {
    if cvr_cpp_xr_use_threaded_submit() {
        cvr_xr::submit_thread::cvr_xr_notify_submit_thread();
        return;
    }
    if !cvr_xr::submit_thread::cvr_xr_acquire_frame_loop(INLINE, 8) {
        cvr_xr::xr_rates::cvr_xr_mark(b's');
        return;
    }
    frame_thread_main();
    // FSR3: the real endpoint after an interpolated frame, or the extrapolated frame after the real one, gets its own
    // display cycle
    if crate::framegen_presenter::wants_fsr3_cycle() && cvr_xr::events::session_running() {
        frame_thread_main();
    }
    cvr_xr::submit_thread::cvr_xr_release_frame_loop(INLINE);
}
/// OpenXRManager::ReportXrFrameRates.
pub fn report_frame_rates() {
    cvr_xr::xr_rates::cvr_xr_report_frame_rates(
        cvr_xr::presented::PRESENTED.present_count.load(Relaxed),
        cvr_xr::submit_state::SUBMIT.predicted_display_period_ns.load(Relaxed),
    );
}

unsafe extern "C" fn use_threaded(_: *mut c_void) -> bool {
    cvr_cpp_xr_use_threaded_submit()
}
unsafe extern "C" fn frame(_: *mut c_void) {
    frame_thread_main();
}
unsafe extern "C" fn steam_vr(_: *mut c_void) -> bool {
    cvr_xr::runtime_info::RUNTIME.steamvr.load(Relaxed)
}
unsafe extern "C" fn mono(_: *mut c_void) -> bool {
    cvr_xr::submit_state::SUBMIT.mono_submit_enabled.load(Relaxed)
}
/// The submit thread's host (the context unused: every call reads the manager's blocks).
struct Host(SubmitHost);
// SAFETY: functions and a null context.
unsafe impl Sync for Host {}
static SUBMIT_HOST: Host = Host(SubmitHost { ctx: core::ptr::null_mut(), use_threaded, frame, steam_vr, mono });
/// OpenXRManager::SubmitThreadThunk / SubmitThreadMain: the submit thread's entry.
unsafe extern "system" fn submit_thread(_: *mut c_void) -> u32 {
    cvr_host::name_this_thread("CVR XR submission");
    // SAFETY: the host lives for the process.
    unsafe { cvr_xr::submit_thread::cvr_xr_submit_thread_main(&SUBMIT_HOST.0) }
}
/// OpenXRManager::InitGraphics: the session's D3D12 binding, the submit thread started with this module's entry.
pub fn init_graphics(device: *mut c_void, queue: *mut c_void) -> bool {
    // SAFETY: the game's live device and queue; the entry runs with no context.
    unsafe { cvr_xr::manager_init::cvr_xr_manager_init_graphics(device, queue, Some(submit_thread), core::ptr::null_mut()) }
}

/// OpenXRManager::UseThreadedSubmit (formerly LiveControlsPoll.cpp's service): with mono submit on, the submit
/// thread owns the loop under frame generation (display-rate submission) or when forced; in auto, everywhere but
/// Virtual Desktop.
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_use_threaded_submit() -> bool {
    if !cvr_xr::submit_state::SUBMIT.mono_submit_enabled.load(Relaxed) {
        return false;
    }
    if cvr_gpu::framegen_core::cvr_framegen_enabled() {
        return true;
    }
    // SAFETY: the export's plain read.
    let mode = unsafe { core::ptr::read_volatile(&raw const crate::used_moved_exports::CyberpunkVR_ThreadedMonoSubmit) };
    match mode {
        0 => false,
        m if m > 0 => true,
        _ => !cvr_xr::runtime_info::RUNTIME.virtual_desktop.load(Relaxed),
    }
}

/// OpenXRManager::SetMonoSubmitEnabled (the live controls' service): the switch, the submit thread woken, the mono
/// present event reset, the captured frame forgotten.
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_set_mono_submit_enabled(enabled: bool) {
    let submit = &cvr_xr::submit_state::SUBMIT;
    submit.mono_submit_enabled.store(enabled, Relaxed);
    cvr_xr::submit_thread::cvr_xr_notify_submit_thread();
    // SAFETY: the event handle the init writes once.
    let event = unsafe { *submit.mono_present_event.get() };
    if !event.is_null() {
        // SAFETY: the manager's live event.
        unsafe { windows_sys::Win32::System::Threading::ResetEvent(event) };
    }
    crate::xr_capture_state::cvr_xr_capture_forget();
}

unsafe extern "C" {
    /// cvr-rig's finger grip: which of the hand tracking (1) and the joints' motion range (2) extensions it wants.
    fn cvr_fingergrip_choose_extensions(has_hand_tracking: bool, has_motion_range: bool, has_data_source: bool) -> i32;
}
/// The Vive trackers' and the finger bones' optional extensions, chosen by their owners, for the Rust instance: their
/// names into `out` (static strings), at most `capacity`, the count back.
///
/// # Safety
/// `available` holds `count` properties; `out` has room for `capacity` names.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_optional_extensions(
    available: *const openxr_sys::ExtensionProperties,
    count: u32,
    out: *mut *const core::ffi::c_char,
    capacity: u32,
) -> u32 {
    let mut enabled: Vec<*const core::ffi::c_char> = Vec::new();
    // the trackers' (only with xr_vive_trackers=1)
    // SAFETY: the caller's properties.
    let tracker = unsafe { cvr_xr::trackers::cvr_trackers_choose_extension(available, count) };
    if !tracker.is_null() {
        enabled.push(tracker);
    }
    // the finger grip's (only with xr_grip_from_fingers=1)
    let props: &[openxr_sys::ExtensionProperties] =
        if available.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(available, count as usize) } };
    let named = |name: &core::ffi::CStr| {
        props.iter().any(|p| {
            // SAFETY: the runtime's terminated name.
            let have = unsafe { core::ffi::CStr::from_ptr(p.extension_name.as_ptr()) };
            have == name
        })
    };
    let hand_tracking = named(c"XR_EXT_hand_tracking");
    let motion_range = named(c"XR_EXT_hand_joints_motion_range");
    let data_source = named(c"XR_EXT_hand_tracking_data_source");
    // SAFETY: cvr-rig's.
    let mask = unsafe { cvr_fingergrip_choose_extensions(hand_tracking, motion_range, data_source) };
    if mask & 1 != 0 {
        enabled.push(c"XR_EXT_hand_tracking".as_ptr());
    }
    if mask & 2 != 0 {
        enabled.push(c"XR_EXT_hand_joints_motion_range".as_ptr());
    }
    if mask & 4 != 0 {
        enabled.push(c"XR_EXT_hand_tracking_data_source".as_ptr());
    }
    let n = enabled.len().min(capacity as usize);
    // SAFETY: the caller's room for `capacity` names.
    unsafe { core::ptr::copy_nonoverlapping(enabled.as_ptr(), out, n) };
    n as u32
}

unsafe extern "C" {
    /// cvr-rig's finger grip: its session handles let go.
    safe fn cvr_fingergrip_session_destroying();
    /// The plugin's folder `folder` (Core/Paths.hpp order: 4 config), narrow, with its trailing separator.
    fn cvr_paths_dir_a(folder: u32) -> *const core::ffi::c_char;
}
/// The session's teardown, before its handles go: the trackers' and the finger grip's.
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_session_destroying() {
    // SAFETY: cvr-xr's trackers, before the session's handles go.
    unsafe { cvr_xr::trackers::cvr_trackers_on_session_destroying() };
    cvr_fingergrip_session_destroying();
}
/// The calibration file's path (config\vrik_calibration.ini), cut short to `size` with its terminator.
///
/// # Safety
/// `out` has room for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_calibration_path(out: *mut core::ffi::c_char, size: usize) {
    if out.is_null() || size == 0 {
        return;
    }
    // SAFETY: a terminated folder kept for the process.
    let mut bytes = unsafe { core::ffi::CStr::from_ptr(cvr_paths_dir_a(4)) }.to_bytes().to_vec();
    bytes.extend_from_slice(b"vrik_calibration.ini");
    bytes.truncate(size - 1);
    // SAFETY: the caller's room.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), out, bytes.len());
        *out.add(bytes.len()) = 0;
    }
}
