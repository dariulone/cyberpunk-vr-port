//! THE SOURCE CBV BINDING'S OBSERVER (formerly src/Render/NativeSourceCbvBinding.cpp): the engine's CBV binds observed
//! per command list, generation, stage and register (their native record, CPU slot and view, the buffer leased), the
//! origins the plugin stamps on the CBVs the engine creates (a COM owner with the resource, its cookie and creation
//! facts), the per-thread scopes of the pooled upload writer joined to the creation they fill, and the read at a native
//! draw that turns a descriptor stamp back into its backing (the resource, the native buffer, the recordings, the
//! writer's completed range). The rules each step applies are source_cbv_binding.rs'; the game reads source_cbv_reads.rs'.
//!
//! The C++ API (include/Render/NativeSourceCbvBinding.hpp) forwards here.
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::collections::BTreeMap;
use std::sync::Mutex;

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12DeviceChild, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_CONSTANT_BUFFER_VIEW_DESC, D3D12_HEAP_FLAGS,
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_GPU_UPLOAD, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_DIMENSION_BUFFER,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::buffer_read::NativeBuffer;
use crate::descriptor_audit::CbvBinding;
use crate::native_recording::{Context, Lease};
use crate::source_cbv_binding::{
    decode_creation, descriptor, direct_creation, direct_producer, read_slot, same_buffer, same_origin, same_source_slot, same_view, select_slot,
    track_stage_mask, Creation, DescriptorStamp, Slot, UploadWriterCompletion, UploadWriterContext, UploadWriterJoin, UploadWriterLedger,
    UploadWriterStamp, DIRECT_CREATOR_RVA, MAX_UPLOAD_WRITER_DEPTH, POOLED_CREATOR_RVA, UPLOAD_WRITER_MEMCPY_RETURN_RVA,
};

/// Whether the observer runs (the tools set it by name).
#[no_mangle]
pub static CyberpunkVR_SourceCbvBindingEnable: AtomicU32 = AtomicU32::new(0);

/// source_cbv::Status.
mod status {
    pub const IGNORED: u32 = 0;
    pub const REGISTERED: u32 = 1;
    pub const TRANSIENT: u32 = 2;
    pub const CREATED: u32 = 3;
    pub const UNSUPPORTED_EXECUTABLE: u32 = 4;
    pub const MISSING_CONTEXT: u32 = 5;
    pub const INVALID_BINDING: u32 = 6;
    pub const MISSING_OBSERVATION: u32 = 7;
    pub const UNKNOWN_CREATOR: u32 = 8;
    pub const UNKNOWN_ORIGIN: u32 = 9;
    pub const CHANGED: u32 = 10;
    pub const DESCRIPTOR_MISMATCH: u32 = 11;
    pub const UNSUPPORTED_RESOURCE: u32 = 12;
    pub const CAPACITY: u32 = 13;
    pub const FAILURE: u32 = 14;
}
/// source_cbv::Phase.
mod phase {
    pub const CODE: u64 = 1;
    pub const CONTEXT: u64 = 2;
    pub const SLOT: u64 = 3;
    pub const STAMP: u64 = 4;
    pub const BINDING: u64 = 5;
    pub const CPU_VIEW: u64 = 6;
    pub const OBSERVATION: u64 = 7;
    pub const ORIGIN: u64 = 8;
    pub const NATIVE: u64 = 9;
    pub const RESOURCE: u64 = 10;
    pub const REVALIDATE: u64 = 11;
    pub const SERIAL: u64 = 12;
    pub const COMPLETE: u64 = 13;
}
const ORIGIN_TYPE: GUID = GUID::from_values(0x178a32ad, 0x2113, 0x4264, [0x9e, 0xc1, 0xb9, 0x34, 0x66, 0x80, 0x21, 0x0f]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const CAPACITY: usize = 8192;
const CREATION_ADDRESS_CALLER: u64 = 0x33468F;
const WRITER_OPEN: u32 = 0;
const WRITER_COMPLETED: u32 = 1;
const WRITER_CANCELLED: u32 = 2;

fn read(address: u64, out: &mut [u8]) -> bool {
    crate::geometry_packets::read(address, out)
}
fn base() -> u64 {
    // SAFETY: the game's own module.
    unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64)
}
fn thread() -> u64 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() as u64 }
}
fn field_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn field_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn valid(context: &Context) -> bool {
    context.context != 0 && context.state != 0 && context.list != 0
}

// ---- references

fn add_ref(object: *mut c_void) -> *mut c_void {
    // SAFETY: a live COM object or null.
    if let Some(o) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        core::mem::forget(o.clone());
    }
    object
}
fn release(object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: a reference this module owns, released once.
        drop(unsafe { IUnknown::from_raw(object) });
    }
}
/// A native_recording::Lease whose token reference is owned (the C++ Lease's ComPtr).
#[repr(transparent)]
struct OwnedLease(Lease);
impl OwnedLease {
    fn empty() -> Self {
        OwnedLease(Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 })
    }
    fn raw(&self) -> &Lease {
        &self.0
    }
    /// The lease, its reference handed over (into a C++ Lease).
    fn into_raw(self) -> Lease {
        let lease = Lease { ..self.0 };
        core::mem::forget(self);
        lease
    }
}
// SAFETY: the token is a free-threaded COM object (the C++ held it in a ComPtr shared across threads).
unsafe impl Send for OwnedLease {}
impl Clone for OwnedLease {
    fn clone(&self) -> Self {
        add_ref(self.0.token);
        OwnedLease(Lease { ..self.0 })
    }
}
impl Drop for OwnedLease {
    fn drop(&mut self) {
        release(self.0.token);
    }
}
/// native_recording::Capture.
fn capture(list: u64, context: &Context) -> Option<OwnedLease> {
    let mut lease = OwnedLease::empty();
    // SAFETY: the context's list (live while its context is current); `lease` an empty lease the capture fills.
    unsafe { crate::native_recording::cvr_native_recording_capture(list as *mut c_void, context, &mut lease.0) }.then_some(lease)
}
fn validate_recording(recording: &OwnedLease, list: u64, context: &Context) -> bool {
    // SAFETY: a valid lease and context.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording.raw(), list as *mut c_void, context) }
}

// ---- the game's state

