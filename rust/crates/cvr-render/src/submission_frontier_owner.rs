//! The submission frontier's owner (formerly src/Render/NativeSubmissionFrontier.cpp; its rules are
//! submission_frontier.rs): with the probe enabled, each call of the engine's range consumer (sub_1422701F0, reached
//! from its one caller) admitted under an epoch -- the range's slots and their contexts captured before the original
//! moves them, each publisher slot's native reset token taken (only under a qualified graph identity), rows of ranges,
//! slots and submissions published to the exports -- every submitted batch that names a held list matched against the
//! ledger of tokens, and the boundary of a batch found for the retained geometry (a contiguous native suffix after both
//! sources, every token still valid at the queue, the list array unchanged).
//!
//! The .cpp keeps the hook's own frame (its return address and the call's RAII), the graph identity it reads from the
//! owned declaration snapshot (definition tracking's C++ graph), and lends the exports and the trampoline through the
//! attached runtime.

use crate::code_tables::Row;
use crate::native_recording::{Context as RecordingContext, Lease};
use crate::submission_frontier::{
    capture_range, failure, matches, producer_slot, read as frontier_read, read_context, token_capture_allowed, Admission, AdmissionState, BoundaryBatch, BoundaryGraph, BoundaryList,
    BoundarySource, Key, Ledger, Slot, MAX_ROWS, MAX_SLOTS, MAX_SUBMIT_LISTS,
};
use crate::target_access::Boundary;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::*};
use std::sync::{Mutex, MutexGuard};

pub const HOOK_RVA: u64 = 0x22701F0;
pub const MAX_RANGES: u32 = 64;
const PUBLISHER_WORK: u64 = 0x332D5C;
/// detail::Failure past the rules' (theirs end at Context).
pub const CODE: u32 = 11;
pub const INSTALL: u32 = 12;
pub const CAPACITY: u32 = 13;
pub const EXCEPTION: u32 = 14;
/// detail::BoundaryFailure values the owner sets.
mod boundary_failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const DISABLED: u32 = 2;
    pub const BATCH: u32 = 3;
    pub const READ: u32 = 4;
    pub const METADATA: u32 = 5;
    pub const SOURCE: u32 = 6;
    pub const NATIVE_TOKEN: u32 = 14;
    pub const CHANGED: u32 = 15;
}

/// The graph identity the .cpp reads for a range (GraphIdentity's numbers; status 1 when none).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GraphIdentity {
    pub status: u32,
    pub consumer: u64,
    pub graph_epoch: u64,
    pub pair: u64,
    pub frame_owner: u64,
    pub fingerprint: u64,
    pub context: u64,
    pub definition: u64,
    pub sync_handle: u64,
}
impl Default for GraphIdentity {
    fn default() -> Self {
        GraphIdentity { status: 1, consumer: 0, graph_epoch: 0, pair: 0, frame_owner: 0, fingerprint: 0, context: 0, definition: 0, sync_handle: 0 }
    }
}
/// A producer slot's node in that graph: any publisher node (its work the context publisher), and the first node of
/// the producer -- its handle, view index (bytes +24), work RVA and definition name (+24).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphNode {
    pub publisher: bool,
    pub found: bool,
    pub handle: u64,
    pub view: u64,
    pub work: u64,
    pub name: u64,
}
/// A call's admission ticket for the .cpp's hook frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ticket {
    pub epoch: u64,
    pub row: u32,
    pub active: bool,
}

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    pub enable: *const AtomicU32,
    pub installed: *const AtomicU32,
    pub state: *const AtomicU32,
    pub failure: *const AtomicU32,
    pub seq: *const AtomicU32,
    pub boundary_failure: *const AtomicU32,
    pub stats: *mut [u64; 16],
    pub details: *mut [u64; 16],
    pub ranges: *mut u64,
    pub slots: *mut u64,
    pub submits: *mut u64,
    pub range_count: *mut u32,
    pub slot_count: *mut u32,
    pub submit_count: *mut u32,
    pub contract: *const Row,
    pub contract_count: usize,
    /// Graph(base, end, mode) into the identity (the graph kept on this thread for `graph_node`).
    pub graph: extern "C" fn(u64, u32, u32, *mut GraphIdentity),
    pub graph_node: extern "C" fn(u32, u64, *mut GraphNode),
    pub hook: *mut c_void,
    pub original: *mut *mut c_void,
}
impl Runtime {
    // SAFETY (all): the .cpp's static, its pointers to live exports, written under the owner's mutex.
    fn enable(&self) -> &AtomicU32 {
        unsafe { &*self.enable }
    }
    fn installed(&self) -> &AtomicU32 {
        unsafe { &*self.installed }
    }
    fn state(&self) -> &AtomicU32 {
        unsafe { &*self.state }
    }
    fn failure(&self) -> &AtomicU32 {
        unsafe { &*self.failure }
    }
    fn seq(&self) -> &AtomicU32 {
        unsafe { &*self.seq }
    }
    fn boundary_failure(&self) -> &AtomicU32 {
        unsafe { &*self.boundary_failure }
    }
    fn stats(&self) -> &mut [u64; 16] {
        unsafe { &mut *self.stats }
    }
    fn details(&self) -> &mut [u64; 16] {
        unsafe { &mut *self.details }
    }
    fn counts(&self) -> (u32, u32, u32) {
        unsafe { (*self.range_count, *self.slot_count, *self.submit_count) }
    }
    fn set_counts(&self, ranges: u32, slots: u32, submits: u32) {
        unsafe { (*self.range_count, *self.slot_count, *self.submit_count) = (ranges, slots, submits) };
    }
}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: an attached runtime (a test's), for the process.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    // The plugin's own: submission_frontier_host.rs.
    #[cfg(feature = "rust-build")]
    let attached = attached.or_else(|| Some(&crate::submission_frontier_host::RT.0));
    attached
}

