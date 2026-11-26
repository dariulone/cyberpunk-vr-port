//! A render-elements node's native stage definition bytes (include/Render/NativeStageDefinition.hpp).
//!
//! The graph node and the definition tree node must agree: the graph's exact shape (handle, order, view flag, object,
//! vtable, work and the first definition word), the tree root of that object, and a tree path from it to the target
//! RENDER ELEMENTS node (vtable 0x30947B8, work 0x33DDA4, its exact destructor). The node's 64-byte header -- its
//! vtable and the owned 48-byte stage vector at +0x30 -- and every stage are read twice, with the tree and the graph
//! revalidated and the header read a final time. Native definition bytes only: the caller holds the original
//! definition owner while reading; neither a copied tree nor this snapshot grants lifetime, callback, clone,
//! scheduling, resource-state or execution permission.
//!
//! Definition header: +0x18 bool gate(context), +0x20 before(context, span), +0x28 after(context), +0x30 the stage
//! vector. Before receives the FULL stage list deduplicated by {DWORD +0x10, BYTE +0x14}, not a sliced prefix. Stage
//! bytes: callback +0, flags +8, mode +0xC, stage +0x10, technique +0x14, filters +0x18/+0x1C, state +0x20,
//! prebuilt list +0x24, partition +0x28/+0x2C; unknown / padding bytes and prebuilt partition fields are kept verbatim.
use super::definition_tree::{self as tree, Kind, Snapshot as Tree};
use super::graph_snapshot::{field_u32, field_u64, Node};

pub const WORK_RVA: u64 = 0x33DDA4;
pub const VTABLE_RVA: u64 = 0x30947B8;
pub const HEADER_BYTES: usize = 64;
pub const STAGE_BYTES: usize = 48;
pub const MAX_STAGES: u32 = tree::MAX_ARRAY_CAPACITY;

pub type Header = [u8; HEADER_BYTES];
pub type Stage = [u8; STAGE_BYTES];
/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Tree,
    Graph,
    Reachability,
    Identity,
    Array,
    Bounds,
    Read,
    Changed,
    Allocation,
}

fn address(at: u64, bytes: u64) -> bool {
    at != 0 && at & 7 == 0 && at <= u64::MAX - bytes
}

/// The graph node's exact shape against a ready tree.
pub fn graph_shape(graph: &Node, tree: &Tree) -> bool {
    let base = tree.image_base();
    let id = graph.handle & 0xDFFF_FFFF;
    tree.ready()
        && base != 0
        && base <= u64::MAX - 0x3BCDCA8
        && id < 65536
        && (graph.handle == id || graph.handle == (id | 0x2000_0000))
        && graph.order == field_u32(&graph.bytes, 0)
        && field_u32(&graph.bytes, 24) < 2
        && graph.vtable != 0
        && graph.work != 0
        && field_u64(&graph.definition, 0) == graph.vtable
        && address(field_u64(&graph.bytes, 16), 32)
}

/// The tree root holding the graph node's object (same work and vtable), or NO_INDEX.
pub fn root_index(graph: &Node, tree: &Tree) -> u32 {
    if !tree.ready() {
        return tree::NO_INDEX;
    }
    let object = field_u64(&graph.bytes, 16);
    for root in tree.roots() {
        if root.object == object && (root.node as usize) < tree.nodes().len() {
            let node = &tree.nodes()[root.node as usize];
            if node.object == object && node.work == graph.work && node.vtable == graph.vtable {
                return root.node;
            }
        }
    }
    tree::NO_INDEX
}

