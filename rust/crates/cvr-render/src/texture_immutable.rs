//! THE NATIVE TEXTURES' IMMUTABLE LEASES (formerly the immutable path of src/Render/NativeTextureRead.cpp): a fixed
//! asset -- a texture the engine created in COMMON on a default heap and never writes again -- leased for reads with its
//! native allocation pinned: the pooled allocation record and its allocator are retained through their own reference
//! counts, the creation receipt (texture_receipts.rs) proves the upload, and `physical` re-checks on every validation
//! that the resource still sits in that allocation (committed, or placed at the exact heap offset the engine's
//! placement metadata gives). The lease's owner is a COM object answering the immutable owner's type, held by the C++
//! ImmutableLease; the fixed pins (texture_pins.rs) create owners of their own.
use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface as _, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND;

use crate::native_recording::{
    cvr_native_recording_capture, cvr_native_recording_validate_recording,
    cvr_native_recording_validate_submission, Context as RecordingContext, Lease as Recording,
};
use crate::source_cbv_details::{
    read_allocation, read_allocator, reader, Allocation, Allocator, KNOWN,
};
use crate::texture_lease::{
    compatible, current_context, default_heap, device_of, fail, fixed_context, fixed_owner, game,
    list_of, read_value, resolve_native, resource, validate, Lease, UploadContext, READ,
};
use crate::texture_read::{immutable_shape, read_state, NativeTexture, SHADER_READS};
use crate::texture_receipts::{acquire_receipt, fixed_compatible};

const OWNER_TYPE: GUID = GUID::from_u128(0x22c496a1_7e0b_45f7_949a_4211c7098031);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

/// constants::ResourceCreationInfo as the C++ lays it out (its ComPtrs are the nullable interfaces).
#[repr(C)]
#[derive(Clone, Default)]
pub struct Creation {
    pub kind: u32,
    pub known: bool,
    pub state: i32,
    pub resource: Option<ID3D12Resource>,
    pub heap: Option<ID3D12Heap>,
    pub offset: u64,
    pub bytes: u64,
}
/// ia_snapshot::detail::PlacedAllocation.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PlacedAllocation {
    pub allocation: u64,
    pub resource: u64,
    pub pimpl: u64,
    pub block: u64,
    pub handle: u64,
    pub heap: u64,
    pub bytes: u64,
    pub metadata: u64,
    pub metadata_vtable: u64,
    pub offset_method: u64,
    pub offset: u64,
}
/// texture_read::ImmutableLease as the C++ lays it out: the owner and the lease, both references.
#[repr(C)]
pub struct ImmutableLease {
    pub owner: *mut c_void,
    pub source: Lease,
}
/// texture_read::ImmutablePageable.
#[repr(C)]
pub struct ImmutablePageable {
    pub source: ImmutableLease,
    pub pageable: *mut c_void,
}
/// texture_read::ImmutableUse.
#[repr(C)]
pub struct ImmutableUse {
    pub source: ImmutableLease,
    pub recording: Recording,
}
const _: () = assert!(
    core::mem::size_of::<Creation>() == 48
        && core::mem::size_of::<PlacedAllocation>() == 88
        && core::mem::size_of::<ImmutableLease>() == 56
        && core::mem::size_of::<ImmutablePageable>() == 64
        && core::mem::size_of::<ImmutableUse>() == 96
);

