//! The paired frame's engine hooks (formerly src/Stereo/NativePairedFrame.cpp), behind CVR_STEREO_PAIRED_ROOT:
//!
//! - the bounded paired-frame root (0x409DD4): the eye's render-to-texture root suppressed while preparation pairs,
//!   its frame data observed (request 3), every root bracketed for definition tracking;
//! - the owned paired graph snapshot (0x547CF4): on request, the declared graph copied (from definition tracking's
//!   owned declaration or read from the engine), its paired pass frontier planned and published, the owned graph
//!   compared (CompareOwnedGraph: the reference order, the CPU frontier's candidate, the whole-pass candidate with its
//!   shared owner orders and context order proofs), definition tracking told of the match, the graph jobs selected;
//! - the retained complete native graph jobs (0x32D60C): the queued graph's jobs captured as deferred receipts while
//!   the queue still owns them (at most four pending), selected against the declared graph;
//! - the exact pre-compiler resource uses (0x3E0330): the pool's resource uses captured for the declared graph before
//!   the sorting modes compile it, checked compiled after;
//! - the native proxy preparation inputs (0x33020C): the gather bracketed for preparation with its eye;
//! - the bounded CPU preparation schedule (0x8EA1C8): the graph's preparation schedule planned (cached by source) and,
//!   in mode 2, the native dependency creation run over its job graph shadow.
//!
//! Each detour is registered here with its code contract (paired_frame_code.rs); the return addresses the C++ took
//! with `_ReturnAddress()` come from a one-frame back trace. The .cpp keeps the exports' C++ definitions for its own
//! build and lends definition tracking and preparation (both still C++) through the runtime below.

use crate::native::graph_jobs::{CapturePlan, QUEUE_RETURN};
use crate::native::graph_snapshot::{field_u32, field_u64, Snapshot};
use crate::native::job_lease::JobLeaseApi;
use crate::native::job_receipt::{DeferredReceipt, FrameOwnerApi, MAX_PENDING_RECEIPTS};
use crate::native::resource_uses;
use crate::native_state_exports as pair;
use crate::owned_native_graph::OwnedGraph;
use crate::definition_wrappers::{CpuFrontierPlan, FrontierPlan as Frontier};
use crate::paired_cpu_frontier as pcf;
use crate::paired_frontier::{self as pf, Edges};
use crate::preparation_schedule as ps;
use crate::preparation_trace as prep;
use crate::definition_tracking as tracking;
use crate::used_moved_exports as x;
use core::ffi::c_void;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering::*};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};



extern "C" {
    static g_vrcam_ctx_key: AtomicU64;
    static g_menuModeValue: i32;
}

// ---- the engine
fn base() -> u64 {
    cvr_engine::code::image_base() as u64
}
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is the hook, frame 1 its caller.
#[inline(always)]
fn return_address() -> u64 {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 {
        0
    } else {
        frames[0] as u64
    }
}
/// The .cpp's Read: ReadProcessMemory of the whole range (the buffer keeps what it got when it fails, as there).
fn read(address: u64, out: &mut [u8]) -> bool {
    cvr_engine::stack::read(address, out)
}
fn read_u64(address: u64) -> (bool, u64) {
    let mut bytes = [0u8; 8];
    let ok = read(address, &mut bytes);
    (ok, u64::from_le_bytes(bytes))
}
fn read_u32(address: u64) -> (bool, u32) {
    let mut bytes = [0u8; 4];
    let ok = read(address, &mut bytes);
    (ok, u32::from_le_bytes(bytes))
}
fn env_paired_root() -> bool {
    std::env::var_os("CVR_STEREO_PAIRED_ROOT").is_some()
}
fn now() -> u64 {
    let mut t = 0i64;
    // SAFETY: a plain counter read.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t as u64
}
/// A vector's data() as the C++ exported it: null until it first had storage.
fn data<T>(v: &Vec<T>) -> u64 {
    if v.capacity() == 0 {
        0
    } else {
        v.as_ptr() as u64
    }
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
/// An exported array, written in place (the exports are plain C++-visible statics).
///
/// # Safety
/// `p` is one of the port's export statics.
unsafe fn out<T>(p: *mut T) -> &'static mut T {
    unsafe { &mut *p }
}
fn atomic_u64(p: *mut u64) -> &'static AtomicU64 {
    // SAFETY: an aligned export word, accessed atomically as the C++ Interlocked calls did.
    unsafe { AtomicU64::from_ptr(p) }
}

// ---- the engine's reference calls
unsafe extern "C" fn retain_graph_job(group: *mut c_void) {
    // SAFETY: a live engine job group; +0x1C is its reference count.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x1C).cast()) }.fetch_add(1, SeqCst);
}
unsafe extern "C" fn release_graph_job(group: *mut *mut c_void) {
    // SAFETY: the engine's release (it nulls the pointer).
    let release: unsafe extern "C" fn(*mut *mut c_void) = unsafe { core::mem::transmute((base() + 0x2744CC) as usize) };
    unsafe { release(group) };
}
unsafe extern "C" fn complete_graph_job(group: *mut c_void) -> bool {
    // SAFETY: a live engine job group; +0x18 is its pending count (read with a full barrier, as the CAS did).
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x18).cast()) }.compare_exchange(0, 0, SeqCst, SeqCst).is_ok()
}
unsafe extern "C" fn retain_graph_owner(owner: *mut c_void) {
    // SAFETY: a live frame owner; +8 is its reference count.
    unsafe { AtomicI32::from_ptr(owner.cast::<u8>().add(8).cast()) }.fetch_add(1, SeqCst);
}
unsafe extern "C" fn release_graph_owner(owner: *mut c_void) {
    // SAFETY: the engine's release.
    let release: unsafe extern "C" fn(*mut c_void) = unsafe { core::mem::transmute((base() + 0x32DE14) as usize) };
    unsafe { release(owner) };
}
/// The declaration's frame owner, held while its graph jobs may be selected.
struct DeclarationOwner(u64);
impl DeclarationOwner {
    fn new(address: u64) -> Self {
        // SAFETY: a frame owner with a positive count, read just before.
        unsafe { retain_graph_owner(address as *mut c_void) };
        DeclarationOwner(address)
    }
}
impl Drop for DeclarationOwner {
    fn drop(&mut self) {
        // SAFETY: the reference taken in new.
        unsafe { release_graph_owner(self.0 as *mut c_void) };
    }
}
/// A deferred receipt in the pending list (its leases hold engine pointers; the list is behind a mutex).
struct Receipt(Box<DeferredReceipt>);
// SAFETY: the receipt's engine pointers are only used under the jobs mutex, as the C++ did.
unsafe impl Send for Receipt {}

