//! THE PORT'S TWEAKDB RECORDS, WITHOUT TWEAKXL (formerly the logic of src/Core/TweakRecords.cpp; that file keeps the
//! TweakDB load hook, the FlatWrites of the C++ native tweaks and a narrow adapter over the SDK's TweakDB and RTTI
//! types, `cvr_cpp_tdb_*`).
//!
//! The records the port adds or changes are written in TweakXL's YAML (config\tweaks\**\*.yaml) and applied right
//! after the game loads its TweakDB: a record with $base is created as a copy of every flat its base has, a record
//! with $type with that type's flats, and each property given is written as a flat of the type the record already
//! holds for it. Arrays take TweakXL's operators (!append, !append-once, !prepend, !prepend-once, !remove). The names
//! of the records made here are registered with the engine. Then the native tweaks run with the same TweakDB.
use core::ffi::{c_char, c_void};
use std::collections::HashSet;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use cvr_host::log;
use cvr_host::paths::{self, Folder};
use cvr_settings::yaml::{load, Node};
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString as EngineText;
use red4ext_abi::rtti::{Class, IType, RttiSystem};
use red4ext_abi::tweakdbid;

// ---- the SDK adapter (src/Core/TweakRecords.cpp)

extern "C" {
    fn cvr_cpp_tdb_create_flat_value(db: *mut c_void, ty: *mut c_void, value: *mut c_void) -> i32;
    fn cvr_cpp_tdb_update_record(db: *mut c_void, id: u64) -> bool;
}
// ---- the RTTI adapters (formerly TweakRecords.cpp's cvr_cpp_* over the SDK), through red4ext-abi
extern "C" {
    fn _aligned_malloc(size: usize, alignment: usize) -> *mut c_void;
    fn _aligned_free(memory: *mut c_void);
}
fn itype<'a>(ty: *mut c_void) -> &'a IType {
    // SAFETY: the callers hand an engine RTTI type.
    unsafe { &*(ty as *const IType) }
}
/// `CRTTISystem::Get()->GetClass(CName(name))`.
///
/// # Safety
/// `name` is a NUL-terminated text.
pub(crate) unsafe fn rt_rtti_class(name: *const c_char) -> *const Class {
    // SAFETY: as documented.
    let name = unsafe { core::ffi::CStr::from_ptr(name) }.to_str().unwrap_or("");
    RttiSystem::get().and_then(|rtti| rtti.get_class(CName::hashed(name))).map_or(core::ptr::null(), |class| class as *const Class)
}
/// A default value of `ty` in memory of its own (`_aligned_malloc` of at least one byte at its alignment, at least 8).
///
/// # Safety
/// `ty` is an engine RTTI type.
unsafe fn rt_type_new(ty: *mut c_void) -> *mut c_void {
    let t = itype(ty);
    // SAFETY: the CRT's allocator; the type constructs into its own size.
    unsafe {
        let memory = _aligned_malloc(t.size().max(1) as usize, t.alignment().max(8) as usize);
        t.construct(memory);
        memory
    }
}
/// A value of `rt_type_new`'s destroyed and freed.
///
/// # Safety
/// `value` came from `rt_type_new(ty)`.
unsafe fn rt_type_free(ty: *mut c_void, value: *mut c_void) {
    // SAFETY: as documented.
    unsafe {
        itype(ty).destruct(value);
        _aligned_free(value);
    }
}
/// # Safety
/// Both are live values of `ty`.
unsafe fn rt_type_equal(ty: *mut c_void, a: *const c_void, b: *const c_void) -> bool {
    // SAFETY: as documented.
    unsafe { itype(ty).is_equal(a, b) }
}
/// # Safety
/// Both are live values of `ty`.
pub(crate) unsafe fn rt_type_assign(ty: *mut c_void, dst: *mut c_void, src: *const c_void) {
    // SAFETY: as documented.
    unsafe { itype(ty).assign(dst, src) }
}
/// `type->FromString(out, CString(text))`.
///
/// # Safety
/// `out` is a live value of `ty`; `text` NUL-terminated.
unsafe fn rt_type_from_string(ty: *mut c_void, out: *mut c_void, text: *const c_char) -> bool {
    // SAFETY: as documented.
    let Some(text) = EngineText::new(unsafe { core::ffi::CStr::from_ptr(text) }) else { return false };
    // SAFETY: as documented.
    unsafe { itype(ty).from_string(out, &text) }
}
/// # Safety
/// An engine RTTI type.
unsafe fn rt_type_name(ty: *mut c_void) -> u64 {
    itype(ty).name().0
}
/// # Safety
/// An engine RTTI type.
unsafe fn rt_type_kind(ty: *mut c_void) -> i32 {
    i32::from(itype(ty).kind())
}
/// # Safety
/// An array type.
unsafe fn rt_array_inner(ty: *mut c_void) -> *mut c_void {
    // SAFETY: as documented.
    unsafe { itype(ty).array_inner().cast() }
}
/// # Safety
/// An array type and one of its values.
pub(crate) unsafe fn rt_array_length(ty: *mut c_void, instance: *mut c_void) -> u32 {
    // SAFETY: as documented.
    unsafe { itype(ty).array_length(instance) }
}
/// # Safety
/// An array type and one of its values.
pub(crate) unsafe fn rt_array_element(ty: *mut c_void, instance: *mut c_void, index: u32) -> *mut c_void {
    // SAFETY: as documented.
    unsafe { itype(ty).array_element(instance, index) }
}
/// # Safety
/// An array type and one of its values.
unsafe fn rt_array_remove_at(ty: *mut c_void, instance: *mut c_void, index: u32) -> bool {
    // SAFETY: as documented.
    unsafe { itype(ty).array_remove_at(instance, index as i32) }
}
/// # Safety
/// An array type and one of its values.
pub(crate) unsafe fn rt_array_insert_at(ty: *mut c_void, instance: *mut c_void, index: u32) -> bool {
    // SAFETY: as documented.
    unsafe { itype(ty).array_insert_at(instance, index as i32) }
}
/// # Safety
/// An array type and one of its values.
pub(crate) unsafe fn rt_array_resize(ty: *mut c_void, instance: *mut c_void, size: u32) -> bool {
    // SAFETY: as documented.
    unsafe { itype(ty).array_resize(instance, size) }
}
/// `*static_cast<CString*>(out) = CString(text)`: the move assignment, which overwrites the slot.
///
/// # Safety
/// `out` is a CString slot; `text` NUL-terminated.
pub(crate) unsafe fn rt_cstring_assign(out: *mut c_void, text: *const c_char) {
    // SAFETY: as documented.
    if let Some(value) = EngineText::new(unsafe { core::ffi::CStr::from_ptr(text) }) {
        // SAFETY: as documented.
        unsafe { core::ptr::write(out.cast::<EngineText>(), value) };
    }
}

