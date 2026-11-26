//! A deferred receipt of a render graph's job groups (include/Render/NativeGraphJobReceipt.hpp).
//!
//! One receipt in the caller's bounded pending collection (at most `MAX_PENDING_RECEIPTS`). Captured while the native
//! queue callback still owns its payload and node-group array: the plan re-verified, the frame owner (references at +8)
//! and every node's groups (references at +1C, separate APIs) held. Afterwards everything works from the immutable deep
//! copy of the source graph and the held references only -- no later operation reads the raw queue-payload owner
//! address or the temporary group-array header. Completion is independent of selection: a completed pending receipt
//! stays available for the later declaration association until the root retires it. All native reference callbacks
//! must not unwind; the caller serializes access.
#![allow(unsafe_op_in_unsafe_fn)]
use super::graph_jobs::{CapturePlan, LeaseSet, Read, MAX_NODES, QUEUE_RETURN};
use super::graph_snapshot::Snapshot;
use super::job_lease::JobLeaseApi;
use core::ffi::c_void;
use std::sync::Arc;

pub const MAX_PENDING_RECEIPTS: usize = 4;

/// The engine's frame owner reference calls.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameOwnerApi {
    pub retain: Option<unsafe extern "C" fn(*mut c_void)>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReceiptFailure {
    #[default]
    None,
    Arguments,
    AlreadyCaptured,
    MissingApi,
    PlanChanged,
    JobCapture,
    Allocation,
    OwnerReference,
}

/// A frame owner reference released on drop unless handed over.
struct OwnerLease {
    release: unsafe extern "C" fn(*mut c_void),
    value: *mut c_void,
}
impl Drop for OwnerLease {
    fn drop(&mut self) {
        if !self.value.is_null() {
            // SAFETY: the owner this lease retained.
            unsafe { (self.release)(self.value) };
        }
    }
}

