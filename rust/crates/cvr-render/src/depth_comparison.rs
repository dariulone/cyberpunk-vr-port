//! The stereo depth comparison (src/Render/StereoDepthComparison.cpp): the diagnostic comparison of the original
//! pipelines' depth against the paired pass's, on private targets and a private queue.
//!
//! - The target: a single-layer, single-mip, single-sample 2D depth-stencil texture up to 4096 x 4096 in
//!   D32_FLOAT_S8X24_UINT (or its typeless form).
//! - The private targets: each eye's reference depth and the paired pass's two-slice array, cleared to the pass's clear
//!   depth, with their three DSVs.
//! - The readback planes: per plane (depth, stencil), eye and mode (reference, paired) the copy footprint, 512-aligned
//!   one after the other; the rows the target's height, the row bytes whole pixels (depth 4 or 8 bytes, stencil 1 or 8,
//!   an 8-byte stencil at component 4), at most 512 MiB.
//! - The recording: no shading rate image and 1x1 rates; the three targets cleared; each eye's reference draws (the
//!   inputs whose view mask holds the eye) with their original pipeline, bank, topology, stencil, blend factor,
//!   viewport, scissor and geometry; then (after the paired pass, the C++ half's) the targets to COPY_SOURCE and every
//!   plane copied.
//! - The read: per eye the written depths (not the clear value), the differing depths, the depths not finite on either
//!   side, the largest finite depth error, the differing stencils.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these run through `cvr_sdc_*`; the bank, the geometry and the paired pass stay
//! the C++ half's (callbacks). The unit tests below run the targets, planes, recording, copies and read on WARP, and
//! tools/single_pass_tests/geometry_pass_test runs the whole comparison in its Rust variant
//! (stereo_depth_comparison_rust.cpp).

use core::ffi::c_void;
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, RECT, S_OK};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::early_depth_compare::{Comparison, Plane};

