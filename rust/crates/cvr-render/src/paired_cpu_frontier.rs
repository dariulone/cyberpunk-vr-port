//! The paired CPU frontier (`include/Render/PairedCpuFrontier.hpp`): over a paired pass frontier's candidate, the CPU
//! (phase 0) edges that move with it -- the peer's early set (its producer prefix, and its discard pass at the discard
//! boundary) grown by the empty synthetic joins of the peer view feeding it; the late barriers among its incoming edges
//! (after the main solid-ready join or the main discard) propagated to every frontier target; the joins of the main
//! solid-ready to the peer collector, the peer solid-ready to the peer discard (at the discard boundary) and the early
//! tail to the main discard; at the discard boundary the orders between nodes sharing one native object kept (only the
//! main-to-peer discard order reversed); then the proofs -- each phase, their raw union and the lifted CPU-ready /
//! ordered-emission events acyclic, unmoved CPU edges preserved, barriers propagated, both producers before the main
//! discard, nothing of the main view after its discard reaching the peer's early tail -- with a witness path or cycle on
//! a rejection. Source-bound topology only: no native jobs, resource ownership or GPU permission.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `Plan::BuildCandidate` (after its source checks, which stay in C++) and the
//! helpers `detail::Union`, `Lifted`, `Cycle` and `EmptyJoin` forward to `cvr_pcf_*`; the plan's members stay in C++,
//! assembled from what Rust hands back. `tools/single_pass_tests/paired_cpu_frontier_test` runs its cases against them
//! in its Rust variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::native::graph_snapshot::Node;
use crate::paired_frontier::{acyclic, path, Edges, Links, DISCARD};

/// paired_cpu_frontier::Result.
pub const CANDIDATE_NEEDS_OWNERSHIP_AND_RESOURCE_COMPILE: u32 = 1;

/// paired_cpu_frontier::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Source,
    SyntheticOwnership,
    UnprovedPrerequisite,
    Propagation,
    CpuCycle,
    OrderCycle,
    LiftedCycle,
    Invariant,
    Allocation,
    SharedOwner,
}

/// paired_cpu_frontier::Invariants.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invariants {
    pub original_cpu_acyclic: bool,
    pub original_order_acyclic: bool,
    pub original_lifted_acyclic: bool,
    pub candidate_cpu_acyclic: bool,
    pub candidate_order_acyclic: bool,
    pub candidate_lifted_acyclic: bool,
    pub original_raw_union_acyclic: bool,
    pub candidate_raw_union_acyclic: bool,
    pub bounded_synthetic_ancestors: bool,
    pub unmoved_cpu_edges_preserved: bool,
    pub barriers_propagated: bool,
    pub producers_before_main_discard: bool,
    pub no_main_post_discard_to_peer_ready: bool,
    pub shared_owner_order_preserved: bool,
    pub native_ownership_proved: bool,
    pub resource_order_compiled: bool,
    pub native_wait_receipts_proved: bool,
    pub execution_permitted: bool,
}
const _: () = assert!(core::mem::size_of::<Invariants>() == 18);

fn field32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn field64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// detail::Union.
pub fn union(edges: &[Edges; 2]) -> Edges {
    edges[0].union(&edges[1]).copied().collect()
}
/// detail::Lifted: event 2*id is CPU-ready, 2*id+1 ordered emission. Native reverse waits are NOT inferred.
pub fn lifted(nodes: impl IntoIterator<Item = u32>, edges: &[Edges; 2]) -> Edges {
    let mut result: Edges = nodes.into_iter().map(|id| (2 * id, 2 * id + 1)).collect();
    for (phase, links) in edges.iter().enumerate() {
        let phase = phase as u32;
        for &(from, to) in links {
            result.insert((2 * from + phase, 2 * to + phase));
        }
    }
    result
}
/// detail::Cycle: the first cycle a depth-first walk from each node in turn meets (closed by its first node), or none.
pub fn cycle<L: Links + ?Sized>(nodes: impl IntoIterator<Item = u32>, edges: &L) -> Vec<u32> {
    fn visit<L: Links + ?Sized>(
        node: u32,
        edges: &L,
        state: &mut BTreeMap<u32, u8>,
        stack: &mut Vec<u32>,
        result: &mut Vec<u32>,
    ) -> bool {
        state.insert(node, 1);
        stack.push(node);
        for next in edges.successors(node) {
            let seen = *state.entry(next).or_insert(0);
            if seen == 1 {
                let begin = stack
                    .iter()
                    .position(|&id| id == next)
                    .unwrap_or(stack.len());
                *result = stack[begin..].to_vec();
                result.push(next);
                return true;
            }
            if seen == 0 && visit(next, edges, state, stack, result) {
                return true;
            }
        }
        stack.pop();
        state.insert(node, 2);
        false
    }
    let mut state = BTreeMap::new();
    let (mut stack, mut result) = (Vec::new(), Vec::new());
    for node in nodes {
        if *state.entry(node).or_insert(0) == 0
            && visit(node, edges, &mut state, &mut stack, &mut result)
        {
            break;
        }
    }
    result
}
/// detail::EmptyJoin: a synthetic join -- no work, no vtable, kind 4, no flags, no object and an all-zero definition.
pub fn empty_join(node: &Node) -> bool {
    node.work == 0
        && node.vtable == 0
        && field32(&node.bytes, 8) == 4
        && field32(&node.bytes, 12) == 0
        && field64(&node.bytes, 16) == 0
        && node.definition.iter().all(|&byte| byte == 0)
}