/// A held lease: its token is a reference the owner releases.
struct Held(Lease);
impl Held {
    fn empty() -> Held {
        Held(Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        if !self.0.token.is_null() {
            // SAFETY: the reference the capture handed over.
            drop(unsafe { <windows::core::IUnknown as windows::core::Interface>::from_raw(self.0.token) });
            self.0.token = core::ptr::null_mut();
        }
    }
}
struct Latest {
    identity: BoundaryBatch,
    first: u32,
    entries: [u32; MAX_SUBMIT_LISTS as usize],
}
const NO_LATEST: Latest = Latest { identity: BoundaryBatch { probe_epoch: 0, batch: 0, thread: 0, queue: 0, lists: 0, count: 0 }, first: 0, entries: [0; MAX_SUBMIT_LISTS as usize] };
struct Store {
    admissions: AdmissionState,
    installed_base: u64,
    next_invocation: u64,
    next_batch: u64,
    hook_created: bool,
    ranges: Box<[[u64; 24]]>,
    slots: Box<[[u64; 20]]>,
    submits: Box<[[u64; 16]]>,
    leases: Vec<Held>,
    ledger: Box<Ledger>,
    latest: Latest,
}
// SAFETY: the leases' tokens are used only under the owner's mutex.
unsafe impl Send for Store {}
impl Store {
    fn new() -> Store {
        // SAFETY: a ledger is plain data, all zero when empty.
        let ledger = unsafe {
            let layout = std::alloc::Layout::new::<Ledger>();
            let raw = std::alloc::alloc_zeroed(layout).cast::<Ledger>();
            assert!(!raw.is_null());
            Box::from_raw(raw)
        };
        Store {
            admissions: AdmissionState::default(),
            installed_base: 0,
            next_invocation: 0,
            next_batch: 0,
            hook_created: false,
            ranges: vec![[0u64; 24]; MAX_RANGES as usize].into_boxed_slice(),
            slots: vec![[0u64; 20]; MAX_ROWS].into_boxed_slice(),
            submits: vec![[0u64; 16]; MAX_ROWS].into_boxed_slice(),
            leases: (0..MAX_ROWS).map(|_| Held::empty()).collect(),
            ledger,
            latest: NO_LATEST,
        }
    }
}
static STORE: Mutex<Option<Store>> = Mutex::new(None);
static INSTALL_LOCK: Mutex<()> = Mutex::new(());
static INSTALLED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static BOUNDARY_FAILURE: Cell<u32> = const { Cell::new(0) };
}
struct Guard(MutexGuard<'static, Option<Store>>);
impl core::ops::Deref for Guard {
    type Target = Store;
    fn deref(&self) -> &Store {
        self.0.as_ref().unwrap()
    }
}
impl core::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut Store {
        self.0.as_mut().unwrap()
    }
}
fn lock() -> Guard {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        *guard = Some(Store::new());
    }
    Guard(guard)
}
/// The exports' publication version around a write.
struct Write<'a>(&'a AtomicU32);
impl<'a> Write<'a> {
    fn new(seq: &'a AtomicU32) -> Self {
        seq.fetch_add(1, AcqRel);
        Write(seq)
    }
}
impl Drop for Write<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Release);
    }
}

