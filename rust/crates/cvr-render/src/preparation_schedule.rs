//! The native preparation schedule (`include/Render/NativePreparationSchedule.hpp`): over a graph with exactly one
//! prepare collector (work 0xC0F204), one scene cull (0xBF436C, flags 17) among three culls and four particle nodes
//! (0xD5BAC4) per view, the peer view's early set -- its collector, the single synthetic bridge from it and its scene
//! cull -- whose only CPU predecessors are the collector's; the frontier the early set feeds, the late barriers (CPU
//! predecessors of the peer collector reachable from the main collector) moved after the peer scene cull and before
//! every frontier target, the main view's collector, culls and particles all made CPU prerequisites of the peer
//! collector; the result acyclic, every other CPU edge kept and every barrier reaching every frontier target. Plus the
//! partial native job-graph shadow 0x8EA1C8 consumes (nodes, CPU edges, edge order and the 0x70-byte header naming
//! them) and the independence of the two graph containers (0xE0500 bytes) and view extents (0x3130).
//!
//! In the Rust build (`CVR_RUST_VRIK`) `Plan::Build` / `Reach`, `JobGraph::Build` and `Independent` forward to
//! `cvr_ps_*`; the plan's members and the job graph's buffers stay in C++ (the header names those buffers). `ready`
//! follows the nodes' id order (C++'s unordered order is unspecified; only its size and members are read).
//! `tools/single_pass_tests/preparation_schedule_test` runs its cases against them in its Rust variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::native::graph_snapshot::{field_u32, Edge, Node};
use crate::paired_frontier::Edges;

/// preparation_schedule::Plan.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub cpu: Edges,
    pub early: Vec<u32>,
    pub ready: Vec<u32>,
    pub barriers: Vec<u32>,
    pub frontier: Vec<u32>,
    pub failure: u32,
}

/// Plan::Reach over CPU edges.
pub fn reach(cpu: &Edges, from: u32, to: u32) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut pending = vec![from];
    let mut i = 0;
    while i < pending.len() {
        if pending[i] == to {
            return true;
        }
        let current = pending[i];
        for &(_, next) in cpu.range((current, 0)..=(current, u32::MAX)) {
            if seen.insert(next) {
                pending.push(next);
            }
        }
        i += 1;
    }
    false
}

impl Plan {
    /// Plan::Build; on a refusal the members computed so far stay, as in C++.
    pub fn build(nodes_in: &[Node], edges_in: &[Edge], source_failure: u32, base: u64) -> Plan {
        let mut plan = Plan {
            failure: 1,
            ..Default::default()
        };
        plan.fill(nodes_in, edges_in, source_failure, base);
        plan
    }

