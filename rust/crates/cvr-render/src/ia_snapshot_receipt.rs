//! THE NATIVE IA SNAPSHOT AT THE ACTUAL CALL (formerly src/Render/NativeIaSnapshot.cpp): the per-thread ledger of actual
//! native source callback scopes; the collection of an original draw's bound index / vertex sources into observations
//! (each source's shape, native record, recording context, API creation, allocation and allocator, placement and
//! state); the private arena they are copied into (one default-heap buffer per list generation, finalized to
//! GENERIC_READ); the receipt that pins every source with the command list; the native combined-read registration
//! (0x337DA0, then the flush 0x3E3BB4) with its recovery; and the proofs the copies validate at the call and at
//! submission. The rules each step applies are the ia_snapshot*.rs modules'.
//!
//! The C++ API (include/Render/NativeIaSnapshot.hpp) forwards here. geometry::DrawResources (a C++ value holding a
//! vector) stays the C++ half's: the receipt keeps a C++ copy through the attached host table, which also re-reads the
//! list's current IA bindings and revalidates the graph parent (still C++, NativePreparation.cpp).
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::{Acquire, Relaxed, Release, SeqCst}};
use std::sync::{Arc, OnceLock};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::buffer_read::NativeBuffer;
use crate::copy_windows::{self, Span, Window};
use crate::geometry_commands::DrawRecord;
use crate::ia_binding::cvr_iab_read;
use crate::ia_snapshot::{
    self as rules, CopyPlan, CopyRange, GeometryPool, Observation, RecoveryContext, ALIGNMENT, CODE_KNOWN, CONTEXT_KNOWN,
    GEOMETRY_POOL_KNOWN, MAX_ARENA_BYTES, MAX_VIEWS, PARENT_CURRENT, PLACED_KNOWN, SCOPE_CURRENT,
};
use crate::ia_snapshot_same::{CreationInfo, Lease};
use crate::indirect_copy::{GraphParentRecord, ScopeLedger, ScopeStamp};
use crate::native_recording::{Context as UploadContext, Lease as Recording};
use crate::source_cbv_details::{self as scd, Allocation, Allocator, Context, ReadState};
use crate::texture_immutable::PlacedAllocation;

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool;
}

/// Whether the snapshots run (the tools set it by name; NativeRetainedGeometry reads it too).
#[no_mangle]
pub static CyberpunkVR_NativeIaSnapshotsEnable: AtomicU32 = AtomicU32::new(0);

/// ia_snapshot::Failure.
mod failure {
    pub const NONE: u64 = 0;
    pub const DISABLED: u64 = 1;
    pub const EMPTY: u64 = 2;
    pub const ARGUMENTS: u64 = 3;
    pub const SCOPE: u64 = 4;
    pub const RECORDING: u64 = 8;
    pub const ALLOCATION_DEFERRED: u64 = 20;
    pub const STATE: u64 = 22;
    pub const CAPACITY: u64 = 23;
    pub const RETENTION: u64 = 24;
    pub const CHANGED: u64 = 25;
    pub const REGISTRATION: u64 = 26;
    pub const RECOVERY_VERIFIED: u64 = 27;
    pub const RECOVERY_FATAL: u64 = 28;
    pub const ALREADY_RECORDED: u64 = 29;
    pub const SUBMISSION: u64 = 30;
    pub const EXCEPTION: u64 = 31;
}
/// detail::Stage.
mod stage {
    pub const PREPARED: u32 = 0;
    pub const RECORDING: u32 = 1;
    pub const RECORDED: u32 = 2;
    pub const FAILED: u32 = 3;
}
const PINNED: u64 = 2048;
const ROLE_INDEX: u64 = 0;
const RECEIPT_TYPE: GUID = GUID::from_values(0x9944fe12, 0x63b7, 0x4b0b, [0x98, 0x31, 0x70, 0x64, 0x24, 0xc8, 0x32, 0x1a]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const REGISTER_RVA: u64 = 0x337DA0;
const FLUSH_RVA: u64 = 0x3E3BB4;
const ALLOCATION_OFFSET_RVA: u64 = 0x349F5C;

thread_local! {
    static SCOPES: RefCell<ScopeLedger> = RefCell::new(ScopeLedger::default());
    static FAILURE: Cell<u64> = const { Cell::new(0) };
}
fn set_failure(value: u64) {
    FAILURE.set(value);
}
fn enabled() -> bool {
    CyberpunkVR_NativeIaSnapshotsEnable.load(Relaxed) != 0
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
fn mem() -> impl Fn(u64, &mut [u8]) -> bool {
    scd::reader(Some(cvr_iab_read), core::ptr::null_mut())
}
fn compatible() -> bool {
    crate::ia_binding::native_ia_compatible()
        && crate::indirect_copy_native::cvr_ic_compatible()
        && crate::native_recording::cvr_native_recording_compatible()
}
fn geometry_pool_compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| crate::ia_snapshot_code::cvr_ias_geometry_pool_code(image()))
}
fn placement_compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| crate::ia_snapshot_code::cvr_ias_placement_code(image()))
}
fn image_bytes() -> u64 {
    static BYTES: OnceLock<u64> = OnceLock::new();
    *BYTES.get_or_init(|| crate::ia_snapshot_code::cvr_ias_image_bytes(image()))
}
/// AllocationOffset: the engine's own offset getter (its method checked against this image), called guarded.
unsafe extern "C" fn allocation_offset(_: *mut c_void, allocation: u64, output: *mut u64) -> bool {
    let base = image();
    if !placement_compatible() || !crate::ia_snapshot_code::cvr_ias_allocation_method(base, allocation) {
        return false;
    }
    let _internal = cvr_host::probe::Internal::new();
    let null = core::ptr::null_mut();
    // SAFETY: the method's code was checked for this image; an exception reads as a failure.
    match unsafe { cvr_engine::code::guarded_call4((base + ALLOCATION_OFFSET_RVA) as usize, allocation as *mut c_void, null, null, null) } {
        Some(offset) => {
            // SAFETY: the caller's output.
            unsafe { *output = offset as u64 };
            true
        }
        None => {
            // SAFETY: as above.
            unsafe { *output = 0 };
            false
        }
    }
}

// ---- the C++ half's values and its host table

