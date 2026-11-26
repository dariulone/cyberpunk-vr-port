//! TweakDB's names without the Lua host (CVR_NO_LUA): the built-in host kept them (src/Lua/cet/scripting/LuaVM.cpp,
//! LuaVM_Hooks.cpp) and answered TDBID.ToStringDEBUG with them -- a shipped game has no names -- and the mechanics
//! match records by name (weapon.rs tdbid_name: the weapon's key, the holsters' looks, the belt's records). Here, the
//! host's way:
//!
//! - the names CET ships, config\tweakdb\tweakdbstr.kark (an Oodle block: OodleLZ_Decompress of the game's
//!   oo2ext_7_win64.dll) or config\tweakdb\tweakdb.str (raw), then r6\cache\modded\tweakdb.str (REDmod's): a header
//!   (magic, version 1, record / flat / query counts) and each name as a compressed length and its bytes, keyed by
//!   TweakDBID(name), read on a thread of its own at the start;
//! - derived names as the engine makes them: CScript's TDBID constructor (base, name) hooked, each one registered
//!   with its base;
//! - the names the plugin itself builds ids from (red4ext_abi::tweakdbid::from_name's observer): the port's own
//!   records, which no shipped list has;
//! - CScript::ToStringDEBUG answered from them, as the host did: the name with its bases before it, else
//!   <TDBID:hash:length[:offset]>.
use core::ffi::{c_char, c_void, CStr};
use red4ext_abi::api::{PluginHandle, Sdk};
use red4ext_abi::stack::StackFrame;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

/// CET's address hashes (src/Lua/cet/reverse/Addresses.h).
const TO_STRING_DEBUG: u32 = 3_515_162_577;
const TDBID_CTOR_DERIVE: u32 = 326_438_016;

struct Entry {
    base: u64,
    name: String,
}
static NAMES: RwLock<Option<HashMap<u64, Entry>>> = RwLock::new(None);
static DERIVE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static TO_STRING_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// RegisterTDBIDString: one name (with its base for a derived one).
fn register(value: u64, base: u64, name: &str) {
    if value == 0 {
        return;
    }
    let mut g = NAMES.write().unwrap_or_else(|p| p.into_inner());
    g.get_or_insert_with(HashMap::new).insert(value, Entry { base, name: name.to_string() });
}
/// red4ext_abi::tweakdbid::from_name's observer: a name the plugin built an id from.
fn observed(value: u64, name: &str) {
    let known = NAMES.read().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|m| m.contains_key(&value));
    if !known {
        register(value, 0, name);
    }
}
/// GetTDBDIDDebugString.
fn debug_string(value: u64) -> String {
    let hash = value as u32;
    let length = (value >> 32) as u8;
    let offset = red4ext_abi::tweakdbid::offset(value);
    if offset != 0 {
        format!("<TDBID:{hash:08X}:{length:02X}:{offset:06X}>")
    } else {
        format!("<TDBID:{hash:08X}:{length:02X}>")
    }
}
/// GetTDBIDString: the name with its bases before it (registered only: None for another).
pub(crate) fn name_of(value: u64) -> Option<String> {
    let g = NAMES.read().unwrap_or_else(|p| p.into_inner());
    let map = g.as_ref()?;
    let first = map.get(&(value & 0xFF_FFFF_FFFF))?;
    let mut text = first.name.clone();
    let mut base = first.base;
    while base != 0 {
        match map.get(&base) {
            Some(e) => {
                text.insert_str(0, &e.name);
                base = e.base;
            }
            None => {
                text.insert_str(0, &debug_string(base));
                break;
            }
        }
    }
    Some(text)
}

