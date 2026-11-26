//! DLSS 5 NEURAL RENDERING on each eye after the game's DLSS (formerly main's src/Render/NeuralRender.cpp, 2e123a4a3 and
//! 5fa0c8b57, adapted there from iPowerTech/cyberpunk-vr-port) -- experimental, off at every start.
//!
//! WHERE IT RIDES. A MinHook on the core NGX dispatcher, _nvngx.dll!NVSDK_NGX_D3D12_EvaluateFeature: Streamline's DLSS
//! reaches every evaluate through it. When the game's evaluate returns, its Output, MotionVectors and Depth are filled
//! and the command list is still recording, so NR and its colour codec are appended to the same list. The hooks go in
//! only once the switch is on (on_present); before that nothing here runs.
//!
//! PER EYE. Under engine stereo DLSS runs once per eye (engine_stereo_dlss.rs): the left eye writes its half of the
//! side-by-side output, the right eye a target of its own copied into its half afterwards. evaluating_areas says which
//! eye it is and where its output, depth and motion are in the resources NGX was given, so NR reads and writes that
//! eye's area only, with one NR feature (one history) per DLSS handle and size. An evaluate under engine stereo outside
//! an eye's is left alone; without engine stereo the whole resources are the one picture.
//!
//! COLOUR. The model wants a display-encoded [0, 1] input; the game's output is scene-linear HDR (R11G11B10 float on 2.5,
//! RGBA16F before). The codec (rust/shaders/passes/src/neural_render.rs) tonemaps the eye's output into an SDR proxy
//! before NR and decodes the result back after, clamped against the original.
//!
//! The NGX calls that can fault -- the parameter object's getter on a foreign object, feature creation, evaluation and
//! release -- run inside cvr-engine's guarded range, through the dispatcher's own entries (the originals behind this
//! module's detours), so only NGX's frames are ever unwound.
#![allow(non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use core::cell::Cell;
use core::ffi::{c_char, c_void, CStr};
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

// ---- the exports: the switch and the model's knobs, live from the STEREO page; none saved -------------------------
// (a build that crashes with NR on cannot crash the next start)

/// THE SWITCH.
#[no_mangle]
pub static mut CyberpunkVR_NrEnable: i32 = 0;
/// The model's preset (applied when the feature is created) and style (0 natural, 1 cinematic).
#[no_mangle]
pub static mut CyberpunkVR_NrPreset: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrStyle: i32 = 0;
/// THE MODEL'S KNOBS AS nvngx_dlssnr 310.9.16 TAKES THEM (the DLSS5 Visual Enhancer bridge's ranges and defaults): tone,
/// structure and skin strength 0..1, default 1. Intensity reaches the model as min(1, intensity); above 1 the decode
/// pushes the model's change further, at 0 it keeps the original. Skin structure acts only with the automatic mask.
#[no_mangle]
pub static mut CyberpunkVR_NrIntensity: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrLocalTone: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrLocalStructure: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrSkinStructure: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrAutoMask: i32 = 0;
/// The codec: the scene's paper white before the tonemap, and how much of the NR result replaces the original.
#[no_mangle]
pub static mut CyberpunkVR_NrPaperWhite: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrColorStrength: f32 = 1.0;
/// Motion and depth. Scale base: 0 the vectors as they are, 1 the eye's DLSS scale times the render size (what DLSS
/// itself gets), 2 the same negated, 3 the output size; the multipliers fine-tune it. Depth: 0 the game's (reversed),
/// 1 not inverted, 2 inverted. Reset: no temporal history at all (a diagnostic).
#[no_mangle]
pub static mut CyberpunkVR_NrMVecScaleMode: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_NrMotionScaleX: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrMotionScaleY: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_NrDepthConversion: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrReset: i32 = 0;
/// One eye per frame (left on even Presents, right on odd): half the NR work, each eye's history at half rate.
#[no_mangle]
pub static mut CyberpunkVR_NrAlternateEyes: i32 = 0;
/// Status. Fault: 0 none, 1 nvngx_dlssnr.dll missing, 2 an NGX export missing, 3 feature unavailable, 4 reserved,
/// 5 CreateFeature faulted, 6 the evaluate faulted, 7 an output format the codec does not read, 8 the hooks would not go
/// in. Disabled: a fault stopped NR (the menu's Retry clears both).
#[no_mangle]
pub static mut CyberpunkVR_NrArmed: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrInited: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrContexts: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrDisabled: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrFault: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrCreateResult: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_NrLastResult: u32 = 0;
/// NR passes, left / right (or the one picture).
#[no_mangle]
pub static mut CyberpunkVR_NrEvals: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_NrPassesPerSec: f32 = 0.0;
/// The game's Presents per second, NR passes per Present (mean) and the most passes one Present had, over a second.
#[no_mangle]
pub static mut CyberpunkVR_NrFrameStats: [f32; 3] = [0.0; 3];
/// GPU milliseconds of one pass, averaged over a second: left encode, NR, decode, then the same for the right eye.
#[no_mangle]
pub static mut CyberpunkVR_NrGpuMs: [f32; 6] = [0.0; 6];

macro_rules! get {
    ($name:ident) => {{
        // SAFETY: the module's export, alive for the process; read whole.
        #[allow(unused_unsafe)]
        let v = unsafe { core::ptr::read_volatile(&raw const $name) };
        v
    }};
}
macro_rules! put {
    ($name:ident, $value:expr) => {{
        let v = $value;
        // SAFETY: as get!.
        #[allow(unused_unsafe)]
        unsafe {
            core::ptr::write_volatile(&raw mut $name, v)
        }
    }};
}

extern "C" {
    /// The frame path's reports (F10 Diagnostics): the [nr] line beside the [xr*] and [fsr3] ones.
    static CyberpunkVR_XrRateLog: i32;
}

fn log(text: &str) {
    cvr_host::log::write(text);
}
fn fault(reason: i32) {
    put!(CyberpunkVR_NrFault, reason);
    put!(CyberpunkVR_NrDisabled, 1);
}

thread_local! {
    /// An NR evaluation of this module's own also crosses the dispatcher hook: not evaluated again.
    static OURS: Cell<bool> = const { Cell::new(false) };
}
fn ours() -> bool {
    OURS.with(Cell::get)
}

const FEATURE_DLSS_NR: usize = 18;
/// The core NGX modules. The per-feature DLLs (nvngx_dlss.dll, ...) are not the dispatcher -- foveated_dlss.rs hooks
/// nvngx_dlss.dll's own evaluate -- and are never touched here.
const NGX_MODULES: [PCWSTR; 2] = [w!("_nvngx.dll"), w!("nvngx.dll")];
const GUARD_FAULT_CREATE: u32 = 0xDEAD_0003;
const GUARD_FAULT_RELEASE: u32 = 0xDEAD_0004;

fn ngx_failed(r: u32) -> bool {
    r & 0xFFF0_0000 == 0xBAD0_0000
}

// ---- NGX's parameter object, by its measured vtable (CP2077 + Streamline), not the SDK's declaration order ---------
/// A parameter object of NGX's.
#[derive(Clone, Copy)]
struct Params(*mut c_void);
impl Params {
    unsafe fn slot(&self, offset: usize) -> usize {
        let vtbl = *(self.0 as *const *const u8);
        *(vtbl.add(offset) as *const usize)
    }
    unsafe fn set_ptr(&self, name: &CStr, v: *mut c_void) {
        let f: extern "C" fn(*mut c_void, *const c_char, *mut c_void) = core::mem::transmute(self.slot(0x00));
        f(self.0, name.as_ptr(), v)
    }
    unsafe fn set_resource(&self, name: &CStr, v: *mut c_void) {
        let f: extern "C" fn(*mut c_void, *const c_char, *mut c_void) = core::mem::transmute(self.slot(0x08));
        f(self.0, name.as_ptr(), v)
    }
    unsafe fn set_uint(&self, name: &CStr, v: u32) {
        let f: extern "C" fn(*mut c_void, *const c_char, u32) = core::mem::transmute(self.slot(0x18));
        f(self.0, name.as_ptr(), v)
    }
    unsafe fn set_int(&self, name: &CStr, v: i32) {
        let f: extern "C" fn(*mut c_void, *const c_char, i32) = core::mem::transmute(self.slot(0x20));
        f(self.0, name.as_ptr(), v)
    }
    unsafe fn set_float(&self, name: &CStr, v: f32) {
        let f: extern "C" fn(*mut c_void, *const c_char, f32) = core::mem::transmute(self.slot(0x30));
        f(self.0, name.as_ptr(), v)
    }
    unsafe fn get_uint(&self, name: &CStr, out: &mut u32) -> u32 {
        let f: extern "C" fn(*mut c_void, *const c_char, *mut u32) -> u32 = core::mem::transmute(self.slot(0x58));
        f(self.0, name.as_ptr(), out)
    }
    unsafe fn get_int(&self, name: &CStr, out: &mut i32) -> u32 {
        let f: extern "C" fn(*mut c_void, *const c_char, *mut i32) -> u32 = core::mem::transmute(self.slot(0x60));
        f(self.0, name.as_ptr(), out)
    }
}

