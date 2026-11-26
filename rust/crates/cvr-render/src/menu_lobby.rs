//! The main menu as a place (formerly src/Lobby/MenuLobby.cpp; Lobby/MenuLobby.hpp says what it is). Verified
//! against game 2.5 with IDA headless and a RenderDoc capture of the main menu.
//!
//! While the lobby is up (xr_lobby, engine stereo, the main menu up and not stepped aside for one of the game's own
//! screens):
//!   * the main menu world renders behind the menu (the runtime UI's render-behind-menus byte), and the menu's
//!     composition block gives the frame to the world instead of the backdrop (ComposeWorld);
//!   * the world layer is active, so the world's screens -- the lobby's among them -- spawn their widgets;
//!   * the player walks on the game's own input: gameplay input on, the main menu's UI context set aside;
//!   * the main menu's ink layer takes no input (the lobby drives the menu's controller, never its screen);
//!   * the main menu's music start is held: the intro and the lobby play their own, and the game gets it back when the
//!     main menu is the game's again or no script answers the curtain within 20 s.
//! The eyes' curtain, the loading screens after the first main menu (hung in the dark, or the eyes shut) and the
//! composite node's inputs (diagnostics) are kept here too.
//!
//! The exports, the main menu's state (engine-stereo SDK), the render node being worked, the ink system's layer and
//! the game systems' lookup stay in the C++ half, which attaches them; the detours are registered there.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use cvr_engine::code;
use cvr_host::log;
use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::engine_stereo;

/// worlduiRuntimeSystemUI.
const RUNTIME_UI_VTABLE: usize = 0x361FC10;
/// gameuiGameSystemUI -> the world's worlduiRuntimeSystemUI.
const RUNTIME_UI: usize = 0x48;
/// Written by SetRenderGameInBackground (sub_142E06DC0).
const RENDER_BEHIND_MENUS: usize = 0x264;
/// The frame graph's resource table: physical handle -> ID3D12Resource.
const RESOURCE_POOL: usize = 0x3BCDCA8;
const POOL_RESOURCES: usize = 0x2F1D8;
const POOL_STRIDE: usize = 0xB0;
/// inkWorldLayer's vtable, inkLayer::active and inkLayer::isInteractive.
const WORLD_LAYER_VTABLE: usize = 0x3165E28;
const LAYER_ACTIVE: usize = 0xB0;
const LAYER_INTERACTIVE: usize = 0xB7;
/// The local player's input: gameIPlayerManager's local player (its virtual at +0x200) holds it at +0x10. Its
/// virtuals: +0xD0 gameplay input on, +0xD8 switch it, +0x98 the UI input context on top, +0xE8 replace that context.
const PLAYER_INPUT_VTABLE: usize = 0x3077280;
const LOCAL_PLAYER: usize = 0x200;
const PLAYER_INPUT: usize = 0x10;
const GAME_INPUT_ON: usize = 0xD0;
const SET_GAME_INPUT_ON: usize = 0xD8;
const UI_CONTEXT: usize = 0x98;
const SET_UI_CONTEXT: usize = 0xE8;
/// r6\config\inputContexts.xml: the main menu's UI context, and the empty one the pre-game menu leaves behind.
const UI_MENU: u64 = 0xB599_2985_3224_746C;
const RELEASE_ALL: u64 = 0xBD3B_8A93_00D0_2CE6;
const CURTAIN_UNANSWERED: u64 = 20000;
const LEAVING_MAX: u64 = 5000;
/// ID3D12Resource::GetDesc.
const GET_DESC_SLOT: usize = 10;

pub mod slot {
    pub const WORLD_LAYER_TICK: usize = 0;
    pub const MENU_MUSIC: usize = 1;
    pub const COUNT: usize = 2;
}

/// LobbyInput: a resource the composite node resolved.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LobbyInput {
    pub logical: u32,
    pub physical: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub frame: u32,
}

/// What the C++ half attaches.
#[repr(C)]
pub struct Runtime {
    pub lobby: *mut i32,
    pub compose: *const i32,
    pub compose_stats: *const AtomicU64,
    pub compose_rejected: *mut f32,
    pub compose_rejected_node: *const AtomicU32,
    pub ambient: *mut i32,
    pub loading_in_space: *const i32,
    pub stats: *const AtomicU64,
    pub composite_node: *const AtomicU32,
    pub inputs: *mut LobbyInput,
    pub input_count: *const AtomicU32,
    /// cvr::engine_stereo::MainMenuUp / MainMenusUp (the engine-stereo SDK's).
    pub main_menu_up: extern "C" fn() -> bool,
    pub main_menus_up: extern "C" fn() -> i32,
    /// FindInkLayer("inkMenuLayer"), main thread.
    pub menu_layer: extern "C" fn() -> *mut u8,
    /// gameuiGameSystemUI and gameIPlayerManager of the current game instance.
    pub game_systems: extern "C" fn(ui: *mut *mut u8, manager: *mut *mut u8),
}
// SAFETY: exported variables and functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

