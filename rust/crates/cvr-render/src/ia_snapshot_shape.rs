//! The native IA snapshot's source shape (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Shape`): a bound source's
//! resource description and heap read into its observation (resource, GPU address, width, heap type and flags, resource
//! flags, dimension, alignment, extent, mips, node masks; a known shape is an unformatted single-sample row-major buffer
//! on a custom-free heap), its protection and session results, the same-device flag, the view's offset into the
//! resource, and the shape agreeing with the source's lease (GPU address, bytes, heap type, flags).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Shape forwards to `cvr_ias_shape` (ia_snapshot's observation mirror);
//! `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against it in its Rust variant, and the unit test
//! below shapes a WARP buffer.

use core::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

use crate::ia_snapshot_objects::{same_device, unprotected};

pub use crate::ia_snapshot::Observation;

/// detail::SameDevice.
const SAME_DEVICE: u64 = 16;

/// The source's lease as Shape compares it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lease {
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
}

/// Shape: the observation, description, heap and heap flags filled; false when the heap is unreadable or the shape
/// disagrees with the lease.
pub fn shape(
    resource: &ID3D12Resource,
    list: Option<&ID3D12GraphicsCommandList>,
    lease: &Lease,
    a: &mut Observation,
    desc: &mut D3D12_RESOURCE_DESC,
    heap: &mut D3D12_HEAP_PROPERTIES,
    heap_flags: &mut D3D12_HEAP_FLAGS,
) -> bool {
    // SAFETY: a live resource.
    *desc = unsafe { resource.GetDesc() };
    *heap_flags = D3D12_HEAP_FLAGS(0);
    // SAFETY: a live resource; the outputs are the caller's.
    if unsafe { resource.GetHeapProperties(Some(heap), Some(heap_flags)) }.is_err() {
        return false;
    }
    a.resource = resource.as_raw() as u64;
    // SAFETY: a live resource.
    a.gpu = unsafe { resource.GetGPUVirtualAddress() };
    a.width = desc.Width;
    a.heap = heap.Type.0 as u64;
    a.heap_flags = heap_flags.0 as u64;
    a.resource_flags = desc.Flags.0 as u64;
    a.dimension = desc.Dimension.0 as u64;
    a.alignment = desc.Alignment;
    a.height = desc.Height as u64;
    a.depth = desc.DepthOrArraySize as u64;
    a.mips = desc.MipLevels as u64;
    a.heap_node_masks = ((heap.CreationNodeMask as u64) << 32) | heap.VisibleNodeMask as u64;
    a.shape_known = (desc.Format == DXGI_FORMAT_UNKNOWN
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && desc.Layout == D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        && heap.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_UNKNOWN
        && heap.MemoryPoolPreference == D3D12_MEMORY_POOL_UNKNOWN) as u64;
    let (ok, result, present) = unprotected(resource);
    a.unprotected = ok as u64;
    a.protected_result = result;
    a.session_present = present;
    if list.is_some_and(|list| same_device(list, resource)) {
        a.flags |= SAME_DEVICE;
    }
    if a.view_gpu >= a.gpu {
        a.com_offset = a.view_gpu - a.gpu;
    }
    a.gpu == lease.gpu && a.width == lease.bytes && a.heap == lease.heap_type as u64 && a.resource_flags == lease.flags as u64
}

// ---- the C ABI

/// # Safety
/// `resource` is live (or null: false); `list` is live or null; the outputs are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ias_shape(
    resource: *mut c_void,
    list: *mut c_void,
    gpu: u64,
    bytes: u64,
    heap_type: u32,
    flags: u32,
    observation: *mut Observation,
    desc: *mut D3D12_RESOURCE_DESC,
    heap: *mut D3D12_HEAP_PROPERTIES,
    heap_flags: *mut D3D12_HEAP_FLAGS,
) -> bool {
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) };
    unsafe { shape(resource, list, &Lease { gpu, bytes, heap_type, flags }, &mut *observation, &mut *desc, &mut *heap, &mut *heap_flags) }
}

#[cfg(test)]
mod tests {
    //! A WARP upload buffer shaped against its lease.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn warp_buffer_shape() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 512,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut buffer: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut buffer) }.unwrap();
        let buffer = buffer.unwrap();
        let gpu = unsafe { buffer.GetGPUVirtualAddress() };
        let lease = Lease { gpu, bytes: 512, heap_type: D3D12_HEAP_TYPE_UPLOAD.0 as u32, flags: 0 };
        let (mut a, mut d, mut h, mut f) = (Observation { view_gpu: gpu + 64, ..Default::default() }, D3D12_RESOURCE_DESC::default(), D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS(0));
        assert!(shape(&buffer, Some(&list), &lease, &mut a, &mut d, &mut h, &mut f));
        assert_eq!((a.width, a.heap, a.com_offset, a.mips, a.depth, a.flags & SAME_DEVICE, a.unprotected), (512, D3D12_HEAP_TYPE_UPLOAD.0 as u64, 64, 1, 1, SAME_DEVICE, 1));
        assert_eq!(a.resource, buffer.as_raw() as u64);
        assert!(!shape(&buffer, Some(&list), &Lease { bytes: 256, ..lease }, &mut a, &mut d, &mut h, &mut f));
        assert!(!shape(&buffer, None, &Lease { heap_type: 1, ..lease }, &mut a, &mut d, &mut h, &mut f));
    }
}
