//! The native definition tracking's certified header (src/Stereo/NativeDefinitionTracking.cpp, `NativeHeaderRead` and
//! `NonemptyCertifiedHeader`): a native definition vector header (data, capacity, count: 16 bytes) is read twice alike;
//! a non-empty certified one holds 1..capacity roots in at most 8192 slots, ends with the definition allocator
//! (image+0x30947E8) after its capacity, whose free callback (+0x20) is image+0x120F6F0, and has a non-null root; the
//! roots then go to the .cpp's tree capture.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ndt_header_read` and `cvr_ndt_header_roots`; the unit tests
//! below read fabricated headers.

use crate::instance_layout::{read, read_u64};

pub const ALLOCATOR_RVA: u64 = 0x30947E8;
pub const CALLBACK_RVA: u64 = 0x120F6F0;
pub const MAX_ROOTS: u32 = 8192;

/// retirement_deferral::NativeHeader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub data: u64,
    pub capacity: u32,
    pub count: u32,
}
const _: () = assert!(core::mem::size_of::<Header>() == 16);

fn header_once(address: u64) -> Option<Header> {
    let mut bytes = [0u8; 16];
    read(address, &mut bytes).then(|| Header {
        data: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        capacity: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        count: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
    })
}
/// NativeHeaderRead.
pub fn header_read(address: u64) -> Option<Header> {
    let first = header_once(address)?;
    (header_once(address)? == first).then_some(first)
}
/// NonemptyCertifiedHeader's header and roots.
pub fn header_roots(base: u64, address: u64) -> Option<Vec<u64>> {
    let h = header_read(address)?;
    let capacity = u64::from(h.capacity);
    if h.capacity == 0 || h.count == 0 || h.count > h.capacity || h.capacity > MAX_ROOTS || h.data > u64::MAX - capacity * 8 - 8 {
        return None;
    }
    let allocator = read_u64(h.data + capacity * 8)?;
    if allocator != base.wrapping_add(ALLOCATOR_RVA) || read_u64(allocator.wrapping_add(0x20))? != base.wrapping_add(CALLBACK_RVA) {
        return None;
    }
    let mut bytes = vec![0u8; h.count as usize * 8];
    if !read(h.data, &mut bytes) {
        return None;
    }
    let roots: Vec<u64> = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    roots.iter().any(|&r| r != 0).then_some(roots)
}

// ---- the C ABI

/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_header_read(address: u64, out: *mut Header) -> bool {
    match header_read(address) {
        Some(h) => {
            unsafe { *out = h };
            true
        }
        None => false,
    }
}
/// # Safety
/// `roots` holds `capacity` slots and `count` is writable; the roots are written when they fit.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_header_roots(base: u64, address: u64, roots: *mut u64, capacity: usize, count: *mut usize) -> bool {
    match header_roots(base, address) {
        Some(found) if found.len() <= capacity => {
            unsafe {
                core::ptr::copy_nonoverlapping(found.as_ptr(), roots, found.len());
                *count = found.len();
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! Fabricated headers.
    use super::*;

    #[repr(C, align(16))]
    struct Memory([u8; 0x200]);
    fn put(m: &mut Memory, at: usize, value: u64) {
        m.0[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    /// A header at 0, its data at 0x40 (capacity 4, count 2), the allocator after it pointing at 0x100 whose callback
    /// slot (+0x20) holds the callback; `base` is chosen so the allocator and callback are image+RVA.
    fn image(roots: [u64; 2]) -> (Box<Memory>, u64, u64) {
        let mut m = Box::new(Memory([0; 0x200]));
        let at = m.0.as_ptr() as u64;
        let base = (at + 0x100).wrapping_sub(ALLOCATOR_RVA);
        put(&mut m, 0, at + 0x40);
        m.0[8..12].copy_from_slice(&4u32.to_le_bytes());
        m.0[12..16].copy_from_slice(&2u32.to_le_bytes());
        put(&mut m, 0x40, roots[0]);
        put(&mut m, 0x48, roots[1]);
        put(&mut m, 0x40 + 4 * 8, at + 0x100);
        put(&mut m, 0x120, base.wrapping_add(CALLBACK_RVA));
        (m, base, at)
    }
    #[test]
    fn headers() {
        let (m, base, at) = image([0, 7]);
        assert_eq!(header_read(at), Some(Header { data: at + 0x40, capacity: 4, count: 2 }));
        assert_eq!(header_roots(base, at), Some(vec![0, 7]));
        assert_eq!(header_roots(base.wrapping_add(8), at), None);
        drop(m);
        let (_m, base, at) = image([0, 0]);
        assert_eq!(header_roots(base, at), None);
        assert_eq!(header_read(0), None);
    }
    #[test]
    fn shapes() {
        for (capacity, count) in [(0u32, 0u32), (4, 0), (4, 5), (8193, 1)] {
            let (mut m, base, at) = image([1, 2]);
            m.0[8..12].copy_from_slice(&capacity.to_le_bytes());
            m.0[12..16].copy_from_slice(&count.to_le_bytes());
            assert_eq!(header_roots(base, at), None, "{capacity} {count}");
        }
        // The callback must be the definition allocator's.
        let (mut m, base, at) = image([1, 2]);
        put(&mut m, 0x120, 0);
        assert_eq!(header_roots(base, at), None);
    }
}
