//! The device's pipeline creation hooks (formerly src/Stereo/DeviceHooks.cpp's Hook_CreatePipelineState,
//! Hook_CreateGraphicsPipelineState, Hook_CreateComputePipelineState and history_mask_twin).
//!
//! Stream pipelines: the caller's subobject stream copied through a safe read and parsed there (where the .cpp
//! parsed the caller's memory under SEH). Graphics and compute descriptions: copied bitwise (their root signature
//! reference untouched). The substitutions tried in order -- the vision outline compute shader, the optics' glass
//! pixel shader (graphics: with its lens velocity and history mask twins), the world markers' vertex shader, the
//! sight's pixel and vertex shaders with its scope variants (graphics: and its history mask) as twins --, each falling
//! back to the native description when the driver refuses it; the native pipeline's shader identities recorded, the
//! hidden-area shader twins and the consumers (VRS, single pass, native probe, families) told of it, the counts.
//!
//! The hidden-area shader twins are registered here (their creators own Rust copies of the descriptions), the sight's
//! root signature dumped here. The .cpp keeps the hooks' symbols (the device creation runtime installs them) and lends
//! the originals, the sight twins' index, the consumers and the log; the optics, markers, shader blobs and identities
//! are Rust's.

use crate::pipeline_stream::PipelineStreamOffsets;
use crate::pso_twins::{twin_made, TwinFn};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering::*};
use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

type Bytecode = D3D12_SHADER_BYTECODE;
type Desc = D3D12_PIPELINE_STATE_STREAM_DESC;
type Graphics = D3D12_GRAPHICS_PIPELINE_STATE_DESC;
type Compute = D3D12_COMPUTE_PIPELINE_STATE_DESC;
pub use crate::pso_twins::{CreateGraphics, CreateStream};
pub type CreateCompute = unsafe extern "system" fn(device: *mut c_void, desc: *const Compute, riid: *const GUID, out: *mut *mut c_void) -> HRESULT;
/// The vision outline shader's DXBC digest (the bytes after its "DXBC" magic).
const VISION_DIGEST: [u8; 16] = [0x78, 0x6E, 0x64, 0xC7, 0x28, 0x8B, 0xCE, 0x14, 0xF7, 0x70, 0x6B, 0x9A, 0x61, 0xC2, 0x64, 0xFF];

/// What the consumers lend (formerly src/Stereo/DeviceHooks.cpp's table, now built here).
#[repr(C)]
pub struct Runtime {
    /// The addresses of g_orig_CreatePso, g_orig_CreateGfxPso and g_orig_CreateComputePso (read per call).
    pub original_stream: *const usize,
    pub original_graphics: *const usize,
    pub original_compute: *const usize,
    /// native_probe, families, VRS and single pass told of the pipeline.
    pub stream_created: extern "C" fn(device: *mut c_void, desc: *const Desc, pso: *mut c_void),
    pub graphics_created: extern "C" fn(device: *mut c_void, desc: *const Graphics, pso: *mut c_void),
    pub log: extern "C" fn(text: *const u8),
}
// SAFETY: immutable; its pointers name process-lifetime objects and functions.
unsafe impl Sync for Runtime {}
/// The engine's own pipeline creations (the device's vtable entries before the hooks), written at install by the
/// device creation runtime.
#[no_mangle]
pub static cvr_pso_original_stream: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
#[no_mangle]
pub static cvr_pso_original_graphics: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
#[no_mangle]
pub static cvr_pso_original_compute: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
mod consumers {
    //! The pipeline's consumers: the native probe (stereo_probe_creation.rs), the families (C++, by their MSVC names), VRS
    //! and single pass.
    use super::{Desc, Graphics};
    use core::ffi::c_void;
    use core::sync::atomic::Ordering::Acquire;
    unsafe extern "C" {
        #[link_name = "cvr_nsf2_stream_created"]
        fn families_stream_created(device: *mut c_void, desc: *const Desc, pso: *mut c_void);
        #[link_name = "cvr_nsf2_graphics_created"]
        fn families_graphics_created(device: *mut c_void, desc: *const Graphics, pso: *mut c_void);
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const core::ffi::c_char);
    }
    /// vrs::StreamCreated (formerly src/Render/FixedVrs.cpp's): a stream pipeline with depth and blend, no other
    /// stages, view instancing or forced samples, told to fixed_vrs.rs as the graphics description it amounts to.
    ///
    /// # Safety
    /// `pso` is null or live; `desc` the engine's stream.
    unsafe fn vrs_stream_created(pso: *mut c_void, desc: *const Desc) {
        if pso.is_null() || cvr_host::probe::internal() {
            return;
        }
        // SAFETY: the engine's stream.
        let Some(info) = (unsafe { crate::pipeline_stream::read(&*desc) }) else { return };
        // SAFETY: a plain description; all zeroes its default (no root signature).
        let mut d: Graphics = unsafe { core::mem::zeroed() };
        d.PS = info.ps;
        d.VS = info.vs;
        d.NumRenderTargets = info.targets.NumRenderTargets;
        d.DepthStencilState = info.depth;
        d.BlendState = info.blend;
        d.SampleDesc = info.samples;
        if !info.have_depth || !info.have_blend || info.other_stages || info.view_instancing || info.forced_samples != 0 {
            return;
        }
        // SAFETY: the live pipeline and its description.
        unsafe { crate::fixed_vrs::cvr_vrs_graphics_created(pso, &d) };
    }
    /// The engine's stream creation, as single pass takes it.
    fn original_stream() -> Option<crate::single_pass_pipelines::CreateStreamFn> {
        // SAFETY: the slot holds the engine's function or zero.
        unsafe { core::mem::transmute::<usize, Option<crate::single_pass_pipelines::CreateStreamFn>>(super::cvr_pso_original_stream.load(Acquire)) }
    }
    /// A stream pipeline made: the native probe, the families, VRS and single pass (with the engine's creation).
    pub(super) extern "C" fn stream_created(device: *mut c_void, desc: *const Desc, pso: *mut c_void) {
        // SAFETY: the creation's own device, description and pipeline.
        unsafe {
            crate::stereo_probe_creation::cvr_nspc_stream_created(device, desc, pso);
            families_stream_created(device, desc, pso);
            vrs_stream_created(pso, desc);
            crate::single_pass_pipelines::cvr_single_pass_stream_created(device, desc.cast(), pso, original_stream());
        }
    }
    /// A graphics pipeline made: VRS, single pass, the creation count, the native probe and the families.
    pub(super) extern "C" fn graphics_created(device: *mut c_void, desc: *const Graphics, pso: *mut c_void) {
        // SAFETY: as stream_created's.
        unsafe {
            crate::fixed_vrs::cvr_vrs_graphics_created(pso, desc.cast());
            crate::single_pass_pipelines::cvr_single_pass_graphics_created(device, desc.cast(), pso, original_stream());
        }
        crate::device_creation::cvr_count_creation(5, 0, core::ptr::null());
        // SAFETY: as above.
        unsafe {
            crate::stereo_probe_creation::cvr_nspc_graphics_created(device, desc, pso);
            families_graphics_created(device, desc, pso);
        }
    }
    pub(super) extern "C" fn log(text: *const u8) {
        // SAFETY: a terminated line.
        unsafe { cvr_cpp_stereo_log(text.cast()) };
    }
}
static RT: Runtime = Runtime {
    original_stream: (&raw const cvr_pso_original_stream).cast(),
    original_graphics: (&raw const cvr_pso_original_graphics).cast(),
    original_compute: (&raw const cvr_pso_original_compute).cast(),
    stream_created: consumers::stream_created,
    graphics_created: consumers::graphics_created,
    log: consumers::log,
};
fn rt() -> Option<&'static Runtime> {
    Some(&RT)
}
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    /// shader_blobs.rs's loaders (the stereo log reports a missing blob once).
    fn cvr_dh_sight_ready() -> bool;
    fn cvr_dh_vision_ready() -> bool;
    /// rootsig_lines.rs's dump of a root signature's blob.
    fn cvr_dh_rootsig_dump(rs: *const c_void, blob: *const u8, size: usize);
}
fn diagnostics() -> bool {
    // SAFETY: the C++ export, read as the .cpp's CVR_DIAGNOSTIC did.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 }
}
macro_rules! bump {
    ($export:ident) => {
        if diagnostics() {
            // SAFETY: the Rust-defined export, a plain counter.
            unsafe { crate::used_moved_exports::$export = crate::used_moved_exports::$export.wrapping_add(1) };
        }
    };
}
/// The stereo log; pointers are written as MSVC's %p writes them (sixteen upper-case hex digits).
fn log(r: &Runtime, text: String) {
    (r.log)(format!("{text}\0").as_ptr());
}
// ---- the sight pipelines' twins (formerly DeviceHooks.cpp's PipelineVariantIndex<1> g_sight_twins): every sight
// pipeline gets a twin built with the scope variant of the pixel shader (CyberpunkVR_SightPsScope.dxil), bound while
// the optics path is 'shader', and a history-mask twin bound while it is 'camera'.

