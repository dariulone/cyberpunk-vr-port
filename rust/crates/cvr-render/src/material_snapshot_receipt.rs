//! THE NATIVE MATERIAL BUFFER SNAPSHOT (formerly src/Render/NativeMaterialBufferSnapshot.cpp): one held material SRV's
//! source buffer (a default-heap native buffer, committed, placed or a fixed geometry pool's slot) copied at the actual
//! call into a private 8 MiB arena (one per recording, finalized to GENERIC_READ); the receipt that pins the source's
//! allocation and allocator and keeps its revalidated backing; the physical identity re-read at the call and at
//! submission; and the private view the copy is read through once its fence has passed. The rules are
//! material_buffer_snapshot.rs'; the source is read again through source_srv_observe.rs.
//!
//! The C++ API (include/Render/NativeMaterialBufferSnapshot.hpp) forwards here; the graph parent's revalidation (still
//! C++, NativePreparation.cpp) is the attached host's.
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering::{Relaxed, SeqCst}};

use windows::core::{IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::ia_binding::cvr_iab_read;
use crate::ia_snapshot::GeometryPool;
use crate::indirect_copy::{GraphParentRecord, ScopeLedger, ScopeStamp};
use crate::native_recording::{Context as UploadContext, Lease as Recording};
use crate::source_cbv_details::{self as scd, Allocation, Allocator, ReadState, KNOWN};
use crate::source_srv_observe::Backing;
use crate::structured_snapshot_receipt::{
    add_ref, clear_backing, current_upload, query_creation, release, transition, unprotected, validate_recording, Creation, OwnedBacking,
    OwnedRecording,
};
use crate::texture_immutable::PlacedAllocation;

#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
use crate::declared_moved_exports::CyberpunkVR_StructuredBufferSnapshotsEnable;
/// The unit tests' switch (the plugin's and the probe tests' is declared_moved_exports').
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static CyberpunkVR_StructuredBufferSnapshotsEnable: AtomicU32 = AtomicU32::new(0);

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
}

/// material_buffer_snapshot::Failure.
mod failure {
    pub const NONE: u32 = 0;
    pub const DISABLED: u32 = 1;
    pub const ARGUMENTS: u32 = 2;
    pub const SCOPE: u32 = 3;
    pub const CREATION: u32 = 5;
    pub const ALLOCATION: u32 = 6;
    pub const POOL: u32 = 7;
    pub const CAPACITY: u32 = 9;
    pub const CHANGED: u32 = 10;
    pub const RETENTION: u32 = 11;
    pub const EXCEPTION: u32 = 12;
}
const CAPACITY: u64 = 8 * 1024 * 1024;
const ALIGNMENT: u64 = 128;
const ROOT_PARAMETER: u32 = 23;
const CAPTURED: u32 = 1;
const POOL_LOW_TYPE: u8 = 13;
const GENERIC_READ: u32 = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
const RECEIPT_TYPE: GUID = GUID::from_values(0x94ab56f3, 0x848e, 0x4c71, [0xa2, 0x6f, 0x92, 0xd0, 0x7a, 0xa1, 0x80, 0x57]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

thread_local! {
    static SCOPES: RefCell<ScopeLedger> = RefCell::new(ScopeLedger::default());
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
fn set_failure(value: u32) {
    FAILURE.set(value);
}
fn thread() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() }
}
fn base() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}

// ---- the C++ half's values and its host

/// material_buffer_snapshot::Epoch.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Epoch {
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub certified: bool,
}
/// material_buffer_snapshot::Copy (the owner and the resource owned references).
#[repr(C)]
pub struct Copy {
    pub owner: *mut c_void,
    pub resource: *mut c_void,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub source_offset: u64,
    pub offset: u64,
    pub bytes: u64,
}
const _: () = assert!(core::mem::size_of::<Epoch>() == 32 && core::mem::size_of::<Copy>() == 80 && core::mem::offset_of!(Copy, source_offset) == 56);
/// preparation::RevalidateGraphParent (preparation_trace.rs).
fn revalidate(parent: &GraphParentRecord) -> bool {
    crate::preparation_trace::revalidate_graph_parent(parent)
}
fn same_recording(a: &Recording, b: &Recording) -> bool {
    // SAFETY: both leases have source_cbv_snapshot's RecordingLease layout (40 bytes, the token as an address).
    unsafe { crate::material_buffer_snapshot::cvr_mbs_same_recording((a as *const Recording).cast(), (b as *const Recording).cast()) }
}
fn private_view(source: &Backing, offset: u64, bytes: u64) -> Option<D3D12_SHADER_RESOURCE_VIEW_DESC> {
    crate::material_buffer_snapshot::private_view(&source.binding.view, source.shader_register, offset, bytes)
}

