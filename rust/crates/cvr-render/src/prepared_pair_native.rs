//! The prepared pair's orchestration (formerly src/Render/NativePreparedPair.cpp): a batch of captured native sources
//! (two eyes' draws of one pass, each with its owned geometry, private constant and material copies, reference tables
//! and pass-entry baseline) is paired by geometry, state and family, given reference and paired binding banks, a
//! stereo mesh plan, the input / membership / mesh comparisons and the VRS maps, recorded once (on the native list at
//! an admitted draw, or on an owned list submitted right after the source prefix), and its query, image and exit
//! readbacks compared once its fence completes. A current batch may also export the paired output into the native
//! targets.
//!
//! The sources stay the .cpp's values (prepared_pair::Source, built by the retained geometry): the .cpp moves them
//! into a holder this module owns (`aux`, freed through the host) and lends one `SourceView` per source, raw pointers
//! into the holder. What stays C++ goes through `Host`: the stereo families, the immutable texture leases and their
//! residency, the material and T5 copies' checks, the membership banks, the export's target access, the queue's
//! original ExecuteCommandLists and the registered constants' queue order lock. The binding banks, geometry
//! commands, geometry chains, comparisons, VRS maps, output copy and command resources are Rust objects called here.

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null, null_mut};
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use cvr_gpu::scene_state::{GeometryTargetsRaw, Rate};

use crate::binding_bank::{DrawInput, Pair as BankPairRow, PlanView, SourceInput, Table, Value};
use crate::camera_binding::Context;
use crate::descriptor_audit::DescriptorStamp;
use crate::geometry_commands::{DrawRecord, Lease as GeometryLease, Resources};
use crate::native_exit::NativeExitPlane;
use crate::native_recording::Lease as RecordingLease;
use crate::paired_root::Location;
use crate::prepared_pair_exports::Step;
use crate::prepared_pair_images::Plane;
use crate::prepared_pair_record::{CompareRange, MeshPair, MeshSource};
use crate::prepared_pair_state::*;

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool;
    fn cvr_gpu_prepare_command_resources(count: u32, lists: *const *mut c_void) -> *mut c_void;
    fn cvr_gpu_clone_submission(submission: *const c_void) -> *mut c_void;
    fn cvr_gpu_free_submission(submission: *mut c_void);
    fn cvr_gpu_submit_command_resources(queue: *mut c_void, submission: *mut c_void);
    fn cvr_gpu_reset_command_resources(list: *mut c_void);
    fn cvr_native_recording_capture(list: *mut c_void, context: *const Context, out: *mut RecordingLease) -> bool;
    fn cvr_native_recording_validate_submission(lease: *const RecordingLease, queue: *mut c_void) -> bool;
    fn cvr_scene_capture_source_bindings(list: usize, out: *mut cvr_gpu::scene_state::Bindings) -> bool;
    fn cvr_scene_bindings_restore(b: *const cvr_gpu::scene_state::Bindings, list: *mut c_void);
    fn cvr_bank_create_pass(device: *mut c_void, draws: *const DrawInput, count: usize, out: *mut *mut c_void) -> HRESULT;
    fn cvr_bank_create_source_pass(device: *mut c_void, inputs: *const SourceInput, count: usize, failure: *mut [u64; 3], out: *mut *mut c_void) -> HRESULT;
    fn cvr_bank_capture_eye(storage: *mut c_void, list: *mut c_void, eye: u32, keep: Retain, has: Retain) -> bool;
    fn cvr_bank_bind(storage: *mut c_void, list: *mut c_void, eye: u32, index: u32, has: Retain) -> bool;
    fn cvr_bank_bind_source(storage: *mut c_void, list: *mut c_void, index: u32, keep: Retain) -> bool;
    fn cvr_bank_counts(storage: *mut c_void, out: *mut crate::binding_bank::Counts);
    fn cvr_sgc_create_sources(device: *mut c_void, inputs: *const SgcSource, count: usize, describe: Describe, out: *mut *mut c_void) -> HRESULT;
    fn cvr_sgc_record(storage: *mut c_void, list: *mut c_void, index: u32, sources: bool, keep: Retain, describe: Describe) -> bool;
    fn cvr_geometry_chain_create(device: *mut c_void, width: u32, height: u32, out: *mut *mut c_void) -> HRESULT;
    fn cvr_geometry_chain_release(storage: *mut c_void);
    fn cvr_geometry_chain_bind(storage: *mut c_void, list: *mut c_void, mask: u32, colors: u32) -> bool;
    fn cvr_geometry_chain_seed_private_eye(storage: *mut c_void, list: *mut c_void, eye: u32, raw: *const GeometryTargetsRaw) -> bool;
    fn cvr_geometry_chain_resource(storage: *mut c_void, slot: u32) -> *mut c_void;
    fn cvr_geometry_compare_new() -> *mut c_void;
    fn cvr_geometry_compare_free(rust: *mut c_void);
    fn cvr_geometry_compare_create(rust: *const c_void, device: *mut c_void, pairs: u32, ranges: *const CompareRange, count: usize, gate: *mut c_void, gate_offset: u64) -> HRESULT;
    fn cvr_geometry_compare_record(rust: *const c_void, list: *mut c_void) -> bool;
    fn cvr_geometry_compare_info(rust: *const c_void, predicates: *mut *mut c_void, counts: *mut u64);
    fn cvr_finer_vrs_new() -> *mut c_void;
    fn cvr_finer_vrs_free(rust: *mut c_void);
    fn cvr_finer_vrs_create(rust: *const c_void, device: *mut c_void, images: *const *mut c_void, count: usize) -> HRESULT;
    fn cvr_finer_vrs_record(rust: *const c_void, list: *mut c_void) -> bool;
    fn cvr_finer_vrs_info(rust: *const c_void, image: *mut *mut c_void, counts: *mut u64);
    fn cvr_finer_vrs_read(rust: *const c_void, pixels: *mut u8, length: usize, invalid: *mut u32) -> HRESULT;
    fn cvr_vrs_snapshot_info(rust: *const c_void, frozen: *mut *mut c_void, width: *mut u32, height: *mut u32);
    fn cvr_vrs_snapshot_read_eye(rust: *const c_void, eye: u32, out: *mut u8, length: usize) -> HRESULT;
    fn cvr_output_copy_new() -> *mut c_void;
    fn cvr_output_copy_free(rust: *mut c_void);
    fn cvr_output_copy_create(rust: *const c_void, device: *mut c_void, arrays: *const [*mut c_void; 4], targets: *const [OutputTarget; 8], predicate: *mut c_void, offset: u64) -> HRESULT;
    fn cvr_output_copy_record(rust: *const c_void, list: *mut c_void) -> bool;
    fn cvr_output_copy_info(rust: *const c_void, readback: *mut *mut c_void, bytes: *mut u64, recorded: *mut bool, planes: *mut [crate::prepared_pair_record::OutputPlane; 10]);
    fn cvr_mlm_create(device: *mut c_void, sources: *const MlmSource, count: usize, gate: *mut c_void, gate_offset: u64, out: *mut *mut c_void) -> HRESULT;
    fn cvr_mlm_record(storage: *mut c_void, list: *mut c_void, keep: Retain) -> bool;
    fn cvr_mlm_info(storage: *mut c_void, out: *mut MlmInfo);
}
pub type Retain = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> bool;
pub type Describe = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void);

// ---- the lent sources (prepared_pair::Source, kept alive by the holder)

/// scene_state::DrawState's values.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawStateView {
    pub pipeline: *mut c_void,
    pub rate_image: *mut c_void,
    pub rate: Rate,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
}
/// source_cbv_snapshot::PrivateCbv (its COM pointers the holder's).
#[repr(C)]
pub struct PrivateCbv {
    pub parameter: u32,
    pub offset: u32,
    pub reflected_bytes: u32,
    pub padded_bytes: u32,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub resource: *mut c_void,
    pub owner: *mut c_void,
}
/// source_cbv_snapshot::PreparedCopies.
#[repr(C)]
pub struct PreparedCopies {
    pub owner: *mut c_void,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub copied_bytes: u64,
    pub count: u32,
}
/// prepared_pair::Material.
#[repr(C)]
pub struct MaterialView {
    pub parameter: u32,
    pub offset: u32,
    pub kind: u32,
    pub has_buffer: bool,
    pub stamp: DescriptorStamp,
    pub resource: *mut c_void,
    pub counter: *mut c_void,
    pub immutable_owner: *mut c_void,
    pub immutable_resource: *mut c_void,
    pub buffer_owner: *mut c_void,
    pub buffer_resource: *mut c_void,
    pub buffer_view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    /// The material's immutable lease, its binding and its buffer copy (null when it has none), the holder's.
    pub immutable: *const crate::texture_immutable::ImmutableLease,
    pub binding: *const crate::material_buffer_snapshot::SourceResourceBinding,
    pub buffer: *const crate::material_snapshot_receipt::Copy,
}
/// source_binding_assembly::ShaderInputCheck.
#[repr(C)]
pub struct InputCheckView {
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub offset: u64,
    pub expected: *const u8,
    pub expected_len: usize,
}
/// source_binding_assembly::GuardedT5View.
#[repr(C)]
pub struct T5View {
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub offset: u64,
    pub bytes: u64,
}
/// structured_buffer_snapshot::PreparedCopy.
#[repr(C)]
pub struct PreparedCopy {
    pub owner: *mut c_void,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
    pub reused: bool,
    pub gpu_copied_bytes: u64,
}
/// structured_buffer_snapshot::PrivateRange.
#[repr(C)]
pub struct PrivateRange {
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied_bytes: u64,
}
/// prepared_pair::Source::FullT5.
#[repr(C)]
pub struct FullT5 {
    pub copy: PreparedCopy,
    pub range: PrivateRange,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
}
/// prepared_pair::NativeExit (`identity` the shared reference's object).
#[repr(C)]
pub struct ExitView {
    pub identity: *const c_void,
    pub readback: *mut c_void,
    pub width: u32,
    pub height: u32,
    pub eye: u32,
    pub source_side: u32,
    pub frame: u32,
    pub list: u64,
    pub generation: u64,
    pub planes: *const NativeExitPlane,
    pub plane_count: usize,
}
/// One lent source.
#[repr(C)]
pub struct SourceView {
    pub ordinal: u64,
    pub recording: *const RecordingLease,
    pub source_queue: *mut c_void,
    pub owned: Resources,
    pub owned_state: DrawStateView,
    pub original_draw: *const DrawRecord,
    pub original_pipeline: *mut c_void,
    pub root: *mut c_void,
    pub shader: *const c_void,
    pub shader_root: *const u8,
    pub shader_root_len: usize,
    pub cbvs: *const PrivateCbv,
    pub cbv_count: usize,
    pub cbv_receipt: *const PreparedCopies,
    pub tables: *const Table,
    pub table_count: usize,
    pub reference_tables: *const Table,
    pub reference_table_count: usize,
    pub materials: *const MaterialView,
    pub material_count: usize,
    pub input_checks: *const InputCheckView,
    pub input_check_count: usize,
    pub t5: *const T5View,
    pub full_t5: *const FullT5,
    pub rate_image: *mut c_void,
    pub baseline: *const c_void,
    pub baseline_captured: bool,
    pub allow_vrs_refinement: bool,
    pub baseline_targets: GeometryTargetsRaw,
    pub native_exit: *const ExitView,
    pub pass_depth: *mut c_void,
    pub rate_capture: *mut c_void,
    pub owners: *const *mut c_void,
    pub owner_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<DrawStateView>() == 72
        && core::mem::size_of::<PrivateCbv>() == 72
        && core::mem::size_of::<PreparedCopies>() == 48
        && core::mem::size_of::<MaterialView>() == 304
        && core::mem::size_of::<InputCheckView>() == 40
        && core::mem::size_of::<T5View>() == 72
        && core::mem::size_of::<FullT5>() == 152
        && core::mem::offset_of!(FullT5, range) == 72
        && core::mem::size_of::<ExitView>() == 72
        && core::mem::size_of::<SourceView>() == 1072
        && core::mem::offset_of!(SourceView, baseline_targets) == 704
        && core::mem::offset_of!(SourceView, native_exit) == 1032
        && core::mem::size_of::<Resources>() == 432
);
fn slice<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: the holder's array of `n`.
        unsafe { core::slice::from_raw_parts(p, n) }
    }
}
impl SourceView {
    fn recording(&self) -> &RecordingLease {
        // SAFETY: the holder's lease.
        unsafe { &*self.recording }
    }
    fn draw(&self) -> &DrawRecord {
        // SAFETY: the holder's owned draw.
        unsafe { &*self.owned.draw }
    }
    fn original(&self) -> &DrawRecord {
        // SAFETY: the holder's original draw.
        unsafe { &*self.original_draw }
    }
    fn cbvs(&self) -> &[PrivateCbv] {
        slice(self.cbvs, self.cbv_count)
    }
    fn cbv_receipt(&self) -> &PreparedCopies {
        // SAFETY: the holder's receipt.
        unsafe { &*self.cbv_receipt }
    }
    fn tables(&self) -> &[Table] {
        slice(self.tables, self.table_count)
    }
    fn reference_tables(&self) -> &[Table] {
        slice(self.reference_tables, self.reference_table_count)
    }
    fn materials(&self) -> &[MaterialView] {
        slice(self.materials, self.material_count)
    }
    fn input_checks(&self) -> &[InputCheckView] {
        slice(self.input_checks, self.input_check_count)
    }
    fn t5(&self) -> Option<&T5View> {
        // SAFETY: the holder's view or null.
        unsafe { self.t5.as_ref() }
    }
    fn full_t5(&self) -> Option<&FullT5> {
        // SAFETY: as above.
        unsafe { self.full_t5.as_ref() }
    }
    fn native_exit(&self) -> Option<&ExitView> {
        // SAFETY: as above.
        unsafe { self.native_exit.as_ref() }
    }
    fn owners(&self) -> &[*mut c_void] {
        slice(self.owners, self.owner_count)
    }
    fn has_shader_root(&self) -> bool {
        !self.shader.is_null() && self.shader_root_len != 0
    }
}
fn table_leases(table: &Table) -> &[*mut c_void] {
    slice(table.leases, table.lease_count)
}

// ---- the .cpp's host

/// A pair's family (families::Family), valid while its reference is held.
#[repr(C)]
pub struct FamilyView {
    pub pipeline: *mut c_void,
    pub source_root: *mut c_void,
    pub root: *mut c_void,
    pub plan: PlanView,
    pub uses: *const crate::early_depth_checks::ResourceUse,
    pub use_count: usize,
    pub depth_state: D3D12_DEPTH_STENCIL_DESC,
    pub render_targets: u32,
    pub paired_root: bool,
    pub instance_rebase_safe: bool,
    pub depth_state_known: bool,
}
/// GpuMultilayerMembership's lent source.
#[repr(C)]
pub struct MlmSource {
    pub b4: *mut c_void,
    pub t5: *mut c_void,
    pub b4_offset: u64,
    pub t5_offset: u64,
    pub t5_bytes: u64,
    pub first_element: u32,
    pub num_elements: u32,
    pub membership: *const u32,
    pub source_id: u64,
    pub eye: u32,
}
#[repr(C)]
pub struct MlmInfo {
    pub predicates: *mut c_void,
    pub results: *mut c_void,
    pub sources: u32,
    pub dispatches: u32,
}
/// prepared_membership::Bank's input and summary (the bank stays in the holder).
#[repr(C)]
pub struct MembershipRow {
    pub source: MlmSource,
    pub family_ok: bool,
    pub ordinal: u64,
    pub native_side: u32,
    pub source_pass_storage_extent: u32,
    pub material_slots: u32,
    pub covered_slots: u32,
    pub immutable_slots: u32,
    pub null_slots: u32,
    pub omitted_slots: u32,
    pub maximum_bit: u32,
    pub b4: *mut c_void,
    pub b4_offset: u64,
    pub t5: *mut c_void,
    pub t5_offset: u64,
    pub t5_bytes: u64,
}
/// NativeOutputCopy::RustTarget.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OutputTarget {
    pub kind: u32,
    pub resource: *mut c_void,
    pub heap: *mut c_void,
    pub heap_offset: u64,
    pub allocation_bytes: u64,
    pub plane_count: u32,
    pub final_states: [i32; 2],
}
/// The geometry commands' lent source (StereoGeometryCommands.cpp's RustSource).
#[repr(C)]
pub struct SgcSource {
    pub geometry: Resources,
    pub indirect: *const c_void,
}
const _: () = assert!(
    core::mem::size_of::<FamilyView>() == 160
        && core::mem::offset_of!(FamilyView, depth_state) == 96
        && core::mem::size_of::<MlmSource>() == 72
        && core::mem::size_of::<MembershipRow>() == 160
        && core::mem::size_of::<OutputTarget>() == 56
        && core::mem::size_of::<SgcSource>() == 440
);
#[repr(C)]
pub struct Host {
    pub aux_free: unsafe extern "C" fn(*mut c_void),
    /// The source's SharedBankTable (`reference` 1: of its reference tables) and its SRV coverage receipt
    /// (source_binding_assembly::DescribeOwnedSrvCoverage for the scope; with a stamp, MatchesOwnedSrvCoverage of the slot).
    pub coverage: unsafe extern "C" fn(*mut c_void, usize, u32, usize, *const crate::geometry_pass_scope::Scope, u32, *const DescriptorStamp) -> bool,
    /// The export target `index`'s borrowed pageable (target_lifetime::GetBorrowedPageable, kept by the holder for the
    /// re-authentication), null when refused.
    pub target_pageable: unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> *mut c_void,
    /// The export target `index` authenticated again: its pageable now has the kept one's owner.
    pub target_reauth: unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> bool,
    /// target_access::Describe of the receipt against the native exits ([list, generation] per eye), the lifetimes,
    /// the target residency; the export's rows, or the refusal (2..=5).
    pub export_describe: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *const c_void, *mut c_void, u32, *const u64, *mut [OutputTarget; 8]) -> u32,
    pub export_validate: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut bool) -> bool,
    /// A new empty holder (the export's targets only) for sources a Rust caller owns (pair_source.rs).
    pub export_new: unsafe extern "C" fn() -> *mut c_void,
}
/// The registered queue order lock (ConstantQueueObserver.cpp's recursive mutex, shared with the ExecuteCommandLists
/// runtime and the queue API observer through DeviceHooks.cpp); the Rust test variants (no DeviceHooks) order their
/// prepared submissions with an SRW lock of their own.
fn queue_order(lock: bool) {
    #[cfg(feature = "rust-build")]
    {
        extern "C" {
            fn cvr_cpp_ecl_registered_order(lock: bool);
        }
        // SAFETY: the .cpp's lock, taken and released in pairs on this thread.
        unsafe { cvr_cpp_ecl_registered_order(lock) };
    }
    #[cfg(not(feature = "rust-build"))]
    {
        use windows::Win32::System::Threading::{AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK};
        struct Order(core::cell::UnsafeCell<SRWLOCK>);
        // SAFETY: an SRW lock, used only through its acquire and release.
        unsafe impl Sync for Order {}
        static ORDER: Order = Order(core::cell::UnsafeCell::new(SRWLOCK { Ptr: null_mut() }));
        // SAFETY: the static lock, acquired and released in pairs on this thread.
        unsafe {
            if lock {
                AcquireSRWLockExclusive(ORDER.0.get());
            } else {
                ReleaseSRWLockExclusive(ORDER.0.get());
            }
        }
    }
}
/// indirect_policy::Describe of a signature (the commands' receipts).
unsafe extern "C-unwind" fn describe_signature(signature: *mut c_void, receipt: *mut c_void) {
    // SAFETY: a live signature (or null); the caller's receipt (an indirect_policy Receipt).
    unsafe {
        *receipt.cast::<crate::indirect_policy::Receipt>() = match ID3D12CommandSignature::from_raw_borrowed(&signature) {
            Some(s) => crate::indirect_policy::describe(s),
            None => Default::default(),
        };
    }
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_npp2_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
fn host() -> &'static Host {
    // SAFETY: attached at the .cpp's static initialisation, before any C++ source reaches this module (the Rust
    // callers' works -- the plugin's only ones -- never reach the host).
    unsafe { &*HOST.load(Acquire) }
}
/// The holder of a Rust caller's work (formerly an empty C++ Aux from the host's export_new): a marker, never
/// dereferenced -- the work's sources and export are Rust's.
static RUST_HOLDER: u8 = 0;
fn rust_holder() -> *mut c_void {
    (&raw const RUST_HOLDER).cast_mut().cast()
}

// ---- owned objects

