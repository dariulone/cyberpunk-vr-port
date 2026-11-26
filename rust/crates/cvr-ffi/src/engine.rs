//! The game build gate for the C++ half: `cvr::game_build` (src/Core/GameBuild.cpp) forwards here, so the C++ hook
//! registries and the Rust hook manager refuse the same builds for the same reasons.
use cvr_engine::{game_build, trampoline};
use std::ffi::{c_char, CString};
use std::sync::OnceLock;

#[no_mangle]
pub extern "C" fn cvr_game_build_supported() -> bool {
    game_build::supported()
}

#[no_mangle]
pub extern "C" fn cvr_game_build_overridden() -> bool {
    game_build::overridden()
}

#[no_mangle]
pub extern "C" fn cvr_game_build_unmapped(rva: usize) -> bool {
    game_build::unmapped(rva)
}

/// "2.5 / 6AD55DAF": the build the port's addresses belong to.
#[no_mangle]
pub extern "C" fn cvr_game_build_expected() -> *const c_char {
    static S: OnceLock<CString> = OnceLock::new();
    S.get_or_init(|| CString::new(game_build::identity().expected.clone()).unwrap_or_default()).as_ptr()
}

/// The running build, the same way.
#[no_mangle]
pub extern "C" fn cvr_game_build_running() -> *const c_char {
    static S: OnceLock<CString> = OnceLock::new();
    S.get_or_init(|| CString::new(game_build::identity().running.clone()).unwrap_or_default()).as_ptr()
}

/// The plugin's one trampoline arena (cvr_engine::trampoline), for the C++ half's AllocateTrampoline: null when it is
/// exhausted or could not be placed within rel32 of 	arget.
#[no_mangle]
pub extern "C" fn cvr_engine_allocate_trampoline(target: *mut core::ffi::c_void, size: usize) -> *mut core::ffi::c_void {
    trampoline::allocate(target as usize, size).map_or(core::ptr::null_mut(), |a| a as *mut core::ffi::c_void)
}
