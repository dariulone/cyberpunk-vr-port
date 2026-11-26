//! The game's own settings file (formerly src/Core/GameSettings.cpp; include/Core/GameSettings.hpp keeps the C++ API,
//! whose Rust-build half calls the C ABI below): the first launch's VR preset merged into the player's
//! UserSettings.json, one option set in place (the launch's borderless window), one option read (the notice's
//! language and volume). The file is read and written as jsoncpp did (json.rs), replaced atomically.
//!
//! THE PRESET'S VALUE GOES IN ONLY WHERE THE PLAYER'S FILE ACCEPTS IT. That file is the game's own schema for the build
//! that wrote it, and one value the game cannot read makes it throw the whole file away. So a list value must be one
//! of the player's list (by name, or by position where the list kept its length); a dynamic list is written only where
//! the player's entry holds a choice; a number must lie in the player's range; anything else keeps its JSON kind. The
//! player's own -- language, sound, difficulty, gallery, telemetry, cloud saves and the picture calibration -- is never
//! carried.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::json::{self, Value};
use std::collections::HashMap;
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CopyFileW, CreateFileW, DeleteFileW, FlushFileBuffers, MoveFileExW, WriteFile, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn in_group(group: &[u8], root: &[u8]) -> bool {
    group == root || (group.starts_with(root) && group.len() > root.len() && group[root.len()] == b'/')
}
/// The player's own settings, never carried by the preset.
fn preserve(group: &[u8], name: &[u8]) -> bool {
    in_group(group, b"/language")
        || in_group(group, b"/audio")
        || in_group(group, b"/gameplay/difficulty")
        || in_group(group, b"/gallery")
        || in_group(group, b"/brightness")
        || (group == b"/gameplay/misc" && (name == b"EnableTelemetry" || name == b"EnableCloudSaves"))
        || (group == b"/video/display" && (name == b"Gamma" || name == b"PaperWhiteLevel" || name == b"MaxMonitorBrightness" || name == b"TonemappingMidpoint"))
}

fn text(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}
fn as_string(v: &Value) -> Vec<u8> {
    // Every caller has validated the value's kind; a jsoncpp throw would be an array or object here.
    v.as_string()
        .unwrap_or_else(|_| panic!("Json::Value::asString(): type is not convertible to string"))
}

/// The settings object, validated as the C++ Parse did.
fn parse(raw: &[u8]) -> Result<Value, String> {
    let root = json::parse(raw)?;
    if !root.is_object() || !root.get(b"data").is_array() {
        return Err("Expected a settings object with a data array".into());
    }
    let Value::Array(groups) = root.get(b"data") else { unreachable!() };
    let mut seen = std::collections::BTreeSet::new();
    for group in groups {
        let name = group.get(b"group_name");
        if !group.is_object() || !name.is_string() || name.empty_string() || !group.get(b"options").is_array() {
            return Err("Invalid settings group".into());
        }
        let name = as_string(name);
        if !seen.insert(name.clone()) {
            return Err(format!("Duplicate settings group: {}", text(&name)));
        }
        let mut options = std::collections::BTreeSet::new();
        let Value::Array(list) = group.get(b"options") else { unreachable!() };
        for option in list {
            let oname = option.get(b"name");
            if !option.is_object() || !oname.is_string() || oname.empty_string() {
                return Err(format!("Invalid setting in {}", text(&name)));
            }
            // A button is an action in the menu, not a value: 2.5 writes /gameplay/misc TakeoutTelemetry with none.
            let button = option.get(b"type").is_string() && as_string(option.get(b"type")) == b"button";
            if !button && !option.is_member(b"value") {
                return Err(format!("Invalid setting in {}", text(&name)));
            }
            if !options.insert(as_string(oname)) {
                return Err(format!("Duplicate setting in {}", text(&name)));
            }
        }
    }
    Ok(root)
}

trait EmptyString {
    fn empty_string(&self) -> bool;
}
impl EmptyString for Value {
    fn empty_string(&self) -> bool {
        matches!(self, Value::Str(s) if s.is_empty())
    }
}

