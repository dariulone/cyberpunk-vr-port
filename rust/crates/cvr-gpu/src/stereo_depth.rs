//! The depth each eye is submitted to the XR runtime with (formerly src/Render/StereoDepth.cpp; the header keeps
//! `cvr::depth::Renderer`, which holds one of these and hands over its inputs).
//!
//! Framegen's copy of the engine's depth for the frame (R32_FLOAT, rasterized with the frame's jitter) is resampled
//! at the jitter's offset into a texture of the plan's size, so it describes the unjittered colour and projection sent
//! to XR, then copied into the eye's depth swapchain image (D32_FLOAT). Device depth is affine over a planar surface:
//! it is interpolated there, and never blended between foreground and background into an invented surface at an edge
//! (the nearest sample is taken where the four distances spread more than 5 %).
//!
//! THE INPUTS ARE HELD UNTIL THE SUBMIT FENCE, not just their producer fence: each of the three in-flight slots keeps
//! its inputs (the caller's handles, released through its callback) and the destinations until the fence passed its
//! value. Same-queue FIFO is the readiness dependency; no wait on another queue is introduced.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use windows::core::{s, w, Interface};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_ENABLE_STRICTNESS};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SHADER: &str = r#"
Texture2D<float> sourceDepth:register(t0);
RWTexture2D<float> outputDepth:register(u0);
cbuffer Parameters:register(b0) {uint2 size;float2 jitter;float nearPlane;float farPlane;uint inverted;uint unused;};
float clean(float d){return isfinite(d) && d>=0 && d<=1 ? d : (inverted?0:1);}
float distanceAt(float d){
    if(inverted)d=1-d;
    if(isinf(farPlane))return nearPlane/max(1-d,1e-20);
    return nearPlane*farPlane/max(farPlane-d*(farPlane-nearPlane),1e-20);
}
[numthreads(8,8,1)] void main(uint3 id:SV_DispatchThreadID) {
    if(any(id.xy>=size))return;
    // Native 0xC28EC0 adds +2*jitterX/W,+2*jitterY/H to NDC. CameraData
    // already flips Y for texture coordinates. Sample the rasterized depth
    // at that offset to describe the unjittered color/projection sent to XR.
    float2 p=float2(id.xy)+jitter;int2 b=int2(floor(p));float2 w=frac(p);
    float4 d=float4(clean(sourceDepth.Load(int3(clamp(b,int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(1,0),int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(0,1),int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(1,1),int2(0,0),int2(size)-1),0))));
    float4 z=float4(distanceAt(d.x),distanceAt(d.y),distanceAt(d.z),distanceAt(d.w));
    float lo=min(min(z.x,z.y),min(z.z,z.w)),hi=max(max(z.x,z.y),max(z.z,z.w));
    // Device depth is affine over a planar surface. Interpolate it there;
    // never blend foreground and background into an invented surface at an edge.
    float value=hi<=lo*1.05 ? lerp(lerp(d.x,d.y,w.x),lerp(d.z,d.w,w.x),w.y)
        : clean(sourceDepth.Load(int3(clamp(int2(floor(p+.5)),int2(0,0),int2(size)-1),0)));
    outputDepth[id.xy]=value;
}"#;

const COPY: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_SOURCE;
const UAV: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;

/// The caller's release of an input it handed over (a heap copy of its shared_ptr).
pub type Release = unsafe extern "C" fn(*mut c_void);

/// One eye of a record: the framegen input's fields the record reads, the destination, the plan's view.
#[repr(C)]
pub struct DepthEye {
    /// The caller's handle on the input (released through the callback once the slot is done, or at once when the
    /// record is refused); null when the eye has no input.
    pub keep: *mut c_void,
    /// The input's depth copy (R32_FLOAT).
    pub depth: *mut c_void,
    pub input_queue: *mut c_void,
    pub ready_fence: *mut c_void,
    pub fence_value: u64,
    pub submitted: bool,
    pub inverted: bool,
    pub near_plane: f32,
    pub far_plane: f32,
    /// The eye's depth swapchain image.
    pub destination: *mut c_void,
    pub width: u32,
    pub height: u32,
    pub jitter: [f32; 2],
    /// A midpoint's depth (main's 0.2.4): B's motion vectors (null for a real frame), their size, the share of them the
    /// depth moves (0 for a real frame) and their scale to a fraction of the image (CameraData::motionScale).
    pub motion: *mut c_void,
    pub motion_width: u32,
    pub motion_height: u32,
    pub motion_share: f32,
    pub motion_scale: [f32; 2],
}
const _: () = assert!(core::mem::size_of::<DepthEye>() == 112);

