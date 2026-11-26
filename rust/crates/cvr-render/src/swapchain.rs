//! THE GAME'S SWAPCHAIN (formerly src/Hooks/SwapChain.cpp): the plugin's bootstrap -- the DXGI factory exports hooked
//! (dxgi.dll and Streamline's interposer), the factory's vtable patched on the first factory the game makes, the
//! swapchain created at the launcher's size, the early init around it (OS hooks, DRED, live controls, OpenXR, the
//! launcher fit), the device and queue wired into the overlay, the mirror, the depth capture and the XR session, the
//! window fitted to the monitor -- and the swapchain's own hooks: Present (the frame's hub, every per-frame poll in its
//! order, the overlay, the XR frame, the desktop mirror, the DXGI Present, the timings), SetFullscreenState (never
//! fullscreen), ResizeBuffers/ResizeBuffers1 (the launcher's size, verbatim).
//!
//! The state other C++ files read (g_gameHwnd, the DRED device, the cursor clip, the ECL counters) and the timing exports
//! stay the C++ half's; the subsystems are called through the runtime it attaches.
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};

use cvr_crt::{format, Arg};
use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{
    IDXGIFactory2, IDXGISwapChain, IDXGISwapChain3, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_DESC1,
};

extern "C" {
    static CyberpunkVR_PresentTiming: [AtomicU64; 8];
    static CyberpunkVR_PresentSections: [AtomicU64; 8];
    static CyberpunkVR_NodeTimeline: i32;
    static CyberpunkVR_PreparationRequest: AtomicU32;
    static CyberpunkVR_EarlyDepthRequest: AtomicU32;
    static CyberpunkVR_NativeStereoProbeState: AtomicU32;
    static CyberpunkVR_NativeStereoProbeRequest: AtomicU32;
    static CyberpunkVR_GeometryPacketRequest: AtomicU32;
    static CyberpunkVR_PreparationCameraTrace: AtomicU32;
    static CyberpunkVR_SinglePassTraceState: AtomicU32;
    static CyberpunkVR_SinglePassTraceRequest: AtomicU32;
    static CyberpunkVR_PreparedPairRequest: AtomicU32;
    fn CyberpunkVR_ProfPublish();
    fn CyberpunkVR_NodeCutPoll();
    fn CyberpunkVR_StageReplayPoll();
    fn CyberpunkVR_WaitDeferredPresent();
    fn CyberpunkVR_RegisterRtvSwapchain(swapchain: *mut c_void);
    fn RegisterGameFacingListVtable(device: *mut c_void);
    fn PrepareStartupLiveControls();
    fn InitOpenXREarly();
    fn FitLauncherResolutionToRuntime();
    fn CyberpunkVRPort_EnableDredOnce();
    fn GetForcedSwapchainWidth() -> u32;
    fn GetForcedSwapchainHeight() -> u32;
    fn GetForcedDisplayModeWidth() -> u32;
    fn GetForcedDisplayModeHeight() -> u32;
    fn GetForcedWindowWidth() -> u32;
    fn GetForcedWindowHeight() -> u32;
}
#[allow(non_snake_case)]
extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
}
const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
const DXGI_ERROR_INVALID_CALL: HRESULT = HRESULT(0x887A_0001_u32 as i32);
const DXGI_ERROR_NOT_CURRENTLY_AVAILABLE: HRESULT = HRESULT(0x887A_0022_u32 as i32);

