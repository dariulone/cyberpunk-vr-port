//! THE FRAME GENERATOR (formerly src/Framegen/Generator.cpp): one interpolated frame between two real ones, per eye,
//! from the port's own colour snapshots and the engine's depth and motion vectors.
//!
//! FidelityFX's frame interpolation does the work in every mode but DLSS-G. Its optical flow is FidelityFX's own, or
//! -- the NVIDIA setting, the default -- NVIDIA's hardware optical flow engine (OFA, nvofapi64.dll), which runs between
//! two command lists: the colour packing first, the flow resolve and the interpolation after. DLSS-G (one NGX feature
//! per eye) stays C++ for now (src/Framegen/DlssFrameGen.cpp) and is called through its adapters. Three work slots
//! keep the pairs in flight; each holds the inputs it read (framegen_inputs' records) until its fence passes.
//!
//! The FidelityFX and NVIDIA optical flow structures are mirrored here at the pinned SDKs' layouts
//! (cmake/NativeFramegen.cmake, externals/nvidia-optical-flow); src/Framegen/Generator.cpp asserts the same numbers
//! against the headers. The C++ class keeps its face (include/Framegen/Generator.hpp) over this module's handle.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::framegen_inputs::{Inputs, Record};
use core::ffi::c_void;
use core::mem::{offset_of, size_of, ManuallyDrop};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering::*};
use cvr_gpu::frame_capture::{self, CameraData, CaptureEye, EyeCamera};
use cvr_gpu::framegen_core::{self, Settings};
use std::sync::Arc;
use windows::core::{w, Interface, PCSTR, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

const FIDELITY_FX: i32 = 0;
const NVIDIA: i32 = 1;
const DLSS_G: i32 = 2;
/// ANR FG (Apex Neural Rendering): a DLSS-G provider dll run through the DLSS-G path (anr_fg.rs, main's 0.2.4).
const ANR: i32 = 3;
/// The backends NGX's DLSS-G helpers run (NgxBackend).
fn ngx_backend(backend: i32) -> bool {
    backend == DLSS_G || backend == ANR
}
fn provider_of(backend: i32) -> u8 {
    if backend == ANR {
        crate::dlssg_native::PROVIDER_ANR
    } else {
        crate::dlssg_native::PROVIDER_NVIDIA
    }
}

const READ: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
const UAV: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
const COPY: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_SOURCE;

// ---- FidelityFX (ffx_types.h, ffx_interface.h, ffx_frameinterpolation.h, ffx_opticalflow.h, ffx_dx12.h) ----

/// FfxInterface: the backend's function table and its scratch, copied by value into each context's description.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub(crate) struct FfxInterface(pub(crate) [u64; 29]);
pub(crate) const FFX_CONTEXT_WORDS: usize = 1024 * 128;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FfxDimensions2D {
    pub(crate) width: u32,
    pub(crate) height: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FfxFloatCoords2D {
    pub(crate) x: f32,
    pub(crate) y: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FfxRect2D {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FfxResourceDescription {
    pub(crate) ty: i32,
    pub(crate) format: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth: u32,
    pub(crate) mip_count: u32,
    pub(crate) flags: i32,
    pub(crate) usage: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct FfxResource {
    pub(crate) resource: *mut c_void,
    pub(crate) description: FfxResourceDescription,
    pub(crate) state: i32,
    pub(crate) name: [u16; 64],
}
impl Default for FfxResource {
    fn default() -> Self {
        // SAFETY: plain data; all zeros is the C++ `{}`.
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
pub(crate) struct FfxFrameInterpolationContextDescription {
    pub(crate) flags: u32,
    pub(crate) max_render_size: FfxDimensions2D,
    pub(crate) display_size: FfxDimensions2D,
    pub(crate) back_buffer_format: i32,
    pub(crate) previous_interpolation_source_format: i32,
    pub(crate) backend_interface: FfxInterface,
}
#[repr(C)]
pub(crate) struct FfxFrameInterpolationPrepareDescription {
    pub(crate) flags: u32,
    pub(crate) command_list: *mut c_void,
    pub(crate) render_size: FfxDimensions2D,
    pub(crate) jitter_offset: FfxFloatCoords2D,
    pub(crate) motion_vector_scale: FfxFloatCoords2D,
    pub(crate) frame_time_delta: f32,
    pub(crate) camera_near: f32,
    pub(crate) camera_far: f32,
    pub(crate) view_space_to_meters_factor: f32,
    pub(crate) camera_fov_angle_vertical: f32,
    pub(crate) depth: FfxResource,
    pub(crate) motion_vectors: FfxResource,
    pub(crate) frame_id: u64,
    pub(crate) dilated_depth: FfxResource,
    pub(crate) dilated_motion_vectors: FfxResource,
    pub(crate) reconstructed_prev_depth: FfxResource,
    pub(crate) camera_position: [f32; 3],
    pub(crate) camera_up: [f32; 3],
    pub(crate) camera_right: [f32; 3],
    pub(crate) camera_forward: [f32; 3],
}
#[repr(C)]
pub(crate) struct FfxFrameInterpolationDispatchDescription {
    pub(crate) flags: u32,
    pub(crate) command_list: *mut c_void,
    pub(crate) display_size: FfxDimensions2D,
    pub(crate) render_size: FfxDimensions2D,
    pub(crate) current_back_buffer: FfxResource,
    pub(crate) current_back_buffer_hudless: FfxResource,
    pub(crate) output: FfxResource,
    pub(crate) interpolation_rect: FfxRect2D,
    pub(crate) optical_flow_vector: FfxResource,
    pub(crate) optical_flow_scene_change_detection: FfxResource,
    pub(crate) optical_flow_buffer_size: FfxDimensions2D,
    pub(crate) optical_flow_scale: FfxFloatCoords2D,
    pub(crate) optical_flow_block_size: i32,
    pub(crate) camera_near: f32,
    pub(crate) camera_far: f32,
    pub(crate) camera_fov_angle_vertical: f32,
    pub(crate) view_space_to_meters_factor: f32,
    pub(crate) frame_time_delta: f32,
    pub(crate) reset: bool,
    pub(crate) back_buffer_transfer_function: i32,
    pub(crate) min_max_luminance: [f32; 2],
    pub(crate) frame_id: u64,
    pub(crate) dilated_depth: FfxResource,
    pub(crate) dilated_motion_vectors: FfxResource,
    pub(crate) reconstructed_prev_depth: FfxResource,
    pub(crate) distortion_field: FfxResource,
}
#[repr(C)]
pub(crate) struct FfxOpticalflowContextDescription {
    pub(crate) backend_interface: FfxInterface,
    pub(crate) flags: u32,
    pub(crate) resolution: FfxDimensions2D,
}
#[repr(C)]
pub(crate) struct FfxOpticalflowDispatchDescription {
    pub(crate) command_list: *mut c_void,
    pub(crate) color: FfxResource,
    pub(crate) optical_flow_vector: FfxResource,
    pub(crate) optical_flow_scd: FfxResource,
    pub(crate) reset: bool,
    pub(crate) backbuffer_transfer_function: i32,
    pub(crate) min_max_luminance: FfxFloatCoords2D,
}
#[repr(C)]
#[derive(Default)]
pub(crate) struct FfxEffectMemoryUsage {
    pub(crate) total_usage_in_bytes: u64,
    pub(crate) aliasable_usage_in_bytes: u64,
}
const _: () = assert!(
    size_of::<FfxInterface>() == 232
        && size_of::<FfxResourceDescription>() == 32
        && size_of::<FfxResource>() == 176
        && offset_of!(FfxResource, state) == 40
        && offset_of!(FfxResource, name) == 44
        && size_of::<FfxFrameInterpolationContextDescription>() == 264
        && offset_of!(FfxFrameInterpolationContextDescription, back_buffer_format) == 20
        && offset_of!(FfxFrameInterpolationContextDescription, backend_interface) == 32
        && size_of::<FfxFrameInterpolationPrepareDescription>() == 1000
        && offset_of!(FfxFrameInterpolationPrepareDescription, command_list) == 8
        && offset_of!(
            FfxFrameInterpolationPrepareDescription,
            camera_fov_angle_vertical
        ) == 56
        && offset_of!(FfxFrameInterpolationPrepareDescription, depth) == 64
        && offset_of!(FfxFrameInterpolationPrepareDescription, frame_id) == 416
        && offset_of!(
            FfxFrameInterpolationPrepareDescription,
            reconstructed_prev_depth
        ) == 776
        && offset_of!(FfxFrameInterpolationPrepareDescription, camera_position) == 952
        && offset_of!(FfxFrameInterpolationPrepareDescription, camera_forward) == 988
        && size_of::<FfxFrameInterpolationDispatchDescription>() == 1696
        && offset_of!(
            FfxFrameInterpolationDispatchDescription,
            current_back_buffer
        ) == 32
        && offset_of!(FfxFrameInterpolationDispatchDescription, interpolation_rect) == 560
        && offset_of!(
            FfxFrameInterpolationDispatchDescription,
            optical_flow_buffer_size
        ) == 928
        && offset_of!(
            FfxFrameInterpolationDispatchDescription,
            optical_flow_block_size
        ) == 944
        && offset_of!(FfxFrameInterpolationDispatchDescription, reset) == 968
        && offset_of!(
            FfxFrameInterpolationDispatchDescription,
            back_buffer_transfer_function
        ) == 972
        && offset_of!(FfxFrameInterpolationDispatchDescription, frame_id) == 984
        && offset_of!(FfxFrameInterpolationDispatchDescription, distortion_field) == 1520
        && size_of::<FfxOpticalflowContextDescription>() == 248
        && offset_of!(FfxOpticalflowContextDescription, flags) == 232
        && size_of::<FfxOpticalflowDispatchDescription>() == 552
        && offset_of!(FfxOpticalflowDispatchDescription, reset) == 536
        && offset_of!(FfxOpticalflowDispatchDescription, min_max_luminance) == 544
);
pub(crate) const FFX_OK: i32 = 0;
pub(crate) const FFX_SURFACE_FORMAT_R8G8B8A8_UNORM: i32 = 10;
pub(crate) const FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB: i32 = 0;
pub(crate) const FFX_RESOURCE_USAGE_READ_ONLY: i32 = 0;
pub(crate) const FFX_RESOURCE_STATE_COMMON: i32 = 1;
pub(crate) const FFX_RESOURCE_STATE_UNORDERED_ACCESS: i32 = 1 << 1;
pub(crate) const FFX_RESOURCE_STATE_COMPUTE_READ: i32 = 1 << 2;
pub(crate) const FFX_RESOURCE_STATE_COPY_SRC: i32 = 1 << 4;
pub(crate) const FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED: u32 = 1 << 0;
pub(crate) const FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE: u32 = 1 << 1;
pub(crate) const FFX_FRAMEINTERPOLATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS: u32 = 1 << 4;
pub(crate) const FFX_FRAMEINTERPOLATION_ENABLE_JITTER_MOTION_VECTORS: u32 = 1 << 5;

extern "C" {
    pub(crate) fn ffxGetScratchMemorySizeDX12(max_contexts: usize) -> usize;
    pub(crate) fn ffxGetDeviceDX12(device: *mut c_void) -> *mut c_void;
    pub(crate) fn ffxGetInterfaceDX12(
        backend: *mut FfxInterface,
        device: *mut c_void,
        scratch: *mut c_void,
        scratch_size: usize,
        max_contexts: usize,
    ) -> i32;
    pub(crate) fn ffxGetCommandListDX12(list: *mut c_void) -> *mut c_void;
    pub(crate) fn ffxGetResourceDX12(
        resource: *const c_void,
        desc: FfxResourceDescription,
        name: *const u16,
        state: i32,
    ) -> FfxResource;
    pub(crate) fn ffxGetSurfaceFormatDX12(format: DXGI_FORMAT) -> i32;
    pub(crate) fn ffxGetResourceDescriptionDX12(
        resource: *const c_void,
        additional_usages: i32,
    ) -> FfxResourceDescription;
    pub(crate) fn ffxFrameInterpolationContextCreate(
        context: *mut c_void,
        desc: *mut FfxFrameInterpolationContextDescription,
    ) -> i32;
    pub(crate) fn ffxFrameInterpolationContextGetGpuMemoryUsage(
        context: *mut c_void,
        usage: *mut FfxEffectMemoryUsage,
    ) -> i32;
    pub(crate) fn ffxFrameInterpolationPrepare(
        context: *mut c_void,
        params: *const FfxFrameInterpolationPrepareDescription,
    ) -> i32;
    pub(crate) fn ffxFrameInterpolationDispatch(
        context: *mut c_void,
        params: *const FfxFrameInterpolationDispatchDescription,
    ) -> i32;
    pub(crate) fn ffxFrameInterpolationContextDestroy(context: *mut c_void) -> i32;
    pub(crate) fn ffxOpticalflowContextCreate(
        context: *mut c_void,
        desc: *mut FfxOpticalflowContextDescription,
    ) -> i32;
    pub(crate) fn ffxOpticalflowContextGetGpuMemoryUsage(
        context: *mut c_void,
        usage: *mut FfxEffectMemoryUsage,
    ) -> i32;
    pub(crate) fn ffxOpticalflowContextDispatch(
        context: *mut c_void,
        params: *const FfxOpticalflowDispatchDescription,
    ) -> i32;
    pub(crate) fn ffxOpticalflowContextDestroy(context: *mut c_void) -> i32;
}

// ---- NVIDIA optical flow (nvOpticalFlowCommon.h, nvOpticalFlowD3D12.h; API 5.0) ----

const NV_OF_API_VERSION: u32 = 80;
const NV_OF_SUCCESS: i32 = 0;
const NV_OF_TRUE: i32 = 1;
const NV_OF_OUTPUT_VECTOR_GRID_SIZE_4: i32 = 4;
const NV_OF_MODE_OPTICALFLOW: i32 = 1;
const NV_OF_PERF_LEVEL_SLOW: i32 = 5;
const NV_OF_PERF_LEVEL_MEDIUM: i32 = 10;
const NV_OF_PERF_LEVEL_FAST: i32 = 20;
const NV_OF_BUFFER_FORMAT_ABGR8: i32 = 3;
#[repr(C)]
#[derive(Clone, Copy)]
struct NvOfFencePoint {
    fence: *mut c_void,
    value: u64,
}
#[repr(C)]
#[derive(Default)]
struct NvOfInitParams {
    width: u32,
    height: u32,
    out_grid_size: i32,
    hint_grid_size: i32,
    mode: i32,
    perf_level: i32,
    enable_external_hints: i32,
    enable_output_cost: i32,
    priv_data: usize,
    disparity_range: i32,
    enable_roi: i32,
    pred_direction: i32,
    enable_global_flow: i32,
    input_buffer_format: i32,
}
#[repr(C)]
struct NvOfRegisterResource {
    resource: *mut c_void,
    input_fence_point: NvOfFencePoint,
    buffer: *mut *mut c_void,
    output_fence_point: NvOfFencePoint,
}
#[repr(C)]
struct NvOfUnregisterResource {
    buffer: *mut c_void,
}
#[repr(C)]
struct NvOfExecuteInput {
    input_frame: *mut c_void,
    reference_frame: *mut c_void,
    external_hints: *mut c_void,
    disable_temporal_hints: i32,
    padding: u32,
    priv_data: *mut c_void,
    padding2: u32,
    num_rois: u32,
    roi_data: *mut c_void,
    padding3: u32,
    num_fence_points: u32,
    fence_point: *mut NvOfFencePoint,
}
#[repr(C)]
struct NvOfExecuteOutput {
    output_buffer: *mut c_void,
    output_cost_buffer: *mut c_void,
    priv_data: *mut c_void,
    bwd_output_buffer: *mut c_void,
    bwd_output_cost_buffer: *mut c_void,
    global_flow_buffer: *mut c_void,
    padding: [u32; 2],
    fence_point: *mut NvOfFencePoint,
}
/// NV_OF_D3D12_API_FUNCTION_LIST.
#[repr(C)]
#[derive(Default)]
struct NvOfApi {
    create: Option<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>,
    init: Option<unsafe extern "system" fn(*mut c_void, *const NvOfInitParams) -> i32>,
    get_surface_format_count: usize,
    get_surface_format: usize,
    register: Option<unsafe extern "system" fn(*mut c_void, *mut NvOfRegisterResource) -> i32>,
    unregister: Option<unsafe extern "system" fn(*mut NvOfUnregisterResource) -> i32>,
    execute: Option<
        unsafe extern "system" fn(
            *mut c_void,
            *const NvOfExecuteInput,
            *mut NvOfExecuteOutput,
        ) -> i32,
    >,
    destroy: Option<unsafe extern "system" fn(*mut c_void) -> i32>,
    get_last_error: usize,
    get_caps: usize,
}
const _: () = assert!(
    size_of::<NvOfApi>() == 80
        && size_of::<NvOfInitParams>() == 64
        && offset_of!(NvOfInitParams, priv_data) == 32
        && offset_of!(NvOfInitParams, input_buffer_format) == 56
        && size_of::<NvOfFencePoint>() == 16
        && size_of::<NvOfRegisterResource>() == 48
        && offset_of!(NvOfRegisterResource, buffer) == 24
        && size_of::<NvOfExecuteInput>() == 72
        && offset_of!(NvOfExecuteInput, priv_data) == 32
        && offset_of!(NvOfExecuteInput, num_rois) == 44
        && offset_of!(NvOfExecuteInput, fence_point) == 64
        && size_of::<NvOfExecuteOutput>() == 64
        && offset_of!(NvOfExecuteOutput, fence_point) == 56
);

// ---- DLSS-G: dlssg_native.rs (formerly src/Framegen/DlssFrameGen.cpp's) ----

/// cvr::framegen::dlssg::Evaluation.
#[repr(C)]
pub(crate) struct DlssEvaluation {
    pub(crate) backbuffer: *mut c_void,
    pub(crate) depth: *mut c_void,
    pub(crate) motion: *mut c_void,
    pub(crate) output: *mut c_void,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth_width: u32,
    pub(crate) depth_height: u32,
    pub(crate) motion_width: u32,
    pub(crate) motion_height: u32,
    pub(crate) camera: *const CameraData,
    pub(crate) previous: *const CameraData,
    pub(crate) reset: bool,
}
const _: () =
    assert!(size_of::<DlssEvaluation>() == 80 && offset_of!(DlssEvaluation, camera) == 56);

#[allow(non_upper_case_globals)]
extern "C" {
    /// src/Framegen/FrameCapture.cpp's switch.
    static CyberpunkVR_FramegenCapture: AtomicI32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetFileAttributesW(name: *const u16) -> u32;
}

fn log(text: &str) {
    cvr_host::log::write(text);
}
fn status(text: &core::ffi::CStr, active: bool) {
    // SAFETY: a NUL-terminated literal.
    unsafe { framegen_core::cvr_framegen_status(text.as_ptr(), active) };
}

pub(crate) fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: Option<&ID3D12Resource>,
    a: D3D12_RESOURCE_STATES,
    b: D3D12_RESOURCE_STATES,
) {
    if a == b {
        return;
    }
    let pointer = raw(resource);
    // SAFETY: the barrier borrows the resource's interface pointer for the call (no reference is taken or dropped).
    unsafe {
        let v = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: core::mem::transmute_copy(&pointer),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: a,
                    StateAfter: b,
                }),
            },
        };
        list.ResourceBarrier(&[v]);
    }
}
pub(crate) fn raw(resource: Option<&ID3D12Resource>) -> *mut c_void {
    resource.map_or(core::ptr::null_mut(), |r| r.as_raw())
}

#[derive(Default)]
struct Texture {
    resource: Option<ID3D12Resource>,
    format: DXGI_FORMAT,
    state: D3D12_RESOURCE_STATES,
    width: u32,
    height: u32,
    bytes: u64,
}
impl Texture {
    fn create(
        &mut self,
        device: &ID3D12Device,
        w: u32,
        h: u32,
        f: DXGI_FORMAT,
        name: PCWSTR,
        initial: D3D12_RESOURCE_STATES,
    ) -> bool {
        let d = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: w as u64,
            Height: h,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: f,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let p = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut created: Option<ID3D12Resource> = None;
        // SAFETY: the live device; descriptions on the stack.
        unsafe {
            if device
                .CreateCommittedResource(&p, D3D12_HEAP_FLAG_NONE, &d, initial, None, &mut created)
                .is_err()
            {
                return false;
            }
            let Some(resource) = created else {
                return false;
            };
            self.format = f;
            self.state = initial;
            self.width = w;
            self.height = h;
            self.bytes = device.GetResourceAllocationInfo(0, &[d]).SizeInBytes;
            let _ = resource.SetName(name);
            self.resource = Some(resource);
        }
        true
    }
    fn to(&mut self, list: &ID3D12GraphicsCommandList, target: D3D12_RESOURCE_STATES) {
        barrier(list, self.resource.as_ref(), self.state, target);
        self.state = target;
    }
    fn raw(&self) -> *mut c_void {
        raw(self.resource.as_ref())
    }
}

/// FfxResource for a D3D12 resource, its format overridden when one is given.
pub(crate) unsafe fn resource(resource: *mut c_void, state: i32, override_format: DXGI_FORMAT) -> FfxResource {
    let mut desc = ffxGetResourceDescriptionDX12(resource, FFX_RESOURCE_USAGE_READ_ONLY);
    if override_format != DXGI_FORMAT_UNKNOWN {
        desc.format = ffxGetSurfaceFormatDX12(override_format);
    }
    ffxGetResourceDX12(resource, desc, w!("CVR_Framegen").as_ptr(), state)
}
unsafe fn texture_resource(t: &Texture, state: i32) -> FfxResource {
    resource(t.raw(), state, t.format)
}
fn ffx(code: i32, operation: &str) -> bool {
    if code == FFX_OK {
        return true;
    }
    log(&format!("[framegen] {operation} failed: {}\n", code as u32));
    let text = std::ffi::CString::new(operation).unwrap_or_default();
    // SAFETY: a NUL-terminated string.
    unsafe { framegen_core::cvr_framegen_status(text.as_ptr(), false) };
    false
}

#[derive(Default)]
struct NvFlow {
    module: HMODULE,
    api: NvOfApi,
    context: usize,
    colors: [usize; 2],
    flow: usize,
    fence: Option<ID3D12Fence>,
    value: u64,
}
impl NvFlow {
    fn unregister(&mut self) {
        let unregister = self.api.unregister;
        let [first, second] = &mut self.colors;
        for h in [first, second, &mut self.flow] {
            if *h != 0 {
                if let Some(unregister) = unregister {
                    let mut p = NvOfUnregisterResource {
                        buffer: *h as *mut c_void,
                    };
                    // SAFETY: a buffer this context registered.
                    unsafe { unregister(&mut p) };
                    *h = 0;
                }
            }
        }
    }
    /// ~NvFlow: the context destroyed, the library freed.
    fn destroy(&mut self) {
        self.unregister();
        if self.context != 0 {
            if let Some(destroy) = self.api.destroy {
                // SAFETY: the context this object created.
                unsafe { destroy(self.context as *mut c_void) };
            }
        }
        self.context = 0;
        if !self.module.is_invalid() {
            // SAFETY: the module this object loaded.
            unsafe {
                let _ = FreeLibrary(self.module);
            }
        }
        self.module = HMODULE::default();
    }
    fn idle(&self) -> bool {
        // SAFETY: the live fence.
        self.fence
            .as_ref()
            .is_none_or(|f| unsafe { f.GetCompletedValue() } >= self.value)
    }
    unsafe fn create(
        &mut self,
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
        ready: &ID3D12Fence,
        input: [&Texture; 2],
        output: &Texture,
        quality: i32,
    ) -> bool {
        let Ok(module) = LoadLibraryExW(w!("nvofapi64.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
        else {
            return false;
        };
        self.module = module;
        type CreateFn = unsafe extern "system" fn(u32, *mut NvOfApi) -> i32;
        let Some(create) = GetProcAddress(module, windows::core::s!("NvOFAPICreateInstanceD3D12"))
        else {
            return false;
        };
        let create: CreateFn = core::mem::transmute(create);
        if create(NV_OF_API_VERSION, &mut self.api) != NV_OF_SUCCESS {
            return false;
        }
        let mut context: *mut c_void = core::ptr::null_mut();
        let Some(create_of) = self.api.create else {
            return false;
        };
        if create_of(device.as_raw(), &mut context) != NV_OF_SUCCESS {
            return false;
        }
        self.context = context as usize;
        let init = NvOfInitParams {
            width: input[0].width,
            height: input[0].height,
            out_grid_size: NV_OF_OUTPUT_VECTOR_GRID_SIZE_4,
            mode: NV_OF_MODE_OPTICALFLOW,
            perf_level: match quality {
                0 => NV_OF_PERF_LEVEL_FAST,
                1 => NV_OF_PERF_LEVEL_MEDIUM,
                _ => NV_OF_PERF_LEVEL_SLOW,
            },
            input_buffer_format: NV_OF_BUFFER_FORMAT_ABGR8,
            ..Default::default()
        };
        let Some(init_of) = self.api.init else {
            return false;
        };
        if init_of(context, &init) != NV_OF_SUCCESS {
            return false;
        }
        let Ok(fence) = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) else {
            return false;
        };
        self.fence = Some(fence.clone());
        let resources = [input[0].raw(), input[1].raw(), output.raw()];
        let mut handles: [*mut c_void; 3] = [core::ptr::null_mut(); 3];
        let Some(register) = self.api.register else {
            return false;
        };
        for i in 0..3 {
            let mut p = NvOfRegisterResource {
                resource: resources[i],
                input_fence_point: NvOfFencePoint {
                    fence: ready.as_raw(),
                    value: 0,
                },
                buffer: &mut handles[i],
                output_fence_point: NvOfFencePoint {
                    fence: fence.as_raw(),
                    value: self.value + 1,
                },
            };
            let r = register(context, &mut p);
            self.colors = [handles[0] as usize, handles[1] as usize];
            self.flow = handles[2] as usize;
            if r != NV_OF_SUCCESS {
                return false;
            }
            self.value += 1;
        }
        queue.Wait(&fence, self.value).is_ok()
    }
    unsafe fn run(&mut self, packed: &ID3D12Fence, packed_value: u64, current: u32) -> bool {
        let Some(fence) = self.fence.clone() else {
            return false;
        };
        let mut waits = [
            NvOfFencePoint {
                fence: packed.as_raw(),
                value: packed_value,
            },
            NvOfFencePoint {
                fence: fence.as_raw(),
                value: self.value,
            },
        ];
        let input = NvOfExecuteInput {
            input_frame: self.colors[current as usize] as *mut c_void,
            reference_frame: self.colors[(current ^ 1) as usize] as *mut c_void,
            external_hints: core::ptr::null_mut(),
            disable_temporal_hints: NV_OF_TRUE,
            padding: 0,
            priv_data: core::ptr::null_mut(),
            padding2: 0,
            num_rois: 0,
            roi_data: core::ptr::null_mut(),
            padding3: 0,
            num_fence_points: 2,
            fence_point: waits.as_mut_ptr(),
        };
        let mut completed = NvOfFencePoint {
            fence: fence.as_raw(),
            value: self.value + 1,
        };
        let mut output = NvOfExecuteOutput {
            output_buffer: self.flow as *mut c_void,
            output_cost_buffer: core::ptr::null_mut(),
            priv_data: core::ptr::null_mut(),
            bwd_output_buffer: core::ptr::null_mut(),
            bwd_output_cost_buffer: core::ptr::null_mut(),
            global_flow_buffer: core::ptr::null_mut(),
            padding: [0; 2],
            fence_point: &mut completed,
        };
        let Some(execute) = self.api.execute else {
            return false;
        };
        if execute(self.context as *mut c_void, &input, &mut output) != NV_OF_SUCCESS {
            return false;
        }
        self.value += 1;
        true
    }
}

/// An FFX context's opaque storage (FfxFrameInterpolationContext, FfxOpticalflowContext), zeroed on the heap.
pub(crate) struct Context(Box<[u32]>);
impl Context {
    pub(crate) fn new() -> Self {
        Context(vec![0u32; FFX_CONTEXT_WORDS].into_boxed_slice())
    }
    pub(crate) fn ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr() as *mut c_void
    }
}

struct Eye {
    /// The backend's scratch (16-byte aligned, as the C++ vector's allocation was).
    scratch: Vec<u128>,
    backend: FfxInterface,
    fi: Context,
    of: Context,
    fi_created: bool,
    of_created: bool,
    packed: [Texture; 2],
    raw_flow: Texture,
    flow: Texture,
    scd: Texture,
    depth: Texture,
    motion: Texture,
    previous_depth: Texture,
    output: Texture,
    prepared_depth: Texture,
    prepared_motion: Texture,
    nv: NvFlow,
    /// DLSS-G: one NGX feature per eye (src/Framegen/DlssFrameGen.cpp), a typed copy of a typeless capture, and the
    /// camera NGX's history was last fed with.
    dlss: *mut c_void,
    backbuffer: Texture,
    /// ANR FG reads the depth and the motion vectors in COPY_DEST, the state Streamline hands DLSS-G its copies in
    /// (OptiScaler's provider proxy passes the game's through): our own copies, kept in that state.
    anr_depth: Texture,
    anr_motion: Texture,
    previous_camera: CameraData,
    has_previous_camera: bool,
    width: u32,
    height: u32,
    render_width: u32,
    render_height: u32,
    flow_width: u32,
    flow_height: u32,
    current: u32,
    optical_flow: bool,
    prepare_inputs: bool,
}
impl Drop for Eye {
    fn drop(&mut self) {
        // SAFETY: the contexts this eye created.
        unsafe {
            if self.fi_created {
                ffxFrameInterpolationContextDestroy(self.fi.ptr());
            }
            if self.of_created {
                ffxOpticalflowContextDestroy(self.of.ptr());
            }
        }
        // The registered D3D12 allocations must be released before destroying their OFA context (also the teardown
        // order used by OFXR). Reversing these steps crashed nvwgf2umx during our hardware retirement test.
        self.nv.unregister();
        self.packed[0].resource = None;
        self.packed[1].resource = None;
        self.raw_flow.resource = None;
        // Then the members in the C++ object's reverse order: the backbuffer copy, the NGX feature, the OFA context.
        self.backbuffer.resource = None;
        if !self.dlss.is_null() {
            // SAFETY: the feature the adapter created.
            unsafe { crate::dlssg_native::free(self.dlss) };
            self.dlss = core::ptr::null_mut();
        }
        self.nv.destroy();
    }
}
impl Eye {
    fn new() -> Box<Eye> {
        Box::new(Eye {
            scratch: Vec::new(),
            backend: FfxInterface([0; 29]),
            fi: Context::new(),
            of: Context::new(),
            fi_created: false,
            of_created: false,
            packed: Default::default(),
            raw_flow: Texture::default(),
            flow: Texture::default(),
            scd: Texture::default(),
            depth: Texture::default(),
            motion: Texture::default(),
            previous_depth: Texture::default(),
            output: Texture::default(),
            prepared_depth: Texture::default(),
            prepared_motion: Texture::default(),
            nv: NvFlow::default(),
            dlss: core::ptr::null_mut(),
            backbuffer: Texture::default(),
            anr_depth: Texture::default(),
            anr_motion: Texture::default(),
            previous_camera: CameraData::default(),
            has_previous_camera: false,
            width: 0,
            height: 0,
            render_width: 0,
            render_height: 0,
            flow_width: 0,
            flow_height: 0,
            current: 0,
            optical_flow: true,
            prepare_inputs: false,
        })
    }
    unsafe fn create(
        &mut self,
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
        fence: &ID3D12Fence,
        color: &ID3D12Resource,
        inputs: &Inputs,
        settings: &Settings,
    ) -> bool {
        let desc = color.GetDesc();
        self.width = desc.Width as u32;
        self.height = desc.Height;
        if desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS
            && desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM
            && desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        {
            status(
                c"Framegen requires the current SDR RGBA8 capture format",
                false,
            );
            return false;
        }
        self.render_width = inputs.width.min(inputs.motion_width);
        self.render_height = inputs.height.min(inputs.motion_height);
        let camera = inputs.camera;
        let (width, height) = (self.width, self.height);
        if ngx_backend(settings.backend) {
            if !self.output.create(
                device,
                width,
                height,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                w!("CVR_FG_Output"),
                COPY,
            ) {
                return false;
            }
            // NGX views the backbuffer in its own format, which a typeless capture does not have.
            return desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS
                || self.backbuffer.create(
                    device,
                    width,
                    height,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    w!("CVR_FG_DlssBackbuffer"),
                    READ,
                );
        }
        // DLAA does not need full-display-resolution depth/MV preparation for interpolation. Keep real color untouched
        // and bound only FI's inputs.
        if inputs.width == width && inputs.height == height {
            self.render_width = self.render_width.min(width.div_ceil(2));
            self.render_height = self.render_height.min(height.div_ceil(2));
        }
        self.prepare_inputs = self.render_width != inputs.width
            || self.render_height != inputs.height
            || self.render_width != inputs.motion_width
            || self.render_height != inputs.motion_height;
        if self.render_width == 0 || self.render_height == 0 {
            return false;
        }
        let scale = settings.flow_scale as u32;
        self.flow_width = 64u32.max(width.wrapping_mul(scale).wrapping_add(99) / 100);
        self.flow_height = 64u32.max(height.wrapping_mul(scale).wrapping_add(99) / 100);
        let nvidia = settings.backend == NVIDIA;
        self.optical_flow = settings.flow_scale > 0;
        // Both modes use FidelityFX's depth/MV-aware interpolation. NVIDIA replaces only the optical-flow stage, with
        // the hardware OFA engine.
        let bytes = ffxGetScratchMemorySizeDX12(2);
        self.scratch = vec![0u128; bytes.div_ceil(16)];
        if !ffx(
            ffxGetInterfaceDX12(
                &mut self.backend,
                ffxGetDeviceDX12(device.as_raw()),
                self.scratch.as_mut_ptr() as *mut c_void,
                bytes,
                2,
            ),
            "FidelityFX backend",
        ) {
            return false;
        }
        let mut config = FfxFrameInterpolationContextDescription {
            flags: 0,
            max_render_size: FfxDimensions2D {
                width: self.render_width,
                height: self.render_height,
            },
            display_size: FfxDimensions2D { width, height },
            back_buffer_format: FFX_SURFACE_FORMAT_R8G8B8A8_UNORM,
            previous_interpolation_source_format: FFX_SURFACE_FORMAT_R8G8B8A8_UNORM,
            backend_interface: self.backend,
        };
        if camera.inverted {
            config.flags |= FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED;
        }
        if camera.infinite {
            config.flags |= FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE;
        }
        if camera.jittered {
            config.flags |= FFX_FRAMEINTERPOLATION_ENABLE_JITTER_MOTION_VECTORS;
        }
        if !self.prepare_inputs && inputs.motion_width == width && inputs.motion_height == height {
            config.flags |= FFX_FRAMEINTERPOLATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS;
        }
        if !ffx(
            ffxFrameInterpolationContextCreate(self.fi.ptr(), &mut config),
            "Frame interpolation context",
        ) {
            return false;
        }
        self.fi_created = true;
        if !nvidia && self.optical_flow {
            let mut optics = FfxOpticalflowContextDescription {
                backend_interface: self.backend,
                flags: 0,
                resolution: FfxDimensions2D {
                    width: self.flow_width,
                    height: self.flow_height,
                },
            };
            if !ffx(
                ffxOpticalflowContextCreate(self.of.ptr(), &mut optics),
                "Optical flow context",
            ) {
                return false;
            }
            self.of_created = true;
        }
        let block = if nvidia { 4u32 } else { 8u32 };
        let (fw, fh) = (self.flow_width, self.flow_height);
        // Without optical flow, interpolation runs on the engine's vectors alone and reads an all-zero flow.
        if self.optical_flow
            && (!self.packed[0].create(
                device,
                fw,
                fh,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                w!("CVR_FG_PackedA"),
                D3D12_RESOURCE_STATE_COMMON,
            ) || (nvidia
                && !self.packed[1].create(
                    device,
                    fw,
                    fh,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    w!("CVR_FG_PackedB"),
                    D3D12_RESOURCE_STATE_COMMON,
                ))
                || !self.raw_flow.create(
                    device,
                    fw.div_ceil(block),
                    fh.div_ceil(block),
                    DXGI_FORMAT_R16G16_SINT,
                    w!("CVR_FG_RawFlow"),
                    if nvidia {
                        D3D12_RESOURCE_STATE_COMMON
                    } else {
                        UAV
                    },
                ))
        {
            return false;
        }
        let (rw, rh) = (self.render_width, self.render_height);
        if !self.flow.create(
            device,
            width.div_ceil(8),
            height.div_ceil(8),
            DXGI_FORMAT_R16G16_SINT,
            w!("CVR_FG_Flow"),
            UAV,
        ) || !self.scd.create(
            device,
            3,
            1,
            DXGI_FORMAT_R32_UINT,
            w!("CVR_FG_SceneChange"),
            UAV,
        ) || !self.depth.create(
            device,
            rw,
            rh,
            DXGI_FORMAT_R32_FLOAT,
            w!("CVR_FG_DilatedDepth"),
            UAV,
        ) || !self.motion.create(
            device,
            rw,
            rh,
            DXGI_FORMAT_R16G16_FLOAT,
            w!("CVR_FG_DilatedMotion"),
            UAV,
        ) || !self.previous_depth.create(
            device,
            rw,
            rh,
            DXGI_FORMAT_R32_UINT,
            w!("CVR_FG_PreviousDepth"),
            UAV,
        ) || !self.output.create(
            device,
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            w!("CVR_FG_Output"),
            COPY,
        ) {
            return false;
        }
        if self.prepare_inputs
            && (!self.prepared_depth.create(
                device,
                rw,
                rh,
                DXGI_FORMAT_R32_FLOAT,
                w!("CVR_FG_PreparedDepth"),
                UAV,
            ) || !self.prepared_motion.create(
                device,
                rw,
                rh,
                DXGI_FORMAT_R32G32_FLOAT,
                w!("CVR_FG_PreparedMotion"),
                UAV,
            ))
        {
            return false;
        }
        if !nvidia || !self.optical_flow {
            return true;
        }
        let (packed0, packed1) = (&self.packed[0], &self.packed[1]);
        let raw_flow = &self.raw_flow;
        let mut nv = core::mem::take(&mut self.nv);
        let created = nv.create(
            device,
            queue,
            fence,
            [packed0, packed1],
            raw_flow,
            settings.quality,
        );
        self.nv = nv;
        created
    }
    fn bytes(&mut self) -> u64 {
        let mut bytes = 0u64;
        for t in [
            &self.packed[0],
            &self.packed[1],
            &self.raw_flow,
            &self.flow,
            &self.scd,
            &self.depth,
            &self.motion,
            &self.previous_depth,
            &self.output,
            &self.prepared_depth,
            &self.prepared_motion,
            &self.backbuffer,
            &self.anr_depth,
            &self.anr_motion,
        ] {
            bytes = bytes.wrapping_add(t.bytes);
        }
        // SAFETY: the feature and contexts this eye created.
        unsafe {
            if !self.dlss.is_null() {
                bytes = bytes.wrapping_add(crate::dlssg_native::bytes(self.dlss));
            }
            let mut usage = FfxEffectMemoryUsage::default();
            if self.fi_created
                && ffxFrameInterpolationContextGetGpuMemoryUsage(self.fi.ptr(), &mut usage)
                    == FFX_OK
            {
                bytes = bytes.wrapping_add(usage.total_usage_in_bytes);
            }
            if self.of_created
                && ffxOpticalflowContextGetGpuMemoryUsage(self.of.ptr(), &mut usage) == FFX_OK
            {
                bytes = bytes.wrapping_add(usage.total_usage_in_bytes);
            }
        }
        bytes
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn interpolate(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        color: &ID3D12Resource,
        inputs: &Inputs,
        frame_ms: f64,
        reset: bool,
        frame_id: u64,
        nvidia: bool,
    ) -> bool {
        let cam = &inputs.camera;
        let (rw, rh) = (self.render_width, self.render_height);
        let command_list = ffxGetCommandListDX12(list.as_raw());
        let p = FfxFrameInterpolationPrepareDescription {
            flags: 0,
            command_list,
            render_size: FfxDimensions2D {
                width: rw,
                height: rh,
            },
            jitter_offset: FfxFloatCoords2D {
                x: cam.jitter[0] * rw as f32 / inputs.width as f32,
                y: cam.jitter[1] * rh as f32 / inputs.height as f32,
            },
            // Streamline's normalized displacement scale -> FFX's pixel displacement scale.
            motion_vector_scale: FfxFloatCoords2D {
                x: cam.motion_scale[0]
                    * (if self.prepare_inputs {
                        rw
                    } else {
                        inputs.motion_width
                    }) as f32,
                y: cam.motion_scale[1]
                    * (if self.prepare_inputs {
                        rh
                    } else {
                        inputs.motion_height
                    }) as f32,
            },
            frame_time_delta: frame_ms as f32,
            camera_near: cam.near_plane,
            camera_far: cam.far_plane,
            view_space_to_meters_factor: 1.0,
            camera_fov_angle_vertical: cam.vertical_fov,
            depth: resource(
                if self.prepare_inputs {
                    self.prepared_depth.raw()
                } else {
                    raw(inputs.depth.as_ref())
                },
                FFX_RESOURCE_STATE_COMPUTE_READ,
                DXGI_FORMAT_UNKNOWN,
            ),
            motion_vectors: resource(
                if self.prepare_inputs {
                    self.prepared_motion.raw()
                } else {
                    raw(inputs.motion.as_ref())
                },
                FFX_RESOURCE_STATE_COMPUTE_READ,
                DXGI_FORMAT_UNKNOWN,
            ),
            frame_id,
            dilated_depth: texture_resource(&self.depth, FFX_RESOURCE_STATE_UNORDERED_ACCESS),
            dilated_motion_vectors: texture_resource(
                &self.motion,
                FFX_RESOURCE_STATE_UNORDERED_ACCESS,
            ),
            reconstructed_prev_depth: texture_resource(
                &self.previous_depth,
                FFX_RESOURCE_STATE_UNORDERED_ACCESS,
            ),
            camera_position: cam.position,
            camera_up: cam.up,
            camera_right: cam.right,
            camera_forward: cam.forward,
        };
        if !ffx(
            ffxFrameInterpolationPrepare(self.fi.ptr(), &p),
            "Frame interpolation prepare",
        ) {
            return false;
        }
        let units = if nvidia { 32.0f32 } else { 1.0f32 };
        let d = FfxFrameInterpolationDispatchDescription {
            flags: 0,
            command_list: p.command_list,
            display_size: FfxDimensions2D {
                width: self.width,
                height: self.height,
            },
            render_size: p.render_size,
            current_back_buffer: resource(
                color.as_raw(),
                FFX_RESOURCE_STATE_COPY_SRC,
                DXGI_FORMAT_R8G8B8A8_UNORM,
            ),
            current_back_buffer_hudless: FfxResource::default(),
            output: texture_resource(&self.output, FFX_RESOURCE_STATE_COPY_SRC),
            interpolation_rect: FfxRect2D {
                left: 0,
                top: 0,
                width: self.width as i32,
                height: self.height as i32,
            },
            optical_flow_vector: texture_resource(&self.flow, FFX_RESOURCE_STATE_UNORDERED_ACCESS),
            optical_flow_scene_change_detection: texture_resource(
                &self.scd,
                FFX_RESOURCE_STATE_UNORDERED_ACCESS,
            ),
            optical_flow_buffer_size: FfxDimensions2D {
                width: self.flow.width,
                height: self.flow.height,
            },
            optical_flow_scale: FfxFloatCoords2D {
                x: 1.0 / (units * self.flow_width as f32),
                y: 1.0 / (units * self.flow_height as f32),
            },
            optical_flow_block_size: 8,
            camera_near: p.camera_near,
            camera_far: p.camera_far,
            camera_fov_angle_vertical: p.camera_fov_angle_vertical,
            view_space_to_meters_factor: 1.0,
            frame_time_delta: p.frame_time_delta,
            reset,
            back_buffer_transfer_function: FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB,
            min_max_luminance: [0.0, 1.0],
            frame_id,
            dilated_depth: p.dilated_depth,
            dilated_motion_vectors: p.dilated_motion_vectors,
            reconstructed_prev_depth: p.reconstructed_prev_depth,
            distortion_field: FfxResource::default(),
        };
        ffx(
            ffxFrameInterpolationDispatch(self.fi.ptr(), &d),
            "Frame interpolation dispatch",
        )
    }
}

use crate::framegen_shaders::{PREPARE_INPUTS_SHADER, SHADER};

#[derive(Default)]
struct Work {
    allocator: Option<ID3D12CommandAllocator>,
    finish_allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    finish: Option<ID3D12GraphicsCommandList>,
    heap: Option<ID3D12DescriptorHeap>,
    fence: u64,
    descriptors: u32,
    timing: bool,
    /// The inputs this slot's pair read, held until its fence passes.
    inputs: [Option<Arc<Record>>; 2],
}

#[derive(Clone, Copy, Default)]
struct Signature {
    width: u64,
    height: u32,
    render_width: u32,
    render_height: u32,
    motion_width: u32,
    motion_height: u32,
    inverted: bool,
    infinite: bool,
    jittered: bool,
}

struct Impl {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: Option<ID3D12Fence>,
    consumer_fence: Option<ID3D12Fence>,
    value: u64,
    consumer_value: u64,
    frequency: u64,
    root: Option<ID3D12RootSignature>,
    pipelines: [Option<ID3D12PipelineState>; 4],
    mode: i32,
    queries: Option<ID3D12QueryHeap>,
    readback: Option<ID3D12Resource>,
    last_timing_fence: u64,
    timing_unavailable: bool,
    work: [Work; 3],
    eyes: [Option<Box<Eye>>; 2],
    settings: Settings,
    cursor: u32,
    failed: bool,
    signatures: [Signature; 2],
}
impl Drop for Impl {
    fn drop(&mut self) {
        // The C++ object's members in reverse: the eyes (right, then left), then the work slots (last first).
        self.eyes[1] = None;
        self.eyes[0] = None;
        for w in self.work.iter_mut().rev() {
            *w = Work::default();
        }
    }
}

/// An input lease: another reference to the record AcquireInputs handed out.
unsafe fn lease(inputs: *const Inputs) -> Arc<Record> {
    let record = inputs as *const Record;
    Arc::increment_strong_count(record);
    Arc::from_raw(record)
}
fn metrics_enabled() -> bool {
    framegen_core::cvr_framegen_metrics_enabled()
}

impl Impl {
    fn idle(&self) -> bool {
        // SAFETY: live fences.
        unsafe {
            if self
                .fence
                .as_ref()
                .is_some_and(|f| f.GetCompletedValue() < self.value)
            {
                return false;
            }
            if self
                .consumer_fence
                .as_ref()
                .is_some_and(|f| f.GetCompletedValue() < self.consumer_value)
            {
                return false;
            }
        }
        self.eyes.iter().flatten().all(|eye| eye.nv.idle())
    }
    unsafe fn create(&mut self, colors: [&ID3D12Resource; 2], inputs: [&Inputs; 2]) -> bool {
        let device = self.device.clone();
        let s = self.settings;
        for i in 0..2 {
            let desc = colors[i].GetDesc();
            let input = inputs[i];
            self.signatures[i] = Signature {
                width: desc.Width,
                height: desc.Height,
                render_width: input.width,
                render_height: input.height,
                motion_width: input.motion_width,
                motion_height: input.motion_height,
                inverted: input.camera.inverted,
                infinite: input.camera.infinite,
                jittered: input.camera.jittered,
            };
        }
        let Ok(fence) = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) else {
            return false;
        };
        self.fence = Some(fence.clone());
        let ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 3,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 0,
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                NumDescriptors: 3,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 0,
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
                        Num32BitValues: 6,
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
            MaxAnisotropy: 1,
            ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
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
        let Some(signature) = blob else { return false };
        let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(
            0,
            core::slice::from_raw_parts(
                signature.GetBufferPointer() as *const u8,
                signature.GetBufferSize(),
            ),
        ) else {
            return false;
        };
        self.root = Some(root.clone());
        const NAMES: [&core::ffi::CStr; 4] = [c"Pack", c"Flow", c"SceneCut", c"PrepareInputs"];
        // the passes made from Rust (rust/shaders/passes/src/framegen.rs); the HLSL above with CVR_SHADER_HLSL
        let dxil = [
            cvr_gpu::dxil::FRAMEGEN_PACK_CS,
            cvr_gpu::dxil::FRAMEGEN_FLOW_CS,
            cvr_gpu::dxil::FRAMEGEN_SCENE_CUT_CS,
            cvr_gpu::dxil::FRAMEGEN_PREPARE_INPUTS_CS,
        ];
        let mut i = 0usize;
        while i < 4 && (self.mode == FIDELITY_FX || self.mode == NVIDIA) {
            if i == 2 && s.backend == FIDELITY_FX {
                i += 1;
                continue;
            }
            let source = if i == 3 {
                PREPARE_INPUTS_SHADER
            } else {
                SHADER
            };
            let hlsl = || {
                let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
                if D3DCompile(
                    source.as_ptr() as *const c_void,
                    source.len(),
                    windows::core::s!("FramegenPrepare"),
                    None,
                    None,
                    PCSTR(NAMES[i].as_ptr() as *const u8),
                    windows::core::s!("cs_5_0"),
                    D3DCOMPILE_OPTIMIZATION_LEVEL3,
                    0,
                    &mut code,
                    Some(&mut errors),
                )
                .is_err()
                {
                    if let Some(e) = errors {
                        let text =
                            core::ffi::CStr::from_ptr(e.GetBufferPointer() as *const core::ffi::c_char);
                        log(&format!(
                            "[framegen] shader {}: {}\n",
                            NAMES[i].to_string_lossy(),
                            text.to_string_lossy()
                        ));
                    }
                    return None;
                }
                code
            };
            let Some(code) = cvr_gpu::dxil::pick(dxil[i], hlsl) else { return false };
            let p = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                CS: code.bytecode(),
                ..Default::default()
            };
            let Ok(pipeline) = device.CreateComputePipelineState::<ID3D12PipelineState>(&p) else {
                return false;
            };
            self.pipelines[i] = Some(pipeline);
            i += 1;
        }
        for w in self.work.iter_mut() {
            let (Ok(allocator), Ok(finish_allocator)) = (
                device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                ),
                device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                ),
            ) else {
                return false;
            };
            w.allocator = Some(allocator.clone());
            w.finish_allocator = Some(finish_allocator.clone());
            let Ok(list) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None,
            ) else {
                return false;
            };
            w.list = Some(list.clone());
            let Ok(finish) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &finish_allocator,
                None,
            ) else {
                return false;
            };
            w.finish = Some(finish.clone());
            let _ = list.Close();
            let _ = finish.Close();
            let h = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 48,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            };
            let Ok(heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&h) else {
                return false;
            };
            w.heap = Some(heap);
        }
        for eye in 0..2 {
            self.eyes[eye] = Some(Eye::new());
            let queue = self.queue.clone();
            let Some(e) = self.eyes[eye].as_mut() else {
                return false;
            };
            if !e.create(&device, &queue, &fence, colors[eye], inputs[eye], &s) {
                return false;
            }
        }
        !ngx_backend(self.mode) || self.create_dlss_features(inputs)
    }
    /// NGX records the features' creation into a command list; it runs before the first pair.
    unsafe fn create_dlss_features(&mut self, inputs: [&Inputs; 2]) -> bool {
        let (Some(allocator), Some(list)) =
            (self.work[0].allocator.clone(), self.work[0].list.clone())
        else {
            return false;
        };
        if allocator.Reset().is_err() || list.Reset(&allocator, None).is_err() {
            return false;
        }
        let mut created = true;
        let mut i = 0;
        let provider = provider_of(self.mode);
        while i < 2 && created {
            let input = inputs[i];
            let motion_format = input
                .motion
                .as_ref()
                .map_or(DXGI_FORMAT_UNKNOWN, |m| m.GetDesc().Format);
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            eye.dlss = crate::dlssg_native::create(
                list.as_raw(),
                eye.width,
                eye.height,
                input.width,
                input.height,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                motion_format,
                DXGI_FORMAT_R32_FLOAT,
                provider,
            );
            created = !eye.dlss.is_null();
            i += 1;
        }
        let submitted = self.submit(&list, 0);
        if !created {
            status(
                if self.mode == ANR {
                    c"ANR FG could not create its features"
                } else {
                    c"DLSS Frame Generation could not create its features"
                },
                false,
            );
        }
        created && submitted
    }
    unsafe fn record_dlss(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        eye: usize,
        color: &ID3D12Resource,
        inputs: &Inputs,
        reset: bool,
    ) -> bool {
        let anr = self.mode == ANR;
        let device = self.device.clone();
        let Some(eye) = self.eyes[eye].as_mut() else {
            return false;
        };
        let mut depth = raw(inputs.depth.as_ref());
        let mut motion = raw(inputs.motion.as_ref());
        if anr {
            // our own COPY_DEST copies of the depth and the motion vectors (main's 0.2.4)
            let copy = |own: &mut Texture, source: Option<&ID3D12Resource>, name: PCWSTR| -> Option<*mut c_void> {
                let source = source?;
                let d = source.GetDesc();
                if own.resource.is_none() || own.width != d.Width as u32 || own.height != d.Height || own.format != d.Format {
                    *own = Texture::default();
                    if !own.create(&device, d.Width as u32, d.Height, d.Format, name, D3D12_RESOURCE_STATE_COPY_DEST) {
                        return None;
                    }
                }
                let target = own.resource.as_ref()?;
                barrier(list, Some(source), READ, COPY);
                list.CopyResource(target, source);
                barrier(list, Some(source), COPY, READ);
                Some(target.as_raw())
            };
            let (Some(d), Some(m)) = (
                copy(&mut eye.anr_depth, inputs.depth.as_ref(), w!("CVR_FG_AnrDepth")),
                copy(&mut eye.anr_motion, inputs.motion.as_ref(), w!("CVR_FG_AnrMotion")),
            ) else {
                return false;
            };
            depth = d;
            motion = m;
        }
        let mut backbuffer = color.clone();
        if let Some(copy) = eye.backbuffer.resource.clone() {
            eye.backbuffer.to(list, D3D12_RESOURCE_STATE_COPY_DEST);
            list.CopyResource(&copy, color);
            eye.backbuffer.to(list, READ);
            backbuffer = copy;
        } else {
            barrier(list, Some(color), COPY, READ);
        }
        eye.output.to(list, UAV);
        // Every real frame is evaluated, resets included: NGX keeps the previous backbuffer itself.
        let reset = reset || !eye.has_previous_camera;
        let e = DlssEvaluation {
            backbuffer: backbuffer.as_raw(),
            depth,
            motion,
            output: eye.output.raw(),
            width: eye.width,
            height: eye.height,
            depth_width: inputs.width,
            depth_height: inputs.height,
            motion_width: inputs.motion_width,
            motion_height: inputs.motion_height,
            camera: &inputs.camera,
            previous: if reset {
                core::ptr::null()
            } else {
                &eye.previous_camera
            },
            reset,
        };
        let evaluated = crate::dlssg_native::evaluate(eye.dlss, list.as_raw(), &e);
        eye.output.to(list, COPY);
        if eye.backbuffer.resource.is_none() {
            barrier(list, Some(color), READ, COPY);
        }
        eye.previous_camera = inputs.camera;
        eye.has_previous_camera = true;
        evaluated
    }
    fn retire_timing(&mut self) {
        if metrics_enabled() {
            return;
        }
        self.timing_unavailable = false;
        // SAFETY: the live fence.
        let pending = self.last_timing_fence != 0
            && self
                .fence
                .as_ref()
                .is_some_and(|f| unsafe { f.GetCompletedValue() } < self.last_timing_fence);
        if self.queries.is_none() || pending {
            return;
        }
        for slot in self.work.iter_mut() {
            slot.timing = false;
        }
        self.queries = None;
        self.readback = None;
        self.last_timing_fence = 0;
        self.frequency = 0;
    }
    unsafe fn timing_ready(&mut self) -> bool {
        if !metrics_enabled() {
            self.retire_timing();
            return false;
        }
        if self.queries.is_some() {
            return true;
        }
        if self.timing_unavailable {
            return false;
        }
        let query = D3D12_QUERY_HEAP_DESC {
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            Count: 12,
            NodeMask: 0,
        };
        let read = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 96,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let mut created_queries: Option<ID3D12QueryHeap> = None;
        let mut created_readback: Option<ID3D12Resource> = None;
        let frequency = if self
            .device
            .CreateQueryHeap(&query, &mut created_queries)
            .is_err()
            || self
                .device
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_NONE,
                    &read,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut created_readback,
                )
                .is_err()
        {
            0
        } else {
            self.queue.GetTimestampFrequency().unwrap_or(0)
        };
        if frequency == 0 || created_queries.is_none() || created_readback.is_none() {
            self.timing_unavailable = true;
            return false;
        }
        self.frequency = frequency;
        self.queries = created_queries;
        self.readback = created_readback;
        true
    }
    /// What differs from the generator's creation (its log line): the device, the queue, a setting or an eye's sizes
    /// and camera.
    unsafe fn change_reason(
        &self,
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
        colors: [&ID3D12Resource; 2],
        inputs: [&Inputs; 2],
        s: &Settings,
    ) -> String {
        if self.device.as_raw() != device.as_raw() {
            return String::from("another device");
        }
        if self.queue.as_raw() != queue.as_raw() {
            return format!("another queue ({:?} -> {:?})", self.queue.as_raw(), queue.as_raw());
        }
        if s.backend != self.settings.backend || s.quality != self.settings.quality || s.flow_scale != self.settings.flow_scale {
            return format!(
                "settings backend {}->{} quality {}->{} flow {}->{}",
                self.settings.backend, s.backend, self.settings.quality, s.quality, self.settings.flow_scale, s.flow_scale
            );
        }
        let mut out = String::new();
        for i in 0..2 {
            let d = colors[i].GetDesc();
            let e = &self.signatures[i];
            let input = inputs[i];
            let c = &input.camera;
            out.push_str(&format!(
                "eye {i}: color {}x{} -> {}x{}, render {}x{} -> {}x{}, motion {}x{} -> {}x{}, inverted {}->{} infinite {}->{} jittered {}->{}; ",
                e.width, e.height, d.Width, d.Height, e.render_width, e.render_height, input.width, input.height,
                e.motion_width, e.motion_height, input.motion_width, input.motion_height,
                e.inverted, c.inverted, e.infinite, c.infinite, e.jittered, c.jittered
            ));
        }
        out
    }
    unsafe fn compatible(
        &self,
        colors: [&ID3D12Resource; 2],
        inputs: [&Inputs; 2],
        s: &Settings,
    ) -> bool {
        if s.backend != self.settings.backend
            || s.quality != self.settings.quality
            || s.flow_scale != self.settings.flow_scale
        {
            return false;
        }
        for i in 0..2 {
            let d = colors[i].GetDesc();
            let eye = &self.signatures[i];
            let input = inputs[i];
            let camera = &input.camera;
            if d.Width != eye.width
                || d.Height != eye.height
                || input.width != eye.render_width
                || input.height != eye.render_height
                || input.motion_width != eye.motion_width
                || input.motion_height != eye.motion_height
                || camera.inverted != eye.inverted
                || camera.infinite != eye.infinite
                || camera.jittered != eye.jittered
            {
                return false;
            }
        }
        true
    }
    /// One of the generator's own passes: three sources and three targets bound from the slot's heap.
    #[allow(clippy::too_many_arguments)]
    unsafe fn dispatch(
        &mut self,
        w: usize,
        list: &ID3D12GraphicsCommandList,
        pass: usize,
        color: *mut c_void,
        color_format: DXGI_FORMAT,
        raw_source: Option<(*mut c_void, DXGI_FORMAT)>,
        previous: *mut c_void,
        outputs: [*mut c_void; 3],
        size: [u32; 4],
        reset: bool,
        clear_scd: bool,
    ) {
        let device = &self.device;
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        let slot = &mut self.work[w];
        let Some(heap) = slot.heap.clone() else {
            return;
        };
        let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
        cpu.ptr += slot.descriptors as usize * step as usize;
        let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
        gpu.ptr += slot.descriptors as u64 * step as u64;
        slot.descriptors += 6;
        let sources = [
            color,
            raw_source.map_or(core::ptr::null_mut(), |r| r.0),
            previous,
        ];
        let formats = [
            color_format,
            raw_source.map_or(DXGI_FORMAT_R16G16_SINT, |r| r.1),
            DXGI_FORMAT_R8G8B8A8_UNORM,
        ];
        for i in 0..3 {
            let d = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: formats[i],
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: 1,
                        ..Default::default()
                    },
                },
            };
            device.CreateShaderResourceView(
                ID3D12Resource::from_raw_borrowed(&sources[i]),
                Some(&d),
                D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: cpu.ptr + i * step as usize,
                },
            );
        }
        let out_formats = [
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_R16G16_SINT,
            DXGI_FORMAT_R32_UINT,
        ];
        for i in 0..3 {
            let d = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                Format: out_formats[i],
                ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            device.CreateUnorderedAccessView(
                ID3D12Resource::from_raw_borrowed(&outputs[i]),
                None::<&ID3D12Resource>,
                Some(&d),
                D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: cpu.ptr + (i + 3) * step as usize,
                },
            );
        }
        list.SetDescriptorHeaps(&[Some(heap.clone())]);
        if let Some(root) = &self.root {
            list.SetComputeRootSignature(root);
        }
        list.SetPipelineState(self.pipelines[pass].as_ref());
        list.SetComputeRootDescriptorTable(0, gpu);
        list.SetComputeRootDescriptorTable(
            1,
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr + 3 * step as u64,
            },
        );
        let [sw, sh, dw, dh] = size;
        let params = [sw, sh, dw, dh, reset as u32, clear_scd as u32];
        list.SetComputeRoot32BitConstants(2, 6, params.as_ptr() as *const c_void, 0);
        if pass == 2 {
            list.Dispatch(1, 1, 1);
        } else {
            list.Dispatch(dw.div_ceil(8), dh.div_ceil(8), 1);
        }
        let uav = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                    pResource: ManuallyDrop::new(None),
                }),
            },
        };
        list.ResourceBarrier(&[uav]);
    }
    unsafe fn submit(&mut self, list: &ID3D12GraphicsCommandList, w: usize) -> bool {
        if list.Close().is_err() {
            return false;
        }
        let Ok(command_list) = list.cast::<ID3D12CommandList>() else {
            return false;
        };
        self.queue.ExecuteCommandLists(&[Some(command_list)]);
        self.value += 1;
        self.work[w].fence = self.value;
        let Some(fence) = &self.fence else {
            return false;
        };
        self.queue.Signal(fence, self.value).is_ok()
    }
    unsafe fn prepare_inputs(
        &mut self,
        w: usize,
        list: &ID3D12GraphicsCommandList,
        eye: usize,
        inputs: &Inputs,
    ) {
        let Some(e) = self.eyes[eye].as_mut() else {
            return;
        };
        if !e.prepare_inputs {
            return;
        }
        e.prepared_depth.to(list, UAV);
        e.prepared_motion.to(list, UAV);
        let device = &self.device;
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        let slot = &mut self.work[w];
        let Some(heap) = slot.heap.clone() else {
            return;
        };
        let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
        cpu.ptr += slot.descriptors as usize * step as usize;
        let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
        gpu.ptr += slot.descriptors as u64 * step as u64;
        slot.descriptors += 6;
        let sources = [
            raw(inputs.depth.as_ref()),
            raw(inputs.motion.as_ref()),
            core::ptr::null_mut(),
        ];
        for i in 0..3 {
            let source = ID3D12Resource::from_raw_borrowed(&sources[i]);
            let d = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: source.map_or(DXGI_FORMAT_R32_FLOAT, |r| r.GetDesc().Format),
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: 1,
                        ..Default::default()
                    },
                },
            };
            device.CreateShaderResourceView(
                source,
                Some(&d),
                D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: cpu.ptr + i * step as usize,
                },
            );
        }
        let targets = [Some(&e.prepared_depth), Some(&e.prepared_motion), None];
        for (i, t) in targets.iter().enumerate() {
            let target = t.map_or(core::ptr::null_mut(), |t| t.raw());
            let d = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                Format: t.map_or(DXGI_FORMAT_R32_UINT, |t| t.format),
                ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            device.CreateUnorderedAccessView(
                ID3D12Resource::from_raw_borrowed(&target),
                None::<&ID3D12Resource>,
                Some(&d),
                D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: cpu.ptr + (i + 3) * step as usize,
                },
            );
        }
        list.SetDescriptorHeaps(&[Some(heap.clone())]);
        if let Some(root) = &self.root {
            list.SetComputeRootSignature(root);
        }
        list.SetPipelineState(self.pipelines[3].as_ref());
        list.SetComputeRootDescriptorTable(0, gpu);
        list.SetComputeRootDescriptorTable(
            1,
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr + 3 * step as u64,
            },
        );
        let params = [
            inputs.width,
            inputs.height,
            e.render_width,
            e.render_height,
            inputs.camera.inverted as u32,
            0,
        ];
        list.SetComputeRoot32BitConstants(2, 6, params.as_ptr() as *const c_void, 0);
        list.Dispatch(e.render_width.div_ceil(8), e.render_height.div_ceil(8), 1);
        e.prepared_depth.to(list, READ);
        e.prepared_motion.to(list, READ);
    }
    fn poll(&mut self) {
        let Some(fence) = &self.fence else { return };
        // SAFETY: the live fence.
        let completed = unsafe { fence.GetCompletedValue() };
        for old in self.work.iter_mut() {
            if old.fence != 0 && completed >= old.fence {
                old.inputs[0] = None;
                old.inputs[1] = None;
            }
        }
    }
    unsafe fn generate(
        &mut self,
        colors: [&ID3D12Resource; 2],
        inputs: [*const Inputs; 2],
        frame_ms: f64,
        reset: bool,
        frame_id: u64,
    ) -> bool {
        self.poll();
        let cursor = self.cursor as usize;
        let Some(fence) = self.fence.clone() else {
            return false;
        };
        if self.work[cursor].fence != 0 && fence.GetCompletedValue() < self.work[cursor].fence {
            framegen_core::cvr_framegen_on_skip();
            return false;
        }
        let timed = self.timing_ready();
        if timed && self.work[cursor].timing {
            // FOUR STAMPS A PAIR: the first list (colour packing, optical flow inputs) begins and ends, the finishing
            // list (flow resolve, interpolation) begins and ends. Between the two the queue waits on NVIDIA's optical
            // flow engine and runs whatever else was submitted meanwhile, so the first stamp to the last is the
            // generation's latency, not its cost: the cost is the two lists' own spans.
            if let Some(readback) = &self.readback {
                let mut pointer: *mut c_void = core::ptr::null_mut();
                let range = D3D12_RANGE {
                    Begin: cursor * 32,
                    End: cursor * 32 + 32,
                };
                if readback.Map(0, Some(&range), Some(&mut pointer)).is_ok() {
                    let times = (pointer as *const u8).add(cursor * 32) as *const u64;
                    let t = [*times, *times.add(1), *times.add(2), *times.add(3)];
                    if t[1] >= t[0] && t[3] >= t[2] && t[2] >= t[1] {
                        let f = self.frequency as f64;
                        framegen_core::cvr_framegen_on_generation(
                            ((t[1] - t[0]) + (t[3] - t[2])) as f64 * 1000.0 / f,
                            (t[3] - t[0]) as f64 * 1000.0 / f,
                        );
                    }
                    let none = D3D12_RANGE::default();
                    readback.Unmap(0, Some(&none));
                }
            }
        }
        self.work[cursor].timing = false;
        let (Some(allocator), Some(list), Some(finish_allocator), Some(finish)) = (
            self.work[cursor].allocator.clone(),
            self.work[cursor].list.clone(),
            self.work[cursor].finish_allocator.clone(),
            self.work[cursor].finish.clone(),
        ) else {
            return false;
        };
        if allocator.Reset().is_err()
            || list.Reset(&allocator, None).is_err()
            || finish_allocator.Reset().is_err()
            || finish.Reset(&finish_allocator, None).is_err()
        {
            return false;
        }
        self.work[cursor].descriptors = 0;
        self.work[cursor].inputs = [Some(lease(inputs[0])), Some(lease(inputs[1]))];
        let input = [&*inputs[0], &*inputs[1]];
        let queries = self.queries.clone();
        let stamp = |list: &ID3D12GraphicsCommandList, q: u32| {
            if let Some(queries) = &queries {
                list.EndQuery(queries, D3D12_QUERY_TYPE_TIMESTAMP, cursor as u32 * 4 + q);
            }
        };
        let resolve = |list: &ID3D12GraphicsCommandList, readback: &Option<ID3D12Resource>| {
            if let (Some(queries), Some(readback)) = (&queries, readback) {
                list.ResolveQueryData(
                    queries,
                    D3D12_QUERY_TYPE_TIMESTAMP,
                    cursor as u32 * 4,
                    4,
                    readback,
                    cursor as u64 * 32,
                );
            }
        };
        let mut list = list;
        if timed {
            stamp(&list, 0);
        }
        if ngx_backend(self.mode) {
            let _ = finish.Close();
            for i in 0..2 {
                if !self.record_dlss(&list, i, colors[i], input[i], reset) {
                    self.failed = true;
                    status(
                        if self.mode == ANR { c"ANR FG evaluation failed" } else { c"DLSS Frame Generation evaluation failed" },
                        false,
                    );
                    self.submit(&list, cursor);
                    return false;
                }
            }
            if timed {
                for q in 1..4 {
                    stamp(&list, q);
                }
                resolve(&list, &self.readback);
            }
            let submitted = self.submit(&list, cursor);
            if timed {
                self.last_timing_fence = self.value;
            }
            if !submitted {
                self.failed = true;
                return false;
            }
            self.work[cursor].timing = timed;
            self.cursor = (self.cursor + 1) % 3;
            return true;
        }
        let nvidia = self.settings.backend == NVIDIA;
        let optical_flow = self.eyes[0].as_ref().is_some_and(|e| e.optical_flow);
        let mut i = 0;
        while i < 2 && optical_flow {
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            let current = eye.current as usize;
            barrier(&list, Some(colors[i]), COPY, READ);
            eye.packed[current].to(&list, UAV);
            let (packed, width, height, fw, fh) = (
                eye.packed[current].raw(),
                eye.width,
                eye.height,
                eye.flow_width,
                eye.flow_height,
            );
            self.dispatch(
                cursor,
                &list,
                0,
                colors[i].as_raw(),
                DXGI_FORMAT_R8G8B8A8_UNORM,
                None,
                core::ptr::null_mut(),
                [packed, core::ptr::null_mut(), core::ptr::null_mut()],
                [width, height, fw, fh],
                reset,
                false,
            );
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            eye.packed[current].to(&list, D3D12_RESOURCE_STATE_COMMON);
            barrier(&list, Some(colors[i]), READ, COPY);
            if !nvidia {
                let d = FfxOpticalflowDispatchDescription {
                    command_list: ffxGetCommandListDX12(list.as_raw()),
                    color: texture_resource(&eye.packed[current], FFX_RESOURCE_STATE_COMMON),
                    optical_flow_vector: texture_resource(
                        &eye.raw_flow,
                        FFX_RESOURCE_STATE_UNORDERED_ACCESS,
                    ),
                    optical_flow_scd: texture_resource(
                        &eye.scd,
                        FFX_RESOURCE_STATE_UNORDERED_ACCESS,
                    ),
                    reset,
                    backbuffer_transfer_function: FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB,
                    min_max_luminance: FfxFloatCoords2D { x: 0.0, y: 1.0 },
                };
                if !ffx(
                    ffxOpticalflowContextDispatch(eye.of.ptr(), &d),
                    "Optical flow dispatch",
                ) {
                    self.failed = true;
                    self.submit(&list, cursor);
                    let _ = finish.Close();
                    return false;
                }
            }
            i += 1;
        }
        if optical_flow {
            // The optical flow's inputs are submitted on their own: NVIDIA's OFA runs between the two lists.
            if timed {
                stamp(&list, 1);
            }
            let packed_submitted = self.submit(&list, cursor);
            if timed {
                self.last_timing_fence = self.value;
            }
            if !packed_submitted {
                self.failed = true;
                let _ = finish.Close();
                return false;
            }
            let packed_value = self.value;
            if nvidia && !reset {
                for e in 0..2 {
                    let Some(eye) = self.eyes[e].as_mut() else {
                        return false;
                    };
                    let current = eye.current;
                    let ran = eye.nv.run(&fence, packed_value, current);
                    let waited = ran
                        && eye
                            .nv
                            .fence
                            .as_ref()
                            .is_some_and(|f| self.queue.Wait(f, eye.nv.value).is_ok());
                    if !waited {
                        self.failed = true;
                        status(c"NVIDIA optical flow failed", false);
                        let _ = finish.Close();
                        return false;
                    }
                }
            }
            list = finish.clone();
            if timed {
                stamp(&list, 2);
            }
        } else {
            let _ = finish.Close(); // no optical flow: one list for the whole pair
            if timed {
                for q in 1..3 {
                    stamp(&list, q);
                }
            }
        }
        for i in 0..2 {
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            if eye.optical_flow {
                let raw_state = eye.raw_flow.state;
                eye.raw_flow.to(&list, READ);
                let (raw_flow, raw_format, rfw, rfh) = (
                    eye.raw_flow.raw(),
                    eye.raw_flow.format,
                    eye.raw_flow.width,
                    eye.raw_flow.height,
                );
                let (flow, fw, fh) = (eye.flow.raw(), eye.flow.width, eye.flow.height);
                self.dispatch(
                    cursor,
                    &list,
                    1,
                    core::ptr::null_mut(),
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    Some((raw_flow, raw_format)),
                    core::ptr::null_mut(),
                    [core::ptr::null_mut(), flow, core::ptr::null_mut()],
                    [rfw, rfh, fw, fh],
                    reset,
                    false,
                );
                if let Some(eye) = self.eyes[i].as_mut() {
                    eye.raw_flow.to(&list, raw_state);
                }
            } else {
                let (flow, scd, fw, fh) = (
                    eye.flow.raw(),
                    eye.scd.raw(),
                    eye.flow.width,
                    eye.flow.height,
                );
                self.dispatch(
                    cursor,
                    &list,
                    1,
                    core::ptr::null_mut(),
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    None,
                    core::ptr::null_mut(),
                    [core::ptr::null_mut(), flow, scd],
                    [1, 1, fw, fh],
                    true,
                    true,
                );
            }
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            if nvidia && eye.optical_flow {
                let current = eye.current as usize;
                let previous = if reset { current } else { current ^ 1 };
                eye.packed[current].to(&list, READ);
                if previous != current {
                    eye.packed[previous].to(&list, READ);
                }
                let (cur, cur_format, prev, scd, fw, fh) = (
                    eye.packed[current].raw(),
                    eye.packed[current].format,
                    eye.packed[previous].raw(),
                    eye.scd.raw(),
                    eye.flow_width,
                    eye.flow_height,
                );
                self.dispatch(
                    cursor,
                    &list,
                    2,
                    cur,
                    cur_format,
                    None,
                    prev,
                    [core::ptr::null_mut(), core::ptr::null_mut(), scd],
                    [fw, fh, 3, 1],
                    reset,
                    false,
                );
                let Some(eye) = self.eyes[i].as_mut() else {
                    return false;
                };
                eye.packed[current].to(&list, D3D12_RESOURCE_STATE_COMMON);
                if previous != current {
                    eye.packed[previous].to(&list, D3D12_RESOURCE_STATE_COMMON);
                }
            }
            self.prepare_inputs(cursor, &list, i, input[i]);
            let Some(eye) = self.eyes[i].as_mut() else {
                return false;
            };
            if !eye.interpolate(
                &list, colors[i], input[i], frame_ms, reset, frame_id, nvidia,
            ) {
                self.failed = true;
                self.submit(&list, cursor);
                return false;
            }
            if nvidia && eye.optical_flow {
                eye.current ^= 1;
            }
        }
        if timed {
            stamp(&list, 3);
            resolve(&list, &self.readback);
        }
        let finished_submitted = self.submit(&list, cursor);
        if timed {
            self.last_timing_fence = self.value;
        }
        if !finished_submitted {
            self.failed = true;
            return false;
        }
        self.work[cursor].timing = timed;
        self.cursor = (self.cursor + 1) % 3;
        true
    }
}

