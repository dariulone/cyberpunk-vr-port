//! The scene graph expansion (`include/Render/SceneGraphExpansion.hpp`, `scene_expansion::Plan`): exact graph topology
//! expansion only -- the selected GBuffer_Discard scene driver node (vtable 0x3094850, work 0x332D5C, view 0 or 1, a
//! 56-byte object) becomes its prefix section and a new tail node (the lowest free id) carrying the second section's
//! definition, the tail taking over the selected node's outgoing edges after one new edge from the prefix; the source's
//! edges and adjacency chains read exactly, the source compiling to its own ranks, both section definitions (8-aligned,
//! 56 bytes, the selected node's definition prefix, clear of every node object and of each other), the expansion
//! compiled by the owned native graph and contracting back to exactly the source edges. The caller separately owns
//! both section definitions and proves their child coverage, native consumers and lifetimes.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `Plan::Build` and `MatchesDeclaration` forward to `cvr_sx_*`; the plan's members
//! stay in C++, assembled from what Rust lends back. `tools/single_pass_tests/scene_prefix_order_plan_test` and
//! `context_order_test` run their cases against them in their Rust variants.

use std::collections::{BTreeMap, BTreeSet};

use crate::native::graph_snapshot::{field_u32, field_u64, Edge, Node};
use crate::owned_native_graph::OwnedGraph;
use crate::paired_frontier::{name, Edges};

pub const SCENE_DRIVER_VTABLE: u64 = 0x3094850;
pub const SCENE_DRIVER_WORK: u64 = 0x332D5C;

/// scene_expansion::Definition.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Definition {
    pub object: u64,
    pub prefix: [u8; 32],
}
/// scene_expansion::Mapping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub source_handle: u32,
    pub execution_handle: u32,
    pub section: u32,
}
const _: () =
    assert!(core::mem::size_of::<Definition>() == 40 && core::mem::size_of::<Mapping>() == 12);

/// scene_expansion::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Source,
    Identity,
    Definitions,
    Capacity,
    Topology,
    Compile,
    Contraction,
    Allocation,
}

/// A graph snapshot's parts.
#[derive(Clone, Copy)]
pub struct Graph<'a> {
    pub nodes: &'a [Node],
    pub edges: &'a [Edge],
    pub failure: u32,
}

/// A built expansion.
#[derive(Clone, Debug, Default)]
pub struct Expansion {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub failure: u32,
    pub mappings: Vec<Mapping>,
    pub expanded_edges: [Edges; 2],
    pub tail: u32,
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
/// Plan::Overlap: two byte ranges overlap (or one wraps).
pub fn overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    a > u64::MAX - a_bytes || b > u64::MAX - b_bytes || (a < b + b_bytes && b < a + a_bytes)
}
/// Plan::Append.
fn append(destination: &mut Vec<Node>, mut node: Node) {
    node.order = destination.len() as u32;
    node.bytes[0..4].copy_from_slice(&node.order.to_le_bytes());
    destination.push(node);
}
/// Plan::ReadEdges.
pub fn read_edges(edges_in: &[Edge], edges: &mut [Edges; 2]) -> bool {
    if edges_in.len() > 65534 {
        return false;
    }
    let mut seen = BTreeSet::new();
    for (i, edge) in edges_in.iter().enumerate() {
        let id = edge.handle & 0xBFFF_FFFF;
        let phase = field_u32(&edge.bytes, 4);
        if edge.order as usize != i
            || id >= 262144
            || (edge.handle != id && edge.handle != (id | 0x4000_0000))
            || !seen.insert(id)
            || field_u32(&edge.bytes, 0) != edge.order
            || phase > 1
        {
            return false;
        }
        let from = field_u32(&edge.bytes, 8);
        let to = field_u32(&edge.bytes, 12);
        if from != ((from & 0xDFFF_FFFF) | 0x2000_0000)
            || to != ((to & 0xDFFF_FFFF) | 0x2000_0000)
            || !edges[phase as usize].insert((from & 0xDFFF_FFFF, to & 0xDFFF_FFFF))
        {
            return false;
        }
    }
    true
}
/// Plan::Adjacency: every node's adjacency chains walk exactly the read edges.
pub fn adjacency(nodes: &[Node], edges_in: &[Edge], edges: &[Edges; 2]) -> bool {
    let mut by_id: BTreeMap<u32, &Edge> = BTreeMap::new();
    for edge in edges_in {
        by_id.entry(edge.handle & 0xBFFF_FFFF).or_insert(edge);
    }
    let mut visited: [BTreeSet<u32>; 4] = Default::default();
    for node in nodes {
        for phase in 0..2u32 {
            for direction in 0..2usize {
                let mut handle = field_u32(
                    &node.bytes,
                    if direction == 1 { 40 } else { 32 } + phase as usize * 4,
                );
                while handle != 0x8000_0000 {
                    let id = handle & 0xBFFF_FFFF;
                    let found = by_id.get(&id);
                    if handle != (id | 0x4000_0000)
                        || found.is_none()
                        || !visited[2 * phase as usize + direction].insert(id)
                    {
                        return false;
                    }
                    let edge = found.unwrap();
                    if field_u32(&edge.bytes, 4) != phase
                        || (field_u32(&edge.bytes, if direction == 1 { 8 } else { 12 })
                            & 0xDFFF_FFFF)
                            != (node.handle & 0xDFFF_FFFF)
                    {
                        return false;
                    }
                    handle = field_u32(&edge.bytes, if direction == 1 { 16 } else { 20 });
                }
            }
        }
    }
    (0..2).all(|phase| {
        visited[2 * phase] == visited[2 * phase + 1]
            && visited[2 * phase].len() == edges[phase].len()
    })
}

