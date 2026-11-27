//! mods/cet/CyberpunkVRPort_Lobby (init.lua and modules/lobby.lua): the main menu as a place. While the plugin shows
//! the main menu in its world (VRLobbyActive), the player is put on the scene's spot (teleported back when more than
//! 60 m off), the time held at 21:30 with its weather and the clock stopped, the scene's exposure set, the scene spawned
//! (its entities, then its loose items and the engrams, its lights), the intro played once, the curtain opened, and
//! the lobby's own menu opened on the holo (the first launch's setup before it); the game's menu window is hidden.
//! When the game's menu goes to one of its own screens the lobby steps aside (VRLobbySuspend); when the main menu
//! goes, everything is taken down. The game's controllers come from the Lua host's observers (VRLobbyMenu,
//! VRLobbySaves); the WorldBuilder bridge (modules/worldbuilder.lua, a development tool driving another CET mod's
//! editor) and the unused terminal (the scene has a holo) stay the Lua host's.
use super::lobby_creation::Creation;
use super::lobby_engrams::{Clothes, Construct, Engrams};
use super::lobby_hands::{Grips, Hands};
use super::lobby_holo::{self as holo, Anchor, Holo};
use super::lobby_ink as ink;
use super::lobby_items::Items;
use super::lobby_lights::Lights;
use super::lobby_menu::MenuState;
use super::lobby_scene as scene;
use super::lobby_setup::Setup;
use super::lobby_spawn::{self as sp, native, Sound, V3};
use super::lobby_intro::Intro;
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::rc::Rc;
use std::sync::Mutex;

const SCENE_TAG: &str = "VRLobbyScene";
const PLACE_REACH: f64 = 60.0;
const CLOCK_SOURCE: &str = "VRLobby";

