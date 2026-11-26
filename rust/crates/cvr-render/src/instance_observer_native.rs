//! The native instance observer (formerly src/Render/NativeInstanceObserver.cpp): it watches the engine's per-instance
//! upload rings through the genuine writer (0x337144) and stamp-publication (0x4F34FC) detours and the successful-CAS
//! reservation witness (CvrNativeInstanceReservationObserve, called by the witness thunk), pins each ring's backing (the
//! upload buffer, its pooled allocation and allocator, the native record, the API creation receipt), keeps the instance
//! ledger (native::instance_ring) of the writers' sampled CPU bytes and their publications, and answers the retained
//! geometry's questions at the original draw: the owned CPU candidate of the instance range the draw reads (a COM
//! receipt pins its backing, recording, draw and graph parent), its check at the original submission and the
//! predicated GPU copies of the original range.
//!
//! The C++ face (include/Render/NativeInstanceObserver.hpp, the `CVR_RUST_VRIK` branch of the .cpp) forwards to
//! `cvr_nio2_*`; a writer or publication call is an `Arc<CallData>` the C++ `Call` holds. The C++-typed values -- the
//! graph parent's revalidation and the pipeline's shader snapshot (its instance layout, its identity) -- come through
//! the host table the .cpp attaches. The observer's gate is reentrant like the C++ std::recursive_mutex.
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::*};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError, Weak};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::declared_moved_exports::{
    CyberpunkVR_NativeInstanceObserverDiagnostics as DIAGNOSTICS, CyberpunkVR_NativeInstanceObserverEnable as ENABLE,
    CyberpunkVR_NativeInstanceObserverFirstLoss as FIRST_LOSS, CyberpunkVR_NativeInstanceObserverInstalled as INSTALLED,
    CyberpunkVR_NativeInstanceObserverResetRequest as RESET_REQUEST,
};
use crate::geometry_commands::DrawRecord;
use crate::geometry_leases::{Lease as BufferLease, NativeBuffer};
use crate::ia_snapshot_same::CreationInfo;
use crate::instance_layout::{self as layout, RingFields};
use crate::instance_observer_plan::{self as plan, CachedCursor, Cursor, ReservationRange};
use crate::native::instance_ring::{
    instance_range, CompletedWriter, CpuCandidate, Ledger, Limits, Payload, Publication, ReadRequest, RingObservation,
    Scope, Status as Ring, WriterTicket,
};
use crate::native::upload_reservation::ThunkFrame;
use crate::native_recording::{Context, Lease as RawRecording};
use crate::preparation_trace::GraphParentRecord;
use crate::source_cbv_details::{read_allocation, read_allocator, read_context, Allocation, Allocator, KNOWN};
use crate::used_moved_exports_rest::CyberpunkVR_NativeInstanceGpuProbePhase as PROBE_PHASE;

const MAX_CURSORS: usize = 4096;
const MAX_WRITERS: usize = 4096;
const RECORD_BYTES: usize = 0xB0;

/// instance_observer::Status.
#[repr(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    Disabled,
    Observed,
    Code,
    Caller,
    Contention,
    Scope,
    Backing,
    Changed,
    Capacity,
    Reservation,
    Cache,
    Returned,
    Sample,
    NoProducer,
    Range,
    Pending,
    Parent,
    Layout,
    Exception,
}
/// instance_observer::QueryField.
mod field {
    pub const STATUS: usize = 0;
    pub const FLAGS: usize = 1;
    pub const OBSERVER_EPOCH: usize = 2;
    pub const SOURCE_LIST: usize = 3;
    pub const SOURCE_GENERATION: usize = 4;
    pub const SOURCE_OCCURRENCE: usize = 5;
    pub const SOURCE_EYE: usize = 6;
    pub const RING: usize = 7;
    pub const RING_GENERATION: usize = 8;
    pub const RESOURCE: usize = 9;
    pub const HANDLE: usize = 10;
    pub const KIND: usize = 11;
    pub const STRIDE: usize = 12;
    pub const WIDTH: usize = 13;
    pub const VIEW_OFFSET: usize = 14;
    pub const FIRST_INSTANCE: usize = 15;
    pub const INSTANCES: usize = 16;
    pub const ABSOLUTE_BEGIN: usize = 17;
    pub const RESERVATION_SEQUENCE: usize = 18;
    pub const RESERVATION_BEGIN: usize = 19;
    pub const RESERVATION_END: usize = 20;
    pub const RESERVATION_HEAD_BEFORE: usize = 21;
    pub const RETIRED_TAIL: usize = 22;
    pub const PUBLISHED_STAMP: usize = 23;
    pub const PUBLISHED_HEAD: usize = 24;
    pub const CPU_BYTES: usize = 25;
    pub const PRODUCER_LIST: usize = 26;
    pub const PRODUCER_GENERATION: usize = 27;
    pub const PRODUCER_OCCURRENCE: usize = 28;
    pub const PRODUCER_THREAD: usize = 29;
    pub const PRODUCER_STAMP: usize = 30;
    pub const LIVE_CPU_BYTES: usize = 31;
}
/// instance_observer::QueryFlag.
const OWNED_CPU: u64 = 1;
const BACKING_PINNED: u64 = 2;
const CURRENT_IA: u64 = 4;
const CURRENT_RECORDING: u64 = 8;
const CURRENT_PARENT: u64 = 16;
const PUBLICATION_OBSERVED: u64 = 32;

// ---- the C++ host

/// The .cpp's host table: the C++-typed checks.
#[repr(C)]
pub struct Host {
    /// shader_snapshot::Capture(pipeline) exists and detail::InstanceLayout(its inputs, stride).
    pub layout: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    /// shader_snapshot::Capture(pipeline).get() == snapshot.
    pub shader: unsafe extern "C" fn(*mut c_void, *const c_void) -> bool,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// The .cpp's host table (static storage; the plugin's .cpp passes null, its host being this module's own) and the
/// detours' registration.
///
/// # Safety
/// `host` lives for the process (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_attach(host: *const Host) {
    HOST.store(host as *mut Host, SeqCst);
    register_detours();
}

// ---- the genuine writer's and stamp publication's detours (formerly DetourRegistry.cpp's ObserveInstanceWriter and
// ObserveInstancePublish), registered with the engine detours when the .cpp attaches (its static initialization)

const WRITER_RVA: usize = 0x337144;
const PUBLISH_RVA: usize = 0x4F34FC;
/// The trampolines to the originals (MinHook writes them at install).
static mut ORIGINAL_WRITER: usize = 0;
static mut ORIGINAL_PUBLISH: usize = 0;
/// The address the detour returns to (`_ReturnAddress()`): frame 0 is the detour, frame 1 its caller.
#[inline(always)]
fn return_address() -> u64 {
    let mut frames = [null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 {
        0
    } else {
        frames[0] as u64
    }
}
/// The detours' wanted: the code guards hold (Compatible).
extern "C" fn detours_compatible() -> bool {
    compatible()
}
/// ObserveInstanceWriter: the call entered around the genuine writer, its first index returned.
unsafe extern "C" fn observe_writer(ring: *mut c_void, bytes: *const c_void, count: u32, cursor: *mut u32) -> u32 {
    // SAFETY: MinHook's trampoline to the original writer.
    let original: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut u32) -> u32 = unsafe { core::mem::transmute(ORIGINAL_WRITER) };
    if dormant() {
        // SAFETY: the game's own call, forwarded.
        return unsafe { original(ring, bytes, count, cursor) };
    }
    let caller = return_address();
    let call = guarded(None, || enter(ring as u64, count, cursor as u64, caller));
    // SAFETY: as above.
    let result = unsafe { original(ring, bytes, count, cursor) };
    if let Some(call) = &call {
        // SAFETY: the live call (Returned).
        unsafe { cvr_nio2_returned(Arc::as_ptr(call), result) };
    }
    guarded((), || drop(call));
    result
}
/// ObserveInstancePublish: the publication entered around the genuine stamp publication.
unsafe extern "C" fn observe_publish(ring: *mut c_void) {
    // SAFETY: MinHook's trampoline to the original publication.
    let original: unsafe extern "C" fn(*mut c_void) = unsafe { core::mem::transmute(ORIGINAL_PUBLISH) };
    if dormant() {
        // SAFETY: the game's own call, forwarded.
        unsafe { original(ring) };
        return;
    }
    let caller = return_address();
    let call = guarded(None, || publication(ring as u64, caller));
    // SAFETY: as above.
    unsafe { original(ring) };
    if let Some(call) = &call {
        // SAFETY: the live call (Returned).
        unsafe { cvr_nio2_published(Arc::as_ptr(call)) };
    }
    guarded((), || drop(call));
}
fn register_detours() {
    use cvr_engine::detours::register;
    let detour = |f: usize| f as *mut c_void;
    register(
        "[instance] completed writer",
        WRITER_RVA,
        detour(observe_writer as *const () as usize),
        core::ptr::addr_of_mut!(ORIGINAL_WRITER).cast(),
        Some(detours_compatible),
        null_mut(),
    );
    register(
        "[instance] stamp publication",
        PUBLISH_RVA,
        detour(observe_publish as *const () as usize),
        core::ptr::addr_of_mut!(ORIGINAL_PUBLISH).cast(),
        Some(detours_compatible),
        null_mut(),
    );
}
/// The plugin's own host (formerly lent by src/Render/NativeInstanceObserver.cpp): the pipeline's shader snapshot
/// (shader_snapshot_native.rs). The Rust test variants attach theirs.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::Host;
    use core::ffi::c_void;
    /// shader_snapshot::Capture(pipeline) exists and detail::InstanceLayout(its inputs, stride).
    unsafe extern "C" fn layout(pipeline: *mut c_void, stride: u32) -> bool {
        let Some(source) = crate::shader_snapshot_native::source(pipeline) else { return false };
        let inputs = &source.info().inputs;
        if inputs.len() > 32 {
            return false;
        }
        let rows: Vec<crate::instance_observer_plan::LayoutInput> = inputs
            .iter()
            .map(|i| crate::instance_observer_plan::LayoutInput { format: i.format, slot: i.slot, offset: i.offset, classification: i.classification, step: i.step })
            .collect();
        // SAFETY: the rows and their count.
        unsafe { crate::instance_observer_plan::cvr_instance_layout(rows.as_ptr(), rows.len(), stride) }
    }
    /// shader_snapshot::Capture(pipeline).get() == snapshot.
    unsafe extern "C" fn shader(pipeline: *mut c_void, snapshot: *const c_void) -> bool {
        crate::shader_snapshot_native::source(pipeline).map_or(core::ptr::null(), |s| s.info().mirror.identity) == snapshot
    }
    pub static HOST: Host = Host { layout, shader };
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached once with static storage.
    let attached = unsafe { HOST.load(SeqCst).as_ref() };
    #[cfg(feature = "rust-build")]
    let attached = attached.or(Some(&plugin::HOST));
    attached
}
/// preparation::RevalidateGraphParent (preparation_trace.rs).
fn revalidate(parent: &GraphParentRecord) -> bool {
    crate::preparation_trace::revalidate_graph_parent(parent)
}