/// What the C++ half provides: its state and the subsystems the hub calls, in the hub's order.
#[repr(C)]
pub struct Runtime {
    game_hwnd: *mut *mut c_void,
    cursor_clipped: *const bool,
    present_queue: *const AtomicPtr<c_void>,
    ecl_total_calls: *const AtomicU64,
    ecl_total_lists: *const AtomicU64,
    ecl_present_calls: *const AtomicU64,
    ecl_present_lists: *const AtomicU64,
    ecl_distinct_queues: *const AtomicU32,
    /// cvr::framegen::FinishGpuFrame under the image-submission lock.
    finish_gpu_frame: extern "C" fn(),
    gpu_profile_poll: extern "C" fn(),
    scene_profile_poll: extern "C" fn(),
    pair_probe_poll: extern "C" fn(),
    cascades_poll: extern "C" fn(),
    xr_on_present: extern "C" fn(*mut c_void),
    xr_pump_inline_frame: extern "C" fn(),
    xr_report_frame_rates: extern "C" fn(),
    desktop_mirror_render: extern "C" fn(*mut c_void, *mut c_void),
    desktop_mirror_invalidate: extern "C" fn(),
    xr_init_graphics: extern "C" fn(*mut c_void, *mut c_void),
    desktop_mirror_set_device_and_queue: extern "C" fn(*mut c_void, *mut c_void),
    desktop_monitor_window_rect: extern "C" fn(*mut c_void, *mut RECT) -> bool,
}
// The subsystems the runtime lent through one-line C++ forwards, called directly (each a plain call into its
// Rust export, with the forward's own preconditions).
fn retained_geometry_active() -> bool {
    unsafe { cvr_nrg2_active() }
}
fn trace_frame_boundary() {
    unsafe { cvr_sp_trace_frame_boundary() }
}
fn early_depth_before_frame_boundary() {
    unsafe { cvr_ned_before_frame_boundary() }
}
fn early_depth_frame_boundary() {
    unsafe { cvr_ned_frame_boundary() }
}
fn early_depth_run_tracking() -> bool {
    unsafe { cvr_ned_run_tracking() }
}
fn packets_frame_boundary(tracking: bool) {
    unsafe { cvr_gp_frame_boundary(tracking) }
}
fn prepared_pair_needs_native_bindings() -> bool {
    unsafe { cvr_npp2_needs_native_bindings() }
}
fn native_probe_frame_boundary(bindings: bool) {
    unsafe { cvr_nsps_frame_boundary(bindings) }
}
fn gpu_probe_poll() {
    unsafe { cvr_sgpw_poll() }
}
fn early_depth_poll() {
    unsafe { cvr_ned_poll() }
}
fn overlay_set_window(hwnd: *mut c_void) {
    unsafe { cvr_overlay_set_window(hwnd) }
}
fn overlay_render(swapchain: *mut c_void) {
    unsafe { cvr_overlay_render(swapchain) }
}
fn overlay_render_desktop(swapchain: *mut c_void) {
    unsafe { cvr_overlay_render_desktop(swapchain) }
}
fn prof_timeline_add(start: i64, end: i64, rva: u32, kind: u32, extra: u32) {
    unsafe { cvr_stereo_prof_timeline_add(start, end, rva, kind, extra) }
}
fn overlay_invalidate_swapchain_resources() {
    unsafe { cvr_overlay_invalidate_swapchain_resources() }
}
fn overlay_set_device_and_queue(device: *mut c_void, queue: *mut c_void) {
    unsafe { cvr_overlay_set_device_and_queue(device, queue) }
}
fn install_command_queue_diag_hook(queue: *mut c_void) {
    unsafe { cvr_depth_install_queue_hook(queue) }
}
fn install_depth_capture_hooks(device: *mut c_void) {
    unsafe { cvr_dh_install_descriptor_hooks(device) }
}
fn install_stage_pre_device() {
    // Stage::PreDevice.
    unsafe { cvr_hooks_install_stage(0) };
}
fn render_doc_resolve_hook_target(module: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { cvr_renderdoc_resolve_hook_target(module, name) }
}
fn render_doc_chain_behind_hook(module: *const c_char, name: *const c_char, trampoline: *mut c_void) {
    unsafe { cvr_renderdoc_chain_behind_hook(module, name, trampoline) };
}
// The hub's subsystems that are Rust's already, called directly (formerly the runtime's callbacks to their C++ facades);
// the cvr-rig ones by symbol (the plugin links them).
extern "C" {
    fn cvr_nrg2_active() -> bool;
    fn cvr_sp_trace_frame_boundary();
    fn cvr_ned_before_frame_boundary();
    fn cvr_ned_frame_boundary();
    fn cvr_ned_run_tracking() -> bool;
    fn cvr_gp_frame_boundary(live_inputs: bool);
    fn cvr_npp2_needs_native_bindings() -> bool;
    fn cvr_nsps_frame_boundary(live: bool);
    fn cvr_sgpw_poll();
    fn cvr_ned_poll();
    fn cvr_overlay_set_window(hwnd: *mut c_void);
    fn cvr_overlay_render(swap_chain: *mut c_void);
    fn cvr_overlay_render_desktop(swap_chain: *mut c_void);
    fn cvr_stereo_prof_timeline_add(start: i64, end: i64, rva: u32, kind: u32, extra: u32);
    fn cvr_overlay_invalidate_swapchain_resources();
    fn cvr_overlay_set_device_and_queue(device: *mut c_void, queue: *mut c_void);
    fn cvr_depth_install_queue_hook(queue: *mut c_void);
    fn cvr_dh_install_descriptor_hooks(device: *mut c_void);
    fn cvr_hooks_install_stage(stage: i32) -> i32;
    fn cvr_renderdoc_resolve_hook_target(module: *mut c_void, function: *const c_char) -> *mut c_void;
    fn cvr_renderdoc_chain_behind_hook(module: *const c_char, function: *const c_char, trampoline: *mut c_void) -> bool;
    fn cvr_os_input_update_cursor_capture(hwnd: *mut c_void);
    fn cvr_os_input_install();
    fn cvr_sound_tick();
}
// SAFETY: functions and the plugin's globals, alive for the process.
unsafe impl Sync for Runtime {}
fn rt() -> &'static Runtime {
    &table::RT
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn ptr<T>(p: *const T) -> String {
    format!("{:016X}", p as usize)
}
fn ticks() -> u64 {
    let mut t = 0i64;
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t as u64
}
fn timing(i: usize) -> &'static AtomicU64 {
    // SAFETY: an export of the C++ half.
    unsafe { &CyberpunkVR_PresentTiming[i] }
}
fn sections(i: usize) -> &'static AtomicU64 {
    // SAFETY: an export of the C++ half.
    unsafe { &CyberpunkVR_PresentSections[i] }
}
fn original(this: *mut c_void, slot: usize) -> *mut c_void {
    // SAFETY: a COM object: its first field is its vtable.
    let vt = unsafe { *(this as *const *mut *mut c_void) };
    cvr_engine::vtable::original(vt, slot)
}
unsafe fn patch(vt: *mut *mut c_void, slot: usize, hook: *mut c_void) -> bool {
    // SAFETY: the caller's live vtable.
    unsafe { cvr_engine::vtable::patch(vt, slot, hook) }
}
fn desc_of(swapchain: *mut c_void) -> Option<DXGI_SWAP_CHAIN_DESC> {
    // SAFETY: the game's swapchain.
    let sc = unsafe { IDXGISwapChain::from_raw_borrowed(&swapchain) }?;
    // SAFETY: as above.
    unsafe { sc.GetDesc() }.ok()
}

/// True for a window of ours (the mirror, dummies): everything the port creates registers a "CyberpunkVR*" class, and
/// its swapchains stay outside every path built around the game's single swapchain.
fn is_our_own_window(hwnd: *mut c_void) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let mut cls = [0u16; 64];
    // SAFETY: a class-name query into our buffer.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, cls.as_mut_ptr(), 63)
    };
    cls[..11].iter().copied().eq("CyberpunkVR".encode_utf16())
}

// ---- the swapchain's hooks ----------------------------------------------------------------------------------------

type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
type SetFullscreenStateFn = unsafe extern "system" fn(*mut c_void, i32, *mut c_void) -> HRESULT;
type ResizeBuffersFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) -> HRESULT;
type ResizeBuffers1Fn = unsafe extern "system" fn(
    *mut c_void,
    u32,
    u32,
    u32,
    i32,
    u32,
    *const u32,
    *const *mut c_void,
) -> HRESULT;

