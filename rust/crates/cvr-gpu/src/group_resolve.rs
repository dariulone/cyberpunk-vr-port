//! Single pass's resolve of one eye's opaque group (formerly src/Render/StereoGroupResolve.cpp): the eye's slice of the
//! paired G-buffer arrays (two RGB10A2 targets, RGBA8 and depth/stencil) drawn back into the engine's own targets, with
//! the depth test and stencil writes of an opaque reverse-Z group. Only such groups qualify: ALWAYS/REPLACE stencil, full
//! MRT writes, no blending. Coverage the group never touched (stencil 255) is discarded. The caller restores the engine's
//! graphics bindings after Record.
use core::ffi::c_void;

use windows::core::{Interface, HRESULT, PCSTR};
use windows::Win32::Foundation::{E_INVALIDARG, RECT, S_OK};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SHADER: &str = r#"
Texture2DArray<float4> C0:register(t0), C1:register(t1), C2:register(t2);
Texture2DArray<float> D:register(t3);
Texture2DArray<uint2> S:register(t4);
cbuffer Params:register(b0) { uint Eye; };
float4 VS(uint id:SV_VertexID):SV_Position {
    return float4(id==2?3:-1,id==1?3:-1,0,1);
}
struct Out {float4 c0:SV_Target0;float4 c1:SV_Target1;float4 c2:SV_Target2;float depth:SV_Depth;};
Out PS(float4 p:SV_Position) {
    int4 q=int4(int2(p.xy),Eye,0);
    // 255 is reserved as untouched coverage; native reference 255 is rejected.
    if(S.Load(q).y==255)discard;
    Out o;o.c0=C0.Load(q);o.c1=C1.Load(q);o.c2=C2.Load(q);o.depth=D.Load(q);return o;
}
float DepthPS(float4 p:SV_Position):SV_Depth {
    int4 q=int4(int2(p.xy),Eye,0);if(S.Load(q).y==255)discard;return D.Load(q);
}"#;

const FORMATS: [DXGI_FORMAT; 4] = [
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R32G8X24_TYPELESS,
];

/// Supports: the opaque reverse-Z policy the resolve reproduces.
pub fn supports(blend: &D3D12_BLEND_DESC, depth: &D3D12_DEPTH_STENCIL_DESC) -> bool {
    if blend.AlphaToCoverageEnable.as_bool()
        || !depth.DepthEnable.as_bool()
        || depth.DepthWriteMask != D3D12_DEPTH_WRITE_MASK_ALL
        || depth.DepthFunc != D3D12_COMPARISON_FUNC_GREATER_EQUAL
        || !depth.StencilEnable.as_bool()
        || depth.StencilWriteMask != 255
    {
        return false;
    }
    let targets = if blend.IndependentBlendEnable.as_bool() { 3 } else { 1 };
    if blend.RenderTarget[..targets]
        .iter()
        .any(|b| b.BlendEnable.as_bool() || b.LogicOpEnable.as_bool() || b.RenderTargetWriteMask != 15)
    {
        return false;
    }
    [depth.FrontFace, depth.BackFace].iter().all(|f| {
        f.StencilFunc == D3D12_COMPARISON_FUNC_ALWAYS
            && f.StencilFailOp == D3D12_STENCIL_OP_KEEP
            && f.StencilDepthFailOp == D3D12_STENCIL_OP_KEEP
            && f.StencilPassOp == D3D12_STENCIL_OP_REPLACE
    })
}

fn compile(entry: &core::ffi::CStr, target: &core::ffi::CStr) -> Result<ID3DBlob, HRESULT> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source's bytes and length, NUL-terminated entry and target, out-blobs.
    unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(target.as_ptr() as *const u8),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    }
    .map_err(|e| e.code())?;
    code.ok_or(E_INVALIDARG)
}

/// StereoGroupResolve. The sources are borrowed: the caller keeps them alive while the resolve is used.
#[derive(Default)]
pub struct Resolve {
    root: Option<ID3D12RootSignature>,
    pipeline: Option<ID3D12PipelineState>,
    heap: Option<ID3D12DescriptorHeap>,
    sources: [*mut c_void; 4],
    depth_only: bool,
}

