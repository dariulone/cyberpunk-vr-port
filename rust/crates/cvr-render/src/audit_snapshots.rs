//! The descriptor audit's snapshot re-checks (src/Render/StereoDescriptorAudit.cpp, `TableMetadataSnapshot::Data::
//! MatchesLocked`): under the metadata lock, a table metadata snapshot still matches when its root's layout is the one
//! it saved (the parameters, flags, reasons, support and every range's parameter, type, space, register, count, offset
//! and visibility), each heap still carries its cookie, is known and spans its whole extent in the store, and each saved
//! range's identity is still the store's; and (`BorrowedSourceRanges::Data::MatchesLocked`) a borrowed source range
//! snapshot still matches when each heap still carries its cookie and is known and each range's heap is one of them and
//! its identity is still the store's (a failure reports its diagnostic stage: 6 a heap, 10 a range's heap, 11 a changed
//! range).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the two MatchesLocked's layout, heap and range checks forward to
//! `cvr_sda_metadata_matches` and `cvr_sda_borrowed_matches` on the .cpp's Rust store (the heaps' current cookies read
//! by the .cpp; the tracking epoch checks stay the .cpp's). `tools/single_pass_tests/table_metadata_test` runs its cases
//! against the first in its Rust variant; the unit test below covers the second.

use crate::audit_borrowed::{Heap as BorrowedHeap, Range as BorrowedRange, Stage};
use crate::audit_lifetimes::heap_known;
use crate::audit_metadata::{Heap, Range};
use crate::audit_roots::with_layout;
use crate::descriptor_store::{same_borrowed_range, Store};
use crate::source_layout::Binding;

/// The saved root layout of a table metadata snapshot.
pub struct SavedLayout<'a> {
    pub ranges: &'a [Binding],
    pub parameters: u32,
    pub flags: u32,
    pub reasons: u32,
    pub supported: bool,
}

fn same_range(a: &Binding, b: &Binding) -> bool {
    a.parameter == b.parameter
        && a.kind == b.kind
        && a.space == b.space
        && a.shader_register == b.shader_register
        && a.count == b.count
        && a.table_offset == b.table_offset
        && a.visibility == b.visibility
}

/// MatchesLocked's layout, heap and range checks.
pub fn metadata_matches(store: &Store, root: usize, saved: &SavedLayout, heaps: &[Heap], current: &[u64], ranges: &[Range]) -> bool {
    let layout = with_layout(root, |actual| {
        actual.parameters == saved.parameters
            && actual.flags == saved.flags
            && actual.reasons == saved.reasons
            && actual.supported == saved.supported
            && actual.ranges.len() == saved.ranges.len()
            && actual.ranges.iter().zip(saved.ranges).all(|(a, b)| same_range(a, b))
    });
    if layout != Some(true) {
        return false;
    }
    for (heap, &cookie) in heaps.iter().zip(current) {
        if cookie != heap.cookie || !heap_known(cookie) {
            return false;
        }
        let full = store.range(heap.gpu, heap.count);
        if !full.valid || full.heap != cookie || full.cpu != heap.cpu || full.stride != heap.stride || full.count != heap.count {
            return false;
        }
    }
    ranges.iter().all(|range| same_borrowed_range(&range.identity, &store.range(range.identity.gpu, range.declared_count)))
}

