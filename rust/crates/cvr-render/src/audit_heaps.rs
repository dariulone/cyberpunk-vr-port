//! The descriptor audit's shader-visible heaps (src/Render/StereoDescriptorAudit.cpp: CaptureTableMetadata's and
//! CaptureBorrowedSourceRanges' heaps, and both snapshots' Physical): a capture admits a CBV/SRV/UAV or sampler heap that
//! is shader-visible, of 1..limit descriptors on node 0 or 1 (optionally of the capture's device), with nonzero CPU and
//! GPU starts and stride whose span stays in range; a snapshot stays physical while the heap keeps that description,
//! those starts, the device and its stride.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these are `cvr_sda_shader_heap` and `cvr_sda_same_heap`; the unit tests below
//! check them on WARP heaps, and tools/single_pass_tests' descriptor tests run them in their Rust variants.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// A heap's admitted shape.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapShape {
    pub kind: u32,
    pub count: u32,
    pub flags: u32,
    pub node_mask: u32,
    pub cpu: u64,
    pub gpu: u64,
    pub stride: u32,
    pub _pad: u32,
}
const _: () = assert!(core::mem::size_of::<HeapShape>() == 40);

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
fn device_of(heap: &ID3D12DescriptorHeap) -> Option<ID3D12Device> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live heap; the output a local.
    unsafe { heap.GetDevice(&mut device) }.ok()?;
    device
}
/// A capture's heap (with `same_device`, also of `device`).
pub fn shader_heap(device: &ID3D12Device, heap: &ID3D12DescriptorHeap, limit: u32, same_device: bool) -> Option<HeapShape> {
    // SAFETY: a live heap.
    let desc = unsafe { heap.GetDesc() };
    if (desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER)
        || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        || desc.NumDescriptors == 0
        || desc.NumDescriptors > limit
        || desc.NodeMask > 1
    {
        return None;
    }
    if same_device && device_of(heap).map(|d| d.as_raw()) != Some(device.as_raw()) {
        return None;
    }
    // SAFETY: as above; a live device.
    let (cpu, gpu, stride) = unsafe { (heap.GetCPUDescriptorHandleForHeapStart().ptr as u64, heap.GetGPUDescriptorHandleForHeapStart().ptr, device.GetDescriptorHandleIncrementSize(desc.Type)) };
    if cpu == 0 || gpu == 0 || stride == 0 || u64::from(desc.NumDescriptors) > (u64::MAX - cpu).min(u64::MAX - gpu) / u64::from(stride) {
        return None;
    }
    Some(HeapShape { kind: desc.Type.0 as u32, count: desc.NumDescriptors, flags: desc.Flags.0 as u32, node_mask: desc.NodeMask, cpu, gpu, stride, _pad: 0 })
}
/// Physical: the heap still has its captured shape on `device`.
pub fn same_heap(device: &ID3D12Device, heap: &ID3D12DescriptorHeap, shape: &HeapShape) -> bool {
    // SAFETY: a live heap and device.
    let (desc, cpu, gpu) = unsafe { (heap.GetDesc(), heap.GetCPUDescriptorHandleForHeapStart().ptr as u64, heap.GetGPUDescriptorHandleForHeapStart().ptr) };
    desc.Type.0 as u32 == shape.kind
        && desc.NumDescriptors == shape.count
        && desc.Flags.0 as u32 == shape.flags
        && desc.NodeMask == shape.node_mask
        && cpu == shape.cpu
        && gpu == shape.gpu
        && device_of(heap).map(|d| d.as_raw()) == Some(device.as_raw())
        // SAFETY: a live device.
        && unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) } == shape.stride
}

/// A capture's heap into `out` (false when refused).
///
/// # Safety
/// The device and heap are live; `out` a shape.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_shader_heap(device: *mut c_void, heap: *mut c_void, limit: u32, same_device: bool, out: *mut HeapShape) -> bool {
    let (Some(device), Some(heap)) = (borrowed::<ID3D12Device>(device), borrowed::<ID3D12DescriptorHeap>(heap)) else { return false };
    match shader_heap(&device, &heap, limit, same_device) {
        Some(shape) => {
            // SAFETY: as documented.
            unsafe { *out = shape };
            true
        }
        None => false,
    }
}
/// Physical.
///
/// # Safety
/// As cvr_sda_shader_heap; `shape` the captured shape.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_same_heap(device: *mut c_void, heap: *mut c_void, shape: *const HeapShape) -> bool {
    let (Some(device), Some(heap)) = (borrowed::<ID3D12Device>(device), borrowed::<ID3D12DescriptorHeap>(heap)) else { return false };
    // SAFETY: as documented.
    same_heap(&device, &heap, unsafe { &*shape })
}

#[cfg(test)]
mod tests {
    //! WARP heaps: a shader-visible heap admitted, the refused ones, the physical check.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, count: u32, visible: bool) -> ID3D12DescriptorHeap {
        let flags = if visible { D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE } else { D3D12_DESCRIPTOR_HEAP_FLAG_NONE };
        unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: count, Flags: flags, NodeMask: 0 }) }.unwrap()
    }
    #[test]
    fn heaps() {
        let device = warp();
        let views = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 64, true);
        let shape = shader_heap(&device, &views, 1024, true).unwrap();
        assert_eq!((shape.kind, shape.count, shape.flags), (D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 as u32, 64, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE.0 as u32));
        assert!(shape.cpu != 0 && shape.gpu != 0 && shape.stride != 0);
        assert!(same_heap(&device, &views, &shape), "the heap as captured");
        assert!(!same_heap(&device, &views, &HeapShape { gpu: shape.gpu + 64, ..shape }), "another GPU start");
        assert!(!same_heap(&device, &views, &HeapShape { count: 63, ..shape }), "another size");
        assert!(shader_heap(&device, &views, 32, true).is_none(), "beyond the limit");
        let samplers = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, 16, true);
        assert!(shader_heap(&device, &samplers, 1024, true).is_some(), "a sampler heap");
        let hidden = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 16, false);
        assert!(shader_heap(&device, &hidden, 1024, false).is_none(), "not shader-visible");
        let targets = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 16, false);
        assert!(shader_heap(&device, &targets, 1024, false).is_none(), "neither views nor samplers");

    }
}