const SIGHT_KEY: windows::core::GUID =
    windows::core::GUID::from_values(0x93e581cd, 0x1498, 0x454d, [0x8b, 0x9c, 0x73, 0x6d, 0xf4, 0x09, 0x31, 0x7b]);
static SIGHT_TWINS: cvr_gpu::pipeline_variants::VariantIndex = cvr_gpu::pipeline_variants::VariantIndex::new(SIGHT_KEY);
/// sight_twin_add: the sight's scope variant and mask twin (both creation references taken; nothing to keep when
/// both are null).
fn sight_twin_add(r: &Runtime, sight: *mut c_void, variant: *mut c_void, mask: *mut c_void) {
    if variant.is_null() && mask.is_null() {
        return;
    }
    // SAFETY: the created sight pipeline; the twins' references are handed over.
    let hr = unsafe { SIGHT_TWINS.store(sight, variant, mask) };
    if hr == windows::Win32::Foundation::S_OK {
        // SAFETY: the Rust-defined export, a counter.
        let n = unsafe { core::sync::atomic::AtomicU64::from_ptr(&raw mut crate::used_moved_exports::CyberpunkVR_SightScopeTwins) }
            .fetch_add(1, SeqCst)
            + 1;
        log(r, format!("[pso] sight scope twin {n}: {:016X} -> {:016X}", sight as usize, variant as usize));
    } else if hr.is_err() {
        log(r, format!("[pso] sight variants attachment failed hr={:08X}", hr.0 as u32));
    }
}
fn optics_export(at: *const i32) -> i32 {
    // SAFETY: a Rust-defined export, an int the controls write.
    unsafe { core::ptr::read_volatile(at) }
}
/// sight_scope_twin: the scope twin of a sight pipeline while the optics path (or the diagnostics' force) wants it,
/// else the pipeline itself.
pub fn sight_scope_twin(pso: *mut c_void) -> *mut c_void {
    use crate::used_moved_exports as x;
    let force = optics_export(&raw const x::CyberpunkVR_SightTwinForce);
    let path = optics_export(&raw const x::CyberpunkVR_OpticsPath);
    let variant = if force >= 0 { force - 1 } else if path == 2 { 0 } else { -1 };
    if variant != 0 || optics_export(&raw const x::CyberpunkVR_SightScopeTwin) == 0 {
        return pso;
    }
    SIGHT_TWINS.find(pso as usize).and_then(|o| o.variant.as_ref()).map_or(pso, |twin| twin.as_raw())
}
/// sight_mask_twin: the history-mask twin of a sight pipeline while the optics path is 'camera' (the camera scope's
/// image surface; sight_mask_ps discards every other sight material), else null.
pub fn sight_mask_twin(pso: *mut c_void) -> *mut c_void {
    if optics_export(&raw const crate::used_moved_exports::CyberpunkVR_OpticsPath) != 3 || pso.is_null() {
        return core::ptr::null_mut();
    }
    SIGHT_TWINS.find(pso as usize).and_then(|o| o.mask.as_ref()).map_or(core::ptr::null_mut(), |mask| mask.as_raw())
}
/// The list detours' twin: 0 the scope twin, 1 the mask twin.
pub fn sight_twin(which: u32, pso: *mut c_void) -> *mut c_void {
    if which != 0 {
        sight_mask_twin(pso)
    } else {
        sight_scope_twin(pso)
    }
}
fn blob(which: u32) -> Option<&'static [u8]> {
    crate::shader_blobs::blob(which)
}
fn bytecode(bytes: &[u8]) -> Bytecode {
    Bytecode { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() }
}
fn first(out: *mut *mut c_void) -> *mut c_void {
    if out.is_null() {
        core::ptr::null_mut()
    } else {
        // SAFETY: the caller's out-pointer.
        unsafe { *out }
    }
}
fn created(out: *mut *mut c_void) -> bool {
    !first(out).is_null()
}
fn record(pso: *mut c_void, ps: &Bytecode, vs: &Bytecode) {
    // SAFETY: a created pipeline and its shaders.
    unsafe { crate::census_pso::cvr_census_pso_ids_record(pso, ps, vs) };
}
fn fnv(code: &Bytecode) -> u64 {
    // SAFETY: the shader's bytes.
    unsafe { crate::census_pso::cvr_census_fnv1a(code.pShaderBytecode.cast(), code.BytecodeLength) }
}
fn vision_target(cs: &Bytecode) -> bool {
    // SAFETY: the Rust-defined switch.
    if unsafe { crate::used_moved_exports::CyberpunkVR_VisionCsSwap } == 0 || cs.pShaderBytecode.is_null() || cs.BytecodeLength < 20 {
        return false;
    }
    let mut head = [0u8; 20];
    cvr_engine::stack::read(cs.pShaderBytecode as u64, &mut head) && &head[..4] == b"DXBC" && head[4..20] == VISION_DIGEST && unsafe { cvr_dh_vision_ready() }
}
fn sight_target(ps: &Bytecode, vs: &Bytecode) -> bool {
    // SAFETY: the Rust-defined exports.
    let (swap, ps_hash, vs_hash) = unsafe { (crate::used_moved_exports::CyberpunkVR_SightPsSwap, crate::used_moved_exports::CyberpunkVR_SightPsHash, crate::used_moved_exports::CyberpunkVR_SightVsHash) };
    swap != 0 && !ps.pShaderBytecode.is_null() && ps.BytecodeLength != 0 && !vs.pShaderBytecode.is_null() && vs.BytecodeLength != 0 && fnv(ps) == ps_hash && fnv(vs) == vs_hash && unsafe { cvr_dh_sight_ready() }
}
fn glass(ps: &Bytecode) -> Option<Bytecode> {
    let mut out = Bytecode::default();
    // SAFETY: our shader descriptions.
    unsafe { cvr_gpu::optics::cvr_optics_glass_replacement(ps, &mut out) }.then_some(out)
}
fn optics_shader(kind: u32) -> Option<Bytecode> {
    let mut out = Bytecode::default();
    // SAFETY: our output.
    unsafe { cvr_gpu::optics::cvr_optics_shader(kind, &mut out) }.then_some(out)
}
fn glass_created(pso: *mut c_void, velocity: *mut c_void, mask: *mut c_void) {
    // SAFETY: a live pipeline; the twins' references handed over.
    unsafe { cvr_gpu::optics::cvr_optics_glass_created(pso, velocity, mask) };
}
fn marker(vs: &Bytecode) -> Option<Bytecode> {
    let mut out = Bytecode::default();
    // SAFETY: our shader descriptions.
    unsafe { crate::world_marker_shader::cvr_world_marker_vertex_shader_replacement(vs.pShaderBytecode, vs.BytecodeLength, &mut out.pShaderBytecode, &mut out.BytecodeLength) }.then_some(out)
}
fn marker_result(vs: &Bytecode, success: bool) {
    // SAFETY: the shader's description.
    unsafe { crate::world_marker_shader::cvr_world_marker_vertex_shader_result(vs.pShaderBytecode, vs.BytecodeLength, success) };
}