unsafe extern "system" fn hooked_present(
    swapchain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let hook_start = ticks();
    let r = rt();
    // SAFETY: the swapchain the hook was given.
    unsafe { cvr_gpu::gpu_diag::cvr_dred_remember_device_from_swapchain(swapchain) };
    let desc = desc_of(swapchain);
    let hwnd = desc.map_or(core::ptr::null_mut(), |d| d.OutputWindow.0);

    // OUR OWN swapchains present through this same hook (the Present vtable is shared by every swapchain in the
    // process) -- straight to DXGI.
    if desc.is_some() && is_our_own_window(hwnd) {
        let orig = original(swapchain, 8);
        if orig.is_null() {
            return DXGI_ERROR_INVALID_CALL;
        }
        // SAFETY: DXGI's own Present.
        return unsafe {
            core::mem::transmute::<*mut c_void, PresentFn>(orig)(swapchain, sync_interval, flags)
        };
    }
    cvr_gpu::queue_timeline::cvr_queue_timeline_present();
    {
        static LAST_PRESENT: AtomicU64 = AtomicU64::new(0);
        if timing(6).load(Relaxed) == 0 {
            let mut f = 0i64;
            // SAFETY: a plain system query.
            unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
            timing(6).store(f as u64, SeqCst);
        }
        let last = LAST_PRESENT.load(Relaxed);
        if last != 0 {
            timing(1).fetch_add(hook_start.wrapping_sub(last), Relaxed);
        }
        LAST_PRESENT.store(hook_start, Relaxed);
        timing(0).fetch_add(1, Relaxed);
    }
    let finish_start = ticks();
    (r.finish_gpu_frame)();
    let mut section_start = ticks();
    timing(2).fetch_add(section_start.wrapping_sub(finish_start), Relaxed);
    let mut section = |index: usize| {
        let now = ticks();
        sections(index).fetch_add(now.wrapping_sub(section_start), Relaxed);
        section_start = now;
    };
    cvr_gpu::command_resources::cvr_gpu_collect_command_resources();
    (r.gpu_profile_poll)();
    (r.scene_profile_poll)();
    (r.pair_probe_poll)();
    (r.cascades_poll)();
    crate::neural_render::on_present();
    crate::resource_census::poll();
    // SAFETY: exports of the C++ half.
    unsafe {
        if CyberpunkVR_PreparationRequest.load(SeqCst) != 0
            && (CyberpunkVR_EarlyDepthRequest.load(SeqCst) != 0 || retained_geometry_active())
            && CyberpunkVR_NativeStereoProbeState.load(SeqCst) != 1
        {
            CyberpunkVR_NativeStereoProbeRequest.store(1, SeqCst);
            CyberpunkVR_GeometryPacketRequest.store(1, SeqCst);
        }
        if CyberpunkVR_PreparationCameraTrace.load(SeqCst) != 0
            && CyberpunkVR_PreparationRequest.load(SeqCst) != 0
            && CyberpunkVR_SinglePassTraceState.load(SeqCst) != 1
            && CyberpunkVR_SinglePassTraceRequest.load(SeqCst) == 0
        {
            CyberpunkVR_SinglePassTraceRequest
                .store(CyberpunkVR_PreparationRequest.load(SeqCst), SeqCst);
        }
    }
    trace_frame_boundary();
    early_depth_before_frame_boundary();
    crate::preparation_trace::cvr_prep_frame_boundary();
    early_depth_frame_boundary();
    crate::collector_probe::cvr_ccp_poll();
    packets_frame_boundary(early_depth_run_tracking());
    let native_bindings = early_depth_run_tracking()
        || prepared_pair_needs_native_bindings()
        // SAFETY: an export of the C++ half.
        || unsafe { CyberpunkVR_PreparedPairRequest.load(Relaxed) } != 0;
    native_probe_frame_boundary(native_bindings);
    gpu_probe_poll();
    early_depth_poll();

    // Bind the overlay to the game's window on first sight: the first Present on a window that is not ours IS the
    // game's.
    // SAFETY: the C++ half's g_gameHwnd, written on this thread.
    unsafe {
        if desc.is_some() && !hwnd.is_null() && *r.game_hwnd != hwnd {
            *r.game_hwnd = hwnd;
            overlay_set_window(hwnd);
            log(&format!(
                "Present: bound overlay to game window {}\n",
                ptr(hwnd)
            ));
        }
    }
    if desc.is_some() {
        // SAFETY: the game's window.
        unsafe { cvr_os_input_update_cursor_capture(hwnd) };
    }
    // DLSS is loaded lazily after the renderer first evaluates a frame: the NGX hook is tried on every Present until it
    // goes in.
    static NGX_HOOK_TRIED: AtomicBool = AtomicBool::new(false);
    if !NGX_HOOK_TRIED.load(Acquire) && crate::ngx::cvr_ngx_install() {
        NGX_HOOK_TRIED.store(true, Release);
    }
    // The stereo profiler's frame ends here (unconditional: it normalises what it accumulated), then the node-cut rule
    // and the stage replay's census.
    // SAFETY: the C++ half's functions.
    unsafe {
        CyberpunkVR_ProfPublish();
        CyberpunkVR_NodeCutPoll();
        CyberpunkVR_StageReplayPoll();
    }
    // The presented size for the scene-depth picker, the choice reopened for the next frame.
    let (w, h, known) = match desc_of(swapchain) {
        Some(d) => (d.BufferDesc.Width, d.BufferDesc.Height, true),
        None => (0, 0, false),
    };
    cvr_gpu::depth_capture::cvr_depth_present(w, h, known);

    section(0);
    crate::engine_stereo::cvr_engine_stereo_note_present();
    crate::menu_lobby::cvr_menu_lobby_tick();
    // SAFETY: a plain tick.
    unsafe { cvr_sound_tick() };
    overlay_render(swapchain);
    section(1);
    crate::xr_capture::DESKTOP_EYE_HANDLED.store(false, Relaxed);
    (r.xr_on_present)(swapchain);
    section(2);
    // One XR frame inline on the Present thread.
    let pump_start = ticks();
    (r.xr_pump_inline_frame)();
    timing(3).fetch_add(ticks().wrapping_sub(pump_start), Relaxed);
    section_start = ticks();
    let mut section = |index: usize| {
        let now = ticks();
        sections(index).fetch_add(now.wrapping_sub(section_start), Relaxed);
        section_start = now;
    };
    // After the pump, so an inline cycle is complete when its numbers are read.
    (r.xr_report_frame_rates)();
    // The monitor's eye: cut from the side-by-side frame when the capture did not record it (the capture skipped the
    // frame), never under a shared menu frame unless the loading screen is drawn into each half.
    if let Some(eye) = crate::xr_capture::monitor_eye() {
        if !crate::xr_capture::DESKTOP_EYE_HANDLED.load(Relaxed)
            && crate::engine_stereo::enabled()
            && (!crate::xr_capture::shared_menu_frame() || crate::xr_capture::loading_side_by_side())
        {
            // SAFETY: the game's live swapchain.
            if let Some(chain) = unsafe { windows::Win32::Graphics::Dxgi::IDXGISwapChain::from_raw_borrowed(&swapchain) } {
                cvr_gpu::desktop_mirror::packed_eye(chain, eye);
            }
        }
    }
    overlay_render_desktop(swapchain);
    section(3);
    // The OpenXR capture has copied the whole frame on this queue; only now is the desktop's copy replaced by its
    // window-aspect centre crop.
    if desc.is_some() && !hwnd.is_null() {
        (r.desktop_mirror_render)(swapchain, hwnd);
    }
    section(4);
    let orig = original(swapchain, 8);
    let dxgi_start = ticks();
    let hr = if orig.is_null() {
        DXGI_ERROR_INVALID_CALL
    } else {
        // SAFETY: DXGI's own Present.
        unsafe {
            core::mem::transmute::<*mut c_void, PresentFn>(orig)(swapchain, sync_interval, flags)
        }
    };
    section_start = ticks();
    timing(4).fetch_add(section_start.wrapping_sub(dxgi_start), Relaxed);
    if hr.is_err() {
        log(&format!(
            "[DRED] Present failure observed. hr=0x{:08X} removed={} swapChain={}\n",
            hr.0 as u32,
            cvr_gpu::gpu_diag::cvr_dred_is_device_removed_hr(hr.0) as i32,
            ptr(swapchain)
        ));
    }
    if hr.is_err() && cvr_gpu::gpu_diag::cvr_dred_is_device_removed_hr(hr.0) {
        // SAFETY: the swapchain the hook was given.
        unsafe { cvr_gpu::gpu_diag::cvr_dred_dump_once(swapchain, hr.0) };
    }
    static PRESENT_LOG_COUNTER: AtomicU64 = AtomicU64::new(0);
    // a failed Present always, the first one, and every 600th only with the verbose diagnostics
    let n = PRESENT_LOG_COUNTER.fetch_add(1, Relaxed).wrapping_add(1);
    if hr.0 != 0 || n == 1 || (cvr_host::log::verbose() && n % 600 == 1) {
        present_report(hr, desc, hwnd);
    }
    let now = ticks();
    sections(5).fetch_add(now.wrapping_sub(section_start), Relaxed);
    // This frame is already rendered: publish and notify the XR consumer before throttling the producer.
    let pace_start = ticks();
    cvr_gpu::framegen_core::cvr_framegen_pace_rendered_frame();
    timing(7).fetch_add(ticks().wrapping_sub(pace_start), Relaxed);
    timing(5).fetch_add(ticks().wrapping_sub(hook_start), Relaxed);
    // SAFETY: an export of the C++ half.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_NodeTimeline) } != 0 {
        prof_timeline_add(hook_start as i64, ticks() as i64, 0, 3, 0);
    }
    hr
}

