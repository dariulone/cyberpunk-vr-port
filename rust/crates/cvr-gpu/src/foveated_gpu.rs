//! Foveated DLSS's own GPU passes (formerly src/Render/FoveatedGpu.cpp): Prepare crops the full-resolution centre (or
//! scales the whole frame for the periphery) out of the engine's colour, depth and motion into private inputs -- depth
//! and motion chosen together at discontinuities when downsampling, motion rescaled to the region's pixels -- and
//! Composite blends the centre's upscaled output into the periphery's over a soft edge. The shader stays HLSL,
//! compiled at first use as the C++ did.
use core::ffi::c_void;
use std::sync::Mutex;

use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const D3DCOMPILE_OPTIMIZATION_LEVEL3: u32 = 1 << 15;
const SHADER: &str = r#"
Texture2D<float4> Color : register(t0);
Texture2D<float> Depth : register(t1);
Texture2D<float2> Motion : register(t2);
RWTexture2D<float4> ColorOut : register(u0);
RWTexture2D<float> DepthOut : register(u1);
RWTexture2D<float2> MotionOut : register(u2);
SamplerState LinearClamp : register(s0);
cbuffer Constants : register(b0) {
    float4 sourceRect;
    uint4 dimensions; // prepared input width/height, original render width/height
    uint4 bases; // original color and depth origins
    uint2 motionBase;
    float2 pixelScale;
    uint4 flags; // low-res motion, inverted depth, original output width/height
    uint4 region; // output x/y/width/height
}
[numthreads(8,8,1)] void Prepare(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= dimensions.xy)) return;
    const float2 uv = (id.xy + 0.5) / dimensions.xy;
    const float2 pixel = sourceRect.xy + uv * sourceRect.zw;
    uint cw,ch; Color.GetDimensions(cw,ch);
    ColorOut[id.xy] = Color.SampleLevel(LinearClamp,(bases.xy+pixel)/float2(cw,ch),0);
    int2 p = clamp(int2(floor(pixel)),int2(0,0),int2(dimensions.zw)-1);
    float depth = Depth.Load(int3(int2(bases.zw)+p,0));
    // Downsampling selects a surface and its motion together at discontinuities.
    if (any(sourceRect.zw / dimensions.xy > 1.0)) {
        [unroll] for(int y=0;y<2;++y) [unroll] for(int x=0;x<2;++x) {
            int2 q=clamp(int2(floor(pixel-0.5))+int2(x,y),int2(0,0),int2(dimensions.zw)-1);
            float d=Depth.Load(int3(int2(bases.zw)+q,0));
            if(flags.y ? d>depth : d<depth) {depth=d;p=q;}
        }
    }
    DepthOut[id.xy] = depth;
    float2 mpos = (p+0.5) / dimensions.zw * (flags.x ? dimensions.zw : flags.zw);
    float2 motion = Motion.Load(int3(motionBase+uint2(mpos),0));
    MotionOut[id.xy] = motion * pixelScale;
}
Texture2D<float4> Center : register(t3);
Texture2D<float4> Peripheral : register(t4);
RWTexture2D<float4> Output : register(u3);
[numthreads(8,8,1)] void Composite(uint3 id : SV_DispatchThreadID) {
    if(any(id.xy >= dimensions.zw)) return;
    float2 uv = (id.xy+0.5)/dimensions.zw;
    float4 peripheral = Peripheral.SampleLevel(LinearClamp,uv,0);
    float2 local = float2(id.xy)+0.5-region.xy;
    float2 edge = min(local,float2(region.zw)-local);
    float blend = saturate(min(edge.x,edge.y)/max(1.0,0.04*min(region.z,region.w)));
    float4 value = peripheral;
    if(blend>0) {
        float2 sourcePosition=(id.xy+0.5)*dimensions.xy/dimensions.zw;
        float2 centerUV=(sourcePosition-sourceRect.xy)/sourceRect.zw;
        value=lerp(peripheral,Center.SampleLevel(LinearClamp,centerUV,0),blend);
    }
    if(flags.x && all(edge>=0) && min(edge.x,edge.y)<3) value=float4(1,0,0,1);
    Output[id.xy]=value;
}
"#;

