//! The native IA snapshot's object checks (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::SameDevice`, `Unprotected` and
//! `UnprotectedHeap`): a list and a device child on the same single-node device, and a resource or heap without a
//! protected session (an object without the session interface counts as unprotected; otherwise the session query's
//! result and presence decide, the resource's query and session results reported).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the three checks forward to `cvr_iao_*`; `tools/single_pass_tests/
//! native_ia_snapshot_test` runs its cases against them in its Rust variant, and the unit test below checks WARP objects.

use core::ffi::c_void;

use windows::core::{IUnknown, Interface};
use windows::Win32::Foundation::E_NOINTERFACE;
use windows::Win32::Graphics::Direct3D12::*;

use crate::structured_snapshot::no_protected_session;

/// SameDevice.
pub fn same_device(list: &ID3D12GraphicsCommandList, child: &ID3D12DeviceChild) -> bool {
    let mut a: Option<ID3D12Device> = None;
    let mut b: Option<ID3D12Device> = None;
    // SAFETY: live objects.
    let queried = unsafe { list.GetDevice(&mut a).is_ok() && child.GetDevice(&mut b).is_ok() };
    queried
        && a.is_some()
        && a.as_ref().map(|d| d.as_raw()) == b.as_ref().map(|d| d.as_raw())
        // SAFETY: a live device.
        && unsafe { a.as_ref().unwrap().GetNodeCount() } == 1
}
/// Unprotected: the resource without a protected session, with the query's and the session's results.
pub fn unprotected(resource: &ID3D12Resource) -> (bool, u64, u64) {
    let extended = resource.cast::<ID3D12Resource1>();
    match extended {
        Err(e) if e.code() == E_NOINTERFACE => (true, e.code().0 as u32 as u64, 0),
        Err(e) => (false, e.code().0 as u32 as u64, 0),
        Ok(extended) => {
            let mut session: *mut c_void = core::ptr::null_mut();
            // SAFETY: a live resource; an out-interface.
            let hr = unsafe { (extended.vtable().GetProtectedResourceSession)(extended.as_raw(), &IUnknown::IID, &mut session) };
            let present = !session.is_null();
            if present {
                // SAFETY: the reference the query added.
                drop(unsafe { IUnknown::from_raw(session) });
            }
            (no_protected_session(hr.0, present), hr.0 as u32 as u64, present as u64)
        }
    }
}
/// UnprotectedHeap.
pub fn unprotected_heap(heap: &ID3D12Heap) -> bool {
    match heap.cast::<ID3D12Heap1>() {
        Err(e) => e.code() == E_NOINTERFACE,
        Ok(extended) => {
            let mut session: *mut c_void = core::ptr::null_mut();
            // SAFETY: a live heap; an out-interface.
            let hr = unsafe { (extended.vtable().GetProtectedResourceSession)(extended.as_raw(), &IUnknown::IID, &mut session) };
            let present = !session.is_null();
            if present {
                // SAFETY: the reference the query added.
                drop(unsafe { IUnknown::from_raw(session) });
            }
            no_protected_session(hr.0, present)
        }
    }
}

// ---- the C ABI

/// # Safety
/// `list` and `child` are live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iao_same_device(list: *mut c_void, child: *mut c_void) -> bool {
    match (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe { ID3D12DeviceChild::from_raw_borrowed(&child) }) {
        (Some(list), Some(child)) => same_device(list, child),
        _ => false,
    }
}
/// # Safety
/// `resource` is live; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iao_unprotected(resource: *mut c_void, result: *mut u64, present: *mut u64) -> bool {
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    let (ok, r, p) = unprotected(resource);
    unsafe {
        *result = r;
        *present = p;
    }
    ok
}
/// # Safety
/// `heap` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iao_unprotected_heap(heap: *mut c_void) -> bool {
    unsafe { ID3D12Heap::from_raw_borrowed(&heap) }.is_some_and(unprotected_heap)
}

#[cfg(test)]
mod tests {
    //! A WARP list, buffer and heap: the same device, no protected sessions.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn warp_objects() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let heap_desc = D3D12_HEAP_DESC { SizeInBytes: 65536, Properties: D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() }, Alignment: 0, Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS };
        let mut heap: Option<ID3D12Heap> = None;
        unsafe { device.CreateHeap(&heap_desc, &mut heap) }.unwrap();
        let heap = heap.unwrap();
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 256,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut buffer: Option<ID3D12Resource> = None;
        unsafe { device.CreatePlacedResource(&heap, 0, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut buffer) }.unwrap();
        let buffer = buffer.unwrap();
        assert!(same_device(&list, &buffer.cast().unwrap()));
        let (ok, result, present) = unprotected(&buffer);
        assert!(ok && present == 0, "{result:x}");
        assert!(unprotected_heap(&heap));
        assert!(unsafe { cvr_iao_unprotected_heap(heap.as_raw()) });
        assert!(!unsafe { cvr_iao_same_device(core::ptr::null_mut(), buffer.as_raw()) });
    }
}