    fn fill(&mut self, nodes_in: &[Node], edges_in: &[Edge], source_failure: u32, base: u64) {
        if source_failure != 0
            || nodes_in.is_empty()
            || nodes_in.len() > 255
            || edges_in.len() > 65536
        {
            return;
        }
        let mut nodes: BTreeMap<u32, &Node> = BTreeMap::new();
        let mut ordinals = BTreeSet::new();
        let (mut prepare, mut scene) = ([0u32; 2], [0u32; 2]);
        let (mut prepares, mut scenes, mut culls, mut particles) =
            ([0u32; 2], [0u32; 2], [0u32; 2], [0u32; 2]);
        for n in nodes_in {
            let id = n.handle & 0xDFFF_FFFF;
            let view = field_u32(&n.bytes, 24);
            let ordinal = field_u32(&n.bytes, 0);
            if view > 1
                || ordinal != n.order
                || nodes.contains_key(&id)
                || !ordinals.insert(ordinal)
            {
                return;
            }
            nodes.insert(id, n);
            let view = view as usize;
            let work = if n.work >= base { n.work - base } else { 0 };
            if work == 0xC0F204 {
                prepare[view] = id;
                prepares[view] += 1;
            }
            if work == 0xBF436C {
                culls[view] += 1;
                if field_u32(&n.definition, 24) == 17 {
                    scene[view] = id;
                    scenes[view] += 1;
                }
            }
            if work == 0xD5BAC4 {
                particles[view] += 1;
            }
        }
        if prepares != [1, 1] || scenes != [1, 1] || culls != [3, 3] || particles != [4, 4] {
            return;
        }
        for edge in edges_in {
            let phase = field_u32(&edge.bytes, 4);
            let from = field_u32(&edge.bytes, 8) & 0xDFFF_FFFF;
            let to = field_u32(&edge.bytes, 12) & 0xDFFF_FFFF;
            if phase > 1 || !nodes.contains_key(&from) || !nodes.contains_key(&to) || from == to {
                return;
            }
            if phase == 0 && !self.cpu.insert((from, to)) {
                return;
            }
        }
        self.failure = 2;
        let incoming: Vec<u32> = self
            .cpu
            .iter()
            .filter(|&&(_, b)| b == scene[1])
            .map(|&(a, _)| a)
            .collect();
        if incoming.len() != 1 {
            return;
        }
        let bridge = incoming[0];
        let node = nodes[&bridge];
        if node.work != 0
            || field_u32(&node.bytes, 24) != 1
            || !self.cpu.contains(&(prepare[1], bridge))
        {
            return;
        }
        if self
            .cpu
            .iter()
            .any(|&(a, b)| b == bridge && a != prepare[1])
        {
            return;
        }
        self.early = vec![prepare[1], bridge, scene[1]];
        let early = self.early.clone();
        let is_early = |id: u32| early.contains(&id);
        for &(a, b) in &self.cpu {
            if !is_early(a) && is_early(b) && b != prepare[1] {
                return;
            }
            if is_early(a) && !is_early(b) {
                self.frontier.push(b);
            }
            if b == prepare[1] && reach(&self.cpu, prepare[0], a) {
                self.barriers.push(a);
            }
        }
        self.frontier.sort_unstable();
        self.frontier.dedup();
        if self.frontier.is_empty() || self.barriers.is_empty() {
            return;
        }
        self.ready.push(prepare[0]);
        for (&id, n) in &nodes {
            if field_u32(&n.bytes, 24) != 0 {
                continue;
            }
            let work = if n.work >= base { n.work - base } else { 0 };
            if work == 0xBF436C || work == 0xD5BAC4 {
                if !reach(&self.cpu, prepare[0], id) {
                    return;
                }
                self.ready.push(id);
            }
        }
        let original = self.cpu.clone();
        for &barrier in &self.barriers {
            self.cpu.remove(&(barrier, prepare[1]));
            // Only the collector/query/materializer move. Every outgoing path
            // rejoins the original MAIN barriers before the remaining eye work.
            for &target in &self.frontier {
                self.cpu.insert((barrier, target));
            }
            self.cpu.insert((scene[1], barrier));
        }
        for &source in &self.ready {
            self.cpu.insert((source, prepare[1]));
        }
        self.failure = 3;
        let mut degree: BTreeMap<u32, u32> = nodes.keys().map(|&id| (id, 0)).collect();
        for &(_, b) in &self.cpu {
            *degree.entry(b).or_insert(0) += 1;
        }
        let mut pending: Vec<u32> = degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut i = 0;
        while i < pending.len() {
            let current = pending[i];
            for &(_, next) in self.cpu.range((current, 0)..=(current, u32::MAX)) {
                let d = degree.entry(next).or_insert(0);
                *d = d.wrapping_sub(1);
                if *d == 0 {
                    pending.push(next);
                }
            }
            i += 1;
        }
        if pending.len() != nodes.len() {
            return;
        }
        self.failure = 4;
        if original
            .iter()
            .any(|&(a, b)| !is_early(a) && !is_early(b) && !self.cpu.contains(&(a, b)))
        {
            return;
        }
        for &source in &self.barriers {
            for &target in &self.frontier {
                if !reach(&self.cpu, source, target) {
                    return;
                }
            }
        }
        self.failure = 0;
    }
}

