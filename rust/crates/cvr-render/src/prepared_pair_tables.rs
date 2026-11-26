//! The native prepared pair's table and plane checks (src/Render/NativePreparedPair.cpp, `prepared_pair::MembershipTable`
//! and `ValidateNativeExit`): a membership bank's t31 table is the source's one reference table for that parameter (the
//! same CPU heap, offset, count, storage scope and extent, identity and version) and covers the asked extent; and a native
//! exit's readback planes never overlap (each plane's byte interval disjoint from every earlier one).
//!
//! In the Rust build (`CVR_RUST_VRIK`) both forward to `cvr_npp_same_reference` and `cvr_npp_planes_disjoint`; the unit
//! tests below cover each field and the overlaps.

/// The table fields MembershipTable compares.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableKey {
    pub heap: u64,
    pub storage_scope: u64,
    pub identity: u64,
    pub version: u64,
    pub offset: u32,
    pub count: u32,
    pub storage_extent: u32,
}
const _: () = assert!(core::mem::size_of::<TableKey>() == 48);

/// MembershipTable's reference check.
pub fn same_reference(reference: &TableKey, table: &TableKey, extent: u32) -> bool {
    reference == table && extent <= table.storage_extent
}
/// ValidateNativeExit's overlap rule: the planes' [begin, end) intervals pairwise disjoint in order.
pub fn planes_disjoint(intervals: &[[u64; 2]]) -> bool {
    intervals.iter().enumerate().all(|(i, &[begin, end])| intervals[..i].iter().all(|&[b, e]| !(begin < e && b < end)))
}

// ---- the C ABI

/// # Safety
/// Both keys are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_same_reference(reference: *const TableKey, table: *const TableKey, extent: u32) -> bool {
    same_reference(unsafe { &*reference }, unsafe { &*table }, extent)
}
/// # Safety
/// `intervals` holds `count` intervals.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_planes_disjoint(intervals: *const [u64; 2], count: usize) -> bool {
    let intervals = if intervals.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(intervals, count) } };
    planes_disjoint(intervals)
}

#[cfg(test)]
mod tests {
    //! Each reference field and the plane overlaps.
    use super::*;

    #[test]
    fn references() {
        let t = TableKey { heap: 1, storage_scope: 2, identity: 3, version: 4, offset: 5, count: 6, storage_extent: 8 };
        assert!(same_reference(&t, &t, 8));
        assert!(!same_reference(&t, &t, 9));
        assert!(!same_reference(&TableKey { version: 5, ..t }, &t, 1));
        assert!(!same_reference(&TableKey { heap: 9, ..t }, &t, 1));
        assert!(!same_reference(&TableKey { count: 7, ..t }, &t, 1));
    }
    #[test]
    fn planes() {
        assert!(planes_disjoint(&[[0, 16], [16, 32], [64, 80]]));
        assert!(!planes_disjoint(&[[0, 16], [8, 24]]));
        assert!(!planes_disjoint(&[[16, 32], [0, 17]]));
        assert!(!planes_disjoint(&[[0, 16], [0, 8]]));
        assert!(planes_disjoint(&[]));
    }
}
