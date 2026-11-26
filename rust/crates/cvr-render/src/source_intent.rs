//! The source resource intent cache (src/Render/SourceResourceIntent.cpp, `source_resource_intent::`).
//!
//! Each default-heap destination buffer that received an observed copy carries a private owner (an IUnknown attached
//! as resource private data, retired on its last release and deleted under the cache lock): its current generic copy
//! ranges (each an immutable payload: the copied bytes and their labels, counted against the owner's and the global
//! budgets while anyone holds it), its structured ranges (pieces of payloads inside a marked structured-128 view) and
//! its diagnostics (operation labels, at most 64 cache events). An observed copy invalidates the overlapping current
//! range first, then samples the upload source twice and publishes the stable bytes; budget pressure evicts the oldest
//! payloads (ordinary copies of owners without structured coverage first; small copies have their own record budget).
//! A draw captures the one covering copy, or the structured pieces covering a marked view (composed into one payload
//! when several). Copy and mark diagnostics are checksummed private data receipts.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's functions forward to `cvr_sri_*`; a Candidate's payload is a Rust
//! payload handle. `tools/single_pass_tests/source_resource_intent_test` and `structured_intent_cache_test` run their
//! cases against it in their Rust variants.

use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering::*};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, HMODULE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN};
use windows::Win32::System::Threading::GetCurrentProcess;

pub const MAX_COPY_BYTES: usize = 65536;
pub const MAX_RESOURCE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_GLOBAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_RANGES_PER_RESOURCE: usize = 512;
pub const MAX_OWNERS: usize = 4096;
pub const MAX_PAYLOADS: usize = 65536;
pub const MAX_SMALL_COPY_BYTES: usize = 512;
pub const MAX_SMALL_RANGES_PER_RESOURCE: usize = 8192;
pub const MAX_GENERIC_RANGES_PER_RESOURCE: usize = MAX_RANGES_PER_RESOURCE + MAX_SMALL_RANGES_PER_RESOURCE;
pub const MAX_STRUCTURED_RANGES_PER_RESOURCE: usize = 65536;
pub const MAX_CACHE_EVENTS_PER_OWNER: usize = 64;

const OWNER_KEY: GUID = GUID::from_values(0x680b32c4, 0x81fa, 0x4c6d, [0xaf, 0x61, 0x22, 0x5a, 0xc3, 0x78, 0x14, 0x81]);
const OWNER_TYPE: GUID = GUID::from_values(0x680b32c4, 0x81fa, 0x4c6d, [0xaf, 0x61, 0x22, 0x5a, 0xc3, 0x78, 0x14, 0x82]);
const STRUCTURED_KEY: GUID = GUID::from_values(0x680b32c4, 0x81fa, 0x4c6d, [0xaf, 0x61, 0x22, 0x5a, 0xc3, 0x78, 0x14, 0x83]);
const COPY_DIAGNOSTIC_KEY: GUID = GUID::from_values(0x680b32c4, 0x81fa, 0x4c6d, [0xaf, 0x61, 0x22, 0x5a, 0xc3, 0x78, 0x14, 0x84]);
const MARK_DIAGNOSTIC_KEY: GUID = GUID::from_values(0x680b32c4, 0x81fa, 0x4c6d, [0xaf, 0x61, 0x22, 0x5a, 0xc3, 0x78, 0x14, 0x85]);
const COPY_DIAGNOSTIC_VERSION: u64 = 0x4350494e54303031;
const MARK_DIAGNOSTIC_VERSION: u64 = 0x53564d41524b3031;
const STRUCTURED_COVERAGE_VERSION: u64 = 0x5356434f56303031;
const GENERIC_COVERAGE_VERSION: u64 = 0x47434f5630303031;
const CACHE_EVENTS_VERSION: u64 = 0x4345565430303031;
const STRUCTURED_SIGNATURE: u64 = 0x5352563132383031;
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

/// CacheEventReason.
mod reason {
    pub const PUBLISHED_COPY: u64 = 1;
    pub const OBSERVE_COPY_INVALIDATION: u64 = 2;
    pub const EXPLICIT_RANGE_INVALIDATION: u64 = 3;
    pub const EXPLICIT_FULL_INVALIDATION: u64 = 4;
    pub const DIRECT_COPY_INVALIDATION: u64 = 5;
    pub const OVERWRITTEN_PAYLOAD: u64 = 6;
    pub const CLEAR: u64 = 7;
    pub const OWNER_CAPACITY_EVICTION: u64 = 8;
    pub const GLOBAL_BYTES_EVICTION: u64 = 9;
    pub const GLOBAL_PAYLOAD_EVICTION: u64 = 10;
    pub const RESOURCE_BYTES_EVICTION: u64 = 11;
    pub const GENERIC_RANGE_EVICTION: u64 = 12;
    pub const RESOURCE_DESTROYED: u64 = 13;
    pub const REJECTED_BASE: u64 = 32;
}
/// GenericOperation.
mod operation {
    pub const NONE: u64 = 0;
    pub const INVALIDATE: u64 = 1;
    pub const INVALIDATE_RANGE: u64 = 2;
    pub const OBSERVE_COPY: u64 = 3;
    pub const RECORD_INTENDED_COPY: u64 = 4;
    pub const CLEAR: u64 = 5;
    pub const OWNER_EVICTION: u64 = 6;
    pub const GENERIC_EVICTION: u64 = 7;
    pub const RESOURCE_DESTROYED: u64 = 8;
}
/// DiagnosticOutcome.
mod outcome {
    pub const SUCCESS: u64 = 1;
    pub const DISABLED: u64 = 2;
    pub const ARGUMENTS: u64 = 3;
    pub const UNSUPPORTED_DESTINATION: u64 = 4;
    pub const SOURCE_DEFAULT: u64 = 5;
    pub const UNSUPPORTED_HEAP: u64 = 6;
    pub const NO_HINT: u64 = 7;
    pub const MAP_FAILED: u64 = 8;
    pub const UNSTABLE: u64 = 9;
    pub const BUDGET: u64 = 10;
    pub const GENERATION_CHANGED: u64 = 11;
    pub const CHANGED: u64 = 12;
    pub const CAPACITY: u64 = 13;
    pub const API_FAILED: u64 = 14;
    pub const INVALID_VIEW: u64 = 15;
}
/// DiagnosticStage.
mod stage {
    pub const BEGIN: u64 = 0;
    pub const DESTINATION: u64 = 1;
    pub const HINT: u64 = 2;
    pub const SOURCE: u64 = 3;
    pub const FIRST_SAMPLE: u64 = 4;
    pub const SECOND_SAMPLE: u64 = 5;
    pub const COMPARE: u64 = 6;
    pub const PUBLICATION: u64 = 7;
    pub const COMPLETE: u64 = 8;
}

// ---- payloads

static PAYLOAD_BYTES: AtomicUsize = AtomicUsize::new(0);
static PAYLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// detail::Payload: immutable copied bytes and their labels, counted against its budgets while held.
#[derive(Default)]
pub struct Payload {
    pub bytes: Vec<u8>,
    budget: Option<Arc<AtomicUsize>>,
    pub producer_list: u64,
    pub sequence: u64,
    pub source_resource: u64,
    pub destination_offset: u64,
    pub source_offset: u64,
    pub copied_bytes: u64,
    pub gpu_base: u64,
    pub resource_width: u64,
}
impl Drop for Payload {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            PAYLOAD_BYTES.fetch_sub(self.bytes.len(), Relaxed);
            PAYLOAD_COUNT.fetch_sub(1, Relaxed);
            budget.fetch_sub(self.bytes.len(), Relaxed);
        }
    }
}
fn account(value: &mut Payload, budget: &Arc<AtomicUsize>) {
    value.budget = Some(budget.clone());
    PAYLOAD_BYTES.fetch_add(value.bytes.len(), Relaxed);
    PAYLOAD_COUNT.fetch_add(1, Relaxed);
    budget.fetch_add(value.bytes.len(), Relaxed);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StructuredHint {
    signature: u64,
    resource: u64,
    gpu: u64,
    width: u64,
    generation: u64,
    offset: u64,
    bytes: u64,
}
#[derive(Clone)]
struct StructuredRange {
    data: Arc<Payload>,
    offset: u64,
    data_offset: usize,
    bytes: usize,
}
/// CacheEventReceipt.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheEventReceipt {
    pub generation: u64,
    pub source_resource: u64,
    pub producer_list: u64,
    pub offset: u64,
    pub bytes: u64,
    pub copy_sequence: u64,
    pub reason: u64,
    pub event_sequence: u64,
}
struct EventHistory {
    events: [CacheEventReceipt; MAX_CACHE_EVENTS_PER_OWNER],
    next: usize,
    count: usize,
}

// ---- owners

#[derive(Default)]
struct OwnerData {
    budget: Arc<AtomicUsize>,
    ranges: Vec<Arc<Payload>>,
    small_ranges: usize,
    structured: Vec<StructuredRange>,
    structured_hint: StructuredHint,
    resource: u64,
    resource_width: u64,
    mutation: u64,
    touch: u64,
    structured_bytes: u64,
    registered: bool,
    full_invalidations: u64,
    range_invalidations: u64,
    generic_evictions: u64,
    last_invalidation_offset: u64,
    last_invalidation_bytes: u64,
    last_operation: u64,
    last_operation_sequence: u64,
    last_operation_source: u64,
    last_operation_producer_list: u64,
    last_operation_generation: u64,
    events: Option<Box<EventHistory>>,
    publications: Vec<*mut Publication>,
    dropped_events: u64,
}
/// The owner: an IUnknown attached to its resource as private data.
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    next: *mut Owner,
    data: OwnerData,
}
static RETIRED: AtomicPtr<Owner> = AtomicPtr::new(core::ptr::null_mut());
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live owner.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != OWNER_TYPE) {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Owner)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live owner.
    unsafe { (*(this as *mut Owner)).refs.fetch_add(1, SeqCst) + 1 }
}
/// D3D12 may hold its object lock here: the last release only retires, never takes the cache lock.
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    let owner = this as *mut Owner;
    // SAFETY: a live owner; at zero it is pushed on the retired stack.
    unsafe {
        let n = (*owner).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let mut next = RETIRED.load(Relaxed);
            loop {
                (*owner).next = next;
                match RETIRED.compare_exchange_weak(next, owner, Release, Relaxed) {
                    Ok(_) => break,
                    Err(actual) => next = actual,
                }
            }
        }
        n
    }
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };

