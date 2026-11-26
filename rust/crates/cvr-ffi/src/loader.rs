//! THE PORT'S OWN LOADER (2026-11-24): RED4ext's part of the port built in, so a player installs one thing. The port
//! lives in its own folder at the game's root (<game>\cyberpunkvrport\), loaded by a proxy of the system's
//! powrprof.dll in bin\x64 (rust/crates/cvr-loader: the game imports it for one function, no other mod uses it --
//! RED4ext's loader is winmm.dll, CET's version.dll). The proxy loads the port when the game's WinMain is entered
//! (where RED4ext loads its plugins: the game's statics are made by then) and calls `CyberpunkVRPort_LoaderAttach`;
//! from there this module does for the port what RED4ext does for a plugin:
//!
//!   addresses   resolved by red4ext-abi::reloc -- RED4ext's resolver when RED4ext runs, else the game's own address
//!               library beside its executable (cyberpunk2077_addresses.json), the file RED4ext reads;
//!   Main(Load)  at once, inside WinMain before the game's own (the proxy found WinMain by hash 240386859);
//!   game states CGameApplication::AddState (hash 4223801011) hooked: each state's class gets OnEnter / OnUpdate /
//!               OnExit wrappers in its vtable's slots 3..5 calling the callbacks the plugin added (each called until
//!               it returns true; BaseInitialization's update holds the state while one is pending, its status set as
//!               RED4ext sets it);
//!   quick_exit  (hash 388826167) hooked: Main(Unload) while D3D is still alive;
//!   hooking     MinHook (the plugin's attach / detach);
//!   scripts     none: the Rust build compiles no redscript.
//!
//! RED4ext installed beside it (for other mods) is no conflict: its plugins load as before (its WinMain hook runs right
//! after the port's start); both hook AddState and quick_exit (MinHook and Detours each carry the other's jump), and
//! each wraps the state vtables over the other's wrappers. An old install of the port under red4ext\plugins is retired first (its settings copied over, its
//! DLL renamed .old), so RED4ext never loads a second copy.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
use cvr_engine::hooks::{install_chain_on_any_build, Detour};
use cvr_host::log;
use red4ext_abi::api::*;
use std::sync::Mutex;

const ADD_STATE_HASH: u32 = 4_223_801_011;
const QUICK_EXIT_HASH: u32 = 388_826_167;

/// This module's HMODULE (the plugin handle the plugin's Main is given).
static MODULE: AtomicUsize = AtomicUsize::new(0);
static ATTACHED: AtomicBool = AtomicBool::new(false);
static UNLOADED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------- the SDK the plugin is given
static RUNTIME: SemVer = SemVer::new(1, 29, 1);
static GAME_STATES: GameStates = GameStates { add: states_add };
static HOOKING: Hooking = Hooking { attach: hook_attach, detach: hook_detach };
static SCRIPTS: Scripts = Scripts { add: scripts_add, register_never_ref_type: scripts_type, register_mixed_ref_type: scripts_type };
struct SdkCell(Sdk);
// SAFETY: pointers to the statics above, never written.
unsafe impl Sync for SdkCell {}
static SDK: SdkCell = SdkCell(Sdk {
    runtime: &RUNTIME,
    logger: core::ptr::null(),
    hooking: &HOOKING,
    game_states: &GAME_STATES,
    scripts: &SCRIPTS,
});

