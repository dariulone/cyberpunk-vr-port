//! The descriptor audit's source capture re-checks (src/Render/StereoDescriptorAudit.cpp): after the CBV backings are
//! resolved, under the metadata lock again, each captured table's range is still the store's (its heap, CPU address and
//! stride; 9 otherwise) and each used slot still holds its captured value (9 with its offset), a CBV's backing resource
//! taking its cookie now (7 when it has none or the value names another); and a stamp is still current
//! (`MatchesSourceSlot`, `MatchesCpuSourceSrv`) when its slot's range is a known heap's and the slot -- and the CPU
//! descriptor, when one is given -- holds exactly the stamped value.
//!
//! In the Rust build (`CVR_RUST_VRIK`) both captures' re-check loops forward to `cvr_sda_source_recheck` (the CBV
//! cookies written back) and the two stamp checks to `cvr_sda_stamp_current`, on the .cpp's Rust store.
//! `tools/single_pass_tests/source_table_capture_test` and `source_sparse_capture_test` run their cases against them in
//! their Rust variants.

use core::ffi::c_void;

use crate::audit_lifetimes::heap_known;
use crate::audit_sources::Failure;
use crate::audit_values::cookie;
use crate::descriptor_store::{RangeIdentity, Store, Value};

/// One captured slot as the .cpp lends it for the re-check.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RecheckSlot {
    pub value: Value,
    pub resource: *mut c_void,
    pub offset: u32,
    pub kind: u32,
    pub used: bool,
}
const _: () = assert!(core::mem::size_of::<RecheckSlot>() == 184);

/// The re-check of one captured table (`count` slots from its range's GPU address): the CBV cookies into `cookies`.
pub fn recheck(store: &Store, captured: &RangeIdentity, count: u32, slots: &[RecheckSlot], cookies: &mut [u64]) -> Result<(), Failure> {
    let range = store.range(captured.gpu, count);
    if !range.valid || range.heap != captured.heap || range.cpu != captured.cpu || range.stride != captured.stride {
        return Err(Failure { reason: 9, offset: u32::MAX });
    }
    for (slot, taken) in slots.iter().zip(cookies.iter_mut()) {
        if !slot.used {
            continue;
        }
        let value = store.get(store.at(range.cpu.wrapping_add(slot.offset as u64 * range.stride as u64)));
        if value != Some(&slot.value) {
            return Err(Failure { reason: 9, offset: slot.offset });
        }
        if slot.kind == 1 {
            *taken = cookie(slot.resource);
            if *taken == 0 || (slot.value[2] != 0 && slot.value[2] != *taken) {
                return Err(Failure { reason: 7, offset: slot.offset });
            }
        }
    }
    Ok(())
}
/// A stamp (`gpu`, `heap`, `value`) still current, and the CPU descriptor `cpu` (when non-zero) holding it too.
pub fn stamp_current(store: &Store, gpu: u64, heap: u64, value: &Value, cpu: u64) -> bool {
    let range = store.range(gpu, 1);
    if !range.valid || range.heap != heap || !heap_known(range.heap) {
        return false;
    }
    store.get(store.at(range.cpu)) == Some(value) && (cpu == 0 || store.get(store.at(cpu)) == Some(value))
}

// ---- the C ABI

/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `captured` is valid; `slots` and `cookies` hold
/// `slot_count` entries; `failure` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_source_recheck(
    store: *const Store,
    captured: *const RangeIdentity,
    count: u32,
    slots: *const RecheckSlot,
    slot_count: usize,
    cookies: *mut u64,
    failure: *mut Failure,
) -> bool {
    let (slots, cookies) = if slot_count == 0 {
        (&[][..], &mut [][..])
    } else {
        unsafe { (core::slice::from_raw_parts(slots, slot_count), core::slice::from_raw_parts_mut(cookies, slot_count)) }
    };
    match recheck(unsafe { &*store }, unsafe { &*captured }, count, slots, cookies) {
        Ok(()) => true,
        Err(error) => {
            unsafe { *failure = error };
            false
        }
    }
}
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_stamp_current(store: *const Store, gpu: u64, heap: u64, value: *const Value, cpu: u64) -> bool {
    stamp_current(unsafe { &*store }, gpu, heap, unsafe { &*value }, cpu)
}