/// A held owner reference (ComPtr<Owner>).
struct OwnerRef(*mut Owner);
impl OwnerRef {
    fn ptr(&self) -> *mut Owner {
        self.0
    }
}
impl Drop for OwnerRef {
    fn drop(&mut self) {
        // SAFETY: the reference this holds.
        unsafe { owner_release(self.0.cast()) };
    }
}
fn data<'a>(owner: *mut Owner) -> &'a mut OwnerData {
    // SAFETY: a live owner, reached under the cache lock.
    unsafe { &mut (*owner).data }
}

/// The observing call's pending publication, invalidated by any overlapping change before its record.
#[derive(Default)]
pub struct Publication {
    owner: *mut Owner,
    offset: u64,
    bytes: u64,
    valid: bool,
}
impl Publication {
    fn begin(&mut self, owner: *mut Owner, offset: u64, bytes: u64) -> bool {
        let o = data(owner);
        if o.publications.len() >= 64 {
            return false;
        }
        o.publications.push(self as *mut Publication);
        self.owner = owner;
        self.offset = offset;
        self.bytes = bytes;
        self.valid = true;
        true
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            let _g = lock();
            let me = self as *mut Publication;
            data(self.owner).publications.retain(|&p| p != me);
        }
    }
}

// ---- the global state

