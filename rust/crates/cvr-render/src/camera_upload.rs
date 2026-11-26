//! The native stereo camera upload (formerly src/Render/NativeStereoCameraUpload.cpp;
//! include/Render/NativeStereoCameraUpload.hpp declares it): the [MAIN, PEER] camera pair (2048 bytes) uploaded through
//! the engine's own allocator (0x333B88, its CBV captured as it is created) into an owned CPU descriptor; bound in place
//! of the camera table's slot for the requested stages (0x336D3C) only when each stage already reads the exact MAIN
//! camera (or the caller's freshly uploaded handle), the slots re-resolved after; restored through the camera binder
//! (0x337968); the delayed paired reference also materializes a fresh GPU table (0x335C3C) and checks the actual bound
//! 2048-byte CBV; the one-shot twin comparison upload and its bind after the native one.
//!
//! The descriptor audit's lookups, the constant upload registry and the native probe's state stay C++, lent through
//! the runtime; the binding tracker, slot resolution, flush state, scene bindings and command resources are Rust's.

use crate::camera_binding::{self as binding, BindingScope, Context, ReadState, Tracker};
use crate::camera_flush::{self as flush, Snapshot};
use crate::camera_upload_checks as checks;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use cvr_gpu::scene_state::Bindings;
use std::sync::{Mutex, OnceLock};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

pub const CAMERA_BYTES: usize = checks::CAMERA_BYTES;
pub type Camera = [u8; CAMERA_BYTES];
const CAPACITY: u32 = 128;

/// camera_upload::Result.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Upload {
    pub cpu_descriptor: u64,
    pub gpu_address: u64,
    pub bytes: u32,
    pub reserved: u32,
}
/// descriptors::CbvBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CbvBinding {
    pub descriptor: u64,
    pub heap: u64,
    pub address: u64,
    pub bytes: u32,
}
/// native_probe::PipelineRecord.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineRecord {
    pub original: u64,
    pub instanced: u64,
    pub root: u64,
    pub result: i32,
    pub targets: u32,
    pub depth_format: u32,
    pub sample_count: u32,
    pub camera_root: u32,
    pub camera_table_offset: u32,
    pub formats: [u32; 8],
}
/// What the .cpp lends: the twin exports and the native probe's pipelines and state (the descriptor audit's lookups
/// and the constant upload registry's read are Rust's, called directly).
#[repr(C)]
pub struct Runtime {
    pub twin_upload: *const AtomicU32,
    pub twin_allocations: *const AtomicU32,
    pub twin_bindings: *const AtomicU32,
    pub twin_failures: *const AtomicU32,
    pub trace_state: *const AtomicU32,
    pub pipeline_count: *const AtomicU32,
    pub pipelines: *const PipelineRecord,
    /// native_probe::Enabled.
    pub probe_enabled: extern "C" fn() -> bool,
}
const _: () = assert!(
    core::mem::size_of::<Upload>() == 24 && core::mem::size_of::<CbvBinding>() == 32 && core::mem::size_of::<PipelineRecord>() == 80 && core::mem::size_of::<Runtime>() == 64
);
// SAFETY: the runtime is immutable; its pointers name process-lifetime exports and functions.
unsafe impl Sync for Runtime {}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own runtime (formerly lent by src/Render/NativeStereoCameraUpload.cpp): the exports Rust defines, the
/// native probe's pipeline records (C++) and its switch (stereo_probe_creation.rs). The Rust test variants attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    #![allow(non_upper_case_globals)]
    use super::{PipelineRecord, Runtime};
    use crate::declared_moved_exports::{
        CyberpunkVR_NativeStereoPipelineCount, CyberpunkVR_SinglePassTraceState, CyberpunkVR_SinglePassTwinAllocations,
        CyberpunkVR_SinglePassTwinBindings, CyberpunkVR_SinglePassTwinFailures, CyberpunkVR_SinglePassTwinUpload,
    };
    extern "C" {
        /// src/Render/NativeStereoProbe.cpp's records.
        static CyberpunkVR_NativeStereoPipelines: [PipelineRecord; 8];
    }
    extern "C" fn probe_enabled() -> bool {
        crate::stereo_probe_creation::enabled()
    }
    pub static RUNTIME: Runtime = Runtime {
        twin_upload: &raw const CyberpunkVR_SinglePassTwinUpload,
        twin_allocations: &raw const CyberpunkVR_SinglePassTwinAllocations,
        twin_bindings: &raw const CyberpunkVR_SinglePassTwinBindings,
        twin_failures: &raw const CyberpunkVR_SinglePassTwinFailures,
        trace_state: &raw const CyberpunkVR_SinglePassTraceState,
        pipeline_count: &raw const CyberpunkVR_NativeStereoPipelineCount,
        pipelines: (&raw const CyberpunkVR_NativeStereoPipelines).cast(),
        probe_enabled,
    };
}
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::RUNTIME));
    attached
}
fn export(at: *const AtomicU32) -> &'static AtomicU32 {
    static ZERO: AtomicU32 = AtomicU32::new(0);
    // SAFETY: the runtime's exports, live for the process.
    unsafe { at.as_ref() }.unwrap_or(&ZERO)
}
fn twin_upload() -> u32 {
    rt().map_or(0, |r| export(r.twin_upload).load(Relaxed))
}
fn trace_state() -> u32 {
    rt().map_or(0, |r| export(r.trace_state).load(Relaxed))
}
fn bump(field: fn(&Runtime) -> *const AtomicU32) {
    if let Some(r) = rt() {
        export(field(r)).fetch_add(1, SeqCst);
    }
}
fn resolve_cpu_cbv(descriptor: u64) -> Option<CbvBinding> {
    let mut out = CbvBinding::default();
    (rt().is_some() && cpu_cbv(descriptor, &mut out)).then_some(out)
}
fn read_upload(address: u64, out: &mut Camera) -> bool {
    // SAFETY: the registry's read into the camera's bytes (constants::ReadUpload).
    rt().is_some() && unsafe { crate::upload_registry::cvr_upload_read(address, out.as_mut_ptr().cast(), out.len(), false) }
}
/// descriptors::ResolveCpuCbvAddress (the descriptor audit's CbvBinding has this layout).
fn cpu_cbv(descriptor: u64, out: &mut CbvBinding) -> bool {
    // SAFETY: a CbvBinding of the audit's layout.
    unsafe { crate::descriptor_audit::cvr_sda_audit_cbv(2, descriptor, 0, 0, 0, (out as *mut CbvBinding).cast()) }
}

