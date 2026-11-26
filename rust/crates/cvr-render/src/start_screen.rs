//! NO START SCREEN (formerly src/Lobby/StartScreen.cpp): the game goes from its boot straight to the main menu, as
//! -skipStartScreen has it, without the argument. The start screen ("press any key") is a flat picture waiting for a
//! key; in the headset the game starts in the lobby, with its own intro. Verified against game 2.5 with IDA headless
//! (2026-10-28).
//!
//! The pre-game session state decides on entering (sub_14018D6F4, its vtable slot 45) between the two: the skip flag
//! (byte_143C2B4E0) is set when skipStartScreen is among the launch arguments or the state's own parameters, cleared
//! when forceStartScreen is, and then the state goes on to the main menu or listens for any key behind the
//! engagement screen. Coming back from a game the state is given skipStartScreen itself, so the flag is how the game
//! asks the question every time. It is set before the state looks at it; forceStartScreen still wins.
//!
//! The export (CyberpunkVR_SkipStartScreen) is Rust's; the detour registers from engine_detours.rs.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use cvr_engine::code;

pub const PRE_GAME_ENTER_RVA: usize = 0x18D6F4;
const SKIP_START_SCREEN: usize = 0x3C2B4E0;

/// The trampoline slot (filled by the install pass; registered from engine_detours.rs).
#[no_mangle]
pub static cvr_start_screen_pre_game_enter_original: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// CyberpunkVR_SkipStartScreen (used_moved_exports_rest.rs's; the reference build keeps its own detour in the .cpp).
#[cfg(feature = "rust-build")]
fn skip_wanted() -> bool {
    // SAFETY: the Rust-defined export, read as the .cpp read it.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(crate::used_moved_exports_rest::CyberpunkVR_SkipStartScreen)) != 0 }
}
#[cfg(not(feature = "rust-build"))]
fn skip_wanted() -> bool {
    false
}

/// Detour_PreGameEnter: the skip flag set before the state looks at it.
///
/// # Safety
/// Called by the engine in place of the pre-game session's enter.
#[no_mangle]
pub unsafe extern "C" fn cvr_start_screen_pre_game_enter(
    state: *mut c_void,
    app: *mut c_void,
    params: *mut c_void,
    launch: *mut c_void,
) -> i64 {
    let base = code::image_base();
    if skip_wanted() && base != 0 {
        code::write::<u8>(base + SKIP_START_SCREEN, 1);
    }
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> i64 =
            core::mem::transmute(cvr_start_screen_pre_game_enter_original.load(Ordering::Acquire));
        f(state, app, params, launch)
    }
}
