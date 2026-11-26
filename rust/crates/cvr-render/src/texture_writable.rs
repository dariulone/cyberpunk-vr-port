//! THE NATIVE TEXTURES' WRITABLE IDENTITIES (formerly the writable-target part of src/Render/NativeTextureRead.cpp): a
//! render target or depth buffer the engine binds for writing is authenticated on the original native draw -- its native
//! state twice the expected writable one on the current upload context, its native record read twice and stable -- and
//! its creation metadata (committed, or placed with the exact heap / offset / bytes) retained with the resource. Later
//! validations recheck that metadata and the native record without reading native state maps; a supplied queue also
//! requires the original recording's pending submission there. The record's +0x10 owner is an observation only, never
//! a native owner pin. The C++ WritableTargetIdentity is written by Rust into a zeroed record; its authenticated copy
//! (the shared Data) stays the C++'s.
use core::ffi::c_void;

use windows::core::{IUnknown, Interface as _};
use windows::Win32::Graphics::Direct3D12::*;

use crate::native_recording::{
    cvr_native_recording_capture, cvr_native_recording_validate_recording,
    cvr_native_recording_validate_submission, Context as RecordingContext, Lease as Recording,
};
use crate::texture_immutable::{observe, query_creation, Creation};
use crate::texture_lease::{
    compatible, current_context, device_of, fail, fixed_context, game, identity, list_of, read,
    resolve_native, resource, UploadContext, READ,
};
use crate::texture_read::{read_state, writable_state, NativeTexture, State};

/// texture_read::WritableTargetIdentity as the C++ lays it out, up to its private authenticated copy.
#[repr(C)]
pub struct WritableIdentity {
    pub creation: Creation,
    pub native: NativeTexture,
    pub recording: Recording,
    pub original_context: UploadContext,
    pub resource_desc: D3D12_RESOURCE_DESC,
    /// Present only for a placed resource (creation.kind 2).
    pub heap_desc: D3D12_HEAP_DESC,
    pub resource_heap_properties: D3D12_HEAP_PROPERTIES,
    pub resource_heap_flags: D3D12_HEAP_FLAGS,
    pub expected_writable_state: D3D12_RESOURCE_STATES,
    /// The original audit bytes, not a full stable ABI.
    pub native_record: [u8; 0x58],
    /// The numeric native +0x10, never a COM owner.
    pub allocator_owner_observation: u64,
}
const _: () = assert!(
    core::mem::size_of::<WritableIdentity>() == 368
        && core::mem::offset_of!(WritableIdentity, resource_desc) == 136
        && core::mem::offset_of!(WritableIdentity, heap_desc) == 192
        && core::mem::offset_of!(WritableIdentity, native_record) == 268
        && core::mem::offset_of!(WritableIdentity, allocator_owner_observation) == 360
);

/// detail::WritableRecordObservation: the resource identity; kind and owner stay uninterpreted numbers.
#[derive(PartialEq, Eq)]
pub(crate) struct WritableRecord {
    resource: u64,
    owner: u64,
    kind: u8,
}
pub(crate) fn writable_record(bytes: &[u8; 0x58]) -> WritableRecord {
    WritableRecord {
        resource: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        owner: u64::from_le_bytes(bytes[0x10..0x18].try_into().unwrap()),
        kind: bytes[8],
    }
}
pub(crate) fn same_resource_description(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Alignment == b.Alignment
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.SampleDesc.Count == b.SampleDesc.Count
        && a.SampleDesc.Quality == b.SampleDesc.Quality
        && a.Layout == b.Layout
        && a.Flags == b.Flags
}
pub(crate) fn same_heap_properties(a: &D3D12_HEAP_PROPERTIES, b: &D3D12_HEAP_PROPERTIES) -> bool {
    a.Type == b.Type
        && a.CPUPageProperty == b.CPUPageProperty
        && a.MemoryPoolPreference == b.MemoryPoolPreference
        && a.CreationNodeMask == b.CreationNodeMask
        && a.VisibleNodeMask == b.VisibleNodeMask
}
pub(crate) fn same_heap_description(a: &D3D12_HEAP_DESC, b: &D3D12_HEAP_DESC) -> bool {
    a.SizeInBytes == b.SizeInBytes
        && a.Alignment == b.Alignment
        && a.Flags == b.Flags
        && same_heap_properties(&a.Properties, &b.Properties)
}
fn raw<T: windows::core::Interface>(p: &Option<T>) -> *mut c_void {
    p.as_ref().map_or(core::ptr::null_mut(), |v| v.as_raw())
}
pub(crate) fn same_creation_metadata(a: &Creation, b: &Creation) -> bool {
    raw(&a.resource) == raw(&b.resource)
        && a.kind == b.kind
        && raw(&a.heap) == raw(&b.heap)
        && a.offset == b.offset
        && a.bytes == b.bytes
        && a.known == b.known
        && a.state == b.state
}
/// detail::SameWritableIdentityMetadata: equality, not authentication or an allocation-lifetime proof.
pub(crate) fn same_identity_metadata(a: &WritableIdentity, b: &WritableIdentity) -> bool {
    same_creation_metadata(&a.creation, &b.creation)
        && a.native.registry == b.native.registry
        && a.native.resource == b.native.resource
        && a.native.handle == b.native.handle
        && a.native.reserved == b.native.reserved
        && a.recording.token == b.recording.token
        && a.recording.generation == b.recording.generation
        && a.recording.context == b.recording.context
        && a.recording.list == b.recording.list
        && a.recording.registry == b.recording.registry
        && a.original_context == b.original_context
        && same_resource_description(&a.resource_desc, &b.resource_desc)
        && same_heap_description(&a.heap_desc, &b.heap_desc)
        && same_heap_properties(&a.resource_heap_properties, &b.resource_heap_properties)
        && a.resource_heap_flags == b.resource_heap_flags
        && a.expected_writable_state == b.expected_writable_state
        && a.native_record == b.native_record
        && a.allocator_owner_observation == b.allocator_owner_observation
}
fn recording_context(c: &UploadContext) -> RecordingContext {
    RecordingContext {
        context: c.context,
        state: c.state,
        list: c.list,
    }
}

