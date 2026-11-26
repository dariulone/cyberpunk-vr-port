//! The owned native graph (`include/Render/OwnedNativeGraph.hpp`): the complete node / order / edge adjacency the
//! native sorter (native985D0C / 9861D0) reads, built from a source graph and a proposed edge set -- 48-byte node
//! records with their four adjacency heads emptied and re-linked, 24-byte edge records (index, phase, endpoints, the
//! next outgoing and incoming edge), the edge order and the 0x70-byte header naming them. NOT a native allocator pool or
//! a definition owner: only the audited synchronous sorter may mutate it; async declaration or execution requires a
//! separate lease on the original unique definition owners and native job consumers. Its reference compile ranks every
//! node per phase by its longest-path level (ties by source order), and its validation walks every adjacency chain
//! back to exactly the proposed edges.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `OwnedNativeGraph` owns this Rust graph (its buffers are the ones the header
//! names) and forwards to `cvr_ong_*`; `tools/single_pass_tests/owned_native_graph_test` runs its cases against it in
//! its Rust variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::native::graph_snapshot::{field_u32, field_u64, Edge, Node};
use crate::paired_frontier::{acyclic, Edges};

const EMPTY: u32 = 0x8000_0000;

/// The graph header (0x70 bytes, 8-aligned).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct Header(pub [u8; 0x70]);
impl Default for Header {
    fn default() -> Self {
        Header([0; 0x70])
    }
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// OwnedNativeGraph.
#[derive(Default)]
pub struct OwnedGraph {
    header: Box<Header>,
    constructed: bool,
    source: Option<(Vec<Node>, Vec<Edge>)>,
    edges: [Edges; 2],
    nodes: Vec<[u8; 48]>,
    order: Vec<u32>,
    edge_order: Vec<u32>,
    edge_data: Vec<[u32; 6]>,
}

impl OwnedGraph {
    /// OwnedNativeGraph::Create.
    pub fn create(
        &mut self,
        source_nodes: &[Node],
        source_edges: &[Edge],
        source_failure: u32,
        edges: [Edges; 2],
    ) -> bool {
        if self.source.is_some()
            || source_failure != 0
            || source_nodes.is_empty()
            || source_nodes.len() > 255
            || source_edges.len() > 65536
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        let mut slots = 0u32;
        for (i, n) in source_nodes.iter().enumerate() {
            let id = n.handle & 0xDFFF_FFFF;
            if id >= 65536
                || n.order as usize != i
                || field_u32(&n.bytes, 0) as usize != i
                || !ids.insert(id)
                || (n.handle != id && n.handle != (id | 0x2000_0000))
            {
                return false;
            }
            slots = slots.max(id + 1);
        }
        if edges[0].len() + edges[1].len() > 65536
            || !acyclic(ids.iter().copied(), &edges[0])
            || !acyclic(ids.iter().copied(), &edges[1])
        {
            return false;
        }
        self.source = Some((source_nodes.to_vec(), source_edges.to_vec()));
        self.edges = edges;
        self.nodes.resize(slots as usize, [0; 48]);
        self.order.reserve(source_nodes.len());
        for n in source_nodes {
            let id = (n.handle & 0xDFFF_FFFF) as usize;
            self.nodes[id] = n.bytes;
            self.order.push(n.handle);
            for at in [32, 36, 40, 44] {
                put32(&mut self.nodes[id], at, EMPTY);
            }
        }
        let total = self.edges[0].len() + self.edges[1].len();
        self.edge_data.reserve(total);
        self.edge_order.reserve(total);
        for phase in 0..2u32 {
            for &(from, to) in &self.edges[phase as usize] {
                if !ids.contains(&from) || !ids.contains(&to) {
                    return false;
                }
                let index = self.edge_data.len() as u32;
                let handle = index | 0x4000_0000;
                let p = phase as usize * 4;
                self.edge_data.push([
                    index,
                    phase,
                    from | 0x2000_0000,
                    to | 0x2000_0000,
                    field_u32(&self.nodes[from as usize], 40 + p),
                    field_u32(&self.nodes[to as usize], 32 + p),
                ]);
                self.edge_order.push(handle);
                put32(&mut self.nodes[from as usize], 40 + p, handle);
                put32(&mut self.nodes[to as usize], 32 + p, handle);
            }
        }
        let header = &mut self.header.0;
        put32(header, 16, self.order.len() as u32);
        put64(header, 24, self.nodes.as_ptr() as u64);
        put64(header, 40, self.order.as_ptr() as u64);
        put32(header, 64, self.edge_data.len() as u32);
        put64(header, 72, self.edge_data.as_ptr() as u64);
        put64(header, 88, self.edge_order.as_ptr() as u64);
        self.constructed = true;
        true
    }

