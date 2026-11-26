//! OpenXRManager::Init, InitGraphics and Shutdown's own parts (formerly src/Runtimes/OpenXRManager.cpp's Rust branch): the
//! instance and the
//! system with the runtime's facts (the preferred runtime first, the Vive hand tracking layer kept out), the action set
//! and its actions; then the session on the game's D3D12 device and queue, the finger tracking's session, the submit
//! state, the tracking, view and hand spaces (STAGE under SteamVR unless a test marker asks for LOCAL), the trackers'
//! session, and the dormant submit thread. Every handle lands in core_handles.rs' block, which the C++ members reference.
//! Shutdown: the session asked to exit and the submit thread stopped, then -- after the C++ half shut its own objects
//! down -- the session ended, the swapchains, spaces, session, action set and instance destroyed and every handle
//! cleared, in the C++ order.
//!
//! The C++ methods forward: Init under the manager's init lock, InitGraphics with its submit thread's entry, Shutdown
//! around its objects.
#[cfg(not(test))]
use core::ffi::{c_char, c_void};
#[cfg(not(test))]
use core::sync::atomic::Ordering::Relaxed;

#[cfg(not(test))]
use cvr_host::log;
#[cfg(not(test))]
use openxr_sys as xr;
#[cfg(not(test))]
use xr::Handle as _;
#[cfg(not(test))]
use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesA, INVALID_FILE_ATTRIBUTES};
#[cfg(not(test))]
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameA;
#[cfg(not(test))]
use windows_sys::Win32::System::Threading::{CreateThread, LPTHREAD_START_ROUTINE};

#[cfg(not(test))]
use crate::actions::{cvr_xr_create_actions, ActionSetup};
#[cfg(not(test))]
use crate::core_handles::core;
#[cfg(not(test))]
use crate::instance::{cvr_xr_create_instance, InstanceSetup};
#[cfg(not(test))]
use crate::session::{cvr_xr_create_session, cvr_xr_create_spaces, SessionSpaces};

#[cfg(not(test))]
extern "C" {
    /// rust/crates/cvr-render/src/vr_core_settings.rs: xr_input_actions.
    fn GetInputActionsEnabled() -> i32;
    /// rust/crates/cvr-render/src/depth_layer.rs: whether the runtime takes depth layers.
    fn cvr_xr_depth_set_supported(value: bool);
    /// rust/crates/cvr-rig/src/vrik/fingergrip.rs: the finger tracking's session (with the loader's proc lookup).
    fn cvr_fingergrip_session_created(instance: u64, system: u64, session: u64, get_proc: *const c_void);
}
#[cfg(not(test))]
extern "system" {
    fn xrGetInstanceProcAddr(instance: xr::Instance, name: *const c_char, function: *mut Option<xr::pfn::VoidFunction>) -> xr::Result;
}
/// XR_TYPE_GRAPHICS_BINDING_D3D12_KHR.
#[cfg(not(test))]
const GRAPHICS_BINDING_D3D12: u64 = 1_000_028_000;

#[cfg(not(test))]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}
/// cvr::TestMarker: a file of this name sits next to the game executable.
pub(crate) fn test_marker(name: &str) -> bool {
    let mut path = [0u8; MAX_PATH as usize];
    // SAFETY: our buffer.
    let length = unsafe { GetModuleFileNameA(core::ptr::null_mut(), path.as_mut_ptr(), MAX_PATH) } as usize;
    if length == 0 {
        return false;
    }
    let dir = match path[..length].iter().rposition(|&c| c == b'\\') {
        Some(slash) => &path[..=slash],
        None => &path[..length],
    };
    let mut full = dir.to_vec();
    full.extend_from_slice(name.as_bytes());
    full.push(0);
    // SAFETY: a NUL-terminated path.
    unsafe { GetFileAttributesA(full.as_ptr()) != INVALID_FILE_ATTRIBUTES }
}