// ---- thread-local results

thread_local! {
    static LAST: Cell<Status> = const { Cell::new(Status::Disabled) };
    static QUERY: Cell<[u64; 32]> = const { Cell::new([0; 32]) };
    static WRITER: Cell<*const CallData> = const { Cell::new(null()) };
}
fn set_last(status: Status) {
    let _ = LAST.try_with(|last| last.set(status));
}
fn put(index: usize, value: u64) {
    let _ = QUERY.try_with(|query| {
        let mut values = query.get();
        values[index] = value;
        query.set(values);
    });
}
fn query_value(index: usize) -> u64 {
    QUERY.try_with(|query| query.get()[index]).unwrap_or(0)
}
fn writer() -> *const CallData {
    WRITER.try_with(Cell::get).unwrap_or(null())
}
fn set_writer(value: *const CallData) {
    let _ = WRITER.try_with(|w| w.set(value));
}
/// Fail: the status, also the query's.
fn fail(status: Status) -> bool {
    set_last(status);
    put(field::STATUS, status as u64);
    false
}

// ---- engine reads

fn tid() -> u64 {
    // SAFETY: no preconditions.
    u64::from(unsafe { GetCurrentThreadId() })
}
fn base() -> u64 {
    // SAFETY: the process image's handle.
    unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |h| h.0 as u64)
}
fn mem(address: u64, out: &mut [u8]) -> bool {
    layout::read(address, out)
}
unsafe extern "C" fn read_callback(_: *mut c_void, address: u64, out: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the caller's output of `bytes` bytes.
    layout::read(address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
fn field_u64(raw: &[u8; RECORD_BYTES], offset: usize) -> u64 {
    u64::from_le_bytes(raw[offset..offset + 8].try_into().expect("8 bytes"))
}
fn read_cursor(address: u64) -> Option<Cursor> {
    let mut bytes = [0u8; 8];
    layout::read(address, &mut bytes).then(|| Cursor {
        first: u32::from_le_bytes(bytes[..4].try_into().expect("4 bytes")),
        end: u32::from_le_bytes(bytes[4..].try_into().expect("4 bytes")),
    })
}
fn enabled() -> bool {
    ENABLE.load(Acquire) != 0 && INSTALLED.load(Acquire) == 7 && !cvr_host::probe::internal()
}
/// Retain: a guarded AddRef of the object an engine record points at, the reference owned.
fn retain(address: u64) -> Option<IUnknown> {
    // SAFETY: seh.c's guarded AddRef; on success this value owns the reference.
    (address != 0 && unsafe { cvr_engine::code::guarded_add_ref(address as usize) }).then(|| unsafe { IUnknown::from_raw(address as *mut c_void) })
}
fn current() -> Context {
    let value = layout::current();
    Context { context: value[0], state: value[1], list: value[2] }
}
fn native_caller(caller: u64, returns: &[u32]) -> bool {
    let base = base();
    caller >= base && returns.iter().any(|&r| u64::from(r) == caller - base)
}

// ---- recordings

/// native_recording::Lease with its token owned.
#[derive(Clone, Default)]
struct Recording {
    token: Option<IUnknown>,
    generation: u64,
    context: u64,
    list: u64,
    registry: u64,
}
impl Recording {
    fn raw(&self) -> RawRecording {
        RawRecording { token: self.token.as_ref().map_or(null_mut(), |t| t.as_raw()), generation: self.generation, context: self.context, list: self.list, registry: self.registry }
    }
    /// A copy of the C++ lease (its token AddRef'd).
    fn copied(raw: &RawRecording) -> Self {
        // SAFETY: the C++ lease's live token or null.
        Self { token: unsafe { IUnknown::from_raw_borrowed(&raw.token) }.cloned(), generation: raw.generation, context: raw.context, list: raw.list, registry: raw.registry }
    }
}
fn validate_recording(recording: &RawRecording, list: u64, context: &Context) -> bool {
    // SAFETY: a lease of the observer's or the caller's and a live list.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list as *mut c_void, context) }
}
/// Scope: the current camera context's recording, registry and stamp for one observed native call.
fn capture_scope(occurrence: u64) -> Option<(Scope, Recording)> {
    let context = current();
    if !context.valid() {
        return None;
    }
    let registry = layout::read_u64(base().wrapping_add(0x3BCDCA8)).filter(|&r| r != 0)?;
    let mut raw = RawRecording { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
    // SAFETY: the context's list; a writable lease.
    let captured = unsafe { crate::native_recording::cvr_native_recording_capture(context.list as *mut c_void, &context, &mut raw) };
    let recording = Recording {
        // SAFETY: the capture's token reference, handed over.
        token: (!raw.token.is_null()).then(|| unsafe { IUnknown::from_raw(raw.token) }),
        generation: raw.generation,
        context: raw.context,
        list: raw.list,
        registry: raw.registry,
    };
    if !captured || recording.registry != registry {
        return None;
    }
    let stamp = layout::stamp(registry, context.context)?;
    let native = read_context(&mem, context.context, context.list);
    if native.observation != KNOWN || !validate_recording(&recording.raw(), context.list, &context) {
        return None;
    }
    Some((Scope { list: context.list, generation: recording.generation, occurrence, thread: tid(), context: context.context, registry, stamp }, recording))
}

// ---- backings

/// A ring's pinned backing: the upload buffer (its lease), its creation receipt, the pooled allocation and allocator
/// (pinned and as read), the native record and the observation it gives.
struct Backing {
    resource: ID3D12Resource,
    source: BufferLease,
    creation: CreationInfo,
    _creation_heap: Option<ID3D12Heap>,
    allocator: IUnknown,
    allocation: IUnknown,
    native_allocation: Allocation,
    native_allocator: Allocator,
    raw: [u8; RECORD_BYTES],
    record_address: u64,
    cookie: u64,
    observed: RingObservation,
    quarantined: Cell<bool>,
}
/// QueryResourceCreation with the receipt's heap kept.
fn creation_of(resource: &ID3D12Resource) -> Option<(CreationInfo, Option<ID3D12Heap>)> {
    let c = crate::creation_receipts::query(resource.as_raw())?;
    let info = CreationInfo {
        kind: c.kind,
        initial_state_known: c.state.is_some(),
        initial_state: c.state.unwrap_or(0),
        resource: resource.as_raw(),
        heap: c.heap.as_ref().map_or(null_mut(), |h| h.as_raw()),
        heap_offset: c.offset,
        allocation_bytes: c.bytes,
    };
    Some((info, c.heap))
}
fn upload_heap(resource: &ID3D12Resource) -> bool {
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok() && heap.Type == D3D12_HEAP_TYPE_UPLOAD
}
/// Allocation: the pooled allocation is still the captured one.
fn allocation_current(value: &Backing) -> bool {
    crate::instance_track::allocation_current(
        Some(read_callback),
        null_mut(),
        base(),
        value.allocation.as_raw() as u64,
        value.allocator.as_raw() as u64,
        value.observed.registry,
        &value.native_allocation,
        &value.native_allocator,
    )
}
/// Physical: the backing as it is now (same record, allocation and creation; current cursors), or None.
fn physical(value: &Backing) -> Option<RingObservation> {
    let fields = layout::fields(value.observed.ring)?;
    if !layout::class(&fields) || !crate::instance_backing::ring_matches(&fields, &value.observed) {
        return None;
    }
    let mut raw = [0u8; RECORD_BYTES];
    let address = layout::record(value.observed.registry, fields.handle, &mut raw)?;
    if address != value.record_address || raw != value.raw || !allocation_current(value) {
        return None;
    }
    let (creation, _heap) = creation_of(&value.resource)?;
    if !crate::instance_backing::same_creation(&creation, &value.creation) {
        return None;
    }
    let mut output = value.observed;
    output.head = fields.head;
    output.tail = fields.tail;
    if !crate::instance_backing::cursors_valid(&output) {
        return None;
    }
    // SAFETY: the held resource.
    let desc = unsafe { value.resource.GetDesc() };
    (desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width == output.width
        && desc.Flags.0 == 0
        // SAFETY: the held resource.
        && unsafe { value.resource.GetGPUVirtualAddress() } == output.gpu
        && upload_heap(&value.resource))
        .then_some(output)
}
/// Capture: the ring's backing pinned from its native record.
fn capture(ring: u64, scope: &Scope) -> Option<Backing> {
    let fields = layout::fields(ring).filter(layout::class)?;
    let mut raw = [0u8; RECORD_BYTES];
    let record_address = layout::record(scope.registry, fields.handle, &mut raw)?;
    if !crate::instance_backing::record_shape(&raw, fields.mapped) {
        return None;
    }
    let resource_address = field_u64(&raw, 0x10);
    let owner = retain(resource_address)?;
    let resource: ID3D12Resource = owner.cast().ok()?;
    // SAFETY: the held resource.
    let desc = unsafe { resource.GetDesc() };
    // SAFETY: the held resource.
    let gpu = unsafe { resource.GetGPUVirtualAddress() };
    let width = u64::from(fields.capacity) * u64::from(fields.stride);
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || desc.Width != width
        || desc.Flags.0 != 0
        || gpu == 0
        || gpu > u64::MAX - width
        || fields.mapped > u64::MAX - width
        || !crate::instance_backing::record_buffer(&raw, gpu, width)
        || !upload_heap(&resource)
    {
        return None;
    }
    let (creation, creation_heap) = creation_of(&resource)?;
    if creation.kind != 2 || creation.heap.is_null() || !creation.initial_state_known || creation.initial_state != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32 {
        return None;
    }
    let allocation = field_u64(&raw, 0x18);
    let native_allocation = read_allocation(&mem, base(), allocation);
    let native_allocator = read_allocator(&mem, base(), scope.registry, &native_allocation);
    if native_allocation.observation != KNOWN
        || native_allocation.kind != 1
        || native_allocation.resource != resource_address
        || native_allocation.bytes != creation.allocation_bytes
        || native_allocation.bytes < width
        || native_allocation.heap != creation.heap as u64
        || native_allocator.observation != KNOWN
        || !native_allocator.pimpl_matches
    {
        return None;
    }
    let allocator = retain(native_allocator.address)?;
    let allocation_owner = retain(allocation)?;
    let cookie = crate::descriptor_audit::source_resource_cookie_at_creation(resource.as_raw());
    if cookie == 0 {
        return None;
    }
    let source = BufferLease {
        resource: resource.as_raw(),
        gpu,
        bytes: width,
        heap_type: D3D12_HEAP_TYPE_UPLOAD.0 as u32,
        flags: 0,
        native: NativeBuffer { resource: resource_address, gpu, bytes: width, registry: scope.registry, context: scope.context, handle: fields.handle, kind: 5 },
    };
    let observed = RingObservation {
        ring,
        generation: 1,
        resource: resource_address,
        gpu,
        mapped: fields.mapped,
        registry: scope.registry,
        allocation,
        allocator: native_allocator.address,
        heap: creation.heap as u64,
        width,
        capacity: u64::from(fields.capacity),
        head: fields.head,
        tail: fields.tail,
        handle: fields.handle,
        stride: fields.stride,
        kind: 5,
    };
    let mut value = Backing {
        resource,
        source,
        creation,
        _creation_heap: creation_heap,
        allocator,
        allocation: allocation_owner,
        native_allocation,
        native_allocator,
        raw,
        record_address,
        cookie,
        observed,
        quarantined: Cell::new(false),
    };
    value.observed = physical(&value)?;
    Some(value)
}
fn same_backing(a: &Backing, b: &Backing) -> bool {
    crate::instance_track::same_backing(&a.observed, &b.observed, a.cookie, b.cookie, &a.raw, &b.raw)
}

// ---- the state and its gate

struct WriterRecord {
    backing: Arc<Backing>,
    completed: CompletedWriter,
    reservation: ReservationRange,
    presence: Weak<WriterTicket>,
}
struct State {
    ledger: Ledger,
    epoch: u64,
    nonce: u64,
    generation: u64,
    reservation: u64,
    active: usize,
    rings: Vec<Arc<Backing>>,
    cursors: Vec<CachedCursor>,
    writers: Vec<WriterRecord>,
    unknown_rings: Vec<u64>,
}
impl State {
    fn new() -> Self {
        Self { ledger: Ledger::new(Limits::default()), epoch: 1, nonce: 0, generation: 0, reservation: 0, active: 0, rings: Vec::new(), cursors: Vec::new(), writers: Vec::new(), unknown_rings: Vec::new() }
    }
}
struct StateCell(UnsafeCell<Option<State>>);
// SAFETY: only touched while the gate is held.
unsafe impl Sync for StateCell {}
static STATE: StateCell = StateCell(UnsafeCell::new(None));
/// The observer's state. The caller holds the gate.
fn state() -> &'static mut State {
    // SAFETY: serialized by the gate.
    unsafe { (*STATE.0.get()).get_or_insert_with(State::new) }
}
static LOST: AtomicBool = AtomicBool::new(false);
static OBSERVED: AtomicBool = AtomicBool::new(false);

/// The gate (the C++ std::recursive_mutex): the thread holding it enters again.
struct Gate {
    lock: Mutex<()>,
    owner: AtomicU32,
    depth: UnsafeCell<u32>,
    held: UnsafeCell<Option<MutexGuard<'static, ()>>>,
}
// SAFETY: depth and held are touched only by the owning thread.
unsafe impl Sync for Gate {}
static GATE: Gate = Gate { lock: Mutex::new(()), owner: AtomicU32::new(0), depth: UnsafeCell::new(0), held: UnsafeCell::new(None) };
impl Gate {
    fn me() -> u32 {
        // SAFETY: no preconditions.
        unsafe { GetCurrentThreadId() }
    }
    fn take(&'static self, guard: MutexGuard<'static, ()>) {
        // SAFETY: this thread now owns the lock.
        unsafe {
            *self.held.get() = Some(guard);
            *self.depth.get() = 1;
        }
        self.owner.store(Self::me(), Relaxed);
    }
    fn enter(&'static self) {
        if self.owner.load(Relaxed) == Self::me() {
            // SAFETY: the owning thread.
            unsafe { *self.depth.get() += 1 };
            return;
        }
        self.take(self.lock.lock().unwrap_or_else(|e| e.into_inner()));
    }
    fn try_enter(&'static self) -> bool {
        if self.owner.load(Relaxed) == Self::me() {
            // SAFETY: the owning thread.
            unsafe { *self.depth.get() += 1 };
            return true;
        }
        match self.lock.try_lock() {
            Ok(guard) => self.take(guard),
            Err(TryLockError::Poisoned(e)) => self.take(e.into_inner()),
            Err(TryLockError::WouldBlock) => return false,
        }
        true
    }
    fn leave(&'static self) {
        // SAFETY: the owning thread.
        unsafe {
            *self.depth.get() -= 1;
            if *self.depth.get() == 0 {
                self.owner.store(0, Relaxed);
                drop((*self.held.get()).take());
            }
        }
    }
}
/// A held level of the gate (std::lock_guard / std::unique_lock).
struct Held {
    active: bool,
}
impl Held {
    fn new() -> Self {
        GATE.enter();
        Self { active: true }
    }
    fn try_new() -> Option<Self> {
        GATE.try_enter().then(|| Self { active: true })
    }
    fn unlock(&mut self) {
        if self.active {
            GATE.leave();
            self.active = false;
        }
    }
    fn relock(&mut self) {
        if !self.active {
            GATE.enter();
            self.active = true;
        }
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        self.unlock();
    }
}

fn lose(status: Status) {
    set_last(status);
    if LOST.compare_exchange(false, true, AcqRel, Acquire).is_ok() {
        FIRST_LOSS.store(status as u32 + 1, Release);
    }
}
fn healthy() -> bool {
    !LOST.load(Acquire)
}
fn apply_loss(s: &mut State) {
    if !healthy() {
        s.ledger.unknown(0);
        s.cursors.clear();
        s.writers.clear();
    }
}
fn clear_state(s: &mut State) {
    s.ledger.reset();
    s.rings.clear();
    s.cursors.clear();
    s.writers.clear();
    s.unknown_rings.clear();
    s.epoch += 1;
    OBSERVED.store(false, Release);
    LOST.store(false, Release);
}
fn reset_at_idle(s: &mut State) -> bool {
    if RESET_REQUEST.load(Acquire) == 0 {
        return true;
    }
    if s.active != 0 {
        lose(Status::Pending);
        return false;
    }
    if s.epoch == u64::MAX {
        lose(Status::Capacity);
        return false;
    }
    clear_state(s);
    FIRST_LOSS.store(0, Release);
    for count in &DIAGNOSTICS {
        count.store(0, Relaxed);
    }
    RESET_REQUEST.store(0, Release);
    true
}
fn local_unknown(s: &mut State, ring: u64, status: Status) {
    if (status as usize) < 19 {
        DIAGNOSTICS[status as usize].fetch_add(1, Relaxed);
    }
    set_last(status);
    s.ledger.unknown(ring);
    if let Some(at) = s.rings.iter().find(|value| value.observed.ring == ring) {
        at.quarantined.set(true);
    } else if !s.unknown_rings.contains(&ring) {
        if s.unknown_rings.len() >= 64 {
            lose(Status::Capacity);
            return;
        }
        s.unknown_rings.push(ring);
    }
    s.cursors.retain(|value| value.ring != ring);
    s.writers.retain(|value| value.backing.observed.ring != ring);
}
fn unobserved(ring: u64, status: Status) {
    set_last(status);
    if !enabled() {
        return;
    }
    let _gate = Held::new();
    if layout::fields(ring).is_some_and(|f| layout::class(&f)) {
        local_unknown(state(), ring, status);
    }
}
fn okay(value: Ring) -> bool {
    value == Ring::Observed || value == Ring::Recreated || value == Ring::Overlap
}
/// Track: the ring's backing, the same one kept, a new generation for a new one.
fn track(s: &mut State, value: Option<Backing>) -> Option<Arc<Backing>> {
    let mut value = value?;
    let at = s.rings.iter().position(|old| old.observed.ring == value.observed.ring);
    if let Some(i) = at {
        if same_backing(&s.rings[i], &value) {
            return Some(s.rings[i].clone());
        }
    }
    if s.generation == u64::MAX || (at.is_none() && s.rings.len() >= 64) {
        lose(Status::Capacity);
        return None;
    }
    s.generation += 1;
    value.observed.generation = s.generation;
    value.quarantined.set(s.unknown_rings.contains(&value.observed.ring));
    let value = Arc::new(value);
    match at {
        None => s.rings.push(value.clone()),
        Some(i) => {
            let ring = value.observed.ring;
            s.cursors.retain(|old| old.ring != ring);
            s.writers.retain(|old| old.backing.observed.ring != ring);
            s.rings[i] = value.clone();
        }
    }
    if !okay(s.ledger.observe(&value.observed)) {
        local_unknown(s, value.observed.ring, Status::Changed);
        return None;
    }
    if value.quarantined.get() {
        s.ledger.unknown(value.observed.ring);
    }
    Some(value)
}

// ---- code guards

struct CodeGuards {
    attempted: bool,
    valid: bool,
    base: u64,
}
static CODE: Mutex<CodeGuards> = Mutex::new(CodeGuards { attempted: false, valid: false, base: 0 });
fn prepare_code_guards(game_base: u64) {
    let mut code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    if code.attempted {
        return;
    }
    code.attempted = true;
    code.base = game_base;
    code.valid = game_base != 0 && game_base == base() && crate::instance_code::guards(game_base);
}
fn compatible() -> bool {
    let code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    code.valid && code.base == base()
}

// ---- the observed native calls

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Writer,
    Reserve,
    Publish,
}
struct CallInner {
    kind: Kind,
    completed: bool,
    counted: bool,
    untracked_cached: bool,
    nonce: u64,
    thread: u64,
    backing: Option<Arc<Backing>>,
    scope: Scope,
    _recording: Recording,
    before: RingFields,
    cursor_address: u64,
    cursor: Cursor,
    instances: u32,
    ticket: WriterTicket,
    previous: Option<CachedCursor>,
    actual: Option<ReservationRange>,
    publication_stamp: u64,
}
/// detail::CallData: one observed native writer or publication (the C++ Call holds it).
pub struct CallData(UnsafeCell<CallInner>);
impl CallData {
    fn new(kind: Kind) -> Self {
        Self(UnsafeCell::new(CallInner {
            kind,
            completed: false,
            counted: false,
            untracked_cached: false,
            nonce: 0,
            thread: 0,
            backing: None,
            scope: Scope::default(),
            _recording: Recording::default(),
            before: RingFields::default(),
            cursor_address: 0,
            cursor: Cursor::default(),
            instances: 0,
            ticket: WriterTicket::default(),
            previous: None,
            actual: None,
            publication_stamp: 0,
        }))
    }
    /// The call's fields: its own thread or the gate's holder (the C++ object is shared the same way).
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut CallInner {
        // SAFETY: as documented.
        unsafe { &mut *self.0.get() }
    }
}
impl Drop for CallData {
    fn drop(&mut self) {
        let _gate = Held::new();
        let s = state();
        let me = self as *const CallData;
        let c = self.0.get_mut();
        if !c.completed && c.counted {
            if healthy() {
                local_unknown(s, c.backing.as_ref().map_or(0, |b| b.observed.ring), Status::Returned);
            }
            apply_loss(s);
        }
        if writer() == me {
            set_writer(null());
        }
        if c.counted && s.active != 0 {
            s.active -= 1;
        }
    }
}
fn finish(call: &CallData) {
    call.get().completed = true;
    if writer() == call as *const CallData {
        set_writer(null());
    }
}
/// Start: an observed native call on the ring, its scope and pinned backing.
fn start(kind: Kind, ring: u64) -> Option<Arc<CallData>> {
    if !enabled() {
        if OBSERVED.load(Acquire) {
            lose(Status::Disabled);
        }
        return None;
    }
    if !compatible() {
        lose(Status::Code);
        return None;
    }
    if layout::fields(ring).is_some_and(|classification| !layout::class(&classification)) {
        return None;
    }
    let call = Arc::new(CallData::new(kind));
    let _gate = Held::new();
    let s = state();
    let c = call.get();
    if !reset_at_idle(s) {
        c.completed = true;
        return None;
    }
    if !healthy() {
        c.completed = true;
        apply_loss(s);
        return None;
    }
    let Some(fields) = layout::fields(ring) else {
        c.completed = true;
        local_unknown(s, ring, Status::Changed);
        return None;
    };
    if !layout::class(&fields) {
        c.completed = true;
        set_last(Status::Backing);
        if s.rings.iter().any(|value| value.observed.ring == ring) {
            lose(Status::Changed);
        }
        return None;
    }
    if s.nonce == u64::MAX {
        c.completed = true;
        lose(Status::Capacity);
        return None;
    }
    s.nonce += 1;
    c.nonce = s.nonce;
    c.thread = tid();
    s.active += 1;
    c.counted = true;
    let Some((scope, recording)) = capture_scope(c.nonce) else {
        c.completed = true;
        local_unknown(s, ring, Status::Scope);
        return None;
    };
    c.scope = scope;
    c._recording = recording;
    c.backing = track(s, capture(ring, &c.scope));
    if c.backing.is_none() {
        c.completed = true;
        local_unknown(s, ring, Status::Backing);
        return None;
    }
    let Some(before) = layout::fields(ring) else {
        c.completed = true;
        local_unknown(s, ring, Status::Changed);
        return None;
    };
    c.before = before;
    OBSERVED.store(true, Release);
    set_last(Status::Observed);
    Some(call)
}
/// ObserveReservationWitness: one successful CAS of the genuine ring allocator.
fn observe_reservation(frame: &ThunkFrame) {
    if !enabled() || !healthy() || frame.rax != frame.r9 || frame.flags & 0x40 == 0 {
        return;
    }
    let Some(fields) = layout::fields(frame.r10).filter(layout::class) else { return };
    if !native_caller(frame.native_caller, &[0x3371EF, 0x337211, 0x9399BB]) {
        unobserved(frame.r10, Status::Caller);
        return;
    }
    let range = if frame.rbx > u64::from(u32::MAX) || frame.r8 > u64::from(u32::MAX) || frame.capacity != u64::from(fields.capacity) {
        None
    } else {
        plan::reservation_result(frame.r9, fields.capacity, frame.rbx as u32, frame.r8 as u32, frame.rcx)
    };
    let Some(mut range) = range.filter(|r| r.absolute_begin == frame.r11) else {
        unobserved(frame.r10, Status::Reservation);
        return;
    };
    // SAFETY: the writer call this thread is inside (its C++ Call holds it).
    let parent = unsafe { writer().as_ref() };
    let mut standalone = None;
    if parent.is_none_or(|p| p.get().backing.as_ref().is_none_or(|b| b.observed.ring != frame.r10)) {
        standalone = start(Kind::Reserve, frame.r10);
        if standalone.is_none() {
            return;
        }
    }
    let _gate = Held::new();
    if !healthy() {
        return;
    }
    let s = state();
    let finish_standalone = |standalone: &Option<Arc<CallData>>| {
        if let Some(call) = standalone {
            finish(call);
        }
    };
    let Some(backing) = (match &standalone {
        Some(call) => call.get().backing.clone(),
        None => parent.and_then(|p| p.get().backing.clone()),
    }) else {
        return;
    };
    let current = physical(&backing);
    let Some(current) = current.filter(|c| range.absolute_end <= c.head) else {
        local_unknown(s, frame.r10, Status::Changed);
        finish_standalone(&standalone);
        return;
    };
    if s.reservation == u64::MAX {
        lose(Status::Capacity);
        finish_standalone(&standalone);
        return;
    }
    s.reservation += 1;
    range.sequence = s.reservation;
    if backing.quarantined.get() {
        if s.ledger.restart_observation(&current) != Ring::Observed {
            local_unknown(s, frame.r10, Status::Changed);
            finish_standalone(&standalone);
            return;
        }
        if let (Some(p), None) = (parent, &standalone) {
            p.get().ticket = WriterTicket::default();
        }
        backing.quarantined.set(false);
        s.unknown_rings.retain(|&ring| ring != current.ring);
    }
    let joined = match (parent, &standalone) {
        (Some(p), None) => {
            let p = p.get();
            p.backing.as_ref().is_some_and(|b| Arc::ptr_eq(b, &backing)) && p.thread == tid() && !p.completed
        }
        _ => false,
    };
    let mut joined_call = if joined { parent.map(CallData::get) } else { None };
    if let Some(p) = joined_call.as_deref_mut().filter(|p| p.ticket.id == 0) {
        let (status, ticket) = s.ledger.begin(&current, &p.scope);
        p.ticket = ticket;
        if status != Ring::Observed {
            local_unknown(s, frame.r10, Status::Changed);
            return;
        }
    }
    let active = joined_call.as_ref().map(|p| p.ticket);
    if !okay(s.ledger.reuse(&current, range.absolute_begin, range.instances, active.as_ref())) {
        local_unknown(s, frame.r10, Status::Changed);
        finish_standalone(&standalone);
        return;
    }
    if let Some(p) = joined_call {
        if p.actual.is_some() {
            local_unknown(s, frame.r10, Status::Reservation);
            return;
        }
        p.actual = Some(range);
        let qualified = s.ledger.qualify_write(&current, &p.ticket, range.absolute_begin, range.first, p.instances);
        if qualified != Ring::Observed && qualified != Ring::Overlap {
            local_unknown(s, frame.r10, Status::Changed);
        }
    }
    finish_standalone(&standalone);
    set_last(Status::Observed);
}
fn set_installed(mask: u32) {
    let previous = INSTALLED.swap(mask & 7, AcqRel);
    if mask & !7 != 0 || (previous != mask && OBSERVED.load(Acquire)) {
        lose(Status::Changed);
    }
}
fn dormant() -> bool {
    ENABLE.load(Acquire) == 0 && !OBSERVED.load(Acquire)
}
/// Enter: the genuine writer is about to fill `instances` records through `cursor`.
fn enter(ring: u64, instances: u32, cursor: u64, caller: u64) -> Option<Arc<CallData>> {
    if enabled() {
        DIAGNOSTICS[19].fetch_add(1, Relaxed);
    }
    if enabled() && !native_caller(caller, &[0x333CDF, 0x335991, 0x9399A4, 0x2E952C5, 0x2457F46]) {
        unobserved(ring, Status::Caller);
        return None;
    }
    let call = start(Kind::Writer, ring)?;
    let _gate = Held::new();
    let s = state();
    let c = call.get();
    let backing = c.backing.clone()?;
    let cursor_value = if !writer().is_null() || instances == 0 || cursor == 0 || 65536u32.checked_div(c.before.stride).is_none_or(|limit| instances > limit) {
        None
    } else {
        read_cursor(cursor)
    };
    let Some(cursor_value) = cursor_value else {
        c.completed = true;
        local_unknown(s, backing.observed.ring, Status::Range);
        return None;
    };
    c.cursor = cursor_value;
    c.cursor_address = cursor;
    c.instances = instances;
    let Some(current) = physical(&backing) else {
        c.completed = true;
        local_unknown(s, backing.observed.ring, Status::Changed);
        return None;
    };
    s.cursors.retain(|value| {
        !(value.ring == current.ring && value.ring_generation == current.generation && (value.reservation.absolute_end <= current.tail || value.next.first == value.next.end))
    });
    if let Some(cached) = s.cursors.iter().find(|value| value.address == c.cursor_address && value.ring == current.ring && value.ring_generation == current.generation && value.thread == c.thread) {
        c.previous = Some(*cached);
    }
    let cached_path = c.cursor.first < c.cursor.end && u64::from(instances) <= u64::from(c.cursor.end) - u64::from(c.cursor.first);
    let mut planned = plan::WriterRange::default();
    if cached_path {
        let after = Cursor { first: c.cursor.first + instances, end: c.cursor.end };
        match plan::resolve_writer(c.cursor_address, current.ring, current.generation, c.thread, current.capacity as u32, c.cursor, after, c.cursor.first, instances, None, c.previous.as_ref()) {
            Some(range) => planned = range,
            None => {
                if !backing.quarantined.get() {
                    let (status, ticket) = s.ledger.begin(&current, &c.scope);
                    c.ticket = ticket;
                    if status != Ring::Observed || !okay(s.ledger.invalidate_physical(&current, c.cursor.first, instances)) {
                        c.completed = true;
                        local_unknown(s, current.ring, Status::Range);
                        return None;
                    }
                }
                let address = c.cursor_address;
                s.cursors.retain(|value| value.address != address);
                c.untracked_cached = true;
                set_writer(Arc::as_ptr(&call));
                return Some(call);
            }
        }
    }
    if !backing.quarantined.get() {
        let (status, ticket) = s.ledger.begin(&current, &c.scope);
        c.ticket = ticket;
        if status != Ring::Observed {
            c.completed = true;
            local_unknown(s, current.ring, Status::Changed);
            return None;
        }
    }
    if c.ticket.id != 0 && cached_path && s.ledger.qualify_write(&current, &c.ticket, planned.absolute_begin, c.cursor.first, instances) != Ring::Observed {
        c.completed = true;
        local_unknown(s, current.ring, Status::Cache);
        return None;
    }
    set_writer(Arc::as_ptr(&call));
    Some(call)
}
/// Publication: the genuine stamp publication is about to run.
fn publication(ring: u64, caller: u64) -> Option<Arc<CallData>> {
    if enabled() && !native_caller(caller, &[0x4F34AF, 0x4F34BB, 0x4F34C7]) {
        unobserved(ring, Status::Caller);
        return None;
    }
    let call = start(Kind::Publish, ring)?;
    let c = call.get();
    c.publication_stamp = c.scope.stamp;
    Some(call)
}
/// Returned(Call&, first): the genuine writer returned `first`; its bytes sampled into the ledger.
fn returned(call: &CallData, first: u32) {
    let c = call.get();
    if c.completed {
        return;
    }
    let mut gate = Held::new();
    let s = state();
    if !healthy() || !enabled() || c.thread != tid() {
        finish(call);
        lose(Status::Changed);
        apply_loss(s);
        return;
    }
    let Some(backing) = c.backing.clone() else { return };
    let Some(current) = physical(&backing) else {
        finish(call);
        local_unknown(s, backing.observed.ring, Status::Changed);
        return;
    };
    if c.kind != Kind::Writer {
        finish(call);
        lose(Status::Returned);
        apply_loss(s);
        return;
    }
    if c.untracked_cached {
        let after = read_cursor(c.cursor_address);
        if c.actual.is_some() || first != c.cursor.first || after.is_none_or(|after| after.first != first.wrapping_add(c.instances) || after.end != c.cursor.end) {
            local_unknown(s, current.ring, Status::Cache);
        } else if c.ticket.id == 0 || !okay(s.ledger.finish_unobserved(&current, &c.ticket, first, c.instances)) {
            local_unknown(s, current.ring, Status::Cache);
        }
        DIAGNOSTICS[21].fetch_add(1, Relaxed);
        finish(call);
        return;
    }
    let range = read_cursor(c.cursor_address).and_then(|after| {
        plan::resolve_writer(c.cursor_address, current.ring, current.generation, c.thread, current.capacity as u32, c.cursor, after, first, c.instances, c.actual.as_ref(), c.previous.as_ref())
    });
    let Some(range) = range else {
        finish(call);
        local_unknown(s, current.ring, Status::Cache);
        return;
    };
    let Some(checked) = instance_range(&current, range.absolute_begin, first, c.instances) else {
        finish(call);
        local_unknown(s, current.ring, Status::Range);
        return;
    };
    let discard = |s: &mut State, reason: Status| {
        s.ledger.discard_completed(&call.get().ticket);
        finish(call);
        set_last(reason);
    };
    if s.ledger.qualify_write(&current, &c.ticket, range.absolute_begin, first, c.instances) != Ring::Observed {
        discard(s, Status::Pending);
        return;
    }
    if range.has_cache != 0 {
        if let Some(existing) = s.cursors.iter_mut().find(|value| value.address == c.cursor_address) {
            *existing = range.cache;
        } else {
            if s.cursors.len() >= MAX_CURSORS {
                finish(call);
                lose(Status::Capacity);
                apply_loss(s);
                return;
            }
            s.cursors.push(range.cache);
        }
    }
    if s.ledger.validate_write(&c.ticket) != Ring::Observed {
        discard(s, Status::Pending);
        return;
    }
    gate.unlock();
    let mut bytes = vec![0u8; checked.bytes as usize];
    let mut again = vec![0u8; bytes.len()];
    let at = current.mapped.wrapping_add(checked.byte_offset);
    let sampled = mem(at, &mut bytes) && mem(at, &mut again) && bytes == again;
    gate.relock();
    let s = state();
    if !sampled || !healthy() {
        discard(s, Status::Sample);
        if !healthy() {
            apply_loss(s);
        }
        return;
    }
    let Some(current) = physical(&backing) else {
        finish(call);
        local_unknown(s, backing.observed.ring, Status::Changed);
        return;
    };
    if s.ledger.validate_write(&c.ticket) != Ring::Observed {
        discard(s, Status::Pending);
        return;
    }
    let completed = CompletedWriter { writer: c.ticket, absolute_begin: range.absolute_begin, first_instance: first, instances: c.instances };
    if s.ledger.complete(&current, &completed, &bytes) != Ring::Completed {
        discard(s, Status::Pending);
        return;
    }
    s.writers.retain(|value| value.presence.strong_count() != 0);
    let presence = s.ledger.completed_presence(&c.ticket);
    if presence.strong_count() == 0 || s.writers.len() >= MAX_WRITERS {
        finish(call);
        local_unknown(s, current.ring, Status::Capacity);
        return;
    }
    s.writers.push(WriterRecord { backing, completed, reservation: range.reservation, presence });
    DIAGNOSTICS[20].fetch_add(1, Relaxed);
    finish(call);
    set_last(Status::Observed);
}
/// Returned(Call&): the genuine publication returned; its {stamp, head} entry joined to the ledger.
fn published(call: &CallData) {
    let c = call.get();
    if c.completed {
        return;
    }
    let _gate = Held::new();
    let s = state();
    if c.kind != Kind::Publish || !healthy() || !enabled() || c.thread != tid() {
        finish(call);
        lose(Status::Returned);
        apply_loss(s);
        return;
    }
    let Some(backing) = c.backing.clone() else { return };
    let Some(current) = physical(&backing) else {
        finish(call);
        local_unknown(s, backing.observed.ring, Status::Changed);
        return;
    };
    let entry = |data: u64, index: u32| {
        let mut bytes = [0u8; 16];
        mem(data.wrapping_add(u64::from(index) * 16), &mut bytes).then(|| [field_at(&bytes, 0), field_at(&bytes, 8)])
    };
    let joined = (|| {
        if current.head != c.before.head {
            return false;
        }
        let (Some(data), Some(capacity), Some(count), Some(index)) = (
            layout::read_u64(current.ring.wrapping_add(0x1C)),
            layout::read_u32(current.ring.wrapping_add(0x24)),
            layout::read_u32(current.ring.wrapping_add(0x28)),
            layout::read_u32(current.ring.wrapping_add(0x30)),
        ) else {
            return false;
        };
        if data == 0 || capacity == 0 || capacity > 4096 || count == 0 || count > capacity || index >= capacity || data > u64::MAX - u64::from(index) * 16 - 16 {
            return false;
        }
        let (Some(first), Some(again)) = (entry(data, index), entry(data, index)) else { return false };
        first == again
            && first[0] == c.publication_stamp
            && first[1] == c.before.head
            && healthy()
            && s.ledger.publish(&current, &Publication { scope: c.scope, head: first[1] }) == Ring::Published
    })();
    if !joined {
        finish(call);
        set_last(Status::Changed);
        DIAGNOSTICS[22].fetch_add(1, Relaxed);
        if !healthy() {
            apply_loss(s);
        }
        return;
    }
    finish(call);
    set_last(Status::Observed);
}
fn field_at(bytes: &[u8; 16], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("8 bytes"))
}

