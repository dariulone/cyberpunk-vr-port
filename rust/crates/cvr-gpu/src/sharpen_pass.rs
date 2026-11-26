//! CAS sharpening (formerly src/Render/SharpenPass.cpp): the colour copied into a private scratch (which also takes a
//! source denied to shaders), filtered through AMD's contrast-adaptive sharpening onto the destination -- in place for
//! an exclusively owned capture, restoring COPY_SOURCE -- with the emphasis and the amount as root constants and an
//! sRGB target re-encoded by hand. Everything the draw uses is kept alive until the GPU has run it
//! (command_resources). The shaders stay HLSL, compiled at first use as the C++ did.
use core::ffi::c_void;
use std::sync::Mutex;

use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const D3DCOMPILE_ENABLE_STRICTNESS: u32 = 1 << 11;

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

const PIXEL_SHADER: &str = r#"
cbuffer FilterCB : register(b0) {
    float4 filterParam; // x=Emphasis, y=4*amount, z=typed sRGB RTV
}
Texture2D<float4> g_source : register(t0);
SamplerState g_sampler : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float3 tap(float2 uv, float2 off) {
    return g_source.SampleLevel(g_sampler, uv + off, 0.0).rgb;
}

float4 PSMain(VSOut input) : SV_Target {
    float w, h;
    g_source.GetDimensions(w, h);
    float2 inv = float2(1.0 / w, 1.0 / h);
    float2 uv = input.uv;

    float3 a = tap(uv, float2(-inv.x, -inv.y));
    float3 b = tap(uv, float2(    0.0, -inv.y));
    float3 c = tap(uv, float2( inv.x, -inv.y));
    float3 d = tap(uv, float2(-inv.x,    0.0));
    float3 e = tap(uv, float2(    0.0,    0.0));
    float3 f = tap(uv, float2( inv.x,    0.0));
    float3 g = tap(uv, float2(-inv.x,  inv.y));
    float3 hh= tap(uv, float2(    0.0,  inv.y));
    float3 i = tap(uv, float2( inv.x,  inv.y));

    float3 mn = min(min(min(d, e), min(f, b)), hh);
    mn += min(mn, min(min(a, c), min(g, i)));
    float3 mx = max(max(max(d, e), max(f, b)), hh);
    mx += max(mx, max(max(a, c), max(g, i)));

    float3 rcpMx = rcp(max(mx, 1e-5));
    float3 amp = saturate(min(mn, 2.0 - mx) * rcpMx);
    amp = sqrt(amp);

    float peak = -1.0 / (8.0 - 3.0 * saturate(filterParam.x));
    float3 wt = amp * peak;
    float3 rcpW = rcp(1.0 + 4.0 * wt);
    float3 sharp = saturate((b * wt + d * wt + f * wt + hh * wt + e) * rcpW);

    float3 outc = saturate(lerp(e, sharp, filterParam.y));
    if (filterParam.z > 0.5) {
        outc = float3(outc.r <= 0.04045 ? outc.r / 12.92 : pow((outc.r + 0.055) / 1.055, 2.4),
                      outc.g <= 0.04045 ? outc.g / 12.92 : pow((outc.g + 0.055) / 1.055, 2.4),
                      outc.b <= 0.04045 ? outc.b / 12.92 : pow((outc.b + 0.055) / 1.055, 2.4));
    }
    return float4(outc, 1.0);
}
"#;

/// The 8-bit (or 10-bit) colour view a texture is filtered through.
fn color_view_format(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_R8G8B8A8_TYPELESS | DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_TYPELESS | DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_R10G10B10A2_TYPELESS | DXGI_FORMAT_R10G10B10A2_UNORM => DXGI_FORMAT_R10G10B10A2_UNORM,
        _ => DXGI_FORMAT_UNKNOWN,
    }
}
fn is_srgb(f: DXGI_FORMAT) -> bool {
    f == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB || f == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
}
fn compatible(d: &D3D12_RESOURCE_DESC, format: DXGI_FORMAT, width: u32, height: u32) -> bool {
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && color_view_format(d.Format) == format
        && d.Width == width as u64
        && d.Height == height
        && d.DepthOrArraySize == 1
        && d.MipLevels == 1
        && d.SampleDesc.Count == 1
}

fn blob_text(b: &Option<ID3DBlob>) -> String {
    match b {
        // SAFETY: the blob's own buffer.
        Some(b) => unsafe { String::from_utf8_lossy(core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() },
        None => String::new(),
    }
}
fn compile(source: &str, entry: &core::ffi::CStr, target: &core::ffi::CStr) -> Option<ID3DBlob> {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
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
            "SharpenPass: shader compile failed {}/{} hr=0x{:08X} {}\n",
            entry.to_string_lossy(),
            target.to_string_lossy(),
            e.code().0 as u32,
            blob_text(&errors)
        );
        return None;
    }
    code
}

