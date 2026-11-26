//! The paired pass frontier (`include/Render/PairedPassFrontier.hpp`): an immutable, source-bound topology receipt over
//! one native graph snapshot -- its nodes (at most 255, views 0 and 1, kinds 0/1/2/4, a working node's vtable and
//! definition agreeing) and its CPU (phase 0) and order (phase 1) edges, both acyclic; the per-eye roles (collector,
//! depth prepass, four solid G-buffer passes, discard, post-G-buffer, lighting, final 2D) and the exact verified
//! 21-step native producer prefix from the collector to the solid-ready join; then the candidate order -- the peer's
//! collector right after the main solid-ready join, the peer's early tail (solid-ready, or the whole discard pass)
//! before the main discard, the peer's continuation after the main final 2D -- with its proofs (acyclic, CPU edges
//! unchanged, unrelated order edges preserved, both producers before the main discard, each eye's geometry before its
//! lighting), the CPU and order blocker paths and the peer's CPU closure with its incoming edges. Not
//! NativePreparationSchedule's JobGraph, a resource dependency graph or an executable native graph shadow.
//!
//! Also the graph helpers others use (`detail::Path`, `Acyclic`, `OnlySuccessor`, `OnlyPredecessor`, `Fingerprint`).
//! In the Rust build (`CVR_RUST_VRIK`) they and `Plan::BuildCandidate` forward to `cvr_pf_*`: the plan's members stay in
//! C++, assembled from what Rust hands back; `tools/single_pass_tests/paired_pass_frontier_test` runs its cases against
//! them in its Rust variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::native::graph_snapshot::{Edge, Node};

pub type Link = (u32, u32);
pub type Edges = BTreeSet<Link>;

/// paired_frontier::Result.
pub const REJECTED: u32 = 0;
pub const CANDIDATE_NEEDS_CPU_AND_RESOURCE_COMPILE: u32 = 1;
/// paired_frontier::Boundary.
pub const SOLID: u32 = 0;
pub const DISCARD: u32 = 1;

/// paired_frontier::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Source,
    Nodes,
    Edges,
    CpuCycle,
    OrderCycle,
    Roles,
    Prefix,
    OrderBoundary,
    CandidateCycle,
    Invariant,
    Allocation,
}

/// paired_frontier::Name: FNV-1a of a pass name.
pub const fn name(value: &str) -> u64 {
    let bytes = value.as_bytes();
    let mut hash = 14695981039346656037u64;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(1099511628211);
        i += 1;
    }
    hash
}

/// An edge set walked in its (from, to) order: `std::set<Link>` or its flattened copy.
pub trait Links {
    /// The targets of `source`'s edges, ascending.
    fn successors(&self, source: u32) -> impl Iterator<Item = u32> + '_;
    /// Every edge, ascending.
    fn links(&self) -> impl Iterator<Item = Link> + '_;
}
impl Links for Edges {
    fn successors(&self, source: u32) -> impl Iterator<Item = u32> + '_ {
        self.range((source, 0)..=(source, u32::MAX))
            .map(|link| link.1)
    }
    fn links(&self) -> impl Iterator<Item = Link> + '_ {
        self.iter().copied()
    }
}
impl Links for [[u32; 2]] {
    fn successors(&self, source: u32) -> impl Iterator<Item = u32> + '_ {
        let first = self.partition_point(|link| link[0] < source);
        self[first..]
            .iter()
            .take_while(move |link| link[0] == source)
            .map(|link| link[1])
    }
    fn links(&self) -> impl Iterator<Item = Link> + '_ {
        self.iter().map(|link| (link[0], link[1]))
    }
}