/// Every 600th Present (and every one that did not succeed): its result, the window, the cursor clip and the
/// command-queue share.
fn present_report(hr: HRESULT, desc: Option<DXGI_SWAP_CHAIN_DESC>, hwnd: *mut c_void) {
    let r = rt();
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetForegroundWindow, GetWindowRect,
    };
    // SAFETY: window queries.
    let foreground = unsafe { GetForegroundWindow() };
    let mut client = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut window = client;
    if desc.is_some() && !hwnd.is_null() {
        // SAFETY: as above.
        unsafe {
            GetClientRect(hwnd, &mut client);
            GetWindowRect(hwnd, &mut window);
        }
    }
    // SAFETY: the C++ half's state.
    let clipped = unsafe { *r.cursor_clipped };
    log(&format!(
        "Present result: hr=0x{:08X} hwnd={} fg={} windowed={} desc={}x{} client={}x{} window=({},{})-({},{}) cursorClipped={}\n",
        hr.0 as u32,
        ptr(if desc.is_some() { hwnd } else { core::ptr::null_mut() }),
        ptr(foreground),
        desc.map_or(-1, |d| (d.Windowed.0 != 0) as i32),
        desc.map_or(0, |d| d.BufferDesc.Width),
        desc.map_or(0, |d| d.BufferDesc.Height),
        client.right - client.left,
        client.bottom - client.top,
        window.left,
        window.top,
        window.right,
        window.bottom,
        clipped as i32
    ));
    // SAFETY: the C++ half's counters.
    unsafe {
        let total_lists = (*r.ecl_total_lists).load(Relaxed);
        let present_lists = (*r.ecl_present_lists).load(Relaxed);
        let share = if total_lists != 0 {
            100.0 * present_lists as f64 / total_lists as f64
        } else {
            0.0
        };
        log(&format(
            c"[ECL-DIAG] presentQueue=%p lists present=%llu/%llu calls=%llu/%llu share=%.1f%% distinctQueues=%u\n",
            &[
                Arg::U((*r.present_queue).load(Relaxed) as u64),
                Arg::U(present_lists),
                Arg::U(total_lists),
                Arg::U((*r.ecl_present_calls).load(Relaxed)),
                Arg::U((*r.ecl_total_calls).load(Relaxed)),
                Arg::F(share),
                Arg::U(u64::from((*r.ecl_distinct_queues).load(Relaxed))),
            ],
        )
        .to_string_lossy());
    }
}

unsafe extern "system" fn hooked_set_fullscreen_state(
    swapchain: *mut c_void,
    fullscreen: i32,
    target: *mut c_void,
) -> HRESULT {
    if !target.is_null() {
        // SAFETY: a live output.
        unsafe { cvr_gpu::display_modes::cvr_display_modes_install_output(target) };
    }
    let orig = original(swapchain, 10);
    log(&format!(
        "SetFullscreenState intercepted: fullscreen={} target={}.\n",
        (fullscreen != 0) as i32,
        ptr(target)
    ));
    if fullscreen != 0 {
        return DXGI_ERROR_NOT_CURRENTLY_AVAILABLE;
    }
    if orig.is_null() {
        return DXGI_ERROR_INVALID_CALL;
    }
    // SAFETY: DXGI's own SetFullscreenState.
    unsafe { core::mem::transmute::<*mut c_void, SetFullscreenStateFn>(orig)(swapchain, 0, target) }
}

