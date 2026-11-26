//! THE NATIVE STRUCTURED BUFFER SNAPSHOT AT THE ACTUAL CALL (formerly src/Render/NativeStructuredBufferSnapshot.cpp): the
//! per-thread ledger of actual native source callback scopes; one held source SRV's backing copied into a private arena
//! (one default-heap buffer per list generation -- dense, or at the source's own indices -- finalized to COPY_SOURCE or
//! GENERIC_READ); a whole original view copied once per occurrence and reused through the arena's copy window; the
//! receipt that keeps the source and its revalidated backing with the command list; the native combined-read
//! registration (0x337DA0, then the flush 0x3E3BB4) with its recovery; and the proofs the copy validates at the call
//! and at submission. The rules each step applies are structured_snapshot.rs'; the source's backing is read again
//! through source_srv_observe.rs.
//!
//! The C++ API (include/Render/NativeStructuredBufferSnapshot.hpp) forwards here; the graph parent's revalidation
//! (still C++, NativePreparation.cpp) is the attached host's.
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::{Acquire, Relaxed, Release, SeqCst}};
use std::sync::{Arc, Mutex};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::copy_windows::{self, Window};
use crate::ia_binding::cvr_iab_read;
use crate::ia_snapshot_same::CreationInfo;
use crate::indirect_copy::{GraphParentRecord, ScopeLedger, ScopeStamp};
use crate::native_recording::{Context as UploadContext, Lease as Recording};
use crate::source_cbv_details::{self as scd, Context, KNOWN};
use crate::source_srv_observe::Backing;
use crate::structured_snapshot::{self as rules, Admission, FullVersion, ReadEpoch, Span, FULL_ORIGINAL_VIEW, ORIGINAL_INDICES};

#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
use crate::declared_moved_exports::{CyberpunkVR_StructuredBufferFinalizeDiagnostic, CyberpunkVR_StructuredBufferSnapshotsEnable};
/// The unit tests' switch and diagnostic (the plugin's and the probe tests' are declared_moved_exports').
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static CyberpunkVR_StructuredBufferSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static mut CyberpunkVR_StructuredBufferFinalizeDiagnostic: [u64; 8] = [0; 8];

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool;
}

/// structured_buffer_snapshot::Failure.
mod failure {
    pub const NONE: u32 = 0;
    pub const DISABLED: u32 = 1;
    pub const ARGUMENTS: u32 = 2;
    pub const SCOPE: u32 = 3;
    pub const CODE: u32 = 4;
    pub const PARENT: u32 = 5;
    pub const SOURCE: u32 = 6;
    pub const CREATION: u32 = 7;
    pub const BACKING: u32 = 8;
    pub const STATE: u32 = 9;
    pub const CAPACITY: u32 = 11;
    pub const RETENTION: u32 = 12;
    pub const CHANGED: u32 = 13;
    pub const REGISTRATION: u32 = 14;
    pub const RECOVERY_VERIFIED: u32 = 15;
    pub const RECOVERY_FATAL: u32 = 16;
    pub const ALREADY_RECORDED: u32 = 17;
    pub const EXCEPTION: u32 = 18;
}
/// The receipt's Stage.
mod stage {
    pub const PREPARED: u32 = 0;
    pub const RECORDING: u32 = 1;
    pub const RECORDED: u32 = 2;
    pub const FAILED: u32 = 3;
}
const SOURCE_BYTES: u64 = 1024 * 1024;
const STRIDE: u64 = 128;
const MAX_ARENA_BYTES: u64 = 8 * 1024 * 1024;
const ALIGNMENT: u64 = 16;
const MAX_FULL_COPIES: usize = (MAX_ARENA_BYTES / SOURCE_BYTES) as usize;
const CAPTURED: u32 = 1;
const COPY_SOURCE: u32 = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
const RECEIPT_TYPE: GUID = GUID::from_values(0x9d673805, 0xdd81, 0x4c14, [0xaa, 0x7e, 0x6a, 0x23, 0x54, 0x4f, 0x15, 0x82]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const REGISTER_RVA: u64 = 0x337DA0;
const FLUSH_RVA: u64 = 0x3E3BB4;

thread_local! {
    static SCOPES: RefCell<ScopeLedger> = RefCell::new(ScopeLedger::default());
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
fn set_failure(value: u32) {
    FAILURE.set(value);
}
fn enabled() -> bool {
    CyberpunkVR_StructuredBufferSnapshotsEnable.load(Relaxed) != 0
}
fn thread() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() }
}
fn image() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn scope_matches(stamp: &ScopeStamp, list: u64, generation: u64, position: u64) -> bool {
    SCOPES.with_borrow(|s| s.matches(stamp, list, generation, position, thread()))
}

// ---- the C++ half's values and its host

/// structured_buffer_snapshot::Request (the source's references owned by whoever holds it).
#[repr(C)]
pub struct Request {
    pub source: Backing,
    pub parameter: u32,
    pub offset: u32,
    pub source_offset: u64,
    pub bytes: u32,
    pub extent: u32,
}
const _: () = assert!(core::mem::size_of::<Request>() == 1136 && core::mem::offset_of!(Request, parameter) == 1112);
/// structured_buffer_snapshot::PreparedCopy (the owner an owned reference).
#[repr(C)]
pub struct PreparedCopy {
    pub owner: *mut c_void,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub reused: bool,
    pub gpu_copied_bytes: u64,
}
const _: () = assert!(core::mem::size_of::<PreparedCopy>() == 72);
/// structured_buffer_snapshot::PrivateRange (the resource and the owner owned references).
#[repr(C)]
pub struct PrivateRange {
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
}
const _: () = assert!(core::mem::size_of::<PrivateRange>() == 40);
/// preparation::RevalidateGraphParent (preparation_trace.rs).
fn revalidate(parent: &GraphParentRecord) -> bool {
    crate::preparation_trace::revalidate_graph_parent(parent)
}

// ---- references