// ---- TweakDB's read side (formerly TweakRecords.cpp's adapters), through red4ext-abi's tweakdb
/// `GetFlatValue(id)` and its `GetValue()`: the value's type, its address in `*value` (both null without the flat).
///
/// # Safety
/// `db` is the live TweakDB; `value` writable.
unsafe fn rt_tdb_flat(db: *mut c_void, id: u64, value: *mut *mut c_void) -> *mut c_void {
    // SAFETY: as documented.
    unsafe {
        let flat = red4ext_abi::tweakdb::flat_value(db, id);
        if flat.is_null() {
            *value = core::ptr::null_mut();
            return core::ptr::null_mut();
        }
        let held = red4ext_abi::tweakdb::flat_stack_value(flat);
        *value = held.value;
        held.ty as *mut c_void
    }
}
/// `flat ? flat->ToTDBOffset() : -1`.
///
/// # Safety
/// `db` is the live TweakDB.
unsafe fn rt_tdb_flat_offset(db: *mut c_void, id: u64) -> i32 {
    // SAFETY: as documented.
    unsafe {
        let flat = red4ext_abi::tweakdb::flat_value(db, id);
        if flat.is_null() { -1 } else { red4ext_abi::tweakdb::flat_offset(flat) }
    }
}
/// The record's native class, or null.
///
/// # Safety
/// `db` is the live TweakDB.
unsafe fn rt_tdb_record_type(db: *mut c_void, id: u64) -> *const Class {
    // SAFETY: as documented.
    unsafe { red4ext_abi::tweakdb::record_type(db, id) as *const Class }
}
/// The first record of `class`'s id, or 0.
///
/// # Safety
/// `db` is the live TweakDB.
unsafe fn rt_tdb_first_record_of_type(db: *mut c_void, class: *const Class) -> u64 {
    // SAFETY: as documented.
    unsafe { red4ext_abi::tweakdb::first_record_of_type(db, class.cast()) }
}

/// `db->flats.InsertOrAssign` of the ids' sorted unique set (each `InsertOrAssign`ed when `assign`, else `Insert`ed),
/// under the flats' exclusive lock.
///
/// # Safety
/// `db` is the live TweakDB; `ids` holds `count` ids.
unsafe fn rt_tdb_insert_flats(db: *mut c_void, ids: *const u64, count: usize, assign: bool) {
    // SAFETY: as documented.
    unsafe {
        let set = red4ext_abi::tweakdb::unique_flats(core::slice::from_raw_parts(ids, count), assign);
        red4ext_abi::tweakdb::merge_flats(db, &set);
    }
}
/// `db->CreateRecord(id, class)`.
///
/// # Safety
/// `db` is the live TweakDB.
pub(crate) unsafe fn rt_tdb_create_record(db: *mut c_void, id: u64, class: *const Class) -> bool {
    // SAFETY: as documented.
    unsafe { red4ext_abi::tweakdb::create_record(db, id, class.cast()) }
}

const KIND_ARRAY: i32 = 3; // ERTTIType::Array

type DeriveFn = unsafe extern "C" fn(base: *const u64, id: *const u64, name: *const c_char);