/// The plugin's table is built in Rust (lobby_tables.rs).
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const crate::lobby_tables::MENU_LOBBY).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

#[no_mangle]
pub extern "C" fn cvr_menu_lobby_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

fn now() -> u64 {
    // SAFETY: no arguments.
    unsafe { GetTickCount64() }
}
fn base() -> usize {
    code::image_base()
}
fn stat(rt: &Runtime, i: usize) -> &'static AtomicU64 {
    // SAFETY: the export's twelve counters.
    unsafe { &*rt.stats.add(i) }
}
fn compose_stat(rt: &Runtime, i: usize) -> &'static AtomicU64 {
    // SAFETY: the export's six counters.
    unsafe { &*rt.compose_stats.add(i) }
}
/// The trampolines MinHook writes: the world layer's tick, the main menu's music (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_menu_lobby_originals: [AtomicPtr<c_void>; slot::COUNT] = [const { AtomicPtr::new(core::ptr::null_mut()) }; slot::COUNT];
fn original(_rt: &Runtime, index: usize) -> *mut c_void {
    cvr_menu_lobby_originals[index].load(Ordering::Acquire)
}

static SUSPENDED: AtomicBool = AtomicBool::new(false);
static SYSTEM_UI: AtomicUsize = AtomicUsize::new(0);
static PLAYER_MANAGER: AtomicUsize = AtomicUsize::new(0);
static COMPOSED_TICK: AtomicU64 = AtomicU64::new(0);
static FORCED: AtomicBool = AtomicBool::new(false);
/// The runtime UI the byte was written to.
static FORCED_RUNTIME: AtomicUsize = AtomicUsize::new(0);
static FRAME: AtomicU32 = AtomicU32::new(0);
static CURTAIN: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0
static CURTAIN_ANSWERED: AtomicBool = AtomicBool::new(false);
static MENU_UP_TICK: AtomicU64 = AtomicU64::new(0);
static LEAVING_TICK: AtomicU64 = AtomicU64::new(0);

// ---- the switches

pub fn enabled() -> bool {
    // SAFETY: an export of the C++ half.
    runtime().is_some_and(|rt| unsafe { core::ptr::read_volatile(rt.lobby) } != 0)
}
pub fn set_enabled(on: bool) {
    if let Some(rt) = runtime() {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(rt.lobby, on as i32) };
    }
}
pub fn ambient() -> bool {
    // SAFETY: as above.
    runtime().is_some_and(|rt| unsafe { core::ptr::read_volatile(rt.ambient) } != 0)
}
pub fn set_ambient(on: bool) {
    if let Some(rt) = runtime() {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(rt.ambient, on as i32) };
    }
}
fn main_menu_up(rt: &Runtime) -> bool {
    (rt.main_menu_up)()
}

/// The lobby is the main menu: switched on, engine stereo, the main menu up and not stepped aside.
pub fn active() -> bool {
    let Some(rt) = runtime() else { return false };
    enabled() && !SUSPENDED.load(Ordering::Relaxed) && engine_stereo::enabled() && main_menu_up(rt)
}
pub fn suspend(on: bool) {
    if SUSPENDED.swap(on, Ordering::Relaxed) != on {
        log!(
            "[lobby] {}\n",
            if on {
                "steps aside for the game's menu"
            } else {
                "back"
            }
        );
    }
}
pub fn suspended() -> bool {
    SUSPENDED.load(Ordering::Relaxed)
}

// ---- the world's runtime UI

/// The world's runtime UI system, read through the game system each time: the main menu world and the game world
/// each have their own.
fn runtime_ui() -> usize {
    let system = SYSTEM_UI.load(Ordering::Acquire);
    let b = base();
    if system == 0 || b == 0 {
        return 0;
    }
    match code::read::<usize>(system + RUNTIME_UI) {
        Some(runtime)
            if runtime != 0 && code::read::<usize>(runtime) == Some(b + RUNTIME_UI_VTABLE) =>
        {
            runtime
        }
        _ => 0,
    }
}

