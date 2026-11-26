//! The early depth pass's object (formerly src/Render/EarlyDepthPass.cpp's Storage, the C++ half around the state and
//! recording of early_depth_pass.rs): an IUnknown, so the command resources keep it alive with the lists that recorded
//! it, owning the device, the two recording tokens, the target arrays and the commit gate (cvr-gpu), the native targets,
//! the instance snapshots and their source and ranges, the shader snapshots and their mapped permits, the shader
//! sources (bone ranges, texture mips, material b4 snapshots), the timestamp queries and readbacks, the readback planes,
//! the DSV heap, the fences and the main queue. include/Render/EarlyDepthPass.hpp's class is the face over it in the
//! Rust build.
//!
//! The texture and material reads stay the C++ half's -- the leases, the copies, the submission checks and their
//! failure codes (the single-pass tests stub them) --, lent through the .cpp's attach, as do the constant arenas'
//! banks (StereoBindingBank over the families' paired-root plans, C++ objects) and Main's bank and family.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::collections::BTreeMap;

use windows::core::{Interface, IUnknown_Vtbl, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, E_UNEXPECTED, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8_UNORM;

use cvr_gpu::depth_commit_gate::{self as gate, BufferProof, Gate};
use cvr_gpu::scene_state::{cvr_scene_bindings_restore, cvr_scene_snapshot, Bindings};
use cvr_gpu::target_array::{self as arrays, TargetArray};

use crate::buffer_read::Proof;
use crate::early_depth_compare::Comparison;
use crate::early_depth_pass::{
    self as edp, Admission, DrawHost, Host, Instances, Objects, Plan, Planned, Range, ReadObjects, Run, Shaders, State,
    MATERIAL_BYTES, MAX_BUFFER_PROOFS, MAX_DRAWS,
};
use crate::geometry_commands::DrawRecord;
use crate::texture_lease::Lease as TextureLease;

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const DEPTH_WRITE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_DEPTH_WRITE;
/// texture_read's copy of a whole R8 texture (Copy) rather than a subresource (CopySubresource).
const WHOLE: u32 = u32::MAX;

/// What the .cpp lends: the texture and material reads and the arenas' banks.
#[repr(C)]
pub struct Reads {
    /// texture_read::Validate(lease, queue).
    pub texture_valid: unsafe extern "C-unwind" fn(*const TextureLease, *mut c_void) -> bool,
    /// texture_read::Copy (mip WHOLE) or CopySubresource(list, lease, mip, buffer, layout).
    pub texture_copy: unsafe extern "C-unwind" fn(*mut c_void, *const TextureLease, u32, *mut c_void, *const D3D12_PLACED_SUBRESOURCE_FOOTPRINT) -> bool,
    /// texture_read::LastFailure.
    pub texture_failure: unsafe extern "C-unwind" fn() -> u32,
    /// material_binding::Lease: a copy, its release, its source buffer.
    pub material_clone: unsafe extern "C-unwind" fn(*const c_void) -> *mut c_void,
    pub material_free: unsafe extern "C-unwind" fn(*mut c_void),
    pub material_resource: unsafe extern "C-unwind" fn(*const c_void) -> *mut c_void,
    /// material_read::CopySnapshot(list, lease, destination, offset, proof).
    pub material_copy: unsafe extern "C-unwind" fn(*mut c_void, *const c_void, *mut c_void, u64, *mut Proof) -> bool,
    /// material_read::ValidateSnapshotSubmission(lease, proof, queue).
    pub material_valid: unsafe extern "C-unwind" fn(*const c_void, *const Proof, *mut c_void) -> bool,
    /// material_read::LastFailure.
    pub material_failure: unsafe extern "C-unwind" fn() -> u32,
}
/// An arena's bank (binding_bank.rs' storage, a reference the pass holds) released.
fn bank_free(bank: *mut c_void) {
    if !bank.is_null() {
        // SAFETY: the pass's reference.
        drop(unsafe { windows::core::IUnknown::from_raw(bank) });
    }
}
static READS: AtomicPtr<Reads> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `reads` lives for the rest of the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_edo_attach(reads: *const Reads) {
    READS.store(reads.cast_mut(), Release);
}
/// The plugin's own reads (formerly lent by src/Render/EarlyDepthPass.cpp). The Rust test variants attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::{Reads, TextureLease, WHOLE};
    use crate::buffer_read::Proof;
    use crate::material_read::Lease as MaterialLease;
    use core::ffi::c_void;
    use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;
    unsafe extern "C-unwind" fn texture_valid(lease: *const TextureLease, queue: *mut c_void) -> bool {
        // SAFETY: the pass's lease (texture_read::Validate).
        unsafe { crate::texture_lease::cvr_tr_validate(lease, queue) }
    }
    unsafe extern "C-unwind" fn texture_copy(list: *mut c_void, lease: *const TextureLease, mip: u32, buffer: *mut c_void, layout: *const D3D12_PLACED_SUBRESOURCE_FOOTPRINT) -> bool {
        // SAFETY: texture_read::Copy (mip WHOLE: the whole texture's subresource 0) or CopySubresource.
        unsafe { crate::texture_lease::cvr_tr_copy_subresource(list, lease, if mip == WHOLE { 0 } else { mip }, buffer, layout) }
    }
    unsafe extern "C-unwind" fn texture_failure() -> u32 {
        crate::texture_lease::cvr_tr_failure()
    }
    /// material_binding::Lease's copy (its buffer referenced) as a Rust box.
    unsafe extern "C-unwind" fn material_clone(lease: *const c_void) -> *mut c_void {
        // SAFETY: the caller's lease.
        let copy = unsafe { core::ptr::read(lease.cast::<MaterialLease>()) };
        crate::structured_snapshot_receipt::add_ref(copy.source.resource);
        Box::into_raw(Box::new(copy)).cast()
    }
    unsafe extern "C-unwind" fn material_free(lease: *mut c_void) {
        if !lease.is_null() {
            // SAFETY: a box material_clone made, released once.
            let copy = unsafe { Box::from_raw(lease.cast::<MaterialLease>()) };
            crate::structured_snapshot_receipt::release(copy.source.resource);
        }
    }
    unsafe extern "C-unwind" fn material_resource(lease: *const c_void) -> *mut c_void {
        // SAFETY: a live copy.
        unsafe { (*lease.cast::<MaterialLease>()).source.resource }
    }
    unsafe extern "C-unwind" fn material_copy(list: *mut c_void, lease: *const c_void, destination: *mut c_void, offset: u64, proof: *mut Proof) -> bool {
        // SAFETY: material_read::CopySnapshot over the pass's copy.
        unsafe { crate::material_read::cvr_mread_copy_snapshot(list, lease.cast(), destination, offset, proof) }
    }
    unsafe extern "C-unwind" fn material_valid(lease: *const c_void, proof: *const Proof, queue: *mut c_void) -> bool {
        // SAFETY: material_read::ValidateSnapshotSubmission over the pass's copy.
        unsafe { crate::material_read::cvr_mread_validate_snapshot_submission(lease.cast(), proof, queue) }
    }
    unsafe extern "C-unwind" fn material_failure() -> u32 {
        crate::material_read::cvr_mread_last_failure()
    }
    pub static READS: Reads = Reads {
        texture_valid,
        texture_copy,
        texture_failure,
        material_clone,
        material_free,
        material_resource,
        material_copy,
        material_valid,
        material_failure,
    };
}
fn reads() -> &'static Reads {
    #[cfg(all(feature = "rust-build", not(test)))]
    if READS.load(Acquire).is_null() {
        return &plugin::READS;
    }
    // SAFETY: attached by the .cpp's static initialisation before any pass exists.
    unsafe { &*READS.load(Acquire) }
}