// ---- the arena

#[repr(C)]
struct Storage {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    device: *mut c_void,
    resource: *mut c_void,
    recording: OwnedRecording,
    context: UploadContext,
    bytes: u64,
    finalized: bool,
    failed: bool,
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
/// Arena::Create: S_OK with `*out` a new storage reference.
///
/// # Safety
/// `existing` is the arena's storage or null; `device` and `list` are live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_arena_create(
    existing: *mut c_void,
    device: *mut c_void,
    list: *mut c_void,
    generation: u64,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the caller's output.
    unsafe { *out = core::ptr::null_mut() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena_create(existing, device, list, generation))) {
        Ok(Ok(raw)) => {
            // SAFETY: as above.
            unsafe { *out = raw };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_FAIL,
    }
}
fn arena_create(existing: *mut c_void, device: *mut c_void, list: *mut c_void, generation: u64) -> Result<*mut c_void, HRESULT> {
    // SAFETY: live objects or null.
    let (Some(d), Some(l)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) })
    else {
        return Err(E_INVALIDARG);
    };
    // SAFETY: live objects.
    if !existing.is_null() || generation == 0 || unsafe { d.GetNodeCount() } != 1 || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
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
        device: add_ref(device),
        resource: core::ptr::null_mut(),
        recording: OwnedRecording::none(),
        context: current_upload(base()),
        bytes: 0,
        finalized: false,
        failed: false,
    }));
    let raw: *mut c_void = next.cast();
    let fail = move |hr: HRESULT| {
        // SAFETY: the one reference made above.
        unsafe { storage_release(raw) };
        Err(hr)
    };
    // SAFETY: the new storage, not yet shared.
    let s = unsafe { &mut *next };
    let mut lease = Recording { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
    // SAFETY: a live list; the context and the lease are the storage's and a local (its token an owned reference).
    let captured = unsafe { crate::native_recording::cvr_native_recording_capture(list, &s.context, &mut lease) };
    s.recording = OwnedRecording(lease);
    if !captured || s.recording.0.generation != generation {
        return fail(E_ACCESSDENIED);
    }
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: CAPACITY,
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
    match (created, resource) {
        (Ok(()), Some(resource)) => {
            s.resource = resource.into_raw();
            Ok(raw)
        }
        (Err(e), _) => fail(e.code()),
        (Ok(()), None) => fail(E_FAIL),
    }
}
/// Arena::Abort.
///
/// # Safety
/// `raw` is the arena's storage or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_arena_abort(raw: *mut c_void) {
    if let Some(s) = storage(raw) {
        s.failed = true;
    }
}
/// Arena::Bytes.
///
/// # Safety
/// As cvr_mbsn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_arena_bytes(raw: *mut c_void) -> u64 {
    storage(raw).map_or(0, |s| s.bytes)
}
/// Arena::Finalize: the arena kept with the list and turned GENERIC_READ.
///
/// # Safety
/// As cvr_mbsn_arena_abort; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_arena_finalize(raw: *mut c_void, list: *mut c_void) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(s) = storage(raw) else {
            return false;
        };
        let keep = [raw];
        // SAFETY: a live list and the storage.
        if s.failed
            || s.finalized
            || s.bytes == 0
            || !validate_recording(&s.recording.0, list, &s.context)
            || !unsafe { cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) }
        {
            return false;
        }
        let barrier = transition(s.resource, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_GENERIC_READ);
        {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list recording; the barrier borrows the arena's resource.
            if let Some(l) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
                unsafe { l.ResourceBarrier(&[barrier]) };
            }
        }
        s.finalized = true;
        true
    }))
    .unwrap_or(false)
}

// ---- the receipt

