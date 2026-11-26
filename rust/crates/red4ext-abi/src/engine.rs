//! The game engine and its game instance (`GameEngine.hpp`): CGameEngine::Get (a relocated pointer to the engine),
//! its framework (+0x348) with the game instance (+0x10) and the world's runtime scene (+0x18), and the instance's
//! systems by type (GameInstance::GetSystem, vtable +0x08). Read on the game thread: the framework and the instance
//! change with a session (main menu, load), so nothing returned here is kept across frames.
use crate::hashes;
use crate::reloc::Reloc;
use crate::rtti::{vfunc, RttiSystem};
use core::ffi::c_void;

const ENGINE_FRAMEWORK: usize = 0x348;
const FRAMEWORK_GAME_INSTANCE: usize = 0x10;
const FRAMEWORK_RUNTIME_SCENE: usize = 0x18;

static ENGINE: Reloc = Reloc::new(hashes::CGameEngine);

/// CGameEngine::Get(): the engine, or null before it exists.
pub fn game_engine() -> *mut c_void {
    match ENGINE.get() {
        // SAFETY: the relocated address is the engine's static pointer.
        Some(at) => unsafe { *(at as *const *mut c_void) },
        None => core::ptr::null_mut(),
    }
}
/// engine->framework (CGameFramework), or null.
pub fn framework() -> *mut c_void {
    let engine = game_engine();
    if engine.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: a live engine's field.
    unsafe { *((engine as *const u8).add(ENGINE_FRAMEWORK) as *const *mut c_void) }
}
/// framework->gameInstance, or null outside a game instance.
pub fn game_instance() -> *mut c_void {
    let framework = framework();
    if framework.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: a live framework's field.
    unsafe { *((framework as *const u8).add(FRAMEWORK_GAME_INSTANCE) as *const *mut c_void) }
}
/// framework->unk18: the world's runtime scene (0 without a framework).
pub fn world_runtime_scene() -> usize {
    let framework = framework();
    if framework.is_null() {
        return 0;
    }
    // SAFETY: a live framework's field.
    unsafe { *((framework as *const u8).add(FRAMEWORK_RUNTIME_SCENE) as *const usize) }
}
/// GameInstance::GetSystem(type): the system the instance holds for an RTTI type (a class), or null.
///
/// # Safety
/// `game` is a live game instance and `ty` an RTTI type pointer (or null).
pub unsafe fn get_system(game: *mut c_void, ty: *const c_void) -> *mut c_void {
    if game.is_null() || ty.is_null() {
        return core::ptr::null_mut();
    }
    type F = unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
    // SAFETY: GameInstance's slot 1, as the SDK declares it.
    unsafe { vfunc::<F>(game, 0x08)(game, ty) }
}
/// The current game instance's system of the named class (RTTI name), or null.
pub fn system_named(class: &str) -> *mut c_void {
    let game = game_instance();
    let Some(rtti) = RttiSystem::get() else { return core::ptr::null_mut() };
    let Some(ty) = rtti.get_class(crate::cname::CName::hashed(class)) else {
        return core::ptr::null_mut();
    };
    // SAFETY: the instance read just now; the class from RTTI.
    unsafe { get_system(game, ty as *const _ as *const c_void) }
}
