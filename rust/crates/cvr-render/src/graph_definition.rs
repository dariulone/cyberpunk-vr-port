//! The graph definition source receipt (`include/Render/NativeGraphDefinitionSource.hpp`): a read-only mapping of a
//! composite graph's nodes to the definitions its per-view owners hold -- not a lease, native-exclusive section,
//! generation monitor or move. The composite graph sits in one of the cache's four 0x90-byte slots; the graph is
//! captured again and equal to the source (its definition prefixes too under ImmutablePrefix); the slot's use counter
//! and key, the owner vector at +0x70 (1..128 per-view graphs, distinct, 0x88 bytes each, clear of the cache, the
//! pointer array and each other) and its allocator; each owner's move plan (its definitions within the 8192 budget, an
//! owned allocation clear of everything else, every definition distinct and clear of every region); every node owned
//! by exactly one definition (or an empty join); then everything re-read and the graph captured a final time. The
//! whole read is taken twice and must agree (the use counter only under StableCounter). Equal bytes cannot exclude
//! allocator ABA; runtime mutation hooks supply that proof.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `SourceReceipt` owns a Rust receipt: `Capture` and `MatchesBefore` forward to
//! `cvr_gd_*`, the accessors read the members C++ keeps, assembled from what Rust lends back (each owner's move plan
//! restored from its fields). `tools/single_pass_tests/definition_source_test` runs its cases against them in its Rust
//! variant.

use std::collections::{BTreeMap, BTreeSet};

use crate::definition_owner_abi::PlanFields;
use crate::graph_snapshot_abi::Read as ReadMemory;
use crate::native::definition_owner::{Failure as OwnerFailure, MovePlan, NativeVector};
use crate::native::graph_snapshot::{field_u32, field_u64, Edge, Node, Snapshot as Graph};
use crate::paired_frontier::fingerprint;

pub const CACHE_SLOTS: u64 = 4;
pub const CACHE_SLOT_BYTES: u64 = 0x90;
pub const MAX_OWNERS: u32 = 128;
pub const MAX_DEFINITIONS: u32 = 8192;

/// graph_definition::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Source,
    CacheSlot,
    OwnerVector,
    OwnerBounds,
    Owners,
    OwnerPlan,
    AllocationOverlap,
    DefinitionBudget,
    DuplicateDefinition,
    UnownedNode,
    Changed,
    Allocation,
}
/// graph_definition::CacheUsePolicy.
pub const STABLE_COUNTER: u32 = 0;
pub const ALLOW_CONCURRENT_HITS: u32 = 1;
/// graph_definition::DefinitionContentPolicy.
pub const IMMUTABLE_PREFIX: u32 = 0;
pub const OWNER_MEMBERSHIP: u32 = 1;

/// graph_definition::ChangedDetail.
pub mod changed {
    pub const CACHE_USE: u32 = 1 << 0;
    pub const CACHE_KEY: u32 = 1 << 1;
    pub const OWNER_VECTOR: u32 = 1 << 2;
    pub const OWNER_POINTERS: u32 = 1 << 3;
    pub const ALLOCATOR: u32 = 1 << 4;
    pub const OWNER_HEADER: u32 = 1 << 5;
    pub const ENTRIES: u32 = 1 << 6;
    pub const NODE_RAW: u32 = 1 << 7;
    pub const EDGE_RAW: u32 = 1 << 8;
    pub const VTABLE_WORK: u32 = 1 << 9;
    pub const DEFINITION_PREFIX: u32 = 1 << 10;
    pub const MAPPINGS: u32 = 1 << 11;
    pub const DOUBLE_SNAPSHOT: u32 = 1 << 12;
    pub const READ: u32 = 1 << 13;
    pub const GRAPH_CAPTURE: u32 = 1 << 14;
    pub const OWNER_CAPTURE: u32 = 1 << 15;
    pub const CACHE_SLOT: u32 = 1 << 16;
    pub const FINGERPRINT: u32 = 1 << 17;
}

