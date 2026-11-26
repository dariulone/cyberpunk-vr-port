//! The C ABI of the engine job groups a compiled render graph's nodes run in (native::graph_jobs) for
//! include/Render/NativeGraphJobs.hpp: in the Rust build the header's CapturePlan keeps a capture's fields (its source
//! graph as the C++ shared snapshot) and hands them here to capture and revalidate; its LeaseSet owns a Rust lease set
//! (the engine's retain / release / complete calls stay its API). tools/single_pass_tests/graph_jobs_test runs its
//! cases against these in its Rust variant.
use std::sync::Arc;

use crate::graph_snapshot_abi::Read;
use crate::native::graph_jobs::{CapturePlan, LeaseSet, Mapping, NativeArray};
use crate::native::graph_snapshot::{Edge, Node, Snapshot};
use crate::native::job_lease::JobLeaseApi;

/// A graph snapshot's records, borrowed.
#[repr(C)]
pub struct SourceView {
    pub nodes: *const Node,
    pub node_count: usize,
    pub edges: *const Edge,
    pub edge_count: usize,
    pub failure: u32,
}
/// A ready CapturePlan's fields, borrowed.
#[repr(C)]
pub struct PlanView {
    pub owner_address: u64,
    pub owner: u64,
    pub graph: u64,
    pub groups_address: u64,
    pub base: u64,
    pub array: NativeArray,
    pub mapping: *const Mapping,
    pub mapping_count: usize,
    pub source: SourceView,
    pub ready: bool,
}
/// What CapturePlan::Capture yields: the failure and the diagnostic either way; on success the frame owner, the
/// group array header and the node mapping (borrowed until cvr_gj_plan_free).
#[repr(C)]
pub struct PlanOut {
    pub failure: u32,
    pub diagnostic: [u64; 8],
    pub owner: u64,
    pub array: NativeArray,
    pub mapping: *const Mapping,
    pub mapping_count: usize,
}

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}
fn reader(read: Read) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}
unsafe fn source_of(view: &SourceView) -> Snapshot {
    unsafe {
        Snapshot {
            nodes: slice(view.nodes, view.node_count).to_vec(),
            edges: slice(view.edges, view.edge_count).to_vec(),
            failure: view.failure,
        }
    }
}
/// The view's plan as a native one (None when the C++ plan is not ready).
unsafe fn plan_of(view: &PlanView) -> Option<CapturePlan> {
    unsafe {
        view.ready.then(|| {
            CapturePlan::restored(
                view.owner_address,
                view.owner,
                view.graph,
                view.groups_address,
                view.base,
                view.array,
                slice(view.mapping, view.mapping_count).to_vec(),
                Arc::new(source_of(&view.source)),
            )
        })
    }
}

/// CapturePlan::Capture: the native plan (its fields through `out`, free it with cvr_gj_plan_free) or null.
///
/// # Safety
/// `source` holds its counts; `out` is writable; `read` follows graph_snapshot::Read.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_gj_plan_capture(
    render_owner: u64,
    graph: u64,
    groups: u64,
    caller: u64,
    image_base: u64,
    source: *const SourceView,
    read: Option<Read>,
    out: *mut PlanOut,
) -> *mut CapturePlan {
    unsafe {
        let source = source_of(&*source);
        let mut adapter = read.map(reader);
        let mut plan = CapturePlan::default();
        let captured = plan.capture(
            render_owner,
            graph,
            groups,
            caller,
            image_base,
            &source,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        );
        *out = PlanOut {
            failure: plan.rejection() as u32,
            diagnostic: *plan.diagnostic(),
            owner: 0,
            array: NativeArray::default(),
            mapping: core::ptr::null(),
            mapping_count: 0,
        };
        if !captured {
            return core::ptr::null_mut();
        }
        let plan = Box::into_raw(Box::new(plan));
        (*out).owner = (*plan).frame_owner();
        (*out).array = (*plan).array();
        (*out).mapping = (*plan).nodes().as_ptr();
        (*out).mapping_count = (*plan).nodes().len();
        plan
    }
}

