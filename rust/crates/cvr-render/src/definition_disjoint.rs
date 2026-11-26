//! The native definition tracking's disjointness checks (src/Stereo/NativeDefinitionTracking.cpp, `TreesDisjoint` and
//! `AllocationReceiptsDisjoint`): two definition trees share no node object or control block, no pair of non-empty arrays
//! overlaps from its data through its trailer's eight bytes, and no pair of allocated strings overlaps; and two allocator
//! receipts' allocations never overlap.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ndt_trees_disjoint` (the trees' records as
//! definition_tree's Rust view) and `cvr_ndt_allocations_disjoint` (allocator_binding's allocations); the unit tests below
//! cover each overlap.

use crate::definition_tree_abi::View;
use crate::native::allocator_binding::Allocation;
use crate::native::definition_tree::{Array, Node, StringStorage};

/// The records TreesDisjoint compares.
pub struct Tree<'a> {
    pub nodes: &'a [Node],
    pub arrays: &'a [Array],
    pub strings: &'a [StringStorage],
}
/// TreesDisjoint.
pub fn trees_disjoint(first: &Tree<'_>, second: &Tree<'_>) -> bool {
    let nodes = first.nodes.iter().all(|a| second.nodes.iter().all(|b| !(a.object == b.object || (a.control_block != 0 && a.control_block == b.control_block))));
    let arrays = first.arrays.iter().all(|a| {
        second.arrays.iter().all(|b| {
            !(a.capacity != 0 && b.capacity != 0 && a.data < b.trailer_address.wrapping_add(8) && b.data < a.trailer_address.wrapping_add(8))
        })
    });
    let strings = first.strings.iter().all(|a| {
        second.strings.iter().all(|b| {
            !(a.allocation_bytes != 0
                && b.allocation_bytes != 0
                && a.data < b.data.wrapping_add(b.allocation_bytes)
                && b.data < a.data.wrapping_add(a.allocation_bytes))
        })
    });
    nodes && arrays && strings
}
/// AllocationReceiptsDisjoint.
pub fn allocations_disjoint(a: &[Allocation], b: &[Allocation]) -> bool {
    a.iter().all(|x| b.iter().all(|y| !(x.address < y.address.wrapping_add(y.bytes) && y.address < x.address.wrapping_add(x.bytes))))
}

// ---- the C ABI

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
unsafe fn tree(view: &View) -> Tree<'_> {
    unsafe { Tree { nodes: slice(view.nodes, view.node_count), arrays: slice(view.arrays, view.array_count), strings: slice(view.strings, view.string_count) } }
}
/// # Safety
/// Both views borrow live snapshot records.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_trees_disjoint(first: *const View, second: *const View) -> bool {
    unsafe { trees_disjoint(&tree(&*first), &tree(&*second)) }
}
/// # Safety
/// The arrays hold their counts (null when empty).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_allocations_disjoint(a: *const Allocation, a_count: usize, b: *const Allocation, b_count: usize) -> bool {
    unsafe { allocations_disjoint(slice(a, a_count), slice(b, b_count)) }
}

#[cfg(test)]
mod tests {
    //! Each overlap.
    use super::*;

    fn node(object: u64, control_block: u64) -> Node {
        Node { object, control_block, ..Default::default() }
    }
    fn array(data: u64, trailer: u64, capacity: u32) -> Array {
        let mut a: Array = unsafe { core::mem::zeroed() };
        a.data = data;
        a.trailer_address = trailer;
        a.capacity = capacity;
        a
    }
    fn string(data: u64, bytes: u64) -> StringStorage {
        StringStorage { data, allocation_bytes: bytes, ..Default::default() }
    }
    #[test]
    fn trees() {
        let run = |a: Tree<'_>, b: Tree<'_>| trees_disjoint(&a, &b);
        let none = Tree { nodes: &[], arrays: &[], strings: &[] };
        assert!(run(Tree { ..none }, Tree { ..none }));
        assert!(run(Tree { nodes: &[node(1, 0)], ..none }, Tree { nodes: &[node(2, 0)], ..none }));
        assert!(!run(Tree { nodes: &[node(1, 0)], ..none }, Tree { nodes: &[node(1, 5)], ..none }));
        assert!(!run(Tree { nodes: &[node(1, 5)], ..none }, Tree { nodes: &[node(2, 5)], ..none }));
        // Arrays: data through the trailer's eight bytes.
        assert!(run(Tree { arrays: &[array(0x100, 0x140, 8)], ..none }, Tree { arrays: &[array(0x148, 0x180, 8)], ..none }));
        assert!(!run(Tree { arrays: &[array(0x100, 0x140, 8)], ..none }, Tree { arrays: &[array(0x147, 0x180, 8)], ..none }));
        assert!(!run(Tree { arrays: &[array(0x147, 0x180, 8)], ..none }, Tree { arrays: &[array(0x100, 0x140, 8)], ..none }));
        assert!(!run(Tree { strings: &[string(0x11F, 0x20)], ..none }, Tree { strings: &[string(0x100, 0x20)], ..none }));
        assert!(run(Tree { arrays: &[array(0x100, 0x140, 0)], ..none }, Tree { arrays: &[array(0x120, 0x180, 8)], ..none }));
        // Strings: their allocations.
        assert!(run(Tree { strings: &[string(0x100, 0x20)], ..none }, Tree { strings: &[string(0x120, 0x20)], ..none }));
        assert!(!run(Tree { strings: &[string(0x100, 0x21)], ..none }, Tree { strings: &[string(0x120, 0x20)], ..none }));
        assert!(run(Tree { strings: &[string(0x100, 0)], ..none }, Tree { strings: &[string(0x100, 0x20)], ..none }));
    }
    #[test]
    fn allocations() {
        let at = |address: u64, bytes: u64| Allocation { address, bytes, ..Default::default() };
        assert!(allocations_disjoint(&[at(0x100, 0x10)], &[at(0x110, 0x10)]));
        assert!(!allocations_disjoint(&[at(0x100, 0x11)], &[at(0x110, 0x10)]));
        assert!(!allocations_disjoint(&[at(0x200, 1), at(0x100, 0x11)], &[at(0x110, 0x10)]));
        assert!(allocations_disjoint(&[], &[at(0x110, 0x10)]));
    }
}
