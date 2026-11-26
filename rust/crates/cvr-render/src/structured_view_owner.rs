//! The structured snapshot view's overlay owner (formerly src/Render/StructuredSnapshotView.cpp's Overlay::Storage and
//! the overlay's methods there): a COM object -- its stage, device, own buffer (a created overlay) or none (an adopted
//! one), plan (structured_view_overlay.rs's, shared with the C++ Plan when created from it) and the destination it was
//! recorded into -- queried by its type when a private view's owner is validated. The .cpp's Overlay class forwards
//! here; the plan's checks, the buffer and the commands are structured_view_overlay.rs's.

use crate::structured_view_overlay::{failure, Destination, Plan, PrivateView};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering::*};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandAllocator, ID3D12CommandQueue, ID3D12Device, ID3D12DeviceChild, ID3D12GraphicsCommandList, ID3D12Resource};

/// The owner's interface (the C++ Storage's ownerType).
const OWNER_TYPE: GUID = GUID::from_values(0x2ae6e4c2, 0xf181, 0x4921, [0xb0, 0xa4, 0x69, 0x90, 0x74, 0x20, 0x7a, 0x34]);
/// Stage: Created, Recording, Recorded, Failed.
const CREATED: u32 = 0;
const RECORDING: u32 = 1;
const RECORDED: u32 = 2;
const FAILED: u32 = 3;
const ARGUMENTS: u32 = 1;
const RETENTION: u32 = 13;
const EXCEPTION: u32 = 15;

