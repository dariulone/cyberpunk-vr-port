//! The retained geometry capture's control front (src/Render/NativeRetainedGeometry.cpp): the capture's lock, the
//! ownership and start of its work (FrameBoundary: the prepared pair's poll and request, the finished capture's
//! completion, a new request's switches, catalog and arming), the frontier and scene observers' latch, arming and
//! disarming, Fail, the chain's counters, the owned-list lookup, the verification proof's recording (Proof), the
//! list observers (Flush, barriers, targets, resources, close, reset, render passes), the submission bookkeeping
//! (Submitted, SubmitCurrent, the infix prefix and suffix, the queue order) and the self-tests.
//!
//! The work, its eyes and its sources are owned on this side (retained_work.rs: the Work, AppendSource); the C++ half
//! keeps the selector catalog (jsoncpp) reached through the attached host table, under this module's lock.

#![allow(static_mut_refs)]
use crate::declared_moved_exports::{CyberpunkVR_SceneAccessEnable, CyberpunkVR_SceneChildEnable, CyberpunkVR_SubmissionFrontierEnable};
use crate::prepared_pair_state::*;
use crate::retained_exports::*;
use crate::retained_payloads::{Plane, ProofHost, ProofOut, ProofTargets};
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};
use cvr_gpu::scene_state::TargetInfo;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::*};
use windows::core::{IUnknown, Interface};
use crate::geometry_commands::DrawRecord;
use crate::preparation_trace::{DrawTag, GraphParentRecord};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandQueue, ID3D12CommandSignature, ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12PipelineState,
    ID3D12Resource, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_FENCE_FLAG_NONE, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_STATE_DEPTH_WRITE,
    D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_CPU_PAGE_PROPERTY_WRITE_BACK, D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE, D3D12_HEAP_FLAGS,
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_HEAP_TYPE_GPU_UPLOAD, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_RESOURCE_DIMENSION_BUFFER,
};
use windows::Win32::System::Threading::{AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK};

// ---- the shared bases

/// native_recording::Lease.
#[repr(C)]
pub struct Recording {
    pub token: Option<IUnknown>,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
/// scene_state::GeometryTargets.
#[repr(C)]
#[derive(Default)]
pub struct Targets {
    pub resources: [Option<ID3D12Resource>; 5],
    pub views: [TargetInfo; 5],
    pub handles: [usize; 5],
    pub colors: u32,
}
/// Eye's scalars (`struct Eye : EyeScalars`).
#[repr(C)]
pub struct EyeScalars {
    pub recording: Recording,
    pub targets: Targets,
    pub imported: [bool; 5],
    pub readback: Option<ID3D12Resource>,
    pub fence: Option<ID3D12Fence>,
    pub source_queue: Option<ID3D12CommandQueue>,
    pub signaled_value: u64,
    pub submission_certified: bool,
    pub planes: [Plane; 8],
    pub plane_count: u32,
    pub scope: u64,
    pub colors: u32,
    pub started: bool,
    pub sealed: bool,
    pub pending: bool,
    pub submitted: bool,
    pub cbv_arena_aborted: bool,
    pub t5_aborted: bool,
    pub ia_aborted: bool,
    pub ia_validated: bool,
    pub full_t5_reference_bytes: u64,
    pub instance_readback_bytes: u64,
    pub tail_first: u64,
    pub seal_reason: u32,
}
/// Work's scalars (`struct Work : WorkScalars`).
#[repr(C)]
pub struct WorkScalars {
    pub device: Option<ID3D12Device>,
    pub journal: *mut c_void,
    pub eye: [*mut EyeScalars; 2],
    pub guarded_attempted: bool,
    pub current_enabled: bool,
    pub frontier_observer: bool,
    pub scene_access_observer: bool,
    pub current_started: bool,
    pub current_submitted: bool,
    pub infix_enabled: bool,
    pub infix_in_flight: bool,
    pub tail_enabled: bool,
    pub allow_vrs_refinement: bool,
    pub full_t5_enabled: bool,
    pub borrowed_tables_enabled: bool,
    pub fixed_assets_enabled: bool,
    pub source_stream: bool,
    pub source_failed: bool,
    pub source_compiled: bool,
    pub source_snapshots: bool,
    pub snapshot_failed: bool,
    pub source_cbvs: bool,
    pub cbv_failed: bool,
    pub source_tables: bool,
    pub tables_failed: bool,
    pub source_details: bool,
    pub details_failed: bool,
    pub source_cbv_snapshots: bool,
    pub cbv_snapshots_failed: bool,
    pub source_ia_snapshots: bool,
    pub cbv_payload_complete: bool,
    pub source_selectors: bool,
    pub assembly_complete: bool,
    pub assembly_truncated: bool,
    pub assembly_bank_attempted: bool,
    pub payloads_processed: bool,
    pub created: bool,
    pub failed: bool,
    pub complete: bool,
    pub instance_overlay_stage: u32,
    pub overlay_stage: u32,
    pub frame: u32,
    pub mode: u32,
    pub stage: u32,
    pub assembly_bank_result: i32,
    pub assembly_descriptors: u64,
    pub assembly_bank_failure: [u64; 4],
}
const _: () = assert!(
    core::mem::size_of::<Recording>() == 40
        && core::mem::size_of::<Targets>() == 328
        && core::mem::size_of::<Plane>() == 88
        && core::mem::size_of::<EyeScalars>() == 1184
        && core::mem::offset_of!(EyeScalars, planes) == 416
        && core::mem::offset_of!(EyeScalars, tail_first) == 1168
        && core::mem::size_of::<WorkScalars>() == 136
        && core::mem::offset_of!(WorkScalars, instance_overlay_stage) == 68
        && core::mem::offset_of!(WorkScalars, assembly_bank_failure) == 104
);
impl WorkScalars {
    /// The eye's scalars: a separate object of the Work (not these scalars), alive while the Work is; the
    /// capture's lock serialises every access.
    fn eye(&self, side: usize) -> &'static mut EyeScalars {
        // SAFETY: the Work's own eyes (set by its constructor), alive while it is.
        unsafe { &mut *self.eye[side] }
    }
    fn eyes(&self) -> [&'static mut EyeScalars; 2] {
        [self.eye(0), self.eye(1)]
    }
}

// ---- the host

/// The C++ half's functions, attached once (`cvr_nrg2_attach`).
#[repr(C)]
pub struct Host {
    /// The selector catalog (loaded once through jsoncpp; source_selector_plan.rs' handle, null when absent).
    pub selector_catalog: unsafe extern "C" fn() -> *mut c_void,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
fn host() -> &'static Host {
    let attached = HOST.load(Acquire);
    #[cfg(all(feature = "rust-build", not(test)))]
    if attached.is_null() {
        return &plugin::HOST;
    }
    // SAFETY: attached with static storage by a test program's initializer before any call.
    unsafe { &*attached }
}
/// The plugin's host (formerly src/Render/NativeRetainedGeometry.cpp's): the selector catalog named by
/// CVR_STEREO_SOURCE_SELECTORS, loaded once through jsoncpp (SourceSelectorPlan.cpp's cvr_cpp_load_selector_catalog);
/// null when the variable is unset or longer than the 32767 characters the C++ buffer held.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use core::ffi::c_void;
    use std::sync::OnceLock;
    unsafe extern "C" {
        fn cvr_cpp_load_selector_catalog(path: *const u16) -> *mut c_void;
    }
    pub(super) static HOST: super::Host = super::Host { selector_catalog };
    unsafe extern "C" fn selector_catalog() -> *mut c_void {
        static HANDLE: OnceLock<usize> = OnceLock::new();
        *HANDLE.get_or_init(|| {
            let name: Vec<u16> = "CVR_STEREO_SOURCE_SELECTORS\0".encode_utf16().collect();
            let mut path = vec![0u16; 32768];
            // SAFETY: a NUL-terminated name and a 32768-character buffer.
            let length = unsafe { windows_sys::Win32::System::Environment::GetEnvironmentVariableW(name.as_ptr(), path.as_mut_ptr(), 32768) };
            if length == 0 || length >= 32768 {
                return 0;
            }
            // SAFETY: the NUL-terminated path just read.
            unsafe { cvr_cpp_load_selector_catalog(path.as_ptr()) as usize }
        }) as *mut c_void
    }
}

// ---- the lock and the state

struct State {
    work: *mut WorkScalars,
    render_passes: Vec<usize>,
    rust: Option<Box<WorkRust>>,
    verified: Vec<ID3D12PipelineState>,
}
struct Shared {
    lock: UnsafeCell<SRWLOCK>,
    state: UnsafeCell<State>,
}
// SAFETY: the state is only touched under the SRW lock.
unsafe impl Sync for Shared {}
static SHARED: Shared = Shared {
    lock: UnsafeCell::new(SRWLOCK { Ptr: null_mut() }),
    state: UnsafeCell::new(State { work: null_mut(), render_passes: Vec::new(), rust: None, verified: Vec::new() }),
};
static WATCHING: AtomicBool = AtomicBool::new(false);
fn raw_lock(on: bool) {
    // SAFETY: the static lock; acquire and release pair up on the calling thread.
    unsafe {
        if on {
            AcquireSRWLockExclusive(SHARED.lock.get());
        } else {
            ReleaseSRWLockExclusive(SHARED.lock.get());
        }
    }
}
struct Locked;
impl Drop for Locked {
    fn drop(&mut self) {
        raw_lock(false);
    }
}
fn lock() -> Locked {
    raw_lock(true);
    Locked
}
/// The state; only under the lock (taken here or by the C++ half).
#[allow(clippy::mut_from_ref)]
fn state() -> &'static mut State {
    // SAFETY: callers hold the capture's lock.
    unsafe { &mut *SHARED.state.get() }
}
fn work() -> Option<&'static mut WorkScalars> {
    // SAFETY: the installed Work, alive until replaced under the lock.
    unsafe { state().work.as_mut() }
}
macro_rules! ex {
    ($name:ident) => {
        // SAFETY: an export of this port, written under the capture's lock as the .cpp wrote it.
        unsafe { &mut *addr_of_mut!($name) }
    };
}
/// CyberpunkVR_OwnedSceneSplitEnable: Rust's in the plugin, the test program's in the probe variants.
fn scene_split() -> &'static std::sync::atomic::AtomicU32 {
    #[cfg(feature = "rust-build")]
    {
        &crate::used_moved_exports::CyberpunkVR_OwnedSceneSplitEnable
    }
    #[cfg(not(feature = "rust-build"))]
    {
        extern "C" {
            static CyberpunkVR_OwnedSceneSplitEnable: std::sync::atomic::AtomicU32;
        }
        // SAFETY: the test program's std::atomic<uint32_t> (the same layout), alive for the program.
        unsafe { &*core::ptr::addr_of!(CyberpunkVR_OwnedSceneSplitEnable) }
    }
}
fn internal() -> bool {
    cvr_host::probe::internal()
}
fn eye_index(side: u32) -> u32 {
    1 - side
}
fn npp_active() -> bool {
    crate::prepared_pair_native::cvr_npp2_active()
}
fn active() -> bool {
    WATCHING.load(Relaxed)
        || CyberpunkVR_RetainedGeometryRequest.load(Relaxed) != 0
        || CyberpunkVR_PreparedPairRequest.load(Relaxed) != 0
        || npp_active()
}

// ---- the frontier and scene observers

fn latch_frontier_observer(capture: &mut WorkScalars) {
    capture.current_enabled = CyberpunkVR_PreparedCurrentEnable.load(SeqCst) != 0;
    let scene = scene_split().load(SeqCst);
    capture.frontier_observer = capture.current_enabled || scene == 1 || scene == 2;
}
fn arm_frontier_observer(capture: &WorkScalars) {
    if !capture.frontier_observer {
        return;
    }
    CyberpunkVR_SubmissionFrontierEnable.store(0, SeqCst);
    if crate::submission_frontier_owner::cvr_sfo_install() && crate::submission_frontier_owner::cvr_sfo_reset() {
        CyberpunkVR_SubmissionFrontierEnable.store(1, SeqCst);
    }
}
fn disarm_finished_frontier_observer(capture: &WorkScalars) {
    if capture.frontier_observer && (capture.complete || capture.failed) {
        CyberpunkVR_SubmissionFrontierEnable.store(0, SeqCst);
    }
    if capture.scene_access_observer && (capture.complete || capture.failed || CyberpunkVR_SceneAccessEnable.load(SeqCst) == 0) {
        crate::scene_access_native::cvr_sao_finish(capture.failed || !capture.complete);
        CyberpunkVR_SceneChildEnable.store(0, SeqCst);
    }
}

// ---- failure, counters, lookups

fn fail(reason: u32) {
    if let Some(w) = work() {
        w.failed = true;
        disarm_finished_frontier_observer(w);
    }
    finish_targets();
    let stats = ex!(CyberpunkVR_RetainedGeometryStats);
    if stats[14] == 0 {
        stats[14] = u64::from(reason);
    }
    CyberpunkVR_RetainedGeometryState.store(4, SeqCst);
}
fn publish() {
    let Some(w) = work() else { return };
    if !w.created {
        return;
    }
    let mut counts = [0u64; 7];
    // SAFETY: the chain's storage (or null) and a writable row.
    unsafe { crate::geometry_chain::cvr_geometry_chain_counts(chain(), &mut counts) };
    let stats = ex!(CyberpunkVR_RetainedGeometryStats);
    for side in 0..2u32 {
        stats[10 + side as usize] = counts[eye_index(side) as usize];
        stats[12 + side as usize] = counts[5 + eye_index(side) as usize];
    }
}
fn publish_sources() {
    publish_sources_impl();
}
fn owned(list: *mut c_void) -> Option<(u32, &'static mut EyeScalars)> {
    let w = work()?;
    for side in 0..2u32 {
        let value = w.eye(side as usize);
        if value.started && !value.sealed && value.recording.list == list as u64 {
            return Some((side, value));
        }
    }
    None
}
fn touches(value: &EyeScalars, resource: *mut c_void) -> bool {
    !resource.is_null()
        && (0..5).any(|slot| value.imported[slot] && value.targets.resources[slot].as_ref().is_some_and(|r| r.as_raw() == resource))
}
fn seal(list: *mut c_void, side: u32, reason: u32, kind: u32) {
    seal_impl(list, side, reason, kind);
}
/// target_handoff::ExitKind.
mod exit_kind {
    pub const UNKNOWN: u32 = 0;
    pub const CLOSE: u32 = 1;
    pub const COPY: u32 = 5;
    pub const DISCARD: u32 = 6;
    pub const RESOLVE: u32 = 8;
    pub const OTHER: u32 = 9;
}

// ---- the verification proof

unsafe extern "C-unwind" fn proof_keep(_user: *mut c_void, list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    // SAFETY: the proof's own objects, `count` of them.
    unsafe { keep(list, objects, count) }
}
unsafe extern "C-unwind" fn proof_internal(user: *mut c_void) {
    // SAFETY: the proof's Option<Internal>, alive across the call.
    unsafe { *user.cast::<Option<cvr_host::probe::Internal>>() = Some(cvr_host::probe::Internal::new()) };
}
fn proof(list: *mut c_void, side: u32) -> bool {
    let Some(w) = work() else { return false };
    let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
    let value = w.eye(side as usize);
    let mut t = ProofTargets { native: [null_mut(); 5], array: [null_mut(); 5], imported: 0, eye: eye_index(side) };
    for slot in 0..5u32 {
        if value.imported[slot as usize] {
            t.imported |= 1 << slot;
            t.native[slot as usize] = value.targets.resources[slot as usize].as_ref().map_or(null_mut(), |r| r.as_raw());
            t.array[slot as usize] = crate::geometry_chain::cvr_geometry_chain_resource(chain(), slot);
        }
    }
    let mut rows = [Plane::default(); 8];
    let mut out = ProofOut { planes: rows.as_mut_ptr(), capacity: rows.len(), count: 0, readback: null_mut() };
    let mut scope: Option<cvr_host::probe::Internal> = None;
    let proof_host = ProofHost { user: (&mut scope as *mut Option<cvr_host::probe::Internal>).cast(), keep: proof_keep, internal: proof_internal };
    // SAFETY: live arguments; the out rows hold their capacity.
    let recorded = unsafe { crate::retained_payloads::cvr_nrg_proof(device, list, &t, &mut out, &proof_host) };
    drop(scope);
    if !out.readback.is_null() {
        // SAFETY: the proof's new readback, one reference handed over.
        value.readback = Some(unsafe { ID3D12Resource::from_raw(out.readback) });
    }
    if !recorded {
        return false;
    }
    let count = out.count.min(8);
    value.planes[..count].copy_from_slice(&rows[..count]);
    value.plane_count = count as u32;
    true
}

// ---- the chain, the pipelines (this module's own per work)

/// What the work holds on this side only: the private-layer chain (StereoGeometryChain's storage) and the drawn
/// pipelines.
#[derive(Default)]
struct WorkRust {
    chain: Option<IUnknown>,
    pipelines: Vec<ID3D12PipelineState>,
    eyes: [EyeRust; 2],
    material_cache: std::collections::HashMap<usize, OwnedImmutable>,
    native_tables: std::collections::HashMap<usize, crate::native_descriptor_arena::Tables>,
    instances: std::collections::HashMap<usize, SourceInstance>,
    borrowed: BorrowedState,
    /// PublishShaders' sources and their input rows (the shader rows point at both).
    shaders: Vec<(crate::shader_snapshot_native::Source, Vec<ShaderInput>)>,
    /// CompileSources' commands (geometry_commands.rs' storage; only its source count is published).
    source_geometry: Option<IUnknown>,
    /// Each source's own on this side, by its index.
    sources: Vec<SourceRust>,
    /// The retained sources' tables, caches, passes and overlay (retained_sources.rs).
    sources_state: sources::WorkSources,
    /// Each appended source's captured tables, selectors, T5 plan and assembly plans, by its index.
    captures: Vec<sources::SourceCapture>,
}
/// A source's own on this side: its private geometry (BuildPrivateGeometry's).
#[derive(Default)]
struct SourceRust {
    private_geometry: Option<Box<PrivateGeometry>>,
}
/// A source's private geometry and its view for the .cpp (pointers into the box).
struct PrivateGeometry {
    resources: crate::geometry_leases::DrawResources,
    view: PrivateGeometryView,
}
/// What the .cpp reads of a private geometry: its draw, its index and vertex leases (geometry::BufferLease layout).
#[repr(C)]
pub struct PrivateGeometryView {
    pub draw: *const DrawRecord,
    pub index: *const crate::geometry_leases::BufferLease,
    pub vertices: *const crate::geometry_leases::BufferLease,
}
fn source_rust(index: usize) -> &'static mut SourceRust {
    let sources = &mut rust_side().sources;
    if sources.len() <= index {
        sources.resize_with(index + 1, SourceRust::default);
    }
    &mut sources[index]
}
/// The source's private geometry, null when BuildPrivateGeometry made none (the .cpp's readers, under the lock).
#[no_mangle]
pub extern "C" fn cvr_nrg2_private_geometry(index: usize) -> *const PrivateGeometryView {
    state()
        .rust
        .as_ref()
        .and_then(|r| r.sources.get(index))
        .and_then(|s| s.private_geometry.as_ref())
        .map_or(core::ptr::null(), |p| &p.view as *const PrivateGeometryView)
}
/// An eye's own on this side: its CBV snapshot arena (the Rust storage) and the arena's readback.
#[derive(Default)]
struct EyeRust {
    /// The pass-depth and tail baseline checkpoints (GeometryTargetCheckpoint).
    pass_depth: Option<std::sync::Arc<crate::pair_source::Checkpoint>>,
    tail_baseline: Option<std::sync::Arc<crate::pair_source::Checkpoint>>,
    /// The tail's target lifetimes and their borrowed scopes (target_lifetime's Lease and BorrowedScope).
    target_lifetimes: [crate::targets::LifetimeLease; 4],
    borrowed_targets: [crate::targets::BorrowedScope; 4],
    cbv_arena: Option<IUnknown>,
    cbv_readback: Option<ID3D12Resource>,
    t5_arena: Option<Option<IUnknown>>,
    t5_readback: Option<ID3D12Resource>,
    full_t5_arena: Option<Option<IUnknown>>,
    full_t5_original_readback: Option<ID3D12Resource>,
    full_t5_private_readback: Option<ID3D12Resource>,
    material_arena: Option<IUnknown>,
    ia_arena: Option<IUnknown>,
    descriptor_cache: crate::native_descriptor_arena::Cache,
    instance_readback: Option<ID3D12Resource>,
    instance_predicate: Option<ID3D12Resource>,
    instance_private_readback: Option<ID3D12Resource>,
    instance_capture: Option<Option<IUnknown>>,
}
fn rust_side() -> &'static mut WorkRust {
    state().rust.get_or_insert_with(Box::default)
}
fn chain() -> *mut c_void {
    state().rust.as_ref().and_then(|r| r.chain.as_ref()).map_or(null_mut(), |c| c.as_raw())
}
fn chain_restore(list: *mut c_void, side: u32, colors: u32) -> bool {
    crate::geometry_chain::cvr_geometry_chain_restore_native(chain(), list, eye_index(side), colors)
}
fn remember_pipeline(pipeline: *mut c_void) {
    let pipelines = &mut rust_side().pipelines;
    if pipeline.is_null() || pipelines.iter().any(|p| p.as_raw() == pipeline) {
        return;
    }
    // SAFETY: the draw's live pipeline; the reference is this list's own.
    if let Some(p) = unsafe { ID3D12PipelineState::from_raw_borrowed(&pipeline) } {
        pipelines.push(p.clone());
    }
}
fn verified(pipeline: *mut c_void) -> bool {
    state().verified.iter().any(|p| p.as_raw() == pipeline)
}

// ---- the draws

