//! The native IA snapshot's geometry pool re-check (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::PoolCurrent`): a
//! source captured from a geometry pool is still that pool's -- its pinned allocation is the pool's owner and the pool
//! read again (through the .cpp's reader, from the saved record and view) equals the captured one.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the comparison forwards to `cvr_iap_pool_current` (the pool flag and the code's
//! compatibility stay the .cpp's); the unit test below covers the owner and an unreadable pool.

use core::ffi::c_void;

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::{read_geometry_pool, GeometryPool};
use crate::source_cbv_details::Reader;

/// PoolCurrent's comparison.
#[allow(clippy::too_many_arguments)]
pub fn pool_current(
    read: Reader,
    user: *mut c_void,
    base: u64,
    pinned_allocation: u64,
    saved: &GeometryPool,
    native: &NativeBuffer,
    raw: &[u8; 0xB0],
    gpu: u64,
    bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
) -> bool {
    pinned_allocation != 0
        && pinned_allocation == saved.owner
        && read_geometry_pool(read, user, base, native, raw, gpu, bytes, view_gpu, view_bytes, 5).is_some_and(|current| current == *saved)
}

// ---- the C ABI

/// # Safety
/// `read` reads this process's memory for `user`; the other pointers are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iap_pool_current(
    read: Reader,
    user: *mut c_void,
    base: u64,
    pinned_allocation: u64,
    saved: *const GeometryPool,
    native: *const NativeBuffer,
    raw: *const [u8; 0xB0],
    gpu: u64,
    bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
) -> bool {
    unsafe { pool_current(read, user, base, pinned_allocation, &*saved, &*native, &*raw, gpu, bytes, view_gpu, view_bytes) }
}

#[cfg(test)]
mod tests {
    //! No pin, a pin that is not the owner, and an unreadable pool.
    use super::*;

    unsafe extern "C" fn unreadable(_: *mut c_void, _: u64, _: *mut c_void, _: usize) -> bool {
        false
    }

    #[test]
    fn pool_owner_and_read() {
        let saved = GeometryPool { owner: 0x77, ..Default::default() };
        let native = NativeBuffer::default();
        let raw = [0u8; 0xB0];
        assert!(!pool_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 0, &saved, &native, &raw, 0, 0, 0, 0));
        assert!(!pool_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 0x78, &saved, &native, &raw, 0, 0, 0, 0));
        assert!(!pool_current(Some(unreadable), core::ptr::null_mut(), 0x1000, 0x77, &saved, &native, &raw, 0, 0, 0, 0));
    }
}
