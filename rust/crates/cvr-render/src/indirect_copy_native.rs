//! THE NATIVE INDIRECT COPY AT THE ACTUAL CALL (formerly src/Render/NativeIndirectCopy.cpp): the per-thread ledger of
//! actual native source callback scopes, the engine reads behind one ExecuteIndirect argument buffer (its upload
//! context, its physical allocation -- own or aliased -- and the allocator that owns it, its residency and the
//! context's transition capacity), the receipt that keeps the allocation and allocator alive with the command list, the
//! native combined-read registration (0x337DA0, then the flush 0x3E3BB4) and the proof the copy validates now and at
//! submission. The rules each step applies are indirect_copy.rs'.
//!
//! The C++ API (include/Render/NativeIndirectCopy.hpp) forwards here; the graph parent's revalidation (still C++,
//! NativePreparation.cpp) comes in as a callback with each call.
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::sync::OnceLock;

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12DeviceChild, ID3D12GraphicsCommandList, ID3D12Heap, D3D12_COMMAND_LIST_TYPE_DIRECT,
    D3D12_HEAP_TYPE_DEFAULT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::ia_binding::Binding;
use crate::indirect_copy::{admit, allocation_form, read_mask, Allocation, AllocationBinding, GraphParentRecord, Observation, ScopeLedger, ScopeStamp};

extern "C" {
    fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool;
}

/// indirect_copy::Failure.
mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const SCOPE: u32 = 2;
    pub const CODE: u32 = 3;
    pub const BINDING: u32 = 5;
    pub const ALLOCATION: u32 = 8;
    pub const ALLOCATOR: u32 = 9;
    pub const RESIDENCY: u32 = 10;
    pub const CAPACITY: u32 = 11;
    pub const CHANGED: u32 = 12;
    pub const OWNERSHIP: u32 = 13;
    pub const REGISTRATION: u32 = 14;
    pub const EXCEPTION: u32 = 15;
}
const RECEIPT_TYPE: GUID = GUID::from_values(0x45f6d18a, 0x394f, 0x45c1, [0xb2, 0x23, 0x2a, 0x68, 0x17, 0x5c, 0x04, 0x7e]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const ALLOCATION_VTABLE: u64 = 0x30466A8;
const ALLOCATOR_VTABLE: u64 = 0x306ED78;
const COPY_SOURCE: u32 = 0x800;
const INT32_MAX: u32 = i32::MAX as u32;

thread_local! {
    static SCOPES: RefCell<ScopeLedger> = RefCell::new(ScopeLedger::default());
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
fn set_failure(value: u32) {
    FAILURE.set(value);
}
fn thread() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() }
}
fn image() -> u64 {
    // SAFETY: the game's own module.
    unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64)
}
fn scope_matches(stamp: &ScopeStamp, list: u64, generation: u64, position: u64) -> bool {
    SCOPES.with_borrow(|s| s.matches(stamp, list, generation, position, thread()))
}

/// IndirectCopyProof (include/Render/StereoGeometryCommands.hpp): the resource and allocation owner are owned references
/// (ComPtr); the state an std::optional.
#[repr(C)]
pub struct Proof {
    pub resource: *mut c_void,
    pub state: u32,
    pub has_state: bool,
    pub recording_generation: u64,
    pub command_position: u64,
    pub allocation_owner: *mut c_void,
}
const _: () = assert!(
    core::mem::size_of::<Proof>() == 40
        && core::mem::offset_of!(Proof, state) == 8
        && core::mem::offset_of!(Proof, has_state) == 12
        && core::mem::offset_of!(Proof, recording_generation) == 16
        && core::mem::offset_of!(Proof, allocation_owner) == 32
);