/// graph_definition::Owner.
#[derive(Clone, Debug, Default)]
pub struct Owner {
    pub per_view_graph: u64,
    pub plan: MovePlan,
}
/// graph_definition::Mapping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub node_handle: u32,
    pub owner_index: u32,
    pub definition_index: u32,
    pub object: u64,
}
const _: () = assert!(core::mem::size_of::<Mapping>() == 24);

/// SourceReceipt::Snapshot.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub last_use: u32,
    pub definition_slots: u32,
    pub key: u64,
    pub allocator: u64,
    pub owner_vector: NativeVector,
    pub owners: Vec<Owner>,
    pub mappings: Vec<Mapping>,
}

/// The source graph a receipt is bound to.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub failure: u32,
}

#[derive(Clone, Copy)]
struct Range {
    begin: u64,
    end: u64,
}
fn overlap(a: Range, b: Range) -> bool {
    a.begin < b.end && b.begin < a.end
}
fn pointer(value: u64) -> bool {
    value != 0 && value & 7 == 0
}
type Reader<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;
fn read_u32(read: Reader<'_>, at: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    read(at, &mut bytes).then(|| u32::from_le_bytes(bytes))
}
fn read_u64(read: Reader<'_>, at: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    read(at, &mut bytes).then(|| u64::from_le_bytes(bytes))
}
fn read_vector(read: Reader<'_>, at: u64) -> Option<NativeVector> {
    let mut b = [0u8; 16];
    read(at, &mut b).then(|| NativeVector {
        data: u64::from_le_bytes(b[0..8].try_into().unwrap()),
        capacity: u32::from_le_bytes(b[8..12].try_into().unwrap()),
        count: u32::from_le_bytes(b[12..16].try_into().unwrap()),
    })
}
fn read_pointers(read: Reader<'_>, at: u64, count: u32) -> Option<Vec<u64>> {
    let mut bytes = vec![0u8; count as usize * 8];
    read(at, &mut bytes).then(|| {
        bytes
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    })
}

/// SourceReceipt::OwnerDifference.
fn owner_difference(a: &MovePlan, b: &MovePlan) -> u32 {
    let mut detail = 0;
    if a.saved_header() != b.saved_header() || a.owner_address() != b.owner_address() {
        detail |= changed::OWNER_HEADER;
    }
    if a.empty_header() != b.empty_header() || a.trailer_address() != b.trailer_address() {
        detail |= changed::ALLOCATOR;
    }
    if a.definitions() != b.definitions() {
        detail |= changed::ENTRIES;
    }
    detail
}
/// SourceReceipt::OwnerFailure.
fn owner_failure(failure: OwnerFailure) -> u32 {
    let mut detail = changed::OWNER_CAPTURE;
    match failure {
        OwnerFailure::Header | OwnerFailure::Bounds => detail |= changed::OWNER_HEADER,
        OwnerFailure::Entries => detail |= changed::ENTRIES,
        OwnerFailure::Allocator => detail |= changed::ALLOCATOR,
        OwnerFailure::Changed => detail |= changed::DOUBLE_SNAPSHOT,
        _ => {}
    }
    detail
}
/// SourceReceipt::Difference.
fn difference(a: &Snapshot, b: &Snapshot, policy: u32) -> u32 {
    let mut detail = 0;
    if policy == STABLE_COUNTER && a.last_use != b.last_use {
        detail |= changed::CACHE_USE;
    }
    if a.key != b.key {
        detail |= changed::CACHE_KEY;
    }
    if a.allocator != b.allocator {
        detail |= changed::ALLOCATOR;
    }
    if a.owner_vector != b.owner_vector {
        detail |= changed::OWNER_VECTOR;
    }
    if a.definition_slots != b.definition_slots {
        detail |= changed::ENTRIES;
    }
    if a.mappings != b.mappings {
        detail |= changed::MAPPINGS;
    }
    if a.owners.len() != b.owners.len() {
        detail |= changed::OWNER_POINTERS;
    }
    for (x, y) in a.owners.iter().zip(&b.owners) {
        if x.per_view_graph != y.per_view_graph {
            detail |= changed::OWNER_POINTERS;
        }
        detail |= owner_difference(&x.plan, &y.plan);
    }
    detail
}
/// SourceReceipt::GraphDifference.
fn graph_difference(a: &Graph, b: &Source, policy: u32) -> u32 {
    let mut detail = 0;
    if a.nodes.len() != b.nodes.len() {
        detail |= changed::NODE_RAW;
    }
    if a.edges.len() != b.edges.len() {
        detail |= changed::EDGE_RAW;
    }
    for (x, y) in a.nodes.iter().zip(&b.nodes) {
        if x.handle != y.handle || x.order != y.order || x.bytes != y.bytes {
            detail |= changed::NODE_RAW;
        }
        if x.vtable != y.vtable || x.work != y.work {
            detail |= changed::VTABLE_WORK;
        }
        if policy == IMMUTABLE_PREFIX && x.definition != y.definition {
            detail |= changed::DEFINITION_PREFIX;
        }
    }
    for (x, y) in a.edges.iter().zip(&b.edges) {
        if x != y {
            detail |= changed::EDGE_RAW;
        }
    }
    detail
}

