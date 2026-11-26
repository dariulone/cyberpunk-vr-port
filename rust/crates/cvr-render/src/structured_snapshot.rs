//! THE STRUCTURED BUFFER SNAPSHOTS' RULES (the pure part of include/Render/NativeStructuredBufferSnapshot.hpp's detail
//! namespace): a 1 MiB dedicated committed structured buffer the engine binds at stage 1, t5 (root parameter 23, table
//! offset 5) may be copied into a private arena -- the admission of its source, creation, backing, view, range and
//! native read state; the copy extent (a selected range of at most 4 KiB, or the whole original view kept at its
//! original indices); the arena reservations (dense, or at the original indices); the reuse of a recorded full version
//! inside the same recording's no-write window; the recovery identity and the read epoch. The header forwards to these
//! in the Rust build; tools/single_pass_tests/structured_buffer_snapshot_test runs its cases against them (its Rust
//! variant).
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

use crate::buffer_read::NativeBuffer;
use crate::source_cbv_details::{Context, ReadState, KNOWN};

pub const SOURCE_BYTES: u64 = 1024 * 1024;
pub const STRIDE: u64 = 128;
pub const MAX_COPY_BYTES: u64 = 4096;
pub const MAX_ARENA_BYTES: u64 = 8 * 1024 * 1024;
pub const ALIGNMENT: u64 = 16;
pub const ALLOWED_HEAP_FLAGS: u32 =
    (D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES.0 | D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES.0) as u32;
// source_srv's binding.
const SHADER_REGISTER: u32 = 5;
const STAGE_INDEX: u32 = 1;
const ROOT_PARAMETER: u32 = 23;
const TABLE_OFFSET: u32 = 5;
const MAX_HANDLES: u32 = 0x100000;
// Extent / Layout.
pub const SELECTED_RANGE: u32 = 0;
pub const FULL_ORIGINAL_VIEW: u32 = 1;
pub const DENSE: u32 = 0;
pub const ORIGINAL_INDICES: u32 = 1;
/// structured_buffer_snapshot::Failure.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    None,
    Disabled,
    Arguments,
    Scope,
    Code,
    Parent,
    Source,
    Creation,
    Backing,
    State,
    Range,
}

/// detail::Admission.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Admission {
    pub resource: u64,
    pub gpu: u64,
    pub native_resource: u64,
    pub native_gpu: u64,
    pub native_bytes: u64,
    pub view_offset: u64,
    pub view_bytes: u64,
    pub source_offset: u64,
    pub bytes: u64,
    pub desc: D3D12_RESOURCE_DESC,
    pub heap: D3D12_HEAP_PROPERTIES,
    pub heap_flags: u32,
    pub creation_kind: u32,
    pub creation_heap: u64,
    pub creation_offset: u64,
    pub creation_bytes: u64,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub state: ReadState,
    pub native_kind: u32,
    pub handle: u32,
    pub stage: u32,
    pub shader_register: u32,
    pub parameter: u32,
    pub offset: u32,
    pub source_current: bool,
    pub creation_known: bool,
    pub unprotected: bool,
    pub same_device: bool,
    pub recording_current: bool,
    pub extent: u32,
    pub layout: u32,
}
/// detail::FullVersion.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FullVersion {
    pub list: u64,
    pub generation: u64,
    pub resource: u64,
    pub gpu: u64,
    pub destination: u64,
    pub occurrence: u64,
    pub recorded: bool,
}
/// copy_windows::Span.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Span {
    pub resource: u64,
    pub gpu: u64,
    pub backing: u64,
    pub offset: u64,
    pub bytes: u64,
    pub destination: u64,
    pub occurrence: u64,
}
/// ReadEpochCertificate.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ReadEpoch {
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub certified: bool,
}
const _: () = assert!(
    core::mem::size_of::<Admission>() == 280
        && core::mem::offset_of!(Admission, desc) == 72
        && core::mem::offset_of!(Admission, view) == 184
        && core::mem::offset_of!(Admission, state) == 224
        && core::mem::offset_of!(Admission, source_current) == 260
        && core::mem::size_of::<FullVersion>() == 56
        && core::mem::size_of::<Span>() == 56
        && core::mem::size_of::<ReadEpoch>() == 32
);

