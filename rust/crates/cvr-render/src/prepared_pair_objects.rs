//! The prepared pair's own D3D12 objects (src/Render/NativePreparedPair.cpp): StartInternal's pass objects -- a
//! pipeline-statistics and an occlusion query heap of three queries each, and the 8-byte UPLOAD predicate word written
//! 0; RecordCurrent's and PrepareNativeExport's direct allocator and list (and the current pass's fence);
//! PrepareGeometry's descriptor tables -- the membership B4's one-CBV heap, and a reference table's CPU copy with its
//! private SRVs written over the copied descriptors. All created as the port's own.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the objects it is handed; the unit test below creates them on WARP.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// A private SRV over a copied table: its slot, resource and view.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TableSrv {
    pub index: u32,
    pub resource: *mut c_void,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
}
const _: () = assert!(core::mem::size_of::<TableSrv>() == 56);

/// The pass's query heaps and its predicate word (written 0).
pub fn pass_objects(device: &ID3D12Device) -> Option<(ID3D12QueryHeap, ID3D12QueryHeap, ID3D12Resource)> {
    let _internal = cvr_host::probe::Internal::new();
    let mut desc = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS, Count: 3, NodeMask: 0 };
    let mut pipeline: Option<ID3D12QueryHeap> = None;
    // SAFETY: a live device; the description a local.
    unsafe { device.CreateQueryHeap(&desc, &mut pipeline) }.ok()?;
    desc.Type = D3D12_QUERY_HEAP_TYPE_OCCLUSION;
    let mut occlusion: Option<ID3D12QueryHeap> = None;
    // SAFETY: as above.
    unsafe { device.CreateQueryHeap(&desc, &mut occlusion) }.ok()?;
    let predicate = crate::readback_buffers::buffer(device, None, 8, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).ok()?;
    if !crate::prepared_pair_record::write_predicate(&predicate, 0) {
        return None;
    }
    Some((pipeline?, occlusion?, predicate))
}
/// A direct allocator and its open list, and with `fence` a fence at 0; each into its slot as created.
pub fn command_objects(device: &ID3D12Device, allocator: &mut Option<ID3D12CommandAllocator>, list: &mut Option<ID3D12GraphicsCommandList>, fence: Option<&mut Option<ID3D12Fence>>) -> bool {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: a live device.
    let Ok(created) = (unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) }) else { return false };
    *allocator = Some(created.clone());
    // SAFETY: as above; the allocator just created.
    let Ok(opened) = (unsafe { device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &created, None) }) else { return false };
    *list = Some(opened);
    match fence {
        // SAFETY: as above.
        Some(slot) => match unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) } {
            Ok(value) => {
                *slot = Some(value);
                true
            }
            Err(_) => false,
        },
        None => true,
    }
}
/// A one-descriptor CPU heap holding `view`.
pub fn cbv_heap(device: &ID3D12Device, view: &D3D12_CONSTANT_BUFFER_VIEW_DESC) -> Option<ID3D12DescriptorHeap> {
    let _internal = cvr_host::probe::Internal::new();
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 1, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
    // SAFETY: a live device; the description a local.
    let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.ok()?;
    // SAFETY: the heap's one descriptor.
    unsafe { device.CreateConstantBufferView(Some(view), heap.GetCPUDescriptorHandleForHeapStart()) };
    Some(heap)
}
/// A CPU copy of `count` descriptors of `source` from `offset`, `views` written over it at their slots.
pub fn table_heap(device: &ID3D12Device, source: &ID3D12DescriptorHeap, offset: u32, count: u32, views: &[TableSrv]) -> Option<ID3D12DescriptorHeap> {
    // SAFETY: a live heap.
    let kind = unsafe { source.GetDesc() }.Type;
    let _internal = cvr_host::probe::Internal::new();
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: count, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
    // SAFETY: a live device; the description a local.
    let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.ok()?;
    // SAFETY: live heaps; the copy of the table's own descriptors.
    unsafe {
        let stride = device.GetDescriptorHandleIncrementSize(kind) as usize;
        let from = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: source.GetCPUDescriptorHandleForHeapStart().ptr + offset as usize * stride };
        let start = heap.GetCPUDescriptorHandleForHeapStart().ptr;
        device.CopyDescriptorsSimple(count, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start }, from, kind);
        for view in views {
            let resource = ID3D12Resource::from_raw_borrowed(&view.resource);
            device.CreateShaderResourceView(resource, Some(&view.view), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start + view.index as usize * stride });
        }
    }
    Some(heap)
}

