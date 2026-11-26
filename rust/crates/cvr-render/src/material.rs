//! THE MATERIAL CONSTANTS' BINDING AND READ RULES (the pure parts of include/Render/NativeMaterialBinding.hpp and
//! include/Render/NativeMaterialRead.hpp): the engine binds the material's 292-byte constant buffer at b4 of the pixel
//! stage; the snapshot of that binding -- the recording context, the registry's buffer record of the handle (a
//! transient handle carries no resource fields), the bound state's descriptor-table slot and its CPU descriptor -- read
//! twice; whether a draw's CBV fits it; and, to read it, the native read state's copy mask (UPLOAD: GENERIC_READ; DEFAULT:
//! a tracked class with shader reads), the recovery mask, the native identity, the submission-time record (only the
//! registry and the saved handle) and the copy ranges. The headers forward to these in the Rust build;
//! tools/single_pass_tests/material_binding_test and material_read_test run their cases against them.
use core::ffi::c_void;

use windows::Win32::Graphics::Direct3D12::*;

use crate::buffer_read::NativeBuffer;
use crate::source_cbv_details::ReadState;
use crate::texture_lease::UploadContext;

pub const SHADER_REGISTER: u32 = 4;
pub const LOGICAL_BYTES: u64 = 292;

/// material_binding::Record.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Record {
    pub gpu: u64,
    pub resource: u64,
    pub descriptor: u64,
    pub bytes: u32,
    pub kind: u32,
}
/// material_binding::CpuSlot.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CpuSlot {
    pub layout: u64,
    pub table: u64,
    pub address: u64,
    pub descriptor: u64,
    pub parameter: u32,
    pub index: u32,
    pub table_offset: u32,
}
/// material_binding::Snapshot.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub context: UploadContext,
    pub registry: u64,
    pub record_address: u64,
    pub handle: u32,
    pub record: Record,
    pub slot: CpuSlot,
}
const _: () = assert!(
    core::mem::size_of::<Record>() == 32
        && core::mem::size_of::<CpuSlot>() == 48
        && core::mem::size_of::<Snapshot>() == 128
        && core::mem::offset_of!(Snapshot, record) == 48
        && core::mem::offset_of!(Snapshot, slot) == 80
);

/// geometry::MemoryReader.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;
/// The binding's guarded read (a non-null in-range address).
fn read(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: the caller's reader writes at most `out.len()` bytes.
    reader.is_some_and(|r| {
        at != 0
            && at <= u64::MAX - out.len() as u64
            && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
    })
}
/// The read rules' raw read (the reader only checked for presence by the caller).
fn read_raw(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: as above.
    reader.is_some_and(|r| unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) })
}
fn read_into<T: Copy>(reader: Reader, user: *mut c_void, at: u64, value: &mut T) -> bool {
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), core::mem::size_of::<T>())
    };
    read(reader, user, at, bytes)
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

// ---- material_binding::detail

