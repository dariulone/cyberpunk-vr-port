//! The instance snapshot view's owners (formerly the C++ half of src/Render/InstanceSnapshotView.cpp): the plan's
//! versions -- each a validated source scope, its pipeline and shader snapshot, the reserved mapping and the CPU bytes
//! -- and the two owner objects the views name: the overlay (an uploaded plan, recorded once) and the native GPU capture
//! arena (versions admitted one by one while the source callback records its copy). A version is qualified while its
//! pipeline still carries the very shader snapshot it was built from and that snapshot's instance layout is readable.
//!
//! The owners are COM objects of this module (QueryInterface for their owner types) wrapping the GPU core of
//! instance_snapshot_arena.rs. The C++ face keeps the header's classes and forwards to `cvr_isv2_*`; the shader
//! snapshot stays a C++ `std::shared_ptr<const Snapshot>`, held here as a heap copy through the host table the .cpp
//! attaches (current, retain, release, get, id, layout). tools/single_pass_tests/instance_snapshot_view_test runs its
//! cases against this in its Rust variant.
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{ID3D12PipelineState, ID3D12Resource};

use crate::instance_snapshot_arena::{self as arena, Completion, Destination, DrawArgs, Label, NativeCompletion, PrivateView, Upload, VersionView};
use crate::instance_snapshot_view::{reserve, valid_scope, view_shape, Mapping, Scope, VertexBufferView, MAX_BYTES};
use crate::native_recording::Lease;

const MAX_VERSIONS: usize = 2048;
/// instance_snapshot_view::Failure.
mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const SCOPE: u32 = 2;
    pub const LAYOUT: u32 = 3;
    pub const VIEW: u32 = 4;
    pub const RANGE: u32 = 5;
    pub const BYTES: u32 = 6;
    pub const CAPACITY: u32 = 7;
    pub const EXCEPTION: u32 = 13;
}

// ---- the C++ host: the shader snapshots

/// The .cpp's host table over `std::shared_ptr<const shader_snapshot::Snapshot>` (each `*const c_void` below points
/// at one such shared_ptr: an Input's, or a heap copy from `retain`).
#[repr(C)]
pub struct Host {
    /// shader_snapshot::Capture(pipeline).get(): the snapshot the pipeline carries now, or null.
    pub current: unsafe extern "C" fn(*mut c_void) -> *const c_void,
    /// A new heap copy of the shared_ptr (null when it is empty).
    pub retain: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    /// Deletes a heap copy.
    pub release: unsafe extern "C" fn(*mut c_void),
    /// shared_ptr::get.
    pub get: unsafe extern "C" fn(*const c_void) -> *const c_void,
    /// The snapshot's id (0 when empty).
    pub id: unsafe extern "C" fn(*const c_void) -> u64,
    /// detail::Layout(snapshot->input, stride) (false when empty).
    pub layout: unsafe extern "C" fn(*const c_void, u32) -> bool,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// The .cpp's host table (static storage).
///
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_attach(host: *const Host) {
    HOST.store(host as *mut Host, SeqCst);
}
/// The plugin's host (formerly InstanceSnapshotView.cpp's): the shader snapshots are shader_snapshot_native.rs' Rust
/// records. The Rust test variants attach the .cpp's.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::Host;
    use crate::shader_snapshot_native::plugin as shs;
    use core::ffi::c_void;
    /// shader_snapshot::Capture(pipeline).get().
    unsafe extern "C" fn current(pipeline: *mut c_void) -> *const c_void {
        crate::shader_snapshot_native::source(pipeline).map_or(core::ptr::null(), |s| s.info().mirror.identity)
    }
    unsafe extern "C" fn retain(shared: *const c_void) -> *mut c_void {
        shs::retain(shared)
    }
    unsafe extern "C" fn release(shared: *mut c_void) {
        // SAFETY: a heap reference made by retain.
        unsafe { shs::free(shared) }
    }
    unsafe extern "C" fn get(shared: *const c_void) -> *const c_void {
        shs::get(shared)
    }
    unsafe extern "C" fn id(shared: *const c_void) -> u64 {
        shs::id(shared)
    }
    unsafe extern "C" fn layout(shared: *const c_void, stride: u32) -> bool {
        shs::layout(shared, stride)
    }
    pub(super) static HOST: Host = Host { current, retain, release, get, id, layout };
}
fn host() -> &'static Host {
    // SAFETY: attached at the C++ face's static initialization, before any owner exists.
    let attached = unsafe { HOST.load(SeqCst).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::HOST));
    attached.expect("instance snapshot view host")
}
pub(crate) fn current(pipeline: &ID3D12PipelineState) -> *const c_void {
    // SAFETY: the host's own function on a live pipeline.
    unsafe { (host().current)(pipeline.as_raw()) }
}
pub(crate) fn shared_get(shared: *const c_void) -> *const c_void {
    // SAFETY: a live shared_ptr.
    unsafe { (host().get)(shared) }
}
pub(crate) fn shared_id(shared: *const c_void) -> u64 {
    // SAFETY: a live shared_ptr.
    unsafe { (host().id)(shared) }
}
pub(crate) fn shared_layout(shared: *const c_void, stride: u32) -> bool {
    // SAFETY: a live shared_ptr.
    unsafe { (host().layout)(shared, stride) }
}
/// A heap copy of a shader snapshot's shared_ptr.
struct Shader(*mut c_void);
// SAFETY: a std::shared_ptr copy of an immutable snapshot, only read and released.
unsafe impl Send for Shader {}
// SAFETY: as above.
unsafe impl Sync for Shader {}
impl Shader {
    fn retain(shared: *const c_void) -> Option<Self> {
        // SAFETY: a live shared_ptr.
        let heap = unsafe { (host().retain)(shared) };
        (!heap.is_null()).then(|| Self(heap))
    }
    fn get(&self) -> *const c_void {
        shared_get(self.0)
    }
}
impl Drop for Shader {
    fn drop(&mut self) {
        // SAFETY: this value's heap copy.
        unsafe { (host().release)(self.0) };
    }
}

