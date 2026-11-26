//! The stage replay's camera groups (src/Render/StageReplay.cpp, `ReadCameraGroups`): the engine's current binding
//! state's camera tables -- for the vertex and the pixel stage, the layout's (state+0x68) group for that stage
//! (+0x455 + stage * 0x1C, under 64, each group once), its entry (+0x38 + group * 16: start, first, count, kind and root
//! parameter, a real non-sampler table of 1..=256 descriptors from b0 or b1 inside the context's 8192-entry CPU table)
//! and the CPU descriptors it must contain (state+0x28's table from start, none null); and the tables' shape (FNV-1a
//! over group, root parameter, count and first of each). A read failing part way keeps the tables already read and
//! their root parameters, as the .cpp's early return does, without the layout or shape.
//!
//! In the Rust build (`CVR_RUST_VRIK`) this forwards to `cvr_srr_camera_groups` (reads through ReadProcessMemory where
//! the .cpp's ReadSafe copies under SEH); the unit tests below read fabricated memory.

use crate::instance_layout::read;

/// One camera table: group, root parameter, start, count and first.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub group: u32,
    pub root_parameter: u32,
    pub start: u32,
    pub count: u32,
    pub first: u32,
}
/// The groups as the .cpp rebuilds them.
#[repr(C)]
pub struct Groups {
    pub layout: u64,
    pub params: u64,
    pub shape: u64,
    pub count: u32,
    pub complete: bool,
    pub tables: [Table; 2],
    pub descriptors: [[u64; 256]; 2],
}
const _: () = assert!(core::mem::size_of::<Groups>() == 32 + 40 + 4096);
impl Default for Groups {
    fn default() -> Self {
        Groups { layout: 0, params: 0, shape: 0, count: 0, complete: false, tables: [Table::default(); 2], descriptors: [[0; 256]; 2] }
    }
}

fn at<const N: usize>(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<[u8; N]> {
    let mut b = [0u8; N];
    (address != 0 && read(address, &mut b)).then_some(b)
}
fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    at::<8>(read, address).map(u64::from_le_bytes)
}

/// ReadCameraGroups into `out` (default on entry).
pub fn camera_groups(read: &impl Fn(u64, &mut [u8]) -> bool, state: u64, out: &mut Groups) {
    if state == 0 {
        return;
    }
    let (Some(layout), Some(cpu_table)) = (u64_at(read, state.wrapping_add(0x68)), u64_at(read, state.wrapping_add(0x28))) else { return };
    if layout == 0 || cpu_table == 0 || layout > u64::MAX - 0x473 {
        return;
    }
    let mut groups_seen = 0u64;
    for stage in 0..2u64 {
        let Some([group]) = at::<1>(read, layout + 0x455 + stage * 0x1C) else { return };
        if group >= 64 {
            return;
        }
        if groups_seen & (1u64 << group) != 0 {
            continue;
        }
        groups_seen |= 1u64 << group;
        let entry = layout + 0x38 + u64::from(group) * 16;
        let (Some(start), Some(first), Some(count), Some([kind]), Some([parameter])) =
            (at::<4>(read, entry + 4), at::<2>(read, entry + 8), at::<2>(read, entry + 10), at::<1>(read, entry + 13), at::<1>(read, entry + 14))
        else {
            return;
        };
        let (start, first, count) = (u32::from_le_bytes(start), u16::from_le_bytes(first), u16::from_le_bytes(count));
        if count == 0 || count > 256 || first > 1 || kind == 2 || parameter >= 64 || start > 8192 || u32::from(count) > 8192 - start {
            return;
        }
        let index = out.count as usize;
        let mut raw = vec![0u8; usize::from(count) * 8];
        if !read(cpu_table.wrapping_add(u64::from(start) * 8), &mut raw) {
            return;
        }
        let descriptors = &mut out.descriptors[index];
        for (d, chunk) in descriptors.iter_mut().zip(raw.chunks_exact(8)) {
            *d = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        if descriptors[..usize::from(count)].contains(&0) {
            return;
        }
        out.params |= 1u64 << parameter;
        out.tables[index] = Table { group: group.into(), root_parameter: parameter.into(), start, count: count.into(), first: first.into() };
        out.count += 1;
    }
    out.layout = layout;
    let mut h = 0xCBF29CE484222325u64;
    let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x100000001B3);
    for t in &out.tables[..out.count as usize] {
        mix(t.group.into());
        mix(t.root_parameter.into());
        mix(t.count.into());
        mix(t.first.into());
    }
    out.shape = h;
    out.complete = true;
}

