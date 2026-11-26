//! A bounded snapshot of the native resource-use pool a render graph's compiler reads (include/Render/
//! NativeResourceUses.hpp). 2.5 (sub_140334A34, sub_1403E0330, sub_1403EB51C): 220 slots of 0x70C0 bytes, each up to
//! 600 declarations of 48 bytes, then the use ids (slot << 16 | index, key in the high half), the auxiliary keys and
//! the boundary ids. Taken at native 1EDA84 (mode 2) entry before the original compiler, while the pool's mode byte is
//! 1 and the declaration jobs completed; `source` is the caller-owned exact graph those jobs used. No native pointer in
//! a captured declaration is dereferenced or retained, and everything read is read again at the end: a pool that
//! changed meanwhile is refused (Failure::Changed), as is anything out of the native bounds.
//!
//! The declarations describe native lifetimes, not GPU read/write access: kind 7 relates keys, kind 8 imports a native
//! handle. Neither these relationships nor key equality prove a reorder safe (`opaque_state_unsupported`).
use super::graph_snapshot::{self, field_u32};
use std::collections::{BTreeMap, HashSet};

pub const SLOT_STRIDE: u64 = 0x70c0;
pub const DECLARATIONS: u64 = 0x48;
pub const DECLARATION_COUNT: u64 = 0x70c8;
pub const USE_COUNT: u64 = 0x60e540;
pub const USE_IDS: u64 = 0x60e548;
pub const AUX_COUNT: u64 = 0x616548;
pub const AUX_KEYS: u64 = 0x61654c;
pub const BOUNDARY_COUNT: u64 = 0x61664c;
pub const BOUNDARY_IDS: u64 = 0x616650;
pub const MAX_SLOTS: u32 = 220;
pub const MAX_DECLARATIONS: u32 = 600;
pub const MAX_USES: u32 = 4096;
pub const MAX_AUX_KEYS: u32 = 64;
pub const MAX_BOUNDARIES: u32 = 24;
const _: () = assert!(
    SLOT_STRIDE * MAX_SLOTS as u64 == 0x60e500
        && DECLARATIONS + 48 * MAX_DECLARATIONS as u64 == DECLARATION_COUNT
);
const _: () = assert!(
    USE_IDS + 8 * MAX_USES as u64 == AUX_COUNT
        && AUX_KEYS + 4 * MAX_AUX_KEYS as u64 == BOUNDARY_COUNT
        && BOUNDARY_IDS + 8 * MAX_BOUNDARIES as u64 == 0x616710
);

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Failure {
    None,
    #[default]
    Arguments,
    SourceGraph,
    Header,
    Limit,
    Ids,
    Slot,
    Index,
    Declaration,
    Changed,
    Allocation,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub slot: u32,
    pub index: u32,
    pub key: u32,
    pub kind: u32,
    pub node_handle: u32,
    pub node_order: u32,
    pub bytes: [u8; 48],
}
impl Default for Declaration {
    fn default() -> Self {
        Declaration {
            slot: 0,
            index: 0,
            key: 0,
            kind: 0,
            node_handle: 0,
            node_order: 0,
            bytes: [0; 48],
        }
    }
}

const _: () = assert!(core::mem::size_of::<Declaration>() == 72);

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub pool: u64,
    pub context_root: u64,
    pub pool_header: [u8; 4],
    pub source_nodes: Vec<graph_snapshot::Node>,
    pub use_ids: Vec<u64>,
    pub boundary_ids: Vec<u64>,
    pub auxiliary_keys: Vec<u32>,
    pub declarations: Vec<Declaration>,
    pub kinds: [u32; 14],
    pub opaque_state_unsupported: bool,
    pub failure: Failure,
}
impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            pool: 0,
            context_root: 0,
            pool_header: [0; 4],
            source_nodes: Vec::new(),
            use_ids: Vec::new(),
            boundary_ids: Vec::new(),
            auxiliary_keys: Vec::new(),
            declarations: Vec::new(),
            kinds: [0; 14],
            opaque_state_unsupported: true,
            failure: Failure::Arguments,
        }
    }
}
fn u32s(raw: &[u8]) -> Vec<u32> {
    raw.chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("four bytes")))
        .collect()
}
fn u64s(raw: &[u8]) -> Vec<u64> {
    raw.chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("eight bytes")))
        .collect()
}