// ---- stream pipelines
/// The caller's stream, copied by a safe read, with its stage offsets.
struct Stream {
    bytes: Vec<u8>,
    offsets: PipelineStreamOffsets,
}
impl Stream {
    fn read(desc: &Desc) -> Option<Stream> {
        if desc.pPipelineStateSubobjectStream.is_null() || desc.SizeInBytes == 0 || desc.SizeInBytes > 1 << 20 {
            return None;
        }
        let mut bytes = vec![0u8; desc.SizeInBytes];
        if !cvr_engine::stack::read(desc.pPipelineStateSubobjectStream as u64, &mut bytes) {
            return None;
        }
        let local = Desc { SizeInBytes: bytes.len(), pPipelineStateSubobjectStream: bytes.as_mut_ptr().cast() };
        let mut offsets = PipelineStreamOffsets::default();
        // SAFETY: our copy.
        if !unsafe { crate::pipeline_stream::cvr_pipeline_stream_offsets(&local, &mut offsets) } {
            offsets = PipelineStreamOffsets::default();
        }
        Some(Stream { bytes, offsets })
    }
    fn at(&self, offset: usize) -> Option<Bytecode> {
        (offset != usize::MAX && offset + core::mem::size_of::<Bytecode>() <= self.bytes.len())
            // SAFETY: within the copy.
            .then(|| unsafe { core::ptr::read_unaligned(self.bytes.as_ptr().add(offset).cast::<Bytecode>()) })
    }
    fn with(&self, edits: &[(usize, Bytecode)], clear_cache: bool) -> Vec<u8> {
        let mut copy = self.bytes.clone();
        for &(offset, code) in edits {
            // SAFETY: an offset the parse found inside the copy.
            unsafe { core::ptr::write_unaligned(copy.as_mut_ptr().add(offset).cast::<Bytecode>(), code) };
        }
        if clear_cache && self.offsets.cached != usize::MAX && self.offsets.cached + 16 <= copy.len() {
            // SAFETY: as above.
            unsafe { core::ptr::write_unaligned(copy.as_mut_ptr().add(self.offsets.cached).cast::<D3D12_CACHED_PIPELINE_STATE>(), D3D12_CACHED_PIPELINE_STATE::default()) };
        }
        copy
    }
}
/// Hook_CreatePipelineState.
///
/// # Safety
/// The device call's arguments.
pub unsafe fn create_stream(device: *mut c_void, desc: *const Desc, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let Some(r) = rt() else { return windows::Win32::Foundation::E_FAIL };
    // SAFETY: the .cpp's original, set before the hook is installed.
    let original: CreateStream = unsafe { core::mem::transmute(*r.original_stream) };
    let call = |d: *const Desc, o: *mut *mut c_void| unsafe {
        let span = cvr_host::hitch::start();
        let hr = original(device, d, riid, o);
        cvr_host::hitch::end(span, 5.0, || "CreatePipelineState (stream)".to_string());
        hr
    };
    let call_pipeline = |d: *const Desc, o: *mut *mut c_void| unsafe { original(device, d, &ID3D12PipelineState::IID, o) };
    if cvr_host::probe::internal() {
        return call(desc, out);
    }
    // SAFETY: the caller's description (or null).
    let stream = unsafe { desc.as_ref() }.and_then(Stream::read);
    if let Some(s) = &stream {
        let local = |bytes: &mut Vec<u8>| Desc { SizeInBytes: bytes.len(), pPipelineStateSubobjectStream: bytes.as_mut_ptr().cast() };
        if s.at(s.offsets.cs).is_some_and(|cs| vision_target(&cs)) {
            let vision = blob(3).map(bytecode).unwrap_or_default();
            let mut copy = s.with(&[(s.offsets.cs, vision)], false);
            let hr = call(&local(&mut copy), out);
            cvr_gpu::dxil::witness(vision.pShaderBytecode, hr.is_ok());
            if hr.is_ok() {
                bump!(CyberpunkVR_DebugVisionCsSwaps);
                log(r, format!("[pso] outline CS substituted (stream desc) pso={:016X}", first(out) as usize));
                return hr;
            }
            log(r, format!("[pso] outline CS substitution REFUSED hr=0x{:08X} (stream) -- original kept", hr.0 as u32));
        }
        if let Some(native_ps) = s.at(s.offsets.ps) {
            if let Some(optic) = glass(&native_ps) {
                let mut copy = s.with(&[(s.offsets.ps, optic)], true);
                let hr = call(&local(&mut copy), out);
                if hr.is_ok() {
                    let native_vs = s.at(s.offsets.vs).unwrap_or_default();
                    if created(out) {
                        record(first(out), &native_ps, &native_vs);
                        static LOGGED: AtomicBool = AtomicBool::new(false);
                        if !LOGGED.swap(true, SeqCst) {
                            log(r, "[optics] Stream glass pipeline: no lens velocity twin".into());
                        }
                        glass_created(first(out), core::ptr::null_mut(), core::ptr::null_mut());
                    }
                    bump!(CyberpunkVR_DebugPsoStream);
                    return hr;
                }
                log(r, format!("[optics] Glass stream PS rejected hr={:08X}; native pipeline retained", hr.0 as u32));
            }
        }
        if let Some(original_vs) = s.at(s.offsets.vs) {
            if let Some(replacement) = marker(&original_vs) {
                let mut copy = s.with(&[(s.offsets.vs, replacement)], true);
                let hr = call(&local(&mut copy), out);
                cvr_gpu::dxil::witness(replacement.pShaderBytecode, hr.is_ok());
                marker_result(&original_vs, hr.is_ok());
                if hr.is_ok() {
                    let ps = s.at(s.offsets.ps).unwrap_or_default();
                    if created(out) {
                        record(first(out), &ps, &original_vs);
                    }
                    bump!(CyberpunkVR_DebugPsoStream);
                    return hr;
                }
            }
        }
        if let (Some(ps), Some(vs)) = (s.at(s.offsets.ps), s.at(s.offsets.vs)) {
            if sight_target(&ps, &vs) {
                let (sight_ps, sight_vs) = (blob(0).map(bytecode).unwrap_or_default(), blob(1).map(bytecode).unwrap_or_default());
                let substituted = Stream { bytes: s.with(&[(s.offsets.ps, sight_ps), (s.offsets.vs, sight_vs)], false), offsets: PipelineStreamOffsets { ..s.offsets } };
                let mut copy = substituted.bytes.clone();
                let hr = call(&local(&mut copy), out);
                cvr_gpu::dxil::witness(sight_ps.pShaderBytecode, hr.is_ok());
                cvr_gpu::dxil::witness(sight_vs.pShaderBytecode, hr.is_ok());
                if hr.is_ok() {
                    bump!(CyberpunkVR_DebugSightSwaps);
                    bump!(CyberpunkVR_DebugPsoStream);
                    log(r, format!("[pso] sight PS substituted (stream desc) pso={:016X}", first(out) as usize));
                    if created(out) {
                        let mut twin: *mut c_void = core::ptr::null_mut();
                        if let Some(variant) = blob(2) {
                            let mut tcopy = substituted.with(&[(s.offsets.ps, bytecode(variant))], true);
                            let ht = call_pipeline(&local(&mut tcopy), &mut twin);
                            cvr_gpu::dxil::witness(variant.as_ptr().cast(), ht.is_ok());
                            if ht.is_err() {
                                twin = core::ptr::null_mut();
                                log(r, format!("[pso] sight twin 0 REFUSED hr=0x{:08X} (stream)", ht.0 as u32));
                            }
                        }
                        sight_twin_add(r, first(out), twin, core::ptr::null_mut());
                    }
                    return hr;
                }
                log(r, format!("[pso] sight PS substitution REFUSED hr=0x{:08X} (stream) -- original kept", hr.0 as u32));
            }
        }
    }
    let hr = call(desc, out);
    // SAFETY: the caller's description.
    if hr.is_ok() && created(out) && unsafe { desc.as_ref() }.is_some_and(|d| !d.pPipelineStateSubobjectStream.is_null()) {
        let pso = first(out);
        let (ps, vs, cs) = stream.as_ref().map_or((None, None, None), |s| (s.at(s.offsets.ps), s.at(s.offsets.vs), s.at(s.offsets.cs)));
        let (ps, vs) = (ps.unwrap_or_default(), vs.unwrap_or_default());
        if !ps.pShaderBytecode.is_null() || !vs.pShaderBytecode.is_null() {
            record(pso, &ps, &vs);
        }
        stream_twins(r, device, desc, pso, &cs.unwrap_or_default(), &ps, &vs, cs.is_some());
        (r.stream_created)(device, desc, pso);
        crate::device_creation::cvr_count_creation(5, 0, core::ptr::null());
        if stream.as_ref().is_some_and(|s| s.offsets.ms != usize::MAX) {
            let mesh = crate::used_moved_exports::CyberpunkVR_CreationCounts[14].fetch_add(1, Relaxed) + 1;
            if mesh <= 5 {
                log(r, format!("[creation] mesh-shader pipeline #{mesh} created: {:016X}", pso as usize));
            }
        }
        bump!(CyberpunkVR_DebugPsoStream);
    }
    hr
}