/// detail::NoProtectedSession.
pub fn no_protected_session(result: i32, present: bool) -> bool {
    !present
        && (result == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND.0
            || result == windows::Win32::Foundation::E_NOINTERFACE.0
            || result >= 0)
}
const GENERIC_READ: u32 = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
/// detail::ReadMask: a buffer of kind 0 or 3 tracked per subresource, readable by pixel shaders, only generic-read bits.
pub fn read_mask(state: ReadState, kind: u32, ready: bool) -> bool {
    (kind == 0 || kind == 3)
        && state.kind == kind
        && state.status == 2
        && state.value & D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 as u32 != 0
        && state.value & !GENERIC_READ == 0
        && (!ready || state.value & D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 != 0)
}
pub fn recovery_mask(previous: u32, current: ReadState, kind: u32) -> Option<u32> {
    if !read_mask(
        ReadState {
            value: previous,
            status: 2,
            kind,
        },
        kind,
        false,
    ) || current.status != 2
        || current.kind != kind
        || current.value & !GENERIC_READ != 0
    {
        return None;
    }
    Some(previous | current.value)
}
/// detail::RecoveryIdentity: a pending barrier may be flushed only while the saved context and record still match.
#[allow(clippy::too_many_arguments)]
pub fn recovery_identity(
    expected: &NativeBuffer,
    saved_record: &[u8; 0xB0],
    saved_context: &Context,
    saved_address: u64,
    current: &NativeBuffer,
    current_record: &[u8; 0xB0],
    current_context: &Context,
    current_address: u64,
) -> bool {
    saved_context.observation == KNOWN
        && current_context.observation == KNOWN
        && saved_context.address != 0
        && saved_context.list != 0
        && saved_context.state != 0
        && saved_context.pending == 0
        && saved_context.submitted == 0
        && (saved_context.kind == 1 || saved_context.kind == 2)
        && current_context.address == saved_context.address
        && current_context.list == saved_context.list
        && current_context.state == saved_context.state
        && current_context.kind == saved_context.kind
        && current_context.submitted == 0
        && expected.context == saved_context.address
        && current.context == expected.context
        && expected.resource != 0
        && current.resource == expected.resource
        && expected.gpu != 0
        && current.gpu == expected.gpu
        && expected.bytes != 0
        && current.bytes == expected.bytes
        && expected.registry != 0
        && current.registry == expected.registry
        && expected.handle != 0
        && current.handle == expected.handle
        && (expected.kind == 0 || expected.kind == 3)
        && current.kind == expected.kind
        && saved_address != 0
        && current_address == saved_address
        && saved_record == current_record
}
/// detail::CopyExtent.
pub fn copy_extent(extent: u32, layout: u32, offset: u64, bytes: u64) -> bool {
    if extent == SELECTED_RANGE {
        return bytes != 0 && bytes <= MAX_COPY_BYTES;
    }
    extent == FULL_ORIGINAL_VIEW
        && layout == ORIGINAL_INDICES
        && offset == 0
        && bytes == SOURCE_BYTES
}
/// detail::Range: the copy inside the resource, the view and the native buffer, on stride boundaries.
pub fn range(a: &Admission) -> bool {
    let width = a.desc.Width;
    if a.resource == 0
        || a.gpu == 0
        || a.gpu > u64::MAX - width
        || !copy_extent(a.extent, a.layout, a.source_offset, a.bytes)
        || a.bytes % STRIDE != 0
        || a.source_offset % STRIDE != 0
        || a.source_offset > width
        || a.bytes > width - a.source_offset
        || a.view_offset > width
        || a.view_bytes > width - a.view_offset
        || a.source_offset < a.view_offset
        || a.source_offset - a.view_offset > a.view_bytes
        || a.bytes > a.view_bytes - (a.source_offset - a.view_offset)
    {
        return false;
    }
    a.native_resource == a.resource
        && a.native_gpu >= a.gpu
        && a.native_gpu - a.gpu <= width
        && a.native_bytes != 0
        && a.native_bytes <= width - (a.native_gpu - a.gpu)
        && a.gpu + a.source_offset >= a.native_gpu
        && a.gpu + a.source_offset - a.native_gpu <= a.native_bytes
        && a.bytes <= a.native_bytes - (a.gpu + a.source_offset - a.native_gpu)
}
/// detail::Admit.
pub fn admit(a: &Admission) -> Failure {
    if !a.source_current
        || !a.same_device
        || !a.recording_current
        || a.handle == 0
        || a.handle > MAX_HANDLES
        || a.stage != STAGE_INDEX
        || a.shader_register != SHADER_REGISTER
        || a.parameter != ROOT_PARAMETER
        || a.offset != TABLE_OFFSET
    {
        return Failure::Source;
    }
    if !a.creation_known
        || a.creation_kind != 1
        || a.creation_heap != 0
        || a.creation_offset != 0
        || a.creation_bytes != 0
        || !a.unprotected
    {
        return Failure::Creation;
    }
    let d = &a.desc;
    let h = &a.heap;
    if d.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || d.Width != SOURCE_BYTES
        || d.Height != 1
        || d.DepthOrArraySize != 1
        || d.MipLevels != 1
        || d.Format != DXGI_FORMAT_UNKNOWN
        || d.SampleDesc.Count != 1
        || d.SampleDesc.Quality != 0
        || d.Layout != D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        || d.Flags.0 != 0
        || (d.Alignment != 0 && d.Alignment != D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT as u64)
        || h.Type != D3D12_HEAP_TYPE_DEFAULT
        || a.native_gpu != a.gpu
        || a.native_bytes != d.Width
        || h.CPUPageProperty != D3D12_CPU_PAGE_PROPERTY_UNKNOWN
        || h.MemoryPoolPreference != D3D12_MEMORY_POOL_UNKNOWN
        || h.CreationNodeMask > 1
        || h.VisibleNodeMask > 1
        || a.heap_flags & !ALLOWED_HEAP_FLAGS != 0
    {
        return Failure::Backing;
    }
    let v = &a.view;
    // SAFETY: the Buffer arm of the view's union (checked by its dimension first).
    let buffer = unsafe { v.Anonymous.Buffer };
    if v.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || v.Format != DXGI_FORMAT_UNKNOWN
        || buffer.Flags.0 != 0
        || v.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || buffer.StructureByteStride as u64 != STRIDE
        || buffer.NumElements == 0
        || buffer.FirstElement > u64::MAX / STRIDE
        || a.view_offset != buffer.FirstElement * STRIDE
        || a.view_bytes != buffer.NumElements as u64 * STRIDE
        || !range(a)
    {
        return Failure::Range;
    }
    if a.extent == FULL_ORIGINAL_VIEW
        && (buffer.FirstElement != 0
            || buffer.NumElements as u64 != SOURCE_BYTES / STRIDE
            || a.view_offset != 0
            || a.view_bytes != SOURCE_BYTES)
    {
        return Failure::Range;
    }
    if read_mask(a.state, a.native_kind, false) {
        Failure::None
    } else {
        Failure::State
    }
}
/// detail::Reserve: a dense copy at the cursor.
pub fn reserve(cursor: u64, capacity: u64, bytes: u64) -> Option<(u64, u64)> {
    if capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity % ALIGNMENT != 0
        || cursor > capacity
        || cursor % ALIGNMENT != 0
        || bytes == 0
        || bytes > MAX_COPY_BYTES
        || bytes % STRIDE != 0
        || bytes > capacity - cursor
    {
        return None;
    }
    Some((cursor, cursor + bytes))
}
/// detail::ReserveView: a copy at its original index inside a whole-view window: (offset, end, view end).
pub fn reserve_view(
    cursor: u64,
    capacity: u64,
    source_offset: u64,
    bytes: u64,
) -> Option<(u64, u64, u64)> {
    if capacity == 0
        || capacity > MAX_ARENA_BYTES
        || cursor % STRIDE != 0
        || source_offset % STRIDE != 0
        || source_offset >= SOURCE_BYTES
        || bytes == 0
        || bytes > MAX_COPY_BYTES
        || bytes % STRIDE != 0
        || bytes > SOURCE_BYTES - source_offset
    {
        return None;
    }
    let target = cursor.max(source_offset);
    let first = target - source_offset;
    if first > capacity || SOURCE_BYTES > capacity - first {
        return None;
    }
    Some((target, target + bytes, first + SOURCE_BYTES))
}
/// detail::ReserveExtent.
pub fn reserve_extent(
    extent: u32,
    layout: u32,
    cursor: u64,
    capacity: u64,
    source_offset: u64,
    bytes: u64,
) -> Option<(u64, u64, u64)> {
    if !copy_extent(extent, layout, source_offset, bytes) {
        return None;
    }
    if extent == SELECTED_RANGE {
        return match layout {
            ORIGINAL_INDICES => reserve_view(cursor, capacity, source_offset, bytes),
            DENSE => reserve(cursor, capacity, bytes).map(|(o, e)| (o, e, 0)),
            _ => None,
        };
    }
    if capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity % STRIDE != 0
        || cursor % STRIDE != 0
        || cursor > capacity
        || bytes > capacity - cursor
    {
        return None;
    }
    Some((cursor, cursor + bytes, cursor + bytes))
}
/// detail::ReuseMatches: a recorded full version of the same resource and recording, earlier in the same no-write window.
#[allow(clippy::too_many_arguments)]
pub fn reuse_matches(
    prior: &FullVersion,
    span: &Span,
    list: u64,
    generation: u64,
    resource: u64,
    gpu: u64,
    occurrence: u64,
    cursor: u64,
    capacity: u64,
) -> bool {
    prior.recorded
        && list != 0
        && generation != 0
        && resource != 0
        && gpu != 0
        && occurrence != 0
        && prior.list == list
        && prior.generation == generation
        && prior.resource == resource
        && prior.gpu == gpu
        && prior.occurrence != 0
        && prior.occurrence < occurrence
        && prior.destination % STRIDE == 0
        && capacity != 0
        && capacity <= MAX_ARENA_BYTES
        && capacity % STRIDE == 0
        && cursor % STRIDE == 0
        && cursor <= capacity
        && prior.destination <= cursor
        && SOURCE_BYTES <= cursor - prior.destination
        && span.resource == resource
        && span.gpu == gpu
        && span.backing == SOURCE_BYTES
        && span.offset == 0
        && span.bytes == SOURCE_BYTES
        && span.destination == prior.destination
        && span.occurrence == prior.occurrence
}
/// detail::Epoch.
pub fn epoch(e: &ReadEpoch, list: u64, generation: u64, occurrence: u64) -> bool {
    e.certified
        && list != 0
        && generation != 0
        && occurrence != 0
        && e.list == list
        && e.generation == generation
        && e.occurrence == occurrence
}