pub(crate) fn add_ref(object: *mut c_void) -> *mut c_void {
    // SAFETY: a live COM-shaped object or null.
    if let Some(o) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        core::mem::forget(o.clone());
    }
    object
}
pub(crate) fn release(object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: a reference this module owns, released once.
        drop(unsafe { IUnknown::from_raw(object) });
    }
}
/// A source SRV backing whose binding resource and recording token are owned.
pub(crate) struct OwnedBacking(pub(crate) Box<Backing>);
impl OwnedBacking {
    pub(crate) fn empty() -> Self {
        // SAFETY: plain data (null references, zero values) -- the C++ value-initialised backing.
        OwnedBacking(Box::new(unsafe { core::mem::zeroed() }))
    }
    pub(crate) fn copy(value: &Backing) -> Self {
        add_ref(value.binding.resource);
        add_ref(value.recording.token);
        // SAFETY: a bitwise copy whose two references were taken above.
        OwnedBacking(Box::new(unsafe { core::ptr::read(value) }))
    }
}
impl Drop for OwnedBacking {
    fn drop(&mut self) {
        release(self.0.recording.token);
        release(self.0.binding.resource);
    }
}
/// The C++ assignment `*value={}` for a backing the revalidation filled and then rejected.
pub(crate) unsafe extern "C" fn clear_backing(value: *mut Backing) {
    // SAFETY: a backing this module owns.
    unsafe {
        release((*value).recording.token);
        release((*value).binding.resource);
        core::ptr::write_bytes(value, 0, 1);
    }
}
/// A request copy with its source references owned.
struct OwnedRequest {
    source: OwnedBacking,
    parameter: u32,
    offset: u32,
    source_offset: u64,
    bytes: u32,
    extent: u32,
}
impl OwnedRequest {
    fn copy(value: &Request) -> Self {
        OwnedRequest {
            source: OwnedBacking::copy(&value.source),
            parameter: value.parameter,
            offset: value.offset,
            source_offset: value.source_offset,
            bytes: value.bytes,
            extent: value.extent,
        }
    }
}
/// A native recording lease whose token reference is owned.
pub(crate) struct OwnedRecording(pub(crate) Recording);
impl OwnedRecording {
    pub(crate) fn copy(lease: &Recording) -> Self {
        add_ref(lease.token);
        OwnedRecording(Recording { ..*lease })
    }
    pub(crate) fn none() -> Self {
        OwnedRecording(Recording { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
}
impl Drop for OwnedRecording {
    fn drop(&mut self) {
        release(self.0.token);
    }
}
/// constants::ResourceCreationInfo with its resource and heap owned.
pub(crate) struct Creation(pub(crate) CreationInfo);
impl Default for Creation {
    fn default() -> Self {
        Creation(CreationInfo {
            kind: 0,
            initial_state_known: false,
            initial_state: 0,
            resource: core::ptr::null_mut(),
            heap: core::ptr::null_mut(),
            heap_offset: 0,
            allocation_bytes: 0,
        })
    }
}
impl Drop for Creation {
    fn drop(&mut self) {
        release(self.0.heap);
        release(self.0.resource);
    }
}
/// constants::QueryResourceCreation.
pub(crate) fn query_creation(resource: *mut c_void) -> Option<Creation> {
    let (mut kind, mut known, mut state, mut heap, mut offset, mut bytes) = (0u32, false, 0u32, core::ptr::null_mut(), 0u64, 0u64);
    // SAFETY: a live resource or null; the outputs are locals.
    if !unsafe { crate::creation_receipts::cvr_crc_query(resource, &mut kind, &mut known, &mut state, &mut heap, &mut offset, &mut bytes) } {
        return None;
    }
    Some(Creation(CreationInfo {
        kind,
        initial_state_known: known,
        initial_state: if known { state } else { 0 },
        resource: add_ref(resource),
        heap,
        heap_offset: offset,
        allocation_bytes: bytes,
    }))
}
fn same_creation(a: &Creation, b: &Creation) -> bool {
    crate::ia_snapshot_same::same_creation(&a.0, &b.0)
}
fn same_upload(a: &UploadContext, b: &UploadContext) -> bool {
    a.context == b.context && a.state == b.state && a.list == b.list
}
pub(crate) fn validate_recording(recording: &Recording, list: *mut c_void, context: &UploadContext) -> bool {
    // SAFETY: a live lease and list.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list, context) }
}
pub(crate) fn current_upload(base: u64) -> UploadContext {
    let c = crate::camera_upload::current(base);
    UploadContext { context: c.context, state: c.state, list: c.list }
}

// ---- the source's shape

fn zeroed_admission() -> Admission {
    // SAFETY: plain data; the C++ value-initialised admission (Selected range, dense).
    unsafe { core::mem::zeroed() }
}
fn same_shape(a: &Admission, b: &Admission) -> bool {
    let (x, y) = (&a.desc, &b.desc);
    a.resource == b.resource
        && a.gpu == b.gpu
        && a.heap_flags == b.heap_flags
        && a.heap.Type == b.heap.Type
        && a.heap.CPUPageProperty == b.heap.CPUPageProperty
        && a.heap.MemoryPoolPreference == b.heap.MemoryPoolPreference
        && a.heap.CreationNodeMask == b.heap.CreationNodeMask
        && a.heap.VisibleNodeMask == b.heap.VisibleNodeMask
        && x.Dimension == y.Dimension
        && x.Alignment == y.Alignment
        && x.Width == y.Width
        && x.Height == y.Height
        && x.DepthOrArraySize == y.DepthOrArraySize
        && x.MipLevels == y.MipLevels
        && x.Format == y.Format
        && x.SampleDesc.Count == y.SampleDesc.Count
        && x.SampleDesc.Quality == y.SampleDesc.Quality
        && x.Layout == y.Layout
        && x.Flags == y.Flags
}
/// Unprotected: no protected resource session behind the resource.
pub(crate) fn unprotected(resource: &ID3D12Resource) -> bool {
    let extended = match resource.cast::<ID3D12Resource1>() {
        Ok(extended) => extended,
        Err(e) if e.code() == E_NOINTERFACE => return true,
        Err(_) => return false,
    };
    let mut session: Option<IUnknown> = None;
    // SAFETY: a live resource; the output is a local.
    let result = match unsafe { extended.GetProtectedResourceSession(&mut session) } {
        Ok(()) => 0,
        Err(e) => e.code().0,
    };
    rules::no_protected_session(result, session.is_some())
}
fn device_of(object: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output is a local.
    unsafe { object.GetDevice(&mut device) }.ok().and(device)
}
#[allow(clippy::too_many_arguments)]
fn observe(
    list: *mut c_void,
    parameter: u32,
    offset: u32,
    source_offset: u64,
    bytes: u32,
    extent: u32,
    source: &Backing,
    creation: &Creation,
    layout: u32,
    output: &mut Admission,
) -> bool {
    let raw = source.binding.resource;
    // SAFETY: live objects (the backing owns its resource) or null.
    let (Some(l), Some(resource)) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe { ID3D12Resource::from_raw_borrowed(&raw) })
    else {
        return false;
    };
    let mut value = zeroed_admission();
    value.resource = raw as u64;
    // SAFETY: a live resource.
    unsafe {
        value.gpu = resource.GetGPUVirtualAddress();
        value.desc = resource.GetDesc();
    }
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: as above; the outputs are locals.
    if unsafe { resource.GetHeapProperties(Some(&mut value.heap), Some(&mut flags)) }.is_err() {
        return false;
    }
    value.heap_flags = flags.0 as u32;
    value.native_resource = source.native.resource;
    value.native_gpu = source.native.gpu;
    value.native_bytes = source.native.bytes;
    value.view_offset = source.source_offset;
    value.view_bytes = source.view_bytes;
    value.source_offset = source_offset;
    value.bytes = bytes as u64;
    value.view = source.binding.view;
    value.creation_known = creation.0.resource == raw;
    value.creation_kind = creation.0.kind;
    value.creation_heap = creation.0.heap as u64;
    value.creation_offset = creation.0.heap_offset;
    value.creation_bytes = creation.0.allocation_bytes;
    value.unprotected = unprotected(resource);
    let device = resource.cast::<ID3D12DeviceChild>().ok().and_then(|c| device_of(&c));
    let recording_device = l.cast::<ID3D12DeviceChild>().ok().and_then(|c| device_of(&c));
    // SAFETY: a live device.
    value.same_device = device.is_some() && device == recording_device && device.as_ref().is_some_and(|d| unsafe { d.GetNodeCount() } == 1);
    value.recording_current = validate_recording(&source.recording, list, &source.context);
    value.state = source.state;
    value.native_kind = source.native.kind;
    value.handle = source.native.handle;
    value.stage = source.stage;
    value.shader_register = source.shader_register;
    value.parameter = parameter;
    value.offset = offset;
    value.source_current = true;
    value.extent = extent;
    value.layout = layout;
    *output = value;
    true
}

// ---- the arena

/// A whole original view copied once: its version, the recording it was copied in and the two resources (owned).
struct FullCopyRecord {
    version: Mutex<FullVersion>,
    recording: OwnedRecording,
    source: *mut c_void,
    destination: *mut c_void,
}
// SAFETY: the references are free-threaded COM objects; the version is locked.
unsafe impl Send for FullCopyRecord {}
unsafe impl Sync for FullCopyRecord {}
impl FullCopyRecord {
    fn version(&self) -> FullVersion {
        *self.version.lock().unwrap_or_else(|e| e.into_inner())
    }
}
impl Drop for FullCopyRecord {
    fn drop(&mut self) {
        release(self.destination);
        release(self.source);
    }
}
/// The arena's storage: a COM object the receipts and the private ranges keep.
#[repr(C)]
struct Storage {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    resource: *mut c_void,
    device: *mut c_void,
    list: u64,
    generation: u64,
    capacity: u64,
    cursor: u64,
    layout: u32,
    thread: u32,
    recording: OwnedRecording,
    context: UploadContext,
    pending: AtomicU32,
    finalized: AtomicBool,
    failed: AtomicBool,
    window: Option<Arc<Window>>,
    full_copies: Vec<Arc<FullCopyRecord>>,
}
static STORAGE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: storage_query, AddRef: storage_add_ref, Release: storage_release };
unsafe extern "system" fn storage_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers.
    unsafe {
        *out = core::ptr::null_mut();
        if *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        storage_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn storage_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live storage.
    unsafe { (*this.cast::<Storage>()).references.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn storage_release(this: *mut c_void) -> u32 {
    // SAFETY: a live storage; the last reference frees it.
    let left = unsafe { (*this.cast::<Storage>()).references.fetch_sub(1, SeqCst) - 1 };
    if left == 0 {
        // SAFETY: made by Box::into_raw in arena_create.
        let storage = unsafe { Box::from_raw(this.cast::<Storage>()) };
        let (resource, device) = (storage.resource, storage.device);
        drop(storage);
        release(resource);
        release(device);
    }
    left
}
fn storage<'a>(raw: *mut c_void) -> Option<&'a mut Storage> {
    // SAFETY: a storage this module made (the C++ arena holds a reference) or null.
    unsafe { raw.cast::<Storage>().as_mut() }
}
pub(crate) fn transition(resource: *mut c_void, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// SnapshotArena::Create: S_OK with `*out` a new storage reference.
///
/// # Safety
/// `existing` is the arena's storage or null; `device` and `list` are live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_create(
    existing: *mut c_void,
    device: *mut c_void,
    list: *mut c_void,
    generation: u64,
    capacity: u64,
    layout: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the caller's output.
    unsafe { *out = core::ptr::null_mut() };
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena_create(existing, device, list, generation, capacity, layout)));
    match made {
        Ok(Ok(raw)) => {
            // SAFETY: as above.
            unsafe { *out = raw };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_FAIL,
    }
}
fn arena_create(existing: *mut c_void, device: *mut c_void, list: *mut c_void, generation: u64, capacity: u64, layout: u32) -> Result<*mut c_void, HRESULT> {
    // SAFETY: live objects or null.
    let (Some(d), Some(l)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) })
    else {
        return Err(E_INVALIDARG);
    };
    // SAFETY: live objects.
    if !existing.is_null()
        || generation == 0
        || capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity % ALIGNMENT != 0
        || unsafe { d.GetNodeCount() } != 1
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || (layout != rules::DENSE && layout != ORIGINAL_INDICES)
        || (layout == ORIGINAL_INDICES && capacity < SOURCE_BYTES)
    {
        return Err(E_INVALIDARG);
    }
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live list; the output is a local.
    if unsafe { l.GetDevice(&mut actual) }.is_err() || actual.as_ref().map(Interface::as_raw) != Some(device) {
        return Err(E_INVALIDARG);
    }
    let next = Box::into_raw(Box::new(Storage {
        vtbl: &STORAGE_VTBL,
        references: AtomicU32::new(1),
        resource: core::ptr::null_mut(),
        device: add_ref(device),
        list: list as u64,
        generation,
        capacity,
        cursor: 0,
        layout: rules::DENSE,
        thread: thread(),
        recording: OwnedRecording::none(),
        context: UploadContext::default(),
        pending: AtomicU32::new(0),
        finalized: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        window: None,
        full_copies: Vec::new(),
    }));
    let raw: *mut c_void = next.cast();
    let fail = move |hr: HRESULT| {
        // SAFETY: the one reference made above.
        unsafe { storage_release(raw) };
        Err(hr)
    };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: capacity,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    let created = {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: a live device; the descriptions are locals.
        unsafe { d.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut resource) }
    };
    if let Err(e) = created {
        return fail(e.code());
    }
    let Some(resource) = resource else {
        return fail(E_FAIL);
    };
    // SAFETY: the new storage, not yet shared.
    let s = unsafe { &mut *next };
    // SAFETY: a live resource.
    let gpu = unsafe { resource.GetGPUVirtualAddress() };
    s.resource = resource.into_raw();
    if gpu == 0 || gpu > u64::MAX - capacity {
        return fail(E_FAIL);
    }
    s.layout = layout;
    Ok(raw)
}
/// SnapshotArena::Abort.
///
/// # Safety
/// `raw` is the arena's storage or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_abort(raw: *mut c_void) {
    if let Some(s) = storage(raw) {
        s.failed.store(true, Release);
        if let Some(window) = &s.window {
            window.close();
        }
    }
}
/// SnapshotArena::Finalized.
///
/// # Safety
/// As cvr_sbsn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_finalized(raw: *mut c_void) -> bool {
    storage(raw).is_some_and(|s| s.finalized.load(Acquire) && !s.failed.load(Acquire))
}
/// SnapshotArena::Bytes.
///
/// # Safety
/// As cvr_sbsn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_bytes(raw: *mut c_void) -> u64 {
    storage(raw).map_or(0, |s| s.cursor)
}
/// SnapshotArena::Resource: a new reference to the arena's buffer, or null.
///
/// # Safety
/// As cvr_sbsn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_resource(raw: *mut c_void) -> *mut c_void {
    storage(raw).map_or(core::ptr::null_mut(), |s| add_ref(s.resource))
}
/// SnapshotArena::Finalize: the copies recorded, the arena turned readable (GENERIC_READ at the source's indices,
/// COPY_SOURCE dense); its diagnostic row written first.
///
/// # Safety
/// As cvr_sbsn_arena_abort; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_arena_finalize(raw: *mut c_void, list: *mut c_void) -> bool {
    guarded(|| {
        let s = storage(raw);
        let row = [
            list as u64,
            s.as_ref().map_or(0, |s| s.list),
            thread() as u64,
            s.as_ref().map_or(0, |s| s.thread as u64),
            s.as_ref().map_or(0, |s| s.pending.load(SeqCst) as u64),
            s.as_ref().map_or(0, |s| s.cursor),
            s.as_ref().map_or(0, |s| s.finalized.load(SeqCst) as u64),
            s.as_ref().map_or(0, |s| s.failed.load(SeqCst) as u64),
        ];
        // SAFETY: the export's eight words (written by the finalizing thread, read by the tools).
        unsafe { core::ptr::addr_of_mut!(CyberpunkVR_StructuredBufferFinalizeDiagnostic).write(row) };
        set_failure(failure::ARGUMENTS);
        let Some(s) = s else {
            return false;
        };
        if list.is_null()
            || list as u64 != s.list
            || s.finalized.load(SeqCst)
            || s.failed.load(SeqCst)
            || s.pending.load(SeqCst) != 0
            || s.cursor == 0
        {
            return false;
        }
        set_failure(failure::RETENTION);
        // SAFETY: a live list and the storage.
        if !unsafe { cvr_gpu_has_command_resource(list, raw) } {
            return false;
        }
        set_failure(failure::SOURCE);
        if !validate_recording(&s.recording.0, list, &s.context) {
            return false;
        }
        let read = if s.layout == ORIGINAL_INDICES { D3D12_RESOURCE_STATE_GENERIC_READ } else { D3D12_RESOURCE_STATE_COPY_SOURCE };
        let barrier = transition(s.resource, D3D12_RESOURCE_STATE_COPY_DEST, read);
        {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list recording; the barrier borrows the arena's resource.
            if let Some(l) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
                unsafe { l.ResourceBarrier(&[barrier]) };
            }
        }
        s.finalized.store(true, Release);
        if let Some(window) = &s.window {
            window.close();
        }
        set_failure(failure::NONE);
        true
    })
}