/// graph_definition::SourceReceipt.
#[derive(Clone, Debug, Default)]
pub struct Receipt {
    pub snapshot: Snapshot,
    pub source: Option<Source>,
    pub cache_base: u64,
    pub graph: u64,
    pub slot: u64,
    pub base: u64,
    pub fingerprint: u64,
    pub ready: bool,
    pub failure: Failure,
    pub changed_details: u32,
    pub policy: u32,
    pub content_policy: u32,
}

impl Receipt {
    fn changed(&mut self, detail: u32) -> bool {
        self.changed_details = detail;
        false
    }

    /// SourceReceipt::Capture.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        &mut self,
        cache_base: u64,
        composite_graph: u64,
        source: &Source,
        image_base: u64,
        read: Option<Reader<'_>>,
        policy: u32,
        content_policy: u32,
    ) -> bool {
        *self = Receipt {
            failure: Failure::Arguments,
            ..Default::default()
        };
        let Some(read) = read else { return false };
        if !pointer(cache_base)
            || !pointer(composite_graph)
            || image_base == 0
            || (policy != STABLE_COUNTER && policy != ALLOW_CONCURRENT_HITS)
            || (content_policy != IMMUTABLE_PREFIX && content_policy != OWNER_MEMBERSHIP)
            || cache_base > u64::MAX - CACHE_SLOTS * CACHE_SLOT_BYTES
            || composite_graph > u64::MAX - 0x80
        {
            return false;
        }
        self.failure = Failure::Source;
        if source.failure != 0
            || source.nodes.is_empty()
            || source.nodes.len() > 8192
            || source.edges.len() > 65536
        {
            return false;
        }
        self.cache_base = cache_base;
        self.graph = composite_graph;
        self.base = image_base;
        self.policy = policy;
        self.content_policy = content_policy;
        self.failure = Failure::CacheSlot;
        if composite_graph < cache_base + 0x10 {
            return false;
        }
        let displacement = composite_graph - cache_base - 0x10;
        if displacement % CACHE_SLOT_BYTES != 0 || displacement / CACHE_SLOT_BYTES >= CACHE_SLOTS {
            return false;
        }
        self.slot = cache_base + displacement;
        let (mut first, mut second) = (Snapshot::default(), Snapshot::default());
        if !self.read_once(read, source, &mut first) || !self.read_once(read, source, &mut second) {
            return false;
        }
        self.failure = Failure::Changed;
        let detail = difference(&first, &second, self.policy);
        if detail != 0 {
            return self.changed(detail | changed::DOUBLE_SNAPSHOT);
        }
        self.snapshot = first;
        self.source = Some(source.clone());
        self.fingerprint = fingerprint(&source.nodes, &source.edges, source.failure, image_base);
        self.ready = true;
        self.failure = Failure::None;
        true
    }

    /// SourceReceipt::MatchesBefore: the receipt captured again and equal; `details` gets what differed.
    pub fn matches_before(&self, read: Option<Reader<'_>>, details: &mut u32) -> bool {
        *details = 0;
        let Some(source) = self.source.as_ref().filter(|_| self.ready) else {
            return false;
        };
        let mut current = Receipt::default();
        if !current.capture(
            self.cache_base,
            self.graph,
            source,
            self.base,
            read,
            self.policy,
            self.content_policy,
        ) {
            *details = current.changed_details;
            return false;
        }
        let mut detail = difference(&current.snapshot, &self.snapshot, self.policy);
        if current.slot != self.slot {
            detail |= changed::CACHE_SLOT;
        }
        if current.fingerprint != self.fingerprint {
            detail |= changed::FINGERPRINT;
        }
        *details = detail;
        detail == 0
    }

    /// SourceReceipt::ReadOnce.
    fn read_once(&mut self, read: Reader<'_>, expected: &Source, out: &mut Snapshot) -> bool {
        *out = Snapshot::default();
        self.failure = Failure::Source;
        let mut current = Graph::default();
        if !current.capture(self.graph, read) {
            return self.changed(changed::GRAPH_CAPTURE);
        }
        let detail = graph_difference(&current, expected, self.content_policy);
        if detail != 0 {
            return self.changed(detail);
        }
        self.failure = Failure::OwnerVector;
        let Some(last_use) = read_u32(read, self.slot) else {
            return false;
        };
        out.last_use = last_use;
        let Some(key) = read_u64(read, self.slot + 8) else {
            return false;
        };
        out.key = key;
        let Some(owner_vector) = read_vector(read, self.graph + 0x70) else {
            return false;
        };
        out.owner_vector = owner_vector;
        let h = out.owner_vector;
        self.failure = Failure::OwnerBounds;
        if !pointer(h.data)
            || h.count == 0
            || h.count > h.capacity
            || h.capacity > MAX_OWNERS
            || h.data > u64::MAX - h.capacity as u64 * 8 - 8
        {
            return false;
        }
        let cache = Range {
            begin: self.cache_base,
            end: self.cache_base + CACHE_SLOTS * CACHE_SLOT_BYTES,
        };
        let pointers = Range {
            begin: h.data,
            end: h.data + h.capacity as u64 * 8 + 8,
        };
        if overlap(cache, pointers) {
            return false;
        }
        let Some(allocator) = read_u64(read, pointers.end - 8) else {
            return false;
        };
        out.allocator = allocator;
        if !pointer(out.allocator)
            || (out.allocator >= pointers.begin && out.allocator < pointers.end)
        {
            return false;
        }
        let Some(graphs) = read_pointers(read, h.data, h.count) else {
            return false;
        };
        let mut unique = BTreeSet::new();
        let mut graph_ranges: Vec<Range> = Vec::new();
        self.failure = Failure::Owners;
        for &graph in &graphs {
            if !pointer(graph) || graph > u64::MAX - 0x88 || !unique.insert(graph) {
                return false;
            }
            let region = Range {
                begin: graph,
                end: graph + 0x88,
            };
            if overlap(region, cache)
                || overlap(region, pointers)
                || graph_ranges
                    .iter()
                    .any(|&previous| overlap(region, previous))
            {
                return false;
            }
            graph_ranges.push(region);
        }
        let mut allocations: Vec<Range> = Vec::new();
        let mut definitions: BTreeMap<u64, (u32, u32)> = BTreeMap::new();
        for (i, &graph) in graphs.iter().enumerate() {
            let mut owner = Owner {
                per_view_graph: graph,
                plan: MovePlan::default(),
            };
            self.failure = Failure::OwnerPlan;
            if !owner.plan.capture(owner.per_view_graph, Some(&mut *read)) {
                return self.changed(owner_failure(owner.plan.rejection()));
            }
            self.failure = Failure::DefinitionBudget;
            let count = owner.plan.definitions().len();
            if count > (MAX_DEFINITIONS - out.definition_slots) as usize {
                return false;
            }
            out.definition_slots += count as u32;
            if owner.plan.owns_allocation() {
                let region = Range {
                    begin: owner.plan.saved_header().data,
                    end: owner.plan.trailer_address() + 8,
                };
                self.failure = Failure::AllocationOverlap;
                if overlap(region, cache)
                    || overlap(region, pointers)
                    || graph_ranges.iter().any(|&object| overlap(region, object))
                    || allocations
                        .iter()
                        .any(|&previous| overlap(region, previous))
                {
                    return false;
                }
                allocations.push(region);
            }
            self.failure = Failure::DuplicateDefinition;
            for (j, &definition) in owner.plan.definitions().iter().enumerate() {
                if definition != 0 {
                    if definitions.contains_key(&definition) {
                        return false;
                    }
                    definitions.insert(definition, (i as u32, j as u32));
                }
            }
            out.owners.push(owner);
        }
        self.failure = Failure::AllocationOverlap;
        for &object in definitions.keys() {
            let point = Range {
                begin: object,
                end: object + 1,
            };
            if overlap(point, cache)
                || overlap(point, pointers)
                || graph_ranges.iter().any(|&region| overlap(point, region))
                || allocations.iter().any(|&region| overlap(point, region))
            {
                return false;
            }
        }
        self.failure = Failure::UnownedNode;
        for node in &expected.nodes {
            let object = field_u64(&node.bytes, 16);
            if object == 0 {
                if node.work != 0 || node.vtable != 0 || field_u32(&node.bytes, 8) != 4 {
                    return false;
                }
                continue;
            }
            let Some(&(owner_index, definition_index)) = definitions.get(&object) else {
                return false;
            };
            out.mappings.push(Mapping {
                node_handle: node.handle,
                owner_index,
                definition_index,
                object,
            });
        }
        self.failure = Failure::Changed;
        let Some(last_use) = read_u32(read, self.slot) else {
            return self.changed(changed::READ | changed::CACHE_USE);
        };
        if self.policy == STABLE_COUNTER && last_use != out.last_use {
            return self.changed(changed::CACHE_USE);
        }
        let Some(key) = read_u64(read, self.slot + 8) else {
            return self.changed(changed::READ | changed::CACHE_KEY);
        };
        if key != out.key {
            return self.changed(changed::CACHE_KEY);
        }
        let Some(owner_vector) = read_vector(read, self.graph + 0x70) else {
            return self.changed(changed::READ | changed::OWNER_VECTOR);
        };
        if owner_vector != h {
            return self.changed(changed::OWNER_VECTOR);
        }
        let Some(graphs_after) = read_pointers(read, h.data, h.count) else {
            return self.changed(changed::READ | changed::OWNER_POINTERS);
        };
        if graphs_after != graphs {
            return self.changed(changed::OWNER_POINTERS);
        }
        let Some(allocator) = read_u64(read, pointers.end - 8) else {
            return self.changed(changed::READ | changed::ALLOCATOR);
        };
        if allocator != out.allocator {
            return self.changed(changed::ALLOCATOR);
        }
        for owner in &out.owners {
            let mut after = MovePlan::default();
            if !after.capture(owner.plan.owner_address() - 0x70, Some(&mut *read)) {
                return self.changed(owner_failure(after.rejection()));
            }
            let detail = owner_difference(&after, &owner.plan);
            if detail != 0 {
                return self.changed(detail);
            }
        }
        let mut after = Graph::default();
        if !after.capture(self.graph, read) {
            return self.changed(changed::GRAPH_CAPTURE);
        }
        let detail = graph_difference(&after, expected, self.content_policy);
        if detail != 0 {
            return self.changed(detail);
        }
        true
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// An owner, lent: its per-view graph and its move plan's fields.
#[repr(C)]
pub struct RawOwner {
    pub per_view_graph: u64,
    pub plan: PlanFields,
}
const _: () = assert!(core::mem::size_of::<RawOwner>() == 80);
/// A receipt's members, lent until `cvr_gd_free` (the owners and mappings only on a ready receipt).
#[repr(C)]
pub struct RawReceipt {
    pub last_use: u32,
    pub definition_slots: u32,
    pub key: u64,
    pub allocator: u64,
    pub owner_vector: NativeVector,
    pub owners: *const RawOwner,
    pub owner_count: usize,
    pub mappings: *const Mapping,
    pub mapping_count: usize,
    pub cache_base: u64,
    pub graph: u64,
    pub slot: u64,
    pub base: u64,
    pub fingerprint: u64,
    pub failure: u32,
    pub changed_details: u32,
    pub policy: u32,
    pub content_policy: u32,
    pub ready: bool,
}
const _: () = assert!(core::mem::size_of::<RawReceipt>() == 136);

/// A ready receipt with its lent owners.
pub struct Held {
    receipt: Receipt,
    owners: Vec<RawOwner>,
}

unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
fn reader(read: ReadMemory) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}

