//! The retained geometry's staged sources (src/Render/NativeRetainedGeometry.cpp, `StagePairSource` and
//! `ValidateStagedSources`): an instance view staged for a source is the source row's own (its scope on the row's list,
//! generation and side at the source's occurrence; its draw the row's index count, instances, first index, base vertex
//! and first instance, words 20..24); and a staged packet's owned geometry is the independently completed one (the same
//! frame, side, pipeline, root, flags, input slots, counts, offsets and the index, vertex, viewport and scissor bytes;
//! the same index lease and the used vertex slots' leases -- resource, GPU address and bytes).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_instance_view` and `cvr_nrg_same_staged` (the scope,
//! draw record and buffer lease mirrored by instance_snapshot_view.rs, geometry_commands.rs and buffer_read.rs); the
//! unit tests below cover each field.

use crate::buffer_read::Lease;
use crate::geometry_commands::DrawRecord;
use crate::instance_snapshot_view::Scope;
use windows::Win32::Graphics::Direct3D12::D3D12_DRAW_INDEXED_ARGUMENTS;

/// The view's identity against the source row.
pub fn instance_view(scope: &Scope, draw: &D3D12_DRAW_INDEXED_ARGUMENTS, saved: &[u64; 48], ordinal: u64) -> bool {
    scope.list == saved[6]
        && scope.generation == saved[7]
        && scope.occurrence == ordinal.wrapping_add(1)
        && u64::from(scope.eye) == saved[1]
        && u64::from(draw.IndexCountPerInstance) == saved[20]
        && u64::from(draw.InstanceCount) == saved[21]
        && u64::from(draw.StartIndexLocation) == saved[22]
        && u64::from(draw.BaseVertexLocation as u32) == saved[23]
        && u64::from(draw.StartInstanceLocation) == saved[24]
}
fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((value as *const T).cast::<u8>(), core::mem::size_of::<T>()) }
}
fn same_lease(a: &Lease, b: &Lease) -> bool {
    a.resource == b.resource && a.gpu == b.gpu && a.bytes == b.bytes
}
/// The staged geometry against the completed one.
pub fn same_staged(a: &DrawRecord, b: &DrawRecord, a_index: &Lease, b_index: &Lease, a_vertices: &[Lease; 16], b_vertices: &[Lease; 16]) -> bool {
    a.frame == b.frame
        && a.side == b.side
        && a.pipeline == b.pipeline
        && a.root == b.root
        && a.flags == b.flags
        && a.input_slot_mask == b.input_slot_mask
        && a.indices == b.indices
        && a.instances == b.instances
        && a.first_index == b.first_index
        && a.base_vertex == b.base_vertex
        && a.first_instance == b.first_instance
        && bytes_of(&a.index) == bytes_of(&b.index)
        && bytes_of(&a.vertices) == bytes_of(&b.vertices)
        && bytes_of(&a.viewport) == bytes_of(&b.viewport)
        && bytes_of(&a.scissor) == bytes_of(&b.scissor)
        && same_lease(a_index, b_index)
        && (0..16).all(|slot| a.input_slot_mask & (1 << slot) == 0 || same_lease(&a_vertices[slot], &b_vertices[slot]))
}

// ---- the C ABI