pub const MAX_READBACK_BYTES: u64 = 512 * 1024 * 1024;

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
fn unowned(resource: *mut c_void) -> core::mem::ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
    unsafe { core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) }
}
/// The target's shape.
pub fn target_shape(t: &D3D12_RESOURCE_DESC) -> bool {
    t.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && t.Width != 0
        && t.Height != 0
        && t.Width <= 4096
        && t.Height <= 4096
        && t.DepthOrArraySize == 1
        && t.MipLevels == 1
        && t.SampleDesc.Count == 1
        && t.SampleDesc.Quality == 0
        && (t.Flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL).0 != 0
        && (t.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT || t.Format == DXGI_FORMAT_R32G8X24_TYPELESS)
}
/// The private targets and their DSVs (the objects with their creation's reference).
#[repr(C)]
pub struct Targets {
    pub depth: [*mut c_void; 3],
    pub dsvs: *mut c_void,
}
/// The three depth targets (each eye's, the paired array) and their DSV heap.
pub fn create_targets(device: &ID3D12Device, target: &D3D12_RESOURCE_DESC, clear: f32, out: &mut Targets) -> HRESULT {
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let value = D3D12_CLEAR_VALUE {
        Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: clear, Stencil: 0 } },
    };
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV, NumDescriptors: 3, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
    // SAFETY: a live device; the descriptions are locals.
    let dsvs: ID3D12DescriptorHeap = match unsafe { device.CreateDescriptorHeap(&desc) } {
        Ok(heap) => heap,
        Err(error) => return error.code(),
    };
    // SAFETY: as above.
    let (stride, start) = unsafe { (device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) as usize, dsvs.GetCPUDescriptorHandleForHeapStart()) };
    out.dsvs = dsvs.into_raw();
    for i in 0..3 {
        let mut desc = *target;
        desc.Alignment = 0;
        desc.DepthOrArraySize = if i == 2 { 2 } else { 1 };
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: as above.
        if let Err(error) = unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_DEPTH_WRITE, Some(&value), &mut resource) } {
            return error.code();
        }
        let Some(resource) = resource else { return E_FAIL };
        let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            ViewDimension: if i == 2 { D3D12_DSV_DIMENSION_TEXTURE2DARRAY } else { D3D12_DSV_DIMENSION_TEXTURE2D },
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: if i == 2 {
                D3D12_DEPTH_STENCIL_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_DSV { MipSlice: 0, FirstArraySlice: 0, ArraySize: 2 } }
            } else {
                D3D12_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 } }
            },
        };
        // SAFETY: as above; the handle inside the heap.
        unsafe { device.CreateDepthStencilView(&resource, Some(&view), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + i * stride }) };
        out.depth[i] = resource.into_raw();
    }
    S_OK
}
fn desc_of(resource: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    // SAFETY: a live resource.
    borrowed::<ID3D12Resource>(resource).map(|r| unsafe { r.GetDesc() })
}
/// The readback planes and their bytes (E_INVALIDARG a footprint off the target, E_NOTIMPL a stride the read refuses,
/// E_OUTOFMEMORY beyond 512 MiB).
pub fn plan(device: &ID3D12Device, depth: &[*mut c_void; 3], width: u32, height: u32, planes: &mut [Plane; 8]) -> Result<u64, HRESULT> {
    let mut bytes = 0u64;
    for plane in 0..2u32 {
        for eye in 0..2u32 {
            for mode in 0..2u32 {
                let copy = &mut planes[(plane * 4 + eye * 2 + mode) as usize];
                let desc = desc_of(depth[if mode != 0 { 2 } else { eye as usize }]).ok_or(E_INVALIDARG)?;
                let sub = if mode != 0 { eye + plane * 2 } else { plane };
                let (mut rows, mut row_bytes, mut total) = (0u32, 0u64, 0u64);
                // SAFETY: a live device; the outputs are locals.
                unsafe { device.GetCopyableFootprints(&desc, sub, 1, bytes, Some(&mut copy.layout), Some(&mut rows), Some(&mut row_bytes), Some(&mut total)) };
                if total == u64::MAX || width == 0 || row_bytes % u64::from(width) != 0 || rows != height {
                    return Err(E_INVALIDARG);
                }
                copy.stride = (row_bytes / u64::from(width)) as u32;
                if (plane == 0 && copy.stride != 4 && copy.stride != 8) || (plane == 1 && copy.stride != 1 && copy.stride != 8) {
                    return Err(E_NOTIMPL);
                }
                copy.component = if plane != 0 && copy.stride == 8 { 4 } else { 0 };
                bytes = (copy.layout.Offset + u64::from(copy.layout.Footprint.RowPitch) * u64::from(rows) + 511) & !511;
                if bytes > MAX_READBACK_BYTES {
                    return Err(E_OUTOFMEMORY);
                }
            }
        }
    }
    Ok(bytes)
}
/// The readback buffer (READBACK, COPY_DEST).
pub fn create_readback(device: &ID3D12Device, bytes: u64, out: &mut *mut c_void) -> HRESULT {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions are locals.
    if let Err(error) = unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut resource) } {
        return error.code();
    }
    match resource {
        Some(resource) => {
            *out = resource.into_raw();
            S_OK
        }
        None => E_FAIL,
    }
}
/// One eye's state of an input's reference draw.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EyeDraw {
    pub pipeline: *mut c_void,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
    pub viewport: D3D12_VIEWPORT,
    pub scissor: RECT,
}
/// An input: its view mask and both eyes' draws.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawInput {
    pub view_mask: u32,
    pub _pad: u32,
    pub eyes: [EyeDraw; 2],
}
/// The C++ objects: an input's reference bank for an eye, its geometry.
#[repr(C)]
pub struct Host {
    pub user: *mut c_void,
    pub bind_eye: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32, u32) -> bool,
    pub geometry: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32) -> bool,
}
const _: () = assert!(
    core::mem::size_of::<Targets>() == 32
        && core::mem::size_of::<EyeDraw>() == 72
        && core::mem::offset_of!(EyeDraw, viewport) == 32
        && core::mem::size_of::<DrawInput>() == 152
        && core::mem::size_of::<Host>() == 24
);
/// The clears and each eye's reference draws (false: a bank or geometry refused).
///
/// # Safety
/// The list is recording; `start` the DSV heap's start with its stride; the inputs' pipelines are live; the callbacks
/// are the C++ half's.
pub unsafe fn record(list: &ID3D12GraphicsCommandList, rates: Option<&ID3D12GraphicsCommandList5>, start: usize, stride: usize, clear: f32, inputs: &[DrawInput], host: &Host) -> bool {
    // SAFETY: as documented.
    unsafe {
        if let Some(rates) = rates {
            rates.RSSetShadingRateImage(None::<&ID3D12Resource>);
            rates.RSSetShadingRate(D3D12_SHADING_RATE_1X1, None);
        }
        for i in 0..3 {
            list.ClearDepthStencilView(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start + i * stride }, D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL, clear, 0, None);
        }
        for eye in 0..2u32 {
            let view = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start + eye as usize * stride };
            list.OMSetRenderTargets(0, None, false, Some(&view));
            for (i, input) in inputs.iter().enumerate() {
                if input.view_mask & (1 << eye) == 0 {
                    continue;
                }
                let draw = &input.eyes[eye as usize];
                list.SetPipelineState(borrowed::<ID3D12PipelineState>(draw.pipeline).as_ref());
                if !(host.bind_eye)(host.user, list.as_raw(), eye, i as u32) {
                    return false;
                }
                list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY(draw.topology));
                list.OMSetStencilRef(draw.stencil);
                list.OMSetBlendFactor(Some(&draw.blend));
                list.RSSetViewports(&[draw.viewport]);
                list.RSSetScissorRects(&[draw.scissor]);
                if !(host.geometry)(host.user, list.as_raw(), i as u32) {
                    return false;
                }
            }
        }
    }
    true
}
/// The targets to COPY_SOURCE and every plane into the readback.
///
/// # Safety
/// The list is recording; the targets in DEPTH_WRITE, the readback in COPY_DEST.
pub unsafe fn copies(list: &ID3D12GraphicsCommandList, depth: &[*mut c_void; 3], readback: *mut c_void, planes: &[Plane; 8]) {
    // SAFETY: as documented.
    unsafe {
        for &target in depth {
            let barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: unowned(target),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        StateAfter: D3D12_RESOURCE_STATE_COPY_SOURCE,
                    }),
                },
            };
            list.ResourceBarrier(&[barrier]);
        }
        for plane in 0..2u32 {
            for eye in 0..2u32 {
                for mode in 0..2u32 {
                    let from = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(depth[if mode != 0 { 2 } else { eye as usize }]),
                        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: if mode != 0 { eye + plane * 2 } else { plane } },
                    };
                    let to = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(readback),
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: planes[(plane * 4 + eye * 2 + mode) as usize].layout },
                    };
                    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                }
            }
        }
    }
}
/// The row of a plane (the last pixel's bytes inside the image).
fn row(image: &[u8], plane: &Plane, y: u32, width: u32, bytes: u64) -> Option<usize> {
    let start = plane.layout.Offset.checked_add(u64::from(y).checked_mul(u64::from(plane.layout.Footprint.RowPitch))?)?.checked_add(u64::from(plane.component))?;
    let span = if width == 0 { 0 } else { u64::from(width - 1).checked_mul(u64::from(plane.stride))?.checked_add(bytes)? };
    (start.checked_add(span)? <= image.len() as u64).then_some(start as usize)
}
/// The read's comparison (false when a plane leaves the readback).
pub fn compare(image: &[u8], planes: &[Plane; 8], clear: f32, result: &mut Comparison) -> bool {
    let clear = clear.to_bits();
    let (width, height) = (result.width, result.height);
    for eye in 0..2usize {
        for plane in 0..2usize {
            let (a, b) = (&planes[plane * 4 + eye * 2], &planes[plane * 4 + eye * 2 + 1]);
            let bytes = if plane != 0 { 1u64 } else { 4 };
            for y in 0..height {
                let (Some(left), Some(right)) = (row(image, a, y, width, bytes), row(image, b, y, width, bytes)) else { return false };
                for x in 0..width as usize {
                    let (l, r) = (left + x * a.stride as usize, right + x * b.stride as usize);
                    if plane != 0 {
                        result.stencil_different[eye] += u64::from(image[l] != image[r]);
                        continue;
                    }
                    let av = u32::from_le_bytes(image[l..l + 4].try_into().unwrap());
                    let bv = u32::from_le_bytes(image[r..r + 4].try_into().unwrap());
                    result.written[eye] += u64::from(av != clear);
                    result.depth_different[eye] += u64::from(av != bv);
                    let (af, bf) = (f32::from_bits(av), f32::from_bits(bv));
                    if !af.is_finite() || !bf.is_finite() {
                        result.invalid_depth[eye] += 1;
                        continue;
                    }
                    result.max_depth_error[eye] = result.max_depth_error[eye].max((af - bf).abs());
                }
            }
        }
    }
    true
}

