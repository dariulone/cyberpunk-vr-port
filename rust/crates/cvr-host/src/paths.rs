//! ONE FOLDER (formerly src/Core/Paths.cpp). Everything the port reads or writes lives in its own folder -- since the
//! port's own loader (2026-11-24) <game>\cyberpunkvrport\, before it red4ext/plugins/cyberpunkvrport/:
//!
//!   shaders/   the shader twins and the port's own shaders      grips/     per-weapon grip and smoke/lighter poses
//!   archive/   the port's archive                               config/    vrport.ini, launcher, calibration, settings
//!   log/       the log and every diagnostic dump                scripts/   the port's scripts (*.reds)
//!   redscript/ the script compiler, for a game without redscript lua/       the port's Lua mods (until stage 6)
//!
//! The folder is the one this module was loaded from, never the game's.
use crate::log;
use crate::wide::{path_from, z};
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};
use windows_sys::Win32::Foundation::{GetLastError, HMODULE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
use windows_sys::Win32::Storage::FileSystem::{
    DeleteFileW, FindClose, FindFirstFileW, FindNextFileW, GetFileAttributesW, MoveFileExW,
    FILE_ATTRIBUTE_DIRECTORY, INVALID_FILE_ATTRIBUTES, MOVEFILE_COPY_ALLOWED, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, WIN32_FIND_DATAW,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    Root,
    Shaders,
    Grips,
    Archive,
    Config,
    Log,
    Scripts,
    Redscript,
    Lua,
}

const NAMES: [&str; 9] = ["", "shaders", "grips", "archive", "config", "log", "scripts", "redscript", "lua"];
/// The folder an earlier release installed the plugin in.
const LEGACY_PLUGIN: &str = "CyberpunkVR_Stereo";
/// The plugin's file, and its name up to 0.1.9: a copy under either name is the port.
pub const MODULE_NAMES: [&str; 2] = ["cyberpunkvrport.dll", "CyberpunkVR_Stereo.dll"];

fn folder_of(module: HMODULE) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32768];
    // SAFETY: buffer owned here, size passed.
    let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    path_from(&buf[..n]).parent().map(Path::to_path_buf)
}

/// The folder this DLL was loaded from.
pub fn module_folder() -> Option<PathBuf> {
    let mut module: HMODULE = core::ptr::null_mut();
    // SAFETY: the address is a function of this module.
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            module_folder as *const () as *const u16,
            &mut module,
        )
    };
    if ok == 0 {
        return None;
    }
    folder_of(module)
}

/// The game executable's folder (bin\x64).
pub fn exe_folder() -> Option<PathBuf> {
    folder_of(core::ptr::null_mut())
}

/// The game's root: three levels above the executable (bin\x64\Cyberpunk2077.exe).
pub fn game_root() -> Option<PathBuf> {
    exe_folder()?.parent()?.parent().map(Path::to_path_buf)
}

// No logging while these are made: the log's own path comes from them.
fn folders() -> &'static [PathBuf; 9] {
    static FOLDERS: OnceLock<[PathBuf; 9]> = OnceLock::new();
    FOLDERS.get_or_init(|| {
        let root = module_folder().unwrap_or_default();
        core::array::from_fn(|i| {
            let dir = if i == 0 { root.clone() } else { root.join(NAMES[i]) };
            if i != 0 && !root.as_os_str().is_empty() {
                let _ = std::fs::create_dir(&dir);
            }
            dir
        })
    })
}

pub fn dir(folder: Folder) -> &'static Path {
    &folders()[folder as usize]
}

pub fn file(folder: Folder, name: &str) -> PathBuf {
    dir(folder).join(name)
}

/// A test switch: true when a file of this name sits next to the game executable (cvr::TestMarker). Callers read it
/// once and keep the result.
pub fn test_marker(name: &str) -> bool {
    exe_folder().is_some_and(|folder| exists(&folder.join(name)))
}