/// The .cpp's Read.
unsafe extern "C-unwind" fn read_state(address: usize, out: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the caller's output of `bytes`.
    address != 0 && crate::geometry_packets::read(address as u64, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
const READ: Option<ReadState> = Some(read_state);
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    (address != 0 && crate::geometry_packets::read(address, &mut b)).then(|| u64::from_le_bytes(b))
}

// The engine's functions (their code guarded by camera_upload_checks).
type GetContext = unsafe extern "C" fn() -> *mut c_void;
type UploadCbv = unsafe extern "C" fn(u32, *const c_void, u64);
type Bind = unsafe extern "C" fn(*mut c_void, u8, u32, u32, *const u64, bool);
type BindCamera = unsafe extern "C" fn(u32, u32, u32) -> i64;
type Materialize = unsafe extern "C" fn(*mut c_void, *mut c_void, u8);
fn engine<T: Copy>(base: u64, rva: u64) -> T {
    debug_assert_eq!(core::mem::size_of::<T>(), 8);
    // SAFETY: a function of the executable at its guarded RVA.
    unsafe { core::mem::transmute_copy(&(base + rva)) }
}

fn native_compatible(base: u64) -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    base != 0 && *COMPATIBLE.get_or_init(|| checks::cvr_ncu_native_code(base))
}
fn flush_compatible(base: u64) -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    base != 0 && *COMPATIBLE.get_or_init(|| checks::cvr_ncu_flush_code(base))
}
/// Current: the engine's current context and its list and state.
pub fn current(base: u64) -> Context {
    if !native_compatible(base) {
        return Context::default();
    }
    // SAFETY: the guarded getter.
    let context = unsafe { engine::<GetContext>(base, 0x337230)() } as u64;
    match checks::context_fields(&crate::geometry_packets::read, context) {
        Some((list, state)) => Context { context, state, list },
        None => Context::default(),
    }
}

#[repr(C, align(16))]
struct Pair([u8; 2048]);
fn pair(main: &Camera, peer: &Camera) -> Pair {
    let mut bytes = Pair([0; 2048]);
    bytes.0[..CAMERA_BYTES].copy_from_slice(main);
    bytes.0[1024..1024 + CAMERA_BYTES].copy_from_slice(peer);
    bytes
}

struct FlushReceipt {
    original: Snapshot,
    flushed: Snapshot,
    bindings: Bindings,
    resource_heap: Option<ID3D12DescriptorHeap>,
    root: Option<ID3D12RootSignature>,
    stages: u32,
}
thread_local! {
    static EXPECTED: Cell<*mut Upload> = const { Cell::new(core::ptr::null_mut()) };
    static PENDING: Cell<(u32, Upload)> = const { Cell::new((0, Upload { cpu_descriptor: 0, gpu_address: 0, bytes: 0, reserved: 0 })) };
    static CURRENT_BINDING: Cell<Upload> = const { Cell::new(Upload { cpu_descriptor: 0, gpu_address: 0, bytes: 0, reserved: 0 }) };
    static TRACKER: Cell<Tracker> = Cell::new(Tracker::default());
    static DIAGNOSTICS: Cell<[u64; 16]> = const { Cell::new([0; 16]) };
    static PAIR_FLUSH: RefCell<Option<FlushReceipt>> = const { RefCell::new(None) };
}
fn diag(f: impl FnOnce(&mut [u64; 16])) {
    DIAGNOSTICS.with(|d| {
        let mut v = d.get();
        f(&mut v);
        d.set(v);
    });
}
fn diagnostics() -> [u64; 16] {
    DIAGNOSTICS.with(Cell::get)
}
fn pair_flush_present() -> bool {
    PAIR_FLUSH.with(|p| p.borrow().is_some())
}
/// ResultCapture: the engine's upload creates its CBV with `result` expected.
fn capture_upload(result: &mut Upload, upload: impl FnOnce()) {
    let previous = EXPECTED.with(|e| e.replace(result));
    upload();
    EXPECTED.with(|e| e.set(previous));
}