/// ReadSnapshotOnce: the recording context still this list's, the registry's record of the handle (a constant buffer of
/// at least 292 bytes; a transient one without resource fields) and the b4 slot of the bound layout.
pub fn read_snapshot_once(
    reader: Reader,
    user: *mut c_void,
    game: u64,
    context: UploadContext,
    handle: u32,
) -> Option<Snapshot> {
    if game == 0
        || game > u64::MAX - 0x3BCDCB0
        || context.context == 0
        || context.state == 0
        || context.list == 0
        || context.context > u64::MAX - 0x68
        || context.state > u64::MAX - 0x70
        || handle == 0
        || handle > 0x100000
    {
        return None;
    }
    let mut next = Snapshot {
        context,
        handle,
        ..Default::default()
    };
    let (mut list, mut state) = (0u64, 0u64);
    if !read_into(reader, user, context.context + 0x30, &mut list)
        || !read_into(reader, user, context.context + 0x60, &mut state)
        || list != context.list
        || state != context.state
        || !read_into(reader, user, game + 0x3BCDCA8, &mut next.registry)
        || next.registry == 0
    {
        return None;
    }
    let offset = 0x5C0AF8 + (handle as u64 - 1) * 0xB0;
    if next.registry > u64::MAX - offset - 0x50 {
        return None;
    }
    next.record_address = next.registry + offset;
    let mut bytes = [0u8; 0x50];
    if !read(reader, user, next.record_address, &mut bytes) || bytes[6] & 15 != 6 {
        return None;
    }
    next.record.bytes = u32_at(&bytes, 0);
    next.record.kind = (bytes[6] >> 4) as u32;
    if (next.record.bytes as u64) < LOGICAL_BYTES {
        return None;
    }
    if next.record.kind != 4 {
        next.record.gpu = u64_at(&bytes, 8);
        next.record.resource = u64_at(&bytes, 16);
        next.record.descriptor = u64_at(&bytes, 0x48);
        let r = &next.record;
        if r.gpu == 0
            || r.gpu & 255 != 0
            || r.resource == 0
            || r.descriptor == 0
            || r.gpu > u64::MAX - r.bytes as u64
        {
            return None;
        }
    }
    let slot = &mut next.slot;
    if !read_into(reader, user, context.state + 0x68, &mut slot.layout)
        || !read_into(reader, user, context.state + 0x28, &mut slot.table)
        || slot.layout == 0
        || slot.table == 0
        || slot.layout > u64::MAX - 0x478
    {
        return None;
    }
    let mut parameter = 0u8;
    if !read_into(reader, user, slot.layout + 0x477, &mut parameter) || parameter >= 64 {
        return None;
    }
    slot.parameter = parameter as u32;
    let entry = slot.layout + 0x38 + parameter as u64 * 16;
    let (mut kind, mut at, mut first, mut count) = (0u32, 0u32, 0u16, 0u16);
    if !read_into(reader, user, entry, &mut kind)
        || kind != 2
        || !read_into(reader, user, entry + 4, &mut at)
        || !read_into(reader, user, entry + 8, &mut first)
        || !read_into(reader, user, entry + 10, &mut count)
        || first as u32 > SHADER_REGISTER
        || SHADER_REGISTER - first as u32 >= count as u32
    {
        return None;
    }
    slot.table_offset = SHADER_REGISTER - first as u32;
    if at >= 8192 || slot.table_offset >= 8192 - at {
        return None;
    }
    slot.index = at + slot.table_offset;
    if slot.table > u64::MAX - slot.index as u64 * 8 - 8 {
        return None;
    }
    slot.address = slot.table + slot.index as u64 * 8;
    if !read_into(reader, user, slot.address, &mut slot.descriptor)
        || slot.descriptor == 0
        || (next.record.kind != 4 && slot.descriptor != next.record.descriptor)
    {
        return None;
    }
    Some(next)
}
/// ReadSnapshot: two equal snapshots.
pub fn read_snapshot(
    reader: Reader,
    user: *mut c_void,
    game: u64,
    context: UploadContext,
    handle: u32,
) -> Option<Snapshot> {
    let a = read_snapshot_once(reader, user, game, context, handle)?;
    let b = read_snapshot_once(reader, user, game, context, handle)?;
    (a == b).then_some(a)
}
/// FitsCbv: a 256-aligned CBV of 292..=65536 bytes at the record's own address (any address for a transient one).
pub fn fits_cbv(source: &Snapshot, address: u64, bytes: u32) -> bool {
    if source.handle == 0
        || (source.record.bytes as u64) < LOGICAL_BYTES
        || address == 0
        || address & 255 != 0
        || (bytes as u64) < LOGICAL_BYTES
        || bytes > 65536
        || bytes & 255 != 0
        || address > u64::MAX - LOGICAL_BYTES
    {
        return false;
    }
    source.record.kind == 4
        || (source.record.gpu == address
            && source.record.resource != 0
            && source.record.descriptor == source.slot.descriptor)
}
#[allow(clippy::too_many_arguments)]
pub fn matches_draw(
    saved: &Snapshot,
    current: &Snapshot,
    parameter: u32,
    offset: u32,
    saved_address: u64,
    saved_bytes: u32,
    actual_address: u64,
    actual_bytes: u32,
) -> bool {
    saved == current
        && current.slot.parameter == parameter
        && current.slot.table_offset == offset
        && saved_address == actual_address
        && saved_bytes == actual_bytes
        && fits_cbv(current, actual_address, actual_bytes)
}
pub fn native(source: &Snapshot) -> NativeBuffer {
    if source.record.kind == 4 {
        return NativeBuffer::default();
    }
    NativeBuffer {
        resource: source.record.resource,
        gpu: source.record.gpu,
        bytes: source.record.bytes as u64,
        registry: source.registry,
        context: source.context.context,
        handle: source.handle,
        kind: source.record.kind,
    }
}

// ---- material_read::detail

const SHADER_READ: u32 = D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 as u32;
const COPY_READ: u32 = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
const ALLOWED_READS: u32 = SHADER_READ
    | D3D12_RESOURCE_STATE_INDEX_BUFFER.0 as u32
    | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32
    | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 as u32
    | D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT.0 as u32
    | COPY_READ;