/// Plan::Build.
pub fn build(
    source: Graph,
    base: u64,
    selected_handle: u32,
    definitions: &[Definition; 2],
) -> Result<Expansion, Failure> {
    if base == 0
        || base > u64::MAX - SCENE_DRIVER_VTABLE
        || source.failure != 0
        || source.nodes.is_empty()
        || source.nodes.len() >= 220
    {
        return Err(Failure::Source);
    }
    let mut original_edges: [Edges; 2] = Default::default();
    if !read_edges(source.edges, &mut original_edges)
        || !adjacency(source.nodes, source.edges, &original_edges)
    {
        return Err(Failure::Source);
    }
    let mut original = OwnedGraph::default();
    if !original.create(
        source.nodes,
        source.edges,
        source.failure,
        original_edges.clone(),
    ) || !original.compile_reference()
        || !original.same_source_orders()
    {
        return Err(Failure::Source);
    }
    let Some(found) = source
        .nodes
        .iter()
        .find(|node| node.handle == selected_handle)
    else {
        return Err(Failure::Identity);
    };
    if found.vtable != base + SCENE_DRIVER_VTABLE
        || found.work != base + SCENE_DRIVER_WORK
        || field_u64(&found.definition, 0) != found.vtable
        || field_u64(&found.definition, 24) != name("GBuffer_Discard")
        || field_u32(&found.bytes, 24) > 1
    {
        return Err(Failure::Identity);
    }
    let source_object = field_u64(&found.bytes, 16);
    if source_object == 0 || source_object & 7 != 0 || source_object > u64::MAX - 56 {
        return Err(Failure::Identity);
    }
    for definition in definitions {
        if definition.object == 0
            || definition.object & 7 != 0
            || definition.object > u64::MAX - 56
            || definition.object == source_object
            || definition.prefix != found.definition
        {
            return Err(Failure::Definitions);
        }
        for node in source.nodes {
            let object = field_u64(&node.bytes, 16);
            let bytes = if node.vtable == base + SCENE_DRIVER_VTABLE
                && node.work == base + SCENE_DRIVER_WORK
            {
                56
            } else {
                32
            };
            if object != 0 && overlap(definition.object, 56, object, bytes) {
                return Err(Failure::Definitions);
            }
        }
    }
    if overlap(definitions[0].object, 56, definitions[1].object, 56) {
        return Err(Failure::Definitions);
    }
    let ids: BTreeSet<u32> = source
        .nodes
        .iter()
        .map(|node| node.handle & 0xDFFF_FFFF)
        .collect();
    let mut next = 0u32;
    while ids.contains(&next) {
        next += 1;
    }
    if next >= 65536 {
        return Err(Failure::Capacity);
    }
    let original_id = selected_handle & 0xDFFF_FFFF;
    let tail_handle = next | (selected_handle & 0x2000_0000);
    let mut expanded: Vec<Node> = Vec::with_capacity(source.nodes.len() + 1);
    let mut mappings = Vec::with_capacity(source.nodes.len() + 1);
    for node in source.nodes {
        let mut copy = *node;
        if node.handle == selected_handle {
            put64(&mut copy.bytes, 16, definitions[0].object);
            copy.definition = definitions[0].prefix;
            mappings.push(Mapping {
                source_handle: node.handle,
                execution_handle: copy.handle,
                section: 1,
            });
        } else {
            mappings.push(Mapping {
                source_handle: node.handle,
                execution_handle: copy.handle,
                section: 0,
            });
        }
        append(&mut expanded, copy);
    }
    let mut tail = *found;
    tail.handle = tail_handle;
    put64(&mut tail.bytes, 16, definitions[1].object);
    tail.definition = definitions[1].prefix;
    append(&mut expanded, tail);
    mappings.push(Mapping {
        source_handle: selected_handle,
        execution_handle: tail_handle,
        section: 2,
    });
    let mut expanded_edges: [Edges; 2] = Default::default();
    for phase in 0..2 {
        for &(from, to) in &original_edges[phase] {
            expanded_edges[phase].insert((if from == original_id { next } else { from }, to));
        }
        expanded_edges[phase].insert((original_id, next));
    }
    let mut compiled = OwnedGraph::default();
    if !compiled.create(&expanded, &[], 0, expanded_edges.clone()) || !compiled.compile_reference()
    {
        return Err(Failure::Compile);
    }
    let (nodes, edges, failure) = match compiled.snapshot() {
        Some((nodes, edges)) => (nodes, edges, 0),
        None => (Vec::new(), Vec::new(), 1),
    };
    let mut contracted: [Edges; 2] = Default::default();
    for phase in 0..2 {
        for &(from, to) in &expanded_edges[phase] {
            if from == original_id && to == next {
                continue;
            }
            contracted[phase].insert((
                if from == next { original_id } else { from },
                if to == next { original_id } else { to },
            ));
        }
    }
    if contracted != original_edges {
        return Err(Failure::Contraction);
    }
    Ok(Expansion {
        nodes,
        edges,
        failure,
        mappings,
        expanded_edges,
        tail: tail_handle,
    })
}

