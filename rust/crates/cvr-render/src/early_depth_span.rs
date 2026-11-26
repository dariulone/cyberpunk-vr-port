//! The native early depth's game reads (src/Render/NativeEarlyDepth.cpp, `PrepareMembership`'s span and peer storage,
//! `Begin`'s stage): the render span's packet range (16-byte packets, 8-aligned, at most 32768, not running backwards)
//! and the paired view's peer storage (at the view's +0x2C40, room for its 0x35A0 bytes); and the node arguments' stage
//! (+16) being the third.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ned_span` and `cvr_ned_stage3` (reads through
//! ReadProcessMemory, as the .cpp's `Read`); the unit tests below read fabricated memory.

use crate::instance_layout::read;

/// packets::MaxSpanPackets.
pub const MAX_SPAN_PACKETS: u64 = 32768;

fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
/// PrepareMembership's span and peer storage: ([begin, end], peer storage).
pub fn span(read: &impl Fn(u64, &mut [u8]) -> bool, span: u64, view: u64) -> Option<([u64; 2], u64)> {
    let mut raw = [0u8; 16];
    if !read(span, &mut raw) {
        return None;
    }
    let range = [u64::from_le_bytes(raw[..8].try_into().unwrap()), u64::from_le_bytes(raw[8..].try_into().unwrap())];
    if range[1] < range[0] || (range[1] - range[0]) % 16 != 0 || range[0] & 7 != 0 || (range[1] - range[0]) / 16 > MAX_SPAN_PACKETS || view == 0 || view > u64::MAX - 0x2C48 {
        return None;
    }
    let peer = u64_at(read, view + 0x2C40).filter(|&p| p != 0 && p <= u64::MAX - 0x35A0)?;
    Some((range, peer))
}
/// Begin's stage check.
pub fn stage3(read: &impl Fn(u64, &mut [u8]) -> bool, arguments: u64) -> bool {
    let mut b = [0u8; 4];
    read(arguments.wrapping_add(16), &mut b) && u32::from_le_bytes(b) == 3
}

// ---- the C ABI

/// # Safety
/// `range` holds two values and `peer` one; both are written only on success.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_span(span_address: u64, view: u64, range: *mut [u64; 2], peer: *mut u64) -> bool {
    match span(&read, span_address, view) {
        Some((r, p)) => {
            unsafe { (*range, *peer) = (r, p) };
            true
        }
        None => false,
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_stage3(arguments: u64) -> bool {
    stage3(&read, arguments)
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;
    use std::collections::BTreeMap;

    fn memory(words: &[(u64, u64)]) -> impl Fn(u64, &mut [u8]) -> bool {
        let mut bytes = BTreeMap::new();
        for &(at, value) in words {
            for (i, b) in value.to_le_bytes().iter().enumerate() {
                bytes.insert(at + i as u64, *b);
            }
        }
        move |address, out: &mut [u8]| {
            for (i, o) in out.iter_mut().enumerate() {
                match bytes.get(&(address + i as u64)) {
                    Some(&b) => *o = b,
                    None => return false,
                }
            }
            true
        }
    }
    #[test]
    fn spans() {
        let (s, view) = (0x1000u64, 0x8000u64);
        let ok = memory(&[(s, 0x2000), (s + 8, 0x2000 + 16 * 5), (view + 0x2C40, 0x9000)]);
        assert_eq!(span(&ok, s, view), Some(([0x2000, 0x2050], 0x9000)));
        // An empty span is fine.
        assert_eq!(span(&memory(&[(s, 0x2000), (s + 8, 0x2000), (view + 0x2C40, 0x9000)]), s, view), Some(([0x2000, 0x2000], 0x9000)));
        for (begin, end) in [(0x2000, 0x1FF0), (0x2000, 0x2008), (0x2004, 0x2014), (0x2000, 0x2000 + 16 * 32769)] {
            assert_eq!(span(&memory(&[(s, begin), (s + 8, end), (view + 0x2C40, 0x9000)]), s, view), None, "{begin:#x}..{end:#x}");
        }
        assert_eq!(span(&memory(&[(s, 0x2000), (s + 8, 0x2000 + 16 * 32768), (view + 0x2C40, 0x9000)]), s, view).map(|r| r.1), Some(0x9000));
        // No view, no peer storage, or one without room.
        assert_eq!(span(&ok, s, 0), None);
        assert_eq!(span(&memory(&[(s, 0x2000), (s + 8, 0x2000), (view + 0x2C40, 0)]), s, view), None);
        assert_eq!(span(&memory(&[(s, 0x2000), (s + 8, 0x2000), (view + 0x2C40, u64::MAX - 0x359F)]), s, view), None);
        assert_eq!(span(&ok, s + 1, view), None);
    }
    #[test]
    fn stages() {
        assert!(stage3(&memory(&[(0x1010, 3)]), 0x1000));
        assert!(!stage3(&memory(&[(0x1010, 2)]), 0x1000));
        assert!(!stage3(&memory(&[]), 0x1000));
    }
}
