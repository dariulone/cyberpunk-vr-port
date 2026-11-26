//! The descriptor audit's heap and resource lifetimes (src/Render/StereoDescriptorAudit.cpp, `descriptors::`): a known
//! descriptor heap or SRV/UAV resource carries a private lifetime object (an IUnknown in its private data) whose last
//! release -- possibly under D3D12's own object lock -- only pushes it on a retired list; the next metadata operation
//! drains the lists, removing a retired heap's slots from the store (the caller's callback) and forgetting a retired
//! resource's cookie unless a newer object holds it. At most 131072 tracked resources; a cookie already held by another
//! resource is refused.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's DrainRetired, TrackSrvResource and RegisterHeap's lifetime half forward
//! to `cvr_sda_*`; `tools/single_pass_tests/descriptor_audit_test` runs its cases against them in its Rust variant.

use cvr_host::fast::{FastMap, FastSet};
use core::ffi::c_void;

use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{ID3D12DescriptorHeap, ID3D12Resource};

const HEAP_LIFETIME_GUID: GUID = GUID::from_values(0xc67d21d0, 0x4d42, 0x4526, [0x82, 0x30, 0x79, 0xaf, 0x13, 0x01, 0x6c, 0x07]);
const RESOURCE_LIFETIME_GUID: GUID = GUID::from_values(0x35d4c78a, 0x4e19, 0x4c02, [0x91, 0x6d, 0xac, 0x83, 0xc1, 0x74, 0x8d, 0x28]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const RESOURCE_LIMIT: usize = 131072;

#[repr(C)]
struct Lifetime {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    next: *mut Lifetime,
    cookie: u64,
    resource: usize,
}
static RETIRED_HEAPS: AtomicPtr<Lifetime> = AtomicPtr::new(core::ptr::null_mut());
static RETIRED_RESOURCES: AtomicPtr<Lifetime> = AtomicPtr::new(core::ptr::null_mut());
unsafe extern "system" fn lifetime_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live lifetime.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Lifetime)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn lifetime_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live lifetime.
    unsafe { (*(this as *mut Lifetime)).refs.fetch_add(1, SeqCst) + 1 }
}
/// D3D12 may release private data under its own object lock: the last release only retires.
unsafe fn retire(this: *mut c_void, stack: &AtomicPtr<Lifetime>) -> u32 {
    let lifetime = this as *mut Lifetime;
    // SAFETY: a live lifetime; at zero it is pushed on its retired stack.
    unsafe {
        let n = (*lifetime).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let mut next = stack.load(Relaxed);
            loop {
                (*lifetime).next = next;
                match stack.compare_exchange_weak(next, lifetime, Release, Relaxed) {
                    Ok(_) => break,
                    Err(actual) => next = actual,
                }
            }
        }
        n
    }
}
unsafe extern "system" fn heap_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { retire(this, &RETIRED_HEAPS) }
}
unsafe extern "system" fn resource_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { retire(this, &RETIRED_RESOURCES) }
}
static HEAP_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: lifetime_query, AddRef: lifetime_add_ref, Release: heap_release };
static RESOURCE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: lifetime_query, AddRef: lifetime_add_ref, Release: resource_release };

#[derive(Default)]
struct Registry {
    heaps: FastSet<u64>,
    resources: FastMap<u64, *mut Lifetime>,
}
// SAFETY: the raw lifetimes are reached only under the registry's lock.
unsafe impl Send for Registry {}
static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);
fn registry() -> MutexGuard<'static, Option<Registry>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// The store's removal of a retired heap's slots.
pub type HeapRemoved = unsafe extern "C-unwind" fn(cookie: u64);

fn drain_locked(r: &mut Registry, removed: HeapRemoved) {
    let mut head = RETIRED_HEAPS.swap(core::ptr::null_mut(), Acquire);
    while !head.is_null() {
        // SAFETY: a retired heap lifetime (refcount 0), deleted once here.
        unsafe {
            let next = (*head).next;
            let cookie = (*head).cookie;
            removed(cookie);
            r.heaps.remove(&cookie);
            drop(Box::from_raw(head));
            head = next;
        }
    }
    let mut resource = RETIRED_RESOURCES.swap(core::ptr::null_mut(), Acquire);
    while !resource.is_null() {
        // SAFETY: a retired resource lifetime (refcount 0), deleted once here.
        unsafe {
            let next = (*resource).next;
            let cookie = (*resource).cookie;
            if r.resources.get(&cookie) == Some(&resource) {
                r.resources.remove(&cookie);
            }
            drop(Box::from_raw(resource));
            resource = next;
        }
    }
}
/// A new lifetime object attached to the object's private data under `guid` (the object takes its own reference).
fn attach(set: impl FnOnce(&IUnknown) -> bool, vtbl: &'static IUnknown_Vtbl, cookie: u64, resource: usize) -> Option<*mut Lifetime> {
    let lifetime = Box::into_raw(Box::new(Lifetime { vtbl, refs: AtomicU32::new(1), next: core::ptr::null_mut(), cookie, resource }));
    // SAFETY: the new object as an IUnknown, holding its creation reference (dropped at the end of this call).
    let owner = unsafe { IUnknown::from_raw(lifetime.cast()) };
    set(&owner).then_some(lifetime)
}

