//! THE NATIVE DESCRIPTOR ARENA'S RULES (the pure part of include/Render/NativeDescriptorArena.hpp, detail): a native
//! descriptor ring's valid observation (its heap, a bounded live interval, outstanding reservations); the absolute
//! position of a nonblocking reservation made from that ring (its first slot in the cycle of the head before, inside
//! the interval the head advanced); the plan of a copy at the native recording state's current cursor (nothing dirty
//! in the graphics groups, no rebind, room in the allocation, the CPU / GPU handles of the first slot); and the check
//! that the native state afterwards is the one before with only its cursor advanced past the copy. The header forwards
//! to these in the Rust build; tools/single_pass_tests/native_descriptor_arena_test runs its cases against them.
use crate::camera_flush::{self, Snapshot, SnapshotView};

/// native_descriptors::detail::Ring.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Ring {
    pub address: u64,
    pub heap: u64,
    pub capacity: u64,
    pub head: u64,
    pub tail: u64,
    pub outstanding: u64,
}
/// native_descriptors::Range.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Range {
    pub first: u32,
    pub count: u32,
    pub before: u32,
    pub after: u32,
    pub cpu: usize,
    pub gpu: u64,
}
const _: () = assert!(core::mem::size_of::<Ring>() == 48 && core::mem::size_of::<Range>() == 32);

/// ValidRing.
pub fn valid_ring(ring: &Ring, heap: u64, heap_count: u32) -> bool {
    ring.address != 0
        && ring.address & 7 == 0
        && ring.heap == heap
        && heap != 0
        && ring.capacity != 0
        && ring.capacity <= heap_count as u64
        && ring.head >= ring.tail
        && ring.head - ring.tail <= ring.capacity
        && ring.outstanding != 0
        && ring.outstanding <= 1_048_576
}

/// RingResult: the reservation's absolute start.
pub fn ring_result(before: &Ring, after: &Ring, first: u32, count: u32) -> Option<u64> {
    let (first, count) = (first as u64, count as u64);
    if count == 0
        || count > 8192
        || !valid_ring(before, before.heap, 1_048_576)
        || !valid_ring(after, before.heap, 1_048_576)
        || before.address != after.address
        || before.capacity != after.capacity
        || first >= before.capacity
        || count > before.capacity - first
        || after.head < before.head
        || after.head - before.head > before.capacity
        || after.tail < before.tail
        || after.tail > before.head
    {
        return None;
    }
    let cycle = before.head - before.head % before.capacity;
    if first > u64::MAX - cycle {
        return None;
    }
    let mut start = cycle + first;
    if start < before.head {
        if before.capacity > u64::MAX - start {
            return None;
        }
        start += before.capacity;
    }
    if start > after.head || count > after.head - start {
        return None;
    }
    Some(start)
}

/// Plan: a copy of `count` descriptors at the native state's current cursor.
pub fn plan(
    state: &Snapshot,
    count: u32,
    heap_count: u32,
    stride: u32,
    cpu: u64,
    gpu: u64,
) -> Option<Range> {
    if count == 0
        || count > 8192
        || state.heap == 0
        || !state.context.valid()
        || stride == 0
        || stride != state.stride
        || cpu == 0
        || gpu == 0
        || state.dirty & state.graphics_mask != 0
        || state.rebind != 0
        || state.capacity == 0
        || state.cursor > state.capacity
        || count > state.capacity - state.cursor
        || state.base as u64 + state.capacity as u64 > heap_count as u64
    {
        return None;
    }
    let first = state.base as u64 + state.cursor as u64;
    let bytes = (first + count as u64) * stride as u64;
    if bytes > u64::MAX - cpu || bytes > u64::MAX - gpu {
        return None;
    }
    Some(Range {
        first: first as u32,
        count,
        before: state.cursor,
        after: state.cursor + count,
        cpu: (cpu + first * stride as u64) as usize,
        gpu: gpu + first * stride as u64,
    })
}

/// Advanced: the state after is the one before with only its cursor at the copy's end.
pub fn advanced(before: &Snapshot, after: &Snapshot, range: &Range) -> bool {
    let mut expected = before.clone();
    expected.cursor = range.after;
    range.count != 0
        && range.cpu != 0
        && range.gpu != 0
        && range.first as u64 == before.base as u64 + before.cursor as u64
        && range.before == before.cursor
        && range.before as u64 + range.count as u64 == range.after as u64
        && range.after <= before.capacity
        && *after == expected
}

/// ValidRing.
///
/// # Safety
/// `ring` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_da_valid_ring(
    ring: *const Ring,
    heap: u64,
    heap_count: u32,
) -> bool {
    valid_ring(unsafe { &*ring }, heap, heap_count)
}

/// RingResult (`absolute` 0 on a refusal).
///
/// # Safety
/// The rings are valid; `absolute` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_da_ring_result(
    before: *const Ring,
    after: *const Ring,
    first: u32,
    count: u32,
    absolute: *mut u64,
) -> bool {
    unsafe {
        let start = ring_result(&*before, &*after, first, count);
        *absolute = start.unwrap_or(0);
        start.is_some()
    }
}

/// Plan (`result` zeroed on a refusal).
///
/// # Safety
/// The view is valid and holds its counts; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_da_plan(
    state: *const SnapshotView,
    count: u32,
    heap_count: u32,
    stride: u32,
    cpu: u64,
    gpu: u64,
    result: *mut Range,
) -> bool {
    unsafe {
        let planned = plan(
            &camera_flush::snapshot_of(&*state),
            count,
            heap_count,
            stride,
            cpu,
            gpu,
        );
        *result = planned.unwrap_or_default();
        planned.is_some()
    }
}

/// Advanced.
///
/// # Safety
/// The views are valid and hold their counts; `range` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_da_advanced(
    before: *const SnapshotView,
    after: *const SnapshotView,
    range: *const Range,
) -> bool {
    unsafe {
        advanced(
            &camera_flush::snapshot_of(&*before),
            &camera_flush::snapshot_of(&*after),
            &*range,
        )
    }
}