const UPLOAD: u32 = D3D12_HEAP_TYPE_UPLOAD.0 as u32;
const DEFAULT: u32 = D3D12_HEAP_TYPE_DEFAULT.0 as u32;

pub fn tracked_class(kind: u32) -> bool {
    kind < 16 && kind != 1 && kind != 2 && kind != 4 && kind != 5
}
/// CopyReadMask: the state to register for the copy (UPLOAD: GENERIC_READ; DEFAULT: a tracked class holding shader
/// reads and only read bits).
pub fn copy_read_mask(state: ReadState, heap: u32, kind: u32) -> Option<u32> {
    if state.kind != kind || kind >= 16 || kind == 4 {
        return None;
    }
    if heap == UPLOAD {
        if state.status != 1 || state.value != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32 {
            return None;
        }
    } else if heap != DEFAULT
        || !tracked_class(kind)
        || state.status != 2
        || state.value & SHADER_READ == 0
        || state.value & !ALLOWED_READS != 0
    {
        return None;
    }
    Some(state.value | SHADER_READ | COPY_READ)
}
pub fn has_read_state(state: ReadState, heap: u32, kind: u32, required: u32) -> bool {
    required != 0
        && required & !ALLOWED_READS == 0
        && required & SHADER_READ != 0
        && copy_read_mask(state, heap, kind).is_some()
        && state.value & required == required
}
/// RecoveryReadMask: after an attempted native mutation, a known tracked read state keeping every read from before.
pub fn recovery_read_mask(state: ReadState, kind: u32, previous: u32) -> Option<u32> {
    if !tracked_class(kind)
        || state.kind != kind
        || state.status != 2
        || state.value & !ALLOWED_READS != 0
        || previous & SHADER_READ == 0
        || previous & !ALLOWED_READS != 0
    {
        return None;
    }
    Some(state.value | previous | SHADER_READ)
}
pub fn native_identity(saved: &Snapshot, current: &NativeBuffer) -> bool {
    let r = &saved.record;
    saved.handle != 0
        && saved.handle <= 0x100000
        && saved.registry != 0
        && saved.context.context != 0
        && saved.context.state != 0
        && saved.context.list != 0
        && r.kind != 4
        && r.kind < 16
        && r.resource != 0
        && r.gpu != 0
        && r.bytes as u64 >= LOGICAL_BYTES
        && current.resource == r.resource
        && current.gpu == r.gpu
        && current.bytes == r.bytes as u64
        && current.registry == saved.registry
        && current.context == saved.context.context
        && current.handle == saved.handle
        && current.kind == r.kind
}
pub fn submission_heap(heap: u32, kind: u32) -> bool {
    kind < 16 && kind != 4 && (heap == UPLOAD || (heap == DEFAULT && tracked_class(kind)))
}
pub fn needs_buffer_receipt(heap: u32, kind: u32) -> bool {
    heap == DEFAULT && kind == 1
}
/// SubmissionRecordOnce: only the global registry and the saved handle -- the record still the saved one.
pub fn submission_record_once(
    reader: Reader,
    user: *mut c_void,
    game: u64,
    saved: &Snapshot,
) -> bool {
    let r = &saved.record;
    if reader.is_none()
        || game == 0
        || game > u64::MAX - 0x3BCDCB0
        || saved.registry == 0
        || saved.handle == 0
        || saved.handle > 0x100000
        || r.kind >= 16
        || r.kind == 4
        || r.resource == 0
        || r.gpu == 0
        || r.gpu & 255 != 0
        || (r.bytes as u64) < LOGICAL_BYTES
        || r.gpu > u64::MAX - r.bytes as u64
        || r.descriptor == 0
        || r.descriptor != saved.slot.descriptor
    {
        return false;
    }
    let mut registry = [0u8; 8];
    if !read_raw(reader, user, game + 0x3BCDCA8, &mut registry)
        || u64::from_le_bytes(registry) != saved.registry
    {
        return false;
    }
    let registry = saved.registry;
    let offset = 0x5C0AF8 + (saved.handle as u64 - 1) * 0xB0;
    if registry > u64::MAX - offset - 0x50 || saved.record_address != registry + offset {
        return false;
    }
    let mut bytes = [0u8; 0x50];
    if !read_raw(reader, user, registry + offset, &mut bytes) || bytes[6] & 15 != 6 {
        return false;
    }
    let current = Record {
        bytes: u32_at(&bytes, 0),
        kind: (bytes[6] >> 4) as u32,
        gpu: u64_at(&bytes, 8),
        resource: u64_at(&bytes, 16),
        descriptor: u64_at(&bytes, 0x48),
    };
    current == *r
}
pub fn submission_record(reader: Reader, user: *mut c_void, game: u64, saved: &Snapshot) -> bool {
    submission_record_once(reader, user, game, saved)
        && submission_record_once(reader, user, game, saved)
}
pub fn source_range(address: u64, gpu: u64, bytes: u64, offset: u64) -> bool {
    address != 0
        && gpu != 0
        && gpu <= u64::MAX - bytes
        && address >= gpu
        && address - gpu == offset
        && offset <= bytes
        && LOGICAL_BYTES <= bytes - offset
}
pub fn destination_range(bytes: u64, offset: u64) -> bool {
    offset <= bytes && LOGICAL_BYTES <= bytes - offset
}

