//! The sRGB decode into the eye swapchain (formerly src/Render/SrgbToLinearPass.cpp): a swapchain that does not
//! declare the eye bytes as sRGB gets them drawn through this pass instead of copied -- a full-screen triangle that
//! loads each source texel and decodes it (by hand for an 8-bit UNORM source, by the view for an _SRGB one), so the
//! target view may encode again. The shaders stay HLSL (docs/rust-migration-plan.md: own shaders are an explicit
//! exception until their own stage), compiled at first use as the C++ did.
use core::ffi::c_void;
use std::sync::Mutex;

use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Three submit command lists, two eyes each.
const DESCRIPTORS: u32 = 6;
const D3DCOMPILE_ENABLE_STRICTNESS: u32 = 1 << 11;

const VERTEX_SHADER: &str = r#"
struct VSOut {
    float4 position : SV_Position;
};

VSOut VSMain(uint vertexId : SV_VertexID) {
    static const float2 positions[3] = {
        float2(-1.0,  1.0),
        float2( 3.0,  1.0),
        float2(-1.0, -3.0)
    };
    VSOut output;
    output.position = float4(positions[vertexId], 0.0, 1.0);
    return output;
}
"#;

const PIXEL_SHADER: &str = r#"
cbuffer ConversionParams : register(b0) {
    uint manuallyDecodeSrgb;
};
Texture2D<float4> sourceTexture : register(t0);

float3 SrgbToLinear(float3 value) {
    const float3 low = value / 12.92;
    const float3 high = pow((value + 0.055) / 1.055, 2.4);
    return lerp(low, high, step(0.04045, value));
}

float4 PSMain(float4 position : SV_Position) : SV_Target {
    const float4 encoded = sourceTexture.Load(int3(position.xy, 0));
    const float3 linearColor = manuallyDecodeSrgb != 0
        ? SrgbToLinear(encoded.rgb)
        : encoded.rgb;
    return float4(linearColor, encoded.a);
}
"#;

fn blob_text(b: &Option<ID3DBlob>) -> String {
    match b {
        // SAFETY: the blob's own buffer.
        Some(b) => unsafe { String::from_utf8_lossy(core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() },
        None => String::new(),
    }
}

fn compile(source: &str, entry: &core::ffi::CStr, target: &core::ffi::CStr) -> Option<ID3DBlob> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source's bytes and lengths, NUL-terminated entry and target, out-blobs.
    let hr = unsafe {
        D3DCompile(
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(target.as_ptr() as *const u8),
            D3DCOMPILE_ENABLE_STRICTNESS,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = hr {
        log!(
            "SrgbToLinearPass: shader compile failed {}/{} hr=0x{:08X} {}\n",
            entry.to_string_lossy(),
            target.to_string_lossy(),
            e.code().0 as u32,
            blob_text(&errors)
        );
        return None;
    }
    code
}

/// The source's view and whether the shader decodes it by hand.
fn source_view(format: DXGI_FORMAT) -> Option<(DXGI_FORMAT, bool)> {
    match format {
        DXGI_FORMAT_R8G8B8A8_TYPELESS | DXGI_FORMAT_R8G8B8A8_UNORM => Some((DXGI_FORMAT_R8G8B8A8_UNORM, true)),
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => Some((DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, false)),
        DXGI_FORMAT_B8G8R8A8_TYPELESS | DXGI_FORMAT_B8G8R8A8_UNORM => Some((DXGI_FORMAT_B8G8R8A8_UNORM, true)),
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => Some((DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, false)),
        _ => None,
    }
}

struct State {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    srv_heap: ID3D12DescriptorHeap,
    rtv_heap: ID3D12DescriptorHeap,
    format: DXGI_FORMAT,
    srv_size: u32,
    rtv_size: u32,
    width: u32,
    height: u32,
}
// SAFETY: free-threaded D3D12 objects, used under the pass's mutex.
unsafe impl Send for State {}

/// SrgbToLinearPass.
#[derive(Default)]
pub struct Pass {
    state: Mutex<Option<State>>,
}

fn build(device: &ID3D12Device, format: DXGI_FORMAT, width: u32, height: u32) -> Option<State> {
    // SAFETY (whole body): the live device; descriptions on the stack outliving the calls that read them.
    unsafe {
        let srv_heap: ID3D12DescriptorHeap = match device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: DESCRIPTORS,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        }) {
            Ok(h) => h,
            Err(_) => {
                log!("SrgbToLinearPass: failed to create SRV heap\n");
                return None;
            }
        };
        let srv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        let rtv_heap: ID3D12DescriptorHeap = match device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: DESCRIPTORS,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        }) {
            Ok(h) => h,
            Err(_) => {
                log!("SrgbToLinearPass: failed to create RTV heap\n");
                return None;
            }
        };
        let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let range = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, BaseShaderRegister: 0, ..Default::default() };
        let parameters = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 1 } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: &range } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];
        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 2,
            pParameters: parameters.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: core::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let mut blob: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        if D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).is_err() {
            log!("SrgbToLinearPass: root signature serialization failed {}\n", blob_text(&errors));
            return None;
        }
        let blob = blob?;
        let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        let root: ID3D12RootSignature = match device.CreateRootSignature(0, bytes) {
            Ok(r) => r,
            Err(_) => {
                log!("SrgbToLinearPass: failed to create root signature\n");
                return None;
            }
        };
        // the shaders made from Rust (rust/shaders/passes/src/fullscreen.rs); the HLSL above with CVR_SHADER_HLSL
        let vs = crate::dxil::pick(crate::dxil::SRGB_VS, || compile(VERTEX_SHADER, c"VSMain", c"vs_5_0"))?;
        let ps = crate::dxil::pick(crate::dxil::SRGB_PS, || compile(PIXEL_SHADER, c"PSMain", c"ps_5_0"))?;
        let mut pso = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(&root),
            VS: vs.bytecode(),
            PS: ps.bytecode(),
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            SampleMask: u32::MAX,
            ..Default::default()
        };
        pso.RTVFormats[0] = format;
        pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let made: windows::core::Result<ID3D12PipelineState> = device.CreateGraphicsPipelineState(&pso);
        // The root signature reference the description borrowed is not the description's to release.
        let _ = core::mem::ManuallyDrop::into_inner(core::mem::take(&mut pso.pRootSignature)).map(core::mem::forget);
        let pso = match made {
            Ok(p) => p,
            Err(_) => {
                log!("SrgbToLinearPass: failed to create pipeline state for format {}\n", format.0);
                return None;
            }
        };
        log!("SrgbToLinearPass: initialized {}x{} outputFmt={}\n", width, height, format.0);
        Some(State { device: device.clone(), root, pso, srv_heap, rtv_heap, format, srv_size, rtv_size, width, height })
    }
}

