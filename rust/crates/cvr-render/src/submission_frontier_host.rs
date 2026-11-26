//! The submission frontier's former .cpp half (src/Render/NativeSubmissionFrontier.cpp's Rust branch): the hook's frame
//! (a thunk hands it its caller's return address), the graph identity it reads for a range from definition tracking's
//! owned declaration snapshot, the producer nodes of that graph, the submissions' observation and the owner's runtime
//! table (submission_frontier_owner.rs). FindBoundary stays a C++ forward: its sources are C++ leases.

use crate::code_tables::Row;
use crate::context_order::{analyze, Failure};
use crate::declared_moved_exports as x;
use crate::native::graph_parent::{read_context, resolve};
use crate::native::graph_snapshot::{Node, Snapshot};
use crate::submission_frontier_owner::{cvr_sfo_enter, cvr_sfo_leave, cvr_sfo_observe_submit, GraphIdentity, GraphNode, Runtime, Ticket};
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::Ordering::*;
use std::cell::RefCell;
use std::sync::LazyLock;

/// 2.5: sub_1422701F0 (the range consumer) and the return address of its one call (sub_140655FAC's).
const HOOK_RVA: u32 = 0x22_701F0;
const CALLER_RETURN: u64 = 0x65_607B;
/// detail::Contract.
static CONTRACT: [Row; 3] = [
    Row { rva: HOOK_RVA, bytes: 0x84, hash: 0 },
    Row { rva: 0x65_5FAC, bytes: 242, hash: 0x2FCB_0595_21D7_B173 },
    Row { rva: 0x3E_7DA0, bytes: 73, hash: 0x3C36_DA23_B820_9163 },
];

/// The range consumer's own function, written by the owner's install.
static mut ORIGINAL: *mut c_void = null_mut();
type Native = unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void, u32, *mut c_void) -> u64;

// The hook's entry: a frame of its own with the six arguments passed on and the caller's return address (the entry's
// [rsp]) as a seventh.
core::arch::global_asm!(
    ".globl cvr_sfo_hook_entry",
    ".def cvr_sfo_hook_entry",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_sfo_hook_entry:",
    ".seh_proc cvr_sfo_hook_entry",
    "    sub rsp, 0x48",
    ".seh_stackalloc 0x48",
    ".seh_endprologue",
    "    mov rax, qword ptr [rsp+0x70]",
    "    mov qword ptr [rsp+0x20], rax",
    "    mov rax, qword ptr [rsp+0x78]",
    "    mov qword ptr [rsp+0x28], rax",
    "    mov rax, qword ptr [rsp+0x48]",
    "    mov qword ptr [rsp+0x30], rax",
    "    call {hook}",
    "    nop",
    "    add rsp, 0x48",
    "    ret",
    ".seh_endproc",
    hook = sym hook,
);
unsafe extern "C" {
    fn cvr_sfo_hook_entry();
}

/// The range consumer's hook: admitted by the owner around the original when the frontier is enabled.
unsafe extern "system" fn hook(
    header: *mut c_void,
    control: *mut c_void,
    end: u32,
    fences: *mut c_void,
    mode: u32,
    job: *mut c_void,
    caller: u64,
) -> u64 {
    // SAFETY: written by the install before the hook was enabled.
    let original: Native = unsafe { core::mem::transmute(ORIGINAL) };
    if x::CyberpunkVR_SubmissionFrontierEnable.load(Acquire) != 1 || cvr_host::probe::internal() {
        // SAFETY: the engine's own call.
        return unsafe { original(header, control, end, fences, mode, job) };
    }
    let mut ticket = Ticket::default();
    cvr_sfo_enter(header as u64, control as u64, end, fences as u64, mode, job as u64, caller, &mut ticket);
    // SAFETY: as above.
    let result = unsafe { original(header, control, end, fences, mode, job) };
    // SAFETY: this frame's ticket.
    unsafe { cvr_sfo_leave(&ticket, result, true) };
    result
}

