//! The material binding's observations (formerly src/Render/NativeMaterialBinding.cpp): the engine's native material
//! bind (register 4, the pixel stage) observed per command list -- its CPU binding slot, the CBV it names and, for a
//! registered buffer, the buffer retained -- and resolved at a native draw into a material_binding::Lease (the
//! descriptor stamped, the source's offset), each step re-read so a moved binding is refused. Status codes are the
//! .cpp's (material_binding::Status).

use crate::buffer_read::Lease as BufferLease;
use crate::descriptor_audit::{CbvBinding, DescriptorStamp};
use crate::material::{self, Snapshot};
use crate::material_read::Lease;
use crate::texture_lease::UploadContext;
use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// material_binding::ShaderRegister, StageIndex, LogicalBytes.
const SHADER_REGISTER: u32 = 4;
const STAGE_INDEX: u32 = 1;
const LOGICAL_BYTES: u64 = 292;
const CAPACITY: usize = 128;

/// material_binding::Status.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ignored,
    Registered,
    Transient,
    UnsupportedExecutable,
    MissingContext,
    InvalidBinding,
    DescriptorMismatch,
    UnsupportedResource,
    MissingObservation,
    StaleObservation,
    Capacity,
    Failure,
}

extern "C" {
    fn cvr_binding_code(table: u32, base: u64) -> bool;
    fn cvr_sda_audit_cbv(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut CbvBinding) -> bool;
    fn cvr_sda_audit_stamp(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut DescriptorStamp, object: *mut c_void) -> bool;
}

/// One list's observed bind; a registered buffer's lease owns one reference to the buffer.
struct Observation {
    native: Snapshot,
    cpu: CbvBinding,
    source: BufferLease,
    serial: u64,
}
impl Drop for Observation {
    fn drop(&mut self) {
        release(self.source.resource);
    }
}
fn release(resource: *mut c_void) {
    if !resource.is_null() {
        // SAFETY: a reference this module owns, released once.
        drop(unsafe { windows::core::IUnknown::from_raw(resource) });
    }
}
fn add_ref(resource: *mut c_void) -> *mut c_void {
    // SAFETY: a live resource this module holds a reference to.
    if let Some(r) = unsafe { windows::core::IUnknown::from_raw_borrowed(&resource) } {
        r.clone().into_raw()
    } else {
        core::ptr::null_mut()
    }
}
// SAFETY: the raw resource is an owned reference to a free-threaded D3D12 object; the rest is plain data.
unsafe impl Send for Observation {}

struct State {
    observations: HashMap<usize, Observation>,
    next_serial: u64,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(|| State { observations: HashMap::new(), next_serial: 0 }))
}

fn zero_cbv() -> CbvBinding {
    CbvBinding { descriptor: 0, heap: 0, address: 0, bytes: 0 }
}
fn game() -> u64 {
    // SAFETY: the process's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
unsafe extern "C" fn read(_: *mut c_void, address: u64, output: *mut c_void, bytes: usize) -> bool {
    if address == 0 || address > u64::MAX - bytes as u64 {
        return false;
    }
    let mut copied = 0usize;
    // SAFETY: a guarded read of our own process into the caller's buffer.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            address as *const c_void,
            output,
            bytes,
            &mut copied,
        ) != 0
            && copied == bytes
    }
}
fn context(game: u64) -> UploadContext {
    let c = crate::camera_upload::current(game);
    UploadContext { context: c.context, state: c.state, list: c.list }
}
fn valid(c: &UploadContext) -> bool {
    c.context != 0 && c.state != 0 && c.list != 0
}
fn snapshot(game: u64, c: &UploadContext, handle: u32) -> Option<Snapshot> {
    // SAFETY: plain data, filled by the read.
    let mut out: Snapshot = unsafe { core::mem::zeroed() };
    // SAFETY: our reader over our own process; locals.
    unsafe { material::cvr_mat_read_snapshot(Some(read), core::ptr::null_mut(), game, c, handle, &mut out, false) }.then_some(out)
}
fn cpu_cbv(descriptor: u64) -> Option<CbvBinding> {
    let mut out = zero_cbv();
    // SAFETY: a local output.
    unsafe { cvr_sda_audit_cbv(2, descriptor, 0, 0, 0, &mut out) }.then_some(out)
}
fn gpu_cbv(list: *mut c_void, root: u64, parameter: u32, offset: u32) -> Option<CbvBinding> {
    let mut out = zero_cbv();
    // SAFETY: a local output.
    unsafe { cvr_sda_audit_cbv(0, list as u64, root, parameter, offset, &mut out) }.then_some(out)
}
fn same_cbv(a: &CbvBinding, b: &CbvBinding) -> bool {
    a.address == b.address && a.bytes == b.bytes
}
/// Active: not our own commands, and the native probe observes bindings.
fn active() -> bool {
    !cvr_host::probe::internal() && crate::stereo_probe::cvr_observe_bindings()
}
fn same_device(list: *mut c_void, resource: *mut c_void) -> bool {
    // SAFETY: the caller's list and the lease's resource (or null).
    let (Some(list), Some(resource)) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }, unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    let (mut a, mut b): (Option<ID3D12Device>, Option<ID3D12Device>) = (None, None);
    // SAFETY: live objects, local outputs.
    (unsafe { list.GetDevice(&mut a).is_ok() && resource.GetDevice(&mut b).is_ok() })
        && matches!((&a, &b), (Some(a), Some(b)) if a.as_raw() == b.as_raw())
}