type QueryCreation = unsafe extern "C" fn(*mut c_void, *mut Creation) -> bool;
type PlacedOffset = unsafe extern "C" fn(*const PlacedAllocation, *mut PlacedAllocation) -> bool;
static QUERY_CREATION: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static PLACED_OFFSET: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `query` is constants::QueryResourceCreation into a zeroed record (its references then the caller's); `placed` is
/// ia_snapshot::ReadPinnedPlacedOffset.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_immutable_attach(query: QueryCreation, placed: PlacedOffset) {
    QUERY_CREATION.store(query as *mut c_void, Release);
    PLACED_OFFSET.store(placed as *mut c_void, Release);
}
/// constants::QueryResourceCreation (the plugin's own, formerly lent by src/Render/NativeTextureRead.cpp): the
/// resource's creation receipt, the resource and its heap referenced.
#[cfg(all(feature = "rust-build", not(test)))]
fn query_creation_rust(r: *mut c_void) -> Option<Creation> {
    let (mut kind, mut known, mut state, mut heap, mut offset, mut bytes) = (0u32, false, 0u32, core::ptr::null_mut(), 0u64, 0u64);
    // SAFETY: a live resource; writable outputs (the heap's reference ours).
    if !unsafe { crate::creation_receipts::cvr_crc_query(r, &mut kind, &mut known, &mut state, &mut heap, &mut offset, &mut bytes) } {
        return None;
    }
    Some(Creation {
        kind,
        known,
        state: if known { state as i32 } else { 0 },
        // SAFETY: the caller's live resource, referenced for the record.
        resource: unsafe { ID3D12Resource::from_raw_borrowed(&r) }.cloned(),
        // SAFETY: the reference the query handed over (or null).
        heap: (!heap.is_null()).then(|| unsafe { ID3D12Heap::from_raw(heap) }),
        offset,
        bytes,
    })
}
pub(crate) fn query_creation(r: *mut c_void) -> Option<Creation> {
    let f = QUERY_CREATION.load(Acquire);
    #[cfg(all(feature = "rust-build", not(test)))]
    if f.is_null() && !r.is_null() {
        return query_creation_rust(r);
    }
    if f.is_null() || r.is_null() {
        return None;
    }
    let mut out = Creation::default();
    // SAFETY: the attached function fills a zeroed record; its references become `out`'s.
    let ok = unsafe { core::mem::transmute::<*mut c_void, QueryCreation>(f)(r, &mut out) };
    ok.then_some(out)
}
fn placed_offset(wanted: &PlacedAllocation) -> Option<PlacedAllocation> {
    let f = PLACED_OFFSET.load(Acquire);
    // The plugin's own (formerly lent by src/Render/NativeTextureRead.cpp): ia_snapshot::ReadPinnedPlacedOffset.
    #[cfg(all(feature = "rust-build", not(test)))]
    if f.is_null() {
        let mut out = PlacedAllocation::default();
        // SAFETY: the wanted record and a writable output.
        return unsafe { crate::ia_snapshot_receipt::cvr_iasn_read_pinned_placed_offset(wanted, &mut out) }.then_some(out);
    }
    if f.is_null() {
        return None;
    }
    let mut out = PlacedAllocation::default();
    // SAFETY: the attached function.
    unsafe { core::mem::transmute::<*mut c_void, PlacedOffset>(f)(wanted, &mut out) }.then_some(out)
}

// ---- the owner

