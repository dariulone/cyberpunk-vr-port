//! THE PAIRED GEOMETRY PROBE (formerly src/Render/NativePairedGeometry.cpp): a tool requests one native draw pair
//! (CyberpunkVR_PairedGeometryRequest); each eye's native draw runs natively once with its writable targets checkpointed
//! before and after into private copies; on the second eye the pair is drawn again view-instanced into a private
//! two-layer chain with the paired native camera, both eyes' proofs (copies into a readback) are recorded, and once
//! both submissions' fences have passed the readbacks are compared into the CyberpunkVR_PairedGeometry* rows. Dormant
//! unless requested.
//!
//! The probe's state lives here under its lock; the camera rows arrive from the trace (`camera`, `bound_camera`); the
//! submissions from the queue detours. The C++ API (Active, Draw, FrameBoundary for the native probe's host) forwards
//! here; the families' bound family is read through src/Render/NativeStereoFamilies.cpp (a held shared_ptr).
#![cfg_attr(not(feature = "rust-build"), allow(dead_code))]
use core::cell::RefCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::{IUnknown, Interface, HRESULT};
use windows::Win32::Foundation::E_UNEXPECTED;
use windows::Win32::Graphics::Direct3D12::*;

use cvr_gpu::scene_state::{Bindings, DrawTargets, GeometryTargetsRaw, TargetInfo};

use crate::camera_binding::{BindingScope, Context};
use crate::camera_upload::{Camera, Upload, CAMERA_BYTES};
use crate::geometry_commands::DrawRecord;
use crate::native_recording::Lease;
use crate::paired_geometry_checks::{Plane, ProofHost, ProofOut, ProofTargets};

/// preparation::DrawTag.
#[repr(C)]
#[derive(Clone, Copy)]
struct DrawTag {
    scope: u64,
    stage: u32,
    frame: u32,
    side: i32,
}
/// What families_native.rs reads of a bound family (its Arc held by the handle; families_native::BoundView).
#[repr(C)]
struct FamilyView {
    identity: *const c_void,
    id: u64,
    vertex: *const u8,
    vertex_len: usize,
    pixel: *const u8,
    pixel_len: usize,
    native_camera_pipeline: *mut c_void,
    native_camera_stages: u32,
    source_root: *mut c_void,
}
const _: () = assert!(core::mem::size_of::<FamilyView>() == 72);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_PairedGeometryRequest: AtomicU32;
    static CyberpunkVR_PairedGeometryState: AtomicU32;
    static CyberpunkVR_PairedGeometryOrdinal: AtomicU32;
    static CyberpunkVR_PairedGeometryFamily: AtomicU32;
    static mut CyberpunkVR_PairedGeometryStats: [u64; 16];
    static mut CyberpunkVR_PairedGeometryDifferent: [u64; 12];
    static mut CyberpunkVR_PairedGeometryChanged: [u64; 12];
    static mut CyberpunkVR_PairedGeometryBytes: [u64; 2];
    static mut CyberpunkVR_PairedGeometryCameraDiagnostics: [u64; 16];
    static CyberpunkVR_PreparationPairMerged: AtomicU32;
    static mut CyberpunkVR_PreparationPairData: [u64; 8];

    /// preparation::CurrentDrawTag (preparation_trace.rs in the plugin).
    fn cvr_prep_draw_tag(out: *mut DrawTag);
    /// native_probe::ObserveBindings (stereo_probe.rs in the plugin).
    fn cvr_observe_bindings() -> bool;
    /// families::Bound (families_native.rs): the list's bound family (a held handle), null when none.
    #[link_name = "cvr_nsf2_bound_view"]
    fn cvr_cpp_family_bound(list: *mut c_void, out: *mut FamilyView) -> *mut c_void;
    #[link_name = "cvr_nsf2_bound_release"]
    fn cvr_cpp_family_release(handle: *mut c_void);
}

const VERTEX_SHA256: &[u8] = b"84b48dd0ebdd29fbaf7895d84acf3e3030eb265d9423fc92dbb6cb56a41df63f";
const PIXEL_SHA256: &[u8] = b"0cf4b32c82315ba36b2ae78b957274a4cf65577d4d76ff126238d75c833df5b2";