/// material_binding::Compatible: the game's own image and its binding code (checked once).
pub fn compatible(game: u64) -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    game != 0 && game == self::game() && *COMPATIBLE.get_or_init(|| unsafe { cvr_binding_code(2, game) })
}
/// material_binding::Reset: the list's observation dropped (its buffer released outside the lock).
pub fn reset(list: *mut c_void) {
    let retired = with(|s| s.observations.remove(&(list as usize)));
    drop(retired);
}
/// material_binding::Clear.
pub fn clear() {
    let retired = with(|s| core::mem::take(&mut s.observations));
    drop(retired);
}
/// material_binding::ObserveBind: after the native bind returned (the stage INDEX, the pixel stage 1).
pub fn observe_bind(game: u64, binding: u32, handle: u32, stage: u32) -> Status {
    if binding != SHADER_REGISTER || stage != STAGE_INDEX || !active() {
        return Status::Ignored;
    }
    if !compatible(game) {
        return Status::UnsupportedExecutable;
    }
    let c = context(game);
    if !valid(&c) {
        clear();
        return Status::MissingContext;
    }
    let list = c.list as *mut c_void;
    reset(list);
    let Some(native) = snapshot(game, &c, handle) else { return Status::InvalidBinding };
    let Some(cpu) = cpu_cbv(native.slot.descriptor) else { return Status::DescriptorMismatch };
    // SAFETY: a local snapshot.
    if !unsafe { material::cvr_mat_fits_cbv(&native, cpu.address, cpu.bytes) } {
        return Status::DescriptorMismatch;
    }
    let transient = native.record.kind == 4;
    // SAFETY: plain data (a null resource).
    let mut next = Observation { native, cpu, source: unsafe { core::mem::zeroed() }, serial: 0 };
    if !transient {
        let buffer = material::native(&next.native);
        if let Some(lease) = crate::ia_binding::acquire_buffer(&buffer, cpu.address, LOGICAL_BYTES) {
            // SAFETY: the same layout (geometry::BufferLease); the owned reference moves into the observation.
            next.source = unsafe { core::mem::transmute::<crate::ia_binding::OwnedLease, BufferLease>(lease) };
        }
        let heap = next.source.heap_type;
        if next.source.resource.is_null()
            || !same_device(list, next.source.resource)
            || (heap != D3D12_HEAP_TYPE_DEFAULT.0 as u32 && heap != D3D12_HEAP_TYPE_UPLOAD.0 as u32 && heap != D3D12_HEAP_TYPE_GPU_UPLOAD.0 as u32)
            || next.source.flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 as u32 != 0
        {
            return Status::UnsupportedResource;
        }
    }
    let stale = context(game) != c
        || snapshot(game, &c, handle).is_none_or(|again| again != next.native)
        || cpu_cbv(next.native.slot.descriptor).is_none_or(|current| !same_cbv(&next.cpu, &current));
    if stale {
        return Status::StaleObservation;
    }
    // One critical section, as the .cpp's: a refused observation is released after the lock.
    let refused = with(move |s| {
        if s.observations.contains_key(&(list as usize)) {
            return Some((Status::StaleObservation, next));
        }
        if s.observations.len() >= CAPACITY || s.next_serial == u64::MAX {
            return Some((Status::Capacity, next));
        }
        s.next_serial += 1;
        next.serial = s.next_serial;
        s.observations.insert(list as usize, next);
        None
    });
    if let Some((status, retired)) = refused {
        drop(retired);
        return status;
    }
    if transient {
        Status::Transient
    } else {
        Status::Registered
    }
}
/// material_binding::ResolveAtDraw into a zeroed lease (the .cpp's `out={}` is the caller's): at an actual native draw,
/// the observation still current, the draw's CBV the observed one, the descriptor stamped; the lease gets its own
/// reference to a registered source.
pub fn resolve_at_draw(list: *mut c_void, root: u64, parameter: u32, offset: u32, out: &mut Lease) -> Status {
    if !active() {
        return Status::Ignored;
    }
    let game = game();
    if !compatible(game) {
        return Status::UnsupportedExecutable;
    }
    let c = context(game);
    if list.is_null() || root == 0 || !valid(&c) || c.list != list as u64 {
        return Status::MissingContext;
    }
    let saved = with(|s| {
        s.observations.get(&(list as usize)).map(|o| (o.native, o.cpu, o.source, o.serial))
    });
    let Some((saved_native, saved_cpu, saved_source, saved_serial)) = saved else { return Status::MissingObservation };
    if saved_native.context != c || saved_serial == 0 {
        return Status::StaleObservation;
    }
    let Some(current) = snapshot(game, &c, saved_native.handle) else { return Status::StaleObservation };
    let mismatch = || Status::DescriptorMismatch;
    let Some(cpu) = cpu_cbv(current.slot.descriptor) else { return mismatch() };
    if !same_cbv(&cpu, &saved_cpu) {
        return mismatch();
    }
    let Some(actual) = gpu_cbv(list, root, parameter, offset) else { return mismatch() };
    // SAFETY: locals.
    if !unsafe {
        material::cvr_mat_matches_draw(&saved_native, &current, parameter, offset, saved_cpu.address, saved_cpu.bytes, actual.address, actual.bytes)
    } {
        return mismatch();
    }
    // SAFETY: plain data, filled by the capture.
    let mut stamp: DescriptorStamp = unsafe { core::mem::zeroed() };
    // SAFETY: a local output.
    if !unsafe { cvr_sda_audit_stamp(0, list as u64, root, parameter, offset, &mut stamp, core::ptr::null_mut()) }
        || stamp.gpu != actual.descriptor
        || stamp.heap != actual.heap
        || stamp.value[0] != 1
        || stamp.value[4] != actual.address
        || stamp.value[5] != actual.bytes as u64
    {
        return mismatch();
    }
    let transient = current.record.kind == 4;
    let mut source_offset = 0u64;
    if !transient {
        let s = &saved_source;
        if s.resource.is_null()
            || s.resource as u64 != current.record.resource
            || actual.address < s.gpu
            || actual.address - s.gpu > s.bytes
            || LOGICAL_BYTES > s.bytes - (actual.address - s.gpu)
        {
            return Status::StaleObservation;
        }
        source_offset = actual.address - s.gpu;
    }
    let stale = context(game) != c
        || snapshot(game, &c, saved_native.handle).is_none_or(|again| again != current)
        || cpu_cbv(current.slot.descriptor).is_none_or(|final_cpu| !same_cbv(&final_cpu, &saved_cpu))
        || gpu_cbv(list, root, parameter, offset)
            .is_none_or(|final_gpu| final_gpu.descriptor != actual.descriptor || final_gpu.heap != actual.heap || !same_cbv(&final_gpu, &actual))
        || !{
            let mut check = stamp;
            // SAFETY: a local stamp.
            unsafe { cvr_sda_audit_stamp(1, 0, 0, 0, 0, &mut check, core::ptr::null_mut()) }
        };
    if stale {
        return Status::StaleObservation;
    }
    // The observation still the same one, its buffer referenced for the lease while the lock holds it alive.
    let source = with(|s| match s.observations.get(&(list as usize)) {
        Some(o) if o.serial == saved_serial => {
            let mut source = o.source;
            source.resource = add_ref(o.source.resource);
            Some(source)
        }
        _ => None,
    });
    let Some(source) = source else { return Status::StaleObservation };
    out.native = current;
    out.binding = actual;
    out.stamp = stamp;
    out.source = source;
    out.source_offset = source_offset;
    out.serial = saved_serial;
    if transient {
        Status::Transient
    } else {
        Status::Registered
    }
}