// ---- receipts

/// The owner a source candidate carries: the epoch, the pinned backing, the read request and its CPU bytes, the
/// recording, the draw and the graph parent; at most one GPU read is recorded through it.
#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    epoch: u64,
    backing: Arc<Backing>,
    request: ReadRequest,
    cpu: CpuCandidate,
    recording: Recording,
    draw: DrawRecord,
    parent: GraphParentRecord,
    probe_recorded: AtomicBool,
}
const RECEIPT_TYPE: GUID = GUID::from_values(0xa29299ae, 0xc2e4, 0x4e54, [0x87, 0xa5, 0x3c, 0xa0, 0x72, 0xf6, 0x04, 0xb1]);
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: receipt_query, AddRef: receipt_add_ref, Release: receipt_release };
unsafe extern "system" fn receipt_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers.
    unsafe {
        *out = null_mut();
        if *iid != IUnknown::IID && *iid != RECEIPT_TYPE {
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
        // SAFETY: made by Box::into_raw in query_at_source.
        drop(unsafe { Box::from_raw(this.cast::<Receipt>()) });
    }
    left
}
/// A held receipt reference.
struct ReceiptRef(*mut c_void);
impl ReceiptRef {
    fn get(&self) -> &Receipt {
        // SAFETY: a live receipt while this reference is held.
        unsafe { &*self.0.cast::<Receipt>() }
    }
}
impl Drop for ReceiptRef {
    fn drop(&mut self) {
        // SAFETY: the reference this value holds.
        unsafe { receipt_release(self.0) };
    }
}
/// Owner: the receipt behind an IUnknown (QueryInterface for the receipt type), held.
fn owner_of(value: *mut c_void) -> Option<ReceiptRef> {
    // SAFETY: the candidate's live owner or null.
    let value = unsafe { IUnknown::from_raw_borrowed(&value) }?;
    let mut out = null_mut();
    // SAFETY: a QueryInterface for the receipt type.
    let hr = unsafe { (Interface::vtable(value).QueryInterface)(value.as_raw(), &RECEIPT_TYPE, &mut out) };
    (hr.is_ok() && !out.is_null()).then(|| ReceiptRef(out))
}

