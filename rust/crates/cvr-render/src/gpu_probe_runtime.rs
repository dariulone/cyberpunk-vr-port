//! The stereo GPU probe (formerly src/Render/StereoGpuProbe.cpp; include/Render/StereoGpuProbe.hpp declares it): one
//! bounded comparison of a native draw (or a draw group, or a scene's depth prefix) rendered per eye against the same
//! geometry rendered once through the view-instanced variant into owned two-slice targets -- the targets and their
//! views made, the reference and shared draws recorded with the native bindings untouched but for the pipeline, targets,
//! viewport, scissor and view mask (all restored), the scene's native targets imported and resolved, late visibility
//! through predicates and a preparation gate, the readback compared once the fence completed.
//!
//! The .cpp keeps the forwarders and lends the exports and the native probe's state through the runtime (the plugin's
//! exports are Rust's, the scene probe test's their reference definitions; by symbol they would leave other Rust test
//! variants unlinked).

use crate::geometry_commands::DrawRecord;
use crate::gpu_probe_compare::{Difference, Layout, Totals};
use crate::gpu_probe_gate::{Gate, GateExports, ReadExports};
use crate::gpu_probe_scene::{FineExports, FineRate};
use crate::gpu_probe_timings::Timed;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use cvr_gpu::scene_state::Bindings;
use std::sync::{Mutex, MutexGuard};
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The exports (the .cpp lends them, in this order).
#[repr(C)]
pub struct Exports {
    pub request: *const AtomicU32,
    pub state: *const AtomicU32,
    pub native_reference: *const AtomicU32,
    pub reference_skipped: *const AtomicU32,
    pub queue_waits: *const AtomicU32,
    pub group: *const AtomicU32,
    pub source_draws: *const AtomicU32,
    pub main_draws: *const AtomicU32,
    pub main_view_mask: *const AtomicU32,
    pub main_reference_mask: *const AtomicU32,
    pub eye_differences: *mut [u64; 2],
    pub target_differences: *mut [u64; 4],
    pub difference_count: *mut u32,
    pub differences: *mut [Difference; 128],
    pub prepare_gate_ms: *const AtomicU32,
    pub prepare_gate_reason: *const AtomicU32,
    pub late_visibility: *const AtomicU32,
    pub matched_draws: *const AtomicU32,
    pub fallback_draws: *const AtomicU32,
    pub prepare_gate_start: *const AtomicU64,
    pub prepare_gate_main: *const AtomicU64,
    pub prepare_gate_release: *const AtomicU64,
    pub result: *const AtomicI32,
    pub different_bytes: *mut u64,
    pub compared_bytes: *mut u64,
    pub coverage: *mut [u64; 2],
    pub scene_depth: *const AtomicU32,
    pub scene_reject: *const AtomicU32,
    pub scene_size: *mut [u32; 2],
    pub scene_phase: *mut [u32; 2],
    pub scene_route: *const AtomicU32,
    pub scene_committed: *const AtomicU32,
    pub scene_routed: *mut [u32; 2],
    pub scene_samples: *mut [u64; 2],
    pub timing: *const AtomicU32,
    pub timing_drops: *const AtomicU32,
    pub ticks: *mut [[u64; 2]; 7],
    pub frequency: *mut [u64; 2],
    pub timed_count: *mut [[u32; 2]; 7],
    pub mixed_materials: *const AtomicU32,
    pub fine_rate: *const AtomicU32,
    pub vrs: *mut [[u32; 4]; 2],
    pub vrs_changes: *mut [u32; 2],
    pub vrs_image: *mut [u64; 2],
    pub vrs_size: *mut [u32; 2],
    pub vrs_captured: *mut [u32; 2],
    pub vrs_difference: *mut u64,
    pub vrs_histogram: *mut [[u64; 16]; 2],
    pub depth_prepass: *const AtomicU32,
    pub start_indices: *const AtomicU32,
    pub start_run: *const AtomicU32,
    /// CyberpunkVR_NativeStereoProbeState.
    pub native_probe_state: *const AtomicU32,
}
/// What the .cpp lends: the exports, the native probe's current list state and the start resets.
#[repr(C)]
pub struct Runtime {
    pub exports: Exports,
    /// cvr_sgp_reset_start (the plugin's native_state_exports, the test's reference).
    pub reset_start: extern "C" fn(),
    /// camera_upload::CurrentBinding's GPU address.
    pub current_binding: extern "C" fn() -> u64,
}
// SAFETY: immutable; its pointers name process-lifetime exports and functions.
unsafe impl Sync for Runtime {}
const _: () = assert!(core::mem::size_of::<Exports>() == 52 * 8 && core::mem::size_of::<GroupContext>() == 32);
/// The plugin's own runtime (formerly lent by src/Render/StereoGpuProbe.cpp): its exports are native_state_exports.rs'
/// and declared_moved_exports.rs' (the differences defined here), the start resets native_state_exports.rs' and the
/// current binding camera_upload.rs'. The Rust test variants still attach their reference definitions through the .cpp.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::{Difference, Exports, Runtime};
    use crate::native_state_exports::*;
    const ZERO: Difference = Difference { target: 0, eye: 0, plane: 0, x: 0, y: 0, reference: 0, shared: 0 };
    #[no_mangle]
    #[allow(non_upper_case_globals)]
    pub static mut CyberpunkVR_StereoGpuProbeDifferences: [Difference; 128] = [ZERO; 128];
    /// camera_upload::CurrentBinding's GPU address.
    extern "C" fn current_binding() -> u64 {
        crate::camera_upload::current_binding().gpu_address
    }
    pub static RUNTIME: Runtime = Runtime {
        exports: Exports {
            request: &raw const CyberpunkVR_StereoGpuProbeRequest,
            state: &raw const CyberpunkVR_StereoGpuProbeState,
            native_reference: &raw const CyberpunkVR_StereoGpuProbeNativeReference,
            reference_skipped: &raw const CyberpunkVR_StereoGpuProbeReferenceSkipped,
            queue_waits: &raw const CyberpunkVR_StereoGpuProbeQueueWaits,
            group: &raw const CyberpunkVR_StereoGpuProbeGroup,
            source_draws: &raw const CyberpunkVR_StereoGpuProbeSourceDraws,
            main_draws: &raw const CyberpunkVR_StereoGpuProbeMainDraws,
            main_view_mask: &raw const CyberpunkVR_StereoGpuProbeMainViewMask,
            main_reference_mask: &raw const CyberpunkVR_StereoGpuProbeMainReferenceMask,
            eye_differences: &raw mut CyberpunkVR_StereoGpuProbeEyeDifferences,
            target_differences: &raw mut CyberpunkVR_StereoGpuProbeTargetDifferences,
            difference_count: &raw mut CyberpunkVR_StereoGpuProbeDifferenceCount,
            differences: &raw mut CyberpunkVR_StereoGpuProbeDifferences,
            prepare_gate_ms: &raw const CyberpunkVR_StereoGpuProbePrepareGateMs,
            prepare_gate_reason: &raw const CyberpunkVR_StereoGpuProbePrepareGateReason,
            late_visibility: &raw const CyberpunkVR_StereoGpuProbeLateVisibility,
            matched_draws: &raw const CyberpunkVR_StereoGpuProbeMatchedDraws,
            fallback_draws: &raw const CyberpunkVR_StereoGpuProbeFallbackDraws,
            prepare_gate_start: &raw const CyberpunkVR_StereoGpuProbePrepareGateStart,
            prepare_gate_main: &raw const CyberpunkVR_StereoGpuProbePrepareGateMain,
            prepare_gate_release: &raw const CyberpunkVR_StereoGpuProbePrepareGateRelease,
            result: (&raw const CyberpunkVR_StereoGpuProbeResult).cast(),
            different_bytes: &raw mut CyberpunkVR_StereoGpuProbeDifferentBytes,
            compared_bytes: &raw mut CyberpunkVR_StereoGpuProbeComparedBytes,
            coverage: &raw mut CyberpunkVR_StereoGpuProbeCoverage,
            scene_depth: &raw const CyberpunkVR_StereoGpuProbeSceneDepth,
            scene_reject: &raw const CyberpunkVR_StereoGpuProbeSceneReject,
            scene_size: &raw mut CyberpunkVR_StereoGpuProbeSceneSize,
            scene_phase: &raw mut CyberpunkVR_StereoGpuProbeScenePhase,
            scene_route: &raw const CyberpunkVR_StereoGpuProbeSceneRoute,
            scene_committed: &raw const CyberpunkVR_StereoGpuProbeSceneCommitted,
            scene_routed: &raw mut CyberpunkVR_StereoGpuProbeSceneRouted,
            scene_samples: &raw mut CyberpunkVR_StereoGpuProbeSceneSamples,
            timing: &raw const CyberpunkVR_StereoGpuProbeTiming,
            timing_drops: &raw const CyberpunkVR_StereoGpuProbeTimingDrops,
            ticks: &raw mut CyberpunkVR_StereoGpuProbeTicks,
            frequency: &raw mut CyberpunkVR_StereoGpuProbeFrequency,
            timed_count: &raw mut CyberpunkVR_StereoGpuProbeTimedCount,
            mixed_materials: &raw const CyberpunkVR_StereoGpuProbeMixedMaterials,
            fine_rate: &raw const CyberpunkVR_StereoGpuProbeFineRate,
            vrs: &raw mut CyberpunkVR_StereoGpuProbeVrs,
            vrs_changes: &raw mut CyberpunkVR_StereoGpuProbeVrsChanges,
            vrs_image: &raw mut CyberpunkVR_StereoGpuProbeVrsImage,
            vrs_size: &raw mut CyberpunkVR_StereoGpuProbeVrsSize,
            vrs_captured: &raw mut CyberpunkVR_StereoGpuProbeVrsCaptured,
            vrs_difference: &raw mut CyberpunkVR_StereoGpuProbeVrsDifference,
            vrs_histogram: &raw mut CyberpunkVR_StereoGpuProbeVrsHistogram,
            depth_prepass: &raw const CyberpunkVR_StereoGpuProbeDepthPrepass,
            start_indices: &raw const CyberpunkVR_StereoGpuProbeStartIndices,
            start_run: &raw const CyberpunkVR_StereoGpuProbeStartRun,
            native_probe_state: &raw const crate::declared_moved_exports::CyberpunkVR_NativeStereoProbeState,
        },
        reset_start: cvr_sgp_reset_start,
        current_binding,
    };
}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    #[cfg(feature = "rust-build")]
    let attached = attached.or(Some(&plugin::RUNTIME));
    attached
}
static ZERO32: AtomicU32 = AtomicU32::new(0);
/// An atomic export (0 while unattached).
macro_rules! a {
    ($field:ident) => {
        // SAFETY: the runtime's export.
        rt().and_then(|r| unsafe { r.exports.$field.as_ref() }).unwrap_or(&ZERO32)
    };
}
/// A plain export table (None while unattached).
macro_rules! t {
    ($field:ident) => {
        // SAFETY: the runtime's export, written under the probe's mutex.
        rt().and_then(|r| unsafe { r.exports.$field.as_mut() })
    };
}
fn state() -> &'static AtomicU32 {
    a!(state)
}