pub fn holds_render(runtime_ui_ptr: usize) -> bool {
    FORCED.load(Ordering::Relaxed)
        && runtime_ui_ptr != 0
        && active()
        && runtime_ui_ptr == runtime_ui()
}

pub fn world_shown() -> bool {
    FORCED.load(Ordering::Relaxed)
        && active()
        && now().wrapping_sub(COMPOSED_TICK.load(Ordering::Relaxed)) < 500
}

// ---- the holds, per world-layer tick

static WORLD_LAYER_HELD: AtomicUsize = AtomicUsize::new(0);

fn hold_world_layer(rt: &Runtime, layer: usize, active: bool) {
    if code::read::<usize>(layer) != Some(base() + WORLD_LAYER_VTABLE) {
        return;
    }
    if active {
        if code::read::<u8>(layer + LAYER_ACTIVE) == Some(0)
            && code::write::<u8>(layer + LAYER_ACTIVE, 1)
        {
            stat(rt, 6).fetch_add(1, Ordering::Relaxed);
        }
        WORLD_LAYER_HELD.store(layer, Ordering::Relaxed);
    } else if WORLD_LAYER_HELD.load(Ordering::Relaxed) == layer {
        if main_menu_up(rt) {
            code::write::<u8>(layer + LAYER_ACTIVE, 0);
        }
        WORLD_LAYER_HELD.store(0, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
struct InputHold {
    input: usize,
    game_was: bool,
    ui_set: bool,
}
static INPUT_HOLD: Mutex<InputHold> = Mutex::new(InputHold {
    input: 0,
    game_was: false,
    ui_set: false,
});

/// An engine object's virtual at `slot`, under the SEH adapter.
fn virtual_fn(object: usize, slot: usize) -> Option<usize> {
    let vtable = code::read::<usize>(object)?;
    code::read::<usize>(vtable + slot).filter(|&f| f != 0)
}
fn call1(object: usize, slot: usize) -> Option<*mut c_void> {
    let f = virtual_fn(object, slot)?;
    // SAFETY: an engine virtual of a checked object, called inside the SEH adapter.
    unsafe {
        code::guarded_call4(
            f,
            object as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }
}
fn call2(object: usize, slot: usize, arg: usize) -> Option<*mut c_void> {
    let f = virtual_fn(object, slot)?;
    // SAFETY: as above.
    unsafe {
        code::guarded_call4(
            f,
            object as *mut c_void,
            arg as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }
}
fn bool_call(object: usize, slot: usize) -> Option<bool> {
    let f = virtual_fn(object, slot)?;
    // SAFETY: as above; the virtual returns bool.
    unsafe {
        code::guarded_call3(
            f,
            object as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }
}

fn player_input() -> Option<usize> {
    let manager = PLAYER_MANAGER.load(Ordering::Acquire);
    if manager == 0 {
        return None;
    }
    let player = call1(manager, LOCAL_PLAYER)? as usize;
    if player == 0 {
        return None;
    }
    let input = code::read::<usize>(player + PLAYER_INPUT)?;
    (input != 0 && code::read::<usize>(input) == Some(base() + PLAYER_INPUT_VTABLE))
        .then_some(input)
}
fn game_input_on(input: usize) -> Option<bool> {
    bool_call(input, GAME_INPUT_ON)
}
fn set_game_input_on(input: usize, on: bool) -> Option<()> {
    call2(input, SET_GAME_INPUT_ON, on as usize).map(|_| ())
}
fn ui_context(input: usize) -> Option<u64> {
    let mut name = 0u64;
    call2(input, UI_CONTEXT, &mut name as *mut u64 as usize)?;
    Some(name)
}
fn set_ui_context(input: usize, name: u64) -> Option<()> {
    call2(input, SET_UI_CONTEXT, name as usize).map(|_| ())
}

fn hold_input(rt: &Runtime, active: bool) {
    let mut hold = INPUT_HOLD.lock().unwrap_or_else(|p| p.into_inner());
    let step = |hold: &mut InputHold| -> Option<()> {
        if active {
            let Some(input) = player_input() else {
                stat(rt, 11).fetch_add(1, Ordering::Relaxed);
                return Some(());
            };
            if input != hold.input {
                *hold = InputHold {
                    input,
                    game_was: game_input_on(input)?,
                    ui_set: false,
                };
            }
            if !game_input_on(input)? {
                set_game_input_on(input, true)?;
                stat(rt, 8).fetch_add(1, Ordering::Relaxed);
            }
            if ui_context(input)? == UI_MENU {
                set_ui_context(input, RELEASE_ALL)?;
                hold.ui_set = true;
                stat(rt, 9).fetch_add(1, Ordering::Relaxed);
            }
        } else if hold.input != 0 {
            let input = hold.input;
            if main_menu_up(rt) && code::read::<usize>(input) == Some(base() + PLAYER_INPUT_VTABLE)
            {
                set_game_input_on(input, hold.game_was)?;
                if hold.ui_set && ui_context(input)? == RELEASE_ALL {
                    set_ui_context(input, UI_MENU)?;
                }
            }
            *hold = InputHold::default();
            stat(rt, 10).fetch_add(1, Ordering::Relaxed);
        }
        Some(())
    };
    if step(&mut hold).is_none() {
        *hold = InputHold::default();
    }
}

#[derive(Clone, Copy, Default)]
struct MenuLayerHold {
    layer: usize,
    was: u8,
}
static MENU_LAYER_HOLD: Mutex<MenuLayerHold> = Mutex::new(MenuLayerHold { layer: 0, was: 0 });

fn hold_menu_layer(rt: &Runtime, active: bool) {
    let mut hold = MENU_LAYER_HOLD.lock().unwrap_or_else(|p| p.into_inner());
    let layer = (rt.menu_layer)() as usize;
    let step = |hold: &mut MenuLayerHold| -> Option<()> {
        if active {
            if layer == 0 {
                return Some(());
            }
            if layer != hold.layer {
                let bytes = code::read::<[u8; 9]>(layer + LAYER_ACTIVE)?;
                *hold = MenuLayerHold {
                    layer,
                    was: bytes[7],
                };
                log!(
                    "[lobby] main menu layer: active {} interactive {} game input {} -> takes no input\n",
                    bytes[0],
                    bytes[7],
                    bytes[8]
                );
            }
            code::write::<u8>(layer + LAYER_INTERACTIVE, 0).then_some(())?;
        } else if hold.layer != 0 {
            if layer == hold.layer {
                code::write::<u8>(layer + LAYER_INTERACTIVE, hold.was).then_some(())?;
            }
            *hold = MenuLayerHold::default();
        }
        Some(())
    };
    if step(&mut hold).is_none() {
        *hold = MenuLayerHold::default();
    }
}

// ---- the main menu's music

static HELD_MUSIC: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static HELD_MUSIC_TICK: AtomicU64 = AtomicU64::new(0);
static CURTAIN_TICK: AtomicU64 = AtomicU64::new(0);

fn start_music(rt: &Runtime, music: *mut c_void) {
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void) =
            core::mem::transmute(original(rt, slot::MENU_MUSIC));
        f(music)
    }
}

fn release_menu_music(rt: &Runtime, why: &str) {
    let music = HELD_MUSIC.swap(core::ptr::null_mut(), Ordering::AcqRel);
    if !music.is_null() {
        start_music(rt, music);
        log!("[lobby] the main menu's music starts: {}\n", why);
    }
}

/// Detour_MenuMusic: the main menu's music start held while the lobby can be the main menu.
///
/// # Safety
/// Called by the engine in place of sub_14155D3F8.
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_lobby_menu_music(music: *mut c_void) {
    let Some(rt) = runtime() else { return };
    if !music.is_null() && enabled() && engine_stereo::enabled() {
        HELD_MUSIC_TICK.store(now(), Ordering::Relaxed);
        if HELD_MUSIC.swap(music, Ordering::AcqRel).is_null() {
            log!("[lobby] the main menu's music is held: the lobby has its own\n");
        }
        return;
    }
    start_music(rt, music);
}

/// Detour_WorldLayerTick: the world layer ticks on the main thread every frame, the main menu's included: the
/// lobby's per-frame holds.
///
/// # Safety
/// Called by the engine in place of sub_140607414.
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_lobby_world_layer_tick(
    layer: *mut c_void,
    tick: *mut c_void,
    jobs: *mut c_void,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    if !layer.is_null() && base() != 0 {
        stat(rt, 7).fetch_add(1, Ordering::Relaxed);
        let active = active();
        hold_world_layer(rt, layer as usize, active);
        hold_input(rt, active);
        hold_menu_layer(rt, active);
        if !HELD_MUSIC.load(Ordering::Relaxed).is_null() {
            let held = HELD_MUSIC_TICK.load(Ordering::Relaxed);
            if !enabled() && main_menu_up(rt) {
                release_menu_music(rt, "the lobby is switched off");
            } else if now().wrapping_sub(held) > CURTAIN_UNANSWERED
                && CURTAIN_TICK.load(Ordering::Relaxed) < held
            {
                release_menu_music(rt, "no word from the lobby");
            }
        }
    }
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
            core::mem::transmute(original(rt, slot::WORLD_LAYER_TICK));
        f(layer, tick, jobs)
    }
}

// ---- the main menu coming and going, the curtain, the loading screens

pub fn note_main_menu() {
    let Some(rt) = runtime() else { return };
    // The eyes shut till the lobby is up -- not for one held up while it is (the new game's screens walked).
    if (rt.main_menus_up)() <= 1 {
        CURTAIN.store(1.0f32.to_bits(), Ordering::Relaxed);
        CURTAIN_ANSWERED.store(false, Ordering::Relaxed);
    }
    trace_boot_text(|| "main menu shown".to_string());
    MENU_UP_TICK.store(now(), Ordering::Relaxed);
    let (mut system, mut manager) = (core::ptr::null_mut(), core::ptr::null_mut());
    (rt.game_systems)(&mut system, &mut manager);
    SYSTEM_UI.store(system as usize, Ordering::Release);
    PLAYER_MANAGER.store(manager as usize, Ordering::Release);
    stat(rt, 5).fetch_add(1, Ordering::Relaxed);
    if system.is_null() {
        log!(
            "[lobby] gameuiGameSystemUI not found: the main menu world will pause behind the menu\n"
        );
    }
    if manager.is_null() {
        log!("[lobby] gameIPlayerManager not found: the player cannot walk in the lobby\n");
    }
}

pub fn loading_in_space() -> bool {
    let Some(rt) = runtime() else { return false };
    // SAFETY: an export of the C++ half.
    let wanted = unsafe { core::ptr::read_volatile(rt.loading_in_space) } != 0;
    wanted
        && MENU_UP_TICK.load(Ordering::Relaxed) != 0
        && engine_stereo::loading_shown()
        && !main_menu_up(rt)
}

pub fn loading_dark() -> bool {
    let Some(rt) = runtime() else { return false };
    if engine_stereo::loading_shown() && !main_menu_up(rt) {
        LEAVING_TICK.store(0, Ordering::Relaxed);
        return !loading_in_space();
    }
    let since = LEAVING_TICK.load(Ordering::Relaxed);
    since != 0 && now().wrapping_sub(since) < LEAVING_MAX
}

pub fn note_main_menu_closed() {
    let Some(rt) = runtime() else { return };
    // The last one: the game's own closing under the one the lobby holds up for the new game's screens leaves the
    // lobby where it is.
    if active() && (rt.main_menus_up)() <= 1 {
        LEAVING_TICK.store(now(), Ordering::Relaxed);
    }
    if !HELD_MUSIC
        .swap(core::ptr::null_mut(), Ordering::AcqRel)
        .is_null()
    {
        log!("[lobby] the main menu closed before its music started\n");
    }
}

pub fn curtain() -> f32 {
    if !active() {
        return 0.0;
    }
    if !CURTAIN_ANSWERED.load(Ordering::Relaxed)
        && now().wrapping_sub(MENU_UP_TICK.load(Ordering::Relaxed)) > CURTAIN_UNANSWERED
    {
        return 0.0;
    }
    f32::from_bits(CURTAIN.load(Ordering::Relaxed))
}

/// Why the curtain is what it is: 0 the lobby's word, 1 off (switched off), 2 stepped aside, 3 no engine stereo, 4 no
/// main menu, 5 unanswered too long, 6 shut till the lobby answers.
pub fn curtain_why() -> u32 {
    let Some(rt) = runtime() else { return 4 };
    if !enabled() {
        1
    } else if SUSPENDED.load(Ordering::Relaxed) {
        2
    } else if !engine_stereo::enabled() {
        3
    } else if !main_menu_up(rt) {
        4
    } else if !CURTAIN_ANSWERED.load(Ordering::Relaxed) {
        if now().wrapping_sub(MENU_UP_TICK.load(Ordering::Relaxed)) > CURTAIN_UNANSWERED {
            5
        } else {
            6
        }
    } else {
        0
    }
}

/// CVR_TRACE_BOOT=1: a line for every captured frame from the boot's screen until 20 s after the main menu came up
/// (the wall clock in ms, the curtain and why, the notice, the intro's overlay, how the frame is taken), to set beside
/// stills of the window.
pub fn trace_boot_frame(curtain: f32, intro: bool, notice: (bool, bool, f32), menu_open: bool, split: bool) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("CVR_TRACE_BOOT").is_some()) {
        return;
    }
    let up = MENU_UP_TICK.load(Ordering::Relaxed);
    if up != 0 && now().wrapping_sub(up) > 20000 {
        return;
    }
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    log!(
        "[boot-trace] {wall} curtain {curtain:.2} why {} answered {:.2} notice shown {} live {} op {:.2} intro {intro} menu_open {menu_open} split {split} world_shown {}\n",
        curtain_why(),
        f32::from_bits(CURTAIN.load(Ordering::Relaxed)),
        notice.0,
        notice.1,
        notice.2,
        world_shown()
    );
}

/// CVR_TRACE_BOOT=1: a capture that did not happen (its presented frame went out without the curtain drawn on it).
pub fn trace_boot_skip(why: &str) {
    if std::env::var_os("CVR_TRACE_BOOT").is_none() {
        return;
    }
    let up = MENU_UP_TICK.load(Ordering::Relaxed);
    if up != 0 && now().wrapping_sub(up) > 20000 {
        return;
    }
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    log!("[boot-trace] {wall} skipped: {why} (curtain {:.2} why {})\n", curtain(), curtain_why());
}

/// CVR_TRACE_BOOT=1: a line of the frame loop's (the XR thread's) in the same window as the captures'.
pub fn trace_boot_text(text: impl FnOnce() -> String) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("CVR_TRACE_BOOT").is_some()) {
        return;
    }
    let up = MENU_UP_TICK.load(Ordering::Relaxed);
    if up != 0 && now().wrapping_sub(up) > 20000 {
        return;
    }
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    log!("[boot-trace] {wall} {}\n", text());
}