/// PureIndirectSource.
#[repr(C)]
#[derive(Clone, Default)]
pub struct PureIndirect {
    pub signature: Option<ID3D12CommandSignature>,
    pub max_command_count: u32,
    pub arguments: Option<ID3D12Resource>,
    pub argument_offset: u64,
    pub count_buffer: Option<ID3D12Resource>,
    pub count_offset: u64,
}
const _: () = assert!(
    core::mem::size_of::<PureIndirect>() == 48
        && core::mem::offset_of!(PureIndirect, arguments) == 16
        && core::mem::offset_of!(PureIndirect, count_offset) == 40
        && core::mem::size_of::<DrawRecord>() == 1096
);
fn raw_targets(targets: &Targets) -> *const cvr_gpu::scene_state::GeometryTargetsRaw {
    // SAFETY: layout-equal (COM pointers as words), asserted by both mirrors' sizes.
    (targets as *const Targets).cast()
}
fn draw_tag() -> DrawTag {
    let mut tag = DrawTag { scope: 0, stage: 0, frame: 0, side: 0 };
    // SAFETY: a writable tag.
    unsafe { crate::preparation_trace::cvr_prep_draw_tag(&mut tag) };
    tag
}
fn geometry_status(pipeline: u64) -> u32 {
    crate::families_status::native_geometry_status(pipeline as *mut c_void)
}
/// target_handoff::FinishObservation and target_access::Finish.
fn finish_targets() {
    crate::target_handoff_observer::cvr_th2_finish_observation();
    crate::target_access_observer::events::finish();
}
/// gpu::KeepCommandResources: the objects kept with the list's current recording.
///
/// # Safety
/// `objects` holds `count` live objects.
unsafe fn keep(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    // SAFETY: as documented.
    unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, objects, count) }
}
/// scene_state::CaptureGeometryTargets: the draw's targets (their references owned) into `targets`.
fn capture_targets(draw: &DrawRecord, targets: &mut Targets) -> bool {
    let lent = cvr_gpu::scene_state::DrawTargets {
        rt_count: draw.rt_count,
        rt_contiguous: draw.rt_contiguous,
        targets: draw.targets,
        depth: draw.depth,
        viewport: draw.viewport,
    };
    let mut raw = cvr_gpu::scene_state::GeometryTargetsRaw::default();
    // SAFETY: local records; the resources come with their references.
    if !unsafe { cvr_gpu::scene_state::cvr_scene_capture_geometry_targets(&lent, &mut raw) } {
        *targets = Targets::default();
        return false;
    }
    *targets = Targets {
        // SAFETY: the references the capture handed over.
        resources: core::array::from_fn(|i| unsafe { (raw.resources[i] != 0).then(|| ID3D12Resource::from_raw(raw.resources[i] as *mut c_void)) }),
        views: raw.views,
        handles: raw.handles,
        colors: raw.colors,
    };
    true
}
fn writable(list: *mut c_void, resource: &Option<ID3D12Resource>, depth: bool) -> bool {
    let state = if depth { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
    // SAFETY: live arguments.
    unsafe { crate::texture_writable::cvr_tr_validate_writable_target(list, resource.as_ref().map_or(null_mut(), |r| r.as_raw()), state.0 as u32) }
}
/// The recording of the eye's first draw (native_recording::Capture with the current camera context).
fn capture_recording(list: *mut c_void, out: &mut Recording) -> bool {
    // SAFETY: the module's own image base.
    let base = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |h| h.0 as u64);
    let mut context = crate::camera_binding::Context::default();
    // SAFETY: a writable context.
    unsafe { crate::camera_upload::cvr_ncu_current(base, &mut context) };
    let mut raw = crate::native_recording::Lease { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
    // SAFETY: the camera context is native_recording::Context's layout (asserted by both halves' C++ sides).
    let captured = unsafe { crate::native_recording::cvr_native_recording_capture(list, (&context as *const crate::camera_binding::Context).cast(), &mut raw) };
    *out = Recording {
        // SAFETY: the capture's token reference, handed over.
        token: if raw.token.is_null() { None } else { Some(unsafe { IUnknown::from_raw(raw.token) }) },
        generation: raw.generation,
        context: raw.context,
        list: raw.list,
        registry: raw.registry,
    };
    captured
}
fn list_of(list: *mut c_void) -> Option<&'static ID3D12GraphicsCommandList> {
    // SAFETY: the hook's live list.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.map(|l| unsafe { &*(l as *const ID3D12GraphicsCommandList) })
}
/// The eye's first draw: its recording, the work's device and chain, the eye's fence. Err(reason) to fail.
fn start_eye(w: &mut WorkScalars, list: *mut c_void, draw: &DrawRecord, value: &mut EyeScalars, frame: u32) -> Result<(), u32> {
    if !capture_recording(list, &mut value.recording) {
        return Err(100 + crate::native_recording::cvr_native_recording_last_failure());
    }
    if !w.created {
        let device = list_of(list).and_then(|l| {
            let mut device: Option<ID3D12Device> = None;
            // SAFETY: a live list.
            unsafe { l.GetDevice(&mut device) }.ok().and(device)
        });
        let Some(device) = device else { return Err(5) };
        w.device = Some(device.clone());
        let mut storage = null_mut();
        // SAFETY: a live device; the chain's storage handed over (one reference).
        let hr = unsafe { crate::geometry_chain::cvr_geometry_chain_create(device.as_raw(), draw.viewport.Width as u32, draw.viewport.Height as u32, &mut storage) };
        if hr.is_err() {
            return Err(5);
        }
        // SAFETY: as above.
        rust_side().chain = (!storage.is_null()).then(|| unsafe { IUnknown::from_raw(storage) });
        w.created = true;
    }
    // SAFETY: a live device.
    let fence: Option<ID3D12Fence> = w.device.as_ref().and_then(|d| unsafe { d.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok());
    let Some(fence) = fence else { return Err(6) };
    value.fence = Some(fence);
    w.frame = frame;
    value.started = true;
    Ok(())
}
fn draw(list: *mut c_void, d: &DrawRecord) -> bool {
    if !internal() && npp_active() && !cvr_nrg2_in_render_pass(list) {
        // SAFETY: a live draw record.
        unsafe { crate::prepared_pair_native::cvr_npp2_draw(list, d) };
    }
    if internal() || !active() {
        return false;
    }
    let tag = draw_tag();
    let _guard = lock();
    let Some(w) = work() else { return false };
    if w.complete {
        return false;
    }
    if w.failed {
        if let Some((side, _)) = owned(list) {
            seal(list, side, 18, exit_kind::UNKNOWN);
        }
        return false;
    }
    if d.side > 1 || tag.scope == 0 || tag.side != d.side as i32 || tag.frame == 0 {
        if let Some((side, _)) = owned(list) {
            seal(list, side, 1, exit_kind::UNKNOWN);
        }
        return false;
    }
    let side = d.side;
    let value = w.eye(side as usize);
    if cvr_nrg2_in_pass_locked(list) {
        if value.started {
            fail(9);
        }
        return false;
    }
    if value.started && (value.sealed || value.recording.list != list as u64) {
        return false;
    }
    if !value.started && tag.stage != w.stage {
        return false;
    }
    if w.frame != 0 && w.frame != tag.frame {
        return false;
    }
    let policy = geometry_status(d.pipeline);
    let mut targets = Targets::default();
    if policy != 1 || d.flags & 64 != 0 || !capture_targets(d, &mut targets) || targets.views[4].view_flags != 0 {
        if value.started {
            seal(list, side, 2, exit_kind::UNKNOWN);
        } else {
            fail(if policy != 1 { 400 + policy } else { 3 });
        }
        return false;
    }
    if w.mode == 2 && !verified(d.pipeline as *mut c_void) {
        fail(4);
        if value.started {
            seal(list, side, 3, exit_kind::UNKNOWN);
        }
        return false;
    }
    // SAFETY: both target sets are GeometryTargetsRaw's layout.
    if value.started && unsafe { crate::retained_targets::cvr_nrg_targets_changed(raw_targets(&value.targets), &value.imported, raw_targets(&targets)) } {
        seal(list, side, 19, exit_kind::UNKNOWN);
        return false;
    }
    if !value.started {
        if let Err(reason) = start_eye(w, list, d, value, tag.frame) {
            fail(reason);
            return false;
        }
    }
    let stats = ex!(CyberpunkVR_RetainedGeometryStats);
    for slot in 0..5usize {
        if (slot == 4 || slot < targets.colors as usize) && !value.imported[slot] && !writable(list, &targets.resources[slot], slot == 4) {
            fail(200 + crate::texture_lease::cvr_tr_failure());
            if stats[side as usize] != 0 {
                seal(list, side, 4, exit_kind::UNKNOWN);
            } else {
                value.sealed = true;
            }
            return false;
        }
    }
    if w.source_ia_snapshots && !value.imported[4] {
        let baseline = &mut ex!(CyberpunkVR_RetainedBaseline)[side as usize * 8..][..8];
        baseline[0] = u64::from(w.frame);
        baseline[1] = u64::from(targets.colors);
        baseline[5] = 1;
        const BUDGET: u64 = 64 * 1024 * 1024;
        let resources: [*mut c_void; 5] = core::array::from_fn(|i| targets.resources[i].as_ref().map_or(null_mut(), |r| r.as_raw()));
        let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
        let slots = 0x10u32 | ((1u32 << targets.colors.min(4)) - 1);
        // SAFETY: the targets' live resources; a writable total.
        if !unsafe { crate::retained_readbacks::cvr_nrg_target_bytes(device, &resources, slots, BUDGET, &mut baseline[2]) } {
            baseline[4] = 1; // the depth and colour targets' bytes within the budget
        }
        if baseline[4] == 0 {
            // SAFETY: the host's own function.
            match pairs::pass_depth(w, list, side, &targets) {
                0 => baseline[3] = 1,
                code => baseline[4] = u64::from(code),
            }
        }
        if baseline[3] == 0 {
            fail(2440 + baseline[4] as u32);
            value.sealed = true;
            return false;
        }
    }
    if w.tail_enabled && tag.stage == 9 && !pairs::tail(w, list, side, &targets) {
        return false;
    }
    let eye = eye_index(side);
    // SAFETY: the chain's storage and the draw's targets.
    let prepared = unsafe { crate::geometry_chain::cvr_geometry_chain_prepare_eye(chain(), list, eye, raw_targets(&targets).cast()) };
    if prepared.is_err() || !crate::geometry_chain::cvr_geometry_chain_bind(chain(), list, 1 << eye, targets.colors) {
        fail(7);
        if stats[side as usize] != 0 {
            seal(list, side, 5, exit_kind::UNKNOWN);
        }
        return false;
    }
    for slot in 0..5usize {
        if slot == 4 || slot < targets.colors as usize {
            value.targets.resources[slot] = targets.resources[slot].clone();
            value.targets.views[slot] = targets.views[slot];
            value.targets.handles[slot] = targets.handles[slot];
            value.imported[slot] = true;
        }
    }
    value.targets.colors = targets.colors;
    value.colors = targets.colors;
    if value.scope != tag.scope {
        value.scope = tag.scope;
        stats[2 + side as usize] += 1;
    }
    if tag.stage < 64 {
        ex!(CyberpunkVR_RetainedGeometryEnds)[2 + side as usize] |= 1u64 << tag.stage;
    }
    work::append_source(list, side, d, None, None);
    if w.failed {
        if !chain_restore(list, side, targets.colors) {
            fail(8);
        }
        publish_sources();
        return false;
    }
    remember_pipeline(d.pipeline as *mut c_void);
    let native_tables = bind_native_source_tables(list);
    if w.failed {
        chain_restore(list, side, targets.colors);
        return false;
    }
    if let Some(l) = list_of(list) {
        crate::copy_windows::read_only(true);
        {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list; the draw's own arguments.
            unsafe { l.DrawIndexedInstanced(d.indices, d.instances, d.first_index, d.base_vertex, d.first_instance) };
        }
        crate::copy_windows::read_only(false);
    }
    if native_tables {
        let _internal = cvr_host::probe::Internal::new();
        restore_last_bindings(list);
    }
    if !chain_restore(list, side, targets.colors) {
        fail(8);
    }
    stats[side as usize] += 1;
    publish();
    w.mode == 2
}
fn pure_draw_stride(signature: *mut c_void) -> u32 {
    let mut receipt = crate::indirect_policy::Receipt::default();
    // SAFETY: a writable receipt.
    unsafe { crate::indirect_policy::cvr_indirect_describe(signature, &mut receipt) };
    // SAFETY: as above.
    unsafe { crate::indirect_policy::cvr_indirect_receipt_pure_stride(&receipt) }
}
#[allow(clippy::too_many_arguments)]
fn indirect(list: *mut c_void, signature: *mut c_void, count: u32, arguments: *mut c_void, offset: u64, count_buffer: *mut c_void, count_offset: u64) -> bool {
    if internal() || !active() {
        return false;
    }
    let tag = draw_tag();
    let _guard = lock();
    let Some((side, value)) = owned(list) else { return false };
    let Some(w) = work() else { return false };
    if w.complete {
        return false;
    }
    // SAFETY: a plain draw record, written whole on success.
    let mut current: DrawRecord = unsafe { core::mem::zeroed() };
    let mut targets = Targets::default();
    let stride = pure_draw_stride(signature);
    // SAFETY: a writable record.
    let state_known = unsafe { crate::stereo_probe_state::cvr_nsps_current(list, &mut current) };
    let policy = if state_known { geometry_status(current.pipeline) } else { 0 };
    let captured = state_known && capture_targets(&current, &mut targets);
    let mut valid = !w.failed
        && stride != 0
        && !arguments.is_null()
        && offset % 4 == 0
        && count_offset % 4 == 0
        && (tag.scope == 0 || (tag.frame == w.frame && tag.side == side as i32))
        && state_known
        && current.flags & 64 == 0
        && policy == 1
        && captured
        && targets.views[4].view_flags == 0;
    let same_device = |resource: *mut c_void, at: u64, bytes: u64| -> bool {
        // SAFETY: the call's live resource.
        let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else { return false };
        // SAFETY: a live resource.
        let desc = unsafe { r.GetDesc() };
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: the description's own storage; a live resource.
        let held = unsafe { crate::retained_indirect::cvr_nrg_indirect_buffer(&desc, at, bytes) };
        // SAFETY: a live resource.
        held && unsafe { r.GetDevice(&mut device) }.is_ok()
            && device.as_ref().map(|d| d.as_raw()) == w.device.as_ref().map(|d| d.as_raw())
    };
    if valid {
        valid = same_device(arguments, offset, u64::from(count) * u64::from(stride));
        if valid && !count_buffer.is_null() {
            valid = same_device(count_buffer, count_offset, 4);
        }
    }
    if valid {
        // SAFETY: both target sets are GeometryTargetsRaw's layout.
        valid = unsafe { crate::retained_indirect::cvr_nrg_indirect_targets(raw_targets(&value.targets), &value.imported, raw_targets(&targets)) };
    }
    let pipeline = current.pipeline as *mut c_void;
    if valid && w.mode == 2 && !verified(pipeline) {
        valid = false;
    }
    if !valid {
        let mut receipt = crate::indirect_policy::Receipt::default();
        // SAFETY: a writable receipt.
        unsafe { crate::indirect_policy::cvr_indirect_describe(signature, &mut receipt) };
        let details = [
            u64::from(stride),
            u64::from(receipt.kind),
            u64::from(receipt.stride),
            tag.scope,
            u64::from(tag.stage),
            u64::from(policy),
            u64::from(captured),
            u64::from(current.flags),
        ];
        ex!(CyberpunkVR_RetainedGeometryIndirectRejected).copy_from_slice(&details);
        seal(list, side, 9, exit_kind::UNKNOWN);
        return false;
    }
    current.side = side;
    current.frame = w.frame;
    current.list = list as u64;
    let borrow = |p: *mut c_void| -> Option<IUnknown> {
        // SAFETY: the call's live objects; a new reference each.
        unsafe { IUnknown::from_raw_borrowed(&p) }.cloned()
    };
    let source = PureIndirect {
        signature: borrow(signature).and_then(|o| o.cast().ok()),
        max_command_count: count,
        arguments: borrow(arguments).and_then(|o| o.cast().ok()),
        argument_offset: offset,
        count_buffer: borrow(count_buffer).and_then(|o| o.cast().ok()),
        count_offset,
    };
    let mut replay = source.clone();
    work::append_source(list, side, &current, Some(&source), Some(&mut replay));
    if w.failed {
        if !chain_restore(list, side, targets.colors) {
            fail(23);
        }
        publish_sources();
        return false;
    }
    let kept = {
        let objects = [signature, arguments, pipeline];
        // SAFETY: three live objects.
        let three = unsafe { keep(list, objects.as_ptr(), 3) };
        // SAFETY: as above; the count buffer alone.
        three && (count_buffer.is_null() || unsafe { keep(list, &count_buffer, 1) })
    };
    if !kept {
        fail(24);
        seal(list, side, 21, exit_kind::UNKNOWN);
        return false;
    }
    let eye = eye_index(side);
    if !crate::geometry_chain::cvr_geometry_chain_bind(chain(), list, 1 << eye, targets.colors) {
        fail(22);
        seal(list, side, 20, exit_kind::UNKNOWN);
        return false;
    }
    remember_pipeline(pipeline);
    let native_tables = bind_native_source_tables(list);
    if w.failed {
        chain_restore(list, side, targets.colors);
        return false;
    }
    if let Some(l) = list_of(list) {
        crate::copy_windows::read_only(true);
        {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list; the replay's own objects.
            unsafe {
                l.ExecuteIndirect(
                    replay.signature.as_ref(),
                    replay.max_command_count,
                    replay.arguments.as_ref(),
                    replay.argument_offset,
                    replay.count_buffer.as_ref(),
                    replay.count_offset,
                )
            };
        }
        crate::copy_windows::read_only(false);
    }
    if native_tables {
        let _internal = cvr_host::probe::Internal::new();
        restore_last_bindings(list);
    }
    if w.source_snapshots {
        let replayed = replay.arguments.as_ref().map_or(0, |a| a.as_raw() as u64);
        if let Some(last) = rows_mut::<24>(w, SNAPSHOT_ROWS).last_mut() {
            if last[11] == replayed {
                last[21] = 1;
                publish_sources();
            }
        }
    }
    if !chain_restore(list, side, targets.colors) {
        fail(23);
    }
    let stats = ex!(CyberpunkVR_RetainedGeometryStats);
    if tag.scope != 0 && value.scope != tag.scope {
        value.scope = tag.scope;
        stats[2 + side as usize] += 1;
    }
    if tag.scope != 0 && tag.stage < 64 {
        ex!(CyberpunkVR_RetainedGeometryEnds)[2 + side as usize] |= 1u64 << tag.stage;
    }
    stats[side as usize] += 1;
    ex!(CyberpunkVR_RetainedGeometryIndirect)[side as usize] += 1;
    w.mode == 2
}
const SNAPSHOT_ROWS: u32 = 26;
const STAGED_ROWS: u32 = 11;
const SOURCE_ROWS: u32 = 24;

// ---- the seal

fn restore_targets(list: *mut c_void, draw: &DrawRecord) {
    let Some(l) = list_of(list) else { return };
    let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: draw.depth as usize };
    let handles: [D3D12_CPU_DESCRIPTOR_HANDLE; 8] = core::array::from_fn(|i| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: draw.targets[i] as usize });
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: a live list; the recorded handles.
    unsafe {
        l.OMSetRenderTargets(
            draw.rt_count,
            if draw.rt_count != 0 { Some(handles.as_ptr()) } else { None },
            draw.rt_contiguous != 0,
            if draw.depth != 0 { Some(&depth) } else { None },
        )
    };
}
fn seal_impl(list: *mut c_void, side: u32, reason: u32, kind: u32) {
    let Some(w) = work() else { return };
    let value = w.eye(side as usize);
    if !value.started || value.sealed || value.recording.list != list as u64 {
        return;
    }
    value.seal_reason = reason;
    if w.tail_enabled {
        crate::target_handoff_observer::cvr_th2_mark_exit(list, reason, kind);
        crate::target_access_observer::events::mark_exit(list);
    }
    // SAFETY: a plain draw record, written whole on success.
    let mut actual: DrawRecord = unsafe { core::mem::zeroed() };
    // SAFETY: a writable record.
    let restore = unsafe { crate::stereo_probe_state::cvr_nsps_current(list, &mut actual) };
    if w.mode == 1 {
        // The diagnostic proof scope around Proof over the eye's colours and depth, failing with 40 + reason.
        let t = &value.targets.resources;
        let held = [&t[0], &t[1], &t[2], &t[4]].map(|r| r.as_ref().map_or(null_mut(), |r| r.as_raw()));
        let (proof, generation) = crate::target_access_observer::events::proof_begin(list, &held);
        let proved = cvr_nrg2_proof(list, side);
        if !proved {
            fail(40 + reason);
        }
        crate::target_access_observer::events::end_proof(list, generation, proof, proved);
    } else if !crate::geometry_chain::cvr_geometry_chain_export_eye(chain(), list, eye_index(side)) {
        fail(60 + reason);
    }
    if restore {
        restore_targets(list, &actual);
    }
    seal_cbv(w, list, side);
    seal_t5(w, list, side);
    seal_ia(w, list, side);
    seal_material(list, side);
    seal_instances(w, list, side);
    value.sealed = true;
    ex!(CyberpunkVR_RetainedGeometryStats)[4 + side as usize] = 1;
    publish();
    let count = ex!(CyberpunkVR_RetainedSourceBoundaryCount);
    if w.source_stream && *count < 2 {
        let peer = w.eye(1 - side as usize);
        let sources = rows_mut::<48>(w, SOURCE_ROWS);
        let ia = rows_mut::<16>(w, IA_COMMANDS);
        let counted = boundary_counts(list, side);
        let mut word = 0u64;
        let mut copied = 0usize;
        let registry = value.recording.registry;
        if registry != 0 && registry <= u64::MAX - 0x14104D8 {
            // SAFETY: a read of this process' memory that may fail.
            let _ = unsafe {
                windows::Win32::System::Diagnostics::Debug::ReadProcessMemory(
                    windows::Win32::System::Threading::GetCurrentProcess(),
                    (registry + 0x14104D0) as *const c_void,
                    (&mut word as *mut u64).cast(),
                    8,
                    Some(&mut copied),
                )
            };
        }
        let finalized = arena_op(side, arena::CBV, 1);
        let row = [
            u64::from(w.frame),
            u64::from(side),
            u64::from(*count),
            value.recording.list,
            value.recording.generation,
            sources.iter().filter(|s| s[1] == u64::from(side)).count() as u64,
            sources.len() as u64,
            u64::from(peer.sealed),
            u64::from(peer.submitted),
            u64::from(peer.submission_certified),
            peer.source_queue.as_ref().map_or(0, |q| q.as_raw() as u64),
            peer.fence.as_ref().map_or(0, |f| f.as_raw() as u64),
            peer.signaled_value,
            // SAFETY: a live fence.
            peer.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() }),
            ia.iter().filter(|c| c[1] == u64::from(side) && c[14] != 0).count() as u64,
            counted[0],
            if copied == 8 { word } else { 0 },
            u64::from(finalized),
            counted[1],
            u64::from(w.payloads_processed),
        ];
        ex!(CyberpunkVR_RetainedSourceBoundaryRows)[*count as usize * 20..][..20].copy_from_slice(&row);
        *count += 1;
    }
    compile_sources();
    publish_sources();
    ex!(CyberpunkVR_RetainedGeometryEnds)[side as usize] = u64::from(reason);
    if w.eye(0).sealed && w.eye(1).sealed && !w.failed {
        CyberpunkVR_RetainedGeometryState.store(2, SeqCst);
    }
}

// ---- the sources

/// A retained source (formerly the .cpp's Work::Source with its shared scalars), owned by the work
/// (retained_work.rs): the raw mirrors' COM pointers are references the source holds, released when it drops.
pub struct SourceScalars {
    pub indirect_proof: crate::indirect_copy_native::Proof,
    pub cbv_prepared: crate::source_cbv_snapshot::PreparedCopies,
    pub ia_prepared: crate::ia_snapshot_receipt::PreparedCopies,
    pub t5_prepared: crate::structured_snapshot_receipt::PreparedCopy,
    pub full_t5_prepared: crate::structured_snapshot_receipt::PreparedCopy,
    pub multilayer_t5: crate::material_buffer_snapshot::SrvBinding,
    pub material_rows: Vec<usize>,
    pub material_owners: Vec<crate::texture_immutable::ImmutableLease>,
    pub parent: GraphParentRecord,
    pub cbvs: Vec<crate::source_cbv_snapshot::SourceCbvBacking>,
    pub private_cbvs: Vec<crate::source_cbv_snapshot::PrivateCbv>,
    pub t5_native: crate::source_srv_observe::Backing,
    pub t5_private: crate::structured_snapshot_receipt::PrivateRange,
    pub full_t5_private: crate::structured_snapshot_receipt::PrivateRange,
    pub t5_row: usize,
    pub t5_original_certified: bool,
    pub material_buffers: Vec<MaterialBuffer>,
    pub material_truncated: bool,
    pub material_inputs_ready: bool,
    pub ia_views: Vec<crate::ia_snapshot_receipt::PrivateIa>,
    pub bindings: cvr_gpu::scene_state::Bindings,
    /// The source's root signature (lent; `held` references it).
    pub root: *mut c_void,
    /// The command's geometry (its leases, draw, instances and state) and its optional pure indirect operands.
    pub geometry: crate::geometry_leases::DrawResources,
    pub indirect: Option<PureIndirect>,
    pub shader: Option<crate::shader_snapshot_native::Source>,
    /// The bindings' root, compute root and descriptor heaps referenced.
    pub held: Vec<IUnknown>,
    pub indirect_binding: crate::ia_binding::Binding,
    pub indirect_snapshot: work::IndirectSnapshot,
}
const _: () = assert!(
    core::mem::size_of::<crate::texture_immutable::ImmutableLease>() == 56
        && core::mem::size_of::<cvr_gpu::scene_state::Bindings>() == 1136
        && core::mem::size_of::<MaterialBuffer>() == 1200
        && core::mem::size_of::<crate::material_buffer_snapshot::SourceResourceBinding>() == 208
);
/// The last source's bindings restored on the list (scene_state::Bindings::Restore).
fn restore_last_bindings(list: *mut c_void) {
    let count = source_count();
    if let Some(last) = count.checked_sub(1).and_then(|i| source(i as u64)) {
        // SAFETY: the source's mirrored bindings and the live list.
        unsafe { cvr_gpu::scene_state::cvr_scene_bindings_restore(&last.bindings, list) };
    }
}
/// The source's CBV family: the pipeline's registry family whose source root is the source's, the source holding its
/// shader snapshot.
fn cbv_family(source: &SourceScalars, pipeline: *mut c_void) -> Option<std::sync::Arc<crate::families_native::Family>> {
    let family = crate::families_native::lookup_raw(pipeline)?;
    // SAFETY: the source's std::shared_ptr<const Snapshot> (its pointer first).
    let shader = unsafe { *source.shader_shared().cast::<*const c_void>() };
    (family.source_root.as_raw() == source.root && !shader.is_null()).then_some(family)
}
/// The work's source `index`, None past its end.
fn source(index: u64) -> Option<&'static SourceScalars> {
    work::source_at(index as usize).map(|s| &*s)
}
fn source_count() -> usize {
    work::sources_len()
}
/// Eye arenas (`which`): the CBV, T5, full-T5, IA and material arenas.
mod arena {
    pub const CBV: u32 = 0;
    pub const T5: u32 = 1;
    pub const FULL_T5: u32 = 2;
    pub const IA: u32 = 3;
}
fn arena_op(side: u32, which: u32, op: u32) -> bool {
    if which == arena::CBV {
        if op == 0 {
            cbv_arena_reset(side);
            return false;
        }
        return cbv_arena_finalized(side);
    }
    if which == 4 {
        if op == 0 {
            material_abort(side);
        }
        return false;
    }
    if which == arena::IA {
        if op == 0 {
            ia_abort(side);
            return false;
        }
        return ia_finalized(side);
    }
    if which == arena::T5 || which == arena::FULL_T5 {
        let full = which == arena::FULL_T5;
        if op == 0 {
            structured_abort(side, full);
            return false;
        }
        return structured_finalized(side, full);
    }
    false
}
const MATERIAL_ROWS: u32 = 15;
const INSTANCE_ROWS: u32 = 19;
const CBV_SNAPSHOT_ROWS: u32 = 33;
const CBV_SNAPSHOT_COMMANDS: u32 = 34;
const SELECTOR_COMMANDS: u32 = 41;
const T5_ROWS: u32 = 42;
const T5_SNAPSHOT_ROWS: u32 = 48;
const FULL_T5_ROWS: u32 = 2;
const NATIVE_TABLE_ROWS: u32 = 12;
/// ia_snapshot::Failure.
const IA_CHANGED: u64 = 25;
const IA_SUBMISSION: u64 = 30;
/// indirect_copy::Failure::Ownership.
const INDIRECT_OWNERSHIP: u64 = 13;

// ---- the submission's checks

fn validate_recording_submission(recording: &Recording, queue: *mut c_void) -> bool {
    // SAFETY: the eye's lease (native_recording::Lease's layout) and the hook's queue.
    unsafe { crate::native_recording::cvr_native_recording_validate_submission((recording as *const Recording).cast(), queue) }
}
fn before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) -> bool {
    crate::prepared_pair_native::cvr_npp2_before_submit(queue, count, lists);
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return false;
    }
    let _guard = lock();
    let Some(w) = work() else { return false };
    if w.complete {
        return false;
    }
    // SAFETY: `count` lists.
    let list_slice = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let mut serialize = false;
    for side in 0..2u32 {
        let value = w.eye(side as usize);
        if !value.started || value.submitted {
            continue;
        }
        let occurrences = list_slice.iter().filter(|l| **l as u64 == value.recording.list).count();
        if occurrences == 0 {
            continue;
        }
        serialize = true;
        value.pending = true;
        if !value.sealed || occurrences != 1 || !validate_recording_submission(&value.recording, queue) {
            fail(300 + crate::native_recording::cvr_native_recording_last_failure());
        }
        submit_eye(w, side, queue);
    }
    publish_sources();
    if serialize {
        sources::guarded(w);
        pairs::build_staged_sources(w);
        pairs::start_current_sources(w, queue);
        observe_staged_residency(queue, count, lists);
        if ex!(CyberpunkVR_RetainedStagedResidency)[0] == 3 {
            let details = ex!(CyberpunkVR_RetainedStagedResidencyDetails) as *mut [u64; 12];
            let membership = ex!(CyberpunkVR_RetainedStagedMembershipDetails) as *mut [u64; 12];
            // SAFETY: writable rows of twelve.
            unsafe {
                crate::queue_residency::cvr_qr_last_details(details);
                crate::queue_residency::cvr_qr_membership_details(membership);
            }
        }
        let stats = ex!(CyberpunkVR_RetainedGeometryStats);
        stats[8] += 1;
        stats[9] += u64::from(count);
    }
    serialize
}
/// ObserveStagedResidency (formerly the .cpp's): at the submission that serialised the staged packets, every staged
/// packet's immutable materials (once per eye and native handle, one resource each) valid on the queue and members of
/// the submission's residency scope, the scope itself valid.
fn observe_staged_residency(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    let Some(w) = work() else { return };
    let stats = ex!(CyberpunkVR_RetainedStagedResidency);
    let staged = ex!(CyberpunkVR_RetainedStagedStats);
    if w.failed || stats[0] != 0 || staged[0] != 2 {
        return;
    }
    stats[0] = 1;
    stats[1] = u64::from(w.frame);
    stats[2] = staged[3];
    stats[8] = u64::from(count);
    // SAFETY: the submission's `count` lists.
    let submitted = if lists.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(lists, count as usize) } };
    for side in 0..2usize {
        for list in submitted {
            if *list as u64 == w.eye(side).recording.list {
                stats[7] |= 1 << side;
            }
        }
    }
    if stats[7] != 3 {
        stats[0] = 4;
        return;
    }
    let mut raw = null_mut();
    // SAFETY: the live queue and its lists; a local output.
    if !unsafe { crate::queue_residency::cvr_qr_capture(queue, count, lists as u64, &mut raw) } {
        stats[0] = 3;
        stats[6] = u64::from(crate::queue_residency::cvr_qr_last_failure());
        return;
    }
    // SAFETY: the new scope's one reference (released when this returns, as the .cpp's Scope was).
    let scope = (!raw.is_null()).then(|| unsafe { IUnknown::from_raw(raw) });
    let proof = scope.as_ref().map_or(null_mut(), |s| s.as_raw());
    let rows: Vec<[u64; 8]> = rows_mut::<8>(w, STAGED_ROWS).to_vec();
    let mut seen: std::collections::BTreeMap<(u32, u32), u64> = std::collections::BTreeMap::new();
    for row in rows.iter().filter(|row| row[2] != 0) {
        let ordinal = row[0] as usize;
        let Some(source) = source(ordinal as u64) else { continue };
        let side = geometry_of(source).prefix.draw.side;
        stats[3] += 1;
        let materials = pairs::staged_immutables(ordinal);
        for material in materials {
            // SAFETY: a staged material's lease (alive with the work's source).
            let immutable = unsafe { &*material };
            let texture = immutable.source.native;
            match seen.entry((side, texture.handle)) {
                std::collections::btree_map::Entry::Occupied(at) => {
                    if *at.get() != texture.resource {
                        stats[0] = 3;
                        stats[6] = 10;
                        return;
                    }
                    continue;
                }
                std::collections::btree_map::Entry::Vacant(at) => {
                    at.insert(texture.resource);
                }
            }
            stats[4] += 1;
            // SAFETY: a live lease and queue.
            if !unsafe { crate::texture_immutable::cvr_tr_validate_immutable(immutable, queue) } {
                stats[0] = 3;
                stats[6] = 100 + u64::from(crate::texture_lease::cvr_tr_failure());
                return;
            }
            let recording = eye_lease(w, side as usize) as *const crate::native_recording::Lease;
            // SAFETY: the scope, the eye's recording and the texture's identity.
            if !unsafe { crate::queue_residency::cvr_qr_contains_runtime(proof, recording.cast(), texture.registry, texture.handle, texture.resource) } {
                stats[0] = 3;
                stats[6] = u64::from(crate::queue_residency::cvr_qr_last_failure());
                return;
            }
            stats[5] += 1;
        }
    }
    // SAFETY: the scope.
    stats[9] = u64::from(unsafe { crate::queue_residency::cvr_qr_validate(proof) });
    stats[10] = u64::from(w.frame);
    if stats[9] == 0 {
        stats[0] = 3;
        stats[6] = u64::from(crate::queue_residency::cvr_qr_last_failure());
        return;
    }
    stats[0] = 2;
}
/// StereoGeometryCommands::MaxSourceCommands.
const MAX_SOURCE_COMMANDS: usize = 2048;
unsafe extern "C-unwind" fn describe_signature(signature: *mut c_void, receipt: *mut crate::indirect_policy::Receipt) {
    // SAFETY: a live signature (or null); the caller's receipt.
    unsafe {
        *receipt = match windows::Win32::Graphics::Direct3D12::ID3D12CommandSignature::from_raw_borrowed(&signature) {
            Some(s) => crate::indirect_policy::describe(s),
            None => Default::default(),
        };
    }
}
fn geometry_counts(storage: &IUnknown) -> (u32, u32, u64) {
    let (mut draws, mut sources, mut bytes) = (0, 0, 0);
    // SAFETY: a live storage; local outputs.
    unsafe { crate::geometry_commands::cvr_sgc_counts(storage.as_raw(), &mut draws, &mut sources, &mut bytes) };
    (draws, sources, bytes)
}
/// CompileSources (formerly the .cpp's): once both eyes sealed, every source's command (its geometry, its replayed
/// indirect source) compiled into geometry commands -- refused (907) when they could not be.
fn compile_sources() {
    let Some(w) = work() else { return };
    if !w.source_stream || w.source_failed || w.source_compiled || !w.eye(0).sealed || !w.eye(1).sealed {
        return;
    }
    let count = source_count();
    let mut indirects: Vec<crate::geometry_commands::IndirectView> = Vec::with_capacity(count);
    let mut lent: Vec<crate::geometry_commands::SourceInput> = Vec::with_capacity(count);
    for index in 0..count {
        let Some(source) = source(index as u64) else { break };
        let mut view = core::ptr::null();
        if let Some(indirect) = source.indirect.as_ref() {
            indirects.push(work::lend_indirect(indirect));
            view = indirects.last().map_or(core::ptr::null(), |v| v as *const _);
        }
        lent.push(crate::geometry_commands::SourceInput { geometry: source.geometry.lend(), indirect: view });
    }
    let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
    let mut created = null_mut();
    let hr = if lent.is_empty() || lent.len() > MAX_SOURCE_COMMANDS || device.is_null() || rust_side().source_geometry.is_some() {
        windows::Win32::Foundation::E_INVALIDARG
    } else {
        // SAFETY: the lent commands live for the call.
        unsafe { crate::geometry_commands::cvr_sgc_create_sources(device, lent.as_ptr(), lent.len(), describe_signature, &mut created) }
    };
    if hr.is_err() || created.is_null() {
        w.source_failed = true;
        ex!(CyberpunkVR_RetainedSourceStreamStats)[12] = 907;
    } else {
        // SAFETY: the new storage's one reference.
        let storage = unsafe { IUnknown::from_raw(created) };
        w.source_compiled = geometry_counts(&storage).1 as usize == count;
        rust_side().source_geometry = Some(storage);
    }
    publish_sources();
}
/// One eye's sources checked against the queue that executes its recording.
fn submit_eye(w: &mut WorkScalars, side: u32, queue: *mut c_void) {
    let value = w.eye(side as usize);
    let s = u64::from(side);
    let material_rows = rows_mut::<24>(w, MATERIAL_ROWS);
    for index in 0..source_count() {
        let Some(source) = source(index as u64) else { continue };
        let owners = source.material_owners.as_slice();
        for (n, &at) in source.material_rows.as_slice().iter().enumerate() {
            let Some(row) = material_rows.get_mut(at) else { continue };
            if row[1] != s {
                continue;
            }
            if row[8] == 1 {
                row[17] = u64::from(!w.failed);
            } else if row[8] == 5 {
                // SAFETY: the source's immutable lease (texture_read::ImmutableLease) and the hook's queue.
                row[17] = u64::from(!w.failed && owners.get(n).is_some_and(|lease| unsafe { crate::texture_immutable::cvr_tr_validate_immutable(lease, queue) }));
            }
        }
    }
    if w.source_snapshots {
        for snapshot in rows_mut::<24>(w, SNAPSHOT_ROWS) {
            if snapshot[1] != s || snapshot[2] == 0 {
                continue;
            }
            // SAFETY: the source's indirect copy proof and the hook's queue.
            if source(snapshot[0]).is_some_and(|source| unsafe { crate::indirect_copy_native::cvr_ic_validate_submission(&source.indirect_proof, queue) }) {
                snapshot[20] = 1;
            } else {
                w.snapshot_failed = true;
                snapshot[5] = u64::from(crate::indirect_copy_native::cvr_ic_last_failure());
                if snapshot[5] == 0 {
                    snapshot[5] = INDIRECT_OWNERSHIP;
                }
            }
        }
    }
    if w.source_ia_snapshots {
        let sources = source_count() as u64;
        for instance in rows_mut::<48>(w, INSTANCE_ROWS) {
            if instance[1] == s && instance[4] != 0 && instance[0] < sources {
                let candidate = rust_side().instances.get(&(instance[0] as usize)).and_then(|i| i.candidate.as_ref());
                instance[15] = u64::from(candidate.is_some_and(|c| crate::instance_observer_native::validate(c, queue)));
            }
        }
        let mut error = if value.ia_aborted { IA_SUBMISSION } else { 0 };
        for command in rows_mut::<16>(w, IA_COMMANDS).iter() {
            if command[1] != s || command[6] == 0 {
                continue;
            }
            if command[7] != 0 || command[14] == 0 || command[0] >= sources {
                error = IA_SUBMISSION;
            } else if error == 0 {
                let Some(source) = source(command[0]) else { continue };
                let proof = &source.ia_prepared;
                if proof.list != command[10]
                    || proof.generation != command[11]
                    || proof.occurrence != command[12]
                    || proof.owner as u64 != command[13]
                    || u64::from(proof.count) != command[8]
                    || proof.copied_bytes != command[9]
                {
                    error = IA_CHANGED;
                // SAFETY: the source's IA proof and the hook's queue.
                } else if !unsafe { crate::ia_snapshot_receipt::cvr_iasn_validate_submission(proof, queue) } {
                    error = crate::ia_snapshot_receipt::cvr_iasn_last_failure();
                }
            }
        }
        if error != 0 {
            value.ia_aborted = true;
            arena_op(side, arena::IA, 0);
            fail(1780 + error as u32);
            for command in rows_mut::<16>(w, IA_COMMANDS) {
                if command[1] == s && command[6] != 0 && command[7] == 0 {
                    command[7] = u64::from(error as u32);
                }
            }
        }
        value.ia_validated = error == 0 && !w.failed;
    }
    if w.source_cbv_snapshots {
        let mut error: u32 = if value.cbv_arena_aborted { 8 } else { 0 };
        let sources = source_count() as u64;
        for snapshot in rows_mut::<24>(w, CBV_SNAPSHOT_COMMANDS).iter() {
            if snapshot[1] != s || snapshot[2] != 1 {
                continue;
            }
            if snapshot[0] >= sources {
                error = 9;
            } else if error == 0 {
                // SAFETY: the source's CBV copies and the hook's queue.
                let valid = source(snapshot[0]).is_some_and(|source| unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_validate_submission((&source.cbv_prepared as *const crate::source_cbv_snapshot::PreparedCopies).cast(), queue) });
                if !valid {
                    error = 100 + crate::source_cbv_snapshot_receipt::cvr_scsn_last_failure();
                }
            }
        }
        if error != 0 {
            w.cbv_snapshots_failed = true;
            value.cbv_arena_aborted = true;
            fail(1200 + error);
        }
        for snapshot in rows_mut::<24>(w, CBV_SNAPSHOT_COMMANDS) {
            if snapshot[1] == s && snapshot[2] == 1 {
                snapshot[18] = u64::from(error == 0);
                if error != 0 {
                    snapshot[3] = u64::from(error);
                }
            }
        }
        for row in rows_mut::<32>(w, CBV_SNAPSHOT_ROWS) {
            if row[1] == s && row[8] == 1 {
                row[30] = u64::from(error == 0);
            }
        }
    }
    if w.source_selectors {
        let mut failure = 0u32;
        let sources = source_count() as u64;
        for row in rows_mut::<32>(w, T5_SNAPSHOT_ROWS).iter() {
            if row[1] != s || row[2] != 1 {
                continue;
            }
            if value.t5_aborted || row[0] >= sources {
                failure = 1;
            } else if failure == 0 {
                // SAFETY: the source's T5 copy proof and the hook's queue.
                let valid = source(row[0]).is_some_and(|source| unsafe { crate::structured_snapshot_receipt::cvr_sbsn_validate_submission(&source.t5_prepared, queue) });
                if !valid {
                    failure = crate::structured_snapshot_receipt::cvr_sbsn_last_failure();
                }
            }
        }
        if failure != 0 {
            value.t5_aborted = true;
            arena_op(side, arena::T5, 0);
            fail(1500 + failure);
        }
        for row in rows_mut::<32>(w, T5_SNAPSHOT_ROWS) {
            if row[1] == s && row[2] == 1 {
                row[24] = u64::from(failure == 0);
            }
        }
        for command in rows_mut::<32>(w, SELECTOR_COMMANDS) {
            if command[1] == s && command[0] < sources {
                command[23] += u64::from(sources::selector_mismatches(command[0] as usize));
                command[24] = u64::from(!w.failed);
            }
        }
        for row in rows_mut::<32>(w, T5_ROWS) {
            if row[1] == s && row[2] != 0 {
                row[29] = u64::from(source(row[0]).is_some_and(|source| {
                    let stamp = (&source.multilayer_t5.stamp as *const crate::source_cbv_binding::DescriptorStamp).cast::<crate::descriptor_audit::DescriptorStamp>();
                    // SAFETY: the source's descriptor stamp, only read (MatchesDescriptor).
                    unsafe { crate::descriptor_audit::cvr_sda_audit_stamp(1, 0, 0, 0, 0, stamp.cast_mut(), null_mut()) }
                }));
            }
        }
    }
    if w.full_t5_enabled {
        let stats = ex!(CyberpunkVR_RetainedFullT5Stats);
        let sources = source_count() as u64;
        for row in rows_mut::<16>(w, FULL_T5_ROWS) {
            if row[1] != s {
                continue;
            }
            let valid = row[0] < sources
                && arena_op(side, arena::FULL_T5, 1)
                // SAFETY: the source's full-T5 copy proof and the hook's queue.
                && source(row[0]).is_some_and(|source| unsafe { crate::structured_snapshot_receipt::cvr_sbsn_validate_submission(&source.full_t5_prepared, queue) });
            if !valid {
                if stats[11] == 0 {
                    stats[11] = 5;
                }
                fail(2705);
            } else {
                row[14] = 1;
                stats[8] += 1;
            }
        }
    }
    validate_material_buffers(w, side, queue);
    value.submission_certified = !w.failed;
    for row in rows_mut::<16>(w, NATIVE_TABLE_ROWS) {
        if row[1] == s && row[2] != 0 {
            row[15] = u64::from(rust_side().native_tables.get(&(row[0] as usize)).is_some_and(|tables| crate::native_descriptor_arena::validate_submission(tables, queue)));
            if row[15] == 0 {
                fail(2312);
                value.submission_certified = false;
            }
        }
    }
    if value.submission_certified {
        // SAFETY: the hook's live queue; a new reference.
        value.source_queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned();
    }
    let predicate = eye_rust(side).instance_predicate.as_ref().map_or(null_mut(), |p| p.as_raw());
    if !predicate.is_null() {
        let gpu = rows_mut::<24>(w, INSTANCE_GPU_ROWS);
        let instances = rows_mut::<48>(w, INSTANCE_ROWS);
        // SAFETY: the eye's predicate buffer and the journal's rows.
        let permits = unsafe {
            crate::retained_permits::cvr_nrg_instance_permits(predicate, gpu.as_mut_ptr(), gpu.len(), instances.as_ptr(), instances.len(), s, w.failed)
        };
        if !permits {
            fail(1803);
            value.submission_certified = false;
        }
    }
}

