//! THE NATIVE IA SNAPSHOTS' RULES (the pure part of include/Render/NativeIaSnapshot.hpp's detail namespace): which
//! bound index / vertex buffers of an original draw may be copied into a private arena -- the admission of an
//! observation (scope, code, parent, bindings, recording, native record, heap, pool or placed backing, creation,
//! protection, shape, view, allocation, allocator, native state), the engine's geometry pools (five size classes, the
//! slot's occupancy and owner) and placed allocations (the metadata's guarded offset getter) read twice, the arena's
//! reservations and the union of overlapping views into one copy per touched range, the private views and the
//! recovery identity. The header forwards to these in the Rust build; tools/single_pass_tests/native_ia_snapshot_test
//! runs its cases against them (its Rust variant).
use core::ffi::c_void;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

use crate::buffer_read::NativeBuffer;
use crate::source_cbv_details::ReadState;
use crate::texture_immutable::PlacedAllocation;

pub const MAX_ARENA_BYTES: u64 = 64 * 1024 * 1024;
pub const ALIGNMENT: u64 = 16;
pub const MAX_VIEWS: usize = 17;
// detail::Flag.
pub const RECORD_KNOWN: u64 = 1;
pub const CREATION_KNOWN: u64 = 2;
pub const ALLOCATION_KNOWN: u64 = 4;
pub const ALLOCATOR_KNOWN: u64 = 8;
pub const SAME_DEVICE: u64 = 16;
pub const SCOPE_CURRENT: u64 = 32;
pub const BINDINGS_CURRENT: u64 = 64;
pub const PARENT_CURRENT: u64 = 128;
pub const CONTEXT_KNOWN: u64 = 256;
pub const CODE_KNOWN: u64 = 512;
pub const FULL_BACKING: u64 = 1024;
pub const GEOMETRY_POOL_KNOWN: u64 = 4096;
pub const PLACED_KNOWN: u64 = 8192;
/// ia_snapshot::Failure.
#[repr(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    None,
    Disabled,
    Empty,
    Arguments,
    Scope,
    Code,
    Parent,
    CurrentIa,
    Recording,
    NativeRecord,
    CreationDeferred,
    HeapDeferred,
    ProtectedDeferred,
    PoolDeferred,
    PlacedDeferred,
    UploadDeferred,
    ClassDeferred,
    AliasDeferred,
    EarlyFixedDeferred,
    BackingDeferred,
    AllocationDeferred,
    AllocatorDeferred,
    State,
}

/// ia_snapshot::Observation: current numeric observations only (64 words).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Observation {
    pub role: u64,
    pub slot: u64,
    pub reason: u64,
    pub resource: u64,
    pub gpu: u64,
    pub width: u64,
    pub view_gpu: u64,
    pub view_bytes: u64,
    pub stride_or_format: u64,
    pub com_offset: u64,
    pub native_resource: u64,
    pub native_gpu: u64,
    pub native_bytes: u64,
    pub registry: u64,
    pub context: u64,
    pub handle: u64,
    pub native_kind: u64,
    pub record_address: u64,
    pub low_type: u64,
    pub heap: u64,
    pub heap_flags: u64,
    pub resource_flags: u64,
    pub creation_kind: u64,
    pub creation_heap: u64,
    pub creation_offset: u64,
    pub creation_bytes: u64,
    pub initial_known: u64,
    pub initial_state: u64,
    pub unprotected: u64,
    pub allocation: u64,
    pub allocation_kind: u64,
    pub allocation_resource: u64,
    pub allocation_heap: u64,
    pub allocation_bytes: u64,
    pub allocation_pimpl: u64,
    pub allocator: u64,
    pub allocator_pimpl: u64,
    pub state_value: u64,
    pub state_status: u64,
    pub state_kind: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub flags: u64,
    pub reserved_bytes: u64,
    pub private_offset: u64,
    pub reserved: u64,
    pub alias_payload: u64,
    pub alias_control: u64,
    pub pool_owner: u64,
    pub pool_slot: u64,
    pub pool_unit: u64,
    pub allocation_observation: u64,
    pub allocator_observation: u64,
    pub pool_observation: u64,
    pub protected_result: u64,
    pub dimension: u64,
    pub alignment: u64,
    pub height: u64,
    pub depth: u64,
    pub mips: u64,
    pub heap_node_masks: u64,
    pub session_present: u64,
    pub shape_known: u64,
}
/// detail::GeometryPool.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct GeometryPool {
    pub header: u64,
    pub data: u64,
    pub entry: u64,
    pub resource: u64,
    pub owner: u64,
    pub occupied_word: u64,
    pub packed: u32,
    pub size_class: u32,
    pub group: u32,
    pub slot: u32,
    pub unit: u32,
    pub capacity: u32,
    pub count: u32,
    pub slots: u32,
    pub units: [u32; 5],
}
/// detail::CopyRange.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CopyRange {
    pub resource: u64,
    pub source_offset: u64,
    pub bytes: u64,
    pub destination: u64,
}
/// detail::RecoveryContext.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RecoveryContext {
    pub address: u64,
    pub list: u64,
    pub state: u64,
    pub submitted: u64,
    pub kind: u32,
    pub pending: u32,
    pub known: bool,
}
const _: () = assert!(
    core::mem::size_of::<Observation>() == 512
        && core::mem::size_of::<GeometryPool>() == 104
        && core::mem::size_of::<CopyRange>() == 32
        && core::mem::size_of::<RecoveryContext>() == 48
);