/// AllocateNative: the pair uploaded into the caller's (or a new) one-descriptor CPU heap.
///
/// # Safety
/// `heap` is the caller's ComPtr.
unsafe fn allocate_native(base: u64, context: Context, main: &Camera, peer: &Camera, heap: *mut *mut c_void, output: &mut Upload) -> bool {
    *output = Upload { reserved: 1, ..Upload::default() };
    if !native_compatible(base) || !EXPECTED.with(Cell::get).is_null() {
        return false;
    }
    output.reserved = 2;
    if !context.valid() {
        return false;
    }
    let raw = context.list as *mut c_void;
    // SAFETY: the engine's live list.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }) else { return false };
    output.reserved = 3;
    let mut device: Option<ID3D12Device> = None;
    if unsafe { list.GetDevice(&mut device) }.is_err() || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    let Some(device) = device else { return false };
    // SAFETY: the caller's ComPtr.
    if unsafe { (*heap).is_null() } {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 1, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
        match unsafe { device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) } {
            Ok(created) => unsafe { *heap = created.into_raw() },
            Err(_) => return false,
        }
    }
    // SAFETY: the ComPtr's object.
    let Some(owned) = (unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&*heap) }) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    if unsafe { owned.GetDevice(&mut actual) }.is_err() || actual.as_ref().map(Interface::as_raw) != Some(device.as_raw()) {
        return false;
    }
    let desc = unsafe { owned.GetDesc() };
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.NumDescriptors != 1 || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE {
        return false;
    }
    output.reserved = 4;
    let bytes = pair(main, peer);
    let mut result = Upload { cpu_descriptor: unsafe { owned.GetCPUDescriptorHandleForHeapStart() }.ptr as u64, ..Upload::default() };
    // SAFETY: the guarded uploader with its 2048 bytes and our descriptor.
    let cpu = result.cpu_descriptor;
    capture_upload(&mut result, || unsafe { engine::<UploadCbv>(base, 0x333B88)(2048, bytes.0.as_ptr().cast(), cpu) });
    if !checks::uploaded(result.bytes, result.gpu_address) {
        return false;
    }
    *output = result;
    true
}
pub fn observe_native_bind(base: u64, binding: u32, handle: u32, stages: u32) {
    if binding == 1 && stages & 3 != 0 {
        let context = current(base);
        TRACKER.with(|t| {
            let mut tracker = t.get();
            tracker.observe(READ, context, handle, stages);
            t.set(tracker);
        });
    }
}
fn bind_stages(base: u64, state: u64, stages: u32, slot: u32, descriptor: *const u64) {
    for stage in 0..2u32 {
        if stages & (1 << stage) != 0 {
            // SAFETY: the guarded binder with the engine's state.
            unsafe { engine::<Bind>(base, 0x336D3C)(state as *mut c_void, stage as u8, 2, slot, descriptor, true) };
        }
    }
}
/// BeginStages: every requested stage must already read the exact current camera (the original shaders may still
/// execute in this scope); the pair allocated; the owned descriptor bound in the camera slot of each stage.
///
/// # Safety
/// `heap` is the caller's ComPtr.
#[allow(clippy::too_many_arguments)]
unsafe fn begin_stages(base: u64, native_handle: u32, current_camera: &Camera, main: &Camera, peer: &Camera, heap: *mut *mut c_void, output: &mut Upload, stages: u32, scope: &mut BindingScope) -> bool {
    *output = Upload { reserved: 5, ..Upload::default() };
    *scope = BindingScope::default();
    DIAGNOSTICS.with(|d| d.set([0; 16]));
    let context = current(base);
    diag(|d| (d[0], d[1], d[2], d[3]) = (stages.into(), context.context, context.state, context.list));
    if !TRACKER.with(Cell::get).capture(READ, context, stages, scope) {
        return false;
    }
    let (mut trusted_address, mut trusted_bytes) = (0u64, 0u32);
    for stage in 0..2usize {
        if stages & (1 << stage) == 0 {
            continue;
        }
        let original = scope.original[stage];
        output.reserved = 6 + stage as u32;
        let resolved = resolve_cpu_cbv(original.descriptor);
        let cbv = resolved.unwrap_or_default();
        diag(|d| (d[4], d[5], d[6], d[7], d[8], d[9]) = (stage as u64, original.handle.into(), original.descriptor, resolved.is_some().into(), cbv.address, cbv.bytes.into()));
        // BufUpload camera handles can name DEFAULT buffers, which are not readable through the CPU upload registry.
        // Both stages using the caller's freshly uploaded/bound handle share its exact camera input.
        if checks::trusted(resolved.is_some(), cbv.bytes, native_handle, original.handle, cbv.address, trusted_address, trusted_bytes) {
            (trusted_address, trusted_bytes) = (cbv.address, cbv.bytes);
            diag(|d| (d[10], d[11], d[15]) = (2, native_handle.into(), 2));
            continue;
        }
        let (mut before, mut again) = ([0u8; CAMERA_BYTES], [0u8; CAMERA_BYTES]);
        let first = resolved.is_some() && cbv.bytes as usize >= CAMERA_BYTES && read_upload(cbv.address, &mut before);
        let second = first && read_upload(cbv.address, &mut again);
        diag(|d| (d[10], d[11], d[12]) = (first.into(), second.into(), u64::MAX));
        if first {
            if let Some((index, actual, expected)) = checks::first_difference(current_camera, &before) {
                diag(|d| (d[12], d[13], d[14]) = (index.into(), actual.into(), expected.into()));
            }
        }
        diag(|d| d[15] = (first && second && before == again).into());
        if !first || !second || before != *current_camera || again != *current_camera {
            *scope = BindingScope::default();
            return false;
        }
    }
    if !unsafe { allocate_native(base, context, main, peer, heap, output) } {
        *scope = BindingScope::default();
        return false;
    }
    for stage in 0..2u32 {
        if stages & (1 << stage) != 0 {
            let changed = || TRACKER.with(Cell::get).changed(scope, stage);
            if current(base) != context || changed() || binding::resolve(READ, context.state, stage).is_none_or(|slot| slot.descriptor != scope.original[stage as usize].descriptor) {
                output.reserved = 8;
                *scope = BindingScope::default();
                return false;
            }
        }
    }
    scope.replacement = output.cpu_descriptor;
    for stage in 0..2u32 {
        if stages & (1 << stage) != 0 {
            scope.applied |= 1 << stage;
            bind_stages(base, context.state, 1 << stage, 1, &scope.replacement);
        }
    }
    for stage in 0..2u32 {
        if stages & (1 << stage) != 0 && (current(base) != context || binding::resolve(READ, context.state, stage).is_none_or(|slot| slot.descriptor != scope.replacement)) {
            output.reserved = 9;
            end_native_stages(base, scope);
            return false;
        }
    }
    output.reserved = 0;
    true
}
unsafe extern "C-unwind" fn restore_binding(user: *mut c_void, expected: Context, handle: u32, stages: u32) -> bool {
    let base = user as u64;
    if current(base) != expected || handle == 0 {
        return false;
    }
    // SAFETY: the guarded camera binder.
    unsafe { engine::<BindCamera>(base, 0x337968)(1, handle, stages) };
    current(base) == expected
}
pub fn end_native_stages(base: u64, scope: &mut BindingScope) -> bool {
    if scope.applied == 0 {
        return true;
    }
    // SAFETY: the scope's own restore through the camera binder.
    unsafe { binding::restore_scope(READ, current(base), scope, Some(restore_binding), base as *mut c_void) }
}
fn same_bindings(before: &Bindings, list: u64) -> bool {
    let mut after = Bindings::default();
    // SAFETY: the engine's list; the output is ours.
    (unsafe { cvr_gpu::scene_state::cvr_scene_snapshot(list as usize, &mut after) }) && before.root == after.root && before.heap_count == after.heap_count && before.heaps == after.heaps
}
fn heap_cache(state: &Snapshot, heap: Option<&ID3D12DescriptorHeap>) -> bool {
    heap.is_some_and(|h| (state.cpu_valid == 0 || state.cached_cpu == unsafe { h.GetCPUDescriptorHandleForHeapStart() }.ptr as u64) && (state.gpu_valid == 0 || state.cached_gpu == unsafe { h.GetGPUDescriptorHandleForHeapStart() }.ptr))
}
/// GpuCamera: each stage's camera slot found in the flushed tables at its root parameter, its CPU and GPU CBVs the
/// same camera-sized buffer (the replacement's 2048 bytes when given).
fn gpu_camera(list: u64, root: u64, stages: u32, state: &Snapshot, replacement: Option<&Upload>) -> bool {
    if rt().is_none() {
        return false;
    }
    for stage in 0..2u32 {
        if stages & (1 << stage) == 0 {
            continue;
        }
        let (mut parameter, mut offset) = (0u32, 0u32);
        let visibility = (if stage != 0 { D3D12_SHADER_VISIBILITY_PIXEL.0 } else { D3D12_SHADER_VISIBILITY_VERTEX.0 }) as u32;
        let Some(slot) = binding::resolve(READ, state.context.state, stage) else { return false };
        // SAFETY: descriptors::FindBinding into two locals.
        if !unsafe { crate::descriptor_audit::cvr_sda_audit_find_binding(root, D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 as u32, 0, 1, visibility, &mut parameter, &mut offset) } {
            return false;
        }
        let Some(found) = state.tables.iter().find(|t| t.group == slot.parameter) else { return false };
        let (mut cpu, mut gpu) = (CbvBinding::default(), CbvBinding::default());
        if found.root_parameter != parameter
            || offset != 1u32.wrapping_sub(found.first)
            || !cpu_cbv(slot.descriptor, &mut cpu)
            // SAFETY: descriptors::ResolveCbv at the list into a CbvBinding of the audit's layout.
            || !unsafe { crate::descriptor_audit::cvr_sda_audit_cbv(0, list, root, parameter, offset, (&mut gpu as *mut CbvBinding).cast()) }
            || cpu.address != gpu.address
            || cpu.bytes != gpu.bytes
            || (cpu.bytes as usize) < CAMERA_BYTES
            || replacement.is_some_and(|x| gpu.address != x.gpu_address || gpu.bytes != 2048)
        {
            return false;
        }
        diag(|d| (d[7], d[8], d[9]) = (1, gpu.address, gpu.bytes.into()));
    }
    true
}
/// Flush: the original indexed draw's own materialization before its D3D draw; the no-rollover receipt reserves both
/// new tables and forbids heap replacement.
fn flush_tables(base: u64, update: &Snapshot, heap: Option<&ID3D12DescriptorHeap>, bindings: &Bindings, stages: u32) -> Option<Snapshot> {
    if !flush_compatible(base) || current(base) != update.context || !heap_cache(update, heap) || !same_bindings(bindings, update.context.list) {
        return None;
    }
    // SAFETY: the guarded materialization with the engine's state and context.
    unsafe { engine::<Materialize>(base, 0x335C3C)(update.context.state as *mut c_void, update.context.context as *mut c_void, 0) };
    if current(base) != update.context {
        return None;
    }
    let after = flush::read(READ, update.context, stages)?;
    (flush::flushed(update, &after) && after.cpu_valid != 0 && after.gpu_valid != 0 && heap_cache(&after, heap) && same_bindings(bindings, update.context.list)).then_some(after)
}
fn borrowed<T: Interface + Clone>(raw: usize) -> Option<T> {
    let raw = raw as *mut c_void;
    // SAFETY: a live object named by the scene's bindings.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
/// BeginNativePairStages: the delayed paired reference (both native draws already executed) -- the current camera
/// checked independently from the [MAIN, PEER] prefix, a fresh GPU table materialized and its bound 2048-byte CBV
/// verified.
///
/// # Safety
/// `heap` is the caller's ComPtr.
#[allow(clippy::too_many_arguments)]
pub unsafe fn begin_native_pair_stages(base: u64, current_handle: u32, expected_current: &Camera, main: &Camera, peer: &Camera, heap: *mut *mut c_void, output: &mut Upload, stages: u32, scope: &mut BindingScope) -> bool {
    *output = Upload::default();
    *scope = BindingScope::default();
    DIAGNOSTICS.with(|d| d.set([0; 16]));
    diag(|d| d[12] = 20);
    let context = current(base);
    diag(|d| (d[0], d[1], d[2], d[3]) = (stages.into(), context.context, context.state, context.list));
    let compatible = flush_compatible(base);
    diag(|d| d[15] = compatible.into());
    if pair_flush_present() {
        diag(|d| d[12] = 25);
        return false;
    }
    if !compatible || !context.valid() {
        return false;
    }
    diag(|d| d[12] = 26);
    let value = |at: u64| read_u64(at).unwrap_or(0);
    let state_head = (value(context.state), value(context.state + 0x68), value(context.state + 0x70), value(context.state + 0x78));
    diag(|d| (d[4], d[5], d[6], d[7]) = state_head);
    for i in 0..4u64 {
        let mut b = [0u8; 4];
        let word = if crate::geometry_packets::read(context.state + 8 + i * 4, &mut b) { u32::from_le_bytes(b) } else { 0 };
        diag(|d| d[8 + i as usize] = word.into());
    }
    let (mut rebind, mut program) = ([0u8; 1], [0u8; 4]);
    crate::geometry_packets::read(context.state + 0x180, &mut rebind);
    crate::geometry_packets::read(context.context + 0x3C0, &mut program);
    diag(|d| (d[13], d[14]) = (rebind[0].into(), u32::from_le_bytes(program).into()));
    let Some(original) = flush::read(READ, context, stages) else { return false };
    diag(|d| (d[12], d[13], d[14]) = (27, original.groups, original.count.into()));
    if !flush::room(&original) {
        return false;
    }
    diag(|d| d[12] = 21);
    let mut bindings = Bindings::default();
    // SAFETY: the engine's list; the output is ours.
    if !unsafe { cvr_gpu::scene_state::cvr_scene_snapshot(context.list as usize, &mut bindings) } || !bindings.reset_seen || !bindings.heaps_seen || bindings.root == 0 {
        return false;
    }
    let mut resource_heap: Option<ID3D12DescriptorHeap> = None;
    for i in 0..bindings.heap_count.min(2) as usize {
        if bindings.heaps[i] as u64 == original.heap {
            resource_heap = borrowed(bindings.heaps[i]);
        }
    }
    let Some(heap_object) = resource_heap.clone() else { return false };
    let root: Option<ID3D12RootSignature> = borrowed(bindings.root);
    let raw = context.list as *mut c_void;
    // SAFETY: the engine's live list.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }) else { return false };
    let desc = unsafe { heap_object.GetDesc() };
    let (mut device, mut heap_device): (Option<ID3D12Device>, Option<ID3D12Device>) = (None, None);
    let checked = unsafe { list.GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT
        && desc.Type == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
        && desc.Flags == D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        && u64::from(original.base) + u64::from(original.capacity) <= u64::from(desc.NumDescriptors)
        && unsafe { list.GetDevice(&mut device) }.is_ok()
        && unsafe { heap_object.GetDevice(&mut heap_device) }.is_ok()
        && device.as_ref().map(Interface::as_raw) == heap_device.as_ref().map(Interface::as_raw)
        && device.as_ref().is_some_and(|d| original.stride == unsafe { d.GetDescriptorHandleIncrementSize(desc.Type) })
        && heap_cache(&original, Some(&heap_object))
        && gpu_camera(context.list, bindings.root as u64, stages, &original, None)
        && cvr_gpu::command_resources::keep(list, &[heap_object.as_raw(), root.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw)]);
    if !checked {
        return false;
    }
    let receipt = FlushReceipt { original, flushed: Snapshot::default(), bindings, resource_heap, root, stages };
    PAIR_FLUSH.with(|p| *p.borrow_mut() = Some(receipt));
    if !unsafe { begin_stages(base, current_handle, expected_current, main, peer, heap, output, stages, scope) } {
        diag(|d| d[12] = 100 + u64::from(output.reserved));
        let failed = diagnostics();
        if scope.applied != 0 {
            end_native_pair_stages(base, scope);
        } else {
            PAIR_FLUSH.with(|p| *p.borrow_mut() = None);
        }
        DIAGNOSTICS.with(|d| d.set(failed));
        return false;
    }
    diag(|d| d[12] = 22);
    let (original, heap_object, bindings, root) = PAIR_FLUSH.with(|p| {
        let p = p.borrow();
        let r = p.as_ref().expect("the receipt just stored");
        (r.original.clone(), r.resource_heap.clone(), r.bindings, r.root.as_ref().map_or(0, |x| x.as_raw() as u64))
    });
    let flushed = flush::read(READ, context, stages)
        .filter(|update| flush::pending(&original, update, output.cpu_descriptor))
        .and_then(|update| flush_tables(base, &update, heap_object.as_ref(), &bindings, stages))
        .filter(|flushed| gpu_camera(context.list, root, stages, flushed, Some(output)));
    match flushed {
        Some(flushed) => {
            PAIR_FLUSH.with(|p| {
                if let Some(r) = p.borrow_mut().as_mut() {
                    r.flushed = flushed;
                }
            });
            diag(|d| (d[12], d[15]) = (0, 1));
            true
        }
        None => {
            let failed = diagnostics();
            let restored = end_native_pair_stages(base, scope);
            DIAGNOSTICS.with(|d| d.set(failed));
            diag(|d| (d[14], d[15]) = (restored.into(), 0));
            false
        }
    }
}
/// EndNativePairStages: restoring one shared VS/PS table can satisfy both stages; the exact CPU descriptor comparison
/// distinguishes that from an unrelated rebind.
pub fn end_native_pair_stages(base: u64, scope: &mut BindingScope) -> bool {
    let Some(receipt) = PAIR_FLUSH.with(|p| p.borrow_mut().take()) else { return end_native_stages(base, scope) };
    diag(|d| d[12] = 23);
    if !end_native_stages(base, scope) || current(base) != receipt.original.context {
        return false;
    }
    let Some(update) = flush::read(READ, receipt.original.context, receipt.stages) else { return false };
    if !flush::same_allocation(&receipt.original, &update)
        || update.dirty != (receipt.original.dirty | update.groups)
        || update.rebind != 0
        || update.cursor > update.capacity
        || update.count > update.capacity - update.cursor
    {
        return false;
    }
    for (i, table) in update.tables.iter().enumerate() {
        if receipt.original.tables.get(i).map(|t| &t.descriptors) != Some(&table.descriptors) || table.cached != -1 {
            return false;
        }
    }
    let root = receipt.root.as_ref().map_or(0, |x| x.as_raw() as u64);
    let Some(after) = flush_tables(base, &update, receipt.resource_heap.as_ref(), &receipt.bindings, receipt.stages) else { return false };
    if !gpu_camera(update.context.list, root, receipt.stages, &after, None) {
        return false;
    }
    diag(|d| (d[12], d[15]) = (0, 1));
    true
}
/// BeginNative: the scoped depth path -- the game's allocator owns the GPU bytes; the caller retains the CPU descriptor
/// until the native recording has consumed it.
///
/// # Safety
/// `heap` is the caller's ComPtr.
pub unsafe fn begin_native(base: u64, main: &Camera, peer: &Camera, heap: *mut *mut c_void, output: &mut Upload) -> bool {
    let context = current(base);
    if !unsafe { allocate_native(base, context, main, peer, heap, output) } {
        return false;
    }
    bind_stages(base, context.state, 1, 1, &output.cpu_descriptor);
    true
}
pub fn end_native(base: u64, handle: u32) {
    if base != 0 && handle != 0 {
        // SAFETY: the guarded camera binder.
        unsafe { engine::<BindCamera>(base, 0x337968)(1, handle, 1) };
    }
}
/// Prepare: the one-shot comparison path -- the engine's current upload allocator and an owned CPU-only descriptor
/// (one of 128); ordinary shaders still read the unchanged prefix.
pub fn prepare(base: u64, native_handle: u32, first: &Camera, second: &Camera) -> Upload {
    PENDING.with(|p| p.set((0, Upload::default())));
    let Some(r) = rt() else { return Upload::default() };
    if base == 0 || native_handle == 0 || twin_upload() == 0 || trace_state() != 1 || !(r.probe_enabled)() {
        return Upload::default();
    }
    // A validated native prototype must exist before attempting its camera path.
    let count = export(r.pipeline_count).load(Acquire) as usize;
    // SAFETY: the probe's published pipelines.
    let pipelines = if r.pipelines.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(r.pipelines, count) } };
    let Some(pipeline) = pipelines.iter().find(|p| p.instanced != 0).map(|p| p.original as *mut c_void).filter(|p| !p.is_null()) else { return Upload::default() };
    struct Heap {
        heap: Option<ID3D12DescriptorHeap>,
        stride: u32,
        next: u32,
    }
    // SAFETY (Send): the heap is a free-threaded D3D12 object.
    struct Shared(Heap);
    unsafe impl Send for Shared {}
    static HEAP: Mutex<Shared> = Mutex::new(Shared(Heap { heap: None, stride: 0, next: 0 }));
    let mut shared = HEAP.lock().unwrap_or_else(|e| e.into_inner());
    let h = &mut shared.0;
    if h.next >= CAPACITY {
        return Upload::default();
    }
    if h.heap.is_none() {
        // The probe retains the original PSO, which supplies its actual device.
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: the probe's retained pipeline.
        let pso = unsafe { ID3D12PipelineState::from_raw_borrowed(&pipeline) };
        let Some(device) = pso.filter(|p| unsafe { p.GetDevice(&mut device) }.is_ok()).and(device) else {
            bump(|r| r.twin_failures);
            return Upload::default();
        };
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: CAPACITY, ..Default::default() };
        let Ok(created) = (unsafe { device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) }) else {
            bump(|r| r.twin_failures);
            return Upload::default();
        };
        h.stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
        h.heap = Some(created);
    }
    let data = pair(first, second);
    let start = unsafe { h.heap.as_ref().expect("created above").GetCPUDescriptorHandleForHeapStart() }.ptr as u64;
    let mut result = Upload { cpu_descriptor: start + u64::from(h.next) * u64::from(h.stride), ..Upload::default() };
    h.next += 1;
    // SAFETY: the guarded uploader with its 2048 bytes and our descriptor.
    let cpu = result.cpu_descriptor;
    capture_upload(&mut result, || unsafe { engine::<UploadCbv>(base, 0x333B88)(2048, data.0.as_ptr().cast(), cpu) });
    if !checks::uploaded(result.bytes, result.gpu_address) {
        bump(|r| r.twin_failures);
        return Upload::default();
    }
    bump(|r| r.twin_allocations);
    PENDING.with(|p| p.set((native_handle, result)));
    result
}
/// CreatedCbv: the engine's CBV for the upload being captured.
pub fn created_cbv(location: u64, size: u32, destination: u64) {
    let expected = EXPECTED.with(Cell::get);
    // SAFETY: the capture's live result.
    if let Some(result) = unsafe { expected.as_mut() } {
        if destination == result.cpu_descriptor {
            result.gpu_address = location;
            result.bytes = size;
        }
    }
}
/// AfterNativeBind: the prepared twin bound after the native camera bind of the same handle, in the VRCAM scene.
pub fn after_native_bind(base: u64, binding: u32, native_handle: u32, stages: u32, scene_vrcam: bool) {
    if binding == 1 {
        CURRENT_BINDING.with(|c| c.set(Upload::default()));
    }
    let (handle, upload) = PENDING.with(Cell::get);
    let descriptor = upload.cpu_descriptor;
    if descriptor == 0 || binding != 1 || native_handle != handle {
        return;
    }
    PENDING.with(|p| p.set((0, Upload::default())));
    if base == 0 || !scene_vrcam || stages == 0 || stages > 3 || trace_state() != 1 || twin_upload() == 0 {
        return;
    }
    // SAFETY: the guarded getter.
    let current = unsafe { engine::<GetContext>(base, 0x337230)() } as u64;
    let Some(state) = read_u64(current.wrapping_add(0x60)).filter(|&s| s != 0) else {
        bump(|r| r.twin_failures);
        return;
    };
    bind_stages(base, state, stages, binding, &descriptor);
    CURRENT_BINDING.with(|c| c.set(upload));
    bump(|r| r.twin_bindings);
}
pub fn current_binding() -> Upload {
    if twin_upload() != 0 && trace_state() == 1 {
        CURRENT_BINDING.with(Cell::get)
    } else {
        Upload::default()
    }
}