// ---- the C ABI (the header's inline functions in the Rust build)

#[no_mangle]
pub extern "C" fn cvr_sb_no_protected_session(result: i32, present: bool) -> bool {
    no_protected_session(result, present)
}
/// # Safety
/// `state` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_sb_read_mask(state: *const ReadState, kind: u32, ready: bool) -> bool {
    // SAFETY: as the caller promises.
    read_mask(unsafe { *state }, kind, ready)
}
/// # Safety
/// `current` and `required` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_sb_recovery_mask(
    previous: u32,
    current: *const ReadState,
    kind: u32,
    required: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = recovery_mask(previous, unsafe { *current }, kind);
    unsafe { *required = value.unwrap_or(0) };
    value.is_some()
}
/// # Safety
/// The records valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_sb_recovery_identity(
    expected: *const NativeBuffer,
    saved_record: *const [u8; 0xB0],
    saved_context: *const Context,
    saved_address: u64,
    current: *const NativeBuffer,
    current_record: *const [u8; 0xB0],
    current_context: *const Context,
    current_address: u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        recovery_identity(
            &*expected,
            &*saved_record,
            &*saved_context,
            saved_address,
            &*current,
            &*current_record,
            &*current_context,
            current_address,
        )
    }
}
#[no_mangle]
pub extern "C" fn cvr_sb_copy_extent(extent: u32, layout: u32, offset: u64, bytes: u64) -> bool {
    copy_extent(extent, layout, offset, bytes)
}
/// # Safety
/// `a` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_sb_range(a: *const Admission) -> bool {
    // SAFETY: as the caller promises.
    range(unsafe { &*a })
}
/// # Safety
/// `a` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_sb_admit(a: *const Admission) -> u32 {
    // SAFETY: as the caller promises.
    admit(unsafe { &*a }) as u32
}
/// # Safety
/// `offset`, `end` and `view_end` valid (`view_end` may be null for the dense reservation).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_sb_reserve(
    mode: u32,
    extent: u32,
    layout: u32,
    cursor: u64,
    capacity: u64,
    source_offset: u64,
    bytes: u64,
    offset: *mut u64,
    end: *mut u64,
    view_end: *mut u64,
) -> bool {
    let value = match mode {
        0 => reserve(cursor, capacity, bytes).map(|(o, e)| (o, e, 0)),
        1 => reserve_view(cursor, capacity, source_offset, bytes),
        _ => reserve_extent(extent, layout, cursor, capacity, source_offset, bytes),
    };
    let (o, e, v) = value.unwrap_or((0, 0, 0));
    // SAFETY: as the caller promises.
    unsafe {
        *offset = o;
        *end = e;
        if !view_end.is_null() {
            *view_end = v;
        }
    }
    value.is_some()
}
/// # Safety
/// `prior` and `span` valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_sb_reuse_matches(
    prior: *const FullVersion,
    span: *const Span,
    list: u64,
    generation: u64,
    resource: u64,
    gpu: u64,
    occurrence: u64,
    cursor: u64,
    capacity: u64,
) -> bool {
    // SAFETY: as the caller promises.
    reuse_matches(
        unsafe { &*prior },
        unsafe { &*span },
        list,
        generation,
        resource,
        gpu,
        occurrence,
        cursor,
        capacity,
    )
}
/// # Safety
/// `e` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_sb_epoch(
    e: *const ReadEpoch,
    list: u64,
    generation: u64,
    occurrence: u64,
) -> bool {
    // SAFETY: as the caller promises.
    epoch(unsafe { &*e }, list, generation, occurrence)
}

