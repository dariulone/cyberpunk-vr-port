//! mods/cet/CyberpunkVRPort_QuickBoot without the Lua host (CVR_NO_LUA): the development helper that, when the first
//! main menu appears (the single-player menu initialised and its save list ready, or 2.5's first-launch pitch screen
//! in a tool-owned launch), resumes the newest save once -- or the save a tool-owned run names -- and then reports the
//! world's readiness for the launch tools (tools/quickboot_tests/launch_to_save.py).
//!
//! The files are the mod's own, in its folder (lua/CyberpunkVRPort_QuickBoot): quickboot.json (enabled, delay),
//! quickboot_state.txt (the phase), quickboot_status.json (the detailed report), quickboot_run.json (a tool-owned
//! run: its id, requireNativeReady, allowResume -- the tool releases the resume once the native menu is idle --,
//! expiresAt and an optional save name). The phases and the report's fields are the mod's: ready, armed:*,
//! awaiting_native_menu, resuming, resumed:*, failed:*, loading, awaiting_continue (the initial loading screen's
//! skip panel up over a valid world), world_ready (30 stable frames and a second of a live, unpaused, unloading
//! world). The menu items the mod listed from AddMenuItem are not reported (the resume uses the handler's
//! LoadLastCheckpoint, as the menu's Continue does).
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_rt as r;
use crate::script_call::Value;
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use serde_json::{json, Map, Value as J};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The Lua host is not installed (plugin.rs) and the mod's folder is there (installed by the launch tools, never by a
/// release package): this port of the mod runs.
pub(crate) static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Boot {
    started: bool,
    enabled: bool,
    delay: f64,
    armed: bool,
    fired: bool,
    elapsed: f64,
    run_id: String,
    phase: String,
    stable_frames: u32,
    loading_frames: u32,
    ready_seconds: f64,
    loading_seconds: f64,
    status_elapsed: f64,
    menu_initialized: bool,
    saves_ready: bool,
    menu: Option<WeakHandle>,
    resume_method: Option<String>,
    require_native_ready: bool,
    resume_allowed: bool,
    gate_elapsed: f64,
    save_name: Option<String>,
    save_names: Vec<String>,
    via_checkpoint: bool,
}
// SAFETY: the weak handle is used on the game's main thread only.
unsafe impl Send for Boot {}
static BOOT: Mutex<Option<Boot>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Boot) -> T) -> T {
    let mut g = BOOT.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(|| Boot { enabled: true, delay: 0.5, run_id: "manual".into(), phase: "starting".into(), ..Boot::default() }))
}
fn folder() -> PathBuf {
    cvr_host::paths::module_folder().unwrap_or_default().join("lua").join("CyberpunkVRPort_QuickBoot")
}
/// The mod installed: its folder with its init.lua (tools/quickboot_tests/launch_to_save.py puts it there).
pub(crate) fn installed() -> bool {
    folder().join("init.lua").is_file()
}
fn read_json(name: &str) -> Option<Map<String, J>> {
    let text = std::fs::read_to_string(folder().join(name)).ok()?;
    match serde_json::from_str::<J>(&text).ok()? {
        J::Object(m) => Some(m),
        _ => None,
    }
}
fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}
fn write_status(b: &mut Boot, phase: &str) {
    b.phase = phase.to_string();
    let _ = std::fs::write(folder().join("quickboot_state.txt"), format!("{phase}\n"));
}
fn write_detailed(b: &Boot, extra: Map<String, J>) {
    let mut v = Map::new();
    v.insert("runId".into(), json!(b.run_id));
    v.insert("phase".into(), json!(b.phase));
    v.insert("enabled".into(), json!(b.enabled));
    v.insert("fired".into(), json!(b.fired));
    v.insert("stableFrames".into(), json!(b.stable_frames));
    v.insert("loadingFrames".into(), json!(b.loading_frames));
    v.insert("continueReady".into(), json!(false));
    v.insert("updatedAt".into(), json!(now_secs()));
    v.insert("menuItems".into(), json!([]));
    if let Some(m) = &b.resume_method {
        v.insert("resumeMethod".into(), json!(m));
    }
    if let Some(s) = &b.save_name {
        v.insert("save".into(), json!(s));
        if !b.fired {
            v.insert("saves".into(), json!(b.save_names));
        }
    }
    let pre_game = extra.get("preGame").and_then(J::as_bool) == Some(true);
    for (k, x) in extra {
        v.insert(k, x);
    }
    if b.fired && b.require_native_ready && pre_game {
        v.insert("loadConfirmation".into(), save_confirmation());
    }
    if b.fired {
        v.insert("menuEvidence".into(), menu_evidence());
    }
    if let Ok(text) = serde_json::to_string(&J::Object(v)) {
        let _ = std::fs::write(folder().join("quickboot_status.json"), text);
    }
}
fn handler() -> Handle {
    super::requests_handler().ok().filter(|h| !h.is_null()).unwrap_or_else(Handle::null)
}
fn ink_layer(name: &str) -> Handle {
    super::lobby_spawn::native_handle("VRInkLayer", &[Value::U64(CName::hashed(name).0)])
}
fn class_name(o: &Handle) -> String {
    // SAFETY: the object's class.
    unsafe { red4ext_abi::rtti::object_class(o.instance) }.map(|c| c.class_name().text().to_string()).unwrap_or_default()
}
fn controllers(layer: &Handle) -> Vec<Handle> {
    rt::call(layer, "GetGameControllers", &[]).map(|o| r::items(&Val::of(&o)).iter().map(Val::handle).collect()).unwrap_or_default()
}
fn children(w: &Handle) -> Vec<Handle> {
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0) as i64;
    (0..n).map(|i| rt::obj(w, "GetWidgetByIndex", &[A::N(i as f64)])).collect()
}
/// The UI_System board's menu flag and the menu layer's controllers (the launch tools' evidence).
fn menu_evidence() -> J {
    let mut result = Map::new();
    if let Ok(defs) = super::boards::defs() {
        if let Ok(def) = super::boards::definition(&defs, "UI_System") {
            if let (Ok(board), Ok(id)) = (super::boards::system_board(def), super::boards::field(def, "IsInMenu")) {
                if !board.is_null() {
                    result.insert("menuBlackboard".into(), json!(r::b(&board, "GetBool", &[A::P(id)])));
                }
            }
        }
    }
    let list: Vec<J> = controllers(&ink_layer("inkMenuLayer"))
        .iter()
        .map(|c| {
            let root = rt::obj(c, "GetRootWidget", &[]);
            json!({ "class": class_name(c), "visible": !root.is_null() && r::b(&root, "IsVisible", &[]) })
        })
        .collect();
    result.insert("menuControllers".into(), J::Array(list));
    J::Object(result)
}
/// The native modded-save warning: one visible popup with its ModLoadingTitle text and both buttons' controllers.
fn save_confirmation() -> J {
    let root = rt::obj(&ink_layer("inkSystemNotificationsLayer"), "GetVirtualWindow", &[]);
    if root.is_null() {
        return json!({ "ready": false });
    }
    #[derive(Default, Clone, Copy)]
    struct Proof {
        modded: bool,
        confirm: bool,
        cancel: bool,
    }
    let mut popups: Vec<Proof> = Vec::new();
    let mut stack: Vec<(Handle, u32, Option<usize>)> = vec![(root, 0, None)];
    let mut count = 0;
    while let Some((w, depth, proof)) = stack.pop() {
        count += 1;
        if count > 512 || depth > 24 {
            return json!({ "ready": false });
        }
        if !r::b(&w, "IsVisible", &[]) || rt::num(&w, "GetOpacity", &[]).unwrap_or(0.0) <= 0.01 {
            continue;
        }
        let name = rt::call(&w, "GetName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default();
        let mut proof = proof;
        if name == "popup" {
            popups.push(Proof::default());
            proof = Some(popups.len() - 1);
        }
        if let Some(i) = proof {
            if name == "ModLoadingTitle" && r::is_a(&w, "inkTextWidget") {
                popups[i].modded = true;
            }
            if name == "buttonConfirm" || name == "buttonCancel" {
                let c = rt::obj(&w, "GetController", &[]);
                if r::is_a(&c, "inkButtonController") {
                    if name == "buttonConfirm" {
                        popups[i].confirm = true;
                    } else {
                        popups[i].cancel = true;
                    }
                }
            }
        }
        if r::is_a(&w, "inkCompoundWidget") {
            let kids = children(&w);
            if count + stack.len() + kids.len() > 512 {
                return json!({ "ready": false });
            }
            for k in kids.into_iter().rev() {
                stack.push((k, depth + 1, proof));
            }
        }
    }
    let p = (popups.len() == 1).then(|| popups[0]);
    let modded = p.is_some_and(|p| p.modded);
    json!({ "kind": if modded { "modded-save" } else { "unknown" }, "ready": p.is_some_and(|p| p.modded && p.confirm && p.cancel) })
}
/// continuePanelReady: the initial loading screen's skip panel visible all the way up.
fn continue_panel_ready(root: &Handle) -> (bool, bool) {
    let mut stack = vec![(root.clone(), 0u32)];
    let (mut nodes, mut found) = (0, false);
    while let Some((w, depth)) = stack.pop() {
        nodes += 1;
        if nodes > 256 || depth > 16 || w.is_null() {
            return (false, true);
        }
        if !r::b(&w, "IsVisible", &[]) {
            continue;
        }
        let c = rt::obj(&w, "GetController", &[]);
        let name = if c.is_null() { String::new() } else { class_name(&c) };
        if name == "inkInitialLoadingScreenLogicController" || name == "InitialLoadingScreenLogicController" {
            let mut panel = super::world_touch::widget_ref(&c, "skipButtonPanel").unwrap_or_else(|_| Handle::null());
            let mut visible = !panel.is_null();
            for _ in 0..24 {
                if panel.is_null() {
                    break;
                }
                if !r::b(&panel, "IsVisible", &[]) {
                    visible = false;
                    break;
                }
                panel = rt::obj(&panel, "GetParentWidget", &[]);
            }
            found |= visible;
        }
        if r::is_a(&w, "inkCompoundWidget") {
            let kids = children(&w);
            if nodes + stack.len() + kids.len() > 256 {
                return (false, true);
            }
            for k in kids.into_iter().rev() {
                stack.push((k, depth + 1));
            }
        }
    }
    (found, false)
}
/// loadingScreenActive: any visible loading-layer controller; the initial supervisor's Continue panel.
fn loading_screen() -> (bool, J, bool) {
    let layer = ink_layer("inkLoadingLayer");
    if layer.is_null() {
        return (false, json!([]), false);
    }
    let (mut active, mut ready, mut failed) = (false, false, false);
    let mut rows = Vec::new();
    for c in controllers(&layer) {
        let name = class_name(&c);
        let root = rt::obj(&c, "GetRootWidget", &[]);
        let visible = !root.is_null() && r::b(&root, "IsVisible", &[]);
        let mut row = Map::new();
        row.insert("class".into(), json!(name));
        row.insert("visible".into(), json!(visible));
        if visible {
            active = true;
            if name == "inkInitialLoadingControllerSupervisor" {
                let (r_, f) = continue_panel_ready(&root);
                row.insert("continueReady".into(), json!(r_));
                ready |= r_;
                failed |= f;
            }
        }
        rows.push(J::Object(row));
    }
    (active, J::Array(rows), ready && !failed)
}
fn sample_world(dt: f64) {
    let requests = handler();
    let pl = super::belt::player();
    let pre_game = requests.is_null() || r::b(&requests, "IsPreGame", &[]);
    let paused = !requests.is_null() && r::b(&requests, "IsGamePaused", &[]);
    let (loading, controllers_j, continue_ready) = loading_screen();
    let pos = super::melee::world_pos(&pl);
    let camera = r::o(&pl, "GetFPPCameraComponent", &[]);
    let valid = pos.is_some_and(|p| p.iter().all(|v| v.is_finite())) && !camera.is_null();
    let player = !pl.is_null();
    let mut sample = Map::new();
    sample.insert("preGame".into(), json!(pre_game));
    sample.insert("paused".into(), json!(paused));
    sample.insert("loading".into(), json!(loading));
    sample.insert("player".into(), json!(player));
    sample.insert("worldValid".into(), json!(valid));
    sample.insert("loadingControllers".into(), controllers_j);
    sample.insert("continueReady".into(), json!(continue_ready));
    with(|b| {
        let ready = player && valid && !pre_game && !paused && !loading;
        b.stable_frames = if ready { b.stable_frames + 1 } else { 0 };
        b.loading_frames = if loading { b.loading_frames + 1 } else { 0 };
        b.ready_seconds = if ready { b.ready_seconds + dt } else { 0.0 };
        b.loading_seconds = if loading { b.loading_seconds + dt } else { 0.0 };
        let mut phase = "loading";
        if ready && b.stable_frames >= 30 && b.ready_seconds >= 1.0 {
            phase = "world_ready";
        } else if player && valid && !pre_game && !paused && loading && continue_ready && b.loading_frames >= 30 && b.loading_seconds >= 1.0 {
            phase = "awaiting_continue";
        }
        let changed = phase != b.phase;
        if changed {
            write_status(b, phase);
            b.status_elapsed = 0.0;
        }
        b.status_elapsed += dt;
        if b.status_elapsed >= 0.5 || changed {
            b.status_elapsed = 0.0;
            write_detailed(b, sample);
        }
    });
}
fn arm(source: &str) {
    with(|b| {
        if b.fired || b.armed || !b.enabled {
            return;
        }
        b.armed = true;
        b.elapsed = 0.0;
        write_status(b, &format!("armed:{source}"));
        write_detailed(b, Map::new());
        cvr_host::log!("[quickboot] main menu observed by {source}; resuming in {:.2}s (from Rust)\n", b.delay);
    });
}
fn resume() {
    let (name, names, controller) = with(|b| (b.save_name.clone(), b.save_names.clone(), b.menu.take().map(|w| w.lock()).unwrap_or_else(Handle::null)));
    if let Some(name) = name {
        let index = names.iter().position(|n| *n == name);
        let ok = match (controller.is_null(), index) {
            (false, Some(i)) => rt::call(&controller, "LoadSaveInGame", &[A::N(i as f64)]).is_some(),
            _ => false,
        };
        with(|b| {
            if ok {
                b.resume_method = Some(format!("named-save:{name}"));
                write_status(b, "resumed:named-save");
            } else {
                write_status(b, &format!("failed:named-save-missing:{name}"));
            }
        });
        return;
    }
    // the menu's own handler first, as the menu's Continue does; LoadLastCheckpoint returns nothing
    let from_menu = if controller.is_null() { Handle::null() } else { rt::obj(&controller, "GetSystemRequestsHandler", &[]) };
    let handler = if from_menu.is_null() { handler() } else { from_menu };
    let ok = !handler.is_null() && super::belt::m::<()>(&handler, "LoadLastCheckpoint", &[Value::Bool(false)]).is_ok();
    with(|b| {
        if ok {
            b.resume_method = Some("LoadLastCheckpoint(false)".into());
            write_status(b, "resumed:LoadLastCheckpoint(false)");
        } else {
            write_status(b, "failed:no-compatible-continue-method");
        }
    });
    cvr_host::log!("[quickboot] resumed the newest save: {ok} (from Rust)\n");
}

/// onInit: the config, a tool-owned run's control, the first status.
fn start() {
    let cfg = read_json("quickboot.json");
    let run = read_json("quickboot_run.json");
    with(|b| {
        b.started = true;
        if let Some(c) = &cfg {
            if let Some(e) = c.get("enabled").and_then(J::as_bool) {
                b.enabled = e;
            }
            if let Some(d) = c.get("delay").and_then(J::as_f64).filter(|d| (0.0..=30.0).contains(d)) {
                b.delay = d;
            }
        }
        if let Some(run) = &run {
            if let Some(id) = run.get("runId").and_then(J::as_str).filter(|s| s.len() <= 80) {
                b.run_id = id.to_string();
                b.require_native_ready =
                    run.get("requireNativeReady").and_then(J::as_bool) == Some(true) && run.get("expiresAt").and_then(J::as_i64).is_some_and(|e| e >= now_secs());
                b.resume_allowed = run.get("allowResume").and_then(J::as_bool) == Some(true);
                if b.require_native_ready {
                    b.save_name = run.get("save").and_then(J::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                }
            }
        }
        let _ = std::fs::write(folder().join("quickboot.json"), format!("{{\n  \"enabled\": {},\n  \"delay\": {:.2}\n}}\n", b.enabled, b.delay));
        let phase = if b.enabled { "ready" } else { "disabled" };
        write_status(b, phase);
        write_detailed(b, Map::new());
        cvr_host::log!("[quickboot] ready from Rust; enabled={} delay={:.2}s run={}\n", b.enabled, b.delay, b.run_id);
    });
}
/// The mod's hotkey (cyberpunkvr_quickboot_toggle, hotkeys.rs): enabled or not for the next launch, saved.
pub(super) fn toggle() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|b| {
        b.enabled = !b.enabled;
        let _ = std::fs::write(folder().join("quickboot.json"), format!("{{\n  \"enabled\": {},\n  \"delay\": {:.2}\n}}\n", b.enabled, b.delay));
        let phase = if b.enabled { "ready" } else { "disabled" };
        write_status(b, phase);
        write_detailed(b, Map::new());
        cvr_host::log!("[quickboot] {} for the next launch (from Rust)\n", if b.enabled { "enabled" } else { "disabled" });
    });
}
/// Every frame (the mechanics' tick) while the Lua host is not installed.
pub(super) fn tick(dt: f64) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if !with(|b| b.started) {
        start();
    }
    let (enabled, fired, phase, require) = with(|b| (b.enabled, b.fired, b.phase.clone(), b.require_native_ready));
    if !enabled {
        with(|b| b.armed = false);
        return;
    }
    if fired {
        if require || phase != "world_ready" {
            sample_world(dt);
        }
        return;
    }
    let go = with(|b| {
        if !b.armed {
            return false;
        }
        b.elapsed += dt;
        if b.elapsed < b.delay {
            return false;
        }
        if b.require_native_ready && !b.resume_allowed {
            b.gate_elapsed += dt;
            if b.phase != "awaiting_native_menu" {
                write_status(b, "awaiting_native_menu");
                write_detailed(b, Map::new());
            }
            if b.gate_elapsed < 0.1 {
                return false;
            }
            b.gate_elapsed = 0.0;
            return false;
        }
        b.fired = true;
        b.armed = false;
        write_status(b, "resuming");
        write_detailed(b, Map::new());
        true
    });
    if !go {
        // the tool releases the resume by rewriting the run control
        let waiting = with(|b| b.armed && b.require_native_ready && !b.resume_allowed && b.gate_elapsed == 0.0);
        if waiting {
            let run = read_json("quickboot_run.json");
            with(|b| {
                b.resume_allowed = run.as_ref().is_some_and(|r| r.get("runId").and_then(J::as_str) == Some(b.run_id.as_str()) && r.get("allowResume").and_then(J::as_bool) == Some(true));
            });
        }
        return;
    }
    resume();
    with(|b| write_detailed(b, Map::new()));
}