/// An owned native graph's compiled snapshot (failure 1 when it has none, as the C++ Snapshot()).
fn owned_snapshot(graph: &OwnedGraph) -> Snapshot {
    match graph.snapshot() {
        Some((nodes, edges)) => Snapshot { nodes, edges, failure: 0 },
        None => Snapshot { failure: 1, ..Default::default() },
    }
}
/// The engine's reference sort over an owned graph (under SEH).
fn native_sort(graph: *mut u8) -> bool {
    // SAFETY: the engine's sort over our constructed graph header; a fault is caught.
    unsafe { cvr_engine::code::guarded_call4((base() + 0x8E9B28) as usize, graph.cast(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) }.is_some()
}
fn owned(graph: &mut OwnedGraph, source: &Snapshot, edges: [Edges; 2]) -> bool {
    graph.create(&source.nodes, &source.edges, source.failure, edges) && native_sort(graph.sortable_data()) && graph.validate_compiled()
}

// ---- the state (one mutex per C++ mutex; the capture's is always taken before the others)
#[derive(Default)]
struct Capture {
    graph: Snapshot,
    frontier: Frontier,
    owned: Snapshot,
    witnesses: Vec<[u32; 6]>,
    shared_owner_rows: Vec<[u64; 4]>,
}
#[derive(Default)]
struct Jobs {
    declaration_owner: Option<DeclarationOwner>,
    receipts: Vec<Receipt>,
    epoch_seen: u64,
    capture_epoch: u64,
    capture_generation: u64,
    capture_owner: u64,
}
#[derive(Default)]
struct Resource {
    uses: resource_uses::Snapshot,
    pool: u64,
    wrapper: u64,
}
static CAPTURE: LazyLock<Mutex<Capture>> = LazyLock::new(Default::default);
static JOBS: LazyLock<Mutex<Jobs>> = LazyLock::new(Default::default);
static RESOURCE: LazyLock<Mutex<Resource>> = LazyLock::new(Default::default);
static SCHEDULE: LazyLock<Mutex<Schedule>> = LazyLock::new(Default::default);
static OBSERVATION: Mutex<()> = Mutex::new(());

/// ArmGraphJobsLocked.
fn arm_graph_jobs(j: &mut Jobs) {
    let epoch = u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst));
    if epoch != j.epoch_seen {
        j.epoch_seen = epoch;
        j.capture_epoch = 0;
        j.capture_generation = 0;
        j.capture_owner = 0;
        j.declaration_owner = None;
        // SAFETY: the exports.
        unsafe {
            out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsSourceReject)).fill(0);
            out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsStats)).fill(0);
        }
    }
    j.receipts.retain(|r| !(r.0.epoch() != epoch && r.0.complete()));
}
/// SelectGraphJobsLocked.
fn select_graph_jobs(j: &mut Jobs, c: &Capture) {
    let base = base();
    // SAFETY: the exports.
    let (selection, pending, meta) = unsafe {
        (
            out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsSelection)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsPending)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationGraphMeta)),
        )
    };
    let graph_state = || x::CyberpunkVR_PreparationGraphState.load(SeqCst);
    let generation = || pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst);
    let jobs_epoch = || u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst));
    *selection = [
        j.capture_epoch,
        j.capture_generation,
        j.capture_owner,
        jobs_epoch(),
        generation(),
        meta[0],
        u64::from(graph_state()),
        j.declaration_owner.is_some() as u64,
    ];
    x::CyberpunkVR_PreparationGraphJobsPendingCount.store(j.receipts.len() as u32, SeqCst);
    for (i, receipt) in j.receipts.iter().enumerate().take(pending.len()) {
        let r = &receipt.0;
        let copy = r.source().unwrap_or_default();
        let mut mask = 0u64;
        mask |= (r.epoch() == j.capture_epoch) as u64;
        mask |= ((r.generation() == j.capture_generation) as u64) << 1;
        mask |= ((r.graph_address() == meta[0]) as u64) << 2;
        mask |= ((r.frame_owner() == j.capture_owner) as u64) << 3;
        if graph_state() == 2 {
            mask |= ((copy.nodes == c.graph.nodes) as u64) << 4;
            mask |= ((copy.edges == c.graph.edges) as u64) << 5;
        }
        pending[i] = [
            r.epoch(),
            r.generation(),
            r.frame_owner(),
            r.graph_address(),
            pf::fingerprint(&copy.nodes, &copy.edges, copy.failure, base),
            mask,
            r.complete() as u64,
            r.selected() as u64,
        ];
    }
    if j.declaration_owner.is_none()
        || x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) != 1
        || graph_state() != 2
        || j.capture_epoch == 0
        || j.capture_epoch != j.epoch_seen
        || j.capture_epoch != jobs_epoch()
        || j.capture_generation != generation()
    {
        return;
    }
    let (epoch, capture_generation, owner, graph) = (j.capture_epoch, j.capture_generation, j.capture_owner, meta[0]);
    for receipt in &mut j.receipts {
        if receipt.0.select(epoch, capture_generation, base, graph, owner, &c.graph) {
            // SAFETY: the export.
            let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsStats)) };
            stats[1] = receipt.0.node_count() as u64;
            stats[2] = receipt.0.frame_owner();
            stats[3] = receipt.0.graph_address();
            stats[4] = base + QUEUE_RETURN;
            stats[7] = c.frontier.plan.fingerprint;
            stats[6] = 0;
            stats[5] = receipt.0.complete() as u64;
            stats[0] = if stats[5] != 0 { 3 } else { 2 };
            return;
        }
    }
}
/// PollOwnedGraphJobs (preparation's graph job poll).
extern "C" fn poll_owned_graph_jobs() {
    tracking::poll();
    let base = base();
    let c = lock(&CAPTURE);
    let mut j = lock(&JOBS);
    arm_graph_jobs(&mut j);
    // SAFETY: the export.
    let meta0 = unsafe { x::CyberpunkVR_PreparationGraphMeta[0] };
    for receipt in &j.receipts {
        let r = &receipt.0;
        if x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 1
            && x::CyberpunkVR_PreparationGraphState.load(SeqCst) == 2
            && r.selected()
            && r.epoch() == j.epoch_seen
            && j.capture_epoch == j.epoch_seen
            && j.capture_generation == pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst)
            && r.matches(j.capture_epoch, j.capture_generation, base, meta0, j.capture_owner, &c.graph)
            && r.complete()
        {
            // SAFETY: the export.
            let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsStats)) };
            stats[5] = 1;
            stats[0] = 3;
        }
    }
    j.receipts.retain(|r| {
        !(r.0.complete()
            && (x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 0
                || r.0.epoch() != u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst))))
    });
    if x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 0 {
        j.declaration_owner = None;
    }
}