/// cvr::foveated::Region (Render/FoveatedDlss.hpp).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub input_w: u32,
    pub input_h: u32,
    pub source_x: u32,
    pub source_y: u32,
}
/// cvr::foveated::Source (Render/FoveatedGpu.hpp).
#[repr(C)]
pub struct Source {
    pub color: *mut c_void,
    pub depth: *mut c_void,
    pub motion: *mut c_void,
    pub output: *mut c_void,
    pub render_w: u32,
    pub render_h: u32,
    pub output_w: u32,
    pub output_h: u32,
    pub color_x: u32,
    pub color_y: u32,
    pub depth_x: u32,
    pub depth_y: u32,
    pub motion_x: u32,
    pub motion_y: u32,
    pub motion_scale_x: f32,
    pub motion_scale_y: f32,
    pub low_motion: bool,
    pub inverted: bool,
}
const _: () = assert!(core::mem::size_of::<Source>() == 88 && core::mem::size_of::<Region>() == 32);

#[repr(C)]
#[derive(Default)]
struct Constants {
    rect: [f32; 4],
    dimensions: [u32; 4],
    bases: [u32; 4],
    motion_base: [u32; 2],
    pixel_scale: [f32; 2],
    flags: [u32; 4],
    region: [u32; 4],
}
const _: () = assert!(core::mem::size_of::<Constants>() == 96);

fn view_format(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_D32_FLOAT => DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R24G8_TYPELESS | DXGI_FORMAT_D24_UNORM_S8_UINT => DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
        DXGI_FORMAT_R16G16_TYPELESS => DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R8G8B8A8_TYPELESS => DXGI_FORMAT_R8G8B8A8_UNORM,
        other => other,
    }
}
/// A transition of every subresource, borrowing the resource.
fn barrier(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
    let b = D3D12_RESOURCE_BARRIER {
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
    };
    // SAFETY: the recording list.
    unsafe { list.ResourceBarrier(&[b]) };
}
fn texture(device: &ID3D12Device, w: u32, h: u32, format: DXGI_FORMAT) -> Option<ID3D12Resource> {
    let d = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: w as u64,
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    // SAFETY: the live device; descriptions on the stack.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, None, &mut r) }.ok()?;
    r
}
/// The HLSL the Rust shaders replaced, compiled by FXC (CVR_SHADER_HLSL).
fn hlsl(entry: &core::ffi::CStr) -> Option<ID3DBlob> {
    let (mut code, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: the source's bytes and length, NUL-terminated entry and target, out-blobs.
    let hr = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(c"cs_5_0".as_ptr() as *const u8),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut error),
        )
    };
    if hr.is_err() {
        let text = match &error {
            // SAFETY: the blob's own NUL-terminated message.
            Some(e) => unsafe { core::ffi::CStr::from_ptr(e.GetBufferPointer() as *const core::ffi::c_char) }
                .to_string_lossy()
                .into_owned(),
            None => "compile failed".to_string(),
        };
        log!("[foveated] {}: {}\n", entry.to_string_lossy(), text);
        return None;
    }
    code
}
/// The pass's pipeline for `entry` (Prepare or Composite): the shaders made from Rust
/// (rust/shaders/passes/src/foveated.rs); the HLSL above with CVR_SHADER_HLSL.
fn compile(device: &ID3D12Device, root: &ID3D12RootSignature, entry: &core::ffi::CStr) -> Option<ID3D12PipelineState> {
    let dxil = if entry == c"Prepare" { crate::dxil::FOVEATED_PREPARE_CS } else { crate::dxil::FOVEATED_COMPOSITE_CS };
    let code = crate::dxil::pick(dxil, || hlsl(entry))?;
    let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        // SAFETY: the same pointer without an AddRef; the field is ManuallyDrop.
        pRootSignature: unsafe { core::mem::transmute_copy(root) },
        CS: code.bytecode(),
        ..Default::default()
    };
    // SAFETY: the live device and a description whose pointers outlive the call.
    unsafe { device.CreateComputePipelineState(&desc) }.ok()
}