/// OpenXRManager::Init (the manager's init lock held by the caller).
#[no_mangle]
#[cfg(not(test))]
pub extern "C" fn cvr_xr_manager_init() -> bool {
    // SAFETY: under the manager's init lock.
    let c = unsafe { core() };
    if c.initialized {
        return true;
    }
    log::write("OpenXRManager: Initializing...\n");
    let name = wide("DISABLE_XR_APILAYER_VIVE_HAND_TRACKING_1");
    let value = wide("1");
    // SAFETY: NUL-terminated wide strings.
    unsafe { SetEnvironmentVariableW(name.as_ptr(), value.as_ptr()) };
    crate::runtime_select::cvr_xr_configure_preferred_runtime();
    // SAFETY: all-zero is an empty setup; the call fills it.
    let mut setup: InstanceSetup = unsafe { core::mem::zeroed() };
    // SAFETY: our setup.
    if !unsafe { cvr_xr_create_instance(&mut setup) } {
        return false;
    }
    c.instance = setup.instance;
    c.system_id = setup.system_id;
    // SAFETY: a plain store.
    unsafe { cvr_xr_depth_set_supported(setup.depth_layer_supported) };
    let steam_frame = setup.steam_frame_enabled;
    // SAFETY: the runtime facts' block.
    let runtime = unsafe { &*crate::runtime_info::cvr_xr_runtime_shared() };
    runtime.virtual_desktop.store(setup.runtime_is_virtual_desktop, Relaxed);
    runtime.steamvr.store(setup.runtime_is_steamvr, Relaxed);
    runtime.simulator.store(setup.runtime_is_simulator, Relaxed);
    runtime.psvr2.store(setup.runtime_is_psvr2, Relaxed);
    // strncpy_s(m_systemName, setup.systemName, _TRUNCATE): up to 127 bytes and the terminator.
    c.system_name = [0; 128];
    for (i, &ch) in setup.system_name.iter().take(127).enumerate() {
        if ch == 0 {
            break;
        }
        c.system_name[i] = ch as u8;
    }
    c.eye_image_limit = setup.eye_image_limit;
    // The PSVR2's haptics: the Toolkit bridge when it is asked for and its listener runs in this Windows session, else
    // OpenXR; a startup snapshot (route changes need a restart).
    let psvr2 = runtime.psvr2.load(Relaxed);
    let bridge = psvr2 && crate::haptic_bridge::detect_toolkit_bridge();
    let route = &cvr_settings::haptic_route::HAPTIC_ROUTE;
    route.begin_session(psvr2, bridge);
    log::write(&format!(
        "OpenXRManager[Haptics]: output={} requested={} bridgeListenerDetected={}; route changes require game restart.\n",
        if route.toolkit() { "Toolkit exclusive" } else { "OpenXR" },
        route.requested(),
        bridge as i32
    ));
    // The gameplay action set, its actions and every profile's bindings: actions.rs.
    // SAFETY: all-zero is an empty setup; the call fills it.
    let mut actions: ActionSetup = unsafe { core::mem::zeroed() };
    // SAFETY: plain getters; our setup.
    unsafe {
        cvr_xr_create_actions(c.instance, GetInputActionsEnabled() != 0, runtime.psvr2.load(Relaxed), steam_frame, &mut actions);
    }
    c.action_set = actions.action_set;
    c.hand_paths = actions.hand_paths;
    c.hand_pose_action = actions.hand_pose;
    c.hand_aim_pose_action = actions.hand_aim_pose;
    c.haptic_action = actions.haptic;
    c.initialized = true;
    true
}