/// preparation_schedule::Independent: the two graph containers (0xE0500) and view extents (0x3130) disjoint.
pub fn independent(ranges: &[u64; 4]) -> bool {
    // 2.5: graph container 0xE0500 (sub_140423180 add r8,0E0500h), view extent 0x3130 (2.31: 0xC5C40 / 0x22C0).
    const SIZES: [u64; 4] = [0xE0500, 0x3130, 0xE0500, 0x3130];
    for i in 0..4 {
        if ranges[i] == 0 || ranges[i] > u64::MAX - SIZES[i] {
            return false;
        }
        for j in 0..i {
            if ranges[i] < ranges[j] + SIZES[j] && ranges[j] < ranges[i] + SIZES[i] {
                return false;
            }
        }
    }
    true
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
pub struct RawIds {
    pub data: *const u32,
    pub count: usize,
}
/// A built plan, lent until `cvr_ps_free`: its CPU edges flattened (ascending), then early, ready, barriers, frontier.
#[repr(C)]
pub struct RawPlan {
    pub cpu: *const [u32; 2],
    pub cpu_count: usize,
    pub ids: [RawIds; 4],
    pub failure: u32,
}
const _: () = assert!(core::mem::size_of::<RawPlan>() == 88);
/// A built plan with its flattened edges.
pub struct Held {
    plan: Plan,
    cpu: Vec<[u32; 2]>,
}

/// Plan::Build: the plan (released by `cvr_ps_free`), lent through `out`.
///
/// # Safety
/// The arrays hold their counts; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_build(
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    source_failure: u32,
    base: u64,
    out: *mut RawPlan,
) -> *mut Held {
    let plan = unsafe {
        Plan::build(
            slice(nodes, node_count),
            slice(edges, edge_count),
            source_failure,
            base,
        )
    };
    let cpu = plan.cpu.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>();
    let held = Box::new(Held { plan, cpu });
    let p = &held.plan;
    let ids = |v: &Vec<u32>| RawIds {
        data: v.as_ptr(),
        count: v.len(),
    };
    unsafe {
        *out = RawPlan {
            cpu: held.cpu.as_ptr(),
            cpu_count: held.cpu.len(),
            ids: [
                ids(&p.early),
                ids(&p.ready),
                ids(&p.barriers),
                ids(&p.frontier),
            ],
            failure: p.failure,
        };
    }
    Box::into_raw(held)
}
/// Releases a plan.
///
/// # Safety
/// `held` came from `cvr_ps_build` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_free(held: *mut Held) {
    if !held.is_null() {
        drop(unsafe { Box::from_raw(held) });
    }
}
/// Plan::Reach over flattened (ascending) CPU edges.
///
/// # Safety
/// `cpu` holds `count` links.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_reach(
    cpu: *const [u32; 2],
    count: usize,
    from: u32,
    to: u32,
) -> bool {
    let edges: Edges = unsafe { slice(cpu, count).iter().map(|l| (l[0], l[1])).collect() };
    reach(&edges, from, to)
}
/// preparation_schedule::Independent.
///
/// # Safety
/// `ranges` holds four values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_independent(ranges: *const [u64; 4]) -> bool {
    unsafe { independent(&*ranges) }
}
/// JobGraph::Build's records: the slot count (max id + 1) and, into `nodes` (that many slots, zeroed by the caller),
/// `edges` and `order` (one per CPU edge), each node's bytes at its id and each CPU edge's record in order.
///
/// # Safety
/// `source` holds `count` nodes; `cpu` holds `cpu_count` links; the outputs have room (null `nodes` asks for the count).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_job_records(
    source: *const Node,
    count: usize,
    cpu: *const [u32; 2],
    cpu_count: usize,
    nodes: *mut [u8; 48],
    edges: *mut [u32; 6],
    order: *mut u32,
) -> u32 {
    unsafe {
        let source = slice(source, count);
        let size = source
            .iter()
            .map(|n| (n.handle & 0xDFFF_FFFF) + 1)
            .max()
            .unwrap_or(0);
        if nodes.is_null() {
            return size;
        }
        for n in source {
            *nodes.add((n.handle & 0xDFFF_FFFF) as usize) = n.bytes;
        }
        for (index, link) in slice(cpu, cpu_count).iter().enumerate() {
            let index32 = index as u32;
            *edges.add(index) = [
                index32,
                0,
                link[0] | 0x2000_0000,
                link[1] | 0x2000_0000,
                0x8000_0000,
                0x8000_0000,
            ];
            *order.add(index) = index32;
        }
        size
    }
}
/// JobGraph's header: the nodes at +0x18, the edge count at +0x40, the edges at +0x48, the edge order at +0x58.
///
/// # Safety
/// `header` is 0x70 writable bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ps_job_header(
    header: *mut [u8; 0x70],
    nodes: u64,
    edge_count: u32,
    edges: u64,
    order: u64,
) {
    unsafe {
        let header = &mut *header;
        header[0x18..0x20].copy_from_slice(&nodes.to_le_bytes());
        header[0x40..0x44].copy_from_slice(&edge_count.to_le_bytes());
        header[0x48..0x50].copy_from_slice(&edges.to_le_bytes());
        header[0x58..0x60].copy_from_slice(&order.to_le_bytes());
    }
}
