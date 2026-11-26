//! SeamlessLoading (formerly src/Hooks/SeamlessLoading.cpp): a loaded save, a new game, goes on into the world the
//! moment the world is ready -- the loading screen does not wait for a button. Verified against game 2.5 with IDA and
//! live (2026-10-28).
//!
//! The initial loading's screen waits for the player once its world is ready: when the load is done (+0x2BD) it shows
//! its skip panel and listens for skip_loading, whose button sets +0x2BF. The supervisor asks the controller each frame
//! whether it can finish (sub_140190004: +0x1A1 or +0x2BF) and closes the screen once it can; +0x1A1 is how the game
//! loads what nobody has to confirm (a fast travel). Every way the world being ready reaches the controller sets +0x2BD
//! first, so the question is where this goes in: once +0x2BD is set, +0x1A1 is set too, and the screen closes when its
//! world is ready, the way a fast travel's does.
//!
//! The controller's bytes are read and written through cvr_engine::code's checked accesses (the C++ one was guarded by
//! SEH): an unreadable controller is left alone, uncounted.
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};
use cvr_engine::code::{read, write};

extern "C" {
    static CyberpunkVR_SeamlessLoading: i32;
    static CyberpunkVR_DebugSeamlessLoads: AtomicU64;
}

const NO_CONFIRM: usize = 0x1A1;
const WORLD_LOADED: usize = 0x2BD;

/// The detour and its original's slot, registered by src/Hooks/SeamlessLoading.cpp with CVR_DETOUR at 0x190004.
type CanFinishFn = unsafe extern "C" fn(*mut c_void) -> bool;
#[no_mangle]
pub static mut cvr_seamless_can_finish_original: *mut c_void = core::ptr::null_mut();

fn no_confirm_once_loaded(controller: usize) {
    let (Some(loaded), Some(no_confirm)) = (read::<u8>(controller + WORLD_LOADED), read::<u8>(controller + NO_CONFIRM)) else {
        return;
    };
    if loaded == 0 || no_confirm != 0 || !write::<u8>(controller + NO_CONFIRM, 1) {
        return;
    }
    // SAFETY: the C++ half's export.
    unsafe { CyberpunkVR_DebugSeamlessLoads.fetch_add(1, Ordering::Relaxed) };
}

#[no_mangle]
pub unsafe extern "C" fn cvr_seamless_can_finish(controller: *mut c_void) -> bool {
    if CyberpunkVR_SeamlessLoading != 0 && !controller.is_null() {
        no_confirm_once_loaded(controller as usize);
    }
    let original: CanFinishFn = core::mem::transmute(cvr_seamless_can_finish_original);
    original(controller)
}