/// # Safety
/// `plan` comes from cvr_gj_plan_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_plan_free(plan: *mut CapturePlan) {
    if !plan.is_null() {
        drop(unsafe { Box::from_raw(plan) });
    }
}

/// CapturePlan::Matches.
///
/// # Safety
/// The view holds its counts; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_plan_matches(
    plan: *const PlanView,
    read: Option<Read>,
) -> bool {
    unsafe {
        let (Some(plan), Some(read)) = (plan_of(&*plan), read) else {
            return false;
        };
        plan.matches(&mut reader(read))
    }
}

/// A lease set over the engine's group calls; free it with cvr_gj_leases_free (which releases every held group).
///
/// # Safety
/// `api` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_leases_new(api: *const JobLeaseApi) -> *mut LeaseSet {
    Box::into_raw(Box::new(LeaseSet::new(unsafe { *api })))
}

/// # Safety
/// `leases` comes from cvr_gj_leases_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_leases_free(leases: *mut LeaseSet) {
    if !leases.is_null() {
        drop(unsafe { Box::from_raw(leases) });
    }
}

/// LeaseSet::Capture.
///
/// # Safety
/// `leases` is live; the view holds its counts and its groups are live engine job groups; `read` follows
/// graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_leases_capture(
    leases: *mut LeaseSet,
    plan: *const PlanView,
    read: Option<Read>,
) -> bool {
    unsafe {
        let (Some(plan), Some(read)) = (plan_of(&*plan), read) else {
            return false;
        };
        (*leases).capture(&plan, &mut reader(read))
    }
}

/// LeaseSet::Complete() (`ordinal` = u32::MAX) or Complete(ordinal, handle).
///
/// # Safety
/// `leases` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_leases_complete(
    leases: *const LeaseSet,
    ordinal: u32,
    handle: u32,
    any_node: bool,
) -> bool {
    unsafe {
        if any_node {
            (*leases).complete()
        } else {
            (*leases).complete_node(ordinal, handle)
        }
    }
}

/// LeaseSet::Sealed / Nodes.
///
/// # Safety
/// `leases` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gj_leases_state(
    leases: *const LeaseSet,
    nodes: *mut usize,
) -> bool {
    unsafe {
        *nodes = (*leases).nodes();
        (*leases).sealed()
    }
}

const _: () = assert!(
    core::mem::size_of::<PlanView>() == 120
        && core::mem::offset_of!(PlanView, source) == 72
        && core::mem::size_of::<PlanOut>() == 112
        && core::mem::offset_of!(PlanOut, array) == 80
);

// ---- the deferred job receipt (native::job_receipt) for include/Render/NativeGraphJobReceipt.hpp -----------------

use crate::native::job_receipt::{DeferredReceipt, FrameOwnerApi};

/// DeferredReceipt's state for the C++ face's accessors.
#[repr(C)]
pub struct ReceiptState {
    pub failure: u32,
    pub captured: bool,
    pub selected: bool,
    pub node_count: usize,
    pub epoch: u64,
    pub generation: u64,
    pub image_base: u64,
    pub graph: u64,
    pub frame_owner: u64,
}
const _: () = assert!(core::mem::size_of::<ReceiptState>() == 56);

/// A receipt over the engine's group and frame owner calls; free it with cvr_jr_free (which releases the groups, then
/// the frame owner).
///
/// # Safety
/// `jobs` and `owner` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_jr_new(
    jobs: *const JobLeaseApi,
    owner: *const FrameOwnerApi,
) -> *mut DeferredReceipt {
    unsafe { Box::into_raw(Box::new(DeferredReceipt::new(*jobs, *owner))) }
}

/// # Safety
/// `receipt` comes from cvr_jr_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_jr_free(receipt: *mut DeferredReceipt) {
    if !receipt.is_null() {
        drop(unsafe { Box::from_raw(receipt) });
    }
}