/// # Safety
/// The records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_view(scope: *const Scope, draw: *const D3D12_DRAW_INDEXED_ARGUMENTS, saved: *const [u64; 48], ordinal: u64) -> bool {
    instance_view(unsafe { &*scope }, unsafe { &*draw }, unsafe { &*saved }, ordinal)
}
/// # Safety
/// The records are valid; the vertex leases are 16 each.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_same_staged(
    a: *const DrawRecord,
    b: *const DrawRecord,
    a_index: *const Lease,
    b_index: *const Lease,
    a_vertices: *const [Lease; 16],
    b_vertices: *const [Lease; 16],
) -> bool {
    unsafe { same_staged(&*a, &*b, &*a_index, &*b_index, &*a_vertices, &*b_vertices) }
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;

    #[test]
    fn views() {
        let mut saved = [0u64; 48];
        saved[1] = 1;
        saved[6] = 10;
        saved[7] = 11;
        saved[20..25].copy_from_slice(&[36, 2, 3, u64::from(-4i32 as u32), 5]);
        let scope = Scope { root: 1, list: 10, generation: 11, occurrence: 8, eye: 1 };
        let draw = D3D12_DRAW_INDEXED_ARGUMENTS { IndexCountPerInstance: 36, InstanceCount: 2, StartIndexLocation: 3, BaseVertexLocation: -4, StartInstanceLocation: 5 };
        assert!(instance_view(&scope, &draw, &saved, 7));
        assert!(!instance_view(&scope, &draw, &saved, 8));
        for change in [
            |s: &mut Scope| s.list = 9,
            |s: &mut Scope| s.generation = 9,
            |s: &mut Scope| s.occurrence = 9,
            |s: &mut Scope| s.eye = 0,
        ] {
            let mut s = Scope { ..scope };
            change(&mut s);
            assert!(!instance_view(&s, &draw, &saved, 7));
        }
        let changes: [fn(&mut D3D12_DRAW_INDEXED_ARGUMENTS); 5] = [
            |d| d.IndexCountPerInstance = 1,
            |d| d.InstanceCount = 1,
            |d| d.StartIndexLocation = 1,
            |d| d.BaseVertexLocation = 4,
            |d| d.StartInstanceLocation = 1,
        ];
        for change in changes {
            let mut d = draw;
            change(&mut d);
            assert!(!instance_view(&scope, &d, &saved, 7));
        }
    }
    fn draw() -> DrawRecord {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.frame = 3;
        d.input_slot_mask = 0b101;
        d.indices = 6;
        d.vertices[0].SizeInBytes = 64;
        d.viewport.Width = 10.0;
        d
    }
    fn lease(gpu: u64) -> Lease {
        let mut l: Lease = unsafe { core::mem::zeroed() };
        l.resource = 1usize as *mut _;
        l.gpu = gpu;
        l.bytes = 64;
        l
    }
    #[test]
    fn staged() {
        let (a, b) = (draw(), draw());
        let vertices: [Lease; 16] = core::array::from_fn(|slot| lease(slot as u64 * 100));
        assert!(same_staged(&a, &b, &lease(1), &lease(1), &vertices, &vertices));
        assert!(!same_staged(&a, &b, &lease(1), &lease(2), &vertices, &vertices));
        let mut other = vertices;
        other[1].gpu = 7;
        assert!(same_staged(&a, &b, &lease(1), &lease(1), &vertices, &other));
        other[2].bytes = 7;
        assert!(!same_staged(&a, &b, &lease(1), &lease(1), &vertices, &other));
        let mut other = vertices;
        other[0].resource = 2usize as *mut _;
        assert!(!same_staged(&a, &b, &lease(1), &lease(1), &vertices, &other));
        let changes: [fn(&mut DrawRecord); 16] = [
            |d| d.frame = 4,
            |d| d.side = 1,
            |d| d.pipeline = 1,
            |d| d.root = 1,
            |d| d.flags = 1,
            |d| d.input_slot_mask = 1,
            |d| d.indices = 1,
            |d| d.instances = 1,
            |d| d.first_index = 1,
            |d| d.base_vertex = 1,
            |d| d.first_instance = 1,
            |d| d.index.Format.0 = 1,
            |d| d.vertices[15].StrideInBytes = 1,
            |d| d.viewport.TopLeftX = -0.0,
            |d| d.scissor.right = 1,
            |d| d.vertices[0].BufferLocation = 1,
        ];
        for (i, change) in changes.into_iter().enumerate() {
            let mut c = draw();
            change(&mut c);
            assert!(!same_staged(&a, &c, &lease(1), &lease(1), &vertices, &vertices), "{i}");
        }
        // Fields outside the comparison.
        let mut c = draw();
        c.sequence = 9;
        c.list = 9;
        c.tables[0] = 9;
        assert!(same_staged(&a, &c, &lease(1), &lease(1), &vertices, &vertices));
    }
}