/// PreparedRegion: one pass's region and its private inputs and output.
#[derive(Default)]
pub struct Prepared {
    pub region: Region,
    pub color: Option<ID3D12Resource>,
    pub depth: Option<ID3D12Resource>,
    pub motion: Option<ID3D12Resource>,
    pub output: Option<ID3D12Resource>,
}
struct Binding {
    sources: [Option<ID3D12Resource>; 4],
    heap: ID3D12DescriptorHeap,
}
#[derive(Default)]
struct State {
    device: Option<ID3D12Device>,
    root: Option<ID3D12RootSignature>,
    prepare: Option<ID3D12PipelineState>,
    composite: Option<ID3D12PipelineState>,
    bindings: Vec<Binding>,
    stride: u32,
    center: Prepared,
    periphery: Prepared,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for State {}

/// FoveatedGpu.
#[derive(Default)]
pub struct Gpu {
    state: Mutex<State>,
}

/// Borrowed resources of a Source (None for a null one).
fn borrowed(p: &*mut c_void) -> Option<&ID3D12Resource> {
    // SAFETY: the caller's ID3D12Resource or null.
    unsafe { ID3D12Resource::from_raw_borrowed(p) }
}
fn raw(r: &Option<ID3D12Resource>) -> *mut c_void {
    r.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw)
}

impl State {
    fn bind(&mut self, s: &Source) -> Option<usize> {
        let wanted = [s.color, s.depth, s.motion, s.output];
        if let Some(i) = self
            .bindings
            .iter()
            .position(|b| b.sources.iter().zip(wanted).all(|(have, want)| raw(have) == want))
        {
            return Some(i);
        }
        let device = self.device.clone()?;
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 13,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        // SAFETY (whole block): the live device; the sources are the caller's live resources.
        unsafe {
            let heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&desc).ok()?;
            let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
            let srvs = [s.color, s.depth, s.motion, raw(&self.center.output), raw(&self.periphery.output)];
            for t in srvs {
                let texture = borrowed(&t)?;
                let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: view_format(texture.GetDesc().Format),
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_SRV {
                            MipLevels: 1,
                            ..Default::default()
                        },
                    },
                };
                device.CreateShaderResourceView(texture, Some(&srv), cpu);
                cpu.ptr += self.stride as usize;
            }
            for pass in [&self.center, &self.periphery] {
                for t in [raw(&pass.color), raw(&pass.depth), raw(&pass.motion), s.output] {
                    let texture = borrowed(&t)?;
                    let uav = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                        Format: view_format(texture.GetDesc().Format),
                        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                        ..Default::default()
                    };
                    device.CreateUnorderedAccessView(texture, None, Some(&uav), cpu);
                    cpu.ptr += self.stride as usize;
                }
            }
            if self.bindings.len() >= 8 {
                self.bindings.remove(0);
            }
            self.bindings.push(Binding {
                sources: wanted.map(|p| borrowed(&p).cloned()),
                heap,
            });
        }
        Some(self.bindings.len() - 1)
    }
}