pub const SIZE: u32 = 256;
pub const MAX_GROUP_DRAWS: usize = 128;
const COMMIT_OFFSET: u64 = MAX_GROUP_DRAWS as u64 * 8;
const PREDICATE_BYTES: u64 = COMMIT_OFFSET + 256;
mod stage {
    pub const NATIVE_REFERENCE: u32 = 0;
    pub const COMMON: u32 = 1;
    pub const IMPORT: u32 = 2;
    pub const RESOLVE: u32 = 3;
    pub const COPY_BACK: u32 = 4;
    pub const READBACK: u32 = 5;
    pub const NATIVE_FALLBACK: u32 = 6;
}
const DEPTH_WRITE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_DEPTH_WRITE;
const RENDER_TARGET: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_RENDER_TARGET;
fn target_state(target: usize) -> D3D12_RESOURCE_STATES {
    if target == 3 {
        DEPTH_WRITE
    } else {
        RENDER_TARGET
    }
}
fn now() -> u64 {
    let mut q = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut q) };
    q as u64
}
fn raw<T: Interface>(o: &Option<T>) -> *mut c_void {
    o.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw)
}
fn keep(list: *mut c_void, objects: &[*mut c_void]) -> bool {
    // SAFETY: the list and objects are live.
    unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
}
fn prepare_gate_main() -> Option<&'static AtomicU64> {
    // SAFETY: the runtime's export.
    rt().and_then(|r| unsafe { r.exports.prepare_gate_main.as_ref() })
}
fn result_export() -> Option<&'static AtomicI32> {
    // SAFETY: the runtime's export.
    rt().and_then(|r| unsafe { r.exports.result.as_ref() })
}

/// GroupContext.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupContext {
    pub id: u64,
    pub side: i32,
    pub plane: u32,
    pub last_pipeline: u64,
    pub depth_run: u32,
    pub depth_candidate: bool,
}
const NO_GROUP: GroupContext = GroupContext { id: 0, side: -1, plane: u32::MAX, last_pipeline: 0, depth_run: 0, depth_candidate: false };
thread_local! {
    static CURRENT_GROUP: Cell<GroupContext> = const { Cell::new(NO_GROUP) };
}
fn current_group() -> GroupContext {
    CURRENT_GROUP.with(Cell::get)
}

/// A target array (cvr-gpu's).
struct TargetArray(*mut cvr_gpu::target_array::TargetArray);
impl TargetArray {
    fn new() -> Self {
        TargetArray(cvr_gpu::target_array::cvr_target_array_new())
    }
    fn resource(&self) -> *mut c_void {
        cvr_gpu::target_array::cvr_target_array_resource(self.0)
    }
}
impl Drop for TargetArray {
    fn drop(&mut self) {
        // SAFETY: our array.
        unsafe { cvr_gpu::target_array::cvr_target_array_free(self.0) };
    }
}
struct Surface {
    target: TargetArray,
    planes: u32,
    footprints: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 4],
    row_bytes: [u64; 4],
    rows: [u32; 4],
}
impl Surface {
    fn new() -> Self {
        Surface { target: TargetArray::new(), planes: 1, footprints: Default::default(), row_bytes: [0; 4], rows: [0; 4] }
    }
}
struct GateHandle(*const Gate);
impl GateHandle {
    fn reason(&self) -> u32 {
        // SAFETY: our gate.
        unsafe { crate::gpu_probe_gate::cvr_sgpg_reason(self.0) }
    }
    fn publish(&self, address: *mut u64) -> bool {
        // SAFETY: our gate and a predicate word.
        unsafe { crate::gpu_probe_gate::cvr_sgpg_publish(self.0, address) }
    }
    fn release(&self, why: u32, commit: *mut u64) -> bool {
        // SAFETY: as publish.
        unsafe { crate::gpu_probe_gate::cvr_sgpg_release(self.0, why, commit) }
    }
}
impl Drop for GateHandle {
    fn drop(&mut self) {
        // SAFETY: our gate.
        unsafe { crate::gpu_probe_gate::cvr_sgpg_free(self.0) };
    }
}
struct Work {
    surfaces: [[Surface; 4]; 2],
    scene_depth: bool,
    depth_only: bool,
    anchor_main: bool,
    width: u32,
    height: u32,
    stencil: u32,
    scene_route: bool,
    direct_resolve: bool,
    mixed_materials: bool,
    route_sequence: [u64; 2],
    rate_override: u32,
    rate_seen: [bool; 2],
    vrs_snapshot: *mut cvr_gpu::vrs_snapshot::Snapshot,
    source_vrs: Option<ID3D12Resource>,
    scene_queries: Option<ID3D12QueryHeap>,
    query_offset: u64,
    timestamps: Option<ID3D12QueryHeap>,
    timestamp_offset: u64,
    frequency: [u64; 2],
    timed: Vec<Timed>,
    resolved: [Surface; 4],
    resolver: *mut cvr_gpu::group_resolve::Resolve,
    imported: [bool; 2],
    resolved_eye: [bool; 2],
    native_targets: [[Option<ID3D12Resource>; 4]; 2],
    rtv: Option<ID3D12DescriptorHeap>,
    dsv: Option<ID3D12DescriptorHeap>,
    readback: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    producer_fence: Option<ID3D12Fence>,
    producer_queue: Option<ID3D12CommandQueue>,
    pipeline: Option<ID3D12PipelineState>,
    list: *mut c_void,
    main_list: *mut c_void,
    source_draw: DrawRecord,
    native_reference: bool,
    producer_submitted: bool,
    main_recorded: bool,
    group: bool,
    source_ended: bool,
    main_ended: bool,
    main_submitted: bool,
    rejected: bool,
    source_group: u64,
    main_group: u64,
    plane: u32,
    main_view_mask: u32,
    main_reference_mask: u32,
    preparation_gate: Option<GateHandle>,
    late_visibility: bool,
    predicates: Option<ID3D12Resource>,
    predicate_data: *mut c_void,
    source_items: Vec<DrawRecord>,
    source_instances: Vec<Vec<u8>>,
    matched: [bool; MAX_GROUP_DRAWS],
    rtv_stride: u32,
    dsv_stride: u32,
    bytes: u64,
}
// SAFETY: owned under the probe's mutex; its D3D12 objects are free-threaded.
unsafe impl Send for Work {}
impl Work {
    fn new(draw: &DrawRecord) -> Box<Self> {
        Box::new(Work {
            surfaces: [core::array::from_fn(|_| Surface::new()), core::array::from_fn(|_| Surface::new())],
            scene_depth: false,
            depth_only: false,
            anchor_main: false,
            width: SIZE,
            height: SIZE,
            stencil: 0,
            scene_route: false,
            direct_resolve: false,
            mixed_materials: false,
            route_sequence: [0; 2],
            rate_override: 0,
            rate_seen: [false; 2],
            vrs_snapshot: cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_new(),
            source_vrs: None,
            scene_queries: None,
            query_offset: 0,
            timestamps: None,
            timestamp_offset: 0,
            frequency: [0; 2],
            timed: Vec::with_capacity(512),
            resolved: core::array::from_fn(|_| Surface::new()),
            resolver: cvr_gpu::group_resolve::cvr_group_resolve_new(),
            imported: [false; 2],
            resolved_eye: [false; 2],
            native_targets: Default::default(),
            rtv: None,
            dsv: None,
            readback: None,
            fence: None,
            producer_fence: None,
            producer_queue: None,
            pipeline: None,
            list: core::ptr::null_mut(),
            main_list: core::ptr::null_mut(),
            source_draw: *draw,
            native_reference: false,
            producer_submitted: false,
            main_recorded: false,
            group: false,
            source_ended: false,
            main_ended: false,
            main_submitted: false,
            rejected: false,
            source_group: 0,
            main_group: 0,
            plane: u32::MAX,
            main_view_mask: u32::MAX,
            main_reference_mask: u32::MAX,
            preparation_gate: None,
            late_visibility: false,
            predicates: None,
            predicate_data: core::ptr::null_mut(),
            // SAFETY: plain records.
            source_items: vec![unsafe { core::mem::zeroed::<DrawRecord>() }; MAX_GROUP_DRAWS],
            source_instances: vec![Vec::new(); MAX_GROUP_DRAWS],
            matched: [false; MAX_GROUP_DRAWS],
            rtv_stride: 0,
            dsv_stride: 0,
            bytes: 0,
        })
    }
    fn handle(heap: &Option<ID3D12DescriptorHeap>, index: u32, stride: u32) -> usize {
        // SAFETY: our heap.
        let start = heap.as_ref().map_or(0, |h| unsafe { h.GetCPUDescriptorHandleForHeapStart() }.ptr);
        start + index as usize * stride as usize
    }
    fn rtv_at(&self, mode: u32, surface: u32, eye: u32) -> usize {
        let index = if mode != 0 { (if eye == 1 { 12 } else { 6 }) + surface } else if eye < 2 { eye * 3 + surface } else { 9 + surface };
        Self::handle(&self.rtv, index, self.rtv_stride)
    }
    fn dsv_at(&self, mode: u32, eye: u32) -> usize {
        let index = if mode != 0 { if eye == 1 { 4 } else { 2 } } else if eye < 2 { eye } else { 3 };
        Self::handle(&self.dsv, index, self.dsv_stride)
    }
    fn resolved_rtv(&self, eye: u32, target: u32) -> usize {
        Self::handle(&self.rtv, 15 + eye * 3 + target, self.rtv_stride)
    }
    fn resolved_dsv(&self, eye: u32) -> usize {
        Self::handle(&self.dsv, 5 + eye, self.dsv_stride)
    }
    fn first_target(&self) -> usize {
        if self.depth_only {
            3
        } else {
            0
        }
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        if let (Some(predicates), false) = (self.predicates.as_ref(), self.predicate_data.is_null()) {
            let written = D3D12_RANGE { Begin: 0, End: PREDICATE_BYTES as usize };
            // SAFETY: the mapped upload buffer.
            unsafe { predicates.Unmap(0, Some(&written)) };
        }
        // SAFETY: our objects.
        unsafe {
            cvr_gpu::group_resolve::cvr_group_resolve_free(self.resolver);
            cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_free(self.vrs_snapshot);
        }
    }
}
static WORK: Mutex<Option<Box<Work>>> = Mutex::new(None);
fn lock() -> MutexGuard<'static, Option<Box<Work>>> {
    WORK.lock().unwrap_or_else(|p| p.into_inner())
}
static NEXT_GROUP: AtomicU64 = AtomicU64::new(0);

