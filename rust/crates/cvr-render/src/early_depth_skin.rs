//! The native early depth's skinned instance window (src/Render/NativeEarlyDepth.cpp, `early_depth::Draw`): a skinned
//! draw's 64-byte instance rows (from its first instance) read through the list's instance lease -- an upload (or GPU
//! upload) lease of the draw's own instance resource, the rows inside both the bound stream and the lease -- and their
//! offset into the lease.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the window forwards to `cvr_ned_skin_window`; the unit test below covers each
//! bound.

use crate::geometry_commands::DrawRecord;

const UPLOAD: u32 = 2;
const GPU_UPLOAD: u32 = 5;

/// The instance lease as Draw sees it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lease {
    pub resource: u64,
    pub heap_type: u32,
    pub gpu: u64,
    pub bytes: u64,
}

/// The rows' offset into the lease, when the window holds.
pub fn skin_window(d: &DrawRecord, lease: &Lease, instance_bytes: u64) -> Option<u64> {
    let stream = &d.vertices[7];
    let relative = d.first_instance as u64 * 64;
    let size = stream.SizeInBytes as u64;
    if lease.resource == 0
        || (lease.heap_type != UPLOAD && lease.heap_type != GPU_UPLOAD)
        || d.instance_resource != lease.resource
        || stream.BufferLocation < lease.gpu
        || stream.BufferLocation > u64::MAX - relative
        || relative > size
        || instance_bytes > size - relative
    {
        return None;
    }
    let offset = stream.BufferLocation + relative - lease.gpu;
    (offset <= lease.bytes && instance_bytes <= lease.bytes - offset).then_some(offset)
}

// ---- the C ABI

/// # Safety
/// `d` is valid; `offset` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_skin_window(d: *const DrawRecord, resource: u64, heap_type: u32, gpu: u64, bytes: u64, instance_bytes: u64, offset: *mut u64) -> bool {
    match skin_window(unsafe { &*d }, &Lease { resource, heap_type, gpu, bytes }, instance_bytes) {
        Some(at) => {
            unsafe { *offset = at };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! The window's lease, stream and lease-extent bounds.
    use super::*;

    #[test]
    fn skin_windows() {
        // SAFETY: a plain record, all zero.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.instance_resource = 0x77;
        d.vertices[7].BufferLocation = 0x10_0100;
        d.vertices[7].SizeInBytes = 64 * 8;
        d.first_instance = 2;
        let lease = Lease { resource: 0x77, heap_type: UPLOAD, gpu: 0x10_0000, bytes: 0x1000 };
        assert_eq!(skin_window(&d, &lease, 64 * 6), Some(0x100 + 128));
        assert_eq!(skin_window(&d, &lease, 64 * 7), None);
        assert_eq!(skin_window(&d, &Lease { heap_type: GPU_UPLOAD, ..lease }, 64), Some(0x180));
        assert_eq!(skin_window(&d, &Lease { heap_type: 1, ..lease }, 64), None);
        assert_eq!(skin_window(&d, &Lease { resource: 0x78, ..lease }, 64), None);
        assert_eq!(skin_window(&d, &Lease { resource: 0, ..lease }, 64), None);
        assert_eq!(skin_window(&d, &Lease { gpu: 0x10_0200, ..lease }, 64), None);
        assert_eq!(skin_window(&d, &Lease { bytes: 0x1C0, ..lease }, 64), Some(0x180));
        assert_eq!(skin_window(&d, &Lease { bytes: 0x1BF, ..lease }, 64), None);
        d.first_instance = 9;
        assert_eq!(skin_window(&d, &lease, 0), None);
    }
}