/// detail::Path: the breadth-first path from `from` to `to` (empty when there is none).
pub fn path<L: Links + ?Sized>(edges: &L, from: u32, to: u32) -> Vec<u32> {
    let mut previous = BTreeMap::from([(from, from)]);
    let mut pending = vec![from];
    let mut i = 0;
    while i < pending.len() {
        let current = pending[i];
        if current == to {
            let mut result = vec![to];
            while *result.last().unwrap() != from {
                result.push(previous[result.last().unwrap()]);
            }
            result.reverse();
            return result;
        }
        for next in edges.successors(current) {
            if let std::collections::btree_map::Entry::Vacant(entry) = previous.entry(next) {
                entry.insert(current);
                pending.push(next);
            }
        }
        i += 1;
    }
    Vec::new()
}
/// detail::Acyclic: every edge between the (distinct) nodes and a topological order of all of them.
pub fn acyclic<L: Links + ?Sized>(nodes: impl IntoIterator<Item = u32>, edges: &L) -> bool {
    let mut incoming: BTreeMap<u32, usize> = nodes.into_iter().map(|id| (id, 0)).collect();
    for (from, to) in edges.links() {
        if !incoming.contains_key(&from) || !incoming.contains_key(&to) {
            return false;
        }
        *incoming.get_mut(&to).unwrap() += 1;
    }
    let mut pending: Vec<u32> = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut i = 0;
    while i < pending.len() {
        for next in edges.successors(pending[i]) {
            let count = incoming.get_mut(&next).unwrap();
            *count -= 1;
            if *count == 0 {
                pending.push(next);
            }
        }
        i += 1;
    }
    pending.len() == incoming.len()
}
/// detail::OnlySuccessor (`target` gets the first successor whenever there is one).
pub fn only_successor<L: Links + ?Sized>(edges: &L, source: u32, target: &mut u32) -> bool {
    let mut successors = edges.successors(source);
    match successors.next() {
        None => false,
        Some(first) => {
            *target = first;
            successors.next().is_none()
        }
    }
}
/// detail::OnlyPredecessor (`source` gets the last predecessor whenever there is one).
pub fn only_predecessor<L: Links + ?Sized>(edges: &L, target: u32, source: &mut u32) -> bool {
    let mut count = 0usize;
    for (from, to) in edges.links() {
        if to == target {
            *source = from;
            count += 1;
        }
    }
    count == 1
}
/// detail::Fingerprint: FNV-1a over the base, the failure, the counts and every node's and edge's fields.
pub fn fingerprint(nodes: &[Node], edges: &[Edge], failure: u32, base: u64) -> u64 {
    let mut hash = 14695981039346656037u64;
    let bytes = |values: &[u8], hash: &mut u64| {
        for &value in values {
            *hash = (*hash ^ value as u64).wrapping_mul(1099511628211);
        }
    };
    let number = |mut value: u64, hash: &mut u64| {
        for _ in 0..8 {
            *hash = (*hash ^ (value as u8) as u64).wrapping_mul(1099511628211);
            value >>= 8;
        }
    };
    number(base, &mut hash);
    number(failure as u64, &mut hash);
    number(nodes.len() as u64, &mut hash);
    number(edges.len() as u64, &mut hash);
    for node in nodes {
        number(node.handle as u64, &mut hash);
        number(node.order as u64, &mut hash);
        bytes(&node.bytes, &mut hash);
        number(node.vtable, &mut hash);
        number(node.work, &mut hash);
        bytes(&node.definition, &mut hash);
    }
    for edge in edges {
        number(edge.handle as u64, &mut hash);
        number(edge.order as u64, &mut hash);
        bytes(&edge.bytes, &mut hash);
    }
    hash
}

