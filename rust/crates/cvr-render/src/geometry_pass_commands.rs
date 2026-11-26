//! The stereo geometry pass's target views and recording state (src/Render/StereoGeometryPass.cpp): at Create, the
//! targets' two-slice views (one RTV heap of every colour target in order, a one-entry DSV heap for the depth target);
//! at Record, the pass's start (no predication, the targets bound as one contiguous range with the depth, no shading-rate
//! image); and before each draw -- of the pass or of a source -- its saved state (topology, stencil reference, blend
//! factor, viewport, scissor, the source's shading-rate image, the shading rate and, for the pass, its view mask).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the pass's admission, its binding bank and geometry, the
//! command-resource retention and the internal-commands scope; the unit test below records these on WARP, and
//! tools/single_pass_tests/geometry_pass_test runs the pass through the .cpp in its Rust variant.

use core::ffi::c_void;
use cvr_gpu::scene_state::Rate;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

/// A draw's recording state, lent by the .cpp.
#[repr(C)]
pub struct State {
    pub rate: Rate,
    pub image: *mut c_void,
    pub set_image: bool,
    pub set_mask: bool,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
    pub viewport: D3D12_VIEWPORT,
    pub scissor: RECT,
    pub mask: u32,
}
const _: () = assert!(core::mem::size_of::<Rate>() == 32 && core::mem::size_of::<State>() == 112);

fn borrowed<T: Interface>(raw: &*mut c_void) -> Option<&T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(raw) }
}
/// The heaps Create keeps: the colour targets' RTV heap and stride, the depth target's DSV heap.
pub struct Views {
    pub rtvs: Option<ID3D12DescriptorHeap>,
    pub stride: u32,
    pub dsv: Option<ID3D12DescriptorHeap>,
}
/// Create's views: each colour target's two-slice RTV in order, the depth target's two-slice DSV.
pub fn views(device: &ID3D12Device, colors: &[*mut c_void], formats: &[DXGI_FORMAT], depth: *mut c_void, depth_format: DXGI_FORMAT) -> Result<Views, i32> {
    let mut out = Views { rtvs: None, stride: 0, dsv: None };
    if !colors.is_empty() {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: colors.len() as u32, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
        // SAFETY: a live device; the description a local.
        let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|e| e.code().0)?;
        out.stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
        let mut handle = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        for (color, format) in colors.iter().zip(formats) {
            let view = D3D12_RENDER_TARGET_VIEW_DESC {
                Format: *format,
                ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 0, ArraySize: 2, PlaneSlice: 0 } },
            };
            // SAFETY: the caller's live target; the handle the new heap's.
            unsafe { device.CreateRenderTargetView(borrowed::<ID3D12Resource>(color), Some(&view), handle) };
            handle.ptr += out.stride as usize;
        }
        out.rtvs = Some(heap);
    }
    if !depth.is_null() {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV, NumDescriptors: 1, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
        // SAFETY: as above.
        let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|e| e.code().0)?;
        let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: depth_format,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_DSV { MipSlice: 0, FirstArraySlice: 0, ArraySize: 2 } },
        };
        unsafe { device.CreateDepthStencilView(borrowed::<ID3D12Resource>(&depth), Some(&view), heap.GetCPUDescriptorHandleForHeapStart()) };
        out.dsv = Some(heap);
    }
    Ok(out)
}
/// Record's start: no predication, the targets (one contiguous range) and depth bound, no shading-rate image.
pub fn begin(list: &ID3D12GraphicsCommandList, rates: &ID3D12GraphicsCommandList5, rtvs: Option<&ID3D12DescriptorHeap>, colors: u32, dsv: Option<&ID3D12DescriptorHeap>) {
    // SAFETY: a live list; the heaps the pass's own.
    unsafe {
        let rtv = rtvs.map(|h| h.GetCPUDescriptorHandleForHeapStart()).unwrap_or_default();
        let depth = dsv.map(|h| h.GetCPUDescriptorHandleForHeapStart()).unwrap_or_default();
        list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
        list.OMSetRenderTargets(colors, rtvs.map(|_| &rtv as *const _), true, dsv.map(|_| &depth as *const _));
        rates.RSSetShadingRateImage(None);
    }
}
/// A draw's saved state: topology, stencil reference, blend factor, viewport, scissor, (a source's) shading-rate image,
/// the shading rate and (the pass's) view mask.
pub fn state(list: &ID3D12GraphicsCommandList, rates: &ID3D12GraphicsCommandList5, views: Option<&ID3D12GraphicsCommandList1>, s: &State) {
    let combiners = [D3D12_SHADING_RATE_COMBINER(s.rate.combiners[0]), D3D12_SHADING_RATE_COMBINER(s.rate.combiners[1])];
    // SAFETY: a live list; the image the draw state's own (or null).
    unsafe {
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY(s.topology));
        list.OMSetStencilRef(s.stencil);
        list.OMSetBlendFactor(Some(&s.blend));
        list.RSSetViewports(&[s.viewport]);
        list.RSSetScissorRects(&[s.scissor]);
        if s.set_image {
            rates.RSSetShadingRateImage(borrowed::<ID3D12Resource>(&s.image));
        }
        rates.RSSetShadingRate(D3D12_SHADING_RATE(s.rate.value), Some(combiners.as_ptr()));
        if let (true, Some(views)) = (s.set_mask, views) {
            views.SetViewInstanceMask(s.mask);
        }
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Create's views: the RTV heap (its creation's reference) and stride, the DSV heap, into the outputs.
///
/// # Safety
/// A live device; `colors` and `formats` hold `count` entries; `depth` null or live; the outputs pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpa_views(device: *mut c_void, colors: *const *mut c_void, formats: *const i32, count: u32, depth: *mut c_void, depth_format: i32, rtvs: *mut *mut c_void, stride: *mut u32, dsv: *mut *mut c_void) -> i32 {
    let Some(device) = borrowed::<ID3D12Device>(&device) else { return 0x8007_0057u32 as i32 };
    let (colors, formats) = if count == 0 {
        (&[][..], Vec::new())
    } else {
        // SAFETY: as documented.
        unsafe { (core::slice::from_raw_parts(colors, count as usize), core::slice::from_raw_parts(formats, count as usize).iter().map(|&f| DXGI_FORMAT(f)).collect()) }
    };
    match views(device, colors, &formats, depth, DXGI_FORMAT(depth_format)) {
        Ok(v) => {
            // SAFETY: as documented; each heap handed over with its reference.
            unsafe {
                *stride = v.stride;
                *rtvs = v.rtvs.map_or(core::ptr::null_mut(), |h| h.into_raw());
                *dsv = v.dsv.map_or(core::ptr::null_mut(), |h| h.into_raw());
            }
            0
        }
        Err(hr) => hr,
    }
}
/// Record's start.
///
/// # Safety
/// A live list and its ID3D12GraphicsCommandList5; the heaps null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpa_begin(list: *mut c_void, rates: *mut c_void, rtvs: *mut c_void, colors: u32, dsv: *mut c_void) {
    if let (Some(l), Some(r)) = (borrowed::<ID3D12GraphicsCommandList>(&list), borrowed::<ID3D12GraphicsCommandList5>(&rates)) {
        begin(l, r, borrowed(&rtvs), colors, borrowed(&dsv))
    }
}
/// A draw's saved state.
///
/// # Safety
/// A live list and its ID3D12GraphicsCommandList5; `views` null or live; `s` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpa_state(list: *mut c_void, rates: *mut c_void, views: *mut c_void, s: *const State) {
    if let (Some(l), Some(r)) = (borrowed::<ID3D12GraphicsCommandList>(&list), borrowed::<ID3D12GraphicsCommandList5>(&rates)) {
        // SAFETY: as documented.
        state(l, r, borrowed(&views), unsafe { &*s })
    }
}

