//! The native definition tracking (formerly src/Stereo/NativeDefinitionTracking.cpp), behind CVR_STEREO_PAIRED_ROOT and
//! its code contract: the engine's definition writers (the builder reset and close, the owner append, the plain
//! graph storage birth and the generation destroy) stamped per owner header; the common allocator's construction,
//! setup and close bounded; the native consumers of a paired root registered, their definition generations bound and
//! verified, and an owned reference graph prepared, published, executed and retired behind its extra readers; the
//! resource replay of its declarations; the scene child scopes of its expanded sections.
//!
//! One mutex guards the state, as the .cpp's did; the consumers, tickets and retained owners it holds are reached only
//! under it. The detours are registered here with their contracts; the return addresses the C++ took with
//! `_ReturnAddress()` come from a one-frame back trace (ConstructHook's return slot from the .cpp's frame).

mod consumers;
mod replay;
pub(crate) mod scene_child;

pub use consumers::*;
pub use replay::*;
pub use scene_child::*;

use crate::definition_wrappers::{BorrowedSections, BothEyesPlan, CallerLifetime, CpuFrontierPlan, OwnedLedger, SceneSections, ScenePlan};
use crate::graph_definition::{self as gd, Mapping as DefinitionMapping};
use crate::native::allocator_binding::{self as ab, Lifecycle};
use crate::native::compiler_identity::CompilerIdentityLease;
use crate::native::consumer_identity::NativeRootTicket;
use crate::native::declaration_completion::DeclarationCompletionLease;
use crate::native::definition_generations as dg;
use crate::native::definition_tree::{NativeVector16, Snapshot as Tree};
use crate::native::graph_snapshot::Snapshot;
use crate::native::job_lease::JobLeaseApi;
use crate::native::job_receipt::FrameOwnerApi;
use crate::native::retirement_deferral::{self as rd, ExtraReaderIdentity, HeaderIdentity, NativeHeader, StateId};
use crate::native::resource_uses;
use crate::native::task_completion::TaskCompletionLease;
use crate::owned_native_graph::OwnedGraph;
use crate::used_moved_exports as x;
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::*};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

// ---- the engine
pub(crate) const MAX_TRACKED_OWNERS: usize = 4096;
pub(crate) const MAX_CONSUMERS: usize = 256;
/// Every call of the builder reset (8E7D7C) and close (8EAF50), one pair per builder function.
pub(crate) const BUILDER_SITES: [[u32; 2]; 8] = [
    [0x8EAC08, 0x8EAF2C],
    [0x2276FEC, 0x227BCF3],
    [0x227BD5F, 0x227C728],
    [0x227C783, 0x227D044],
    [0x227D0B8, 0x227E355],
    [0x2E75281, 0x2E75AC3],
    [0x2E75B0E, 0x2E75ED7],
    [0x2E75F27, 0x2E762D7],
];
pub(crate) const RENDERER_GLOBAL_RVA: u64 = 0x3BBCAE8;
pub(crate) fn base() -> u64 {
    cvr_engine::code::image_base() as u64
}
/// The .cpp's Read: ReadProcessMemory of the whole range.
pub(crate) fn read(address: u64, out: &mut [u8]) -> bool {
    cvr_engine::stack::read(address, out)
}
pub(crate) fn read_u64(address: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    read(address, &mut bytes).then(|| u64::from_le_bytes(bytes))
}
pub(crate) fn read_u32(address: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    read(address, &mut bytes).then(|| u32::from_le_bytes(bytes))
}
pub(crate) fn read_u8(address: u64) -> Option<u8> {
    let mut bytes = [0u8; 1];
    read(address, &mut bytes).then_some(bytes[0])
}
/// A reader for the Rust cores (their `&mut dyn FnMut` reads).
pub(crate) fn reader() -> impl FnMut(u64, &mut [u8]) -> bool {
    |address, out| read(address, out)
}
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is the hook, frame 1 its caller.
#[inline(always)]
pub(crate) fn return_address() -> u64 {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 {
        0
    } else {
        frames[0] as u64
    }
}
/// An export, written in place.
///
/// # Safety
/// `p` is one of the port's export statics.
pub(crate) unsafe fn out<T>(p: *mut T) -> &'static mut T {
    unsafe { &mut *p }
}
/// A vector's data() as the C++ exported it: null until it first had storage.
pub(crate) fn data<T>(v: &Vec<T>) -> u64 {
    if v.capacity() == 0 {
        0
    } else {
        v.as_ptr() as u64
    }
}
pub(crate) fn closing() -> bool {
    x::CyberpunkVR_OwnedExecutionClosing.load(SeqCst) != 0
}
/// FailFastProtectedIntent.
pub(crate) fn fail_fast() -> ! {
    // SAFETY: the process ends here.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::RaiseFailFastException(core::ptr::null(), core::ptr::null(), 0);
        windows_sys::Win32::System::Threading::TerminateProcess(windows_sys::Win32::System::Threading::GetCurrentProcess(), 0xC0000409);
    }
    std::process::abort()
}

// ---- the engine's reference calls
pub(crate) unsafe extern "C" fn retain_frame(owner: *mut c_void) {
    // SAFETY: a live frame owner; +8 is its reference count.
    unsafe { AtomicI32::from_ptr(owner.cast::<u8>().add(8).cast()) }.fetch_add(1, SeqCst);
}
pub(crate) unsafe extern "C" fn release_frame(owner: *mut c_void) {
    if !closing() {
        // SAFETY: the engine's release.
        let release: unsafe extern "C" fn(*mut c_void) = unsafe { core::mem::transmute((base() + 0x32DE14) as usize) };
        unsafe { release(owner) };
    }
}
pub(crate) unsafe extern "C" fn retain_group(group: *mut c_void) {
    // SAFETY: a live engine job group; +0x1C is its reference count.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x1C).cast()) }.fetch_add(1, SeqCst);
}
pub(crate) unsafe extern "C" fn release_group(group: *mut *mut c_void) {
    if !closing() {
        // SAFETY: the engine's release (it nulls the pointer).
        let release: unsafe extern "C" fn(*mut *mut c_void) = unsafe { core::mem::transmute((base() + 0x2744CC) as usize) };
        unsafe { release(group) };
    }
}
pub(crate) unsafe extern "C" fn complete_group(group: *mut c_void) -> bool {
    // SAFETY: a live engine job group; +0x18 its pending count (a full-barrier read, as the CAS was).
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x18).cast()) }.compare_exchange(0, 0, SeqCst, SeqCst).is_ok()
}
pub(crate) fn job_api(complete: bool) -> JobLeaseApi {
    JobLeaseApi { retain: Some(retain_group), release: Some(release_group), complete: complete.then_some(complete_group as unsafe extern "C" fn(*mut c_void) -> bool) }
}
pub(crate) fn frame_api() -> FrameOwnerApi {
    FrameOwnerApi { retain: Some(retain_frame), release: Some(release_frame) }
}
/// EarlyTerminalPin: an early execution group held while its owned execution may run.
pub(crate) struct EarlyTerminalPin(*mut c_void);
impl EarlyTerminalPin {
    pub(crate) fn new(group: u64) -> Self {
        // SAFETY: a live group with a positive count, read just before.
        unsafe { retain_group(group as *mut c_void) };
        EarlyTerminalPin(group as *mut c_void)
    }
    pub(crate) fn group(&self) -> u64 {
        self.0 as u64
    }
}
impl Drop for EarlyTerminalPin {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the reference taken in new.
            unsafe { release_group(&mut self.0) };
        }
    }
}

// ---- a cell reached only under the mutex
/// A shared value the .cpp held in a shared_ptr and changed under its mutex.
pub(crate) struct Shared<T>(UnsafeCell<T>);
// SAFETY: every access is under the definition tracking mutex (or before the value is shared).
unsafe impl<T> Sync for Shared<T> {}
unsafe impl<T> Send for Shared<T> {}
impl<T> Shared<T> {
    pub(crate) fn new(value: T) -> Arc<Shared<T>> {
        Arc::new(Shared(UnsafeCell::new(value)))
    }
    /// # Safety
    /// The caller holds the mutex and no other reference to the value is alive.
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}
pub(crate) type Ticket = Arc<Shared<NativeRootTicket>>;
pub(crate) type ConsumerRef = Arc<Shared<Consumer>>;

// ---- the telemetry seqlock
thread_local! {
    static TELEMETRY_DEPTH: Cell<u32> = const { Cell::new(0) };
}
struct TelemetryLock(UnsafeCell<windows_sys::Win32::System::Threading::SRWLOCK>);
// SAFETY: an SRW lock, shared by design.
unsafe impl Sync for TelemetryLock {}
static TELEMETRY_LOCK: TelemetryLock = TelemetryLock(UnsafeCell::new(windows_sys::Win32::System::Threading::SRWLOCK { Ptr: core::ptr::null_mut() }));
/// OwnedTelemetryWrite: the owned exports written between two sequence bumps, one writer at a time.
pub(crate) struct TelemetryWrite;
impl TelemetryWrite {
    pub(crate) fn new() -> Self {
        TELEMETRY_DEPTH.with(|depth| {
            if depth.get() == 0 {
                // SAFETY: the static SRW lock.
                unsafe { windows_sys::Win32::System::Threading::AcquireSRWLockExclusive(TELEMETRY_LOCK.0.get()) };
                x::CyberpunkVR_OwnedExecutionSeq.fetch_add(1, AcqRel);
            }
            depth.set(depth.get() + 1);
        });
        TelemetryWrite
    }
}
impl Drop for TelemetryWrite {
    fn drop(&mut self) {
        TELEMETRY_DEPTH.with(|depth| {
            depth.set(depth.get() - 1);
            if depth.get() == 0 {
                x::CyberpunkVR_OwnedExecutionSeq.fetch_add(1, Release);
                // SAFETY: acquired by this thread in new.
                unsafe { windows_sys::Win32::System::Threading::ReleaseSRWLockExclusive(TELEMETRY_LOCK.0.get()) };
            }
        });
    }
}

