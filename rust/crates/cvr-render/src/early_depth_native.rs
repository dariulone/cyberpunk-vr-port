//! The native early depth's orchestration (formerly src/Render/NativeEarlyDepth.cpp): the MAIN eye's admitted draws
//! recorded once more into the pass's private depth targets and replayed for the peer eye, every input proved at the
//! actual native draw. Begin opens a group at the vrcam node's third stage (the request or a run's frame, the work's
//! settings and hints, the eyes' packet membership resolved twice from the live render span and the peer storage, the
//! native camera bound); Draw admits one native draw (the family bound to the list, its instances, material, skinning
//! and alpha inputs, the membership plan and its extras, the constants or the native camera) and records it through the
//! pass object (early_depth_object.rs); the peer eye's draws are matched by geometry, material, camera, skin and alpha
//! and their instance coverage planned and committed; End checks every instance was covered, commits or aborts the
//! native depth and reads the comparison back; the submissions revalidate the descriptors, constants, recordings and
//! instance bytes; Poll seals the material captures and publishes the result. A run (the request's frame count)
//! reuses up to three works whose results complete asynchronously into the run's rows.
//!
//! What stays C++ (the .cpp's Rust branch, attached through `Host`): the stereo families (`families::Bound`, the
//! native depth status of a pipeline, a family's fields and paired-root plan lent as a view), the native probe's
//! instance lease and the binding banks over the paired-root plans (StereoBindingBank, whose storage is
//! binding_bank.rs's; the pass's constant arenas stay those C++ banks, as EarlyDepthPass.cpp's reads free them).

use core::cell::{Cell, RefCell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{addr_of, addr_of_mut, null_mut};
use core::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_UNEXPECTED, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use cvr_gpu::scene_state::{cvr_scene_capture_targets, cvr_scene_snapshot, Bindings, DrawTargets};

use crate::buffer_read::Lease as BufferLease;
use crate::camera_binding::{BindingScope, Context};
use crate::camera_upload::{self, Upload};
use crate::declared_moved_exports::{
    CyberpunkVR_EarlyDepthRunBindingChecks, CyberpunkVR_NativeStereoLiveFailure, CyberpunkVR_NativeStereoProbeRequest,
    CyberpunkVR_NativeStereoProbeState,
};
use crate::descriptor_audit::{self as audit, CbvBinding, DescriptorStamp};
use crate::descriptor_store::Value;
use crate::early_depth_checks::ResourceUse;
use crate::early_depth_compare::Comparison;
use crate::early_depth_exports::*;
use crate::early_depth_object::{self as edo, Bank};
use crate::early_depth_pass::Admission;
use crate::eye_membership::{self, EyeMembership};
use crate::geometry_commands::DrawRecord;
use crate::geometry_leases::ReadState;
use crate::instance_coverage::{self, InstanceRun, Source};
use crate::material_observe::{self, Status};
use crate::material_read::{self, Lease as MaterialLease};
use crate::material_readback::{ReadbackState, PERMIT_ENABLE, PERMIT_NONE, REJECTED, SEALED};
use crate::native_recording::Lease as RecordingLease;
use crate::native_state_exports::{
    CyberpunkVR_ConstantCaptureEnable, CyberpunkVR_PreparationPairData, CyberpunkVR_PreparationPairMerged,
    CyberpunkVR_PreparationPairState, CyberpunkVR_PreparationRunFailure, CyberpunkVR_PreparationRunRequest,
    CyberpunkVR_PreparationRunState, CyberpunkVR_PreparationRunStop, CyberpunkVR_PreparationState,
    CyberpunkVR_StereoFamilyNativeCameras,
};
use crate::packet_inputs::{self, Blob, Inputs, MappedInstances, Packet, MAX_INPUT_BLOB_BYTES};
use crate::paired_root::{Binding, Location};
use crate::skinning_read_plan as skinning;
use crate::texture_lease::Lease as TextureLease;
use crate::texture_read::MipFootprint;
use crate::used_moved_exports_rest::{
    CyberpunkVR_EarlyDepthAlphaOptimized, CyberpunkVR_EarlyDepthAlphaProofs, CyberpunkVR_EarlyDepthAlphaReference,
    CyberpunkVR_EarlyDepthAlphaStats, CyberpunkVR_EarlyDepthMaterialDescriptors, CyberpunkVR_EarlyDepthMaterialHintCount,
    CyberpunkVR_EarlyDepthMaterialHints, CyberpunkVR_EarlyDepthSkinnedAlpha, CyberpunkVR_EarlyDepthSkinnedAlphaStats,
};

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool;
    fn cvr_iab_read_state(list: *mut c_void, source: *const BufferLease, out: *mut ReadState);
    fn cvr_buffer_read_last_failure_details(out: *mut [u64; 4]);
    fn cvr_native_recording_compatible() -> bool;
    fn cvr_native_recording_last_failure() -> u32;
    fn cvr_native_recording_last_details(out: *mut [u64; 8]);
    fn cvr_native_recording_capture(list: *mut c_void, context: *const Context, out: *mut RecordingLease) -> bool;
    fn cvr_native_recording_validate_submission(lease: *const RecordingLease, queue: *mut c_void) -> bool;
    fn cvr_tr_failure() -> u32;
    fn cvr_tr_acquire(list: *mut c_void, resource: *mut c_void, out: *mut TextureLease) -> bool;
    fn cvr_tr_validate(source: *const TextureLease, queue: *mut c_void) -> bool;
    fn cvr_tr_view_footprints(
        device: *mut c_void,
        source: *const TextureLease,
        view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
        offset: u64,
        out: *mut MipFootprint,
        capacity: usize,
        count: *mut usize,
    ) -> bool;
    fn cvr_tr_alias_affects_resources(watched: *mut c_void, before: *mut c_void, after: *mut c_void) -> bool;
    fn cvr_upload_read(address: u64, output: *mut c_void, bytes: usize, fixed_only: bool) -> bool;
    fn cvr_gp_live(renderer: u64, packets: usize, pool: *mut u64, uploaded: *mut MappedInstances) -> bool;
    fn cvr_gp_current_geometry(renderer: u64, instances: u32, stride: u32, output: *mut [u8; 64]) -> bool;
}

// ---- the limits (include/Render/EarlyDepthLimits.hpp)

const ALL_DRAWS: u32 = u32::MAX;
const MAX_DRAWS: u32 = 1024;
const MAX_INSTANCE_ROWS: usize = MAX_DRAWS as usize * 4;
const MAX_INSTANCE_BYTES: usize = 8 * 1024 * 1024;
/// material_binding::LogicalBytes.
const LOGICAL_BYTES: usize = 292;
const NODE: u32 = 0x33DDA4;
const CAMERA_BYTES: usize = 848;

// ---- the exports this module defines (formerly the .cpp's)

/// The last comparison's result (stereo::DepthComparisonResult).
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthComparison: Comparison = Comparison {
    width: 0,
    height: 0,
    draws: 0,
    complete: 0,
    depth_different: [0; 2],
    stencil_different: [0; 2],
    written: [0; 2],
    invalid_depth: [0; 2],
    max_depth_error: [0.0; 2],
};
/// The material capture's row size (29 u64).
#[no_mangle]
pub static CyberpunkVR_EarlyDepthMaterialRowBytes: u32 = 29 * 8;

/// The exports as references (the early depth's lock or a single writer: the .cpp's discipline).
macro_rules! ex {
    ($name:ident) => {
        // SAFETY: an export of this port, written as the .cpp wrote it.
        unsafe { &mut *addr_of_mut!($name) }
    };
}

// ---- the C++ host

/// A family's fields (families::Family and its pass contract), valid while its reference is held.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FamilyView {
    pub asset: u32,
    pub alpha_depth_contract: u32,
    pub skinning_read_contract: u32,
    pub native_camera_stages: u32,
    pub render_targets: u32,
    pub depth_format: i32,
    pub depth_func: i32,
    pub depth_write: i32,
    pub samples_count: u32,
    pub samples_quality: u32,
    pub pixel_shader: bool,
    pub paired_root: bool,
    pub native_camera_pipeline: bool,
    pub instance_ordinal_independent: bool,
    pub fixed_raster: bool,
    pub depth_known: bool,
    pub depth_enable: bool,
    pub pipeline: bool,
    pub uses: *const ResourceUse,
    pub use_count: usize,
    pub bindings: *const Location,
    pub binding_count: usize,
    pub source_root: *mut c_void,
    pub pipeline_state: *mut c_void,
    pub camera_pipeline: *mut c_void,
    pub root: *mut c_void,
    /// The paired root's plan (when there is a paired root): its serialized root and its parameters.
    pub serialized: *const u8,
    pub serialized_len: usize,
    pub parameters: [u32; 2],
    pub descriptors: [u32; 2],
    pub original_parameters: u32,
    pub root_dwords: u32,
}
const _: () = assert!(core::mem::size_of::<FamilyView>() == 152);
/// One BankPair: the binding and its two eyes' constants.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PairSpec {
    pub binding: Binding,
    pub eyes: [*const u8; 2],
    pub bytes: [usize; 2],
}
const _: () = assert!(core::mem::size_of::<PairSpec>() == 48);
extern "C" {
    /// native_probe::InstanceLease (stereo_probe_state.rs): geometry::BufferLease's layout, its resource an owned reference.
    fn cvr_nsp2_instance_lease(list: *mut c_void, out: *mut BufferLease);
}
/// A bound family (families_native.rs'): its reference and view.
struct Family {
    _held: std::sync::Arc<crate::families_native::Family>,
    view: FamilyView,
}
impl Family {
    fn bound(list: *mut c_void) -> Option<Family> {
        let f = crate::families_native::bound(list)?;
        let (pass, paired) = (f.pass, &f.paired_root);
        let plan = &paired.plan;
        let view = FamilyView {
            asset: f.asset,
            alpha_depth_contract: f.alpha_depth_contract,
            skinning_read_contract: f.skinning_read_contract,
            native_camera_stages: f.native_camera_stages,
            render_targets: pass.targets.NumRenderTargets,
            depth_format: pass.depth.0,
            depth_func: pass.depth_state.DepthFunc.0,
            depth_write: pass.depth_state.DepthWriteMask.0,
            samples_count: pass.samples.Count,
            samples_quality: pass.samples.Quality,
            pixel_shader: pass.pixel_shader,
            paired_root: true,
            native_camera_pipeline: f.native_camera_pipeline.is_some(),
            instance_ordinal_independent: f.instance_ordinal_independent,
            fixed_raster: pass.fixed_raster_state,
            depth_known: pass.depth_state_known,
            depth_enable: pass.depth_state.DepthEnable.as_bool(),
            pipeline: true,
            // The same 24-byte families::ResourceUse.
            uses: f.uses.as_ptr().cast(),
            use_count: f.uses.len(),
            bindings: plan.bindings.as_ptr(),
            binding_count: plan.bindings.len(),
            source_root: f.source_root.as_raw(),
            pipeline_state: f.pipeline.as_raw(),
            camera_pipeline: f.native_camera_pipeline.as_ref().map_or(null_mut(), |p| p.as_raw()),
            root: paired.root.as_raw(),
            serialized: plan.serialized.as_ptr(),
            serialized_len: plan.serialized.len(),
            parameters: plan.parameters,
            descriptors: plan.descriptors,
            original_parameters: plan.original_parameters,
            root_dwords: plan.root_dwords,
        };
        Some(Family { _held: f, view })
    }
    /// The paired root's bindings (none without one).
    fn bindings(&self) -> &[Location] {
        if !self.view.paired_root {
            return &[];
        }
        slice(self.view.bindings, self.view.binding_count)
    }
}
/// A per-draw bank (the .cpp's StereoBindingBank), freed with the draw.
struct DrawBank(*mut c_void);
impl Drop for DrawBank {
    fn drop(&mut self) {
        bank_release(self.0);
    }
}

// ---- the binding banks (formerly the host's StereoBindingBank calls): the per-draw banks and the arenas created,
// counted, captured, bound and appended through binding_bank.rs directly over the family's paired root plan; the
// command resources' keep and has stay the .cpp's (the host's), as StereoBindingBank's did.

fn plan_view(f: &FamilyView) -> crate::binding_bank::PlanView {
    crate::binding_bank::PlanView {
        serialized: f.serialized,
        serialized_len: f.serialized_len,
        bindings: f.bindings,
        binding_count: f.binding_count,
        parameters: f.parameters,
        descriptors: f.descriptors,
        original_parameters: f.original_parameters,
        root_dwords: f.root_dwords,
    }
}
/// BankPairs: each binding with its two eyes' constants (the rest of the values default).
fn bank_pairs(pairs: &[PairSpec]) -> Vec<crate::binding_bank::Pair> {
    let value = |constants: *const u8, constants_len: usize| {
        // SAFETY: every field is valid zeroed (null sources and owners, empty views, COMMON).
        let mut v: crate::binding_bank::Value = unsafe { core::mem::zeroed() };
        v.constants = constants;
        v.constants_len = constants_len;
        v
    };
    pairs.iter().map(|p| crate::binding_bank::Pair { binding: p.binding, eyes: [value(p.eyes[0], p.bytes[0]), value(p.eyes[1], p.bytes[1])] }).collect()
}
/// gpu::KeepCommandResources(list, {bank}): the banks' retention (command_resources.rs).
unsafe extern "C-unwind" fn bank_keep(list: *mut c_void, storage: *mut c_void) -> bool {
    // SAFETY: the live list and bank.
    unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, &storage, 1) }
}
/// gpu::HasCommandResource(list, bank).
unsafe extern "C-unwind" fn bank_has(list: *mut c_void, storage: *mut c_void) -> bool {
    // SAFETY: the list as the tracker's key; the live bank.
    unsafe { cvr_gpu::command_resources::cvr_gpu_has_command_resource(list, storage) }
}
/// A bank's storage released (StereoBindingBank's destruction).
pub(crate) fn bank_release(storage: *mut c_void) {
    if !storage.is_null() {
        // SAFETY: the reference the bank's holder owns.
        drop(unsafe { windows::core::IUnknown::from_raw(storage) });
    }
}
/// StereoBindingBank::Create over the family's paired root: the draw's bank.
fn bank_create(device: *mut c_void, f: &FamilyView, pairs: &[PairSpec], out: &mut *mut c_void) -> HRESULT {
    *out = null_mut();
    if !f.paired_root || device.is_null() {
        return E_INVALIDARG;
    }
    let plan = plan_view(f);
    let lent = bank_pairs(pairs);
    let draw = crate::binding_bank::DrawInput {
        root: f.root,
        plan: &plan,
        pairs: lent.as_ptr(),
        pair_count: lent.len(),
        shared: core::ptr::null(),
        shared_count: 0,
        source_root: null_mut(),
    };
    // SAFETY: the live device; the lent draw.
    unsafe { crate::binding_bank::cvr_bank_create_pass(device, &draw, 1, out) }
}
/// StereoBindingBank::CreateArena over the family's paired root (MAX_DRAWS draws, 4 KiB each).
fn arena_create(device: *mut c_void, f: &FamilyView, out: &mut *mut c_void) -> HRESULT {
    *out = null_mut();
    if !f.paired_root || device.is_null() {
        return E_INVALIDARG;
    }
    let plan = plan_view(f);
    // SAFETY: the live device and paired root; the lent plan.
    unsafe { crate::binding_bank::cvr_bank_create_arena(device, f.root, &plan, MAX_DRAWS, u64::from(MAX_DRAWS) * 4096, out) }
}
/// StereoBindingBank::AppendConstants: the pairs' constants appended to the arena.
fn arena_append(arena: *mut c_void, f: &FamilyView, pairs: &[PairSpec], index: &mut u32) -> HRESULT {
    if !f.paired_root || arena.is_null() {
        return E_INVALIDARG;
    }
    let plan = plan_view(f);
    let lent = bank_pairs(pairs);
    // SAFETY: the live arena; the lent plan and pairs.
    unsafe { crate::binding_bank::cvr_bank_append_constants(arena, &plan, lent.as_ptr(), lent.len(), index) }
}
/// DrawCount and SnapshotCount (a null bank: zero).
fn bank_counts(bank: *mut c_void, draws: &mut u32, snapshots: &mut u32) {
    (*draws, *snapshots) = (0, 0);
    if !bank.is_null() {
        // SAFETY: every field is valid zeroed; the live bank.
        let mut counts: crate::binding_bank::Counts = unsafe { core::mem::zeroed() };
        unsafe { crate::binding_bank::cvr_bank_counts(bank, &mut counts) };
        (*draws, *snapshots) = (counts.draws, counts.snapshots);
    }
}
/// CapturedFor(list) || (CaptureEye(list, 0) && CaptureEye(list, 1)).
unsafe extern "C-unwind" fn bank_capture(bank: *mut c_void, list: *mut c_void) -> bool {
    if bank.is_null() || list.is_null() {
        return false;
    }
    // SAFETY: the live bank and list.
    unsafe {
        crate::binding_bank::cvr_bank_captured_for(bank, list, bank_has)
            || (crate::binding_bank::cvr_bank_capture_eye(bank, list, 0, bank_keep, bank_has)
                && crate::binding_bank::cvr_bank_capture_eye(bank, list, 1, bank_keep, bank_has))
    }
}
/// Bind(list, index).
unsafe extern "C-unwind" fn bank_bind(bank: *mut c_void, list: *mut c_void, index: u32) -> bool {
    // SAFETY: the live bank and list.
    !bank.is_null() && !list.is_null() && unsafe { crate::binding_bank::cvr_bank_bind(bank, list, u32::MAX, index, bank_has) }
}

// ---- small helpers