/// An IUnknown-backed Rust storage (bank, geometry commands, membership) or a COM object, held.
type Com = Option<windows::core::IUnknown>;
fn com(raw: *mut c_void) -> Com {
    // SAFETY: a reference handed to the caller.
    (!raw.is_null()).then(|| unsafe { windows::core::IUnknown::from_raw(raw) })
}
fn raw_of<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), |v| v.as_raw())
}
macro_rules! rust_object {
    ($name:ident, $new:ident, $free:ident) => {
        struct $name(*mut c_void);
        impl $name {
            fn new() -> Self {
                // SAFETY: a fresh object.
                $name(unsafe { $new() })
            }
        }
        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: the object made in new, freed once.
                unsafe { $free(self.0) };
            }
        }
    };
}
rust_object!(Compare, cvr_geometry_compare_new, cvr_geometry_compare_free);
rust_object!(FinerVrs, cvr_finer_vrs_new, cvr_finer_vrs_free);
rust_object!(OutputCopy, cvr_output_copy_new, cvr_output_copy_free);
impl Compare {
    fn predicates(&self) -> *mut c_void {
        let mut p = null_mut();
        // SAFETY: a live comparison.
        unsafe { cvr_geometry_compare_info(self.0, &mut p, null_mut()) };
        p
    }
    fn counts(&self) -> [u64; 4] {
        let mut n = [0u64; 4];
        // SAFETY: as above.
        unsafe { cvr_geometry_compare_info(self.0, null_mut(), n.as_mut_ptr()) };
        n
    }
    fn create(&self, device: *mut c_void, pairs: u32, ranges: &[CompareRange], gate: *mut c_void) -> HRESULT {
        // SAFETY: the rows live through the call.
        unsafe { cvr_geometry_compare_create(self.0, device, pairs, ranges.as_ptr(), ranges.len(), gate, 0) }
    }
    fn record(&self, list: *mut c_void) -> bool {
        // SAFETY: a live list.
        unsafe { cvr_geometry_compare_record(self.0, list) }
    }
}
impl FinerVrs {
    fn image(&self) -> *mut c_void {
        let mut image = null_mut();
        // SAFETY: a live object.
        unsafe { cvr_finer_vrs_info(self.0, &mut image, null_mut()) };
        image
    }
    fn counts(&self) -> [u64; 4] {
        let mut n = [0u64; 4];
        // SAFETY: as above.
        unsafe { cvr_finer_vrs_info(self.0, null_mut(), n.as_mut_ptr()) };
        n
    }
    /// GpuFinerVrs::Read: the common map (its readback's bytes) and its invalid rates; empty unless read.
    fn read(&self) -> (HRESULT, Vec<u8>, u32) {
        let mut output = vec![0u8; self.counts()[3] as usize];
        let mut invalid = 0u32;
        // SAFETY: `output` holds the map.
        let hr = unsafe { cvr_finer_vrs_read(self.0, output.as_mut_ptr(), output.len(), &mut invalid) };
        if hr.is_err() {
            output.clear();
        }
        (hr, output, invalid)
    }
}
impl OutputCopy {
    fn info(&self) -> (*mut c_void, u64, bool) {
        let (mut readback, mut bytes, mut recorded) = (null_mut(), 0u64, false);
        // SAFETY: a live copy.
        unsafe { cvr_output_copy_info(self.0, &mut readback, &mut bytes, &mut recorded, null_mut()) };
        (readback, bytes, recorded)
    }
    fn planes(&self) -> Vec<crate::prepared_pair_record::OutputPlane> {
        if self.info().0.is_null() {
            return Vec::new();
        }
        let mut planes = [crate::prepared_pair_record::OutputPlane::default(); 10];
        // SAFETY: as above.
        unsafe { cvr_output_copy_info(self.0, null_mut(), null_mut(), null_mut(), &mut planes) };
        planes.to_vec()
    }
}
/// A geometry chain's storage (StereoGeometryChain).
struct Chain(*mut c_void);
impl Chain {
    fn create(device: *mut c_void, width: u32, height: u32) -> Option<Chain> {
        let mut storage = null_mut();
        // SAFETY: a live device.
        (unsafe { cvr_geometry_chain_create(device, width, height, &mut storage) }.is_ok() && !storage.is_null()).then_some(Chain(storage))
    }
    fn resource(&self, slot: u32) -> *mut c_void {
        // SAFETY: a live chain.
        unsafe { cvr_geometry_chain_resource(self.0, slot) }
    }
    fn bind(&self, list: *mut c_void, mask: u32, colors: u32) -> bool {
        // SAFETY: as above.
        !list.is_null() && unsafe { cvr_geometry_chain_bind(self.0, list, mask, colors) }
    }
    fn seed_private_eye(&self, list: *mut c_void, eye: u32, baseline: &GeometryTargetsRaw) -> bool {
        // SAFETY: as above.
        !list.is_null() && unsafe { cvr_geometry_chain_seed_private_eye(self.0, list, eye, baseline) }
    }
}
impl Drop for Chain {
    fn drop(&mut self) {
        // SAFETY: the storage made in create.
        unsafe { cvr_geometry_chain_release(self.0) };
    }
}
fn chain_resource(chain: &Option<Chain>, slot: u32) -> *mut c_void {
    chain.as_ref().map_or(null_mut(), |c| c.resource(slot))
}
fn bank_counts(bank: &Com) -> crate::binding_bank::Counts {
    let mut counts = crate::binding_bank::Counts { draws: 0, descriptors: 0, snapshots: 0, constant_bytes: 0 };
    if let Some(b) = bank {
        // SAFETY: a live bank.
        unsafe { cvr_bank_counts(b.as_raw(), &mut counts) };
    }
    counts
}
fn mlm_info(storage: &Com) -> MlmInfo {
    let mut info = MlmInfo { predicates: null_mut(), results: null_mut(), sources: 0, dispatches: 0 };
    if let Some(s) = storage {
        // SAFETY: a live membership.
        unsafe { cvr_mlm_info(s.as_raw(), &mut info) };
    }
    info
}
unsafe extern "C-unwind" fn keep(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: a live list and object.
    unsafe { cvr_gpu_keep_command_resources(list, &object, 1) }
}
unsafe extern "C-unwind" fn has(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: as above.
    unsafe { cvr_gpu_has_command_resource(list, object) }
}
fn keep_all(list: *mut c_void, objects: &[*mut c_void]) -> bool {
    // SAFETY: the list's live objects.
    unsafe { cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
}
/// A family of the registry (families_native.rs) and its view (pointers into the family, alive with it).
struct Family {
    _owner: std::sync::Arc<crate::families_native::Family>,
    view: FamilyView,
}
impl Family {
    fn of(owner: std::sync::Arc<crate::families_native::Family>) -> Family {
        let f = &*owner;
        let p = &f.paired_root.plan;
        let view = FamilyView {
            pipeline: f.pipeline.as_raw(),
            source_root: f.source_root.as_raw(),
            root: f.paired_root.root.as_raw(),
            plan: PlanView {
                serialized: p.serialized.as_ptr(),
                serialized_len: p.serialized.len(),
                bindings: p.bindings.as_ptr(),
                binding_count: p.bindings.len(),
                parameters: p.parameters,
                descriptors: p.descriptors,
                original_parameters: p.original_parameters,
                root_dwords: p.root_dwords,
            },
            uses: f.uses.as_ptr().cast(),
            use_count: f.uses.len(),
            depth_state: f.pass.depth_state,
            render_targets: f.pass.targets.NumRenderTargets,
            paired_root: true,
            instance_rebase_safe: f.instance_rebase_safe,
            depth_state_known: f.pass.depth_state_known,
        };
        Family { _owner: owner, view }
    }
    /// families::Lookup of the pipeline.
    fn lookup(pipeline: *mut c_void) -> Option<Family> {
        crate::families_native::lookup_raw(pipeline).map(Family::of)
    }
    fn bindings(&self) -> &[Location] {
        slice(self.view.plan.bindings, self.view.plan.binding_count)
    }
    fn uses(&self) -> &[crate::early_depth_checks::ResourceUse] {
        slice(self.view.uses, self.view.use_count)
    }
}

/// A shared bank table (SharedBankTable) with its leases.
#[derive(Clone)]
struct OwnedTable {
    parameter: u32,
    offset: u32,
    count: u32,
    cpu_heap: *mut c_void,
    leases: Vec<*mut c_void>,
    identity: u64,
    version: u64,
    storage_scope: u64,
    storage_extent: u32,
}
impl OwnedTable {
    fn of(t: &Table) -> OwnedTable {
        OwnedTable {
            parameter: t.parameter,
            offset: t.offset,
            count: t.count,
            cpu_heap: t.cpu_heap,
            leases: table_leases(t).to_vec(),
            identity: t.identity,
            version: t.version,
            storage_scope: t.storage_scope,
            storage_extent: t.storage_extent,
        }
    }
    fn lend(&self) -> Table {
        Table {
            parameter: self.parameter,
            offset: self.offset,
            count: self.count,
            cpu_heap: self.cpu_heap,
            leases: self.leases.as_ptr(),
            lease_count: self.leases.len(),
            identity: self.identity,
            version: self.version,
            storage_scope: self.storage_scope,
            storage_extent: self.storage_extent,
        }
    }
}
fn table_key(t: &Table) -> crate::prepared_pair_tables::TableKey {
    crate::prepared_pair_tables::TableKey {
        heap: t.cpu_heap as u64,
        storage_scope: t.storage_scope,
        identity: t.identity,
        version: t.version,
        offset: t.offset,
        count: t.count,
        storage_extent: t.storage_extent,
    }
}
struct Pair {
    sources: [u32; 2],
    family: Option<std::rc::Rc<Family>>,
    bindings: Vec<BankPairRow>,
    shared: Vec<OwnedTable>,
    command: u32,
    ready: bool,
    actual_bindings: bool,
}
#[derive(Default)]
struct MeshPlan {
    draws: Vec<Step>,
    common: u32,
}
/// The recording's token, held.
struct Recording(RecordingLease);
impl Default for Recording {
    fn default() -> Self {
        Recording(RecordingLease { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        if !self.0.token.is_null() {
            // SAFETY: the lease's reference.
            drop(unsafe { windows::core::IUnknown::from_raw(self.0.token) });
        }
    }
}

const QUERY_STRIDE: u64 = 12 * 8;
const PREDICATE_READBACK_OFFSET: u64 = 3 * QUERY_STRIDE;
const MESH_READBACK_OFFSET: u64 = PREDICATE_READBACK_OFFSET + 8;
const INPUT_READBACK_OFFSET: u64 = MESH_READBACK_OFFSET + 256 * 16;
/// gpu::residency::State::Acquired / Submitted.
const ACQUIRED: i64 = 1;
const SUBMITTED: i64 = 2;
/// The host's residency operations: `which` 0 the sources' lease, 1 the export targets'.
#[derive(Clone, Copy)]
#[repr(u32)]
enum Residency {
    Status = 0,
    SourceCount = 1,
    ObjectCount = 2,
    Cancel = 3,
    Quarantine = 4,
    MarkSubmitted = 5,
    AssociateFence = 6,
    Retire = 7,
    QuarantinedCount = 8,
    Acquire = 9,
    AssetCount = 10,
    MarkTogether = 11,
}

struct Work {
    device: Option<ID3D12Device>,
    aux: *mut c_void,
    views: *const SourceView,
    view_count: usize,
    /// A Rust caller's sources (the views point into them; their tables answer the coverage), None for the .cpp's.
    rust_sources: Option<Box<crate::pair_source::Lent>>,
    /// A Rust caller's native export (its access receipt and borrowed target lifetimes), None for the .cpp's.
    rust_export: Option<Box<RustExport>>,
    eyes: [Vec<u32>; 2],
    pairs: Vec<Pair>,
    pair_for_source: Vec<u32>,
    plan: MeshPlan,
    sequence: bool,
    mesh: Compare,
    image_readback: Option<ID3D12Resource>,
    predicate: Option<ID3D12Resource>,
    inputs: Compare,
    expected_inputs: Option<ID3D12Resource>,
    actual_membership: bool,
    membership_fault: bool,
    membership_banks: Vec<MembershipRow>,
    /// The banks' own membership bits (their rows point at them).
    membership_bits: Vec<Box<[u32; 1024]>>,
    membership_arrays: Vec<Box<[u32; 1024]>>,
    membership: Com,
    membership_readback: Option<ID3D12Resource>,
    rates: FinerVrs,
    common_rates: Vec<u8>,
    original_rates: [Vec<u8>; 2],
    rate_tile: u32,
    rate_width: u32,
    rate_height: u32,
    rate_maps_ready: bool,
    pipeline_queries: Option<ID3D12QueryHeap>,
    occlusion_queries: Option<ID3D12QueryHeap>,
    query_readback: Option<ID3D12Resource>,
    mesh_bytes: u64,
    image_bytes: u64,
    planes: Vec<Plane>,
    native_exits: [*const ExitView; 2],
    allow_vrs_refinement: bool,
    reference_geometry: Com,
    reference_bank: Com,
    paired_bank: Com,
    reference_targets: Option<Chain>,
    paired_targets: Option<Chain>,
    native_rate_targets: Option<Chain>,
    colors: u32,
    width: u32,
    height: u32,
    recording: Recording,
    fence: Option<ID3D12Fence>,
    queue: Option<ID3D12CommandQueue>,
    submitted: bool,
    failed: bool,
    complete: bool,
    stage: u32,
    export_requested: bool,
    export_prepared: bool,
    export_submitted: bool,
    output_copy: Option<OutputCopy>,
    export_allocator: Option<ID3D12CommandAllocator>,
    export_list: Option<ID3D12GraphicsCommandList>,
    current: bool,
    closed: bool,
    execution_issued: bool,
    source_frame: u32,
    owned_serial: u64,
    owned_allocator: Option<ID3D12CommandAllocator>,
    owned_list: Option<ID3D12GraphicsCommandList>,
    owned_flight: *mut c_void,
    quarantined_next: Option<Box<Work>>,
    /// The residency leases: the immutable sources' (0), the export targets' (1).
    leases: core::cell::RefCell<[ResidencyLease; 2]>,
    /// The immutable uses registered on the owned list (texture_read::ImmutableUse, their references this work's).
    uses: Vec<OwnedUse>,
}
// ---- the residency leases (formerly GpuResidencyLease.cpp's gpu::residency::Lease; this is its plugin user)

/// gpu::residency::State.
mod lease_state {
    pub const SUBMITTED: u32 = 2;
    pub const RETIRED: u32 = 5;
    pub const QUARANTINED: u32 = 6;
}
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
fn release_raw(raw: *mut c_void) {
    if !raw.is_null() {
        // SAFETY: a reference this value holds, released once.
        drop(unsafe { windows::core::IUnknown::from_raw(raw) });
    }
}
/// An immutable source's pageable (texture_read::ImmutablePageable: its lease's owner and resource and the pageable
/// referenced), released with its storage.
struct HeldPageable(crate::texture_immutable::ImmutablePageable);
impl Drop for HeldPageable {
    fn drop(&mut self) {
        for raw in [self.0.source.owner, self.0.source.source.resource, self.0.pageable] {
            release_raw(raw);
        }
    }
}
/// A lease's admitted storage (residency_lease.rs'), its sources' pageables and its export targets' indices.
struct LeaseStorage {
    device: Option<ID3D12Device>,
    rust: *mut crate::residency_lease::Storage,
    _sources: Vec<HeldPageable>,
    targets: Vec<u32>,
}
impl Drop for LeaseStorage {
    fn drop(&mut self) {
        // SAFETY: the storage this value admitted, freed once.
        unsafe { crate::residency_lease::cvr_rl_free(self.rust) };
    }
}
/// gpu::residency::Lease.
#[derive(Default)]
struct ResidencyLease {
    storage: Option<Box<LeaseStorage>>,
    terminal: u32,
    objects: usize,
    sources: usize,
}
impl ResidencyLease {
    fn status(&self) -> u32 {
        // SAFETY: the held storage.
        self.storage.as_ref().map_or(self.terminal, |s| unsafe { crate::residency_lease::cvr_rl_status(s.rust) })
    }
    fn begin(&self, device: *mut c_void, count: usize) -> Result<Box<LeaseStorage>, i32> {
        let mut rust = null_mut();
        // SAFETY: a live device (or null); a local output.
        let hr = unsafe { crate::residency_lease::cvr_rl_begin(device, count, self.storage.is_some(), self.terminal, &mut rust) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: the live device (referenced for the storage).
        let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned();
        Ok(Box::new(LeaseStorage { device, rust, _sources: Vec::new(), targets: Vec::new() }))
    }
    fn acquire_storage(&mut self, next: Box<LeaseStorage>, source_count: usize) -> i32 {
        // SAFETY: the admitted storage.
        let hr = unsafe { crate::residency_lease::cvr_rl_acquire(next.rust) };
        // SAFETY: as above.
        if hr < 0 && unsafe { crate::residency_lease::cvr_rl_status(next.rust) } != lease_state::QUARANTINED {
            return hr;
        }
        // SAFETY: as above.
        self.objects = unsafe { crate::residency_lease::cvr_rl_objects(next.rust) };
        self.sources = source_count;
        self.storage = Some(next);
        if hr < 0 {
            self.quarantine();
        }
        hr
    }
    /// Acquire: the immutable sources' pageables made resident.
    fn acquire(&mut self, device: *mut c_void, assets: &[*const crate::texture_immutable::ImmutableLease]) -> i32 {
        let mut next = match self.begin(device, assets.len()) {
            Ok(next) => next,
            Err(hr) => return hr,
        };
        for &asset in assets {
            // SAFETY: a zeroed pageable (no references) for the call to fill.
            let mut physical: crate::texture_immutable::ImmutablePageable = unsafe { core::mem::zeroed() };
            // SAFETY: the device and the holder's lease.
            let made = unsafe { crate::texture_immutable::cvr_tr_immutable_pageable(device, asset, &mut physical) };
            let held = HeldPageable(physical);
            if !made {
                return E_ACCESSDENIED;
            }
            // SAFETY: the admitted storage and a live pageable.
            let added = unsafe { crate::residency_lease::cvr_rl_add(next.rust, held.0.pageable) };
            if added < 0 {
                return added;
            }
            next._sources.push(held);
        }
        self.acquire_storage(next, assets.len())
    }
    /// AcquireBorrowedTargets: the eight export targets' pageables (the holder's, through the host) made resident.
    fn acquire_targets(&mut self, device: *mut c_void, aux: *mut c_void, rust: Option<&RustExport>) -> i32 {
        let mut next = match self.begin(device, 8) {
            Ok(next) => next,
            Err(hr) => return hr,
        };
        for index in 0..8u32 {
            let pageable = match rust {
                Some(r) => r.pageable(index, device),
                // SAFETY: the host's holder and device.
                None => unsafe { (host().target_pageable)(aux, index, device) },
            };
            if pageable.is_null() {
                return E_ACCESSDENIED;
            }
            // SAFETY: the admitted storage and the holder's live pageable.
            let added = unsafe { crate::residency_lease::cvr_rl_add(next.rust, pageable) };
            if added < 0 {
                return added;
            }
            next.targets.push(index);
        }
        self.acquire_storage(next, 8)
    }
    fn associate_fence(&mut self, queue: *mut c_void, fence: *mut c_void, value: u64, signal: i32) -> i32 {
        let Some(storage) = self.storage.as_ref() else { return E_UNEXPECTED };
        if self.status() != lease_state::SUBMITTED {
            return E_UNEXPECTED;
        }
        // SAFETY: the held storage, the queue and fence.
        let hr = unsafe { crate::residency_lease::cvr_rl_fence(storage.rust, queue, fence, value, signal) };
        if hr < 0 {
            self.quarantine();
        }
        hr
    }
    /// Cancel (0) / Retire (1).
    fn release(&mut self, which: u32) -> i32 {
        let Some(storage) = self.storage.as_ref() else { return E_UNEXPECTED };
        // SAFETY: the held storage.
        let hr = unsafe { crate::residency_lease::cvr_rl_release(storage.rust, which) };
        match self.status() {
            lease_state::QUARANTINED => self.quarantine(),
            lease_state::RETIRED => {
                self.storage = None;
                self.terminal = lease_state::RETIRED;
            }
            _ => {}
        }
        hr
    }
    /// Quarantine: a storage whose residency may still be in flight is kept for the process.
    fn quarantine(&mut self) {
        let Some(storage) = self.storage.take() else { return };
        // SAFETY: the held storage.
        if !unsafe { crate::residency_lease::cvr_rl_quarantine(storage.rust) } {
            return;
        }
        core::mem::forget(storage);
        self.terminal = lease_state::QUARANTINED;
    }
}
impl Drop for ResidencyLease {
    fn drop(&mut self) {
        self.quarantine();
    }
}
/// MarkSubmittedTogether: the leases submitted with the queue, each one's export targets authenticated again.
fn mark_together(group: &[&ResidencyLease], aux: *mut c_void, rust: Option<&RustExport>, queue: *mut c_void) -> i32 {
    struct Context<'a> {
        group: &'a [&'a ResidencyLease],
        aux: *mut c_void,
        rust: Option<&'a RustExport>,
    }
    unsafe extern "C-unwind" fn current(context: *mut c_void, index: usize) -> bool {
        // SAFETY: the context below, alive for the call.
        let c = unsafe { &*context.cast::<Context>() };
        let Some(storage) = c.group.get(index).and_then(|l| l.storage.as_ref()) else { return true };
        let device = storage.device.as_ref().map_or(null_mut(), |d| d.as_raw());
        storage.targets.iter().all(|&i| match c.rust {
            Some(r) => r.reauth(i, device),
            // SAFETY: the host's holder and the storage's device.
            None => unsafe { (host().target_reauth)(c.aux, i, device) },
        })
    }
    let members: Vec<crate::residency_lease::Member> = group
        .iter()
        .map(|l| crate::residency_lease::Member {
            lease: (*l as *const ResidencyLease).cast(),
            storage: l.storage.as_ref().map_or(null_mut(), |s| s.rust),
        })
        .collect();
    let mut context = Context { group, aux, rust };
    // SAFETY: the members and the context live for the call.
    unsafe { crate::residency_lease::cvr_rl_submit(queue, members.as_ptr(), members.len(), current, (&raw mut context).cast()) }
}
/// An immutable use whose references (the lease's owner and resource, the recording's token) are released with it.
struct OwnedUse(crate::texture_immutable::ImmutableUse);
impl Drop for OwnedUse {
    fn drop(&mut self) {
        for raw in [self.0.source.owner, self.0.source.source.resource, self.0.recording.token] {
            if !raw.is_null() {
                // SAFETY: a reference the registration handed this use.
                drop(unsafe { windows::core::IUnknown::from_raw(raw) });
            }
        }
    }
}
impl Work {
    fn new(aux: *mut c_void, views: *const SourceView, view_count: usize) -> Box<Work> {
        Box::new(Work {
            device: None,
            aux,
            views,
            view_count,
            rust_sources: None,
            rust_export: None,
            eyes: [Vec::new(), Vec::new()],
            pairs: Vec::new(),
            pair_for_source: Vec::new(),
            plan: MeshPlan::default(),
            sequence: false,
            mesh: Compare::new(),
            image_readback: None,
            predicate: None,
            inputs: Compare::new(),
            expected_inputs: None,
            actual_membership: false,
            membership_fault: false,
            membership_banks: Vec::new(),
            membership_bits: Vec::new(),
            membership_arrays: Vec::new(),
            membership: None,
            membership_readback: None,
            rates: FinerVrs::new(),
            common_rates: Vec::new(),
            original_rates: [Vec::new(), Vec::new()],
            rate_tile: 0,
            rate_width: 0,
            rate_height: 0,
            rate_maps_ready: false,
            pipeline_queries: None,
            occlusion_queries: None,
            query_readback: None,
            mesh_bytes: 0,
            image_bytes: 0,
            planes: Vec::new(),
            native_exits: [null(); 2],
            allow_vrs_refinement: false,
            reference_geometry: None,
            reference_bank: None,
            paired_bank: None,
            reference_targets: None,
            paired_targets: None,
            native_rate_targets: None,
            colors: 0,
            width: 0,
            height: 0,
            recording: Recording::default(),
            fence: None,
            queue: None,
            submitted: false,
            failed: false,
            complete: false,
            stage: 3,
            export_requested: false,
            export_prepared: false,
            export_submitted: false,
            output_copy: None,
            export_allocator: None,
            export_list: None,
            current: false,
            closed: false,
            execution_issued: false,
            source_frame: 0,
            owned_serial: 0,
            owned_allocator: None,
            owned_list: None,
            owned_flight: null_mut(),
            quarantined_next: None,
            uses: Vec::new(),
            leases: Default::default(),
        })
    }
    fn sources(&self) -> &[SourceView] {
        slice(self.views, self.view_count)
    }
    fn source(&self, i: usize) -> &SourceView {
        &self.sources()[i]
    }
    /// A residency lease's operation (`which` 1 the export targets', else the sources'; MarkTogether's a bitmask:
    /// 1 the sources', 2 the targets'); Acquire passes the device as `queue`.
    fn residency(&self, which: u32, op: Residency, queue: *mut c_void, fence: *mut c_void, value: u64, signal: HRESULT) -> i64 {
        if self.aux.is_null() {
            return 0;
        }
        let mut leases = self.leases.borrow_mut();
        let at = usize::from(which == 1);
        match op {
            Residency::Status => i64::from(leases[at].status()),
            Residency::SourceCount => leases[at].sources as i64,
            Residency::ObjectCount => leases[at].objects as i64,
            Residency::Cancel => i64::from(leases[at].release(0)),
            Residency::Quarantine => {
                leases[at].quarantine();
                0
            }
            Residency::MarkSubmitted => i64::from(mark_together(&[&leases[at]], self.aux, self.rust_export.as_deref(), queue)),
            Residency::AssociateFence => i64::from(leases[at].associate_fence(queue, fence, value, signal.0)),
            Residency::Retire => i64::from(leases[at].release(1)),
            Residency::QuarantinedCount => crate::residency_lease::cvr_rl_quarantined() as i64,
            Residency::Acquire => {
                let assets = self.assets();
                i64::from(leases[0].acquire(queue, &assets))
            }
            Residency::AssetCount => self.assets().len() as i64,
            Residency::MarkTogether => {
                let (sources, targets) = leases.split_at(1);
                let mut group: Vec<&ResidencyLease> = Vec::new();
                if which & 1 != 0 {
                    group.push(&sources[0]);
                }
                if which & 2 != 0 {
                    group.push(&targets[0]);
                }
                i64::from(mark_together(&group, self.aux, self.rust_export.as_deref(), queue))
            }
        }
    }
    /// The sources' immutable leases, once per owner in source and material order (formerly the .cpp's Assets).
    fn assets(&self) -> Vec<*const crate::texture_immutable::ImmutableLease> {
        let mut owners = std::collections::BTreeSet::new();
        let mut assets = Vec::new();
        for i in 0..self.view_count {
            for m in self.source(i).materials() {
                if !m.immutable.is_null() && !m.immutable_owner.is_null() && owners.insert(m.immutable_owner as usize) {
                    assets.push(m.immutable);
                }
            }
        }
        assets
    }
    fn res(&self, which: u32, op: Residency) -> i64 {
        self.residency(which, op, null_mut(), null_mut(), 0, S_OK)
    }
    fn native_exit(&self, eye: usize) -> Option<&ExitView> {
        // SAFETY: a holder's exit or null.
        unsafe { self.native_exits[eye].as_ref() }
    }
    fn draw_permission(&self) -> *mut c_void {
        let inputs = self.inputs.predicates();
        if !inputs.is_null() {
            inputs
        } else {
            raw_of(&self.predicate)
        }
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        // SAFETY: the lists' command resources.
        unsafe {
            if let Some(list) = &self.owned_list {
                cvr_gpu_reset_command_resources(list.as_raw());
            }
            if let Some(list) = &self.export_list {
                cvr_gpu_reset_command_resources(list.as_raw());
            }
        }
        if self.res(0, Residency::Status) == ACQUIRED {
            self.res(0, Residency::Cancel);
        }
        if self.res(1, Residency::Status) == ACQUIRED {
            self.res(1, Residency::Cancel);
        }
        if !self.owned_flight.is_null() {
            // SAFETY: the submission prepared and never handed on.
            unsafe { cvr_gpu_free_submission(self.owned_flight) };
        }
        self.pairs.clear();
        if self.aux != rust_holder() {
            // SAFETY: the holder, freed once (the sources and the C++ objects it keeps).
            unsafe { (host().aux_free)(self.aux) };
        }
    }
}

// ---- the shared state (the .cpp's mutex and its globals)

struct State {
    work: Option<Box<Work>>,
    next_owned_serial: u64,
    quarantined: Option<Box<Work>>,
}
struct Shared {
    lock: Mutex<()>,
    state: UnsafeCell<State>,
}
// SAFETY: the state is touched only under `lock`.
unsafe impl Sync for Shared {}
static SHARED: Shared = Shared { lock: Mutex::new(()), state: UnsafeCell::new(State { work: None, next_owned_serial: 0, quarantined: None }) };
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TEST_RESET_ENTERED: AtomicBool = AtomicBool::new(false);
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
fn active() -> bool {
    ACTIVE.load(Acquire)
}
macro_rules! ex {
    ($name:ident) => {
        // SAFETY: an export of this port, written under the prepared pair's lock as the .cpp wrote it.
        unsafe { &mut *addr_of_mut!($name) }
    };
}
fn internal() -> bool {
    cvr_host::probe::internal()
}

// ---- failure and publication

fn cancel_unsubmitted_residency(w: &Work) {
    match w.res(1, Residency::Status) {
        ACQUIRED => ex!(CyberpunkVR_PreparedNativeExport)[19] = w.res(1, Residency::Cancel) as u32 as u64,
        SUBMITTED => {
            w.res(1, Residency::Quarantine);
        }
        _ => {}
    }
    if w.res(0, Residency::SourceCount) == 0 {
        return;
    }
    match w.res(0, Residency::Status) {
        ACQUIRED => ex!(CyberpunkVR_PreparedPairResidency)[6] = w.res(0, Residency::Cancel) as u32 as u64,
        SUBMITTED => {
            w.res(0, Residency::Quarantine);
        }
        _ => {}
    }
    let residency = ex!(CyberpunkVR_PreparedPairResidency);
    residency[2] = w.res(0, Residency::Status) as u64;
    residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
}
fn error(s: &mut State, reason: u32) {
    let stats = ex!(CyberpunkVR_PreparedPairStats);
    if stats[10] == 0 {
        stats[10] = u64::from(reason);
    }
    if let Some(w) = s.work.as_mut() {
        w.failed = true;
        if w.stage == 3 {
            cancel_unsubmitted_residency(w);
        }
    }
}
fn publish(s: &State) {
    let Some(w) = s.work.as_ref() else { return };
    let stats = ex!(CyberpunkVR_PreparedPairStats);
    stats[11] = u64::from(w.stage);
    ex!(CyberpunkVR_PreparedCurrent)[12] = stats[10];
    CyberpunkVR_PreparedPairState.store(if w.complete { if w.failed { 6 } else { 5 } } else { w.stage }, Release);
}
fn fail_current(s: &mut State, reason: u32) {
    if !s.work.as_ref().is_some_and(|w| w.current) {
        return;
    }
    error(s, reason);
    let w = s.work.as_mut().expect("work");
    w.complete = true;
    ACTIVE.store(false, SeqCst);
    if w.execution_issued {
        w.res(0, Residency::Quarantine);
        w.res(1, Residency::Quarantine);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[2] = w.res(0, Residency::Status) as u64;
        residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
        publish(s);
        let mut w = s.work.take().expect("work");
        w.quarantined_next = s.quarantined.take();
        s.quarantined = Some(w);
    } else {
        cancel_unsubmitted_residency(w);
        publish(s);
        s.work = None;
    }
}

// ---- the source checks

/// The material's buffer copy matches its binding (staged: at the source's recording and queue; else privately).
fn material_matches(w: &Work, source: usize, material: usize) -> bool {
    let s = w.source(source);
    let m = &s.materials()[material];
    if !m.has_buffer || m.buffer.is_null() || m.binding.is_null() {
        return false;
    }
    // SAFETY: the holder's copy, binding, recording and queue.
    unsafe {
        if w.current {
            crate::material_snapshot_receipt::cvr_mbsn_matches(m.buffer, m.binding.cast(), true, s.recording.cast(), s.source_queue)
        } else {
            crate::material_snapshot_receipt::cvr_mbsn_matches(m.buffer, m.binding.cast(), false, core::ptr::null(), null_mut())
        }
    }
}
/// texture_read::ValidateImmutable of the source's material (on `queue`, or unsubmitted when null).
fn immutable_valid(w: &Work, source: usize, material: usize, queue: *mut c_void) -> bool {
    let m = &w.source(source).materials()[material];
    // SAFETY: the holder's lease.
    !m.immutable.is_null() && unsafe { crate::texture_immutable::cvr_tr_validate_immutable(m.immutable, queue) }
}
fn has_baseline(source: &SourceView) -> bool {
    if source.baseline.is_null() || !source.baseline_captured || source.baseline_targets.colors != 3 {
        return false;
    }
    let resources = source.baseline_targets.resources.map(|r| r as u64);
    // SAFETY: five values.
    unsafe { crate::prepared_pair_inputs::cvr_npp_baseline(source.baseline_targets.colors, source.pass_depth as u64, &resources) }
}
fn native_exit_matches_source(w: &Work, source: &SourceView, eye: u32) -> bool {
    let Some(exit) = source.native_exit() else { return false };
    let lent = crate::prepared_pair_exit::Exit {
        width: exit.width,
        height: exit.height,
        eye: exit.eye,
        source_side: exit.source_side,
        frame: exit.frame,
        list: exit.list,
        generation: exit.generation,
    };
    let owner = crate::prepared_pair_exit::Owner {
        width: w.width,
        height: w.height,
        source_frame: w.source_frame,
        original_frame: source.original().frame,
        owned_frame: source.draw().frame,
        original_side: source.original().side,
        owned_side: source.draw().side,
        recording_list: source.recording().list,
        original_list: source.original().list,
        owned_list: source.draw().list,
        recording_generation: source.recording().generation,
    };
    // SAFETY: local values.
    unsafe { crate::prepared_pair_exit::cvr_npp_exit_matches(&lent, &owner, eye) }
}
fn validate_native_exit(w: &Work, source: &SourceView) -> bool {
    let Some(exit) = source.native_exit() else { return false };
    let resources = source.baseline_targets.resources.map(|r| r as *mut c_void);
    // SAFETY: the exit's planes and the baseline's targets.
    unsafe {
        crate::prepared_pair_record::cvr_npp_validate_exit(raw_of(&w.device), exit.readback, exit.planes, exit.plane_count, &resources, w.width, w.height)
    }
}
fn exit_view(exit: Option<&ExitView>) -> crate::prepared_pair_record::ExitView {
    exit.map_or(crate::prepared_pair_record::ExitView { readback: null_mut(), planes: null(), count: 0 }, |e| crate::prepared_pair_record::ExitView {
        readback: e.readback,
        planes: e.planes,
        count: e.plane_count,
    })
}
fn compare_native_exit(s: &mut State, image: &[u8]) {
    let w = s.work.as_ref().expect("work");
    if w.native_exits[0].is_null() {
        return;
    }
    let c = crate::prepared_pair_record::ExitCompare {
        exits: [exit_view(w.native_exit(0)), exit_view(w.native_exit(1))],
        planes: w.planes.as_ptr(),
        plane_count: w.planes.len(),
        image: image.as_ptr(),
        image_bytes: image.len(),
        common: w.common_rates.as_ptr(),
        common_bytes: w.common_rates.len(),
        original: [w.original_rates[0].as_ptr(), w.original_rates[1].as_ptr()],
        original_bytes: [w.original_rates[0].len(), w.original_rates[1].len()],
        baseline: ex!(CyberpunkVR_PreparedBaseline)[22],
        pair_different: [ex!(CyberpunkVR_PreparedPairStats)[6], ex!(CyberpunkVR_PreparedPairStats)[7]],
        width: w.width,
        height: w.height,
        tile: w.rate_tile,
        map_width: w.rate_width,
        map_height: w.rate_height,
        maps_ready: w.rate_maps_ready,
        allow_refinement: w.allow_vrs_refinement,
    };
    let counters = crate::prepared_pair_record::ExitCounters {
        exit: addr_of_mut!(CyberpunkVR_PreparedNativeExit),
        samples: addr_of_mut!(CyberpunkVR_PreparedNativeExitSamples),
        rate: addr_of_mut!(CyberpunkVR_PreparedNativeRate),
        attribution: addr_of_mut!(CyberpunkVR_PreparedVrsAttribution),
        acceptance: addr_of_mut!(CyberpunkVR_PreparedNativeAcceptance),
    };
    // SAFETY: the comparison's lent views.
    if unsafe { crate::prepared_pair_record::cvr_npp_compare_exit(&c, &counters) } {
        error(s, 43);
    }
}
fn readback(device: *mut c_void, bytes: u64, out: &mut Option<ID3D12Resource>) -> HRESULT {
    if bytes == 0 || bytes > 512 * 1024 * 1024 {
        return windows::Win32::Foundation::E_INVALIDARG;
    }
    *out = None;
    // SAFETY: a live device; `out` receives the reference.
    unsafe {
        crate::readback_buffers::cvr_rb_buffer(
            device,
            null_mut(),
            bytes,
            D3D12_HEAP_TYPE_READBACK.0,
            D3D12_RESOURCE_STATE_COPY_DEST.0,
            (out as *mut Option<ID3D12Resource>).cast(),
        )
    }
}
fn same_state(a: &DrawStateView, b: &DrawStateView) -> bool {
    // SAFETY: the states' rates and blend factors.
    unsafe {
        crate::prepared_pair_checks::cvr_npp_same_state(
            a.pipeline as u64,
            b.pipeline as u64,
            (&a.rate as *const Rate).cast(),
            (&b.rate as *const Rate).cast(),
            a.topology,
            b.topology,
            a.stencil,
            b.stencil,
            &a.blend,
            &b.blend,
        )
    }
}
fn material_at(source: &SourceView, parameter: u32, offset: u32) -> Option<usize> {
    source.materials().iter().position(|m| m.parameter == parameter && m.offset == offset)
}
/// The full T5 copy is the source's own (its recording and ordinal) and its recorded view is the lent one.
fn full_t5_view(w: &Work, source: usize) -> bool {
    let s = w.source(source);
    let Some(full) = s.full_t5() else { return false };
    // SAFETY: the holder's recording.
    let Some(recording) = (unsafe { s.recording.as_ref() }) else { return false };
    if !crate::prepared_pair_inputs::cvr_npp_full_t5(
        full.copy.list,
        full.copy.generation,
        full.copy.occurrence,
        recording.list,
        recording.generation,
        s.ordinal,
        full.range.source_offset,
        full.range.copied_bytes,
    ) {
        return false;
    }
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: the holder's copy and range (structured_buffer_snapshot's records); local and lent views.
    unsafe {
        crate::structured_snapshot_receipt::cvr_sbsn_recorded_view((&raw const full.copy).cast(), (&raw const full.range).cast(), &mut view)
            && crate::structured_snapshot::cvr_ssv_check(0, &view, &full.view, core::ptr::null())
    }
}
impl Work {
    /// The immutable uses: 0 cleared, 1 each distinct immutable owner's lease registered on `argument` (a list; its
    /// failure 100 + texture_read's), 2 validated on `argument` (a queue; the failures), 3 their count.
    fn uses_op(&mut self, op: u32, argument: *mut c_void) -> u32 {
        match op {
            0 => {
                self.uses.clear();
                0
            }
            1 => {
                let mut seen = std::collections::BTreeSet::new();
                let mut leases = Vec::new();
                for i in 0..self.view_count {
                    for m in self.source(i).materials() {
                        if !m.immutable.is_null() && !m.immutable_owner.is_null() && seen.insert(m.immutable_owner as usize) {
                            leases.push(m.immutable);
                        }
                    }
                }
                {
                    for immutable in leases {
                        // SAFETY: a zeroed use (no references) for the registration to fill.
                        let mut value: crate::texture_immutable::ImmutableUse = unsafe { core::mem::zeroed() };
                        // SAFETY: the list and the holder's lease.
                        let ok = unsafe { crate::texture_immutable::cvr_tr_register_immutable_use(argument, immutable, &mut value) };
                        let value = OwnedUse(value);
                        if !ok {
                            return 100 + crate::texture_lease::cvr_tr_failure();
                        }
                        self.uses.push(value);
                    }
                }
                0
            }
            // SAFETY: the uses and the queue.
            2 => self.uses.iter().filter(|u| !unsafe { crate::texture_immutable::cvr_tr_validate_immutable_use(&u.0, argument) }).count() as u32,
            _ => self.uses.len() as u32,
        }
    }
}
fn membership_error(reason: u32, ordinal: u64, builder: u32) -> bool {
    let stats = ex!(CyberpunkVR_PreparedActualMembershipStats);
    if stats[15] == 0 {
        stats[15] = u64::from(reason);
        stats[16] = if ordinal == u64::MAX { 0 } else { ordinal + 1 };
        stats[17] = u64::from(builder);
    }
    false
}
fn reset_membership(requested: bool, frame: u32) {
    *ex!(CyberpunkVR_PreparedActualMembershipStats) = [0; 24];
    ex!(CyberpunkVR_PreparedActualMembershipStats)[0] = 2;
    *ex!(CyberpunkVR_PreparedActualMembershipRows) = [0; 16384];
    // SAFETY: as ex!.
    unsafe { CyberpunkVR_PreparedActualMembershipCount = 0 };
    ex!(CyberpunkVR_PreparedActualMembershipStats)[1] = u64::from(requested);
    if requested {
        ex!(CyberpunkVR_PreparedActualMembershipStats)[2] = u64::from(frame);
    }
}
/// The source's one private B4 CBV (parameter 19, offset 0) whose recorded copy is the source's receipt's.
fn membership_b4(w: &Work, source: usize) -> Option<usize> {
    let s = w.source(source);
    let mut found = None;
    for (i, cbv) in s.cbvs().iter().enumerate() {
        if cbv.parameter == 19 && cbv.offset == 0 {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    let at = found?;
    // SAFETY: the holder's receipt and private CBV (the same records).
    let matches = unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_matches_recorded(s.cbv_receipt.cast(), (&raw const s.cbvs()[at]).cast()) };
    matches.then_some(at)
}

// ---- preparation

fn prepare_membership_banks(w: &mut Work) -> bool {
    if !w.actual_membership {
        return true;
    }
    if !w.current || !w.sequence || w.native_exits[0].is_null() || w.native_exits[1].is_null() || w.view_count > 512 {
        return membership_error(1, u64::MAX, 0);
    }
    let stats = ex!(CyberpunkVR_PreparedActualMembershipStats);
    stats[3] = w.view_count as u64;
    for i in 0..w.view_count {
        let source = w.source(i);
        let (ordinal, side) = (source.ordinal, source.original().side);
        let mut bits = Box::new([0u32; 1024]);
        let mut row = match build_membership(w, i, &mut bits) {
            Ok(row) => row,
            Err(failure) => return membership_error(2, ordinal, failure),
        };
        row.source.membership = bits.as_ptr();
        w.membership_bits.push(bits);
        let at = w.membership_banks.len() * 32;
        let values = [
            ordinal,
            u64::from(side),
            u64::from(w.source_frame),
            row.b4 as u64,
            row.b4_offset,
            row.t5 as u64,
            row.t5_offset,
            row.t5_bytes,
            u64::from(row.source_pass_storage_extent),
            u64::from(row.material_slots),
            u64::from(row.covered_slots),
            u64::from(row.immutable_slots),
            u64::from(row.null_slots),
            u64::from(row.omitted_slots),
            u64::from(row.maximum_bit),
        ];
        ex!(CyberpunkVR_PreparedActualMembershipRows)[at..at + values.len()].copy_from_slice(&values);
        stats[5] += u64::from(row.covered_slots);
        stats[6] += u64::from(row.omitted_slots);
        w.membership_banks.push(row);
        stats[4] = w.membership_banks.len() as u64;
        // SAFETY: as ex!.
        unsafe { CyberpunkVR_PreparedActualMembershipCount = w.membership_banks.len() as u32 };
    }
    true
}
/// The source's table `table` is its one bindless reference table's storage (parameter 31, the same heap, labels and
/// extent) with an SRV coverage receipt covering every material slot below `extent`.
fn membership_table(w: &Work, source: usize, table: usize, extent: u32) -> bool {
    let s = w.source(source);
    let mut reference = None;
    for t in s.reference_tables() {
        if t.parameter == 31 {
            if reference.is_some() {
                return false;
            }
            reference = Some(t);
        }
    }
    let (Some(reference), Some(actual)) = (reference, s.tables().get(table)) else { return false };
    let key = |t: &Table| crate::prepared_pair_tables::TableKey {
        heap: t.cpu_heap as u64,
        storage_scope: t.storage_scope,
        identity: t.identity,
        version: t.version,
        offset: t.offset,
        count: t.count,
        storage_extent: t.storage_extent,
    };
    // SAFETY: two local keys.
    if !unsafe { crate::prepared_pair_tables::cvr_npp_same_reference(&key(reference), &key(actual), extent) } {
        return false;
    }
    let scope = source_scope(s);
    coverage(w, source, false, table, &scope, None)
        && s.materials().iter().all(|m| m.parameter != 31 || m.offset >= extent || coverage(w, source, false, table, &scope, Some((m.offset, &m.stamp))))
}
// ---- the membership banks (formerly PreparedMembershipBank.cpp's Build) and the membership table check

/// prepared_membership::Failure.
mod membership_failure {
    pub const ARGUMENTS: u32 = 1;
    pub const FAMILY: u32 = 2;
    pub const SCOPE: u32 = 3;
    pub const B4: u32 = 4;
    pub const T5: u32 = 5;
    pub const TABLE: u32 = 6;
    pub const COVERAGE: u32 = 7;
    pub const COLLISION: u32 = 8;
    pub const CAPACITY: u32 = 10;
}
/// structured_buffer_snapshot::detail::SourceBytes; multilayer_selectors::B4Bytes, DeclaredExtent.
const FULT_T5_SOURCE_BYTES: u64 = 1024 * 1024;
const MULTILAYER_B4_BYTES: u32 = 228;
const MULTILAYER_DECLARED_EXTENT: u32 = 32768;
/// The source's assembly scope (source_binding_assembly::Scope): its root, recording, ordinal and side.
fn source_scope(s: &SourceView) -> crate::geometry_pass_scope::Scope {
    let recording = s.recording();
    crate::geometry_pass_scope::Scope {
        root: s.root as u64,
        list: recording.list,
        generation: recording.generation,
        occurrence: s.ordinal.wrapping_add(1),
        source: s.ordinal.wrapping_add(1),
        eye: s.original().side,
    }
}
/// The holder's SharedBankTable `table` (of the source's tables, `reference` its reference tables): its SRV coverage
/// receipt described for the scope, or with `slot` this slot's descriptor matched in it.
fn coverage(w: &Work, source: usize, reference: bool, table: usize, scope: &crate::geometry_pass_scope::Scope, slot: Option<(u32, &DescriptorStamp)>) -> bool {
    if let Some(lent) = w.rust_sources.as_ref() {
        let Some(s) = lent.sources.get(source) else { return false };
        let Some(t) = (if reference { &s.reference_tables } else { &s.tables }).get(table) else { return false };
        return match slot {
            // SAFETY: the same 176-byte stamp record.
            Some((offset, stamp)) => crate::shared_table::matches(t, scope, offset, unsafe { &*(stamp as *const DescriptorStamp).cast() }),
            None => crate::shared_table::describe(t, scope).is_some(),
        };
    }
    let (offset, stamp) = slot.map_or((0, null()), |(o, s)| (o, s as *const DescriptorStamp));
    // SAFETY: the holder's source and table; the scope and stamp live for the call.
    unsafe { (host().coverage)(w.aux, source, u32::from(reference), table, scope, offset, stamp) }
}
/// A default-heap buffer holding [offset, offset + bytes) (PreparedMembershipBank.cpp's PrivateRange).
fn membership_range(resource: *mut c_void, offset: u64, bytes: u64) -> bool {
    if resource.is_null() || bytes == 0 || offset % 4 != 0 {
        return false;
    }
    // SAFETY: the holder's live resource.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else { return false };
    // SAFETY: plain queries of a live resource.
    let desc = unsafe { r.GetDesc() };
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Flags.0 == 0
        && offset <= desc.Width
        && bytes <= desc.Width - offset
        // SAFETY: local outputs.
        && unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type == D3D12_HEAP_TYPE_DEFAULT
}
fn multilayer_family(f: &crate::families_native::Family) -> bool {
    f.root_sha == crate::multilayer_selectors::ROOT_SHA256 && f.vertex_sha == crate::multilayer_selectors::VERTEX_SHA256 && f.pixel_sha == crate::multilayer_selectors::PIXEL_SHA256
}
/// prepared_membership::Build: the multilayer source's B4 and full T5 private copies and the SourcePass storage's
/// authenticated SRV coverage; each material slot of the bindless table either covered (its typed null or immutable
/// receipt) or omitted; the membership bits (kept in `bits`) and the summary row.
fn build_membership(w: &Work, index: usize, bits: &mut [u32; 1024]) -> Result<MembershipRow, u32> {
    use membership_failure::*;
    let s = w.source(index);
    // SAFETY: the holder's recording (or null).
    let recording = unsafe { s.recording.as_ref() }.ok_or(ARGUMENTS)?;
    let (original, owned) = (s.original(), s.draw());
    if s.root.is_null()
        || s.shader.is_null()
        || s.original_pipeline.is_null()
        || recording.token.is_null()
        || recording.list == 0
        || recording.generation == 0
        || s.ordinal == u64::MAX
        || original.side > 1
        || original.side != owned.side
        || original.list != recording.list
        || owned.list != recording.list
        || s.materials().len() > 32768
    {
        return Err(ARGUMENTS);
    }
    let family = crate::families_native::lookup_raw(s.original_pipeline).ok_or(FAMILY)?;
    let snapshot = crate::shader_snapshot_native::source(s.original_pipeline).ok_or(FAMILY)?;
    let mirror = &snapshot.info().mirror;
    if !multilayer_family(&family) || family.source_root.as_raw() != s.root || mirror.identity != s.shader || mirror.root_len == 0 || mirror.vs_len == 0 || mirror.ps_len == 0 {
        return Err(FAMILY);
    }
    let scope = source_scope(s);
    let receipt = s.cbv_receipt();
    if receipt.list != scope.list || receipt.generation != scope.generation || receipt.occurrence != scope.occurrence {
        return Err(SCOPE);
    }
    let mut b4 = None;
    for cbv in s.cbvs() {
        if cbv.parameter == 19 && cbv.offset == 0 {
            if b4.is_some() {
                return Err(SCOPE);
            }
            b4 = Some(cbv);
        }
    }
    let b4 = b4.ok_or(B4)?;
    // SAFETY: the holder's receipt and private CBV (the same records).
    let recorded = unsafe { crate::source_cbv_snapshot_receipt::cvr_scsn_matches_recorded((receipt as *const PreparedCopies).cast(), (b4 as *const PrivateCbv).cast()) };
    let b4_raw = b4.resource;
    // SAFETY: the holder's live resource (or null).
    let gpu = unsafe { ID3D12Resource::from_raw_borrowed(&b4_raw) }.map_or(0, |r| unsafe { r.GetGPUVirtualAddress() });
    if !recorded
        || b4.reflected_bytes != MULTILAYER_B4_BYTES
        || b4.copied_bytes != u64::from(MULTILAYER_B4_BYTES)
        || b4.padded_bytes != 256
        || b4.view.SizeInBytes != 256
        || !membership_range(b4.resource, b4.private_offset, 256)
        || b4.view.BufferLocation != gpu.wrapping_add(b4.private_offset)
    {
        return Err(B4);
    }
    let full = s.full_t5().ok_or(T5)?;
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: the holder's copy and range (structured_buffer_snapshot's records); a local view.
    let viewed = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_recorded_view((&raw const full.copy).cast(), (&raw const full.range).cast(), &mut view) };
    // SAFETY: two descriptions.
    let same = viewed && unsafe { crate::structured_snapshot::cvr_ssv_check(0, &view, &full.view, null()) };
    // SAFETY: a buffer view (checked just below).
    let buffer = unsafe { view.Anonymous.Buffer };
    if full.copy.list != scope.list
        || full.copy.generation != scope.generation
        || full.copy.occurrence != scope.occurrence
        || full.copy.source_offset != 0
        || full.range.source_offset != 0
        || full.copy.copied_bytes != FULT_T5_SOURCE_BYTES
        || full.range.copied_bytes != FULT_T5_SOURCE_BYTES
        || !same
        || view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || buffer.StructureByteStride != 128
        || buffer.NumElements != 8192
        || buffer.Flags.0 != 0
        || view.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || buffer.FirstElement > u64::MAX / 128
        || buffer.FirstElement * 128 != full.range.private_offset
        || !membership_range(full.range.resource, full.range.private_offset, FULT_T5_SOURCE_BYTES)
    {
        return Err(T5);
    }
    let mut table = None;
    for (i, t) in s.reference_tables().iter().enumerate() {
        if t.parameter == 31 {
            if table.is_some() {
                return Err(TABLE);
            }
            table = Some(i);
        }
    }
    let at = table.ok_or(TABLE)?;
    if !coverage(w, index, true, at, &scope, None) {
        return Err(TABLE);
    }
    let extent = s.reference_tables()[at].storage_extent;
    let mut row: MembershipRow = unsafe { core::mem::zeroed() };
    row.source = MlmSource {
        b4: b4.resource,
        t5: full.range.resource,
        b4_offset: b4.private_offset,
        t5_offset: full.range.private_offset,
        t5_bytes: full.range.copied_bytes,
        first_element: 0,
        num_elements: 8192,
        membership: core::ptr::null(),
        source_id: s.ordinal + 1,
        eye: original.side,
    };
    row.family_ok = true;
    row.ordinal = s.ordinal;
    row.native_side = scope.eye;
    row.source_pass_storage_extent = extent;
    row.maximum_bit = u32::MAX;
    let mut offsets = std::collections::BTreeSet::new();
    *bits = [0; 1024];
    for m in s.materials() {
        if m.parameter != 31 {
            continue;
        }
        row.material_slots += 1;
        if !offsets.insert(m.offset) {
            return Err(COLLISION);
        }
        if m.kind != D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32 || m.offset >= MULTILAYER_DECLARED_EXTENT || m.offset >= extent || m.has_buffer || !m.counter.is_null() {
            row.omitted_slots += 1;
            continue;
        }
        if !coverage(w, index, true, at, &scope, Some((m.offset, &m.stamp))) {
            row.omitted_slots += 1;
            continue;
        }
        let mut srv = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
        // SAFETY: the stamp's value and a local view.
        let class = unsafe { crate::borrowed_fixed::cvr_bfd_classify(&m.stamp.value, &mut srv) };
        // borrowed_fixed_detail::DescriptorClass: TypedNull 1, Candidate 2.
        let null_slot = class == 1;
        if null_slot {
            if !m.resource.is_null() || !m.immutable_owner.is_null() || !m.immutable_resource.is_null() {
                row.omitted_slots += 1;
                continue;
            }
        } else {
            let resource = m.resource;
            // SAFETY: the holder's live resource (or null).
            let desc = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.map(|r| unsafe { r.GetDesc() });
            let valid = class == 2
                && !resource.is_null()
                && m.immutable_resource == resource
                // SAFETY: the holder's lease (or null).
                && !m.immutable.is_null()
                && unsafe { crate::texture_immutable::cvr_tr_validate_immutable(m.immutable, null_mut()) }
                && crate::descriptor_audit::cvr_sda_audit_held_cookie(resource) == m.stamp.value[2]
                && m.stamp.value[2] != 0
                // SAFETY: the stamp's value, a live resource's description and the classified view.
                && desc.is_some_and(|d| unsafe { crate::borrowed_fixed::cvr_bfd_view_matches(&m.stamp.value, &d, &srv) });
            if !valid {
                row.omitted_slots += 1;
                continue;
            }
        }
        // SAFETY: the bank's bits and the row's counters.
        if !unsafe { crate::multilayer_selectors::cvr_ml_mark(bits, m.offset, extent, &mut row.covered_slots, &mut row.maximum_bit) } {
            return Err(CAPACITY);
        }
        if null_slot {
            row.null_slots += 1;
        } else {
            row.immutable_slots += 1;
        }
    }
    if row.covered_slots + row.omitted_slots != row.material_slots || !coverage(w, index, true, at, &scope, None) {
        return Err(COVERAGE);
    }
    row.b4 = b4.resource;
    row.b4_offset = b4.private_offset;
    row.t5 = full.range.resource;
    row.t5_offset = full.range.private_offset;
    row.t5_bytes = full.range.copied_bytes;
    Ok(row)
}