// ---- the CBV snapshots

/// families::ResourceUse.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceUse {
    pub kind: u32,
    pub visibility: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
    pub bytes: u32,
}
/// source_cbv_snapshot::detail::MaxRequests.
const MAX_CBV_REQUESTS: usize = 28;
const CBV_COPY_OWNERS: u32 = 35;
const TABLE_COMMANDS: u32 = 32;
/// The eye's CBV arena (its Rust storage) and its readback.
fn eye_rust(side: u32) -> &'static mut EyeRust {
    &mut rust_side().eyes[side as usize]
}
fn cbv_arena_reset(side: u32) {
    eye_rust(side).cbv_arena = None;
}
fn cbv_arena_finalized(side: u32) -> bool {
    // SAFETY: the arena's live storage.
    eye_rust(side).cbv_arena.as_ref().is_some_and(|a| unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_arena_finalized(a.as_raw()) })
}
/// QueryResourceCreation's kind (the creation receipt; its heap reference released at once).
fn creation_kind(resource: *mut c_void) -> Option<u32> {
    let (mut kind, mut known, mut state, mut heap, mut offset, mut bytes) = (0u32, false, 0u32, null_mut(), 0u64, 0u64);
    // SAFETY: a live resource; writable outputs.
    let found = unsafe { crate::creation_receipts::cvr_crc_query(resource, &mut kind, &mut known, &mut state, &mut heap, &mut offset, &mut bytes) };
    if !heap.is_null() {
        // SAFETY: the receipt's heap reference, handed over.
        drop(unsafe { IUnknown::from_raw(heap) });
    }
    found.then_some(kind)
}
fn cbv_reject(side: u32, command: &mut [u64; 24], rows: &mut [[u64; 32]], code: u32) {
    command[2] = 3;
    command[3] = u64::from(code);
    let w = work().expect("capturing");
    w.cbv_snapshots_failed = true;
    w.eye(side as usize).cbv_arena_aborted = true;
    cbv_arena_reset(side);
    fail(950 + code);
    for row in rows.iter_mut() {
        if row[8] == 0 {
            row[8] = 3;
            row[9] = u64::from(code);
        }
    }
}
/// CaptureCbvSnapshots: the source's CBVs copied at the draw into the eye's arena. Err: the C++ throw ("Private CBV
/// source row is missing"), left to AppendSource's handler.
fn capture_cbv_snapshots(list: *mut c_void, side: u32, source: &mut SourceScalars, source_row: &[u64; 48]) -> Result<(), ()> {
    let w = work().expect("capturing");
    let eye = w.eye(side as usize);
    let mut command = [0u64; 24];
    // SAFETY: a writable command row and the source row.
    unsafe {
        crate::retained_source_rows::cvr_nrg_cbv_command_head(&mut command, source_row, u64::from(side), list as u64, eye.recording.generation, eye.recording.context)
    };
    let mut rows: Vec<[u64; 32]> = Vec::new();
    let mut requests: Vec<crate::source_cbv_snapshot_receipt::Request> = Vec::new();
    let reject = |command: &mut [u64; 24], rows: &mut Vec<[u64; 32]>, code: u32| cbv_reject(side, command, rows, code);
    // families::Lookup: the family's root is the source's, the source has its shader (the family held for its uses).
    let held = cbv_family(source, source_row[10] as *mut c_void);
    let family = held.is_some();
    let (uses, use_count): (*const ResourceUse, usize) = held.as_ref().map_or((core::ptr::null(), 0), |f| (f.uses.as_ptr().cast(), f.uses.len()));
    let tables = rows_mut::<24>(w, TABLE_COMMANDS);
    if eye.cbv_arena_aborted
        || w.mode != 1
        || source_row[44] != 0
        || !family
        || w.cbv_failed
        || w.tables_failed
        || !w.source_tables
        || tables.last().is_none_or(|last| last[0] != source_row[0] || last[2] == 0)
    {
        reject(&mut command, &mut rows, 1);
    } else {
        // SAFETY: the family's uses, `use_count` of them.
        let uses = if uses.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(uses, use_count) } };
        rows.reserve(uses.len());
        requests.reserve(MAX_CBV_REQUESTS);
        let mut current = true;
        let cbvs = source.cbvs.as_slice();
        for u in uses.iter().filter(|u| u.kind == 2) {
            let mut row = [0u64; 32];
            row[0] = source_row[0];
            row[1] = u64::from(side);
            row[2] = u64::from(u.visibility);
            row[3] = u64::from(u.shader_register);
            row[6] = u64::from(u.bytes);
            command[6] += 1;
            let stage = match u.visibility {
                1 => 0u32,
                5 => 1,
                _ => u32::MAX,
            };
            let found = cbvs.iter().find(|cbv| cbv.stage == stage && cbv.shader_register == u.shader_register);
            let (mut parameter, mut offset) = (0u32, 0u32);
            // SAFETY: writable outputs.
            let bound = u.count == 1
                && found.is_some()
                && unsafe { crate::descriptor_audit::cvr_sda_audit_find_binding(source_row[11], 2, u.space, u.shader_register, u.visibility, &mut parameter, &mut offset) };
            let Some(cbv) = found.filter(|_| bound) else {
                row[8] = 3;
                row[9] = 2;
                current = false;
                rows.push(row);
                continue;
            };
            row[4] = u64::from(parameter);
            row[5] = u64::from(offset);
            row[7] = u64::from(cbv.descriptor_bytes);
            row[12] = cbv.resource;
            row[13] = cbv.stamp.value[4];
            row[14] = cbv.source_offset;
            row[15] = u64::from(cbv.native.kind);
            row[16] = u64::from(cbv.native.handle);
            row[23] = cbv.stamp.gpu;
            row[24] = cbv.stamp.heap;
            row[25] = cbv.recording.generation;
            row[26] = cbv.recording.context;
            row[27] = cbv.cookie;
            row[28] = cbv.origin;
            let resource = cbv.resource as *mut c_void;
            let mut heap = D3D12_HEAP_PROPERTIES::default();
            let mut flags = D3D12_HEAP_FLAGS::default();
            // SAFETY: the backing's live resource.
            let heap_known = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }
                .is_some_and(|r| unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok());
            if !heap_known {
                row[8] = 3;
                row[9] = 3;
                current = false;
                rows.push(row);
                continue;
            }
            let creation = creation_kind(resource);
            row[10] = heap.Type.0 as u32 as u64;
            row[11] = u64::from(creation.unwrap_or(0));
            let original_epoch = crate::source_cbv_snapshot::ReadEpochCertificate { list: list as u64, generation: eye.recording.generation, occurrence: command[14], certified: true };
            // SAFETY: the backing (SourceCbvBacking) and the certificate.
            let inline_upload = heap.Type == D3D12_HEAP_TYPE_UPLOAD
                && crate::declared_moved_exports::CyberpunkVR_SourceCbvSnapshotsUploadEnable.load(Relaxed) != 0
                && crate::source_cbv_snapshot_receipt::cvr_scsn_upload_compatible()
                && unsafe { crate::source_cbv_snapshot::cvr_scs_inline_upload(2, cbv, &original_epoch, u.bytes) };
            if (heap.Type == D3D12_HEAP_TYPE_UPLOAD || heap.Type == D3D12_HEAP_TYPE_GPU_UPLOAD) && !inline_upload {
                row[8] = 2;
                command[5] += 1;
                rows.push(row);
                continue;
            }
            // SAFETY: a plain detail, written whole by the capture.
            let mut detail: crate::source_cbv_snapshot_receipt::Detail = unsafe { core::mem::zeroed() };
            let observed = creation.is_some()
                && (inline_upload
                    || (heap.Type == D3D12_HEAP_TYPE_DEFAULT
                        && heap.CPUPageProperty != D3D12_CPU_PAGE_PROPERTY_WRITE_BACK
                        && heap.CPUPageProperty != D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE))
                // SAFETY: the backing (Backing's layout) and a writable detail.
                && unsafe {
                    crate::source_cbv_snapshot_receipt::cvr_scsn_capture_at_source(list, source_row[11], parameter, offset, (cbv as *const crate::source_cbv_snapshot::SourceCbvBacking).cast(), &mut detail)
                };
            let state = detail.state;
            // SAFETY: the detail's read state.
            let read_mask = unsafe { crate::source_cbv_snapshot::cvr_scs_read_mask((&state as *const crate::source_cbv_details::ReadState).cast(), false) };
            let generic_read = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
            let read_only = observed
                && detail.context.observation == crate::source_cbv_details::KNOWN
                && detail.context.pending == 0
                && detail.context.submitted == 0
                && detail.state_queried
                && detail.state_value_known
                && ((inline_upload && state.status == 1 && state.value == generic_read)
                    || (cbv.native.kind == 1 && state.kind == 1 && (state.status == 8 || state.status == 9) && state.value == generic_read)
                    || read_mask);
            let context = crate::native_recording::Context { context: cbv.recording.context, state: detail.context.state, list: cbv.recording.list };
            // SAFETY: the backing's lease (native_recording::Lease's layout) and the hook's list.
            let recorded = read_only
                && cbv.recording.list == list as u64
                && cbv.recording.generation == eye.recording.generation
                && unsafe { crate::native_recording::cvr_native_recording_validate_recording((&cbv.recording as *const crate::source_cbv_snapshot::RecordingLease).cast(), list, &context) };
            if !recorded {
                row[8] = 3;
                row[9] = 4;
                current = false;
                rows.push(row);
                continue;
            }
            // SAFETY: a plain request; the backing copied word for word (its references stay the source's).
            let mut request: crate::source_cbv_snapshot_receipt::Request = unsafe { core::mem::zeroed() };
            // SAFETY: both 472-byte backings (asserted by their modules).
            unsafe { core::ptr::copy_nonoverlapping((cbv as *const crate::source_cbv_snapshot::SourceCbvBacking).cast::<u8>(), (&mut request.source as *mut crate::source_cbv_observe::Backing).cast::<u8>(), 472) };
            request.parameter = parameter;
            request.offset = offset;
            request.reflected_bytes = u.bytes;
            requests.push(request);
            command[4] += 1;
            rows.push(row);
        }
        if !current {
            reject(&mut command, &mut rows, 5);
        } else if requests.is_empty() {
            command[2] = 2;
        } else if !crate::preparation_trace::revalidate_graph_parent(&source.parent) {
            reject(&mut command, &mut rows, 6);
        } else {
            if eye_rust(side).cbv_arena.is_none() {
                let mut made = null_mut();
                let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
                // SAFETY: a live device and list; the arena's storage handed over (one reference).
                let hr = unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_arena_create(null_mut(), device, list, eye.recording.generation, 8 * 1024 * 1024, &mut made) };
                if hr.is_ok() && !made.is_null() {
                    // SAFETY: as above.
                    eye_rust(side).cbv_arena = Some(unsafe { IUnknown::from_raw(made) });
                } else {
                    reject(&mut command, &mut rows, 7);
                }
            }
            if command[3] == 0 {
                let mut stamp = crate::indirect_copy::ScopeStamp::default();
                // SAFETY: the scope's stamp, exited below.
                unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_scope_enter(list as u64, eye.recording.generation, command[14], &mut stamp) };
                let epoch = crate::source_cbv_snapshot::ReadEpochCertificate { list: list as u64, generation: eye.recording.generation, occurrence: command[14], certified: true };
                let arena = eye_rust(side).cbv_arena.as_ref().map_or(null_mut(), |a| a.as_raw());
                // SAFETY: the scope's stamp; the requests, parent, epoch, arena and the source's proof.
                let prepared = unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_scope_active(&stamp) }
                    && unsafe {
                        crate::source_cbv_snapshot_receipt::cvr_scsn_prepare(
                            list,
                            source_row[11],
                            requests.as_ptr(),
                            requests.len(),
                            (&source.parent as *const GraphParentRecord).cast(),
                            &epoch,
                            arena,
                            &mut source.cbv_prepared,
                        )
                    };
                let mut outcome = Ok(());
                if !prepared {
                    reject(&mut command, &mut rows, 100 + crate::source_cbv_snapshot_receipt::cvr_scsn_last_failure());
                } else {
                    command[15] = 1;
                    command[19] = 1;
                    let mut views = [crate::source_cbv_snapshot::PrivateCbv::default(); MAX_CBV_REQUESTS];
                    let mut count = 0u32;
                    // SAFETY: the proof and room for every view.
                    let recorded = unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_record(list, &source.cbv_prepared, source.private_cbvs.as_slice().is_empty(), views.as_mut_ptr(), &mut count) };
                    if !recorded {
                        reject(&mut command, &mut rows, 100 + crate::source_cbv_snapshot_receipt::cvr_scsn_last_failure());
                    } else {
                        // The views' references move into the source's vector.
                        source.private_cbvs = views[..(count as usize).min(MAX_CBV_REQUESTS)].to_vec();
                        command[2] = 1;
                        command[16] = 1;
                        let captured = source.private_cbvs.as_slice();
                        command[7] = captured.len() as u64;
                        for value in captured {
                            let Some(row) = rows.iter_mut().find(|row| row[4] == u64::from(value.parameter) && row[5] == u64::from(value.offset) && row[8] == 0) else {
                                outcome = Err(()); // "Private CBV source row is missing"
                                break;
                            };
                            row[8] = 1;
                            row[17] = value.resource;
                            row[18] = value.view_location;
                            row[19] = value.private_offset;
                            row[20] = value.copied_bytes;
                            row[21] = u64::from(value.padded_bytes);
                            row[22] = value.owner;
                            command[8] += value.copied_bytes;
                            command[9] += u64::from(value.padded_bytes);
                            command[10] = row[17];
                        }
                    }
                }
                // SAFETY: the stamp entered above.
                unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_scope_exit(&stamp) };
                outcome?;
            }
        }
    }
    for row in &rows {
        // SAFETY: the work's journal; whole rows of 32.
        unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_SNAPSHOT_ROWS, row.as_ptr(), 1) };
    }
    let owner = if command[2] == 1 { source.cbv_prepared.owner } else { 0 };
    let owners = [command[0], command[1], command[11], command[12], command[14], owner, command[7], command[8]];
    // SAFETY: as above.
    unsafe {
        crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_SNAPSHOT_COMMANDS, command.as_ptr(), 1);
        crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_COPY_OWNERS, owners.as_ptr(), 1);
    }
    Ok(())
}
/// Seal's CBV stage: the eye's arena finalized, its rows marked, the private CBVs read back.
fn seal_cbv(w: &mut WorkScalars, list: *mut c_void, side: u32) {
    let value = w.eye(side as usize);
    let Some(arena) = eye_rust(side).cbv_arena.as_ref().map(|a| a.as_raw()) else { return };
    if !w.source_cbv_snapshots || value.cbv_arena_aborted {
        return;
    }
    // SAFETY: the arena's live storage and the sealed list.
    if !unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_arena_finalize(arena, list) } {
        w.cbv_snapshots_failed = true;
        value.cbv_arena_aborted = true;
        cbv_arena_reset(side);
        fail(1100 + crate::source_cbv_snapshot_receipt::cvr_scsn_last_failure());
        return;
    }
    let s = u64::from(side);
    for row in rows_mut::<24>(w, CBV_SNAPSHOT_COMMANDS) {
        if row[1] == s && row[2] == 1 {
            row[17] = 1;
        }
    }
    for row in rows_mut::<32>(w, CBV_SNAPSHOT_ROWS) {
        if row[1] == s && row[8] == 1 {
            row[29] = 1;
        }
    }
    let sources = rows_mut::<48>(w, SOURCE_ROWS);
    let (mut resource, mut bytes) = (0u64, 0u64);
    for index in 0..source_count() {
        let Some(captured) = source(index as u64) else { continue };
        if sources.get(index).is_none_or(|row| row[1] != s) {
            continue;
        }
        for cbv in captured.private_cbvs.as_slice() {
            resource = cbv.resource;
            bytes = bytes.max(cbv.private_offset + u64::from(cbv.padded_bytes));
        }
    }
    if resource != 0 && bytes != 0 {
        let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
        let mut readback = null_mut();
        // SAFETY: live device, list and resource; the readback handed over (one reference).
        let made = unsafe { crate::retained_readbacks::cvr_nrg_readback(device, list, resource as *mut c_void, bytes, true, &mut readback) };
        // SAFETY: as above.
        eye_rust(side).cbv_readback = (!readback.is_null()).then(|| unsafe { ID3D12Resource::from_raw(readback) });
        if !made {
            w.cbv_snapshots_failed = true;
            fail(1120);
        }
    }
}

// ---- the T5 snapshots

/// structured_buffer_snapshot::Failure.
const STRUCTURED_SOURCE: u32 = 6;
const STRUCTURED_CAPACITY: u32 = 11;
const STRUCTURED_RECOVERY_FATAL: u32 = 16;
/// structured_buffer_snapshot::detail::SourceBytes.
const FULT_T5_BYTES: u64 = 1024 * 1024;
const T5_COVERAGE: u32 = 52;
const T5_PIECES: u32 = 51;
const T5_INTENT_BLOB: u32 = 6;
/// SourceAssemblyCommandLimit.
const SOURCE_ASSEMBLY_COMMAND_LIMIT: usize = 2048;
/// The source's T5 read plan (multilayer_selectors::ReadPlan) as the snapshot reads it.
#[repr(C)]
#[derive(Default)]
pub struct T5Plan {
    pub resource_offset: u64,
    pub byte_extent: u64,
    pub coverage_complete: bool,
}
/// The source's T5 intent candidate (source_resource_intent::Candidate) as the snapshot reads it.
#[repr(C)]
pub struct T5Candidate {
    pub present: bool,
    pub composed: bool,
    pub copy_sequence: u64,
    pub bytes: *const u8,
    pub byte_count: usize,
    pub pieces: *const [u64; 9],
    pub piece_count: usize,
}
/// The eye's structured arena: None until a creation was attempted (the C++ unique_ptr), then its storage or null.
fn structured_slot(side: u32, full: bool) -> &'static mut Option<Option<IUnknown>> {
    let eye = eye_rust(side);
    if full { &mut eye.full_t5_arena } else { &mut eye.t5_arena }
}
fn structured_present(side: u32, full: bool) -> bool {
    structured_slot(side, full).is_some()
}
fn structured_arena(side: u32, full: bool) -> *mut c_void {
    structured_slot(side, full).as_ref().and_then(|a| a.as_ref()).map_or(null_mut(), |a| a.as_raw())
}
fn structured_abort(side: u32, full: bool) {
    let arena = structured_arena(side, full);
    if !arena.is_null() {
        // SAFETY: the arena's live storage.
        unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_abort(arena) };
    }
}
fn structured_finalized(side: u32, full: bool) -> bool {
    let arena = structured_arena(side, full);
    // SAFETY: as above.
    !arena.is_null() && unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_finalized(arena) }
}
fn structured_bytes(side: u32, full: bool) -> u64 {
    let arena = structured_arena(side, full);
    // SAFETY: as above.
    if arena.is_null() { 0 } else { unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_bytes(arena) } }
}
/// A new OriginalIndices arena of 8 MiB for the eye; false when it could not be created.
fn structured_create(w: &WorkScalars, list: *mut c_void, side: u32, full: bool) -> bool {
    let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
    let mut made = null_mut();
    // SAFETY: a live device and list; the storage handed over (one reference).
    let hr = unsafe {
        crate::structured_snapshot_receipt::cvr_sbsn_arena_create(null_mut(), device, list, w.eye(side as usize).recording.generation, 8 * 1024 * 1024, 1, &mut made)
    };
    let created = hr.is_ok() && !made.is_null();
    // SAFETY: as above.
    *structured_slot(side, full) = Some(created.then(|| unsafe { IUnknown::from_raw(made) }));
    created
}
/// FullT5Readback: a readback buffer of `bytes` (at most 128 MiB) into `output` (replaced when attempted).
fn full_readback(w: &WorkScalars, list: *mut c_void, bytes: u64, output: &mut Option<ID3D12Resource>) -> bool {
    if bytes == 0 || bytes > 128 * 1024 * 1024 {
        return false;
    }
    let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
    let mut out = null_mut();
    // SAFETY: a live device and list; the readback handed over.
    let made = unsafe { crate::retained_readbacks::cvr_nrg_readback(device, list, null_mut(), bytes, false, &mut out) };
    // SAFETY: as above.
    *output = (!out.is_null()).then(|| unsafe { ID3D12Resource::from_raw(out) });
    made
}
/// The structured copy at the actual call (prepare, record and, for the full view, validate) under its scope.
#[allow(clippy::too_many_arguments)]
fn structured_copy(
    list: *mut c_void,
    side: u32,
    full: bool,
    occurrence: u64,
    epoch_list: u64,
    root: u64,
    request: &crate::structured_snapshot_receipt::Request,
    parent: &GraphParentRecord,
    proof: &mut crate::structured_snapshot_receipt::PreparedCopy,
    range: &mut crate::structured_snapshot_receipt::PrivateRange,
) -> (bool, bool) {
    let generation = work().expect("capturing").eye(side as usize).recording.generation;
    let mut stamp = crate::indirect_copy::ScopeStamp::default();
    // SAFETY: the scope's stamp, exited below.
    unsafe { crate::structured_snapshot_receipt::cvr_sbsn_scope_enter(list as u64, generation, occurrence, &mut stamp) };
    let epoch = crate::structured_snapshot::ReadEpoch { list: epoch_list, generation, occurrence, certified: true };
    let arena = structured_arena(side, full);
    // SAFETY: the stamp, the request, parent, epoch, arena and the source's proof.
    let prepared = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_scope_active(&stamp) }
        && unsafe { crate::structured_snapshot_receipt::cvr_sbsn_prepare(list, root, request, (parent as *const GraphParentRecord).cast(), &epoch, arena, proof) };
    // SAFETY: the prepared proof and the source's range.
    let recorded = prepared && unsafe { crate::structured_snapshot_receipt::cvr_sbsn_record(list, proof, range) };
    // SAFETY: as above.
    let valid = recorded && (!full || unsafe { crate::structured_snapshot_receipt::cvr_sbsn_validate(list, proof) });
    // SAFETY: the stamp entered above.
    unsafe { crate::structured_snapshot_receipt::cvr_sbsn_scope_exit(&stamp) };
    (prepared, if full { valid } else { recorded })
}
fn structured_request(source: &SourceScalars, offset: u64, bytes: u32, extent: u32) -> crate::structured_snapshot_receipt::Request {
    // SAFETY: a plain request; the backing copied word for word (its references stay the source's).
    let mut request: crate::structured_snapshot_receipt::Request = unsafe { core::mem::zeroed() };
    // SAFETY: both are the 1112-byte SourceSrvBacking.
    unsafe { core::ptr::copy_nonoverlapping((&source.t5_native as *const crate::source_srv_observe::Backing).cast::<u8>(), (&mut request.source as *mut crate::source_srv_observe::Backing).cast::<u8>(), 1112) };
    request.parameter = 23;
    request.offset = 5;
    request.source_offset = offset;
    request.bytes = bytes;
    request.extent = extent;
    request
}
/// CaptureT5Snapshot: the multilayer T5's selected range copied at the draw.
fn capture_t5_snapshot(list: *mut c_void, side: u32, source: &mut SourceScalars, source_row: &[u64; 48]) {
    let w = work().expect("capturing");
    if !w.source_selectors {
        return;
    }
    let Some(command) = rows_mut::<32>(w, SELECTOR_COMMANDS).last_mut() else { return };
    if command[25] == 0 {
        return;
    }
    let eye = w.eye(side as usize);
    let binding = &source.multilayer_t5;
    let mut row = [0u64; 32];
    // SAFETY: a writable row and the source row.
    unsafe {
        crate::retained_source_rows::cvr_nrg_t5_head(&mut row, source_row, u64::from(side), binding.resource, binding.stamp.gpu, binding.stamp.heap, eye.recording.context, eye.recording.generation)
    };
    let native = &source.t5_native;
    row[6] = u64::from(native.native.handle);
    row[7] = u64::from(native.native.kind);
    row[8] = u64::from(native.state.status);
    row[9] = u64::from(native.state.value);
    let resource = binding.resource as *mut c_void;
    // SAFETY: the binding's live resource.
    if let Some(r) = unsafe { ID3D12Resource::from_raw_borrowed(&resource) } {
        // SAFETY: as above.
        row[5] = unsafe { r.GetGPUVirtualAddress() };
    }
    command[29] = u64::from(!native.recording.token.is_null() && native.native.resource == row[4]);
    let plan = sources::t5_plan_of(source_row[0] as usize);
    row[10] = plan.resource_offset;
    row[11] = plan.byte_extent;
    let mut coverage = [0u64; 20];
    coverage[0] = source_row[0];
    coverage[1] = u64::from(side);
    if !resource.is_null() && plan.coverage_complete && plan.byte_extent != 0 {
        let mut diagnostic = [0u64; 16];
        // SAFETY: a live resource; a 16-word diagnostic.
        coverage[2] = u64::from(unsafe { crate::source_intent::cvr_sri_query_coverage(2, resource, plan.resource_offset, plan.byte_extent as usize, diagnostic.as_mut_ptr().cast()) });
        coverage[4..20].copy_from_slice(&diagnostic);
    }
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    if journal.count(T5_COVERAGE) < SOURCE_ASSEMBLY_COMMAND_LIMIT {
        journal.append(T5_COVERAGE, &coverage);
    }
    source.t5_row = journal.count(T5_SNAPSHOT_ROWS);
    command[28] = source.t5_row as u64 + 1;
    let candidate = sources::t5_candidate_of(source_row[0] as usize);
    if candidate.present {
        row[30] = u64::from(candidate.composed);
        row[25] = if candidate.composed { source_row[0] + 1 } else { candidate.copy_sequence };
        row[26] = journal.blob(T5_INTENT_BLOB).len() as u64 + 1;
        if candidate.byte_count != 0 {
            // SAFETY: the candidate's bytes.
            journal.append_blob(T5_INTENT_BLOB, unsafe { core::slice::from_raw_parts(candidate.bytes, candidate.byte_count) });
        }
    }
    if candidate.piece_count != 0 {
        // SAFETY: the candidate's piece receipts.
        for piece in unsafe { core::slice::from_raw_parts(candidate.pieces, candidate.piece_count) } {
            let mut value = [0u64; 16];
            value[0] = source_row[0];
            value[1] = u64::from(side);
            value[2] = journal.count(T5_PIECES) as u64;
            value[3] = row[25];
            value[4..13].copy_from_slice(piece);
            journal.append(T5_PIECES, &value);
        }
    }
    let planned = command[2] == 1 || (command[2] == 3 && command[3] == 2 && plan.byte_extent != 0);
    if !planned || eye.t5_aborted || source_row[44] != 0 || command[29] == 0 {
        row[2] = 3;
        row[3] = u64::from(STRUCTURED_SOURCE);
    } else if row[11] == 0 {
        row[2] = 2;
    } else {
        let reject = |row: &mut [u64; 32], error: u32, fatal: bool| {
            row[2] = 3;
            row[3] = u64::from(error);
            if fatal {
                work().expect("capturing").eye(side as usize).t5_aborted = true;
                structured_abort(side, false);
                fail(1350 + error);
            }
        };
        if !structured_present(side, false) && !structured_create(w, list, side, false) {
            reject(&mut row, STRUCTURED_CAPACITY, false);
        }
        if row[3] == 0 {
            let request = structured_request(source, row[10], row[11] as u32, 0);
            let parent = source.parent;
            let (prepared, recorded) =
                structured_copy(list, side, false, row[20], source_row[6], source_row[11], &request, &parent, &mut source.t5_prepared, &mut source.t5_private);
            if !prepared {
                reject(&mut row, crate::structured_snapshot_receipt::cvr_sbsn_last_failure(), false);
            } else {
                row[21] = 1;
                row[29] = 1;
                if !recorded {
                    let failure = crate::structured_snapshot_receipt::cvr_sbsn_last_failure();
                    eye.t5_aborted = true;
                    structured_abort(side, false);
                    reject(&mut row, failure, failure == STRUCTURED_RECOVERY_FATAL);
                } else {
                    row[2] = 1;
                    row[22] = 1;
                    row[16] = source.t5_private.resource as u64;
                    row[17] = source.t5_private.private_offset;
                    row[18] = source.t5_private.copied_bytes;
                    row[19] = source.t5_private.owner as u64;
                    command[26] = 1;
                }
            }
        }
    }
    journal.append(T5_SNAPSHOT_ROWS, &row);
}
/// CaptureFullT5: the stage-9 draw's whole original T5 view copied at the draw and its original bytes read back.
fn capture_full_t5(list: *mut c_void, side: u32, source: &mut SourceScalars, saved: &[u64; 48]) {
    let w = work().expect("capturing");
    if !w.full_t5_enabled || saved[4] != 9 || w.failed {
        return;
    }
    let eye = w.eye(side as usize);
    let stats = ex!(CyberpunkVR_RetainedFullT5Stats);
    let reject = |why: u32| {
        let stats = ex!(CyberpunkVR_RetainedFullT5Stats);
        if stats[11] == 0 {
            stats[11] = u64::from(why);
        }
        structured_abort(side, true);
        fail(2700 + why);
    };
    if !source.t5_original_certified
        || source.t5_native.recording.token.is_null()
        || saved[44] != 0
        || saved[2] != 0
        || eye.full_t5_reference_bytes > 128 * 1024 * 1024 - FULT_T5_BYTES
    {
        reject(1);
        return;
    }
    if !structured_present(side, true) {
        let created = structured_create(w, list, side, true);
        if !created || !full_readback(w, list, 128 * 1024 * 1024, &mut eye_rust(side).full_t5_original_readback) {
            reject(2);
            return;
        }
    }
    let request = structured_request(source, 0, FULT_T5_BYTES as u32, 1);
    let parent = source.parent;
    let (_, valid) = structured_copy(list, side, true, saved[0] + 1, saved[6], saved[11], &request, &parent, &mut source.full_t5_prepared, &mut source.full_t5_private);
    if !valid {
        reject(100 + crate::structured_snapshot_receipt::cvr_sbsn_last_failure());
        return;
    }
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: the source's proof and range; a writable view.
    if !unsafe { crate::structured_snapshot_receipt::cvr_sbsn_recorded_view(&source.full_t5_prepared, &source.full_t5_private, &mut view) } {
        reject(3);
        return;
    }
    let original = eye_rust(side).full_t5_original_readback.clone();
    let t5 = source.multilayer_t5.resource as *mut c_void;
    if let (Some(l), Some(readback)) = (list_of(list), original.as_ref()) {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: live list, readback and T5 resource.
        if let Some(t5) = unsafe { ID3D12Resource::from_raw_borrowed(&t5) } {
            // SAFETY: as above.
            unsafe { l.CopyBufferRegion(readback, eye.full_t5_reference_bytes, t5, 0, FULT_T5_BYTES) };
        }
    }
    let proof = &source.full_t5_prepared;
    let copy = &source.full_t5_private;
    // SAFETY: the view's buffer arm (a buffer SRV).
    let first = unsafe { view.Anonymous.Buffer.FirstElement };
    let row = [
        saved[0],
        u64::from(side),
        u64::from(w.frame),
        saved[6],
        eye.recording.generation,
        saved[0] + 1,
        t5 as u64,
        copy.resource as u64,
        copy.private_offset,
        copy.copied_bytes,
        u64::from(proof.reused),
        proof.gpu_copied_bytes,
        eye.full_t5_reference_bytes,
        first,
        0,
        u64::MAX,
    ];
    // SAFETY: the work's journal; one row of 16.
    unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), FULL_T5_ROWS, row.as_ptr(), 1) };
    eye.full_t5_reference_bytes += FULT_T5_BYTES;
    stats[2] = u64::from(w.frame);
    stats[3] += 1;
    stats[if proof.reused { 5 } else { 4 }] += 1;
    stats[6] += proof.gpu_copied_bytes;
    stats[12 + side as usize] = structured_bytes(side, true);
    stats[14] += FULT_T5_BYTES;
}
/// Seal's T5 and full-T5 stages.
fn seal_t5(w: &mut WorkScalars, list: *mut c_void, side: u32) {
    let value = w.eye(side as usize);
    let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
    let arena = structured_arena(side, false);
    if w.source_selectors && structured_present(side, false) && structured_bytes(side, false) != 0 && !value.t5_aborted {
        // SAFETY: the arena's live storage and the sealed list.
        if !unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_finalize(arena, list) } {
            value.t5_aborted = true;
            structured_abort(side, false);
            fail(1400 + crate::structured_snapshot_receipt::cvr_sbsn_last_failure());
        } else {
            // SAFETY: the arena's resource (a new reference, released when dropped).
            let resource = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_resource(arena) };
            // SAFETY: as above.
            let held = (!resource.is_null()).then(|| unsafe { IUnknown::from_raw(resource) });
            let mut out = null_mut();
            // SAFETY: live device, list and resource; the readback handed over.
            let made = unsafe { crate::retained_readbacks::cvr_nrg_readback(device, list, resource, structured_bytes(side, false), true, &mut out) };
            drop(held);
            // SAFETY: as above.
            eye_rust(side).t5_readback = (!out.is_null()).then(|| unsafe { ID3D12Resource::from_raw(out) });
            if !made {
                value.t5_aborted = true;
                fail(1450);
            } else {
                for row in rows_mut::<32>(w, T5_SNAPSHOT_ROWS) {
                    if row[1] == u64::from(side) && row[2] == 1 {
                        row[23] = 1;
                    }
                }
            }
        }
    }
    let full = structured_arena(side, true);
    if w.full_t5_enabled && structured_present(side, true) {
        let stats = ex!(CyberpunkVR_RetainedFullT5Stats);
        // SAFETY: the arena's storage (or null) and the sealed list.
        let finalized = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_finalize(full, list) };
        if !finalized || !full_readback(w, list, structured_bytes(side, true), &mut eye_rust(side).full_t5_private_readback) {
            if stats[11] == 0 {
                stats[11] = 4;
            }
            structured_abort(side, true);
            fail(2704);
        } else {
            // SAFETY: the arena's resource (a new reference).
            let resource = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_arena_resource(full) };
            // SAFETY: as above.
            let held = (!resource.is_null()).then(|| unsafe { ID3D12Resource::from_raw(resource) });
            if let (Some(l), Some(target), Some(source)) = (list_of(list), eye_rust(side).full_t5_private_readback.as_ref(), held.as_ref()) {
                let _internal = cvr_host::probe::Internal::new();
                // SAFETY: live list and buffers.
                unsafe { l.CopyBufferRegion(target, 0, source, 0, structured_bytes(side, true)) };
            }
            stats[7] |= 1u64 << side;
        }
    }
}

