//! The HUD's element layout (formerly src/Runtimes/HudLayout.cpp; include/Runtimes/HudLayout.hpp keeps the element
//! table's C++ view and its inline geometry, the C++ names forward here): the player's edit of each HUD element --
//! offset in percent of the canvas, scale, opacity, shown or hidden -- and each element's status as the HUD panel last
//! saw it, the `hud_element_<key>` lines of vrport.ini read with the C runtime's sscanf and written with its fprintf.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::crt::{self, Arg};
use core::ffi::{c_char, c_void};
use std::sync::Mutex;

/// One element: its HUD entry name, its label in the settings panel and its ini key.
pub struct Element {
    pub name: &'static str,
    pub label: &'static str,
    pub key: &'static str,
}
const fn e(name: &'static str, label: &'static str, key: &'static str) -> Element {
    Element { name, label, key }
}
/// cvr::hud::Elements, in its order.
pub const ELEMENTS: [Element; 41] = [
    e("player_health_bar", "Health", "health"),
    e("activity_log", "Activity log", "activity"),
    e("bottom_hints", "Button hints", "hints"),
    e("ammo_counter", "Weapon / ammo", "ammo"),
    e("warning", "Warning", "warning"),
    e("cursor_device", "Device cursor", "device_cursor"),
    e("dpad_hint", "Quick slots", "quick_slots"),
    e("boss_healthbar", "Boss health", "boss"),
    e("hud_progress_bar", "Progress bar", "progress"),
    e("oxygenbar", "Oxygen", "oxygen"),
    e("minimap", "Minimap", "minimap"),
    e("wanted_bar", "Wanted level", "wanted"),
    e("minimap_q305", "Alternate minimap", "minimap_alternate"),
    e(
        "vehicle_summon_notification",
        "Vehicle summon",
        "vehicle_summon",
    ),
    e("quest_list", "Quest tracker", "quest"),
    e("vehicle scan widget", "Vehicle scan", "vehicle_scan"),
    e("cooldown", "Cooldown", "cooldown"),
    e("input_hint", "Input prompt", "input_prompt"),
    e("zone alert notification", "Zone alert", "zone_alert"),
    e("staminabar", "Stamina", "stamina"),
    e("crouch_indicator", "Crouch indicator", "crouch"),
    e("militech warning", "Militech warning", "militech"),
    e("new_phone", "Phone / messages", "phone"),
    e("driver_combat_hud", "Vehicle combat", "vehicle_combat"),
    e("car hud", "Vehicle dashboard", "vehicle"),
    e("dpad_hint_consumables", "Consumables", "consumables"),
    e("songbird_phone", "Songbird audio call", "songbird_audio"),
    e("songbird_holocall", "Holocall", "holocall"),
    e("hud_courier_bar", "Courier progress", "courier"),
    e("radio_custom", "Radio", "radio"),
    e("subtitles", "Subtitles", "subtitles"),
    e("interactions_root", "Dialogs / interactions", "dialogs"),
    e(
        "Notifications_Journal",
        "Journal / area notifications",
        "journal_notifications",
    ),
    e("hud tank", "Basilisk HUD", "basilisk"),
    e("q304_dossier", "Identity dossier", "identity_dossier"),
    e(
        "q304_imprint_active_indicator",
        "Identity imprint",
        "identity_imprint",
    ),
    e(
        "briefing_sequence_player",
        "Story briefings / imprint",
        "briefing_sequence",
    ),
    e("scanner_details", "Scanner data", "scanner_data"),
    e("camera_hud", "Surveillance camera HUD", "surveillance"),
    e("scanner", "Scanner reticle / progress", "scanner_reticle"),
    // main's 0.2.4
    e("items_notification_queue", "Money / item notifications", "item_notifications"),
];
pub const COUNT: usize = ELEMENTS.len();

/// cvr::hud::ElementSettings: x, y in percent of the canvas's width and height.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementSettings {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub opacity: f32,
    pub visible: i32,
}
impl ElementSettings {
    pub const DEFAULT: ElementSettings = ElementSettings {
        x: 0.0,
        y: 0.0,
        scale: 1.0,
        opacity: 1.0,
        visible: 1,
    };
}
impl Default for ElementSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}
/// cvr::hud::ElementStatus.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElementStatus {
    pub available: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
