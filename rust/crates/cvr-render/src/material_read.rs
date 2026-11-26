//! The material binding's source reads (formerly src/Render/NativeMaterialRead.cpp): a lease's validation at a draw and
//! at its submission, the snapshot copy into a default-heap buffer (through a buffer read receipt) and the readback copy,
//! which registers the read state the native recording needs (and recovers it when the registration did not hold).
//! The failure codes are the .cpp's: 1 arguments, 2 compatibility, 3 identity, 4 destination, 5 device, 6 ownership,
//! 7 context or state, 8 exception, 10 + the read state's status, 40 / 41 recovery, 42 after the receipt, 100 + the
//! buffer read's failure.

use crate::buffer_read::{Lease as BufferLease, Proof};
use crate::descriptor_audit::{CbvBinding, DescriptorStamp};
use crate::material::{self, Snapshot};
use crate::source_cbv_details::ReadState;
use crate::texture_lease::UploadContext;
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::OnceLock;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// material_binding::LogicalBytes.
const LOGICAL_BYTES: u64 = 292;
/// The native recording's read registration and its flush (the game's code).
const REGISTER_READ: u64 = 0x337DA0;
const FLUSH: u64 = 0x3E3BB4;

/// material_binding::Lease.
#[repr(C)]
pub struct Lease {
    pub native: Snapshot,
    pub binding: CbvBinding,
    pub stamp: DescriptorStamp,
    /// geometry::BufferLease (its ComPtr is the resource pointer).
    pub source: BufferLease,
    pub source_offset: u64,
    pub serial: u64,
    pub read_proof: Proof,
}
const _: () = assert!(
    core::mem::size_of::<Lease>() == 528
        && core::mem::offset_of!(Lease, binding) == 128
        && core::mem::offset_of!(Lease, stamp) == 160
        && core::mem::offset_of!(Lease, source) == 336
        && core::mem::offset_of!(Lease, source_offset) == 416
        && core::mem::offset_of!(Lease, read_proof) == 432
);