/// Memory: this process's memory, through ReadProcessMemory.
fn memory(address: u64, out: &mut [u8]) -> bool {
    let mut copied = 0usize;
    address != 0
        && address <= u64::MAX - out.len() as u64
        // SAFETY: a read the process may refuse.
        && unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                address as *const c_void,
                out.as_mut_ptr().cast(),
                out.len(),
                &mut copied,
            )
        } != 0
        && copied == out.len()
}
/// GraphRead: detail::Read through Memory.
fn graph_read(address: u64, out: &mut [u8]) -> bool {
    let mut read = memory;
    crate::submission_frontier::read(&mut Some(&mut read), address, out)
}
/// detail::Field through Memory.
fn field(address: u64, offset: u64, out: &mut [u8]) -> bool {
    let mut read = memory;
    crate::submission_frontier::field(&mut Some(&mut read), address, offset, out)
}

// The graph this thread last read for a range: its status and nodes (for `graph_node`).
thread_local! {
    static GRAPH: RefCell<(u32, Vec<Node>)> = const { RefCell::new((1, Vec::new())) };
}
/// The definition-tracking copy of a declaration, kept by `copy_into`.
extern "C" fn copy_into(
    sink: *mut c_void,
    nodes: *const Node,
    node_count: usize,
    edges: *const crate::native::graph_snapshot::Edge,
    edge_count: usize,
    failure: u32,
) {
    // SAFETY: the caller's snapshot; the arrays hold their counts.
    unsafe {
        let snapshot = &mut *sink.cast::<Snapshot>();
        snapshot.nodes = if nodes.is_null() { Vec::new() } else { core::slice::from_raw_parts(nodes, node_count).to_vec() };
        snapshot.edges = if edges.is_null() { Vec::new() } else { core::slice::from_raw_parts(edges, edge_count).to_vec() };
        snapshot.failure = failure;
    }
}
/// Graph: the owned execution's consumer and its declaration's graph, the range's synchronizing context checked
/// against it, the context order's fingerprint, and the owned execution unchanged throughout (status 0 when all hold).
fn graph(base: u64, end: u32, mode: u32) -> (GraphIdentity, Snapshot) {
    let mut result = GraphIdentity::default();
    let mut snapshot = Snapshot { nodes: Vec::new(), edges: Vec::new(), failure: 0 };
    let stats = (&raw const crate::used_moved_exports::CyberpunkVR_OwnedExecutionStats) as u64;
    let sequence = crate::used_moved_exports::CyberpunkVR_OwnedExecutionSeq.load(Acquire);
    let mut before = [0u64; 24];
    // SAFETY: a local row.
    if !unsafe { crate::frontier_reads::cvr_nsfr_owned(stats, sequence, &mut before) } {
        return (result, snapshot);
    }
    result.consumer = before[1];
    result.graph_epoch = before[2];
    result.pair = before[3];
    result.frame_owner = before[6];
    result.status = 2;
    let mut copy = Snapshot { nodes: Vec::new(), edges: Vec::new(), failure: 0 };
    if !crate::definition_tracking::cvr_ndt_copy_declaration(result.consumer, (&raw mut copy).cast(), copy_into) {
        return (result, snapshot);
    }
    snapshot = copy;
    result.status = 3;
    // SAFETY: local outputs.
    if !unsafe { crate::frontier_reads::cvr_nsfr_sync_frame(base, CALLER_RETURN, &mut result.context, &mut result.definition) } {
        return (result, snapshot);
    }
    result.status = 4;
    let mut read = graph_read;
    let Some(context) = read_context(Some(&mut read), result.context) else { return (result, snapshot) };
    let mut frame_owner = [0u8; 8];
    let mut flags = [0u8; 1];
    if context.slot != end
        || !field(result.context, 0, &mut frame_owner)
        || !field(result.context, 0x30, &mut flags)
        || flags[0] & 2 == 0
    {
        return (result, snapshot);
    }
    result.status = 5;
    if u64::from_le_bytes(frame_owner) != result.frame_owner {
        return (result, snapshot);
    }
    result.status = 6;
    let receipt = resolve(&snapshot, &context);
    let sync = receipt.unwrap_or_default();
    result.sync_handle = u64::from(sync.handle);
    if receipt.is_none() || sync.object != result.definition || sync.work != base + 0x65_5FAC || sync.name as u32 != mode {
        return (result, snapshot);
    }
    result.status = 7;
    let order = analyze(&snapshot.nodes, &snapshot.edges, snapshot.failure, base, Some(&mut read));
    if order.failure != Failure::None {
        return (result, snapshot);
    }
    result.fingerprint = order.fingerprint;
    result.status = 8;
    let mut after = [0u8; 24 * 8];
    let before_bytes: Vec<u8> = before.iter().flat_map(|v| v.to_le_bytes()).collect();
    if !graph_read(stats, &mut after)
        || before_bytes[..] != after[..]
        || crate::used_moved_exports::CyberpunkVR_OwnedExecutionSeq.load(Acquire) != sequence
    {
        return (result, snapshot);
    }
    result.status = 0;
    (result, snapshot)
}
/// ReadGraph: the graph identity for a range, its graph kept on this thread.
extern "C" fn read_graph(base: u64, end: u32, mode: u32, out: *mut GraphIdentity) {
    let (identity, snapshot) = graph(base, end, mode);
    GRAPH.with(|g| *g.borrow_mut() = (identity.status, snapshot.nodes));
    // SAFETY: the owner's output.
    unsafe { *out = identity };
}
/// GraphNodeOf: in this thread's graph (when complete), whether a node of the producer slot is the context publisher,
/// and the first such node's handle, view index, work RVA and definition name.
extern "C" fn graph_node(producer: u32, base: u64, out: *mut GraphNode) {
    let mut node = GraphNode::default();
    GRAPH.with(|g| {
        let g = g.borrow();
        if g.0 != 0 {
            return;
        }
        for n in &g.1 {
            if u32::from(n.bytes[29]) != producer {
                continue;
            }
            if n.work == base + 0x33_2D5C {
                node.publisher = true;
            }
            if !node.found {
                node.found = true;
                node.handle = u64::from(n.handle);
                node.view = u64::from(u32::from_le_bytes(n.bytes[24..28].try_into().unwrap()));
                node.work = if n.work >= base { n.work - base } else { 0 };
                node.name = u64::from_le_bytes(n.definition[24..32].try_into().unwrap());
            }
        }
    });
    // SAFETY: the owner's output.
    unsafe { *out = node };
}

