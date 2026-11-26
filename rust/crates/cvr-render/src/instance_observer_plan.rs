//! The pure plan of single pass's native instance observer (formerly src/Render/NativeInstanceObserverPlan.cpp):
//! which instance range a native writer filled, from the genuine allocator's reservation or from the 128-instance
//! cursor a previous writer left cached, and whether a pipeline's per-instance layout is one the observer can read.
use windows::Win32::Graphics::Direct3D12::{D3D12_APPEND_ALIGNED_ELEMENT, D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA};
use windows::Win32::Graphics::Dxgi::Common::*;

/// detail::Cursor: a writer's physical cursor, [first, end).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub first: u32,
    pub end: u32,
}
/// detail::ReservationRange: one reservation of the native ring allocator, in absolute coordinates.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ReservationRange {
    pub sequence: u64,
    pub head_before: u64,
    pub absolute_begin: u64,
    pub absolute_end: u64,
    pub capacity: u64,
    pub first: u32,
    pub instances: u32,
}
/// detail::CachedCursor: a 128-instance reservation a writer only partly used, and where its next writer starts.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CachedCursor {
    pub address: u64,
    pub ring: u64,
    pub ring_generation: u64,
    pub thread: u64,
    pub reservation: ReservationRange,
    pub next: Cursor,
}
const _: () = assert!(core::mem::size_of::<ReservationRange>() == 48 && core::mem::size_of::<CachedCursor>() == 88);

/// detail::WriterRange with its optional cache flattened (the header converts).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct WriterRange {
    pub absolute_begin: u64,
    pub reservation: ReservationRange,
    pub has_cache: u32,
    pub cache: CachedCursor,
}

/// One per-instance input of the pipeline's layout (shader_snapshot::Input without its semantic).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LayoutInput {
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub classification: u32,
    pub step: u32,
}

/// detail::ReservationResult: the genuine allocator's reservation, from the head just before it (only the runtime's
/// nonblocking gate around the guarded native allocator makes that head exclusive), the instances asked for, the
/// physical first it returned and the head right after it. A wrap pads to the ring's end, in absolute coordinates.
pub fn reservation_result(old: u64, capacity: u32, instances: u32, first: u32, after: u64) -> Option<ReservationRange> {
    if capacity == 0 || !capacity.is_power_of_two() || instances == 0 || instances > capacity {
        return None;
    }
    let physical = old & (capacity as u64 - 1);
    let room = capacity as u64 - physical;
    let mut begin = old;
    if instances as u64 > room {
        if old > u64::MAX - room {
            return None;
        }
        begin += room;
    }
    if begin > u64::MAX - instances as u64 || first as u64 != begin % capacity as u64 || after != begin + instances as u64 {
        return None;
    }
    Some(ReservationRange {
        sequence: 0,
        head_before: old,
        absolute_begin: begin,
        absolute_end: after,
        capacity: capacity as u64,
        first,
        instances,
    })
}