fn base() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn thread() -> u64 {
    // SAFETY: a plain call.
    u64::from(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() })
}
fn memory() -> impl FnMut(u64, &mut [u8]) -> bool {
    |address, out| crate::geometry_packets::read(address, out)
}
fn publish_addresses(rt: &Runtime, store: &Store) {
    // SAFETY: the .cpp's exports.
    unsafe {
        *rt.ranges = store.ranges.as_ptr() as u64;
        *rt.slots = store.slots.as_ptr() as u64;
        *rt.submits = store.submits.as_ptr() as u64;
    }
}
fn reject(rt: &Runtime, reason: u32) {
    rt.failure().store(reason, SeqCst);
    if reason == CAPACITY {
        rt.state().store(4, SeqCst);
        rt.stats()[14] += 1;
    } else {
        rt.state().store(3, SeqCst);
    }
}
fn boundary_reject(rt: &Runtime, reason: u32) -> bool {
    BOUNDARY_FAILURE.with(|f| f.set(reason));
    rt.boundary_failure().store(reason, SeqCst);
    false
}
fn validate_submission(lease: &Lease, queue: *mut c_void) -> (bool, u32) {
    // SAFETY: a lease the owner holds; a live queue.
    let valid = unsafe { crate::native_recording::cvr_native_recording_validate_submission(lease, queue) };
    (valid, crate::native_recording::cvr_native_recording_last_failure())
}

/// Begin: the range consumer's call -- its slots captured, its graph identity read, each allowed publisher slot's token
/// taken and its context read again -- as one range row and its slot rows (the tokens kept in the ledger).
#[allow(clippy::too_many_arguments)]
fn begin(rt: &Runtime, header: u64, control: u64, end: u32, fences: u64, mode: u32, job: u64, caller: u64, epoch: u64) -> u32 {
    let invocation = {
        let mut store = lock();
        let _write = Write::new(rt.seq());
        if !store.admissions.matches(epoch) {
            reject(rt, failure::CHANGED);
            return u32::MAX;
        }
        publish_addresses(rt, &store);
        rt.stats()[0] += 1;
        if rt.counts().0 >= MAX_RANGES || store.next_invocation == u64::MAX {
            reject(rt, CAPACITY);
            return u32::MAX;
        }
        store.next_invocation += 1;
        store.next_invocation
    };
    let base = base();
    let installed_base = lock().installed_base;
    let mut read = memory();
    let captured = if INSTALLED.load(Acquire) && base == installed_base {
        capture_range(Some(&mut read as crate::submission_frontier::Read), base, caller, header, end, mode)
    } else {
        Err(CODE)
    };
    let failure = match &captured {
        Ok(_) => failure::NONE,
        Err(reason) if *reason == failure::NONE => CODE,
        Err(reason) => *reason,
    };
    let (range, slots) = captured.unwrap_or_default();
    let ok = failure == failure::NONE;
    let mut identity = GraphIdentity::default();
    if ok {
        (rt.graph)(base, end, mode, &mut identity);
    }
    struct Captured {
        slot: Slot,
        held: Held,
        failure: u32,
        node: GraphNode,
    }
    let mut values: Vec<Captured> = Vec::with_capacity(slots.len());
    if ok {
        for slot in &slots {
            let context = slot.context;
            let producer = producer_slot(slot.index);
            let mut node = GraphNode::default();
            (rt.graph_node)(producer, base, &mut node);
            let publisher = identity.status == 0 && node.publisher;
            let mut value = Captured { slot: *slot, held: Held::empty(), failure: 0, node };
            if token_capture_allowed(identity.status, publisher, slot) {
                // Capture can only return a genuine existing native reset token; the qualified synchronizer chain
                // protects the bank's unique context owner until the original transfers it.
                let recording = RecordingContext { context: context.address, state: context.state, list: context.list };
                // SAFETY: the context's list and a lease to fill.
                if !unsafe { crate::native_recording::cvr_native_recording_capture(context.list as *mut c_void, &recording, &mut value.held.0) } {
                    value.failure = crate::native_recording::cvr_native_recording_last_failure();
                }
                let mut again_read = memory();
                let mut reader = Some(&mut again_read as crate::submission_frontier::Read);
                if read_context(&mut reader, range.registry, slot.id) != Some(context) {
                    value.held = Held::empty();
                    value.failure = 100 + failure::CHANGED;
                }
            } else if context.address != 0 {
                value.failure = if identity.status != 0 { 200 + identity.status } else if !publisher { 209 } else { 100 + failure::CONTEXT };
            }
            values.push(value);
        }
    }
    let mut store = lock();
    let _write = Write::new(rt.seq());
    if !store.admissions.matches(epoch) {
        reject(rt, failure::CHANGED);
        return u32::MAX;
    }
    let (range_count, slot_count, submit_count) = rt.counts();
    if range_count >= MAX_RANGES || values.len() > MAX_ROWS - slot_count as usize {
        reject(rt, CAPACITY);
        return u32::MAX;
    }
    let index = range_count;
    rt.set_counts(range_count + 1, slot_count, submit_count);
    let thread = thread();
    store.ranges[index as usize] = [
        epoch,
        invocation,
        thread,
        caller,
        header,
        range.table,
        u64::from(range.begin),
        u64::from(end),
        u64::from(mode),
        control,
        fences,
        job,
        u64::from(slot_count),
        values.len() as u64,
        0,
        0,
        u64::from(failure),
        u64::from(identity.status),
        identity.consumer,
        identity.graph_epoch,
        identity.pair,
        identity.frame_owner,
        if identity.status != 0 { u64::MAX } else { identity.sync_handle },
        identity.fingerprint,
    ];
    *rt.details() = [base, 0, 0, 0, caller, header, range.table, u64::from(range.begin), u64::from(end), u64::from(mode), u64::from(identity.status), identity.context, identity.definition, thread, invocation, u64::from(failure)];
    if !ok {
        rt.stats()[2] += 1;
        reject(rt, failure);
        return index;
    }
    rt.stats()[1] += 1;
    rt.stats()[if identity.status != 0 { 8 } else { 7 }] += 1;
    for value in values {
        let (_, slots_now, submits_now) = rt.counts();
        let slot_index = slots_now;
        rt.set_counts(index + 1, slots_now + 1, submits_now);
        let slot = value.slot;
        let context = slot.context;
        let producer = producer_slot(slot.index);
        let (mut node, mut view, mut work, mut name) = (u64::MAX, u64::MAX, 0u64, 0u64);
        if identity.status == 0 && value.node.found {
            (node, view, work, name) = (value.node.handle, value.node.view, value.node.work, value.node.name);
        }
        let token = value.held.0.token as u64;
        store.slots[slot_index as usize] = [
            epoch,
            invocation,
            u64::from(index),
            u64::from(slot.index),
            u64::from(producer),
            u64::from(slot.id),
            context.address,
            context.list,
            context.state,
            u64::from(context.kind),
            context.submitted,
            value.held.0.generation,
            token,
            (token != 0) as u64,
            if slot.failure != failure::NONE { 100 + u64::from(slot.failure) } else { u64::from(value.failure) },
            node,
            view,
            work,
            name,
            context.residency,
        ];
        let stats = rt.stats();
        stats[3] += 1;
        if context.address != 0 {
            stats[4] += 1;
        }
        if token != 0 {
            stats[5] += 1;
        } else if context.address != 0 {
            stats[6] += 1;
        }
        if context.address != 0 && context.list != 0 {
            let ledger_index = store.ledger.count;
            let key = Key { list: context.list, context: context.address, generation: value.held.0.generation, token, invocation, row: slot_index, graph_known: identity.status == 0 && node != u64::MAX && work == PUBLISHER_WORK };
            if !store.ledger.add(&key) {
                reject(rt, CAPACITY);
                return index;
            }
            store.leases[ledger_index] = value.held;
        }
    }
    if rt.state().load(SeqCst) < 3 {
        rt.state().store(1, SeqCst);
    }
    index
}