fn tls() -> u64 {
    let tls: u64;
    // SAFETY: the thread's TEB slot array pointer.
    unsafe { core::arch::asm!("mov {}, gs:[0x58]", out(reg) tls, options(nostack, readonly, preserves_flags)) };
    tls
}
/// Current: the engine's current upload context on this thread.
fn current() -> Context {
    match crate::source_cbv_reads::current(&read, tls()) {
        Some((context, state, list)) => Context { context, state, list },
        None => Context::default(),
    }
}
fn active() -> bool {
    CyberpunkVR_SourceCbvBindingEnable.load(Relaxed) != 0 && !cvr_host::probe::internal() && crate::stereo_probe::cvr_observe_bindings()
}
static CODE: Mutex<(bool, bool, u64)> = Mutex::new((false, false, 0));
/// Compatible: the code guards prepared for this image.
fn compatible() -> bool {
    let code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    code.1 && code.2 == base()
}
fn same_device(expected: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: a live device child or null.
    let Some(object) = (unsafe { ID3D12DeviceChild::from_raw_borrowed(&object) }) else {
        return false;
    };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live object; the out-parameter is ours.
    let located = unsafe { object.GetDevice(&mut actual) }.is_ok();
    located && actual.is_some_and(|d| d.as_raw() == expected)
}
fn list_device(list: u64) -> Option<ID3D12Device> {
    let raw = list as *mut c_void;
    // SAFETY: the context's live list.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list; the out-parameter is ours.
    unsafe { list.GetDevice(&mut device) }.ok()?;
    device
}

// ---- the writer receipts and origins (COM owners)

/// The pooled upload writer's receipt: its recording, the creation it joined and its completion.
#[repr(C)]
struct WriterReceipt {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    stage: AtomicU32,
    recording: OwnedLease,
    progress: Mutex<(UploadWriterJoin, UploadWriterCompletion)>,
}
static WRITER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: writer_query, AddRef: writer_add_ref, Release: writer_release };
unsafe extern "system" fn writer_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live receipt.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut WriterReceipt)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn writer_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live receipt.
    unsafe { (*(this as *mut WriterReceipt)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn writer_release(this: *mut c_void) -> u32 {
    // SAFETY: a live receipt; the last reference frees it.
    unsafe {
        let n = (*(this as *mut WriterReceipt)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut WriterReceipt));
        }
        n
    }
}
/// A counted reference to a writer receipt (ComPtr<WriterReceipt>).
struct Writer(*mut WriterReceipt);
impl Writer {
    fn get(&self) -> &WriterReceipt {
        // SAFETY: a live receipt this handle references.
        unsafe { &*self.0 }
    }
}
impl Clone for Writer {
    fn clone(&self) -> Self {
        // SAFETY: a live receipt.
        unsafe { writer_add_ref(self.0.cast()) };
        Writer(self.0)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: the reference this handle holds.
        unsafe { writer_release(self.0.cast()) };
    }
}

/// A CBV's origin: the resource it views, its cookie and identity, the creation facts.
#[repr(C)]
struct Origin {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    resource: Option<ID3D12Resource>,
    id: u64,
    cookie: u64,
    gpu: u64,
    bytes: u64,
    offset: u64,
    cpu: u64,
    view_bytes: u32,
    caller: u32,
    logical_bytes: u32,
    reserved_bytes: u32,
    completed_producer_rva: u32,
    creation_recording: OwnedLease,
    writer: Option<Writer>,
    native: NativeBuffer,
}
static ORIGIN_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: origin_query, AddRef: origin_add_ref, Release: origin_release };
unsafe extern "system" fn origin_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live origin.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != ORIGIN_TYPE) {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Origin)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn origin_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live origin.
    unsafe { (*(this as *mut Origin)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn origin_release(this: *mut c_void) -> u32 {
    // SAFETY: a live origin; the last reference frees it.
    unsafe {
        let n = (*(this as *mut Origin)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Origin));
        }
        n
    }
}
/// A counted reference to an origin (ComPtr<Origin>).
struct OriginRef(*mut Origin);
impl OriginRef {
    fn get(&self) -> &Origin {
        // SAFETY: a live origin this handle references.
        unsafe { &*self.0 }
    }
}
impl Drop for OriginRef {
    fn drop(&mut self) {
        // SAFETY: the reference this handle holds.
        unsafe { origin_release(self.0.cast()) };
    }
}
/// GetOrigin: `value` as this module's origin.
fn get_origin(value: *mut c_void) -> Option<OriginRef> {
    if value.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object; QueryInterface through its first slot.
    unsafe {
        let vtbl = *(value as *const *const IUnknown_Vtbl);
        let _ = ((*vtbl).QueryInterface)(value, &ORIGIN_TYPE, &mut out);
    }
    (!out.is_null()).then(|| OriginRef(out.cast()))
}