fn stats() -> &'static mut [u64; 16] {
    // SAFETY: the exported rows, written under the probe's lock (the counters' tools only read them).
    unsafe { &mut *(&raw mut CyberpunkVR_PairedGeometryStats) }
}
fn state() -> &'static AtomicU32 {
    // SAFETY: an exported atomic.
    unsafe { &CyberpunkVR_PairedGeometryState }
}
fn request() -> &'static AtomicU32 {
    // SAFETY: an exported atomic.
    unsafe { &CyberpunkVR_PairedGeometryRequest }
}
fn copy_diagnostics() {
    let mut value = [0u64; 16];
    // SAFETY: a writable row; the exported diagnostics written under the lock.
    unsafe {
        crate::camera_upload::cvr_ncu_diagnostics(&mut value);
        *(&raw mut CyberpunkVR_PairedGeometryCameraDiagnostics) = value;
    }
}
fn base() -> u64 {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: the game module's handle.
    unsafe { GetModuleHandleW(None) }.map_or(0, |h| h.0 as u64)
}
fn current_context() -> Context {
    let mut context = Context::default();
    // SAFETY: a writable record.
    unsafe { crate::camera_upload::cvr_ncu_current(base(), &mut context) };
    context
}
fn internal() -> bool {
    cvr_host::probe::internal()
}
fn release(token: *mut c_void) {
    if !token.is_null() {
        // SAFETY: a reference this side holds.
        drop(unsafe { IUnknown::from_raw(token) });
    }
}
fn raw_of<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), |v| v.as_raw())
}

/// A bound family, held (its pipelines and root stay alive with it).
struct Family {
    handle: *mut c_void,
    view: FamilyView,
}
impl Drop for Family {
    fn drop(&mut self) {
        // SAFETY: the handle the families gave.
        unsafe { cvr_cpp_family_release(self.handle) };
    }
}
impl Family {
    fn bound(list: *mut c_void) -> Option<Family> {
        // SAFETY: every field is valid zeroed.
        let mut view: FamilyView = unsafe { core::mem::zeroed() };
        // SAFETY: a live list; a writable view.
        let handle = unsafe { cvr_cpp_family_bound(list, &mut view) };
        (!handle.is_null()).then_some(Family { handle, view })
    }
    fn text(p: *const u8, n: usize) -> &'static [u8] {
        // SAFETY: the family's string while it is held.
        if p.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(p, n) } }
    }
    fn vertex(&self) -> &[u8] {
        Self::text(self.view.vertex, self.view.vertex_len)
    }
    fn pixel(&self) -> &[u8] {
        Self::text(self.view.pixel, self.view.pixel_len)
    }
}

fn empty_lease() -> Lease {
    Lease { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 }
}
/// A native recording's lease, its token owned.
struct OwnedLease(Lease);
impl Default for OwnedLease {
    fn default() -> Self {
        OwnedLease(empty_lease())
    }
}
impl Drop for OwnedLease {
    fn drop(&mut self) {
        release(self.0.token);
    }
}

/// scene_state::GeometryTargets: the five resources (owned), their views and descriptors, the colour count.
#[derive(Default)]
struct Targets {
    resources: [Option<ID3D12Resource>; 5],
    views: [TargetInfo; 5],
    handles: [usize; 5],
    colors: u32,
}
impl Targets {
    fn raw(&self) -> GeometryTargetsRaw {
        GeometryTargetsRaw {
            resources: core::array::from_fn(|i| raw_of(&self.resources[i]) as usize),
            views: self.views,
            handles: self.handles,
            colors: self.colors,
        }
    }
    fn from_owned(raw: &GeometryTargetsRaw) -> Targets {
        Targets {
            // SAFETY: the references the producer handed over.
            resources: core::array::from_fn(|i| unsafe { (raw.resources[i] != 0).then(|| ID3D12Resource::from_raw(raw.resources[i] as *mut c_void)) }),
            views: raw.views,
            handles: raw.handles,
            colors: raw.colors,
        }
    }
}

/// GeometryTargetCheckpoint: once-recorded private copies of a native draw's targets.
#[derive(Default)]
struct Checkpoint {
    rust: Option<core::ptr::NonNull<c_void>>,
    targets: Targets,
    captured: bool,
}
impl Drop for Checkpoint {
    fn drop(&mut self) {
        if let Some(rust) = self.rust {
            // SAFETY: the checkpoint created below.
            unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_free(rust.as_ptr().cast()) };
        }
    }
}
impl Checkpoint {
    fn create(&mut self, device: &ID3D12Device, shape: &Targets) -> HRESULT {
        if self.rust.is_some() {
            return E_UNEXPECTED;
        }
        let raw = shape.raw();
        let mut out: *mut cvr_gpu::geometry_checkpoint::Checkpoint = null_mut();
        // SAFETY: a live device; the shape; a writable output.
        let hr = unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_create(device.as_raw(), &raw, &mut out) };
        if hr.is_err() {
            return hr;
        }
        self.rust = core::ptr::NonNull::new(out.cast());
        hr
    }
    fn capture(&mut self, list: *mut c_void, native: &Targets) -> bool {
        let Some(rust) = self.rust else { return false };
        if list.is_null() || self.captured {
            return false;
        }
        let raw = native.raw();
        // SAFETY: our checkpoint; the live list; the native targets.
        if !unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_capture(rust.as_ptr().cast(), list, &raw) } {
            return false;
        }
        let mut targets = GeometryTargetsRaw::default();
        // SAFETY: as above; the private targets come with their references.
        if !unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_targets(rust.as_ptr().cast(), &mut targets) } {
            return false;
        }
        self.targets = Targets::from_owned(&targets);
        self.captured = true;
        true
    }
    fn targets(&self) -> Option<&Targets> {
        self.captured.then_some(&self.targets)
    }
}