/// TimingScope: a timestamp pair around a stage of the probe's commands (none without the timing heap; a drop when
/// its 512 records are taken).
struct TimingScope {
    list: *mut c_void,
    heap: *mut c_void,
    readback: *mut c_void,
    offset: u64,
    index: u32,
    active: bool,
}
impl TimingScope {
    fn new(list: *mut c_void, w: &mut Work, stage: u32, eye: u32) -> Self {
        let mut scope = TimingScope { list, heap: raw(&w.timestamps), readback: raw(&w.readback), offset: w.timestamp_offset, index: 0, active: false };
        if scope.heap.is_null() {
            return scope;
        }
        if w.timed.len() == 512 {
            a!(timing_drops).fetch_add(1, SeqCst);
            return scope;
        }
        scope.index = w.timed.len() as u32 * 2;
        w.timed.push(Timed { stage, eye, index: scope.index });
        scope.active = true;
        // SAFETY: the list and our query heap.
        unsafe { crate::gpu_probe_commands::cvr_sgpr_timestamp(list, scope.heap, scope.index, core::ptr::null_mut(), 0) };
        scope
    }
}
impl Drop for TimingScope {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: as new.
            unsafe { crate::gpu_probe_commands::cvr_sgpr_timestamp(self.list, self.heap, self.index + 1, self.readback, self.offset + u64::from(self.index) * 8) };
        }
    }
}
fn fail(hr: HRESULT) {
    if let Some(result) = result_export() {
        result.store(hr.0, SeqCst);
    }
    state().store(4, Release);
}
fn scene_reject(w: &mut Work, reason: u32) -> bool {
    w.rejected = true;
    a!(scene_reject).store(reason, SeqCst);
    false
}
/// Initialize: the RTV/DSV heaps, the two modes' four target arrays with their footprints and views; for a scene the
/// VRS snapshot, the resolved targets, the resolver and the occlusion queries; the timestamps; the readback; the
/// predicates; the fences.
fn initialize(device: &ID3D12Device, w: &mut Work) -> HRESULT {
    let dev = device.as_raw();
    let (mut rtv, mut dsv) = (core::ptr::null_mut(), core::ptr::null_mut());
    // SAFETY: our outputs.
    let hr = HRESULT(unsafe { crate::gpu_probe_scene::cvr_sgpr_heaps(dev, w.scene_depth, &mut rtv, &mut dsv, &mut w.rtv_stride, &mut w.dsv_stride) });
    // SAFETY: references handed over.
    unsafe {
        w.rtv = (!rtv.is_null()).then(|| ID3D12DescriptorHeap::from_raw(rtv));
        w.dsv = (!dsv.is_null()).then(|| ID3D12DescriptorHeap::from_raw(dsv));
    }
    if hr.is_err() {
        return hr;
    }
    let formats = [DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R32G8X24_TYPELESS];
    for mode in 0..2u32 {
        for target in w.first_target()..4 {
            let depth = target == 3;
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: w.width.into(),
                Height: w.height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Format: formats[target],
                Flags: if depth { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET },
                ..Default::default()
            };
            let clear = D3D12_CLEAR_VALUE { Format: if depth { DXGI_FORMAT_D32_FLOAT_S8X24_UINT } else { desc.Format }, ..Default::default() };
            let (eyes, array) = if depth { ([w.dsv_at(mode, 0), w.dsv_at(mode, 1)], w.dsv_at(mode, 2)) } else { ([w.rtv_at(mode, target as u32, 0), w.rtv_at(mode, target as u32, 1)], w.rtv_at(mode, target as u32, 2)) };
            let bytes = w.bytes;
            let surface = &mut w.surfaces[mode as usize][target];
            // SAFETY: our array and description.
            let hr = unsafe { cvr_gpu::target_array::cvr_target_array_initialize(surface.target.0, dev, &desc, target_state(target), &clear) };
            if hr.is_err() {
                return hr;
            }
            surface.planes = cvr_gpu::target_array::cvr_target_array_planes(surface.target.0);
            let resource = surface.target.resource();
            // SAFETY: our outputs.
            w.bytes = unsafe { crate::gpu_probe_commands::cvr_sgpr_footprints(dev, resource, surface.planes, bytes, &mut surface.footprints, &mut surface.rows, &mut surface.row_bytes) };
            let eyes = eyes.map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
            // SAFETY: our resource and heaps.
            unsafe { crate::gpu_probe_commands::cvr_sgpr_views(dev, resource, depth, desc.Format.0, array, &eyes, mode) };
        }
    }
    if w.scene_depth {
        if let Some(source) = w.source_vrs.as_ref() {
            // SAFETY: our snapshot over the native image.
            let hr = unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_initialize(w.vrs_snapshot, dev, source.as_raw()) };
            if hr.is_err() {
                return hr;
            }
            let (mut width, mut height) = (0u32, 0u32);
            unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_info(w.vrs_snapshot, core::ptr::null_mut(), &mut width, &mut height) };
            if let Some(size) = t!(vrs_size) {
                *size = [width, height];
            }
        }
        if !w.direct_resolve {
            for target in w.first_target()..4 {
                let depth = target == 3;
                let source = w.surfaces[0][target].target.resource();
                // SAFETY: our resource.
                let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&source) }) else { return E_FAIL };
                let mut desc = unsafe { resource.GetDesc() };
                desc.DepthOrArraySize = 1;
                // SAFETY: our array.
                let hr = unsafe { cvr_gpu::target_array::cvr_target_array_initialize(w.resolved[target].target.0, dev, &desc, target_state(target), core::ptr::null()) };
                if hr.is_err() {
                    return hr;
                }
                let eyes = if depth { [w.resolved_dsv(0), w.resolved_dsv(1)] } else { [w.resolved_rtv(0, target as u32), w.resolved_rtv(1, target as u32)] }.map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
                unsafe { crate::gpu_probe_commands::cvr_sgpr_views(dev, w.resolved[target].target.resource(), depth, desc.Format.0, 0, &eyes, 0) };
            }
        }
        let sources = [0, 1, 2, 3].map(|t| w.surfaces[1][t].target.resource());
        // SAFETY: our resolver over our arrays.
        let hr = unsafe { cvr_gpu::group_resolve::cvr_group_resolve_initialize(w.resolver, dev, &sources, w.depth_only) };
        if hr.is_err() {
            return hr;
        }
        let mut heap = core::ptr::null_mut();
        let hr = HRESULT(unsafe { crate::gpu_probe_scene::cvr_sgpr_queries(dev, D3D12_QUERY_HEAP_TYPE_OCCLUSION.0, 2, &mut w.bytes, &mut w.query_offset, &mut heap) });
        // SAFETY: a reference handed over.
        w.scene_queries = (!heap.is_null()).then(|| unsafe { ID3D12QueryHeap::from_raw(heap) });
        if hr.is_err() {
            return hr;
        }
    }
    if a!(timing).load(Relaxed) != 0 {
        let mut heap = core::ptr::null_mut();
        let hr = HRESULT(unsafe { crate::gpu_probe_scene::cvr_sgpr_queries(dev, D3D12_QUERY_HEAP_TYPE_TIMESTAMP.0, 1024, &mut w.bytes, &mut w.timestamp_offset, &mut heap) });
        // SAFETY: a reference handed over.
        w.timestamps = (!heap.is_null()).then(|| unsafe { ID3D12QueryHeap::from_raw(heap) });
        if hr.is_err() {
            return hr;
        }
    }
    let mut readback = core::ptr::null_mut();
    // SAFETY: our output.
    let hr = unsafe { crate::readback_buffers::cvr_rb_buffer(dev, core::ptr::null_mut(), w.bytes, D3D12_HEAP_TYPE_READBACK.0, D3D12_RESOURCE_STATE_COPY_DEST.0, &mut readback) };
    w.readback = (!readback.is_null()).then(|| unsafe { ID3D12Resource::from_raw(readback) });
    if hr.is_err() {
        return hr;
    }
    if w.late_visibility {
        let mut predicates = core::ptr::null_mut();
        // SAFETY: our outputs.
        let hr = unsafe { crate::readback_buffers::cvr_rb_mapped_upload(dev, PREDICATE_BYTES, &mut predicates, &mut w.predicate_data) };
        if hr.is_err() {
            return hr;
        }
        w.predicates = (!predicates.is_null()).then(|| unsafe { ID3D12Resource::from_raw(predicates) });
    }
    match unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) } {
        Ok(f) => w.producer_fence = Some(f),
        Err(e) => return e.code(),
    }
    match unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) } {
        Ok(f) => w.fence = Some(f),
        Err(e) => return e.code(),
    }
    S_OK
}
fn same_geometry(a: &DrawRecord, b: &DrawRecord, instances_a: &[u8], instances_b: &[u8]) -> bool {
    // SAFETY: our records.
    unsafe {
        if !crate::gpu_probe_checks::cvr_sgpp_same_fields(a, b) {
            return false;
        }
        if !instances_a.is_empty() || !instances_b.is_empty() {
            let (sa, sb) = (a.vertices[7].StrideInBytes, b.vertices[7].StrideInBytes);
            if !crate::instance_data::cvr_id_equal(a.instances, sa, instances_a.as_ptr(), instances_a.len(), b.instances, sb, instances_b.as_ptr(), instances_b.len()) {
                return false;
            }
        } else if !crate::gpu_probe_checks::cvr_sgpp_inline_instances(a, b) {
            return false;
        }
        crate::gpu_probe_checks::cvr_sgpp_vertex_views(a, b)
    }
}
fn retain_surfaces(list: *mut c_void, w: &Work) -> bool {
    keep(list, &[raw(&w.rtv), raw(&w.dsv), raw(&w.readback), raw(&w.pipeline), w.surfaces[0][3].target.resource(), w.surfaces[1][3].target.resource()])
        && (w.depth_only
            || keep(list, &[w.surfaces[0][0].target.resource(), w.surfaces[0][1].target.resource(), w.surfaces[0][2].target.resource(), w.surfaces[1][0].target.resource(), w.surfaces[1][1].target.resource(), w.surfaces[1][2].target.resource()]))
}
fn same_pass(a: &DrawRecord, b: &DrawRecord, mixed: bool) -> bool {
    // SAFETY: our records.
    unsafe { crate::gpu_probe_checks::cvr_sgpp_same_pass(a, b, mixed) }
}
fn copy_mode(list: *mut c_void, w: &mut Work, mode: usize, force: bool) {
    if w.scene_depth && !force {
        return;
    }
    let eye = (current_group().side == 0 || (w.main_recorded && list == w.main_list)) as u32;
    let _timing = TimingScope::new(list, w, stage::READBACK, eye);
    for target in w.first_target()..4 {
        let surface = &w.surfaces[mode][target];
        let resource = if w.scene_depth && mode != 0 { w.resolved[target].target.resource() } else { surface.target.resource() };
        // SAFETY: the list, our resources and footprints.
        unsafe { crate::gpu_probe_commands::cvr_sgpr_copy(list, resource, target_state(target).0, raw(&w.readback), surface.footprints.as_ptr(), 0, 1, surface.planes * 2, w.group) };
    }
}
fn restore(list: *mut c_void, draw: &DrawRecord) {
    // SAFETY: the list and its native draw.
    unsafe { crate::gpu_probe_commands::cvr_sgpr_restore(list, draw) };
}
/// FineRateScope: the eye's fine shading rate for the probe's commands (a scene reject when it cannot hold).
struct FineRateScope {
    state: FineRate,
    ok: bool,
}
impl FineRateScope {
    fn new(list: *mut c_void, w: &mut Work, eye: u32) -> Self {
        let exports = FineExports {
            vrs: rt().map_or(core::ptr::null_mut(), |r| r.exports.vrs),
            changes: rt().map_or(core::ptr::null_mut(), |r| r.exports.vrs_changes),
            images: rt().map_or(core::ptr::null_mut(), |r| r.exports.vrs_image),
        };
        let mut scope = FineRateScope { state: unsafe { core::mem::zeroed() }, ok: true };
        let source = if w.rate_override == 2 {
            let mut frozen = core::ptr::null_mut();
            // SAFETY: our snapshot.
            unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_info(w.vrs_snapshot, &mut frozen, core::ptr::null_mut(), core::ptr::null_mut()) };
            frozen
        } else {
            core::ptr::null_mut()
        };
        // SAFETY: the list, our seen flag, outputs and exports.
        let reject = unsafe { crate::gpu_probe_scene::cvr_sgpr_fine_rate(list, w.scene_depth, eye, &mut w.rate_seen[eye as usize], w.rate_override, source, &exports, &mut scope.state) };
        if reject != 0 {
            scope.ok = scene_reject(w, reject);
        }
        scope
    }
}
impl Drop for FineRateScope {
    fn drop(&mut self) {
        // SAFETY: our state.
        unsafe { crate::gpu_probe_scene::cvr_sgpr_fine_rate_end(&mut self.state) };
    }
}
fn snapshot(list: *mut c_void) -> Option<Bindings> {
    let mut b = Bindings::default();
    // SAFETY: the list; our output.
    unsafe { cvr_gpu::scene_state::cvr_scene_snapshot(list as usize, &mut b) }.then_some(b)
}
fn scene_bindings(list: *mut c_void, w: &mut Work) -> bool {
    if !w.scene_depth {
        return true;
    }
    snapshot(list).is_some_and(|b| b.stencil == w.stencil) || scene_reject(w, 9)
}
fn capture_targets(draw: &DrawRecord) -> Option<[Option<ID3D12Resource>; 4]> {
    let targets = cvr_gpu::scene_state::DrawTargets { rt_count: draw.rt_count, rt_contiguous: draw.rt_contiguous, targets: draw.targets, depth: draw.depth, viewport: draw.viewport };
    let mut out = [0usize; 4];
    // SAFETY: our input and output; the references are handed over.
    if !unsafe { cvr_gpu::scene_state::cvr_scene_capture_targets(&targets, &mut out) } {
        return None;
    }
    Some(out.map(|p| (p != 0).then(|| unsafe { ID3D12Resource::from_raw(p as *mut c_void) })))
}
fn import_scene(list: *mut c_void, w: &mut Work, draw: &DrawRecord, eye: usize) -> bool {
    let Some(bindings) = snapshot(list).filter(|b| b.stencil == w.stencil) else { return scene_reject(w, 2) };
    if w.native_targets[eye][3].is_none() {
        match capture_targets(draw) {
            Some(t) => w.native_targets[eye] = t,
            None => return scene_reject(w, 3),
        }
    }
    if w.source_vrs.is_some() {
        // SAFETY: our snapshot, the list and its rate image.
        if !bindings.rate.known || bindings.rate.image == 0 || unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_capture(w.vrs_snapshot, list, bindings.rate.image as *mut c_void, eye as u32) }.is_err() {
            return scene_reject(w, 13);
        }
        if let Some(captured) = t!(vrs_captured) {
            captured[eye] = 1;
        }
    }
    let r: [*mut c_void; 4] = core::array::from_fn(|i| raw(&w.native_targets[eye][i]));
    // SAFETY: our resolver and the list.
    if !unsafe { cvr_gpu::group_resolve::cvr_group_resolve_retain(w.resolver, list) } || !keep(list, &[raw(&w.scene_queries), raw(&w.predicates), r[3]]) {
        return scene_reject(w, 4);
    }
    if !w.depth_only && !keep(list, &[r[0], r[1], r[2]]) {
        return scene_reject(w, 4);
    }
    if w.direct_resolve {
        let rtvs = [w.resolved_rtv(eye as u32, 0), w.resolved_rtv(eye as u32, 1), w.resolved_rtv(eye as u32, 2)].map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
        // SAFETY: the list, the native targets and our views.
        if !unsafe { crate::gpu_probe_scene::cvr_sgpr_direct_views(list, &r, &rtvs, w.resolved_dsv(eye as u32), w.depth_only) } {
            return scene_reject(w, 11);
        }
    } else if !keep(list, &[w.resolved[3].target.resource()]) || (!w.depth_only && !keep(list, &[w.resolved[0].target.resource(), w.resolved[1].target.resource(), w.resolved[2].target.resource()])) {
        return scene_reject(w, 4);
    }
    let _timing = TimingScope::new(list, w, stage::IMPORT, eye as u32);
    for target in w.first_target()..4 {
        let state = target_state(target);
        // SAFETY: our arrays, the list and the native target.
        let failed = cvr_gpu::target_array::cvr_target_array_import_eye(w.surfaces[0][target].target.0, list, eye as u32, state, r[target], state).is_err()
            || (!w.direct_resolve && cvr_gpu::target_array::cvr_target_array_import_eye(w.resolved[target].target.0, list, eye as u32, state, r[target], state).is_err());
        if failed {
            return scene_reject(w, 5);
        }
    }
    w.imported[eye] = true;
    if let Some(phase) = t!(scene_phase) {
        phase[eye] = 1;
    }
    true
}
fn copy_native_eye(list: *mut c_void, w: &mut Work, eye: usize) {
    let _timing = TimingScope::new(list, w, stage::READBACK, eye as u32);
    for t in w.first_target()..4 {
        let surface = &w.surfaces[1][t];
        // SAFETY: the list, the native target, our readback and footprints.
        unsafe { crate::gpu_probe_commands::cvr_sgpr_copy(list, raw(&w.native_targets[eye][t]), target_state(t).0, raw(&w.readback), surface.footprints.as_ptr(), eye as u32, 2, surface.planes, true) };
    }
}
/// A command list method through the vtable (the port's hooks included, as the .cpp's calls).
macro_rules! vcall {
    ($list:expr, $slot:expr, fn($($t:ty),*), ($($a:expr),*)) => {{
        let list: *mut c_void = $list;
        // SAFETY: the live list's vtable method of this signature.
        let f: unsafe extern "system" fn(*mut c_void $(, $t)*) = unsafe { core::mem::transmute(*(*(list as *const *const usize)).add($slot)) };
        unsafe { f(list $(, $a)*) }
    }};
}
fn set_predication(list: *mut c_void, resource: *mut c_void, offset: u64) {
    vcall!(list, 55, fn(*mut c_void, u64, i32), (resource, offset, D3D12_PREDICATION_OP_EQUAL_ZERO.0));
}
fn finish_scene(list: *mut c_void, w: &mut Work, draw: &DrawRecord, eye: usize) {
    if !w.scene_depth || w.resolved_eye[eye] || !w.imported[eye] || w.rejected {
        return;
    }
    let Some(bindings) = snapshot(list) else {
        scene_reject(w, 6);
        return;
    };
    let _scope = cvr_host::probe::Internal::new();
    let e = eye as u32;
    let targets = [w.resolved_rtv(e, 0), w.resolved_rtv(e, 1), w.resolved_rtv(e, 2)].map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
    let queries = raw(&w.scene_queries);
    if w.direct_resolve {
        set_predication(list, raw(&w.predicates), COMMIT_OFFSET);
    }
    {
        let _timing = TimingScope::new(list, w, stage::RESOLVE, e);
        vcall!(list, 52, fn(*mut c_void, i32, u32), (queries, D3D12_QUERY_TYPE_OCCLUSION.0, e));
        // SAFETY: our resolver, the list and the source draw's viewport and scissor.
        unsafe { cvr_gpu::group_resolve::cvr_group_resolve_record(w.resolver, list, e, w.stencil, &w.source_draw.viewport, &w.source_draw.scissor, &targets, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: w.resolved_dsv(e) }) };
        vcall!(list, 53, fn(*mut c_void, i32, u32), (queries, D3D12_QUERY_TYPE_OCCLUSION.0, e));
    }
    if w.direct_resolve {
        set_predication(list, core::ptr::null_mut(), 0);
    }
    vcall!(list, 54, fn(*mut c_void, i32, u32, u32, *mut c_void, u64), (queries, D3D12_QUERY_TYPE_OCCLUSION.0, e, 1, raw(&w.readback), w.query_offset + u64::from(e) * 8));
    if w.scene_route && !w.direct_resolve {
        let _timing = TimingScope::new(list, w, stage::COPY_BACK, e);
        set_predication(list, raw(&w.predicates), COMMIT_OFFSET);
        for t in w.first_target()..4 {
            let state = target_state(t);
            // SAFETY: our array, the list and the native target.
            if cvr_gpu::target_array::cvr_target_array_copy_eye(w.resolved[t].target.0, list, e, state, raw(&w.native_targets[eye][t]), state, state).is_err() {
                scene_reject(w, 10);
            }
        }
        set_predication(list, core::ptr::null_mut(), 0);
    }
    if w.scene_route {
        copy_native_eye(list, w, eye);
    }
    // SAFETY: the snapshot's bindings back onto the list.
    unsafe { cvr_gpu::scene_state::cvr_scene_bindings_restore(&bindings, list) };
    restore(list, draw);
    w.resolved_eye[eye] = true;
    if let Some(phase) = t!(scene_phase) {
        phase[eye] = 2;
    }
    // Both eyes of the readback are written together only after MAIN's prefix.
    if eye == 1 {
        copy_mode(list, w, 0, true);
        if !w.scene_route {
            copy_mode(list, w, 1, true);
        }
    }
}
fn reference(list: *mut c_void, w: &mut Work, draw: &DrawRecord, eye: u32, mode: u32) {
    let rate = FineRateScope::new(list, w, eye);
    if !rate.ok {
        return;
    }
    let _timing = TimingScope::new(list, w, if mode != 0 { stage::NATIVE_FALLBACK } else { stage::NATIVE_REFERENCE }, eye);
    let targets = [w.rtv_at(mode, 0, eye), w.rtv_at(mode, 1, eye), w.rtv_at(mode, 2, eye)].map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
    // SAFETY: the list, the native draw and our views.
    unsafe { crate::gpu_probe_commands::cvr_sgpr_reference(list, draw, w.scene_depth, w.width, w.height, &targets, w.dsv_at(mode, eye), w.depth_only) };
}
fn shared_draw(list: *mut c_void, views: *mut c_void, w: &mut Work, draw: &DrawRecord, ordinal: u32, instances: &[u8]) {
    let rate = FineRateScope::new(list, w, 0);
    if !rate.ok {
        return;
    }
    let _timing = TimingScope::new(list, w, stage::COMMON, 0);
    if w.late_visibility {
        w.source_items[ordinal as usize] = *draw;
        w.source_instances[ordinal as usize] = instances.to_vec();
    }
    // Late visibility: exactly one predicated branch executes -- shared for a pair, the mono pipeline on slice zero
    // otherwise; else one draw under the group's view mask.
    let mask = if w.late_visibility || !w.group || w.main_view_mask & 1u32.wrapping_shl(ordinal) != 0 { 3 } else { 1 };
    let predicates = if w.late_visibility { raw(&w.predicates) } else { core::ptr::null_mut() };
    // SAFETY: the list, its view-instancing interface, our predicates and the native draw.
    unsafe { crate::gpu_probe_commands::cvr_sgpr_shared(list, views, predicates, u64::from(ordinal) * 8, mask, draw) };
}
fn finish_group_if_submitted(w: &Work) {
    if w.group && w.source_ended && w.main_ended && w.main_submitted && !w.rejected {
        state().store(2, Release);
    }
}
fn current_binding_gpu() -> u64 {
    rt().map_or(0, |r| (r.current_binding)())
}

