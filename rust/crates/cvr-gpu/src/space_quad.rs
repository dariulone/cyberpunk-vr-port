//! Pictures hung in the tracking space, drawn into an eye's image (formerly src/Render/SpaceQuad.cpp; the header's
//! inline helpers -- LocatedPose, Ahead, FreeLookHeading -- stay in include/Render/SpaceQuad.hpp). A quad placed in the
//! space the eyes are tracked in is drawn into one eye's image through the pose and frustum that image is submitted
//! with, so the compositor shows it where it hangs and the eyes fuse it at its distance. One drawer for every such
//! picture (the intro's overlay, the loading screen); frame-capture thread only, as the C++ class was. The shaders
//! are Rust (rust/shaders/passes/src/space_quad.rs), embedded as DXIL (dxil.rs). The HLSL they replace stays below only
//! as the reference of the A/B (CVR_SHADER_AB, shader_ab.rs) and the way back (CVR_SHADER_HLSL) until the replacement
//! is confirmed in both eyes.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use std::collections::BTreeMap;

use cvr_body::cmath;
use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const D3DCOMPILE_ENABLE_STRICTNESS: u32 = 1 << 11;
/// b0: the quad's four corners as clip coordinates (top left, top right, bottom left, bottom right); the texture's part
/// on it (u0, v0, u1, v1); x = opacity, y = black point; the fill's colour.
const CONSTANTS: u32 = 28;
/// A shader-visible descriptor is read when the list runs, not when it is recorded: the ring is long enough for every
/// draw of the capture lists in flight. Slot 0 is a null view, for the fill.
const DESCRIPTORS: u32 = 256;
/// The full target, as the fill and the resample draw it.
const FULL: [f32; 16] = [-1.0, 1.0, 0.5, 1.0, 1.0, 1.0, 0.5, 1.0, -1.0, -1.0, 0.5, 1.0, 1.0, -1.0, 0.5, 1.0];

/// A picture on a quad is read through its mips by the footprint of each pixel, anisotropically (s0); the resample pass
/// reads one level at one bilinear tap (s1).
const SHADER: &str = r#"
Texture2D<float4> g_picture : register(t0);
SamplerState g_footprint : register(s0);
SamplerState g_linear : register(s1);
cbuffer Quad : register(b0) { float4 g_corner[4]; float4 g_uv; float4 g_params; float4 g_colour; };
struct VSOut { float4 position : SV_Position; float2 uv : TEXCOORD0; };
VSOut VSMain(uint id : SV_VertexID) {
    VSOut o;
    o.position = g_corner[id];
    o.uv = lerp(g_uv.xy, g_uv.zw, float2(id & 1, id >> 1));
    return o;
}
float4 PSFill(VSOut i) : SV_Target { return g_colour; }
float4 PSOver(VSOut i) : SV_Target { return float4(g_picture.Sample(g_footprint, i.uv).rgb, g_params.x); }
float4 PSCopy(VSOut i) : SV_Target { return float4(g_picture.SampleLevel(g_linear, i.uv, 0.0).rgb, 1.0); }
float4 PSLight(VSOut i) : SV_Target {
    float3 c = g_picture.Sample(g_footprint, i.uv).rgb;
    if (g_params.y > 0.0) {
        const float peak = max(c.r, max(c.g, c.b));
        c *= saturate((peak - g_params.y) / max(peak * (1.0 - g_params.y), 1e-5));
    }
    return float4(c * g_params.x, 0.0);
}
"#;

/// XrQuaternionf, XrVector3f, XrPosef, XrFovf.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Pose {
    pub orientation: Quat,
    pub position: Vec3,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Fov {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_up: f32,
    pub angle_down: f32,
}
/// cvr::render::SpaceQuad: its centre (facing along the pose's +Z, as an OpenXR quad layer), its size (m) and the part
/// of the texture on it (u0, v0, u1, v1).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SpaceQuad {
    pub pose: Pose,
    pub width: f32,
    pub height: f32,
    pub uv: [f32; 4],
}
/// SpaceQuads::Target: an eye's image -- the resource, the format it is written through, the eye's rectangle in it,
/// the mip level.
#[repr(C)]
pub struct Target {
    pub resource: *mut c_void,
    pub view: i32,
    pub rect: RECT,
    pub mip: u32,
}
const _: () = assert!(core::mem::size_of::<SpaceQuad>() == 52 && core::mem::size_of::<Target>() == 32);

