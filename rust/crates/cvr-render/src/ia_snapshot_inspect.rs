//! The native IA snapshot's allocation inspection (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Inspect`): the read
//! allocation and allocator recorded in a source's observation (their observations, kind, resource, heap, bytes and
//! implementations; the allocation known, the allocator known with a matching implementation, the full backing when the
//! native buffer is the whole resource), and whether the source is a placed candidate (every required flag, an index or
//! vertex buffer kind 0, 1 or 3, a placed creation with its heap, a known kind-1 allocation and a known matching
//! allocator, no alias or pool owner, full backing, a default heap).
//!
//! In the Rust build (`CVR_RUST_VRIK`) those two steps forward to `cvr_iai_inspect_allocation` and
//! `cvr_iai_placement_candidate` (the placement's code compatibility stays the .cpp's); the unit tests below cover them.

use windows::Win32::Graphics::Direct3D12::D3D12_HEAP_TYPE_DEFAULT;

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot_shape::Observation;
use crate::source_cbv_details::{Allocation, Allocator, KNOWN};

const RECORD_KNOWN: u64 = 1;
const CREATION_KNOWN: u64 = 2;
const ALLOCATION_KNOWN: u64 = 4;
const ALLOCATOR_KNOWN: u64 = 8;
const SAME_DEVICE: u64 = 16;
const SCOPE_CURRENT: u64 = 32;
const BINDINGS_CURRENT: u64 = 64;
const PARENT_CURRENT: u64 = 128;
const CONTEXT_KNOWN: u64 = 256;
const CODE_KNOWN: u64 = 512;
const FULL_BACKING: u64 = 1024;
/// detail::RequiredFlags.
pub const REQUIRED: u64 = RECORD_KNOWN | CREATION_KNOWN | ALLOCATION_KNOWN | ALLOCATOR_KNOWN | SAME_DEVICE | SCOPE_CURRENT | BINDINGS_CURRENT | PARENT_CURRENT | CONTEXT_KNOWN | CODE_KNOWN | FULL_BACKING;

/// Inspect's allocation step.
pub fn inspect_allocation(a: &mut Observation, allocation: &Allocation, allocator: &Allocator, native: &NativeBuffer) {
    a.allocation_observation = allocation.observation as u64;
    a.allocator_observation = allocator.observation as u64;
    a.allocation_kind = allocation.kind as u64;
    a.allocation_resource = allocation.resource;
    a.allocation_heap = allocation.heap;
    a.allocation_bytes = allocation.bytes;
    a.allocation_pimpl = allocation.pimpl;
    a.allocator = allocator.address;
    a.allocator_pimpl = allocator.pimpl;
    if allocation.observation == KNOWN {
        a.flags |= ALLOCATION_KNOWN;
    }
    if allocator.observation == KNOWN && allocator.pimpl_matches {
        a.flags |= ALLOCATOR_KNOWN;
    }
    if native.gpu == a.gpu && native.bytes == a.width {
        a.flags |= FULL_BACKING;
    }
}
/// Inspect's placed candidate.
pub fn placement_candidate(a: &Observation, native_kind: u32, creation_kind: u32, creation_heap: bool, allocation: &Allocation, allocator: &Allocator) -> bool {
    a.flags & REQUIRED == REQUIRED
        && matches!(native_kind, 0 | 1 | 3)
        && creation_kind == 2
        && creation_heap
        && allocation.observation == KNOWN
        && allocation.kind == 1
        && allocator.observation == KNOWN
        && allocator.pimpl_matches
        && a.alias_payload == 0
        && a.alias_control == 0
        && a.pool_owner == 0
        && a.flags & FULL_BACKING != 0
        && a.heap == D3D12_HEAP_TYPE_DEFAULT.0 as u64
}

// ---- the C ABI

/// # Safety
/// All pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_inspect_allocation(a: *mut Observation, allocation: *const Allocation, allocator: *const Allocator, native: *const NativeBuffer) {
    inspect_allocation(unsafe { &mut *a }, unsafe { &*allocation }, unsafe { &*allocator }, unsafe { &*native });
}
/// # Safety
/// All pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_placement_candidate(a: *const Observation, native_kind: u32, creation_kind: u32, creation_heap: bool, allocation: *const Allocation, allocator: *const Allocator) -> bool {
    placement_candidate(unsafe { &*a }, native_kind, creation_kind, creation_heap, unsafe { &*allocation }, unsafe { &*allocator })
}

#[cfg(test)]
mod tests {
    //! The recorded fields, the three flags and each placed-candidate condition.
    use super::*;

    fn parts() -> (Allocation, Allocator) {
        // SAFETY: plain records, all zero.
        let (mut allocation, mut allocator): (Allocation, Allocator) = unsafe { (core::mem::zeroed(), core::mem::zeroed()) };
        allocation.observation = KNOWN;
        allocation.kind = 1;
        allocation.resource = 5;
        allocation.heap = 6;
        allocation.bytes = 64;
        allocation.pimpl = 7;
        allocator.observation = KNOWN;
        allocator.address = 8;
        allocator.pimpl = 9;
        allocator.pimpl_matches = true;
        (allocation, allocator)
    }

    #[test]
    fn inspection() {
        let (allocation, allocator) = parts();
        let native = NativeBuffer { gpu: 0x100, bytes: 64, ..Default::default() };
        let mut a = Observation { gpu: 0x100, width: 64, ..Default::default() };
        inspect_allocation(&mut a, &allocation, &allocator, &native);
        assert_eq!((a.allocation_kind, a.allocation_heap, a.allocator, a.allocator_pimpl), (1, 6, 8, 9));
        assert_eq!(a.flags, ALLOCATION_KNOWN | ALLOCATOR_KNOWN | FULL_BACKING);
        let mut b = Observation { gpu: 0x100, width: 128, ..Default::default() };
        let mut unmatched = allocator;
        unmatched.pimpl_matches = false;
        inspect_allocation(&mut b, &allocation, &unmatched, &native);
        assert_eq!(b.flags, ALLOCATION_KNOWN);
    }
    #[test]
    fn placed_candidates() {
        let (allocation, allocator) = parts();
        let a = Observation { flags: REQUIRED, heap: D3D12_HEAP_TYPE_DEFAULT.0 as u64, ..Default::default() };
        assert!(placement_candidate(&a, 1, 2, true, &allocation, &allocator));
        assert!(!placement_candidate(&a, 2, 2, true, &allocation, &allocator));
        assert!(!placement_candidate(&a, 1, 1, true, &allocation, &allocator));
        assert!(!placement_candidate(&a, 1, 2, false, &allocation, &allocator));
        assert!(!placement_candidate(&Observation { flags: REQUIRED & !SCOPE_CURRENT, ..a }, 1, 2, true, &allocation, &allocator));
        assert!(!placement_candidate(&Observation { pool_owner: 1, ..a }, 1, 2, true, &allocation, &allocator));
        assert!(!placement_candidate(&Observation { alias_control: 1, ..a }, 1, 2, true, &allocation, &allocator));
        assert!(!placement_candidate(&Observation { heap: 2, ..a }, 1, 2, true, &allocation, &allocator));
        let mut direct = allocation;
        direct.kind = 0;
        assert!(!placement_candidate(&a, 1, 2, true, &direct, &allocator));
    }
}