// ---- the C ABI (src/Render/NativeMaterialBinding.cpp's Rust build)

#[no_mangle]
pub extern "C" fn cvr_mbind_compatible(game: u64) -> bool {
    compatible(game)
}
#[no_mangle]
pub extern "C" fn cvr_mbind_reset(list: *mut c_void) {
    reset(list)
}
#[no_mangle]
pub extern "C" fn cvr_mbind_clear() {
    clear()
}
#[no_mangle]
pub extern "C" fn cvr_mbind_observe_bind(game: u64, binding: u32, handle: u32, stage: u32) -> u32 {
    observe_bind(game, binding, handle, stage) as u32
}
/// # Safety
/// `out` is a zeroed material_binding::Lease (no reference held); `list` a live command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mbind_resolve_at_draw(list: *mut c_void, root: u64, parameter: u32, offset: u32, out: *mut Lease) -> u32 {
    // SAFETY: the caller's lease.
    match unsafe { out.as_mut() } {
        Some(out) => resolve_at_draw(list, root, parameter, offset, out) as u32,
        None => Status::Failure as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_are_the_cpp_enum() {
        assert_eq!(Status::Ignored as u32, 0);
        assert_eq!(Status::Registered as u32, 1);
        assert_eq!(Status::Transient as u32, 2);
        assert_eq!(Status::MissingObservation as u32, 8);
        assert_eq!(Status::StaleObservation as u32, 9);
        assert_eq!(Status::Failure as u32, 11);
    }
    #[test]
    fn wrong_register_or_stage_is_ignored() {
        assert_eq!(observe_bind(game(), 3, 1, STAGE_INDEX), Status::Ignored);
        assert_eq!(observe_bind(game(), SHADER_REGISTER, 1, 0), Status::Ignored);
    }
}
