//! The stereo GPU probe's scene pieces (src/Render/StereoGpuProbe.cpp): Initialize's descriptor heaps (21 RTVs with the
//! scene, else 15; 7 or 5 DSVs) and their strides, and its query heaps placed in the readback after the bytes so far
//! (8-aligned: the occlusion pair, the 1024 timestamps); the fine-rate scope (the list's shading rate from its scene
//! state, exported per eye with a change count; with the override and the eye selected, 1x1 with passthrough
//! combiners or the source's VRS image; restored at the scope's end); and ImportScene's direct-resolve views (the
//! native colour targets' 2D RTVs unless depth alone, the depth target's D32_S8X24 2D DSV).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the probe's state and the scope's owner; the unit test below runs
//! these on WARP, and tools/single_pass_tests/scene_probe_test runs every probe mode (the fine-rate and direct-resolve
//! ones among them) through the .cpp in its Rust variant.

use core::ffi::c_void;
use cvr_gpu::scene_state::{cvr_scene_snapshot, Bindings, Rate};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT_S8X24_UINT;

const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

fn borrowed<T: Interface>(raw: &*mut c_void) -> Option<&T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(raw) }
}

/// Initialize's descriptor heaps and their strides.
pub fn heaps(device: &ID3D12Device, scene: bool) -> Result<(ID3D12DescriptorHeap, ID3D12DescriptorHeap, u32, u32), i32> {
    let make = |kind, count| {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: count, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
        // SAFETY: a live device; the description a local.
        unsafe { device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) }.map_err(|e| e.code().0)
    };
    let rtv = make(D3D12_DESCRIPTOR_HEAP_TYPE_RTV, if scene { 21 } else { 15 })?;
    let dsv = make(D3D12_DESCRIPTOR_HEAP_TYPE_DSV, if scene { 7 } else { 5 })?;
    // SAFETY: a live device.
    let strides = unsafe { (device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV), device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV)) };
    Ok((rtv, dsv, strides.0, strides.1))
}
/// A query heap of `count` queries resolved after `bytes` (8-aligned): the heap and its offset; `bytes` its end.
pub fn queries(device: &ID3D12Device, kind: D3D12_QUERY_HEAP_TYPE, count: u32, bytes: &mut u64) -> Result<(ID3D12QueryHeap, u64), i32> {
    let desc = D3D12_QUERY_HEAP_DESC { Type: kind, Count: count, NodeMask: 0 };
    let mut heap: Option<ID3D12QueryHeap> = None;
    // SAFETY: a live device; the description a local.
    unsafe { device.CreateQueryHeap(&desc, &mut heap) }.map_err(|e| e.code().0)?;
    let offset = (*bytes + 7) & !7u64;
    *bytes = offset + u64::from(count) * 8;
    Ok((heap.ok_or(E_INVALIDARG)?, offset))
}

/// The fine-rate scope's exports.
#[repr(C)]
pub struct FineExports {
    pub vrs: *mut [[u32; 4]; 2],
    pub changes: *mut [u32; 2],
    pub images: *mut [u64; 2],
}
/// The scope's state: the list (its reference held while the rate is to be restored), the rate saved.
#[repr(C)]
pub struct FineRate {
    pub list: *mut c_void,
    pub saved: Rate,
    pub image_changed: bool,
}
const _: () = assert!(core::mem::size_of::<FineRate>() == 48 && core::mem::size_of::<FineExports>() == 24);