// ---- the paired-frame root (0x409DD4)
static mut ROOT: usize = 0;
/// Observe: the root's frame data, once per row (16).
fn observe(data: u64, caller: u64) {
    let _guard = lock(&OBSERVATION);
    let index = x::CyberpunkVR_PreparationRootObservedCount.load(SeqCst);
    if index >= 16 {
        return;
    }
    let mut row = [0u64; 8];
    row[0] = caller.wrapping_sub(base());
    row[1] = data;
    row[5] = prep::active() as u64;
    row[6] = u64::from(pair::CyberpunkVR_PreparationPairVerified.load(SeqCst));
    row[7] = u64::from(pair::CyberpunkVR_PreparationRootSkipped.load(SeqCst));
    if data != 0 {
        let (_, mode) = read_u32(data + 0xFA0);
        let (_, count) = read_u32(data + 0x14C);
        row[2] = u64::from(mode);
        row[3] = u64::from(count);
        if count != 0 {
            let (ok, names) = read_u64(data + 0x140);
            if ok {
                let mut first = [0u8; 8];
                read(names, &mut first);
                row[4] = u64::from_le_bytes(first);
            }
        }
    }
    // SAFETY: the export; the count published after the row.
    unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationRootObserved))[index as usize] = row };
    x::CyberpunkVR_PreparationRootObservedCount.store(index + 1, Release);
}
extern "C" fn root_compatible() -> bool {
    if !env_paired_root() {
        return false;
    }
    let valid = crate::paired_frame_code::cvr_npf_code(0, base());
    pair::CyberpunkVR_PreparationRootCompatible.store(if valid { 1 } else { 2 }, SeqCst);
    valid
}
/// FrameData: the context's owner's frame data (its vtable slot 4), under SEH.
fn frame_data(context: u64) -> u64 {
    let Some(owner) = cvr_engine::code::read::<usize>(context.wrapping_add(0x38) as usize) else { return 0 };
    if owner == 0 {
        return 0;
    }
    let Some(table) = cvr_engine::code::read::<usize>(owner) else { return 0 };
    let Some(method) = cvr_engine::code::read::<usize>(table.wrapping_add(4 * 8)) else { return 0 };
    // SAFETY: the owner's own method; a fault is caught.
    unsafe { cvr_engine::code::guarded_call4(method, owner as *mut c_void, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) }
        .map_or(0, |data| data as u64)
}
extern "C" fn root_hook(renderer: *mut c_void, context: *mut c_void) -> usize {
    let caller = return_address();
    let generation = pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst);
    let mut data = 0u64;
    let request = pair::CyberpunkVR_PreparationPairRequest.load(Relaxed);
    // SAFETY: the C++ and Rust globals, read as plain values.
    let (menu, cull_reuse) = unsafe {
        (core::ptr::read_volatile(core::ptr::addr_of!(g_menuModeValue)), core::ptr::read_volatile(core::ptr::addr_of!(crate::declared_moved_exports::CyberpunkVR_CullReuseMode)))
    };
    if (request == 2 || request == 3) && prep::pairing() && menu == 0 && cull_reuse == 0 && caller == base() + 0xB60377 {
        data = frame_data(context as u64);
        if request == 3 && prep::active() {
            observe(data, caller);
        }
        // SAFETY: the C++ global.
        if prep::suppress_rtt(data, unsafe { g_vrcam_ctx_key.load(SeqCst) }, true) {
            return 0;
        }
    }
    let admission = tracking::begin_root(renderer as u64, caller);
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(ROOT) };
    let result = original(renderer, context);
    tracking::end_root(admission);
    if data != 0 {
        prep::root_completed(data, generation);
    }
    result
}

