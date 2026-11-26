//! The CPU collector probe's comparisons (src/Stereo/CpuCollectorProbe.cpp, `CompareEye`'s query comparison and
//! `DifferentIds`): the owned clone query (the collector storage's +0xDF2F0) against the source query -- the same query
//! type, whose fine-materialize entry (+0x30) is the game's, then their frustum planes (+0x1A0, 128 bytes) and view
//! flags (+0x190, all but bytes 2, 3 and 15) compared byte for byte; and the difference of two id sets (sorted: the
//! length difference plus every differing position).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ccp_compare_query` and `cvr_ccp_different_ids` (reads
//! through ReadProcessMemory, as the .cpp's `Read`; the planes the reads reached are kept, as the .cpp's arrays are);
//! the unit tests below read fabricated memory.

use crate::instance_layout::read;

/// The query comparison.
#[repr(C)]
pub struct QueryComparison {
    pub clone_query: u64,
    pub source_type: u64,
    pub clone_type: u64,
    pub differences: u32,
    pub valid: bool,
    pub planes: [[u8; 128]; 2],
}
const _: () = assert!(core::mem::size_of::<QueryComparison>() == 32 + 256);
impl Default for QueryComparison {
    fn default() -> Self {
        QueryComparison { clone_query: 0, source_type: 0, clone_type: 0, differences: 0, valid: false, planes: [[0; 128]; 2] }
    }
}
/// The game's fine-materialize entry, from the image base.
pub const FINE_MATERIALIZE_RVA: u64 = 0x29D110;
const COMPARED_FLAGS: [usize; 13] = [0, 1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    (address != 0 && read(address, &mut b)).then(|| u64::from_le_bytes(b))
}
/// CompareEye's query comparison into `out` (default on entry).
pub fn compare_query(read: &impl Fn(u64, &mut [u8]) -> bool, storage: u64, source: u64, base: u64, out: &mut QueryComparison) {
    let rd = |address: u64, buffer: &mut [u8]| address != 0 && read(address, buffer);
    let mut valid = match u64_at(read, storage.wrapping_add(0xDF2F0)) {
        Some(clone) => {
            out.clone_query = clone;
            clone != 0
        }
        None => false,
    };
    valid = valid
        && match u64_at(read, source) {
            Some(t) => {
                out.source_type = t;
                true
            }
            None => false,
        };
    valid = valid
        && match u64_at(read, out.clone_query) {
            Some(t) => {
                out.clone_type = t;
                true
            }
            None => false,
        };
    valid = valid && out.source_type == out.clone_type && u64_at(read, out.source_type.wrapping_add(0x30)) == Some(base.wrapping_add(FINE_MATERIALIZE_RVA));
    let (mut source_flags, mut clone_flags) = ([0u8; 16], [0u8; 16]);
    if valid {
        let mut planes = [0u8; 128];
        valid = rd(source.wrapping_add(0x1A0), &mut planes);
        if valid {
            out.planes[0] = planes;
            valid = rd(out.clone_query.wrapping_add(0x1A0), &mut planes);
            if valid {
                out.planes[1] = planes;
            }
        }
        valid = valid && rd(source.wrapping_add(0x190), &mut source_flags) && rd(out.clone_query.wrapping_add(0x190), &mut clone_flags);
    }
    out.valid = valid;
    if valid {
        let planes = out.planes[0].iter().zip(&out.planes[1]).filter(|(a, b)| a != b).count();
        let flags = COMPARED_FLAGS.iter().filter(|&&i| source_flags[i] != clone_flags[i]).count();
        out.differences = (planes + flags) as u32;
    }
}
/// DifferentIds.
pub fn different_ids(a: &[u64], b: &[u64]) -> u32 {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    let count = a.len().abs_diff(b.len());
    (count + a.iter().zip(&b).filter(|(x, y)| x != y).count()) as u32
}

// ---- the C ABI

