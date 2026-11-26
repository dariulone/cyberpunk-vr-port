//! The engine job groups a compiled render graph's nodes run in (include/Render/NativeGraphJobs.hpp).
//!
//! `CapturePlan` is taken only after the original 0x32D60C returns, while its caller (the queue, return 0x3E90BA) still
//! owns the 24-byte group array: the frame owner, the array header, the exact source graph (re-captured and compared to
//! the caller's), and per node its begin/end groups with a positive group reference (the pending count may legitimately
//! change while jobs run). Everything is read twice and compared. The receipt identifies native groups; it is not a
//! lifetime lease by itself (the worker/context refcount +8 is NOT the job-group refcount).
//!
//! `LeaseSet` holds every node's group pair (NativeJobLease), validated against the plan before and after retaining,
//! and answers completion bound to the held ordinal/handle mapping -- never to a later raw graph or group-array read.
//! Completion, not execution permission.
#![allow(unsafe_op_in_unsafe_fn)]
use super::graph_snapshot::{self, field_u32};
use super::job_lease::{JobLeaseApi, NativeJobLease};
use core::ffi::c_void;
use std::collections::BTreeSet;
use std::sync::Arc;

pub const MAX_NODES: u32 = 255;
pub const QUEUE_RETURN: u64 = 0x3E90BA;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeArray {
    pub data: u64,
    pub capacity: u32,
    pub count: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeGroups {
    pub begin: u64,
    pub end: u64,
    pub incoming: u32,
    pub outgoing: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub node: u32,
    pub ordinal: u32,
    pub groups: NodeGroups,
}
const _: () = assert!(
    core::mem::size_of::<NativeArray>() == 16
        && core::mem::size_of::<NodeGroups>() == 24
        && core::mem::size_of::<Mapping>() == 32
);
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Caller,
    Source,
    Owner,
    Header,
    Count,
    Groups,
    Changed,
    Allocation,
}

fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().expect("eight"))
}
fn pointer(value: u64) -> bool {
    value != 0 && value & 7 == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Snapshot {
    owner: u64,
    array: NativeArray,
    groups: Vec<NodeGroups>,
}

#[derive(Clone, Debug, Default)]
pub struct CapturePlan {
    source: Option<Arc<graph_snapshot::Snapshot>>,
    array: NativeArray,
    mapping: Vec<Mapping>,
    owner_address: u64,
    owner: u64,
    graph: u64,
    groups_address: u64,
    base: u64,
    ready: bool,
    failure: Failure,
    diagnostic: [u64; 8],
}
impl CapturePlan {
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        &mut self,
        render_owner: u64,
        graph: u64,
        groups: u64,
        caller: u64,
        image_base: u64,
        source: &graph_snapshot::Snapshot,
        read: Option<Read<'_>>,
    ) -> bool {
        *self = CapturePlan {
            failure: Failure::Arguments,
            ..CapturePlan::default()
        };
        let Some(read) = read else { return false };
        if !pointer(render_owner)
            || !pointer(graph)
            || !pointer(groups)
            || render_owner > u64::MAX - 8
            || graph > u64::MAX - 0x70
            || groups > u64::MAX - 16
            || image_base == 0
            || image_base > u64::MAX - QUEUE_RETURN
        {
            return false;
        }
        self.failure = Failure::Caller;
        if caller != image_base + QUEUE_RETURN {
            return false;
        }
        self.failure = Failure::Source;
        self.diagnostic[0] = 1;
        self.diagnostic[2] = source.nodes.len() as u64;
        if source.failure != 0
            || source.nodes.is_empty()
            || source.nodes.len() > MAX_NODES as usize
            || source.edges.len() > 65536
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        for (i, node) in source.nodes.iter().enumerate() {
            if node.order as usize != i
                || field_u32(&node.bytes, 0) as usize != i
                || !ids.insert(node.handle & 0xDFFF_FFFF)
            {
                self.diagnostic[1] = i as u64;
                self.diagnostic[3] = field_u32(&node.bytes, 0) as u64;
                return false;
            }
        }
        let Some(first) = self.read_once(read, render_owner, graph, groups, source) else {
            return false;
        };
        let Some(second) = self.read_once(read, render_owner, graph, groups, source) else {
            return false;
        };
        self.failure = Failure::Changed;
        if first != second {
            return false;
        }
        self.owner_address = render_owner;
        self.owner = first.owner;
        self.graph = graph;
        self.groups_address = groups;
        self.array = first.array;
        self.base = image_base;
        self.mapping = source
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| Mapping {
                node: n.handle & 0xDFFF_FFFF,
                ordinal: i as u32,
                groups: first.groups[i],
            })
            .collect();
        self.source = Some(Arc::new(source.clone()));
        self.ready = true;
        self.failure = Failure::None;
        true
    }
    /// A ready plan captured before (the C++ face keeps a capture's fields and hands them back to revalidate it or to
    /// lease its groups).
    #[allow(clippy::too_many_arguments)]
    pub fn restored(
        owner_address: u64,
        owner: u64,
        graph: u64,
        groups_address: u64,
        base: u64,
        array: NativeArray,
        mapping: Vec<Mapping>,
        source: Arc<graph_snapshot::Snapshot>,
    ) -> CapturePlan {
        CapturePlan {
            source: Some(source),
            array,
            mapping,
            owner_address,
            owner,
            graph,
            groups_address,
            base,
            ready: true,
            failure: Failure::None,
            diagnostic: [0; 8],
        }
    }
    pub fn matches(&self, read: Read<'_>) -> bool {
        let (true, Some(source)) = (self.ready, self.source.as_ref()) else {
            return false;
        };
        let mut now = CapturePlan::default();
        now.capture(
            self.owner_address,
            self.graph,
            self.groups_address,
            self.base + QUEUE_RETURN,
            self.base,
            source,
            Some(read),
        ) && now.owner == self.owner
            && now.array == self.array
            && now.mapping == self.mapping
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
    pub fn owner_address(&self) -> u64 {
        self.owner_address
    }
    pub fn frame_owner(&self) -> u64 {
        self.owner
    }
    pub fn graph_address(&self) -> u64 {
        self.graph
    }
    pub fn image_base(&self) -> u64 {
        self.base
    }
    pub fn diagnostic(&self) -> &[u64; 8] {
        &self.diagnostic
    }
    pub fn groups_address(&self) -> u64 {
        self.groups_address
    }
    pub fn array(&self) -> NativeArray {
        self.array
    }
    pub fn nodes(&self) -> &[Mapping] {
        &self.mapping
    }
    pub fn source(&self) -> Option<Arc<graph_snapshot::Snapshot>> {
        self.source.clone()
    }

    fn read_once(
        &mut self,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        render_owner: u64,
        graph: u64,
        groups: u64,
        source: &graph_snapshot::Snapshot,
    ) -> Option<Snapshot> {
        let mut out = Snapshot::default();
        self.failure = Failure::Owner;
        let mut b8 = [0u8; 8];
        if !read(render_owner, &mut b8) {
            return None;
        }
        out.owner = u64::from_le_bytes(b8);
        if !pointer(out.owner) {
            return None;
        }
        self.failure = Failure::Header;
        let mut h16 = [0u8; 16];
        if !read(groups, &mut h16) {
            return None;
        }
        out.array = NativeArray {
            data: le64(&h16),
            capacity: field_u32(&h16, 8),
            count: field_u32(&h16, 12),
        };
        let h = out.array;
        self.failure = Failure::Count;
        if !pointer(h.data)
            || h.count as usize != source.nodes.len()
            || h.capacity < h.count
            || h.capacity > 8192
            || h.data > u64::MAX - h.capacity as u64 * 24
        {
            return None;
        }
        let mut c4 = [0u8; 4];
        if !read(graph + 0x10, &mut c4) || u32::from_le_bytes(c4) != h.count {
            return None;
        }
        self.failure = Failure::Source;
        let mut current = graph_snapshot::Snapshot::default();
        let captured = current.capture(graph, read);
        self.diagnostic[0] = 2;
        self.diagnostic[3] = current.nodes.len() as u64;
        self.diagnostic[4] = current.failure as u64;
        if !captured || current.nodes != source.nodes || current.edges != source.edges {
            if captured {
                for i in 0..current.nodes.len().min(source.nodes.len()) {
                    if current.nodes[i] != source.nodes[i] {
                        if self.diagnostic[5] == 0 {
                            self.diagnostic[1] = i as u64;
                            self.diagnostic[7] =
                                first_difference(&current.nodes[i], &source.nodes[i]) as u64;
                        }
                        self.diagnostic[5] += 1;
                    }
                }
                for i in 0..current.edges.len().min(source.edges.len()) {
                    self.diagnostic[6] += (current.edges[i] != source.edges[i]) as u64;
                }
            }
            return None;
        }
        self.failure = Failure::Groups;
        let mut raw = vec![0u8; h.count as usize * 24];
        if !read(h.data, &mut raw) {
            return None;
        }
        out.groups = raw
            .chunks_exact(24)
            .map(|c| NodeGroups {
                begin: le64(&c[0..8]),
                end: le64(&c[8..16]),
                incoming: field_u32(c, 16),
                outgoing: field_u32(c, 20),
            })
            .collect();
        for row in &out.groups {
            if !pointer(row.begin)
                || !pointer(row.end)
                || row.begin > u64::MAX - 0x20
                || row.end > u64::MAX - 0x20
                || row.incoming > 65536
                || row.outgoing > 65536
            {
                return None;
            }
            for p in [row.begin, row.end] {
                // Pending may legitimately change while jobs execute: only the positive group reference is read.
                let mut r4 = [0u8; 4];
                if !read(p + 0x1C, &mut r4) || i32::from_le_bytes(r4) <= 0 {
                    return None;
                }
            }
        }
        self.failure = Failure::Changed;
        let (mut owner_after, mut header_after) = ([0u8; 8], [0u8; 16]);
        (read(render_owner, &mut owner_after)
            && u64::from_le_bytes(owner_after) == out.owner
            && read(groups, &mut header_after)
            && header_after == h16)
            .then_some(out)
    }
}
/// The first differing byte of two nodes as the C++ struct lays them out (handle, order, bytes, padding, vtable,
/// work, definition).
fn first_difference(a: &graph_snapshot::Node, b: &graph_snapshot::Node) -> usize {
    fn layout(n: &graph_snapshot::Node) -> [u8; 104] {
        let mut out = [0u8; 104];
        out[0..4].copy_from_slice(&n.handle.to_le_bytes());
        out[4..8].copy_from_slice(&n.order.to_le_bytes());
        out[8..56].copy_from_slice(&n.bytes);
        out[56..64].copy_from_slice(&n.vtable.to_le_bytes());
        out[64..72].copy_from_slice(&n.work.to_le_bytes());
        out[72..104].copy_from_slice(&n.definition);
        out
    }
    let (x, y) = (layout(a), layout(b));
    x.iter()
        .zip(y.iter())
        .position(|(p, q)| p != q)
        .unwrap_or(0)
}

