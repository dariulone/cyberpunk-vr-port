//! The port's mods out of Cyber Engine Tweaks' way (formerly src/Lua/Migration.cpp's MigrateFromCet, which only the
//! built-in Lua host ran; the Rust build starts no Lua host).
//!
//! Before 0.2 the port's CET mods were installed into CET's own mods folder. With CET installed for other mods, CET
//! would run those copies beside the Rust mechanics -- every mechanic twice. At the plugin's start they are taken out:
//! the player's data (what the mods wrote at run time: belt.cfg, wrist.cfg, holster_places.json, ...) into the plugin's
//! lua\<mod>\ (where Rust reads it), the scripts and CET's own per-mod files (its log, db.sqlite3) into legacy\cet\<mod>\.
//! The mods' key binds come along from CET's bindings.json into the plugin's config\lua_bindings.json (made once), and
//! the overlay key of the port's windows moves off CET's own (CET's overlay keeps its key and its input).
use std::fs;
use std::path::{Path, PathBuf};

use cvr_host::log;
use cvr_host::paths::{self, Folder};
use serde_json::Value;

/// The port's CET mods (mods/cet), every one Rust's in the Rust build.
const PORT_MODS: [&str; 26] = [
    "CyberpunkVRPort_Basketball",
    "CyberpunkVRPort_BraindanceHud",
    "CyberpunkVRPort_Crosshair",
    "CyberpunkVRPort_Driving",
    "CyberpunkVRPort_ForceFPP",
    "CyberpunkVRPort_HandCollision",
    "CyberpunkVRPort_HandsOnly",
    "CyberpunkVRPort_Holster",
    "CyberpunkVRPort_Ladder",
    "CyberpunkVRPort_Lobby",
    "CyberpunkVRPort_Menu",
    "CyberpunkVRPort_Optics",
    "CyberpunkVRPort_PhysicalCrouch",
    "CyberpunkVRPort_QuickBoot",
    "CyberpunkVRPort_Reach",
    "CyberpunkVRPort_Reload",
    "CyberpunkVRPort_ReloadRecorder",
    "CyberpunkVRPort_Smoking",
    "CyberpunkVRPort_Stereo",
    "CyberpunkVRPort_Swimming",
    "CyberpunkVRPort_TacticalBelt",
    "CyberpunkVRPort_VRIK",
    "CyberpunkVRPort_Weapon",
    "CyberpunkVRPort_WorldMap",
    "CyberpunkVRPort_WorldMapDiag",
    "CyberpunkVRPort_WristHud",
];

/// VKBindings' packing: the first key in the top 16 bits.
const HOME: u64 = 0x24 << 48;
const SCROLL_LOCK: u64 = 0x91 << 48;
const PAUSE: u64 = 0x13 << 48;

/// The key the port's overlay takes when CET's is `cet`: Home, the port's own default, unless CET has it.
pub(crate) fn key_beside_cet(cet: u64) -> u64 {
    if cet == HOME {
        SCROLL_LOCK
    } else {
        HOME
    }
}
/// The keys earlier releases picked by themselves beside CET (Scroll Lock, Pause when CET had Scroll Lock).
fn earlier_automatic(key: u64) -> bool {
    key == SCROLL_LOCK || key == PAUSE
}
/// lua_bindings.json's "cet" -> "overlay_default": 2 once the overlay key went to Home (0.2.5); a key the player sets
/// after that is theirs.
const DEFAULT_GENERATION: u64 = 2;

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => fs::read(a).ok() == fs::read(b).ok(),
        _ => false,
    }
}

/// CET's own per-mod files: its log (<mod>.log, rotated <mod>.N.log) and the mod's sqlite database.
fn cet_own(relative: &Path) -> bool {
    relative.extension().is_some_and(|e| e.eq_ignore_ascii_case("log"))
        || relative.file_name().is_some_and(|n| n.eq_ignore_ascii_case("db.sqlite3"))
}

fn move_over(from: &Path, to: &Path) -> bool {
    if let Some(p) = to.parent() {
        let _ = fs::create_dir_all(p);
    }
    if fs::rename(from, to).is_ok() {
        return true;
    }
    // another volume, or a file in the way: copied, then removed
    fs::copy(from, to).is_ok() && fs::remove_file(from).is_ok()
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
    let Ok(list) = fs::read_dir(dir) else { return };
    for e in list.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                dirs.push(e.path());
                walk(&e.path(), files, dirs);
            }
            Ok(_) => files.push(e.path()),
            Err(_) => {}
        }
    }
}

