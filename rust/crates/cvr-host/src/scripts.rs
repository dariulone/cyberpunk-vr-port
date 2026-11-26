//! THE PLUGIN'S SCRIPTS, WITH OR WITHOUT REDSCRIPT INSTALLED (formerly src/Core/ScriptCompiler.cpp).
//!
//! A plugin's scripts are scripts\**\*.reds in its folder, handed to RED4ext's script compilation like any plugin's.
//! What compiles them is redscript's compiler, scc_lib.dll -- the one redscript installs in engine\tools, else the
//! plugin's own copy in redscript\:
//!
//!   redscript installed   nothing else happens: its compiler compiles the game's scripts and every mod's.
//!   not installed         the engine's Scripts/EnableCompilation is switched on and the engine's start of scc.exe is
//!                         pointed at redscript\scc.exe; RED4ext, which runs the compiler in process from the
//!                         scc_lib.dll beside that path, then compiles with the plugin's copy.
use crate::log;
use crate::paths::{self, Folder};
use crate::wide::{ansi, z};
use red4ext_abi::api::{PluginHandle, Sdk};
use red4ext_abi::cstring::CString;
use red4ext_abi::reloc;
use std::ffi::{c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The engine starting a process: the script compiler among them.
const EXECUTE_PROCESS_HASH: u32 = 2_203_918_127;

/// Game 2.5 (3.0.88.46235) addresses, as include/Stereo/EngineRvas.hpp has them until tools/patchday generates both
/// languages' tables from one source (stage 2).
pub mod rva {
    pub const SCRIPTS_ENABLE_COMPILATION_VAR: usize = 0x3915C68;
    pub const CONFIG_BOOL_VAR_VTABLE: usize = 0x3078FA0;
}

fn redscript_installed(root: &Path) -> bool {
    let tools = root.join("engine").join("tools");
    tools.join("scc_lib.dll").exists() || tools.join("scc.exe").exists()
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                dirs.push(path.clone());
                walk(&path, files, dirs);
            }
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "reds") => files.push(path),
            _ => {}
        }
    }
}

/// r6\scripts\<prefix>*: files with the same bytes as the plugin's copy are deleted, anything else is renamed
/// .legacy (which the compiler does not read); folders left empty go. Compiled beside the plugin's copies they would
/// define everything twice.
pub fn retire_legacy_scripts(root: &Path, prefix: &str) {
    let scripts = root.join("r6").join("scripts");
    let own = paths::dir(Folder::Scripts);
    let Ok(entries) = std::fs::read_dir(&scripts) else { return };
    let folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    let (mut deleted, mut renamed) = (0, 0);
    for folder in folders {
        let (mut files, mut dirs) = (Vec::new(), Vec::new());
        walk(&folder, &mut files, &mut dirs);
        for file in files {
            let Ok(relative) = file.strip_prefix(&scripts) else { continue };
            if same_bytes(&file, &own.join(relative)) {
                if std::fs::remove_file(&file).is_ok() {
                    deleted += 1;
                }
            } else {
                let mut aside = file.clone().into_os_string();
                aside.push(".legacy");
                if std::fs::rename(&file, PathBuf::from(aside)).is_ok() {
                    renamed += 1;
                }
            }
        }
        // Deepest first: the mod's own folder last.
        dirs.insert(0, folder);
        for d in dirs.iter().rev() {
            let _ = std::fs::remove_dir(d); // only succeeds when empty
        }
    }
    if deleted > 0 || renamed > 0 {
        log!(
            "[scripts] an earlier release's scripts in r6\\scripts taken out: {deleted} deleted (same as the plugin's), \
             {renamed} renamed .legacy\n"
        );
    }
}

