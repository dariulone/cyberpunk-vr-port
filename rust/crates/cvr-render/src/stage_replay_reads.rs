//! The stage replay's native context reads (src/Render/StageReplay.cpp, `ReadBound`, `ReadView` and `ReadSlice`): the
//! descriptor a context's CPU table binds at (shader stage, kind, slot) through the current layout's slot maps
//! (sub_140336D3C: kind 0 SRV at layout+0x4C3, 1 UAV +0x8C3, 2 CBV +0x453, 3 sampler +0x95B; two bytes per slot, the
//! first the group, whose entry at layout+0x38 gives the table start and the slot range), or 0; the view descriptors at
//! consumer entry (the camera's two, each stage's view CBV 0 and its texture SRV 37 unless it is the null descriptor its
//! neighbours 36 or 38 hold); and the context's descriptor slice (heap, base, capacity, stride and cursor, read in order
//! and kept up to the first failure).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_srr_bound`, `cvr_srr_view` and `cvr_srr_slice` (the slice
//! mirrored); the unit tests below read fabricated contexts.

use crate::instance_layout::read;

fn value<const N: usize>(address: u64) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    read(address, &mut bytes).then_some(bytes)
}
fn u64_at(address: u64) -> Option<u64> {
    value::<8>(address).map(u64::from_le_bytes)
}
fn u32_at(address: u64) -> Option<u32> {
    value::<4>(address).map(u32::from_le_bytes)
}
fn u16_at(address: u64) -> Option<u16> {
    value::<2>(address).map(u16::from_le_bytes)
}

/// ReadBound.
pub fn bound(state: u64, stage: u32, kind: u32, slot: u32) -> u64 {
    const BASE: [u64; 4] = [1219, 2243, 1107, 2395];
    const STRIDE: [u64; 4] = [256, 38, 28, 32];
    const SLOTS: [u32; 4] = [128, 19, 14, 16];
    let inner = || -> Option<u64> {
        if state == 0 || stage > 2 || kind > 3 || slot >= SLOTS[kind as usize] {
            return None;
        }
        let k = kind as usize;
        let layout = u64_at(state + 0x68)?;
        let cpu = u64_at(state + 0x28)?;
        if layout == 0 || cpu == 0 || layout > u64::MAX - 0xA00 {
            return None;
        }
        let group = value::<1>(layout + BASE[k] + u64::from(stage) * STRIDE[k] + u64::from(slot) * 2)?[0];
        if group >= 64 {
            return None;
        }
        let entry = layout + 0x38 + u64::from(group) * 16;
        let start = u32_at(entry + 4)?;
        let first = u32::from(u16_at(entry + 8)?);
        let count = u32::from(u16_at(entry + 10)?);
        if slot < first || slot - first >= count || start > 8192 {
            return None;
        }
        u64_at(cpu.wrapping_add((u64::from(start) + u64::from(slot - first)).wrapping_mul(8)))
    };
    inner().unwrap_or(0)
}
/// ReadView's descriptors (stage_replay::ViewDescriptors: camera VS, camera PS, view VS, view PS, texture VS, texture PS).
pub fn view(state: u64, camera_vs: u64, camera_ps: u64) -> [u64; 6] {
    let mut d = [0u64; 6];
    d[0] = camera_vs;
    d[1] = camera_ps;
    for stage in 0..2u32 {
        d[2 + stage as usize] = bound(state, stage, 2, 0);
        let texture = bound(state, stage, 0, 37);
        if texture != 0 && texture != bound(state, stage, 0, 36) && texture != bound(state, stage, 0, 38) {
            d[4 + stage as usize] = texture;
        }
    }
    d
}
/// The .cpp's Slice.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slice {
    pub heap: u64,
    pub base: u32,
    pub capacity: u32,
    pub stride: u32,
    pub cursor: u32,
    pub ok: bool,
}
const _: () = assert!(core::mem::size_of::<Slice>() == 32);
/// ReadSlice: the fields read in order, kept up to the first failure.
pub fn slice(state: u64) -> Slice {
    let mut s = Slice::default();
    let mut step = || -> Option<()> {
        s.heap = u64_at(state)?;
        s.base = u32_at(state + 8)?;
        s.capacity = u32_at(state + 0xC)?;
        s.stride = u32_at(state + 0x10)?;
        s.cursor = u32_at(state + 0x14)?;
        Some(())
    };
    let ok = state != 0 && step().is_some();
    s.ok = ok;
    s
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_srr_bound(state: u64, stage: u32, kind: u32, slot: u32) -> u64 {
    bound(state, stage, kind, slot)
}
/// # Safety
/// `out` holds six descriptors.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srr_view(state: u64, camera_vs: u64, camera_ps: u64, out: *mut [u64; 6]) {
    unsafe { *out = view(state, camera_vs, camera_ps) };
}
/// # Safety
/// `out` is the mirrored slice.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srr_slice(state: u64, out: *mut Slice) {
    unsafe { *out = slice(state) };
}

