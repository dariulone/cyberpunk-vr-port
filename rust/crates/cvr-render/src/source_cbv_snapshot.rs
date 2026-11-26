//! The source CBV snapshot's rules (`include/Render/NativeSourceCbvSnapshot.hpp`, `source_cbv_snapshot::detail`): a CBV
//! source's admission by its observed class (an exact 2.5 upload -- the direct VS b5 creator 0x3362A0 or the transient
//! writer 0x3346B1 --, a class-1 pool slot of at most 512 bytes, or a directly placed class-3 buffer whose allocation,
//! heap and padded extent match), its range inside the backing and the native view, the read and recovery state masks,
//! the private arena's 256-aligned slices (at most 28 inside 64 MiB), the original read epoch's certificate, the
//! recorded copy fields, the private CBV identity and its recorded extent, and the inline upload routes inheriting the
//! original epoch (direct, or transient through its completed writer).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions forward to `cvr_scs_*`; the records are mirrored here (the
//! COM pointers read as addresses, never retained). `tools/single_pass_tests/source_cbv_snapshot_test` runs its cases
//! against them in its Rust variant.

use crate::buffer_read::NativeBuffer;
use crate::indirect_copy::ReadState;
use crate::source_cbv_binding::DescriptorStamp;

pub const MAX_REQUESTS: usize = 28;
pub const MAX_ARENA_BYTES: u64 = 64 * 1024 * 1024;
const VERTEX_AND_CONSTANT_BUFFER: u32 = 0x1;
const COPY_SOURCE: u32 = 0x800;
const GENERIC_READ: u32 = 0xAC3;
const COMMON: u32 = 0;
const HEAP_DEFAULT: u32 = 1;
const HEAP_UPLOAD: u32 = 2;
const DIRECT_CREATOR_RVA: u32 = 0x3362A0;
const DIRECT_MEMCPY_RETURN_RVA: u32 = 0x33626F;
const POOLED_CREATOR_RVA: u32 = 0x3346B1;
const UPLOAD_WRITER_MEMCPY_RETURN_RVA: u32 = 0x33435E;

/// source_cbv_snapshot::Failure (the values Admit returns).
pub mod failure {
    pub const NONE: u32 = 0;
    pub const SOURCE: u32 = 6;
    pub const CREATION: u32 = 7;
    pub const CLASS: u32 = 8;
    pub const STATE: u32 = 9;
    pub const ALLOCATION: u32 = 10;
    pub const POOL: u32 = 11;
    pub const RANGE: u32 = 12;
}
/// detail::Kind.
pub mod kind {
    pub const UNKNOWN: u32 = 0;
    pub const UPLOAD: u32 = 1;
    pub const POOL: u32 = 2;
    pub const TRACKED: u32 = 3;
}

/// detail::Admission.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Admission {
    pub resource: u64,
    pub gpu: u64,
    pub width: u64,
    pub com_offset: u64,
    pub native_gpu: u64,
    pub native_bytes: u64,
    pub creation_heap: u64,
    pub creation_bytes: u64,
    pub creation_offset: u64,
    pub allocation: u64,
    pub allocation_resource: u64,
    pub allocation_bytes: u64,
    pub allocation_heap: u64,
    pub allocator_pimpl: u64,
    pub allocation_pimpl: u64,
    pub reflected_bytes: u32,
    pub view_bytes: u32,
    pub heap: u32,
    pub flags: u32,
    pub native_kind: u32,
    pub handle: u32,
    pub creator: u32,
    pub creation_kind: u32,
    pub initial_state: u32,
    pub allocation_kind: u32,
    pub state: ReadState,
    pub source_current: bool,
    pub creation_known: bool,
    pub initial_known: bool,
    pub detail_known: bool,
    pub state_known: bool,
    pub pool_known: bool,
    pub allocation_known: bool,
    pub allocator_known: bool,
    pub direct: bool,
    pub alias_fields_empty: bool,
}
/// source_cbv_snapshot::ReadEpochCertificate.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadEpochCertificate {
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub certified: bool,
}
/// source_cbv_snapshot::PrivateCbv (the COM pointers as addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PrivateCbv {
    pub parameter: u32,
    pub offset: u32,
    pub reflected_bytes: u32,
    pub padded_bytes: u32,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub view_location: u64,
    pub view_bytes: u32,
    pub resource: u64,
    pub owner: u64,
}
/// source_cbv_snapshot::PreparedCopies (the owner as an address).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PreparedCopies {
    pub owner: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub copied_bytes: u64,
    pub count: u32,
}
/// detail::PrivateCbvIdentity.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateCbvIdentity {
    pub resource: u64,
    pub owner: u64,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub gpu: u64,
    pub parameter: u32,
    pub offset: u32,
    pub reflected_bytes: u32,
    pub padded_bytes: u32,
    pub view_bytes: u32,
}
/// native_recording::Lease (the token as an address).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordingLease {
    pub token: u64,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