/// # Safety
/// `out` is a writable QueryComparison.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_compare_query(storage: u64, source: u64, base: u64, out: *mut QueryComparison) {
    let out = unsafe { &mut *out };
    *out = QueryComparison::default();
    compare_query(&read, storage, source, base, out);
}
/// # Safety
/// `a` holds `na` ids and `b` `nb` (either may be null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_different_ids(a: *const u64, na: usize, b: *const u64, nb: usize) -> u32 {
    let slice = |p: *const u64, n: usize| if p.is_null() || n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(p, n) } };
    different_ids(slice(a, na), slice(b, nb))
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;
    use std::collections::BTreeMap;

    struct Memory(BTreeMap<u64, u8>);
    impl Memory {
        fn put(&mut self, at: u64, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.0.insert(at + i as u64, b);
            }
        }
        fn reader(&self) -> impl Fn(u64, &mut [u8]) -> bool + '_ {
            |address, out| {
                for (i, o) in out.iter_mut().enumerate() {
                    match self.0.get(&(address + i as u64)) {
                        Some(&b) => *o = b,
                        None => return false,
                    }
                }
                true
            }
        }
    }
    const BASE: u64 = 0x1_4000_0000;
    fn world() -> (Memory, u64, u64) {
        let mut m = Memory(BTreeMap::new());
        let (storage, source, clone, kind) = (0x10_0000u64, 0x20_0000u64, 0x30_0000u64, 0x40_0000u64);
        m.put(storage + 0xDF2F0, &clone.to_le_bytes());
        m.put(source, &kind.to_le_bytes());
        m.put(clone, &kind.to_le_bytes());
        m.put(kind + 0x30, &(BASE + FINE_MATERIALIZE_RVA).to_le_bytes());
        for q in [source, clone] {
            m.put(q + 0x1A0, &[7u8; 128]);
            m.put(q + 0x190, &[1u8; 16]);
        }
        (m, storage, source)
    }
    #[test]
    fn queries() {
        let (m, storage, source) = world();
        let mut q = QueryComparison::default();
        compare_query(&m.reader(), storage, source, BASE, &mut q);
        assert!(q.valid);
        assert_eq!((q.clone_query, q.differences, q.planes[1][5]), (0x30_0000, 0, 7));
        // Differing planes and flags count; the flag bytes 2, 3 and 15 do not.
        let (mut m, storage, source) = world();
        m.put(0x30_0000 + 0x1A0 + 9, &[8, 8]);
        m.put(0x30_0000 + 0x190 + 2, &[9, 9]);
        m.put(0x30_0000 + 0x190 + 15, &[9]);
        m.put(0x30_0000 + 0x190 + 14, &[9]);
        let mut q = QueryComparison::default();
        compare_query(&m.reader(), storage, source, BASE, &mut q);
        assert_eq!((q.valid, q.differences), (true, 3));
        // Another fine-materialize entry, another type or no clone is invalid.
        let (mut m, storage, source) = world();
        m.put(0x40_0000 + 0x30, &0u64.to_le_bytes());
        let mut q = QueryComparison::default();
        compare_query(&m.reader(), storage, source, BASE, &mut q);
        assert!(!q.valid);
        let (mut m, storage, source) = world();
        m.put(0x30_0000, &1u64.to_le_bytes());
        let mut q = QueryComparison::default();
        compare_query(&m.reader(), storage, source, BASE, &mut q);
        assert_eq!((q.valid, q.clone_type), (false, 1));
        let (mut m, storage, source) = world();
        m.put(storage + 0xDF2F0, &0u64.to_le_bytes());
        let mut q = QueryComparison::default();
        compare_query(&m.reader(), storage, source, BASE, &mut q);
        assert_eq!((q.valid, q.source_type), (false, 0));
    }
    #[test]
    fn ids() {
        assert_eq!(different_ids(&[3, 1, 2], &[1, 2, 3]), 0);
        assert_eq!(different_ids(&[3, 1, 2], &[1, 2]), 1);
        assert_eq!(different_ids(&[1, 5], &[1, 2, 3]), 2);
        assert_eq!(different_ids(&[], &[]), 0);
    }
}