/// A shader-readable two-channel float view of the game's motion vector copy; UNKNOWN for a layout it cannot be.
fn motion_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R16G16_FLOAT | DXGI_FORMAT_R16G16_TYPELESS => DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R32G32_FLOAT | DXGI_FORMAT_R32G32_TYPELESS => DXGI_FORMAT_R32G32_FLOAT,
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R32G32B32A32_FLOAT | DXGI_FORMAT_R32G32B32A32_TYPELESS => DXGI_FORMAT_R32G32B32A32_FLOAT,
        _ => DXGI_FORMAT_UNKNOWN,
    }
}

#[derive(Default)]
struct Slot {
    heap: Option<ID3D12DescriptorHeap>,
    output: [Option<ID3D12Resource>; 2],
    destination: [Option<ID3D12Resource>; 2],
    keep: [*mut c_void; 2],
    fence: Option<ID3D12Fence>,
    value: u64,
}
impl Slot {
    fn idle(&self) -> bool {
        // SAFETY: a live fence.
        self.fence
            .as_ref()
            .is_none_or(|f| unsafe { f.GetCompletedValue() } >= self.value)
    }
}
#[derive(Default)]
struct Pipeline {
    device: Option<ID3D12Device>,
    root: Option<ID3D12RootSignature>,
    pso: Option<ID3D12PipelineState>,
    slots: [Slot; 3],
}
pub struct Renderer {
    p: Pipeline,
    release: Release,
}
impl Renderer {
    unsafe fn release_keeps(&mut self, slot: usize) {
        for k in self.p.slots[slot].keep.iter_mut() {
            if !k.is_null() {
                (self.release)(*k);
                *k = core::ptr::null_mut();
            }
        }
    }
    fn idle(&self) -> bool {
        self.p.slots.iter().all(Slot::idle)
    }
    unsafe fn reset(&mut self) {
        for s in 0..3 {
            self.release_keeps(s);
        }
        self.p = Pipeline::default();
    }
    unsafe fn init(&mut self, device: &ID3D12Device) -> bool {
        if self.p.pso.is_some() {
            return self
                .p
                .device
                .as_ref()
                .is_some_and(|d| d.as_raw() == device.as_raw());
        }
        self.p.device = Some(device.clone());
        let ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 2,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 0,
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                NumDescriptors: 1,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 2,
            },
        ];
        let params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 2,
                        pDescriptorRanges: ranges.as_ptr(),
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 12,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 2,
            pParameters: params.as_ptr(),
            ..Default::default()
        };
        let (mut blob, mut error, mut code): (
            Option<ID3DBlob>,
            Option<ID3DBlob>,
            Option<ID3DBlob>,
        ) = (None, None, None);
        if D3D12SerializeRootSignature(
            &desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut blob,
            Some(&mut error),
        )
        .is_err()
        {
            return false;
        }
        let Some(blob) = blob else { return false };
        let bytes =
            core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(0, bytes) else {
            return false;
        };
        // the shader made from Rust (rust/shaders/passes/src/stereo_depth.rs); the HLSL above with CVR_SHADER_HLSL
        let hlsl = || {
            D3DCompile(
                SHADER.as_ptr().cast(),
                SHADER.len(),
                None,
                None,
                None,
                s!("main"),
                s!("cs_5_0"),
                D3DCOMPILE_ENABLE_STRICTNESS,
                0,
                &mut code,
                Some(&mut error),
            )
            .ok()?;
            code.clone()
        };
        let Some(code) = crate::dxil::pick(crate::dxil::STEREO_DEPTH_CS, hlsl) else { return false };
        let state = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(&root),
            CS: code.bytecode(),
            ..Default::default()
        };
        let pso = device.CreateComputePipelineState::<ID3D12PipelineState>(&state);
        self.p.root = Some(root);
        match pso {
            Ok(pso) => {
                self.p.pso = Some(pso);
                true
            }
            Err(_) => false,
        }
    }
}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    a: D3D12_RESOURCE_STATES,
    b: D3D12_RESOURCE_STATES,
) {
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: core::mem::transmute_copy(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: a,
                StateAfter: b,
            }),
        },
    };
    list.ResourceBarrier(&[v]);
}