/// The borrowed MatchesLocked's heap and range checks: the failure's stage, when any.
pub fn borrowed_matches(store: &Store, heaps: &[BorrowedHeap], current: &[u64], ranges: &[BorrowedRange]) -> Result<(), Stage> {
    for (i, (heap, &cookie)) in heaps.iter().zip(current).enumerate() {
        if cookie != heap.cookie || !heap_known(cookie) {
            return Err(Stage { stage: 6, index: i as u64, gpu: heap.gpu, expected: heap.cookie, actual: cookie });
        }
    }
    for (i, range) in ranges.iter().enumerate() {
        if range.heap_index as usize >= heaps.len() {
            return Err(Stage { stage: 10, index: i as u64, gpu: range.identity.gpu, expected: 0, actual: 0 });
        }
        let now = store.range(range.identity.gpu, range.used.count);
        if !same_borrowed_range(&range.identity, &now) {
            return Err(Stage { stage: 11, index: i as u64, gpu: range.identity.gpu, expected: range.identity.version, actual: now.version });
        }
    }
    Ok(())
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
/// `store` is the .cpp's Rust store under its metadata lock; `ranges` holds `count` bindings; `heaps` and `current`
/// hold `heap_count` entries; `saved` holds `range_count` summary ranges.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_metadata_matches(
    store: *const Store,
    root: *const core::ffi::c_void,
    ranges: *const Binding,
    count: usize,
    parameters: u32,
    flags: u32,
    reasons: u32,
    supported: bool,
    heaps: *const Heap,
    current: *const u64,
    heap_count: usize,
    saved: *const Range,
    range_count: usize,
) -> bool {
    let layout = SavedLayout { ranges: slice(ranges, count), parameters, flags, reasons, supported };
    metadata_matches(unsafe { &*store }, root as usize, &layout, slice(heaps, heap_count), slice(current, heap_count), slice(saved, range_count))
}
/// The borrowed re-check: true, or false with `failure` set.
///
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `heaps` and `current` hold `heap_count` entries; `ranges`
/// holds `range_count` snapshot ranges; `failure` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_borrowed_matches(
    store: *const Store,
    heaps: *const BorrowedHeap,
    current: *const u64,
    heap_count: usize,
    ranges: *const BorrowedRange,
    range_count: usize,
    failure: *mut Stage,
) -> bool {
    match borrowed_matches(unsafe { &*store }, slice(heaps, heap_count), slice(current, heap_count), slice(ranges, range_count)) {
        Ok(()) => true,
        Err(stage) => {
            unsafe { *failure = stage };
            false
        }
    }
}

#[cfg(test)]
mod tests {
    //! The borrowed re-check's stages on a stored heap: a changed cookie, an unknown heap, a range's missing heap and a
    //! rewritten range.
    use super::*;
    use crate::audit_lifetimes::heap_insert;
    use crate::descriptor_store::{BorrowedUsedRange, RangeIdentity};

    #[test]
    fn borrowed_re_check_stages() {
        let (cpu, gpu, stride, cookie) = (0x10_0000u64, 0x20_0000u64, 32u32, 0xB105_0001u64);
        let mut store = Store::new(false);
        store.set_track_versions(true);
        assert!(store.heap_created(cpu, gpu, 16, stride, cookie));
        let heaps = [BorrowedHeap { object: core::ptr::null_mut(), kind: 0, count: 16, cpu, gpu, stride, cookie }];
        let identity: RangeIdentity = store.range(gpu + 64, 4);
        assert!(identity.valid);
        let used = BorrowedUsedRange { count: 4, ..Default::default() };
        let mut ranges = [BorrowedRange { used, heap_index: 0, table: gpu, identity }];
        assert_eq!(borrowed_matches(&store, &heaps, &[cookie], &ranges), Err(Stage { stage: 6, index: 0, gpu, expected: cookie, actual: cookie }));
        heap_insert(cookie);
        assert_eq!(borrowed_matches(&store, &heaps, &[cookie], &ranges), Ok(()));
        assert_eq!(borrowed_matches(&store, &heaps, &[cookie + 1], &ranges), Err(Stage { stage: 6, index: 0, gpu, expected: cookie, actual: cookie + 1 }));
        ranges[0].heap_index = 1;
        assert_eq!(borrowed_matches(&store, &heaps, &[cookie], &ranges), Err(Stage { stage: 10, index: 0, gpu: gpu + 64, expected: 0, actual: 0 }));
        ranges[0].heap_index = 0;
        let mut value = [0u64; 20];
        value[0] = 4;
        store.write_value(cpu + 96, stride, &value);
        let now = store.range(gpu + 64, 4);
        assert_ne!(now.version, identity.version);
        assert_eq!(
            borrowed_matches(&store, &heaps, &[cookie], &ranges),
            Err(Stage { stage: 11, index: 0, gpu: gpu + 64, expected: identity.version, actual: now.version })
        );
    }
}