/// The fine-rate scope's start: 0, or the scene rejection (12 no scene state or rate, 13 no source image).
///
/// # Safety
/// The exports are live and writable.
pub unsafe fn fine_rate(list: &ID3D12GraphicsCommandList, scene: bool, eye: usize, seen: &mut bool, rate_override: u32, source_image: *mut c_void, e: &FineExports, out: &mut FineRate) -> u32 {
    if !scene {
        return 0;
    }
    let mut bindings = Bindings::default();
    // SAFETY: a live list; the output a local.
    if !unsafe { cvr_scene_snapshot(list.as_raw() as usize, &mut bindings) } {
        return 12;
    }
    out.saved = bindings.rate;
    let saved = out.saved;
    let values = [u32::from(saved.known), saved.value as u32, saved.combiners[0] as u32, saved.combiners[1] as u32];
    // SAFETY: as documented.
    let (vrs, changes, images) = unsafe { (&mut (*e.vrs)[eye], &mut (*e.changes)[eye], &mut (*e.images)[eye]) };
    if *seen && (*vrs != values || *images != saved.image as u64) {
        *changes += 1;
    }
    *seen = true;
    *vrs = values;
    *images = saved.image as u64;
    if rate_override == 0 || (rate_override == 2 && eye == 0) {
        return 0;
    }
    if !saved.known {
        return 12;
    }
    let rates: ID3D12GraphicsCommandList5 = match list.cast() {
        Ok(rates) => rates,
        Err(error) if error.code().0 == E_NOINTERFACE => return 0,
        Err(_) => return 12,
    };
    let fine = [D3D12_SHADING_RATE_COMBINER_PASSTHROUGH; 2];
    let reject = if rate_override == 1 {
        // SAFETY: a live list in recording.
        unsafe { rates.RSSetShadingRate(D3D12_SHADING_RATE_1X1, Some(fine.as_ptr())) };
        0
    } else if let Some(image) = borrowed::<ID3D12Resource>(&source_image) {
        unsafe { rates.RSSetShadingRateImage(image) };
        out.image_changed = true;
        0
    } else {
        13
    };
    out.list = rates.into_raw();
    reject
}
/// The fine-rate scope's end: the saved rate (and image) restored, the list's reference released.
///
/// # Safety
/// `state` from fine_rate (or zeroed).
pub unsafe fn fine_rate_end(state: &mut FineRate) {
    if state.list.is_null() {
        return;
    }
    // SAFETY: the reference fine_rate kept.
    let rates = unsafe { ID3D12GraphicsCommandList5::from_raw(core::mem::replace(&mut state.list, core::ptr::null_mut())) };
    let saved = state.saved;
    let combiners = [D3D12_SHADING_RATE_COMBINER(saved.combiners[0]), D3D12_SHADING_RATE_COMBINER(saved.combiners[1])];
    // SAFETY: a live list in recording; the saved image the engine's (or null).
    unsafe {
        if state.image_changed {
            let image = saved.image as *mut c_void;
            rates.RSSetShadingRateImage(borrowed::<ID3D12Resource>(&image));
        }
        rates.RSSetShadingRate(D3D12_SHADING_RATE(saved.value), Some(combiners.as_ptr()));
    }
}
/// ImportScene's direct-resolve views on the list's device (false when its device is unavailable).
pub fn direct_views(list: &ID3D12GraphicsCommandList, targets: &[*mut c_void; 4], rtvs: &[D3D12_CPU_DESCRIPTOR_HANDLE; 3], dsv: D3D12_CPU_DESCRIPTOR_HANDLE, depth_only: bool) -> bool {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list; the output a local.
    if unsafe { list.GetDevice(&mut device) }.is_err() {
        return false;
    }
    let Some(device) = device else { return false };
    // SAFETY: the native targets live (the list keeps them); the handles the probe's own heap.
    unsafe {
        if !depth_only {
            for (target, handle) in targets[..3].iter().zip(rtvs) {
                let Some(resource) = borrowed::<ID3D12Resource>(target) else { continue };
                let view = D3D12_RENDER_TARGET_VIEW_DESC { Format: resource.GetDesc().Format, ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D, ..Default::default() };
                device.CreateRenderTargetView(resource, Some(&view), *handle);
            }
        }
        let view = D3D12_DEPTH_STENCIL_VIEW_DESC { Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT, ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D, ..Default::default() };
        device.CreateDepthStencilView(borrowed::<ID3D12Resource>(&targets[3]), Some(&view), dsv);
    }
    true
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Initialize's heaps (each handed over with its reference) and strides.
///
/// # Safety
/// A live device; the outputs pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_heaps(device: *mut c_void, scene: bool, rtv: *mut *mut c_void, dsv: *mut *mut c_void, rtv_stride: *mut u32, dsv_stride: *mut u32) -> i32 {
    let Some(device) = borrowed::<ID3D12Device>(&device) else { return E_INVALIDARG };
    match heaps(device, scene) {
        Ok((r, d, rs, ds)) => {
            // SAFETY: as documented.
            unsafe {
                (*rtv, *dsv, *rtv_stride, *dsv_stride) = (r.into_raw(), d.into_raw(), rs, ds);
            }
            0
        }
        Err(hr) => hr,
    }
}
/// A query heap (handed over with its reference) after `bytes`.
///
/// # Safety
/// A live device; the outputs pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_queries(device: *mut c_void, kind: i32, count: u32, bytes: *mut u64, offset: *mut u64, heap: *mut *mut c_void) -> i32 {
    let Some(device) = borrowed::<ID3D12Device>(&device) else { return E_INVALIDARG };
    // SAFETY: as documented.
    match queries(device, D3D12_QUERY_HEAP_TYPE(kind), count, unsafe { &mut *bytes }) {
        Ok((h, at)) => {
            unsafe { (*offset, *heap) = (at, h.into_raw()) };
            0
        }
        Err(hr) => hr,
    }
}
/// The fine-rate scope's start (0 or the scene rejection).
///
/// # Safety
/// A live list; `seen` and `out` writable; `source_image` null or live; the exports live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_fine_rate(list: *mut c_void, scene: bool, eye: u32, seen: *mut bool, rate_override: u32, source_image: *mut c_void, exports: *const FineExports, out: *mut FineRate) -> u32 {
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(&list) else { return 12 };
    // SAFETY: as documented.
    unsafe { fine_rate(list, scene, eye as usize & 1, &mut *seen, rate_override, source_image, &*exports, &mut *out) }
}
/// The fine-rate scope's end.
///
/// # Safety
/// `state` from cvr_sgpr_fine_rate (or zeroed).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_fine_rate_end(state: *mut FineRate) {
    // SAFETY: as documented.
    unsafe { fine_rate_end(&mut *state) }
}
/// ImportScene's direct-resolve views.
///
/// # Safety
/// A live list; the targets (four) live or null; `rtvs` three handles.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_direct_views(list: *mut c_void, targets: *const [*mut c_void; 4], rtvs: *const [D3D12_CPU_DESCRIPTOR_HANDLE; 3], dsv: usize, depth_only: bool) -> bool {
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(&list) else { return false };
    // SAFETY: as documented.
    direct_views(list, unsafe { &*targets }, unsafe { &*rtvs }, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: dsv }, depth_only)
}