/// geometry::MemoryReader.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;
/// detail::AllocationOffsetReader: the guarded native getter of a pinned allocation's heap offset.
pub type OffsetGetter = Option<unsafe extern "C" fn(*mut c_void, u64, *mut u64) -> bool>;

fn read(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: the caller's reader writes at most `out.len()` bytes.
    reader.is_some_and(|r| {
        at != 0
            && at <= u64::MAX - out.len() as u64
            && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
    })
}
fn read_value<T: Copy + Default>(reader: Reader, user: *mut c_void, at: u64) -> Option<T> {
    let mut value = T::default();
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut value as *mut T).cast::<u8>(),
            core::mem::size_of::<T>(),
        )
    };
    read(reader, user, at, bytes).then_some(value)
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// detail::Selection: the slots to copy -- the original's, less the separately captured instance slot 7.
pub fn selection(original: u32, separate: u32) -> Option<u32> {
    if original & !65535 != 0
        || (separate != 0 && separate != 128)
        || separate & original != separate
    {
        return None;
    }
    Some(original & !separate)
}
/// detail::ReadPlacedOffset: a pinned placed allocation's heap offset through its metadata's guarded getter, the whole
/// route read before and after, twice.
#[allow(clippy::too_many_arguments)]
pub fn read_placed_offset(
    reader: Reader,
    getter: OffsetGetter,
    user: *mut c_void,
    base: u64,
    image_bytes: u64,
    expected: &PlacedAllocation,
) -> Option<PlacedAllocation> {
    let getter = getter?;
    if reader.is_none()
        || base == 0
        || image_bytes <= 0x306EC50
        || image_bytes > u64::MAX - base
        || expected.allocation == 0
        || expected.allocation > u64::MAX - 0x68
        || expected.resource == 0
        || expected.pimpl == 0
        || expected.block == 0
        || expected.block > u64::MAX - 0x48
        || expected.handle == 0
        || expected.heap == 0
        || expected.bytes == 0
    {
        return None;
    }
    let valid = |raw: &[u8; 0x68]| {
        u64_at(raw, 0) == base + 0x30466A8
            && u32_at(raw, 8) != 0
            && u32_at(raw, 8) < i32::MAX as u32
            && u32_at(raw, 0x60) & 3 == 1
            && u64_at(raw, 0x10) == expected.pimpl
            && u64_at(raw, 0x18) == expected.bytes
            && u64_at(raw, 0x28) == expected.resource
            && u64_at(raw, 0x40) == expected.handle
            && u64_at(raw, 0x48) == expected.block
    };
    let once = || -> Option<PlacedAllocation> {
        let mut current = *expected;
        let mut raw = [0u8; 0x68];
        if !read(reader, user, expected.allocation, &mut raw) || !valid(&raw) {
            return None;
        }
        let heap = read_value::<u64>(reader, user, expected.block + 0x38)?;
        if heap != expected.heap {
            return None;
        }
        let metadata = read_value::<u64>(reader, user, expected.block + 0x40)?;
        if metadata == 0 || metadata > u64::MAX - 8 {
            return None;
        }
        current.metadata_vtable = read_value::<u64>(reader, user, metadata)?;
        if current.metadata_vtable < base || current.metadata_vtable - base > image_bytes - 0x38 {
            return None;
        }
        current.offset_method = read_value::<u64>(reader, user, current.metadata_vtable + 0x30)?;
        if current.offset_method < base || current.offset_method - base >= image_bytes {
            return None;
        }
        current.metadata = metadata;
        // SAFETY: the caller's getter.
        if !unsafe { getter(user, expected.allocation, &mut current.offset) } {
            return None;
        }
        let same = read(reader, user, expected.allocation, &mut raw)
            && valid(&raw)
            && read_value::<u64>(reader, user, expected.block + 0x38) == Some(heap)
            && read_value::<u64>(reader, user, expected.block + 0x40) == Some(metadata)
            && read_value::<u64>(reader, user, metadata) == Some(current.metadata_vtable)
            && read_value::<u64>(reader, user, current.metadata_vtable + 0x30)
                == Some(current.offset_method);
        same.then_some(current)
    };
    let first = once()?;
    let again = once()?;
    (first == again).then_some(first)
}
/// detail::LogicalRange: the native buffer inside the resource range and the view inside the native buffer.
pub fn logical_range(
    gpu: u64,
    bytes: u64,
    native_gpu: u64,
    native_bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
) -> bool {
    gpu != 0
        && bytes != 0
        && gpu <= u64::MAX - bytes
        && native_gpu >= gpu
        && native_bytes != 0
        && native_gpu - gpu <= bytes
        && native_bytes <= bytes - (native_gpu - gpu)
        && view_gpu >= native_gpu
        && view_bytes != 0
        && view_gpu - native_gpu <= native_bytes
        && view_bytes <= native_bytes - (view_gpu - native_gpu)
}
/// detail::ReadGeometryPool: the engine's geometry pools (five size classes): the record unchanged, the pool enabled
/// (the geometry-only switch off for type 5), the slot inside the resource and occupied by it, read twice.
#[allow(clippy::too_many_arguments)]
pub fn read_geometry_pool(
    reader: Reader,
    user: *mut c_void,
    base: u64,
    native: &NativeBuffer,
    saved: &[u8; 0xB0],
    gpu: u64,
    bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
    low_type: u8,
) -> Option<GeometryPool> {
    if base == 0
        || base > u64::MAX - 0x3BCDCB0
        || native.registry == 0
        || native.registry > u64::MAX - 0x1AE2C6A
        || native.context == 0
        || native.handle == 0
        || native.handle > 0x100000
        || native.kind != 1
        || (low_type != 5 && low_type != 13)
        || saved[6] != (0x10 | low_type)
        || native.resource == 0
        || (native.bytes > 32768 && (native.bytes <= 65536 || native.bytes > 98304))
        || !logical_range(gpu, bytes, native.gpu, native.bytes, view_gpu, view_bytes)
        || u64_at(saved, 0x18) != 0
        || u64_at(saved, 0x20) != 0
        || u64_at(saved, 0x28) != 0
    {
        return None;
    }
    let record_offset = 0x5C0AF8 + (native.handle as u64 - 1) * 0xB0;
    if native.registry > u64::MAX - record_offset - saved.len() as u64 {
        return None;
    }
    let record_address = native.registry + record_offset;
    let once = || -> Option<GeometryPool> {
        let mut pool = GeometryPool::default();
        let mut raw = [0u8; 0xB0];
        let mut units = [0u8; 20];
        if read_value::<u64>(reader, user, base + 0x3BCDCA8) != Some(native.registry)
            || read_value::<u8>(reader, user, native.registry + 0x14105C0).is_none_or(|v| v == 0)
        {
            return None;
        }
        let disabled = read_value::<u8>(reader, user, native.registry + 0x1AE2C69)?;
        if (disabled != 0 && low_type == 5)
            || !read(reader, user, record_address, &mut raw)
            || raw != *saved
            || u32_at(&raw, 0) as u64 != native.bytes
            || u64_at(&raw, 8) != native.gpu
            || u64_at(&raw, 16) != native.resource
            || !read(reader, user, base + 0x36F44C8, &mut units)
        {
            return None;
        }
        for (i, unit) in pool.units.iter_mut().enumerate() {
            *unit = u32_at(&units, i * 4);
        }
        pool.packed = u32_at(&raw, 0x40);
        pool.size_class = pool.packed & 7;
        pool.group = (pool.packed & 65535) >> 3;
        pool.slot = pool.packed >> 16;
        if pool.size_class as usize >= pool.units.len() {
            return None;
        }
        for i in 0..pool.units.len() {
            if pool.units[i] == 0
                || pool.units[i] > 98304
                || (i != 0 && pool.units[i] <= pool.units[i - 1])
            {
                return None;
            }
        }
        pool.unit = pool.units[pool.size_class as usize];
        let unit = pool.unit as u64;
        if native.bytes > unit
            || (pool.size_class != 0
                && native.bytes <= pool.units[pool.size_class as usize - 1] as u64)
            || bytes % unit != 0
            || bytes / unit > 1024
            || bytes == 0
            || pool.slot as u64 >= bytes / unit
            || native.gpu != gpu + pool.slot as u64 * unit
        {
            return None;
        }
        pool.slots = (bytes / unit) as u32;
        pool.header = native.registry + 0x1410548 + pool.size_class as u64 * 24;
        let mut header = [0u8; 16];
        if !read(reader, user, pool.header, &mut header) {
            return None;
        }
        pool.data = u64_at(&header, 0);
        pool.capacity = u32_at(&header, 8);
        pool.count = u32_at(&header, 12);
        if pool.data == 0
            || pool.count == 0
            || pool.count > pool.capacity
            || pool.capacity > 8192
            || pool.group >= pool.count
            || pool.data > u64::MAX - pool.group as u64 * 144 - 144
        {
            return None;
        }
        pool.entry = pool.data + pool.group as u64 * 144;
        let mut entry = [0u8; 144];
        if !read(reader, user, pool.entry, &mut entry) {
            return None;
        }
        pool.resource = u64_at(&entry, 0);
        pool.owner = u64_at(&entry, 8);
        pool.occupied_word = u64_at(&entry, 16 + (pool.slot as usize / 64) * 8);
        (pool.resource == native.resource
            && pool.owner != 0
            && pool.occupied_word & (1u64 << (pool.slot % 64)) != 0)
            .then_some(pool)
    };
    let first = once()?;
    let again = once()?;
    (first == again).then_some(first)
}
pub fn requires_uniform_state_proof(first: u32, second: u32) -> bool {
    first == 0 || first == 3 || second == 0 || second == 3
}
/// detail::ReadBit: the role's read state (index buffer, or vertex and constant buffer).
pub fn read_bit(role: u64) -> u32 {
    if role == 0 {
        D3D12_RESOURCE_STATE_INDEX_BUFFER.0 as u32
    } else {
        D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 as u32
    }
}
/// detail::ReadMask: a buffer of kind 0 or 3 tracked per subresource, holding the reads and only generic-read bits.
pub fn read_mask(state: ReadState, kind: u32, reads: u32, ready: bool) -> bool {
    (kind == 0 || kind == 3)
        && state.kind == kind
        && state.status == 2
        && reads != 0
        && state.value & reads == reads
        && state.value & !(D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32) == 0
        && (!ready || state.value & D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 != 0)
}
/// detail::RecoveryMask: the state to restore after our copy (the previous reads and the current ones).
pub fn recovery_mask(previous: u32, current: ReadState, kind: u32, reads: u32) -> Option<u32> {
    if !read_mask(
        ReadState {
            value: previous,
            status: 2,
            kind,
        },
        kind,
        reads,
        false,
    ) || current.status != 2
        || current.kind != kind
        || current.value & !(D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32) != 0
    {
        return None;
    }
    Some(previous | current.value)
}
pub fn barrier_capacity(data: u64, capacity: u32, pending: u32) -> bool {
    data != 0
        && capacity != 0
        && capacity <= 1048576
        && pending == 0
        && data <= u64::MAX - capacity as u64 * 32
}
/// detail::Reserve: `bytes` (16-aligned) at the arena cursor: (offset, end).
pub fn reserve(cursor: u64, capacity: u64, bytes: u64) -> Option<(u64, u64)> {
    if capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity % ALIGNMENT != 0
        || cursor % ALIGNMENT != 0
        || cursor > capacity
        || bytes == 0
        || bytes > u32::MAX as u64
        || bytes > u64::MAX - (ALIGNMENT - 1)
    {
        return None;
    }
    let padded = (bytes + ALIGNMENT - 1) & !(ALIGNMENT - 1);
    if padded > capacity - cursor {
        return None;
    }
    Some((cursor, cursor + padded))
}
/// detail::CopyPlan.
#[derive(Default, Debug)]
pub struct CopyPlan {
    pub ranges: Vec<CopyRange>,
    pub destinations: Vec<u64>,
    pub next: u64,
    pub copied: u64,
}
/// detail::PlanCopies: one copy per run of overlapping/touching views of one resource (never bridging an unread gap),
/// each at a 16-aligned arena offset keeping its source offset's alignment; every view's destination inside its copy.
pub fn plan_copies(input: &[CopyRange], mut cursor: u64, capacity: u64) -> Option<CopyPlan> {
    if input.is_empty() || input.len() > MAX_VIEWS {
        return None;
    }
    let mut sorted = input.to_vec();
    if sorted
        .iter()
        .any(|r| r.resource == 0 || r.bytes == 0 || r.source_offset > u64::MAX - r.bytes)
    {
        return None;
    }
    sorted.sort_by(|a, b| (a.resource, a.source_offset).cmp(&(b.resource, b.source_offset)));
    let mut plan = CopyPlan::default();
    for range in sorted {
        if let Some(last) = plan.ranges.last_mut() {
            if last.resource == range.resource
                && range.source_offset <= last.source_offset + last.bytes
            {
                last.bytes = (last.source_offset + last.bytes)
                    .max(range.source_offset + range.bytes)
                    - last.source_offset;
                continue;
            }
        }
        plan.ranges.push(range);
    }
    for range in &mut plan.ranges {
        let prefix = range.source_offset % ALIGNMENT;
        if range.bytes > u64::MAX - prefix {
            return None;
        }
        let (start, end) = reserve(cursor, capacity, range.bytes + prefix)?;
        range.destination = start + prefix;
        cursor = end;
        plan.copied += range.bytes;
    }
    for range in input {
        let group = plan.ranges.iter().find(|copy| {
            copy.resource == range.resource
                && range.source_offset >= copy.source_offset
                && range.source_offset - copy.source_offset <= copy.bytes
                && range.bytes <= copy.bytes - (range.source_offset - copy.source_offset)
        })?;
        plan.destinations
            .push(group.destination + range.source_offset - group.source_offset);
    }
    plan.next = cursor;
    Some(plan)
}
pub fn index_view(
    gpu: u64,
    offset: u64,
    capacity: u64,
    source: &D3D12_INDEX_BUFFER_VIEW,
) -> Option<D3D12_INDEX_BUFFER_VIEW> {
    let unit: u64 = match source.Format {
        f if f == DXGI_FORMAT_R16_UINT => 2,
        f if f == DXGI_FORMAT_R32_UINT => 4,
        _ => 0,
    };
    let size = source.SizeInBytes as u64;
    if unit == 0
        || gpu == 0
        || gpu > u64::MAX - capacity
        || size == 0
        || offset > capacity
        || size > capacity - offset
        || gpu > u64::MAX - offset
        || (gpu + offset) % unit != 0
        || size % unit != 0
    {
        return None;
    }
    Some(D3D12_INDEX_BUFFER_VIEW {
        BufferLocation: gpu + offset,
        ..*source
    })
}
pub fn vertex_view(
    gpu: u64,
    offset: u64,
    capacity: u64,
    source: &D3D12_VERTEX_BUFFER_VIEW,
) -> Option<D3D12_VERTEX_BUFFER_VIEW> {
    let size = source.SizeInBytes as u64;
    if gpu == 0
        || gpu > u64::MAX - capacity
        || size == 0
        || source.StrideInBytes == 0
        || offset > capacity
        || size > capacity - offset
        || gpu > u64::MAX - offset
    {
        return None;
    }
    Some(D3D12_VERTEX_BUFFER_VIEW {
        BufferLocation: gpu + offset,
        ..*source
    })
}
/// detail::Admit: the first reason an observed view may not be copied (None: admitted).
pub fn admit(a: &Observation) -> Failure {
    use Failure::*;
    if a.flags & SCOPE_CURRENT == 0 {
        return Scope;
    }
    if a.flags & CODE_KNOWN == 0 {
        return Code;
    }
    if a.flags & PARENT_CURRENT == 0 {
        return Parent;
    }
    if a.flags & BINDINGS_CURRENT == 0 {
        return CurrentIa;
    }
    if a.flags & CONTEXT_KNOWN == 0 || a.list == 0 || a.generation == 0 || a.occurrence == 0 {
        return Recording;
    }
    if a.flags & RECORD_KNOWN == 0
        || a.resource == 0
        || a.native_resource != a.resource
        || a.registry == 0
        || a.context == 0
        || a.handle == 0
        || a.handle > 0x100000
    {
        return NativeRecord;
    }
    if a.heap == D3D12_HEAP_TYPE_UPLOAD.0 as u64 || a.heap == 5 {
        return UploadDeferred;
    }
    if a.heap != D3D12_HEAP_TYPE_DEFAULT.0 as u64 {
        return HeapDeferred;
    }
    let pooled = a.flags & GEOMETRY_POOL_KNOWN != 0;
    let placed = a.flags & PLACED_KNOWN != 0;
    if a.pool_owner != 0 && !pooled {
        return PoolDeferred;
    }
    if pooled
        && (a.native_kind != 1
            || a.low_type != 5
            || a.pool_owner == 0
            || a.pool_owner != a.allocation
            || a.pool_observation != 1
            || a.reserved > u32::MAX as u64
            || a.reserved & 7 >= 5
            || a.reserved >> 16 != a.pool_slot
            || a.pool_unit == 0
            || a.pool_unit > 98304
            || a.width % a.pool_unit != 0
            || a.width / a.pool_unit > 1024
            || a.pool_slot >= a.width / a.pool_unit
            || a.native_gpu < a.gpu
            || a.native_gpu - a.gpu != a.pool_slot.wrapping_mul(a.pool_unit)
            || a.native_bytes > a.pool_unit)
    {
        return PoolDeferred;
    }
    if a.alias_payload != 0 || a.alias_control != 0 {
        return AliasDeferred;
    }
    if a.flags & CREATION_KNOWN == 0 {
        return CreationDeferred;
    }
    let alignment = D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT as u64;
    if a.creation_kind == 2 {
        if !placed
            || pooled
            || a.creation_heap == 0
            || a.creation_bytes == 0
            || a.creation_offset > u64::MAX - a.creation_bytes
            || a.creation_offset % alignment != 0
            || a.reserved != a.creation_offset
            || a.allocation_kind != 1
            || a.allocation_heap != a.creation_heap
            || a.allocation_bytes != a.creation_bytes
        {
            return PlacedDeferred;
        }
    } else if placed
        || a.creation_kind != 1
        || a.creation_heap != 0
        || a.creation_offset != 0
        || a.creation_bytes != 0
    {
        return CreationDeferred;
    }
    let allowed_heap_flags =
        (D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES.0 | D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES.0) as u64;
    if a.unprotected == 0 || a.session_present != 0 || a.heap_flags & !allowed_heap_flags != 0 {
        return ProtectedDeferred;
    }
    if a.flags & SAME_DEVICE == 0
        || a.shape_known == 0
        || a.dimension != D3D12_RESOURCE_DIMENSION_BUFFER.0 as u64
        || a.width == 0
        || a.gpu == 0
        || a.gpu > u64::MAX - a.width
        || a.height != 1
        || a.depth != 1
        || a.mips != 1
        || (a.alignment != 0 && a.alignment != alignment)
        || (a.heap_node_masks >> 32) > 1
        || (a.heap_node_masks as u32) > 1
        || (!pooled
            && (a.flags & FULL_BACKING == 0 || a.native_gpu != a.gpu || a.native_bytes != a.width))
        || (pooled
            && !logical_range(
                a.gpu,
                a.width,
                a.native_gpu,
                a.native_bytes,
                a.view_gpu,
                a.view_bytes,
            ))
    {
        return BackingDeferred;
    }
    if a.view_gpu == 0
        || a.view_bytes == 0
        || a.view_gpu < a.gpu
        || a.com_offset != a.view_gpu - a.gpu
        || a.com_offset > a.width
        || a.view_bytes > a.width - a.com_offset
    {
        return Arguments;
    }
    if a.role > 1
        || (if a.role == 0 {
            a.slot != u32::MAX as u64
        } else {
            a.slot >= 16
        })
    {
        return Arguments;
    }
    if a.role == 0 {
        let unit = match a.stride_or_format {
            f if f == DXGI_FORMAT_R16_UINT.0 as u64 => 2,
            f if f == DXGI_FORMAT_R32_UINT.0 as u64 => 4,
            _ => 0,
        };
        if unit == 0 || a.view_gpu % unit != 0 || a.view_bytes % unit != 0 {
            return Arguments;
        }
    } else if a.stride_or_format == 0 || a.stride_or_format > u32::MAX as u64 {
        return Arguments;
    }
    if a.flags & ALLOCATION_KNOWN == 0
        || a.allocation == 0
        || a.allocation_kind != (placed as u64)
        || a.allocation_resource != a.resource
        || (!placed && a.allocation_heap != 0)
        || a.allocation_bytes < a.width
        || a.allocation_pimpl == 0
    {
        return AllocationDeferred;
    }
    if a.flags & ALLOCATOR_KNOWN == 0 || a.allocator == 0 || a.allocator_pimpl != a.allocation_pimpl
    {
        return AllocatorDeferred;
    }
    if a.native_kind == 1 {
        if a.state_status == 9 {
            return EarlyFixedDeferred;
        }
        if a.resource_flags != 0
            || a.initial_known == 0
            || (a.initial_state != D3D12_RESOURCE_STATE_COMMON.0 as u64
                && a.initial_state != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32 as u64)
            || a.state_kind != 1
            || a.state_status != 8
            || a.state_value != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32 as u64
        {
            return State;
        }
    } else if a.native_kind == 0 || a.native_kind == 3 {
        let state = ReadState {
            value: a.state_value as u32,
            status: a.state_status as u32,
            kind: a.state_kind as u32,
        };
        if a.resource_flags & !(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 as u64) != 0
            || !read_mask(state, a.native_kind as u32, read_bit(a.role), false)
        {
            return State;
        }
    } else {
        return ClassDeferred;
    }
    None
}
/// detail::RecoveryIdentity: the same native buffer, record bytes and still-recording context as at observation.
#[allow(clippy::too_many_arguments)]
pub fn recovery_identity(
    saved: &NativeBuffer,
    raw: &[u8; 0xB0],
    address: u64,
    context: &RecoveryContext,
    current: &NativeBuffer,
    current_raw: &[u8; 0xB0],
    current_address: u64,
    now: &RecoveryContext,
) -> bool {
    context.known
        && now.known
        && context.address != 0
        && context.list != 0
        && context.state != 0
        && context.pending == 0
        && context.submitted == 0
        && (context.kind == 1 || context.kind == 2)
        && now.address == context.address
        && now.list == context.list
        && now.state == context.state
        && now.kind == context.kind
        && now.submitted == 0
        && saved.context == context.address
        && current.context == saved.context
        && saved.resource != 0
        && current.resource == saved.resource
        && saved.gpu != 0
        && current.gpu == saved.gpu
        && saved.bytes != 0
        && current.bytes == saved.bytes
        && saved.registry != 0
        && current.registry == saved.registry
        && saved.handle != 0
        && current.handle == saved.handle
        && saved.kind == current.kind
        && (saved.kind == 0 || saved.kind == 3)
        && address != 0
        && current_address == address
        && raw == current_raw
}