/// The paired pass frontier's candidate this plan builds on.
pub struct Source<'a> {
    pub nodes: &'a [Node],
    pub boundary: u32,
    pub early_tail: u32,
    pub original: [Edges; 2],
    pub proposed: [Edges; 2],
    pub main_solid_ready: u32,
    pub main_discard: u32,
    pub peer_collector: u32,
    pub peer_solid_ready: u32,
    pub peer_discard: u32,
    pub peer_prefix: &'a [u32],
}

/// paired_cpu_frontier::Plan's members past its source checks.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub result: u32,
    pub failure: Failure,
    pub proposed: [Edges; 2],
    pub removed: [Edges; 2],
    pub added: [Edges; 2],
    pub early: BTreeSet<u32>,
    pub synthetic: BTreeSet<u32>,
    pub barriers: BTreeSet<u32>,
    pub frontier: BTreeSet<u32>,
    pub incoming: Edges,
    pub shared_owner_orders: Edges,
    pub restored_owner_orders: Edges,
    pub reversed_owner_orders: Edges,
    pub invariants: Invariants,
    pub witness: Vec<u32>,
    pub original_raw_cycle: Vec<u32>,
    pub candidate_raw_cycle: Vec<u32>,
}

impl Plan {
    /// Plan::BuildCandidate past its source checks; on a rejection the members computed so far stay, as in C++.
    pub fn build_candidate(source: &Source) -> Plan {
        let mut plan = Plan {
            failure: Failure::Source,
            proposed: source.proposed.clone(),
            ..Default::default()
        };
        plan.build(source);
        plan
    }