/// descriptors::CbvOrigin: the owner an owned reference.
#[repr(C)]
pub struct RawOrigin {
    pub cookie: u64,
    pub id: u64,
    pub owner: *mut c_void,
}
impl RawOrigin {
    fn none() -> Self {
        RawOrigin { cookie: 0, id: 0, owner: core::ptr::null_mut() }
    }
}
impl Drop for RawOrigin {
    fn drop(&mut self) {
        release(self.owner);
    }
}
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
/// MakeOrigin.
fn make_origin(
    resource: &ID3D12Resource,
    view: &D3D12_CONSTANT_BUFFER_VIEW_DESC,
    cpu: u64,
    caller: u32,
    native: NativeBuffer,
    creation: Option<&Creation>,
    recording: Option<&OwnedLease>,
) -> (RawOrigin, *mut Origin) {
    let none = (RawOrigin::none(), core::ptr::null_mut());
    // SAFETY: a live resource.
    let (desc, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || !descriptor(gpu, desc.Width, view.BufferLocation, view.SizeInBytes) {
        return none;
    }
    if caller == DIRECT_CREATOR_RVA
        && !creation.is_some_and(|c| {
            direct_producer(c, view.SizeInBytes)
                && c.resource == resource.as_raw() as u64
                && c.offset == view.BufferLocation.wrapping_sub(gpu)
                && c.cpu == cpu
        })
    {
        return none;
    }
    let cookie = cookie_at_creation(resource.as_raw());
    if cookie == 0 {
        return none;
    }
    let mut id = NEXT_ID.load(Relaxed);
    loop {
        if id == u64::MAX {
            return none;
        }
        match NEXT_ID.compare_exchange_weak(id, id + 1, Relaxed, Relaxed) {
            Ok(_) => break,
            Err(actual) => id = actual,
        }
    }
    let direct = creation.filter(|_| caller == DIRECT_CREATOR_RVA);
    let owner = Box::into_raw(Box::new(Origin {
        vtbl: &ORIGIN_VTBL,
        refs: AtomicU32::new(1),
        resource: Some(resource.clone()),
        id: id + 1,
        cookie,
        gpu,
        bytes: desc.Width,
        offset: view.BufferLocation.wrapping_sub(gpu),
        cpu,
        view_bytes: view.SizeInBytes,
        caller,
        logical_bytes: direct.map_or(0, |c| c.logical_bytes),
        reserved_bytes: direct.map_or(0, |c| c.reserved_bytes),
        completed_producer_rva: direct.map_or(0, |c| c.completed_producer_rva),
        creation_recording: recording.cloned().unwrap_or_else(OwnedLease::empty),
        writer: None,
        native,
    }));
    (RawOrigin { cookie, id: id + 1, owner: owner.cast() }, owner)
}
fn cookie_at_creation(resource: *mut c_void) -> u64 {
    // SAFETY: a live resource the caller holds.
    unsafe { crate::descriptor_audit::cvr_sda_audit_write(8, core::ptr::null_mut(), 0, 0, 0, 0, resource, core::ptr::null_mut()) }
}
fn resolve_cpu_cbv(descriptor: u64) -> Option<CbvBinding> {
    // SAFETY: an all-zero CbvBinding is valid.
    let mut out: CbvBinding = unsafe { core::mem::zeroed() };
    // SAFETY: our output.
    unsafe { crate::descriptor_audit::cvr_sda_audit_cbv(2, descriptor, 0, 0, 0, &mut out) }.then_some(out)
}
fn resolve_cbv(list: u64, root: u64, parameter: u32, offset: u32) -> Option<CbvBinding> {
    // SAFETY: an all-zero CbvBinding is valid.
    let mut out: CbvBinding = unsafe { core::mem::zeroed() };
    // SAFETY: our output.
    unsafe { crate::descriptor_audit::cvr_sda_audit_cbv(0, list, root, parameter, offset, &mut out) }.then_some(out)
}
fn matches_descriptor(stamp: &DescriptorStamp) -> bool {
    let mut copy = *stamp;
    // SAFETY: descriptor_audit's DescriptorStamp has this layout (176 bytes, gpu, heap, value).
    unsafe {
        crate::descriptor_audit::cvr_sda_audit_stamp(1, 0, 0, 0, 0, (&mut copy as *mut DescriptorStamp).cast(), core::ptr::null_mut())
    }
}

// ---- the observations

#[derive(Clone)]
struct Observation {
    context: Context,
    recording: OwnedLease,
    slot: Slot,
    native: NativeBuffer,
    resource: Option<ID3D12Resource>,
    serial: u64,
    address: u64,
    view_bytes: u32,
    logical_bytes: u32,
    kind: u32,
    stage: u32,
    reg: u32,
}
impl Default for Observation {
    fn default() -> Self {
        Observation {
            context: Context::default(),
            recording: OwnedLease::empty(),
            slot: Slot::default(),
            native: NativeBuffer::default(),
            resource: None,
            serial: 0,
            address: 0,
            view_bytes: 0,
            logical_bytes: 0,
            kind: 0,
            stage: 0,
            reg: 0,
        }
    }
}
type Key = (u64, u64, u32, u32);
struct Observations {
    map: BTreeMap<Key, Observation>,
    next_serial: u64,
}
static OBSERVATIONS: Mutex<Observations> = Mutex::new(Observations { map: BTreeMap::new(), next_serial: 0 });
fn observations() -> std::sync::MutexGuard<'static, Observations> {
    OBSERVATIONS.lock().unwrap_or_else(|e| e.into_inner())
}

struct AddressHint {
    resource: Option<ID3D12Resource>,
    gpu: u64,
    context: Context,
}
thread_local! {
    static LAST: Cell<u32> = const { Cell::new(0) };
    static DIAGNOSTIC: Cell<[u64; 16]> = const { Cell::new([0; 16]) };
    static WRITERS: RefCell<UploadWriterLedger> = RefCell::new(UploadWriterLedger::default());
    static WRITER_OWNERS: RefCell<[Option<Writer>; MAX_UPLOAD_WRITER_DEPTH]> = const { RefCell::new([const { None }; MAX_UPLOAD_WRITER_DEPTH]) };
    static ADDRESS_HINT: RefCell<AddressHint> = const { RefCell::new(AddressHint { resource: None, gpu: 0, context: Context { context: 0, state: 0, list: 0 } }) };
}
fn set_last(value: u32) {
    LAST.set(value);
}
fn diagnose(index: usize, value: u64) {
    let mut d = DIAGNOSTIC.get();
    d[index] = value;
    DIAGNOSTIC.set(d);
}
fn at(phase: u64) {
    diagnose(0, phase);
}
fn writer_context(recording: &Lease) -> UploadWriterContext {
    UploadWriterContext {
        thread: thread(),
        token: recording.token as u64,
        list: recording.list,
        generation: recording.generation,
        context: recording.context,
        registry: recording.registry,
    }
}

/// Native: the bind's native record, its CPU slot and buffer.
fn native(context: &Context, handle: u32, stage: u32, reg: u32, require_current: bool) -> Option<Observation> {
    if !valid(context) || handle == 0 || handle > 0x100000 || stage > 1 || reg > 13 {
        return None;
    }
    let (mut registry, mut address) = (0u64, 0u64);
    let mut record = [0u8; 0xB0];
    // SAFETY: our outputs.
    if !unsafe { crate::source_cbv_reads::cvr_scb_record(base(), handle, &mut record, &mut registry, &mut address) } {
        return None;
    }
    let mut value = Observation {
        context: *context,
        logical_bytes: field_u32(&record, 0),
        kind: (record[6] >> 4) as u32,
        stage,
        reg,
        ..Default::default()
    };
    if value.logical_bytes == 0 {
        return None;
    }
    let descriptor = crate::source_cbv_reads::cpu_descriptor(&read, context.context, handle, value.kind, address)?;
    let mut reader = |a: u64, out: &mut [u8]| read(a, out);
    value.slot = select_slot(Some(&mut reader), context.state, stage, reg, descriptor, require_current)?;
    if value.kind != 4 {
        value.native = NativeBuffer {
            resource: field_u64(&record, 0x10),
            gpu: field_u64(&record, 8),
            bytes: value.logical_bytes as u64,
            registry,
            context: context.context,
            handle,
            kind: value.kind,
        };
        if value.native.resource == 0 || value.native.gpu == 0 || value.native.gpu > u64::MAX - value.native.bytes {
            return None;
        }
    } else {
        value.native = NativeBuffer { bytes: value.logical_bytes as u64, registry, context: context.context, handle, kind: 4, ..Default::default() };
    }
    Some(value)
}
fn same_native(a: &Observation, b: &Observation) -> bool {
    a.context == b.context
        && a.slot == b.slot
        && a.kind == b.kind
        && a.logical_bytes == b.logical_bytes
        && a.stage == b.stage
        && a.reg == b.reg
        && same_buffer(&a.native, &b.native)
}
fn same_source_native(a: &Observation, b: &Observation) -> bool {
    a.context == b.context
        && same_source_slot(&a.slot, &b.slot)
        && a.kind == b.kind
        && a.logical_bytes == b.logical_bytes
        && a.stage == b.stage
        && a.reg == b.reg
        && same_buffer(&a.native, &b.native)
}