// ---- the failures and events
pub(crate) mod owned_failure {
    pub const MODE: u32 = 1;
    pub const ADMISSION: u32 = 2;
    pub const CAPACITY: u32 = 3;
    pub const WRITER_COVERAGE: u32 = 4;
    pub const STACK: u32 = 6;
    pub const SOURCE: u32 = 7;
    pub const TREE: u32 = 8;
    pub const SORT: u32 = 9;
    pub const OWNERS: u32 = 10;
    pub const RESERVATION: u32 = 11;
    pub const TICKET: u32 = 12;
    pub const DECLARATION: u32 = 13;
    pub const TERMINAL: u32 = 14;
    pub const PUBLICATION: u32 = 15;
    pub const ROLLBACK: u32 = 16;
    pub const CONSTRUCT: u32 = 17;
    pub const QUEUE: u32 = 18;
    pub const EXECUTION: u32 = 19;
    pub const RESOURCE: u32 = 22;
    pub const RETIREMENT_INVARIANT: u32 = 23;
    pub const ALLOCATOR_BINDING: u32 = 24;
    pub const SCENE_EXPANSION: u32 = 25;
    const _: u32 = MODE;
}
#[derive(Clone, Copy)]
#[repr(u64)]
pub(crate) enum RetireEvent {
    Reserve = 1,
    Admit,
    Complete,
    Cancel,
    Intent,
    Detach,
    DestroyBegin,
    DestroyReturn,
    Recycle,
    Abandon,
    Quarantine,
    AllocatorBound,
    AllocatorClose,
    AllocatorReset,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeIntentResult {
    NoAttachedOwner,
    Detached,
    Quarantined,
    FatalInvariant,
}

// ---- the state
#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    pub epoch: u64,
    pub generation: u64,
    pub storage: u64,
    pub service_epoch: u64,
    pub service_storage: u64,
    pub in_flight: u32,
    pub kind: u32,
    pub builders: u32,
    pub operations: u32,
    pub slot: u32,
    pub builder_closed: bool,
    pub storage_alive: bool,
    pub service_owner: *mut RetainedOwner,
}
impl Default for Stamp {
    fn default() -> Self {
        Stamp {
            epoch: 0,
            generation: 0,
            storage: 0,
            service_epoch: 0,
            service_storage: 0,
            in_flight: 0,
            kind: 0,
            builders: 0,
            operations: 0,
            slot: 0,
            builder_closed: false,
            storage_alive: false,
            service_owner: core::ptr::null_mut(),
        }
    }
}
#[derive(Clone, Copy, Default)]
pub(crate) struct BuilderReservation {
    pub address: u64,
    pub header: u64,
    pub epoch: u64,
    pub thread: u64,
    pub end_return: u32,
}
/// The published owner row (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct OwnerRow {
    pub graph: u64,
    pub header: u64,
    pub data: u64,
    pub allocator: u64,
    pub epoch: u64,
    pub entry_start: u64,
    pub capacity: u32,
    pub count: u32,
    pub in_flight: u32,
    pub kind: u32,
}
const _: () = assert!(core::mem::size_of::<OwnerRow>() == 64 && core::mem::size_of::<DefinitionMapping>() == 24);
#[derive(Clone, Copy, Default)]
pub(crate) struct DefinitionBinding {
    pub generation: u64,
    pub born_epoch: u64,
}
pub(crate) struct RetainedOwner {
    pub identity: HeaderIdentity,
    pub state: Option<StateId>,
    pub tree: Tree,
    pub allocator_binding: ab::Receipt,
    pub definitions: Vec<u64>,
    pub ready: bool,
    pub emergency_detached: bool,
    pub destroy_in_flight: bool,
    pub last_consumer_id: u64,
    pub last_epoch: u64,
    pub last_pair_generation: u64,
    pub protected_attached: bool,
    pub completion_flags: u64,
    pub declaration_group: u64,
    pub execution_group: u64,
    pub compiler_group: u64,
    pub completed_readers_receipt: u64,
    pub service_header: u64,
    pub service_storage: u64,
    pub service_epoch: u64,
    pub service_caller: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct ExtraReader {
    pub owner: *mut RetainedOwner,
    pub identity: ExtraReaderIdentity,
}
#[derive(Default)]
pub(crate) struct AllocatorDomain {
    pub incarnation: u64,
    pub reset_epoch: u64,
    pub services_active: u64,
    pub close_reset_events: u64,
    pub in_flight: u32,
    pub last_entry: u32,
    pub ready: bool,
    pub closing: bool,
    pub service_certificate_observed: bool,
    pub binding: ab::Receipt,
}
/// The published stage definition row (128 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct StageDefinitionRow {
    pub object: u64,
    pub root_handle: u64,
    pub view: u64,
    pub first: u64,
    pub count: u64,
    pub vtable: u64,
    pub work: u64,
    pub root_object: u64,
    pub header: [u8; 64],
}
const _: () = assert!(core::mem::size_of::<StageDefinitionRow>() == 128);
/// The published scene root row (112 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SceneRootRow {
    pub object: u64,
    pub handle: u64,
    pub view: u64,
    pub first: u64,
    pub count: u64,
    pub split: u64,
    pub reserved: u64,
    pub header: [u8; 56],
}
const _: () = assert!(core::mem::size_of::<SceneRootRow>() == 112);

/// A native consumer of a paired root (the .cpp's Consumer).
pub(crate) struct Consumer {
    pub ticket: Option<Ticket>,
    pub completion: Option<Box<TaskCompletionLease>>,
    pub declaration: Option<Box<DeclarationCompletionLease>>,
    pub compiler: Option<Box<CompilerIdentityLease>>,
    pub fallback_ticket: Option<Ticket>,
    pub fallback_declaration: Option<Box<DeclarationCompletionLease>>,
    pub callback_entered: bool,
    pub callback_returned: bool,
    pub rejected: bool,
    pub source_verified: bool,
    pub execution_verified: bool,
    pub generation_rejected: bool,
    pub renderer: u64,
    pub rejection_detail: u64,
    pub rejection_header: u64,
    pub definitions: BTreeMap<u64, DefinitionBinding>,
    pub generations: Vec<DefinitionBinding>,
    pub owned_graph: Option<Box<OwnedGraph>>,
    pub original_graph: Snapshot,
    pub execution_graph: Snapshot,
    pub original_source: gd::Receipt,
    pub original_tree: Tree,
    pub scene_split_source: SceneSections,
    pub scene_views: BorrowedSections,
    pub scene_lifetime: CallerLifetime,
    pub scene_expansion: Option<Box<ScenePlan>>,
    pub scene_both_sources: [SceneSections; 2],
    pub scene_both_views: [BorrowedSections; 2],
    pub scene_both_lifetimes: [CallerLifetime; 2],
    pub scene_both_expansion: Option<Box<BothEyesPlan>>,
    pub scene_sections: [SceneSections; 2],
    pub scene_child_scopes: u32,
    pub coupled_graph: Snapshot,
    /// The owned execution state's consumer id in the ledger (the .cpp's State*).
    pub owned_state: Option<u64>,
    pub extra_readers: Vec<ExtraReader>,
    pub execution_terminal: Option<EarlyTerminalPin>,
    pub producer_scope: u64,
    pub declaration_context: u64,
    pub resource_pool: u64,
    pub resource_wrapper: u64,
    pub compiler_producer_identity: u64,
    pub extra_readers_completed: bool,
    pub resource_compiled: bool,
    pub declaration_snapshot_ready: bool,
    pub coupled_schedule: Option<Box<CpuFrontierPlan>>,
    pub resource_uses: resource_uses::Snapshot,
    pub resource_replay: Option<Box<crate::resource_replay::Receipt>>,
    pub resource_replay_rows: Vec<[u32; 8]>,
    pub resource_replay_failure: u32,
    pub resource_replay_slot: u32,
    pub resource_replay_cursor: u32,
    pub resource_replay_misses: u32,
    pub resource_replay_compiler_checked: bool,
    pub resource_replay_terminal_checked: bool,
    pub resource_replay_reset_sealed: bool,
    pub resource_replay_reset_returned: bool,
}
impl Default for Consumer {
    fn default() -> Self {
        Consumer {
            ticket: None,
            completion: None,
            declaration: None,
            compiler: None,
            fallback_ticket: None,
            fallback_declaration: None,
            callback_entered: false,
            callback_returned: false,
            rejected: false,
            source_verified: false,
            execution_verified: false,
            generation_rejected: false,
            renderer: 0,
            rejection_detail: 0,
            rejection_header: 0,
            definitions: BTreeMap::new(),
            generations: Vec::new(),
            owned_graph: None,
            original_graph: Snapshot::default(),
            execution_graph: Snapshot::default(),
            original_source: gd::Receipt::default(),
            original_tree: Tree::default(),
            scene_split_source: SceneSections::default(),
            scene_views: BorrowedSections::default(),
            scene_lifetime: CallerLifetime::default(),
            scene_expansion: None,
            scene_both_sources: Default::default(),
            scene_both_views: Default::default(),
            scene_both_lifetimes: Default::default(),
            scene_both_expansion: None,
            scene_sections: Default::default(),
            scene_child_scopes: 0,
            coupled_graph: Snapshot::default(),
            owned_state: None,
            extra_readers: Vec::new(),
            execution_terminal: None,
            producer_scope: 0,
            declaration_context: 0,
            resource_pool: 0,
            resource_wrapper: 0,
            compiler_producer_identity: 0,
            extra_readers_completed: false,
            resource_compiled: false,
            declaration_snapshot_ready: false,
            coupled_schedule: None,
            resource_uses: resource_uses::Snapshot::default(),
            resource_replay: None,
            resource_replay_rows: Vec::new(),
            resource_replay_failure: 0,
            resource_replay_slot: 0,
            resource_replay_cursor: 0,
            resource_replay_misses: 0,
            resource_replay_compiler_checked: false,
            resource_replay_terminal_checked: false,
            resource_replay_reset_sealed: false,
            resource_replay_reset_returned: false,
        }
    }
}
impl Consumer {
    /// The consumer's ticket. Like the .cpp's `consumer.ticket->`, the reference is not tied to the consumer's
    /// borrow: the ticket lives in its shared cell for as long as the consumer holds it (callers do not replace it
    /// while they use the reference).
    pub(crate) fn ticket<'t>(&self) -> &'t mut NativeRootTicket {
        // SAFETY: under the mutex; a consumer always has its ticket once registered; the cell outlives the use.
        unsafe { &mut *self.ticket.as_ref().expect("a registered consumer's ticket").0.get() }
    }
    pub(crate) fn has_scene_expansion(&self) -> bool {
        self.scene_expansion.is_some() || self.scene_both_expansion.is_some()
    }
}

