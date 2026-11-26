//! The native definition tracking's resource replay capture (src/Stereo/NativeDefinitionTracking.cpp, `CaptureReplay`'s
//! capture): with the resource pool in mode 1 (byte +2) before and after, each of its 220 slots (0x70C0 bytes apart)
//! yields its declaration count (+0x70C8, at most 600) and that many 48-byte declarations (+0x48), in slot order.
//!
//! And `ReplayGrammarUnchanged`: with the pool in mode 2 before and after, every slot still holds its captured
//! declaration count (read again after its rows) and every declaration -- read twice -- still has its captured kind, its
//! own slot and the captured bytes from 4 through its payload (resource_replay's payload sizes; an unknown kind fails),
//! all the captured declarations consumed.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the capture forwards to `cvr_ndt_replay_capture` (its rows lent until
//! `cvr_ndt_replay_free`) and the check to `cvr_ndt_replay_unchanged`; the unit tests below read a fabricated pool.

use crate::instance_layout::{read, read_u32};

pub const SLOTS: usize = 220;
pub const ROWS_PER_SLOT: u32 = 600;
pub const SLOT_STRIDE: u64 = 0x70C0;
pub const DECLARATIONS: u64 = 0x48;
pub const DECLARATION_COUNT: u64 = 0x70C8;

fn mode(pool: u64) -> Option<u8> {
    let mut b = [0u8; 1];
    read(pool.wrapping_add(2), &mut b).then_some(b[0])
}
/// The pool's declaration counts and rows, or None when it is not stable in mode 1 or a count is out of range.
pub fn capture(pool: u64, counts: &mut [u32; SLOTS]) -> Option<Vec<[u8; 48]>> {
    if mode(pool)? != 1 {
        return None;
    }
    let mut rows: Vec<[u8; 48]> = Vec::new();
    for (slot, count) in counts.iter_mut().enumerate() {
        let at = pool.wrapping_add(slot as u64 * SLOT_STRIDE);
        *count = read_u32(at.wrapping_add(DECLARATION_COUNT))?;
        if *count > ROWS_PER_SLOT {
            return None;
        }
        if *count != 0 {
            let mut bytes = vec![0u8; *count as usize * 48];
            if !read(at.wrapping_add(DECLARATIONS), &mut bytes) {
                return None;
            }
            rows.extend(bytes.chunks_exact(48).map(|c| <[u8; 48]>::try_from(c).unwrap()));
        }
    }
    (mode(pool)? == 1).then_some(rows)
}

/// ReplayGrammarUnchanged against the captured counts and declarations.
pub fn grammar_unchanged(pool: u64, declared: &[u32; SLOTS], expected: &[[u8; 48]]) -> bool {
    if mode(pool) != Some(2) {
        return false;
    }
    let mut index = 0usize;
    for (slot, &declared_count) in declared.iter().enumerate() {
        let at = pool.wrapping_add(slot as u64 * SLOT_STRIDE);
        let Some(count) = read_u32(at.wrapping_add(DECLARATION_COUNT)) else { return false };
        if count != declared_count {
            return false;
        }
        for i in 0..count {
            let row = at.wrapping_add(DECLARATIONS + u64::from(i) * 48);
            let (mut a, mut b) = ([0u8; 48], [0u8; 48]);
            if index >= expected.len() || !read(row, &mut a) || !read(row, &mut b) {
                return false;
            }
            let e = &expected[index];
            let bytes = crate::resource_replay::payload_bytes(u32::from(e[0]));
            if bytes == u32::MAX {
                return false;
            }
            let end = (8 + bytes as usize).min(48);
            let equal = |v: &[u8; 48]| v[0] == e[0] && u32::from(v[1]) == slot as u32 && v[4..end] == e[4..end];
            if !equal(&a) || !equal(&b) {
                return false;
            }
            index += 1;
        }
        if read_u32(at.wrapping_add(DECLARATION_COUNT)) != Some(count) {
            return false;
        }
    }
    index == expected.len() && mode(pool) == Some(2)
}

// ---- the C ABI

/// # Safety
/// `declared` holds 220 counts; `expected` holds `count` declarations (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_replay_unchanged(pool: u64, declared: *const [u32; SLOTS], expected: *const [u8; 48], count: usize) -> bool {
    let expected = if expected.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(expected, count) } };
    grammar_unchanged(pool, unsafe { &*declared }, expected)
}

/// # Safety
/// `counts` holds 220 counts; `rows`, `length` and `handle` are writable. On success the rows stay lent until
/// `cvr_ndt_replay_free(handle)`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_replay_capture(pool: u64, counts: *mut [u32; SLOTS], rows: *mut *const u8, length: *mut usize, handle: *mut *mut Vec<[u8; 48]>) -> bool {
    match capture(pool, unsafe { &mut *counts }) {
        Some(found) => {
            let boxed = Box::new(found);
            unsafe {
                *rows = boxed.as_ptr().cast();
                *length = boxed.len();
                *handle = Box::into_raw(boxed);
            }
            true
        }
        None => false,
    }
}
/// # Safety
/// `handle` came from `cvr_ndt_replay_capture` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_replay_free(handle: *mut Vec<[u8; 48]>) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[cfg(test)]
mod tests {
    //! A fabricated pool.
    use super::*;

