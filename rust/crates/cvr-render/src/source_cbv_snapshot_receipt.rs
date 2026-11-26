//! THE SOURCE CBV DETAILS AND SNAPSHOTS AT THE ACTUAL CALL (formerly src/Render/NativeSourceCbvDetails.cpp's
//! CaptureAtSource and src/Render/NativeSourceCbvSnapshot.cpp): a held source CBV's current metadata read between two
//! identical reads at the source (its buffer's shape, the recording context, the native record, the state at the draw,
//! the allocation behind it -- direct, aliased or a pool's); up to 28 such sources copied at the actual call into a
//! private arena (one default-heap buffer per list generation, finalized to GENERIC_READ); the receipt that pins each
//! source's allocation; the pool sources' ranges registered through buffer_read.rs; the tracked sources' native
//! combined-read registration (0x337DA0, then the flush 0x3E3BB4) with its recovery; and the proofs the copies validate
//! at the call, for a recorded private CBV and at submission. The rules are source_cbv_snapshot.rs' and
//! source_cbv_details.rs'; the source is read again through source_cbv_observe.rs.
//!
//! The C++ APIs (include/Render/NativeSourceCbvDetails.hpp's CaptureAtSource, include/Render/NativeSourceCbvSnapshot.hpp)
//! forward here; the graph parent's revalidation (still C++, NativePreparation.cpp) is the attached host's.
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::{Acquire, Relaxed, Release, SeqCst}};
use std::sync::Mutex;

use windows::core::{IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::buffer_read::{Lease as BufferLease, Proof};
use crate::ia_binding::cvr_iab_read;
use crate::indirect_copy::{GraphParentRecord, ScopeLedger, ScopeStamp};
use crate::native_recording::{Context as UploadContext, Lease as Recording};
use crate::source_cbv_details::{self as scd, Alias, Allocation, Allocator, Context, Pool, ReadState, Record, KNOWN, MISMATCH, NOT_APPLICABLE, UNREADABLE};
use crate::source_cbv_observe::Backing;
use crate::source_cbv_snapshot::{self as rules, Admission, PreparedCopies, PrivateCbv, ReadEpochCertificate, SourceCbvBacking};
use crate::structured_snapshot_receipt::{add_ref, query_creation, release, transition, validate_recording, Creation, OwnedRecording};

#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
use crate::declared_moved_exports::{CyberpunkVR_SourceCbvSnapshotsEnable, CyberpunkVR_SourceCbvSnapshotsUploadEnable};
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
use crate::used_moved_exports_rest::CyberpunkVR_SourceCbvSnapshotsPoolFailure;
/// The unit tests' switches and pool failure row (the plugin's and the probe tests' are the moved exports').
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static CyberpunkVR_SourceCbvSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static CyberpunkVR_SourceCbvSnapshotsUploadEnable: AtomicU32 = AtomicU32::new(0);
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
#[allow(non_upper_case_globals)]
static mut CyberpunkVR_SourceCbvSnapshotsPoolFailure: [u64; 16] = [0; 16];

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool;
}

fn thread() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() }
}
fn image() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn mem() -> impl Fn(u64, &mut [u8]) -> bool {
    scd::reader(Some(cvr_iab_read), core::ptr::null_mut())
}
fn field_u64(raw: &[u8; 0xB0], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}

// ---- the source CBV backing

/// source_cbv::SourceCbvBacking with its resource, origin owner and three leases' tokens owned.
struct OwnedCbv(Box<Backing>);
impl OwnedCbv {
    fn empty() -> Self {
        // SAFETY: plain data (null references, zero values) -- the C++ value-initialised backing.
        OwnedCbv(Box::new(unsafe { core::mem::zeroed() }))
    }
    fn copy(value: &Backing) -> Self {
        add_ref(value.resource);
        add_ref(value.origin_owner);
        add_ref(value.recording.token);
        add_ref(value.creation_recording.token);
        add_ref(value.writer_recording.token);
        // SAFETY: a bitwise copy whose five references were taken above.
        OwnedCbv(Box::new(unsafe { core::ptr::read(value) }))
    }
}
impl Drop for OwnedCbv {
    fn drop(&mut self) {
        let b = &self.0;
        release(b.writer_recording.token);
        release(b.creation_recording.token);
        release(b.recording.token);
        release(b.origin_owner);
        release(b.resource);
    }
}
/// The rules' view of a backing (the COM pointers as addresses; the same layout).
fn rules_of(value: &Backing) -> &SourceCbvBacking {
    // SAFETY: source_cbv_snapshot's SourceCbvBacking mirrors the same 472 bytes.
    unsafe { &*(value as *const Backing).cast::<SourceCbvBacking>() }
}
fn same_lease(a: &Recording, b: &Recording) -> bool {
    a.token == b.token && a.generation == b.generation && a.context == b.context && a.list == b.list && a.registry == b.registry
}
/// detail::SameSource: the backings alike in every identity field.
fn same_source(a: &Backing, b: &Backing) -> bool {
    a.resource == b.resource
        && a.origin_owner == b.origin_owner
        && a.stamp.gpu == b.stamp.gpu
        && a.stamp.heap == b.stamp.heap
        && a.stamp.value == b.stamp.value
        && same_lease(&a.recording, &b.recording)
        && crate::source_cbv_binding::same_buffer(&a.native, &b.native)
        && a.cookie == b.cookie
        && a.origin == b.origin
        && a.gpu == b.gpu
        && a.backing_bytes == b.backing_bytes
        && a.source_offset == b.source_offset
        && a.serial == b.serial
        && a.cpu_descriptor == b.cpu_descriptor
        && a.descriptor_bytes == b.descriptor_bytes
        && a.logical_bytes == b.logical_bytes
        && a.stage == b.stage
        && a.shader_register == b.shader_register
        && a.creator_rva == b.creator_rva
        && a.reserved_bytes == b.reserved_bytes
        && a.completed_producer_rva == b.completed_producer_rva
        && same_lease(&a.creation_recording, &b.creation_recording)
        && a.writer_nonce == b.writer_nonce
        && a.writer_bytes == b.writer_bytes
        && a.writer_destination == b.writer_destination
        && same_lease(&a.writer_recording, &b.writer_recording)
}
/// source_cbv::ReadAtSource into the empty `fresh`, registered or created, the same source.
fn current_source(list: *mut c_void, root: u64, parameter: u32, offset: u32, saved: &Backing, fresh: &mut OwnedCbv) -> bool {
    // SAFETY: the saved backing's stamp and origin; `fresh` is an empty backing this module owns.
    let status = unsafe {
        crate::source_cbv_observe::cvr_scbo_read_at_source(list as u64, root, parameter, offset, &saved.stamp, saved.origin_owner, &mut *fresh.0)
    };
    (status == 1 || status == 3) && same_source(saved, &fresh.0)
}
fn read_state(list: *mut c_void, lease: &BufferLease) -> ReadState {
    let mut out = ReadState::default();
    // SAFETY: a live list; the lease is laid out as ia_binding's OwnedLease (both geometry::BufferLease).
    unsafe { crate::ia_binding::cvr_iab_read_state(list, (lease as *const BufferLease).cast(), &mut out) };
    out
}