/// Everything the .cpp's mutex guarded.
pub(crate) struct State {
    pub builders: Box<[BuilderReservation; MAX_TRACKED_OWNERS]>,
    pub next_storage: u64,
    pub storage_slots: u64,
    pub mutations: BTreeMap<u64, Stamp>,
    pub owners: Vec<OwnerRow>,
    pub mappings: Vec<DefinitionMapping>,
    pub entries: Vec<u64>,
    pub source: gd::Receipt,
    pub definition_tree: Tree,
    pub next_mutation: u64,
    pub capture_epoch: u64,
    pub capture_generation: u64,
    pub capture_owner: u64,
    pub next_consumer: u64,
    pub generation_ledger: dg::Ledger,
    pub closed_generations: Vec<(u64, u64)>,
    pub source_ticket: Option<Ticket>,
    pub published_replay_rows: Vec<[u32; 8]>,
    pub published_schedule_source: Snapshot,
    pub published_schedule_bindings: Vec<crate::native::graph_snapshot::Node>,
    pub published_stage_definitions: Vec<StageDefinitionRow>,
    pub published_stage_entries: Vec<[u8; 48]>,
    pub published_scene_roots: Vec<SceneRootRow>,
    pub published_scene_children: Vec<crate::scene_sections::Child>,
    pub published_split_source: Snapshot,
    pub published_split_mapping: Vec<crate::scene_expansion::Mapping>,
    pub replay_payloads: BTreeMap<u64, replay::ReplayPayload>,
    pub async_replay_payloads: BTreeMap<u64, replay::AsyncReplay>,
    pub consumers: BTreeMap<u64, ConsumerRef>,
    pub owned_execution_states: OwnedLedger,
    pub retirement_states: rd::Ledger,
    pub retained_owners: Vec<Option<Box<RetainedOwner>>>,
    pub next_extra_reader: u64,
    pub allocator_domain: AllocatorDomain,
    pub scene_child_next_invocation: u64,
}
impl State {
    fn new() -> State {
        State {
            builders: Box::new([BuilderReservation::default(); MAX_TRACKED_OWNERS]),
            next_storage: 0,
            storage_slots: 0,
            mutations: BTreeMap::new(),
            owners: Vec::new(),
            mappings: Vec::new(),
            entries: Vec::new(),
            source: gd::Receipt::default(),
            definition_tree: Tree::default(),
            next_mutation: 0,
            capture_epoch: 0,
            capture_generation: 0,
            capture_owner: 0,
            next_consumer: 0,
            generation_ledger: dg::Ledger::default(),
            closed_generations: Vec::new(),
            source_ticket: None,
            published_replay_rows: Vec::new(),
            published_schedule_source: Snapshot::default(),
            published_schedule_bindings: Vec::new(),
            published_stage_definitions: Vec::new(),
            published_stage_entries: Vec::new(),
            published_scene_roots: Vec::new(),
            published_scene_children: Vec::new(),
            published_split_source: Snapshot::default(),
            published_split_mapping: Vec::new(),
            replay_payloads: BTreeMap::new(),
            async_replay_payloads: BTreeMap::new(),
            consumers: BTreeMap::new(),
            owned_execution_states: OwnedLedger::default(),
            retirement_states: rd::Ledger::default(),
            retained_owners: (0..rd::MAX_STATES).map(|_| None).collect(),
            next_extra_reader: 0,
            allocator_domain: AllocatorDomain::default(),
            scene_child_next_invocation: 0,
        }
    }
    /// The retirement state of an owner (`owner->state`).
    pub(crate) fn rs(&mut self, owner: *mut RetainedOwner) -> Option<&mut rd::State> {
        // SAFETY: an owner boxed in retained_owners, under the mutex.
        let id = unsafe { (*owner).state }?;
        self.retirement_states.get_mut(id)
    }
    pub(crate) fn rs_ref(&self, owner: *const RetainedOwner) -> Option<&rd::State> {
        // SAFETY: as rs.
        let id = unsafe { (*owner).state }?;
        self.retirement_states.get(id)
    }
    pub(crate) fn owner_ptrs(&mut self) -> Vec<*mut RetainedOwner> {
        self.retained_owners.iter_mut().flatten().map(|owner| &mut **owner as *mut RetainedOwner).collect()
    }
}
struct Global {
    lock: Mutex<()>,
    state: UnsafeCell<Option<State>>,
}
// SAFETY: the state is reached only through the lock.
unsafe impl Sync for Global {}
static GLOBAL: Global = Global { lock: Mutex::new(()), state: UnsafeCell::new(None) };
/// The mutex held (std::lock_guard lock(mutex)).
pub(crate) struct Locked {
    _guard: MutexGuard<'static, ()>,
}
pub(crate) fn lock() -> Locked {
    Locked { _guard: GLOBAL.lock.lock().unwrap_or_else(|p| p.into_inner()) }
}
impl Locked {
    /// The guarded state. Callers take it once per lock and pass it down.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn state(&self) -> &mut State {
        // SAFETY: the lock is held by this guard.
        unsafe { (*GLOBAL.state.get()).get_or_insert_with(State::new) }
    }
}

// ---- the thread-locals and the atomics outside the mutex
thread_local! {
    pub(crate) static ROOT_ADMISSIONS: core::cell::RefCell<[crate::definition_cache::Admission; 32]> = core::cell::RefCell::new([crate::definition_cache::Admission::default(); 32]);
    pub(crate) static ROOT_ADMISSION_DEPTH: Cell<u32> = const { Cell::new(0) };
    pub(crate) static RETIREMENT_BYPASS: Cell<u32> = const { Cell::new(0) };
    pub(crate) static RETIREMENT_CALL_OWNER: Cell<*mut RetainedOwner> = const { Cell::new(core::ptr::null_mut()) };
    pub(crate) static RETIREMENT_CALL_HEADER: Cell<u64> = const { Cell::new(0) };
    pub(crate) static RETIREMENT_CALL_SCOPE: Cell<u64> = const { Cell::new(0) };
}
pub(crate) fn root_depth() -> u32 {
    ROOT_ADMISSION_DEPTH.with(Cell::get)
}
/// The innermost root admission (rootAdmissions[rootAdmissionDepth-1]) when in range.
pub(crate) fn with_admission<R>(f: impl FnOnce(&mut crate::definition_cache::Admission) -> R) -> Option<R> {
    let depth = root_depth();
    if depth == 0 || depth > 32 {
        return None;
    }
    Some(ROOT_ADMISSIONS.with(|admissions| f(&mut admissions.borrow_mut()[depth as usize - 1])))
}
pub(crate) static WRITER_COVERAGE_LOST: AtomicBool = AtomicBool::new(false);
pub(crate) static MUTATION_OVERFLOW: AtomicBool = AtomicBool::new(false);
pub(crate) static PROTECTED_ATTACHED_OWNERS: AtomicU64 = AtomicU64::new(0);
pub(crate) static OWNED_ADMISSION_WINDOWS: AtomicU64 = AtomicU64::new(0);
pub(crate) static ALLOCATOR_SERVICES_ACTIVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static ALLOCATOR_READY_OBSERVED: AtomicBool = AtomicBool::new(false);
pub(crate) static LAST_PROTECTION_IDENTITY: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
pub(crate) static LAST_PROTECTION_SEQ: AtomicU64 = AtomicU64::new(0);
/// OwnedAdmissionWindow.
pub(crate) struct AdmissionWindow(bool);
impl AdmissionWindow {
    pub(crate) fn new() -> Self {
        let active = x::CyberpunkVR_OwnedExecutionMode.load(SeqCst) == 1 && x::CyberpunkVR_OwnedExecutionRequest.load(SeqCst) != 0;
        if active {
            OWNED_ADMISSION_WINDOWS.fetch_add(1, SeqCst);
        }
        AdmissionWindow(active)
    }
}
impl Drop for AdmissionWindow {
    fn drop(&mut self) {
        if self.0 {
            OWNED_ADMISSION_WINDOWS.fetch_sub(1, SeqCst);
        }
    }
}