/// Scripts/EnableCompilation: a bool config variable (vtable +0, name +8, group +16, value +48) whose vtable slot 3
/// sets it from a string view {const char*, u32 length}. Set before the engine reads its config: an ini that names
/// it still decides.
fn enable_compilation() -> bool {
    let base = reloc::image_base();
    let variable = (base + rva::SCRIPTS_ENABLE_COMPILATION_VAR) as *mut u8;
    // SAFETY: reads within the game image's data section at addresses this build is known to have (checked below
    // before anything is called).
    unsafe {
        let vtable = *(variable as *const *const usize);
        let name = *(variable.add(8) as *const *const i8);
        let group = *(variable.add(16) as *const *const i8);
        let matches = vtable as usize == base + rva::CONFIG_BOOL_VAR_VTABLE
            && !name.is_null()
            && !group.is_null()
            && CStr::from_ptr(name) == c"EnableCompilation"
            && CStr::from_ptr(group) == c"Scripts";
        if !matches {
            log!("[scripts] Scripts/EnableCompilation is not where this build keeps it: the scripts are not compiled\n");
            return false;
        }
        #[repr(C)]
        struct StringView {
            text: *const u8,
            length: u32,
        }
        let view = StringView { text: b"true".as_ptr(), length: 4 };
        type SetFromString = unsafe extern "C" fn(*mut c_void, *const StringView) -> bool;
        let set: SetFromString = core::mem::transmute(*vtable.add(3));
        set(variable as *mut c_void, &view)
    }
}

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static COMPILER: OnceLock<std::ffi::CString> = OnceLock::new();
static VERSION: OnceLock<&'static str> = OnceLock::new();

type ExecuteProcessFn = unsafe extern "C" fn(*mut c_void, *mut CString, *mut c_void, *mut CString, i8) -> bool;

/// The engine starts scc.exe with the compilation's arguments; RED4ext's hook on the same function runs the compiler
/// in process from the scc_lib.dll beside the path it is given. The plugin's path goes in place of the engine's.
unsafe extern "C" fn detour_execute_process(
    this: *mut c_void,
    command: *mut CString,
    arguments: *mut c_void,
    directory: *mut CString,
    flags: i8,
) -> bool {
    // SAFETY: ORIGINAL is set by RED4ext's attach before the hook can run.
    let original: ExecuteProcessFn = unsafe { core::mem::transmute(ORIGINAL.load(Ordering::Acquire)) };
    let is_compiler = unsafe { command.as_ref() }.is_some_and(|c| c.as_str().contains("scc.exe"));
    let compiler = COMPILER.get();
    if !is_compiler || compiler.is_none() {
        return unsafe { original(this, command, arguments, directory, flags) };
    }
    let Some(mut ours) = CString::new(compiler.unwrap()) else {
        return unsafe { original(this, command, arguments, directory, flags) };
    };
    let started = std::time::Instant::now();
    let ok = unsafe { original(this, &mut ours, arguments, directory, flags) };
    log!(
        "[scripts] compiled with the plugin's compiler (redscript {}): {}, {:.0} ms\n",
        VERSION.get().copied().unwrap_or("?"),
        if ok { "done" } else { "FAILED" },
        started.elapsed().as_secs_f64() * 1000.0
    );
    ok
}

/// At load (the only time RED4ext takes a plugin's script paths). `legacy_prefix`: the r6\scripts folders an earlier
/// release of this plugin installed (the port: "CyberpunkVRPort_"). `redscript_version` goes in the log.
///
/// # Safety
/// `sdk` must be the pointer RED4ext passed to `Main`.
pub unsafe fn install(handle: PluginHandle, sdk: *const Sdk, legacy_prefix: Option<&str>, redscript_version: &'static str) {
    // SAFETY: as documented.
    unsafe { install_except(handle, sdk, legacy_prefix, redscript_version, &[]) }
}