// ---- graphics pipelines
/// A bitwise copy of the description (its root signature reference neither taken nor released).
fn copy_graphics(desc: &Graphics) -> core::mem::ManuallyDrop<Graphics> {
    // SAFETY: a plain bitwise copy; ManuallyDrop keeps the copy's reference from being released.
    core::mem::ManuallyDrop::new(unsafe { core::ptr::read(desc) })
}
/// A single-target twin's output: `format` in target 0, `mask` written, no blending, no depth or stencil writes.
fn single_target(twin: &mut Graphics, format: DXGI_FORMAT, mask: u8) {
    twin.NumRenderTargets = 1;
    twin.RTVFormats = [DXGI_FORMAT_UNKNOWN; 8];
    twin.RTVFormats[0] = format;
    twin.BlendState = D3D12_BLEND_DESC::default();
    for target in &mut twin.BlendState.RenderTarget {
        *target = D3D12_RENDER_TARGET_BLEND_DESC {
            BlendEnable: false.into(),
            LogicOpEnable: false.into(),
            SrcBlend: D3D12_BLEND_ONE,
            DestBlend: D3D12_BLEND_ZERO,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ONE,
            DestBlendAlpha: D3D12_BLEND_ZERO,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            LogicOp: D3D12_LOGIC_OP_NOOP,
            RenderTargetWriteMask: 0,
        };
    }
    twin.BlendState.RenderTarget[0].RenderTargetWriteMask = mask;
    twin.DepthStencilState.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ZERO;
    twin.DepthStencilState.StencilWriteMask = 0;
}
/// history_mask_twin: the history mask's R8 twin of `base` with `ps`.
fn history_mask_twin(r: &Runtime, original: CreateGraphics, device: *mut c_void, base: &Graphics, ps: Bytecode, what: &str) -> *mut c_void {
    if ps.pShaderBytecode.is_null() || ps.BytecodeLength == 0 {
        return core::ptr::null_mut();
    }
    let mut twin = copy_graphics(base);
    twin.PS = ps;
    twin.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
    single_target(&mut twin, DXGI_FORMAT_R8_UNORM, D3D12_COLOR_WRITE_ENABLE_RED.0 as u8);
    let mut made = core::ptr::null_mut();
    // SAFETY: the original over our description.
    let hr = unsafe { original(device, &*twin, &ID3D12PipelineState::IID, &mut made) };
    cvr_gpu::dxil::witness(twin.PS.pShaderBytecode, hr.is_ok());
    if hr.is_err() {
        log(r, format!("[optics] {what} history mask twin rejected hr={:08X}", hr.0 as u32));
        return core::ptr::null_mut();
    }
    made
}
/// Hook_CreateGraphicsPipelineState.
///
/// # Safety
/// The device call's arguments.
pub unsafe fn create_graphics(device: *mut c_void, desc: *const Graphics, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let Some(r) = rt() else { return windows::Win32::Foundation::E_FAIL };
    // SAFETY: the .cpp's original, set before the hook is installed.
    let original: CreateGraphics = unsafe { core::mem::transmute(*r.original_graphics) };
    let call = |d: *const Graphics, o: *mut *mut c_void| unsafe {
        // CVR_HITCH_TRACE: a graphics pipeline that took 5 ms or more to make, on whatever thread made it
        let span = cvr_host::hitch::start();
        let hr = original(device, d, riid, o);
        cvr_host::hitch::end(span, 5.0, || "CreateGraphicsPipelineState".to_string());
        if let Some(g) = d.as_ref() {
            cvr_gpu::dxil::witness(g.VS.pShaderBytecode, hr.is_ok());
            cvr_gpu::dxil::witness(g.PS.pShaderBytecode, hr.is_ok());
        }
        hr
    };
    if cvr_host::probe::internal() {
        return call(desc, out);
    }
    // SAFETY: the caller's description (or null).
    let source = unsafe { desc.as_ref() };
    if let Some(d) = source {
        if let Some(optic) = glass(&d.PS) {
            let mut patched = copy_graphics(d);
            patched.PS = optic;
            patched.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
            let hr = call(&*patched, out);
            if hr.is_ok() {
                if created(out) {
                    record(first(out), &d.PS, &d.VS);
                    let mut velocity = core::ptr::null_mut();
                    if let Some(velocity_ps) = optics_shader(0) {
                        let mut twin = copy_graphics(&patched);
                        twin.PS = velocity_ps;
                        single_target(&mut twin, DXGI_FORMAT_R16G16_FLOAT, (D3D12_COLOR_WRITE_ENABLE_RED.0 | D3D12_COLOR_WRITE_ENABLE_GREEN.0) as u8);
                        // SAFETY: the original over our description.
                        let twin_hr = unsafe { original(device, &*twin, &ID3D12PipelineState::IID, &mut velocity) };
                        cvr_gpu::dxil::witness(twin.PS.pShaderBytecode, twin_hr.is_ok());
                        if twin_hr.is_err() {
                            velocity = core::ptr::null_mut();
                            log(r, format!("[optics] Lens velocity twin rejected hr={:08X}", twin_hr.0 as u32));
                        }
                    }
                    let mask = optics_shader(1).map_or(core::ptr::null_mut(), |mask_ps| history_mask_twin(r, original, device, &patched, mask_ps, "Lens"));
                    glass_created(first(out), velocity, mask);
                }
                bump!(CyberpunkVR_DebugPsoGfx);
                return hr;
            }
            log(r, format!("[optics] Glass PS rejected hr={:08X}; native pipeline retained", hr.0 as u32));
        }
        if let Some(replacement) = marker(&d.VS) {
            let mut patched = copy_graphics(d);
            patched.VS = replacement;
            patched.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
            let hr = call(&*patched, out);
            marker_result(&d.VS, hr.is_ok());
            if hr.is_ok() {
                if created(out) {
                    record(first(out), &d.PS, &d.VS);
                }
                bump!(CyberpunkVR_DebugPsoGfx);
                return hr;
            }
        }
        // SAFETY: the Rust-defined export.
        if !d.PS.pShaderBytecode.is_null() && d.PS.BytecodeLength != 0 && fnv(&d.PS) == unsafe { crate::used_moved_exports::CyberpunkVR_SightPsHash } {
            rootsig_dump(r, as_raw_root(d));
        }
        if sight_target(&d.PS, &d.VS) {
            let mut sight = copy_graphics(d);
            sight.PS = blob(0).map(bytecode).unwrap_or_default();
            sight.VS = blob(1).map(bytecode).unwrap_or_default();
            let hr = call(&*sight, out);
            if hr.is_ok() {
                bump!(CyberpunkVR_DebugSightSwaps);
                log(r, format!("[pso] sight PS substituted (graphics desc) pso={:016X}", first(out) as usize));
                if created(out) {
                    let mut twin = core::ptr::null_mut();
                    if let Some(variant) = blob(2) {
                        let mut t = copy_graphics(&sight);
                        t.PS = bytecode(variant);
                        t.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
                        // SAFETY: the original over our description.
                        let ht = unsafe { original(device, &*t, &ID3D12PipelineState::IID, &mut twin) };
                        cvr_gpu::dxil::witness(t.PS.pShaderBytecode, ht.is_ok());
                        if ht.is_err() {
                            twin = core::ptr::null_mut();
                            log(r, format!("[pso] sight twin 0 REFUSED hr=0x{:08X}", ht.0 as u32));
                        }
                    }
                    let mask = optics_shader(2).map_or(core::ptr::null_mut(), |mask_ps| history_mask_twin(r, original, device, &sight, mask_ps, "Sight"));
                    sight_twin_add(r, first(out), twin, mask);
                    record(first(out), &d.PS, &d.VS);
                }
                bump!(CyberpunkVR_DebugPsoGfx);
                return hr;
            }
            log(r, format!("[pso] sight PS substitution REFUSED hr=0x{:08X} -- keeping the original", hr.0 as u32));
        }
    }
    let hr = call(desc, out);
    if hr.is_ok() && created(out) {
        if let Some(d) = source {
            let pso = first(out);
            record(pso, &d.PS, &d.VS);
            graphics_twins(r, device, desc, pso);
            (r.graphics_created)(device, desc, pso);
            bump!(CyberpunkVR_DebugPsoGfx);
        }
    }
    hr
}
fn as_raw_root(d: &Graphics) -> *mut c_void {
    // SAFETY: the description's root signature field, a nullable COM pointer.
    unsafe { core::ptr::read(core::ptr::addr_of!(d.pRootSignature).cast::<*mut c_void>()) }
}