/// Plan::SameNodeStructure.
fn same_node_structure(current: &Node, frozen: &Node) -> bool {
    current.handle == frozen.handle
        && current.order == frozen.order
        && current.bytes == frozen.bytes
        && current.vtable == frozen.vtable
        && current.work == frozen.work
        && field_u64(&current.definition, 0) == field_u64(&frozen.definition, 0)
}

/// A ready plan's frozen members.
pub struct Frozen<'a> {
    pub source: Graph<'a>,
    pub expanded: Graph<'a>,
    pub mappings: &'a [Mapping],
    pub definitions: &'a [Definition; 2],
    pub selected: u32,
    pub base: u64,
}
/// Plan::MatchesDeclaration (of a ready plan).
pub fn matches_declaration(frozen: &Frozen, source: Graph, expanded: Graph, base: u64) -> bool {
    if base != frozen.base
        || source.failure != 0
        || expanded.failure != 0
        || source.nodes.len() != frozen.source.nodes.len()
        || expanded.nodes.len() != frozen.expanded.nodes.len()
        || source.edges != frozen.source.edges
        || expanded.edges != frozen.expanded.edges
        || frozen.mappings.len() != expanded.nodes.len()
    {
        return false;
    }
    let Some(selected) = frozen
        .source
        .nodes
        .iter()
        .find(|node| node.handle == frozen.selected)
    else {
        return false;
    };
    let selected_object = field_u64(&selected.bytes, 16);
    for (current, original) in source.nodes.iter().zip(frozen.source.nodes) {
        if !same_node_structure(current, original) {
            return false;
        }
        let object = field_u64(&current.bytes, 16);
        // Other-eye aliases of the selected object are the same native
        // definition and cannot carry an independent refreshed prefix.
        if (object == 0 || object == selected_object) && current.definition != original.definition {
            return false;
        }
    }
    for ((current, held), mapping) in expanded
        .nodes
        .iter()
        .zip(frozen.expanded.nodes)
        .zip(frozen.mappings)
    {
        if !same_node_structure(current, held) || mapping.execution_handle != current.handle {
            return false;
        }
        let Some(original) = source
            .nodes
            .iter()
            .find(|node| node.handle == mapping.source_handle)
        else {
            return false;
        };
        if mapping.section == 0 {
            if current.handle != original.handle
                || field_u64(&current.bytes, 16) != field_u64(&original.bytes, 16)
                || current.definition != original.definition
            {
                return false;
            }
        } else if mapping.section as usize > frozen.definitions.len()
            || mapping.source_handle != frozen.selected
            || current.definition != held.definition
            || current.definition != frozen.definitions[mapping.section as usize - 1].prefix
            || field_u64(&current.bytes, 16)
                != frozen.definitions[mapping.section as usize - 1].object
        {
            return false;
        }
    }
    true
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// A graph snapshot, lent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawGraph {
    pub nodes: *const Node,
    pub node_count: usize,
    pub edges: *const Edge,
    pub edge_count: usize,
    pub failure: u32,
}
unsafe fn graph<'a>(raw: &RawGraph) -> Graph<'a> {
    unsafe {
        Graph {
            nodes: if raw.nodes.is_null() || raw.node_count == 0 {
                &[]
            } else {
                core::slice::from_raw_parts(raw.nodes, raw.node_count)
            },
            edges: if raw.edges.is_null() || raw.edge_count == 0 {
                &[]
            } else {
                core::slice::from_raw_parts(raw.edges, raw.edge_count)
            },
            failure: raw.failure,
        }
    }
}
/// A built expansion, lent until `cvr_sx_free`.
#[repr(C)]
pub struct RawExpansion {
    pub expanded: RawGraph,
    pub mappings: *const Mapping,
    pub mapping_count: usize,
    pub edges: [*const [u32; 2]; 2],
    pub edge_counts: [usize; 2],
    pub tail: u32,
    pub failure: u32,
}
const _: () =
    assert!(core::mem::size_of::<RawGraph>() == 40 && core::mem::size_of::<RawExpansion>() == 96);