// ---- the C ABI ------------------------------------------------------------------------------------------------------

/// # Safety
/// `desc` is a resource description.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_target_shape(desc: *const D3D12_RESOURCE_DESC) -> bool {
    // SAFETY: as documented.
    target_shape(unsafe { &*desc })
}
/// # Safety
/// The device is live; `out` the targets (each created object handed over with its reference).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_targets(device: *mut c_void, target: *const D3D12_RESOURCE_DESC, clear: f32, out: *mut Targets) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    // SAFETY: as documented.
    unsafe { create_targets(&device, &*target, clear, &mut *out) }
}
/// The planes and the readback (`stage` 7 the planes, 8 the readback where it failed).
///
/// # Safety
/// As cvr_sdc_targets; `planes` eight planes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_planes(
    device: *mut c_void,
    depth: *const [*mut c_void; 3],
    width: u32,
    height: u32,
    planes: *mut [Plane; 8],
    bytes: *mut u64,
    readback: *mut *mut c_void,
    stage: *mut u32,
) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    // SAFETY: as documented.
    unsafe {
        *stage = 7;
        let planned = match plan(&device, &*depth, width, height, &mut *planes) {
            Ok(planned) => planned,
            Err(error) => return error,
        };
        *bytes = planned;
        *stage = 8;
        create_readback(&device, planned, &mut *readback)
    }
}
/// The clears and each eye's reference draws.
///
/// # Safety
/// As record; the inputs hold their count.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_record(
    list: *mut c_void,
    rates: *mut c_void,
    start: usize,
    stride: usize,
    clear: f32,
    inputs: *const DrawInput,
    count: usize,
    host: *const Host,
) -> bool {
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    let rates = borrowed::<ID3D12GraphicsCommandList5>(rates);
    // SAFETY: as documented.
    unsafe {
        let inputs = if inputs.is_null() || count == 0 { &[][..] } else { core::slice::from_raw_parts(inputs, count) };
        record(&list, rates.as_ref(), start, stride, clear, inputs, &*host)
    }
}
/// # Safety
/// As copies.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_copies(list: *mut c_void, depth: *const [*mut c_void; 3], readback: *mut c_void, planes: *const [Plane; 8]) {
    if let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) {
        // SAFETY: as documented.
        unsafe { copies(&list, &*depth, readback, &*planes) };
    }
}
/// Read once the private queue's fence completed: the readback mapped and compared into `result`.
///
/// # Safety
/// The readback is live and holds `bytes`; `result` holds the width and height.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_read(readback: *mut c_void, bytes: u64, planes: *const [Plane; 8], clear: f32, result: *mut Comparison) -> HRESULT {
    let Some(resource) = borrowed::<ID3D12Resource>(readback) else { return E_INVALIDARG };
    let range = D3D12_RANGE { Begin: 0, End: bytes as usize };
    let mut mapped = core::ptr::null_mut();
    // SAFETY: a live readback.
    if let Err(error) = unsafe { resource.Map(0, Some(&range), Some(&mut mapped)) } {
        return error.code();
    }
    if mapped.is_null() {
        // SAFETY: as above.
        unsafe { resource.Unmap(0, None) };
        return E_FAIL;
    }
    // SAFETY: the mapped readback's bytes; the caller's planes and result.
    let compared = unsafe { compare(core::slice::from_raw_parts(mapped as *const u8, bytes as usize), &*planes, clear, &mut *result) };
    // SAFETY: as above.
    unsafe { resource.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    if !compared {
        return E_FAIL;
    }
    // SAFETY: as documented.
    unsafe { (*result).complete = 1 };
    S_OK
}

