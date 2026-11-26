//! The native execution context order (`include/Render/NativeContextOrder.hpp`): each graph node classified by its
//! exact native profile -- an empty join, a type-2 context publisher (scene driver vtable 0x3094850, work 0x332D5C,
//! its type leaf 0x1941AC0 returning 2), a synchronizer (vtable 0x36CED58, work 0x655FAC, mode <= 2, its leaf
//! 0x1942080 returning 3) or one of the audited zero-type nodes (their leaf 0xADF4E0 returning 0) -- and, over an
//! acyclic CPU phase with ranks rising along every edge, whether every publisher and synchronizer reaches on the CPU
//! each later-ranked synchronizer that consumes and clears its slot. 1EC404 publishes type-2 contexts only in execution
//! mode; 58DC98 consumes and clears EVERY slot through its phase-1 rank, even an as-yet unpublished slot, so a phase-1
//! DAG alone cannot prove these producers finished on the CPU. CPU readiness for native prefix consumption only; not
//! resource or alias safety.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `Classify` and `Analyze` forward to `cvr_co_*`; `Analyze`'s result is assembled
//! in C++ from what Rust lends back. `tools/single_pass_tests/context_order_test` runs its cases against them in its
//! Rust variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph_snapshot_abi::Read;
use crate::native::graph_snapshot::{field_u32, field_u64, Edge, Node};
use crate::paired_frontier::{acyclic, fingerprint, Edges};

/// context_order::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    None,
    /// A result's failure until the analysis gets further.
    #[default]
    Source,
    Classification,
    Edges,
    Ranks,
    CpuCycle,
    UnpublishedPrefix,
    Allocation,
}
/// context_order::Classification.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Classification {
    pub handle: u32,
    pub kind: u32,
    pub method_rva: u32,
}
/// context_order::Witness.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Witness {
    pub producer: u32,
    pub consumer: u32,
    pub producer_slot: u32,
    pub consumer_slot: u32,
    pub kind: u32,
}
/// context_order::Result.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub failure: Failure,
    pub fingerprint: u64,
    pub publishers: u32,
    pub synchronizers: u32,
    pub required_pairs: u32,
    pub proved_pairs: u32,
    pub classes: Vec<Classification>,
    pub missing: Vec<Witness>,
}

/// The audited zero-type nodes: (vtable, work).
const ZERO: [(u64, u64); 13] = [
    (0x30946F8, 0xCB1E88),
    (0x3094758, 0xBF436C),
    (0x36D10F0, 0xBFDD24),
    (0x36D10C0, 0xBF60F0),
    (0x36D05B0, 0xC28A18),
    (0x36D05E0, 0x1FF2B0),
    (0x36D0580, 0x4F4708),
    (0x36D0970, 0xC7A5C8),
    (0x36D0940, 0x5AEEC8),
    (0x36D0790, 0x425AA0),
    (0x36D1738, 0xD5BAC4),
    (0x36D13A8, 0xC0F204),
    (0x36D1D98, 0x77596C),
];

/// context_order::Classify: `out` gets the handle, and the kind and type leaf as far as the rules got.
pub fn classify(
    node: &Node,
    base: u64,
    read: Option<&mut dyn FnMut(u64, &mut [u8]) -> bool>,
    out: &mut Classification,
) -> bool {
    *out = Classification {
        handle: node.handle & 0xDFFF_FFFF,
        kind: 0,
        method_rva: 0,
    };
    if node.work == 0 && node.vtable == 0 {
        return field_u64(&node.bytes, 16) == 0 && node.definition.iter().all(|&byte| byte == 0);
    }
    let Some(read) = read else { return false };
    if base == 0
        || node.vtable < base
        || node.vtable - base > u32::MAX as u64
        || node.vtable > u64::MAX - 0x30
        || field_u64(&node.definition, 0) != node.vtable
    {
        return false;
    }
    let mut method = [0u8; 8];
    if !read(node.vtable + 0x28, &mut method) {
        return false;
    }
    let method = u64::from_le_bytes(method);
    if method < base || method - base > u32::MAX as u64 {
        return false;
    }
    let vtable = node.vtable - base;
    out.method_rva = (method - base) as u32;
    let mut bytes = [0u8; 6];
    if vtable == 0x3094850 && node.work == base + 0x332D5C && out.method_rva == 0x1941AC0 {
        out.kind = 2;
        return read(method, &mut bytes) && bytes == [0xB8, 2, 0, 0, 0, 0xC3];
    }
    if vtable == 0x36CED58 && node.work == base + 0x655FAC && out.method_rva == 0x1942080 {
        out.kind = 3;
        return field_u32(&node.definition, 24) <= 2
            && read(method, &mut bytes)
            && bytes == [0xB8, 3, 0, 0, 0, 0xC3];
    }
    let known = ZERO
        .iter()
        .any(|&(v, w)| vtable == v && node.work == base + w);
    known
        && out.method_rva == 0xADF4E0
        && read(method, &mut bytes[..3])
        && bytes[..3] == [0x33, 0xC0, 0xC3]
}