// ---- the publication, the details, the poll

/// The C++-held counters PublishSources reports: the compiled sources, the assembly bank and cache, the table cache,
/// the T5 overlay, each eye's instance capture (present, versions, copied, required).
#[repr(C)]
#[derive(Default)]
pub struct PublishCounts {
    pub source_count: u32,
    pub bank_present: bool,
    pub overlay_present: bool,
    pub bank: [u64; 2],
    pub overlay_bytes: [u64; 2],
    pub table_cache: [u64; 4],
    pub assembly_cache: [u64; 4],
    pub instance: [[u64; 4]; 2],
}
fn publish_sources_impl() {
    let Some(w) = work() else { return };
    if !w.source_stream {
        return;
    }
    let mut c = PublishCounts::default();
    sources::publish_counts(&mut c);
    c.source_count = rust_side().source_geometry.as_ref().map_or(0, |g| geometry_counts(g).1);
    for side in 0..2u32 {
        if let Some(capture) = &eye_rust(side).instance_capture {
            let count = |which| capture.as_ref().map_or(0, |a| crate::instance_snapshot_native::arena_count(a, which));
            c.instance[side as usize] = [1, count(0), count(2), count(1)];
        }
    }
    let (e0, e1) = (w.eye(0), w.eye(1));
    let mut p = crate::retained_journal::Publish {
        frame: w.frame,
        source_count: c.source_count,
        overlay_stage: w.overlay_stage,
        instance_overlay_stage: w.instance_overlay_stage,
        bank_result: w.assembly_bank_result,
        arenas_finalized: u32::from(cbv_arena_finalized(0)) + u32::from(cbv_arena_finalized(1)),
        source_failed: w.source_failed,
        source_compiled: w.source_compiled,
        sealed: [e0.sealed, e1.sealed],
        source_cbvs: w.source_cbvs,
        cbv_failed: w.cbv_failed,
        source_snapshots: w.source_snapshots,
        snapshot_failed: w.snapshot_failed,
        source_tables: w.source_tables,
        tables_failed: w.tables_failed,
        source_details: w.source_details,
        details_failed: w.details_failed,
        source_cbv_snapshots: w.source_cbv_snapshots,
        cbv_snapshots_failed: w.cbv_snapshots_failed,
        cbv_payload_complete: w.cbv_payload_complete,
        source_selectors: w.source_selectors,
        complete: w.complete,
        assembly_truncated: w.assembly_truncated,
        assembly_complete: w.assembly_complete,
        bank_attempted: w.assembly_bank_attempted,
        bank_present: c.bank_present,
        overlay_present: c.overlay_present,
        table_cache: c.table_cache,
        assembly_cache: c.assembly_cache,
        bank: c.bank,
        bank_failure: w.assembly_bank_failure,
        overlay_bytes: c.overlay_bytes,
        instance: Default::default(),
    };
    for (side, eye) in [(0usize, &*e0), (1, &*e1)] {
        let counts = c.instance[side];
        if counts[0] == 0 {
            continue;
        }
        p.instance[side] = crate::retained_journal::PublishEye {
            present: 1,
            versions: counts[1],
            copied: counts[2],
            required: counts[3],
            signaled: eye.signaled_value,
            // SAFETY: a live fence.
            completed: eye.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() }),
        };
    }
    // SAFETY: the work's journal; the values above.
    unsafe { crate::retained_journal::cvr_nrg_publish_sources(w.journal.cast(), &p) };
}
const CBV_ROWS: u32 = 27;
const DETAIL_ROWS: u32 = 30;
const CBV_PRODUCERS: u32 = 28;
const CBV_DIAGNOSTICS: u32 = 29;
/// AppendSource's CBV capture: for each of the family's CBVs (the pipeline's family whose source root is the source's
/// bound root) the binding in the draw's root, its descriptor and backing at the source (registered, transient or
/// created), the CBV row, its producer and diagnostic, and its detail row; the backings kept in the source. False: an
/// allocation failed (the C++ throw).
///
/// # Safety
/// A live list; `source` the appended Work::Source's scalars; `row` its source row; `draw` its draw record.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_source_cbvs(
    list: *mut c_void,
    side: u32,
    source: *mut SourceScalars,
    row: *const [u64; 48],
    draw: *const DrawRecord,
    root: *mut c_void,
) -> bool {
    let Some(w) = work() else { return true };
    if !w.source_cbvs {
        return true;
    }
    // SAFETY: as documented.
    let (source, row, d) = unsafe { (&mut *source, &*row, &*draw) };
    // families::Lookup: the pipeline's family whose source root is `root` (held for the loop, its uses with it).
    let Some(family) = crate::families_native::lookup_raw(d.pipeline as *mut c_void).filter(|f| f.source_root.as_raw() == root) else {
        w.cbv_failed = true;
        ex!(CyberpunkVR_RetainedSourceCbvStats)[11] = 7; // source_cbv::Status::MissingObservation
        return true;
    };
    // SAFETY: the same 24-byte families::ResourceUse record.
    let uses: &[ResourceUse] = unsafe { core::slice::from_raw_parts(family.uses.as_ptr().cast(), family.uses.len()) };
    for u in uses.iter().filter(|u| u.kind == 2) {
        let mut value = [0u64; 24];
        value[0] = row[0];
        value[1] = u64::from(side);
        value[2] = u64::from(u.visibility);
        value[3] = u64::from(u.shader_register);
        value[6] = u64::from(u.bytes);
        let mut producer = [0u64; 40];
        producer[..6].copy_from_slice(&value[..6]);
        let (mut parameter, mut offset) = (0u32, 0u32);
        // SAFETY: plain records; all zeroes their default.
        let mut stamp: crate::descriptor_audit::DescriptorStamp = unsafe { core::mem::zeroed() };
        let mut backing = [0u64; 59];
        let mut diagnostic = [0u64; 16];
        let mut status = 6u32; // InvalidBinding
        // SAFETY: writable outputs.
        if u.count == 1
            && unsafe { crate::descriptor_audit::cvr_sda_audit_find_binding(d.root, 2, u.space, u.shader_register, u.visibility, &mut parameter, &mut offset) }
        {
            value[4] = u64::from(parameter);
            value[5] = u64::from(offset);
            producer[4] = u64::from(parameter);
            producer[5] = u64::from(offset);
            let mut owner: *mut c_void = null_mut();
            // SAFETY: the live list; writable outputs (descriptors::CaptureCbvSource: the origin's reference ours).
            if unsafe { crate::descriptor_audit::cvr_sda_audit_stamp(2, list as u64, d.root, parameter, offset, &mut stamp, &mut owner) } {
                // SAFETY: the reference the capture handed over, released after the read.
                let origin = unsafe { (!owner.is_null()).then(|| IUnknown::from_raw(owner)) };
                // SAFETY: the list, the stamp and a zeroed backing (source_cbv::ReadAtSource).
                status = unsafe {
                    crate::source_cbv_observe::cvr_scbo_read_at_source(
                        list as u64,
                        d.root,
                        parameter,
                        offset,
                        (&raw const stamp).cast(),
                        owner,
                        backing.as_mut_ptr().cast(),
                    )
                };
                // SAFETY: a writable diagnostic.
                unsafe { crate::source_cbv_observe::cvr_scbo_last_diagnostic(&mut diagnostic) };
                drop(origin);
            }
        }
        value[7] = u64::from(status);
        value[8] = stamp.gpu;
        value[9] = stamp.heap;
        value[10] = stamp.value[4];
        value[11] = stamp.value[5];
        // SAFETY: the backing as source_cbv::SourceCbvBacking (472 bytes, its COM pointers as addresses).
        let b = unsafe { *backing.as_ptr().cast::<crate::source_cbv_snapshot::SourceCbvBacking>() };
        let keep = b.resource != 0 && (1..=3).contains(&status);
        if keep {
            value[12] = b.resource;
            value[13] = b.gpu;
            value[14] = b.backing_bytes;
            value[15] = b.recording.context;
            value[16] = b.recording.generation;
            value[17] = b.cookie;
            value[18] = b.origin;
            value[19] = u64::from(b.creator_rva);
            value[20] = b.cpu_descriptor;
            value[21] = u64::from(b.native.handle);
            value[22] = u64::from(b.logical_bytes);
            value[23] = 1;
            producer[6] = u64::from(b.logical_bytes);
            producer[7] = u64::from(b.reserved_bytes);
            producer[8] = u64::from(b.completed_producer_rva);
            producer[9] = u64::from(b.creator_rva);
            producer[10] = b.resource;
            producer[11] = b.stamp.value[4];
            let put = |producer: &mut [u64; 40], at: usize, l: &crate::source_cbv_snapshot::RecordingLease| {
                producer[at..at + 5].copy_from_slice(&[l.token, l.list, l.generation, l.context, l.registry]);
            };
            put(&mut producer, 12, &b.recording);
            put(&mut producer, 17, &b.creation_recording);
            let epoch = crate::source_cbv_snapshot::ReadEpochCertificate { list: row[6], generation: row[7], occurrence: row[0] + 1, certified: true };
            // SAFETY: the backing and the certificate.
            producer[22] = u64::from(unsafe { crate::source_cbv_snapshot::cvr_scs_inline_upload(0, &b, &epoch, u.bytes) });
            producer[24] = b.writer_nonce;
            producer[25] = u64::from(b.writer_bytes);
            producer[26] = b.writer_destination;
            put(&mut producer, 27, &b.writer_recording);
            // SAFETY: as above.
            producer[32] = u64::from(unsafe { crate::source_cbv_snapshot::cvr_scs_inline_upload(1, &b, &epoch, u.bytes) });
        } else {
            w.cbv_failed = true;
        }
        // The backing's references move into the source (kept) or are released (not).
        if keep {
            source.cbvs.push(b);
        } else {
            work::release_cbv_backing(&b);
        }
        if w.source_details {
            if value[23] != 0 {
                if let Some(kept) = source.cbvs.as_slice().last() {
                    capture_details(list, d.root, &value, kept);
                }
            } else {
                let mut detail = [0u64; 128];
                detail[0] = rows_mut::<24>(w, CBV_ROWS).len() as u64;
                detail[1..7].copy_from_slice(&value[..6]);
                detail[39] = 2; // source_cbv_details::Status::SourceBefore
                // SAFETY: the work's journal; one detail row.
                unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), DETAIL_ROWS, detail.as_ptr(), 1) };
                w.details_failed = true;
            }
        }
        // SAFETY: the work's journal; one row each.
        unsafe {
            crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_ROWS, value.as_ptr(), 1);
            crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_PRODUCERS, producer.as_ptr(), 1);
            crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), CBV_DIAGNOSTICS, diagnostic.as_ptr(), 1);
        }
    }
    true
}
const T5_NATIVE_ROWS: u32 = 47;
const T5_COPY_DIAGNOSTICS: u32 = 49;
const T5_MARK_DIAGNOSTICS: u32 = 50;
/// descriptors::ResolveSrvAtDraw into `binding` (emptied first: its resource released).
fn resolve_srv_at_draw(list: *mut c_void, root: u64, parameter: u32, offset: u32, binding: &mut crate::material_buffer_snapshot::SrvBinding) -> bool {
    if binding.resource != 0 {
        // SAFETY: the binding's own reference.
        drop(unsafe { IUnknown::from_raw(binding.resource as *mut c_void) });
    }
    // SAFETY: a plain record; all zeroes its empty value (as the .cpp's `output={}`).
    *binding = unsafe { core::mem::zeroed() };
    // SAFETY: plain records.
    let mut stamp: crate::descriptor_audit::DescriptorStamp = unsafe { core::mem::zeroed() };
    let mut resource: *mut c_void = null_mut();
    // SAFETY: as above.
    let mut view: D3D12_SHADER_RESOURCE_VIEW_DESC = unsafe { core::mem::zeroed() };
    // SAFETY: the live list; writable outputs (the resource's reference handed over).
    if !unsafe { crate::descriptor_audit::cvr_sda_audit_srv(list as u64, root, parameter, offset, &mut stamp, &mut resource, &mut view) } {
        return false;
    }
    binding.description = stamp.value;
    // SAFETY: the same 176-byte DescriptorStamp record.
    binding.stamp = unsafe { core::mem::transmute_copy(&stamp) };
    binding.resource = resource as u64;
    binding.view = view;
    true
}
/// CaptureMultilayerT5: the source's multilayer T5 (t5, parameter 23 offset 5) at the draw -- its binding resolved and
/// checked (a 128-byte-stride structured buffer in a default heap, the descriptor unchanged), its creation, the B4
/// window over the first selector input's intent (`b4`), the original slot's certification, the native read at the
/// source, and the copy and mark diagnostics (one row each).
///
/// # Safety
/// `list` is the live list; `source` the work's source (its scalars); `command` its source row; `b4` the first
/// selector input's intent bytes (`b4_bytes` of them) when `has_b4`.
pub(super) unsafe fn capture_multilayer_t5(
    list: *mut c_void,
    source: *mut SourceScalars,
    command: *const [u64; 48],
    b4: *const u8,
    b4_bytes: usize,
    has_b4: bool,
    tables: &[crate::native_descriptor_arena::SourceTable],
) {
    let Some(w) = work() else { return };
    // SAFETY: as documented.
    let (source, command) = unsafe { (&mut *source, &*command) };
    let mut row = [0u64; 32];
    row[0] = command[0];
    row[1] = command[1];
    row[27] = command[7];
    let (mut parameter, mut offset) = (0u32, 0u32);
    // SAFETY: writable outputs (D3D12_DESCRIPTOR_RANGE_TYPE_SRV, space 0, t5, D3D12_SHADER_VISIBILITY_PIXEL).
    let found = unsafe { crate::descriptor_audit::cvr_sda_audit_find_binding(command[11], 0, 0, 5, 5, &mut parameter, &mut offset) };
    if !found || parameter != 23 || offset != 5 || !resolve_srv_at_draw(list, command[11], parameter, offset, &mut source.multilayer_t5) {
        row[3] = 1;
    } else {
        let binding = &source.multilayer_t5;
        let resource_raw = binding.resource as *mut c_void;
        // SAFETY: the binding's held resource.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource_raw) }.expect("a resolved SRV's resource");
        // SAFETY: a plain query of the live resource.
        let desc = unsafe { resource.GetDesc() };
        // SAFETY: buffer views (the union's Buffer member, as the .cpp reads it).
        let buffer = unsafe { binding.view.Anonymous.Buffer };
        row[4] = u64::from(parameter);
        row[5] = u64::from(offset);
        row[6] = binding.stamp.gpu;
        row[7] = binding.stamp.heap;
        row[8] = binding.description[0];
        row[9] = binding.description[2];
        row[10] = binding.resource;
        row[11] = buffer.FirstElement;
        row[12] = u64::from(buffer.NumElements);
        row[13] = u64::from(buffer.StructureByteStride);
        let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
        let stamp_unchanged = || {
            // SAFETY: the binding's stamp as descriptor_audit's record (descriptors::MatchesDescriptor reads it).
            unsafe { crate::descriptor_audit::cvr_sda_audit_stamp(1, 0, 0, 0, 0, (&raw const binding.stamp).cast_mut().cast(), null_mut()) }
        };
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || binding.view.ViewDimension != windows::Win32::Graphics::Direct3D12::D3D12_SRV_DIMENSION_BUFFER
            || binding.view.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN
            || buffer.Flags.0 != 0
            || row[13] != 128
            // SAFETY: plain queries of the live resource.
            || unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err()
            || heap.Type != D3D12_HEAP_TYPE_DEFAULT
            || !stamp_unchanged()
        {
            row[3] = 2;
        } else {
            row[2] = 1;
            let (mut kind, mut known, mut state, mut creation_heap, mut heap_offset, mut bytes) = (0u32, false, 0u32, null_mut(), 0u64, 0u64);
            // SAFETY: the live resource; writable outputs (constants::QueryResourceCreation; the heap's reference ours).
            if unsafe { crate::creation_receipts::cvr_crc_query(resource_raw, &mut kind, &mut known, &mut state, &mut creation_heap, &mut heap_offset, &mut bytes) } {
                row[21] = u64::from(kind);
                row[22] = u64::from(known);
                row[23] = u64::from(state);
                row[24] = creation_heap as u64;
                row[25] = heap_offset;
                row[26] = bytes;
                if !creation_heap.is_null() {
                    // SAFETY: the reference the query handed over (only its address is recorded).
                    drop(unsafe { IUnknown::from_raw(creation_heap) });
                }
            }
            if has_b4 {
                // SAFETY: the intent's bytes, as documented.
                unsafe { crate::retained_capture::cvr_nrg_t5_window(b4, b4_bytes, desc.Width, &mut row) };
            }
        }
    }
    if row[2] != 0 {
        let captured = tables.iter().find(|t| t.parameter == 23);
        source.t5_original_certified = captured.is_some_and(|t| {
            // SAFETY: the captured table and the binding, as audit_source_tables' records.
            unsafe { crate::audit_source_tables::cvr_sda_st_matches((t as *const crate::native_descriptor_arena::SourceTable).cast(), 5, (&raw const source.multilayer_t5).cast()) }
        });
    }
    // SAFETY: the work's journal; one row.
    unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), T5_ROWS, row.as_ptr(), 1) };
    let mut native = [0u64; 96];
    native[0] = command[0];
    native[1] = command[1];
    native[15] = command[6];
    native[16] = command[7];
    if row[2] != 0 {
        let recording = (&raw const w.eye(command[1] as usize).recording).cast::<crate::native_recording::Lease>();
        // SAFETY: the live list, the held binding, the eye's recording and the source's (empty) native backing.
        native[2] = u64::from(unsafe {
            crate::source_srv_observe::cvr_ssrvo_read_at_source(list as u64, command[11], 23, 5, (&raw const source.multilayer_t5).cast(), recording, &mut source.t5_native)
        });
        let c = &source.t5_native;
        native[3] = u64::from(c.stage);
        native[4] = u64::from(c.shader_register);
        native[5] = u64::from(c.route);
        native[6] = u64::from(c.low_type);
        native[7] = u64::from(c.native.handle);
        native[8] = u64::from(c.native.kind);
        native[9] = c.native.gpu;
        native[10] = c.native.bytes;
        native[11] = c.native.resource;
        native[12] = c.native.registry;
        native[13] = c.context.context;
        native[14] = c.context.state;
        native[17] = c.record_address;
        native[18] = c.cpu_descriptor;
        native[19] = c.serial;
        native[20] = u64::from(c.state.status);
        native[21] = u64::from(c.state.value);
        native[22] = u64::from(c.state.kind);
        native[23] = u64::from(native[2] == SRV_CAPTURED);
        native[24] = u64::from(c.allocation.observation);
        native[25] = c.allocation.address;
        native[26] = u64::from(c.allocation.kind);
        native[27] = c.allocation.resource;
        native[28] = c.allocation.bytes;
        native[29] = c.allocation.heap;
        native[30] = c.allocation.pimpl;
        native[31] = c.allocator.address;
        native[32] = c.allocator.pimpl;
        native[33] = u64::from(c.allocator.observation);
        native[34] = c.gpu;
        native[35] = c.backing_bytes;
        native[36] = c.source_offset;
        native[37] = c.view_bytes;
        native[38] = u64::from(c.slot.group);
        native[39] = u64::from(c.slot.first);
        native[40] = u64::from(c.slot.count);
        native[41] = u64::from(c.slot.cached_index);
        native[42] = u64::from(crate::source_srv_observe::cvr_ssrvo_compatible());
        native[43] = u64::from(c.heap_type);
        native[44] = u64::from(c.flags);
        for (i, chunk) in c.raw_record.chunks(8).enumerate() {
            native[48 + i] = u64::from_le_bytes(chunk.try_into().expect("8 bytes"));
        }
        for (i, chunk) in c.slot.raw.chunks(8).enumerate() {
            native[72 + i] = u64::from_le_bytes(chunk.try_into().expect("8 bytes"));
        }
    }
    // SAFETY: the work's journal; one row.
    unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), T5_NATIVE_ROWS, native.as_ptr(), 1) };
    let (mut copied, mut marked) = ([0u64; 32], [0u64; 32]);
    copied[0] = command[0];
    marked[0] = command[0];
    copied[1] = command[1];
    marked[1] = command[1];
    let resource = source.multilayer_t5.resource as *mut c_void;
    if !resource.is_null() {
        let mut diagnostic = [0u64; 24];
        // SAFETY: the held resource; a writable diagnostic (source_resource_intent::QueryCopyDiagnostic).
        if unsafe { crate::source_intent::cvr_sri_query_diagnostic(resource, false, &mut diagnostic) } {
            copied[2] = 1;
            copied[4..28].copy_from_slice(&diagnostic);
        }
        let mut diagnostic = [0u64; 24];
        // SAFETY: as above (QueryMarkDiagnostic).
        if unsafe { crate::source_intent::cvr_sri_query_diagnostic(resource, true, &mut diagnostic) } {
            marked[2] = 1;
            marked[4..28].copy_from_slice(&diagnostic);
        }
    }
    // SAFETY: the work's journal; one row each.
    unsafe {
        crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), T5_COPY_DIAGNOSTICS, copied.as_ptr(), 1);
        crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), T5_MARK_DIAGNOSTICS, marked.as_ptr(), 1);
    }
}
fn words(row: &mut [u64; 128], at: usize, bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(8).enumerate() {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        row[at + i] = u64::from_le_bytes(word);
    }
}
/// CaptureDetails: the CBV backing's source details at the draw (one detail row).
fn capture_details(list: *mut c_void, root: u64, cbv: &[u64; 24], backing: &crate::source_cbv_snapshot::SourceCbvBacking) {
    let w = work().expect("capturing");
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let mut row = [0u64; 128];
    row[0] = journal.count(CBV_ROWS) as u64;
    row[1..7].copy_from_slice(&cbv[..6]);
    // SAFETY: a plain detail, written whole by the capture.
    let mut detail: crate::source_cbv_snapshot_receipt::Detail = unsafe { core::mem::zeroed() };
    // SAFETY: the backing (Backing's layout) and a writable detail.
    let captured = unsafe {
        crate::source_cbv_snapshot_receipt::cvr_scsn_capture_at_source(list, root, cbv[4] as u32, cbv[5] as u32, (backing as *const crate::source_cbv_snapshot::SourceCbvBacking).cast(), &mut detail)
    };
    let shape = &detail.resource;
    row[7] = u64::from(captured);
    row[8] = shape.heap.Type.0 as u32 as u64;
    row[9] = shape.heap_flags.0 as u32 as u64;
    row[10] = shape.flags.0 as u32 as u64;
    row[11] = shape.width;
    row[12] = shape.gpu;
    row[13] = shape.com_offset;
    row[14] = backing.recording.generation;
    row[15] = backing.recording.context;
    row[16] = root;
    row[17] = list as u64;
    if captured {
        let (mut kind, mut known, mut state, mut heap, mut offset, mut bytes) = (0u32, false, 0u32, null_mut(), 0u64, 0u64);
        // SAFETY: the backing's live resource; writable outputs.
        if unsafe { crate::creation_receipts::cvr_crc_query(backing.resource as *mut c_void, &mut kind, &mut known, &mut state, &mut heap, &mut offset, &mut bytes) } {
            row[18] = u64::from(kind);
            row[19] = u64::from(known);
            row[20] = if known { u64::from(state) } else { 0 };
            row[21] = heap as u64;
            row[22] = offset;
            row[23] = bytes;
        }
        if !heap.is_null() {
            // SAFETY: the receipt's heap reference, handed over.
            drop(unsafe { IUnknown::from_raw(heap) });
        }
    }
    use crate::source_cbv_details::{KNOWN, UNKNOWN_ABI, UNSUPPORTED};
    let context = &detail.context;
    row[24] = u64::from(context.observation == KNOWN);
    row[25] = u64::from(context.kind);
    row[26] = u64::from(context.pending);
    row[27] = context.submitted;
    row[28] = u64::from(detail.state.status);
    row[29] = u64::from(detail.state.value);
    row[30] = u64::from(detail.state.kind);
    if detail.record.observation == KNOWN {
        row[31] = 1;
        row[32] = detail.record.address;
        words(&mut row, 40, &detail.record.bytes);
    }
    row[33] = u64::from(backing.native.kind);
    row[34] = u64::from(backing.native.handle);
    row[35] = backing.resource;
    row[36] = backing.stamp.value[4];
    row[37] = u64::from(backing.descriptor_bytes);
    row[38] = cbv[6];
    row[39] = u64::from(detail.status);
    let readable = |value: u32| value == KNOWN || value == UNKNOWN_ABI || value == UNSUPPORTED;
    let (allocation, allocator, pool, alias) = (&detail.allocation, &detail.allocator, &detail.pool, &detail.alias);
    if readable(allocation.observation) {
        row[64] = 1;
        row[65] = allocation.address;
        words(&mut row, 66, &allocation.raw);
    }
    if readable(allocator.observation) {
        row[79] = 1;
        row[80] = allocator.address;
        words(&mut row, 81, &allocator.raw);
    }
    if pool.observation == KNOWN {
        row[84] = 1;
        row[85] = pool.entry;
        row[86] = pool.owner;
        row[87] = u64::from(pool.packed);
        row[88] = u64::from(pool.slot);
        row[89] = u64::from(pool.unit);
        row[90] = pool.occupied_word;
    }
    if readable(alias.observation) {
        row[96] = 1;
        row[97] = alias.payload;
        row[98] = alias.control;
        words(&mut row, 99, &alias.raw);
    }
    row[106] = u64::from(allocation.observation);
    row[107] = u64::from(allocator.observation);
    row[108] = u64::from(pool.observation);
    row[109] = u64::from(alias.observation);
    row[110] = u64::from(detail.allocation_path);
    row[111] = u64::from(allocation.association_known);
    row[112] = u64::from(allocation.resource_matches);
    row[113] = u64::from(allocation.extent_covers);
    row[114] = u64::from(allocation.alias_shape_known);
    row[115] = u64::from(allocation.alias_shape_matches);
    row[116] = u64::from(allocator.association_known);
    row[117] = u64::from(allocator.pimpl_matches);
    row[118] = u64::from(shape.heap.CreationNodeMask);
    row[119] = u64::from(shape.heap.VisibleNodeMask);
    row[120] = shape.heap.CPUPageProperty.0 as u32 as u64;
    row[121] = shape.heap.MemoryPoolPreference.0 as u32 as u64;
    row[122] = backing.native.registry;
    row[123] = context.state;
    row[124] = allocation.heap;
    row[125] = allocation.block;
    row[126] = allocation.handle;
    if !captured {
        w.details_failed = true;
    }
    journal.append(DETAIL_ROWS, &row);
}
extern "C" fn poll_fail(reason: u32) {
    fail(reason);
}
extern "C" fn poll_publish() {
    publish_sources_impl();
}
fn finish_poll(w: &mut WorkScalars) {
    w.complete = true;
    WATCHING.store(false, SeqCst);
}
/// Poll: once both eyes' submissions completed, the payloads read back and compared, the overlays and the private
/// geometry built, the sources assembled and staged, the capture completed.
fn poll() {
    let Some(w) = work() else { return };
    if w.complete {
        return;
    }
    for eye in w.eyes() {
        // SAFETY: a live fence.
        let completed = eye.fence.as_ref().map(|f| unsafe { f.GetCompletedValue() });
        if !eye.submitted || !eye.submission_certified || eye.source_queue.is_none() || eye.signaled_value == 0 || completed.is_none_or(|c| c < eye.signaled_value) {
            return;
        }
    }
    if !w.payloads_processed {
        let at = |p: *mut c_void| p as u64;
        let eye_payload = |side: u32| {
            let eye = w.eye(side as usize);
            crate::retained_journal::PayloadEye {
                instance_readback: at(eye_rust(side).instance_readback.as_ref().map_or(null_mut(), |r| r.as_raw())),
                instance_bytes: eye.instance_readback_bytes,
                fence: eye.fence.as_ref().map_or(0, |f| f.as_raw() as u64),
                signaled: eye.signaled_value,
                readback: eye.readback.as_ref().map_or(0, |r| r.as_raw() as u64),
                planes: eye.planes.as_ptr(),
                plane_count: eye.plane_count as usize,
                targets: core::array::from_fn(|slot| eye.targets.resources[slot].as_ref().map_or(0, |r| r.as_raw() as u64)),
                cbv_readback: at(cvr_nrg2_cbv_readback(side)),
                t5_readback: at(cvr_nrg2_readback(side, 1)),
                full_original: at(cvr_nrg2_readback(side, 2)),
                full_private: at(cvr_nrg2_readback(side, 3)),
                full_reference_bytes: eye.full_t5_reference_bytes,
                full_arena_bytes: structured_bytes(side, true),
                t5_aborted: eye.t5_aborted,
                full_ready: structured_finalized(side, true),
            }
        };
        let (sources, count) = sources::payload_sources();
        let payloads = crate::retained_journal::Payloads {
            eyes: [eye_payload(0), eye_payload(1)],
            sources,
            source_count: count,
            mode: w.mode,
            source_cbv_snapshots: w.source_cbv_snapshots,
            source_selectors: w.source_selectors,
            full_t5: w.full_t5_enabled,
            cbv_snapshots_failed: &mut w.cbv_snapshots_failed,
            cbv_payload_complete: &mut w.cbv_payload_complete,
            fail: poll_fail,
            publish: poll_publish,
        };
        // SAFETY: the work's journal and the payloads above.
        if unsafe { crate::retained_journal::cvr_nrg_poll_payloads(w.journal.cast(), &payloads) } {
            finish_poll(w);
            return;
        }
        w.payloads_processed = true;
    }
    if w.failed {
        finish_poll(w);
        publish_sources_impl();
        return;
    }
    if !sources::poll_overlay(w) {
        if w.overlay_stage == 3 {
            finish_poll(w);
        }
        publish_sources_impl();
        return;
    }
    if !prepare_instance_overlay() {
        if w.instance_overlay_stage == 3 {
            finish_poll(w);
        }
        publish_sources_impl();
        return;
    }
    if build_private_geometry(w).is_err() {
        fail(1820);
    }
    finalize_material_buffer_copies(w);
    sources::poll_assemblies(w);
    finalize_material_resources(w);
    if !pairs::validate_staged_sources() {
        fail(2421);
    }
    if ex!(CyberpunkVR_RetainedGeometryDifferent).iter().any(|v| *v != 0) {
        fail(92);
    }
    if !w.failed {
        publish_shaders();
        if w.mode == 1 {
            state().verified = rust_side().pipelines.clone();
            CyberpunkVR_RetainedGeometryVerified.store(1, SeqCst);
        }
        CyberpunkVR_RetainedGeometryState.store(3, SeqCst);
    }
    finish_poll(w);
    publish_sources_impl();
}