fn read(path: &[u16]) -> Option<Vec<u8>> {
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(path));
    let size = std::fs::metadata(&path).ok()?.len();
    if size == 0 || size > 16 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    (bytes.len() as u64 == size).then_some(bytes)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn z(v: &[u16]) -> Vec<u16> {
    let mut out = v.to_vec();
    out.push(0);
    out
}
fn win_error(action: &str) -> String {
    // SAFETY: the calling thread's last error.
    format!("{action} (Windows error {})", unsafe { GetLastError() })
}
/// The folder of `player` (as std::filesystem::path::parent_path), joined with `name`.
fn beside(player: &[u16], name: &[u16]) -> Vec<u16> {
    let cut = player.iter().rposition(|&c| c == b'\\' as u16 || c == b'/' as u16);
    let mut out = match cut {
        Some(i) => player[..i].to_vec(),
        None => Vec::new(),
    };
    if !out.is_empty() {
        out.push(b'\\' as u16);
    }
    out.extend_from_slice(name);
    out
}
/// The settings file replaced in one step: written beside it under a temporary name, flushed, then moved over it.
/// `backup`, when given, receives a copy of the old one first.
fn write_atomically(player: &[u16], bytes: &[u8], suffix: &[u16], backup: Option<&[u16]>) -> Result<(), String> {
    let mut name = wide("UserSettings.vrport-");
    name.extend_from_slice(suffix);
    name.extend_from_slice(&wide(".tmp"));
    let temporary = z(&beside(player, &name));
    let (player_z, backup_z) = (z(player), backup.map(z));
    // SAFETY: NUL-terminated paths; the handle closed below.
    unsafe {
        let file = CreateFileW(
            temporary.as_ptr(),
            GENERIC_WRITE,
            0,
            core::ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            core::ptr::null_mut(),
        );
        if file == INVALID_HANDLE_VALUE {
            return Err(win_error("Cannot create temporary settings"));
        }
        let mut written = 0u32;
        let saved = WriteFile(file, bytes.as_ptr(), bytes.len() as u32, &mut written, core::ptr::null_mut()) != 0
            && written as usize == bytes.len()
            && FlushFileBuffers(file) != 0;
        let mut error = String::new();
        if !saved {
            error = win_error("Cannot write temporary settings");
        }
        CloseHandle(file);
        let mut ok = false;
        if saved {
            if let Some(b) = &backup_z {
                if CopyFileW(player_z.as_ptr(), b.as_ptr(), 1) == 0 {
                    error = win_error("Cannot back up player settings");
                }
            }
            if error.is_empty() {
                if MoveFileExW(temporary.as_ptr(), player_z.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) != 0 {
                    ok = true;
                } else {
                    error = win_error("Cannot install settings");
                }
            }
        }
        if !ok {
            DeleteFileW(temporary.as_ptr());
            return Err(error);
        }
    }
    Ok(())
}

fn same_kind(a: &Value, b: &Value) -> bool {
    core::mem::discriminant(a) == core::mem::discriminant(b) || (a.is_numeric() && b.is_numeric())
}
/// `Accepts`: whether the player's option `target` takes `source`'s value, and what to write (`value`, `index`).
fn accepts(target: &Value, source: &Value, value: &mut Value, index: &mut Value) -> bool {
    let kind = as_string(target.get(b"type"));
    if kind == b"button" || (source.get(b"type").is_string() && as_string(source.get(b"type")) != kind) {
        return false;
    }
    let wanted = source.get(b"value");
    if kind.ends_with(b"_list") {
        if target.get(b"is_dynamic").as_bool().unwrap_or_else(|_| panic!("Json::Value::asBool()")) {
            let chosen = target.get(b"index").is_int()
                && target.get(b"index").as_int().unwrap_or(0) >= 0
                && !(target.get(b"value").is_string() && as_string(target.get(b"value")).is_empty());
            if !chosen || !same_kind(wanted, target.get(b"value")) {
                return false;
            }
            *value = wanted.clone();
            if source.is_member(b"index") {
                *index = source.get(b"index").clone();
            }
            return true;
        }
        let Value::Array(list) = target.get(b"values") else { return false };
        if list.is_empty() {
            return false;
        }
        if let Some(i) = list.iter().position(|v| v == wanted) {
            *value = wanted.clone();
            *index = Value::Int(i as i64);
            return true;
        }
        let theirs = source.get(b"values");
        let at = if source.get(b"index").is_int() {
            source.get(b"index").as_int().unwrap_or(-1)
        } else {
            -1
        };
        if !theirs.is_array() || theirs.size() != list.len() || at < 0 || at as usize >= list.len() {
            return false;
        }
        *value = list[at as usize].clone();
        *index = Value::Int(at as i64);
        return true;
    }
    if kind == b"int" || kind == b"float" {
        if !wanted.is_numeric() || (kind == b"int" && !wanted.is_integral()) {
            return false;
        }
        let v = wanted.as_double().unwrap_or(0.0);
        let (min, max) = (target.get(b"min_value"), target.get(b"max_value"));
        if (min.is_numeric() && v < min.as_double().unwrap_or(0.0)) || (max.is_numeric() && v > max.as_double().unwrap_or(0.0)) {
            return false;
        }
        *value = if kind == b"int" {
            match wanted.as_int() {
                Ok(i) => Value::Int(i as i64),
                // jsoncpp throws for an integral number past 32 bits; refused here.
                Err(_) => return false,
            }
        } else {
            wanted.clone()
        };
        return true;
    }
    if !same_kind(wanted, target.get(b"value")) {
        return false;
    }
    *value = wanted.clone();
    true
}
/// The time and process a temporary file and a backup are named by.
fn suffix() -> Vec<u16> {
    // SAFETY: plain system queries into a local.
    let (t, pid) = unsafe {
        let mut t = core::mem::zeroed();
        GetLocalTime(&mut t);
        (t, GetCurrentProcessId())
    };
    wide(&format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}-{}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds, pid
    ))
}