/// context_order::Analyze.
pub fn analyze(
    nodes_in: &[Node],
    edges_in: &[Edge],
    source_failure: u32,
    base: u64,
    read: Option<&mut dyn FnMut(u64, &mut [u8]) -> bool>,
) -> Analysis {
    let mut result = Analysis::default();
    let Some(read) = read else { return result };
    if source_failure != 0
        || nodes_in.is_empty()
        || nodes_in.len() > 220
        || edges_in.len() > 65536
        || base == 0
        || base > u64::MAX - 0x36D1CD8
    {
        return result;
    }
    let mut nodes: BTreeMap<u32, &Node> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut ranks: [BTreeSet<u8>; 2] = Default::default();
    for (i, node) in nodes_in.iter().enumerate() {
        let id = node.handle & 0xDFFF_FFFF;
        if id >= 65536
            || node.order as usize != i
            || field_u32(&node.bytes, 0) as usize != i
            || !ids.insert(id)
            || field_u32(&node.bytes, 24) > 1
        {
            return result;
        }
        for phase in 0..2 {
            let rank = node.bytes[28 + phase];
            if rank as usize >= nodes_in.len() || !ranks[phase].insert(rank) {
                result.failure = Failure::Ranks;
                return result;
            }
        }
        let mut classification = Classification::default();
        if !classify(node, base, Some(&mut *read), &mut classification) {
            result.failure = Failure::Classification;
            return result;
        }
        result.publishers += (classification.kind == 2) as u32;
        result.synchronizers += (classification.kind == 3) as u32;
        result.classes.push(classification);
        nodes.entry(id).or_insert(node);
    }
    let mut edges: [Edges; 2] = Default::default();
    let mut edge_ids = BTreeSet::new();
    result.failure = Failure::Edges;
    for (i, row) in edges_in.iter().enumerate() {
        let phase = field_u32(&row.bytes, 4);
        let from = field_u32(&row.bytes, 8) & 0xDFFF_FFFF;
        let to = field_u32(&row.bytes, 12) & 0xDFFF_FFFF;
        if row.order as usize != i
            || field_u32(&row.bytes, 0) as usize != i
            || phase > 1
            || !edge_ids.insert(row.handle & 0xBFFF_FFFF)
            || !nodes.contains_key(&from)
            || !nodes.contains_key(&to)
            || from == to
            || !edges[phase as usize].insert((from, to))
        {
            return result;
        }
        if nodes[&from].bytes[28 + phase as usize] >= nodes[&to].bytes[28 + phase as usize] {
            result.failure = Failure::Ranks;
            return result;
        }
    }
    if !acyclic(ids.iter().copied(), &edges[0]) {
        result.failure = Failure::CpuCycle;
        return result;
    }
    result.fingerprint = fingerprint(nodes_in, edges_in, source_failure, base);
    for producer in result.classes.clone() {
        if producer.kind != 2 && producer.kind != 3 {
            continue;
        }
        let slot = nodes[&producer.handle].bytes[29];
        let mut reachable = BTreeSet::from([producer.handle]);
        let mut pending = vec![producer.handle];
        let mut i = 0;
        while i < pending.len() {
            let current = pending[i];
            for &(_, next) in edges[0].range((current, 0)..=(current, u32::MAX)) {
                if reachable.insert(next) {
                    pending.push(next);
                }
            }
            i += 1;
        }
        for consumer in &result.classes.clone() {
            if consumer.kind != 3 {
                continue;
            }
            let end = nodes[&consumer.handle].bytes[29];
            if slot >= end {
                continue;
            }
            result.required_pairs += 1;
            if reachable.contains(&consumer.handle) {
                result.proved_pairs += 1;
            } else {
                result.missing.push(Witness {
                    producer: producer.handle,
                    consumer: consumer.handle,
                    producer_slot: slot as u32,
                    consumer_slot: end as u32,
                    kind: producer.kind,
                });
            }
        }
    }
    result.failure = if result.missing.is_empty() {
        Failure::None
    } else {
        Failure::UnpublishedPrefix
    };
    result
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
fn reader(read: Read) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}

/// context_order::Classify.
///
/// # Safety
/// `node` is valid; `out` is writable; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_co_classify(
    node: *const Node,
    base: u64,
    read: Option<Read>,
    out: *mut Classification,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        classify(
            &*node,
            base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            &mut *out,
        )
    }
}
/// context_order::Result, lent until `cvr_co_free`.
#[repr(C)]
pub struct RawAnalysis {
    pub failure: u32,
    pub publishers: u32,
    pub fingerprint: u64,
    pub synchronizers: u32,
    pub required_pairs: u32,
    pub proved_pairs: u32,
    pub classes: *const Classification,
    pub class_count: usize,
    pub missing: *const Witness,
    pub missing_count: usize,
}
const _: () = assert!(core::mem::size_of::<RawAnalysis>() == 64);
/// context_order::Analyze: the analysis (released by `cvr_co_free`), lent through `out`.
///
/// # Safety
/// The arrays hold their counts; `out` is writable; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_co_analyze(
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    source_failure: u32,
    base: u64,
    read: Option<Read>,
    out: *mut RawAnalysis,
) -> *mut Analysis {
    let mut adapter = read.map(reader);
    let analysis = unsafe {
        Box::new(analyze(
            slice(nodes, node_count),
            slice(edges, edge_count),
            source_failure,
            base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        ))
    };
    unsafe {
        *out = RawAnalysis {
            failure: analysis.failure as u32,
            publishers: analysis.publishers,
            fingerprint: analysis.fingerprint,
            synchronizers: analysis.synchronizers,
            required_pairs: analysis.required_pairs,
            proved_pairs: analysis.proved_pairs,
            classes: analysis.classes.as_ptr(),
            class_count: analysis.classes.len(),
            missing: analysis.missing.as_ptr(),
            missing_count: analysis.missing.len(),
        };
    }
    Box::into_raw(analysis)
}
/// Releases an analysis.
///
/// # Safety
/// `analysis` came from `cvr_co_analyze` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_co_free(analysis: *mut Analysis) {
    if !analysis.is_null() {
        drop(unsafe { Box::from_raw(analysis) });
    }
}