/// The source's geometry (its command's DrawResources) copied: its leases and state referenced again.
fn copied_geometry(source: &SourceScalars) -> crate::geometry_leases::DrawResources {
    source.geometry.clone()
}
const PRIVATE_GEOMETRY_ROWS: u32 = 23;
/// BuildPrivateGeometry (formerly the .cpp's): once the IA snapshots were read back, each source whose IA copy completed
/// gets its private geometry -- its draw with the private IA leases, and for a GPU-matched instance stream the private
/// instance view and its read-back bytes; a row per source. Err: a source without IA selection or whose private instance
/// coordinates changed (the caller fails 1820).
fn build_private_geometry(w: &mut WorkScalars) -> Result<(), ()> {
    if !w.source_ia_snapshots || !rows_mut::<16>(w, PRIVATE_GEOMETRY_ROWS).is_empty() {
        return Ok(());
    }
    for index in 0..source_count() {
        let Some(source) = source(index as u64) else { break };
        let Some(&saved) = rows_mut::<48>(w, SOURCE_ROWS).get(index) else { return Err(()) };
        let mut row = [0u64; 16];
        row[0] = index as u64;
        row[1] = saved[1];
        row[3] = saved[19];
        row[4] = saved[6];
        row[5] = saved[7];
        row[6] = index as u64 + 1;
        let (Some(&copied), Some(&selection)) = (rows_mut::<16>(w, IA_COMMANDS).get(index), rows_mut::<5>(w, IA_SELECTIONS).get(index)) else {
            return Err(());
        };
        if copied[6] != 0 && copied[14] != 0 && copied[15] != 0 && copied[7] == 0 {
            let mut assembled = copied_geometry(source);
            let mut ready = selection[2] == 0;
            // SAFETY: plain views; all zeroes before the call.
            let mut view: InstanceViewOut = unsafe { core::mem::zeroed() };
            let matched = selection[2] == 128
                && rows_mut::<24>(w, INSTANCE_GPU_ROWS).get(index).is_some_and(|r| r[5] == 1)
                // SAFETY: a local output.
                && unsafe { cvr_nrg2_instance_view(index, 0, saved[1] as u32, null_mut(), &mut view) };
            if matched {
                // SAFETY: the view's live private buffer.
                let resource = unsafe { ID3D12Resource::from_raw_borrowed(&view.resource) }.cloned().ok_or(())?;
                // SAFETY: as above.
                let (desc, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
                // SAFETY: the view's scope and draw (the same 20-byte indexed arguments), the source row.
                if !unsafe { crate::retained_staged::cvr_nrg_instance_view(&view.scope, (&raw const view.draw).cast(), &saved, index as u64) } {
                    return Err(());
                }
                assembled.vertices[7] = crate::geometry_leases::BufferLease {
                    resource: Some(resource),
                    gpu,
                    bytes: desc.Width,
                    heap_type: windows::Win32::Graphics::Direct3D12::D3D12_HEAP_TYPE_DEFAULT.0 as u32,
                    flags: desc.Flags.0 as u32,
                    native: Default::default(),
                };
                assembled.draw.vertices[7] = windows::Win32::Graphics::Direct3D12::D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: view.vertex.buffer_location,
                    SizeInBytes: view.vertex.size_in_bytes,
                    StrideInBytes: view.vertex.stride_in_bytes,
                };
                let gpu_row = rows_mut::<24>(w, INSTANCE_GPU_ROWS)[index];
                // SAFETY: the Work's journal.
                let journal = unsafe { &*w.journal.cast::<crate::retained_journal::Journal>() };
                let blob = journal.blob(INSTANCE_GPU_BLOB);
                let begin = (gpu_row[18] as usize).wrapping_sub(1);
                let bytes = blob.get(begin..begin.saturating_add(gpu_row[11] as usize)).ok_or(())?;
                assembled.instances = bytes.to_vec();
                ready = true;
                row[9] = view.resource as u64;
                row[10] = view.owner as u64;
                row[11] = view.vertex.buffer_location;
            }
            if ready {
                row[2] = 1;
                row[7] = source.ia_prepared.owner as u64;
                row[8] = source.ia_views.as_slice().len() as u64;
                row[12] = u64::from(assembled.draw.first_instance);
                row[13] = u64::from(assembled.draw.instances);
                let mut boxed = Box::new(PrivateGeometry {
                    resources: assembled,
                    view: PrivateGeometryView { draw: core::ptr::null(), index: core::ptr::null(), vertices: core::ptr::null() },
                });
                boxed.view = PrivateGeometryView {
                    draw: &boxed.resources.draw,
                    index: &boxed.resources.index,
                    vertices: boxed.resources.vertices.as_ptr(),
                };
                source_rust(index).private_geometry = Some(boxed);
            }
        }
        // SAFETY: the Work's journal.
        unsafe { &mut *w.journal.cast::<crate::retained_journal::Journal>() }.append(PRIVATE_GEOMETRY_ROWS, &row);
    }
    Ok(())
}

// ---- the materials

/// Work::Source::MaterialBuffer (the buffer's source SRV backing, its private copy, its copy row).
#[repr(C)]
pub struct MaterialBuffer {
    pub backing: crate::source_srv_observe::Backing,
    pub copy: crate::material_snapshot_receipt::Copy,
    pub row: usize,
}
/// texture_read::ImmutableLease with its two references (the owner, the source resource) owned.
struct OwnedImmutable(crate::texture_immutable::ImmutableLease);
impl Clone for OwnedImmutable {
    fn clone(&self) -> Self {
        // SAFETY: a plain lease; its references gain one each.
        let copy = unsafe { core::ptr::read(&self.0) };
        crate::structured_snapshot_receipt::add_ref(copy.owner);
        crate::structured_snapshot_receipt::add_ref(copy.source.resource);
        OwnedImmutable(copy)
    }
}
impl Drop for OwnedImmutable {
    fn drop(&mut self) {
        crate::structured_snapshot_receipt::release(self.0.owner);
        crate::structured_snapshot_receipt::release(self.0.source.resource);
    }
}
/// material_buffer_snapshot::Failure.
const MATERIAL_NATIVE: u64 = 4;
const MATERIAL_CAPACITY: u64 = 9;
/// source_srv::Status::InvalidBinding / Captured.
const SRV_INVALID_BINDING: u64 = 5;
const SRV_CAPTURED: u64 = 1;
const MATERIAL_BUFFER_ROWS: u32 = 16;
const MATERIAL_BUFFER_COPIES: u32 = 17;
const MATERIAL_SOURCE_ROWS: u32 = 18;
const SOURCE_ASSEMBLY_DETAIL_LIMIT: usize = 65536;
fn material_arena(side: u32) -> *mut c_void {
    eye_rust(side).material_arena.as_ref().map_or(null_mut(), |a| a.as_raw())
}
fn material_abort(side: u32) {
    let arena = material_arena(side);
    if !arena.is_null() {
        // SAFETY: the arena's live storage.
        unsafe { crate::material_snapshot_receipt::cvr_mbsn_arena_abort(arena) };
    }
}
fn resource_of(raw: u64) -> Option<&'static ID3D12Resource> {
    let raw = raw as *mut c_void;
    // SAFETY: a live resource of the capture's source (or null).
    unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.map(|r| unsafe { &*(r as *const ID3D12Resource) })
}
/// CaptureMaterialBuffer: a buffer material's SRV read at the source and copied into the eye's material arena.
fn capture_material_buffer(list: *mut c_void, source: &mut SourceScalars, saved: &[u64; 48], binding: &crate::material_buffer_snapshot::SourceResourceBinding) {
    let w = work().expect("capturing");
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    if binding.parameter != 23 || (binding.offset != 4 && binding.offset != 6) || journal.count(MATERIAL_BUFFER_ROWS) >= SOURCE_ASSEMBLY_DETAIL_LIMIT {
        return;
    }
    let mut row = [0u64; 40];
    row[0] = saved[0];
    row[1] = saved[1];
    row[2] = u64::from(binding.parameter);
    row[3] = u64::from(binding.offset);
    row[4] = SRV_INVALID_BINDING;
    row[5] = binding.resource;
    let resource = resource_of(binding.resource);
    if let Some(r) = resource {
        // SAFETY: a live resource.
        unsafe {
            row[6] = r.GetGPUVirtualAddress();
            row[7] = r.GetDesc().Width;
            row[11] = r.GetDesc().Flags.0 as u32 as u64;
        }
    }
    row[33] = saved[6];
    row[34] = saved[7];
    row[35] = saved[0] + 1;
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    if resource.is_some_and(|r| unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()) {
        row[10] = heap.Type.0 as u32 as u64;
    }
    let (mut kind, mut known, mut state, mut creation_heap, mut offset, mut bytes) = (0u32, false, 0u32, null_mut(), 0u64, 0u64);
    // SAFETY: the binding's resource; writable outputs.
    if unsafe { crate::creation_receipts::cvr_crc_query(binding.resource as *mut c_void, &mut kind, &mut known, &mut state, &mut creation_heap, &mut offset, &mut bytes) } {
        row[24] = u64::from(kind);
        row[25] = creation_heap as u64;
        row[26] = offset;
        row[27] = bytes;
    }
    if !creation_heap.is_null() {
        // SAFETY: the receipt's heap reference, handed over.
        drop(unsafe { IUnknown::from_raw(creation_heap) });
    }
    let eye = w.eye(saved[1] as usize);
    let mut backing = crate::structured_snapshot_receipt::OwnedBacking::empty();
    // SAFETY: a plain SRV binding (its resource reference released below).
    let mut held: crate::material_buffer_snapshot::SrvBinding = unsafe { core::mem::zeroed() };
    let mut held_resource = null_mut();
    // SAFETY: writable outputs.
    let resolved = unsafe {
        crate::descriptor_audit::cvr_sda_audit_srv(list as u64, saved[11], binding.parameter, binding.offset, (&mut held.stamp as *mut crate::source_cbv_binding::DescriptorStamp).cast(), &mut held_resource, &mut held.view)
    };
    held.resource = held_resource as u64;
    held.description = held.stamp.value;
    if resolved && held.resource == binding.resource && held.stamp.value == binding.stamp.value {
        // SAFETY: the view's buffer arm (a buffer SRV).
        let buffer = unsafe { held.view.Anonymous.Buffer };
        row[28] = held.view.Format.0 as u32 as u64;
        row[29] = buffer.FirstElement;
        row[30] = u64::from(buffer.NumElements);
        row[31] = u64::from(buffer.StructureByteStride);
        row[32] = buffer.Flags.0 as u32 as u64;
        // SAFETY: the held binding (SrvBinding's layout), the eye's lease (native_recording::Lease's) and a value-initialised backing.
        row[4] = u64::from(unsafe {
            crate::source_srv_observe::cvr_ssrvo_read_at_source(
                list as u64,
                saved[11],
                binding.parameter,
                binding.offset,
                (&held as *const crate::material_buffer_snapshot::SrvBinding).cast(),
                (&eye.recording as *const Recording).cast(),
                &mut *backing.0,
            )
        });
        if row[4] == SRV_CAPTURED {
            let b = &backing.0;
            row[8] = b.source_offset;
            row[9] = b.view_bytes;
            row[12] = u64::from(b.native.handle);
            row[13] = u64::from(b.native.kind);
            row[14] = b.native.bytes;
            row[15] = u64::from(b.low_type);
            row[16] = u64::from(b.state.status);
            row[17] = u64::from(b.state.value);
            row[18] = u64::from(b.allocation.observation);
            row[19] = b.allocation.address;
            row[20] = u64::from(b.allocation.kind);
            row[21] = b.allocation.bytes;
            row[22] = u64::from(b.allocator.observation);
            row[23] = b.allocator.address;
            row[36] = b.record_address;
            row[37] = b.cpu_descriptor;
        }
    }
    crate::structured_snapshot_receipt::release(held_resource);
    journal.append(MATERIAL_BUFFER_ROWS, &row);
    let mut copied = [0u64; 16];
    copied[0] = saved[0];
    copied[1] = saved[1];
    copied[2] = u64::from(binding.offset);
    copied[3] = row[4];
    copied[4] = MATERIAL_NATIVE;
    copied[13] = saved[6];
    copied[14] = saved[7];
    copied[15] = saved[0] + 1;
    if row[4] == SRV_CAPTURED && w.source_selectors {
        let side = saved[1] as u32;
        // SAFETY: a plain copy, written whole by the capture (its references then the source's).
        let mut copy: crate::material_snapshot_receipt::Copy = unsafe { core::mem::zeroed() };
        let copy_row = journal.count(MATERIAL_BUFFER_COPIES);
        if eye_rust(side).material_arena.is_none() {
            let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
            let mut made = null_mut();
            // SAFETY: a live device and list; the storage handed over.
            let hr = unsafe { crate::material_snapshot_receipt::cvr_mbsn_arena_create(null_mut(), device, list, saved[7], &mut made) };
            if hr.is_ok() && !made.is_null() {
                // SAFETY: as above.
                eye_rust(side).material_arena = Some(unsafe { IUnknown::from_raw(made) });
            }
        }
        let arena = material_arena(side);
        if !arena.is_null() {
            let mut stamp = crate::indirect_copy::ScopeStamp::default();
            // SAFETY: the scope's stamp, exited below.
            unsafe { crate::material_snapshot_receipt::cvr_mbsn_scope_enter(list as u64, saved[7], saved[0] + 1, &mut stamp) };
            let epoch = crate::material_snapshot_receipt::Epoch { list: saved[6], generation: saved[7], occurrence: saved[0] + 1, certified: true };
            // SAFETY: the stamp; the backing, parent, epoch, arena and a writable copy.
            let captured = unsafe { crate::material_snapshot_receipt::cvr_mbsn_scope_active(&stamp) }
                && unsafe { crate::material_snapshot_receipt::cvr_mbsn_capture(list, saved[11], &*backing.0, (&source.parent as *const GraphParentRecord).cast(), &epoch, arena, &mut copy) };
            // SAFETY: the stamp entered above.
            unsafe { crate::material_snapshot_receipt::cvr_mbsn_scope_exit(&stamp) };
            if captured {
                copied[4] = 0;
                copied[5] = copy.owner as u64;
                copied[6] = copy.resource as u64;
                copied[7] = copy.source_offset;
                copied[8] = copy.offset;
                copied[9] = copy.bytes;
                copied[10] = 1;
            } else {
                copied[4] = u64::from(crate::material_snapshot_receipt::cvr_mbsn_last_failure());
            }
        } else {
            copied[4] = MATERIAL_CAPACITY;
        }
        // The backing's and the copy's references move into the source's item (the move leaves the backing null).
        // SAFETY: the backing's record, its references moved out and the record zeroed after.
        let moved = unsafe { core::ptr::read(&*backing.0) };
        // SAFETY: as above.
        unsafe { core::ptr::write_bytes(&mut *backing.0 as *mut crate::source_srv_observe::Backing, 0, 1) };
        source.material_buffers.push(MaterialBuffer { backing: moved, copy, row: copy_row });
    }
    journal.append(MATERIAL_BUFFER_COPIES, &copied);
}
/// CaptureMaterialResources: each finite and sparse dependency's material row, the textures' immutable leases.
fn capture_material_resources(list: *mut c_void, source: &mut SourceScalars, saved: &[u64; 48]) {
    let w = work().expect("capturing");
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let [finite, sparse] = sources::dependencies_of(saved[0] as usize);
    let groups = [finite.as_ptr_range(), sparse.as_ptr_range()];
    for range in groups {
        let mut at = range.start;
        while at < range.end {
            // SAFETY: the dependency vectors' elements (the source's; only the material vectors grow below).
            let binding = unsafe { &*at };
            at = at.wrapping_add(1);
            if journal.count(MATERIAL_ROWS) >= SOURCE_ASSEMBLY_DETAIL_LIMIT {
                source.material_truncated = true;
                return;
            }
            let mut row = [0u64; 24];
            row[0] = saved[0];
            row[1] = saved[1];
            row[2] = u64::from(binding.parameter);
            row[3] = u64::from(binding.offset);
            row[4] = u64::from(binding.kind);
            row[5] = binding.resource;
            row[6] = binding.counter;
            row[7] = binding.stamp.value[2];
            row[14] = saved[6];
            row[15] = saved[7];
            row[16] = saved[0] + 1;
            let mut owned: Option<OwnedImmutable> = None;
            if binding.kind != 0 {
                row[8] = 3;
            } else if binding.resource == 0 && binding.counter == 0 && binding.stamp.value[2] == 0 {
                row[8] = 1;
            } else if let Some(r) = resource_of(binding.resource) {
                // SAFETY: a live resource.
                let desc = unsafe { r.GetDesc() };
                row[19] = desc.Dimension.0 as u32 as u64;
                row[20] = desc.Format.0 as u32 as u64;
                if desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER {
                    row[8] = 2;
                    capture_material_buffer(list, source, saved, binding);
                } else {
                    row[8] = 4;
                    let cache = &mut rust_side().material_cache;
                    // SAFETY: the cached lease and no queue.
                    let cached = cache.get(&(binding.resource as usize)).filter(|lease| unsafe { crate::texture_immutable::cvr_tr_validate_immutable(&lease.0, null_mut()) });
                    if let Some(lease) = cached {
                        owned = Some(lease.clone());
                    } else {
                        // SAFETY: a value-initialised lease, its references then this capture's.
                        let mut acquired = OwnedImmutable(unsafe { core::mem::zeroed() });
                        // SAFETY: a live list and resource; the writable lease.
                        if unsafe { crate::texture_immutable::cvr_tr_acquire_immutable(list, binding.resource as *mut c_void, &mut acquired.0) } {
                            cache.insert(binding.resource as usize, acquired.clone());
                            owned = Some(acquired);
                        } else {
                            row[9] = u64::from(crate::texture_lease::cvr_tr_failure());
                        }
                    }
                    if let Some(lease) = owned.as_ref().filter(|lease| !lease.0.owner.is_null()) {
                        row[8] = 5;
                        row[10] = lease.0.owner as u64;
                        row[11] = u64::from(lease.0.source.native.handle);
                        row[12] = lease.0.source.receipt;
                        row[13] = lease.0.source.owner;
                    }
                }
            } else {
                row[8] = 4;
                row[9] = 95;
            }
            let index = journal.count(MATERIAL_ROWS);
            // SAFETY: a value-initialised lease when none was acquired.
            let lease = owned.unwrap_or_else(|| OwnedImmutable(unsafe { core::mem::zeroed() }));
            // The lease's references move into the source's vector.
            source.material_rows.push(index);
            // SAFETY: the lease's record, its references now the source's.
            source.material_owners.push(unsafe { core::ptr::read(&lease.0) });
            core::mem::forget(lease);
            journal.append(MATERIAL_ROWS, &row);
        }
    }
}
/// FinalizeMaterialResources: each source's material rows decided against the sources' queues, overlays and copies.
fn finalize_material_resources(w: &mut WorkScalars) {
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    journal.clear(MATERIAL_SOURCE_ROWS);
    let source_rows = rows_mut::<48>(w, SOURCE_ROWS);
    for ordinal in 0..source_count() {
        let Some(source) = work::source_at(ordinal) else { continue };
        let plan = sources::material_plan_of(ordinal);
        let mut ready = plan.ready && !source.material_truncated;
        let rows = rows_mut::<24>(w, MATERIAL_ROWS);
        let copies = rows_mut::<16>(w, MATERIAL_BUFFER_COPIES);
        let [finite, sparse] = sources::dependencies_of(ordinal);
        let owners = source.material_owners.as_slice();
        let buffers = source.material_buffers.as_slice();
        for (index, &at) in source.material_rows.as_slice().iter().enumerate() {
            let Some(row) = rows.get_mut(at) else { continue };
            if row[8] == 1 {
                row[18] = row[17];
            } else if row[8] == 5 {
                let queue = w.eye(row[1] as usize).source_queue.as_ref().map_or(null_mut(), |q| q.as_raw());
                // SAFETY: the source's lease and the eye's queue.
                row[18] = u64::from(row[17] != 0 && owners.get(index).is_some_and(|lease| unsafe { crate::texture_immutable::cvr_tr_validate_immutable(lease, queue) }));
            } else if row[8] == 2
                && row[2] == 23
                && row[3] == 5
                && plan.t5_view
                && source.t5_original_certified
                && w.overlay_stage == 2
                && source.multilayer_t5.resource == row[5]
                && plan.ready
            {
                row[8] = 6;
                row[10] = plan.t5_view_owner;
                row[18] = 1;
            } else if row[8] == 2 && row[2] == 23 && (row[3] == 4 || row[3] == 6) {
                let found = buffers.iter().find(|item| u64::from(item.backing.shader_register) == row[3]);
                let binding = finite.get(index).or_else(|| index.checked_sub(finite.len()).and_then(|i| sparse.get(i)));
                if let (Some(found), Some(binding)) = (found, binding) {
                    // SAFETY: the item's private copy and the dependency's binding (SourceResourceBinding's layout).
                    let matches = copies.get(found.row).is_some_and(|c| c[12] != 0)
                        && unsafe { crate::material_snapshot_receipt::cvr_mbsn_matches(&found.copy, (binding as *const crate::material_buffer_snapshot::SourceResourceBinding).cast(), false, core::ptr::null(), null_mut()) };
                    if matches {
                        row[8] = 7;
                        row[10] = found.copy.owner as u64;
                        row[17] = 1;
                        row[18] = 1;
                    }
                }
            }
            ready &= row[18] != 0;
        }
        source.material_inputs_ready = ready;
        let side = source_rows.get(ordinal).map_or(0, |row| row[1]);
        journal.append(MATERIAL_SOURCE_ROWS, &[ordinal as u64, side, source.material_rows.as_slice().len() as u64, u64::from(ready), u64::from(source.material_truncated)]);
    }
}
/// FinalizeMaterialBufferCopies: each copied material buffer's readiness after its eye's submission.
fn finalize_material_buffer_copies(w: &mut WorkScalars) {
    let copies = rows_mut::<16>(w, MATERIAL_BUFFER_COPIES);
    for index in 0..source_count() {
        let Some(source) = source(index as u64) else { continue };
        for item in source.material_buffers.as_slice() {
            if item.copy.owner.is_null() {
                continue;
            }
            let Some(row) = copies.get_mut(item.row) else { continue };
            let eye = w.eye(row[1] as usize);
            // SAFETY: the item's copy and the eye's lease, queue and fence.
            row[12] = u64::from(
                row[11] != 0
                    && unsafe {
                        crate::material_snapshot_receipt::cvr_mbsn_ready(
                            &item.copy,
                            (&eye.recording as *const Recording).cast(),
                            eye.source_queue.as_ref().map_or(null_mut(), |q| q.as_raw()),
                            eye.fence.as_ref().map_or(null_mut(), |f| f.as_raw()),
                            eye.signaled_value,
                            eye.submitted && eye.submission_certified,
                        )
                    },
            );
        }
    }
}
/// BeforeSubmit's material copies: each validated on the queue (the arena aborted and Fail(2210) on a refusal).
fn validate_material_buffers(w: &mut WorkScalars, side: u32, queue: *mut c_void) {
    let copies = rows_mut::<16>(w, MATERIAL_BUFFER_COPIES);
    for index in 0..source_count() {
        let Some(source) = source(index as u64) else { continue };
        for item in source.material_buffers.as_slice() {
            if item.copy.owner.is_null() {
                continue;
            }
            let Some(row) = copies.get_mut(item.row) else { continue };
            if row[1] != u64::from(side) {
                continue;
            }
            // SAFETY: the item's copy and the hook's queue.
            row[11] = u64::from(unsafe { crate::material_snapshot_receipt::cvr_mbsn_validate_submission(&item.copy, queue) });
            if row[11] == 0 {
                material_abort(side);
                fail(2210);
            }
        }
    }
}
/// Seal's material stage.
fn seal_material(list: *mut c_void, side: u32) {
    let arena = material_arena(side);
    // SAFETY: the arena's live storage and the sealed list.
    if !arena.is_null() && unsafe { crate::material_snapshot_receipt::cvr_mbsn_arena_bytes(arena) } != 0 && !unsafe { crate::material_snapshot_receipt::cvr_mbsn_arena_finalize(arena, list) } {
        material_abort(side);
        fail(2201);
    }
}
/// The source's binding plan readiness and its T5 view's owner (host).
#[repr(C)]
#[derive(Default)]
pub struct MaterialPlan {
    pub ready: bool,
    pub t5_view: bool,
    pub t5_view_owner: u64,
}

// ---- the IA snapshots