#[cfg(test)]
mod tests {
    //! On WARP: two colour targets and a depth target's views; the start and a draw's state recorded and closed.
    use super::*;
    use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn texture(device: &ID3D12Device, format: DXGI_FORMAT, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> ID3D12Resource {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 8,
            Height: 8,
            DepthOrArraySize: 2,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: flags,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let mut out: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.unwrap();
        out.unwrap()
    }
    #[test]
    fn views_and_state() {
        let device = warp();
        let rt = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        let (a, b) = (texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, rt, D3D12_RESOURCE_STATE_RENDER_TARGET), texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, rt, D3D12_RESOURCE_STATE_RENDER_TARGET));
        let depth = texture(&device, DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_STATE_DEPTH_WRITE);
        let v = views(&device, &[a.as_raw(), b.as_raw()], &[DXGI_FORMAT_R8G8B8A8_UNORM; 2], depth.as_raw(), DXGI_FORMAT_D32_FLOAT).unwrap();
        let rtvs = v.rtvs.as_ref().unwrap();
        assert_eq!((unsafe { rtvs.GetDesc() }.NumDescriptors, v.stride != 0, v.dsv.is_some()), (2, true, true));
        assert!(views(&device, &[], &[], core::ptr::null_mut(), DXGI_FORMAT_D32_FLOAT).is_ok_and(|v| v.rtvs.is_none() && v.dsv.is_none()), "no targets: no heaps");
        // The recording needs shading-rate images (VRS tier 2); WARP may offer less: then only the views are checked
        // here (tools/single_pass_tests/geometry_pass_test records the pass on its adapter).
        let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS6::default();
        let queried = unsafe { device.CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6, &mut options as *mut _ as *mut c_void, core::mem::size_of_val(&options) as u32) };
        if queried.is_err() || options.VariableShadingRateTier.0 < D3D12_VARIABLE_SHADING_RATE_TIER_2.0 {
            return;
        }
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let (rates, instancing): (ID3D12GraphicsCommandList5, ID3D12GraphicsCommandList1) = (list.cast().unwrap(), list.cast().unwrap());
        begin(&list, &rates, Some(rtvs), 2, v.dsv.as_ref());
        // The views draw: both eyes' slices of the first target cleared through the two-slice view.
        unsafe { list.ClearRenderTargetView(rtvs.GetCPUDescriptorHandleForHeapStart(), &[0.5; 4], None) };
        let s = State {
            rate: Rate::default(),
            image: core::ptr::null_mut(),
            set_image: false,
            set_mask: true,
            topology: D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0,
            stencil: 3,
            blend: [1.0; 4],
            viewport: D3D12_VIEWPORT { Width: 8.0, Height: 8.0, MaxDepth: 1.0, ..Default::default() },
            scissor: RECT { right: 8, bottom: 8, ..Default::default() },
            mask: 3,
        };
        state(&list, &rates, Some(&instancing), &s);
        state(&list, &rates, None, &State { set_image: true, set_mask: false, ..s });
        unsafe { list.Close() }.unwrap();
    }
}
