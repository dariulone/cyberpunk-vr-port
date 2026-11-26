//! The plugin's entry points (formerly src/Main.cpp), owned by Rust from stage 2.
//!
//! WHAT RUNS WHEN. The order is the one Main.cpp had, and it is load-bearing:
//!   1. another copy of the port installed -> this one stays off, before it registers or hooks anything
//!   2. the folders an earlier release used move in; the log opens
//!   3. the natives' RTTI callbacks FIRST: cheap, and they have a window -- the graphics init after them waits on the
//!      game's device for seconds, which is how natives once came back nil in Lua
//!   4. late signature completion and the spawn system at game states Initialization/Running
//!   5. the BUILD GATE, after the natives (the scripts declaring them must still load) and before anything that
//!      touches an engine address: on another build nothing else happens and the game runs flat
//!   6. input bindings, TweakDB records, the notice, the archive, the scripts, the Lua host, prompt icons, the first
//!      launch settings, the window mode -- then the stereo boot (the D3D12CreateDevice hook must precede the
//!      device), the swapchain/overlay/OpenXR bootstrap (must precede the swapchain), the worker thread.
//! The C++ subsystems not yet in Rust are reached through `cvr_cpp_*` (src/Main.cpp, now the C++ half's seams).
use cvr_engine::game_build;
use cvr_host::guard::{guard, install_panic_log};
use cvr_host::paths::{self, Folder};
use cvr_host::{log, scripts};
use red4ext_abi::api::{self, GameApplication, GameState, GameStateType, PluginHandle, PluginInfo, Sdk};
use red4ext_abi::wide;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
};

/// The redscript release whose compiler (scc_lib.dll) ships in redscript\ -- CMakeLists.txt's CVR_REDSCRIPT_TAG,
/// passed to cargo by cmake/RustStaticlib.cmake. The C++ build's: the Rust build starts no compiler.
#[cfg(not(all(feature = "vrik", not(test))))]
const REDSCRIPT_VERSION: &str = match option_env!("CVR_REDSCRIPT_VERSION") {
    Some(v) => v,
    None => "unknown",
};

type StateFn = unsafe extern "C" fn(*mut GameApplication) -> bool;

extern "C" {
    fn cvr_cpp_init_runtime_paths();
    #[cfg(not(feature = "vrik"))]
    fn cvr_cpp_register_natives();
    #[cfg(not(feature = "vrik"))]
    fn cvr_cpp_complete_signatures(app: *mut GameApplication) -> bool;
    fn cvr_cpp_spawn_add_to_game(app: *mut GameApplication) -> bool;
    // The vendor route is Rust's with the vrik feature (rust/crates/cvr-rig/src/vrik/vendor_route.rs).
    #[cfg_attr(feature = "vrik", link_name = "cvr_vendor_route_update")]
    fn cvr_cpp_vendor_route_update(app: *mut GameApplication) -> bool;
    #[cfg_attr(feature = "vrik", link_name = "cvr_vendor_route_reset")]
    fn cvr_cpp_vendor_route_reset(app: *mut GameApplication) -> bool;
    #[cfg_attr(feature = "vrik", link_name = "cvr_vendor_route_watchdog_registered")]
    fn cvr_cpp_vendor_route_watchdog_registered(registered: bool);
    fn cvr_cpp_renderdoc_allow_nvapi();
    fn cvr_cpp_input_config_apply();
    fn cvr_cpp_tweak_records_install(handle: PluginHandle, sdk: *const Sdk);
    fn cvr_cpp_lobby_notice_install();
    fn cvr_cpp_archive_install(handle: PluginHandle, sdk: *const Sdk);
    fn cvr_cpp_lua_self_test() -> bool;
    fn cvr_cpp_lua_install();
    // The prompts' icons are Rust's with the vrik feature (rust/crates/cvr-rig/src/vrik/input_prompts.rs).
    #[cfg_attr(feature = "vrik", link_name = "cvr_input_update_prompt_icons")]
    fn cvr_cpp_update_prompt_icons(app: *mut GameApplication) -> bool;
    fn cvr_cpp_start_worker_thread();
    fn cvr_cpp_shutdown_overlay_and_xr();
    fn ApplyFirstLaunchGameSettings();
    fn ApplyLaunchWindowMode();
    fn CyberpunkVRPort_InitStereo();
    fn CyberpunkVRPort_PluginBootstrap();
    fn CyberpunkVR_CloseOwnedExecution();
}

