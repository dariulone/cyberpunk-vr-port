//! The native prepared pair's source inputs (src/Render/NativePreparedPair.cpp, `prepared_pair::HasBaseline` and
//! `FullT5View`): a source's captured pass baseline holds the three color targets and the depth of slots 0, 1, 2 and 4
//! (its depth the pass depth, when one is kept); and a full t5 snapshot is the source's own (its copy on the source
//! recording's list and generation at the source's occurrence, a whole 1 MiB range from offset 0; the recorded view's
//! equality stays the .cpp's).
//!
//! In the Rust build (`CVR_RUST_VRIK`) both forward to `cvr_npp_baseline` and `cvr_npp_full_t5`; the unit tests below
//! cover each condition.

/// structured_buffer_snapshot::detail::SourceBytes.
pub const SOURCE_BYTES: u64 = 1024 * 1024;

/// HasBaseline's target checks: `resources` the baseline's five target resources.
pub fn baseline(colors: u32, depth: u64, resources: &[u64; 5]) -> bool {
    colors == 3 && (depth == 0 || depth == resources[4]) && [0usize, 1, 2, 4].iter().all(|&slot| resources[slot] != 0)
}
/// FullT5View's identity checks.
#[allow(clippy::too_many_arguments)]
pub fn full_t5(copy_list: u64, copy_generation: u64, copy_occurrence: u64, list: u64, generation: u64, ordinal: u64, source_offset: u64, copied_bytes: u64) -> bool {
    copy_list == list && copy_generation == generation && copy_occurrence == ordinal.wrapping_add(1) && source_offset == 0 && copied_bytes == SOURCE_BYTES
}

// ---- the C ABI

/// # Safety
/// `resources` holds five addresses.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_baseline(colors: u32, depth: u64, resources: *const [u64; 5]) -> bool {
    baseline(colors, depth, unsafe { &*resources })
}
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C-unwind" fn cvr_npp_full_t5(copy_list: u64, copy_generation: u64, copy_occurrence: u64, list: u64, generation: u64, ordinal: u64, source_offset: u64, copied_bytes: u64) -> bool {
    full_t5(copy_list, copy_generation, copy_occurrence, list, generation, ordinal, source_offset, copied_bytes)
}

#[cfg(test)]
mod tests {
    //! Each baseline slot and each snapshot identity field.
    use super::*;

    #[test]
    fn baselines() {
        let r = [1, 2, 3, 0, 5];
        assert!(baseline(3, 0, &r));
        assert!(baseline(3, 5, &r));
        assert!(!baseline(3, 6, &r));
        assert!(!baseline(2, 0, &r));
        assert!(!baseline(3, 0, &[1, 2, 0, 9, 5]));
        assert!(!baseline(3, 0, &[1, 2, 3, 9, 0]));
    }
    #[test]
    fn snapshots() {
        assert!(full_t5(7, 8, 10, 7, 8, 9, 0, SOURCE_BYTES));
        assert!(!full_t5(7, 8, 9, 7, 8, 9, 0, SOURCE_BYTES));
        assert!(!full_t5(7, 9, 10, 7, 8, 9, 0, SOURCE_BYTES));
        assert!(!full_t5(7, 8, 10, 7, 8, 9, 16, SOURCE_BYTES));
        assert!(!full_t5(7, 8, 10, 7, 8, 9, 0, SOURCE_BYTES - 1));
    }
}
