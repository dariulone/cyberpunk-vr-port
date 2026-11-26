//! FSR3 FRAME GENERATION, PER EYE (formerly main's src/Framegen/Fsr3.cpp, Fsr3Dilate.cpp and Fsr3Extrapolate.cpp, main's
//! 2e123a4a3 and 5fa0c8b57, adapted there from iPowerTech/cyberpunk-vr-port): the frame generator's backends 4 (FSR3
//! interpolation) and 5 (FSR3 extrapolation), on the port's pinned FidelityFX SDK (cmake/NativeFramegen.cmake).
//!
//! INTERPOLATION: per eye a FidelityFX frame interpolation context and an optical flow context, fed by the dilate pass
//! (rust/shaders/passes/src/fsr3.rs: the nearest depth of each pixel's 3x3, the motion of that tap, the previous depth
//! reconstructed): the frame between the previous capture and this one.
//! EXTRAPOLATION: the last real frame carried half a game frame ahead along the dilated motion (the warp pass), shown on
//! the next display cycle that still has that capture.
//!
//! Everything is recorded on the module's own COMPUTE queue -- the dilate pass, FidelityFX's interpolation and optical
//! flow (their DX12 backend records dispatches, copies and barriers only) are compute work, and on a compute queue the
//! GPU runs them beside the game's graphics work (main measured the headset's DIRECT queue taking turns with the game:
//! 35-40 fps with FSR3 off, 33-34 interpolating unseen). Three lists deep, each holding the inputs and the capture lease
//! it read until its fence passes; five timestamps per list give the generation's GPU time -- an interpolated frame fits
//! between two real ones only if it is made in half the time between them (framegen_presenter.rs, select_fsr3).
//!
//! The presenter owns the cadence and calls in from the frame loop's thread; the frame loop's copy queue waits on the
//! last generation before it copies (queue_wait_for_last). Faults inside the SDK end FSR3 until it is switched on again
//! (main's __try around the whole generation; here each SDK call is made inside cvr-engine's guarded range, so only the
//! SDK's own frames are unwound).
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_void, CStr};
use core::mem::ManuallyDrop;
use std::sync::{LazyLock, Mutex, MutexGuard};

use cvr_gpu::framegen_core::{self as core_fg, Settings};
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::framegen_generator::{
    barrier, raw, resource as wrap, Context, FfxDimensions2D, FfxFloatCoords2D,
    FfxFrameInterpolationContextDescription, FfxFrameInterpolationDispatchDescription, FfxInterface,
    FfxOpticalflowContextDescription, FfxOpticalflowDispatchDescription, FfxRect2D, FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB,
    FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE, FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED, FFX_OK,
    FFX_RESOURCE_STATE_COMPUTE_READ, FFX_RESOURCE_STATE_UNORDERED_ACCESS, FFX_SURFACE_FORMAT_R8G8B8A8_UNORM,
};
use crate::framegen_generator as ffx;
use crate::framegen_inputs::Inputs;
use crate::framegen_presenter::{InputsRef, Lease};

/// cvr::framegen::Backend::Fsr3Interpolation / Fsr3Extrapolation.
pub const INTERPOLATION: i32 = 4;
pub const EXTRAPOLATION: i32 = 5;
/// cvr::framegen::Fsr3Backend.
pub fn is_backend(backend: i32) -> bool {
    backend == INTERPOLATION || backend == EXTRAPOLATION
}

const POOL_SLOTS: usize = 3;
const LISTS: usize = 3;
/// Start, then per eye after its dilate and after its interpolation (the last one is the end).
const STAMPS: usize = 5;
/// The snap shows a frame or two after it is handed over, plus the copies queued.
const SNAP_CAPTURES: i32 = 6;
/// HALF A GAME FRAME AHEAD: the headset shows the extrapolated frame half the game's frame interval after its real one,
/// and the pose it goes with is moved as far (framegen_presenter.rs, ahead). A fixed half, as main's first test has it.
const EXTRAPOLATION_FACTOR: f32 = 0.5;
/// Descriptors per pass: t0, t1, u0..u4 (the root signature's one table).
const TABLE: usize = 7;
/// Per list and eye, the dilate pass's table and the warp's.
const TABLES_PER_LIST: usize = 4;

/// cvr::fsr3::Status.
#[derive(Clone, Copy)]
pub struct Status {
    pub ready: bool,
    pub faulted: bool,
    pub interpolated: u64,
    pub primed: u64,
    pub skipped: u64,
    pub extrapolated: u64,
    pub text: &'static CStr,
}
impl Default for Status {
    fn default() -> Self {
        Status { ready: false, faulted: false, interpolated: 0, primed: 0, skipped: 0, extrapolated: 0, text: c"Off" }
    }
}

/// cvr::fsr3::Settings, from the frame generator's: on with FSR3 as the backend, its mode, its optical flow, and whether
/// the generated frames go to the headset (off, FSR3 still generates and the headset gets only the real frames: what
/// showing them costs, apart from what computing them costs).
#[derive(Clone, Copy, PartialEq)]
struct Wanted {
    enabled: bool,
    extrapolation: bool,
    optical_flow: bool,
    show: bool,
}

#[derive(Default)]
struct Slot {
    texture: Option<ID3D12Resource>,
    serial: u64,
}

#[derive(Default)]
struct List {
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    fence: u64,
    /// Its timestamps were resolved into the readback buffer.
    timed: bool,
    inputs: [Option<InputsRef>; 2],
    lease: Option<Lease>,
}

/// One eye's dilate outputs (render size): the dilated depth (R32_FLOAT), the dilated motion (R16G16_FLOAT), the
/// reconstructed previous depth (R32_FLOAT) and the scatter's scratch (R32_UINT), all resting in UNORDERED_ACCESS.
#[derive(Default)]
struct Dilated {
    depth: Option<ID3D12Resource>,
    motion: Option<ID3D12Resource>,
    previous: Option<ID3D12Resource>,
    scratch: Option<ID3D12Resource>,
    width: u32,
    height: u32,
}

/// The passes made from Rust: one root signature (b0 constants, a table t0-t1 + u0-u4, a linear clamp sampler s0),
/// dilate's three pipelines and the warp's, and a shader-visible heap of a table per list, eye and pass.
struct Passes {
    root: ID3D12RootSignature,
    clear: ID3D12PipelineState,
    dilate: ID3D12PipelineState,
    resolve: ID3D12PipelineState,
    warp: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    step: usize,
    eyes: [Dilated; 2],
}

/// What the contexts, the dilate outputs and the pool were made for; a change retires all of them first.
#[derive(Clone, Copy, PartialEq)]
struct Layout {
    device: usize,
    width: u64,
    height: u32,
    render_width: u32,
    render_height: u32,
    inverted: bool,
    infinite: bool,
}