// ---- the source queries

/// The C++ SourceCandidate as the forwards see it: the CPU candidate's producer, request and bytes, and its owner.
#[repr(C)]
pub struct CandidateView {
    pub producer: CompletedWriter,
    pub source: ReadRequest,
    pub bytes: *const u8,
    pub size: usize,
    pub owner: *mut c_void,
    pub cpu: bool,
}
impl CandidateView {
    fn bytes(&self) -> &[u8] {
        if self.bytes.is_null() || self.size == 0 {
            return &[];
        }
        // SAFETY: the candidate's payload, alive while the caller holds the candidate.
        unsafe { core::slice::from_raw_parts(self.bytes, self.size) }
    }
}
/// What QueryAtSource hands the C++ SourceCandidate: the payload (one reference), its producer, request,
/// publication and subrange, and the receipt (one reference).
#[repr(C)]
pub struct SourceOut {
    pub payload: *const Payload,
    pub producer: CompletedWriter,
    pub source: ReadRequest,
    pub publication: Publication,
    pub offset: usize,
    pub bytes: usize,
    pub owner: *mut c_void,
}
/// instance_snapshot_view::Input as RecordGpuCapture checks it.
#[repr(C)]
pub struct InputView {
    pub root: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub eye: u32,
    pub bytes: bool,
    pub pipeline: *mut c_void,
    pub shader: *const c_void,
    pub original: D3D12_VERTEX_BUFFER_VIEW,
    pub draw: D3D12_DRAW_INDEXED_ARGUMENTS,
}
const _: () = assert!(
    core::mem::size_of::<CandidateView>() == 296
        && core::mem::size_of::<SourceOut>() == 360
        && core::mem::size_of::<InputView>() == 96
        && core::mem::size_of::<DrawRecord>() == 1096
        && core::mem::size_of::<BufferLease>() == 80
);

