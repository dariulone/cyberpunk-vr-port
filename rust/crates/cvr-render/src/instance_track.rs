//! The native instance observer's backing identities (src/Render/NativeInstanceObserver.cpp,
//! `instance_observer::Allocation` and `SameBacking`): a ring's pooled allocation still the captured one (both pins held;
//! the allocation record read again through the .cpp's reader is the same allocation; its allocator known, the pinned
//! one, with the captured vtable and implementation and a matching implementation), and two captures of one backing (the
//! same ring observation but for its generation and cursors, the same resource cookie and record bytes).
//!
//! In the Rust build (`CVR_RUST_VRIK`) both forward to `cvr_nit_*`; the unit tests below cover them.

use core::ffi::c_void;

use crate::allocation_identity::same_allocation;
use crate::native::instance_ring::RingObservation;
use crate::source_cbv_details::{read_allocation, read_allocator, reader, Allocation, Allocator, Reader, KNOWN};

/// Allocation.
#[allow(clippy::too_many_arguments)]
pub fn allocation_current(read: Reader, user: *mut c_void, base: u64, allocation: u64, allocator: u64, registry: u64, saved_allocation: &Allocation, saved_allocator: &Allocator) -> bool {
    let mem = reader(read, user);
    let current = read_allocation(&mem, base, allocation);
    let current_allocator = read_allocator(&mem, base, registry, &current);
    allocation != 0
        && allocator != 0
        && same_allocation(saved_allocation, &current)
        && current_allocator.observation == KNOWN
        && current_allocator.address == allocator
        && current_allocator.vtable == saved_allocator.vtable
        && current_allocator.pimpl == saved_allocator.pimpl
        && current_allocator.pimpl_matches
}
/// SameBacking.
pub fn same_backing(a: &RingObservation, b: &RingObservation, a_cookie: u64, b_cookie: u64, a_raw: &[u8; 0xB0], b_raw: &[u8; 0xB0]) -> bool {
    let strip = |o: &RingObservation| RingObservation { generation: 0, head: 0, tail: 0, ..*o };
    strip(a) == strip(b) && a_cookie == b_cookie && a_raw == b_raw
}

// ---- the C ABI

/// # Safety
/// `read` reads this process's memory for `user`; the records are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nit_allocation_current(
    read: Reader,
    user: *mut c_void,
    base: u64,
    allocation: u64,
    allocator: u64,
    registry: u64,
    saved_allocation: *const Allocation,
    saved_allocator: *const Allocator,
) -> bool {
    unsafe { allocation_current(read, user, base, allocation, allocator, registry, &*saved_allocation, &*saved_allocator) }
}
/// # Safety
/// All pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nit_same_backing(a: *const RingObservation, b: *const RingObservation, a_cookie: u64, b_cookie: u64, a_raw: *const [u8; 0xB0], b_raw: *const [u8; 0xB0]) -> bool {
    unsafe { same_backing(&*a, &*b, a_cookie, b_cookie, &*a_raw, &*b_raw) }
}

#[cfg(test)]
mod tests {
    //! The ignored and compared observation fields, the cookie and the record; missing pins and an unreadable allocation.
    use super::*;

    unsafe extern "C" fn unreadable(_: *mut c_void, _: u64, _: *mut c_void, _: usize) -> bool {
        false
    }

    #[test]
    fn backings() {
        let a = RingObservation { ring: 1, generation: 2, head: 3, tail: 4, handle: 5, ..Default::default() };
        let raw = [7u8; 0xB0];
        assert!(same_backing(&a, &RingObservation { generation: 9, head: 9, tail: 9, ..a }, 1, 1, &raw, &raw));
        assert!(!same_backing(&a, &RingObservation { handle: 6, ..a }, 1, 1, &raw, &raw));
        assert!(!same_backing(&a, &a, 1, 2, &raw, &raw));
        let mut other = raw;
        other[0] = 0;
        assert!(!same_backing(&a, &a, 1, 1, &raw, &other));
    }
    #[test]
    fn allocations() {
        // SAFETY: plain records, all zero.
        let (saved, allocator): (Allocation, Allocator) = unsafe { (core::mem::zeroed(), core::mem::zeroed()) };
        assert!(!allocation_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 0, 1, 2, &saved, &allocator));
        assert!(!allocation_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 1, 0, 2, &saved, &allocator));
        assert!(!allocation_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 1, 1, 2, &saved, &allocator));
    }
}