fn build_bank(w: &Work, candidate: &mut Pair) -> bool {
    let Some(family) = candidate.family.clone() else { return false };
    let mut pairs = Vec::new();
    let (mut b4_checks, mut table_checks) = (0u32, 0u32);
    for location in family.bindings() {
        let mut pair = BankPairRow { binding: location.binding, eyes: [zero_value(), zero_value()] };
        for eye in 0..2 {
            let index = candidate.sources[eye] as usize;
            let source = w.source(index);
            let value = &mut pair.eyes[eye];
            if location.binding.kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV {
                let Some(found) = source.cbvs().iter().position(|c| c.parameter == location.source_parameter && c.offset == location.source_offset) else {
                    return false;
                };
                let cbv = &source.cbvs()[found];
                if cbv.resource.is_null() || cbv.owner.is_null() {
                    return false;
                }
                if w.actual_membership && location.source_parameter == 19 && location.source_offset == 0 {
                    if membership_b4(w, index) != Some(found) {
                        return false;
                    }
                    b4_checks += 1;
                }
                value.source = cbv.resource;
                value.owner = cbv.owner;
                value.gpu_cbv = cbv.view;
            } else if location.binding.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV {
                let Some(m) = material_at(source, location.source_parameter, location.source_offset) else { return false };
                let material = &source.materials()[m];
                let Some(decoded) = crate::descriptor_views::decode_srv(&material.stamp.value, None) else { return false };
                value.srv = decoded.view;
                if material.resource.is_null() {
                    if decoded.implicit {
                        return false;
                    }
                    continue;
                }
                value.borrowed_srv = true;
                let t5_slot = location.source_parameter == 23 && location.source_offset == 5;
                if w.actual_membership && t5_slot && material.has_buffer {
                    return false;
                }
                if material.has_buffer {
                    if !material_matches(w, index, m) {
                        return false;
                    }
                    value.source = material.buffer_resource;
                    value.owner = material.buffer_owner;
                    value.srv = material.buffer_view;
                } else if t5_slot && source.full_t5().is_some() {
                    if !full_t5_view(w, index) {
                        return false;
                    }
                    let full = source.full_t5().expect("full T5");
                    value.source = full.range.resource;
                    value.owner = full.copy.owner;
                    value.srv = full.view;
                } else if let (true, Some(t5)) = (t5_slot, source.t5()) {
                    if !source.input_checks().iter().any(|check| {
                        check.resource == t5.resource && check.owner == t5.owner && check.offset == t5.offset && check.expected_len as u64 == t5.bytes
                    }) {
                        return false;
                    }
                    value.source = t5.resource;
                    value.owner = t5.owner;
                    value.srv = t5.srv;
                } else {
                    // SAFETY: the holder's material.
                    if material.immutable_owner.is_null() || !immutable_valid(w, index, m, null_mut()) {
                        return false;
                    }
                    value.source = material.resource;
                    value.owner = material.immutable_owner;
                    value.implicit_srv = decoded.implicit;
                }
            } else {
                return false;
            }
        }
        pairs.push(pair);
    }
    let mut shared_parameters = std::collections::BTreeSet::new();
    for use_ in family.uses() {
        let paired = use_.space == 0
            && use_.count == 1
            && family.bindings().iter().any(|value| {
                value.binding.kind.0 as u32 == use_.kind && value.binding.visibility.0 as u32 == use_.visibility && value.binding.source_register == use_.shader_register
            });
        if paired {
            continue;
        }
        if use_.kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 as u32 || use_.kind == D3D12_DESCRIPTOR_RANGE_TYPE_UAV.0 as u32 {
            return false;
        }
        let Some((parameter, _)) = crate::descriptor_audit::find_binding(family.view.source_root as u64, use_.kind, use_.space, use_.shader_register, use_.visibility) else {
            return false;
        };
        shared_parameters.insert(parameter);
    }
    let mut shared = Vec::new();
    for parameter in shared_parameters {
        let (left, right) = (w.source(candidate.sources[0] as usize), w.source(candidate.sources[1] as usize));
        let (Some(a), Some(b)) = (left.tables().iter().position(|t| t.parameter == parameter), right.tables().iter().position(|t| t.parameter == parameter)) else {
            return false;
        };
        let (ta, tb) = (&left.tables()[a], &right.tables()[b]);
        let mut count = 0u32;
        // SAFETY: local keys.
        if !unsafe { crate::prepared_pair_ranges::cvr_npp_shared_table(&table_key(ta), &table_key(tb), &mut count) } {
            return false;
        }
        let mut table = OwnedTable::of(ta);
        table.count = count;
        table.leases.extend_from_slice(table_leases(tb));
        if w.actual_membership && parameter == 31 {
            for eye in 0..2 {
                let index = candidate.sources[eye] as usize;
                let summary = &w.membership_banks[index];
                if (summary.maximum_bit != u32::MAX && summary.maximum_bit >= table.count) || !membership_table(w, index, if eye != 0 { b } else { a }, table.count) {
                    return false;
                }
                table_checks += 1;
            }
        }
        for source in [left, right] {
            for material in source.materials() {
                if !material.immutable_owner.is_null() {
                    table.leases.push(material.immutable_owner);
                }
            }
        }
        shared.push(table);
    }
    if w.actual_membership && (b4_checks != 2 || table_checks != 2) {
        return false;
    }
    candidate.actual_bindings = w.actual_membership;
    candidate.bindings = pairs;
    candidate.shared = shared;
    true
}
fn zero_value() -> Value {
    // SAFETY: a plain value whose all-zero form is BankValue's default (COMMON state, no source).
    unsafe { core::mem::zeroed() }
}
/// BuildStereoMeshPlan: the longest common subsequence of the eyes' pair ids (ids 0 never match), as the draws of
/// both eyes in order.
fn build_stereo_mesh_plan(first: &[u64], second: &[u64]) -> Option<MeshPlan> {
    const DRAWS_PER_EYE: usize = 32768;
    const MATCHING_PAIRS: usize = 262144;
    if first.len() > DRAWS_PER_EYE || second.len() > DRAWS_PER_EYE {
        return None;
    }
    let mut positions: std::collections::HashMap<u64, Vec<u32>> = std::collections::HashMap::with_capacity(second.len());
    for (j, &id) in second.iter().enumerate() {
        if id != 0 {
            positions.entry(id).or_default().push(j as u32);
        }
    }
    let mut match_count = 0usize;
    for &id in first {
        if id == 0 {
            continue;
        }
        let Some(found) = positions.get(&id) else { continue };
        if found.len() > MATCHING_PAIRS - match_count {
            return None;
        }
        match_count += found.len();
    }
    struct Match {
        first: u32,
        second: u32,
        previous: u32,
    }
    let mut matches: Vec<Match> = Vec::with_capacity(match_count);
    let mut tails: Vec<u32> = Vec::new();
    let mut tail_nodes: Vec<u32> = Vec::new();
    for (i, &id) in first.iter().enumerate() {
        if id == 0 {
            continue;
        }
        let Some(found) = positions.get(&id) else { continue };
        // Descending positions prevent a duplicate in one eye from matching more than one occurrence in the other eye
        // in the same chain.
        for &j in found.iter().rev() {
            let at = tails.partition_point(|&t| t < j);
            if at < tails.len() && tails[at] == j {
                continue;
            }
            let previous = if at != 0 { tail_nodes[at - 1] } else { u32::MAX };
            matches.push(Match { first: i as u32, second: j, previous });
            let node = (matches.len() - 1) as u32;
            if at == tails.len() {
                tails.push(j);
                tail_nodes.push(node);
            } else {
                tails[at] = j;
                tail_nodes[at] = node;
            }
        }
    }
    let mut shared = Vec::new();
    let mut at = tail_nodes.last().copied().unwrap_or(u32::MAX);
    while at != u32::MAX {
        let m = &matches[at as usize];
        shared.push([m.first, m.second]);
        at = m.previous;
    }
    shared.reverse();
    let mut result = MeshPlan { draws: Vec::with_capacity(first.len() + second.len() - shared.len()), common: shared.len() as u32 };
    let (mut i, mut j) = (0u32, 0u32);
    let singles = |result: &mut MeshPlan, i: &mut u32, j: &mut u32, end_first: u32, end_second: u32| {
        while *i < end_first {
            result.draws.push(Step { mask: 1, source: [*i, u32::MAX] });
            *i += 1;
        }
        while *j < end_second {
            result.draws.push(Step { mask: 2, source: [u32::MAX, *j] });
            *j += 1;
        }
    };
    for pair in &shared {
        singles(&mut result, &mut i, &mut j, pair[0], pair[1]);
        result.draws.push(Step { mask: 3, source: [i, j] });
        i += 1;
        j += 1;
    }
    singles(&mut result, &mut i, &mut j, first.len() as u32, second.len() as u32);
    Some(result)
}
fn prepare_geometry(w: &mut Work) -> bool {
    let device = raw_of(&w.device);
    let count = w.view_count;
    let mut reference = Vec::with_capacity(count);
    let mut source_tables: Vec<Vec<OwnedTable>> = Vec::with_capacity(count);
    let mut heaps: Vec<windows::core::IUnknown> = Vec::new();
    let mut ids = vec![0u64; count];
    for index in 0..count {
        let source = w.source(index);
        reference.push(SgcSource { geometry: copy_resources(&source.owned), indirect: null() });
        if source.reference_tables().is_empty() {
            return false;
        }
        let mut tables: Vec<OwnedTable> = source.reference_tables().iter().map(OwnedTable::of).collect();
        for table in &mut tables {
            if w.actual_membership && table.parameter == 19 {
                let bank = source_tables.len();
                let b4 = membership_b4(w, index).map(|at| &source.cbvs()[at]);
                let input = w.membership_banks.get(bank);
                let Some(b4) = b4 else { return membership_error(3, source.ordinal, 0) };
                if table.count != 1 || !input.is_some_and(|row| row.b4 == b4.resource && row.b4_offset == b4.private_offset) {
                    return membership_error(3, source.ordinal, 0);
                }
                let mut heap = null_mut();
                // SAFETY: a live device; `heap` receives a reference.
                if !unsafe { crate::prepared_pair_objects::cvr_npp_cbv_heap(device, &b4.view, &mut heap) } {
                    return membership_error(3, source.ordinal, 0);
                }
                let held = com(heap).expect("heap");
                table.cpu_heap = held.as_raw();
                heaps.push(held);
                table.offset = 0;
                table.identity = 0;
                table.version = 0;
                table.storage_scope = 0;
                table.storage_extent = 0;
                table.leases.push(b4.resource);
                table.leases.push(b4.owner);
                table.leases.push(source.cbv_receipt().owner);
                let bound = &mut ex!(CyberpunkVR_PreparedActualMembershipRows)[bank * 32 + 15];
                if *bound != 0 {
                    return membership_error(3, source.ordinal, 0);
                }
                *bound = 1;
                ex!(CyberpunkVR_PreparedActualMembershipStats)[19] += 1;
                continue;
            }
            let overrides: Vec<usize> = source
                .materials()
                .iter()
                .enumerate()
                .filter(|(_, m)| m.has_buffer && m.parameter == table.parameter)
                .map(|(i, _)| i)
                .collect();
            let full_t5 = table.parameter == 23 && source.full_t5().is_some();
            if overrides.is_empty() && !full_t5 {
                continue;
            }
            // SAFETY: the table's live CPU heap.
            let description = unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&table.cpu_heap) }.map(|h| unsafe { h.GetDesc() });
            if description.is_none_or(|d| d.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) || table.count != 8 || table.parameter != 23 {
                return false;
            }
            let mut views = Vec::with_capacity(overrides.len() + 1);
            for &m in &overrides {
                let material = &source.materials()[m];
                if material.offset >= table.count || !material_matches(w, index, m) {
                    return false;
                }
                views.push(crate::prepared_pair_objects::TableSrv { index: material.offset, resource: material.buffer_resource, view: material.buffer_view });
                table.leases.push(material.buffer_owner);
            }
            if full_t5 {
                if !full_t5_view(w, index) {
                    return false;
                }
                let full = source.full_t5().expect("full T5");
                views.push(crate::prepared_pair_objects::TableSrv { index: 5, resource: full.range.resource, view: full.view });
                table.leases.push(full.copy.owner);
                table.leases.push(full.range.owner);
            }
            let mut heap = null_mut();
            // SAFETY: the table's descriptors and the views; `heap` receives a reference.
            if !unsafe { crate::prepared_pair_objects::cvr_npp_table_heap(device, table.cpu_heap, table.offset, table.count, views.as_ptr(), views.len(), &mut heap) } {
                return false;
            }
            let held = com(heap).expect("heap");
            table.cpu_heap = held.as_raw();
            heaps.push(held);
            table.offset = 0;
            table.identity = 0;
            table.version = 0;
            table.storage_scope = 0;
            table.storage_extent = 0;
        }
        source_tables.push(tables);
    }
    if w.actual_membership && ex!(CyberpunkVR_PreparedActualMembershipStats)[19] != count as u64 {
        return membership_error(3, u64::MAX, 0);
    }
    let lent_tables: Vec<Vec<Table>> = source_tables.iter().map(|tables| tables.iter().map(OwnedTable::lend).collect()).collect();
    let banks: Vec<SourceInput> = (0..count)
        .map(|i| {
            let source = w.source(i);
            SourceInput {
                root: source.root,
                serialized: source.shader_root,
                serialized_len: source.shader_root_len,
                tables: lent_tables[i].as_ptr(),
                table_count: lent_tables[i].len(),
            }
        })
        .collect();
    let mut created = null_mut();
    // SAFETY: the lent sources, alive through the call.
    if unsafe { cvr_sgc_create_sources(device, reference.as_ptr(), reference.len(), describe_signature, &mut created) }.is_err() {
        return false;
    }
    w.reference_geometry = com(created);
    let mut created = null_mut();
    let mut failure = [0u64; 3];
    // SAFETY: as above.
    if unsafe { cvr_bank_create_source_pass(device, banks.as_ptr(), banks.len(), &mut failure, &mut created) }.is_err() {
        return false;
    }
    w.reference_bank = com(created);
    drop(heaps);
    let mut pairs = core::mem::take(&mut w.pairs);
    for (i, pair) in pairs.iter_mut().enumerate() {
        pair.ready = build_bank(w, pair);
        if !pair.ready {
            ex!(CyberpunkVR_PreparedBatchStats)[10] += 1;
            if !w.sequence {
                w.pairs = pairs;
                return false;
            }
            continue;
        }
        ids[pair.sources[0] as usize] = i as u64 + 1;
        ids[pair.sources[1] as usize] = i as u64 + 1;
    }
    w.pairs = pairs;
    let eye_ids: [Vec<u64>; 2] = [w.eyes[0].iter().map(|&s| ids[s as usize]).collect(), w.eyes[1].iter().map(|&s| ids[s as usize]).collect()];
    match build_stereo_mesh_plan(&eye_ids[0], &eye_ids[1]) {
        Some(plan) if plan.common != 0 => w.plan = plan,
        _ => return false,
    }
    let mut paired = Vec::new();
    let mut cursor = [0u32; 2];
    for step in &w.plan.draws {
        for eye in 0..2 {
            if step.mask & (1 << eye) != 0 {
                if step.source[eye] != cursor[eye] {
                    return false;
                }
                cursor[eye] += 1;
            }
        }
        if step.mask != 3 {
            continue;
        }
        let at = (eye_ids[0][step.source[0] as usize] - 1) as usize;
        let command = paired.len() as u32;
        let candidate = &mut w.pairs[at];
        candidate.command = command;
        if w.actual_membership {
            if !candidate.actual_bindings {
                return membership_error(4, u64::MAX, 0);
            }
            ex!(CyberpunkVR_PreparedActualMembershipStats)[20] += 2;
        }
        paired.push(at);
    }
    if cursor[0] as usize != w.eyes[0].len() || cursor[1] as usize != w.eyes[1].len() {
        return false;
    }
    let shared: Vec<Vec<Table>> = paired.iter().map(|&at| w.pairs[at].shared.iter().map(OwnedTable::lend).collect()).collect();
    let draws: Vec<DrawInput> = paired
        .iter()
        .zip(&shared)
        .map(|(&at, tables)| {
            let pair = &w.pairs[at];
            let family = pair.family.as_ref().expect("family");
            DrawInput {
                root: family.view.root,
                plan: &family.view.plan,
                pairs: pair.bindings.as_ptr(),
                pair_count: pair.bindings.len(),
                shared: tables.as_ptr(),
                shared_count: tables.len(),
                source_root: null_mut(),
            }
        })
        .collect();
    let mut created = null_mut();
    // SAFETY: the lent draws, alive through the call.
    if unsafe { cvr_bank_create_pass(device, draws.as_ptr(), draws.len(), &mut created) }.is_err() {
        return false;
    }
    w.paired_bank = com(created);
    let (reference_counts, paired_counts) = (bank_counts(&w.reference_bank), bank_counts(&w.paired_bank));
    *ex!(CyberpunkVR_PreparedBatchDescriptors) =
        [u64::from(reference_counts.descriptors), u64::from(paired_counts.descriptors), u64::from(paired_counts.draws), u64::from(paired_counts.snapshots)];
    let batch = ex!(CyberpunkVR_PreparedBatchStats);
    batch[3] = u64::from(w.plan.common);
    batch[6] = w.plan.draws.len() as u64;
    batch[11] = 1;
    // SAFETY: as ex!.
    unsafe { CyberpunkVR_PreparedBatchPlanCount = w.plan.draws.len() as u32 };
    let ordinals: [Vec<u64>; 2] = [
        w.eyes[0].iter().map(|&s| w.source(s as usize).ordinal).collect(),
        w.eyes[1].iter().map(|&s| w.source(s as usize).ordinal).collect(),
    ];
    let commands: Vec<u64> =
        eye_ids[0].iter().map(|&id| if id != 0 && id <= w.pairs.len() as u64 { u64::from(w.pairs[(id - 1) as usize].command) } else { u64::MAX }).collect();
    // SAFETY: the plan, the rows and the exports.
    unsafe {
        crate::prepared_pair_exports::cvr_npp_plan_rows(
            w.plan.draws.as_ptr(),
            w.plan.draws.len(),
            ordinals[0].as_ptr(),
            ordinals[0].len(),
            ordinals[1].as_ptr(),
            ordinals[1].len(),
            commands.as_ptr(),
            commands.len(),
            addr_of_mut!(CyberpunkVR_PreparedBatchPlan).cast(),
            addr_of_mut!(CyberpunkVR_PreparedBatchStats).cast(),
        )
    };
    true
}
fn copy_resources(r: &Resources) -> Resources {
    Resources { index: r.index, vertices: r.vertices, draw: r.draw, instances: r.instances, instance_count: r.instance_count }
}
fn prepare_inputs(w: &mut Work) -> bool {
    static ACCEPTED: u64 = 1;
    let device = raw_of(&w.device);
    let mut pieces = Vec::new();
    if w.actual_membership {
        let mut lent = Vec::with_capacity(w.membership_banks.len());
        w.membership_arrays.clear();
        for row in &w.membership_banks {
            let mut array = Box::new([0u32; 1024]);
            // SAFETY: the bank's 1024 membership words.
            if !row.source.membership.is_null() {
                array.copy_from_slice(unsafe { core::slice::from_raw_parts(row.source.membership, 1024) });
            }
            if w.membership_fault {
                array.fill(0);
            }
            if !row.family_ok {
                return membership_error(5, u64::MAX, 0);
            }
            lent.push(MlmSource { membership: array.as_ptr(), ..copy_mlm(&row.source) });
            w.membership_arrays.push(array);
        }
        let mut created = null_mut();
        // SAFETY: the lent sources, alive through the call.
        let hr = if lent.is_empty() {
            E_FAIL
        } else {
            unsafe { cvr_mlm_create(device, lent.as_ptr(), lent.len(), raw_of(&w.predicate), 0, &mut created) }
        };
        if hr.is_err() {
            return membership_error(5, u64::MAX, 0);
        }
        w.membership = com(created);
        let bytes = u64::from(mlm_info(&w.membership).sources) * 48;
        if readback(device, bytes, &mut w.membership_readback).is_err() {
            return membership_error(5, u64::MAX, 0);
        }
        let predicates = mlm_info(&w.membership).predicates;
        for index in 0..lent.len() as u64 {
            pieces.push(crate::prepared_pair_record::InputPiece { resource: predicates, offset: index * 16, data: (&ACCEPTED as *const u64).cast(), bytes: 8, align: false });
        }
        ex!(CyberpunkVR_PreparedPairInputStats)[0] = lent.len() as u64;
        ex!(CyberpunkVR_PreparedActualMembershipStats)[8] = bytes;
    } else {
        let mut total = 0usize;
        for source in w.sources() {
            if !source.input_checks().is_empty() {
                ex!(CyberpunkVR_PreparedPairInputStats)[0] += 1;
            }
            for input in source.input_checks() {
                if !crate::prepared_pair_ranges::cvr_npp_input_admitted(!input.resource.is_null(), !input.owner.is_null(), input.expected_len, total) {
                    return false;
                }
                pieces.push(crate::prepared_pair_record::InputPiece {
                    resource: input.resource,
                    offset: input.offset,
                    data: input.expected,
                    bytes: input.expected_len,
                    align: true,
                });
                total = (total + input.expected_len + 15) & !15;
            }
        }
    }
    if pieces.is_empty() {
        return true;
    }
    let mut rows = vec![CompareRange::default(); pieces.len()];
    let (mut upload, mut bytes) = (null_mut(), 0u64);
    let _internal = cvr_host::probe::Internal::new();
    let fault = !w.actual_membership && CyberpunkVR_PreparedPairInputFault.load(Relaxed) != 0;
    // SAFETY: the pieces and the rows; `upload` receives a reference.
    let created = unsafe { crate::prepared_pair_record::cvr_npp_expected_inputs(device, pieces.as_ptr(), pieces.len(), fault, rows.as_mut_ptr(), &mut upload, &mut bytes) };
    if !upload.is_null() {
        // SAFETY: the reference handed out.
        w.expected_inputs = Some(unsafe { ID3D12Resource::from_raw(upload) });
    }
    if !created {
        return false;
    }
    if w.inputs.create(device, 1, &rows, raw_of(&w.predicate)).is_err() {
        return false;
    }
    let counts = w.inputs.counts();
    let stats = ex!(CyberpunkVR_PreparedPairInputStats);
    stats[1] = rows.len() as u64;
    stats[2] = counts[3];
    stats[4] = bytes;
    stats[7] = 16;
    true
}
fn copy_mlm(s: &MlmSource) -> MlmSource {
    MlmSource {
        b4: s.b4,
        t5: s.t5,
        b4_offset: s.b4_offset,
        t5_offset: s.t5_offset,
        t5_bytes: s.t5_bytes,
        first_element: s.first_element,
        num_elements: s.num_elements,
        membership: s.membership,
        source_id: s.source_id,
        eye: s.eye,
    }
}
fn prepare_mesh(w: &mut Work) -> bool {
    let pairs: Vec<MeshPair> = w.pairs.iter().map(|p| MeshPair { command: p.command, sources: p.sources }).collect();
    let sources: Vec<MeshSource> = w
        .sources()
        .iter()
        .map(|s| {
            let mut leases = [GeometryLease { resource: null_mut(), gpu: 0, bytes: 0 }; 17];
            leases[0] = s.owned.index;
            leases[1..].copy_from_slice(&s.owned.vertices);
            MeshSource { draw: s.owned.draw, leases }
        })
        .collect();
    let mut rows = vec![CompareRange::default(); pairs.len() * 17];
    let mut count = 0usize;
    // SAFETY: the lent pairs and sources; `rows` holds its capacity.
    if !unsafe { crate::prepared_pair_record::cvr_npp_mesh_ranges(pairs.as_ptr(), pairs.len(), sources.as_ptr(), sources.len(), rows.as_mut_ptr(), rows.len(), &mut count) } {
        return false;
    }
    rows.truncate(count);
    let device = raw_of(&w.device);
    let draws = bank_counts(&w.paired_bank).draws;
    if w.mesh.create(device, draws, &rows, w.draw_permission()).is_err() {
        return false;
    }
    let pair_count = w.mesh.counts()[0];
    let bytes = if !w.inputs.predicates().is_null() { INPUT_READBACK_OFFSET + 16 } else { MESH_READBACK_OFFSET + pair_count * 16 };
    if readback(device, bytes, &mut w.query_readback).is_err() {
        return false;
    }
    let counts = w.mesh.counts();
    w.mesh_bytes = counts[3] * 2;
    let stats = ex!(CyberpunkVR_PreparedPairMeshStats);
    stats[0] = counts[0];
    stats[1] = counts[1];
    stats[2] = counts[3];
    stats[7] = counts[0] * 16;
    true
}
fn compatibility_failure(a: &SourceView, b: &SourceView) -> u32 {
    let (x, y) = (a.original(), b.original());
    // SAFETY: the sources' draws.
    let failure = unsafe { crate::prepared_pair_checks::cvr_npp_draw_compatibility(x, y, a.draw().vertices[7].StrideInBytes, b.draw().vertices[7].StrideInBytes) };
    if failure != 0 {
        return failure;
    }
    let (p, q) = (&a.owned_state, &b.owned_state);
    if !same_state(p, q) {
        let mismatch = ex!(CyberpunkVR_PreparedPairStateMismatch);
        if mismatch[2] == 0 {
            let values = [
                a.ordinal,
                b.ordinal,
                p.pipeline as u64,
                q.pipeline as u64,
                p.topology as u32 as u64,
                q.topology as u32 as u64,
                u64::from(p.stencil),
                u64::from(q.stencil),
                u64::from(p.rate.known),
                u64::from(q.rate.known),
                p.rate.value as u32 as u64,
                q.rate.value as u32 as u64,
                p.rate.image as u64,
                q.rate.image as u64,
                p.rate_image as u64,
                q.rate_image as u64,
                p.rate.combiners[0] as u32 as u64,
                p.rate.combiners[1] as u32 as u64,
                q.rate.combiners[0] as u32 as u64,
                q.rate.combiners[1] as u32 as u64,
            ];
            mismatch[..20].copy_from_slice(&values);
            // SAFETY: plain floats' bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(p.blend.as_ptr().cast::<u8>(), mismatch[20..].as_mut_ptr().cast::<u8>(), 16);
                core::ptr::copy_nonoverlapping(q.blend.as_ptr().cast::<u8>(), mismatch[22..].as_mut_ptr().cast::<u8>(), 16);
            }
        }
        return 8;
    }
    // SAFETY: the sources' draws.
    if !unsafe { crate::prepared_pair_checks::cvr_npp_vertex_slots(x, y) } {
        return 9;
    }
    let image = |s: &SourceView| s.owned_state.rate.image != 0;
    if image(a) != !a.rate_image.is_null() || image(b) != !b.rate_image.is_null() || a.rate_image.is_null() != b.rate_image.is_null() {
        return 10;
    }
    if !a.rate_image.is_null() {
        // SAFETY: live rate images.
        let desc = |r: *mut c_void| unsafe { ID3D12Resource::from_raw_borrowed(&r) }.map(|r| unsafe { r.GetDesc() });
        let (p, q) = (desc(a.rate_image), desc(b.rate_image));
        if p.map(|d| (d.Width, d.Height)) != q.map(|d| (d.Width, d.Height)) {
            return 10;
        }
    }
    if !has_baseline(a) || !has_baseline(b) {
        return 11;
    }
    0
}