/// SourceReceipt::Capture: the ready receipt (released by `cvr_gd_free`) or null, its members through `out` either way.
///
/// # Safety
/// The source arrays hold their counts; `out` is writable; `read` follows definition_owner::ReadMemory.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_gd_capture(
    cache_base: u64,
    composite_graph: u64,
    nodes: *const Node,
    node_count: usize,
    edges: *const Edge,
    edge_count: usize,
    source_failure: u32,
    image_base: u64,
    read: Option<ReadMemory>,
    policy: u32,
    content_policy: u32,
    out: *mut RawReceipt,
) -> *mut Held {
    let source = unsafe {
        Source {
            nodes: slice(nodes, node_count).to_vec(),
            edges: slice(edges, edge_count).to_vec(),
            failure: source_failure,
        }
    };
    let mut adapter = read.map(reader);
    let mut receipt = Receipt::default();
    let ready = receipt.capture(
        cache_base,
        composite_graph,
        &source,
        image_base,
        adapter
            .as_mut()
            .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        policy,
        content_policy,
    );
    let mut held = Box::new(Held {
        receipt,
        owners: Vec::new(),
    });
    let owners: Vec<RawOwner> = held
        .receipt
        .snapshot
        .owners
        .iter()
        .map(|owner| RawOwner {
            per_view_graph: owner.per_view_graph,
            plan: PlanFields {
                address: owner.plan.owner_address(),
                trailer: owner.plan.trailer_address(),
                saved: owner.plan.saved_header(),
                empty: owner.plan.empty_header(),
                definitions: owner.plan.definitions().as_ptr(),
                count: owner.plan.definitions().len(),
                failure: owner.plan.rejection() as u32,
                ready: owner.plan.ready(),
            },
        })
        .collect();
    held.owners = owners;
    let r = &held.receipt;
    let s = &r.snapshot;
    unsafe {
        *out = RawReceipt {
            last_use: s.last_use,
            definition_slots: s.definition_slots,
            key: s.key,
            allocator: s.allocator,
            owner_vector: s.owner_vector,
            owners: held.owners.as_ptr(),
            owner_count: held.owners.len(),
            mappings: s.mappings.as_ptr(),
            mapping_count: s.mappings.len(),
            cache_base: r.cache_base,
            graph: r.graph,
            slot: r.slot,
            base: r.base,
            fingerprint: r.fingerprint,
            failure: r.failure as u32,
            changed_details: r.changed_details,
            policy: r.policy,
            content_policy: r.content_policy,
            ready,
        };
    }
    if ready {
        Box::into_raw(held)
    } else {
        unsafe {
            (*out).owners = core::ptr::null();
            (*out).owner_count = 0;
            (*out).mappings = core::ptr::null();
            (*out).mapping_count = 0;
        }
        core::ptr::null_mut()
    }
}
/// SourceReceipt::MatchesBefore of a ready receipt; `details` gets what differed.
///
/// # Safety
/// `held` came from `cvr_gd_capture`; `details` is writable; `read` follows definition_owner::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gd_matches(
    held: *const Held,
    read: Option<ReadMemory>,
    details: *mut u32,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        (*held).receipt.matches_before(
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            &mut *details,
        )
    }
}
/// Releases a receipt.
///
/// # Safety
/// `held` came from `cvr_gd_capture` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gd_free(held: *mut Held) {
    if !held.is_null() {
        drop(unsafe { Box::from_raw(held) });
    }
}