/// The camera a trace row named (pending) and the one the native draw bound (bound).
#[derive(Clone, Copy)]
struct CameraData {
    bytes: Camera,
    context: Context,
    epoch: u64,
    view: u64,
    frame: u32,
    handle: u32,
    stages: u32,
    side: i32,
}
impl Default for CameraData {
    fn default() -> Self {
        CameraData { bytes: [0; CAMERA_BYTES], context: Context::default(), epoch: 0, view: 0, frame: 0, handle: 0, stages: 0, side: -1 }
    }
}
thread_local! {
    static PENDING: RefCell<CameraData> = RefCell::new(CameraData::default());
    static BOUND: RefCell<CameraData> = RefCell::new(CameraData::default());
}

struct Eye {
    before: Checkpoint,
    after: Checkpoint,
    draw: DrawRecord,
    recording: OwnedLease,
    camera: CameraData,
    fence: Option<ID3D12Fence>,
    readback: Option<ID3D12Resource>,
    planes: Vec<Plane>,
    native_executed: bool,
    captured: bool,
    pending: bool,
    submitted: bool,
}
impl Default for Eye {
    fn default() -> Self {
        Eye {
            before: Checkpoint::default(),
            after: Checkpoint::default(),
            // SAFETY: a plain record, valid zeroed.
            draw: unsafe { core::mem::zeroed() },
            recording: OwnedLease::default(),
            camera: CameraData::default(),
            fence: None,
            readback: None,
            planes: Vec::new(),
            native_executed: false,
            captured: false,
            pending: false,
            submitted: false,
        }
    }
}
/// StereoGeometryChain's storage (released as its ComPtr was).
struct Chain(*mut c_void);
impl Drop for Chain {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the chain created below.
            unsafe { crate::geometry_chain::cvr_geometry_chain_release(self.0) };
        }
    }
}
struct Work {
    device: Option<ID3D12Device>,
    main_queue: Option<ID3D12CommandQueue>,
    original_pipeline: Option<ID3D12PipelineState>,
    family: Option<Family>,
    eyes: [Eye; 2],
    chain: Option<Chain>,
    /// The camera upload's heap (a ComPtr's pointer the stages fill).
    camera_heap: *mut c_void,
    frame: u32,
    ordinal: u32,
    selector: u32,
    failed: bool,
    complete: bool,
}
impl Default for Work {
    fn default() -> Self {
        Work {
            device: None,
            main_queue: None,
            original_pipeline: None,
            family: None,
            eyes: [Eye::default(), Eye::default()],
            chain: None,
            camera_heap: null_mut(),
            frame: 0,
            ordinal: 0,
            selector: 0,
            failed: false,
            complete: false,
        }
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        release(self.camera_heap);
    }
}

struct Held(Option<Box<Work>>);
// SAFETY: the work's COM objects are only touched under the lock.
unsafe impl Send for Held {}
static LOCK: Mutex<Held> = Mutex::new(Held(None));
static WATCHING: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(0);
fn lock() -> MutexGuard<'static, Held> {
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}
fn fail(work: Option<&mut Work>, reason: u32) {
    if let Some(work) = work {
        work.failed = true;
    }
    let stats = stats();
    if stats[14] == 0 {
        stats[14] = u64::from(reason);
    }
    state().store(4, SeqCst);
}