// ---- the C ABI

/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_current(base: u64, out: *mut Context) {
    unsafe { *out = current(base) };
}
/// # Safety
/// The cameras hold 848 bytes; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_prepare(base: u64, handle: u32, first: *const Camera, second: *const Camera, out: *mut Upload) {
    unsafe { *out = prepare(base, handle, &*first, &*second) };
}
/// # Safety
/// `desc` is null or a CBV description.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_created_cbv(desc: *const D3D12_CONSTANT_BUFFER_VIEW_DESC, destination: u64) {
    if let Some(d) = unsafe { desc.as_ref() } {
        created_cbv(d.BufferLocation, d.SizeInBytes, destination);
    }
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_current_binding(out: *mut Upload) {
    unsafe { *out = current_binding() };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_after_bind(base: u64, binding: u32, handle: u32, stages: u32, scene_vrcam: bool) {
    after_native_bind(base, binding, handle, stages, scene_vrcam);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_observe_bind(base: u64, binding: u32, handle: u32, stages: u32) {
    observe_native_bind(base, binding, handle, stages);
}
/// # Safety
/// The cameras hold 848 bytes; `heap` is the caller's ComPtr; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_begin_native(base: u64, main: *const Camera, peer: *const Camera, heap: *mut *mut c_void, out: *mut Upload) -> bool {
    unsafe { begin_native(base, &*main, &*peer, heap, &mut *out) }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_end_native(base: u64, handle: u32) {
    end_native(base, handle);
}
/// Stages, or with `pair` the delayed paired reference's.
///
/// # Safety
/// The cameras hold 848 bytes; `heap` is the caller's ComPtr; `out` and `scope` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ncu_begin_stages(pair: bool, base: u64, handle: u32, current_camera: *const Camera, main: *const Camera, peer: *const Camera, heap: *mut *mut c_void, out: *mut Upload, stages: u32, scope: *mut BindingScope) -> bool {
    unsafe {
        if pair {
            begin_native_pair_stages(base, handle, &*current_camera, &*main, &*peer, heap, &mut *out, stages, &mut *scope)
        } else {
            begin_stages(base, handle, &*main, &*main, &*peer, heap, &mut *out, stages, &mut *scope)
        }
    }
}
/// # Safety
/// `scope` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_end_stages(pair: bool, base: u64, scope: *mut BindingScope) -> bool {
    let scope = unsafe { &mut *scope };
    if pair {
        end_native_pair_stages(base, scope)
    } else {
        end_native_stages(base, scope)
    }
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_diagnostics(out: *mut [u64; 16]) {
    unsafe { *out = diagnostics() };
}

#[cfg(test)]
mod tests {
    //! The paths reachable without the engine: the CBV capture, the twin's refusals and counters, the pending twin's
    //! consumption, the binding's gate, the stages' refusals and the pair receipt's exclusion.
    use super::*;

    static TWIN: AtomicU32 = AtomicU32::new(0);
    static ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
    static BINDINGS: AtomicU32 = AtomicU32::new(0);
    static FAILURES: AtomicU32 = AtomicU32::new(0);
    static TRACE: AtomicU32 = AtomicU32::new(0);
    static PIPELINES: AtomicU32 = AtomicU32::new(0);
    extern "C" fn enabled() -> bool {
        true
    }
    static RT: Runtime = Runtime {
        twin_upload: &TWIN,
        twin_allocations: &ALLOCATIONS,
        twin_bindings: &BINDINGS,
        twin_failures: &FAILURES,
        trace_state: &TRACE,
        pipeline_count: &PIPELINES,
        pipelines: core::ptr::null(),
        probe_enabled: enabled,
    };

    #[test]
    fn captures_twins_and_refusals() {
        unsafe { cvr_ncu_attach(&RT) };
        // The capture: only the expected descriptor's CBV, only while capturing.
        let mut result = Upload { cpu_descriptor: 0x500, ..Upload::default() };
        created_cbv(0x9000, 2048, 0x500);
        assert_eq!(result.gpu_address, 0, "no capture outside the upload");
        capture_upload(&mut result, || {
            created_cbv(0x7700, 1024, 0x600);
            created_cbv(0x8800, 2048, 0x500);
        });
        assert_eq!((result.gpu_address, result.bytes), (0x8800, 2048));
        assert!(EXPECTED.with(Cell::get).is_null(), "the capture restored");
        // The twin: refused without its switch, the trace or a pipeline -- and nothing pending.
        let camera = [7u8; CAMERA_BYTES];
        assert_eq!(prepare(0x1000, 5, &camera, &camera), Upload::default());
        TWIN.store(1, SeqCst);
        TRACE.store(1, SeqCst);
        assert_eq!(prepare(0x1000, 5, &camera, &camera), Upload::default(), "no instanced pipeline");
        assert_eq!(FAILURES.load(SeqCst) + ALLOCATIONS.load(SeqCst), 0);
        // A pending twin: consumed by its own handle's bind, the current binding reset by any camera bind.
        CURRENT_BINDING.with(|c| c.set(result));
        assert_eq!(current_binding(), result, "the trace and the twin on");
        PENDING.with(|p| p.set((5, result)));
        after_native_bind(0, 1, 6, 1, true);
        assert_eq!(current_binding(), Upload::default(), "a camera bind resets the binding");
        assert_eq!(PENDING.with(Cell::get).0, 5, "another handle leaves the twin pending");
        after_native_bind(0, 1, 5, 1, true);
        assert_eq!(PENDING.with(Cell::get), (0, Upload::default()), "its handle consumes it (no base: not bound)");
        assert_eq!(BINDINGS.load(SeqCst), 0);
        TRACE.store(2, SeqCst);
        CURRENT_BINDING.with(|c| c.set(result));
        assert_eq!(current_binding(), Upload::default(), "the trace off");
        // The stages: no engine context -- refused at the tracker (5), nothing applied; nothing to restore.
        let (mut out, mut scope) = (Upload::default(), BindingScope { applied: 3, ..BindingScope::default() });
        let mut heap: *mut c_void = core::ptr::null_mut();
        assert!(!unsafe { begin_stages(0, 5, &camera, &camera, &camera, &mut heap, &mut out, 1, &mut scope) });
        assert_eq!((out.reserved, scope.applied), (5, 0));
        assert!(end_native_stages(0, &mut scope), "an unapplied scope");
        assert!(heap.is_null(), "no heap made");
        // The pair: refused while another receipt is held (25); without one, the plain end.
        PAIR_FLUSH.with(|p| *p.borrow_mut() = Some(FlushReceipt { original: Snapshot::default(), flushed: Snapshot::default(), bindings: Bindings::default(), resource_heap: None, root: None, stages: 1 }));
        assert!(!unsafe { begin_native_pair_stages(0, 5, &camera, &camera, &camera, &mut heap, &mut out, 1, &mut scope) });
        assert_eq!(diagnostics()[12], 25);
        assert!(!end_native_pair_stages(0, &mut scope) && !pair_flush_present(), "the receipt taken; no context to restore in");
        assert!(end_native_pair_stages(0, &mut scope), "no receipt: the plain end");
        assert!(!unsafe { begin_native_pair_stages(0, 5, &camera, &camera, &camera, &mut heap, &mut out, 1, &mut scope) });
        assert_eq!(diagnostics()[12], 20, "not compatible without the game");
    }
}