pub struct Merged {
    pub ok: bool,
    pub changed: u32,
    pub json: Vec<u8>,
    pub error: String,
    pub skipped: Vec<Vec<u8>>,
}

/// MergeGameSettings: the preset's values over the player's file, as the file to write.
pub fn merge(player: &[u8], preset: &[u8]) -> Merged {
    let mut result = Merged {
        ok: false,
        changed: 0,
        json: Vec::new(),
        error: String::new(),
        skipped: Vec::new(),
    };
    let mut current = match parse(player) {
        Ok(v) => v,
        Err(e) => {
            result.error = format!("Player settings: {e}");
            return result;
        }
    };
    let shipped = match parse(preset) {
        Ok(v) => v,
        Err(e) => {
            result.error = format!("VR preset: {e}");
            return result;
        }
    };
    let Value::Array(groups) = current.member(b"data") else { unreachable!() };
    let mut by_group: HashMap<Vec<u8>, usize> = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        by_group.entry(as_string(g.get(b"group_name"))).or_insert(i);
    }
    let Value::Array(source_groups) = shipped.get(b"data") else { unreachable!() };
    for source_group in source_groups {
        let group_name = as_string(source_group.get(b"group_name"));
        if preserve(&group_name, b"") {
            continue;
        }
        let Value::Array(source_options) = source_group.get(b"options") else {
            continue;
        };
        // A group or an option the player's file does not have is one this build of the game does not have.
        let Some(&found) = by_group.get(&group_name) else {
            for option in source_options {
                let name = as_string(option.get(b"name"));
                if !preserve(&group_name, &name) {
                    result.skipped.push([group_name.as_slice(), b"/", name.as_slice()].concat());
                }
            }
            continue;
        };
        let Value::Array(options) = groups[found].member(b"options") else { continue };
        let mut by_name: HashMap<Vec<u8>, usize> = HashMap::new();
        for (i, o) in options.iter().enumerate() {
            by_name.entry(as_string(o.get(b"name"))).or_insert(i);
        }
        for source in source_options {
            let name = as_string(source.get(b"name"));
            if preserve(&group_name, &name) {
                continue;
            }
            let (mut value, mut index) = (Value::Null, Value::Null);
            let at = by_name.get(&name).copied();
            if at.is_none() || !accepts(&options[at.unwrap()], source, &mut value, &mut index) {
                result.skipped.push([group_name.as_slice(), b"/", name.as_slice()].concat());
                continue;
            }
            // Keep the player's schema, defaults and unknown fields: only the saved value and its list index change.
            let target = &mut options[at.unwrap()];
            let mut changed = *target.member(b"value") != value;
            *target.member(b"value") = value;
            if !index.is_null() {
                changed |= *target.member(b"index") != index;
                *target.member(b"index") = index;
            }
            if changed {
                result.changed += 1;
            }
        }
    }
    let mut out = json::write(&current);
    out.push(b'\n');
    result.json = out;
    result.ok = true;
    result
}

pub struct Installed {
    pub ok: bool,
    pub changed: u32,
    pub backup: Vec<u16>,
    pub error: String,
    pub skipped: Vec<Vec<u8>>,
}
/// InstallGameSettings: the preset merged into the player's file, the old one kept as UserSettings.pre-vr-*.json.
pub fn install(preset: &[u16], player: &[u16]) -> Installed {
    let mut result = Installed {
        ok: false,
        changed: 0,
        backup: Vec::new(),
        error: String::new(),
        skipped: Vec::new(),
    };
    let Some(source) = read(preset) else {
        result.error = "Cannot read VR preset".into();
        return result;
    };
    let Some(existing) = read(player) else {
        result.error = "Cannot read existing player settings".into();
        return result;
    };
    let merged = merge(&existing, &source);
    if !merged.ok {
        result.error = merged.error;
        return result;
    }
    result.changed = merged.changed;
    result.skipped = merged.skipped;
    if merged.changed == 0 {
        result.ok = true;
        return result;
    }
    let suffix = suffix();
    let mut name = wide("UserSettings.pre-vr-");
    name.extend_from_slice(&suffix);
    name.extend_from_slice(&wide(".json"));
    let backup = beside(player, &name);
    match write_atomically(player, &merged.json, &suffix, Some(&backup)) {
        Ok(()) => {
            result.ok = true;
            result.backup = backup;
        }
        Err(e) => result.error = e,
    }
    result
}