#[cfg(test)]
mod tests {
    //! Fabricated contexts.
    use super::*;

    #[repr(C, align(16))]
    struct Memory([u8; 0x3000]);
    fn put(m: &mut Memory, at: usize, bytes: &[u8]) {
        m.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    /// A context at 0, its layout at 0x100, its CPU table at 0x2000: SRV group 3 for stage 0 slots 36..38 (table
    /// start 4, slots 30..40), CBV group 5 for stage 1 slot 0 (start 1, slots 0..1).
    fn context() -> (Box<Memory>, u64) {
        let mut m = Box::new(Memory([0; 0x3000]));
        let base = m.0.as_ptr() as u64;
        put(&mut m, 0x68, &(base + 0x100).to_le_bytes());
        put(&mut m, 0x28, &(base + 0x2000).to_le_bytes());
        for slot in 36..39u64 {
            put(&mut m, (0x100 + 1219 + slot * 2) as usize, &[3]);
        }
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 4, &4u32.to_le_bytes());
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 8, &30u16.to_le_bytes());
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 10, &10u16.to_le_bytes());
        put(&mut m, 0x100 + 1107 + 28, &[5]);
        put(&mut m, 0x100 + 0x38 + 5 * 16 + 4, &1u32.to_le_bytes());
        put(&mut m, 0x100 + 0x38 + 5 * 16 + 10, &1u16.to_le_bytes());
        for (slot, descriptor) in [(36u64, 0xAAu64), (37, 0xBB), (38, 0xAA)] {
            put(&mut m, (0x2000 + (4 + slot - 30) * 8) as usize, &descriptor.to_le_bytes());
        }
        put(&mut m, 0x2000 + 8, &0xCCu64.to_le_bytes());
        (m, base)
    }
    #[test]
    fn bounds() {
        let (mut m, base) = context();
        assert_eq!(bound(base, 0, 0, 37), 0xBB);
        assert_eq!(bound(base, 0, 0, 36), 0xAA);
        assert_eq!(bound(base, 1, 2, 0), 0xCC);
        assert_eq!(bound(base, 0, 2, 0), 0); // group 0's entry is empty
        assert_eq!(bound(base, 3, 0, 37), 0);
        assert_eq!(bound(base, 0, 4, 0), 0);
        assert_eq!(bound(base, 0, 1, 19), 0);
        assert_eq!(bound(0, 0, 0, 37), 0);
        put(&mut m, 0x100 + 1219 + 37 * 2, &[64]);
        assert_eq!(bound(base, 0, 0, 37), 0);
        put(&mut m, 0x100 + 1219 + 37 * 2, &[3]);
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 10, &7u16.to_le_bytes());
        assert_eq!(bound(base, 0, 0, 37), 0); // 37 - 30 = 7 is past the range
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 10, &10u16.to_le_bytes());
        put(&mut m, 0x100 + 0x38 + 3 * 16 + 4, &8193u32.to_le_bytes());
        assert_eq!(bound(base, 0, 0, 37), 0);
    }
    #[test]
    fn views() {
        let (mut m, base) = context();
        assert_eq!(view(base, 1, 2), [1, 2, 0, 0xCC, 0xBB, 0]);
        // The texture slot holding its neighbour's null descriptor counts as unbound.
        put(&mut m, (0x2000 + (4 + 37 - 30) * 8) as usize, &0xAAu64.to_le_bytes());
        assert_eq!(view(base, 1, 2)[4], 0);
    }
    #[test]
    fn slices() {
        let mut m = Box::new(Memory([0; 0x3000]));
        let base = m.0.as_ptr() as u64;
        put(&mut m, 0, &0x77u64.to_le_bytes());
        for (at, v) in [(8usize, 1u32), (0xC, 2), (0x10, 3), (0x14, 4)] {
            put(&mut m, at, &v.to_le_bytes());
        }
        assert_eq!(slice(base), Slice { heap: 0x77, base: 1, capacity: 2, stride: 3, cursor: 4, ok: true });
        assert_eq!(slice(0), Slice::default());
    }
}