fn native_queue(lease: &Lease, queue: &ID3D12CommandQueue) -> bool {
    crate::paired_geometry_checks::cvr_npg_native_queue(base(), lease.registry, queue.as_raw() as u64)
        // SAFETY: a live queue.
        && unsafe { queue.GetDesc() }.Type == D3D12_COMMAND_LIST_TYPE_DIRECT
}
fn native_draw(list: &ID3D12GraphicsCommandList, draw: &DrawRecord) {
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the live list replays the native draw.
    unsafe { list.DrawIndexedInstanced(draw.indices, draw.instances, draw.first_index, draw.base_vertex, draw.first_instance) };
}
fn capture_geometry_targets(draw: &DrawRecord) -> Option<Targets> {
    let targets = DrawTargets { rt_count: draw.rt_count, rt_contiguous: draw.rt_contiguous, targets: draw.targets, depth: draw.depth, viewport: draw.viewport };
    let mut raw = GeometryTargetsRaw::default();
    // SAFETY: local records; the resources come with their references.
    unsafe { cvr_gpu::scene_state::cvr_scene_capture_geometry_targets(&targets, &mut raw) }.then(|| Targets::from_owned(&raw))
}
fn writable_targets(list: *mut c_void, targets: &Targets) -> bool {
    if targets.colors != 3 {
        return false;
    }
    [0usize, 1, 2, 4].iter().all(|&slot| {
        let expected = if slot == 4 { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
        // SAFETY: the live list and the draw's target.
        unsafe { crate::texture_writable::cvr_tr_validate_writable_target(list, raw_of(&targets.resources[slot]), expected.0 as u32) }
    })
}

/// The native draw's state put back after the pair's own draw: the view-instance mask, the pipeline, the targets, the
/// viewport and scissor (the camera stages ended first).
struct Restore {
    views: Option<ID3D12GraphicsCommandList1>,
    scope: BindingScope,
    valid: bool,
    camera_touched: bool,
}
impl Restore {
    fn new(list: &ID3D12GraphicsCommandList, draw: &DrawRecord) -> Restore {
        let mut bindings = Bindings::default();
        // SAFETY: the live list; a writable record.
        let snapshot = unsafe { cvr_gpu::scene_state::cvr_scene_snapshot(list.as_raw() as usize, &mut bindings) };
        let views = list.cast::<ID3D12GraphicsCommandList1>().ok();
        let valid = snapshot && bindings.reset_seen && bindings.heaps_seen && bindings.root as u64 == draw.root && views.is_some();
        Restore { views, scope: BindingScope::default(), valid, camera_touched: false }
    }
    fn finish(mut self, work: &mut Work, list: &ID3D12GraphicsCommandList, draw: &DrawRecord) {
        if !self.valid {
            return;
        }
        // SAFETY: the scope the stages began.
        if self.camera_touched && !unsafe { crate::camera_upload::cvr_ncu_end_stages(true, base(), &mut self.scope) } {
            copy_diagnostics();
            fail(Some(work), 31);
        }
        let _internal = cvr_host::probe::Internal::new();
        let colors: [D3D12_CPU_DESCRIPTOR_HANDLE; 8] = core::array::from_fn(|i| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: draw.targets[i] as usize });
        let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: draw.depth as usize };
        // SAFETY: the live list and the native draw's own state.
        unsafe {
            if let Some(views) = &self.views {
                views.SetViewInstanceMask(u32::MAX);
            }
            let pipeline = draw.pipeline as *mut c_void;
            list.SetPipelineState(ID3D12PipelineState::from_raw_borrowed(&pipeline));
            list.OMSetRenderTargets(
                draw.rt_count,
                (draw.rt_count != 0).then_some(colors.as_ptr()),
                draw.rt_contiguous != 0,
                (draw.depth != 0).then_some(&depth as *const D3D12_CPU_DESCRIPTOR_HANDLE),
            );
            list.RSSetViewports(&[draw.viewport]);
            list.RSSetScissorRects(&[draw.scissor]);
        }
    }
}

unsafe extern "C-unwind" fn proof_keep(_: *mut c_void, list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    // SAFETY: the proof's live list and objects.
    (count == 1 || count == 3) && unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, objects, count) }
}
unsafe extern "C-unwind" fn proof_internal(user: *mut c_void) {
    // SAFETY: the record_proof's scope slot (the optional's emplace: the old scope ends before the new one begins).
    let slot = unsafe { &mut *user.cast::<Option<cvr_host::probe::Internal>>() };
    slot.take();
    *slot = Some(cvr_host::probe::Internal::new());
}
fn record_proof(work: &mut Work, list: &ID3D12GraphicsCommandList, side: usize) -> bool {
    let chain = work.chain.as_ref().map_or(null_mut(), |c| c.0);
    let eye = &work.eyes[side];
    let empty = Targets::default();
    let (expected, initial) = (eye.after.targets().unwrap_or(&empty), eye.before.targets().unwrap_or(&empty));
    let mut t = ProofTargets { native: [null_mut(); 5], array: [null_mut(); 5], before: [null_mut(); 5] };
    for slot in [0usize, 1, 2, 4] {
        t.native[slot] = raw_of(&expected.resources[slot]);
        t.array[slot] = crate::geometry_chain::cvr_geometry_chain_resource(chain, slot as u32);
        t.before[slot] = raw_of(&initial.resources[slot]);
    }
    let mut rows = [Plane::default(); 8];
    let mut out = ProofOut { planes: rows.as_mut_ptr(), capacity: rows.len(), count: 0, readback: null_mut() };
    let mut internal: Option<cvr_host::probe::Internal> = None;
    let host = ProofHost { user: (&raw mut internal).cast(), keep: proof_keep, internal: proof_internal };
    // SAFETY: the live device and list; local records.
    let recorded = unsafe {
        crate::paired_geometry_checks::cvr_npg_record_proof(raw_of(&work.device), list.as_raw(), side as u32, &t, &mut out, &host)
    };
    let eye = &mut work.eyes[side];
    if !out.readback.is_null() {
        // SAFETY: the readback the proof created, with its reference.
        eye.readback = Some(unsafe { ID3D12Resource::from_raw(out.readback) });
    }
    if !recorded {
        return false;
    }
    eye.planes = rows[..out.count.min(rows.len())].to_vec();
    drop(internal);
    true
}

