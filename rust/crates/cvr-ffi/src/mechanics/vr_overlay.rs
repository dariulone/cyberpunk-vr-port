//! mods/cet/CyberpunkVRPort_Stereo/modules/vr_overlay.lua (the VR menu: it never pauses gameplay nor enters the native
//! menu state; a chosen game action opens ESC and is handed to that item's own handler). Every frame the VR menu's
//! command (VROverlayCommand's): 1 opens it -- its actions read off the native pause menu's list when it is up
//! (mainListVert's items: label, event, action), else the default set (Resume, Save, Load when there are saves,
//! Settings, Credits, Main menu, Quit) -- and closes a native pause presentation in the way; 2 closes it; 100..115
//! picks an action: Continue closes, any other opens the native pause menu (OnOpenPauseMenu) and activates the matching
//! item there (OnMenuItemActivated). The menu is ready only in game, with no native pause menu or its background up;
//! the game's state and the labels go to the VR menu (VROverlayMenu's work), whose visibility is the menu's "active".
//! A failure closes it (reported once until it changes).
use crate::script_call::{call_handle, method, property, Value};
use core::ffi::c_char;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    Opening,
    Native,
    Panel,
}

/// One action of the VR menu: from the native list (its label, event and action) or the default set (event or kind).
#[derive(Clone, Debug, Default)]
struct Action {
    label: String,
    native_label: Option<String>,
    event: Option<String>,
    /// The PauseMenuAction (its name and value), as CET's tostring "PauseMenuAction : <name> (<value>)" carried it.
    action: Option<(String, i64)>,
    kind: Option<String>,
}
/// One item of the native pause menu's list.
struct NativeItem {
    index: i32,
    controller: Handle,
    label: String,
    event: String,
    action: (String, i64),
}

struct State {
    error: Option<String>,
    phase: Phase,
    waiting: i32,
    actions: Vec<Action>,
    pending: Option<Action>,
    requested_close: bool,
}
static STATE: Mutex<State> =
    Mutex::new(State { error: None, phase: Phase::Idle, waiting: 0, actions: Vec::new(), pending: None, requested_close: false });

type R<T> = Result<T, String>;

/// The mod's englishLabel().
fn english_label(event: Option<&str>, kind: Option<&str>) -> String {
    let named = match event {
        Some("OnClosePauseMenu") => Some("CONTINUE"),
        Some("OnSwitchToLoadGame") => Some("LOAD GAME"),
        Some("OnSwitchToSettings") => Some("SETTINGS"),
        Some("OnCreditsPicker") => Some("CREDITS"),
        Some("OnBuyGame") => Some("BUY GAME"),
        _ => None,
    };
    if let Some(named) = named {
        return named.into();
    }
    match kind {
        Some("Save") => "SAVE GAME",
        Some("QuickSave") => "QUICKSAVE",
        Some("ExitToMainMenu") => "MAIN MENU",
        Some("ExitGame") => "QUIT GAME",
        _ => "GAME ACTION",
    }
    .into()
}

/// The mod's controllers(): the menu layer's pause menu, in-game menu, and whether the pause background is up.
fn controllers() -> R<(Handle, Handle, bool)> {
    let layer = crate::engine_objects::layer_wrapper(CName::hashed("inkMenuLayer").0);
    let (mut pause, mut ingame, mut background) = (Handle::null(), Handle::null(), false);
    if layer.is_null() {
        return Ok((pause, ingame, background));
    }
    for c in super::hud_panel::game_controllers(&layer)? {
        if c.is_null() {
            continue;
        }
        // SAFETY: the controller, held.
        let name = CName(unsafe { method::<u64>(c.instance, "GetClassName", &[]) }.ok_or("GetClassName failed")?);
        if name == CName::hashed("PauseMenuGameController") {
            pause = c.clone();
        }
        if name == CName::hashed("gameuiInGameMenuGameController") {
            ingame = c.clone();
        }
        if name == CName::hashed("PauseMenuBackgroundGameController") {
            background = true;
        }
    }
    Ok((pause, ingame, background))
}

/// A scripted object's property of any plain kind, in place.
fn field(object: &Handle, name: &str) -> R<*mut core::ffi::c_void> {
    // SAFETY: the object, held; its property found by RTTI.
    unsafe { property(object.instance, name) }.ok_or_else(|| format!("no {name}"))
}