// ---- versions

/// detail::Version.
struct Version {
    scope: Scope,
    mapping: Mapping,
    pipeline: ID3D12PipelineState,
    shader: Shader,
    shader_id: u64,
    upload_offset: u64,
    original: VertexBufferView,
    draw: DrawArgs,
    bytes: Vec<u8>,
}
/// Qualified: the pipeline still carries this very snapshot (so its id and input layout are the saved ones -- the
/// snapshot is immutable) and the layout is the readable instance layout.
fn qualified(version: &Version) -> bool {
    let now = current(&version.pipeline);
    !now.is_null()
        && now == version.shader.get()
        && version.shader_id != 0
        && shared_id(version.shader.0) == version.shader_id
        && shared_layout(version.shader.0, version.original.stride_in_bytes)
}
fn lend(version: &Version) -> VersionView {
    VersionView { scope: version.scope, mapping: version.mapping, original: version.original, draw: version.draw }
}
/// View: the version's original view at its mapped offset in `resource`.
fn view_of(version: &Version, resource: *mut c_void) -> VertexBufferView {
    // SAFETY: the owner's live buffer or null.
    let gpu = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.map_or(0, |r| unsafe { r.GetGPUVirtualAddress() });
    VertexBufferView { buffer_location: gpu.wrapping_add(version.mapping.view_offset), ..version.original }
}
/// instance_snapshot_view::Input as the face lends it.
#[repr(C)]
pub struct InputView {
    pub scope: Scope,
    pub pipeline: *mut c_void,
    pub shader: *const c_void,
    pub original: VertexBufferView,
    pub draw: DrawArgs,
    pub bytes: *const u8,
    pub size: usize,
    pub has_bytes: bool,
}
/// A PrivateView's parts, the references borrowed from the owner (the face copies them).
#[repr(C)]
pub struct ViewOut {
    pub scope: Scope,
    pub resource: *mut c_void,
    pub pipeline: *mut c_void,
    pub shader: *const c_void,
    pub vertex: VertexBufferView,
    pub original: VertexBufferView,
    pub draw: DrawArgs,
    pub mapping: Mapping,
    pub copy_list: u64,
    pub copy_generation: u64,
    pub version: u32,
}
const _: () = assert!(core::mem::size_of::<InputView>() == 120 && core::mem::size_of::<ViewOut>() == 192);
fn view_out(version: &Version, index: u32, label: &Label) -> ViewOut {
    ViewOut {
        scope: version.scope,
        resource: label.resource,
        pipeline: version.pipeline.as_raw(),
        shader: version.shader.0,
        vertex: view_of(version, label.resource),
        original: version.original,
        draw: version.draw,
        mapping: version.mapping,
        copy_list: label.list,
        copy_generation: label.generation,
        version: index,
    }
}
fn pipeline_of(pipeline: *mut c_void) -> Option<ID3D12PipelineState> {
    // SAFETY: the input's live pipeline or null.
    unsafe { ID3D12PipelineState::from_raw_borrowed(&pipeline) }.cloned()
}