#[cfg(test)]
mod tests {
    //! On WARP: the heaps and strides, the query heaps' placement, the fine-rate scope's exports on a list without a
    //! scene state (rejected) and outside the scene (nothing).
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
    #[test]
    fn scene_pieces() {
        let device = warp();
        let (rtv, dsv, rs, ds) = heaps(&device, true).unwrap();
        assert_eq!((unsafe { rtv.GetDesc() }.NumDescriptors, unsafe { dsv.GetDesc() }.NumDescriptors, rs != 0, ds != 0), (21, 7, true, true));
        let (rtv, dsv, _, _) = heaps(&device, false).unwrap();
        assert_eq!((unsafe { rtv.GetDesc() }.NumDescriptors, unsafe { dsv.GetDesc() }.NumDescriptors), (15, 5));
        let mut bytes = 1001u64;
        let (_, occlusion) = queries(&device, D3D12_QUERY_HEAP_TYPE_OCCLUSION, 2, &mut bytes).unwrap();
        assert_eq!((occlusion, bytes), (1008, 1024), "8-aligned after the bytes so far");
        let (_, stamps) = queries(&device, D3D12_QUERY_HEAP_TYPE_TIMESTAMP, 1024, &mut bytes).unwrap();
        assert_eq!((stamps, bytes), (1024, 1024 + 8192));
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let (mut vrs, mut changes, mut images) = ([[7u32; 4]; 2], [0u32; 2], [0u64; 2]);
        let e = FineExports { vrs: &mut vrs, changes: &mut changes, images: &mut images };
        let mut state = FineRate { list: core::ptr::null_mut(), saved: Rate::default(), image_changed: false };
        let mut seen = false;
        assert_eq!(unsafe { fine_rate(&list, false, 0, &mut seen, 1, core::ptr::null_mut(), &e, &mut state) }, 0, "outside the scene: nothing");
        assert_eq!((seen, vrs[0]), (false, [7; 4]), "nothing exported outside the scene");
        // A list the scene state never saw has no snapshot: the scope rejects it (12), restoring nothing.
        assert_eq!(unsafe { fine_rate(&list, true, 0, &mut seen, 1, core::ptr::null_mut(), &e, &mut state) }, 12);
        assert!(state.list.is_null());
        unsafe { fine_rate_end(&mut state) };
        unsafe { list.Close() }.unwrap();
    }
}
