//! The prompts' buttons (formerly src/Natives/InputPrompts.cpp; include/Hooks/PromptButtons.hpp): the part of the
//! controller icon atlas the game shows, mapped onto the physical button that produces that pad button through the
//! port's XInput merge -- the seat's mirrors while mounted (X shown as A, B as X), and the other hand's button when the
//! controllers' roles are traded left-handed (cvr-input handedness::mirrored_part).
//!
//! THE ICON OF A KEY, for every inkInputDisplayController and the hold indicators: 2.5 sub_1407885C4(mapping, {key,
//! hold}, out, device) writes the atlas part's name at out+24; the part is mapped after the lookup, so the key the game
//! binds stays the key it binds and only its picture is the physical button's. When the layout changes, the icon
//! mapping's listeners are told the device changed (sub_140798A10, as gameuiMenuGameController.RefreshInputIcons does)
//! from the main thread's game state (rust/crates/cvr-ffi/src/plugin.rs). VRPromptPart maps one part for the scripts.
use super::env::g_isInVehicle;
use super::xinput::InputControls;
use core::ffi::c_void;
use cvr_body::policy::vehicle_button_policy;
use cvr_engine::code::{image_base, read};
use cvr_engine::hooks::{install_chain, Detour};
use cvr_input::handedness::mirrored_part;
pub use cvr_input::prompts::{chord_part, Place, CHORD_DPAD, CHORD_GADGET, CHORD_ZOOM, GRIP_LEFT, GRIP_RIGHT, UNREACHABLE};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{RefCnt, WeakHandle};
use red4ext_abi::stack::StackFrame;
use std::collections::BTreeSet;
use std::sync::Mutex;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_VehicleDialogConfirmOnA: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn cvr_cpp_pose_vehicle_state() -> i32;
}

/// What the prompts show the buttons as.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct PromptLayout {
    /// Mounted with the seat's mirrors on: pad X <- A, pad B <- X (held).
    vehicle: bool,
    /// The controllers' roles traded.
    left_handed: bool,
    /// How far (xr_left_handed_bindings): 0 triggers, 1 + buttons, 2 + sticks.
    bindings: i32,
    /// Seated: the buttons mean something else there (chord_part).
    mounted: bool,
    /// The controllers have the pad's D-pad and bumpers; the thumbrest chord is mirrored.
    native: bool,
    mirrored: bool,
}
fn diagnostics() -> bool {
    // SAFETY: a plain read of the C++ half's flag.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn current_layout() -> PromptLayout {
    let mut controls = InputControls::default();
    // SAFETY: C++ services and the plugin's globals.
    unsafe {
        super::live_reads::input_controls(&mut controls);
        let seat = vehicle_button_policy(core::ptr::read_volatile(&raw const g_isInVehicle), cvr_cpp_pose_vehicle_state());
        PromptLayout {
            vehicle: seat.confirm_on_a && seat.exit_on_x && CyberpunkVR_VehicleDialogConfirmOnA != 0,
            left_handed: controls.left_handed != 0,
            bindings: controls.left_handed_bindings,
            mounted: core::ptr::read_volatile(&raw const g_isInVehicle),
            native: native_pad(),
            mirrored: chord_mirrored(),
        }
    }
}
/// The physical button's part for the pad button `part` names: "x", "x_hold", "x_disabled".
fn prompt_part(part: &str, layout: &PromptLayout) -> String {
    let (mut base, mut state) = (part, "");
    for suffix in ["_hold", "_disabled"] {
        if part.len() > suffix.len() && part.ends_with(suffix) {
            base = &part[..part.len() - suffix.len()];
            state = suffix;
            break;
        }
    }
    let mut physical = base;
    if layout.vehicle {
        if base == "x" {
            physical = "a"; // DialogConfirm / Choice1: A mirrors X while seated
        } else if base == "b" {
            physical = "x"; // ExitVehicle: a held X becomes B
        }
    }
    let out = format!("{physical}{state}");
    // The seat's mirrors are the merge's roles; left-handed, a role's button is the other hand's.
    if layout.left_handed {
        mirrored_part(&out, layout.bindings)
    } else {
        out
    }
}

type IconLookup = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8, i32) -> u8;
const ICON_LOOKUP_RVA: usize = 0x7885C4;
static mut ORIGINAL_LOOKUP: *mut c_void = core::ptr::null_mut();
static LOGGED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