/// A built expansion with its flattened edges.
pub struct Held {
    expansion: Expansion,
    flat: [Vec<[u32; 2]>; 2],
}

/// Plan::Build: the expansion (released by `cvr_sx_free`) or null with the failure in `out`.
///
/// # Safety
/// `source` and `definitions` (2) are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sx_build(
    source: *const RawGraph,
    base: u64,
    selected_handle: u32,
    definitions: *const [Definition; 2],
    out: *mut RawExpansion,
) -> *mut Held {
    unsafe {
        let empty = RawGraph {
            nodes: core::ptr::null(),
            node_count: 0,
            edges: core::ptr::null(),
            edge_count: 0,
            failure: 0,
        };
        *out = RawExpansion {
            expanded: empty,
            mappings: core::ptr::null(),
            mapping_count: 0,
            edges: [core::ptr::null(); 2],
            edge_counts: [0; 2],
            tail: 0,
            failure: 0,
        };
        match build(graph(&*source), base, selected_handle, &*definitions) {
            Err(failure) => {
                (*out).failure = failure as u32;
                core::ptr::null_mut()
            }
            Ok(expansion) => {
                let flat = [0, 1].map(|phase| {
                    expansion.expanded_edges[phase]
                        .iter()
                        .map(|&(a, b)| [a, b])
                        .collect::<Vec<_>>()
                });
                let held = Box::new(Held { expansion, flat });
                let e = &held.expansion;
                *out = RawExpansion {
                    expanded: RawGraph {
                        nodes: e.nodes.as_ptr(),
                        node_count: e.nodes.len(),
                        edges: e.edges.as_ptr(),
                        edge_count: e.edges.len(),
                        failure: e.failure,
                    },
                    mappings: e.mappings.as_ptr(),
                    mapping_count: e.mappings.len(),
                    edges: [held.flat[0].as_ptr(), held.flat[1].as_ptr()],
                    edge_counts: [held.flat[0].len(), held.flat[1].len()],
                    tail: e.tail,
                    failure: 0,
                };
                Box::into_raw(held)
            }
        }
    }
}
/// Releases an expansion.
///
/// # Safety
/// `held` came from `cvr_sx_build` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sx_free(held: *mut Held) {
    if !held.is_null() {
        drop(unsafe { Box::from_raw(held) });
    }
}
/// Plan::MatchesDeclaration of a ready plan's frozen members.
///
/// # Safety
/// The graphs, `mappings` (`mapping_count`) and `definitions` (2) are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sx_matches_declaration(
    frozen_source: *const RawGraph,
    frozen_expanded: *const RawGraph,
    mappings: *const Mapping,
    mapping_count: usize,
    definitions: *const [Definition; 2],
    selected: u32,
    frozen_base: u64,
    source: *const RawGraph,
    expanded: *const RawGraph,
    base: u64,
) -> bool {
    unsafe {
        let frozen = Frozen {
            source: graph(&*frozen_source),
            expanded: graph(&*frozen_expanded),
            mappings: if mappings.is_null() || mapping_count == 0 {
                &[]
            } else {
                core::slice::from_raw_parts(mappings, mapping_count)
            },
            definitions: &*definitions,
            selected,
            base: frozen_base,
        };
        matches_declaration(&frozen, graph(&*source), graph(&*expanded), base)
    }
}

