//! The descriptor audit's source capture pieces (src/Render/StereoDescriptorAudit.cpp, CaptureSourceTablesAtDraw and
//! CaptureSourceSlotsAtDraw): a request's table bound through a shader-visible heap of its type, of the capture's
//! device, whose cookie is the range's heap (every bound heap of that type and flags asked for its cookie, as the .cpp
//! asked), at the device's stride for that type; a CBV's backing a buffer of the device whose range holds the
//! descriptor's view; a new owner's CPU heap of the table's slots, each slot's descriptor emitted at its place (the
//! port's own commands).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends its capture slots; the unit test below runs these on WARP.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::audit_values::cookie;
use crate::descriptor_store::Value;
use crate::instance_snapshot_arena::same_device;

/// SourceCaptureSlot (the .cpp's capture slot).
#[repr(C)]
pub struct CaptureSlot {
    pub value: Value,
    pub stamp_gpu: u64,
    pub stamp_heap: u64,
    pub stamp_value: Value,
    pub resource: *mut c_void,
    pub counter: *mut c_void,
    pub origin: *mut c_void,
    pub extra: *mut c_void,
    pub cookie: u64,
    pub used: bool,
    pub kind: u32,
}
const _: () = assert!(core::mem::size_of::<CaptureSlot>() == 384 && core::mem::offset_of!(CaptureSlot, resource) == 336);

fn borrowed<T: Interface>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live interface of that kind or null, borrowed for the call.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}

/// The table bound through one of `heaps` of `kind` with the range's heap `cookie`, at the device's `stride`.
pub fn bound_heap(device: &ID3D12Device, heaps: &[*mut c_void], kind: D3D12_DESCRIPTOR_HEAP_TYPE, heap_cookie: u64, stride: u32) -> bool {
    let mut bound = false;
    for &raw in heaps {
        let Some(heap) = borrowed::<ID3D12DescriptorHeap>(raw) else { continue };
        if !same_device(device, raw) {
            continue;
        }
        // SAFETY: a live heap.
        let desc = unsafe { heap.GetDesc() };
        if desc.Type == kind && desc.Flags == D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE && cookie(raw) == heap_cookie {
            bound = true;
        }
    }
    // SAFETY: a live device.
    bound && stride == unsafe { device.GetDescriptorHandleIncrementSize(kind) }
}
/// A CBV's backing: a buffer of `device` whose range holds the view at `address` of `bytes`.
pub fn backing(device: &ID3D12Device, resource: *mut c_void, address: u64, bytes: u64) -> bool {
    let Some(buffer) = borrowed::<ID3D12Resource>(resource) else { return false };
    if !same_device(device, resource) {
        return false;
    }
    // SAFETY: a live resource.
    let (desc, base) = unsafe { (buffer.GetDesc(), buffer.GetGPUVirtualAddress()) };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER && base != 0 && address >= base && address - base <= desc.Width && bytes <= desc.Width - (address - base)
}
/// A new owner's CPU heap of `slots`' descriptors (None when it cannot be created or addressed).
pub fn owner_heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, slots: &[CaptureSlot]) -> Option<ID3D12DescriptorHeap> {
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: slots.len() as u32, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: a live device; the description a local.
    let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.ok()?;
    // SAFETY: a live heap and device.
    let (start, stride) = unsafe { (heap.GetCPUDescriptorHandleForHeapStart().ptr, device.GetDescriptorHandleIncrementSize(kind) as usize) };
    if start == 0 || stride == 0 || slots.len().wrapping_sub(1) > (usize::MAX - start) / stride {
        return None;
    }
    for (n, slot) in slots.iter().enumerate() {
        let resource = borrowed::<ID3D12Resource>(slot.resource);
        let counter = borrowed::<ID3D12Resource>(slot.counter);
        // SAFETY: the slot's place inside the new heap of its type.
        unsafe { crate::audit_emit::emit(device, slot.kind, slot.used, &slot.value, resource.as_ref(), counter.as_ref(), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start + n * stride }) };
    }
    Some(heap)
}