// ---- the TweakDB load hook (formerly TweakRecords.cpp's Install and Detour_TryLoadTweakDB)
/// Loads tweakdb.bin; 2.5: (a1, TweakDB*, path, a4, bool).
const TRY_LOAD_TWEAKDB_HASH: u32 = 464_332_086;
/// Registers a TweakDBID's name: (base, id, name).
const TWEAKDB_ID_DERIVE_HASH: u32 = 326_438_016;
static TRY_LOAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static DERIVE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The TweakDB load: the original, then -- when it loaded a database -- the port's records applied.
unsafe extern "C" fn detour_try_load_tweakdb(a1: *mut c_void, db: *mut c_void, path: *mut c_void, a4: *mut c_void, a5: bool) -> bool {
    type TryLoad = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, bool) -> bool;
    use core::sync::atomic::Ordering::Acquire;
    // SAFETY: RED4ext's attach wrote the trampoline before the detour could run.
    let original = unsafe { core::mem::transmute::<usize, TryLoad>(TRY_LOAD.load(Acquire)) };
    // SAFETY: the engine's arguments.
    let loaded = unsafe { original(a1, db, path, a4, a5) };
    if loaded && !db.is_null() {
        let derive = DERIVE.load(Acquire);
        // SAFETY: the loaded database; the derive function the relocation named, or none.
        unsafe { cvr_tweaks_apply_all(db, (derive != 0).then(|| core::mem::transmute::<usize, DeriveFn>(derive))) };
    }
    loaded
}
/// The tweaks' install: the legacy file retired, the derive function and the TweakDB load resolved, the load hooked,
/// one `[tweaks]` line.
///
/// # Safety
/// `handle` and `sdk` are what RED4ext handed the plugin's Main.
#[no_mangle]
pub unsafe extern "C" fn cvr_tweaks_install(handle: red4ext_abi::api::PluginHandle, sdk: *const red4ext_abi::api::Sdk) {
    use core::sync::atomic::Ordering::Release;
    cvr_tweaks_retire_legacy();
    let derive = red4ext_abi::reloc::resolve(TWEAKDB_ID_DERIVE_HASH).unwrap_or(0);
    DERIVE.store(derive, Release);
    // SAFETY: RED4ext's SDK table, valid for the plugin's lifetime.
    let hooking = unsafe { sdk.as_ref().and_then(|s| s.hooking.as_ref()) };
    let hooked = match (red4ext_abi::reloc::resolve(TRY_LOAD_TWEAKDB_HASH), hooking) {
        // SAFETY: RED4ext's attach writes the trampoline into the slot before the detour can run.
        (Some(target), Some(h)) => unsafe {
            (h.attach)(
                handle,
                target as *mut c_void,
                detour_try_load_tweakdb as *const () as *mut c_void,
                TRY_LOAD.as_ptr() as *mut *mut c_void,
            )
        },
        _ => false,
    };
    log!(
        "[tweaks] TweakDB load {}{}\n",
        if hooked { "hooked" } else { "NOT hooked: the port's records are missing" },
        if derive != 0 { "" } else { " (record names not registered)" }
    );
}
type NativeTweak = unsafe extern "C" fn(db: *mut c_void);

pub(crate) fn cstr(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}
pub(crate) fn type_name(ty: *mut c_void) -> String {
    // SAFETY: an RTTI type of the engine's.
    CName(unsafe { rt_type_name(ty) }).text().to_owned()
}

/// A value of a TweakDB flat type, built outside the flat buffer and handed to it.
pub(crate) struct Value {
    ty: *mut c_void,
    pub(crate) memory: *mut c_void,
}
impl Value {
    pub(crate) fn new(ty: *mut c_void) -> Value {
        // SAFETY: the type's own construction in aligned memory of its size.
        Value {
            ty,
            memory: unsafe { rt_type_new(ty) },
        }
    }
}
impl Drop for Value {
    fn drop(&mut self) {
        // SAFETY: made by rt_type_new with this type.
        unsafe { rt_type_free(self.ty, self.memory) };
    }
}

// ---- values from YAML

fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}
/// std::stof: the longest number at the start (after white space), none without one.
fn stof(text: &str) -> Option<f32> {
    let t = text.trim_start();
    let b = t.as_bytes();
    let mut i = usize::from(b.first().is_some_and(|&c| c == b'+' || c == b'-'));
    let low = lower(&t[i.min(t.len())..]);
    if low.starts_with("inf") || low.starts_with("nan") {
        let v = if low.starts_with("inf") {
            f32::INFINITY
        } else {
            f32::NAN
        };
        return Some(if b[0] == b'-' { -v } else { v });
    }
    let whole = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    i += whole;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        frac = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += 1 + frac;
    }
    if whole + frac == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let e = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if e > 0 {
            i = j + e;
        }
    }
    t[..i].parse::<f32>().ok()
}
/// std::stoi: an int at the start; none without digits or out of range.
fn stoi(text: &str) -> Option<i32> {
    let t = text.trim_start();
    let b = t.as_bytes();
    let sign = usize::from(b.first().is_some_and(|&c| c == b'+' || c == b'-'));
    let digits = b[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    t[..sign + digits]
        .parse::<i64>()
        .ok()
        .and_then(|v| i32::try_from(v).ok())
}
/// yaml-cpp's as<float>() / as<int>(): the whole scalar a number.
fn yaml_float(node: &Node) -> Option<f32> {
    node.text().and_then(|t| t.trim().parse::<f32>().ok())
}
fn yaml_int(node: &Node) -> Option<i32> {
    node.text().and_then(|t| t.trim().parse::<i32>().ok())
}

/// One element of a struct written as a YAML map: {x: 0, y: 0, z: 0}.
fn struct_into(type_name: &str, out: *mut c_void, node: &Node) -> bool {
    let Node::Map { entries, .. } = node else {
        return false;
    };
    let floats = |fields: &[(&str, usize)]| {
        let f = out as *mut f32;
        for (k, v) in entries {
            let Some(key) = k.text().map(|t| lower(&t)) else {
                return false;
            };
            let Some(&(_, index)) = fields.iter().find(|(n, _)| *n == key) else {
                return false;
            };
            let Some(value) = yaml_float(v) else {
                return false;
            };
            // SAFETY: the struct's float field.
            unsafe { *f.add(index) = value };
        }
        true
    };
    match type_name {
        "Vector2" => floats(&[("x", 0), ("y", 1)]),
        "Vector3" => floats(&[("x", 0), ("y", 1), ("z", 2)]),
        "Vector4" => floats(&[("x", 0), ("y", 1), ("z", 2), ("w", 3)]),
        "Quaternion" => floats(&[("i", 0), ("j", 1), ("k", 2), ("r", 3)]),
        "EulerAngles" => floats(&[("roll", 0), ("pitch", 1), ("yaw", 2)]),
        "Color" => {
            let c = out as *mut u8;
            for (k, v) in entries {
                let Some(key) = k.text().map(|t| lower(&t)) else {
                    return false;
                };
                let Some(channel) = ["red", "green", "blue", "alpha"]
                    .iter()
                    .position(|n| *n == key)
                else {
                    return false;
                };
                let Some(value) = yaml_int(v) else {
                    return false;
                };
                // SAFETY: the colour's channel byte.
                unsafe { *c.add(channel) = value as u8 };
            }
            true
        }
        _ => false,
    }
}
/// A single value of `ty` from YAML. False when the YAML does not fit the type.
fn scalar_into(ty: *mut c_void, out: *mut c_void, node: &Node) -> bool {
    let name = type_name(ty);
    if matches!(node, Node::Map { .. }) {
        return struct_into(&name, out, node);
    }
    let text = match node {
        Node::Null => String::new(),
        Node::Scalar { text, .. } => text.clone(),
        _ => return false,
    };
    // SAFETY: `out` holds a constructed value of `ty`.
    unsafe {
        match name.as_str() {
            "TweakDBID" => {
                *(out as *mut u64) = if text.is_empty() {
                    0
                } else {
                    tweakdbid::from_name(&text)
                }
            }
            "CName" => {
                *(out as *mut CName) = if text.is_empty() || text == "None" {
                    CName::NONE
                } else {
                    CName::add(&cstr(&text))
                };
            }
            "String" => rt_cstring_assign(out, cstr(&text).as_ptr()),
            "Float" => match stof(&text) {
                Some(v) => *(out as *mut f32) = v,
                None => return false,
            },
            "Int32" => match stoi(&text) {
                Some(v) => *(out as *mut i32) = v,
                None => return false,
            },
            "Bool" => {
                let v = lower(&text);
                if !matches!(v.as_str(), "true" | "false" | "1" | "0") {
                    return false;
                }
                *(out as *mut bool) = v == "true" || v == "1";
            }
            "raRef:CResource" => {
                *(out as *mut u64) = if text.is_empty() {
                    0
                } else {
                    red4ext_abi::resource::resource_path(&text)
                }
            }
            "gamedataLocKeyWrapper" => {
                // LocKey#<number>, or a secondary key that the localization hashes.
                let key = if let Some(n) = text.strip_prefix("LocKey#") {
                    match n.trim_start().parse::<u64>() {
                        Ok(v) => v,
                        Err(_) => return false,
                    }
                } else if !text.is_empty() {
                    red4ext_abi::fnv::fnv1a64(text.as_bytes())
                } else {
                    0
                };
                *(out as *mut u64) = key;
            }
            _ => return rt_type_from_string(ty, out, cstr(&text).as_ptr()),
        }
    }
    true
}

#[derive(PartialEq, Clone, Copy)]
enum Op {
    Plain,
    Append,
    AppendOnce,
    Prepend,
    PrependOnce,
    Remove,
}
fn op_of(node: &Node) -> Op {
    match node.tag() {
        "!append" => Op::Append,
        "!append-once" => Op::AppendOnce,
        "!prepend" => Op::Prepend,
        "!prepend-once" => Op::PrependOnce,
        "!remove" => Op::Remove,
        _ => Op::Plain,
    }
}
fn index_of(
    array: *mut c_void,
    instance: *mut c_void,
    inner: *mut c_void,
    element: *const c_void,
) -> Option<u32> {
    // SAFETY: the array type's own accessors on a live instance.
    unsafe {
        (0..rt_array_length(array, instance)).find(|&i| {
            rt_type_equal(inner, rt_array_element(array, instance, i), element)
        })
    }
}
/// An array value from a YAML list: untagged lists replace the array; a list with operators edits `current`.
fn array_into(ty: *mut c_void, out: *mut c_void, node: &Node, current: *const c_void) -> bool {
    let Node::Seq { items, .. } = node else {
        return false;
    };
    // SAFETY: the array type's own accessors on a constructed value.
    unsafe {
        let inner = rt_array_inner(ty);
        if items.iter().any(|i| op_of(i) != Op::Plain) && !current.is_null() {
            rt_type_assign(ty, out, current);
        }
        for item in items {
            let element = Value::new(inner);
            if !scalar_into(inner, element.memory, item) {
                return false;
            }
            let op = op_of(item);
            let at = index_of(ty, out, inner, element.memory);
            if op == Op::Remove {
                if let Some(at) = at {
                    rt_array_remove_at(ty, out, at);
                }
                continue;
            }
            if matches!(op, Op::AppendOnce | Op::PrependOnce) && at.is_some() {
                continue;
            }
            let mut index = rt_array_length(ty, out);
            if matches!(op, Op::Prepend | Op::PrependOnce) {
                if !rt_array_insert_at(ty, out, 0) {
                    return false;
                }
                index = 0;
            } else if !rt_array_resize(ty, out, index + 1) {
                return false;
            }
            rt_type_assign(inner, rt_array_element(ty, out, index), element.memory);
        }
    }
    true
}

// ---- the TweakDB

/// Whether TweakDB holds the record `name` (TweakDB:GetRecord).
pub(crate) fn record_exists(name: &str) -> bool {
    let db = red4ext_abi::tweakdb::get();
    // SAFETY: the database's own record lookup.
    !db.is_null() && unsafe { red4ext_abi::tweakdb::record_exists(db, tweakdbid::from_name(name)) }
}
/// TweakDB:SetFlat("<record>.<field>", CName) and TweakDB:Update(record), at run time (the belt's grenade prop).
pub(crate) fn set_cname_flat(record: &str, field: &str, value: u64) -> bool {
    let db = red4ext_abi::tweakdb::get();
    if db.is_null() {
        return false;
    }
    let id = tweakdbid::from_name(&format!("{record}.{field}"));
    let Some((ty, _)) = flat(db, id) else { return false };
    let v = Value::new(ty);
    // SAFETY: a CName flat's value (8 bytes).
    unsafe { *(v.memory as *mut u64) = value };
    let mut w = FlatWrites::new(db);
    if !w.set(id, ty, v.memory) {
        return false;
    }
    w.commit();
    // SAFETY: the adapter over UpdateRecord.
    unsafe { cvr_cpp_tdb_update_record(db, tweakdbid::from_name(record)) }
}
/// The belt's propAllowSlot at run time: "<record>.placementSlots" gains `slot` (TweakDB:SetFlat + Update). Some(true)
/// when added, Some(false) when the list already had it, None when the record has no such list or the write failed.
pub(crate) fn allow_slot(record: &str, slot: &str) -> Option<bool> {
    let db = red4ext_abi::tweakdb::get();
    if db.is_null() {
        return None;
    }
    let id = tweakdbid::from_name(&format!("{record}.placementSlots"));
    let (ty, cur) = flat(db, id)?;
    let want = tweakdbid::from_name(slot);
    const KEY: u64 = 0xFF_FFFF_FFFF;
    // SAFETY: the flat's own array type and values; the copy made and grown by the type.
    unsafe {
        let n = rt_array_length(ty, cur);
        if (0..n).any(|i| *(rt_array_element(ty, cur, i) as *const u64) & KEY == want & KEY) {
            return Some(false);
        }
        let copy = Value::new(ty);
        rt_type_assign(ty, copy.memory, cur);
        if !rt_array_resize(ty, copy.memory, n + 1) {
            return None;
        }
        *(rt_array_element(ty, copy.memory, n) as *mut u64) = want;
        let mut w = FlatWrites::new(db);
        if !w.set(id, ty, copy.memory) {
            return None;
        }
        w.commit();
        cvr_cpp_tdb_update_record(db, tweakdbid::from_name(record)).then_some(true)
    }
}

pub(crate) fn flat(db: *mut c_void, id: u64) -> Option<(*mut c_void, *mut c_void)> {
    let mut value = core::ptr::null_mut();
    // SAFETY: the adapter over TweakDB::GetFlatValue.
    let ty = unsafe { rt_tdb_flat(db, id, &mut value) };
    (!ty.is_null()).then_some((ty, value))
}
fn record_type(db: *mut c_void, id: u64) -> Option<&'static Class> {
    // SAFETY: the adapter over TweakDB::GetRecord.
    unsafe { rt_tdb_record_type(db, id).as_ref() }
}