// ---------------------------------------------------------------- the pad buttons a VR controller reaches by a chord
/// The shared slot that says the map is open.
const K_MAP_OPEN: usize = 81;
fn current_place() -> Place {
    // SAFETY: C++ services and the plugin's globals.
    unsafe {
        let menu_mode = cvr_cpp_menu_mode() != 0;
        let shared = cvr_cpp_shot_shared();
        let map = !shared.is_null() && core::ptr::read_volatile((shared as *const u32).add(K_MAP_OPEN)) != 0;
        let mounted = core::ptr::read_volatile(&raw const g_isInVehicle);
        let mut controls = InputControls::default();
        super::live_reads::input_controls(&mut controls);
        Place {
            menu: (menu_mode || cvr_cpp_in_braindance() || map) && !cvr_cpp_lobby_world_shown(),
            mounted,
            click_sprint: controls.sprint_mode == 1,
        }
    }
}
/// The controllers have the pad's D-pad and bumpers themselves: nothing stands in for them.
fn native_pad() -> bool {
    cvr_input::modifier::NATIVE_PAD.load(core::sync::atomic::Ordering::Relaxed)
}
fn chord_mirrored() -> bool {
    cvr_input::modifier::SCHEME.load(core::sync::atomic::Ordering::Relaxed) == cvr_input::modifier::SCHEME_THUMBREST
        && cvr_input::modifier::SIDE.load(core::sync::atomic::Ordering::Relaxed) == 1
}
extern "C" {
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_in_braindance() -> bool;
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_shot_shared() -> *mut f32;
}