// ---- both eyes (include/Render/BothEyesSceneExpansion.hpp, BothEyesPlan) ----------------------------------------------
//
// Two source-bound expansions composed without moving either eye's tail before the other eye's prefix: the peer view's
// expansion first, then the main view's over it. In the Rust build BothEyesPlan's own rules -- the selection and the
// four definitions before the two builds, the composed mappings, the contracted edges and the per-phase topology
// after them, and MatchesDeclaration -- forward to `cvr_sxb_*`; the two plans stay its C++ members (each Rust's).
// `tools/single_pass_tests/both_eyes_scene_expansion_test` runs its cases against them in its Rust variant.

/// A plan's frozen members and its tail, lent.
#[repr(C)]
pub struct RawFrozen {
    pub source: RawGraph,
    pub expanded: RawGraph,
    pub mappings: *const Mapping,
    pub mapping_count: usize,
    pub definitions: *const [Definition; 2],
    pub selected: u32,
    pub tail: u32,
    pub base: u64,
    pub ready: bool,
}
const _: () = assert!(core::mem::size_of::<RawFrozen>() == 128);
unsafe fn frozen<'a>(raw: &'a RawFrozen) -> Frozen<'a> {
    unsafe {
        Frozen {
            source: graph(&raw.source),
            expanded: graph(&raw.expanded),
            mappings: if raw.mappings.is_null() || raw.mapping_count == 0 {
                &[]
            } else {
                core::slice::from_raw_parts(raw.mappings, raw.mapping_count)
            },
            definitions: &*raw.definitions,
            selected: raw.selected,
            base: raw.base,
        }
    }
}

/// BothEyesPlan::Build before the two expansions: the two selected GBuffer_Discard nodes (main, peer).
pub fn both_select(
    source: Graph,
    base: u64,
    definitions: &[[Definition; 2]; 2],
) -> Result<[Node; 2], Failure> {
    if base == 0
        || base > u64::MAX - SCENE_DRIVER_VTABLE
        || source.failure != 0
        || source.nodes.is_empty()
    {
        return Err(Failure::Source);
    }
    if source.nodes.len() > 218 {
        return Err(Failure::Capacity);
    }
    let mut selected: [Option<&Node>; 2] = [None, None];
    for node in source.nodes {
        if field_u64(&node.definition, 24) != name("GBuffer_Discard") {
            continue;
        }
        let view = field_u32(&node.bytes, 24);
        if view > 1 || selected[view as usize].is_some() {
            return Err(Failure::Identity);
        }
        selected[view as usize] = Some(node);
    }
    let (Some(main), Some(peer)) = (selected[0], selected[1]) else {
        return Err(Failure::Identity);
    };
    if field_u64(&main.bytes, 16) == field_u64(&peer.bytes, 16)
        && main.definition != peer.definition
    {
        return Err(Failure::Identity);
    }
    let chosen = [main, peer];
    for view in 0..2 {
        for section in 0..2 {
            let definition = &definitions[view][section];
            if definition.object == 0
                || definition.object & 7 != 0
                || definition.object > u64::MAX - 56
                || definition.prefix != chosen[view].definition
            {
                return Err(Failure::Definitions);
            }
            for node in source.nodes {
                let object = field_u64(&node.bytes, 16);
                let bytes = if node.vtable == base + SCENE_DRIVER_VTABLE
                    && node.work == base + SCENE_DRIVER_WORK
                {
                    56
                } else {
                    32
                };
                if object != 0 && overlap(definition.object, 56, object, bytes) {
                    return Err(Failure::Definitions);
                }
            }
            for other in 0..view * 2 + section {
                if overlap(
                    definition.object,
                    56,
                    definitions[other / 2][other % 2].object,
                    56,
                ) {
                    return Err(Failure::Definitions);
                }
            }
        }
    }
    Ok([*main, *peer])
}