/// The destination a created overlay was recorded into (DestinationRecording, its objects held).
#[derive(Default)]
struct Recorded {
    list: u64,
    generation: u64,
    allocator: Option<ID3D12CommandAllocator>,
    queue: Option<ID3D12CommandQueue>,
}
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    stage: AtomicU32,
    adopted: bool,
    device: ID3D12Device,
    resource: Option<ID3D12Resource>,
    plan: Arc<Plan>,
    /// Written once, while the stage is Recording; read once it is Recorded.
    destination: UnsafeCell<Recorded>,
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers; a live owner.
    unsafe {
        *out = null_mut();
        if iid.is_null() || (*iid != IUnknown::IID && *iid != OWNER_TYPE) {
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
fn new_owner(device: ID3D12Device, resource: Option<ID3D12Resource>, plan: Arc<Plan>, adopted: bool, stage: u32) -> *mut c_void {
    Box::into_raw(Box::new(Owner {
        vtbl: &OWNER_VTBL,
        references: AtomicU32::new(1),
        stage: AtomicU32::new(stage),
        adopted,
        device,
        resource,
        plan,
        destination: UnsafeCell::new(Recorded::default()),
    }))
    .cast()
}
impl Owner {
    fn recorded(&self) -> &Recorded {
        // SAFETY: written before the stage became Recorded (release), read after observing it (acquire) or never.
        unsafe { &*self.destination.get() }
    }
    fn resource_raw(&self) -> *mut c_void {
        self.resource.as_ref().map_or(null_mut(), Interface::as_raw)
    }
}
/// The face's own owner pointer (its ComPtr's), borrowed.
fn owner<'a>(storage: *mut c_void) -> Option<&'a Owner> {
    // SAFETY: an owner this module made, held by the face.
    unsafe { storage.cast::<Owner>().as_ref() }
}
/// A held owner reference.
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
/// The owner behind a view's IUnknown (QueryInterface for the owner type), held.
fn owner_of(value: *mut c_void) -> Option<OwnerRef> {
    // SAFETY: the view's live owner or null.
    let value = unsafe { IUnknown::from_raw_borrowed(&value) }?;
    let mut out = null_mut();
    // SAFETY: a QueryInterface for the owner type.
    let hr = unsafe { (Interface::vtable(value).QueryInterface)(value.as_raw(), &OWNER_TYPE, &mut out) };
    (hr.is_ok() && !out.is_null()).then(|| OwnerRef(out))
}
fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
/// SameDevice: the child's device is the owner's.
fn same_device(device: &ID3D12Device, child: *mut c_void) -> bool {
    let Some(child) = borrowed::<ID3D12DeviceChild>(child) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output ours.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|actual| actual.as_raw() == device.as_raw())
}
/// A plan pointer of the C++ PlanData's (one Arc reference), retained.
fn retain(plan: *const Plan) -> Arc<Plan> {
    // SAFETY: the C++ PlanData's live Arc reference (structured_view_overlay.rs's cvr_sov_plan_new).
    unsafe {
        Arc::increment_strong_count(plan);
        Arc::from_raw(plan)
    }
}

// ---- the C ABI (the .cpp's Overlay class)

/// Overlay::Create: a created overlay over the plan (shared), its buffer made under the internal-commands scope.
///
/// # Safety
/// `device` live; `plan` the C++ PlanData's; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_create(device: *mut c_void, plan: *const Plan, out: *mut *mut c_void) -> HRESULT {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
        if plan.is_null() {
            return E_INVALIDARG;
        }
        let plan = retain(plan);
        let created = {
            let _internal = cvr_host::probe::Internal::new();
            plan.create(&device)
        };
        match created {
            Ok(resource) => {
                // SAFETY: as documented.
                unsafe { *out = new_owner(device, Some(resource), plan, false, CREATED) };
                S_OK
            }
            Err(hr) => HRESULT(hr),
        }
    }))
    .unwrap_or(E_OUTOFMEMORY)
}
/// Overlay::AdoptSources after BuildPlan: the checked plan adopted (a Failure; the owner Recorded).
///
/// # Safety
/// `device` live; `checked` the C++ PlanData's; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_adopt(device: *mut c_void, checked: *const Plan, out: *mut *mut c_void) -> u32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(device) = borrowed::<ID3D12Device>(device) else { return failure::DEVICE };
        // SAFETY: as documented.
        match unsafe { (*checked).adopt(&device) } {
            Ok(plan) => {
                // SAFETY: as documented.
                unsafe { *out = new_owner(device, None, Arc::new(plan), true, RECORDED) };
                failure::NONE
            }
            Err(code) => code,
        }
    }))
    .unwrap_or(EXCEPTION)
}
/// Overlay::Record: the plan's checks, the command resources kept, the copies recorded once (a Failure).
///
/// # Safety
/// The face's owner; a live list; `destination` the C++ DestinationRecording.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_record(storage: *mut c_void, list: *mut c_void, destination: *const Destination) -> u32 {
    let Some(o) = owner(storage) else { return ARGUMENTS };
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) else { return ARGUMENTS };
    // SAFETY: as documented.
    let d = unsafe { &*destination };
    catch_unwind(AssertUnwindSafe(|| {
        let code = o.plan.record_check(&o.device, &list, d, o.stage.load(Acquire) == CREATED, o.resource_raw());
        if code != failure::NONE {
            return code;
        }
        if !cvr_gpu::command_resources::keep(&list, &[storage, d.allocator, d.queue]) {
            return RETENTION;
        }
        if o.stage.compare_exchange(CREATED, RECORDING, AcqRel, Acquire).is_err() {
            return failure::ALREADY_RECORDED;
        }
        // SAFETY: the Recording stage is this call's alone.
        unsafe {
            *o.destination.get() = Recorded {
                list: d.list,
                generation: d.generation,
                allocator: borrowed::<ID3D12CommandAllocator>(d.allocator),
                queue: borrowed::<ID3D12CommandQueue>(d.queue),
            };
        }
        if let Some(target) = &o.resource {
            let _internal = cvr_host::probe::Internal::new();
            o.plan.record(&list, target);
        }
        o.stage.store(RECORDED, Release);
        failure::NONE
    }))
    .unwrap_or_else(|_| {
        if o.stage.load(SeqCst) == RECORDING {
            o.stage.store(FAILED, Release);
        }
        EXCEPTION
    })
}
/// Overlay::GetView: the view (its owners' fields left to the face) and its resource (the dense source buffer of an
/// adopted overlay, else the overlay's own), borrowed.
///
/// # Safety
/// The face's owner; `out` a zeroed view; `resource` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_view(storage: *mut c_void, index: u32, out: *mut PrivateView, resource: *mut *mut c_void) -> bool {
    let Some(o) = owner(storage) else { return false };
    if o.stage.load(Acquire) != RECORDED {
        return false;
    }
    let r = o.recorded();
    let Some((view, dense)) = o.plan.view(index, o.adopted, r.list, r.generation) else { return false };
    // SAFETY: as documented: the owners' fields are written as they were (null).
    unsafe {
        let (held_owner, held_resource) = ((*out).owner, (*out).resource);
        *out = PrivateView { owner: held_owner, resource: held_resource, ..view };
        *resource = if o.adopted { dense } else { o.resource_raw() };
    }
    true
}
/// VersionCount (0), RequiredBytes (1), CopiedBytes (2).
///
/// # Safety
/// The face's owner.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_count(storage: *mut c_void, which: u32) -> u64 {
    let Some(o) = owner(storage) else { return 0 };
    match which {
        0 => o.plan.count() as u64,
        1 => o.plan.width,
        _ => o.plan.copied,
    }
}
/// Overlay::ValidateDestinationSubmission.
///
/// # Safety
/// The face's owner; `destination` the C++ DestinationRecording; a live queue or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_submitted(storage: *mut c_void, destination: *const Destination, queue: *mut c_void) -> bool {
    let Some(o) = owner(storage) else { return false };
    if o.adopted || o.stage.load(Acquire) != RECORDED {
        return false;
    }
    // SAFETY: as documented.
    let d = unsafe { &*destination };
    let actual = o.recorded();
    let raw = |x: &Option<ID3D12CommandQueue>| x.as_ref().map_or(null_mut(), Interface::as_raw);
    d.certified
        && d.list == actual.list
        && d.generation == actual.generation
        && d.allocator == actual.allocator.as_ref().map_or(null_mut(), Interface::as_raw)
        && d.queue == raw(&actual.queue)
        && queue == raw(&actual.queue)
        && same_device(&o.device, queue)
}
/// ValidatePrivateView: the view's owner is a recorded overlay whose plan the view matches.
fn validate(view: &PrivateView) -> Option<OwnerRef> {
    let held = owner_of(view.owner)?;
    let o = held.get();
    if o.stage.load(Acquire) != RECORDED {
        return None;
    }
    let r = o.recorded();
    o.plan.matches(view, o.adopted, o.resource_raw(), r.list, r.generation).then_some(held)
}
/// # Safety
/// `view` the C++ PrivateView.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_validate(view: *const PrivateView) -> bool {
    // SAFETY: as documented.
    validate(unsafe { &*view }).is_some()
}
/// ValidatePrivateViewForRecording: a created overlay's view, recorded into this list's destination and kept by it.
///
/// # Safety
/// `view` and `destination` the C++ objects; `list` live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov2_validate_recording(view: *const PrivateView, list: *mut c_void, destination: *const Destination) -> bool {
    // SAFETY: as documented.
    let (view, d) = unsafe { (&*view, &*destination) };
    let Some(held) = validate(view) else { return false };
    let Some(list_ref) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    if view.copy_list != list as u64 || d.list != view.copy_list || d.generation != view.copy_generation || !d.certified {
        return false;
    }
    let o = held.get();
    let r = o.recorded();
    !o.adopted
        && d.allocator == r.allocator.as_ref().map_or(null_mut(), Interface::as_raw)
        && d.queue == r.queue.as_ref().map_or(null_mut(), Interface::as_raw)
        && cvr_gpu::command_resources::has(&list_ref, held.0)
        && same_device(&o.device, list)
}