// ---- compute pipelines
/// Hook_CreateComputePipelineState.
///
/// # Safety
/// The device call's arguments.
pub unsafe fn create_compute(device: *mut c_void, desc: *const Compute, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let Some(r) = rt() else { return windows::Win32::Foundation::E_FAIL };
    // SAFETY: the .cpp's original, set before the hook is installed.
    let original: CreateCompute = unsafe { core::mem::transmute(*r.original_compute) };
    crate::device_creation::cvr_count_creation(6, 0, core::ptr::null());
    // SAFETY: the caller's description (or null).
    let source = unsafe { desc.as_ref() };
    if let Some(d) = source.filter(|d| vision_target(&d.CS)) {
        // SAFETY: a bitwise copy, its root reference neither taken nor released.
        let mut vision = core::mem::ManuallyDrop::new(unsafe { core::ptr::read(d) });
        vision.CS = blob(3).map(bytecode).unwrap_or_default();
        // SAFETY: the original over our description.
        let hr = unsafe { original(device, &*vision, riid, out) };
        cvr_gpu::dxil::witness(vision.CS.pShaderBytecode, hr.is_ok());
        if hr.is_ok() {
            bump!(CyberpunkVR_DebugVisionCsSwaps);
            log(r, format!("[pso] outline CS substituted (compute desc) pso={:016X}", first(out) as usize));
            return hr;
        }
        log(r, format!("[pso] outline CS substitution REFUSED hr=0x{:08X} -- original kept", hr.0 as u32));
    }
    // SAFETY: the original over the caller's description.
    let span = cvr_host::hitch::start();
    let hr = unsafe { original(device, desc, riid, out) };
    cvr_host::hitch::end(span, 5.0, || "CreateComputePipelineState".to_string());
    if let Some(d) = source {
        cvr_gpu::dxil::witness(d.CS.pShaderBytecode, hr.is_ok());
    }
    if hr.is_ok() && created(out) && source.is_some() {
        let span = cvr_host::hitch::start();
        compute_twins(r, device, desc, first(out));
        cvr_host::hitch::end(span, 5.0, || "compute twins of a new compute pipeline".to_string());
    }
    hr
}
const _: () = assert!(core::mem::size_of::<Bytecode>() == 16);