struct State {
    wanted: Wanted,
    faulted: bool,
    status: Status,
    /// The generation's GPU time, both eyes, smoothed (0 until measured).
    generation_ms: f64,
    // ---- FidelityFX: one backend, per eye a frame interpolation and an optical flow context
    scratch: Vec<u128>,
    backend: FfxInterface,
    backend_ready: bool,
    fi: [Option<Context>; 2],
    of: [Option<Context>; 2],
    fi_valid: [bool; 2],
    of_valid: [bool; 2],
    /// Grow-only: the engine's dynamic resolution moves the render size, and a context dispatched beyond the size it was
    /// made for writes past its own buffers (the fork's first GPU hang).
    context_render: [u32; 2],
    context_display: [u32; 2],
    first_dispatch: [bool; 2],
    of_vector: [Option<ID3D12Resource>; 2],
    of_scd: [Option<ID3D12Resource>; 2],
    of_vector_size: [[u32; 2]; 2],
    // ---- the generated frames, per eye, tagged with the capture they lead to
    pool: [[Slot; POOL_SLOTS]; 2],
    pool_next: [usize; 2],
    // ---- the module's own command lists, on its own compute queue
    queue: Option<ID3D12CommandQueue>,
    lists: [List; LISTS],
    list_next: usize,
    fence: Option<ID3D12Fence>,
    fence_value: u64,
    timestamps: Option<ID3D12QueryHeap>,
    readback: Option<ID3D12Resource>,
    ticks_to_ms: f64,
    // ---- what the previous capture was, for "does this one follow it" (0 = nothing to follow)
    last_serial: u64,
    last_origin: u64,
    last_frame: u32,
    // ---- a turn the motion does not continue (camera_jumped)
    snap_seen: u32,
    snap_left: i32,
    snap_threshold: f32,
    snap_found: bool,
    last_forward: [f32; 3],
    last_camera_frame: u32,
    last_camera_origin: u64,
    passes: Option<Passes>,
    layout: Option<Layout>,
    producer_fence: Option<ID3D12Fence>,
    consumer_fence: Option<ID3D12Fence>,
    producer_value: u64,
    consumer_value: u64,
    dispatch_logs: u32,
}
// SAFETY: the XR frame thread's state, used under the lock; the D3D12 objects are free-threaded.
unsafe impl Send for State {}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        wanted: Wanted { enabled: false, extrapolation: false, optical_flow: true, show: true },
        faulted: false,
        status: Status::default(),
        generation_ms: 0.0,
        scratch: Vec::new(),
        backend: FfxInterface([0; 29]),
        backend_ready: false,
        fi: [None, None],
        of: [None, None],
        fi_valid: [false; 2],
        of_valid: [false; 2],
        context_render: [0; 2],
        context_display: [0; 2],
        first_dispatch: [false; 2],
        of_vector: [None, None],
        of_scd: [None, None],
        of_vector_size: [[0; 2]; 2],
        pool: Default::default(),
        pool_next: [0; 2],
        queue: None,
        lists: Default::default(),
        list_next: 0,
        fence: None,
        fence_value: 0,
        timestamps: None,
        readback: None,
        ticks_to_ms: 0.0,
        last_serial: 0,
        last_origin: 0,
        last_frame: 0,
        snap_seen: 0,
        snap_left: 0,
        snap_threshold: 0.0,
        snap_found: false,
        last_forward: [0.0; 3],
        last_camera_frame: 0,
        last_camera_origin: 0,
        passes: None,
        layout: None,
        producer_fence: None,
        consumer_fence: None,
        producer_value: 0,
        consumer_value: 0,
        dispatch_logs: 0,
    })
});
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn log(text: &str) {
    cvr_host::log::write(text);
}

/// An SDK call `f(a, b)` inside cvr-engine's guarded range: its FfxErrorCode, or None when it faulted.
unsafe fn guarded(f: usize, a: *mut c_void, b: *const c_void) -> Option<i32> {
    cvr_engine::code::guarded_call4(f, a, b.cast_mut(), core::ptr::null_mut(), core::ptr::null_mut())
        .map(|r| r as usize as u32 as i32)
}

/// A render-target-less texture of the module's: UAV-capable, in `state`.
unsafe fn texture(device: &ID3D12Device, w: u64, h: u32, format: DXGI_FORMAT, state: D3D12_RESOURCE_STATES, name: PCWSTR) -> Option<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: w,
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut created: Option<ID3D12Resource> = None;
    device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut created).ok()?;
    let texture = created?;
    let _ = texture.SetName(name);
    Some(texture)
}

fn uav_barrier(list: &ID3D12GraphicsCommandList, resource: Option<&ID3D12Resource>) {
    let pointer = raw(resource);
    // SAFETY: the barrier borrows the resource's interface pointer for the call (no reference is taken or dropped).
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
unsafe fn srv(device: &ID3D12Device, resource: Option<&ID3D12Resource>, format: DXGI_FORMAT, at: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let d = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
    };
    device.CreateShaderResourceView(resource, Some(&d), at);
}
/// A UAV (a null one, of `format`, without a resource).
unsafe fn uav(device: &ID3D12Device, resource: Option<&ID3D12Resource>, format: DXGI_FORMAT, at: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let d = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV::default() },
    };
    device.CreateUnorderedAccessView(resource, None::<&ID3D12Resource>, Some(&d), at);
}

impl State {
    fn on(&self) -> bool {
        self.wanted.enabled && !self.faulted && !crate::afw::cvr_afw_launch()
    }
    fn text(&mut self, text: &'static CStr) {
        self.status.text = text;
    }

    unsafe fn destroy_contexts(&mut self) {
        for eye in 0..2 {
            if self.fi_valid[eye] {
                if let Some(c) = &mut self.fi[eye] {
                    ffx::ffxFrameInterpolationContextDestroy(c.ptr());
                }
            }
            if self.of_valid[eye] {
                if let Some(c) = &mut self.of[eye] {
                    ffx::ffxOpticalflowContextDestroy(c.ptr());
                }
            }
            self.fi_valid[eye] = false;
            self.of_valid[eye] = false;
        }
    }

