//! The native allocation identities shared by the IA snapshot and the instance observer
//! (src/Render/NativeIaSnapshot.cpp and src/Render/NativeInstanceObserver.cpp): two read pooled allocation records are
//! the same allocation when both were read known and agree in address, vtable, implementation, bytes, resource, heap,
//! block, handle and kind; two allocators when both were read known and agree in address, vtable and implementation (the
//! second's implementation matching its allocation's); and two native buffers when they agree in resource, GPU address,
//! bytes, registry, context, handle and kind.
//!
//! In the Rust build (`CVR_RUST_VRIK`) both files' SameAllocation, the IA snapshot's SameAllocator and SameNative, and the
//! observer's allocator check forward to `cvr_nai_*`; the unit tests below cover each field.

use crate::buffer_read::NativeBuffer;
use crate::source_cbv_details::{Allocation, Allocator, KNOWN};

/// SameAllocation.
pub fn same_allocation(a: &Allocation, b: &Allocation) -> bool {
    a.observation == KNOWN
        && b.observation == KNOWN
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.bytes == b.bytes
        && a.resource == b.resource
        && a.heap == b.heap
        && a.block == b.block
        && a.handle == b.handle
        && a.kind == b.kind
}
/// SameAllocator.
pub fn same_allocator(a: &Allocator, b: &Allocator) -> bool {
    a.observation == KNOWN && b.observation == KNOWN && a.address == b.address && a.vtable == b.vtable && a.pimpl == b.pimpl && b.pimpl_matches
}
/// SameNative.
pub fn same_native(a: &NativeBuffer, b: &NativeBuffer) -> bool {
    a.resource == b.resource && a.gpu == b.gpu && a.bytes == b.bytes && a.registry == b.registry && a.context == b.context && a.handle == b.handle && a.kind == b.kind
}

// ---- the C ABI

/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nai_same_allocation(a: *const Allocation, b: *const Allocation) -> bool {
    same_allocation(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// Both allocators are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nai_same_allocator(a: *const Allocator, b: *const Allocator) -> bool {
    same_allocator(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// Both buffers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nai_same_native(a: *const NativeBuffer, b: *const NativeBuffer) -> bool {
    same_native(unsafe { &*a }, unsafe { &*b })
}

#[cfg(test)]
mod tests {
    //! Each identity field, the observations and the allocator's implementation match.
    use super::*;

    fn allocation() -> Allocation {
        // SAFETY: a plain record, all zero.
        let mut a: Allocation = unsafe { core::mem::zeroed() };
        a.observation = KNOWN;
        a.address = 1;
        a.vtable = 2;
        a.pimpl = 3;
        a.bytes = 4;
        a.resource = 5;
        a.heap = 6;
        a.block = 7;
        a.handle = 8;
        a.kind = 1;
        a
    }

    #[test]
    fn allocations() {
        let a = allocation();
        assert!(same_allocation(&a, &a));
        let mut b = a;
        b.references = 9;
        b.raw[0] = 1;
        assert!(same_allocation(&a, &b));
        for change in [
            |x: &mut Allocation| x.address += 1,
            |x: &mut Allocation| x.vtable += 1,
            |x: &mut Allocation| x.pimpl += 1,
            |x: &mut Allocation| x.bytes += 1,
            |x: &mut Allocation| x.resource += 1,
            |x: &mut Allocation| x.heap += 1,
            |x: &mut Allocation| x.block += 1,
            |x: &mut Allocation| x.handle += 1,
            |x: &mut Allocation| x.kind += 1,
            |x: &mut Allocation| x.observation = 5,
        ] {
            let mut c = a;
            change(&mut c);
            assert!(!same_allocation(&a, &c) && !same_allocation(&c, &a));
        }
    }
    #[test]
    fn allocators_and_buffers() {
        // SAFETY: plain records, all zero.
        let mut a: Allocator = unsafe { core::mem::zeroed() };
        a.observation = KNOWN;
        a.address = 1;
        a.vtable = 2;
        a.pimpl = 3;
        a.pimpl_matches = true;
        assert!(same_allocator(&a, &a));
        let mut b = a;
        b.pimpl_matches = false;
        assert!(!same_allocator(&a, &b));
        assert!(same_allocator(&b, &a));
        b = a;
        b.pimpl = 4;
        assert!(!same_allocator(&a, &b));
        let n = NativeBuffer { resource: 1, gpu: 2, bytes: 3, registry: 4, context: 5, handle: 6, kind: 7 };
        assert!(same_native(&n, &n));
        assert!(!same_native(&n, &NativeBuffer { context: 9, ..n }));
        assert!(!same_native(&n, &NativeBuffer { kind: 1, ..n }));
    }
}