// ---- the C ABI
/// # Safety
/// The device call's arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pso_create_stream(device: *mut c_void, desc: *const Desc, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { create_stream(device, desc, riid, out) }
}
/// # Safety
/// The device call's arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pso_create_graphics(device: *mut c_void, desc: *const Graphics, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { create_graphics(device, desc, riid, out) }
}
/// # Safety
/// The device call's arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pso_create_compute(device: *mut c_void, desc: *const Compute, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { create_compute(device, desc, riid, out) }
}

// ---- the hidden-area shader twins and the sight's root signature (formerly the .cpp's runtime lambdas)

unsafe extern "C" fn twin_call(ctx: *mut c_void, code: *const u8, size: usize) -> *mut c_void {
    let bytes = if code.is_null() {
        &[][..]
    } else {
        // SAFETY: the patched bytes the registry hands over.
        unsafe { core::slice::from_raw_parts(code, size) }
    };
    // SAFETY: the boxed creator this module handed over.
    unsafe { (*ctx.cast::<TwinFn>())(bytes) }
}
unsafe extern "C" fn twin_free(ctx: *mut c_void) {
    // SAFETY: the box this module leaked, freed once.
    drop(unsafe { Box::from_raw(ctx.cast::<TwinFn>()) });
}
/// hidden_area::shaders::Register.
fn register_twin(original: *mut c_void, code: &Bytecode, create: TwinFn) {
    let ctx = Box::into_raw(Box::new(create)).cast::<c_void>();
    // SAFETY: the created pipeline, the stage's bytes and a creator the registry owns until it frees it.
    unsafe { crate::shader_patches::cvr_shader_patches_register(original, code.pShaderBytecode, code.BytecodeLength, ctx, twin_call, twin_free) };
}
fn has_patch(code: &Bytecode) -> bool {
    // SAFETY: the shader's bytes.
    unsafe { crate::shader_patches::cvr_shader_patches_has_patch(code.pShaderBytecode, code.BytecodeLength) }
}
fn pipeline_created(pso: *mut c_void, cs: &Bytecode, ps: &Bytecode, vs: &Bytecode) {
    // SAFETY: the created pipeline and its shaders.
    unsafe {
        crate::shader_patches::cvr_shader_patches_pipeline_created(
            pso,
            cs.pShaderBytecode,
            cs.BytecodeLength,
            ps.pShaderBytecode,
            ps.BytecodeLength,
            vs.pShaderBytecode,
            vs.BytecodeLength,
        )
    };
}
/// The stream pipeline's twin: the patched stage in a copy of the description the creator owns.
#[allow(clippy::too_many_arguments)]
fn stream_twins(r: &'static Runtime, device: *mut c_void, desc: *const Desc, pso: *mut c_void, cs: &Bytecode, ps: &Bytecode, vs: &Bytecode, compute: bool) {
    pipeline_created(pso, cs, ps, vs);
    let vertex_patch = !compute && !has_patch(ps) && has_patch(vs);
    let patched = if compute {
        cs
    } else if vertex_patch {
        vs
    } else {
        ps
    };
    if !has_patch(patched) {
        return;
    }
    // SAFETY: the caller's description, parsed into an owned copy.
    let Some(d) = (unsafe { desc.as_ref() }) else { return };
    // SAFETY: the description's stream.
    let Some(owned) = (unsafe { crate::pipeline_stream::own_stream(d) }) else { return };
    let stage = if compute {
        owned.offsets.cs
    } else if vertex_patch {
        owned.offsets.vs
    } else {
        owned.offsets.ps
    };
    if stage == usize::MAX {
        return;
    }
    register_twin(pso, patched, crate::pso_twins::stream_creator(r.original_stream, device as usize, owned, stage, vertex_patch));
}
/// The graphics pipeline's twin: the patched pixel shader (with the fixed vertex twin) or the patched vertex shader.
fn graphics_twins(r: &'static Runtime, device: *mut c_void, desc: *const Graphics, pso: *mut c_void) {
    // SAFETY: the caller's description.
    let Some(d) = (unsafe { desc.as_ref() }) else { return };
    pipeline_created(pso, &Bytecode::default(), &d.PS, &d.VS);
    let pixel = has_patch(&d.PS);
    if !pixel && !has_patch(&d.VS) {
        return;
    }
    // SAFETY: the description copied with its parts.
    let owned = unsafe { crate::pipeline_stream::own_graphics(d) };
    let patched = if pixel { d.PS } else { d.VS };
    register_twin(pso, &patched, crate::pso_twins::graphics_creator(r.original_graphics, device as usize, pixel, owned));
}
/// The compute pipeline's twin: the patched compute shader in the caller's description, copied as it was.
fn compute_twins(r: &'static Runtime, device: *mut c_void, desc: *const Compute, pso: *mut c_void) {
    // SAFETY: the caller's description.
    let Some(d) = (unsafe { desc.as_ref() }) else { return };
    pipeline_created(pso, &d.CS, &Bytecode::default(), &Bytecode::default());
    if !has_patch(&d.CS) {
        return;
    }
    // SAFETY: a bitwise copy, as the .cpp's capture by value was.
    let base = unsafe { core::ptr::read(d) };
    // A reference of the twin's own on the root signature (the copy borrows it; the twin is created later, at a bind).
    let root: Option<ID3D12RootSignature> = (*d.pRootSignature).clone();
    let device = device as usize;
    register_twin(
        pso,
        &d.CS,
        Box::new(move |code: &[u8]| {
            let _root = &root; // held as long as the creator
            // SAFETY: a bitwise copy of the captured description.
            let mut twin = unsafe { core::ptr::read(&base) };
            twin.CS = Bytecode { pShaderBytecode: code.as_ptr().cast(), BytecodeLength: code.len() };
            twin.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
            let mut out = core::ptr::null_mut();
            // SAFETY: the original over the twin's description.
            let hr = unsafe { (core::mem::transmute::<usize, CreateCompute>(*r.original_compute))(device as *mut c_void, &twin, &ID3D12PipelineState::IID, &mut out) };
            twin_made(hr, out)
        }),
    );
}
/// rootsig_dump: the sight's root signature, once (with the verbose diagnostics).
fn rootsig_dump(r: &Runtime, root: *mut c_void) {
    static DONE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    if root.is_null() || !cvr_host::log::verbose() || DONE.compare_exchange(core::ptr::null_mut(), root, SeqCst, SeqCst).is_err() {
        return;
    }
    // SAFETY: the root signature's recorded blob.
    let n = unsafe { crate::device_creation::cvr_root_signature_blob(root, core::ptr::null_mut(), 0) };
    let mut blob = vec![0u8; n];
    // SAFETY: a buffer of the blob's size.
    if n == 0 || unsafe { crate::device_creation::cvr_root_signature_blob(root, blob.as_mut_ptr(), n) } != n {
        log(r, format!("[rootsig] sight signature {:016X}: blob not captured (created before the hook)", root as usize));
        return;
    }
    // SAFETY: the blob's bytes.
    unsafe { cvr_dh_rootsig_dump(root, blob.as_ptr(), blob.len()) };
}