fn slice<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: the caller's array of `n`.
        unsafe { core::slice::from_raw_parts(p, n) }
    }
}
fn zeroed<T>() -> T {
    // SAFETY: plain mirrors whose all-zero value is their C++ default.
    unsafe { core::mem::zeroed() }
}
fn read(address: u64, out: &mut [u8]) -> bool {
    crate::instance_layout::read(address, out)
}
unsafe extern "C-unwind" fn read_raw(address: u64, out: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the snapshot's buffer of `bytes`.
    read(address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
/// constants::ReadUpload (`fixed` ReadFixedUpload).
fn read_upload(address: u64, out: *mut c_void, bytes: usize, fixed: bool) -> bool {
    // SAFETY: the caller's buffer of `bytes`.
    unsafe { cvr_upload_read(address, out, bytes, fixed) }
}
fn read_fixed<T>(address: u64, out: &mut T, bytes: usize) -> bool {
    debug_assert!(bytes <= core::mem::size_of::<T>());
    read_upload(address, (out as *mut T).cast(), bytes, true)
}
fn list_of(raw: *mut c_void) -> Option<ID3D12GraphicsCommandList> {
    // SAFETY: a live list of the caller's or null.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }.cloned()
}
fn device_of(list: *mut c_void) -> Option<ID3D12Device> {
    let list = list_of(list)?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list.
    unsafe { list.GetDevice(&mut device) }.ok()?;
    device
}
fn raw<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), |v| v.as_raw())
}
fn add_ref(p: *mut c_void) {
    if !p.is_null() {
        // SAFETY: a live COM object.
        unsafe { ((*(*(p as *mut *mut windows::core::IUnknown_Vtbl))).AddRef)(p) };
    }
}
fn release(p: *mut c_void) {
    if !p.is_null() {
        // SAFETY: a reference the caller owns.
        unsafe { ((*(*(p as *mut *mut windows::core::IUnknown_Vtbl))).Release)(p) };
    }
}
fn image_base() -> u64 {
    cvr_engine::code::image_base() as u64
}
fn sfence() {
    // SAFETY: a store fence.
    unsafe { core::arch::x86_64::_mm_sfence() };
}
fn write_permit(mapped: *mut c_void, value: u64) {
    if !mapped.is_null() {
        // SAFETY: the permit's mapped 8 bytes.
        unsafe { core::ptr::write_volatile(mapped.cast::<u64>(), value) };
    }
}
fn data_ptr<T>(v: &Vec<T>) -> u64 {
    if v.capacity() == 0 {
        0
    } else {
        v.as_ptr() as u64
    }
}
fn qpc() -> i64 {
    let mut now = 0i64;
    // SAFETY: a local output.
    let _ = unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    now
}
fn internal() -> bool {
    cvr_host::probe::internal()
}
fn healthy() -> bool {
    audit::healthy()
}
fn find_binding(root: u64, kind: D3D12_DESCRIPTOR_RANGE_TYPE, space: u32, register: u32, visibility: D3D12_SHADER_VISIBILITY) -> Option<(u32, u32)> {
    audit::find_binding(root, kind.0 as u32, space, register, visibility.0 as u32)
}
fn resolve_cbv(list: *mut c_void, root: u64, parameter: u32, offset: u32) -> Option<CbvBinding> {
    audit::resolve_cbv(list as u64, root, parameter, offset)
}
/// descriptors::CaptureDescriptor into `out` (zeroed first).
fn capture_descriptor(list: *mut c_void, root: u64, parameter: u32, offset: u32, out: &mut DescriptorStamp) -> bool {
    *out = zeroed();
    match audit::capture_descriptor(list as u64, root, parameter, offset) {
        Some(stamp) => {
            *out = stamp;
            true
        }
        None => false,
    }
}
fn matches(stamp: &DescriptorStamp) -> bool {
    audit::matches_descriptor(stamp)
}
fn preparation_active() -> bool {
    CyberpunkVR_PreparationState.load(Relaxed) == 1
}
fn run_active() -> bool {
    CyberpunkVR_PreparationRunState.load(Acquire) == 1
}
fn pairing() -> bool {
    preparation_active() || run_active()
}
fn framegen_enabled() -> bool {
    cvr_gpu::framegen_core::cvr_framegen_enabled()
}

// ---- owned leases (the .cpp's ComPtr members)

/// descriptors::SrvBinding.
#[derive(Clone)]
struct Srv {
    description: Value,
    resource: Option<ID3D12Resource>,
    view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    stamp: DescriptorStamp,
}
impl Default for Srv {
    fn default() -> Self {
        Srv { description: [0; 20], resource: None, view: zeroed(), stamp: zeroed() }
    }
}
/// descriptors::ResolveSrvAtDraw (`out` reset first).
fn resolve_srv(list: *mut c_void, root: u64, parameter: u32, offset: u32, out: &mut Srv) -> bool {
    *out = Srv::default();
    match audit::resolve_srv_at_draw(list as u64, root, parameter, offset) {
        Some((stamp, resource, view)) => {
            out.description = stamp.value;
            out.stamp = stamp;
            // SAFETY: the reference resolve_srv_at_draw handed out.
            out.resource = (!resource.is_null()).then(|| unsafe { ID3D12Resource::from_raw(resource) });
            out.view = view;
            true
        }
        None => false,
    }
}
/// descriptors::SamplerBinding.
#[derive(Clone, Copy)]
struct Sampler {
    stamp: DescriptorStamp,
    #[allow(dead_code)] // the resolved sampler's description, kept as the .cpp kept it
    view: D3D12_SAMPLER_DESC,
}
impl Default for Sampler {
    fn default() -> Self {
        Sampler { stamp: zeroed(), view: zeroed() }
    }
}
fn resolve_sampler(list: *mut c_void, root: u64, parameter: u32, offset: u32, out: &mut Sampler) -> bool {
    *out = Sampler::default();
    match audit::resolve_sampler_at_draw(list as u64, root, parameter, offset) {
        Some((stamp, view)) => {
            *out = Sampler { stamp, view };
            true
        }
        None => false,
    }
}
/// texture_read::Lease, its resource held.
struct Texture(TextureLease);
impl Default for Texture {
    fn default() -> Self {
        Texture(zeroed())
    }
}
impl Clone for Texture {
    fn clone(&self) -> Self {
        add_ref(self.0.resource);
        // SAFETY: a plain copy whose reference was just taken.
        Texture(unsafe { core::ptr::read(&self.0) })
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        release(self.0.resource);
    }
}
/// texture_read::Acquire.
fn texture_acquire(list: *mut c_void, resource: *mut c_void, out: &mut Texture) -> bool {
    *out = Texture::default();
    let mut lease: TextureLease = zeroed();
    // SAFETY: a live list and resource; `lease` is writable.
    if !unsafe { cvr_tr_acquire(list, resource, &mut lease) } {
        return false;
    }
    add_ref(resource);
    lease.resource = resource;
    out.0 = lease;
    true
}
fn texture_failure() -> u32 {
    // SAFETY: a plain query.
    unsafe { cvr_tr_failure() }
}
/// material_binding::Lease, its source buffer held.
struct Material(MaterialLease);
impl Default for Material {
    fn default() -> Self {
        Material(zeroed())
    }
}
impl Clone for Material {
    fn clone(&self) -> Self {
        add_ref(self.0.source.resource);
        // SAFETY: as Texture's.
        Material(unsafe { core::ptr::read(&self.0) })
    }
}
impl Drop for Material {
    fn drop(&mut self) {
        release(self.0.source.resource);
    }
}
/// geometry::BufferLease, its resource held.
struct Buffer(BufferLease);
impl Default for Buffer {
    fn default() -> Self {
        Buffer(zeroed())
    }
}
impl Clone for Buffer {
    fn clone(&self) -> Self {
        add_ref(self.0.resource);
        Buffer(self.0)
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        release(self.0.resource);
    }
}
/// native_recording::Lease, its token held.
struct Recording(RecordingLease);
impl Default for Recording {
    fn default() -> Self {
        Recording(RecordingLease { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        release(self.0.token);
    }
}

/// The pass (EarlyDepthPass over early_depth_object.rs; null until created).
#[derive(Default)]
struct Pass(Option<windows::core::IUnknown>);
impl Pass {
    fn p(&self) -> *mut c_void {
        raw(&self.0)
    }
    fn create(&mut self, device: &ID3D12Device, width: u32, height: u32, mode: u32) -> HRESULT {
        if self.0.is_some() {
            return windows::Win32::Foundation::E_INVALIDARG;
        }
        let mut created = null_mut();
        // SAFETY: a live device; `created` is writable.
        let hr = unsafe { edo::cvr_edo_create(device.as_raw(), width, height, mode, &mut created) };
        if hr.is_ok() && !created.is_null() {
            // SAFETY: the object's one reference.
            self.0 = Some(unsafe { windows::core::IUnknown::from_raw(created) });
        }
        hr
    }
    fn counts<const N: usize>(&self, which: u32) -> [u64; N] {
        let mut out = [0u64; N];
        if self.0.is_some() {
            // SAFETY: `out` holds the counter's values.
            unsafe { edo::cvr_edo_counts(self.p(), which, out.as_mut_ptr()) };
        }
        out
    }
    fn instance_proof_counts(&self) -> [u64; 2] {
        self.counts(0)
    }
    fn alpha_proof_counts(&self) -> [u64; 6] {
        self.counts(1)
    }
    fn shader_proof_counts(&self) -> [u64; 4] {
        self.counts(2)
    }
    fn optimized_counts(&self) -> [u64; 8] {
        self.counts(3)
    }
    fn native_differences(&self) -> [u64; 4] {
        self.counts(4)
    }
    fn gpu_times(&self) -> [u64; 6] {
        self.counts(5)
    }
    fn native_counts(&self) -> [u32; 8] {
        let mut out = [0u32; 8];
        if self.0.is_some() {
            // SAFETY: `out` holds eight values.
            unsafe { edo::cvr_edo_native_counts(self.p(), out.as_mut_ptr()) };
        }
        out
    }
    fn capture_instances(&self, list: *mut c_void, source: *mut c_void, offset: u64, bytes: u32, state: i32, combined: bool) -> bool {
        // SAFETY: the pass's live objects.
        self.0.is_some() && unsafe { edo::cvr_edo_capture_instances(self.p(), list, source, offset, bytes, state, combined) }
    }
    fn release_instance_read(&self, list: *mut c_void) {
        if self.0.is_some() {
            // SAFETY: as above.
            unsafe { edo::cvr_edo_release_instance_read(self.p(), list) };
        }
    }
    fn instance_source(&self) -> *mut c_void {
        if self.0.is_some() {
            edo::cvr_edo_instance_source(self.p())
        } else {
            null_mut()
        }
    }
    fn shader_source(&self, resource: *mut c_void) -> bool {
        self.0.is_some() && edo::cvr_edo_shader_source(self.p(), resource)
    }
    fn reject_shader_snapshots(&self) {
        if self.0.is_some() {
            edo::cvr_edo_reject_shader_snapshots(self.p());
        }
    }
    fn capture_skinning(&self, list: *mut c_void, bones: *mut c_void, ranges: &[skinning::BoneRange], texture: &Texture, sequence: u32) -> bool {
        self.0.is_some()
            // SAFETY: the ranges are [offset, bytes] pairs; the lease lives through the call.
            && unsafe { edo::cvr_edo_capture_skinning(self.p(), list, bones, ranges.as_ptr().cast(), ranges.len(), &texture.0, sequence) }
    }
    fn capture_alpha(&self, list: *mut c_void, material: &Material, texture: &Texture, mips: &[u32], expected: u32, sequence: u32) -> bool {
        self.0.is_some()
            // SAFETY: as above.
            && unsafe {
                edo::cvr_edo_capture_alpha(self.p(), list, (&material.0 as *const MaterialLease).cast(), &texture.0, mips.as_ptr(), mips.len(), expected, sequence)
            }
    }
    fn start_native(&self, list: *mut c_void, d: &DrawRecord, target: *mut c_void, instance_source: *mut c_void, state: i32) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_start_native(self.p(), list, d, target, instance_source, state) }
    }
    fn observe_native(&self, list: *mut c_void, d: &DrawRecord) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_observe_native(self.p(), list, d) }
    }
    fn original(&self, list: *mut c_void, d: &DrawRecord) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_original(self.p(), list, d) }
    }
    fn peer_remainder(&self, list: *mut c_void, d: &DrawRecord, remaining: &[InstanceRun]) -> bool {
        // SAFETY: as above (an InstanceRun is the object's Run).
        self.0.is_some() && unsafe { edo::cvr_edo_peer_remainder(self.p(), list, d, remaining.as_ptr().cast(), remaining.len()) }
    }
    fn finish_native(&self, list: *mut c_void, side: u32) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_finish_native(self.p(), list, side) }
    }
    fn commit_native(&self, allow: bool) -> bool {
        self.0.is_some() && edo::cvr_edo_commit_native(self.p(), allow)
    }
    fn abort_native(&self, list: *mut c_void, side: u32) {
        if self.0.is_some() {
            // SAFETY: as above.
            unsafe { edo::cvr_edo_abort_native(self.p(), list, side) };
        }
    }
    fn peer(&self, list: *mut c_void, d: &DrawRecord) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_peer(self.p(), list, d) }
    }
    fn readback(&self, list: *mut c_void) -> bool {
        // SAFETY: as above.
        self.0.is_some() && unsafe { edo::cvr_edo_readback(self.p(), list) }
    }
    fn read(&self, result: &mut Comparison) -> HRESULT {
        if self.0.is_none() {
            return E_UNEXPECTED;
        }
        // SAFETY: `result` is writable.
        unsafe { edo::cvr_edo_read(self.p(), result) }
    }
    fn restart(&self) -> HRESULT {
        if self.0.is_none() {
            return E_UNEXPECTED;
        }
        edo::cvr_edo_restart(self.p())
    }
    fn before_submit(&self, queue: *mut c_void, count: u32, lists: *const *mut c_void) {
        if self.0.is_some() && !queue.is_null() && !lists.is_null() {
            // SAFETY: the submission's lists.
            unsafe { edo::cvr_edo_before_submit(self.p(), queue, count, lists) };
        }
    }
    fn submitted(&self, queue: *mut c_void, count: u32, lists: *const *mut c_void) {
        if self.0.is_some() && !queue.is_null() && !lists.is_null() {
            // SAFETY: as above.
            unsafe { edo::cvr_edo_submitted(self.p(), queue, count, lists) };
        }
    }
    fn main_submitted(&self) -> bool {
        self.0.is_some() && edo::cvr_edo_main_submitted(self.p())
    }
    fn failed(&self) -> bool {
        self.0.is_some() && edo::cvr_edo_failed(self.p())
    }
    fn reset(&self, list: *mut c_void) {
        if self.0.is_some() && !list.is_null() {
            edo::cvr_edo_reset(self.p(), list);
        }
    }
    fn arena_count(&self) -> u32 {
        if self.0.is_some() {
            edo::cvr_edo_arena_count(self.p())
        } else {
            0
        }
    }
    /// Main: the admission from the family's pass contract and the bank's counts.
    #[allow(clippy::too_many_arguments)]
    fn main(
        &self,
        list: *mut c_void,
        d: &DrawRecord,
        family: &Family,
        bank: *mut c_void,
        masks: &[u8],
        peer_only: &[u8],
        bank_draw: u32,
        native_camera: bool,
        alpha_reference: bool,
        alpha_optimized: bool,
    ) -> bool {
        if self.0.is_none() {
            return false;
        }
        let f = &family.view;
        let (mut draws, mut snapshots) = (0u32, 0u32);
        // SAFETY: the host's bank (or null).
        bank_counts(bank, &mut draws, &mut snapshots);
        let admission = Admission {
            masks: masks.as_ptr(),
            mask_count: masks.len(),
            peer_only: peer_only.len(),
            bank_draw,
            bank_draws: draws,
            bank_snapshots: snapshots,
            alpha_depth_contract: f.alpha_depth_contract,
            skinning_read_contract: f.skinning_read_contract,
            native_camera_stages: f.native_camera_stages,
            render_targets: f.render_targets,
            depth_format: f.depth_format,
            depth_func: f.depth_func,
            depth_write: f.depth_write,
            samples_count: f.samples_count,
            samples_quality: f.samples_quality,
            source_root: f.source_root as u64,
            native_camera,
            alpha_reference,
            alpha_optimized,
            family_pipeline: f.pipeline,
            ordinal_independent: f.instance_ordinal_independent,
            native_camera_pipeline: f.native_camera_pipeline,
            pixel_shader: f.pixel_shader,
            fixed_raster: f.fixed_raster,
            depth_known: f.depth_known,
            depth_enable: f.depth_enable,
        };
        let banks = Bank { user: bank, capture: bank_capture, bind: bank_bind };
        // SAFETY: the pass's live objects, the family's and the bank's.
        unsafe {
            edo::cvr_edo_main(
                self.p(),
                list,
                d,
                &admission,
                masks.as_ptr(),
                masks.len(),
                peer_only.as_ptr(),
                peer_only.len(),
                bank_draw,
                native_camera,
                alpha_optimized,
                if native_camera { f.camera_pipeline } else { f.pipeline_state },
                f.camera_pipeline,
                f.source_root,
                &banks,
            )
        }
    }
    /// MainConstants: the paired root's constant arena (at most eight), the draw's constants appended to it.
    fn main_constants(&self, list: *mut c_void, d: &DrawRecord, family: &Family, pairs: &[PairSpec], masks: &[u8], extra: &[u8]) -> bool {
        if self.0.is_none() || !edo::cvr_edo_constants_open(self.p()) || !family.view.paired_root {
            return false;
        }
        let root = family.view.root;
        let mut arena = edo::cvr_edo_arena(self.p(), root);
        if arena.is_null() {
            if edo::cvr_edo_arena_count(self.p()) >= 8 {
                return false;
            }
            let mut made = null_mut();
            // SAFETY: the pass's device and the family's plan.
            if arena_create(edo::cvr_edo_device(self.p()), &family.view, &mut made).is_err() || made.is_null() {
                return false;
            }
            arena = made;
            edo::cvr_edo_adopt_arena(self.p(), root, arena);
        }
        let mut index = 0u32;
        // SAFETY: the pass's arena and the family's plan.
        if arena_append(arena, &family.view, pairs, &mut index).is_err() {
            return false;
        }
        self.main(list, d, family, arena, masks, extra, index, false, false, false)
    }
}