/// source_cbv::SourceCbvBacking (the COM pointers as addresses).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SourceCbvBacking {
    pub resource: u64,
    pub origin_owner: u64,
    pub stamp: DescriptorStamp,
    pub recording: RecordingLease,
    pub native: NativeBuffer,
    pub cookie: u64,
    pub origin: u64,
    pub gpu: u64,
    pub backing_bytes: u64,
    pub source_offset: u64,
    pub serial: u64,
    pub cpu_descriptor: u64,
    pub descriptor_bytes: u32,
    pub logical_bytes: u32,
    pub stage: u32,
    pub shader_register: u32,
    pub creator_rva: u32,
    pub reserved_bytes: u32,
    pub completed_producer_rva: u32,
    pub creation_recording: RecordingLease,
    pub writer_nonce: u64,
    pub writer_destination: u64,
    pub writer_bytes: u32,
    pub writer_recording: RecordingLease,
}
const _: () = assert!(
    core::mem::size_of::<Admission>() == 184
        && core::mem::size_of::<ReadEpochCertificate>() == 32
        && core::mem::size_of::<PrivateCbv>() == 72
        && core::mem::size_of::<PreparedCopies>() == 48
        && core::mem::size_of::<PrivateCbvIdentity>() == 72
        && core::mem::size_of::<RecordingLease>() == 40
        && core::mem::size_of::<SourceCbvBacking>() == 472
        && core::mem::offset_of!(SourceCbvBacking, recording) == 192
        && core::mem::offset_of!(SourceCbvBacking, native) == 232
        && core::mem::offset_of!(SourceCbvBacking, descriptor_bytes) == 336
        && core::mem::offset_of!(SourceCbvBacking, creation_recording) == 368
        && core::mem::offset_of!(SourceCbvBacking, writer_recording) == 432
);

