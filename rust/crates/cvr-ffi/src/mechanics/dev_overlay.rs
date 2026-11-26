//! The CET overlay without the Lua host (CVR_NO_LUA): the port's CET mods' tuning windows are Rust's, drawn by the
//! plugin's overlay where the host drew its windows (cvr_render::imgui_overlay::attach_host: the Lua host's place in
//! the overlay's table taken by these four functions), as the host did it (src/Lua/Host.cpp):
//!
//! - the overlay key, "cet.overlay_key" in config\lua_bindings.json (CET's packing; Home when the file has none),
//!   opens and closes it as the keys go up; with Cyber Engine Tweaks itself installed its own key does too;
//! - while it is open ImGui has the mouse and keys, and of the raw input only the releases reach the game (a key or
//!   button held when it opened is let go);
//! - each window is drawn while it is open, on the render thread: they read and write the mechanics' own state (its
//!   locks), and what touches a game object is asked of the main thread's tick.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether the windows are open.
pub(crate) static OPEN: AtomicBool = AtomicBool::new(false);
/// The overlay key's combination (CET's packing) and the keys of it held now (bit i: key i).
static KEY: AtomicU64 = AtomicU64::new(0);
static HELD: AtomicU64 = AtomicU64::new(0);
/// Cyber Engine Tweaks' own overlay key, when it is installed (its config's, read once).
static CET_KEY: AtomicU64 = AtomicU64::new(0);

const VK_HOME: u64 = 0x24;
const WM_KEYDOWN: u32 = 0x100;
const WM_KEYUP: u32 = 0x101;
const WM_SYSKEYDOWN: u32 = 0x104;
const WM_SYSKEYUP: u32 = 0x105;

/// At the plugin's start, with no Lua host: the overlay's host table is ours.
pub(crate) fn install() {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Config, "lua_bindings.json");
    let json: serde_json::Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut key = json.get("cet").and_then(|c| c.get("overlay_key")).and_then(serde_json::Value::as_u64).filter(|&k| k != 0).unwrap_or(VK_HOME << 48);
    let cet = cet_overlay_key();
    if cet != 0 && key == cet {
        // never both overlays on one key (cet_migration moves a kept key; this is a CET bound to ours since)
        key = crate::cet_migration::key_beside_cet(cet);
        cvr_host::log!("[overlay] CET's overlay has the same key: the port's windows take {:X?}\n", super::hotkeys::decode(key));
    }
    KEY.store(key, Ordering::Relaxed);
    CET_KEY.store(cet, Ordering::Relaxed);
    cvr_render::imgui_overlay::attach_host(draw, on_wndproc, open, pass_raw_input);
    // CVR_OVERLAY_OPEN (a development switch): open from the start -- a test session cannot press the key
    if std::env::var_os("CVR_OVERLAY_OPEN").is_some() {
        OPEN.store(true, Ordering::Relaxed);
    }
    cvr_host::log!("[overlay] the CET mods' windows are Rust's (no Lua host): the overlay key {:X?}\n", super::hotkeys::decode(key));
}

/// Cyber Engine Tweaks' overlay key when it is installed (bindings.json beside cyber_engine_tweaks.asi: "cet" ->
/// "overlay_key"), else 0.
fn cet_overlay_key() -> u64 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }
    let name: Vec<u16> = "cyber_engine_tweaks.asi\0".encode_utf16().collect();
    // SAFETY: a plain Win32 query.
    if unsafe { GetModuleHandleW(name.as_ptr()) }.is_null() {
        return 0;
    }
    let Some(root) = cvr_host::paths::game_root() else { return 0 };
    let file = root.join("bin").join("x64").join("plugins").join("cyber_engine_tweaks").join("bindings.json");
    let json: serde_json::Value = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    json.get("cet").and_then(|c| c.get("overlay_key")).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