fn exists(path: &Path) -> bool {
    // SAFETY: plain Win32 call.
    unsafe { GetFileAttributesW(z(path).as_ptr()) != INVALID_FILE_ATTRIBUTES }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let (a, b) = (z(a), z(b));
    // SAFETY: lengths exclude the NULs.
    unsafe {
        CompareStringOrdinal(a.as_ptr(), a.len() as i32 - 1, b.as_ptr(), b.len() as i32 - 1, 1) == CSTR_EQUAL
    }
}

/// Both read whole: what migrates is a settings file or a pose, a few kilobytes (at most 1 MiB is compared).
fn same_content(a: &Path, b: &Path) -> bool {
    let read = |p: &Path| std::fs::read(p).ok().filter(|v| v.len() <= 1 << 20);
    matches!((read(a), read(b)), (Some(x), Some(y)) if x == y)
}

/// red4ext\plugins\ -- where RED4ext loads plugins from (the port's folder was in it before its own loader).
fn plugins_folder() -> Option<PathBuf> {
    game_root().map(|g| g.join("red4ext").join("plugins"))
}

/// THE PORT'S OWN LOADER TAKES OVER FROM RED4EXT: an install of the port under red4ext\plugins\cyberpunkvrport (or
/// the 0.1.x CyberpunkVR_Stereo) gives its settings to this folder -- every file of its config\ (and its grips\) this
/// folder does not have yet -- and its DLL is renamed .old, so RED4ext (when installed, for other mods) never loads a
/// second copy. Run by the loader before RED4ext looks for plugins; nothing when this folder is that one.
pub fn retire_red4ext_install() {
    let Some(plugins) = plugins_folder() else { return };
    let root = dir(Folder::Root);
    if root.as_os_str().is_empty() {
        return;
    }
    for name in ["cyberpunkvrport", LEGACY_PLUGIN] {
        let old = plugins.join(name);
        if same_path(&old, root) || !exists(&old) {
            continue;
        }
        let mut copied = 0;
        for sub in ["config", "grips"] {
            copied += copy_missing(&old.join(sub), &root.join(sub));
        }
        for module in MODULE_NAMES {
            let dll = old.join(module);
            if !exists(&dll) {
                continue;
            }
            let mut retired = dll.clone().into_os_string();
            retired.push(".old");
            let retired = PathBuf::from(retired);
            // SAFETY: plain Win32 calls on owned paths.
            unsafe { DeleteFileW(z(&retired).as_ptr()) };
            if unsafe { MoveFileExW(z(&dll).as_ptr(), z(&retired).as_ptr(), MOVEFILE_REPLACE_EXISTING) } != 0 {
                log!("[paths] the RED4ext install {} retired: {copied} setting file(s) copied here, its DLL renamed .old\n", old.display());
            } else {
                let error = unsafe { GetLastError() };
                log!("[paths] the RED4ext install's {} could not be renamed (error {error})\n", dll.display());
            }
        }
    }
}

/// Every file under `from` that `to` lacks, copied (folders made); how many.
fn copy_missing(from: &Path, to: &Path) -> usize {
    let mut n = 0;
    let Ok(entries) = std::fs::read_dir(from) else { return 0 };
    for e in entries.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        if src.is_dir() {
            n += copy_missing(&src, &dst);
        } else if !exists(&dst) {
            let _ = std::fs::create_dir_all(to);
            if std::fs::copy(&src, &dst).is_ok() {
                n += 1;
            }
        }
    }
    n
}

fn move_one(from: &Path, to: &Path) {
    if !exists(from) {
        return;
    }
    if exists(to) {
        // The same bytes are already in place (the release ships them there): the old copy is only clutter.
        // SAFETY: plain Win32 call.
        if same_content(from, to) && unsafe { DeleteFileW(z(from).as_ptr()) } != 0 {
            log!("[paths] {} removed: the same file is in place at {}\n", from.display(), to.display());
        } else {
            log!("[paths] {} kept where it is: {} is already in place\n", from.display(), to.display());
        }
        return;
    }
    // SAFETY: plain Win32 calls.
    if unsafe { MoveFileExW(z(from).as_ptr(), z(to).as_ptr(), MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH) } != 0 {
        log!("[paths] moved {} -> {}\n", from.display(), to.display());
    } else {
        let error = unsafe { GetLastError() };
        log!("[paths] could not move {} -> {} (error {error})\n", from.display(), to.display());
    }
}

