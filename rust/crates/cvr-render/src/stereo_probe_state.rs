//! The native stereo probe's state (formerly src/Render/NativeStereoProbe.cpp's pipeline registry and per-list state):
//! each native pipeline's input-slot mask, the root signatures' blobs and camera bindings, the view-instanced variants
//! registered beside their native pipelines; each command list's bound state mirrored from its API calls (heaps,
//! pipeline, vertex and index buffers, root signature, tables and CBVs, predication, targets, viewport, scissor); the
//! frame boundary's capture state machine (a requested capture over at most four frames, or the live bindings window);
//! the draw capture into the exported rows -- the instance data read from the bound instance buffer, the descriptor
//! snapshot, the pass targets -- and its routing to the retained, paired, early-depth and GPU-probe consumers; the
//! current state of a list for the consumers.
//!
//! The .cpp keeps the exports' storage, the probe's shader assets and its pipeline creation preambles (the replacement
//! shaders and view-instanced creation lent per call), and the instance buffers' leases (C++ geometry types); every
//! other subsystem is lent through the runtime.

use cvr_host::fast::FastMap;
use crate::camera_upload::PipelineRecord;
use crate::geometry_commands::DrawRecord;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::*};
use std::sync::{Mutex, MutexGuard};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

pub const PIPELINE_CAPACITY: usize = 8;
pub const DRAW_CAPACITY: usize = 4096;
pub const TARGET_RECORD_BYTES: usize = 464;
/// The node of the native scene's draws.
const SCENE_NODE: u32 = 0x33DDA4;

/// preparation::DrawTag.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DrawTag {
    pub scope: u64,
    pub stage: u32,
    pub frame: u32,
    pub side: i32,
}
/// pass_targets::Record's head (its nine views follow).
#[repr(C)]
struct TargetsHead {
    sequence: u64,
    scope: u64,
    stage: u32,
    frame: u32,
    status: u32,
    colors: u32,
}
type List = *mut c_void;

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    pub probe_state: *const AtomicU32,
    pub probe_request: *const AtomicU32,
    pub live_bindings: *const AtomicU32,
    pub live_failure: *const AtomicU32,
    pub probe_seq: *const AtomicU32,
    pub draw_count: *const AtomicU32,
    pub pipeline_count: *const AtomicU32,
    pub all_draws: *const AtomicU32,
    pub gpu_scene_depth: *const AtomicU32,
    pub gpu_state: *const AtomicU32,
    pub gpu_request: *const AtomicU32,
    pub gpu_native_reference: *const AtomicU32,
    pub gpu_depth_prepass: *const AtomicU32,
    pub constant_capture: *const AtomicU32,
    pub retained_source_stream: *const AtomicU32,
    pub early_depth_state: *const AtomicU32,
    pub preparation_all_stages: *const AtomicU32,
    pub pipelines: *mut PipelineRecord,
    pub draws: *mut DrawRecord,
    pub targets: *mut u8,
    /// upload_reservation, graph jobs, paired and retained geometry, geometry snapshots and constants: polled.
    pub frame_polls: extern "C" fn(),
    /// 3 constants::End, 6 constants::Begin, 7 geometry::Begin(`arg`) (the other steps are direct::capture's).
    pub capture: extern "C" fn(which: u32, arg: bool),
    /// families::RootBound.
    pub root: extern "C" fn(list: List, root: *mut c_void),
    /// The constant capture's family and constants snapshot, the draw state and targets, geometry::Snapshot.
    pub snapshot_geometry: extern "C" fn(list: List, pipeline: *mut c_void, record: *mut DrawRecord, batch: *const u8, bytes: usize),
    /// instance_data::CopyObservedDefault: the batch (when asked for) as the .cpp's thread buffer.
    pub copy_default: extern "C" fn(record: *mut DrawRecord, batch: bool, bytes: *mut usize) -> *const u8,
    /// instance_data::Range.
    pub instance_range: extern "C" fn(record: *const DrawRecord, batch: bool, offset: *mut u64, bytes: *mut usize) -> bool,
}
// SAFETY: immutable; its pointers name process-lifetime exports and functions.
unsafe impl Sync for Runtime {}