#[repr(C)]
pub(crate) struct Owner {
    pub(crate) vtbl: *const IUnknown_Vtbl,
    pub(crate) refs: AtomicU32,
    /// The native allocator and allocation records, retained through their own reference counts (vtable slots 1/2).
    pub(crate) allocator_pin: Option<IUnknown>,
    pub(crate) allocation_pin: Option<IUnknown>,
    /// `source.resource` is `held`'s pointer.
    pub(crate) held: Option<ID3D12Resource>,
    pub(crate) source: Lease,
    pub(crate) creation: Creation,
    pub(crate) allocation: Allocation,
    pub(crate) allocator: Allocator,
}
unsafe extern "system" fn owner_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Owner.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != OWNER_TYPE) {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *const Owner)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner.
    unsafe { (*(this as *const Owner)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner; at zero it is the last reference and is freed.
    unsafe {
        let left = (*(this as *const Owner)).refs.fetch_sub(1, SeqCst) - 1;
        if left == 0 {
            drop(Box::from_raw(this as *mut Owner));
        }
        left
    }
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: owner_query,
    AddRef: owner_add_ref,
    Release: owner_release,
};
/// One reference to an owner, released on drop.
pub(crate) struct Held(*mut Owner);
impl Held {
    pub(crate) fn new(owner: Owner) -> Held {
        Held(Box::into_raw(Box::new(owner)))
    }
    pub(crate) fn get(&self) -> &Owner {
        // SAFETY: a live owner while this reference is held.
        unsafe { &*self.0 }
    }
    pub(crate) fn get_mut(&mut self) -> &mut Owner {
        // SAFETY: only while the owner is not yet shared (creation).
        unsafe { &mut *self.0 }
    }
    pub(crate) fn into_raw(self) -> *mut c_void {
        let p = self.0 as *mut c_void;
        core::mem::forget(self);
        p
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the reference this holds.
        unsafe { owner_release(self.0 as *mut c_void) };
    }
}
pub(crate) fn empty_owner() -> Owner {
    Owner {
        vtbl: &OWNER_VTBL,
        refs: AtomicU32::new(1),
        allocator_pin: None,
        allocation_pin: None,
        held: None,
        source: Lease {
            resource: core::ptr::null_mut(),
            native: NativeTexture::default(),
            receipt: 0,
            owner: 0,
        },
        creation: Creation::default(),
        allocation: Allocation::default(),
        allocator: Allocator::default(),
    }
}
/// The owner behind a lease's IUnknown: queried for the owner's type, as the C++ did.
fn owner_of(unknown: *mut c_void) -> Option<Held> {
    if unknown.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live IUnknown of the caller's.
    let hr = unsafe {
        ((*(*(unknown as *mut *const IUnknown_Vtbl))).QueryInterface)(
            unknown,
            &OWNER_TYPE,
            &mut out,
        )
    };
    if hr.is_err() || out.is_null() {
        return None;
    }
    let held = Held(out as *mut Owner);
    // Only this module's owners answer the type; anything else is refused.
    (held.get().vtbl == &OWNER_VTBL as *const _).then_some(held)
}
/// One of this module's owners, borrowed from a pointer the caller holds a reference through (no QueryInterface).
pub(crate) fn owner_ref<'a>(unknown: *mut c_void) -> Option<&'a Owner> {
    if unknown.is_null() {
        return None;
    }
    // SAFETY: a live COM object of the caller's; only this module's owners carry OWNER_VTBL.
    let owner = unsafe { &*(unknown as *const Owner) };
    (owner.vtbl == &OWNER_VTBL as *const _).then_some(owner)
}
/// A borrowed engine record as a retained IUnknown (its slot 1 AddRef).
fn retain_native(address: u64) -> Option<IUnknown> {
    let p = address as *mut c_void;
    // SAFETY: a live engine record whose first slots are the audited QueryInterface/AddRef/Release.
    unsafe { IUnknown::from_raw_borrowed(&p) }.cloned()
}
fn add_ref(p: *mut c_void) -> *mut c_void {
    if !p.is_null() {
        // SAFETY: a live COM object.
        unsafe { ((*(*(p as *mut *const IUnknown_Vtbl))).AddRef)(p) };
    }
    p
}
/// An owned copy of an owner's lease for the C++ (the owner and the resource each referenced once more).
pub(crate) fn lease_out(owner: &Held) -> ImmutableLease {
    let o = owner.get();
    ImmutableLease {
        owner: add_ref(owner.0 as *mut c_void),
        source: Lease {
            resource: add_ref(o.source.resource),
            ..o.source
        },
    }
}

pub(crate) fn same_allocation(a: &Allocation, b: &Allocation) -> bool {
    a.observation == KNOWN
        && b.observation == KNOWN
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.bytes == b.bytes
        && a.resource == b.resource
        && a.heap == b.heap
        && a.block == b.block
        && a.handle == b.handle
        && a.kind == b.kind
}
fn raw<T: windows::core::Interface>(p: &Option<T>) -> *mut c_void {
    p.as_ref().map_or(core::ptr::null_mut(), |v| v.as_raw())
}
/// Physical: the pinned allocation still holds the resource as created -- the creation receipt unchanged and COMMON,
/// the allocation and allocator records the pinned ones, and a placed resource at the heap offset the engine's own
/// placement metadata gives.
pub(crate) fn physical(owner: &Owner) -> bool {
    let base = game();
    let saved = &owner.source;
    if owner.allocation_pin.is_none() || owner.allocator_pin.is_none() {
        return false;
    }
    let Some(current) = query_creation(saved.resource) else {
        return false;
    };
    let expected = &owner.creation;
    if raw(&current.resource) != saved.resource
        || raw(&current.resource) != raw(&expected.resource)
        || current.kind != expected.kind
        || raw(&current.heap) != raw(&expected.heap)
        || current.offset != expected.offset
        || current.bytes != expected.bytes
        || !current.known
        || current.state != D3D12_RESOURCE_STATE_COMMON.0
    {
        return false;
    }
    let allocation = read_allocation(
        &reader(READ, core::ptr::null_mut()),
        base,
        raw(&owner.allocation_pin) as u64,
    );
    let allocator = read_allocator(
        &reader(READ, core::ptr::null_mut()),
        base,
        saved.native.registry,
        &allocation,
    );
    if !same_allocation(&owner.allocation, &allocation)
        || allocation.address != saved.owner
        || allocation.resource != saved.native.resource
        || allocator.observation != KNOWN
        || !allocator.pimpl_matches
        || allocator.address != raw(&owner.allocator_pin) as u64
        || allocator.address != owner.allocator.address
        || allocator.vtable != owner.allocator.vtable
        || allocator.pimpl != owner.allocator.pimpl
    {
        return false;
    }
    if current.kind == 1 {
        return allocation.kind == 0
            && allocation.heap == 0
            && current.heap.is_none()
            && current.offset == 0
            && current.bytes == 0;
    }
    let Some(heap) = current.heap.as_ref() else {
        return false;
    };
    if current.kind != 2
        || allocation.kind != 1
        || allocation.heap != heap.as_raw() as u64
        || current.bytes == 0
        || allocation.bytes != current.bytes
    {
        return false;
    }
    // SAFETY: a live heap.
    let desc = unsafe { heap.GetDesc() };
    if desc.Properties.Type != D3D12_HEAP_TYPE_DEFAULT
        || current.offset > desc.SizeInBytes
        || current.bytes > desc.SizeInBytes - current.offset
    {
        return false;
    }
    let wanted = PlacedAllocation {
        allocation: allocation.address,
        resource: allocation.resource,
        pimpl: allocation.pimpl,
        block: allocation.block,
        handle: allocation.handle,
        heap: allocation.heap,
        bytes: allocation.bytes,
        ..Default::default()
    };
    placed_offset(&wanted).is_some_and(|found| found.offset == current.offset)
}
/// Unprotected: the resource has no protected session (an interface without ID3D12Resource1 has none).
pub(crate) fn unprotected(r: &ID3D12Resource) -> bool {
    let extended = match r.cast::<ID3D12Resource1>() {
        Ok(v) => v,
        Err(e) => return e.code() == E_NOINTERFACE,
    };
    let mut session = core::ptr::null_mut();
    // SAFETY: a live ID3D12Resource1; the session (if any) is released below.
    let result = unsafe {
        (extended.vtable().GetProtectedResourceSession)(
            extended.as_raw(),
            &IID_IUNKNOWN,
            &mut session,
        )
    };
    let present = !session.is_null();
    if present {
        // SAFETY: the reference GetProtectedResourceSession returned.
        drop(unsafe { IUnknown::from_raw(session) });
    }
    !present && (result == DXGI_ERROR_NOT_FOUND || result == E_NOINTERFACE || result.is_ok())
}
pub(crate) fn same_device(
    r: &ID3D12Resource,
    l: &ID3D12GraphicsCommandList,
) -> Option<ID3D12Device> {
    let device = device_of(r)?;
    let actual = device_of(l)?;
    // SAFETY: a live device.
    (device.as_raw() == actual.as_raw() && unsafe { device.GetNodeCount() } == 1).then_some(device)
}
pub(crate) fn required_bytes(device: &ID3D12Device, r: &ID3D12Resource) -> u64 {
    // SAFETY: live objects.
    unsafe { device.GetResourceAllocationInfo(0, &[r.GetDesc()]) }.SizeInBytes
}
pub(crate) fn observe() -> bool {
    crate::stereo_probe::observe_bindings()
}
pub(crate) fn receipt_for(resource_ptr: *mut c_void, registry: u64) -> u64 {
    let mut code = 0;
    let receipt = acquire_receipt(resource_ptr, registry, &mut code);
    fail(code);
    receipt
}

/// AcquireImmutable: a fixed asset bound on this list as an immutable lease -- its native allocation and allocator
/// pinned, its creation receipt acquired.
fn acquire_immutable(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    out: &mut ImmutableLease,
) -> bool {
    fail(85);
    let (Some(l), Some(r)) = (list_of(&list), resource(&resource_ptr)) else {
        return false;
    };
    // SAFETY (whole function): live objects.
    if !observe()
        || !compatible()
        || !fixed_compatible()
        || !immutable_shape(&unsafe { r.GetDesc() })
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || !default_heap(r)
        || !unprotected(r)
    {
        return false;
    }
    let base = game();
    let context = current_context(base);
    fail(86);
    if context.list != list as u64 || !fixed_context(&context) {
        return false;
    }
    let Some(native) = resolve_native(r) else {
        return false;
    };
    fail(87);
    let fixed = fixed_owner(&native);
    if fixed == 0
        || read_state(
            &native,
            context.context,
            context.list,
            READ,
            core::ptr::null_mut(),
        )
        .status
            != 8
    {
        return false;
    }
    let receipt = receipt_for(resource_ptr, native.registry);
    if receipt == 0 {
        return false;
    }
    let mut owner = Held::new(empty_owner());
    {
        let o = owner.get_mut();
        o.held = Some(r.clone());
        o.source = Lease {
            resource: resource_ptr,
            native,
            receipt,
            owner: fixed,
        };
        o.allocation = read_allocation(&reader(READ, core::ptr::null_mut()), base, fixed);
        o.allocator = read_allocator(
            &reader(READ, core::ptr::null_mut()),
            base,
            native.registry,
            &o.allocation,
        );
    }
    fail(88);
    let o = owner.get();
    if o.allocation.observation != KNOWN
        || o.allocation.resource != native.resource
        || o.allocation.kind > 1
        || o.allocator.observation != KNOWN
        || !o.allocator.pimpl_matches
    {
        return false;
    }
    fail(89);
    let Some(creation) = query_creation(resource_ptr) else {
        return false;
    };
    owner.get_mut().creation = creation;
    let Some(device) = same_device(r, l) else {
        return false;
    };
    let required = required_bytes(&device, r);
    if required == 0 || required == u64::MAX || owner.get().allocation.bytes < required {
        return false;
    }
    {
        let o = owner.get_mut();
        o.allocator_pin = retain_native(o.allocator.address);
        o.allocation_pin = retain_native(o.allocation.address);
    }
    fail(90);
    if !physical(owner.get())
        || !validate(&owner.get().source, core::ptr::null_mut())
        || !fixed_context(&context)
        || current_context(base) != context
    {
        return false;
    }
    *out = lease_out(&owner);
    fail(0);
    true
}
/// ValidateImmutable: the lease is its owner's, and the owner's lease and pinned allocation still hold.
pub(crate) fn validate_immutable(value: &ImmutableLease, queue: *mut c_void) -> bool {
    fail(93);
    let Some(owner) = owner_of(value.owner) else {
        return false;
    };
    let saved = &owner.get().source;
    let current = &value.source;
    if saved.resource != current.resource
        || saved.native.registry != current.native.registry
        || saved.native.resource != current.native.resource
        || saved.native.handle != current.native.handle
        || saved.receipt != current.receipt
        || saved.owner != current.owner
        || !validate(saved, queue)
        || !physical(owner.get())
    {
        return false;
    }
    fail(0);
    true
}
/// GetImmutablePageable: the lease's physical-memory owner -- the committed resource, or the exact placed heap.
pub(crate) fn immutable_pageable(
    device: *mut c_void,
    value: &ImmutableLease,
    out: &mut ImmutablePageable,
) -> bool {
    fail(100);
    // SAFETY: a live device of the caller's, or null.
    let Some(d) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return false;
    };
    // SAFETY: a live device.
    if unsafe { d.GetNodeCount() } != 1 || !validate_immutable(value, core::ptr::null_mut()) {
        return false;
    }
    fail(101);
    let Some(owner) = owner_of(value.owner) else {
        return false;
    };
    let o = owner.get();
    // The C++ upcasts (no QueryInterface).
    let pageable: Option<ID3D12Pageable> = match o.creation.kind {
        1 => o.held.as_ref().map(|r| -> ID3D12Pageable { (**r).clone() }),
        2 => o
            .creation
            .heap
            .as_ref()
            .map(|h| -> ID3D12Pageable { (**h).clone() }),
        _ => return false,
    };
    fail(102);
    let Some(pageable) = pageable else {
        return false;
    };
    let Some(r) = o.held.as_ref() else {
        return false;
    };
    let (Some(resource_device), Some(pageable_device)) = (device_of(r), device_of(&pageable))
    else {
        return false;
    };
    if resource_device.as_raw() != device || pageable_device.as_raw() != device {
        return false;
    }
    if !validate_immutable(value, core::ptr::null_mut()) {
        return false;
    }
    *out = ImmutablePageable {
        source: ImmutableLease {
            owner: add_ref(value.owner),
            source: Lease {
                resource: add_ref(value.source.resource),
                ..value.source
            },
        },
        pageable: pageable.into_raw(),
    };
    fail(0);
    true
}
type Register = unsafe extern "C" fn(*mut c_void, u32, u32, u32);
fn recording_context(c: &UploadContext) -> RecordingContext {
    RecordingContext {
        context: c.context,
        state: c.state,
        list: c.list,
    }
}
/// RegisterImmutableUse: the fixed asset registered for shader reads on the original native recording -- added to the
/// native residency/use set -- and kept alive with the list.
fn register_immutable_use(
    list: *mut c_void,
    source: &ImmutableLease,
    out: &mut ImmutableUse,
) -> bool {
    fail(96);
    let Some(l) = list_of(&list) else {
        return false;
    };
    if cvr_host::probe::internal() || !validate_immutable(source, core::ptr::null_mut()) {
        return false;
    }
    let base = game();
    let context = current_context(base);
    let native_context = recording_context(&context);
    let mut recording = Recording {
        token: core::ptr::null_mut(),
        generation: 0,
        context: 0,
        list: 0,
        registry: 0,
    };
    fail(97);
    let ok = context.list == list as u64
        && fixed_context(&context)
        // SAFETY: valid records; the token returned is this function's reference.
        && unsafe { cvr_native_recording_capture(list, &native_context, &mut recording) };
    // The token's reference, released unless it moves into the output.
    let token = (!recording.token.is_null())
        // SAFETY: the reference Capture returned.
        .then(|| unsafe { IUnknown::from_raw(recording.token) });
    if !ok || recording.registry != source.source.native.registry {
        return false;
    }
    let handle = source.source.native.handle;
    {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the engine's read registration, fingerprinted by compatible().
        unsafe {
            core::mem::transmute::<usize, Register>(base as usize + 0x337264)(
                context.context as *mut c_void,
                handle,
                SHADER_READS,
                u32::MAX,
            )
        };
    }
    let offset = 0x2F1D8 + (handle as u64 - 1) * 0xB0;
    fail(98);
    if recording.registry > u64::MAX - offset - 0xB0 || context.context > u64::MAX - 0x638 {
        return false;
    }
    let address = recording.registry + offset;
    let Some(resident) = read_value::<u64>(address + 0x68) else {
        return false;
    };
    if resident != 0 {
        if resident != source.source.native.resource {
            return false;
        }
        let tracker = read_value::<u64>(context.context + 0x630).unwrap_or(0);
        if tracker == 0 || tracker > u64::MAX - 38 || read_value::<u8>(tracker + 37) != Some(0) {
            return false;
        }
    }
    let token_ptr = token.as_ref().map_or(core::ptr::null_mut(), |t| t.as_raw());
    // SAFETY: valid records.
    if current_context(base) != context
        || !fixed_context(&context)
        || !validate_immutable(source, core::ptr::null_mut())
        || !unsafe { cvr_native_recording_validate_recording(&recording, list, &native_context) }
        || !cvr_gpu::command_resources::keep(l, &[source.owner, token_ptr])
    {
        return false;
    }
    *out = ImmutableUse {
        source: ImmutableLease {
            owner: add_ref(source.owner),
            source: Lease {
                resource: add_ref(source.source.resource),
                ..source.source
            },
        },
        recording: Recording {
            token: token.map_or(core::ptr::null_mut(), |t| t.into_raw()),
            ..recording
        },
    };
    fail(0);
    true
}