impl Gpu {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Initialize: the passes for `center` and `periphery` on `device` (the textures made so far stay on failure).
    pub fn initialize(&self, device: &ID3D12Device, center: Region, periphery: Region) -> bool {
        let mut s = self.lock();
        s.device = Some(device.clone());
        s.center.region = center;
        s.periphery.region = periphery;
        // SAFETY: the live device.
        s.stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        for central in [true, false] {
            let pass = if central { &mut s.center } else { &mut s.periphery };
            let r = pass.region;
            pass.color = texture(device, r.input_w, r.input_h, DXGI_FORMAT_R16G16B16A16_FLOAT);
            if pass.color.is_none() {
                return false;
            }
            pass.depth = texture(device, r.input_w, r.input_h, DXGI_FORMAT_R32_FLOAT);
            if pass.depth.is_none() {
                return false;
            }
            pass.motion = texture(device, r.input_w, r.input_h, DXGI_FORMAT_R32G32_FLOAT);
            if pass.motion.is_none() {
                return false;
            }
            pass.output = texture(device, r.width, r.height, DXGI_FORMAT_R16G16B16A16_FLOAT);
            if pass.output.is_none() {
                return false;
            }
        }
        let ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 5,
                ..Default::default()
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                NumDescriptors: 4,
                ..Default::default()
            },
        ];
        let table = |range: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let params = [
            table(&ranges[0]),
            table(&ranges[1]),
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: (core::mem::size_of::<Constants>() / 4) as u32,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxLOD: D3D12_FLOAT32_MAX,
            ..Default::default()
        };
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 3,
            pParameters: params.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        };
        let (mut blob, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        // SAFETY (whole block): the description's arrays outlive the calls; the live device.
        let root: Option<ID3D12RootSignature> = unsafe {
            if D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)).is_err() {
                return false;
            }
            let Some(blob) = blob else { return false };
            device
                .CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()))
                .ok()
        };
        let Some(root) = root else { return false };
        s.root = Some(root.clone());
        s.prepare = compile(device, &root, c"Prepare");
        if s.prepare.is_none() {
            return false;
        }
        s.composite = compile(device, &root, c"Composite");
        s.composite.is_some()
    }

    /// Prepare: the centre's (or periphery's) private colour, depth and motion from the engine's, left readable.
    pub fn prepare(&self, list: &ID3D12GraphicsCommandList, src: &Source, central: bool) -> bool {
        let mut s = self.lock();
        let Some(index) = s.bind(src) else { return false };
        let (Some(root), Some(prepare)) = (s.root.clone(), s.prepare.clone()) else {
            return false;
        };
        let heap = s.bindings[index].heap.clone();
        let pass = if central { &s.center } else { &s.periphery };
        let r = pass.region;
        let keep = [
            heap.as_raw(),
            src.color,
            src.depth,
            src.motion,
            src.output,
            root.as_raw(),
            prepare.as_raw(),
            raw(&pass.color),
            raw(&pass.depth),
            raw(&pass.motion),
            raw(&pass.output),
        ];
        if !crate::command_resources::keep(list, &keep) {
            return false;
        }
        // Periphery's source is the whole frame even when its private output is smaller.
        let rect = if central {
            [r.source_x as f32, r.source_y as f32, r.input_w as f32, r.input_h as f32]
        } else {
            [0.0, 0.0, src.render_w as f32, src.render_h as f32]
        };
        let motion_w = if src.low_motion { rect[2] } else if central { r.width as f32 } else { src.output_w as f32 };
        let motion_h = if src.low_motion { rect[3] } else if central { r.height as f32 } else { src.output_h as f32 };
        let data = Constants {
            rect,
            dimensions: [r.input_w, r.input_h, src.render_w, src.render_h],
            bases: [src.color_x, src.color_y, src.depth_x, src.depth_y],
            motion_base: [src.motion_x, src.motion_y],
            pixel_scale: [src.motion_scale_x * r.input_w as f32 / motion_w, src.motion_scale_y * r.input_h as f32 / motion_h],
            flags: [src.low_motion as u32, src.inverted as u32, src.output_w, src.output_h],
            region: [0; 4],
        };
        // SAFETY (whole block): the recording list and live objects.
        unsafe {
            let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
            list.SetDescriptorHeaps(&[Some(heap.clone())]);
            list.SetComputeRootSignature(&root);
            list.SetPipelineState(&prepare);
            list.SetComputeRootDescriptorTable(0, gpu);
            gpu.ptr += (if central { 5 } else { 9 }) as u64 * s.stride as u64;
            list.SetComputeRootDescriptorTable(1, gpu);
            list.SetComputeRoot32BitConstants(2, (core::mem::size_of::<Constants>() / 4) as u32, &data as *const Constants as *const c_void, 0);
            list.Dispatch(r.input_w.div_ceil(8), r.input_h.div_ceil(8), 1);
        }
        let pass = if central { &s.center } else { &s.periphery };
        for t in [&pass.color, &pass.depth, &pass.motion].into_iter().flatten() {
            barrier(list, t, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        }
        true
    }

    /// Composite: the centre's output over the periphery's into the source's output (a red edge when `boundary`).
    pub fn composite(&self, list: &ID3D12GraphicsCommandList, src: &Source, boundary: bool) -> bool {
        let mut s = self.lock();
        let Some(index) = s.bind(src) else { return false };
        let (Some(root), Some(composite)) = (s.root.clone(), s.composite.clone()) else {
            return false;
        };
        let heap = s.bindings[index].heap.clone();
        let keep = [
            heap.as_raw(),
            src.output,
            root.as_raw(),
            composite.as_raw(),
            raw(&s.center.output),
            raw(&s.periphery.output),
        ];
        if !crate::command_resources::keep(list, &keep) {
            return false;
        }
        for t in [&s.center.output, &s.periphery.output].into_iter().flatten() {
            barrier(list, t, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        }
        let c = s.center.region;
        let data = Constants {
            dimensions: [src.render_w, src.render_h, src.output_w, src.output_h],
            flags: [boundary as u32, 0, 0, 0],
            rect: [c.source_x as f32, c.source_y as f32, c.input_w as f32, c.input_h as f32],
            region: [c.x, c.y, c.width, c.height],
            ..Default::default()
        };
        // SAFETY (whole block): the recording list and live objects.
        unsafe {
            list.SetDescriptorHeaps(&[Some(heap.clone())]);
            list.SetComputeRootSignature(&root);
            list.SetPipelineState(&composite);
            let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
            list.SetComputeRootDescriptorTable(0, gpu);
            gpu.ptr += 5 * s.stride as u64;
            list.SetComputeRootDescriptorTable(1, gpu);
            list.SetComputeRoot32BitConstants(2, (core::mem::size_of::<Constants>() / 4) as u32, &data as *const Constants as *const c_void, 0);
            list.Dispatch(src.output_w.div_ceil(8), src.output_h.div_ceil(8), 1);
            let uav = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                    UAV: core::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                        pResource: core::mem::transmute_copy(&src.output),
                    }),
                },
            };
            list.ResourceBarrier(&[uav]);
        }
        for pass in [&s.center, &s.periphery] {
            if let Some(o) = &pass.output {
                barrier(list, o, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
            }
            for t in [&pass.color, &pass.depth, &pass.motion].into_iter().flatten() {
                barrier(list, t, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
            }
        }
        true
    }

    /// The pass's region and resources (each one more reference, or null).
    pub fn prepared(&self, central: bool) -> (Region, [*mut c_void; 4]) {
        let s = self.lock();
        let pass = if central { &s.center } else { &s.periphery };
        let take = |r: &Option<ID3D12Resource>| r.clone().map_or(core::ptr::null_mut(), Interface::into_raw);
        (pass.region, [take(&pass.color), take(&pass.depth), take(&pass.motion), take(&pass.output)])
    }
}