/// The Rust build: none of the plugin's scripts is compiled (every module's work is Rust's) and the plugin starts no
/// compiler -- other mods' scripts are redscript's business, installed by whoever wants those mods. Only the copies an
/// earlier install left in r6\scripts (`legacy_prefix`) are retired: a separately installed redscript would compile
/// them beside the plugin's own script classes.
pub fn install_none(legacy_prefix: Option<&str>) {
    let Some(root) = paths::game_root() else { return };
    if let Some(prefix) = legacy_prefix {
        retire_legacy_scripts(&root, prefix);
    }
    log!(
        "[scripts] the plugin's scripts are Rust's: nothing added to the script compilation, no compiler started ({})\n",
        if redscript_installed(&root) { "redscript is installed: it compiles the other mods' scripts" } else { "redscript is not installed" }
    );
}

/// As [`install`], the modules named in `ported` (the plugin's script folders whose work the plugin now does itself)
/// left out of the compilation: the others are added one by one.
///
/// # Safety
/// As [`install`].
pub unsafe fn install_except(handle: PluginHandle, sdk: *const Sdk, legacy_prefix: Option<&str>, redscript_version: &'static str, ported: &[&str]) {
    let _ = VERSION.set(redscript_version);
    let Some(root) = paths::game_root() else { return };
    if let Some(prefix) = legacy_prefix {
        retire_legacy_scripts(&root, prefix);
    }
    let own = paths::dir(Folder::Scripts);
    let sdk = unsafe { &*sdk };
    let add = |path: &Path| unsafe { sdk.scripts.as_ref() }.is_some_and(|s| unsafe { (s.add)(handle, z(path).as_ptr()) });
    let added = if ported.is_empty() {
        add(&own)
    } else {
        let mut parts: Vec<PathBuf> = std::fs::read_dir(&own).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        parts.sort();
        let (mut joined, mut left) = (0, Vec::new());
        for part in parts {
            let name = part.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if ported.contains(&name.as_str()) {
                left.push(name);
            } else if (part.is_dir() || part.extension().is_some_and(|e| e == "reds")) && add(&part) {
                joined += 1;
            }
        }
        log!("[scripts] {joined} module(s) added one by one; done by the plugin, not compiled: {}\n", left.join(", "));
        joined > 0
    };
    log!(
        "[scripts] {} {}\n",
        own.display(),
        if added { "joins the script compilation" } else { "NOT added to the script compilation" }
    );
    if !added && !ported.is_empty() {
        // every module's work is the plugin's: nothing of the plugin's is compiled, and the plugin starts no compiler --
        // other mods' scripts are redscript's business, installed by whoever wants those mods
        log!("[scripts] no module left to compile: the plugin starts no script compiler\n");
        return;
    }
    if redscript_installed(&root) {
        log!("[scripts] redscript is installed: its compiler compiles the plugin's scripts\n");
        return;
    }
    if !paths::file(Folder::Redscript, "scc_lib.dll").exists() {
        log!("[scripts] neither redscript nor the plugin's compiler (redscript\\scc_lib.dll): the scripts are not compiled\n");
        return;
    }
    if !enable_compilation() {
        return;
    }
    let compiler = ansi(&paths::file(Folder::Redscript, "scc.exe"));
    let Ok(compiler) = std::ffi::CString::new(compiler) else { return };
    let _ = COMPILER.set(compiler);
    let target = reloc::resolve(EXECUTE_PROCESS_HASH);
    let hooking = unsafe { sdk.hooking.as_ref() };
    let attached = match (target, hooking) {
        (Some(target), Some(hooking)) => {
            // RED4ext writes the trampoline straight into ORIGINAL, so it is in place before the detour can run.
            let ok = unsafe {
                (hooking.attach)(
                    handle,
                    target as *mut c_void,
                    detour_execute_process as *mut c_void,
                    ORIGINAL.as_ptr() as *mut *mut c_void,
                )
            };
            ok && ORIGINAL.load(Ordering::Acquire) != 0
        }
        _ => false,
    };
    if !attached {
        log!("[scripts] the engine's start of the compiler is NOT hooked: the scripts are not compiled\n");
        return;
    }
    log!("[scripts] redscript is not installed: the plugin's compiler compiles the scripts\n");
}