pub struct DeferredReceipt {
    jobs_api: JobLeaseApi,
    owner_api: FrameOwnerApi,
    jobs: Option<LeaseSet>,
    source: Option<Arc<Snapshot>>,
    epoch: u64,
    generation: u64,
    image_base: u64,
    graph: u64,
    frame_owner: u64,
    captured: bool,
    selected: bool,
    failure: ReceiptFailure,
}
impl Drop for DeferredReceipt {
    fn drop(&mut self) {
        // The groups first: the frame owner outlives its jobs.
        self.jobs = None;
        if let (true, Some(release)) = (self.frame_owner != 0, self.owner_api.release) {
            // SAFETY: the owner this receipt retained.
            unsafe { release(self.frame_owner as usize as *mut c_void) };
        }
    }
}
impl DeferredReceipt {
    pub fn new(jobs: JobLeaseApi, owner: FrameOwnerApi) -> Self {
        DeferredReceipt {
            jobs_api: jobs,
            owner_api: owner,
            jobs: None,
            source: None,
            epoch: 0,
            generation: 0,
            image_base: 0,
            graph: 0,
            frame_owner: 0,
            captured: false,
            selected: false,
            failure: ReceiptFailure::None,
        }
    }
    /// The plan's frame owner and groups held while the queue callback still owns them.
    ///
    /// # Safety
    /// The plan's frame owner and groups are live engine objects; the callbacks do not unwind.
    pub unsafe fn capture(
        &mut self,
        plan: &CapturePlan,
        epoch: u64,
        generation: u64,
        image_base: u64,
        read: Option<Read<'_>>,
    ) -> bool {
        if self.captured {
            self.failure = ReceiptFailure::AlreadyCaptured;
            return false;
        }
        self.failure = ReceiptFailure::Arguments;
        let Some(read) = read else { return false };
        let Some(source) = plan.source() else {
            return false;
        };
        if epoch == 0
            || generation == 0
            || image_base == 0
            || image_base != plan.image_base()
            || image_base > u64::MAX - QUEUE_RETURN
            || !plan.ready()
            || plan.frame_owner() == 0
            || plan.graph_address() == 0
            || plan.nodes().is_empty()
            || plan.nodes().len() > MAX_NODES as usize
        {
            return false;
        }
        self.failure = ReceiptFailure::MissingApi;
        let (Some(retain_owner), Some(release_owner)) =
            (self.owner_api.retain, self.owner_api.release)
        else {
            return false;
        };
        if self.jobs_api.retain.is_none()
            || self.jobs_api.release.is_none()
            || self.jobs_api.complete.is_none()
        {
            return false;
        }
        self.failure = ReceiptFailure::PlanChanged;
        if !plan.matches(read) {
            return false;
        }
        self.failure = ReceiptFailure::OwnerReference;
        let mut references = [0u8; 4];
        if plan.frame_owner() > u64::MAX - 12
            || !read(plan.frame_owner() + 8, &mut references)
            || i32::from_le_bytes(references) <= 0
        {
            return false;
        }
        let mut owner = OwnerLease {
            release: release_owner,
            value: plan.frame_owner() as usize as *mut c_void,
        };
        retain_owner(owner.value);
        // Declared after the owner: on a failure the groups are released before it.
        let mut jobs = LeaseSet::new(self.jobs_api);
        self.failure = ReceiptFailure::JobCapture;
        if !jobs.capture(plan, read) {
            return false;
        }
        self.source = Some(source);
        self.epoch = epoch;
        self.generation = generation;
        self.image_base = image_base;
        self.graph = plan.graph_address();
        self.frame_owner = plan.frame_owner();
        self.jobs = Some(jobs);
        owner.value = core::ptr::null_mut();
        self.captured = true;
        self.failure = ReceiptFailure::None;
        true
    }
    pub fn matches(
        &self,
        epoch: u64,
        generation: u64,
        image_base: u64,
        graph_address: u64,
        declaration_frame_owner: u64,
        source: &Snapshot,
    ) -> bool {
        let Some(held) = self.source.as_ref() else {
            return false;
        };
        self.captured
            && epoch == self.epoch
            && generation == self.generation
            && image_base == self.image_base
            && graph_address == self.graph
            && declaration_frame_owner == self.frame_owner
            && source.failure == 0
            && source.nodes == held.nodes
            && source.edges == held.edges
    }
    pub fn select(
        &mut self,
        epoch: u64,
        generation: u64,
        image_base: u64,
        graph_address: u64,
        declaration_frame_owner: u64,
        source: &Snapshot,
    ) -> bool {
        if self.selected
            || !self.matches(
                epoch,
                generation,
                image_base,
                graph_address,
                declaration_frame_owner,
                source,
            )
        {
            return false;
        }
        self.selected = true;
        true
    }
    pub fn complete(&self) -> bool {
        self.captured && self.jobs.as_ref().is_some_and(LeaseSet::complete)
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn selected(&self) -> bool {
        self.selected
    }
    pub fn rejection(&self) -> ReceiptFailure {
        self.failure
    }
    pub fn node_count(&self) -> usize {
        self.jobs.as_ref().map_or(0, LeaseSet::nodes)
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn image_base(&self) -> u64 {
        self.image_base
    }
    pub fn graph_address(&self) -> u64 {
        self.graph
    }
    pub fn frame_owner(&self) -> u64 {
        self.frame_owner
    }
    pub fn source(&self) -> Option<Arc<Snapshot>> {
        self.source.clone()
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_job_receipt_test.cpp's cases.
    use super::super::graph_jobs::{NativeArray, NodeGroups};
    use super::super::graph_snapshot::{Edge, Node};
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const PAYLOAD: u64 = 0x10000;
    const FRAME: u64 = 0x18000;
    const GRAPH: u64 = 0x20000;
    const NODES: u64 = 0x30000;
    const ORDER: u64 = 0x40000;
    const HEADER: u64 = 0x50000;
    const GROUPS: u64 = 0x60000;
    const EPOCH: u64 = 13;
    const GENERATION: u64 = 47;

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        group_refs: BTreeMap<u64, i32>,
        group_pending: BTreeMap<u64, i32>,
        frame_refs: i32,
        reads: u32,
        owner_retains: u32,
        owner_releases: u32,
        job_retains: u32,
        job_releases: u32,
        change_after_job_retain: u32,
        reads_forbidden: bool,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put_in(w: &mut World, address: u64, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            w.memory.insert(address + i as u64, *v);
        }
    }
    fn put(address: u64, b: &[u8]) {
        with(|w| put_in(w, address, b));
    }
    fn array_bytes(a: NativeArray) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..8].copy_from_slice(&a.data.to_le_bytes());
        b[8..12].copy_from_slice(&a.capacity.to_le_bytes());
        b[12..16].copy_from_slice(&a.count.to_le_bytes());
        b
    }
    fn groups_bytes(g: NodeGroups) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[0..8].copy_from_slice(&g.begin.to_le_bytes());
        b[8..16].copy_from_slice(&g.end.to_le_bytes());
        b[16..20].copy_from_slice(&g.incoming.to_le_bytes());
        b[20..24].copy_from_slice(&g.outgoing.to_le_bytes());
        b
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            assert!(!w.reads_forbidden, "read an expired payload");
            w.reads += 1;
            if address > u64::MAX - out.len() as u64 {
                return false;
            }
            for (i, o) in out.iter_mut().enumerate() {
                match w.memory.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            true
        })
    }
    unsafe extern "C" fn retain_owner(value: *mut c_void) {
        with(|w| {
            assert_eq!(value as u64, FRAME);
            w.frame_refs += 1;
            w.owner_retains += 1;
            let r = w.frame_refs;
            put_in(w, FRAME + 8, &r.to_le_bytes());
        });
    }
    unsafe extern "C" fn release_owner(value: *mut c_void) {
        with(|w| {
            assert!(value as u64 == FRAME && w.frame_refs > 1 && w.job_releases == w.job_retains);
            w.frame_refs -= 1;
            w.owner_releases += 1;
            let r = w.frame_refs;
            put_in(w, FRAME + 8, &r.to_le_bytes());
        });
    }
    unsafe extern "C" fn retain_job(value: *mut c_void) {
        let group = value as u64;
        with(|w| {
            let refs = w.group_refs.get_mut(&group).expect("a known group");
            *refs += 1;
            let r = *refs;
            w.job_retains += 1;
            put_in(w, group + 0x1C, &r.to_le_bytes());
            if w.job_retains == w.change_after_job_retain {
                let header = array_bytes(NativeArray {
                    data: GROUPS,
                    capacity: 2,
                    count: 1,
                });
                put_in(w, HEADER, &header);
            }
        });
    }
    unsafe extern "C" fn release_job(value: *mut *mut c_void) {
        assert!(!value.is_null() && !(*value).is_null());
        let group = *value as u64;
        with(|w| {
            let refs = w.group_refs.get_mut(&group).expect("a known group");
            assert!(*refs > 1);
            *refs -= 1;
            let r = *refs;
            w.job_releases += 1;
            put_in(w, group + 0x1C, &r.to_le_bytes());
        });
        *value = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete_job(value: *mut c_void) -> bool {
        with(|w| w.group_pending[&(value as u64)] == 0)
    }
    const JOBS: JobLeaseApi = JobLeaseApi {
        retain: Some(retain_job),
        release: Some(release_job),
        complete: Some(complete_job),
    };
    const OWNERS: FrameOwnerApi = FrameOwnerApi {
        retain: Some(retain_owner),
        release: Some(release_owner),
    };

    fn fresh() -> Snapshot {
        with(|w| {
            *w = World {
                frame_refs: 1,
                ..World::default()
            }
        });
        put(PAYLOAD, &FRAME.to_le_bytes());
        put(FRAME + 8, &1i32.to_le_bytes());
        let mut header = [0u8; 0x70];
        header[0x10..0x14].copy_from_slice(&2u32.to_le_bytes());
        header[0x18..0x20].copy_from_slice(&NODES.to_le_bytes());
        header[0x28..0x30].copy_from_slice(&ORDER.to_le_bytes());
        put(GRAPH, &header);
        put(
            HEADER,
            &array_bytes(NativeArray {
                data: GROUPS,
                capacity: 2,
                count: 2,
            }),
        );
        let mut source = Snapshot::default();
        for i in 0..2u32 {
            let mut node = Node {
                handle: 17 + i * 4,
                order: i,
                bytes: [0; 48],
                vtable: 0,
                work: 0,
                definition: [0; 32],
            };
            node.bytes[0..4].copy_from_slice(&i.to_le_bytes());
            node.bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
            node.bytes[24..28].copy_from_slice(&i.to_le_bytes());
            put(ORDER + 4 * i as u64, &node.handle.to_le_bytes());
            put(NODES + node.handle as u64 * 48, &node.bytes);
            source.nodes.push(node);
            let begin = 0x80000 + i as u64 * 0x100;
            let end = if i != 0 { begin } else { begin + 0x40 };
            put(
                GROUPS + 24 * i as u64,
                &groups_bytes(NodeGroups {
                    begin,
                    end,
                    incoming: i,
                    outgoing: 1 - i,
                }),
            );
            with(|w| {
                w.group_refs.insert(begin, 1);
                w.group_refs.insert(end, 1);
                w.group_pending.insert(begin, 0);
                w.group_pending.insert(end, if i != 0 { 0 } else { 1 });
            });
            put(begin + 0x1C, &1i32.to_le_bytes());
            put(end + 0x1C, &1i32.to_le_bytes());
        }
        source
    }
    fn plan(source: &Snapshot) -> CapturePlan {
        let mut plan = CapturePlan::default();
        assert!(plan.capture(
            PAYLOAD,
            GRAPH,
            HEADER,
            BASE + QUEUE_RETURN,
            BASE,
            source,
            Some(&mut read)
        ));
        plan
    }
    fn capture(
        receipt: &mut DeferredReceipt,
        plan: &CapturePlan,
        epoch: u64,
        generation: u64,
        base: u64,
    ) -> bool {
        unsafe { receipt.capture(plan, epoch, generation, base, Some(&mut read)) }
    }
    fn matched(receipt: &DeferredReceipt, source: &Snapshot) -> bool {
        receipt.matches(EPOCH, GENERATION, BASE, GRAPH, FRAME, source)
    }
    fn balanced() {
        with(|w| {
            assert!(
                w.frame_refs == 1
                    && w.owner_retains == w.owner_releases
                    && w.job_retains == w.job_releases
            );
            assert!(w.group_refs.values().all(|&r| r == 1));
        });
    }

    #[test]
    fn completion_before_selection_and_expired_payload() {
        assert_eq!(MAX_PENDING_RECEIPTS, 4);
        let source = fresh();
        let mut p = plan(&source);
        let held;
        {
            let mut receipt = DeferredReceipt::new(JOBS, OWNERS);
            assert!(
                !receipt.captured()
                    && !receipt.selected()
                    && !receipt.complete()
                    && !matched(&receipt, &source)
            );
            assert!(
                capture(&mut receipt, &p, EPOCH, GENERATION, BASE)
                    && receipt.captured()
                    && !receipt.selected()
            );
            assert_eq!(receipt.node_count(), 2);
            assert!(
                with(|w| w.frame_refs == 2 && w.owner_retains == 1 && w.job_retains == 3)
                    && !receipt.complete()
            );
            assert!(
                receipt.epoch() == EPOCH
                    && receipt.generation() == GENERATION
                    && receipt.image_base() == BASE
            );
            assert!(receipt.graph_address() == GRAPH && receipt.frame_owner() == FRAME);
            held = receipt.source();
            assert!(held.as_ref().is_some_and(|h| h.nodes == source.nodes));
            // The queue callback's payload and group-array header have expired: late matching and polling use only
            // the immutable copied metadata and the held references.
            let read_count = with(|w| {
                w.memory.clear();
                w.reads_forbidden = true;
                w.reads
            });
            p = CapturePlan::default();
            let _ = &p;
            with(|w| w.group_pending.insert(0x80040, 0));
            assert!(receipt.complete() && !receipt.selected() && matched(&receipt, &source));
            assert!(!receipt.matches(EPOCH + 1, GENERATION, BASE, GRAPH, FRAME, &source));
            assert!(!receipt.matches(EPOCH, GENERATION + 1, BASE, GRAPH, FRAME, &source));
            assert!(!receipt.matches(EPOCH, GENERATION, BASE + 0x10000, GRAPH, FRAME, &source));
            assert!(!receipt.matches(EPOCH, GENERATION, BASE, GRAPH + 0x10000, FRAME, &source));
            assert!(!receipt.matches(EPOCH, GENERATION, BASE, GRAPH, FRAME + 0x10000, &source));
            let mut changed = source.clone();
            changed.nodes[0].bytes[12] ^= 1;
            assert!(!matched(&receipt, &changed));
            let mut changed = source.clone();
            changed.nodes[0].definition[8] ^= 1;
            assert!(!matched(&receipt, &changed));
            let mut changed = source.clone();
            changed.failure = 1;
            assert!(!matched(&receipt, &changed));
            let mut changed = source.clone();
            changed.edges.push(Edge {
                handle: 0,
                order: 0,
                bytes: [0; 24],
            });
            assert!(!matched(&receipt, &changed));
            // The same numeric source/owner addresses from a later generation are no identity match, even with
            // identical graph bytes.
            assert!(!receipt.select(EPOCH, GENERATION + 1, BASE, GRAPH, FRAME, &source));
            assert!(receipt.select(EPOCH, GENERATION, BASE, GRAPH, FRAME, &source));
            assert!(receipt.selected() && receipt.complete());
            assert!(
                !receipt.select(EPOCH, GENERATION, BASE, GRAPH, FRAME, &source)
                    && matched(&receipt, &source)
            );
            assert_eq!(with(|w| w.reads), read_count);
        }
        balanced();
        assert!(held.is_some_and(|h| h.nodes == source.nodes));
    }
    #[test]
    fn failures() {
        let source = fresh();
        let p = plan(&source);
        with(|w| w.change_after_job_retain = 1);
        {
            let mut receipt = DeferredReceipt::new(JOBS, OWNERS);
            assert!(!capture(&mut receipt, &p, EPOCH, GENERATION, BASE));
            assert_eq!(receipt.rejection(), ReceiptFailure::JobCapture);
            assert!(
                !receipt.captured()
                    && !receipt.selected()
                    && !receipt.complete()
                    && receipt.source().is_none()
            );
        }
        balanced();
        assert!(with(|w| w.owner_retains == 1 && w.job_retains == 3));

        let source = fresh();
        let p = plan(&source);
        put(PAYLOAD, &(FRAME + 0x10000).to_le_bytes());
        {
            let mut receipt = DeferredReceipt::new(JOBS, OWNERS);
            assert!(!capture(&mut receipt, &p, EPOCH, GENERATION, BASE));
            assert_eq!(receipt.rejection(), ReceiptFailure::PlanChanged);
        }
        balanced();
        assert!(with(|w| w.owner_retains == 0 && w.job_retains == 0));

        let source = fresh();
        let p = plan(&source);
        put(FRAME + 8, &0i32.to_le_bytes());
        {
            let mut receipt = DeferredReceipt::new(JOBS, OWNERS);
            assert!(!capture(&mut receipt, &p, EPOCH, GENERATION, BASE));
            assert_eq!(receipt.rejection(), ReceiptFailure::OwnerReference);
        }
        balanced();
        assert!(with(|w| w.owner_retains == 0 && w.job_retains == 0));

        let source = fresh();
        let p = plan(&source);
        {
            let mut receipt = DeferredReceipt::new(JOBS, OWNERS);
            assert!(!capture(&mut receipt, &p, 0, GENERATION, BASE));
            assert_eq!(receipt.rejection(), ReceiptFailure::Arguments);
            assert!(!capture(&mut receipt, &p, EPOCH, 0, BASE));
            assert!(!capture(&mut receipt, &p, EPOCH, GENERATION, 0));
            assert!(!capture(
                &mut receipt,
                &p,
                EPOCH,
                GENERATION,
                BASE + 0x10000
            ));
            assert!(!unsafe { receipt.capture(&p, EPOCH, GENERATION, BASE, None) });
            assert!(capture(&mut receipt, &p, EPOCH, GENERATION, BASE));
            assert!(!capture(&mut receipt, &p, EPOCH, GENERATION, BASE));
            assert_eq!(receipt.rejection(), ReceiptFailure::AlreadyCaptured);
        }
        balanced();

        let source = fresh();
        let p = plan(&source);
        {
            let mut owner_missing = DeferredReceipt::new(
                JOBS,
                FrameOwnerApi {
                    retain: None,
                    release: Some(release_owner),
                },
            );
            assert!(!capture(&mut owner_missing, &p, EPOCH, GENERATION, BASE));
            assert_eq!(owner_missing.rejection(), ReceiptFailure::MissingApi);
        }
        {
            let mut job_missing = DeferredReceipt::new(
                JobLeaseApi {
                    release: None,
                    ..JOBS
                },
                OWNERS,
            );
            assert!(!capture(&mut job_missing, &p, EPOCH, GENERATION, BASE));
            assert_eq!(job_missing.rejection(), ReceiptFailure::MissingApi);
        }
        balanced();
        assert!(with(|w| w.owner_retains == 0 && w.job_retains == 0));
    }
}