std::thread_local! {
    /// The chord whose glyph this thread's icon lookup has just shown (inside an input display's refresh).
    static CHORD_SEEN: core::cell::Cell<u8> = const { core::cell::Cell::new(0) };
    /// The input action of the display being refreshed (a CName's hash, 0 outside a refresh).
    static DISPLAY_ACTION: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    /// The action of the hint whose displays are being set up (gameuiInputHintController: its displays get the keys
    /// alone).
    static HINT_ACTION: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    /// Inside an input display's refresh: only there is a chord's other half shown.
    static IN_DISPLAY: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// The displays a hint gave only keys, with the hint's action: (display, its key, the action's hash), the newest 256.
static HINT_DISPLAYS: Mutex<Vec<(usize, u64, u64)>> = Mutex::new(Vec::new());

/// An input display (inkInputDisplayController) whose icon is a chord's: the HUD's hints put the chord's other half
/// beside it (cvr-ffi mechanics::hud_hints).
pub struct ChordDisplay {
    pub controller: WeakHandle,
    /// CHORD_DPAD / CHORD_GADGET / ..., 0 once its icon is another button's again.
    pub kind: u8,
    /// Counts the refreshes: each one sets the game's own icon again.
    pub serial: u32,
    id: usize,
}
// SAFETY: a weak reference's counts are atomic; the instance is only reached through `lock`.
unsafe impl Send for ChordDisplay {}
static DISPLAYS: Mutex<Vec<ChordDisplay>> = Mutex::new(Vec::new());
static DISPLAYS_CHANGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// The object's own weak reference (ISerializable::ref at +8: the instance, its counts at +0x10), held -- only for an
/// object some handle already owns (its counts made), so nothing here can end its life or make it a second owner.
unsafe fn own_weak(instance: *mut c_void) -> WeakHandle {
    let base = instance as usize;
    let me = read::<usize>(base + 0x8).unwrap_or(0);
    let counts = read::<usize>(base + 0x10).unwrap_or(0) as *mut RefCnt;
    if me != base || counts.is_null() {
        return WeakHandle::null();
    }
    let Some(c) = counts.as_ref() else { return WeakHandle::null() };
    if c.strong.load(core::sync::atomic::Ordering::Acquire) == 0 {
        return WeakHandle::null();
    }
    c.weak.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    WeakHandle { instance, ref_count: counts }
}

fn note_display(controller: *mut c_void, kind: u8) {
    if controller.is_null() {
        return;
    }
    let id = controller as usize;
    let mut list = DISPLAYS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(d) = list.iter_mut().find(|d| d.id == id) {
        d.serial = d.serial.wrapping_add(1);
        if d.kind != kind || matches!(kind, GRIP_LEFT | GRIP_RIGHT | UNREACHABLE) {
            d.kind = kind;
            DISPLAYS_CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        // the same address may be a new object now: its own reference taken again
        if d.controller.lock().is_null() {
            // SAFETY: the live object of the refresh that is running.
            d.controller = unsafe { own_weak(controller) };
        }
        return;
    }
    if kind == 0 {
        return;
    }
    // SAFETY: as above.
    let weak = unsafe { own_weak(controller) };
    if weak.ref_count.is_null() {
        return;
    }
    if list.len() >= 512 {
        list.retain(|d| !d.controller.lock().is_null());
    }
    list.push(ChordDisplay { controller: weak, kind, serial: 1, id });
    DISPLAYS_CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// The input displays showing a chord's glyph (or showing one no longer), for the main thread: the list's copy (the
/// controller, the kind, its refreshes' count), and whether it changed since the last call. Dead ones are dropped.
pub fn chord_displays() -> (Vec<(WeakHandle, u8, u32)>, bool) {
    let mut list = DISPLAYS.lock().unwrap_or_else(|p| p.into_inner());
    list.retain(|d| !d.controller.lock().is_null());
    let changed = DISPLAYS_CHANGED.swap(false, core::sync::atomic::Ordering::Relaxed);
    (list.iter().map(|d| (d.controller.clone(), d.kind, d.serial)).collect(), changed)
}

type DisplayRefresh = unsafe extern "C" fn(*mut c_void);
/// inkInputDisplayController's refresh (2.5 sub_140787680): the key's icon looked up (sub_1407885C4) and set.
const DISPLAY_REFRESH_RVA: usize = 0x787680;
static mut ORIGINAL_REFRESH: *mut c_void = core::ptr::null_mut();

/// inkInputDisplayController's input action (a CName; none for a display of a plain key), and its key.
const DISPLAY_ACTION_OFFSET: usize = 0x128;
const DISPLAY_KEY_OFFSET: usize = 0x130;

/// The display's action: its own, else the one of the hint that gave it its key.
unsafe fn display_action(controller: *mut c_void) -> u64 {
    if controller.is_null() {
        return 0;
    }
    let at = controller as usize;
    let own = read::<u64>(at + DISPLAY_ACTION_OFFSET).unwrap_or(0);
    if own != 0 {
        return own;
    }
    let key = read::<u64>(at + DISPLAY_KEY_OFFSET).unwrap_or(0);
    let hint = HINT_ACTION.with(|c| c.get());
    let mut list = HINT_DISPLAYS.lock().unwrap_or_else(|p| p.into_inner());
    if hint != 0 {
        list.retain(|e| e.0 != at);
        if list.len() >= 256 {
            list.remove(0);
        }
        list.push((at, key, hint));
        return hint;
    }
    list.iter().find(|e| e.0 == at && e.1 == key).map_or(0, |e| e.2)
}

unsafe extern "C" fn display_refresh(controller: *mut c_void) {
    CHORD_SEEN.with(|c| c.set(0));
    let action = display_action(controller);
    DISPLAY_ACTION.with(|c| c.set(action));
    IN_DISPLAY.with(|c| c.set(true));
    core::mem::transmute::<*mut c_void, DisplayRefresh>(ORIGINAL_REFRESH)(controller);
    IN_DISPLAY.with(|c| c.set(false));
    DISPLAY_ACTION.with(|c| c.set(0));
    let kind = CHORD_SEEN.with(|c| c.replace(0));
    note_display(controller, kind);
}

/// gameuiInputHintController's displays set up (2.5 sub_1406F8EF8): each gets a key of the hint's action (+0xC8), and the
/// action itself only for a hold.
const HINT_SETUP_RVA: usize = 0x6F8EF8;
const HINT_ACTION_OFFSET: usize = 0xC8;
static mut ORIGINAL_HINT: *mut c_void = core::ptr::null_mut();
unsafe extern "C" fn hint_setup(controller: *mut c_void) -> usize {
    let action = if controller.is_null() { 0 } else { read::<u64>(controller as usize + HINT_ACTION_OFFSET).unwrap_or(0) };
    let before = HINT_ACTION.with(|c| c.replace(action));
    let r = core::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> usize>(ORIGINAL_HINT)(controller);
    HINT_ACTION.with(|c| c.set(before));
    r
}

/// A text's inline button icons looked up (2.5 sub_140C75844; its action at +0x198).
const TEXT_ICONS_RVA: usize = 0xC75844;
const TEXT_ACTION_OFFSET: usize = 0x198;
static mut ORIGINAL_TEXT: *mut c_void = core::ptr::null_mut();
unsafe extern "C" fn text_icons(text: *mut c_void) -> usize {
    let action = if text.is_null() { 0 } else { read::<u64>(text as usize + TEXT_ACTION_OFFSET).unwrap_or(0) };
    let before = DISPLAY_ACTION.with(|c| c.replace(action));
    let r = core::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> usize>(ORIGINAL_TEXT)(text);
    DISPLAY_ACTION.with(|c| c.set(before));
    r
}

unsafe extern "C" fn icon_lookup(mapping: *mut c_void, key: *mut c_void, out: *mut u8, device: i32) -> u8 {
    let found = core::mem::transmute::<*mut c_void, IconLookup>(ORIGINAL_LOOKUP)(mapping, key, out, device);
    if found == 0 || out.is_null() || mapping.is_null() {
        return found;
    }
    // A device of 0 is the mapping's current one (+504); 1 is the keyboard, whose keys are no hand's.
    let kind = if device != 0 {
        device
    } else {
        *((mapping as *const u8).add(504) as *const i32)
    };
    if kind == 1 {
        return found;
    }
    let part = &mut *(out.add(24) as *mut CName);
    // the pad buttons the controllers reach another way: the physical control's own glyph
    let text = part.text();
    if !text.is_empty() && !native_pad() {
        let action = CName(DISPLAY_ACTION.with(|c| c.get()));
        let named = action.text();
        // outside an input display (a text's inline icon) a chord's other half has no place: the pad's own button stays
        let own = IN_DISPLAY.with(|c| c.get());
        if let Some((mut mapped, chord)) = chord_part(text, named, chord_mirrored(), current_place()).filter(|m| own || m.1 == 0) {
            if chord == 0 {
                let layout = current_layout();
                if layout.left_handed {
                    mapped = mirrored_part(&mapped, layout.bindings);
                }
            }
            if diagnostics() {
                let mut logged = LOGGED.lock().unwrap_or_else(|p| p.into_inner());
                if logged.len() < 256 && logged.insert(format!("{named}:{text}>{mapped}:{chord}")) {
                    super::env::log(&format!("[prompt-icon] {named}: {text} -> {mapped} (chord {chord})\n"));
                }
            }
            if mapped != text {
                *part = CName::hashed(&mapped);
            }
            CHORD_SEEN.with(|c| c.set(chord));
            return found;
        }
        if diagnostics() && !named.is_empty() {
            let mut logged = LOGGED.lock().unwrap_or_else(|p| p.into_inner());
            if logged.len() < 256 && logged.insert(format!("{named}:{text}")) {
                super::env::log(&format!("[prompt-icon] {named}: {text}\n"));
            }
        }
    }
    let layout = current_layout();
    if !layout.vehicle && !layout.left_handed {
        return found;
    }
    let text = part.text();
    if text.is_empty() {
        return found;
    }
    let mapped = prompt_part(text, &layout);
    if mapped == text {
        return found;
    }
    if diagnostics() {
        // Each mapping once a session: what a prompt asked for and what it was shown.
        let mut logged = LOGGED.lock().unwrap_or_else(|p| p.into_inner());
        if logged.len() < 64 && logged.insert(format!("{text}>{mapped}")) {
            super::env::log(&format!(
                "[prompt-icon] {text} -> {mapped} (vehicle={} left-handed={})\n",
                layout.vehicle as i32, layout.left_handed as i32
            ));
        }
    }
    *part = CName::hashed(&mapped);
    found
}

/// The icon lookup (src/Natives/InputPrompts.cpp registers it at Boot, order 84).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_prompt_icons_install() -> bool {
    let detours = [
        Detour {
            name: "input icon lookup sub_1407885C4",
            target: ICON_LOOKUP_RVA,
            absolute: false,
            detour: icon_lookup as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_LOOKUP),
            expect: Some("48 89 5C 24 08 48 89 74 24 10 44 89 4C 24 20 55"),
        },
        Detour {
            name: "input display refresh sub_140787680",
            target: DISPLAY_REFRESH_RVA,
            absolute: false,
            detour: display_refresh as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_REFRESH),
            expect: Some("48 89 5C 24 18 55 56 57 41 54 41 55 41 56 41 57"),
        },
    ];
    let installed = install_chain("prompt-icons", &detours).is_ok();
    // the hints' and the texts' actions: without them their icons go by the key alone
    let named = [
        Detour {
            name: "input hint setup sub_1406F8EF8",
            target: HINT_SETUP_RVA,
            absolute: false,
            detour: hint_setup as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_HINT),
            expect: Some("48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 4C 89 60 20 55 41 56 41 57"),
        },
        Detour {
            name: "text input icons sub_140C75844",
            target: TEXT_ICONS_RVA,
            absolute: false,
            detour: text_icons as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_TEXT),
            expect: Some("48 89 5C 24 10 48 89 74 24 18 55 57 41 56 48 8D 6C 24 90"),
        },
    ];
    if installed && install_chain("prompt-actions", &named).is_err() {
        super::env::log("[prompt-icon] the hints' actions are not hooked: their icons go by the key\n");
    }
    installed
}