/// The Running state's NightCoop lookup (cvr-rig vrik::nightcoop).
#[cfg(feature = "vrik")]
unsafe extern "C" fn nightcoop_update(app: *mut GameApplication) -> bool {
    // SAFETY: the game state's own call.
    unsafe { cvr_rig::vrik::nightcoop::cvr_nightcoop_update(app.cast()) }
}
static STARTED: AtomicBool = AtomicBool::new(false);
static DISABLED: AtomicBool = AtomicBool::new(false);

const fn parse_u32(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0u32;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}
const VERSION_MAJOR: u8 = parse_u32(env!("CARGO_PKG_VERSION_MAJOR")) as u8;
const VERSION_MINOR: u16 = parse_u32(env!("CARGO_PKG_VERSION_MINOR")) as u16;
const VERSION_PATCH: u32 = parse_u32(env!("CARGO_PKG_VERSION_PATCH"));

/// Said off the loader's thread: a message box on the thread that loads plugins would hold the game's start until
/// it is answered. The player is usually in the headset, so the log line comes first.
fn message(text: String) {
    std::thread::spawn(move || {
        let text = cvr_host::wide::z(text);
        let title = cvr_host::wide::z("CyberpunkVRPort");
        // SAFETY: both strings NUL-terminated and alive for the call.
        unsafe { MessageBoxW(core::ptr::null_mut(), text.as_ptr(), title.as_ptr(),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST) };
    });
}

fn unsupported_build_message() {
    let id = game_build::identity();
    let text = if id.expected == id.running {
        // Same strings: the build is known and CVR_PRETEND_UNKNOWN_BUILD=1 made it a stranger.
        format!(
            "CVR_PRETEND_UNKNOWN_BUILD=1 is set: this launch of Cyberpunk 2077 {} is treated as a build the port does \
             not know.\n\nVR is OFF for this launch: the port hooked nothing, the game runs as normal.\nUnset the \
             variable to play in VR.",
            id.running
        )
    } else {
        format!(
            "CyberpunkVRPort was made for Cyberpunk 2077 {}, and this game is {}.\n\nVR is OFF for this launch: the \
             port hooked nothing, the game runs as normal.\nInstall a CyberpunkVRPort release made for this game \
             version.",
            id.expected, id.running
        )
    };
    message(text);
}

fn other_copy_message(other: PathBuf) {
    let root = paths::dir(Folder::Root);
    let text = if other.starts_with(root) {
        format!(
            "An older copy of CyberpunkVRPort was left beside this one:\n\n{}\n\nIt started first, so it runs this \
             launch and this one stays off. It has been renamed to CyberpunkVR_Stereo.dll.old (the plugin is \
             cyberpunkvrport.dll now): start the game again.",
            other.display()
        )
    } else {
        format!(
            "Two copies of CyberpunkVRPort are installed:\n\n{}\\\n{}\n\nBoth would hook the game, so this one stays \
             off for this launch. Delete the folder of the one you do not want (an earlier release's \
             red4ext\\plugins\\CyberpunkVR_Stereo is replaced by red4ext\\plugins\\cyberpunkvrport) and start the game \
             again.",
            root.display(),
            other.display()
        )
    };
    message(text);
}

