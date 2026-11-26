//! mods/cet/CyberpunkVRPort_Stereo/modules/vrcam_select.lua (exactly ONE authored VRCAM component enabled, chosen by
//! render resolution): the player carries one entRenderToTextureCameraComponent per resolution (vrcam_<W>x<H>, all
//! authored off; the braindance replacer the same set as vrcam_braindance_<W>x<H>), the launcher writes the pick into
//! vrcam.json, and this switches it on through the component's own Toggle (the isEnabled field when there is none) and
//! every sibling off -- by NAME only. The live component's virtualCameraName goes to bridge/vrcam_active.txt (the view
//! key is hashed from the real name), and the camera is nudged a tenth of a millimetre for two ticks (a render-to-
//! texture camera switched on under a still view draws nothing until its transform changes). The entity walked away
//! from (a save, a braindance) has its cameras released; in the main menu and on loading screens every camera is kept
//! off (the menu's preview player about once a second). Re-applied only when the selection, the wanted state (the
//! overlay's VRCAM switch, VRVrcamEnableRequest) or the player's entity changes, retried every 30 ticks.
use crate::script_call::{method, property_of_kind, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::RttiKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// ~150 ms at 200 fps: the player is not there during a load.
const RETRY_TICKS: i32 = 30;
/// The main menu's player: its cameras kept off about once a second.
const MENU_RELEASE_TICKS: i32 = 90;
const NAME_PREFIX: &str = "vrcam_";
/// The braindance replacer's own set: same virtualCameraName, same texture.
const BD_PREFIX: &str = "vrcam_braindance_";
const NUDGE_TICKS: i32 = 2;

struct Nudged {
    component: Handle,
    position: [f32; 4],
    ticks: i32,
}
struct State {
    selected: Option<String>,
    want: bool,
    applied_name: Option<Option<String>>,
    applied_want: Option<bool>,
    applied_id: Option<u64>,
    last_error: Option<String>,
    ticks_to_retry: i32,
    enabled_count: i32,
    seen_count: i32,
    prev_player: Handle,
    prev_id: Option<u64>,
    on_bd_replacer: bool,
    loading_active: bool,
    nudged: Option<Nudged>,
    loaded: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<State> = Mutex::new(State {
    selected: None,
    want: true,
    applied_name: None,
    applied_want: None,
    applied_id: None,
    last_error: None,
    ticks_to_retry: 0,
    enabled_count: 0,
    seen_count: 0,
    prev_player: Handle::null(),
    prev_id: None,
    on_bd_replacer: false,
    loading_active: false,
    nudged: None,
    loaded: false,
});
/// The menu or no session (Game.GetSystemRequestsHandler():IsPreGame()), for the Lua host's GC pacing
/// (VRStereoPreGame), and whether this tick has run yet.
static PRE_GAME: AtomicBool = AtomicBool::new(false);
static TICKED: AtomicBool = AtomicBool::new(false);

/// VrcamSel.preGame as the mod kept it: the last tick's answer (before the first tick, asked now).
pub(in crate::mechanics) fn pre_game() -> bool {
    if TICKED.load(Ordering::Relaxed) {
        PRE_GAME.load(Ordering::Relaxed)
    } else {
        pre_game_active().unwrap_or(false)
    }
}

/// vrcam.json's "component" (scanned, not parsed: the file is two fields and a comment array).
fn read_selection() -> Result<String, String> {
    let path = cvr_render::vrcam_selection::config_file().ok_or("vrcam.json not found in the mod folder")?;
    let body = std::fs::read_to_string(&path).map_err(|_| "vrcam.json not found in the mod folder".to_string())?;
    if body.is_empty() {
        return Err("vrcam.json is empty".into());
    }
    // '"component"%s*:%s*"([^"]+)"'
    let mut rest = body.as_str();
    while let Some(i) = rest.find("\"component\"") {
        rest = &rest[i + "\"component\"".len()..];
        let after = rest.trim_start();
        if let Some(after) = after.strip_prefix(':') {
            if let Some(value) = after.trim_start().strip_prefix('"') {
                if let Some(end) = value.find('"') {
                    if end > 0 {
                        return Ok(value[..end].to_string());
                    }
                }
            }
        }
    }
    Err("vrcam.json has no \"component\" field".into())
}

/// The mod's reload(): the selection read again, applied once more.
pub(in crate::mechanics) fn reload() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    reload_in(&mut s);
}
fn reload_in(s: &mut State) {
    match read_selection() {
        Ok(name) => {
            log!("[Stereo.VRCAM] selection: {name}\n");
            s.selected = Some(name);
            s.last_error = None;
        }
        Err(e) => {
            log!("[Stereo.VRCAM] {e} -- no component will be enabled\n");
            s.selected = None;
            s.last_error = Some(e);
        }
    }
    s.applied_name = None;
    s.applied_want = None;
    s.loaded = true;
}

/// Optics' stereoBound(): the eyes are settled on the player in hand -- the second view enabled on it (exactly one
/// component), or none asked for at all -- as the mod's state said it (not while loading, the selection applied, to
/// this player's entity).
pub(in crate::mechanics) fn settled() -> bool {
    let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() || s.loading_active || s.applied_name.clone().flatten() != s.selected || s.applied_id.is_none() || s.applied_id != entity_id(&player) {
        return false;
    }
    let settled = match s.applied_want {
        Some(false) => true,
        Some(true) => s.enabled_count == 1,
        None => false,
    };
    static REPORTED: super::PerPlayer = super::PerPlayer::new();
    if settled && REPORTED.first() {
        log!("[mechanics] vrcam: the eyes settled on the player, for Optics, from Rust (want={:?} enabled={})\n", s.applied_want, s.enabled_count);
    }
    settled
}

/// The mod's status().
pub(in crate::mechanics) fn status() -> String {
    status_of(&STATE.lock().unwrap_or_else(|p| p.into_inner()))
}
fn status_of(s: &State) -> String {
    format!(
        "selected={} want={} applied={} vrcamComps={} enabled={}{}",
        s.selected.as_deref().unwrap_or("nil"),
        s.want,
        s.applied_name.as_ref().and_then(|n| n.as_deref()).unwrap_or("nil"),
        s.seen_count,
        s.enabled_count,
        s.last_error.as_ref().map(|e| format!(" err={e}")).unwrap_or_default()
    )
}

/// A component's CName property's text (name, virtualCameraName).
fn name_property(component: &Handle, name: &str) -> Option<String> {
    // SAFETY: the component, held; its CName property.
    let p = unsafe { property_of_kind(component.instance, name, RttiKind::Name as u8) }?;
    // SAFETY: as above.
    let text = CName(unsafe { *(p as *const u64) }).text();
    (!text.is_empty()).then(|| text.to_string())
}
fn is_enabled(component: &Handle) -> bool {
    // SAFETY: the component, held; its Bool property.
    unsafe { property_of_kind(component.instance, "isEnabled", RttiKind::Fundamental as u8) }.is_some_and(|p| unsafe { *(p as *const bool) })
}
/// The mod's setEnabled(): Toggle, else the field.
fn set_enabled(component: &Handle, want: bool) -> Result<&'static str, String> {
    // SAFETY: the component, held.
    if unsafe { method::<()>(component.instance, "Toggle", &[Value::Bool(want)]) }.is_some() {
        return Ok("Toggle");
    }
    // SAFETY: as above; its Bool property.
    match unsafe { property_of_kind(component.instance, "isEnabled", RttiKind::Fundamental as u8) } {
        Some(p) => {
            unsafe { *(p as *mut bool) = want };
            Ok("isEnabled")
        }
        None => Err("no way to toggle".into()),
    }
}
fn entity_id(entity: &Handle) -> Option<u64> {
    // SAFETY: the entity, held.
    unsafe { method::<u64>(entity.instance, "GetEntityID", &[]) }
}