    fn pool() -> Vec<u8> {
        let mut m = vec![0u8; SLOTS * SLOT_STRIDE as usize + 16];
        m[2] = 1;
        let slot = |s: usize| s * SLOT_STRIDE as usize;
        m[slot(3) + DECLARATION_COUNT as usize..][..4].copy_from_slice(&2u32.to_le_bytes());
        m[slot(3) + DECLARATIONS as usize] = 0xA1;
        m[slot(3) + DECLARATIONS as usize + 48] = 0xA2;
        m[slot(219) + DECLARATION_COUNT as usize..][..4].copy_from_slice(&1u32.to_le_bytes());
        m[slot(219) + DECLARATIONS as usize + 47] = 0xB1;
        m
    }
    #[test]
    fn captures() {
        let m = pool();
        let mut counts = [0u32; SLOTS];
        let rows = capture(m.as_ptr() as u64, &mut counts).unwrap();
        assert_eq!((counts[3], counts[219], counts.iter().sum::<u32>()), (2, 1, 3));
        assert_eq!((rows.len(), rows[0][0], rows[1][0], rows[2][47]), (3, 0xA1, 0xA2, 0xB1));
    }
    #[test]
    fn refusals() {
        let mut m = pool();
        m[2] = 0;
        assert!(capture(m.as_ptr() as u64, &mut [0; SLOTS]).is_none());
        let mut m = pool();
        m[3 * SLOT_STRIDE as usize + DECLARATION_COUNT as usize..][..4].copy_from_slice(&601u32.to_le_bytes());
        assert!(capture(m.as_ptr() as u64, &mut [0; SLOTS]).is_none());
        let mut m = pool();
        m[3 * SLOT_STRIDE as usize + DECLARATION_COUNT as usize..][..4].copy_from_slice(&600u32.to_le_bytes());
        assert_eq!(capture(m.as_ptr() as u64, &mut [0; SLOTS]).map(|r| r.len()), Some(601));
        assert!(capture(0, &mut [0; SLOTS]).is_none());
    }
    #[test]
    fn grammars() {
        // Slot 3's two rows are kind 2 (16 payload bytes) and slot 219's one row kind 0; the pool in mode 2.
        let mut m = pool();
        m[2] = 2;
        let slot = |s: usize| s * SLOT_STRIDE as usize + DECLARATIONS as usize;
        for (at, s) in [(slot(3), 3u8), (slot(3) + 48, 3), (slot(219), 219)] {
            m[at] = if s == 3 { 2 } else { 0 };
            m[at + 1] = s;
            m[at + 5] = 7;
        }
        let mut declared = [0u32; SLOTS];
        declared[3] = 2;
        declared[219] = 1;
        let rows = |m: &Vec<u8>| [slot(3), slot(3) + 48, slot(219)].map(|at| <[u8; 48]>::try_from(&m[at..at + 48]).unwrap());
        let expected = rows(&m);
        let base = m.as_ptr() as u64;
        assert!(grammar_unchanged(base, &declared, &expected));
        // A byte past the kind's payload may change; one inside it may not.
        let mut changed = m.clone();
        changed[slot(3) + 8 + 16] = 9;
        assert!(grammar_unchanged(changed.as_ptr() as u64, &declared, &expected));
        changed[slot(3) + 8 + 15] = 9;
        assert!(!grammar_unchanged(changed.as_ptr() as u64, &declared, &expected));
        // Bytes 2 and 3 are padding; byte 1 must be the slot.
        let mut changed = m.clone();
        changed[slot(3) + 2] = 9;
        assert!(grammar_unchanged(changed.as_ptr() as u64, &declared, &expected));
        changed[slot(3) + 1] = 4;
        assert!(!grammar_unchanged(changed.as_ptr() as u64, &declared, &expected));
        assert!(!grammar_unchanged(base, &declared, &expected[..2]));
        let mut more = declared;
        more[219] = 2;
        assert!(!grammar_unchanged(base, &more, &expected));
        let mut idle = m.clone();
        idle[2] = 1;
        assert!(!grammar_unchanged(idle.as_ptr() as u64, &declared, &expected));
        let mut unknown = expected;
        unknown[2][0] = 99;
        assert!(!grammar_unchanged(base, &declared, &unknown));
    }
    #[test]
    fn lends() {
        let m = pool();
        let (mut counts, mut rows, mut length, mut handle) = ([0u32; SLOTS], core::ptr::null(), 0usize, core::ptr::null_mut());
        assert!(unsafe { cvr_ndt_replay_capture(m.as_ptr() as u64, &mut counts, &mut rows, &mut length, &mut handle) });
        assert_eq!((length, unsafe { *rows }), (3, 0xA1));
        unsafe { cvr_ndt_replay_free(handle) };
    }
}