    unsafe fn ensure_contexts(&mut self, device: &ID3D12Device, render: [u32; 2], display: [u32; 2], inverted: bool, infinite: bool) -> bool {
        if !self.backend_ready {
            // 2 eyes x (frame interpolation + optical flow)
            let size = ffx::ffxGetScratchMemorySizeDX12(4);
            self.scratch = vec![0u128; size.div_ceil(16)];
            if ffx::ffxGetInterfaceDX12(&mut self.backend, ffx::ffxGetDeviceDX12(device.as_raw()), self.scratch.as_mut_ptr().cast(), size, 4)
                != FFX_OK
            {
                self.scratch = Vec::new();
                log("[fsr3] ffxGetInterfaceDX12 failed\n");
                return false;
            }
            self.backend_ready = true;
        }
        if self.fi_valid[0]
            && self.fi_valid[1]
            && render[0] <= self.context_render[0]
            && render[1] <= self.context_render[1]
            && display[0] <= self.context_display[0]
            && display[1] <= self.context_display[1]
        {
            return true;
        }
        let render = [render[0].max(self.context_render[0]), render[1].max(self.context_render[1])];
        let display = [display[0].max(self.context_display[0]), display[1].max(self.context_display[1])];
        self.destroy_contexts();
        for eye in 0..2 {
            // The captures are the displayed SDR image (sRGB-encoded bytes), not scene-linear HDR.
            let mut desc = FfxFrameInterpolationContextDescription {
                flags: if inverted { FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED } else { 0 }
                    | if infinite { FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE } else { 0 },
                max_render_size: FfxDimensions2D { width: render[0], height: render[1] },
                display_size: FfxDimensions2D { width: display[0], height: display[1] },
                back_buffer_format: FFX_SURFACE_FORMAT_R8G8B8A8_UNORM,
                previous_interpolation_source_format: FFX_SURFACE_FORMAT_R8G8B8A8_UNORM,
                backend_interface: self.backend,
            };
            let fi = self.fi[eye].get_or_insert_with(Context::new).ptr();
            match guarded(ffx::ffxFrameInterpolationContextCreate as *const () as usize, fi, (&raw mut desc).cast()) {
                Some(FFX_OK) => self.fi_valid[eye] = true,
                Some(_) => {
                    log(&format!("[fsr3] frame interpolation context eye {eye} failed\n"));
                    self.destroy_contexts();
                    return false;
                }
                None => {
                    self.fault(c"FSR3 interpolation fault");
                    return false;
                }
            }
            let mut of = FfxOpticalflowContextDescription {
                backend_interface: self.backend,
                flags: 0,
                resolution: FfxDimensions2D { width: display[0], height: display[1] },
            };
            let ofp = self.of[eye].get_or_insert_with(Context::new).ptr();
            match guarded(ffx::ffxOpticalflowContextCreate as *const () as usize, ofp, (&raw mut of).cast()) {
                Some(code) => self.of_valid[eye] = code == FFX_OK,
                None => {
                    self.fault(c"FSR3 interpolation fault");
                    return false;
                }
            }
            if !self.of_valid[eye] {
                log(&format!("[fsr3] optical flow context eye {eye} failed: game motion vectors only\n"));
            }
            // No history yet: the first dispatch after a (re)create resets.
            self.first_dispatch[eye] = true;
        }
        self.context_render = render;
        self.context_display = display;
        log(&format!(
            "[fsr3] contexts (re)created: render {}x{} display {}x{}\n",
            render[0], render[1], display[0], display[1]
        ));
        true
    }

    /// Optical flow's two outputs, sized as ffxOpticalflowGetSharedResourceDescriptions documents: one vector per 8x8
    /// block of the display, and a fixed 3x1 scene change buffer.
    unsafe fn ensure_optical_flow(&mut self, device: &ID3D12Device, eye: usize, display_w: u32, display_h: u32) -> bool {
        let (w, h) = (display_w.div_ceil(8), display_h.div_ceil(8));
        let [have_w, have_h] = self.of_vector_size[eye];
        if self.of_vector[eye].is_none() || w > have_w || h > have_h {
            let (w, h) = (w.max(have_w), h.max(have_h));
            self.of_vector[eye] = texture(device, u64::from(w), h, DXGI_FORMAT_R16G16_SINT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, w!("CVR_FSR3_Flow"));
            if self.of_vector[eye].is_none() {
                return false;
            }
            self.of_vector_size[eye] = [w, h];
        }
        if self.of_scd[eye].is_none() {
            self.of_scd[eye] = texture(device, 3, 1, DXGI_FORMAT_R32_UINT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, w!("CVR_FSR3_SceneChange"));
        }
        self.of_scd[eye].is_some()
    }

    /// The pool's next slot of `eye`, for a frame of the captures' size. About to be overwritten: nothing may ask for what
    /// it held. The captures' format family, typeless as the second eye's captures are: the XR loop copies it into the
    /// swapchain image or reads it through its sRGB decode exactly like a capture.
    unsafe fn pool_slot(&mut self, device: &ID3D12Device, eye: usize, width: u64, height: u32) -> Option<ID3D12Resource> {
        let slot = &mut self.pool[eye][self.pool_next[eye]];
        slot.serial = 0;
        if let Some(t) = &slot.texture {
            let d = t.GetDesc();
            if d.Width == width && d.Height == height {
                return Some(t.clone());
            }
            *slot = Slot::default();
        }
        let t = texture(device, width, height, DXGI_FORMAT_R8G8B8A8_TYPELESS, D3D12_RESOURCE_STATE_COPY_SOURCE, w!("CVR_FSR3_Interpolated"))?;
        slot.texture = Some(t.clone());
        Some(t)
    }

