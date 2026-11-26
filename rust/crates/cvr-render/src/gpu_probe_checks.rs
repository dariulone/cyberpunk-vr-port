//! The stereo GPU probe's draw checks (src/Render/StereoGpuProbe.cpp, `SameGeometry` and `SamePass`): two draws are the
//! same geometry when they share the frame, pipeline, root, index count, instances, first index, base vertex, vertex
//! mask, render target count, depth, targets and index view; then either their instance payloads agree (the .cpp's
//! instance_data::Equal when payloads were captured) or, without payloads, both are single instances carrying the same
//! non-empty inline instance data; and every used vertex view but slot 7's is byte-equal. A draw stays in the pass when
//! it shares the frame, root, render targets, depth, viewport and scissor bytes (and the pipeline unless the pass is
//! mixed), binds its targets separately (or none) and carries the pass flags (the low seven bits exactly 63).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_sgpp_same_fields`, `cvr_sgpp_inline_instances`,
//! `cvr_sgpp_vertex_views` and `cvr_sgpp_same_pass` (the draw record mirrored by geometry_commands.rs); the unit tests
//! below cover each field.

use crate::geometry_commands::DrawRecord;

fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((value as *const T).cast::<u8>(), core::mem::size_of::<T>()) }
}
/// SameGeometry's field checks.
pub fn same_fields(a: &DrawRecord, b: &DrawRecord) -> bool {
    a.frame == b.frame
        && a.pipeline == b.pipeline
        && a.root == b.root
        && a.indices == b.indices
        && a.instances == b.instances
        && a.first_index == b.first_index
        && a.base_vertex == b.base_vertex
        && a.vertex_mask == b.vertex_mask
        && a.rt_count == b.rt_count
        && a.depth == b.depth
        && a.targets == b.targets
        && bytes_of(&a.index) == bytes_of(&b.index)
}
/// SameGeometry without captured payloads: single instances with the same non-empty inline data.
pub fn inline_instances(a: &DrawRecord, b: &DrawRecord) -> bool {
    a.instances == 1 && a.instance_data_bytes != 0 && a.instance_data_bytes == b.instance_data_bytes && a.instance_data == b.instance_data
}
/// SameGeometry's vertex views: every used slot but 7.
pub fn vertex_views(a: &DrawRecord, b: &DrawRecord) -> bool {
    (0..16).all(|i| i == 7 || a.vertex_mask & (1 << i) == 0 || bytes_of(&a.vertices[i]) == bytes_of(&b.vertices[i]))
}
/// SamePass.
pub fn same_pass(a: &DrawRecord, b: &DrawRecord, mixed: bool) -> bool {
    a.frame == b.frame
        && (mixed || a.pipeline == b.pipeline)
        && a.root == b.root
        && a.rt_count == b.rt_count
        && a.targets == b.targets
        && a.depth == b.depth
        && (b.rt_count == 0 || b.rt_contiguous == 0)
        && b.flags & 127 == 63
        && bytes_of(&a.viewport) == bytes_of(&b.viewport)
        && bytes_of(&a.scissor) == bytes_of(&b.scissor)
}

// ---- the C ABI

/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpp_same_fields(a: *const DrawRecord, b: *const DrawRecord) -> bool {
    unsafe { same_fields(&*a, &*b) }
}
/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpp_inline_instances(a: *const DrawRecord, b: *const DrawRecord) -> bool {
    unsafe { inline_instances(&*a, &*b) }
}
/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpp_vertex_views(a: *const DrawRecord, b: *const DrawRecord) -> bool {
    unsafe { vertex_views(&*a, &*b) }
}
/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpp_same_pass(a: *const DrawRecord, b: *const DrawRecord, mixed: bool) -> bool {
    unsafe { same_pass(&*a, &*b, mixed) }
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;

    fn draw() -> DrawRecord {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.frame = 4;
        d.pipeline = 5;
        d.root = 6;
        d.indices = 36;
        d.instances = 1;
        d.vertex_mask = 0b1000_0001;
        d.rt_count = 3;
        d.flags = 63 | 128;
        d.instance_data_bytes = 16;
        d.instance_data[0] = 9;
        d.viewport.Width = 10.0;
        d
    }
    #[test]
    fn fields() {
        let a = draw();
        assert!(same_fields(&a, &draw()));
        let changes: [fn(&mut DrawRecord); 12] = [
            |d| d.frame = 5,
            |d| d.pipeline = 1,
            |d| d.root = 1,
            |d| d.indices = 1,
            |d| d.instances = 2,
            |d| d.first_index = 1,
            |d| d.base_vertex = -1,
            |d| d.vertex_mask = 1,
            |d| d.rt_count = 2,
            |d| d.depth = 1,
            |d| d.targets[7] = 1,
            |d| d.index.SizeInBytes = 1,
        ];
        for (i, change) in changes.into_iter().enumerate() {
            let mut b = draw();
            change(&mut b);
            assert!(!same_fields(&a, &b), "{i}");
        }
        let mut b = draw();
        b.viewport.Width = 11.0;
        b.list = 3;
        assert!(same_fields(&a, &b));
    }
    #[test]
    fn instances() {
        let a = draw();
        assert!(inline_instances(&a, &draw()));
        let mut b = draw();
        b.instance_data[63] = 1;
        assert!(!inline_instances(&a, &b));
        let mut b = draw();
        b.instance_data_bytes = 8;
        assert!(!inline_instances(&a, &b));
        let mut c = draw();
        c.instances = 2;
        assert!(!inline_instances(&c, &c));
        c.instances = 1;
        c.instance_data_bytes = 0;
        assert!(!inline_instances(&c, &c));
    }
    #[test]
    fn views() {
        let a = draw();
        let mut b = draw();
        b.vertices[7].StrideInBytes = 4;
        b.vertices[3].StrideInBytes = 4;
        assert!(vertex_views(&a, &b));
        b.vertices[0].SizeInBytes = 4;
        assert!(!vertex_views(&a, &b));
    }
    #[test]
    fn passes() {
        let a = draw();
        assert!(same_pass(&a, &draw(), false));
        let mut b = draw();
        b.pipeline = 1;
        assert!(!same_pass(&a, &b, false));
        assert!(same_pass(&a, &b, true));
        let changes: [fn(&mut DrawRecord); 8] = [
            |d| d.frame = 1,
            |d| d.root = 1,
            |d| d.rt_count = 2,
            |d| d.targets[0] = 1,
            |d| d.depth = 1,
            |d| d.rt_contiguous = 1,
            |d| d.flags = 62,
            |d| d.scissor.left = 1,
        ];
        for (i, change) in changes.into_iter().enumerate() {
            let mut b = draw();
            change(&mut b);
            assert!(!same_pass(&a, &b, true), "{i}");
        }
        let mut b = draw();
        b.viewport.MinDepth = -0.0;
        assert!(!same_pass(&a, &b, true));
        let mut b = draw();
        b.rt_count = 0;
        b.rt_contiguous = 1;
        let mut c = draw();
        c.rt_count = 0;
        assert!(same_pass(&c, &b, false));
    }
}
