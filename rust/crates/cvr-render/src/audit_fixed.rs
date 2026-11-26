//! The descriptor audit's fixed borrowed range snapshot (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::CaptureBorrowedFixedRange` and `FixedCanonical`): under the metadata lock, each slot of a fixed range's
//! canonical value in the store (an absent slot left all zero), its resource cookie and the registered resource of that
//! cookie as its candidate; and the saved values still the store's (an absent slot matching only an all-zero value), for
//! at most the store's slot limit of a valid range.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the snapshot's slot walk forwards to `cvr_sda_fixed_snapshot` and FixedCanonical to
//! `cvr_sda_fixed_canonical`, on the .cpp's Rust store. `tools/single_pass_tests/descriptor_audit_test` reaches only
//! the refusals; the unit test below covers both walks.

use crate::audit_lifetimes::registered_resource;
use crate::descriptor_store::{RangeIdentity, Store, Value, SLOT_LIMIT};

/// The snapshot: `canonical` and `candidates` hold one entry per slot of `range`.
pub fn snapshot(store: &Store, range: &RangeIdentity, canonical: &mut [Value], candidates: &mut [u64]) {
    for (i, (value, candidate)) in canonical.iter_mut().zip(candidates.iter_mut()).enumerate().take(range.count as usize) {
        let Some(current) = store.get(store.at(range.cpu.wrapping_add(i as u64 * range.stride as u64))) else {
            continue;
        };
        *value = *current;
        if let Some(resource) = registered_resource(current[2]) {
            *candidate = resource as u64;
        }
    }
}
/// FixedCanonical.
pub fn canonical(store: &Store, saved: &[Value], range: &RangeIdentity) -> bool {
    if saved.len() != range.count as usize || !range.valid || range.cpu == 0 || range.stride == 0 || range.count as usize > SLOT_LIMIT {
        return false;
    }
    saved.iter().enumerate().all(|(i, value)| match store.get(store.at(range.cpu + i as u64 * range.stride as u64)) {
        Some(current) => current == value,
        None => *value == [0; 20],
    })
}

// ---- the C ABI

/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `range` is valid; `canonical` and `candidates` hold
/// `range.count` zeroed entries.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_fixed_snapshot(store: *const Store, range: *const RangeIdentity, canonical: *mut Value, candidates: *mut u64) {
    let range = unsafe { &*range };
    let n = range.count as usize;
    if n == 0 {
        return;
    }
    let (canonical, candidates) = unsafe { (core::slice::from_raw_parts_mut(canonical, n), core::slice::from_raw_parts_mut(candidates, n)) };
    snapshot(unsafe { &*store }, range, canonical, candidates);
}
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `saved` holds `count` values; `range` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_fixed_canonical(store: *const Store, saved: *const Value, count: usize, range: *const RangeIdentity) -> bool {
    let saved = if saved.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(saved, count) } };
    canonical(unsafe { &*store }, saved, unsafe { &*range })
}

#[cfg(test)]
mod tests {
    //! A fixed range's snapshot (a registered and an unregistered cookie, an absent slot) and its canonical re-check.
    use super::*;

    #[test]
    fn snapshots_and_rechecks_fixed_ranges() {
        let (cpu, gpu, stride) = (0x50_0000u64, 0x60_0000u64, 32u32);
        let mut store = Store::new(false);
        assert!(store.heap_created(cpu, gpu, 4, stride, 0xB109));
        let mut value = [0u64; 20];
        value[0] = 2;
        value[1] = 1;
        value[2] = 0xB109_0001;
        store.write_value(cpu, stride, &value);
        let range = store.range(gpu, 3);
        let (mut canonical, mut candidates) = ([[0u64; 20]; 3], [0u64; 3]);
        snapshot(&store, &range, &mut canonical, &mut candidates);
        assert_eq!((canonical[0], canonical[1], candidates), (value, [0; 20], [0; 3]));
        assert!(canonical_matches(&store, &canonical, &range));
        assert!(!canonical_matches(&store, &canonical[..2], &range));
        let mut changed = value;
        changed[4] = 7;
        store.write_value(cpu + stride as u64, stride, &changed);
        assert!(!canonical_matches(&store, &canonical, &range));
    }
    fn canonical_matches(store: &Store, saved: &[Value], range: &RangeIdentity) -> bool {
        canonical(store, saved, range)
    }
}