/// Enter: a call admitted while the probe is enabled, and its range begun.
#[allow(clippy::too_many_arguments)]
pub fn enter(rt: &Runtime, header: u64, control: u64, end: u32, fences: u64, mode: u32, job: u64, caller: u64) -> Ticket {
    let mut ticket = Admission::default();
    let admitted = {
        let mut store = lock();
        let enabled = rt.enable().load(Acquire) == 1;
        store.admissions.admit(enabled, &mut ticket)
    };
    if !admitted {
        return Ticket::default();
    }
    let row = begin(rt, header, control, end, fences, mode, job, caller, ticket.epoch);
    Ticket { epoch: ticket.epoch, row, active: true }
}
/// Leave: the call's return (or not) on its range row, the admission released.
pub fn leave(rt: &Runtime, ticket: Ticket, result: u64, returned: bool) {
    if !ticket.active {
        return;
    }
    let mut store = lock();
    let _write = Write::new(rt.seq());
    let (ranges, _, _) = rt.counts();
    if store.admissions.matches(ticket.epoch) && ticket.row < ranges && store.ranges[ticket.row as usize][0] == ticket.epoch {
        let row = &mut store.ranges[ticket.row as usize];
        row[14] = result;
        row[15] = returned as u64;
        if !returned {
            row[16] = u64::from(EXCEPTION);
            reject(rt, EXCEPTION);
        }
    }
    let mut admission = Admission { epoch: ticket.epoch, active: true };
    store.admissions.release(&mut admission);
}
/// Install: the consumer's detour, once the code contract holds; the trampoline kept even after an ambiguous enable
/// failure (an original call must stay forwardable until MinHook's teardown).
pub fn install(rt: &Runtime) -> bool {
    let _install = INSTALL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let base = base();
    if INSTALLED.load(Acquire) {
        return lock().installed_base == base;
    }
    if lock().hook_created {
        return false;
    }
    let mut diagnostic = [0u64; 16];
    diagnostic[0] = base;
    let rows = if rt.contract.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(rt.contract, rt.contract_count) } };
    let mut code = [0u64; 3];
    // SAFETY: the contract's rows and three outputs.
    let valid = base != 0 && crate::native_recording::cvr_native_recording_compatible() && unsafe { crate::frontier_reads::cvr_nsfr_code(base, rows.as_ptr(), rows.len(), &mut code) };
    diagnostic[1..4].copy_from_slice(&code);
    if !valid {
        let _store = lock();
        let _write = Write::new(rt.seq());
        *rt.details() = diagnostic;
        rt.installed().store(2, SeqCst);
        reject(rt, CODE);
        return false;
    }
    extern "system" {
        fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
        fn MH_EnableHook(target: *mut c_void) -> i32;
    }
    let target = (base + HOOK_RVA) as *mut c_void;
    // SAFETY: the consumer's address as the contract proved it; the .cpp's detour and trampoline slot.
    if unsafe { MH_CreateHook(target, rt.hook, rt.original) } != 0 {
        rt.installed().store(3, SeqCst);
        rt.failure().store(INSTALL, SeqCst);
        return false;
    }
    {
        let mut store = lock();
        store.hook_created = true;
        store.installed_base = base;
    }
    // SAFETY: the hook just created.
    if unsafe { MH_EnableHook(target) } != 0 {
        rt.installed().store(3, SeqCst);
        rt.failure().store(INSTALL, SeqCst);
        return false;
    }
    {
        let store = lock();
        let _write = Write::new(rt.seq());
        publish_addresses(rt, &store);
    }
    INSTALLED.store(true, Release);
    rt.installed().store(1, SeqCst);
    true
}
/// Reset: everything cleared while the probe is off and no call is in flight (a new epoch).
pub fn reset(rt: &Runtime) -> bool {
    let mut store = lock();
    let enabled = rt.enable().load(Acquire) != 0;
    if !store.admissions.reset(enabled) {
        return false;
    }
    let _write = Write::new(rt.seq());
    let used = store.ledger.count.min(MAX_ROWS);
    for held in &mut store.leases[..used] {
        *held = Held::empty();
    }
    store.ledger.clear();
    store.next_invocation = 0;
    store.next_batch = 0;
    store.latest = NO_LATEST;
    rt.set_counts(0, 0, 0);
    *rt.stats() = [0; 16];
    *rt.details() = [0; 16];
    rt.state().store(0, SeqCst);
    rt.failure().store(0, SeqCst);
    rt.boundary_failure().store(0, SeqCst);
    publish_addresses(rt, &store);
    true
}
/// ObserveSubmit: a batch naming a held list -- each list matched against the ledger (its token validated at the
/// queue), one submit row each; the batch kept as the latest for FindBoundary.
pub fn observe_submit(rt: &Runtime, queue: u64, count: u32, lists: u64, internal: bool) {
    if rt.enable().load(Acquire) != 1 || internal || queue == 0 || lists == 0 || count == 0 {
        return;
    }
    let mut guard = lock();
    if rt.enable().load(Acquire) != 1 {
        return;
    }
    let store: &mut Store = &mut guard;
    store.latest = NO_LATEST;
    let _write = Write::new(rt.seq());
    if count > MAX_SUBMIT_LISTS {
        reject(rt, failure::BOUNDS);
        return;
    }
    let mut bytes = vec![0u8; count as usize * 8];
    let mut read = memory();
    if !frontier_read(&mut Some(&mut read as crate::submission_frontier::Read), lists, &mut bytes) {
        reject(rt, failure::READ);
        return;
    }
    let addresses: Vec<u64> = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    if !addresses.iter().any(|&a| store.ledger.mentions(a)) {
        return;
    }
    let (ranges, slots, submits) = rt.counts();
    if count as usize > MAX_ROWS - submits as usize || store.next_batch == u64::MAX {
        reject(rt, CAPACITY);
        return;
    }
    store.next_batch += 1;
    let batch = store.next_batch;
    rt.stats()[9] += 1;
    let first = submits;
    let thread = thread();
    let epoch = store.admissions.epoch;
    for (i, &address) in addresses.iter().enumerate() {
        let Store { ledger, leases, slots: slot_rows, submits: submit_rows, latest, .. } = &mut *store;
        let mut validate = |index: u32, failure: &mut u32| {
            let (valid, reason) = validate_submission(&leases[index as usize].0, queue as *mut c_void);
            *failure = reason;
            valid
        };
        let found = ledger.observe(address, &mut validate);
        latest.entries[i] = found.entry;
        let (_, _, at) = rt.counts();
        rt.set_counts(ranges, slots, at + 1);
        let row = &mut submit_rows[at as usize];
        *row = [epoch, batch, thread, queue, u64::from(count), i as u64, address, u64::from(found.kind), 0, u64::MAX, 0, u64::from(found.failure), 0, u64::MAX, u64::MAX, u64::MAX];
        if found.entry != u32::MAX {
            let key = ledger.entries[found.entry as usize].key;
            let source = &slot_rows[key.row as usize];
            row[8] = key.invocation;
            row[9] = u64::from(key.row);
            row[10] = key.generation;
            row[12] = key.context;
            row[13] = source[4];
            row[14] = source[15];
            row[15] = source[16];
        }
        let stats = rt.stats();
        match found.kind {
            matches::MATCHED | matches::MATCHED_UNKNOWN_GRAPH => stats[10] += 1,
            matches::UNKNOWN_LIST => stats[11] += 1,
            matches::REPLAY => stats[13] += 1,
            _ => stats[12] += 1,
        }
    }
    store.latest.first = first;
    store.latest.identity = BoundaryBatch { probe_epoch: epoch, batch, thread, queue, lists, count };
    if rt.state().load(SeqCst) < 3 {
        rt.state().store(2, SeqCst);
    }
}
/// FindBoundary: the latest observed batch's boundary -- every matched row's ledger key, lease, slot and range rows
/// consistent, both sources the context publisher's, the suffix selected (submission_frontier.rs), every token still
/// valid at the queue and the list array unchanged.
pub fn find_boundary(rt: &Runtime, queue: u64, count: u32, lists: u64, sources: &[BoundarySource; 2], leases: [&Lease; 2], internal: bool, output: &mut Boundary) -> bool {
    *output = Boundary::default();
    if queue == 0 || lists == 0 || count < 3 || count > MAX_SUBMIT_LISTS {
        return boundary_reject(rt, boundary_failure::ARGUMENTS);
    }
    if internal || rt.enable().load(Acquire) != 1 {
        return boundary_reject(rt, boundary_failure::DISABLED);
    }
    let guard = lock();
    if rt.enable().load(Acquire) != 1 || !INSTALLED.load(Acquire) {
        return boundary_reject(rt, boundary_failure::DISABLED);
    }
    let store: &Store = &guard;
    let (ranges, slots, submits) = rt.counts();
    let current = BoundaryBatch { probe_epoch: store.admissions.epoch, batch: store.next_batch, thread: thread(), queue, lists, count };
    if store.latest.identity != current || store.latest.first > submits || count != submits - store.latest.first {
        return boundary_reject(rt, boundary_failure::BATCH);
    }
    let base = base();
    if base == 0 || base != store.installed_base || base > u64::MAX - PUBLISHER_WORK {
        return boundary_reject(rt, boundary_failure::METADATA);
    }
    let read_lists = || {
        let mut bytes = vec![0u8; count as usize * 8];
        let mut read = memory();
        frontier_read(&mut Some(&mut read as crate::submission_frontier::Read), lists, &mut bytes).then(|| bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect::<Vec<u64>>())
    };
    let Some(addresses) = read_lists() else { return boundary_reject(rt, boundary_failure::READ) };
    let mut rows = vec![BoundaryList::default(); count as usize];
    for i in 0..count as usize {
        let submitted = &store.submits[store.latest.first as usize + i];
        let row = &mut rows[i];
        if submitted[0] != current.probe_epoch || submitted[1] != current.batch || submitted[2] != current.thread || submitted[3] != current.queue || submitted[4] != u64::from(count) || submitted[5] != i as u64 {
            return boundary_reject(rt, boundary_failure::BATCH);
        }
        if submitted[6] != addresses[i] {
            return boundary_reject(rt, boundary_failure::CHANGED);
        }
        row.list = submitted[6];
        row.kind = submitted[7] as u32;
        if row.kind != matches::MATCHED {
            continue;
        }
        let entry = store.latest.entries[i];
        if entry as usize >= store.ledger.count {
            return boundary_reject(rt, boundary_failure::METADATA);
        }
        let key = store.ledger.entries[entry as usize].key;
        let lease = &store.leases[entry as usize].0;
        if !key.graph_known
            || key.row >= slots
            || submitted[9] != u64::from(key.row)
            || submitted[8] != key.invocation
            || submitted[10] != key.generation
            || submitted[11] != 0
            || submitted[12] != key.context
            || key.list != row.list
            || lease.list != key.list
            || lease.context != key.context
            || lease.generation != key.generation
            || lease.token as u64 != key.token
            || key.token == 0
            || lease.registry == 0
        {
            return boundary_reject(rt, boundary_failure::METADATA);
        }
        let slot = &store.slots[key.row as usize];
        if slot[0] != current.probe_epoch
            || slot[1] != key.invocation
            || slot[2] >= u64::from(ranges)
            || slot[3] != slot[4] + 1
            || slot[4] >= u64::from(MAX_SLOTS)
            || slot[5] == 0
            || slot[6] != key.context
            || slot[7] != key.list
            || slot[8] == 0
            || (slot[9] != 1 && slot[9] != 2)
            || slot[10] != 0
            || slot[11] != key.generation
            || slot[12] != key.token
            || slot[13] != 1
            || slot[14] != 0
            || slot[15] > u64::from(u32::MAX)
            || slot[16] > 1
            || slot[17] != PUBLISHER_WORK
            || submitted[13] != slot[4]
            || submitted[14] != slot[15]
            || submitted[15] != slot[16]
        {
            return boundary_reject(rt, boundary_failure::METADATA);
        }
        let range = &store.ranges[slot[2] as usize];
        if range[0] != current.probe_epoch || range[1] != key.invocation || range[16] != 0 || range[17] != 0 || slot[3] < range[6] || slot[3] > range[7] {
            return boundary_reject(rt, boundary_failure::METADATA);
        }
        row.context = key.context;
        row.generation = key.generation;
        row.token = key.token;
        row.registry = lease.registry;
        row.work = base + slot[17];
        row.name = slot[18];
        row.slot = slot[4] as u32;
        row.node = slot[15] as u32;
        row.view = slot[16] as u32;
        row.graph = BoundaryGraph { invocation: range[1], consumer: range[18], epoch: range[19], pair_generation: range[20], frame_owner: range[21], fingerprint: range[23], range: slot[2] as u32 };
        row.qualified = true;
    }
    if sources.iter().any(|s| s.work != base + PUBLISHER_WORK) {
        return boundary_reject(rt, boundary_failure::SOURCE);
    }
    let selected = match crate::submission_frontier::select_boundary(&store.latest.identity, &current, &rows, &addresses, sources) {
        Ok(selected) => selected,
        Err(reason) => return boundary_reject(rt, reason),
    };
    if leases.iter().any(|lease| !validate_submission(lease, queue as *mut c_void).0) {
        return boundary_reject(rt, boundary_failure::NATIVE_TOKEN);
    }
    for i in 0..count {
        if i >= selected.after_index || i == selected.source_indices[0] || i == selected.source_indices[1] {
            let entry = store.latest.entries[i as usize] as usize;
            if entry >= store.ledger.count || !validate_submission(&store.leases[entry].0, queue as *mut c_void).0 {
                return boundary_reject(rt, boundary_failure::NATIVE_TOKEN);
            }
        }
    }
    if read_lists().as_deref() != Some(&addresses[..]) {
        return boundary_reject(rt, boundary_failure::CHANGED);
    }
    *output = selected;
    BOUNDARY_FAILURE.with(|f| f.set(boundary_failure::NONE));
    rt.boundary_failure().store(0, SeqCst);
    true
}

