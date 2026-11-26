//! The native IA snapshot's observation seed and placed heap re-check (src/Render/NativeIaSnapshot.cpp,
//! `ia_snapshot::Collect` and `Physical`): a new source's observation takes its role, slot, list, generation and
//! occurrence and its bound view (an index view's address, bytes and format, or a vertex view's address, bytes and
//! stride); and a placed source's heap is still the one it was captured on (size, alignment, properties, flags).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Collect's seed forwards to `cvr_ias_seed` and Physical's heap comparison to
//! `cvr_ias_same_heap_desc`; the unit tests below cover them.

use windows::Win32::Graphics::Direct3D12::{D3D12_HEAP_DESC, D3D12_INDEX_BUFFER_VIEW, D3D12_VERTEX_BUFFER_VIEW};

use crate::ia_snapshot::Observation;
use crate::ia_snapshot_checks::same_heap;

/// Collect's seed: role 0 (index) takes the index view, otherwise the vertex view.
pub fn seed(a: &mut Observation, role: u64, slot: u64, list: u64, generation: u64, occurrence: u64, index: &D3D12_INDEX_BUFFER_VIEW, vertex: &D3D12_VERTEX_BUFFER_VIEW) {
    a.role = role;
    a.slot = slot;
    a.list = list;
    a.generation = generation;
    a.occurrence = occurrence;
    if role == 0 {
        a.view_gpu = index.BufferLocation;
        a.view_bytes = index.SizeInBytes as u64;
        a.stride_or_format = index.Format.0 as u32 as u64;
    } else {
        a.view_gpu = vertex.BufferLocation;
        a.view_bytes = vertex.SizeInBytes as u64;
        a.stride_or_format = vertex.StrideInBytes as u64;
    }
}
/// Physical's placed heap comparison.
pub fn same_heap_desc(a: &D3D12_HEAP_DESC, b: &D3D12_HEAP_DESC) -> bool {
    a.SizeInBytes == b.SizeInBytes && a.Alignment == b.Alignment && same_heap(&a.Properties, &b.Properties) && a.Flags == b.Flags
}

// ---- the C ABI

/// # Safety
/// All pointers are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ias_seed(
    a: *mut Observation,
    role: u64,
    slot: u64,
    list: u64,
    generation: u64,
    occurrence: u64,
    index: *const D3D12_INDEX_BUFFER_VIEW,
    vertex: *const D3D12_VERTEX_BUFFER_VIEW,
) {
    unsafe { seed(&mut *a, role, slot, list, generation, occurrence, &*index, &*vertex) };
}
/// # Safety
/// Both descriptions are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ias_same_heap_desc(a: *const D3D12_HEAP_DESC, b: *const D3D12_HEAP_DESC) -> bool {
    same_heap_desc(unsafe { &*a }, unsafe { &*b })
}

#[cfg(test)]
mod tests {
    //! Both seeds and each heap field.
    use super::*;
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16_UINT;

    #[test]
    fn seeds() {
        let index = D3D12_INDEX_BUFFER_VIEW { BufferLocation: 0x100, SizeInBytes: 64, Format: DXGI_FORMAT_R16_UINT };
        let vertex = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x200, SizeInBytes: 96, StrideInBytes: 12 };
        let mut a = Observation::default();
        seed(&mut a, 0, u32::MAX as u64, 7, 8, 9, &index, &vertex);
        assert_eq!((a.role, a.slot, a.list, a.generation, a.occurrence, a.view_gpu, a.view_bytes, a.stride_or_format), (0, u32::MAX as u64, 7, 8, 9, 0x100, 64, DXGI_FORMAT_R16_UINT.0 as u64));
        seed(&mut a, 1, 3, 7, 8, 9, &index, &vertex);
        assert_eq!((a.slot, a.view_gpu, a.view_bytes, a.stride_or_format), (3, 0x200, 96, 12));
    }
    #[test]
    fn heaps() {
        let h = D3D12_HEAP_DESC { SizeInBytes: 0x10000, Properties: D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() }, Alignment: 0, Flags: D3D12_HEAP_FLAG_NONE };
        assert!(same_heap_desc(&h, &h));
        assert!(!same_heap_desc(&h, &D3D12_HEAP_DESC { SizeInBytes: 0x20000, ..h }));
        assert!(!same_heap_desc(&h, &D3D12_HEAP_DESC { Alignment: 0x400000, ..h }));
        assert!(!same_heap_desc(&h, &D3D12_HEAP_DESC { Flags: D3D12_HEAP_FLAG_DENY_BUFFERS, ..h }));
        assert!(!same_heap_desc(&h, &D3D12_HEAP_DESC { Properties: D3D12_HEAP_PROPERTIES { VisibleNodeMask: 1, ..h.Properties }, ..h }));
    }
}