fn borrowed<T: Interface>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live object of the caller's of that interface, or null.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
fn raw<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(core::ptr::null_mut(), |v| v.as_raw())
}
fn adopt<T: Interface>(raw: *mut c_void) -> Option<T> {
    // SAFETY: an object with the reference its creation took, or null.
    (!raw.is_null()).then(|| unsafe { T::from_raw(raw) })
}

/// A texture lease the pass holds (its resource referenced).
struct Texture(TextureLease);
impl Texture {
    fn of(lease: &TextureLease) -> Self {
        if let Some(r) = borrowed::<ID3D12Resource>(lease.resource) {
            core::mem::forget(r.clone());
        }
        Texture(*lease)
    }
    fn resource(&self) -> *mut c_void {
        self.0.resource
    }
    fn r8(&self) -> bool {
        // SAFETY: the lease's live resource.
        borrowed::<ID3D12Resource>(self.0.resource).is_some_and(|r| unsafe { r.GetDesc() }.Format == DXGI_FORMAT_R8_UNORM)
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        drop(adopt::<ID3D12Resource>(self.0.resource));
    }
}
/// A material lease the pass holds (the .cpp's copy).
struct Material(*mut c_void);
impl Material {
    fn of(lease: *const c_void) -> Self {
        // SAFETY: the caller's lease.
        Material(unsafe { (reads().material_clone)(lease) })
    }
    fn resource(&self) -> *mut c_void {
        // SAFETY: the pass's copy.
        unsafe { (reads().material_resource)(self.0) }
    }
}
impl Drop for Material {
    fn drop(&mut self) {
        // SAFETY: the pass's copy, released once.
        unsafe { (reads().material_free)(self.0) };
    }
}

/// A shader source: a bone range (`buffer`), a texture mip (`texture`) or a material's b4 (`material`), at `offset`
/// in the snapshots.
#[derive(Default)]
struct ShaderSource {
    buffer: Option<ID3D12Resource>,
    source_offset: u64,
    offset: u32,
    bytes: u32,
    texture: Option<Texture>,
    layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    mip: u32,
    row_bytes: u32,
    rows: u32,
    material: Option<Material>,
    material_proof: [Proof; 2],
    expected_index: Option<ID3D12Resource>,
}
impl ShaderSource {
    fn texture_resource(&self) -> *mut c_void {
        self.texture.as_ref().map_or(core::ptr::null_mut(), Texture::resource)
    }
    fn material_resource(&self) -> *mut c_void {
        self.material.as_ref().map_or(core::ptr::null_mut(), Material::resource)
    }
}

/// A recording token: an IUnknown the command resources keep for one side's recording.
#[repr(C)]
struct Token {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
}
unsafe extern "system" fn token_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live token.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Token)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn token_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live token.
    unsafe { (*(this as *mut Token)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn token_release(this: *mut c_void) -> u32 {
    // SAFETY: a live token, freed at zero.
    unsafe {
        let n = (*(this as *mut Token)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Token));
        }
        n
    }
}
static TOKEN_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: token_query, AddRef: token_add_ref, Release: token_release };
/// A token the pass holds (one reference).
struct TokenRef(*mut Token);
impl Drop for TokenRef {
    fn drop(&mut self) {
        // SAFETY: the pass's reference.
        unsafe { token_release(self.0.cast()) };
    }
}

/// The pass.
#[repr(C)]
pub struct Pass {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    s: State,
    device: Option<ID3D12Device>,
    tokens: [Option<TokenRef>; 2],
    targets: [*mut TargetArray; 2],
    native: [Option<ID3D12Resource>; 2],
    gate: *mut Gate,
    instance_snapshots: [Option<ID3D12Resource>; 2],
    instance_source: Option<ID3D12Resource>,
    instance_ranges: Vec<Range>,
    shader_snapshots: [Option<ID3D12Resource>; 2],
    snapshot_permits: [Option<ID3D12Resource>; 2],
    permit_mappings: [*mut c_void; 2],
    shader_sources: Vec<ShaderSource>,
    /// The constant arenas by root (the .cpp's StereoBindingBank objects).
    arenas: BTreeMap<usize, *mut c_void>,
    timestamps: Option<ID3D12QueryHeap>,
    timing_readback: [Option<ID3D12Resource>; 2],
    readback: Option<ID3D12Resource>,
    dsvs: Option<ID3D12DescriptorHeap>,
    main_fence: Option<ID3D12Fence>,
    done_fence: Option<ID3D12Fence>,
    main_queue: Option<ID3D12CommandQueue>,
    main_list: *mut c_void,
    peer_list: *mut c_void,
}
unsafe extern "system" fn pass_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live pass.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Pass)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn pass_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live pass.
    unsafe { (*(this as *mut Pass)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn pass_release(this: *mut c_void) -> u32 {
    // SAFETY: a live pass, freed at zero.
    unsafe {
        let n = (*(this as *mut Pass)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Pass));
        }
        n
    }
}
static PASS_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: pass_query, AddRef: pass_add_ref, Release: pass_release };

impl Drop for Pass {
    fn drop(&mut self) {
        for i in 0..2 {
            if !self.permit_mappings[i].is_null() {
                if let Some(permit) = &self.snapshot_permits[i] {
                    // SAFETY: the permit mapped since its creation.
                    unsafe { permit.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 8 })) };
                }
            }
        }
        for bank in core::mem::take(&mut self.arenas).into_values() {
            // SAFETY: the .cpp's bank, deleted once.
            bank_free(bank);
        }
        self.shader_sources.clear();
        // SAFETY: the pass's arrays and gate, freed once.
        unsafe {
            for a in self.targets {
                arrays::cvr_target_array_free(a);
            }
            gate::cvr_depth_gate_free(self.gate);
        }
    }
}

fn of<'a>(user: *mut c_void) -> &'a mut Pass {
    // SAFETY: `user` is the pass the call was given (its IUnknown).
    unsafe { &mut *(user as *mut Pass) }
}
fn keep(list: *mut c_void, objects: &[*mut c_void]) -> bool {
    // SAFETY: the caller's list and objects.
    unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
}
fn has(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: the caller's list.
    unsafe { cvr_gpu::command_resources::cvr_gpu_has_command_resource(list, object) }
}

// ---- the host the recording reaches (formerly the Storage's lambdas)

