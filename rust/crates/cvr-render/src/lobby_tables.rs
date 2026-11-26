//! The lobby's runtime tables built in Rust (formerly src/Lobby/MenuLobby.cpp's and src/Lobby/Intro.cpp's): the menu
//! lobby's exported switches and counters, the engine-stereo SDK's main-menu count, the menu's ink layer and the game
//! systems it asks; the intro's boot notice with the lobby's hum and the effects volume. The composite node's inputs
//! (`CyberpunkVR_LobbyInputs`) are defined here.
use core::ffi::{c_char, c_void};

use crate::intro_boot;
use crate::menu_lobby::{self, LobbyInput};
use crate::notice;
use crate::used_moved_exports_rest as x;

/// The resources the composite node resolved in the last frame it ran (formerly MenuLobby.cpp's).
#[no_mangle]
pub static mut CyberpunkVR_LobbyInputs: [LobbyInput; 16] =
    [LobbyInput { logical: 0, physical: 0, format: 0, width: 0, height: 0, frame: 0 }; 16];

unsafe extern "C" {
    /// The engine-stereo SDK's count of main menus up (engine_stereo_sdk.rs).
    fn cvr_engine_stereo_sdk_main_menus_up() -> i32;
    /// FindInkLayer (cvr-ffi engine_objects.rs): the ink layer of a native type, or null.
    fn cvr_find_ink_layer(type_name: u64) -> *mut c_void;
    /// The port's sound channels (cvr-rig port_sound.rs).
    fn cvr_sound_channel(name: *const c_char, sound: *const Sound, fade: f32) -> bool;
}

/// cvr::engine_stereo::MainMenuUp.
extern "C" fn main_menu_up() -> bool {
    // SAFETY: the SDK's counter.
    unsafe { cvr_engine_stereo_sdk_main_menus_up() > 0 }
}
/// cvr::engine_stereo::MainMenusUp.
extern "C" fn main_menus_up() -> i32 {
    // SAFETY: the SDK's counter.
    unsafe { cvr_engine_stereo_sdk_main_menus_up() }
}
/// FindInkLayer("inkMenuLayer"), a fault caught (the C++ __try).
extern "C" fn menu_layer() -> *mut u8 {
    let name = red4ext_abi::cname::CName::hashed("inkMenuLayer").0;
    // SAFETY: a lookup over the ink system's layers, run inside the SEH adapter.
    unsafe {
        cvr_engine::code::guarded_call4(cvr_find_ink_layer as *const () as usize, name as *mut c_void, core::ptr::null_mut(),
                                        core::ptr::null_mut(), core::ptr::null_mut())
    }
    .unwrap_or(core::ptr::null_mut())
    .cast()
}
/// The current game instance's gameuiGameSystemUI and gameIPlayerManager (null without them).
extern "C" fn game_systems(ui: *mut *mut u8, manager: *mut *mut u8) {
    // SAFETY: the caller's two slots.
    unsafe {
        *ui = red4ext_abi::engine::system_named("gameuiGameSystemUI").cast();
        *manager = red4ext_abi::engine::system_named("gameIPlayerManager").cast();
    }
}

pub static MENU_LOBBY: menu_lobby::Runtime = menu_lobby::Runtime {
    lobby: &raw mut x::CyberpunkVR_Lobby,
    compose: &raw const x::CyberpunkVR_LobbyCompose,
    compose_stats: (&raw const x::CyberpunkVR_LobbyComposeStats).cast(),
    compose_rejected: (&raw mut x::CyberpunkVR_LobbyComposeRejected).cast(),
    compose_rejected_node: &raw const x::CyberpunkVR_LobbyComposeRejectedNode,
    ambient: &raw mut x::CyberpunkVR_LobbyAmbient,
    loading_in_space: &raw const x::CyberpunkVR_LoadingInSpace,
    stats: (&raw const x::CyberpunkVR_LobbyStats).cast(),
    composite_node: &raw const x::CyberpunkVR_LobbyCompositeNode,
    inputs: (&raw mut CyberpunkVR_LobbyInputs).cast(),
    input_count: &raw const x::CyberpunkVR_LobbyInputCount,
    main_menu_up,
    main_menus_up,
    menu_layer,
    game_systems,
};

/// cvr::sound::Sound (include/Audio/PortSound.hpp).
#[repr(C)]
struct Sound {
    file: *const c_char,
    volume: f32,
    lowpass: f32,
    loop_from: f32,
    loop_to: f32,
    crossfade: f32,
}
const _: () = assert!(core::mem::size_of::<Sound>() == 32);

/// The lobby's own sound (tools/vr_lobby_assets/lobby_sounds.py): looped from 11 s to 20.6 s, the seam crossfaded over
/// 1.5 s, under a 900 Hz low-pass; it comes up over 1.5 s and goes over 2.5 s. Off: the channel goes silent.
pub extern "C" fn hum(on: bool, volume: f32) {
    let sound = if on {
        Sound {
            file: c"CyberpunkVRPort_Lobby/sounds/intro_loop.ogg".as_ptr(),
            volume: 0.55 * volume.clamp(0.0, 1.0),
            lowpass: 900.0,
            loop_from: 11.0,
            loop_to: 20.6,
            crossfade: 1.5,
        }
    } else {
        Sound { file: c"".as_ptr(), volume: 1.0, lowpass: 0.0, loop_from: -1.0, loop_to: 0.0, crossfade: 0.0 }
    };
    // SAFETY: a local sound and names that live for the call.
    unsafe { cvr_sound_channel(c"lobby_intro".as_ptr(), &sound, if on { 1.5 } else { 2.5 }) };
}

pub static INTRO_BOOT: intro_boot::Runtime = intro_boot::Runtime {
    main_menu_up,
    boot_over: notice::cvr_notice_boot_over,
    hum,
    effects_volume: notice::cvr_notice_effects_volume,
};