impl Pass {
    /// EnsureInitialized: the pass for `device`, the swapchain's typed `format` and its size; multi-call safe.
    pub fn ensure(&self, device: &ID3D12Device, format: DXGI_FORMAT, width: u32, height: u32) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if format == DXGI_FORMAT_UNKNOWN || width == 0 || height == 0 {
            return false;
        }
        if let Some(st) = s.as_ref() {
            if st.device.as_raw() == device.as_raw() && st.format == format && st.width == width && st.height == height {
                return true;
            }
        }
        *s = None;
        *s = build(device, format, width, height);
        s.is_some()
    }

    /// Record: `source` (in PIXEL_SHADER_RESOURCE) drawn into `destination` (in RENDER_TARGET) through descriptor slot
    /// `index` (< 6: one per submit list and eye, so a list in flight never has its descriptors rewritten).
    pub fn record(&self, list: &ID3D12GraphicsCommandList, source: &ID3D12Resource, destination: &ID3D12Resource, index: u32) -> bool {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(st) = s.as_ref() else { return false };
        if index >= DESCRIPTORS {
            return false;
        }
        // SAFETY (whole body): the live device, heaps, resources and recording list.
        unsafe {
            let source_format = source.GetDesc().Format;
            let Some((view_format, decode)) = source_view(source_format) else {
                log!("SrgbToLinearPass: unsupported source format {}\n", source_format.0);
                return false;
            };
            let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: view_format,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
            };
            let mut srv_cpu = st.srv_heap.GetCPUDescriptorHandleForHeapStart();
            srv_cpu.ptr += (index * st.srv_size) as usize;
            st.device.CreateShaderResourceView(source, Some(&srv), srv_cpu);
            let rtv = D3D12_RENDER_TARGET_VIEW_DESC { Format: st.format, ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D, ..Default::default() };
            let mut rtv_cpu = st.rtv_heap.GetCPUDescriptorHandleForHeapStart();
            rtv_cpu.ptr += (index * st.rtv_size) as usize;
            st.device.CreateRenderTargetView(destination, Some(&rtv), rtv_cpu);
            let viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: st.width as f32, Height: st.height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
            let scissor = RECT { left: 0, top: 0, right: st.width as i32, bottom: st.height as i32 };
            list.RSSetViewports(&[viewport]);
            list.RSSetScissorRects(&[scissor]);
            list.OMSetRenderTargets(1, Some(&rtv_cpu), false, None);
            list.SetGraphicsRootSignature(&st.root);
            list.SetPipelineState(&st.pso);
            list.SetDescriptorHeaps(&[Some(st.srv_heap.clone())]);
            let mut srv_gpu = st.srv_heap.GetGPUDescriptorHandleForHeapStart();
            srv_gpu.ptr += (index * st.srv_size) as u64;
            list.SetGraphicsRoot32BitConstant(0, decode as u32, 0);
            list.SetGraphicsRootDescriptorTable(1, srv_gpu);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.DrawInstanced(3, 1, 0, 0);
        }
        true
    }

    /// Shutdown: every object released.
    pub fn shutdown(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

// ---- the C ABI (include/Render/SrgbToLinearPass.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_srgb_pass_new() -> *mut Pass {
    Box::into_raw(Box::default())
}
/// # Safety
/// `pass` came from cvr_gpu_srgb_pass_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_srgb_pass_free(pass: *mut Pass) {
    if !pass.is_null() {
        // SAFETY: the caller's pass, freed once.
        drop(unsafe { Box::from_raw(pass) });
    }
}
/// # Safety
/// `pass` is live; `device` an ID3D12Device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_srgb_pass_ensure(pass: *const Pass, device: *mut c_void, format: i32, width: u32, height: u32) -> bool {
    // SAFETY: the caller's pass and device.
    unsafe {
        match (pass.as_ref(), ID3D12Device::from_raw_borrowed(&device)) {
            (Some(p), Some(d)) => p.ensure(d, DXGI_FORMAT(format), width, height),
            _ => false,
        }
    }
}
/// # Safety
/// `pass` is live; the others are a recording ID3D12GraphicsCommandList and two ID3D12Resource, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_srgb_pass_record(pass: *const Pass, list: *mut c_void, source: *mut c_void, destination: *mut c_void, index: u32) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            pass.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&source),
            ID3D12Resource::from_raw_borrowed(&destination),
        ) {
            (Some(p), Some(l), Some(s), Some(d)) => p.record(l, s, d, index),
            _ => false,
        }
    }
}
/// # Safety
/// `pass` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_srgb_pass_shutdown(pass: *const Pass) {
    // SAFETY: the caller's pass.
    if let Some(p) = unsafe { pass.as_ref() } {
        p.shutdown();
    }
}