unsafe extern "C-unwind" fn host_keep(_: *mut c_void, list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    if !(1..=4).contains(&count) {
        return false;
    }
    // SAFETY: the recording's `count` objects.
    keep(list, unsafe { core::slice::from_raw_parts(objects, count) })
}
unsafe extern "C-unwind" fn host_has(_: *mut c_void, list: *mut c_void, object: *mut c_void) -> bool {
    has(list, object)
}
unsafe extern "C-unwind" fn host_target(user: *mut c_void, array: u32, op: u32, list: *mut c_void, eye: u32, native: *mut c_void) -> HRESULT {
    // SAFETY: the pass's array (its field, not the state the recording holds).
    let a = unsafe { (*(user as *mut Pass)).targets[array as usize] };
    match op {
        0 => arrays::cvr_target_array_copy_eye(a, list, eye, DEPTH_WRITE, native, DEPTH_WRITE, DEPTH_WRITE),
        1 => arrays::cvr_target_array_import_eye(a, list, eye, DEPTH_WRITE, native, DEPTH_WRITE),
        _ => arrays::cvr_target_array_import_eyes(a, list, eye, DEPTH_WRITE, native, DEPTH_WRITE),
    }
}
unsafe extern "C-unwind" fn host_gate(user: *mut c_void, op: u32, list: *mut c_void, flag: bool) -> bool {
    // SAFETY: the pass's gate (its field).
    let g = unsafe { (*(user as *mut Pass)).gate };
    match op {
        0 => gate::cvr_depth_gate_bind(g, list, flag),
        1 => gate::cvr_depth_gate_decide(g, flag),
        _ => {
            gate::cvr_depth_gate_freeze(g);
            true
        }
    }
}
unsafe extern "C-unwind" fn host_shader_source(user: *mut c_void, index: usize, queue: *mut c_void, side: u32) -> u32 {
    // SAFETY: the pass's sources (their field).
    let sources = unsafe { &(*(user as *mut Pass)).shader_sources };
    let source = &sources[index];
    if let Some(texture) = &source.texture {
        // SAFETY: the lease the pass holds.
        if !unsafe { (reads().texture_valid)(&texture.0, queue) } {
            return if !texture.r8() { 2 } else { 1 };
        }
    }
    if let Some(material) = &source.material {
        // SAFETY: as above.
        if !unsafe { (reads().material_valid)(material.0, &source.material_proof[side as usize], queue) } {
            return 3;
        }
    }
    0
}
unsafe extern "C-unwind" fn track_cb(user: *mut c_void, list: *mut c_void, side: u32) -> bool {
    // SAFETY: the pass (its state read through the same pointer the recording was given).
    unsafe { track(user as *mut Pass, list, side) }
}
/// Track: the side's recording token kept with `list` (once), unless the side's recording retired.
///
/// # Safety
/// `p` is a live pass.
unsafe fn track(p: *mut Pass, list: *mut c_void, side: u32) -> bool {
    let i = side as usize;
    // SAFETY: as documented.
    unsafe {
        if (*p).s.recording_retired[i] {
            return false;
        }
        if let Some(token) = &(*p).tokens[i] {
            return has(list, token.0.cast());
        }
        let token = TokenRef(Box::into_raw(Box::new(Token { vtbl: &TOKEN_VTBL, refs: AtomicU32::new(1) })));
        if !keep(list, &[p.cast(), token.0.cast()]) {
            return false;
        }
        (*p).tokens[i] = Some(token);
    }
    true
}
unsafe extern "C-unwind" fn no_draw(_: *mut c_void, _: *mut c_void, _: *const DrawRecord) -> bool {
    false
}
unsafe extern "C-unwind" fn no_capture(_: *mut c_void, _: *mut c_void) -> bool {
    false
}
unsafe extern "C-unwind" fn no_bind(_: *mut c_void, _: *mut c_void, _: u32) -> bool {
    false
}
unsafe extern "C-unwind" fn no_restore(_: *mut c_void, _: *mut c_void) {}
unsafe extern "C-unwind" fn keep_pair(_: *mut c_void, list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    // SAFETY: the capture's two objects.
    count == 2 && keep(list, unsafe { core::slice::from_raw_parts(objects, 2) })
}
unsafe extern "C-unwind" fn gate_read(user: *mut c_void, value: *mut u64) -> HRESULT {
    // SAFETY: the pass's gate.
    unsafe { gate::cvr_depth_gate_read((*(user as *mut Pass)).gate, value) }
}
unsafe extern "C-unwind" fn gate_readback(user: *mut c_void, list: *mut c_void) -> bool {
    // SAFETY: the pass's gate.
    gate::cvr_depth_gate_readback(unsafe { (*(user as *mut Pass)).gate }, list)
}

impl Pass {
    fn user(&mut self) -> *mut c_void {
        (self as *mut Pass).cast()
    }
    fn host(&mut self) -> Host {
        Host {
            user: self.user(),
            keep: host_keep,
            has: host_has,
            target: host_target,
            gate: host_gate,
            shader_source: host_shader_source,
            shader_sources: self.shader_sources.len(),
        }
    }
    fn objects(&self) -> Objects {
        Objects {
            main_list: self.main_list,
            peer_list: self.peer_list,
            native: [raw(&self.native[0]), raw(&self.native[1])],
            dsvs: raw(&self.dsvs),
            timestamps: raw(&self.timestamps),
            timing: [raw(&self.timing_readback[0]), raw(&self.timing_readback[1])],
            readback: raw(&self.readback),
            instance_source: raw(&self.instance_source),
            tokens: [
                self.tokens[0].as_ref().map_or(core::ptr::null_mut(), |t| t.0.cast()),
                self.tokens[1].as_ref().map_or(core::ptr::null_mut(), |t| t.0.cast()),
            ],
            permits: self.permit_mappings,
            main_queue: raw(&self.main_queue),
            main_fence: raw(&self.main_fence),
        }
    }
    fn shader_objects(&self) -> Shaders {
        Shaders {
            snapshots: [raw(&self.shader_snapshots[0]), raw(&self.shader_snapshots[1])],
            permits: [raw(&self.snapshot_permits[0]), raw(&self.snapshot_permits[1])],
            mappings: self.permit_mappings,
        }
    }
    fn adopt_shader_objects(&mut self, o: &Shaders) {
        for i in 0..2 {
            if self.shader_snapshots[i].is_none() && !o.snapshots[i].is_null() {
                self.shader_snapshots[i] = adopt(o.snapshots[i]);
            }
            if self.snapshot_permits[i].is_none() && !o.permits[i].is_null() {
                self.snapshot_permits[i] = adopt(o.permits[i]);
                self.permit_mappings[i] = o.mappings[i];
            }
        }
    }
    fn dsv(&self, i: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: the pass's live DSV heap.
        let base = self.dsvs.as_ref().map_or(0, |h| unsafe { h.GetCPUDescriptorHandleForHeapStart() }.ptr);
        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base + i as usize * self.s.stride as usize }
    }
    fn commit_native(&mut self, allow: bool) -> bool {
        let host = self.host();
        edp::commit_native(&mut self.s, allow, &host)
    }
    fn reject_shader_snapshots(&mut self) {
        self.s.shader_ready = false;
        self.commit_native(false);
    }
}