/// The launcher's size, verbatim: nothing is recomputed here (the old aspect re-derivation cost 3.4 degrees of
/// vertical field on a Quest 3, see the C++ reference).
fn forced_size(name: &str, width: u32, height: u32) -> (u32, u32) {
    // SAFETY: a deferred present finishes before the buffers change; the forced sizes are the C++ half's getters.
    let (fw, fh) = unsafe {
        CyberpunkVR_WaitDeferredPresent();
        (GetForcedSwapchainWidth(), GetForcedSwapchainHeight())
    };
    let w = if fw != 0 { fw } else { width };
    let h = if fh != 0 { fh } else { height };
    if w != width || h != height {
        log(&format!("{name} override: {width}x{height} -> {w}x{h}\n"));
    }
    (w, h)
}
unsafe extern "system" fn hooked_resize_buffers(
    swapchain: *mut c_void,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
) -> HRESULT {
    let (w, h) = forced_size("ResizeBuffers", width, height);
    let orig = original(swapchain, 13);
    let r = rt();
    overlay_invalidate_swapchain_resources();
    (r.desktop_mirror_invalidate)();
    let result = if orig.is_null() {
        DXGI_ERROR_INVALID_CALL
    } else {
        // SAFETY: DXGI's own ResizeBuffers.
        unsafe {
            core::mem::transmute::<*mut c_void, ResizeBuffersFn>(orig)(
                swapchain, count, w, h, format, flags,
            )
        }
    };
    if result.is_ok() {
        // SAFETY: the C++ half's function.
        unsafe { CyberpunkVR_RegisterRtvSwapchain(swapchain) };
    }
    result
}
#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn hooked_resize_buffers1(
    swapchain: *mut c_void,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
    node_mask: *const u32,
    queues: *const *mut c_void,
) -> HRESULT {
    let (w, h) = forced_size("ResizeBuffers1", width, height);
    let orig = original(swapchain, 39);
    let r = rt();
    overlay_invalidate_swapchain_resources();
    (r.desktop_mirror_invalidate)();
    let result = if orig.is_null() {
        DXGI_ERROR_INVALID_CALL
    } else {
        // SAFETY: DXGI's own ResizeBuffers1.
        unsafe {
            core::mem::transmute::<*mut c_void, ResizeBuffers1Fn>(orig)(
                swapchain, count, w, h, format, flags, node_mask, queues,
            )
        }
    };
    if result.is_ok() {
        // SAFETY: the C++ half's function.
        unsafe { CyberpunkVR_RegisterRtvSwapchain(swapchain) };
    }
    result
}

/// InstallSwapchainHooks: Present (8), SetFullscreenState (10), ResizeBuffers (13) and IDXGISwapChain3's ResizeBuffers1
/// (39).
#[no_mangle]
pub unsafe extern "C" fn cvr_swapchain_install_hooks(swapchain: *mut c_void) {
    // SAFETY: the C++ half's function; the game's swapchain and its vtables.
    unsafe {
        CyberpunkVR_RegisterRtvSwapchain(swapchain);
        if swapchain.is_null() {
            return;
        }
        let vt = *(swapchain as *const *mut *mut c_void);
        if vt.is_null() {
            return;
        }
        patch(vt, 8, hooked_present as *mut c_void);
        patch(vt, 10, hooked_set_fullscreen_state as *mut c_void);
        patch(vt, 13, hooked_resize_buffers as *mut c_void);
        if let Some(sc) = IDXGISwapChain::from_raw_borrowed(&swapchain) {
            if let Ok(sc3) = sc.cast::<IDXGISwapChain3>() {
                let vt3 = *(sc3.as_raw() as *const *mut *mut c_void);
                patch(vt3, 39, hooked_resize_buffers1 as *mut c_void);
            }
        }
    }
}

// ---- the factory ---------------------------------------------------------------------------------------------------

type FacCreateSwapChainFn = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    *mut DXGI_SWAP_CHAIN_DESC,
    *mut *mut c_void,
) -> HRESULT;
type FacCreateSwapChainForHwndFn = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *const DXGI_SWAP_CHAIN_DESC1,
    *const c_void,
    *mut c_void,
    *mut *mut c_void,
) -> HRESULT;
static ORIG_FAC_CSC: AtomicUsize = AtomicUsize::new(0);
static ORIG_FAC_CSCFH: AtomicUsize = AtomicUsize::new(0);

/// Everything before the game's swapchain is created: the one-time early init, then the device and queue wired in
/// from the command queue the game is creating it with.
unsafe fn plugin_pre_swapchain(device: *mut c_void, hwnd: *mut c_void) {
    let r = rt();
    static EARLY: AtomicBool = AtomicBool::new(false);
    if EARLY.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
        // SAFETY: the C++ half's state and functions, in the boot order.
        unsafe {
            if !hwnd.is_null() {
                *r.game_hwnd = hwnd;
            }
            cvr_os_input_install();
            CyberpunkVRPort_EnableDredOnce();
            PrepareStartupLiveControls();
            InitOpenXREarly(); // creates the XR instance/session; InitGraphics needs it
            FitLauncherResolutionToRuntime(); // before the swapchain is sized from the pick
        }
    }
    // SAFETY: the game's device argument (its command queue).
    let Some(unknown) = (unsafe { windows::core::IUnknown::from_raw_borrowed(&device) }) else {
        return;
    };
    let Ok(queue) = unknown.cast::<ID3D12CommandQueue>() else {
        return;
    };
    let mut d3d: Option<ID3D12Device> = None;
    // SAFETY: the game's queue.
    if unsafe { queue.GetDevice(&mut d3d) }.is_ok() {
        if let Some(d3d) = d3d {
            static WIRED: AtomicBool = AtomicBool::new(false);
            if WIRED.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
                let (d, q) = (d3d.as_raw(), queue.as_raw());
                log(&format!(
                    "PluginBootstrap: device={} queue={}\n",
                    ptr(d),
                    ptr(q)
                ));
                // SAFETY: the live device.
                unsafe { cvr_gpu::gpu_diag::cvr_dred_remember_device(d) };
                (r.xr_init_graphics)(d, q);
                overlay_set_device_and_queue(d, q);
                (r.desktop_mirror_set_device_and_queue)(d, q);
                install_command_queue_diag_hook(q);
                install_depth_capture_hooks(d);
                // The device came from the queue the GAME passed: the game-facing one (wrapped under a capture layer).
                // SAFETY: the C++ half's function.
                unsafe { RegisterGameFacingListVtable(d) };
            }
        }
    }
}