struct Global {
    owners: Vec<*mut Owner>,
    current_bytes: u64,
    ranges: u64,
    sequence: u64,
    touch: u64,
    evictions: u64,
    captures: u64,
    rejected: u64,
    diagnostic_sequence: u64,
    generation: u64,
    structured_bytes: u64,
    structured_ranges: u64,
    event_sequence: u64,
}
// SAFETY: the raw owners are reached only under the cache lock.
unsafe impl Send for Global {}
static STATE: Mutex<Global> = Mutex::new(Global {
    owners: Vec::new(),
    current_bytes: 0,
    ranges: 0,
    sequence: 0,
    touch: 0,
    evictions: 0,
    captures: 0,
    rejected: 0,
    diagnostic_sequence: 0,
    generation: 1,
    structured_bytes: 0,
    structured_ranges: 0,
    event_sequence: 0,
});
static CREATION: Mutex<()> = Mutex::new(());
fn lock() -> MutexGuard<'static, Global> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn increment(value: &mut u64) {
    if *value != u64::MAX {
        *value += 1;
    }
}
fn span(offset: u64, bytes: u64, width: u64) -> bool {
    bytes != 0 && offset <= width && bytes <= width - offset
}
fn remove(g: &mut Global, o: &mut OwnerData, index: usize) {
    let bytes = o.ranges[index].bytes.len();
    if bytes <= MAX_SMALL_COPY_BYTES {
        o.small_ranges -= 1;
    }
    g.current_bytes -= bytes as u64;
    g.ranges -= 1;
    o.ranges.remove(index);
}
fn remove_structured(g: &mut Global, o: &mut OwnerData, index: usize) {
    let bytes = o.structured[index].bytes as u64;
    g.structured_bytes -= bytes;
    g.structured_ranges -= 1;
    o.structured_bytes -= bytes;
    o.structured.remove(index);
}
fn empty_structured(g: &mut Global, o: &mut OwnerData) {
    g.structured_bytes -= o.structured_bytes;
    g.structured_ranges -= o.structured.len() as u64;
    o.structured_bytes = 0;
    o.structured = Vec::new();
    o.structured_hint = StructuredHint::default();
}
#[allow(clippy::too_many_arguments)]
fn event(g: &mut Global, o: &mut OwnerData, why: u64, offset: u64, bytes: u64, copy_sequence: u64, list: u64, source: u64) {
    if !o.registered || g.event_sequence == u64::MAX {
        increment(&mut o.dropped_events);
        return;
    }
    let history = o.events.get_or_insert_with(|| {
        Box::new(EventHistory { events: [CacheEventReceipt::default(); MAX_CACHE_EVENTS_PER_OWNER], next: 0, count: 0 })
    });
    if history.count == MAX_CACHE_EVENTS_PER_OWNER {
        increment(&mut o.dropped_events);
    } else {
        history.count += 1;
    }
    g.event_sequence += 1;
    history.events[history.next] = CacheEventReceipt {
        generation: g.generation,
        source_resource: source,
        producer_list: list,
        offset,
        bytes,
        copy_sequence,
        reason: why,
        event_sequence: g.event_sequence,
    };
    history.next = (history.next + 1) % MAX_CACHE_EVENTS_PER_OWNER;
}
fn release_events(o: &mut OwnerData) {
    if let Some(history) = o.events.take() {
        for _ in 0..history.count {
            increment(&mut o.dropped_events);
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn operation(g: &mut Global, o: &mut OwnerData, op: u64, offset: u64, bytes: u64, list: u64, source: u64, mut sequence: u64) {
    o.last_operation = op;
    o.last_invalidation_offset = offset;
    o.last_invalidation_bytes = bytes;
    if sequence == 0 && g.diagnostic_sequence != u64::MAX {
        g.diagnostic_sequence += 1;
        sequence = g.diagnostic_sequence;
    }
    o.last_operation_sequence = sequence;
    o.last_operation_source = source;
    o.last_operation_producer_list = list;
    o.last_operation_generation = g.generation;
}
fn empty(g: &mut Global, o: &mut OwnerData, op: u64, diagnose: bool) {
    for &pending in &o.publications {
        // SAFETY: a live publication of an observing call (it unregisters itself under the cache lock).
        unsafe { (*pending).valid = false };
    }
    increment(&mut o.full_invalidations);
    if diagnose {
        operation(g, o, op, 0, o.resource_width, 0, 0, 0);
    }
    let why = match op {
        operation::CLEAR => reason::CLEAR,
        operation::OWNER_EVICTION => reason::OWNER_CAPACITY_EVICTION,
        operation::RESOURCE_DESTROYED => reason::RESOURCE_DESTROYED,
        _ => reason::EXPLICIT_FULL_INVALIDATION,
    };
    if diagnose {
        let width = o.resource_width;
        event(g, o, why, 0, width, 0, 0, 0);
    }
    if op == operation::OWNER_EVICTION {
        let labels: Vec<_> = o.ranges.iter().map(|v| (v.destination_offset, v.bytes.len() as u64, v.sequence, v.producer_list, v.source_resource)).collect();
        for (offset, bytes, sequence, list, source) in labels {
            event(g, o, why, offset, bytes, sequence, list, source);
        }
        for _ in 0..o.ranges.len() {
            increment(&mut o.generic_evictions);
        }
    }
    while !o.ranges.is_empty() {
        let last = o.ranges.len() - 1;
        remove(g, o, last);
    }
    empty_structured(g, o);
    o.mutation += 1;
}
fn drain(g: &mut Global) {
    let mut p = RETIRED.swap(core::ptr::null_mut(), Acquire);
    while !p.is_null() {
        // SAFETY: a retired owner (refcount 0), deleted once here.
        let next = unsafe { (*p).next };
        let o = data(p);
        empty(g, o, operation::RESOURCE_DESTROYED, true);
        if o.registered {
            g.owners.retain(|&x| x != p);
            o.registered = false;
        }
        release_events(o);
        drop(unsafe { Box::from_raw(p) });
        p = next;
    }
}
fn register(g: &mut Global, owner: *mut Owner) {
    g.touch += 1;
    let o = data(owner);
    o.touch = g.touch;
    if o.registered {
        return;
    }
    if g.owners.len() == MAX_OWNERS {
        let oldest = *g.owners.iter().min_by_key(|&&p| data(p).touch).expect("owners");
        let old = data(oldest);
        empty(g, old, operation::OWNER_EVICTION, true);
        old.registered = false;
        release_events(old);
        g.owners.retain(|&x| x != oldest);
        g.evictions += 1;
    }
    g.owners.push(owner);
    data(owner).registered = true;
}

fn resource<'a>(raw: &'a *mut c_void) -> Option<&'a ID3D12Resource> {
    // SAFETY: a live resource of the caller's, or null.
    unsafe { ID3D12Resource::from_raw_borrowed(raw) }
}
fn existing(raw: *mut c_void) -> Option<OwnerRef> {
    let r = resource(&raw)?;
    let mut held: *mut c_void = core::ptr::null_mut();
    let mut size = 8u32;
    // SAFETY: the interface private data: an owned IUnknown pointer.
    let ok = unsafe { r.GetPrivateData(&OWNER_KEY, &mut size, Some(&mut held as *mut *mut c_void as *mut c_void)) }.is_ok();
    if !ok || size != 8 || held.is_null() {
        return None;
    }
    // SAFETY: the reference the retrieval added.
    let held = unsafe { IUnknown::from_raw(held) };
    let mut owner: *mut c_void = core::ptr::null_mut();
    // SAFETY: a live object; only our own owner answers its type.
    unsafe {
        let _ = held.query(&OWNER_TYPE, &mut owner);
    }
    (!owner.is_null()).then(|| OwnerRef(owner.cast()))
}
fn pinned() -> bool {
    static PINNED: OnceLock<bool> = OnceLock::new();
    *PINNED.get_or_init(|| {
        let mut module = HMODULE::default();
        // SAFETY: this module's own static; the module is pinned until process exit.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                PCWSTR(&STATE as *const Mutex<Global> as *const u16),
                &mut module,
            )
        }
        .is_ok()
    })
}
fn cache(raw: *mut c_void, create: bool) -> Option<OwnerRef> {
    let current = existing(raw);
    if current.is_some() || !create {
        return current;
    }
    let _serial = CREATION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(current) = existing(raw) {
        return Some(current);
    }
    if !pinned() {
        return None;
    }
    let r = resource(&raw)?;
    // SAFETY: a live resource.
    let width = unsafe { r.GetDesc() }.Width;
    let owner = Box::into_raw(Box::new(Owner {
        vtbl: &OWNER_VTBL,
        refs: AtomicU32::new(1),
        next: core::ptr::null_mut(),
        data: OwnerData { resource: raw as u64, resource_width: width, ..OwnerData::default() },
    }));
    let created = OwnerRef(owner);
    // SAFETY: the owner as an IUnknown, borrowed (the resource takes its own reference).
    let unknown = core::mem::ManuallyDrop::new(unsafe { IUnknown::from_raw(owner.cast()) });
    if unsafe { r.SetPrivateDataInterface(&OWNER_KEY, &*unknown) }.is_err() {
        return None;
    }
    Some(created)
}
fn destination(raw: *mut c_void, offset: u64, bytes: usize, limit: usize) -> Option<(u64, u64)> {
    let r = resource(&raw)?;
    if bytes == 0 || bytes > limit {
        return None;
    }
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY (whole block): a live resource.
    unsafe {
        let desc = r.GetDesc();
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || desc.Flags.0 != 0
            || !span(offset, bytes as u64, desc.Width)
            || r.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err()
            || heap.Type != D3D12_HEAP_TYPE_DEFAULT
        {
            return None;
        }
        let gpu = r.GetGPUVirtualAddress();
        (gpu != 0 && gpu <= u64::MAX - desc.Width).then_some((gpu, desc.Width))
    }
}
fn raw_hint(raw: *mut c_void) -> Option<StructuredHint> {
    let r = resource(&raw)?;
    let mut current = StructuredHint::default();
    let mut size = 56u32;
    // SAFETY: room for the hint.
    let ok = unsafe { r.GetPrivateData(&STRUCTURED_KEY, &mut size, Some(&mut current as *mut StructuredHint as *mut c_void)) }.is_ok();
    (ok && size == 56 && current.signature == STRUCTURED_SIGNATURE && current.resource == raw as u64).then_some(current)
}
fn hint(raw: *mut c_void, generation: u64, offset: u64, bytes: u64, gpu: u64, width: u64) -> Option<StructuredHint> {
    let current = raw_hint(raw)?;
    let valid = current.generation == generation
        && current.gpu == gpu
        && current.width == width
        && current.bytes != 0
        && current.bytes <= MAX_RESOURCE_BYTES as u64
        && span(current.offset, current.bytes, width)
        && offset >= current.offset
        && span(offset - current.offset, bytes, current.bytes);
    valid.then_some(current)
}
fn overlap(value: &Payload, offset: u64, bytes: u64) -> bool {
    value.destination_offset < offset.wrapping_add(bytes) && offset < value.destination_offset.wrapping_add(value.bytes.len() as u64)
}
fn erase_overlap(g: &mut Global, o: &mut OwnerData, offset: u64, bytes: u64, list: u64, source: u64) {
    if bytes == 0 || offset > u64::MAX - bytes {
        let labels: Vec<_> = o.ranges.iter().map(|v| (v.destination_offset, v.bytes.len() as u64, v.sequence)).collect();
        for (at, length, sequence) in labels {
            event(g, o, reason::OVERWRITTEN_PAYLOAD, at, length, sequence, list, source);
        }
        empty(g, o, operation::NONE, false);
        return;
    }
    for &pending in &o.publications {
        // SAFETY: a live publication (see empty).
        unsafe {
            if (*pending).offset < offset + bytes && offset < (*pending).offset.wrapping_add((*pending).bytes) {
                (*pending).valid = false;
            }
        }
    }
    for index in (0..o.ranges.len()).rev() {
        if overlap(&o.ranges[index], offset, bytes) {
            let (at, length, sequence) = (o.ranges[index].destination_offset, o.ranges[index].bytes.len() as u64, o.ranges[index].sequence);
            event(g, o, reason::OVERWRITTEN_PAYLOAD, at, length, sequence, list, source);
            remove(g, o, index);
        }
    }
    o.mutation += 1;
}
fn erase_structured_overlap(g: &mut Global, o: &mut OwnerData, offset: u64, bytes: u64) {
    if bytes == 0 || offset > u64::MAX - bytes {
        empty_structured(g, o);
        return;
    }
    let mut retained: Vec<StructuredRange> = Vec::with_capacity(o.structured.len() + 1);
    let mut retained_bytes = 0u64;
    let end_of_write = offset + bytes;
    for range in &o.structured {
        let end = range.offset + range.bytes as u64;
        if end <= offset || range.offset >= end_of_write {
            retained.push(range.clone());
            retained_bytes += range.bytes as u64;
            continue;
        }
        if range.offset < offset {
            let mut left = range.clone();
            left.bytes = (offset - range.offset) as usize;
            retained_bytes += left.bytes as u64;
            retained.push(left);
        }
        if end > end_of_write {
            let mut right = range.clone();
            let skip = end_of_write - range.offset;
            right.offset += skip;
            right.data_offset += skip as usize;
            right.bytes -= skip as usize;
            retained_bytes += right.bytes as u64;
            retained.push(right);
        }
    }
    while retained.len() > MAX_STRUCTURED_RANGES_PER_RESOURCE {
        retained_bytes -= retained[0].bytes as u64;
        retained.remove(0);
        g.evictions += 1;
    }
    g.structured_bytes -= o.structured_bytes;
    g.structured_ranges -= o.structured.len() as u64;
    o.structured = retained;
    o.structured_bytes = retained_bytes;
    g.structured_bytes += retained_bytes;
    g.structured_ranges += o.structured.len() as u64;
}
#[allow(clippy::too_many_arguments)]
fn invalidate_range_event(raw: *mut c_void, offset: u64, bytes: u64, op: u64, list: u64, source: u64, sequence: u64) -> bool {
    let Some(owner) = cache(raw, false) else {
        return false;
    };
    let mut g = lock();
    drain(&mut g);
    let o = data(owner.ptr());
    increment(&mut o.range_invalidations);
    operation(&mut g, o, op, offset, bytes, list, source, sequence);
    let full = bytes == 0 || offset > u64::MAX - bytes;
    let why = if op == operation::OBSERVE_COPY { reason::OBSERVE_COPY_INVALIDATION } else { reason::EXPLICIT_RANGE_INVALIDATION };
    let width = o.resource_width;
    event(&mut g, o, why, if full { 0 } else { offset }, if full { width } else { bytes }, 0, list, source);
    erase_overlap(&mut g, o, offset, bytes, list, source);
    erase_structured_overlap(&mut g, o, offset, bytes);
    true
}
fn rejected_event(raw: *mut c_void, offset: u64, bytes: u64, list: u64, source: u64, why: u64) {
    let Some(owner) = cache(raw, false) else {
        return;
    };
    let mut g = lock();
    drain(&mut g);
    event(&mut g, data(owner.ptr()), reason::REJECTED_BASE + why, offset, bytes, 0, list, source);
}
/// Repacks only backing data with no captured or generic holder; each range keeps its exact original labels.
fn compact_structured(o: &mut OwnerData) {
    let mut groups: Vec<(*const Payload, usize, Vec<usize>)> = Vec::new();
    for (index, range) in o.structured.iter().enumerate() {
        let key = Arc::as_ptr(&range.data);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(group) => {
                group.1 += range.bytes;
                group.2.push(index);
            }
            None => groups.push((key, range.bytes, vec![index])),
        }
    }
    for (_, bytes, indexes) in groups {
        let first = &o.structured[indexes[0]].data;
        if bytes == first.bytes.len() || Arc::strong_count(first) != indexes.len() {
            continue;
        }
        let old = first.clone();
        let mut packed = Payload::default();
        packed.bytes = Vec::with_capacity(bytes);
        packed.producer_list = old.producer_list;
        packed.sequence = old.sequence;
        packed.source_resource = old.source_resource;
        packed.destination_offset = old.destination_offset;
        packed.source_offset = old.source_offset;
        packed.copied_bytes = old.copied_bytes;
        packed.gpu_base = old.gpu_base;
        packed.resource_width = old.resource_width;
        for &index in &indexes {
            let range = &o.structured[index];
            packed.bytes.extend_from_slice(&old.bytes[range.data_offset..range.data_offset + range.bytes]);
        }
        account(&mut packed, &o.budget);
        let packed = Arc::new(packed);
        let mut at = 0usize;
        for &index in &indexes {
            let range = &mut o.structured[index];
            range.data = packed.clone();
            range.data_offset = at;
            at += range.bytes;
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeClass {
    Any,
    Small,
    Ordinary,
}
struct Selection {
    owner: *mut Owner,
    at: usize,
    oldest: u64,
    payload: *const Payload,
}
fn select(s: &mut Selection, owners: &[*mut Owner], only: *mut Owner, generic_only: bool, without_structured: bool, candidates: RangeClass) {
    for &p in owners {
        let o = data(p);
        if (!only.is_null() && p != only) || (without_structured && !o.structured.is_empty()) {
            continue;
        }
        for (index, value) in o.ranges.iter().enumerate() {
            let small = value.bytes.len() <= MAX_SMALL_COPY_BYTES;
            if (candidates == RangeClass::Small && !small) || (candidates == RangeClass::Ordinary && small) {
                continue;
            }
            if value.sequence < s.oldest {
                s.owner = p;
                s.at = index;
                s.payload = Arc::as_ptr(value);
                s.oldest = value.sequence;
            }
        }
        if !generic_only {
            for range in &o.structured {
                if range.data.sequence < s.oldest {
                    s.owner = p;
                    s.payload = Arc::as_ptr(&range.data);
                    s.oldest = range.data.sequence;
                }
            }
        }
    }
}
fn evict(g: &mut Global, only: *mut Owner, generic_only: bool, why: u64, class: RangeClass) -> bool {
    let mut s = Selection { owner: core::ptr::null_mut(), at: 0, oldest: u64::MAX, payload: core::ptr::null() };
    // Large transient copies must not flush many untouched small material records just because those original uploads
    // happened earlier.
    let owners = g.owners.clone();
    if only.is_null() && class == RangeClass::Any {
        select(&mut s, &owners, only, generic_only, true, RangeClass::Ordinary);
    }
    if s.owner.is_null() {
        select(&mut s, &owners, only, generic_only, only.is_null(), class);
    }
    if s.owner.is_null() && only.is_null() {
        select(&mut s, &owners, only, generic_only, false, class);
    }
    if s.owner.is_null() {
        return false;
    }
    let (selected, at, payload) = (s.owner, s.at, s.payload);
    let o = data(selected);
    let prior = o.ranges.len();
    // SAFETY: the selected payload, still held by the owner.
    let (offset, bytes, source, list, sequence) = unsafe {
        ((*payload).destination_offset, (*payload).bytes.len() as u64, (*payload).source_resource, (*payload).producer_list, (*payload).sequence)
    };
    if generic_only {
        event(g, o, why, offset, bytes, sequence, list, source);
        remove(g, o, at);
    } else {
        let labels: Vec<_> = o
            .ranges
            .iter()
            .filter(|v| Arc::as_ptr(v) == payload)
            .map(|v| (v.destination_offset, v.bytes.len() as u64, v.sequence, v.producer_list, v.source_resource))
            .collect();
        for (at, length, sequence, list, source) in labels {
            event(g, o, why, at, length, sequence, list, source);
            if length as usize <= MAX_SMALL_COPY_BYTES {
                o.small_ranges -= 1;
            }
            g.current_bytes -= length;
            g.ranges -= 1;
        }
        o.ranges.retain(|v| Arc::as_ptr(v) != payload);
        let labels: Vec<_> = o
            .structured
            .iter()
            .filter(|r| Arc::as_ptr(&r.data) == payload)
            .map(|r| (r.offset, r.bytes as u64, r.data.sequence, r.data.producer_list, r.data.source_resource))
            .collect();
        for (at, length, sequence, list, source) in labels {
            event(g, o, why, at, length, sequence, list, source);
            g.structured_bytes -= length;
            o.structured_bytes -= length;
            g.structured_ranges -= 1;
        }
        o.structured.retain(|r| Arc::as_ptr(&r.data) != payload);
        if o.structured.is_empty() {
            o.structured = Vec::new();
        }
    }
    let removed = prior - o.ranges.len();
    for _ in 0..removed {
        increment(&mut o.generic_evictions);
    }
    if removed != 0 {
        operation(g, o, operation::GENERIC_EVICTION, offset, bytes, list, source, 0);
    }
    o.mutation += 1;
    g.evictions += 1;
    true
}
fn space(g: &mut Global, owner: *mut Owner, bytes: usize) -> bool {
    let small = bytes <= MAX_SMALL_COPY_BYTES;
    loop {
        let o = data(owner);
        let full = if small { o.small_ranges >= MAX_SMALL_RANGES_PER_RESOURCE } else { o.ranges.len() - o.small_ranges >= MAX_RANGES_PER_RESOURCE };
        if !full {
            break;
        }
        if !evict(g, owner, true, reason::GENERIC_RANGE_EVICTION, if small { RangeClass::Small } else { RangeClass::Ordinary }) {
            return false;
        }
    }
    let resource_limit = MAX_RESOURCE_BYTES.wrapping_sub(bytes);
    if data(owner).budget.load(Relaxed) > resource_limit {
        compact_structured(data(owner));
    }
    while data(owner).budget.load(Relaxed) > resource_limit {
        if !evict(g, owner, false, reason::RESOURCE_BYTES_EVICTION, RangeClass::Any) {
            return false;
        }
    }
    let global_limit = MAX_GLOBAL_BYTES.wrapping_sub(bytes);
    while PAYLOAD_BYTES.load(Relaxed) > global_limit || PAYLOAD_COUNT.load(Relaxed) >= MAX_PAYLOADS {
        let why = if PAYLOAD_BYTES.load(Relaxed) > global_limit { reason::GLOBAL_BYTES_EVICTION } else { reason::GLOBAL_PAYLOAD_EVICTION };
        if !evict(g, core::ptr::null_mut(), false, why, RangeClass::Any) {
            return false;
        }
    }
    true
}
struct RecordRequest<'a> {
    list: u64,
    offset: u64,
    source_offset: u64,
    bytes: &'a [u8],
    gpu: u64,
    width: u64,
    generation: u64,
    publication: Option<&'a Publication>,
    enabled: Option<&'a AtomicU32>,
    destination: *mut c_void,
    expected_hint: Option<StructuredHint>,
    observed_source: u64,
}
fn record(owner: *mut Owner, request: &RecordRequest, result: &mut u64) -> bool {
    let mut g = lock();
    drain(&mut g);
    let length = request.bytes.len() as u64;
    let reject = |g: &mut Global, result: &mut u64, why: u64| {
        *result = why;
        g.rejected += 1;
        false
    };
    if g.generation != request.generation {
        return reject(&mut g, result, outcome::GENERATION_CHANGED);
    }
    if request.enabled.is_some_and(|e| e.load(Acquire) == 0) {
        return reject(&mut g, result, outcome::DISABLED);
    }
    register(&mut g, owner);
    if let Some(p) = request.publication {
        if p.owner != owner || !p.valid || p.offset != request.offset || p.bytes != length {
            return reject(&mut g, result, outcome::CHANGED);
        }
    }
    if let Some(expected) = request.expected_hint {
        let current = hint(request.destination, request.generation, request.offset, length, request.gpu, request.width);
        if current != Some(expected) {
            return reject(&mut g, result, outcome::NO_HINT);
        }
    }
    let o = data(owner);
    if request.observed_source == 0 {
        increment(&mut o.range_invalidations);
        operation(&mut g, o, operation::RECORD_INTENDED_COPY, request.offset, length, request.list, 0, 0);
        event(&mut g, o, reason::DIRECT_COPY_INVALIDATION, request.offset, length, 0, request.list, 0);
    }
    erase_overlap(&mut g, o, request.offset, length, request.list, request.observed_source);
    if request.observed_source == 0 {
        erase_structured_overlap(&mut g, o, request.offset, length);
    }
    if g.sequence == u64::MAX {
        return reject(&mut g, result, outcome::CAPACITY);
    }
    if !space(&mut g, owner, request.bytes.len()) {
        return reject(&mut g, result, outcome::BUDGET);
    }
    let o = data(owner);
    let structured = request.observed_source != 0 && request.expected_hint.is_some();
    if let (true, Some(expected)) = (structured, request.expected_hint) {
        if o.structured_hint != expected {
            empty_structured(&mut g, o);
            o.structured_hint = expected;
        }
        while o.structured.len() >= MAX_STRUCTURED_RANGES_PER_RESOURCE {
            remove_structured(&mut g, o, 0);
            g.evictions += 1;
        }
    }
    g.sequence += 1;
    let mut value = Payload::default();
    value.bytes = request.bytes.to_vec();
    value.producer_list = request.list;
    value.sequence = g.sequence;
    value.destination_offset = request.offset;
    value.source_resource = request.observed_source;
    value.source_offset = request.source_offset;
    value.copied_bytes = length;
    value.gpu_base = request.gpu;
    value.resource_width = request.width;
    account(&mut value, &o.budget);
    let value = Arc::new(value);
    if structured {
        let at = o.structured.partition_point(|r| r.offset < request.offset);
        o.structured.insert(at, StructuredRange { data: value.clone(), offset: request.offset, data_offset: 0, bytes: request.bytes.len() });
        o.structured_bytes += length;
        g.structured_bytes += length;
        g.structured_ranges += 1;
    }
    o.ranges.push(value);
    if request.bytes.len() <= MAX_SMALL_COPY_BYTES {
        o.small_ranges += 1;
    }
    g.current_bytes += length;
    g.ranges += 1;
    let sequence = g.sequence;
    event(&mut g, o, reason::PUBLISHED_COPY, request.offset, length, sequence, request.list, request.observed_source);
    *result = outcome::SUCCESS;
    true
}
fn recording(list: *mut c_void) -> bool {
    // SAFETY: a live command list of the caller's, or null.
    unsafe { ID3D12CommandList::from_raw_borrowed(&list) }.is_some_and(|l| {
        let kind = unsafe { l.GetType() };
        kind == D3D12_COMMAND_LIST_TYPE_DIRECT || kind == D3D12_COMMAND_LIST_TYPE_COPY
    })
}
fn device_of(raw: *mut c_void) -> Option<ID3D12Device> {
    // SAFETY: a live device child of the caller's, or null.
    let child = unsafe { ID3D12DeviceChild::from_raw_borrowed(&raw) }?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live object.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}
fn same_device(a: *mut c_void, b: *mut c_void) -> bool {
    match (device_of(a), device_of(b)) {
        (Some(x), Some(y)) => x.as_raw() == y.as_raw(),
        _ => false,
    }
}
fn sample(raw: *mut c_void, offset: u64, output: &mut [u8]) -> bool {
    let Some(source) = resource(&raw) else {
        return false;
    };
    let bytes = output.len() as u64;
    // SAFETY (whole block): a live resource; the mapped range covers the bytes read.
    unsafe {
        let desc = source.GetDesc();
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || !span(offset, bytes, desc.Width)
            || offset > usize::MAX as u64 - bytes
            || source.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err()
            || heap.Type != D3D12_HEAP_TYPE_UPLOAD
        {
            return false;
        }
        let requested = D3D12_RANGE { Begin: offset as usize, End: (offset + bytes) as usize };
        let mut mapped: *mut c_void = core::ptr::null_mut();
        if source.Map(0, Some(&requested), Some(&mut mapped)).is_err() {
            return false;
        }
        let address = mapped as usize;
        let mut read = 0usize;
        let okay = !mapped.is_null()
            && address <= usize::MAX - offset as usize
            && address + offset as usize <= usize::MAX - output.len()
            && ReadProcessMemory(GetCurrentProcess(), (address + offset as usize) as *const c_void, output.as_mut_ptr().cast(), output.len(), Some(&mut read)).is_ok()
            && read == output.len();
        let none = D3D12_RANGE::default();
        source.Unmap(0, Some(&none));
        okay
    }
}
fn rejected() {
    let mut g = lock();
    drain(&mut g);
    g.rejected += 1;
}
fn generation() -> u64 {
    lock().generation
}
fn diagnostic_sequence() -> u64 {
    let mut g = lock();
    if g.diagnostic_sequence == u64::MAX {
        0
    } else {
        g.diagnostic_sequence += 1;
        g.diagnostic_sequence
    }
}

// ---- the diagnostics (CopyDiagnostic / MarkDiagnostic: 24 values, the checksum last)

type Diagnostic = [u64; 24];
fn checksum(value: &Diagnostic) -> u64 {
    value[..23]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037u64, |hash, b| (hash ^ b as u64).wrapping_mul(1099511628211))
}
fn read_diagnostic(raw: *mut c_void, key: &GUID, version: u64) -> Option<Diagnostic> {
    let r = resource(&raw)?;
    let mut value: Diagnostic = [0; 24];
    let mut size = 192u32;
    // SAFETY: room for the receipt.
    let ok = unsafe { r.GetPrivateData(key, &mut size, Some(value.as_mut_ptr().cast())) }.is_ok();
    (ok && size == 192 && value[0] == version && value[1] == raw as u64 && value[2] != 0 && value[23] == checksum(&value)).then_some(value)
}
fn write_diagnostic(raw: *mut c_void, key: &GUID, mut value: Diagnostic) {
    let Some(r) = resource(&raw) else {
        return;
    };
    let _g = lock();
    if read_diagnostic(raw, key, value[0]).is_some_and(|previous| previous[2] > value[2]) {
        return;
    }
    value[23] = checksum(&value);
    // SAFETY: 192 bytes of the receipt.
    let _ = unsafe { r.SetPrivateData(key, 192, Some(value.as_ptr().cast())) };
}
mod copy_field {
    pub const VERSION: usize = 0;
    pub const RESOURCE: usize = 1;
    pub const SEQUENCE: usize = 2;
    pub const GENERATION: usize = 3;
    pub const PUBLICATION_GENERATION: usize = 4;
    pub const PRODUCER_LIST: usize = 5;
    pub const SOURCE_RESOURCE: usize = 6;
    pub const DESTINATION_OFFSET: usize = 7;
    pub const SOURCE_OFFSET: usize = 8;
    pub const BYTES: usize = 9;
    pub const DESTINATION_GPU: usize = 10;
    pub const DESTINATION_WIDTH: usize = 11;
    pub const SOURCE_HEAP_TYPE: usize = 12;
    pub const SOURCE_CPU_PAGE: usize = 13;
    pub const SOURCE_HEAP_FLAGS: usize = 14;
    pub const HINT_PRESENT: usize = 15;
    pub const HINT_GENERATION: usize = 16;
    pub const HINT_FRESH: usize = 17;
    pub const HINT_OFFSET: usize = 18;
    pub const HINT_BYTES: usize = 19;
    pub const OUTCOME: usize = 20;
    pub const STAGE: usize = 21;
}
mod mark_field {
    pub const VERSION: usize = 0;
    pub const RESOURCE: usize = 1;
    pub const SEQUENCE: usize = 2;
    pub const GENERATION: usize = 3;
    pub const OUTCOME: usize = 4;
    pub const FIRST_ELEMENT: usize = 5;
    pub const NUM_ELEMENTS: usize = 6;
    pub const STRIDE: usize = 7;
    pub const FORMAT: usize = 8;
    pub const DIMENSION: usize = 9;
    pub const COMPONENT_MAPPING: usize = 10;
    pub const FLAGS: usize = 11;
    pub const VIEW_OFFSET: usize = 12;
    pub const VIEW_BYTES: usize = 13;
    pub const RESOURCE_GPU: usize = 14;
    pub const RESOURCE_WIDTH: usize = 15;
    pub const HEAP_TYPE: usize = 16;
    pub const CPU_PAGE: usize = 17;
    pub const HEAP_FLAGS: usize = 18;
    pub const HINT_ACCEPTED: usize = 19;
    pub const STAGE: usize = 20;
}