/// source_cbv::Annotation: the origin's owner an owned reference.
#[repr(C)]
pub struct Annotation {
    pub cpu: u64,
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub origin: RawOrigin,
}
const _: () = assert!(core::mem::size_of::<Annotation>() == 48 && core::mem::offset_of!(Annotation, origin) == 24);

/// Bind: the bind observed in each tracked stage; at most one annotation per stage.
fn bind(reg: u32, handle: u32, stages: u32, out: &mut Vec<Annotation>) {
    set_last(status::IGNORED);
    if !active() {
        return;
    }
    set_last(status::UNSUPPORTED_EXECUTABLE);
    if !compatible() {
        return;
    }
    set_last(status::INVALID_BINDING);
    let (accepted, tracked) = track_stage_mask(stages);
    if reg > 13 || !accepted {
        return;
    }
    let context = current();
    set_last(status::MISSING_CONTEXT);
    if !valid(&context) {
        return;
    }
    let Some(recording) = capture(context.list, &context) else {
        return;
    };
    for stage in 0..2u32 {
        if tracked & (1 << stage) == 0 {
            continue;
        }
        let key: Key = (context.list, recording.raw().generation, stage, reg);
        let retired = observations().map.remove(&key);
        drop(retired);
        set_last(status::INVALID_BINDING);
        let Some(mut value) = native(&context, handle, stage, reg, false) else {
            continue;
        };
        value.recording = recording.clone();
        let mut annotation = Annotation { cpu: value.slot.descriptor, view: D3D12_CONSTANT_BUFFER_VIEW_DESC::default(), origin: RawOrigin::none() };
        if value.kind != 4 {
            set_last(status::UNSUPPORTED_RESOURCE);
            let Some(cpu) = resolve_cpu_cbv(value.slot.descriptor) else {
                continue;
            };
            value.address = cpu.address;
            value.view_bytes = cpu.bytes;
            annotation.view = D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: cpu.address, SizeInBytes: cpu.bytes };
            if value.native.gpu != cpu.address || value.logical_bytes > cpu.bytes {
                continue;
            }
            // SAFETY: an all-zero BufferLease is the empty lease.
            let mut lease: crate::ia_binding::OwnedLease = unsafe { core::mem::zeroed() };
            // SAFETY: our output (an empty lease).
            unsafe { crate::ia_binding::cvr_iab_acquire(&value.native, cpu.address, value.logical_bytes as u64, &mut lease) };
            // SAFETY: the lease's owned reference moves here.
            let resource = (!lease.resource.is_null()).then(|| unsafe { ID3D12Resource::from_raw(lease.resource) });
            let Some(resource) = resource else {
                continue;
            };
            if !descriptor(lease.gpu, lease.bytes, cpu.address, cpu.bytes) {
                continue;
            }
            let Some(device) = list_device(context.list) else {
                continue;
            };
            if !same_device(device.as_raw(), resource.as_raw()) {
                continue;
            }
            annotation.origin = make_origin(&resource, &annotation.view, annotation.cpu, 0, NativeBuffer::default(), None, None).0;
            value.resource = Some(resource);
            if annotation.origin.owner.is_null() {
                continue;
            }
        }
        set_last(status::CHANGED);
        if current() != context
            || !native(&context, handle, stage, reg, false).is_some_and(|again| same_native(&value, &again))
            || !validate_recording(&recording, context.list, &context)
        {
            continue;
        }
        {
            let mut o = observations();
            set_last(status::CAPACITY);
            if o.map.len() >= CAPACITY || o.next_serial == u64::MAX {
                continue;
            }
            o.next_serial += 1;
            value.serial = o.next_serial;
            if o.map.contains_key(&key) {
                set_last(status::CHANGED);
                continue;
            }
            o.map.insert(key, value.clone());
        }
        let transient = value.kind == 4;
        if !annotation.origin.owner.is_null() {
            out.push(annotation);
        }
        set_last(if transient { status::TRANSIENT } else { status::REGISTERED });
    }
}
fn guarded(f: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        set_last(status::FAILURE);
    }
}

/// ObserveBindMask (`which` 0: `stages` a mask) or ObserveBindStage (1: `stages` a stage index): up to two annotations
/// into `out`, their count returned.
///
/// # Safety
/// `out` holds two annotations' room.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_bind(which: u32, reg: u32, handle: u32, stages: u32, out: *mut Annotation) -> u32 {
    let mut found = Vec::with_capacity(2);
    guarded(|| {
        if which == 1 {
            if stages > 1 {
                set_last(status::INVALID_BINDING);
                return;
            }
            bind(reg, handle, 1 << stages, &mut found);
        } else {
            bind(reg, handle, stages, &mut found);
        }
    });
    let count = found.len().min(2);
    for (i, annotation) in found.into_iter().take(2).enumerate() {
        // SAFETY: as documented; the owner reference moves to the caller.
        unsafe { out.add(i).write(annotation) };
    }
    count as u32
}

/// PrepareCodeGuards: the creation and binding code (code_tables.rs table 2), checked once.
#[no_mangle]
pub extern "C-unwind" fn cvr_scbo_prepare_code_guards(game: u64) {
    let mut code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    if code.0 {
        return;
    }
    code.0 = true;
    let verified = game != 0 && game == base() && crate::code_tables::cvr_code_table(2, game);
    code.2 = game;
    code.1 = verified;
}
#[no_mangle]
pub extern "C-unwind" fn cvr_scbo_compatible() -> bool {
    compatible()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_scbo_last_status() -> u32 {
    LAST.get()
}
/// LastDiagnostic: the values, the status in the second.
///
/// # Safety
/// `out` holds 16 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_last_diagnostic(out: *mut [u64; 16]) {
    let mut d = DIAGNOSTIC.get();
    d[1] = LAST.get() as u64;
    // SAFETY: as documented.
    unsafe { *out = d };
}

// ---- the upload writer's scopes