/// ...and everything after, on the swapchain that came back: the window fitted to the monitor (the backbuffer stays at
/// the forced size), the overlay bound, the swapchain's hooks.
unsafe fn plugin_post_swapchain(swapchain: *mut c_void, hwnd: *mut c_void) {
    if swapchain.is_null() || is_our_own_window(hwnd) {
        return;
    }
    let r = rt();
    if !hwnd.is_null() {
        // SAFETY: the C++ half's getters.
        let (mut win_w, mut win_h) = unsafe { (GetForcedWindowWidth(), GetForcedWindowHeight()) };
        let (mut win_x, mut win_y) = (0, 0);
        let mut place = false;
        // SAFETY: as above.
        if win_w == unsafe { GetForcedDisplayModeWidth() } {
            win_w = 0; // not a real window override
        }
        if win_w == 0 {
            let mut rect = RECT::default();
            if (r.desktop_monitor_window_rect)(hwnd, &mut rect) {
                win_w = (rect.right - rect.left) as u32;
                win_h = (rect.bottom - rect.top) as u32;
                win_x = rect.left;
                win_y = rect.top;
                place = true;
            }
        }
        if win_w != 0 && win_h != 0 {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, SWP_NOMOVE, SWP_NOZORDER,
            };
            // SAFETY: the game's window.
            unsafe {
                SetWindowPos(
                    hwnd,
                    core::ptr::null_mut(),
                    win_x,
                    win_y,
                    win_w as i32,
                    win_h as i32,
                    SWP_NOZORDER | if place { 0 } else { SWP_NOMOVE },
                )
            };
            log(&format!(
                "PluginBootstrap: desktop window set to {win_w}x{win_h}\n"
            ));
        }
    }
    if !hwnd.is_null() {
        // SAFETY: the C++ half's g_gameHwnd.
        unsafe { *r.game_hwnd = hwnd };
        overlay_set_window(hwnd);
    }
    // SAFETY: the game's swapchain.
    unsafe { cvr_swapchain_install_hooks(swapchain) };
    // SAFETY: a one-time install.
    unsafe { cvr_os_input_install() };
    log(&format!(
        "PluginBootstrap: swapchain hooked, overlay bound to {}\n",
        ptr(hwnd)
    ));
}

unsafe extern "system" fn detour_fac_create_swapchain(
    this: *mut c_void,
    device: *mut c_void,
    desc: *mut DXGI_SWAP_CHAIN_DESC,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the game's arguments; the factory's original.
    unsafe {
        plugin_pre_swapchain(
            device,
            desc.as_ref()
                .map_or(core::ptr::null_mut(), |d| d.OutputWindow.0),
        );
        let mut local: Option<DXGI_SWAP_CHAIN_DESC> = desc.as_ref().copied();
        let mut use_desc = desc;
        let (fw, fh) = (GetForcedDisplayModeWidth(), GetForcedDisplayModeHeight());
        if let (Some(d), Some(local)) = (desc.as_ref(), local.as_mut()) {
            let mut changed = false;
            if fw != 0 && fh != 0 {
                local.BufferDesc.Width = fw;
                local.BufferDesc.Height = fh;
                changed = true;
                log(&format!(
                    "CreateSwapChain override: {}x{} -> {}x{}\n",
                    d.BufferDesc.Width, d.BufferDesc.Height, fw, fh
                ));
            }
            if !local.Windowed.as_bool() {
                local.Windowed = true.into();
                changed = true;
            }
            if changed {
                use_desc = local;
            }
        }
        let original: FacCreateSwapChainFn = core::mem::transmute(ORIG_FAC_CSC.load(SeqCst));
        let hr = original(this, device, use_desc, out);
        if hr.is_ok() && !out.is_null() && !(*out).is_null() {
            plugin_post_swapchain(
                *out,
                use_desc
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |d| d.OutputWindow.0),
            );
        }
        hr
    }
}

unsafe extern "system" fn detour_fac_create_swapchain_for_hwnd(
    this: *mut c_void,
    device: *mut c_void,
    hwnd: *mut c_void,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen: *const c_void,
    restrict: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the game's arguments; the factory's original.
    unsafe {
        let original: FacCreateSwapChainForHwndFn =
            core::mem::transmute(ORIG_FAC_CSCFH.load(SeqCst));
        // Our own mirror/dummy windows go straight through.
        if is_our_own_window(hwnd) {
            return original(this, device, hwnd, desc, fullscreen, restrict, out);
        }
        plugin_pre_swapchain(device, hwnd);
        let mut local: Option<DXGI_SWAP_CHAIN_DESC1> = desc.as_ref().copied();
        let mut use_desc = desc;
        let (fw, fh) = (GetForcedSwapchainWidth(), GetForcedSwapchainHeight());
        if let (Some(d), Some(local), true) = (desc.as_ref(), local.as_mut(), fw != 0 && fh != 0) {
            local.Width = fw;
            local.Height = fh;
            use_desc = local;
            log(&format!(
                "CreateSwapChainForHwnd override: {}x{} -> {}x{}\n",
                d.Width, d.Height, fw, fh
            ));
        }
        let hr = original(this, device, hwnd, use_desc, fullscreen, restrict, out);
        if hr.is_ok() && !out.is_null() && !(*out).is_null() {
            plugin_post_swapchain(*out, hwnd);
        }
        hr
    }
}

/// The vtable of a factory the GAME created (a factory we ask for ourselves is refused while Streamline's interposer
/// sits on DXGI): CreateSwapChain (10) and CreateSwapChainForHwnd (15). The table is shared, so one patch covers every
/// factory.
unsafe fn hook_factory_vtable(factory: *mut c_void) -> bool {
    static PATCHED: AtomicBool = AtomicBool::new(false);
    // SAFETY: the factory the game was handed.
    let Some(unknown) = (unsafe { windows::core::IUnknown::from_raw_borrowed(&factory) }) else {
        return false;
    };
    let Ok(f2) = unknown.cast::<IDXGIFactory2>() else {
        return false;
    };
    if PATCHED
        .compare_exchange(false, true, SeqCst, SeqCst)
        .is_err()
    {
        return true;
    }
    // SAFETY: its live vtable.
    unsafe {
        let vt = *(f2.as_raw() as *const *mut *mut c_void);
        ORIG_FAC_CSC.store(*vt.add(10) as usize, SeqCst);
        ORIG_FAC_CSCFH.store(*vt.add(15) as usize, SeqCst);
        let a = patch(vt, 10, detour_fac_create_swapchain as *mut c_void);
        let b = patch(vt, 15, detour_fac_create_swapchain_for_hwnd as *mut c_void);
        log(&format!(
            "PluginBootstrap: factory vtable hooked csc={} cscfh={} fac={}\n",
            a as i32,
            b as i32,
            ptr(factory)
        ));
        a || b
    }
}