    unsafe fn ensure_lists(&mut self, device: &ID3D12Device) -> bool {
        if self.fence.is_some() {
            return true;
        }
        if self.queue.is_none() {
            let q = D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_COMPUTE,
                Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0,
                ..Default::default()
            };
            let Ok(queue) = device.CreateCommandQueue::<ID3D12CommandQueue>(&q) else { return false };
            let _ = queue.SetName(w!("CVR_FSR3_Compute"));
            self.queue = Some(queue);
        }
        for l in &mut self.lists {
            let Ok(allocator) = device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_COMPUTE) else {
                return false;
            };
            let Ok(list) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_COMPUTE, &allocator, None)
            else {
                return false;
            };
            let _ = list.Close();
            l.allocator = Some(allocator);
            l.list = Some(list);
        }
        let qh = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: (LISTS * STAMPS) as u32, NodeMask: 0 };
        let rb = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let rd = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: (LISTS * STAMPS * 8) as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut heap: Option<ID3D12QueryHeap> = None;
        let mut readback: Option<ID3D12Resource> = None;
        if device.CreateQueryHeap(&qh, &mut heap).is_ok()
            && device.CreateCommittedResource(&rb, D3D12_HEAP_FLAG_NONE, &rd, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback).is_ok()
        {
            self.timestamps = heap;
            self.readback = readback;
        }
        match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
            Ok(f) => {
                self.fence = Some(f);
                true
            }
            Err(_) => false,
        }
    }

    unsafe fn ensure_passes(&mut self, device: &ID3D12Device) -> bool {
        if self.passes.is_some() {
            return true;
        }
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
                NumDescriptors: 5,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 2,
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
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            MaxLOD: D3D12_FLOAT32_MAX,
            ..Default::default()
        };
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 2,
            pParameters: params.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        };
        let (mut blob, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        if D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)).is_err() {
            log("[fsr3] root signature serialize failed\n");
            return false;
        }
        let Some(signature) = blob else { return false };
        let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(
            0,
            core::slice::from_raw_parts(signature.GetBufferPointer() as *const u8, signature.GetBufferSize()),
        ) else {
            log("[fsr3] CreateRootSignature failed\n");
            return false;
        };
        // main's HLSL (src/Framegen/Fsr3Dilate.cpp, Fsr3Extrapolate.cpp), or the passes made from Rust
        // (rust/shaders/passes/src/fsr3.rs) with CVR_SHADER_RUST=1
        let make = |dxil: &'static [u8]| -> Option<ID3D12PipelineState> {
            use crate::fsr3_shaders::{compile_cs, DILATE, WARP};
            let (source, entry) = if dxil.as_ptr() == cvr_gpu::dxil::FSR3_DILATE_CLEAR_CS.as_ptr() {
                (DILATE, "Clear")
            } else if dxil.as_ptr() == cvr_gpu::dxil::FSR3_DILATE_CS.as_ptr() {
                (DILATE, "ReconstructAndDilate")
            } else if dxil.as_ptr() == cvr_gpu::dxil::FSR3_DILATE_RESOLVE_CS.as_ptr() {
                (DILATE, "Resolve")
            } else {
                (WARP, "Warp")
            };
            let code = cvr_gpu::dxil::pick(dxil, || compile_cs(source, entry, "fsr3"))?;
            let p = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                CS: code.bytecode(),
                ..Default::default()
            };
            device.CreateComputePipelineState::<ID3D12PipelineState>(&p).ok()
        };
        let (Some(clear), Some(dilate), Some(resolve), Some(warp)) = (
            make(cvr_gpu::dxil::FSR3_DILATE_CLEAR_CS),
            make(cvr_gpu::dxil::FSR3_DILATE_CS),
            make(cvr_gpu::dxil::FSR3_DILATE_RESOLVE_CS),
            make(cvr_gpu::dxil::FSR3_WARP_CS),
        ) else {
            log("[fsr3] pipeline creation failed\n");
            return false;
        };
        let hd = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: (LISTS * TABLES_PER_LIST * TABLE) as u32,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        let Ok(heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&hd) else {
            log("[fsr3] descriptor heap creation failed\n");
            return false;
        };
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
        self.passes = Some(Passes { root, clear, dilate, resolve, warp, heap, step, eyes: Default::default() });
        log("[fsr3] passes ready (dilate and warp made from Rust, render size, both eyes)\n");
        true
    }

    /// The table `table` of the heap: its CPU and GPU handles.
    fn table(p: &Passes, table: usize) -> (D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_GPU_DESCRIPTOR_HANDLE) {
        // SAFETY: the passes' live heap.
        let (mut cpu, mut gpu) = unsafe { (p.heap.GetCPUDescriptorHandleForHeapStart(), p.heap.GetGPUDescriptorHandleForHeapStart()) };
        cpu.ptr += table * TABLE * p.step;
        gpu.ptr += (table * TABLE * p.step) as u64;
        (cpu, gpu)
    }

    /// FSR3-upscaler's "reconstruct dilated velocity and previous depth" for one eye (clear, dilate with the scatter,
    /// resolve), its outputs (re)made when the render size changed. The depth and motion rest readable; the outputs
    /// are left in UNORDERED_ACCESS.
    #[allow(clippy::too_many_arguments)]
    unsafe fn dilate(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        table: usize,
        eye: usize,
        i: &Inputs,
        scale: [f32; 2],
    ) -> bool {
        let (Some(depth), Some(motion)) = (i.depth.as_ref(), i.motion.as_ref()) else { return false };
        let Some(p) = self.passes.as_mut() else { return false };
        let (w, h) = (i.width, i.height);
        if w == 0 || h == 0 {
            return false;
        }
        let e = &mut p.eyes[eye];
        if e.width != w || e.height != h || e.depth.is_none() {
            let uav_state = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
            *e = Dilated {
                depth: texture(device, u64::from(w), h, DXGI_FORMAT_R32_FLOAT, uav_state, w!("CVR_FSR3_DilatedDepth")),
                motion: texture(device, u64::from(w), h, DXGI_FORMAT_R16G16_FLOAT, uav_state, w!("CVR_FSR3_DilatedMotion")),
                previous: texture(device, u64::from(w), h, DXGI_FORMAT_R32_FLOAT, uav_state, w!("CVR_FSR3_PreviousDepth")),
                scratch: texture(device, u64::from(w), h, DXGI_FORMAT_R32_UINT, uav_state, w!("CVR_FSR3_DepthScatter")),
                width: w,
                height: h,
            };
            if e.depth.is_none() || e.motion.is_none() || e.previous.is_none() || e.scratch.is_none() {
                log(&format!("[fsr3] eye {eye}: dilate outputs could not be made ({w}x{h})\n"));
                *e = Dilated::default();
                return false;
            }
        }
        let (cpu, gpu) = Self::table(p, table);
        let at = |n: usize| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu.ptr + n * p.step };
        srv(device, Some(depth), DXGI_FORMAT_R32_FLOAT, at(0));
        // R16G16 or R32G32 float, or RGBA16F (.xy)
        srv(device, Some(motion), motion.GetDesc().Format, at(1));
        let e = &p.eyes[eye];
        uav(device, e.depth.as_ref(), DXGI_FORMAT_R32_FLOAT, at(2));
        uav(device, e.motion.as_ref(), DXGI_FORMAT_R16G16_FLOAT, at(3));
        uav(device, e.scratch.as_ref(), DXGI_FORMAT_R32_UINT, at(4));
        uav(device, e.previous.as_ref(), DXGI_FORMAT_R32_FLOAT, at(5));
        // the scratch again at u4: the scatter's InterlockedMin for a depth that is not inverted
        uav(device, e.scratch.as_ref(), DXGI_FORMAT_R32_UINT, at(6));
        // b0: RenderSize, MvScale, InvertedDepth, ClearValue ("infinitely far")
        let inverted = i.camera.inverted;
        let k = [w, h, scale[0].to_bits(), scale[1].to_bits(), u32::from(inverted), if inverted { 0.0f32 } else { 1.0 }.to_bits()];
        list.SetDescriptorHeaps(&[Some(p.heap.clone())]);
        list.SetComputeRootSignature(&p.root);
        list.SetComputeRoot32BitConstants(0, 6, k.as_ptr().cast(), 0);
        list.SetComputeRootDescriptorTable(1, gpu);
        let (gx, gy) = (w.div_ceil(8), h.div_ceil(8));
        list.SetPipelineState(&p.clear);
        list.Dispatch(gx, gy, 1);
        // the clear lands before the scatter's interlocked maximum, the scatter before the resolve
        uav_barrier(list, e.scratch.as_ref());
        list.SetPipelineState(&p.dilate);
        list.Dispatch(gx, gy, 1);
        uav_barrier(list, e.scratch.as_ref());
        list.SetPipelineState(&p.resolve);
        list.Dispatch(gx, gy, 1);
        true
    }

    /// EXTRAPOLATION's pass for one eye: each output pixel takes the colour of the source pixel whose motion carries it
    /// there, `factor` of a frame ahead. The caller puts the colour and the motion in NON_PIXEL_SHADER_RESOURCE and the
    /// output in UNORDERED_ACCESS first, and back after.
    #[allow(clippy::too_many_arguments)]
    unsafe fn warp(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        table: usize,
        colour: &ID3D12Resource,
        motion: Option<&ID3D12Resource>,
        output: &ID3D12Resource,
        display: [u32; 2],
        render: [u32; 2],
        factor: f32,
    ) -> bool {
        let Some(p) = self.passes.as_ref() else { return false };
        if motion.is_none() || display[0] == 0 || display[1] == 0 {
            return false;
        }
        let (cpu, gpu) = Self::table(p, table);
        let at = |n: usize| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu.ptr + n * p.step };
        // the capture's sRGB-encoded bytes, untouched
        srv(device, Some(colour), DXGI_FORMAT_R8G8B8A8_UNORM, at(0));
        srv(device, motion, DXGI_FORMAT_R16G16_FLOAT, at(1));
        uav(device, Some(output), DXGI_FORMAT_R8G8B8A8_UNORM, at(2));
        for n in 3..TABLE {
            uav(device, None, DXGI_FORMAT_R32_FLOAT, at(n));
        }
        let k = [display[0], display[1], render[0], render[1], factor.to_bits(), 0];
        list.SetDescriptorHeaps(&[Some(p.heap.clone())]);
        list.SetComputeRootSignature(&p.root);
        list.SetComputeRoot32BitConstants(0, 6, k.as_ptr().cast(), 0);
        list.SetComputeRootDescriptorTable(1, gpu);
        list.SetPipelineState(&p.warp);
        list.Dispatch(display[0].div_ceil(8), display[1].div_ceil(8), 1);
        true
    }

    /// The list at `index` has left the GPU: its time into the report.
    unsafe fn read_timestamps(&mut self, index: usize) {
        if !self.lists[index].timed {
            return;
        }
        self.lists[index].timed = false;
        let (Some(readback), Some(queue)) = (self.readback.as_ref(), self.queue.as_ref()) else { return };
        if self.ticks_to_ms == 0.0 {
            match queue.GetTimestampFrequency() {
                Ok(f) if f != 0 => self.ticks_to_ms = 1000.0 / f as f64,
                _ => return,
            }
        }
        let range = D3D12_RANGE { Begin: index * STAMPS * 8, End: (index + 1) * STAMPS * 8 };
        let mut data = core::ptr::null_mut();
        if readback.Map(0, Some(&range), Some(&mut data)).is_err() || data.is_null() {
            return;
        }
        let t = core::slice::from_raw_parts((data as *const u64).add(index * STAMPS), STAMPS);
        if t[0] <= t[1] && t[1] <= t[2] && t[2] <= t[3] && t[3] <= t[4] && t[4] > t[0] {
            let ms = (t[4] - t[0]) as f64 * self.ticks_to_ms;
            self.generation_ms = if self.generation_ms > 0.0 { self.generation_ms * 0.9 + ms * 0.1 } else { ms };
            core_fg::cvr_framegen_on_generation(ms, ms);
        }
        readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
    }

    /// The engine camera's turn since the capture before, degrees; 0 with none to compare against (the first, another
    /// tracking space, or more than a few frames apart).
    fn camera_turn_deg(&mut self, i: &Inputs) -> f32 {
        let f = i.camera.forward;
        let length = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
        let gap = i.frame_id.wrapping_sub(self.last_camera_frame);
        let mut turn = 0.0;
        if length > 0.0 && self.last_camera_frame != 0 && i.origin == self.last_camera_origin && (1..=4).contains(&gap) {
            let l = self.last_forward;
            let dot = (f[0] * l[0] + f[1] * l[1] + f[2] * l[2]) / length;
            turn = dot.clamp(-1.0, 1.0).acos() * 57.29578;
        }
        if length > 0.0 {
            self.last_forward = [f[0] / length, f[1] / length, f[2] / length];
        }
        self.last_camera_frame = i.frame_id;
        self.last_camera_origin = i.origin;
        turn
    }
    /// A TURN THE MOTION DOES NOT CONTINUE. A snap turn moves the engine camera by the whole snap angle from one frame to
    /// the next and the engine does not mark it as a cut: the motion vectors carry it as motion, so extrapolation carries
    /// the view on past the snap for one frame and interpolation shows a frame halfway round. So after each snap turn
    /// handed to the game (cvr_host::snap_turn), a capture among the next few whose camera turned more than half the snap
    /// is treated as a cut. Only then: a fast head turn alone is never one, whatever its speed.
    fn camera_jumped(&mut self, i: &Inputs) -> bool {
        let turn = self.camera_turn_deg(i);
        let (count, degrees) = cvr_host::snap_turn::latest();
        if count != self.snap_seen {
            self.snap_seen = count;
            self.snap_left = SNAP_CAPTURES;
            self.snap_threshold = 0.5 * degrees.abs();
            self.snap_found = false;
        }
        if self.snap_left <= 0 {
            return false;
        }
        self.snap_left -= 1;
        if turn > self.snap_threshold {
            self.snap_found = true;
            if crate::framegen_presenter::diagnostics() {
                log(&format!(
                    "[fsr3] snap turn: camera turned {turn:.1} deg from the capture before (frame {}, capture {} of {SNAP_CAPTURES} after the snap): a cut, nothing generated\n",
                    i.frame_id,
                    SNAP_CAPTURES - self.snap_left
                ));
            }
            return true;
        }
        if self.snap_left == 0 && !self.snap_found && crate::framegen_presenter::diagnostics() {
            log(&format!(
                "[fsr3] snap turn of {:.0} deg: no capture among the {SNAP_CAPTURES} after it turned more than {:.1} deg\n",
                2.0 * self.snap_threshold,
                self.snap_threshold
            ));
        }
        false
    }

    /// The next list, once the GPU is done with it (its timestamps read, the inputs it is to read held, reset for
    /// recording): its index and the list, or None (counted as skipped, or a fault).
    unsafe fn begin_list(&mut self, inputs: [&InputsRef; 2]) -> Option<(usize, ID3D12GraphicsCommandList)> {
        let index = self.list_next;
        self.list_next = (self.list_next + 1) % LISTS;
        let completed = self.fence.as_ref().map_or(0, |f| f.GetCompletedValue());
        if self.lists[index].fence != 0 && completed < self.lists[index].fence {
            self.status.skipped += 1;
            self.last_serial = 0;
            return None;
        }
        self.read_timestamps(index);
        let l = &mut self.lists[index];
        l.inputs = [Some(inputs[0].clone()), Some(inputs[1].clone())];
        let (Some(allocator), Some(list)) = (l.allocator.clone(), l.list.clone()) else { return None };
        if allocator.Reset().is_err() || list.Reset(&allocator, None).is_err() {
            self.faulted = true;
            self.text(c"FSR3 command list failed");
            return None;
        }
        Some((index, list))
    }
    unsafe fn stamp(&self, list: &ID3D12GraphicsCommandList, index: usize, at: usize) {
        if let Some(t) = &self.timestamps {
            list.EndQuery(t, D3D12_QUERY_TYPE_TIMESTAMP, (index * STAMPS + at) as u32);
        }
    }
    /// The list closed (its timestamps resolved when all were written) and submitted behind the producer's and the
    /// consumer's fences and the inputs' own; its fence signalled. The barriers recorded leave every resource as it was
    /// found, so even a failed eye is submitted.
    unsafe fn submit(&mut self, index: usize, list: &ID3D12GraphicsCommandList, ok: bool, waits: &[(Option<ID3D12Fence>, u64); 2], inputs: [&InputsRef; 2]) {
        let timed = self.timestamps.is_some() && ok;
        if let (true, Some(t), Some(rb)) = (timed, &self.timestamps, &self.readback) {
            list.ResolveQueryData(t, D3D12_QUERY_TYPE_TIMESTAMP, (index * STAMPS) as u32, STAMPS as u32, rb, (index * STAMPS * 8) as u64);
        }
        self.lists[index].timed = timed;
        let _ = list.Close();
        let (Some(queue), Some(fence)) = (self.queue.clone(), self.fence.clone()) else { return };
        for (f, value) in waits {
            if let (Some(f), true) = (f, *value != 0) {
                let _ = queue.Wait(f, *value);
            }
        }
        for i in inputs {
            let i = i.get();
            if let (Some(f), true) = (&i.ready_fence, i.fence_value != 0) {
                let _ = queue.Wait(f, i.fence_value);
            }
        }
        if let Ok(l) = list.cast::<ID3D12CommandList>() {
            queue.ExecuteCommandLists(&[Some(l)]);
        }
        self.fence_value += 1;
        self.lists[index].fence = self.fence_value;
        let _ = queue.Signal(&fence, self.fence_value);
    }
    /// An SDK fault: FSR3 off until it is switched on again; the list being recorded closed unsubmitted.
    fn fault(&mut self, text: &'static CStr) {
        self.faulted = true;
        self.text(text);
        log(&format!("[fsr3] {}\n", text.to_string_lossy()));
    }
    /// The generated frames of `serial`, both eyes, published.
    fn publish(&mut self, serial: u64) {
        for eye in 0..2 {
            self.pool[eye][self.pool_next[eye]].serial = serial;
            self.pool_next[eye] = (self.pool_next[eye] + 1) % POOL_SLOTS;
        }
    }

    /// INTERPOLATION: the frame between the previous capture and this one, per eye; a capture that does not follow the
    /// previous one in the same tracking space only primes the history (reset) and shows nothing in between.
    #[allow(clippy::too_many_arguments)]
    unsafe fn interpolate(
        &mut self,
        device: &ID3D12Device,
        waits: &[(Option<ID3D12Fence>, u64); 2],
        colors: [&ID3D12Resource; 2],
        inputs: [&InputsRef; 2],
        serial: u64,
        frame_ms: f64,
    ) {
        for r in inputs {
            let i = r.get();
            if !i.has_depth || !i.has_motion || i.depth.is_none() || i.motion.is_none() {
                self.last_serial = 0;
                self.status.skipped += 1;
                self.text(c"Waiting for depth and motion vectors in both eyes");
                return;
            }
            if i.motion_width != i.width || i.motion_height != i.height {
                self.last_serial = 0;
                self.status.skipped += 1;
                self.text(c"Motion vectors are not at the render resolution: not supported");
                return;
            }
        }
        let (a, b) = (inputs[0].get(), inputs[1].get());
        let color = colors[0].GetDesc();
        if !self.ensure_lists(device)
            || !self.ensure_contexts(device, [a.width, a.height], [color.Width as u32, color.Height], a.camera.inverted, a.camera.infinite)
            || !self.ensure_passes(device)
        {
            if !self.faulted {
                self.faulted = true;
                self.text(c"FSR3 could not start: see cyberpunkvrport.log");
            }
            return;
        }
        self.status.ready = true;
        let previous = self.last_serial;
        let jumped = self.camera_jumped(a);
        let follows = !jumped
            && previous != 0
            && serial == previous + 1
            && a.origin == self.last_origin
            && a.frame_id == self.last_frame.wrapping_add(1)
            && b.frame_id == a.frame_id
            && !a.camera.reset
            && !b.camera.reset
            && frame_ms > 0.0
            && frame_ms <= 100.0;
        self.last_serial = serial;
        self.last_origin = a.origin;
        self.last_frame = a.frame_id;
        let Some((index, list)) = self.begin_list(inputs) else { return };
        // Always, not only for the report: generation_ms decides whether an interpolated frame fits.
        self.stamp(&list, index, 0);
        let flow = self.wanted.optical_flow;
        let mut ok = true;
        for eye in 0..2 {
            let i = inputs[eye].get();
            let desc = colors[eye].GetDesc();
            let (dw, dh) = (desc.Width as u32, desc.Height);
            let Some(output) = self.pool_slot(device, eye, desc.Width, desc.Height) else {
                ok = false;
                break;
            };
            let mut reset = !follows;
            if self.first_dispatch[eye] {
                reset = true;
                self.first_dispatch[eye] = false;
            }
            // The engine's vectors are UV offsets to the previous frame once its own scale is applied.
            let scale = i.camera.motion_scale.map(|s| if s != 0.0 { s } else { 1.0 });
            if !self.dilate(device, &list, index * TABLES_PER_LIST + eye * 2, eye, i, scale) {
                ok = false;
                break;
            }
            self.stamp(&list, index, 1 + eye * 2);
            barrier(&list, Some(colors[eye]), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            barrier(&list, Some(&output), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
            let command_list = ffx::ffxGetCommandListDX12(list.as_raw());
            let unorm = DXGI_FORMAT_R8G8B8A8_UNORM;
            // Optical flow first, from the same colour, as AMD's own FSR3 orchestration does.
            let mut have_flow = false;
            if flow && self.of_valid[eye] && self.ensure_optical_flow(device, eye, dw, dh) {
                let of = FfxOpticalflowDispatchDescription {
                    command_list,
                    color: wrap(colors[eye].as_raw(), FFX_RESOURCE_STATE_COMPUTE_READ, unorm),
                    optical_flow_vector: wrap(raw(self.of_vector[eye].as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN),
                    optical_flow_scd: wrap(raw(self.of_scd[eye].as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN),
                    reset,
                    backbuffer_transfer_function: FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB,
                    min_max_luminance: FfxFloatCoords2D { x: 0.0, y: 1.0 },
                };
                let context = self.of[eye].as_mut().map_or(core::ptr::null_mut(), Context::ptr);
                match guarded(ffx::ffxOpticalflowContextDispatch as *const () as usize, context, (&raw const of).cast()) {
                    Some(code) => have_flow = code == FFX_OK,
                    None => {
                        let _ = list.Close();
                        self.fault(c"FSR3 interpolation fault");
                        return;
                    }
                }
            }
            let Some(p) = self.passes.as_ref() else { return };
            let e = &p.eyes[eye];
            // SAFETY: plain data; all zeros is the C++ `{}`.
            let mut d: FfxFrameInterpolationDispatchDescription = core::mem::zeroed();
            d.command_list = command_list;
            d.display_size = FfxDimensions2D { width: dw, height: dh };
            d.render_size = FfxDimensions2D { width: i.width, height: i.height };
            d.current_back_buffer = wrap(colors[eye].as_raw(), FFX_RESOURCE_STATE_COMPUTE_READ, unorm);
            d.output = wrap(output.as_raw(), FFX_RESOURCE_STATE_UNORDERED_ACCESS, unorm);
            d.dilated_depth = wrap(raw(e.depth.as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN);
            d.dilated_motion_vectors = wrap(raw(e.motion.as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN);
            d.reconstructed_prev_depth = wrap(raw(e.previous.as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN);
            d.interpolation_rect = FfxRect2D { left: 0, top: 0, width: dw as i32, height: dh as i32 };
            if have_flow {
                d.optical_flow_vector = wrap(raw(self.of_vector[eye].as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN);
                d.optical_flow_scene_change_detection =
                    wrap(raw(self.of_scd[eye].as_ref()), FFX_RESOURCE_STATE_UNORDERED_ACCESS, DXGI_FORMAT_UNKNOWN);
                d.optical_flow_buffer_size = FfxDimensions2D { width: dw.div_ceil(8), height: dh.div_ceil(8) };
                d.optical_flow_scale = FfxFloatCoords2D { x: 1.0 / dw as f32, y: 1.0 / dh as f32 };
            }
            d.optical_flow_block_size = 8;
            d.camera_near = i.camera.near_plane;
            d.camera_far = i.camera.far_plane;
            d.camera_fov_angle_vertical = i.camera.vertical_fov;
            d.view_space_to_meters_factor = 1.0;
            d.frame_id = serial;
            d.frame_time_delta = frame_ms as f32;
            d.reset = reset;
            d.back_buffer_transfer_function = FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;
            d.min_max_luminance = [0.0, 1.0];
            let context = self.fi[eye].as_mut().map_or(core::ptr::null_mut(), Context::ptr);
            let result = match guarded(ffx::ffxFrameInterpolationDispatch as *const () as usize, context, (&raw const d).cast()) {
                Some(code) => code,
                None => {
                    let _ = list.Close();
                    self.fault(c"FSR3 interpolation fault");
                    return;
                }
            };
            barrier(&list, Some(&output), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
            barrier(&list, Some(colors[eye]), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
            self.stamp(&list, index, 2 + eye * 2);
            if result != FFX_OK {
                if self.dispatch_logs < 8 {
                    self.dispatch_logs += 1;
                    log(&format!("[fsr3] dispatch eye {eye} failed 0x{:08X}\n", result as u32));
                }
                ok = false;
                break;
            }
        }
        self.submit(index, &list, ok, waits, inputs);
        if !ok {
            self.last_serial = 0;
            self.status.skipped += 1;
            self.text(c"FSR3 dispatch failed: see cyberpunkvrport.log");
            return;
        }
        if !follows {
            self.status.primed += 1;
            self.text(c"Priming: waiting for two consecutive frames");
            return;
        }
        self.publish(serial);
        self.status.interpolated += 1;
        self.text(if flow {
            c"Interpolating: engine motion vectors and depth, optical flow"
        } else {
            c"Interpolating: engine motion vectors and depth"
        });
    }

    /// EXTRAPOLATION: the last real frame moved half a game frame ahead along the engine's own motion, per eye.
    unsafe fn extrapolate(
        &mut self,
        device: &ID3D12Device,
        waits: &[(Option<ID3D12Fence>, u64); 2],
        colors: [&ID3D12Resource; 2],
        inputs: [&InputsRef; 2],
        serial: u64,
    ) -> bool {
        for r in inputs {
            let i = r.get();
            if !i.has_motion || i.motion.is_none() || i.depth.is_none() {
                self.status.skipped += 1;
                self.text(c"Waiting for depth and motion vectors in both eyes");
                return false;
            }
            if i.motion_width != i.width || i.motion_height != i.height {
                self.status.skipped += 1;
                self.text(c"Motion vectors are not at the render resolution: not supported");
                return false;
            }
            // A cut: the vectors lead from a picture that is not the one before. Nothing to move ahead.
            if i.camera.reset {
                return false;
            }
        }
        if self.camera_jumped(inputs[0].get()) {
            return false;
        }
        if !self.ensure_lists(device) || !self.ensure_passes(device) {
            self.faulted = true;
            self.text(c"FSR3 could not start: see cyberpunkvrport.log");
            return false;
        }
        self.status.ready = true;
        let Some((index, list)) = self.begin_list(inputs) else { return false };
        self.stamp(&list, index, 0);
        let mut ok = true;
        for eye in 0..2 {
            let i = inputs[eye].get();
            let desc = colors[eye].GetDesc();
            let Some(output) = self.pool_slot(device, eye, desc.Width, desc.Height) else {
                ok = false;
                break;
            };
            let scale = i.camera.motion_scale.map(|s| if s != 0.0 { s } else { 1.0 });
            if !self.dilate(device, &list, index * TABLES_PER_LIST + eye * 2, eye, i, scale) {
                ok = false;
                break;
            }
            self.stamp(&list, index, 1 + eye * 2);
            let motion = self.passes.as_ref().and_then(|p| p.eyes[eye].motion.clone());
            let (read, uav_state, copy) =
                (D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
            barrier(&list, Some(colors[eye]), copy, read);
            barrier(&list, motion.as_ref(), uav_state, read);
            barrier(&list, Some(&output), copy, uav_state);
            ok = self.warp(
                device,
                &list,
                index * TABLES_PER_LIST + eye * 2 + 1,
                colors[eye],
                motion.as_ref(),
                &output,
                [desc.Width as u32, desc.Height],
                [i.width, i.height],
                EXTRAPOLATION_FACTOR,
            );
            barrier(&list, Some(&output), uav_state, copy);
            barrier(&list, motion.as_ref(), read, uav_state);
            barrier(&list, Some(colors[eye]), read, copy);
            self.stamp(&list, index, 2 + eye * 2);
            if !ok {
                break;
            }
        }
        self.submit(index, &list, ok, waits, inputs);
        if !ok {
            self.status.skipped += 1;
            self.text(c"FSR3 dispatch failed: see cyberpunkvrport.log");
            return false;
        }
        self.publish(serial);
        self.status.extrapolated += 1;
        self.text(c"Extrapolating: engine motion vectors, half a game frame ahead");
        true
    }

    fn poll(&mut self) {
        let Some(fence) = &self.fence else { return };
        // SAFETY: the module's live fence.
        let completed = unsafe { fence.GetCompletedValue() };
        for l in &mut self.lists {
            if l.fence <= completed {
                l.lease = None;
                l.inputs = [None, None];
            }
        }
    }
    fn reset_history(&mut self) {
        self.last_serial = 0;
        self.last_camera_frame = 0;
        for eye in &mut self.pool {
            for slot in eye {
                slot.serial = 0;
            }
        }
    }
    /// Everything retired: false while the compute work or the last headset read is still on the GPU.
    fn release_resources(&mut self) -> bool {
        if self.layout.is_none() {
            return true;
        }
        self.poll();
        // SAFETY: the module's live fences.
        let busy = |f: &Option<ID3D12Fence>, value: u64| f.as_ref().is_some_and(|f| unsafe { f.GetCompletedValue() } < value);
        if busy(&self.fence, self.fence_value) || busy(&self.consumer_fence, self.consumer_value) {
            return false;
        }
        // SAFETY: the GPU is done with every context and resource.
        unsafe { self.destroy_contexts() };
        self.passes = None;
        self.reset_history();
        self.pool = Default::default();
        self.lists = Default::default();
        self.of_vector = [None, None];
        self.of_scd = [None, None];
        self.of_vector_size = [[0; 2]; 2];
        self.pool_next = [0; 2];
        self.scratch = Vec::new();
        self.backend = FfxInterface([0; 29]);
        self.backend_ready = false;
        self.context_render = [0; 2];
        self.context_display = [0; 2];
        self.timestamps = None;
        self.readback = None;
        self.queue = None;
        self.fence = None;
        self.producer_fence = None;
        self.consumer_fence = None;
        self.list_next = 0;
        self.fence_value = 0;
        self.producer_value = 0;
        self.consumer_value = 0;
        self.layout = None;
        self.ticks_to_ms = 0.0;
        self.generation_ms = 0.0;
        self.status.ready = false;
        true
    }
    fn output(&self, eye: usize, serial: u64) -> *mut c_void {
        if eye > 1 || serial == 0 {
            return core::ptr::null_mut();
        }
        self.pool[eye].iter().find(|s| s.serial == serial).map_or(core::ptr::null_mut(), |s| raw(s.texture.as_ref()))
    }
}

/// cvr::fsr3::SetSettings, from the frame generator's settings (read each presentation cycle): on with FSR3 as the
/// backend; switched on again, a fault is forgotten; a mode change primes interpolation again (its history is not kept
/// up while extrapolating).
pub(crate) fn sync(settings: &Settings) {
    let wanted = Wanted {
        enabled: settings.enabled && is_backend(settings.backend),
        extrapolation: settings.backend == EXTRAPOLATION,
        optical_flow: settings.fsr3_optical_flow(),
        show: settings.fsr3_show(),
    };
    let mut s = state();
    if wanted == s.wanted {
        return;
    }
    let before = s.wanted;
    if wanted.enabled && !before.enabled {
        s.faulted = false;
        // snaps handed over while off are not looked for
        s.snap_seen = cvr_host::snap_turn::latest().0;
        s.snap_left = 0;
    }
    if wanted.enabled != before.enabled {
        log(&format!("[fsr3] frame generation {}\n", if wanted.enabled { "on" } else { "off" }));
    }
    if wanted.extrapolation != before.extrapolation {
        log(&format!("[fsr3] mode {}\n", if wanted.extrapolation { "extrapolation" } else { "interpolation" }));
        s.last_serial = 0;
    }
    s.wanted = wanted;
    if !wanted.enabled {
        s.last_serial = 0;
        s.text(c"Off");
    }
}
/// cvr::fsr3::Enabled: switched on and not stopped by a fault. Off for an AFW launch (afw.rs): its frames are drawn for
/// alternating eyes, and generating between two of them would blend pictures of different eyes.
pub fn enabled() -> bool {
    state().on()
}
/// cvr::fsr3::ShowGenerated.
pub fn show_generated() -> bool {
    let s = state();
    s.on() && s.wanted.show
}
/// cvr::fsr3::GenerationMs: the generation's GPU time, both eyes, smoothed (0 until measured).
pub fn generation_ms() -> f64 {
    state().generation_ms
}
/// cvr::fsr3::GetStatus.
pub fn status() -> Status {
    let s = state();
    let mut result = s.status;
    result.faulted = s.faulted;
    if s.faulted {
        result.text = c"Stopped by a fault: see cyberpunkvrport.log";
    } else if !s.wanted.enabled {
        result.text = c"Off";
    }
    result
}
/// cvr::fsr3::Poll: the lists the GPU is done with let go of their inputs and capture leases.
pub fn poll() {
    state().poll();
}
/// cvr::fsr3::ResetHistory.
pub fn reset_history() {
    state().reset_history();
}
/// cvr::fsr3::ReleaseResources: everything retired once the GPU is done with it (false while it is not).
pub fn release_resources() -> bool {
    state().release_resources()
}
/// cvr::fsr3::ConsumerFence (the presentation owner's): a format or size change retires both the compute writes and the
/// last headset read before any SDK context, descriptor or output is recycled.
pub fn consumer_fence(fence: *mut c_void, value: u64) {
    let mut s = state();
    if s.layout.is_none() || value == 0 {
        return;
    }
    // SAFETY: the caller's live fence or null.
    if let Some(f) = unsafe { ID3D12Fence::from_raw_borrowed(&fence) } {
        s.consumer_fence = Some(f.clone());
        s.consumer_value = value;
    }
}
/// cvr::fsr3::QueueWaitForLast: `queue` waits on the GPU for the last generation -- its frames, and its reads of the
/// capture.
pub fn queue_wait_for_last(queue: *mut c_void) {
    let s = state();
    // SAFETY: the caller's live queue or null.
    if let (Some(queue), Some(fence), true) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }, &s.fence, s.fence_value != 0) {
        // SAFETY: live objects.
        let _ = unsafe { queue.Wait(fence, s.fence_value) };
    }
}
/// cvr::fsr3::Output: the frame generated for `serial`'s capture -- in extrapolation, the one extrapolated from it -- per
/// eye (resting in COPY_SOURCE, the capture's own size and format), or null when there is none.
pub fn output(eye: usize, serial: u64) -> *mut c_void {
    state().output(eye, serial)
}

/// cvr::fsr3::Generate: the pair's frame generated on the module's compute queue (behind the producer queue's capture
/// writes and the last headset copy), the capture lease held by the list until the GPU is done with it -> both eyes'
/// outputs of `serial` exist.
pub(crate) fn generate(
    device: *mut c_void,
    producer: *mut c_void,
    colors: [usize; 2],
    inputs: &[Option<InputsRef>; 2],
    serial: u64,
    frame_ms: f64,
    lease: Option<Lease>,
) -> bool {
    let mut s = state();
    let (Some(in0), Some(in1)) = (&inputs[0], &inputs[1]) else { return false };
    let c0 = colors[0] as *mut c_void;
    let c1 = colors[1] as *mut c_void;
    // SAFETY: the caller's live device, queue and pictures, or null.
    let (Some(device), Some(producer), Some(c0), Some(c1)) = (unsafe {
        (
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12CommandQueue::from_raw_borrowed(&producer),
            ID3D12Resource::from_raw_borrowed(&c0),
            ID3D12Resource::from_raw_borrowed(&c1),
        )
    }) else {
        return false;
    };
    if !s.on() {
        return false;
    }
    // SAFETY (whole body): live objects; the GPU work is ordered by the fences below.
    unsafe {
        let (color, right) = (c0.GetDesc(), c1.GetDesc());
        // MAIN can be UNORM while the secondary capture is TYPELESS. Both use the same UNORM view.
        let rgba8 = |f: DXGI_FORMAT| f == DXGI_FORMAT_R8G8B8A8_TYPELESS || f == DXGI_FORMAT_R8G8B8A8_UNORM;
        if !rgba8(color.Format) || !rgba8(right.Format) {
            s.text(c"FSR3 needs RGBA8 eye captures");
            return false;
        }
        let (a, b) = (in0.get(), in1.get());
        let requested = Layout {
            device: device.as_raw() as usize,
            width: color.Width,
            height: color.Height,
            render_width: a.width,
            render_height: a.height,
            inverted: a.camera.inverted,
            infinite: a.camera.infinite,
        };
        if right.Width != color.Width || right.Height != color.Height || b.width != a.width || b.height != a.height {
            s.text(c"Waiting for matching stereo input dimensions");
            return false;
        }
        if s.layout.is_some_and(|l| l != requested) && !s.release_resources() {
            return false;
        }
        s.layout = Some(requested);
        if s.producer_fence.is_none() {
            match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                Ok(f) => s.producer_fence = Some(f),
                Err(_) => return false,
            }
        }
        s.producer_value += 1;
        let Some(pf) = s.producer_fence.clone() else { return false };
        if producer.Signal(&pf, s.producer_value).is_err() {
            return false;
        }
        let waits = [(Some(pf), s.producer_value), (s.consumer_fence.clone(), s.consumer_value)];
        let index = s.list_next;
        let before = s.fence_value;
        if s.wanted.extrapolation {
            s.extrapolate(device, &waits, [c0, c1], [in0, in1], serial);
        } else {
            s.interpolate(device, &waits, [c0, c1], [in0, in1], serial, frame_ms);
        }
        if s.fence_value > before {
            s.lists[index].lease = lease;
        }
        !s.output(0, serial).is_null() && !s.output(1, serial).is_null()
    }
}