/// UploadWriterScope's constructor: `stamp` the scope's stamp (zero when untracked).
///
/// # Safety
/// `stamp` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_writer_enter(handle: u32, bytes: u32, stamp: *mut UploadWriterStamp) {
    // SAFETY: as documented.
    let stamp = unsafe { &mut *stamp };
    WRITERS.with_borrow_mut(|w| {
        if w.depth != 0 {
            w.invalidate();
        }
    });
    let entered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !active() || !compatible() {
            return;
        }
        let context = current();
        if !valid(&context) {
            return;
        }
        let Some(recording) = capture(context.list, &context) else {
            return;
        };
        *stamp = WRITERS.with_borrow_mut(|w| w.enter(&writer_context(recording.raw()), handle, bytes));
        if stamp.nonce == 0 {
            return;
        }
        let owner = Writer(Box::into_raw(Box::new(WriterReceipt {
            vtbl: &WRITER_VTBL,
            refs: AtomicU32::new(1),
            stage: AtomicU32::new(WRITER_OPEN),
            recording,
            progress: Mutex::new((UploadWriterJoin::default(), UploadWriterCompletion::default())),
        })));
        WRITER_OWNERS.with_borrow_mut(|o| o[stamp.depth as usize - 1] = Some(owner));
    }));
    if entered.is_err() && stamp.nonce != 0 {
        WRITERS.with_borrow_mut(|w| w.cancel(stamp));
        WRITER_OWNERS.with_borrow_mut(|o| o[stamp.depth as usize - 1] = None);
        *stamp = UploadWriterStamp::default();
    }
}
/// UploadWriterScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_writer_exit(stamp: *const UploadWriterStamp) {
    // SAFETY: as documented.
    let stamp = unsafe { *stamp };
    if stamp.nonce == 0 {
        return;
    }
    if stamp.depth as usize <= MAX_UPLOAD_WRITER_DEPTH {
        let owner = WRITER_OWNERS.with_borrow_mut(|o| o[stamp.depth as usize - 1].take());
        if let Some(owner) = owner {
            owner.get().stage.store(WRITER_CANCELLED, Release);
        }
    }
    WRITERS.with_borrow_mut(|w| {
        if !w.cancel(&stamp) {
            w.invalidate();
        }
    });
}
/// UploadWriterScope::CompleteNormalReturn: `stamp` cleared.
///
/// # Safety
/// `stamp` is valid and writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_writer_complete(stamp: *mut UploadWriterStamp) {
    // SAFETY: as documented.
    let stamp = unsafe { &mut *stamp };
    if stamp.nonce == 0 {
        return;
    }
    let depth = stamp.depth as usize;
    let owner = WRITER_OWNERS.with_borrow(|o| o[depth - 1].clone());
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = current();
        let recording = if owner.is_some() && valid(&context) && active() && compatible() { capture(context.list, &context) } else { None };
        let writer = recording.as_ref().map_or_else(UploadWriterContext::default, |r| writer_context(r.raw()));
        let completion = WRITERS.with_borrow_mut(|w| w.finish(stamp, &writer));
        if let Some(owner) = &owner {
            match completion {
                Some(completion) => {
                    owner.get().progress.lock().unwrap_or_else(|e| e.into_inner()).1 = completion;
                    owner.get().stage.store(WRITER_COMPLETED, Release);
                }
                None => owner.get().stage.store(WRITER_CANCELLED, Release),
            }
        }
    }));
    if done.is_err() {
        if let Some(owner) = &owner {
            owner.get().stage.store(WRITER_CANCELLED, Release);
        }
        WRITERS.with_borrow_mut(|w| w.cancel(stamp));
    }
    WRITER_OWNERS.with_borrow_mut(|o| o[depth - 1] = None);
    *stamp = UploadWriterStamp::default();
}

// ---- the creations