// ---- the work's records

#[derive(Clone, Copy)]
struct CameraData {
    frame: u32,
    handle: u32,
    side: i32,
    #[allow(dead_code)] // the trace row's view, kept as the .cpp kept it
    view: u64,
    own: [u8; CAMERA_BYTES],
    peer: [u8; CAMERA_BYTES],
    prepared: bool,
}
impl Default for CameraData {
    fn default() -> Self {
        CameraData { frame: 0, handle: 0, side: -1, view: 0, own: [0; CAMERA_BYTES], peer: [0; CAMERA_BYTES], prepared: false }
    }
}
/// early_depth::Group.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Group {
    pub id: u64,
    pub frame: u32,
    pub side: i32,
    pub restore_camera: u32,
    pub camera_scope: BindingScope,
}
const _: () = assert!(core::mem::size_of::<Group>() == 120 && core::mem::offset_of!(Group, camera_scope) == 24);
impl Default for Group {
    fn default() -> Self {
        Group { id: 0, frame: 0, side: -1, restore_camera: 0, camera_scope: BindingScope::default() }
    }
}
thread_local! {
    static PENDING_CAMERA: RefCell<CameraData> = RefCell::new(CameraData::default());
    static BOUND_CAMERA: RefCell<CameraData> = RefCell::new(CameraData::default());
    static GROUP: Cell<Group> = Cell::new(Group::default());
}
fn group() -> Group {
    GROUP.with(|g| g.get())
}
fn set_group(value: Group) {
    GROUP.with(|g| g.set(value));
}
fn bound_camera() -> CameraData {
    BOUND_CAMERA.with(|c| *c.borrow())
}

#[derive(Clone, Default)]
struct SkinInputs {
    alpha: bool,
    modifiers: Modifiers,
    bones: Srv,
    deformation: Srv,
    texture: Texture,
    descriptors: [Stamp; 4],
}
#[derive(Clone, Copy)]
struct Modifiers([u8; 448]);
impl Default for Modifiers {
    fn default() -> Self {
        Modifiers([0; 448])
    }
}
#[derive(Clone, Copy)]
struct Stamp(DescriptorStamp);
impl Default for Stamp {
    fn default() -> Self {
        Stamp(zeroed())
    }
}
#[derive(Clone)]
struct AlphaInputs {
    skinned: bool,
    material: Material,
    modifiers: [u32; 4],
    vertex_material: DescriptorStamp,
    modifier_binding: DescriptorStamp,
    dither: Srv,
    sampler: Sampler,
    image: Srv,
    texture: Texture,
    expected_index: u32,
    mips: Vec<u32>,
}
impl Default for AlphaInputs {
    fn default() -> Self {
        AlphaInputs {
            skinned: false,
            material: Material::default(),
            modifiers: [0; 4],
            vertex_material: zeroed(),
            modifier_binding: zeroed(),
            dither: Srv::default(),
            sampler: Sampler::default(),
            image: Srv::default(),
            texture: Texture::default(),
            expected_index: u32::MAX,
            mips: Vec::new(),
        }
    }
}
struct Item {
    draw: DrawRecord,
    bindings: Bindings,
    instances: Vec<u8>,
    material: [u8; 112],
    peer: [u8; CAMERA_BYTES],
    coverage: Vec<bool>,
    masks: Vec<u8>,
    matched: bool,
    gpu_identity: bool,
    skin: Option<SkinInputs>,
    alpha: Option<AlphaInputs>,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct InstanceRow {
    sequence: u64,
    side: u32,
    offset: u32,
    bytes: u32,
    stride: u32,
    material: [u8; 112],
}
const _: () = assert!(core::mem::size_of::<InstanceRow>() == 136);
struct BindingCheck {
    list: *mut c_void,
    side: u32,
    skin: SkinInputs,
    material: [u8; 112],
    instance_source: Buffer,
    instance_offset: u64,
    instances: Vec<u8>,
    submitted: bool,
}
struct AlphaCheck {
    list: *mut c_void,
    inputs: AlphaInputs,
    vertex_material: [u8; 112],
    submitted: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MaterialRow {
    sequence: u64,
    frame: u64,
    side: u64,
    family: u64,
    resource: u64,
    handle: u64,
    kind: u64,
    descriptor: u64,
    address: u64,
    source_offset: u64,
    heap_type: u64,
    native_status: u64,
    native_state: u64,
    stage: u64,
    failure: u64,
    payload_offset: u64,
    payload_bytes: u64,
    failure_details: [u64; 4],
    alpha_gate_bits: u64,
    dither_scale_bits: u64,
    dither_bias_bits: u64,
    dither_selector: u64,
    control_mask: u64,
    hinted_index: u64,
    descriptor_mask: u64,
    texture_confirmed: u64,
}
const _: () = assert!(core::mem::size_of::<MaterialRow>() == 29 * 8);
struct Control {
    stamp: DescriptorStamp,
    address: u64,
    bytes: u32,
    value: [u32; 4],
}
struct MaterialCopy {
    list: *mut c_void,
    lease: Material,
    permit: Option<ID3D12Resource>,
    mapped: *mut c_void,
    state: ReadbackState,
    controls: Vec<Control>,
    textures: [Srv; 2],
    sampler: Sampler,
}
impl Drop for MaterialCopy {
    fn drop(&mut self) {
        if let (false, Some(permit)) = (self.mapped.is_null(), &self.permit) {
            let written = D3D12_RANGE { Begin: 0, End: 8 };
            // SAFETY: the permit mapped at its creation.
            unsafe { permit.Unmap(0, Some(addr_of!(written))) };
        }
    }
}

struct Work {
    begin_qpc: u64,
    camera_heap: Option<ID3D12DescriptorHeap>,
    native_camera: Upload,
    camera_checked: bool,
    native_camera_draws: u64,
    alpha_reference: bool,
    alpha_optimized: bool,
    skinned_alpha: bool,
    alpha_valid: bool,
    native_recordings: [Recording; 2],
    native_recording_submitted: [bool; 2],
    camera_stage_masks: [u64; 4],
    pass: Pass,
    items: Vec<Item>,
    instance_rows: Vec<InstanceRow>,
    instance_blob: Vec<u8>,
    membership: Box<EyeMembership>,
    gpu_membership: Box<EyeMembership>,
    renderer: u64,
    gpu_instance_resource: u64,
    frame: u32,
    limit: u32,
    main_group: u64,
    peer_group: u64,
    main_done: bool,
    whole: bool,
    created: bool,
    combined_instance_read: bool,
    mode: u32,
    main_list: *mut c_void,
    peer_list: *mut c_void,
    status: u32,
    final_stats: [u32; 16],
    skin_stats: [u64; 6],
    binding_checks: Vec<BindingCheck>,
    binding_stats: [u64; 4],
    bindings_valid: bool,
    alpha_checks: Vec<AlphaCheck>,
    material_readback: Option<ID3D12Resource>,
    material_capture: bool,
    material_rows: Vec<MaterialRow>,
    material_copies: Vec<Box<MaterialCopy>>,
    material_blob: Vec<u8>,
    material_hints: Vec<[u32; 2]>,
    material_descriptors: Vec<[Value; 3]>,
}
impl Work {
    fn new() -> Box<Work> {
        Box::new(Work {
            begin_qpc: 0,
            camera_heap: None,
            native_camera: Upload::default(),
            camera_checked: false,
            native_camera_draws: 0,
            alpha_reference: false,
            alpha_optimized: false,
            skinned_alpha: false,
            alpha_valid: true,
            native_recordings: Default::default(),
            native_recording_submitted: [false; 2],
            camera_stage_masks: [0; 4],
            pass: Pass::default(),
            items: Vec::new(),
            instance_rows: Vec::new(),
            instance_blob: Vec::new(),
            membership: Box::default(),
            gpu_membership: Box::default(),
            renderer: 0,
            gpu_instance_resource: 0,
            frame: 0,
            limit: 0,
            main_group: 0,
            peer_group: 0,
            main_done: false,
            whole: false,
            created: false,
            combined_instance_read: false,
            mode: 0,
            main_list: null_mut(),
            peer_list: null_mut(),
            status: 0,
            final_stats: [0; 16],
            skin_stats: [0; 6],
            binding_checks: Vec::new(),
            binding_stats: [0; 4],
            bindings_valid: true,
            alpha_checks: Vec::new(),
            material_readback: None,
            material_capture: false,
            material_rows: Vec::new(),
            material_copies: Vec::new(),
            material_blob: Vec::new(),
            material_hints: Vec::new(),
            material_descriptors: Vec::new(),
        })
    }
}

// ---- the shared state (the .cpp's mutex and its globals)

struct State {
    work: Option<Box<Work>>,
    pool: Vec<Box<Work>>,
    run_budget: u32,
    last_run_frame: u32,
    next_group: u64,
}
struct Shared {
    lock: Mutex<()>,
    state: UnsafeCell<State>,
}
// SAFETY: the state is touched only under `lock`.
unsafe impl Sync for Shared {}
static SHARED: Shared = Shared {
    lock: Mutex::new(()),
    state: UnsafeCell::new(State { work: None, pool: Vec::new(), run_budget: 0, last_run_frame: u32::MAX, next_group: 0 }),
};
struct Locked {
    _guard: MutexGuard<'static, ()>,
}
impl Locked {
    fn state(&mut self) -> &mut State {
        // SAFETY: the lock is held.
        unsafe { &mut *SHARED.state.get() }
    }
}
fn lock() -> Locked {
    Locked { _guard: SHARED.lock.lock().unwrap_or_else(|e| e.into_inner()) }
}
static MATERIAL_WATCHING: AtomicBool = AtomicBool::new(false);

/// A CPU phase's time, counted while a run tracks.
struct CpuScope {
    phase: usize,
    begin: i64,
}
impl CpuScope {
    fn new(phase: usize) -> Self {
        CpuScope { phase, begin: if run_tracking() { qpc() } else { 0 } }
    }
    fn end(&mut self) {
        if self.begin != 0 {
            let now = qpc();
            CyberpunkVR_EarlyDepthRunCpu[self.phase * 2].fetch_add(1, SeqCst);
            CyberpunkVR_EarlyDepthRunCpu[self.phase * 2 + 1].fetch_add(now.wrapping_sub(self.begin) as u64, SeqCst);
            self.begin = 0;
        }
    }
}
impl Drop for CpuScope {
    fn drop(&mut self) {
        self.end();
    }
}

fn run_fail(reason: u32) {
    if CyberpunkVR_EarlyDepthRunState.load(SeqCst) != 1 {
        return;
    }
    if CyberpunkVR_EarlyDepthRunStats[15].load(SeqCst) == 0 {
        CyberpunkVR_EarlyDepthRunStats[15].store(u64::from(reason), SeqCst);
    }
    CyberpunkVR_EarlyDepthRunStop.store(1, SeqCst);
    CyberpunkVR_PreparationRunStop.store(1, SeqCst);
}

// ---- the material capture

fn capture_material(value: &mut Work, list: *mut c_void, draw: &DrawRecord, family: &Family, lease: Material) -> bool {
    const CAPACITY: usize = 128;
    const STRIDE: u64 = 512;
    if value.material_rows.len() >= CAPACITY || (list != value.main_list && list != value.peer_list) {
        return false;
    }
    value.material_rows.reserve(CAPACITY.saturating_sub(value.material_rows.len()));
    value.material_copies.reserve(CAPACITY.saturating_sub(value.material_copies.len()));
    value.material_descriptors.reserve(CAPACITY.saturating_sub(value.material_descriptors.len()));
    let mut copy = Box::new(MaterialCopy {
        list,
        lease,
        permit: None,
        mapped: null_mut(),
        state: ReadbackState::default(),
        controls: Vec::new(),
        textures: [Srv::default(), Srv::default()],
        sampler: Sampler::default(),
    });
    let mut hint = u32::MAX;
    let mut descriptor_mask = 0u64;
    let mut descriptor_values: [Value; 3] = [[0; 20]; 3];
    for item in &value.material_hints {
        if item[0] == copy.lease.0.native.handle {
            hint = item[1];
            break;
        }
    }
    let mut texture = |copy: &mut MaterialCopy, slot: usize, space: u32, register: u32| {
        if let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_SRV, space, register, D3D12_SHADER_VISIBILITY_PIXEL) {
            if resolve_srv(list, draw.root, parameter, offset, &mut copy.textures[slot]) {
                descriptor_mask |= 1u64 << slot;
                descriptor_values[slot] = copy.textures[slot].description;
            }
        }
    };
    if hint != u32::MAX {
        texture(&mut copy, 0, 1, hint);
    }
    texture(&mut copy, 1, 0, 67);
    if let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, 0, 0, D3D12_SHADER_VISIBILITY_PIXEL) {
        if resolve_sampler(list, draw.root, parameter, offset, &mut copy.sampler) {
            descriptor_mask |= 4;
            descriptor_values[2] = copy.sampler.stamp.value;
        }
    }
    let mut control_mask = 0u64;
    let (mut modifiers, mut selector) = ([0u32; 4], [0u32; 4]);
    for location in family.bindings() {
        let binding = &location.binding;
        if binding.kind != D3D12_DESCRIPTOR_RANGE_TYPE_CBV
            || binding.visibility != D3D12_SHADER_VISIBILITY_PIXEL
            || (binding.source_register != 0 && binding.source_register != 7)
        {
            continue;
        }
        let (offset, bytes) = if binding.source_register == 0 { (452u32, 4u32) } else { (0, 16) };
        let mut control = Control { stamp: zeroed(), address: 0, bytes: 0, value: [0; 4] };
        let mut again = [0u32; 4];
        let Some(cbv) = resolve_cbv(list, draw.root, location.source_parameter, location.source_offset) else { continue };
        if cbv.bytes < offset + bytes
            || cbv.address > u64::MAX - u64::from(offset)
            || !capture_descriptor(list, draw.root, location.source_parameter, location.source_offset, &mut control.stamp)
            || control.stamp.value[4] != cbv.address
            || control.stamp.value[5] != u64::from(cbv.bytes)
        {
            continue;
        }
        control.address = cbv.address + u64::from(offset);
        control.bytes = bytes;
        if !read_fixed(control.address, &mut control.value, bytes as usize)
            || !read_fixed(control.address, &mut again, bytes as usize)
            || control.value != again
        {
            continue;
        }
        if binding.source_register == 7 {
            modifiers = control.value;
            control_mask |= 1;
        } else {
            selector = control.value;
            control_mask |= 2;
        }
        copy.controls.push(control);
    }
    let Some(device) = device_of(list) else { return false };
    let _internal = cvr_host::probe::Internal::new();
    if value.material_readback.is_none() {
        let mut made: Option<ID3D12Resource> = None;
        // SAFETY: a live device; `made` is writable.
        let hr = unsafe {
            crate::readback_buffers::cvr_rb_buffer(
                device.as_raw(),
                null_mut(),
                CAPACITY as u64 * STRIDE,
                D3D12_HEAP_TYPE_READBACK.0,
                D3D12_RESOURCE_STATE_COPY_DEST.0,
                (&mut made as *mut Option<ID3D12Resource>).cast(),
            )
        };
        if hr.is_err() {
            return false;
        }
        value.material_readback = made;
    }
    {
        let mut word = null_mut();
        // SAFETY: a live device; the outputs are writable.
        if unsafe { crate::readback_buffers::cvr_rb_mapped_word(device.as_raw(), &mut word, &mut copy.mapped) }.is_err() {
            return false;
        }
        // SAFETY: the permit's one reference.
        copy.permit = (!word.is_null()).then(|| unsafe { ID3D12Resource::from_raw(word) });
    }
    let objects = [copy.lease.0.source.resource, raw(&value.material_readback), raw(&copy.permit)];
    // SAFETY: the list's live objects.
    if !unsafe { cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) } {
        return false;
    }
    let mut state = ReadState::default();
    // SAFETY: the lease's buffer; `state` is writable.
    unsafe { cvr_iab_read_state(list, &copy.lease.0.source, &mut state) };
    let slot = value.material_rows.len() as u64;
    let source = &copy.lease.0;
    let mut row = MaterialRow {
        sequence: draw.sequence,
        frame: u64::from(draw.frame),
        side: u64::from(draw.side),
        family: u64::from(family.view.asset),
        resource: source.source.resource as u64,
        handle: u64::from(source.native.handle),
        kind: u64::from(source.native.record.kind),
        descriptor: source.binding.descriptor,
        address: source.binding.address,
        source_offset: source.source_offset,
        heap_type: u64::from(source.source.heap_type),
        native_status: u64::from(state.status),
        native_state: u64::from(state.value),
        hinted_index: u64::from(u32::MAX),
        ..MaterialRow::default()
    };
    row.alpha_gate_bits = u64::from(modifiers[0]);
    row.dither_scale_bits = u64::from(modifiers[2]);
    row.dither_bias_bits = u64::from(modifiers[3]);
    row.dither_selector = u64::from(selector[0]);
    row.control_mask = control_mask;
    row.hinted_index = u64::from(hint);
    row.descriptor_mask = descriptor_mask;
    let Some(recording) = list_of(list) else { return false };
    // SAFETY: a live list and the permit it keeps.
    unsafe { recording.SetPredication(copy.permit.as_ref(), 0, D3D12_PREDICATION_OP_EQUAL_ZERO) };
    let copied = material_read::copy(list, &mut copy.lease.0, raw(&value.material_readback), slot * STRIDE);
    // SAFETY: as above.
    unsafe { recording.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO) };
    if !copied {
        row.failure = u64::from(material_read::cvr_mread_last_failure());
        // SAFETY: `failure_details` holds four values.
        unsafe { cvr_buffer_read_last_failure_details(&mut row.failure_details) };
        copy.state.invalidated();
    }
    row.stage = u64::from(copy.state.stage);
    value.material_rows.push(row);
    value.material_copies.push(copy);
    value.material_descriptors.push(descriptor_values);
    MATERIAL_WATCHING.store(true, SeqCst);
    row.failure != 40
}
fn submit_materials(value: &mut Work, queue: *mut c_void, count: u32, lists: *const *mut c_void, after: bool) {
    if lists.is_null() {
        return;
    }
    let lists = slice(lists, count as usize);
    for i in 0..value.material_copies.len() {
        let copy = &mut *value.material_copies[i];
        let row = &mut value.material_rows[i];
        if copy.state.stage == SEALED {
            continue;
        }
        let occurrences = lists.iter().filter(|&&l| l == copy.list).count() as u32;
        if occurrences == 0 {
            continue;
        }
        if after {
            copy.state.submitted();
        } else {
            // SAFETY: the copy's list and permit.
            let owned = unsafe { cvr_gpu_has_command_resource(copy.list, raw(&copy.permit)) };
            let valid = material_read::validate_submission(&copy.lease.0, queue);
            for slot in 0..2 {
                if row.descriptor_mask & (1u64 << slot) != 0 && !matches(&copy.textures[slot].stamp) {
                    row.descriptor_mask &= !(1u64 << slot);
                }
            }
            if row.descriptor_mask & 4 != 0 && !matches(&copy.sampler.stamp) {
                row.descriptor_mask &= !4u64;
            }
            for control in &copy.controls {
                let (mut current, mut again) = ([0u32; 4], [0u32; 4]);
                if !matches(&control.stamp)
                    || !read_fixed(control.address, &mut current, control.bytes as usize)
                    || !read_fixed(control.address, &mut again, control.bytes as usize)
                    || current != control.value
                    || again != control.value
                {
                    row.control_mask = 0;
                }
            }
            let update = copy.state.before_submit(occurrences, valid, owned);
            if update != PERMIT_NONE {
                write_permit(copy.mapped, u64::from(update == PERMIT_ENABLE));
                sfence();
            }
            if copy.state.stage == REJECTED && row.failure == 0 {
                row.failure = if valid { 103 } else { 100 };
            }
        }
        row.stage = u64::from(copy.state.stage);
    }
}
fn seal_materials(value: &mut Work) -> HRESULT {
    if value.material_copies.is_empty() {
        return S_OK;
    }
    if value.material_copies.iter().any(|c| c.state.pending != 0) {
        return S_FALSE;
    }
    if value.material_copies.iter().any(|c| c.state.permit_may_be_in_flight(true)) {
        return E_FAIL;
    }
    for copy in &mut value.material_copies {
        if copy.state.seal(true) || copy.state.disarm_rejected(true) {
            write_permit(copy.mapped, 0);
        }
    }
    sfence();
    let mut sealed = false;
    for i in 0..value.material_copies.len() {
        let state = &value.material_copies[i].state;
        if state.enabled {
            return E_FAIL;
        }
        value.material_rows[i].stage = u64::from(state.stage);
        sealed |= state.stage == SEALED;
    }
    if !sealed {
        value.material_blob.clear();
        MATERIAL_WATCHING.store(false, SeqCst);
        return S_OK;
    }
    let mut blob = vec![0u8; value.material_copies.len() * LOGICAL_BYTES];
    for i in 0..value.material_copies.len() {
        value.material_rows[i].stage = u64::from(value.material_copies[i].state.stage);
    }
    let mut bytes = 0usize;
    // SAFETY: the readback, the rows (29 values each) and the blob of `count` payloads.
    let result = unsafe {
        crate::early_depth_run::cvr_ned_seal_read(
            raw(&value.material_readback),
            value.material_rows.as_mut_ptr().cast(),
            value.material_copies.len(),
            SEALED,
            LOGICAL_BYTES as u32,
            blob.as_mut_ptr(),
            &mut bytes,
        )
    };
    if result < 0 {
        return HRESULT(result);
    }
    blob.truncate(bytes);
    value.material_blob = blob;
    MATERIAL_WATCHING.store(false, SeqCst);
    S_OK
}