/// An enum property's value, read at the enum's own size.
fn enum_field(object: &Handle, name: &str) -> R<i64> {
    // SAFETY: the object, held; its property and type found by RTTI, read at the type's size.
    unsafe {
        let class = red4ext_abi::rtti::object_class(object.instance).ok_or("no class")?;
        let p = class.property_named(name).ok_or_else(|| format!("no {name}"))?;
        let ty = p.ty.as_ref().ok_or("no type")?;
        let at = p.value_ptr::<u8>(object.instance);
        Ok(match ty.size() {
            1 => *at as i8 as i64,
            2 => *(at as *const i16) as i64,
            4 => *(at as *const i32) as i64,
            8 => *(at as *const i64),
            n => return Err(format!("{name}: an enum of {n} bytes")),
        })
    }
}

/// The mod's nativeItems(): the pause menu's list items (16 at most) with data.
fn native_items(pause: &Handle) -> R<Vec<NativeItem>> {
    let mut result = Vec::new();
    // SAFETY: the pause menu, held; each widget, controller and data held by its handle.
    unsafe {
        let root = call_handle(pause.instance, "GetRootWidget", &[]).ok_or("GetRootWidget failed")?;
        if root.is_null() {
            return Ok(result);
        }
        let list = call_handle(root.instance, "GetWidgetByPathName", &[Value::U64(CName::hashed("mainListVert").0)])
            .ok_or("GetWidgetByPathName failed")?;
        if list.is_null() {
            return Ok(result);
        }
        let n = method::<i32>(list.instance, "GetNumChildren", &[]).ok_or("GetNumChildren failed")?.min(16);
        for i in 0..n {
            let widget = call_handle(list.instance, "GetWidgetByIndex", &[Value::Int32(i)]).ok_or("GetWidgetByIndex failed")?;
            if widget.is_null() {
                continue;
            }
            let item = call_handle(widget.instance, "GetController", &[]).ok_or("GetController failed")?;
            if item.is_null() {
                continue;
            }
            let data = call_handle(item.instance, "GetData", &[]).ok_or("GetData failed")?;
            if data.is_null() {
                continue;
            }
            let label = (*(field(&data, "label")? as *const CString)).as_str().to_string();
            let event = CName(*(field(&data, "eventName")? as *const u64));
            let event = if event.0 == 0 { "None".to_string() } else { event.text().to_string() };
            let value = enum_field(&data, "action")?;
            let action = (crate::script_call::enum_name("PauseMenuAction", value).unwrap_or_default(), value);
            result.push(NativeItem { index: i, controller: item, label, event, action });
        }
    }
    Ok(result)
}

/// The mod's defaultItems(): the menu's own set when no native pause list is there to read.
fn default_items(s: &mut State) -> R<()> {
    let handler = super::requests_handler()?;
    if handler.is_null() {
        return Err("no system requests handler".into());
    }
    let specs: [(Option<&str>, Option<&str>); 7] = [
        (Some("OnClosePauseMenu"), None),
        (None, Some("Save")),
        (Some("OnSwitchToLoadGame"), None),
        (Some("OnSwitchToSettings"), None),
        (Some("OnCreditsPicker"), None),
        (None, Some("ExitToMainMenu")),
        (None, Some("ExitGame")),
    ];
    s.actions.clear();
    for (event, kind) in specs {
        if event != Some("OnSwitchToLoadGame")
            // SAFETY: the handler, held.
            || unsafe { method::<i32>(handler.instance, "RequestSavesCountSync", &[]) }.ok_or("RequestSavesCountSync failed")? > 0
        {
            s.actions.push(Action {
                label: english_label(event, kind),
                event: event.map(str::to_string),
                kind: kind.map(str::to_string),
                ..Default::default()
            });
        }
    }
    Ok(())
}

/// The mod's labels(): one line each.
fn labels(s: &State) -> String {
    s.actions.iter().map(|a| a.label.replace(['\r', '\n'], " ")).collect::<Vec<_>>().join("\n")
}

/// The mod's same(): the pending action and a native item.
fn same(a: &Action, b: &NativeItem) -> bool {
    if let Some(event) = a.event.as_deref().filter(|e| *e != "None") {
        return event == b.event;
    }
    if let Some(action) = &a.action {
        return *action == b.action && a.native_label.as_deref() == Some(b.label.as_str());
    }
    a.kind.as_deref().is_some_and(|k| !b.action.0.is_empty() && b.action.0 == k)
}

/// VROverlayMenu's work: the game's state and the labels to the VR menu; whether it is visible.
fn bridge_update(ready: bool, in_game: bool, labels: &str) -> bool {
    let labels = std::ffi::CString::new(labels.split('\0').next().unwrap_or_default()).unwrap_or_default();
    // SAFETY: a NUL-terminated text for the call.
    unsafe { cvr_body::vr_overlay::cvr_vrui_bridge_update(ready, in_game, labels.as_ptr() as *const c_char) };
    let visible = cvr_body::vr_overlay::cvr_vrui_visible();
    super::hud_clearance::OVERLAY_ACTIVE.store(visible, Ordering::Relaxed);
    visible
}

