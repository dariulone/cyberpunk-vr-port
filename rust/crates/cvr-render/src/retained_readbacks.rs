//! The retained geometry's readbacks (src/Render/NativeRetainedGeometry.cpp, Seal and FullT5Readback): a READBACK
//! buffer in COPY_DEST of `bytes` -- shaped like a source buffer, or plain -- created as the port's own, kept by the
//! list with the source it copies; with a source, its first `bytes` copied into it on the list.
//!
//! In the Rust build (`CVR_RUST_VRIK`) Seal's CBV and T5 readbacks and the full T5 readback come here; the unit test
//! below copies on WARP.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// The readback (None when it cannot be created) and whether the list keeps it and the copy was recorded.
pub fn readback(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, like: Option<&ID3D12Resource>, bytes: u64, copy: bool) -> (Option<ID3D12Resource>, bool) {
    let _internal = cvr_host::probe::Internal::new();
    let Ok(target) = crate::readback_buffers::buffer(device, like, bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST) else { return (None, false) };
    let source = like.filter(|_| copy);
    let kept = match source {
        Some(source) => cvr_gpu::command_resources::keep(list, &[source.as_raw(), target.as_raw()]),
        None => cvr_gpu::command_resources::keep(list, &[target.as_raw()]),
    };
    if !kept {
        return (Some(target), false);
    }
    if let Some(source) = source {
        // SAFETY: a live recording list; both buffers live, the source at least `bytes` long.
        unsafe { list.CopyBufferRegion(&target, 0, source, 0, bytes) };
    }
    (Some(target), true)
}

/// The readback into `out` (a reference, when created); whether kept and copied.
///
/// # Safety
/// A live device and list; `like` a live buffer or null; `out` a slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_readback(device: *mut c_void, list: *mut c_void, like: *mut c_void, bytes: u64, copy: bool, out: *mut *mut c_void) -> bool {
    // SAFETY: as documented.
    let (Some(device), Some(list)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return false };
    let like = unsafe { ID3D12Resource::from_raw_borrowed(&like) };
    let (target, ok) = readback(device, list, like, bytes, copy);
    if let Some(target) = target {
        // SAFETY: the caller's slot.
        unsafe { *out = target.into_raw() };
    }
    ok
}

/// Draw's pass-depth baseline: the allocation bytes of the target slots in `slots` (bit 4 the depth, the others colour)
/// as render or depth targets, added to `total` in slot order; false at the first that is missing, empty or past
/// `budget`.
pub fn target_bytes(device: &ID3D12Device, resources: &[*mut c_void; 5], slots: u32, budget: u64, total: &mut u64) -> bool {
    for (slot, &raw) in resources.iter().enumerate() {
        if slots & (1 << slot) == 0 {
            continue;
        }
        // SAFETY: a live target of the draw or null.
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else { return false };
        // SAFETY: a live resource and device.
        let mut desc = unsafe { resource.GetDesc() };
        desc.Alignment = 0;
        desc.Flags = if slot == 4 { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET };
        let size = unsafe { device.GetResourceAllocationInfo(0, &[desc]) }.SizeInBytes;
        if size == 0 || size > budget - *total {
            return false;
        }
        *total += size;
    }
    true
}
/// The baseline's bytes.
///
/// # Safety
/// A live device; `resources` holds five targets (null outside `slots`); `total` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_target_bytes(device: *mut c_void, resources: *const [*mut c_void; 5], slots: u32, budget: u64, total: *mut u64) -> bool {
    // SAFETY: as documented.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else { return false };
    unsafe { target_bytes(device, &*resources, slots, budget, &mut *total) }
}

#[cfg(test)]
mod tests {
    //! WARP: an upload buffer's bytes copied into its readback; a plain readback kept alone.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn target_budget() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 256,
            Height: 256,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let mut target: Option<ID3D12Resource> = None;
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_RENDER_TARGET, None, &mut target) }.unwrap();
        let target = target.unwrap();
        let one = unsafe { device.GetResourceAllocationInfo(0, &[desc]) }.SizeInBytes;
        let mut resources = [core::ptr::null_mut(); 5];
        resources[0] = target.as_raw();
        resources[1] = target.as_raw();
        let mut total = 0;
        assert!(target_bytes(&device, &resources, 3, 1 << 26, &mut total) && total == 2 * one, "two colour targets");
        let mut total = 0;
        assert!(!target_bytes(&device, &resources, 3, one + 1, &mut total) && total == one, "the second past the budget");
        let mut total = 0;
        assert!(!target_bytes(&device, &resources, 0x10, 1 << 26, &mut total), "a missing depth target");
    }

    #[test]
    fn readbacks_copy() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let source = crate::readback_buffers::buffer(&device, None, 256, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let mut mapped = core::ptr::null_mut();
        unsafe { source.Map(0, None, Some(&mut mapped)) }.unwrap();
        for i in 0..256 {
            unsafe { *mapped.cast::<u8>().add(i) = i as u8 };
        }
        unsafe { source.Unmap(0, None) };
        let (copied, ok) = readback(&device, &list, Some(&source), 64, true);
        let copied = copied.unwrap();
        assert!(ok);
        assert_eq!(unsafe { copied.GetDesc() }.Width, 64, "the source's shape at the copied width");
        assert!(cvr_gpu::command_resources::has(&list, source.as_raw()) && cvr_gpu::command_resources::has(&list, copied.as_raw()));
        let (plain, ok) = readback(&device, &list, None, 128, false);
        assert!(ok && unsafe { plain.unwrap().GetDesc() }.Width == 128);
        unsafe { list.Close() }.unwrap();
        unsafe { queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap())]) };
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe { queue.Signal(&fence, 1) }.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }
        let mut read = core::ptr::null_mut();
        unsafe { copied.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 64 }), Some(&mut read)) }.unwrap();
        let bytes = unsafe { core::slice::from_raw_parts(read as *const u8, 64) };
        assert!(bytes.iter().enumerate().all(|(i, &b)| b == i as u8));
        unsafe { copied.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    }
}