/// One bound resource of the game's evaluate and its description, the getter called on the foreign object inside the
/// guarded range. NGX's getter returns the stored pointer without a reference, and nothing is released.
unsafe fn read_resource(params: *const c_void, key: &CStr) -> Option<(ID3D12Resource, D3D12_RESOURCE_DESC)> {
    if params.is_null() {
        return None;
    }
    let vtbl = cvr_engine::code::read::<usize>(params as usize)?;
    let getter = cvr_engine::code::read::<usize>(vtbl + 0x48)?;
    let mut out: *mut c_void = core::ptr::null_mut();
    cvr_engine::code::guarded_call4(getter, params.cast_mut(), key.as_ptr().cast_mut().cast(), (&raw mut out).cast(), core::ptr::null_mut())?;
    let r = ID3D12Resource::from_raw_borrowed(&out)?.clone();
    let d = r.GetDesc();
    (d.Width != 0 && d.Height != 0).then_some((r, d))
}

/// The runtime asks this while it decides how much to scale (the add-on's callback).
extern "C" fn compute_scaling_ratio(p: *mut c_void) -> u32 {
    if p.is_null() {
        return 0xBAD0_0005;
    }
    let p = Params(p);
    // SAFETY: the runtime's parameter object.
    unsafe {
        let mut upscaling = 0u32;
        if ngx_failed(p.get_uint(c"DLSSNR.Upscaling", &mut upscaling)) {
            upscaling = 0;
        }
        let (mut input, mut output) = (0i32, 0i32);
        if ngx_failed(p.get_int(c"DLSSNR.InputWidth", &mut input)) {
            input = 0;
        }
        if ngx_failed(p.get_int(c"DLSSNR.OutputWidth", &mut output)) {
            output = 0;
        }
        let ratio = if upscaling != 0 && input != 0 && output != 0 { input as f32 / output as f32 } else { 1.0 };
        p.set_float(c"DLSSNR.ScalingRatio", ratio);
    }
    1
}

// ---- the colour codec -----------------------------------------------------------------------------------------------
/// Two descriptors a pass, four passes a frame: reused after 32 frames.
const CODEC_RING: usize = 256;
struct Codec {
    root: ID3D12RootSignature,
    encode: ID3D12PipelineState,
    decode: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    step: usize,
    ring: usize,
}
impl Codec {
    unsafe fn new(device: &ID3D12Device) -> Option<Self> {
        let ranges = [
            D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, ..Default::default() },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                NumDescriptors: 1,
                OffsetInDescriptorsFromTableStart: 1,
                ..Default::default()
            },
        ];
        let params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 6 },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 2, pDescriptorRanges: ranges.as_ptr() },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC { NumParameters: 2, pParameters: params.as_ptr(), ..Default::default() };
        let (mut blob, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        if D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)).is_err() {
            log("[nr] codec root signature failed\n");
            return None;
        }
        let blob = blob?;
        let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(
            0,
            core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()),
        ) else {
            log("[nr] codec CreateRootSignature failed\n");
            return None;
        };
        // main's HLSL (src/Render/NeuralRender.cpp), or the codec made from Rust (rust/shaders/passes/src/
        // neural_render.rs) with CVR_SHADER_RUST=1
        let make = |dxil: &'static [u8]| -> Option<ID3D12PipelineState> {
            let entry = if dxil.as_ptr() == cvr_gpu::dxil::NR_ENCODE_CS.as_ptr() { "Encode" } else { "Decode" };
            let code = cvr_gpu::dxil::pick(dxil, || crate::fsr3_shaders::compile_cs(crate::fsr3_shaders::CODEC, entry, "nr"))?;
            let p = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                CS: code.bytecode(),
                ..Default::default()
            };
            device.CreateComputePipelineState::<ID3D12PipelineState>(&p).ok()
        };
        let (Some(encode), Some(decode)) = (make(cvr_gpu::dxil::NR_ENCODE_CS), make(cvr_gpu::dxil::NR_DECODE_CS)) else {
            log("[nr] codec PSO failed\n");
            return None;
        };
        let hd = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: CODEC_RING as u32,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        let Ok(heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&hd) else {
            log("[nr] codec heap failed\n");
            return None;
        };
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
        log("[nr] codec ready\n");
        Some(Codec { root, encode, decode, heap, step, ring: 0 })
    }

    /// One pass, `src` (SRV) -> `dst` (UAV), each viewed in its own format; the caller has them in those states.
    #[allow(clippy::too_many_arguments)]
    unsafe fn run(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        encode: bool,
        src: &ID3D12Resource,
        src_format: DXGI_FORMAT,
        dst: &ID3D12Resource,
        dst_format: DXGI_FORMAT,
        size: [u32; 2],
        offset: [u32; 2],
        paper_white: f32,
        color_strength: f32,
    ) {
        let slot = (self.ring % (CODEC_RING / 2)) * 2;
        self.ring = self.ring.wrapping_add(1);
        let mut cpu = self.heap.GetCPUDescriptorHandleForHeapStart();
        let mut gpu = self.heap.GetGPUDescriptorHandleForHeapStart();
        cpu.ptr += slot * self.step;
        gpu.ptr += (slot * self.step) as u64;
        let sv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: src_format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
        };
        device.CreateShaderResourceView(src, Some(&sv), cpu);
        let uv = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: dst_format,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV::default() },
        };
        device.CreateUnorderedAccessView(dst, None::<&ID3D12Resource>, Some(&uv), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu.ptr + self.step });
        list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
        list.SetComputeRootSignature(&self.root);
        list.SetPipelineState(if encode { &self.encode } else { &self.decode });
        let k = [size[0], size[1], paper_white.to_bits(), color_strength.to_bits(), offset[0], offset[1]];
        list.SetComputeRoot32BitConstants(0, 6, k.as_ptr().cast(), 0);
        list.SetComputeRootDescriptorTable(1, gpu);
        list.Dispatch(size[0].div_ceil(8), size[1].div_ceil(8), 1);
    }
}

fn transition(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource, from: D3D12_RESOURCE_STATES, to: D3D12_RESOURCE_STATES) {
    crate::framegen_generator::barrier(list, Some(resource), from, to);
}
fn uav_barrier(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource) {
    let pointer = resource.as_raw();
    // SAFETY: the barrier borrows the resource's interface pointer for the call.
    unsafe {
        let b = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER { pResource: core::mem::transmute_copy(&pointer) }),
            },
        };
        list.ResourceBarrier(&[b]);
    }
}

// ---- NGX: its entries once, one NR feature per DLSS handle ---------------------------------------------------------
#[derive(Clone, Copy, Default)]
struct Ngx {
    allocate: usize,
    destroy_params: usize,
    create: usize,
    evaluate: usize,
    release: usize,
}