/// DeferredReceipt::Capture against a C++ plan.
///
/// # Safety
/// `receipt` is live; the view holds its counts and names live engine objects; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_jr_capture(
    receipt: *mut DeferredReceipt,
    plan: *const PlanView,
    epoch: u64,
    generation: u64,
    image_base: u64,
    read: Option<Read>,
) -> bool {
    unsafe {
        // A plan that is not ready is refused as an argument, as by the C++ rules.
        let plan = plan_of(&*plan).unwrap_or_default();
        let mut adapter = read.map(reader);
        (*receipt).capture(
            &plan,
            epoch,
            generation,
            image_base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        )
    }
}

/// DeferredReceipt::Match (`select` false) or Select (true).
///
/// # Safety
/// `receipt` is live; the view holds its counts.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_jr_match(
    receipt: *mut DeferredReceipt,
    epoch: u64,
    generation: u64,
    image_base: u64,
    graph_address: u64,
    declaration_frame_owner: u64,
    source: *const SourceView,
    select: bool,
) -> bool {
    unsafe {
        let source = source_of(&*source);
        if select {
            (*receipt).select(
                epoch,
                generation,
                image_base,
                graph_address,
                declaration_frame_owner,
                &source,
            )
        } else {
            (*receipt).matches(
                epoch,
                generation,
                image_base,
                graph_address,
                declaration_frame_owner,
                &source,
            )
        }
    }
}

/// DeferredReceipt::Complete.
///
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_jr_complete(receipt: *const DeferredReceipt) -> bool {
    unsafe { (*receipt).complete() }
}

/// The receipt's state.
///
/// # Safety
/// `receipt` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_jr_state(
    receipt: *const DeferredReceipt,
    out: *mut ReceiptState,
) {
    unsafe {
        let r = &*receipt;
        *out = ReceiptState {
            failure: r.rejection() as u32,
            captured: r.captured(),
            selected: r.selected(),
            node_count: r.node_count(),
            epoch: r.epoch(),
            generation: r.generation(),
            image_base: r.image_base(),
            graph: r.graph_address(),
            frame_owner: r.frame_owner(),
        };
    }
}

// ---- the scheduler callback's completion lease (native::task_completion) for include/Render/NativeGraphTaskCompletion.hpp

use crate::native::task_completion::TaskCompletionLease;

/// TaskCompletionLease's state for the C++ face's accessors.
#[repr(C)]
pub struct TaskState {
    pub group: u64,
    pub handle_address: u64,
    pub failure: u32,
    pub captured: bool,
    pub original_returned: bool,
}
const _: () = assert!(core::mem::size_of::<TaskState>() == 24);

/// A lease; free it with cvr_tc_free (which releases the held group).
#[no_mangle]
pub extern "C-unwind" fn cvr_tc_new() -> *mut TaskCompletionLease {
    Box::into_raw(Box::default())
}

/// # Safety
/// `lease` comes from cvr_tc_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tc_free(lease: *mut TaskCompletionLease) {
    if !lease.is_null() {
        drop(unsafe { Box::from_raw(lease) });
    }
}

/// TaskCompletionLease::CaptureBeforeCallback.
///
/// # Safety
/// `lease` is live; `api` is valid; the worker's group is a live engine job group; `read` follows
/// graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tc_capture(
    lease: *mut TaskCompletionLease,
    worker: u64,
    caller: u64,
    image_base: u64,
    root_prerequisite: u64,
    read: Option<Read>,
    api: *const JobLeaseApi,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).capture_before_callback(
            worker,
            caller,
            image_base,
            root_prerequisite,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            *api,
        )
    }
}

/// TaskCompletionLease::MarkOriginalReturned (`mark`) or Complete.
///
/// # Safety
/// `lease` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tc_step(lease: *mut TaskCompletionLease, mark: bool) -> bool {
    unsafe {
        if mark {
            (*lease).mark_original_returned()
        } else {
            (*lease).complete()
        }
    }
}

/// The lease's state.
///
/// # Safety
/// `lease` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tc_state(
    lease: *const TaskCompletionLease,
    out: *mut TaskState,
) {
    unsafe {
        let l = &*lease;
        *out = TaskState {
            group: l.group(),
            handle_address: l.handle_address(),
            failure: l.rejection() as u32,
            captured: l.captured(),
            original_returned: l.original_returned(),
        };
    }
}

