//! The native definition tracking's native reads (src/Stereo/NativeDefinitionTracking.cpp, `EarlyExecutionGroup` and
//! `SavedVectorMatches`): a graph slot's early execution group is the aligned group pointer read twice alike at +0xF0,
//! with its references (+0x1C) positive and below the 32-bit maximum and its pending count (+0x18) not negative (else
//! 0); and a retained owner's saved definition vector still holds its definitions -- the trailer after its capacity
//! read twice alike and equal to the empty allocator, every definition in its slot, and as many definitions as the
//! saved count.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ndt_early_group` and `cvr_ndt_saved_vector`; the unit
//! tests below read fabricated memory.

use crate::instance_layout::{read_u32, read_u64};

/// EarlyExecutionGroup.
pub fn early_group(slot: u64) -> u64 {
    if slot > u64::MAX - 0xF8 {
        return 0;
    }
    let (Some(group), Some(again)) = (read_u64(slot + 0xF0), read_u64(slot + 0xF0)) else { return 0 };
    if group == 0 || group != again || group & 7 != 0 || group > u64::MAX - 0x20 {
        return 0;
    }
    let (Some(refs), Some(pending)) = (read_u32(group + 0x1C).map(|v| v as i32), read_u32(group + 0x18).map(|v| v as i32)) else { return 0 };
    if refs <= 0 || refs == i32::MAX || pending < 0 {
        return 0;
    }
    group
}
/// SavedVectorMatches: `data`, `capacity` and `count` the saved header, `empty` the empty allocator's data.
pub fn saved_vector(data: u64, capacity: u32, count: u32, empty: u64, definitions: &[u64]) -> bool {
    if capacity == 0 {
        return false;
    }
    let trailer = data.wrapping_add(u64::from(capacity) * 8);
    let (Some(footer), Some(again)) = (read_u64(trailer), read_u64(trailer)) else { return false };
    if footer != again || footer != empty {
        return false;
    }
    definitions.iter().enumerate().all(|(i, &object)| read_u64(data.wrapping_add(i as u64 * 8)) == Some(object)) && definitions.len() == count as usize
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_early_group(slot: u64) -> u64 {
    early_group(slot)
}
/// # Safety
/// `definitions` holds `count_definitions` objects (null when empty).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_saved_vector(data: u64, capacity: u32, count: u32, empty: u64, definitions: *const u64, count_definitions: usize) -> bool {
    let definitions = if definitions.is_null() || count_definitions == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(definitions, count_definitions) } };
    saved_vector(data, capacity, count, empty, definitions)
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    #[repr(C, align(8))]
    struct Memory([u8; 0x200]);
    fn put(m: &mut Memory, at: usize, bytes: &[u8]) {
        m.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    #[test]
    fn groups() {
        let mut m = Box::new(Memory([0; 0x200]));
        let base = m.0.as_ptr() as u64;
        let group = base + 0x100;
        put(&mut m, 0xF0, &group.to_le_bytes());
        put(&mut m, 0x11C, &2i32.to_le_bytes());
        put(&mut m, 0x118, &0i32.to_le_bytes());
        assert_eq!(early_group(base), group);
        put(&mut m, 0x118, &(-1i32).to_le_bytes());
        assert_eq!(early_group(base), 0);
        put(&mut m, 0x118, &3i32.to_le_bytes());
        put(&mut m, 0x11C, &0i32.to_le_bytes());
        assert_eq!(early_group(base), 0);
        put(&mut m, 0x11C, &i32::MAX.to_le_bytes());
        assert_eq!(early_group(base), 0);
        put(&mut m, 0x11C, &1i32.to_le_bytes());
        assert_eq!(early_group(base), group);
        put(&mut m, 0xF0, &(group + 4).to_le_bytes());
        assert_eq!(early_group(base), 0);
        put(&mut m, 0xF0, &0u64.to_le_bytes());
        assert_eq!(early_group(base), 0);
        assert_eq!(early_group(u64::MAX - 0xF0), 0);
    }
    #[test]
    fn vectors() {
        let mut m = Box::new(Memory([0; 0x200]));
        let data = m.0.as_ptr() as u64;
        put(&mut m, 0, &11u64.to_le_bytes());
        put(&mut m, 8, &12u64.to_le_bytes());
        put(&mut m, 32, &0xABCDu64.to_le_bytes()); // the trailer after a capacity of four
        assert!(saved_vector(data, 4, 2, 0xABCD, &[11, 12]));
        assert!(!saved_vector(data, 4, 3, 0xABCD, &[11, 12]));
        assert!(!saved_vector(data, 4, 2, 0xABCE, &[11, 12]));
        assert!(!saved_vector(data, 4, 2, 0xABCD, &[11, 13]));
        assert!(!saved_vector(data, 0, 0, 0xABCD, &[]));
        assert!(!saved_vector(data, 3, 2, 0xABCD, &[11, 12]));
        assert!(saved_vector(data, 4, 0, 0xABCD, &[]));
    }
}