/// RefreshPromptIcons: the icon mapping's listeners told the device changed, so every icon widget looks its key up again
/// (the caller is the main thread).
unsafe fn refresh_prompt_icons() -> bool {
    const NOTIFY_RVA: usize = 0x798A10;
    const ENGINE_RVA: usize = 0x3C2B390;
    let base = image_base();
    if !cvr_engine::game_build::supported() || (*(&raw const ORIGINAL_LOOKUP)).is_null() {
        return false;
    }
    let pattern = cvr_engine::code::Pattern::parse("48 89 5C 24 10 48 89 74 24 18 55 57");
    if !pattern.is_some_and(|p| cvr_engine::code::expect(base + NOTIFY_RVA, &p)) {
        return false;
    }
    let engine = read::<usize>(base + ENGINE_RVA).unwrap_or(0);
    let mapping = if engine != 0 { read::<usize>(engine + 528).unwrap_or(0) } else { 0 };
    if mapping == 0 {
        return false;
    }
    type Notify = unsafe extern "C" fn(usize, u32, u32) -> i64;
    core::mem::transmute::<usize, Notify>(base + NOTIFY_RVA)(mapping + 48, 0, read::<u32>(mapping + 504).unwrap_or(0));
    true
}
/// The layout the icons on screen were looked up with.
static SHOWN: Mutex<PromptLayout> = Mutex::new(PromptLayout {
    vehicle: false,
    left_handed: false,
    bindings: 2,
    mounted: false,
    native: false,
    mirrored: false,
});
/// UpdatePromptIcons (game state Running, update): the icons looked up again when the layout changes.
#[no_mangle]
pub unsafe extern "C" fn cvr_input_update_prompt_icons(_app: *mut c_void) -> bool {
    let now = current_layout();
    let mut shown = SHOWN.lock().unwrap_or_else(|p| p.into_inner());
    if *shown == now {
        return false;
    }
    if refresh_prompt_icons() {
        if diagnostics() {
            super::env::log(&format!(
                "[prompt-icon] refreshed (vehicle={} left-handed={} mounted={} native={} mirrored={})\n",
                now.vehicle as i32, now.left_handed as i32, now.mounted as i32, now.native as i32, now.mirrored as i32
            ));
        }
        *shown = now;
    }
    false
}

/// VRPromptPart(part) -> CName: the icon part of the physical button that presses the pad button `part` names.
#[no_mangle]
pub unsafe extern "C" fn VRPromptPart(_this: *mut c_void, frame: *mut StackFrame, out: *mut CName, _unused: i64) {
    let mut part = CName::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut part);
        frame.finish_params();
    }
    let Some(out) = out.as_mut() else { return };
    *out = part;
    let text = part.text();
    if text.is_empty() {
        return;
    }
    if !native_pad() {
        if let Some((mapped, 0)) = chord_part(text, "", chord_mirrored(), current_place()) {
            *out = CName::hashed(&mapped);
            return;
        }
    }
    let mapped = prompt_part(text, &current_layout());
    if mapped != text {
        *out = CName::hashed(&mapped);
    }
}