/// ia_snapshot::Failure.
const IA_EMPTY: u64 = 2;
const IA_ARGUMENTS: u64 = 3;
const IA_CURRENT: u64 = 7;
const IA_CAPACITY: u64 = 23;
const IA_EXCEPTION: u64 = 31;
const IA_MAX_VIEWS: usize = 17;
/// ia_snapshot::detail::MaxArenaBytes.
const IA_ARENA_BYTES: u64 = 64 * 1024 * 1024;
const IA_REUSE: u32 = 38;
const IA_TRANSFERS: u32 = 39;
const IA_OBSERVATIONS: u32 = 40;
const IA_SELECTIONS: u32 = 22;
/// geometry::DrawResources' prefix: the index and vertex leases and the draw record.
#[repr(C)]
pub struct DrawResourcesPrefix {
    pub index: crate::geometry_leases::Lease,
    pub vertices: [crate::geometry_leases::Lease; 16],
    pub draw: DrawRecord,
}
const _: () = assert!(core::mem::offset_of!(DrawResourcesPrefix, draw) == 1360);
fn ia_arena(side: u32) -> *mut c_void {
    eye_rust(side).ia_arena.as_ref().map_or(null_mut(), |a| a.as_raw())
}
fn ia_abort(side: u32) {
    let arena = ia_arena(side);
    if !arena.is_null() {
        // SAFETY: the arena's live storage.
        unsafe { crate::ia_snapshot_receipt::cvr_iasn_arena_abort(arena) };
    }
}
fn ia_finalized(side: u32) -> bool {
    let arena = ia_arena(side);
    // SAFETY: as above.
    !arena.is_null() && unsafe { crate::ia_snapshot_receipt::cvr_iasn_arena_finalized(arena) }
}
fn ia_bytes(side: u32) -> u64 {
    let arena = ia_arena(side);
    // SAFETY: as above.
    if arena.is_null() { 0 } else { unsafe { crate::ia_snapshot_receipt::cvr_iasn_arena_bytes(arena) } }
}
/// The prepared and recorded IA copies (the C++ try block); Err: what the C++ code threw.
#[allow(clippy::too_many_arguments)]
fn ia_copy(
    list: *mut c_void,
    side: u32,
    source: &mut SourceScalars,
    source_row: &[u64; 48],
    indexed: bool,
    copied_mask: u32,
    epoch: &crate::ia_snapshot_receipt::Epoch,
    command: &mut [u64; 16],
    observations: &mut crate::ia_snapshot_receipt::Observations,
    error: &mut u64,
) -> Result<(), ()> {
    let w = work().expect("capturing");
    let eye = w.eye(side as usize);
    let geometry = (&source.geometry as *const crate::geometry_leases::DrawResources).cast::<c_void>();
    // SAFETY: the source's geometry (its head in the C++ layout), the eye's lease and the source's parent.
    command[2] = u64::from(unsafe {
        crate::ia_snapshot_receipt::cvr_iasn_observe(list, geometry, indexed, (&eye.recording as *const Recording).cast(), (&source.parent as *const GraphParentRecord).cast(), epoch, observations)
    });
    *error = crate::ia_snapshot_receipt::cvr_iasn_last_failure();
    if observations.count as usize > IA_MAX_VIEWS {
        return Err(()); // "IA observation count exceeds its bounded set"
    }
    if command[2] == 0 {
        if *error == 0 {
            *error = IA_ARGUMENTS;
        }
    } else if source_row[44] != 0 || w.mode != 1 {
        *error = IA_CURRENT;
    } else if eye.ia_aborted {
        *error = IA_CHANGED;
    } else if observations.count == 0 {
        *error = IA_EMPTY;
    } else if observations.admitted == observations.count && observations.first == 0 {
        if eye_rust(side).ia_arena.is_none() {
            let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
            let mut made = null_mut();
            // SAFETY: a live device and list; the storage handed over.
            let hr = unsafe { crate::ia_snapshot_receipt::cvr_iasn_arena_create(null_mut(), device, list, eye.recording.generation, IA_ARENA_BYTES, &mut made) };
            if hr.is_err() || made.is_null() {
                *error = IA_CAPACITY;
            } else {
                // SAFETY: as above.
                eye_rust(side).ia_arena = Some(unsafe { IUnknown::from_raw(made) });
            }
        }
        if *error == 0 {
            // SAFETY: the observations are plain data.
            let initial = unsafe { core::ptr::read(observations) };
            // The source's geometry, the eye's lease, the parent, the epoch, the arena and the source's proof.
            let parent = source.parent;
            command[5] = u64::from(crate::ia_snapshot_receipt::prepare_owned(
                list,
                &source.geometry,
                indexed,
                // SAFETY: the eye's lease and the source's parent in their C++ layouts.
                unsafe { &*(&eye.recording as *const Recording).cast() },
                // SAFETY: as above.
                unsafe { &*(&parent as *const GraphParentRecord).cast() },
                epoch,
                ia_arena(side),
                &mut source.ia_prepared,
                observations,
            ));
            *error = crate::ia_snapshot_receipt::cvr_iasn_last_failure();
            if command[5] == 0 && observations.count == 0 {
                *observations = initial;
            }
            if observations.count as usize > IA_MAX_VIEWS {
                return Err(()); // "Prepared IA observation count exceeds its bounded set"
            }
            if command[5] != 0 {
                command[13] = source.ia_prepared.owner as u64;
                // SAFETY: plain views, written whole by the record (their references then moved into the source).
                let mut views: [crate::ia_snapshot_receipt::PrivateIa; IA_MAX_VIEWS] = unsafe { core::mem::zeroed() };
                let mut count = 0u32;
                // SAFETY: the proof and room for every view.
                let recorded = unsafe { crate::ia_snapshot_receipt::cvr_iasn_record(list, &source.ia_prepared, source.ia_views.as_slice().is_empty(), views.as_mut_ptr(), &mut count) };
                if recorded {
                    // The views' references move into the source's vector (the rest stay empty).
                    source.ia_views = views.into_iter().take((count as usize).min(IA_MAX_VIEWS)).collect();
                }
                if !recorded {
                    *error = crate::ia_snapshot_receipt::cvr_iasn_last_failure();
                    eye.ia_aborted = true;
                    ia_abort(side);
                    fail(1700 + *error as u32);
                } else {
                    let recorded_views = source.ia_views.as_slice();
                    command[6] = 1;
                    command[8] = recorded_views.len() as u64;
                    command[9] = source.ia_prepared.copied_bytes;
                    if recorded_views.len() != observations.count as usize || recorded_views.len() != source.ia_prepared.count as usize {
                        return Err(()); // "Recorded IA set differs from its complete proof"
                    }
                    let lent: Vec<crate::retained_ia_views::IaView> = recorded_views
                        .iter()
                        .map(|view| crate::retained_ia_views::IaView {
                            role: view.role as u32,
                            slot: view.slot,
                            resource: view.resource,
                            owner: !view.owner.is_null(),
                            source_offset: view.source_offset,
                            private_offset: view.private_offset,
                            copied: view.copied_bytes,
                            reserved: view.reserved_bytes,
                            origin: view.origin_occurrence,
                            reused: view.reused,
                            index: view.index,
                            vertex: view.vertex,
                        })
                        .collect();
                    let mut leases = vec![crate::retained_ia_views::Lease { gpu: 0, width: 0, flags: 0, _pad: 0 }; lent.len()];
                    let mut view_rows = vec![[0u64; 16]; lent.len()];
                    let mut reuse_rows = vec![[0u64; 8]; lent.len()];
                    // The geometry's draw record copied out (the replacement's), then the views' rows.
                    let mut draw = source.geometry.draw;
                    // SAFETY: `lent.len()` views, leases and rows.
                    let refused = unsafe {
                        crate::retained_ia_views::cvr_nrg_ia_views(
                            lent.as_ptr(),
                            lent.len(),
                            indexed,
                            copied_mask,
                            &mut draw,
                            leases.as_mut_ptr(),
                            source_row[0],
                            u64::from(side),
                            view_rows.as_mut_ptr(),
                            reuse_rows.as_mut_ptr(),
                        )
                    };
                    if refused != 0 {
                        return Err(()); // the refusal's message
                    }
                    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
                    // SAFETY: the work's journal.
                    let journal = unsafe { &mut *journal };
                    for n in 0..lent.len() {
                        journal.append(IA_VIEWS, &view_rows[n]);
                        journal.append(IA_REUSE, &reuse_rows[n]);
                    }
                    replace_ia(source, &draw, &leases);
                }
            }
        }
    }
    Ok(())
}
/// The source's geometry replaced: its draw record and each IA view's private lease (a DEFAULT-heap buffer).
fn replace_ia(source: &mut SourceScalars, draw: &DrawRecord, leases: &[crate::retained_ia_views::Lease]) {
    let mut replacement = source.geometry.clone();
    replacement.draw = *draw;
    for (view, lease) in source.ia_views.iter().zip(leases) {
        // SAFETY: the private view's live resource (or null), referenced for the lease.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&view.resource) }.cloned();
        let owned = crate::geometry_leases::BufferLease {
            resource,
            gpu: lease.gpu,
            bytes: lease.width,
            heap_type: D3D12_HEAP_TYPE_DEFAULT.0 as u32,
            flags: lease.flags,
            native: Default::default(),
        };
        if view.role == IA_ROLE_INDEX {
            replacement.index = owned;
        } else if let Some(slot) = replacement.vertices.get_mut(view.slot as usize) {
            *slot = owned;
        }
    }
    source.geometry = replacement;
}
/// ia_snapshot::Role::Index.
const IA_ROLE_INDEX: u64 = 0;
/// CaptureIaSnapshots: the draw's index and vertex buffers observed, copied and rebound at the draw.
fn capture_ia_snapshots(list: *mut c_void, side: u32, source: &mut SourceScalars, source_row: &[u64; 48], indexed: bool) {
    let w = work().expect("capturing");
    if !w.source_ia_snapshots {
        return;
    }
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let eye = w.eye(side as usize);
    let mut command = [0u64; 16];
    command[0] = source_row[0];
    command[1] = u64::from(side);
    let gpu_rows = rows_mut::<24>(w, INSTANCE_GPU_ROWS);
    let separate = if gpu_rows.get(source_row[0] as usize).is_some_and(|row| row[2] != 0) { 128u32 } else { 0 };
    let copied_mask = source.geometry.draw.input_slot_mask & !separate;
    // SAFETY: a writable command and the source row.
    unsafe { crate::retained_source_rows::cvr_nrg_ia_command_head(&mut command, source_row, u64::from(side)) };
    // SAFETY: plain observations (the C++ value-initialised set).
    let mut observations: crate::ia_snapshot_receipt::Observations = unsafe { core::mem::zeroed() };
    let mut error = 0u64;
    for (kind, extra) in [(IA_COMMANDS, 1usize), (IA_OBSERVATIONS, IA_MAX_VIEWS), (IA_VIEWS, IA_MAX_VIEWS), (IA_REUSE, IA_MAX_VIEWS), (IA_TRANSFERS, 1)] {
        journal.reserve(kind, journal.count(kind) + extra);
    }
    let mut stamp = crate::indirect_copy::ScopeStamp::default();
    // SAFETY: the scope's stamp, exited below.
    unsafe { crate::ia_snapshot_receipt::cvr_iasn_scope_enter(list as u64, eye.recording.generation, command[12], &mut stamp) };
    let epoch = crate::ia_snapshot_receipt::Epoch { list: source_row[6], generation: source_row[7], occurrence: command[12], certified: true, separate_vertices: separate };
    let copied = ia_copy(list, side, source, source_row, indexed, copied_mask, &epoch, &mut command, &mut observations, &mut error);
    // SAFETY: the stamp entered above.
    unsafe { crate::ia_snapshot_receipt::cvr_iasn_scope_exit(&stamp) };
    if copied.is_err() {
        eye.ia_aborted = true;
        ia_abort(side);
        fail(1731);
        error = IA_EXCEPTION;
        let views = source.ia_views.as_slice();
        command[5] = u64::from(!source.ia_prepared.owner.is_null());
        command[6] = u64::from(!views.is_empty());
        command[8] = views.len() as u64;
        command[9] = source.ia_prepared.copied_bytes;
        command[13] = source.ia_prepared.owner as u64;
    }
    command[3] = u64::from(observations.count);
    command[4] = u64::from(observations.admitted);
    command[7] = u64::from(error as u32);
    for index in 0..(observations.count as usize).min(IA_MAX_VIEWS) {
        let mut row = [0u64; 67];
        row[0] = source_row[0];
        row[1] = u64::from(side);
        row[2] = index as u64;
        // SAFETY: an observation is 64 plain words.
        let words = unsafe { core::slice::from_raw_parts((&observations.rows[index] as *const crate::ia_snapshot::Observation).cast::<u64>(), 64) };
        row[3..].copy_from_slice(words);
        journal.append(IA_OBSERVATIONS, &row);
    }
    journal.append(IA_COMMANDS, &command);
    let moved = command[6] != 0;
    journal.append(
        IA_TRANSFERS,
        &[
            source_row[0],
            u64::from(side),
            if moved { source.ia_prepared.transferred_bytes } else { 0 },
            if moved { source.ia_prepared.reused_bytes } else { 0 },
            ia_bytes(side),
        ],
    );
    journal.append(IA_SELECTIONS, &[source_row[0], u64::from(side), u64::from(separate), u64::from(source.geometry.draw.input_slot_mask), u64::from(copied_mask)]);
}
/// Seal's IA stage.
fn seal_ia(w: &mut WorkScalars, list: *mut c_void, side: u32) {
    let value = w.eye(side as usize);
    let arena = ia_arena(side);
    if !w.source_ia_snapshots || arena.is_null() || ia_bytes(side) == 0 || value.ia_aborted {
        return;
    }
    let s = u64::from(side);
    // SAFETY: the arena's live storage and the sealed list.
    if !unsafe { crate::ia_snapshot_receipt::cvr_iasn_arena_finalize(arena, list) } {
        let error = crate::ia_snapshot_receipt::cvr_iasn_last_failure();
        value.ia_aborted = true;
        ia_abort(side);
        fail(1740 + error as u32);
        for command in rows_mut::<16>(w, IA_COMMANDS) {
            if command[1] == s && command[6] != 0 && command[7] == 0 {
                command[7] = u64::from(error as u32);
            }
        }
    } else {
        for command in rows_mut::<16>(w, IA_COMMANDS) {
            if command[1] == s && command[6] != 0 && command[7] == 0 {
                command[14] = 1;
            }
        }
        for row in rows_mut::<16>(w, IA_VIEWS) {
            if row[1] == s {
                row[13] = 1;
            }
        }
    }
}

// ---- the native tables

const NATIVE_TABLE_ENTRIES: u32 = 13;
const NATIVE_TABLE_RINGS: u32 = 14;
/// BindNativeSourceTables: the last source's tables placed in the camera heap and bound (Draw, Indirect).
fn bind_native_source_tables(list: *mut c_void) -> bool {
    use crate::native_descriptor_arena as arena;
    let Some(w) = work() else { return false };
    let count = source_count();
    if !w.source_stream || !w.source_tables || w.mode != 1 || count == 0 {
        return false;
    }
    let ordinal = count - 1;
    let Some(source) = source(ordinal as u64) else { return false };
    let Some(&saved) = rows_mut::<48>(w, SOURCE_ROWS).get(ordinal) else { return false };
    let mut row = [0u64; 16];
    row[0] = ordinal as u64;
    row[1] = saved[1];
    row[11] = saved[6];
    row[12] = saved[7];
    let side = (saved[1] as usize).min(1);
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let mut bound = false;
    match arena::copy_tables(list, &source.bindings, sources::tables_of(ordinal), ordinal as u64, &mut rust_side().eyes[side].descriptor_cache) {
        Some(native) => {
            row[2] = if native.ring[2] != 0 { 2 } else { 1 };
            row[4] = u64::from(native.descriptors);
            row[5] = native.bindings.len() as u64;
            row[6] = native.heap;
            row[7] = native.base;
            row[8] = native.capacity;
            row[9] = u64::from(native.range.before);
            row[10] = u64::from(native.range.after);
            row[13] = native.borrowed_mask;
            bound = arena::bind(list, &native);
            row[14] = u64::from(bound);
            if !bound {
                fail(2300);
            }
            for entry in &native.entries {
                journal.append(NATIVE_TABLE_ENTRIES, entry);
            }
            if native.ring[2] != 0 {
                journal.append(NATIVE_TABLE_RINGS, &native.ring);
            }
            rust_side().native_tables.insert(ordinal, native);
        }
        None => {
            rust_side().native_tables.remove(&ordinal);
            row[3] = u64::from(arena::last_failure());
            if row[3] != u64::from(arena::failure::CAPACITY) && row[3] != u64::from(arena::failure::STATE) {
                fail(2300 + row[3] as u32);
            }
        }
    }
    journal.append(NATIVE_TABLE_ROWS, &row);
    publish_sources();
    bound
}

// ---- the native instance capture

const INSTANCE_OVERLAY_ROWS: u32 = 21;
const INSTANCE_BLOB: u32 = 0;
const INSTANCE_GPU_BLOB: u32 = 1;
const INSTANCE_OVERLAY_BLOB: u32 = 2;
/// The eye's instance readback and the sources' CPU blob, each at most 16 MiB.
const INSTANCE_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
/// geometry::DrawResources' head as the C++ layout lends it (the source's own DrawResources begins with it).
#[repr(C)]
pub struct DrawResourcesFull {
    pub prefix: DrawResourcesPrefix,
}
/// A source's instance capture on this side (the C++ Source's instanceCandidate and instanceView).
#[derive(Default)]
struct SourceInstance {
    candidate: Option<crate::instance_observer_native::Candidate>,
    view: Option<NativeView>,
}
/// A native capture's instance_snapshot_view::PrivateView: the arena owner keeps what `out` points at.
struct NativeView {
    owner: IUnknown,
    out: crate::instance_snapshot_native::ViewOut,
    shader: *const c_void,
}
fn raw_lease(recording: &Recording) -> crate::native_recording::Lease {
    crate::native_recording::Lease {
        token: recording.token.as_ref().map_or(null_mut(), |t| t.as_raw()),
        generation: recording.generation,
        context: recording.context,
        list: recording.list,
        registry: recording.registry,
    }
}
fn private_view(view: &NativeView) -> crate::instance_snapshot_arena::PrivateView {
    let o = &view.out;
    crate::instance_snapshot_arena::PrivateView {
        scope: o.scope,
        resource: o.resource,
        owner: view.owner.as_raw(),
        pipeline: o.pipeline,
        shader: [0; 2],
        vertex: o.vertex,
        original: o.original,
        draw: o.draw,
        mapping: o.mapping,
        copy_list: o.copy_list,
        copy_generation: o.copy_generation,
        version: o.version,
    }
}
/// instance_snapshot_view::NativeCompletion of the eye.
fn native_completion(eye: &EyeScalars, certified: bool) -> crate::instance_snapshot_arena::NativeCompletion {
    crate::instance_snapshot_arena::NativeCompletion {
        recording: raw_lease(&eye.recording),
        queue: eye.source_queue.as_ref().map_or(null_mut(), |q| q.as_raw()),
        fence: eye.fence.as_ref().map_or(null_mut(), |f| f.as_raw()),
        value: eye.signaled_value,
        certified,
    }
}
/// ValidateNativeViewForRecording.
fn view_for_recording(view: &NativeView, list: *mut c_void, recording: &Recording) -> bool {
    let lease = raw_lease(recording);
    crate::instance_snapshot_native::native_view_check(1, &private_view(view), view.shader, list, &lease, core::ptr::null())
}
/// ValidateNativeViewAfterSubmission (2) / AfterCompletion (3).
fn view_after(view: &NativeView, kind: u32, completion: &crate::instance_snapshot_arena::NativeCompletion) -> bool {
    crate::instance_snapshot_native::native_view_check(kind, &private_view(view), view.shader, null_mut(), core::ptr::null(), completion)
}
fn instance_view(index: usize) -> Option<&'static NativeView> {
    rust_side().instances.get(&index).and_then(|i| i.view.as_ref())
}
fn geometry_of(source: *const SourceScalars) -> &'static DrawResourcesFull {
    // SAFETY: the source's own DrawResources (its head in the C++ layout), alive with the source.
    unsafe { &*(&(*source).geometry as *const crate::geometry_leases::DrawResources).cast::<DrawResourcesFull>() }
}
/// The source's draw state's pipeline (lent).
fn geometry_pipeline(source: *const SourceScalars) -> *mut c_void {
    // SAFETY: a live source.
    unsafe { &(*source).geometry.state.original }.as_ref().map_or(null_mut(), |p| p.as_raw())
}
fn d3d_view(view: &crate::instance_snapshot_view::VertexBufferView) -> windows::Win32::Graphics::Direct3D12::D3D12_VERTEX_BUFFER_VIEW {
    windows::Win32::Graphics::Direct3D12::D3D12_VERTEX_BUFFER_VIEW { BufferLocation: view.buffer_location, SizeInBytes: view.size_in_bytes, StrideInBytes: view.stride_in_bytes }
}
/// A new readback/upload buffer (cvr_rb_buffer), `like` sizing it after an existing one when given.
fn instance_buffer(device: *mut c_void, like: *mut c_void, bytes: u64, heap: i32, state: i32) -> Option<ID3D12Resource> {
    let mut out = null_mut();
    // SAFETY: a live device; a writable slot.
    let created = unsafe { crate::readback_buffers::cvr_rb_buffer(device, like, bytes, heap, state, &mut out) };
    // SAFETY: the new buffer's one reference.
    (created.is_ok() && !out.is_null()).then(|| unsafe { ID3D12Resource::from_raw(out) })
}
const HEAP_UPLOAD: i32 = 2;
const HEAP_READBACK: i32 = 3;
const STATE_COPY_DEST: i32 = 0x400;
const STATE_GENERIC_READ: i32 = 0xAC3;
/// CaptureInstanceWriter: at the source's actual draw, the observer's CPU candidate of the instance range it reads,
/// and the original range copied into the eye's native capture arena under the source's predicate. Err is the .cpp's
/// throw (AppendSource drops the source).
fn capture_instance_writer(list: *mut c_void, side: u32, source: *mut SourceScalars, source_row: &[u64; 48]) -> Result<(), ()> {
    use crate::instance_observer_native as observer;
    use crate::instance_snapshot_native as snapshot;
    let Some(w) = work() else { return Ok(()) };
    if !w.source_ia_snapshots {
        return Ok(());
    }
    let ordinal = source_row[0] as usize;
    rust_side().instances.remove(&ordinal);
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let geometry = geometry_of(source);
    let draw = &geometry.prefix.draw;
    let view = draw.vertices[7];
    let held = &geometry.prefix.vertices[7];
    let lent = crate::instance_snapshot_view::VertexBufferView { buffer_location: view.BufferLocation, size_in_bytes: view.SizeInBytes, stride_in_bytes: view.StrideInBytes };
    let mut row = [0u64; 48];
    // SAFETY: the rows and the view.
    unsafe { crate::retained_source_rows::cvr_nrg_instance_head(&mut row, source_row, u64::from(side), held.resource as u64, held.gpu, &lent) };
    // SAFETY: a live source (its shader snapshot shared_ptr).
    let shader = unsafe { (*source).shader_shared() };
    let eye = w.eye(side as usize);
    let mut candidate = None;
    if source_row[2] == 0 && source_row[44] == 0 && draw.input_slot_mask & 128 != 0 && !snapshot::shared_get(shader).is_null() && snapshot::shared_layout(shader, view.StrideInBytes) {
        row[3] = 1;
        // SAFETY: the source lent by the host.
        let parent = unsafe { &(*source).parent };
        let found = observer::query(list, draw, held, geometry_pipeline(source), true, &raw_lease(&eye.recording), parent, source_row[0] + 1);
        row[16..48].copy_from_slice(&observer::last_query());
        if let Some(found) = found.as_ref().filter(|c| c.cpu.is_some()) {
            let bytes = found.cpu.bytes();
            let blob = journal.blob(INSTANCE_BLOB).len();
            if bytes.is_empty() || bytes.len() > (INSTANCE_CAPTURE_BYTES as usize).wrapping_sub(blob) {
                return Err(());
            }
            row[4] = 1;
            row[5] = found.owner.as_raw() as u64;
            row[6] = blob as u64 + 1;
            row[7] = bytes.len() as u64;
            journal.append_blob(INSTANCE_BLOB, bytes);
        }
        candidate = found;
    }
    let mut probe_row = [0u64; 24];
    probe_row[0] = source_row[0];
    probe_row[1] = u64::from(side);
    if row[4] != 0 {
        let er = eye_rust(side);
        let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
        CyberpunkVR_RetainedInstanceGpuPhase.store(1, SeqCst);
        if er.instance_readback.is_none() {
            let previous = cvr_host::probe::set_internal(true);
            let made = (|| {
                er.instance_readback = Some(instance_buffer(device, null_mut(), INSTANCE_CAPTURE_BYTES, HEAP_READBACK, STATE_COPY_DEST)?);
                CyberpunkVR_RetainedInstanceGpuPhase.store(2, SeqCst);
                er.instance_predicate = Some(instance_buffer(device, null_mut(), 2048 * 8, HEAP_UPLOAD, STATE_GENERIC_READ)?);
                CyberpunkVR_RetainedInstanceGpuPhase.store(3, SeqCst);
                let predicate = er.instance_predicate.as_ref().map_or(null_mut(), |p| p.as_raw());
                // SAFETY: the new upload buffer.
                unsafe { crate::readback_buffers::cvr_rb_zero(predicate, 2048 * 8) }.then_some(())
            })();
            cvr_host::probe::set_internal(previous);
            made.ok_or(())?;
        }
        let offset = eye.instance_readback_bytes;
        let predicate_offset = source_row[0] * 8;
        CyberpunkVR_RetainedInstanceGpuPhase.store(4, SeqCst);
        if row[7] > INSTANCE_CAPTURE_BYTES.wrapping_sub(offset) {
            return Err(());
        }
        if er.instance_capture.is_none() {
            er.instance_capture = Some(snapshot::arena_create(device, &raw_lease(&eye.recording), side, crate::instance_snapshot_view::MAX_BYTES / 2).ok());
            if matches!(er.instance_capture, Some(None)) {
                return Err(());
            }
        }
        let input = snapshot::InputView {
            scope: crate::instance_snapshot_view::Scope { root: source_row[11], list: source_row[6], generation: source_row[7], occurrence: source_row[0] + 1, eye: side },
            pipeline: geometry_pipeline(source),
            shader,
            original: lent,
            draw: crate::instance_snapshot_arena::DrawArgs { index_count: draw.indices, instance_count: draw.instances, start_index: draw.first_index, base_vertex: draw.base_vertex, start_instance: draw.first_instance },
            bytes: core::ptr::null(),
            size: 0,
            has_bytes: false,
        };
        let readback = er.instance_readback.as_ref().map_or(null_mut(), |r| r.as_raw());
        let predicate = er.instance_predicate.as_ref().map_or(null_mut(), |p| p.as_raw());
        let arena = er.instance_capture.clone().flatten();
        let captured = match (&arena, candidate.as_ref()) {
            (Some(arena), Some(found)) => snapshot::arena_capture(arena, list, &input, &mut |target, at| {
                let actual = observer::InputView {
                    root: input.scope.root,
                    list: input.scope.list,
                    generation: input.scope.generation,
                    occurrence: input.scope.occurrence,
                    eye: input.scope.eye,
                    bytes: input.has_bytes,
                    pipeline: input.pipeline,
                    shader: snapshot::shared_get(input.shader),
                    original: d3d_view(&input.original),
                    draw: windows::Win32::Graphics::Direct3D12::D3D12_DRAW_INDEXED_ARGUMENTS {
                        IndexCountPerInstance: input.draw.index_count,
                        InstanceCount: input.draw.instance_count,
                        StartIndexLocation: input.draw.start_index,
                        BaseVertexLocation: input.draw.base_vertex,
                        StartInstanceLocation: input.draw.start_instance,
                    },
                };
                observer::record_capture(list, found, &actual, target, at, readback, offset, predicate, predicate_offset)
            }),
            _ => None,
        };
        let (Some(out), Some(arena)) = (captured, arena) else { return Err(()) };
        let shader = snapshot::shared_get(out.shader);
        rust_side().instances.entry(ordinal).or_default().view = Some(NativeView { owner: arena, out, shader });
        // SAFETY: the rows.
        unsafe { crate::retained_source_rows::cvr_nrg_instance_probe(&mut probe_row, &row, source_row, readback as u64, predicate as u64, offset, predicate_offset) };
        eye.instance_readback_bytes += row[7];
        CyberpunkVR_RetainedInstanceGpuPhase.store(5, SeqCst);
    }
    if candidate.is_some() {
        rust_side().instances.entry(ordinal).or_default().candidate = candidate;
    }
    journal.append(INSTANCE_ROWS, &row);
    journal.append(INSTANCE_GPU_ROWS, &probe_row);
    Ok(())
}
/// The .cpp's AppendSource: false is its throw.
///
/// # Safety
/// A live list; the .cpp's pending source and its row.
#[no_mangle]
pub unsafe extern "C" fn cvr_nrg2_capture_instance(list: *mut c_void, side: u32, source: *mut SourceScalars, row: *const [u64; 48]) -> bool {
    // SAFETY: as documented.
    let row = unsafe { &*row };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture_instance_writer(list, side, source, row).is_ok())).unwrap_or(false)
}
/// PrepareInstanceOverlay: after both submissions completed, each native instance copy checked against the
/// original GPU bytes and joined to the overlay rows and blob.
fn prepare_instance_overlay() -> bool {
    let Some(w) = work() else { return true };
    if !w.source_ia_snapshots || w.instance_overlay_stage == 2 {
        return true;
    }
    if w.instance_overlay_stage == 3 {
        return false;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| instance_overlay(w))).unwrap_or(Err("Unknown instance overlay exception")) {
        Ok(()) => {
            w.instance_overlay_stage = 2;
            true
        }
        Err(message) => {
            let error = ex!(CyberpunkVR_RetainedInstanceOverlayError);
            let n = message.len().min(error.len() - 1);
            error[..n].copy_from_slice(&message.as_bytes()[..n]);
            error[n] = 0;
            w.instance_overlay_stage = 3;
            fail(1810);
            publish_sources();
            false
        }
    }
}
/// The eyes' private readbacks, mapped while the overlay reads them.
struct InstanceMaps([Option<(ID3D12Resource, *mut c_void)>; 2]);
impl Drop for InstanceMaps {
    fn drop(&mut self) {
        let none = windows::Win32::Graphics::Direct3D12::D3D12_RANGE { Begin: 0, End: 0 };
        for (resource, _) in self.0.iter().flatten() {
            // SAFETY: a buffer this value mapped.
            unsafe { resource.Unmap(0, Some(&none)) };
        }
    }
}
fn instance_overlay(w: &mut WorkScalars) -> Result<(), &'static str> {
    let mut maps = InstanceMaps([None, None]);
    for side in 0..2u32 {
        let er = eye_rust(side);
        if er.instance_capture.is_none() {
            continue;
        }
        let range = windows::Win32::Graphics::Direct3D12::D3D12_RANGE { Begin: 0, End: w.eye(side as usize).instance_readback_bytes as usize };
        let Some(readback) = er.instance_private_readback.clone() else { return Err("Native instance copy diagnostic readback failed") };
        let mut data = null_mut();
        // SAFETY: a live readback buffer.
        if unsafe { readback.Map(0, Some(&range), Some(&mut data)) }.is_err() || data.is_null() {
            return Err("Native instance copy diagnostic readback failed");
        }
        maps.0[side as usize] = Some((readback, data));
    }
    let journal: *mut crate::retained_journal::Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let gpu_blob = journal.blob(INSTANCE_GPU_BLOB);
    let (blob, blob_bytes) = (gpu_blob.as_ptr(), gpu_blob.len());
    let gpu = rows_mut::<24>(w, INSTANCE_GPU_ROWS);
    for (index, original) in gpu.iter().enumerate() {
        if original[5] != 1 {
            continue;
        }
        let side = original[1] as usize;
        let eye = w.eye(side);
        let data = maps.0.get(side).and_then(|m| m.as_ref()).map_or(null_mut(), |m| m.1);
        let completion = native_completion(eye, eye.submission_certified);
        let view = instance_view(index);
        // SAFETY: the row.
        let evidence = || unsafe { crate::retained_instances::cvr_nrg_instance_evidence(original, !data.is_null(), eye.instance_readback_bytes, blob_bytes as u64) };
        let Some(view) = view.filter(|v| view_after(v, 3, &completion) && evidence()) else {
            return Err("Native instance copy lacks original submission evidence");
        };
        let bytes = (data as *const u8).wrapping_add(original[10] as usize);
        // SAFETY: the mapped private copy and the journal's GPU blob.
        if !unsafe { crate::retained_instances::cvr_nrg_instance_payload(view.out.mapping.copy_bytes, original, bytes, blob, blob_bytes) } {
            return Err("Native private instance copy differs from original GPU bytes");
        }
        let o = &view.out;
        let required = eye_rust(side as u32).instance_capture.as_ref().and_then(|a| a.as_ref()).map_or(0, |a| crate::instance_snapshot_native::arena_count(a, 1));
        let overlay: [u64; 24] = [
            index as u64,
            u64::from(o.scope.eye),
            u64::from(o.version),
            o.scope.root,
            o.scope.list,
            o.scope.generation,
            o.scope.occurrence,
            o.pipeline as u64,
            crate::instance_snapshot_native::shared_id(o.shader),
            o.original.buffer_location,
            u64::from(o.original.size_in_bytes),
            u64::from(o.original.stride_in_bytes),
            o.resource as u64,
            view.owner.as_raw() as u64,
            o.vertex.buffer_location,
            o.mapping.target_offset,
            o.mapping.copy_bytes,
            required,
            1,
            1,
            1,
            original[18],
            o.copy_list,
            o.copy_generation,
        ];
        journal.append(INSTANCE_OVERLAY_ROWS, &overlay);
        // SAFETY: the mapped private copy, `original[11]` bytes checked by the payload rule.
        journal.append_blob(INSTANCE_OVERLAY_BLOB, unsafe { core::slice::from_raw_parts(bytes, original[11] as usize) });
    }
    Ok(())
}
/// The seal's instance copies: the eye's native capture arena finalized and each of its private copies of this eye's
/// sources read back after it.
fn seal_instances(w: &mut WorkScalars, list: *mut c_void, side: u32) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let er = eye_rust(side);
        let value = w.eye(side as usize);
        let Some(Some(arena)) = er.instance_capture.clone() else { return };
        if crate::instance_snapshot_native::arena_count(&arena, 0) == 0 {
            return;
        }
        if !crate::instance_snapshot_native::arena_finalize(&arena, list) || value.instance_readback_bytes == 0 {
            fail(1811);
            return;
        }
        let device = w.device.as_ref().map_or(null_mut(), |d| d.as_raw());
        let previous = cvr_host::probe::set_internal(true);
        er.instance_private_readback = instance_buffer(device, crate::instance_snapshot_native::arena_resource(&arena), value.instance_readback_bytes, HEAP_READBACK, STATE_COPY_DEST);
        cvr_host::probe::set_internal(previous);
        let Some(private) = er.instance_private_readback.clone() else {
            fail(1812);
            return;
        };
        let keep = [private.as_raw()];
        // SAFETY: the list and a live buffer.
        if !unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, keep.as_ptr(), keep.len()) } {
            fail(1812);
            return;
        }
        // SAFETY: the hook's live list.
        let Some(commands) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return };
        let gpu = rows_mut::<24>(w, INSTANCE_GPU_ROWS);
        let bytes = value.instance_readback_bytes;
        for index in 0..source_count() {
            let Some(view) = instance_view(index) else { continue };
            let Some(source) = source(index as u64) else { continue };
            if geometry_of(source).prefix.draw.side != side {
                continue;
            }
            if index >= gpu.len() || !view_for_recording(view, list, &value.recording) {
                fail(1813);
                break;
            }
            let row = gpu[index];
            if row[2] == 0 || row[10] > bytes || row[11] > bytes - row[10] || row[11] != view.out.mapping.copy_bytes {
                fail(1814);
                break;
            }
            // SAFETY: the view's buffer, kept by its arena owner.
            let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&view.out.resource) }) else { continue };
            let previous = cvr_host::probe::set_internal(true);
            // SAFETY: the validated private copy and the arena's buffer.
            unsafe { commands.CopyBufferRegion(&private, row[10], resource, view.out.mapping.target_offset, view.out.mapping.copy_bytes) };
            cvr_host::probe::set_internal(previous);
        }
    }));
}
/// The boundary's counts of this eye's sources: with owned tables, with a native instance view usable on `list`.
fn boundary_counts(list: *mut c_void, side: u32) -> [u64; 2] {
    let Some(w) = work() else { return [0; 2] };
    let value = w.eye(side as usize);
    let mut counted = [0u64; 2];
    for index in 0..source_count() {
        let Some(source) = source(index as u64) else { continue };
        if geometry_of(source).prefix.draw.side != side {
            continue;
        }
        counted[0] += u64::from(!sources::tables_of(index).is_empty());
        counted[1] += u64::from(instance_view(index).is_some_and(|view| view_for_recording(view, list, &value.recording)));
    }
    counted
}
/// A source's native instance view for the .cpp: kind 0 after its eye completed (BuildPrivateGeometry); kind 1 after the
/// eye's certified submission, or before it for the recording on `list` (StagePairSource).
#[repr(C)]
pub struct InstanceViewOut {
    pub scope: crate::instance_snapshot_view::Scope,
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub vertex: crate::instance_snapshot_view::VertexBufferView,
    pub draw: crate::instance_snapshot_arena::DrawArgs,
}
const _: () = assert!(core::mem::size_of::<InstanceViewOut>() == 96);
/// # Safety
/// `out` writable; called under the capture's lock.
#[no_mangle]
pub unsafe extern "C" fn cvr_nrg2_instance_view(index: usize, kind: u32, side: u32, list: *mut c_void, out: *mut InstanceViewOut) -> bool {
    let Some(w) = work() else { return false };
    let Some(view) = instance_view(index) else { return false };
    let eye = w.eye(side.min(1) as usize);
    let ready = match kind {
        0 => view_after(view, 3, &native_completion(eye, eye.submission_certified)),
        _ if eye.submitted => view_after(view, 2, &native_completion(eye, eye.submitted && eye.submission_certified)),
        _ => view_for_recording(view, list, &eye.recording),
    };
    if ready {
        let o = &view.out;
        // SAFETY: as documented.
        unsafe { *out = InstanceViewOut { scope: o.scope, resource: o.resource, owner: view.owner.as_raw(), vertex: o.vertex, draw: o.draw } };
    }
    ready
}