fn find(nodes: &[Node], handle: u32) -> Option<&Node> {
    nodes.iter().find(|node| node.handle == handle)
}

/// BothEyesPlan::ContractedEdgesMatch.
fn contracted_edges_match(
    source: Graph,
    mappings: &[Mapping],
    main_edges: [&[[u32; 2]]; 2],
    internal_links: [(u32, u32); 2],
) -> bool {
    let mut inverse = BTreeMap::new();
    for mapping in mappings {
        if inverse
            .insert(
                mapping.execution_handle & 0xDFFF_FFFF,
                mapping.source_handle & 0xDFFF_FFFF,
            )
            .is_some()
        {
            return false;
        }
    }
    let mut original: [Edges; 2] = Default::default();
    for edge in source.edges {
        let phase = field_u32(&edge.bytes, 4);
        if phase > 1
            || !original[phase as usize].insert((
                field_u32(&edge.bytes, 8) & 0xDFFF_FFFF,
                field_u32(&edge.bytes, 12) & 0xDFFF_FFFF,
            ))
        {
            return false;
        }
    }
    for phase in 0..2 {
        let mut contracted = Edges::new();
        let mut internal = 0;
        for link in main_edges[phase] {
            let (from, to) = (link[0], link[1]);
            if internal_links.contains(&(from, to)) {
                internal += 1;
                continue;
            }
            let (Some(&a), Some(&b)) = (inverse.get(&from), inverse.get(&to)) else {
                return false;
            };
            if a == b || !contracted.insert((a, b)) {
                return false;
            }
        }
        if internal != 2 || contracted != original[phase] {
            return false;
        }
    }
    true
}

/// BothEyesPlan::Build after the two expansions: the failure, `mappings` getting the composed mappings (as far as the
/// composition got on a refusal there, as in C++).
pub fn both_compose(
    source: Graph,
    selected: &[Node; 2],
    peer: &Frozen,
    peer_tail: u32,
    main: &Frozen,
    main_tail: u32,
    main_edges: [&[[u32; 2]]; 2],
    mappings: &mut Vec<Mapping>,
) -> Failure {
    for second in main.mappings {
        let Some(first) = peer
            .mappings
            .iter()
            .find(|mapping| mapping.execution_handle == second.source_handle)
        else {
            return Failure::Contraction;
        };
        if first.section != 0 && second.section != 0 {
            return Failure::Contraction;
        }
        mappings.push(Mapping {
            source_handle: first.source_handle,
            execution_handle: second.execution_handle,
            section: if second.section != 0 {
                second.section
            } else {
                first.section
            },
        });
    }
    let mask = 0xDFFF_FFFF;
    let internal = [
        (peer.selected & mask, peer_tail & mask),
        (main.selected & mask, main_tail & mask),
    ];
    if mappings.len() != source.nodes.len() + 2
        || !contracted_edges_match(source, mappings, main_edges, internal)
    {
        return Failure::Contraction;
    }
    for phase in 0..2 {
        let expanded = main.expanded.nodes;
        let (Some(peer_prefix), Some(peer_end), Some(main_prefix), Some(main_end)) = (
            find(expanded, peer.selected),
            find(expanded, peer_tail),
            find(expanded, main.selected),
            find(expanded, main_tail),
        ) else {
            return Failure::Topology;
        };
        let rank = |node: &Node| node.bytes[28 + phase];
        if rank(&selected[1]) >= rank(&selected[0])
            || rank(peer_prefix) >= rank(peer_end)
            || rank(peer_end) >= rank(main_prefix)
            || rank(main_prefix) >= rank(main_end)
        {
            return Failure::Topology;
        }
        let chain = [peer.selected, peer_tail, main.selected, main_tail];
        for pair in chain.windows(2) {
            if crate::paired_frontier::path(main_edges[phase], pair[0] & mask, pair[1] & mask)
                .is_empty()
            {
                return Failure::Topology;
            }
        }
    }
    Failure::None
}