    /// OwnedNativeGraph::SortableData.
    pub fn sortable_data(&mut self) -> *mut u8 {
        if self.constructed {
            self.header.0.as_mut_ptr()
        } else {
            core::ptr::null_mut()
        }
    }
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// OwnedNativeGraph::ReferenceRanks: each node's rank by longest-path level, ties by source order.
    fn reference_ranks(&self, phase: usize) -> BTreeMap<u32, u32> {
        let mut degree: BTreeMap<u32, u32> = BTreeMap::new();
        let mut level: BTreeMap<u32, u32> = BTreeMap::new();
        for &raw in &self.order {
            let id = raw & 0xDFFF_FFFF;
            degree.insert(id, 0);
            level.insert(id, 0);
        }
        for &(a, b) in &self.edges[phase] {
            if !degree.contains_key(&a) || !degree.contains_key(&b) {
                return BTreeMap::new();
            }
            *degree.get_mut(&b).unwrap() += 1;
        }
        let mut ready: Vec<u32> = degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut i = 0;
        while i < ready.len() {
            let current = ready[i];
            for &(a, b) in self.edges[phase].range((current, 0)..=(current, u32::MAX)) {
                let next = level[&a] + 1;
                let entry = level.get_mut(&b).unwrap();
                *entry = (*entry).max(next);
                let d = degree.get_mut(&b).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(b);
                }
            }
            i += 1;
        }
        if ready.len() != self.order.len() {
            return BTreeMap::new();
        }
        let mut sorted: Vec<(u32, u32)> = (0..self.order.len() as u32)
            .map(|ordinal| {
                (
                    level[&(self.order[ordinal as usize] & 0xDFFF_FFFF)],
                    ordinal,
                )
            })
            .collect();
        sorted.sort();
        sorted
            .iter()
            .enumerate()
            .map(|(rank, &(_, ordinal))| (self.order[ordinal as usize] & 0xDFFF_FFFF, rank as u32))
            .collect()
    }

    /// OwnedNativeGraph::CompileReference.
    pub fn compile_reference(&mut self) -> bool {
        if !self.constructed {
            return false;
        }
        let Some((source_nodes, _)) = &self.source else {
            return false;
        };
        if self.order.len() != source_nodes.len() {
            return false;
        }
        for (i, &raw) in self.order.iter().enumerate() {
            if raw != source_nodes[i].handle || (raw & 0xDFFF_FFFF) as usize >= self.nodes.len() {
                return false;
            }
        }
        for phase in 0..2 {
            let ranks = self.reference_ranks(phase);
            if ranks.len() != self.order.len() {
                return false;
            }
            for (id, rank) in ranks {
                self.nodes[id as usize][28 + phase] = rank as u8;
            }
        }
        self.header.0[0x68] = 1;
        self.validate_compiled()
    }