// ---- the C ABI (src/Render/NativeTextureRead.cpp in the Rust build; the outputs are the C++ records, zeroed by the
// caller, receiving references)

/// # Safety
/// `list` and `resource` live or null; `out` a zeroed ImmutableLease.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_acquire_immutable(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    out: *mut ImmutableLease,
) -> bool {
    // SAFETY: as the caller promises.
    acquire_immutable(list, resource_ptr, unsafe { &mut *out })
}
/// # Safety
/// `value` is valid; `queue` live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_validate_immutable(
    value: *const ImmutableLease,
    queue: *mut c_void,
) -> bool {
    // SAFETY: as the caller promises.
    validate_immutable(unsafe { &*value }, queue)
}
/// # Safety
/// `device` live or null; `value` valid; `out` a zeroed ImmutablePageable.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_immutable_pageable(
    device: *mut c_void,
    value: *const ImmutableLease,
    out: *mut ImmutablePageable,
) -> bool {
    // SAFETY: as the caller promises.
    immutable_pageable(device, unsafe { &*value }, unsafe { &mut *out })
}
/// # Safety
/// `list` live or null; `source` valid; `out` a zeroed ImmutableUse.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_register_immutable_use(
    list: *mut c_void,
    source: *const ImmutableLease,
    out: *mut ImmutableUse,
) -> bool {
    // SAFETY: as the caller promises.
    register_immutable_use(list, unsafe { &*source }, unsafe { &mut *out })
}
/// ValidateImmutableUse: the use's recording was submitted on this queue and its lease still holds.
///
/// # Safety
/// `value` is valid; `queue` live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_validate_immutable_use(
    value: *const ImmutableUse,
    queue: *mut c_void,
) -> bool {
    // SAFETY: as the caller promises.
    let value = unsafe { &*value };
    !queue.is_null()
        && !value.recording.token.is_null()
        && unsafe { cvr_native_recording_validate_submission(&value.recording, queue) }
        && validate_immutable(&value.source, queue)
}