/// BothEyesPlan::MatchesDeclaration (of a ready plan built at `both_base`).
pub fn both_matches_declaration(
    peer: &Frozen,
    main: &Frozen,
    both_base: u64,
    source: Graph,
    expanded: Graph,
    base: u64,
) -> bool {
    if base != both_base || source.failure != 0 || source.nodes.len() != peer.source.nodes.len() {
        return false;
    }
    let mut intermediate = peer.expanded.nodes.to_vec();
    if intermediate.len() != peer.mappings.len() {
        return false;
    }
    for (node, mapping) in intermediate.iter_mut().zip(peer.mappings) {
        if mapping.section != 0 {
            continue;
        }
        let Some(original) = find(source.nodes, mapping.source_handle) else {
            return false;
        };
        node.definition = original.definition;
    }
    let intermediate = Graph {
        nodes: &intermediate,
        edges: peer.expanded.edges,
        failure: peer.expanded.failure,
    };
    matches_declaration(peer, source, intermediate, base)
        && matches_declaration(main, intermediate, expanded, base)
}

/// BothEyesPlan::Build before the two expansions: the failure, the selected (main, peer) nodes through `selected`.
///
/// # Safety
/// `source` and `definitions` are valid; `selected` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sxb_select(
    source: *const RawGraph,
    base: u64,
    definitions: *const [[Definition; 2]; 2],
    selected: *mut [Node; 2],
) -> u32 {
    unsafe {
        match both_select(graph(&*source), base, &*definitions) {
            Ok(nodes) => {
                *selected = nodes;
                Failure::None as u32
            }
            Err(failure) => failure as u32,
        }
    }
}
/// BothEyesPlan::Build after the two expansions: the failure, and the composed mappings (as far as they got; released
/// by `cvr_sxb_free`) through `held` / `mappings` / `count`.
///
/// # Safety
/// The pointers are valid; the main edges hold their counts; the outputs are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sxb_compose(
    source: *const RawGraph,
    selected: *const [Node; 2],
    peer: *const RawFrozen,
    main: *const RawFrozen,
    main_cpu: *const [u32; 2],
    main_cpu_count: usize,
    main_order: *const [u32; 2],
    main_order_count: usize,
    held: *mut *mut Vec<Mapping>,
    mappings: *mut *const Mapping,
    count: *mut usize,
) -> u32 {
    unsafe {
        let edges = |data: *const [u32; 2], len: usize| {
            if data.is_null() || len == 0 {
                &[][..]
            } else {
                core::slice::from_raw_parts(data, len)
            }
        };
        let (peer, main) = (&*peer, &*main);
        let mut composed = Box::new(Vec::new());
        let failure = both_compose(
            graph(&*source),
            &*selected,
            &frozen(peer),
            peer.tail,
            &frozen(main),
            main.tail,
            [
                edges(main_cpu, main_cpu_count),
                edges(main_order, main_order_count),
            ],
            &mut composed,
        );
        *mappings = composed.as_ptr();
        *count = composed.len();
        *held = Box::into_raw(composed);
        failure as u32
    }
}
/// Releases composed mappings.
///
/// # Safety
/// `held` came from `cvr_sxb_compose` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sxb_free(held: *mut Vec<Mapping>) {
    if !held.is_null() {
        drop(unsafe { Box::from_raw(held) });
    }
}
/// BothEyesPlan::MatchesDeclaration of a ready plan.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sxb_matches_declaration(
    peer: *const RawFrozen,
    main: *const RawFrozen,
    both_base: u64,
    source: *const RawGraph,
    expanded: *const RawGraph,
    base: u64,
) -> bool {
    unsafe {
        let (peer, main) = (&*peer, &*main);
        peer.ready
            && main.ready
            && both_matches_declaration(
                &frozen(peer),
                &frozen(main),
                both_base,
                graph(&*source),
                graph(&*expanded),
                base,
            )
    }
}