/// ReadCompressedInt.
fn compressed_int(data: &[u8], at: &mut usize) -> Option<usize> {
    let mut next = || {
        let b = *data.get(*at)?;
        *at += 1;
        Some(b)
    };
    let mut b = next()?;
    let mut v = usize::from(b & 0x3F);
    if b & 0x40 != 0 {
        b = next()?;
        v |= usize::from(b & 0x7F) << 6;
        if b & 0x80 != 0 {
            b = next()?;
            v |= usize::from(b & 0x7F) << 13;
            if b & 0x80 != 0 {
                b = next()?;
                v |= usize::from(b & 0x7F) << 20;
                if b & 0x80 != 0 {
                    b = next()?;
                    v |= usize::from(b) << 27;
                }
            }
        }
    }
    Some(v)
}
/// The header and the three name arrays: how many were read.
fn read_names(data: &[u8], map: &mut HashMap<u64, Entry>) -> Option<usize> {
    let word = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(data.get(i * 4..i * 4 + 4)?.try_into().ok()?)) };
    if word(1)? != 1 {
        return None;
    }
    let count = word(2)? as usize + word(3)? as usize + word(4)? as usize;
    let mut at = 20;
    for _ in 0..count {
        let n = compressed_int(data, &mut at)?;
        let name = std::str::from_utf8(data.get(at..at + n)?).ok()?;
        at += n;
        map.entry(red4ext_abi::tweakdbid::name_value(name)).or_insert_with(|| Entry { base: 0, name: name.to_string() });
    }
    Some(count)
}
/// tweakdbstr.kark: its 8-byte header (the second word the raw size) and an Oodle block.
fn decompress_kark(bytes: &[u8]) -> Option<Vec<u8>> {
    type Decompress = unsafe extern "C" fn(*const u8, i32, *mut u8, i64, i32, i32, i32, *mut c_void, usize, *mut c_void, *mut c_void, *mut c_void, usize, i32) -> i64;
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    let raw = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let name: Vec<u16> = "oo2ext_7_win64.dll\0".encode_utf16().collect();
    // SAFETY: the game's own Oodle library (loaded already) and its export, called as CET calls it.
    unsafe {
        let module = LoadLibraryW(name.as_ptr());
        if module.is_null() {
            return None;
        }
        let f = GetProcAddress(module, c"OodleLZ_Decompress".as_ptr());
        if f.is_null() {
            return None;
        }
        let f: Decompress = core::mem::transmute(f);
        let mut out = vec![0u8; raw];
        let mut work = vec![0u8; 0x80000];
        let body = &bytes[8..];
        let n = f(
            body.as_ptr(),
            body.len() as i32,
            out.as_mut_ptr(),
            raw as i64,
            1,
            1,
            0,
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            work.as_mut_ptr().cast(),
            work.len(),
            3,
        );
        (n as usize == raw).then_some(out)
    }
}
/// The shipped names and REDmod's, read into the map (on its own thread at the start).
fn load() {
    let started = std::time::Instant::now();
    let dir = cvr_host::paths::dir(cvr_host::paths::Folder::Config).join("tweakdb");
    let mut map = HashMap::new();
    let primary = match std::fs::read(dir.join("tweakdb.str")) {
        Ok(raw) => read_names(&raw, &mut map),
        Err(_) => std::fs::read(dir.join("tweakdbstr.kark")).ok().and_then(|b| decompress_kark(&b)).and_then(|raw| read_names(&raw, &mut map)),
    };
    let modded = cvr_host::paths::game_root()
        .and_then(|r| std::fs::read(r.join("r6").join("cache").join("modded").join("tweakdb.str")).ok())
        .and_then(|raw| read_names(&raw, &mut map));
    let n = map.len();
    {
        let mut g = NAMES.write().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            // names registered meanwhile (derived, the plugin's) stay
            Some(existing) => {
                for (k, v) in map {
                    existing.entry(k).or_insert(v);
                }
            }
            None => *g = Some(map),
        }
    }
    cvr_host::log!(
        "[tweakdb] names from Rust (no Lua host): {} shipped{}, {n} in all, read in {} ms\n",
        primary.map_or("NOT READ".to_string(), |c| c.to_string()),
        modded.map_or(String::new(), |c| format!(", {c} of REDmod's")),
        started.elapsed().as_millis()
    );
}