// ---- the published receipts
/// RetirementEvent.
#[allow(clippy::too_many_arguments)]
pub(crate) fn retirement_event(s: &State, kind: RetireEvent, owner: *const RetainedOwner, failure: u64, current_header: u64, current_storage: u64, current_epoch: u64, caller: u64, consumer: u64, epoch: u64, pair: u64) {
    let _write = TelemetryWrite::new();
    // SAFETY: the exports.
    let (sequence, events) = unsafe {
        x::CyberpunkVR_OwnedExecutionRetirementEventSeq = x::CyberpunkVR_OwnedExecutionRetirementEventSeq.wrapping_add(1);
        (x::CyberpunkVR_OwnedExecutionRetirementEventSeq, out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirementEvents)))
    };
    let row = &mut events[((sequence - 1) % 32) as usize];
    row.fill(0);
    row[0] = sequence;
    row[1] = kind as u64;
    if !owner.is_null() {
        // SAFETY: a live retained owner under the mutex.
        let o = unsafe { &*owner };
        let id = &o.identity;
        row[2] = id.owner_header;
        row[3] = id.generation;
        row[4] = id.born_epoch;
        row[5] = id.saved_native.data;
        row[6] = u64::from(id.saved_native.capacity);
        row[7] = u64::from(id.saved_native.count);
        row[8] = o.last_consumer_id;
        row[9] = o.last_epoch;
        row[10] = o.last_pair_generation;
        row[16] = o.declaration_group;
        row[17] = o.execution_group;
        row[18] = o.compiler_group;
        row[19] = o.completed_readers_receipt;
        row[20] = s.rs_ref(owner).map_or(0, |state| state.pending_readers() as u64);
        row[21] = o.completion_flags << 32;
        row[23] = o.allocator_binding.allocations().len() as u64;
    }
    if consumer != 0 {
        row[8] = consumer;
        row[9] = epoch;
        row[10] = pair;
    }
    row[11] = s.allocator_domain.incarnation;
    row[12] = s.allocator_domain.reset_epoch;
    row[13] = current_header;
    row[14] = current_storage;
    row[15] = current_epoch;
    row[21] |= failure;
    row[22] = caller;
    if !owner.is_null() && current_header == 0 && matches!(kind, RetireEvent::Recycle) {
        // SAFETY: as above.
        let o = unsafe { &*owner };
        row[13] = o.service_header;
        row[14] = o.service_storage;
        row[15] = o.service_epoch;
        row[22] = o.service_caller;
    }
    // SAFETY: the export.
    unsafe { x::CyberpunkVR_OwnedExecutionRetirementEventCount = sequence.min(32) };
}
pub(crate) fn event(s: &State, kind: RetireEvent, owner: *const RetainedOwner) {
    retirement_event(s, kind, owner, 0, 0, 0, 0, 0, 0, 0, 0);
}
pub(crate) fn totals() -> &'static mut [u64; 16] {
    // SAFETY: the export.
    unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirementTotals)) }
}
pub(crate) fn owned_stats() -> &'static mut [u64; 24] {
    // SAFETY: the export.
    unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionStats)) }
}
/// OwnedReject.
pub(crate) fn owned_reject(reason: u32, detail: u64) {
    let _write = TelemetryWrite::new();
    let stats = owned_stats();
    stats[0] = 5;
    stats[22] = u64::from(reason);
    stats[23] = detail;
}
/// PublishCompilerReceipt.
pub(crate) fn publish_compiler_receipt(consumer: &Consumer) {
    let Some(lease) = consumer.compiler.as_ref() else { return };
    let _write = TelemetryWrite::new();
    let binding = lease.binding();
    let fields = [
        binding.consumer_id,
        lease.group(),
        lease.captured() as u64,
        lease.matched() as u64,
        lease.original_dispatch_returned() as u64,
        lease.original_root_returned() as u64,
        lease.original_compiler_returned() as u64,
        (!closing() && lease.complete()) as u64,
        lease.rejection() as u64,
        binding.renderer,
        binding.wrapper,
        binding.pool,
        binding.frame_owner,
        binding.execution_graph,
        binding.epoch,
        binding.generation,
    ];
    // SAFETY: the export.
    unsafe { *out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionCompiler)) = fields };
}
/// PublishRetirementReceipt.
pub(crate) fn publish_retirement_receipt(s: &mut State) {
    let _write = TelemetryWrite::new();
    // SAFETY: the export.
    let fields = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirement)) };
    fields.fill(0);
    let d = &s.allocator_domain;
    fields[0] = (d.service_certificate_observed && d.ready && !d.closing && !closing() && allocator_coverage_ready()) as u64;
    fields[1] = s.retirement_states.size() as u64;
    fields[2] = s.retirement_states.owner_count() as u64;
    for owner in s.owner_ptrs() {
        // SAFETY: a boxed owner under the mutex.
        let emergency = unsafe { (*owner).emergency_detached };
        if let Some(state) = s.rs_ref(owner) {
            fields[3] += state.pending_readers() as u64;
            fields[4] += (state.exact_header_detached() || emergency) as u64;
            fields[5] += state.destroy_attempt_consumed() as u64;
            fields[6] += state.destroy_returned() as u64;
        }
    }
    fields[7] = closing() as u64;
}
/// PublishGenerationStats.
pub(crate) fn publish_generation_stats(s: &State) {
    let stats = &x::CyberpunkVR_PreparationGenerationStats;
    let l = &s.generation_ledger;
    stats[2].store(l.cancelled_consumers(), SeqCst);
    stats[7].store(l.completed_consumers(), SeqCst);
    stats[8].store(l.forgotten_observations(), SeqCst);
    stats[9].store(l.unsupported_consumers(), SeqCst);
    stats[11].store(l.active_consumers() as u64, SeqCst);
    stats[12].store(l.active_generations() as u64, SeqCst);
    stats[14].store(l.retired_generations(), SeqCst);
}
/// LoseGenerationCoverage.
pub(crate) fn lose_generation_coverage(s: &mut State, reason: dg::Failure) {
    x::CyberpunkVR_PreparationGenerationCoverageLost.store(1, SeqCst);
    let stamped: Vec<(u64, u64)> = s.mutations.values().filter(|stamp| stamp.generation != 0).map(|stamp| (stamp.generation, stamp.epoch)).collect();
    for (generation, epoch) in stamped {
        s.generation_ledger.unsupported_generation(generation, epoch, reason);
    }
    let closed = s.closed_generations.clone();
    for (id, epoch) in closed {
        s.generation_ledger.unsupported_generation(id, epoch, reason);
    }
}
/// RejectGenerations.
pub(crate) fn reject_generations(s: &mut State, consumer: &mut Consumer, reason: dg::Failure) {
    if consumer.generation_rejected {
        return;
    }
    consumer.generation_rejected = true;
    x::CyberpunkVR_PreparationGenerationStats[10].fetch_add(1, SeqCst);
    x::CyberpunkVR_PreparationGenerationStats[15].store(reason as u64, SeqCst);
    let ticket = consumer.ticket();
    let diagnostic = [reason as u64, ticket.id(), ticket.epoch(), ticket.generation(), ticket.graph_address(), consumer.renderer, consumer.rejection_detail, consumer.rejection_header];
    // SAFETY: the export.
    unsafe { *out(addr_of_mut!(x::CyberpunkVR_PreparationGenerationReject)) = diagnostic };
    x::CyberpunkVR_PreparationGenerationRejectPhase.store(consumer.callback_entered as u32, SeqCst);
    if s.generation_ledger.has_consumer(ticket.id()) {
        s.generation_ledger.unsupported_consumer(ticket.id(), ticket.epoch(), ticket.generation(), reason);
    } else {
        lose_generation_coverage(s, reason);
    }
    publish_generation_stats(s);
}
/// PublishSelectedGenerations.
pub(crate) fn publish_selected_generations(s: &State, consumer: &Consumer, complete: bool) {
    x::CyberpunkVR_PreparationGenerationSeq.fetch_add(1, SeqCst);
    let ticket = consumer.ticket();
    let identity = [ticket.id(), ticket.epoch(), ticket.generation(), consumer.source_verified as u64 | (consumer.execution_verified as u64) << 1 | (complete as u64) << 2];
    // SAFETY: the exports.
    let (ticket_out, rows) = unsafe { (out(addr_of_mut!(x::CyberpunkVR_PreparationGenerationTicket)), out(addr_of_mut!(x::CyberpunkVR_PreparationGenerationRows))) };
    *ticket_out = identity;
    let count = consumer.generations.len().min(128);
    x::CyberpunkVR_PreparationGenerationCount.store(count as u32, SeqCst);
    for (i, binding) in consumer.generations.iter().take(count).enumerate() {
        let row = &mut rows[i];
        row.fill(0);
        row[0] = binding.generation;
        row[2] = binding.born_epoch;
        let Some(value) = s.generation_ledger.get_generation(binding.generation) else { continue };
        row[1] = value.owner_header;
        row[3] = value.closed_mutation_epoch;
        row[4] = value.closed_at_consumer;
        row[5] = value.bound_consumers;
        row[6] = value.completed_consumers;
        row[7] = value.pending_consumers;
        row[8] = value.unsupported_consumers;
        row[9] = value.source_closed as u64 | (value.detached as u64) << 1 | (value.all_prior_consumers_accounted as u64) << 2 | (value.unsupported as u64) << 3 | (value.retiring as u64) << 4;
        row[10] = consumer.definitions.values().filter(|item| item.generation == binding.generation).count() as u64;
        row[11] = s.generation_ledger.can_adopt(binding.generation, false) as u64;
    }
    x::CyberpunkVR_PreparationGenerationSeq.fetch_add(1, SeqCst);
}

// ---- the code contracts
pub(crate) fn env_paired_root() -> bool {
    std::env::var_os("CVR_STEREO_PAIRED_ROOT").is_some()
}
/// CodeCompatible.
pub(crate) extern "C" fn code_compatible() -> bool {
    if !env_paired_root() {
        return false;
    }
    static VALID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let valid = *VALID.get_or_init(|| {
        // SAFETY: plain module lookups.
        unsafe {
            use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
            let name: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
            let kernel = GetModuleHandleW(name.as_ptr());
            if kernel.is_null() {
                return false;
            }
            let alloc = GetProcAddress(kernel, c"FlsAlloc".as_ptr().cast()).map_or(0, |f| f as usize as u64);
            let set = GetProcAddress(kernel, c"FlsSetValue".as_ptr().cast()).map_or(0, |f| f as usize as u64);
            crate::definition_code::cvr_ndt_code(base(), alloc, set)
        }
    });
    x::CyberpunkVR_PreparationDefinitionCompatible.store(if valid { 1 } else { 2 }, SeqCst);
    valid
}
/// SceneCodeCompatible.
pub(crate) fn scene_code_compatible() -> bool {
    static VALID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VALID.get_or_init(|| crate::definition_code::cvr_ndt_scene_code(base()))
}
/// WriterCallsitesCompatible.
pub(crate) fn writer_callsites_compatible() -> bool {
    static VALID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VALID.get_or_init(|| crate::definition_code::writer_callsites(base(), &BUILDER_SITES))
}
pub(crate) fn installed(rva: usize) -> bool {
    cvr_engine::detours::installed_at(rva)
}
/// AllocatorCoverageReady.
pub(crate) fn allocator_coverage_ready() -> bool {
    code_compatible() && [0x23999B0, 0x260E2B4, 0x260E630, 0x260E358, 0x260E3DC, 0x260E410, 0x68CBA4, 0x68CA94, 0x68CACC, 0x260E9DC, 0x260EAEC].iter().all(|&rva| installed(rva))
}
/// CaptureCompatible.
pub(crate) extern "C" fn capture_compatible() -> bool {
    code_compatible() && installed(0x8E7D7C) && installed(0x8E7704) && installed(0xB79044)
}
/// OwnedWriterCoverageReady.
pub(crate) fn owned_writer_coverage_ready() -> bool {
    code_compatible()
        && writer_callsites_compatible()
        && !WRITER_COVERAGE_LOST.load(SeqCst)
        && !MUTATION_OVERFLOW.load(SeqCst)
        && installed(0x8E7AE0) // RenderGraphCacheObservationInstalled: [render-parity] GraphCacheLookup
        && installed(0x40A1D1)
        && installed(0x8E7CA8)
        && installed(0x8EAF50)
        && installed(0x8EA438)
        && installed(0x8E7D7C)
        && installed(0x8E7704)
        && installed(0xB52BF0)
        && installed(0xB66B00)
}

// ---- the native headers
pub(crate) fn native_header_read(header: u64) -> Option<NativeHeader> {
    crate::definition_header::header_read(header).map(|h| NativeHeader { data: h.data, capacity: h.capacity, count: h.count })
}
/// NonemptyCertifiedHeader.
pub(crate) fn nonempty_certified_header(header: u64, tree: &mut Tree) -> bool {
    let Some(roots) = crate::definition_header::header_roots(base(), header) else { return false };
    tree.capture(&roots, base(), Some(&mut reader()), true) && crate::definition_wrappers::tree_certified(tree) && tree.matches_before(&mut reader())
}
fn disjoint_view(t: &Tree) -> crate::definition_disjoint::Tree<'_> {
    crate::definition_disjoint::Tree { nodes: t.nodes(), arrays: t.arrays(), strings: t.strings() }
}
pub(crate) fn trees_disjoint(first: &Tree, second: &Tree) -> bool {
    crate::definition_disjoint::trees_disjoint(&disjoint_view(first), &disjoint_view(second))
}
fn invoke_saved_destroy(header: u64) -> bool {
    // SAFETY: the engine's destroy over the saved header; a fault is caught.
    unsafe { cvr_engine::code::guarded_call4((base() + 0x8E7704) as usize, header as *mut c_void, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) }.is_some()
}
fn write_empty_native_header(header: u64, allocator: u64) -> bool {
    cvr_engine::code::write::<u64>((header + 8) as usize, 0) && cvr_engine::code::write::<u64>(header as usize, allocator)
}
fn saved_vector_matches(owner: &RetainedOwner) -> bool {
    let header = &owner.identity.saved_native;
    crate::definition_reads::saved_vector(header.data, header.capacity, header.count, owner.identity.empty_allocator.data, &owner.definitions)
}

