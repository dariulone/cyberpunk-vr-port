//! The native prepared pair's eye compatibility (src/Render/NativePreparedPair.cpp, `prepared_pair::CompatibilityFailure`
//! and `SameState`): a right (side 1) and a left (side 0) draw of one frame pair only when they share the pipeline and
//! root (3), the index count, instances, first index and base vertex, all non-zero where counted (4), the input slots
//! with an instance stream and the index view bytes (5), the viewport and scissor bytes (6), the owned instance stride (7)
//! and every used vertex view but the instance stream's (9); a frame of 0 or two frames refuse (2), the wrong sides (1).
//! Their draw states agree when both have the same original pipeline, a triangle list, the same stencil and blend factor
//! and known equal shading rates and combiners.
//!
//! In the Rust build (`CVR_RUST_VRIK`) those checks forward to `cvr_npp_*` (the state mismatch diagnostics, the rate
//! images and the baselines stay the .cpp's); the unit tests below cover each failure.

use cvr_gpu::scene_state::Rate;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;

use crate::geometry_commands::DrawRecord;
use crate::geometry_pass_state::EyeState;

fn bytes<T>(value: &T) -> &[u8] {
    // SAFETY: a plain value's bytes.
    unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) }
}

/// CompatibilityFailure's draw steps before the state (1-7; 0 when they pass).
pub fn draw_compatibility(x: &DrawRecord, y: &DrawRecord, x_stride: u32, y_stride: u32) -> u32 {
    if x.side != 1 || y.side != 0 {
        return 1;
    }
    if x.frame == 0 || x.frame != y.frame {
        return 2;
    }
    if x.pipeline != y.pipeline || x.root != y.root {
        return 3;
    }
    if x.indices != y.indices || x.indices == 0 || x.instances != y.instances || x.instances == 0 || x.first_index != y.first_index || x.base_vertex != y.base_vertex {
        return 4;
    }
    if x.input_slot_mask != y.input_slot_mask || x.input_slot_mask & 128 == 0 || bytes(&x.index) != bytes(&y.index) {
        return 5;
    }
    if bytes(&x.viewport) != bytes(&y.viewport) || bytes(&x.scissor) != bytes(&y.scissor) {
        return 6;
    }
    if x_stride != y_stride {
        return 7;
    }
    0
}
/// CompatibilityFailure's vertex step (9).
pub fn vertex_slots(x: &DrawRecord, y: &DrawRecord) -> bool {
    (0..16).all(|slot| slot == 7 || x.input_slot_mask & (1 << slot) == 0 || bytes(&x.vertices[slot]) == bytes(&y.vertices[slot]))
}
/// SameState.
pub fn same_state(a: &EyeState, b: &EyeState) -> bool {
    a.original != 0
        && a.original == b.original
        && a.topology == D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0
        && a.topology == b.topology
        && a.stencil == b.stencil
        && a.blend == b.blend
        && a.rate.known
        && b.rate.known
        && a.rate.value == b.rate.value
        && a.rate.combiners == b.rate.combiners
}

// ---- the C ABI

/// # Safety
/// Both draws are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_draw_compatibility(x: *const DrawRecord, y: *const DrawRecord, x_stride: u32, y_stride: u32) -> u32 {
    draw_compatibility(unsafe { &*x }, unsafe { &*y }, x_stride, y_stride)
}
/// # Safety
/// Both draws are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_vertex_slots(x: *const DrawRecord, y: *const DrawRecord) -> bool {
    vertex_slots(unsafe { &*x }, unsafe { &*y })
}
/// # Safety
/// The rates are valid; the blend factors hold four floats each.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp_same_state(
    a_original: u64,
    b_original: u64,
    a_rate: *const Rate,
    b_rate: *const Rate,
    a_topology: i32,
    b_topology: i32,
    a_stencil: u32,
    b_stencil: u32,
    a_blend: *const [f32; 4],
    b_blend: *const [f32; 4],
) -> bool {
    let a = EyeState { original: a_original, rate_image: 0, rate: unsafe { *a_rate }, topology: a_topology, stencil: a_stencil, blend: unsafe { *a_blend } };
    let b = EyeState { original: b_original, rate_image: 0, rate: unsafe { *b_rate }, topology: b_topology, stencil: b_stencil, blend: unsafe { *b_blend } };
    same_state(&a, &b)
}

#[cfg(test)]
mod tests {
    //! Each draw failure, the vertex slots and each state condition.
    use super::*;

    fn pair() -> (DrawRecord, DrawRecord) {
        // SAFETY: plain records, all zero.
        let mut x: DrawRecord = unsafe { core::mem::zeroed() };
        x.side = 1;
        x.frame = 9;
        x.indices = 3;
        x.instances = 2;
        x.input_slot_mask = 1 | 128;
        x.vertices[0].BufferLocation = 0x100;
        let mut y = x;
        y.side = 0;
        (x, y)
    }

    #[test]
    fn draw_failures() {
        let (x, y) = pair();
        assert_eq!(draw_compatibility(&x, &y, 48, 48), 0);
        assert_eq!(draw_compatibility(&y, &x, 48, 48), 1);
        assert_eq!(draw_compatibility(&x, &x, 48, 48), 1);
        assert_eq!(draw_compatibility(&DrawRecord { frame: 0, ..x }, &DrawRecord { frame: 0, ..y }, 48, 48), 2);
        assert_eq!(draw_compatibility(&x, &DrawRecord { root: 1, ..y }, 48, 48), 3);
        assert_eq!(draw_compatibility(&DrawRecord { instances: 0, ..x }, &DrawRecord { instances: 0, ..y }, 48, 48), 4);
        assert_eq!(draw_compatibility(&DrawRecord { input_slot_mask: 1, ..x }, &DrawRecord { input_slot_mask: 1, ..y }, 48, 48), 5);
        let mut z = y;
        z.viewport.Width = 1.0;
        assert_eq!(draw_compatibility(&x, &z, 48, 48), 6);
        assert_eq!(draw_compatibility(&x, &y, 48, 64), 7);
        assert!(vertex_slots(&x, &y));
        let mut w = y;
        w.vertices[7].BufferLocation = 9;
        w.vertices[3].BufferLocation = 9;
        assert!(vertex_slots(&x, &w));
        w.vertices[0].BufferLocation = 9;
        assert!(!vertex_slots(&x, &w));
    }
    #[test]
    fn states() {
        let rate = Rate { known: true, ..Rate::default() };
        let a = EyeState { original: 1, topology: D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0, blend: [1.0; 4], rate, ..Default::default() };
        assert!(same_state(&a, &a));
        assert!(!same_state(&EyeState { original: 0, ..a }, &EyeState { original: 0, ..a }));
        assert!(!same_state(&EyeState { topology: 5, ..a }, &EyeState { topology: 5, ..a }));
        assert!(!same_state(&a, &EyeState { stencil: 1, ..a }));
        assert!(!same_state(&a, &EyeState { rate: Rate::default(), ..a }));
        assert!(!same_state(&a, &EyeState { rate: Rate { combiners: [1, 0], ..rate }, ..a }));
        assert!(same_state(&a, &EyeState { rate_image: 5, ..a }));
    }
}