// ---- the declaration's D0 completion lease (native::declaration_completion) for
// include/Render/NativeGraphDeclarationCompletion.hpp

use crate::native::declaration_completion::{
    DeclarationCompletionLease, Failure as DeclarationFailure,
};

/// DeclarationCompletionLease's state for the C++ face's accessors.
#[repr(C)]
pub struct DeclarationState {
    pub context_address: u64,
    pub group: u64,
    pub frame_owner: u64,
    pub execution_graph: u64,
    pub image_base: u64,
    pub epoch: u64,
    pub generation: u64,
    pub id: u64,
    pub failure: u32,
    pub captured: bool,
    pub matched: bool,
    pub declare_returned: bool,
    pub root_returned: bool,
    pub failed: bool,
}
const _: () = assert!(core::mem::size_of::<DeclarationState>() == 80);

/// A lease; free it with cvr_dc_free (which releases the pinned group).
#[no_mangle]
pub extern "C-unwind" fn cvr_dc_new() -> *mut DeclarationCompletionLease {
    Box::into_raw(Box::default())
}

/// # Safety
/// `lease` comes from cvr_dc_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dc_free(lease: *mut DeclarationCompletionLease) {
    if !lease.is_null() {
        drop(unsafe { Box::from_raw(lease) });
    }
}

/// DeclarationCompletionLease::CaptureBeforeConstruct.
///
/// # Safety
/// `lease` is live; `api` is valid; the context's group is a live engine job group; `read` follows
/// graph_snapshot::Read.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_dc_capture(
    lease: *mut DeclarationCompletionLease,
    owner_address: u64,
    execution_graph: u64,
    caller: u64,
    image_base: u64,
    epoch: u64,
    generation: u64,
    consumer_id: u64,
    read: Option<Read>,
    api: *const JobLeaseApi,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).capture_before_construct(
            owner_address,
            execution_graph,
            caller,
            image_base,
            epoch,
            generation,
            consumer_id,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            *api,
        )
    }
}

/// DeclarationCompletionLease::MatchBeforeDeclare.
///
/// # Safety
/// `lease` is live; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dc_match(
    lease: *mut DeclarationCompletionLease,
    job: u64,
    graph: u64,
    caller: u64,
    read: Option<Read>,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).match_before_declare(
            job,
            graph,
            caller,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        )
    }
}

/// MarkOriginalDeclareReturned (`step` 0), MarkOriginalRootReturned (1), Complete (2), or the refusal of a throwing
/// reader (3, Read).
///
/// # Safety
/// `lease` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dc_step(
    lease: *mut DeclarationCompletionLease,
    step: u32,
) -> bool {
    unsafe {
        let lease = &mut *lease;
        match step {
            0 => lease.mark_original_declare_returned(),
            1 => lease.mark_original_root_returned(),
            2 => lease.complete(),
            _ => lease.reject(DeclarationFailure::Read),
        }
    }
}

/// The lease's state.
///
/// # Safety
/// `lease` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dc_state(
    lease: *const DeclarationCompletionLease,
    out: *mut DeclarationState,
) {
    unsafe {
        let l = &*lease;
        *out = DeclarationState {
            context_address: l.context_address(),
            group: l.group(),
            frame_owner: l.frame_owner(),
            execution_graph: l.execution_graph(),
            image_base: l.image_base(),
            epoch: l.epoch(),
            generation: l.generation(),
            id: l.id(),
            failure: l.rejection() as u32,
            captured: l.captured(),
            matched: l.matched(),
            declare_returned: l.original_declare_returned(),
            root_returned: l.original_root_returned(),
            failed: l.failed(),
        };
    }
}

// ---- the native root's identity ticket (native::consumer_identity) for include/Render/NativeGraphConsumerIdentity.hpp

use crate::native::consumer_identity::{IdentityFailure, NativeRootTicket};