type CreateFactoryFn = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;
type CreateFactory2Fn = unsafe extern "system" fn(u32, *const GUID, *mut *mut c_void) -> HRESULT;
static ORIG_CREATE_FACTORY: [AtomicUsize; 3] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];
unsafe extern "system" fn hook_create_dxgi_factory(
    riid: *const GUID,
    pp: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the trampoline MinHook made; the factory it returns.
    unsafe {
        let hr = core::mem::transmute::<usize, CreateFactoryFn>(
            ORIG_CREATE_FACTORY[0].load(SeqCst),
        )(riid, pp);
        if hr.is_ok() && !pp.is_null() && !(*pp).is_null() {
            hook_factory_vtable(*pp);
        }
        hr
    }
}
unsafe extern "system" fn hook_create_dxgi_factory1(
    riid: *const GUID,
    pp: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: as above.
    unsafe {
        let hr = core::mem::transmute::<usize, CreateFactoryFn>(
            ORIG_CREATE_FACTORY[1].load(SeqCst),
        )(riid, pp);
        if hr.is_ok() && !pp.is_null() && !(*pp).is_null() {
            hook_factory_vtable(*pp);
        }
        hr
    }
}
unsafe extern "system" fn hook_create_dxgi_factory2(
    flags: u32,
    riid: *const GUID,
    pp: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: as above.
    unsafe {
        let hr = core::mem::transmute::<usize, CreateFactory2Fn>(
            ORIG_CREATE_FACTORY[2].load(SeqCst),
        )(flags, riid, pp);
        if hr.is_ok() && !pp.is_null() && !(*pp).is_null() {
            hook_factory_vtable(*pp);
        }
        hr
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}
fn env_is_one(name: &CStr) -> bool {
    let mut value = [0u8; 8];
    // SAFETY: a lookup into our buffer.
    unsafe {
        windows_sys::Win32::System::Environment::GetEnvironmentVariableA(
            name.as_ptr() as *const u8,
            value.as_mut_ptr(),
            8,
        ) > 0
            && value[0] == b'1'
    }
}

/// The three factory exports of `module` (resolved through RenderDoc when a capture layer is resident, so the factory
/// the game gets is the wrapped one).
fn hook_factory_exports_in(module: &str) {
    let name_w = wide(module);
    // SAFETY: a module query.
    let m = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name_w.as_ptr()) };
    if m.is_null() {
        return;
    }
    let targets: [(&CStr, usize); 3] = [
        (
            c"CreateDXGIFactory",
            hook_create_dxgi_factory as *const () as usize,
        ),
        (
            c"CreateDXGIFactory1",
            hook_create_dxgi_factory1 as *const () as usize,
        ),
        (
            c"CreateDXGIFactory2",
            hook_create_dxgi_factory2 as *const () as usize,
        ),
    ];
    for (i, (name, detour)) in targets.into_iter().enumerate() {
        let target = render_doc_resolve_hook_target(m, name.as_ptr());
        if target.is_null() || ORIG_CREATE_FACTORY[i].load(SeqCst) != 0 {
            continue;
        }
        // SAFETY: MinHook on the export.
        let ok = unsafe {
            MH_CreateHook(
                target,
                detour as *mut c_void,
                ORIG_CREATE_FACTORY[i].as_ptr() as *mut *mut c_void,
            ) == MH_OK
                && MH_EnableHook(target) == MH_OK
        };
        if ok {
            log(&format!(
                "PluginBootstrap: hooked {}!{}\n",
                module,
                name.to_string_lossy()
            ));
            // ...and OPTIONALLY tell a resident capture layer that ITS original is our trampoline (off by default: it
            // was seen to kill the game; CPVR_RD_CHAIN=1 tries it again under a debugger).
            if env_is_one(c"CPVR_RD_CHAIN") {
                let ansi = std::ffi::CString::new(module).unwrap_or_default();
                render_doc_chain_behind_hook(
                    ansi.as_ptr(),
                    name.as_ptr(),
                    ORIG_CREATE_FACTORY[i].load(SeqCst) as *mut c_void,
                );
            }
        }
    }
}

/// CyberpunkVRPort_PluginBootstrap: from the plugin entry, as early as possible -- the game must still be ahead of its
/// own CreateDXGIFactory call.
#[no_mangle]
pub extern "C" fn cvr_swapchain_plugin_bootstrap() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED
        .compare_exchange(false, true, SeqCst, SeqCst)
        .is_err()
    {
        return;
    }
    // SAFETY: MinHook's initialisation.
    let st = unsafe { MH_Initialize() };
    if st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED {
        log(&format!("PluginBootstrap: MH_Initialize failed {st}\n"));
        return;
    }
    // Device-initialisation observers go in synchronously while RED4ext loads the plugin.
    install_stage_pre_device();
    // Before anything touches NVAPI: RenderDoc's switch that lets it through (a no-op without a capture layer).
    // SAFETY: a plain query.
    unsafe { cvr_gpu::renderdoc_bridge::cvr_renderdoc_allow_nvapi() };
    // A capture layer in the process is only named in the log; CPVR_NO_FACTORY_HOOKS=1 is the one thing that disarms
    // the factory hooks (stereo, the mirror and the forced FOV are built on them).
    let capture = [
        "ngfx-capture-interception.dll",
        "ngfx-capture-injection.dll",
    ]
    .into_iter()
    .find(|m| {
        let w = wide(m);
        // SAFETY: a module query.
        !unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(w.as_ptr()) }
            .is_null()
    });
    let mut value = [0u8; 8];
    // SAFETY: a lookup into our buffer.
    let have_var = unsafe {
        windows_sys::Win32::System::Environment::GetEnvironmentVariableA(
            c"CPVR_NO_FACTORY_HOOKS".as_ptr() as *const u8,
            value.as_mut_ptr(),
            8,
        )
    } > 0;
    if have_var && value[0] == b'1' {
        log(&format!(
            "PluginBootstrap: factory exports left unhooked ({}) -- stereo and the mirror will not work, the render and the capture will. capture layer: {}\n",
            "CPVR_NO_FACTORY_HOOKS=1",
            capture.unwrap_or("none")
        ));
        return;
    }
    if let Some(layer) = capture {
        log(&format!(
            "PluginBootstrap: capture layer {layer} is resident and the factory hooks are ON -- stereo, the mirror and the forced FOV work; a capture from Nsight has been seen to crash inside that layer.\n"
        ));
    }
    // Both the real DXGI and Streamline's interposer: with frame generation the game talks to the interposer's exports.
    hook_factory_exports_in("dxgi.dll");
    hook_factory_exports_in("sl.interposer.dll");
}

// ---- the helpers the other swapchain files use -------------------------------------------------------------------

/// # Safety
/// `out_w` and `out_h` are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_swapchain_fit_render_size(
    rw: u32,
    rh: u32,
    mw: u32,
    mh: u32,
    out_w: *mut u32,
    out_h: *mut u32,
) {
    let (w, h) = crate::swapchain_fit::fit_render_size_inside_bounds(rw, rh, mw, mh);
    // SAFETY: the caller's slots.
    unsafe {
        *out_w = w;
        *out_h = h;
    }
}