// ---- the operations

/// Invalidate(destination): the whole owner.
pub fn invalidate(raw: *mut c_void) {
    let Some(owner) = cache(raw, false) else {
        return;
    };
    let mut g = lock();
    drain(&mut g);
    empty(&mut g, data(owner.ptr()), operation::INVALIDATE, true);
}
/// Invalidate(destination, offset, bytes).
pub fn invalidate_range(raw: *mut c_void, offset: u64, bytes: u64) {
    invalidate_range_event(raw, offset, bytes, operation::INVALIDATE_RANGE, 0, 0, 0);
}
/// RecordIntendedCopy: the caller's stable sample of an upload source as the current copy candidate.
pub fn record_intended_copy(list: *mut c_void, raw: *mut c_void, offset: u64, source_offset: u64, bytes: Option<&[u8]>) -> bool {
    let current = generation();
    let length = bytes.map_or(0, |b| b.len());
    let target = destination(raw, offset, length, MAX_COPY_BYTES);
    let fail = || {
        invalidate_range(raw, offset, length as u64);
        rejected();
        false
    };
    let (Some(bytes), Some((gpu, width))) = (bytes, target) else {
        return fail();
    };
    if !recording(list) || source_offset > u64::MAX - length as u64 || !same_device(list, raw) {
        return fail();
    }
    let Some(owner) = cache(raw, true) else {
        return fail();
    };
    let mut result = 0;
    record(
        owner.ptr(),
        &RecordRequest {
            list: list as u64,
            offset,
            source_offset,
            bytes,
            gpu,
            width,
            generation: current,
            publication: None,
            enabled: None,
            destination: core::ptr::null_mut(),
            expected_hint: None,
            observed_source: 0,
        },
        &mut result,
    )
}
/// MarkStructuredView: the current generation's exact structured-128 SRV hint (or its removal).
pub fn mark_structured_view(raw: *mut c_void, view: Option<&D3D12_SHADER_RESOURCE_VIEW_DESC>, enabled: Option<&AtomicU32>) -> bool {
    use mark_field::*;
    let mut d: Diagnostic = [0; 24];
    d[VERSION] = MARK_DIAGNOSTIC_VERSION;
    d[RESOURCE] = raw as u64;
    d[SEQUENCE] = diagnostic_sequence();
    d[GENERATION] = generation();
    d[STAGE] = stage::BEGIN;
    let mut buffer: Option<D3D12_BUFFER_SRV> = None;
    if let Some(v) = view {
        d[FORMAT] = v.Format.0 as u64;
        d[DIMENSION] = v.ViewDimension.0 as u64;
        d[COMPONENT_MAPPING] = v.Shader4ComponentMapping as u64;
        if v.ViewDimension == D3D12_SRV_DIMENSION_BUFFER {
            // SAFETY: the union member the dimension names.
            let b = unsafe { v.Anonymous.Buffer };
            d[FIRST_ELEMENT] = b.FirstElement;
            d[NUM_ELEMENTS] = b.NumElements as u64;
            d[STRIDE] = b.StructureByteStride as u64;
            d[FLAGS] = b.Flags.0 as u64;
            buffer = Some(b);
        }
    }
    let finish = |d: &mut Diagnostic, why: u64, result: bool| {
        d[OUTCOME] = why;
        d[HINT_ACCEPTED] = result as u64;
        write_diagnostic(raw, &MARK_DIAGNOSTIC_KEY, *d);
        result
    };
    write_diagnostic(raw, &MARK_DIAGNOSTIC_KEY, d);
    if enabled.is_some_and(|e| e.load(Acquire) == 0) {
        return finish(&mut d, outcome::DISABLED, false);
    }
    let current = d[GENERATION];
    if let Some(r) = resource(&raw) {
        // SAFETY (whole block): a live resource.
        unsafe {
            d[RESOURCE_WIDTH] = r.GetDesc().Width;
            d[RESOURCE_GPU] = r.GetGPUVirtualAddress();
            let mut heap = D3D12_HEAP_PROPERTIES::default();
            let mut flags = D3D12_HEAP_FLAGS::default();
            if r.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_ok() {
                d[HEAP_TYPE] = heap.Type.0 as u64;
                d[CPU_PAGE] = heap.CPUPageProperty.0 as u64;
                d[HEAP_FLAGS] = flags.0 as u64;
            }
        }
    }
    d[STAGE] = stage::HINT;
    let (mut gpu, mut width, mut offset, mut bytes) = (0u64, 0u64, 0u64, 0u64);
    let mut valid = !raw.is_null()
        && view.is_some_and(|v| {
            v.ViewDimension == D3D12_SRV_DIMENSION_BUFFER
                && v.Format == DXGI_FORMAT_UNKNOWN
                && v.Shader4ComponentMapping == D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        })
        && buffer.is_some_and(|b| b.Flags == D3D12_BUFFER_SRV_FLAG_NONE && b.StructureByteStride == 128 && b.NumElements != 0 && b.FirstElement <= u64::MAX / 128);
    if valid {
        let b = buffer.expect("buffer view");
        offset = b.FirstElement * 128;
        bytes = b.NumElements as u64 * 128;
        valid = bytes <= MAX_RESOURCE_BYTES as u64
            && match destination(raw, offset, bytes as usize, MAX_RESOURCE_BYTES) {
                Some((g, w)) => {
                    gpu = g;
                    width = w;
                    true
                }
                None => false,
            };
    }
    d[VIEW_OFFSET] = offset;
    d[VIEW_BYTES] = bytes;
    let mut why = outcome::INVALID_VIEW;
    let mut accepted = false;
    {
        let mut g = lock();
        drain(&mut g);
        if g.generation != current {
            why = outcome::GENERATION_CHANGED;
        } else if enabled.is_some_and(|e| e.load(Acquire) == 0) {
            why = outcome::DISABLED;
        } else if !valid {
            if let Some(r) = resource(&raw) {
                // SAFETY: removes the hint.
                let _ = unsafe { r.SetPrivateData(&STRUCTURED_KEY, 0, None) };
            }
            if let Some(owner) = existing(raw) {
                let o = data(owner.ptr());
                empty_structured(&mut g, o);
                o.mutation += 1;
            }
        } else {
            let hint = StructuredHint { signature: STRUCTURED_SIGNATURE, resource: raw as u64, gpu, width, generation: current, offset, bytes };
            // SAFETY: 56 bytes of the hint.
            accepted = resource(&raw).is_some_and(|r| unsafe { r.SetPrivateData(&STRUCTURED_KEY, 56, Some(&hint as *const StructuredHint as *const c_void)) }.is_ok());
            why = if accepted { outcome::SUCCESS } else { outcome::API_FAILED };
            if let Some(owner) = existing(raw) {
                let o = data(owner.ptr());
                if !accepted || o.structured_hint != hint {
                    empty_structured(&mut g, o);
                    o.mutation += 1;
                }
            }
        }
    }
    d[STAGE] = stage::COMPLETE;
    finish(&mut d, why, accepted)
}
/// ObserveCopy: an actual copy from an upload source, sampled twice and published (or the destination range dropped).
#[allow(clippy::too_many_arguments)]
pub fn observe_copy(list: *mut c_void, raw: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, bytes: u64, enabled: Option<&AtomicU32>) -> bool {
    use copy_field::*;
    let mut d: Diagnostic = [0; 24];
    d[VERSION] = COPY_DIAGNOSTIC_VERSION;
    d[RESOURCE] = raw as u64;
    d[SEQUENCE] = diagnostic_sequence();
    d[GENERATION] = generation();
    d[PRODUCER_LIST] = list as u64;
    d[SOURCE_RESOURCE] = source as u64;
    d[DESTINATION_OFFSET] = offset;
    d[SOURCE_OFFSET] = source_offset;
    d[BYTES] = bytes;
    let mut attempted = false;
    let finish = |d: &mut Diagnostic, attempted: bool, why: u64, result: bool| {
        d[OUTCOME] = why;
        d[PUBLICATION_GENERATION] = generation();
        if attempted && why != outcome::SUCCESS {
            rejected_event(raw, offset, bytes, d[PRODUCER_LIST], d[SOURCE_RESOURCE], why);
        }
        write_diagnostic(raw, &COPY_DIAGNOSTIC_KEY, *d);
        result
    };
    write_diagnostic(raw, &COPY_DIAGNOSTIC_KEY, d);
    if enabled.is_some_and(|e| e.load(Acquire) == 0) {
        return finish(&mut d, attempted, outcome::DISABLED, false);
    }
    attempted = true;
    let current = d[GENERATION];
    let invalidated = invalidate_range_event(raw, offset, bytes, operation::OBSERVE_COPY, list as u64, source as u64, d[SEQUENCE]);
    let large = bytes > MAX_COPY_BYTES as u64;
    if let Some(r) = resource(&raw) {
        // SAFETY: a live resource.
        unsafe {
            d[DESTINATION_WIDTH] = r.GetDesc().Width;
            d[DESTINATION_GPU] = r.GetGPUVirtualAddress();
        }
    }
    if let Some(observed) = raw_hint(raw) {
        d[HINT_PRESENT] = 1;
        d[HINT_GENERATION] = observed.generation;
        d[HINT_OFFSET] = observed.offset;
        d[HINT_BYTES] = observed.bytes;
    }
    let mut source_heap = D3D12_HEAP_PROPERTIES::default();
    let mut source_flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live source of the caller's, or null.
    let heap_known = resource(&source).is_some_and(|s| unsafe { s.GetHeapProperties(Some(&mut source_heap), Some(&mut source_flags)) }.is_ok());
    if heap_known {
        d[SOURCE_HEAP_TYPE] = source_heap.Type.0 as u64;
        d[SOURCE_CPU_PAGE] = source_heap.CPUPageProperty.0 as u64;
        d[SOURCE_HEAP_FLAGS] = source_flags.0 as u64;
    }
    d[STAGE] = stage::DESTINATION;
    if !recording(list) || bytes == 0 || bytes > MAX_RESOURCE_BYTES as u64 || source.is_null() || !same_device(list, raw) || !same_device(raw, source) {
        rejected();
        return finish(&mut d, attempted, outcome::ARGUMENTS, false);
    }
    let Some((gpu, width)) = destination(raw, offset, bytes as usize, if large { MAX_RESOURCE_BYTES } else { MAX_COPY_BYTES }) else {
        rejected();
        return finish(&mut d, attempted, outcome::UNSUPPORTED_DESTINATION, false);
    };
    let mut fresh = hint(raw, current, offset, bytes, gpu, width);
    d[HINT_FRESH] = fresh.is_some() as u64;
    d[STAGE] = stage::SOURCE;
    if heap_known && source_heap.Type == D3D12_HEAP_TYPE_DEFAULT {
        rejected();
        return finish(&mut d, attempted, outcome::SOURCE_DEFAULT, false);
    }
    if !heap_known || source_heap.Type != D3D12_HEAP_TYPE_UPLOAD {
        rejected();
        return finish(&mut d, attempted, outcome::UNSUPPORTED_HEAP, false);
    }
    if generation() != current {
        rejected();
        return finish(&mut d, attempted, outcome::GENERATION_CHANGED, false);
    }
    if enabled.is_some_and(|e| e.load(Acquire) == 0) {
        rejected();
        return finish(&mut d, attempted, outcome::DISABLED, false);
    }
    if large {
        let mut g = lock();
        drain(&mut g);
        fresh = if g.generation == current { hint(raw, current, offset, bytes, gpu, width) } else { None };
        d[HINT_FRESH] = fresh.is_some() as u64;
    }
    d[STAGE] = stage::HINT;
    if large && fresh.is_none() {
        rejected();
        return finish(&mut d, attempted, outcome::NO_HINT, false);
    }
    let Some(owner) = cache(raw, true) else {
        rejected();
        return finish(&mut d, attempted, outcome::API_FAILED, false);
    };
    let mut publication = Box::new(Publication::default());
    let mut ready = outcome::SUCCESS;
    {
        let mut g = lock();
        drain(&mut g);
        if g.generation != current {
            ready = outcome::GENERATION_CHANGED;
        } else if enabled.is_some_and(|e| e.load(Acquire) == 0) {
            ready = outcome::DISABLED;
        } else {
            register(&mut g, owner.ptr());
            let o = data(owner.ptr());
            if !invalidated {
                increment(&mut o.range_invalidations);
                operation(&mut g, o, operation::OBSERVE_COPY, offset, bytes, list as u64, source as u64, d[SEQUENCE]);
                event(&mut g, o, reason::OBSERVE_COPY_INVALIDATION, offset, bytes, 0, d[PRODUCER_LIST], d[SOURCE_RESOURCE]);
            }
            erase_overlap(&mut g, o, offset, bytes, d[PRODUCER_LIST], d[SOURCE_RESOURCE]);
            if !publication.begin(owner.ptr(), offset, bytes) {
                ready = outcome::CAPACITY;
            }
        }
    }
    if ready != outcome::SUCCESS {
        rejected();
        return finish(&mut d, attempted, ready, false);
    }
    let mut first = vec![0u8; bytes as usize];
    let mut second = vec![0u8; bytes as usize];
    d[STAGE] = stage::FIRST_SAMPLE;
    if !sample(source, source_offset, &mut first) {
        rejected();
        return finish(&mut d, attempted, outcome::MAP_FAILED, false);
    }
    d[STAGE] = stage::SECOND_SAMPLE;
    if !sample(source, source_offset, &mut second) {
        rejected();
        return finish(&mut d, attempted, outcome::MAP_FAILED, false);
    }
    d[STAGE] = stage::COMPARE;
    if first != second {
        rejected();
        return finish(&mut d, attempted, outcome::UNSTABLE, false);
    }
    d[STAGE] = stage::PUBLICATION;
    let mut result = outcome::API_FAILED;
    let accepted = record(
        owner.ptr(),
        &RecordRequest {
            list: list as u64,
            offset,
            source_offset,
            bytes: &first,
            gpu,
            width,
            generation: current,
            publication: Some(&publication),
            enabled,
            destination: raw,
            expected_hint: if d[HINT_FRESH] != 0 { fresh } else { None },
            observed_source: source as u64,
        },
        &mut result,
    );
    if accepted {
        d[STAGE] = stage::COMPLETE;
    }
    let answer = finish(&mut d, attempted, result, accepted);
    drop(publication);
    drop(owner);
    answer
}
/// QueryCopyDiagnostic (`mark` false) / QueryMarkDiagnostic.
pub fn query_diagnostic(raw: *mut c_void, mark: bool) -> Option<Diagnostic> {
    if mark {
        read_diagnostic(raw, &MARK_DIAGNOSTIC_KEY, MARK_DIAGNOSTIC_VERSION)
    } else {
        read_diagnostic(raw, &COPY_DIAGNOSTIC_KEY, COPY_DIAGNOSTIC_VERSION)
    }
}
/// The bounded buffer query every coverage query starts with: (gpu, width).
fn query_target(raw: *mut c_void, offset: u64, bytes: usize, address: Option<u64>) -> Option<(u64, u64)> {
    let r = resource(&raw)?;
    if bytes == 0 || bytes > MAX_COPY_BYTES {
        return None;
    }
    // SAFETY: a live resource.
    let (desc, gpu) = unsafe { (r.GetDesc(), r.GetGPUVirtualAddress()) };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || gpu == 0 || gpu > u64::MAX - desc.Width {
        return None;
    }
    let offset = match address {
        Some(address) if address < gpu => return None,
        Some(address) => address - gpu,
        None => offset,
    };
    span(offset, bytes as u64, desc.Width).then_some((gpu, desc.Width))
}
/// QueryGenericCoverage into the caller's (default) 24 values.
pub fn query_generic_coverage(raw: *mut c_void, offset: u64, bytes: usize, out: &mut [u64; 24]) -> bool {
    let Some((gpu, width)) = query_target(raw, offset, bytes, None) else {
        return false;
    };
    let owner = cache(raw, false);
    let mut g = lock();
    drain(&mut g);
    out[0] = GENERIC_COVERAGE_VERSION;
    out[1] = raw as u64;
    out[2] = g.generation;
    out[6] = offset;
    out[7] = bytes as u64;
    out[11] = offset;
    let Some(o) = owner.as_ref().map(|o| data(o.ptr())).filter(|o| o.resource == raw as u64) else {
        return true;
    };
    out[3] = 1;
    out[4] = o.ranges.len() as u64;
    out[5] = o.budget.load(Relaxed) as u64;
    out[14] = o.full_invalidations;
    out[15] = o.range_invalidations;
    out[16] = o.generic_evictions;
    out[17] = o.last_invalidation_offset;
    out[18] = o.last_invalidation_bytes;
    out[19] = o.last_operation;
    out[20] = o.last_operation_sequence;
    out[21] = o.last_operation_source;
    out[22] = o.last_operation_producer_list;
    out[23] = o.last_operation_generation;
    if o.ranges.len() > MAX_GENERIC_RANGES_PER_RESOURCE {
        return false;
    }
    let end = offset + bytes as u64;
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for value in &o.ranges {
        if out[12] == 0 || value.sequence < out[12] {
            out[12] = value.sequence;
        }
        out[13] = out[13].max(value.sequence);
        if value.gpu_base != gpu || value.resource_width != width {
            continue;
        }
        if offset >= value.destination_offset && span(offset - value.destination_offset, bytes as u64, value.bytes.len() as u64) {
            out[9] += 1;
        }
        let value_end = value.destination_offset + value.bytes.len() as u64;
        if value_end <= offset || value.destination_offset >= end {
            continue;
        }
        spans.push((offset.max(value.destination_offset), end.min(value_end)));
        out[8] += 1;
    }
    spans.sort_unstable();
    let mut cursor = offset;
    let mut hole = false;
    for (begin, finish) in spans {
        if !hole && begin > cursor {
            out[11] = cursor;
            hole = true;
        }
        let start = cursor.max(begin);
        if finish > start {
            out[10] += finish - start;
        }
        cursor = cursor.max(finish);
    }
    if !hole {
        out[11] = if cursor == end { u64::MAX } else { cursor };
    }
    true
}
/// QueryCacheEvents into the caller's 32 values.
pub fn query_cache_events(raw: *mut c_void, offset: u64, bytes: usize, out: &mut [u64; 32]) -> bool {
    if query_target(raw, offset, bytes, None).is_none() {
        return false;
    }
    let owner = cache(raw, false);
    let mut g = lock();
    drain(&mut g);
    out[0] = CACHE_EVENTS_VERSION;
    out[1] = raw as u64;
    out[2] = g.generation;
    out[4] = offset;
    out[5] = bytes as u64;
    let Some(o) = owner.as_ref().map(|o| data(o.ptr())).filter(|o| o.resource == raw as u64) else {
        return true;
    };
    out[3] = 1;
    out[7] = o.dropped_events;
    let Some(history) = &o.events else {
        return true;
    };
    out[6] = history.count as u64;
    let end = offset + bytes as u64;
    let newer = |out: &mut [u64; 32], at: usize, e: &CacheEventReceipt| {
        if e.event_sequence > out[at + 7] {
            out[at..at + 8].copy_from_slice(&[e.generation, e.source_resource, e.producer_list, e.offset, e.bytes, e.copy_sequence, e.reason, e.event_sequence]);
        }
    };
    for e in &history.events[..history.count] {
        let intersects = e.bytes != 0 && (e.offset > u64::MAX - e.bytes || (e.offset < end && offset < e.offset + e.bytes));
        if !intersects {
            continue;
        }
        if e.reason == reason::PUBLISHED_COPY {
            newer(out, 8, e);
        } else {
            newer(out, 16, e);
            if matches!(
                e.reason,
                reason::OWNER_CAPACITY_EVICTION
                    | reason::GLOBAL_BYTES_EVICTION
                    | reason::GLOBAL_PAYLOAD_EVICTION
                    | reason::RESOURCE_BYTES_EVICTION
                    | reason::GENERIC_RANGE_EVICTION
            ) {
                newer(out, 24, e);
            }
        }
    }
    true
}
/// QueryStructuredCoverage into the caller's (default) 16 values.
pub fn query_structured_coverage(raw: *mut c_void, offset: u64, bytes: usize, out: &mut [u64; 16]) -> bool {
    let Some((gpu, width)) = query_target(raw, offset, bytes, None) else {
        return false;
    };
    let owner = cache(raw, false);
    let mut g = lock();
    drain(&mut g);
    out[0] = STRUCTURED_COVERAGE_VERSION;
    out[1] = raw as u64;
    out[2] = g.generation;
    out[12] = offset;
    out[13] = bytes as u64;
    out[15] = offset;
    let marker = raw_hint(raw);
    if let Some(m) = marker {
        out[4] = m.generation;
        out[8] = m.offset;
        out[9] = m.bytes;
        out[5] = hint(raw, g.generation, m.offset, m.bytes, gpu, width).is_some() as u64;
    }
    let Some(o) = owner.as_ref().map(|o| data(o.ptr())).filter(|o| o.resource == raw as u64) else {
        return true;
    };
    out[3] = 1;
    out[6] = o.structured_hint.generation;
    out[7] = (out[5] != 0 && Some(o.structured_hint) == marker) as u64;
    out[10] = o.structured_bytes;
    out[11] = o.structured.len() as u64;
    let end = offset + bytes as u64;
    let mut at = o.structured.partition_point(|r| r.offset < offset);
    at = at.saturating_sub(1);
    let mut cursor = offset;
    let mut hole = false;
    for range in o.structured[at..].iter().take_while(|r| r.offset < end) {
        let range_end = range.offset + range.bytes as u64;
        if range_end <= offset {
            continue;
        }
        let (begin, finish) = (offset.max(range.offset), end.min(range_end));
        out[14] += finish - begin;
        if !hole && begin > cursor {
            out[15] = cursor;
            hole = true;
        }
        cursor = finish;
    }
    if !hole {
        out[15] = if cursor == end { u64::MAX } else { cursor };
    }
    true
}
/// PieceReceipt.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PieceReceipt {
    pub producer_list: u64,
    pub copy_sequence: u64,
    pub source_resource: u64,
    pub destination_offset: u64,
    pub source_offset: u64,
    pub copied_bytes: u64,
    pub piece_destination_offset: u64,
    pub piece_source_offset: u64,
    pub piece_bytes: u64,
}
/// A captured candidate: its payload, the offsets of the view in it and in the original copy, its pieces.
pub struct Captured {
    pub payload: Arc<Payload>,
    pub data_offset: usize,
    pub view_offset: usize,
    pub composed: bool,
    pub pieces: Vec<PieceReceipt>,
}
/// CaptureAtDraw: the one current copy covering the bytes.
pub fn capture_at_draw(raw: *mut c_void, address: u64, bytes: usize) -> Option<Captured> {
    let (gpu, width) = query_target(raw, 0, bytes, Some(address))?;
    let owner = cache(raw, false)?;
    let mut g = lock();
    drain(&mut g);
    let o = data(owner.ptr());
    if o.resource != raw as u64 {
        return None;
    }
    let offset = address - gpu;
    let value = o.ranges.iter().find(|v| {
        v.gpu_base == gpu && v.resource_width == width && offset >= v.destination_offset && span(offset - v.destination_offset, bytes as u64, v.bytes.len() as u64)
    })?;
    let at = (offset - value.destination_offset) as usize;
    let captured = Captured { payload: value.clone(), data_offset: at, view_offset: at, composed: false, pieces: Vec::new() };
    g.touch += 1;
    o.touch = g.touch;
    g.captures += 1;
    Some(captured)
}
/// CaptureStructuredAtDraw: the structured pieces covering the bytes inside the current marked view (composed into one
/// payload when several).
pub fn capture_structured_at_draw(raw: *mut c_void, address: u64, bytes: usize) -> Option<Captured> {
    let (gpu, width) = query_target(raw, 0, bytes, Some(address))?;
    let owner = cache(raw, false)?;
    let mut g = lock();
    drain(&mut g);
    let o = data(owner.ptr());
    let offset = address - gpu;
    if o.resource != raw as u64 || hint(raw, g.generation, offset, bytes as u64, gpu, width) != Some(o.structured_hint) {
        return None;
    }
    let end = offset + bytes as u64;
    let mut cursor = offset;
    let mut selected: Vec<usize> = Vec::new();
    let mut pieces: Vec<PieceReceipt> = Vec::new();
    for (index, range) in o.structured.iter().enumerate() {
        if range.offset + range.bytes as u64 <= cursor {
            continue;
        }
        if range.offset > cursor {
            break;
        }
        let value = &range.data;
        if value.gpu_base != gpu || value.resource_width != width || value.source_resource == 0 {
            return None;
        }
        let count = (range.offset + range.bytes as u64).min(end) - cursor;
        let piece = PieceReceipt {
            producer_list: value.producer_list,
            copy_sequence: value.sequence,
            source_resource: value.source_resource,
            destination_offset: value.destination_offset,
            source_offset: value.source_offset,
            copied_bytes: value.copied_bytes,
            piece_destination_offset: cursor,
            piece_source_offset: value.source_offset.wrapping_add(cursor.wrapping_sub(value.destination_offset)),
            piece_bytes: count,
        };
        match pieces.last_mut() {
            Some(last) if last.copy_sequence == piece.copy_sequence && last.piece_destination_offset + last.piece_bytes == cursor => last.piece_bytes += count,
            _ => pieces.push(piece),
        }
        selected.push(index);
        cursor += count;
        if cursor == end {
            break;
        }
    }
    if cursor != end || pieces.is_empty() {
        return None;
    }
    let captured = if pieces.len() == 1 {
        let range = &o.structured[selected[0]];
        Captured {
            payload: range.data.clone(),
            data_offset: range.data_offset + (offset - range.offset) as usize,
            view_offset: (offset - range.data.destination_offset) as usize,
            composed: false,
            pieces,
        }
    } else {
        let over = |o: &OwnerData| {
            o.budget.load(Relaxed) > MAX_RESOURCE_BYTES.wrapping_sub(bytes) || PAYLOAD_BYTES.load(Relaxed) > MAX_GLOBAL_BYTES.wrapping_sub(bytes)
        };
        if over(o) {
            compact_structured(o);
        }
        if over(o) || PAYLOAD_COUNT.load(Relaxed) >= MAX_PAYLOADS {
            return None;
        }
        let mut value = Payload::default();
        value.bytes = Vec::with_capacity(bytes);
        let mut cursor = offset;
        for &index in &selected {
            let range = &o.structured[index];
            let at = range.data_offset + (cursor - range.offset) as usize;
            let count = ((range.offset + range.bytes as u64).min(end) - cursor) as usize;
            value.bytes.extend_from_slice(&range.data.bytes[at..at + count]);
            cursor += count as u64;
        }
        account(&mut value, &o.budget);
        Captured { payload: Arc::new(value), data_offset: 0, view_offset: 0, composed: true, pieces }
    };
    g.touch += 1;
    o.touch = g.touch;
    g.captures += 1;
    Some(captured)
}
/// Clear: a new generation; every owner emptied and unregistered.
pub fn clear() {
    let mut g = lock();
    drain(&mut g);
    g.generation += 1;
    let owners = core::mem::take(&mut g.owners);
    for owner in owners {
        let o = data(owner);
        empty(&mut g, o, operation::CLEAR, true);
        o.registered = false;
        release_events(o);
    }
}
/// GetStatistics: the 12 values of Statistics.
pub fn statistics() -> [u64; 12] {
    let mut g = lock();
    drain(&mut g);
    [
        g.current_bytes,
        g.ranges,
        PAYLOAD_BYTES.load(Relaxed) as u64,
        PAYLOAD_COUNT.load(Relaxed) as u64,
        g.owners.len() as u64,
        g.sequence,
        g.evictions,
        g.captures,
        g.rejected,
        g.generation,
        g.structured_bytes,
        g.structured_ranges,
    ]
}