/// detail::Range.
pub fn range(a: &Admission) -> bool {
    if a.resource == 0
        || a.gpu == 0
        || a.width == 0
        || a.gpu > u64::MAX - a.width
        || a.reflected_bytes == 0
        || a.reflected_bytes > 65536
        || a.view_bytes == 0
        || a.view_bytes > 65536
        || a.view_bytes & 255 != 0
        || a.reflected_bytes > a.view_bytes
        || a.com_offset > a.width
        || a.view_bytes as u64 > a.width - a.com_offset
        || (a.gpu + a.com_offset) & 255 != 0
    {
        return false;
    }
    if a.handle != 0 && a.native_kind == 4 {
        return a.native_bytes != 0 && a.reflected_bytes as u64 <= a.native_bytes;
    }
    let at = a.gpu + a.com_offset;
    a.handle == 0
        || (a.native_gpu != 0
            && a.native_bytes != 0
            && a.native_gpu >= a.gpu
            && a.native_gpu - a.gpu <= a.width
            && a.native_bytes <= a.width - (a.native_gpu - a.gpu)
            && at >= a.native_gpu
            && at - a.native_gpu <= a.native_bytes
            && a.reflected_bytes as u64 <= a.native_bytes - (at - a.native_gpu))
}
/// detail::ReadMask.
pub fn read_mask(state: ReadState, copy: bool) -> bool {
    state.status == 2
        && state.kind == 3
        && state.value & VERTEX_AND_CONSTANT_BUFFER != 0
        && state.value & !GENERIC_READ == 0
        && (!copy || state.value & COPY_SOURCE != 0)
}
/// detail::RecoveryMask: the required state, if any.
pub fn recovery_mask(previous: u32, current: ReadState) -> Option<u32> {
    if previous & VERTEX_AND_CONSTANT_BUFFER == 0
        || previous & !GENERIC_READ != 0
        || current.status != 2
        || current.kind != 3
        || current.value & !GENERIC_READ != 0
    {
        return None;
    }
    Some(previous | current.value | VERTEX_AND_CONSTANT_BUFFER)
}
/// detail::Admit: (failure, kind).
pub fn admit(a: &Admission) -> (u32, u32) {
    let refuse = |failure: u32| (failure, kind::UNKNOWN);
    if !a.source_current || !a.detail_known {
        return refuse(failure::SOURCE);
    }
    if !a.creation_known || !a.initial_known {
        return refuse(failure::CREATION);
    }
    if a.flags != 0 || !a.state_known {
        return refuse(failure::STATE);
    }
    if !range(a) {
        return refuse(failure::RANGE);
    }
    if a.heap == HEAP_UPLOAD {
        if a.creation_kind != 1
            || a.creation_heap != 0
            || a.initial_state != GENERIC_READ
            || a.state.status != 1
            || a.state.value != GENERIC_READ
            || !((a.handle == 0 && a.creator == DIRECT_CREATOR_RVA) || (a.handle != 0 && a.native_kind == 4 && a.creator == POOLED_CREATOR_RVA))
        {
            return refuse(failure::CLASS);
        }
        return (failure::NONE, kind::UPLOAD);
    }
    if a.heap != HEAP_DEFAULT || a.handle == 0 || (a.native_kind != 1 && a.native_kind != 3) {
        return refuse(failure::CLASS);
    }
    if !a.allocation_known
        || !a.allocator_known
        || a.allocation == 0
        || a.allocation_resource != a.resource
        || a.allocation_bytes < a.width
        || a.allocation_pimpl == 0
        || a.allocator_pimpl != a.allocation_pimpl
    {
        return refuse(failure::ALLOCATION);
    }
    if a.native_kind == 1 {
        if a.creation_kind != 1
            || a.creation_heap != 0
            || a.allocation_kind != 0
            || a.allocation_heap != 0
            || !a.pool_known
            || a.native_bytes > 512
            || (a.initial_state != COMMON && a.initial_state != GENERIC_READ)
            || (a.state.status != 8 && a.state.status != 9)
            || a.state.kind != 1
            || a.state.value != GENERIC_READ
        {
            return refuse(failure::POOL);
        }
        return (failure::NONE, kind::POOL);
    }
    if a.creation_kind != 2
        || a.allocation_kind != 1
        || a.creation_heap == 0
        || a.creation_heap != a.allocation_heap
        || a.creation_bytes == 0
        || a.creation_bytes != a.allocation_bytes
        || a.creation_offset > u64::MAX - a.creation_bytes
        || !a.direct
        || !a.alias_fields_empty
        || a.initial_state != COMMON
        || a.com_offset != 0
        || a.native_gpu != a.gpu
        || a.view_bytes as u64 != a.width
        || a.native_bytes > u64::MAX - 255
        || (a.native_bytes + 255) & !255 != a.width
    {
        return refuse(failure::ALLOCATION);
    }
    if !read_mask(a.state, false) {
        return refuse(failure::STATE);
    }
    (failure::NONE, kind::TRACKED)
}
/// detail::Reserve: the slices' offsets on success, and the end reached (written on refusal too, as in C++).
pub fn reserve(cursor: u64, capacity: u64, views: &[u32]) -> (Option<Vec<u64>>, u64) {
    let mut end = cursor;
    if capacity == 0 || capacity > MAX_ARENA_BYTES || cursor > capacity || cursor & 255 != 0 || views.is_empty() || views.len() > MAX_REQUESTS {
        return (None, end);
    }
    let mut next = Vec::with_capacity(views.len());
    for &bytes in views {
        if bytes == 0 || bytes > 65536 || bytes & 255 != 0 || bytes as u64 > capacity - end {
            return (None, end);
        }
        next.push(end);
        end += bytes as u64;
    }
    (Some(next), end)
}
/// detail::Epoch.
pub fn epoch(epoch: &ReadEpochCertificate, list: u64, generation: u64, occurrence: u64) -> bool {
    epoch.certified
        && list != 0
        && generation != 0
        && occurrence != 0
        && epoch.list == list
        && epoch.generation == generation
        && epoch.occurrence == occurrence
}
/// detail::RecordedCopyFields.
pub fn recorded_copy_fields(proof: &PreparedCopies, certificate: &ReadEpochCertificate, copied: u64, count: usize) -> bool {
    epoch(certificate, proof.list, proof.generation, proof.occurrence)
        && copied != 0
        && count != 0
        && count <= MAX_REQUESTS
        && proof.copied_bytes == copied
        && proof.count as usize == count
}
/// detail::PrivateIdentity.
pub fn private_identity(view: &PrivateCbv) -> PrivateCbvIdentity {
    PrivateCbvIdentity {
        resource: view.resource,
        owner: view.owner,
        source_offset: view.source_offset,
        private_offset: view.private_offset,
        copied_bytes: view.copied_bytes,
        gpu: view.view_location,
        parameter: view.parameter,
        offset: view.offset,
        reflected_bytes: view.reflected_bytes,
        padded_bytes: view.padded_bytes,
        view_bytes: view.view_bytes,
    }
}
/// detail::RecordedPrivateIdentity: numeric equality and extent only.
pub fn recorded_private_identity(
    actual: &PrivateCbvIdentity,
    saved: &PrivateCbvIdentity,
    resource: u64,
    owner: u64,
    gpu: u64,
    cursor: u64,
    capacity: u64,
) -> bool {
    actual == saved
        && resource != 0
        && owner != 0
        && gpu != 0
        && capacity != 0
        && capacity <= MAX_ARENA_BYTES
        && cursor != 0
        && cursor <= capacity
        && gpu <= u64::MAX - capacity
        && actual.resource == resource
        && actual.owner == owner
        && actual.parameter < 64
        && actual.reflected_bytes != 0
        && actual.reflected_bytes <= 65536
        && actual.copied_bytes == actual.reflected_bytes as u64
        && actual.padded_bytes != 0
        && actual.padded_bytes <= 65536
        && actual.padded_bytes & 255 == 0
        && actual.view_bytes == actual.padded_bytes
        && actual.reflected_bytes <= actual.padded_bytes
        && actual.private_offset & 255 == 0
        && actual.private_offset <= cursor
        && actual.padded_bytes as u64 <= cursor - actual.private_offset
        && actual.gpu == gpu + actual.private_offset
}
fn padded(source: &SourceCbvBacking) -> bool {
    source.reserved_bytes == source.descriptor_bytes && source.reserved_bytes as u64 == ((source.logical_bytes as u64 + 255) & !255)
}
fn same_recording(a: &RecordingLease, original: &RecordingLease) -> bool {
    a.token == original.token && a.list == original.list && a.generation == original.generation && a.context == original.context && a.registry == original.registry
}
/// detail::InlineDirectUpload.
pub fn inline_direct_upload(source: &SourceCbvBacking, certificate: &ReadEpochCertificate, bytes: u32) -> bool {
    let (created, original) = (&source.creation_recording, &source.recording);
    source.creator_rva == DIRECT_CREATOR_RVA
        && source.completed_producer_rva == DIRECT_MEMCPY_RETURN_RVA
        && bytes != 0
        && bytes <= source.logical_bytes
        && source.logical_bytes <= 65536
        && padded(source)
        && created.token != 0
        && original.token != 0
        && same_recording(created, original)
        && epoch(certificate, original.list, original.generation, certificate.occurrence)
}
/// detail::InlineTransientUpload.
pub fn inline_transient_upload(source: &SourceCbvBacking, certificate: &ReadEpochCertificate, bytes: u32) -> bool {
    let (created, written, original) = (&source.creation_recording, &source.writer_recording, &source.recording);
    source.creator_rva == POOLED_CREATOR_RVA
        && source.completed_producer_rva == UPLOAD_WRITER_MEMCPY_RETURN_RVA
        && source.native.kind == 4
        && source.native.handle != 0
        && source.writer_nonce != 0
        && source.writer_destination != 0
        && bytes != 0
        && bytes <= source.writer_bytes
        && source.writer_bytes <= source.logical_bytes
        && source.logical_bytes <= 65536
        && padded(source)
        && created.token != 0
        && written.token != 0
        && original.token != 0
        && same_recording(created, original)
        && same_recording(written, original)
        && epoch(certificate, original.list, original.generation, certificate.occurrence)
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Range.
///
/// # Safety
/// `a` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_range(a: *const Admission) -> bool {
    range(unsafe { &*a })
}
/// ReadMask.
///
/// # Safety
/// `state` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_read_mask(state: *const ReadState, copy: bool) -> bool {
    read_mask(unsafe { *state }, copy)
}
/// RecoveryMask (`required` always written).
///
/// # Safety
/// `current` is valid; `required` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_recovery_mask(previous: u32, current: *const ReadState, required: *mut u32) -> bool {
    let result = recovery_mask(previous, unsafe { *current });
    unsafe { *required = result.unwrap_or(0) };
    result.is_some()
}
/// Admit: the failure (`kind` always written).
///
/// # Safety
/// `a` is valid; `kind` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_admit(a: *const Admission, kind: *mut u32) -> u32 {
    let (failure, value) = admit(unsafe { &*a });
    unsafe { *kind = value };
    failure
}
/// Reserve: `offsets` (room for MAX_REQUESTS) and `count` written on success, `end` always.
///
/// # Safety
/// `views` holds `view_count` values (or is null with none); `offsets`, `count` and `end` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_reserve(
    cursor: u64,
    capacity: u64,
    views: *const u32,
    view_count: usize,
    offsets: *mut u64,
    count: *mut usize,
    end: *mut u64,
) -> bool {
    let views = if views.is_null() || view_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(views, view_count) } };
    let (result, reached) = reserve(cursor, capacity, views);
    unsafe {
        *end = reached;
        *count = 0;
    }
    let Some(next) = result else { return false };
    unsafe {
        core::ptr::copy_nonoverlapping(next.as_ptr(), offsets, next.len());
        *count = next.len();
    }
    true
}
/// Epoch.
///
/// # Safety
/// `certificate` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_epoch(certificate: *const ReadEpochCertificate, list: u64, generation: u64, occurrence: u64) -> bool {
    epoch(unsafe { &*certificate }, list, generation, occurrence)
}
/// RecordedCopyFields.
///
/// # Safety
/// `proof` and `certificate` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_recorded_copy_fields(
    proof: *const PreparedCopies,
    certificate: *const ReadEpochCertificate,
    copied: u64,
    count: usize,
) -> bool {
    unsafe { recorded_copy_fields(&*proof, &*certificate, copied, count) }
}
/// PrivateIdentity.
///
/// # Safety
/// `view` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_private_identity(view: *const PrivateCbv, out: *mut PrivateCbvIdentity) {
    unsafe { *out = private_identity(&*view) };
}
/// RecordedPrivateIdentity.
///
/// # Safety
/// `actual` and `saved` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_recorded_private_identity(
    actual: *const PrivateCbvIdentity,
    saved: *const PrivateCbvIdentity,
    resource: u64,
    owner: u64,
    gpu: u64,
    cursor: u64,
    capacity: u64,
) -> bool {
    unsafe { recorded_private_identity(&*actual, &*saved, resource, owner, gpu, cursor, capacity) }
}
/// InlineDirectUpload (`which` 0), InlineTransientUpload (1), InlineUpload (2: either).
///
/// # Safety
/// `source` and `certificate` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scs_inline_upload(
    which: u32,
    source: *const SourceCbvBacking,
    certificate: *const ReadEpochCertificate,
    bytes: u32,
) -> bool {
    let (source, certificate) = unsafe { (&*source, &*certificate) };
    match which {
        0 => inline_direct_upload(source, certificate, bytes),
        1 => inline_transient_upload(source, certificate, bytes),
        _ => inline_direct_upload(source, certificate, bytes) || inline_transient_upload(source, certificate, bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_keeps_the_partial_end() {
        let (ok, end) = reserve(256, 1792, &[256, 512, 1024]);
        assert!(ok.is_none());
        assert_eq!(end, 1024);
        let (ok, end) = reserve(256, 2048, &[256, 512, 1024]);
        assert_eq!(ok.unwrap(), vec![256, 512, 1024]);
        assert_eq!(end, 2048);
    }
}