// ---- source_cbv_details::Detail and CaptureAtSource

/// source_cbv_details::Resource.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResourceShape {
    pub observation: u32,
    pub heap: D3D12_HEAP_PROPERTIES,
    pub heap_flags: D3D12_HEAP_FLAGS,
    pub flags: D3D12_RESOURCE_FLAGS,
    pub width: u64,
    pub gpu: u64,
    pub com_offset: u64,
}
/// source_cbv_details::Detail.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Detail {
    pub status: u32,
    pub resource: ResourceShape,
    pub context: Context,
    pub record: Record,
    pub state: ReadState,
    pub state_queried: bool,
    pub state_value_known: bool,
    pub allocation_path: u32,
    pub alias: Alias,
    pub pool: Pool,
    pub allocation: Allocation,
    pub allocator: Allocator,
}
const _: () = assert!(
    core::mem::size_of::<ResourceShape>() == 56
        && core::mem::size_of::<Detail>() == 760
        && core::mem::offset_of!(Detail, record) == 112
        && core::mem::offset_of!(Detail, state) == 304
        && core::mem::offset_of!(Detail, alias) == 328
        && core::mem::offset_of!(Detail, allocator) == 696
);
/// source_cbv_details::Status.
mod detail_status {
    pub const CAPTURED: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const SOURCE_BEFORE: u32 = 2;
    pub const RESOURCE: u32 = 3;
    pub const CONTEXT: u32 = 4;
    pub const RECORD: u32 = 5;
    pub const SOURCE_AFTER: u32 = 6;
    pub const CHANGED: u32 = 7;
    pub const EXCEPTION: u32 = 8;
}
/// source_cbv_details::AllocationPath.
mod path {
    pub const DIRECT: u32 = 1;
    pub const ALIAS: u32 = 2;
    pub const POOL: u32 = 3;
    pub const ARENA: u32 = 4;
}
impl Detail {
    fn new() -> Self {
        // SAFETY: plain data; the C++ value-initialised detail but for its status (Arguments).
        let mut d: Detail = unsafe { core::mem::zeroed() };
        d.status = detail_status::ARGUMENTS;
        d
    }
}
fn observe_allocation(base: u64, source: &Backing, out: &mut Detail) {
    if source.native.handle == 0 || source.native.kind == 4 {
        out.allocation_path = path::ARENA;
        out.alias.observation = NOT_APPLICABLE;
        out.pool.observation = NOT_APPLICABLE;
        out.allocation.observation = NOT_APPLICABLE;
        out.allocator.observation = NOT_APPLICABLE;
        return;
    }
    if out.record.observation != KNOWN {
        return;
    }
    let raw = out.record.bytes;
    let (direct, payload, control) = (field_u64(&raw, 0x18), field_u64(&raw, 0x20), field_u64(&raw, 0x28));
    let read = mem();
    out.pool = scd::read_pool(&read, base, &source.native, &out.record, source.gpu, source.backing_bytes);
    let mut owner = 0u64;
    if out.pool.observation == KNOWN {
        out.allocation_path = path::POOL;
        owner = out.pool.owner;
    } else if direct != 0 && payload == 0 && control == 0 {
        out.allocation_path = path::DIRECT;
        owner = direct;
    } else if direct == 0 {
        out.alias = scd::read_alias(&read, payload, control);
        if out.alias.observation == KNOWN {
            out.allocation_path = path::ALIAS;
            owner = out.alias.allocation;
        }
    } else {
        out.allocation.observation = MISMATCH;
    }
    if owner != 0 {
        out.allocation = scd::read_allocation(&read, base, owner);
        if out.allocation.observation == KNOWN {
            let alias_bytes = out.alias.bytes;
            let a = &mut out.allocation;
            a.extent_covers = a.bytes >= source.backing_bytes;
            if out.allocation_path == path::ALIAS {
                a.alias_shape_known = true;
                a.alias_shape_matches = a.resource == 0 && a.heap != 0 && (a.kind == 1 || a.kind == 2) && a.bytes == alias_bytes;
            } else {
                a.association_known = true;
                a.resource_matches = a.resource == source.resource as u64;
            }
        }
    }
    out.allocator = scd::read_allocator(&read, base, source.native.registry, &out.allocation);
}
fn capture_at_source(list: *mut c_void, root: u64, parameter: u32, offset: u32, source: &Backing, output: &mut Detail) -> bool {
    *output = Detail::new();
    if list.is_null() || root == 0 || source.resource.is_null() || source.recording.token.is_null() || source.recording.context == 0 {
        return false;
    }
    output.status = detail_status::SOURCE_BEFORE;
    let (mut before, mut after) = (OwnedCbv::empty(), OwnedCbv::empty());
    if !current_source(list, root, parameter, offset, source, &mut before) {
        return false;
    }
    output.status = detail_status::RESOURCE;
    let raw = source.resource;
    // SAFETY: the backing's owned resource.
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return false;
    };
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    {
        let r = &mut output.resource;
        r.width = desc.Width;
        r.flags = desc.Flags;
        // SAFETY: as above.
        r.gpu = unsafe { resource.GetGPUVirtualAddress() };
        r.com_offset = source.source_offset;
        // SAFETY: as above; the outputs are the detail's.
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || unsafe { resource.GetHeapProperties(Some(&mut r.heap), Some(&mut r.heap_flags)) }.is_err() {
            r.observation = UNREADABLE;
            return false;
        }
        if r.gpu != source.gpu
            || r.width != source.backing_bytes
            || !crate::source_cbv_binding::descriptor(r.gpu, r.width, source.stamp.value[4], source.descriptor_bytes)
            || source.stamp.value[4].wrapping_sub(r.gpu) != r.com_offset
        {
            r.observation = MISMATCH;
            return false;
        }
        r.observation = KNOWN;
    }
    output.status = detail_status::CONTEXT;
    let read = mem();
    output.context = scd::read_context(&read, source.recording.context, list as u64);
    if output.context.observation != KNOWN {
        return false;
    }
    output.status = detail_status::RECORD;
    output.record = scd::read_record(&read, &source.native);
    if source.native.handle != 0 && output.record.observation != KNOWN {
        return false;
    }
    let lease = BufferLease {
        resource: raw,
        gpu: output.resource.gpu,
        bytes: output.resource.width,
        heap_type: output.resource.heap.Type.0 as u32,
        flags: output.resource.flags.0 as u32,
        native: source.native,
    };
    output.state = read_state(list, &lease);
    output.state_queried = true;
    output.state_value_known = scd::state_value_known(output.state);
    observe_allocation(image(), source, output);
    output.status = detail_status::SOURCE_AFTER;
    if !current_source(list, root, parameter, offset, source, &mut after) {
        return false;
    }
    output.status = detail_status::CHANGED;
    let context = scd::read_context(&read, source.recording.context, list as u64);
    if context != output.context || !same_source(&before.0, &after.0) {
        return false;
    }
    if source.native.handle != 0 {
        let again = scd::read_record(&read, &source.native);
        if again.observation != KNOWN || again.address != output.record.address || again.bytes != output.record.bytes {
            return false;
        }
    }
    output.status = detail_status::CAPTURED;
    true
}
/// CaptureAtSource: `*output` the source CBV's current metadata (its status says how far it got).
///
/// # Safety
/// `source` is valid; `output` is writable; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_capture_at_source(
    list: *mut c_void,
    root: u64,
    parameter: u32,
    offset: u32,
    source: *const Backing,
    output: *mut Detail,
) -> bool {
    // SAFETY: as documented.
    let (source, output) = unsafe { (&*source, &mut *output) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture_at_source(list, root, parameter, offset, source, output))) {
        Ok(done) => done,
        Err(_) => {
            output.status = detail_status::EXCEPTION;
            false
        }
    }
}