// ---- the receipt

#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    stage: AtomicU32,
    arena: *mut c_void,
    request: OwnedRequest,
    creation: Creation,
    parent: GraphParentRecord,
    epoch: ReadEpoch,
    scope: ScopeStamp,
    observed: Admission,
    native_context: Context,
    root: u64,
    base: u64,
    destination: u64,
    before_state: u32,
    required_state: u32,
    mutated: bool,
    reused: bool,
    reuse_token: Option<Arc<copy_windows::Entry>>,
    full_copy: Option<Arc<FullCopyRecord>>,
}
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: receipt_query, AddRef: receipt_add_ref, Release: receipt_release };
unsafe extern "system" fn receipt_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers.
    unsafe {
        *out = core::ptr::null_mut();
        if *iid != IID_IUNKNOWN && *iid != RECEIPT_TYPE {
            return E_NOINTERFACE;
        }
        *out = this;
        receipt_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn receipt_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live receipt.
    unsafe { (*this.cast::<Receipt>()).references.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn receipt_release(this: *mut c_void) -> u32 {
    // SAFETY: a live receipt; the last reference frees it.
    let left = unsafe { (*this.cast::<Receipt>()).references.fetch_sub(1, SeqCst) - 1 };
    if left == 0 {
        // SAFETY: made by Box::into_raw in prepare.
        let receipt = unsafe { Box::from_raw(this.cast::<Receipt>()) };
        let arena = receipt.arena;
        drop(receipt);
        release(arena);
    }
    left
}
/// A receipt reference this module holds.
struct Held(*mut Receipt);
impl Held {
    fn get(&self) -> &mut Receipt {
        // SAFETY: a live receipt (the C++ half used it without locking: one recording thread at a time).
        unsafe { &mut *self.0 }
    }
    fn raw(&self) -> *mut c_void {
        self.0.cast()
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the reference this holder owns.
        unsafe { receipt_release(self.0.cast()) };
    }
}
fn owner(proof: &PreparedCopy) -> Option<Held> {
    if proof.owner.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object the proof owns.
    unsafe {
        let vtbl = *(proof.owner as *const *const IUnknown_Vtbl);
        let _ = ((*vtbl).QueryInterface)(proof.owner, &RECEIPT_TYPE, &mut out);
    }
    (!out.is_null()).then(|| Held(out.cast()))
}
fn arena_of<'a>(owner: &Receipt) -> &'a mut Storage {
    // SAFETY: the receipt owns a storage reference (a separate object).
    unsafe { &mut *owner.arena.cast::<Storage>() }
}
fn public_proof(proof: &PreparedCopy, owner: &Receipt) -> bool {
    !owner.arena.is_null()
        && proof.list == owner.epoch.list
        && proof.generation == owner.epoch.generation
        && proof.occurrence == owner.epoch.occurrence
        && proof.source_offset == owner.request.source_offset
        && proof.private_offset == owner.destination
        && proof.copied_bytes == owner.request.bytes as u64
        && proof.reused == owner.reused
        && proof.gpu_copied_bytes == if owner.reused { 0 } else { owner.request.bytes as u64 }
}
fn span(value: &copy_windows::Span) -> Span {
    Span {
        resource: value.resource,
        gpu: value.gpu,
        backing: value.backing,
        offset: value.offset,
        bytes: value.bytes,
        destination: value.destination,
        occurrence: value.occurrence,
    }
}
fn reuse_valid(owner: &Receipt) -> bool {
    if !owner.reused {
        return true;
    }
    let arena = arena_of(owner);
    let (Some(original), Some(window), Some(token)) = (&owner.full_copy, &arena.window, &owner.reuse_token) else {
        return false;
    };
    if !copy_windows::valid(token) {
        return false;
    }
    let source = &owner.request.source.0;
    let (a, b) = (&original.recording.0, &source.recording);
    window.list() == owner.epoch.list
        && window.generation() == owner.epoch.generation
        && original.source == source.binding.resource
        && original.destination == arena.resource
        && !a.token.is_null()
        && a.token == b.token
        && a.list == b.list
        && a.generation == b.generation
        && a.context == b.context
        && a.registry == b.registry
        && rules::reuse_matches(
            &original.version(),
            &span(&token.span),
            owner.epoch.list,
            owner.epoch.generation,
            source.binding.resource as u64,
            owner.observed.gpu,
            owner.epoch.occurrence,
            arena.cursor,
            arena.capacity,
        )
}
fn physical(list: *mut c_void, owner: &Receipt, current: &mut Admission) -> bool {
    let request = &owner.request;
    let Some(creation) = query_creation(request.source.0.binding.resource) else {
        return false;
    };
    if !same_creation(&owner.creation, &creation) {
        return false;
    }
    observe(
        list,
        request.parameter,
        request.offset,
        request.source_offset,
        request.bytes,
        request.extent,
        &request.source.0,
        &creation,
        arena_of(owner).layout,
        current,
    ) && same_shape(&owner.observed, current)
        && current.creation_known
        && current.creation_kind == 1
        && current.creation_heap == 0
        && current.creation_offset == 0
        && current.creation_bytes == 0
        && current.unprotected
        && current.same_device
}
fn scope(list: *mut c_void, owner: &Receipt) -> bool {
    if list.is_null() || owner.arena.is_null() {
        return false;
    }
    let arena = arena_of(owner);
    scope_matches(&owner.scope, list as u64, owner.epoch.generation, owner.epoch.occurrence)
        && rules::epoch(&owner.epoch, list as u64, arena.generation, owner.scope.position)
        && !arena.failed.load(Acquire)
        && !arena.finalized.load(Acquire)
        && owner.base == image()
        && revalidate(&owner.parent)
}
/// Source: the held SRV's backing read again at the source, the same identity (source_srv::RevalidateAtSource).
fn source(list: *mut c_void, owner: &Receipt, current: &mut OwnedBacking) -> bool {
    let request = &owner.request;
    // SAFETY: the saved backing is the receipt's; `current` is an empty backing this module owns.
    unsafe {
        crate::source_srv_observe::cvr_ssrvo_revalidate_at_source(
            list as u64,
            owner.root,
            request.parameter,
            request.offset,
            &*request.source.0,
            &mut *current.0,
            clear_backing,
        ) == CAPTURED
    }
}
fn check(list: *mut c_void, owner: &Receipt, ready: bool) -> bool {
    set_failure(failure::SCOPE);
    if !scope(list, owner) {
        return false;
    }
    let mut current = OwnedBacking::empty();
    let mut shape = zeroed_admission();
    set_failure(failure::CHANGED);
    if !source(list, owner, &mut current) || !physical(list, owner, &mut shape) {
        return false;
    }
    let s = &current.0;
    set_failure(failure::STATE);
    if !rules::read_mask(s.state, s.native.kind, ready) {
        return false;
    }
    if ready {
        s.state.value & owner.required_state == owner.required_state
    } else {
        s.state.value == owner.before_state
    }
}
fn recovery_preflight(list: *mut c_void, owner: &Receipt, current: &mut Context) -> bool {
    let source = &owner.request.source.0;
    if !scope(list, owner)
        || !same_upload(&current_upload(owner.base), &source.context)
        || source.recording.list != list as u64
        || source.recording.context != source.context.context
        || source.recording.registry != source.native.registry
        || !validate_recording(&source.recording, list, &source.context)
        || !crate::descriptor_audit::matches_descriptor(stamp(source))
    {
        return false;
    }
    let read = scd::reader(Some(cvr_iab_read), core::ptr::null_mut());
    *current = scd::read_context(&read, source.context.context, source.context.list);
    let (found, record, address, native) =
        crate::source_srv::read_record(Some(cvr_iab_read), core::ptr::null_mut(), source.native.registry, source.context.context, source.native.handle);
    let mut shape = zeroed_admission();
    found
        && rules::recovery_identity(&source.native, &source.raw_record, &owner.native_context, source.record_address, &native, &record, current, address)
        && source.context.state == current.state
        && physical(list, owner, &mut shape)
        && same_upload(&current_upload(owner.base), &source.context)
        && validate_recording(&source.recording, list, &source.context)
}
fn stamp(source: &Backing) -> &crate::descriptor_audit::DescriptorStamp {
    // SAFETY: descriptors::DescriptorStamp as both modules mirror it (176 bytes, the same fields).
    unsafe { &*(&source.binding.stamp as *const crate::source_cbv_binding::DescriptorStamp).cast() }
}
fn register(base: u64, context: u64, handle: u32, required: u32) {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the engine's combined-read registration (the code contract is the source's, checked when it was read).
    unsafe {
        let register: unsafe extern "C" fn(*mut c_void, u32, u32) = core::mem::transmute((base + REGISTER_RVA) as usize);
        register(context as *mut c_void, handle, required);
    }
}
fn flush(base: u64, context: u64) {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the engine's context flush (as above).
    unsafe {
        let flush: unsafe extern "C" fn(*mut c_void) = core::mem::transmute((base + FLUSH_RVA) as usize);
        flush(context as *mut c_void);
    }
}
fn recover(list: *mut c_void, owner: &Receipt) -> bool {
    let mut context = Context::default();
    if !recovery_preflight(list, owner, &mut context) {
        return false;
    }
    if context.pending != 0 {
        flush(owner.base, context.address);
    }
    let mut current = OwnedBacking::empty();
    let mut shape = zeroed_admission();
    if !source(list, owner, &mut current) || !physical(list, owner, &mut shape) {
        return false;
    }
    let (state, native, context_address) = (current.0.state, current.0.native, current.0.context.context);
    let Some(required) = rules::recovery_mask(owner.before_state, state, native.kind) else {
        return false;
    };
    if state.value & required != required {
        register(owner.base, context_address, native.handle, required);
        flush(owner.base, context_address);
    }
    let mut again = OwnedBacking::empty();
    source(list, owner, &mut again)
        && physical(list, owner, &mut shape)
        && rules::read_mask(again.0.state, again.0.native.kind, false)
        && again.0.state.value & required == required
}
fn failed(owner: &Receipt) {
    let arena = arena_of(owner);
    arena.failed.store(true, Release);
    arena.pending.fetch_sub(1, SeqCst);
    owner.stage.store(stage::FAILED, Release);
}
fn guarded(f: impl FnOnce() -> bool) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            set_failure(failure::EXCEPTION);
            false
        }
    }
}
fn compatible() -> bool {
    crate::source_srv_observe::cvr_ssrvo_compatible()
        && crate::indirect_copy_native::cvr_ic_compatible()
        && crate::native_recording::cvr_native_recording_compatible()
}