/// A test switch: a file of this name next to the game executable.
unsafe fn test_marker(name: &str) -> bool {
    let mut path = [0u16; 260];
    let n = GetModuleFileNameW(None, &mut path) as usize;
    if n == 0 {
        return false;
    }
    let path = String::from_utf16_lossy(&path[..n]);
    let folder = path.rfind('\\').map_or("", |cut| &path[..=cut]);
    let full: Vec<u16> = format!("{folder}{name}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    GetFileAttributesW(full.as_ptr()) != u32::MAX
}

/// NVIDIA's optical-flow engine needs an NVIDIA GPU and its driver library. Anywhere else (AMD, Intel) the FidelityFX
/// optical flow stands in, so the NVIDIA setting -- the default -- still generates.
unsafe fn nvidia_optical_flow(device: &ID3D12Device) -> bool {
    static CHECKED: AtomicUsize = AtomicUsize::new(0);
    static AVAILABLE: AtomicBool = AtomicBool::new(false);
    if device.as_raw() as usize == CHECKED.load(Relaxed) {
        return AVAILABLE.load(Relaxed);
    }
    CHECKED.store(device.as_raw() as usize, Relaxed);
    let mut available = false;
    let mut vendor = 0u32;
    if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory4>() {
        if let Ok(adapter) = factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()) {
            if let Ok(desc) = adapter.GetDesc1() {
                vendor = desc.VendorId;
                if desc.VendorId == 0x10DE {
                    if let Ok(module) =
                        LoadLibraryExW(w!("nvofapi64.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                    {
                        available = true;
                        let _ = FreeLibrary(module);
                    }
                }
            }
        }
    }
    // TEST ONLY: cvr_simulate_amd.txt next to the executable makes an NVIDIA GPU take the AMD path.
    if available && test_marker("cvr_simulate_amd.txt") {
        available = false;
        vendor = 0x1002;
        log("[framegen] TEST: non-NVIDIA GPU simulated (cvr_simulate_amd.txt)\n");
    }
    log(&format!(
        "[framegen] GPU vendor 0x{vendor:04X}: NVIDIA optical flow {}\n",
        if available {
            "available"
        } else {
            "unavailable, FidelityFX optical flow is used"
        }
    ));
    AVAILABLE.store(available, Relaxed);
    available
}

/// cvr::framegen::Generator's state: the backend, created on the first pair and again whenever the frames change.
#[derive(Default)]
pub struct Generator {
    state: Option<Box<Impl>>,
}
impl Generator {
    fn idle(&self) -> bool {
        self.state.as_ref().is_none_or(|s| s.idle())
    }
    fn reset(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        self.state = None;
        true
    }
    fn bytes(&mut self) -> u64 {
        let mut result = 0u64;
        if let Some(s) = self.state.as_mut() {
            for eye in s.eyes.iter_mut().flatten() {
                result = result.wrapping_add(eye.bytes());
            }
        }
        result
    }
}

#[no_mangle]
pub extern "C" fn cvr_framegen_generator_new() -> *mut Generator {
    Box::into_raw(Box::default())
}
/// ~Generator: session teardown normally drained the queue; never free an unknown live allocation if a device
/// shutdown bypassed that drain.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_free(generator: *mut Generator) {
    if generator.is_null() {
        return;
    }
    let mut g = Box::from_raw(generator);
    if g.state.as_ref().is_some_and(|s| !s.idle()) {
        log("[framegen] retaining in-flight resources through shutdown\n");
        if let Some(state) = g.state.take() {
            core::mem::forget(state);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_idle(generator: *const Generator) -> bool {
    generator.as_ref().is_none_or(|g| g.idle())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_poll(generator: *mut Generator) {
    if let Some(s) = generator.as_mut().and_then(|g| g.state.as_mut()) {
        s.poll();
    }
}
/// Nonblocking retirement: false means a previous GPU consumer still owns it.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_reset(generator: *mut Generator) -> bool {
    generator.as_mut().is_none_or(|g| g.reset())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_bytes(generator: *mut Generator) -> u64 {
    generator.as_mut().map_or(0, |g| g.bytes())
}
/// The eye's interpolated frame (the generator keeps the reference).
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_output(
    generator: *const Generator,
    eye: u32,
) -> *mut c_void {
    let Some(s) = generator.as_ref().and_then(|g| g.state.as_ref()) else {
        return core::ptr::null_mut();
    };
    if eye >= 2 {
        return core::ptr::null_mut();
    }
    s.eyes[eye as usize]
        .as_ref()
        .map_or(core::ptr::null_mut(), |e| e.output.raw())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_generator_consumer_fence(
    generator: *mut Generator,
    fence: *mut c_void,
    value: u64,
) {
    if let Some(s) = generator.as_mut().and_then(|g| g.state.as_mut()) {
        s.consumer_fence = ID3D12Fence::from_raw_borrowed(&fence).cloned();
        s.consumer_value = value;
    }
}
/// Generator::Generate: the pair's frame captured when armed, the backend chosen (DLSS-G needs an RTX 40/50 and the
/// SDK in this build, NVIDIA's flow its engine; FidelityFX runs anywhere), the backend (re)created when the frames
/// changed, and the pair generated.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_framegen_generator_generate(
    generator: *mut Generator,
    device: *mut c_void,
    queue: *mut c_void,
    colors: *const *mut c_void,
    inputs: *const *const Inputs,
    cameras: *const [[EyeCamera; 2]; 2],
    settings: *const Settings,
    frame_ms: f64,
    reset: bool,
    frame_id: u64,
    interpolated: *mut bool,
) -> bool {
    if !interpolated.is_null() {
        *interpolated = false;
    }
    let Some(g) = generator.as_mut() else {
        return false;
    };
    let colors_raw = [*colors, *colors.add(1)];
    let inputs = [*inputs, *inputs.add(1)];
    let (Some(device), Some(queue), Some(color0), Some(color1)) = (
        ID3D12Device::from_raw_borrowed(&device),
        ID3D12CommandQueue::from_raw_borrowed(&queue),
        ID3D12Resource::from_raw_borrowed(&colors_raw[0]),
        ID3D12Resource::from_raw_borrowed(&colors_raw[1]),
    ) else {
        return false;
    };
    if inputs[0].is_null() || inputs[1].is_null() {
        return false;
    }
    let colors = [color0, color1];
    let input = [&*inputs[0], &*inputs[1]];
    capture_frame(device, queue, &colors_raw, input, &*cameras);
    let mut effective = *settings;
    if ngx_backend(effective.backend) && !crate::dlssg_native::available(device.as_raw(), provider_of(effective.backend)) {
        effective.backend = FIDELITY_FX;
    }
    if effective.backend == NVIDIA && (effective.flow_scale == 0 || !nvidia_optical_flow(device)) {
        effective.backend = FIDELITY_FX;
    }
    framegen_core::cvr_framegen_publish_active_backend(effective.backend);
    let mut reset = reset;
    let changed = g.state.as_ref().is_some_and(|s| {
        s.device.as_raw() != device.as_raw()
            || s.queue.as_raw() != queue.as_raw()
            || !s.compatible(colors, input, &effective)
    });
    if changed {
        if let Some(s) = g.state.as_ref() {
            log(&format!("[framegen] the generator recreated: {}\n", s.change_reason(device, queue, colors, input, &effective)));
        }
        if !g.reset() {
            return false;
        }
        reset = true;
    }
    if let Some(s) = g.state.as_mut() {
        s.retire_timing();
        if s.failed {
            status(
                c"Backend failed; toggle generation off/on or change the backend",
                false,
            );
            return false;
        }
    }
    if g.state.is_none() {
        let mut s = Box::new(Impl {
            device: device.clone(),
            queue: queue.clone(),
            fence: None,
            consumer_fence: None,
            value: 0,
            consumer_value: 0,
            frequency: 0,
            root: None,
            pipelines: Default::default(),
            mode: effective.backend,
            queries: None,
            readback: None,
            last_timing_fence: 0,
            timing_unavailable: false,
            work: Default::default(),
            eyes: [None, None],
            settings: effective,
            cursor: 0,
            failed: false,
            signatures: [Signature::default(); 2],
        });
        let created = s.create(colors, input);
        if !created {
            s.failed = true;
        }
        g.state = Some(s);
        if !created {
            status(c"Framegen backend initialization failed", false);
            return false;
        }
        reset = true;
    }
    let Some(s) = g.state.as_mut() else {
        return false;
    };
    let result = s.generate(colors, inputs, frame_ms, reset, frame_id);
    if result && !interpolated.is_null() {
        *interpolated = !reset;
    }
    if metrics_enabled() {
        let bytes = g
            .bytes()
            .wrapping_add(crate::framegen_inputs::cvr_framegen_input_vram());
        framegen_core::cvr_framegen_set_vram(bytes);
    }
    result
}

/// CaptureFrame (src/Framegen/FrameCapture.cpp): each eye's inputs and the camera it is shown with, to the capture.
unsafe fn capture_frame(
    device: &ID3D12Device,
    queue: &ID3D12CommandQueue,
    colors: &[*mut c_void; 2],
    inputs: [&Inputs; 2],
    cameras: &[[EyeCamera; 2]; 2],
) {
    if CyberpunkVR_FramegenCapture.load(Relaxed) != 1 {
        return;
    }
    let eye = |e: usize| CaptureEye {
        depth: raw(inputs[e].depth.as_ref()),
        motion: raw(inputs[e].motion.as_ref()),
        camera: &inputs[e].camera,
        eye: &cameras[e][1],
        pose_id: inputs[e].pose_id,
        frame_id: inputs[e].frame_id,
        has_motion: inputs[e].has_motion,
    };
    let eyes = [eye(0), eye(1)];
    frame_capture::cvr_gpu_framegen_capture_frame(
        &raw const CyberpunkVR_FramegenCapture,
        device.as_raw(),
        queue.as_raw(),
        colors.as_ptr(),
        eyes.as_ptr(),
    );
}