// ---- the snapshots

/// source_cbv_snapshot::Failure.
mod failure {
    pub const NONE: u32 = 0;
    pub const DISABLED: u32 = 1;
    pub const ARGUMENTS: u32 = 2;
    pub const SCOPE: u32 = 3;
    pub const CODE: u32 = 4;
    pub const PARENT: u32 = 5;
    pub const SOURCE: u32 = 6;
    pub const CREATION: u32 = 7;
    pub const CLASS: u32 = 8;
    pub const ALLOCATION: u32 = 10;
    pub const POOL: u32 = 11;
    pub const CAPACITY: u32 = 13;
    pub const CHANGED: u32 = 14;
    pub const RETENTION: u32 = 15;
    pub const REGISTRATION: u32 = 16;
    pub const RECOVERY_VERIFIED: u32 = 17;
    pub const RECOVERY_FATAL: u32 = 18;
    pub const ALREADY_RECORDED: u32 = 19;
    pub const EXCEPTION: u32 = 20;
}
mod stage {
    pub const PREPARED: u32 = 0;
    pub const RECORDING: u32 = 1;
    pub const RECORDED: u32 = 2;
    pub const FAILED: u32 = 3;
}
/// detail::Kind.
mod kind {
    pub const UPLOAD: u32 = 1;
    pub const POOL: u32 = 2;
    pub const TRACKED: u32 = 3;
}
const MAX_REQUESTS: usize = 28;
const MAX_ARENA_BYTES: u64 = 64 * 1024 * 1024;
const GENERIC_READ: u32 = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
const COPY_SOURCE: u32 = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
const RECEIPT_TYPE: GUID = GUID::from_values(0xe1992d61, 0x199b, 0x47d3, [0x82, 0x9c, 0x41, 0x8b, 0x79, 0x30, 0x01, 0xb2]);
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
    CyberpunkVR_SourceCbvSnapshotsEnable.load(Relaxed) != 0
}
fn upload_enabled() -> bool {
    CyberpunkVR_SourceCbvSnapshotsUploadEnable.load(Relaxed) != 0
}
fn scope_matches(stamp: &ScopeStamp, list: u64, generation: u64, position: u64) -> bool {
    SCOPES.with_borrow(|s| s.matches(stamp, list, generation, position, thread()))
}

/// The upload code contract: (attempted, valid, base).
static UPLOAD_CODE: Mutex<(bool, bool, u64)> = Mutex::new((false, false, 0));
/// PrepareUploadCodeGuards: the five upload paths' code at their RVAs hashes to the recorded fingerprints (once).
#[no_mangle]
pub extern "C-unwind" fn cvr_scsn_prepare_upload_code_guards(base: u64) {
    let mut code = UPLOAD_CODE.lock().unwrap_or_else(|e| e.into_inner());
    if code.0 {
        return;
    }
    code.0 = true;
    code.2 = base;
    const CONTRACT: [(u64, usize, u64); 5] = [
        (0x3E9860, 185, 0x0990890D546C54BD),
        (0x3E96EC, 82, 0x792C0A460B932B2E),
        (0x3E7D34, 106, 0x0A0DB7DBCE20E1CD),
        (0x3E6CD4, 38, 0xB56EF3B145CCE822),
        (0x5B4028, 229, 0x4BC3EFD38EC58363),
    ];
    let mut body = [0u8; 256];
    let mut valid = base != 0 && base == image();
    for &(rva, bytes, hash) in &CONTRACT {
        // SAFETY: the reader writes at most `bytes` bytes into the local buffer.
        if !valid
            || base > u64::MAX - rva - bytes as u64
            || bytes > body.len()
            || !unsafe { cvr_iab_read(core::ptr::null_mut(), base + rva, body.as_mut_ptr().cast(), bytes) }
        {
            valid = false;
            break;
        }
        if crate::native_recording::fingerprint(&body[..bytes]) != hash {
            valid = false;
            break;
        }
    }
    code.1 = valid;
}
/// UploadCompatible.
#[no_mangle]
pub extern "C-unwind" fn cvr_scsn_upload_compatible() -> bool {
    let code = UPLOAD_CODE.lock().unwrap_or_else(|e| e.into_inner());
    code.1 && code.2 == image()
}

/// source_cbv_snapshot::Request (the source's references owned by whoever holds it).
#[repr(C)]
pub struct Request {
    pub source: Backing,
    pub parameter: u32,
    pub offset: u32,
    pub reflected_bytes: u32,
}
const _: () = assert!(core::mem::size_of::<Request>() == 488);
/// preparation::RevalidateGraphParent (preparation_trace.rs).
fn revalidate(parent: &GraphParentRecord) -> bool {
    crate::preparation_trace::revalidate_graph_parent(parent)
}