/// `GameStates::Add`; RED4ext copies the state, so it may live on the stack.
unsafe fn add_state(sdk: &Sdk, handle: PluginHandle, kind: GameStateType, enter: Option<StateFn>,
    update: Option<StateFn>, exit: Option<StateFn>) -> bool {
    let mut state = GameState { on_enter: enter, on_update: update, on_exit: exit };
    // SAFETY: RED4ext's game state table, valid for the plugin's lifetime.
    unsafe { sdk.game_states.as_ref() }.is_some_and(|g| unsafe { (g.add)(handle, kind, &mut state) })
}

/// False declines the load: RED4ext unloads the module (the desktop game was chosen in the launcher).
unsafe fn load(handle: PluginHandle, sdk_ptr: *const Sdk) -> bool {
    if STARTED.swap(true, Ordering::AcqRel) {
        return true;
    }
    install_panic_log();
    // The exports' tables, before anything renders (formerly C++ static initialisers).
    #[cfg(feature = "vrik")]
    cvr_render::export_attach::attach();
    // TWO COPIES HOOK EVERYTHING TWICE, natives included: this copy stays out before it registers or hooks anything.
    if let Some(other) = paths::retire_old_name().or_else(paths::other_copy) {
        DISABLED.store(true, Ordering::Release);
        log!("[paths] another copy of the port is installed ({}): this one ({}\\) stays off\n", other.display(),
            paths::dir(Folder::Root).display());
        other_copy_message(other);
        return true;
    }
    // Everything lives in the plugin's own folder; what an earlier release kept elsewhere moves in first.
    paths::migrate_legacy_layout();
    // SAFETY (this and every cvr_cpp_* call below): the C++ half's seams, called once from the loader's thread as
    // Main.cpp called them.
    unsafe { cvr_cpp_init_runtime_paths() };
    // The launcher: desktop or VR, before anything registers or hooks.
    #[cfg(feature = "vrik")]
    if !cvr_render::vr_core_settings::choose_vr_launch() {
        crate::input_config::retire_input_loader_copies();
        DISABLED.store(true, Ordering::Release);
        log!("[launcher] Desktop launch selected. Releasing the VR plugin before registration.\n");
        cvr_host::log::flush();
        return false;
    }
    log!("=== CyberpunkVRPort red4ext plugin loaded (no dxgi proxy) ===\n");
    log!("[rust] entry points, log, folders, build gate and script compiler in Rust (CyberpunkVRPort {}, cvr_rust {})\n",
        cvr_host::RELEASE, env!("CARGO_PKG_VERSION"));
    let sdk = unsafe { &*sdk_ptr };
    // The natives: the table is Rust's with the Rust solve (crate::natives), C++'s otherwise (src/Natives/Registration.cpp).
    #[cfg(feature = "vrik")]
    let complete_signatures: StateFn = {
        crate::natives::register();
        crate::natives::complete_signatures
    };
    #[cfg(not(feature = "vrik"))]
    let complete_signatures: StateFn = {
        unsafe { cvr_cpp_register_natives() };
        cvr_cpp_complete_signatures
    };
    // Native signatures whose types RTTI made only after registration, before any Lua.
    unsafe {
        add_state(sdk, handle, GameStateType::Initialization, Some(complete_signatures), None, None);
        add_state(sdk, handle, GameStateType::Running, Some(complete_signatures), None, None);
        // The port's spawn system joins the game's systems once there is a game instance.
        add_state(sdk, handle, GameStateType::Running, Some(cvr_cpp_spawn_add_to_game), None, None);
    }
    let watchdog = unsafe {
        add_state(sdk, handle, GameStateType::Running, None, Some(cvr_cpp_vendor_route_update),
            Some(cvr_cpp_vendor_route_reset))
    };
    unsafe { cvr_cpp_vendor_route_watchdog_registered(watchdog) };
    log!("[vendor-route] main-thread watchdog registered={}\n", i32::from(watchdog));
    // THE BUILD GATE.
    if !game_build::supported() {
        DISABLED.store(true, Ordering::Release);
        let id = game_build::identity();
        log!("[build] game {}, port made for {}: NOT the same build -- VR off, nothing installed.\n", id.running,
            id.expected);
        unsupported_build_message();
        // The one thing kept: it touches no engine address, only a resident capture layer. Without it a RenderDoc
        // launch of a build we do not know loses NVAPI and with it ray tracing.
        unsafe { cvr_cpp_renderdoc_allow_nvapi() };
        return true;
    }
    let id = game_build::identity();
    if game_build::overridden() {
        log!("[build] game {}, port made for {}: CVR_ALLOW_UNKNOWN_BUILD=1 -- every old address trusted.\n",
            id.running, id.expected);
    } else {
        log!("[build] game {} matches the port's build.\n", id.running);
    }
    unsafe {
        cvr_cpp_input_config_apply();
        cvr_cpp_tweak_records_install(handle, sdk_ptr);
        cvr_cpp_lobby_notice_install();
        cvr_cpp_archive_install(handle, sdk_ptr);
        // the Rust build compiles no script: every redscript module's work is Rust's (mechanics/reds_hooks.rs), and
        // other mods' scripts are redscript's, installed by whoever wants those mods; the copies an earlier install
        // left in r6\scripts are still retired (a separately installed redscript would compile them beside the
        // classes Rust defines)
        #[cfg(all(feature = "vrik", not(test)))]
        scripts::install_none(Some("CyberpunkVRPort_"));
        #[cfg(not(all(feature = "vrik", not(test))))]
        scripts::install_except(handle, sdk_ptr, Some("CyberpunkVRPort_"), REDSCRIPT_VERSION, &[]);
        // the script classes and methods Rust defines (formerly CyberpunkVRPort_Engine), made in the script loader's
        // last step
        #[cfg(all(feature = "vrik", not(test)))]
        log!("[script-classes] InitializeScriptRuntime hooked: {}\n", crate::script_classes::install(handle, sdk_ptr));
        // THE RUST BUILD NEVER BRINGS THE BUILT-IN LUA HOST UP: every one of the port's CET mods is Rust's (their
        // mechanics, windows, hotkeys, QuickBoot, the TweakDB names the host kept), and other mods' Lua is Cyber Engine
        // Tweaks' business, installed by whoever wants those mods. CVR_LUA (a development switch, set only by the test
        // harness for its natives probe) is the one exception. The C++ build always installs it.
        let no_lua = cfg!(all(feature = "vrik", not(test))) && std::env::var_os("CVR_LUA").is_none();
        if !no_lua && cfg!(all(feature = "vrik", not(test))) {
            log!("[lua] the built-in Lua host is installed: CVR_LUA (a development switch)\n");
        }
        if no_lua {
            // the port's mods out of CET's mods folder (an install before 0.2), their binds, the overlay key off CET's
            #[cfg(all(feature = "vrik", not(test)))]
            crate::cet_migration::run();
            #[cfg(all(feature = "vrik", not(test)))]
            crate::mechanics::NO_LUA_HOST.store(true, std::sync::atomic::Ordering::Relaxed);
            // QuickBoot (a development helper, never in a release package) runs only where the launch tools put its
            // folder, as CET ran the mod only when it was installed
            #[cfg(all(feature = "vrik", not(test)))]
            crate::mechanics::quickboot::ACTIVE.store(crate::mechanics::quickboot::installed(), std::sync::atomic::Ordering::Relaxed);
            #[cfg(all(feature = "vrik", not(test)))]
            crate::mechanics::hotkeys::ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
            #[cfg(all(feature = "vrik", not(test)))]
            crate::mechanics::dev_overlay::install();
            // the TweakDB names the host kept (TDBID.ToStringDEBUG; the mechanics match records by name)
            #[cfg(all(feature = "vrik", not(test)))]
            crate::tdb_names::install(handle, sdk_ptr);
            log!("[lua] the built-in Lua host is not installed: the port's CET mods are Rust's\n");
        } else if cvr_cpp_lua_self_test() {
            cvr_cpp_lua_install();
        }
    }
    // The key icons follow the button layout: looked up again on the main thread when it changes.
    let icons = unsafe { add_state(sdk, handle, GameStateType::Running, None, Some(cvr_cpp_update_prompt_icons), None) };
    log!("[prompt-icon] main-thread refresh registered={}\n", i32::from(icons));
    // NightCoop (co-op), when installed: looked up from the main thread, then the VR body goes to the partners as its
    // pose source (cvr-rig vrik::nightcoop; main's bd7adc549).
    #[cfg(feature = "vrik")]
    {
        let coop = unsafe { add_state(sdk, handle, GameStateType::Running, None, Some(nightcoop_update), None) };
        log!("[nightcoop] main-thread lookup registered={}\n", i32::from(coop));
    }
    // The mechanics moved from the CET mods' Lua (rust/crates/cvr-ffi/src/mechanics.rs), ticked on the main thread.
    #[cfg(feature = "vrik")]
    {
        // before the game's own update, where CET ran the Lua they came from (loader::add_running_before)
        let before = crate::loader::add_running_before(crate::mechanics::update);
        let mechanics = unsafe {
            add_state(sdk, handle, GameStateType::Running, None, (!before).then_some(crate::mechanics::update as StateFn), Some(crate::mechanics::exit))
        };
        log!("[mechanics] main-thread tick registered={} (before the game's update: {before})\n", i32::from(mechanics));
    }
    unsafe {
        ApplyFirstLaunchGameSettings();
        ApplyLaunchWindowMode();
        CyberpunkVRPort_InitStereo();
        CyberpunkVRPort_PluginBootstrap();
        cvr_cpp_start_worker_thread();
    }
    true
}

