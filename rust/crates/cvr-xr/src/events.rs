//! The session's life (formerly OpenXRManager::PollEvents, BeginSession and EndSession): the runtime's events, the
//! session begun on READY and ended on STOPPING, the frame threads stopped on EXITING and LOSS_PENDING, an automatic
//! recenter when the headset comes back on after an absence (include/Runtimes/PresenceRecenter.hpp), and a local
//! recenter when the runtime's own recenter moves the tracking space.
//!
//! The session's state is owned here (`SessionShared`); in the Rust build OpenXRManager's members are references to it,
//! so every reader in the C++ half reads this. What the C++ half does on a change it is asked through `EventHost`.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_host::log;
use openxr_sys as xr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;
use xr::Handle as _;

/// The session's state, shared with OpenXRManager (its m_stopFrameThread, m_sessionRunning, m_sessionFocused and
/// m_sessionState are references to these).
#[repr(C)]
pub struct SessionShared {
    pub stop_frame_thread: AtomicBool,
    pub running: AtomicBool,
    pub focused: AtomicBool,
    /// XrSessionState.
    pub state: AtomicI32,
}
static SHARED: SessionShared = SessionShared {
    stop_frame_thread: AtomicBool::new(false),
    running: AtomicBool::new(false),
    focused: AtomicBool::new(false),
    state: AtomicI32::new(0),
};
#[no_mangle]
pub extern "C" fn cvr_xr_session_shared() -> *const SessionShared {
    &SHARED
}
/// The session's shared state (the frame threads' stop flag among it).
pub fn cvr_xr_session_shared_ref() -> &'static SessionShared {
    &SHARED
}
/// The session runs (begun, not yet ended).
pub fn session_running() -> bool {
    SHARED.running.load(Ordering::Acquire)
}
/// The session's current state.
pub fn session_state() -> xr::SessionState {
    xr::SessionState::from_raw(SHARED.state.load(Ordering::Relaxed))
}

#[cfg(not(test))]
extern "system" {
    fn xrPollEvent(instance: xr::Instance, buffer: *mut xr::EventDataBuffer) -> xr::Result;
    fn xrBeginSession(session: xr::Session, info: *const xr::SessionBeginInfo) -> xr::Result;
    fn xrEndSession(session: xr::Session) -> xr::Result;
}
#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    /// xr_auto_recenter.
    static CyberpunkVR_AutoRecenter: i32;
}

/// include/Runtimes/PresenceRecenter.hpp OnSessionState: true exactly once per return from an absence.
fn on_session_state(away: &mut bool, previous: xr::SessionState, current: xr::SessionState) -> bool {
    let was_shown = previous == xr::SessionState::VISIBLE || previous == xr::SessionState::FOCUSED;
    if current == xr::SessionState::SYNCHRONIZED && was_shown {
        *away = true;
    }
    if current == xr::SessionState::STOPPING || current == xr::SessionState::IDLE {
        *away = false;
    }
    if current == xr::SessionState::FOCUSED && *away {
        *away = false;
        return true;
    }
    false
}
static AWAY: Mutex<bool> = Mutex::new(false);

/// OpenXRManager::BeginSession: false when there is no session or it already runs; once begun, a threaded submit
/// wanted before the session started may begin (the submit thread woken).
///
/// # Safety
/// `session` is the live session or 0.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_begin_session(session: u64) -> bool {
    if session == 0 || SHARED.running.load(Ordering::Relaxed) {
        return false;
    }
    let info = xr::SessionBeginInfo {
        ty: xr::StructureType::SESSION_BEGIN_INFO,
        next: core::ptr::null(),
        primary_view_configuration_type: xr::ViewConfigurationType::PRIMARY_STEREO,
    };
    // SAFETY: the live session.
    unsafe {
        let res = xrBeginSession(xr::Session::from_raw(session), &info);
        if res.into_raw() < 0 {
            log!("OpenXRManager: xrBeginSession failed (res={})\n", res.into_raw());
            return false;
        }
        SHARED.running.store(true, Ordering::Relaxed);
        // In case threaded submit was wanted before the session started.
        crate::submit_thread::cvr_xr_notify_submit_thread();
    }
    log!("OpenXRManager: Session begun.\n");
    true
}
/// OpenXRManager::EndSession.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_end_session(session: u64) {
    if session == 0 || !SHARED.running.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: the live session.
    unsafe { xrEndSession(xr::Session::from_raw(session)) };
    SHARED.running.store(false, Ordering::Relaxed);
    log!("OpenXRManager: Session ended.\n");
}

