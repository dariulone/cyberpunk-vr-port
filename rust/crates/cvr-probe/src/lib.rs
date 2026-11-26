//! cvr_rust_probe.dll: stage 1's second experiment (docs/rust-migration-plan.md). A RED4ext plugin written in Rust
//! end to end, loaded INSTEAD of the port, never beside it:
//!
//!   Query/Supports/Main  the exports RED4ext looks for, with the port's own Query values
//!   CVRRustProbe         a script native (Int32 -> Int32, value + 1), registered in the RTTI post-register window
//!   scripts\             cvr_rust_probe.reds calls it from the main menu, compiled by the plugin's own scc_lib
//!   rtti_natives.json    every native global function the game's RTTI holds once Running begins
//!   Unload               logged and flushed: the exit is clean when the log ends with it and the game exits 0
//!
//! With CVR_PROBE_DUMP_ONLY=1 it registers nothing, compiles nothing and only writes the dump -- the way to read the
//! C++ port's natives (names, parameter and return types) from the running game for the stage 0 baseline.
use cvr_host::guard::{guard, install_panic_log};
use cvr_host::paths::{self, Folder};
use cvr_host::{log, scripts};
use red4ext_abi::api::{self, GameApplication, GameState, GameStateType, PluginHandle, PluginInfo, Sdk};
use red4ext_abi::function::{function_flags, GlobalFunction};
use red4ext_abi::rtti::RttiSystem;
use red4ext_abi::stack::StackFrame;
use red4ext_abi::wide;
use std::ffi::c_void;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const REDSCRIPT_VERSION: &str = "1.0.0-preview.22";

static STARTED: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);

fn dump_only() -> bool {
    std::env::var_os("CVR_PROBE_DUMP_ONLY").is_some_and(|v| v == "1")
}

/// `CVRRustProbe(value: Int32) -> Int32`: value + 1, each call logged.
unsafe extern "C" fn cvr_rust_probe(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    guard("CVRRustProbe", (), || {
        // SAFETY: the VM passes its frame positioned at our single Int32 parameter.
        let frame = unsafe { &mut *frame };
        let mut value: i32 = 0;
        unsafe { frame.get_parameter(&mut value) };
        frame.finish_params();
        let result = value.wrapping_add(1);
        let n = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
        log!("[probe] CVRRustProbe({value}) -> {result} (call {n})\n");
        if !out.is_null() {
            unsafe { *(out as *mut i32) = result };
        }
    })
}