fn toggle() {
    let now = !OPEN.load(Ordering::Relaxed);
    OPEN.store(now, Ordering::Relaxed);
    for word in &PRESSED_HERE {
        word.store(0, Ordering::Relaxed);
    }
    if !now {
        // the mods' onOverlayClose: what is saved when the windows close
        super::dev_windows::closed();
    }
    cvr_host::log!("[overlay] the CET mods' windows {} (from Rust)\n", if now { "open" } else { "closed" });
}

extern "C" fn open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

extern "C" fn draw(width: f32, height: f32) {
    if OPEN.load(Ordering::Relaxed) {
        let _ = std::panic::catch_unwind(|| super::dev_windows::draw(width, height));
    }
}

/// The host's bindings' window-message hook: the overlay key, fired as its keys go up after being held together.
extern "C" fn on_wndproc(_hwnd: *mut c_void, msg: u32, wparam: usize, _lparam: isize) {
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
    if !down && !up {
        return;
    }
    let keys = super::hotkeys::decode(KEY.load(Ordering::Relaxed));
    let Some(i) = keys.iter().position(|&k| usize::from(k) == wparam) else { return };
    let all = (1u64 << keys.len()) - 1;
    let held = HELD.load(Ordering::Relaxed);
    if down {
        HELD.store(held | (1 << i), Ordering::Relaxed);
    } else {
        HELD.store(held & !(1 << i), Ordering::Relaxed);
        if held == all {
            toggle();
        }
    }
}

/// The keys pressed while the windows were open (virtual keys, a bit each): the game saw no press of them.
static PRESSED_HERE: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
/// While open, which raw input reaches the game: a mouse button's release, the release of a key pressed before the
/// windows opened (so none stays held in the game) -- not of one pressed while they were: typed into a text field (the
/// weapon list's filter) the game took the releases alone as its taps and drew the arm cyberware (2026-11-25). Cyber
/// Engine Tweaks' own overlay key passes whole and toggles these windows with its own.
extern "C" fn pass_raw_input(lparam: isize) -> bool {
    #[repr(C)]
    struct Header {
        kind: u32,
        size: u32,
        device: *mut c_void,
        wparam: usize,
    }
    #[repr(C)]
    struct Raw {
        header: Header,
        data: [u16; 12],
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetRawInputData(input: isize, command: u32, data: *mut c_void, size: *mut u32, header_size: u32) -> u32;
    }
    const RID_INPUT: u32 = 0x1000_0003;
    const RIM_TYPEMOUSE: u32 = 0;
    const RIM_TYPEKEYBOARD: u32 = 1;
    // SAFETY: a zeroed RAWINPUT-sized buffer the call fills.
    let mut raw: Raw = unsafe { core::mem::zeroed() };
    let mut size = core::mem::size_of::<Raw>() as u32;
    // SAFETY: the window's own raw input handle.
    let got = unsafe { GetRawInputData(lparam, RID_INPUT, (&mut raw as *mut Raw).cast(), &mut size, core::mem::size_of::<Header>() as u32) };
    if got == u32::MAX {
        return true; // bigger than a key or a mouse event: another device's
    }
    match raw.header.kind {
        RIM_TYPEMOUSE => {
            // RAWMOUSE: usFlags, then (after 2 bytes of padding) usButtonFlags
            const RELEASES: u16 = 0x0002 | 0x0008 | 0x0020 | 0x0080 | 0x0200;
            raw.data[2] & RELEASES != 0
        }
        RIM_TYPEKEYBOARD => {
            // RAWKEYBOARD: MakeCode, Flags, Reserved, VKey
            let released = raw.data[1] & 0x01 != 0;
            let pressed = raw.data[3];
            let cet = super::hotkeys::decode(CET_KEY.load(Ordering::Relaxed));
            if cet.contains(&pressed) {
                if released && cet.last() == Some(&pressed) {
                    toggle();
                }
                return true;
            }
            let (word, bit) = (&PRESSED_HERE[usize::from(pressed >> 6) & 3], 1u64 << (pressed & 63));
            if !released {
                word.fetch_or(bit, Ordering::Relaxed);
                return false;
            }
            word.fetch_and(!bit, Ordering::Relaxed) & bit == 0
        }
        _ => true,
    }
}