/// One NR feature, its two textures (the SDR proxy in, the NR result out, both the eye's output size) and the one
/// parameter object it was created with: the runtime ties the object to its feature, a fresh one at evaluate is
/// refused (0xBAD00005).
#[derive(Default)]
struct Context {
    handle: usize,
    feature: usize,
    params: usize,
    color: Option<ID3D12Resource>,
    output: Option<ID3D12Resource>,
    w: u32,
    h: u32,
    broken: bool,
    /// Its first evaluate resets the history.
    fresh: bool,
}

/// Four timestamps a pass -- before the encode, before NR, after NR, after the decode -- resolved into a readback ring of
/// 16 frames. Present reads the frame recorded 8 Presents ago, long finished on the GPU, and averages a second of them
/// into CyberpunkVR_NrGpuMs: what the codec and the model cost, apart from the rest of the frame.
const TIMER_FRAMES: u32 = 16;
const TIMER_STAMPS: u32 = 4;
struct Timer {
    heap: ID3D12QueryHeap,
    readback: ID3D12Resource,
    ticks_to_ms: f64,
}
impl Timer {
    fn index(frame: u32, eye: usize, stamp: u32) -> u32 {
        ((frame % TIMER_FRAMES) * 2 + eye as u32) * TIMER_STAMPS + stamp
    }
    unsafe fn new(device: &ID3D12Device) -> Option<Self> {
        // the frequency is a queue's; every queue of the adapter counts at the same rate
        let qd = D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() };
        let queue = device.CreateCommandQueue::<ID3D12CommandQueue>(&qd).ok()?;
        let frequency = queue.GetTimestampFrequency().ok().filter(|&f| f != 0)?;
        drop(queue);
        let count = TIMER_FRAMES * 2 * TIMER_STAMPS;
        let hd = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: count, NodeMask: 0 };
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(&hd, &mut heap).ok()?;
        let hp = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let rd = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: u64::from(count) * 8,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut readback: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &rd, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback).ok()?;
        Some(Timer { heap: heap?, readback: readback?, ticks_to_ms: 1000.0 / frequency as f64 })
    }
    unsafe fn stamp(&self, list: &ID3D12GraphicsCommandList, frame: u32, eye: usize, stamp: u32) {
        list.EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, Self::index(frame, eye, stamp));
    }
    unsafe fn resolve(&self, list: &ID3D12GraphicsCommandList, frame: u32, eye: usize) {
        let first = Self::index(frame, eye, 0);
        list.ResolveQueryData(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, first, TIMER_STAMPS, &self.readback, u64::from(first) * 8);
        WRITTEN[(frame % TIMER_FRAMES) as usize].fetch_or(1 << eye, Relaxed);
    }
}
/// Per ring frame, a bit per eye whose pass was resolved.
static WRITTEN: [AtomicU8; TIMER_FRAMES as usize] = [const { AtomicU8::new(0) }; TIMER_FRAMES as usize];

struct State {
    ngx: Option<Ngx>,
    contexts: Vec<Context>,
    codec: Option<Codec>,
    codec_tried: bool,
    timer: Option<Timer>,
    timer_tried: bool,
    /// The adapter, for the [nr] line's VRAM (by its LUID: a reference to the game's device would outlive its teardown).
    luid: Option<LUID>,
    adapter: Option<(LUID, IDXGIAdapter3)>,
    // ReadTimes' and CountFrame's windows
    gpu_sum: [f64; 6],
    gpu_count: [u32; 2],
    gpu_window: u64,
    frame_window: u64,
    presents: u32,
    passes: u32,
    most: u32,
    evaluate_logs: u32,
}
// SAFETY: the NGX handles and D3D12 objects are free-threaded; used under the lock.
unsafe impl Send for State {}
static STATE: Mutex<State> = Mutex::new(State {
    ngx: None,
    contexts: Vec::new(),
    codec: None,
    codec_tried: false,
    timer: None,
    timer_tried: false,
    luid: None,
    adapter: None,
    gpu_sum: [0.0; 6],
    gpu_count: [0; 2],
    gpu_window: 0,
    frame_window: 0,
    presents: 0,
    passes: 0,
    most: 0,
    evaluate_logs: 0,
});
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
static PRESENT_FRAME: AtomicU32 = AtomicU32::new(0);
static PASSES_SINCE_PRESENT: AtomicU32 = AtomicU32::new(0);

// ---- the detours' originals (MinHook's trampolines) -----------------------------------------------------------------
static mut ORIG_EVALUATE: *mut c_void = core::ptr::null_mut();
static mut ORIG_RELEASE: *mut c_void = core::ptr::null_mut();
static mut ORIG_SHUTDOWN1: *mut c_void = core::ptr::null_mut();
static mut ORIG_SHUTDOWN: *mut c_void = core::ptr::null_mut();
fn original(slot: *const *mut c_void) -> usize {
    // SAFETY: a trampoline slot written once by MinHook before the detour is enabled.
    unsafe { core::ptr::read_volatile(slot) as usize }
}
/// The dispatcher's evaluate as this module calls it: the original behind its own detour (so a fault unwinds NGX's frames
/// only), or the entry itself before the detour is in.
static EVALUATE_ENTRY: AtomicUsize = AtomicUsize::new(0);

unsafe fn module() -> Option<windows::Win32::Foundation::HMODULE> {
    NGX_MODULES.iter().find_map(|&name| GetModuleHandleW(name).ok())
}
unsafe fn export(core: windows::Win32::Foundation::HMODULE, name: &CStr) -> usize {
    GetProcAddress(core, windows::core::PCSTR(name.as_ptr().cast())).map_or(0, |f| f as usize)
}

impl State {
    unsafe fn init(&mut self) {
        if self.ngx.is_some() {
            return;
        }
        for &name in &NGX_MODULES {
            let Ok(core) = GetModuleHandleW(name) else { continue };
            let n = Ngx {
                allocate: export(core, c"NVSDK_NGX_D3D12_AllocateParameters"),
                destroy_params: export(core, c"NVSDK_NGX_D3D12_DestroyParameters"),
                create: export(core, c"NVSDK_NGX_D3D12_CreateFeature"),
                evaluate: export(core, c"NVSDK_NGX_D3D12_EvaluateFeature"),
                release: export(core, c"NVSDK_NGX_D3D12_ReleaseFeature"),
            };
            if n.allocate != 0 && n.destroy_params != 0 && n.create != 0 && n.evaluate != 0 && n.release != 0 {
                self.ngx = Some(n);
                put!(CyberpunkVR_NrInited, 1);
                return;
            }
        }
        fault(2);
    }

    fn live(&self) -> i32 {
        self.contexts.iter().filter(|c| c.handle != 0).count() as i32
    }