/// submission_frontier::ObserveSubmit.
pub fn observe_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    cvr_sfo_observe_submit(queue as u64, count, lists as u64, cvr_host::probe::internal());
}

/// The owner's runtime (formerly the .cpp's kRuntime).
pub struct Rt(pub Runtime);
// SAFETY: the exports and functions, for the process.
unsafe impl Sync for Rt {}
// SAFETY: as above.
unsafe impl Send for Rt {}
pub static RT: LazyLock<Rt> = LazyLock::new(|| {
    Rt(Runtime {
        enable: &x::CyberpunkVR_SubmissionFrontierEnable,
        installed: &x::CyberpunkVR_SubmissionFrontierInstalled,
        state: &x::CyberpunkVR_SubmissionFrontierState,
        failure: &x::CyberpunkVR_SubmissionFrontierFailure,
        seq: &x::CyberpunkVR_SubmissionFrontierSeq,
        boundary_failure: &x::CyberpunkVR_SubmissionFrontierBoundaryFailure,
        stats: &raw mut x::CyberpunkVR_SubmissionFrontierStats,
        details: &raw mut x::CyberpunkVR_SubmissionFrontierDetails,
        ranges: &raw mut x::CyberpunkVR_SubmissionFrontierRanges,
        slots: &raw mut x::CyberpunkVR_SubmissionFrontierSlots,
        submits: &raw mut x::CyberpunkVR_SubmissionFrontierSubmits,
        range_count: &raw mut x::CyberpunkVR_SubmissionFrontierRangeCount,
        slot_count: &raw mut x::CyberpunkVR_SubmissionFrontierSlotCount,
        submit_count: &raw mut x::CyberpunkVR_SubmissionFrontierSubmitCount,
        contract: CONTRACT.as_ptr(),
        contract_count: CONTRACT.len(),
        graph: read_graph,
        graph_node,
        hook: cvr_sfo_hook_entry as *mut c_void,
        original: (&raw mut ORIGINAL).cast(),
    })
});