pub fn set_curtain(closure: f32) {
    CURTAIN.store(closure.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    CURTAIN_ANSWERED.store(true, Ordering::Relaxed);
    CURTAIN_TICK.store(now(), Ordering::Relaxed);
}

/// Tick: the main menu world renders behind the menu while the lobby is up; the byte is taken back when the runtime
/// UI is there again, and only where it was written.
pub fn tick() {
    let Some(rt) = runtime() else { return };
    FRAME.fetch_add(1, Ordering::Relaxed);
    if !main_menu_up(rt) {
        SUSPENDED.store(false, Ordering::Relaxed);
    }
    let active = active();
    let forced = FORCED.load(Ordering::Relaxed);
    if !active && !forced {
        return;
    }
    let runtime = runtime_ui();
    if runtime == 0 {
        stat(rt, 3).fetch_add(1, Ordering::Relaxed);
        return;
    }
    if active {
        if code::read::<u8>(runtime + RENDER_BEHIND_MENUS) == Some(0)
            && code::write::<u8>(runtime + RENDER_BEHIND_MENUS, 1)
        {
            stat(rt, 1).fetch_add(1, Ordering::Relaxed);
            if !forced {
                log!("[lobby] the main menu world renders behind the menu\n");
            }
        }
        FORCED.store(true, Ordering::Relaxed);
        FORCED_RUNTIME.store(runtime, Ordering::Relaxed);
    } else {
        if runtime == FORCED_RUNTIME.load(Ordering::Relaxed)
            && code::write::<u8>(runtime + RENDER_BEHIND_MENUS, 0)
        {
            stat(rt, 2).fetch_add(1, Ordering::Relaxed);
            log!("[lobby] the world no longer renders behind menus\n");
        }
        FORCED.store(false, Ordering::Relaxed);
        FORCED_RUNTIME.store(0, Ordering::Relaxed);
    }
}

// ---- the composition block

/// The composition node (sub_14033E388): it builds the block in its own stack frame and uploads 0x1B0 bytes from
/// there. Rows 22-26 are written only with render feature 30, never in the main menu: whatever the stack held (IDA
/// 2.5, 2026-11-09). The test used to call them a repeat of rows 4-8 -- true by the stack's chance alone, and the Rust
/// build's frames left other words there. So the block is the composition's when it is uploaded under that node.
pub const COMPOSITION_NODE: usize = 0x33E388;

/// fxCompositionShaderParams (432 bytes): 0 when the block is the main menu's composition, else what it failed
/// (2 weights, 3 target size, 4 not uploaded under the composition node, 5 grading values). Every word read must be
/// what this block holds: a weight read from an integer is a denormal, and 1.0 written over an index faults the GPU.
pub fn composition_block(b: &[f32; 108], under_composition_node: bool) -> usize {
    let weight = |v: f32| v == 0.0 || (1e-3..=1.0).contains(&v);
    if [4, 5, 6, 7, 8].iter().any(|&i| !weight(b[i])) {
        return 2;
    }
    let (w, h) = (b[16 * 4 + 2], b[16 * 4 + 3]);
    if !((64.0..=16384.0).contains(&w) && (64.0..=16384.0).contains(&h))
        || w != w as i32 as f32
        || h != h as i32 as f32
    {
        return 3;
    }
    if !under_composition_node {
        return 4;
    }
    // The grading is real: saturation set, aberration tiny.
    let (sat, ab) = (b[6 * 4 + 2], b[6 * 4 + 3]);
    if (1e-3..=4.0).contains(&sat) && (0.0..0.01).contains(&ab) {
        0
    } else {
        5
    }
}

fn node_rva() -> Option<u32> {
    let b = base();
    let node = crate::render_tls::current_node_work();
    (b != 0 && node > b).then(|| (node - b) as u32)
}

/// ComposeWorld: the menu's composition block gives the frame to the world, not the backdrop.
///
/// # Safety
/// `block` is null or the 432-byte block being uploaded.
pub unsafe fn compose_world(block: *mut f32) -> bool {
    let Some(rt) = runtime() else { return false };
    // SAFETY: an export of the C++ half.
    if block.is_null() || unsafe { core::ptr::read_volatile(rt.compose) } == 0 || !active() {
        return false;
    }
    compose_stat(rt, 0).fetch_add(1, Ordering::Relaxed);
    // SAFETY: the caller's block.
    let b = unsafe { &mut *(block as *mut [f32; 108]) };
    let why = composition_block(b, node_rva() == Some(COMPOSITION_NODE as u32));
    if why != 0 {
        compose_stat(rt, why).fetch_add(1, Ordering::Relaxed);
        // SAFETY: the export, 108 floats.
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), rt.compose_rejected, 108) };
        if let Some(node) = node_rva() {
            // SAFETY: the export.
            unsafe { (*rt.compose_rejected_node).store(node, Ordering::Relaxed) };
        }
        return false;
    }
    compose_stat(rt, 1).fetch_add(1, Ordering::Relaxed);
    b[1 * 4 + 2] = 1.0;
    b[1 * 4 + 3] = 0.0;
    b[2 * 4] = 0.0;
    if let Some(node) = node_rva() {
        // SAFETY: the export.
        unsafe { (*rt.composite_node).store(node, Ordering::Relaxed) };
    }
    COMPOSED_TICK.store(now(), Ordering::Relaxed);
    stat(rt, 0).fetch_add(1, Ordering::Relaxed);
    true
}