    unsafe fn destroy_params(&self, params: usize) {
        if let (Some(n), true) = (self.ngx, params != 0) {
            let _ = cvr_engine::code::guarded_call4(n.destroy_params, params as *mut c_void, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        }
    }
    unsafe fn release_context(&mut self, index: usize, why: &str) {
        let c = core::mem::take(&mut self.contexts[index]);
        let mut released = 0u32;
        if let (Some(n), true) = (self.ngx, c.feature != 0) {
            // the original behind the release detour when it is in
            let release = match original(&raw const ORIG_RELEASE) {
                0 => n.release,
                f => f,
            };
            OURS.with(|o| o.set(true));
            released = cvr_engine::code::guarded_call4(release, c.feature as *mut c_void, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
                .map_or(GUARD_FAULT_RELEASE, |r| r as usize as u32);
            OURS.with(|o| o.set(false));
        }
        self.destroy_params(c.params);
        log(&format!("[nr] context handle {:#x} released ({why}): 0x{released:08X}\n", c.handle));
        put!(CyberpunkVR_NrContexts, self.live());
    }

    unsafe fn vram(&mut self, luid: LUID) -> Option<(f64, f64)> {
        let same = |a: &LUID| a.LowPart == luid.LowPart && a.HighPart == luid.HighPart;
        if !self.adapter.as_ref().is_some_and(|(l, _)| same(l)) {
            self.adapter = None;
            let factory = CreateDXGIFactory1::<IDXGIFactory4>().ok()?;
            let adapter = factory.EnumAdapterByLuid::<IDXGIAdapter3>(luid).ok()?;
            self.adapter = Some((luid, adapter));
        }
        let (_, adapter) = self.adapter.as_ref()?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).ok()?;
        Some((info.CurrentUsage as f64 / 1048576.0, info.Budget as f64 / 1048576.0))
    }

    /// The NR feature of `handle` at this size: found, or made (its parameter object filled for a native pass -- colour
    /// and output the same size, Upscaling 0: the upscaling contract was refused, 0xBAD00005).
    unsafe fn context_for(&mut self, handle: usize, device: &ID3D12Device, list: &ID3D12GraphicsCommandList, w: u32, h: u32) -> Option<usize> {
        let mut free = None;
        for (i, c) in self.contexts.iter().enumerate() {
            // The game may still have commands in flight at the old extent: its release retires every context of the
            // handle.
            if c.handle == handle && c.w == w && c.h == h {
                return Some(i);
            }
            if c.handle == 0 && free.is_none() {
                free = Some(i);
            }
        }
        let index = match free {
            Some(i) => i,
            None if self.contexts.len() < 8 => {
                self.contexts.push(Context::default());
                self.contexts.len() - 1
            }
            None => return None,
        };
        let n = self.ngx?;
        let before = self.vram(device.GetAdapterLuid());
        let mut params: *mut c_void = core::ptr::null_mut();
        let allocate: unsafe extern "C" fn(*mut *mut c_void) -> u32 = core::mem::transmute(n.allocate);
        if ngx_failed(allocate(&mut params)) || params.is_null() {
            fault(2);
            return None;
        }
        fill_create_params(Params(params), w as i32, h as i32);
        let mut feature: *mut c_void = core::ptr::null_mut();
        OURS.with(|o| o.set(true));
        let r = cvr_engine::code::guarded_call4(n.create, list.as_raw(), FEATURE_DLSS_NR as *mut c_void, params, (&raw mut feature).cast())
            .map_or(GUARD_FAULT_CREATE, |r| r as usize as u32);
        OURS.with(|o| o.set(false));
        put!(CyberpunkVR_NrCreateResult, r);
        if r == GUARD_FAULT_CREATE {
            self.contexts[index] = Context { handle, params: params as usize, w, h, broken: true, ..Default::default() };
            fault(5);
            log(&format!("[nr] CreateFeature(18) FAULTED (handle {handle:#x})\n"));
            return Some(index);
        }
        if ngx_failed(r) || feature.is_null() {
            self.destroy_params(params as usize);
            // a later Retry issues a fresh create request
            fault(3);
            log(&format!("[nr] CreateFeature(18) failed 0x{r:08X} (handle {handle:#x}, {w}x{h})\n"));
            if r == 0xBAD0_000C {
                log("[nr] 0xBAD0000C = OutOfDate: the display driver predates DLSS 5 NR; a driver with it (616.64 or newer) is needed\n");
            }
            return None;
        }
        let color = texture(device, w, h, w!("CVR_NR_Proxy"));
        let output = texture(device, w, h, w!("CVR_NR_Result"));
        if self.codec.is_none() && !self.codec_tried {
            self.codec_tried = true;
            self.codec = Codec::new(device);
        }
        let broken = color.is_none() || output.is_none() || self.codec.is_none();
        self.contexts[index] = Context { handle, feature: feature as usize, params: params as usize, color, output, w, h, broken, fresh: true };
        put!(CyberpunkVR_NrContexts, self.live());
        let after = self.vram(device.GetAdapterLuid());
        let now = windows::Win32::System::SystemInformation::GetLocalTime();
        let (b, (a, budget)) = (before.map_or(-1.0, |v| v.0), after.unwrap_or((-1.0, -1.0)));
        log(&format!(
            "[nr] {:02}:{:02}:{:02} context {} live: VRAM {b:.0} -> {a:.0} MB of a {budget:.0} MB budget\n",
            now.wHour,
            now.wMinute,
            now.wSecond,
            self.live()
        ));
        log(&format!(
            "[nr] context handle {handle:#x} feature {:#x}, {w}x{h}{}\n",
            feature as usize,
            if broken { " BROKEN" } else { "" }
        ));
        Some(index)
    }
}

unsafe fn fill_create_params(p: Params, w: i32, h: i32) {
    p.set_int(c"Width", w);
    p.set_int(c"Height", h);
    p.set_int(c"OutWidth", w);
    p.set_int(c"OutHeight", h);
    p.set_int(c"DLSSNR.Width", w);
    p.set_int(c"DLSSNR.Height", h);
    p.set_int(c"DLSSNR.InputWidth", w);
    p.set_int(c"DLSSNR.InputHeight", h);
    p.set_int(c"DLSSNR.OutputWidth", w);
    p.set_int(c"DLSSNR.OutputHeight", h);
    p.set_int(c"DLSSNR.Output.Width", w);
    p.set_int(c"DLSSNR.Output.Height", h);
    p.set_float(c"DLSSNR.ScalingRatio", 1.0);
    p.set_float(c"DLSSNR.Scale", 1.0);
    p.set_uint(c"DLSSNR.Upscaling", 0);
    p.set_ptr(c"DLSSNRComputeScalingRatioCallback", compute_scaling_ratio as *const () as *mut c_void);
    p.set_int(c"DLSSNR.Hint.Render.Preset", get!(CyberpunkVR_NrPreset));
    p.set_uint(c"PerfQualityValue", 0);
    // the add-on's
    p.set_uint(c"DLSS.Feature.Create.Flags", 0x4B);
    p.set_int(c"CreationNodeMask", 1);
    p.set_int(c"VisibilityNodeMask", 1);
}

unsafe fn texture(device: &ID3D12Device, w: u32, h: u32, name: PCWSTR) -> Option<ID3D12Resource> {
    let hp = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let rd = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(w),
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    device.CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &rd, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, None, &mut r).ok()?;
    let r = r?;
    let _ = r.SetName(name);
    Some(r)
}

/// The codec reads the eye's output through an SRV and writes it back through a UAV it also loads from. RGBA16F is
/// always a typed-UAV-load format; R11G11B10_FLOAT -- the game's DLSS output on 2.5 -- only where the device says so.
unsafe fn codec_reads(device: &ID3D12Device, format: DXGI_FORMAT) -> bool {
    if format == DXGI_FORMAT_R16G16B16A16_FLOAT {
        return true;
    }
    if format != DXGI_FORMAT_R11G11B10_FLOAT {
        return false;
    }
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let mut s = D3D12_FEATURE_DATA_FORMAT_SUPPORT { Format: DXGI_FORMAT_R11G11B10_FLOAT, ..Default::default() };
        let need = D3D12_FORMAT_SUPPORT2_UAV_TYPED_LOAD.0 | D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE.0;
        device
            .CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT, (&raw mut s).cast(), core::mem::size_of_val(&s) as u32)
            .is_ok()
            && s.Support1.0 & D3D12_FORMAT_SUPPORT1_SHADER_LOAD.0 != 0
            && s.Support2.0 & need == need
    })
}

/// Where an eye is in the resources its evaluate hands NGX.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct EyeArea {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}
/// The eye's output, depth and motion areas (in pixels of each resource) and the motion scale its DLSS was given.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct EyeAreas {
    pub output: EyeArea,
    pub depth: EyeArea,
    pub motion: EyeArea,
    pub motion_scale: [f32; 2],
}

/// One eye's pass.
struct Job<'a> {
    list: &'a ID3D12GraphicsCommandList,
    device: &'a ID3D12Device,
    /// The game's DLSS output; the eye's area at `at`, the context's size.
    output: &'a ID3D12Resource,
    format: DXGI_FORMAT,
    at: [u32; 2],
    motion: &'a ID3D12Resource,
    motion_area: EyeArea,
    depth: &'a ID3D12Resource,
    depth_area: EyeArea,
    /// The eye's DLSS scale, 1 without engine stereo.
    motion_scale: [f32; 2],
    /// 0 left (or the one picture), 1 right.
    eye: usize,
    /// The Present count, for the timer's ring.
    frame: u32,
}