/// How a picture meets what is under it (cvr::render::Blend).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// A solid picture over what is there, as opaque as its opacity.
    Over,
    /// Light added to what is there: black is clear, light under the black point dropped.
    Light,
}

fn rotate(q: &Quat, v: Vec3) -> Vec3 {
    let tx = 2.0 * (q.y * v.z - q.z * v.y);
    let ty = 2.0 * (q.z * v.x - q.x * v.z);
    let tz = 2.0 * (q.x * v.y - q.y * v.x);
    Vec3 {
        x: v.x + q.w * tx + (q.y * tz - q.z * ty),
        y: v.y + q.w * ty + (q.z * tx - q.x * tz),
        z: v.z + q.w * tz + (q.x * ty - q.y * tx),
    }
}
/// A point of the tracking space as this eye's clip coordinates (OpenXR: -Z ahead, +Y up). Depth is kept inside 0..w
/// for anything ahead of the eye: there is no depth buffer, only the eye's own plane clips.
fn clip(eye: &Pose, fov: &Fov, point: Vec3, out: &mut [f32]) {
    let inverse = Quat {
        x: -eye.orientation.x,
        y: -eye.orientation.y,
        z: -eye.orientation.z,
        w: eye.orientation.w,
    };
    let v = rotate(
        &inverse,
        Vec3 {
            x: point.x - eye.position.x,
            y: point.y - eye.position.y,
            z: point.z - eye.position.z,
        },
    );
    let w = -v.z;
    let (l, r) = (cmath::tan(fov.angle_left), cmath::tan(fov.angle_right));
    let (d, u) = (cmath::tan(fov.angle_down), cmath::tan(fov.angle_up));
    out[0] = (2.0 * v.x - (r + l) * w) / (r - l);
    out[1] = (2.0 * v.y - (u + d) * w) / (u - d);
    out[2] = 0.5 * w;
    out[3] = w;
}
/// The quad's corners as the eye's clip coordinates, in the order the strip draws them.
fn corners(eye: &Pose, fov: &Fov, quad: &SpaceQuad, out: &mut [f32]) {
    let right = rotate(
        &quad.pose.orientation,
        Vec3 {
            x: quad.width * 0.5,
            y: 0.0,
            z: 0.0,
        },
    );
    let up = rotate(
        &quad.pose.orientation,
        Vec3 {
            x: 0.0,
            y: quad.height * 0.5,
            z: 0.0,
        },
    );
    let c = quad.pose.position;
    let points = [
        Vec3 {
            x: c.x - right.x + up.x,
            y: c.y - right.y + up.y,
            z: c.z - right.z + up.z,
        },
        Vec3 {
            x: c.x + right.x + up.x,
            y: c.y + right.y + up.y,
            z: c.z + right.z + up.z,
        },
        Vec3 {
            x: c.x - right.x - up.x,
            y: c.y - right.y - up.y,
            z: c.z - right.z - up.z,
        },
        Vec3 {
            x: c.x + right.x - up.x,
            y: c.y + right.y - up.y,
            z: c.z + right.z - up.z,
        },
    ];
    for (i, p) in points.into_iter().enumerate() {
        clip(eye, fov, p, &mut out[4 * i..4 * i + 4]);
    }
}

fn compile(entry: &core::ffi::CStr, target: &core::ffi::CStr) -> Option<ID3DBlob> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the shader's bytes and length, NUL-terminated entry and target, out-blobs.
    let hr = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
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
    if hr.is_ok() {
        return code;
    }
    let text = match &errors {
        // SAFETY: the blob's own buffer.
        Some(b) => unsafe { String::from_utf8_lossy(core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() },
        None => String::new(),
    };
    log!("[space-quad] shader {} failed: {}\n", entry.to_string_lossy(), text);
    None
}

struct Pipelines {
    fill: ID3D12PipelineState,
    over: ID3D12PipelineState,
    light: ID3D12PipelineState,
    copy: ID3D12PipelineState,
}

use crate::dxil::Code;