/// CScript's TDBID constructor from a base and a name (Derive): the engine's, then the name registered.
unsafe extern "C" fn derive(base: *const u64, this: *mut u64, name: *const c_char) -> *mut u64 {
    type F = unsafe extern "C" fn(*const u64, *mut u64, *const c_char) -> *mut u64;
    // SAFETY: the engine's constructor (RED4ext's trampoline) with its own arguments.
    let r = unsafe { core::mem::transmute::<usize, F>(DERIVE_ORIGINAL.load(Ordering::Acquire))(base, this, name) };
    // SAFETY: the engine's TweakDBIDs and its C string.
    unsafe {
        if let (Some(b), Some(t), false) = (base.as_ref(), this.as_ref(), name.is_null()) {
            if let Ok(text) = CStr::from_ptr(name).to_str() {
                register(*t, *b, text);
            }
        }
    }
    r
}
/// CScript::ToStringDEBUG(TweakDBID) -> String: the registered name, else the debug form.
unsafe extern "C" fn to_string_debug(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut value = 0u64;
    // SAFETY: the engine's frame for this native: one TweakDBID parameter.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut value as *mut u64);
            frame.finish_params();
        }
        if !out.is_null() {
            let text = name_of(value).unwrap_or_else(|| debug_string(value));
            red4ext_abi::cstring::CString::assign(out.cast(), &text);
        }
    }
}

/// A function whose entry is a jump has been hooked already (the game's natives start with their prologue).
///
/// # Safety
/// `entry` is readable code.
unsafe fn detoured(entry: *const u8) -> bool {
    // SAFETY: the caller's.
    unsafe { *entry == 0xE9 || (*entry == 0xFF && *entry.add(1) == 0x25) }
}

/// At the plugin's start, with no Lua host: the two hooks, the observer, and the names read on a thread.
///
/// # Safety
/// RED4ext's plugin handle and SDK, from Main's load.
pub(crate) unsafe fn install(handle: PluginHandle, sdk: *const Sdk) -> bool {
    // Cyber Engine Tweaks, installed for other mods, keeps the same names behind the same two functions: the plugin
    // then leaves both to it (two hosts over one function -- the second's detour over the first's -- answer twice).
    let cet = cvr_host::paths::game_root().is_some_and(|r| r.join("bin").join("x64").join("plugins").join("cyber_engine_tweaks.asi").is_file());
    let taken = [TO_STRING_DEBUG, TDBID_CTOR_DERIVE].iter().any(|&h| {
        // SAFETY: the engine function's first bytes, readable code.
        red4ext_abi::reloc::resolve(h).is_some_and(|a| unsafe { detoured(a as *const u8) })
    });
    // the names the plugin builds ids from are kept whatever answers the rest: the port's own records are in no
    // shipped list, and another host never sees them (name_of is asked first, weapon.rs tdbid_name)
    red4ext_abi::tweakdbid::observe_names(observed);
    if cet || taken {
        cvr_host::log!(
            "[tweakdb] names left to {}: the plugin hooks neither ToStringDEBUG nor the TDBID derive (its own records' \
             names kept)\n",
            if cet { "Cyber Engine Tweaks (installed)" } else { "another host (already hooked)" }
        );
        return false;
    }
    // SAFETY: the SDK's hooking table, from the loader.
    let Some(hooking) = (unsafe { sdk.as_ref() }).and_then(|s| unsafe { s.hooking.as_ref() }) else { return false };
    let attach = |hash: u32, detour: *mut c_void, slot: &AtomicUsize| -> bool {
        let Some(target) = red4ext_abi::reloc::resolve(hash) else { return false };
        // SAFETY: the engine function the hash names; RED4ext writes the trampoline into the slot.
        let ok = unsafe { (hooking.attach)(handle, target as *mut c_void, detour, slot.as_ptr() as *mut *mut c_void) };
        ok && slot.load(Ordering::Acquire) != 0
    };
    let derive_ok = attach(TDBID_CTOR_DERIVE, derive as *mut c_void, &DERIVE_ORIGINAL);
    let to_string_ok = attach(TO_STRING_DEBUG, to_string_debug as *mut c_void, &TO_STRING_ORIGINAL);
    let _ = std::thread::Builder::new().name("cvr-tdb-names".into()).spawn(load);
    cvr_host::log!("[tweakdb] names from Rust: derive hooked {derive_ok}, ToStringDEBUG hooked {to_string_ok}\n");
    derive_ok && to_string_ok
}