/// One of the port's mods out of CET's mods folder.
fn retire(cet_mod: &Path, own: &Path, legacy: &Path) {
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    walk(cet_mod, &mut files, &mut dirs);
    let (mut deleted, mut adopted, mut kept, mut stuck) = (0, 0, 0, 0);
    for file in &files {
        let Ok(relative) = file.strip_prefix(cet_mod) else { continue };
        let target = own.join(relative);
        let script = relative.extension().is_some_and(|e| e.eq_ignore_ascii_case("lua"));
        let done = if script || cet_own(relative) {
            // the mod's Lua is Rust's now; CET's files are CET's: both kept aside, never adopted
            let done = move_over(file, &legacy.join(relative));
            kept += usize::from(done);
            done
        } else if target.exists() && same_bytes(file, &target) {
            let done = fs::remove_file(file).is_ok();
            deleted += usize::from(done);
            done
        } else if !target.exists() {
            let done = move_over(file, &target);
            adopted += usize::from(done);
            done
        } else {
            // data the mod rewrites (its settings) over what is there: the player's copy from CET is the one used
            let done = move_over(&target, &legacy.join(relative)) && move_over(file, &target);
            adopted += usize::from(done);
            done
        };
        stuck += usize::from(!done);
    }
    // deepest first, the mod's own folder last
    dirs.insert(0, cet_mod.to_path_buf());
    for dir in dirs.iter().rev() {
        if fs::read_dir(dir).is_ok_and(|mut r| r.next().is_none()) {
            let _ = fs::remove_dir(dir);
        }
    }
    log!(
        "[cet] {} taken out of CET's mods: {deleted} deleted (same as the plugin's), {adopted} of the player's (written at run \
         time) moved in, {kept} kept in legacy\\cet{}\n",
        cet_mod.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        if stuck > 0 { format!(", {stuck} NOT MOVED (CET runs it too)") } else { String::new() }
    );
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}
fn overlay_key(json: &Value) -> Option<u64> {
    json.get("cet").and_then(|c| c.get("overlay_key")).and_then(Value::as_u64)
}
fn write_json(path: &Path, json: &Value) {
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Ok(text) = serde_json::to_string_pretty(json) {
        let _ = fs::write(path, text);
    }
}

/// At the plugin's start, before the hotkeys and the overlay read config\lua_bindings.json.
pub(crate) fn run() {
    let Some(root) = paths::game_root() else { return };
    let cet_root = root.join("bin").join("x64").join("plugins").join("cyber_engine_tweaks");
    let lua = paths::dir(Folder::Lua);
    let plugin = paths::dir(Folder::Root);
    for name in PORT_MODS {
        let cet_mod = cet_root.join("mods").join(name);
        if cet_mod.is_dir() {
            retire(&cet_mod, &lua.join(name), &plugin.join("legacy").join("cet").join(name));
        }
    }
    let cet_bindings = cet_root.join("bindings.json");
    let bindings = paths::file(Folder::Config, "lua_bindings.json");
    let cet = read_json(&cet_bindings);
    let cet_key = overlay_key(&cet).unwrap_or(0);
    if !bindings.exists() {
        // made once: the overlay key (Home, or beside CET's when CET has Home), and the port's mods' binds CET kept
        let key = if cet_key != 0 { key_beside_cet(cet_key) } else { HOME };
        let mut own = serde_json::json!({ "cet": { "overlay_key": key, "overlay_default": DEFAULT_GENERATION } });
        let mut binds = 0;
        for name in PORT_MODS {
            if let Some(m) = cet.get(name).filter(|m| m.is_object()) {
                binds += m.as_object().map_or(0, serde_json::Map::len);
                own[name] = m.clone();
            }
        }
        if cet_key != 0 || binds > 0 {
            write_json(&bindings, &own);
            log!("[cet] key binds made: overlay key {key:#x}, {binds} mod bind(s) from CET's bindings.json\n");
        }
        return;
    }
    let mut own = read_json(&bindings);
    let Some(current) = overlay_key(&own) else { return };
    let generation = own.get("cet").and_then(|c| c.get("overlay_default")).and_then(Value::as_u64).unwrap_or(0);
    // the key an earlier release picked by itself (Scroll Lock or Pause beside CET) goes to Home once; CET's own key
    // never stays ours (one press would open both overlays); any other key was chosen on purpose and stays
    let moved = if current == cet_key && cet_key != 0 {
        key_beside_cet(cet_key)
    } else if generation < DEFAULT_GENERATION && earlier_automatic(current) {
        key_beside_cet(cet_key)
    } else {
        current
    };
    if moved == current && generation >= DEFAULT_GENERATION {
        return;
    }
    own["cet"]["overlay_key"] = Value::from(moved);
    own["cet"]["overlay_default"] = Value::from(DEFAULT_GENERATION);
    write_json(&bindings, &own);
    if moved != current {
        log!("[cet] overlay key {current:#x} -> {moved:#x} (Home by default; CET's own overlay keeps its key and its input)\n");
    }
}
