//! The native IA snapshot's comparisons and summary (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::`): two resource
//! descriptions alike in every field (dimension, alignment, extent, mips, format, sampling, layout, flags), two heap
//! properties alike (type, CPU page property, memory pool, node masks), a placed source's heap shape (a default heap
//! like the resource's, with the same flags and a non-zero size holding the allocation at its creation offset, at the
//! default or MSAA placement alignment or none), and the observations' summary (the rows admitted and the first
//! refusal).
//!
//! In the Rust build (`CVR_RUST_VRIK`) SameDesc, SameHeap, PlacementShape's heap checks and Summarize's count forward to
//! `cvr_ias_*`; `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against them in its Rust variant, and
//! the unit tests below cover each comparison.

use windows::Win32::Graphics::Direct3D12::*;

/// SameDesc.
pub fn same_desc(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Alignment == b.Alignment
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.SampleDesc.Count == b.SampleDesc.Count
        && a.SampleDesc.Quality == b.SampleDesc.Quality
        && a.Layout == b.Layout
        && a.Flags == b.Flags
}
/// SameHeap.
pub fn same_heap(a: &D3D12_HEAP_PROPERTIES, b: &D3D12_HEAP_PROPERTIES) -> bool {
    a.Type == b.Type
        && a.CPUPageProperty == b.CPUPageProperty
        && a.MemoryPoolPreference == b.MemoryPoolPreference
        && a.CreationNodeMask == b.CreationNodeMask
        && a.VisibleNodeMask == b.VisibleNodeMask
}
/// PlacementShape's heap checks (the heap's protection is the .cpp's).
pub fn placement_heap(heap: &D3D12_HEAP_DESC, resource_heap: &D3D12_HEAP_PROPERTIES, resource_flags: D3D12_HEAP_FLAGS, offset: u64, bytes: u64) -> bool {
    heap.Properties.Type == D3D12_HEAP_TYPE_DEFAULT
        && same_heap(&heap.Properties, resource_heap)
        && heap.Flags == resource_flags
        && heap.SizeInBytes != 0
        && offset <= heap.SizeInBytes
        && bytes <= heap.SizeInBytes - offset
        && (heap.Alignment == 0 || heap.Alignment == D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT as u64 || heap.Alignment == D3D12_DEFAULT_MSAA_RESOURCE_PLACEMENT_ALIGNMENT as u64)
}
/// Summarize's count: the reasons' admitted rows (reason 0) and the first refusal (0 when none).
pub fn summarize(reasons: impl Iterator<Item = u64>) -> (u32, u64) {
    let (mut admitted, mut first) = (0u32, 0u64);
    for reason in reasons {
        if reason == 0 {
            admitted += 1;
        } else if first == 0 {
            first = reason;
        }
    }
    (admitted, first)
}

// ---- the C ABI

/// # Safety
/// Both descriptions are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_same_desc(a: *const D3D12_RESOURCE_DESC, b: *const D3D12_RESOURCE_DESC) -> bool {
    same_desc(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// Both properties are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_same_heap(a: *const D3D12_HEAP_PROPERTIES, b: *const D3D12_HEAP_PROPERTIES) -> bool {
    same_heap(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// `heap` and `resource_heap` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_placement_heap(heap: *const D3D12_HEAP_DESC, resource_heap: *const D3D12_HEAP_PROPERTIES, resource_flags: i32, offset: u64, bytes: u64) -> bool {
    placement_heap(unsafe { &*heap }, unsafe { &*resource_heap }, D3D12_HEAP_FLAGS(resource_flags), offset, bytes)
}
/// The rows' admitted count and first refusal: each row is 64 values with its reason at index 2.
///
/// # Safety
/// `rows` holds `count` rows of 64 values; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_summarize(rows: *const [u64; 64], count: usize, admitted: *mut u32, first: *mut u64) {
    let rows = if rows.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(rows, count) } };
    let (a, f) = summarize(rows.iter().map(|row| row[2]));
    unsafe {
        *admitted = a;
        *first = f;
    }
}

#[cfg(test)]
mod tests {
    //! Each comparison's fields, the placement's bounds and alignments, and the summary.
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8_UNORM, DXGI_SAMPLE_DESC};

    #[test]
    fn descriptions_and_heaps() {
        let a = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: 64, Height: 1, DepthOrArraySize: 1, MipLevels: 1, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR, ..Default::default() };
        assert!(same_desc(&a, &a));
        assert!(!same_desc(&a, &D3D12_RESOURCE_DESC { Format: DXGI_FORMAT_R8_UNORM, ..a }));
        assert!(!same_desc(&a, &D3D12_RESOURCE_DESC { Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, ..a }));
        assert!(!same_desc(&a, &D3D12_RESOURCE_DESC { SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 1 }, ..a }));
        let h = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() };
        assert!(same_heap(&h, &h));
        assert!(!same_heap(&h, &D3D12_HEAP_PROPERTIES { VisibleNodeMask: 3, ..h }));
        assert!(!same_heap(&h, &D3D12_HEAP_PROPERTIES { CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_WRITE_BACK, ..h }));
    }
    #[test]
    fn placement() {
        let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let heap = D3D12_HEAP_DESC { SizeInBytes: 0x20000, Properties: props, Alignment: 0, Flags: D3D12_HEAP_FLAG_NONE };
        assert!(placement_heap(&heap, &props, D3D12_HEAP_FLAG_NONE, 0x10000, 0x10000));
        assert!(!placement_heap(&heap, &props, D3D12_HEAP_FLAG_NONE, 0x10000, 0x10001));
        assert!(!placement_heap(&heap, &props, D3D12_HEAP_FLAG_NONE, 0x20001, 0));
        assert!(!placement_heap(&heap, &props, D3D12_HEAP_FLAG_DENY_BUFFERS, 0, 1));
        assert!(placement_heap(&D3D12_HEAP_DESC { Alignment: 0x400000, ..heap }, &props, D3D12_HEAP_FLAG_NONE, 0, 1));
        assert!(!placement_heap(&D3D12_HEAP_DESC { Alignment: 0x1000, ..heap }, &props, D3D12_HEAP_FLAG_NONE, 0, 1));
        assert!(!placement_heap(&D3D12_HEAP_DESC { SizeInBytes: 0, ..heap }, &props, D3D12_HEAP_FLAG_NONE, 0, 0));
        let upload = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
        assert!(!placement_heap(&D3D12_HEAP_DESC { Properties: upload, ..heap }, &upload, D3D12_HEAP_FLAG_NONE, 0, 1));
    }
    #[test]
    fn summaries() {
        assert_eq!(summarize([0u64, 5, 0, 7].into_iter()), (2, 5));
        assert_eq!(summarize([0u64, 0].into_iter()), (2, 0));
        assert_eq!(summarize(core::iter::empty()), (0, 0));
    }
}