// ---- the destination recording

/// The native list's scene state, restored after the pass's commands at an admitted draw.
struct Restore {
    list: *mut c_void,
    draw: DrawRecord,
    bindings: Box<cvr_gpu::scene_state::Bindings>,
    views: Option<ID3D12GraphicsCommandList1>,
    rate: Option<ID3D12GraphicsCommandList5>,
    valid: bool,
}
impl Restore {
    fn new(list: *mut c_void, original: &DrawRecord) -> Restore {
        let mut bindings = Box::<cvr_gpu::scene_state::Bindings>::default();
        // SAFETY: a live list.
        let native = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.cloned();
        let views = native.as_ref().and_then(|l| l.cast::<ID3D12GraphicsCommandList1>().ok());
        let rate = native.as_ref().and_then(|l| l.cast::<ID3D12GraphicsCommandList5>().ok());
        let draw = *original;
        // SAFETY: `bindings` is writable.
        let valid = (draw.flags & 191) == 191
            && draw.flags & 64 == 0
            && unsafe { cvr_scene_capture_source_bindings(list as usize, &mut *bindings) }
            && bindings.root as u64 == draw.root
            && views.is_some();
        Restore { list, draw, bindings, views, rate, valid }
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        if !self.valid {
            return;
        }
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the live list whose state is restored.
        unsafe {
            cvr_scene_bindings_restore(&*self.bindings, self.list);
            let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&self.list) else { return };
            if self.bindings.compute_root == 0 {
                list.SetComputeRootSignature(None);
            }
            if let Some(views) = &self.views {
                views.SetViewInstanceMask(u32::MAX);
            }
            let d = &self.draw;
            let pipeline = d.pipeline as *mut c_void;
            list.SetPipelineState(ID3D12PipelineState::from_raw_borrowed(&pipeline));
            list.IASetIndexBuffer(Some(&d.index));
            list.IASetVertexBuffers(0, Some(&d.vertices));
            list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            if let (Some(rate), true) = (&self.rate, self.bindings.rate.known) {
                let combiners = [D3D12_SHADING_RATE_COMBINER(self.bindings.rate.combiners[0]), D3D12_SHADING_RATE_COMBINER(self.bindings.rate.combiners[1])];
                rate.RSSetShadingRate(D3D12_SHADING_RATE(self.bindings.rate.value), Some(combiners.as_ptr()));
                let image = self.bindings.rate.image as *mut c_void;
                rate.RSSetShadingRateImage(ID3D12Resource::from_raw_borrowed(&image));
            }
            let colors: [D3D12_CPU_DESCRIPTOR_HANDLE; 8] = core::array::from_fn(|i| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: d.targets[i] as usize });
            let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: d.depth as usize };
            list.OMSetRenderTargets(
                d.rt_count,
                if d.rt_count != 0 { Some(colors.as_ptr()) } else { None },
                d.rt_contiguous != 0,
                if d.depth != 0 { Some(&depth) } else { None },
            );
            list.RSSetViewports(&[d.viewport]);
            list.RSSetScissorRects(&[d.scissor]);
        }
    }
}
fn capture_destination(w: &mut Work, list: *mut c_void) -> bool {
    let context = crate::camera_upload::current(cvr_engine::code::image_base() as u64);
    w.recording = Recording::default();
    w.queue = None;
    w.submitted = false;
    w.uses_op(0, null_mut());
    // SAFETY: a live list; the lease is writable.
    if !unsafe { cvr_native_recording_capture(list, &context, &mut w.recording.0) } {
        return false;
    }
    // SAFETY: a live device.
    match w.device.as_ref().map(|d| unsafe { d.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) {
        Some(Ok(fence)) => w.fence = Some(fence),
        _ => return false,
    }
    let stats = ex!(CyberpunkVR_PreparedPairStats);
    stats[12] = w.recording.0.list;
    stats[13] = w.recording.0.generation;
    true
}
unsafe extern "C-unwind" fn image_keep(_: *mut c_void, list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: forwarded.
    unsafe { keep(list, object) }
}
unsafe extern "C-unwind" fn image_internal(user: *mut c_void) {
    // SAFETY: image_copies' scope slot.
    unsafe { *(user as *mut Option<cvr_host::probe::Internal>) = Some(cvr_host::probe::Internal::new()) };
}
fn image_copies(w: &mut Work, list: *mut c_void) -> bool {
    let native = !w.native_exits[0].is_null();
    let mut t = crate::prepared_pair_record::ImageTargets {
        chains: [[null_mut(); 5]; 3],
        baselines: [[null_mut(); 5]; 2],
        colors: w.colors,
        width: w.width,
        height: w.height,
        versions: if native { 4 } else { 3 },
    };
    for slot in 0..5u32 {
        if slot < w.colors || slot == 4 {
            t.chains[0][slot as usize] = chain_resource(&w.reference_targets, slot);
            t.chains[1][slot as usize] = chain_resource(&w.paired_targets, slot);
            if native {
                t.chains[2][slot as usize] = chain_resource(&w.native_rate_targets, slot);
            }
        }
    }
    let sources = slice(w.views, w.view_count);
    for eye in 0..2 {
        let baseline = &sources[w.eyes[eye][0] as usize].baseline_targets;
        for slot in 0..5 {
            t.baselines[eye][slot] = baseline.resources[slot] as *mut c_void;
        }
    }
    let mut rows = [Plane::default(); 10];
    let mut out = crate::prepared_pair_record::ImageOut { planes: rows.as_mut_ptr(), capacity: rows.len(), count: 0, readback: null_mut(), bytes: 0 };
    let mut internal: Option<cvr_host::probe::Internal> = None;
    let image_host = crate::prepared_pair_record::ImageHost { user: (&mut internal as *mut Option<cvr_host::probe::Internal>).cast(), keep: image_keep, internal: image_internal };
    // SAFETY: the lent targets and the planes' room.
    let copied = unsafe { crate::prepared_pair_record::cvr_npp_image_copies(raw_of(&w.device), list, &t, &mut out, &image_host) };
    w.planes.extend_from_slice(&rows[..out.count]);
    if !out.readback.is_null() {
        // SAFETY: the reference handed out.
        w.image_readback = Some(unsafe { ID3D12Resource::from_raw(out.readback) });
    }
    if out.bytes != 0 {
        w.image_bytes = out.bytes;
    }
    drop(internal);
    copied
}
unsafe extern "C-unwind" fn draw_targets(user: *mut c_void, list: *mut c_void, chain: u32, mask: u32) -> bool {
    // SAFETY: record_commands' work.
    let w = unsafe { &*(user as *const Work) };
    let target = match chain {
        0 => &w.reference_targets,
        1 => &w.paired_targets,
        _ => &w.native_rate_targets,
    };
    target.as_ref().is_some_and(|c| c.bind(list, mask, w.colors))
}
fn record_source_geometry(w: &Work, list: *mut c_void, index: u32) -> bool {
    w.reference_geometry.as_ref().is_some_and(|g| {
        // SAFETY: the live geometry commands.
        !list.is_null() && unsafe { cvr_sgc_record(g.as_raw(), list, index, true, keep, describe_signature) }
    })
}
unsafe extern "C-unwind" fn draw_source(user: *mut c_void, list: *mut c_void, index: u32) -> bool {
    // SAFETY: as above.
    let w = unsafe { &*(user as *const Work) };
    w.reference_bank.as_ref().is_some_and(|b| !list.is_null() && unsafe { cvr_bank_bind_source(b.as_raw(), list, index, keep) })
        && record_source_geometry(w, list, index)
}
unsafe extern "C-unwind" fn draw_paired(user: *mut c_void, list: *mut c_void, command: u32, geometry: u32) -> bool {
    // SAFETY: as above.
    let w = unsafe { &*(user as *const Work) };
    w.paired_bank.as_ref().is_some_and(|b| !list.is_null() && unsafe { cvr_bank_bind(b.as_raw(), list, u32::MAX, command, has) })
        && record_source_geometry(w, list, geometry)
}
fn record_commands(w: &mut Work, list: *mut c_void, views: *mut c_void, rate: *mut c_void) -> u32 {
    if list.is_null() || views.is_null() || (!w.rates.image().is_null() && rate.is_null()) {
        return 3;
    }
    let sources = slice(w.views, w.view_count);
    for source in sources {
        for &owner in source.owners() {
            if !owner.is_null() && !keep_all(list, &[owner]) {
                return 4;
            }
        }
    }
    for source in sources {
        if !source.rate_image.is_null() && !keep_all(list, &[source.rate_image]) {
            return 4;
        }
    }
    if !keep_all(list, &[raw_of(&w.fence), raw_of(&w.predicate)]) {
        return 4;
    }
    for source in sources {
        if !keep_all(list, &[source.owned_state.pipeline]) {
            return 4;
        }
    }
    for pair in &w.pairs {
        if pair.command != u32::MAX && !keep_all(list, &[pair.family.as_ref().map_or(null_mut(), |f| f.view.pipeline)]) {
            return 4;
        }
    }
    if !keep_all(list, &[raw_of(&w.pipeline_queries), raw_of(&w.occlusion_queries), raw_of(&w.query_readback)]) {
        return 4;
    }
    w.stage = 4; // Recording may now leave GPU commands in the destination.
    if w.actual_membership {
        let recorded = w.membership.as_ref().is_some_and(|m| unsafe { cvr_mlm_record(m.as_raw(), list, keep) });
        if !keep_all(list, &[raw_of(&w.membership_readback)]) || !recorded {
            membership_error(7, u64::MAX, 0);
            return 47;
        }
        ex!(CyberpunkVR_PreparedActualMembershipStats)[7] = u64::from(mlm_info(&w.membership).dispatches);
    }
    if !w.inputs.predicates().is_null() {
        if !w.actual_membership {
            for source in sources {
                for input in source.input_checks() {
                    if !keep_all(list, &[input.owner]) {
                        return 30;
                    }
                }
            }
        }
        if !w.inputs.record(list) {
            return 30;
        }
        ex!(CyberpunkVR_PreparedPairInputStats)[3] = w.inputs.counts()[2];
    }
    if !w.mesh.record(list) {
        return 2;
    }
    ex!(CyberpunkVR_PreparedPairMeshStats)[3] = w.mesh.counts()[2];
    if !w.rates.image().is_null() {
        // SAFETY: a live list.
        if !unsafe { cvr_finer_vrs_record(w.rates.0, list) } {
            return 31;
        }
        ex!(CyberpunkVR_PreparedPairVrsGpu)[1] = w.rates.counts()[1];
    }
    for eye in 0..2u32 {
        let baseline = &sources[w.eyes[eye as usize][0] as usize].baseline_targets;
        for chain in [&w.reference_targets, &w.paired_targets] {
            if !chain.as_ref().is_some_and(|c| c.seed_private_eye(list, eye, baseline)) {
                return 25;
            }
            ex!(CyberpunkVR_PreparedBatchStats)[12] += 1;
            ex!(CyberpunkVR_PreparedBaseline)[2] += 1;
        }
    }
    if !w.native_exits[0].is_null() {
        for eye in 0..2u32 {
            let baseline = &sources[w.eyes[eye as usize][0] as usize].baseline_targets;
            if !w.native_rate_targets.as_ref().is_some_and(|c| c.seed_private_eye(list, eye, baseline)) {
                return 25;
            }
        }
    }
    let capture = |eye| w.paired_bank.as_ref().is_some_and(|b| unsafe { cvr_bank_capture_eye(b.as_raw(), list, eye, keep, has) });
    if !capture(0) || !capture(1) {
        return 6;
    }
    {
        let _internal = cvr_host::probe::Internal::new();
        let states: Vec<crate::prepared_pair_record::SourceState> = sources
            .iter()
            .map(|source| {
                let state = &source.owned_state;
                let draw = source.draw();
                crate::prepared_pair_record::SourceState {
                    pipeline: state.pipeline,
                    rate_image: source.rate_image,
                    topology: state.topology,
                    stencil: state.stencil,
                    blend: state.blend,
                    rate: state.rate.value,
                    combiners: state.rate.combiners,
                    viewport: draw.viewport,
                    scissor: draw.scissor,
                }
            })
            .collect();
        let pairs: Vec<crate::prepared_pair_record::PairRow> = w
            .pairs
            .iter()
            .map(|pair| crate::prepared_pair_record::PairRow {
                command: pair.command,
                sources: pair.sources,
                _pad: 0,
                pipeline: pair.family.as_ref().map_or(null_mut(), |f| f.view.pipeline),
            })
            .collect();
        let membership = mlm_info(&w.membership);
        let d = crate::prepared_pair_record::Draws {
            queries: [raw_of(&w.pipeline_queries), raw_of(&w.occlusion_queries)],
            query_readback: raw_of(&w.query_readback),
            predicate: raw_of(&w.predicate),
            permission: w.draw_permission(),
            mesh: w.mesh.predicates(),
            inputs: w.inputs.predicates(),
            membership: if w.actual_membership { [membership.predicates, membership.results, raw_of(&w.membership_readback)] } else { [null_mut(); 3] },
            rate_image: w.rates.image(),
            sources: states.as_ptr(),
            source_count: states.len(),
            eyes: [w.eyes[0].as_ptr(), w.eyes[1].as_ptr()],
            eye_counts: [w.eyes[0].len(), w.eyes[1].len()],
            steps: w.plan.draws.as_ptr(),
            step_count: w.plan.draws.len(),
            pair_for_source: w.pair_for_source.as_ptr(),
            pairs: pairs.as_ptr(),
            pair_count: pairs.len(),
            mesh_pairs: w.mesh.counts()[0] as u32,
            membership_sources: if w.actual_membership { membership.sources } else { 0 },
            native_rate: !w.native_exits[0].is_null(),
            has_membership: w.actual_membership,
        };
        let counters = crate::prepared_pair_record::DrawCounters {
            depth_state: addr_of_mut!(CyberpunkVR_PreparedPairDepthState),
            pair_stats: addr_of_mut!(CyberpunkVR_PreparedPairStats),
            batch_stats: addr_of_mut!(CyberpunkVR_PreparedBatchStats),
            mesh_stats: addr_of_mut!(CyberpunkVR_PreparedPairMeshStats),
            native_rate: addr_of_mut!(CyberpunkVR_PreparedNativeRate),
        };
        let draw_host =
            crate::prepared_pair_record::DrawHost { user: (w as *mut Work).cast(), targets: draw_targets, source: draw_source, paired: draw_paired };
        // SAFETY: the lent draws, counters and host, alive through the call.
        let reason = unsafe { crate::prepared_pair_record::cvr_npp_record_draws(list, views, rate, &d, &counters, &draw_host) };
        if reason != 0 {
            return reason;
        }
    }
    if !image_copies(w, list) {
        return 9;
    }
    ex!(CyberpunkVR_PreparedBatchStats)[13] += 1;
    0
}
fn record_current(s: &mut State) -> bool {
    let _internal = cvr_host::probe::Internal::new();
    let serial = s.next_owned_serial;
    let w = s.work.as_mut().expect("work");
    let device = raw_of(&w.device);
    w.owned_allocator = None;
    w.owned_list = None;
    w.fence = None;
    // SAFETY: a live device; the outputs receive references.
    let made = serial != u64::MAX
        && unsafe {
            crate::prepared_pair_objects::cvr_npp_command_objects(
                device,
                (&mut w.owned_allocator as *mut Option<ID3D12CommandAllocator>).cast(),
                (&mut w.owned_list as *mut Option<ID3D12GraphicsCommandList>).cast(),
                (&mut w.fence as *mut Option<ID3D12Fence>).cast(),
            )
        };
    if !made {
        fail_current(s, 36);
        return false;
    }
    s.next_owned_serial += 1;
    let w = s.work.as_mut().expect("work");
    w.owned_serial = s.next_owned_serial;
    let list = raw_of(&w.owned_list);
    let stats = ex!(CyberpunkVR_PreparedPairStats);
    stats[12] = list as u64;
    stats[13] = w.owned_serial;
    let views = w.owned_list.as_ref().and_then(|l| l.cast::<ID3D12GraphicsCommandList1>().ok());
    if views.is_none() || !keep_all(list, &[raw_of(&w.owned_allocator)]) {
        fail_current(s, 36);
        return false;
    }
    let rate = w.owned_list.as_ref().and_then(|l| l.cast::<ID3D12GraphicsCommandList5>().ok());
    let reason = record_commands(w, list, raw_of(&views), raw_of(&rate));
    if reason != 0 {
        fail_current(s, reason);
        return false;
    }
    let current = ex!(CyberpunkVR_PreparedCurrent);
    current[2] = u64::from(w.source_frame);
    current[4] = 1;
    // SAFETY: the owned list, recorded.
    if unsafe { w.owned_list.as_ref().expect("list").Close() }.is_err() {
        fail_current(s, 38);
        return false;
    }
    w.closed = true;
    ex!(CyberpunkVR_PreparedCurrent)[5] = 1;
    publish(s);
    true
}

// ---- the start

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn start_internal(
    device: *mut c_void,
    aux: *mut c_void,
    views: *const SourceView,
    count: usize,
    sequence: bool,
    single_family: *mut c_void,
    current_queue: *mut c_void,
    current_frame: u32,
    rust: Option<Box<crate::pair_source::Lent>>,
) -> bool {
    let mut guard = lock();
    let s = guard.state();
    let single = Family::lookup(single_family).map(std::rc::Rc::new);
    let mut next = Work::new(aux, views, count);
    next.rust_sources = rust;
    if active() || device.is_null() || !(2..=512).contains(&count) {
        return false;
    }
    let sources = slice(views, count);
    // SAFETY: a live device of the caller's.
    next.device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned();
    next.sequence = sequence;
    next.current = !current_queue.is_null() || current_frame != 0;
    // SAFETY: a live queue of the caller's or null.
    next.queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&current_queue) }.cloned();
    next.source_frame = current_frame;
    next.export_requested = next.current && CyberpunkVR_PreparedNativeExportEnable.load(SeqCst) != 0;
    next.actual_membership = next.current && CyberpunkVR_PreparedActualMembershipEnable.load(SeqCst) != 0;
    next.membership_fault = next.actual_membership && CyberpunkVR_PreparedPairInputFault.load(SeqCst) != 0;
    reset_membership(next.actual_membership, current_frame);
    ex!(CyberpunkVR_PreparedActualMembershipStats)[18] = u64::from(next.membership_fault);
    crate::prepared_pair_state::cvr_npp_reset_start(current_frame, next.current, next.export_requested, count);
    if next.current {
        let Some(queue) = next.queue.as_ref() else { return false };
        // SAFETY: a live queue.
        let direct = current_frame != 0 && unsafe { queue.GetDesc() }.Type == D3D12_COMMAND_LIST_TYPE_DIRECT;
        // SAFETY: as above.
        let mut actual: Option<ID3D12Device> = None;
        // SAFETY: as above.
        let same = unsafe { queue.GetDevice(&mut actual) }.is_ok() && raw_of(&actual) == device;
        if !direct || !same {
            return false;
        }
    }
    next.pair_for_source = vec![u32::MAX; count];
    let native_exit_mode = sources.iter().any(|s| !s.native_exit.is_null());
    next.allow_vrs_refinement = sources[0].allow_vrs_refinement;
    crate::prepared_pair_state::cvr_npp_reset_outputs(native_exit_mode, next.allow_vrs_refinement);
    if (next.allow_vrs_refinement && !native_exit_mode) || sources.iter().any(|s| s.allow_vrs_refinement != next.allow_vrs_refinement) {
        return false;
    }
    if native_exit_mode && (!next.current || !sequence) {
        return false;
    }
    crate::prepared_pair_state::cvr_npp_reset_batch();
    let draw = sources[0].draw();
    next.width = draw.viewport.Width as u32;
    next.height = draw.viewport.Height as u32;
    next.colors = 3;
    if next.width == 0 || next.height == 0 || draw.viewport.TopLeftX != 0.0 || draw.viewport.TopLeftY != 0.0 {
        return false;
    }
    let mut families: Vec<Option<std::rc::Rc<Family>>> = Vec::with_capacity(count);
    for (i, source) in sources.iter().enumerate() {
        let actual = source.draw();
        let family = if sequence { Family::lookup(source.owned_state.pipeline).map(std::rc::Rc::new) } else { single.clone() };
        let facts = crate::prepared_pair_admission::SourceFacts {
            root: source.root as u64,
            family_root: family.as_ref().map_or(0, |f| f.view.source_root as u64),
            native_status: crate::families_status::native_geometry_status(source.owned_state.pipeline),
            has_family: family.is_some(),
            shader: source.has_shader_root(),
            baseline: has_baseline(source),
            rate_known: source.owned_state.rate.known,
            rate_image: source.owned_state.rate.image != 0,
            source_rate_image: !source.rate_image.is_null(),
        };
        // SAFETY: the lent facts and draws.
        if !unsafe { crate::prepared_pair_admission::cvr_npp_source_shape(&facts, actual, source.original().root, draw) } {
            return false;
        }
        if next.current {
            let now = crate::prepared_pair_admission::CurrentFacts {
                queue: current_queue as u64,
                source_queue: source.source_queue as u64,
                frame: current_frame,
                membership: next.actual_membership,
                input_checks: !source.input_checks().is_empty(),
            };
            // SAFETY: as above.
            if !unsafe { crate::prepared_pair_admission::cvr_npp_source_current(&now, (source.recording as *const RecordingLease).cast(), actual, source.original()) } {
                return false;
            }
        }
        let eye = (1 - actual.side) as usize;
        let indices = &next.eyes[eye];
        let (back, front) = (indices.last().map(|&b| sources[b as usize].ordinal), indices.first().map(|&f| sources[f as usize].baseline as u64));
        if !crate::prepared_pair_admission::cvr_npp_eye_order(!indices.is_empty(), source.ordinal, back.unwrap_or(0), source.baseline as u64, front.unwrap_or(0)) {
            return false;
        }
        for slot in [0usize, 1, 2, 4] {
            let resource = source.baseline_targets.resources[slot] as *mut c_void;
            // SAFETY: the checkpoint's live targets.
            let desc = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.map(|r| unsafe { r.GetDesc() });
            if desc.is_none_or(|d| d.Width != u64::from(next.width) || d.Height != next.height) {
                return false;
            }
        }
        if native_exit_mode {
            let exit = source.native_exit;
            let (first, matches) = (next.eyes[eye].is_empty(), exit.is_null() || (!next.eyes[eye].is_empty() && !same_exit(exit, next.native_exits[eye])));
            if exit.is_null() || matches || !native_exit_matches_source(&next, source, eye as u32) || (first && !validate_native_exit(&next, source)) {
                return false;
            }
            next.native_exits[eye] = exit;
        }
        next.eyes[eye].push(i as u32);
        families.push(family);
        let rows = ex!(CyberpunkVR_PreparedBatchSources);
        rows[i * 2] = source.ordinal;
        rows[i * 2 + 1] = eye as u64;
    }
    if next.eyes[0].is_empty() || next.eyes[1].is_empty() {
        return false;
    }
    if native_exit_mode {
        let (a, b) = (next.native_exit(0), next.native_exit(1));
        if same_exit(next.native_exits[0], next.native_exits[1]) || a.zip(b).is_some_and(|(a, b)| a.readback == b.readback) {
            return false;
        }
        ex!(CyberpunkVR_PreparedNativeExit)[1] = 2;
        ex!(CyberpunkVR_PreparedNativeRate)[1] = 2;
    }
    let (left, right) = (&sources[next.eyes[0][0] as usize], &sources[next.eyes[1][0] as usize]);
    if left.baseline == right.baseline {
        return false;
    }
    for a in [0usize, 1, 2, 4] {
        for b in [0usize, 1, 2, 4] {
            if left.baseline_targets.resources[a] == right.baseline_targets.resources[b] {
                return false;
            }
        }
    }
    let baseline = ex!(CyberpunkVR_PreparedBaseline);
    baseline[0] = 2;
    baseline[1] = u64::from(next.colors);
    let (lefts, rights) = (next.eyes[0].clone(), next.eyes[1].clone());
    for &left in &lefts {
        for &right in &rights {
            if next.pair_for_source[right as usize] != u32::MAX || compatibility_failure(&sources[left as usize], &sources[right as usize]) != 0 {
                continue;
            }
            let Some(family) = families[left as usize].clone() else { continue };
            let f = &family.view;
            if f.pipeline.is_null() || !f.paired_root || !f.instance_rebase_safe || !f.depth_state_known || f.render_targets != 3 {
                continue;
            }
            let index = next.pairs.len() as u32;
            next.pair_for_source[left as usize] = index;
            next.pair_for_source[right as usize] = index;
            next.pairs.push(Pair {
                sources: [left, right],
                family: Some(family),
                bindings: Vec::new(),
                shared: Vec::new(),
                command: u32::MAX,
                ready: false,
                actual_bindings: false,
            });
            break;
        }
    }
    if next.pairs.is_empty() {
        return false;
    }
    let depth_state = next.pairs[0].family.as_ref().expect("family").view.depth_state;
    let mut depth_values = [0u64; 13];
    // SAFETY: the description and 13 values.
    unsafe { crate::prepared_pair_exports::cvr_npp_depth_row(&depth_state, sources[0].owned_state.stencil, &mut depth_values) };
    ex!(CyberpunkVR_PreparedPairDepthState)[..13].copy_from_slice(&depth_values);
    {
        let _internal = cvr_host::probe::Internal::new();
        next.reference_targets = Chain::create(device, next.width, next.height);
        next.paired_targets = Chain::create(device, next.width, next.height);
        if next.reference_targets.is_none() || next.paired_targets.is_none() {
            return false;
        }
        if native_exit_mode {
            next.native_rate_targets = Chain::create(device, next.width, next.height);
            if next.native_rate_targets.is_none() {
                return false;
            }
        }
        // SAFETY: a live device; the outputs receive references (both query heaps, the predicate word written 0).
        let made = unsafe {
            crate::prepared_pair_objects::cvr_npp_pass_objects(
                device,
                (&mut next.pipeline_queries as *mut Option<ID3D12QueryHeap>).cast(),
                (&mut next.occlusion_queries as *mut Option<ID3D12QueryHeap>).cast(),
                (&mut next.predicate as *mut Option<ID3D12Resource>).cast(),
            )
        };
        if !made {
            return false;
        }
        if !sources[0].rate_image.is_null() {
            let mut images = Vec::with_capacity(count);
            for source in sources {
                if source.rate_image.is_null() || source.rate_capture.is_null() || vrs_source_image(source.rate_capture) != source.rate_image {
                    return false;
                }
                images.push(source.rate_image);
            }
            // SAFETY: the live images.
            if unsafe { cvr_finer_vrs_create(next.rates.0, device, images.as_ptr(), images.len()) }.is_err() {
                return false;
            }
            let image = next.rates.image();
            // SAFETY: the created image.
            let Some(desc) = (unsafe { ID3D12Resource::from_raw_borrowed(&image) }).map(|r| unsafe { r.GetDesc() }) else { return false };
            let counts = next.rates.counts();
            let vrs = ex!(CyberpunkVR_PreparedPairVrs);
            vrs[0] = desc.Width;
            vrs[1] = u64::from(desc.Height);
            vrs[2] = desc.Width * u64::from(desc.Height);
            vrs[6] = 2;
            let gpu = ex!(CyberpunkVR_PreparedPairVrsGpu);
            gpu[0] = counts[0];
            gpu[2] = counts[2];
            if native_exit_mode {
                let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS6::default();
                // SAFETY: a local output of the queried size.
                let queried = next.device.as_ref().is_some_and(|d| unsafe {
                    d.CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6, (&mut options as *mut D3D12_FEATURE_DATA_D3D12_OPTIONS6).cast(), core::mem::size_of_val(&options) as u32)
                }
                .is_ok());
                if queried && options.VariableShadingRateTier.0 >= D3D12_VARIABLE_SHADING_RATE_TIER_2.0 {
                    next.rate_tile = options.ShadingRateImageTileSize;
                }
                next.rate_width = desc.Width as u32;
                next.rate_height = desc.Height;
                let attribution = ex!(CyberpunkVR_PreparedVrsAttribution);
                attribution[0] = u64::from(next.rate_tile);
                attribution[1] = u64::from(next.rate_width);
                attribution[2] = u64::from(next.rate_height);
            }
        } else if sources.iter().any(|s| !s.rate_image.is_null()) {
            return false;
        }
    }
    if !prepare_membership_banks(&mut next) || !prepare_geometry(&mut next) || !prepare_inputs(&mut next) || !prepare_mesh(&mut next) {
        return false;
    }
    if next.current && next.inputs.predicates().is_null() {
        return false;
    }
    // The distinct immutable assets made resident (their count first).
    ex!(CyberpunkVR_PreparedCurrent)[13] = next.res(0, Residency::AssetCount) as u64;
    if next.res(0, Residency::AssetCount) != 0 {
        let resident = next.residency(0, Residency::Acquire, device, null_mut(), 0, S_OK);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[6] = resident as u32 as u64;
        if HRESULT(resident as i32).is_err() {
            return false;
        }
        residency[0] = next.res(0, Residency::SourceCount) as u64;
        residency[1] = next.res(0, Residency::ObjectCount) as u64;
        residency[2] = next.res(0, Residency::Status) as u64;
    }
    *ex!(CyberpunkVR_PreparedPairStats) = [0; 16];
    let stats = ex!(CyberpunkVR_PreparedPairStats);
    stats[0] = sources[next.eyes[0][0] as usize].ordinal;
    stats[1] = sources[next.eyes[1][0] as usize].ordinal;
    stats[2] = next.mesh_bytes;
    let batch = ex!(CyberpunkVR_PreparedBatchStats);
    batch[0] = next.eyes[0].len() as u64;
    batch[1] = next.eyes[1].len() as u64;
    batch[2] = next.pairs.len() as u64;
    batch[15] = u64::from(sequence);
    // SAFETY: as ex!.
    unsafe { CyberpunkVR_PreparedBatchSourceCount = count as u32 };
    let current = next.current;
    s.work = Some(next);
    ACTIVE.store(true, SeqCst);
    if current {
        return record_current(s);
    }
    publish(s);
    true
}
fn same_exit(a: *const ExitView, b: *const ExitView) -> bool {
    // SAFETY: the holder's exits or null.
    match unsafe { (a.as_ref(), b.as_ref()) } {
        (Some(a), Some(b)) => a.identity == b.identity,
        (None, None) => true,
        _ => false,
    }
}
fn vrs_source_image(capture: *mut c_void) -> *mut c_void {
    let mut frozen = null_mut();
    // SAFETY: the snapshot's live Rust object.
    unsafe { cvr_vrs_snapshot_info(capture, &mut frozen, null_mut(), null_mut()) };
    frozen
}