/// ValidateWritableTarget: the resource's native state is twice the expected writable one on this list's current
/// upload context, its record still naming it.
fn validate_writable_target(list: *mut c_void, resource_ptr: *mut c_void, expected: u32) -> bool {
    fail(80);
    let (Some(l), Some(r)) = (list_of(&list), resource(&resource_ptr)) else {
        return false;
    };
    // SAFETY: a live list.
    if !compatible()
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || !writable_state(
            State {
                value: expected,
                status: 1,
            },
            expected,
        )
    {
        return false;
    }
    let base = game();
    let context = current_context(base);
    fail(81);
    if context.list != list as u64 || !fixed_context(&context) {
        return false;
    }
    let Some(native) = resolve_native(r) else {
        fail(82);
        return false;
    };
    let first = read_state(
        &native,
        context.context,
        context.list,
        READ,
        core::ptr::null_mut(),
    );
    fail(83);
    if !writable_state(first, expected) {
        return false;
    }
    let second = read_state(
        &native,
        context.context,
        context.list,
        READ,
        core::ptr::null_mut(),
    );
    fail(84);
    if !writable_state(second, expected)
        || !identity(&native)
        || !fixed_context(&context)
        || current_context(base) != context
    {
        return false;
    }
    fail(0);
    true
}
/// ReadWritableRecord: the record read twice with the same identity, still naming the resource (the second sample).
/// The rest of the record is an audit sample: unknown or mutable fields never become an owner or final-state contract.
fn read_writable_record(native: &NativeTexture) -> Option<[u8; 0x58]> {
    if !identity(native) {
        return None;
    }
    let offset = 0x2f1d8 + (native.handle as u64 - 1) * 0xb0;
    if native.registry > u64::MAX - offset - 0x58 {
        return None;
    }
    let (mut first, mut second) = ([0u8; 0x58], [0u8; 0x58]);
    if !read(native.registry + offset, &mut first) || !read(native.registry + offset, &mut second) {
        return None;
    }
    let current = writable_record(&second);
    if current.resource != native.resource
        || current != writable_record(&first)
        || !identity(native)
    {
        return None;
    }
    Some(second)
}
/// WritablePhysicalMetadata: the creation metadata, the description, the heap properties (and a placed heap's
/// description) unchanged, and the native record's identity the captured one.
fn writable_physical_metadata(value: &WritableIdentity) -> bool {
    let held = raw(&value.creation.resource);
    if held.is_null() || value.native.resource != held as u64 {
        return false;
    }
    let Some(current) = query_creation(held) else {
        return false;
    };
    let Some(r) = current.resource.as_ref() else {
        return false;
    };
    // SAFETY (whole function): live objects.
    if !same_creation_metadata(&value.creation, &current)
        || !same_resource_description(&value.resource_desc, &unsafe { r.GetDesc() })
    {
        return false;
    }
    let mut properties = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    if unsafe { r.GetHeapProperties(Some(&mut properties), Some(&mut flags)) }.is_err()
        || !same_heap_properties(&value.resource_heap_properties, &properties)
        || flags != value.resource_heap_flags
    {
        return false;
    }
    if current.kind == 2 {
        match current.heap.as_ref() {
            Some(heap) if same_heap_description(&value.heap_desc, &unsafe { heap.GetDesc() }) => {}
            _ => return false,
        }
    }
    read_writable_record(&value.native)
        .is_some_and(|record| writable_record(&record) == writable_record(&value.native_record))
}
/// CaptureWritableIdentity: see the module comment; `out` receives the identity (references for the caller).
fn capture_writable_identity(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    expected: u32,
    out: *mut WritableIdentity,
) -> bool {
    fail(103);
    let (Some(l), Some(r)) = (list_of(&list), resource(&resource_ptr)) else {
        return false;
    };
    if cvr_host::probe::internal() || !observe() {
        return false;
    }
    let mut next = WritableIdentity {
        creation: Creation {
            resource: Some(r.clone()),
            ..Default::default()
        },
        native: NativeTexture::default(),
        recording: Recording {
            token: core::ptr::null_mut(),
            generation: 0,
            context: 0,
            list: 0,
            registry: 0,
        },
        original_context: UploadContext::default(),
        resource_desc: D3D12_RESOURCE_DESC::default(),
        heap_desc: D3D12_HEAP_DESC::default(),
        resource_heap_properties: D3D12_HEAP_PROPERTIES::default(),
        resource_heap_flags: D3D12_HEAP_FLAGS::default(),
        expected_writable_state: D3D12_RESOURCE_STATES(expected as i32),
        native_record: [0; 0x58],
        allocator_owner_observation: 0,
    };
    if !validate_writable_target(list, resource_ptr, expected) {
        return false;
    }
    let base = game();
    next.original_context = current_context(base);
    let native_context = recording_context(&next.original_context);
    fail(104);
    let captured = next.original_context.list == list as u64
        // SAFETY: valid records; the token returned is this function's reference.
        && unsafe { cvr_native_recording_capture(list, &native_context, &mut next.recording) };
    // The token's reference: it moves into the output, or is released.
    let token = (!next.recording.token.is_null())
        // SAFETY: the reference Capture returned.
        .then(|| unsafe { IUnknown::from_raw(next.recording.token) });
    if !captured {
        return false;
    }
    fail(105);
    let Some(native) = resolve_native(r) else {
        fail(105);
        return false;
    };
    next.native = native;
    let Some(record) = (next.native.registry == next.recording.registry)
        .then(|| read_writable_record(&next.native))
        .flatten()
    else {
        fail(105);
        return false;
    };
    next.native_record = record;
    next.allocator_owner_observation = writable_record(&record).owner;
    fail(106);
    let Some(creation) = query_creation(resource_ptr) else {
        return false;
    };
    next.creation = creation;
    // SAFETY (rest of the function): live objects.
    next.resource_desc = unsafe { r.GetDesc() };
    if let Some(heap) = next.creation.heap.as_ref() {
        next.heap_desc = unsafe { heap.GetDesc() };
    }
    fail(107);
    if unsafe {
        r.GetHeapProperties(
            Some(&mut next.resource_heap_properties),
            Some(&mut next.resource_heap_flags),
        )
    }
    .is_err()
    {
        return false;
    }
    match (device_of(r), device_of(l)) {
        (Some(a), Some(b)) if a.as_raw() == b.as_raw() => {}
        _ => return false,
    }
    let recorded = |next: &WritableIdentity| {
        // SAFETY: valid records.
        unsafe { cvr_native_recording_validate_recording(&next.recording, list, &native_context) }
    };
    fail(108);
    if !writable_physical_metadata(&next)
        || current_context(base) != next.original_context
        || !recorded(&next)
    {
        return false;
    }
    if !validate_writable_target(list, resource_ptr, expected) {
        return false;
    }
    fail(108);
    if current_context(base) != next.original_context
        || !writable_physical_metadata(&next)
        || !recorded(&next)
    {
        return false;
    }
    next.recording.token = token.map_or(core::ptr::null_mut(), |t| t.into_raw());
    // SAFETY: the caller's zeroed record takes the identity's references.
    unsafe { core::ptr::write(out, next) };
    fail(0);
    true
}
/// ValidateWritableIdentity: the identity is its authenticated copy's, whose metadata and native record still hold;
/// a queue also requires the original recording's pending submission there.
fn validate_writable_identity(
    value: &WritableIdentity,
    captured: Option<&WritableIdentity>,
    queue: *mut c_void,
) -> bool {
    fail(109);
    let Some(captured) = captured else {
        return false;
    };
    if !same_identity_metadata(value, captured) {
        return false;
    }
    fail(108);
    if !writable_physical_metadata(captured) {
        return false;
    }
    fail(110);
    // SAFETY: valid records.
    if !queue.is_null()
        && !unsafe { cvr_native_recording_validate_submission(&captured.recording, queue) }
    {
        return false;
    }
    fail(0);
    true
}

// ---- the C ABI (src/Render/NativeTextureRead.cpp in the Rust build)

/// # Safety
/// `list` and `resource` live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_validate_writable_target(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    expected: u32,
) -> bool {
    validate_writable_target(list, resource_ptr, expected)
}
/// # Safety
/// `list` and `resource` live or null; `out` a zeroed WritableTargetIdentity.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_capture_writable_identity(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    expected: u32,
    out: *mut WritableIdentity,
) -> bool {
    capture_writable_identity(list, resource_ptr, expected, out)
}
/// # Safety
/// `value` valid; `captured` its authenticated copy or null; `queue` live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_validate_writable_identity(
    value: *const WritableIdentity,
    captured: *const WritableIdentity,
    queue: *mut c_void,
) -> bool {
    // SAFETY: as the caller promises (borrowed views: nothing here is dropped).
    validate_writable_identity(unsafe { &*value }, unsafe { captured.as_ref() }, queue)
}