    fn build(&mut self, source: &Source) {
        let original = &source.original;
        let discard_boundary = source.boundary == DISCARD;
        let (early_tail, main_ready, main_discard) = (
            source.early_tail,
            source.main_solid_ready,
            source.main_discard,
        );
        let mut nodes: BTreeMap<u32, &Node> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for node in source.nodes {
            let id = node.handle & 0xDFFF_FFFF;
            ids.insert(id);
            nodes.insert(id, node);
        }
        self.original_raw_cycle = cycle(ids.iter().copied(), &union(original));
        self.invariants.original_raw_union_acyclic = self.original_raw_cycle.is_empty();
        self.invariants.original_cpu_acyclic = acyclic(ids.iter().copied(), &original[0]);
        self.invariants.original_order_acyclic = acyclic(ids.iter().copied(), &original[1]);
        let events: BTreeSet<u32> = ids.iter().flat_map(|&id| [2 * id, 2 * id + 1]).collect();
        self.invariants.original_lifted_acyclic = acyclic(
            events.iter().copied(),
            &lifted(ids.iter().copied(), original),
        );
        if !self.invariants.original_cpu_acyclic
            || !self.invariants.original_order_acyclic
            || !self.invariants.original_lifted_acyclic
        {
            return;
        }
        self.early.extend(source.peer_prefix.iter().copied());
        if discard_boundary {
            self.early.insert(source.peer_discard);
        }
        self.failure = Failure::SyntheticOwnership;
        let mut changed = true;
        while changed {
            changed = false;
            for &(from, to) in &original[0] {
                if !self.early.contains(&to) || self.early.contains(&from) {
                    continue;
                }
                let node = nodes[&from];
                if field32(&node.bytes, 24) != 1 || node.work != 0 {
                    continue;
                }
                if !empty_join(node) {
                    self.witness = vec![from, to];
                    return;
                }
                changed |= self.early.insert(from);
                self.synthetic.insert(from);
            }
        }
        self.invariants.bounded_synthetic_ancestors = true;
        for &(a, b) in &original[0] {
            if self.early.contains(&a) && !self.early.contains(&b) {
                self.frontier.insert(b);
            }
            if self.early.contains(&a) || !self.early.contains(&b) {
                continue;
            }
            self.incoming.insert((a, b));
            if a != main_ready
                && (!path(&original[1], main_ready, a).is_empty()
                    || !path(&original[0], main_discard, a).is_empty())
            {
                self.barriers.insert(a);
            }
        }
        // Other real prerequisites must already precede MAIN's ready point. They are not moved.
        self.failure = Failure::UnprovedPrerequisite;
        for &(from, to) in &self.incoming {
            if !self.barriers.contains(&from)
                && from != main_ready
                && nodes[&from].work != 0
                && path(&original[0], from, main_ready).is_empty()
                && path(&original[1], from, main_ready).is_empty()
            {
                self.witness = vec![from, to];
                return;
            }
        }
        for edge in &self.incoming {
            if self.barriers.contains(&edge.0) {
                self.proposed[0].remove(edge);
            }
        }
        self.failure = Failure::Propagation;
        for &barrier in &self.barriers {
            for &target in &self.frontier {
                if discard_boundary && target == main_discard {
                    continue;
                }
                if barrier == target {
                    self.witness = vec![barrier, target];
                    return;
                }
                self.proposed[0].insert((barrier, target));
            }
        }
        self.proposed[0].insert((main_ready, source.peer_collector));
        // Source CPU dependencies may let discard run before the solid ready job.
        if discard_boundary {
            self.proposed[0].insert((source.peer_solid_ready, source.peer_discard));
        }
        self.proposed[0].insert((early_tail, main_discard));
        if discard_boundary {
            self.failure = Failure::SharedOwner;
            let all: Vec<(u32, &Node)> = nodes.iter().map(|(&id, &node)| (id, node)).collect();
            for (i, &(a, node_a)) in all.iter().enumerate() {
                let object = field64(&node_a.bytes, 16);
                if object == 0 {
                    continue;
                }
                for &(b, node_b) in &all[i + 1..] {
                    if field64(&node_b.bytes, 16) != object {
                        continue;
                    }
                    if node_a.vtable != node_b.vtable || node_a.work != node_b.work {
                        self.witness = vec![a, b];
                        return;
                    }
                    for edge in [(a, b), (b, a)] {
                        if path(&original[0], edge.0, edge.1).is_empty() {
                            continue;
                        }
                        self.shared_owner_orders.insert(edge);
                        if edge == (main_discard, source.peer_discard) {
                            if path(&self.proposed[0], edge.1, edge.0).is_empty() {
                                self.witness = vec![edge.0, edge.1];
                                return;
                            }
                            self.reversed_owner_orders.insert(edge);
                            continue;
                        }
                        if path(&self.proposed[0], edge.0, edge.1).is_empty() {
                            self.proposed[0].insert(edge);
                            self.restored_owner_orders.insert(edge);
                        }
                    }
                }
            }
            // Node completion includes its native children; pointer equality alone
            // is not a certificate that those children are read-only or disjoint.
            self.invariants.shared_owner_order_preserved =
                self.shared_owner_orders.iter().all(|edge| {
                    self.reversed_owner_orders.contains(edge)
                        || !path(&self.proposed[0], edge.0, edge.1).is_empty()
                });
            if !self.invariants.shared_owner_order_preserved {
                return;
            }
        }
        for phase in 0..2 {
            self.removed[phase] = original[phase]
                .difference(&self.proposed[phase])
                .copied()
                .collect();
            self.added[phase] = self.proposed[phase]
                .difference(&original[phase])
                .copied()
                .collect();
        }
        self.failure = Failure::CpuCycle;
        self.witness = cycle(ids.iter().copied(), &self.proposed[0]);
        self.invariants.candidate_cpu_acyclic = self.witness.is_empty();
        if !self.witness.is_empty() {
            return;
        }
        self.failure = Failure::OrderCycle;
        self.witness = cycle(ids.iter().copied(), &self.proposed[1]);
        self.invariants.candidate_order_acyclic = self.witness.is_empty();
        if !self.witness.is_empty() {
            return;
        }
        self.candidate_raw_cycle = cycle(ids.iter().copied(), &union(&self.proposed));
        self.invariants.candidate_raw_union_acyclic = self.candidate_raw_cycle.is_empty();
        self.failure = Failure::LiftedCycle;
        self.witness = cycle(
            events.iter().copied(),
            &lifted(ids.iter().copied(), &self.proposed),
        );
        self.invariants.candidate_lifted_acyclic = self.witness.is_empty();
        if !self.witness.is_empty() {
            return;
        }
        let proposed = &self.proposed[0];
        self.invariants.unmoved_cpu_edges_preserved = original[0].iter().all(|edge| {
            self.early.contains(&edge.0) || self.early.contains(&edge.1) || proposed.contains(edge)
        });
        self.invariants.barriers_propagated = true;
        for &barrier in &self.barriers {
            for &target in &self.frontier {
                if !discard_boundary || target != main_discard {
                    self.invariants.barriers_propagated &= proposed.contains(&(barrier, target));
                }
            }
        }
        self.invariants.producers_before_main_discard = !path(proposed, main_ready, main_discard)
            .is_empty()
            && !path(proposed, source.peer_solid_ready, early_tail).is_empty()
            && !path(proposed, early_tail, main_discard).is_empty();
        self.invariants.no_main_post_discard_to_peer_ready = true;
        for (&id, node) in &nodes {
            if field32(&node.bytes, 24) == 0
                && (!path(&original[1], main_discard, id).is_empty()
                    || !path(&original[0], main_discard, id).is_empty())
            {
                let reach = path(proposed, id, early_tail);
                if !reach.is_empty() {
                    self.invariants.no_main_post_discard_to_peer_ready = false;
                    self.witness = reach;
                    break;
                }
            }
        }
        self.failure = Failure::Invariant;
        let proofs = &self.invariants;
        if !proofs.unmoved_cpu_edges_preserved
            || !proofs.barriers_propagated
            || !proofs.producers_before_main_discard
            || !proofs.no_main_post_discard_to_peer_ready
        {
            return;
        }
        self.failure = Failure::None;
        self.result = CANDIDATE_NEEDS_OWNERSHIP_AND_RESOURCE_COMPILE;
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
/// Lent ids.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawIds {
    pub data: *const u32,
    pub count: usize,
}
/// A lent, flattened (ascending) edge set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawLinks {
    pub data: *const [u32; 2],
    pub count: usize,
}
unsafe fn edges(raw: RawLinks) -> Edges {
    unsafe {
        slice(raw.data, raw.count)
            .iter()
            .map(|link| (link[0], link[1]))
            .collect()
    }
}
unsafe fn ids<'a>(raw: RawIds) -> &'a [u32] {
    unsafe { slice(raw.data, raw.count) }
}

