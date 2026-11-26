//! The native IA snapshot's creation and lease identities (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::SameCreation`
//! and `SameLease`): two API creation receipts alike (kind, resource, placement heap, heap offset, allocation bytes, initial
//! state and whether it is known) and two buffer leases alike (resource, GPU address, bytes, heap type, flags and the same
//! native buffer).
//!
//! In the Rust build (`CVR_RUST_VRIK`) both forward to `cvr_ias_same_creation` and `cvr_ias_same_lease` (the C++ shapes
//! mirrored: a creation receipt of 48 bytes, a lease of 80); the unit tests below cover each field.

use core::ffi::c_void;

use crate::allocation_identity::same_native;
use crate::buffer_read::NativeBuffer;

/// constants::ResourceCreationInfo (its COM pointers as addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CreationInfo {
    pub kind: u32,
    pub initial_state_known: bool,
    pub initial_state: u32,
    pub resource: *mut c_void,
    pub heap: *mut c_void,
    pub heap_offset: u64,
    pub allocation_bytes: u64,
}
const _: () = assert!(core::mem::size_of::<CreationInfo>() == 48 && core::mem::offset_of!(CreationInfo, resource) == 16);

/// geometry::BufferLease (its resource as an address).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Lease {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
    pub native: NativeBuffer,
}
const _: () = assert!(core::mem::size_of::<Lease>() == 80 && core::mem::offset_of!(Lease, native) == 32);

/// SameCreation.
pub fn same_creation(a: &CreationInfo, b: &CreationInfo) -> bool {
    a.kind == b.kind
        && a.resource == b.resource
        && a.heap == b.heap
        && a.heap_offset == b.heap_offset
        && a.allocation_bytes == b.allocation_bytes
        && a.initial_state_known == b.initial_state_known
        && a.initial_state == b.initial_state
}
/// SameLease.
pub fn same_lease(a: &Lease, b: &Lease) -> bool {
    a.resource == b.resource && a.gpu == b.gpu && a.bytes == b.bytes && a.heap_type == b.heap_type && a.flags == b.flags && same_native(&a.native, &b.native)
}

// ---- the C ABI

/// # Safety
/// Both receipts are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_same_creation(a: *const CreationInfo, b: *const CreationInfo) -> bool {
    same_creation(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// Both leases are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_same_lease(a: *const Lease, b: *const Lease) -> bool {
    same_lease(unsafe { &*a }, unsafe { &*b })
}

#[cfg(test)]
mod tests {
    //! Each creation and lease field.
    use super::*;

    #[test]
    fn creations() {
        let a = CreationInfo { kind: 2, initial_state_known: true, initial_state: 0xAC3, resource: 1 as *mut c_void, heap: 2 as *mut c_void, heap_offset: 0x10000, allocation_bytes: 0x2000 };
        assert!(same_creation(&a, &a));
        assert!(!same_creation(&a, &CreationInfo { kind: 1, ..a }));
        assert!(!same_creation(&a, &CreationInfo { heap: 3 as *mut c_void, ..a }));
        assert!(!same_creation(&a, &CreationInfo { heap_offset: 0, ..a }));
        assert!(!same_creation(&a, &CreationInfo { initial_state_known: false, ..a }));
        assert!(!same_creation(&a, &CreationInfo { initial_state: 0, ..a }));
        assert!(!same_creation(&a, &CreationInfo { allocation_bytes: 1, ..a }));
    }
    #[test]
    fn leases() {
        let native = NativeBuffer { resource: 1, gpu: 2, bytes: 3, registry: 4, context: 5, handle: 6, kind: 1 };
        let a = Lease { resource: 1 as *mut c_void, gpu: 0x100, bytes: 64, heap_type: 2, flags: 0, native };
        assert!(same_lease(&a, &a));
        assert!(!same_lease(&a, &Lease { flags: 1, ..a }));
        assert!(!same_lease(&a, &Lease { heap_type: 1, ..a }));
        assert!(!same_lease(&a, &Lease { native: NativeBuffer { handle: 7, ..native }, ..a }));
    }
}