/// A transition of every subresource, borrowing the resource (the barrier holds no reference of its own).
fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

struct State {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    srv_heap: ID3D12DescriptorHeap,
    rtv_heap: ID3D12DescriptorHeap,
    scratch: ID3D12Resource,
    color_format: DXGI_FORMAT,
    width: u32,
    height: u32,
}
// SAFETY: free-threaded D3D12 objects, used under the pass's mutex.
unsafe impl Send for State {}

/// SharpenPass.
#[derive(Default)]
pub struct Pass {
    state: Mutex<Option<State>>,
}

fn build(device: &ID3D12Device, color_format: DXGI_FORMAT, scratch_format: DXGI_FORMAT, width: u32, height: u32) -> Option<State> {
    // SAFETY (whole body): the live device; descriptions on the stack outliving the calls that read them.
    unsafe {
        let Ok(srv_heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 1,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        }) else {
            log!("SharpenPass: failed to create SRV heap\n");
            return None;
        };
        let _ = srv_heap.SetName(windows::core::w!("SharpenPass_SRV_heap"));
        let Ok(rtv_heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        }) else {
            log!("SharpenPass: failed to create RTV heap\n");
            return None;
        };
        let _ = rtv_heap.SetName(windows::core::w!("SharpenPass_RTV_heap"));
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: scratch_format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let mut scratch: Option<ID3D12Resource> = None;
        if device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut scratch)
            .is_err()
        {
            log!("SharpenPass: failed to create SRV scratch\n");
            return None;
        }
        let scratch = scratch?;
        let _ = scratch.SetName(windows::core::w!("CAS_Sharpening_SRV_scratch"));
        let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: scratch_format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        device.CreateShaderResourceView(&scratch, Some(&srv), srv_heap.GetCPUDescriptorHandleForHeapStart());
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            ..Default::default()
        };
        let parameters = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            ShaderRegister: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            MaxLOD: D3D12_FLOAT32_MAX,
            ..Default::default()
        };
        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 2,
            pParameters: parameters.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let (mut blob, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        if D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).is_err() {
            log!("SharpenPass: root sig serialize failed {}\n", blob_text(&errors));
            return None;
        }
        let blob = blob?;
        let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(0, bytes) else {
            log!("SharpenPass: failed to create root sig\n");
            return None;
        };
        let _ = root.SetName(windows::core::w!("SharpenPass_RootSig"));
        // the shaders made from Rust (rust/shaders/passes/src/fullscreen.rs); the HLSL above with CVR_SHADER_HLSL
        let vs = crate::dxil::pick(crate::dxil::FULLSCREEN_UV_VS, || compile(VERTEX_SHADER, c"VSMain", c"vs_5_0"))?;
        let ps = crate::dxil::pick(crate::dxil::SHARPEN_PS, || compile(PIXEL_SHADER, c"PSMain", c"ps_5_0"))?;
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
        pso.RTVFormats[0] = color_format;
        pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let made: windows::core::Result<ID3D12PipelineState> = device.CreateGraphicsPipelineState(&pso);
        let Ok(pso) = made else {
            log!("SharpenPass: PSO create failed\n");
            return None;
        };
        let _ = pso.SetName(windows::core::w!("SharpenPass_PSO"));
        log!("SharpenPass: initialized {}x{} colorFmt={}\n", width, height, color_format.0);
        Some(State {
            device: device.clone(),
            root,
            pso,
            srv_heap,
            rtv_heap,
            scratch,
            color_format,
            width,
            height,
        })
    }
}