/// FlatWrites: flat writes committed together (one insert into the TweakDB's sorted flat array).
pub(crate) struct FlatWrites {
    db: *mut c_void,
    pending: Vec<u64>,
}
impl FlatWrites {
    pub(crate) fn new(db: *mut c_void) -> FlatWrites {
        FlatWrites {
            db,
            pending: Vec::new(),
        }
    }
    pub(crate) fn set(&mut self, id: u64, ty: *mut c_void, value: *mut c_void) -> bool {
        if let Some((held, current)) = flat(self.db, id) {
            // SAFETY: the type's own comparison.
            if held == ty && unsafe { rt_type_equal(ty, current, value) } {
                return true;
            }
        }
        // The SDK's CreateFlatValue MOVES from what it is given: it gets a copy of its own.
        let copy = Value::new(ty);
        // SAFETY: the type's own assignment and the adapter over CreateFlatValue.
        let offset = unsafe {
            rt_type_assign(ty, copy.memory, value);
            cvr_cpp_tdb_create_flat_value(self.db, ty, copy.memory)
        };
        if offset < 0 {
            return false;
        }
        self.pending.push(tweakdbid::with_offset(id, offset));
        true
    }
    pub(crate) fn commit(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        // SAFETY: the adapter: a sorted unique array of these (the last one of an id kept), inserted under mutex00.
        unsafe {
            rt_tdb_insert_flats(self.db, self.pending.as_ptr(), self.pending.len(), true)
        };
        self.pending.clear();
    }
}
impl Drop for FlatWrites {
    fn drop(&mut self) {
        self.commit();
    }
}