fn free_aux(aux: *mut c_void) {
    if aux != rust_holder() {
        // SAFETY: the holder handed to this module, freed once.
        unsafe { (host().aux_free)(aux) };
    }
}
fn start_current_sequence(device: *mut c_void, aux: *mut c_void, views: *const SourceView, count: usize, queue: *mut c_void, frame: u32, rust: Option<Box<crate::pair_source::Lent>>) -> bool {
    {
        let _guard = lock();
        if active() {
            free_aux(aux);
            return false;
        }
        reset_membership(CyberpunkVR_PreparedActualMembershipEnable.load(SeqCst) != 0, frame);
        let sources = slice(views, count);
        crate::prepared_pair_state::cvr_npp_reset_current(
            frame,
            sources.iter().any(|s| !s.native_exit.is_null()),
            sources.first().is_some_and(|s| s.allow_vrs_refinement),
            count,
        );
        if queue.is_null() || frame == 0 {
            ex!(CyberpunkVR_PreparedPairStats)[10] = 35;
            ex!(CyberpunkVR_PreparedCurrent)[12] = 35;
            CyberpunkVR_PreparedPairState.store(6, SeqCst);
            free_aux(aux);
            return false;
        }
    }
    let started = start_internal(device, aux, views, count, true, null_mut(), queue, frame, rust);
    if !started {
        let _guard = lock();
        if !active() {
            let stats = ex!(CyberpunkVR_PreparedPairStats);
            if stats[10] == 0 {
                stats[10] = 35;
            }
            let current = ex!(CyberpunkVR_PreparedCurrent);
            current[0] = 1;
            current[1] = u64::from(frame);
            current[12] = stats[10];
            CyberpunkVR_PreparedPairState.store(6, SeqCst);
        }
    }
    started
}
fn needs_native_bindings() -> bool {
    if !active() {
        return false;
    }
    let mut guard = lock();
    active() && guard.state().work.as_ref().is_some_and(|w| !w.current)
}
fn refuse_native_export(w: &mut Work, reason: u32) -> bool {
    let export = ex!(CyberpunkVR_PreparedNativeExport);
    if export[11] == 0 {
        export[11] = u64::from(reason);
    }
    if w.res(1, Residency::Status) == ACQUIRED {
        export[19] = w.res(1, Residency::Cancel) as u32 as u64;
    }
    if let Some(list) = &w.export_list {
        // SAFETY: the export list's command resources.
        unsafe { cvr_gpu_reset_command_resources(list.as_raw()) };
    }
    w.export_list = None;
    w.export_allocator = None;
    w.output_copy = None;
    w.export_prepared = false;
    false
}
fn compare_native_output(w: &Work, paired: &[u8]) -> bool {
    if !w.export_submitted {
        return true;
    }
    let (mut readback, mut bytes, mut recorded) = (null_mut(), 0u64, false);
    let mut planes = Vec::new();
    if let Some(copy) = &w.output_copy {
        (readback, bytes, recorded) = copy.info();
        planes = copy.planes();
    }
    let c = crate::prepared_pair_record::OutputCompare {
        readback,
        bytes,
        copy_planes: planes.as_ptr(),
        copy_count: planes.len(),
        exits: [exit_view(w.native_exit(0)), exit_view(w.native_exit(1))],
        planes: w.planes.as_ptr(),
        plane_count: w.planes.len(),
        paired: paired.as_ptr(),
        paired_bytes: paired.len(),
        width: w.width,
        height: w.height,
        recorded,
    };
    let baseline = ex!(CyberpunkVR_PreparedBaseline)[22];
    // SAFETY: the lent comparison and the export.
    unsafe { crate::prepared_pair_record::cvr_npp_compare_output(&c, true, baseline, addr_of_mut!(CyberpunkVR_PreparedNativeExport)) }
}
/// PrepareNativeExport for a Rust caller (the retained capture's target access receipt and borrowed lifetimes).
pub fn prepare_native_export_rust(queue: *mut c_void, frame: u32, receipt: Option<crate::targets::Receipt>, lifetimes: [crate::targets::BorrowedScope; 8]) -> bool {
    {
        let mut guard = lock();
        let s = guard.state();
        let Some(w) = s.work.as_mut() else { return false };
        w.rust_export = Some(Box::new(RustExport { receipt, lifetimes, resident: Default::default(), kept: Default::default() }));
    }
    prepare_native_export(queue, frame, null(), null())
}
/// A Rust caller's native export: the receipt, the eight borrowed target lifetimes, the described targets and the
/// borrowed pageables kept for their re-authentication.
pub struct RustExport {
    receipt: Option<crate::targets::Receipt>,
    lifetimes: [crate::targets::BorrowedScope; 8],
    resident: [Option<(crate::targets::Identity, crate::targets::BorrowedScope)>; 8],
    kept: core::cell::RefCell<[Option<(windows::core::IUnknown, crate::targets::Identity, crate::targets::BorrowedScope)>; 8]>,
}
impl RustExport {
    /// The host's ExportDescribe: the receipt's sources against the native exits ([list, generation] per eye) and the
    /// lifetimes; the export's rows; 0, or the refusal (2 described, 3 a source, 4 a target).
    fn describe(&mut self, queue: *mut c_void, frame: u32, exits: &[u64; 4], rows: &mut [OutputTarget; 8]) -> u32 {
        let Some(targets) = self.receipt.as_ref().and_then(|r| r.describe(queue, frame)) else { return 2 };
        let mut resident: [Option<(crate::targets::Identity, crate::targets::BorrowedScope)>; 8] = Default::default();
        for side in 0..2usize {
            let source = &targets[side];
            let other = 1 - side;
            if source.side != side as u32 || source.frame != frame || source.recording.0.list != exits[other * 2] || source.recording.0.generation != exits[other * 2 + 1] {
                return 3;
            }
            for i in 0..4usize {
                let target = &source.targets[i];
                let lifetime = &self.lifetimes[side * 4 + i];
                if target.slot != (if i == 3 { 4 } else { i as u32 }) || !lifetime.validate() {
                    return 4;
                }
                let c = &target.identity.value().creation;
                rows[side * 4 + i] = OutputTarget {
                    kind: c.kind,
                    resource: c.resource.as_ref().map_or(null_mut(), |r| r.as_raw()),
                    heap: c.heap.as_ref().map_or(null_mut(), |h| h.as_raw()),
                    heap_offset: c.offset,
                    allocation_bytes: c.bytes,
                    plane_count: target.planes,
                    final_states: [target.final_states[0] as i32, target.final_states[1] as i32],
                };
                resident[side * 4 + i] = Some((target.identity.clone(), lifetime.clone()));
            }
        }
        self.resident = resident;
        0
    }
    /// The host's TargetPageable: the export target's borrowed pageable (kept for the re-authentication).
    fn pageable(&self, index: u32, device: *mut c_void) -> *mut c_void {
        let Some((identity, lifetime)) = self.resident.get(index as usize).and_then(|r| r.as_ref()) else { return null_mut() };
        let Some((pageable, captured)) = lifetime.pageable(device, identity) else { return null_mut() };
        let raw = pageable.as_raw();
        self.kept.borrow_mut()[index as usize] = Some((pageable, captured, lifetime.clone()));
        raw
    }
    /// The host's TargetReauth: the kept pageable's owner again.
    fn reauth(&self, index: u32, device: *mut c_void) -> bool {
        let kept = self.kept.borrow();
        let Some((pageable, identity, lifetime)) = kept.get(index as usize).and_then(|k| k.as_ref()) else { return false };
        let Some((now, _)) = lifetime.pageable(device, identity) else { return false };
        // SAFETY: two live pageables.
        unsafe { crate::residency_lease::cvr_rl_same_owner(now.as_raw(), pageable.as_raw()) }
    }
    /// The host's ExportValidate: the receipt live, every lifetime valid.
    fn validate(&self, queue: *mut c_void, frame: u32, live: &mut bool) -> bool {
        let valid = self.receipt.as_ref().is_some_and(|r| r.live(queue, frame));
        *live = self.lifetimes.iter().fold(true, |alive, l| l.validate() && alive);
        valid
    }
}
fn prepare_native_export(queue: *mut c_void, frame: u32, receipt: *const c_void, lifetimes: *const c_void) -> bool {
    let mut guard = lock();
    let s = guard.state();
    let Some(w) = s.work.as_mut() else { return false };
    if !w.current || !w.export_requested || w.source_frame != frame {
        return false;
    }
    if w.export_prepared || w.execution_issued || w.submitted {
        return false;
    }
    if !w.closed || w.failed || raw_of(&w.queue) != queue || !w.sequence || w.native_exits[0].is_null() || w.native_exits[1].is_null() || !w.allow_vrs_refinement {
        return refuse_native_export(w, 1);
    }
    let (a, b) = (w.native_exit(0).expect("exit"), w.native_exit(1).expect("exit"));
    let exits = [a.list, a.generation, b.list, b.generation];
    let mut rows = [OutputTarget { kind: 0, resource: null_mut(), heap: null_mut(), heap_offset: 0, allocation_bytes: 0, plane_count: 0, final_states: [0; 2] }; 8];
    let refused = match w.rust_export.as_mut() {
        Some(r) => r.describe(queue, frame, &exits, &mut rows),
        // SAFETY: the holder's export state; the caller's receipt and lifetimes; the rows are writable.
        None => unsafe { (host().export_describe)(w.aux, raw_of(&w.device), receipt, lifetimes, queue, frame, exits.as_ptr(), &mut rows) },
    };
    if refused != 0 {
        return refuse_native_export(w, refused);
    }
    // The export targets' residency (formerly the .cpp's AcquireBorrowedTargets in its describe).
    let acquired = {
        let device = raw_of(&w.device);
        let aux = w.aux;
        w.leases.borrow_mut()[1].acquire_targets(device, aux, w.rust_export.as_deref())
    };
    ex!(CyberpunkVR_PreparedNativeExport)[19] = u64::from(acquired as u32);
    ex!(CyberpunkVR_PreparedNativeExport)[15] = u64::from(acquired >= 0);
    if acquired < 0 {
        return refuse_native_export(w, 5);
    }
    ex!(CyberpunkVR_PreparedNativeExport)[14] = w.leases.borrow()[1].objects as u64;
    let _internal = cvr_host::probe::Internal::new();
    let copy = OutputCopy::new();
    let arrays = [0u32, 1, 2, 4].map(|slot| chain_resource(&w.paired_targets, slot));
    // SAFETY: the arrays and rows live through the call.
    if unsafe { cvr_output_copy_create(copy.0, raw_of(&w.device), &arrays, &rows, w.draw_permission(), 0) }.is_err() {
        return refuse_native_export(w, 6);
    }
    w.export_allocator = None;
    w.export_list = None;
    // SAFETY: a live device; the outputs receive references.
    let made = unsafe {
        crate::prepared_pair_objects::cvr_npp_command_objects(
            raw_of(&w.device),
            (&mut w.export_allocator as *mut Option<ID3D12CommandAllocator>).cast(),
            (&mut w.export_list as *mut Option<ID3D12GraphicsCommandList>).cast(),
            null_mut(),
        )
    };
    if !made {
        return refuse_native_export(w, 7);
    }
    let list = raw_of(&w.export_list);
    if !keep_all(list, &[raw_of(&w.export_allocator), raw_of(&w.fence), w.draw_permission(), copy.info().0]) {
        return refuse_native_export(w, 8);
    }
    for array in arrays {
        if !keep_all(list, &[array]) {
            return refuse_native_export(w, 8);
        }
    }
    for row in &rows {
        if !keep_all(list, &[row.resource, row.heap]) {
            return refuse_native_export(w, 8);
        }
    }
    // SAFETY: the export list, recorded and closed.
    if !unsafe { cvr_output_copy_record(copy.0, list) } || unsafe { w.export_list.as_ref().expect("list").Close() }.is_err() {
        return refuse_native_export(w, 9);
    }
    let (_, bytes, _) = copy.info();
    let planes = copy.planes().len();
    w.output_copy = Some(copy);
    w.export_prepared = true;
    let export = ex!(CyberpunkVR_PreparedNativeExport);
    export[1] = 1;
    export[4] = planes as u64;
    export[5] = bytes;
    export[13] = list as u64;
    true
}
type ExecuteFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);
fn submit_current(queue: *mut c_void, frame: u32) -> bool {
    let mut guard = lock();
    let s = guard.state();
    if !active() || !s.work.as_ref().is_some_and(|w| w.current) {
        return false;
    }
    let w = s.work.as_mut().expect("work");
    if w.submitted || w.execution_issued {
        return false;
    }
    if queue.is_null()
        || raw_of(&w.queue) != queue
        || frame != w.source_frame
        || !w.closed
        || w.owned_list.is_none()
        || w.owned_allocator.is_none()
        || w.fence.is_none()
        || w.failed
        || w.stage != 4
    {
        fail_current(s, 39);
        return false;
    }
    // The hooked queue table's original ExecuteCommandLists (command_queue_original_execute).
    let Some(original) = crate::queue_dispatch::original_execute(queue) else {
        fail_current(s, 40);
        return false;
    };
    let original: ExecuteFn = original;
    for (i, source) in slice(w.views, w.view_count).iter().enumerate() {
        if source.source_queue != queue || source.draw().frame != frame || source.original().frame != frame {
            fail_current(s, 39);
            return false;
        }
        for (m, material) in source.materials().iter().enumerate() {
            if material.has_buffer && !material_matches(w, i, m) {
                fail_current(s, 39);
                return false;
            }
            if !material.immutable_owner.is_null() && !immutable_valid(w, i, m, queue) {
                fail_current(s, 12);
                return false;
            }
        }
    }
    if w.export_requested {
        if !w.export_prepared {
            refuse_native_export(w, 11);
        } else {
            let mut live = false;
            // SAFETY: the holder's export receipt and lifetimes.
            let valid = match w.rust_export.as_ref() {
                Some(r) => r.validate(queue, frame, &mut live),
                // SAFETY: the holder's export state.
                None => unsafe { (host().export_validate)(w.aux, queue, frame, &mut live) },
            };
            let export = ex!(CyberpunkVR_PreparedNativeExport);
            export[20] = u64::from(valid);
            export[21] = u64::from(live);
            if !valid || !live {
                refuse_native_export(w, 12);
            }
        }
    }
    // SAFETY: the predicate word.
    if !unsafe { crate::prepared_pair_record::cvr_npp_write_predicate(raw_of(&w.predicate), 1) } {
        fail_current(s, 23);
        return false;
    }
    ex!(CyberpunkVR_PreparedPairDepthState)[13] = 1;
    let lists = [raw_of(&w.owned_list), raw_of(&w.export_list)];
    let list_count = if w.export_prepared { 2u32 } else { 1 };
    // SAFETY: the lists' command resources.
    w.owned_flight = unsafe { cvr_gpu_prepare_command_resources(list_count, lists.as_ptr()) };
    if w.owned_flight.is_null() {
        fail_current(s, 4);
        return false;
    }
    let w = s.work.as_mut().expect("work");
    // SAFETY: a copy of the prepared submission (the work keeps its own).
    let resources = unsafe { cvr_gpu_clone_submission(w.owned_flight) };
    let source_lease = w.res(0, Residency::SourceCount) != 0;
    if source_lease || w.export_prepared {
        let which = u32::from(source_lease) | u32::from(w.export_prepared) << 1;
        let resident = w.residency(which, Residency::MarkTogether, queue, null_mut(), 0, S_OK);
        if source_lease {
            let residency = ex!(CyberpunkVR_PreparedPairResidency);
            residency[6] = resident as u32 as u64;
            residency[3] = u64::from(resident >= 0);
            residency[2] = w.res(0, Residency::Status) as u64;
        }
        if w.export_prepared {
            let export = ex!(CyberpunkVR_PreparedNativeExport);
            export[19] = resident as u32 as u64;
            export[16] = u64::from(resident >= 0);
        }
        if resident < 0 {
            if w.export_prepared {
                ex!(CyberpunkVR_PreparedNativeExport)[11] = 13;
            }
            let reason = if w.export_prepared { 44 } else { 33 };
            // SAFETY: the unused copy.
            unsafe { cvr_gpu_free_submission(resources) };
            fail_current(s, reason);
            return false;
        }
    }
    let signal = {
        // The registered queue order lock.
        queue_order(true);
        let internal = cvr_host::probe::Internal::new();
        w.execution_issued = true;
        // SAFETY: the queue's original ExecuteCommandLists with the owned (and export) lists.
        unsafe { original(queue, list_count, lists.as_ptr()) };
        w.export_submitted = w.export_prepared;
        ex!(CyberpunkVR_PreparedNativeExport)[2] = u64::from(w.export_submitted);
        let current = ex!(CyberpunkVR_PreparedCurrent);
        current[3] = u64::from(frame);
        current[6] = 1;
        // SAFETY: a live queue and fence.
        let signal = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue).expect("queue").Signal(w.fence.as_ref().expect("fence"), 1) };
        drop(internal);
        queue_order(false);
        signal.err().map_or(S_OK, |e| e.code())
    };
    let fence = raw_of(&w.fence);
    let mut resident = S_OK.0 as i64;
    if source_lease {
        resident = w.residency(0, Residency::AssociateFence, queue, fence, 1, signal);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[6] = resident as u32 as u64;
        residency[4] = u64::from(resident >= 0);
        residency[2] = w.res(0, Residency::Status) as u64;
        residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
    }
    let mut target_resident = S_OK.0 as i64;
    if w.export_submitted {
        target_resident = w.residency(1, Residency::AssociateFence, queue, fence, 1, signal);
        let export = ex!(CyberpunkVR_PreparedNativeExport);
        export[19] = target_resident as u32 as u64;
        export[17] = u64::from(target_resident >= 0);
    }
    // SAFETY: the submission handed on.
    unsafe { cvr_gpu_submit_command_resources(queue, resources) };
    if signal.is_err() || resident < 0 || target_resident < 0 {
        fail_current(s, 13);
        return false;
    }
    w.submitted = true;
    publish(s);
    true
}
fn draw(list: *mut c_void, current: &DrawRecord) {
    if internal() || !active() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if !active() || !s.work.as_ref().is_some_and(|w| !w.current && w.stage == 3) {
        return;
    }
    // SAFETY: the host's families.
    if current.rt_count != 3 || current.depth == 0 || (current.flags & 191) != 191 || current.flags & 64 != 0 || crate::families_status::native_geometry_status(current.pipeline as *mut c_void) != 1 {
        return;
    }
    ex!(CyberpunkVR_PreparedCurrent)[9] += 1;
    let finish = |s: &mut State, reason: u32| {
        error(s, reason);
        if let Some(w) = s.work.as_mut() {
            w.complete = true;
        }
        ACTIVE.store(false, SeqCst);
        publish(s);
    };
    if !capture_destination(s.work.as_mut().expect("work"), list) {
        finish(s, 1);
        return;
    }
    let restore = Restore::new(list, current);
    if !restore.valid {
        finish(s, 3);
        return;
    }
    let w = s.work.as_mut().expect("work");
    // The holder's immutable leases, registered on the list (the uses kept here).
    let registered = w.uses_op(1, list);
    if registered != 0 {
        finish(s, registered);
        return;
    }
    // SAFETY: as above.
    ex!(CyberpunkVR_PreparedPairStats)[14] = u64::from(w.uses_op(3, null_mut()));
    let (views, rate) = (raw_of(&restore.views), raw_of(&restore.rate));
    let reason = record_commands(w, list, views, rate);
    if reason != 0 {
        error(s, reason);
        let w = s.work.as_mut().expect("work");
        if w.stage == 3 {
            w.complete = true;
            ACTIVE.store(false, SeqCst);
        }
    }
    publish(s);
    drop(restore);
}
fn lists_of<'a>(count: u32, lists: *const *mut c_void) -> &'a [*mut c_void] {
    slice(lists, count as usize)
}
fn before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if !active() || !s.work.as_ref().is_some_and(|w| !w.current) {
        return;
    }
    let w = s.work.as_mut().expect("work");
    if w.submitted || w.stage != 4 {
        return;
    }
    let matches = lists_of(count, lists).iter().filter(|&&l| l as u64 == w.recording.0.list).count();
    if matches == 0 {
        return;
    }
    // SAFETY: the recording's lease.
    if matches != 1 || !unsafe { cvr_native_recording_validate_submission(&w.recording.0, queue) } {
        error(s, 11);
    }
    let w = s.work.as_mut().expect("work");
    if w.uses_op(2, queue) != 0 {
        error(s, 12);
    }
    let w = s.work.as_mut().expect("work");
    if w.res(0, Residency::SourceCount) != 0 {
        let resident = w.residency(0, Residency::MarkSubmitted, queue, null_mut(), 0, S_OK);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[6] = resident as u32 as u64;
        residency[3] = u64::from(resident >= 0);
        residency[2] = w.res(0, Residency::Status) as u64;
        if resident < 0 {
            error(s, 33);
        }
    }
    let w = s.work.as_mut().expect("work");
    if w.stage == 4 {
        let value = u64::from(!w.failed);
        // SAFETY: the predicate word.
        if !unsafe { crate::prepared_pair_record::cvr_npp_write_predicate(raw_of(&w.predicate), value) } {
            error(s, 23);
        } else {
            ex!(CyberpunkVR_PreparedPairDepthState)[13] = value;
        }
    }
    let w = s.work.as_mut().expect("work");
    // SAFETY: a live queue.
    w.queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned();
}
fn submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if internal() || !active() || queue.is_null() || lists.is_null() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if !active() || !s.work.as_ref().is_some_and(|w| !w.current) {
        return;
    }
    let w = s.work.as_mut().expect("work");
    if w.submitted || raw_of(&w.queue) != queue {
        return;
    }
    if !lists_of(count, lists).iter().any(|&l| l as u64 == w.recording.0.list) {
        return;
    }
    // SAFETY: a live queue and fence.
    let signal = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue).expect("queue").Signal(w.fence.as_ref().expect("fence"), 1) }.err().map_or(S_OK, |e| e.code());
    let mut resident = S_OK.0 as i64;
    if w.res(0, Residency::SourceCount) != 0 {
        resident = w.residency(0, Residency::AssociateFence, queue, raw_of(&w.fence), 1, signal);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[6] = resident as u32 as u64;
        residency[4] = u64::from(resident >= 0);
        residency[2] = w.res(0, Residency::Status) as u64;
        residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
    }
    if signal.is_err() || resident < 0 {
        w.res(0, Residency::Quarantine);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[2] = w.res(0, Residency::Status) as u64;
        residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
        error(s, 13);
        s.work.as_mut().expect("work").complete = true;
        ACTIVE.store(false, SeqCst);
    } else {
        w.submitted = true;
    }
    publish(s);
}
fn reset(list: *mut c_void) {
    if internal() || !active() {
        return;
    }
    TEST_RESET_ENTERED.store(true, SeqCst);
    let mut guard = lock();
    let s = guard.state();
    if !active() || !s.work.as_ref().is_some_and(|w| !w.current) {
        return;
    }
    let w = s.work.as_mut().expect("work");
    if w.recording.0.list == list as u64 && !w.submitted && w.stage == 4 {
        cancel_unsubmitted_residency(w);
        error(s, 14);
        s.work.as_mut().expect("work").complete = true;
        ACTIVE.store(false, SeqCst);
        publish(s);
    }
}
fn poll_membership(w: &Work) -> bool {
    if !w.actual_membership {
        return true;
    }
    let count = mlm_info(&w.membership).sources;
    // SAFETY: as ex!.
    let published = unsafe { CyberpunkVR_PreparedActualMembershipCount };
    if count == 0 || count as usize != w.view_count || count != published || w.membership_readback.is_none() {
        return membership_error(6, u64::MAX, 0);
    }
    // SAFETY: the readback and the exports.
    let read = unsafe {
        crate::prepared_pair_queries::cvr_npp_membership_readback(
            raw_of(&w.membership_readback),
            u64::from(count) * 48,
            count,
            addr_of_mut!(CyberpunkVR_PreparedActualMembershipRows).cast(),
            addr_of_mut!(CyberpunkVR_PreparedActualMembershipStats).cast(),
            addr_of_mut!(CyberpunkVR_PreparedPairInputStats).cast(),
        )
    };
    match read {
        0 => true,
        1 => membership_error(8, u64::MAX, 0),
        _ => membership_error(9, u64::MAX, 0),
    }
}
struct Rates<'a> {
    sources: &'a [SourceView],
    pixels: Vec<u8>,
    common: Vec<u8>,
    original: [Vec<u8>; 2],
}
unsafe extern "C-unwind" fn read_rates(user: *mut c_void, index: usize, data: *mut *const u8, bytes: *mut usize, width: *mut u32, height: *mut u32) -> bool {
    // SAFETY: poll's rates.
    let r = unsafe { &mut *(user as *mut Rates) };
    let source = &r.sources[index];
    r.pixels.clear();
    if source.rate_capture.is_null() {
        return false;
    }
    let (mut w, mut h) = (0u32, 0u32);
    // SAFETY: the snapshot's live Rust object.
    unsafe { cvr_vrs_snapshot_info(source.rate_capture, null_mut(), &mut w, &mut h) };
    let mut pixels = vec![0u8; w as usize * h as usize];
    // SAFETY: `pixels` holds the eye's map.
    if unsafe { cvr_vrs_snapshot_read_eye(source.rate_capture, 0, pixels.as_mut_ptr(), pixels.len()) }.is_err() {
        return false;
    }
    r.pixels = pixels;
    // SAFETY: the caller's outputs.
    unsafe {
        *data = r.pixels.as_ptr();
        *bytes = r.pixels.len();
        *width = w;
        *height = h;
    }
    true
}
unsafe extern "C-unwind" fn store_rates(user: *mut c_void, which: u32, data: *const u8, bytes: usize) {
    // SAFETY: as above.
    let r = unsafe { &mut *(user as *mut Rates) };
    let target = if which < 2 { &mut r.original[which as usize] } else { &mut r.common };
    *target = slice(data, bytes).to_vec();
}
fn poll() {
    if !active() {
        return;
    }
    let mut guard = lock();
    let s = guard.state();
    if !active() {
        return;
    }
    let Some(w) = s.work.as_mut() else { return };
    if !w.submitted {
        return;
    }
    let Some(fence) = w.fence.as_ref() else { return };
    // SAFETY: a live fence.
    let completed = unsafe { fence.GetCompletedValue() };
    if completed < 1 {
        return;
    }
    if completed == u64::MAX {
        if w.current {
            fail_current(s, 15);
            return;
        }
        error(s, 15);
    } else if w.current {
        ex!(CyberpunkVR_PreparedCurrent)[7] = 1;
    }
    let w = s.work.as_mut().expect("work");
    if w.export_submitted {
        let export = ex!(CyberpunkVR_PreparedNativeExport);
        export[3] = 1;
        let retired = w.res(1, Residency::Retire);
        export[19] = retired as u32 as u64;
        export[18] = u64::from(retired == 0);
        if retired != 0 {
            export[11] = 14;
            error(s, 45);
        }
    }
    let w = s.work.as_mut().expect("work");
    if w.res(0, Residency::SourceCount) != 0 {
        let retired = w.res(0, Residency::Retire);
        let residency = ex!(CyberpunkVR_PreparedPairResidency);
        residency[6] = retired as u32 as u64;
        residency[2] = w.res(0, Residency::Status) as u64;
        residency[5] = u64::from(retired == 0);
        residency[7] = w.res(0, Residency::QuarantinedCount) as u64;
        if retired != 0 {
            error(s, 34);
        }
    }
    let w = s.work.as_mut().expect("work");
    if !w.failed && w.stage == 4 {
        if !w.rates.image().is_null() {
            let (hr, actual, invalid) = w.rates.read();
            let read = hr.is_ok() && invalid == 0;
            ex!(CyberpunkVR_PreparedPairVrs)[7] = u64::from(invalid);
            let sources = slice(w.views, w.view_count);
            let sides: Vec<u32> = sources.iter().map(|s| s.draw().side).collect();
            let mut rates = Rates { sources, pixels: Vec::new(), common: core::mem::take(&mut w.common_rates), original: core::mem::take(&mut w.original_rates) };
            let maps = crate::prepared_pair_record::RateMaps {
                actual: actual.as_ptr(),
                actual_bytes: actual.len(),
                sides: sides.as_ptr(),
                sources: sides.len(),
                eye_front: [w.eyes[0][0] as usize, w.eyes[1][0] as usize],
                eye_counts: [w.eyes[0].len(), w.eyes[1].len()],
                width: w.width,
                height: w.height,
                tile: w.rate_tile,
                map_width: w.rate_width,
                map_height: w.rate_height,
                read,
                native: !w.native_exits[0].is_null(),
            };
            let mut ready = false;
            // SAFETY: the lent maps and callbacks; the exports.
            let valid = unsafe {
                crate::prepared_pair_record::cvr_npp_rate_maps(
                    &maps,
                    (&mut rates as *mut Rates).cast(),
                    read_rates,
                    store_rates,
                    addr_of_mut!(CyberpunkVR_PreparedPairVrs),
                    addr_of_mut!(CyberpunkVR_PreparedPairVrsGpu),
                    addr_of_mut!(CyberpunkVR_PreparedVrsAttribution),
                    &mut ready,
                )
            };
            w.common_rates = rates.common;
            w.original_rates = rates.original;
            if !w.native_exits[0].is_null() {
                w.rate_maps_ready = ready;
            }
            if !valid {
                error(s, 32);
            }
        }
        let w = s.work.as_mut().expect("work");
        {
            let read = crate::prepared_pair_queries::QueryRead {
                pairs: w.mesh.counts()[0] as u32,
                predicates: !w.inputs.predicates().is_null(),
                native_exit: !w.native_exits[0].is_null(),
                sequence: w.sequence,
                draws: w.plan.draws.len() as u64,
            };
            let exports = crate::prepared_pair_queries::QueryExports {
                gpu: addr_of_mut!(CyberpunkVR_PreparedPairGpu).cast(),
                baseline: addr_of_mut!(CyberpunkVR_PreparedBaseline).cast(),
                depth_state: addr_of_mut!(CyberpunkVR_PreparedPairDepthState).cast(),
                mesh_results: addr_of_mut!(CyberpunkVR_PreparedPairMeshResults).cast(),
                input_stats: addr_of_mut!(CyberpunkVR_PreparedPairInputStats).cast(),
                mesh_stats: addr_of_mut!(CyberpunkVR_PreparedPairMeshStats).cast(),
                batch_stats: addr_of_mut!(CyberpunkVR_PreparedBatchStats).cast(),
                pair_stats: addr_of_mut!(CyberpunkVR_PreparedPairStats).cast(),
                exit: addr_of_mut!(CyberpunkVR_PreparedNativeExit).cast(),
                rate: addr_of_mut!(CyberpunkVR_PreparedNativeRate).cast(),
            };
            // SAFETY: the readback and the exports.
            let errors = unsafe { crate::prepared_pair_queries::cvr_npp_query_readback(raw_of(&w.query_readback), &read, &exports) };
            for (bit, reason) in [(1, 26), (2, 29), (4, 28), (8, 17)] {
                if errors & bit != 0 {
                    error(s, reason);
                }
            }
        }
        let w = s.work.as_mut().expect("work");
        if !poll_membership(w) {
            error(s, 47);
        }
        let w = s.work.as_mut().expect("work");
        let range = D3D12_RANGE { Begin: 0, End: w.image_bytes as usize };
        let mut mapped = null_mut();
        // SAFETY: the image readback, mapped for the read.
        let map_ok = w.image_readback.as_ref().is_some_and(|r| unsafe { r.Map(0, Some(&range), Some(&mut mapped)) }.is_ok()) && !mapped.is_null();
        if !map_ok {
            error(s, 19);
        } else {
            let image = slice(mapped.cast::<u8>().cast_const(), w.image_bytes as usize);
            // SAFETY: the mapped bytes, the planes and the exports.
            let compared = unsafe {
                crate::prepared_pair_images::cvr_npp_compare_images(
                    mapped.cast(),
                    w.image_bytes as usize,
                    w.planes.as_ptr(),
                    w.planes.len(),
                    w.width,
                    w.colors,
                    addr_of_mut!(CyberpunkVR_PreparedPairStats).cast(),
                    addr_of_mut!(CyberpunkVR_PreparedBaseline).cast(),
                    addr_of_mut!(CyberpunkVR_PreparedPairColor).cast(),
                )
            };
            if !compared {
                error(s, 19);
            }
            compare_native_exit(s, image);
            let w = s.work.as_mut().expect("work");
            if !compare_native_output(w, image) {
                let export = ex!(CyberpunkVR_PreparedNativeExport);
                if export[11] == 0 {
                    export[11] = 15;
                }
                error(s, 46);
            }
            let w = s.work.as_mut().expect("work");
            let none = D3D12_RANGE::default();
            // SAFETY: the mapping above.
            unsafe { w.image_readback.as_ref().expect("readback").Unmap(0, Some(&none)) };
        }
        // SAFETY: the exports.
        let errors = unsafe { crate::prepared_pair_images::cvr_npp_baseline_verdict(addr_of_mut!(CyberpunkVR_PreparedBaseline).cast(), addr_of_mut!(CyberpunkVR_PreparedPairStats).cast()) };
        if errors != 0 {
            if errors & 0xff != 0 {
                error(s, errors & 0xff);
            }
            if errors >> 8 != 0 {
                error(s, errors >> 8);
            }
        }
    }
    let w = s.work.as_mut().expect("work");
    w.complete = true;
    ACTIVE.store(false, SeqCst);
    publish(s);
    // Results live in the exported POD arrays. GPU completion above permits dropping descriptor banks and source pins
    // before the next native capture.
    s.work = None;
}