/// detail::ResolveWriter: the absolute range one native writer call filled, from the cursor before and after it,
/// the physical first it returned and either the reservation it made itself or the cursor a previous writer cached.
#[allow(clippy::too_many_arguments)]
pub fn resolve_writer(
    address: u64,
    ring: u64,
    generation: u64,
    thread: u64,
    capacity: u32,
    before: Cursor,
    after: Cursor,
    first: u32,
    instances: u32,
    actual: Option<&ReservationRange>,
    previous: Option<&CachedCursor>,
) -> Option<WriterRange> {
    if address == 0 || ring == 0 || generation == 0 || thread == 0 || instances == 0 || first >= capacity || instances > capacity - first {
        return None;
    }
    let mut out = WriterRange::default();
    if let Some(range) = actual {
        if range.sequence == 0
            || range.capacity != capacity as u64
            || range.absolute_end < range.absolute_begin
            || range.absolute_end - range.absolute_begin != range.instances as u64
            || range.first as u64 != range.absolute_begin % capacity as u64
            || first != range.first
            || instances > range.instances
        {
            return None;
        }
        let cached = before.first >= before.end && instances < 128 && range.instances == 128;
        if cached {
            if after.first != first + instances || after.end != first.wrapping_add(128) || 128 > capacity - first {
                return None;
            }
            out.has_cache = 1;
            out.cache = CachedCursor {
                address,
                ring,
                ring_generation: generation,
                thread,
                reservation: *range,
                next: after,
            };
        } else if range.instances != instances || before != after || (before.first < before.end && instances as u64 <= before.end as u64 - before.first as u64)
        {
            return None;
        }
        out.absolute_begin = range.absolute_begin;
        out.reservation = *range;
        return Some(out);
    }
    let cache = previous?;
    let range = &cache.reservation;
    if cache.address != address
        || cache.ring != ring
        || cache.ring_generation != generation
        || cache.thread != thread
        || cache.next != before
        || range.sequence == 0
        || range.capacity != capacity as u64
        || range.instances != 128
        || range.absolute_begin > u64::MAX - range.instances as u64
        || range.absolute_end != range.absolute_begin + range.instances as u64
        || range.first as u64 != range.absolute_begin % capacity as u64
        || before.end != range.first.wrapping_add(128)
        || first != before.first
        || first < range.first
        || first >= before.end
        || instances > before.end - first
        || after.first != first + instances
        || after.end != before.end
    {
        return None;
    }
    out.absolute_begin = range.absolute_begin.wrapping_add(first as u64).wrapping_sub(range.first as u64);
    out.reservation = *range;
    out.has_cache = 1;
    out.cache = CachedCursor { next: after, ..*cache };
    Some(out)
}

/// detail::InstanceLayout: slot 7 carries whole 32-bit per-instance elements (step 1) inside a 48- or 64-byte
/// stride, and at least one of them exists.
pub fn instance_layout(inputs: &[LayoutInput], stride: u32) -> bool {
    if inputs.is_empty() || inputs.len() > 32 || (stride != 48 && stride != 64) {
        return false;
    }
    let mut next = 0u32;
    let mut found = false;
    for input in inputs.iter().filter(|i| i.slot == 7) {
        let format = DXGI_FORMAT(input.format as i32);
        let bytes = if [DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32B32A32_UINT, DXGI_FORMAT_R32G32B32A32_SINT].contains(&format) {
            16
        } else if [DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32_UINT, DXGI_FORMAT_R32G32B32_SINT].contains(&format) {
            12
        } else if [DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32_UINT, DXGI_FORMAT_R32G32_SINT].contains(&format) {
            8
        } else if [DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32_SINT].contains(&format) {
            4
        } else {
            return false;
        };
        let at = if input.offset == D3D12_APPEND_ALIGNED_ELEMENT { next } else { input.offset };
        if input.classification != D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA.0 as u32 || input.step != 1 || at > stride || bytes > stride - at {
            return false;
        }
        next = at + bytes;
        found = true;
    }
    found
}

/// # Safety
/// `out` points to a reservation range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_instance_reservation_result(old: u64, capacity: u32, instances: u32, first: u32, after: u64, out: *mut ReservationRange) -> bool {
    let result = reservation_result(old, capacity, instances, first, after);
    // SAFETY: the caller's output.
    unsafe { *out = result.unwrap_or_default() };
    result.is_some()
}

/// # Safety
/// `actual` and `previous` are null or point to their values; `out` points to a writer range.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_instance_resolve_writer(
    address: u64,
    ring: u64,
    generation: u64,
    thread: u64,
    capacity: u32,
    before: Cursor,
    after: Cursor,
    first: u32,
    instances: u32,
    actual: *const ReservationRange,
    previous: *const CachedCursor,
    out: *mut WriterRange,
) -> bool {
    // SAFETY: the caller's optional inputs.
    let result = resolve_writer(
        address,
        ring,
        generation,
        thread,
        capacity,
        before,
        after,
        first,
        instances,
        unsafe { actual.as_ref() },
        unsafe { previous.as_ref() },
    );
    // SAFETY: the caller's output.
    unsafe { *out = result.unwrap_or_default() };
    result.is_some()
}

/// # Safety
/// `inputs` points to `count` layout inputs (or is null with a count of 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_instance_layout(inputs: *const LayoutInput, count: usize, stride: u32) -> bool {
    let inputs = if count == 0 || inputs.is_null() {
        &[][..]
    } else {
        // SAFETY: the caller's array.
        unsafe { core::slice::from_raw_parts(inputs, count) }
    };
    instance_layout(inputs, stride)
}