/// The list's current IA leases (CopyCurrentBindings), released with this value.
struct CurrentLeases {
    vertices: [Option<BufferLease>; 16],
    index: Option<BufferLease>,
}
impl CurrentLeases {
    fn capture(list: u64, draw: &DrawRecord, indexed: bool) -> Option<Self> {
        let (index, vertices) = crate::geometry_leases::current(list, draw, indexed, cvr_host::probe::internal())?;
        Some(Self { vertices, index })
    }
    fn vertex(&self, slot: usize) -> BufferLease {
        self.vertices[slot].unwrap_or(BufferLease { resource: null_mut(), gpu: 0, bytes: 0, heap_type: 0, flags: 0, native: NativeBuffer::default() })
    }
}
impl Drop for CurrentLeases {
    fn drop(&mut self) {
        for lease in self.vertices.iter().chain(core::iter::once(&self.index)).flatten() {
            if !lease.resource.is_null() {
                // SAFETY: the reference the lease owns.
                drop(unsafe { IUnknown::from_raw(lease.resource) });
            }
        }
    }
}
fn same_view(a: &D3D12_VERTEX_BUFFER_VIEW, b: &D3D12_VERTEX_BUFFER_VIEW) -> bool {
    a.BufferLocation == b.BufferLocation && a.SizeInBytes == b.SizeInBytes && a.StrideInBytes == b.StrideInBytes
}
/// CaptureDrawState: the list's mirrored draw state is known (the pipeline is the caller's).
fn draw_state_known(list: u64, pipeline: u64) -> bool {
    if list == 0 || pipeline == 0 {
        return false;
    }
    // SAFETY: a plain-data output.
    let mut raw: cvr_gpu::scene_state::DrawStateRaw = unsafe { core::mem::zeroed() };
    // SAFETY: a writable output.
    if !unsafe { cvr_gpu::scene_state::cvr_scene_capture_draw_state(list as usize, &mut raw) } {
        return false;
    }
    if raw.rate_image != 0 {
        // SAFETY: the reference the capture handed over.
        drop(unsafe { IUnknown::from_raw(raw.rate_image as *mut c_void) });
    }
    true
}