// ---- the C ABI

/// # Safety
/// `out` is a writable Groups.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srr_camera_groups(state: u64, out: *mut Groups) {
    let out = unsafe { &mut *out };
    *out = Groups::default();
    camera_groups(&read, state, out);
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;
    use std::collections::BTreeMap;

    struct Memory(BTreeMap<u64, u8>);
    impl Memory {
        fn put(&mut self, at: u64, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.0.insert(at + i as u64, b);
            }
        }
        fn reader(&self) -> impl Fn(u64, &mut [u8]) -> bool + '_ {
            |address, out| {
                for (i, o) in out.iter_mut().enumerate() {
                    match self.0.get(&(address + i as u64)) {
                        Some(&b) => *o = b,
                        None => return false,
                    }
                }
                true
            }
        }
    }
    fn entry(m: &mut Memory, layout: u64, group: u8, start: u32, first: u16, count: u16, kind: u8, parameter: u8) {
        let e = layout + 0x38 + u64::from(group) * 16;
        m.put(e + 4, &start.to_le_bytes());
        m.put(e + 8, &first.to_le_bytes());
        m.put(e + 10, &count.to_le_bytes());
        m.put(e + 13, &[kind]);
        m.put(e + 14, &[parameter]);
    }
    fn state(groups: [u8; 2]) -> (Memory, u64) {
        let mut m = Memory(BTreeMap::new());
        let (state, layout, table) = (0x1000u64, 0x10000u64, 0x20000u64);
        m.put(state + 0x68, &layout.to_le_bytes());
        m.put(state + 0x28, &table.to_le_bytes());
        m.put(layout + 0x455, &[groups[0]]);
        m.put(layout + 0x455 + 0x1C, &[groups[1]]);
        entry(&mut m, layout, 3, 10, 1, 2, 0, 5);
        entry(&mut m, layout, 7, 20, 0, 1, 0, 9);
        for i in 0..32u64 {
            m.put(table + i * 8, &(0x9000 + i).to_le_bytes());
        }
        (m, state)
    }
    #[test]
    fn groups() {
        let (m, s) = state([3, 7]);
        let mut g = Groups::default();
        camera_groups(&m.reader(), s, &mut g);
        assert!(g.complete);
        assert_eq!((g.layout, g.params, g.count), (0x10000, (1 << 5) | (1 << 9), 2));
        assert_eq!(g.tables[0], Table { group: 3, root_parameter: 5, start: 10, count: 2, first: 1 });
        assert_eq!(&g.descriptors[0][..2], &[0x900A, 0x900B]);
        assert_eq!(g.descriptors[1][0], 0x9014);
        let mut h = 0xCBF29CE484222325u64;
        for v in [3u64, 5, 2, 1, 7, 9, 1, 0] {
            h = (h ^ v).wrapping_mul(0x100000001B3);
        }
        assert_eq!(g.shape, h);
        // One group shared by both stages is read once.
        let (m, s) = state([3, 3]);
        let mut g = Groups::default();
        camera_groups(&m.reader(), s, &mut g);
        assert_eq!((g.count, g.complete), (1, true));
    }
    #[test]
    fn partial() {
        // The pixel stage's group is out of range: the vertex stage's table is kept, without layout or shape.
        let (m, s) = state([3, 64]);
        let mut g = Groups::default();
        camera_groups(&m.reader(), s, &mut g);
        assert_eq!((g.count, g.complete, g.params, g.layout, g.shape), (1, false, 1 << 5, 0, 0));
        // A sampler table, a null descriptor or no state reads nothing.
        let (mut m, s) = state([3, 7]);
        entry(&mut m, 0x10000, 3, 10, 1, 2, 2, 5);
        let mut g = Groups::default();
        camera_groups(&m.reader(), s, &mut g);
        assert_eq!((g.count, g.complete), (0, false));
        let (mut m, s) = state([3, 7]);
        m.put(0x20000 + 11 * 8, &0u64.to_le_bytes());
        let mut g = Groups::default();
        camera_groups(&m.reader(), s, &mut g);
        assert_eq!((g.count, g.complete), (0, false));
        let mut g = Groups::default();
        camera_groups(&m.reader(), 0, &mut g);
        assert!(!g.complete);
    }
}