fn read(address: u64, out: &mut [u8]) -> bool {
    crate::geometry_packets::read(address, out)
}
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_u32(address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
fn field_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}
fn field_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// A COM reference taken on `object` (the engine's allocator and allocation objects are reference counted through
/// IUnknown's slots, as the .cpp's ComPtr treated them).
fn add_ref(object: *mut c_void) -> *mut c_void {
    // SAFETY: a live COM-shaped object or null.
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

/// Device: the list and the device child are on one device.
fn same_device(list: &ID3D12GraphicsCommandList, child: *mut c_void) -> bool {
    // SAFETY: a live device child or null.
    let Some(child) = (unsafe { ID3D12DeviceChild::from_raw_borrowed(&child) }) else {
        return false;
    };
    let (mut a, mut b): (Option<ID3D12Device>, Option<ID3D12Device>) = (None, None);
    // SAFETY: live objects; the out-parameters are ours.
    let located = unsafe { list.GetDevice(&mut a).is_ok() && child.GetDevice(&mut b).is_ok() };
    located && a.as_ref().map(|d| d.as_raw()) == b.as_ref().map(|d| d.as_raw())
}
/// Context: the binding's upload context still current and idle (its kind and pending count written as read).
fn context(binding: &Binding, kind: &mut u32, pending: &mut u32) -> bool {
    let c = binding.context;
    if c.context == 0 || c.state == 0 || c.list == 0 || c.context > u64::MAX - 0x640 {
        return false;
    }
    if read_u64(c.context + 0x30) != Some(c.list) || read_u64(c.context + 0x60) != Some(c.state) {
        return false;
    }
    let Some(k) = read_u32(c.context + 0x68) else {
        return false;
    };
    *kind = k;
    if !(k == 1 || k == 2) || read_u64(c.context + 0x638) != Some(0) {
        return false;
    }
    let Some(p) = read_u32(c.context + 0x534) else {
        return false;
    };
    *pending = p;
    p == 0
}
/// AllocationBinding: the native record's direct allocation, or its aliased payload and control block.
fn allocation_binding(source: &Binding, result: &mut AllocationBinding) -> bool {
    *result = AllocationBinding::default();
    let direct = field_u64(&source.raw_record, 0x18);
    let (payload, control) = (field_u64(&source.raw_record, 0x20), field_u64(&source.raw_record, 0x28));
    if direct != 0 {
        if payload != 0 || control != 0 {
            return false;
        }
        result.allocation = direct;
        return true;
    }
    if payload == 0 || control == 0 || payload > u64::MAX - 0x38 {
        return false;
    }
    let mut bytes = [0u8; 0x38];
    if !read(payload, &mut bytes) {
        return false;
    }
    let Some(refs) = read_u32(control).map(|r| r as i32) else {
        return false;
    };
    if refs <= 0 || refs == i32::MAX {
        return false;
    }
    *result = AllocationBinding {
        allocation: field_u64(&bytes, 8),
        payload,
        control,
        flags: field_u32(&bytes, 0x10) as u64,
        alignment: field_u64(&bytes, 0x18),
        bytes: field_u64(&bytes, 0x20),
        alias: true,
    };
    result.allocation != 0 && result.flags == 0xC0 && result.alignment == 0x10000 && result.bytes != 0
}
/// Allocation: the physical allocation object at `address` (committed, placed in a block's heap, or in a heap).
fn allocation(base: u64, address: u64, output: &mut Allocation) -> bool {
    let mut bytes = [0u8; 0x68];
    if address == 0 || base > u64::MAX - ALLOCATION_VTABLE || !read(address, &mut bytes) {
        return false;
    }
    let refs = field_u32(&bytes, 8);
    let mut result = Allocation {
        address,
        vtable: field_u64(&bytes, 0),
        pimpl: field_u64(&bytes, 0x10),
        bytes: field_u64(&bytes, 0x18),
        resource: field_u64(&bytes, 0x28),
        ..Default::default()
    };
    result.kind = field_u32(&bytes, 0x60) & 3;
    if result.vtable != base + ALLOCATION_VTABLE || refs == 0 || refs >= INT32_MAX || result.pimpl == 0 || result.bytes == 0 || result.kind == 3 {
        return false;
    }
    if result.kind == 1 {
        result.block = field_u64(&bytes, 0x48);
        result.handle = field_u64(&bytes, 0x40);
        if result.block == 0 || result.block > u64::MAX - 0x40 {
            return false;
        }
        match read_u64(result.block + 0x38) {
            Some(heap) if heap != 0 => result.heap = heap,
            _ => return false,
        }
    } else if result.kind == 2 {
        result.heap = field_u64(&bytes, 0x58);
        if result.heap == 0 {
            return false;
        }
    }
    *output = result;
    true
}
/// Allocator: the registry's allocator, owning `allocation`'s implementation.
fn allocator(base: u64, source: &Binding, allocation: &Allocation, address: &mut u64) -> bool {
    *address = 0;
    let registry = source.arguments.native.registry;
    if base > u64::MAX - ALLOCATOR_VTABLE || registry > u64::MAX - 0x1410548 {
        return false;
    }
    match read_u64(registry + 0x1410540) {
        Some(a) => *address = a,
        None => return false,
    }
    if *address == 0 {
        return false;
    }
    let mut bytes = [0u8; 0x18];
    read(*address, &mut bytes)
        && field_u64(&bytes, 0) == base + ALLOCATOR_VTABLE
        && field_u32(&bytes, 8) != 0
        && field_u32(&bytes, 8) < INT32_MAX
        && field_u64(&bytes, 0x10) == allocation.pimpl
}
/// Residency: the native record's resident resource is the arguments' (or none), the tracker not rejecting.
fn residency(source: &Binding) -> bool {
    if source.record_address > u64::MAX - 0x70 {
        return false;
    }
    let Some(resident) = read_u64(source.record_address + 0x68) else {
        return false;
    };
    if resident == 0 {
        return true;
    }
    if resident != source.arguments.resource as u64 || source.context.context > u64::MAX - 0x638 {
        return false;
    }
    let Some(tracker) = read_u64(source.context.context + 0x630) else {
        return false;
    };
    if tracker == 0 || tracker > u64::MAX - 38 {
        return false;
    }
    let mut rejected = [0u8; 1];
    read(tracker + 37, &mut rejected) && rejected[0] == 0
}
/// TransitionCapacity: already a copy source, or the context's transition list has room and nothing pending.
fn transition_capacity(source: &Binding) -> bool {
    if source.state.value & COPY_SOURCE != 0 {
        return true;
    }
    let c = source.context.context;
    if c > u64::MAX - 0x538 {
        return false;
    }
    let Some(data) = read_u64(c + 0x528).filter(|&d| d != 0) else {
        return false;
    };
    let Some(capacity) = read_u32(c + 0x530).filter(|&n| n != 0 && n <= 1048576) else {
        return false;
    };
    data <= u64::MAX - capacity as u64 * 32 && read_u32(c + 0x534) == Some(0)
}

type Revalidate = unsafe extern "C" fn(*const GraphParentRecord) -> bool;

/// Observe: what the admission rules judge, re-read now.
fn observe(list: &ID3D12GraphicsCommandList, binding: &Binding, parent: &GraphParentRecord, position: u64, revalidate: Revalidate) -> Observation {
    let a = &binding.arguments;
    let n = &a.native;
    let mut value = Observation {
        native: crate::indirect_copy::NativeBuffer {
            resource: n.resource,
            gpu: n.gpu,
            bytes: n.bytes,
            registry: n.registry,
            context: n.context,
            handle: n.handle,
            kind: n.kind,
        },
        list: list.as_raw() as u64,
        resource: a.resource as u64,
        low_type: (binding.raw_record[6] & 15) as u32,
        generation: binding.recording.generation,
        position,
        backing_gpu: a.gpu,
        backing_bytes: a.bytes,
        offset: binding.argument_offset,
        bytes: binding.argument_bytes,
        record_address: binding.record_address,
        caller: binding.caller_rva,
        max_count: binding.max_count,
        signature_kind: binding.signature_kind,
        stride: binding.signature_stride,
        heap: a.heap_type,
        flags: a.flags,
        state: crate::indirect_copy::ReadState { value: binding.state.value, status: binding.state.status, kind: binding.state.kind },
        full_backing: binding.full_backing,
        ..Default::default()
    };
    // SAFETY: the C++ half's revalidation of a valid record.
    value.parent_current = unsafe { revalidate(parent) };
    let current = context(binding, &mut value.context_kind, &mut value.pending);
    let upload = crate::native_recording::Context { context: binding.context.context, state: binding.context.state, list: binding.context.list };
    // SAFETY: live list; valid lease and context.
    value.recording_current = current
        && unsafe { crate::native_recording::cvr_native_recording_validate_recording(&binding.recording, list.as_raw(), &upload) };
    // SAFETY: as above.
    value.native_origin =
        unsafe { crate::ia_binding::cvr_iab_revalidate_indirect(list.as_raw(), &binding.recording, binding, cvr_host::probe::internal()) };
    value.same_device = same_device(list, a.resource) && same_device(list, binding.signature);
    if !current {
        value.pending = u32::MAX;
    }
    value
}

/// The receipt the list keeps: the allocation and its allocator (references through their IUnknown slots), the binding
/// (its references owned) and what was proven about them.
#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    allocator: *mut c_void,
    allocation: *mut c_void,
    binding: Binding,
    parent: GraphParentRecord,
    scope: ScopeStamp,
    allocation_binding: AllocationBinding,
    physical: Allocation,
    image: u64,
    allocator_address: u64,
    ready: bool,
}
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: receipt_query, AddRef: receipt_add_ref, Release: receipt_release };
unsafe extern "system" fn receipt_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live receipt.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != RECEIPT_TYPE) {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Receipt)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn receipt_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live receipt.
    unsafe { (*(this as *mut Receipt)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn receipt_release(this: *mut c_void) -> u32 {
    let receipt = this as *mut Receipt;
    // SAFETY: a live receipt; the last reference frees it and what it holds.
    unsafe {
        let n = (*receipt).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let r = Box::from_raw(receipt);
            release(r.allocator);
            release(r.allocation);
            release_binding(&r.binding);
        }
        n
    }
}
/// A copy of `source` with its own references (IndirectBinding's copy constructor).
fn clone_binding(source: &Binding) -> Binding {
    // SAFETY: a bitwise copy whose COM members get references of their own.
    let copy = unsafe { core::ptr::read(source) };
    add_ref(copy.arguments.resource);
    add_ref(copy.signature);
    add_ref(copy.recording.token);
    copy
}
fn release_binding(binding: &Binding) {
    release(binding.arguments.resource);
    release(binding.signature);
    release(binding.recording.token);
}
/// A receipt reference this module holds (ComPtr<Receipt>).
struct Held(*mut Receipt);
impl Held {
    fn get(&self) -> &Receipt {
        // SAFETY: a live receipt this handle references.
        unsafe { &*self.0 }
    }
    fn get_mut(&mut self) -> &mut Receipt {
        // SAFETY: as above; the module's own receipt, used on one thread while preparing.
        unsafe { &mut *self.0 }
    }
    fn raw(&self) -> *mut c_void {
        self.0.cast()
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the reference this handle holds.
        unsafe { receipt_release(self.0.cast()) };
    }
}
/// Owner: the proof's allocation owner as this module's receipt.
fn owner(proof: &Proof) -> Option<Held> {
    if proof.allocation_owner.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object; QueryInterface through its first slot.
    unsafe {
        let vtbl = *(proof.allocation_owner as *const *const IUnknown_Vtbl);
        let _ = ((*vtbl).QueryInterface)(proof.allocation_owner, &RECEIPT_TYPE, &mut out);
    }
    (!out.is_null()).then(|| Held(out.cast()))
}
/// Physical: the receipt's allocation, binding and allocator unchanged, its heap a default heap on the list's device.
fn physical(list: &ID3D12GraphicsCommandList, owner: &Receipt) -> bool {
    let mut binding = AllocationBinding::default();
    let mut physical = Allocation::default();
    let mut allocator_address = 0u64;
    if owner.allocation.is_null()
        || owner.allocator.is_null()
        || !allocation_binding(&owner.binding, &mut binding)
        || binding != owner.allocation_binding
        || !allocation(owner.image, binding.allocation, &mut physical)
        || physical != owner.physical
        || !allocation_form(&binding, &physical, owner.binding.arguments.resource as u64, owner.binding.arguments.bytes)
        || !allocator(owner.image, &owner.binding, &physical, &mut allocator_address)
        || allocator_address != owner.allocator_address
        || owner.allocation as u64 != physical.address
        || owner.allocator as u64 != allocator_address
    {
        return false;
    }
    if physical.heap != 0 {
        let heap = physical.heap as *mut c_void;
        if !same_device(list, heap) {
            return false;
        }
        // SAFETY: the allocation's live heap.
        let Some(heap) = (unsafe { ID3D12Heap::from_raw_borrowed(&heap) }) else {
            return false;
        };
        // SAFETY: as above.
        if unsafe { heap.GetDesc() }.Properties.Type != D3D12_HEAP_TYPE_DEFAULT {
            return false;
        }
    }
    true
}

/// ActualCallScope's constructor: `out` the scope's stamp on this thread's ledger.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_scope_enter(list: u64, generation: u64, position: u64, out: *mut ScopeStamp) {
    let stamp = SCOPES.with_borrow_mut(|s| s.enter(list, generation, position, thread()));
    // SAFETY: as documented.
    unsafe { *out = stamp };
}
/// ActualCallScope's destructor.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_scope_exit(stamp: *const ScopeStamp) {
    // SAFETY: as documented.
    let stamp = unsafe { *stamp };
    if stamp.nonce != 0 {
        SCOPES.with_borrow_mut(|s| s.exit(&stamp));
    }
}
/// ActualCallScope::Active.
///
/// # Safety
/// `stamp` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_scope_active(stamp: *const ScopeStamp) -> bool {
    // SAFETY: as documented.
    let stamp = unsafe { *stamp };
    scope_matches(&stamp, stamp.list, stamp.generation, stamp.position)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ic_last_failure() -> u32 {
    FAILURE.get()
}
/// Compatible: the eighteen functions and the allocator vtables (code_tables.rs table 3), checked once.
#[no_mangle]
pub extern "C-unwind" fn cvr_ic_compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| {
        let base = image();
        base != 0
            && crate::ia_binding::cvr_iab_indirect_compatible()
            && crate::code_tables::cvr_code_table(3, base)
            && crate::code_tables::cvr_nic_allocator_vtables(base)
    })
}

