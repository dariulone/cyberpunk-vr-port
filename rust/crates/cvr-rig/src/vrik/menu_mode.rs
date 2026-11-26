//! MenuMode (formerly the callback and stub of src/Hooks/MenuMode.cpp): the engine says its menu state changed, and the
//! whole VR path branches on it -- the submitted layer becomes a quad, the camera stops following the head, input is
//! rerouted. The lobby holds the menu up while it renders behind it. The startup observers of the menu source (an RTTI
//! dump behind CVR_STARTUP_DIAGNOSTICS) stay in MenuMode.cpp.
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

/// One menu transition, as the diagnostics record it (MenuMode.cpp MenuTransition).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MenuTransition {
    pub tick: u64,
    pub state: u64,
    pub previous: i32,
    pub requested: i32,
    pub native_before: i32,
    pub thread: u32,
}
const _: () = assert!(core::mem::size_of::<MenuTransition>() == 32);

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_menuModeValue: i32;
    static CyberpunkVR_MenuTransitionSeq: AtomicU32;
    static CyberpunkVR_MenuTransitionCount: AtomicU32;
    static mut CyberpunkVR_MenuTransitions: [MenuTransition; 64];
    fn cvr_cpp_lobby_holds_render(menu_state: usize) -> bool;
    fn cvr_cpp_verbose_log() -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
    fn GetCurrentThreadId() -> u32;
}

static TRANSITIONS: Mutex<()> = Mutex::new(());
/// The transition table is kept only when the stereo family manifest or the startup diagnostics are asked for.
fn diagnostics() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CVR_STEREO_FAMILY_MANIFEST").is_some() || std::env::var_os("CVR_STARTUP_DIAGNOSTICS").is_some())
}

/// rcx = rbx at the site (the menu state), edx = r8d (the mode about to be exchanged into state+0x128).
#[no_mangle]
pub unsafe extern "C" fn OnMenuModeHookCallback(menu_state: usize, new_mode: i32) {
    // SAFETY (whole body): the plugin's menu mode and diagnostics, a C++ service, the engine's state through a SEH read.
    unsafe {
        let mut new_mode = new_mode;
        if new_mode == 0 && cvr_cpp_lobby_holds_render(menu_state) {
            new_mode = 1;
        }
        let prev_mode = core::ptr::read_volatile(&raw const g_menuModeValue);
        core::ptr::write_volatile(&raw mut g_menuModeValue, new_mode);
        if diagnostics() && prev_mode != new_mode {
            let _guard = TRANSITIONS.lock().unwrap_or_else(|p| p.into_inner());
            CyberpunkVR_MenuTransitionSeq.fetch_add(1, Ordering::SeqCst);
            let index = CyberpunkVR_MenuTransitionCount.load(Ordering::SeqCst);
            if index < 64 {
                CyberpunkVR_MenuTransitions[index as usize] = MenuTransition {
                    tick: GetTickCount64(),
                    state: menu_state as u64,
                    previous: prev_mode,
                    requested: new_mode,
                    native_before: read::<i32>(menu_state.wrapping_add(0x128)).unwrap_or(-1),
                    thread: GetCurrentThreadId(),
                };
                CyberpunkVR_MenuTransitionCount.store(index + 1, Ordering::SeqCst);
            }
            CyberpunkVR_MenuTransitionSeq.fetch_add(1, Ordering::SeqCst);
        }
        if prev_mode != new_mode && cvr_cpp_verbose_log() != 0 {
            super::env::log(&format!("MenuMode hook: state={menu_state:016X} prev={prev_mode} new={new_mode}\n"));
        }
    }
}

/// xor ecx,ecx / mov edx,r8d / mov eax,r8d / xchg [rbx+128h],eax
const PATTERN: &[u8] = b"\x33\xC9\x41\x8B\xD0\x41\x8B\xC0\x87\x83\x28\x01\x00\x00";
const MASK: &[u8] = b"xxxxxxxxxxxxxx";

/// Installed from src/Hooks/MenuMode.cpp's registration (Boot, 70).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_menu_mode_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 384) else { return false };
    let mut c = Code::new(tramp);
    c.volatile_call(4, &[0x48, 0x89, 0xD9, 0x44, 0x89, 0xC2], OnMenuModeHookCallback as *const () as u64) // mov rcx,rbx / mov edx,r8d
        .bytes(PATTERN)
        .jmp(found + PATTERN.len());
    // SAFETY: fresh arena memory, then a site whose 14 bytes the stub reproduces.
    unsafe {
        c.commit();
        patch_jump(found, PATTERN.len(), tramp);
    }
    true
}