unsafe extern "C" fn scripts_add(_handle: PluginHandle, _path: *const u16) -> bool {
    false
}
unsafe extern "C" fn scripts_type(_name: *const u8) -> bool {
    false
}
/// The plugin's own hooks through MinHook, enabled at once.
unsafe extern "C" fn hook_attach(_handle: PluginHandle, target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> bool {
    if target.is_null() || detour.is_null() {
        return false;
    }
    let d = [Detour { name: "a plugin hook", target: target as usize, absolute: true, detour, original, expect: None }];
    // SAFETY: the plugin vouches for the detour's signature, as with RED4ext's attach.
    let ok = unsafe { install_chain_on_any_build("loader:plugin", &d) }.is_ok();
    if !ok {
        log!("[loader] a plugin hook at {target:p} could not be attached\n");
    }
    ok
}
unsafe extern "C" fn hook_detach(_handle: PluginHandle, target: *mut c_void) -> bool {
    // SAFETY: MinHook's own disable and removal of a hook it made.
    unsafe { cvr_engine::hooks::remove_one(target) }
}

// ---------------------------------------------------------------- the game states
type StateCallback = unsafe extern "C" fn(*mut GameApplication) -> bool;
#[derive(Default)]
struct Lists {
    enter: Vec<StateCallback>,
    update: Vec<StateCallback>,
    exit: Vec<StateCallback>,
}
static STATES: Mutex<[Option<Lists>; 4]> = Mutex::new([None, None, None, None]);
/// The Running state's callbacks run BEFORE the game's own update -- CET's order (GameMainThread::HookStateTick drains
/// its tasks, every mod's onUpdate among them, and then calls CRunningState::OnTick), where RED4ext's run after it. The
/// mechanics moved from the CET mods' Lua keep the Lua's place in the frame: run after the game's update, VRIK's
/// camera / entity pair was read with the world already ticked, and in a moving car the camera stood up to 0.3 m off
/// the seat from frame to frame (the body shook).
static RUNNING_BEFORE: Mutex<Vec<StateCallback>> = Mutex::new(Vec::new());
/// A Running-state update run before the game's own, as CET runs its onUpdate. False when this loader is not the one
/// that started the plugin (its callbacks then go to the host's own list).
pub fn add_running_before(f: StateCallback) -> bool {
    if !ATTACHED.load(Acquire) {
        return false;
    }
    RUNNING_BEFORE.lock().unwrap_or_else(|p| p.into_inner()).push(f);
    true
}
/// The before-update callbacks, each dropped once it returns true (as `run`).
fn run_before(app: *mut GameApplication) {
    let pending = core::mem::take(&mut *RUNNING_BEFORE.lock().unwrap_or_else(|p| p.into_inner()));
    if pending.is_empty() {
        return;
    }
    let mut kept = Vec::new();
    for f in pending {
        // SAFETY: a callback the plugin added for this state, called on the game's thread with the application.
        let done = cvr_host::guard::guard("a game state callback", true, || unsafe { f(app) });
        if !done {
            kept.push(f);
        }
    }
    let mut list = RUNNING_BEFORE.lock().unwrap_or_else(|p| p.into_inner());
    kept.append(&mut list);
    *list = kept;
}

unsafe extern "C" fn states_add(_handle: PluginHandle, kind: GameStateType, state: *mut GameState) -> bool {
    // SAFETY: the caller's GameState, copied.
    let Some(s) = (unsafe { state.as_ref() }) else { return false };
    let i = kind as usize;
    if i >= 4 {
        return false;
    }
    let mut g = STATES.lock().unwrap_or_else(|p| p.into_inner());
    let lists = g[i].get_or_insert_with(Lists::default);
    if let Some(f) = s.on_enter {
        lists.enter.push(f);
    }
    if let Some(f) = s.on_update {
        lists.update.push(f);
    }
    if let Some(f) = s.on_exit {
        lists.exit.push(f);
    }
    true
}
/// The callbacks of one list, each dropped once it returns true (RED4ext's StateSystem::Run): true when none is left
/// pending. The list is taken out while they run (a callback may add a state).
fn run(kind: usize, which: u8, app: *mut GameApplication) -> bool {
    let pending = {
        let mut g = STATES.lock().unwrap_or_else(|p| p.into_inner());
        let Some(lists) = g[kind].as_mut() else { return true };
        let list = match which {
            0 => &mut lists.enter,
            1 => &mut lists.update,
            _ => &mut lists.exit,
        };
        core::mem::take(list)
    };
    let mut kept = Vec::new();
    for f in pending {
        // SAFETY: a callback the plugin added for this state, called on the game's thread with the application.
        let done = cvr_host::guard::guard("a game state callback", true, || unsafe { f(app) });
        if !done {
            kept.push(f);
        }
    }
    let all_done = kept.is_empty();
    let mut g = STATES.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(lists) = g[kind].as_mut() {
        let list = match which {
            0 => &mut lists.enter,
            1 => &mut lists.update,
            _ => &mut lists.exit,
        };
        // the ones added while they ran stay after the kept ones
        kept.append(list);
        *list = kept;
    }
    all_done
}

type StateFn = unsafe extern "C" fn(*mut c_void, *mut GameApplication) -> bool;
/// The originals of each state class's slots 3..5 (OnEnter, OnUpdate, OnExit), by type.
static ORIGINALS: [[AtomicUsize; 3]; 4] = [const { [const { AtomicUsize::new(0) }; 3] }; 4];
/// CGameApplication.status (RED4ext/GameApplication.hpp: EGameStateStatus at 0x2A0): Initialized, Ran.
const APP_STATUS_OFFSET: usize = 0x2A0;
const APP_STATUS_INITIALIZED: u32 = 1;
const APP_STATUS_RAN: u32 = 2;

unsafe fn original(kind: usize, slot: usize, this: *mut c_void, app: *mut GameApplication) -> bool {
    let f = ORIGINALS[kind][slot].load(Acquire);
    if f == 0 {
        return true;
    }
    // SAFETY: the slot's own function, saved before it was replaced.
    unsafe { core::mem::transmute::<usize, StateFn>(f)(this, app) }
}
unsafe extern "C" fn on_enter<const K: usize>(this: *mut c_void, app: *mut GameApplication) -> bool {
    if K == GameStateType::Running as usize {
        // the scripts are loaded and every native registered: the RTTI's functions are all there from here on
        crate::script_call::mark_rtti_complete();
    }
    // SAFETY: the game's call, passed on.
    let result = unsafe { original(K, 0, this, app) };
    run(K, 0, app);
    result
}
unsafe extern "C" fn on_update<const K: usize>(this: *mut c_void, app: *mut GameApplication) -> bool {
    if K == GameStateType::Running as usize {
        // also here: a port attached while the game already ran saw no OnEnter of it
        crate::script_call::mark_rtti_complete();
        run_before(app);
    }
    // SAFETY: as above.
    let result = unsafe { original(K, 1, this, app) };
    let ours = run(K, 1, app);
    if K == GameStateType::BaseInitialization as usize {
        // The base initialization is held while a callback is pending; the game may SetState meanwhile, so its
        // status is set from the answer (RED4ext's BaseInitializationState::OnUpdate).
        let done = ours && result;
        // SAFETY: the application's status word.
        unsafe { set_status(app, if done { APP_STATUS_RAN } else { APP_STATUS_INITIALIZED }) };
        return done;
    }
    result
}
unsafe extern "C" fn on_exit<const K: usize>(this: *mut c_void, app: *mut GameApplication) -> bool {
    run(K, 2, app);
    // SAFETY: as above.
    unsafe { original(K, 2, this, app) }
}
/// The application's state status.
unsafe fn set_status(app: *mut GameApplication, status: u32) {
    if !app.is_null() {
        // SAFETY: the SDK's layout of the application.
        unsafe { core::ptr::write_volatile((app as *mut u8).add(APP_STATUS_OFFSET) as *mut u32, status) };
    }
}

const WRAPPERS: [[StateFn; 3]; 4] = [
    [on_enter::<0>, on_update::<0>, on_exit::<0>],
    [on_enter::<1>, on_update::<1>, on_exit::<1>],
    [on_enter::<2>, on_update::<2>, on_exit::<2>],
    [on_enter::<3>, on_update::<3>, on_exit::<3>],
];

/// The state object's class: slots 3..5 of its vtable replaced by the wrappers of its type (once per class).
unsafe fn wrap_state(state: *mut c_void) {
    if state.is_null() {
        return;
    }
    // SAFETY: an IGameState: its vtable, GetType at slot 2.
    let vtable = unsafe { *(state as *const *mut usize) };
    type GetType = unsafe extern "C" fn(*mut c_void) -> u32;
    let kind = unsafe { core::mem::transmute::<usize, GetType>(*vtable.add(2))(state) } as usize;
    if kind >= 4 {
        log!("[loader] a game state of unknown type {kind}: left as it is\n");
        return;
    }
    for slot in 0..3 {
        let at = unsafe { vtable.add(3 + slot) };
        let wrapper = WRAPPERS[kind][slot] as usize;
        let current = unsafe { core::ptr::read_volatile(at) };
        if current == wrapper {
            continue;
        }
        ORIGINALS[kind][slot].store(current, Release);
        let mut old = 0u32;
        // SAFETY: the vtable's slot made writable for the write, its protection put back.
        unsafe {
            use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
            if VirtualProtect(at.cast(), 8, PAGE_READWRITE, &mut old) == 0 {
                log!("[loader] the game state {kind}'s slot {} could not be made writable\n", 3 + slot);
                continue;
            }
            core::ptr::write_volatile(at, wrapper);
            VirtualProtect(at.cast(), 8, old, &mut old);
        }
    }
    log!("[loader] game state {} wrapped\n", ["BaseInitialization", "Initialization", "Running", "Shutdown"][kind]);
}

// ---------------------------------------------------------------- the game's hooks
static mut ORIGINAL_ADD_STATE: *mut c_void = core::ptr::null_mut();
static mut ORIGINAL_QUICK_EXIT: *mut c_void = core::ptr::null_mut();

unsafe extern "C" fn add_state(app: *mut GameApplication, state: *mut c_void) -> bool {
    // SAFETY: the game's state object, before it is added.
    unsafe { wrap_state(state) };
    type F = unsafe extern "C" fn(*mut GameApplication, *mut c_void) -> bool;
    // SAFETY: the trampoline.
    unsafe { core::mem::transmute::<*mut c_void, F>(ORIGINAL_ADD_STATE)(app, state) }
}
unsafe extern "C" fn quick_exit(code: i32) {
    if !UNLOADED.swap(true, AcqRel) {
        log!("[loader] quick_exit({code}): the port unloads\n");
        // SAFETY: the plugin's entry with Unload, once.
        unsafe { crate::plugin::Main(MODULE.load(Acquire) as PluginHandle, MainReason::Unload as u8, &SDK.0) };
    }
    type F = unsafe extern "C" fn(i32);
    // SAFETY: the trampoline; it does not return.
    unsafe { core::mem::transmute::<*mut c_void, F>(ORIGINAL_QUICK_EXIT)(code) }
}

fn red4ext_present() -> bool {
    let name: Vec<u16> = "RED4ext.dll\0".encode_utf16().collect();
    // SAFETY: a plain lookup.
    !unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name.as_ptr()) }.is_null()
}