/// A private CBV with its resource and owner owned.
struct OwnedView(PrivateCbv);
impl OwnedView {
    fn duplicate(&self) -> PrivateCbv {
        add_ref(self.0.resource as *mut c_void);
        add_ref(self.0.owner as *mut c_void);
        self.0
    }
}
impl Drop for OwnedView {
    fn drop(&mut self) {
        release(self.0.owner as *mut c_void);
        release(self.0.resource as *mut c_void);
    }
}
/// A buffer lease with its resource owned.
struct OwnedLease(BufferLease);
impl Drop for OwnedLease {
    fn drop(&mut self) {
        release(self.0.resource);
    }
}
/// The allocator and allocation pinned (the allocation released first).
struct Pins {
    allocator: *mut c_void,
    allocation: *mut c_void,
}
impl Drop for Pins {
    fn drop(&mut self) {
        release(self.allocation);
        release(self.allocator);
    }
}
/// One requested source (the lease, the pins, the creation and the request released in that order).
struct Entry {
    lease: OwnedLease,
    pins: Pins,
    creation: Creation,
    source: OwnedCbv,
    parameter: u32,
    offset: u32,
    reflected_bytes: u32,
    observed: Detail,
    kind: u32,
    range: Proof,
    destination: u64,
    before_state: u32,
    required_state: u32,
    mutated: bool,
}
fn pool_failure(phase: u32, entry: &Entry, current: Option<&Proof>) {
    let mut details = [0u64; 4];
    // SAFETY: a local output.
    unsafe { crate::buffer_read::cvr_buffer_read_last_failure_details(&mut details) };
    let s = &entry.source.0;
    let row = [
        phase as u64,
        entry.parameter as u64,
        entry.offset as u64,
        entry.reflected_bytes as u64,
        s.source_offset,
        s.native.handle as u64,
        crate::buffer_read::cvr_buffer_read_last_failure() as u64,
        details[0],
        details[1],
        details[2],
        details[3],
        entry.range.epoch,
        current.map_or(0, |c| c.epoch),
        entry.range.recording,
        current.map_or(0, |c| c.recording),
        s.resource as u64,
    ];
    // SAFETY: the export's sixteen words (written by the recording thread, read by the tools).
    unsafe { core::ptr::addr_of_mut!(CyberpunkVR_SourceCbvSnapshotsPoolFailure).write(row) };
}
fn same_allocation(a: &Allocation, b: &Allocation) -> bool {
    a.observation == KNOWN
        && b.observation == KNOWN
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.bytes == b.bytes
        && a.resource == b.resource
        && a.heap == b.heap
        && a.block == b.block
        && a.handle == b.handle
        && a.kind == b.kind
}
fn same_allocator(a: &Allocator, b: &Allocator) -> bool {
    a.observation == KNOWN && b.observation == KNOWN && a.address == b.address && a.vtable == b.vtable && a.pimpl == b.pimpl && b.association_known && b.pimpl_matches
}
fn same_pool(a: &Pool, b: &Pool) -> bool {
    a.observation == KNOWN
        && b.observation == KNOWN
        && a.entry == b.entry
        && a.resource == b.resource
        && a.owner == b.owner
        && a.unit == b.unit
        && a.packed == b.packed
        && a.group == b.group
        && a.slot == b.slot
        && a.occupied
        && b.occupied
}
fn rules_state(state: ReadState) -> crate::indirect_copy::ReadState {
    crate::indirect_copy::ReadState { value: state.value, status: state.status, kind: state.kind }
}
fn observe(source: &Backing, reflected_bytes: u32, d: &Detail, c: &Creation) -> Admission {
    let c = &c.0;
    Admission {
        resource: source.resource as u64,
        gpu: d.resource.gpu,
        width: d.resource.width,
        com_offset: source.source_offset,
        native_gpu: source.native.gpu,
        native_bytes: source.native.bytes,
        reflected_bytes,
        view_bytes: source.descriptor_bytes,
        heap: d.resource.heap.Type.0 as u32,
        flags: d.resource.flags.0 as u32,
        native_kind: source.native.kind,
        handle: source.native.handle,
        creator: source.creator_rva,
        creation_kind: c.kind,
        initial_known: c.initial_state_known,
        initial_state: c.initial_state,
        creation_heap: c.heap as u64,
        creation_bytes: c.allocation_bytes,
        creation_offset: c.heap_offset,
        creation_known: c.resource == source.resource && (c.kind == 1 || c.kind == 2),
        source_current: d.status == detail_status::CAPTURED,
        detail_known: d.resource.observation == KNOWN && d.context.observation == KNOWN && d.context.pending == 0 && d.context.submitted == 0,
        state: rules_state(d.state),
        state_known: d.state_queried && d.state_value_known,
        pool_known: d.pool.observation == KNOWN && d.pool.unit == 512 && d.pool.occupied,
        allocation_known: d.allocation.observation == KNOWN && d.allocation.association_known && d.allocation.resource_matches && d.allocation.extent_covers,
        allocator_known: d.allocator.observation == KNOWN && d.allocator.association_known && d.allocator.pimpl_matches,
        allocation: d.allocation.address,
        allocation_resource: d.allocation.resource,
        allocation_bytes: d.allocation.bytes,
        allocation_heap: d.allocation.heap,
        allocation_kind: d.allocation.kind,
        allocation_pimpl: d.allocation.pimpl,
        allocator_pimpl: d.allocator.pimpl,
        direct: d.allocation_path == path::DIRECT,
        alias_fields_empty: d.record.observation == KNOWN && field_u64(&d.record.bytes, 0x20) == 0 && field_u64(&d.record.bytes, 0x28) == 0,
    }
}
fn physical(entry: &Entry, base: u64) -> bool {
    let Some(current) = query_creation(entry.source.0.resource) else {
        return false;
    };
    if !crate::ia_snapshot_same::same_creation(&entry.creation.0, &current.0) {
        return false;
    }
    if entry.kind == kind::UPLOAD {
        return entry.pins.allocation.is_null() && entry.pins.allocator.is_null();
    }
    let read = mem();
    let allocation = scd::read_allocation(&read, base, entry.observed.allocation.address);
    let allocator = scd::read_allocator(&read, base, entry.source.0.native.registry, &allocation);
    !entry.pins.allocation.is_null()
        && !entry.pins.allocator.is_null()
        && entry.pins.allocation as u64 == allocation.address
        && entry.pins.allocator as u64 == allocator.address
        && same_allocation(&entry.observed.allocation, &allocation)
        && same_allocator(&entry.observed.allocator, &allocator)
}
fn inline_upload(source: &Backing, epoch: &ReadEpochCertificate, bytes: u32) -> bool {
    // SAFETY: plain values.
    unsafe { rules::cvr_scs_inline_upload(2, rules_of(source), epoch, bytes) }
}