// ---- the retirement
/// FatalProtectedIntent.
pub(crate) fn fatal_protected_intent(s: &mut State, owner: *mut RetainedOwner, detail: u64) -> NativeIntentResult {
    if let Some(state) = s.rs(owner) {
        state.unsupported_extra_completion(0);
    }
    WRITER_COVERAGE_LOST.store(true, SeqCst);
    x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
    let _write = TelemetryWrite::new();
    let fields = owned_stats();
    fields[0] = 6;
    fields[22] = u64::from(owned_failure::RETIREMENT_INVARIANT);
    fields[23] = detail;
    // SAFETY: a boxed owner under the mutex.
    let o = unsafe { &*owner };
    let failure = [detail, o.identity.owner_header, o.identity.generation, o.identity.born_epoch, o.last_consumer_id, o.last_epoch, o.last_pair_generation, PROTECTED_ATTACHED_OWNERS.load(SeqCst)];
    // SAFETY: the export.
    unsafe { *out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirementFailure)) = failure };
    totals()[10] += 1;
    retirement_event(s, RetireEvent::Quarantine, owner, u64::from(owned_failure::RETIREMENT_INVARIANT), 0, 0, 0, 0, 0, 0, 0);
    NativeIntentResult::FatalInvariant
}
/// FailClosedIntentException.
pub(crate) fn fail_closed_intent_exception(header: u64, detail: u64) {
    if PROTECTED_ATTACHED_OWNERS.load(SeqCst) == 0 && OWNED_ADMISSION_WINDOWS.load(SeqCst) == 0 && ALLOCATOR_SERVICES_ACTIVE.load(SeqCst) == 0 {
        return;
    }
    let _write = TelemetryWrite::new();
    // SAFETY: the export.
    let fields = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirementFailure)) };
    fields[0] = detail;
    fields[1] = header;
    fields[2..7].fill(0);
    let sequence = LAST_PROTECTION_SEQ.load(SeqCst);
    let observation: [u64; 6] = core::array::from_fn(|i| LAST_PROTECTION_IDENTITY[i].load(SeqCst));
    if sequence & 1 == 0 && sequence == LAST_PROTECTION_SEQ.load(SeqCst) && observation[0] == header {
        fields[2..7].copy_from_slice(&observation[1..6]);
    }
    fields[7] = PROTECTED_ATTACHED_OWNERS.load(SeqCst);
    let stats = owned_stats();
    stats[0] = 6;
    stats[22] = u64::from(owned_failure::RETIREMENT_INVARIANT);
    stats[23] = header;
    x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
    fail_fast();
}
/// HandleNativeIntentLocked.
pub(crate) fn handle_native_intent_locked(s: &mut State, header: u64, kind: rd::IntentKind, supported_scope: bool, caller: u32) -> NativeIntentResult {
    let Some(stamp) = s.mutations.get(&header).copied() else { return NativeIntentResult::NoAttachedOwner };
    if !stamp.service_owner.is_null() {
        return fatal_protected_intent(s, stamp.service_owner, 4);
    }
    let mut attached: *mut RetainedOwner = core::ptr::null_mut();
    for owner in s.owner_ptrs() {
        // SAFETY: boxed owners under the mutex.
        let o = unsafe { &*owner };
        if o.identity.owner_header == header && !o.emergency_detached && s.rs_ref(owner).is_some_and(|state| !state.exact_header_detached()) {
            attached = owner;
        }
    }
    if attached.is_null() {
        return NativeIntentResult::NoAttachedOwner;
    }
    {
        let _write = TelemetryWrite::new();
        totals()[4] += 1;
        retirement_event(s, RetireEvent::Intent, attached, 0, header, stamp.storage, stamp.epoch, u64::from(caller), 0, 0, 0);
    }
    // SAFETY: as above.
    let identity = unsafe { (*attached).identity };
    let before = native_header_read(header);
    if identity.generation != stamp.generation
        || identity.born_epoch != stamp.epoch
        || !supported_scope
        || stamp.builders != 0
        || stamp.operations != 0
        || stamp.in_flight != 0
        || !stamp.storage_alive
        || !stamp.builder_closed
        || before != Some(identity.saved_native)
    {
        return fatal_protected_intent(s, attached, 1);
    }
    let before = before.unwrap_or_default();
    // Everything needed for prolongation was reserved at admission. No tree recapture, allocation or native release
    // may occur in this handoff.
    // SAFETY: as above.
    let infrastructure = crate::definition_wrappers::allocators_match(unsafe { &(*attached).tree }, &mut reader());
    let intent = {
        let state = s.rs(attached).expect("an attached owner's state");
        let allocator = rd::AllocatorProof { identity: *state.frozen_allocator(), infrastructure_matches: infrastructure, alive: true };
        state.observe_native_intent(kind, &identity, rd::NativeIntentFacts { exact_native_retirement_point: true, layout_audited: true, unique_writer: true, destructor_tree_safe: true }, &allocator)
    };
    let empty = identity.empty_allocator;
    if !write_empty_native_header(header, empty.data) {
        return fatal_protected_intent(s, attached, 2);
    }
    let after = native_header_read(header);
    if after != Some(empty) {
        return fatal_protected_intent(s, attached, 3);
    }
    // SAFETY: as above.
    let o = unsafe { &mut *attached };
    if o.protected_attached {
        o.protected_attached = false;
        PROTECTED_ATTACHED_OWNERS.fetch_sub(1, SeqCst);
    }
    o.emergency_detached = true;
    let transferred = intent && s.rs(attached).is_some_and(|state| state.record_header_transfer(&rd::HeaderTransferProof { source_before: identity, saved_copy: before, source_after: empty, successful_exact_transfer: true }));
    if !transferred {
        if let Some(state) = s.rs(attached) {
            state.unsupported_extra_completion(0);
        }
        WRITER_COVERAGE_LOST.store(true, SeqCst);
        x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
        let _write = TelemetryWrite::new();
        totals()[10] += 1;
        retirement_event(s, RetireEvent::Quarantine, attached, u64::from(owned_failure::RETIREMENT_INVARIANT), header, stamp.storage, stamp.epoch, u64::from(caller), 0, 0, 0);
        return NativeIntentResult::Quarantined;
    }
    o.emergency_detached = false;
    {
        let _write = TelemetryWrite::new();
        totals()[5] += 1;
        retirement_event(s, RetireEvent::Detach, attached, 0, header, stamp.storage, stamp.epoch, u64::from(caller), 0, 0, 0);
        publish_retirement_receipt(s);
    }
    NativeIntentResult::Detached
}
/// BeginMutationLocked.
pub(crate) fn begin_mutation_locked(s: &mut State, header: u64, kind: u32) -> u64 {
    let counters = &x::CyberpunkVR_PreparationDefinitionMutationCounters;
    counters[0].fetch_add(1, SeqCst);
    counters[kind as usize].fetch_add(1, SeqCst);
    if header == 0 || (s.mutations.len() >= MAX_TRACKED_OWNERS && !s.mutations.contains_key(&header)) {
        MUTATION_OVERFLOW.store(true, SeqCst);
        counters[3].fetch_add(1, SeqCst);
        return 0;
    }
    s.next_mutation += 1;
    let next = s.next_mutation;
    let stamp = s.mutations.entry(header).or_default();
    let (previous_epoch, previous_generation) = (stamp.epoch, stamp.generation);
    stamp.epoch = next;
    stamp.generation = 0;
    stamp.in_flight += 1;
    stamp.kind = kind;
    let epoch = stamp.epoch;
    if previous_generation != 0 {
        if s.generation_ledger.close_source(previous_generation, previous_epoch, epoch) {
            s.closed_generations.push((previous_generation, previous_epoch));
            x::CyberpunkVR_PreparationGenerationStats[4].fetch_add(1, SeqCst);
        } else {
            x::CyberpunkVR_PreparationGenerationStats[15].store(s.generation_ledger.rejection() as u64, SeqCst);
        }
    }
    publish_generation_stats(s);
    epoch
}
/// EndMutation.
pub(crate) fn end_mutation(header: u64, token: u64) {
    if token == 0 {
        return;
    }
    let g = lock();
    let s = g.state();
    match s.mutations.get_mut(&header) {
        Some(stamp) if stamp.in_flight != 0 => stamp.in_flight -= 1,
        _ => MUTATION_OVERFLOW.store(true, SeqCst),
    }
}
/// FindRetainedOwner.
pub(crate) fn find_retained_owner(s: &mut State, identity: &HeaderIdentity) -> *mut RetainedOwner {
    s.owner_ptrs().into_iter().find(|&owner| {
        // SAFETY: a boxed owner under the mutex.
        unsafe { (*owner).identity == *identity }
    }).unwrap_or(core::ptr::null_mut())
}
/// CancelExtraReaders.
pub(crate) fn cancel_extra_readers(s: &mut State, consumer: &mut Consumer, restored: bool) {
    let mut kept = Vec::with_capacity(consumer.extra_readers.len());
    for reader in core::mem::take(&mut consumer.extra_readers) {
        let proof = rd::UnescapedCancellationProof { reader: reader.identity, native_original_never_invoked: true, verified_original_slot_restored: restored, no_native_escape: true };
        let canceled = s.rs(reader.owner).is_some_and(|state| state.cancel_unescaped_extra_reader(&proof));
        if canceled {
            let _write = TelemetryWrite::new();
            totals()[3] += 1;
            event(s, RetireEvent::Cancel, reader.owner);
        } else {
            kept.push(reader);
        }
    }
    consumer.extra_readers = kept;
}
fn consumer_references_owner(s: &State, owner: *const RetainedOwner) -> bool {
    s.consumers.values().any(|consumer| {
        // SAFETY: under the mutex.
        unsafe { consumer.get() }.extra_readers.iter().any(|reader| reader.owner as *const _ == owner)
    })
}
/// RecycleRetainedMetadata.
pub(crate) fn recycle_retained_metadata(s: &mut State) {
    for slot in 0..s.retained_owners.len() {
        let Some(owner) = s.retained_owners[slot].as_deref_mut().map(|o| o as *mut RetainedOwner) else { continue };
        // SAFETY: a boxed owner under the mutex.
        let o = unsafe { &mut *owner };
        if o.state.is_none() || o.destroy_in_flight || consumer_references_owner(s, owner) {
            continue;
        }
        let identity = o.identity;
        let Some(state) = s.rs_ref(owner) else { continue };
        let destroyed = state.can_retire_metadata();
        if !destroyed && !state.can_abandon_unused_reservation() {
            continue;
        }
        let retired = if destroyed {
            s.retirement_states.retire_metadata(identity.owner_header, identity.generation, identity.born_epoch)
        } else {
            s.retirement_states.abandon_unused_reservation(identity.owner_header, identity.generation, identity.born_epoch)
        };
        if retired {
            let _write = TelemetryWrite::new();
            totals()[if destroyed { 8 } else { 9 }] += 1;
            o.state = None;
            event(s, if destroyed { RetireEvent::Recycle } else { RetireEvent::Abandon }, owner);
            if o.protected_attached {
                PROTECTED_ATTACHED_OWNERS.fetch_sub(1, SeqCst);
            }
            s.retained_owners[slot] = None;
        }
    }
}