// ---------------------------------------------------------------- the menu's observers
fn menu_finished(call: &mut Call) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let c = call.this();
    let ready = with(|b| {
        b.menu = Some(WeakHandle::from_handle(&c));
        b.menu_initialized = true;
        b.menu_initialized && b.saves_ready
    });
    if ready {
        arm("SingleplayerMenuGameController:menu-and-saves-ready");
    }
}
fn saves_ready(call: &mut Call) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let names = call.arg_strings(0);
    let ready = with(|b| {
        b.save_names = names;
        b.saves_ready = true;
        b.menu_initialized && b.saves_ready
    });
    if ready {
        arm("SingleplayerMenuGameController:menu-and-saves-ready");
    }
}
fn menu_closed(_call: &mut Call) {
    with(|b| b.menu = None);
}
fn pitch_screen(_call: &mut Call) {
    if !ACTIVE.load(Ordering::Relaxed) || !with(|b| b.require_native_ready) {
        return;
    }
    with(|b| b.via_checkpoint = true);
    arm("PitchMenuGameController:first-launch-screen");
}
/// The observers (installed as the CET host's: outer), only while the Lua host is not installed.
pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 4] = [
    ("SingleplayerMenuGameController", "FinishMenuInitialization", Kind::After, menu_finished),
    ("SingleplayerMenuGameController", "OnSavesForLoadReady", Kind::After, saves_ready),
    ("SingleplayerMenuGameController", "OnUninitialize", Kind::After, menu_closed),
    ("PitchMenuGameController", "OnInitialize", Kind::After, pitch_screen),
];