/// `controller:SpawnMenuInstanceEvent(event)`.
fn spawn_event(controller: &Handle, event: &str) -> R<()> {
    // SAFETY: the controller, held.
    unsafe { method::<()>(controller.instance, "SpawnMenuInstanceEvent", &[Value::U64(CName::hashed(event).0)]) }
        .ok_or_else(|| "SpawnMenuInstanceEvent failed".into())
}

fn step(s: &mut State) -> R<()> {
    let command = cvr_body::vr_overlay::cvr_vrui_poll_command();
    if command == 0 && s.phase == Phase::Idle {
        return Ok(());
    }
    let (pause, ingame, background) = controllers()?;
    if command == 1 {
        s.phase = Phase::Opening;
        s.waiting = 180;
        s.requested_close = false;
        s.pending = None;
        if !pause.is_null() {
            s.actions = native_items(&pause)?
                .into_iter()
                .map(|a| Action {
                    label: english_label(Some(a.event.as_str()), Some(a.action.0.as_str())),
                    native_label: Some(a.label),
                    event: Some(a.event),
                    action: Some(a.action),
                    kind: None,
                })
                .collect();
        } else if s.actions.is_empty() {
            default_items(s)?;
        }
    } else if command == 2 {
        s.phase = Phase::Idle;
        s.pending = None;
        bridge_update(false, false, &labels(s));
        super::hud_clearance::OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
        return Ok(());
    } else if (100..116).contains(&command) {
        s.pending = s.actions.get((command - 100) as usize).cloned();
        if s.pending.as_ref().is_some_and(|p| p.event.as_deref() == Some("OnClosePauseMenu")) {
            s.phase = Phase::Idle;
            s.pending = None;
            bridge_update(false, false, &labels(s));
            super::hud_clearance::OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
            return Ok(());
        }
        if s.pending.is_some() && !ingame.is_null() {
            s.phase = Phase::Native;
            s.waiting = 180;
            if pause.is_null() {
                spawn_event(&ingame, "OnOpenPauseMenu")?;
            }
        } else {
            s.phase = Phase::Idle;
        }
    }
    match s.phase {
        Phase::Opening => {
            s.waiting -= 1;
            if !pause.is_null() || background {
                if !ingame.is_null() && !s.requested_close {
                    spawn_event(&ingame, "OnClosePauseMenu")?;
                    s.requested_close = true;
                }
            } else if !ingame.is_null() {
                s.phase = Phase::Panel;
            }
            if s.waiting <= 0 {
                s.phase = Phase::Idle;
                return Err("Could not leave the native pause presentation".into());
            }
        }
        Phase::Native => {
            s.waiting -= 1;
            if !pause.is_null() && s.pending.is_some() {
                let pending = s.pending.clone().expect("checked above");
                for a in native_items(&pause)? {
                    if same(&pending, &a) {
                        // SAFETY: the pause menu and the item's controller, held.
                        unsafe { method::<bool>(pause.instance, "OnMenuItemActivated", &[Value::Int32(a.index), Value::Handle(&a.controller)]) };
                        s.pending = None;
                        break;
                    }
                }
                // Without a matching item the real menu stays visible.
                s.phase = Phase::Idle;
                s.pending = None;
            } else if s.waiting <= 0 {
                s.phase = Phase::Idle;
                s.pending = None;
            }
        }
        Phase::Panel if !pause.is_null() || background || ingame.is_null() => s.phase = Phase::Idle,
        _ => {}
    }
    let ready = s.phase == Phase::Panel && pause.is_null() && !background;
    let active = bridge_update(ready, !ingame.is_null(), &labels(s));
    if s.phase == Phase::Panel && !active {
        s.phase = Phase::Idle;
    }
    Ok(())
}

/// The mod's tick: every frame.
pub(in crate::mechanics) fn tick() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    match step(&mut s) {
        Ok(()) => s.error = None,
        Err(e) => {
            bridge_update(false, false, "");
            super::hud_clearance::OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
            s.phase = Phase::Idle;
            if s.error.as_deref() != Some(e.as_str()) {
                log!("[Stereo.VR menu] {e}\n");
                s.error = Some(e);
            }
        }
    }
}

/// The mod's shutdown.
pub(in crate::mechanics) fn shutdown() {
    bridge_update(false, false, "");
    super::hud_clearance::OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
    STATE.lock().unwrap_or_else(|p| p.into_inner()).phase = Phase::Idle;
}