pub struct Set {
    pub ok: bool,
    pub changed: bool,
    pub previous: Vec<u8>,
    pub error: String,
}
/// SetGameSetting: one option's value and list index, written only when the file holds something else.
pub fn set(player: &[u16], group: &[u8], name: &[u8], value: &[u8], index: i32) -> Set {
    let mut result = Set {
        ok: false,
        changed: false,
        previous: Vec::new(),
        error: String::new(),
    };
    let Some(raw) = read(player) else {
        result.error = "Cannot read player settings".into();
        return result;
    };
    let mut root = match parse(&raw) {
        Ok(v) => v,
        Err(e) => {
            result.error = format!("Player settings: {e}");
            return result;
        }
    };
    let mut hit = None;
    if let Value::Array(groups) = root.member(b"data") {
        'search: for (gi, g) in groups.iter_mut().enumerate() {
            if as_string(g.member(b"group_name")) != group {
                continue;
            }
            if let Value::Array(options) = g.member(b"options") {
                for (oi, option) in options.iter_mut().enumerate() {
                    if as_string(option.member(b"name")) != name {
                        continue;
                    }
                    result.previous = as_string(option.member(b"value"));
                    if result.previous == value && option.member(b"index").as_int().unwrap_or_else(|_| panic!("Json::Value::asInt()")) == index {
                        result.ok = true;
                        return result;
                    }
                    *option.member(b"value") = Value::Str(value.to_vec());
                    *option.member(b"index") = Value::Int(index as i64);
                    hit = Some((gi, oi));
                    break 'search;
                }
            }
        }
    }
    if hit.is_none() {
        result.error = format!("No {}/{} in the player settings", text(group), text(name));
        return result;
    }
    let mut out = json::write(&root);
    out.push(b'\n');
    match write_atomically(player, &out, &suffix(), None) {
        Ok(()) => {
            result.ok = true;
            result.changed = true;
        }
        Err(e) => result.error = e,
    }
    result
}

/// PlayerSettingsFile: %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json, or nothing.
pub fn player_settings_file() -> Vec<u16> {
    let mut local = [0u16; 260];
    let name = z(&wide("LOCALAPPDATA"));
    // SAFETY: a NUL-terminated name and a MAX_PATH buffer.
    let length = unsafe { windows_sys::Win32::System::Environment::GetEnvironmentVariableW(name.as_ptr(), local.as_mut_ptr(), 260) } as usize;
    if length == 0 || length >= 260 {
        return Vec::new();
    }
    let mut out = local[..length].to_vec();
    out.extend_from_slice(&wide("\\CD Projekt Red\\Cyberpunk 2077\\UserSettings.json"));
    out
}

/// GetGameSetting: one option's saved value as text; empty when the file or the option is not there.
pub fn get(player: &[u16], group: &[u8], name: &[u8]) -> Vec<u8> {
    let Some(raw) = read(player) else { return Vec::new() };
    let Ok(root) = parse(&raw) else { return Vec::new() };
    let Value::Array(groups) = root.get(b"data") else { return Vec::new() };
    for g in groups {
        if as_string(g.get(b"group_name")) != group {
            continue;
        }
        let Value::Array(options) = g.get(b"options") else { continue };
        for option in options {
            if as_string(option.get(b"name")) == name && option.get(b"value").is_convertible_to_string() {
                return as_string(option.get(b"value"));
            }
        }
    }
    Vec::new()
}

// ---- the C ABI (src/Core/GameSettings.cpp's Rust-build half)