// ---- the frame boundary

fn config_failure(w: &WorkScalars, request: u32, phase: u32) -> u32 {
    let config = crate::retained_config::Config {
        request,
        stage: w.stage,
        scene_access: CyberpunkVR_SceneAccessEnable.load(SeqCst),
        scene_split: scene_split().load(SeqCst),
        current: w.current_enabled,
        infix: w.infix_enabled,
        tail: w.tail_enabled,
        full_t5: w.full_t5_enabled,
        borrowed_tables: w.borrowed_tables_enabled,
        fixed_assets: w.fixed_assets_enabled,
        vrs_refinement: w.allow_vrs_refinement,
        selectors: w.source_selectors,
        ia_snapshots: w.source_ia_snapshots,
        tables: w.source_tables,
        stream: w.source_stream,
        cbv_snapshots: w.source_cbv_snapshots,
        cbvs: w.source_cbvs,
        details: w.source_details,
        snapshots: w.source_snapshots,
    };
    crate::retained_config::config_failure(&config, phase)
}
/// The pending work's switches; Err when FrameBoundary's C++ try block would have thrown.
fn configure(p: &mut WorkScalars, request: u32) -> Result<(), ()> {
    p.mode = request;
    p.stage = CyberpunkVR_RetainedGeometryStage.load(SeqCst);
    p.source_stream = CyberpunkVR_RetainedSourceStreamEnable.load(SeqCst) != 0;
    p.source_snapshots = CyberpunkVR_RetainedIndirectSnapshotsEnable.load(SeqCst) != 0;
    p.source_cbvs = CyberpunkVR_RetainedSourceCbvEnable.load(SeqCst) != 0;
    p.source_tables = CyberpunkVR_RetainedSourceTablesEnable.load(SeqCst) != 0;
    p.source_details = CyberpunkVR_RetainedSourceDetailsEnable.load(SeqCst) != 0;
    p.source_cbv_snapshots = CyberpunkVR_RetainedSourceCbvSnapshotsEnable.load(SeqCst) != 0;
    p.source_ia_snapshots = crate::ia_snapshot_receipt::CyberpunkVR_NativeIaSnapshotsEnable.load(SeqCst) != 0;
    latch_frontier_observer(p);
    p.scene_access_observer = CyberpunkVR_SceneAccessEnable.load(SeqCst) == 1;
    p.infix_enabled = CyberpunkVR_PreparedInfixEnable.load(SeqCst) != 0;
    p.tail_enabled = CyberpunkVR_PreparedTailEnable.load(SeqCst) != 0;
    p.full_t5_enabled = CyberpunkVR_RetainedFullT5Enable.load(SeqCst) != 0;
    p.borrowed_tables_enabled = CyberpunkVR_RetainedBorrowedTablesEnable.load(SeqCst) != 0;
    p.fixed_assets_enabled = CyberpunkVR_RetainedFixedAssetsEnable.load(SeqCst) != 0;
    p.allow_vrs_refinement = CyberpunkVR_PreparedVrsRefinementEnable.load(SeqCst) != 0;
    if config_failure(p, request, 0) != 0 {
        return Err(()); // "Retained capture switches are inconsistent"
    }
    p.source_selectors = p.source_cbv_snapshots && crate::registered_copies::entries::cvr_rc2_intent_configured();
    if config_failure(p, request, 1) != 0 {
        return Err(()); // "Current prepared sequence requires complete source-copy capture"
    }
    if p.source_selectors && !work::work_prepare(p, 0) {
        return Err(()); // "Source selector catalog is invalid"
    }
    if config_failure(p, request, 2) != 0 {
        return Err(()); // "Source capture requires its owned inputs and the original reference stream"
    }
    if !work::work_prepare(p, 1) {
        return Err(());
    }
    Ok(())
}
fn frame_boundary() {
    crate::prepared_pair_native::cvr_npp2_poll();
    if !active() && CyberpunkVR_RetainedGeometryState.load(SeqCst) != 2 {
        return;
    }
    let _guard = lock();
    frame_boundary_locked();
}
fn frame_boundary_locked() {
    if work().is_some_and(|w| w.infix_in_flight) {
        return;
    }
    if let Some(w) = work() {
        if WATCHING.load(SeqCst) && CyberpunkVR_RetainedGeometryState.load(SeqCst) == 0 && !w.eyes().iter().any(|eye| eye.started && !eye.sealed) {
            w.complete = true;
            WATCHING.store(false, SeqCst);
            finish_targets();
        }
    }
    poll();
    if work().is_some_and(|w| w.complete) {
        release_borrowed();
    }
    if let Some(w) = work() {
        disarm_finished_frontier_observer(w);
    }
    let pair_request = CyberpunkVR_PreparedPairRequest.swap(0, SeqCst);
    if pair_request != 0 && !npp_active() {
        pairs::pair_request(pair_request);
    }
    if npp_active() {
        return;
    }
    let request = CyberpunkVR_RetainedGeometryRequest.swap(0, SeqCst);
    if request == 0 {
        return;
    }
    if work().is_some_and(|w| !w.complete && (w.eye(0).started || w.eye(1).started)) {
        fail(1);
        return;
    }
    let pending = work::work_new();
    // SAFETY: a new Work or null.
    let configured = match unsafe { pending.as_mut() } {
        Some(p) => configure(p, request).is_ok(),
        None => false,
    };
    if !configured {
        if !pending.is_null() {
            work::work_free(pending);
        }
        CyberpunkVR_SubmissionFrontierEnable.store(0, SeqCst);
        CyberpunkVR_RetainedGeometryState.store(4, SeqCst);
        let stream = ex!(CyberpunkVR_RetainedSourceStreamStats);
        stream[0] = 4;
        stream[12] = 906;
        WATCHING.store(false, SeqCst);
        return;
    }
    {
        // SAFETY: as above.
        let p = unsafe { &*pending };
        crate::retained_exports::cvr_nrg_reset_exports(
            p.full_t5_enabled,
            p.borrowed_tables_enabled,
            p.fixed_assets_enabled,
            p.current_enabled,
            p.infix_enabled,
            p.tail_enabled,
        );
    }
    // The pending Work becomes the installed one.
    work::work_install(pending);
    state().rust = Some(Box::default());
    let w = work().expect("installed");
    ex!(CyberpunkVR_RetainedGeometryStats)[15] = u64::from(request);
    let none_verified = state().verified.is_empty();
    if (request != 1 && request != 2)
        || (w.stage != 6 && w.stage != 7)
        || !crate::stereo_probe::cvr_observe_bindings()
        || !crate::native_recording::cvr_native_recording_compatible()
        || (request == 2 && (CyberpunkVR_RetainedGeometryVerified.load(SeqCst) == 0 || none_verified))
    {
        fail(2);
        w.complete = true;
        return;
    }
    if request == 1 {
        state().verified.clear();
        CyberpunkVR_RetainedGeometryVerified.store(0, SeqCst);
    }
    crate::target_handoff_observer::cvr_th2_clear();
    crate::target_access_observer::events::clear();
    if w.tail_enabled {
        crate::target_access_observer::events::arm();
    }
    arm_frontier_observer(w);
    if w.scene_access_observer {
        crate::scene_access_native::cvr_sao_arm();
        CyberpunkVR_SceneChildEnable.store(1, SeqCst);
    }
    WATCHING.store(true, SeqCst);
    CyberpunkVR_RetainedGeometryState.store(1, SeqCst);
}

// ---- the list observers

fn flush(list: *mut c_void, reason: u32) {
    if internal() || !active() {
        return;
    }
    let _guard = lock();
    if let Some((side, _)) = owned(list) {
        seal(list, side, reason, if reason == 0 { exit_kind::CLOSE } else { exit_kind::OTHER });
    }
}
fn before_barriers(list: *mut c_void, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
    if internal() || !active() || count == 0 {
        return;
    }
    let _guard = lock();
    let Some((side, value)) = owned(list) else { return };
    if barriers.is_null() {
        seal(list, side, 10, exit_kind::UNKNOWN);
        return;
    }
    let mut kind = 0u32;
    // SAFETY: `count` barriers; the eye's targets are layout-equal to GeometryTargetsRaw (COM pointers as words).
    let sealed = unsafe {
        crate::retained_indirect::cvr_nrg_barrier_seal(
            barriers,
            count,
            (&value.targets as *const Targets).cast(),
            value.imported.as_ptr().cast(),
            &mut kind,
        )
    };
    if sealed {
        seal(list, side, 11, kind);
    }
}
fn before_resources(list: *mut c_void, source: *mut c_void, destination: *mut c_void, reason: u32) {
    if internal() || !active() {
        return;
    }
    let _guard = lock();
    if let Some((side, value)) = owned(list) {
        if touches(value, source) || touches(value, destination) {
            let kind = match reason {
                11 => exit_kind::RESOLVE,
                3 => exit_kind::DISCARD,
                _ => exit_kind::COPY,
            };
            seal(list, side, reason, kind);
        }
    }
}
fn reset(list: *mut c_void) {
    crate::target_handoff_observer::cvr_th2_reset(list);
    crate::prepared_pair_native::cvr_npp2_reset(list);
    if internal() || !crate::stereo_probe::cvr_observe_bindings() {
        return;
    }
    let _guard = lock();
    state().render_passes.retain(|l| *l != list as usize);
    if let Some((side, value)) = owned(list) {
        if work().is_some_and(|w| w.source_cbv_snapshots) {
            work().expect("owned").cbv_snapshots_failed = true;
            value.cbv_arena_aborted = true;
            arena_op(side, arena::CBV, 0);
        }
        value.sealed = true;
        fail(20);
    }
}
fn render_pass(list: *mut c_void, begin: bool) {
    if internal() || !crate::stereo_probe::cvr_observe_bindings() {
        return;
    }
    let _guard = lock();
    if begin {
        if let Some((side, _)) = owned(list) {
            seal(list, side, 12, exit_kind::UNKNOWN);
        }
        if !state().render_passes.contains(&(list as usize)) {
            state().render_passes.push(list as usize);
        }
    } else {
        state().render_passes.retain(|l| *l != list as usize);
    }
}

// ---- submissions

fn rows_mut<const N: usize>(w: &mut WorkScalars, kind: u32) -> &'static mut [[u64; N]] {
    let mut count = 0usize;
    // SAFETY: the Work's journal; the table's width is N (retained_journal.rs ROWS).
    let rows = unsafe { crate::retained_journal::cvr_nrg_rows(w.journal.cast(), kind, &mut count) };
    if rows.is_null() || count == 0 {
        return &mut [];
    }
    // SAFETY: `count` whole rows of N words.
    unsafe { core::slice::from_raw_parts_mut(rows.cast::<[u64; N]>(), count) }
}
const INSTANCE_GPU_ROWS: u32 = 20;
const IA_COMMANDS: u32 = 36;
const IA_VIEWS: u32 = 37;

fn submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    crate::prepared_pair_native::cvr_npp2_submitted(queue, count, lists);
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return;
    }
    let _guard = lock();
    let Some(w) = work() else { return };
    // SAFETY: `count` lists.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let order = ex!(CyberpunkVR_RetainedSourceQueueOrder);
    if order[14] != 0 && order[1] == queue as u64 && lists.iter().any(|l| *l as u64 == order[4]) {
        order[15] = 1;
    }
    for side in 0..2usize {
        let value = w.eye(side);
        if !value.pending || value.submitted {
            continue;
        }
        if !lists.iter().any(|l| *l as u64 == value.recording.list) {
            continue;
        }
        value.pending = false;
        // SAFETY: a live queue (the hook's argument).
        let signaled = value.submission_certified
            && value.source_queue.as_ref().is_some_and(|q| q.as_raw() == queue)
            && value.fence.as_ref().is_some_and(|f| unsafe {
                ID3D12CommandQueue::from_raw_borrowed(&queue).is_some_and(|q| q.Signal(f, 1).is_ok())
            });
        if !signaled {
            value.submission_certified = false;
            value.source_queue = None;
            fail(21);
            w.complete = true;
            WATCHING.store(false, SeqCst);
            publish_sources();
            return;
        }
        value.signaled_value = 1;
        value.submitted = true;
        ex!(CyberpunkVR_RetainedGeometryStats)[6 + side] = 1;
        for row in rows_mut::<24>(w, INSTANCE_GPU_ROWS) {
            if row[1] == side as u64 && row[2] != 0 && row[3] != 0 {
                row[4] = 1;
            }
        }
        if w.source_ia_snapshots && w.eye(side).ia_validated {
            for command in rows_mut::<16>(w, IA_COMMANDS) {
                if command[1] == side as u64 && command[6] != 0 && command[14] != 0 && command[7] == 0 {
                    command[15] = 1;
                }
            }
            for row in rows_mut::<16>(w, IA_VIEWS) {
                if row[1] == side as u64 && row[13] != 0 {
                    row[14] = 1;
                }
            }
        }
    }
}
fn submit_current(queue: *mut c_void) {
    if internal() || queue.is_null() || !active() {
        return;
    }
    let _guard = lock();
    let Some(w) = work() else { return };
    if !w.current_enabled || !w.current_started || w.current_submitted {
        return;
    }
    if w.failed {
        crate::prepared_pair_native::cvr_npp2_cancel_current(w.frame);
        w.current_started = false;
        return;
    }
    if w.eyes().iter().any(|eye| !eye.submitted) {
        return;
    }
    if submit_current_checked(w, queue).is_err() {
        ex!(CyberpunkVR_RetainedCurrent)[15] = 2431;
        fail(2431);
        if let Some(w) = work() {
            crate::prepared_pair_native::cvr_npp2_cancel_current(w.frame);
            w.current_started = false;
        }
    }
}
fn submit_current_checked(w: &mut WorkScalars, queue: *mut c_void) -> Result<(), ()> {
    let infix = ex!(CyberpunkVR_RetainedInfix);
    if w.infix_enabled && (infix[3] == 0 || infix[12] != infix[3] || infix[14] != 0) {
        return Err(()); // "Infix consumer is outside the certified prefix boundary"
    }
    let stats = ex!(CyberpunkVR_RetainedCurrent);
    stats[10] = u64::from(w.eye(0).submitted);
    stats[11] = u64::from(w.eye(1).submitted);
    validate_borrowed_tables(true);
    observe_borrowed_pageables(true);
    stats[13] = w.eye(0).signaled_value;
    stats[14] = w.eye(1).signaled_value;
    for eye in w.eyes() {
        if !eye.sealed || !eye.submission_certified || eye.source_queue.as_ref().map_or(null_mut(), |q| q.as_raw()) != queue || eye.fence.is_none() || eye.signaled_value == 0 {
            return Err(()); // "Current source execution lacks its own queued fence"
        }
    }
    let lifetime = ex!(CyberpunkVR_RetainedTargetLifetime);
    for side in 0..2u32 {
        for i in 0..4u32 {
            let row = &mut lifetime[((side * 4 + i) * 16) as usize..][..16];
            if row[3] != 0 {
                let mut failure = 0u32;
                let mut details = [0u64; 2];
                // SAFETY: the host's own function.
                row[4] = u64::from(pairs::lifetime_validate(side, i, &mut failure, &mut details));
                if row[4] == 0 {
                    row[5] = 300 + u64::from(failure);
                    row[6] = details[0];
                    row[7] = details[1];
                }
            }
        }
    }
    if stats[12] != queue as u64 || !crate::prepared_pair_native::cvr_npp2_submit_current(queue, w.frame) {
        return Err(()); // "Current consumer submission failed"
    }
    w.current_submitted = true;
    stats[9] = 1;
    if w.infix_enabled {
        ex!(CyberpunkVR_RetainedInfix)[13] = 1;
    }
    Ok(())
}
fn current_prefix_executed(count: u32) {
    let _guard = lock();
    if let Some(w) = work() {
        if w.infix_enabled && w.infix_in_flight && !w.current_submitted {
            ex!(CyberpunkVR_RetainedInfix)[12] = u64::from(count);
        }
    }
}
fn current_suffix_executed(count: u32) {
    let _guard = lock();
    current_suffix_executed_locked(count);
}
fn current_suffix_executed_locked(count: u32) {
    if let Some(w) = work() {
        if w.infix_enabled && w.infix_in_flight {
            ex!(CyberpunkVR_RetainedInfix)[14] = u64::from(count);
            w.infix_in_flight = false;
            finish_targets();
            release_borrowed();
        }
    }
}
/// pair_submission::Check (include/Render/NativePairSubmission.hpp).
struct PairSource {
    list: u64,
    generation: u64,
    queue: u64,
    sealed: bool,
    certified: bool,
    submitted: bool,
}
fn pair_check(producer: &PairSource, consumer: &PairSource, queue: u64, lists: &[u64], separate: bool) -> (u32, u32, u32) {
    let (mut order, mut producer_index, mut consumer_index) = (0u32, u32::MAX, u32::MAX);
    if queue == 0
        || lists.is_empty()
        || lists.len() > 256
        || producer.list == 0
        || consumer.list == 0
        || producer.list == consumer.list
        || producer.generation == 0
        || consumer.generation == 0
        || !producer.sealed
        || !consumer.sealed
        || !producer.certified
        || !consumer.certified
        || producer.queue != queue
        || consumer.queue != queue
        || consumer.submitted
    {
        return (order, producer_index, consumer_index);
    }
    let (mut producers, mut consumers) = (0u32, 0u32);
    for (i, list) in lists.iter().enumerate() {
        if *list == producer.list {
            producers += 1;
            producer_index = i as u32;
        }
        if *list == consumer.list {
            consumers += 1;
            consumer_index = i as u32;
        }
    }
    if consumers != 1 {
        return (order, producer_index, consumer_index);
    }
    if producer.submitted {
        if producers == 0 {
            order = 1; // EarlierSubmission
        }
    } else if producers == 1 && separate && producer_index < consumer_index {
        order = 2; // SeparateCallsInBatch
    }
    (order, producer_index, consumer_index)
}
fn ordered_execution(queue: *mut c_void, count: u32, lists: *const *mut c_void, separate: bool) {
    if internal() || !active() || queue.is_null() || lists.is_null() || count == 0 || count > 256 {
        return;
    }
    let _guard = lock();
    let Some(w) = work() else { return };
    if w.complete || !w.source_stream || w.eye(0).submitted {
        return;
    }
    // SAFETY: `count` lists.
    let addresses: Vec<u64> = unsafe { core::slice::from_raw_parts(lists, count as usize) }.iter().map(|l| *l as u64).collect();
    if !addresses.contains(&w.eye(0).recording.list) {
        return;
    }
    let input = |eye: &EyeScalars| PairSource {
        list: eye.recording.list,
        generation: eye.recording.generation,
        queue: eye.source_queue.as_ref().map_or(0, |q| q.as_raw() as u64),
        sealed: eye.sealed,
        certified: eye.submission_certified,
        submitted: eye.submitted,
    };
    let (order, producer_index, consumer_index) = pair_check(&input(w.eye(1)), &input(w.eye(0)), queue as u64, &addresses, separate);
    let arenas = |side: u32| arena_op(side, arena::CBV, 1);
    let finalized = !w.failed
        && w.eye(0).sealed
        && w.eye(1).sealed
        && (!w.source_cbv_snapshots || (arenas(0) && arenas(1)))
        && (!w.source_ia_snapshots || (w.eye(0).ia_validated && w.eye(1).ia_validated));
    let (main, peer) = (&*w.eye(0), &*w.eye(1));
    let row = [
        u64::from(w.frame),
        queue as u64,
        peer.recording.list,
        peer.recording.generation,
        main.recording.list,
        main.recording.generation,
        u64::from(producer_index),
        u64::from(consumer_index),
        u64::from(count),
        u64::from(separate),
        u64::from(peer.submitted),
        u64::from(peer.submission_certified),
        u64::from(main.submission_certified),
        u64::from(finalized),
        if finalized { u64::from(order) } else { 0 },
        0,
    ];
    ex!(CyberpunkVR_RetainedSourceQueueOrder).copy_from_slice(&row);
}

// ---- the self-tests

fn test_infix_capture_lifetime() -> bool {
    let guard = lock();
    if work().is_some() || WATCHING.load(SeqCst) || npp_active() {
        return false;
    }
    let state_before = CyberpunkVR_RetainedGeometryState.load(SeqCst);
    let request = CyberpunkVR_RetainedGeometryRequest.load(SeqCst);
    let previous = *ex!(CyberpunkVR_RetainedInfix);
    let capture = work::work_new();
    // SAFETY: a new Work or null.
    let Some(c) = (unsafe { capture.as_mut() }) else { return false };
    c.infix_enabled = true;
    c.infix_in_flight = true;
    c.complete = true;
    // The new Work becomes the installed one.
    work::work_install(capture);
    state().rust = Some(Box::default());
    WATCHING.store(true, SeqCst);
    CyberpunkVR_RetainedGeometryState.store(3, SeqCst);
    CyberpunkVR_RetainedGeometryRequest.store(1, SeqCst);
    drop(guard);
    frame_boundary();
    let guard = lock();
    let mut result = state().work == capture && work().is_some_and(|w| w.infix_in_flight) && CyberpunkVR_RetainedGeometryRequest.load(SeqCst) == 1;
    drop(guard);
    current_suffix_executed(5);
    let _guard = lock();
    result = result && state().work == capture && work().is_some_and(|w| !w.infix_in_flight) && ex!(CyberpunkVR_RetainedInfix)[14] == 5;
    // The installed Work is deleted.
    work::work_install(null_mut());
    state().rust = None;
    WATCHING.store(false, SeqCst);
    CyberpunkVR_RetainedGeometryState.store(state_before, SeqCst);
    CyberpunkVR_RetainedGeometryRequest.store(request, SeqCst);
    *ex!(CyberpunkVR_RetainedInfix) = previous;
    result
}
fn test_frontier_failure_disarm() -> bool {
    let _guard = lock();
    if work().is_some() || WATCHING.load(SeqCst) {
        return false;
    }
    let state_before = CyberpunkVR_RetainedGeometryState.load(SeqCst);
    let error = ex!(CyberpunkVR_RetainedGeometryStats)[14];
    let enabled = CyberpunkVR_SubmissionFrontierEnable.load(SeqCst);
    let current = CyberpunkVR_PreparedCurrentEnable.load(SeqCst);
    let split = scene_split().load(SeqCst);
    let mut result = true;
    for prepared in [false, true] {
        CyberpunkVR_PreparedCurrentEnable.store(u32::from(prepared), SeqCst);
        scene_split().store(u32::from(!prepared), SeqCst);
        let capture = work::work_new();
        if capture.is_null() {
            return false;
        }
        // The new Work becomes the installed one (deleting the previous test's).
        work::work_install(capture);
        state().rust = Some(Box::default());
        let w = work().expect("installed");
        latch_frontier_observer(w);
        CyberpunkVR_SubmissionFrontierEnable.store(1, SeqCst);
        CyberpunkVR_PreparedCurrentEnable.store(0, SeqCst);
        scene_split().store(0, SeqCst);
        fail(2449);
        let w = work().expect("installed");
        result = result
            && w.current_enabled == prepared
            && w.frontier_observer
            && w.failed
            && CyberpunkVR_SubmissionFrontierEnable.load(SeqCst) == 0
            && CyberpunkVR_RetainedGeometryState.load(SeqCst) == 4;
    }
    // The installed Work is deleted.
    work::work_install(null_mut());
    state().rust = None;
    CyberpunkVR_RetainedGeometryState.store(state_before, SeqCst);
    ex!(CyberpunkVR_RetainedGeometryStats)[14] = error;
    CyberpunkVR_SubmissionFrontierEnable.store(enabled, SeqCst);
    CyberpunkVR_PreparedCurrentEnable.store(current, SeqCst);
    scene_split().store(split, SeqCst);
    result
}
fn test_frontier_observer_modes() -> bool {
    let _guard = lock();
    if work().is_some() || WATCHING.load(SeqCst) {
        return false;
    }
    let enabled = CyberpunkVR_SubmissionFrontierEnable.load(SeqCst);
    let current = CyberpunkVR_PreparedCurrentEnable.load(SeqCst);
    let split = scene_split().load(SeqCst);
    let mut result = true;
    for prepared in [0u32, 1] {
        for scene in [0u32, 1, 2, 3] {
            CyberpunkVR_PreparedCurrentEnable.store(prepared, SeqCst);
            scene_split().store(scene, SeqCst);
            let capture = work::work_new();
            // SAFETY: a new Work or null, this test's own.
            let Some(c) = (unsafe { capture.as_mut() }) else { return false };
            latch_frontier_observer(c);
            let requested = prepared != 0 || scene == 1 || scene == 2;
            result = result
                && c.current_enabled == (prepared != 0)
                && c.frontier_observer == requested
                && !c.current_started
                && !c.current_submitted
                && !c.infix_enabled;
            CyberpunkVR_PreparedCurrentEnable.store(0, SeqCst);
            scene_split().store(0, SeqCst);
            CyberpunkVR_SubmissionFrontierEnable.store(1, SeqCst);
            arm_frontier_observer(c);
            result = result && CyberpunkVR_SubmissionFrontierEnable.load(SeqCst) == if requested { 0 } else { 1 };
            CyberpunkVR_SubmissionFrontierEnable.store(1, SeqCst);
            disarm_finished_frontier_observer(c);
            result = result && CyberpunkVR_SubmissionFrontierEnable.load(SeqCst) == 1;
            c.complete = true;
            disarm_finished_frontier_observer(c);
            result = result && CyberpunkVR_SubmissionFrontierEnable.load(SeqCst) == if requested { 0 } else { 1 };
            work::work_free(capture);
        }
    }
    CyberpunkVR_SubmissionFrontierEnable.store(enabled, SeqCst);
    CyberpunkVR_PreparedCurrentEnable.store(current, SeqCst);
    scene_split().store(split, SeqCst);
    result
}

// ---- the C ABI