// ---- plans

/// detail::PlanData.
pub struct PlanData {
    versions: Vec<Version>,
    width: u64,
    copied: u64,
}
/// BuildPlan.
fn build_plan(inputs: &[InputView], budget: u64) -> Result<PlanData, u32> {
    if inputs.is_empty() || inputs.len() > MAX_VERSIONS || budget == 0 || budget > MAX_BYTES {
        return Err(failure::ARGUMENTS);
    }
    let mut pending = PlanData { versions: Vec::with_capacity(inputs.len()), width: 0, copied: 0 };
    let mut cursor = 0u64;
    let mut seen = HashSet::new();
    for input in inputs {
        let s = &input.scope;
        if !valid_scope(s) || !seen.insert((s.root, s.list, s.generation, s.occurrence, s.eye)) {
            return Err(failure::SCOPE);
        }
        let Some(pipeline) = pipeline_of(input.pipeline) else { return Err(failure::LAYOUT) };
        let shader = shared_get(input.shader);
        let shader_id = shared_id(input.shader);
        if shader.is_null() || shader_id == 0 || current(&pipeline) != shader || !shared_layout(input.shader, input.original.stride_in_bytes) {
            return Err(failure::LAYOUT);
        }
        if !view_shape(&input.original) {
            return Err(failure::VIEW);
        }
        if input.draw.index_count == 0 {
            return Err(failure::RANGE);
        }
        let Some(shader) = Shader::retain(input.shader) else { return Err(failure::EXCEPTION) };
        let Some(mapping) = reserve(cursor, &input.original, input.draw.start_instance, input.draw.instance_count, budget) else {
            return Err(failure::CAPACITY);
        };
        if !input.has_bytes || input.size as u64 != mapping.copy_bytes || input.size == 0 {
            return Err(failure::BYTES);
        }
        // SAFETY: the input's live bytes.
        let bytes = unsafe { core::slice::from_raw_parts(input.bytes, input.size) }.to_vec();
        let version = Version { scope: input.scope, mapping, pipeline, shader, shader_id, upload_offset: pending.copied, original: input.original, draw: input.draw, bytes };
        cursor = mapping.next_free;
        pending.width = pending.width.max(mapping.view_end);
        if mapping.copy_bytes > MAX_BYTES - pending.copied {
            return Err(failure::CAPACITY);
        }
        pending.copied += mapping.copy_bytes;
        pending.versions.push(version);
    }
    Ok(pending)
}

// ---- the owners