// ---- the C ABI

/// A payload handle's view for the .cpp's Candidate.
#[repr(C)]
pub struct PayloadView {
    pub bytes: *const u8,
    pub length: usize,
    pub producer_list: u64,
    pub sequence: u64,
    pub source_resource: u64,
    pub destination_offset: u64,
    pub source_offset: u64,
    pub copied_bytes: u64,
}
/// A capture's answer.
#[repr(C)]
pub struct CaptureOut {
    pub payload: *const Payload,
    pub view: PayloadView,
    pub data_offset: usize,
    pub view_offset: usize,
    pub composed: bool,
}
fn enabled_ref<'a>(enabled: *const AtomicU32) -> Option<&'a AtomicU32> {
    // SAFETY: the caller's live flag, or null.
    unsafe { enabled.as_ref() }
}
/// # Safety
/// The resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_invalidate(resource: *mut c_void) {
    invalidate(resource);
}
/// # Safety
/// The resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_invalidate_range(resource: *mut c_void, offset: u64, bytes: u64) {
    invalidate_range(resource, offset, bytes);
}
/// # Safety
/// `bytes` (when non-null) holds `count` bytes; the objects are live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_record_intended_copy(list: *mut c_void, resource: *mut c_void, offset: u64, source_offset: u64, bytes: *const u8, count: usize) -> bool {
    let sample = (!bytes.is_null()).then(|| unsafe { core::slice::from_raw_parts(bytes, count) });
    record_intended_copy(list, resource, offset, source_offset, sample)
}
/// # Safety
/// The view and the flag are valid or null; the resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_mark_structured_view(resource: *mut c_void, view: *const D3D12_SHADER_RESOURCE_VIEW_DESC, enabled: *const AtomicU32) -> bool {
    mark_structured_view(resource, unsafe { view.as_ref() }, enabled_ref(enabled))
}
/// # Safety
/// The flag is valid or null; the objects are live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_observe_copy(
    list: *mut c_void,
    resource: *mut c_void,
    offset: u64,
    source: *mut c_void,
    source_offset: u64,
    bytes: u64,
    enabled: *const AtomicU32,
) -> bool {
    observe_copy(list, resource, offset, source, source_offset, bytes, enabled_ref(enabled))
}
/// # Safety
/// `output` has room for 24 values; the resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_query_diagnostic(resource: *mut c_void, mark: bool, output: *mut [u64; 24]) -> bool {
    match query_diagnostic(resource, mark) {
        Some(value) => {
            unsafe { *output = value };
            true
        }
        None => false,
    }
}
/// # Safety
/// `output` holds the caller's default values (24 / 32 / 16 for `which` 0 / 1 / 2); the resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_query_coverage(which: u32, resource: *mut c_void, offset: u64, bytes: usize, output: *mut u64) -> bool {
    match which {
        0 => query_generic_coverage(resource, offset, bytes, unsafe { &mut *output.cast::<[u64; 24]>() }),
        1 => query_cache_events(resource, offset, bytes, unsafe { &mut *output.cast::<[u64; 32]>() }),
        _ => query_structured_coverage(resource, offset, bytes, unsafe { &mut *output.cast::<[u64; 16]>() }),
    }
}
/// CaptureAtDraw (`structured` false) / CaptureStructuredAtDraw: the payload handle (one reference for the caller) and
/// the pieces through `append`.
///
/// # Safety
/// `output` is writable; `append` accepts `pieces` and the receipts; the resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_capture(
    structured: bool,
    resource: *mut c_void,
    address: u64,
    bytes: usize,
    output: *mut CaptureOut,
    pieces: *mut c_void,
    append: unsafe extern "C-unwind" fn(*mut c_void, *const PieceReceipt, usize),
) -> bool {
    let captured = if structured { capture_structured_at_draw(resource, address, bytes) } else { capture_at_draw(resource, address, bytes) };
    let Some(captured) = captured else {
        return false;
    };
    unsafe {
        append(pieces, captured.pieces.as_ptr(), captured.pieces.len());
        let view = payload_view(&captured.payload);
        *output = CaptureOut {
            payload: Arc::into_raw(captured.payload),
            view,
            data_offset: captured.data_offset,
            view_offset: captured.view_offset,
            composed: captured.composed,
        };
    }
    true
}
fn payload_view(p: &Payload) -> PayloadView {
    PayloadView {
        bytes: p.bytes.as_ptr(),
        length: p.bytes.len(),
        producer_list: p.producer_list,
        sequence: p.sequence,
        source_resource: p.source_resource,
        destination_offset: p.destination_offset,
        source_offset: p.source_offset,
        copied_bytes: p.copied_bytes,
    }
}
/// Releases a payload handle from cvr_sri_capture.
///
/// # Safety
/// `payload` is such a handle (or null), released once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_payload_release(payload: *const Payload) {
    if !payload.is_null() {
        drop(unsafe { Arc::from_raw(payload) });
    }
}
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_clear() {
    clear();
}
/// # Safety
/// `output` has room for 12 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sri_statistics(output: *mut [u64; 12]) {
    unsafe { *output = statistics() };
}