/// The pass's five stages: the vertex shader and the four pixel shaders.
struct Stages {
    vs: Code,
    fill: Code,
    over: Code,
    light: Code,
    copy: Code,
}
impl Stages {
    fn dxil() -> Self {
        Stages {
            vs: Code::Dxil(crate::dxil::SPACE_QUAD_VS),
            fill: Code::Dxil(crate::dxil::SPACE_QUAD_FILL),
            over: Code::Dxil(crate::dxil::SPACE_QUAD_OVER),
            light: Code::Dxil(crate::dxil::SPACE_QUAD_LIGHT),
            copy: Code::Dxil(crate::dxil::SPACE_QUAD_COPY),
        }
    }
    fn hlsl() -> Option<Self> {
        Some(Stages {
            vs: Code::Hlsl(compile(c"VSMain", c"vs_5_0")?),
            fill: Code::Hlsl(compile(c"PSFill", c"ps_5_0")?),
            over: Code::Hlsl(compile(c"PSOver", c"ps_5_0")?),
            light: Code::Hlsl(compile(c"PSLight", c"ps_5_0")?),
            copy: Code::Hlsl(compile(c"PSCopy", c"ps_5_0")?),
        })
    }
}

/// cvr::render::SpaceQuads.
#[derive(Default)]
pub struct SpaceQuads {
    device: Option<ID3D12Device>,
    root: Option<ID3D12RootSignature>,
    stages: Option<Stages>,
    pipelines: BTreeMap<i32, Pipelines>,
    /// CVR_SHADER_AB: the HLSL's pipelines beside the DXIL's, and the takes.
    reference: Option<(Stages, BTreeMap<i32, Pipelines>)>,
    ab: crate::shader_ab::ShaderAb,
    /// The target the last bind set: its view and rectangle (restored after a take).
    bound: Option<(D3D12_CPU_DESCRIPTOR_HANDLE, RECT)>,
    srv_heap: Option<ID3D12DescriptorHeap>,
    rtv_heap: Option<ID3D12DescriptorHeap>,
    srv_stride: u32,
    rtv_stride: u32,
    cursor: u32,
    failed: bool,
}

fn bytecode(code: &Code) -> D3D12_SHADER_BYTECODE {
    code.bytecode()
}

/// The pass's four pipelines for a target format from `stages` (not made when one fails).
fn make_pipelines(device: &ID3D12Device, root: &ID3D12RootSignature, stages: &Stages, format: i32) -> Option<Pipelines> {
    // SAFETY: the description borrows the root signature (released below without a release of its own) and the
    // shaders' code, alive across the calls.
    unsafe {
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(root),
            VS: bytecode(&stages.vs),
            SampleMask: u32::MAX,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        desc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        desc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        desc.RasterizerState.DepthClipEnable = true.into();
        desc.RTVFormats[0] = DXGI_FORMAT(format);
        {
            let blend = &mut desc.BlendState.RenderTarget[0];
            blend.BlendEnable = true.into();
            blend.BlendOp = D3D12_BLEND_OP_ADD;
            blend.BlendOpAlpha = D3D12_BLEND_OP_ADD;
            blend.SrcBlendAlpha = D3D12_BLEND_ZERO;
            blend.DestBlendAlpha = D3D12_BLEND_ONE;
            blend.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        }
        // The fill and a solid picture blend by their alpha; light adds; a resample writes.
        let mut make = |ps: &Code, light: bool, enable: bool| -> Option<ID3D12PipelineState> {
            desc.PS = bytecode(ps);
            let blend = &mut desc.BlendState.RenderTarget[0];
            blend.BlendEnable = enable.into();
            blend.SrcBlend = if light { D3D12_BLEND_ONE } else { D3D12_BLEND_SRC_ALPHA };
            blend.DestBlend = if light { D3D12_BLEND_ONE } else { D3D12_BLEND_INV_SRC_ALPHA };
            match device.CreateGraphicsPipelineState(&desc) {
                Ok(p) => Some(p),
                Err(e) => {
                    log!("[space-quad] pipeline for format {} not made: {e}\n", format as u32);
                    None
                }
            }
        };
        let made = (|| {
            let fill = make(&stages.fill, false, true)?;
            let over = make(&stages.over, false, true)?;
            let light = make(&stages.light, true, true)?;
            let copy = make(&stages.copy, false, false)?;
            Some(Pipelines { fill, over, light, copy })
        })();
        let _ = core::mem::ManuallyDrop::into_inner(core::mem::take(&mut desc.pRootSignature)).map(core::mem::forget);
        made
    }
}