/// The bound heap.
///
/// # Safety
/// A live device; `heaps` holds `count` heaps or nulls.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bound_heap(device: *mut c_void, heaps: *const *mut c_void, count: u32, kind: u32, heap_cookie: u64, stride: u32) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    let heaps = if heaps.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(heaps, count as usize) } };
    bound_heap(&device, heaps, D3D12_DESCRIPTOR_HEAP_TYPE(kind as i32), heap_cookie, stride)
}
/// The CBV's backing.
///
/// # Safety
/// A live device; `resource` live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_backing(device: *mut c_void, resource: *mut c_void, address: u64, bytes: u64) -> bool {
    borrowed::<ID3D12Device>(device).is_some_and(|device| backing(&device, resource, address, bytes))
}
/// The owner's heap into `out` (a reference).
///
/// # Safety
/// A live device; `slots` holds `count`; `out` a slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_owner_heap(device: *mut c_void, kind: u32, slots: *const CaptureSlot, count: usize, out: *mut *mut c_void) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    let slots = if slots.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(slots, count) } };
    match owner_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE(kind as i32), slots) {
        Some(heap) => {
            // SAFETY: the caller's slot.
            unsafe { *out = heap.into_raw() };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! WARP: the bound heap and its refusals, a buffer backing's range, an owner heap of null descriptors.
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
    fn heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, visible: bool) -> ID3D12DescriptorHeap {
        let flags = if visible { D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE } else { D3D12_DESCRIPTOR_HEAP_FLAG_NONE };
        unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: 4, Flags: flags, NodeMask: 0 }) }.unwrap()
    }
    fn slot(kind: u32) -> CaptureSlot {
        CaptureSlot {
            value: [0; 20],
            stamp_gpu: 0,
            stamp_heap: 0,
            stamp_value: [0; 20],
            resource: core::ptr::null_mut(),
            counter: core::ptr::null_mut(),
            origin: core::ptr::null_mut(),
            extra: core::ptr::null_mut(),
            cookie: 0,
            used: false,
            kind,
        }
    }

    #[test]
    fn source_capture_pieces() {
        let device = warp();
        let resources = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, true);
        let samplers = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, true);
        let hidden = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, false);
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        let resource_cookie = cookie(resources.as_raw());
        let bound = [samplers.as_raw(), resources.as_raw()];
        assert!(bound_heap(&device, &bound, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, resource_cookie, stride));
        assert!(!bound_heap(&device, &bound, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, resource_cookie, stride + 1), "another stride");
        assert!(!bound_heap(&device, &bound, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, resource_cookie + 1000, stride), "another heap");
        assert!(!bound_heap(&device, &[samplers.as_raw(), core::ptr::null_mut()], D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, resource_cookie, stride));
        let hidden_cookie = cookie(hidden.as_raw());
        assert!(!bound_heap(&device, &[hidden.as_raw()], D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, hidden_cookie, stride), "not shader-visible");

        let buffer = crate::readback_buffers::buffer(&device, None, 1024, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let base = unsafe { buffer.GetGPUVirtualAddress() };
        assert!(backing(&device, buffer.as_raw(), base + 256, 768));
        assert!(!backing(&device, buffer.as_raw(), base + 256, 769), "past the buffer");
        assert!(!backing(&device, buffer.as_raw(), base - 256, 256), "before the buffer");
        assert!(!backing(&device, core::ptr::null_mut(), base, 256));

        let slots = [slot(1), slot(2), slot(3)];
        let owner = owner_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, &slots).unwrap();
        let desc = unsafe { owner.GetDesc() };
        assert_eq!((desc.NumDescriptors, desc.Flags), (3, D3D12_DESCRIPTOR_HEAP_FLAG_NONE));
        let samplers_owner = owner_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, &[slot(4)]).unwrap();
        assert_eq!(unsafe { samplers_owner.GetDesc() }.Type, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
        assert!(owner_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, &[]).is_none(), "no slots");
    }
}