// ---- the allocator domain
fn lifecycle(s: &State) -> Lifecycle {
    let d = &s.allocator_domain;
    Lifecycle { incarnation: d.incarnation, reset_epoch: d.reset_epoch, in_flight: d.in_flight, closing: d.closing || closing(), coverage_complete: allocator_coverage_ready() }
}
/// PublishAllocatorReceipt.
pub(crate) fn publish_allocator_receipt(s: &State, receipt: Option<&ab::Receipt>, tree_nodes: u64) {
    let _write = TelemetryWrite::new();
    // SAFETY: the export.
    let fields = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionAllocator)) };
    fields.fill(0);
    let base = base();
    let common = base + ab::COMMON_RVA;
    let d = &s.allocator_domain;
    fields[0] = if d.closing || closing() {
        2
    } else if receipt.is_some_and(|r| r.rejection() != ab::Failure::None) {
        3
    } else if d.ready {
        1
    } else {
        0
    };
    fields[1] = d.incarnation;
    fields[2] = d.reset_epoch;
    fields[3] = u64::from(d.in_flight);
    fields[4] = d.services_active;
    fields[5] = allocator_coverage_ready() as u64;
    fields[6] = common;
    let mut field = |at: usize, address: u64| {
        let mut bytes = [0u8; 8];
        read(address, &mut bytes);
        fields[at] = u64::from_le_bytes(bytes);
    };
    field(7, base + crate::native::definition_tree::OBJECT_ALLOCATOR_RVA);
    field(8, base + crate::native::definition_tree::CONTROL_ALLOCATOR_RVA);
    field(9, base + crate::native::definition_tree::STRING_ALLOCATOR_RVA);
    field(10, common + ab::POOL_OFFSET);
    field(11, common + ab::SMALL_BEGIN_OFFSET);
    field(12, common + ab::SMALL_END_OFFSET);
    field(13, common + ab::MIDDLE_BEGIN_OFFSET);
    field(14, common + ab::MIDDLE_END_OFFSET);
    if let Some(receipt) = receipt {
        fields[15] = receipt.allocations().len() as u64;
        for allocation in receipt.allocations() {
            fields[match allocation.region {
                ab::Region::Small => 16,
                ab::Region::Middle => 17,
                _ => 18,
            }] += 1;
        }
        fields[20] = receipt.all_exact_extents() as u64;
        fields[21] = receipt.rejection() as u64;
    }
    fields[19] = tree_nodes;
    fields[22] = d.close_reset_events;
    fields[23] = u64::from(d.last_entry);
}
/// CaptureAllocatorLocked.
pub(crate) fn capture_allocator_locked(s: &mut State, receipt: &mut ab::Receipt) -> bool {
    if s.allocator_domain.closing || closing() || s.allocator_domain.in_flight != 0 || !allocator_coverage_ready() {
        return false;
    }
    // This is a local identity episode anchored by fresh constructed-state observations, not an invented native
    // constructor or liveness event.
    if s.allocator_domain.incarnation == 0 {
        s.allocator_domain.incarnation = 1;
    }
    let tracking = lifecycle(s);
    let mut observe = |value: &mut Lifecycle| {
        *value = tracking;
        true
    };
    let facts = ab::LayoutFacts { globals_and_component_guarded: true, allocation_routes_guarded: true, exact_extent_facts_audited: false };
    if !receipt.capture(base(), facts, tracking, Some(&mut reader()), Some(&mut observe)) {
        publish_allocator_receipt(s, Some(receipt), 0);
        return false;
    }
    let d = &s.allocator_domain;
    if d.ready && (d.binding.identity() != receipt.identity() || d.binding.tracking() != receipt.tracking()) {
        s.allocator_domain.closing = true;
        x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
        s.retirement_states.shutdown();
        {
            let _write = TelemetryWrite::new();
            totals()[12] += 1;
            event(s, RetireEvent::AllocatorClose, core::ptr::null());
        }
        publish_allocator_receipt(s, Some(receipt), 0);
        return false;
    }
    if !s.allocator_domain.ready {
        s.allocator_domain.ready = true;
        s.allocator_domain.binding = receipt.clone();
        ALLOCATOR_READY_OBSERVED.store(true, SeqCst);
        let _write = TelemetryWrite::new();
        totals()[11] += 1;
        event(s, RetireEvent::AllocatorBound, core::ptr::null());
    }
    true
}
/// AllocatorBoundary.
fn allocator_boundary(entry: u32, reset: bool) -> bool {
    let g = lock();
    let s = g.state();
    let _write = TelemetryWrite::new();
    s.allocator_domain.last_entry = entry;
    s.allocator_domain.close_reset_events += 1;
    if s.allocator_domain.services_active != 0 {
        if let Some(owner) = s.owner_ptrs().into_iter().find(|&owner| {
            // SAFETY: boxed owners under the mutex.
            unsafe { (*owner).destroy_in_flight }
        }) {
            fatal_protected_intent(s, owner, 20);
        }
        fail_fast();
    }
    if let Some(owner) = s.owner_ptrs().into_iter().find(|&owner| s.rs_ref(owner).is_some_and(|state| state.pending_readers() != 0)) {
        fatal_protected_intent(s, owner, 21);
        fail_fast();
    }
    if reset {
        s.allocator_domain.reset_epoch += 1;
        s.allocator_domain.in_flight += 1;
        totals()[13] += 1;
        retirement_event(s, RetireEvent::AllocatorReset, core::ptr::null(), 0, 0, 0, 0, u64::from(entry), 0, 0, 0);
    }
    if !reset || s.allocator_domain.ready {
        s.allocator_domain.closing = true;
        x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
        s.retirement_states.shutdown();
        totals()[12] += 1;
        retirement_event(s, RetireEvent::AllocatorClose, core::ptr::null(), 0, 0, 0, 0, u64::from(entry), 0, 0, 0);
    } else {
        s.allocator_domain.incarnation += 1;
    }
    publish_allocator_receipt(s, None, 0);
    publish_retirement_receipt(s);
    true
}
/// AllocatorResetReturned.
fn allocator_reset_returned() {
    let g = lock();
    let s = g.state();
    if s.allocator_domain.in_flight != 0 {
        s.allocator_domain.in_flight -= 1;
    } else {
        s.allocator_domain.closing = true;
        x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
    }
    publish_allocator_receipt(s, None, 0);
}
fn common_component(object: *mut c_void, offset: u64) -> bool {
    object as u64 == base() + ab::COMMON_RVA + offset
}
macro_rules! original {
    ($slot:ident: fn($($t:ty),*) -> $r:ty) => {
        // SAFETY: MinHook's trampoline to the original, set before the detour is enabled.
        unsafe { core::mem::transmute::<usize, extern "C" fn($($t),*) -> $r>($slot) }
    };
}
pub(crate) use original;
static mut ALLOCATOR_CLOSE: usize = 0;
static mut ALLOCATOR_CONSTRUCT: usize = 0;
static mut ALLOCATOR_SETUP: usize = 0;
static mut SMALL_CONSTRUCT: usize = 0;
static mut SMALL_DESTROY: usize = 0;
static mut SMALL_SCALAR: usize = 0;
static mut MIDDLE_CONSTRUCT: usize = 0;
static mut MIDDLE_SETUP: usize = 0;
static mut MIDDLE_RANGES: usize = 0;
static mut FALLBACK_CONSTRUCT: usize = 0;
static mut FALLBACK_SETUP: usize = 0;
extern "C" fn allocator_close_hook() {
    if allocator_boundary(0x23999B0, false) {
        original!(ALLOCATOR_CLOSE: fn() -> ())();
    }
}
extern "C" fn allocator_construct_hook() -> usize {
    if !allocator_boundary(0x260E2B4, true) {
        return 0;
    }
    let result = original!(ALLOCATOR_CONSTRUCT: fn() -> usize)();
    allocator_reset_returned();
    result
}
extern "C" fn allocator_setup_hook(unused: *mut c_void, config: *mut c_void) -> usize {
    if !allocator_boundary(0x260E630, true) {
        return 0;
    }
    let result = original!(ALLOCATOR_SETUP: fn(*mut c_void, *mut c_void) -> usize)(unused, config);
    allocator_reset_returned();
    result
}
extern "C" fn small_construct_hook(pool: *mut c_void) -> usize {
    let tracked = common_component(pool, 0xC928);
    if tracked && !allocator_boundary(0x260E358, true) {
        return 0;
    }
    let result = original!(SMALL_CONSTRUCT: fn(*mut c_void) -> usize)(pool);
    if tracked {
        allocator_reset_returned();
    }
    result
}
extern "C" fn small_destroy_hook(pool: *mut c_void) {
    if !common_component(pool, 0xC928) || allocator_boundary(0x260E3DC, false) {
        original!(SMALL_DESTROY: fn(*mut c_void) -> ())(pool);
    }
}
extern "C" fn small_scalar_hook(pool: *mut c_void, flags: u32) -> usize {
    if common_component(pool, 0xC928) && !allocator_boundary(0x260E410, false) {
        return 0;
    }
    original!(SMALL_SCALAR: fn(*mut c_void, u32) -> usize)(pool, flags)
}
fn arena_hook(slot: usize, entry: u32, arena: *mut c_void, second: *mut c_void, offset: u64) -> usize {
    let tracked = common_component(arena, offset);
    if tracked && !allocator_boundary(entry, true) {
        return 0;
    }
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(slot) };
    let result = original(arena, second);
    if tracked {
        allocator_reset_returned();
    }
    result
}
extern "C" fn middle_construct_hook(arena: *mut c_void, config: *mut c_void) -> usize {
    // SAFETY: the trampoline's value.
    arena_hook(unsafe { MIDDLE_CONSTRUCT }, 0x68CBA4, arena, config, 0xCD80)
}
extern "C" fn middle_setup_hook(arena: *mut c_void, range: *mut c_void) -> usize {
    // SAFETY: as above.
    arena_hook(unsafe { MIDDLE_SETUP }, 0x68CA94, arena, range, 0xCD80)
}
extern "C" fn middle_ranges_hook(arena: *mut c_void, range: *mut c_void) -> usize {
    // SAFETY: as above.
    arena_hook(unsafe { MIDDLE_RANGES }, 0x68CACC, arena, range, 0xCD80)
}
extern "C" fn fallback_construct_hook(arena: *mut c_void) -> usize {
    let tracked = common_component(arena, 0xCE40);
    if tracked && !allocator_boundary(0x260E9DC, true) {
        return 0;
    }
    let result = original!(FALLBACK_CONSTRUCT: fn(*mut c_void) -> usize)(arena);
    if tracked {
        allocator_reset_returned();
    }
    result
}
extern "C" fn fallback_setup_hook(arena: *mut c_void, config: *mut c_void) -> usize {
    // SAFETY: as above.
    arena_hook(unsafe { FALLBACK_SETUP }, 0x260EAEC, arena, config, 0xCE40)
}