// ---------------------------------------------------------------------------------------------------------------------
// The entry points
// ---------------------------------------------------------------------------------------------------------------------
pub fn boundary(list: *mut c_void, draw: &DrawRecord, require_pipeline_change: bool, candidate_variant: *mut c_void) {
    let mut group = current_group();
    if require_pipeline_change && group.id != 0 && draw.side == 1 && a!(depth_prepass).load(Relaxed) != 0 {
        let candidate = !candidate_variant.is_null() && draw.rt_count == 0;
        if candidate && (!group.depth_candidate || group.last_pipeline != draw.pipeline) {
            group.depth_run += 1;
        }
        group.depth_candidate = candidate;
        group.last_pipeline = draw.pipeline;
        CURRENT_GROUP.with(|c| c.set(group));
    }
    if state().load(Acquire) != 1 {
        return;
    }
    let mut guard = lock();
    let Some(w) = guard.as_deref_mut().filter(|w| w.scene_depth && !w.rejected) else { return };
    let eye = if draw.side == 1 { 0 } else { 1 };
    if group.id != (if eye != 0 { w.main_group } else { w.source_group }) || list != (if eye != 0 { w.main_list } else { w.list }) {
        return;
    }
    if require_pipeline_change {
        let same_stencil = snapshot(list).is_some_and(|b| b.stencil == w.stencil);
        // SAFETY: a pipeline the engine bound.
        let compatible = || !candidate_variant.is_null() && cvr_gpu::scene_state::cvr_scene_compatible(draw.pipeline as usize);
        if same_stencil && same_pass(&w.source_draw, draw, w.mixed_materials) && (draw.pipeline == w.source_draw.pipeline || compatible()) {
            return;
        }
    }
    finish_scene(list, w, draw, eye);
}
pub fn route_native(list: *mut c_void, draw: &DrawRecord) -> bool {
    if state().load(Acquire) != 1 || a!(scene_route).load(Relaxed) == 0 {
        return false;
    }
    let mut guard = lock();
    let Some(w) = guard.as_deref_mut().filter(|w| w.scene_route && !w.rejected) else { return false };
    if draw.side > 1 || draw.sequence == 0 {
        return false;
    }
    let eye = if draw.side == 1 { 0 } else { 1 };
    if w.route_sequence[eye] != draw.sequence || (if eye != 0 { w.main_group } else { w.source_group }) != current_group().id {
        return false;
    }
    if !keep(list, &[raw(&w.predicates)]) {
        scene_reject(w, 4);
        return false;
    }
    let _scope = cvr_host::probe::Internal::new();
    // The native draw remains recorded as the fallback; it is suppressed only when MAIN completes validation and
    // publishes the frame-wide commit flag.
    // SAFETY: the list, our predicates and the native draw.
    unsafe { crate::gpu_probe_commands::cvr_sgpr_routed(list, raw(&w.predicates), COMMIT_OFFSET, draw) };
    if let Some(routed) = t!(scene_routed) {
        routed[eye] += 1;
    }
    true
}
pub fn begin_group(side: i32, plane: u32) -> GroupContext {
    let previous = current_group();
    CURRENT_GROUP.with(|c| c.set(GroupContext { id: NEXT_GROUP.fetch_add(1, Relaxed) + 1, side, plane, ..NO_GROUP }));
    if side == 0 && state().load(Acquire) == 1 {
        let guard = lock();
        if let Some(w) = guard.as_deref().filter(|w| w.group && w.plane == plane && w.source_ended) {
            if let Some(gate) = w.preparation_gate.as_ref() {
                let now = now();
                if prepare_gate_main().is_some_and(|m| m.compare_exchange(0, now, SeqCst, SeqCst).is_ok()) && !w.late_visibility {
                    gate.release(1, core::ptr::null_mut());
                }
            }
        }
    }
    previous
}
pub fn end_group(previous: GroupContext) {
    let ended = current_group();
    CURRENT_GROUP.with(|c| c.set(previous));
    if state().load(Acquire) != 1 {
        return;
    }
    let mut guard = lock();
    let Some(w) = guard.as_deref_mut().filter(|w| w.group) else { return };
    if w.scene_depth && (ended.id == w.source_group || ended.id == w.main_group) {
        let eye = if ended.id == w.source_group { 0 } else { 1 };
        let list = if eye != 0 { w.main_list } else { w.list };
        // SAFETY: plain record.
        let mut saved: DrawRecord = unsafe { core::mem::zeroed() };
        if !w.resolved_eye[eye] && (list.is_null() || !(rt().is_some() && unsafe { crate::stereo_probe_state::cvr_nsps_current(list, &mut saved) })) {
            scene_reject(w, 7);
        }
        if !list.is_null() {
            finish_scene(list, w, &saved, eye);
        }
        if !w.resolved_eye[eye] {
            scene_reject(w, 7);
        }
    }
    if ended.id == w.source_group {
        w.source_ended = true;
    }
    if ended.id == w.main_group {
        w.main_ended = true;
        if w.late_visibility {
            if let Some(gate) = w.preparation_gate.as_ref() {
                let commit = if w.scene_route && !w.rejected { w.predicate_data.cast::<u64>().wrapping_add(MAX_GROUP_DRAWS) } else { core::ptr::null_mut() };
                if !gate.release(if w.rejected { 4 } else { 1 }, commit) {
                    w.rejected = true;
                }
            }
        }
    }
    finish_group_if_submitted(w);
}
/// Draw: a source draw of the group (the shared draw), a MAIN draw of the native reference, or the probe's start.
pub fn draw(list: *mut c_void, variant: *mut c_void, draw: &DrawRecord, instances: &[u8]) {
    let group = current_group();
    let raw_list = list;
    // SAFETY: the engine's live list.
    let Some(l) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw_list) }) else { return };
    if draw.side == 1 && a!(group).load(Relaxed) != 0 && state().load(Acquire) == 1 {
        let mut guard = lock();
        let Some(w) = guard.as_deref_mut() else { return };
        if !w.group || group.id != w.source_group || w.source_ended || w.rejected {
            return;
        }
        if !w.mixed_materials && draw.pipeline != w.source_draw.pipeline {
            return;
        }
        if w.scene_depth && w.resolved_eye[0] {
            return;
        }
        let ordinal = a!(source_draws).load(Relaxed);
        // This bounded test aggregates one native command list per eye: never append after submission/reset, nor
        // silently compare a partial group.
        let limit = if w.late_visibility { MAX_GROUP_DRAWS as u32 } else { 32 };
        if ordinal >= limit || w.producer_submitted || list != w.list || !same_pass(&w.source_draw, draw, w.mixed_materials) || current_binding_gpu() == 0 || variant.is_null() {
            w.rejected = true;
            return;
        }
        let views = match l.cast::<ID3D12GraphicsCommandList1>() {
            Ok(v) => v,
            Err(e) => {
                fail(e.code());
                return;
            }
        };
        if !scene_bindings(list, w) {
            return;
        }
        if !keep(list, &[variant]) {
            fail(E_OUTOFMEMORY);
            return;
        }
        let _scope = cvr_host::probe::Internal::new();
        reference(list, w, draw, 0, 0);
        let targets = [w.rtv_at(1, 0, 2), w.rtv_at(1, 1, 2), w.rtv_at(1, 2, 2)].map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
        let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: w.dsv_at(1, 2) };
        vcall!(list, 25, fn(*mut c_void), (variant));
        let (count, rts) = if w.depth_only { (0, core::ptr::null()) } else { (3, targets.as_ptr()) };
        vcall!(list, 46, fn(u32, *const D3D12_CPU_DESCRIPTOR_HANDLE, i32, *const D3D12_CPU_DESCRIPTOR_HANDLE), (count, rts, 0, &depth));
        shared_draw(list, views.as_raw(), w, draw, ordinal, instances);
        copy_mode(list, w, 1, false);
        vcall!(views.as_raw(), 65, fn(u32), (u32::MAX));
        restore(list, draw);
        w.route_sequence[0] = draw.sequence;
        a!(source_draws).fetch_add(1, SeqCst);
        return;
    }
    if draw.side == 0 && a!(native_reference).load(Relaxed) != 0 && state().load(Acquire) == 1 {
        let mut guard = lock();
        let Some(w) = guard.as_deref_mut() else { return };
        if !w.native_reference || w.rejected || draw.flags & 127 != 63 || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
            return;
        }
        if w.group {
            if group.id == 0 || group.side != 0 || group.plane != w.plane || (w.main_group != 0 && group.id != w.main_group) || (!w.mixed_materials && draw.pipeline != w.source_draw.pipeline) || draw.frame != w.source_draw.frame {
                return;
            }
            // Native sorting differs between eyes: a later MAIN prefix starts at its first exact member of the
            // captured source group, not merely the first source object (which may sort last or be culled).
            if w.anchor_main && w.main_group == 0 {
                let count = a!(source_draws).load(Relaxed) as usize;
                if !(0..count).any(|i| same_geometry(&w.source_items[i], draw, &w.source_instances[i], instances)) {
                    return;
                }
            }
            if w.scene_depth && w.resolved_eye[1] {
                return;
            }
            if !same_pass(&w.source_draw, draw, w.mixed_materials) {
                w.rejected = true;
                return;
            }
            let limit = if w.late_visibility { MAX_GROUP_DRAWS as u32 } else { 32 };
            if a!(main_draws).load(Relaxed) >= limit || !w.source_ended || !w.producer_submitted || w.main_submitted || (!w.main_list.is_null() && w.main_list != list) {
                w.rejected = true;
                a!(reference_skipped).fetch_add(1, SeqCst);
                return;
            }
            w.main_group = group.id;
        } else if w.main_recorded || !same_geometry(&w.source_draw, draw, &[], &[]) {
            return;
        }
        // Recording MAIN can precede GPU completion: once the producer has been submitted and its fence enqueued,
        // BeforeSubmit establishes the ordering.
        if !w.producer_submitted {
            a!(reference_skipped).fetch_add(1, SeqCst);
            return;
        }
        if !retain_surfaces(list, w) {
            fail(E_OUTOFMEMORY);
            return;
        }
        if w.timestamps.is_some() && !keep(list, &[raw(&w.timestamps)]) {
            fail(E_OUTOFMEMORY);
            return;
        }
        if !scene_bindings(list, w) {
            return;
        }
        let _scope = cvr_host::probe::Internal::new();
        if w.scene_depth && !w.imported[1] && !import_scene(list, w, draw, 1) {
            return;
        }
        let ordinal = a!(main_draws).load(Relaxed);
        let mut matched = false;
        if w.late_visibility {
            if w.preparation_gate.as_ref().is_none_or(|g| g.reason() != 0) {
                w.rejected = true;
                return;
            }
            let count = a!(source_draws).load(Relaxed) as usize;
            for i in 0..count {
                if !w.matched[i] && same_geometry(&w.source_items[i], draw, &w.source_instances[i], instances) {
                    let word = w.predicate_data.cast::<u64>().wrapping_add(i);
                    if !w.preparation_gate.as_ref().is_some_and(|g| g.publish(word)) {
                        w.rejected = true;
                        return;
                    }
                    w.matched[i] = true;
                    matched = true;
                    a!(matched_draws).fetch_add(1, SeqCst);
                    break;
                }
            }
        }
        if !w.group || w.main_reference_mask == u32::MAX || (ordinal < 32 && w.main_reference_mask & (1 << ordinal) != 0) {
            reference(list, w, draw, 1, 0);
        }
        if w.late_visibility {
            if !matched {
                reference(list, w, draw, 1, 1);
                a!(fallback_draws).fetch_add(1, SeqCst);
            }
            copy_mode(list, w, 1, false);
        }
        copy_mode(list, w, 0, false);
        restore(list, draw);
        w.main_list = list;
        w.main_recorded = true;
        w.route_sequence[1] = draw.sequence;
        a!(main_draws).fetch_add(1, SeqCst);
        return;
    }
    let group_on = a!(group).load(Relaxed) != 0;
    // SAFETY: the native draw.
    let depth_prepass = a!(depth_prepass).load(SeqCst) != 0;
    let start = || unsafe { crate::gpu_probe_settings::cvr_sgps_start(draw, group_on, depth_prepass) };
    if a!(request).load(Relaxed) == 0 || state().load(Acquire) != 0 || !start() || variant.is_null() || current_binding_gpu() == 0 || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return;
    }
    let start_indices = a!(start_indices).load(Relaxed);
    let start_run = a!(start_run).load(Relaxed);
    if start_indices != 0 && draw.indices != start_indices {
        return;
    }
    if start_run != 0 && group.depth_run != start_run {
        return;
    }
    let mut guard = lock();
    if state().load(SeqCst) != 0 || a!(request).swap(0, SeqCst) == 0 {
        return;
    }
    let late = a!(late_visibility).load(SeqCst) != 0;
    let scene = a!(scene_depth).load(SeqCst) != 0;
    let route_mode = a!(scene_route).load(SeqCst);
    let mixed = a!(mixed_materials).load(SeqCst) != 0;
    let rate_override = a!(fine_rate).load(SeqCst);
    let depth_only = a!(depth_prepass).load(SeqCst) != 0;
    let native_reference = a!(native_reference).load(SeqCst) != 0;
    let settings = crate::gpu_probe_settings::Settings {
        group_id: group.id,
        group_side: group.side,
        group_plane: group.plane,
        route_mode,
        rate_override,
        start_indices,
        start_run,
        prepare_gate_ms: a!(prepare_gate_ms).load(SeqCst),
        main_view_mask: a!(main_view_mask).load(SeqCst),
        main_reference_mask: a!(main_reference_mask).load(SeqCst),
        group: group_on,
        late,
        scene,
        mixed,
        depth_only,
        native_reference,
    };
    // SAFETY: our settings. The rate override is diagnosis only; visible native VRS stays unchanged.
    if !unsafe { crate::gpu_probe_settings::cvr_sgps_settings(&settings) } {
        fail(E_INVALIDARG);
        return;
    }
    if let Some(r) = rt() {
        (r.reset_start)();
    }
    let _scope = cvr_host::probe::Internal::new();
    let mut device: Option<ID3D12Device> = None;
    if let Err(e) = unsafe { l.GetDevice(&mut device) } {
        fail(e.code());
        return;
    }
    let Some(device) = device else {
        fail(E_FAIL);
        return;
    };
    let views = match l.cast::<ID3D12GraphicsCommandList1>() {
        Ok(v) => v,
        Err(e) => {
            fail(e.code());
            return;
        }
    };
    let mut pending = Work::new(draw);
    let w = &mut *pending;
    let raw_variant = variant;
    // SAFETY: the engine's variant pipeline, referenced for the probe.
    w.pipeline = unsafe { ID3D12PipelineState::from_raw_borrowed(&raw_variant) }.cloned();
    w.list = list;
    w.native_reference = native_reference;
    (w.group, w.source_group, w.plane) = (group_on, group.id, group.plane);
    (w.late_visibility, w.scene_depth) = (late, scene);
    (w.scene_route, w.direct_resolve) = (route_mode != 0, route_mode == 2);
    w.route_sequence[0] = draw.sequence;
    (w.mixed_materials, w.rate_override, w.depth_only) = (mixed, rate_override, depth_only);
    w.anchor_main = start_indices != 0 || start_run > 1;
    if scene {
        // SAFETY: a pipeline the engine bound.
        if !cvr_gpu::scene_state::cvr_scene_compatible(draw.pipeline as usize) {
            a!(scene_reject).store(1, SeqCst);
            fail(E_NOTIMPL);
            return;
        }
        let Some(bindings) = snapshot(list) else {
            a!(scene_reject).store(2, SeqCst);
            fail(E_NOTIMPL);
            return;
        };
        if !depth_only && bindings.rate.known && bindings.rate.image != 0 {
            let image = bindings.rate.image as *mut c_void;
            // SAFETY: the bound rate image, referenced for the probe.
            w.source_vrs = unsafe { ID3D12Resource::from_raw_borrowed(&image) }.cloned();
        }
        match capture_targets(draw) {
            Some(t) => w.native_targets[0] = t,
            None => {
                a!(scene_reject).store(3, SeqCst);
                fail(E_NOTIMPL);
                return;
            }
        }
        (w.width, w.height, w.stencil) = (draw.viewport.Width as u32, draw.viewport.Height as u32, bindings.stencil);
        if let Some(size) = t!(scene_size) {
            *size = [w.width, w.height];
        }
    }
    w.main_view_mask = a!(main_view_mask).load(SeqCst);
    w.main_reference_mask = a!(main_reference_mask).load(SeqCst);
    let hr = initialize(&device, w);
    if hr.is_err() {
        fail(hr);
        return;
    }
    if !retain_surfaces(list, w) || (w.predicates.is_some() && !keep(list, &[raw(&w.predicates)])) || (w.timestamps.is_some() && !keep(list, &[raw(&w.timestamps)])) {
        fail(E_OUTOFMEMORY);
        return;
    }
    // The native list already owns all original material/mesh/root bindings; only the pipeline, targets, viewport,
    // scissor and view mask change, and are restored.
    // SAFETY: the list, the variant and the native draw.
    unsafe { crate::gpu_probe_commands::cvr_sgpr_begin(list, variant, draw, scene, w.width, w.height) };
    for mode in 0..2u32 {
        let targets = [w.rtv_at(mode, 0, 2), w.rtv_at(mode, 1, 2), w.rtv_at(mode, 2, 2)].map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr });
        // SAFETY: the list and our views.
        unsafe { crate::gpu_probe_commands::cvr_sgpr_clear(list, &targets, w.dsv_at(mode, 2), w.depth_only, if scene && mode != 0 { 255 } else { 0 }) };
        if mode == 0 && w.native_reference {
            if scene && !import_scene(list, w, draw, 0) {
                restore(list, draw);
                fail(E_FAIL);
                return;
            }
            reference(list, w, draw, 0, 0);
            vcall!(list, 25, fn(*mut c_void), (variant));
        } else if mode == 0 {
            // SAFETY: the list, its view-instancing interface and the native draw.
            unsafe { crate::gpu_probe_commands::cvr_sgpr_eyes(list, views.as_raw(), draw) };
        } else {
            shared_draw(list, views.as_raw(), w, draw, 0, instances);
        }
        if mode != 0 || !w.native_reference {
            copy_mode(list, w, mode as usize, false);
        }
    }
    vcall!(views.as_raw(), 65, fn(u32), (u32::MAX));
    restore(list, draw);
    *guard = Some(pending);
    state().store(1, Release);
}
/// BeforeSubmit: the bounded preparation gate on the producer's submission; MAIN's list waits on the producer's fence
/// when it goes to another queue.
pub fn before_submit(queue: *mut c_void, lists: &[*mut c_void]) {
    if state().load(Acquire) != 1 {
        return;
    }
    let mut guard = lock();
    // One bounded diagnostic stall answers whether MAIN's CPU preparation can progress while VRCAM's GPU work is
    // still pending; a separate CPU worker always releases it within the requested (at most 50 ms) interval.
    let gate_ms = a!(prepare_gate_ms).load(Relaxed).min(50);
    if let Some(w) = guard.as_deref_mut() {
        if w.group && !w.rejected && !w.producer_submitted && !w.main_recorded && w.preparation_gate.is_none() && gate_ms != 0 && lists.contains(&w.list) {
            let r = rt();
            let exports = GateExports {
                committed: r.map_or(core::ptr::null(), |r| r.exports.scene_committed),
                release: r.map_or(core::ptr::null(), |r| r.exports.prepare_gate_release),
                reason: r.map_or(core::ptr::null(), |r| r.exports.prepare_gate_reason),
                start: r.map_or(core::ptr::null(), |r| r.exports.prepare_gate_start),
            };
            let mut gate: *const Gate = core::ptr::null();
            // SAFETY: the queue, our exports and output.
            let hr = HRESULT(unsafe { crate::gpu_probe_gate::cvr_sgpg_begin(queue, gate_ms, &exports, &mut gate) });
            if !gate.is_null() {
                w.preparation_gate = Some(GateHandle(gate));
            }
            if hr.is_err() {
                fail(hr);
                return;
            }
        }
    }
    let Some(w) = guard.as_deref() else { return };
    if !w.native_reference || !w.main_recorded || !w.producer_submitted || !lists.contains(&w.main_list) {
        return;
    }
    if queue != raw(&w.producer_queue) {
        let q = queue;
        // SAFETY: the engine's queue.
        let Some(q) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&q) }) else { return };
        if let Some(fence) = w.producer_fence.as_ref() {
            if let Err(e) = unsafe { q.Wait(fence, 1) } {
                fail(e.code());
                return;
            }
        }
        a!(queue_waits).fetch_add(1, SeqCst);
    }
}
pub fn submitted(queue: *mut c_void, lists: &[*mut c_void]) {
    if state().load(Acquire) != 1 {
        return;
    }
    let mut guard = lock();
    let Some(w) = guard.as_deref_mut() else { return };
    if state().load(SeqCst) != 1 {
        return;
    }
    let raw_queue = queue;
    // SAFETY: the engine's queue.
    let Some(q) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&raw_queue) }) else { return };
    let signal = |fence: &Option<ID3D12Fence>| fence.as_ref().map_or(Err(E_FAIL), |f| unsafe { q.Signal(f, 1) }.map_err(|e| e.code()));
    if lists.contains(&w.list) && !w.main_recorded {
        if w.timestamps.is_some() {
            match unsafe { q.GetTimestampFrequency() } {
                Ok(f) => w.frequency[0] = f,
                Err(_) => {
                    fail(E_FAIL);
                    return;
                }
            }
        }
        if w.native_reference {
            if !w.producer_submitted {
                if let Err(hr) = signal(&w.producer_fence) {
                    fail(hr);
                    return;
                }
                w.producer_queue = Some(q.clone());
                w.producer_submitted = true;
            }
        } else {
            if let Err(hr) = signal(&w.fence) {
                fail(hr);
                return;
            }
            state().store(2, Release);
            return;
        }
    }
    if w.native_reference && w.main_recorded && lists.contains(&w.main_list) {
        if w.main_submitted {
            return;
        }
        if w.timestamps.is_some() {
            match unsafe { q.GetTimestampFrequency() } {
                Ok(f) => w.frequency[1] = f,
                Err(_) => {
                    fail(E_FAIL);
                    return;
                }
            }
        }
        if let Err(hr) = signal(&w.fence) {
            fail(hr);
            return;
        }
        w.main_submitted = true;
        if w.group {
            finish_group_if_submitted(w);
        } else {
            state().store(2, Release);
        }
    }
}
pub fn poll() {
    if state().load(Acquire) == 1 && a!(native_probe_state).load(Acquire) != 1 {
        let mut guard = lock();
        if let Some(w) = guard.as_deref() {
            let completed = |f: &Option<ID3D12Fence>| f.as_ref().is_some_and(|f| unsafe { f.GetCompletedValue() } == 1);
            if w.native_reference && w.producer_submitted && completed(&w.producer_fence) && (!w.main_recorded || (w.group && w.main_submitted && completed(&w.fence))) {
                *guard = None;
                state().store(6, Release);
            }
        }
        return;
    }
    if state().load(Acquire) != 2 {
        return;
    }
    let mut guard = lock();
    let Some(w) = guard.as_deref() else { return };
    let mut layouts: [[Layout; 4]; 2] = unsafe { core::mem::zeroed() };
    for mode in 0..2 {
        for target in 0..4 {
            let s = &w.surfaces[mode][target];
            let l = &mut layouts[mode][target];
            (l.planes, l.footprints, l.row_bytes, l.rows) = (s.planes, s.footprints, s.row_bytes, s.rows);
        }
    }
    let r = rt();
    let exports = ReadExports {
        eyes: r.map_or(core::ptr::null_mut(), |r| r.exports.eye_differences),
        targets: r.map_or(core::ptr::null_mut(), |r| r.exports.target_differences),
        samples: r.map_or(core::ptr::null_mut(), |r| r.exports.differences),
        count: r.map_or(core::ptr::null_mut(), |r| r.exports.difference_count),
        scene: r.map_or(core::ptr::null_mut(), |r| r.exports.scene_samples),
        drops: r.map_or(core::ptr::null(), |r| r.exports.timing_drops),
        ticks: r.map_or(core::ptr::null_mut(), |r| r.exports.ticks),
        timed: r.map_or(core::ptr::null_mut(), |r| r.exports.timed_count),
    };
    let mut totals = Totals::default();
    // SAFETY: our fence, readback, layouts, timings and the exports.
    let hr = HRESULT(unsafe {
        crate::gpu_probe_gate::cvr_sgpr_read(
            raw(&w.fence),
            raw(&w.readback),
            w.bytes,
            &layouts,
            w.width,
            w.depth_only,
            if w.scene_depth { w.query_offset } else { u64::MAX },
            w.timed.as_ptr(),
            w.timed.len(),
            w.timestamps.is_some(),
            w.timestamp_offset,
            &exports,
            &mut totals,
        )
    });
    if hr == S_FALSE {
        return;
    }
    if hr.is_err() {
        fail(hr);
        return;
    }
    if w.timestamps.is_some() {
        if let Some(f) = t!(frequency) {
            *f = w.frequency;
        }
    }
    let captured = t!(vrs_captured).map_or([0, 0], |c| *c);
    if w.source_vrs.is_some() && captured[0] != 0 && captured[1] != 0 {
        let mut histogram = [[0u64; 16]; 2];
        let mut difference = 0u64;
        // SAFETY: our snapshot; our outputs.
        let result = unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_read(w.vrs_snapshot, &mut difference, &mut histogram) };
        if let Some(d) = t!(vrs_difference) {
            *d = difference;
        }
        if result.is_err() {
            fail(result);
            return;
        }
        if let Some(h) = t!(vrs_histogram) {
            *h = histogram;
        }
    }
    if let (Some(d), Some(c), Some(cov)) = (t!(different_bytes), t!(compared_bytes), t!(coverage)) {
        (*d, *c, *cov) = (totals.different, totals.compared, totals.coverage);
    }
    if let Some(result) = result_export() {
        result.store(S_OK.0, SeqCst);
    }
    *guard = None;
    state().store(if totals.coverage[0] != 0 && totals.coverage[1] != 0 { 3 } else { 5 }, Release);
}