/// The mod's releaseEntity(): every enabled vrcam_* of an entity walked away from turned off (two enabled cameras on
/// one feed is the flicker).
fn release_entity(entity: &Handle) -> i32 {
    if entity.is_null() {
        return 0;
    }
    let mut off = 0;
    for c in crate::engine_objects::entity_components(entity) {
        if name_property(&c, "name").is_some_and(|n| n.starts_with(NAME_PREFIX)) && is_enabled(&c) {
            let _ = set_enabled(&c, false);
            off += 1;
        }
    }
    off
}

/// The mod's nudge() and nudgeBack().
fn nudge(s: &mut State, component: &Handle) {
    // SAFETY: the component, held.
    let Some(position) = (unsafe { method::<[f32; 4]>(component.instance, "GetLocalPosition", &[]) }) else { return };
    let moved = [position[0], position[1], position[2] + 0.0001, position[3]];
    // SAFETY: as above; the value lives for the call.
    if unsafe { method::<()>(component.instance, "SetLocalPosition", &[Value::Ptr(moved.as_ptr().cast())]) }.is_some() {
        s.nudged = Some(Nudged { component: component.clone(), position, ticks: NUDGE_TICKS });
    }
}
fn nudge_back(s: &mut State) {
    let Some(n) = s.nudged.as_mut() else { return };
    n.ticks -= 1;
    if n.ticks > 0 {
        return;
    }
    let n = s.nudged.take().expect("checked above");
    // SAFETY: the component, held; the value lives for the call.
    unsafe { method::<()>(n.component.instance, "SetLocalPosition", &[Value::Ptr(n.position.as_ptr().cast())]) };
}