/// The paired pass frontier's candidate, lent.
#[repr(C)]
pub struct RawSource {
    pub nodes: *const Node,
    pub node_count: usize,
    pub boundary: u32,
    pub early_tail: u32,
    /// main solidReady, main discard, peer collector, peer solidReady, peer discard.
    pub roles: [u32; 5],
    pub peer_prefix: RawIds,
    /// original[0..2], proposed[0..2].
    pub links: [RawLinks; 4],
}
/// A built plan, lent until `cvr_pcf_free`.
#[repr(C)]
pub struct RawPlan {
    pub result: u32,
    pub failure: u32,
    /// proposed[0..2], removed[0..2], added[0..2], incoming, sharedOwnerOrders, restoredOwnerOrders, reversedOwnerOrders.
    pub links: [RawLinks; 10],
    /// early, synthetic, barriers, frontier, witness, originalRawCycle, candidateRawCycle.
    pub ids: [RawIds; 7],
    pub invariants: Invariants,
}
const _: () = assert!(
    core::mem::size_of::<RawSource>() == 128
        && core::mem::size_of::<RawPlan>() == 304
        && core::mem::offset_of!(RawPlan, invariants) == 280
);

/// A built plan with its flattened sets.
pub struct Built {
    links: [Vec<[u32; 2]>; 10],
    ids: [Vec<u32>; 7],
}

