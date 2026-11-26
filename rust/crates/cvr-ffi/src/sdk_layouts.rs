//! The RED4ext.SDK class layouts the port is compiled against, checked against the running game (formerly
//! src/Core/SdkLayouts.cpp). The SDK's headers carry the field offsets of the game build they were generated from, and
//! the C++ half reads engine objects through them; after a game patch a class whose fields moved is read wrong with no
//! error anywhere. The game's RTTI holds the real layout, so once, right after the types are registered, every class
//! the port includes (the generated table, tools/patchday/sdk_layouts.py gen, which the C++ half hands over) is checked
//! by its size and by each field RTTI knows.
//!
//! Report only. A field RTTI does not list is counted, not checked. Every read of the engine's classes is guarded: if
//! the SDK's own view of CClass/CProperty no longer fits, the answer is a line saying so.
use core::ffi::{CStr, c_char};

use cvr_engine::code;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::RttiSystem;

/// CClass: parent +0x10, props (DynArray<CProperty*>) +0x28, size +0x68 (RTTITypes.hpp).
const CLASS_PARENT: usize = 0x10;
const CLASS_PROPS: usize = 0x28;
const CLASS_SIZE: usize = 0x68;
/// CProperty: name +0x08, valueOffset +0x20 (CProperty.hpp).
const PROPERTY_NAME: usize = 0x08;
const PROPERTY_OFFSET: usize = 0x20;
/// The lines a report prints at most; the rest are counted.
const PRINTED: u32 = 40;

/// The generated table's field (SdkLayouts.cpp SdkField).
#[repr(C)]
pub struct SdkField {
    pub name: *const c_char,
    pub offset: u32,
}
/// The generated table's class (SdkLayouts.cpp SdkType).
#[repr(C)]
pub struct SdkType {
    pub rtti: *const c_char,
    pub size: u32,
    pub fields: *const SdkField,
    pub count: u32,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Tally {
    pub types: u32,
    pub unknown: u32,
    pub sizes: u32,
    pub fields: u32,
    pub offsets: u32,
    pub absent: u32,
    pub printed: u32,
}

/// A read of the engine's class data faulted.
struct Fault;

fn read<T: Copy>(address: usize) -> Result<T, Fault> {
    code::read::<T>(address).ok_or(Fault)
}

/// CClass's own property `name`, or a parent's.
fn find_property(mut class: usize, name: CName) -> Result<Option<usize>, Fault> {
    while class != 0 {
        let entries = read::<usize>(class + CLASS_PROPS)?;
        let size = read::<u32>(class + CLASS_PROPS + 12)?;
        for i in 0..size as usize {
            let property = read::<usize>(entries + i * 8)?;
            if property != 0 && read::<u64>(property + PROPERTY_NAME)? == name.0 {
                return Ok(Some(property));
            }
        }
        class = read::<usize>(class + CLASS_PARENT)?;
    }
    Ok(None)
}

fn text(p: *const c_char) -> &'static str {
    if p.is_null() {
        return "";
    }
    // SAFETY: a string literal of the generated table.
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

fn compare(
    types: &[SdkType],
    t: &mut Tally,
    class_of: &dyn Fn(CName) -> usize,
) -> Result<(), Fault> {
    for ty in types {
        t.types += 1;
        let rtti = text(ty.rtti);
        let class = class_of(CName::hashed(rtti));
        if class == 0 {
            t.unknown += 1;
            if t.printed < PRINTED {
                log!("[sdk-layout] {}: not a class in this game's RTTI\n", rtti);
            }
            t.printed += 1;
            continue;
        }
        let size = read::<u32>(class + CLASS_SIZE)?;
        if size != ty.size {
            t.sizes += 1;
            if t.printed < PRINTED {
                log!(
                    "[sdk-layout] MISMATCH {} size: SDK 0x{:X}, game 0x{:X}\n",
                    rtti,
                    ty.size,
                    size
                );
            }
            t.printed += 1;
        }
        for i in 0..ty.count as usize {
            // SAFETY: the generated table's fields.
            let field = unsafe { &*ty.fields.add(i) };
            let name = text(field.name);
            let Some(property) = find_property(class, CName::hashed(name))? else {
                t.absent += 1;
                continue;
            };
            t.fields += 1;
            let offset = read::<u32>(property + PROPERTY_OFFSET)?;
            if offset != field.offset {
                t.offsets += 1;
                if t.printed < PRINTED {
                    log!(
                        "[sdk-layout] MISMATCH {}.{}: SDK +0x{:X}, game +0x{:X}\n",
                        rtti,
                        name,
                        field.offset,
                        offset
                    );
                }
                t.printed += 1;
            }
        }
    }
    Ok(())
}

/// CyberpunkVR_CheckSdkLayouts: the table checked against this game's RTTI, one summary line (or the fault's).
///
/// # Safety
/// `types` points at `count` entries of the generated table.
#[no_mangle]
pub unsafe extern "C" fn cvr_sdk_layouts_check(types: *const SdkType, count: usize) {
    let types = if types.is_null() {
        &[][..]
    } else {
        // SAFETY: the caller's table.
        unsafe { core::slice::from_raw_parts(types, count) }
    };
    let rtti = RttiSystem::get();
    let class_of = |name: CName| -> usize {
        rtti.and_then(|r| r.get_class(name))
            .map_or(0, |c| c as *const _ as usize)
    };
    let mut t = Tally::default();
    if compare(types, &mut t, &class_of).is_err() {
        log!(
            "[sdk-layout] the check faulted after {} classes (the SDK's CClass/CProperty view no longer fits this game) -- not checked\n",
            t.types
        );
        return;
    }
    log!(
        "[sdk-layout] {} classes ({} not in RTTI), {} fields checked: {} size and {} offset mismatches ({} SDK fields RTTI does not list)\n",
        t.types,
        t.unknown,
        t.fields,
        t.sizes,
        t.offsets,
        t.absent
    );
}