impl SpaceQuads {
    /// The device's objects, made once; a device other than the first is refused, and so is every call after a
    /// failure (until Shutdown).
    fn ensure_device(&mut self, device: Option<&ID3D12Device>) -> bool {
        if let Some(d) = &self.device {
            return device.is_some_and(|device| d.as_raw() == device.as_raw());
        }
        if self.failed {
            return false;
        }
        self.failed = true;
        let Some(device) = device else { return false };
        // the shaders made from Rust; the HLSL only when asked for (the way back) or as the A/B's reference
        if crate::dxil::hlsl_requested() {
            self.stages = Stages::hlsl();
            if self.stages.is_none() {
                return false;
            }
            log!("[space-quad] the HLSL shaders (CVR_SHADER_HLSL)\n");
        } else {
            self.stages = Some(Stages::dxil());
            if crate::shader_ab::enabled() {
                self.reference = Stages::hlsl().map(|s| (s, BTreeMap::new()));
                log!("[space-quad] A/B: the HLSL reference compiled: {}\n", self.reference.is_some());
            }
        }
        // SAFETY (the rest): the live device; descriptions on the stack outliving the calls that read them.
        unsafe {
            let range = D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 1,
                ..Default::default()
            };
            let parameters = [
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
                            Num32BitValues: CONSTANTS,
                        },
                    },
                    ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
                },
            ];
            let sampler = |register: u32, filter: D3D12_FILTER, anisotropy: u32| D3D12_STATIC_SAMPLER_DESC {
                Filter: filter,
                AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                MaxAnisotropy: anisotropy,
                MaxLOD: D3D12_FLOAT32_MAX,
                ShaderRegister: register,
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
                ..Default::default()
            };
            let samplers = [sampler(0, D3D12_FILTER_ANISOTROPIC, 8), sampler(1, D3D12_FILTER_MIN_MAG_MIP_LINEAR, 0)];
            let desc = D3D12_ROOT_SIGNATURE_DESC {
                NumParameters: 2,
                pParameters: parameters.as_ptr(),
                NumStaticSamplers: 2,
                pStaticSamplers: samplers.as_ptr(),
                Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
            };
            let mut blob: Option<ID3DBlob> = None;
            if D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).is_err() {
                return false;
            }
            let Some(blob) = blob else { return false };
            let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
            let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(0, bytes) else {
                return false;
            };
            self.root = Some(root);
            let heap = |kind, flags| {
                device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: kind,
                    NumDescriptors: DESCRIPTORS,
                    Flags: flags,
                    NodeMask: 0,
                })
            };
            let Ok(srv_heap) = heap(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE) else {
                return false;
            };
            self.srv_heap = Some(srv_heap.clone());
            let Ok(rtv_heap) = heap(D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_DESCRIPTOR_HEAP_FLAG_NONE) else {
                return false;
            };
            self.rtv_heap = Some(rtv_heap);
            self.srv_stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            self.rtv_stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let none = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: 1,
                        ..Default::default()
                    },
                },
            };
            device.CreateShaderResourceView(None, Some(&none), srv_heap.GetCPUDescriptorHandleForHeapStart());
        }
        self.device = Some(device.clone());
        self.failed = false;
        true
    }

    /// The four pipelines for a target format, made at its first use (not kept when one fails); with the A/B, the
    /// HLSL reference's beside them.
    fn pipelines_for(&mut self, format: i32) -> bool {
        if self.pipelines.contains_key(&format) {
            return true;
        }
        let (Some(device), Some(root), Some(stages)) = (&self.device, &self.root, &self.stages) else {
            return false;
        };
        let Some(made) = make_pipelines(device, root, stages, format) else { return false };
        log!("[space-quad] pipelines for format {}\n", format as u32);
        self.pipelines.insert(format, made);
        if let Some((reference, made)) = &mut self.reference {
            if let Some(p) = make_pipelines(device, root, reference, format) {
                made.insert(format, p);
            }
        }
        true
    }

    /// The target's rectangle bound; the slot of the descriptors this draw uses.
    unsafe fn bind(&mut self, list: &ID3D12GraphicsCommandList, target: &Target, resource: &ID3D12Resource) -> u32 {
        let slot = 1 + self.cursor;
        self.cursor = (self.cursor + 1) % (DESCRIPTORS - 1);
        let (Some(device), Some(rtv_heap), Some(srv_heap), Some(root)) = (&self.device, &self.rtv_heap, &self.srv_heap, &self.root) else {
            return slot;
        };
        let mut rtv = rtv_heap.GetCPUDescriptorHandleForHeapStart();
        rtv.ptr += slot as usize * self.rtv_stride as usize;
        let view = D3D12_RENDER_TARGET_VIEW_DESC {
            Format: DXGI_FORMAT(target.view),
            ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_RTV {
                    MipSlice: target.mip,
                    PlaneSlice: 0,
                },
            },
        };
        device.CreateRenderTargetView(resource, Some(&view), rtv);
        let r = target.rect;
        let viewport = D3D12_VIEWPORT {
            TopLeftX: r.left as f32,
            TopLeftY: r.top as f32,
            Width: (r.right - r.left) as f32,
            Height: (r.bottom - r.top) as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        list.RSSetViewports(&[viewport]);
        list.RSSetScissorRects(&[r]);
        list.OMSetRenderTargets(1, Some(&rtv), false, None);
        self.bound = Some((rtv, r));
        list.SetGraphicsRootSignature(root);
        list.SetDescriptorHeaps(&[Some(srv_heap.clone())]);
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        slot
    }
    /// `levels` of `texture` from `mip` on as the draw's picture, in the slot's descriptor.
    unsafe fn bind_picture(&self, list: &ID3D12GraphicsCommandList, slot: u32, texture: &ID3D12Resource, view: i32, mip: u32, levels: u32) {
        let (Some(device), Some(srv_heap)) = (&self.device, &self.srv_heap) else {
            return;
        };
        let mut cpu = srv_heap.GetCPUDescriptorHandleForHeapStart();
        let mut gpu = srv_heap.GetGPUDescriptorHandleForHeapStart();
        cpu.ptr += slot as usize * self.srv_stride as usize;
        gpu.ptr += slot as u64 * self.srv_stride as u64;
        let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT(view),
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: mip,
                    MipLevels: levels,
                    ..Default::default()
                },
            },
        };
        device.CreateShaderResourceView(texture, Some(&srv), cpu);
        list.SetGraphicsRootDescriptorTable(0, gpu);
    }
    unsafe fn null_picture(&self, list: &ID3D12GraphicsCommandList) {
        if let Some(srv_heap) = &self.srv_heap {
            list.SetGraphicsRootDescriptorTable(0, srv_heap.GetGPUDescriptorHandleForHeapStart());
        }
    }
    unsafe fn draw_with(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        target: &Target,
        kind: &str,
        pick: fn(&Pipelines) -> &ID3D12PipelineState,
        constants: &[f32; 28],
    ) {
        let format = target.view;
        self.ab.poll();
        // CVR_SHADER_AB: this draw again, by the HLSL reference and by the DXIL, into the take's targets
        // an eye is its rectangle in the target (the capture's targets are a ring: their resources change each frame)
        let key = format!("space_quad.{kind} rect {},{} format {}", target.rect.left, target.rect.top, format as u32);
        if self.ab.wants(&key) {
            if let (Some(device), Some((_, reference)), Some((rtv, rect))) = (self.device.clone(), &self.reference, self.bound) {
                if let (Some(r), Some(p)) = (reference.get(&format), self.pipelines.get(&format)) {
                    let (r, p) = (pick(r).clone(), pick(p).clone());
                    let (w, h) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
                    self.ab.take(&device, list, &key, DXGI_FORMAT(format), w, h, |replacement| {
                        list.SetPipelineState(if replacement { &p } else { &r });
                        list.SetGraphicsRoot32BitConstants(1, CONSTANTS, constants.as_ptr() as *const c_void, 0);
                        list.DrawInstanced(4, 1, 0, 0);
                    });
                    // the caller's target back
                    let viewport = D3D12_VIEWPORT {
                        TopLeftX: rect.left as f32,
                        TopLeftY: rect.top as f32,
                        Width: (rect.right - rect.left) as f32,
                        Height: (rect.bottom - rect.top) as f32,
                        MinDepth: 0.0,
                        MaxDepth: 1.0,
                    };
                    list.RSSetViewports(&[viewport]);
                    list.RSSetScissorRects(&[rect]);
                    list.OMSetRenderTargets(1, Some(&rtv), false, None);
                }
            }
        }
        let Some(p) = self.pipelines.get(&format) else { return };
        list.SetPipelineState(pick(p));
        list.SetGraphicsRoot32BitConstants(1, CONSTANTS, constants.as_ptr() as *const c_void, 0);
        list.DrawInstanced(4, 1, 0, 0);
    }
    /// What every draw asks first: a list, a target with a rectangle, the device's objects and the format's pipelines.
    fn ready<'a>(&mut self, device: Option<&ID3D12Device>, target: &'a Target, list: Option<&ID3D12GraphicsCommandList>) -> Option<ID3D12Resource> {
        list?;
        // SAFETY: the caller's resource or null.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&target.resource) }?.clone();
        let r = target.rect;
        if r.right <= r.left || r.bottom <= r.top || !self.ensure_device(device) || !self.pipelines_for(target.view) {
            return None;
        }
        Some(resource)
    }

    /// Fill: the rectangle covered with a colour, as much as its alpha.
    pub fn fill(&mut self, device: Option<&ID3D12Device>, list: Option<&ID3D12GraphicsCommandList>, target: &Target, rgba: &[f32; 4]) -> bool {
        let Some(resource) = self.ready(device, target, list) else { return false };
        let list = list.unwrap();
        // SAFETY: the recording list and the live objects.
        unsafe {
            self.bind(list, target, &resource);
            self.null_picture(list);
            let mut constants = [0.0f32; 28];
            constants[..16].copy_from_slice(&FULL);
            constants[24..].copy_from_slice(rgba);
            self.draw_with(list, target, "fill", |p| &p.fill, &constants);
        }
        true
    }
    /// Solid: `quad` in one colour, as much as its alpha, as the eye at `eye` with `fov` sees it.
    pub fn solid(
        &mut self,
        device: Option<&ID3D12Device>,
        list: Option<&ID3D12GraphicsCommandList>,
        target: &Target,
        eye: &Pose,
        fov: &Fov,
        quad: &SpaceQuad,
        rgba: &[f32; 4],
    ) -> bool {
        let Some(resource) = self.ready(device, target, list) else { return false };
        let list = list.unwrap();
        // SAFETY: as fill.
        unsafe {
            self.bind(list, target, &resource);
            self.null_picture(list);
            let mut constants = [0.0f32; 28];
            corners(eye, fov, quad, &mut constants);
            constants[24..].copy_from_slice(rgba);
            self.draw_with(list, target, "solid", |p| &p.fill, &constants);
        }
        true
    }
    /// Draw: `texture`, read through `view`, on `quad` as the eye at `eye` with `fov` sees it, through all its mips.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: Option<&ID3D12Device>,
        list: Option<&ID3D12GraphicsCommandList>,
        target: &Target,
        eye: &Pose,
        fov: &Fov,
        texture: Option<&ID3D12Resource>,
        view: i32,
        quad: &SpaceQuad,
        blend: Blend,
        opacity: f32,
        black_point: f32,
    ) -> bool {
        let Some(texture) = texture else { return false };
        let Some(resource) = self.ready(device, target, list) else { return false };
        let list = list.unwrap();
        // SAFETY: as fill; the caller's texture.
        unsafe {
            let slot = self.bind(list, target, &resource);
            self.bind_picture(list, slot, texture, view, 0, u32::MAX);
            let mut constants = [0.0f32; 28];
            corners(eye, fov, quad, &mut constants);
            constants[16..20].copy_from_slice(&quad.uv);
            constants[20] = cmath::clamp(opacity, 0.0, 1.0);
            constants[21] = black_point;
            if blend == Blend::Light {
                self.draw_with(list, target, "light", |p| &p.light, &constants);
            } else {
                self.draw_with(list, target, "over", |p| &p.over, &constants);
            }
        }
        true
    }
    /// Resample: the part `uv` of `texture`'s level `mip`, read through `view`, written over the whole target rectangle
    /// (one bilinear tap a pixel).
    #[allow(clippy::too_many_arguments)]
    pub fn resample(
        &mut self,
        device: Option<&ID3D12Device>,
        list: Option<&ID3D12GraphicsCommandList>,
        target: &Target,
        texture: Option<&ID3D12Resource>,
        view: i32,
        mip: u32,
        uv: &[f32; 4],
    ) -> bool {
        let Some(texture) = texture else { return false };
        let Some(resource) = self.ready(device, target, list) else { return false };
        let list = list.unwrap();
        // SAFETY: as draw.
        unsafe {
            let slot = self.bind(list, target, &resource);
            self.bind_picture(list, slot, texture, view, mip, 1);
            let mut constants = [0.0f32; 28];
            constants[..16].copy_from_slice(&FULL);
            constants[16..20].copy_from_slice(uv);
            self.draw_with(list, target, "copy", |p| &p.copy, &constants);
        }
        true
    }
    /// Shutdown: every object released, the ring at its start.
    pub fn shutdown(&mut self) {
        *self = SpaceQuads::default();
    }
}