fn device_of(device: *mut c_void) -> Option<ID3D12Device> {
    // SAFETY: a live device or null.
    unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned()
}
fn put<T: Interface>(slot: *mut *mut c_void, value: Option<T>) {
    if let Some(value) = value {
        // SAFETY: the caller's slot.
        unsafe { *slot = value.into_raw() };
    }
}

/// The pass objects into their slots (references, on success).
///
/// # Safety
/// A live device; the slots writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_pass_objects(device: *mut c_void, pipeline: *mut *mut c_void, occlusion: *mut *mut c_void, predicate: *mut *mut c_void) -> bool {
    let Some(device) = device_of(device) else { return false };
    let Some((a, b, c)) = pass_objects(&device) else { return false };
    put(pipeline, Some(a));
    put(occlusion, Some(b));
    put(predicate, Some(c));
    true
}
/// The command objects into their slots (references, as created; `fence` null for none).
///
/// # Safety
/// A live device; the slots writable, `fence` null or writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_command_objects(device: *mut c_void, allocator: *mut *mut c_void, list: *mut *mut c_void, fence: *mut *mut c_void) -> bool {
    let Some(device) = device_of(device) else { return false };
    let (mut a, mut l, mut f) = (None, None, None);
    let ok = command_objects(&device, &mut a, &mut l, if fence.is_null() { None } else { Some(&mut f) });
    put(allocator, a);
    put(list, l);
    if !fence.is_null() {
        put(fence, f);
    }
    ok
}
/// The one-CBV heap into `out`.
///
/// # Safety
/// A live device; `view` lent; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_cbv_heap(device: *mut c_void, view: *const D3D12_CONSTANT_BUFFER_VIEW_DESC, out: *mut *mut c_void) -> bool {
    let Some(device) = device_of(device) else { return false };
    // SAFETY: as documented.
    let heap = cbv_heap(&device, unsafe { &*view });
    let ok = heap.is_some();
    put(out, heap);
    ok
}
/// The table's CPU copy into `out`.
///
/// # Safety
/// A live device and source heap; `views` holds `count`; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_table_heap(device: *mut c_void, source: *mut c_void, offset: u32, count: u32, views: *const TableSrv, view_count: usize, out: *mut *mut c_void) -> bool {
    let Some(device) = device_of(device) else { return false };
    // SAFETY: as documented.
    let Some(source) = (unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&source) }) else { return false };
    let views = if views.is_null() || view_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(views, view_count) } };
    let heap = table_heap(&device, source, offset, count, views);
    let ok = heap.is_some();
    put(out, heap);
    ok
}

#[cfg(test)]
mod tests {
    //! WARP: the pass objects, the command objects, a CBV heap and a table copy with a private SRV.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn prepared_pair_objects() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let (_, _, predicate) = pass_objects(&device).unwrap();
        let mut mapped = core::ptr::null_mut();
        unsafe { predicate.Map(0, None, Some(&mut mapped)) }.unwrap();
        assert_eq!((unsafe { *(mapped as *const u64) }, unsafe { predicate.GetDesc() }.Width), (0, 8));
        unsafe { predicate.Unmap(0, None) };
        let (mut a, mut l, mut f) = (None, None, None);
        assert!(command_objects(&device, &mut a, &mut l, Some(&mut f)));
        assert!(a.is_some() && f.is_some() && unsafe { l.as_ref().unwrap().GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT);
        let (mut a2, mut l2) = (None, None);
        assert!(command_objects(&device, &mut a2, &mut l2, None) && l2.is_some());

        let buffer = crate::readback_buffers::buffer(&device, None, 65536, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let cbv = D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: unsafe { buffer.GetGPUVirtualAddress() }, SizeInBytes: 256 };
        let one = cbv_heap(&device, &cbv).unwrap();
        assert_eq!(unsafe { one.GetDesc() }.NumDescriptors, 1);
        let source: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 16, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 })
        }
        .unwrap();
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_UNKNOWN,
            ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            ..Default::default()
        };
        view.Anonymous.Buffer = D3D12_BUFFER_SRV { FirstElement: 0, NumElements: 16, StructureByteStride: 16, Flags: D3D12_BUFFER_SRV_FLAG_NONE };
        let table = table_heap(&device, &source, 4, 8, &[TableSrv { index: 5, resource: buffer.as_raw(), view }]).unwrap();
        let desc = unsafe { table.GetDesc() };
        assert_eq!((desc.Type, desc.NumDescriptors, desc.Flags), (D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 8, D3D12_DESCRIPTOR_HEAP_FLAG_NONE));
    }
}