// ---- the self-tests (native_probe_test runs them)

fn test_native_binding_modes() -> bool {
    struct Saved {
        previous: Option<Box<Work>>,
        was_active: bool,
        arrays: Vec<(*mut u64, Vec<u64>)>,
        atomics: [u32; 7],
        source_count: u32,
        plan_count: u32,
        membership_count: u32,
    }
    fn atomics() -> [&'static core::sync::atomic::AtomicU32; 7] {
        [
            &CyberpunkVR_PreparedPairRequest,
            &CyberpunkVR_PreparedPairState,
            &CyberpunkVR_PreparedPairSelection,
            &CyberpunkVR_PreparedPairSelectionCount,
            &CyberpunkVR_PreparedPairInputFault,
            &CyberpunkVR_PreparedNativeExportEnable,
            &CyberpunkVR_PreparedActualMembershipEnable,
        ]
    }
    macro_rules! span {
        ($name:ident) => {
            // SAFETY: an export's words.
            unsafe { (addr_of_mut!($name).cast::<u64>(), core::mem::size_of_val(&*addr_of_mut!($name)) / 8) }
        };
    }
    let spans = [
        span!(CyberpunkVR_PreparedPairStats),
        span!(CyberpunkVR_PreparedCurrent),
        span!(CyberpunkVR_PreparedBaseline),
        span!(CyberpunkVR_PreparedPairCandidates),
        span!(CyberpunkVR_PreparedPairStateMismatch),
        span!(CyberpunkVR_PreparedPairGpu),
        span!(CyberpunkVR_PreparedPairColor),
        span!(CyberpunkVR_PreparedPairDepthState),
        span!(CyberpunkVR_PreparedPairMeshStats),
        span!(CyberpunkVR_PreparedPairMeshResults),
        span!(CyberpunkVR_PreparedPairVrs),
        span!(CyberpunkVR_PreparedPairVrsGpu),
        span!(CyberpunkVR_PreparedPairInputStats),
        span!(CyberpunkVR_PreparedPairResidency),
        span!(CyberpunkVR_PreparedBatchStats),
        span!(CyberpunkVR_PreparedBatchDescriptors),
        span!(CyberpunkVR_PreparedNativeExit),
        span!(CyberpunkVR_PreparedNativeExitSamples),
        span!(CyberpunkVR_PreparedNativeRate),
        span!(CyberpunkVR_PreparedVrsAttribution),
        span!(CyberpunkVR_PreparedNativeAcceptance),
        span!(CyberpunkVR_PreparedNativeExport),
        span!(CyberpunkVR_PreparedActualMembershipStats),
        span!(CyberpunkVR_PreparedActualMembershipRows),
    ];
    let saved = {
        let mut guard = lock();
        if active() {
            return false;
        }
        let s = guard.state();
        Saved {
            previous: s.work.take(),
            was_active: ACTIVE.swap(false, SeqCst),
            // SAFETY: the exports' words, copied.
            arrays: spans.iter().map(|&(p, n)| (p, unsafe { core::slice::from_raw_parts(p, n) }.to_vec())).collect(),
            atomics: atomics().map(|a| a.load(SeqCst)),
            // SAFETY: as ex!.
            source_count: unsafe { CyberpunkVR_PreparedBatchSourceCount },
            plan_count: unsafe { CyberpunkVR_PreparedBatchPlanCount },
            membership_count: unsafe { CyberpunkVR_PreparedActualMembershipCount },
        }
    };
    let valid = (|| {
        let mut valid = !needs_native_bindings();
        {
            let mut guard = lock();
            guard.state().work = Some(Work::new(null_mut(), null(), 0));
            ACTIVE.store(true, SeqCst);
        }
        valid = needs_native_bindings() && valid;
        let state = CyberpunkVR_PreparedPairState.load(SeqCst);
        let mode = ex!(CyberpunkVR_PreparedCurrent)[0];
        valid = !start_current_sequence(null_mut(), null_mut(), null(), 0, null_mut(), 0, None)
            && needs_native_bindings()
            && CyberpunkVR_PreparedPairState.load(SeqCst) == state
            && ex!(CyberpunkVR_PreparedCurrent)[0] == mode
            && valid;
        {
            let mut guard = lock();
            guard.state().work.as_mut().expect("work").current = true;
        }
        valid = !needs_native_bindings() && valid;
        for repeated in 0..3 {
            let copy;
            {
                let mut guard = lock();
                let w = guard.state().work.as_mut().expect("work");
                w.source_frame = 73;
                w.export_requested = true;
                w.export_prepared = repeated == 0;
                w.execution_issued = repeated == 1;
                w.submitted = repeated == 2;
                w.output_copy = Some(OutputCopy::new());
                copy = w.output_copy.as_ref().expect("copy").0;
                *ex!(CyberpunkVR_PreparedNativeExport) = [17; 24];
            }
            valid = !prepare_native_export(null_mut(), 73, null(), null()) && valid;
            {
                let mut guard = lock();
                let w = guard.state().work.as_ref().expect("work");
                valid = w.output_copy.as_ref().is_some_and(|c| c.0 == copy)
                    && w.export_prepared == (repeated == 0)
                    && w.execution_issued == (repeated == 1)
                    && w.submitted == (repeated == 2)
                    && ex!(CyberpunkVR_PreparedNativeExport).iter().all(|&v| v == 17)
                    && valid;
            }
        }
        {
            let mut guard = lock();
            ACTIVE.store(false, SeqCst);
            guard.state().work = None;
        }
        for (queue, frame) in [(null_mut(), 0u32), (null_mut(), 7), (1usize as *mut c_void, 0)] {
            {
                let _guard = lock();
                *ex!(CyberpunkVR_PreparedCurrent) = [u64::MAX; 16];
                ex!(CyberpunkVR_PreparedPairStats)[10] = 99;
                CyberpunkVR_PreparedPairState.store(5, SeqCst);
            }
            let current = ex!(CyberpunkVR_PreparedCurrent);
            valid = !start_current_sequence(null_mut(), null_mut(), null(), 0, queue, frame, None)
                && !active()
                && !needs_native_bindings()
                && current[0] == 1
                && current[1] == u64::from(frame)
                && current[4] == 0
                && current[5] == 0
                && current[6] == 0
                && current[7] == 0
                && current[12] == 35
                && current[13] == 0
                && ex!(CyberpunkVR_PreparedPairStats)[10] == 35
                && CyberpunkVR_PreparedPairState.load(SeqCst) == 6
                && valid;
        }
        valid
    })();
    {
        let mut guard = lock();
        ACTIVE.store(false, SeqCst);
        let s = guard.state();
        s.work = saved.previous;
        for (p, values) in &saved.arrays {
            // SAFETY: the exports' words, restored.
            unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), *p, values.len()) };
        }
        for (atomic, value) in atomics().iter().zip(saved.atomics) {
            atomic.store(value, SeqCst);
        }
        // SAFETY: as ex!.
        unsafe {
            CyberpunkVR_PreparedBatchSourceCount = saved.source_count;
            CyberpunkVR_PreparedBatchPlanCount = saved.plan_count;
            CyberpunkVR_PreparedActualMembershipCount = saved.membership_count;
        }
        ACTIVE.store(saved.was_active, SeqCst);
    }
    valid
}
fn test_released_reset_race() -> bool {
    let callback;
    {
        let mut guard = lock();
        let s = guard.state();
        if active() || s.work.is_some() {
            return false;
        }
        s.work = Some(Work::new(null_mut(), null(), 0));
        ACTIVE.store(true, SeqCst);
        TEST_RESET_ENTERED.store(false, SeqCst);
        callback = std::thread::spawn(|| reset(null_mut()));
        while !TEST_RESET_ENTERED.load(SeqCst) {
            std::thread::yield_now();
        }
        ACTIVE.store(false, SeqCst);
        s.work = None;
    }
    let _ = callback.join();
    let mut guard = lock();
    !active() && guard.state().work.is_none()
}

