//! An owned snapshot of a compiled render graph's nodes and edges (include/Render/NativeGraphSnapshot.hpp; the base of
//! single pass's native graph records). The graph's header (0x70 bytes) names its node table (48-byte records, listed
//! through DWORD handles, bit 29 a flag) and its edge table (24-byte records, bit 30 a flag); each node's object is
//! read for its vtable, its work function (vtable+8) and its first 32 bytes. The whole capture is checked again at the
//! end -- header, both index lists and every record -- so a graph the engine changed meanwhile is refused rather than
//! half-copied. `failure`: 1 no header, 2 counts or tables out of bounds, 3 a bad or repeated node, 4 a bad or repeated
//! edge, 5 changed while read, 0 captured.
use std::collections::HashSet;

/// A read of `len` bytes of the engine's memory into the slice; false when it is not readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub handle: u32,
    pub order: u32,
    pub bytes: [u8; 48],
    pub vtable: u64,
    pub work: u64,
    pub definition: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub handle: u32,
    pub order: u32,
    pub bytes: [u8; 24],
}

const _: () = assert!(
    core::mem::size_of::<Node>() == 104
        && core::mem::offset_of!(Node, vtable) == 56
        && core::mem::offset_of!(Node, definition) == 72
        && core::mem::size_of::<Edge>() == 32
);