const _: () = assert!(
    core::mem::size_of::<ElementSettings>() == 20 && core::mem::size_of::<ElementStatus>() == 20
);
const STATUS_NONE: ElementStatus = ElementStatus {
    available: false,
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

struct State {
    settings: [ElementSettings; COUNT],
    status: [ElementStatus; COUNT],
}
static STATE: Mutex<State> = Mutex::new(State {
    settings: [ElementSettings::DEFAULT; COUNT],
    status: [STATUS_NONE; COUNT],
});
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// cvr::hud::NameHash: FNV-1a 64.
pub const fn name_hash(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut h: u64 = 14695981039346656037;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(1099511628211);
        i += 1;
    }
    h
}
/// cvr::hud::ElementIndex: the element of a HUD entry, -1 for none.
pub fn element_index(name: u64) -> i32 {
    ELEMENTS
        .iter()
        .position(|e| name_hash(e.name) == name)
        .map_or(-1, |i| i as i32)
}
/// cvr::hud::Sanitize.
pub fn sanitize(mut s: ElementSettings) -> ElementSettings {
    s.x = if s.x.is_finite() {
        crt::cpp_clamp(s.x, -100.0, 100.0)
    } else {
        0.0
    };
    s.y = if s.y.is_finite() {
        crt::cpp_clamp(s.y, -100.0, 100.0)
    } else {
        0.0
    };
    s.scale = if s.scale.is_finite() {
        crt::cpp_clamp(s.scale, 0.1, 3.0)
    } else {
        1.0
    };
    s.opacity = if s.opacity.is_finite() {
        crt::cpp_clamp(s.opacity, 0.0, 1.0)
    } else {
        1.0
    };
    s.visible = (s.visible != 0) as i32;
    s
}

pub fn settings() -> [ElementSettings; COUNT] {
    state().settings
}
pub fn set_settings(value: &[ElementSettings; COUNT]) {
    let mut s = state();
    for (out, v) in s.settings.iter_mut().zip(value) {
        *out = sanitize(*v);
    }
}
pub fn status() -> [ElementStatus; COUNT] {
    state().status
}
pub fn set_status(value: &[ElementStatus; COUNT]) {
    state().status = *value;
}

/// cvr::hud::ParseLayoutSetting: a `hud_element_<key> = x , y , scale , opacity , visible` line into `settings` (the
/// key matched as a prefix, blanks before '='); true when the key is an element's, whether or not the values parse.
///
/// # Safety
/// `line` is a C string.
pub unsafe fn parse(line: *const c_char, settings: &mut [ElementSettings; COUNT]) -> bool {
    for (i, element) in ELEMENTS.iter().enumerate() {
        let key = format!("hud_element_{}", element.key);
        let key = key.as_bytes();
        // strncmp(line, key, n): equal through the key's n bytes.
        let mut matches = true;
        for (j, &k) in key.iter().enumerate() {
            if *line.add(j) as u8 != k {
                matches = false;
                break;
            }
        }
        if !matches {
            continue;
        }
        let mut p = line.add(key.len());
        while *p == b' ' as c_char || *p == b'\t' as c_char {
            p = p.add(1);
        }
        if *p != b'=' as c_char {
            continue;
        }
        let mut s = ElementSettings::DEFAULT;
        let fields = crt::scan(
            p.add(1),
            c"%f , %f , %f , %f , %d",
            &[
                crt::ptr(&mut s.x),
                crt::ptr(&mut s.y),
                crt::ptr(&mut s.scale),
                crt::ptr(&mut s.opacity),
                crt::ptr(&mut s.visible),
            ],
        );
        if fields == 5 {
            settings[i] = sanitize(s);
        }
        return true;
    }
    false
}
/// cvr::hud::SaveLayoutSettings: every element's line, as fprintf writes it.
///
/// # Safety
/// `file` is a FILE the C++ half opened for writing.
pub unsafe fn save(file: *mut c_void, settings: &[ElementSettings; COUNT]) {
    for (element, value) in ELEMENTS.iter().zip(settings) {
        let s = sanitize(*value);
        let key = std::ffi::CString::new(element.key).unwrap_or_default();
        let line = crt::format(
            c"hud_element_%s=%.3f,%.3f,%.4f,%.4f,%d\n",
            &[
                Arg::S(key.as_ptr()),
                Arg::F(s.x as f64),
                Arg::F(s.y as f64),
                Arg::F(s.scale as f64),
                Arg::F(s.opacity as f64),
                Arg::I(s.visible),
            ],
        );
        crt::fputs_to(file, &line);
    }
}