fn field32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn field64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// paired_frontier::Roles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roles {
    pub collector: u32,
    pub depth: u32,
    pub solid_ready: u32,
    pub discard: u32,
    pub after_discard: u32,
    pub post_gbuffer: u32,
    pub lighting: u32,
    pub final_2d: u32,
    pub solid: Vec<u32>,
    pub prefix: Vec<u32>,
}
/// paired_frontier::Invariants.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invariants {
    pub original_cpu_acyclic: bool,
    pub original_order_acyclic: bool,
    pub candidate_order_acyclic: bool,
    pub cpu_unchanged: bool,
    pub unrelated_order_edges_preserved: bool,
    pub both_producers_before_main_discard: bool,
    pub main_geometry_before_lighting: bool,
    pub peer_geometry_before_lighting: bool,
    /// These require native scheduling/resource/parent receipts, not this planner.
    pub cpu_producer_closure_proved: bool,
    pub resource_order_compiled: bool,
    pub parent_pass_mapping_proved: bool,
}
const _: () = assert!(core::mem::size_of::<Invariants>() == 11);

/// paired_frontier::Plan's computed members (the source snapshot itself stays with the caller).
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub result: u32,
    pub failure: Failure,
    pub boundary: u32,
    pub base: u64,
    pub fingerprint: u64,
    /// The source was retained (on a candidate).
    pub has_source: bool,
    pub roles: [Roles; 2],
    pub original: [Edges; 2],
    pub proposed: [Edges; 2],
    pub removed: [Edges; 2],
    pub added: [Edges; 2],
    pub invariants: Invariants,
    pub cpu_blocker: Vec<u32>,
    pub order_blocker: Vec<u32>,
    pub peer_cpu_closure: Vec<u32>,
    pub cpu_incoming: Edges,
}

/// Plan::PrefixRole: a node's role in the verified producer prefix (0 for none).
fn prefix_role(node: &Node, base: u64) -> u32 {
    let work = if node.work >= base {
        node.work - base
    } else {
        0
    };
    let word = field32(&node.definition, 24);
    match work {
        0xC0F204 => 1,
        0xBF436C => match word {
            17 => 17,
            2 => 18,
            4 => 19,
            _ => 0,
        },
        0xD5BAC4 if word < 4 && field32(&node.definition, 28) == 4 => 32 + word,
        0x77596C => 40,
        0x655FAC => match word {
            1 => 43,
            2 => 49,
            _ => 0,
        },
        0xCB1E88 => 47,
        0x332D5C => {
            let pass = field64(&node.definition, 24);
            if pass == name("RenderPrep") {
                41
            } else if pass == name("PrepareGloballyAccessibleSystems") {
                42
            } else if pass == name("AsyncWater") {
                44
            } else if pass == name("DepthPrepass") {
                45
            } else if pass == name("GBufferEarly_Velocity_Weapon") {
                46
            } else if pass == name("GBuffer_Solid") {
                48
            } else {
                0
            }
        }
        _ => 0,
    }
}

impl Plan {
    /// Plan::EarlyTail.
    pub fn early_tail(&self) -> u32 {
        if self.boundary == DISCARD {
            self.roles[1].discard
        } else {
            self.roles[1].solid_ready
        }
    }

    /// Plan::BuildCandidate: the plan for one snapshot; on a rejection the members computed so far stay, as in C++.
    pub fn build_candidate(
        nodes: &[Node],
        edges: &[Edge],
        source_failure: u32,
        image_base: u64,
        boundary: u32,
    ) -> Plan {
        let mut plan = Plan {
            failure: Failure::Source,
            ..Default::default()
        };
        plan.build(nodes, edges, source_failure, image_base, boundary);
        plan
    }