// ---- the subsystems formerly lent through the runtime, called directly (the plugin and the native probe's test link
// them; the default and unit-test builds attach no runtime, so these never run there)
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
mod direct {
    use super::{DrawRecord, List};
    use crate::ia_binding::OwnedLease;
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicU32, Ordering::*};
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    use windows::core::{IUnknown, Interface};
    use windows::Win32::Graphics::Direct3D12::*;

    #[allow(non_upper_case_globals)]
    extern "C" {
        static CyberpunkVR_NativeStereoProbeState: AtomicU32;
        static CyberpunkVR_GeometryLeaseFailure: AtomicU32;
        fn cvr_ned_reset(list: *mut c_void);
        fn cvr_ned_drawing() -> bool;
    }
    fn internal() -> bool {
        cvr_host::probe::internal()
    }
    /// scene_state's detail::Recording: the probe capturing, outside the port's own commands.
    fn recording() -> bool {
        // SAFETY: an exported atomic.
        !internal() && unsafe { CyberpunkVR_NativeStereoProbeState.load(Relaxed) } == 1
    }
    fn lease_failure(value: u32) {
        // SAFETY: an exported atomic.
        unsafe { CyberpunkVR_GeometryLeaseFailure.store(value, SeqCst) };
    }
    fn zero_lease() -> OwnedLease {
        // SAFETY: every field is valid zeroed (no resource).
        unsafe { core::mem::zeroed() }
    }
    fn release(lease: &OwnedLease) {
        if !lease.resource.is_null() {
            // SAFETY: the reference the lease holds.
            drop(unsafe { IUnknown::from_raw(lease.resource) });
        }
    }
    fn add_ref(resource: *mut c_void) {
        // SAFETY: a live resource or null; the clone's reference kept.
        if let Some(unknown) = unsafe { IUnknown::from_raw_borrowed(&resource) } {
            core::mem::forget(unknown.clone());
        }
    }

    /// early_depth::Reset (a new recording), geometry::Reset and scene_state::Reset.
    pub fn reset(list: List, rate_known: bool, new_recording: bool) {
        if new_recording {
            // SAFETY: the live list.
            unsafe { cvr_ned_reset(list) };
        }
        crate::geometry_leases::cvr_gl_bindings(4, list);
        if recording() {
            cvr_gpu::scene_state::cvr_scene_reset(list as usize, rate_known);
        }
    }
    /// geometry::Vertices: the bound vertex buffers' leases told to the geometry leases.
    pub fn vertices(list: List, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW) {
        if !crate::geometry_leases::cvr_gl_flag(0) || internal() || list.is_null() {
            return;
        }
        if start > 32 || count > 32 - start {
            lease_failure(2);
            return;
        }
        if start >= 16 {
            return;
        }
        let mut leases: [OwnedLease; 16] = core::array::from_fn(|_| zero_lease());
        // SAFETY: the API call's views; sixteen writable leases (their references ours).
        unsafe { crate::ia_binding::cvr_iab_bound_vertices(start, count, views, leases.as_mut_ptr()) };
        // SAFETY: the live list, the views and the sixteen leases (lent).
        let failure = unsafe { crate::geometry_leases::cvr_gl_vertices(list, start, count, views, leases.as_ptr().cast()) };
        if failure != 0 {
            lease_failure(failure);
        }
        leases.iter().for_each(release);
    }
    /// geometry::Index: the bound index buffer's lease told to the geometry leases.
    pub fn index(list: List, view: *const D3D12_INDEX_BUFFER_VIEW) {
        if !crate::geometry_leases::cvr_gl_flag(0) || internal() || list.is_null() {
            return;
        }
        let mut lease = zero_lease();
        if !view.is_null() {
            // SAFETY: the API call's view; a writable lease (its reference ours).
            unsafe { crate::ia_binding::cvr_iab_bound_index(view, &mut lease) };
        }
        // SAFETY: the live list, the view and the lease (lent).
        if !unsafe { crate::geometry_leases::cvr_gl_index(list, view, (&raw const lease).cast()) } {
            lease_failure(1);
        }
        release(&lease);
    }
    pub fn root(list: List, root: *mut c_void) {
        if recording() {
            cvr_gpu::scene_state::cvr_scene_root(list as usize, root as usize);
        }
    }
    pub fn table(list: List, index: u32, table: u64) {
        if recording() {
            cvr_gpu::scene_state::cvr_scene_table(list as usize, index, table);
        }
    }
    pub fn paired_active() -> bool {
        crate::paired_geometry_native::active()
    }
    pub fn paired_draw(list: List, record: &DrawRecord) -> bool {
        crate::paired_geometry_native::draw(list, record)
    }
    /// scene_state::DescribeTargets(record, views).
    pub fn describe_targets(record: &DrawRecord, views: *mut u8) -> bool {
        let targets = cvr_gpu::scene_state::DrawTargets {
            rt_count: record.rt_count,
            rt_contiguous: record.rt_contiguous,
            targets: record.targets,
            depth: record.depth,
            viewport: record.viewport,
        };
        // SAFETY: a local record; the caller's nine views.
        unsafe { cvr_gpu::scene_state::cvr_scene_describe_targets(&targets, views.cast()) }
    }
    pub fn pipeline_created(original: *mut c_void, supported: bool) {
        cvr_gpu::scene_state::cvr_scene_pipeline_created(original as usize, supported);
    }
    /// The capture's steps: 0 geometry::BeginBindings, 1 scene_state::ResetCapture, 2 geometry::EndBindings,
    /// 4 geometry::End, 5 geometry::ClearBindings here; 3 constants::End, 6 constants::Begin and 7 geometry::Begin(`arg`)
    /// the .cpp's.
    pub fn capture(which: u32, arg: bool, host: extern "C" fn(u32, bool)) {
        match which {
            0 => crate::geometry_leases::cvr_gl_bindings(0, core::ptr::null_mut()),
            1 => cvr_gpu::scene_state::cvr_scene_reset_capture(),
            2 => crate::geometry_leases::cvr_gl_bindings(1, core::ptr::null_mut()),
            4 => crate::geometry_leases::cvr_gl_bindings(2, core::ptr::null_mut()),
            5 => crate::geometry_leases::cvr_gl_bindings(3, core::ptr::null_mut()),
            _ => host(which, arg),
        }
    }
    /// shader_snapshot::RootCreated and scene_state::RootCreated.
    pub fn roots_created(root: *mut c_void, data: *const u8, bytes: usize) {
        // SAFETY: the creation's live root and blob.
        unsafe {
            crate::shader_snapshot_native::cvr_shs_root_created(root, data.cast(), bytes);
            cvr_gpu::scene_state::cvr_scene_root_created(root as usize, data.cast(), bytes);
        }
    }

    // The instance leases (formerly the .cpp's instanceSources map): each list's instance-data buffer as bound.
    struct Leases(HashMap<usize, OwnedLease>);
    // SAFETY: the leases' references are only touched under the lock.
    unsafe impl Send for Leases {}
    impl Drop for Leases {
        fn drop(&mut self) {
            self.0.values().for_each(release);
        }
    }
    static INSTANCES: LazyLock<Mutex<Leases>> = LazyLock::new(|| Mutex::new(Leases(HashMap::new())));
    fn instances() -> std::sync::MutexGuard<'static, Leases> {
        INSTANCES.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// 0 clear, 1 erase `list`'s, 2 set `list`'s (the resource and its shape; while early depth draws, the native bound
    /// vertex lease of the same resource), 3 `list`'s resource (borrowed) or null.
    #[allow(clippy::too_many_arguments)]
    pub fn instance(which: u32, list: List, resource: *mut c_void, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW, gpu: u64, bytes: u64, heap: u32, flags: u32) -> *mut c_void {
        let mut all = instances();
        match which {
            0 => {
                all.0.values().for_each(release);
                all.0.clear();
            }
            1 => {
                if let Some(old) = all.0.remove(&(list as usize)) {
                    release(&old);
                }
            }
            2 => {
                add_ref(resource);
                let mut lease = OwnedLease { resource, gpu, bytes, heap_type: heap, flags, ..zero_lease() };
                // SAFETY: a plain query.
                if unsafe { cvr_ned_drawing() } {
                    let mut native: [OwnedLease; 16] = core::array::from_fn(|_| zero_lease());
                    // SAFETY: the API call's views; sixteen writable leases (their references ours).
                    unsafe { crate::ia_binding::cvr_iab_bound_vertices(start, count, views, native.as_mut_ptr()) };
                    if start <= 7 && ((7 - start) as usize) < native.len() && native[(7 - start) as usize].resource == resource {
                        release(&lease);
                        let chosen = &native[(7 - start) as usize];
                        add_ref(chosen.resource);
                        // SAFETY: a plain record copied (its reference taken above).
                        lease = unsafe { core::ptr::read(chosen) };
                    }
                    native.iter().for_each(release);
                }
                if let Some(old) = all.0.insert(list as usize, lease) {
                    release(&old);
                }
            }
            _ => return all.0.get(&(list as usize)).map_or(core::ptr::null_mut(), |l| l.resource),
        }
        core::ptr::null_mut()
    }
    /// native_probe::InstanceLease: `list`'s instance lease, copied into the caller's empty BufferLease (a reference
    /// taken for it).
    ///
    /// # Safety
    /// `out` is a default (empty) geometry::BufferLease.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_nsp2_instance_lease(list: List, out: *mut OwnedLease) {
        let all = instances();
        if let (Some(lease), false) = (all.0.get(&(list as usize)), out.is_null()) {
            add_ref(lease.resource);
            // SAFETY: as documented (a plain record written over the empty one).
            unsafe { core::ptr::copy_nonoverlapping(lease, out, 1) };
        }
    }
}
#[cfg(not(all(any(feature = "rust-build", feature = "probe-exports"), not(test))))]
mod direct {
    //! The default and unit-test builds attach no runtime: never called.
    use super::{DrawRecord, List};
    use core::ffi::c_void;
    use windows::Win32::Graphics::Direct3D12::{D3D12_INDEX_BUFFER_VIEW, D3D12_VERTEX_BUFFER_VIEW};
    pub fn reset(_: List, _: bool, _: bool) {}
    pub fn vertices(_: List, _: u32, _: u32, _: *const D3D12_VERTEX_BUFFER_VIEW) {}
    pub fn index(_: List, _: *const D3D12_INDEX_BUFFER_VIEW) {}
    pub fn root(_: List, _: *mut c_void) {}
    pub fn table(_: List, _: u32, _: u64) {}
    pub fn paired_active() -> bool {
        false
    }
    pub fn paired_draw(_: List, _: &DrawRecord) -> bool {
        false
    }
    pub fn describe_targets(_: &DrawRecord, _: *mut u8) -> bool {
        false
    }
    pub fn pipeline_created(_: *mut c_void, _: bool) {}
    pub fn capture(which: u32, arg: bool, host: extern "C" fn(u32, bool)) {
        host(which, arg);
    }
    pub fn roots_created(_: *mut c_void, _: *const u8, _: usize) {}
    #[allow(clippy::too_many_arguments)]
    pub fn instance(_: u32, _: List, _: *mut c_void, _: u32, _: u32, _: *const D3D12_VERTEX_BUFFER_VIEW, _: u64, _: u64, _: u32, _: u32) -> *mut c_void {
        core::ptr::null_mut()
    }
}