/// A source candidate kept on the Rust side (the C++ SourceCandidate): the CPU bytes and the receipt pinning them.
pub struct Candidate {
    pub cpu: CpuCandidate,
    pub owner: IUnknown,
}
fn failed<T>(status: Status) -> Option<T> {
    fail(status);
    None
}
/// QueryAtSource.
#[allow(clippy::too_many_arguments)]
fn query_at_source(list: u64, draw: &DrawRecord, held: &BufferLease, original: u64, indexed: bool, recording: &RawRecording, parent: &GraphParentRecord, occurrence: u64) -> Option<Candidate> {
    let _ = QUERY.try_with(|q| q.set([0; 32]));
    put(field::SOURCE_LIST, list);
    put(field::SOURCE_GENERATION, recording.generation);
    put(field::SOURCE_OCCURRENCE, occurrence);
    put(field::SOURCE_EYE, u64::from(draw.side));
    if !enabled() {
        return failed(Status::Disabled);
    }
    if !compatible() {
        return failed(Status::Code);
    }
    let _gate = Held::new();
    let s = state();
    if !reset_at_idle(s) {
        return failed(Status::Pending);
    }
    put(field::OBSERVER_EPOCH, s.epoch);
    if !healthy() {
        apply_loss(s);
        return failed(Status::Changed);
    }
    if list == 0 || occurrence == 0 || draw.list != list || draw.flags & 128 == 0 || draw.flags & 64 != 0 || draw.input_slot_mask & 128 == 0 || draw.instances == 0 {
        return failed(Status::Range);
    }
    if !revalidate(parent) {
        return failed(Status::Parent);
    }
    put(field::FLAGS, CURRENT_PARENT);
    let context = current();
    if !context.valid() || context.list != draw.list || recording.list != draw.list || recording.context != context.context || !validate_recording(recording, list, &context) {
        return failed(Status::Scope);
    }
    put(field::FLAGS, query_value(field::FLAGS) | CURRENT_RECORDING);
    let Some(actual) = CurrentLeases::capture(list, draw, indexed) else { return failed(Status::Scope) };
    let current_lease = actual.vertex(7);
    let view = draw.vertices[7];
    if held.resource.is_null()
        || current_lease.resource != held.resource
        || held.gpu != current_lease.gpu
        || held.bytes != current_lease.bytes
        || !same_view(&view, &draw.vertices[7])
        || held.native.handle != current_lease.native.handle
        || held.native.registry != recording.registry
        || held.native.kind != 5
        || held.heap_type != D3D12_HEAP_TYPE_UPLOAD.0 as u32
    {
        return failed(Status::Backing);
    }
    let actual_draw = crate::stereo_probe_state::current_state(list as *mut c_void);
    let Some(actual_draw) = actual_draw.filter(|a| a.pipeline == draw.pipeline && a.root == draw.root) else { return failed(Status::Layout) };
    if !draw_state_known(list, actual_draw.pipeline) || actual_draw.pipeline != original || actual_draw.pipeline != draw.pipeline {
        return failed(Status::Layout);
    }
    // SAFETY: the host's own function.
    if !host().is_some_and(|h| unsafe { (h.layout)(actual_draw.pipeline as *mut c_void, view.StrideInBytes) }) {
        return failed(Status::Layout);
    }
    put(field::FLAGS, query_value(field::FLAGS) | CURRENT_IA);
    let stride = u64::from(view.StrideInBytes);
    let relative = u64::from(draw.first_instance) * stride;
    let bytes = u64::from(draw.instances) * stride;
    if view.BufferLocation < held.gpu
        || view.BufferLocation - held.gpu > held.bytes
        || u64::from(view.SizeInBytes) > held.bytes - (view.BufferLocation - held.gpu)
        || relative > u64::from(view.SizeInBytes)
        || bytes > u64::from(view.SizeInBytes) - relative
    {
        return failed(Status::Range);
    }
    let offset = view.BufferLocation - held.gpu + relative;
    put(field::RESOURCE, held.resource as u64);
    put(field::HANDLE, u64::from(held.native.handle));
    put(field::KIND, u64::from(held.native.kind));
    put(field::STRIDE, stride);
    put(field::WIDTH, held.bytes);
    put(field::VIEW_OFFSET, view.BufferLocation - held.gpu);
    put(field::FIRST_INSTANCE, u64::from(draw.first_instance));
    put(field::INSTANCES, u64::from(draw.instances));
    let Some(stamp) = layout::stamp(recording.registry, recording.context) else { return failed(Status::Scope) };
    let scope = Scope { list: draw.list, generation: recording.generation, occurrence, thread: tid(), context: recording.context, registry: recording.registry, stamp };
    for writer in s.writers.iter().rev() {
        let backing = &*writer.backing;
        if backing.source.resource != held.resource
            || backing.observed.handle != held.native.handle
            || backing.observed.registry != held.native.registry
            || backing.observed.stride != view.StrideInBytes
            || backing.observed.width != held.bytes
        {
            continue;
        }
        let writer_offset = u64::from(writer.completed.first_instance) * stride;
        let writer_bytes = u64::from(writer.completed.instances) * stride;
        if offset < writer_offset || offset - writer_offset > writer_bytes || bytes > writer_bytes - (offset - writer_offset) {
            continue;
        }
        let Some(observed) = physical(backing) else { return failed(Status::Changed) };
        let absolute = writer.completed.absolute_begin + (offset - writer_offset) / stride;
        if offset % stride != 0 {
            return failed(Status::Range);
        }
        let request = ReadRequest { writer: writer.completed.writer, source: scope, absolute_begin: absolute, first_instance: (offset / stride) as u32, instances: draw.instances };
        let (status, cpu) = s.ledger.query(&observed, &request);
        if status != Ring::Completed && status != Ring::Published {
            continue;
        }
        if !healthy() || cpu.bytes().len() as u64 != bytes {
            return failed(Status::Changed);
        }
        put(field::FLAGS, query_value(field::FLAGS) | OWNED_CPU | BACKING_PINNED | if status == Ring::Published { PUBLICATION_OBSERVED } else { 0 });
        put(field::RING, observed.ring);
        put(field::RING_GENERATION, observed.generation);
        put(field::ABSOLUTE_BEGIN, absolute);
        put(field::RESERVATION_SEQUENCE, writer.reservation.sequence);
        put(field::RESERVATION_BEGIN, writer.reservation.absolute_begin);
        put(field::RESERVATION_END, writer.reservation.absolute_end);
        put(field::RESERVATION_HEAD_BEFORE, writer.reservation.head_before);
        put(field::RETIRED_TAIL, observed.tail);
        put(field::PUBLISHED_STAMP, cpu.published().scope.stamp);
        put(field::PUBLISHED_HEAD, cpu.published().head);
        put(field::CPU_BYTES, bytes);
        let producer = writer.completed.writer.scope;
        put(field::PRODUCER_LIST, producer.list);
        put(field::PRODUCER_GENERATION, producer.generation);
        put(field::PRODUCER_OCCURRENCE, producer.occurrence);
        put(field::PRODUCER_THREAD, producer.thread);
        put(field::PRODUCER_STAMP, producer.stamp);
        put(field::LIVE_CPU_BYTES, s.ledger.live_cpu_bytes() as u64);
        set_last(Status::Observed);
        put(field::STATUS, Status::Observed as u64);
        let receipt = Box::new(Receipt {
            vtbl: &RECEIPT_VTBL,
            references: AtomicU32::new(1),
            epoch: s.epoch,
            backing: writer.backing.clone(),
            request,
            cpu: cpu.clone(),
            recording: Recording::copied(recording),
            draw: *draw,
            parent: *parent,
            probe_recorded: AtomicBool::new(false),
        });
        // SAFETY: the new receipt's one reference, handed over.
        let owner = unsafe { IUnknown::from_raw(Box::into_raw(receipt).cast()) };
        return Some(Candidate { cpu, owner });
    }
    failed(Status::NoProducer)
}
/// ValidateSource.
fn validate_source(candidate: &CandidateView, queue: *mut c_void) -> bool {
    if !enabled() || !compatible() || !healthy() || !candidate.cpu || queue.is_null() {
        return false;
    }
    let _gate = Held::new();
    let s = state();
    let Some(owner) = owner_of(candidate.owner) else { return false };
    let o = owner.get();
    if o.epoch != s.epoch
        || candidate.producer.writer != o.cpu.producer().writer
        || candidate.source != o.request
        || candidate.bytes().len() != o.cpu.bytes().len()
        // SAFETY: the receipt's lease and the hook's queue.
        || !unsafe { crate::native_recording::cvr_native_recording_validate_submission(&o.recording.raw(), queue) }
    {
        return false;
    }
    let Some(observed) = physical(&o.backing) else { return false };
    let (status, current) = s.ledger.query(&observed, &o.request);
    healthy() && (status == Ring::Completed || status == Ring::Published) && current.bytes() == o.cpu.bytes()
}
/// RecordGpuCopy: the original range copied, predicated, into the readback (and the private copy).
#[allow(clippy::too_many_arguments)]
fn record_gpu_copy(
    list: *mut c_void,
    candidate: &CandidateView,
    readback: *mut c_void,
    offset: u64,
    predicate: *mut c_void,
    predicate_offset: u64,
    private_copy: *mut c_void,
    private_offset: u64,
    input: Option<&InputView>,
) -> bool {
    PROBE_PHASE.store(1, SeqCst);
    if !enabled() || !compatible() || !healthy() || list.is_null() || readback.is_null() || predicate.is_null() || predicate_offset % 8 != 0 {
        return false;
    }
    let mut gate = Held::new();
    let owner = owner_of(candidate.owner);
    PROBE_PHASE.store(2, SeqCst);
    // SAFETY: the caller's live list and resources.
    let (Some(commands), Some(readback), Some(predicate)) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe { ID3D12Resource::from_raw_borrowed(&readback) }, unsafe { ID3D12Resource::from_raw_borrowed(&predicate) }) else {
        return false;
    };
    // SAFETY: the caller's live resource or null.
    let private_copy = unsafe { ID3D12Resource::from_raw_borrowed(&private_copy) };
    let s = state();
    let Some(owner) = owner else { return false };
    let o = owner.get();
    if o.probe_recorded.load(SeqCst)
        || o.epoch != s.epoch
        || !candidate.cpu
        || candidate.producer.writer != o.cpu.producer().writer
        || candidate.source != o.request
        // SAFETY: a live list.
        || unsafe { commands.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || o.draw.flags & 64 != 0
        || !revalidate(&o.parent)
    {
        return false;
    }
    if private_copy.is_some() != input.is_some() {
        return false;
    }
    if let Some(input) = input {
        let draw = &o.draw;
        let args = &input.draw;
        if input.bytes
            || input.root != draw.root
            || input.list != o.recording.list
            || input.generation != o.recording.generation
            || input.occurrence != o.request.source.occurrence
            || input.eye != draw.side
            || input.pipeline as u64 != draw.pipeline
            // SAFETY: the host's own function.
            || !host().is_some_and(|h| unsafe { (h.shader)(input.pipeline, input.shader) })
            || !same_view(&input.original, &draw.vertices[7])
            || args.IndexCountPerInstance != draw.indices
            || args.InstanceCount != draw.instances
            || args.StartIndexLocation != draw.first_index
            || args.BaseVertexLocation != draw.base_vertex
            || args.StartInstanceLocation != draw.first_instance
        {
            return false;
        }
    }
    let context = current();
    if !context.valid() || context.list != o.recording.list || !validate_recording(&o.recording.raw(), list as u64, &context) {
        return false;
    }
    PROBE_PHASE.store(3, SeqCst);
    let actual = crate::stereo_probe_state::current_state(list);
    let Some(_) = actual.filter(|a| a.pipeline == o.draw.pipeline && a.root == o.draw.root && a.flags & 64 == 0) else { return false };
    let Some(bindings) = CurrentLeases::capture(list as u64, &o.draw, true) else { return false };
    let held = &o.backing.source;
    let view = o.draw.vertices[7];
    let bound = bindings.vertex(7);
    if bound.resource != held.resource || bound.native.handle != held.native.handle || !same_view(&o.draw.vertices[7], &view) {
        return false;
    }
    drop(bindings);
    let Some(observed) = physical(&o.backing) else { return false };
    PROBE_PHASE.store(4, SeqCst);
    let (status, current) = s.ledger.query(&observed, &o.request);
    if (status != Ring::Completed && status != Ring::Published) || current.bytes() != candidate.bytes() {
        return false;
    }
    let bytes = current.bytes().len() as u64;
    let source_offset = u64::from(o.request.first_instance) * u64::from(observed.stride);
    // SAFETY: live resources.
    let (destination, permit) = unsafe { (readback.GetDesc(), predicate.GetDesc()) };
    let heap_type = |resource: &ID3D12Resource| {
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        // SAFETY: a live resource.
        unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.ok().map(|()| heap.Type)
    };
    if bytes == 0
        || source_offset > held.bytes
        || bytes > held.bytes - source_offset
        || destination.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || offset > destination.Width
        || bytes > destination.Width - offset
        || destination.Flags.0 != 0
        || permit.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || predicate_offset > permit.Width
        || 8 > permit.Width - predicate_offset
        || permit.Flags.0 != 0
        || heap_type(readback) != Some(D3D12_HEAP_TYPE_READBACK)
        || heap_type(predicate) != Some(D3D12_HEAP_TYPE_UPLOAD)
    {
        return false;
    }
    if let Some(target) = private_copy {
        // SAFETY: a live resource.
        let desc = unsafe { target.GetDesc() };
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags.0 != 0 || private_offset > desc.Width || bytes > desc.Width - private_offset || heap_type(target) != Some(D3D12_HEAP_TYPE_DEFAULT) {
            return false;
        }
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list.
    if unsafe { commands.GetDevice(&mut device) }.is_err() {
        return false;
    }
    PROBE_PHASE.store(5, SeqCst);
    let same_device = |resource: &ID3D12Resource| {
        let mut other: Option<ID3D12Device> = None;
        // SAFETY: a live resource.
        unsafe { resource.GetDevice(&mut other) }.is_ok() && other == device
    };
    if !same_device(&o.backing.resource) || !same_device(readback) || !same_device(predicate) || private_copy.is_some_and(|p| !same_device(p)) {
        return false;
    }
    o.probe_recorded.store(true, SeqCst);
    gate.unlock();
    PROBE_PHASE.store(6, SeqCst);
    let keep = [owner.0, o.backing.resource.as_raw(), readback.as_raw(), predicate.as_raw()];
    // SAFETY: the list and four live objects.
    if !unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
        return false;
    }
    if let Some(target) = private_copy {
        let keep = [target.as_raw()];
        // SAFETY: the list and a live object.
        if !unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
            return false;
        }
    }
    PROBE_PHASE.store(7, SeqCst);
    {
        let previous = cvr_host::probe::set_internal(true);
        let record = |destination: &ID3D12Resource, destination_offset: u64| {
            // SAFETY: the validated list, resources and ranges (instance_gpu_probe::Record).
            unsafe {
                commands.SetPredication(predicate, predicate_offset, D3D12_PREDICATION_OP_EQUAL_ZERO);
                commands.CopyBufferRegion(destination, destination_offset, &o.backing.resource, source_offset, bytes);
                commands.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            }
        };
        if let Some(target) = private_copy {
            record(target, private_offset);
        }
        record(readback, offset);
        cvr_host::probe::set_internal(previous);
    }
    PROBE_PHASE.store(8, SeqCst);
    true
}
fn clear() {
    let Some(_gate) = Held::try_new() else {
        lose(Status::Contention);
        return;
    };
    let s = state();
    if ENABLE.load(Acquire) != 0 || s.active != 0 || s.epoch == u64::MAX {
        lose(Status::Pending);
        return;
    }
    clear_state(s);
    let _ = QUERY.try_with(|q| q.set([0; 32]));
    set_last(Status::Disabled);
}