/// Plan::BuildCandidate past its source checks: the built plan (released by `cvr_pcf_free`), lent through `output`.
///
/// # Safety
/// `source`'s arrays hold their counts; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcf_build(
    source: *const RawSource,
    output: *mut RawPlan,
) -> *mut Built {
    let plan = unsafe {
        let raw = &*source;
        Plan::build_candidate(&Source {
            nodes: slice(raw.nodes, raw.node_count),
            boundary: raw.boundary,
            early_tail: raw.early_tail,
            original: [edges(raw.links[0]), edges(raw.links[1])],
            proposed: [edges(raw.links[2]), edges(raw.links[3])],
            main_solid_ready: raw.roles[0],
            main_discard: raw.roles[1],
            peer_collector: raw.roles[2],
            peer_solid_ready: raw.roles[3],
            peer_discard: raw.roles[4],
            peer_prefix: ids(raw.peer_prefix),
        })
    };
    let flat = |edges: &Edges| edges.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>();
    let list = |ids: &BTreeSet<u32>| ids.iter().copied().collect::<Vec<_>>();
    let built = Box::new(Built {
        links: [
            flat(&plan.proposed[0]),
            flat(&plan.proposed[1]),
            flat(&plan.removed[0]),
            flat(&plan.removed[1]),
            flat(&plan.added[0]),
            flat(&plan.added[1]),
            flat(&plan.incoming),
            flat(&plan.shared_owner_orders),
            flat(&plan.restored_owner_orders),
            flat(&plan.reversed_owner_orders),
        ],
        ids: [
            list(&plan.early),
            list(&plan.synthetic),
            list(&plan.barriers),
            list(&plan.frontier),
            plan.witness.clone(),
            plan.original_raw_cycle.clone(),
            plan.candidate_raw_cycle.clone(),
        ],
    });
    let raw = RawPlan {
        result: plan.result,
        failure: plan.failure as u32,
        links: core::array::from_fn(|i| RawLinks {
            data: built.links[i].as_ptr(),
            count: built.links[i].len(),
        }),
        ids: core::array::from_fn(|i| RawIds {
            data: built.ids[i].as_ptr(),
            count: built.ids[i].len(),
        }),
        invariants: plan.invariants,
    };
    unsafe { *output = raw };
    Box::into_raw(built)
}
/// Releases a built plan.
///
/// # Safety
/// `built` came from `cvr_pcf_build` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcf_free(built: *mut Built) {
    if !built.is_null() {
        drop(unsafe { Box::from_raw(built) });
    }
}
/// detail::Cycle over flattened (ascending, distinct) nodes and edges; writes at most `node_count + edge_count + 1` ids
/// to `output`, returns the count.
///
/// # Safety
/// The arrays hold their counts; `output` has room for the bound.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcf_cycle(
    nodes: *const u32,
    node_count: usize,
    links: *const [u32; 2],
    link_count: usize,
    output: *mut u32,
) -> usize {
    unsafe {
        let result = cycle(
            slice(nodes, node_count).iter().copied(),
            slice(links, link_count),
        );
        core::ptr::copy_nonoverlapping(result.as_ptr(), output, result.len());
        result.len()
    }
}
/// detail::EmptyJoin.
///
/// # Safety
/// `node` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcf_empty_join(node: *const Node) -> bool {
    unsafe { empty_join(&*node) }
}
/// The lifted events of `edges` (detail::Lifted) or their union (detail::Union, `which` 1) into `output`, flattened
/// ascending; returns the count (`output` has room for `node_count + cpu_count + order_count` links).
///
/// # Safety
/// The arrays hold their counts; `output` has room for the bound.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcf_combine(
    which: u32,
    nodes: *const u32,
    node_count: usize,
    cpu: RawLinks,
    order: RawLinks,
    output: *mut [u32; 2],
) -> usize {
    unsafe {
        let sets = [edges(cpu), edges(order)];
        let result = if which == 0 {
            lifted(slice(nodes, node_count).iter().copied(), &sets)
        } else {
            union(&sets)
        };
        for (i, &(a, b)) in result.iter().enumerate() {
            *output.add(i) = [a, b];
        }
        result.len()
    }
}