impl State {
    /// Encode the eye into the proxy, NR the proxy into the result, decode that back onto the eye -> false on a fault.
    unsafe fn run(&mut self, index: usize, j: &Job) -> bool {
        let (Some(n), Some(color), Some(result)) = (self.ngx, self.contexts[index].color.clone(), self.contexts[index].output.clone()) else {
            return true;
        };
        let c = &self.contexts[index];
        let (w, h, feature, params) = (c.w, c.h, c.feature, Params(c.params as *mut c_void));
        let stamp = |s: &State, stamp: u32| {
            if let Some(t) = &s.timer {
                t.stamp(j.list, j.frame, j.eye, stamp);
            }
        };
        let (uav, read) = (D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        let paper_white = get!(CyberpunkVR_NrPaperWhite);
        stamp(self, 0);
        transition(j.list, j.output, uav, read);
        let Some(codec) = self.codec.as_mut() else { return true };
        codec.run(j.device, j.list, true, j.output, j.format, &color, DXGI_FORMAT_R16G16B16A16_FLOAT, [w, h], j.at, paper_white, 1.0);
        transition(j.list, j.output, read, uav);
        uav_barrier(j.list, &color);
        stamp(self, 1);

        let p = params;
        p.set_resource(c"DLSSNR.Color", color.as_raw());
        p.set_resource(c"DLSSNR.Output", result.as_raw());
        p.set_resource(c"DLSSNR.MVec", j.motion.as_raw());
        p.set_resource(c"DLSSNR.Depth", j.depth.as_raw());
        let (wi, hi) = (w as i32, h as i32);
        p.set_int(c"Width", wi);
        p.set_int(c"Height", hi);
        p.set_int(c"OutWidth", wi);
        p.set_int(c"OutHeight", hi);
        p.set_int(c"DLSSNR.ColorSubrectBaseX", 0);
        p.set_int(c"DLSSNR.ColorSubrectBaseY", 0);
        p.set_int(c"DLSSNR.ColorSubrectWidth", wi);
        p.set_int(c"DLSSNR.ColorSubrectHeight", hi);
        p.set_int(c"DLSSNR.OutputSubrectBaseX", 0);
        p.set_int(c"DLSSNR.OutputSubrectBaseY", 0);
        p.set_int(c"DLSSNR.OutputSubrectWidth", wi);
        p.set_int(c"DLSSNR.OutputSubrectHeight", hi);
        let (d, m) = (j.depth_area, j.motion_area);
        p.set_int(c"DLSSNR.DepthSubrectBaseX", d.left as i32);
        p.set_int(c"DLSSNR.DepthSubrectBaseY", d.top as i32);
        p.set_int(c"DLSSNR.DepthSubrectWidth", d.width as i32);
        p.set_int(c"DLSSNR.DepthSubrectHeight", d.height as i32);
        p.set_int(c"DLSSNR.MVecSubrectBaseX", m.left as i32);
        p.set_int(c"DLSSNR.MVecSubrectBaseY", m.top as i32);
        p.set_int(c"DLSSNR.MVecSubrectWidth", m.width as i32);
        p.set_int(c"DLSSNR.MVecSubrectHeight", m.height as i32);
        p.set_int(c"DLSSNR.InputWidth", wi);
        p.set_int(c"DLSSNR.InputHeight", hi);
        p.set_int(c"DLSSNR.OutputWidth", wi);
        p.set_int(c"DLSSNR.OutputHeight", hi);
        p.set_float(c"DLSSNR.ScalingRatio", 1.0);
        p.set_float(c"DLSSNR.Scale", 1.0);
        p.set_uint(c"DLSSNR.Upscaling", 0);
        p.set_ptr(c"DLSSNRComputeScalingRatioCallback", compute_scaling_ratio as *const () as *mut c_void);
        // A wrong motion scale is moving colour noise: the history reprojects with MVec * scale, in pixels.
        let (render_x, render_y) = (j.motion_scale[0] * m.width as f32, j.motion_scale[1] * m.height as f32);
        let (base_x, base_y) = match get!(CyberpunkVR_NrMVecScaleMode) {
            1 => (render_x, render_y),
            2 => (-render_x, -render_y),
            3 => (w as f32, h as f32),
            _ => (1.0, 1.0),
        };
        p.set_float(c"DLSSNR.MVecScaleX", base_x * get!(CyberpunkVR_NrMotionScaleX));
        p.set_float(c"DLSSNR.MVecScaleY", base_y * get!(CyberpunkVR_NrMotionScaleY));
        p.set_uint(c"DLSSNR.DepthInverted", u32::from(get!(CyberpunkVR_NrDepthConversion) != 1));
        p.set_uint(c"DLSSNR.Enabled", 1);
        let fresh = self.contexts[index].fresh;
        p.set_uint(c"DLSSNR.Reset", u32::from(get!(CyberpunkVR_NrReset) != 0 || fresh));
        self.contexts[index].fresh = false;
        let unit = |v: f32| v.clamp(0.0, 1.0);
        p.set_float(c"DLSSNR.Intensity", unit(get!(CyberpunkVR_NrIntensity)));
        p.set_float(c"DLSSNR.LocalToneStrength", unit(get!(CyberpunkVR_NrLocalTone)));
        p.set_float(c"DLSSNR.LocalStructureStrength", unit(get!(CyberpunkVR_NrLocalStructure)));
        p.set_float(c"DLSSNR.SkinStructureStrength", unit(get!(CyberpunkVR_NrSkinStructure)));
        p.set_uint(c"DLSSNR.UseAutoMask", u32::from(get!(CyberpunkVR_NrAutoMask) != 0));
        p.set_int(c"DLSSNR.Style", get!(CyberpunkVR_NrStyle));
        p.set_uint(c"DLSSNR.UICorrection", 0);
        // set last, as the add-on does; without them the runtime refused the evaluate (0xBAD00005)
        p.set_uint(c"DLSS.Indicator.Invert.X.Axis", 0);
        p.set_uint(c"DLSS.Indicator.Invert.Y.Axis", 0);

        // through the dispatcher's original (OURS lets the detour pass it in any case)
        let evaluate = match EVALUATE_ENTRY.load(Relaxed) {
            0 => n.evaluate,
            f => f,
        };
        OURS.with(|o| o.set(true));
        let r = cvr_engine::code::guarded_call4(evaluate, j.list.as_raw(), feature as *mut c_void, params.0, core::ptr::null_mut());
        OURS.with(|o| o.set(false));
        let Some(r) = r.map(|r| r as usize as u32) else { return false };
        stamp(self, 2);
        put!(CyberpunkVR_NrLastResult, r);
        if self.evaluate_logs < 6 {
            self.evaluate_logs += 1;
            log(&format!(
                "[nr] evaluate handle {:#x} -> 0x{r:08X} ({w}x{h} at {},{}; motion {}x{} at {},{} scale {:.4},{:.4})\n",
                self.contexts[index].handle, j.at[0], j.at[1], m.width, m.height, m.left, m.top, j.motion_scale[0], j.motion_scale[1]
            ));
        }
        if ngx_failed(r) {
            return true;
        }
        transition(j.list, &result, uav, read);
        // the blend onto the game's picture: the colour strength, times the intensity above 1 (the bridge's push past the
        // model's own strength), nothing at intensity 0
        let intensity = get!(CyberpunkVR_NrIntensity).clamp(0.0, 2.0);
        let blend = get!(CyberpunkVR_NrColorStrength).clamp(0.0, 1.0) * if intensity <= 0.0 { 0.0 } else { intensity.max(1.0) };
        if let Some(codec) = self.codec.as_mut() {
            codec.run(j.device, j.list, false, &result, DXGI_FORMAT_R16G16B16A16_FLOAT, j.output, j.format, [w, h], j.at, paper_white, blend);
        }
        transition(j.list, &result, read, uav);
        stamp(self, 3);
        if let Some(t) = &self.timer {
            t.resolve(j.list, j.frame, j.eye);
        }
        true
    }

    /// Present thread: the passes recorded 8 Presents ago, summed and published once a second.
    unsafe fn read_times(&mut self, present: u32) {
        let Some(t) = &self.timer else { return };
        let frame = present.wrapping_sub(8);
        let eyes = WRITTEN[(frame % TIMER_FRAMES) as usize].swap(0, Relaxed);
        if eyes != 0 {
            let first = Timer::index(frame, 0, 0) as usize;
            let range = D3D12_RANGE { Begin: first * 8, End: (first + 2 * TIMER_STAMPS as usize) * 8 };
            let mut data = core::ptr::null_mut();
            if t.readback.Map(0, Some(&range), Some(&mut data)).is_ok() && !data.is_null() {
                let all = core::slice::from_raw_parts((data as *const u64).add(first), 2 * TIMER_STAMPS as usize);
                for eye in 0..2 {
                    let s = &all[eye * TIMER_STAMPS as usize..(eye + 1) * TIMER_STAMPS as usize];
                    if eyes & (1 << eye) == 0 || s[1] < s[0] || s[2] < s[1] || s[3] < s[2] {
                        continue;
                    }
                    for phase in 0..3 {
                        self.gpu_sum[eye * 3 + phase] += (s[phase + 1] - s[phase]) as f64 * t.ticks_to_ms;
                    }
                    self.gpu_count[eye] += 1;
                }
                t.readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            }
        }
        let now = windows::Win32::System::SystemInformation::GetTickCount64();
        if self.gpu_window == 0 {
            self.gpu_window = now;
        }
        if now - self.gpu_window < 1000 {
            return;
        }
        let mut ms = [0.0f32; 6];
        for (i, v) in ms.iter_mut().enumerate() {
            let n = self.gpu_count[i / 3];
            *v = if n != 0 { (self.gpu_sum[i] / f64::from(n)) as f32 } else { 0.0 };
            self.gpu_sum[i] = 0.0;
        }
        put!(CyberpunkVR_NrGpuMs, ms);
        self.gpu_count = [0; 2];
        self.gpu_window = now;
    }

    /// Present thread: the game's Presents and the NR passes between them, published once a second -- passes per second,
    /// Presents per second, the mean and the most passes one frame had (two a frame is one per eye).
    unsafe fn count_frame(&mut self) {
        let n = PASSES_SINCE_PRESENT.swap(0, Relaxed);
        self.presents += 1;
        self.passes += n;
        self.most = self.most.max(n);
        let now = windows::Win32::System::SystemInformation::GetTickCount64();
        if self.frame_window == 0 {
            self.frame_window = now;
        }
        if now - self.frame_window < 1000 {
            return;
        }
        let seconds = (now - self.frame_window) as f64 / 1000.0;
        let stats = [
            (f64::from(self.presents) / seconds) as f32,
            (f64::from(self.passes) / f64::from(self.presents)) as f32,
            self.most as f32,
        ];
        put!(CyberpunkVR_NrPassesPerSec, (f64::from(self.passes) / seconds) as f32);
        put!(CyberpunkVR_NrFrameStats, stats);
        // next to the [xr*] and [fsr3] lines, to weigh NR against the headset's flow: off unless that report is ticked
        if core::ptr::read_volatile(&raw const CyberpunkVR_XrRateLog) != 0 && get!(CyberpunkVR_NrEnable) != 0 {
            let (used, budget) = match self.luid {
                Some(l) => self.vram(l).unwrap_or((-1.0, -1.0)),
                None => (-1.0, -1.0),
            };
            let g = get!(CyberpunkVR_NrGpuMs);
            log(&format!(
                "[nr] game frames {:.1}/s, NR passes {:.1}/s ({:.2} a frame, most {}) | GPU per pass: left {:.2} ms = encode {:.2} + model {:.2} + decode {:.2}, right {:.2} ms = {:.2} + {:.2} + {:.2} | fault {} | contexts {}, VRAM {used:.0} of {budget:.0} MB\n",
                stats[0],
                get!(CyberpunkVR_NrPassesPerSec),
                stats[1],
                self.most,
                g[0] + g[1] + g[2],
                g[0],
                g[1],
                g[2],
                g[3] + g[4] + g[5],
                g[3],
                g[4],
                g[5],
                get!(CyberpunkVR_NrFault),
                get!(CyberpunkVR_NrContexts)
            ));
        }
        self.frame_window = now;
        self.presents = 0;
        self.passes = 0;
        self.most = 0;
    }

    fn teardown(&mut self, why: &str) {
        for i in 0..self.contexts.len() {
            if self.contexts[i].handle != 0 {
                // SAFETY: the contexts' own NGX objects.
                unsafe { self.release_context(i, why) };
            }
        }
        self.ngx = None;
        put!(CyberpunkVR_NrInited, 0);
    }
}

/// After the game's evaluate: this eye's NR, when the switch is on and the evaluate is a DLSS one this can read.
unsafe fn after_evaluate(list: *mut c_void, handle: *const c_void, params: *const c_void) {
    let eye = crate::engine_stereo_dlss::cvr_engine_stereo_dlss_evaluating_eye();
    let areas = if eye >= 0 { crate::engine_stereo_dlss::evaluating_areas() } else { None };
    let per_eye = areas.is_some();
    // UNDER ENGINE STEREO, AN EYE'S EVALUATE OR NONE. One outside an eye's (the engine drawing one picture: a menu, a
    // load) had NR make a feature for the whole side-by-side output beside the two eyes' (main, 2026-11-02, a launch
    // that ended in GPU timeouts). Only without engine stereo is the whole output the one picture.
    if !per_eye && crate::engine_stereo::cvr_engine_stereo_enabled() {
        return;
    }
    let (Some((output, od)), Some((motion, md)), Some((depth, dd))) =
        (read_resource(params, c"Output"), read_resource(params, c"MotionVectors"), read_resource(params, c"Depth"))
    else {
        return;
    };
    let whole = |d: &D3D12_RESOURCE_DESC| EyeArea { left: 0, top: 0, width: d.Width as u32, height: d.Height };
    let areas = areas.unwrap_or(EyeAreas { output: whole(&od), motion: whole(&md), depth: whole(&dd), motion_scale: [1.0; 2] });
    let inside = |a: &EyeArea, d: &D3D12_RESOURCE_DESC| {
        a.width != 0 && a.height != 0 && u64::from(a.left + a.width) <= d.Width && a.top + a.height <= d.Height
    };
    if !inside(&areas.output, &od) || !inside(&areas.motion, &md) || !inside(&areas.depth, &dd) {
        return;
    }
    let frame = PRESENT_FRAME.load(Relaxed);
    if get!(CyberpunkVR_NrAlternateEyes) != 0 && per_eye && (eye == 1) != (frame & 1 != 0) {
        return;
    }
    let mut device: Option<ID3D12Device> = None;
    if output.GetDevice(&mut device).is_err() {
        return;
    }
    let Some(device) = device else { return };
    let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&list) else { return };
    let mut s = state();
    if s.luid.is_none() {
        s.luid = Some(device.GetAdapterLuid());
    }
    if !codec_reads(&device, od.Format) {
        log(&format!("[nr] the DLSS output is format {}: the codec cannot read and write it\n", od.Format.0));
        fault(7);
        return;
    }
    s.init();
    if s.ngx.is_none() {
        return;
    }
    let (w, h) = (areas.output.width, areas.output.height);
    let Some(index) = s.context_for(handle as usize, &device, list, w, h) else { return };
    let c = &s.contexts[index];
    if c.broken || c.w != w || c.h != h {
        return;
    }
    if s.timer.is_none() && !s.timer_tried {
        s.timer_tried = true;
        s.timer = Timer::new(&device);
    }
    let eye = usize::from(eye == 1);
    let job = Job {
        list,
        device: &device,
        output: &output,
        format: od.Format,
        at: [areas.output.left, areas.output.top],
        motion: &motion,
        motion_area: areas.motion,
        depth: &depth,
        depth_area: areas.depth,
        motion_scale: areas.motion_scale,
        eye,
        frame,
    };
    if s.run(index, &job) {
        if !ngx_failed(get!(CyberpunkVR_NrLastResult)) {
            let mut evals = get!(CyberpunkVR_NrEvals);
            evals[eye] = evals[eye].wrapping_add(1);
            put!(CyberpunkVR_NrEvals, evals);
            PASSES_SINCE_PRESENT.fetch_add(1, Relaxed);
        }
    } else {
        log("[nr] evaluate FAULTED -- NR stopped\n");
        fault(6);
    }
}