/// NativeRootTicket's state for the C++ face's accessors.
#[repr(C)]
pub struct TicketState {
    pub id: u64,
    pub epoch: u64,
    pub generation: u64,
    pub image_base: u64,
    pub frame_owner: u64,
    pub source_graph: u64,
    pub graph: u64,
    pub group: u64,
    pub failure: u32,
    pub captured: bool,
    pub construct_verified: bool,
    pub redirected: bool,
}
const _: () = assert!(core::mem::size_of::<TicketState>() == 72);

/// A ticket; free it with cvr_ci_free (which releases the group, then the frame owner).
#[no_mangle]
pub extern "C-unwind" fn cvr_ci_new() -> *mut NativeRootTicket {
    Box::into_raw(Box::default())
}

/// # Safety
/// `ticket` comes from cvr_ci_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ci_free(ticket: *mut NativeRootTicket) {
    if !ticket.is_null() {
        drop(unsafe { Box::from_raw(ticket) });
    }
}

/// NativeRootTicket::CaptureBeforeConstruct.
///
/// # Safety
/// `ticket` is live; `jobs` / `owner` are valid; the read owner and group are live engine objects; `read` follows
/// graph_snapshot::Read.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ci_capture(
    ticket: *mut NativeRootTicket,
    owner_address: u64,
    graph_pointer_address: u64,
    completion_handle_address: u64,
    caller: u64,
    image_base: u64,
    epoch: u64,
    generation: u64,
    monotonic_id: u64,
    read: Option<Read>,
    jobs: *const JobLeaseApi,
    owner: *const FrameOwnerApi,
    expected_execution_graph: u64,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*ticket).capture_before_construct(
            owner_address,
            graph_pointer_address,
            completion_handle_address,
            caller,
            image_base,
            epoch,
            generation,
            monotonic_id,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            *jobs,
            *owner,
            expected_execution_graph,
        )
    }
}

/// NativeRootTicket::VerifyConstruct.
///
/// # Safety
/// `ticket` is live; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ci_verify(
    ticket: *mut NativeRootTicket,
    destination: u64,
    read: Option<Read>,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*ticket).verify_construct(
            destination,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        )
    }
}

/// NativeRootTicket::MatchQueued (`execution` false) or MatchExecutionCallback (true).
///
/// # Safety
/// `ticket` is live; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ci_match(
    ticket: *const NativeRootTicket,
    payload: u64,
    graph: u64,
    caller: u64,
    read: Option<Read>,
    execution: bool,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        let read = adapter
            .as_mut()
            .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool);
        if execution {
            (*ticket).match_execution_callback(payload, graph, caller, read)
        } else {
            (*ticket).match_queued(payload, graph, caller, read)
        }
    }
}

/// The refusal of a throwing reader (Read), as by the C++ rules.
///
/// # Safety
/// `ticket` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ci_fail_read(ticket: *mut NativeRootTicket) {
    unsafe { (*ticket).set_rejection(IdentityFailure::Read) }
}

/// The ticket's state.
///
/// # Safety
/// `ticket` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ci_state(
    ticket: *const NativeRootTicket,
    out: *mut TicketState,
) {
    unsafe {
        let t = &*ticket;
        *out = TicketState {
            id: t.id(),
            epoch: t.epoch(),
            generation: t.generation(),
            image_base: t.image_base(),
            frame_owner: t.frame_owner(),
            source_graph: t.source_graph_address(),
            graph: t.graph_address(),
            group: t.group(),
            failure: t.rejection() as u32,
            captured: t.captured(),
            construct_verified: t.construct_verified(),
            redirected: t.redirected(),
        };
    }
}

// ---- the compiler's D1 identity lease (native::compiler_identity) for include/Render/NativeGraphCompilerIdentity.hpp

use crate::native::compiler_identity::{
    CompilerBinding, CompilerIdentityLease, Failure as CompilerFailure,
};

