//! The CET mods' hotkeys without the Lua host (CVR_NO_LUA): the four registerHotkey bindings of the port's mods --
//! QuickBoot's toggle, Stereo's VRCAM re-read, VRIK's ToggleVRHands and LogVRDiag -- read from the host's own bindings
//! file (config\lua_bindings.json: mod -> id -> the key combination, 0 for none, as CET packs it: up to four 16-bit
//! virtual-key codes, the first in the top bits) and fired as CET fires a hotkey: on the release of a combination held
//! whole, the game's window in front. The host's overlay key (cet.overlay_key) has no overlay to open here.
use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether the Lua host is off (plugin.rs): only then are the hotkeys Rust's.
pub(crate) static ACTIVE: AtomicBool = AtomicBool::new(false);

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetForegroundWindow() -> *mut core::ffi::c_void;
    fn GetWindowThreadProcessId(window: *mut core::ffi::c_void, process: *mut u32) -> u32;
}

/// The hotkeys: (mod, id).
const KEYS: [(&str, &str); 4] = [
    ("CyberpunkVRPort_QuickBoot", "cyberpunkvr_quickboot_toggle"),
    ("CyberpunkVRPort_Stereo", "vrcam_reload_selection"),
    ("CyberpunkVRPort_VRIK", "ToggleVRHands"),
    ("CyberpunkVRPort_VRIK", "LogVRDiag"),
];
struct Bound {
    /// The combination's keys (CET's VKCodeBindDecoded without its empty slots).
    keys: Vec<u16>,
    /// The whole combination is held.
    held: bool,
}
static BOUND: Mutex<Option<Vec<Bound>>> = Mutex::new(None);
/// A combination was held behind another window (logged once).
static BEHIND: AtomicBool = AtomicBool::new(false);
/// VRIK's vrTrackingEnabled, the ToggleVRHands state.
static TRACKING: AtomicBool = AtomicBool::new(false);

/// CET's VKBindings::DecodeVKCodeBind: the keys of a packed combination, the first in the top 16 bits.
pub(super) fn decode(code: u64) -> Vec<u16> {
    (0..4).map(|i| ((code >> (48 - 16 * i)) & 0xFFFF) as u16).filter(|&k| k != 0).collect()
}

fn load() -> Vec<Bound> {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Config, "lua_bindings.json");
    let json: serde_json::Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let bound: Vec<Bound> = KEYS
        .iter()
        .map(|(m, id)| Bound { keys: decode(json.get(m).and_then(|v| v.get(id)).and_then(serde_json::Value::as_u64).unwrap_or(0)), held: false })
        .collect();
    let named: Vec<String> = KEYS.iter().zip(&bound).filter(|(_, b)| !b.keys.is_empty()).map(|((_, id), b)| format!("{id} {:X?}", b.keys)).collect();
    cvr_host::log!("[hotkeys] the CET mods' hotkeys from Rust (no Lua host): {}\n", if named.is_empty() { "none bound".to_string() } else { named.join(", ") });
    bound
}

fn game_in_front() -> bool {
    let mut pid = 0u32;
    // SAFETY: plain Win32 queries.
    unsafe {
        let w = GetForegroundWindow();
        !w.is_null() && GetWindowThreadProcessId(w, &mut pid) != 0 && pid == std::process::id()
    }
}
fn down(key: u16) -> bool {
    // SAFETY: a plain Win32 query.
    unsafe { GetAsyncKeyState(i32::from(key)) as u16 & 0x8000 != 0 }
}

fn fire(i: usize) {
    use crate::script_call::Value;
    cvr_host::log!("[hotkeys] {} fired (from Rust)
", KEYS[i].1);
    match i {
        0 => super::quickboot::toggle(),
        1 => {
            super::belt::native::<()>("VRVrcamReload", &[]);
        }
        2 => {
            let on = !TRACKING.load(Ordering::Relaxed);
            TRACKING.store(on, Ordering::Relaxed);
            super::belt::native::<()>("VRIKTrackingEnabled", &[Value::Bool(on)]);
            if on {
                super::belt::native::<i32>("InstallVRAnimPoseHook", &[]);
                super::belt::native::<i32>("ArmVRAnimPosePlayer", &[]);
                super::belt::native::<i32>("SetVRBindMode", &[Value::Int32(4)]);
            } else {
                super::belt::native::<i32>("SetVRBindMode", &[Value::Int32(0)]);
            }
            cvr_host::log!("[hotkeys] ToggleVRHands: VR hands {} (from Rust)\n", if on { "on" } else { "off" });
        }
        3 => {
            super::belt::native::<()>("VRIKLogDiag", &[]);
        }
        _ => {}
    }
}

/// Every frame while the Lua host is off.
pub(super) fn tick() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let mut guard = BOUND.lock().unwrap_or_else(|p| p.into_inner());
    let bound = guard.get_or_insert_with(load);
    // CVR_HOTKEYS_ANY_WINDOW (a development switch): fired whatever window is in front -- a test session's own
    // windows keep the game behind them
    static ANY_WINDOW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let front = *ANY_WINDOW.get_or_init(|| std::env::var_os("CVR_HOTKEYS_ANY_WINDOW").is_some()) || game_in_front();
    let mut fired = Vec::new();
    for (i, b) in bound.iter_mut().enumerate() {
        if b.keys.is_empty() {
            continue;
        }
        let held = b.keys.iter().all(|&k| down(k));
        if held && !front && !BEHIND.swap(true, Ordering::Relaxed) {
            cvr_host::log!("[hotkeys] {} held with another window in front: not fired (from Rust)
", KEYS[i].1);
        }
        let all = front && held;
        if b.held && !all {
            fired.push(i);
        }
        b.held = all;
    }
    drop(guard);
    for i in fired {
        fire(i);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_as_cet() {
        assert_eq!(super::decode(10_133_099_161_583_616), vec![0x24]);
        assert_eq!(super::decode(0x0011_0041_0000_0000), vec![0x11, 0x41]);
        assert!(super::decode(0).is_empty());
    }
}