fn prepare(list: *mut c_void, binding: &Binding, parent: &GraphParentRecord, position: u64, output: &mut Proof, revalidate: Revalidate) -> bool {
    set_failure(failure::ARGUMENTS);
    // SAFETY: a live list or null.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) };
    let Some(list) = list.filter(|_| !cvr_host::probe::internal()) else {
        return false;
    };
    if !output.resource.is_null()
        || output.has_state
        || output.recording_generation != 0
        || output.command_position != 0
        || !output.allocation_owner.is_null()
        // SAFETY: a live list.
        || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || binding.arguments.resource.is_null()
        || binding.signature.is_null()
    {
        return false;
    }
    let list_address = list.as_raw() as u64;
    let generation = binding.recording.generation;
    let scope = SCOPES.with_borrow(|s| s.current());
    set_failure(failure::SCOPE);
    if !scope_matches(&scope, list_address, generation, position) {
        return false;
    }
    set_failure(failure::CODE);
    if !cvr_ic_compatible() {
        return false;
    }
    set_failure(admit(&observe(list, binding, parent, position, revalidate), parent));
    if FAILURE.get() != failure::NONE {
        return false;
    }
    set_failure(failure::ALLOCATION);
    let mut binding_form = AllocationBinding::default();
    let mut physical_allocation = Allocation::default();
    let base = image();
    if !allocation_binding(binding, &mut binding_form)
        || !allocation(base, binding_form.allocation, &mut physical_allocation)
        || !allocation_form(&binding_form, &physical_allocation, binding.arguments.resource as u64, binding.arguments.bytes)
    {
        return false;
    }
    set_failure(failure::ALLOCATOR);
    let mut allocator_address = 0u64;
    if !allocator(base, binding, &physical_allocation, &mut allocator_address) {
        return false;
    }
    let mut owner = Held(Box::into_raw(Box::new(Receipt {
        vtbl: &RECEIPT_VTBL,
        refs: AtomicU32::new(1),
        allocator: add_ref(allocator_address as *mut c_void),
        allocation: add_ref(physical_allocation.address as *mut c_void),
        binding: clone_binding(binding),
        parent: *parent,
        scope,
        allocation_binding: binding_form,
        physical: physical_allocation,
        image: base,
        allocator_address,
        ready: false,
    })));
    set_failure(failure::ALLOCATION);
    if !physical(list, owner.get()) {
        return false;
    }
    set_failure(failure::RESIDENCY);
    if !residency(binding) {
        return false;
    }
    set_failure(failure::CAPACITY);
    if !transition_capacity(binding) {
        return false;
    }
    let mut pending = Pending(Proof {
        resource: add_ref(binding.arguments.resource),
        state: 0,
        has_state: false,
        recording_generation: generation,
        command_position: position,
        allocation_owner: add_ref(owner.raw()),
    });
    set_failure(failure::OWNERSHIP);
    let kept = [owner.raw()];
    // SAFETY: a live list and receipt.
    if !unsafe { cvr_gpu_keep_command_resources(list.as_raw(), kept.as_ptr(), 1) } {
        return false;
    }
    set_failure(admit(&observe(list, binding, parent, position, revalidate), parent));
    if FAILURE.get() != failure::NONE {
        return false;
    }
    set_failure(failure::CHANGED);
    if !physical(list, owner.get()) || !residency(binding) || !scope_matches(&scope, list_address, generation, position) {
        return false;
    }
    let required = binding.state.value | COPY_SOURCE;
    {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the engine's audited registration and flush (Compatible checked their code) on the binding's context.
        unsafe {
            let register: unsafe extern "C" fn(*mut c_void, u32, u32) = core::mem::transmute((base + 0x337DA0) as usize);
            let flush: unsafe extern "C" fn(*mut c_void) = core::mem::transmute((base + 0x3E3BB4) as usize);
            register(binding.context.context as *mut c_void, binding.arguments.native.handle, required);
            flush(binding.context.context as *mut c_void);
        }
    }
    set_failure(failure::REGISTRATION);
    let state = crate::ia_binding::read_state_at_draw(list_address, &owner.get().binding.arguments);
    owner.get_mut().binding.state = state;
    let o = owner.get();
    let rule_state = crate::indirect_copy::ReadState { value: o.binding.state.value, status: o.binding.state.status, kind: o.binding.state.kind };
    if o.binding.state.value != required
        || !read_mask(&rule_state, binding.arguments.native.kind, true)
        // SAFETY: live list; valid lease and binding.
        || !unsafe { crate::ia_binding::cvr_iab_revalidate_indirect(list.as_raw(), &o.binding.recording, &o.binding, cvr_host::probe::internal()) }
        // SAFETY: the C++ half's revalidation of a valid record.
        || !unsafe { revalidate(parent) }
        || !physical(list, o)
        || !residency(&o.binding)
        || !scope_matches(&scope, list_address, generation, position)
    {
        return false;
    }
    owner.get_mut().ready = true;
    pending.0.state = required;
    pending.0.has_state = true;
    if !validate(list.as_raw(), &pending.0, binding.argument_offset, 20, revalidate) {
        return false;
    }
    *output = pending.take();
    set_failure(failure::NONE);
    true
}
/// A proof under construction (its references released unless taken).
struct Pending(Proof);
impl Pending {
    fn take(&mut self) -> Proof {
        core::mem::replace(
            &mut self.0,
            Proof { resource: core::ptr::null_mut(), state: 0, has_state: false, recording_generation: 0, command_position: 0, allocation_owner: core::ptr::null_mut() },
        )
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        release(self.0.resource);
        release(self.0.allocation_owner);
    }
}
fn validate(list: *mut c_void, proof: &Proof, offset: u64, bytes: u64, revalidate: Revalidate) -> bool {
    set_failure(failure::ARGUMENTS);
    // SAFETY: a live list or null.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) };
    let Some(list) = list.filter(|_| !cvr_host::probe::internal()) else {
        return false;
    };
    if proof.resource.is_null() || !proof.has_state || proof.allocation_owner.is_null() || bytes != 20 || offset % 4 != 0 {
        return false;
    }
    let Some(owner) = owner(proof).filter(|o| o.get().ready) else {
        return false;
    };
    let o = owner.get();
    set_failure(failure::SCOPE);
    if !scope_matches(&o.scope, list.as_raw() as u64, proof.recording_generation, proof.command_position) {
        return false;
    }
    set_failure(failure::CHANGED);
    let rule_state = crate::indirect_copy::ReadState { value: o.binding.state.value, status: o.binding.state.status, kind: o.binding.state.kind };
    if o.image != image()
        || proof.resource != o.binding.arguments.resource
        || proof.state != o.binding.state.value
        || offset != o.binding.argument_offset
        || proof.recording_generation != o.binding.recording.generation
        || !read_mask(&rule_state, o.binding.arguments.native.kind, true)
        || !physical(list, o)
        || !residency(&o.binding)
    {
        return false;
    }
    set_failure(admit(&observe(list, &o.binding, &o.parent, proof.command_position, revalidate), &o.parent));
    if FAILURE.get() != failure::NONE {
        return false;
    }
    set_failure(failure::NONE);
    true
}
fn validate_submission(proof: &Proof, queue: *mut c_void) -> bool {
    set_failure(failure::ARGUMENTS);
    if queue.is_null() {
        return false;
    }
    let owner = owner(proof);
    set_failure(failure::OWNERSHIP);
    let Some(owner) = owner.filter(|o| {
        let o = o.get();
        o.ready && !o.allocation.is_null() && !o.allocator.is_null()
    }) else {
        return false;
    };
    let o = owner.get();
    set_failure(failure::CHANGED);
    if o.image != image()
        || proof.resource != o.binding.arguments.resource
        || !proof.has_state
        || proof.state != o.binding.state.value
        || proof.recording_generation != o.binding.recording.generation
        || proof.command_position != o.scope.position
    {
        return false;
    }
    set_failure(failure::BINDING);
    // SAFETY: a valid lease; a live queue.
    if !unsafe { crate::native_recording::cvr_native_recording_validate_submission(&o.binding.recording, queue) } {
        return false;
    }
    set_failure(failure::NONE);
    true
}
/// Runs `f`, a panic counted as the .cpp's caught exception.
fn guarded(f: impl FnOnce() -> bool) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        set_failure(failure::EXCEPTION);
        false
    })
}