#[cfg(not(test))]
extern "system" {
    fn xrGetCurrentInteractionProfile(session: xr::Session, top: xr::Path, profile: *mut xr::InteractionProfileState) -> xr::Result;
    fn xrPathToString(instance: xr::Instance, path: xr::Path, capacity: u32, count: *mut u32, buffer: *mut core::ffi::c_char) -> xr::Result;
}
#[cfg(not(test))]
extern "C" {
    fn GetInputActionsEnabled() -> i32;
    /// cvr-rig haptics.rs: every pending pulse dropped.
    fn cvr_haptics_clear();
}
/// Each hand's interaction profile as the runtime selected it.
///
/// # Safety
/// The live instance and session.
#[cfg(not(test))]
unsafe fn log_interaction_profiles(instance: xr::Instance, session: xr::Session) {
    // SAFETY (whole body): the manager's handles and paths; buffers of their sizes.
    unsafe {
        let paths = crate::core_handles::core().hand_paths;
        for (hand, &top) in paths.iter().enumerate() {
            let mut profile: xr::InteractionProfileState = core::mem::zeroed();
            profile.ty = xr::StructureType::INTERACTION_PROFILE_STATE;
            let result = xrGetCurrentInteractionProfile(session, xr::Path::from_raw(top), &mut profile);
            let mut name = String::from("unbound");
            if result.into_raw() >= 0 && profile.interaction_profile != xr::Path::NULL {
                let mut buffer = [0 as core::ffi::c_char; 256];
                let mut count = 0u32;
                name = if xrPathToString(instance, profile.interaction_profile, buffer.len() as u32, &mut count, buffer.as_mut_ptr()).into_raw() < 0 {
                    "unresolved".into()
                } else {
                    core::ffi::CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
                };
            }
            log!(
                "OpenXRManager[Input]: hand={} profile={} result={} actions={} xinput={} disable_mouse_y={}\n",
                if hand == 0 { "left" } else { "right" },
                name,
                result.into_raw(),
                GetInputActionsEnabled(),
                cvr_settings::lc!(xrXInputHook),
                cvr_settings::lc!(xrDisableMouseY)
            );
        }
    }
}

/// OpenXRManager::PollEvents: every event the runtime has queued; a recenter asked for (crate::tracking) when the
/// headset is back on after an absence or the runtime moved the tracking space.
///
/// # Safety
/// `instance` and `session` are the live handles (0 when absent).
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_poll_events(instance: u64, session: u64) {
    if instance == 0 {
        return;
    }
    // SAFETY (whole body): the loader's queue read into a buffer of its size.
    unsafe {
        let instance = xr::Instance::from_raw(instance);
        let mut event: xr::EventDataBuffer = core::mem::zeroed();
        event.ty = xr::StructureType::EVENT_DATA_BUFFER;
        while xrPollEvent(instance, &mut event) == xr::Result::SUCCESS {
            if event.ty == xr::StructureType::EVENT_DATA_SESSION_STATE_CHANGED {
                let changed = &*(&event as *const xr::EventDataBuffer as *const xr::EventDataSessionStateChanged);
                let previous = xr::SessionState::from_raw(SHARED.state.load(Ordering::Relaxed));
                let current = changed.state;
                SHARED.state.store(current.into_raw(), Ordering::Relaxed);
                SHARED.focused.store(current == xr::SessionState::FOCUSED, Ordering::Relaxed);
                if current != xr::SessionState::FOCUSED {
                    crate::input::focus_lost();
                }
                log!("OpenXRManager: Session state -> {}\n", current.into_raw());
                let back = {
                    let mut away = AWAY.lock().unwrap_or_else(|p| p.into_inner());
                    on_session_state(&mut away, previous, current)
                };
                if back {
                    if CyberpunkVR_AutoRecenter != 0 {
                        crate::tracking::cvr_xr_request_recenter();
                        log!("OpenXRManager: headset back on after an absence -> automatic recenter.\n");
                    } else {
                        log!("OpenXRManager: headset back on after an absence (xr_auto_recenter=0, not recentering).\n");
                    }
                }
                if current == xr::SessionState::READY {
                    cvr_xr_begin_session(session);
                } else if current == xr::SessionState::STOPPING {
                    // the controllers stop buzzing with the session (main's 0.2.4)
                    cvr_haptics_clear();
                    cvr_xr_end_session(session);
                } else if current == xr::SessionState::EXITING || current == xr::SessionState::LOSS_PENDING {
                    SHARED.stop_frame_thread.store(true, Ordering::Relaxed);
                }
            } else if event.ty == xr::StructureType::EVENT_DATA_INTERACTION_PROFILE_CHANGED {
                // A successful binding suggestion is not proof the runtime selected that profile: the actual per-hand
                // profile reported for input reports (main's 0.2.4).
                if session != 0 {
                    log_interaction_profiles(instance, xr::Session::from_raw(session));
                }
            } else if event.ty == xr::StructureType::EVENT_DATA_REFERENCE_SPACE_CHANGE_PENDING {
                let changed = &*(&event as *const xr::EventDataBuffer as *const xr::EventDataReferenceSpaceChangePending);
                log!(
                    "OpenXRManager: Tracking space change pending (native recenter), refSpace={} -> local recenter.\n",
                    changed.reference_space_type.into_raw()
                );
                crate::tracking::cvr_xr_request_recenter();
            }
            event = core::mem::zeroed();
            event.ty = xr::StructureType::EVENT_DATA_BUFFER;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn count(states: &[xr::SessionState]) -> usize {
        let mut away = false;
        states.windows(2).filter(|w| on_session_state(&mut away, w[0], w[1])).count()
    }
    #[test]
    fn presence_recenters_as_the_header_asserts() {
        use xr::SessionState as S;
        assert_eq!(count(&[S::IDLE, S::READY, S::SYNCHRONIZED, S::VISIBLE, S::FOCUSED]), 0, "start-up");
        assert_eq!(count(&[S::FOCUSED, S::VISIBLE, S::FOCUSED]), 0, "dashboard");
        assert_eq!(count(&[S::FOCUSED, S::VISIBLE, S::SYNCHRONIZED, S::VISIBLE, S::FOCUSED]), 1, "off and on");
        assert_eq!(count(&[S::FOCUSED, S::SYNCHRONIZED, S::FOCUSED, S::SYNCHRONIZED, S::FOCUSED]), 2, "twice");
    }
}