// ---- the subsystems formerly lent through the runtime's one-line C++ forwards, called directly
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
extern "C" {
    fn cvr_ned_drawing() -> bool;
    fn cvr_ned_draw(list: *mut c_void, draw_record: *const DrawRecord, instances: *const u8, bytes: usize) -> bool;
    fn cvr_nrg2_active() -> bool;
    fn cvr_nrg2_draw(list: *mut c_void, record: *const DrawRecord) -> bool;
}
/// The default and unit-test builds link neither early depth nor the retained geometry: neither draws.
#[cfg(not(all(any(feature = "rust-build", feature = "probe-exports"), not(test))))]
mod absent {
    use super::DrawRecord;
    use core::ffi::c_void;
    pub unsafe fn cvr_ned_drawing() -> bool {
        false
    }
    pub unsafe fn cvr_ned_draw(_: *mut c_void, _: *const DrawRecord, _: *const u8, _: usize) -> bool {
        false
    }
    pub unsafe fn cvr_nrg2_active() -> bool {
        false
    }
    pub unsafe fn cvr_nrg2_draw(_: *mut c_void, _: *const DrawRecord) -> bool {
        false
    }
}
#[cfg(not(all(any(feature = "rust-build", feature = "probe-exports"), not(test))))]
use absent::*;
/// early_depth::Drawing.
fn early_drawing() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_ned_drawing() }
}
/// early_depth::Draw.
fn early_draw(list: List, record: *const DrawRecord, batch: *const u8, bytes: usize) -> bool {
    // SAFETY: the caller's live list, record and batch.
    unsafe { cvr_ned_draw(list, record, batch, bytes) }
}
/// retained_geometry::Active.
fn retained_active() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_nrg2_active() }
}
/// retained_geometry::Draw.
fn retained_draw(list: List, record: *const DrawRecord) -> bool {
    // SAFETY: the caller's live list and record.
    unsafe { cvr_nrg2_draw(list, record) }
}
/// preparation::DrawRecorded.
fn draw_recorded(list: List) {
    crate::preparation_trace::cvr_prep_draw_recorded(list as u64);
}
/// preparation::CurrentDrawTag (the trace's DrawTag has this layout).
fn draw_tag(out: *mut DrawTag) {
    // SAFETY: the caller's output, of the trace's layout.
    unsafe { crate::preparation_trace::cvr_prep_draw_tag(out.cast()) };
}
/// gpu_probe::Boundary(list, record, true, variant).
fn gpu_boundary(list: List, record: *const DrawRecord, variant: *mut c_void) {
    // SAFETY: the caller's live list and record.
    unsafe { crate::gpu_probe_runtime::cvr_sgpw_boundary(list, record, true, variant) };
}
/// gpu_probe::Draw, then gpu_probe::RouteNative's answer.
fn gpu_draw(list: List, variant: *mut c_void, record: *const DrawRecord, batch: *const u8, bytes: usize) -> bool {
    // SAFETY: the caller's live list, record and batch.
    unsafe {
        crate::gpu_probe_runtime::cvr_sgpw_draw(list, variant, record, batch, bytes);
        crate::gpu_probe_runtime::cvr_sgpw_route(list, record)
    }
}
/// The plugin's runtime (formerly NativeStereoProbe.cpp's lambdas, all over Rust): the exports, the .cpp's record
/// arrays, the frame polls, the capture steps, the root binding, the geometry snapshot and the instance reads. The Rust
/// test variants attach the .cpp's table instead.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::{DrawRecord, PipelineRecord, Runtime, DRAW_CAPACITY, TARGET_RECORD_BYTES};
    use crate::declared_moved_exports::{
        CyberpunkVR_NativeStereoAllDraws, CyberpunkVR_NativeStereoDrawCount, CyberpunkVR_NativeStereoLiveBindings,
        CyberpunkVR_NativeStereoLiveFailure, CyberpunkVR_NativeStereoPipelineCount, CyberpunkVR_NativeStereoProbeRequest,
        CyberpunkVR_NativeStereoProbeSeq, CyberpunkVR_NativeStereoProbeState,
    };
    use crate::native_state_exports::{
        CyberpunkVR_ConstantCaptureEnable, CyberpunkVR_PreparationAllStages, CyberpunkVR_RegisteredIntentCacheStats,
        CyberpunkVR_RegisteredStructuredIntentStats, CyberpunkVR_StereoGpuProbeDepthPrepass, CyberpunkVR_StereoGpuProbeNativeReference,
        CyberpunkVR_StereoGpuProbeRequest, CyberpunkVR_StereoGpuProbeSceneDepth, CyberpunkVR_StereoGpuProbeState,
    };
    use core::cell::RefCell;
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicBool, Ordering::*};
    #[allow(non_upper_case_globals)]
    extern "C" {
        /// src/Render/NativeStereoProbe.cpp's records.
        static mut CyberpunkVR_NativeStereoPipelines: [PipelineRecord; 8];
        static mut CyberpunkVR_NativeStereoDraws: [DrawRecord; DRAW_CAPACITY];
        static mut CyberpunkVR_NativeStereoTargets: [u8; DRAW_CAPACITY * TARGET_RECORD_BYTES];
        /// upload_reservation_runtime.rs' (its private `abi` module).
        fn cvr_ur_rt_poll_installation(base: usize);
    }
    /// constants::PollRegisteredIntent (formerly RegisteredConstantUpload.cpp's): an intent switched off has its
    /// observations cleaned (the source cache, and the registered entries while the constant capture is off); the cache
    /// statistics published while it is or was on.
    fn poll_registered_intent() {
        static ENABLED: AtomicBool = AtomicBool::new(false);
        let requested = crate::registered_copies::entries::cvr_rc2_intent_configured();
        let cleanup = !requested && crate::registered_copies::entries::cvr_rc2_intent_observed_take();
        if cleanup {
            // SAFETY: the source cache's own call.
            unsafe { crate::source_intent::cvr_sri_clear() };
            if CyberpunkVR_ConstantCaptureEnable.load(Relaxed) == 0 {
                crate::registered_copies::entries::cvr_rc2_clear_registered(false);
            }
        }
        if ENABLED.load(Relaxed) || requested || cleanup {
            let mut s = [0u64; 12];
            // SAFETY: room for the twelve statistics.
            unsafe { crate::source_intent::cvr_sri_statistics(&mut s) };
            let values = [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], u64::from(requested)];
            // SAFETY: the exported statistics, written by this poll as the .cpp's was.
            unsafe {
                (*(&raw mut CyberpunkVR_RegisteredIntentCacheStats)).copy_from_slice(&values);
                (*(&raw mut CyberpunkVR_RegisteredStructuredIntentStats))[0] = s[10];
                (*(&raw mut CyberpunkVR_RegisteredStructuredIntentStats))[1] = s[11];
            }
        }
        ENABLED.store(requested, Relaxed);
    }
    extern "C" fn frame_polls() {
        // SAFETY: the reservation runtime's poll (the process image's base found by it).
        unsafe { cvr_ur_rt_poll_installation(0) };
        crate::preparation_trace::cvr_prep_poll_graph_jobs();
        crate::paired_geometry_native::cvr_pg2_frame_boundary();
        crate::retained_geometry_native::cvr_nrg2_frame_boundary();
        crate::geometry_freeze::cvr_gfz_poll();
        poll_registered_intent();
        crate::constant_bank::cvr_ncb_poll();
    }
    extern "C" fn capture(which: u32, argument: bool) {
        match which {
            3 => crate::constant_capture_state::cvr_ncc2_end(),
            6 => crate::constant_capture_state::cvr_ncc2_begin(),
            _ => crate::geometry_leases::natives::cvr_gl_begin_all(argument), // the freeze begins with them
        }
    }
    extern "C" fn root(list: *mut c_void, root: *mut c_void) {
        // SAFETY: the probe's live list and root.
        unsafe { crate::families_native::cvr_nsf2_root_bound(list, root) };
    }
    extern "C" fn snapshot_geometry(list: *mut c_void, pso: *mut c_void, record: *mut DrawRecord, batch: *const u8, bytes: usize) {
        // SAFETY: the probe's live list, pipeline, record and batch.
        unsafe { crate::geometry_leases::natives::cvr_gl_probe_snapshot(list, pso, record, batch, bytes) };
    }
    /// instance_data::Range.
    fn range(d: &DrawRecord, allow_observed_default: bool) -> Option<(u64, usize)> {
        let v = &d.vertices[7];
        let input = crate::instance_data::RangeInput {
            buffer_location: v.BufferLocation,
            size_in_bytes: v.SizeInBytes,
            stride: v.StrideInBytes,
            instance_resource: d.instance_resource,
            instance_gpu_base: d.instance_gpu_base,
            instance_bytes_total: d.instance_bytes_total,
            vertex_mask: d.vertex_mask,
            instances: d.instances,
            first_instance: d.first_instance,
            heap_type: d.instance_heap_type,
        };
        crate::instance_data::range(&input, allow_observed_default)
    }
    extern "C" fn instance_range(record: *const DrawRecord, batch: bool, offset: *mut u64, bytes: *mut usize) -> bool {
        // SAFETY: the probe's record and outputs.
        let Some((at, count)) = range(unsafe { &*record }, batch) else { return false };
        // SAFETY: as above.
        unsafe {
            *offset = at;
            *bytes = count;
        }
        true
    }
    thread_local! {
        static BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }
    /// D3D12_HEAP_TYPE_DEFAULT.
    const HEAP_DEFAULT: u32 = 1;
    /// instance_data::CopyObservedDefault with the upload shadow's reader: a DEFAULT-heap draw's instances read from
    /// the bytes observed in CPU uploads (the batch into this thread's buffer when asked for, a single instance into
    /// the row).
    fn copy_observed_default(row: &mut DrawRecord, batch: Option<&mut Vec<u8>>) -> bool {
        row.instance_data_bytes = 0;
        row.instance_data = [0; 64];
        let mut batch = batch;
        if let Some(b) = batch.as_deref_mut() {
            b.clear();
        }
        if row.instance_heap_type != HEAP_DEFAULT || (batch.is_none() && row.instances != 1) {
            return false;
        }
        let Some((offset, bytes)) = range(row, true) else { return false };
        if batch.is_none() && bytes > row.instance_data.len() {
            return false;
        }
        let output = match batch.as_deref_mut() {
            Some(b) => {
                b.resize(bytes, 0);
                b.as_mut_ptr()
            }
            None => row.instance_data.as_mut_ptr(),
        };
        // SAFETY: room for `bytes` at `output`.
        if !unsafe { crate::upload_shadow::cvr_upload_shadow_read_watched(row.instance_gpu_base, row.instance_resource, offset, output, bytes) } {
            row.instance_data = [0; 64];
            if let Some(b) = batch.as_deref_mut() {
                b.clear();
            }
            return false;
        }
        if row.instances == 1 {
            if let Some(b) = batch.as_deref() {
                let n = bytes.min(row.instance_data.len());
                row.instance_data[..n].copy_from_slice(&b[..n]);
            }
            row.instance_data_bytes = bytes as u32;
        }
        true
    }
    extern "C" fn copy_default(record: *mut DrawRecord, batch: bool, bytes: *mut usize) -> *const u8 {
        BUFFER.with_borrow_mut(|buffer| {
            buffer.clear();
            // SAFETY: the probe's record.
            let row = unsafe { &mut *record };
            copy_observed_default(row, if batch { Some(&mut *buffer) } else { None });
            // SAFETY: the caller's output.
            unsafe { *bytes = buffer.len() };
            buffer.as_ptr()
        })
    }
    pub static RUNTIME: Runtime = Runtime {
        probe_state: &raw const CyberpunkVR_NativeStereoProbeState,
        probe_request: &raw const CyberpunkVR_NativeStereoProbeRequest,
        live_bindings: &raw const CyberpunkVR_NativeStereoLiveBindings,
        live_failure: &raw const CyberpunkVR_NativeStereoLiveFailure,
        probe_seq: &raw const CyberpunkVR_NativeStereoProbeSeq,
        draw_count: &raw const CyberpunkVR_NativeStereoDrawCount,
        pipeline_count: &raw const CyberpunkVR_NativeStereoPipelineCount,
        all_draws: &raw const CyberpunkVR_NativeStereoAllDraws,
        gpu_scene_depth: &raw const CyberpunkVR_StereoGpuProbeSceneDepth,
        gpu_state: &raw const CyberpunkVR_StereoGpuProbeState,
        gpu_request: &raw const CyberpunkVR_StereoGpuProbeRequest,
        gpu_native_reference: &raw const CyberpunkVR_StereoGpuProbeNativeReference,
        gpu_depth_prepass: &raw const CyberpunkVR_StereoGpuProbeDepthPrepass,
        constant_capture: &raw const CyberpunkVR_ConstantCaptureEnable,
        retained_source_stream: &raw const crate::retained_exports::CyberpunkVR_RetainedSourceStreamEnable,
        early_depth_state: &raw const crate::early_depth_exports::CyberpunkVR_EarlyDepthState,
        preparation_all_stages: &raw const CyberpunkVR_PreparationAllStages,
        pipelines: (&raw mut CyberpunkVR_NativeStereoPipelines).cast(),
        draws: (&raw mut CyberpunkVR_NativeStereoDraws).cast(),
        targets: (&raw mut CyberpunkVR_NativeStereoTargets).cast(),
        frame_polls,
        capture,
        root,
        snapshot_geometry,
        copy_default,
        instance_range,
    };
}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::RUNTIME));
    attached
}
static ZERO: AtomicU32 = AtomicU32::new(0);
macro_rules! a {
    ($field:ident) => {{
        #[allow(unused_unsafe)]
        let field = rt().and_then(|r| unsafe { r.$field.as_ref() }).unwrap_or(&ZERO);
        field
    }};
}

