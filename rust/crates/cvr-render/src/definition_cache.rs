//! The native definition tracking's cache results (src/Stereo/NativeDefinitionTracking.cpp, `RecordCacheResultLocked`
//! and `CacheResultCompatible`): a root's pending cache lookup is recorded once, when the published cache is the
//! admission's (not closing) and the returned slot is one of its four 0x90-byte entries holding the admission's use,
//! key and view count, with the miss flag 0 or 1; the record counts the result (hit or miss), keeps the latest lookup's
//! identity and the hook's RVA in the stats, and counts it on the cache's row for that key and view count (the first
//! free one of the 64 rows when new; all taken flags the table full). The hook point itself is the 2.5 code
//! `mov rbx, rax; mov [rsp+68h], rax; lea r15, [rax+10h]` at 0x40A1D1.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ndt_cache_slot` (the checks, before the telemetry write),
//! `cvr_ndt_cache_record` (inside it) and `cvr_ndt_cache_code` (the root admission mirrored; the code guard ahead of
//! the hook point stays the .cpp's); the unit tests below cover each check and each row case.

use crate::instance_layout::read;

/// The .cpp's RootAdmission.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Admission {
    pub id: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub renderer: u64,
    pub prerequisite_group: u64,
    pub cache: u64,
    pub key: u64,
    pub missed: u64,
    pub use_: u32,
    pub views: u32,
    pub cache_pending: bool,
    pub registered: bool,
    pub constructed: bool,
}
const _: () = assert!(core::mem::size_of::<Admission>() == 80);

/// The hook's RVA and its expected 2.5 code.
pub const HOOK_RVA: u64 = 0x40A1D1;
pub const HOOK_CODE: [u8; 12] = [0x48, 0x8B, 0xD8, 0x48, 0x89, 0x44, 0x24, 0x68, 0x4C, 0x8D, 0x78, 0x10];

fn read_value<const N: usize>(address: u64) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    read(address, &mut bytes).then_some(bytes)
}
/// The pending lookup's checks: clears the pending flag and returns the miss flag when the slot is recordable.
pub fn cache_slot(a: &mut Admission, slot: u64, published: u64, closing: bool) -> Option<bool> {
    if !a.cache_pending {
        return None;
    }
    a.cache_pending = false;
    let address = a.cache;
    if published == 0 || published != address || closing || slot < address || (slot - address) % 0x90 != 0 || (slot - address) / 0x90 >= 4 {
        return None;
    }
    let miss = read_value::<1>(a.missed)?[0];
    (miss <= 1
        && read_value::<8>(slot + 8).map(u64::from_le_bytes) == Some(a.key)
        && read_value::<4>(slot + 0x8C).map(u32::from_le_bytes) == Some(a.views)
        && read_value::<4>(slot).map(u32::from_le_bytes) == Some(a.use_))
    .then_some(miss == 1)
}
/// The record of a checked lookup.
pub fn cache_record(a: &Admission, slot: u64, miss: bool, stats: &mut [u64; 16], rows: &mut [[u64; 8]; 64], totals: &mut [u64; 16]) {
    let address = a.cache;
    stats[0] += 1;
    stats[if miss { 2 } else { 1 }] += 1;
    stats[5] = stats[0];
    stats[7] = a.key;
    stats[8] = u64::from(a.views);
    stats[9] = (slot - address) / 0x90;
    stats[10] = u64::from(a.use_);
    stats[11] = a.renderer;
    stats[12] = a.id;
    stats[13] = a.epoch;
    stats[14] = a.pair_generation;
    stats[15] = HOOK_RVA;
    let views = u64::from(a.views);
    let matching = rows.iter().rposition(|row| row[0] == address && row[1] == a.key && row[2] == views);
    let row = match matching {
        Some(at) => Some(at),
        None => rows.iter().position(|row| row[0] == 0).inspect(|&at| {
            let row = &mut rows[at];
            row[..4].copy_from_slice(&[address, a.key, views, stats[0]]);
            row[7] = a.renderer;
            stats[3] += 1;
            totals[15] += 1;
        }),
    };
    match row {
        Some(at) => {
            rows[at][4] = stats[0];
            rows[at][if miss { 6 } else { 5 }] += 1;
        }
        None => stats[4] = 1,
    }
    totals[14] += 1;
}
/// CacheResultCompatible's code check.
pub fn cache_code(base: u64) -> bool {
    base <= u64::MAX - HOOK_RVA && read_value::<12>(base + HOOK_RVA) == Some(HOOK_CODE)
}

// ---- the C ABI