/// DrainRetired.
pub fn drain(removed: HeapRemoved) {
    let mut guard = registry();
    drain_locked(guard.get_or_insert_with(Registry::default), removed);
}
/// TrackSrvResource: the resource holds `cookie`'s lifetime (true for no resource).
pub fn track_resource(raw: *mut c_void, cookie: u64, removed: HeapRemoved) -> bool {
    if raw.is_null() {
        return true;
    }
    let mut guard = registry();
    let r = guard.get_or_insert_with(Registry::default);
    drain_locked(r, removed);
    if cookie == 0 {
        return false;
    }
    if let Some(&found) = r.resources.get(&cookie) {
        // SAFETY: a registered (live) lifetime.
        return unsafe { (*found).resource } == raw as usize;
    }
    if r.resources.len() >= RESOURCE_LIMIT {
        return false;
    }
    // SAFETY: a live resource of the caller's.
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return false;
    };
    // SAFETY: the resource keeps its own reference to the lifetime.
    let lifetime = attach(|owner| unsafe { resource.SetPrivateDataInterface(&RESOURCE_LIFETIME_GUID, owner) }.is_ok(), &RESOURCE_VTBL, cookie, raw as usize);
    match lifetime {
        Some(lifetime) => {
            r.resources.insert(cookie, lifetime);
            true
        }
        None => false,
    }
}
/// Whether the heap's cookie is known.
pub fn heap_known(cookie: u64) -> bool {
    registry().as_ref().is_some_and(|r| r.heaps.contains(&cookie))
}
/// RegisterHeap's lifetime half: an unknown heap's lifetime object attached (false when it cannot be).
pub fn heap_attach(raw: *mut c_void, cookie: u64) -> bool {
    if heap_known(cookie) {
        return true;
    }
    // SAFETY: a live heap of the caller's, or null.
    let Some(heap) = (unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&raw) }) else {
        return false;
    };
    // SAFETY: the heap keeps its own reference to the lifetime.
    attach(|owner| unsafe { heap.SetPrivateDataInterface(&HEAP_LIFETIME_GUID, owner) }.is_ok(), &HEAP_VTBL, cookie, 0).is_some()
}
/// The heap's slots are in the store: its cookie is known.
pub fn heap_insert(cookie: u64) {
    registry().get_or_insert_with(Registry::default).heaps.insert(cookie);
}

/// The registered resource of `cookie` (its pointer, alive or retired until a drain forgets it).
pub fn registered_resource(cookie: u64) -> Option<usize> {
    let guard = registry();
    let lifetime = *guard.as_ref()?.resources.get(&cookie)?;
    // SAFETY: a registered lifetime, deleted only by a drain under the registry lock held here.
    Some(unsafe { (*lifetime).resource })
}
/// A tracked resource's cookie (alive or retired, until a drain forgets it).
pub fn resource_known(cookie: u64) -> bool {
    registry().as_ref().is_some_and(|r| r.resources.contains_key(&cookie))
}
/// The tracked resource of `cookie` while its lifetime is alive (its pointer, not a reference).
pub fn live_resource(cookie: u64) -> Option<*mut c_void> {
    let guard = registry();
    let lifetime = *guard.as_ref()?.resources.get(&cookie)?;
    // SAFETY: a registered lifetime, deleted only by a drain under the registry lock held here.
    let lifetime = unsafe { &*lifetime };
    (lifetime.refs.load(Acquire) != 0 && lifetime.resource != 0).then_some(lifetime.resource as *mut c_void)
}

// ---- the C ABI

/// # Safety
/// `removed` is the .cpp's callback.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_drain(removed: HeapRemoved) {
    drain(removed);
}
/// # Safety
/// The resource is live or null; `removed` is the .cpp's callback.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_track_resource(resource: *mut c_void, cookie: u64, removed: HeapRemoved) -> bool {
    track_resource(resource, cookie, removed)
}
/// `which` 0: known; 1: attach (the heap live); 2: insert.
///
/// # Safety
/// For attach the heap is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_heap(which: u32, heap: *mut c_void, cookie: u64) -> bool {
    match which {
        0 => heap_known(cookie),
        1 => heap_attach(heap, cookie),
        _ => {
            heap_insert(cookie);
            true
        }
    }
}
/// The live lifetime of the resource holding `cookie`, or null (valid while the .cpp's metadata mutex is held).
///
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_resource_find(cookie: u64) -> *const c_void {
    registry().as_ref().and_then(|r| r.resources.get(&cookie).copied()).map_or(core::ptr::null(), |p| p as *const c_void)
}
const _: () = assert!(core::mem::size_of::<Lifetime>() == 40);