impl Resolve {
    /// Initialize: once, from the four paired arrays (two layers, one mip, one sample, one size; the colour ones are
    /// skipped for a depth-only group).
    pub fn initialize(&mut self, device: &ID3D12Device, sources: [*mut c_void; 4], depth_only: bool) -> HRESULT {
        if self.pipeline.is_some() {
            return E_INVALIDARG;
        }
        self.depth_only = depth_only;
        let first_index = if depth_only { 3 } else { 0 };
        let mut first = D3D12_RESOURCE_DESC::default();
        for (i, &source) in sources.iter().enumerate().skip(first_index) {
            // SAFETY: the caller's live resource (or null).
            let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&source) }) else {
                return E_INVALIDARG;
            };
            // SAFETY: a live resource.
            let d = unsafe { resource.GetDesc() };
            if i == first_index {
                first = d;
            }
            if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || d.DepthOrArraySize != 2
                || d.MipLevels != 1
                || d.SampleDesc.Count != 1
                || d.Width != first.Width
                || d.Height != first.Height
                || d.Format != FORMATS[i]
                || (d.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) != D3D12_RESOURCE_FLAG_NONE
            {
                return E_INVALIDARG;
            }
        }
        match self.build(device, sources, depth_only) {
            Ok(()) => S_OK,
            Err(hr) => hr,
        }
    }

    fn build(&mut self, device: &ID3D12Device, sources: [*mut c_void; 4], depth_only: bool) -> Result<(), HRESULT> {
        // the shaders made from Rust (rust/shaders/passes/src/group_resolve.rs); the HLSL above with CVR_SHADER_HLSL
        let failed = windows::Win32::Foundation::E_FAIL;
        let vs = crate::dxil::pick(crate::dxil::GROUP_RESOLVE_VS, || compile(c"VS", c"vs_5_0").ok()).ok_or(failed)?;
        let ps = if depth_only {
            crate::dxil::pick(crate::dxil::GROUP_RESOLVE_DEPTH_PS, || compile(c"DepthPS", c"ps_5_0").ok())
        } else {
            crate::dxil::pick(crate::dxil::GROUP_RESOLVE_PS, || compile(c"PS", c"ps_5_0").ok())
        }
        .ok_or(failed)?;
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 5,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };
        let params = [
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
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 1,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 2,
            pParameters: params.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: core::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        };
        let mut blob: Option<ID3DBlob> = None;
        // SAFETY (whole block): the live device; descriptions on the stack outliving the calls that read them.
        unsafe {
            D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).map_err(|e| e.code())?;
            let blob = blob.ok_or(E_INVALIDARG)?;
            let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
            let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes).map_err(|e| e.code())?;
            self.root = Some(root.clone());
            let mut pd = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                VS: vs.bytecode(),
                PS: ps.bytecode(),
                PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
                NumRenderTargets: if depth_only { 0 } else { 3 },
                DSVFormat: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                SampleMask: u32::MAX,
                ..Default::default()
            };
            let count = pd.NumRenderTargets as usize;
            pd.RTVFormats[..count].copy_from_slice(&FORMATS[..count]);
            pd.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
            pd.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
            pd.RasterizerState.DepthClipEnable = true.into();
            for b in &mut pd.BlendState.RenderTarget {
                b.SrcBlend = D3D12_BLEND_ONE;
                b.SrcBlendAlpha = D3D12_BLEND_ONE;
                b.DestBlend = D3D12_BLEND_ZERO;
                b.DestBlendAlpha = D3D12_BLEND_ZERO;
                b.BlendOp = D3D12_BLEND_OP_ADD;
                b.BlendOpAlpha = D3D12_BLEND_OP_ADD;
                b.LogicOp = D3D12_LOGIC_OP_NOOP;
                b.RenderTargetWriteMask = 15;
            }
            let ds = &mut pd.DepthStencilState;
            ds.DepthEnable = true.into();
            ds.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ALL;
            ds.DepthFunc = D3D12_COMPARISON_FUNC_GREATER_EQUAL;
            ds.StencilEnable = true.into();
            ds.StencilReadMask = 255;
            ds.StencilWriteMask = 255;
            let face = D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_REPLACE,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            };
            ds.FrontFace = face;
            ds.BackFace = face;
            let pipeline: ID3D12PipelineState = device.CreateGraphicsPipelineState(&pd).map_err(|e| e.code())?;
            self.pipeline = Some(pipeline);
            let hd = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 5,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            };
            let heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&hd).map_err(|e| e.code())?;
            let mut handle = heap.GetCPUDescriptorHandleForHeapStart();
            let stride = device.GetDescriptorHandleIncrementSize(hd.Type) as usize;
            for i in 0..5 {
                let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: match i {
                        0..=2 => FORMATS[i],
                        3 => DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
                        _ => DXGI_FORMAT_X32_TYPELESS_G8X24_UINT,
                    },
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                            MipLevels: 1,
                            ArraySize: 2,
                            PlaneSlice: if i == 4 { 1 } else { 0 },
                            ..Default::default()
                        },
                    },
                };
                let source = sources[if i < 4 { i } else { 3 }];
                // A depth-only group's colour views are null descriptors (never read).
                device.CreateShaderResourceView(ID3D12Resource::from_raw_borrowed(&source), Some(&view), handle);
                handle.ptr += stride;
            }
            self.heap = Some(heap);
        }
        self.sources = sources;
        Ok(())
    }

    /// Retain: the resolve's objects and its sources live as long as the list's submissions.
    pub fn retain(&self, list: &ID3D12GraphicsCommandList) -> bool {
        let (Some(root), Some(pipeline)) = (&self.root, &self.pipeline) else {
            return false;
        };
        let heap = self.heap.as_ref().map_or(core::ptr::null_mut(), |h| h.as_raw());
        crate::command_resources::keep(list, &[root.as_raw(), pipeline.as_raw(), heap, self.sources[3]])
            && (self.depth_only || crate::command_resources::keep(list, &self.sources[..3]))
    }

    /// Record: the eye's slice drawn into `targets` and `depth` under `viewport`/`scissor`, with the group's stencil
    /// reference; the sources are pixel-shader resources only while the draw runs.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        stencil: u32,
        viewport: &D3D12_VIEWPORT,
        scissor: &RECT,
        targets: &[D3D12_CPU_DESCRIPTOR_HANDLE; 3],
        depth: D3D12_CPU_DESCRIPTOR_HANDLE,
    ) {
        let (Some(root), Some(pipeline), Some(heap)) = (&self.root, &self.pipeline, &self.heap) else {
            return;
        };
        let first = if self.depth_only { 3 } else { 0 };
        let barrier = |i: usize, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES| D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                    pResource: unsafe { core::mem::transmute_copy(&self.sources[i]) },
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        };
        let written = |i: usize| {
            if i == 3 {
                D3D12_RESOURCE_STATE_DEPTH_WRITE
            } else {
                D3D12_RESOURCE_STATE_RENDER_TARGET
            }
        };
        let to_read: Vec<_> = (first..4).map(|i| barrier(i, written(i), D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)).collect();
        let to_write: Vec<_> = (first..4).map(|i| barrier(i, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, written(i))).collect();
        // SAFETY: the caller's open list; the resolve's objects are alive (Retain keeps them for the submission).
        unsafe {
            list.ResourceBarrier(&to_read);
            list.SetPipelineState(pipeline);
            list.SetGraphicsRootSignature(root);
            list.SetDescriptorHeaps(&[Some(heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, heap.GetGPUDescriptorHandleForHeapStart());
            list.SetGraphicsRoot32BitConstant(1, eye, 0);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.RSSetViewports(core::slice::from_ref(viewport));
            list.RSSetScissorRects(core::slice::from_ref(scissor));
            if self.depth_only {
                list.OMSetRenderTargets(0, None, false, Some(&depth));
            } else {
                list.OMSetRenderTargets(3, Some(targets.as_ptr()), false, Some(&depth));
            }
            list.OMSetStencilRef(stencil);
            list.DrawInstanced(3, 1, 0, 0);
            list.ResourceBarrier(&to_write);
        }
    }
}

