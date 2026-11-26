//! THE NATIVE PROBE'S PIPELINE OBSERVATION (formerly src/Render/NativeStereoProbe.cpp's Enabled, RootCreated,
//! GraphicsCreated and StreamCreated): the probe's offline-validated shader assets (loaded once by
//! stereo_probe_pipelines.rs), a root signature's blob observed, and an engine pipeline -- classic or stream -- observed
//! (its shader snapshot and input layout while bindings are observed or the probe is on) and, when it is one of the
//! probe's target pipelines, registered with its view-instanced variant (the probe's vertex shader and, with a pixel
//! stage, its pixel shader; layer 0 and 1 in one render-target array, masking allowed) through stereo_probe_state.rs.
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT_S8X24_UINT;

use crate::declared_moved_exports::CyberpunkVR_NativeStereoProbeAssetStatus;
use crate::pipeline_stream::{self, PipelineStreamInfo};
use crate::stereo_probe_pipelines::{cvr_nsp_assets, cvr_nsp_target, AssetView};
use crate::stereo_probe_state::{add_pipeline, cvr_nsps_pipeline, PipelineInfo};
use crate::used_moved_exports_rest::CyberpunkVR_NativeStereoStaticAssetStatus;

/// The probe's shaders (the loaded assets' bytes, alive for the process).
struct Shaders {
    vs: D3D12_SHADER_BYTECODE,
    ps: D3D12_SHADER_BYTECODE,
    static_vs: D3D12_SHADER_BYTECODE,
    depth_vs: D3D12_SHADER_BYTECODE,
}
// SAFETY: immutable views of process-lifetime bytes.
unsafe impl Send for Shaders {}
// SAFETY: as above.
unsafe impl Sync for Shaders {}
static SHADERS: OnceLock<Option<Shaders>> = OnceLock::new();
fn bytecode(data: *const u8, bytes: usize) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE { pShaderBytecode: data.cast(), BytecodeLength: bytes }
}
fn shaders() -> Option<&'static Shaders> {
    SHADERS
        .get_or_init(|| {
            let mut v = AssetView {
                vs: null(),
                vs_bytes: 0,
                ps: null(),
                ps_bytes: 0,
                static_vs: null(),
                static_bytes: 0,
                depth_vs: null(),
                depth_bytes: 0,
            };
            // SAFETY: the exported statuses and a local view.
            unsafe { cvr_nsp_assets(&CyberpunkVR_NativeStereoProbeAssetStatus, &CyberpunkVR_NativeStereoStaticAssetStatus, &mut v) }.then(|| Shaders {
                vs: bytecode(v.vs, v.vs_bytes),
                ps: bytecode(v.ps, v.ps_bytes),
                static_vs: bytecode(v.static_vs, v.static_bytes),
                depth_vs: bytecode(v.depth_vs, v.depth_bytes),
            })
        })
        .as_ref()
}
/// Enabled: the probe's shader assets loaded (once).
pub fn enabled() -> bool {
    shaders().is_some()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nspc_enabled() -> bool {
    enabled()
}
fn observe() -> bool {
    crate::stereo_probe::cvr_observe_bindings()
}
fn target(s: &Shaders, vs: &D3D12_SHADER_BYTECODE, ps: &D3D12_SHADER_BYTECODE) -> bool {
    // SAFETY: the pipeline's shader views.
    unsafe { cvr_nsp_target(vs, ps, s.depth_vs.BytecodeLength != 0, s.static_vs.BytecodeLength != 0) }
}
/// The replacement vertex shader by the original's size (the depth, the static or the probe's own).
fn replacement(s: &Shaders, vs_bytes: usize) -> D3D12_SHADER_BYTECODE {
    match vs_bytes {
        3635 => s.depth_vs,
        5238 => s.static_vs,
        _ => s.vs,
    }
}
const LOCATIONS: [D3D12_VIEW_INSTANCE_LOCATION; 2] = [
    D3D12_VIEW_INSTANCE_LOCATION { ViewportArrayIndex: 0, RenderTargetArrayIndex: 0 },
    D3D12_VIEW_INSTANCE_LOCATION { ViewportArrayIndex: 0, RenderTargetArrayIndex: 1 },
];
fn observe_input(original: *mut c_void, input: &D3D12_INPUT_LAYOUT_DESC) {
    // SAFETY: the pipeline's input layout for the call.
    unsafe { cvr_nsps_pipeline(0, null_mut(), original, (input as *const D3D12_INPUT_LAYOUT_DESC).cast(), 0, null(), None, null()) };
}
/// AddPipeline: the registration with the group resolve's support decided here.
fn add(device: *mut c_void, original: *mut c_void, info: &PipelineStreamInfo, samples: u32, create: crate::stereo_probe_state::Create, context: *const c_void) {
    let targets = info.targets.NumRenderTargets;
    let mut lent = PipelineInfo { root: info.root, targets, depth_format: info.depth_format.0 as u32, samples, formats: [0; 8], group_resolve: false };
    for i in 0..(targets.min(8) as usize) {
        lent.formats[i] = info.targets.RTFormats[i].0 as u32;
    }
    let mut blend = info.blend;
    if targets == 0 {
        blend = D3D12_BLEND_DESC::default();
        for b in &mut blend.RenderTarget {
            b.RenderTargetWriteMask = 15;
        }
    }
    lent.group_resolve = (info.have_blend || targets == 0) && info.have_depth && !info.depth_bounds && cvr_gpu::group_resolve::supports(&blend, &info.depth);
    add_pipeline(device, original as usize, &lent, create, context);
}

struct GraphicsVariant {
    source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    shaders: &'static Shaders,
}
unsafe extern "C-unwind" fn graphics_variant(context: *const c_void, device: *mut c_void, out: *mut *mut c_void) -> i32 {
    // SAFETY: graphics_created's context for the call.
    let c = unsafe { &*context.cast::<GraphicsVariant>() };
    // SAFETY: the engine's description, copied bit for bit (its root signature borrowed, never released).
    let mut desc = unsafe { core::ptr::read(c.source) };
    desc.VS = replacement(c.shaders, desc.VS.BytecodeLength);
    if desc.PS.BytecodeLength != 0 {
        desc.PS = c.shaders.ps;
    }
    // SAFETY: the device, the description and the output.
    unsafe { pipeline_stream::cvr_pipeline_stream_create_view_instanced_graphics(device, &desc, LOCATIONS.as_ptr(), true, out) }.0
}
struct StreamVariant {
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    vs_bytes: usize,
    pixel: bool,
    shaders: &'static Shaders,
}
unsafe extern "C-unwind" fn stream_variant(context: *const c_void, device: *mut c_void, out: *mut *mut c_void) -> i32 {
    // SAFETY: stream_created's context for the call.
    let c = unsafe { &*context.cast::<StreamVariant>() };
    let vs = replacement(c.shaders, c.vs_bytes);
    let ps = if c.pixel { c.shaders.ps } else { D3D12_SHADER_BYTECODE::default() };
    // SAFETY: the device, the engine's stream and the outputs.
    unsafe { pipeline_stream::cvr_pipeline_stream_create_view_instanced(device, c.source, &vs, LOCATIONS.as_ptr(), true, out, &ps, null_mut()) }.0
}

/// RootCreated: a root signature's blob, while bindings are observed or the probe is on.
///
/// # Safety
/// `data` holds `bytes` bytes (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nspc_root_created(root: *mut c_void, data: *const c_void, bytes: usize) {
    if root.is_null() || data.is_null() || bytes == 0 || bytes > 65536 || (!observe() && !enabled()) {
        return;
    }
    // SAFETY: as documented.
    unsafe { cvr_nsps_pipeline(3, null_mut(), root, data, bytes, null(), None, null()) };
}
/// GraphicsCreated: a classic pipeline observed and, when targeted, registered with its variant.
///
/// # Safety
/// `source` is the engine's description; the pipeline live (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nspc_graphics_created(device: *mut c_void, source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC, original: *mut c_void) {
    if cvr_host::probe::internal() {
        return;
    }
    // SAFETY: as documented.
    let source = unsafe { &*source };
    if observe() && !device.is_null() {
        // SAFETY: the pipeline and its description.
        unsafe { crate::shader_snapshot_native::cvr_shs_graphics_desc_created(original, source) };
    }
    if observe() || enabled() {
        observe_input(original, &source.InputLayout);
    }
    let Some(s) = shaders() else { return };
    if original.is_null() || !target(s, &source.VS, &source.PS) {
        return;
    }
    if source.PS.BytecodeLength == 0 && (source.NumRenderTargets != 0 || source.DSVFormat != DXGI_FORMAT_D32_FLOAT_S8X24_UINT) {
        return;
    }
    if source.NumRenderTargets > 8 {
        return;
    }
    // SAFETY: a plain record; all zeroes is its default.
    let mut info: PipelineStreamInfo = unsafe { core::mem::zeroed() };
    info.vs = source.VS;
    info.ps = source.PS;
    // SAFETY: the description's root signature pointer, borrowed.
    info.root = unsafe { core::mem::transmute_copy::<core::mem::ManuallyDrop<Option<ID3D12RootSignature>>, *mut c_void>(&source.pRootSignature) };
    info.depth_format = source.DSVFormat;
    info.targets.NumRenderTargets = source.NumRenderTargets;
    info.targets.RTFormats[..source.NumRenderTargets as usize].copy_from_slice(&source.RTVFormats[..source.NumRenderTargets as usize]);
    info.blend = source.BlendState;
    info.depth = source.DepthStencilState;
    info.have_blend = true;
    info.have_depth = true;
    let context = GraphicsVariant { source, shaders: s };
    add(device, original, &info, source.SampleDesc.Count, graphics_variant, (&raw const context).cast());
}
/// StreamCreated: a stream pipeline observed (its input mask forgotten first) and, when targeted, registered.
///
/// # Safety
/// `source` is the engine's stream; the pipeline live (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nspc_stream_created(device: *mut c_void, source: *const D3D12_PIPELINE_STATE_STREAM_DESC, original: *mut c_void) {
    if cvr_host::probe::internal() {
        return;
    }
    if original.is_null() || (!observe() && !enabled()) {
        return;
    }
    // SAFETY: the pipeline's address.
    unsafe { cvr_nsps_pipeline(1, null_mut(), original, null(), 0, null(), None, null()) };
    // SAFETY: as documented.
    let Some(info) = (unsafe { pipeline_stream::read(&*source) }) else { return };
    if observe() && !info.other_stages {
        // SAFETY: the pipeline and its read stream.
        unsafe { crate::shader_snapshot_native::cvr_shs_graphics_stream_created(original, &info) };
    }
    if info.vs.BytecodeLength != 0 && !info.other_stages {
        observe_input(original, &info.input);
    }
    let Some(s) = shaders() else { return };
    if !target(s, &info.vs, &info.ps) {
        return;
    }
    if info.ps.BytecodeLength == 0 && (info.targets.NumRenderTargets != 0 || info.depth_format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT) {
        return;
    }
    let context = StreamVariant { source, vs_bytes: info.vs.BytecodeLength, pixel: info.ps.BytecodeLength != 0, shaders: s };
    add(device, original, &info, info.samples.Count, stream_variant, (&raw const context).cast());
}