// ---- the arena

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
    recording: OwnedRecording,
    state: u64,
    pending: AtomicU32,
    finalized: AtomicBool,
    failed: AtomicBool,
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
/// SnapshotArena::Create: S_OK with `*out` a new storage reference.
///
/// # Safety
/// `existing` is the arena's storage or null; `device` and `list` are live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_arena_create(
    existing: *mut c_void,
    device: *mut c_void,
    list: *mut c_void,
    generation: u64,
    capacity: u64,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the caller's output.
    unsafe { *out = core::ptr::null_mut() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena_create(existing, device, list, generation, capacity))) {
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
    // SAFETY: live objects or null.
    let (Some(d), Some(l)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) })
    else {
        return Err(E_INVALIDARG);
    };
    // SAFETY: a live list.
    if !existing.is_null()
        || generation == 0
        || capacity == 0
        || capacity > MAX_ARENA_BYTES
        || capacity & 255 != 0
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
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
        gpu: 0,
        capacity,
        cursor: 0,
        recording: OwnedRecording::none(),
        state: 0,
        pending: AtomicU32::new(0),
        finalized: AtomicBool::new(false),
        failed: AtomicBool::new(false),
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
    s.gpu = unsafe { resource.GetGPUVirtualAddress() };
    s.resource = resource.into_raw();
    if s.gpu == 0 || s.gpu > u64::MAX - capacity {
        return fail(E_FAIL);
    }
    Ok(raw)
}
/// SnapshotArena::Finalized.
///
/// # Safety
/// `raw` is the arena's storage or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_arena_finalized(raw: *mut c_void) -> bool {
    storage(raw).is_some_and(|s| s.finalized.load(Acquire) && !s.failed.load(Acquire))
}
/// SnapshotArena::Bytes.
///
/// # Safety
/// As cvr_scsn_arena_finalized.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_arena_bytes(raw: *mut c_void) -> u64 {
    storage(raw).map_or(0, |s| s.cursor)
}
/// SnapshotArena::Finalize: the copies recorded, the arena turned GENERIC_READ.
///
/// # Safety
/// As cvr_scsn_arena_finalized; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_arena_finalize(raw: *mut c_void, list: *mut c_void) -> bool {
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let Some(s) = storage(raw) else {
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
        let context = UploadContext { context: s.recording.0.context, state: s.state, list: s.list };
        set_failure(failure::SOURCE);
        if !validate_recording(&s.recording.0, list, &context) {
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
    views: Vec<OwnedView>,
    entries: Vec<Entry>,
    parent: GraphParentRecord,
    epoch: ReadEpochCertificate,
    scope: ScopeStamp,
    root: u64,
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
fn owner(proof: &PreparedCopies) -> Option<Held> {
    if proof.owner == 0 {
        return None;
    }
    let raw = proof.owner as *mut c_void;
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object the proof owns.
    unsafe {
        let vtbl = *(raw as *const *const IUnknown_Vtbl);
        let _ = ((*vtbl).QueryInterface)(raw, &RECEIPT_TYPE, &mut out);
    }
    (!out.is_null()).then(|| Held(out.cast()))
}
fn arena_of<'a>(owner: &Receipt) -> &'a mut Storage {
    // SAFETY: the receipt owns a storage reference (a separate object).
    unsafe { &mut *owner.arena.cast::<Storage>() }
}
fn public_proof(proof: &PreparedCopies, owner: &Receipt) -> bool {
    !owner.arena.is_null()
        && proof.list == owner.epoch.list
        && proof.generation == owner.epoch.generation
        && proof.occurrence == owner.epoch.occurrence
        && proof.copied_bytes == owner.copied
        && proof.count as usize == owner.entries.len()
}
fn source_check(list: *mut c_void, owner: &Receipt, entry: &Entry, current: &mut Detail, ready: bool) -> bool {
    if !capture_at_source(list, owner.root, entry.parameter, entry.offset, &entry.source.0, current) || !physical(entry, owner.base) {
        return false;
    }
    let (now, saved) = (&current.resource, &entry.observed.resource);
    if now.heap.Type != saved.heap.Type
        || now.heap_flags != saved.heap_flags
        || now.flags != saved.flags
        || now.width != saved.width
        || now.gpu != saved.gpu
        || current.context != entry.observed.context
    {
        return false;
    }
    if entry.kind == kind::POOL {
        return same_pool(&entry.observed.pool, &current.pool);
    }
    if entry.kind == kind::TRACKED {
        return current.state_queried
            && current.state_value_known
            && rules::read_mask(rules_state(current.state), ready)
            && (!ready || current.state.value & entry.required_state == entry.required_state);
    }
    cvr_scsn_upload_compatible()
        && inline_upload(&entry.source.0, &owner.epoch, entry.reflected_bytes)
        && current.state_queried
        && current.state_value_known
        && current.state.status == 1
        && current.state.value == GENERIC_READ
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
fn validate_range(entry: &Entry, queue: *mut c_void) -> bool {
    // SAFETY: the entry's owned lease and proof.
    unsafe {
        crate::buffer_read::cvr_buffer_read_validate_range(&entry.lease.0, &entry.range, entry.source.0.source_offset, entry.reflected_bytes as u64, queue)
    }
}
fn check(list: *mut c_void, owner: &Receipt, ready: bool) -> bool {
    set_failure(failure::SCOPE);
    if !scope(list, owner) {
        return false;
    }
    for entry in &owner.entries {
        let mut current = Detail::new();
        set_failure(failure::CHANGED);
        if !source_check(list, owner, entry, &mut current, ready) {
            return false;
        }
        if !ready && entry.kind == kind::TRACKED && current.state.value != entry.before_state {
            return false;
        }
        if entry.kind == kind::POOL {
            set_failure(failure::POOL);
            if ready {
                if !validate_range(entry, core::ptr::null_mut()) {
                    pool_failure(4, entry, None);
                    return false;
                }
            } else {
                let mut fresh = Proof::default();
                // SAFETY: a live list; the entry's owned lease; a local proof.
                if !unsafe {
                    crate::buffer_read::cvr_buffer_read_preflight_range(
                        list,
                        &entry.lease.0,
                        entry.source.0.source_offset,
                        entry.reflected_bytes as u64,
                        &mut fresh,
                    )
                } {
                    pool_failure(2, entry, Some(&fresh));
                    return false;
                }
                // SAFETY: two local proofs.
                if !unsafe { crate::buffer_read::cvr_brd_proof(0, &entry.range, &fresh, 0, 0) } {
                    pool_failure(3, entry, Some(&fresh));
                    return false;
                }
            }
        }
    }
    true
}
fn register(base: u64, context: u64, handle: u32, required: u32) {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the engine's combined-read registration (its code contract was checked when the source was read).
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
        if !scope(list, owner) || !physical(entry, owner.base) {
            return false;
        }
        let source = &entry.source.0;
        let mut fresh = OwnedCbv::empty();
        // SAFETY: the saved backing's stamp and origin; `fresh` is an empty backing this module owns.
        let status = unsafe {
            crate::source_cbv_observe::cvr_scbo_read_at_source(
                list as u64,
                owner.root,
                entry.parameter,
                entry.offset,
                &source.stamp,
                source.origin_owner,
                &mut *fresh.0,
            )
        };
        if (status != 1 && status != 3) || !same_source(source, &fresh.0) {
            return false;
        }
        let mut state = read_state(list, &entry.lease.0);
        if state.status == 4 {
            flush(owner.base, source.recording.context);
            state = read_state(list, &entry.lease.0);
        }
        let Some(required) = rules::recovery_mask(entry.before_state, rules_state(state)) else {
            return false;
        };
        if state.value & required != required {
            register(owner.base, source.recording.context, source.native.handle, required);
            flush(owner.base, source.recording.context);
        }
        let mut current = Detail::new();
        if !capture_at_source(list, owner.root, entry.parameter, entry.offset, source, &mut current)
            || !physical(entry, owner.base)
            || !current.state_value_known
            || !rules::read_mask(rules_state(current.state), false)
            || current.state.value & required != required
        {
            return false;
        }
    }
    true
}
fn fail_receipt(owner: &Receipt) {
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

/// ActualCallScope's constructor.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_scope_enter(list: u64, generation: u64, occurrence: u64, out: *mut ScopeStamp) {
    let stamp = SCOPES.with_borrow_mut(|s| s.enter(list, generation, occurrence, thread()));
    // SAFETY: the caller's output.
    unsafe { *out = stamp };
}
/// ActualCallScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_scope_exit(stamp: *const ScopeStamp) {
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
pub unsafe extern "C-unwind" fn cvr_scsn_scope_active(stamp: *const ScopeStamp) -> bool {
    // SAFETY: the caller's stamp.
    let stamp = unsafe { *stamp };
    scope_matches(&stamp, stamp.list, stamp.generation, stamp.position)
}
/// LastFailure.
#[no_mangle]
pub extern "C-unwind" fn cvr_scsn_last_failure() -> u32 {
    FAILURE.get()
}
/// PrepareAtActualCall: on success `*output` holds a new receipt reference.
///
/// # Safety
/// `requests` holds `count` requests; `parent` and `epoch` are valid; `arena` is the SnapshotArena's storage or null;
/// `output` is the caller's PreparedCopies.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_scsn_prepare(
    list: *mut c_void,
    root: u64,
    requests: *const Request,
    count: usize,
    parent: *const GraphParentRecord,
    epoch: *const ReadEpochCertificate,
    arena: *mut c_void,
    output: *mut PreparedCopies,
) -> bool {
    // SAFETY: as documented.
    let (requests, parent, epoch, output) = unsafe {
        (if count == 0 { &[][..] } else { core::slice::from_raw_parts(requests, count) }, &*parent, &*epoch, &mut *output)
    };
    guarded(|| prepare(list, root, requests, parent, epoch, arena, output))
}
fn prepare(
    list: *mut c_void,
    root: u64,
    requests: &[Request],
    parent: &GraphParentRecord,
    epoch: &ReadEpochCertificate,
    arena: *mut c_void,
    output: &mut PreparedCopies,
) -> bool {
    // SAFETY: the export's sixteen words.
    unsafe { core::ptr::addr_of_mut!(CyberpunkVR_SourceCbvSnapshotsPoolFailure).write([0; 16]) };
    set_failure(failure::DISABLED);
    if !enabled() || cvr_host::probe::internal() {
        return false;
    }
    set_failure(failure::ARGUMENTS);
    let Some(s) = storage(arena) else {
        return false;
    };
    if output.owner != 0
        || list.is_null()
        || root == 0
        || requests.is_empty()
        || requests.len() > MAX_REQUESTS
        || s.list != list as u64
        || s.generation != epoch.generation
        || s.finalized.load(SeqCst)
        || s.failed.load(SeqCst)
    {
        return false;
    }
    set_failure(failure::SCOPE);
    let scope_now = SCOPES.with_borrow(|l| l.current());
    if !scope_matches(&scope_now, list as u64, epoch.generation, epoch.occurrence) || !rules::epoch(epoch, list as u64, s.generation, scope_now.position) {
        return false;
    }
    set_failure(failure::CODE);
    if !crate::indirect_copy_native::cvr_ic_compatible() {
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
        views: Vec::with_capacity(requests.len()),
        entries: Vec::with_capacity(requests.len()),
        parent: *parent,
        epoch: *epoch,
        scope: scope_now,
        root,
        base: image(),
        copied: 0,
    })));
    let o = held.get();
    let mut views = Vec::with_capacity(requests.len());
    for request in requests {
        set_failure(failure::ARGUMENTS);
        let source = &request.source;
        if source.recording.list != epoch.list || source.recording.generation != epoch.generation || request.reflected_bytes == 0 || request.parameter >= 64 {
            return false;
        }
        if o.entries.iter().any(|e| e.parameter == request.parameter && e.offset == request.offset) {
            return false;
        }
        let mut entry = Entry {
            lease: OwnedLease(BufferLease { resource: core::ptr::null_mut(), gpu: 0, bytes: 0, heap_type: 0, flags: 0, native: source.native }),
            pins: Pins { allocator: core::ptr::null_mut(), allocation: core::ptr::null_mut() },
            creation: Creation::default(),
            source: OwnedCbv::copy(source),
            parameter: request.parameter,
            offset: request.offset,
            reflected_bytes: request.reflected_bytes,
            observed: Detail::new(),
            kind: 0,
            range: Proof::default(),
            destination: 0,
            before_state: 0,
            required_state: 0,
            mutated: false,
        };
        set_failure(failure::SOURCE);
        if !capture_at_source(list, root, request.parameter, request.offset, source, &mut entry.observed) {
            return false;
        }
        set_failure(failure::CREATION);
        match query_creation(source.resource) {
            Some(creation) => entry.creation = creation,
            None => return false,
        }
        let (admitted, kind_now) = rules::admit(&observe(source, request.reflected_bytes, &entry.observed, &entry.creation));
        entry.kind = kind_now;
        set_failure(admitted);
        if admitted != failure::NONE {
            return false;
        }
        if entry.kind == kind::UPLOAD && (!upload_enabled() || !cvr_scsn_upload_compatible() || !inline_upload(source, epoch, request.reflected_bytes)) {
            set_failure(failure::CLASS);
            return false;
        }
        entry.lease = OwnedLease(BufferLease {
            resource: add_ref(source.resource),
            gpu: source.gpu,
            bytes: source.backing_bytes,
            heap_type: entry.observed.resource.heap.Type.0 as u32,
            flags: entry.observed.resource.flags.0 as u32,
            native: source.native,
        });
        entry.before_state = entry.observed.state.value;
        entry.required_state = entry.before_state | COPY_SOURCE;
        if entry.kind != kind::UPLOAD {
            entry.pins.allocator = add_ref(entry.observed.allocator.address as *mut c_void);
            entry.pins.allocation = add_ref(entry.observed.allocation.address as *mut c_void);
            set_failure(failure::ALLOCATION);
            if !physical(&entry, o.base) {
                return false;
            }
        }
        if entry.kind == kind::POOL {
            set_failure(failure::POOL);
            let mut range = Proof::default();
            // SAFETY: a live list; the entry's owned lease; a local proof.
            let found = unsafe {
                crate::buffer_read::cvr_buffer_read_preflight_range(list, &entry.lease.0, source.source_offset, request.reflected_bytes as u64, &mut range)
            };
            entry.range = range;
            if !found {
                pool_failure(1, &entry, None);
                return false;
            }
        }
        views.push(source.descriptor_bytes);
        o.copied += request.reflected_bytes as u64;
        o.entries.push(entry);
    }
    set_failure(failure::CAPACITY);
    let arena_now = arena_of(o);
    let (offsets, end) = rules::reserve(arena_now.cursor, arena_now.capacity, &views);
    let Some(offsets) = offsets else {
        return false;
    };
    for (i, entry) in o.entries.iter_mut().enumerate() {
        entry.destination = offsets[i];
        let source = &entry.source.0;
        o.views.push(OwnedView(PrivateCbv {
            parameter: entry.parameter,
            offset: entry.offset,
            reflected_bytes: entry.reflected_bytes,
            padded_bytes: source.descriptor_bytes,
            source_offset: source.source_offset,
            private_offset: offsets[i],
            copied_bytes: entry.reflected_bytes as u64,
            view_location: arena_now.gpu + offsets[i],
            view_bytes: source.descriptor_bytes,
            resource: add_ref(arena_now.resource) as u64,
            owner: add_ref(o.arena) as u64,
        }));
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
    arena_now.recording = OwnedRecording::copy(&requests[0].source.recording);
    arena_now.state = o.entries[0].observed.context.state;
    arena_now.cursor = end;
    arena_now.pending.fetch_add(1, SeqCst);
    // SAFETY: the receipt; the output takes its own reference.
    unsafe { receipt_add_ref(held.raw()) };
    *output = PreparedCopies {
        owner: held.raw() as u64,
        list: epoch.list,
        generation: epoch.generation,
        occurrence: epoch.occurrence,
        copied_bytes: o.copied,
        count: o.entries.len() as u32,
    };
    set_failure(failure::NONE);
    true
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
    let now = o.stage.load(Acquire);
    if now != stage::PREPARED && now != stage::RECORDED {
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
pub unsafe extern "C-unwind" fn cvr_scsn_validate(list: *mut c_void, proof: *const PreparedCopies) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| validate(list, proof))
}
/// RecordAtActualCall: on success `out[..*count]` hold the private CBVs (new references, at most 28).
///
/// # Safety
/// `proof` is valid; `out` holds 28 empty views; `count` is writable; `list` is live or null; `output_empty` is whether
/// the caller's vector was empty.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_record(
    list: *mut c_void,
    proof: *const PreparedCopies,
    output_empty: bool,
    out: *mut PrivateCbv,
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
            for view in views.into_iter().take(MAX_REQUESTS) {
                // SAFETY: the caller's 28 empty slots (null references, so nothing is overwritten).
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
                fail_receipt(o);
            }
            false
        }
    }
}
fn record_at_call(list: *mut c_void, proof: &PreparedCopies, output_empty: bool, held: &mut Option<Held>, recording: &mut bool) -> Option<Vec<PrivateCbv>> {
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
    if o.entries.iter().any(|e| e.kind == kind::UPLOAD) && !upload_enabled() {
        set_failure(failure::CLASS);
        return None;
    }
    let result: Vec<PrivateCbv> = o.views.iter().map(OwnedView::duplicate).collect();
    let release_result = |views: Vec<PrivateCbv>| {
        for view in views {
            drop(OwnedView(view));
        }
    };
    if !validate(list, proof) {
        release_result(result);
        return None;
    }
    set_failure(failure::ALREADY_RECORDED);
    if o.stage.compare_exchange(stage::PREPARED, stage::RECORDING, SeqCst, SeqCst).is_err() {
        release_result(result);
        return None;
    }
    *recording = true;
    let mut ok = true;
    for i in 0..o.entries.len() {
        let kind_now = o.entries[i].kind;
        if kind_now == kind::POOL {
            set_failure(failure::REGISTRATION);
            let entry = &mut o.entries[i];
            // SAFETY: a live list; the entry's owned lease; the entry's proof (the expected one in, the captured one out).
            if !unsafe {
                crate::buffer_read::cvr_buffer_read_capture_range(
                    list,
                    &entry.lease.0,
                    entry.source.0.source_offset,
                    entry.reflected_bytes as u64,
                    &mut entry.range,
                )
            } {
                ok = false;
                break;
            }
        } else if kind_now == kind::TRACKED {
            let mut current = Detail::new();
            set_failure(failure::CHANGED);
            if !scope(list, o)
                || !source_check(list, o, &o.entries[i], &mut current, false)
                || current.state.value & o.entries[i].before_state != o.entries[i].before_state
            {
                ok = false;
                break;
            }
            let required = current.state.value | COPY_SOURCE;
            o.entries[i].required_state = required;
            if current.state.value & COPY_SOURCE != 0 {
                continue;
            }
            o.entries[i].mutated = true;
            let source = &o.entries[i].source.0;
            register(o.base, source.recording.context, source.native.handle, required);
            flush(o.base, source.recording.context);
        }
    }
    if ok {
        ok = check(list, o, true);
    }
    if !ok {
        if o.entries.iter().any(|e| e.mutated) {
            set_failure(if recover(list, o) { failure::RECOVERY_VERIFIED } else { failure::RECOVERY_FATAL });
        }
        fail_receipt(o);
        release_result(result);
        return None;
    }
    let arena = arena_of(o);
    {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: a live list; the arena's resource and each source's resource are owned references.
        if let (Some(l), Some(destination)) =
            (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe { ID3D12Resource::from_raw_borrowed(&arena.resource) })
        {
            for entry in &o.entries {
                let source_resource = entry.source.0.resource;
                if let Some(from) = unsafe { ID3D12Resource::from_raw_borrowed(&source_resource) } {
                    unsafe {
                        l.CopyBufferRegion(destination, entry.destination, from, entry.source.0.source_offset, entry.reflected_bytes as u64)
                    };
                }
            }
        }
    }
    arena.pending.fetch_sub(1, SeqCst);
    o.stage.store(stage::RECORDED, Release);
    set_failure(failure::NONE);
    Some(result)
}
/// MatchesRecordedPrivateCbv: `value` is exactly one of this recorded receipt's private CBVs.
///
/// # Safety
/// `proof` and `value` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_matches_recorded(proof: *const PreparedCopies, value: *const PrivateCbv) -> bool {
    // SAFETY: as documented.
    let (proof, value) = unsafe { (&*proof, &*value) };
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let Some(held) = owner(proof) else {
            return false;
        };
        let o = held.get();
        if !public_proof(proof, o)
            || proof.count == 0
            || proof.count as usize > MAX_REQUESTS
            || !rules::recorded_copy_fields(proof, &o.epoch, o.copied, o.entries.len())
            || o.views.len() != o.entries.len()
        {
            return false;
        }
        set_failure(failure::RETENTION);
        let arena = arena_of(o);
        if o.stage.load(Acquire) != stage::RECORDED
            || !arena.finalized.load(Acquire)
            || arena.failed.load(Acquire)
            || arena.pending.load(Acquire) != 0
            || arena.resource.is_null()
            || !rules::epoch(&o.epoch, arena.list, arena.generation, o.scope.position)
            || o.scope.list != proof.list
            || o.scope.generation != proof.generation
        {
            return false;
        }
        set_failure(failure::CHANGED);
        let mut matching = o.views.iter().enumerate().filter(|(_, v)| v.0.parameter == value.parameter && v.0.offset == value.offset);
        let Some((index, found)) = matching.next() else {
            return false;
        };
        if matching.next().is_some() {
            return false;
        }
        let entry = &o.entries[index];
        let source = &entry.source.0;
        if !rules::recorded_private_identity(
            &rules::private_identity(value),
            &rules::private_identity(&found.0),
            arena.resource as u64,
            o.arena as u64,
            arena.gpu,
            arena.cursor,
            arena.capacity,
        ) || entry.destination != value.private_offset
            || entry.parameter != value.parameter
            || entry.offset != value.offset
            || entry.reflected_bytes != value.reflected_bytes
            || source.source_offset != value.source_offset
            || source.descriptor_bytes != value.padded_bytes
            || source.recording.token.is_null()
            || source.recording.list != proof.list
            || source.recording.generation != proof.generation
            || source.recording.token != arena.recording.0.token
            || source.recording.context != arena.recording.0.context
            || source.recording.registry != arena.recording.0.registry
        {
            return false;
        }
        if o.stage.load(Acquire) != stage::RECORDED || !arena.finalized.load(Acquire) || arena.failed.load(Acquire) || arena.pending.load(Acquire) != 0 {
            return false;
        }
        set_failure(failure::NONE);
        true
    })
}
/// ValidateSourceSubmission.
///
/// # Safety
/// `proof` is valid; `queue` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scsn_validate_submission(proof: *const PreparedCopies, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    let proof = unsafe { &*proof };
    guarded(|| {
        set_failure(failure::ARGUMENTS);
        let held = owner(proof);
        let Some(h) = held.as_ref().filter(|_| !queue.is_null()) else {
            return false;
        };
        let o = h.get();
        let arena = arena_of(o);
        if !public_proof(proof, o) || o.stage.load(SeqCst) != stage::RECORDED || !arena.finalized.load(Acquire) || arena.failed.load(Acquire) {
            return false;
        }
        let submitted = |lease: &Recording| {
            // SAFETY: the backing's lease and a live queue.
            unsafe { crate::native_recording::cvr_native_recording_validate_submission(lease, queue) }
        };
        for entry in &o.entries {
            set_failure(failure::SOURCE);
            let source = &entry.source.0;
            if !submitted(&source.recording) || !physical(entry, o.base) {
                return false;
            }
            if entry.kind == kind::UPLOAD
                && (!cvr_scsn_upload_compatible() || !inline_upload(source, &o.epoch, entry.reflected_bytes) || !submitted(&source.creation_recording))
            {
                return false;
            }
            if entry.kind == kind::UPLOAD && !source.writer_recording.token.is_null() && !submitted(&source.writer_recording) {
                return false;
            }
            if entry.kind == kind::POOL {
                set_failure(failure::POOL);
                if !validate_range(entry, queue) {
                    return false;
                }
            }
        }
        set_failure(failure::NONE);
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_refuses_an_empty_source() {
        // SAFETY: a zeroed backing (no resource, no recording).
        let source: Backing = unsafe { core::mem::zeroed() };
        let mut detail = Detail::new();
        detail.status = 99;
        // SAFETY: local values.
        assert!(!unsafe { cvr_scsn_capture_at_source(core::ptr::null_mut(), 1, 0, 0, &source, &mut detail) });
        assert_eq!(detail.status, detail_status::ARGUMENTS);
    }

    #[test]
    fn proofs_without_an_owner_are_refused() {
        let proof = PreparedCopies::default();
        let view = PrivateCbv::default();
        // SAFETY: local values; nothing is read without an owner.
        unsafe {
            assert!(!cvr_scsn_validate(core::ptr::null_mut(), &proof));
            assert_eq!(cvr_scsn_last_failure(), failure::ARGUMENTS);
            assert!(!cvr_scsn_validate_submission(&proof, core::ptr::null_mut()));
            assert!(!cvr_scsn_matches_recorded(&proof, &view));
        }
    }

    #[test]
    fn arenas_refuse_bad_arguments_and_upload_code_needs_this_image() {
        let mut out = core::ptr::null_mut();
        // SAFETY: null objects are refused before use.
        assert_eq!(unsafe { cvr_scsn_arena_create(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 1, 256, &mut out) }, E_INVALIDARG);
        assert!(out.is_null());
        // SAFETY: no storage.
        unsafe {
            assert!(!cvr_scsn_arena_finalized(core::ptr::null_mut()));
            assert_eq!(cvr_scsn_arena_bytes(core::ptr::null_mut()), 0);
        }
        cvr_scsn_prepare_upload_code_guards(1);
        assert!(!cvr_scsn_upload_compatible());
    }
}