#[no_mangle]
pub extern "C" fn cvr_gpu_stereo_depth_new(release: Release) -> *mut c_void {
    Box::into_raw(Box::new(Renderer {
        p: Pipeline::default(),
        release,
    }))
    .cast()
}
/// # Safety
/// `r` from cvr_gpu_stereo_depth_new, or null; the GPU done with its slots.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stereo_depth_free(r: *mut c_void) {
    if !r.is_null() {
        let mut r = Box::from_raw(r as *mut Renderer);
        r.reset();
    }
}
/// Every slot's submit fence passed.
///
/// # Safety
/// `r` from cvr_gpu_stereo_depth_new.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stereo_depth_idle(r: *const c_void) -> bool {
    (*(r as *const Renderer)).idle()
}
/// The finished slots' inputs and destinations let go.
///
/// # Safety
/// `r` from cvr_gpu_stereo_depth_new.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stereo_depth_poll(r: *mut c_void) {
    let r = &mut *(r as *mut Renderer);
    for s in 0..3 {
        if r.p.slots[s].idle() {
            r.release_keeps(s);
            r.p.slots[s].destination = [None, None];
        }
    }
}

/// Both eyes' unjittered depth recorded on `list` into their destinations, slot `index` held until `fence` reaches
/// `value`. False (and nothing recorded, the eyes' handles released) when an input, a destination or the plan does not
/// fit, or the slot is still in flight.
///
/// # Safety
/// `r` from cvr_gpu_stereo_depth_new; live D3D12 objects or null; `eyes` two eyes.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stereo_depth_record(
    r: *mut c_void,
    device: *mut c_void,
    list: *mut c_void,
    queue: *mut c_void,
    eyes: *const DepthEye,
    index: u32,
    fence: *mut c_void,
    value: u64,
) -> bool {
    let r = &mut *(r as *mut Renderer);
    let eyes = core::slice::from_raw_parts(eyes, 2);
    let refuse = |r: &mut Renderer| {
        for e in eyes {
            if !e.keep.is_null() {
                (r.release)(e.keep);
            }
        }
        false
    };
    let (Some(device), Some(list), Some(fence)) = (
        ID3D12Device::from_raw_borrowed(&device),
        ID3D12GraphicsCommandList::from_raw_borrowed(&list),
        ID3D12Fence::from_raw_borrowed(&fence),
    ) else {
        return refuse(r);
    };
    if queue.is_null() || value == 0 || index >= 3 {
        return refuse(r);
    }
    if r.p
        .device
        .as_ref()
        .is_some_and(|d| d.as_raw() != device.as_raw())
    {
        if !r.idle() {
            return refuse(r);
        }
        r.reset();
    }
    let slot = index as usize;
    if !r.p.slots[slot].idle() || !r.init(device) {
        return refuse(r);
    }
    // Same-queue FIFO is the readiness dependency; from another queue only once finished there (the runtime's own
    // queue reading the game's captures, main's 0.2.4).
    let mut inputs: [Option<&ID3D12Resource>; 2] = [None, None];
    let mut destinations: [Option<&ID3D12Resource>; 2] = [None, None];
    for (e, eye) in eyes.iter().enumerate() {
        if eye.keep.is_null()
            || !eye.submitted
            || eye.ready_fence.is_null()
            || eye.fence_value == 0
        {
            return refuse(r);
        }
        if eye.input_queue != queue {
            let finished = ID3D12Fence::from_raw_borrowed(&eye.ready_fence)
                .is_some_and(|f| f.GetCompletedValue() >= eye.fence_value);
            if !finished {
                return refuse(r);
            }
        }
        if eye.motion_share > 0.0 {
            let usable = ID3D12Resource::from_raw_borrowed(&eye.motion)
                .is_some_and(|m| motion_format(m.GetDesc().Format) != DXGI_FORMAT_UNKNOWN);
            if !usable || eye.motion_width == 0 || eye.motion_height == 0 {
                return refuse(r);
            }
        }
        let (Some(depth), Some(destination)) = (
            ID3D12Resource::from_raw_borrowed(&eye.depth),
            ID3D12Resource::from_raw_borrowed(&eye.destination),
        ) else {
            return refuse(r);
        };
        let (a, b) = (depth.GetDesc(), destination.GetDesc());
        if eye.width == 0
            || eye.height == 0
            || a.Width != eye.width as u64
            || a.Height != eye.height
            || b.Width != eye.width as u64
            || b.Height != eye.height
            || a.Format != DXGI_FORMAT_R32_FLOAT
            || (b.Format != DXGI_FORMAT_D32_FLOAT && b.Format != DXGI_FORMAT_R32_TYPELESS)
            || a.DepthOrArraySize != 1
            || b.DepthOrArraySize != 1
            || a.SampleDesc.Count != 1
            || b.SampleDesc.Count != 1
        {
            return refuse(r);
        }
        inputs[e] = Some(depth);
        destinations[e] = Some(destination);
    }
    let (Some(root), Some(pso)) = (r.p.root.clone(), r.p.pso.clone()) else {
        return refuse(r);
    };
    let s = &mut r.p.slots[slot];
    if s.heap.is_none() {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 6,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        match device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) {
            Ok(h) => s.heap = Some(h),
            Err(_) => return refuse(r),
        }
    }
    let heap = r.p.slots[slot].heap.clone().expect("made above");
    let step =
        device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
    for (e, eye) in eyes.iter().enumerate() {
        let remake = r.p.slots[slot].output[e].as_ref().is_none_or(|o| {
            let d = o.GetDesc();
            d.Width != eye.width as u64 || d.Height != eye.height
        });
        if remake {
            let d = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: eye.width as u64,
                Height: eye.height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_R32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                ..Default::default()
            };
            let heap_props = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            let mut created: Option<ID3D12Resource> = None;
            if device
                .CreateCommittedResource(
                    &heap_props,
                    D3D12_HEAP_FLAG_NONE,
                    &d,
                    COPY,
                    None,
                    &mut created,
                )
                .is_err()
                || created.is_none()
            {
                return refuse(r);
            }
            let created = created.expect("checked");
            let _ = created.SetName(w!("CVR_OpenXR_UnjitteredDepth"));
            r.p.slots[slot].output[e] = Some(created);
        }
        let s = &mut r.p.slots[slot];
        let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
        cpu.ptr += 3 * e * step;
        let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        device.CreateShaderResourceView(inputs[e].expect("checked"), Some(&srv), cpu);
        cpu.ptr += step;
        // B's motion vectors for a midpoint; a null view otherwise (the shader does not read it then)
        let warp = eye.motion_share > 0.0;
        let motion = if warp { ID3D12Resource::from_raw_borrowed(&eye.motion) } else { None };
        let motion_srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: motion.map_or(DXGI_FORMAT_R16G16_FLOAT, |m| motion_format(m.GetDesc().Format)),
            ..srv
        };
        device.CreateShaderResourceView(motion, Some(&motion_srv), cpu);
        cpu.ptr += step;
        let uav = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        device.CreateUnorderedAccessView(s.output[e].as_ref(), None, Some(&uav), cpu);
    }
    let s = &mut r.p.slots[slot];
    for (e, eye) in eyes.iter().enumerate() {
        if !s.keep[e].is_null() {
            (r.release)(s.keep[e]);
        }
        s.keep[e] = eye.keep;
        s.destination[e] = destinations[e].cloned();
    }
    s.fence = Some(fence.clone());
    s.value = value;
    list.SetDescriptorHeaps(&[Some(heap.clone())]);
    list.SetComputeRootSignature(&root);
    list.SetPipelineState(&pso);
    for (e, eye) in eyes.iter().enumerate() {
        #[repr(C)]
        struct Constants {
            w: u32,
            h: u32,
            jitter: [f32; 2],
            near_plane: f32,
            far_plane: f32,
            inverted: u32,
            warp: u32,
            motion_to_pixels: [f32; 2],
            motion_w: u32,
            motion_h: u32,
        }
        const _: () = assert!(core::mem::size_of::<Constants>() == 12 * 4);
        // The vectors' scale makes them a fraction of the image; the depth texture's size makes that pixels.
        let warp = eye.motion_share > 0.0;
        let constants = Constants {
            w: eye.width,
            h: eye.height,
            jitter: eye.jitter,
            near_plane: eye.near_plane,
            far_plane: eye.far_plane,
            inverted: eye.inverted as u32,
            warp: warp as u32,
            motion_to_pixels: if warp {
                [
                    eye.motion_share * eye.motion_scale[0] * eye.width as f32,
                    eye.motion_share * eye.motion_scale[1] * eye.height as f32,
                ]
            } else {
                [0.0; 2]
            },
            motion_w: if warp { eye.motion_width } else { 1 },
            motion_h: if warp { eye.motion_height } else { 1 },
        };
        let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
        gpu.ptr += (3 * e * step) as u64;
        let output = r.p.slots[slot].output[e].clone().expect("made above");
        let destination = destinations[e].expect("checked");
        barrier(list, &output, COPY, UAV);
        list.SetComputeRootDescriptorTable(0, gpu);
        list.SetComputeRoot32BitConstants(1, 12, (&constants as *const Constants).cast(), 0);
        list.Dispatch(eye.width.div_ceil(8), eye.height.div_ceil(8), 1);
        barrier(list, &output, UAV, COPY);
        barrier(
            list,
            destination,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(&output),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(destination),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        barrier(
            list,
            destination,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4};
    unsafe extern "C" fn nothing(_: *mut c_void) {}
    #[test]
    fn the_pipeline_builds_on_warp() {
        unsafe {
            let factory: IDXGIFactory4 = CreateDXGIFactory1().unwrap();
            let adapter: IDXGIAdapter = factory.EnumWarpAdapter().unwrap();
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).unwrap();
            let device = device.unwrap();
            let mut r = Renderer {
                p: Pipeline::default(),
                release: nothing,
            };
            assert!(r.init(&device), "init failed");
        }
    }

    /// A midpoint's depth on WARP: B's depth (a gradient along x) moved half of a constant motion vector of a quarter
    /// image (8 of 64 pixels) toward A; a real frame's unchanged.
    #[test]
    fn a_midpoint_depth_moves_along_the_motion_vectors_on_warp() {
        const W: u32 = 64;
        const H: u32 = 32;
        unsafe {
            let factory: IDXGIFactory4 = CreateDXGIFactory1().unwrap();
            let adapter: IDXGIAdapter = factory.EnumWarpAdapter().unwrap();
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).unwrap();
            let device = device.unwrap();
            let queue: ID3D12CommandQueue = device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })
                .unwrap();
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).unwrap();
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None).unwrap();
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
            let texture = |format: DXGI_FORMAT, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES| {
                let d = D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                    Width: W as u64,
                    Height: H,
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Flags: flags,
                    ..Default::default()
                };
                let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
                let mut r: Option<ID3D12Resource> = None;
                device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, state, None, &mut r).unwrap();
                r.unwrap()
            };
            let buffer = |kind: D3D12_HEAP_TYPE, bytes: u64, state: D3D12_RESOURCE_STATES| {
                let d = D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                    Width: bytes,
                    Height: 1,
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                    ..Default::default()
                };
                let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
                let mut r: Option<ID3D12Resource> = None;
                device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, state, None, &mut r).unwrap();
                r.unwrap()
            };
            let depth = texture(DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
            let motion = texture(DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
            let targets = [
                texture(DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_STATE_DEPTH_WRITE),
                texture(DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_STATE_DEPTH_WRITE),
            ];
            // rows of 64 four-byte texels: 256 bytes, the copy pitch
            let pitch = W as usize * 4;
            let plane = pitch * H as usize;
            let upload = buffer(D3D12_HEAP_TYPE_UPLOAD, (plane * 2) as u64, D3D12_RESOURCE_STATE_GENERIC_READ);
            let mut mapped: *mut c_void = core::ptr::null_mut();
            upload.Map(0, None, Some(&mut mapped)).unwrap();
            let at = |x: u32| 0.25 + x as f32 * 0.005;
            let bytes = core::slice::from_raw_parts_mut(mapped.cast::<u8>(), plane * 2);
            for y in 0..H as usize {
                for x in 0..W as usize {
                    let o = y * pitch + x * 4;
                    bytes[o..o + 4].copy_from_slice(&at(x as u32).to_le_bytes());
                    // R16G16_FLOAT (0.25, 0): a quarter of the image to the right
                    bytes[plane + o..plane + o + 2].copy_from_slice(&0x3400u16.to_le_bytes());
                    bytes[plane + o + 2..plane + o + 4].copy_from_slice(&0u16.to_le_bytes());
                }
            }
            upload.Unmap(0, None);
            let footprint = |offset: u64, format: DXGI_FORMAT| D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: offset,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT { Format: format, Width: W, Height: H, Depth: 1, RowPitch: pitch as u32 },
            };
            let copy = |to: &ID3D12Resource, from: &ID3D12Resource, offset: u64, format: DXGI_FORMAT, into_buffer: bool| {
                let texture_location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
                    pResource: core::mem::transmute_copy(r),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
                };
                let buffer_location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
                    pResource: core::mem::transmute_copy(r),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint(offset, format) },
                };
                let (dst, src) = if into_buffer {
                    (buffer_location(to), texture_location(from))
                } else {
                    (texture_location(to), buffer_location(from))
                };
                list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            };
            copy(&depth, &upload, 0, DXGI_FORMAT_R32_FLOAT, false);
            copy(&motion, &upload, plane as u64, DXGI_FORMAT_R16G16_FLOAT, false);
            barrier(&list, &depth, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            barrier(&list, &motion, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);

            let r = cvr_gpu_stereo_depth_new(nothing);
            let eye = |target: &ID3D12Resource, share: f32| DepthEye {
                keep: 1 as *mut c_void,
                depth: depth.as_raw(),
                input_queue: queue.as_raw(),
                ready_fence: fence.as_raw(),
                fence_value: 1,
                submitted: true,
                inverted: false,
                near_plane: 0.1,
                far_plane: 100.0,
                destination: target.as_raw(),
                width: W,
                height: H,
                jitter: [0.0; 2],
                motion: motion.as_raw(),
                motion_width: W,
                motion_height: H,
                motion_share: share,
                motion_scale: [1.0, 1.0],
            };
            let eyes = [eye(&targets[0], 0.5), eye(&targets[1], 0.0)];
            assert!(cvr_gpu_stereo_depth_record(
                r,
                device.as_raw(),
                list.as_raw(),
                queue.as_raw(),
                eyes.as_ptr(),
                0,
                fence.as_raw(),
                2
            ));
            let readback = buffer(D3D12_HEAP_TYPE_READBACK, (plane * 2) as u64, D3D12_RESOURCE_STATE_COPY_DEST);
            for (i, t) in targets.iter().enumerate() {
                barrier(&list, t, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                copy(&readback, t, (plane * i) as u64, DXGI_FORMAT_D32_FLOAT, true);
            }
            list.Close().unwrap();
            queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap())]);
            queue.Signal(&fence, 2).unwrap();
            while fence.GetCompletedValue() < 2 {
                std::thread::yield_now();
            }
            readback.Map(0, None, Some(&mut mapped)).unwrap();
            let out = core::slice::from_raw_parts(mapped.cast::<f32>(), plane / 2);
            for y in [0usize, 15, 31] {
                for x in [0u32, 7, 8, 20, 63] {
                    let warped = out[y * W as usize + x as usize];
                    let real = out[plane / 4 + y * W as usize + x as usize];
                    assert!((warped - at(x.saturating_sub(8))).abs() < 1e-6, "midpoint x={x} y={y}: {warped}");
                    assert!((real - at(x)).abs() < 1e-6, "real x={x} y={y}: {real}");
                }
            }
            readback.Unmap(0, None);
            cvr_gpu_stereo_depth_free(r);
        }
    }
}