// ---- the C ABI (include/Render/FoveatedGpu.hpp)

#[no_mangle]
pub extern "C" fn cvr_foveated_gpu_new() -> *mut Gpu {
    Box::into_raw(Box::default())
}
/// # Safety
/// `gpu` came from cvr_foveated_gpu_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_gpu_free(gpu: *mut Gpu) {
    if !gpu.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(gpu) });
    }
}
/// # Safety
/// `gpu` is live; `device` an ID3D12Device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_gpu_initialize(gpu: *const Gpu, device: *mut c_void, center: Region, periphery: Region) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (gpu.as_ref(), ID3D12Device::from_raw_borrowed(&device)) {
            (Some(g), Some(d)) => g.initialize(d, center, periphery),
            _ => false,
        }
    }
}
/// # Safety
/// `gpu` is live; `list` a recording ID3D12GraphicsCommandList or null; `source` a Source or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_gpu_prepare(gpu: *const Gpu, list: *mut c_void, source: *const Source, central: bool) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (gpu.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list), source.as_ref()) {
            (Some(g), Some(l), Some(s)) => g.prepare(l, s, central),
            _ => false,
        }
    }
}
/// # Safety
/// As cvr_foveated_gpu_prepare.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_gpu_composite(gpu: *const Gpu, list: *mut c_void, source: *const Source, boundary: bool) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (gpu.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list), source.as_ref()) {
            (Some(g), Some(l), Some(s)) => g.composite(l, s, boundary),
            _ => false,
        }
    }
}
/// The pass's region into `region` and its colour, depth, motion and output (one reference each, or null) into
/// `resources`.
///
/// # Safety
/// `gpu` is live; `region` one Region and `resources` four pointers, writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_gpu_prepared(gpu: *const Gpu, central: bool, region: *mut Region, resources: *mut *mut c_void) {
    // SAFETY: the caller's objects.
    unsafe {
        let Some(g) = gpu.as_ref() else { return };
        let (r, list) = g.prepared(central);
        if let Some(out) = region.as_mut() {
            *out = r;
        }
        if !resources.is_null() {
            for (i, p) in list.into_iter().enumerate() {
                *resources.add(i) = p;
            }
        } else {
            for p in list {
                if !p.is_null() {
                    drop(ID3D12Resource::from_raw(p));
                }
            }
        }
    }
}