/// The native draw's state around the pass's own recording (formerly the .cpp's Restore): the game's bindings, the
/// view instance mask, the shading rate, the depth target, viewport, scissor and pipeline, put back when it drops.
struct Restore {
    list: ID3D12GraphicsCommandList,
    draw: *const DrawRecord,
    bindings: Bindings,
    views: Option<ID3D12GraphicsCommandList1>,
    rates: Option<ID3D12GraphicsCommandList5>,
    valid: bool,
    restore_depth: bool,
    restore_bindings: bool,
}
impl Restore {
    fn new(list: &ID3D12GraphicsCommandList, draw: &DrawRecord, depth: bool, restore_state: bool) -> Self {
        let mut bindings = Bindings::default();
        // SAFETY: the list's recorded bindings into ours.
        let snapped = unsafe { cvr_scene_snapshot(list.as_raw() as usize, &mut bindings) };
        let (views, rates) = (list.cast::<ID3D12GraphicsCommandList1>().ok(), list.cast::<ID3D12GraphicsCommandList5>().ok());
        let valid = snapped && bindings.rate.known && views.is_some() && rates.is_some();
        Restore { list: list.clone(), draw, bindings, views, rates, valid, restore_depth: depth, restore_bindings: restore_state }
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        if !self.valid {
            return;
        }
        // SAFETY: the draw outlives the restore (both the call's); the list records the game's state back.
        unsafe {
            let d = &*self.draw;
            if self.restore_bindings {
                cvr_scene_bindings_restore(&self.bindings, self.list.as_raw());
            }
            if let Some(views) = &self.views {
                views.SetViewInstanceMask(u32::MAX);
            }
            if let Some(rates) = &self.rates {
                let image = self.bindings.rate.image as *mut c_void;
                rates.RSSetShadingRateImage(ID3D12Resource::from_raw_borrowed(&image));
                let combiners = [D3D12_SHADING_RATE_COMBINER(self.bindings.rate.combiners[0]), D3D12_SHADING_RATE_COMBINER(self.bindings.rate.combiners[1])];
                rates.RSSetShadingRate(D3D12_SHADING_RATE(self.bindings.rate.value), Some(combiners.as_ptr()));
            }
            if self.restore_depth {
                let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: d.depth as usize };
                self.list.OMSetRenderTargets(0, None, false, Some(&depth));
            }
            self.list.RSSetViewports(&[d.viewport]);
            self.list.RSSetScissorRects(&[d.scissor]);
            let pipeline = d.pipeline as *mut c_void;
            self.list.SetPipelineState(ID3D12PipelineState::from_raw_borrowed(&pipeline));
        }
    }
}

// ---- the C ABI (include/Render/EarlyDepthPass.hpp's face in the Rust build; every pointer the pass's or live)

fn pass<'a>(p: *mut c_void) -> &'a mut Pass {
    of(p)
}
fn lists<'a>(lists: *const *mut c_void, count: u32) -> &'a [*mut c_void] {
    // SAFETY: the caller's submission.
    if lists.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(lists, count as usize) } }
}