    /// OwnedNativeGraph::ValidateCompiled.
    pub fn validate_compiled(&self) -> bool {
        let Some((source_nodes, _)) = &self.source else {
            return false;
        };
        let header = &self.header.0;
        if !self.constructed
            || header[0x68] != 1
            || field_u32(header, 16) as usize != self.order.len()
            || field_u64(header, 24) != self.nodes.as_ptr() as u64
            || field_u64(header, 40) != self.order.as_ptr() as u64
            || field_u32(header, 64) as usize != self.edge_data.len()
            || field_u64(header, 72) != self.edge_data.as_ptr() as u64
            || field_u64(header, 88) != self.edge_order.as_ptr() as u64
        {
            return false;
        }
        for (i, original) in source_nodes.iter().enumerate() {
            let id = (original.handle & 0xDFFF_FFFF) as usize;
            let node = &self.nodes[id];
            if self.order[i] != original.handle
                || original.bytes[..28] != node[..28]
                || node[30] != original.bytes[30]
                || node[31] != original.bytes[31]
            {
                return false;
            }
        }
        for phase in 0..2 {
            let ranks = self.reference_ranks(phase);
            if ranks.len() != self.order.len() {
                return false;
            }
            for (id, rank) in ranks {
                if self.nodes[id as usize][28 + phase] as u32 != rank {
                    return false;
                }
            }
        }
        let mut actual: [Edges; 2] = Default::default();
        let mut outgoing_seen: [BTreeSet<u32>; 2] = Default::default();
        let mut incoming_seen: [BTreeSet<u32>; 2] = Default::default();
        for &raw in &self.order {
            let id = raw & 0xDFFF_FFFF;
            for phase in 0..2u32 {
                for direction in 0..2 {
                    let mut handle = field_u32(
                        &self.nodes[id as usize],
                        if direction == 1 { 40 } else { 32 } + phase as usize * 4,
                    );
                    let mut chain = BTreeSet::new();
                    while handle != EMPTY {
                        let index = handle & 0xBFFF_FFFF;
                        if index as usize >= self.edge_data.len()
                            || handle != (index | 0x4000_0000)
                            || !chain.insert(index)
                        {
                            return false;
                        }
                        let e = &self.edge_data[index as usize];
                        if e[0] != index
                            || e[1] != phase
                            || (e[if direction == 1 { 2 } else { 3 }] & 0xDFFF_FFFF) != id
                        {
                            return false;
                        }
                        let seen = if direction == 1 {
                            &mut outgoing_seen[phase as usize]
                        } else {
                            &mut incoming_seen[phase as usize]
                        };
                        if !seen.insert(index) {
                            return false;
                        }
                        if direction == 1 {
                            actual[phase as usize].insert((e[2] & 0xDFFF_FFFF, e[3] & 0xDFFF_FFFF));
                        }
                        handle = e[if direction == 1 { 4 } else { 5 }];
                    }
                }
            }
        }
        if actual != self.edges {
            return false;
        }
        for phase in 0..2 {
            if outgoing_seen[phase] != incoming_seen[phase]
                || outgoing_seen[phase].len() != self.edges[phase].len()
            {
                return false;
            }
        }
        self.edge_order
            .iter()
            .enumerate()
            .all(|(i, &handle)| handle == (i as u32 | 0x4000_0000))
    }

    /// OwnedNativeGraph::Snapshot: the compiled nodes and edges (None for C++'s failed snapshot).
    pub fn snapshot(&self) -> Option<(Vec<Node>, Vec<Edge>)> {
        let (source_nodes, _) = self.source.as_ref()?;
        if !self.validate_compiled() {
            return None;
        }
        let nodes = source_nodes
            .iter()
            .map(|n| Node {
                bytes: self.nodes[(n.handle & 0xDFFF_FFFF) as usize],
                ..*n
            })
            .collect();
        let edges = self
            .edge_data
            .iter()
            .enumerate()
            .map(|(i, data)| {
                let mut bytes = [0u8; 24];
                for (k, word) in data.iter().enumerate() {
                    bytes[k * 4..k * 4 + 4].copy_from_slice(&word.to_le_bytes());
                }
                Edge {
                    handle: self.edge_order[i],
                    order: i as u32,
                    bytes,
                }
            })
            .collect();
        Some((nodes, edges))
    }