// ---- the owned paired graph snapshot (0x547CF4)
static mut DECLARE: usize = 0;
extern "C" fn graph_compatible() -> bool {
    env_paired_root() && crate::paired_frame_code::cvr_npf_code(1, base())
}
fn native_sort_compatible() -> bool {
    static VALID: OnceLock<bool> = OnceLock::new();
    *VALID.get_or_init(|| crate::paired_frame_code::cvr_npf_code(2, base()))
}
/// CompareOwnedGraph.
fn compare_owned_graph(c: &mut Capture) {
    let mode = x::CyberpunkVR_PreparationOwnedGraphMode.load(SeqCst);
    if mode == 0 {
        return;
    }
    // SAFETY: the exports.
    let (stats, whole_pass, context_order, shared_owner) = unsafe {
        (
            out(addr_of_mut!(x::CyberpunkVR_PreparationOwnedGraphStats)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationWholePassStats)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationContextOrderStats)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationSharedOwnerStats)),
        )
    };
    stats.fill(0);
    stats[0] = 1;
    stats[1] = u64::from(mode);
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_PreparationOwnedGraphNodes = 0;
        x::CyberpunkVR_PreparationOwnedGraphEdges = 0;
    }
    c.owned = Snapshot::default();
    whole_pass.fill(0);
    context_order.fill(0);
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_PreparationContextOrderWitnesses = 0;
        x::CyberpunkVR_PreparationContextOrderWitnessCount = 0;
    }
    c.witnesses.clear();
    shared_owner.fill(0);
    // SAFETY: the export.
    unsafe { x::CyberpunkVR_PreparationSharedOwnerRows = 0 };
    c.shared_owner_rows.clear();
    let fail = |stats: &mut [u64; 16], reason: u64| {
        stats[14] = reason;
        stats[0] = 4;
    };
    if !(1..=3).contains(&mode) || !native_sort_compatible() || !c.frontier.candidate() {
        return fail(stats, 1);
    }
    let base = base();
    stats[2] = c.graph.nodes.len() as u64;
    stats[3] = c.graph.edges.len() as u64;
    stats[15] = c.frontier.plan.fingerprint;
    let mut reference = OwnedGraph::default();
    if !owned(&mut reference, &c.graph, c.frontier.plan.original.clone()) {
        return fail(stats, 2);
    }
    stats[4] = 1;
    stats[5] = reference.same_source_orders() as u64;
    if stats[5] == 0 {
        return fail(stats, 3);
    }
    if mode == 2 || mode == 3 {
        let whole = if mode == 3 {
            let whole = Frontier::build(&c.graph, base, pf::DISCARD);
            if !whole.candidate() {
                return fail(stats, 7);
            }
            Some(whole)
        } else {
            None
        };
        let selected = whole.as_ref().unwrap_or(&c.frontier);
        let cpu = CpuFrontierPlan::build(selected).plan;
        if cpu.result != pcf::CANDIDATE_NEEDS_OWNERSHIP_AND_RESOURCE_COMPILE {
            stats[13] = cpu.failure as u64;
            return fail(stats, 4);
        }
        let mut candidate = OwnedGraph::default();
        if !owned(&mut candidate, &c.graph, cpu.proposed.clone()) {
            return fail(stats, 5);
        }
        c.owned = owned_snapshot(&candidate);
        stats[6] = 1;
        for (i, node) in c.graph.nodes.iter().enumerate() {
            let Some(compiled) = c.owned.nodes.get(i) else { break };
            for phase in 0..2 {
                stats[7 + phase] += (node.bytes[28 + phase] != compiled.bytes[28 + phase]) as u64;
            }
        }
        stats[9] = cpu.proposed[0].len() as u64;
        stats[10] = cpu.proposed[1].len() as u64;
        stats[11] = cpu.early.len() as u64;
        stats[12] = cpu.barriers.len() as u64;
        if mode == 3 {
            let plan = &selected.plan;
            let peer = &plan.roles[1];
            *whole_pass = [
                3,
                1,
                plan.fingerprint,
                c.graph.nodes.len() as u64,
                plan.original[0].len() as u64,
                plan.original[1].len() as u64,
                cpu.proposed[0].len() as u64,
                cpu.proposed[1].len() as u64,
                cpu.early.len() as u64,
                cpu.barriers.len() as u64,
                cpu.frontier.len() as u64,
                cpu.removed[0].len() as u64,
                cpu.added[0].len() as u64,
                u64::from(plan.early_tail()),
                u64::from(peer.after_discard),
                0,
            ];
            for &edge in &cpu.shared_owner_orders {
                let Some(source) = c.graph.nodes.iter().find(|node| node.handle & 0xDFFF_FFFF == edge.0) else {
                    return fail(stats, 11);
                };
                let kind = if cpu.reversed_owner_orders.contains(&edge) {
                    2
                } else if cpu.restored_owner_orders.contains(&edge) {
                    1
                } else {
                    0
                };
                c.shared_owner_rows.push([field_u64(&source.bytes, 16), u64::from(edge.0), u64::from(edge.1), kind]);
            }
            *shared_owner = [
                3,
                plan.fingerprint,
                cpu.shared_owner_orders.len() as u64,
                cpu.restored_owner_orders.len() as u64,
                cpu.reversed_owner_orders.len() as u64,
                cpu.invariants.shared_owner_order_preserved as u64,
                c.shared_owner_rows.len() as u64,
                0,
            ];
            // SAFETY: the export.
            unsafe { x::CyberpunkVR_PreparationSharedOwnerRows = data(&c.shared_owner_rows) };
            let mut phase_only = OwnedGraph::default();
            if !owned(&mut phase_only, &c.graph, plan.proposed.clone()) {
                return fail(stats, 8);
            }
            let phase_snapshot = owned_snapshot(&phase_only);
            let scenarios = [&c.graph, &phase_snapshot, &c.owned];
            let mut witnesses = core::mem::take(&mut c.witnesses);
            for (i, scenario) in scenarios.iter().enumerate() {
                let mut reader = |address: u64, out: &mut [u8]| read(address, out);
                let proof = crate::context_order::analyze(&scenario.nodes, &scenario.edges, scenario.failure, base, Some(&mut reader as &mut dyn FnMut(u64, &mut [u8]) -> bool));
                context_order[i * 8..i * 8 + 8].copy_from_slice(&[
                    proof.failure as u64,
                    proof.fingerprint,
                    u64::from(proof.publishers),
                    u64::from(proof.synchronizers),
                    u64::from(proof.required_pairs),
                    u64::from(proof.proved_pairs),
                    proof.missing.len() as u64,
                    0,
                ]);
                for w in &proof.missing {
                    witnesses.push([i as u32, w.producer, w.consumer, w.producer_slot, w.consumer_slot, w.kind]);
                }
                if witnesses.len() > 4096 {
                    c.witnesses = witnesses;
                    return fail(stats, 9);
                }
            }
            c.witnesses = witnesses;
            // SAFETY: the exports.
            unsafe {
                x::CyberpunkVR_PreparationContextOrderWitnesses = data(&c.witnesses);
                x::CyberpunkVR_PreparationContextOrderWitnessCount = c.witnesses.len() as u32;
            }
            // This checks the native publication/cursor dependency, not ownership or GPU aliasing. The candidate is
            // sorted only, never executed here.
            if context_order[0] != 0 || context_order[16] != 0 {
                return fail(stats, 10);
            }
        }
    } else {
        c.owned = owned_snapshot(&reference);
    }
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_PreparationOwnedGraphNodes = data(&c.owned.nodes);
        x::CyberpunkVR_PreparationOwnedGraphEdges = data(&c.owned.edges);
    }
    stats[0] = 3;
}
/// The frontier's published rows (GraphHook, a valid capture).
fn publish_frontier(c: &Capture) {
    let plan = &c.frontier.plan;
    // SAFETY: the exports.
    let (stats, roles, cuts, adds) = unsafe {
        (
            out(addr_of_mut!(x::CyberpunkVR_PreparationFrontierStats)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationFrontierRoles)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationFrontierCuts)),
            out(addr_of_mut!(x::CyberpunkVR_PreparationFrontierAdds)),
        )
    };
    stats.fill(0);
    roles.fill(0);
    cuts.fill(0);
    adds.fill(0);
    stats[0] = u64::from(plan.result);
    stats[1] = plan.failure as u64;
    stats[2] = 0; // ExecutionReady: never from this planner.
    stats[3] = plan.fingerprint;
    stats[4] = c.graph.nodes.len() as u64;
    stats[5] = plan.original[0].len() as u64;
    stats[6] = plan.original[1].len() as u64;
    stats[7] = plan.proposed[0].len() as u64;
    stats[8] = plan.proposed[1].len() as u64;
    stats[9] = plan.cpu_blocker.len() as u64;
    stats[10] = plan.order_blocker.len() as u64;
    stats[11] = plan.peer_cpu_closure.len() as u64;
    stats[12] = plan.cpu_incoming.len() as u64;
    stats[13] = plan.removed[1].len() as u64;
    stats[14] = plan.added[1].len() as u64;
    let p = &plan.invariants;
    let checks = [
        p.original_cpu_acyclic,
        p.original_order_acyclic,
        p.candidate_order_acyclic,
        p.cpu_unchanged,
        p.unrelated_order_edges_preserved,
        p.both_producers_before_main_discard,
        p.main_geometry_before_lighting,
        p.peer_geometry_before_lighting,
        p.cpu_producer_closure_proved,
        p.resource_order_compiled,
        p.parent_pass_mapping_proved,
    ];
    for (i, &check) in checks.iter().enumerate() {
        if check {
            stats[15] |= 1u64 << i;
        }
    }
    for (eye, r) in plan.roles.iter().enumerate() {
        let values = [r.collector, r.depth, r.solid_ready, r.discard, r.post_gbuffer, r.lighting, r.final_2d];
        for (k, &value) in values.iter().enumerate() {
            roles[eye * 11 + k] = u64::from(value);
        }
        for (i, &solid) in r.solid.iter().take(4).enumerate() {
            roles[eye * 11 + 7 + i] = u64::from(solid);
        }
    }
    for (index, &(from, to)) in plan.removed[1].iter().take(3).enumerate() {
        cuts[index * 2] = u64::from(from);
        cuts[index * 2 + 1] = u64::from(to);
    }
    for (index, &(from, to)) in plan.added[1].iter().take(3).enumerate() {
        adds[index * 2] = u64::from(from);
        adds[index * 2 + 1] = u64::from(to);
    }
}
extern "C" fn graph_hook(graph: *mut c_void, context: *mut c_void, job: *mut c_void) -> usize {
    let caller = return_address();
    let declaration = tracking::before_declare(graph as u64, context as u64, job as u64, caller);
    let declare = || {
        // SAFETY: MinHook's trampoline to the original.
        let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(DECLARE) };
        let result = original(graph, context, job);
        tracking::after_declare(declaration);
        result
    };
    if prep::active() && pair::CyberpunkVR_PreparationPairState.load(SeqCst) == 2 && x::CyberpunkVR_PreparationGraphRequest.load(SeqCst) != 0 {
        let mut c = lock(&CAPTURE);
        if !prep::active() || pair::CyberpunkVR_PreparationPairState.load(SeqCst) != 2 || x::CyberpunkVR_PreparationGraphRequest.swap(0, SeqCst) == 0 {
            drop(c);
            return declare();
        }
        capture_graph(&mut c, graph as u64, context as u64, declaration);
    }
    declare()
}
/// GraphHook's capture, under the capture mutex.
fn capture_graph(c: &mut Capture, graph: u64, context: u64, declaration: u64) {
    let base = base();
    let capture_mode = x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst);
    let capture_epoch = x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst);
    let capture_generation = pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst);
    x::CyberpunkVR_PreparationGraphState.store(1, SeqCst);
    x::CyberpunkVR_PreparationGraphSeq.fetch_add(1, SeqCst);
    {
        let mut j = lock(&JOBS);
        arm_graph_jobs(&mut j);
        j.capture_epoch = u64::from(capture_epoch);
        j.capture_generation = capture_generation;
        j.declaration_owner = None;
        j.capture_owner = read_u64(context).1;
        let owner = j.capture_owner;
        let mut references = [0u8; 4];
        if capture_mode == 1 && owner != 0 && owner <= u64::MAX - 12 && read(owner + 8, &mut references) && i32::from_le_bytes(references) > 0 {
            j.declaration_owner = Some(DeclarationOwner::new(owner));
        } else {
            j.capture_owner = 0;
        }
    }
    let captured = catch_unwind(AssertUnwindSafe(|| {
        (match tracking::copy_owned_declaration_snapshot(declaration) {
            Some(copy) => {
                c.graph = copy;
                true
            }
            None => false,
        }) || {
            let mut reader = |address: u64, out: &mut [u8]| read(address, out);
            c.graph.capture(graph, &mut reader)
        }
    }));
    let valid = captured.unwrap_or_else(|_| {
        c.graph.failure = 6;
        false
    });
    // SAFETY: the exports.
    unsafe {
        let meta = out(addr_of_mut!(x::CyberpunkVR_PreparationGraphMeta));
        meta[0] = graph;
        meta[1] = base;
        meta[2] = context;
        meta[3] = u64::from(pair::CyberpunkVR_PreparationRootSkipped.load(SeqCst));
    }
    {
        let mut resource = lock(&RESOURCE);
        resource.uses = resource_uses::Snapshot::default();
        resource.pool = 0;
        resource.wrapper = 0;
        // SAFETY: the exports.
        unsafe {
            out(addr_of_mut!(x::CyberpunkVR_PreparationResourceUsesStats)).fill(0);
            x::CyberpunkVR_PreparationResourceUsesRows = 0;
            x::CyberpunkVR_PreparationResourceUseIds = 0;
            x::CyberpunkVR_PreparationResourceAuxKeys = 0;
            x::CyberpunkVR_PreparationResourceBoundaryIds = 0;
        }
        let (ok, wrapper) = read_u64(context + 8);
        resource.wrapper = wrapper;
        if ok && wrapper != 0 {
            resource.pool = read_u64(wrapper).1;
        }
    }
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_PreparationGraphNodes = data(&c.graph.nodes);
        x::CyberpunkVR_PreparationGraphEdges = data(&c.graph.edges);
    }
    x::CyberpunkVR_PreparationGraphNodeCount.store(c.graph.nodes.len() as u32, SeqCst);
    x::CyberpunkVR_PreparationGraphEdgeCount.store(c.graph.edges.len() as u32, SeqCst);
    x::CyberpunkVR_PreparationGraphFailure.store(c.graph.failure, SeqCst);
    prep::publish_graph_parent(&c.graph.nodes, &c.graph.edges, c.graph.failure, base);
    if valid {
        c.frontier = Frontier::build(&c.graph, base, pf::SOLID);
        publish_frontier(c);
        if catch_unwind(AssertUnwindSafe(|| compare_owned_graph(c))).is_err() {
            // SAFETY: the export.
            let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationOwnedGraphStats)) };
            stats[0] = 4;
            stats[14] = 6;
        }
    }
    x::CyberpunkVR_PreparationGraphSeq.fetch_add(1, SeqCst);
    if capture_mode == 0
        || (x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == capture_mode
            && x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst) == capture_epoch
            && pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst) == capture_generation)
    {
        x::CyberpunkVR_PreparationGraphState.store(if valid { 2 } else { 3 }, Release);
    }
    if valid {
        let owner = lock(&JOBS).capture_owner;
        tracking::match_snapshot(&c.graph, graph, owner, u64::from(capture_epoch), capture_generation);
    }
    let mut j = lock(&JOBS);
    select_graph_jobs(&mut j, c);
}