// ---- the pipeline registry
struct Pipelines {
    originals: [usize; PIPELINE_CAPACITY],
    variants: [Option<ID3D12PipelineState>; PIPELINE_CAPACITY],
    root_blobs: FastMap<usize, Vec<u8>>,
    input_masks: FastMap<usize, u32>,
}
// SAFETY: the variants are free-threaded D3D12 objects; the rest are numbers.
unsafe impl Send for Pipelines {}
static PIPELINES: Mutex<Option<Pipelines>> = Mutex::new(None);
fn pipelines() -> MutexGuard<'static, Option<Pipelines>> {
    let mut guard = PIPELINES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.get_or_insert_with(|| Pipelines { originals: [0; PIPELINE_CAPACITY], variants: Default::default(), root_blobs: cvr_host::fast::fast_map(), input_masks: cvr_host::fast::fast_map() });
    guard
}
fn input_mask_of(pso: usize) -> Option<u32> {
    pipelines().as_ref().and_then(|p| p.input_masks.get(&pso).copied())
}
/// ObserveInput: the native pipeline's input-slot mask (forgotten when not representable).
pub fn observe_input(original: usize, input: &D3D12_INPUT_LAYOUT_DESC) {
    if original == 0 {
        return;
    }
    let mut mask = 0u32;
    // SAFETY: the creation's input layout.
    let valid = unsafe { crate::stereo_probe_pipelines::cvr_nsp_input_mask(input, &mut mask) };
    let mut guard = pipelines();
    let p = guard.as_mut().expect("initialized");
    if !valid {
        p.input_masks.remove(&original);
        return;
    }
    if p.input_masks.len() < 8192 || p.input_masks.contains_key(&original) {
        p.input_masks.insert(original, mask);
    }
}
pub fn forget_input(original: usize) {
    if let Some(p) = pipelines().as_mut() {
        p.input_masks.remove(&original);
    }
}
/// RootCreated: the snapshots' and scene state's roots, the descriptor audit's, and the blob kept for camera bindings.
pub fn root_created(root: *mut c_void, data: *const u8, bytes: usize) {
    if rt().is_some() {
        direct::roots_created(root, data, bytes);
    }
    crate::descriptor_audit::root_created(root, data, bytes);
    let mut guard = pipelines();
    let p = guard.as_mut().expect("initialized");
    if p.root_blobs.len() < 128 {
        // SAFETY: the creation's blob.
        p.root_blobs.insert(root as usize, unsafe { core::slice::from_raw_parts(data, bytes) }.to_vec());
    }
}
/// CameraBinding: the camera table's root parameter and offset from the root's blob.
fn camera_binding(record: &mut PipelineRecord, blob: &[u8]) {
    let mut deserializer: Option<ID3D12VersionedRootSignatureDeserializer> = None;
    // SAFETY: the kept blob.
    if unsafe { D3D12CreateVersionedRootSignatureDeserializer(blob.as_ptr().cast(), blob.len(), &ID3D12VersionedRootSignatureDeserializer::IID, core::ptr::from_mut(&mut deserializer).cast()) }.is_err() {
        return;
    }
    let Some(deserializer) = deserializer else { return };
    let Ok(desc) = (unsafe { deserializer.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }) else { return };
    let (mut root, mut offset) = (0u32, 0u32);
    // SAFETY: the deserializer's 1.1 description, alive with it.
    if unsafe { crate::stereo_probe_pipelines::cvr_nsp_camera_binding(&(*desc).Anonymous.Desc_1_1, &mut root, &mut offset) } {
        (record.camera_root, record.camera_table_offset) = (root, offset);
    }
}
/// What AddPipeline records of a native pipeline.
#[repr(C)]
pub struct PipelineInfo {
    pub root: *mut c_void,
    pub targets: u32,
    pub depth_format: u32,
    pub samples: u32,
    pub formats: [u32; 8],
    /// The scene state's group resolve support (blend, depth, bounds) as the .cpp decided.
    pub group_resolve: bool,
}
/// The .cpp's view-instanced creation for one pipeline: its HRESULT, the variant (one reference) into `out`.
pub type Create = unsafe extern "C-unwind" fn(context: *const c_void, device: *mut c_void, out: *mut *mut c_void) -> i32;
/// AddPipeline: a native pipeline registered with its view-instanced variant.
pub fn add_pipeline(device: *mut c_void, original: usize, info: &PipelineInfo, create: Create, context: *const c_void) {
    let Some(r) = rt() else { return };
    let mut guard = pipelines();
    let p = guard.as_mut().expect("initialized");
    let count = a!(pipeline_count).load(Relaxed) as usize;
    if count >= PIPELINE_CAPACITY || p.originals[..count].contains(&original) {
        return;
    }
    let mut record = PipelineRecord { original: original as u64, instanced: 0, root: info.root as u64, result: 0, targets: info.targets, depth_format: info.depth_format, sample_count: info.samples, camera_root: u32::MAX, camera_table_offset: u32::MAX, formats: [0; 8] };
    for i in 0..(info.targets as usize).min(8) {
        record.formats[i] = info.formats[i];
    }
    if let Some(blob) = p.root_blobs.get(&(info.root as usize)) {
        camera_binding(&mut record, blob);
    }
    // SAFETY: the caller's live device.
    let d2 = unsafe { ID3D12Device::from_raw_borrowed(&device) }.and_then(|d| d.cast::<ID3D12Device2>().ok());
    let mut variant = core::ptr::null_mut();
    record.result = match &d2 {
        Some(d2) => {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: the .cpp's creation over the device.
            unsafe { create(context, d2.as_raw(), &mut variant) }
        }
        None => windows::Win32::Foundation::E_NOINTERFACE.0,
    };
    // SAFETY: one reference handed over (or null).
    p.variants[count] = (!variant.is_null()).then(|| unsafe { ID3D12PipelineState::from_raw(variant) });
    record.instanced = variant as u64;
    p.originals[count] = original;
    direct::pipeline_created(original as *mut c_void, info.group_resolve);
    // SAFETY: the exported rows, `count` below capacity.
    unsafe { *r.pipelines.add(count) = record };
    a!(pipeline_count).store(count as u32 + 1, Release);
}