/// The resource behind a physical handle of the frame graph: its format and size.
fn describe(physical: u32, row: &mut LobbyInput) -> bool {
    if physical == 0 || physical > 0x100000 {
        return false;
    }
    let Some(pool) = code::read::<usize>(base() + RESOURCE_POOL).filter(|&p| p != 0) else {
        return false;
    };
    let Some(resource) =
        code::read::<usize>(pool + POOL_RESOURCES + (physical as usize - 1) * POOL_STRIDE)
            .filter(|&r| r != 0)
    else {
        return false;
    };
    let Some(get_desc) = virtual_fn(resource, GET_DESC_SLOT * 8) else {
        return false;
    };
    let mut desc = D3D12_RESOURCE_DESC::default();
    // SAFETY: ID3D12Resource::GetDesc (this, the returned struct's buffer), inside the SEH adapter.
    let called = unsafe {
        code::guarded_call4(
            get_desc,
            resource as *mut c_void,
            &mut desc as *mut D3D12_RESOURCE_DESC as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    };
    if called.is_none() {
        return false;
    }
    row.format = desc.Format.0 as u32;
    row.width = desc.Width as u32;
    row.height = desc.Height;
    true
}

static OBSERVED_FRAME: AtomicU32 = AtomicU32::new(0);

/// ObserveResolve: the resources the composite node resolves, one frame's worth (diagnostics).
pub fn observe_resolve(logical: u32, physical: u32) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the export.
    let node = unsafe { (*rt.composite_node).load(Ordering::Relaxed) };
    let b = base();
    if node == 0 || b == 0 || crate::render_tls::current_node_work() != b + node as usize || !active() {
        return;
    }
    let frame = FRAME.load(Ordering::Relaxed);
    // SAFETY: the exports.
    let count = unsafe { &*rt.input_count };
    if OBSERVED_FRAME.swap(frame, Ordering::Relaxed) != frame {
        count.store(0, Ordering::Relaxed);
    }
    let n = count.load(Ordering::Relaxed);
    if n >= 16 {
        return;
    }
    let mut row = LobbyInput {
        logical,
        physical,
        format: 0,
        width: 0,
        height: 0,
        frame,
    };
    describe(physical, &mut row);
    // SAFETY: the export, sixteen rows.
    unsafe { *rt.inputs.add(n as usize) = row };
    count.store(n + 1, Ordering::Relaxed);
    stat(rt, 4).fetch_add(1, Ordering::Relaxed);
}

