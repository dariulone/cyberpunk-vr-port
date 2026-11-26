//! The stage replay's engine reads and packings (src/Render/StageReplay.cpp, `EngineFrame`, `RingReserve`, `LogEvent`
//! and `ReadCamera`): the native renderer's frame clock (renderer+0x4CC8 of the renderer at image+0x3BBCAE8; 0 when
//! unreadable); the shader-visible descriptor ring's reservation -- its 2.5 code (177 bytes at 0x419DF4, FNV
//! 0xBF311A1C73348164), the ring (registry at image+0x3BCDCA8, +0x14F1E40) on exactly one heap that is the replay heap
//! with a capacity inside it (else diagnostic 9), a count inside the capacity (else 10), and a reserved range inside the
//! capacity (else 10); a census event's key (frame, then side, stage, ordinal and kind bytes from bit 32); and the
//! camera slots' packing (each stage's parameter | index << 16, 0xFFFF when unresolved, the PS in the high half).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_srg_engine_frame`, `cvr_srg_ring_code`,
//! `cvr_srg_ring_find`, `cvr_srg_ring_reserved`, `cvr_srg_event_key` and `cvr_srg_camera_packed` (the reservation call
//! itself, its once-only code check and the diagnostics stay the .cpp's); the unit tests below read fabricated memory.

use crate::instance_layout::{read, read_u32, read_u64};
use crate::native_recording::fingerprint;

pub const RENDERER_RVA: u64 = 0x3BBCAE8;
pub const REGISTRY_RVA: u64 = 0x3BCDCA8;
pub const RING_OFFSET: u64 = 0x14F1E40;
pub const RESERVE_RVA: u64 = 0x419DF4;
pub const RESERVE_BYTES: usize = 177;
pub const RESERVE_FNV: u64 = 0xBF31_1A1C_7334_8164;