    fn build(
        &mut self,
        source_nodes: &[Node],
        source_edges: &[Edge],
        source_failure: u32,
        image_base: u64,
        boundary: u32,
    ) {
        if source_failure != 0
            || source_nodes.is_empty()
            || source_nodes.len() > 255
            || source_edges.len() > 65536
            || image_base == 0
            || image_base > u64::MAX - 0xD5BAC4
            || (boundary != SOLID && boundary != DISCARD)
        {
            return;
        }
        self.boundary = boundary;
        let mut nodes: BTreeMap<u32, &Node> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut edge_ids = BTreeSet::new();
        self.failure = Failure::Nodes;
        for (i, node) in source_nodes.iter().enumerate() {
            let id = node.handle & 0xDFFF_FFFF;
            let view = field32(&node.bytes, 24);
            let kind = field32(&node.bytes, 8);
            if id >= 65536
                || node.order as usize != i
                || field32(&node.bytes, 0) != node.order
                || view > 1
                || !matches!(kind, 0 | 1 | 2 | 4)
                || !ids.insert(id)
                || (node.work != 0
                    && (node.vtable == 0
                        || field64(&node.bytes, 16) == 0
                        || field64(&node.definition, 0) != node.vtable))
            {
                return;
            }
            nodes.insert(id, node);
        }
        self.failure = Failure::Edges;
        for (i, edge) in source_edges.iter().enumerate() {
            let id = edge.handle & 0xBFFF_FFFF;
            let phase = field32(&edge.bytes, 4);
            let from = field32(&edge.bytes, 8) & 0xDFFF_FFFF;
            let to = field32(&edge.bytes, 12) & 0xDFFF_FFFF;
            if id >= 262144
                || edge.order as usize != i
                || field32(&edge.bytes, 0) != edge.order
                || phase > 1
                || !edge_ids.insert(id)
                || !ids.contains(&from)
                || !ids.contains(&to)
                || from == to
                || !self.original[phase as usize].insert((from, to))
            {
                return;
            }
        }
        self.failure = Failure::CpuCycle;
        self.invariants.original_cpu_acyclic = acyclic(ids.iter().copied(), &self.original[0]);
        if !self.invariants.original_cpu_acyclic {
            return;
        }
        self.failure = Failure::OrderCycle;
        self.invariants.original_order_acyclic = acyclic(ids.iter().copied(), &self.original[1]);
        if !self.invariants.original_order_acyclic {
            return;
        }

        self.failure = Failure::Roles;
        let mut counts = [[0u32; 6]; 2];
        for (&id, node) in &nodes {
            let view = field32(&node.bytes, 24) as usize;
            let roles = &mut self.roles[view];
            let work = if node.work >= image_base {
                node.work - image_base
            } else {
                0
            };
            if work == 0xC0F204 {
                roles.collector = id;
                counts[view][0] += 1;
            }
            if work != 0x332D5C {
                continue;
            }
            let pass = field64(&node.definition, 24);
            if pass == name("DepthPrepass") {
                roles.depth = id;
                counts[view][1] += 1;
            } else if pass == name("GBuffer_Solid") {
                roles.solid.push(id);
            } else if pass == name("GBuffer_Discard") {
                roles.discard = id;
                counts[view][2] += 1;
            } else if pass == name("PostGBufferOverrides") {
                roles.post_gbuffer = id;
                counts[view][3] += 1;
            } else if pass == name("Lighting") {
                roles.lighting = id;
                counts[view][4] += 1;
            } else if pass == name("RenderFinal2D") {
                roles.final_2d = id;
                counts[view][5] += 1;
            }
        }
        for eye in 0..2 {
            if counts[eye] != [1; 6] || self.roles[eye].solid.len() != 4 {
                return;
            }
            let mut ready = 0;
            if !only_predecessor(&self.original[1], self.roles[eye].discard, &mut ready) {
                return;
            }
            let node = nodes[&ready];
            if node.work != image_base + 0x655FAC
                || field32(&node.bytes, 24) != eye as u32
                || field32(&node.definition, 24) != 2
            {
                return;
            }
            self.roles[eye].solid_ready = ready;
        }

        self.failure = Failure::Prefix;
        // Exact verified native producer shape, identified by roles, not node IDs.
        const EXPECTED: [u32; 21] = [
            1, 17, 18, 19, 32, 33, 34, 35, 40, 41, 42, 43, 44, 45, 46, 47, 48, 48, 48, 48, 49,
        ];
        for eye in 0..2 {
            let mut current = self.roles[eye].collector;
            for (step, &expected) in EXPECTED.iter().enumerate() {
                let node = nodes[&current];
                if field32(&node.bytes, 24) != eye as u32
                    || prefix_role(node, image_base) != expected
                {
                    return;
                }
                self.roles[eye].prefix.push(current);
                if step + 1 < EXPECTED.len() {
                    let mut next = 0;
                    if !only_successor(&self.original[1], current, &mut next) {
                        return;
                    }
                    let mut previous = 0;
                    if !only_predecessor(&self.original[1], next, &mut previous)
                        || previous != current
                    {
                        return;
                    }
                    current = next;
                }
            }
            let roles = &self.roles[eye];
            if current != roles.solid_ready || !roles.prefix.contains(&roles.depth) {
                return;
            }
            let solid: BTreeSet<u32> = roles.solid.iter().copied().collect();
            let prefix_solid: BTreeSet<u32> = roles.prefix
                [roles.prefix.len() - 5..roles.prefix.len() - 1]
                .iter()
                .copied()
                .collect();
            if solid != prefix_solid {
                return;
            }
        }

        self.failure = Failure::OrderBoundary;
        for eye in 0..2 {
            let discard = self.roles[eye].discard;
            let successor = only_successor(
                &self.original[1],
                discard,
                &mut self.roles[eye].after_discard,
            );
            if self.boundary != DISCARD {
                continue;
            }
            let roles = &self.roles[eye];
            let mut previous = 0;
            if !successor
                || !only_predecessor(&self.original[1], roles.after_discard, &mut previous)
                || previous != discard
            {
                return;
            }
            // The whole discard pass precedes the unchanged per-eye post/lighting suffix.
            let mut current = discard;
            for target in [roles.post_gbuffer, roles.lighting, roles.final_2d] {
                while current != target {
                    let mut next = 0;
                    if !only_successor(&self.original[1], current, &mut next)
                        || field32(&nodes[&next].bytes, 24) != eye as u32
                        || !only_predecessor(&self.original[1], next, &mut previous)
                        || previous != current
                    {
                        return;
                    }
                    current = next;
                }
            }
        }
        let (main, peer) = (self.roles[0].clone(), self.roles[1].clone());
        let order = &self.original[1];
        let mut previous = 0;
        if !only_predecessor(order, peer.collector, &mut previous)
            || previous != main.final_2d
            || !order.contains(&(main.solid_ready, main.discard))
            || !order.contains(&(peer.solid_ready, peer.discard))
            || path(order, main.discard, main.post_gbuffer).is_empty()
            || path(order, main.post_gbuffer, main.lighting).is_empty()
            || path(order, main.lighting, main.final_2d).is_empty()
            || path(order, peer.discard, peer.post_gbuffer).is_empty()
            || path(order, peer.post_gbuffer, peer.lighting).is_empty()
        {
            return;
        }
        let tail = self.early_tail();
        let continuation = if self.boundary == DISCARD {
            peer.after_discard
        } else {
            peer.discard
        };
        self.removed[1] = Edges::from([
            (main.solid_ready, main.discard),
            (main.final_2d, peer.collector),
            (tail, continuation),
        ]);
        self.added[1] = Edges::from([
            (main.solid_ready, peer.collector),
            (tail, main.discard),
            (main.final_2d, continuation),
        ]);
        self.proposed = self.original.clone();
        for edge in &self.removed[1] {
            if !self.proposed[1].remove(edge) {
                return;
            }
        }
        for edge in &self.added[1] {
            if !self.proposed[1].insert(*edge) {
                return;
            }
        }
        self.failure = Failure::CandidateCycle;
        self.invariants.candidate_order_acyclic = acyclic(ids.iter().copied(), &self.proposed[1]);
        if !self.invariants.candidate_order_acyclic {
            return;
        }
        self.invariants.cpu_unchanged = self.proposed[0] == self.original[0];
        let mut unrelated = self.proposed[1].clone();
        for edge in &self.added[1] {
            unrelated.remove(edge);
        }
        for edge in &self.removed[1] {
            unrelated.insert(*edge);
        }
        self.invariants.unrelated_order_edges_preserved = unrelated == self.original[1];
        let candidate = &self.proposed[1];
        let reaches = |from: u32, to: u32| !path(candidate, from, to).is_empty();
        self.invariants.both_producers_before_main_discard =
            reaches(main.solid_ready, main.discard) && reaches(tail, main.discard);
        self.invariants.main_geometry_before_lighting =
            reaches(main.discard, main.post_gbuffer) && reaches(main.post_gbuffer, main.lighting);
        self.invariants.peer_geometry_before_lighting =
            reaches(peer.discard, peer.post_gbuffer) && reaches(peer.post_gbuffer, peer.lighting);
        self.failure = Failure::Invariant;
        let proofs = &self.invariants;
        if !proofs.cpu_unchanged
            || !proofs.unrelated_order_edges_preserved
            || !proofs.both_producers_before_main_discard
            || !proofs.main_geometry_before_lighting
            || !proofs.peer_geometry_before_lighting
        {
            return;
        }
        self.cpu_blocker = path(&self.original[0], main.discard, tail);
        self.order_blocker = path(&self.original[1], main.discard, tail);
        // Include synthetic joins and per-eye prerequisites in the reported CPU
        // closure, but do not assume their ownership constraints can be moved.
        let mut closure = BTreeSet::from([tail]);
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &self.original[0] {
                if closure.contains(&b) && field32(&nodes[&a].bytes, 24) == 1 {
                    changed |= closure.insert(a);
                }
            }
        }
        self.peer_cpu_closure = closure.iter().copied().collect();
        for &(a, b) in &self.original[0] {
            if !closure.contains(&a) && closure.contains(&b) {
                self.cpu_incoming.insert((a, b));
            }
        }
        self.has_source = true;
        self.base = image_base;
        self.fingerprint = fingerprint(source_nodes, source_edges, source_failure, image_base);
        self.failure = Failure::None;
        self.result = CANDIDATE_NEEDS_CPU_AND_RESOURCE_COMPILE;
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

/// detail::Path over a flattened edge set (ascending); writes at most `count + 1` ids to `output`, returns the count.
///
/// # Safety
/// `edges` holds `count` links; `output` has room for `count + 1` ids.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_path(
    edges: *const [u32; 2],
    count: usize,
    from: u32,
    to: u32,
    output: *mut u32,
) -> usize {
    unsafe {
        let result = path(slice(edges, count), from, to);
        core::ptr::copy_nonoverlapping(result.as_ptr(), output, result.len());
        result.len()
    }
}
/// detail::Acyclic over flattened (ascending, distinct) nodes and edges.
///
/// # Safety
/// The arrays hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_acyclic(
    nodes: *const u32,
    node_count: usize,
    edges: *const [u32; 2],
    edge_count: usize,
) -> bool {
    unsafe {
        acyclic(
            slice(nodes, node_count).iter().copied(),
            slice(edges, edge_count),
        )
    }
}
/// detail::OnlySuccessor (`which` 0) / OnlyPredecessor (1) over a flattened edge set.
///
/// # Safety
/// `edges` holds `count` links; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_only(
    which: u32,
    edges: *const [u32; 2],
    count: usize,
    node: u32,
    output: *mut u32,
) -> bool {
    unsafe {
        let edges = slice(edges, count);
        if which == 0 {
            only_successor(edges, node, &mut *output)
        } else {
            only_predecessor(edges, node, &mut *output)
        }
    }
}
/// detail::Fingerprint.
///
/// # Safety
/// The arrays hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_fingerprint(
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    failure: u32,
    base: u64,
) -> u64 {
    unsafe {
        fingerprint(
            slice(nodes, node_count),
            slice(edges, edge_count),
            failure,
            base,
        )
    }
}