// ---- the retained complete native graph jobs (0x32D60C)
static mut QUEUE: usize = 0;
extern "C" fn graph_jobs_compatible() -> bool {
    if !env_paired_root() {
        return false;
    }
    static VALID: OnceLock<bool> = OnceLock::new();
    let valid = *VALID.get_or_init(|| {
        // SAFETY: the export.
        let diagnostic = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsCompatibility)) };
        diagnostic[0] = 2;
        // The schedule contract includes this function's original body. Cache it before installing our queue
        // observer, not after its detour is enabled.
        diagnostic[2] = schedule_compatible() as u64;
        if diagnostic[2] == 0 {
            return false;
        }
        let base = base();
        diagnostic[1] = base;
        // SAFETY: three of the export's words.
        if !unsafe { crate::paired_frame_code::cvr_npf_graph_jobs(base, diagnostic.as_mut_ptr().add(3).cast()) } {
            return false;
        }
        diagnostic[0] = 1;
        true
    });
    if valid {
        prep::set_graph_job_poll(poll_owned_graph_jobs);
    }
    valid
}
extern "C" fn queue_hook(owner: *mut c_void, graph: *mut c_void, groups: *mut c_void) -> usize {
    let caller = return_address();
    let replay = tracking::begin_resource_queue(owner as u64, graph as u64);
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(QUEUE) };
    let result = original(owner, graph, groups);
    tracking::end_resource_queue(replay);
    let base = base();
    // SAFETY: the export.
    let meta0 = unsafe { x::CyberpunkVR_PreparationGraphMeta[0] };
    if x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 1 {
        // SAFETY: the export's words, updated atomically as the Interlocked calls did.
        let d = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsCompatibility)) }.as_mut_ptr();
        // SAFETY: as above.
        unsafe {
            atomic_u64(d.add(6)).fetch_add(1, SeqCst);
            atomic_u64(d.add(7)).swap(if graph as u64 == meta0 { u64::from(x::CyberpunkVR_PreparationGraphState.load(SeqCst)) } else { 10 }, SeqCst);
        }
    }
    if x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 1
        && prep::active()
        && pair::CyberpunkVR_PreparationPairState.load(SeqCst) == 2
        && caller == base + QUEUE_RETURN
    {
        let Some((epoch, generation)) = tracking::queue_identity(owner as u64, graph as u64, caller) else {
            return result;
        };
        let c = lock(&CAPTURE);
        let mut j = lock(&JOBS);
        arm_graph_jobs(&mut j);
        // SAFETY: the export.
        let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsStats)) };
        if matches!(stats[0], 2..=4) {
            return result;
        }
        let current = || {
            x::CyberpunkVR_PreparationGraphJobsMode.load(SeqCst) == 1
                && u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)) == epoch
                && pair::CyberpunkVR_PreparationPairGeneration.load(SeqCst) == generation
        };
        let fail = |stats: &mut [u64; 8], reason: u64| {
            if current() {
                stats[6] = reason;
                stats[0] = 4;
            }
        };
        let captured = catch_unwind(AssertUnwindSafe(|| {
            let mut reader = |address: u64, out: &mut [u8]| read(address, out);
            let mut source = Snapshot::default();
            if !source.capture(graph as u64, &mut reader) {
                return;
            }
            let frontier = pf::Plan::build_candidate(&source.nodes, &source.edges, source.failure, base, pf::SOLID);
            if frontier.result != pf::CANDIDATE_NEEDS_CPU_AND_RESOURCE_COMPILE || epoch == 0 || !current() {
                return;
            }
            let (ok, frame_owner) = read_u64(owner as u64);
            if !ok {
                return;
            }
            if j.receipts.iter().any(|r| r.0.matches(epoch, generation, base, graph as u64, frame_owner, &source)) {
                return;
            }
            if j.receipts.len() >= MAX_PENDING_RECEIPTS {
                return fail(stats, 102);
            }
            stats[0] = 1;
            let mut plan = CapturePlan::default();
            if !plan.capture(owner as u64, graph as u64, groups as u64, caller, base, &source, Some(&mut reader as &mut dyn FnMut(u64, &mut [u8]) -> bool)) {
                if current() {
                    // SAFETY: the export.
                    unsafe { *out(addr_of_mut!(x::CyberpunkVR_PreparationGraphJobsSourceReject)) = *plan.diagnostic() };
                }
                return fail(stats, plan.rejection() as u64);
            }
            let mut receipt = Box::new(DeferredReceipt::new(
                JobLeaseApi { retain: Some(retain_graph_job), release: Some(release_graph_job), complete: Some(complete_graph_job) },
                FrameOwnerApi { retain: Some(retain_graph_owner), release: Some(release_graph_owner) },
            ));
            // SAFETY: the queue still owns the plan's frame owner and groups.
            if !unsafe { receipt.capture(&plan, epoch, generation, base, Some(&mut reader as &mut dyn FnMut(u64, &mut [u8]) -> bool)) } {
                return fail(stats, 100);
            }
            if !current() {
                return;
            }
            j.receipts.push(Receipt(receipt));
            select_graph_jobs(&mut j, &c);
        }));
        if captured.is_err() {
            fail(stats, 101);
        }
    }
    result
}