// ---- the writers
/// ServiceReadyRetirements.
fn service_ready_retirements(current_header: u64, supported_scope: bool, scope_epoch: u64, caller: u32) {
    if !supported_scope || closing() || RETIREMENT_BYPASS.with(Cell::get) != 0 {
        return;
    }
    let mut current = Tree::default();
    let current_native;
    let mut current_allocator = ab::Receipt::default();
    let mut candidates: Vec<(usize, HeaderIdentity)> = Vec::new();
    {
        let g = lock();
        let s = g.state();
        let any = s.owner_ptrs().into_iter().any(|owner| {
            // SAFETY: boxed owners under the mutex.
            let o = unsafe { &*owner };
            o.state.is_some() && o.ready && !o.destroy_in_flight
        });
        if !any {
            return;
        }
        let Some(found) = s.mutations.get(&current_header).copied() else { return };
        if !found.storage_alive || found.epoch != scope_epoch || found.builders > 1 || found.operations != 0 || found.in_flight != 1 || !allocator_coverage_ready() || s.allocator_domain.closing {
            return;
        }
        let Some(native) = native_header_read(current_header) else { return };
        current_native = native;
        if !nonempty_certified_header(current_header, &mut current) || !capture_allocator_locked(s, &mut current_allocator) {
            return;
        }
        if !current_allocator.bind_saved(NativeVector16 { data: current_native.data, capacity: current_native.capacity, count: current_native.count }, &current, &[]) {
            return;
        }
        for owner in s.owner_ptrs() {
            // SAFETY: as above.
            let o = unsafe { &*owner };
            if o.identity.owner_header == current_header && !o.emergency_detached && s.rs_ref(owner).is_some_and(|state| !state.exact_header_detached()) {
                return;
            }
        }
        for (i, slot) in s.retained_owners.iter().enumerate() {
            if let Some(o) = slot {
                if o.state.is_some() && o.ready && !o.destroy_in_flight {
                    candidates.push((i, o.identity));
                }
            }
        }
    }
    for (slot, identity) in candidates {
        let saved: Option<*mut NativeHeader>;
        let owner: *mut RetainedOwner;
        {
            let g = lock();
            let s = g.state();
            let Some(holder) = s.retained_owners[slot].as_deref_mut() else { continue };
            if holder.identity != identity || holder.state.is_none() || !holder.ready || holder.destroy_in_flight {
                continue;
            }
            owner = holder as *mut RetainedOwner;
            // SAFETY: a boxed owner under the mutex.
            let o = unsafe { &mut *owner };
            let scope = s.mutations.get(&current_header).copied();
            let mut fresh = ab::Receipt::default();
            let ok = !closing()
                && scope.is_some_and(|scope| scope.epoch == scope_epoch && scope.in_flight == 1 && scope.builders <= 1 && scope.operations == 0)
                && current.matches_before(&mut reader())
                && crate::definition_wrappers::allocators_match(&current, &mut reader())
                && o.completion_flags == 0x1FF
                && o.declaration_group != 0
                && o.execution_group != 0
                && o.compiler_group != 0
                && native_header_read(current_header) == Some(current_native)
                && o.tree.matches_before(&mut reader())
                && crate::definition_wrappers::allocators_match(&o.tree, &mut reader())
                && saved_vector_matches(o)
                && trees_disjoint(&o.tree, &current)
                && capture_allocator_locked(s, &mut fresh)
                && o.allocator_binding.matches_fresh(&fresh)
                && current_allocator.matches_fresh(&fresh)
                && crate::definition_disjoint::allocations_disjoint(o.allocator_binding.allocations(), current_allocator.allocations());
            if !ok {
                continue;
            }
            let state = s.rs(owner).expect("a ready owner's state");
            let allocator = rd::AllocatorProof { identity: *state.frozen_allocator(), infrastructure_matches: true, alive: true };
            saved = state.begin_destroy(&allocator);
            if saved.is_some() {
                o.destroy_in_flight = true;
                let stamp = s.mutations.get_mut(&current_header).expect("the scope's stamp");
                stamp.service_owner = owner;
                stamp.service_epoch = scope_epoch;
                stamp.service_storage = stamp.storage;
                let storage = stamp.storage;
                s.allocator_domain.services_active += 1;
                ALLOCATOR_SERVICES_ACTIVE.fetch_add(1, SeqCst);
                s.allocator_domain.service_certificate_observed = true;
                o.service_header = current_header;
                o.service_storage = storage;
                o.service_epoch = scope_epoch;
                o.service_caller = u64::from(caller);
                let _write = TelemetryWrite::new();
                totals()[6] += 1;
                retirement_event(s, RetireEvent::DestroyBegin, owner, 0, current_header, storage, scope_epoch, u64::from(caller), 0, 0, 0);
                publish_allocator_receipt(s, Some(&o.allocator_binding), o.tree.nodes().len() as u64);
                publish_retirement_receipt(s);
            }
        }
        let Some(saved) = saved else { continue };
        // BeginDestroy plus destroyInFlight is a committed permit under the same gate as Close. Closing prevents later
        // permits; this one finishes once. Only plain pointers/scalars enter this certified native-use interval.
        RETIREMENT_CALL_OWNER.with(|c| c.set(owner));
        RETIREMENT_CALL_HEADER.with(|c| c.set(saved as u64));
        RETIREMENT_CALL_SCOPE.with(|c| c.set(scope_epoch));
        RETIREMENT_BYPASS.with(|c| c.set(c.get() + 1));
        let returned = invoke_saved_destroy(saved as u64);
        RETIREMENT_BYPASS.with(|c| c.set(c.get() - 1));
        RETIREMENT_CALL_OWNER.with(|c| c.set(core::ptr::null_mut()));
        RETIREMENT_CALL_HEADER.with(|c| c.set(0));
        RETIREMENT_CALL_SCOPE.with(|c| c.set(0));
        let g = lock();
        let s = g.state();
        let scope_ok = s.mutations.get(&current_header).is_some_and(|scope| scope.service_owner == owner && scope.service_epoch == scope_epoch && scope.service_storage == scope.storage);
        if scope_ok {
            let scope = s.mutations.get_mut(&current_header).expect("checked");
            scope.service_owner = core::ptr::null_mut();
            scope.service_epoch = 0;
            scope.service_storage = 0;
        } else {
            fatal_protected_intent(s, owner, 8);
            fail_fast();
        }
        let finished = s.rs(owner).is_some_and(|state| state.finish_destroy(returned));
        // SAFETY: a boxed owner under the mutex.
        let o = unsafe { &mut *owner };
        o.ready = false;
        o.destroy_in_flight = false;
        s.allocator_domain.services_active -= 1;
        ALLOCATOR_SERVICES_ACTIVE.fetch_sub(1, SeqCst);
        let _write = TelemetryWrite::new();
        totals()[if finished { 7 } else { 10 }] += 1;
        retirement_event(s, RetireEvent::DestroyReturn, owner, if finished { 0 } else { u64::from(owned_failure::RETIREMENT_INVARIANT) }, current_header, o.service_storage, scope_epoch, u64::from(caller), 0, 0, 0);
        publish_allocator_receipt(s, Some(&o.allocator_binding), o.tree.nodes().len() as u64);
        publish_retirement_receipt(s);
    }
}
static mut RESET: usize = 0;
extern "C" fn reset_hook(builder: *mut c_void, graph: *mut c_void, header: *mut c_void) -> *mut c_void {
    let caller = return_address();
    if RETIREMENT_BYPASS.with(Cell::get) != 0 {
        let g = lock();
        let s = g.state();
        let owner = RETIREMENT_CALL_OWNER.with(Cell::get);
        if !owner.is_null() {
            fatal_protected_intent(s, owner, 9);
        }
        fail_fast();
    }
    let base = base();
    let site = BUILDER_SITES.iter().find(|row| caller == base + u64::from(row[0]));
    let (b, g_, h) = (builder as u64, graph as u64, header as u64);
    let layout = b != 0 && b & 7 == 0 && b <= u64::MAX - 0xF8 && g_ != 0 && g_ & 7 == 0 && g_ <= u64::MAX - 0x88 && h == g_ + 0x70 && !(b < g_ + 0x88 && g_ < b + 0xF8);
    let supported = site.is_some() && layout && writer_callsites_compatible();
    let caller32 = caller.wrapping_sub(base) as u32;
    let token = {
        let g = lock();
        let s = g.state();
        if handle_native_intent_locked(s, h, rd::IntentKind::Reset, supported, caller32) == NativeIntentResult::FatalInvariant {
            fail_fast();
        }
        let token = begin_mutation_locked(s, h, 1);
        if let Some(stamp) = s.mutations.get_mut(&h) {
            stamp.builder_closed = false;
            let slot = stamp.slot as usize;
            if !supported || !stamp.storage_alive || stamp.builders != 0 || stamp.operations != 0 || slot >= s.builders.len() || s.builders[slot].address != 0 {
                WRITER_COVERAGE_LOST.store(true, SeqCst);
            } else {
                // SAFETY: a plain thread id query.
                let thread = u64::from(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() });
                s.builders[slot] = BuilderReservation { address: b, header: h, epoch: token, thread, end_return: site.map_or(0, |row| row[1]) };
            }
            stamp.builders += 1;
        }
        token
    };
    service_ready_retirements(h, supported, token, caller32);
    original!(RESET: fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void)(builder, graph, header)
}
static mut DESTROY: usize = 0;
extern "C" fn destroy_hook(header: *mut c_void) {
    let caller = return_address();
    if RETIREMENT_BYPASS.with(Cell::get) != 0 {
        let owner = RETIREMENT_CALL_OWNER.with(Cell::get);
        if !owner.is_null() && RETIREMENT_CALL_SCOPE.with(Cell::get) != 0 && header as u64 == RETIREMENT_CALL_HEADER.with(Cell::get) {
            original!(DESTROY: fn(*mut c_void) -> ())(header);
            return;
        }
        let g = lock();
        let s = g.state();
        if !owner.is_null() {
            fatal_protected_intent(s, owner, 10);
        }
        fail_fast();
    }
    let base = base();
    let h = header as u64;
    let supported = (caller == base + 0x8E7C6D || caller == base + 0x8E767B || caller == base + 0x24E88AD) && writer_callsites_compatible();
    let caller32 = caller.wrapping_sub(base) as u32;
    let token = {
        let g = lock();
        let s = g.state();
        if handle_native_intent_locked(s, h, rd::IntentKind::Destroy, supported, caller32) == NativeIntentResult::FatalInvariant {
            fail_fast();
        }
        begin_mutation_locked(s, h, 2)
    };
    service_ready_retirements(h, supported, token, caller32);
    original!(DESTROY: fn(*mut c_void) -> ())(header);
    end_mutation(h, token);
    let g = lock();
    if let Some(stamp) = g.state().mutations.get_mut(&h) {
        stamp.storage_alive = false;
        stamp.builder_closed = false;
    }
}
static mut BIRTH: usize = 0;
extern "C" fn birth_hook(output: *mut c_void) -> *mut c_void {
    let caller = return_address();
    let result = original!(BIRTH: fn(*mut c_void) -> *mut c_void)(output);
    let base = base();
    let g = lock();
    let s = g.state();
    let graph = read_u64(output as u64);
    let Some(graph) = graph.filter(|&graph| caller == base + 0x8E7C1C && graph != 0 && graph & 7 == 0 && graph <= u64::MAX - 0x88) else {
        WRITER_COVERAGE_LOST.store(true, SeqCst);
        return result;
    };
    let header = graph + 0x70;
    if let Some(collision) = s.mutations.get(&header).map(|stamp| stamp.service_owner).filter(|owner| !owner.is_null()) {
        fatal_protected_intent(s, collision, 5);
        fail_fast();
    }
    if native_header_read(header) != Some(NativeHeader { data: base + 0x30947E8, capacity: 0, count: 0 }) {
        WRITER_COVERAGE_LOST.store(true, SeqCst);
        return result;
    }
    let slot = match s.mutations.get(&header) {
        None => {
            if s.storage_slots >= MAX_TRACKED_OWNERS as u64 {
                WRITER_COVERAGE_LOST.store(true, SeqCst);
                return result;
            }
            s.storage_slots += 1;
            (s.storage_slots - 1) as u32
        }
        Some(found) => {
            if found.builders != 0 || found.operations != 0 || found.in_flight != 0 {
                WRITER_COVERAGE_LOST.store(true, SeqCst);
                return result;
            }
            found.slot
        }
    };
    if s.next_storage == u64::MAX {
        WRITER_COVERAGE_LOST.store(true, SeqCst);
        return result;
    }
    let stamp = s.mutations.entry(header).or_default();
    if !stamp.service_owner.is_null() {
        let owner = stamp.service_owner;
        fatal_protected_intent(s, owner, 5);
        fail_fast();
    }
    s.next_storage += 1;
    *stamp = Stamp { slot, storage: s.next_storage, storage_alive: true, ..Stamp::default() };
    result
}
static mut END_BUILDER: usize = 0;
extern "C" fn end_builder_hook(builder: *mut c_void) {
    let caller = return_address();
    let b = builder as u64;
    let mut reservation = BuilderReservation::default();
    {
        let g = lock();
        let s = g.state();
        if let Some(active) = s.builders.iter().find(|active| active.address == b) {
            reservation = *active;
        }
        if let Some(owner) = s.mutations.get(&reservation.header).map(|stamp| stamp.service_owner).filter(|owner| !owner.is_null()) {
            fatal_protected_intent(s, owner, 6);
            fail_fast();
        }
        // SAFETY: a plain thread id query.
        let thread = u64::from(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() });
        if reservation.address == 0 || caller != base() + u64::from(reservation.end_return) || reservation.thread != thread {
            WRITER_COVERAGE_LOST.store(true, SeqCst);
        }
    }
    original!(END_BUILDER: fn(*mut c_void) -> ())(builder);
    if reservation.address != 0 {
        let g = lock();
        let s = g.state();
        let lost = match s.mutations.get(&reservation.header) {
            None => true,
            Some(found) => {
                found.slot as usize >= s.builders.len()
                    || s.builders[found.slot as usize].address != b
                    || found.epoch != reservation.epoch
                    || found.builders == 0
                    || found.in_flight == 0
            }
        };
        if lost {
            WRITER_COVERAGE_LOST.store(true, SeqCst);
        } else {
            let stamp = s.mutations.get_mut(&reservation.header).expect("checked");
            stamp.builders -= 1;
            stamp.in_flight -= 1;
            stamp.builder_closed = stamp.builders == 0 && stamp.operations == 0;
            let slot = stamp.slot as usize;
            s.builders[slot] = BuilderReservation::default();
        }
    }
}
static mut APPEND: usize = 0;
extern "C" fn append_hook(header: *mut c_void, unique: *mut c_void) {
    let caller = return_address();
    let h = header as u64;
    let base = base();
    let mut tracked = false;
    {
        let g = lock();
        let s = g.state();
        match s.mutations.get(&h).copied() {
            Some(stamp) => {
                if !stamp.service_owner.is_null() {
                    fatal_protected_intent(s, stamp.service_owner, 7);
                    fail_fast();
                }
                for owner in s.owner_ptrs() {
                    // SAFETY: boxed owners under the mutex.
                    let o = unsafe { &*owner };
                    if o.identity.owner_header == h && !o.emergency_detached && s.rs_ref(owner).is_some_and(|state| state.pending_readers() != 0 && !state.exact_header_detached()) {
                        fatal_protected_intent(s, owner, 11);
                        fail_fast();
                    }
                }
                // SAFETY: a plain thread id query.
                let thread = u64::from(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() });
                let slot = stamp.slot as usize;
                if (caller != base + 0x8E9484 && caller != base + 0x8E957A && caller != base + 0x8E9779)
                    || stamp.builders == 0
                    || slot >= s.builders.len()
                    || s.builders[slot].header != h
                    || s.builders[slot].thread != thread
                {
                    WRITER_COVERAGE_LOST.store(true, SeqCst);
                }
                s.mutations.get_mut(&h).expect("present").operations += 1;
                tracked = true;
            }
            None => WRITER_COVERAGE_LOST.store(true, SeqCst),
        }
    }
    original!(APPEND: fn(*mut c_void, *mut c_void) -> ())(header, unique);
    if tracked {
        let g = lock();
        match g.state().mutations.get_mut(&h) {
            Some(stamp) if stamp.operations != 0 => stamp.operations -= 1,
            _ => WRITER_COVERAGE_LOST.store(true, SeqCst),
        }
    }
}