/// Create: a new pass (its reference out) or the failure.
///
/// # Safety
/// `device` is live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_create(device: *mut c_void, width: u32, height: u32, mode: u32, out: *mut *mut c_void) -> HRESULT {
    let Some(device_object) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    if width == 0 || height == 0 || width > 4096 || height > 4096 || mode > 3 {
        return E_INVALIDARG;
    }
    let mut s = State::default();
    s.result.width = width;
    s.result.height = height;
    s.native_counts[0] = mode;
    s.optimized[0] = (mode == 2) as u64;
    s.optimized[1] = u64::MAX;
    let p = Box::into_raw(Box::new(Pass {
        vtbl: &PASS_VTBL,
        refs: AtomicU32::new(1),
        s,
        device: Some(device_object.clone()),
        tokens: [None, None],
        targets: [arrays::cvr_target_array_new(), arrays::cvr_target_array_new()],
        native: [None, None],
        gate: gate::cvr_depth_gate_new(),
        instance_snapshots: [None, None],
        instance_source: None,
        instance_ranges: Vec::new(),
        shader_snapshots: [None, None],
        snapshot_permits: [None, None],
        permit_mappings: [core::ptr::null_mut(); 2],
        shader_sources: Vec::new(),
        arenas: BTreeMap::new(),
        timestamps: None,
        timing_readback: [None, None],
        readback: None,
        dsvs: None,
        main_fence: None,
        done_fence: None,
        main_queue: None,
        main_list: core::ptr::null_mut(),
        peer_list: core::ptr::null_mut(),
    }));
    unsafe extern "C-unwind" fn init_targets(user: *mut c_void, desc: *const D3D12_RESOURCE_DESC, clear: *const D3D12_CLEAR_VALUE, out: *mut [*mut c_void; 2]) -> HRESULT {
        // SAFETY: the pass being created and the native target's description.
        unsafe {
            let p = user as *mut Pass;
            let device = raw(&(*p).device);
            for a in (*p).targets {
                let hr = arrays::cvr_target_array_initialize(a, device, desc, DEPTH_WRITE, clear);
                if hr.is_err() {
                    return hr;
                }
            }
            *out = [arrays::cvr_target_array_resource((*p).targets[0]), arrays::cvr_target_array_resource((*p).targets[1])];
        }
        S_OK
    }
    unsafe extern "C-unwind" fn create_gate(user: *mut c_void, initial: *mut c_void) -> HRESULT {
        // SAFETY: the pass being created.
        unsafe {
            let p = user as *mut Pass;
            gate::cvr_depth_gate_create((*p).gate, raw(&(*p).device), initial)
        }
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the new pass; the callbacks reach its arrays and gate.
    let created = unsafe { edp::create(&device_object, width, height, mode, &mut (*p).s, init_targets, create_gate, p.cast()) };
    match created {
        Ok(c) => {
            // SAFETY: the new pass, its objects with their creations' references.
            unsafe {
                let q = &mut *p;
                q.timestamps = adopt(c.timestamps);
                q.timing_readback = [adopt(c.timing[0]), adopt(c.timing[1])];
                q.dsvs = adopt(c.dsvs);
                q.readback = adopt(c.readback);
                q.main_fence = adopt(c.main_fence);
                q.done_fence = adopt(c.done_fence);
                *out = p.cast();
            }
            S_OK
        }
        Err(hr) => {
            // SAFETY: the creation's reference.
            unsafe { pass_release(p.cast()) };
            hr
        }
    }
}
/// # Safety
/// `p` is a pass; the list and source live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_capture_instances(p: *mut c_void, list: *mut c_void, source: *mut c_void, offset: u64, bytes: u32, state: i32, combined: bool) -> bool {
    let s = pass(p);
    s.instance_ranges.reserve(MAX_DRAWS);
    let _internal = cvr_host::probe::Internal::new();
    let mut objects = Instances {
        snapshots: [raw(&s.instance_snapshots[0]), raw(&s.instance_snapshots[1])],
        source: raw(&s.instance_source),
        range_count: s.instance_ranges.len(),
    };
    let created = objects.snapshots[0].is_null();
    let Some(device) = s.device.clone() else { return false };
    let user = s.user();
    // SAFETY: as documented.
    let captured = unsafe { edp::capture_instances(&mut s.s, &device, list, s.main_list, source, offset, bytes, state, combined, &mut objects, keep_pair, user, core::ptr::null_mut()) };
    if created && !objects.snapshots[0].is_null() {
        s.instance_snapshots = [adopt(objects.snapshots[0]), adopt(objects.snapshots[1])];
    }
    if !captured {
        return false;
    }
    s.instance_source = borrowed(objects.source);
    s.instance_ranges.push(Range { from: offset, bytes });
    true
}
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_release_instance_read(p: *mut c_void, list: *mut c_void) {
    let s = pass(p);
    let _internal = cvr_host::probe::Internal::new();
    let source = raw(&s.instance_source);
    // SAFETY: as documented.
    unsafe { edp::release_instance_read(&mut s.s, list, s.main_list, source) };
}
#[no_mangle]
pub extern "C" fn cvr_edo_instance_source(p: *mut c_void) -> *mut c_void {
    raw(&pass(p).instance_source)
}
/// The pass's counters: 0 the instance proofs (2), 1 the alpha proofs (6), 2 the shader proofs (4), 3 the optimized
/// counts (8), 4 the native differences (4), 5 the GPU times (6).
///
/// # Safety
/// `out` has room for the kind's count.
#[no_mangle]
pub unsafe extern "C" fn cvr_edo_counts(p: *mut c_void, which: u32, out: *mut u64) {
    let q = pass(p);
    let ranges = q.instance_ranges.len() as u64;
    let s = &q.s;
    let values: &[u64] = match which {
        0 => &[ranges, s.instance_bytes as u64],
        1 => &s.alpha_stats,
        2 => &s.shader_stats,
        3 => &s.optimized,
        4 => &s.native_differences,
        _ => &s.gpu_times,
    };
    // SAFETY: as documented.
    unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), out, values.len()) };
}
/// # Safety
/// `out` has room for 8.
#[no_mangle]
pub unsafe extern "C" fn cvr_edo_native_counts(p: *mut c_void, out: *mut u32) {
    // SAFETY: as documented.
    unsafe { core::ptr::copy_nonoverlapping(pass(p).s.native_counts.as_ptr(), out, 8) };
}
/// ShaderSource: whether `resource` (any, when null) is a shader source's.
#[no_mangle]
pub extern "C" fn cvr_edo_shader_source(p: *mut c_void, resource: *mut c_void) -> bool {
    pass(p).shader_sources.iter().any(|source| {
        resource.is_null() || raw(&source.buffer) == resource || source.texture_resource() == resource || source.material_resource() == resource
    })
}
#[no_mangle]
pub extern "C" fn cvr_edo_reject_shader_snapshots(p: *mut c_void) {
    pass(p).reject_shader_snapshots();
}
/// CaptureSkinning.
///
/// # Safety
/// The list, bones and lease are live; `ranges` holds `count` (offset, bytes) pairs.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_capture_skinning(p: *mut c_void, list: *mut c_void, bones: *mut c_void, ranges: *const [u64; 2], count: usize, texture: *const TextureLease, sequence: u32) -> bool {
    let s = pass(p);
    s.s.skin_pending_sequence = 0;
    // SAFETY: as documented.
    let (ranges, texture) = unsafe { (if count == 0 { &[][..] } else { core::slice::from_raw_parts(ranges, count) }, &*texture) };
    let Some(device) = s.device.clone() else { return false };
    let mut planned = vec![Planned::default(); ranges.len() + 1];
    let mut plan = Plan { sources: planned.as_mut_ptr(), capacity: planned.len(), count: 0, end: 0, buffer_bytes: 0, texture_bytes: 0 };
    // SAFETY: as documented.
    if !unsafe { edp::skinning_plan(&s.s, &device, list, s.main_list, bones, ranges, texture.resource, s.shader_sources.len(), &mut plan) } {
        return false;
    }
    let mut pending: Vec<ShaderSource> = Vec::with_capacity(plan.count);
    for planned in &planned[..plan.count] {
        let mut source = ShaderSource { offset: planned.offset, bytes: planned.bytes, ..ShaderSource::default() };
        if planned.kind == 0 {
            source.buffer = borrowed(bones);
            source.source_offset = planned.source_offset;
        } else {
            source.texture = Some(Texture::of(texture));
            source.layout = planned.layout;
            source.row_bytes = planned.row_bytes;
            source.rows = planned.rows;
        }
        pending.push(source);
    }
    s.shader_sources.reserve((MAX_BUFFER_PROOFS - 1).saturating_sub(s.shader_sources.len()));
    let _internal = cvr_host::probe::Internal::new();
    let mut objects = s.shader_objects();
    let mut constant = core::ptr::null_mut();
    let keep_objects = [bones, texture.resource];
    let host = s.host();
    unsafe extern "C-unwind" fn copy(user: *mut c_void, index: usize, list: *mut c_void, snapshot: *mut c_void) -> u32 {
        // SAFETY: the capture's pending sources.
        let sources = unsafe { &*(user as *mut Vec<ShaderSource>) };
        let source = &sources[index];
        let Some(texture) = &source.texture else { return 1 };
        // SAFETY: the lease the source holds.
        if unsafe { (reads().texture_copy)(list, &texture.0, WHOLE, snapshot, &source.layout) } { 0 } else { 1 }
    }
    let pending_user = (&mut pending as *mut Vec<ShaderSource>).cast();
    // SAFETY: as documented.
    let failure = unsafe { edp::shader_record(&mut s.s, &device, list, &mut objects, &plan, None, &mut constant, &keep_objects, &host, copy, pending_user, bones) };
    s.adopt_shader_objects(&objects);
    if failure != 0 {
        return false;
    }
    s.shader_sources.extend(pending);
    s.s.shader_bytes = plan.end as u32;
    s.s.skin_pending_sequence = sequence;
    s.s.shader_stats[0] += 1;
    s.s.shader_stats[2] += plan.buffer_bytes;
    s.s.shader_stats[3] += plan.texture_bytes;
    true
}
/// CaptureAlpha.
///
/// # Safety
/// The list and leases are live; `mips` holds `count` mips.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edo_capture_alpha(
    p: *mut c_void,
    list: *mut c_void,
    material: *const c_void,
    texture: *const TextureLease,
    mips: *const u32,
    count: usize,
    expected_index: u32,
    sequence: u32,
) -> bool {
    let s = pass(p);
    fn reject(s: &mut Pass, reason: u64) -> bool {
        s.s.skin_pending_sequence = 0;
        s.s.alpha_stats[5] = reason;
        false
    }
    // SAFETY: as documented.
    let (mips, texture) = unsafe { (if count == 0 { &[][..] } else { core::slice::from_raw_parts(mips, count) }, &*texture) };
    let Some(device) = s.device.clone() else { return false };
    // SAFETY: the caller's lease.
    let material_resource = unsafe { (reads().material_resource)(material) };
    let mut planned = vec![Planned::default(); mips.len() + 1];
    let mut plan = Plan { sources: planned.as_mut_ptr(), capacity: planned.len(), count: 0, end: 0, buffer_bytes: 0, texture_bytes: 0 };
    // SAFETY: as documented.
    let refused = unsafe { edp::alpha_plan(&s.s, &device, list, s.main_list, !material_resource.is_null(), texture.resource, mips, expected_index, sequence, s.shader_sources.len(), &mut plan) };
    if refused != 0 {
        return reject(s, refused);
    }
    let mut pending: Vec<ShaderSource> = Vec::with_capacity(plan.count);
    for planned in &planned[..plan.count] {
        let mut source = ShaderSource { offset: planned.offset, bytes: planned.bytes, ..ShaderSource::default() };
        if planned.kind == 1 {
            source.material = Some(Material::of(material));
        } else {
            source.texture = Some(Texture::of(texture));
            source.mip = planned.mip;
            source.layout = planned.layout;
            source.row_bytes = planned.row_bytes;
            source.rows = planned.rows;
        }
        pending.push(source);
    }
    s.shader_sources.reserve((MAX_BUFFER_PROOFS - 1).saturating_sub(s.shader_sources.len()));
    let _internal = cvr_host::probe::Internal::new();
    /// The copies' sources and the read failure that fails the pass (40, applied after the recording).
    struct Copies {
        pending: Vec<ShaderSource>,
        failed: bool,
    }
    let mut copies = Copies { pending, failed: false };
    unsafe extern "C-unwind" fn copy(user: *mut c_void, index: usize, list: *mut c_void, snapshot: *mut c_void) -> u32 {
        // SAFETY: the capture's copies.
        let c = unsafe { &mut *(user as *mut Copies) };
        let source = &mut c.pending[index];
        let copied = match (&source.material, &source.texture) {
            // SAFETY: the leases the source holds.
            (Some(material), _) => unsafe { (reads().material_copy)(list, material.0, snapshot, source.offset as u64, &mut source.material_proof[0]) },
            (None, Some(texture)) => unsafe { (reads().texture_copy)(list, &texture.0, source.mip, snapshot, &source.layout) },
            (None, None) => false,
        };
        if copied {
            return 0;
        }
        let material = source.material.is_some();
        // SAFETY: the reads' failure codes.
        let failure = unsafe { if material { (reads().material_failure)() } else { (reads().texture_failure)() } };
        c.failed |= failure == 40;
        (if material { 1000 } else { 2000 }) + failure
    }
    let mut objects = s.shader_objects();
    let mut constant = core::ptr::null_mut();
    let keep_objects = [material_resource, texture.resource];
    let host = s.host();
    let copies_user = (&mut copies as *mut Copies).cast();
    // SAFETY: as documented.
    let failure = unsafe { edp::shader_record(&mut s.s, &device, list, &mut objects, &plan, Some(expected_index), &mut constant, &keep_objects, &host, copy, copies_user, core::ptr::null_mut()) };
    s.s.failed |= copies.failed;
    s.adopt_shader_objects(&objects);
    if !constant.is_null() {
        if let Some(first) = copies.pending.first_mut() {
            first.expected_index = adopt(constant);
        } else {
            drop(adopt::<ID3D12Resource>(constant));
        }
    }
    if failure >= 1000 {
        s.reject_shader_snapshots();
        return reject(s, failure);
    }
    if failure != 0 {
        return reject(s, failure);
    }
    s.shader_sources.extend(copies.pending);
    s.s.shader_bytes = plan.end as u32;
    s.s.alpha_captures += 1;
    s.s.alpha_pending_sequence = sequence;
    s.s.alpha_stats[0] += 1;
    s.s.alpha_stats[3] += MATERIAL_BYTES as u64;
    s.s.alpha_stats[4] += plan.texture_bytes;
    s.s.shader_stats[0] += 1;
    s.s.shader_stats[2] += MATERIAL_BYTES as u64;
    s.s.shader_stats[3] += plan.texture_bytes;
    true
}