// ---- the per-list state
struct States {
    states: FastMap<usize, DrawRecord>,
    bound_heaps: FastMap<usize, [usize; 2]>,
    frame: u32,
    limit: u32,
    live_sequence: u64,
}
static STATES: Mutex<Option<States>> = Mutex::new(None);
static HAVE_INSTANCE_SOURCES: AtomicBool = AtomicBool::new(false);
fn states() -> MutexGuard<'static, Option<States>> {
    let mut guard = STATES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.get_or_insert_with(|| States { states: cvr_host::fast::fast_map(), bound_heaps: cvr_host::fast::fast_map(), frame: 0, limit: 0, live_sequence: 0 });
    guard
}
fn zeroed_record() -> DrawRecord {
    // SAFETY: an all-zero record is its default.
    unsafe { core::mem::zeroed() }
}
fn probing() -> bool {
    a!(probe_state).load(Relaxed) == 1
}
/// Edit: `update` on the list's state while the probe captures (at most 512 lists).
fn edit(list: List, update: impl FnOnce(&mut States, &mut DrawRecord)) {
    if cvr_host::probe::internal() || list.is_null() || !probing() {
        return;
    }
    let mut guard = states();
    if !probing() {
        return;
    }
    let s = guard.as_mut().expect("initialized");
    let key = list as usize;
    let mut record = match s.states.remove(&key) {
        Some(found) => found,
        None if s.states.len() < 512 => zeroed_record(),
        None => return,
    };
    update(s, &mut record);
    s.states.insert(key, record);
}
fn instance(which: u32, list: List) -> *mut c_void {
    rt().map_or(core::ptr::null_mut(), |_| direct::instance(which, list, core::ptr::null_mut(), 0, 0, core::ptr::null(), 0, 0, 0, 0))
}
fn clear_lists(s: &mut States) {
    s.states.clear();
    s.bound_heaps.clear();
    instance(0, core::ptr::null_mut());
    HAVE_INSTANCE_SOURCES.store(false, Release);
}