/// # Safety
/// `host` points at a table with static storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
/// The capture's lock (the C++ half's mutex): `true` acquires, `false` releases.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_lock(on: bool) {
    raw_lock(on);
}
/// Under the lock: the installed work (C++ Work installs and deletes go through the host, so this is its view).
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_set_watching(on: bool) {
    WATCHING.store(on, SeqCst);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_watching() -> bool {
    WATCHING.load(SeqCst)
}
/// Under the lock.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_fail(reason: u32) {
    fail(reason);
}
/// Under the lock.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_publish() {
    publish();
}
/// Under the lock: the eye whose open recording is `list` (its side through `side`), else null.
///
/// # Safety
/// `side` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_owned(list: *mut c_void, side: *mut u32) -> *mut EyeScalars {
    match owned(list) {
        Some((s, eye)) => {
            // SAFETY: as documented.
            unsafe { *side = s };
            eye
        }
        None => {
            // SAFETY: as documented (the C++ loop left `side` at 2).
            unsafe { *side = 2 };
            null_mut()
        }
    }
}
/// Under the lock.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_proof(list: *mut c_void, side: u32) -> bool {
    proof(list, side)
}
/// Under the lock: whether `list` is inside a render pass.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_in_pass_locked(list: *mut c_void) -> bool {
    state().render_passes.contains(&(list as usize))
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_in_render_pass(list: *mut c_void) -> bool {
    let _guard = lock();
    state().render_passes.contains(&(list as usize))
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_frame_boundary() {
    frame_boundary();
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_flush(list: *mut c_void, reason: u32) {
    flush(list, reason);
}
/// # Safety
/// `barriers` holds `count` barriers or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_before_barriers(list: *mut c_void, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
    before_barriers(list, count, barriers);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_before_resources(list: *mut c_void, source: *mut c_void, destination: *mut c_void, reason: u32) {
    before_resources(list, source, destination, reason);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_reset(list: *mut c_void) {
    reset(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_render_pass(list: *mut c_void, begin: bool) {
    render_pass(list, begin);
}
/// # Safety
/// `lists` holds `count` lists or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    submitted(queue, count, lists);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_submit_current(queue: *mut c_void) {
    submit_current(queue);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_prefix_executed(count: u32) {
    current_prefix_executed(count);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_suffix_executed(count: u32) {
    current_suffix_executed(count);
}
/// # Safety
/// `lists` holds `count` lists or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_ordered_execution(queue: *mut c_void, count: u32, lists: *const *mut c_void, separate: bool) {
    ordered_execution(queue, count, lists, separate);
}
/// # Safety
/// `record` points at a draw record.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_draw(list: *mut c_void, record: *const DrawRecord) -> bool {
    // SAFETY: as documented.
    draw(list, unsafe { &*record })
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_indirect(
    list: *mut c_void,
    signature: *mut c_void,
    count: u32,
    arguments: *mut c_void,
    offset: u64,
    count_buffer: *mut c_void,
    count_offset: u64,
) -> bool {
    indirect(list, signature, count, arguments, offset, count_buffer, count_offset)
}
/// Under the lock: the work's drawn pipelines (PublishShaders), their count through `count`.
///
/// # Safety
/// `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_pipelines(count: *mut usize) -> *const *mut c_void {
    let pipelines = &rust_side().pipelines;
    // SAFETY: as documented.
    unsafe { *count = pipelines.len() };
    pipelines.as_ptr().cast()
}
/// Under the lock: the verified pipelines become the work's (Poll, a mode-1 capture that completed).
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_verify_pipelines() {
    state().verified = rust_side().pipelines.clone();
}
/// # Safety
/// `lists` holds `count` lists or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) -> bool {
    before_submit(queue, count, lists)
}
/// Under the lock (AppendSource): CaptureCbvSnapshots; false when the C++ capture would have thrown.
///
/// # Safety
/// `source` is the capture's source (its shared scalars) and `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_cbv(list: *mut c_void, side: u32, source: *mut SourceScalars, row: *const [u64; 48]) -> bool {
    // SAFETY: as documented.
    capture_cbv_snapshots(list, side, unsafe { &mut *source }, unsafe { &*row }).is_ok()
}
/// Under the lock: the eye's CBV arena -- op 0 reset, 1 whether finalized; `cvr_nrg2_cbv_readback` its readback.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_cbv_arena(side: u32, op: u32) -> bool {
    arena_op(side, arena::CBV, op)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_cbv_readback(side: u32) -> *mut c_void {
    eye_rust(side).cbv_readback.as_ref().map_or(null_mut(), |r| r.as_raw())
}
/// Under the lock (AppendSource): CaptureT5Snapshot and CaptureFullT5.
///
/// # Safety
/// `source` is the capture's source (its shared scalars) and `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_t5(list: *mut c_void, side: u32, source: *mut SourceScalars, row: *const [u64; 48], full: bool) {
    // SAFETY: as documented.
    let (source, row) = unsafe { (&mut *source, &*row) };
    if full {
        capture_full_t5(list, side, source, row);
    } else {
        capture_t5_snapshot(list, side, source, row);
    }
}
/// Under the lock: the eye's arena (`which`: CBV, T5, full T5) -- op 0 abort (CBV: reset), 1 whether finalized,
/// 2 whether present (the T5 arenas); `cvr_nrg2_arena_bytes` its bytes; `cvr_nrg2_readback` the eye's readbacks
/// (1 T5, 2 the full T5's original bytes, 3 its private copies).
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_arena(side: u32, which: u32, op: u32) -> bool {
    if op == 2 {
        return structured_present(side, which == arena::FULL_T5);
    }
    arena_op(side, which, op)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_arena_bytes(side: u32, which: u32) -> u64 {
    structured_bytes(side, which == arena::FULL_T5)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_readback(side: u32, which: u32) -> *mut c_void {
    let eye = eye_rust(side);
    let readback = match which {
        1 => &eye.t5_readback,
        2 => &eye.full_t5_original_readback,
        _ => &eye.full_t5_private_readback,
    };
    readback.as_ref().map_or(null_mut(), |r| r.as_raw())
}
/// Under the lock: PublishSources.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_publish_sources() {
    publish_sources_impl();
}
/// Under the lock (AppendSource): CaptureDetails.
///
/// # Safety
/// `cbv` is the CBV row and `backing` the source's CBV backing.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_details(list: *mut c_void, root: u64, cbv: *const [u64; 24], backing: *const crate::source_cbv_snapshot::SourceCbvBacking) {
    // SAFETY: as documented.
    capture_details(list, root, unsafe { &*cbv }, unsafe { &*backing });
}
/// Under the lock (AppendSource): CaptureMaterialResources.
///
/// # Safety
/// `source` is the capture's source (its shared scalars) and `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_materials(list: *mut c_void, source: *mut SourceScalars, row: *const [u64; 48]) {
    // SAFETY: as documented.
    capture_material_resources(list, unsafe { &mut *source }, unsafe { &*row });
}
/// Under the lock (AppendSource): CaptureIaSnapshots.
///
/// # Safety
/// `source` is the capture's source (its shared scalars) and `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_ia(list: *mut c_void, side: u32, source: *mut SourceScalars, row: *const [u64; 48], indexed: bool) {
    // SAFETY: as documented.
    capture_ia_snapshots(list, side, unsafe { &mut *source }, unsafe { &*row }, indexed);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_test(which: u32) -> bool {
    match which {
        0 => test_infix_capture_lifetime(),
        1 => test_frontier_failure_disarm(),
        _ => test_frontier_observer_modes(),
    }
}
// The graphics list type is the hooks' argument; kept for the signatures' documentation.
#[allow(dead_code)]
type List = ID3D12GraphicsCommandList;

// ---- the borrowed tables (formerly the .cpp's CaptureBorrowedTables, CaptureFixedAssets, ValidateBorrowedTables,
// ObserveBorrowedPageables and ReleaseBorrowedObservers): a stage-9 source's borrowed descriptor ranges captured at the
// draw (borrowed_sources.rs), their physical-remainder ranges' inventories and fixed assets, validated at the boundary
// and after the prefix, and the staged pairs' immutable materials bound as pageable uses of the boundary's scope.

const BORROWED_TABLE_ROWS: u32 = 3;
const BORROWED_RANGE_ROWS: u32 = 4;
const BORROWED_INVENTORY_ROWS: u32 = 5;
const BORROWED_PAGEABLE_ROWS: u32 = 6;
const FIXED_RANGE_ROWS: u32 = 7;
const FIXED_ASSET_ROWS: u32 = 8;
const FIXED_SLOT_ROWS: u32 = 9;
const FIXED_DESCRIPTOR_ROWS: u32 = 10;
/// queue_residency::PendingPageableUse::Data: the scope and source recording it was bound in, the physical pageable
/// (its references held) and its membership.
struct PageableUse {
    scope: Option<IUnknown>,
    source: crate::borrowed_sources::Recording,
    physical: crate::texture_immutable::ImmutablePageable,
    membership: crate::queue_residency::PageableMembership,
    kind: u32,
}
impl Drop for PageableUse {
    fn drop(&mut self) {
        crate::structured_snapshot_receipt::release(self.physical.source.owner);
        crate::structured_snapshot_receipt::release(self.physical.source.source.resource);
        crate::structured_snapshot_receipt::release(self.physical.pageable);
    }
}
/// The work's borrowed tables' state (the .cpp's Work fields and each source's borrowedRanges).
#[derive(Default)]
struct BorrowedState {
    ranges: std::collections::HashMap<usize, std::sync::Arc<crate::borrowed_sources::SourceRanges>>,
    fixed: Vec<crate::borrowed_sources::FixedRange>,
    pageables: Vec<Option<PageableUse>>,
    scope: Option<IUnknown>,
    inventories: std::collections::BTreeSet<(u64, u64, u64, u64)>,
}
fn borrowed() -> Option<&'static mut BorrowedState> {
    state().rust.as_mut().map(|r| &mut r.borrowed)
}
fn eye_lease(w: &WorkScalars, side: usize) -> &'static crate::native_recording::Lease {
    // SAFETY: the eye's recording, the same 40-byte native_recording::Lease (its token an IUnknown pointer).
    unsafe { &*(&raw const w.eye(side).recording).cast::<crate::native_recording::Lease>() }
}
fn borrowed_last() -> (u32, [u64; 4]) {
    let mut details = [0u64; 4];
    // SAFETY: a writable record.
    let failure = unsafe { crate::audit_borrowed_ranges::cvr_sda_bsr_last(&mut details) };
    (failure, details)
}
/// BorrowedTableFailure: the row's first failure, and the capture's first failure with the borrowed stage's details.
fn borrowed_table_failure(row: &mut [u64; 16], code: u32) {
    if row[14] == 0 {
        row[14] = u64::from(code);
    }
    let stats = ex!(CyberpunkVR_RetainedBorrowedTablesStats);
    if stats[13] == 0 {
        stats[12] = row[0] + 1;
        stats[13] = u64::from(code);
        let detail = ex!(CyberpunkVR_RetainedBorrowedFailure);
        detail[0] = row[0];
        detail[1] = u64::from(code);
        if (100..200).contains(&code) {
            let (failure, observed) = borrowed_last();
            detail[2] = u64::from(failure);
            detail[3..7].copy_from_slice(&observed);
        }
    }
}
fn append<const N: usize>(w: &mut WorkScalars, kind: u32, row: &[u64; N]) -> usize {
    // SAFETY: the work's journal; one row of the table's width.
    unsafe { crate::retained_journal::cvr_nrg_rows_append(w.journal.cast(), kind, row.as_ptr(), 1) };
    rows_mut::<N>(w, kind).len() - 1
}
/// CaptureFixedAssets: the physical-remainder range's fixed assets (at most four ranges, 65536 slots each).
fn capture_fixed_assets(
    list: *mut c_void,
    side: usize,
    ranges: &std::sync::Arc<crate::borrowed_sources::SourceRanges>,
    saved: &[u64; 48],
    range_index: u32,
    range_row: u64,
    range: &crate::borrowed_sources::RangeRow,
) {
    let Some(w) = work() else { return };
    if !w.fixed_assets_enabled {
        return;
    }
    let stats = ex!(CyberpunkVR_RetainedFixedAssetsStats);
    stats[2] = u64::from(w.frame);
    stats[3] += 1;
    let mut first = [0u64; 24];
    first[..8].copy_from_slice(&[saved[0], side as u64, u64::from(w.frame), range_row, range.heap, range.gpu, u64::from(range.count), range.version]);
    let at = append(w, FIXED_RANGE_ROWS, &first);
    let refused = |w: &mut WorkScalars, code: u32| {
        rows_mut::<24>(w, FIXED_RANGE_ROWS)[at][17] = u64::from(code);
        ex!(CyberpunkVR_RetainedFixedAssetsStats)[11] += 1;
    };
    if rows_mut::<24>(w, FIXED_RANGE_ROWS).len() > 4
        || range.count > 65536
        || range.count as usize > 262_144usize.saturating_sub(rows_mut::<8>(w, FIXED_SLOT_ROWS).len())
    {
        refused(w, 1);
        return;
    }
    let Some(captured) = crate::borrowed_sources::capture_fixed(list, eye_lease(w, side), ranges, range_index) else {
        refused(w, 100 + borrowed_last().0);
        return;
    };
    // DescribeBorrowedFixedRange's stages.
    crate::audit_borrowed_ranges::stage(1, u64::MAX, 0, 0, 0);
    crate::audit_borrowed_ranges::stage(0, u64::MAX, 0, 0, 0);
    let s = &captured.summary;
    if s.list != saved[6]
        || s.generation != saved[7]
        || s.root != saved[11]
        || s.pipeline != saved[10]
        || s.range.heap != range.heap
        || s.range.gpu != range.gpu
        || s.range.count != range.count
        || s.range.version != range.version
        || s.slots.len() != range.count as usize
        || s.assets.len() > 65536usize.saturating_sub(rows_mut::<16>(w, FIXED_ASSET_ROWS).len())
    {
        refused(w, 2);
        return;
    }
    let capture_index = (rows_mut::<24>(w, FIXED_RANGE_ROWS).len() - 1) as u64;
    {
        let assets_at = rows_mut::<16>(w, FIXED_ASSET_ROWS).len() as u64;
        let slots_at = rows_mut::<8>(w, FIXED_SLOT_ROWS).len() as u64;
        let row = &mut rows_mut::<24>(w, FIXED_RANGE_ROWS)[at];
        row[10] = assets_at;
        row[11] = s.assets.len() as u64;
        row[12] = slots_at;
        row[13] = s.slots.len() as u64;
        row[14] = s.root;
        row[15] = s.pipeline;
        row[16] = s.family;
    }
    let stats = ex!(CyberpunkVR_RetainedFixedAssetsStats);
    for (index, asset) in s.assets.iter().enumerate() {
        let mut value = [0u64; 16];
        value[..8].copy_from_slice(&[
            capture_index,
            index as u64,
            asset.cookie,
            asset.candidate,
            asset.actual_cookie,
            u64::from(asset.status),
            u64::from(asset.stage),
            u64::from(asset.failure),
        ]);
        value[8..16].copy_from_slice(&asset.pin_details);
        append(w, FIXED_ASSET_ROWS, &value);
        if asset.status == crate::borrowed_sources::status::IMMUTABLE {
            stats[9] += 1;
        } else {
            stats[10] += 1;
        }
    }
    for slot in &s.slots {
        append(w, FIXED_SLOT_ROWS, &[capture_index, u64::from(slot.offset), u64::from(slot.asset), slot.cookie, slot.candidate, u64::from(slot.status), 0, 0]);
    }
    for sample in &s.samples {
        let mut diagnostic = [0u64; 24];
        diagnostic[..4].copy_from_slice(&[capture_index, u64::from(sample.offset), u64::from(sample.status), u64::from(sample.asset)]);
        diagnostic[4..24].copy_from_slice(&sample.canonical);
        append(w, FIXED_DESCRIPTOR_ROWS, &diagnostic);
    }
    let counts = ex!(CyberpunkVR_RetainedFixedAssetsCounts);
    for (i, c) in s.counts.iter().enumerate() {
        counts[i] += c;
    }
    let (qualified, slots, assets, immutable) =
        (u64::from(s.all_slots_qualified), s.slots.len() as u64, s.assets.len() as u64, s.counts[crate::borrowed_sources::status::IMMUTABLE as usize]);
    {
        let row = &mut rows_mut::<24>(w, FIXED_RANGE_ROWS)[at];
        row[8] = 1;
        row[9] = qualified;
    }
    stats[4] += 1;
    stats[5] += qualified;
    stats[6] += slots;
    stats[7] += assets;
    stats[8] += immutable;
    if let Some(b) = borrowed() {
        b.fixed.push(captured);
    }
}
/// CaptureBorrowedTables: a stage-9 source's borrowed ranges at its draw, described, each range a row, each new
/// physical-remainder range's inventory a row (at most 256) and its fixed assets.
///
/// # Safety
/// `list` is the live list; `source` the work's source; `saved` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_borrowed(list: *mut c_void, side: u32, source: *const SourceScalars, saved: *const [u64; 48]) {
    let Some(w) = work() else { return };
    // SAFETY: as documented.
    let (source, saved) = unsafe { (&*source, &*saved) };
    if !w.borrowed_tables_enabled || saved[4] != 9 || w.failed {
        return;
    }
    let side = side as usize;
    let stats = ex!(CyberpunkVR_RetainedBorrowedTablesStats);
    stats[2] = u64::from(w.frame);
    stats[3] += 1;
    let mut first = [0u64; 16];
    first[..7].copy_from_slice(&[saved[0], side as u64, u64::from(w.frame), saved[6], saved[7], saved[11], saved[10]]);
    let at = append(w, BORROWED_TABLE_ROWS, &first);
    let fail = |w: &mut WorkScalars, code: u32| {
        ex!(CyberpunkVR_RetainedBorrowedTablesStats)[9] += 1;
        borrowed_table_failure(&mut rows_mut::<16>(w, BORROWED_TABLE_ROWS)[at], code);
    };
    if saved[44] != 0 || saved[2] != 0 {
        fail(w, 7);
        return;
    }
    let Some(ranges) = crate::borrowed_sources::capture(list, eye_lease(w, side), &source.bindings) else {
        fail(w, 100 + borrowed_last().0);
        return;
    };
    if let Some(b) = borrowed() {
        b.ranges.insert(saved[0] as usize, ranges.clone());
    }
    let range_rows = rows_mut::<12>(w, BORROWED_RANGE_ROWS).len();
    let summary = crate::borrowed_sources::describe(&ranges).filter(|s| {
        s.list == saved[6]
            && s.generation == saved[7]
            && s.root == saved[11]
            && s.pipeline == saved[10]
            && s.family != 0
            && !s.ranges.is_empty()
            && s.ranges.len() <= 16384usize.saturating_sub(range_rows)
    });
    let Some(summary) = summary else {
        fail(w, 2);
        return;
    };
    let first_range = range_rows as u64;
    {
        let row = &mut rows_mut::<16>(w, BORROWED_TABLE_ROWS)[at];
        row[7] = summary.family;
        row[8] = first_range;
        row[9] = summary.ranges.len() as u64;
    }
    for r in &summary.ranges {
        append(
            w,
            BORROWED_RANGE_ROWS,
            &[
                saved[0],
                u64::from(r.parameter),
                u64::from(r.kind),
                u64::from(r.first_offset),
                u64::from(r.count),
                u64::from(r.stride),
                u64::from(r.shader_declared_count),
                r.table,
                r.gpu,
                r.heap,
                r.version,
                u64::from(r.physical_remainder),
            ],
        );
        rows_mut::<16>(w, BORROWED_TABLE_ROWS)[at][10] += u64::from(r.count);
    }
    for (index, r) in summary.ranges.iter().enumerate() {
        if !r.physical_remainder {
            continue;
        }
        let key = (r.heap, r.gpu, u64::from(r.count), r.version);
        if borrowed().is_some_and(|b| b.inventories.contains(&key)) {
            continue;
        }
        let inventory = (rows_mut::<24>(w, BORROWED_INVENTORY_ROWS).len() < 256)
            .then(|| crate::borrowed_sources::inventory(&ranges, index as u32))
            .flatten()
            .filter(|i| {
                i.list == saved[6]
                    && i.generation == saved[7]
                    && i.root == saved[11]
                    && i.pipeline == saved[10]
                    && i.range.heap == r.heap
                    && i.range.gpu == r.gpu
                    && i.range.count == r.count
                    && i.range.version == r.version
            });
        let Some(inventory) = inventory else {
            fail(w, 100 + borrowed_last().0);
            return;
        };
        let c = inventory.counts;
        if let Some(b) = borrowed() {
            b.inventories.insert(key);
        }
        append(
            w,
            BORROWED_INVENTORY_ROWS,
            &[
                saved[0],
                side as u64,
                u64::from(w.frame),
                first_range + index as u64,
                r.heap,
                r.gpu,
                u64::from(r.count),
                r.version,
                c.slots,
                c.unknown,
                c.known_null,
                c.resource_srv,
                c.other_kinds,
                c.unique_cookies,
                c.tracked_cookies,
                c.untracked_cookies,
                c.texture_2d,
                c.texture_2d_array,
                c.flags_rt_ds_uav,
                c.shape_unknown,
                0,
                0,
                0,
                0,
            ],
        );
        capture_fixed_assets(list, side, &ranges, saved, index as u32, first_range + index as u64, r);
    }
    let (count, descriptors) = {
        let row = &mut rows_mut::<16>(w, BORROWED_TABLE_ROWS)[at];
        row[11] = 1;
        (row[9], row[10])
    };
    let stats = ex!(CyberpunkVR_RetainedBorrowedTablesStats);
    stats[4] += 1;
    stats[5] += count;
    stats[6] += descriptors;
}
/// ValidateBorrowedTables: each captured source's ranges still valid (in the boundary's scope, or after the prefix).
fn validate_borrowed_tables(after_prefix: bool) {
    let Some(w) = work() else { return };
    if !w.borrowed_tables_enabled {
        return;
    }
    let scope = borrowed().and_then(|b| b.scope.as_ref().map(|s| s.as_raw())).unwrap_or(null_mut());
    // SAFETY: the scope's proof or null.
    let scope_valid = unsafe { crate::queue_residency::cvr_qr_validate(scope) };
    let sources = source_count();
    let rows = rows_mut::<16>(w, BORROWED_TABLE_ROWS);
    for row in rows.iter_mut() {
        let joined = scope_valid
            && row[11] != 0
            && (row[0] as usize) < sources
            && if after_prefix {
                row[12] != 0
            } else {
                // SAFETY: the proof; the eye's recording.
                unsafe { crate::queue_residency::cvr_qr_contains_recording_runtime(scope, eye_lease(w, row[1] as usize)) }
            };
        let valid = joined
            && match borrowed().and_then(|b| b.ranges.get(&(row[0] as usize)).cloned()) {
                Some(r) => crate::borrowed_sources::validate(&r),
                None => {
                    crate::audit_borrowed_ranges::stage(1, u64::MAX, 0, 0, 0);
                    false
                }
            };
        row[if after_prefix { 13 } else { 12 }] = u64::from(valid);
        let stats = ex!(CyberpunkVR_RetainedBorrowedTablesStats);
        stats[match (valid, after_prefix) {
            (true, true) => 8,
            (true, false) => 7,
            (false, true) => 11,
            (false, false) => 10,
        }] += 1;
        if !valid {
            borrowed_table_failure(row, if joined { 100 + borrowed_last().0 } else if after_prefix { 5 } else { 4 });
        }
    }
}
/// The failure of a pageable use: the residency's failure and details on the row and, first, in the exports.
fn pageable_failure(row: &mut [u64; 16]) {
    let mut details = [0u64; 12];
    // SAFETY: writable records.
    unsafe { crate::queue_residency::cvr_qr_last_details(&mut details) };
    row[13] = u64::from(crate::queue_residency::cvr_qr_last_failure());
    row[14] = details[8];
    let stats = ex!(CyberpunkVR_RetainedPageableUseStats);
    if stats[11] == 0 {
        stats[11] = row[13];
        stats[12] = row[14];
        stats[13] = row[0] + 1;
        let failure = ex!(CyberpunkVR_RetainedPageableUseFailure);
        failure[..12].copy_from_slice(&details);
        let mut member = [0u64; 12];
        let mut search = [0u64; 20];
        // SAFETY: as above.
        unsafe {
            crate::queue_residency::cvr_qr_membership_details(&mut member);
            crate::queue_residency::cvr_qr_pageable_search_details(&mut search);
        }
        failure[12..24].copy_from_slice(&member);
        *ex!(CyberpunkVR_RetainedPageableSearch) = search;
    }
}
/// ObserveBorrowedPageables: at the boundary, the staged pairs' immutable materials bound as pageable uses of its
/// scope (once per side and native texture, at most 2048); after the prefix, each bound use validated again.
fn observe_borrowed_pageables(after_prefix: bool) {
    let Some(w) = work() else { return };
    if !w.borrowed_tables_enabled {
        return;
    }
    let stats = ex!(CyberpunkVR_RetainedPageableUseStats);
    stats[2] = u64::from(w.frame);
    if after_prefix {
        let rows = rows_mut::<16>(w, BORROWED_PAGEABLE_ROWS);
        for (i, row) in rows.iter_mut().enumerate() {
            if row[11] == 0 {
                continue;
            }
            let held = borrowed().and_then(|b| b.pageables.get(i)).and_then(|p| p.as_ref());
            let valid = held.is_some_and(|p| {
                // SAFETY: the use's own records.
                unsafe {
                    crate::queue_residency::cvr_qr_validate_pageable_use(
                        p.scope.as_ref().map_or(null_mut(), |s| s.as_raw()),
                        &p.source.0,
                        &p.physical,
                        &p.membership,
                    )
                }
            });
            row[12] = u64::from(valid);
            stats[if valid { 5 } else { 7 }] += 1;
            if !valid {
                pageable_failure(row);
            }
        }
        return;
    }
    let mut seen = std::collections::BTreeSet::new();
    let sources = source_count();
    let borrowed_rows: Vec<[u64; 16]> = rows_mut::<16>(w, BORROWED_TABLE_ROWS).to_vec();
    for b in &borrowed_rows {
        let ordinal = b[0] as usize;
        if ordinal >= sources {
            continue;
        }
        let side = b[1] as usize;
        let materials = pairs::staged_immutables(ordinal);
        for material in materials {
            // SAFETY: a staged material's lease (alive with the work's source).
            let immutable = unsafe { &*material };
            let native = immutable.source.native;
            if !seen.insert((side, native.registry, native.handle, native.resource)) {
                continue;
            }
            if rows_mut::<16>(w, BORROWED_PAGEABLE_ROWS).len() >= 2048 {
                stats[10] = 1;
                return;
            }
            let lease = eye_lease(w, side);
            let mut row = [0u64; 16];
            row[..6].copy_from_slice(&[ordinal as u64, side as u64, u64::from(w.frame), u64::from(native.handle), native.resource, lease.list]);
            let scope = borrowed().and_then(|b| b.scope.clone());
            let mut receipt = PageableUse {
                scope: scope.clone(),
                source: crate::borrowed_sources::Recording::copy(lease),
                // SAFETY: zeroed are the empty records the bind fills.
                physical: unsafe { core::mem::zeroed() },
                membership: Default::default(),
                kind: 0,
            };
            // SAFETY: the proof (or null), the recording, the material's lease and the receipt's outputs.
            let bound = unsafe {
                crate::queue_residency::cvr_qr_bind_pageable_use(
                    scope.as_ref().map_or(null_mut(), |s| s.as_raw()),
                    &receipt.source.0,
                    immutable,
                    &mut receipt.physical,
                    &mut receipt.membership,
                    &mut receipt.kind,
                )
            };
            row[11] = u64::from(bound);
            stats[3] += 1;
            stats[if bound { 4 } else { 6 }] += 1;
            if bound {
                let m = &receipt.membership;
                row[6] = m.pageable;
                row[7] = m.managed;
                row[8] = u64::from(m.member_index);
                row[9] = m.source_set;
                row[10] = u64::from(receipt.kind);
                stats[if receipt.kind == 2 { 9 } else { 8 }] += 1;
            } else {
                pageable_failure(&mut row);
            }
            if let Some(b) = borrowed() {
                b.pageables.push(bound.then_some(receipt));
            }
            append(w, BORROWED_PAGEABLE_ROWS, &row);
        }
    }
}
/// ReleaseBorrowedObservers: the sources' ranges, the fixed assets, the pageable uses and the scope let go.
fn release_borrowed() {
    if let Some(b) = borrowed() {
        b.ranges.clear();
        b.fixed.clear();
        b.pageables.clear();
        b.scope = None;
    }
}
/// CurrentBoundary's borrowed half: the boundary's queue scope kept (its proof referenced; null when not captured),
/// the tables validated in it and the staged materials bound.
///
/// # Safety
/// `scope` is a Scope's proof or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_borrowed_boundary(scope: *mut c_void) {
    if !work().is_some_and(|w| w.borrowed_tables_enabled) {
        return;
    }
    if let Some(b) = borrowed() {
        // SAFETY: the caller's proof, referenced for the work.
        b.scope = unsafe { IUnknown::from_raw_borrowed(&scope) }.cloned();
    }
    validate_borrowed_tables(false);
    observe_borrowed_pageables(false);
}

// ---- the shader rows (formerly the .cpp's PublishShaders)

const SHADER_ROWS: u32 = 0;
/// Work::InputRow: an input element's semantic (NUL-padded, under 64 bytes) and its fields.
#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderInput {
    name: [u8; 64],
    index: u32,
    format: u32,
    slot: u32,
    offset: u32,
    classification: u32,
    step: u32,
}
const _: () = assert!(core::mem::size_of::<ShaderInput>() == 88);
/// PublishShaders: each verified pipeline's source snapshot as a shader row (its id, the pipeline, its VS, PS and root
/// bytes, its input rows), a snapshot with a semantic of 64 bytes or more left out; the exports name the rows.
fn publish_shaders() {
    let Some(w) = work() else { return };
    let rs = rust_side();
    rs.shaders.clear();
    // SAFETY: the work's journal.
    unsafe { crate::retained_journal::cvr_nrg_rows_clear(w.journal.cast(), SHADER_ROWS) };
    let pipelines: Vec<*mut c_void> = rs.pipelines.iter().map(|p| p.as_raw()).collect();
    // SAFETY: as above.
    unsafe { crate::retained_journal::cvr_nrg_rows_reserve(w.journal.cast(), SHADER_ROWS, pipelines.len()) };
    for pipeline in pipelines {
        let Some(source) = crate::shader_snapshot_native::source(pipeline) else { continue };
        let info = source.info();
        let mut inputs = Vec::with_capacity(info.inputs.len());
        let mut valid = true;
        for i in &info.inputs {
            if i.semantic.len() >= 64 {
                valid = false;
                break;
            }
            let mut name = [0u8; 64];
            name[..i.semantic.len()].copy_from_slice(&i.semantic);
            inputs.push(ShaderInput { name, index: i.index, format: i.format, slot: i.slot, offset: i.offset, classification: i.classification, step: i.step });
        }
        if !valid {
            continue;
        }
        let m = info.mirror;
        let row = [
            info.id,
            pipeline as u64,
            m.vs as u64,
            m.vs_len as u64,
            m.ps as u64,
            m.ps_len as u64,
            m.root as u64,
            m.root_len as u64,
            inputs.as_ptr() as u64,
            inputs.len() as u64,
            core::mem::size_of::<ShaderInput>() as u64,
        ];
        rs.shaders.push((source, inputs));
        append(w, SHADER_ROWS, &row);
    }
    let rows = rows_mut::<11>(w, SHADER_ROWS);
    // SAFETY: the exports, written under the capture's lock.
    unsafe {
        CyberpunkVR_RetainedGeometryShaderRows = if rows.is_empty() { 0 } else { rows.as_ptr() as u64 };
        CyberpunkVR_RetainedGeometryShaderCount = rows.len() as u32;
    }
}

// The retained sources' own state and capture (moving from the .cpp in one slice).
#[path = "retained_sources.rs"]
mod sources;
#[path = "retained_work.rs"]
mod work;
#[path = "retained_pairs.rs"]
mod pairs;
/// CurrentBoundary (the submission table's serialize step 2): retained_pairs.rs'.
pub fn current_boundary(queue: *mut c_void, count: u32, lists: *const *mut c_void) -> u32 {
    pairs::current_boundary(queue, count, lists)
}