// ---- the exact pre-compiler resource uses (0x3E0330)
static mut POOL: usize = 0;
extern "C" fn resource_uses_compatible() -> bool {
    if !env_paired_root() {
        return false;
    }
    let base = base();
    if !crate::paired_frame_code::cvr_npf_code(3, base) {
        return false;
    }
    let valid = crate::paired_frame_code::cvr_npf_code(4, base);
    x::CyberpunkVR_PreparationResourceMarkerContract.store(if valid { 1 } else { 2 }, SeqCst);
    true
}
extern "C" fn compile_pool_hook(pool: *mut c_void, mode: u8, job: *mut c_void) -> usize {
    let caller = return_address();
    let reset = tracking::before_resource_reset(pool as u64, mode, caller);
    let owned = tracking::before_owned_resource_compile(pool as u64, mode, job as u64, caller);
    let sorting_mode = x::CyberpunkVR_PreparationOwnedGraphMode.load(SeqCst);
    let (mut captured, mut captured_sequence, mut captured_fingerprint, mut captured_epoch) = (false, 0u32, 0u64, 0u32);
    // SAFETY: the export.
    let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationResourceUsesStats)) };
    if mode == 2 && caller == base() + 0xBB68B5 && (sorting_mode == 2 || sorting_mode == 3) && x::CyberpunkVR_PreparationGraphState.load(SeqCst) == 2 && prep::active() {
        let c = lock(&CAPTURE);
        let mut resource = lock(&RESOURCE);
        if x::CyberpunkVR_PreparationGraphState.load(SeqCst) == 2 && stats[0] == 0 && pool as u64 == resource.pool {
            stats[0] = 1;
            stats[1] = pool as u64;
            stats[2] = resource.wrapper;
            stats[3] = c.frontier.plan.fingerprint;
            // SAFETY: the export.
            let root = unsafe { x::CyberpunkVR_PreparationGraphMeta[2] };
            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut reader = |address: u64, out: &mut [u8]| read(address, out);
                resource.uses.capture_pool(pool as u64, &c.graph, Some(&mut reader as &mut dyn FnMut(u64, &mut [u8]) -> bool), root)
            }));
            match result {
                Ok(false) => {
                    stats[10] = resource.uses.failure as u64;
                    stats[0] = 4;
                }
                Ok(true) => {
                    let uses = &resource.uses;
                    stats[4] = uses.use_ids.len() as u64;
                    stats[5] = uses.declarations.len() as u64;
                    stats[6] = uses.auxiliary_keys.len() as u64;
                    stats[7] = uses.boundary_ids.len() as u64;
                    stats[8] = core::mem::size_of::<resource_uses::Declaration>() as u64;
                    stats[9] = uses.opaque_state_unsupported as u64;
                    // SAFETY: the exports.
                    unsafe {
                        x::CyberpunkVR_PreparationResourceUsesRows = data(&uses.declarations);
                        x::CyberpunkVR_PreparationResourceUseIds = data(&uses.use_ids);
                        x::CyberpunkVR_PreparationResourceAuxKeys = data(&uses.auxiliary_keys);
                        x::CyberpunkVR_PreparationResourceBoundaryIds = data(&uses.boundary_ids);
                    }
                    stats[0] = 2;
                    captured = true;
                    captured_sequence = x::CyberpunkVR_PreparationGraphSeq.load(SeqCst);
                    captured_epoch = x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst);
                    captured_fingerprint = stats[3];
                }
                Err(_) => {
                    stats[10] = 100;
                    stats[0] = 4;
                }
            }
        }
    }
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, u8, *mut c_void) -> usize = unsafe { core::mem::transmute(POOL) };
    let result = original(pool, mode, job);
    tracking::after_resource_reset(reset, pool as u64, mode);
    tracking::after_owned_resource_compile(owned, pool as u64);
    if captured && mode == 2 && x::CyberpunkVR_PreparationOwnedGraphMode.load(SeqCst) == sorting_mode {
        let resource = lock(&RESOURCE);
        if stats[0] == 2
            && pool as u64 == resource.uses.pool
            && x::CyberpunkVR_PreparationGraphState.load(SeqCst) == 2
            && captured_sequence == x::CyberpunkVR_PreparationGraphSeq.load(SeqCst)
            && captured_epoch == x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)
            && captured_fingerprint == stats[3]
        {
            let mut after = [0u8; 1];
            if read(pool as u64 + 2, &mut after) && after[0] == 2 {
                stats[11] = 1;
                stats[0] = 3;
            } else {
                stats[0] = 4;
                stats[10] = 101;
            }
        }
    }
    result
}