// The header's wrapper (StereoGroupResolve.hpp in the Rust build).

/// # Safety
/// Both point to their descriptions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_group_resolve_supports(blend: *const D3D12_BLEND_DESC, depth: *const D3D12_DEPTH_STENCIL_DESC) -> bool {
    // SAFETY: the caller's descriptions.
    match (unsafe { blend.as_ref() }, unsafe { depth.as_ref() }) {
        (Some(b), Some(d)) => supports(b, d),
        _ => false,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_group_resolve_new() -> *mut Resolve {
    Box::into_raw(Box::default())
}
/// # Safety
/// `r` came from cvr_group_resolve_new (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_group_resolve_free(r: *mut Resolve) {
    if !r.is_null() {
        // SAFETY: the wrapper's resolve, dropped once.
        drop(unsafe { Box::from_raw(r) });
    }
}
/// # Safety
/// `r` is a live resolve; `device` a live device (or null); `sources` points to four resources (live or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_group_resolve_initialize(r: *mut Resolve, device: *mut c_void, sources: *const [*mut c_void; 4], depth_only: bool) -> HRESULT {
    // SAFETY: the wrapper's resolve, the caller's device and sources.
    let (Some(r), Some(device), Some(sources)) = (unsafe { r.as_mut() }, unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe {
        sources.as_ref()
    }) else {
        return E_INVALIDARG;
    };
    r.initialize(device, *sources, depth_only)
}
/// # Safety
/// `r` is a live resolve; `list` a live list (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_group_resolve_retain(r: *const Resolve, list: *mut c_void) -> bool {
    // SAFETY: the wrapper's resolve and the caller's list.
    match (unsafe { r.as_ref() }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) {
        (Some(r), Some(list)) => r.retain(list),
        _ => false,
    }
}
/// # Safety
/// `r` is a live resolve; `list` an open list; the remaining pointers point to their values.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_group_resolve_record(
    r: *const Resolve,
    list: *mut c_void,
    eye: u32,
    stencil: u32,
    viewport: *const D3D12_VIEWPORT,
    scissor: *const RECT,
    targets: *const [D3D12_CPU_DESCRIPTOR_HANDLE; 3],
    depth: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    // SAFETY: the wrapper's resolve and the caller's list and values.
    let (Some(r), Some(list), Some(viewport), Some(scissor), Some(targets)) = (
        unsafe { r.as_ref() },
        unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) },
        unsafe { viewport.as_ref() },
        unsafe { scissor.as_ref() },
        unsafe { targets.as_ref() },
    ) else {
        return;
    };
    r.record(list, eye, stencil, viewport, scissor, targets, depth);
}