pub struct LeaseSet {
    api: JobLeaseApi,
    leases: Vec<NativeJobLease>,
    sealed: bool,
    mapping: Vec<Mapping>,
}
impl LeaseSet {
    pub fn new(api: JobLeaseApi) -> Self {
        LeaseSet {
            api,
            leases: Vec::new(),
            sealed: false,
            mapping: Vec::new(),
        }
    }
    /// Every node's groups held while the native array still owns them (validated before and after retaining).
    ///
    /// # Safety
    /// The plan's groups are live engine job groups; the callbacks do not unwind.
    pub unsafe fn capture(&mut self, plan: &CapturePlan, read: Read<'_>) -> bool {
        if self.sealed
            || self.api.retain.is_none()
            || self.api.release.is_none()
            || self.api.complete.is_none()
            || !plan.ready()
            || !plan.matches(read)
        {
            return false;
        }
        let mut next: Vec<NativeJobLease> = plan
            .nodes()
            .iter()
            .map(|_| NativeJobLease::new(self.api))
            .collect();
        for (lease, node) in next.iter_mut().zip(plan.nodes()) {
            if !lease.capture(
                node.groups.begin as usize as *mut c_void,
                node.groups.end as usize as *mut c_void,
            ) {
                return false;
            }
        }
        if !plan.matches(read) {
            return false;
        }
        self.mapping = plan.nodes().to_vec();
        self.leases = next;
        self.sealed = true;
        true
    }
    pub fn complete(&self) -> bool {
        self.sealed && !self.leases.is_empty() && self.leases.iter().all(NativeJobLease::complete)
    }
    /// Completion bound to the held ordinal/handle mapping.
    pub fn complete_node(&self, ordinal: u32, handle: u32) -> bool {
        let i = ordinal as usize;
        self.sealed
            && i < self.mapping.len()
            && i < self.leases.len()
            && self.mapping[i].ordinal == ordinal
            && self.mapping[i].node == handle
            && self.leases[i].complete()
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    pub fn nodes(&self) -> usize {
        self.leases.len()
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_jobs_test.cpp's cases.
    use super::super::definition_owner::RetireState;
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const OWNER: u64 = 0x10000;
    const FRAME: u64 = 0x11000;
    const GRAPH: u64 = 0x20000;
    const NODES: u64 = 0x30000;
    const NODE_ORDER: u64 = 0x40000;
    const GROUP_HEADER: u64 = 0x50000;
    const GROUP_DATA: u64 = 0x60000;

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        references: BTreeMap<u64, i32>,
        pending: BTreeMap<u64, i32>,
        reads: u64,
        retains: u32,
        releases: u32,
        change_on_retain: u32,
        mutate_after_read: u64,
        mutate_address: u64,
        mutate_byte: u8,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn put(address: u64, b: &[u8]) {
        W.with(|w| {
            let mut w = w.borrow_mut();
            for (i, v) in b.iter().enumerate() {
                w.memory.insert(address + i as u64, *v);
            }
        });
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
        W.with(|w| {
            let mut w = w.borrow_mut();
            if address > u64::MAX - out.len() as u64 {
                return false;
            }
            for (i, o) in out.iter_mut().enumerate() {
                match w.memory.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            w.reads += 1;
            if w.reads == w.mutate_after_read {
                let (a, b) = (w.mutate_address, w.mutate_byte);
                w.memory.insert(a, b);
            }
            true
        })
    }
    unsafe extern "C" fn retain(value: *mut c_void) {
        let group = value as u64;
        let (refs, change) = W.with(|w| {
            let mut w = w.borrow_mut();
            assert!(w.references.contains_key(&group));
            *w.references.get_mut(&group).expect("group") += 1;
            w.retains += 1;
            (w.references[&group], w.retains == w.change_on_retain)
        });
        put(group + 0x1C, &refs.to_le_bytes());
        if change {
            put(
                GROUP_HEADER,
                &array_bytes(NativeArray {
                    data: GROUP_DATA,
                    capacity: 8,
                    count: 1,
                }),
            );
        }
    }
    unsafe extern "C" fn release(value: *mut *mut c_void) {
        let group = *value as u64;
        let refs = W.with(|w| {
            let mut w = w.borrow_mut();
            assert!(w.references.get(&group).is_some_and(|r| *r > 1));
            *w.references.get_mut(&group).expect("group") -= 1;
            w.releases += 1;
            w.references[&group]
        });
        put(group + 0x1C, &refs.to_le_bytes());
        *value = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete(value: *mut c_void) -> bool {
        W.with(|w| w.borrow().pending[&(value as u64)] == 0)
    }
    const API: JobLeaseApi = JobLeaseApi {
        retain: Some(retain),
        release: Some(release),
        complete: Some(complete),
    };

    fn fresh(count: u32) -> graph_snapshot::Snapshot {
        W.with(|w| *w.borrow_mut() = World::default());
        put(OWNER, &FRAME.to_le_bytes());
        let mut header = [0u8; 0x70];
        header[0x10..0x14].copy_from_slice(&count.to_le_bytes());
        header[0x18..0x20].copy_from_slice(&NODES.to_le_bytes());
        header[0x28..0x30].copy_from_slice(&NODE_ORDER.to_le_bytes());
        put(GRAPH, &header);
        put(
            GROUP_HEADER,
            &array_bytes(NativeArray {
                data: GROUP_DATA,
                capacity: count + 2,
                count,
            }),
        );
        let mut source = graph_snapshot::Snapshot::default();
        for i in 0..count {
            let mut node = graph_snapshot::Node {
                handle: 7 + i * 3,
                order: i,
                bytes: [0; 48],
                vtable: 0,
                work: 0,
                definition: [0; 32],
            };
            node.bytes[0..4].copy_from_slice(&i.to_le_bytes());
            node.bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
            node.bytes[24..28].copy_from_slice(&(i % 2).to_le_bytes());
            put(NODE_ORDER + i as u64 * 4, &node.handle.to_le_bytes());
            put(NODES + node.handle as u64 * 48, &node.bytes);
            source.nodes.push(node);
            let begin = 0x80000 + i as u64 * 0x100;
            let end = if i == 1 { begin } else { begin + 0x40 };
            put(
                GROUP_DATA + i as u64 * 24,
                &groups_bytes(NodeGroups {
                    begin,
                    end,
                    incoming: i,
                    outgoing: count - 1 - i,
                }),
            );
            W.with(|w| {
                let mut w = w.borrow_mut();
                w.references.insert(begin, 1);
                w.references.insert(end, 1);
                w.pending.insert(begin, 0);
                w.pending.insert(end, if i == 0 { 1 } else { 0 });
            });
            put(begin + 0x1C, &1i32.to_le_bytes());
            put(end + 0x1C, &1i32.to_le_bytes());
        }
        source
    }
    fn capture(plan: &mut CapturePlan, source: &graph_snapshot::Snapshot) -> bool {
        plan.capture(
            OWNER,
            GRAPH,
            GROUP_HEADER,
            BASE + QUEUE_RETURN,
            BASE,
            source,
            Some(&mut read),
        )
    }
    fn reject(source: &graph_snapshot::Snapshot, expected: Failure) {
        let mut plan = CapturePlan::default();
        assert!(
            !capture(&mut plan, source) && !plan.ready(),
            "accepted, expected {expected:?}"
        );
        assert_eq!(plan.rejection(), expected);
        assert!(!plan.matches(&mut read));
    }
    fn counts() -> (u32, u32) {
        W.with(|w| (w.borrow().retains, w.borrow().releases))
    }
    fn balanced() -> bool {
        W.with(|w| w.borrow().references.values().all(|&r| r == 1))
    }
    fn set_pending(group: u64, v: i32) {
        W.with(|w| {
            w.borrow_mut().pending.insert(group, v);
        });
    }

    #[test]
    fn plan_and_leases() {
        let source = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(
            capture(&mut plan, &source)
                && plan.ready()
                && plan.nodes().len() == 3
                && plan.matches(&mut read)
        );
        assert!(
            plan.frame_owner() == FRAME
                && plan.owner_address() == OWNER
                && plan.graph_address() == GRAPH
                && plan.groups_address() == GROUP_HEADER
        );
        assert_eq!(
            plan.array(),
            NativeArray {
                data: GROUP_DATA,
                capacity: 5,
                count: 3
            }
        );
        for i in 0..3 {
            assert!(
                plan.nodes()[i].node == 7 + i as u32 * 3 && plan.nodes()[i].ordinal == i as u32
            );
        }
        assert!(plan.source().is_some_and(|s| s.nodes == source.nodes));
        unsafe {
            let mut leases = LeaseSet::new(API);
            assert!(
                !leases.complete()
                    && !leases.complete_node(0, 7)
                    && leases.capture(&plan, &mut read)
                    && leases.sealed()
                    && leases.nodes() == 3
            );
            assert!(counts().0 == 5 && !leases.complete());
            assert!(
                !leases.complete_node(0, 7)
                    && leases.complete_node(1, 10)
                    && leases.complete_node(2, 13)
            );
            assert!(
                !leases.complete_node(0, 10)
                    && !leases.complete_node(1, 7)
                    && !leases.complete_node(3, 16)
                    && !leases.complete_node(u32::MAX, 7)
            );
            assert!(!leases.complete_node(0, 7 | 0x2000_0000));
            assert!(!leases.capture(&plan, &mut read));
            set_pending(0x80040, 0);
            assert!(leases.complete() && leases.complete_node(0, 7));
            let mut retirement = RetireState::new(1, true);
            assert!(
                retirement.add_consumer(1, 22)
                    && retirement.verify_detach(1, true)
                    && retirement.close_source_uses(1)
            );
            assert!(!retirement.ready_to_retire());
            if leases.complete() {
                assert!(retirement.complete_consumer(1, 22));
            }
            assert!(retirement.ready_to_retire());
        }
        assert!(counts() == (5, 5) && balanced());
    }
    #[test]
    fn leases_outlive_the_plan_and_the_memory() {
        let source = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(capture(&mut plan, &source));
        unsafe {
            let mut selected = LeaseSet::new(API);
            assert!(selected.capture(&plan, &mut read));
            let before = W.with(|w| w.borrow().reads);
            W.with(|w| w.borrow_mut().memory.clear());
            plan = CapturePlan::default();
            let _ = &plan;
            assert!(
                !selected.complete_node(0, 7)
                    && selected.complete_node(1, 10)
                    && selected.complete_node(2, 13)
            );
            assert_eq!(W.with(|w| w.borrow().reads), before);
            set_pending(0x80200, 1);
            assert!(!selected.complete_node(2, 13) && selected.complete_node(1, 10));
            set_pending(0x80200, 0);
            set_pending(0x80040, 0);
            assert!(selected.complete_node(0, 7) && selected.complete());
        }
        assert!(counts() == (5, 5) && balanced());
    }
    #[test]
    fn a_change_while_retaining_is_refused() {
        let source = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(capture(&mut plan, &source));
        W.with(|w| w.borrow_mut().change_on_retain = 2);
        unsafe {
            let mut invalid = LeaseSet::new(API);
            assert!(
                !invalid.capture(&plan, &mut read)
                    && !invalid.sealed()
                    && !invalid.complete()
                    && !invalid.complete_node(0, 7)
            );
        }
        assert!(counts() == (5, 5) && balanced());
        let source = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(capture(&mut plan, &source));
        unsafe {
            let mut missing = LeaseSet::new(JobLeaseApi {
                retain: None,
                release: Some(release),
                complete: Some(complete),
            });
            assert!(!missing.capture(&plan, &mut read) && !missing.complete());
        }
        assert_eq!(counts().0, 0);
    }
    #[test]
    fn rejections() {
        let source = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(
            !plan.capture(
                OWNER,
                GRAPH,
                GROUP_HEADER,
                BASE + QUEUE_RETURN + 1,
                BASE,
                &source,
                Some(&mut read)
            ) && plan.rejection() == Failure::Caller
        );
        assert!(
            !plan.capture(
                OWNER,
                GRAPH,
                GROUP_HEADER,
                BASE + QUEUE_RETURN,
                BASE,
                &source,
                None
            ) && plan.rejection() == Failure::Arguments
        );
        assert!(
            !plan.capture(
                OWNER + 1,
                GRAPH,
                GROUP_HEADER,
                BASE + QUEUE_RETURN,
                BASE,
                &source,
                Some(&mut read)
            ) && plan.rejection() == Failure::Arguments
        );
        assert!(
            !plan.capture(
                OWNER,
                u64::MAX - 7,
                GROUP_HEADER,
                BASE + QUEUE_RETURN,
                BASE,
                &source,
                Some(&mut read)
            ) && plan.rejection() == Failure::Arguments
        );
        let mut s = fresh(3);
        s.failure = 1;
        reject(&s, Failure::Source);
        let mut s = fresh(3);
        s.nodes[1].order = 0;
        reject(&s, Failure::Source);
        let mut s = fresh(3);
        s.nodes[1].handle = s.nodes[0].handle;
        reject(&s, Failure::Source);
        let s = fresh(3);
        put(OWNER, &0u64.to_le_bytes());
        reject(&s, Failure::Owner);
        let s = fresh(3);
        W.with(|w| {
            let mut w = w.borrow_mut();
            for i in 0..16 {
                w.memory.remove(&(GROUP_HEADER + i));
            }
        });
        reject(&s, Failure::Header);
        for a in [
            NativeArray {
                data: GROUP_DATA,
                capacity: 2,
                count: 3,
            },
            NativeArray {
                data: GROUP_DATA,
                capacity: 3,
                count: 2,
            },
            NativeArray {
                data: GROUP_DATA,
                capacity: 8193,
                count: 3,
            },
            NativeArray {
                data: u64::MAX - 7,
                capacity: 3,
                count: 3,
            },
        ] {
            let s = fresh(3);
            put(GROUP_HEADER, &array_bytes(a));
            reject(&s, Failure::Count);
        }
        let s = fresh(3);
        put(GRAPH + 0x10, &2u32.to_le_bytes());
        reject(&s, Failure::Count);
        let s = fresh(3);
        W.with(|w| {
            *w.borrow_mut()
                .memory
                .get_mut(&(NODES + 7 * 48 + 12))
                .expect("node byte") ^= 1;
        });
        reject(&s, Failure::Source);
        for g in [
            NodeGroups {
                begin: 0,
                end: 0x80040,
                incoming: 0,
                outgoing: 2,
            },
            NodeGroups {
                begin: 0x80001,
                end: 0x80040,
                incoming: 0,
                outgoing: 2,
            },
            NodeGroups {
                begin: 0x80000,
                end: 0x80040,
                incoming: 65537,
                outgoing: 0,
            },
        ] {
            let s = fresh(3);
            put(GROUP_DATA, &groups_bytes(g));
            reject(&s, Failure::Groups);
        }
        let s = fresh(3);
        put(0x80000 + 0x1C, &0i32.to_le_bytes());
        reject(&s, Failure::Groups);
        let s = fresh(3);
        let mut plan = CapturePlan::default();
        assert!(capture(&mut plan, &s));
        put(OWNER, &(FRAME + 0x100).to_le_bytes());
        assert!(!plan.matches(&mut read));
        let s = fresh(3);
        assert!(capture(&mut plan, &s));
        put(GROUP_DATA + 8, &0x80100u64.to_le_bytes());
        assert!(!plan.matches(&mut read));
        // A stable pointer field changed during the first whole receipt read.
        let s = fresh(3);
        W.with(|w| {
            let mut w = w.borrow_mut();
            w.mutate_after_read = 1;
            w.mutate_address = OWNER + 1;
            w.mutate_byte = 0x20;
        });
        reject(&s, Failure::Changed);
        let s = fresh(MAX_NODES);
        assert!(capture(&mut plan, &s) && plan.nodes().len() == MAX_NODES as usize);
        let s = fresh(MAX_NODES + 1);
        reject(&s, Failure::Source);
    }
}