// ---- the C ABI (the .cpp's API forwards)

/// Start / StartSequence: `aux` (the holder of the lent sources) is this module's from here on.
///
/// # Safety
/// `views` holds `count` sources alive while `aux` lives; `family` is the single family's pipeline (its lookup key) or null.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp2_start(device: *mut c_void, aux: *mut c_void, views: *const SourceView, count: usize, sequence: bool, family: *mut c_void) -> bool {
    start_internal(device, aux, views, count, sequence, family, null_mut(), 0, None)
}
/// StartCurrentSequence.
///
/// # Safety
/// As cvr_npp2_start.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp2_start_current(device: *mut c_void, aux: *mut c_void, views: *const SourceView, count: usize, queue: *mut c_void, frame: u32) -> bool {
    start_current_sequence(device, aux, views, count, queue, frame, None)
}
/// Start / StartSequence over a Rust caller's sources (the retained capture's): `family` the single family's pipeline
/// (its lookup key) or null.
pub fn start_rust(device: *mut c_void, sources: Vec<crate::pair_source::PairSource>, sequence: bool, family: *mut c_void) -> bool {
    let lent = crate::pair_source::lend(sources);
    let (views, count) = (lent.views.as_ptr(), lent.views.len());
    start_internal(device, rust_holder(), views, count, sequence, family, null_mut(), 0, Some(lent))
}
/// StartCurrentSequence over a Rust caller's sources.
pub fn start_current_rust(device: *mut c_void, sources: Vec<crate::pair_source::PairSource>, queue: *mut c_void, frame: u32) -> bool {
    let lent = crate::pair_source::lend(sources);
    let (views, count) = (lent.views.as_ptr(), lent.views.len());
    start_current_sequence(device, rust_holder(), views, count, queue, frame, Some(lent))
}
/// CompatibilityFailure of two Rust sources.
pub fn compatibility_rust(a: &crate::pair_source::PairSource, b: &crate::pair_source::PairSource) -> u32 {
    let lent = crate::pair_source::lend(vec![a.clone(), b.clone()]);
    compatibility_failure(&lent.views[0], &lent.views[1])
}
/// CompatibilityFailure of two lent sources.
///
/// # Safety
/// The views are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp2_compatibility(a: *const SourceView, b: *const SourceView) -> u32 {
    // SAFETY: as documented.
    unsafe { compatibility_failure(&*a, &*b) }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_needs_native_bindings() -> bool {
    needs_native_bindings()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_wants_native_export(frame: u32) -> bool {
    let mut guard = lock();
    guard.state().work.as_ref().is_some_and(|w| w.current && w.source_frame == frame && w.export_requested)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_cancel_current(frame: u32) {
    let mut guard = lock();
    let s = guard.state();
    if s.work.as_ref().is_some_and(|w| w.current && w.source_frame == frame) {
        fail_current(s, 39);
    }
}
/// PrepareNativeExport (`receipt` and `lifetimes` the caller's target_access::Receipt and BorrowedScope array).
///
/// # Safety
/// The pointers are the caller's values, alive through the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp2_prepare_export(queue: *mut c_void, frame: u32, receipt: *const c_void, lifetimes: *const c_void) -> bool {
    prepare_native_export(queue, frame, receipt, lifetimes)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_submit_current(queue: *mut c_void, frame: u32) -> bool {
    submit_current(queue, frame)
}
/// # Safety
/// `record` is the probe's draw.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp2_draw(list: *mut c_void, record: *const DrawRecord) {
    // SAFETY: as documented.
    draw(list, unsafe { &*record });
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_before_submit(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    before_submit(queue, count, lists);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    submitted(queue, count, lists);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_reset(list: *mut c_void) {
    reset(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_poll() {
    poll();
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_test_binding_modes() -> bool {
    test_native_binding_modes()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp2_test_released_reset_race() -> bool {
    test_released_reset_race()
}