const OVERLAY_TYPE: GUID = GUID::from_values(0x0e1d90db, 0x6f45, 0x442e, [0x89, 0x1c, 0x95, 0x0e, 0xa5, 0x59, 0xe2, 0x7c]);
const ARENA_TYPE: GUID = GUID::from_values(0xec3f3de4, 0x7a7d, 0x4eea, [0x92, 0x26, 0xe9, 0x24, 0x4c, 0xa6, 0xbb, 0x23]);
/// Overlay::Storage or GpuCaptureArena::Storage: the GPU core and the versions (the plan's, or the arena's own).
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    kind: GUID,
    core: *mut c_void,
    plan: Option<Arc<PlanData>>,
    versions: UnsafeCell<Vec<Version>>,
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers; a live owner.
    unsafe {
        *out = null_mut();
        if *iid != IUnknown::IID && *iid != (*this.cast::<Owner>()).kind {
            return E_NOINTERFACE;
        }
        *out = this;
        owner_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live owner.
    unsafe { (*this.cast::<Owner>()).references.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live owner; the last reference frees it.
    let left = unsafe { (*this.cast::<Owner>()).references.fetch_sub(1, SeqCst) - 1 };
    if left == 0 {
        // SAFETY: made by Box::into_raw in new_owner.
        drop(unsafe { Box::from_raw(this.cast::<Owner>()) });
    }
    left
}
impl Drop for Owner {
    fn drop(&mut self) {
        // The versions go before the core (the C++ Storage's members before ~RustOwner).
        self.plan = None;
        drop(core::mem::take(self.versions.get_mut()));
        // SAFETY: the owner's core, freed once.
        unsafe { arena::cvr_isv_free(self.core) };
    }
}
fn new_owner(kind: GUID, core: *mut c_void, plan: Option<Arc<PlanData>>, versions: Vec<Version>) -> *mut c_void {
    Box::into_raw(Box::new(Owner { vtbl: &OWNER_VTBL, references: AtomicU32::new(1), kind, core, plan, versions: UnsafeCell::new(versions) })).cast()
}
impl Owner {
    fn versions(&self) -> &[Version] {
        match &self.plan {
            Some(plan) => &plan.versions,
            // SAFETY: the arena's versions grow only in its single-threaded capture (the C++ object's contract).
            None => unsafe { &*self.versions.get() },
        }
    }
}
/// A held owner reference of one kind.
struct OwnerRef(*mut c_void);
impl OwnerRef {
    fn get(&self) -> &Owner {
        // SAFETY: a live owner while this reference is held.
        unsafe { &*self.0.cast::<Owner>() }
    }
}
impl Drop for OwnerRef {
    fn drop(&mut self) {
        // SAFETY: the reference this value holds.
        unsafe { owner_release(self.0) };
    }
}
/// The owner behind a view's IUnknown (QueryInterface for `kind`), held.
fn owner_of(value: *mut c_void, kind: &GUID) -> Option<OwnerRef> {
    // SAFETY: the view's live owner or null.
    let value = unsafe { IUnknown::from_raw_borrowed(&value) }?;
    let mut out = null_mut();
    // SAFETY: a QueryInterface for the owner type.
    let hr = unsafe { (Interface::vtable(value).QueryInterface)(value.as_raw(), kind, &mut out) };
    (hr.is_ok() && !out.is_null()).then(|| OwnerRef(out))
}
/// The face's own owner pointer (its ComPtr's), borrowed.
fn owner<'a>(storage: *mut c_void) -> Option<&'a Owner> {
    // SAFETY: an owner this module made, held by the face.
    unsafe { storage.cast::<Owner>().as_ref() }
}
fn label(core: *mut c_void) -> (Label, bool) {
    let mut out = Label { resource: null_mut(), list: 0, generation: 0 };
    // SAFETY: a live core.
    let recorded = unsafe { arena::cvr_isv_label(core, &mut out) };
    (out, recorded)
}
/// ValidatePrivateView / ValidateNativeView.
fn view_valid(owner: &OwnerRef, view: &PrivateView, shader: *const c_void) -> bool {
    let o = owner.get();
    let Some(version) = o.versions().get(view.version as usize) else { return false };
    let lent = lend(version);
    qualified(version)
        && view.pipeline == version.pipeline.as_raw()
        && shader == version.shader.get()
        // SAFETY: the owner's core; the view and the version lent.
        && unsafe { arena::cvr_isv_view_valid(o.core, view, &lent) }
}

// ---- the C ABI (the C++ face's CVR_RUST_VRIK branch)

/// BuildPlan: the plan (one Arc reference the face's PlanData owns) or null, the failure in `*code`.
///
/// # Safety
/// `inputs` holds `count` lent inputs; `code` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_build_plan(inputs: *const InputView, count: usize, budget: u64, code: *mut u32) -> *const PlanData {
    let inputs = if inputs.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(inputs, count) } };
    let (plan, value) = match catch_unwind(AssertUnwindSafe(|| build_plan(inputs, budget))) {
        Ok(Ok(plan)) => (Arc::into_raw(Arc::new(plan)), failure::NONE),
        Ok(Err(value)) => (null(), value),
        Err(_) => (null(), failure::EXCEPTION),
    };
    // SAFETY: as documented.
    unsafe { *code = value };
    plan
}
/// # Safety
/// `plan` from cvr_isv2_build_plan, released once.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_plan_free(plan: *const PlanData) {
    if !plan.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Arc::from_raw(plan) });
    }
}
/// The plan's version count (0), width (1) or copied bytes (2).
///
/// # Safety
/// A live plan.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_plan_count(plan: *const PlanData, which: u32) -> u64 {
    // SAFETY: as documented.
    let plan = unsafe { &*plan };
    match which {
        0 => plan.versions.len() as u64,
        1 => plan.width,
        _ => plan.copied,
    }
}
/// A version's mapping (0) or scope (1), null out of range (alive with the plan).
///
/// # Safety
/// A live plan.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_plan_version(plan: *const PlanData, index: u32, which: u32) -> *const c_void {
    // SAFETY: as documented.
    let plan = unsafe { &*plan };
    plan.versions.get(index as usize).map_or(null(), |v| if which == 0 { (&v.mapping as *const Mapping).cast() } else { (&v.scope as *const Scope).cast() })
}
/// Overlay::Create: the owner (one reference) into `*out`.
///
/// # Safety
/// A live device or null; a live plan; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_overlay_new(device: *mut c_void, plan: *const PlanData, out: *mut *mut c_void) -> HRESULT {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as documented.
        let plan = unsafe {
            Arc::increment_strong_count(plan);
            Arc::from_raw(plan)
        };
        let count = plan.versions.len();
        if device.is_null() || count == 0 || plan.width == 0 || plan.width > MAX_BYTES || plan.copied == 0 || plan.copied > MAX_BYTES {
            return E_INVALIDARG;
        }
        let mut uploads = Vec::with_capacity(count);
        for version in &plan.versions {
            if !qualified(version) {
                return E_INVALIDARG;
            }
            uploads.push(Upload {
                target: version.mapping.target_offset,
                upload: version.upload_offset,
                bytes: version.mapping.copy_bytes,
                data: version.bytes.as_ptr(),
                size: version.bytes.len(),
                pipeline: version.pipeline.as_raw(),
            });
        }
        let mut core = null_mut();
        // SAFETY: a live device; the uploads lent.
        let result = unsafe { arena::cvr_isv_overlay_new(device, plan.width, plan.copied, uploads.as_ptr(), uploads.len(), &mut core) };
        if result.is_ok() {
            // SAFETY: as documented.
            unsafe { *out = new_owner(OVERLAY_TYPE, core, Some(plan), Vec::new()) };
        } else {
            // SAFETY: a failed creation's core (null or partial), freed.
            unsafe { arena::cvr_isv_free(core) };
        }
        result
    }))
    .unwrap_or(E_OUTOFMEMORY)
}
/// Overlay::Record: the failure.
///
/// # Safety
/// The face's owner; a live list or null; `destination` lent.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_overlay_record(storage: *mut c_void, list: *mut c_void, destination: *const Destination) -> u32 {
    let Some(o) = owner(storage) else { return failure::ARGUMENTS };
    catch_unwind(AssertUnwindSafe(|| {
        let qualified = o.versions().iter().all(qualified);
        // SAFETY: the owner's core; the destination lent.
        unsafe { arena::cvr_isv_overlay_record(o.core, list, storage, destination, qualified) }
    }))
    .unwrap_or_else(|_| {
        // SAFETY: the owner's core.
        unsafe { arena::cvr_isv_fail(o.core, false) };
        failure::EXCEPTION
    })
}
/// Overlay::GetView.
///
/// # Safety
/// The face's owner; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_overlay_view(storage: *mut c_void, index: u32, out: *mut ViewOut) -> bool {
    let Some(o) = owner(storage) else { return false };
    let (label, recorded) = label(o.core);
    let Some(version) = o.versions().get(index as usize).filter(|_| recorded) else { return false };
    // SAFETY: as documented.
    unsafe { *out = view_out(version, index, &label) };
    true
}
/// The owner's version count (0), required (1) or copied (2) bytes.
///
/// # Safety
/// The face's owner.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_count(storage: *mut c_void, which: u32) -> u64 {
    let Some(o) = owner(storage) else { return 0 };
    match (&o.plan, which) {
        (_, 0) => o.versions().len() as u64,
        (Some(plan), 1) => plan.width,
        (Some(plan), _) => plan.copied,
        // SAFETY: the arena's core.
        (None, n) => unsafe { arena::cvr_isv_arena_bytes(o.core, n - 1) },
    }
}
/// Overlay::ValidateDestinationSubmission.
///
/// # Safety
/// The face's owner; `destination` lent; a live queue or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_overlay_submitted(storage: *mut c_void, destination: *const Destination, queue: *mut c_void) -> bool {
    let Some(o) = owner(storage) else { return false };
    // SAFETY: the owner's core; the destination lent.
    unsafe { arena::cvr_isv_overlay_valid(o.core, 0, destination, queue, null_mut(), null_mut(), null(), null()) }
}
/// The views' checks. Overlay (arena false): kind 0 ValidatePrivateView, 1 ForRecording (`list`, `destination`),
/// 2 AfterCompletion (`completion`). Arena: 0 ValidateNativeView, 1 ForRecording (`list`, `recording`),
/// 2 AfterSubmission and 3 AfterCompletion (`native`).
///
/// # Safety
/// `view` lent with its shader snapshot's object; the pointers the kind reads are live.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_view_check(
    arena_view: bool,
    kind: u32,
    view: *const PrivateView,
    shader: *const c_void,
    list: *mut c_void,
    destination: *const Destination,
    completion: *const Completion,
    recording: *const Lease,
    native: *const NativeCompletion,
) -> bool {
    // SAFETY: as documented.
    let view = unsafe { &*view };
    catch_unwind(AssertUnwindSafe(|| {
        let Some(held) = owner_of(view.owner, if arena_view { &ARENA_TYPE } else { &OVERLAY_TYPE }) else { return false };
        if !view_valid(&held, view, shader) {
            return false;
        }
        let o = held.get();
        // SAFETY: the owner's core; the pointers the kind reads, lent.
        unsafe {
            match (arena_view, kind) {
                (_, 0) => true,
                (false, 1) => !list.is_null() && arena::cvr_isv_overlay_valid(o.core, 1, destination, null_mut(), list, held.0, view, null()),
                (false, _) => arena::cvr_isv_overlay_valid(o.core, 2, null(), null_mut(), null_mut(), null_mut(), null(), completion),
                (true, 1) => !list.is_null() && view.copy_list == list as u64 && arena::cvr_isv_arena_valid(o.core, 1, list, held.0, recording, null()),
                (true, k) => arena::cvr_isv_arena_valid(o.core, k, null_mut(), null_mut(), null(), native),
            }
        }
    }))
    .unwrap_or(false)
}
/// GpuCaptureArena::Create: the owner (one reference) into `*out`.
///
/// # Safety
/// A live device; `recording` lent; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_arena_new(device: *mut c_void, recording: *const Lease, eye: u32, capacity: u64, out: *mut *mut c_void) -> HRESULT {
    let mut core = null_mut();
    // SAFETY: as documented.
    let result = unsafe { arena::cvr_isv_arena_new(device, recording, eye, capacity, &mut core) };
    if result.is_err() {
        // SAFETY: a failed creation's core (null or partial), freed.
        unsafe { arena::cvr_isv_free(core) };
        return result;
    }
    // SAFETY: as documented.
    unsafe { *out = new_owner(ARENA_TYPE, core, None, Vec::with_capacity(MAX_VERSIONS)) };
    S_OK
}
/// The face's record callback: the native copy into the arena's fresh range.
pub type RecordNativeCopy = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *mut c_void, u64) -> bool;
/// GpuCaptureArena::Capture.
///
/// # Safety
/// The face's owner; a live list; `input` lent with the face's Input it describes (`source`); `out` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_arena_capture(
    storage: *mut c_void,
    list: *mut c_void,
    input: *const InputView,
    record: RecordNativeCopy,
    context: *mut c_void,
    source: *const c_void,
    out: *mut ViewOut,
) -> bool {
    // SAFETY: as documented.
    let input = unsafe { &*input };
    // SAFETY: the face's callback with its own Input.
    let captured = capture_with(storage, list, input, &mut |resource, offset| unsafe { record(context, list, source, resource, offset) });
    match captured {
        Some(view) => {
            // SAFETY: as documented.
            unsafe { *out = view };
            true
        }
        None => false,
    }
}
/// GpuCaptureArena::Capture with a copy recorder of this crate's: the view, or None (a panic fails the arena).
fn capture_with(storage: *mut c_void, list: *mut c_void, input: &InputView, record: &mut dyn FnMut(*mut c_void, u64) -> bool) -> Option<ViewOut> {
    let o = owner(storage)?;
    let captured = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the arena's versions (its single-threaded capture).
        let versions = unsafe { &mut *o.versions.get() };
        let shader = shared_get(input.shader);
        if input.pipeline.is_null() || shader.is_null() || shared_id(input.shader) == 0 || versions.iter().any(|v| v.scope.occurrence == input.scope.occurrence) {
            return None;
        }
        let pipeline = pipeline_of(input.pipeline)?;
        let shader = Shader::retain(input.shader)?;
        let shader_id = shared_id(shader.0);
        let mut version = Version { scope: input.scope, mapping: Mapping::default(), pipeline, shader, shader_id, upload_offset: 0, original: input.original, draw: input.draw, bytes: Vec::new() };
        if !qualified(&version)
            // SAFETY: the arena's core; the scope, views and draw lent.
            || !unsafe { arena::cvr_isv_arena_admit(o.core, list, storage, &input.scope, input.pipeline, &input.original, &input.draw, versions.len(), input.has_bytes, &mut version.mapping) }
        {
            return None;
        }
        let index = versions.len() as u32;
        versions.push(version);
        let saved = versions.last().expect("pushed");
        let (label, _) = label(o.core);
        if !record(label.resource, saved.mapping.target_offset) {
            // SAFETY: the arena's core.
            unsafe { arena::cvr_isv_arena_commit(o.core, null()) };
            return None;
        }
        // SAFETY: the arena's core; the saved mapping.
        unsafe { arena::cvr_isv_arena_commit(o.core, &saved.mapping) };
        Some(view_out(saved, index, &label))
    }));
    match captured {
        Ok(view) => view,
        Err(_) => {
            // SAFETY: the arena's core.
            unsafe { arena::cvr_isv_fail(o.core, true) };
            None
        }
    }
}
/// GpuCaptureArena::Finalize.
///
/// # Safety
/// The face's owner; a live list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_arena_finalize(storage: *mut c_void, list: *mut c_void) -> bool {
    let Some(o) = owner(storage) else { return false };
    // SAFETY: the arena's core.
    !list.is_null() && unsafe { arena::cvr_isv_arena_finalize(o.core, list, storage, o.versions().len()) }
}
/// GpuCaptureArena::Resource (borrowed).
///
/// # Safety
/// The face's owner.
#[no_mangle]
pub unsafe extern "C" fn cvr_isv2_resource(storage: *mut c_void) -> *mut c_void {
    owner(storage).map_or(null_mut(), |o| label(o.core).0.resource)
}