// ---- the native proxy preparation inputs (0x33020C)
static mut PROXY: usize = 0;
extern "C" fn proxy_gather_compatible() -> bool {
    env_paired_root() && crate::paired_frame_code::cvr_npf_code(5, base())
}
extern "C" fn proxy_gather_hook(context: *mut c_void, range: *mut c_void) -> usize {
    let mut ticket = prep::MaterializeTicket::default();
    if prep::active() {
        let mut side = -1;
        let (ok, view) = read_u64(context as u64 + 0x18);
        if ok && view != 0 {
            let (ok, name) = read_u64(view + 0x28);
            if ok {
                // SAFETY: the C++ global.
                if name == 0 {
                    side = 0;
                } else if name == unsafe { g_vrcam_ctx_key.load(SeqCst) } {
                    side = 1;
                }
            }
        }
        ticket = prep::begin_proxy_gather(context as u64, range as u64, side);
    }
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(PROXY) };
    let result = original(context, range);
    prep::end_materialize(&ticket, prep::kind::PROXY_GATHER_END);
    result
}

// ---- the bounded CPU preparation schedule (0x8EA1C8)
static mut DEPENDENCIES: usize = 0;
/// JobGraph: the shadow 0x8EA1C8 reads synchronously to create native job-group dependencies (0x32D60C still gets
/// the untouched original graph; never used with graph mutation, sorting or execution).
#[repr(C, align(8))]
struct JobHeader([u8; 0x70]);
struct JobGraph {
    header: JobHeader,
    nodes: Vec<[u8; 48]>,
    edges: Vec<[u32; 6]>,
    edge_order: Vec<u32>,
}
/// preparation_schedule::PreparedPlan: immutable once built, retained through the native call.
struct PreparedPlan {
    source: Snapshot,
    base: u64,
    plan: ps::Plan,
    jobs: JobGraph,
    edges: Vec<[u32; 2]>,
}
impl PreparedPlan {
    fn matches(&self, value: &Snapshot, image: u64) -> bool {
        value.failure == 0 && self.base == image && self.source.nodes == value.nodes && self.source.edges == value.edges
    }
    /// The plan and its job graph, boxed in place so the header's pointers stay valid.
    fn build(source: &Snapshot, base: u64) -> Result<Arc<PreparedPlan>, u32> {
        let plan = ps::Plan::build(&source.nodes, &source.edges, source.failure, base);
        if plan.failure != 0 {
            return Err(plan.failure);
        }
        let cpu: Vec<[u32; 2]> = plan.cpu.iter().map(|&(a, b)| [a, b]).collect();
        // SAFETY: the source's nodes; a null output asks for the slot count.
        let slots = unsafe { ps::cvr_ps_job_records(source.nodes.as_ptr(), source.nodes.len(), core::ptr::null(), 0, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) };
        let mut jobs = JobGraph { header: JobHeader([0; 0x70]), nodes: vec![[0; 48]; slots as usize], edges: vec![[0; 6]; cpu.len()], edge_order: vec![0; cpu.len()] };
        // SAFETY: the outputs have the room asked for.
        unsafe {
            ps::cvr_ps_job_records(source.nodes.as_ptr(), source.nodes.len(), cpu.as_ptr(), cpu.len(), jobs.nodes.as_mut_ptr(), jobs.edges.as_mut_ptr(), jobs.edge_order.as_mut_ptr());
            ps::cvr_ps_job_header(&mut jobs.header.0, data(&jobs.nodes), jobs.edges.len() as u32, data(&jobs.edges), data(&jobs.edge_order));
        }
        Ok(Arc::new(PreparedPlan { source: source.clone(), base, plan, jobs, edges: cpu }))
    }
}
/// preparation_schedule::PlanCache (the caller serializes acquisition).
#[derive(Default)]
struct PlanCache {
    failure: u32,
    recent: [Option<Arc<PreparedPlan>>; 4],
}
impl PlanCache {
    fn acquire(&mut self, source: &Snapshot, base: u64, reuse: bool, hit: &mut bool) -> Option<Arc<PreparedPlan>> {
        self.failure = 0;
        *hit = false;
        if reuse {
            if let Some(i) = self.recent.iter().position(|p| p.as_ref().is_some_and(|p| p.matches(source, base))) {
                *hit = true;
                let result = self.recent[i].clone();
                self.recent[..=i].rotate_right(1);
                self.recent[0] = result.clone();
                return result;
            }
        }
        let next = match PreparedPlan::build(source, base) {
            Ok(next) => next,
            Err(failure) => {
                self.failure = failure;
                return None;
            }
        };
        if !reuse {
            self.recent = Default::default();
        }
        self.recent.rotate_right(1);
        self.recent[0] = Some(next.clone());
        Some(next)
    }
}
#[derive(Default)]
struct Schedule {
    cache: PlanCache,
    published: Option<Arc<PreparedPlan>>,
}
fn schedule_time(phase: usize, ticks: u64) {
    x::CyberpunkVR_PreparationScheduleCpu[phase * 2].fetch_add(1, SeqCst);
    x::CyberpunkVR_PreparationScheduleCpu[phase * 2 + 1].fetch_add(ticks, SeqCst);
}
extern "C" fn schedule_compatible() -> bool {
    if !env_paired_root() {
        return false;
    }
    static VALID: OnceLock<bool> = OnceLock::new();
    let valid = *VALID.get_or_init(|| {
        let matched = crate::paired_frame_code::cvr_npf_code(6, base());
        let mut frequency = 0i64;
        // SAFETY: a plain query.
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency) };
        // SAFETY: the export.
        unsafe { x::CyberpunkVR_PreparationScheduleCpuFrequency = frequency as u64 };
        matched
    });
    x::CyberpunkVR_PreparationScheduleCompatible.store(if valid { 1 } else { 2 }, SeqCst);
    valid
}
extern "C" fn dependencies_hook(graph: *mut c_void, groups: *mut c_void, parent: *mut c_void) -> usize {
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize = unsafe { core::mem::transmute(DEPENDENCIES) };
    let mode = x::CyberpunkVR_PreparationScheduleMode.load(Acquire);
    let base = base();
    if (mode != 1 && mode != 2) || !prep::pairing() || pair::CyberpunkVR_PreparationPairState.load(SeqCst) != 2 || return_address() != base + 0x3E9050 {
        return original(graph, groups, parent);
    }
    let mut snapshot = Snapshot::default();
    let mut selected: Option<Arc<PreparedPlan>> = None;
    let mut apply = false;
    {
        let mut s = lock(&SCHEDULE);
        x::CyberpunkVR_PreparationScheduleSeq.fetch_add(1, SeqCst);
        // SAFETY: the export.
        let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationScheduleStats)) };
        let (mut failure, mut eligible) = (0u32, true);
        let capture_begin = now();
        let mut capture_ticks = 0u64;
        let planned = catch_unwind(AssertUnwindSafe(|| {
            let mut reader = |address: u64, out: &mut [u8]| read(address, out);
            let captured = snapshot.capture(graph as u64, &mut reader);
            capture_ticks = now() - capture_begin;
            if !captured {
                failure = 10 + snapshot.failure;
            } else if !snapshot.nodes.iter().any(|n| field_u32(&n.bytes, 24) == 1) {
                eligible = false;
            } else {
                let plan_begin = now();
                // SAFETY: the exports.
                let pair_data = unsafe { *core::ptr::addr_of!(pair::CyberpunkVR_PreparationPairData) };
                let (main, eye) = (pair_data[4], pair_data[3]);
                let mut raw = [0u8; 32];
                let inputs = read(main.wrapping_add(0x2C40), &mut raw[..16]) && read(eye.wrapping_add(0x2C40), &mut raw[16..]);
                let mut ranges = [0u64; 4];
                for (i, range) in ranges.iter_mut().enumerate() {
                    *range = u64::from_le_bytes(raw[i * 8..i * 8 + 8].try_into().unwrap());
                }
                let inputs = inputs && ranges[0] == pair_data[6] && ranges[2] == pair_data[5];
                // SAFETY: the export.
                unsafe { x::CyberpunkVR_PreparationScheduleRanges = ranges };
                if !inputs || !ps::independent(&ranges) {
                    failure = 20;
                } else {
                    let mut hit = false;
                    let reuse = x::CyberpunkVR_PreparationScheduleCache.load(SeqCst) != 0;
                    selected = s.cache.acquire(&snapshot, base, reuse, &mut hit);
                    match &selected {
                        None => failure = 30 + s.cache.failure,
                        Some(chosen) => {
                            s.published = Some(chosen.clone());
                            // SAFETY: the export (the published plan keeps the edges alive).
                            unsafe { x::CyberpunkVR_PreparationScheduleEdges = data(&chosen.edges) };
                            let plan = &chosen.plan;
                            stats[5] = snapshot.nodes.len() as u32;
                            stats[6] = snapshot.edges.iter().filter(|e| field_u32(&e.bytes, 4) == 0).count() as u32;
                            stats[7] = chosen.edges.len() as u32;
                            stats[8] = plan.early.len() as u32;
                            stats[9] = plan.ready.len() as u32;
                            stats[10] = plan.barriers.len() as u32;
                            stats[11] = plan.frontier.len() as u32;
                            stats[1] += 1;
                            stats[if hit { 12 } else { 13 }] += 1;
                            if mode == 2 {
                                apply = true;
                                stats[2] += 1;
                            }
                        }
                    }
                }
                schedule_time(1, now() - plan_begin);
            }
        }));
        if planned.is_err() {
            failure = 40;
        }
        if eligible {
            stats[0] += 1;
            schedule_time(0, capture_ticks);
        }
        if failure != 0 {
            stats[3] += 1;
            stats[4] = failure;
        }
        x::CyberpunkVR_PreparationScheduleSeq.fetch_add(1, SeqCst);
    }
    // Retain this exact immutable shadow through the synchronous native call.
    let native_begin = now();
    let shadow = match (&selected, apply) {
        (Some(chosen), true) => chosen.jobs.header.0.as_ptr() as *mut c_void,
        _ => graph,
    };
    let result = original(shadow, groups, parent);
    if apply {
        schedule_time(2, now() - native_begin);
    }
    drop(selected);
    result
}