/// DebugName: the ANSI name when there is one, else the wide one converted to UTF-8 into `buffer`.
///
/// # Safety
/// `ansi` and `wide` are null or NUL-terminated; `buffer` holds `size` bytes (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_swapchain_debug_name(
    ansi: *const c_char,
    wide: *const u16,
    buffer: *mut c_char,
    size: usize,
) -> *const c_char {
    // SAFETY: the caller's strings and buffer.
    unsafe {
        if !ansi.is_null() && *ansi != 0 {
            return ansi;
        }
        if wide.is_null() {
            return c"<unnamed>".as_ptr();
        }
        if buffer.is_null() || size == 0 {
            return c"<invalid-buffer>".as_ptr();
        }
        let written = windows_sys::Win32::Globalization::WideCharToMultiByte(
            windows_sys::Win32::Globalization::CP_UTF8,
            0,
            wide,
            -1,
            buffer as *mut u8,
            size as i32,
            core::ptr::null(),
            core::ptr::null_mut(),
        );
        if written > 0 {
            buffer
        } else {
            c"<wide-conversion-failed>".as_ptr()
        }
    }
}

// ---- the hub's table (formerly src/Hooks/SwapChain.cpp's): the state it reads and the subsystems it calls, in the
// hub's order -- Rust's directly, the OpenXR manager's and the GPU frame timer's through the C++ half's services
mod table {
    use super::Runtime;
    use core::ffi::c_void;
    use core::sync::atomic::AtomicPtr;
    use windows::Win32::Foundation::RECT;

    /// std::atomic<void*> g_presentQueue: the queue the game presents on (the queue observer's, DepthCapture.cpp's).
    #[export_name = "?g_presentQueue@@3U?$atomic@PEAX@std@@A"]
    #[allow(non_upper_case_globals)]
    pub static g_presentQueue: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        /// HWND g_gameHwnd (src/Hooks/OsInput.cpp).
        #[link_name = "?g_gameHwnd@@3PEAUHWND__@@EA"]
        static mut g_gameHwnd: *mut c_void;
    }
    unsafe extern "C" fn metrics() -> bool {
        cvr_gpu::framegen_core::cvr_framegen_metrics_enabled()
    }
    unsafe extern "C" fn on_gpu_frame(ms: f64) {
        cvr_gpu::framegen_core::cvr_framegen_on_gpu_frame(ms)
    }
    /// cvr::framegen::FinishGpuFrame under the image-submission lock.
    extern "C" fn finish_gpu_frame() {
        cvr_host::submission::cvr_image_submission_lock();
        // SAFETY: the frame timer's callbacks of their types.
        unsafe { cvr_gpu::frame_timer::cvr_gpu_frame_timer_finish(metrics, on_gpu_frame) };
        cvr_host::submission::cvr_image_submission_unlock();
    }
    extern "C" fn gpu_profile_poll() {
        cvr_gpu::stage_profile::cvr_gpu_stage_poll()
    }
    extern "C" fn scene_profile_poll() {
        cvr_gpu::scene_profile::cvr_scene_profile_poll()
    }
    extern "C" fn pair_probe_poll() {
        crate::pair_probe::cvr_pair_probe_poll()
    }
    extern "C" fn cascades_poll() {
        cvr_gpu::mono_cascades::cvr_mono_cascades_poll()
    }
    extern "C" fn xr_on_present(swap_chain: *mut c_void) {
        crate::xr_present_host::on_present(swap_chain)
    }
    extern "C" fn xr_pump_inline_frame() {
        crate::xr_manager_loop::pump_inline_frame()
    }
    extern "C" fn xr_report_frame_rates() {
        crate::xr_manager_loop::report_frame_rates()
    }
    /// DesktopMirrorRender: only with a swapchain and a window.
    extern "C" fn desktop_mirror_render(swap_chain: *mut c_void, hwnd: *mut c_void) {
        if !swap_chain.is_null() && !hwnd.is_null() {
            // SAFETY: the game's swapchain and window.
            unsafe { cvr_gpu::desktop_mirror::cvr_desktop_mirror_render(swap_chain, hwnd) }
        }
    }
    extern "C" fn desktop_mirror_invalidate() {
        cvr_gpu::desktop_mirror::cvr_desktop_mirror_invalidate()
    }
    extern "C" fn xr_init_graphics(device: *mut c_void, queue: *mut c_void) {
        crate::xr_manager_loop::init_graphics(device, queue);
    }
    extern "C" fn desktop_mirror_set_device_and_queue(device: *mut c_void, queue: *mut c_void) {
        // SAFETY: the game's device and queue (or null).
        unsafe { cvr_gpu::desktop_mirror::cvr_desktop_mirror_set_device_and_queue(device, queue) }
    }
    /// GetDesktopMonitorWindowRect(hwnd, rect): the window's rectangle on its monitor, no monitor or target rectangle.
    extern "C" fn desktop_monitor_window_rect(hwnd: *mut c_void, rect: *mut RECT) -> bool {
        // SAFETY: the caller's window and rectangle.
        unsafe {
            cvr_gpu::desktop_mirror::cvr_desktop_monitor_window_rect(hwnd, rect.cast(), core::ptr::null_mut(), core::ptr::null_mut())
        }
    }
    use crate::moved_globals::{g_cursorClipped, g_eclDistinctQueues, g_eclPresentCalls, g_eclPresentLists, g_eclTotalCalls, g_eclTotalLists};
    pub(super) static RT: Runtime = Runtime {
        game_hwnd: &raw mut g_gameHwnd,
        cursor_clipped: (&raw const g_cursorClipped).cast::<bool>(),
        present_queue: &raw const g_presentQueue,
        ecl_total_calls: &raw const g_eclTotalCalls,
        ecl_total_lists: &raw const g_eclTotalLists,
        ecl_present_calls: &raw const g_eclPresentCalls,
        ecl_present_lists: &raw const g_eclPresentLists,
        ecl_distinct_queues: &raw const g_eclDistinctQueues,
        finish_gpu_frame,
        gpu_profile_poll,
        scene_profile_poll,
        pair_probe_poll,
        cascades_poll,
        xr_on_present,
        xr_pump_inline_frame,
        xr_report_frame_rates,
        desktop_mirror_render,
        desktop_mirror_invalidate,
        xr_init_graphics,
        desktop_mirror_set_device_and_queue,
        desktop_monitor_window_rect,
    };
}