fn poll(slot: &mut Option<Box<Work>>) {
    let Some(work) = slot.as_deref_mut() else { return };
    if work.complete || (!work.failed && stats()[4] == 0) {
        return;
    }
    for side in 0..2 {
        let eye = &work.eyes[side];
        if eye.native_executed {
            let (true, Some(fence)) = (eye.submitted, eye.fence.as_ref()) else { return };
            // SAFETY: a live fence.
            let done = unsafe { fence.GetCompletedValue() };
            if done == u64::MAX {
                fail(Some(work), 41);
                work.complete = true;
                WATCHING.store(false, SeqCst);
                return;
            }
            if done < 1 {
                return;
            }
        }
    }
    if !work.failed {
        for side in 0..2 {
            let eye = &work.eyes[side];
            let (Some(readback), false) = (eye.readback.clone(), eye.planes.is_empty()) else {
                fail(Some(work), 42);
                break;
            };
            // SAFETY: a live readback; the planes; the exported rows written under the lock.
            let failed = unsafe {
                crate::private_submits::cvr_npg_compare_readback(
                    readback.as_raw(),
                    eye.planes.as_ptr(),
                    eye.planes.len(),
                    side as u32,
                    &raw mut CyberpunkVR_PairedGeometryDifferent,
                    &raw mut CyberpunkVR_PairedGeometryBytes,
                    &raw mut CyberpunkVR_PairedGeometryChanged,
                )
            } != 0;
            if failed {
                fail(Some(work), 43);
                break;
            }
        }
    }
    // SAFETY: the exported row, read under the lock.
    if !work.failed && unsafe { (*(&raw const CyberpunkVR_PairedGeometryDifferent)).iter().any(|&v| v != 0) } {
        fail(Some(work), 44);
    }
    if !work.failed {
        state().store(3, SeqCst);
    }
    work.complete = true;
    WATCHING.store(false, SeqCst);
}

pub fn active() -> bool {
    WATCHING.load(Relaxed) || request().load(Relaxed) != 0
}
pub fn needs_camera() -> bool {
    active()
}

/// FrameBoundary: the work polled; a new request starts a fresh one (its rows cleared) if every condition holds.
pub fn frame_boundary() {
    if !active() && state().load(SeqCst) != 2 {
        return;
    }
    let mut held = lock();
    let slot = &mut held.0;
    poll(slot);
    if let Some(work) = slot.as_deref_mut().filter(|w| !w.complete && state().load(SeqCst) == 0) {
        fail(Some(work), 45);
        poll(slot);
    }
    let requested = request().swap(0, SeqCst);
    if requested == 0 {
        return;
    }
    if let Some(work) = slot.as_deref_mut() {
        if !work.complete {
            fail(Some(work), 1);
            return;
        }
    }
    *slot = Some(Box::default());
    let work = slot.as_deref_mut().expect("just set");
    // SAFETY: exported atomics and rows, written under the lock.
    unsafe {
        work.ordinal = CyberpunkVR_PairedGeometryOrdinal.load(SeqCst);
        work.selector = CyberpunkVR_PairedGeometryFamily.load(SeqCst);
        EPOCH.fetch_add(1, Release);
        stats().fill(0);
        (*(&raw mut CyberpunkVR_PairedGeometryDifferent)).fill(0);
        (*(&raw mut CyberpunkVR_PairedGeometryChanged)).fill(0);
        (*(&raw mut CyberpunkVR_PairedGeometryBytes)).fill(0);
        (*(&raw mut CyberpunkVR_PairedGeometryCameraDiagnostics)).fill(0);
    }
    stats()[15] = u64::from(requested);
    // SAFETY: plain queries of the plugin's other halves.
    let refused = unsafe {
        requested != 1
            || work.ordinal >= 64
            || (work.selector != 0 && work.selector != 70)
            || !cvr_observe_bindings()
            || !crate::native_recording::cvr_native_recording_compatible()
            || crate::retained_geometry_native::cvr_nrg2_active()
            || crate::early_depth_native::cvr_ned_drawing()
            || crate::early_depth_native::cvr_ned_run_tracking()
    };
    if refused {
        fail(Some(work), 2);
        work.complete = true;
        return;
    }
    WATCHING.store(true, SeqCst);
    state().store(1, SeqCst);
}