/// FrameBoundary: the subsystems polled; the live bindings window opened or closed, or a requested capture begun,
/// counted down and ended.
pub fn frame_boundary(live: bool) {
    let Some(r) = rt() else { return };
    (r.frame_polls)();
    let host_capture = r.capture;
    let capture = |which: u32, arg: bool| direct::capture(which, arg, host_capture);
    if live {
        let mut guard = states();
        if a!(probe_request).load(SeqCst) != 0 || (a!(live_bindings).load(SeqCst) == 0 && a!(probe_state).load(SeqCst) == 1) || !crate::descriptor_audit::begin_bindings() {
            a!(live_failure).store(1, SeqCst);
            return;
        }
        let s = guard.as_mut().expect("initialized");
        clear_lists(s);
        s.live_sequence = 0;
        s.frame += 1;
        capture(0, false);
        capture(1, false);
        a!(live_bindings).store(1, SeqCst);
        a!(probe_state).store(1, SeqCst);
        return;
    }
    if a!(live_bindings).swap(0, SeqCst) != 0 {
        let mut guard = states();
        crate::descriptor_audit::end_bindings();
        clear_lists(guard.as_mut().expect("initialized"));
        capture(2, false);
        capture(1, false);
        a!(probe_state).store(0, SeqCst);
    }
    if probing() || a!(probe_request).load(Relaxed) != 0 {
        capture(1, false);
    }
    if !probing() && a!(probe_request).load(Relaxed) == 0 {
        if HAVE_INSTANCE_SOURCES.swap(false, AcqRel) {
            let mut guard = states();
            let s = guard.as_mut().expect("initialized");
            instance(0, core::ptr::null_mut());
            s.states.clear();
            s.bound_heaps.clear();
        }
        return;
    }
    let mut guard = states();
    let s = guard.as_mut().expect("initialized");
    if probing() {
        s.frame += 1;
        if s.frame > s.limit {
            crate::descriptor_audit::end_capture();
            capture(3, false);
            capture(4, false);
            a!(probe_state).store(2, Release);
        } else {
            capture(5, false);
        }
        clear_lists(s);
        return;
    }
    let request = a!(probe_request).swap(0, AcqRel);
    if request == 0 {
        return;
    }
    a!(probe_seq).fetch_add(1, AcqRel);
    clear_lists(s);
    s.frame = 1;
    s.limit = request.min(4);
    a!(draw_count).store(0, Relaxed);
    a!(probe_seq).fetch_add(1, Release);
    crate::descriptor_audit::begin_capture();
    capture(6, false);
    if a!(constant_capture).load(SeqCst) != 0 {
        capture(7, true);
    } else if a!(retained_source_stream).load(SeqCst) != 0 {
        capture(0, false);
    } else {
        capture(7, false);
    }
    a!(probe_state).store(1, Release);
}
pub fn reset(list: List, pso: *mut c_void, rate_known: bool, new_recording: bool) {
    if new_recording {
        crate::descriptor_audit::reset(list as u64);
    } else {
        crate::descriptor_audit::clear_bindings(list as u64);
    }
    if rt().is_some() {
        direct::reset(list, rate_known, new_recording);
    }
    edit(list, |s, state| {
        instance(1, list);
        s.bound_heaps.remove(&(list as usize));
        *state = zeroed_record();
        state.flags = 1;
        state.pipeline = pso as u64;
    });
}
pub fn heaps(list: List, count: u32, heaps: *const *mut c_void) {
    edit(list, |s, state| {
        let mut incoming = [0usize; 2];
        if count <= 2 && (count == 0 || !heaps.is_null()) {
            for i in 0..count as usize {
                // SAFETY: the API call's heaps.
                incoming[i] = unsafe { *heaps.add(i) } as usize;
            }
        }
        let old = s.bound_heaps.entry(list as usize).or_insert([0; 2]);
        if *old != incoming {
            state.table_mask = 0;
            state.tables = [0; 32];
            *old = incoming;
            crate::descriptor_audit::clear_bindings(list as u64);
        }
    });
}
pub fn pipeline(list: List, pso: *mut c_void) {
    edit(list, |_, state| state.pipeline = pso as u64);
}
pub fn vertex_buffers(list: List, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW) {
    if rt().is_none() {
        return;
    }
    direct::vertices(list, start, count, views);
    edit(list, |_, s| {
        for i in 0..count {
            if start + i >= 16 {
                break;
            }
            // SAFETY: the API call's views.
            s.vertices[(start + i) as usize] = if views.is_null() { unsafe { core::mem::zeroed() } } else { unsafe { *views.add(i as usize) } };
            s.vertex_mask |= 1 << (start + i);
        }
        if start > 7 || count <= 7 - start {
            return;
        }
        instance(1, list);
        (s.instance_resource, s.instance_gpu_base, s.instance_bytes_total, s.instance_heap_type, s.instance_data_bytes) = (0, 0, 0, 0, 0);
        s.instance_data = [0; 64];
        if a!(live_bindings).load(Relaxed) != 0 && !early_drawing() {
            return;
        }
        let address = crate::stereo_probe_pipelines::cvr_nsp_instance_resource();
        if address == 0 || views.is_null() {
            return;
        }
        let raw = address as *mut c_void;
        // SAFETY: the engine's live instance buffer (held for the call, as the .cpp's ComPtr did).
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }).cloned() else { return };
        let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
        let wide = a!(all_draws).load(Relaxed) != 0 || a!(live_bindings).load(Relaxed) != 0 || a!(gpu_depth_prepass).load(Relaxed) != 0;
        if unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() || (heap.Type != D3D12_HEAP_TYPE_UPLOAD && !(wide && heap.Type == D3D12_HEAP_TYPE_DEFAULT)) {
            return;
        }
        let desc = unsafe { resource.GetDesc() };
        let gpu = unsafe { resource.GetGPUVirtualAddress() };
        let view = s.vertices[7];
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || view.BufferLocation < gpu || view.BufferLocation - gpu > desc.Width || u64::from(view.SizeInBytes) > desc.Width - (view.BufferLocation - gpu) {
            return;
        }
        (s.instance_resource, s.instance_gpu_base, s.instance_bytes_total, s.instance_heap_type) = (address, gpu, desc.Width, heap.Type.0 as u32);
        direct::instance(2, list, raw, start, count, views, gpu, desc.Width, heap.Type.0 as u32, desc.Flags.0 as u32);
        HAVE_INSTANCE_SOURCES.store(true, Release);
    });
}
pub fn index_buffer(list: List, view: *const D3D12_INDEX_BUFFER_VIEW) {
    if rt().is_some() {
        direct::index(list, view);
    }
    edit(list, |_, s| {
        // SAFETY: the API call's view (or null).
        s.index = unsafe { view.as_ref() }.copied().unwrap_or_default();
        s.flags |= 2;
    });
}
pub fn root_signature(list: List, root: *mut c_void) {
    if let Some(r) = rt() {
        (r.root)(list, root);
        direct::root(list, root);
    }
    crate::descriptor_audit::root(list as u64, root as u64);
    edit(list, |_, s| {
        if s.root != root as u64 {
            s.root = root as u64;
            s.tables = [0; 32];
            s.cbvs = [0; 32];
            s.table_mask = 0;
            s.cbv_mask = 0;
        }
        s.flags |= 4;
    });
}
pub fn root_table(list: List, index: u32, table: u64) {
    if rt().is_some() {
        direct::table(list, index, table);
    }
    crate::descriptor_audit::table(list as u64, index, table);
    if index < 32 {
        edit(list, |_, s| {
            s.tables[index as usize] = table;
            s.table_mask |= 1 << index;
        });
    }
}
pub fn root_cbv(list: List, index: u32, address: u64) {
    if index < 32 {
        edit(list, |_, s| {
            s.cbvs[index as usize] = address;
            s.cbv_mask |= 1 << index;
        });
    }
}
pub fn predication(list: List, buffer: *mut c_void) {
    edit(list, |_, s| s.flags = (s.flags & !64) | if buffer.is_null() { 0 } else { 64 });
}
pub fn targets(list: List, count: u32, handles: *const u64, contiguous: bool, depth: *const u64) {
    edit(list, |_, s| {
        s.targets = [0; 8];
        // SAFETY: the API call's handles (or null).
        s.depth = unsafe { depth.as_ref() }.copied().unwrap_or(0);
        (s.rt_count, s.rt_contiguous) = (count, contiguous.into());
        if !handles.is_null() {
            for i in 0..count.min(if contiguous { 1 } else { 8 }) as usize {
                s.targets[i] = unsafe { *handles.add(i) };
            }
        }
        s.flags |= 8;
    });
}
pub fn viewports(list: List, count: u32, values: *const D3D12_VIEWPORT) {
    edit(list, |_, s| {
        s.flags &= !16;
        if count == 1 && !values.is_null() {
            // SAFETY: the API call's viewport.
            s.viewport = unsafe { *values };
            s.flags |= 16;
        }
    });
}
pub fn scissors(list: List, count: u32, values: *const windows::Win32::Foundation::RECT) {
    edit(list, |_, s| {
        s.flags &= !32;
        if count == 1 && !values.is_null() {
            // SAFETY: the API call's rect.
            s.scissor = unsafe { *values };
            s.flags |= 32;
        }
    });
}
fn read_memory(address: u64, out: *mut u8, bytes: usize) -> bool {
    let mut done = 0usize;
    // SAFETY: our own process; a failed read leaves `out` as it was.
    address != 0 && unsafe { windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(windows_sys::Win32::System::Threading::GetCurrentProcess(), address as *const c_void, out.cast(), bytes, &mut done) } != 0 && done == bytes
}
/// CopyInstance: the draw's instance data from its bound instance buffer (into `batch` for a whole batch).
fn copy_instance(resource: *mut c_void, row: &mut DrawRecord, batch: Option<&mut Vec<u8>>) {
    let Some(r) = rt() else { return };
    row.instance_data_bytes = 0;
    row.instance_data = [0; 64];
    if !resource.is_null() && row.instance_heap_type == D3D12_HEAP_TYPE_DEFAULT.0 as u32 {
        let mut bytes = 0usize;
        let copied = (r.copy_default)(row, batch.is_some(), &mut bytes);
        if let Some(batch) = batch {
            // SAFETY: the .cpp's thread buffer, valid until its next call.
            *batch = if copied.is_null() || bytes == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(copied, bytes) }.to_vec() };
        }
        return;
    }
    let (mut offset, mut bytes) = (0u64, 0usize);
    if resource.is_null() || (batch.is_none() && row.instances != 1) || !(r.instance_range)(row, batch.is_some(), &mut offset, &mut bytes) {
        return;
    }
    // SAFETY: the lease's live buffer.
    let Some(buffer) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else { return };
    let mut batch = batch;
    if let Some(b) = batch.as_deref_mut() {
        b.resize(bytes, 0);
    }
    let read = D3D12_RANGE { Begin: offset as usize, End: offset as usize + bytes };
    let mut mapped = core::ptr::null_mut();
    let hr = unsafe { buffer.Map(0, Some(&read), Some(&mut mapped)) };
    if hr.is_err() || mapped.is_null() {
        if let Some(b) = batch.as_deref_mut() {
            b.clear();
        }
        if hr.is_ok() {
            unsafe { buffer.Unmap(0, Some(&D3D12_RANGE::default())) };
        }
        return;
    }
    let output = match batch.as_deref_mut() {
        Some(b) => b.as_mut_ptr(),
        None => row.instance_data.as_mut_ptr(),
    };
    let address = mapped as u64;
    if address <= u64::MAX - offset && read_memory(address + offset, output, bytes) {
        if row.instances == 1 {
            if batch.is_some() {
                // SAFETY: an instance's bytes fit the row's 64.
                unsafe { core::ptr::copy_nonoverlapping(output, row.instance_data.as_mut_ptr(), bytes.min(64)) };
            }
            row.instance_data_bytes = bytes as u32;
        }
    } else if let Some(b) = batch.as_deref_mut() {
        b.clear();
    }
    unsafe { buffer.Unmap(0, Some(&D3D12_RANGE::default())) };
}
fn batch_parts(batch: &[u8]) -> (*const u8, usize) {
    (if batch.is_empty() { core::ptr::null() } else { batch.as_ptr() }, batch.len())
}
/// Draw: a native scene draw captured (the live window: copied for the retained or early-depth consumers; a
/// requested capture: into the exported rows) and routed.
#[allow(clippy::too_many_arguments)]
pub fn draw(list: List, pso: *mut c_void, node: u32, side: i32, indices: u32, instances: u32, first: u32, base: i32, start: u32, retained_observed: Option<&mut bool>) -> bool {
    let mut retained_observed = retained_observed;
    if let Some(o) = retained_observed.as_deref_mut() {
        *o = false;
    }
    let Some(r) = rt() else { return false };
    if node != SCENE_NODE || !(0..=1).contains(&side) || !probing() {
        return false;
    }
    let pso_key = pso as u64;
    if a!(live_bindings).load(Relaxed) != 0 {
        let (early, retained) = (early_drawing(), retained_active());
        if !early && !retained {
            return false;
        }
        let input = input_mask_of(pso as usize);
        let mut snapshot = zeroed_record();
        let mut copied = false;
        let mut batch: Vec<u8> = Vec::new();
        edit(list, |s, saved| {
            if saved.pipeline != pso_key {
                return;
            }
            s.live_sequence += 1;
            snapshot = *saved;
            (snapshot.sequence, snapshot.frame, snapshot.node, snapshot.side) = (s.live_sequence, s.frame, node, side as u32);
            (snapshot.list, snapshot.indices, snapshot.instances, snapshot.first_index, snapshot.base_vertex, snapshot.first_instance) = (list as u64, indices, instances, first, base, start);
            snapshot.input_slot_mask = input.unwrap_or(0);
            if input.is_some() {
                snapshot.flags |= 128;
            }
            if early {
                let resource = instance(3, list);
                if !resource.is_null() {
                    copy_instance(resource, &mut snapshot, Some(&mut batch));
                }
            }
            copied = true;
        });
        if copied && retained {
            if let Some(o) = retained_observed.as_deref_mut() {
                *o = true;
            }
            if retained_draw(list, &snapshot) {
                draw_recorded(list);
                return true;
            }
        }
        let (p, n) = batch_parts(&batch);
        if copied && early && early_draw(list, &snapshot, p, n) {
            draw_recorded(list);
            return true;
        }
        return false;
    }
    let (mut candidate, mut variant) = (false, core::ptr::null_mut::<c_void>());
    let count = a!(pipeline_count).load(Acquire) as usize;
    for i in 0..count.min(PIPELINE_CAPACITY) {
        // SAFETY: the exported rows below the count.
        let pipeline = unsafe { &*r.pipelines.add(i) };
        if pipeline.original == pso_key {
            candidate = true;
            let f = &pipeline.formats;
            if (pipeline.targets == 0 || (pipeline.targets == 3 && f[0] == 24 && f[1] == 24 && f[2] == 28)) && pipeline.depth_format == 20 && pipeline.sample_count == 1 {
                variant = pipeline.instanced as *mut c_void;
            }
        }
    }
    if a!(gpu_scene_depth).load(Relaxed) != 0 && (a!(gpu_state).load(Acquire) == 1 || a!(gpu_request).load(Relaxed) != 0) {
        let mut current = zeroed_record();
        let mut frame = 0;
        edit(list, |s, saved| {
            current = *saved;
            frame = s.frame;
        });
        (current.side, current.frame, current.pipeline) = (side as u32, frame, pso_key);
        gpu_boundary(list, &current, variant);
    }
    if !candidate && a!(all_draws).load(Relaxed) == 0 && !retained_active() && !direct::paired_active() {
        return false;
    }
    let input = input_mask_of(pso as usize);
    let mut snapshot = zeroed_record();
    let (mut copied, mut early_copied, mut retained_copied, mut paired_copied) = (false, false, false, false);
    let mut batch: Vec<u8> = Vec::new();
    edit(list, |s, saved| {
        if saved.pipeline != pso_key {
            return;
        }
        let n = a!(draw_count).load(Relaxed) as usize;
        if n == DRAW_CAPACITY {
            a!(probe_state).store(3, Release);
            return;
        }
        a!(probe_seq).fetch_add(1, AcqRel);
        // SAFETY: the exported rows, `n` below capacity.
        let row = unsafe { &mut *r.draws.add(n) };
        *row = *saved;
        let mut now = 0i64;
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
        (row.sequence, row.qpc, row.frame, row.node, row.side) = (n as u64 + 1, now as u64, s.frame, node, side as u32);
        (row.list, row.pipeline, row.indices, row.instances, row.first_index, row.base_vertex, row.first_instance) = (list as u64, pso_key, indices, instances, first, base, start);
        row.thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        row.input_slot_mask = input.unwrap_or(0);
        if input.is_some() {
            row.flags |= 128;
        }
        // SAFETY: the exported target rows.
        let targets = unsafe { r.targets.add(n * TARGET_RECORD_BYTES) };
        unsafe { core::ptr::write_bytes(targets, 0, TARGET_RECORD_BYTES) };
        if a!(preparation_all_stages).load(Relaxed) != 0 {
            let mut tag = DrawTag::default();
            draw_tag(&mut tag);
            let head = unsafe { &mut *targets.cast::<TargetsHead>() };
            (head.sequence, head.scope, head.stage, head.frame, head.colors) = (row.sequence, tag.scope, tag.stage, tag.frame, row.rt_count);
            head.status = if tag.scope == 0 {
                2
            } else if tag.side != row.side as i32 {
                4
            } else if direct::describe_targets(row, unsafe { targets.add(core::mem::size_of::<TargetsHead>()) }) {
                1
            } else {
                3
            };
        }
        let depth_batch = !variant.is_null() && row.rt_count == 0 && a!(gpu_depth_prepass).load(Relaxed) != 0 && (a!(gpu_request).load(Relaxed) != 0 || a!(gpu_state).load(Relaxed) == 1);
        let own_instances = depth_batch || a!(constant_capture).load(Relaxed) != 0 || a!(early_depth_state).load(SeqCst) == 1;
        let resource = instance(3, list);
        if !resource.is_null() {
            copy_instance(resource, row, own_instances.then_some(&mut batch));
        }
        crate::descriptor_audit::snapshot(row);
        let (p, len) = batch_parts(&batch);
        (r.snapshot_geometry)(list, pso, row, p, len);
        a!(draw_count).store(n as u32 + 1, Relaxed);
        a!(probe_seq).fetch_add(1, Release);
        if a!(gpu_request).load(Relaxed) != 0 || (a!(gpu_native_reference).load(Relaxed) != 0 && a!(gpu_state).load(Relaxed) == 1) {
            snapshot = *row;
            copied = true;
        }
        if a!(early_depth_state).load(SeqCst) == 1 {
            snapshot = *row;
            early_copied = true;
        }
        if retained_active() {
            snapshot = *row;
            retained_copied = true;
        }
        if direct::paired_active() {
            snapshot = *row;
            paired_copied = true;
        }
    });
    if retained_copied {
        if let Some(o) = retained_observed.as_deref_mut() {
            *o = true;
        }
        if retained_draw(list, &snapshot) {
            draw_recorded(list);
            return true;
        }
    }
    if paired_copied && direct::paired_draw(list, &snapshot) {
        draw_recorded(list);
        return true;
    }
    let (p, n) = batch_parts(&batch);
    if early_copied && early_draw(list, &snapshot, p, n) {
        draw_recorded(list);
        return true;
    }
    if copied {
        return gpu_draw(list, variant, &snapshot, p, n);
    }
    false
}
/// CurrentState: the list's mirrored state with its pipeline's input mask.
pub fn current_state(list: List) -> Option<DrawRecord> {
    if cvr_host::probe::internal() || list.is_null() || !probing() {
        return None;
    }
    let mut result = {
        let guard = states();
        if !probing() {
            return None;
        }
        *guard.as_ref().expect("initialized").states.get(&(list as usize))?
    };
    result.input_slot_mask = 0;
    result.flags &= !128;
    result.list = list as u64;
    if let Some(mask) = input_mask_of(result.pipeline as usize) {
        result.input_slot_mask = mask;
        result.flags |= 128;
    }
    Some(result)
}