// ---- the C names (src/Runtimes/HudLayout.cpp forwards to these)

/// # Safety
/// `out` holds `count` settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_settings(out: *mut ElementSettings, count: usize) -> usize {
    let s = settings();
    let n = count.min(COUNT);
    core::ptr::copy_nonoverlapping(s.as_ptr(), out, n);
    n
}
/// # Safety
/// `value` holds COUNT settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_set_settings(value: *const ElementSettings) {
    set_settings(&*(value as *const [ElementSettings; COUNT]));
}
/// # Safety
/// `out` holds `count` statuses.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_status(out: *mut ElementStatus, count: usize) -> usize {
    let s = status();
    let n = count.min(COUNT);
    core::ptr::copy_nonoverlapping(s.as_ptr(), out, n);
    n
}
/// # Safety
/// `value` holds COUNT statuses.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_set_status(value: *const ElementStatus) {
    set_status(&*(value as *const [ElementStatus; COUNT]));
}
/// # Safety
/// `line` is a C string, `settings` holds COUNT settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_parse(
    line: *const c_char,
    settings: *mut ElementSettings,
) -> bool {
    !line.is_null() && parse(line, &mut *(settings as *mut [ElementSettings; COUNT]))
}
/// # Safety
/// `file` is an open FILE, `settings` holds COUNT settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_layout_save(file: *mut c_void, settings: *const ElementSettings) {
    save(file, &*(settings as *const [ElementSettings; COUNT]));
}
#[no_mangle]
pub extern "C" fn cvr_hud_layout_element_index(name: u64) -> i32 {
    element_index(name)
}
/// The element table's size, for the C++ half's assert.
#[no_mangle]
pub extern "C" fn cvr_hud_layout_count() -> usize {
    COUNT
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::CStr;

    fn parsed(line: &CStr) -> Option<[ElementSettings; COUNT]> {
        let mut s = [ElementSettings::DEFAULT; COUNT];
        // SAFETY: a C string.
        unsafe { parse(line.as_ptr(), &mut s) }.then_some(s)
    }
    #[test]
    fn lines_parse_as_the_cpp_did() {
        let s = parsed(c"hud_element_minimap = 5, -3.5 ,1.5, 0.25,0").unwrap();
        assert_eq!(
            s[10],
            ElementSettings {
                x: 5.0,
                y: -3.5,
                scale: 1.5,
                opacity: 0.25,
                visible: 0
            }
        );
        // The alternate minimap's key starts with the minimap's: the first key matches, then fails at '_', and the
        // search goes on to the right one.
        let s = parsed(c"hud_element_minimap_alternate=1,2,3,0.5,1").unwrap();
        assert_eq!(
            s[12],
            ElementSettings {
                x: 1.0,
                y: 2.0,
                scale: 3.0,
                opacity: 0.5,
                visible: 1
            }
        );
        assert_eq!(s[10], ElementSettings::DEFAULT);
        // Sanitised: out of range clamped.
        let s = parsed(c"hud_element_health=500,-500,9,2,7").unwrap();
        assert_eq!(
            s[0],
            ElementSettings {
                x: 100.0,
                y: -100.0,
                scale: 3.0,
                opacity: 1.0,
                visible: 1
            }
        );
        // A known key with bad values is taken (true) and leaves the settings alone.
        assert_eq!(
            parsed(c"hud_element_quest=1,2").unwrap()[14],
            ElementSettings::DEFAULT
        );
        assert!(parsed(c"hud_element_nothing=1,2,3,4,5").is_none());
        assert!(parsed(c"HUD_ELEMENT_HEALTH=1,2,3,0.5,1").is_none());
    }
    #[test]
    fn element_names_find_their_index() {
        assert_eq!(element_index(name_hash("scanner")), 39);
        assert_eq!(element_index(name_hash("player_health_bar")), 0);
        assert_eq!(element_index(name_hash("nope")), -1);
    }
}