// ---- the C++ names

#[no_mangle]
pub extern "C" fn cvr_menu_lobby_enabled() -> bool {
    enabled()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_set_enabled(on: bool) {
    set_enabled(on)
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_ambient() -> bool {
    ambient()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_set_ambient(on: bool) {
    set_ambient(on)
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_suspend(on: bool) {
    suspend(on)
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_suspended() -> bool {
    suspended()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_note_main_menu() {
    note_main_menu()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_holds_render(runtime_ui: *const c_void) -> bool {
    holds_render(runtime_ui as usize)
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_world_shown() -> bool {
    world_shown()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_loading_dark() -> bool {
    loading_dark()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_loading_in_space() -> bool {
    loading_in_space()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_note_main_menu_closed() {
    note_main_menu_closed()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_curtain() -> f32 {
    curtain()
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_set_curtain(closure: f32) {
    set_curtain(closure)
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_tick() {
    tick()
}
/// # Safety
/// As [`compose_world`].
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_lobby_compose_world(block: *mut f32) -> bool {
    // SAFETY: forwarded.
    unsafe { compose_world(block) }
}
#[no_mangle]
pub extern "C" fn cvr_menu_lobby_observe_resolve(logical: u32, physical: u32) {
    observe_resolve(logical, physical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_block() -> [f32; 108] {
        let mut b = [0.0f32; 108];
        b[4] = 1.0; // uiFactor
        b[5] = 0.0; // uiPassthroughFactor
        b[6] = 0.0; // mainRenderFactor
        b[7] = 1.0; // blurredRenderFactor
        b[8] = 1.0; // backgroundTextureFactor
        for (i, v) in b[16..34].iter_mut().enumerate() {
            *v = 0.25 + i as f32 * 0.01;
        }
        b[6 * 4 + 2] = 1.0; // saturation
        b[6 * 4 + 3] = 0.001; // aberration
        b[16 * 4 + 2] = 4128.0;
        b[16 * 4 + 3] = 2232.0;
        let grading: Vec<f32> = b[16..34].to_vec();
        b[88..106].copy_from_slice(&grading);
        b[106] = f32::from_bits(0x7FFE_6559); // padding: stack left-overs
        b
    }

    #[test]
    fn the_composition_block_is_recognised_word_for_word() {
        let b = menu_block();
        assert_eq!(composition_block(&b, true), 0);
        // A weight that is really an integer (a denormal) or above 1.
        let mut w = b;
        w[6] = f32::from_bits(3);
        assert_eq!(composition_block(&w, true), 2);
        w = b;
        w[8] = 2.0;
        assert_eq!(composition_block(&w, true), 2);
        // The target: whole pixels in range.
        let mut t = b;
        t[16 * 4 + 2] = 4128.5;
        assert_eq!(composition_block(&t, true), 3);
        t = b;
        t[16 * 4 + 3] = 32.0;
        assert_eq!(composition_block(&t, true), 3);
        // Another node's 432 bytes are not the composition, whatever they hold.
        assert_eq!(composition_block(&b, false), 4);
        // Rows 22-26 are the stack's: anything there leaves the block the composition's.
        let mut g = b;
        for v in &mut g[88..108] {
            *v = f32::from_bits(0x0000_01F9);
        }
        assert_eq!(composition_block(&g, true), 0);
        // Real grading values.
        let mut v = b;
        v[6 * 4 + 3] = 0.5;
        assert_eq!(composition_block(&v, true), 5);
    }
    #[test]
    fn nothing_without_the_cpp_half() {
        assert!(!active());
        assert_eq!(curtain(), 0.0);
        assert!(!loading_dark());
        let mut b = menu_block();
        // SAFETY: a local block.
        assert!(!unsafe { compose_world(b.as_mut_ptr()) });
    }
}