impl Pass {
    /// EnsureInitialized: the pass for `device`, the output colour format (an sRGB one kept, the others taken as their
    /// colour view) and the size; multi-call safe.
    pub fn ensure(&self, device: &ID3D12Device, color_format: DXGI_FORMAT, width: u32, height: u32) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let scratch_format = color_view_format(color_format);
        let color_format = if is_srgb(color_format) { color_format } else { scratch_format };
        if width == 0 || height == 0 || color_format == DXGI_FORMAT_UNKNOWN {
            return false;
        }
        if let Some(st) = s.as_ref() {
            if st.device.as_raw() == device.as_raw() && st.color_format == color_format && st.width == width && st.height == height {
                return true;
            }
        }
        *s = None;
        *s = build(device, color_format, scratch_format, width, height);
        s.is_some()
    }

    /// RecordSharpen: the source (in COPY_SOURCE) sharpened onto the destination (in RENDER_TARGET, or the source itself,
    /// returned to COPY_SOURCE); false records no commands.
    pub fn record(&self, list: &ID3D12GraphicsCommandList, src: &ID3D12Resource, dst: &ID3D12Resource, sharpness: f32, amount: f32) -> bool {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(st) = s.as_ref() else { return false };
        // SAFETY (whole body): the live objects and the recording list.
        unsafe {
            let (sd, dd) = (src.GetDesc(), dst.GetDesc());
            let view = color_view_format(st.color_format);
            if !sharpness.is_finite()
                || !amount.is_finite()
                || !compatible(&sd, view, st.width, st.height)
                || !compatible(&dd, view, st.width, st.height)
                || (is_srgb(dd.Format) && dd.Format != st.color_format)
                || !dd.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)
            {
                return false;
            }
            let keep = [
                src.as_raw(),
                dst.as_raw(),
                st.scratch.as_raw(),
                st.srv_heap.as_raw(),
                st.rtv_heap.as_raw(),
                st.root.as_raw(),
                st.pso.as_raw(),
            ];
            if !crate::command_resources::keep(list, &keep) {
                return false;
            }
            let constants = [
                cpp_clamp(sharpness, 0.0, 1.0),
                4.0 * cpp_clamp(amount, 0.0, 1.0),
                if is_srgb(st.color_format) { 1.0 } else { 0.0 },
                0.0f32,
            ];
            list.CopyResource(&st.scratch, src);
            let to_read = transition(&st.scratch, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
            list.ResourceBarrier(&[to_read]);
            let in_place = src.as_raw() == dst.as_raw();
            if in_place {
                list.ResourceBarrier(&[transition(dst, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
            }
            let rtv_cpu = st.rtv_heap.GetCPUDescriptorHandleForHeapStart();
            let rtv = D3D12_RENDER_TARGET_VIEW_DESC {
                Format: st.color_format,
                ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            st.device.CreateRenderTargetView(dst, Some(&rtv), rtv_cpu);
            let vp = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: st.width as f32,
                Height: st.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            list.RSSetViewports(&[vp]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: st.width as i32,
                bottom: st.height as i32,
            }]);
            list.OMSetRenderTargets(1, Some(&rtv_cpu), false, None);
            list.SetGraphicsRootSignature(&st.root);
            list.SetPipelineState(&st.pso);
            list.SetDescriptorHeaps(&[Some(st.srv_heap.clone())]);
            list.SetGraphicsRoot32BitConstants(0, 4, constants.as_ptr() as *const c_void, 0);
            list.SetGraphicsRootDescriptorTable(1, st.srv_heap.GetGPUDescriptorHandleForHeapStart());
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.DrawInstanced(3, 1, 0, 0);
            if in_place {
                list.ResourceBarrier(&[transition(dst, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            }
            list.ResourceBarrier(&[transition(
                &st.scratch,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                D3D12_RESOURCE_STATE_COPY_DEST,
            )]);
        }
        true
    }

    pub fn shutdown(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// std::clamp(v, lo, hi) (a NaN passes through; callers have checked finiteness).
fn cpp_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

// ---- the C ABI (include/Render/SharpenPass.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_sharpen_pass_new() -> *mut Pass {
    Box::into_raw(Box::default())
}
/// # Safety
/// `pass` came from cvr_gpu_sharpen_pass_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_sharpen_pass_free(pass: *mut Pass) {
    if !pass.is_null() {
        // SAFETY: the caller's pass, freed once.
        drop(unsafe { Box::from_raw(pass) });
    }
}
/// # Safety
/// `pass` is live; `device` an ID3D12Device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_sharpen_pass_ensure(pass: *const Pass, device: *mut c_void, format: i32, width: u32, height: u32) -> bool {
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
pub unsafe extern "C" fn cvr_gpu_sharpen_pass_record(
    pass: *const Pass,
    list: *mut c_void,
    src: *mut c_void,
    dst: *mut c_void,
    sharpness: f32,
    amount: f32,
) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            pass.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&src),
            ID3D12Resource::from_raw_borrowed(&dst),
        ) {
            (Some(p), Some(l), Some(s), Some(d)) => p.record(l, s, d, sharpness, amount),
            _ => false,
        }
    }
}
/// # Safety
/// `pass` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_sharpen_pass_shutdown(pass: *const Pass) {
    // SAFETY: the caller's pass.
    if let Some(p) = unsafe { pass.as_ref() } {
        p.shutdown();
    }
}