#[cfg(test)]
mod tests {
    //! The targets, planes, recording, copies and read on WARP: cleared targets agree; one eye's reference depth and
    //! stencil cleared apart differ in every pixel; the shapes the comparison refuses.
    use super::*;

    fn warp() -> ID3D12Device {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn target(width: u64, height: u32) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
            ..Default::default()
        }
    }
    /// A depth-only pipeline drawing a full-target triangle at depth 0.25, and its empty root signature.
    fn pipeline(device: &ID3D12Device) -> (ID3D12RootSignature, ID3D12PipelineState) {
        use windows::core::PCSTR;
        use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
        use windows::Win32::Graphics::Direct3D::ID3DBlob;
        let source = "float4 main(uint id : SV_VertexID) : SV_Position { float2 p = float2((id << 1) & 2, id & 2); return float4(p * float2(2, -2) + float2(-1, 1), 0.25, 1); }";
        let mut code: Option<ID3DBlob> = None;
        unsafe {
            D3DCompile(source.as_ptr().cast(), source.len(), PCSTR::null(), None, None, PCSTR(b"main ".as_ptr()), PCSTR(b"vs_5_0 ".as_ptr()), 0, 0, &mut code, None)
        }
        .unwrap();
        let code = code.unwrap();
        let mut blob: Option<ID3DBlob> = None;
        unsafe { D3D12SerializeRootSignature(&D3D12_ROOT_SIGNATURE_DESC::default(), D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None) }.unwrap();
        let blob = blob.unwrap();
        let root: ID3D12RootSignature =
            unsafe { device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())) }.unwrap();
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::ManuallyDrop::new(Some(root.clone())),
            VS: D3D12_SHADER_BYTECODE { pShaderBytecode: unsafe { code.GetBufferPointer() }, BytecodeLength: unsafe { code.GetBufferSize() } },
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC { FillMode: D3D12_FILL_MODE_SOLID, CullMode: D3D12_CULL_MODE_NONE, DepthClipEnable: true.into(), ..Default::default() },
            DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                ..Default::default()
            },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            DSVFormat: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let state: ID3D12PipelineState = unsafe { device.CreateGraphicsPipelineState(&desc) }.unwrap();
        unsafe { core::mem::ManuallyDrop::drop(&mut desc.pRootSignature) };
        (root, state)
    }
    unsafe extern "C-unwind" fn bind_root(user: *mut c_void, list: *mut c_void, _: u32, _: u32) -> bool {
        let list = borrowed::<ID3D12GraphicsCommandList>(list).unwrap();
        unsafe { list.SetGraphicsRootSignature(&*(user as *const ID3D12RootSignature)) };
        true
    }
    unsafe extern "C-unwind" fn triangle(_: *mut c_void, list: *mut c_void, _: u32) -> bool {
        let list = borrowed::<ID3D12GraphicsCommandList>(list).unwrap();
        unsafe { list.DrawInstanced(3, 1, 0, 0) };
        true
    }
    unsafe extern "C-unwind" fn refuse(_: *mut c_void, _: *mut c_void, _: u32, _: u32) -> bool {
        false
    }
    #[test]
    fn shapes() {
        assert!(target_shape(&target(8, 4)));
        assert!(target_shape(&D3D12_RESOURCE_DESC { Format: DXGI_FORMAT_R32G8X24_TYPELESS, ..target(8, 4) }));
        assert!(!target_shape(&target(8192, 4)), "wider than 4096");
        assert!(!target_shape(&D3D12_RESOURCE_DESC { Format: DXGI_FORMAT_D32_FLOAT, ..target(8, 4) }), "no stencil");
        assert!(!target_shape(&D3D12_RESOURCE_DESC { Flags: D3D12_RESOURCE_FLAG_NONE, ..target(8, 4) }), "not a depth-stencil");
        assert!(!target_shape(&D3D12_RESOURCE_DESC { MipLevels: 2, ..target(8, 4) }), "mips");
    }
    #[test]
    fn comparison_on_warp() {
        let device = warp();
        let (width, height, clear) = (8u32, 4u32, 0.0f32);
        let mut t = Targets { depth: [core::ptr::null_mut(); 3], dsvs: core::ptr::null_mut() };
        assert_eq!(create_targets(&device, &target(u64::from(width), height), clear, &mut t), S_OK);
        let depth: Vec<ID3D12Resource> = t.depth.iter().map(|&d| unsafe { ID3D12Resource::from_raw(d) }).collect();
        let dsvs = unsafe { ID3D12DescriptorHeap::from_raw(t.dsvs) };
        let mut planes = [Plane::default(); 8];
        let bytes = plan(&device, &t.depth, width, height, &mut planes).unwrap();
        for (i, p) in planes.iter().enumerate() {
            assert_eq!(p.layout.Offset % 512, 0);
            assert_eq!(p.component, if i >= 4 && p.stride == 8 { 4 } else { 0 });
            if i > 0 {
                assert!(p.layout.Offset > planes[i - 1].layout.Offset, "each plane after the last");
            }
        }
        assert!(bytes >= planes[7].layout.Offset + 3 * u64::from(planes[7].layout.Footprint.RowPitch) + 8);
        assert_eq!(plan(&device, &t.depth, width, height * 2, &mut [Plane::default(); 8]), Err(E_INVALIDARG), "rows off the target");
        assert_eq!(plan(&device, &t.depth, width * 2, height, &mut [Plane::default(); 8]), Err(E_NOTIMPL), "a stride the read refuses");
        let mut readback = core::ptr::null_mut();
        assert_eq!(create_readback(&device, bytes, &mut readback), S_OK);
        let readback = unsafe { ID3D12Resource::from_raw(readback) };
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let start = unsafe { dsvs.GetCPUDescriptorHandleForHeapStart() }.ptr;
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) } as usize;
        let (root, state) = pipeline(&device);
        let host = Host { user: (&root as *const ID3D12RootSignature).cast_mut().cast(), bind_eye: bind_root, geometry: triangle };
        // The left eye's reference draws the triangle at 0.25; the right eye's has nothing to draw (the view mask).
        let viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        let scissor = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
        let eye = EyeDraw { pipeline: state.as_raw(), topology: 4, stencil: 0, blend: [1.0; 4], viewport, scissor };
        let input = DrawInput { view_mask: 1, _pad: 0, eyes: [eye; 2] };
        assert!(unsafe { record(&list, None, start, stride, clear, &[input], &host) });
        unsafe { copies(&list, &t.depth, readback.as_raw(), &planes) };
        unsafe { list.Close() }.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe {
            queue.ExecuteCommandLists(&[Some(list.cast().unwrap())]);
            queue.Signal(&fence, 1).unwrap();
        }
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }
        let mut result = Comparison { width, height, draws: 1, ..Comparison::default() };
        assert_eq!(unsafe { cvr_sdc_read(readback.as_raw(), bytes, &planes, clear, &mut result) }, S_OK);
        let pixels = u64::from(width * height);
        assert_eq!(result.complete, 1);
        assert_eq!((result.written, result.depth_different, result.stencil_different), ([pixels, 0], [pixels, 0], [0, 0]), "the left eye's reference drawn");
        assert_eq!((result.invalid_depth, result.max_depth_error), ([0, 0], [0.25, 0.0]));
        // A bank refused: the recording stops.
        unsafe { allocator.Reset() }.unwrap();
        unsafe { list.Reset(&allocator, None) }.unwrap();
        let refusing = Host { bind_eye: refuse, ..host };
        assert!(!unsafe { record(&list, None, start, stride, clear, &[input], &refusing) });
        unsafe { list.Close() }.unwrap();
        drop(depth);
    }
}