/// What the Lua host's observers report of the game's menus.
#[derive(Default)]
pub(super) struct Shared {
    pub main_menu: Handle,
    pub menu_away: bool,
    pub save_names: Option<Vec<String>>,
    pub names_gen: u64,
    pub creation: Handle,
    pub creation_events: Handle,
    pub creating: bool,
}
#[derive(Default)]
pub(super) struct Lobby {
    pub holo: Holo,
    pub sound: Sound,
    pub shared: Shared,
    pub menu: MenuState,
    pub creation: Creation,
    pub engrams: Engrams,
    pub construct: Construct,
    pub clothes: Clothes,
    pub items: Items,
    pub lights: Lights,
    pub hands: Hands,
    pub grips: Grips,
    pub setup: Setup,
    pub intro: Intro,
    window: Handle,
    hidden: bool,
    error: Option<String>,
    scene: Option<Vec<Option<u64>>>,
    anchor: Option<(V3, f64)>,
    clock: f64,
    away: bool,
    items_open: bool,
    exposure: f64,
    place_clock: f64,
    clock_stopped: Option<bool>,
    inited: bool,
    reported: bool,
    controls_tested: bool,
    coop_tested: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Lobby {}
static LOBBY: Mutex<Option<Lobby>> = Mutex::new(None);

impl Lobby {
    pub(super) fn anchor(&self) -> Option<(V3, f64)> {
        self.anchor
    }
}
fn anchor_here() -> Option<(V3, f64)> {
    Some(scene::PLACE)
}
fn put_on_place(l: &mut Lobby, dt: f64) -> bool {
    let at = scene::PLACE.0;
    let there = super::lobby_hands::player_pose().is_some_and(|(p, _)| {
        let d = [p[0] - at[0], p[1] - at[1], p[2] - at[2]];
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < PLACE_REACH * PLACE_REACH
    });
    l.place_clock -= dt;
    if there || l.place_clock > 0.0 {
        return there;
    }
    let pl = super::belt::player();
    let tf = super::game_system("GetTeleportationFacility");
    if !pl.is_null() && !tf.is_null() {
        let pos = [at[0] as f32, at[1] as f32, (at[2] + 0.1) as f32, 1.0];
        let rot = [0.0f32, 0.0, scene::PLACE.1 as f32];
        ink::call(&tf, "Teleport", &[Value::Handle(&pl), Value::Ptr(pos.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast())]);
        l.place_clock = 3.0;
    }
    false
}
fn curtain(closure: f64) {
    native::<()>("VRLobbyCurtain", &[Value::Float(closure as f32)]);
}
fn open_menu(l: &mut Lobby) {
    let t = super::lobby_menu::texts();
    let anchor = l.anchor.map(|(pos, yaw)| Anchor { pos, yaw });
    let spec = Some(scene::holo());
    // the controls board stands beside the menu (lobby_controls); a menu opened again takes it down first
    let board = |l: &mut Lobby| holo::board(l, super::lobby_controls::BOARD, super::lobby_controls::draw_board);
    if super::lobby_setup::needed() {
        let main: Rc<dyn Fn(&mut Lobby)> = Rc::new(move |l| {
            let anchor = l.anchor.map(|(pos, yaw)| Anchor { pos, yaw });
            holo::open(l, anchor, Some(scene::holo()), super::lobby_menu::root(), Some(super::lobby_menu::texts().back.into()));
            board(l);
        });
        let page = super::lobby_setup::root(l, main);
        holo::open(l, anchor, spec, page, Some(t.back.into()));
        board(l);
    } else {
        holo::open(l, anchor, spec, super::lobby_menu::root(), Some(t.back.into()));
        board(l);
    }
}
fn spawn_scene(l: &mut Lobby) {
    if l.anchor.is_none() {
        l.anchor = anchor_here();
    }
    let Some((pos, yaw)) = l.anchor else { return };
    let q = super::lobby_hands::yaw_quat(yaw);
    let ids: Vec<Option<u64>> = scene::ENTITIES.iter().map(|p| sp::spawn(p, pos, q, SCENE_TAG)).collect();
    if ids.iter().any(Option::is_none) {
        l.error = Some("spawning the place failed".into());
    }
    l.scene = Some(ids);
    l.items_open = false;
}
fn scene_stands(l: &Lobby) -> bool {
    l.scene.as_ref().is_some_and(|s| sp::alive(s.first().copied().flatten()))
}
fn scene_spawned(l: &Lobby) -> bool {
    l.scene.as_ref().is_some_and(|s| s.len() == scene::ENTITIES.len() && s.iter().all(|id| sp::spawned(*id)))
}
fn despawn_scene(l: &mut Lobby) {
    super::lobby_creation::close(l);
    holo::close(l);
    super::lobby_engrams::close(l);
    super::lobby_items::close(l);
    super::lobby_lights::close(l);
    sp::clear(SCENE_TAG);
    (l.scene, l.items_open) = (None, false);
}
fn set_exposure(l: &mut Lobby, stops: f64) {
    if native::<f32>("VRLobbyExposureGet", &[]).is_some_and(|g| (f64::from(g) - stops).abs() > 0.001) {
        native::<()>("VRLobbyExposureSet", &[Value::Float(stops as f32)]);
    }
    l.exposure = stops;
}
fn stop_clock(l: &mut Lobby, stopped: bool) {
    if l.clock_stopped == Some(stopped) {
        return;
    }
    let ts = super::game_system("GetTimeSystem");
    if ts.is_null() {
        return;
    }
    ink::call(&ts, "SetPausedState", &[Value::Bool(stopped), sp::cn(CLOCK_SOURCE)]);
    l.clock_stopped = Some(stopped);
}
fn hold_atmosphere(l: &mut Lobby, dt: f64) {
    l.clock -= dt;
    if l.clock > 0.0 {
        return;
    }
    l.clock = 1.0;
    let ts = super::game_system("GetTimeSystem");
    if !ts.is_null() {
        if let Some(t) = super::lobby_rtti::call(&ts, "GetGameTime", &[]) {
            let secs = super::lobby_rtti::Val::of(&t).f("seconds").and_then(|v| v.num()).unwrap_or(0.0) as i64;
            let now = (secs / 3600) % 24 * 60 + (secs / 60) % 60;
            let held = scene::HOUR * 60 + scene::MINUTE;
            if now < held || now > held + 15 {
                ink::call(&ts, "SetGameTimeByHMS", &[Value::Int32(scene::HOUR as i32), Value::Int32(scene::MINUTE as i32), Value::Int32(0)]);
            }
        }
        let w = native::<u64>("VRWeatherState", &[]).map(|n| red4ext_abi::cname::CName(n).text().to_string()).unwrap_or_default();
        if w != scene::WEATHER {
            native::<bool>("VRWeatherSet", &[sp::cn(scene::WEATHER), Value::Float(0.0), Value::Int32(0)]);
        }
    }
    stop_clock(l, true);
    set_exposure(l, scene::EXPOSURE);
}
fn menu_window() -> Handle {
    let layer = sp::native_handle("VRInkLayer", &[sp::cn("inkMenuLayer")]);
    ink::child(&layer, "GetVirtualWindow", &[])
}
fn show(l: &mut Lobby) {
    if l.hidden && !l.window.is_null() {
        ink::visible(&l.window, true);
    }
    (l.window, l.hidden) = (Handle::null(), false);
}
fn close(l: &mut Lobby) {
    show(l);
    despawn_scene(l);
    super::lobby_engrams::restore(&mut l.construct);
    set_exposure(l, 0.0);
    stop_clock(l, false);
    l.anchor = None;
}
fn init(l: &mut Lobby) {
    sp::clear(sp::TAG);
    l.inited = true;
}
fn update(l: &mut Lobby, dt: f64) {
    if !l.inited {
        init(l);
    }
    let away = l.shared.menu_away && !l.shared.creating;
    if away != l.away {
        l.away = away;
        native::<()>("VRLobbySuspend", &[Value::Bool(away)]);
        if away {
            holo::close(l);
        }
    }
    let active = native::<bool>("VRLobbyActive", &[]) == Some(true);
    l.sound.update(dt);
    if !active {
        super::lobby_intro::stop(l);
        if !l.away {
            l.sound.ambient(false);
        }
        if l.hidden {
            show(l);
        }
        if !l.away {
            if l.scene.is_some() {
                despawn_scene(l);
            }
            super::lobby_engrams::restore(&mut l.construct);
            if l.exposure != 0.0 {
                set_exposure(l, 0.0);
            }
            stop_clock(l, false);
            l.anchor = None;
        }
        return;
    }
    let placed = l.away || put_on_place(l, dt);
    hold_atmosphere(l, dt);
    super::lobby_engrams::construct_update(&mut l.construct, dt);
    if l.scene.is_some() && !scene_stands(l) {
        despawn_scene(l);
        l.anchor = None;
    }
    if l.scene.is_none() {
        spawn_scene(l);
    }
    if !l.items_open && scene_spawned(l) {
        let list = super::lobby_engrams::items(l, scene::items());
        let anchor = l.anchor;
        super::lobby_items::open(l, anchor, list);
        super::lobby_lights::open(l, anchor, None);
        l.items_open = true;
    }
    let ready = placed && l.items_open;
    if ready && l.error.is_none() {
        super::lobby_intro::begin(l);
    }
    super::lobby_intro::update(l, dt);
    let mut shut = if ready || l.error.is_some() { 0.0 } else { 1.0 };
    if super::lobby_intro::running(l) {
        shut = super::lobby_intro::closure(l);
    }
    curtain(shut.max(super::lobby_creation::closure(l)));
    let ambient = !super::lobby_intro::running(l) && (ready || l.error.is_some());
    l.sound.ambient(ambient);
    if l.anchor.is_some() && (ready || l.error.is_some()) && !l.away && !holo::is_open(l) && !super::lobby_intro::running(l) {
        open_menu(l);
    }
    holo::update(l, dt);
    super::lobby_setup::update(l, dt);
    super::lobby_creation::update(l, dt);
    l.hands.update(dt);
    super::lobby_lights::update(l, dt);
    let ui = holo::pointing(l);
    super::lobby_items::update(l, dt, Some(ui));
    super::lobby_engrams::update(l, dt);
    // CVR_LOBBY_CONTROLS_TEST: the Controls page opened from the main menu once the board stands
    if !l.controls_tested && holo::board_shown(l) && holo::depth(l) == 1 && std::env::var_os("CVR_LOBBY_CONTROLS_TEST").is_some() {
        l.controls_tested = true;
        log!("[VRLobby] controls test: the Controls page pushed ({})\n", holo::state(l));
        holo::push(l, super::lobby_controls::page());
    }
    // CVR_LOBBY_COOP_TEST=page|keyboard: the co-op page (and its nickname keyboard) opened once the board stands
    if !l.coop_tested && holo::board_shown(l) && holo::depth(l) == 1 {
        if let Some(what) = std::env::var_os("CVR_LOBBY_COOP_TEST") {
            l.coop_tested = true;
            log!("[VRLobby] co-op test: {:?} (NightCoop loaded {})\n", what, super::lobby_coop::available());
            holo::push(l, super::lobby_coop::page());
            if what == "keyboard" {
                holo::push(l, super::lobby_coop::nickname_keyboard());
            }
        }
    }
    if l.window.is_null() {
        l.window = menu_window();
    }
    if l.window.is_null() {
        return;
    }
    if ink::is_visible(&l.window) {
        ink::visible(&l.window, false);
    }
    l.hidden = true;
    if !l.reported && holo::is_open(l) {
        l.reported = true;
        log!("[VRLobby] from Rust: the scene up, the menu on the holo ({}); {}\n", holo::state(l), super::lobby_items::state(l));
    }
}
/// The mod's onUpdate (the main menu as well: before the world's player).
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut g = LOBBY.lock().unwrap_or_else(|p| p.into_inner());
    let l = g.get_or_insert_with(Lobby::default);
    drain(l);
    let press = DEV_PRESS.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(key) = press {
        let ok = holo::dev_press(l, &key);
        log!("[dev] lobby '{key}': pressed {ok}; rows {:?}\n", holo::dev_rows(l));
    }
    update(l, f64::from(dt));
    drain(l);
}
/// The "lobby <row>" development command (dev_cmd.rs): a row of the holo's top page pressed on the next tick.
static DEV_PRESS: Mutex<Option<String>> = Mutex::new(None);
pub(in crate::mechanics) fn dev_press(key: &str) {
    *DEV_PRESS.lock().unwrap_or_else(|p| p.into_inner()) = Some(key.to_string());
}
/// What the game's menus told the lobby while it was busy. THE NEW GAME HANG (2026-11-27): the lobby's own calls into
/// the game run the menus' scripts synchronously -- the new game step's SpawnEvent("OnNewGame") leaves the main menu's
/// scenario and brings the new game's screen up inside the call -- and their observers (VRLobbyMenu, VRLobbySaves)
/// come back here on the same thread, the lobby still locked by its tick: the lock waited on itself and the game
/// froze. Such news is queued instead and applied as soon as the tick lets go of the lobby (the C++ lobby's state was
/// a plain global, read and written in place).
enum Pending {
    Menu(i32, Handle),
    Saves(String),
}
// SAFETY: the game's handles are only touched on the game thread, as the lobby's own are (Lobby is Send for the same
// reason).
unsafe impl Send for Pending {}
static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
fn deliver(ev: Pending) {
    match LOBBY.try_lock() {
        Ok(mut g) => apply(g.get_or_insert_with(Lobby::default), ev),
        Err(std::sync::TryLockError::Poisoned(p)) => apply(p.into_inner().get_or_insert_with(Lobby::default), ev),
        Err(std::sync::TryLockError::WouldBlock) => PENDING.lock().unwrap_or_else(|p| p.into_inner()).push(ev),
    }
}
fn drain(l: &mut Lobby) {
    let queued = std::mem::take(&mut *PENDING.lock().unwrap_or_else(|p| p.into_inner()));
    for ev in queued {
        apply(l, ev);
    }
}
/// The mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut g = LOBBY.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(l) = g.as_mut() {
        close(l);
    }
}
/// VRLobbyMenu(what, object): the Lua host's observers -- 0 the main menu's controller up, 1 down, 2 the new game's
/// screen up, 3 its dispatcher, 4 its screen down, 5 the menu at its root, 6 the menu on one of its own screens.
pub(in crate::mechanics) fn menu_event(what: i32, obj: Handle) {
    deliver(Pending::Menu(what, obj));
}
fn apply(l: &mut Lobby, ev: Pending) {
    let s = &mut l.shared;
    let (what, obj) = match ev {
        Pending::Menu(what, obj) => (what, obj),
        Pending::Saves(names) => {
            s.save_names = Some(names.split('\n').filter(|s| !s.is_empty()).map(str::to_string).collect());
            s.names_gen += 1;
            return;
        }
    };
    match what {
        0 => s.main_menu = obj,
        1 => {
            if s.main_menu.instance == obj.instance {
                s.main_menu = Handle::null();
            }
        }
        2 => s.creation = obj,
        3 => s.creation_events = obj,
        4 => {
            if s.creation.instance == obj.instance {
                s.creation = Handle::null();
            }
        }
        5 => s.menu_away = false,
        6 => s.menu_away = true,
        _ => {}
    }
}
/// VRLobbySaves(names): the main menu's saves, newest first, one per line (OnSavesForLoadReady).
pub(in crate::mechanics) fn saves(names: String) {
    deliver(Pending::Saves(names));
}