// ---- the retained geometry's own calls (its instance capture holds the candidate on the Rust side)

/// QueryAtSource (the C++ catch(...): the exception lost and failed).
#[allow(clippy::too_many_arguments)]
pub(crate) fn query(list: *mut c_void, draw: &DrawRecord, held: &BufferLease, original: *mut c_void, indexed: bool, recording: &RawRecording, parent: &GraphParentRecord, occurrence: u64) -> Option<Candidate> {
    catch_unwind(AssertUnwindSafe(|| query_at_source(list as u64, draw, held, original as u64, indexed, recording, parent, occurrence))).unwrap_or_else(|_| {
        lose(Status::Exception);
        fail(Status::Exception);
        None
    })
}
/// LastQuery.
pub(crate) fn last_query() -> [u64; 32] {
    QUERY.try_with(Cell::get).unwrap_or([0; 32])
}
fn candidate_view(candidate: &Candidate) -> CandidateView {
    let bytes = candidate.cpu.bytes();
    CandidateView { producer: candidate.cpu.producer(), source: *candidate.cpu.source(), bytes: bytes.as_ptr(), size: bytes.len(), owner: candidate.owner.as_raw(), cpu: candidate.cpu.is_some() }
}
/// ValidateSource.
pub(crate) fn validate(candidate: &Candidate, queue: *mut c_void) -> bool {
    let view = candidate_view(candidate);
    guarded(false, || validate_source(&view, queue))
}
/// RecordGpuCapture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_capture(list: *mut c_void, candidate: &Candidate, input: &InputView, destination: *mut c_void, destination_offset: u64, readback: *mut c_void, readback_offset: u64, predicate: *mut c_void, predicate_offset: u64) -> bool {
    if destination.is_null() {
        return false;
    }
    let view = candidate_view(candidate);
    catch_unwind(AssertUnwindSafe(|| record_gpu_copy(list, &view, readback, readback_offset, predicate, predicate_offset, destination, destination_offset, Some(input)))).unwrap_or(false)
}