/// Camera: the trace's row of a main-camera upload for one eye of this frame's pair, kept as pending.
#[cfg(feature = "rust-build")]
pub fn camera(row: &crate::single_pass_trace::Record) {
    if internal() || !needs_camera() {
        return;
    }
    PENDING.with(|p| *p.borrow_mut() = CameraData::default());
    BOUND.with(|b| *b.borrow_mut() = CameraData::default());
    // SAFETY: the exported row, read only.
    let expected_view = unsafe { (*(&raw const CyberpunkVR_PreparationPairData))[if row.side != 0 { 3 } else { 4 }] };
    if row.side < 0 || row.side > 1 || row.node != 0x33DDA4 || row.input_valid == 0 || row.index == u32::MAX || row.index == 0 || row.view != expected_view {
        return;
    }
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.bytes = row.camera;
        p.epoch = EPOCH.load(Acquire);
        p.view = row.view;
        // SAFETY: an exported atomic.
        p.frame = unsafe { CyberpunkVR_PreparationPairMerged.load(SeqCst) };
        p.side = row.side;
        p.handle = row.index;
    });
}
/// BoundCamera: the pending camera bound by the native draw's upload (its stages accumulated).
pub fn bound_camera(side: i32, binding: u32, handle: u32, stages: u32) {
    if internal() || !needs_camera() || binding != 1 || stages & 3 == 0 {
        return;
    }
    let context = current_context();
    let pending = PENDING.with(|p| *p.borrow());
    BOUND.with(|b| {
        let mut bound = b.borrow_mut();
        if !context.valid() || pending.epoch != EPOCH.load(Acquire) || pending.side != side || pending.handle != handle {
            *bound = CameraData::default();
            return;
        }
        if bound.handle != handle || bound.context != context || bound.epoch != pending.epoch {
            *bound = pending;
            bound.context = context;
            bound.stages = 0;
        }
        bound.stages |= stages & 3;
    });
}