/// ia_snapshot::Observations.
#[repr(C)]
pub struct Observations {
    pub rows: [Observation; MAX_VIEWS],
    pub count: u32,
    pub admitted: u32,
    pub first: u64,
}
const _: () = assert!(core::mem::size_of::<Observations>() == 8720);
/// ia_snapshot::ReadEpochCertificate.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Epoch {
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub certified: bool,
    pub separate_vertices: u32,
}
const _: () = assert!(core::mem::size_of::<Epoch>() == 32);
/// ia_snapshot::PreparedCopies (the owner an owned reference).
#[repr(C)]
pub struct PreparedCopies {
    pub owner: *mut c_void,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub copied_bytes: u64,
    pub count: u32,
    pub transferred_bytes: u64,
    pub reused_bytes: u64,
}
const _: () = assert!(core::mem::size_of::<PreparedCopies>() == 64);
/// ia_snapshot::PrivateIa (the resource and the owner owned references).
#[repr(C)]
pub struct PrivateIa {
    pub role: u64,
    pub slot: u32,
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub reserved_bytes: u64,
    pub origin_occurrence: u64,
    pub reused: bool,
    pub index: D3D12_INDEX_BUFFER_VIEW,
    pub vertex: D3D12_VERTEX_BUFFER_VIEW,
}
const _: () = assert!(core::mem::size_of::<PrivateIa>() == 112 && core::mem::offset_of!(PrivateIa, index) == 80);
impl PrivateIa {
    fn new() -> Self {
        PrivateIa {
            role: 0,
            slot: u32::MAX,
            resource: core::ptr::null_mut(),
            owner: core::ptr::null_mut(),
            source_offset: 0,
            private_offset: 0,
            copied_bytes: 0,
            reserved_bytes: 0,
            origin_occurrence: 0,
            reused: false,
            index: D3D12_INDEX_BUFFER_VIEW::default(),
            vertex: D3D12_VERTEX_BUFFER_VIEW::default(),
        }
    }
    /// A copy with its own references.
    fn duplicate(&self) -> Self {
        PrivateIa {
            role: self.role,
            slot: self.slot,
            resource: add_ref(self.resource),
            owner: add_ref(self.owner),
            source_offset: self.source_offset,
            private_offset: self.private_offset,
            copied_bytes: self.copied_bytes,
            reserved_bytes: self.reserved_bytes,
            origin_occurrence: self.origin_occurrence,
            reused: self.reused,
            index: self.index,
            vertex: self.vertex,
        }
    }
}
impl Drop for PrivateIa {
    fn drop(&mut self) {
        release(self.owner);
        release(self.resource);
    }
}
/// geometry::DrawResources' leading members (the C++ checks the offsets).
#[repr(C)]
struct DrawHead {
    index: Lease,
    vertices: [Lease; 16],
    draw: DrawRecord,
}
const _: () = assert!(core::mem::offset_of!(DrawHead, draw) == 1360);
/// What the C++ half lends: its DrawResources copies and current bindings, and the graph parent's revalidation.
#[repr(C)]
pub struct Host {
    /// A heap copy of a geometry::DrawResources (null when it could not be made).
    pub clone: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub free: unsafe extern "C" fn(*mut c_void),
    /// `*output = *saved` (DrawResources).
    pub assign: unsafe extern "C" fn(*mut c_void, *const c_void),
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(core::ptr::null_mut());
/// The C++ half's host table (static storage).
///
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_attach(host: *const Host) {
    HOST.store(host as *mut Host, SeqCst);
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached once with static storage.
    unsafe { HOST.load(SeqCst).as_ref() }
}
/// CurrentIa (formerly the C++ host's): the list's current IA bindings (geometry::CopyCurrentBindings, the exact bound
/// leases) are still the saved draw's -- its index buffer when indexed, every input slot's vertex buffer.
fn current_ia(list: *mut c_void, draw: *const c_void, indexed: bool) -> bool {
    if draw.is_null() {
        return false;
    }
    // SAFETY: a DrawResources the host made or lent (its head: the leases and the draw record).
    let saved = unsafe { &*draw.cast::<DrawHead>() };
    let Some((index, vertices)) = crate::geometry_leases::current(list as u64, &saved.draw, indexed, cvr_host::probe::internal()) else {
        return false;
    };
    let lent = |l: &Option<crate::geometry_leases::Lease>| -> Lease {
        // SAFETY: the same 80-byte geometry::BufferLease record (an empty slot all zeroes, as the .cpp's default).
        l.as_ref().map_or_else(|| unsafe { core::mem::zeroed() }, |l| unsafe { core::mem::transmute_copy(l) })
    };
    // SAFETY: two lease records each (ia_snapshot::detail::SameLease).
    let same = |a: &Lease, b: &Lease| unsafe { crate::ia_snapshot_same::cvr_ias_same_lease(a, b) };
    let mut matches = !indexed || same(&saved.index, &lent(&index));
    for slot in 0..16 {
        if matches && saved.draw.input_slot_mask & (1u32 << slot) != 0 && !same(&saved.vertices[slot], &lent(&vertices[slot])) {
            matches = false;
        }
    }
    // The current leases' references (the .cpp's temporary DrawResources) let go.
    for l in core::iter::once(&index).chain(vertices.iter()).flatten() {
        if !l.resource.is_null() {
            // SAFETY: a given lease's own reference, released once.
            drop(unsafe { windows::core::IUnknown::from_raw(l.resource) });
        }
    }
    matches
}
/// preparation::RevalidateGraphParent (preparation_trace.rs).
fn revalidate(parent: &GraphParentRecord) -> bool {
    crate::preparation_trace::revalidate_graph_parent(parent)
}

// ---- references

fn add_ref(object: *mut c_void) -> *mut c_void {
    // SAFETY: a live COM-shaped object or null.
    if let Some(o) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        core::mem::forget(o.clone());
    }
    object
}
fn release(object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: a reference this module owns, released once.
        drop(unsafe { IUnknown::from_raw(object) });
    }
}
/// geometry::BufferLease with its resource owned.
struct Buffer(Lease);
impl Buffer {
    fn copy(lease: &Lease) -> Self {
        add_ref(lease.resource);
        Buffer(*lease)
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        release(self.0.resource);
    }
}
/// constants::ResourceCreationInfo with its resource and heap owned.
struct Creation(CreationInfo);
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
fn query_creation(resource: *mut c_void) -> Option<Creation> {
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
/// detail::NativePins: the engine's allocator and allocation kept alive (the allocation released first).
struct Pins {
    allocator: *mut c_void,
    allocation: *mut c_void,
}
impl Pins {
    fn set(&mut self, allocator: u64, allocation: u64) {
        let next = add_ref(allocator as *mut c_void);
        release(core::mem::replace(&mut self.allocator, next));
        let next = add_ref(allocation as *mut c_void);
        release(core::mem::replace(&mut self.allocation, next));
    }
}
impl Drop for Pins {
    fn drop(&mut self) {
        release(self.allocation);
        release(self.allocator);
    }
}
/// A native recording lease whose token reference is owned.
struct OwnedRecording(Recording);
impl OwnedRecording {
    fn copy(lease: &Recording) -> Self {
        add_ref(lease.token);
        OwnedRecording(Recording { ..*lease })
    }
    fn none() -> Self {
        OwnedRecording(Recording { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
}
impl Drop for OwnedRecording {
    fn drop(&mut self) {
        release(self.0.token);
    }
}
fn validate_recording(recording: &Recording, list: *mut c_void, context: UploadContext) -> bool {
    // SAFETY: a live lease and list.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list, &context) }
}

// ---- one source

/// One bound source and everything its observation saw (the pins, the creation and the source released in that order).
struct Entry {
    pins: Pins,
    creation: Creation,
    source: Buffer,
    observation: Observation,
    index: D3D12_INDEX_BUFFER_VIEW,
    vertex: D3D12_VERTEX_BUFFER_VIEW,
    desc: D3D12_RESOURCE_DESC,
    heap: D3D12_HEAP_PROPERTIES,
    heap_flags: D3D12_HEAP_FLAGS,
    raw: [u8; 0xB0],
    context: Context,
    geometry_pool: GeometryPool,
    placement: PlacedAllocation,
    placement_heap: D3D12_HEAP_DESC,
    allocation: Allocation,
    allocator: Allocator,
    before: u32,
    required: u32,
    reads: u32,
    destination: u64,
    reserved: u64,
    mutated: bool,
}
impl Entry {
    fn new(source: &Lease) -> Self {
        Entry {
            pins: Pins { allocator: core::ptr::null_mut(), allocation: core::ptr::null_mut() },
            creation: Creation::default(),
            source: Buffer::copy(source),
            observation: Observation::default(),
            index: D3D12_INDEX_BUFFER_VIEW::default(),
            vertex: D3D12_VERTEX_BUFFER_VIEW::default(),
            desc: D3D12_RESOURCE_DESC::default(),
            heap: D3D12_HEAP_PROPERTIES::default(),
            heap_flags: D3D12_HEAP_FLAGS::default(),
            raw: [0; 0xB0],
            context: Context::default(),
            geometry_pool: GeometryPool::default(),
            placement: PlacedAllocation::default(),
            placement_heap: D3D12_HEAP_DESC::default(),
            allocation: Allocation::default(),
            allocator: Allocator::default(),
            before: 0,
            required: 0,
            reads: 0,
            destination: 0,
            reserved: 0,
            mutated: false,
        }
    }
    fn native(&self) -> &NativeBuffer {
        &self.source.0.native
    }
    fn resource(&self) -> u64 {
        self.source.0.resource as u64
    }
}
fn record(native: &NativeBuffer, raw: &mut [u8; 0xB0], address: &mut u64) -> bool {
    // SAFETY: plain values.
    unsafe { crate::ia_snapshot_native::cvr_ian_record(native, raw, address) }
}
fn read_state(list: *mut c_void, source: &Lease) -> ReadState {
    let mut out = ReadState::default();
    // SAFETY: the lease is laid out as ia_binding's OwnedLease (both geometry::BufferLease); the list is live.
    unsafe { crate::ia_binding::cvr_iab_read_state(list, (source as *const Lease).cast(), &mut out) };
    out
}
fn same_native(a: &NativeBuffer, b: &NativeBuffer) -> bool {
    // SAFETY: plain values.
    unsafe { crate::allocation_identity::cvr_nai_same_native(a, b) }
}
fn recovery_context(value: &Context) -> RecoveryContext {
    RecoveryContext {
        address: value.address,
        list: value.list,
        state: value.state,
        submitted: value.submitted,
        kind: value.kind,
        pending: value.pending,
        known: value.observation == scd::KNOWN,
    }
}
fn field_u64(raw: &[u8; 0xB0], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}
fn heap_desc(heap: *mut c_void) -> D3D12_HEAP_DESC {
    // SAFETY: an owned heap reference.
    unsafe { ID3D12Heap::from_raw_borrowed(&heap) }.map(|h| unsafe { h.GetDesc() }).unwrap_or_default()
}
fn placement_shape(entry: &Entry, heap: &mut D3D12_HEAP_DESC) -> bool {
    let (allocation, creation) = (&entry.allocation, &entry.creation.0);
    if creation.kind != 2
        || creation.heap.is_null()
        || creation.resource != entry.source.0.resource
        || creation.allocation_bytes == 0
        || creation.heap_offset > u64::MAX - creation.allocation_bytes
        || allocation.observation != scd::KNOWN
        || allocation.kind != 1
        || allocation.block == 0
        || allocation.handle == 0
        || allocation.resource != entry.resource()
        || allocation.heap != creation.heap as u64
        || allocation.bytes != creation.allocation_bytes
        || allocation.bytes < entry.source.0.bytes
        || entry.allocator.observation != scd::KNOWN
        || !entry.allocator.pimpl_matches
        || entry.pins.allocation.is_null()
        || entry.pins.allocator.is_null()
        || entry.pins.allocation as u64 != allocation.address
        || entry.pins.allocator as u64 != entry.allocator.address
        || entry.observation.alias_payload != 0
        || entry.observation.alias_control != 0
        || entry.observation.pool_owner != 0
    {
        return false;
    }
    *heap = heap_desc(creation.heap);
    // SAFETY: plain values; an owned heap reference.
    unsafe {
        crate::ia_snapshot_checks::cvr_ias_placement_heap(heap, &entry.heap, entry.heap_flags.0, creation.heap_offset, creation.allocation_bytes)
            && crate::ia_snapshot_objects::cvr_iao_unprotected_heap(creation.heap)
    }
}
fn placement_value(entry: &Entry, base: u64, output: &mut PlacedAllocation) -> bool {
    let mut heap = D3D12_HEAP_DESC::default();
    if !placement_compatible() || !placement_shape(entry, &mut heap) {
        return false;
    }
    let read = mem();
    let allocation = scd::read_allocation(&read, base, entry.pins.allocation as u64);
    let allocator = scd::read_allocator(&read, base, entry.native().registry, &allocation);
    // SAFETY: plain values.
    if !unsafe { crate::allocation_identity::cvr_nai_same_allocation(&entry.allocation, &allocation) }
        || !unsafe { crate::allocation_identity::cvr_nai_same_allocator(&entry.allocator, &allocator) }
    {
        return false;
    }
    let expected = PlacedAllocation {
        allocation: allocation.address,
        resource: allocation.resource,
        pimpl: allocation.pimpl,
        block: allocation.block,
        handle: allocation.handle,
        heap: allocation.heap,
        bytes: allocation.bytes,
        ..Default::default()
    };
    match rules::read_placed_offset(Some(cvr_iab_read), Some(allocation_offset), core::ptr::null_mut(), base, image_bytes(), &expected) {
        Some(value) => {
            *output = value;
            output.offset == entry.creation.0.heap_offset
        }
        None => {
            *output = PlacedAllocation::default();
            false
        }
    }
}
fn shape(entry: &mut Entry, list: *mut c_void) -> bool {
    let s = &entry.source.0;
    // SAFETY: an owned resource and a live list; the outputs are the entry's.
    !s.resource.is_null()
        && unsafe {
            crate::ia_snapshot_shape::cvr_ias_shape(
                s.resource,
                list,
                s.gpu,
                s.bytes,
                s.heap_type,
                s.flags,
                &mut entry.observation,
                &mut entry.desc,
                &mut entry.heap,
                &mut entry.heap_flags,
            )
        }
}
fn inspect(entry: &mut Entry, list: *mut c_void, base: u64, recording: &Recording) {
    let n = *entry.native();
    {
        let a = &mut entry.observation;
        a.native_resource = n.resource;
        a.native_gpu = n.gpu;
        a.native_bytes = n.bytes;
        a.registry = n.registry;
        a.context = n.context;
        a.handle = n.handle as u64;
        a.native_kind = n.kind as u64;
    }
    if !shape(entry, list) {
        entry.observation.reason = 25; // Changed
        return;
    }
    if record(&n, &mut entry.raw, &mut entry.observation.record_address) {
        let geometry = n.kind == 1
            && (entry.raw[6] & 15) == 5
            && field_u64(&entry.raw, 0x18) == 0
            && field_u64(&entry.raw, 0x20) == 0
            && field_u64(&entry.raw, 0x28) == 0
            && geometry_pool_compatible();
        // SAFETY: plain values; the reader reads the engine's memory.
        unsafe {
            crate::ia_snapshot_record::cvr_iar_inspect_record(
                Some(cvr_iab_read),
                core::ptr::null_mut(),
                base,
                &n,
                &entry.raw,
                &mut entry.observation,
                geometry,
                &mut entry.geometry_pool,
            )
        };
    }
    let read = mem();
    entry.context = scd::read_context(&read, n.context, list as u64);
    let context = UploadContext { context: entry.context.address, state: entry.context.state, list: entry.context.list };
    // SAFETY: plain values.
    if unsafe { crate::ia_snapshot_collect::cvr_iac_context_admitted(&entry.context, recording.context, recording.registry, &n) }
        && validate_recording(recording, list, context)
    {
        entry.observation.flags |= CONTEXT_KNOWN;
    }
    entry.creation = Creation::default();
    if let Some(creation) = query_creation(entry.source.0.resource) {
        entry.creation = creation;
        let c = &entry.creation.0;
        // SAFETY: plain values.
        unsafe {
            crate::ia_snapshot_collect::cvr_iac_record_creation(
                &mut entry.observation,
                c.kind,
                c.heap as u64,
                c.heap_offset,
                c.allocation_bytes,
                c.initial_state_known,
                c.initial_state,
            )
        };
    }
    entry.allocation = scd::read_allocation(&read, base, entry.observation.allocation);
    entry.allocator = scd::read_allocator(&read, base, n.registry, &entry.allocation);
    // SAFETY: plain values.
    unsafe { crate::ia_snapshot_inspect::cvr_iai_inspect_allocation(&mut entry.observation, &entry.allocation, &entry.allocator, &n) };
    // SAFETY: as above.
    if unsafe {
        crate::ia_snapshot_inspect::cvr_iai_placement_candidate(
            &entry.observation,
            n.kind,
            entry.creation.0.kind,
            !entry.creation.0.heap.is_null(),
            &entry.allocation,
            &entry.allocator,
        )
    } && placement_compatible()
    {
        entry.pins.set(entry.allocator.address, entry.allocation.address);
        let mut placement = PlacedAllocation::default();
        let ok = placement_value(entry, base, &mut placement);
        entry.placement = placement;
        let mut heap = D3D12_HEAP_DESC::default();
        if ok && placement_shape(entry, &mut heap) {
            entry.placement_heap = heap;
            entry.observation.flags |= PLACED_KNOWN;
            entry.observation.reserved = entry.placement.offset;
        } else if ok {
            entry.placement_heap = heap;
        }
    }
    let state = read_state(list, &entry.source.0);
    // SAFETY: plain values.
    unsafe {
        crate::ia_snapshot_state::cvr_iai_inspect_state(&mut entry.observation, &state, &n, &mut entry.before, &mut entry.reads, &mut entry.required)
    };
}
fn epoch_current(epoch: &Epoch, list: u64, recording: &Recording) -> bool {
    epoch.certified
        && epoch.list == list
        && epoch.list == recording.list
        && epoch.generation != 0
        && epoch.generation == recording.generation
        && epoch.occurrence != 0
}
fn summarize(entries: &[Entry], output: &mut Observations) {
    clear_observations(output);
    output.count = entries.len() as u32;
    for (row, entry) in output.rows.iter_mut().zip(entries) {
        *row = entry.observation;
    }
    let mut first = 0u64;
    // SAFETY: the rows are Observation values (64 words each).
    unsafe {
        crate::ia_snapshot_checks::cvr_ias_summarize(output.rows.as_ptr().cast(), output.count as usize, &mut output.admitted, &mut first)
    };
    output.first = first;
}
fn clear_observations(output: &mut Observations) {
    output.rows = [Observation::default(); MAX_VIEWS];
    output.count = 0;
    output.admitted = 0;
    output.first = 0;
}
fn list_type(list: *mut c_void) -> Option<D3D12_COMMAND_LIST_TYPE> {
    // SAFETY: a live list or null.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.map(|l| unsafe { l.GetType() })
}
#[allow(clippy::too_many_arguments)]
fn collect(
    list: *mut c_void,
    draw: *const c_void,
    indexed: bool,
    recording: &Recording,
    parent: &GraphParentRecord,
    epoch: &Epoch,
    entries: &mut Vec<Entry>,
    output: &mut Observations,
) -> bool {
    clear_observations(output);
    entries.clear();
    set_failure(failure::ARGUMENTS);
    // SAFETY: the C++ half's DrawResources begins with the head (the C++ checks the offsets).
    let head = unsafe { &*draw.cast::<DrawHead>() };
    if list.is_null() || list_type(list) != Some(D3D12_COMMAND_LIST_TYPE_DIRECT) || head.draw.list != list as u64 || head.draw.flags & 128 == 0 {
        return false;
    }
    let Some(copied_mask) = rules::selection(head.draw.input_slot_mask, epoch.separate_vertices) else {
        return false;
    };
    let mut add = |role: u64, slot: u32, source: &Lease| {
        let mut entry = Entry::new(source);
        if role == ROLE_INDEX {
            entry.index = head.draw.index;
        } else {
            entry.vertex = head.draw.vertices[slot as usize];
        }
        // SAFETY: plain values.
        unsafe {
            crate::ia_snapshot_seed::cvr_ias_seed(
                &mut entry.observation,
                role,
                slot as u64,
                list as u64,
                epoch.generation,
                epoch.occurrence,
                &entry.index,
                &entry.vertex,
            )
        };
        entries.push(entry);
    };
    if indexed {
        add(ROLE_INDEX, u32::MAX, &head.index);
    }
    for slot in 0..16u32 {
        if copied_mask & (1 << slot) != 0 {
            add(1, slot, &head.vertices[slot as usize]);
        }
    }
    let stamp = SCOPES.with_borrow(|s| s.current());
    let base = image();
    let mut flags = 0u64;
    if epoch_current(epoch, list as u64, recording) && scope_matches(&stamp, list as u64, epoch.generation, epoch.occurrence) {
        flags |= SCOPE_CURRENT;
    }
    if compatible() {
        flags |= CODE_KNOWN;
    }
    if crate::indirect_copy::parent(parent) && revalidate(parent) {
        flags |= PARENT_CURRENT;
    }
    if current_ia(list, draw, indexed) {
        flags |= rules::BINDINGS_CURRENT;
    }
    for entry in entries.iter_mut() {
        entry.observation.flags = flags;
        inspect(entry, list, base, recording);
        // SAFETY: plain values.
        unsafe { crate::ia_snapshot_collect::cvr_iac_check_view(&mut entry.observation, &entry.index, &entry.vertex) };
    }
    let resources: Vec<u64> = entries.iter().map(Entry::resource).collect();
    let natives: Vec<NativeBuffer> = entries.iter().map(|e| *e.native()).collect();
    let mut reasons: Vec<u64> = entries.iter().map(|e| e.observation.reason).collect();
    // SAFETY: three arrays of the entries' count.
    unsafe { crate::ia_snapshot_backing::cvr_iab_shared_backing(resources.as_ptr(), natives.as_ptr(), reasons.as_mut_ptr(), entries.len()) };
    for (entry, reason) in entries.iter_mut().zip(reasons) {
        entry.observation.reason = reason;
    }
    summarize(entries, output);
    set_failure(output.first);
    true
}

// ---- the arena

/// The arena's storage: a COM object the receipts and the private views keep.
#[repr(C)]
struct Storage {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    resource: *mut c_void,
    device: *mut c_void,
    list: u64,
    generation: u64,
    gpu: u64,
    capacity: u64,
    cursor: u64,
    thread: u32,
    recording: OwnedRecording,
    context: UploadContext,
    pending: AtomicU32,
    finalized: AtomicBool,
    failed: AtomicBool,
    window: Option<Arc<Window>>,
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
        release(storage.resource);
        release(storage.device);
    }
    left
}
fn storage<'a>(raw: *mut c_void) -> Option<&'a mut Storage> {
    // SAFETY: a storage this module made (the C++ arena holds a reference) or null.
    unsafe { raw.cast::<Storage>().as_mut() }
}

/// SnapshotArena::Create: S_OK with `*out` a new storage reference.
///
/// # Safety
/// `existing` is the arena's storage or null; `device` and `list` are live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_arena_create(
    existing: *mut c_void,
    device: *mut c_void,
    list: *mut c_void,
    generation: u64,
    capacity: u64,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the caller's output.
    unsafe { *out = core::ptr::null_mut() };
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena_create(existing, device, list, generation, capacity)));
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
fn arena_create(existing: *mut c_void, device: *mut c_void, list: *mut c_void, generation: u64, capacity: u64) -> Result<*mut c_void, HRESULT> {
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return Err(E_ACCESSDENIED);
    }
    // SAFETY: live objects or null.
    let (Some(d), Some(l)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) })
    else {
        return Err(E_INVALIDARG);
    };
    // SAFETY: live objects.
    if !existing.is_null()
        || unsafe { d.GetNodeCount() } != 1
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || generation == 0
        || capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity % ALIGNMENT != 0
    {
        return Err(E_INVALIDARG);
    }
    // SAFETY: a live list.
    let mut actual: Option<ID3D12Device> = None;
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
        gpu: 0,
        capacity,
        cursor: 0,
        thread: thread(),
        recording: OwnedRecording::none(),
        context: UploadContext::default(),
        pending: AtomicU32::new(0),
        finalized: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        window: None,
    }));
    let raw: *mut c_void = next.cast();
    let fail = move |hr: HRESULT| {
        // SAFETY: the one reference made above.
        unsafe { storage_release(raw) };
        Err(hr)
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
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
    s.gpu = unsafe { resource.GetGPUVirtualAddress() };
    s.resource = resource.into_raw();
    if s.gpu == 0 || s.gpu > u64::MAX - capacity {
        return fail(E_FAIL);
    }
    s.window = copy_windows::create(s.list, s.generation);
    set_failure(failure::NONE);
    Ok(next.cast())
}
/// SnapshotArena::Abort.
///
/// # Safety
/// `raw` is the arena's storage or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_arena_abort(raw: *mut c_void) {
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
/// As cvr_iasn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_arena_finalized(raw: *mut c_void) -> bool {
    storage(raw).is_some_and(|s| s.finalized.load(Acquire) && !s.failed.load(Acquire))
}
/// SnapshotArena::Bytes.
///
/// # Safety
/// As cvr_iasn_arena_abort.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_arena_bytes(raw: *mut c_void) -> u64 {
    storage(raw).map_or(0, |s| s.cursor)
}
/// SnapshotArena::Finalize: the copies recorded, the arena turned GENERIC_READ.
///
/// # Safety
/// As cvr_iasn_arena_abort; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_arena_finalize(raw: *mut c_void, list: *mut c_void) -> bool {
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let Some(s) = storage(raw) else {
            return false;
        };
        if list.is_null()
            || list as u64 != s.list
            || s.pending.load(SeqCst) != 0
            || s.cursor == 0
            || s.finalized.load(SeqCst)
            || s.failed.load(SeqCst)
        {
            return false;
        }
        set_failure(failure::RETENTION);
        // SAFETY: a live list and the storage.
        if !unsafe { cvr_gpu_has_command_resource(list, raw) } {
            return false;
        }
        set_failure(failure::RECORDING);
        if !validate_recording(&s.recording.0, list, s.context) {
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
        s.finalized.store(true, Release);
        if let Some(window) = &s.window {
            window.close();
        }
        set_failure(failure::NONE);
        true
    })
}
fn transition(resource: *mut c_void, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
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

// ---- the receipt

#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    stage: AtomicU32,
    arena: *mut c_void,
    entries: Vec<Entry>,
    views: Vec<PrivateIa>,
    draw: *mut c_void,
    /// The Rust capture's own copy of the draw (`draw` then points at it); None: `draw` is the C++ host's copy.
    draw_owned: Option<Box<crate::geometry_leases::DrawResources>>,
    indexed: bool,
    recording: OwnedRecording,
    parent: GraphParentRecord,
    copies: CopyPlan,
    transfers: Vec<CopyRange>,
    reused: Vec<Arc<copy_windows::Entry>>,
    origins: Vec<u64>,
    transferred: u64,
    reused_bytes: u64,
    epoch: Epoch,
    scope: ScopeStamp,
    base: u64,
    copied: u64,
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
        let (draw, arena, owned) = (receipt.draw, receipt.arena, receipt.draw_owned.is_some());
        drop(receipt);
        if !draw.is_null() && !owned {
            if let Some(h) = host() {
                // SAFETY: the host's own copy.
                unsafe { (h.free)(draw) };
            }
        }
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
fn owner(proof: &PreparedCopies) -> Option<Held> {
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
fn arena_of(owner: &Receipt) -> &mut Storage {
    // SAFETY: the receipt owns a storage reference.
    unsafe { &mut *owner.arena.cast::<Storage>() }
}
fn public_proof(proof: &PreparedCopies, owner: &Receipt) -> bool {
    !owner.arena.is_null()
        && proof.list == owner.epoch.list
        && proof.generation == owner.epoch.generation
        && proof.occurrence == owner.epoch.occurrence
        && proof.count as usize == owner.entries.len()
        && proof.copied_bytes == owner.copied
        && proof.transferred_bytes == owner.transferred
        && proof.reused_bytes == owner.reused_bytes
}
fn scope(list: *mut c_void, owner: &Receipt) -> bool {
    if list.is_null() || owner.arena.is_null() {
        return false;
    }
    let arena = arena_of(owner);
    !arena.failed.load(SeqCst)
        && !arena.finalized.load(SeqCst)
        && owner.base == image()
        && scope_matches(&owner.scope, list as u64, owner.epoch.generation, owner.epoch.occurrence)
        && epoch_current(&owner.epoch, list as u64, &owner.recording.0)
        && revalidate(&owner.parent)
}
fn physical(entry: &Entry, base: u64) -> bool {
    let Some(creation) = query_creation(entry.source.0.resource) else {
        return false;
    };
    // SAFETY: plain values.
    if !unsafe { crate::ia_snapshot_same::cvr_ias_same_creation(&entry.creation.0, &creation.0) }
        || entry.pins.allocation.is_null()
        || entry.pins.allocator.is_null()
    {
        return false;
    }
    let read = mem();
    let allocation = scd::read_allocation(&read, base, entry.pins.allocation as u64);
    let allocator = scd::read_allocator(&read, base, entry.native().registry, &allocation);
    // SAFETY: plain values.
    if !unsafe { crate::allocation_identity::cvr_nai_same_allocation(&entry.allocation, &allocation) }
        || !unsafe { crate::allocation_identity::cvr_nai_same_allocator(&entry.allocator, &allocator) }
        || entry.pins.allocator as u64 != allocator.address
        || !unsafe {
            crate::ia_snapshot_identity::cvr_iai_physical_allocation(
                &allocation,
                entry.observation.flags & PLACED_KNOWN != 0,
                entry.resource(),
                entry.source.0.bytes,
            )
        }
    {
        return false;
    }
    if entry.observation.flags & PLACED_KNOWN != 0 {
        let mut heap = D3D12_HEAP_DESC::default();
        let mut placement = PlacedAllocation::default();
        // SAFETY: plain values.
        if !placement_shape(entry, &mut heap)
            || !unsafe { crate::ia_snapshot_seed::cvr_ias_same_heap_desc(&heap, &entry.placement_heap) }
            || !placement_value(entry, base, &mut placement)
            || placement != entry.placement
        {
            return false;
        }
    }
    let resource = entry.source.0.resource;
    // SAFETY: an owned resource.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    // SAFETY: a live resource.
    let desc = unsafe { r.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    let (mut result, mut session) = (0u64, 0u64);
    // SAFETY: plain values and a live resource.
    unsafe {
        crate::ia_snapshot_checks::cvr_ias_same_desc(&entry.desc, &desc)
            && r.GetGPUVirtualAddress() == entry.source.0.gpu
            && r.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_ok()
            && crate::ia_snapshot_checks::cvr_ias_same_heap(&entry.heap, &heap)
            && flags == entry.heap_flags
            && crate::ia_snapshot_objects::cvr_iao_unprotected(resource, &mut result, &mut session)
    }
}
fn pool_current(entry: &Entry, base: u64) -> bool {
    if entry.observation.flags & GEOMETRY_POOL_KNOWN == 0 {
        return true;
    }
    // SAFETY: plain values; the reader reads the engine's memory.
    geometry_pool_compatible()
        && unsafe {
            crate::ia_snapshot_pool::cvr_iap_pool_current(
                Some(cvr_iab_read),
                core::ptr::null_mut(),
                base,
                entry.pins.allocation as u64,
                &entry.geometry_pool,
                entry.native(),
                &entry.raw,
                entry.source.0.gpu,
                entry.source.0.bytes,
                entry.observation.view_gpu,
                entry.observation.view_bytes,
            )
        }
}
fn identity(list: *mut c_void, owner: &Receipt, entry: &Entry, pending: bool, context: &mut Context) -> bool {
    if !scope(list, owner) || !current_ia(list, owner.draw, owner.indexed) || !physical(entry, owner.base) || !pool_current(entry, owner.base) {
        return false;
    }
    let mut raw = [0u8; 0xB0];
    let mut address = 0u64;
    if !record(entry.native(), &mut raw, &mut address) || raw != entry.raw || address != entry.observation.record_address {
        return false;
    }
    *context = scd::read_context(&mem(), entry.native().context, list as u64);
    // SAFETY: plain values.
    if !unsafe { crate::ia_snapshot_identity::cvr_iai_same_context(context, &entry.context, pending) } {
        return false;
    }
    let expected = UploadContext { context: context.address, state: context.state, list: context.list };
    // SAFETY: plain values.
    validate_recording(&owner.recording.0, list, expected) && unsafe { crate::ia_snapshot_native::cvr_ian_residency(entry.native(), address) }
}
fn check(list: *mut c_void, owner: &Receipt, ready: bool) -> bool {
    set_failure(failure::SCOPE);
    if !scope(list, owner) {
        return false;
    }
    for entry in &owner.entries {
        let mut context = Context::default();
        set_failure(failure::CHANGED);
        if !identity(list, owner, entry, false, &mut context) {
            return false;
        }
        let state = read_state(list, &entry.source.0);
        set_failure(failure::STATE);
        // SAFETY: plain values.
        if !unsafe {
            crate::ia_snapshot_identity::cvr_iai_state_ready(&state, entry.native().kind, entry.reads, entry.required, entry.before, ready)
        } {
            return false;
        }
    }
    true
}
fn register(base: u64, context: u64, handle: u32, required: u32) {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the engine's combined-read registration (the code contract was checked when the observation was taken).
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
    for entry in owner.entries.iter().filter(|e| e.mutated) {
        let mut context = Context::default();
        if !identity(list, owner, entry, true, &mut context) {
            return false;
        }
        let mut raw = [0u8; 0xB0];
        let mut address = 0u64;
        if !record(entry.native(), &mut raw, &mut address)
            || !rules::recovery_identity(
                entry.native(),
                &entry.raw,
                entry.observation.record_address,
                &recovery_context(&entry.context),
                entry.native(),
                &raw,
                address,
                &recovery_context(&context),
            )
        {
            return false;
        }
        if context.pending != 0 {
            flush(owner.base, context.address);
        }
        if !identity(list, owner, entry, false, &mut context) {
            return false;
        }
        let state = read_state(list, &entry.source.0);
        let Some(required) = rules::recovery_mask(entry.before, state, entry.native().kind, entry.reads) else {
            return false;
        };
        if state.value & required != required {
            register(owner.base, context.address, entry.native().handle, required);
            flush(owner.base, context.address);
        }
        if !identity(list, owner, entry, false, &mut context)
            || !rules::read_mask(read_state(list, &entry.source.0), entry.native().kind, entry.reads, false)
        {
            return false;
        }
        if read_state(list, &entry.source.0).value & required != required {
            return false;
        }
    }
    true
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

// ---- the C++ API

/// ActualCallScope's constructor: `*out` the stamp (zero when the snapshots are off or the call is internal).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_scope_enter(list: u64, generation: u64, occurrence: u64, out: *mut ScopeStamp) {
    let stamp = if enabled() && !cvr_host::probe::internal() {
        SCOPES.with_borrow_mut(|s| s.enter(list, generation, occurrence, thread()))
    } else {
        ScopeStamp::default()
    };
    // SAFETY: the caller's output.
    unsafe { *out = stamp };
}
/// ActualCallScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_scope_exit(stamp: *const ScopeStamp) {
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
pub unsafe extern "C-unwind" fn cvr_iasn_scope_active(stamp: *const ScopeStamp) -> bool {
    // SAFETY: the caller's stamp.
    let stamp = unsafe { *stamp };
    scope_matches(&stamp, stamp.list, stamp.generation, stamp.position)
}
/// LastFailure.
#[no_mangle]
pub extern "C-unwind" fn cvr_iasn_last_failure() -> u64 {
    FAILURE.get()
}
/// ReadPinnedPlacedOffset.
///
/// # Safety
/// `expected` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_read_pinned_placed_offset(expected: *const PlacedAllocation, out: *mut PlacedAllocation) -> bool {
    // SAFETY: as documented.
    placement_compatible()
        && unsafe {
            rules::cvr_ia_read_placed_offset(Some(cvr_iab_read), Some(allocation_offset), core::ptr::null_mut(), image(), image_bytes(), expected, out)
        }
}
/// ReadFixedPool.
///
/// # Safety
/// `native` and `raw` are valid; `out` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iasn_read_fixed_pool(
    native: *const NativeBuffer,
    raw: *const [u8; 0xB0],
    gpu: u64,
    bytes: u64,
    view_gpu: u64,
    view_bytes: u64,
    low_type: u8,
    out: *mut GeometryPool,
) -> bool {
    // SAFETY: as documented.
    geometry_pool_compatible()
        && unsafe {
            rules::cvr_ia_read_geometry_pool(Some(cvr_iab_read), core::ptr::null_mut(), image(), native, raw, gpu, bytes, view_gpu, view_bytes, out, low_type)
        }
}
/// ObserveAtActualCall.
///
/// # Safety
/// `draw` is a geometry::DrawResources; `recording`, `parent` and `epoch` are valid; `output` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iasn_observe(
    list: *mut c_void,
    draw: *const c_void,
    indexed: bool,
    recording: *const Recording,
    parent: *const GraphParentRecord,
    epoch: *const Epoch,
    output: *mut Observations,
) -> bool {
    // SAFETY: as documented.
    let (recording, parent, epoch, output) = unsafe { (&*recording, &*parent, &*epoch, &mut *output) };
    guarded(|| {
        clear_observations(output);
        set_failure(failure::DISABLED);
        if !enabled() || cvr_host::probe::internal() {
            return false;
        }
        let mut entries = Vec::with_capacity(MAX_VIEWS);
        collect(list, draw, indexed, recording, parent, epoch, &mut entries, output)
    })
}
/// PrepareAtActualCall: on success `*output` holds a new receipt reference.
///
/// # Safety
/// As cvr_iasn_observe; `arena` is the SnapshotArena's storage or null; `output` is the caller's PreparedCopies.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iasn_prepare(
    list: *mut c_void,
    draw: *const c_void,
    indexed: bool,
    recording: *const Recording,
    parent: *const GraphParentRecord,
    epoch: *const Epoch,
    arena: *mut c_void,
    output: *mut PreparedCopies,
    observed: *mut Observations,
) -> bool {
    // SAFETY: as documented.
    let (recording, parent, epoch, output, observed) = unsafe { (&*recording, &*parent, &*epoch, &mut *output, &mut *observed) };
    guarded(|| prepare(list, draw, indexed, recording, parent, epoch, arena, output, observed, None))
}
/// PrepareAtDraw for the Rust capture: its own DrawResources (in the C++ head layout) copied into the receipt.
#[allow(clippy::too_many_arguments)]
pub fn prepare_owned(
    list: *mut c_void,
    draw: &crate::geometry_leases::DrawResources,
    indexed: bool,
    recording: &Recording,
    parent: &GraphParentRecord,
    epoch: &Epoch,
    arena: *mut c_void,
    output: &mut PreparedCopies,
    observed: &mut Observations,
) -> bool {
    let head = (draw as *const crate::geometry_leases::DrawResources).cast::<c_void>();
    guarded(|| prepare(list, head, indexed, recording, parent, epoch, arena, output, observed, Some(draw)))
}
#[allow(clippy::too_many_arguments)]
fn prepare(
    list: *mut c_void,
    draw: *const c_void,
    indexed: bool,
    recording: &Recording,
    parent: &GraphParentRecord,
    epoch: &Epoch,
    arena: *mut c_void,
    output: &mut PreparedCopies,
    observed: &mut Observations,
    owned: Option<&crate::geometry_leases::DrawResources>,
) -> bool {
    clear_observations(observed);
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return false;
    }
    let mut entries = Vec::with_capacity(MAX_VIEWS);
    if !collect(list, draw, indexed, recording, parent, epoch, &mut entries, observed) {
        return false;
    }
    if observed.first != failure::NONE {
        set_failure(observed.first);
        return false;
    }
    set_failure(failure::EMPTY);
    if entries.is_empty() {
        return false;
    }
    set_failure(failure::ARGUMENTS);
    let Some(storage_ref) = storage(arena) else {
        return false;
    };
    if !output.owner.is_null()
        || storage_ref.list != list as u64
        || storage_ref.generation != epoch.generation
        || storage_ref.thread != thread()
        || storage_ref.failed.load(SeqCst)
        || storage_ref.finalized.load(SeqCst)
    {
        return false;
    }
    let (draw_copy, draw_owned) = match owned {
        Some(value) => {
            let copy = Box::new(value.clone());
            ((&*copy as *const crate::geometry_leases::DrawResources).cast_mut().cast::<c_void>(), Some(copy))
        }
        None => {
            let Some(h) = host() else {
                return false;
            };
            // SAFETY: the host's copy of the caller's DrawResources.
            (unsafe { (h.clone)(draw) }, None)
        }
    };
    if draw_copy.is_null() {
        set_failure(failure::EXCEPTION);
        return false;
    }
    let held = Held(Box::into_raw(Box::new(Receipt {
        vtbl: &RECEIPT_VTBL,
        references: AtomicU32::new(1),
        stage: AtomicU32::new(stage::PREPARED),
        arena: add_ref(arena),
        entries,
        views: Vec::new(),
        draw: draw_copy,
        draw_owned,
        indexed,
        recording: OwnedRecording::copy(recording),
        parent: *parent,
        copies: CopyPlan { ranges: Vec::new(), destinations: Vec::new(), next: 0, copied: 0 },
        transfers: Vec::new(),
        reused: Vec::new(),
        origins: Vec::new(),
        transferred: 0,
        reused_bytes: 0,
        epoch: *epoch,
        scope: SCOPES.with_borrow(|s| s.current()),
        base: image(),
        copied: 0,
    })));
    let o = held.get();
    o.views.reserve(o.entries.len());
    // SAFETY: the receipt's own storage reference (a separate object from the receipt).
    let arena = unsafe { &mut *o.arena.cast::<Storage>() };
    let mut cursor = arena.cursor;
    let ranges: Vec<CopyRange> = o
        .entries
        .iter()
        .map(|e| CopyRange { resource: e.resource(), source_offset: e.observation.com_offset, bytes: e.observation.view_bytes, destination: 0 })
        .collect();
    set_failure(failure::CAPACITY);
    match rules::plan_copies(&ranges, 0, arena.capacity) {
        Some(plan) => o.copies = plan,
        None => {
            o.entries[0].observation.reason = failure::CAPACITY;
            summarize(&o.entries, observed);
            return false;
        }
    }
    o.copied = o.copies.copied;
    for i in 0..o.copies.ranges.len() {
        let range = o.copies.ranges[i];
        let eligible = o.entries.iter().all(|e| e.resource() != range.resource || e.source.0.flags == 0);
        let cached = if eligible {
            arena.window.as_ref().and_then(|w| w.find(range.resource, range.source_offset, range.bytes, epoch.occurrence))
        } else {
            None
        };
        if let Some(cached) = cached {
            o.copies.ranges[i].destination = cached.span.destination + range.source_offset - cached.span.offset;
            o.origins.push(cached.span.occurrence);
            o.reused.push(cached);
            o.reused_bytes += range.bytes;
        } else {
            let prefix = range.source_offset % ALIGNMENT;
            let reserved = if range.bytes > u64::MAX - prefix { None } else { rules::reserve(cursor, arena.capacity, range.bytes + prefix) };
            let Some((start, end)) = reserved else {
                o.entries[0].observation.reason = FAILURE.get();
                summarize(&o.entries, observed);
                return false;
            };
            o.copies.ranges[i].destination = start + prefix;
            cursor = end;
            o.transfers.push(o.copies.ranges[i]);
            o.origins.push(epoch.occurrence);
            o.transferred += range.bytes;
        }
    }
    o.copies.next = cursor;
    for (i, range) in ranges.iter().enumerate() {
        let found = o.copies.ranges.iter().find(|copy| {
            copy.resource == range.resource
                && range.source_offset >= copy.source_offset
                && range.source_offset - copy.source_offset <= copy.bytes
                && range.bytes <= copy.bytes - (range.source_offset - copy.source_offset)
        });
        let Some(found) = found else {
            return false;
        };
        o.copies.destinations[i] = found.destination + range.source_offset - found.source_offset;
    }
    for view_index in 0..o.entries.len() {
        let destination = o.copies.destinations[view_index];
        {
            let entry = &mut o.entries[view_index];
            entry.destination = destination;
            entry.reserved = entry.observation.view_bytes;
            entry.observation.private_offset = entry.destination;
            entry.observation.reserved_bytes = entry.reserved;
            set_failure(failure::ALLOCATION_DEFERRED);
            entry.pins.set(entry.allocator.address, entry.allocation.address);
        }
        let entry = &o.entries[view_index];
        if !physical(entry, o.base) || !pool_current(entry, o.base) {
            set_failure(failure::CHANGED);
            o.entries[view_index].observation.reason = failure::CHANGED;
            summarize(&o.entries, observed);
            return false;
        }
        o.entries[view_index].observation.flags |= PINNED;
        let entry = &o.entries[view_index];
        let mut view = PrivateIa::new();
        view.role = entry.observation.role;
        view.slot = entry.observation.slot as u32;
        view.resource = add_ref(arena.resource);
        view.owner = add_ref(o.arena);
        view.source_offset = entry.observation.com_offset;
        view.private_offset = entry.destination;
        view.copied_bytes = entry.observation.view_bytes;
        view.reserved_bytes = entry.reserved;
        for (i, range) in o.copies.ranges.iter().enumerate() {
            if range.resource == entry.resource()
                && view.source_offset >= range.source_offset
                && view.source_offset - range.source_offset <= range.bytes
                && view.copied_bytes <= range.bytes - (view.source_offset - range.source_offset)
            {
                view.origin_occurrence = o.origins[i];
                view.reused = view.origin_occurrence != epoch.occurrence;
                break;
            }
        }
        if view.origin_occurrence == 0 {
            return false;
        }
        let made = if view.role == ROLE_INDEX {
            rules::index_view(arena.gpu, entry.destination, arena.capacity, &entry.index).map(|v| view.index = v)
        } else {
            rules::vertex_view(arena.gpu, entry.destination, arena.capacity, &entry.vertex).map(|v| view.vertex = v)
        };
        if made.is_none() {
            set_failure(failure::ARGUMENTS);
            return false;
        }
        o.views.push(view);
    }
    for i in 0..o.entries.len() {
        for j in 0..o.entries.len() {
            if same_native(o.entries[i].native(), o.entries[j].native()) {
                let reads = o.entries[j].reads;
                o.entries[i].reads |= reads;
            }
        }
    }
    if !check(list, o, false) {
        return false;
    }
    set_failure(failure::RETENTION);
    let keep = [held.raw(), o.arena];
    // SAFETY: a live list; both objects are COM objects this receipt owns.
    if !unsafe { cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
        return false;
    }
    arena.recording = OwnedRecording::copy(recording);
    let context = o.entries[0].context;
    arena.context = UploadContext { context: context.address, state: context.state, list: context.list };
    arena.cursor = cursor;
    arena.pending.fetch_add(1, SeqCst);
    *output = PreparedCopies {
        owner: {
            receipt_add_ref_raw(held.raw());
            held.raw()
        },
        list: epoch.list,
        generation: epoch.generation,
        occurrence: epoch.occurrence,
        copied_bytes: o.copied,
        count: o.entries.len() as u32,
        transferred_bytes: o.transferred,
        reused_bytes: o.reused_bytes,
    };
    summarize(&o.entries, observed);
    set_failure(failure::NONE);
    true
}
fn receipt_add_ref_raw(raw: *mut c_void) {
    // SAFETY: a live receipt.
    unsafe { receipt_add_ref(raw) };
}
fn validate(list: *mut c_void, proof: &PreparedCopies) -> bool {
    set_failure(failure::ARGUMENTS);
    let Some(held) = owner(proof) else {
        return false;
    };
    let o = held.get();
    if !public_proof(proof, o) {
        return false;
    }
    let stage_now = o.stage.load(Acquire);
    if stage_now != stage::PREPARED && stage_now != stage::RECORDED {
        return false;
    }
    if stage_now == stage::PREPARED && o.reused.iter().any(|token| !copy_windows::valid(token)) {
        set_failure(failure::CHANGED);
        return false;
    }
    set_failure(failure::RETENTION);
    // SAFETY: a live list and the receipt.
    if !unsafe { cvr_gpu_has_command_resource(list, held.raw()) } {
        return false;
    }
    if !check(list, o, stage_now == stage::RECORDED) {
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
pub unsafe extern "C-unwind" fn cvr_iasn_validate(list: *mut c_void, proof: *const PreparedCopies) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| validate(list, proof))
}
/// RecordAtActualCall: on success `out[..*count]` hold the private views (new references, at most 17).
///
/// # Safety
/// `proof` is valid; `out` holds 17 views; `count` is writable; `list` is live or null; `output_empty` is whether the
/// caller's vector was empty.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_record(
    list: *mut c_void,
    proof: *const PreparedCopies,
    output_empty: bool,
    out: *mut PrivateIa,
    count: *mut u32,
) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    unsafe { *count = 0 };
    let mut held: Option<Held> = None;
    let mut recording = false;
    let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record_at_call(list, proof, output_empty, &mut held, &mut recording)));
    match body {
        Ok(Some(views)) => {
            let mut n = 0u32;
            for view in views.into_iter().take(MAX_VIEWS) {
                // SAFETY: the caller's 17 empty slots (null references, so nothing is overwritten).
                unsafe { out.add(n as usize).write(view) };
                n += 1;
            }
            // SAFETY: the caller's count.
            unsafe { *count = n };
            true
        }
        Ok(None) => false,
        Err(_) => {
            set_failure(failure::EXCEPTION);
            if let (true, Some(h)) = (recording, held.as_ref()) {
                let o = h.get();
                if o.entries.iter().any(|e| e.mutated) {
                    let recovered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recover(list, o))).unwrap_or(false);
                    set_failure(if recovered { failure::RECOVERY_VERIFIED } else { failure::RECOVERY_FATAL });
                }
                failed(o);
            }
            false
        }
    }
}
fn record_at_call(
    list: *mut c_void,
    proof: &PreparedCopies,
    output_empty: bool,
    held: &mut Option<Held>,
    recording: &mut bool,
) -> Option<Vec<PrivateIa>> {
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return None;
    }
    set_failure(failure::ARGUMENTS);
    *held = owner(proof);
    let h = held.as_ref()?;
    let o = h.get();
    if !public_proof(proof, o) || !output_empty {
        return None;
    }
    let result: Vec<PrivateIa> = o.views.iter().map(PrivateIa::duplicate).collect();
    if !validate(list, proof) {
        return None;
    }
    set_failure(failure::ALREADY_RECORDED);
    if o.stage.compare_exchange(stage::PREPARED, stage::RECORDING, SeqCst, SeqCst).is_err() {
        return None;
    }
    *recording = true;
    let mut ok = true;
    for i in 0..o.entries.len() {
        let duplicate = (0..i).any(|j| same_native(o.entries[i].native(), o.entries[j].native()));
        if duplicate {
            continue;
        }
        let mut context = Context::default();
        set_failure(failure::CHANGED);
        if !identity(list, o, &o.entries[i], false, &mut context) {
            ok = false;
            break;
        }
        let entry = &o.entries[i];
        let state = read_state(list, &entry.source.0);
        let kind = entry.native().kind;
        let unready = if kind == 1 {
            state.status != 8 || state.kind != 1 || state.value != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32
        } else {
            !rules::read_mask(state, kind, entry.reads, false)
        };
        if unready {
            set_failure(failure::STATE);
            ok = false;
            break;
        }
        let required = state.value | D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
        o.entries[i].required = required;
        if kind != 1 && state.value & D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 != 0 {
            continue;
        }
        // SAFETY: plain values.
        if kind != 1 && !unsafe { crate::ia_snapshot_native::cvr_ian_transition_capacity(o.entries[i].native()) } {
            set_failure(failure::CAPACITY);
            ok = false;
            break;
        }
        set_failure(failure::REGISTRATION);
        if kind != 1 {
            o.entries[i].mutated = true;
        }
        register(o.base, context.address, o.entries[i].native().handle, required);
        if kind != 1 {
            flush(o.base, context.address);
        }
    }
    if ok {
        ok = check(list, o, true);
    }
    if ok && o.reused.iter().any(|token| !copy_windows::valid(token)) {
        set_failure(failure::CHANGED);
        ok = false;
    }
    if !ok {
        if o.entries.iter().any(|e| e.mutated) {
            set_failure(if recover(list, o) { failure::RECOVERY_VERIFIED } else { failure::RECOVERY_FATAL });
        }
        failed(o);
        return None;
    }
    let arena = arena_of(o);
    {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: a live list; the arena's resource and the sources are owned references.
        if let (Some(l), Some(destination)) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe {
            ID3D12Resource::from_raw_borrowed(&arena.resource)
        }) {
            for copy in &o.transfers {
                let source = copy.resource as *mut c_void;
                if let Some(source) = unsafe { ID3D12Resource::from_raw_borrowed(&source) } {
                    unsafe { l.CopyBufferRegion(destination, copy.destination, source, copy.source_offset, copy.bytes) };
                }
            }
        }
    }
    if let Some(window) = &arena.window {
        for range in &o.transfers {
            if let Some(entry) = o.entries.iter().find(|item| item.resource() == range.resource) {
                if entry.source.0.flags == 0 {
                    window.publish(Span {
                        resource: range.resource,
                        gpu: entry.source.0.gpu,
                        backing: entry.source.0.bytes,
                        offset: range.source_offset,
                        bytes: range.bytes,
                        destination: range.destination,
                        occurrence: o.epoch.occurrence,
                    });
                }
            }
        }
    }
    arena.pending.fetch_sub(1, SeqCst);
    o.stage.store(stage::RECORDED, Release);
    set_failure(failure::NONE);
    Some(result)
}
/// ValidateSourceSubmission.
///
/// # Safety
/// `proof` is valid; `queue` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_validate_submission(proof: *const PreparedCopies, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let held = owner(proof);
        let Some(h) = held.as_ref().filter(|_| !queue.is_null()) else {
            return false;
        };
        let o = h.get();
        if !public_proof(proof, o) || o.stage.load(Acquire) != stage::RECORDED {
            return false;
        }
        let arena = arena_of(o);
        if !arena.finalized.load(Acquire) || arena.failed.load(Acquire) {
            return false;
        }
        set_failure(failure::SUBMISSION);
        // SAFETY: the receipt's lease and a live queue.
        if !unsafe { crate::native_recording::cvr_native_recording_validate_submission(&o.recording.0, queue) } {
            return false;
        }
        set_failure(failure::CHANGED);
        if o.entries.iter().any(|entry| !physical(entry, o.base)) {
            return false;
        }
        set_failure(failure::NONE);
        true
    })
}
/// CopiedSourceShape: `*output = ` the recorded draw's DrawResources.
///
/// # Safety
/// `proof` is valid; `output` is a geometry::DrawResources.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iasn_copied_shape(proof: *const PreparedCopies, output: *mut c_void) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    let Some(h) = owner(proof) else {
        return false;
    };
    let o = h.get();
    if !public_proof(proof, o) || o.stage.load(Acquire) != stage::RECORDED {
        return false;
    }
    let arena = arena_of(o);
    if !arena.finalized.load(SeqCst) || arena.failed.load(SeqCst) {
        return false;
    }
    let Some(host) = host() else {
        return false;
    };
    if o.draw_owned.is_some() {
        return false; // the Rust capture's copy is not a C++ DrawResources
    }
    // SAFETY: the host's own assignment between two DrawResources.
    unsafe { (host.assign)(output, o.draw) };
    true
}
/// CopiedSourceShape for Rust callers: the proof's recorded geometry::DrawResources (the same checks), alive with
/// the proof's owner.
pub fn copied_shape(proof: &PreparedCopies) -> Option<*const c_void> {
    let h = owner(proof)?;
    let o = h.get();
    if !public_proof(proof, o) || o.stage.load(Acquire) != stage::RECORDED {
        return None;
    }
    let arena = arena_of(o);
    if !arena.finalized.load(SeqCst) || arena.failed.load(SeqCst) {
        return None;
    }
    Some(o.draw as *const c_void)
}
/// CopiedSourceShape for the Rust capture: the proof's recorded draw (its own copy), alive with the proof's owner.
pub fn copied_shape_owned(proof: &PreparedCopies) -> Option<*const crate::geometry_leases::DrawResources> {
    let h = owner(proof)?;
    let o = h.get();
    if !public_proof(proof, o) || o.stage.load(Acquire) != stage::RECORDED {
        return None;
    }
    let arena = arena_of(o);
    if !arena.finalized.load(SeqCst) || arena.failed.load(SeqCst) {
        return None;
    }
    o.draw_owned.as_deref().map(|d| d as *const crate::geometry_leases::DrawResources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dormant_without_the_switch() {
        assert_eq!(CyberpunkVR_NativeIaSnapshotsEnable.load(Relaxed), 0);
        let mut stamp = ScopeStamp { nonce: 7, ..Default::default() };
        // SAFETY: a local stamp.
        unsafe { cvr_iasn_scope_enter(1, 2, 3, &mut stamp) };
        assert_eq!(stamp, ScopeStamp::default());
        let mut out = core::ptr::null_mut();
        // SAFETY: null objects are refused before use.
        let hr = unsafe { cvr_iasn_arena_create(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 1, 16, &mut out) };
        assert_eq!(hr, E_ACCESSDENIED);
        assert!(out.is_null());
        assert_eq!(cvr_iasn_last_failure(), failure::DISABLED);
    }

    #[test]
    fn proofs_without_an_owner_are_refused() {
        let proof = PreparedCopies {
            owner: core::ptr::null_mut(),
            list: 0,
            generation: 0,
            occurrence: 0,
            copied_bytes: 0,
            count: 0,
            transferred_bytes: 0,
            reused_bytes: 0,
        };
        // SAFETY: a local proof.
        assert!(!unsafe { cvr_iasn_validate(core::ptr::null_mut(), &proof) });
        assert_eq!(cvr_iasn_last_failure(), failure::ARGUMENTS);
        // SAFETY: as above; nothing is written without an owner.
        assert!(!unsafe { cvr_iasn_validate_submission(&proof, core::ptr::null_mut()) });
        assert!(!unsafe { cvr_iasn_copied_shape(&proof, core::ptr::null_mut()) });
    }

    #[test]
    fn storage_and_receipt_answer_their_interfaces() {
        let raw = Box::into_raw(Box::new(Storage {
            vtbl: &STORAGE_VTBL,
            references: AtomicU32::new(1),
            resource: core::ptr::null_mut(),
            device: core::ptr::null_mut(),
            list: 0,
            generation: 0,
            gpu: 0,
            capacity: 0,
            cursor: 48,
            thread: 0,
            recording: OwnedRecording::none(),
            context: UploadContext::default(),
            pending: AtomicU32::new(0),
            finalized: AtomicBool::new(true),
            failed: AtomicBool::new(false),
            window: None,
        }))
        .cast::<c_void>();
        // SAFETY: the storage made above.
        unsafe {
            assert!(cvr_iasn_arena_finalized(raw));
            assert_eq!(cvr_iasn_arena_bytes(raw), 48);
            cvr_iasn_arena_abort(raw);
            assert!(!cvr_iasn_arena_finalized(raw));
            let mut out = core::ptr::null_mut();
            assert_eq!(storage_query(raw, &RECEIPT_TYPE, &mut out), E_NOINTERFACE);
            assert_eq!(storage_query(raw, &IID_IUNKNOWN, &mut out), S_OK);
            assert_eq!(storage_release(out), 1);
            assert_eq!(storage_release(raw), 0);
        }
    }
}