/// ObserveCreationAddress: the pooled creation's resource and address, for the CBV it creates next.
///
/// # Safety
/// `resource` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_creation_address(resource: *mut c_void, address: u64, caller: u64) {
    if caller != base() + CREATION_ADDRESS_CALLER {
        return;
    }
    ADDRESS_HINT.with_borrow_mut(|h| *h = AddressHint { resource: None, gpu: 0, context: Context::default() });
    if !active() || !compatible() || address == 0 {
        return;
    }
    // SAFETY: as documented.
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width == 0 || address > u64::MAX - desc.Width {
        return;
    }
    let hint = AddressHint { resource: Some(resource.clone()), gpu: address, context: current() };
    ADDRESS_HINT.with_borrow_mut(|h| *h = hint);
}
fn take_hint() -> AddressHint {
    ADDRESS_HINT.with_borrow_mut(|h| core::mem::replace(h, AddressHint { resource: None, gpu: 0, context: Context::default() }))
}
fn created(device: *mut c_void, view: &D3D12_CONSTANT_BUFFER_VIEW_DESC, cpu: u64, caller: u64) -> RawOrigin {
    set_last(status::IGNORED);
    if !active() {
        return RawOrigin::none();
    }
    set_last(status::UNSUPPORTED_EXECUTABLE);
    if !compatible() {
        return RawOrigin::none();
    }
    set_last(status::UNKNOWN_CREATOR);
    if device.is_null() || cpu == 0 {
        return RawOrigin::none();
    }
    let game = base();
    let Some(frame) = crate::source_cbv_binding::creation_frame(game, caller) else {
        return RawOrigin::none();
    };
    let mut reader = |a: u64, out: &mut [u8]| read(a, out);
    let (Some(a), Some(b)) = (decode_creation(&frame, Some(&mut reader)), decode_creation(&frame, Some(&mut reader))) else {
        return RawOrigin::none();
    };
    if a != b || a.cpu != cpu {
        return RawOrigin::none();
    }
    let context = current();
    set_last(status::MISSING_CONTEXT);
    if !valid(&context) || (a.context != 0 && a.context != context.context) {
        return RawOrigin::none();
    }
    let mut native_buffer = NativeBuffer::default();
    let source: ID3D12Resource;
    if a.caller == POOLED_CREATOR_RVA {
        set_last(status::UNKNOWN_ORIGIN);
        let (Some(first), Some(second)) = (native(&context, a.handle, 0, 0, false), native(&context, a.handle, 0, 0, false)) else {
            return RawOrigin::none();
        };
        if !same_native(&first, &second) || first.kind != 4 || first.slot.descriptor != a.cpu {
            return RawOrigin::none();
        }
        native_buffer = first.native;
        let hint = take_hint();
        match hint.resource {
            Some(resource)
                if hint.context == context
                    && resource.as_raw() as u64 == a.resource
                    && hint.gpu <= u64::MAX - a.offset
                    && hint.gpu + a.offset == view.BufferLocation =>
            {
                source = resource
            }
            _ => return RawOrigin::none(),
        }
    } else {
        let raw = a.resource as *mut c_void;
        // SAFETY: the creator's live resource (the engine's, during its creation call).
        match unsafe { ID3D12Resource::from_raw_borrowed(&raw) } {
            Some(resource) => source = resource.clone(),
            None => return RawOrigin::none(),
        }
    }
    set_last(status::UNSUPPORTED_RESOURCE);
    // SAFETY: a live resource.
    let gpu = unsafe { source.GetGPUVirtualAddress() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    if !same_device(device, source.as_raw())
        || gpu > u64::MAX - a.offset
        || gpu + a.offset != view.BufferLocation
        // SAFETY: a live resource; our outputs.
        || unsafe { source.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err()
        || (heap.Type != D3D12_HEAP_TYPE_UPLOAD && heap.Type != D3D12_HEAP_TYPE_GPU_UPLOAD)
    {
        return RawOrigin::none();
    }
    // A creation occurrence retained when native tracking supplies one; without it the creation's facts still stand.
    let recording = capture(context.list, &context).unwrap_or_else(OwnedLease::empty);
    let (result, origin) = make_origin(&source, view, cpu, a.caller, native_buffer, Some(&a), Some(&recording));
    if a.caller == POOLED_CREATOR_RVA && WRITERS.with_borrow(|w| w.depth != 0) {
        let stamp = WRITERS.with_borrow(|w| w.current());
        let writer = WRITER_OWNERS.with_borrow(|o| o[stamp.depth as usize - 1].clone());
        let joined = UploadWriterJoin {
            // SAFETY: the origin made here, not yet published.
            origin: if origin.is_null() { 0 } else { unsafe { (*origin).id } },
            destination: frame.r15,
            resource: a.resource,
            offset: a.offset,
            handle: a.handle,
            logical_bytes: native_buffer.bytes as u32,
            reserved_bytes: view.SizeInBytes,
        };
        let joinable = writer.is_some()
            && !origin.is_null()
            && native_buffer.bytes <= 65536
            && crate::source_cbv_binding::original_upload_writer_path(game)
            && WRITERS.with_borrow_mut(|w| w.join(&stamp, &writer_context(recording.raw()), &joined));
        match writer {
            Some(writer) if joinable => {
                writer.get().progress.lock().unwrap_or_else(|e| e.into_inner()).0 = joined;
                // SAFETY: as above.
                unsafe { (*origin).writer = Some(writer) };
            }
            _ => WRITERS.with_borrow_mut(|w| w.invalidate()),
        }
    }
    set_last(if result.owner.is_null() { status::FAILURE } else { status::CREATED });
    result
}
/// ObserveCreatedCbv: into the empty `out` the origin of the CBV the engine creates.
///
/// # Safety
/// `device` is live or null; `view` valid or null; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_created(device: *mut c_void, view: *const D3D12_CONSTANT_BUFFER_VIEW_DESC, cpu: u64, caller: u64, out: *mut RawOrigin) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: as documented.
        match unsafe { view.as_ref() } {
            Some(view) => created(device, view, cpu, caller),
            None => {
                set_last(if active() { if compatible() { status::UNKNOWN_CREATOR } else { status::UNSUPPORTED_EXECUTABLE } } else { status::IGNORED });
                RawOrigin::none()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last(status::FAILURE);
        RawOrigin::none()
    });
    // SAFETY: as documented; the owner reference moves to the caller.
    unsafe { out.write(result) };
}
/// Reset: the list's observations dropped (released after the lock), this thread's hint for it too.
#[no_mangle]
pub extern "C-unwind" fn cvr_scbo_reset(list: u64) {
    let retired: Vec<Observation> = {
        let mut o = observations();
        let keys: Vec<Key> = o.map.keys().filter(|k| k.0 == list).copied().collect();
        keys.iter().filter_map(|k| o.map.remove(k)).collect()
    };
    drop(retired);
    ADDRESS_HINT.with_borrow_mut(|h| {
        if h.context.list == list {
            *h = AddressHint { resource: None, gpu: 0, context: Context::default() };
        }
    });
}
#[no_mangle]
pub extern "C-unwind" fn cvr_scbo_clear() {
    let retired = core::mem::take(&mut observations().map);
    drop(retired);
    drop(take_hint());
}

// ---- the read at the source

/// source_cbv::SourceCbvBacking: the resource, origin owner and leases' tokens owned references.
#[repr(C)]
pub struct Backing {
    pub resource: *mut c_void,
    pub origin_owner: *mut c_void,
    pub stamp: DescriptorStamp,
    pub recording: Lease,
    pub native: NativeBuffer,
    pub cookie: u64,
    pub origin: u64,
    pub gpu: u64,
    pub backing_bytes: u64,
    pub source_offset: u64,
    pub serial: u64,
    pub cpu_descriptor: u64,
    pub descriptor_bytes: u32,
    pub logical_bytes: u32,
    pub stage: u32,
    pub shader_register: u32,
    pub creator_rva: u32,
    pub reserved_bytes: u32,
    pub completed_producer_rva: u32,
    pub creation_recording: Lease,
    pub writer_nonce: u64,
    pub writer_destination: u64,
    pub writer_bytes: u32,
    pub writer_recording: Lease,
}
const _: () = assert!(
    core::mem::size_of::<Backing>() == 472
        && core::mem::offset_of!(Backing, recording) == 192
        && core::mem::offset_of!(Backing, native) == 232
        && core::mem::offset_of!(Backing, descriptor_bytes) == 336
        && core::mem::offset_of!(Backing, creation_recording) == 368
        && core::mem::offset_of!(Backing, writer_bytes) == 424
        && core::mem::offset_of!(Backing, writer_recording) == 432
);

fn read_at_source(list: u64, root: u64, parameter: u32, offset: u32, stamp: &DescriptorStamp, origin_owner: *mut c_void, out: &mut Backing) -> u32 {
    DIAGNOSTIC.set([0; 16]);
    set_last(status::IGNORED);
    if !active() {
        return status::IGNORED;
    }
    at(phase::CODE);
    set_last(status::UNSUPPORTED_EXECUTABLE);
    if !compatible() {
        return status::UNSUPPORTED_EXECUTABLE;
    }
    let context = current();
    set_last(status::MISSING_CONTEXT);
    at(phase::CONTEXT);
    diagnose(5, context.context);
    if list == 0 || root == 0 || context.list != list {
        return status::MISSING_CONTEXT;
    }
    let Some(recording) = capture(list, &context) else {
        return status::MISSING_CONTEXT;
    };
    diagnose(4, recording.raw().generation);
    diagnose(7, stamp.value[2]);
    diagnose(8, stamp.value[3]);
    let (mut slot, mut stage, mut reg, mut matches) = (Slot::default(), 0u32, 0u32, 0u32);
    at(phase::SLOT);
    for s in 0..2 {
        for r in 0..14 {
            let mut reader = |a: u64, out: &mut [u8]| read(a, out);
            if let Some(candidate) = read_slot(Some(&mut reader), context.state, s, r) {
                if candidate.parameter == parameter && candidate.offset == offset {
                    slot = candidate;
                    stage = s;
                    reg = r;
                    matches += 1;
                }
            }
        }
    }
    set_last(status::DESCRIPTOR_MISMATCH);
    if matches != 1 {
        return status::DESCRIPTOR_MISMATCH;
    }
    diagnose(2, stage as u64);
    diagnose(3, reg as u64);
    diagnose(6, slot.descriptor);
    at(phase::STAMP);
    if !matches_descriptor(stamp) {
        return status::DESCRIPTOR_MISMATCH;
    }
    at(phase::BINDING);
    let Some(actual) = resolve_cbv(list, root, parameter, offset).filter(|a| a.descriptor == stamp.gpu && a.heap == stamp.heap) else {
        return status::DESCRIPTOR_MISMATCH;
    };
    at(phase::CPU_VIEW);
    if !resolve_cpu_cbv(slot.descriptor).is_some_and(|cpu| cpu.address == actual.address && cpu.bytes == actual.bytes)
        || !same_view(stamp, actual.address, actual.bytes)
    {
        return status::DESCRIPTOR_MISMATCH;
    }
    let key: Key = (context.list, recording.raw().generation, stage, reg);
    at(phase::OBSERVATION);
    let found = observations().map.get(&key).cloned();
    let mut observed = found.is_some();
    let mut saved = found.unwrap_or_default();
    diagnose(11, saved.native.handle as u64);
    diagnose(12, saved.kind as u64);
    diagnose(13, saved.serial);
    diagnose(14, saved.address);
    diagnose(15, saved.view_bytes as u64);
    let owner = get_origin(origin_owner);
    let (mut resource, mut cookie, mut id, mut logical, mut creator): (Option<ID3D12Resource>, u64, u64, u64, u32) = (None, 0, 0, 0, 0);
    at(phase::ORIGIN);
    set_last(status::UNKNOWN_ORIGIN);
    if (!origin_owner.is_null() && owner.is_none()) || (owner.is_none() && (stamp.value[2] != 0 || stamp.value[3] != 0)) {
        return status::UNKNOWN_ORIGIN;
    }
    if let Some(owner) = &owner {
        let o = owner.get();
        diagnose(9, o.caller as u64);
        diagnose(10, o.cpu);
        if !same_origin(stamp, o.cookie, o.id) || !same_view(stamp, o.gpu.wrapping_add(o.offset), o.view_bytes) {
            return status::UNKNOWN_ORIGIN;
        }
        resource = o.resource.clone();
        cookie = o.cookie;
        id = o.id;
        creator = o.caller;
    }
    // This exact creator writes VS b5 directly, superseding a prior setter.
    let direct = owner.as_ref().is_some_and(|o| direct_creation(stage, reg, o.get().caller));
    if direct {
        let o = owner.as_ref().unwrap().get();
        let creation = Creation {
            resource: o.resource.as_ref().map_or(0, |r| r.as_raw() as u64),
            offset: o.offset,
            cpu: o.cpu,
            context: 0,
            caller: o.caller,
            handle: 0,
            logical_bytes: o.logical_bytes,
            reserved_bytes: o.reserved_bytes,
            completed_producer_rva: o.completed_producer_rva,
        };
        if !direct_producer(&creation, actual.bytes) {
            return status::UNKNOWN_ORIGIN;
        }
        logical = o.logical_bytes as u64;
        observed = false;
        saved = Observation::default();
    }
    let transient = owner.as_ref().is_some_and(|o| o.get().caller == POOLED_CREATOR_RVA && o.get().native.kind == 4);
    let mut native_receipt = observed;
    at(phase::NATIVE);
    if transient {
        let o = owner.as_ref().unwrap().get();
        set_last(status::CHANGED);
        let current_native = (o.native.context == context.context && o.cpu == slot.descriptor)
            .then(|| native(&context, o.native.handle, stage, reg, true))
            .flatten()
            .filter(|c| c.kind == 4 && same_buffer(&o.native, &c.native));
        let Some(current_native) = current_native else {
            return status::CHANGED;
        };
        saved = current_native;
        observed = false;
        native_receipt = true;
        logical = saved.logical_bytes as u64;
        diagnose(11, saved.native.handle as u64);
        diagnose(12, saved.kind as u64);
        diagnose(13, 0);
        diagnose(14, 0);
        diagnose(15, 0);
    } else if observed {
        set_last(status::CHANGED);
        let fresh = native(&context, saved.native.handle, stage, reg, true);
        let Some(fresh) = fresh.filter(|c| saved.context == context && same_source_native(&saved, c)) else {
            return status::CHANGED;
        };
        logical = saved.logical_bytes as u64;
        if saved.kind != 4 {
            let saved_resource = saved.resource.as_ref().map(|r| r.as_raw());
            if saved_resource.is_none()
                || saved_resource.map(|r| r as u64) != Some(fresh.native.resource)
                || resource.as_ref().is_some_and(|r| Some(r.as_raw()) != saved_resource)
                || saved.address != actual.address
                || saved.view_bytes != actual.bytes
            {
                return status::CHANGED;
            }
            resource = saved.resource.clone();
            if cookie == 0 {
                cookie = cookie_at_creation(resource.as_ref().unwrap().as_raw());
            }
        } else {
            set_last(status::UNKNOWN_ORIGIN);
            return status::UNKNOWN_ORIGIN;
        }
    } else if !direct {
        set_last(status::MISSING_OBSERVATION);
        return status::MISSING_OBSERVATION;
    }
    at(phase::RESOURCE);
    set_last(status::UNSUPPORTED_RESOURCE);
    let Some(resource) = resource.filter(|_| cookie != 0) else {
        return status::UNSUPPORTED_RESOURCE;
    };
    let Some(device) = list_device(list) else {
        return status::UNSUPPORTED_RESOURCE;
    };
    if !same_device(device.as_raw(), resource.as_raw()) {
        return status::UNSUPPORTED_RESOURCE;
    }
    // SAFETY: a live resource.
    let (desc, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || !descriptor(gpu, desc.Width, actual.address, actual.bytes) {
        return status::UNSUPPORTED_RESOURCE;
    }
    at(phase::REVALIDATE);
    set_last(status::CHANGED);
    let mut reader = |a: u64, out: &mut [u8]| read(a, out);
    if current() != context
        || read_slot(Some(&mut reader), context.state, stage, reg) != Some(slot)
        || !validate_recording(&recording, list, &context)
        || !matches_descriptor(stamp)
    {
        return status::CHANGED;
    }
    if native_receipt && !native(&context, saved.native.handle, stage, reg, true).is_some_and(|c| same_source_native(&saved, &c)) {
        return status::CHANGED;
    }
    if observed {
        at(phase::SERIAL);
        if !observations().map.get(&key).is_some_and(|f| f.serial == saved.serial) {
            return status::CHANGED;
        }
    }
    let creation_recording = owner.as_ref().map_or_else(OwnedLease::empty, |o| o.get().creation_recording.clone());
    *out = Backing {
        resource: resource.into_raw(),
        origin_owner: owner.as_ref().map_or(core::ptr::null_mut(), |o| add_ref(o.0.cast())),
        stamp: *stamp,
        recording: recording.into_raw(),
        native: saved.native,
        cookie,
        origin: id,
        gpu,
        backing_bytes: desc.Width,
        source_offset: actual.address - gpu,
        serial: saved.serial,
        cpu_descriptor: slot.descriptor,
        descriptor_bytes: actual.bytes,
        logical_bytes: logical as u32,
        stage,
        shader_register: reg,
        creator_rva: creator,
        reserved_bytes: 0,
        completed_producer_rva: 0,
        creation_recording: creation_recording.into_raw(),
        writer_nonce: 0,
        writer_destination: 0,
        writer_bytes: 0,
        writer_recording: OwnedLease::empty().into_raw(),
    };
    if direct {
        let o = owner.as_ref().unwrap().get();
        out.reserved_bytes = o.reserved_bytes;
        out.completed_producer_rva = o.completed_producer_rva;
    }
    if transient {
        let o = owner.as_ref().unwrap().get();
        if let Some(writer) = o.writer.as_ref().filter(|w| w.get().stage.load(Acquire) == WRITER_COMPLETED) {
            let (joined, completed) = *writer.get().progress.lock().unwrap_or_else(|e| e.into_inner());
            if completed.nonce != 0
                && completed.origin == o.id
                && completed.destination == joined.destination
                && completed.producer_rva == UPLOAD_WRITER_MEMCPY_RETURN_RVA
                && joined.resource == out.resource as u64
                && joined.offset == o.offset
                && joined.handle == saved.native.handle
                && joined.logical_bytes == saved.logical_bytes
                && joined.reserved_bytes == actual.bytes
                && completed.bytes != 0
                && completed.bytes <= saved.logical_bytes
            {
                out.writer_nonce = completed.nonce;
                out.writer_bytes = completed.bytes;
                out.writer_destination = completed.destination;
                out.writer_recording = writer.get().recording.clone().into_raw();
                out.reserved_bytes = actual.bytes;
                out.completed_producer_rva = completed.producer_rva;
            }
        }
    }
    at(phase::COMPLETE);
    let result = if observed && saved.kind != 4 { status::REGISTERED } else { status::CREATED };
    set_last(result);
    result
}
/// ReadAtSource: into the empty `out` (its references owned) the stamp's backing; the status returned.
///
/// # Safety
/// `stamp` is valid; `origin_owner` a live COM object or null; `out` an empty SourceCbvBacking.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scbo_read_at_source(
    list: u64,
    root: u64,
    parameter: u32,
    offset: u32,
    stamp: *const DescriptorStamp,
    origin_owner: *mut c_void,
    out: *mut Backing,
) -> u32 {
    // SAFETY: as documented.
    let (stamp, out) = unsafe { (&*stamp, &mut *out) };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_at_source(list, root, parameter, offset, stamp, origin_owner, out))).unwrap_or_else(|_| {
        set_last(status::FAILURE);
        status::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dormant_without_the_switch() {
        assert_eq!(CyberpunkVR_SourceCbvBindingEnable.load(Relaxed), 0);
        let mut out: [core::mem::MaybeUninit<Annotation>; 2] = [const { core::mem::MaybeUninit::uninit() }; 2];
        // SAFETY: room for two annotations.
        assert_eq!(unsafe { cvr_scbo_bind(0, 0, 1, 1, out.as_mut_ptr().cast()) }, 0);
        assert_eq!(cvr_scbo_last_status(), status::IGNORED);
        // SAFETY: as above.
        assert_eq!(unsafe { cvr_scbo_bind(1, 0, 1, 2, out.as_mut_ptr().cast()) }, 0);
        assert_eq!(cvr_scbo_last_status(), status::INVALID_BINDING);
        let mut origin = RawOrigin::none();
        let view = D3D12_CONSTANT_BUFFER_VIEW_DESC::default();
        // SAFETY: valid records; no device.
        unsafe { cvr_scbo_created(core::ptr::null_mut(), &view, 1, 0, &mut origin) };
        assert!(origin.owner.is_null());
        assert_eq!(cvr_scbo_last_status(), status::IGNORED);
        let mut stamp = UploadWriterStamp { nonce: 7, depth: 1 };
        // SAFETY: a local stamp.
        unsafe { cvr_scbo_writer_enter(1, 16, &mut stamp) };
        assert_eq!(stamp, UploadWriterStamp { nonce: 7, depth: 1 }, "an inactive scope leaves its stamp alone");
        let mut empty = UploadWriterStamp::default();
        // SAFETY: local stamps.
        unsafe {
            cvr_scbo_writer_enter(1, 16, &mut empty);
            cvr_scbo_writer_complete(&mut empty);
            cvr_scbo_writer_exit(&empty);
        }
        assert_eq!(empty, UploadWriterStamp::default());
        let mut diagnostic = [9u64; 16];
        // SAFETY: room for 16 values.
        unsafe { cvr_scbo_last_diagnostic(&mut diagnostic) };
        assert_eq!(diagnostic[1], status::IGNORED as u64);
    }

    #[test]
    fn origins_answer_their_type_and_writers_only_iunknown() {
        let origin = Box::into_raw(Box::new(Origin {
            vtbl: &ORIGIN_VTBL,
            refs: AtomicU32::new(1),
            resource: None,
            id: 3,
            cookie: 4,
            gpu: 0,
            bytes: 0,
            offset: 0,
            cpu: 0,
            view_bytes: 0,
            caller: 0,
            logical_bytes: 0,
            reserved_bytes: 0,
            completed_producer_rva: 0,
            creation_recording: OwnedLease::empty(),
            writer: None,
            native: NativeBuffer::default(),
        }));
        let found = get_origin(origin.cast()).expect("the origin answers its type");
        assert_eq!((found.get().id, found.get().cookie), (3, 4));
        drop(found);
        let writer = Box::into_raw(Box::new(WriterReceipt {
            vtbl: &WRITER_VTBL,
            refs: AtomicU32::new(1),
            stage: AtomicU32::new(WRITER_OPEN),
            recording: OwnedLease::empty(),
            progress: Mutex::new((UploadWriterJoin::default(), UploadWriterCompletion::default())),
        }));
        assert!(get_origin(writer.cast()).is_none(), "a writer receipt is no origin");
        // SAFETY: the last references.
        unsafe {
            (*origin).writer = Some(Writer(writer));
            assert_eq!(origin_release(origin.cast()), 0);
        }
    }

    #[test]
    fn reset_drops_one_lists_observations() {
        {
            let mut o = observations();
            o.map.insert((0x100, 1, 0, 2), Observation::default());
            o.map.insert((0x200, 1, 1, 3), Observation::default());
        }
        cvr_scbo_reset(0x100);
        {
            let o = observations();
            assert!(!o.map.contains_key(&(0x100, 1, 0, 2)) && o.map.contains_key(&(0x200, 1, 1, 3)));
        }
        cvr_scbo_clear();
        assert!(observations().map.is_empty());
    }
}