// ---- the C ABI (the header's inline functions in the Rust build)

#[no_mangle]
pub unsafe extern "C" fn cvr_ia_selection(original: u32, separate: u32, copied: *mut u32) -> bool {
    let value = selection(original, separate);
    // SAFETY: the caller's slot.
    unsafe { *copied = value.unwrap_or(0) };
    value.is_some()
}
/// # Safety
/// `expected` and `out` valid; `reader` and `getter` the caller's for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_read_placed_offset(
    reader: Reader,
    getter: OffsetGetter,
    user: *mut c_void,
    base: u64,
    image_bytes: u64,
    expected: *const PlacedAllocation,
    out: *mut PlacedAllocation,
) -> bool {
    // SAFETY: as the caller promises.
    let value = read_placed_offset(reader, getter, user, base, image_bytes, unsafe {
        &*expected
    });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_ia_logical_range(
    gpu: u64,
    bytes: u64,
    native_gpu: u64,
    native_bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
) -> bool {
    logical_range(gpu, bytes, native_gpu, native_bytes, view_gpu, view_bytes)
}
/// # Safety
/// The records valid; `reader` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_ia_read_geometry_pool(
    reader: Reader,
    user: *mut c_void,
    base: u64,
    native: *const NativeBuffer,
    saved: *const [u8; 0xB0],
    gpu: u64,
    bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
    out: *mut GeometryPool,
    low_type: u8,
) -> bool {
    // SAFETY: as the caller promises.
    let value = read_geometry_pool(
        reader,
        user,
        base,
        unsafe { &*native },
        unsafe { &*saved },
        gpu,
        bytes,
        view_gpu,
        view_bytes,
        low_type,
    );
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_ia_requires_uniform_state_proof(first: u32, second: u32) -> bool {
    requires_uniform_state_proof(first, second)
}
#[no_mangle]
pub extern "C" fn cvr_ia_read_bit(role: u64) -> u32 {
    read_bit(role)
}
/// # Safety
/// `state` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_read_mask(
    state: *const ReadState,
    kind: u32,
    reads: u32,
    ready: bool,
) -> bool {
    // SAFETY: as the caller promises.
    read_mask(unsafe { *state }, kind, reads, ready)
}
/// # Safety
/// `current` and `required` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_recovery_mask(
    previous: u32,
    current: *const ReadState,
    kind: u32,
    reads: u32,
    required: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = recovery_mask(previous, unsafe { *current }, kind, reads);
    unsafe { *required = value.unwrap_or(0) };
    value.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_ia_barrier_capacity(data: u64, capacity: u32, pending: u32) -> bool {
    barrier_capacity(data, capacity, pending)
}
/// # Safety
/// `offset` and `end` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_reserve(
    cursor: u64,
    capacity: u64,
    bytes: u64,
    offset: *mut u64,
    end: *mut u64,
) -> bool {
    let value = reserve(cursor, capacity, bytes);
    let (o, e) = value.unwrap_or((0, 0));
    // SAFETY: as the caller promises.
    unsafe {
        *offset = o;
        *end = e;
    }
    value.is_some()
}
/// PlanCopies into the caller's buffers: `ranges` and `destinations` hold `count` entries each (a plan never has more
/// ranges than views); `range_count`, `next` and `copied` receive the rest.
///
/// # Safety
/// The buffers valid for `count`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_ia_plan_copies(
    input: *const CopyRange,
    count: usize,
    cursor: u64,
    capacity: u64,
    ranges: *mut CopyRange,
    range_count: *mut usize,
    destinations: *mut u64,
    next: *mut u64,
    copied: *mut u64,
) -> bool {
    // SAFETY: as the caller promises.
    let input = if count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(input, count) }
    };
    unsafe {
        *range_count = 0;
        *next = 0;
        *copied = 0;
    }
    let Some(plan) = plan_copies(input, cursor, capacity) else {
        return false;
    };
    // SAFETY: at most `count` of each.
    unsafe {
        core::ptr::copy_nonoverlapping(plan.ranges.as_ptr(), ranges, plan.ranges.len());
        core::ptr::copy_nonoverlapping(
            plan.destinations.as_ptr(),
            destinations,
            plan.destinations.len(),
        );
        *range_count = plan.ranges.len();
        *next = plan.next;
        *copied = plan.copied;
    }
    true
}
/// # Safety
/// `source` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_index_view(
    gpu: u64,
    offset: u64,
    capacity: u64,
    source: *const D3D12_INDEX_BUFFER_VIEW,
    out: *mut D3D12_INDEX_BUFFER_VIEW,
) -> bool {
    // SAFETY: as the caller promises.
    let value = index_view(gpu, offset, capacity, unsafe { &*source });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `source` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_vertex_view(
    gpu: u64,
    offset: u64,
    capacity: u64,
    source: *const D3D12_VERTEX_BUFFER_VIEW,
    out: *mut D3D12_VERTEX_BUFFER_VIEW,
) -> bool {
    // SAFETY: as the caller promises.
    let value = vertex_view(gpu, offset, capacity, unsafe { &*source });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `observation` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ia_admit(observation: *const Observation) -> u64 {
    // SAFETY: as the caller promises.
    admit(unsafe { &*observation }) as u64
}
/// # Safety
/// The records valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_ia_recovery_identity(
    saved: *const NativeBuffer,
    raw: *const [u8; 0xB0],
    address: u64,
    context: *const RecoveryContext,
    current: *const NativeBuffer,
    current_raw: *const [u8; 0xB0],
    current_address: u64,
    now: *const RecoveryContext,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        recovery_identity(
            &*saved,
            &*raw,
            address,
            &*context,
            &*current,
            &*current_raw,
            current_address,
            &*now,
        )
    }
}