// ---- the structured snapshot view's rules (include/Render/StructuredSnapshotView.hpp, detail) ----------------------

/// structured_snapshot_view::Stride / Elements.
pub const VIEW_STRIDE: u64 = 128;
pub const VIEW_ELEMENTS: u64 = 8192;

/// structured_snapshot_view::Scope.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ViewScope {
    pub root: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub eye: u32,
}
/// structured_snapshot_view::Mapping.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ViewMapping {
    pub target_offset: u64,
    pub private_first_element: u64,
    pub copy_bytes: u64,
    pub next_free: u64,
    pub view_end: u64,
}
const _: () =
    assert!(core::mem::size_of::<ViewScope>() == 40 && core::mem::size_of::<ViewMapping>() == 40);

/// detail::SameBufferView: two equal buffer SRV descriptions.
pub fn same_buffer_view(
    a: &D3D12_SHADER_RESOURCE_VIEW_DESC,
    b: &D3D12_SHADER_RESOURCE_VIEW_DESC,
) -> bool {
    let (x, y) = unsafe { (a.Anonymous.Buffer, b.Anonymous.Buffer) };
    a.ViewDimension == D3D12_SRV_DIMENSION_BUFFER
        && b.ViewDimension == a.ViewDimension
        && a.Format == b.Format
        && a.Shader4ComponentMapping == b.Shader4ComponentMapping
        && x.FirstElement == y.FirstElement
        && x.NumElements == y.NumElements
        && x.StructureByteStride == y.StructureByteStride
        && x.Flags == y.Flags
}
/// detail::ValidScope.
pub fn valid_view_scope(scope: &ViewScope) -> bool {
    scope.root != 0
        && scope.list != 0
        && scope.generation != 0
        && scope.occurrence != 0
        && scope.eye < 2
}
/// detail::Reserve: the records' copy at its original index inside a whole-view window.
pub fn reserve_records(
    cursor: u64,
    first_record: u32,
    bytes: u32,
    budget: u64,
) -> Option<ViewMapping> {
    if first_record as u64 >= VIEW_ELEMENTS {
        return None;
    }
    let source = first_record as u64 * VIEW_STRIDE;
    let (target, end, view_end) = reserve_view(cursor, budget, source, bytes as u64)?;
    Some(ViewMapping {
        target_offset: target,
        private_first_element: (target - source) / VIEW_STRIDE,
        copy_bytes: bytes as u64,
        next_free: end,
        view_end,
    })
}

/// detail::SameBufferView (`which` 0), ValidScope (1, `scope`).
///
/// # Safety
/// The descriptions (or the scope) are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssv_check(
    which: u32,
    a: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    b: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    scope: *const ViewScope,
) -> bool {
    unsafe {
        if which == 0 {
            same_buffer_view(&*a, &*b)
        } else {
            valid_view_scope(&*scope)
        }
    }
}
/// detail::Reserve (`output` zeroed on a refusal).
///
/// # Safety
/// `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssv_reserve(
    cursor: u64,
    first_record: u32,
    bytes: u32,
    budget: u64,
    output: *mut ViewMapping,
) -> bool {
    let mapping = reserve_records(cursor, first_record, bytes, budget);
    unsafe { *output = mapping.unwrap_or_default() };
    mapping.is_some()
}