// ---- the C ABI

/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// # Safety
/// `previous` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_begin_group(side: i32, plane: u32, previous: *mut GroupContext) {
    unsafe { *previous = begin_group(side, plane) };
}
/// # Safety
/// `previous` is a context begin_group returned.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_end_group(previous: *const GroupContext) {
    end_group(unsafe { *previous });
}
/// # Safety
/// The list, variant and draw are the native probe's; `instances` holds `count` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_draw(list: *mut c_void, variant: *mut c_void, record: *const DrawRecord, instances: *const u8, count: usize) {
    let bytes = if instances.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(instances, count) } };
    draw(list, variant, unsafe { &*record }, bytes);
}
/// # Safety
/// As draw.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_boundary(list: *mut c_void, record: *const DrawRecord, require_pipeline_change: bool, candidate: *mut c_void) {
    boundary(list, unsafe { &*record }, require_pipeline_change, candidate);
}
/// # Safety
/// As draw.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_route(list: *mut c_void, record: *const DrawRecord) -> bool {
    route_native(list, unsafe { &*record })
}
/// # Safety
/// `lists` holds `count` command lists.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpw_submit(after: bool, queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if lists.is_null() {
        return;
    }
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    if after {
        submitted(queue, lists);
    } else {
        before_submit(queue, lists);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sgpw_poll() {
    poll();
}

#[cfg(test)]
mod tests {
    //! The handles and the group context without a device: the descriptor layout of both modes and the resolved
    //! targets, the group ids and their nesting, the draw-time admission refusals (no request, a wrong side), the
    //! timing scope without its heap, and the poll's idle states.
    use super::*;

    #[test]
    fn handles_groups_and_idle_paths() {
        // SAFETY: plain record.
        let draw: DrawRecord = unsafe { core::mem::zeroed() };
        let mut w = Work::new(&draw);
        (w.rtv_stride, w.dsv_stride) = (32, 8);
        // Without heaps the handles are offsets: mode 0's eyes 0..1 then the array, mode 1's eyes 6/12, resolved 15+.
        assert_eq!([w.rtv_at(0, 1, 0), w.rtv_at(0, 1, 1), w.rtv_at(0, 1, 2), w.rtv_at(1, 2, 0), w.rtv_at(1, 2, 1), w.resolved_rtv(1, 2)], [32, 128, 320, 256, 448, 640]);
        assert_eq!([w.dsv_at(0, 0), w.dsv_at(0, 2), w.dsv_at(1, 0), w.dsv_at(1, 1), w.resolved_dsv(1)], [0, 24, 16, 32, 48]);
        // No timing heap: no record, no drop.
        let t = TimingScope::new(core::ptr::null_mut(), &mut w, stage::COMMON, 0);
        assert!(!t.active && w.timed.is_empty());
        drop(t);
        // Groups: fresh ids, the previous one restored.
        let outer = begin_group(1, 4);
        let first = current_group();
        assert!(first.id != 0 && first.side == 1 && first.plane == 4 && outer == NO_GROUP);
        let inner = begin_group(0, 5);
        assert_eq!(inner, first);
        assert!(current_group().id > first.id);
        end_group(inner);
        assert_eq!(current_group(), first);
        end_group(outer);
        assert_eq!(current_group(), NO_GROUP);
        // Idle: no request, nothing in flight.
        poll();
        assert!(lock().is_none());
        assert!(!route_native(core::ptr::null_mut(), &draw));
    }
}