/// Draw: at the native draw of an eye of the requested pair -- executed natively between two checkpoints; the second
/// eye then draws the pair view-instanced with the paired camera and records both proofs. True when the native draw was
/// executed here (the caller must not execute it again).
pub fn draw(list: *mut c_void, draw: &DrawRecord) -> bool {
    if internal() || !active() {
        return false;
    }
    let mut tag = DrawTag { scope: 0, stage: 0, frame: 0, side: -1 };
    // SAFETY: a writable record.
    unsafe { cvr_prep_draw_tag(&mut tag) };
    let mut held = lock();
    let Some(work) = held.0.as_deref_mut() else { return false };
    // SAFETY: the detour's live list.
    let Some(command) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }).cloned() else { return false };
    if work.complete
        || work.failed
        || draw.side > 1
        || tag.side != draw.side as i32
        || tag.stage != 7
        || tag.scope == 0
        || tag.frame == 0
        || crate::retained_geometry_native::cvr_nrg2_in_render_pass(list)
    {
        return false;
    }
    let side = draw.side as usize;
    if work.eyes[side].native_executed {
        return false;
    }
    let family = Family::bound(list);
    let camera_stages = if work.selector != 0 { 3u32 } else { 1 };
    let shader = family.as_ref().is_some_and(|f| {
        if work.selector != 0 {
            f.vertex() == VERTEX_SHA256 && f.pixel() == PIXEL_SHA256
        } else {
            let (v, p) = (f.vertex(), f.pixel());
            // SAFETY: the family's strings.
            unsafe { crate::gbuffer_material::cvr_gm_matches_shaders(v.as_ptr(), v.len(), p.as_ptr(), p.len()) }
        }
    });
    let Some(family) = family.filter(|_| shader) else { return false };
    let v = &family.view;
    if v.native_camera_pipeline.is_null()
        || v.native_camera_stages != camera_stages
        || crate::families_status::native_geometry_status(draw.pipeline as *mut c_void) != 1
        || v.source_root as u64 != draw.root
        || (work.selector == 0 && draw.indices != 6)
        || draw.indices < 3
        || draw.indices % 3 != 0
        || draw.instances != 1
        || draw.vertices[7].StrideInBytes != 48
        || draw.instance_data_bytes != 48
        || draw.rt_count != 3
        || (draw.flags & 255) != 191
        || draw.depth == 0
        || draw.viewport.Width == 0.0
        || draw.viewport.Height == 0.0
        || draw.viewport.TopLeftX != 0.0
        || draw.viewport.TopLeftY != 0.0
    {
        return false;
    }
    stats()[side] += 1;
    if side == 0 && stats()[0] <= u64::from(work.ordinal) {
        return false;
    }
    if side == 1 {
        let same_family = work.family.as_ref().is_some_and(|f| f.view.identity == family.view.identity);
        // SAFETY: two plain records.
        let same = unsafe { crate::paired_geometry_checks::cvr_npg_same_geometry(&work.eyes[0].draw, draw) };
        if !work.eyes[0].captured || work.frame != tag.frame || !same_family || !same {
            return false;
        }
    }
    if side == 0 && work.frame != 0 {
        return false;
    }
    let bound = BOUND.with(|b| *b.borrow());
    let context = current_context();
    if bound.epoch != EPOCH.load(Acquire)
        || bound.side != side as i32
        || (bound.stages & camera_stages) != camera_stages
        || bound.context != context
        || bound.context.list != list as u64
    {
        fail(Some(work), 3);
        return false;
    }
    let Some(targets) = capture_geometry_targets(draw).filter(|t| writable_targets(list, t)) else {
        fail(Some(work), 4);
        return false;
    };
    let mut lease = empty_lease();
    // SAFETY: the live list; local records; the token comes with its reference.
    let captured = unsafe { crate::native_recording::cvr_native_recording_capture(list, &current_recording_context(), &mut lease) };
    work.eyes[side].recording = OwnedLease(lease);
    if !captured {
        fail(Some(work), 100 + crate::native_recording::cvr_native_recording_last_failure());
        return false;
    }
    if side == 1 {
        let ok = work.eyes[0].submitted
            && work.main_queue.as_ref().is_some_and(|q| native_queue(&work.eyes[1].recording.0, q))
            && work.eyes[1].recording.0.registry == work.eyes[0].recording.0.registry;
        if !ok {
            fail(Some(work), 5);
            return false;
        }
    }
    if side == 0 {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: the live list.
        if unsafe { command.GetDevice(&mut device) }.is_err() || device.is_none() {
            fail(Some(work), 6);
            return false;
        }
        work.device = device;
        work.frame = tag.frame;
        let pipeline = draw.pipeline as *mut c_void;
        // SAFETY: the native draw's live pipeline (a reference taken).
        work.original_pipeline = unsafe { ID3D12PipelineState::from_raw_borrowed(&pipeline) }.cloned();
        let stats = stats();
        stats[8] = u64::from(draw.viewport.Width as u32);
        stats[9] = u64::from(draw.viewport.Height as u32);
        stats[12] = family.view.id;
    }
    // The first eye's family held by the work (the second eye's is the same family, checked above).
    let family = if side == 0 {
        work.family = Some(family);
        None
    } else {
        Some(family)
    };
    let device = work.device.clone().expect("the first eye's device");
    {
        let eye = &mut work.eyes[side];
        eye.draw = *draw;
        eye.camera = bound;
        // SAFETY: the live device.
        let fence = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
        let ok = match fence {
            Ok(f) => {
                eye.fence = Some(f);
                eye.before.create(&device, &targets).is_ok() && eye.after.create(&device, &targets).is_ok() && eye.before.capture(list, &targets)
            }
            Err(_) => false,
        };
        if !ok {
            fail(Some(work), 7);
            return false;
        }
    }
    native_draw(&command, draw);
    let handled = true;
    work.eyes[side].native_executed = true;
    stats()[10 + side] = draw.sequence;
    if !work.eyes[side].after.capture(list, &targets) {
        fail(Some(work), 8);
        return handled;
    }
    work.eyes[side].captured = true;
    stats()[2 + side] = 1;
    if side == 0 {
        return handled;
    }
    let mut restore = Restore::new(&command, draw);
    if !restore.valid {
        fail(Some(work), 9);
        return handled;
    }
    pair_draw(work, &command, draw, family.as_ref().expect("the second eye's family"), camera_stages, &mut restore);
    restore.finish(work, &command, draw);
    handled
}
fn current_recording_context() -> crate::native_recording::Context {
    let c = current_context();
    crate::native_recording::Context { context: c.context, state: c.state, list: c.list }
}
/// The second eye's pair: the private chain prepared from both eyes' checkpoints, the paired camera's stages begun,
/// the view-instanced draw, both proofs recorded.
fn pair_draw(work: &mut Work, list: &ID3D12GraphicsCommandList, draw: &DrawRecord, family: &Family, camera_stages: u32, restore: &mut Restore) {
    let device = work.device.clone().expect("the first eye's device");
    let mut storage = null_mut();
    // SAFETY: the live device; a writable output.
    let created = unsafe {
        crate::geometry_chain::cvr_geometry_chain_create(device.as_raw(), draw.viewport.Width as u32, draw.viewport.Height as u32, &mut storage)
    };
    if created.is_ok() {
        work.chain = Some(Chain(storage));
    }
    let empty = Targets::default();
    let prepare = |eye: u32, targets: &Targets| {
        let raw = targets.raw();
        // SAFETY: the chain, the live list, the checkpoint's targets (the chain's own mirror of the same record).
        unsafe { crate::geometry_chain::cvr_geometry_chain_prepare_eye(storage, list.as_raw(), eye, (&raw as *const GeometryTargetsRaw).cast()) }.is_ok()
    };
    if created.is_err()
        || !prepare(1, work.eyes[0].before.targets().unwrap_or(&empty))
        || !prepare(0, work.eyes[1].before.targets().unwrap_or(&empty))
    {
        fail(Some(work), 10);
        return;
    }
    let mut camera = Upload::default();
    restore.camera_touched = true;
    let (current, main, peer) = (work.eyes[1].camera.bytes, work.eyes[0].camera.bytes, work.eyes[1].camera.bytes);
    // SAFETY: the cameras, the heap slot, writable outputs.
    let begun = unsafe {
        crate::camera_upload::cvr_ncu_begin_stages(
            true,
            base(),
            work.eyes[1].camera.handle,
            &current,
            &main,
            &peer,
            &mut work.camera_heap,
            &mut camera,
            camera_stages,
            &mut restore.scope,
        )
    };
    copy_diagnostics();
    if !begun {
        fail(Some(work), 11);
        return;
    }
    let objects = [family.view.native_camera_pipeline, family.view.source_root, work.camera_heap];
    // SAFETY: the live list and objects.
    if camera.bytes != 2048
        || camera.gpu_address == 0
        || work.camera_heap.is_null()
        || !unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list.as_raw(), objects.as_ptr(), objects.len()) }
    {
        fail(Some(work), 12);
        return;
    }
    {
        let _internal = cvr_host::probe::Internal::new();
        let pipeline = family.view.native_camera_pipeline;
        // SAFETY: the live list, the family's pipeline.
        unsafe {
            list.SetPipelineState(ID3D12PipelineState::from_raw_borrowed(&pipeline));
            if let Some(views) = &restore.views {
                views.SetViewInstanceMask(3);
            }
        }
        if !crate::geometry_chain::cvr_geometry_chain_bind(storage, list.as_raw(), 3, 3) {
            fail(Some(work), 13);
            return;
        }
        // SAFETY: the live list.
        unsafe { list.DrawIndexedInstanced(draw.indices, draw.instances, draw.first_index, draw.base_vertex, draw.first_instance) };
    }
    stats()[4] = 1;
    if !record_proof(work, list, 0) || !record_proof(work, list, 1) {
        fail(Some(work), 14);
        return;
    }
    state().store(2, SeqCst);
}