/// A plan's roles, lent.
#[repr(C)]
pub struct RawRoles {
    /// collector, depth, solidReady, discard, afterDiscard, postGBuffer, lighting, final2D.
    pub ids: [u32; 8],
    pub solid: *const u32,
    pub solid_count: usize,
    pub prefix: *const u32,
    pub prefix_count: usize,
}
/// An edge set, lent flattened (ascending).
#[repr(C)]
pub struct RawLinks {
    pub data: *const [u32; 2],
    pub count: usize,
}
/// Node ids, lent.
#[repr(C)]
pub struct RawIds {
    pub data: *const u32,
    pub count: usize,
}
/// A built plan, lent until `cvr_pf_free`.
#[repr(C)]
pub struct RawPlan {
    pub result: u32,
    pub failure: u32,
    pub boundary: u32,
    pub has_source: bool,
    pub base: u64,
    pub fingerprint: u64,
    pub roles: [RawRoles; 2],
    /// original[0..2], proposed[0..2], removed[0..2], added[0..2], cpuIncoming.
    pub links: [RawLinks; 9],
    /// cpuBlocker, orderBlocker, peerCpuClosure.
    pub ids: [RawIds; 3],
    pub invariants: Invariants,
}
const _: () = assert!(
    core::mem::size_of::<RawRoles>() == 64
        && core::mem::size_of::<RawPlan>() == 368
        && core::mem::offset_of!(RawPlan, invariants) == 352
);