/// Every directory entry matching `pattern` (a FindFirstFileW pattern): (name, is_directory).
fn find(pattern: &Path) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    // SAFETY: WIN32_FIND_DATAW is plain data; the handle is closed below.
    unsafe {
        let mut data: WIN32_FIND_DATAW = core::mem::zeroed();
        let handle = FindFirstFileW(z(pattern).as_ptr(), &mut data);
        if handle == INVALID_HANDLE_VALUE {
            return out;
        }
        loop {
            let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
            let name = String::from_utf16_lossy(&data.cFileName[..len]);
            out.push((name, data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0));
            if FindNextFileW(handle, &mut data) == 0 {
                break;
            }
        }
        FindClose(handle);
    }
    out
}

/// Once at load, before anything opens a file: what an earlier release kept next to the exe (vrport.ini,
/// vrport-launcher.ini, the grips) or in its own plugin folder (vrik_calibration.ini) moves here. A file already here
/// wins and the old one is left where it is.
pub fn migrate_legacy_layout() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(exe) = exe_folder() else { return };
        if dir(Folder::Root).as_os_str().is_empty() {
            return;
        }
        for name in ["vrport.ini", "vrport-launcher.ini"] {
            move_one(&exe.join(name), &file(Folder::Config, name));
        }
        for (name, is_dir) in find(&exe.join("CyberpunkVR_*Grip_*.ini")) {
            if !is_dir {
                move_one(&exe.join(&name), &file(Folder::Grips, &name));
            }
        }
        if let Some(plugins) = plugins_folder() {
            let legacy = plugins.join(LEGACY_PLUGIN);
            if !same_path(&legacy, dir(Folder::Root)) {
                move_one(&legacy.join("vrik_calibration.ini"), &file(Folder::Config, "vrik_calibration.ini"));
            }
        }
    });
}

/// Another copy of the port's plugin under red4ext/plugins (an old release's folder left behind): its full path.
/// Two copies hook everything twice, so the second one to find the other stays inert.
pub fn other_copy() -> Option<PathBuf> {
    let plugins = plugins_folder()?;
    for (name, is_dir) in find(&plugins.join("*")) {
        if !is_dir || name.starts_with('.') {
            continue;
        }
        let folder = plugins.join(&name);
        if same_path(&folder, dir(Folder::Root)) {
            continue;
        }
        for module in MODULE_NAMES {
            let candidate = folder.join(module);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// The plugin's file is cyberpunkvrport.dll; up to 0.1.9 it was CyberpunkVR_Stereo.dll, and a release unpacked
/// over an older one leaves that file beside this one. Renamed to .old here; its path when it still runs this launch
/// (already loaded, or the rename failed), so this copy stays inert.
pub fn retire_old_name() -> Option<PathBuf> {
    let root = dir(Folder::Root);
    if root.as_os_str().is_empty() {
        return None;
    }
    let old = root.join(MODULE_NAMES[1]);
    if !exists(&old) {
        return None;
    }
    // SAFETY: plain Win32 calls on owned paths.
    let loaded = unsafe { !GetModuleHandleW(z(&old).as_ptr()).is_null() };
    let mut retired = old.clone().into_os_string();
    retired.push(".old");
    let retired = PathBuf::from(retired);
    unsafe { DeleteFileW(z(&retired).as_ptr()) };
    if unsafe { MoveFileExW(z(&old).as_ptr(), z(&retired).as_ptr(), MOVEFILE_REPLACE_EXISTING) } == 0 {
        let error = unsafe { GetLastError() };
        log!("[paths] {} is an older copy of this plugin and could not be renamed ({error})\n", old.display());
        return Some(old);
    }
    log!(
        "[paths] {} (an older copy of this plugin) renamed to .old{}\n",
        old.display(),
        if loaded { "; it is already loaded, so this copy stays off for this launch" } else { "" }
    );
    loaded.then_some(old)
}