// ---- the C ABI

/// # Safety
/// `runtime` is the .cpp's static.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sfo_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C-unwind" fn cvr_sfo_enter(header: u64, control: u64, end: u32, fences: u64, mode: u32, job: u64, caller: u64, ticket: *mut Ticket) {
    let t = runtime().map_or(Ticket::default(), |rt| enter(rt, header, control, end, fences, mode, job, caller));
    // SAFETY: the hook frame's ticket.
    unsafe { *ticket = t };
}
/// # Safety
/// `ticket` is the hook frame's.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sfo_leave(ticket: *const Ticket, result: u64, returned: bool) {
    if let (Some(rt), Some(t)) = (runtime(), unsafe { ticket.as_ref() }) {
        leave(rt, *t, result, returned);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sfo_install() -> bool {
    runtime().is_some_and(install)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sfo_reset() -> bool {
    runtime().is_some_and(reset)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sfo_observe_submit(queue: u64, count: u32, lists: u64, internal: bool) {
    if let Some(rt) = runtime() {
        observe_submit(rt, queue, count, lists, internal);
    }
}
/// # Safety
/// `sources` holds two numeric sources, `leases` their two recordings; `output` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sfo_find_boundary(queue: u64, count: u32, lists: u64, sources: *const [BoundarySource; 2], leases: *const [*const Lease; 2], internal: bool, output: *mut Boundary) -> bool {
    let Some(rt) = runtime() else { return false };
    let (sources, leases, output) = unsafe { (&*sources, &*leases, &mut *output) };
    let leases = unsafe { [&*leases[0], &*leases[1]] };
    find_boundary(rt, queue, count, lists, sources, leases, internal, output)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sfo_last_boundary_failure() -> u32 {
    BOUNDARY_FAILURE.with(|f| f.get())
}

#[cfg(test)]
mod tests {
    //! The owner against a runtime of its own: a disabled probe, the admission's epoch, a range refused by its caller,
    //! a submission naming no held list, the boundary's argument and batch refusals, the reset.
    use super::*;

    extern "C" fn no_graph(_: u64, _: u32, _: u32, out: *mut GraphIdentity) {
        unsafe { *out = GraphIdentity::default() };
    }
    extern "C" fn no_node(_: u32, _: u64, out: *mut GraphNode) {
        unsafe { *out = GraphNode::default() };
    }

    #[test]
    fn admissions_ranges_submissions_and_boundaries() {
        let atom = || Box::leak(Box::new(AtomicU32::new(0))) as &'static AtomicU32;
        let rt: &'static Runtime = Box::leak(Box::new(Runtime {
            enable: atom(),
            installed: atom(),
            state: atom(),
            failure: atom(),
            seq: atom(),
            boundary_failure: atom(),
            stats: Box::leak(Box::new([0u64; 16])),
            details: Box::leak(Box::new([0u64; 16])),
            ranges: Box::leak(Box::new(0u64)),
            slots: Box::leak(Box::new(0u64)),
            submits: Box::leak(Box::new(0u64)),
            range_count: Box::leak(Box::new(0u32)),
            slot_count: Box::leak(Box::new(0u32)),
            submit_count: Box::leak(Box::new(0u32)),
            contract: core::ptr::null(),
            contract_count: 0,
            graph: no_graph,
            graph_node: no_node,
            hook: core::ptr::null_mut(),
            original: core::ptr::null_mut(),
        }));
        assert!(!enter(rt, 0x1000, 0, 4, 0, 0, 0, 0x55).active, "disabled");
        rt.enable().store(1, SeqCst);
        let ticket = enter(rt, 0x1000, 0, 4, 0, 0, 0, 0x55);
        assert!(ticket.active && ticket.row == 0, "an admitted call's range row");
        assert_eq!(rt.counts().0, 1);
        assert_eq!(unsafe { *rt.ranges } != 0, true, "the rows published");
        let store_row = lock().ranges[0];
        assert_eq!((store_row[0], store_row[1], store_row[16]), (ticket.epoch, 1, u64::from(CODE)), "not installed: refused by its code");
        assert_eq!((rt.stats()[0], rt.stats()[2], rt.state().load(SeqCst)), (1, 1, 3));
        leave(rt, ticket, 77, true);
        let returned = lock().ranges[0];
        assert_eq!((returned[14], returned[15]), (77, 1), "its return");
        assert!(!reset(rt), "no reset while enabled");
        let lists = [0x1234u64];
        observe_submit(rt, 0x99, 1, lists.as_ptr() as u64, false);
        assert_eq!(rt.counts().2, 0, "a batch naming no held list");
        let mut out = Boundary::default();
        let source = BoundarySource::default();
        let lease = Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
        assert!(!find_boundary(rt, 0x99, 2, lists.as_ptr() as u64, &[source, source], [&lease, &lease], false, &mut out) && cvr_sfo_last_boundary_failure() == boundary_failure::ARGUMENTS);
        let three = [1u64, 2, 3];
        assert!(!find_boundary(rt, 0x99, 3, three.as_ptr() as u64, &[source, source], [&lease, &lease], false, &mut out) && cvr_sfo_last_boundary_failure() == boundary_failure::DISABLED, "not installed");
        rt.enable().store(0, SeqCst);
        assert!(reset(rt) && rt.counts() == (0, 0, 0) && rt.state().load(SeqCst) == 0 && rt.stats()[0] == 0, "a new epoch, cleared");
        rt.enable().store(1, SeqCst);
        let second = enter(rt, 0x1000, 0, 4, 0, 0, 0, 0x55);
        assert!(second.active && second.epoch == ticket.epoch + 1 && second.row == 0);
        leave(rt, second, 0, false);
        assert_eq!((lock().ranges[0][16], rt.failure().load(SeqCst)), (u64::from(EXCEPTION), EXCEPTION), "a call that did not return");
    }
}