/// StartNative's gate for the peer: the instance and shader proofs, the shader sources' peer copies (predicated on the
/// peer's permit), Prepare and Bind.
unsafe extern "C-unwind" fn peer_gate(user: *mut c_void, list: *mut c_void, draw: *const DrawRecord) -> bool {
    let p = user as *mut Pass;
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    // SAFETY: the pass (the state's fields through the pointer the recording was given) and the native draw.
    unsafe {
        let d = &*draw;
        let sources = &mut (*p).shader_sources;
        let snapshots = [raw(&(*p).shader_snapshots[0]), raw(&(*p).shader_snapshots[1])];
        let mut proofs: Vec<BufferProof> = Vec::with_capacity(1 + sources.len() + (*p).s.alpha_captures as usize);
        if (*p).s.instance_bytes != 0 {
            proofs.push(BufferProof {
                main: raw(&(*p).instance_snapshots[0]),
                peer: raw(&(*p).instance_snapshots[1]),
                bytes: (*p).s.instance_bytes,
                row_bytes: 0,
                row_pitch: 0,
                rows: 0,
                main_offset: 0,
                peer_offset: 0,
            });
        }
        if !sources.is_empty() {
            let permit = (*p).snapshot_permits[1].clone();
            recording.SetPredication(permit.as_ref(), 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            let end_predicate = || recording.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            let peer = (*p).shader_snapshots[1].clone();
            for source in sources.iter_mut() {
                if let Some(material) = &source.material {
                    if !(reads().material_copy)(list, material.0, snapshots[1], source.offset as u64, &mut source.material_proof[1]) {
                        let failure = (reads().material_failure)();
                        (*p).s.alpha_stats[5] = 1000 + failure as u64;
                        (*p).s.shader_ready = false;
                        if failure == 40 {
                            end_predicate();
                            return false;
                        }
                        break;
                    }
                    (*p).s.alpha_stats[1] += 1;
                } else if let Some(buffer) = &source.buffer {
                    if let Some(peer) = &peer {
                        recording.CopyBufferRegion(peer, source.offset as u64, buffer, source.source_offset, source.bytes as u64);
                    }
                } else if let Some(texture) = &source.texture {
                    let r8 = texture.r8();
                    let mip = if r8 { WHOLE } else { source.mip };
                    if !(reads().texture_copy)(list, &texture.0, mip, snapshots[1], &source.layout) {
                        let failure = (reads().texture_failure)();
                        if !r8 {
                            (*p).s.alpha_stats[5] = 2000 + failure as u64;
                        }
                        (*p).s.shader_ready = false;
                        if failure == 40 {
                            end_predicate();
                            return false;
                        }
                        break;
                    }
                }
                if source.texture.as_ref().is_some_and(|t| !t.r8()) {
                    (*p).s.alpha_stats[2] += 1;
                }
                (*p).s.shader_stats[1] += 1;
            }
            end_predicate();
        }
        if !sources.is_empty() && (*p).s.shader_ready {
            let barrier = |resource: *mut c_void| D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                        StateAfter: D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                    }),
                },
            };
            recording.ResourceBarrier(&[barrier(snapshots[0]), barrier(snapshots[1])]);
            (*p).s.shaders_readable = true;
            for source in sources.iter() {
                let mut proof = BufferProof {
                    main: snapshots[0],
                    peer: snapshots[1],
                    bytes: source.bytes,
                    row_bytes: 0,
                    row_pitch: 0,
                    rows: 0,
                    main_offset: source.offset,
                    peer_offset: source.offset,
                };
                if source.texture.is_some() {
                    proof.row_bytes = source.row_bytes;
                    proof.row_pitch = source.layout.Footprint.RowPitch;
                    proof.rows = source.rows;
                }
                proofs.push(proof);
                if let Some(index) = &source.expected_index {
                    proofs.push(BufferProof {
                        main: snapshots[0],
                        peer: index.as_raw(),
                        bytes: 4,
                        row_bytes: 0,
                        row_pitch: 0,
                        rows: 0,
                        main_offset: source.offset,
                        peer_offset: 0,
                    });
                }
            }
        }
        let g = (*p).gate;
        gate::cvr_depth_gate_prepare(g, list, d.flags, d.pipeline as *mut c_void, proofs.as_ptr(), proofs.len()) && gate::cvr_depth_gate_bind(g, list, true)
    }
}
/// StartNative.
///
/// # Safety
/// The list, draw, target and instance source are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_start_native(p: *mut c_void, list: *mut c_void, draw: *const DrawRecord, target: *mut c_void, instance_source: *mut c_void, instance_state: i32) -> bool {
    let s = pass(p);
    let _internal = cvr_host::probe::Internal::new();
    let user = s.user();
    let draws = DrawHost { user, track: track_cb, peer_gate, capture: no_capture, bind: no_bind, restore: no_restore };
    let objects = s.objects();
    let host = s.host();
    let snapshots = [raw(&s.instance_snapshots[0]), raw(&s.instance_snapshots[1])];
    let device = raw(&s.device);
    // SAFETY: as documented.
    let d = unsafe { &*draw };
    // SAFETY: as documented; the callbacks reach the pass through `user`.
    let outcome = unsafe { edp::start_native(&mut s.s, &objects, device, list, d, target, instance_source, instance_state, &s.instance_ranges, snapshots, &host, &draws) };
    if outcome == edp::NATIVE_TAKEN {
        s.native[d.side as usize & 1] = borrowed(target);
        if d.side != 0 {
            s.peer_list = list;
        } else {
            s.main_list = list;
        }
    }
    outcome != edp::NATIVE_REFUSED
}
/// # Safety
/// The list and draw are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_observe_native(p: *mut c_void, list: *mut c_void, draw: *const DrawRecord) -> bool {
    let s = pass(p);
    let objects = s.objects();
    // SAFETY: as documented.
    edp::observe_native(&mut s.s, &objects, list, unsafe { &*draw })
}
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_original(p: *mut c_void, list: *mut c_void, draw: *const DrawRecord) -> bool {
    let s = pass(p);
    let _internal = cvr_host::probe::Internal::new();
    let objects = s.objects();
    let host = s.host();
    // SAFETY: as documented.
    unsafe { edp::original(&mut s.s, &objects, list, &*draw, &host) }
}
/// # Safety
/// As above; `runs` holds `count` runs.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_peer_remainder(p: *mut c_void, list: *mut c_void, draw: *const DrawRecord, runs: *const Run, count: usize) -> bool {
    let s = pass(p);
    let _internal = cvr_host::probe::Internal::new();
    let objects = s.objects();
    let host = s.host();
    // SAFETY: as documented.
    let runs = if count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(runs, count) } };
    // SAFETY: as documented.
    unsafe { edp::peer_remainder(&mut s.s, &objects, list, &*draw, runs, &host) }
}
/// # Safety
/// The list is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_finish_native(p: *mut c_void, list: *mut c_void, side: u32) -> bool {
    let s = pass(p);
    let _internal = cvr_host::probe::Internal::new();
    let objects = s.objects();
    let host = s.host();
    // SAFETY: as documented.
    unsafe { edp::finish_native(&mut s.s, &objects, list, side, &host) }
}
#[no_mangle]
pub extern "C" fn cvr_edo_commit_native(p: *mut c_void, allow: bool) -> bool {
    pass(p).commit_native(allow)
}
/// AbortNative: mode 2's decision refused, then FinishNative.
///
/// # Safety
/// The list is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_abort_native(p: *mut c_void, list: *mut c_void, side: u32) {
    let s = pass(p);
    if s.s.native_counts[0] == 2 {
        gate::cvr_depth_gate_decide(s.gate, false);
        s.s.optimized[7] = 0;
    }
    // SAFETY: as documented.
    unsafe { cvr_edo_finish_native(p, list, side) };
}
/// Main's bank and its restore (the .cpp's StereoBindingBank).
#[repr(C)]
pub struct Bank {
    pub user: *mut c_void,
    /// CapturedFor(list) || (CaptureEye(list, 0) && CaptureEye(list, 1)).
    pub capture: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> bool,
    /// Bind(list, index).
    pub bind: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32) -> bool,
}
struct MainUser {
    bank: *const Bank,
    bindings: *const Bindings,
}
unsafe extern "C-unwind" fn main_capture(user: *mut c_void, list: *mut c_void) -> bool {
    // SAFETY: Main's user and the .cpp's bank.
    unsafe {
        let bank = &*(*(user as *mut MainUser)).bank;
        (bank.capture)(bank.user, list)
    }
}
unsafe extern "C-unwind" fn main_bind(user: *mut c_void, list: *mut c_void, index: u32) -> bool {
    // SAFETY: as above.
    unsafe {
        let bank = &*(*(user as *mut MainUser)).bank;
        (bank.bind)(bank.user, list, index)
    }
}
unsafe extern "C-unwind" fn main_restore(user: *mut c_void, list: *mut c_void) {
    // SAFETY: Main's restore's bindings, recorded back on the list.
    unsafe { cvr_scene_bindings_restore((*(user as *mut MainUser)).bindings, list) };
}
/// Main: the admission, the game's state saved around the pass's recording, the recording token, the native camera's
/// pipeline and root kept, the recording.
///
/// # Safety
/// The list, draw and pipelines are live; the masks hold their counts; `bank` is the .cpp's.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edo_main(
    p: *mut c_void,
    list: *mut c_void,
    draw: *const DrawRecord,
    admission: *const Admission,
    masks: *const u8,
    mask_count: usize,
    peer_only: *const u8,
    peer_only_count: usize,
    bank_draw: u32,
    native_camera: bool,
    alpha_optimized: bool,
    pipeline: *mut c_void,
    camera_pipeline: *mut c_void,
    source_root: *mut c_void,
    bank: *const Bank,
) -> bool {
    let s = pass(p);
    // SAFETY: as documented.
    let (d, a) = unsafe { (&*draw, &*admission) };
    let native0 = s.native[0].is_some();
    if !edp::main_admitted(&s.s, s.main_list, native0, list, d, a) {
        return false;
    }
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    let _internal = cvr_host::probe::Internal::new();
    let restore = Restore::new(&recording, d, s.s.native_counts[0] != 2, !native_camera);
    // SAFETY: the pass.
    if !restore.valid || !unsafe { track(s, list, 0) } {
        return false;
    }
    if native_camera && !keep(list, &[camera_pipeline, source_root]) {
        return false;
    }
    let mut user = MainUser { bank, bindings: &restore.bindings };
    let draws = DrawHost { user: (&mut user as *mut MainUser).cast(), track: no_track, peer_gate: no_draw, capture: main_capture, bind: main_bind, restore: main_restore };
    let objects = s.objects();
    let host = s.host();
    let Some(device) = s.device.clone() else { return false };
    // SAFETY: as documented.
    let (masks, peer_only) = unsafe {
        (
            if mask_count == 0 { &[][..] } else { core::slice::from_raw_parts(masks, mask_count) },
            if peer_only_count == 0 { &[][..] } else { core::slice::from_raw_parts(peer_only, peer_only_count) },
        )
    };
    // SAFETY: as documented.
    let outcome = unsafe { edp::main_record(&mut s.s, &objects, &device, list, d, masks, peer_only, bank_draw, native_camera, alpha_optimized, pipeline, &host, &draws) };
    if outcome & 2 != 0 {
        s.main_list = list;
    }
    drop(restore);
    outcome & 1 != 0
}
unsafe extern "C-unwind" fn no_track(_: *mut c_void, _: *mut c_void, _: u32) -> bool {
    false
}
/// MainConstants' opening: mode 2, the main side not submitted nor finished.
#[no_mangle]
pub extern "C" fn cvr_edo_constants_open(p: *mut c_void) -> bool {
    let s = &pass(p).s;
    s.native_counts[0] == 2 && !s.main_submitted && !s.native_finished[0]
}
#[no_mangle]
pub extern "C" fn cvr_edo_device(p: *mut c_void) -> *mut c_void {
    raw(&pass(p).device)
}
/// The arena (the .cpp's bank) for `root`, or null.
#[no_mangle]
pub extern "C" fn cvr_edo_arena(p: *mut c_void, root: *mut c_void) -> *mut c_void {
    pass(p).arenas.get(&(root as usize)).copied().unwrap_or(core::ptr::null_mut())
}
#[no_mangle]
pub extern "C" fn cvr_edo_arena_count(p: *mut c_void) -> u32 {
    pass(p).arenas.len() as u32
}
/// The pass takes the .cpp's new arena for `root` (deleting it with itself).
#[no_mangle]
pub extern "C" fn cvr_edo_adopt_arena(p: *mut c_void, root: *mut c_void, bank: *mut c_void) {
    if let Some(old) = pass(p).arenas.insert(root as usize, bank) {
        // SAFETY: a replaced bank (never: the .cpp adopts only for a root without one).
        bank_free(old);
    }
}
/// Peer: the peer's native draw into the pass's peer DSV.
///
/// # Safety
/// The list and draw are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_peer(p: *mut c_void, list: *mut c_void, draw: *const DrawRecord) -> bool {
    let s = pass(p);
    // SAFETY: as documented.
    let d = unsafe { &*draw };
    if list.is_null()
        || s.s.native_counts[0] > 2
        || d.side != 1
        || d.rt_count != 0
        || d.depth == 0
        || s.s.failed
        || !s.s.main_submitted
        || s.s.copied
        || (!s.peer_list.is_null() && s.peer_list != list)
    {
        return false;
    }
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    let _internal = cvr_host::probe::Internal::new();
    let restore = Restore::new(&recording, d, true, true);
    // SAFETY: the pass.
    if !restore.valid || !unsafe { track(s, list, 1) } {
        return false;
    }
    s.peer_list = list;
    let target = s.dsv(1);
    // SAFETY: the list recording the native draw into the pass's DSV.
    unsafe {
        recording.OMSetRenderTargets(0, None, false, Some(&target));
        recording.DrawIndexedInstanced(d.indices, d.instances, d.first_index, d.base_vertex, d.first_instance);
    }
    drop(restore);
    true
}
/// Readback.
///
/// # Safety
/// The list is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_readback(p: *mut c_void, list: *mut c_void) -> bool {
    let s = pass(p);
    if list.is_null() || s.s.failed || !s.s.initialized || !s.s.main_submitted || (!s.peer_list.is_null() && s.peer_list != list) || s.s.copied {
        return false;
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the pass.
    if !unsafe { track(s, list, 1) } {
        return false;
    }
    s.peer_list = list;
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    let targets = [arrays::cvr_target_array_resource(s.targets[0]), arrays::cvr_target_array_resource(s.targets[1])];
    let (readback, timestamps, timing) = (raw(&s.readback), raw(&s.timestamps), raw(&s.timing_readback[1]));
    let user = s.user();
    // SAFETY: as documented.
    unsafe { edp::readback(&mut s.s, &recording, targets, readback, timestamps, timing, gate_readback, user) }
}
/// Read.
///
/// # Safety
/// `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_read(p: *mut c_void, result: *mut Comparison) -> HRESULT {
    let s = pass(p);
    let objects = ReadObjects {
        done_fence: raw(&s.done_fence),
        main_fence: raw(&s.main_fence),
        timestamps: raw(&s.timestamps),
        timing: [raw(&s.timing_readback[0]), raw(&s.timing_readback[1])],
        readback: raw(&s.readback),
    };
    let user = s.user();
    // SAFETY: as documented.
    unsafe { edp::read(&mut s.s, &objects, gate_read, user, &mut *result) }
}
/// Restart (mode 2): the pass's work read and done, the gate and arenas restarted, the recording's objects dropped.
#[no_mangle]
pub extern "C-unwind" fn cvr_edo_restart(p: *mut c_void) -> HRESULT {
    let s = pass(p);
    if !edp::restart_ready(&s.s) {
        return E_UNEXPECTED;
    }
    let mut done = Comparison::default();
    // SAFETY: the pass's read into a local result.
    let hr = unsafe { cvr_edo_read(p, &mut done) };
    if hr != S_OK {
        return hr;
    }
    if done.complete == 0 || !gate::cvr_depth_gate_restart(s.gate) {
        return E_FAIL;
    }
    for bank in s.arenas.values() {
        // SAFETY: the .cpp's banks.
        if !unsafe { crate::binding_bank::cvr_bank_restart_arena(*bank) } {
            return E_FAIL;
        }
    }
    s.instance_ranges.clear();
    s.instance_source = None;
    s.shader_sources.clear();
    // SAFETY: the pass's permits.
    unsafe { edp::restart(&mut s.s, s.permit_mappings) };
    s.native = [None, None];
    s.main_queue = None;
    s.main_list = core::ptr::null_mut();
    s.peer_list = core::ptr::null_mut();
    s.tokens = [None, None];
    S_OK
}
/// # Safety
/// The queue and lists are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_before_submit(p: *mut c_void, queue: *mut c_void, count: u32, submitted: *const *mut c_void) {
    let s = pass(p);
    let Some(queue) = borrowed::<ID3D12CommandQueue>(queue) else { return };
    let objects = s.objects();
    let host = s.host();
    // SAFETY: as documented.
    unsafe { edp::before_submit(&mut s.s, &objects, &queue, lists(submitted, count), &host) };
}
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edo_submitted(p: *mut c_void, queue: *mut c_void, count: u32, submitted: *const *mut c_void) {
    let s = pass(p);
    let Some(queue_object) = borrowed::<ID3D12CommandQueue>(queue) else { return };
    let (main_list, peer_list, timestamps) = (s.main_list, s.peer_list, s.timestamps.is_some());
    let (main_fence, done_fence) = (raw(&s.main_fence), raw(&s.done_fence));
    // SAFETY: as documented.
    if unsafe { edp::submitted(&mut s.s, &queue_object, lists(submitted, count), main_list, peer_list, timestamps, main_fence, done_fence) } {
        s.main_queue = Some(queue_object);
    }
}
#[no_mangle]
pub extern "C" fn cvr_edo_reset(p: *mut c_void, list: *mut c_void) {
    let s = pass(p);
    let tokens = [s.tokens[0].is_some(), s.tokens[1].is_some()];
    edp::reset(&mut s.s, list, s.main_list, s.peer_list, tokens);
}
#[no_mangle]
pub extern "C" fn cvr_edo_main_submitted(p: *mut c_void) -> bool {
    let s = &pass(p).s;
    s.main_submitted && !s.failed
}
#[no_mangle]
pub extern "C" fn cvr_edo_failed(p: *mut c_void) -> bool {
    pass(p).s.failed
}