/// RTTI's post-register window: every native type exists, the scripts are not loaded yet.
extern "C" fn register_natives() {
    guard("register_natives", (), || {
        let Some(rtti) = RttiSystem::get() else {
            log!("[probe] CRTTISystem::Get is not resolved: no natives\n");
            return;
        };
        let Some(f) = GlobalFunction::create(c"CVRRustProbe", c"CVRRustProbe", cvr_rust_probe) else {
            log!("[probe] CGlobalFunction could not be made\n");
            return;
        };
        f.base.flags = function_flags::IS_NATIVE | function_flags::IS_STATIC;
        let returns = f.base.set_return_type(rtti, c"Int32");
        let param = f.base.add_param(rtti, c"Int32", c"value", false, false);
        // SAFETY: made by GlobalFunction::create from PoolRTTIFunction.
        unsafe { rtti.register_function(f) };
        log!("[probe] CVRRustProbe registered (return type {returns}, parameter {param})\n");
    })
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Every native global function: name, flags, return type, parameters (type and name), sorted by name.
fn dump_natives() {
    let Some(rtti) = RttiSystem::get() else { return };
    let mut rows: Vec<String> = Vec::new();
    for f in rtti.global_functions() {
        if f.flags & function_flags::IS_NATIVE == 0 {
            continue;
        }
        let name = f.full_name.text();
        let returns = f.return_type().map(|t| t.name().text()).unwrap_or("");
        let mut params = String::new();
        for (i, p) in f.params.as_slice().iter().enumerate() {
            // SAFETY: the registry's properties live as long as their function.
            let p = unsafe { &**p };
            let ty = unsafe { p.ty.as_ref() }.map(|t| t.name().text()).unwrap_or("");
            if i > 0 {
                params.push(',');
            }
            let _ = write!(
                params,
                "{{\"type\":\"{}\",\"name\":\"{}\",\"flags\":{}}}",
                json_escape(ty),
                json_escape(p.name.text()),
                p.flags
            );
        }
        rows.push(format!(
            "{{\"name\":\"{}\",\"flags\":{},\"returns\":\"{}\",\"params\":[{}]}}",
            json_escape(name),
            f.flags,
            json_escape(returns),
            params
        ));
    }
    rows.sort();
    let path = paths::file(Folder::Log, "rtti_natives.json");
    let body = format!("[\n{}\n]\n", rows.join(",\n"));
    match std::fs::write(&path, body) {
        Ok(()) => log!("[probe] {} native global functions -> {}\n", rows.len(), path.display()),
        Err(e) => log!("[probe] {}: {e}\n", path.display()),
    }
}

unsafe extern "C" fn on_running(_app: *mut GameApplication) -> bool {
    guard("on_running", true, || {
        log!("[probe] game state Running entered; CVRRustProbe calls so far: {}\n", CALLS.load(Ordering::Relaxed));
        dump_natives();
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn Query(info: *mut PluginInfo) {
    guard("Query", (), || {
        // SAFETY: RED4ext passes a writable PluginInfo.
        let Some(info) = (unsafe { info.as_mut() }) else { return };
        info.name = wide!("CyberpunkVRPort Rust probe").as_ptr();
        info.author = wide!("CyberpunkVRPort").as_ptr();
        info.version = api::SemVer::new(0, 2, 2);
        info.runtime = api::RUNTIME_INDEPENDENT;
        info.sdk = api::SDK_VERSION_COMPAT_0_5_0;
    })
}

#[no_mangle]
pub extern "C" fn Supports() -> u32 {
    api::API_VERSION_1_COMPAT_0
}

#[no_mangle]
pub unsafe extern "C" fn Main(handle: PluginHandle, reason: u8, sdk: *const Sdk) -> bool {
    guard("Main", true, || {
        match reason {
            0 => {
                if STARTED.swap(true, Ordering::AcqRel) {
                    return true;
                }
                cvr_host::log::set_file_name("cvr_rust_probe.log");
                install_panic_log();
                let dump = dump_only();
                log!("=== cvr_rust_probe loaded (Rust {}){} ===\n", env!("CARGO_PKG_VERSION"),
                    if dump { ", dump only" } else { "" });
                // SAFETY: RED4ext's Sdk is valid for the plugin's lifetime.
                let sdk_ref = unsafe { &*sdk };
                if let Some(runtime) = unsafe { sdk_ref.runtime.as_ref() } {
                    log!("[probe] game {}.{}.{}\n", runtime.major, runtime.minor, runtime.patch);
                }
                if !dump {
                    match RttiSystem::get() {
                        Some(rtti) => {
                            rtti.add_post_register_callback(register_natives);
                            log!("[probe] RTTI post-register callback added\n");
                        }
                        None => log!("[probe] CRTTISystem::Get is not resolved\n"),
                    }
                    unsafe { scripts::install(handle, sdk, None, REDSCRIPT_VERSION) };
                }
                let mut running = GameState { on_enter: Some(on_running), on_update: None, on_exit: None };
                let added = unsafe { sdk_ref.game_states.as_ref() }
                    .is_some_and(|g| unsafe { (g.add)(handle, GameStateType::Running, &mut running) });
                log!("[probe] Running game state registered={added}\n");
            }
            1 => {
                log!("[probe] unload: {} CVRRustProbe call(s) this session\n", CALLS.load(Ordering::Relaxed));
                log!("=== cvr_rust_probe unloaded ===\n");
                cvr_host::log::flush();
            }
            other => log!("[probe] Main with unknown reason {other}\n"),
        }
        true
    })
}