/// The mod's apply(): true when the entity was read (applied, or retrying would not change the outcome).
fn apply(s: &mut State, player: &Handle, wanted: Option<&str>, want_on: bool) -> bool {
    let Some(wanted) = wanted else { return false };
    let comps = crate::engine_objects::entity_components(player);
    if comps.is_empty() {
        s.last_error = Some("player has no components yet".into());
        return false;
    }
    let names: Vec<Option<String>> = comps.iter().map(|c| name_property(c, "name")).collect();
    // Which of the two sets this entity carries: the resolution suffix kept, the prefix chosen.
    let suffix = wanted.get(NAME_PREFIX.len()..).unwrap_or("");
    let wanted_bd = format!("{BD_PREFIX}{suffix}");
    let has_bd = names.iter().any(|n| n.as_deref() == Some(wanted_bd.as_str()));
    let wanted = if has_bd { wanted_bd.clone() } else { wanted.to_string() };
    s.on_bd_replacer = has_bd;
    // ...and the entity come from let go of, once, on the frame the player changes.
    let id = entity_id(player);
    if let (Some(id), Some(prev)) = (id, s.prev_id) {
        if id != prev {
            let prev_player = std::mem::replace(&mut s.prev_player, Handle::null());
            let off = release_entity(&prev_player);
            log!("[Stereo.VRCAM] player changed ({prev} -> {id}): {off} component(s) released\n");
        }
    }
    s.prev_player = player.clone();
    s.prev_id = id;

    let (mut seen, mut enabled, mut disabled, mut found, mut how) = (0, 0, 0, false, None);
    let mut enabled_comp = None;
    for (c, name) in comps.iter().zip(&names) {
        let Some(name) = name.as_deref().filter(|n| n.starts_with(NAME_PREFIX)) else { continue };
        seen += 1;
        let want = want_on && name == wanted;
        match set_enabled(c, want) {
            Ok(via) => {
                if want {
                    found = true;
                    enabled += 1;
                    how = Some(via);
                    enabled_comp = Some(c.clone());
                } else {
                    disabled += 1;
                }
            }
            Err(via) => s.last_error = Some(format!("cannot toggle {name}: {via}")),
        }
    }
    s.seen_count = seen;
    s.enabled_count = enabled;
    if seen == 0 {
        // No retrying forever on an entity that simply has no VRCAM components authored.
        s.last_error = Some(format!("no {NAME_PREFIX}* components on the player entity"));
        return false;
    }
    if want_on && !found {
        let e = format!("{wanted} not found among {seen} {NAME_PREFIX}* component(s) -- all left disabled");
        log!("[Stereo.VRCAM] {e}\n");
        s.last_error = Some(e);
        return true;
    }
    s.last_error = None;
    // The ACTUAL virtualCameraName of the component just enabled: the asset is the source of truth.
    let mut active = String::new();
    if let Some(c) = enabled_comp.as_ref().filter(|_| want_on) {
        active = name_property(c, "virtualCameraName").unwrap_or_default();
        nudge(s, c);
    }
    cvr_render::vrcam_selection::bridge_write("vrcam_active.txt", &active);
    if want_on {
        log!(
            "[Stereo.VRCAM] enabled {wanted} (camera {}) via {} (1 of {seen}, {disabled} disabled)\n",
            if active.is_empty() { "?" } else { active.as_str() },
            how.unwrap_or("nil")
        );
        let expect = format!("vrcam_feed_{}", wanted.get(NAME_PREFIX.len()..).unwrap_or(""));
        if !active.is_empty() && active != expect {
            log!("[Stereo.VRCAM] {wanted} has virtualCameraName={active} (expected {expect}) -- native will use the real one\n");
        }
    } else {
        log!("[Stereo.VRCAM] VRCAM off ({disabled} component(s) disabled)\n");
    }
    true
}

/// The mod's preGameActive(): GetPlayer can return the menu's preview player before a session exists.
fn pre_game_active() -> Result<bool, String> {
    let handler = super::requests_handler()?;
    if handler.is_null() {
        return Ok(true);
    }
    // SAFETY: the handler, held.
    unsafe { method::<bool>(handler.instance, "IsPreGame", &[]) }.ok_or_else(|| "IsPreGame failed".into())
}

