//! The depth plane resolve for the depth layer (formerly src/Render/DepthResolve.cpp): the game's scene depth is
//! R32G8X24_TYPELESS (64 bits a texel: the float depth in plane 0, the stencil in plane 1), and the OpenXR depth
//! swapchain wants one typed 32-bit depth. A copy between the two hangs the device, so a full-screen triangle samples
//! plane 0 through an R32_FLOAT_X8X24_TYPELESS view (R24_UNORM_X8_TYPELESS for R24G8, R32_FLOAT for a 32-bit source)
//! and writes it to SV_Depth through a DSV of the output. The shaders stay HLSL (docs/rust-migration-plan.md), compiled
//! at first use as the C++ did. The C++ class also declared an R32_FLOAT colour variant that nothing called; it is not
//! carried over.
use core::ffi::c_void;
use std::sync::Mutex;

use cvr_host::log;
use windows::core::{w, Interface, PCSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const D3DCOMPILE_ENABLE_STRICTNESS: u32 = 1 << 11;

/// Full-screen triangle, no vertex buffer.
const VERTEX_SHADER: &str = r#"
struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut VSMain(uint vid : SV_VertexID) {
    static const float2 positions[3] = {
        float2(-1.0,  1.0),
        float2( 3.0,  1.0),
        float2(-1.0, -3.0)
    };
    static const float2 uvs[3] = {
        float2(0.0, 0.0),
        float2(2.0, 0.0),
        float2(0.0, 2.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.uv = uvs[vid];
    return o;
}
"#;

/// Plane 0 sampled (the view reads only the float depth) and written to SV_Depth; no colour attachment.
const PIXEL_SHADER: &str = r#"
Texture2D<float> g_srcDepth : register(t0);
SamplerState g_pointClamp   : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float PSMain(VSOut input) : SV_Depth {
    return g_srcDepth.SampleLevel(g_pointClamp, input.uv, 0.0);
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
    // SAFETY: the source's bytes and length, NUL-terminated entry and target, out-blobs.
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
            "DepthResolve: shader compile failed {}/{} hr=0x{:08X} {}\n",
            entry.to_string_lossy(),
            target.to_string_lossy(),
            e.code().0 as u32,
            blob_text(&errors)
        );
        return None;
    }
    code
}

/// The typed view of the source's depth plane.
fn source_view(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R32G8X24_TYPELESS => DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
        DXGI_FORMAT_R24G8_TYPELESS => DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
        // R32_TYPELESS, D32_FLOAT, R32_FLOAT: already 32 bits, sampled directly.
        _ => DXGI_FORMAT_R32_FLOAT,
    }
}

struct State {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    srv_heap: ID3D12DescriptorHeap,
    dsv_heap: ID3D12DescriptorHeap,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
}
// SAFETY: free-threaded D3D12 objects, used under the resolve's mutex.
unsafe impl Send for State {}

/// DepthResolve.
#[derive(Default)]
pub struct Resolve {
    state: Mutex<Option<State>>,
}

fn heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, flags: D3D12_DESCRIPTOR_HEAP_FLAGS) -> Option<ID3D12DescriptorHeap> {
    // SAFETY: the live device; the description outlives the call.
    unsafe {
        device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: kind,
                NumDescriptors: 1,
                Flags: flags,
                NodeMask: 0,
            })
            .ok()
    }
}

fn build(device: &ID3D12Device, format: DXGI_FORMAT, width: u32, height: u32) -> Option<State> {
    // SAFETY (whole body): the live device; descriptions on the stack outliving the calls that read them.
    unsafe {
        let Some(srv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE) else {
            log!("DepthResolve: failed to create SRV heap\n");
            return None;
        };
        let _ = srv_heap.SetName(w!("DepthResolve_SRV_heap"));
        let Some(dsv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_DSV, D3D12_DESCRIPTOR_HEAP_FLAG_NONE) else {
            log!("DepthResolve: failed to create DSV heap\n");
            return None;
        };
        let _ = dsv_heap.SetName(w!("DepthResolve_DSV_heap"));
        // The root signature: an SRV table at t0 and a static point sampler.
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            ..Default::default()
        };
        let parameter = D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            ..Default::default()
        };
        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: &parameter,
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let mut blob: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        if D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).is_err() {
            log!("DepthResolve: root sig serialize failed {}\n", blob_text(&errors));
            return None;
        }
        let blob = blob?;
        let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        let root: ID3D12RootSignature = match device.CreateRootSignature(0, bytes) {
            Ok(r) => r,
            Err(_) => {
                log!("DepthResolve: failed to create root sig\n");
                return None;
            }
        };
        let _ = root.SetName(w!("DepthResolve_RootSig"));
        // the shaders made from Rust (rust/shaders/passes/src/fullscreen.rs); the HLSL above with CVR_SHADER_HLSL
        let vs = crate::dxil::pick(crate::dxil::FULLSCREEN_UV_VS, || compile(VERTEX_SHADER, c"VSMain", c"vs_5_0"))?;
        let ps = crate::dxil::pick(crate::dxil::DEPTH_RESOLVE_PS, || compile(PIXEL_SHADER, c"PSMain", c"ps_5_0"))?;
        let mut pso = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(&root),
            VS: vs.bytecode(),
            PS: ps.bytecode(),
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            // Depth only.
            NumRenderTargets: 0,
            DSVFormat: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            SampleMask: u32::MAX,
            ..Default::default()
        };
        pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        // No colour writes; every depth written, whatever the destination held (the shader returns SV_Depth).
        pso.BlendState.RenderTarget[0].RenderTargetWriteMask = 0;
        pso.DepthStencilState.DepthEnable = true.into();
        pso.DepthStencilState.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ALL;
        pso.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
        pso.DepthStencilState.StencilEnable = false.into();
        let made: windows::core::Result<ID3D12PipelineState> = device.CreateGraphicsPipelineState(&pso);
        // The root signature reference the description borrowed is not the description's to release.
        let _ = core::mem::ManuallyDrop::into_inner(core::mem::take(&mut pso.pRootSignature)).map(core::mem::forget);
        let pso = match made {
            Ok(p) => p,
            Err(_) => {
                log!("DepthResolve: PSO create failed\n");
                return None;
            }
        };
        let _ = pso.SetName(w!("DepthResolve_PSO"));
        log!("DepthResolve: initialized {}x{} outputFmt={}\n", width, height, format.0);
        Some(State {
            device: device.clone(),
            root,
            pso,
            srv_heap,
            dsv_heap,
            format,
            width,
            height,
        })
    }
}