// ---- the detours ----------------------------------------------------------------------------------------------------
unsafe extern "C" fn hooked_evaluate(list: *mut c_void, handle: *const c_void, params: *const c_void, callback: *mut c_void) -> u32 {
    let f: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void, *mut c_void) -> u32 =
        core::mem::transmute(original(&raw const ORIG_EVALUATE));
    let r = f(list, handle, params, callback);
    if !ours()
        && get!(CyberpunkVR_NrEnable) != 0
        && get!(CyberpunkVR_NrDisabled) == 0
        && !ngx_failed(r)
        && !list.is_null()
        && !params.is_null()
    {
        after_evaluate(list, handle, params);
    }
    r
}
/// The game's NGX teardown: NR's goes first.
unsafe extern "C" fn hooked_release(handle: *mut c_void) -> u32 {
    if !ours() && !handle.is_null() {
        let mut s = state();
        for i in 0..s.contexts.len() {
            if s.contexts[i].handle == handle as usize {
                s.release_context(i, "the game released its DLSS feature");
            }
        }
    }
    let f: unsafe extern "C" fn(*mut c_void) -> u32 = core::mem::transmute(original(&raw const ORIG_RELEASE));
    f(handle)
}
unsafe extern "C" fn hooked_shutdown1(device: *mut c_void) -> u32 {
    if !ours() {
        state().teardown("game shuts NGX down");
    }
    let f: unsafe extern "C" fn(*mut c_void) -> u32 = core::mem::transmute(original(&raw const ORIG_SHUTDOWN1));
    f(device)
}
unsafe extern "C" fn hooked_shutdown() -> u32 {
    if !ours() {
        state().teardown("game shuts NGX down");
    }
    let f: unsafe extern "C" fn() -> u32 = core::mem::transmute(original(&raw const ORIG_SHUTDOWN));
    f()
}