// ---- the eyes' packet membership

/// packets::ResolveLive: every packet's inputs resolved from the renderer's live pool into one blob.
fn resolve_live(renderer: u64, packets: &[Packet], geometry_only: bool) -> Option<(Vec<Inputs>, Vec<u8>)> {
    let mut pool = 0u64;
    let mut uploaded = MappedInstances::default();
    // SAFETY: the outputs are writable.
    if !unsafe { cvr_gp_live(renderer, packets.len(), &mut pool, &mut uploaded) } {
        return None;
    }
    let mut result = Vec::with_capacity(packets.len());
    let mut blob: Vec<u8> = Vec::new();
    for &packet in packets {
        let mut reader = |address: u64, out: &mut [u8]| crate::geometry_packets::read_input(Some(&uploaded), address, out);
        let len = blob.len();
        let data = blob.as_mut_ptr();
        let target: *mut Vec<u8> = &mut blob;
        let mut grow = |size: usize, growing: bool| {
            // SAFETY: the blob outlives the call; nothing else borrows it meanwhile.
            let bytes = unsafe { &mut *target };
            if growing && size > bytes.capacity() {
                let wanted = MAX_INPUT_BLOB_BYTES.min(size.max(if bytes.capacity() != 0 { bytes.capacity() + bytes.capacity() / 2 } else { 4096 }));
                bytes.reserve_exact(wanted.saturating_sub(bytes.len()));
            }
            bytes.resize(size, 0);
            bytes.as_mut_ptr()
        };
        let mut view = Blob { len, data, resize: &mut grow };
        result.push(packet_inputs::resolve_details(packet, pool, uploaded, Some(&mut reader), &mut view, geometry_only));
    }
    Some((result, blob))
}
fn same_inputs(a: &[Inputs], b: &[Inputs]) -> bool {
    let bytes = |v: &[Inputs]| -> &[u8] {
        // SAFETY: plain 256-byte records.
        unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), core::mem::size_of_val(v)) }
    };
    bytes(a) == bytes(b)
}
fn membership_inputs(input: &Inputs) -> &eye_membership::Inputs {
    // SAFETY: both mirror packets::Inputs (256 bytes, the same layout).
    unsafe { &*(input as *const Inputs).cast::<eye_membership::Inputs>() }
}
/// packets::GpuInstanceIdentities.
fn gpu_instance_identities(resource: u64, address: u64, count: u32, stride: u32) -> Option<Vec<u8>> {
    // SAFETY: a validity check only.
    if !unsafe { crate::eye_membership::cvr_em_identities(resource, address, count, stride, null_mut()) } {
        return None;
    }
    let mut result = vec![0u8; count as usize * stride as usize];
    // SAFETY: `result` holds count*stride bytes (stride 48).
    unsafe { crate::eye_membership::cvr_em_identities(resource, address, count, stride, result.as_mut_ptr()) };
    Some(result)
}
fn prepare_membership(work: &mut Work, renderer: u64, span: u64) -> bool {
    let _cpu = CpuScope::new(0);
    let view = ex!(CyberpunkVR_PreparationPairData)[3];
    let Some((range, peer_storage)) = crate::early_depth_span::span(&read, span, view) else { return false };
    let mut inputs: [Vec<Packet>; 2] = [Vec::new(), Vec::new()];
    inputs[0] = vec![Packet::default(); ((range[1] - range[0]) / 16) as usize];
    if !inputs[0].is_empty() {
        // SAFETY: plain 16-byte packets.
        let bytes = unsafe { core::slice::from_raw_parts_mut(inputs[0].as_mut_ptr().cast::<u8>(), inputs[0].len() * 16) };
        if !read(range[0], bytes) {
            return false;
        }
    }
    let mut peer = crate::eager_depth::Snapshot::default();
    if !peer.capture(peer_storage + 0x34C0, Some(read_raw)) {
        return false;
    }
    inputs[1] = peer.packets.iter().map(|p| Packet { first: p.first, second: p.second }).collect();
    for side in 0..2usize {
        let geometry_only = run_tracking();
        let (Some((details, blob)), Some((again, again_blob))) =
            (resolve_live(renderer, &inputs[side], geometry_only), resolve_live(renderer, &inputs[side], geometry_only))
        else {
            return false;
        };
        if details.len() != again.len() || blob != again_blob || !same_inputs(&details, &again) {
            return false;
        }
        if geometry_only && CyberpunkVR_EarlyDepthRunStats[0].load(SeqCst) == 1 {
            let full = resolve_live(renderer, &inputs[side], false);
            let Some((full, full_blob)) = full.filter(|(f, _)| f.len() == details.len()) else {
                CyberpunkVR_EarlyDepthRunGeometryChecks[1].fetch_add(1, SeqCst);
                return false;
            };
            for i in 0..full.len() {
                if !packet_inputs::equal_geometry(&details[i], &blob, &full[i], &full_blob) {
                    CyberpunkVR_EarlyDepthRunGeometryChecks[1].fetch_add(1, SeqCst);
                    let failure = ex!(CyberpunkVR_EarlyDepthFailureData);
                    failure[0] = 5;
                    failure[1] = side as u64;
                    failure[2] = i as u64;
                    return false;
                }
            }
            CyberpunkVR_EarlyDepthRunGeometryChecks[0].fetch_add(1, SeqCst);
        }
        ex!(CyberpunkVR_EarlyDepthMaskStats)[side] = inputs[side].len() as u32;
        for i in 0..details.len() {
            let host_input = details[i];
            if host_input.source == 2 && host_input.instance_stride == 48 && host_input.instance_resource != 0 && host_input.instance_gpu_address != 0 {
                if work.gpu_instance_resource != 0 && work.gpu_instance_resource != host_input.instance_resource {
                    return false;
                }
                work.gpu_instance_resource = host_input.instance_resource;
                let Some(identities) =
                    gpu_instance_identities(host_input.instance_resource, host_input.instance_gpu_address, host_input.instances, 48)
                else {
                    return false;
                };
                let mut gpu = host_input;
                gpu.valid |= 16;
                gpu.instance_offset = 0;
                gpu.instance_bytes = identities.len() as u32;
                if !work.gpu_membership.add(side as u32, inputs[side][i].second, membership_inputs(&gpu), &identities) {
                    return false;
                }
                continue;
            }
            if !work.membership.add(side as u32, inputs[side][i].second, membership_inputs(&host_input), &blob) {
                let d = &details[i];
                let failure = [1, side as u64, i as u64, u64::from(d.valid), u64::from(d.source), u64::from(d.instances)];
                *ex!(CyberpunkVR_EarlyDepthFailureData) = failure;
                return false;
            }
        }
        let stats = ex!(CyberpunkVR_EarlyDepthMaskStats);
        stats[2 + side] = work.membership.totals[side].wrapping_add(work.gpu_membership.totals[side]);
        stats[8 + side] = work.membership.unresolved[side];
    }
    let extras = work.membership.known_extras();
    let stats = ex!(CyberpunkVR_EarlyDepthMaskStats);
    stats[13] = extras;
    stats[12] = extras;
    work.renderer = renderer;
    true
}

// ---- the published state

fn publish(s: &State) {
    let w = s.work.as_deref();
    // SAFETY: the early depth's exports, under its lock.
    unsafe {
        CyberpunkVR_EarlyDepthMaterialRows = w.map_or(0, |w| data_ptr(&w.material_rows));
        CyberpunkVR_EarlyDepthMaterialBlob = w.map_or(0, |w| data_ptr(&w.material_blob));
        CyberpunkVR_EarlyDepthMaterialCount = w.map_or(0, |w| w.material_rows.len() as u32);
        CyberpunkVR_EarlyDepthMaterialBytes = w.map_or(0, |w| w.material_blob.len() as u32);
        CyberpunkVR_EarlyDepthMaterialDescriptors = w.map_or(0, |w| data_ptr(&w.material_descriptors));
        CyberpunkVR_EarlyDepthInstanceRows = w.map_or(0, |w| data_ptr(&w.instance_rows));
        CyberpunkVR_EarlyDepthInstanceBlob = w.map_or(0, |w| data_ptr(&w.instance_blob));
        CyberpunkVR_EarlyDepthInstanceCount = w.map_or(0, |w| w.instance_rows.len() as u32);
        CyberpunkVR_EarlyDepthInstanceBytes = w.map_or(0, |w| w.instance_blob.len() as u32);
    }
    let Some(w) = w else { return };
    *ex!(CyberpunkVR_EarlyDepthNativeCounts) = w.pass.native_counts();
    *ex!(CyberpunkVR_EarlyDepthNativeDifferences) = w.pass.native_differences();
    *ex!(CyberpunkVR_EarlyDepthOptimized) = w.pass.optimized_counts();
    *ex!(CyberpunkVR_EarlyDepthGpuTimes) = w.pass.gpu_times();
    ex!(CyberpunkVR_EarlyDepthGpuInstances)[2..4].copy_from_slice(&w.pass.instance_proof_counts());
    *ex!(CyberpunkVR_EarlyDepthShaderProofs) = w.pass.shader_proof_counts();
    *ex!(CyberpunkVR_EarlyDepthAlphaProofs) = w.pass.alpha_proof_counts();
    *ex!(CyberpunkVR_EarlyDepthBindingChecks) = w.binding_stats;
}
fn fail(s: &mut State, reason: u32, result: HRESULT) {
    publish(s);
    CyberpunkVR_EarlyDepthStats[15].store(reason, SeqCst);
    // SAFETY: the export, under the lock.
    unsafe { CyberpunkVR_EarlyDepthResult = result.0 };
    CyberpunkVR_EarlyDepthState.store(4, SeqCst);
    if let Some(w) = s.work.as_mut() {
        w.status = 4;
    }
    run_fail(reason);
}
fn clear_output() {
    MATERIAL_WATCHING.store(false, SeqCst);
    // SAFETY: the exports, under the lock.
    unsafe {
        *addr_of_mut!(CyberpunkVR_EarlyDepthComparison) = zeroed();
        crate::early_depth_exports::cvr_ned_clear_output(
            addr_of_mut!(CyberpunkVR_EarlyDepthMaterialDescriptors),
            addr_of_mut!(CyberpunkVR_EarlyDepthAlphaStats),
            addr_of_mut!(CyberpunkVR_EarlyDepthSkinnedAlphaStats),
            addr_of_mut!(CyberpunkVR_EarlyDepthAlphaProofs),
        );
    }
}

// ---- a run's works

fn complete_run_work(value: &mut Work) {
    if value.status != 2 {
        return;
    }
    let mut result: Comparison = zeroed();
    let hr = value.pass.read(&mut result);
    if hr == S_FALSE {
        return;
    }
    if hr.is_err() {
        value.status = 4;
        run_fail(32);
        return;
    }
    let run = crate::early_depth_run::RunWork {
        frame: value.frame,
        final_stats: value.final_stats,
        native: value.pass.native_counts(),
        optimized: value.pass.optimized_counts(),
        times: value.pass.gpu_times(),
        instance_proofs: value.pass.instance_proof_counts(),
        shader_proofs: value.pass.shader_proof_counts(),
        skin_stats: value.skin_stats,
        binding_stats: value.binding_stats,
        camera_stage_masks: value.camera_stage_masks,
        native_camera_draws: value.native_camera_draws,
        begin_qpc: value.begin_qpc,
        native_camera: value.native_camera.gpu_address != 0,
    };
    let exports = crate::early_depth_run::RunExports {
        instance_proofs: &CyberpunkVR_EarlyDepthRunInstanceProofs,
        shader_proofs: &CyberpunkVR_EarlyDepthRunShaderProofs,
        skinning_stats: &CyberpunkVR_EarlyDepthRunSkinningStats,
        binding_checks: &CyberpunkVR_EarlyDepthRunBindingChecks,
        native_camera: &CyberpunkVR_EarlyDepthRunNativeCamera,
        camera_stage_masks: &CyberpunkVR_EarlyDepthRunCameraStageMasks,
        seq: &CyberpunkVR_EarlyDepthRunSeq,
        rows: addr_of_mut!(CyberpunkVR_EarlyDepthRunRows),
        frame_starts: addr_of_mut!(CyberpunkVR_EarlyDepthRunFrameStarts),
        count: &CyberpunkVR_EarlyDepthRunCount,
        stats: &CyberpunkVR_EarlyDepthRunStats,
    };
    // SAFETY: the run's exports.
    if !unsafe { crate::early_depth_run::cvr_ned_complete_run(&run, &exports) } {
        value.status = 4;
        run_fail(33);
        return;
    }
    value.status = 3;
}
fn poll_run(s: &mut State) {
    if let Some(w) = s.work.as_mut() {
        complete_run_work(w);
    }
    for value in &mut s.pool {
        complete_run_work(value);
    }
}
fn acquire_run_work(s: &mut State) -> bool {
    poll_run(s);
    if CyberpunkVR_EarlyDepthRunStats[15].load(SeqCst) != 0 {
        return false;
    }
    if s.work.as_ref().is_some_and(|w| w.status == 1) {
        run_fail(42);
        return false;
    }
    if s.work.as_ref().is_some_and(|w| w.status != 3) {
        let w = s.work.take().expect("checked");
        s.pool.push(w);
    }
    if s.work.is_none() {
        if let Some(at) = s.pool.iter().position(|w| w.status == 3) {
            s.work = Some(s.pool.remove(at));
        }
    }
    match s.work.as_mut() {
        None => {
            if s.pool.len() >= 3 {
                CyberpunkVR_EarlyDepthRunStats[7].fetch_add(1, SeqCst);
                CyberpunkVR_EarlyDepthState.store(0, SeqCst);
                return false;
            }
            s.work = Some(Work::new());
            CyberpunkVR_EarlyDepthRunStats[13].fetch_max(s.pool.len() as u64 + 1, SeqCst);
        }
        Some(w) => {
            if w.pass.restart() != S_OK {
                run_fail(34);
                return false;
            }
            CyberpunkVR_EarlyDepthRunStats[6].fetch_add(1, SeqCst);
            CyberpunkVR_EarlyDepthRunArenas[1].fetch_add(u64::from(w.pass.arena_count()), SeqCst);
            w.items.clear();
            w.instance_rows.clear();
            w.instance_blob.clear();
            w.membership = Box::default();
            w.gpu_membership = Box::default();
            w.renderer = 0;
            w.gpu_instance_resource = 0;
            w.main_group = 0;
            w.peer_group = 0;
            w.main_done = false;
            w.main_list = null_mut();
            w.peer_list = null_mut();
            w.final_stats = [0; 16];
            w.native_camera = Upload::default();
            w.camera_checked = false;
            w.native_camera_draws = 0;
            w.camera_stage_masks = [0; 4];
            w.skin_stats = [0; 6];
            w.binding_checks.clear();
            w.binding_stats = [0; 4];
            w.bindings_valid = true;
        }
    }
    CyberpunkVR_EarlyDepthRunStats[1].fetch_add(1, SeqCst);
    true
}