#[cfg(test)]
mod tests {
    //! The cache rules reachable without a device (the D3D12 paths run in the single-pass tests' Rust variants).
    use super::*;

    fn payload(bytes: usize, sequence: u64) -> Arc<Payload> {
        let mut p = Payload::default();
        p.bytes = vec![7; bytes];
        p.sequence = sequence;
        Arc::new(p)
    }
    fn global() -> Global {
        Global {
            owners: Vec::new(),
            current_bytes: 0,
            ranges: 0,
            sequence: 0,
            touch: 0,
            evictions: 0,
            captures: 0,
            rejected: 0,
            diagnostic_sequence: 0,
            generation: 1,
            structured_bytes: 0,
            structured_ranges: 0,
            event_sequence: 0,
        }
    }

    #[test]
    fn empty_releases_structured_capacity() {
        // Clear empties every owner: its structured metadata allocation is released, not just its length.
        let mut g = global();
        let mut o = OwnerData::default();
        let data = payload(1024 * 1024, 1);
        for i in 0..1025u64 {
            o.structured.push(StructuredRange { data: data.clone(), offset: i * 256, data_offset: (i * 256) as usize, bytes: 255 });
        }
        o.structured_bytes = 1025 * 255;
        g.structured_bytes = o.structured_bytes;
        g.structured_ranges = 1025;
        o.ranges.push(data.clone());
        g.current_bytes = data.bytes.len() as u64;
        g.ranges = 1;
        empty(&mut g, &mut o, operation::CLEAR, true);
        assert_eq!(o.structured.capacity(), 0);
        assert!(o.ranges.is_empty() && o.structured_bytes == 0 && o.structured_hint == StructuredHint::default());
        assert!(g.structured_bytes == 0 && g.structured_ranges == 0 && g.current_bytes == 0 && g.ranges == 0);
        assert_eq!(Arc::strong_count(&data), 1, "the owner holds no payload reference");
    }
    #[test]
    fn structured_overlap_splits_ranges() {
        let mut g = global();
        let mut o = OwnerData::default();
        let data = payload(1024, 1);
        o.structured.push(StructuredRange { data: data.clone(), offset: 0, data_offset: 0, bytes: 1024 });
        o.structured_bytes = 1024;
        g.structured_bytes = 1024;
        g.structured_ranges = 1;
        erase_structured_overlap(&mut g, &mut o, 256, 1);
        let shape: Vec<_> = o.structured.iter().map(|r| (r.offset, r.data_offset, r.bytes)).collect();
        assert_eq!(shape, [(0, 0, 256), (257, 257, 767)]);
        assert!(o.structured_bytes == 1023 && g.structured_bytes == 1023 && g.structured_ranges == 2);
        erase_structured_overlap(&mut g, &mut o, 0, 0);
        assert!(o.structured.is_empty() && g.structured_bytes == 0 && g.structured_ranges == 0, "an unbounded write drops everything");
    }
}