    /// OwnedNativeGraph::SameSourceOrders.
    pub fn same_source_orders(&self) -> bool {
        let Some((source_nodes, _)) = &self.source else {
            return false;
        };
        if !self.validate_compiled() {
            return false;
        }
        source_nodes.iter().all(|n| {
            (0..2).all(|phase| {
                self.nodes[(n.handle & 0xDFFF_FFFF) as usize][28 + phase] == n.bytes[28 + phase]
            })
        })
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

/// A new, unconstructed graph (released by `cvr_ong_free`).
#[no_mangle]
pub extern "C-unwind" fn cvr_ong_new() -> *mut OwnedGraph {
    Box::into_raw(Box::default())
}
/// Releases a graph.
///
/// # Safety
/// `graph` came from `cvr_ong_new` (or is null) and the native sorter no longer holds its data.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ong_free(graph: *mut OwnedGraph) {
    if !graph.is_null() {
        drop(unsafe { Box::from_raw(graph) });
    }
}
/// OwnedNativeGraph::Create with the proposed edges flattened (ascending) per phase.
///
/// # Safety
/// `graph` is valid; the arrays hold their counts.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ong_create(
    graph: *mut OwnedGraph,
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    source_failure: u32,
    cpu: *const [u32; 2],
    cpu_count: usize,
    order: *const [u32; 2],
    order_count: usize,
) -> bool {
    unsafe {
        let set = |data, count| {
            slice(data, count)
                .iter()
                .map(|l: &[u32; 2]| (l[0], l[1]))
                .collect::<Edges>()
        };
        let proposed = [set(cpu, cpu_count), set(order, order_count)];
        (*graph).create(
            slice(nodes, node_count),
            slice(edges, edge_count),
            source_failure,
            proposed,
        )
    }
}
/// OwnedNativeGraph::SortableData (`which` 0), the header (1), CompileReference (2 → non-null on success),
/// ValidateCompiled (3), SameSourceOrders (4) -- each a pointer or null.
///
/// # Safety
/// `graph` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ong_query(graph: *mut OwnedGraph, which: u32) -> *const u8 {
    let me = graph as *const u8;
    let yes = |value: bool| if value { me } else { core::ptr::null() };
    unsafe {
        let graph = &mut *graph;
        match which {
            0 => graph.sortable_data() as *const u8,
            1 => graph.header().0.as_ptr(),
            2 => yes(graph.compile_reference()),
            3 => yes(graph.validate_compiled()),
            _ => yes(graph.same_source_orders()),
        }
    }
}
/// A compiled snapshot, lent until `cvr_ong_snapshot_free`.
#[repr(C)]
pub struct RawSnapshot {
    pub nodes: *const Node,
    pub node_count: usize,
    pub edges: *const Edge,
    pub edge_count: usize,
}
/// OwnedNativeGraph::Snapshot: the compiled nodes and edges (released by `cvr_ong_snapshot_free`), or null for a
/// failed snapshot.
///
/// # Safety
/// `graph` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ong_snapshot(
    graph: *const OwnedGraph,
    out: *mut RawSnapshot,
) -> *mut (Vec<Node>, Vec<Edge>) {
    unsafe {
        *out = RawSnapshot {
            nodes: core::ptr::null(),
            node_count: 0,
            edges: core::ptr::null(),
            edge_count: 0,
        };
        let Some(snapshot) = (*graph).snapshot() else {
            return core::ptr::null_mut();
        };
        let held = Box::new(snapshot);
        *out = RawSnapshot {
            nodes: held.0.as_ptr(),
            node_count: held.0.len(),
            edges: held.1.as_ptr(),
            edge_count: held.1.len(),
        };
        Box::into_raw(held)
    }
}
/// Releases a compiled snapshot.
///
/// # Safety
/// `snapshot` came from `cvr_ong_snapshot` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ong_snapshot_free(snapshot: *mut (Vec<Node>, Vec<Edge>)) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}
