//! The core's small C++ services in Rust (formerly Hooks/SettingsRes.cpp's, Framegen/Inputs.cpp's,
//! Natives/EngineStereoMenu.cpp's and Main.cpp's): the settings' resolution override applied with the settings object
//! kept, the forced render size, the current node's pose for frame generation, the lobby's ambient switch saved with
//! the live controls, and the game hooks' worker thread.

use core::ffi::c_void;
use core::sync::atomic::Ordering::Relaxed;

unsafe extern "C" {
    /// cvr-rig's pose_identity.rs: the current node's pose identity.
    fn cvr_pose_node_current(out: *mut c_void);
}

/// The settings object's resolution override (settings_res.rs's callback): the object kept, the override applied,
/// the previous object returned.
#[no_mangle]
pub extern "C" fn cvr_cpp_settings_res_apply(settings: usize) -> usize {
    let previous = crate::vrcore_exports::g_settingsResPtr.load(Relaxed);
    crate::vrcore_exports::g_settingsResPtr.store(settings, Relaxed);
    crate::vr_core_settings::cvr_apply_settings_resolution_override(settings);
    previous
}
/// The forced render size (0 when not forced).
///
/// # Safety
/// Both outputs are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_forced_render_size(width: *mut u32, height: *mut u32) {
    // SAFETY: the caller's outputs.
    unsafe {
        *width = crate::vr_core_settings::cvr_forced_render_size(true);
        *height = crate::vr_core_settings::cvr_forced_render_size(false);
    }
}

/// The node pose frame generation keys its inputs by (framegen_inputs.rs's CvrNodePose).
#[repr(C)]
pub struct NodePose {
    pose_id: u64,
    origin: u64,
    view: u32,
    render_frame_id: u32,
    valid: bool,
    has_render_frame_id: bool,
}
/// The current node's pose identity as frame generation reads it.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_framegen_node_pose(out: *mut NodePose) {
    let mut identity = core::mem::MaybeUninit::<crate::xr_capture::PoseIdentity>::zeroed();
    // SAFETY: an identity to fill (plain data, all zero the empty one); the caller's output.
    unsafe {
        cvr_pose_node_current(identity.as_mut_ptr().cast());
        let p = identity.assume_init();
        *out = NodePose {
            pose_id: p.pose_id,
            origin: p.head.origin_serial,
            view: p.view,
            render_frame_id: p.render_frame_id,
            valid: p.present(),
            has_render_frame_id: p.has_render_frame_id,
        };
    }
}

/// The lobby's ambient switch, saved with the live controls (lobby_natives.rs's).
#[no_mangle]
pub extern "C" fn cvr_cpp_set_lobby_ambient(on: bool) {
    // SAFETY: plain numbers and flags, assigned whole by the getter, then applied and saved whole.
    unsafe {
        let mut state: cvr_settings::live_controls_ui_state::LiveControlsUiState = core::mem::zeroed();
        GetLiveControlsUiState(&mut state);
        state.lobbyAmbient = i32::from(on);
        crate::live_controls_ui::SetLiveControlsUiState(&state, 1);
    }
}

/// The game hooks' thread (WorkerThread).
unsafe extern "system" fn worker_thread(_: *mut c_void) -> u32 {
    crate::worker::cvr_worker_thread_main()
}
/// The game hooks on their own thread, exactly as DllMain used to start them.
#[no_mangle]
pub extern "C" fn cvr_cpp_start_worker_thread() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::CreateThread;
    // SAFETY: a new thread on the worker's entry; its handle closed.
    unsafe {
        let handle = CreateThread(core::ptr::null(), 0, Some(worker_thread), core::ptr::null(), 0, core::ptr::null_mut());
        if !handle.is_null() {
            CloseHandle(handle);
        }
    }
}

/// GetLiveControlsUiState (formerly LiveControlsPoll.cpp's, over MakeLiveControlsUiState): the overlay's snapshot of
/// the live controls, built from a zeroed state as the C++ value-initialized it.
///
/// # Safety
/// `out` is a LiveControlsUiState or null.
#[no_mangle]
pub unsafe extern "C" fn GetLiveControlsUiState(out: *mut cvr_settings::live_controls_ui_state::LiveControlsUiState) {
    if out.is_null() {
        return;
    }
    // SAFETY: plain numbers and flags; the caller's state.
    unsafe {
        *out = core::mem::zeroed();
        crate::live_controls_ui::cvr_make_live_controls_ui_state(out);
    }
}