#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    arena: *mut c_void,
    creation: Creation,
    source: OwnedBacking,
    allocation: Allocation,
    allocator: Allocator,
    pin_allocator: *mut c_void,
    pin_allocation: *mut c_void,
    pool: GeometryPool,
    placement: PlacedAllocation,
    parent: GraphParentRecord,
    epoch: Epoch,
    root: u64,
    offset: u64,
    bytes: u64,
    recorded: bool,
    validated: bool,
    complete: bool,
    queue: *mut c_void,
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
        // SAFETY: made by Box::into_raw in capture.
        let receipt = unsafe { Box::from_raw(this.cast::<Receipt>()) };
        // The C++ member order reversed: the queue, the pins (the allocation first), the creation and the source, the arena.
        release(receipt.queue);
        release(receipt.pin_allocation);
        release(receipt.pin_allocator);
        let arena = receipt.arena;
        drop(receipt);
        release(arena);
    }
    left
}
struct Held(*mut Receipt);
impl Held {
    fn get(&self) -> &mut Receipt {
        // SAFETY: a live receipt (one recording thread at a time, as the C++ used it).
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
fn arena_of<'a>(owner: &Receipt) -> &'a mut Storage {
    // SAFETY: the receipt owns a storage reference (a separate object).
    unsafe { &mut *owner.arena.cast::<Storage>() }
}
/// SameAllocation: both known and alike in every identity field.
fn same_allocation(a: &Allocation, b: &Allocation) -> bool {
    a.observation == KNOWN
        && b.observation == KNOWN
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.resource == b.resource
        && a.heap == b.heap
        && a.bytes == b.bytes
        && a.block == b.block
        && a.handle == b.handle
        && a.kind == b.kind
}
fn read_fixed_pool(source: &Backing, out: &mut GeometryPool) -> bool {
    // SAFETY: the backing's native buffer and record; `out` is the caller's.
    unsafe {
        crate::ia_snapshot_receipt::cvr_iasn_read_fixed_pool(
            &source.native,
            &source.raw_record,
            source.gpu,
            source.backing_bytes,
            source.gpu + source.source_offset,
            source.view_bytes,
            POOL_LOW_TYPE,
            out,
        )
    }
}
fn placed(allocation: &Allocation) -> PlacedAllocation {
    PlacedAllocation {
        allocation: allocation.address,
        resource: allocation.resource,
        pimpl: allocation.pimpl,
        block: allocation.block,
        handle: allocation.handle,
        heap: allocation.heap,
        bytes: allocation.bytes,
        ..Default::default()
    }
}
fn physical(owner: &Receipt) -> bool {
    let source = &owner.source.0;
    let saved = &owner.creation.0;
    if owner.pin_allocation.is_null() || owner.pin_allocator.is_null() {
        return false;
    }
    let Some(creation) = query_creation(source.binding.resource) else {
        return false;
    };
    let c = &creation.0;
    if c.resource != saved.resource
        || c.kind != saved.kind
        || c.heap != saved.heap
        || c.heap_offset != saved.heap_offset
        || c.allocation_bytes != saved.allocation_bytes
        || !c.initial_state_known
        || c.initial_state != saved.initial_state
        || (c.initial_state != D3D12_RESOURCE_STATE_COMMON.0 as u32 && c.initial_state != GENERIC_READ)
    {
        return false;
    }
    let read = scd::reader(Some(cvr_iab_read), core::ptr::null_mut());
    let allocation = scd::read_allocation(&read, base(), owner.pin_allocation as u64);
    let allocator = scd::read_allocator(&read, base(), source.native.registry, &allocation);
    if !same_allocation(&owner.allocation, &allocation)
        || allocator.observation != KNOWN
        || !allocator.pimpl_matches
        || allocator.address != owner.allocator.address
        || allocator.vtable != owner.allocator.vtable
        || allocator.pimpl != owner.allocator.pimpl
        || allocator.address != owner.pin_allocator as u64
        || allocation.resource != source.binding.resource as u64
        || allocation.bytes < source.backing_bytes
    {
        return false;
    }
    if owner.pool.owner != 0 {
        let mut current = GeometryPool::default();
        if !read_fixed_pool(source, &mut current)
            || current != owner.pool
            || allocation.address != current.owner
            || allocation.kind != 0
            || allocation.heap != 0
            || c.kind != 1
            || !c.heap.is_null()
            || c.heap_offset != 0
            || c.allocation_bytes != 0
        {
            return false;
        }
    } else if c.kind == 2 {
        let expected = placed(&allocation);
        let mut current = PlacedAllocation::default();
        // SAFETY: plain values.
        if allocation.kind != 1
            || c.heap.is_null()
            || allocation.heap != c.heap as u64
            || allocation.bytes != c.allocation_bytes
            || !unsafe { crate::ia_snapshot_receipt::cvr_iasn_read_pinned_placed_offset(&expected, &mut current) }
            || current.offset != c.heap_offset
            || current != owner.placement
        {
            return false;
        }
        // SAFETY: an owned heap reference.
        let Some(heap) = (unsafe { ID3D12Heap::from_raw_borrowed(&c.heap) }).map(|h| unsafe { h.GetDesc() }) else {
            return false;
        };
        if heap.Properties.Type != D3D12_HEAP_TYPE_DEFAULT || c.heap_offset > heap.SizeInBytes || c.allocation_bytes > heap.SizeInBytes - c.heap_offset {
            return false;
        }
    } else if c.kind != 1 || allocation.kind != 0 || allocation.heap != 0 || !c.heap.is_null() || c.heap_offset != 0 || c.allocation_bytes != 0 {
        return false;
    }
    let raw = source.binding.resource;
    // SAFETY: the backing's owned resource.
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return false;
    };
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    let allowed = (D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES.0 | D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES.0) as u32;
    // SAFETY: as above; the outputs are locals.
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width == source.backing_bytes
        && desc.Flags.0 == 0
        && desc.Height == 1
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.Format == DXGI_FORMAT_UNKNOWN
        && desc.Layout == D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && (desc.Alignment == 0 || desc.Alignment == D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT as u64)
        && unsafe { resource.GetGPUVirtualAddress() } == source.gpu
        && unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type == D3D12_HEAP_TYPE_DEFAULT
        && heap.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_UNKNOWN
        && heap.MemoryPoolPreference == D3D12_MEMORY_POOL_UNKNOWN
        && heap.CreationNodeMask <= 1
        && heap.VisibleNodeMask <= 1
        && flags.0 as u32 & !allowed == 0
        && unprotected(resource)
}
fn current(list: *mut c_void, owner: &Receipt) -> bool {
    let source = &owner.source.0;
    let mut now = OwnedBacking::empty();
    // SAFETY: the saved backing is the receipt's; `now` is an empty backing this module owns.
    if !revalidate(&owner.parent)
        || unsafe {
            crate::source_srv_observe::cvr_ssrvo_revalidate_at_source(
                list as u64,
                owner.root,
                ROOT_PARAMETER,
                source.shader_register,
                &**source,
                &mut *now.0,
                clear_backing,
            )
        } != CAPTURED
        || !physical(owner)
    {
        return false;
    }
    let lease = crate::ia_snapshot_same::Lease {
        resource: source.binding.resource,
        gpu: source.gpu,
        bytes: source.backing_bytes,
        heap_type: source.heap_type,
        flags: source.flags,
        native: source.native,
    };
    let mut state = ReadState::default();
    // SAFETY: a live list; the lease borrows the backing's resource for the call.
    unsafe { crate::ia_binding::cvr_iab_read_state(list, (&lease as *const crate::ia_snapshot_same::Lease).cast(), &mut state) };
    state.kind == 1 && state.status == 8 && state.value == GENERIC_READ
}
/// Owner: the copy's receipt, recorded, with the copy exactly what it recorded.
fn owner(copy: &Copy) -> Option<Held> {
    if copy.owner.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object the copy owns.
    unsafe {
        let vtbl = *(copy.owner as *const *const IUnknown_Vtbl);
        let _ = ((*vtbl).QueryInterface)(copy.owner, &RECEIPT_TYPE, &mut out);
    }
    let held = (!out.is_null()).then(|| Held(out.cast()))?;
    let o = held.get();
    if !o.recorded
        || o.arena.is_null()
        || copy.resource != arena_of(o).resource
        || copy.offset != o.offset
        || copy.bytes != o.bytes
        || copy.source_offset != o.source.0.source_offset
    {
        return None;
    }
    let expected = private_view(&o.source.0, o.offset, o.bytes)?;
    crate::material_buffer_snapshot::same_buffer_view(&expected, &copy.view).then_some(held)
}

// ---- the C++ API

/// ActualCallScope's constructor (no stamp for the port's own commands).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_scope_enter(list: u64, generation: u64, occurrence: u64, out: *mut ScopeStamp) {
    let stamp = if cvr_host::probe::internal() {
        ScopeStamp::default()
    } else {
        SCOPES.with_borrow_mut(|s| s.enter(list, generation, occurrence, thread()))
    };
    // SAFETY: the caller's output.
    unsafe { *out = stamp };
}
/// ActualCallScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_scope_exit(stamp: *const ScopeStamp) {
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
pub unsafe extern "C-unwind" fn cvr_mbsn_scope_active(stamp: *const ScopeStamp) -> bool {
    // SAFETY: the caller's stamp.
    let stamp = unsafe { *stamp };
    SCOPES.with_borrow(|s| s.matches(&stamp, stamp.list, stamp.generation, stamp.position, thread()))
}
/// LastFailure.
#[no_mangle]
pub extern "C-unwind" fn cvr_mbsn_last_failure() -> u32 {
    FAILURE.get()
}
/// Capture: on success `*output` (empty) the copy with a new receipt reference.
///
/// # Safety
/// `source`, `parent` and `epoch` are valid; `arena` is the Arena's storage or null; `output` is the caller's Copy.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_capture(
    list: *mut c_void,
    root: u64,
    source: *const Backing,
    parent: *const GraphParentRecord,
    epoch: *const Epoch,
    arena: *mut c_void,
    output: *mut Copy,
) -> bool {
    // SAFETY: as documented.
    let (source, parent, epoch, output) = unsafe { (&*source, &*parent, &*epoch, &mut *output) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(list, root, source, parent, epoch, arena, output))) {
        Ok(done) => done,
        Err(_) => {
            set_failure(failure::EXCEPTION);
            false
        }
    }
}
fn capture(list: *mut c_void, root: u64, source: &Backing, parent: &GraphParentRecord, epoch: &Epoch, arena: *mut c_void, output: &mut Copy) -> bool {
    set_failure(failure::DISABLED);
    if CyberpunkVR_StructuredBufferSnapshotsEnable.load(Relaxed) == 0 || cvr_host::probe::internal() {
        return false;
    }
    set_failure(failure::ARGUMENTS);
    let Some(s) = storage(arena) else {
        return false;
    };
    if !output.owner.is_null()
        || s.failed
        || s.finalized
        || source.binding.resource.is_null()
        || source.native.kind != 1
        || source.flags != 0
        || source.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32
        || (source.shader_register != 4 && source.shader_register != 6)
        || source.view_bytes == 0
        || source.view_bytes > 98304
        || source.gpu == 0
    {
        return false;
    }
    let scope = SCOPES.with_borrow(|l| l.current());
    set_failure(failure::SCOPE);
    if !epoch.certified
        || epoch.occurrence == 0
        || !SCOPES.with_borrow(|l| l.matches(&scope, epoch.list, epoch.generation, epoch.occurrence, thread()))
        || epoch.list != list as u64
        || !same_recording(&s.recording.0, &source.recording)
        || source.recording.generation != epoch.generation
        || !crate::indirect_copy::parent(parent)
        || !revalidate(parent)
    {
        return false;
    }
    let held = Held(Box::into_raw(Box::new(Receipt {
        vtbl: &RECEIPT_VTBL,
        references: AtomicU32::new(1),
        arena: add_ref(arena),
        source: OwnedBacking::copy(source),
        creation: Creation::default(),
        allocation: Allocation::default(),
        allocator: Allocator::default(),
        pin_allocator: core::ptr::null_mut(),
        pin_allocation: core::ptr::null_mut(),
        pool: GeometryPool::default(),
        placement: PlacedAllocation::default(),
        parent: *parent,
        epoch: *epoch,
        root,
        offset: 0,
        bytes: source.view_bytes,
        recorded: false,
        validated: false,
        complete: false,
        queue: core::ptr::null_mut(),
    })));
    let o = held.get();
    set_failure(failure::CREATION);
    match query_creation(source.binding.resource) {
        Some(creation) => o.creation = creation,
        None => return false,
    }
    let mut allocation = source.allocation.address;
    if source.low_type == POOL_LOW_TYPE as u32 && allocation == 0 {
        set_failure(failure::POOL);
        let mut pool = GeometryPool::default();
        let found = read_fixed_pool(source, &mut pool);
        o.pool = pool;
        if !found {
            return false;
        }
        allocation = o.pool.owner;
    } else if source.native.gpu != source.gpu || source.native.bytes != source.backing_bytes {
        return false;
    }
    let read = scd::reader(Some(cvr_iab_read), core::ptr::null_mut());
    o.allocation = scd::read_allocation(&read, base(), allocation);
    o.allocator = scd::read_allocator(&read, base(), source.native.registry, &o.allocation);
    set_failure(failure::ALLOCATION);
    if o.allocation.observation != KNOWN
        || o.allocation.resource != source.binding.resource as u64
        || o.allocation.bytes < source.backing_bytes
        || o.allocator.observation != KNOWN
        || !o.allocator.pimpl_matches
    {
        return false;
    }
    let next = add_ref(o.allocator.address as *mut c_void);
    release(core::mem::replace(&mut o.pin_allocator, next));
    let next = add_ref(o.allocation.address as *mut c_void);
    release(core::mem::replace(&mut o.pin_allocation, next));
    if o.creation.0.kind == 2 {
        let wanted = placed(&o.allocation);
        let mut placement = PlacedAllocation::default();
        // SAFETY: plain values.
        let found = unsafe { crate::ia_snapshot_receipt::cvr_iasn_read_pinned_placed_offset(&wanted, &mut placement) };
        o.placement = placement;
        if !found {
            return false;
        }
    }
    set_failure(failure::CHANGED);
    if !current(list, o) {
        return false;
    }
    let start = (s.bytes + ALIGNMENT - 1) & !(ALIGNMENT - 1);
    let padded = (o.bytes + ALIGNMENT - 1) & !(ALIGNMENT - 1);
    set_failure(failure::CAPACITY);
    if start > CAPACITY || padded > CAPACITY - start {
        return false;
    }
    o.offset = start;
    let Some(view) = private_view(source, start, source.view_bytes) else {
        return false;
    };
    set_failure(failure::RETENTION);
    let keep = [held.raw(), source.binding.resource, o.arena];
    // SAFETY: a live list; all three are live COM objects.
    if !unsafe { cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
        return false;
    }
    set_failure(failure::CHANGED);
    if !current(list, o) {
        return false;
    }
    {
        let _internal = cvr_host::probe::Internal::new();
        let source_resource = source.binding.resource;
        // SAFETY: a live list; the arena's resource and the source are live.
        if let (Some(l), Some(destination), Some(from)) = (
            unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) },
            unsafe { ID3D12Resource::from_raw_borrowed(&s.resource) },
            unsafe { ID3D12Resource::from_raw_borrowed(&source_resource) },
        ) {
            unsafe { l.CopyBufferRegion(destination, start, from, source.source_offset, source.view_bytes) };
        }
    }
    o.recorded = true;
    s.bytes = start + padded;
    // SAFETY: the receipt; the output takes its own reference.
    unsafe { receipt_add_ref(held.raw()) };
    *output = Copy {
        owner: held.raw(),
        resource: add_ref(s.resource),
        view,
        source_offset: source.source_offset,
        offset: start,
        bytes: source.view_bytes,
    };
    set_failure(failure::NONE);
    true
}
/// ValidateSubmission: the copy's source still physically the same and its record unchanged; the queue remembered.
///
/// # Safety
/// `copy` is valid; `queue` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_validate_submission(copy: *const Copy, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    let copy = unsafe { &*copy };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(held) = owner(copy) else {
            return false;
        };
        let o = held.get();
        let arena = arena_of(o);
        let source = &o.source.0;
        // SAFETY: the receipt's lease and a live queue.
        if queue.is_null()
            || !arena.finalized
            || arena.failed
            || !physical(o)
            || !unsafe { crate::native_recording::cvr_native_recording_validate_submission(&source.recording, queue) }
        {
            return false;
        }
        let (found, raw, address, _native) = crate::source_srv::read_record(
            Some(cvr_iab_read),
            core::ptr::null_mut(),
            source.native.registry,
            source.context.context,
            source.native.handle,
        );
        if !found || raw != source.raw_record || address != source.record_address {
            return false;
        }
        let next = add_ref(queue);
        release(core::mem::replace(&mut o.queue, next));
        o.validated = true;
        true
    }))
    .unwrap_or(false)
}
/// Ready: the copy's submission on its queue has completed (`fence` reached `signal`).
///
/// # Safety
/// `copy` and `recording` are valid; `queue` and `fence` are live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_ready(
    copy: *const Copy,
    recording: *const Recording,
    queue: *mut c_void,
    fence: *mut c_void,
    signal: u64,
    submitted: bool,
) -> bool {
    // SAFETY: as documented.
    let (copy, recording) = unsafe { (&*copy, &*recording) };
    let Some(held) = owner(copy) else {
        return false;
    };
    let o = held.get();
    let arena = arena_of(o);
    if !o.validated
        || arena.failed
        || !arena.finalized
        || !submitted
        || queue.is_null()
        || o.queue != queue
        || !same_recording(&o.source.0.recording, recording)
        || signal == 0
    {
        return false;
    }
    // SAFETY: a live fence or null.
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else {
        return false;
    };
    // SAFETY: a live fence.
    let completed = unsafe { fence.GetCompletedValue() };
    o.complete = completed != u64::MAX && completed >= signal;
    o.complete
}
/// MatchesPrivate (`staged` false) / MatchesStagedPrivate (`staged` true): the copy stands for this source binding.
///
/// # Safety
/// `copy` and `binding` are valid; `recording` is valid when staged; `queue` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbsn_matches(
    copy: *const Copy,
    binding: *const c_void,
    staged: bool,
    recording: *const Recording,
    queue: *mut c_void,
) -> bool {
    // SAFETY: as documented.
    let copy = unsafe { &*copy };
    let Some(held) = owner(copy) else {
        return false;
    };
    let o = held.get();
    let arena = arena_of(o);
    if !arena.finalized || arena.failed {
        return false;
    }
    if staged {
        // SAFETY: as documented.
        let recording = unsafe { &*recording };
        if !o.validated
            || queue.is_null()
            || o.queue != queue
            || !same_recording(&o.source.0.recording, recording)
            || !same_recording(&arena.recording.0, recording)
        {
            return false;
        }
    } else if !o.complete {
        return false;
    }
    // SAFETY: the backing's binding has material_buffer_snapshot's SrvBinding layout; `binding` is the caller's.
    unsafe { crate::material_buffer_snapshot::cvr_mbs_source_binding((&o.source.0.binding as *const _ as *const c_void).cast(), o.source.0.shader_register, binding.cast()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_copy() -> Copy {
        Copy {
            owner: core::ptr::null_mut(),
            resource: core::ptr::null_mut(),
            view: D3D12_SHADER_RESOURCE_VIEW_DESC::default(),
            source_offset: 0,
            offset: 0,
            bytes: 0,
        }
    }

    #[test]
    fn copies_without_an_owner_are_refused() {
        let copy = empty_copy();
        let recording = Recording { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
        // SAFETY: local values; nothing is read without an owner.
        unsafe {
            assert!(!cvr_mbsn_validate_submission(&copy, core::ptr::null_mut()));
            assert!(!cvr_mbsn_ready(&copy, &recording, core::ptr::null_mut(), core::ptr::null_mut(), 1, true));
            assert!(!cvr_mbsn_matches(&copy, core::ptr::null(), false, &recording, core::ptr::null_mut()));
        }
    }

    #[test]
    fn arenas_refuse_bad_arguments_and_capture_is_dormant() {
        let mut out = core::ptr::null_mut();
        // SAFETY: null objects are refused before use.
        assert_eq!(unsafe { cvr_mbsn_arena_create(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 1, &mut out) }, E_INVALIDARG);
        assert!(out.is_null());
        // SAFETY: no storage.
        unsafe {
            assert_eq!(cvr_mbsn_arena_bytes(core::ptr::null_mut()), 0);
            assert!(!cvr_mbsn_arena_finalize(core::ptr::null_mut(), core::ptr::null_mut()));
        }
        // SAFETY: a zeroed backing and local values; the switch is off, so nothing is read.
        let source: Backing = unsafe { core::mem::zeroed() };
        let parent: GraphParentRecord = unsafe { core::mem::zeroed() };
        let mut copy = empty_copy();
        assert!(!unsafe { cvr_mbsn_capture(core::ptr::null_mut(), 0, &source, &parent, &Epoch::default(), core::ptr::null_mut(), &mut copy) });
        assert_eq!(cvr_mbsn_last_failure(), failure::DISABLED);
    }
}