/// THE MODEL'S RUNTIME: NGX finds a feature's DLL beside the game's exe (bin\x64, as nvngx_dlss.dll), and without
/// nvngx_dlssnr.dll every creation fails with 0xBAD0000B -- shown as "the runtime does not expose NR" while the file
/// was simply not there. Asked once a switch-on, before the hooks go in; its size logged.
fn runtime_present() -> bool {
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return true };
    let path = dir.join("nvngx_dlssnr.dll");
    match std::fs::metadata(&path) {
        Ok(m) => {
            log(&format!("[neural-render] the DLSS 5 runtime {} ({:.1} MB)\n", path.display(), m.len() as f64 / 1048576.0));
            true
        }
        Err(_) => {
            log(&format!("[neural-render] no DLSS 5 runtime: {} is missing (the port's package puts it in bin\\x64)\n", path.display()));
            false
        }
    }
}

/// The NGX hooks, lifecycle first (before any evaluation can make a context), as one chain: all in, or NR faults.
unsafe fn arm() {
    let Some(core) = module() else { return };
    let evaluate = export(core, c"NVSDK_NGX_D3D12_EvaluateFeature");
    let release = export(core, c"NVSDK_NGX_D3D12_ReleaseFeature");
    // NGX not loaded yet: the next Present tries again
    if evaluate == 0 {
        return;
    }
    if release == 0 {
        fault(8);
        return;
    }
    let detour = |name: &'static str, target: usize, detour: *const c_void, original: *mut *mut c_void| cvr_engine::hooks::Detour {
        name,
        target,
        absolute: true,
        detour,
        original,
        expect: None,
    };
    let mut detours = vec![detour("NVSDK_NGX_D3D12_ReleaseFeature", release, hooked_release as *const c_void, &raw mut ORIG_RELEASE)];
    let shutdown1 = export(core, c"NVSDK_NGX_D3D12_Shutdown1");
    if shutdown1 != 0 {
        detours.push(detour("NVSDK_NGX_D3D12_Shutdown1", shutdown1, hooked_shutdown1 as *const c_void, &raw mut ORIG_SHUTDOWN1));
    }
    let shutdown = export(core, c"NVSDK_NGX_D3D12_Shutdown");
    if shutdown != 0 {
        detours.push(detour("NVSDK_NGX_D3D12_Shutdown", shutdown, hooked_shutdown as *const c_void, &raw mut ORIG_SHUTDOWN));
    }
    detours.push(detour("NVSDK_NGX_D3D12_EvaluateFeature", evaluate, hooked_evaluate as *const c_void, &raw mut ORIG_EVALUATE));
    match cvr_engine::hooks::install_chain_on_any_build("neural-render", &detours) {
        Ok(()) => {
            for d in &detours {
                log(&format!("[neural-render] hooked {} at {:#x}\n", d.name, d.target));
            }
            EVALUATE_ENTRY.store(original(&raw const ORIG_EVALUATE), Relaxed);
            put!(CyberpunkVR_NrArmed, 1);
        }
        Err(e) => {
            log(&format!("[neural-render] the NGX hooks did not go in: {e}\n"));
            fault(8);
        }
    }
}

/// cvr::neural_render::OnPresent: once per Present -- the frame the alternate-eyes mode counts, the GPU times and the
/// pass counts, and the NGX hooks put in once the switch is on.
pub fn on_present() {
    let (armed, enabled) = (get!(CyberpunkVR_NrArmed) != 0, get!(CyberpunkVR_NrEnable) != 0);
    if !armed && !enabled {
        return;
    }
    let frame = PRESENT_FRAME.fetch_add(1, Relaxed).wrapping_add(1);
    {
        let mut s = state();
        // SAFETY: the module's timer and counters.
        unsafe {
            s.read_times(frame);
            s.count_frame();
        }
    }
    if enabled && !armed && get!(CyberpunkVR_NrDisabled) == 0 {
        static ASKED: AtomicU8 = AtomicU8::new(0);
        if ASKED.swap(1, Relaxed) == 0 && !runtime_present() {
            ASKED.store(0, Relaxed); // asked again after the menu's Retry
            fault(1);
            return;
        }
        // SAFETY: NGX's exports, hooked once.
        unsafe { arm() };
    }
}