struct Record {
    name: String,
    file: String,
    body: Node,
}
struct Planned<'a> {
    record: &'a Record,
    id: u64,
    class: &'static Class,
    names: Vec<String>,
}

/// The record's flat names: each getter of the class chain whose flat exists on `sample`, lowercase first, uppercase
/// otherwise.
fn flat_names(db: *mut c_void, class: &'static Class, sample: u64) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let stop = CName::hashed("gamedataTweakDBRecord");
    let mut cls = Some(class);
    while let Some(c) = cls {
        if c.class_name() == stop {
            break;
        }
        for &f in c.funcs() {
            // SAFETY: a registered class's functions.
            let Some(f) = (unsafe { f.as_ref() }) else {
                continue;
            };
            let name = f.short_name.text();
            if name.is_empty() {
                continue;
            }
            for upper in [false, true] {
                let mut chars: Vec<char> = name.chars().collect();
                chars[0] = if upper {
                    chars[0].to_ascii_uppercase()
                } else {
                    chars[0].to_ascii_lowercase()
                };
                let candidate: String = chars.into_iter().collect();
                if flat(db, tweakdbid::derive(sample, &format!(".{candidate}"))).is_some() {
                    if seen.insert(candidate.clone()) {
                        names.push(candidate);
                    }
                    break;
                }
            }
        }
        cls = c.parent();
    }
    names
}

