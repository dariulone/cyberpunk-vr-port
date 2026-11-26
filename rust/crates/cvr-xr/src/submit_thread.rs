//! Who drives the XR frame loop (formerly OpenXRManager::AcquireFrameLoop, ReleaseFrameLoop, NotifySubmitThread and
//! SubmitThreadMain): one owner at a time -- the inline Present pump or the dedicated submit thread. The thread parks
//! (no CPU) while the inline pump owns the loop; when threaded submit is wanted it takes the loop and drives the frame
//! (self-paced by xrWaitFrame), and bootstraps the session itself when threaded submit is on from launch.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::c_void;
use cvr_host::log;
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// OpenXRManager::FrameLoopOwner.
pub const NONE: i32 = 0;
pub const INLINE: i32 = 1;
pub const THREADED: i32 = 2;

static OWNER: Mutex<i32> = Mutex::new(NONE);
static OWNER_CHANGED: Condvar = Condvar::new();
static WAKE: Mutex<()> = Mutex::new(());
static WAKE_CV: Condvar = Condvar::new();

/// OpenXRManager::AcquireFrameLoop: the loop taken by `who`, or false if another owner still holds it after the wait.
#[no_mangle]
pub extern "C" fn cvr_xr_acquire_frame_loop(who: i32, timeout_ms: u32) -> bool {
    let guard = OWNER.lock().unwrap_or_else(|p| p.into_inner());
    let (mut guard, result) = OWNER_CHANGED
        .wait_timeout_while(guard, Duration::from_millis(timeout_ms as u64), |owner| *owner != NONE)
        .unwrap_or_else(|p| p.into_inner());
    if result.timed_out() && *guard != NONE {
        return false;
    }
    *guard = who;
    true
}
/// OpenXRManager::ReleaseFrameLoop.
#[no_mangle]
pub extern "C" fn cvr_xr_release_frame_loop(who: i32) {
    {
        let mut guard = OWNER.lock().unwrap_or_else(|p| p.into_inner());
        if *guard == who {
            *guard = NONE;
        }
    }
    OWNER_CHANGED.notify_all();
}
/// OpenXRManager::NotifySubmitThread: wake the parked thread (threaded submit may be wanted now).
#[no_mangle]
pub extern "C" fn cvr_xr_notify_submit_thread() {
    WAKE_CV.notify_all();
}

/// What the submit thread asks of OpenXRManager.
#[repr(C)]
pub struct SubmitHost {
    pub ctx: *mut c_void,
    /// UseThreadedSubmit: mono submit on and (framegen, the forced mode, or the runtime) wanting the thread.
    pub use_threaded: unsafe extern "C" fn(*mut c_void) -> bool,
    /// One FrameThreadMain.
    pub frame: unsafe extern "C" fn(*mut c_void),
    pub steam_vr: unsafe extern "C" fn(*mut c_void) -> bool,
    pub mono: unsafe extern "C" fn(*mut c_void) -> bool,
}

/// OpenXRManager::SubmitThreadMain.
///
/// # Safety
/// `host` is OpenXRManager's, alive for the thread's life.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_submit_thread_main(host: *const SubmitHost) -> u32 {
    // SAFETY: the caller's host and its callbacks.
    let Some(h) = (unsafe { host.as_ref() }) else { return 0 };
    let stop = || crate::events::cvr_xr_session_shared_ref().stop_frame_thread.load(Ordering::Relaxed);
    // SAFETY: the host's callbacks on its own object.
    let threaded = || unsafe { (h.use_threaded)(h.ctx) };
    log!("OpenXRManager: VR submit thread started (dormant until threaded submit is wanted).\n");
    while !stop() {
        {
            // Wake on threaded submit ALONE (not on a running session): this thread may have to bootstrap it.
            let guard = WAKE.lock().unwrap_or_else(|p| p.into_inner());
            let _ = WAKE_CV.wait_timeout_while(guard, Duration::from_millis(200), |_| !(stop() || threaded()));
        }
        if stop() {
            break;
        }
        if !threaded() {
            continue;
        }
        // Take the loop from the inline owner: not the Present thread, so a bounded wait-and-retry is safe.
        if !cvr_xr_acquire_frame_loop(THREADED, 100) {
            continue;
        }
        // SAFETY: the host's callbacks.
        let (steam_vr, mono) = unsafe { ((h.steam_vr)(h.ctx), (h.mono)(h.ctx)) };
        log!("OpenXRManager: VR submit thread acquired frame loop (steamvr={} mono={}).\n", steam_vr as i32, mono as i32);
        while !stop() && threaded() {
            // SAFETY: one frame of OpenXRManager's loop.
            unsafe { (h.frame)(h.ctx) };
        }
        cvr_xr_release_frame_loop(THREADED);
        log!("OpenXRManager: VR submit thread released frame loop (parking).\n");
    }
    log!("OpenXRManager: VR submit thread exiting.\n");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_owner_at_a_time() {
        assert!(cvr_xr_acquire_frame_loop(INLINE, 0));
        assert!(!cvr_xr_acquire_frame_loop(THREADED, 10), "held by the inline pump");
        cvr_xr_release_frame_loop(THREADED);
        assert!(!cvr_xr_acquire_frame_loop(THREADED, 0), "only its owner releases it");
        cvr_xr_release_frame_loop(INLINE);
        assert!(cvr_xr_acquire_frame_loop(THREADED, 0));
        cvr_xr_release_frame_loop(THREADED);
    }
}