/// Bytes handed to C++, owned here until cvr_settings_buf_free.
#[repr(C)]
pub struct CvrBuf {
    pub ptr: *mut u8,
    pub len: usize,
}
impl CvrBuf {
    fn from(v: Vec<u8>) -> CvrBuf {
        let mut b = v.into_boxed_slice();
        let out = CvrBuf {
            ptr: b.as_mut_ptr(),
            len: b.len(),
        };
        core::mem::forget(b);
        out
    }
    fn wide(v: Vec<u16>) -> CvrBuf {
        CvrBuf::from(v.iter().flat_map(|c| c.to_le_bytes()).collect())
    }
    fn list(items: &[Vec<u8>]) -> CvrBuf {
        // Each item as its u32 length (little-endian), then its bytes.
        let mut out = Vec::new();
        for item in items {
            out.extend_from_slice(&(item.len() as u32).to_le_bytes());
            out.extend_from_slice(item);
        }
        CvrBuf::from(out)
    }
}
/// # Safety
/// `buf` came from this module, once.
#[no_mangle]
pub unsafe extern "C" fn cvr_settings_buf_free(buf: CvrBuf) {
    if !buf.ptr.is_null() {
        drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(buf.ptr, buf.len)));
    }
}
unsafe fn bytes<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    }
}
fn guarded<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T, String> {
    std::panic::catch_unwind(f).map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "JSON type error".into())
    })
}

#[repr(C)]
pub struct CvrMergeOut {
    pub ok: bool,
    pub changed: u32,
    pub json: CvrBuf,
    pub error: CvrBuf,
    pub skipped: CvrBuf,
}
/// # Safety
/// The texts hold their lengths' bytes; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_game_settings_merge(player: *const u8, player_len: usize, preset: *const u8, preset_len: usize, out: *mut CvrMergeOut) {
    let (player, preset) = (bytes(player, player_len), bytes(preset, preset_len));
    let m = guarded(|| merge(player, preset)).unwrap_or_else(|e| Merged {
        ok: false,
        changed: 0,
        json: Vec::new(),
        error: e,
        skipped: Vec::new(),
    });
    *out = CvrMergeOut {
        ok: m.ok,
        changed: m.changed,
        json: CvrBuf::from(m.json),
        error: CvrBuf::from(m.error.into_bytes()),
        skipped: CvrBuf::list(&m.skipped),
    };
}

#[repr(C)]
pub struct CvrInstallOut {
    pub ok: bool,
    pub changed: u32,
    pub backup: CvrBuf,
    pub error: CvrBuf,
    pub skipped: CvrBuf,
}
/// # Safety
/// The paths hold their lengths' UTF-16 units; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_game_settings_install(preset: *const u16, preset_len: usize, player: *const u16, player_len: usize, out: *mut CvrInstallOut) {
    let (preset, player) = (bytes(preset, preset_len), bytes(player, player_len));
    let r = guarded(|| install(preset, player)).unwrap_or_else(|e| Installed {
        ok: false,
        changed: 0,
        backup: Vec::new(),
        error: e,
        skipped: Vec::new(),
    });
    *out = CvrInstallOut {
        ok: r.ok,
        changed: r.changed,
        backup: CvrBuf::wide(r.backup),
        error: CvrBuf::from(r.error.into_bytes()),
        skipped: CvrBuf::list(&r.skipped),
    };
}

#[repr(C)]
pub struct CvrSetOut {
    pub ok: bool,
    pub changed: bool,
    pub previous: CvrBuf,
    pub error: CvrBuf,
}
/// # Safety
/// The path holds its UTF-16 units, the texts their bytes; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_game_settings_set(
    player: *const u16,
    player_len: usize,
    group: *const u8,
    group_len: usize,
    name: *const u8,
    name_len: usize,
    value: *const u8,
    value_len: usize,
    index: i32,
    out: *mut CvrSetOut,
) {
    let (player, group, name, value) = (
        bytes(player, player_len),
        bytes(group, group_len),
        bytes(name, name_len),
        bytes(value, value_len),
    );
    let r = guarded(|| set(player, group, name, value, index)).unwrap_or_else(|e| Set {
        ok: false,
        changed: false,
        previous: Vec::new(),
        error: e,
    });
    *out = CvrSetOut {
        ok: r.ok,
        changed: r.changed,
        previous: CvrBuf::from(r.previous),
        error: CvrBuf::from(r.error.into_bytes()),
    };
}

/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_game_settings_player_file(out: *mut CvrBuf) {
    *out = CvrBuf::wide(player_settings_file());
}

/// # Safety
/// As cvr_game_settings_set.
#[no_mangle]
pub unsafe extern "C" fn cvr_game_settings_get(
    player: *const u16,
    player_len: usize,
    group: *const u8,
    group_len: usize,
    name: *const u8,
    name_len: usize,
    out: *mut CvrBuf,
) {
    let (player, group, name) = (bytes(player, player_len), bytes(group, group_len), bytes(name, name_len));
    *out = CvrBuf::from(guarded(|| get(player, group, name)).unwrap_or_default());
}