extern "C" {
    fn cvr_binding_code(table: u32, base: u64) -> bool;
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
    fn cvr_sda_audit_cbv(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut CbvBinding) -> bool;
    fn cvr_sda_audit_stamp(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut DescriptorStamp, object: *mut c_void) -> bool;
    fn cvr_iab_read_state(list: *mut c_void, source: *const BufferLease, out: *mut ReadState);
    fn cvr_buffer_read_capture(list: *mut c_void, source: *const BufferLease, out: *mut Proof) -> bool;
    fn cvr_buffer_read_validate_range(source: *const BufferLease, proof: *const Proof, com_offset: u64, copy_bytes: u64, queue: *mut c_void) -> bool;
    fn cvr_buffer_read_last_failure() -> u32;
}

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
fn fail(code: u32) {
    FAILURE.with(|f| f.set(code));
}
fn game() -> u64 {
    // SAFETY: the process's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
/// The guarded process read the binding rules take.
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
/// material_binding::Compatible: the game's own image and its binding code (checked once).
fn binding_compatible(game: u64) -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    game != 0 && game == self::game() && *COMPATIBLE.get_or_init(|| unsafe { cvr_binding_code(2, game) })
}
/// buffer_read::Validate: the 292 logical bytes at the native buffer's offset.
fn buffer_valid(source: &BufferLease, proof: &Proof, queue: *mut c_void) -> bool {
    // SAFETY: the lease and the proof by reference; the queue the caller's (or null).
    source.native.bytes >= LOGICAL_BYTES
        && source.native.gpu >= source.gpu
        && unsafe { cvr_buffer_read_validate_range(source, proof, source.native.gpu - source.gpu, LOGICAL_BYTES, queue) }
}
fn read_state(list: *mut c_void, source: &BufferLease) -> ReadState {
    let mut state = ReadState { value: 0, status: 0, kind: 0 };
    // SAFETY: the caller's list and lease.
    unsafe { cvr_iab_read_state(list, source, &mut state) };
    state
}
fn keep(list: *mut c_void, objects: &[*mut c_void]) -> bool {
    // SAFETY: the caller's list and live objects.
    unsafe { cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
}
/// The lease's native binding still current on `list`'s recording.
fn current(game: u64, list: *mut c_void, lease: &Lease) -> bool {
    let c = context(game);
    if !valid(&c) || c != lease.native.context || c.list != list as u64 {
        return false;
    }
    // SAFETY: plain data, filled by the read.
    let mut actual: Snapshot = unsafe { core::mem::zeroed() };
    // SAFETY: our reader over our own process; locals.
    (unsafe { material::cvr_mat_read_snapshot(Some(read), core::ptr::null_mut(), game, &c, lease.native.handle, &mut actual, false) })
        && actual == lease.native
}
/// The lease's CBV descriptor: its stamp, its range in the snapshot, its CPU descriptor and its current value.
fn descriptor(lease: &Lease) -> bool {
    let binding = &lease.binding;
    let stamp = &lease.stamp;
    if !(stamp.gpu == binding.descriptor
        && stamp.heap == binding.heap
        && stamp.value[0] == 1
        && stamp.value[4] == binding.address
        && stamp.value[5] == binding.bytes as u64)
    {
        return false;
    }
    // SAFETY: the lease's snapshot.
    if !unsafe { material::cvr_mat_fits_cbv(&lease.native, binding.address, binding.bytes) } {
        return false;
    }
    let mut cpu = CbvBinding { descriptor: 0, heap: 0, address: 0, bytes: 0 };
    let mut stamp_copy = *stamp;
    // SAFETY: locals; the audit's own lookups.
    unsafe {
        cvr_sda_audit_cbv(2, lease.native.slot.descriptor, 0, 0, 0, &mut cpu)
            && cpu.address == binding.address
            && cpu.bytes == binding.bytes
            && cvr_sda_audit_stamp(1, 0, 0, 0, 0, &mut stamp_copy, core::ptr::null_mut())
    }
}
fn resource<'a>(raw: &'a *mut c_void) -> Option<&'a ID3D12Resource> {
    // SAFETY: a lease's or caller's live resource (or null).
    unsafe { ID3D12Resource::from_raw_borrowed(raw) }
}
/// The source buffer as the lease saw it: a buffer of its size and flags, not UAV, at its address, in its heap type.
fn source_shape(source: &BufferLease, flags_must_be_zero: bool, heap_type: Option<u32>) -> bool {
    let Some(r) = resource(&source.resource) else { return false };
    // SAFETY: the live resource.
    let desc = unsafe { r.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    let flags_ok = if flags_must_be_zero {
        desc.Flags.0 == 0
    } else {
        desc.Flags.0 as u32 == source.flags && (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 == 0
    };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width == source.bytes
        && flags_ok
        // SAFETY: the live resource.
        && unsafe { r.GetGPUVirtualAddress() } == source.gpu
        && unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type.0 as u32 == heap_type.unwrap_or(source.heap_type)
}
fn identity(lease: &Lease) -> bool {
    let source = &lease.source;
    // SAFETY: the lease's snapshot and native buffer.
    (unsafe { material::cvr_mat_native_identity(&lease.native, &source.native) })
        && source.resource as u64 == lease.native.record.resource
        && material::cvr_mat_source_range(lease.binding.address, source.gpu, source.bytes, lease.source_offset)
}
fn submission_record(game: u64, lease: &Lease) -> bool {
    // SAFETY: our reader; the lease's snapshot.
    unsafe { material::cvr_mat_submission_record(Some(read), core::ptr::null_mut(), game, &lease.native, false) }
}
fn same_device(list: &ID3D12GraphicsCommandList, objects: &[&ID3D12Resource]) -> bool {
    let mut a: Option<ID3D12Device> = None;
    // SAFETY: live objects, local outputs.
    if unsafe { list.GetDevice(&mut a) }.is_err() {
        return false;
    }
    let Some(a) = a else { return false };
    objects.iter().all(|o| {
        let mut d: Option<ID3D12Device> = None;
        // SAFETY: as above.
        unsafe { o.GetDevice(&mut d) }.is_ok() && d.is_some_and(|d| d.as_raw() == a.as_raw())
    })
}
fn destination_buffer(destination: &ID3D12Resource, offset: u64, heap_type: D3D12_HEAP_TYPE) -> bool {
    // SAFETY: the live destination.
    let desc = unsafe { destination.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Flags == D3D12_RESOURCE_FLAG_NONE
        && material::cvr_mat_destination_range(desc.Width, offset)
        // SAFETY: the live destination.
        && unsafe { destination.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type == heap_type
}
fn list_of<'a>(raw: &'a *mut c_void) -> Option<&'a ID3D12GraphicsCommandList> {
    // SAFETY: the caller's live list (or null).
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(raw) }
}

/// material_read::Compatible: the read's binding code (checked once).
pub fn compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| unsafe { cvr_binding_code(1, game()) })
}
/// material_read::Validate.
pub fn validate(list: *mut c_void, lease: &Lease) -> bool {
    let game = game();
    if list.is_null() || lease.serial == 0 || lease.source.resource.is_null() || !binding_compatible(game) || !identity(lease)
        || !current(game, list, lease)
        || !descriptor(lease)
    {
        return false;
    }
    source_shape(&lease.source, false, None) && current(game, list, lease) && descriptor(lease)
}
/// material_read::ValidateSubmission.
pub fn validate_submission(lease: &Lease, queue: *mut c_void) -> bool {
    let game = game();
    let source = &lease.source;
    let implicit = material::cvr_mat_needs_buffer_receipt(source.heap_type, lease.native.record.kind);
    if lease.serial == 0 || source.resource.is_null() || !binding_compatible(game) {
        return false;
    }
    // SAFETY: the lease's snapshot and native buffer.
    if !unsafe { material::cvr_mat_native_identity(&lease.native, &source.native) } {
        return false;
    }
    if !implicit && (!material::cvr_mat_submission_heap(source.heap_type, lease.native.record.kind) || lease.read_proof.receipt != 0) {
        return false;
    }
    if implicit && (queue.is_null() || !buffer_valid(source, &lease.read_proof, queue)) {
        return false;
    }
    if source.resource as u64 != lease.native.record.resource
        || !material::cvr_mat_source_range(lease.binding.address, source.gpu, source.bytes, lease.source_offset)
        || !submission_record(game, lease)
        || !descriptor(lease)
        || !source_shape(source, false, None)
    {
        return false;
    }
    submission_record(game, lease) && descriptor(lease) && (!implicit || buffer_valid(source, &lease.read_proof, queue))
}
/// SnapshotIdentity: a default-heap tracked buffer (kind 1) with no flags, still the registry's.
fn snapshot_identity(lease: &Lease) -> bool {
    let game = game();
    let source = &lease.source;
    if source.resource.is_null() || source.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32 || source.flags != 0 || lease.native.record.kind != 1
        || !identity(lease)
        || !submission_record(game, lease)
    {
        return false;
    }
    source_shape(source, true, Some(D3D12_HEAP_TYPE_DEFAULT.0 as u32))
}
/// material_read::CopySnapshot: the 292 bytes into a default-heap buffer at `offset`, through a buffer read receipt.
pub fn copy_snapshot(list_raw: *mut c_void, lease: &Lease, destination_raw: *mut c_void, offset: u64, proof: &mut Proof) -> bool {
    // SAFETY: plain data.
    *proof = unsafe { core::mem::zeroed() };
    fail(1);
    let (Some(list), Some(destination)) = (list_of(&list_raw), resource(&destination_raw)) else { return false };
    // SAFETY: the live list.
    if destination_raw == lease.source.resource || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    fail(2);
    if !compatible() {
        return false;
    }
    fail(3);
    if !snapshot_identity(lease) {
        return false;
    }
    fail(4);
    if !destination_buffer(destination, offset, D3D12_HEAP_TYPE_DEFAULT) {
        return false;
    }
    fail(5);
    let Some(source_resource) = resource(&lease.source.resource) else { return false };
    if !same_device(list, &[destination, source_resource]) {
        return false;
    }
    let c = context(game());
    fail(7);
    if !valid(&c) || c.list != list_raw as u64 {
        return false;
    }
    let mut source = lease.source;
    source.native.context = c.context;
    fail(6);
    if !keep(list_raw, &[lease.source.resource, destination_raw]) {
        return false;
    }
    // SAFETY: the list, the lease's source and our proof.
    if !unsafe { cvr_buffer_read_capture(list_raw, &source, proof) } {
        fail(100 + unsafe { cvr_buffer_read_last_failure() });
        return false;
    }
    fail(42);
    if !snapshot_identity(lease) || !buffer_valid(&source, proof, core::ptr::null_mut()) {
        return false;
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the live list and buffers, the checked ranges.
    unsafe { list.CopyBufferRegion(destination, offset, source_resource, lease.source_offset, LOGICAL_BYTES) };
    fail(0);
    true
}
/// material_read::ValidateSnapshotSubmission.
pub fn validate_snapshot_submission(lease: &Lease, proof: &Proof, queue: *mut c_void) -> bool {
    !queue.is_null() && snapshot_identity(lease) && buffer_valid(&lease.source, proof, queue)
}
type RegisterRead = unsafe extern "C" fn(*mut c_void, u32, u32);
type Flush = unsafe extern "C" fn(*mut c_void);
/// material_read::Copy: the 292 bytes into a readback buffer at `offset`; a buffer the native recording tracks gets its
/// read state registered first (and recovered when the registration did not hold).
pub fn copy(list_raw: *mut c_void, lease: &mut Lease, destination_raw: *mut c_void, offset: u64) -> bool {
    // SAFETY: plain data.
    lease.read_proof = unsafe { core::mem::zeroed() };
    fail(1);
    let (Some(list), Some(destination)) = (list_of(&list_raw), resource(&destination_raw)) else { return false };
    // SAFETY: the live list.
    if lease.source.resource.is_null() || destination_raw == lease.source.resource || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    fail(2);
    if !compatible() {
        return false;
    }
    fail(3);
    if !validate(list_raw, lease) {
        return false;
    }
    fail(4);
    if !destination_buffer(destination, offset, D3D12_HEAP_TYPE_READBACK) {
        return false;
    }
    fail(5);
    let source_raw = lease.source.resource;
    let Some(source_resource) = resource(&source_raw) else { return false };
    if !same_device(list, &[destination, source_resource]) {
        return false;
    }
    let kind = lease.native.record.kind;
    let source_heap = lease.source.heap_type;
    if material::cvr_mat_needs_buffer_receipt(source_heap, kind) {
        fail(6);
        if !keep(list_raw, &[source_raw, destination_raw]) {
            return false;
        }
        let source = lease.source;
        // SAFETY: the list, the lease's source and its proof.
        if !unsafe { cvr_buffer_read_capture(list_raw, &source, &mut lease.read_proof) } {
            fail(100 + unsafe { cvr_buffer_read_last_failure() });
            return false;
        }
        fail(42);
        if !validate(list_raw, lease) || !buffer_valid(&lease.source, &lease.read_proof, core::ptr::null_mut()) {
            return false;
        }
        // SAFETY: the live list and buffers, the checked ranges.
        unsafe { list.CopyBufferRegion(destination, offset, source_resource, lease.source_offset, LOGICAL_BYTES) };
        fail(0);
        return true;
    }
    let before = read_state(list_raw, &lease.source);
    let mut required = 0u32;
    fail(10 + before.status);
    // SAFETY: locals.
    if !unsafe { material::cvr_mat_copy_read_mask(&before, source_heap, kind, &mut required) } {
        return false;
    }
    fail(6);
    if !keep(list_raw, &[source_raw, destination_raw]) {
        return false;
    }
    let game = game();
    let native_context = lease.native.context.context as *mut c_void;
    // SAFETY: the game's own registration and flush, at the addresses its build check vouched for.
    let register_read: RegisterRead = unsafe { core::mem::transmute::<u64, RegisterRead>(game + REGISTER_READ) };
    let flush: Flush = unsafe { core::mem::transmute::<u64, Flush>(game + FLUSH) };
    let _internal = cvr_host::probe::Internal::new();
    let has_read = |state: &ReadState, required: u32| unsafe { material::cvr_mat_has_read_state(state, source_heap, kind, required) };
    let mut mutated = false;
    let recover = |lease: &Lease| -> bool {
        fail(40);
        if !validate(list_raw, lease) {
            return false;
        }
        let mut state = read_state(list_raw, &lease.source);
        if state.status == 4 {
            // SAFETY: the recording's own context.
            unsafe { flush(native_context) };
            state = read_state(list_raw, &lease.source);
            if !validate(list_raw, lease) {
                return false;
            }
        }
        let mut recovery = 0u32;
        // SAFETY: locals.
        if !unsafe { material::cvr_mat_recovery_read_mask(&state, kind, before.value, &mut recovery) } {
            return false;
        }
        if state.value & recovery != recovery {
            // SAFETY: as above.
            unsafe {
                register_read(native_context, lease.native.handle, recovery);
                flush(native_context);
            }
            state = read_state(list_raw, &lease.source);
        }
        if validate(list_raw, lease) && has_read(&state, recovery) {
            fail(41);
        }
        false
    };
    fail(7);
    if !validate(list_raw, lease) {
        return false;
    }
    let now = read_state(list_raw, &lease.source);
    let mut current_required = 0u32;
    // SAFETY: locals.
    if !unsafe { material::cvr_mat_copy_read_mask(&now, source_heap, kind, &mut current_required) } || now.value != before.value {
        return false;
    }
    if !has_read(&now, required) {
        mutated = true;
        fail(40);
        // SAFETY: as above.
        unsafe {
            register_read(native_context, lease.native.handle, required);
            flush(native_context);
        }
    }
    let ready = read_state(list_raw, &lease.source);
    if !validate(list_raw, lease) || !has_read(&ready, required) {
        if mutated {
            return recover(lease);
        }
        fail(7);
        return false;
    }
    // SAFETY: the live list and buffers, the checked ranges.
    unsafe { list.CopyBufferRegion(destination, offset, source_resource, lease.source_offset, LOGICAL_BYTES) };
    fail(0);
    true
}

// ---- the C ABI (src/Render/NativeMaterialRead.cpp's Rust build)

#[no_mangle]
pub extern "C" fn cvr_mread_last_failure() -> u32 {
    FAILURE.with(|f| f.get())
}
#[no_mangle]
pub extern "C" fn cvr_mread_compatible() -> bool {
    compatible()
}
/// # Safety
/// `lease` is a material_binding::Lease; `list` a live command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mread_validate(list: *mut c_void, lease: *const Lease) -> bool {
    // SAFETY: the caller's lease.
    unsafe { lease.as_ref() }.is_some_and(|l| validate(list, l))
}
/// # Safety
/// As cvr_mread_validate; `queue` a live queue or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mread_validate_submission(lease: *const Lease, queue: *mut c_void) -> bool {
    // SAFETY: the caller's lease.
    unsafe { lease.as_ref() }.is_some_and(|l| validate_submission(l, queue))
}
/// # Safety
/// As cvr_mread_validate; `destination` a live resource or null; `proof` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_mread_copy_snapshot(list: *mut c_void, lease: *const Lease, destination: *mut c_void, offset: u64, proof: *mut Proof) -> bool {
    // SAFETY: the caller's lease and proof.
    match unsafe { (lease.as_ref(), proof.as_mut()) } {
        (Some(l), Some(p)) => copy_snapshot(list, l, destination, offset, p),
        _ => false,
    }
}
/// # Safety
/// As cvr_mread_validate_submission; `proof` readable.
#[no_mangle]
pub unsafe extern "C" fn cvr_mread_validate_snapshot_submission(lease: *const Lease, proof: *const Proof, queue: *mut c_void) -> bool {
    // SAFETY: the caller's lease and proof.
    match unsafe { (lease.as_ref(), proof.as_ref()) } {
        (Some(l), Some(p)) => validate_snapshot_submission(l, p, queue),
        _ => false,
    }
}
/// # Safety
/// As cvr_mread_validate, the lease writable (its read proof is replaced).
#[no_mangle]
pub unsafe extern "C" fn cvr_mread_copy(list: *mut c_void, lease: *mut Lease, destination: *mut c_void, offset: u64) -> bool {
    // SAFETY: the caller's lease.
    unsafe { lease.as_mut() }.is_some_and(|l| copy(list, l, destination, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_arguments_fail_with_the_cpp_codes() {
        // SAFETY: plain data.
        let mut lease: Lease = unsafe { core::mem::zeroed() };
        let mut proof: Proof = unsafe { core::mem::zeroed() };
        assert!(!copy_snapshot(core::ptr::null_mut(), &lease, core::ptr::null_mut(), 0, &mut proof));
        assert_eq!(cvr_mread_last_failure(), 1);
        assert!(!copy(core::ptr::null_mut(), &mut lease, core::ptr::null_mut(), 0));
        assert_eq!(cvr_mread_last_failure(), 1);
        assert!(!validate(core::ptr::null_mut(), &lease));
        assert!(!validate_submission(&lease, core::ptr::null_mut()));
        assert!(!validate_snapshot_submission(&lease, &proof, core::ptr::null_mut()));
    }
    #[test]
    fn descriptor_needs_the_stamped_binding() {
        // SAFETY: plain data.
        let mut lease: Lease = unsafe { core::mem::zeroed() };
        lease.binding = CbvBinding { descriptor: 7, heap: 9, address: 0x1000, bytes: 512 };
        lease.stamp.gpu = 7;
        lease.stamp.heap = 9;
        lease.stamp.value[0] = 1;
        lease.stamp.value[4] = 0x1000;
        lease.stamp.value[5] = 256; // the stamped size differs from the binding's
        assert!(!descriptor(&lease));
    }
}