// ---- the retained geometry's own calls (its per-eye native capture arena)

/// GpuCaptureArena::Create: the owner.
pub(crate) fn arena_create(device: *mut c_void, recording: &Lease, eye: u32, capacity: u64) -> Result<IUnknown, HRESULT> {
    let mut out = null_mut();
    // SAFETY: a live device or null; the lease lent.
    let result = unsafe { cvr_isv2_arena_new(device, recording, eye, capacity, &mut out) };
    if result.is_err() {
        return Err(result);
    }
    // SAFETY: the new owner's one reference.
    Ok(unsafe { IUnknown::from_raw(out) })
}
/// GpuCaptureArena::Capture: `record(resource, offset)` records the original copy into the fresh range.
pub(crate) fn arena_capture(arena: &IUnknown, list: *mut c_void, input: &InputView, record: &mut dyn FnMut(*mut c_void, u64) -> bool) -> Option<ViewOut> {
    if list.is_null() {
        return None;
    }
    capture_with(arena.as_raw(), list, input, record)
}
/// GpuCaptureArena::Finalize.
pub(crate) fn arena_finalize(arena: &IUnknown, list: *mut c_void) -> bool {
    // SAFETY: an owner of this module.
    unsafe { cvr_isv2_arena_finalize(arena.as_raw(), list) }
}
/// The arena's version count (0), required (1) or copied (2) bytes.
pub(crate) fn arena_count(arena: &IUnknown, which: u32) -> u64 {
    // SAFETY: an owner of this module.
    unsafe { cvr_isv2_count(arena.as_raw(), which) }
}
/// GpuCaptureArena::Resource (borrowed).
pub(crate) fn arena_resource(arena: &IUnknown) -> *mut c_void {
    // SAFETY: an owner of this module.
    unsafe { cvr_isv2_resource(arena.as_raw()) }
}
/// The native views' checks: 0 ValidateNativeView, 1 ForRecording (`list`, `recording`), 2 AfterSubmission and
/// 3 AfterCompletion (`native`); `shader` is the view's snapshot object.
pub(crate) fn native_view_check(kind: u32, view: &PrivateView, shader: *const c_void, list: *mut c_void, recording: *const Lease, native: *const NativeCompletion) -> bool {
    // SAFETY: the view and the pointers the kind reads, lent.
    unsafe { cvr_isv2_view_check(true, kind, view, shader, list, null(), null(), recording, native) }
}