/// A little-endian field of a record.
pub fn field_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}
pub fn field_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"))
}
fn ids(raw: &[u8]) -> Vec<u32> {
    raw.chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("four bytes")))
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub failure: u32,
}
impl Snapshot {
    /// The graph at `graph`, read through `read`.
    pub fn capture(&mut self, graph: u64, read: Read<'_>) -> bool {
        self.nodes.clear();
        self.edges.clear();
        self.failure = 1;
        let mut header = [0u8; 0x70];
        if graph == 0 || !read(graph, &mut header) {
            return false;
        }
        let (count, edge_count) = (field_u32(&header, 0x10), field_u32(&header, 0x40));
        let (node_base, node_order) = (field_u64(&header, 0x18), field_u64(&header, 0x28));
        let (edge_base, edge_order) = (field_u64(&header, 0x48), field_u64(&header, 0x58));
        self.failure = 2;
        if count == 0
            || count > 8192
            || edge_count > 65536
            || node_base == 0
            || node_order == 0
            || (edge_count != 0 && (edge_base == 0 || edge_order == 0))
        {
            return false;
        }
        let mut node_raw = vec![0u8; count as usize * 4];
        let mut edge_raw = vec![0u8; edge_count as usize * 4];
        if !read(node_order, &mut node_raw) || (edge_count != 0 && !read(edge_order, &mut edge_raw))
        {
            return false;
        }
        let (node_ids, edge_ids) = (ids(&node_raw), ids(&edge_raw));
        let mut seen = HashSet::new();
        self.failure = 3;
        for (i, &raw) in node_ids.iter().enumerate() {
            let id = raw & 0xDFFF_FFFF;
            if id >= 65536 || !seen.insert(id) {
                return false;
            }
            let mut node = Node {
                handle: raw,
                order: i as u32,
                bytes: [0; 48],
                vtable: 0,
                work: 0,
                definition: [0; 32],
            };
            if node_base > u64::MAX - id as u64 * 48
                || !read(node_base + id as u64 * 48, &mut node.bytes)
            {
                return false;
            }
            let object = field_u64(&node.bytes, 16);
            if object != 0 {
                let mut vtable = [0u8; 8];
                if read(object, &mut vtable) {
                    node.vtable = u64::from_le_bytes(vtable);
                    if node.vtable <= u64::MAX - 8 {
                        let mut work = [0u8; 8];
                        if read(node.vtable + 8, &mut work) {
                            node.work = u64::from_le_bytes(work);
                        }
                    }
                }
                let mut definition = [0u8; 32];
                if read(object, &mut definition) {
                    node.definition = definition;
                }
            }
            self.nodes.push(node);
        }
        seen.clear();
        self.failure = 4;
        for (i, &raw) in edge_ids.iter().enumerate() {
            let id = raw & 0xBFFF_FFFF;
            if id >= 262144 || !seen.insert(id) {
                return false;
            }
            let mut edge = Edge {
                handle: raw,
                order: i as u32,
                bytes: [0; 24],
            };
            if edge_base > u64::MAX - id as u64 * 24
                || !read(edge_base + id as u64 * 24, &mut edge.bytes)
            {
                return false;
            }
            self.edges.push(edge);
        }
        self.failure = 5;
        for node in &self.nodes {
            let mut again = [0u8; 48];
            if !read(
                node_base + (node.handle & 0xDFFF_FFFF) as u64 * 48,
                &mut again,
            ) || again != node.bytes
            {
                return false;
            }
        }
        for edge in &self.edges {
            let mut again = [0u8; 24];
            if !read(
                edge_base + (edge.handle & 0xBFFF_FFFF) as u64 * 24,
                &mut again,
            ) || again != edge.bytes
            {
                return false;
            }
        }
        let mut after = [0u8; 0x70];
        let mut nodes_after = vec![0u8; count as usize * 4];
        let mut edges_after = vec![0u8; edge_count as usize * 4];
        if !read(graph, &mut after)
            || after != header
            || !read(node_order, &mut nodes_after)
            || nodes_after != node_raw
            || (edge_count != 0 && (!read(edge_order, &mut edges_after) || edges_after != edge_raw))
        {
            return false;
        }
        self.failure = 0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A fake address space of byte regions.
    #[derive(Default)]
    struct Memory {
        regions: HashMap<u64, Vec<u8>>,
    }
    impl Memory {
        fn put(&mut self, at: u64, bytes: Vec<u8>) {
            self.regions.insert(at, bytes);
        }
        fn read(&self, address: u64, out: &mut [u8]) -> bool {
            self.regions.iter().any(|(&at, bytes)| {
                if address >= at && address + out.len() as u64 <= at + bytes.len() as u64 {
                    let start = (address - at) as usize;
                    out.copy_from_slice(&bytes[start..start + out.len()]);
                    true
                } else {
                    false
                }
            })
        }
    }
    fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put64(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// tools/single_pass_tests/graph_snapshot_test.cpp's cases.
    #[test]
    fn owned_nodes_and_edges_sparse_indices_and_bounded_rejection() {
        let (graph_at, nodes_at, order_at, edge_at, edge_order_at, object_at, vtable_at) = (
            0x1000u64, 0x2000u64, 0x3000u64, 0x4000u64, 0x5000u64, 0x6000u64, 0x7000u64,
        );
        let mut graph = vec![0u8; 0x70];
        put32(&mut graph, 0x10, 2);
        put64(&mut graph, 0x18, nodes_at);
        put64(&mut graph, 0x28, order_at);
        put32(&mut graph, 0x40, 1);
        put64(&mut graph, 0x48, edge_at);
        put64(&mut graph, 0x58, edge_order_at);
        let mut nodes = vec![0u8; 48 * 4];
        put64(&mut nodes, 2 * 48 + 16, object_at);
        put32(&mut nodes, 2 * 48 + 24, 1);
        let mut edge = vec![0u8; 24];
        put32(&mut edge, 8, 0x2000_0002);
        let mut order = vec![0u8; 8];
        put32(&mut order, 0, 2);
        put32(&mut order, 4, 0);
        let mut object = vec![0u8; 32];
        put64(&mut object, 0, vtable_at);
        let mut vtable = vec![0u8; 16];
        put64(&mut vtable, 8, 0x1234);
        let mut m = Memory::default();
        m.put(graph_at, graph.clone());
        m.put(nodes_at, nodes);
        m.put(order_at, order.clone());
        m.put(edge_at, edge);
        m.put(edge_order_at, vec![0u8; 4]);
        m.put(object_at, object);
        m.put(vtable_at, vtable);
        let mut s = Snapshot::default();
        assert!(s.capture(graph_at, &mut |a, o| m.read(a, o)));
        assert!(
            s.nodes.len() == 2
                && s.nodes[0].handle == 2
                && s.nodes[0].work == 0x1234
                && s.edges.len() == 1
        );
        assert_eq!(field_u32(&s.nodes[0].bytes, 24), 1);
        // a repeated node
        put32(&mut order, 4, 2);
        m.put(order_at, order.clone());
        assert!(!s.capture(graph_at, &mut |a, o| m.read(a, o)) && s.failure == 3);
        // too many nodes
        put32(&mut order, 4, 0);
        m.put(order_at, order);
        put32(&mut graph, 0x10, 9000);
        m.put(graph_at, graph);
        assert!(!s.capture(graph_at, &mut |a, o| m.read(a, o)) && s.failure == 2);
    }
}
