//! The stage replay's layout groups (src/Render/StageReplay.cpp, `ReadLayoutGroups` and `RecordAnyTable`): a root
//! layout's camera groups are the descriptor groups visible to the VS, PS or GS (the three masks at layout+0x18) whose
//! entry (layout+0x38, 16 bytes each) holds 1..256 descriptors starting at most at 8192 and ending inside the 8192-entry
//! table, of a kind other than 2 on a root parameter below 64 -- at most 24, in group order (none when the layout
//! cannot be read); and the census copies the CPU-table contents behind a root table bind: the first visible group on
//! the bound parameter with 1..256 descriptors starting at most at 8192, as [count | group << 32, descriptors...].
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_srl_groups` and `cvr_srl_any_table` (guarded reads in
//! place of the .cpp's SEH); the unit tests below read fabricated layouts.

use crate::instance_layout::read;

fn value<const N: usize>(address: u64) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    read(address, &mut bytes).then_some(bytes)
}
fn u64_at(address: u64) -> Option<u64> {
    value::<8>(address).map(u64::from_le_bytes)
}
/// One group entry: (start, count, kind, parameter).
fn entry(layout: u64, group: u32) -> Option<(u32, u32, u8, u8)> {
    let at = layout.checked_add(0x38 + u64::from(group) * 16)?;
    let bytes = value::<16>(at)?;
    Some((
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        u32::from(u16::from_le_bytes(bytes[10..12].try_into().unwrap())),
        bytes[13],
        bytes[14],
    ))
}
fn visible(layout: u64) -> Option<u64> {
    let at = layout.checked_add(0x18)?;
    Some(u64_at(at)? | u64_at(at + 8)? | u64_at(at + 16)?)
}

/// ReadLayoutGroups: (parameter, start, count) rows and their count, or None when the layout cannot be read.
pub fn groups(layout: u64, out: &mut [[u32; 3]; 24]) -> Option<u32> {
    let mask = visible(layout)?;
    let mut count = 0usize;
    for group in 0..64u32 {
        if count >= out.len() {
            break;
        }
        if mask & (1u64 << group) == 0 {
            continue;
        }
        let (start, n, kind, parameter) = entry(layout, group)?;
        if n == 0 || n > 256 || kind == 2 || parameter >= 64 || start > 8192 || n > 8192 - start {
            continue;
        }
        out[count] = [u32::from(parameter), start, n];
        count += 1;
    }
    Some(count as u32)
}
/// RecordAnyTable: the payload's bytes (0 when no group or the reads fail).
pub fn any_table(state: u64, parameter: u64, buffer: &mut [u64; 257]) -> usize {
    let inner = |buffer: &mut [u64; 257]| -> Option<usize> {
        let layout = u64_at(state.checked_add(0x68)?)?;
        let cpu = u64_at(state.checked_add(0x28)?)?;
        if layout == 0 || cpu == 0 || layout > u64::MAX - 0x473 {
            return Some(0);
        }
        let mask = visible(layout)?;
        for group in 0..64u32 {
            if mask & (1u64 << group) == 0 {
                continue;
            }
            let (start, n, _, p) = entry(layout, group)?;
            if u64::from(p) != parameter || n == 0 || n > 256 || start > 8192 {
                continue;
            }
            buffer[0] = u64::from(n) | (u64::from(group) << 32);
            let mut bytes = vec![0u8; n as usize * 8];
            if !read(cpu.checked_add(u64::from(start) * 8)?, &mut bytes) {
                return None;
            }
            for (slot, chunk) in buffer[1..=n as usize].iter_mut().zip(bytes.chunks_exact(8)) {
                *slot = u64::from_le_bytes(chunk.try_into().unwrap());
            }
            return Some((n as usize + 1) * 8);
        }
        Some(0)
    };
    inner(buffer).unwrap_or(0)
}

// ---- the C ABI

/// # Safety
/// `out` holds 24 groups of three words; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srl_groups(layout: u64, out: *mut [[u32; 3]; 24], count: *mut u32) -> bool {
    let found = groups(layout, unsafe { &mut *out });
    unsafe { *count = found.unwrap_or(0) };
    found.is_some_and(|n| n != 0)
}
/// # Safety
/// `buffer` holds 257 words.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srl_any_table(state: u64, parameter: u64, buffer: *mut [u64; 257]) -> usize {
    any_table(state, parameter, unsafe { &mut *buffer })
}

#[cfg(test)]
mod tests {
    //! Fabricated layouts.
    use super::*;

    #[repr(C, align(16))]
    struct Memory([u8; 0x3000]);
    fn put(m: &mut Memory, at: usize, bytes: &[u8]) {
        m.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn group(m: &mut Memory, layout: usize, group: usize, start: u32, n: u16, kind: u8, parameter: u8) {
        let at = layout + 0x38 + group * 16;
        put(m, at + 4, &start.to_le_bytes());
        put(m, at + 10, &n.to_le_bytes());
        m.0[at + 13] = kind;
        m.0[at + 14] = parameter;
    }
    /// A layout at 0x100 with groups 1 (VS), 3 (PS) and 5 (GS) visible, 7 not.
    fn layout() -> (Box<Memory>, u64) {
        let mut m = Box::new(Memory([0; 0x3000]));
        put(&mut m, 0x100 + 0x18, &(1u64 << 1).to_le_bytes());
        put(&mut m, 0x100 + 0x20, &(1u64 << 3).to_le_bytes());
        put(&mut m, 0x100 + 0x28, &(1u64 << 5).to_le_bytes());
        group(&mut m, 0x100, 1, 4, 3, 0, 9);
        group(&mut m, 0x100, 3, 8000, 192, 1, 10);
        group(&mut m, 0x100, 5, 8000, 193, 1, 11);
        group(&mut m, 0x100, 7, 0, 1, 0, 12);
        let base = m.0.as_ptr() as u64;
        (m, base)
    }
    #[test]
    fn layouts() {
        let (mut m, base) = layout();
        let mut out = [[0u32; 3]; 24];
        assert_eq!(groups(base + 0x100, &mut out), Some(2));
        assert_eq!(out[..2], [[9, 4, 3], [10, 8000, 192]]);
        for (kind, parameter, n) in [(2u8, 9u8, 3u16), (0, 64, 3), (0, 9, 0), (0, 9, 257)] {
            group(&mut m, 0x100, 1, 4, n, kind, parameter);
            assert_eq!(groups(base + 0x100, &mut out), Some(1), "{kind} {parameter} {n}");
        }
        group(&mut m, 0x100, 1, 8193, 1, 0, 9);
        assert_eq!(groups(base + 0x100, &mut out), Some(1));
        // Every group visible: the first 24 kept.
        put(&mut m, 0x100 + 0x18, &u64::MAX.to_le_bytes());
        for g in 0..64 {
            group(&mut m, 0x100, g, 0, 1, 0, g as u8 % 64);
        }
        assert_eq!(groups(base + 0x100, &mut out), Some(24));
        assert_eq!(out[23], [23, 0, 1]);
        assert_eq!(groups(0, &mut out), None);
    }
    #[test]
    fn tables() {
        let (mut m, base) = layout();
        put(&mut m, 0x68, &(base + 0x100).to_le_bytes());
        put(&mut m, 0x28, &(base + 0x1000).to_le_bytes());
        for i in 0..3u64 {
            put(&mut m, (0x1000 + (4 + i) * 8) as usize, &(0xA0 + i).to_le_bytes());
        }
        let mut buffer = [0u64; 257];
        assert_eq!(any_table(base, 9, &mut buffer), 32);
        assert_eq!(buffer[..4], [3 | (1 << 32), 0xA0, 0xA1, 0xA2]);
        assert_eq!(any_table(base, 12, &mut buffer), 0); // group 7 is not visible
        assert_eq!(any_table(base, 13, &mut buffer), 0);
        put(&mut m, 0x28, &0u64.to_le_bytes());
        assert_eq!(any_table(base, 9, &mut buffer), 0);
        assert_eq!(any_table(0, 9, &mut buffer), 0);
    }
}