/// CompilerIdentityLease's state for the C++ face's accessors.
#[repr(C)]
pub struct CompilerState {
    pub binding: CompilerBinding,
    pub group: u64,
    pub producer_context: u64,
    pub renderer_argument: u64,
    pub compiler_job: u64,
    pub compiler_parent_handle: u64,
    pub failure: u32,
    pub captured: bool,
    pub matched: bool,
    pub dispatch_returned: bool,
    pub compiler_returned: bool,
    pub root_returned: bool,
    pub failed: bool,
}
const _: () = assert!(
    core::mem::size_of::<CompilerBinding>() == 72 && core::mem::size_of::<CompilerState>() == 128
);

/// A lease; free it with cvr_cl_free (which releases the pinned group).
#[no_mangle]
pub extern "C-unwind" fn cvr_cl_new() -> *mut CompilerIdentityLease {
    Box::into_raw(Box::default())
}

/// # Safety
/// `lease` comes from cvr_cl_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_free(lease: *mut CompilerIdentityLease) {
    if !lease.is_null() {
        drop(unsafe { Box::from_raw(lease) });
    }
}

/// CompilerIdentityLease::CaptureBeforeDispatch.
///
/// # Safety
/// `lease` is live; `binding` and `api` are valid; the dispatch's group is a live engine job group; `read` follows
/// graph_snapshot::Read.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_cl_capture(
    lease: *mut CompilerIdentityLease,
    binding: *const CompilerBinding,
    producer_job: u64,
    renderer_argument_address: u64,
    caller: u64,
    image_base: u64,
    read: Option<Read>,
    api: *const JobLeaseApi,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).capture_before_dispatch(
            &*binding,
            producer_job,
            renderer_argument_address,
            caller,
            image_base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            *api,
        )
    }
}

/// CompilerIdentityLease::MatchesCompilerParent.
///
/// # Safety
/// `lease` is live; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_matches_parent(
    lease: *const CompilerIdentityLease,
    compiler_job: u64,
    read: Option<Read>,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).matches_compiler_parent(
            compiler_job,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        )
    }
}

/// CompilerIdentityLease::MatchBeforeCompiler.
///
/// # Safety
/// `lease` is live; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_match(
    lease: *mut CompilerIdentityLease,
    pool: u64,
    mode: u8,
    compiler_job: u64,
    caller: u64,
    read: Option<Read>,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*lease).match_before_compiler(
            pool,
            mode,
            compiler_job,
            caller,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        )
    }
}

/// MarkOriginalDispatchReturned (`step` 0), MarkOriginalCompilerReturned (1), MarkOriginalRootReturned (2),
/// Complete (3), or the refusal of a throwing reader (4, Read).
///
/// # Safety
/// `lease` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_step(lease: *mut CompilerIdentityLease, step: u32) -> bool {
    unsafe {
        let lease = &mut *lease;
        match step {
            0 => lease.mark_original_dispatch_returned(),
            1 => lease.mark_original_compiler_returned(),
            2 => lease.mark_original_root_returned(),
            3 => lease.complete(),
            _ => lease.reject(CompilerFailure::Read),
        }
    }
}

/// CompilerIdentityLease::MatchesBinding.
///
/// # Safety
/// `lease` is live; `expected` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_matches_binding(
    lease: *const CompilerIdentityLease,
    expected: *const CompilerBinding,
) -> bool {
    unsafe { (*lease).matches_binding(&*expected) }
}

/// The lease's state.
///
/// # Safety
/// `lease` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cl_state(
    lease: *const CompilerIdentityLease,
    out: *mut CompilerState,
) {
    unsafe {
        let l = &*lease;
        *out = CompilerState {
            binding: *l.binding(),
            group: l.group(),
            producer_context: l.producer_context_address(),
            renderer_argument: l.renderer_argument_address(),
            compiler_job: l.compiler_job_address(),
            compiler_parent_handle: l.compiler_parent_handle(),
            failure: l.rejection() as u32,
            captured: l.captured(),
            matched: l.matched(),
            dispatch_returned: l.original_dispatch_returned(),
            compiler_returned: l.original_compiler_returned(),
            root_returned: l.original_root_returned(),
            failed: l.failed(),
        };
    }
}