// ---- the C ABI
/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsps_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// The list calls: 0 reset (`a` the pipeline, `b` rate known, `c` new recording), 1 heaps (`n`, `a` the heaps), 2
/// pipeline (`a`), 3 vertex buffers (`n` from `index`, `a` the views), 4 index buffer (`a`), 5 root signature (`a`),
/// 6 root table (`index`, `value`), 7 root CBV (`index`, `value`), 8 predication (`a`), 9 targets (`n`, `a` the
/// handles, `b` contiguous, `value` the depth handle's address), 10 viewports (`n`, `a`), 11 scissors (`n`, `a`).
///
/// # Safety
/// The API call's arguments.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nsps_list(which: u32, list: List, index: u32, n: u32, a: *mut c_void, b: bool, c: bool, value: u64) {
    match which {
        0 => reset(list, a, b, c),
        1 => heaps(list, n, a as *const *mut c_void),
        2 => pipeline(list, a),
        3 => vertex_buffers(list, index, n, a as *const _),
        4 => index_buffer(list, a as *const _),
        5 => root_signature(list, a),
        6 => root_table(list, index, value),
        7 => root_cbv(list, index, value),
        8 => predication(list, a),
        9 => targets(list, n, a as *const u64, b, value as *const u64),
        10 => viewports(list, n, a as *const _),
        11 => scissors(list, n, a as *const _),
        _ => {}
    }
}
/// # Safety
/// The API call's arguments; `retained` is writable (or null).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nsps_draw(list: List, pso: *mut c_void, node: u32, side: i32, indices: u32, instances: u32, first: u32, base: i32, start: u32, retained: *mut bool) -> bool {
    draw(list, pso, node, side, indices, instances, first, base, start, unsafe { retained.as_mut() })
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nsps_frame_boundary(live: bool) {
    frame_boundary(live);
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsps_current(list: List, out: *mut DrawRecord) -> bool {
    match current_state(list) {
        Some(state) => {
            unsafe { *out = state };
            true
        }
        None => {
            unsafe { *out = zeroed_record() };
            false
        }
    }
}
/// The pipelines: 0 observe `input`'s mask, 1 forget the mask, 2 add with `info`, `create` and `context`, 3 a root
/// created (`input` its blob, `bytes` its size).
///
/// # Safety
/// The creation's arguments.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nsps_pipeline(which: u32, device: *mut c_void, original: *mut c_void, input: *const c_void, bytes: usize, info: *const PipelineInfo, create: Option<Create>, context: *const c_void) {
    match which {
        0 => {
            if let Some(input) = unsafe { input.cast::<D3D12_INPUT_LAYOUT_DESC>().as_ref() } {
                observe_input(original as usize, input);
            }
        }
        1 => forget_input(original as usize),
        2 => {
            if let (Some(info), Some(create)) = (unsafe { info.as_ref() }, create) {
                add_pipeline(device, original as usize, info, create, context);
            }
        }
        3 => root_created(original, input.cast(), bytes),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    //! The probe idle without a capture: no list state, no draw, no current state; the pipeline registry's masks.
    use super::*;

    #[test]
    fn idle_lists_and_masks() {
        let list = 0x5000 as List;
        pipeline(list, 0x10 as *mut c_void);
        root_cbv(list, 3, 0x40);
        assert!(states().as_ref().unwrap().states.is_empty());
        assert!(current_state(list).is_none());
        let mut retained = true;
        assert!(!draw(list, 0x10 as *mut c_void, SCENE_NODE, 0, 3, 1, 0, 0, 0, Some(&mut retained)) && !retained);
        assert!(!draw(list, 0x10 as *mut c_void, 7, 0, 3, 1, 0, 0, 0, None));
        // The input masks: a representable layout recorded, then forgotten.
        let elements = [D3D12_INPUT_ELEMENT_DESC { InputSlot: 2, ..Default::default() }];
        let layout = D3D12_INPUT_LAYOUT_DESC { pInputElementDescs: elements.as_ptr(), NumElements: 1 };
        observe_input(0x77, &layout);
        let observed = input_mask_of(0x77);
        forget_input(0x77);
        assert!(input_mask_of(0x77).is_none());
        if let Some(mask) = observed {
            assert_ne!(mask, 0);
        }
        let mut out = zeroed_record();
        out.sequence = 9;
        assert!(!unsafe { cvr_nsps_current(list, &mut out) } && out.sequence == 0);
    }
}