// ---- the registration
extern "C" {
    /// The asm thunks (src/Hooks/RenderGraphCacheThunk.asm) and their trampolines.
    fn CvrRenderGraphCacheResultDetour();
    static mut CvrRenderGraphCacheResultOriginal: *mut c_void;
    fn CvrResourceContextDetour();
    static mut CvrResourceContextOriginal: *mut c_void;
}
extern "C" fn cache_result_compatible() -> bool {
    code_compatible() && crate::definition_cache::cvr_ndt_cache_code(base())
}
extern "C" fn execute_compatible() -> bool {
    // SAFETY: the export.
    code_compatible() && unsafe { x::CyberpunkVR_PreparationGraphJobsCompatibility[0] } == 1
}
/// Registers the detours (the .cpp's static initialization, before the install pass).
fn register() {
    use cvr_engine::detours::register;
    let none = core::ptr::null_mut();
    macro_rules! reg {
        ($name:expr, $rva:expr, $hook:expr, $slot:expr, $wanted:expr) => {
            register($name, $rva, $hook as *const () as usize as *mut c_void, addr_of_mut!($slot).cast(), Some($wanted), none)
        };
    }
    reg!("[single-pass] native definition generation reset", 0x8E7D7C, reset_hook, RESET, code_compatible);
    reg!("[single-pass] native definition generation destroy", 0x8E7704, destroy_hook, DESTROY, code_compatible);
    reg!("[single-pass] native plain graph storage birth", 0x8E7CA8, birth_hook, BIRTH, code_compatible);
    reg!("[single-pass] full native definition builder end", 0x8EAF50, end_builder_hook, END_BUILDER, code_compatible);
    reg!("[single-pass] scoped native definition owner append", 0x8EA438, append_hook, APPEND, code_compatible);
    reg!("[single-pass] common allocator CRT close", 0x23999B0, allocator_close_hook, ALLOCATOR_CLOSE, code_compatible);
    reg!("[single-pass] common allocator construction boundary", 0x260E2B4, allocator_construct_hook, ALLOCATOR_CONSTRUCT, code_compatible);
    reg!("[single-pass] common allocator backing boundary", 0x260E630, allocator_setup_hook, ALLOCATOR_SETUP, code_compatible);
    reg!("[single-pass] common small pool construction", 0x260E358, small_construct_hook, SMALL_CONSTRUCT, code_compatible);
    reg!("[single-pass] common small pool close", 0x260E3DC, small_destroy_hook, SMALL_DESTROY, code_compatible);
    reg!("[single-pass] common small pool scalar close", 0x260E410, small_scalar_hook, SMALL_SCALAR, code_compatible);
    reg!("[single-pass] common middle arena construction", 0x68CBA4, middle_construct_hook, MIDDLE_CONSTRUCT, code_compatible);
    reg!("[single-pass] common middle arena setup", 0x68CA94, middle_setup_hook, MIDDLE_SETUP, code_compatible);
    reg!("[single-pass] common middle arena range writer", 0x68CACC, middle_ranges_hook, MIDDLE_RANGES, code_compatible);
    reg!("[single-pass] common fallback construction boundary", 0x260E9DC, fallback_construct_hook, FALLBACK_CONSTRUCT, code_compatible);
    reg!("[single-pass] common fallback setup boundary", 0x260EAEC, fallback_setup_hook, FALLBACK_SETUP, code_compatible);
    // The asm thunk's symbols.
    register("[single-pass] immediate native cache result", 0x40A1D1, CvrRenderGraphCacheResultDetour as *const () as usize as *mut c_void, addr_of_mut!(CvrRenderGraphCacheResultOriginal), Some(cache_result_compatible), none);
    replay::register_detours(none);
    consumers::register_detours(none, execute_compatible);
    register("[single-pass] source-bound asynchronous resource context", 0x333374, CvrResourceContextDetour as *const () as usize as *mut c_void, addr_of_mut!(CvrResourceContextOriginal), Some(code_compatible), none);
}

// ---- the C ABI
/// The registration (the .cpp's static initialization).
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_attach() {
    register();
}
/// definition_tracking::PrepareCodeGuards.
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_prepare_code_guards() {
    code_compatible();
    scene_code_compatible();
}
/// The asm thunk's call after the immediate native cache result (CvrRenderGraphCacheResult).
#[no_mangle]
pub extern "C" fn CvrRenderGraphCacheResult(slot: u64) {
    if root_depth() == 0 || root_depth() > 32 || closing() {
        return;
    }
    let g = lock();
    let _ = g.state();
    with_admission(|admission| {
        let mut miss = false;
        // SAFETY: the export; the admission is this thread's.
        let cache6 = unsafe { x::CyberpunkVR_OwnedExecutionCache[6] };
        if !unsafe { crate::definition_cache::cvr_ndt_cache_slot(admission, slot, cache6, closing(), &mut miss) } {
            return;
        }
        let _write = TelemetryWrite::new();
        // SAFETY: the exports.
        unsafe {
            crate::definition_cache::cvr_ndt_cache_record(
                admission,
                slot,
                miss,
                addr_of_mut!(x::CyberpunkVR_OwnedExecutionCache).cast(),
                addr_of_mut!(x::CyberpunkVR_OwnedExecutionCacheRows).cast(),
                addr_of_mut!(x::CyberpunkVR_OwnedExecutionRetirementTotals).cast(),
            )
        };
    });
}
/// definition_tracking::BeforeCacheLookup.
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_before_cache_lookup(cache: u64, use_: u32, key: u64, views: u32, missed: u64, caller: u64) {
    consumers::before_cache_lookup(cache, use_, key, views, missed, caller);
}
/// definition_tracking::CloseOwnedExecution (and CyberpunkVR_CloseOwnedExecution).
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_close() {
    consumers::close_owned_execution();
}
/// The C++ caller's sink for a copied declaration snapshot: (sink, nodes, node count, edges, edge count, failure).
pub type SnapshotSink = extern "C" fn(*mut c_void, *const crate::native::graph_snapshot::Node, usize, *const crate::native::graph_snapshot::Edge, usize, u32);
/// definition_tracking::CopyOwnedDeclarationSnapshot: on success the copy handed to `fill(sink, ...)`.
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_copy_declaration(consumer: u64, sink: *mut c_void, fill: SnapshotSink) -> bool {
    match consumers::copy_owned_declaration_snapshot(consumer) {
        Some(copy) => {
            fill(sink, copy.nodes.as_ptr(), copy.nodes.len(), copy.edges.as_ptr(), copy.edges.len(), copy.failure);
            true
        }
        None => false,
    }
}