/// OpenXRManager::InitGraphics: `thread` and `context` the manager's submit thread entry (SubmitThreadThunk, this).
///
/// # Safety
/// `device` and `queue` are the game's live D3D12 device and queue; `thread` runs with `context`.
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn cvr_xr_manager_init_graphics(device: *mut c_void, queue: *mut c_void, thread: LPTHREAD_START_ROUTINE, context: *mut c_void) -> bool {
    // SAFETY: the manager's sequencing (InitGraphics once the instance is up).
    let c = unsafe { core() };
    if !c.initialized || c.session != 0 {
        return false;
    }
    log::write("OpenXRManager: Initializing D3D12 Graphics Binding...\n");
    // XrGraphicsBindingD3D12KHR: the type (its high half the padding), `next` left as it was, the device, the queue.
    c.graphics_binding[0] = (c.graphics_binding[0] & !0xFFFF_FFFF) | GRAPHICS_BINDING_D3D12;
    c.graphics_binding[2] = device as u64;
    c.graphics_binding[3] = queue as u64;
    // SAFETY: the instance and system made by Init; the caller's device and queue.
    c.session = unsafe { cvr_xr_create_session(c.instance, c.system_id, device, queue) };
    if c.session == 0 {
        return false;
    }
    // SAFETY: the loader's lookup for the finger tracking's extension functions.
    unsafe { cvr_fingergrip_session_created(c.instance, c.system_id, c.session, xrGetInstanceProcAddr as *const c_void) };
    crate::views::cvr_xr_reset_fov_log();
    // SAFETY: as documented.
    if !unsafe { crate::submit_state::cvr_xr_submit_take(device, queue) } {
        return false;
    }
    log::write("OpenXRManager: Pose-only mode active until xr_mono_submit is enabled.\n");
    // SAFETY: the runtime facts' block.
    let steamvr = unsafe { &*crate::runtime_info::cvr_xr_runtime_shared() }.steamvr.load(Relaxed);
    let stage_tracking = steamvr && !test_marker("cvr_steamvr_local_space.txt");
    // The tracking and view spaces, the action set attached and the hands' spaces: session.rs.
    // SAFETY: all-zero is an empty result; the call fills it.
    let mut spaces: SessionSpaces = unsafe { core::mem::zeroed() };
    // SAFETY: the session and actions made above; our result.
    let made = unsafe {
        cvr_xr_create_spaces(c.session, stage_tracking, c.action_set, c.hand_pose_action, c.hand_aim_pose_action, c.hand_paths.as_ptr(), &mut spaces)
    };
    if !made {
        return false;
    }
    c.local_space = spaces.local;
    c.view_space = spaces.view;
    c.hand_spaces = spaces.hands;
    c.hand_aim_spaces = spaces.hand_aims;
    if c.action_set != 0 {
        // SAFETY: the session and its hand spaces.
        unsafe { crate::trackers::cvr_trackers_on_session_created(c.session, c.hand_spaces.as_ptr()) };
    }
    crate::events::cvr_xr_session_shared_ref().stop_frame_thread.store(false, Relaxed);
    if c.frame_thread.is_null() {
        // SAFETY: the caller's thread entry and context.
        let handle: HANDLE = unsafe { CreateThread(core::ptr::null(), 0, thread, context, 0, core::ptr::null_mut()) };
        c.frame_thread = handle;
        if !handle.is_null() {
            log::write("OpenXRManager: VR submit thread created (dormant).\n");
        } else {
            // SAFETY: no preconditions.
            let error = unsafe { GetLastError() };
            log::write(&format!("OpenXRManager: WARNING failed to create the VR submit thread (err={error}); threaded submit unavailable.\n"));
        }
    }
    log::write("OpenXRManager: Session created successfully.\n");
    true
}

#[cfg(not(test))]
extern "C" {
    /// rust/crates/cvr-render/src/xr_submit.rs.
    fn cvr_xr_submit_destroy_swapchains();
    fn cvr_xr_submit_release();
    /// rust/crates/cvr-render/src/xr_capture_state.rs.
    fn cvr_xr_capture_shutdown();
    /// rust/crates/cvr-render/src/depth_layer.rs.
    fn cvr_xr_depth_forget_format();
}
#[cfg(not(test))]
extern "system" {
    fn xrRequestExitSession(session: xr::Session) -> xr::Result;
}