/// PrepareAtActualCall: on success `out` (an empty proof) gets the arguments' and the receipt's references.
///
/// # Safety
/// `list` is live or null; `binding`, `parent` and `out` are valid; `revalidate` revalidates a graph parent.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_prepare(
    list: *mut c_void,
    binding: *const Binding,
    parent: *const GraphParentRecord,
    position: u64,
    out: *mut Proof,
    revalidate: Revalidate,
) -> bool {
    // SAFETY: as documented.
    guarded(|| unsafe { prepare(list, &*binding, &*parent, position, &mut *out, revalidate) })
}
/// Validate.
///
/// # Safety
/// `list` is live or null; `proof` is valid; `revalidate` as for cvr_ic_prepare.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_validate(list: *mut c_void, proof: *const Proof, offset: u64, bytes: u64, revalidate: Revalidate) -> bool {
    // SAFETY: as documented.
    guarded(|| validate(list, unsafe { &*proof }, offset, bytes, revalidate))
}
/// ValidateSubmission.
///
/// # Safety
/// `proof` is valid; `queue` live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_validate_submission(proof: *const Proof, queue: *mut c_void) -> bool {
    // SAFETY: as documented.
    guarded(|| validate_submission(unsafe { &*proof }, queue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nest_and_match_on_this_thread() {
        let mut outer = ScopeStamp::default();
        let mut inner = ScopeStamp::default();
        // SAFETY: local stamps.
        unsafe {
            cvr_ic_scope_enter(0x10, 2, 3, &mut outer);
            assert!(cvr_ic_scope_active(&outer));
            cvr_ic_scope_enter(0x10, 2, 4, &mut inner);
            assert!(cvr_ic_scope_active(&inner) && !cvr_ic_scope_active(&outer));
            cvr_ic_scope_exit(&inner);
            // The nested occurrence advanced the nonce: the outer scope no longer labels the current occurrence.
            assert!(!cvr_ic_scope_active(&outer));
            cvr_ic_scope_exit(&outer);
        }
        let mut empty = ScopeStamp::default();
        // SAFETY: a local stamp.
        unsafe { cvr_ic_scope_enter(0, 2, 3, &mut empty) };
        assert_eq!(empty.nonce, 0);
    }

    #[test]
    fn arguments_refused_without_a_list_or_with_a_used_proof() {
        let binding_bytes = [0u8; core::mem::size_of::<Binding>()];
        // SAFETY: an all-zero binding is a valid empty IndirectBinding.
        let binding: &Binding = unsafe { &*(binding_bytes.as_ptr() as *const Binding) };
        let parent = GraphParentRecord::default();
        let mut proof = Proof { resource: core::ptr::null_mut(), state: 0, has_state: false, recording_generation: 0, command_position: 0, allocation_owner: core::ptr::null_mut() };
        unsafe extern "C" fn never(_: *const GraphParentRecord) -> bool {
            panic!("not reached")
        }
        // SAFETY: valid records; no list.
        assert!(!unsafe { cvr_ic_prepare(core::ptr::null_mut(), binding, &parent, 1, &mut proof, never) });
        assert_eq!(cvr_ic_last_failure(), failure::ARGUMENTS);
        // SAFETY: as above.
        assert!(!unsafe { cvr_ic_validate(core::ptr::null_mut(), &proof, 0, 20, never) });
        assert_eq!(cvr_ic_last_failure(), failure::ARGUMENTS);
        // SAFETY: as above.
        assert!(!unsafe { cvr_ic_validate_submission(&proof, core::ptr::null_mut()) });
        assert_eq!(cvr_ic_last_failure(), failure::ARGUMENTS);
        // A proof with no owner: no receipt behind it.
        // SAFETY: as above; a non-null dummy queue never dereferenced (no owner).
        assert!(!unsafe { cvr_ic_validate_submission(&proof, 8 as *mut c_void) });
        assert_eq!(cvr_ic_last_failure(), failure::OWNERSHIP);
    }

    #[test]
    fn receipt_answers_its_own_interface_and_frees_at_zero() {
        let binding_bytes = [0u8; core::mem::size_of::<Binding>()];
        let receipt = Box::into_raw(Box::new(Receipt {
            vtbl: &RECEIPT_VTBL,
            refs: AtomicU32::new(1),
            allocator: core::ptr::null_mut(),
            allocation: core::ptr::null_mut(),
            // SAFETY: an all-zero binding (no references).
            binding: unsafe { core::ptr::read(binding_bytes.as_ptr() as *const Binding) },
            parent: GraphParentRecord::default(),
            scope: ScopeStamp::default(),
            allocation_binding: AllocationBinding::default(),
            physical: Allocation::default(),
            image: 0,
            allocator_address: 0,
            ready: true,
        }));
        let proof = Proof { resource: core::ptr::null_mut(), state: 0, has_state: false, recording_generation: 0, command_position: 0, allocation_owner: receipt.cast() };
        let held = owner(&proof).expect("the receipt answers its type");
        assert!(held.get().ready);
        // SAFETY: the receipt is live (two references).
        assert_eq!(unsafe { (*receipt).refs.load(SeqCst) }, 2);
        drop(held);
        // SAFETY: the last reference.
        assert_eq!(unsafe { receipt_release(receipt.cast()) }, 0);
    }
}
