//! The notice at the boot (formerly the boot half of src/Lobby/Intro.cpp; Lobby/Intro.hpp says what it is). The
//! launch's first loading screen -- the legal notice the game plays before the main menu -- hangs in the dark before
//! the player; when the main menu comes up its screen closes, and the notice stays until the lobby's intro takes it
//! over (VRIntroTakeBootNotice), or fades by itself: at most 30 s after its screen closed, not after the curtain opened
//! without it, never without the lobby.
//!
//! The intro's picture, its heading, the notice's language and the sound stay in the C++ half, which attaches the
//! questions this needs.
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;

use cvr_host::log;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::menu_lobby;

/// The notice once its screen has closed, with nothing taking it over: it stays at most this long (ms).
const BOOT_HOLD: u64 = 30000;
/// It fades out (s) when nothing takes it: the fade the intro would give it.
const BOOT_FADE: f32 = 0.8;

/// What the C++ half attaches.
#[repr(C)]
pub struct Runtime {
    /// cvr::engine_stereo::MainMenuUp (the engine-stereo SDK's).
    pub main_menu_up: extern "C" fn() -> bool,
    /// cvr::lobby::notice::BootOver: the boot is over, the notice's bink may go.
    pub boot_over: extern "C" fn(),
    /// cvr::lobby::intro::Hum and the game's effects volume (cvr::lobby::notice::EffectsVolume).
    pub hum: extern "C" fn(on: bool, volume: f32),
    pub effects_volume: extern "C" fn() -> f32,
}
// SAFETY: functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

/// The plugin's table is built in Rust (lobby_tables.rs).
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const crate::lobby_tables::INTRO_BOOT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

#[no_mangle]
pub extern "C" fn cvr_intro_boot_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

/// cvr::lobby::intro::BootFrame: the notice this frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BootFrame {
    pub shown: bool,
    pub live: bool,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BootState {
    up: bool,
    held: bool,
    fading: bool,
    shut: bool,
    closed: u64,
    fade_from: u64,
    fade: f32,
}
const FRESH: BootState = BootState {
    up: false,
    held: false,
    fading: false,
    shut: false,
    closed: 0,
    fade_from: 0,
    fade: BOOT_FADE,
};
static BOOT: Mutex<BootState> = Mutex::new(FRESH);
static BOOTED: AtomicBool = AtomicBool::new(false);
static SEEN: AtomicBool = AtomicBool::new(false);

fn boot_state() -> std::sync::MutexGuard<'static, BootState> {
    BOOT.lock().unwrap_or_else(|p| p.into_inner())
}

fn now() -> u64 {
    // SAFETY: no arguments.
    unsafe { GetTickCount64() }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// One frame of the boot's notice: its screen closing as the main menu comes up, the fade when nothing takes it.
pub fn boot() -> BootFrame {
    let Some(rt) = runtime() else {
        return BootFrame::default();
    };
    let mut hushed = false;
    let mut frame = BootFrame::default();
    if !BOOTED.load(Ordering::Relaxed) && (rt.main_menu_up)() {
        BOOTED.store(true, Ordering::Relaxed);
        (rt.boot_over)();
    }
    {
        let mut b = boot_state();
        if !b.held {
            return frame;
        }
        let now = now();
        if b.up && (rt.main_menu_up)() {
            // The main menu came up: the boot's loading screen is gone.
            b.up = false;
            b.closed = now;
            log!("[lobby-intro] the boot's screen closed: the main menu is up\n");
        }
        if !b.fading {
            let curtain = menu_lobby::curtain();
            if curtain >= 1.0 {
                b.shut = true;
            }
            if !b.up
                && (!menu_lobby::enabled()
                    || now.wrapping_sub(b.closed) > BOOT_HOLD
                    || (b.shut && curtain < 1.0))
            {
                b.fading = true;
                b.fade_from = now;
                b.fade = BOOT_FADE;
                hushed = true;
                log!("[lobby-intro] the boot's notice ends: nothing took it over\n");
            }
        }
        frame.opacity = 1.0;
        if b.fading {
            let k = now.wrapping_sub(b.fade_from) as f32 / (1000.0 * b.fade);
            if k >= 1.0 {
                b.held = false;
                return BootFrame::default();
            }
            frame.opacity = 1.0 - smooth(k);
        }
        frame.shown = true;
        frame.live = b.up;
    }
    if hushed {
        (rt.hum)(false, 0.0);
    }
    frame
}

/// The loading layer's state as engine stereo reads it on each tick: only the launch's first screen, before the main
/// menu, is the boot's (a later one is a loading screen like any).
pub fn note_loading_layer(active: bool) {
    let Some(rt) = runtime() else { return };
    if !active || SEEN.swap(true, Ordering::Relaxed) || (rt.main_menu_up)() {
        return;
    }
    {
        let mut b = boot_state();
        *b = FRESH;
        b.up = true;
        b.held = true;
    }
    log!("[lobby-intro] the boot's screen is up: its notice hangs in the dark\n");
    if menu_lobby::enabled() {
        (rt.hum)(true, (rt.effects_volume)());
    }
}

/// VRIntroTakeBootNotice(fade): the intro takes the notice over, fading it its own way.
pub fn take_boot_notice(fade: f32) -> bool {
    let mut b = boot_state();
    if !b.held {
        return false;
    }
    if !b.fading {
        b.fading = true;
        b.fade_from = now();
        b.fade = fade.clamp(0.05, 10.0);
        log!("[lobby-intro] the intro takes the boot's notice over\n");
    }
    true
}

/// Boot(), into the caller's frame (a struct return is left to neither compiler's rules).
///
/// # Safety
/// `out` is null or a BootFrame.
#[no_mangle]
pub unsafe extern "C" fn cvr_intro_boot_frame(out: *mut BootFrame) {
    let frame = boot();
    if !out.is_null() {
        // SAFETY: the caller's.
        unsafe { *out = frame };
    }
}
#[no_mangle]
pub extern "C" fn cvr_intro_boot_note_loading_layer(active: bool) {
    note_loading_layer(active)
}
#[no_mangle]
pub extern "C" fn cvr_intro_boot_take_notice(fade: f32) -> bool {
    take_boot_notice(fade)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fade_is_smooth() {
        assert_eq!(smooth(-1.0), 0.0);
        assert_eq!(smooth(0.5), 0.5);
        assert_eq!(smooth(2.0), 1.0);
        assert!(smooth(0.25) < 0.25);
    }

    #[test]
    fn nothing_held_without_the_cpp_half() {
        assert_eq!(boot(), BootFrame::default());
        // Not held: the intro has nothing to take.
        assert!(!take_boot_notice(1.0));
    }
}