/// BeforeSubmit: a submission of either eye's native recording validated (once, on the main queue for the second);
/// true when the submission must be serialized.
///
/// # Safety
/// `lists` holds `count_of` lists.
pub unsafe fn before_submit(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) -> bool {
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return false;
    }
    let mut held = lock();
    let Some(work) = held.0.as_deref_mut() else { return false };
    if work.complete {
        return false;
    }
    // SAFETY: the caller's lists and live queue.
    let (lists, queue_ref) = unsafe { (core::slice::from_raw_parts(lists, count_of as usize), ID3D12CommandQueue::from_raw_borrowed(&queue)) };
    let Some(queue_ref) = queue_ref else { return false };
    let mut serialize = false;
    for side in 0..2 {
        let eye = &work.eyes[side];
        if !eye.native_executed || eye.submitted {
            continue;
        }
        let occurrences = lists.iter().filter(|&&l| l as u64 == eye.recording.0.list).count();
        if occurrences == 0 {
            continue;
        }
        serialize = true;
        work.eyes[side].pending = true;
        let eye = &work.eyes[side];
        // SAFETY: a valid lease.
        let valid = occurrences == 1
            && unsafe { crate::native_recording::cvr_native_recording_validate_submission(&eye.recording.0, queue) }
            && native_queue(&eye.recording.0, queue_ref)
            && (side == 0 || (work.eyes[0].submitted && raw_of(&work.main_queue) == queue));
        if !valid {
            fail(Some(work), 200 + crate::native_recording::cvr_native_recording_last_failure());
            continue;
        }
        if side == 0 {
            work.main_queue = Some(queue_ref.clone());
        }
        stats()[7] |= 1u64 << side;
    }
    if serialize {
        stats()[13] += 1;
    }
    serialize
}
/// Submitted: each pending eye's submission signals its fence.
///
/// # Safety
/// `lists` holds `count_of` lists.
pub unsafe fn submitted(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return;
    }
    let mut held = lock();
    let Some(work) = held.0.as_deref_mut() else { return };
    if work.complete {
        return;
    }
    // SAFETY: the caller's lists and live queue.
    let (lists, queue_ref) = unsafe { (core::slice::from_raw_parts(lists, count_of as usize), ID3D12CommandQueue::from_raw_borrowed(&queue)) };
    for side in 0..2 {
        let eye = &work.eyes[side];
        if !eye.pending || eye.submitted || !lists.iter().any(|&l| l as u64 == eye.recording.0.list) {
            continue;
        }
        // SAFETY: the live queue and fence.
        let signalled = match (queue_ref, eye.fence.as_ref()) {
            (Some(q), Some(f)) => unsafe { q.Signal(f, 1) }.is_ok(),
            _ => false,
        };
        if !signalled {
            fail(Some(work), 16);
        }
        let eye = &mut work.eyes[side];
        eye.submitted = true;
        eye.pending = false;
        stats()[5 + side] = 1;
    }
}

// ---- the C ABI the C++ API forwards to (src/Render/NativePairedGeometry.cpp: the native probe's host)

#[no_mangle]
pub extern "C-unwind" fn cvr_pg2_active() -> bool {
    active()
}
/// # Safety
/// `record` is a valid draw record.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pg2_draw(list: *mut c_void, record: *const DrawRecord) -> bool {
    // SAFETY: as documented.
    unsafe { record.as_ref() }.is_some_and(|r| draw(list, r))
}
#[no_mangle]
pub extern "C-unwind" fn cvr_pg2_frame_boundary() {
    frame_boundary();
}