struct Applier {
    db: *mut c_void,
    derive: Option<DeriveFn>,
    created: usize,
    flats: usize,
    errors: usize,
}
impl Applier {
    fn fail(&mut self, record: &Record, what: &str) {
        log!("[tweaks] {} {} ({})\n", record.name, what, record.file);
        self.errors += 1;
    }
    /// The class $type names: the full RTTI name, or the short one TweakXL also takes (AttachmentSlot).
    fn type_named(name: &str) -> Option<&'static Class> {
        // SAFETY: the adapter over CRTTISystem::GetClass.
        unsafe {
            rt_rtti_class(cstr(name).as_ptr())
                .as_ref()
                .or_else(|| {
                    rt_rtti_class(cstr(&format!("gamedata{name}_Record")).as_ptr()).as_ref()
                })
        }
    }
    /// A new record of the type of its $base (every flat the base holds, shared with it) or of its $type (default
    /// flats, of the types any record of the type holds them in).
    fn plan<'a>(&mut self, record: &'a Record, flats: &mut Vec<u64>) -> Option<Planned<'a>> {
        let id = tweakdbid::from_name(&record.name);
        let (class, source, copy) = if let Some(base) = record.body.get("$base") {
            let source = tweakdbid::from_name(&base.text().unwrap_or_default());
            let Some(class) = record_type(self.db, source) else {
                self.fail(record, "has a $base that is no record");
                return None;
            };
            (class, source, true)
        } else if let Some(type_name) = record.body.get("$type") {
            let class = Self::type_named(&type_name.text().unwrap_or_default());
            // SAFETY: the adapter over recordsByType under mutex01.
            let source = class.map_or(0, |c| unsafe {
                rt_tdb_first_record_of_type(self.db, c)
            });
            let (Some(class), true) = (class, tweakdbid::is_valid(source)) else {
                self.fail(record, "has a $type no record has");
                return None;
            };
            (class, source, false)
        } else {
            self.fail(record, "is no record and has neither $base nor $type");
            return None;
        };
        let names = flat_names(self.db, class, source);
        for property in &names {
            let from_id = tweakdbid::derive(source, &format!(".{property}"));
            // SAFETY: the adapter over FlatValue::ToTDBOffset.
            let mut offset = unsafe { rt_tdb_flat_offset(self.db, from_id) };
            if offset < 0 {
                continue;
            }
            if !copy {
                let Some((ty, _)) = flat(self.db, from_id) else {
                    continue;
                };
                let empty = Value::new(ty);
                // SAFETY: the adapter over CreateFlatValue.
                offset = unsafe { cvr_cpp_tdb_create_flat_value(self.db, ty, empty.memory) };
                if offset < 0 {
                    self.fail(record, "could not get its flats");
                    return None;
                }
            }
            flats.push(tweakdbid::with_offset(
                tweakdbid::derive(id, &format!(".{property}")),
                offset,
            ));
        }
        Some(Planned {
            record,
            id,
            class,
            names,
        })
    }
    /// The engine's names for a new record and its flats, so they read back as names.
    fn register_names(&self, plan: &Planned) {
        let Some(derive) = self.derive else { return };
        let none = 0u64;
        // SAFETY: the engine's TweakDBID::Derive(base, id, name).
        unsafe {
            derive(&none, &plan.id, cstr(&plan.record.name).as_ptr());
            for property in &plan.names {
                let flat = tweakdbid::derive(plan.id, &format!(".{property}"));
                derive(&plan.id, &flat, cstr(&format!(".{property}")).as_ptr());
            }
        }
    }
    /// Each property, as a flat of the type the record holds it in.
    fn write(&mut self, record: &Record, writes: &mut FlatWrites) {
        let id = tweakdbid::from_name(&record.name);
        let Node::Map { entries, .. } = &record.body else {
            return;
        };
        for (key, value) in entries {
            let name = key.text().unwrap_or_default();
            if name.is_empty() || name.starts_with('$') {
                continue;
            }
            let flat_id = tweakdbid::derive(id, &format!(".{name}"));
            let Some((ty, held)) = flat(self.db, flat_id) else {
                log!(
                    "[tweaks] {}.{}: the record has no such property ({})\n",
                    record.name,
                    name,
                    record.file
                );
                self.errors += 1;
                continue;
            };
            let v = Value::new(ty);
            // SAFETY: the type's kind.
            let ok = if unsafe { rt_type_kind(ty) } == KIND_ARRAY {
                array_into(ty, v.memory, value, held)
            } else {
                scalar_into(ty, v.memory, value)
            };
            if !ok || !writes.set(flat_id, ty, v.memory) {
                log!(
                    "[tweaks] {}.{}: not a {} value ({})\n",
                    record.name,
                    name,
                    type_name(ty),
                    record.file
                );
                self.errors += 1;
                continue;
            }
            self.flats += 1;
        }
    }
    /// The records a level at a time: every record a level creates gets its flats in one insert, then its record
    /// object, then the properties of the whole level are written.
    fn apply_level(&mut self, level: &[&Record]) {
        let mut planned = Vec::new();
        let mut flats: Vec<u64> = Vec::new();
        let mut ready: Vec<&Record> = Vec::new();
        for &record in level {
            if record_type(self.db, tweakdbid::from_name(&record.name)).is_some() {
                ready.push(record);
                continue;
            }
            if let Some(plan) = self.plan(record, &mut flats) {
                planned.push(plan);
            }
        }
        // SAFETY: the adapter: a sorted unique array (the first of an id kept), inserted under mutex00.
        unsafe { rt_tdb_insert_flats(self.db, flats.as_ptr(), flats.len(), false) };
        for plan in &planned {
            // SAFETY: the adapter over CreateRecord.
            if !unsafe { rt_tdb_create_record(self.db, plan.id, plan.class) }
                || record_type(self.db, plan.id).is_none()
            {
                self.fail(plan.record, "was not created");
                continue;
            }
            self.register_names(plan);
            ready.push(plan.record);
            self.created += 1;
        }
        let mut writes = FlatWrites {
            db: self.db,
            pending: Vec::new(),
        };
        for &record in &ready {
            self.write(record, &mut writes);
        }
        writes.commit();
        for &record in &ready {
            // SAFETY: the adapter over UpdateRecord under mutex01.
            if !unsafe { cvr_cpp_tdb_update_record(self.db, tweakdbid::from_name(&record.name)) } {
                self.fail(record, "could not be refreshed");
            }
        }
    }
}