/// Shutdown's start (the manager's init lock held): the session asked to exit if it runs, the parked submit thread woken
/// to see the stop flag, and joined (5 s) and closed.
#[no_mangle]
#[cfg(not(test))]
pub extern "C" fn cvr_xr_manager_stop() {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    // SAFETY: under the manager's init lock.
    let c = unsafe { core() };
    let session = crate::events::cvr_xr_session_shared_ref();
    session.stop_frame_thread.store(true, Relaxed);
    if c.session != 0 && session.running.load(Relaxed) {
        // SAFETY: the live session.
        unsafe { xrRequestExitSession(xr::Session::from_raw(c.session)) };
    }
    crate::submit_thread::cvr_xr_notify_submit_thread();
    if !c.frame_thread.is_null() {
        // SAFETY: the thread this manager made.
        let wait = unsafe { WaitForSingleObject(c.frame_thread, 5000) };
        if wait != WAIT_OBJECT_0 {
            log::write(&format!("OpenXRManager: WARNING submit thread did not stop during shutdown (wait={wait}).
"));
        }
        // SAFETY: as above, closed once.
        unsafe { CloseHandle(c.frame_thread) };
        c.frame_thread = core::ptr::null_mut();
    }
}
/// Shutdown's end (the manager's init lock held, its objects shut down): the session ended, the swapchains destroyed,
/// the spaces, session and action set torn down, the handles cleared, the view configurations, submit ring, capture
/// ring, presented frame, capture state, submit device and depth format released, the instance destroyed, the head's
/// pose and base forgotten, one log line.
#[no_mangle]
#[cfg(not(test))]
pub extern "C" fn cvr_xr_manager_teardown() {
    // SAFETY: under the manager's init lock.
    let c = unsafe { core() };
    // SAFETY: the session (or none).
    unsafe { crate::events::cvr_xr_end_session(c.session) };
    // SAFETY: plain releases of the Rust halves' own state.
    unsafe { cvr_xr_submit_destroy_swapchains() };
    // SAFETY: the calibration block, alive for the process.
    let calibration = unsafe { &*crate::calibration::cvr_xr_calibration() };
    let mut down = crate::teardown::Teardown {
        hand_spaces: c.hand_spaces,
        hand_aim_spaces: c.hand_aim_spaces,
        view_space: c.view_space,
        local_space: c.local_space,
        stage_space: calibration.stage_space.load(Relaxed),
        session: c.session,
        action_set: c.action_set,
    };
    // SAFETY: the handles this manager made.
    unsafe { crate::teardown::cvr_xr_destroy_session(&mut down) };
    c.hand_spaces = [0; 2];
    c.hand_aim_spaces = [0; 2];
    c.view_space = 0;
    c.local_space = 0;
    calibration.stage_space.store(0, Relaxed);
    calibration.stage_unavailable.store(false, Relaxed);
    c.session = 0;
    c.action_set = 0;
    c.hand_pose_action = 0;
    c.hand_aim_pose_action = 0;
    crate::view_configs::cvr_xr_forget_view_configs();
    // SAFETY: the runtime facts' block.
    unsafe { &*crate::runtime_info::cvr_xr_runtime_shared() }.steamvr.store(false, Relaxed);
    // SAFETY: plain releases of the Rust halves' own state.
    unsafe { cvr_xr_submit_release() };
    crate::capture_ring::cvr_xr_capture_release_ring();
    crate::presented::cvr_xr_presented_release();
    // SAFETY: as above.
    unsafe {
        cvr_xr_capture_shutdown();
        crate::submit_state::cvr_xr_submit_release_device();
        cvr_xr_depth_forget_format();
    }
    if c.instance != 0 {
        // SAFETY: the instance this manager made.
        unsafe { crate::teardown::cvr_xr_destroy_instance(c.instance) };
        c.instance = 0;
    }
    c.initialized = false;
    crate::tracking::HEAD.pose_valid.store(false, Relaxed);
    {
        let guard = crate::tracking::HEAD.render_pose_lock.lock();
        *crate::tracking::HEAD.base_pose_set.get(&guard) = false;
    }
    log::write("OpenXRManager: Shutdown complete.
");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_marker_beside_the_test() {
        assert!(!test_marker("cvr_no_such_marker_for_tests.txt"));
    }
}
