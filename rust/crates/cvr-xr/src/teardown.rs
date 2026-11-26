//! The haptic pulse (formerly OpenXRManager::ApplyHaptic) and the handles' teardown (OpenXRManager::Shutdown's OpenXR
//! calls): the hands' and the tracking spaces, the session (its C++ owners -- the Vive trackers, the finger bones --
//! told first), the action set, and at the end the instance. The swapchains and everything D3D12 stay in C++ for now.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use openxr_sys as xr;
use xr::Handle as _;

#[cfg(not(test))]
extern "system" {
    fn xrApplyHapticFeedback(session: xr::Session, info: *const xr::HapticActionInfo, feedback: *const xr::HapticBaseHeader) -> xr::Result;
    fn xrDestroySpace(space: xr::Space) -> xr::Result;
    fn xrDestroySession(session: xr::Session) -> xr::Result;
    fn xrDestroyActionSet(set: xr::ActionSet) -> xr::Result;
    fn xrDestroyInstance(instance: xr::Instance) -> xr::Result;
    fn xrEnumerateBoundSourcesForAction(session: xr::Session, info: *const xr::BoundSourcesForActionEnumerateInfo,
        capacity: u32, count: *mut u32, sources: *mut xr::Path) -> xr::Result;
    fn xrPathToString(instance: xr::Instance, path: xr::Path, capacity: u32, count: *mut u32, buffer: *mut core::ffi::c_char) -> xr::Result;
}
#[cfg(not(test))]
extern "C" {
    /// The Vive trackers' and the finger bones' OnSessionDestroying.
    fn cvr_cpp_xr_session_destroying();
}

/// The vibration on one hand: amplitude clamped to 1, duration to 0.5 s, the runtime's own frequency. The result of
/// xrApplyHapticFeedback, or -1 when there is nothing to vibrate (src/Runtimes/Haptics.cpp).
///
/// # Safety
/// The handles are live or 0.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_apply_haptic(session: u64, action: u64, hand_path: u64, running: bool, hand: i32, amplitude: f32, duration_sec: f32) -> i32 {
    if action == 0 || session == 0 || !running {
        return -1;
    }
    if !(0..=1).contains(&hand) || !(amplitude > 0.0) {
        return -1;
    }
    let vibration = xr::HapticVibration {
        ty: xr::StructureType::HAPTIC_VIBRATION,
        next: core::ptr::null(),
        duration: xr::Duration::from_nanos(((if duration_sec > 0.5 { 0.5 } else { duration_sec }) as f64 * 1.0e9) as i64),
        frequency: xr::FREQUENCY_UNSPECIFIED,
        amplitude: if amplitude > 1.0 { 1.0 } else { amplitude },
    };
    let info = xr::HapticActionInfo {
        ty: xr::StructureType::HAPTIC_ACTION_INFO,
        next: core::ptr::null(),
        action: xr::Action::from_raw(action),
        subaction_path: xr::Path::from_raw(hand_path),
    };
    // SAFETY: the live session and action; the vibration as its base header.
    unsafe {
        xrApplyHapticFeedback(xr::Session::from_raw(session), &info, (&vibration as *const xr::HapticVibration).cast()).into_raw()
    }
}

/// The haptic action's bound outputs, logged once: an accepted xrApplyHapticFeedback can be a no-op when a saved SteamVR
/// binding lacks the action (main's OpenXRManager::ApplyHaptic).
///
/// # Safety
/// The handles are live.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_log_haptic_bindings(instance: u64, session: u64, action: u64) {
    let info = xr::BoundSourcesForActionEnumerateInfo {
        ty: xr::StructureType::BOUND_SOURCES_FOR_ACTION_ENUMERATE_INFO,
        next: core::ptr::null(),
        action: xr::Action::from_raw(action),
    };
    let session = xr::Session::from_raw(session);
    let mut count = 0u32;
    // SAFETY (whole body): the live session and instance; buffers of the sizes passed.
    unsafe {
        let r = xrEnumerateBoundSourcesForAction(session, &info, 0, &mut count, core::ptr::null_mut());
        cvr_host::log::write(&format!(
            "OpenXRManager[Haptics]: bound outputs result={} count={count} (zero means check SteamVR bindings).\n",
            r.into_raw()
        ));
        if r.into_raw() >= 0 && count > 0 && count <= 16 {
            let mut paths = [xr::Path::from_raw(0); 16];
            let r = xrEnumerateBoundSourcesForAction(session, &info, 16, &mut count, paths.as_mut_ptr());
            if r.into_raw() >= 0 {
                for path in &paths[..count.min(16) as usize] {
                    let mut text = [0 as core::ffi::c_char; 256];
                    let mut size = 0u32;
                    if xrPathToString(xr::Instance::from_raw(instance), *path, 256, &mut size, text.as_mut_ptr()).into_raw() >= 0 {
                        let name = core::ffi::CStr::from_ptr(text.as_ptr()).to_string_lossy();
                        cvr_host::log::write(&format!("OpenXRManager[Haptics]: bound output {name}\n"));
                    }
                }
            }
        }
    }
}

/// The handles OpenXRManager holds when it shuts the session down; each is destroyed and zeroed.
#[repr(C)]
pub struct Teardown {
    pub hand_spaces: [u64; 2],
    pub hand_aim_spaces: [u64; 2],
    pub view_space: u64,
    pub local_space: u64,
    pub stage_space: u64,
    pub session: u64,
    pub action_set: u64,
}

/// The spaces, the session (its owners told first) and the action set, in OpenXRManager::Shutdown's order.
///
/// # Safety
/// `t` holds live handles or zeros.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_destroy_session(t: *mut Teardown) {
    // SAFETY (whole body): the caller's handles, each destroyed once and zeroed.
    unsafe {
        let Some(t) = t.as_mut() else { return };
        for h in [&crate::frame::SESSION, &crate::frame::LOCAL_SPACE, &crate::frame::VIEW_SPACE] {
            h.store(0, core::sync::atomic::Ordering::Release);
        }
        *crate::session::HAND_SPACES.lock().unwrap_or_else(|p| p.into_inner()) = ([0; 2], [0; 2]);
        if t.action_set != 0 {
            *crate::actions::ACTIONS.lock().unwrap_or_else(|p| p.into_inner()) = crate::actions::ActionSetup::NONE;
        }
        let space = |s: &mut u64| {
            if *s != 0 {
                xrDestroySpace(xr::Space::from_raw(*s));
                *s = 0;
            }
        };
        for i in 0..2 {
            space(&mut t.hand_spaces[i]);
            space(&mut t.hand_aim_spaces[i]);
        }
        space(&mut t.view_space);
        space(&mut t.local_space);
        space(&mut t.stage_space);
        if t.session != 0 {
            cvr_cpp_xr_session_destroying();
            xrDestroySession(xr::Session::from_raw(t.session));
            t.session = 0;
            // the runtime's own queue goes with the session it was bound to
            crate::runtime_queue::release();
        }
        if t.action_set != 0 {
            xrDestroyActionSet(xr::ActionSet::from_raw(t.action_set));
            t.action_set = 0;
        }
    }
}
/// The instance, last.
///
/// # Safety
/// A live instance or 0.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_destroy_instance(instance: u64) {
    crate::view_configs::set_system(0, 0);
    if instance != 0 {
        // SAFETY: the caller's live instance, destroyed once.
        unsafe { xrDestroyInstance(xr::Instance::from_raw(instance)) };
    }
}