// ---- a draw's inputs

fn material_inputs(list: *mut c_void, d: &DrawRecord, family: &Family, bytes: &mut [u8; 112]) -> bool {
    let f = &family.view;
    if f.pixel_shader || f.use_count != 2 || !f.paired_root || f.binding_count != 2 {
        return false;
    }
    // SAFETY: the family's uses.
    if !unsafe { crate::early_depth_inputs::cvr_ned_material_uses(f.uses, f.use_count) } {
        return false;
    }
    for binding in family.bindings() {
        if binding.binding.source_register == 5 {
            if binding.source_type != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                return false;
            }
            let mut again = [0u8; 112];
            return resolve_cbv(list, d.root, binding.source_parameter, binding.source_offset).is_some_and(|value| {
                value.bytes as usize >= bytes.len()
                    && read_upload(value.address, bytes.as_mut_ptr().cast(), bytes.len(), false)
                    && read_upload(value.address, again.as_mut_ptr().cast(), again.len(), false)
                    && *bytes == again
            });
        }
    }
    false
}
fn vertex_binding(family: &Family, kind: D3D12_DESCRIPTOR_RANGE_TYPE, register: u32) -> Option<Location> {
    if !family.view.paired_root {
        return None;
    }
    let mut result = None;
    for entry in family.bindings() {
        if entry.binding.kind == kind && entry.binding.visibility == D3D12_SHADER_VISIBILITY_VERTEX && entry.binding.source_register == register {
            if result.is_some() || entry.source_type != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                return None;
            }
            result = Some(*entry);
        }
    }
    result
}
/// SkinMaterial's constants: the vertex CBV at `register` read twice into `bytes`, its descriptor stamped.
fn skin_constant(list: *mut c_void, d: &DrawRecord, family: &Family, register: u32, bytes: &mut [u8], stamp: &mut DescriptorStamp) -> bool {
    let Some(binding) = vertex_binding(family, D3D12_DESCRIPTOR_RANGE_TYPE_CBV, register) else { return false };
    let Some(value) = resolve_cbv(list, d.root, binding.source_parameter, binding.source_offset) else { return false };
    let mut again = [0u8; 448];
    value.bytes as usize >= bytes.len()
        && capture_descriptor(list, d.root, binding.source_parameter, binding.source_offset, stamp)
        && stamp.value[0] == 1
        && stamp.value[4] == value.address
        && stamp.value[5] == u64::from(value.bytes)
        && read_upload(value.address, bytes.as_mut_ptr().cast(), bytes.len(), true)
        && read_upload(value.address, again.as_mut_ptr().cast(), bytes.len(), true)
        && *bytes == again[..bytes.len()]
}
fn skin_resource(list: *mut c_void, d: &DrawRecord, family: &Family, register: u32, out: &mut Srv, stamp: &mut DescriptorStamp) -> bool {
    let Some(binding) = vertex_binding(family, D3D12_DESCRIPTOR_RANGE_TYPE_SRV, register) else { return false };
    resolve_srv(list, d.root, binding.source_parameter, binding.source_offset, out)
        && capture_descriptor(list, d.root, binding.source_parameter, binding.source_offset, stamp)
        && stamp.value == out.description
}
fn skin_material(list: *mut c_void, d: &DrawRecord, family: &Family, material: &mut [u8; 112], output: &mut SkinInputs, copy_proof: bool) -> bool {
    let stats = ex!(CyberpunkVR_EarlyDepthSkinningStats);
    let previous = stats[5];
    stats[5] = 1;
    let f = &family.view;
    let alpha = f.skinning_read_contract == 2 && f.alpha_depth_contract == 2 && f.pixel_shader && f.native_camera_stages == 1;
    if (!alpha && (f.skinning_read_contract != 1 || f.pixel_shader)) || d.vertices[7].StrideInBytes != 64 {
        return false;
    }
    output.alpha = alpha;
    // SAFETY: the family's uses.
    if !unsafe { crate::early_depth_checks::cvr_ned_skin_uses(f.uses, f.use_count, alpha) } {
        return false;
    }
    stats[5] = 2;
    {
        let [first, second, ..] = &mut output.descriptors;
        if !skin_constant(list, d, family, 5, material, &mut first.0) || !skin_constant(list, d, family, 7, &mut output.modifiers.0, &mut second.0) {
            return false;
        }
    }
    stats[5] = 3;
    if !skinning::zero_cull(&output.modifiers.0) {
        return false;
    }
    stats[5] = 4;
    {
        let [_, _, third, fourth] = &mut output.descriptors;
        if !skin_resource(list, d, family, 10, &mut output.bones, &mut third.0) || !skin_resource(list, d, family, 77, &mut output.deformation, &mut fourth.0) {
            return false;
        }
    }
    // SAFETY: a view description.
    if !unsafe { crate::early_depth_inputs::cvr_ned_deformation_view(&output.deformation.view) } {
        return false;
    }
    stats[5] = 5;
    let Some(bones) = output.bones.resource.as_ref() else { return false };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource; local outputs.
    if unsafe { bones.GetHeapProperties(Some(addr_of_mut!(heap)), Some(addr_of_mut!(flags))) }.is_err()
        || (heap.Type != D3D12_HEAP_TYPE_UPLOAD && heap.Type != D3D12_HEAP_TYPE_GPU_UPLOAD)
    {
        return false;
    }
    // Reference mode uses the already-bound native SRV and retains native eye output for exact comparison. Only
    // replacement needs an early copy lease.
    if copy_proof && d.side == 0 && !texture_acquire(list, raw(&output.deformation.resource), &mut output.texture) {
        stats[5] = 100 + u64::from(texture_failure());
        return false;
    }
    stats[5] = previous;
    true
}
fn same_skin(a: &Option<SkinInputs>, b: &Option<SkinInputs>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.alpha == b.alpha
                && skinning::same_modifiers(&a.modifiers.0, &b.modifiers.0)
                && a.bones.description == b.bones.description
                && a.deformation.description == b.deformation.description
                && raw(&a.bones.resource) == raw(&b.bones.resource)
                && raw(&a.deformation.resource) == raw(&b.deformation.resource)
        }
        _ => false,
    }
}
/// alpha_depth::InstanceStride.
fn instance_stride(contract: u32) -> u32 {
    match contract {
        1 => 48,
        2 => 64,
        _ => 0,
    }
}
#[allow(clippy::too_many_arguments)]
fn alpha_material(
    list: *mut c_void,
    draw: &DrawRecord,
    family: &Family,
    material: &mut [u8; 112],
    output: &mut AlphaInputs,
    optimized: bool,
    hints: &[[u32; 2]],
) -> bool {
    let reason = &mut ex!(CyberpunkVR_EarlyDepthAlphaStats)[5];
    *reason = 1;
    let f = &family.view;
    let stride = instance_stride(f.alpha_depth_contract);
    if stride == 0
        || !f.pixel_shader
        || f.native_camera_stages != 1
        || draw.vertices[7].StrideInBytes != stride
        || (f.alpha_depth_contract == 2 && f.skinning_read_contract != 2)
    {
        return false;
    }
    output.skinned = f.alpha_depth_contract == 2;
    let vertex = vertex_binding(family, D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 5);
    let mut again = [0u8; 112];
    *reason = 2;
    let Some(vertex) = vertex else { return false };
    let Some(cbv) = resolve_cbv(list, draw.root, vertex.source_parameter, vertex.source_offset) else { return false };
    if (cbv.bytes as usize) < material.len()
        || !capture_descriptor(list, draw.root, vertex.source_parameter, vertex.source_offset, &mut output.vertex_material)
        || !read_fixed(cbv.address, material, material.len())
        || !read_fixed(cbv.address, &mut again, 112)
        || !(if output.skinned { skinning::same_material(material, &again) } else { *material == again })
    {
        return false;
    }
    *reason = 3;
    let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 0, 4, D3D12_SHADER_VISIBILITY_PIXEL) else {
        return false;
    };
    output.material = Material::default();
    if material_observe::resolve_at_draw(list, draw.root, parameter, offset, &mut output.material.0) != Status::Registered {
        return false;
    }
    *reason = 4;
    let mut modifiers_again = [0u32; 4];
    let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 0, 7, D3D12_SHADER_VISIBILITY_PIXEL) else {
        return false;
    };
    let Some(cbv) = resolve_cbv(list, draw.root, parameter, offset) else { return false };
    if cbv.bytes < 16
        || !capture_descriptor(list, draw.root, parameter, offset, &mut output.modifier_binding)
        || !read_fixed(cbv.address, &mut output.modifiers, 16)
        || !read_fixed(cbv.address, &mut modifiers_again, 16)
        || output.modifiers != modifiers_again
    {
        return false;
    }
    *reason = 5;
    // SAFETY: the modifiers' four values.
    if !unsafe { crate::early_depth_alpha::cvr_ned_alpha_modifiers(&output.modifiers) } {
        return false;
    }
    let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 0, 67, D3D12_SHADER_VISIBILITY_PIXEL) else {
        return false;
    };
    if !resolve_srv(list, draw.root, parameter, offset, &mut output.dither) {
        return false;
    }
    let Some(dither) = output.dither.resource.as_ref() else { return false };
    // SAFETY: a live resource.
    let desc = unsafe { dither.GetDesc() };
    // SAFETY: the view and the description.
    if !unsafe { crate::early_depth_alpha::cvr_ned_dither_view(&output.dither.view, &desc) } {
        return false;
    }
    *reason = 6;
    let Some((parameter, offset)) = find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, 0, 0, D3D12_SHADER_VISIBILITY_PIXEL) else {
        return false;
    };
    if !resolve_sampler(list, draw.root, parameter, offset, &mut output.sampler) {
        return false;
    }
    if optimized {
        *reason = 7;
        for hint in hints {
            if hint[0] == output.material.0.native.handle {
                output.expected_index = hint[1];
                break;
            }
        }
        if output.expected_index >= 32768 {
            return false;
        }
        let Some((parameter, offset)) =
            find_binding(draw.root, D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, output.expected_index, D3D12_SHADER_VISIBILITY_PIXEL)
        else {
            return false;
        };
        if !resolve_srv(list, draw.root, parameter, offset, &mut output.image) {
            return false;
        }
        // SAFETY: a view description.
        if !unsafe { crate::early_depth_alpha::cvr_ned_alpha_image(&output.image.view) } {
            return false;
        }
        if draw.side == 0 {
            if !texture_acquire(list, raw(&output.image.resource), &mut output.texture) {
                *reason = 100 + u64::from(texture_failure());
                return false;
            }
            let device = device_of(list);
            let mut footprints = vec![zeroed::<MipFootprint>(); D3D12_REQ_SUBRESOURCES as usize];
            let mut count = 0usize;
            let viewed = device.as_ref().is_some_and(|device| {
                // SAFETY: the device, the lease and the view; `footprints` holds its capacity.
                unsafe {
                    cvr_tr_view_footprints(device.as_raw(), &output.texture.0, &output.image.view, 0, footprints.as_mut_ptr(), footprints.len(), &mut count)
                }
            });
            if !viewed {
                *reason = 200 + u64::from(texture_failure());
                return false;
            }
            footprints.truncate(count);
            output.mips.extend(footprints.iter().map(|f| f.mip));
        }
    }
    *reason = 0;
    true
}
fn same_alpha(a: &Option<AlphaInputs>, b: &Option<AlphaInputs>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let (m, n) = (&a.material.0, &b.material.0);
            a.skinned == b.skinned
                && m.native.registry == n.native.registry
                && m.native.handle == n.native.handle
                && m.native.record == n.native.record
                && m.source.resource == n.source.resource
                && a.modifiers == b.modifiers
                && a.dither.description == b.dither.description
                && a.sampler.stamp.value == b.sampler.stamp.value
                && a.expected_index == b.expected_index
                && a.image.description == b.image.description
                && raw(&a.image.resource) == raw(&b.image.resource)
        }
        _ => false,
    }
}
fn same_submitted_instances(check: &BindingCheck) -> bool {
    let source = &check.instance_source.0;
    // SAFETY: the check's held resource.
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&source.resource) }) else { return false };
    if check.instances.is_empty() || check.instance_offset > source.bytes || check.instances.len() as u64 > source.bytes - check.instance_offset {
        return false;
    }
    let mut current = vec![0u8; check.instances.len()];
    let mut mapped = null_mut();
    let range = D3D12_RANGE { Begin: check.instance_offset as usize, End: (check.instance_offset + check.instances.len() as u64) as usize };
    // SAFETY: a live upload buffer; `mapped` is writable.
    if unsafe { resource.Map(0, Some(addr_of!(range)), Some(addr_of_mut!(mapped))) }.is_err() {
        return false;
    }
    struct Unmap<'a>(&'a ID3D12Resource);
    impl Drop for Unmap<'_> {
        fn drop(&mut self) {
            let none = D3D12_RANGE::default();
            // SAFETY: the mapping above.
            unsafe { self.0.Unmap(0, Some(addr_of!(none))) };
        }
    }
    let _unmap = Unmap(resource);
    let base = mapped as u64;
    if base == 0 || base > u64::MAX - check.instance_offset {
        return false;
    }
    for _ in 0..2 {
        if !read(base + check.instance_offset, &mut current) || current != check.instances {
            return false;
        }
    }
    true
}
fn validate_submission(value: &mut Work, queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    let submitted = slice(lists, count as usize);
    let contains = |list: *mut c_void| !lists.is_null() && submitted.contains(&list);
    if value.alpha_optimized {
        for side in 0..2 {
            let list = value.native_recordings[side].0.list as *mut c_void;
            if list.is_null() || value.native_recording_submitted[side] || !contains(list) {
                continue;
            }
            value.native_recording_submitted[side] = true;
            // SAFETY: the recording's lease.
            if unsafe { cvr_native_recording_validate_submission(&value.native_recordings[side].0, queue) } {
                ex!(CyberpunkVR_EarlyDepthRecording)[2 + side] += 1;
            } else {
                value.alpha_valid = false;
                // SAFETY: a plain query.
                ex!(CyberpunkVR_EarlyDepthRecording)[4] = u64::from(unsafe { cvr_native_recording_last_failure() });
                ex!(CyberpunkVR_EarlyDepthAlphaStats)[4] += 1;
                // SAFETY: the export's eight values.
                unsafe { cvr_native_recording_last_details(addr_of_mut!(CyberpunkVR_EarlyDepthRecordingDetails)) };
            }
        }
    }
    for check in &mut value.alpha_checks {
        if check.submitted || !contains(check.list) {
            continue;
        }
        check.submitted = true;
        let inputs = &check.inputs;
        let mut valid = true;
        for stamp in [&inputs.vertex_material, &inputs.modifier_binding, &inputs.material.0.stamp, &inputs.dither.stamp, &inputs.sampler.stamp] {
            valid &= matches(stamp);
        }
        if inputs.image.resource.is_some() {
            valid &= matches(&inputs.image.stamp);
        }
        for _ in 0..2 {
            let mut material = [0u8; 112];
            let mut modifiers = [0u32; 4];
            valid &= read_fixed(inputs.vertex_material.value[4], &mut material, 112)
                && (if inputs.skinned { skinning::same_material(&material, &check.vertex_material) } else { material == check.vertex_material });
            valid &= read_fixed(inputs.modifier_binding.value[4], &mut modifiers, 16) && modifiers == inputs.modifiers;
        }
        if !valid {
            value.alpha_valid = false;
            ex!(CyberpunkVR_EarlyDepthAlphaStats)[4] += 1;
            if inputs.skinned {
                let stats = ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats);
                stats[4] += 1;
                stats[5] = 301;
            }
        }
    }
    for check in &mut value.binding_checks {
        if check.submitted || !contains(check.list) {
            continue;
        }
        check.submitted = true;
        value.binding_stats[check.side as usize] += 1;
        let mut reason = 0u64;
        for stamp in &check.skin.descriptors {
            if !matches(&stamp.0) {
                reason = 1;
                break;
            }
        }
        // SAFETY: the check's held texture lease.
        if reason == 0 && value.mode == 2 && !check.skin.texture.0.resource.is_null() && !unsafe { cvr_tr_validate(&check.skin.texture.0, null_mut()) } {
            reason = 5;
        }
        let (mut material, mut material_again) = ([0u8; 112], [0u8; 112]);
        let (mut modifiers, mut modifiers_again) = ([0u8; 448], [0u8; 448]);
        let at = |i: usize| check.skin.descriptors[i].0.value[4];
        if reason == 0
            && (!read_fixed(at(0), &mut material, 112)
                || !read_fixed(at(0), &mut material_again, 112)
                || !skinning::same_material(&material, &check.material)
                || !skinning::same_material(&material_again, &check.material))
        {
            reason = 2;
        }
        if reason == 0
            && (!read_fixed(at(1), &mut modifiers, 448)
                || !read_fixed(at(1), &mut modifiers_again, 448)
                || !skinning::same_modifiers(&modifiers, &check.skin.modifiers.0)
                || !skinning::same_modifiers(&modifiers_again, &check.skin.modifiers.0))
        {
            reason = 3;
        }
        if reason == 0 && !same_submitted_instances(check) {
            reason = 4;
        }
        if reason != 0 {
            value.bindings_valid = false;
            value.binding_stats[2] += 1;
            value.binding_stats[3] = reason;
            if check.skin.alpha {
                ex!(CyberpunkVR_EarlyDepthAlphaStats)[4] += 1;
                let stats = ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats);
                stats[4] += 1;
                stats[5] = 310 + reason;
            }
        }
    }
    if (!value.bindings_valid || !value.alpha_valid) && value.mode == 2 {
        value.pass.reject_shader_snapshots();
    }
}
fn same_geometry(a: &Item, b: &DrawRecord, bindings: &Bindings) -> bool {
    // SAFETY: the records and their blend factors.
    unsafe {
        crate::early_depth_checks::cvr_ned_same_geometry(
            &a.draw,
            b,
            a.bindings.stencil,
            a.bindings.topology as u32,
            &a.bindings.blend_factor,
            bindings.stencil,
            bindings.topology as u32,
            &bindings.blend_factor,
        )
    }
}