// ---- the C ABI (include/Render/SpaceQuad.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_space_quads_new() -> *mut SpaceQuads {
    Box::into_raw(Box::default())
}
/// # Safety
/// `quads` came from cvr_gpu_space_quads_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_free(quads: *mut SpaceQuads) {
    if !quads.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(quads) });
    }
}
/// # Safety
/// `quads` is live and used by one thread; the others are the caller's live objects or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_fill(
    quads: *mut SpaceQuads,
    device: *mut c_void,
    list: *mut c_void,
    target: *const Target,
    rgba: *const [f32; 4],
) -> bool {
    // SAFETY: as the caller vouches.
    unsafe {
        let (Some(q), Some(target), Some(rgba)) = (quads.as_mut(), target.as_ref(), rgba.as_ref()) else {
            return false;
        };
        q.fill(
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            target,
            rgba,
        )
    }
}
/// # Safety
/// As cvr_gpu_space_quads_fill.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_solid(
    quads: *mut SpaceQuads,
    device: *mut c_void,
    list: *mut c_void,
    target: *const Target,
    eye: *const Pose,
    fov: *const Fov,
    quad: *const SpaceQuad,
    rgba: *const [f32; 4],
) -> bool {
    // SAFETY: as the caller vouches.
    unsafe {
        let (Some(q), Some(target), Some(eye), Some(fov), Some(quad), Some(rgba)) =
            (quads.as_mut(), target.as_ref(), eye.as_ref(), fov.as_ref(), quad.as_ref(), rgba.as_ref())
        else {
            return false;
        };
        q.solid(
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            target,
            eye,
            fov,
            quad,
            rgba,
        )
    }
}
/// # Safety
/// As cvr_gpu_space_quads_fill; `blend` is cvr::render::Blend (0 Over, 1 Light).
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_draw(
    quads: *mut SpaceQuads,
    device: *mut c_void,
    list: *mut c_void,
    target: *const Target,
    eye: *const Pose,
    fov: *const Fov,
    texture: *mut c_void,
    view: i32,
    quad: *const SpaceQuad,
    blend: i32,
    opacity: f32,
    black_point: f32,
) -> bool {
    // SAFETY: as the caller vouches.
    unsafe {
        let (Some(q), Some(target), Some(eye), Some(fov), Some(quad)) = (quads.as_mut(), target.as_ref(), eye.as_ref(), fov.as_ref(), quad.as_ref()) else {
            return false;
        };
        let blend = if blend == 1 { Blend::Light } else { Blend::Over };
        q.draw(
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            target,
            eye,
            fov,
            ID3D12Resource::from_raw_borrowed(&texture),
            view,
            quad,
            blend,
            opacity,
            black_point,
        )
    }
}
/// # Safety
/// As cvr_gpu_space_quads_fill.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_resample(
    quads: *mut SpaceQuads,
    device: *mut c_void,
    list: *mut c_void,
    target: *const Target,
    texture: *mut c_void,
    view: i32,
    mip: u32,
    uv: *const [f32; 4],
) -> bool {
    // SAFETY: as the caller vouches.
    unsafe {
        let (Some(q), Some(target), Some(uv)) = (quads.as_mut(), target.as_ref(), uv.as_ref()) else {
            return false;
        };
        q.resample(
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            target,
            ID3D12Resource::from_raw_borrowed(&texture),
            view,
            mip,
            uv,
        )
    }
}
/// # Safety
/// `quads` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_space_quads_shutdown(quads: *mut SpaceQuads) {
    // SAFETY: the caller's object.
    if let Some(q) = unsafe { quads.as_mut() } {
        q.shutdown();
    }
}