// ---- the C ABI (the headers' inline functions in the Rust build)

/// # Safety
/// `context` and `out` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_read_snapshot(
    reader: Reader,
    user: *mut c_void,
    game: u64,
    context: *const UploadContext,
    handle: u32,
    out: *mut Snapshot,
    once: bool,
) -> bool {
    // SAFETY: as the caller promises.
    let context = unsafe { *context };
    let value = if once {
        read_snapshot_once(reader, user, game, context, handle)
    } else {
        read_snapshot(reader, user, game, context, handle)
    };
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `source` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_fits_cbv(
    source: *const Snapshot,
    address: u64,
    bytes: u32,
) -> bool {
    // SAFETY: as the caller promises.
    fits_cbv(unsafe { &*source }, address, bytes)
}
/// # Safety
/// The snapshots valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_mat_matches_draw(
    saved: *const Snapshot,
    current: *const Snapshot,
    parameter: u32,
    offset: u32,
    saved_address: u64,
    saved_bytes: u32,
    actual_address: u64,
    actual_bytes: u32,
) -> bool {
    // SAFETY: as the caller promises.
    matches_draw(
        unsafe { &*saved },
        unsafe { &*current },
        parameter,
        offset,
        saved_address,
        saved_bytes,
        actual_address,
        actual_bytes,
    )
}
/// # Safety
/// `source` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_native(source: *const Snapshot, out: *mut NativeBuffer) {
    // SAFETY: as the caller promises.
    unsafe { *out = native(&*source) };
}
#[no_mangle]
pub extern "C" fn cvr_mat_tracked_class(kind: u32) -> bool {
    tracked_class(kind)
}
/// # Safety
/// `state` and `required` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_copy_read_mask(
    state: *const ReadState,
    heap: u32,
    kind: u32,
    required: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = copy_read_mask(unsafe { *state }, heap, kind);
    unsafe { *required = value.unwrap_or(0) };
    value.is_some()
}
/// # Safety
/// `state` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_has_read_state(
    state: *const ReadState,
    heap: u32,
    kind: u32,
    required: u32,
) -> bool {
    // SAFETY: as the caller promises.
    has_read_state(unsafe { *state }, heap, kind, required)
}
/// # Safety
/// `state` and `required` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_recovery_read_mask(
    state: *const ReadState,
    kind: u32,
    previous: u32,
    required: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = recovery_read_mask(unsafe { *state }, kind, previous);
    unsafe { *required = value.unwrap_or(0) };
    value.is_some()
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_native_identity(
    saved: *const Snapshot,
    current: *const NativeBuffer,
) -> bool {
    // SAFETY: as the caller promises.
    native_identity(unsafe { &*saved }, unsafe { &*current })
}
#[no_mangle]
pub extern "C" fn cvr_mat_submission_heap(heap: u32, kind: u32) -> bool {
    submission_heap(heap, kind)
}
#[no_mangle]
pub extern "C" fn cvr_mat_needs_buffer_receipt(heap: u32, kind: u32) -> bool {
    needs_buffer_receipt(heap, kind)
}
/// # Safety
/// `saved` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_mat_submission_record(
    reader: Reader,
    user: *mut c_void,
    game: u64,
    saved: *const Snapshot,
    once: bool,
) -> bool {
    // SAFETY: as the caller promises.
    let saved = unsafe { &*saved };
    if once {
        submission_record_once(reader, user, game, saved)
    } else {
        submission_record(reader, user, game, saved)
    }
}
#[no_mangle]
pub extern "C" fn cvr_mat_source_range(address: u64, gpu: u64, bytes: u64, offset: u64) -> bool {
    source_range(address, gpu, bytes, offset)
}
#[no_mangle]
pub extern "C" fn cvr_mat_destination_range(bytes: u64, offset: u64) -> bool {
    destination_range(bytes, offset)
}
