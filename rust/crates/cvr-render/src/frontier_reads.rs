//! The submission frontier's game reads (src/Render/NativeSubmissionFrontier.cpp, `Code` and `Graph`'s owned
//! execution snapshot): the frontier's code contract (its header's rows, passed in; each row's RVA and expected hash
//! reported, then -- once read within the 512-byte buffer -- its actual FNV-1a 64 hash); and the owned execution's
//! published statistics (an even sequence, status 2 or 3, its consumer, graph epoch, pair and frame owner set).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nsfr_code` and `cvr_nsfr_owned` (reads through
//! ReadProcessMemory, as the .cpp's `GraphRead`; code_tables' Row as the contract's row); the unit tests below read
//! fabricated memory.

use crate::code_tables::Row;
use crate::instance_layout::read;
use crate::native_recording::fingerprint;

/// Code: the rows with their diagnostics ([RVA, expected, actual]).
pub fn code_diagnosed(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, rows: &[Row], capacity: u32, diagnostic: &mut [u64; 3]) -> bool {
    for r in rows {
        *diagnostic = [r.rva.into(), r.hash, 0];
        if base > u64::MAX - u64::from(r.rva) || r.bytes > capacity || r.bytes == 0 {
            return false;
        }
        let mut bytes = vec![0u8; r.bytes as usize];
        if !read(base + u64::from(r.rva), &mut bytes) {
            return false;
        }
        diagnostic[2] = fingerprint(&bytes);
        if diagnostic[2] != r.hash {
            return false;
        }
    }
    true
}
/// Graph's owned execution snapshot.
pub fn owned_stats(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64, sequence: u32) -> Option<[u64; 24]> {
    if sequence & 1 != 0 {
        return None;
    }
    let mut raw = [0u8; 192];
    if !read(address, &mut raw) {
        return None;
    }
    let mut stats = [0u64; 24];
    for (i, s) in stats.iter_mut().enumerate() {
        *s = u64::from_le_bytes(raw[i * 8..i * 8 + 8].try_into().unwrap());
    }
    ((stats[0] == 2 || stats[0] == 3) && stats[1] != 0 && stats[2] != 0 && stats[3] != 0 && stats[6] != 0).then_some(stats)
}

// ---- the C ABI

/// # Safety
/// `rows` holds `count` rows (or is null with 0); `diagnostic` holds three values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsfr_code(base: u64, rows: *const Row, count: usize, diagnostic: *mut [u64; 3]) -> bool {
    let rows = if rows.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(rows, count) } };
    code_diagnosed(&read, base, rows, 512, unsafe { &mut *diagnostic })
}
/// # Safety
/// `out` holds 24 values (written only on success).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsfr_owned(address: u64, sequence: u32, out: *mut [u64; 24]) -> bool {
    match owned_stats(&read, address, sequence) {
        Some(stats) => {
            unsafe { *out = stats };
            true
        }
        None => false,
    }
}

/// SyncFrame (src/Render/NativeSubmissionFrontier.cpp): strictly up the stack (the TIB's limits, no leaf frames) to the
/// first frame in the image, which must be the submission caller's return (`caller_return`) inside its function
/// (0x655FAC): its RSI (the context) and RDI (the definition); None for any other frame or a walk that ends.
pub fn sync_frame(base: u64, caller_return: u64) -> Option<(u64, u64)> {
    // MaxUnwoundFrames, from four frames lower than the .cpp's walk.
    cvr_engine::stack::walk_strict(64 + 4, |c, _, _, function| {
        let f = function?;
        (f.image == base).then(|| (c.Rip == base + caller_return && f.begin == 0x655FAC && u64::from(f.end) > caller_return).then_some((c.Rsi, c.Rdi)))
    })
    .ok()
    .flatten()
}
/// SyncFrame into `context` and `definition` (0 unless the caller's frame was found); true when both are set.
///
/// # Safety
/// Both outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsfr_sync_frame(base: u64, caller_return: u64, context: *mut u64, definition: *mut u64) -> bool {
    let (c, d) = sync_frame(base, caller_return).unwrap_or((0, 0));
    unsafe { (*context, *definition) = (c, d) };
    c != 0 && d != 0
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    fn memory(base: u64, bytes: Vec<u8>) -> impl Fn(u64, &mut [u8]) -> bool {
        move |address, out| {
            let Some(at) = address.checked_sub(base).map(|a| a as usize) else { return false };
            if at.checked_add(out.len()).is_none_or(|e| e > bytes.len()) {
                return false;
            }
            out.copy_from_slice(&bytes[at..at + out.len()]);
            true
        }
    }
    #[test]
    fn codes() {
        let base = 0x1000u64;
        let bytes: Vec<u8> = (0..0x400u32).map(|i| (i * 3) as u8).collect();
        let hash = |rva: usize, n: usize| fingerprint(&bytes[rva..rva + n]);
        let rows = [Row { rva: 0x10, bytes: 0x20, hash: hash(0x10, 0x20) }, Row { rva: 0x100, bytes: 0x40, hash: hash(0x100, 0x40) }];
        let read = memory(base, bytes.clone());
        let mut d = [9u64; 3];
        assert!(code_diagnosed(&read, base, &rows, 512, &mut d));
        assert_eq!(d, [0x100, rows[1].hash, rows[1].hash]);
        // A wrong hash reports the actual one; an unreadable or oversized row reports none.
        let wrong = [Row { hash: 1, ..rows[0] }];
        assert!(!code_diagnosed(&read, base, &wrong, 512, &mut d));
        assert_eq!(d, [0x10, 1, rows[0].hash]);
        assert!(!code_diagnosed(&read, base, &[Row { rva: 0x3F0, bytes: 0x20, hash: 0 }], 512, &mut d));
        assert_eq!(d, [0x3F0, 0, 0]);
        assert!(!code_diagnosed(&read, base, &[Row { rva: 0x10, bytes: 513, hash: 0 }], 512, &mut d));
    }
    #[test]
    fn owned() {
        let mut raw = vec![0u8; 192];
        let put = |raw: &mut Vec<u8>, i: usize, v: u64| raw[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        for (i, v) in [(0, 3), (1, 7), (2, 8), (3, 9), (6, 10)] {
            put(&mut raw, i, v);
        }
        let read = memory(0x5000, raw.clone());
        assert_eq!(owned_stats(&read, 0x5000, 4).map(|s| (s[0], s[6])), Some((3, 10)));
        assert_eq!(owned_stats(&read, 0x5000, 5), None);
        for (i, v) in [(0, 1), (1, 0), (2, 0), (3, 0), (6, 0)] {
            let mut r = raw.clone();
            put(&mut r, i, v);
            assert_eq!(owned_stats(&memory(0x5000, r), 0x5000, 4), None, "{i}");
        }
        assert_eq!(owned_stats(&read, 0x5008, 4), None);
    }
}