fn tweaks_dir() -> PathBuf {
    paths::dir(Folder::Config).join("tweaks")
}
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() => {
                if p.extension()
                    .is_some_and(|x| matches!(lower(&x.to_string_lossy()).as_str(), "yaml" | "yml"))
                {
                    out.push(p);
                }
            }
            _ => {}
        }
    }
}
fn relative_shown(path: &Path) -> String {
    path.strip_prefix(tweaks_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
fn read_records() -> Vec<Record> {
    let mut files = Vec::new();
    walk(&tweaks_dir(), &mut files);
    files.sort();
    let mut records = Vec::new();
    for file in &files {
        let shown = relative_shown(file);
        let text = std::fs::read(file)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        match load(&text) {
            Ok(Node::Map { entries, .. }) => {
                for (k, v) in entries {
                    records.push(Record {
                        name: k.text().unwrap_or_default(),
                        file: shown.clone(),
                        body: v,
                    });
                }
            }
            Ok(_) => {}
            Err(e) => log!("[tweaks] {}: {}\n", shown, e),
        }
    }
    records
}

/// ApplyAll: the port's records, a level at a time (every record still pending whose $base is not itself pending),
/// then the native tweaks.
///
/// # Safety
/// `db` is the TweakDB the engine just loaded; `derive` is the engine's TweakDBID::Derive or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tweaks_apply_all(db: *mut c_void, derive: Option<DeriveFn>) {
    let started = std::time::Instant::now();
    let records = read_records();
    let read = started.elapsed();
    let mut pending: HashSet<String> = records.iter().map(|r| r.name.clone()).collect();
    let mut applier = Applier {
        db,
        derive,
        created: 0,
        flats: 0,
        errors: 0,
    };
    let mut levels = 0;
    while !pending.is_empty() {
        let level: Vec<&Record> = records
            .iter()
            .filter(|r| pending.contains(&r.name))
            .filter(|r| match r.body.get("$base").and_then(Node::text) {
                Some(base) => base == r.name || !pending.contains(&base),
                None => true,
            })
            .collect();
        if level.is_empty() {
            for r in records.iter().filter(|r| pending.contains(&r.name)) {
                applier.fail(r, "is in a $base cycle");
            }
            break;
        }
        for r in &level {
            pending.remove(&r.name);
        }
        applier.apply_level(&level);
        levels += 1;
    }
    // The port's native tweaks (crate::native_tweaks).
    let tweaks: [NativeTweak; 2] = [
        crate::native_tweaks::shrink_on_foot_capsules,
        crate::native_tweaks::add_launcher_ready_state,
    ];
    for t in &tweaks {
        // SAFETY: a native tweak, with the TweakDB.
        unsafe { t(db) };
    }
    let total = started.elapsed();
    log!(
        "[tweaks] {} records from config\\tweaks in {} level(s): {} created, {} flats written, {} errors, {} native tweak(s) (read {:.1} ms, applied {:.1} ms)\n",
        records.len(),
        levels,
        applier.created,
        applier.flats,
        applier.errors,
        tweaks.len(),
        read.as_secs_f64() * 1000.0,
        (total - read).as_secs_f64() * 1000.0
    );
}

/// RetireLegacyCopies: an earlier release's copies of the port's files in r6\tweaks taken out (same bytes: deleted;
/// different: renamed .legacy), and a file no release carries any more removed.
#[no_mangle]
pub extern "C" fn cvr_tweaks_retire_legacy() {
    let retired = tweaks_dir().join("vrport").join("VRPort.LobbyBody.yaml");
    if std::fs::remove_file(&retired).is_ok() {
        log!(
            "[tweaks] removed {}: no release carries it any more\n",
            retired.display()
        );
    }
    let Some(legacy) = std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.parent()?.parent()?.join("r6").join("tweaks")))
    else {
        return;
    };
    let mut files = Vec::new();
    walk_all(&tweaks_dir(), &mut files);
    for file in files {
        let Ok(relative) = file.strip_prefix(tweaks_dir()) else {
            continue;
        };
        let old = legacy.join(relative);
        if !old.exists() {
            continue;
        }
        if std::fs::read(&old).ok() == std::fs::read(&file).ok() {
            let _ = std::fs::remove_file(&old);
            log!(
                "[tweaks] removed the earlier release's copy {}\n",
                old.display()
            );
        } else {
            let mut aside = old.clone().into_os_string();
            aside.push(".legacy");
            let aside = PathBuf::from(aside);
            let _ = std::fs::rename(&old, &aside);
            log!(
                "[tweaks] set the earlier release's copy aside: {}\n",
                aside.display()
            );
        }
        let mut dir = old.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if d == legacy || std::fs::read_dir(&d).map_or(true, |mut e| e.next().is_some()) {
                break;
            }
            let _ = std::fs::remove_dir(&d);
            dir = d.parent().map(Path::to_path_buf);
        }
    }
}
fn walk_all(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => walk_all(&e.path(), out),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_keeps_tags_and_nulls() {
        let n = load("A.b:\n  $base: A.c\n  list:\n    - !append-once X\n    - Y\n  empty:\n  v: {x: 1, y: 2}\n").unwrap();
        let Node::Map { entries, .. } = &n else {
            panic!()
        };
        let body = &entries[0].1;
        assert_eq!(
            body.get("$base").and_then(Node::text).as_deref(),
            Some("A.c")
        );
        let Some(Node::Seq { items, .. }) = body.get("list") else {
            panic!()
        };
        assert!(op_of(&items[0]) == Op::AppendOnce && op_of(&items[1]) == Op::Plain);
        assert!(matches!(body.get("empty"), Some(Node::Null)));
        assert!(matches!(body.get("v"), Some(Node::Map { .. })));
    }

    #[test]
    fn numbers_read_as_the_crt_reads_them() {
        assert_eq!(stof(" 1.5abc"), Some(1.5));
        assert_eq!(stof("x"), None);
        assert_eq!(stoi("42px"), Some(42));
        assert_eq!(stoi("99999999999"), None);
    }
}