/// # Safety
/// `admission` is the mirrored root admission; `miss` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_cache_slot(admission: *mut Admission, slot: u64, published: u64, closing: bool, miss: *mut bool) -> bool {
    match cache_slot(unsafe { &mut *admission }, slot, published, closing) {
        Some(value) => {
            unsafe { *miss = value };
            true
        }
        None => false,
    }
}
/// # Safety
/// `admission` is the mirrored root admission; the arrays hold 16, 64 x 8 and 16 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_cache_record(
    admission: *const Admission,
    slot: u64,
    miss: bool,
    stats: *mut [u64; 16],
    rows: *mut [[u64; 8]; 64],
    totals: *mut [u64; 16],
) {
    unsafe { cache_record(&*admission, slot, miss, &mut *stats, &mut *rows, &mut *totals) };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_cache_code(base: u64) -> bool {
    cache_code(base)
}

#[cfg(test)]
mod tests {
    //! Each check and each row case.
    use super::*;

    /// A fabricated cache of four 0x90-byte entries, a fifth entry past it (both matching the admission) and a miss
    /// flag after them.
    fn cache(miss: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 5 * 0x90 + 8];
        for entry in [2 * 0x90, 4 * 0x90] {
            bytes[entry..entry + 4].copy_from_slice(&7u32.to_le_bytes());
            bytes[entry + 8..entry + 16].copy_from_slice(&0x1234u64.to_le_bytes());
            bytes[entry + 0x8C..entry + 0x90].copy_from_slice(&2u32.to_le_bytes());
        }
        bytes[5 * 0x90] = miss;
        bytes
    }
    fn admission(memory: &[u8]) -> Admission {
        let base = memory.as_ptr() as u64;
        Admission { id: 1, epoch: 2, pair_generation: 3, renderer: 4, cache: base, key: 0x1234, missed: base + 5 * 0x90, use_: 7, views: 2, cache_pending: true, ..Default::default() }
    }
    #[test]
    fn slots() {
        let memory = cache(1);
        let base = memory.as_ptr() as u64;
        let slot = base + 2 * 0x90;
        let mut a = admission(&memory);
        assert_eq!(cache_slot(&mut a, slot, base, false), Some(true));
        assert!(!a.cache_pending);
        assert_eq!(cache_slot(&mut a, slot, base, false), None);
        let check = |slot: u64, published: u64, closing: bool, change: fn(&mut Admission)| {
            let mut a = admission(&memory);
            change(&mut a);
            let result = cache_slot(&mut a, slot, published, closing);
            assert!(!a.cache_pending);
            result
        };
        let same: fn(&mut Admission) = |_| {};
        assert_eq!(check(slot, base, false, same), Some(true));
        assert_eq!(check(slot, 0, false, same), None);
        assert_eq!(check(slot, base + 1, false, same), None);
        assert_eq!(check(slot, base, true, same), None);
        assert_eq!(check(slot + 1, base, false, same), None);
        assert_eq!(check(base - 0x90, base, false, same), None);
        assert_eq!(check(base + 4 * 0x90, base, false, same), None);
        assert_eq!(check(slot, base, false, |a| a.key = 9), None);
        assert_eq!(check(slot, base, false, |a| a.views = 3), None);
        assert_eq!(check(slot, base, false, |a| a.use_ = 8), None);
        assert_eq!(check(slot, base, false, |a| a.missed = 0), None);
        let hit = cache(0);
        let hit_base = hit.as_ptr() as u64;
        assert_eq!(cache_slot(&mut admission(&hit), hit_base + 2 * 0x90, hit_base, false), Some(false));
        let odd = cache(2);
        let odd_base = odd.as_ptr() as u64;
        assert_eq!(cache_slot(&mut admission(&odd), odd_base + 2 * 0x90, odd_base, false), None);
        let mut idle = admission(&memory);
        idle.cache_pending = false;
        assert_eq!(cache_slot(&mut idle, slot, base, false), None);
    }
    #[test]
    fn records() {
        let a = Admission { id: 1, epoch: 2, pair_generation: 3, renderer: 4, cache: 0x1000, key: 0x55, use_: 7, views: 2, ..Default::default() };
        let (mut stats, mut rows, mut totals) = ([0u64; 16], [[0u64; 8]; 64], [0u64; 16]);
        cache_record(&a, 0x1000 + 0x120, true, &mut stats, &mut rows, &mut totals);
        assert_eq!(stats, [1, 0, 1, 1, 0, 1, 0, 0x55, 2, 2, 7, 4, 1, 2, 3, HOOK_RVA]);
        assert_eq!(rows[0], [0x1000, 0x55, 2, 1, 1, 0, 1, 4]);
        assert_eq!((totals[14], totals[15]), (1, 1));
        cache_record(&a, 0x1000, false, &mut stats, &mut rows, &mut totals);
        assert_eq!((stats[0], stats[1], stats[2], stats[3], stats[9]), (2, 1, 1, 1, 0));
        assert_eq!(rows[0], [0x1000, 0x55, 2, 1, 2, 1, 1, 4]);
        assert_eq!(rows[1], [0; 8]);
        // A second key takes the next free row; a full table flags itself.
        let b = Admission { key: 0x66, ..a };
        cache_record(&b, 0x1000, false, &mut stats, &mut rows, &mut totals);
        assert_eq!(rows[1][..3], [0x1000, 0x66, 2]);
        let mut full = [[1u64; 8]; 64];
        let (mut stats, mut totals) = ([0u64; 16], [0u64; 16]);
        cache_record(&a, 0x1000, false, &mut stats, &mut full, &mut totals);
        assert_eq!((stats[3], stats[4], totals[14], totals[15]), (0, 1, 1, 0));
        // A freed row before the matching one: the match is kept.
        let mut gap = [[0u64; 8]; 64];
        gap[3][..3].copy_from_slice(&[0x1000, 0x55, 2]);
        let mut stats = [0u64; 16];
        cache_record(&a, 0x1000, false, &mut stats, &mut gap, &mut [0; 16]);
        assert_eq!((gap[3][5], gap[0][0], stats[3]), (1, 0, 0));
    }
    #[test]
    fn code() {
        let mut image = vec![0u8; HOOK_RVA as usize + 12];
        image[HOOK_RVA as usize..].copy_from_slice(&HOOK_CODE);
        let base = image.as_ptr() as u64;
        assert!(cache_code(base));
        image[HOOK_RVA as usize + 7] = 0x60;
        assert!(!cache_code(base));
    }
}