// ---- the API (include/Render/NativeEarlyDepth.hpp)

fn run_tracking() -> bool {
    CyberpunkVR_EarlyDepthRunState.load(Acquire) == 1
}
fn drawing() -> bool {
    group().id != 0 && CyberpunkVR_EarlyDepthState.load(Relaxed) == 1
}
fn material_pending() -> bool {
    MATERIAL_WATCHING.load(Relaxed)
}
fn needs_camera() -> bool {
    (run_tracking() || CyberpunkVR_EarlyDepthRequest.load(Relaxed) != 0 || CyberpunkVR_EarlyDepthState.load(Relaxed) == 1)
        && CyberpunkVR_NativeStereoProbeState.load(SeqCst) == 1
        && pairing()
        && CyberpunkVR_PreparationPairState.load(SeqCst) == 2
}
fn before_frame_boundary() {
    if run_tracking() && CyberpunkVR_EarlyDepthRunStop.load(SeqCst) != 0 {
        CyberpunkVR_PreparationRunStop.store(1, SeqCst);
    }
}
fn frame_boundary() {
    if !run_tracking() && CyberpunkVR_EarlyDepthRunRequest.load(SeqCst) == 0 {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if run_tracking() {
        poll_run(s);
        if CyberpunkVR_NativeStereoLiveFailure.load(SeqCst) != 0 {
            run_fail(35);
        }
        if CyberpunkVR_EarlyDepthRunStop.load(SeqCst) != 0 {
            CyberpunkVR_PreparationRunStop.store(1, SeqCst);
            CyberpunkVR_EarlyDepthRunStats[14].store(1, SeqCst);
        }
        if !run_active() {
            if CyberpunkVR_PreparationRunFailure.load(SeqCst) != 0 {
                run_fail(36);
            }
            if s.work.as_ref().is_some_and(|w| w.status == 1) || s.pool.iter().any(|p| p.status == 1) {
                run_fail(37);
            }
            let pending = s.work.as_ref().is_some_and(|w| w.status == 2) || s.pool.iter().any(|p| p.status == 2);
            if !pending || CyberpunkVR_EarlyDepthRunStats[15].load(SeqCst) != 0 {
                if CyberpunkVR_EarlyDepthRunStop.load(SeqCst) == 0
                    && (CyberpunkVR_EarlyDepthRunStats[0].load(SeqCst) != u64::from(s.run_budget)
                        || CyberpunkVR_EarlyDepthRunStats[1].load(SeqCst) != CyberpunkVR_EarlyDepthRunStats[2].load(SeqCst))
                {
                    run_fail(38);
                }
                s.work = None;
                s.pool.clear();
                CyberpunkVR_EarlyDepthState.store(0, SeqCst);
                publish(s);
                CyberpunkVR_EarlyDepthRunState.store(if CyberpunkVR_EarlyDepthRunStats[15].load(SeqCst) != 0 { 3 } else { 2 }, Release);
            }
        }
        return;
    }
    if !run_active() && CyberpunkVR_PreparationRunState.load(SeqCst) != 3 {
        if CyberpunkVR_PreparationRunRequest.load(SeqCst) == 0 && CyberpunkVR_PreparationRunState.load(SeqCst) == 0 {
            CyberpunkVR_PreparationRunRequest.store(CyberpunkVR_EarlyDepthRunRequest.load(SeqCst), SeqCst);
        }
        return;
    }
    let count = CyberpunkVR_EarlyDepthRunRequest.swap(0, SeqCst);
    let state = CyberpunkVR_EarlyDepthState.load(SeqCst);
    if !run_active()
        || !(30..=1200).contains(&count)
        || state == 1
        || state == 2
        || CyberpunkVR_EarlyDepthRequest.load(SeqCst) != 0
        || CyberpunkVR_EarlyDepthWholeTarget.load(SeqCst) != 0
        || CyberpunkVR_EarlyDepthRejectCommit.load(SeqCst) != 0
        || CyberpunkVR_NativeStereoProbeRequest.load(SeqCst) != 0
        || CyberpunkVR_NativeStereoProbeState.load(SeqCst) == 1
        || CyberpunkVR_ConstantCaptureEnable.load(SeqCst) != 0
        || framegen_enabled()
        || !healthy()
    {
        CyberpunkVR_EarlyDepthRunStats[15].store(39, SeqCst);
        CyberpunkVR_EarlyDepthRunState.store(3, SeqCst);
        CyberpunkVR_PreparationRunStop.store(1, SeqCst);
        return;
    }
    s.work = None;
    s.pool.clear();
    s.run_budget = count;
    s.last_run_frame = u32::MAX;
    CyberpunkVR_EarlyDepthRunCount.store(0, SeqCst);
    crate::early_depth_exports::cvr_ned_reset_run(); // the run's counters, the CPU frequency, the stop flag and the state
    for stat in &CyberpunkVR_EarlyDepthRunBindingChecks {
        stat.store(0, SeqCst);
    }
    CyberpunkVR_NativeStereoLiveFailure.store(0, SeqCst);
    publish(s);
    CyberpunkVR_EarlyDepthRunState.store(1, Release);
}
#[allow(clippy::too_many_arguments)]
fn camera(node: u32, input_valid: u32, index: u32, side: i32, view: u64, reserved: u32, shader_prediction_valid: u32, own: &[u8; CAMERA_BYTES], peer: &[u8; CAMERA_BYTES]) {
    PENDING_CAMERA.with(|c| *c.borrow_mut() = CameraData::default());
    if !needs_camera() || node != NODE || input_valid == 0 || index == u32::MAX || !(0..=1).contains(&side) {
        return;
    }
    let expected = ex!(CyberpunkVR_PreparationPairData)[if side != 0 { 3 } else { 4 }];
    if view != expected || (side == 0 && (reserved != 12 || shader_prediction_valid == 0)) {
        return;
    }
    BOUND_CAMERA.with(|c| *c.borrow_mut() = CameraData::default()); // A fresh upload must be followed by its matching native VS bind.
    let data = CameraData {
        frame: CyberpunkVR_PreparationPairMerged.load(SeqCst),
        handle: index,
        side,
        view,
        own: *own,
        peer: *peer,
        prepared: side == 0,
    };
    PENDING_CAMERA.with(|c| *c.borrow_mut() = data);
}
fn bound_camera_event(binding: u32, handle: u32, stages: u32) {
    if binding != 1 || stages & 1 == 0 {
        return;
    }
    BOUND_CAMERA.with(|c| *c.borrow_mut() = CameraData::default());
    let pending = PENDING_CAMERA.with(|c| *c.borrow());
    if stages & 1 != 0 && pending.handle == handle && needs_camera() {
        BOUND_CAMERA.with(|c| *c.borrow_mut() = pending);
        PENDING_CAMERA.with(|c| *c.borrow_mut() = CameraData::default());
    }
}
fn begin(node: u32, side: i32, renderer: u64, arguments: u64, span: u64) -> Group {
    let previous = group();
    set_group(Group::default());
    if node != NODE || !(0..=1).contains(&side) || !needs_camera() {
        return previous;
    }
    if !crate::early_depth_span::stage3(&read, arguments) {
        return previous;
    }
    let mut guard = lock();
    let s = guard.state();
    s.next_group += 1;
    let mut g = Group { id: s.next_group, frame: CyberpunkVR_PreparationPairMerged.load(SeqCst), side, ..Group::default() };
    set_group(g);
    let running = run_tracking();
    if side == 0 && (running || CyberpunkVR_EarlyDepthRequest.load(SeqCst) != 0) {
        let request = if running { ALL_DRAWS } else { CyberpunkVR_EarlyDepthRequest.swap(0, SeqCst) };
        let count = if request == ALL_DRAWS { MAX_DRAWS } else { request };
        if running {
            if !run_active() {
                set_group(Group::default());
                return previous;
            }
            // A stop may arrive after the root already admitted both eyes. Finish that pair; an unpaired native tail
            // keeps the old frame ID.
            if s.last_run_frame == g.frame && (CyberpunkVR_EarlyDepthRunStop.load(SeqCst) != 0 || CyberpunkVR_PreparationRunStop.load(SeqCst) != 0) {
                set_group(Group::default());
                return previous;
            }
            if s.last_run_frame == g.frame || CyberpunkVR_EarlyDepthRunStats[0].load(SeqCst) >= u64::from(s.run_budget) {
                run_fail(40);
                set_group(Group::default());
                return previous;
            }
            s.last_run_frame = g.frame;
            CyberpunkVR_EarlyDepthRunStats[0].fetch_add(1, SeqCst);
            if !healthy() {
                run_fail(41);
                set_group(Group::default());
                return previous;
            }
            if !acquire_run_work(s) {
                set_group(Group::default());
                return previous;
            }
        } else {
            if count == 0 || count > MAX_DRAWS || (s.work.is_some() && CyberpunkVR_EarlyDepthState.load(SeqCst) == 2) {
                fail(s, 1, E_FAIL);
                return previous;
            }
            s.work = Some(Work::new());
        }
        clear_output();
        {
            let w = s.work.as_mut().expect("work");
            w.status = 1;
            w.frame = g.frame;
            w.limit = count;
            w.main_group = g.id;
            w.items.reserve(count as usize);
        }
        publish(s);
        if let Some(code) = begin_main(s.work.as_mut().expect("work"), running, renderer, span, &mut g) {
            set_group(g);
            fail(s, code, E_FAIL);
            return previous;
        }
    }
    if side == 1 {
        if let Some(w) = s.work.as_mut() {
            if w.frame == g.frame && w.main_done && w.peer_group == 0 {
                w.peer_group = g.id;
            }
        }
    }
    set_group(g);
    previous
}
/// Begin's MAIN start: the work's settings, hints, membership and native camera (a failure's code).
fn begin_main(w: &mut Work, running: bool, renderer: u64, span: u64, g: &mut Group) -> Option<u32> {
    w.begin_qpc = qpc() as u64;
    w.combined_instance_read = CyberpunkVR_EarlyDepthInstanceReadScope.load(SeqCst) != 0;
    w.mode = if running { 2 } else { CyberpunkVR_EarlyDepthWholeTarget.load(SeqCst) };
    if w.mode > 3 {
        return Some(23);
    }
    w.whole = w.mode != 0;
    w.material_capture = CyberpunkVR_EarlyDepthMaterialCapture.load(SeqCst) != 0;
    w.alpha_reference = CyberpunkVR_EarlyDepthAlphaReference.load(SeqCst) != 0;
    w.alpha_optimized = CyberpunkVR_EarlyDepthAlphaOptimized.load(SeqCst) != 0;
    w.skinned_alpha = CyberpunkVR_EarlyDepthSkinnedAlpha.load(SeqCst) != 0;
    let failure = crate::early_depth_settings::cvr_ned_settings(
        w.mode,
        running,
        CyberpunkVR_EarlyDepthNativeCamera.load(SeqCst) != 0,
        CyberpunkVR_EarlyDepthSkinning.load(SeqCst) != 0,
        w.alpha_reference,
        w.alpha_optimized,
        w.skinned_alpha,
        w.material_capture,
    );
    if failure != 0 {
        return Some(failure);
    }
    w.material_hints = vec![[0u32; 2]; 128];
    // SAFETY: the hints export and the work's 128 slots.
    let hints = unsafe {
        crate::early_depth_settings::cvr_ned_hints(
            addr_of!(CyberpunkVR_EarlyDepthMaterialHints).cast(),
            CyberpunkVR_EarlyDepthMaterialHintCount.load(Acquire),
            w.material_hints.as_mut_ptr(),
        )
    };
    if hints > 128 {
        w.material_hints.clear();
        return Some(54);
    }
    w.material_hints.truncate(hints);
    if w.mode >= 2 && framegen_enabled() {
        return Some(31);
    }
    if !prepare_membership(w, renderer, span) {
        return Some(17);
    }
    if w.mode != 3 && CyberpunkVR_EarlyDepthNativeCamera.load(SeqCst) != 0 {
        let base = image_base();
        let stages = CyberpunkVR_EarlyDepthCameraStages.load(SeqCst);
        let camera = bound_camera();
        let ready = CyberpunkVR_StereoFamilyNativeCameras.load(SeqCst) != 0 && camera.frame == w.frame && camera.side == 0 && camera.prepared;
        let heap = (&mut w.camera_heap as *mut Option<ID3D12DescriptorHeap>).cast::<*mut c_void>();
        let bound = ready
            && if stages == 1 {
                // SAFETY: the cameras' bytes; the work's heap and upload.
                unsafe { camera_upload::cvr_ncu_begin_native(base, &camera.own, &camera.peer, heap, &mut w.native_camera) }
            } else {
                stages == 3 && {
                    // SAFETY: as above; the group's scope.
                    let begun = unsafe {
                        camera_upload::cvr_ncu_begin_stages(
                            false,
                            base,
                            camera.handle,
                            &camera.own,
                            &camera.own,
                            &camera.peer,
                            heap,
                            &mut w.native_camera,
                            stages,
                            &mut g.camera_scope,
                        )
                    };
                    set_group(*g);
                    begun
                }
            };
        if !bound {
            // SAFETY: the export's 16 values.
            unsafe { camera_upload::cvr_ncu_diagnostics(addr_of_mut!(CyberpunkVR_EarlyDepthCameraBindingInfo)) };
            let failure = ex!(CyberpunkVR_EarlyDepthFailureData);
            failure[0] = 6;
            failure[1] = u64::from(w.native_camera.reserved);
            return Some(45);
        }
        if stages == 1 {
            g.restore_camera = camera.handle;
        }
        w.camera_stage_masks = [u64::from(stages), u64::from(stages), 0, 0];
        *ex!(CyberpunkVR_EarlyDepthCameraStageMasks) = w.camera_stage_masks;
        ex!(CyberpunkVR_EarlyDepthNativeCameraStats)[0] = 1;
    }
    None
}
fn end(previous: Group) {
    let ended = group();
    set_group(previous);
    let base = image_base();
    if ended.restore_camera != 0 {
        camera_upload::cvr_ncu_end_native(base, ended.restore_camera);
    }
    let mut scope = ended.camera_scope;
    // SAFETY: the ended group's scope.
    let camera_restored = unsafe { camera_upload::cvr_ncu_end_stages(false, base, &mut scope) };
    if ended.restore_camera != 0 || scope.requested != 0 {
        let mut guard = lock();
        let s = guard.state();
        if let Some(w) = s.work.as_mut().filter(|w| w.main_group == ended.id) {
            w.camera_stage_masks[2] = if ended.restore_camera != 0 { 1 } else { u64::from(scope.restored) };
            w.camera_stage_masks[3] = u64::from(scope.preserved);
            *ex!(CyberpunkVR_EarlyDepthCameraStageMasks) = w.camera_stage_masks;
        }
        if !camera_restored {
            fail(s, 46, E_FAIL);
            return;
        }
    }
    if CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if s.work.is_none() || ended.id == 0 {
        return;
    }
    let w = s.work.as_mut().expect("work");
    if ended.id == w.main_group {
        if w.whole && !w.pass.finish_native(w.main_list, 0) {
            fail(s, 26, E_FAIL);
            return;
        }
        w.main_done = true;
        CyberpunkVR_EarlyDepthStats[12].store(1, SeqCst);
        if w.items.is_empty() && w.mode != 3 && !run_tracking() {
            fail(s, 2, E_FAIL);
        }
    }
    let w = s.work.as_mut().expect("work");
    if ended.id == w.peer_group {
        CyberpunkVR_EarlyDepthStats[13].store(1, SeqCst);
        for item in &mut w.items {
            item.matched = true;
            for i in 0..item.masks.len() {
                if item.coverage[i] != (item.masks[i] != 2) {
                    item.matched = false;
                    break;
                }
            }
            if item.matched {
                CyberpunkVR_EarlyDepthStats[3].fetch_add(1, SeqCst);
            }
        }
        let complete = !w.peer_list.is_null() && w.items.iter().all(|i| i.matched);
        if w.mode == 2
            && !w.pass.commit_native(
                complete && w.bindings_valid && w.alpha_valid && !w.items.is_empty() && CyberpunkVR_EarlyDepthRejectCommit.load(SeqCst) == 0,
            )
        {
            w.pass.abort_native(w.peer_list, 1);
            fail(s, 30, E_FAIL);
            return;
        }
        if w.whole && !w.pass.finish_native(w.peer_list, 1) {
            fail(s, 26, E_FAIL);
            return;
        }
        if !complete && !run_tracking() {
            fail(s, 3, E_FAIL);
            return;
        }
        if !w.pass.readback(w.peer_list) {
            fail(s, 4, E_FAIL);
            return;
        }
        publish(s);
        let w = s.work.as_mut().expect("work");
        for i in 0..16 {
            w.final_stats[i] = CyberpunkVR_EarlyDepthStats[i].load(SeqCst);
        }
        w.status = 2;
        w.skin_stats = *ex!(CyberpunkVR_EarlyDepthSkinningStats);
        CyberpunkVR_EarlyDepthState.store(2, Release);
    }
}

/// A draw's failure: its reason, result and whether it was already replaced.
struct DrawFailure(u32, HRESULT, bool);
fn draw(list: *mut c_void, d: &DrawRecord, instances: &[u8]) -> bool {
    if CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 || group().id == 0 {
        return false;
    }
    let _cpu = CpuScope::new(4);
    let mut guard = lock();
    let s = guard.state();
    let g = group();
    let Some(w) = s.work.as_mut() else { return false };
    if CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 || g.frame != w.frame || d.side != g.side as u32 {
        return false;
    }
    if (d.side == 0 && g.id != w.main_group) || (d.side == 1 && g.id != w.peer_group) {
        return false;
    }
    match draw_admitted(w, list, d, instances) {
        Ok(replaced) => replaced,
        Err(DrawFailure(reason, result, replaced)) => {
            let w = s.work.as_mut().expect("work");
            if w.whole {
                w.pass.abort_native(list, d.side);
            }
            fail(s, reason, result);
            replaced
        }
    }
}
fn draw_admitted(w: &mut Work, list: *mut c_void, d: &DrawRecord, instances: &[u8]) -> Result<bool, DrawFailure> {
    let mut replaced = false;
    macro_rules! fail {
        ($reason:expr) => {
            return Err(DrawFailure($reason, E_FAIL, replaced))
        };
        ($reason:expr, $hr:expr) => {
            return Err(DrawFailure($reason, $hr, replaced))
        };
    }
    macro_rules! fallback {
        () => {{
            if !w.whole {
                return Ok(false);
            }
            if !w.pass.original(list, d) {
                fail!(25);
            }
            return Ok(true);
        }};
    }
    let mut instance_lease = Buffer::default();
    // SAFETY: the host's map; the lease's reference is the caller's.
    unsafe { cvr_nsp2_instance_lease(list, &mut instance_lease.0) };
    let mut instance_state = ReadState::default();
    // SAFETY: the lease; `instance_state` is writable.
    unsafe { cvr_iab_read_state(list, &instance_lease.0, &mut instance_state) };
    let readable = matches!(instance_state.status, 1 | 2 | 8 | 9);
    CyberpunkVR_EarlyDepthStats[if d.side == 0 { 0 } else { 1 }].fetch_add(1, SeqCst);
    if w.whole {
        // SAFETY: the host's families.
        if w.mode != 3 && !crate::families_status::native_depth_compatible(d.pipeline as *mut c_void) {
            let failure = ex!(CyberpunkVR_EarlyDepthFailureData);
            failure[0] = 3;
            failure[1] = d.sequence;
            failure[2] = d.pipeline;
            // SAFETY: as above.
            failure[3] = u64::from(crate::families_status::native_depth_status(d.pipeline as *mut c_void));
            fail!(24);
        }
        let targets = DrawTargets { rt_count: d.rt_count, rt_contiguous: d.rt_contiguous, targets: d.targets, depth: d.depth, viewport: d.viewport };
        let mut held = [0usize; 4];
        // SAFETY: the draw's targets; `held` receives owned references.
        if !unsafe { cvr_scene_capture_targets(&targets, &mut held) } {
            fail!(27);
        }
        // SAFETY: the references handed out.
        let held: [Option<ID3D12Resource>; 4] = held.map(|p| (p != 0).then(|| unsafe { ID3D12Resource::from_raw(p as *mut c_void) }));
        if !w.created {
            let Some(device) = device_of(list) else { fail!(5) };
            let hr = w.pass.create(&device, d.viewport.Width as u32, d.viewport.Height as u32, w.mode);
            if hr.is_err() {
                fail!(6, hr);
            }
            w.created = true;
            if run_tracking() {
                CyberpunkVR_EarlyDepthRunStats[5].fetch_add(1, SeqCst);
            }
        }
        let side = d.side as usize;
        if w.alpha_optimized && w.native_recordings[side].0.token.is_null() {
            let base = image_base();
            // SAFETY: a plain query.
            ex!(CyberpunkVR_EarlyDepthRecording)[5] = u64::from(unsafe { cvr_native_recording_compatible() });
            let context = camera_upload::current(base);
            w.native_recordings[side] = Recording::default();
            // SAFETY: a live list; the lease is writable.
            if !unsafe { cvr_native_recording_capture(list, &context, &mut w.native_recordings[side].0) } {
                // SAFETY: a plain query.
                ex!(CyberpunkVR_EarlyDepthRecording)[4] = u64::from(unsafe { cvr_native_recording_last_failure() });
                // SAFETY: the export's eight values.
                unsafe { cvr_native_recording_last_details(addr_of_mut!(CyberpunkVR_EarlyDepthRecordingDetails)) };
                fail!(58);
            }
            ex!(CyberpunkVR_EarlyDepthRecording)[side] += 1;
        }
        let source = if readable { instance_lease.0.resource } else { null_mut() };
        if !w.pass.start_native(list, d, raw(&held[3]), source, instance_state.value as i32) {
            if texture_failure() == 40 {
                ex!(CyberpunkVR_EarlyDepthSkinningStats)[5] = 240;
                ex!(CyberpunkVR_EarlyDepthFailureData)[5] = 40;
            }
            fail!(28);
        }
        if d.side != 0 {
            w.peer_list = list;
        } else {
            w.main_list = list;
        }
    }
    if w.mode == 3 {
        if !w.pass.observe_native(list, d) {
            fail!(32);
        }
        return Ok(false);
    }
    if d.side == 0 && w.items.len() >= w.limit as usize {
        fallback!();
    }
    // SAFETY: the draw's record.
    if !unsafe { crate::early_depth_admission::cvr_ned_draw_admitted(d) } {
        CyberpunkVR_EarlyDepthStats[4].fetch_add(1, SeqCst);
        fallback!();
    }
    let stride = d.vertices[7].StrideInBytes;
    let gpu_identities;
    let mut instances = instances;
    let gpu_identity = w.gpu_instance_resource != 0 && d.instance_resource == w.gpu_instance_resource && stride == 48;
    if gpu_identity {
        let mut window = 0u64;
        // SAFETY: the draw's record; `window` is writable.
        let identities = if unsafe { crate::early_depth_admission::cvr_ned_identity_window(d, &mut window) } {
            gpu_instance_identities(d.instance_resource, window, d.instances, 48)
        } else {
            None
        };
        let Some(identities) = identities else {
            CyberpunkVR_EarlyDepthStats[4].fetch_add(1, SeqCst);
            fallback!();
        };
        gpu_identities = identities;
        instances = &gpu_identities;
    }
    if instances.is_empty() || instances.len() as u64 != u64::from(d.instances) * u64::from(stride) {
        CyberpunkVR_EarlyDepthStats[4].fetch_add(1, SeqCst);
        fallback!();
    }
    let family = Family::bound(list);
    let mut bindings = Bindings::default();
    let mut material = [0u8; 112];
    if let Some(f) = family.as_ref().filter(|f| {
        (CyberpunkVR_EarlyDepthSkinning.load(SeqCst) != 0 || CyberpunkVR_EarlyDepthMaterialCapture.load(SeqCst) != 0) && f.view.pixel_shader && f.view.paired_root
    }) {
        for location in f.bindings() {
            let binding = &location.binding;
            if binding.kind != D3D12_DESCRIPTOR_RANGE_TYPE_CBV || binding.visibility != D3D12_SHADER_VISIBILITY_PIXEL || binding.source_register != 4 {
                continue;
            }
            let mut lease = Material::default();
            let status = material_observe::resolve_at_draw(list, d.root, location.source_parameter, location.source_offset, &mut lease.0);
            ex!(CyberpunkVR_EarlyDepthMaterialBindings)[status as usize] += 1;
            if CyberpunkVR_EarlyDepthMaterialCapture.load(SeqCst) != 0
                && w.mode == 1
                && (f.view.asset == 39 || f.view.asset == 43)
                && status == Status::Registered
                && !capture_material(w, list, d, f, lease)
            {
                fail!(51);
            }
            break;
        }
    }
    let alpha_modes = family.as_ref().map_or(0, |f| {
        crate::early_depth_admission::cvr_ned_alpha_modes(
            f.view.alpha_depth_contract,
            f.view.skinning_read_contract,
            f.view.native_camera_pipeline,
            f.view.native_camera_stages,
            w.mode,
            w.alpha_reference,
            w.alpha_optimized,
            w.skinned_alpha,
            CyberpunkVR_EarlyDepthSkinning.load(SeqCst) != 0,
            w.native_camera.gpu_address != 0,
        )
    });
    let (alpha_reference, alpha_optimized) = (alpha_modes & 1 != 0, alpha_modes & 2 != 0);
    let Some(family) = family else {
        CyberpunkVR_EarlyDepthStats[4].fetch_add(1, SeqCst);
        fallback!();
    };
    let f = family.view;
    // SAFETY: the list's scene state; `bindings` is writable.
    if (f.pixel_shader && !alpha_reference && !alpha_optimized) || !f.instance_ordinal_independent || !unsafe { cvr_scene_snapshot(list as usize, &mut bindings) } {
        CyberpunkVR_EarlyDepthStats[4].fetch_add(1, SeqCst);
        fallback!();
    }
    let camera = bound_camera();
    if camera.frame != w.frame || camera.side != d.side as i32 || camera.handle == 0 {
        CyberpunkVR_EarlyDepthStats[5].fetch_add(1, SeqCst);
        fallback!();
    }
    let mut skin: Option<SkinInputs> = None;
    let mut alpha: Option<AlphaInputs> = None;
    let skinned_alpha = (alpha_reference || alpha_optimized) && f.alpha_depth_contract == 2;
    let reject_skinned = |reason: u64| {
        if skinned_alpha {
            let stats = ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats);
            stats[3] += 1;
            stats[5] = reason;
        }
    };
    if (alpha_reference || alpha_optimized) && d.side == 0 {
        ex!(CyberpunkVR_EarlyDepthAlphaStats)[0] += 1;
        if skinned_alpha {
            ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats)[0] += 1;
        }
    }
    let mut skin_instance_offset = 0u64;
    if CyberpunkVR_EarlyDepthSkinning.load(SeqCst) != 0 && w.native_camera.gpu_address != 0 && f.native_camera_pipeline && f.skinning_read_contract != 0 {
        if d.side == 0 {
            ex!(CyberpunkVR_EarlyDepthSkinningStats)[0] += 1;
        }
        let inputs = skin.insert(SkinInputs::default());
        if !skin_material(list, d, &family, &mut material, inputs, w.mode == 2) {
            let stats = ex!(CyberpunkVR_EarlyDepthSkinningStats);
            stats[3] += 1;
            CyberpunkVR_EarlyDepthStats[6].fetch_add(1, SeqCst);
            reject_skinned(100 + stats[5]);
            fallback!();
        }
        let lease = &instance_lease.0;
        // SAFETY: the draw's record; the offset is writable.
        if !unsafe {
            crate::early_depth_skin::cvr_ned_skin_window(
                d,
                lease.resource as u64,
                lease.heap_type,
                lease.gpu,
                lease.bytes,
                instances.len() as u64,
                &mut skin_instance_offset,
            )
        } {
            let stats = ex!(CyberpunkVR_EarlyDepthSkinningStats);
            stats[3] += 1;
            stats[5] = 7;
            reject_skinned(107);
            fallback!();
        }
    }
    if alpha_reference || alpha_optimized {
        let skin_material_bytes = material;
        let inputs = alpha.insert(AlphaInputs::default());
        if !alpha_material(list, d, &family, &mut material, inputs, alpha_optimized, &w.material_hints) {
            let stats = ex!(CyberpunkVR_EarlyDepthAlphaStats);
            stats[3] += 1;
            reject_skinned(200 + stats[5]);
            fallback!();
        }
        if skinned_alpha && (skin.is_none() || !skinning::same_material(&skin_material_bytes, &material)) {
            ex!(CyberpunkVR_EarlyDepthAlphaStats)[3] += 1;
            reject_skinned(210);
            fallback!();
        }
    } else if skin.is_none() && !material_inputs(list, d, &family, &mut material) {
        CyberpunkVR_EarlyDepthStats[6].fetch_add(1, SeqCst);
        fallback!();
    }
    if w.instance_rows.len() >= MAX_INSTANCE_ROWS || instances.len() > MAX_INSTANCE_BYTES - w.instance_blob.len() {
        fail!(16);
    }
    // Domains 3/4 publish GPU address identities, not CPU instance contents.
    w.instance_rows.push(InstanceRow {
        sequence: d.sequence,
        side: if gpu_identity { d.side + 3 } else { d.side },
        offset: w.instance_blob.len() as u32,
        bytes: instances.len() as u32,
        stride,
        material,
    });
    w.instance_blob.extend_from_slice(instances);
    if d.side == 0 {
        if !camera.prepared {
            CyberpunkVR_EarlyDepthStats[5].fetch_add(1, SeqCst);
            fallback!();
        }
        let native_camera = w.native_camera.gpu_address != 0 && f.native_camera_pipeline;
        if native_camera {
            let mut bound = false;
            for binding in family.bindings() {
                if binding.binding.source_register == 1 {
                    bound = resolve_cbv(list, d.root, binding.source_parameter, binding.source_offset)
                        .is_some_and(|value| value.address == w.native_camera.gpu_address && value.bytes == 2048);
                }
            }
            if !bound {
                fail!(46);
            }
            if !w.camera_checked {
                let mut expected = [0u8; 2048];
                let (mut actual, mut again) = ([0u8; 2048], [0u8; 2048]);
                expected[..CAMERA_BYTES].copy_from_slice(&camera.own);
                expected[1024..1024 + CAMERA_BYTES].copy_from_slice(&camera.peer);
                if !read_upload(w.native_camera.gpu_address, actual.as_mut_ptr().cast(), actual.len(), false)
                    || !read_upload(w.native_camera.gpu_address, again.as_mut_ptr().cast(), again.len(), false)
                    || actual != expected
                    || again != expected
                {
                    fail!(47);
                }
                w.camera_checked = true;
                ex!(CyberpunkVR_EarlyDepthNativeCameraStats)[2] = 1;
            }
        }
        let mut planning = CpuScope::new(1);
        let mut geometry = [0u8; 64];
        // SAFETY: `geometry` is writable.
        if !unsafe { cvr_gp_current_geometry(w.renderer, d.instances, stride, &mut geometry) } {
            if run_tracking() {
                CyberpunkVR_EarlyDepthRunDeferred[0].fetch_add(1, SeqCst);
                fallback!();
            }
            ex!(CyberpunkVR_EarlyDepthMaskStats)[7] += 1;
            fail!(18);
        }
        let mut membership_reason = 1u32;
        let mut masks = vec![0u8; if (stride == 48 || stride == 64) && instances.len() <= 16384 { instances.len() / stride as usize } else { 0 }];
        let membership = if gpu_identity { &mut w.gpu_membership } else { &mut w.membership };
        if !membership.plan(&geometry, instances, stride, &mut masks, &mut membership_reason) {
            if run_tracking() {
                CyberpunkVR_EarlyDepthRunDeferred[0].fetch_add(1, SeqCst);
                fallback!();
            }
            let failure = ex!(CyberpunkVR_EarlyDepthFailureData);
            failure[0] = 2;
            failure[1] = d.sequence;
            failure[2] = u64::from(membership_reason);
            fail!(19);
        }
        let mut extras = Vec::new();
        if !gpu_identity {
            match w.membership.extras(&geometry, stride) {
                Some(bytes) => extras = bytes,
                None => {
                    if !run_tracking() {
                        fail!(22);
                    }
                    CyberpunkVR_EarlyDepthRunDeferred[1].fetch_add(1, SeqCst);
                    extras.clear();
                }
            }
        }
        planning.end();
        let _emit = CpuScope::new(2);
        let Some(device) = device_of(list) else { fail!(5) };
        if !w.created {
            let hr = w.pass.create(&device, d.viewport.Width as u32, d.viewport.Height as u32, 0);
            if hr.is_err() {
                fail!(6, hr);
            }
            w.created = true;
        }
        let mut pairs = Vec::new();
        if !native_camera {
            for entry in family.bindings() {
                let eyes: [&[u8]; 2] = if entry.binding.source_register == 1 { [&camera.peer, &camera.own] } else { [&material, &material] };
                pairs.push(PairSpec { binding: entry.binding, eyes: [eyes[0].as_ptr(), eyes[1].as_ptr()], bytes: [eyes[0].len(), eyes[1].len()] });
            }
        }
        let mut bank = DrawBank(null_mut());
        if !run_tracking() && !native_camera {
            // SAFETY: the device, the family's plan and the pairs' constants (copied by the bank).
            let hr = bank_create(device.as_raw(), &family.view, &pairs, &mut bank.0);
            if hr.is_err() {
                CyberpunkVR_EarlyDepthStats[7].fetch_add(1, SeqCst);
                fail!(7, hr);
            }
        }
        let mut item = Item {
            draw: *d,
            bindings,
            instances: instances.to_vec(),
            material,
            peer: camera.peer,
            coverage: vec![false; d.instances as usize],
            masks: masks.clone(),
            matched: false,
            gpu_identity,
            skin,
            alpha,
        };
        let extra_count = extras.len() / stride as usize;
        if extra_count != 0 {
            item.instances.extend_from_slice(&extras);
            item.coverage.resize(d.instances as usize + extra_count, false);
            item.masks.extend(core::iter::repeat(1u8).take(extra_count));
            if w.instance_rows.len() >= MAX_INSTANCE_ROWS || extras.len() > MAX_INSTANCE_BYTES - w.instance_blob.len() {
                fail!(16);
            }
            w.instance_rows.push(InstanceRow {
                sequence: d.sequence,
                side: 2,
                offset: w.instance_blob.len() as u32,
                bytes: extras.len() as u32,
                stride,
                material,
            });
            w.instance_blob.extend_from_slice(&extras);
        }
        if gpu_identity && w.mode == 2 {
            let address = d.vertices[7].BufferLocation.wrapping_add(u64::from(d.first_instance) * 48);
            let lease = &instance_lease.0;
            if !readable
                || lease.resource as u64 != d.instance_resource
                || address < lease.gpu
                || !w.pass.capture_instances(list, lease.resource, address - lease.gpu, instances.len() as u32, instance_state.value as i32, w.combined_instance_read)
            {
                fail!(43);
            }
        }
        if let Some(sk) = &item.skin {
            let Some(bones) = sk.bones.resource.as_ref() else { fail!(12) };
            // SAFETY: a live resource.
            let width = unsafe { bones.GetDesc() }.Width;
            let Some(ranges) = skinning::bone_ranges(f.skinning_read_contract, &item.instances, &sk.bones.view, width) else {
                let stats = ex!(CyberpunkVR_EarlyDepthSkinningStats);
                stats[4] += 1;
                stats[5] = 6;
                reject_skinned(106);
                fallback!();
            };
            if skinned_alpha && d.sequence > u64::from(u32::MAX) {
                reject_skinned(211);
                fallback!();
            }
            if w.mode == 2
                && !w.pass.capture_skinning(list, bones.as_raw(), &ranges, &sk.texture, if skinned_alpha { d.sequence as u32 } else { 0 })
            {
                let stats = ex!(CyberpunkVR_EarlyDepthSkinningStats);
                stats[4] += 1;
                stats[5] = 200 + u64::from(texture_failure());
                reject_skinned(100 + stats[5]);
                if texture_failure() == 40 {
                    fail!(50);
                }
                fallback!();
            }
        }
        if native_camera {
            if alpha_optimized {
                let a = item.alpha.as_ref().expect("alpha inputs");
                if d.sequence > u64::from(u32::MAX)
                    || !w.pass.capture_alpha(list, &a.material, &a.texture, &a.mips, a.expected_index, d.sequence as u32)
                {
                    let stats = ex!(CyberpunkVR_EarlyDepthAlphaStats);
                    stats[3] += 1;
                    stats[5] = if d.sequence > u64::from(u32::MAX) { 10 } else { w.pass.alpha_proof_counts()[5] };
                    reject_skinned(200 + stats[5]);
                    if stats[5] == 1040 || stats[5] == 2040 {
                        fail!(57);
                    }
                    fallback!();
                }
            }
            if !w.pass.main(list, d, &family, bank.0, &masks, &extras, 0, true, alpha_reference, alpha_optimized) {
                fail!(8);
            }
            w.native_camera_draws += 1;
            ex!(CyberpunkVR_EarlyDepthNativeCameraStats)[1] += 1;
            if run_tracking() {
                CyberpunkVR_EarlyDepthRunStats[8].fetch_add(1, SeqCst);
            }
        } else if run_tracking() {
            let before = w.pass.arena_count();
            let rendered = w.pass.main_constants(list, d, &family, &pairs, &masks, &extras);
            CyberpunkVR_EarlyDepthRunArenas[0].fetch_add(u64::from(w.pass.arena_count().wrapping_sub(before)), SeqCst);
            if !rendered {
                fail!(8);
            }
            CyberpunkVR_EarlyDepthRunStats[8].fetch_add(1, SeqCst);
        } else if !w.pass.main(list, d, &family, bank.0, &masks, &extras, 0, false, false, false) {
            fail!(8);
        }
        replaced = w.whole;
        // The item's rows and mask counts.
        // SAFETY: the masks' bytes; the exports under the lock.
        unsafe { crate::early_depth_exports::cvr_ned_main_masks(w.items.len(), d.sequence, d.instances, extra_count as u32, masks.as_ptr(), masks.len()) };
        if let Some(sk) = &item.skin {
            w.binding_checks.push(BindingCheck {
                list,
                side: 0,
                skin: sk.clone(),
                material,
                instance_source: instance_lease.clone(),
                instance_offset: skin_instance_offset,
                instances: instances.to_vec(),
                submitted: false,
            });
        }
        if let Some(a) = &item.alpha {
            w.alpha_checks.push(AlphaCheck { list, inputs: a.clone(), vertex_material: material, submitted: false });
            ex!(CyberpunkVR_EarlyDepthAlphaStats)[1] += 1;
            if a.skinned {
                ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats)[1] += 1;
            }
        }
        let skinned = item.skin.is_some();
        w.items.push(item);
        CyberpunkVR_EarlyDepthStats[2].fetch_add(1, SeqCst);
        if skinned {
            ex!(CyberpunkVR_EarlyDepthSkinningStats)[1] += 1;
        }
        ex!(CyberpunkVR_EarlyDepthGpuInstances)[0] += u64::from(gpu_identity);
    } else {
        let _peer_cpu = CpuScope::new(3);
        if !w.pass.main_submitted() {
            fail!(9);
        }
        if !w.peer_list.is_null() && w.peer_list != list {
            fail!(21);
        }
        w.peer_list = list;
        let mut ids = Vec::new();
        for (i, item) in w.items.iter().enumerate() {
            if item.gpu_identity == gpu_identity
                && same_geometry(item, d, &bindings)
                && (if skin.is_some() { skinning::same_material(&item.material, &material) } else { item.material == material })
                && item.peer == camera.own
                && same_skin(&item.skin, &skin)
                && same_alpha(&item.alpha, &alpha)
            {
                ids.push(i);
            }
        }
        let sources: Vec<Source> = ids
            .iter()
            .map(|&i| {
                let item = &w.items[i];
                Source { bytes: &item.instances, covered: Some(item.coverage.clone()), view_masks: &item.masks }
            })
            .collect();
        let Some(plan) = instance_coverage::plan(&sources, instances, stride) else { fail!(14) };
        if plan.matches.is_empty() {
            CyberpunkVR_EarlyDepthStats[8].fetch_add(1, SeqCst);
            fallback!();
        }
        if let Some(a) = &alpha {
            w.alpha_checks.push(AlphaCheck { list, inputs: a.clone(), vertex_material: material, submitted: false });
            ex!(CyberpunkVR_EarlyDepthAlphaStats)[2] += 1;
            if a.skinned {
                ex!(CyberpunkVR_EarlyDepthSkinnedAlphaStats)[2] += 1;
            }
        }
        ex!(CyberpunkVR_EarlyDepthGpuInstances)[1] += u64::from(gpu_identity);
        if let Some(sk) = skin.as_mut() {
            let matched = &w.items[ids[plan.matches[0].source as usize]];
            sk.texture = matched.skin.as_ref().map(|m| m.texture.clone()).unwrap_or_default();
            ex!(CyberpunkVR_EarlyDepthSkinningStats)[2] += 1;
            w.binding_checks.push(BindingCheck {
                list,
                side: 1,
                skin: sk.clone(),
                material,
                instance_source: instance_lease.clone(),
                instance_offset: skin_instance_offset,
                instances: instances.to_vec(),
                submitted: false,
            });
        }
        for m in &plan.matches {
            if w.items[ids[m.source as usize]].masks[m.instance as usize] == 2 {
                fail!(20);
            }
        }
        if w.whole {
            let mut remaining = vec![InstanceRun::default(); plan.runs.len() + 1]; // the instances no run covers
            // SAFETY: the runs and `remaining` (one more than the runs).
            let count = unsafe {
                crate::early_depth_run::cvr_ned_remaining(plan.runs.as_ptr().cast(), plan.runs.len(), d.instances, remaining.as_mut_ptr().cast())
            };
            remaining.truncate(count);
            if !w.pass.peer_remainder(list, d, &remaining) {
                fail!(11);
            }
            replaced = true;
            if w.mode != 2 {
                CyberpunkVR_EarlyDepthStats[10].fetch_add(1, SeqCst);
            }
        } else {
            for run in &plan.runs {
                let mut reference = *d;
                reference.first_instance += run.first;
                reference.instances = run.count;
                if !w.pass.peer(list, &reference) {
                    fail!(11);
                }
                CyberpunkVR_EarlyDepthStats[10].fetch_add(1, SeqCst);
            }
        }
        let Some(covered) = instance_coverage::commit(&sources, &plan.matches) else { fail!(15) };
        drop(sources);
        for (k, coverage) in covered.into_iter().enumerate() {
            w.items[ids[k]].coverage = coverage;
        }
        for m in &plan.matches {
            let at = ids[m.source as usize];
            let rows = ex!(CyberpunkVR_EarlyDepthSources);
            if rows[at][1] == 0 {
                rows[at][1] = d.sequence;
            }
            if m.instance < w.items[at].draw.instances {
                ex!(CyberpunkVR_EarlyDepthCoverage)[at][0] += 1;
            } else {
                ex!(CyberpunkVR_EarlyDepthExtraCoverage)[at][0] += 1;
            }
        }
    }
    Ok(replaced)
}
fn unsupported(list: *mut c_void, reason: u32) {
    if internal() || CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 || group().id == 0 {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    let g = group();
    let Some(w) = s.work.as_mut() else { return };
    if !w.whole || g.frame != w.frame || (g.id != w.main_group && g.id != w.peer_group) {
        return;
    }
    w.pass.abort_native(list, g.side as u32);
    let failure = ex!(CyberpunkVR_EarlyDepthFailureData);
    failure[0] = 4;
    failure[1] = u64::from(reason);
    fail(s, 29, E_FAIL);
}
/// InstanceWrite (`after` null) and InstanceAlias: the material copies a write or alias reaches are invalidated; the
/// optimized pass's peer list loses its instance or shader sources.
fn instance_change(list: *mut c_void, resource: *mut c_void, alias: Option<(*mut c_void, *mut c_void)>) {
    if internal() {
        return;
    }
    let g = group();
    let legacy = CyberpunkVR_EarlyDepthState.load(SeqCst) == 1 && g.side == 1 && g.id != 0;
    if !material_pending() && !legacy {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if let Some(w) = s.work.as_mut().filter(|_| material_pending()) {
        for i in 0..w.material_copies.len() {
            let copy = &mut *w.material_copies[i];
            if copy.state.stage == SEALED {
                continue;
            }
            let source = copy.lease.0.source.resource;
            let affected = match alias {
                // SAFETY: plain pointers compared by the receipts.
                Some((before, after)) => unsafe { cvr_tr_alias_affects_resources(source, before, after) },
                None => resource.is_null() || resource == source,
            };
            if affected {
                copy.state.invalidated();
                if w.material_rows[i].failure == 0 {
                    w.material_rows[i].failure = if alias.is_some() { 105 } else { 101 };
                }
            }
        }
    }
    let Some(w) = s.work.as_mut() else { return };
    if !legacy || w.mode != 2 || w.peer_list != list {
        return;
    }
    let sources = match alias {
        Some(_) => !w.pass.instance_source().is_null() || w.pass.shader_source(null_mut()),
        None => {
            let instance = w.pass.instance_source();
            (!instance.is_null() && (resource.is_null() || resource == instance)) || w.pass.shader_source(resource)
        }
    };
    if sources {
        w.pass.abort_native(list, 1);
        fail(s, 44, E_FAIL);
    }
}
fn instance_barrier(list: *mut c_void, resource: *mut c_void) {
    let g = group();
    if internal() || CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 || g.side != 0 || g.id == 0 {
        return;
    }
    let mut guard = lock();
    if let Some(w) = guard.state().work.as_mut() {
        if w.mode == 2 && (resource.is_null() || resource == w.pass.instance_source()) {
            w.pass.release_instance_read(list);
        }
    }
}
fn closing(list: *mut c_void) {
    if internal() || CyberpunkVR_EarlyDepthState.load(SeqCst) != 1 {
        return;
    }
    let mut guard = lock();
    if let Some(w) = guard.state().work.as_mut() {
        w.pass.release_instance_read(list);
    }
}
fn reset(list: *mut c_void) {
    let state = CyberpunkVR_EarlyDepthState.load(SeqCst);
    if !run_tracking() && state != 1 && state != 2 {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if let Some(w) = s.work.as_mut() {
        for copy in &mut w.material_copies {
            if copy.list == list {
                copy.state.reset_before_submit();
            }
        }
        w.pass.reset(list);
        if w.pass.failed() {
            fail(s, 13, E_FAIL);
        }
    }
    for value in &s.pool {
        value.pass.reset(list);
        if value.pass.failed() {
            run_fail(13);
        }
    }
}
fn before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if CyberpunkVR_EarlyDepthState.load(SeqCst) == 0 && !run_tracking() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if let Some(w) = s.work.as_mut() {
        submit_materials(w, queue, count, lists, false);
        validate_submission(w, queue, count, lists);
        w.pass.before_submit(queue, count, lists);
        publish(s);
    }
    for value in &mut s.pool {
        validate_submission(value, queue, count, lists);
        value.pass.before_submit(queue, count, lists);
    }
}
fn submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if CyberpunkVR_EarlyDepthState.load(SeqCst) == 0 && !run_tracking() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if let Some(w) = s.work.as_mut() {
        w.pass.submitted(queue, count, lists);
        submit_materials(w, queue, count, lists, true);
    }
    for value in &s.pool {
        value.pass.submitted(queue, count, lists);
    }
}
fn poll() {
    if run_tracking() || CyberpunkVR_EarlyDepthState.load(SeqCst) != 2 {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    let Some(w) = s.work.as_mut() else { return };
    if !w.alpha_valid {
        fail(s, 56, E_FAIL);
        return;
    }
    if w.material_capture {
        let mut covered = [false; 2];
        for row in &w.material_rows {
            if row.side < 2 {
                covered[row.side as usize] = true;
            }
        }
        if !covered[0] || !covered[1] {
            fail(s, 53, E_FAIL);
            return;
        }
    }
    if w.material_copies.iter().any(|c| c.state.pending != 0) {
        return;
    }
    if w.material_copies.iter().any(|c| c.state.replayed) {
        fail(s, 52, E_FAIL);
        return;
    }
    let mut result: Comparison = zeroed();
    let mut hr = w.pass.read(&mut result);
    if hr == S_FALSE {
        return;
    }
    if hr.is_ok() {
        hr = seal_materials(w);
        if hr == S_FALSE {
            return;
        }
    }
    // SAFETY: the exports, under the lock.
    unsafe {
        CyberpunkVR_EarlyDepthResult = hr.0;
        *addr_of_mut!(CyberpunkVR_EarlyDepthComparison) = result;
    }
    publish(s);
    if let Some(w) = s.work.as_mut() {
        w.status = if hr.is_ok() { 3 } else { 4 };
    }
    CyberpunkVR_EarlyDepthState.store(if hr.is_ok() { 3 } else { 4 }, Release);
}

// ---- the C ABI (the .cpp's API forwards)

#[no_mangle]
pub extern "C" fn cvr_ned_run_tracking() -> bool {
    run_tracking()
}
#[no_mangle]
pub extern "C" fn cvr_ned_drawing() -> bool {
    drawing()
}
#[no_mangle]
pub extern "C" fn cvr_ned_material_pending() -> bool {
    material_pending()
}
#[no_mangle]
pub extern "C" fn cvr_ned_needs_camera() -> bool {
    needs_camera()
}
#[no_mangle]
pub extern "C" fn cvr_ned_before_frame_boundary() {
    before_frame_boundary();
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_frame_boundary() {
    frame_boundary();
}
/// Camera: the trace row's fields (`own` its camera, `peer` its shader prediction; 848 bytes each).
///
/// # Safety
/// The cameras hold 848 bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_ned_camera(
    node: u32,
    input_valid: u32,
    index: u32,
    side: i32,
    view: u64,
    reserved: u32,
    shader_prediction_valid: u32,
    own: *const [u8; CAMERA_BYTES],
    peer: *const [u8; CAMERA_BYTES],
) {
    // SAFETY: as documented.
    unsafe { camera(node, input_valid, index, side, view, reserved, shader_prediction_valid, &*own, &*peer) };
}
#[no_mangle]
pub extern "C" fn cvr_ned_bound_camera(binding: u32, handle: u32, stages: u32) {
    bound_camera_event(binding, handle, stages);
}
/// Begin: the previous group into `out`.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_begin(node: u32, side: i32, renderer: u64, arguments: u64, span: u64, out: *mut Group) {
    let previous = begin(node, side, renderer, arguments, span);
    // SAFETY: as documented.
    unsafe { *out = previous };
}
/// # Safety
/// `previous` is a group Begin returned.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_end(previous: *const Group) {
    // SAFETY: as documented.
    end(unsafe { *previous });
}
/// # Safety
/// `draw` is the probe's record; `instances` holds `bytes`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_draw(list: *mut c_void, draw_record: *const DrawRecord, instances: *const u8, bytes: usize) -> bool {
    // SAFETY: as documented.
    draw(list, unsafe { &*draw_record }, slice(instances, bytes))
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_unsupported(list: *mut c_void, reason: u32) {
    unsupported(list, reason);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_instance_write(list: *mut c_void, resource: *mut c_void) {
    instance_change(list, resource, None);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_instance_alias(list: *mut c_void, before: *mut c_void, after: *mut c_void) {
    instance_change(list, null_mut(), Some((before, after)));
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_instance_barrier(list: *mut c_void, resource: *mut c_void) {
    instance_barrier(list, resource);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_closing(list: *mut c_void) {
    closing(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_reset(list: *mut c_void) {
    reset(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    before_submit(queue, count, lists);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    submitted(queue, count, lists);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ned_poll() {
    poll();
}