/// A loading screen is up (loading_screen_active; false when it cannot be told).
pub(in crate::mechanics) fn loading_screen() -> bool {
    loading_screen_active().unwrap_or(false)
}
/// The mod's loadingScreenActive(): a fast-travel or initial loading supervisor's root shown on the loading layer.
fn loading_screen_active() -> Result<bool, String> {
    let layer = crate::engine_objects::layer_wrapper(CName::hashed("inkLoadingLayer").0);
    if layer.is_null() {
        return Ok(false);
    }
    for controller in super::hud_panel::game_controllers(&layer)? {
        if controller.is_null() {
            continue;
        }
        // SAFETY: the controller, held; its root held by its handle.
        unsafe {
            let class = CName(method::<u64>(controller.instance, "GetClassName", &[]).ok_or("GetClassName failed")?);
            if class == CName::hashed("inkFastTravelLoadingControllerSupervisor") || class == CName::hashed("inkInitialLoadingControllerSupervisor") {
                let root = crate::script_call::call_handle(controller.instance, "GetRootWidget", &[]).ok_or("GetRootWidget failed")?;
                if !root.is_null() && method::<bool>(root.instance, "IsVisible", &[]).ok_or("IsVisible failed")? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// The mod's releaseForLoading().
fn release_for_loading(s: &mut State) {
    let prev = std::mem::replace(&mut s.prev_player, Handle::null());
    release_entity(&prev);
    s.prev_id = None;
    s.applied_name = None;
    s.applied_want = None;
    s.applied_id = None;
    s.enabled_count = 0;
    s.nudged = None;
}

/// VRVrcamEnableRequest's answer: -1 none, 0 disable, 1 enable.
fn enable_request() -> i32 {
    cvr_render::vrcam_selection::cvr_cpp_vrcam_enable_request()
}

/// The mod's tick: every frame.
pub(in crate::mechanics) fn tick() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if !s.loaded {
        // VrcamSelect.init() at the mod's onInit.
        reload_in(&mut s);
        log!("[Stereo] {}\n", status_of(&s));
    }
    TICKED.store(true, Ordering::Relaxed);
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    nudge_back(&mut s);
    // No second view wanted and this player's cameras already off: the loading layer is not walked every frame.
    if !player.is_null() && !s.want && s.applied_want == Some(false) && s.applied_id.is_some() {
        if entity_id(&player) == s.applied_id && enable_request() != 1 {
            return;
        }
    }
    let session = pre_game_active();
    let pre_game = session.as_ref().is_ok_and(|p| *p);
    PRE_GAME.store(pre_game, Ordering::Relaxed);
    // The loading layer only when the menu does not already say so.
    let mut loading = Ok(false);
    if session.is_ok() && !pre_game {
        loading = loading_screen_active();
    }
    let loading = loading.unwrap_or(s.loading_active);
    if session.is_err() || pre_game || loading {
        if !s.loading_active {
            // A player can exist throughout a save's replacement, and the new one may reuse its EntityID: the previous
            // entity and its render target released before the old world's renderer resources go.
            release_for_loading(&mut s);
            s.ticks_to_retry = 0;
            cvr_render::vrcam_selection::bridge_write("vrcam_active.txt", "");
            log!("[Stereo.VRCAM] suspended in the main menu or loading screen\n");
        }
        s.loading_active = true;
        if s.ticks_to_retry <= 0 {
            // Preview/restored players: their render targets kept off too, no handle kept.
            release_entity(&player);
            s.ticks_to_retry = if pre_game { MENU_RELEASE_TICKS } else { 5 };
        } else {
            s.ticks_to_retry -= 1;
        }
        return;
    }
    if s.loading_active {
        s.loading_active = false;
        s.ticks_to_retry = 0;
        log!("[Stereo.VRCAM] gameplay ready; rebind the current player\n");
    }
    if player.is_null() {
        release_for_loading(&mut s);
        s.ticks_to_retry = 0;
        return;
    }
    if s.ticks_to_retry > 0 {
        s.ticks_to_retry -= 1;
        return;
    }
    s.ticks_to_retry = RETRY_TICKS;
    match enable_request() {
        1 => s.want = true,
        0 => s.want = false,
        _ => {}
    }
    // Re-applied only when the wanted state changed -- and when the player's entity changed (a braindance swaps it).
    let id = entity_id(&player);
    if s.applied_name.as_ref() == Some(&s.selected) && s.applied_want == Some(s.want) && s.applied_id == id {
        return;
    }
    let (selected, want) = (s.selected.clone(), s.want);
    if apply(&mut s, &player, selected.as_deref(), want) {
        s.applied_name = Some(selected);
        s.applied_want = Some(want);
        s.applied_id = id;
    }
}