/// The proxy's call at the game's WinMain: the hooks made, then the plugin's Main(Load). False when the port does not
/// start (the log says why); the game then runs as it is.
///
/// # Safety
/// Called once by bin\x64\powrprof.dll, on the game's main thread in WinMain, with this module's handle.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVRPort_LoaderAttach(module: *mut c_void) -> bool {
    if ATTACHED.swap(true, AcqRel) {
        return true;
    }
    MODULE.store(module as usize, Release);
    log!("=== CyberpunkVRPort: the port's own loader ({}) ===\n", cvr_host::paths::dir(cvr_host::paths::Folder::Root).display());
    // An install of the port under red4ext\plugins: its settings come here, its DLL is retired before RED4ext looks.
    cvr_host::paths::retire_red4ext_install();
    let resolve = |hash: u32, what: &str| {
        let a = red4ext_abi::reloc::resolve(hash);
        if a.is_none() {
            log!("[loader] {what} (hash {hash}) is not in this game's address library: the port does not start\n");
        }
        a
    };
    let (Some(add_state_at), Some(exit_at)) = (resolve(ADD_STATE_HASH, "CGameApplication::AddState"), resolve(QUICK_EXIT_HASH, "quick_exit")) else {
        return false;
    };
    log!(
        "[loader] addresses from {} ({} entries read from the game's library): AddState +{:#x}, quick_exit +{:#x}\n",
        if red4ext_present() { "RED4ext" } else { "cyberpunk2077_addresses.json" },
        red4ext_abi::reloc::library_entries(),
        add_state_at - red4ext_abi::reloc::image_base(),
        exit_at - red4ext_abi::reloc::image_base()
    );
    let detours = [
        Detour { name: "CGameApplication::AddState", target: add_state_at, absolute: true, detour: add_state as *const c_void, original: &raw mut ORIGINAL_ADD_STATE, expect: None },
        Detour { name: "quick_exit", target: exit_at, absolute: true, detour: quick_exit as *const c_void, original: &raw mut ORIGINAL_QUICK_EXIT, expect: None },
    ];
    // SAFETY: each detour has its target's signature (AddState(app, state) -> bool, quick_exit(int)).
    if let Err(e) = unsafe { install_chain_on_any_build("loader", &detours) } {
        log!("[loader] the loader's hooks: {e}\n");
        return false;
    }
    log!("[loader] WinMain: the port starts (its own loader{})\n", if red4ext_present() { ", RED4ext runs beside it" } else { ", no RED4ext" });
    // SAFETY: the plugin's entry, as RED4ext calls it: our module, Load, the SDK above.
    let loaded = unsafe { crate::plugin::Main(module, MainReason::Load as u8, &SDK.0) };
    if !loaded {
        log!("[loader] the plugin declined the load: the game runs without it\n");
    }
    loaded
}