// ---- the ray tracing state objects (formerly the .cpp's Hook_CreateStateObject)

type CreateState = unsafe extern "system" fn(device: *mut c_void, desc: *const D3D12_STATE_OBJECT_DESC, riid: *const GUID, out: *mut *mut c_void) -> HRESULT;
/// The original CreateStateObject (ID3D12Device5 slot 62), the device creation hooks' slot.
#[no_mangle]
pub static cvr_pso_original_state_object: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// A wide string's text ("" for null).
fn wide(text: windows::core::PCWSTR) -> String {
    if text.is_null() {
        String::new()
    } else {
        // SAFETY: a terminated wide string.
        String::from_utf16_lossy(unsafe { text.as_wide() })
    }
}
/// Hook_CreateStateObject: each DXIL library with a stereo_rt twin is created from its twin (the associations
/// rebased onto the copied subobjects), the native description when the driver refuses it.
///
/// # Safety
/// The device call's arguments.
#[no_mangle]
pub unsafe extern "system" fn cvr_pso_create_state_object(device: *mut c_void, desc: *const D3D12_STATE_OBJECT_DESC, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    // SAFETY: the slot the device hooks filled before installing this hook.
    let original: CreateState = unsafe { core::mem::transmute(cvr_pso_original_state_object.load(Acquire)) };
    // SAFETY: the caller's description and subobjects.
    if let Some(d) = unsafe { desc.as_ref() }.filter(|d| !d.pSubobjects.is_null()) {
        let n = d.NumSubobjects as usize;
        // SAFETY: the caller's subobjects.
        let mut subobjects: Vec<D3D12_STATE_SUBOBJECT> = unsafe { core::slice::from_raw_parts(d.pSubobjects, n) }.to_vec();
        // SAFETY: plain descriptions, zeroed.
        let mut libraries: Vec<D3D12_DXIL_LIBRARY_DESC> = (0..n).map(|_| unsafe { core::mem::zeroed() }).collect();
        let mut twinned = 0u32;
        for i in 0..n {
            if subobjects[i].Type != D3D12_STATE_SUBOBJECT_TYPE_DXIL_LIBRARY || subobjects[i].pDesc.is_null() {
                continue;
            }
            // SAFETY: a DXIL library subobject's description.
            let library = unsafe { &*subobjects[i].pDesc.cast::<D3D12_DXIL_LIBRARY_DESC>() };
            let (mut code, mut size) = (core::ptr::null(), 0usize);
            // SAFETY: the library's bytes into local outputs.
            let found = unsafe {
                crate::shader_patches::cvr_shader_patches_library_twin(library.DXILLibrary.pShaderBytecode, library.DXILLibrary.BytecodeLength, &mut code, &mut size)
            };
            if !found || code.is_null() {
                continue;
            }
            // SAFETY: a bitwise copy of the library's description.
            libraries[i] = unsafe { core::ptr::read(library) };
            libraries[i].DXILLibrary = Bytecode { pShaderBytecode: code, BytecodeLength: size };
            subobjects[i].pDesc = (&libraries[i] as *const D3D12_DXIL_LIBRARY_DESC).cast();
            twinned += 1;
            let first = if library.NumExports != 0 && !library.pExports.is_null() {
                // SAFETY: the library's first export.
                wide(unsafe { (*library.pExports).Name })
            } else {
                "(all)".to_string()
            };
            if let (true, Some(r)) = (cvr_host::log::verbose(), rt()) {
                log(r, format!("[pso] stereo_rt twin for a {}-byte library with {} export(s), first {first}", library.DXILLibrary.BytecodeLength, library.NumExports));
            }
        }
        if twinned != 0 {
            // SAFETY: plain descriptions, zeroed.
            let mut associations: Vec<D3D12_SUBOBJECT_TO_EXPORTS_ASSOCIATION> = (0..n).map(|_| unsafe { core::mem::zeroed() }).collect();
            let first = d.pSubobjects;
            for i in 0..n {
                if subobjects[i].Type != D3D12_STATE_SUBOBJECT_TYPE_SUBOBJECT_TO_EXPORTS_ASSOCIATION || subobjects[i].pDesc.is_null() {
                    continue;
                }
                // SAFETY: an association subobject's description.
                associations[i] = unsafe { core::ptr::read(subobjects[i].pDesc.cast::<D3D12_SUBOBJECT_TO_EXPORTS_ASSOCIATION>()) };
                let target = associations[i].pSubobjectToAssociate;
                // SAFETY: a pointer comparison within the caller's array.
                if target >= first && target < unsafe { first.add(n) } {
                    // SAFETY: the same index in the copy.
                    associations[i].pSubobjectToAssociate = unsafe { subobjects.as_ptr().offset(target.offset_from(first)) };
                }
                subobjects[i].pDesc = (&associations[i] as *const D3D12_SUBOBJECT_TO_EXPORTS_ASSOCIATION).cast();
            }
            let twin = D3D12_STATE_OBJECT_DESC { Type: d.Type, NumSubobjects: d.NumSubobjects, pSubobjects: subobjects.as_ptr() };
            // SAFETY: the original over the twin's description.
            let hr = unsafe { original(device, &twin, riid, out) };
            crate::shader_patches::cvr_shader_patches_library_twin_created(hr.is_ok(), twinned);
            if let (true, Some(r)) = (cvr_host::log::verbose() || hr.is_err(), rt()) {
                log(r, format!("[pso] state object with {twinned} stereo_rt library twin(s): hr=0x{:08X}", hr.0 as u32));
            }
            if hr.is_ok() {
                return hr;
            }
        }
    }
    // SAFETY: the original over the caller's description.
    unsafe { original(device, desc, riid, out) }
}

