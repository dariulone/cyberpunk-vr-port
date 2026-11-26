//! The descriptor audit's slot writes (src/Render/StereoDescriptorAudit.cpp, `descriptors::`): one canonical value into
//! a slot (in place for live metadata, otherwise interned), a private write's exact invalidation of tracked slots (a
//! rejected one fails the tracking), and a CopyDescriptors call's walk -- equal destination and source totals within
//! the store's slot budget, copied as the longest runs both sides share.
//!
//! In the Rust build (`CVR_RUST_VRIK`) Put's store write, InvalidateInternal's and Copy's tracked invalidations and
//! Copy's walk forward to `cvr_sda_*` on the .cpp's Rust store; `tools/single_pass_tests/descriptor_audit_test` runs its
//! cases against them in its Rust variant.

use crate::descriptor_store::{Store, Value, SLOT_LIMIT};

/// TrackedDescriptorWrite::Rejected.
const REJECTED: u32 = 2;

/// Put's store write.
pub fn put(store: &mut Store, cpu: u64, stride: u32, value: &Value, live: bool) {
    if live {
        store.write_value(cpu, stride, value);
    } else {
        let id = store.intern(value);
        store.write(cpu, stride, id);
    }
}
/// The tracked slots of each destination run invalidated; false when the store rejects one (the walk stops there).
pub fn invalidate(store: &mut Store, destinations: &[u64], sizes: Option<&[u32]>, stride: u32) -> bool {
    for (i, &cpu) in destinations.iter().enumerate() {
        if store.invalidate_tracked(cpu, sizes.map_or(1, |s| s[i]), stride) == REJECTED {
            return false;
        }
    }
    true
}
/// Copy's walk; false when the totals differ or pass the slot budget.
pub fn copy(store: &mut Store, destinations: &[u64], sizes: Option<&[u32]>, sources: &[u64], counts: Option<&[u32]>, stride: u32) -> bool {
    let size = |i: usize| sizes.map_or(1, |s| s[i]);
    let count = |i: usize| counts.map_or(1, |c| c[i]);
    let a: u64 = (0..destinations.len()).map(|i| size(i) as u64).sum();
    let b: u64 = (0..sources.len()).map(|i| count(i) as u64).sum();
    if a != b || a > SLOT_LIMIT as u64 {
        return false;
    }
    let (mut di, mut si, mut da, mut sa) = (0usize, 0usize, 0u32, 0u32);
    while di < destinations.len() && si < sources.len() {
        let (dn, sn) = (size(di), count(si));
        let step = (dn - da).min(sn - sa);
        if step != 0 {
            store.copy(destinations[di] + da as u64 * stride as u64, sources[si] + sa as u64 * stride as u64, step, stride);
        }
        da += step;
        sa += step;
        if da == dn {
            di += 1;
            da = 0;
        }
        if sa == sn {
            si += 1;
            sa = 0;
        }
    }
    true
}

// ---- the C ABI

fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the .cpp's arrays, alive for the call.
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}
/// # Safety
/// `store` is the .cpp's Rust store, held under its metadata lock; `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_put(store: *mut Store, cpu: u64, stride: u32, value: *const Value, live: bool) {
    put(unsafe { &mut *store }, cpu, stride, unsafe { &*value }, live);
}
/// # Safety
/// `store` as for cvr_sda_put; `destinations` (and `sizes`, when non-null) hold `count` entries.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_invalidate(store: *mut Store, destinations: *const u64, sizes: *const u32, count: usize, stride: u32) -> bool {
    let sizes = (!sizes.is_null()).then(|| slice(sizes, count));
    invalidate(unsafe { &mut *store }, slice(destinations, count), sizes, stride)
}
/// # Safety
/// `store` as for cvr_sda_put; the destination (and size) arrays hold `n` entries, the source (and count) arrays `m`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_copy(
    store: *mut Store,
    n: usize,
    destinations: *const u64,
    sizes: *const u32,
    m: usize,
    sources: *const u64,
    counts: *const u32,
    stride: u32,
) -> bool {
    let sizes = (!sizes.is_null()).then(|| slice(sizes, n));
    let counts = (!counts.is_null()).then(|| slice(counts, m));
    copy(unsafe { &mut *store }, slice(destinations, n), sizes, slice(sources, m), counts, stride)
}