unsafe fn unload() {
    if DISABLED.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: the C++ half's shutdown seams, in Main.cpp's order: stop the main-thread watchdog and owned execution,
    // then the overlay's device references and OpenXR while D3D is still alive (RED4ext unloads plugins before the
    // process tears the graphics runtime down -- the crash-on-quit this order once fixed).
    unsafe {
        cvr_cpp_vendor_route_reset(core::ptr::null_mut());
        #[cfg(feature = "vrik")]
        cvr_rig::vrik::nightcoop::cvr_nightcoop_shutdown();
        CyberpunkVR_CloseOwnedExecution();
        cvr_cpp_shutdown_overlay_and_xr();
    }
    log!("=== CyberpunkVRPort red4ext plugin unloaded ===\n");
    cvr_host::log::flush();
}

#[no_mangle]
pub unsafe extern "C" fn Main(handle: PluginHandle, reason: u8, sdk: *const Sdk) -> bool {
    guard("Main", true, || {
        match reason {
            0 => {
                // the guarded reads' faults taken before the engine's crash handler sees them (seh.rs)
                cvr_engine::install_first_chance();
                return unsafe { load(handle, sdk) };
            }
            1 => unsafe { unload() },
            other => log!("[rust] Main with unknown reason {other}\n"),
        }
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn Query(info: *mut PluginInfo) {
    guard("Query", (), || {
        // SAFETY: RED4ext passes a writable PluginInfo.
        let Some(info) = (unsafe { info.as_mut() }) else { return };
        info.name = wide!("CyberpunkVRPort").as_ptr();
        info.author = wide!("CyberpunkVRPort").as_ptr();
        info.version = api::SemVer::new(VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH);
        // Matches the loader installed with the port (runtime 1.29.1+), which predates API v1 and skips plugins that
        // declare it outright.
        info.runtime = api::RUNTIME_INDEPENDENT;
        info.sdk = api::SDK_VERSION_COMPAT_0_5_0;
    })
}

#[no_mangle]
pub extern "C" fn Supports() -> u32 {
    api::API_VERSION_1_COMPAT_0
}

// Keep the C ABI of the game-state callbacks honest: the seams have exactly RED4ext's OnEnter/OnUpdate/OnExit type.
const _: StateFn = cvr_cpp_spawn_add_to_game;
#[allow(dead_code)]
fn _unused(_: *mut c_void) {}