// ---- the C++ API

/// ActualCallScope's constructor.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_scope_enter(list: u64, generation: u64, occurrence: u64, out: *mut ScopeStamp) {
    let stamp = SCOPES.with_borrow_mut(|s| s.enter(list, generation, occurrence, thread()));
    // SAFETY: the caller's output.
    unsafe { *out = stamp };
}
/// ActualCallScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_scope_exit(stamp: *const ScopeStamp) {
    // SAFETY: the caller's stamp.
    let stamp = unsafe { *stamp };
    if stamp.nonce != 0 {
        SCOPES.with_borrow_mut(|s| s.exit(&stamp));
    }
}
/// ActualCallScope::Active.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_scope_active(stamp: *const ScopeStamp) -> bool {
    // SAFETY: the caller's stamp.
    let stamp = unsafe { *stamp };
    scope_matches(&stamp, stamp.list, stamp.generation, stamp.position)
}
/// LastFailure.
#[no_mangle]
pub extern "C-unwind" fn cvr_sbsn_last_failure() -> u32 {
    FAILURE.get()
}
/// Compatible.
#[no_mangle]
pub extern "C-unwind" fn cvr_sbsn_compatible() -> bool {
    compatible()
}
/// PrepareAtActualCall: on success `*output` holds a new receipt reference.
///
/// # Safety
/// `request`, `parent` and `epoch` are valid; `arena` is the SnapshotArena's storage or null; `output` is the caller's
/// PreparedCopy.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_prepare(
    list: *mut c_void,
    root: u64,
    request: *const Request,
    parent: *const GraphParentRecord,
    epoch: *const ReadEpoch,
    arena: *mut c_void,
    output: *mut PreparedCopy,
) -> bool {
    // SAFETY: as documented.
    let (request, parent, epoch, output) = unsafe { (&*request, &*parent, &*epoch, &mut *output) };
    guarded(|| prepare(list, root, request, parent, epoch, arena, output))
}
fn prepare(list: *mut c_void, root: u64, request: &Request, parent: &GraphParentRecord, epoch: &ReadEpoch, arena: *mut c_void, output: &mut PreparedCopy) -> bool {
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return false;
    }
    set_failure(failure::ARGUMENTS);
    let Some(storage_ref) = storage(arena) else {
        return false;
    };
    if !output.owner.is_null()
        || list.is_null()
        || root == 0
        || storage_ref.list != list as u64
        || storage_ref.generation != epoch.generation
        || storage_ref.thread != thread()
        || storage_ref.failed.load(SeqCst)
        || storage_ref.finalized.load(SeqCst)
        || request.source.recording.list != epoch.list
        || request.source.recording.generation != epoch.generation
    {
        return false;
    }
    set_failure(failure::SCOPE);
    let scope_now = SCOPES.with_borrow(|s| s.current());
    if !scope_matches(&scope_now, list as u64, epoch.generation, epoch.occurrence)
        || !rules::epoch(epoch, list as u64, storage_ref.generation, scope_now.position)
    {
        return false;
    }
    set_failure(failure::CODE);
    if !compatible() {
        return false;
    }
    set_failure(failure::PARENT);
    if !crate::indirect_copy::parent(parent) || !revalidate(parent) {
        return false;
    }
    let held = Held(Box::into_raw(Box::new(Receipt {
        vtbl: &RECEIPT_VTBL,
        references: AtomicU32::new(1),
        stage: AtomicU32::new(stage::PREPARED),
        arena: add_ref(arena),
        request: OwnedRequest::copy(request),
        creation: Creation::default(),
        parent: *parent,
        epoch: *epoch,
        scope: scope_now,
        observed: zeroed_admission(),
        native_context: Context::default(),
        root,
        base: image(),
        destination: 0,
        before_state: 0,
        required_state: 0,
        mutated: false,
        reused: false,
        reuse_token: None,
        full_copy: None,
    })));
    let o = held.get();
    let arena = arena_of(o);
    let mut current = OwnedBacking::empty();
    set_failure(failure::SOURCE);
    if !source(list, o, &mut current) {
        return false;
    }
    let s = &current.0;
    let read = scd::reader(Some(cvr_iab_read), core::ptr::null_mut());
    o.native_context = scd::read_context(&read, s.context.context, s.context.list);
    if o.native_context.observation != KNOWN || o.native_context.pending != 0 || o.native_context.submitted != 0 || o.native_context.state != s.context.state {
        return false;
    }
    set_failure(failure::CREATION);
    match query_creation(request.source.binding.resource) {
        Some(creation) => o.creation = creation,
        None => return false,
    }
    set_failure(failure::BACKING);
    let mut observed = zeroed_admission();
    if !observe(list, request.parameter, request.offset, request.source_offset, request.bytes, request.extent, s, &o.creation, arena.layout, &mut observed) {
        return false;
    }
    o.observed = observed;
    let admitted = rules::admit(&o.observed) as u32;
    set_failure(admitted);
    if admitted != failure::NONE {
        return false;
    }
    o.before_state = s.state.value;
    o.required_state = o.before_state | COPY_SOURCE;
    let mut end = arena.cursor;
    set_failure(failure::CAPACITY);
    if arena.layout == ORIGINAL_INDICES && (o.observed.view_offset != 0 || o.observed.view_bytes != SOURCE_BYTES) {
        return false;
    }
    if request.extent == FULL_ORIGINAL_VIEW {
        if arena.window.is_none() {
            arena.full_copies.reserve(MAX_FULL_COPIES);
            arena.window = copy_windows::create(epoch.list, epoch.generation);
            if arena.window.is_none() {
                return false;
            }
        }
        o.reuse_token = arena.window.as_ref().and_then(|w| w.find(o.observed.resource, 0, SOURCE_BYTES, epoch.occurrence));
        if let Some(token) = &o.reuse_token {
            let token_span = span(&token.span);
            o.full_copy = arena
                .full_copies
                .iter()
                .find(|candidate| {
                    rules::reuse_matches(
                        &candidate.version(),
                        &token_span,
                        epoch.list,
                        epoch.generation,
                        o.observed.resource,
                        o.observed.gpu,
                        epoch.occurrence,
                        arena.cursor,
                        arena.capacity,
                    )
                })
                .cloned();
            set_failure(failure::CHANGED);
            let Some(full) = &o.full_copy else {
                return false;
            };
            o.reused = true;
            o.destination = full.version().destination;
            if !reuse_valid(o) {
                return false;
            }
        }
    }
    if !o.reused {
        set_failure(failure::CAPACITY);
        let Some((destination, next, _view_end)) =
            rules::reserve_extent(request.extent, arena.layout, arena.cursor, arena.capacity, request.source_offset, request.bytes as u64)
        else {
            return false;
        };
        o.destination = destination;
        end = next;
        if request.extent == FULL_ORIGINAL_VIEW {
            if arena.full_copies.len() >= MAX_FULL_COPIES {
                return false;
            }
            o.full_copy = Some(Arc::new(FullCopyRecord {
                version: Mutex::new(FullVersion {
                    list: epoch.list,
                    generation: epoch.generation,
                    resource: o.observed.resource,
                    gpu: o.observed.gpu,
                    destination: o.destination,
                    occurrence: epoch.occurrence,
                    recorded: false,
                }),
                recording: OwnedRecording::copy(&s.recording),
                source: add_ref(s.binding.resource),
                destination: add_ref(arena.resource),
            }));
        }
    }
    if !check(list, o, false) {
        return false;
    }
    set_failure(failure::CHANGED);
    if !reuse_valid(o) {
        return false;
    }
    set_failure(failure::RETENTION);
    let keep = [held.raw(), o.arena];
    // SAFETY: a live list; both objects are COM objects this receipt owns.
    if !unsafe { cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
        return false;
    }
    if let (Some(full), false) = (&o.full_copy, o.reused) {
        arena.full_copies.push(full.clone());
    }
    arena.recording = OwnedRecording::copy(&s.recording);
    arena.context = s.context;
    arena.cursor = end;
    arena.pending.fetch_add(1, SeqCst);
    // SAFETY: the receipt; the output takes its own reference.
    unsafe { receipt_add_ref(held.raw()) };
    *output = PreparedCopy {
        owner: held.raw(),
        list: epoch.list,
        generation: epoch.generation,
        occurrence: epoch.occurrence,
        source_offset: request.source_offset,
        private_offset: o.destination,
        copied_bytes: request.bytes as u64,
        reused: o.reused,
        gpu_copied_bytes: if o.reused { 0 } else { request.bytes as u64 },
    };
    set_failure(failure::NONE);
    true
}
fn validate(list: *mut c_void, proof: &PreparedCopy) -> bool {
    set_failure(failure::ARGUMENTS);
    let Some(held) = owner(proof) else {
        return false;
    };
    let o = held.get();
    if !public_proof(proof, o) {
        return false;
    }
    let now = o.stage.load(Acquire);
    if now != stage::PREPARED && now != stage::RECORDED {
        return false;
    }
    set_failure(failure::CHANGED);
    if now == stage::PREPARED && !reuse_valid(o) {
        return false;
    }
    set_failure(failure::RETENTION);
    // SAFETY: a live list and the receipt.
    if !unsafe { cvr_gpu_has_command_resource(list, held.raw()) } {
        return false;
    }
    if !check(list, o, now == stage::RECORDED) {
        return false;
    }
    set_failure(failure::NONE);
    true
}
/// ValidateAtActualCall.
///
/// # Safety
/// `proof` is valid; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_validate(list: *mut c_void, proof: *const PreparedCopy) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| validate(list, proof))
}
/// RecordAtActualCall: on success `*output` (empty) the private range (new references).
///
/// # Safety
/// `proof` is valid; `output` is the caller's PrivateRange; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_record(list: *mut c_void, proof: *const PreparedCopy, output: *mut PrivateRange) -> bool {
    // SAFETY: as documented.
    let (proof, output) = unsafe { (&*proof, &mut *output) };
    let mut held: Option<Held> = None;
    let mut recording = false;
    let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record_at_call(list, proof, output, &mut held, &mut recording)));
    match body {
        Ok(done) => done,
        Err(_) => {
            set_failure(failure::EXCEPTION);
            if let (true, Some(h)) = (recording, held.as_ref()) {
                let o = h.get();
                if o.mutated {
                    let recovered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recover(list, o))).unwrap_or(false);
                    set_failure(if recovered { failure::RECOVERY_VERIFIED } else { failure::RECOVERY_FATAL });
                }
                failed(o);
            }
            false
        }
    }
}
fn record_at_call(list: *mut c_void, proof: &PreparedCopy, output: &mut PrivateRange, held: &mut Option<Held>, recording: &mut bool) -> bool {
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return false;
    }
    set_failure(failure::ARGUMENTS);
    *held = owner(proof);
    let Some(h) = held.as_ref() else {
        return false;
    };
    let o = h.get();
    if !public_proof(proof, o) || !output.resource.is_null() || !output.owner.is_null() {
        return false;
    }
    let arena = arena_of(o);
    if !validate(list, proof) {
        return false;
    }
    set_failure(failure::ALREADY_RECORDED);
    if o.stage.compare_exchange(stage::PREPARED, stage::RECORDING, SeqCst, SeqCst).is_err() {
        return false;
    }
    *recording = true;
    let mut current = OwnedBacking::empty();
    set_failure(failure::CHANGED);
    let mut okay = scope(list, o)
        && source(list, o, &mut current)
        && rules::read_mask(current.0.state, current.0.native.kind, false)
        && current.0.state.value & o.before_state == o.before_state;
    if okay {
        o.required_state = current.0.state.value | COPY_SOURCE;
        if current.0.state.value & COPY_SOURCE == 0 {
            set_failure(failure::REGISTRATION);
            o.mutated = true;
            register(o.base, current.0.context.context, current.0.native.handle, o.required_state);
            flush(o.base, current.0.context.context);
        }
        okay = check(list, o, true);
    }
    if okay && !reuse_valid(o) {
        set_failure(failure::CHANGED);
        okay = false;
    }
    if !okay {
        if o.mutated {
            set_failure(if recover(list, o) { failure::RECOVERY_VERIFIED } else { failure::RECOVERY_FATAL });
        }
        failed(o);
        return false;
    }
    if !o.reused {
        {
            let _internal = cvr_host::probe::Internal::new();
            let source_resource = o.request.source.0.binding.resource;
            // SAFETY: a live list; the arena's resource and the request's source are owned references.
            if let (Some(l), Some(destination), Some(source_resource)) = (
                unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) },
                unsafe { ID3D12Resource::from_raw_borrowed(&arena.resource) },
                unsafe { ID3D12Resource::from_raw_borrowed(&source_resource) },
            ) {
                unsafe { l.CopyBufferRegion(destination, o.destination, source_resource, o.request.source_offset, o.request.bytes as u64) };
            }
        }
        if let Some(full) = &o.full_copy {
            full.version.lock().unwrap_or_else(|e| e.into_inner()).recorded = true;
            if let Some(window) = &arena.window {
                window.publish(copy_windows::Span {
                    resource: o.observed.resource,
                    gpu: o.observed.gpu,
                    backing: SOURCE_BYTES,
                    offset: 0,
                    bytes: SOURCE_BYTES,
                    destination: o.destination,
                    occurrence: o.epoch.occurrence,
                });
            }
        }
    }
    *output = PrivateRange {
        resource: add_ref(arena.resource),
        owner: add_ref(o.arena),
        source_offset: o.request.source_offset,
        private_offset: o.destination,
        copied_bytes: o.request.bytes as u64,
    };
    arena.pending.fetch_sub(1, SeqCst);
    o.stage.store(stage::RECORDED, Release);
    set_failure(failure::NONE);
    true
}
/// RecordedView: `*output` the source's SRV over the private copy at the source's own indices.
///
/// # Safety
/// `proof` and `range` are valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_recorded_view(
    proof: *const PreparedCopy,
    range: *const PrivateRange,
    output: *mut D3D12_SHADER_RESOURCE_VIEW_DESC,
) -> bool {
    // SAFETY: as documented.
    let (proof, range, output) = unsafe { (&*proof, &*range, &mut *output) };
    *output = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let held = owner(proof)?;
        let o = held.get();
        if !public_proof(proof, o) || o.stage.load(SeqCst) != stage::RECORDED {
            return None;
        }
        let arena = arena_of(o);
        if arena.failed.load(SeqCst)
            || arena.layout != ORIGINAL_INDICES
            || range.resource != arena.resource
            || range.owner != o.arena
            || range.source_offset != proof.source_offset
            || range.private_offset != proof.private_offset
            || range.copied_bytes != proof.copied_bytes
            || range.private_offset < range.source_offset
            || (range.private_offset - range.source_offset) % STRIDE != 0
        {
            return None;
        }
        let first = range.private_offset - range.source_offset;
        if first > arena.capacity || SOURCE_BYTES > arena.capacity - first {
            return None;
        }
        let mut view = o.observed.view;
        view.Anonymous.Buffer.FirstElement = first / STRIDE;
        Some(view)
    }));
    match made {
        Ok(Some(view)) => {
            *output = view;
            true
        }
        _ => false,
    }
}
/// ValidateSourceSubmission.
///
/// # Safety
/// `proof` is valid; `queue` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sbsn_validate_submission(proof: *const PreparedCopy, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let held = owner(proof);
        let Some(h) = held.as_ref().filter(|_| !queue.is_null()) else {
            return false;
        };
        let o = h.get();
        if !public_proof(proof, o) || o.stage.load(SeqCst) != stage::RECORDED {
            return false;
        }
        let arena = arena_of(o);
        if !arena.finalized.load(Acquire) || arena.failed.load(Acquire) {
            return false;
        }
        let source = &o.request.source.0;
        set_failure(failure::SOURCE);
        // SAFETY: the request's lease and a live queue.
        if !unsafe { crate::native_recording::cvr_native_recording_validate_submission(&source.recording, queue) } {
            return false;
        }
        set_failure(failure::CHANGED);
        if !crate::descriptor_audit::matches_descriptor(stamp(source)) {
            return false;
        }
        let raw = source.binding.resource;
        set_failure(failure::CREATION);
        let Some(creation) = query_creation(raw) else {
            return false;
        };
        // SAFETY: the request's owned resource.
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
            return false;
        };
        if !same_creation(&o.creation, &creation) || !unprotected(resource) {
            return false;
        }
        let mut current = o.observed;
        let mut flags = D3D12_HEAP_FLAGS::default();
        // SAFETY: a live resource; the outputs are locals.
        unsafe {
            current.desc = resource.GetDesc();
            if resource.GetHeapProperties(Some(&mut current.heap), Some(&mut flags)).is_err() {
                return false;
            }
            current.heap_flags = flags.0 as u32;
            current.gpu = resource.GetGPUVirtualAddress();
        }
        set_failure(failure::CHANGED);
        if !same_shape(&o.observed, &current) {
            return false;
        }
        set_failure(failure::NONE);
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_proof() -> PreparedCopy {
        PreparedCopy {
            owner: core::ptr::null_mut(),
            list: 0,
            generation: 0,
            occurrence: 0,
            source_offset: 0,
            private_offset: 0,
            copied_bytes: 0,
            reused: false,
            gpu_copied_bytes: 0,
        }
    }

    #[test]
    fn proofs_without_an_owner_are_refused() {
        let proof = empty_proof();
        // SAFETY: a local proof; nothing is read without an owner.
        assert!(!unsafe { cvr_sbsn_validate(core::ptr::null_mut(), &proof) });
        assert_eq!(cvr_sbsn_last_failure(), failure::ARGUMENTS);
        assert!(!unsafe { cvr_sbsn_validate_submission(&proof, core::ptr::null_mut()) });
        let range = PrivateRange { resource: core::ptr::null_mut(), owner: core::ptr::null_mut(), source_offset: 0, private_offset: 0, copied_bytes: 0 };
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
        assert!(!unsafe { cvr_sbsn_recorded_view(&proof, &range, &mut view) });
    }

    #[test]
    fn arenas_refuse_bad_arguments() {
        let mut out = core::ptr::null_mut();
        // SAFETY: null objects are refused before use.
        let hr = unsafe { cvr_sbsn_arena_create(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 1, 16, 0, &mut out) };
        assert_eq!(hr, E_INVALIDARG);
        assert!(out.is_null());
        // SAFETY: no storage.
        unsafe {
            assert!(!cvr_sbsn_arena_finalized(core::ptr::null_mut()));
            assert_eq!(cvr_sbsn_arena_bytes(core::ptr::null_mut()), 0);
            assert!(cvr_sbsn_arena_resource(core::ptr::null_mut()).is_null());
            assert!(!cvr_sbsn_arena_finalize(core::ptr::null_mut(), core::ptr::null_mut()));
        }
        assert_eq!(cvr_sbsn_last_failure(), failure::ARGUMENTS);
    }

    #[test]
    fn only_the_newest_scope_is_active() {
        let mut outer = ScopeStamp::default();
        let mut inner = ScopeStamp::default();
        // SAFETY: local stamps.
        unsafe {
            cvr_sbsn_scope_enter(7, 2, 1, &mut outer);
            assert!(cvr_sbsn_scope_active(&outer));
            cvr_sbsn_scope_enter(7, 2, 2, &mut inner);
            assert!(cvr_sbsn_scope_active(&inner));
            cvr_sbsn_scope_exit(&inner);
            // A nested callback ended the outer scope's claim (the C++ ledger's rule).
            assert!(!cvr_sbsn_scope_active(&outer));
            cvr_sbsn_scope_exit(&outer);
            assert!(!cvr_sbsn_scope_active(&outer));
        }
    }
}