/// A built plan with its flattened edge sets.
pub struct Built {
    plan: Plan,
    links: [Vec<[u32; 2]>; 9],
}

/// Plan::BuildCandidate: the built plan (released by `cvr_pf_free`), lent through `output`.
///
/// # Safety
/// The arrays hold their counts; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_build(
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    failure: u32,
    base: u64,
    boundary: u32,
    output: *mut RawPlan,
) -> *mut Built {
    let plan = unsafe {
        Plan::build_candidate(
            slice(nodes, node_count),
            slice(edges, edge_count),
            failure,
            base,
            boundary,
        )
    };
    let flat = |edges: &Edges| edges.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>();
    let links = [
        flat(&plan.original[0]),
        flat(&plan.original[1]),
        flat(&plan.proposed[0]),
        flat(&plan.proposed[1]),
        flat(&plan.removed[0]),
        flat(&plan.removed[1]),
        flat(&plan.added[0]),
        flat(&plan.added[1]),
        flat(&plan.cpu_incoming),
    ];
    let built = Box::new(Built { plan, links });
    let plan = &built.plan;
    let roles = |r: &Roles| RawRoles {
        ids: [
            r.collector,
            r.depth,
            r.solid_ready,
            r.discard,
            r.after_discard,
            r.post_gbuffer,
            r.lighting,
            r.final_2d,
        ],
        solid: r.solid.as_ptr(),
        solid_count: r.solid.len(),
        prefix: r.prefix.as_ptr(),
        prefix_count: r.prefix.len(),
    };
    let ids = |v: &Vec<u32>| RawIds {
        data: v.as_ptr(),
        count: v.len(),
    };
    let raw = RawPlan {
        result: plan.result,
        failure: plan.failure as u32,
        boundary: plan.boundary,
        has_source: plan.has_source,
        base: plan.base,
        fingerprint: plan.fingerprint,
        roles: [roles(&plan.roles[0]), roles(&plan.roles[1])],
        links: core::array::from_fn(|i| RawLinks {
            data: built.links[i].as_ptr(),
            count: built.links[i].len(),
        }),
        ids: [
            ids(&plan.cpu_blocker),
            ids(&plan.order_blocker),
            ids(&plan.peer_cpu_closure),
        ],
        invariants: plan.invariants,
    };
    unsafe { *output = raw };
    Box::into_raw(built)
}
/// Releases a built plan.
///
/// # Safety
/// `built` came from `cvr_pf_build` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pf_free(built: *mut Built) {
    if !built.is_null() {
        drop(unsafe { Box::from_raw(built) });
    }
}