impl Snapshot {
    fn reject(&mut self, reason: Failure) -> bool {
        *self = Snapshot {
            failure: reason,
            ..Snapshot::default()
        };
        false
    }
    /// The pool at `address` for the exact `graph` its declaration jobs used; `root` the frame context it belongs to.
    pub fn capture_pool(
        &mut self,
        address: u64,
        graph: &graph_snapshot::Snapshot,
        read: Option<Read<'_>>,
        root: u64,
    ) -> bool {
        *self = Snapshot::default();
        let Some(read) = read else {
            return self.reject(Failure::Arguments);
        };
        if address == 0 || address > u64::MAX - (BOUNDARY_IDS + MAX_BOUNDARIES as u64 * 8) {
            return self.reject(Failure::Arguments);
        }
        let mut memory = |offset: u64, out: &mut [u8]| -> bool {
            offset <= u64::MAX - address
                && (out.len() as u64) <= u64::MAX - address - offset
                && read(address + offset, out)
        };
        if graph.failure != 0 || graph.nodes.is_empty() || graph.nodes.len() > MAX_SLOTS as usize {
            return self.reject(Failure::SourceGraph);
        }
        let mut nodes: [Option<usize>; MAX_SLOTS as usize] = [None; MAX_SLOTS as usize];
        let mut handles = HashSet::new();
        for (i, node) in graph.nodes.iter().enumerate() {
            let id = node.handle & 0xdfff_ffff;
            let slot = node.bytes[0x1d] as u32;
            if id >= 65536
                || (node.handle != id && node.handle != (id | 0x2000_0000))
                || node.order as usize != i
                || field_u32(&node.bytes, 0) as usize != i
                || !handles.insert(id)
                || slot >= MAX_SLOTS
                || nodes[slot as usize].is_some()
            {
                return self.reject(Failure::SourceGraph);
            }
            nodes[slot as usize] = Some(i);
        }
        let mut next = Snapshot {
            pool: address,
            context_root: root,
            source_nodes: graph.nodes.clone(),
            ..Snapshot::default()
        };
        let (mut uses, mut auxiliary, mut boundaries) = ([0u8; 4], [0u8; 4], [0u8; 4]);
        if !memory(0, &mut next.pool_header)
            || next.pool_header[2] != 1
            || !memory(USE_COUNT, &mut uses)
            || !memory(AUX_COUNT, &mut auxiliary)
            || !memory(BOUNDARY_COUNT, &mut boundaries)
        {
            return self.reject(Failure::Header);
        }
        let (uses, auxiliary, boundaries) = (
            u32::from_le_bytes(uses),
            u32::from_le_bytes(auxiliary),
            u32::from_le_bytes(boundaries),
        );
        if uses > MAX_USES || auxiliary > MAX_AUX_KEYS || boundaries > MAX_BOUNDARIES {
            return self.reject(Failure::Limit);
        }
        let (mut use_raw, mut aux_raw, mut bound_raw) = (
            vec![0u8; uses as usize * 8],
            vec![0u8; auxiliary as usize * 4],
            vec![0u8; boundaries as usize * 8],
        );
        if (uses != 0 && !memory(USE_IDS, &mut use_raw))
            || (auxiliary != 0 && !memory(AUX_KEYS, &mut aux_raw))
            || (boundaries != 0 && !memory(BOUNDARY_IDS, &mut bound_raw))
        {
            return self.reject(Failure::Ids);
        }
        next.use_ids = u64s(&use_raw);
        next.auxiliary_keys = u32s(&aux_raw);
        next.boundary_ids = u64s(&bound_raw);
        let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
        let mut rows: BTreeMap<u32, usize> = BTreeMap::new();
        for &value in &next.use_ids.clone() {
            let slot = ((value >> 16) & 65535) as u32;
            let index = (value & 65535) as u32;
            let key = (value >> 32) as u32;
            if slot >= MAX_SLOTS || nodes[slot as usize].is_none() {
                return self.reject(Failure::Slot);
            }
            if let std::collections::btree_map::Entry::Vacant(e) = counts.entry(slot) {
                let mut count = [0u8; 4];
                if !memory(SLOT_STRIDE * slot as u64 + DECLARATION_COUNT, &mut count) {
                    return self.reject(Failure::Declaration);
                }
                let count = u32::from_le_bytes(count);
                if count > MAX_DECLARATIONS {
                    return self.reject(Failure::Limit);
                }
                e.insert(count);
            }
            if index >= counts[&slot] || index >= MAX_DECLARATIONS {
                return self.reject(Failure::Index);
            }
            let ordinal = (slot << 16) | index;
            if let Some(&found) = rows.get(&ordinal) {
                if next.declarations[found].key != key {
                    return self.reject(Failure::Declaration);
                }
                continue;
            }
            let node = &graph.nodes[nodes[slot as usize].expect("checked")];
            let mut row = Declaration {
                slot,
                index,
                key,
                node_handle: node.handle,
                node_order: node.order,
                ..Declaration::default()
            };
            if !memory(
                SLOT_STRIDE * slot as u64 + DECLARATIONS + index as u64 * 48,
                &mut row.bytes,
            ) || row.bytes[1] as u32 != slot
                || field_u32(&row.bytes, 4) != key
            {
                return self.reject(Failure::Declaration);
            }
            row.kind = row.bytes[0] as u32;
            if (row.kind as usize) < next.kinds.len() {
                next.kinds[row.kind as usize] += 1;
            }
            rows.insert(ordinal, next.declarations.len());
            next.declarations.push(row);
        }
        for &value in &next.boundary_ids {
            if value >> 32 != 0 || !rows.contains_key(&(value as u32)) {
                return self.reject(Failure::Ids);
            }
            let row = &next.declarations[rows[&(value as u32)]];
            let subtype = field_u32(&row.bytes, 8);
            if row.kind != 11 || (subtype != 1 && subtype != 2) {
                return self.reject(Failure::Declaration);
            }
        }
        for row in &next.declarations {
            let mut again = [0u8; 48];
            if !memory(
                SLOT_STRIDE * row.slot as u64 + DECLARATIONS + row.index as u64 * 48,
                &mut again,
            ) || again != row.bytes
            {
                return self.reject(Failure::Changed);
            }
        }
        for (&slot, &count) in &counts {
            let mut again = [0u8; 4];
            if !memory(SLOT_STRIDE * slot as u64 + DECLARATION_COUNT, &mut again)
                || u32::from_le_bytes(again) != count
            {
                return self.reject(Failure::Changed);
            }
        }
        let (mut use_again, mut aux_again, mut bound_again) = (
            vec![0u8; use_raw.len()],
            vec![0u8; aux_raw.len()],
            vec![0u8; bound_raw.len()],
        );
        if (uses != 0 && (!memory(USE_IDS, &mut use_again) || use_again != use_raw))
            || (auxiliary != 0 && (!memory(AUX_KEYS, &mut aux_again) || aux_again != aux_raw))
            || (boundaries != 0
                && (!memory(BOUNDARY_IDS, &mut bound_again) || bound_again != bound_raw))
        {
            return self.reject(Failure::Changed);
        }
        let (mut header, mut u, mut a, mut b) = ([0u8; 4], [0u8; 4], [0u8; 4], [0u8; 4]);
        if !memory(0, &mut header)
            || header != next.pool_header
            || !memory(USE_COUNT, &mut u)
            || u32::from_le_bytes(u) != uses
            || !memory(AUX_COUNT, &mut a)
            || u32::from_le_bytes(a) != auxiliary
            || !memory(BOUNDARY_COUNT, &mut b)
            || u32::from_le_bytes(b) != boundaries
        {
            return self.reject(Failure::Changed);
        }
        next.failure = Failure::None;
        *self = next;
        true
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/resource_uses_test.cpp's cases.
    use super::*;
    use std::collections::BTreeMap;

    const POOL: u64 = 0x1000_0000;
    #[derive(Default, Clone)]
    struct Memory {
        values: BTreeMap<u64, Vec<u8>>,
        calls: BTreeMap<u64, usize>,
        reads: usize,
        flip: Option<(u64, usize, usize)>,
    }
    impl Memory {
        fn put(&mut self, address: u64, bytes: &[u8]) {
            self.values.insert(address, bytes.to_vec());
        }
        fn put32(&mut self, address: u64, v: u32) {
            self.put(address, &v.to_le_bytes());
        }
        fn ids(&mut self, ids: &[u64]) {
            let bytes: Vec<u8> = ids.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.values.insert(POOL + USE_IDS, bytes);
            self.put32(POOL + USE_COUNT, ids.len() as u32);
        }
        fn read(&mut self, address: u64, out: &mut [u8]) -> bool {
            self.reads += 1;
            let call = {
                let c = self.calls.entry(address).or_insert(0);
                *c += 1;
                *c
            };
            if let Some((at, on, byte)) = self.flip {
                if at == address && call == on {
                    self.values.get_mut(&at).expect("flipped value")[byte] ^= 1;
                }
            }
            let Some((&base, data)) = self.values.range(..=address).next_back() else {
                return false;
            };
            let offset = (address - base) as usize;
            if offset > data.len() || out.len() > data.len() - offset {
                return false;
            }
            out.copy_from_slice(&data[offset..offset + out.len()]);
            true
        }
    }
    fn id(slot: u32, index: u32, key: u32) -> u64 {
        ((key as u64) << 32) | ((slot as u64) << 16) | index as u64
    }
    fn graph(count: u32) -> graph_snapshot::Snapshot {
        let mut g = graph_snapshot::Snapshot::default();
        for i in 0..count {
            let mut node = graph_snapshot::Node {
                handle: (100 + i) | 0x2000_0000,
                order: i,
                bytes: [0; 48],
                vtable: 0,
                work: 0,
                definition: [0; 32],
            };
            node.bytes[0..4].copy_from_slice(&i.to_le_bytes());
            node.bytes[0x1d] = i as u8;
            g.nodes.push(node);
        }
        g
    }
    fn fixture() -> Memory {
        let mut m = Memory::default();
        m.put(POOL, &[0, 0, 1, 0]);
        m.put32(POOL + AUX_COUNT, 0);
        m.put32(POOL + BOUNDARY_COUNT, 0);
        m.put32(POOL + DECLARATION_COUNT, 2);
        m.put32(POOL + SLOT_STRIDE + DECLARATION_COUNT, 1);
        let (mut first, mut second, mut third) = ([0u8; 48], [0u8; 48], [0u8; 48]);
        first[0] = 4;
        first[4..8].copy_from_slice(&0x123u32.to_le_bytes());
        first[8..12].copy_from_slice(&3u32.to_le_bytes());
        first[47] = 0x55;
        second[0] = 5;
        second[4..8].copy_from_slice(&0x123u32.to_le_bytes());
        third[0] = 7;
        third[1] = 1;
        third[4..8].copy_from_slice(&0x456u32.to_le_bytes());
        third[8..12].copy_from_slice(&0x123u32.to_le_bytes());
        m.put(POOL + DECLARATIONS, &first);
        m.put(POOL + DECLARATIONS + 48, &second);
        m.put(POOL + SLOT_STRIDE + DECLARATIONS, &third);
        m.ids(&[
            id(1, 0, 0x456),
            id(0, 1, 0x123),
            id(0, 0, 0x123),
            id(0, 1, 0x123),
        ]);
        m
    }
    fn capture(m: &mut Memory, out: &mut Snapshot, g: &graph_snapshot::Snapshot) -> bool {
        out.capture_pool(POOL, g, Some(&mut |a, o| m.read(a, o)), 0x9988)
    }
    fn rejected(m: &mut Memory, reason: Failure, g: &graph_snapshot::Snapshot) {
        let mut out = Snapshot {
            pool: 1,
            use_ids: vec![7],
            declarations: vec![Declaration::default()],
            ..Snapshot::default()
        };
        assert!(!capture(m, &mut out, g), "accepted, expected {reason:?}");
        assert_eq!(out.failure, reason);
        assert!(
            out.pool == 0
                && out.context_root == 0
                && out.use_ids.is_empty()
                && out.declarations.is_empty()
        );
    }

    #[test]
    fn valid() {
        let mut m = fixture();
        let mut out = Snapshot::default();
        assert!(capture(&mut m, &mut out, &graph(2)));
        assert!(
            out.failure == Failure::None
                && out.pool == POOL
                && out.context_root == 0x9988
                && out.opaque_state_unsupported
        );
        assert!(
            out.use_ids.len() == 4
                && out.declarations.len() == 3
                && out.use_ids[1] == out.use_ids[3]
        );
        assert!(
            out.declarations[0].slot == 1
                && out.declarations[0].node_handle == (101 | 0x2000_0000)
                && out.declarations[0].node_order == 1
        );
        assert!(
            out.declarations[2].bytes[47] == 0x55
                && out.kinds[4] == 1
                && out.kinds[5] == 1
                && out.kinds[7] == 1
        );
        assert_eq!(out.source_nodes, graph(2).nodes);
        let mut m = fixture();
        m.ids(&[]);
        assert!(capture(&mut m, &mut out, &graph(2)) && out.declarations.is_empty());
        let mut m = fixture();
        assert!(capture(&mut m, &mut out, &graph(155)) && out.source_nodes.len() == 155);
        let mut m = fixture();
        m.values.get_mut(&(POOL + DECLARATIONS)).expect("row")[0] = 250;
        assert!(
            capture(&mut m, &mut out, &graph(2))
                && out.declarations[2].kind == 250
                && out.opaque_state_unsupported
        );
    }
    #[test]
    fn arguments_and_graph() {
        let mut m = fixture();
        let mut out = Snapshot::default();
        assert!(!out.capture_pool(0, &graph(2), Some(&mut |a, o| m.read(a, o)), 0));
        assert!(!out.capture_pool(u64::MAX, &graph(2), Some(&mut |a, o| m.read(a, o)), 0));
        assert!(!out.capture_pool(POOL, &graph(2), None, 0));
        assert_eq!(m.reads, 0);
        for option in 0..8 {
            let mut g = graph(2);
            match option {
                0 => g.failure = 1,
                1 => g.nodes.clear(),
                2 => g = graph(221),
                3 => g.nodes[1].handle = g.nodes[0].handle,
                4 => g.nodes[1].order = 0,
                5 => g.nodes[1].bytes[0x1d] = 0,
                6 => g.nodes[1].bytes[0x1d] = 220,
                _ => g.nodes[0].handle |= 0x8000_0000,
            }
            let mut m = fixture();
            rejected(&mut m, Failure::SourceGraph, &g);
        }
    }
    #[test]
    fn native_bounds() {
        for offset in [USE_COUNT, AUX_COUNT, BOUNDARY_COUNT] {
            let mut m = fixture();
            m.put32(POOL + offset, u32::MAX);
            rejected(&mut m, Failure::Limit, &graph(2));
        }
        for mode in [0u8, 2, 3, 255] {
            let mut m = fixture();
            m.values.get_mut(&POOL).expect("header")[2] = mode;
            rejected(&mut m, Failure::Header, &graph(2));
        }
        let cases: [(&[u64], Failure); 4] = [
            (&[id(2, 0, 0x123)], Failure::Slot),
            (&[id(65535, 0, 0x123)], Failure::Slot),
            (&[id(0, 2, 0x123)], Failure::Index),
            (&[id(0, 0, 0x124)], Failure::Declaration),
        ];
        for (ids, reason) in cases {
            let mut m = fixture();
            m.ids(ids);
            rejected(&mut m, reason, &graph(2));
        }
        let mut m = fixture();
        m.put32(POOL + DECLARATION_COUNT, MAX_DECLARATIONS + 1);
        rejected(&mut m, Failure::Limit, &graph(2));
        let mut m = fixture();
        m.ids(&[id(0, 0, 0x123), id(0, 0, 0x124)]);
        rejected(&mut m, Failure::Declaration, &graph(2));
        let mut m = fixture();
        m.values.get_mut(&(POOL + DECLARATIONS)).expect("row")[1] = 1;
        rejected(&mut m, Failure::Declaration, &graph(2));
        let mut m = fixture();
        m.values.remove(&(POOL + DECLARATIONS));
        rejected(&mut m, Failure::Declaration, &graph(2));
        let mut m = fixture();
        m.values.get_mut(&(POOL + USE_IDS)).expect("ids").pop();
        rejected(&mut m, Failure::Ids, &graph(2));
        let mut m = fixture();
        let mut g = graph(1);
        g.nodes[0].bytes[0x1d] = 219;
        m.put32(POOL + 219 * SLOT_STRIDE + DECLARATION_COUNT, 500);
        let mut row = [0u8; 48];
        row[0] = 1;
        row[1] = 219;
        row[4..8].copy_from_slice(&123u32.to_le_bytes());
        m.put(POOL + 219 * SLOT_STRIDE + DECLARATIONS + 499 * 48, &row);
        m.ids(&[id(219, 499, 123)]);
        let mut out = Snapshot::default();
        assert!(capture(&mut m, &mut out, &g));
    }
    #[test]
    fn aux_and_boundaries() {
        let mut m = fixture();
        m.put32(POOL + AUX_COUNT, 2);
        let aux: Vec<u8> = [0x123u32, 0x456]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        m.put(POOL + AUX_KEYS, &aux);
        m.put32(POOL + BOUNDARY_COUNT, 1);
        m.put(POOL + BOUNDARY_IDS, &1u64.to_le_bytes());
        {
            let row = m.values.get_mut(&(POOL + DECLARATIONS + 48)).expect("row");
            row[0] = 11;
            row[8..12].copy_from_slice(&1u32.to_le_bytes());
        }
        let mut out = Snapshot::default();
        assert!(
            capture(&mut m, &mut out, &graph(2))
                && out.auxiliary_keys.len() == 2
                && out.boundary_ids == vec![1]
        );
        m.calls.clear();
        m.put(POOL + BOUNDARY_IDS, &id(0, 1, 1).to_le_bytes());
        rejected(&mut m, Failure::Ids, &graph(2));
        let mut m = fixture();
        m.put32(POOL + BOUNDARY_COUNT, 1);
        m.put(POOL + BOUNDARY_IDS, &5u64.to_le_bytes());
        rejected(&mut m, Failure::Ids, &graph(2));
        let mut m = fixture();
        m.put32(POOL + BOUNDARY_COUNT, 1);
        m.put(POOL + BOUNDARY_IDS, &1u64.to_le_bytes());
        rejected(&mut m, Failure::Declaration, &graph(2));
    }
    #[test]
    fn changed_reads() {
        for address in [
            POOL,
            POOL + USE_COUNT,
            POOL + AUX_COUNT,
            POOL + BOUNDARY_COUNT,
            POOL + USE_IDS,
            POOL + DECLARATIONS,
            POOL + DECLARATION_COUNT,
        ] {
            let mut m = fixture();
            m.flip = Some((address, 2, if address == POOL { 2 } else { 0 }));
            rejected(&mut m, Failure::Changed, &graph(2));
        }
        let mut m = fixture();
        m.put32(POOL + AUX_COUNT, 1);
        m.put32(POOL + AUX_KEYS, 0x123);
        m.flip = Some((POOL + AUX_KEYS, 2, 0));
        rejected(&mut m, Failure::Changed, &graph(2));
    }
}