// ---- the C ABI
/// Registers the six detours (the .cpp's static initialization, before the install pass).
///
/// # Safety
/// Called once, before the install pass.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npf_attach() {
    use cvr_engine::detours::register;
    let none = core::ptr::null_mut();
    let detour = |f: usize| f as *mut c_void;
    register("[single-pass] bounded paired-frame root", 0x409DD4, detour(root_hook as *const () as usize), addr_of_mut!(ROOT).cast(), Some(root_compatible), none);
    register("[single-pass] owned paired graph snapshot", 0x547CF4, detour(graph_hook as *const () as usize), addr_of_mut!(DECLARE).cast(), Some(graph_compatible), none);
    register("[single-pass] retain complete native graph jobs", 0x32D60C, detour(queue_hook as *const () as usize), addr_of_mut!(QUEUE).cast(), Some(graph_jobs_compatible), none);
    register("[single-pass] exact pre-compiler resource uses", 0x3E0330, detour(compile_pool_hook as *const () as usize), addr_of_mut!(POOL).cast(), Some(resource_uses_compatible), none);
    register("[single-pass] native proxy preparation inputs", 0x33020C, detour(proxy_gather_hook as *const () as usize), addr_of_mut!(PROXY).cast(), Some(proxy_gather_compatible), none);
    register("[single-pass] bounded CPU preparation schedule", 0x8EA1C8, detour(dependencies_hook as *const () as usize), addr_of_mut!(DEPENDENCIES).cast(), Some(schedule_compatible), none);
}
/// preparation::PrepareGraphJobCodeGuards: the graph jobs' code contract cached before any detour is live.
#[no_mangle]
pub extern "C-unwind" fn cvr_npf_prepare_guards() {
    graph_jobs_compatible();
}