/// EngineFrame.
pub fn engine_frame(base: u64) -> u32 {
    if base == 0 {
        return 0;
    }
    match read_u64(base.wrapping_add(RENDERER_RVA)) {
        Some(renderer) if renderer != 0 => read_u32(renderer.wrapping_add(0x4CC8)).unwrap_or(0),
        _ => 0,
    }
}
/// A code block read whole at `base + rva` matching its fingerprint.
pub fn code_at(base: u64, rva: u64, len: usize, fnv: u64) -> bool {
    let mut bytes = vec![0u8; len];
    base != 0 && read(base.wrapping_add(rva), &mut bytes) && fingerprint(&bytes) == fnv
}
/// RingReserve's code check.
pub fn ring_code(base: u64) -> bool {
    code_at(base, RESERVE_RVA, RESERVE_BYTES, RESERVE_FNV)
}
/// RingReserve's ring: Ok((ring, capacity)) or the diagnostic slot (9 the ring, 10 the count).
pub fn ring_find(base: u64, heap: u64, heap_count: u32, count: u32) -> Result<(u64, u64), u32> {
    let registry = read_u64(base.wrapping_add(REGISTRY_RVA)).ok_or(9u32)?;
    if registry == 0 || registry > u64::MAX - RING_OFFSET - 72 {
        return Err(9);
    }
    let ring = registry + RING_OFFSET;
    let heaps = read_u32(ring + 64).ok_or(9u32)?;
    if heaps != 1 {
        return Err(9);
    }
    let ring_heap = read_u64(ring + 56).ok_or(9u32)?;
    let capacity = read_u64(ring + 16).ok_or(9u32)?;
    if ring_heap != heap || capacity == 0 || capacity > u64::from(heap_count) {
        return Err(9);
    }
    if count == 0 || u64::from(count) > capacity {
        return Err(10);
    }
    Ok((ring, capacity))
}
/// RingReserve's result check.
pub fn ring_reserved(first: u64, count: u32, capacity: u64) -> bool {
    first < capacity && u64::from(count) <= capacity - first
}
/// LogEvent's key.
pub fn event_key(frame: u32, side: u32, stage: u32, ordinal: u32, kind: u32) -> u64 {
    u64::from(frame) | (u64::from(side & 0xFF) << 32) | (u64::from(stage & 0xFF) << 40) | (u64::from(ordinal & 0xFF) << 48) | (u64::from(kind & 0xFF) << 56)
}
/// ReadCamera's packing: each stage's slot when resolved.
pub fn camera_packed(vs: Option<(u32, u32)>, ps: Option<(u32, u32)>) -> u64 {
    let half = |slot: Option<(u32, u32)>| slot.map_or(0xFFFF, |(parameter, index)| u64::from(parameter) | (u64::from(index) << 16));
    half(vs) | (half(ps) << 32)
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_srg_engine_frame(base: u64) -> u32 {
    engine_frame(base)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srg_ring_code(base: u64) -> bool {
    ring_code(base)
}
/// # Safety
/// `ring` and `capacity` are writable. Returns 0, or the diagnostic slot 9 or 10.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srg_ring_find(base: u64, heap: u64, heap_count: u32, count: u32, ring: *mut u64, capacity: *mut u64) -> u32 {
    match ring_find(base, heap, heap_count, count) {
        Ok((found, size)) => {
            unsafe {
                *ring = found;
                *capacity = size;
            }
            0
        }
        Err(slot) => slot,
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srg_ring_reserved(first: u64, count: u32, capacity: u64) -> bool {
    ring_reserved(first, count, capacity)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srg_event_key(frame: u32, side: u32, stage: u32, ordinal: u32, kind: u32) -> u64 {
    event_key(frame, side, stage, ordinal, kind)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srg_camera_packed(vs: bool, vs_parameter: u32, vs_index: u32, ps: bool, ps_parameter: u32, ps_index: u32) -> u64 {
    camera_packed(vs.then_some((vs_parameter, vs_index)), ps.then_some((ps_parameter, ps_index)))
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    #[test]
    fn frames() {
        let renderer = vec![0u8; 0x4D00];
        let mut renderer = renderer;
        renderer[0x4CC8..0x4CCC].copy_from_slice(&77u32.to_le_bytes());
        let slot = renderer.as_ptr() as u64;
        // A fabricated image whose renderer slot holds the renderer: base = slot cell - RENDERER_RVA.
        let cell = Box::new(slot);
        let base = (&*cell as *const u64 as u64).wrapping_sub(RENDERER_RVA);
        assert_eq!(engine_frame(base), 77);
        assert_eq!(engine_frame(0), 0);
        let empty = Box::new(0u64);
        assert_eq!(engine_frame((&*empty as *const u64 as u64).wrapping_sub(RENDERER_RVA)), 0);
    }
    #[test]
    fn rings() {
        let mut ring = vec![0u8; 80];
        ring[64..68].copy_from_slice(&1u32.to_le_bytes());
        ring[56..64].copy_from_slice(&0xAAu64.to_le_bytes());
        ring[16..24].copy_from_slice(&100u64.to_le_bytes());
        let registry = (ring.as_ptr() as u64).wrapping_sub(RING_OFFSET);
        let cell = Box::new(registry);
        let base = (&*cell as *const u64 as u64).wrapping_sub(REGISTRY_RVA);
        assert_eq!(ring_find(base, 0xAA, 100, 10), Ok((ring.as_ptr() as u64, 100)));
        assert_eq!(ring_find(base, 0xAB, 100, 10), Err(9));
        assert_eq!(ring_find(base, 0xAA, 99, 10), Err(9));
        assert_eq!(ring_find(base, 0xAA, 100, 0), Err(10));
        assert_eq!(ring_find(base, 0xAA, 100, 101), Err(10));
        assert_eq!(ring_find(base, 0xAA, 100, 100), Ok((ring.as_ptr() as u64, 100)));
        ring[64..68].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(ring_find(base, 0xAA, 100, 10), Err(9));
        let zero = Box::new(0u64);
        assert_eq!(ring_find((&*zero as *const u64 as u64).wrapping_sub(REGISTRY_RVA), 0xAA, 100, 10), Err(9));
        assert!(ring_reserved(90, 10, 100));
        assert!(!ring_reserved(91, 10, 100));
        assert!(!ring_reserved(100, 0, 100));
    }
    #[test]
    fn codes() {
        let mut image = vec![0x90u8; 0x200];
        let base = image.as_ptr() as u64;
        let fnv = fingerprint(&image[0x40..0x40 + RESERVE_BYTES]);
        assert!(code_at(base, 0x40, RESERVE_BYTES, fnv));
        image[0x40 + RESERVE_BYTES - 1] = 0xCC;
        assert!(!code_at(base, 0x40, RESERVE_BYTES, fnv));
        assert!(!code_at(0, 0x40, RESERVE_BYTES, fnv));
        assert!(!ring_code(0));
    }
    #[test]
    fn packings() {
        assert_eq!(event_key(5, 1, 6, 2, 3), 5 | (1 << 32) | (6 << 40) | (2 << 48) | (3 << 56));
        assert_eq!(event_key(5, 0x101, 0x106, 0x102, 0x103), event_key(5, 1, 6, 2, 3));
        assert_eq!(camera_packed(Some((3, 2)), None), 3 | (2 << 16) | (0xFFFF << 32));
        assert_eq!(camera_packed(None, Some((4, 1))), 0xFFFF | ((4 | (1 << 16)) << 32));
    }
}