impl Resolve {
    /// EnsureInitialized: the resolve for `device`, the depth output's `format` and its size; multi-call safe.
    pub fn ensure(&self, device: &ID3D12Device, format: DXGI_FORMAT, width: u32, height: u32) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if width == 0 || height == 0 {
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

    /// RecordResolve: the depth plane of `source` (in PIXEL_SHADER_RESOURCE) drawn into the depth of `destination` (in
    /// DEPTH_WRITE).
    pub fn record(&self, list: &ID3D12GraphicsCommandList, source: &ID3D12Resource, destination: &ID3D12Resource) -> bool {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(st) = s.as_ref() else { return false };
        // SAFETY (whole body): the live device, heaps, resources and recording list.
        unsafe {
            let srv_cpu = st.srv_heap.GetCPUDescriptorHandleForHeapStart();
            let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: source_view(source.GetDesc().Format),
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: 1,
                        ..Default::default()
                    },
                },
            };
            st.device.CreateShaderResourceView(source, Some(&srv), srv_cpu);
            let dsv_cpu = st.dsv_heap.GetCPUDescriptorHandleForHeapStart();
            let dsv = D3D12_DEPTH_STENCIL_VIEW_DESC {
                Format: st.format,
                ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            st.device.CreateDepthStencilView(destination, Some(&dsv), dsv_cpu);
            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: st.width as f32,
                Height: st.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            let scissor = RECT {
                left: 0,
                top: 0,
                right: st.width as i32,
                bottom: st.height as i32,
            };
            list.RSSetViewports(&[viewport]);
            list.RSSetScissorRects(&[scissor]);
            list.OMSetRenderTargets(0, None, false, Some(&dsv_cpu));
            list.SetGraphicsRootSignature(&st.root);
            list.SetPipelineState(&st.pso);
            list.SetDescriptorHeaps(&[Some(st.srv_heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, st.srv_heap.GetGPUDescriptorHandleForHeapStart());
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

// ---- the C ABI (include/Render/DepthResolve.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_depth_resolve_new() -> *mut Resolve {
    Box::into_raw(Box::default())
}
/// # Safety
/// `resolve` came from cvr_gpu_depth_resolve_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_depth_resolve_free(resolve: *mut Resolve) {
    if !resolve.is_null() {
        // SAFETY: the caller's resolve, freed once.
        drop(unsafe { Box::from_raw(resolve) });
    }
}
/// # Safety
/// `resolve` is live; `device` an ID3D12Device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_depth_resolve_ensure(resolve: *const Resolve, device: *mut c_void, format: i32, width: u32, height: u32) -> bool {
    // SAFETY: the caller's resolve and device.
    unsafe {
        match (resolve.as_ref(), ID3D12Device::from_raw_borrowed(&device)) {
            (Some(r), Some(d)) => r.ensure(d, DXGI_FORMAT(format), width, height),
            _ => false,
        }
    }
}
/// # Safety
/// `resolve` is live; the others are a recording ID3D12GraphicsCommandList and two ID3D12Resource, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_depth_resolve_record(resolve: *const Resolve, list: *mut c_void, source: *mut c_void, destination: *mut c_void) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            resolve.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&source),
            ID3D12Resource::from_raw_borrowed(&destination),
        ) {
            (Some(r), Some(l), Some(s), Some(d)) => r.record(l, s, d),
            _ => false,
        }
    }
}
/// # Safety
/// `resolve` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_depth_resolve_shutdown(resolve: *const Resolve) {
    // SAFETY: the caller's resolve.
    if let Some(r) = unsafe { resolve.as_ref() } {
        r.shutdown();
    }
}