/// A tree path from the graph node's root to `target`, through consistent handles only.
pub fn reachable(graph: &Node, tree: &Tree, target: u32) -> bool {
    let nodes = tree.nodes();
    let handles = tree.handles();
    if !graph_shape(graph, tree) || target as usize >= nodes.len() {
        return false;
    }
    let root = root_index(graph, tree);
    if root == tree::NO_INDEX {
        return false;
    }
    let mut pending = vec![root];
    let mut seen = vec![false; nodes.len()];
    seen[root as usize] = true;
    let mut at = 0;
    while at < pending.len() {
        let index = pending[at];
        at += 1;
        if index == target {
            return true;
        }
        let node = &nodes[index as usize];
        if node.first_handle == tree::NO_INDEX {
            if node.handle_count != 0 {
                return false;
            }
            continue;
        }
        if node.first_handle as usize > handles.len()
            || node.handle_count as usize > handles.len() - node.first_handle as usize
        {
            return false;
        }
        for i in 0..node.handle_count {
            let handle = &handles[(node.first_handle + i) as usize];
            if handle.parent != index {
                return false;
            }
            if handle.child == tree::NO_INDEX {
                if handle.object != 0 {
                    return false;
                }
                continue;
            }
            if handle.child as usize >= nodes.len()
                || handle.object != nodes[handle.child as usize].object
            {
                return false;
            }
            if !seen[handle.child as usize] {
                seen[handle.child as usize] = true;
                pending.push(handle.child);
            }
        }
    }
    false
}

fn read_graph(graph: &Node, read: &mut dyn FnMut(u64, &mut [u8]) -> bool) -> bool {
    let mut definition = [0u8; 32];
    let mut work = [0u8; 8];
    address(graph.vtable, 16)
        && read(field_u64(&graph.bytes, 16), &mut definition)
        && definition == graph.definition
        && read(graph.vtable + 8, &mut work)
        && u64::from_le_bytes(work) == graph.work
}
fn header_matches(header: &Header, node: &tree::Node, array: &tree::Array) -> bool {
    field_u64(header, 0) == node.vtable
        && field_u64(header, 0x30) == array.data
        && field_u32(header, 0x38) == array.capacity
        && field_u32(header, 0x3C) == array.count
}

/// The target node's header and stages, each read twice, the tree and graph revalidated and the header read again.
pub fn capture(
    graph: &Node,
    tree: &Tree,
    target: u32,
    read: Option<Read<'_>>,
) -> Result<(Header, Vec<Stage>), Failure> {
    let Some(read) = read else {
        return Err(Failure::Arguments);
    };
    if !graph_shape(graph, tree) || target as usize >= tree.nodes().len() {
        return Err(Failure::Arguments);
    }
    if !reachable(graph, tree, target) {
        return Err(Failure::Reachability);
    }
    if !tree.matches_before(read) {
        return Err(Failure::Tree);
    }
    if !read_graph(graph, read) {
        return Err(Failure::Graph);
    }
    let node = &tree.nodes()[target as usize];
    let base = tree.image_base();
    if node.kind != Kind::RenderElements
        || node.vtable != base + VTABLE_RVA
        || node.work != base + WORK_RVA
        || node.destructor != base + tree::RENDER_ELEMENTS_DESTRUCTOR_RVA
        || !address(node.object, HEADER_BYTES as u64)
        || node.array as usize >= tree.arrays().len()
    {
        return Err(Failure::Identity);
    }
    let array = &tree.arrays()[node.array as usize];
    if array.node != target
        || array.header_address != node.object + 0x30
        || array.stride != STAGE_BYTES as u32
        || array.count > array.capacity
        || array.capacity > MAX_STAGES
        || array.data == 0
        || array.data & 7 != 0
    {
        return Err(Failure::Array);
    }
    let count = array.count as usize;
    if count != 0 && !address(array.data, count as u64 * STAGE_BYTES as u64) {
        return Err(Failure::Bounds);
    }
    let (mut first, mut second) = ([0u8; HEADER_BYTES], [0u8; HEADER_BYTES]);
    let mut before = vec![[0u8; STAGE_BYTES]; count];
    let mut after = vec![[0u8; STAGE_BYTES]; count];
    if !read(node.object, &mut first) || !header_matches(&first, node, array) {
        return Err(Failure::Read);
    }
    if count != 0 && !read(array.data, before.as_flattened_mut()) {
        return Err(Failure::Read);
    }
    if !read(node.object, &mut second) || !header_matches(&second, node, array) {
        return Err(Failure::Read);
    }
    if count != 0 && !read(array.data, after.as_flattened_mut()) {
        return Err(Failure::Read);
    }
    if first != second || before != after || !tree.matches_before(read) || !read_graph(graph, read)
    {
        return Err(Failure::Changed);
    }
    let mut last = [0u8; HEADER_BYTES];
    if !read(node.object, &mut last) || last != second {
        return Err(Failure::Changed);
    }
    Ok((second, after))
}
