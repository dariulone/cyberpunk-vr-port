//! The native early depth's draw checks (src/Render/NativeEarlyDepth.cpp, `early_depth::`): SkinMaterial's resource use
//! contract (each of the vertex CBVs b1 of 848 bytes, b5 of 112, b7 of 448 and b8 of 512 and the vertex SRVs t10 and t77
//! exactly once, in space 0 with one descriptor; an alpha-tested skin's pixel uses skipped) and SameGeometry (the same
//! pipeline, root, index count, first index, base vertex, input slots and instance stride, the same index view, viewport
//! and scissor bytes, stencil reference, topology and blend factor, and the same vertex views in every used slot but the
//! instance stream's).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the use contract forwards to `cvr_ned_skin_uses` and SameGeometry to
//! `cvr_ned_same_geometry`; the unit tests below cover both.

use crate::geometry_commands::DrawRecord;

/// families::ResourceUse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceUse {
    pub kind: u32,
    pub visibility: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
    pub bytes: u32,
}
const _: () = assert!(core::mem::size_of::<ResourceUse>() == 24);

const SRV: u32 = 0;
const CBV: u32 = 2;
const VERTEX: u32 = 1;
const PIXEL: u32 = 5;

/// SkinMaterial's use contract.
pub fn skin_uses(uses: &[ResourceUse], alpha: bool) -> bool {
    let mut mask = 0u32;
    for u in uses {
        if alpha && u.visibility == PIXEL {
            continue;
        }
        if u.visibility != VERTEX || u.space != 0 || u.count != 1 {
            return false;
        }
        let bit = match (u.kind, u.shader_register, u.bytes) {
            (CBV, 1, 848) => 1,
            (CBV, 5, 112) => 2,
            (CBV, 7, 448) => 4,
            (CBV, 8, 512) => 8,
            (SRV, 10, 0) => 16,
            (SRV, 77, 0) => 32,
            _ => 0,
        };
        if bit == 0 || mask & bit != 0 {
            return false;
        }
        mask |= bit;
    }
    mask == 63
}

/// The bindings SameGeometry compares besides the draws.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub stencil: u32,
    pub topology: u32,
    pub blend: [f32; 4],
}

fn bytes<T>(value: &T) -> &[u8] {
    // SAFETY: a plain value's bytes.
    unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) }
}
/// SameGeometry.
pub fn same_geometry(a: &DrawRecord, b: &DrawRecord, a_state: &State, b_state: &State) -> bool {
    if a.pipeline != b.pipeline
        || a.root != b.root
        || a.indices != b.indices
        || a.first_index != b.first_index
        || a.base_vertex != b.base_vertex
        || a.input_slot_mask != b.input_slot_mask
        || a.vertices[7].StrideInBytes != b.vertices[7].StrideInBytes
        || bytes(&a.index) != bytes(&b.index)
        || bytes(&a.viewport) != bytes(&b.viewport)
        || bytes(&a.scissor) != bytes(&b.scissor)
        || a_state.stencil != b_state.stencil
        || a_state.topology != b_state.topology
        || a_state.blend != b_state.blend
    {
        return false;
    }
    (0..16).all(|i| i == 7 || a.input_slot_mask & (1 << i) == 0 || bytes(&a.vertices[i]) == bytes(&b.vertices[i]))
}

// ---- the C ABI

/// # Safety
/// `uses` holds `count` uses.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_skin_uses(uses: *const ResourceUse, count: usize, alpha: bool) -> bool {
    let uses = if uses.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(uses, count) } };
    skin_uses(uses, alpha)
}
/// # Safety
/// The draws are valid; the blend factors hold four floats each.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ned_same_geometry(
    a: *const DrawRecord,
    b: *const DrawRecord,
    a_stencil: u32,
    a_topology: u32,
    a_blend: *const [f32; 4],
    b_stencil: u32,
    b_topology: u32,
    b_blend: *const [f32; 4],
) -> bool {
    let a_state = State { stencil: a_stencil, topology: a_topology, blend: unsafe { *a_blend } };
    let b_state = State { stencil: b_stencil, topology: b_topology, blend: unsafe { *b_blend } };
    same_geometry(unsafe { &*a }, unsafe { &*b }, &a_state, &b_state)
}

#[cfg(test)]
mod tests {
    //! The skin use contract's cases and SameGeometry's slot rules.
    use super::*;

    fn cbv(register: u32, bytes: u32) -> ResourceUse {
        ResourceUse { kind: CBV, visibility: VERTEX, space: 0, shader_register: register, count: 1, bytes }
    }
    fn srv(register: u32) -> ResourceUse {
        ResourceUse { kind: SRV, visibility: VERTEX, space: 0, shader_register: register, count: 1, bytes: 0 }
    }

    #[test]
    fn skin_use_contract() {
        let full = [cbv(1, 848), cbv(5, 112), cbv(7, 448), cbv(8, 512), srv(10), srv(77)];
        assert!(skin_uses(&full, false));
        assert!(!skin_uses(&full[..5], false));
        let mut twice = full.to_vec();
        twice.push(srv(10));
        assert!(!skin_uses(&twice, false));
        let mut pixel = full.to_vec();
        pixel.push(ResourceUse { visibility: PIXEL, ..srv(3) });
        assert!(!skin_uses(&pixel, false));
        assert!(skin_uses(&pixel, true));
        let mut sized = full;
        sized[0].bytes = 832;
        assert!(!skin_uses(&sized, false));
    }
    #[test]
    fn same_geometry_slots() {
        // SAFETY: a plain record, all zero.
        let mut a: DrawRecord = unsafe { core::mem::zeroed() };
        a.pipeline = 1;
        a.input_slot_mask = (1 << 0) | (1 << 7);
        a.vertices[0].BufferLocation = 0x1000;
        a.vertices[7].StrideInBytes = 48;
        let state = State { stencil: 1, topology: 4, blend: [1.0; 4] };
        let mut b = a;
        assert!(same_geometry(&a, &b, &state, &state));
        b.vertices[7].BufferLocation = 0x9000;
        b.vertices[3].BufferLocation = 0x9000;
        assert!(same_geometry(&a, &b, &state, &state));
        b.vertices[0].BufferLocation = 0x2000;
        assert!(!same_geometry(&a, &b, &state, &state));
        b = a;
        b.viewport.MinDepth = -0.0;
        assert!(!same_geometry(&a, &b, &state, &state));
        assert!(!same_geometry(&a, &a, &state, &State { blend: [1.0, 1.0, 1.0, 0.5], ..state }));
        assert!(!same_geometry(&a, &a, &state, &State { stencil: 2, ..state }));
    }
}