// ---- the C ABI (the C++ face's CVR_RUST_VRIK branch)

/// Runs `f`, turning a panic into the C++ catch(...)'s `Lose(Status::Exception)`.
fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        lose(Status::Exception);
        fallback
    })
}
#[no_mangle]
pub extern "C" fn cvr_nio2_prepare_code_guards(game_base: u64) {
    prepare_code_guards(game_base);
}
#[no_mangle]
pub extern "C" fn cvr_nio2_compatible() -> bool {
    compatible()
}
#[no_mangle]
pub extern "C" fn cvr_nio2_set_installed(mask: u32) {
    set_installed(mask);
}
#[no_mangle]
pub extern "C" fn cvr_nio2_dormant() -> bool {
    dormant()
}
/// ObserveReservationWitness, called by the witness thunk.
///
/// # Safety
/// `frame` is the thunk's frame or null.
#[no_mangle]
pub unsafe extern "C" fn CvrNativeInstanceReservationObserve(frame: *const ThunkFrame) {
    // SAFETY: as documented.
    if let Some(frame) = unsafe { frame.as_ref() } {
        guarded((), || observe_reservation(frame));
    }
}
/// Enter: the call (one Arc reference the C++ Call owns) or null.
#[no_mangle]
pub extern "C" fn cvr_nio2_enter(ring: *mut c_void, instances: u32, cursor: *mut u32, caller: u64) -> *const CallData {
    guarded(None, || enter(ring as u64, instances, cursor as u64, caller)).map_or(null(), Arc::into_raw)
}
/// Publication: the call (one Arc reference the C++ Call owns) or null.
#[no_mangle]
pub extern "C" fn cvr_nio2_publication(ring: *mut c_void, caller: u64) -> *const CallData {
    guarded(None, || publication(ring as u64, caller)).map_or(null(), Arc::into_raw)
}
/// Call::operator bool.
///
/// # Safety
/// `call` came from cvr_nio2_enter or cvr_nio2_publication and is still held.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_call_live(call: *const CallData) -> bool {
    // SAFETY: as documented.
    unsafe { call.as_ref() }.is_some_and(|c| !c.get().completed && healthy())
}
/// The C++ Call's reference released.
///
/// # Safety
/// As cvr_nio2_call_live; the reference is not used again.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_call_drop(call: *const CallData) {
    if !call.is_null() {
        // SAFETY: as documented.
        guarded((), || drop(unsafe { Arc::from_raw(call) }));
    }
}
/// The catch(...) of both Returned.
fn returned_failed(call: &CallData) {
    let _gate = Held::new();
    finish(call);
    apply_loss(state());
}
/// Returned(Call&, first).
///
/// # Safety
/// As cvr_nio2_call_live.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_returned(call: *const CallData, first: u32) {
    // SAFETY: as documented.
    if let Some(call) = unsafe { call.as_ref() } {
        if catch_unwind(AssertUnwindSafe(|| returned(call, first))).is_err() {
            lose(Status::Exception);
            returned_failed(call);
        }
    }
}
/// Returned(Call&).
///
/// # Safety
/// As cvr_nio2_call_live.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_published(call: *const CallData) {
    // SAFETY: as documented.
    if let Some(call) = unsafe { call.as_ref() } {
        if catch_unwind(AssertUnwindSafe(|| published(call))).is_err() {
            lose(Status::Exception);
            returned_failed(call);
        }
    }
}
/// QueryAtSource: true with `out` filled (two references handed over), or false.
///
/// # Safety
/// The C++ face's live arguments (the source's draw, its vertex slot 7 lease, the recording and the parent).
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_query(
    list: *mut c_void,
    draw: *const DrawRecord,
    held: *const BufferLease,
    original: *mut c_void,
    indexed: bool,
    recording: *const RawRecording,
    parent: *const GraphParentRecord,
    occurrence: u64,
    out: *mut SourceOut,
) -> bool {
    // SAFETY: as documented.
    let (draw, held, recording, parent, out) = unsafe { (&*draw, &*held, &*recording, &*parent, &mut *out) };
    match query(list, draw, held, original, indexed, recording, parent, occurrence) {
        Some(candidate) => {
            let publication = *candidate.cpu.published();
            let producer = candidate.cpu.producer();
            let source = *candidate.cpu.source();
            let (payload, at, length) = candidate.cpu.into_payload();
            *out = SourceOut {
                payload: payload.map_or(null(), Arc::into_raw),
                producer,
                source,
                publication,
                offset: at,
                bytes: length,
                owner: candidate.owner.into_raw(),
            };
            true
        }
        None => false,
    }
}
/// ValidateSource.
///
/// # Safety
/// `candidate` describes the caller's live SourceCandidate.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_validate(candidate: *const CandidateView, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    let candidate = unsafe { &*candidate };
    guarded(false, || validate_source(candidate, queue))
}
/// RecordGpuProbe (no private copy, no input) and RecordGpuCapture.
///
/// # Safety
/// The caller's live list, candidate, resources and optional input.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_record(
    list: *mut c_void,
    candidate: *const CandidateView,
    readback: *mut c_void,
    offset: u64,
    predicate: *mut c_void,
    predicate_offset: u64,
    private_copy: *mut c_void,
    private_offset: u64,
    input: *const InputView,
) -> bool {
    // SAFETY: as documented.
    let (candidate, input) = unsafe { (&*candidate, input.as_ref()) };
    catch_unwind(AssertUnwindSafe(|| record_gpu_copy(list, candidate, readback, offset, predicate, predicate_offset, private_copy, private_offset, input))).unwrap_or(false)
}
/// LastQuery.
///
/// # Safety
/// `out` holds 32 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_nio2_last_query(out: *mut [u64; 32]) {
    // SAFETY: as documented.
    unsafe { *out = QUERY.try_with(Cell::get).unwrap_or([0; 32]) };
}
#[no_mangle]
pub extern "C" fn cvr_nio2_last_status() -> u64 {
    LAST.try_with(Cell::get).unwrap_or(Status::Disabled) as u64
}
#[no_mangle]
pub extern "C" fn cvr_nio2_clear() {
    guarded((), clear);
}