/// The STEREO page's DLSS 5 NEURAL RENDERING section (main's DrawNeuralRenderControls).
pub fn draw_controls() {
    use crate::ui;
    use cvr_crt::Arg;
    let checkbox = |label: &CStr, value: *mut i32| {
        // SAFETY: the module's export.
        let mut on = unsafe { core::ptr::read_volatile(value) } != 0;
        if ui::checkbox(label, &mut on) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(value, i32::from(on)) };
        }
    };
    let slider = |label: &CStr, value: *mut f32, min: f32, max: f32, fmt: &CStr| {
        // SAFETY: the module's export.
        let mut v = unsafe { core::ptr::read_volatile(value) };
        if ui::slider_float(label, &mut v, min, max, fmt) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(value, v) };
        }
    };
    let combo = |label: &CStr, value: *mut i32, items: &[&CStr]| {
        // SAFETY: the module's export.
        let mut v = unsafe { core::ptr::read_volatile(value) };
        if ui::combo(label, &mut v, items) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(value, v) };
        }
    };
    crate::vr_widgets::section(c"DLSS 5 NEURAL RENDERING");
    ui::text_wrapped(
        c"Turn DLSS on in the game's graphics settings, then tick DLSS 5 Neural Render here. Needs an RTX GPU, an NVIDIA driver with DLSS 5 (616.64 or newer) and nvngx_dlssnr.dll in bin\\x64 (the port's package carries it). Starts off at every launch.",
        &[],
    );
    ui::separator();
    checkbox(c"DLSS 5 Neural Render", &raw mut CyberpunkVR_NrEnable);
    ui::tooltip(c"Runs the neural render pass on both eyes and puts its picture in the headset.\nOff = the game's own DLSS picture, untouched.");
    const FAULTS: [&CStr; 9] = [
        c"",
        c"nvngx_dlssnr.dll (the DLSS 5 runtime) is not in the game's bin\\x64 folder.",
        c"An NGX function is missing.",
        c"The installed NGX runtime does not expose DLSS 5 NR for this game.",
        c"NGX crashed while starting.",
        c"Creating the neural render crashed.",
        c"The neural render pass crashed.",
        c"The game's DLSS picture is in a format this cannot read.",
        c"The DLSS hook could not be put in.",
    ];
    let (red, amber, green) = ([1.0, 0.4, 0.4, 1.0], [1.0, 0.8, 0.35, 1.0], [0.45, 0.9, 0.5, 1.0]);
    if get!(CyberpunkVR_NrDisabled) != 0 {
        let f = get!(CyberpunkVR_NrFault);
        let mut reason: &CStr = if f > 0 && (f as usize) < FAULTS.len() { FAULTS[f as usize] } else { c"NGX error." };
        // 5fa0c8b57: the driver's own refusal named
        if f == 3 && get!(CyberpunkVR_NrCreateResult) == 0xBAD0_000C {
            reason = c"the NVIDIA driver is too old for DLSS 5 (NGX: out of date). Update it to a driver with DLSS 5 (616.64 or newer).";
        }
        ui::text_colored(red, c"Stopped: %s", &[Arg::S(reason.as_ptr())]);
        if ui::button(c"Retry") {
            put!(CyberpunkVR_NrFault, 0);
            put!(CyberpunkVR_NrDisabled, 0);
        }
    } else if get!(CyberpunkVR_NrEnable) == 0 {
        ui::text_disabled(c"Off.", &[]);
    } else if get!(CyberpunkVR_NrArmed) == 0 {
        ui::text_colored(amber, c"Waiting for the game's DLSS.", &[]);
    } else if get!(CyberpunkVR_NrInited) == 0 {
        ui::text_colored(amber, c"Waiting for the first DLSS frame.", &[]);
    } else {
        let n = get!(CyberpunkVR_NrContexts);
        ui::text_colored(green, c"Running: %d eye context%s", &[Arg::I(n), Arg::S(if n == 1 { c"".as_ptr() } else { c"s".as_ptr() })]);
    }
    if get!(CyberpunkVR_NrEnable) == 0 {
        return;
    }
    let evals = get!(CyberpunkVR_NrEvals);
    ui::text_disabled(
        c"passes/s %.1f   left %u  right %u   last 0x%08X   create 0x%08X",
        &[
            Arg::F(get!(CyberpunkVR_NrPassesPerSec).into()),
            Arg::U(evals[0].into()),
            Arg::U(evals[1].into()),
            Arg::U(get!(CyberpunkVR_NrLastResult).into()),
            Arg::U(get!(CyberpunkVR_NrCreateResult).into()),
        ],
    );
    let stats = get!(CyberpunkVR_NrFrameStats);
    ui::text(
        c"Game frames/s %.1f   NR passes per frame %.2f (most %.0f)",
        &[Arg::F(stats[0].into()), Arg::F(stats[1].into()), Arg::F(stats[2].into())],
    );
    ui::tooltip(c"The game's Presents per second, and how many neural render passes ran between two of them:\n2 = one per eye, as it should be.");
    let g = get!(CyberpunkVR_NrGpuMs);
    ui::text(
        c"GPU ms  left: encode %.2f  NR %.2f  decode %.2f   right: encode %.2f  NR %.2f  decode %.2f",
        &g.map(|v| Arg::F(v.into())),
    );
    ui::tooltip(c"The GPU time of each eye's pass, averaged over a second: the colour encode, the neural\nrender model itself and the decode back onto the eye.");
    checkbox(c"Alternate eyes (one eye per frame)", &raw mut CyberpunkVR_NrAlternateEyes);
    ui::tooltip(c"Half the work: the left eye on even frames, the right on odd, the other eye showing\nthe game's own picture. Each eye's history updates at half rate and may shimmer.");

    crate::vr_widgets::section(c"MODEL");
    combo(
        c"NR preset",
        &raw mut CyberpunkVR_NrPreset,
        &[c"Default", c"Preset #1", c"Preset #2", c"Preset #3", c"Preset #4", c"Preset #5", c"Preset #6"],
    );
    ui::tooltip(c"Applied when the neural render is created: turn it off and on again.");
    combo(c"NR style", &raw mut CyberpunkVR_NrStyle, &[c"Natural", c"Cinematic", c"Style 2"]);
    slider(c"NR intensity", &raw mut CyberpunkVR_NrIntensity, 0.0, 2.0, c"%.2f");
    ui::tooltip(c"Up to 1: the model's own strength. Above 1: the change the model makes is pushed further\n(as the DLSS5 Visual Enhancer does). 0 = the game's picture.");
    slider(c"Local tone strength", &raw mut CyberpunkVR_NrLocalTone, 0.0, 1.0, c"%.2f");
    slider(c"Local structure strength", &raw mut CyberpunkVR_NrLocalStructure, 0.0, 1.0, c"%.2f");
    checkbox(c"Automatic mask", &raw mut CyberpunkVR_NrAutoMask);
    ui::tooltip(c"The model finds the skin in the picture itself and gives it its own structure strength.");
    ui::begin_disabled(get!(CyberpunkVR_NrAutoMask) == 0);
    slider(c"Skin structure strength", &raw mut CyberpunkVR_NrSkinStructure, 0.0, 1.0, c"%.2f");
    ui::end_disabled();
    // ImGuiHoveredFlags_AllowWhenDisabled
    if ui::is_item_hovered_flags(1 << 10) {
        ui::set_tooltip(c"The structure strength on skin. Needs the automatic mask.", &[]);
    }

    crate::vr_widgets::section(c"MOTION / DEPTH");
    combo(
        c"Motion scale",
        &raw mut CyberpunkVR_NrMVecScaleMode,
        &[c"As given", c"As DLSS (render size)", c"As DLSS, inverted", c"Output size"],
    );
    ui::tooltip(c"How the game's motion vectors become pixels for the model. A wrong one is moving colour\nnoise; \"As DLSS\" is what the game's own DLSS gets.");
    slider(c"Motion scale X", &raw mut CyberpunkVR_NrMotionScaleX, -2.0, 2.0, c"%.3f");
    slider(c"Motion scale Y", &raw mut CyberpunkVR_NrMotionScaleY, -2.0, 2.0, c"%.3f");
    combo(c"Depth", &raw mut CyberpunkVR_NrDepthConversion, &[c"Game (reversed)", c"Not inverted", c"Inverted"]);
    checkbox(c"Reset the history every frame", &raw mut CyberpunkVR_NrReset);
    ui::tooltip(c"Diagnostic: no temporal accumulation. Moving noise that stops with this on is the motion\nscale; noise that stays is the colour input.");

    crate::vr_widgets::section(c"COLOUR");
    slider(c"Colour strength", &raw mut CyberpunkVR_NrColorStrength, 0.0, 1.0, c"%.2f");
    ui::tooltip(c"How much of the neural render's picture replaces the game's: 0 = none, 1 = all.");
    slider(c"Scene paper white", &raw mut CyberpunkVR_NrPaperWhite, 0.1, 4.0, c"%.2f");
    ui::tooltip(c"The scene brightness taken as white when the HDR picture is mapped for the model.");
}

#[cfg(test)]
mod tests {
    #[test]
    fn ngx_failures_are_the_bad_range() {
        assert!(super::ngx_failed(0xBAD0_0005));
        assert!(super::ngx_failed(0xBAD0_000C));
        assert!(!super::ngx_failed(1));
        assert!(!super::ngx_failed(0xDEAD_0003));
    }
    #[test]
    fn timer_ring_indices() {
        use super::Timer;
        assert_eq!(Timer::index(0, 0, 0), 0);
        assert_eq!(Timer::index(0, 1, 3), 7);
        assert_eq!(Timer::index(16, 0, 0), 0);
        assert_eq!(Timer::index(15, 1, 3), (15 * 2 + 1) * 4 + 3);
    }
}
