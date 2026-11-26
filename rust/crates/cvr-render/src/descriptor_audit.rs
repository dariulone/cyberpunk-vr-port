//! The descriptor audit's core (formerly src/Render/StereoDescriptorAudit.cpp's state and its write, capture and
//! resolve paths): the metadata lock (a recursive critical section, as the .cpp's), the capture/bindings/live-metadata
//! switches, the descriptor strides and the tracking failure; every descriptor creation (CBV with its creation origin,
//! SRV, UAV with its counter, sampler) and copy written into the store, or only invalidating the tracked ranges while
//! internal commands or borrowed pins hold; the heaps registered; the root layouts recorded; the draw capture, the
//! bindings window, the tracked tables per list, the submissions and resets; the binding lookups and the CBV, SRV,
//! sampler and stamp resolutions at a draw.
//!
//! The .cpp keeps the store and the borrowed-range tracking objects (lent here at attach), the table metadata,
//! borrowed-range and source-table captures (which take this lock and read this state through `cvr_sda_audit_*`), and
//! the forwarders; the exports, the native probe's switches and the observation profile are lent through the runtime.

use crate::audit_capture::{Draw, Layout, View};
use crate::audit_roots::{RootInfo, RootView};
use crate::descriptor_store::{BorrowedRangeTracking, RangeIdentity, Store, Value};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::OnceLock;
use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;
use windows_sys::Win32::System::Threading::{EnterCriticalSection, InitializeCriticalSectionEx, LeaveCriticalSection, CRITICAL_SECTION};

/// The object cookie's private-data key (the .cpp's cookieGuid).
pub const COOKIE_GUID: GUID = GUID::from_values(0xc7ec4301, 0x790e, 0x4b32, [0x93, 0x87, 0x7c, 0xc1, 0x19, 0x52, 0xd1, 0x44]);

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    pub store: *mut Store,
    pub tracking: *const BorrowedRangeTracking,
    /// native_probe::ObserveBindings.
    pub observe_bindings: extern "C" fn() -> bool,
    pub native_probe_state: *const AtomicU32,
    pub draw_capacity: usize,
    pub capture_failure: *const AtomicU32,
    pub capture_seq: *const AtomicU32,
    pub dense_slots: *const AtomicU32,
    pub dense_active: *const AtomicU32,
    pub live_metadata: *const AtomicU32,
    pub live_metadata_active: *const AtomicU32,
    pub pending_lists: *const AtomicU32,
    pub root_count: *mut u32,
    pub roots: *mut RootInfo,
    pub roots_capacity: usize,
    pub store_stats: *mut [u64; 6],
    pub heap_slots: *mut u64,
    pub draws: *mut u64,
    pub draw_count: *const AtomicU32,
    pub bindings: *mut u64,
    pub binding_count: *const AtomicU32,
    pub ids: *mut u64,
    pub id_count: *const AtomicU32,
    pub values: *mut u64,
    pub value_count: *const AtomicU32,
    /// observation_profile's switch and its 16 stats.
    pub profile_enable: *const AtomicU32,
    pub profile_stats: *const AtomicU64,
}
// SAFETY: immutable; its pointers name process-lifetime objects, exports and functions.
unsafe impl Sync for Runtime {}
/// The plugin's runtime (formerly lent by StereoDescriptorAudit.cpp): the store (made here once, dense, as the .cpp's
/// static Store was), the borrowed-range tracking, the probe's switch and the exports. The Rust test variants attach
/// the .cpp's.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::Runtime;
    use crate::audit_roots::RootInfo;
    use crate::declared_moved_exports::*;
    use crate::descriptor_store::BorrowedRangeTracking;
    use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
    use std::sync::OnceLock;
    #[allow(non_upper_case_globals)]
    extern "C" {
        /// StereoDescriptorAudit.cpp's exported roots.
        static mut CyberpunkVR_DescriptorRoots: [RootInfo; 128];
        /// include/Render/ObservationProfile.hpp's exported switch and stats.
        static CyberpunkVR_ObservationProfileEnable: AtomicU32;
        static CyberpunkVR_ObservationProfileStats: [AtomicU64; 16];
    }
    static TRACKING: BorrowedRangeTracking = BorrowedRangeTracking {
        epoch: AtomicU64::new(1),
        pins: AtomicU32::new(0),
        mutations: AtomicU32::new(0),
        ready: AtomicBool::new(false),
        unavailable: AtomicBool::new(false),
    };
    extern "C" fn observe_bindings() -> bool {
        crate::stereo_probe::observe_bindings()
    }
    struct Shared(Runtime);
    // SAFETY: as Runtime's.
    unsafe impl Sync for Shared {}
    // SAFETY: as above.
    unsafe impl Send for Shared {}
    pub(super) fn runtime() -> &'static Runtime {
        static RT: OnceLock<Shared> = OnceLock::new();
        &RT.get_or_init(|| {
            Shared(Runtime {
                store: crate::descriptor_store::cvr_ds_new(true),
                tracking: &TRACKING,
                observe_bindings,
                native_probe_state: &raw const CyberpunkVR_NativeStereoProbeState,
                draw_capacity: crate::stereo_probe_state::DRAW_CAPACITY,
                capture_failure: &raw const CyberpunkVR_DescriptorCaptureFailure,
                capture_seq: &raw const CyberpunkVR_DescriptorCaptureSeq,
                dense_slots: &raw const CyberpunkVR_DescriptorDenseSlots,
                dense_active: &raw const CyberpunkVR_DescriptorDenseActive,
                live_metadata: &raw const CyberpunkVR_DescriptorLiveMetadata,
                live_metadata_active: &raw const CyberpunkVR_DescriptorLiveMetadataActive,
                pending_lists: &raw const CyberpunkVR_DescriptorPendingLists,
                root_count: &raw mut CyberpunkVR_DescriptorRootCount,
                roots: (&raw mut CyberpunkVR_DescriptorRoots).cast(),
                roots_capacity: 128,
                store_stats: &raw mut CyberpunkVR_DescriptorStoreStats,
                heap_slots: &raw mut CyberpunkVR_DescriptorHeapSlots,
                draws: &raw mut CyberpunkVR_DescriptorDraws,
                draw_count: &raw const CyberpunkVR_DescriptorDrawCount,
                bindings: &raw mut CyberpunkVR_DescriptorBindings,
                binding_count: &raw const CyberpunkVR_DescriptorBindingCount,
                ids: &raw mut CyberpunkVR_DescriptorIds,
                id_count: &raw const CyberpunkVR_DescriptorIdCount,
                values: &raw mut CyberpunkVR_DescriptorValues,
                value_count: &raw const CyberpunkVR_DescriptorValueCount,
                profile_enable: &raw const CyberpunkVR_ObservationProfileEnable,
                profile_stats: (&raw const CyberpunkVR_ObservationProfileStats).cast(),
            })
        })
        .0
    }
}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or_else(|| Some(plugin::runtime()));
    attached
}
static ZERO: AtomicU32 = AtomicU32::new(0);
macro_rules! a {
    ($field:ident) => {{
        // SAFETY: the runtime's export.
        #[allow(unused_unsafe)]
        let field = rt().and_then(|r| unsafe { r.$field.as_ref() }).unwrap_or(&ZERO);
        field
    }};
}
pub(crate) fn store() -> *mut Store {
    rt().map_or(core::ptr::null_mut(), |r| r.store)
}
pub(crate) fn tracking() -> Option<&'static BorrowedRangeTracking> {
    // SAFETY: the .cpp's tracking object.
    rt().and_then(|r| unsafe { r.tracking.as_ref() })
}
pub(crate) fn tracking_call(which: u32, value: u64) -> bool {
    // SAFETY: the .cpp's tracking object.
    rt().is_some_and(|r| unsafe { crate::descriptor_store::cvr_ds_tracking(r.tracking, which, value) })
}

// ---- the metadata lock (recursive, as the .cpp's critical section)
struct Lock(UnsafeCell<CRITICAL_SECTION>);
// SAFETY: a critical section is shared by design.
unsafe impl Sync for Lock {}
unsafe impl Send for Lock {}
static LOCK: OnceLock<Box<Lock>> = OnceLock::new();
fn lock_cell() -> &'static Lock {
    LOCK.get_or_init(|| {
        // SAFETY: zeroed storage the call initializes.
        let lock = Box::new(Lock(UnsafeCell::new(unsafe { core::mem::zeroed() })));
        unsafe { InitializeCriticalSectionEx(lock.0.get(), 4000, 0x0100_0000) };
        lock
    })
}
pub struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: entered by guard().
        unsafe { LeaveCriticalSection(lock_cell().0.get()) };
    }
}
pub fn guard() -> Guard {
    // SAFETY: an initialized critical section.
    unsafe { EnterCriticalSection(lock_cell().0.get()) };
    Guard
}

// ---- the state
static RECORDING: AtomicBool = AtomicBool::new(false);
static BINDING_ONLY: AtomicBool = AtomicBool::new(false);
static LIVE_METADATA: AtomicBool = AtomicBool::new(false);
static TRACKING_FAILURE: AtomicU32 = AtomicU32::new(0);
static RESOURCE_STRIDE: AtomicU32 = AtomicU32::new(0);
static SAMPLER_STRIDE: AtomicU32 = AtomicU32::new(0);
mod mode {
    pub const IGNORE: u32 = 0;
    pub const CANONICAL: u32 = 1;
    pub const INVALIDATE_TRACKED: u32 = 2;
}
mod phase {
    pub const CBV: u32 = 0;
    pub const OTHER_DESCRIPTOR: u32 = 1;
    pub const COPY_DESCRIPTORS: u32 = 2;
    pub const BIND_HEAPS: u32 = 3;
}
/// observation_profile::Scope.
struct Profile {
    index: usize,
    begin: i64,
    enabled: bool,
}
fn qpc() -> i64 {
    let mut q = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut q) };
    q
}
impl Profile {
    fn new(phase: u32) -> Self {
        // SAFETY: the runtime's export.
        let enabled = rt().and_then(|r| unsafe { r.profile_enable.as_ref() }).is_some_and(|e| e.load(Relaxed) != 0);
        Profile { index: phase as usize * 2, begin: if enabled { qpc() } else { 0 }, enabled }
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        if let (true, Some(stats)) = (self.enabled, rt().map(|r| r.profile_stats).filter(|s| !s.is_null())) {
            let end = qpc();
            // SAFETY: the 16 stats.
            unsafe {
                (*stats.add(self.index)).fetch_add(1, Relaxed);
                (*stats.add(self.index + 1)).fetch_add((end - self.begin) as u64, Relaxed);
            }
        }
    }
}

/// native_probe::ObserveBindings.
pub(crate) fn observe_bindings() -> bool {
    rt().is_some_and(|r| (r.observe_bindings)())
}
pub fn enabled() -> bool {
    rt().is_some_and(|r| (r.observe_bindings)()) && !cvr_host::probe::internal()
}
fn mutation_mode() -> u32 {
    let enabled = enabled();
    let pins = tracking().is_some_and(|t| t.pins.load(Acquire) != 0);
    crate::descriptor_store::cvr_ds_mutation_mode(cvr_host::probe::internal(), enabled, pins)
}
pub fn fail(reason: u32) {
    TRACKING_FAILURE.store(reason, SeqCst);
    a!(capture_failure).store(reason, SeqCst);
}
/// The tracking failure (the .cpp's trackingFailure).
pub(crate) fn tracking_failure() -> u32 {
    TRACKING_FAILURE.load(SeqCst)
}
/// The store's version tracking (Store::TrackVersions()).
pub(crate) fn track_versions() -> bool {
    // SAFETY: the store.
    unsafe { crate::descriptor_store::cvr_ds_query(store(), 2, 0) != 0 }
}
/// An object's cookie through its private data, 0 when absent or malformed (GetPrivateData(cookieGuid)).
pub(crate) fn object_cookie(object: &ID3D12Object) -> u64 {
    let (mut current, mut bytes) = (0u64, 8u32);
    // SAFETY: our 8-byte output.
    match unsafe { object.GetPrivateData(&COOKIE_GUID, &mut bytes, Some((&mut current as *mut u64).cast())) } {
        Ok(()) if bytes == 8 => current,
        _ => 0,
    }
}
fn pending_lists() -> usize {
    // SAFETY: a count query.
    unsafe { crate::audit_capture::cvr_sda_capture_state(1, core::ptr::null_mut()) }
}
pub(crate) fn overflow() -> bool {
    // SAFETY: the store.
    unsafe { crate::descriptor_store::cvr_ds_query(store(), 0, 0) != 0 }
}
/// SelectStorage: the dense representation and the live metadata as the exports ask (each at most 1).
pub fn select_storage() {
    let s = store();
    let dense = a!(dense_slots).load(Relaxed);
    if dense > 1 {
        fail(7);
        return;
    }
    // SAFETY: the store, under the lock.
    unsafe {
        if (crate::descriptor_store::cvr_ds_query(s, 1, 0) != 0) != (dense != 0) {
            crate::descriptor_store::cvr_ds_change(s, 1, (dense != 0).into(), 0, 0, 0, 0);
            a!(dense_active).store(dense, Release);
        }
    }
    let live = a!(live_metadata).load(Relaxed);
    if live > 1 {
        fail(8);
        return;
    }
    let live_metadata = live != 0 && BINDING_ONLY.load(Relaxed) && !RECORDING.load(Relaxed) && pending_lists() == 0;
    LIVE_METADATA.store(live_metadata, Relaxed);
    let versions = tracking_call(7, live_metadata.into());
    // SAFETY: the store, under the lock.
    unsafe { crate::descriptor_store::cvr_ds_change(s, 0, versions.into(), 0, 0, 0, 0) };
    a!(live_metadata_active).store(live_metadata.into(), Release);
}
fn publish() {
    let Some(r) = rt() else { return };
    // SAFETY: the exports and the store.
    unsafe {
        if !r.root_count.is_null() {
            *r.root_count = crate::audit_roots::cvr_sda_root_rows(r.roots, r.roots_capacity) as u32;
        }
        if !r.store_stats.is_null() {
            crate::descriptor_store::cvr_ds_stats(store(), r.store_stats);
        }
        if !r.heap_slots.is_null() {
            *r.heap_slots = crate::descriptor_store::cvr_ds_query(store(), 4, 0);
        }
        let mut view: View = core::mem::zeroed();
        crate::audit_capture::cvr_sda_capture_state(2, &mut view);
        let set = |p: *mut u64, v: u64| {
            if !p.is_null() {
                *p = v;
            }
        };
        set(r.draws, view.draws as u64);
        a!(draw_count).store(view.draw_count as u32, SeqCst);
        set(r.bindings, view.bindings as u64);
        a!(binding_count).store(view.binding_count as u32, SeqCst);
        set(r.ids, view.ids as u64);
        a!(id_count).store(view.id_count as u32, SeqCst);
        set(r.values, view.values as u64);
        a!(value_count).store(view.value_count as u32, SeqCst);
    }
}
unsafe extern "C-unwind" fn heap_removed(cookie: u64) {
    // SAFETY: the store, under the lock (the drain runs there).
    unsafe { crate::descriptor_store::cvr_ds_change(store(), 2, cookie, 0, 0, 0, 0) };
}
/// DrainRetired: the retired heaps' and resources' lifetimes, their slots removed.
pub fn drain_retired() {
    // SAFETY: the callback removes from our store.
    unsafe { crate::audit_lifetimes::cvr_sda_drain(heap_removed) };
}
fn track_resource(resource: *mut c_void, cookie: u64) -> bool {
    // SAFETY: a resource the caller holds.
    unsafe { crate::audit_lifetimes::cvr_sda_track_resource(resource, cookie, heap_removed) }
}
fn cookie(object: *mut c_void) -> u64 {
    // SAFETY: an object the caller holds (or null).
    unsafe { crate::audit_values::cvr_sda_cookie(object) }
}
unsafe extern "C-unwind" fn retire(value: *const Value) {
    // SAFETY: the store's retiring value.
    let v = unsafe { &*value };
    if v[0] == 1 && v[3] != 0 {
        crate::audit_origins::cvr_sda_origin_erase(v[3]);
    }
}
fn put(cpu: u64, value: &Value, stride: u32) {
    if stride == 0 {
        fail(1);
        return;
    }
    drain_retired();
    select_storage();
    // SAFETY: the store, under the lock.
    unsafe { crate::audit_writes::cvr_sda_put(store(), cpu, stride, value, LIVE_METADATA.load(Relaxed)) };
}
fn invalidate_internal(destination: u64, count: u32, stride: u32) {
    let _g = guard();
    drain_retired();
    select_storage();
    // SAFETY: our one destination and size.
    if !unsafe { crate::audit_writes::cvr_sda_invalidate(store(), &destination, &count, 1, stride) } {
        fail(11);
    }
}
fn register_heap(device: *mut c_void, heap: *mut c_void, fresh: bool) {
    if heap.is_null() || device.is_null() {
        return;
    }
    drain_retired();
    // SAFETY: the caller's live heap and device.
    let (Some(h), Some(d)) = (unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&heap) }, unsafe { ID3D12Device::from_raw_borrowed(&device) }) else { return };
    let desc = unsafe { h.GetDesc() };
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER {
        return;
    }
    let cookie = cookie(heap);
    if cookie == 0 {
        return;
    }
    // SAFETY: a lifetime query.
    if !fresh && unsafe { crate::audit_lifetimes::cvr_sda_heap(0, core::ptr::null_mut(), cookie) } {
        return;
    }
    let stride = unsafe { d.GetDescriptorHandleIncrementSize(desc.Type) };
    if desc.Type == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER {
        SAMPLER_STRIDE.store(stride, Relaxed);
    } else {
        RESOURCE_STRIDE.store(stride, Relaxed);
    }
    let gpu = if desc.Flags.0 & D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE.0 != 0 { unsafe { h.GetGPUDescriptorHandleForHeapStart() }.ptr } else { 0 };
    // SAFETY: the heap's lifetime registered.
    if !unsafe { crate::audit_lifetimes::cvr_sda_heap(1, heap, cookie) } {
        fail(6);
        return;
    }
    let cpu = unsafe { h.GetCPUDescriptorHandleForHeapStart() }.ptr as u64;
    // SAFETY: the store, under the lock.
    if unsafe { crate::descriptor_store::cvr_ds_change(store(), 6, cpu, gpu, cookie, desc.NumDescriptors, stride) } != 0 {
        unsafe { crate::audit_lifetimes::cvr_sda_heap(2, core::ptr::null_mut(), cookie) };
    }
}
fn value_of(kind: u32, desc: *const c_void, resource: *mut c_void, resource_cookie: u64, counter: *mut c_void, counter_cookie: u64) -> Value {
    let mut v: Value = [0; 20];
    // SAFETY: the caller's description and resources.
    unsafe { crate::audit_values::cvr_sda_value(kind, desc, resource, resource_cookie, counter, counter_cookie, &mut v) };
    v
}
fn stored_at(cpu: u64) -> Option<Value> {
    // SAFETY: the store, under the lock.
    unsafe {
        let id = crate::descriptor_store::cvr_ds_query(store(), 5, cpu) as u32;
        crate::descriptor_store::cvr_ds_get(store(), id).as_ref().copied()
    }
}

// ---- the descriptor writes
pub fn initialize(device: *mut c_void) {
    if !enabled() || device.is_null() {
        return;
    }
    let _g = guard();
    // SAFETY: the caller's device.
    let Some(d) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else { return };
    RESOURCE_STRIDE.store(unsafe { d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) }, Relaxed);
    SAMPLER_STRIDE.store(unsafe { d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER) }, Relaxed);
}
pub fn heap_created(device: *mut c_void, heap: *mut c_void) {
    if mutation_mode() != mode::CANONICAL {
        return;
    }
    let _g = guard();
    register_heap(device, heap, true);
}
pub fn heaps_bound(device: *mut c_void, heaps: &[*mut c_void]) {
    if !enabled() || heaps.len() > 2 {
        return;
    }
    let _profile = Profile::new(phase::BIND_HEAPS);
    let _g = guard();
    for &heap in heaps {
        register_heap(device, heap, false);
    }
}
pub fn root_created(root: *mut c_void, bytes: *const u8, size: usize) {
    if !enabled() || root.is_null() {
        return;
    }
    let _g = guard();
    // SAFETY: the creation's blob.
    unsafe { crate::audit_roots::cvr_sda_root_created(root, bytes, size) };
}
pub fn source_resource_cookie_at_creation(resource: *mut c_void) -> u64 {
    if !enabled() || resource.is_null() {
        return 0;
    }
    let _g = guard();
    let c = cookie(resource);
    if c != 0 && track_resource(resource, c) {
        c
    } else {
        0
    }
}
pub fn held_cookie(resource: *mut c_void) -> Option<u64> {
    // SAFETY: a resource the caller holds.
    let r = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }?;
    let (mut current, mut bytes) = (0u64, 8u32);
    unsafe { r.GetPrivateData(&COOKIE_GUID, &mut bytes, Some((&mut current as *mut u64).cast())) }.ok()?;
    (bytes == 8).then_some(current)
}
pub fn cbv(desc: *const D3D12_CONSTANT_BUFFER_VIEW_DESC, cpu: u64, origin_cookie: u64, origin_id: u64, owner: *mut c_void) {
    let m = mutation_mode();
    if m == mode::IGNORE {
        return;
    }
    if m == mode::INVALIDATE_TRACKED {
        invalidate_internal(cpu, 1, RESOURCE_STRIDE.load(Relaxed));
        return;
    }
    let _profile = Profile::new(phase::CBV);
    let _g = guard();
    // ObserveOrigins: a CBV's origin leaves with its value.
    // SAFETY: the store, under the lock.
    unsafe { crate::descriptor_store::cvr_ds_set_retirement(store(), Some(retire)) };
    let mut v = value_of(1, desc.cast(), core::ptr::null_mut(), 0, core::ptr::null_mut(), 0);
    if !desc.is_null() && origin_cookie != 0 && origin_id != 0 && !owner.is_null() {
        (v[2], v[3]) = (origin_cookie, origin_id);
        // SAFETY: the origin's owner, held by the caller.
        if !unsafe { crate::audit_origins::cvr_sda_origin_record(origin_id, &v, owner) } {
            fail(10);
            return;
        }
    }
    put(cpu, &v, RESOURCE_STRIDE.load(Relaxed));
    if v[3] != 0 && stored_at(cpu) != Some(v) {
        crate::audit_origins::cvr_sda_origin_erase(v[3]);
    }
}
pub fn srv(resource: *mut c_void, desc: *const D3D12_SHADER_RESOURCE_VIEW_DESC, cpu: u64) {
    let m = mutation_mode();
    if m == mode::IGNORE {
        return;
    }
    if m == mode::INVALIDATE_TRACKED {
        invalidate_internal(cpu, 1, RESOURCE_STRIDE.load(Relaxed));
        return;
    }
    let _profile = Profile::new(phase::OTHER_DESCRIPTOR);
    let _g = guard();
    let mut v = value_of(2, desc.cast(), resource, cookie(resource), core::ptr::null_mut(), 0);
    if v[0] != 0 && !track_resource(resource, v[2]) {
        v = [0; 20];
        fail(9);
    }
    put(cpu, &v, RESOURCE_STRIDE.load(Relaxed));
}
pub fn uav(resource: *mut c_void, counter: *mut c_void, desc: *const D3D12_UNORDERED_ACCESS_VIEW_DESC, cpu: u64) {
    let m = mutation_mode();
    if m == mode::IGNORE {
        return;
    }
    if m == mode::INVALIDATE_TRACKED {
        invalidate_internal(cpu, 1, RESOURCE_STRIDE.load(Relaxed));
        return;
    }
    let _profile = Profile::new(phase::OTHER_DESCRIPTOR);
    let _g = guard();
    let (resource_cookie, counter_cookie) = (cookie(resource), cookie(counter));
    let mut v = value_of(3, desc.cast(), resource, resource_cookie, counter, counter_cookie);
    if v[0] != 0 && (!track_resource(resource, v[2]) || !track_resource(counter, v[3])) {
        v = [0; 20];
        fail(9);
    }
    put(cpu, &v, RESOURCE_STRIDE.load(Relaxed));
}
pub fn sampler(desc: *const D3D12_SAMPLER_DESC, cpu: u64) {
    let m = mutation_mode();
    if m == mode::IGNORE {
        return;
    }
    if m == mode::INVALIDATE_TRACKED {
        invalidate_internal(cpu, 1, SAMPLER_STRIDE.load(Relaxed));
        return;
    }
    let _profile = Profile::new(phase::OTHER_DESCRIPTOR);
    let _g = guard();
    let v = value_of(4, desc.cast(), core::ptr::null_mut(), 0, core::ptr::null_mut(), 0);
    put(cpu, &v, SAMPLER_STRIDE.load(Relaxed));
}
/// Copy (CopyDescriptors/CopyDescriptorsSimple): copied values, or only the tracked destinations invalidated.
///
/// # Safety
/// The device call's arrays.
#[allow(clippy::too_many_arguments)]
pub unsafe fn copy(device: *mut c_void, n: u32, dest: *const u64, sizes: *const u32, m: u32, source: *const u64, counts: *const u32, kind: i32) {
    let mutation = mutation_mode();
    if mutation == mode::IGNORE || (kind != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 && kind != D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER.0) {
        return;
    }
    // SAFETY: the caller's device.
    let d = unsafe { ID3D12Device::from_raw_borrowed(&device) };
    if mutation == mode::INVALIDATE_TRACKED {
        let Some(d) = d.filter(|_| !(n != 0 && dest.is_null()) && n <= 65536) else {
            fail(11);
            return;
        };
        let _g = guard();
        drain_retired();
        select_storage();
        let stride = unsafe { d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE(kind)) };
        // SAFETY: as documented.
        if !unsafe { crate::audit_writes::cvr_sda_invalidate(store(), dest, sizes, n as usize, stride) } {
            fail(11);
        }
        return;
    }
    let Some(d) = d.filter(|_| !dest.is_null() && !source.is_null() && n <= 65536 && m <= 65536) else {
        if tracking().is_some_and(|t| t.pins.load(Acquire) != 0) {
            fail(3);
        }
        return;
    };
    let _profile = Profile::new(phase::COPY_DESCRIPTORS);
    let _g = guard();
    drain_retired();
    select_storage();
    let stride = unsafe { d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE(kind)) };
    // SAFETY: as documented.
    if !unsafe { crate::audit_writes::cvr_sda_copy(store(), n as usize, dest, sizes, m as usize, source, counts, stride) } {
        fail(3);
    }
}

// ---- the capture and the bindings window
pub fn begin_capture() {
    if !enabled() {
        return;
    }
    let _g = guard();
    drain_retired();
    a!(capture_seq).fetch_add(1, SeqCst);
    tracking_call(8, 0);
    // SAFETY: the capture's clear.
    unsafe { crate::audit_capture::cvr_sda_capture_state(0, core::ptr::null_mut()) };
    crate::audit_tables::cvr_sda_tables_update(4, 0, 0, 0);
    a!(pending_lists).store(0, SeqCst);
    BINDING_ONLY.store(false, Relaxed);
    RECORDING.store(true, Relaxed);
    select_storage();
    a!(capture_failure).store(if overflow() { 4 } else { TRACKING_FAILURE.load(SeqCst) }, SeqCst);
    publish();
    a!(capture_seq).fetch_add(1, SeqCst);
}
pub(crate) fn root_view(root: u64) -> Option<RootView> {
    // SAFETY: our output.
    let mut view: RootView = unsafe { core::mem::zeroed() };
    unsafe { crate::audit_roots::cvr_sda_root_find(root as *const c_void, &mut view) }.then_some(view)
}
/// Snapshot: a native draw's descriptor bindings captured while recording.
pub fn snapshot(draw: &crate::geometry_commands::DrawRecord) {
    if !enabled() {
        return;
    }
    let _g = guard();
    if !RECORDING.load(Relaxed) {
        return;
    }
    drain_retired();
    a!(capture_seq).fetch_add(1, SeqCst);
    let root = root_view(draw.root);
    let table = crate::audit_tables::cvr_sda_tables_find(draw.list);
    let lent = Draw { sequence: draw.sequence, list: draw.list, root: draw.root, table_mask: draw.table_mask, tables: draw.tables.as_ptr() };
    let layout = root.as_ref().map(|r| Layout { ranges: r.ranges, count: r.count, supported: r.supported, parameters: r.parameters });
    let capacity = rt().map_or(0, |r| r.draw_capacity);
    // SAFETY: our draw and layout, the store and the list's tables (the audit_tables and audit_capture mirrors agree).
    let outcome = unsafe {
        crate::audit_capture::cvr_sda_capture_snapshot(
            store(),
            &lent,
            layout.as_ref().map_or(core::ptr::null(), |l| l as *const Layout),
            table.cast(),
            RESOURCE_STRIDE.load(Relaxed),
            SAMPLER_STRIDE.load(Relaxed),
            capacity,
        )
    };
    if outcome & 255 != 0 {
        a!(capture_failure).store(outcome & 255, SeqCst);
    }
    if outcome & 256 != 0 {
        RECORDING.store(false, Relaxed);
    }
    a!(pending_lists).store(pending_lists() as u32, SeqCst);
    publish();
    a!(capture_seq).fetch_add(1, SeqCst);
}
pub fn end_capture() {
    if !enabled() {
        return;
    }
    let _g = guard();
    RECORDING.store(false, Relaxed);
}
pub fn begin_bindings() -> bool {
    if !enabled() {
        return false;
    }
    let _g = guard();
    drain_retired();
    if RECORDING.load(Relaxed) || pending_lists() != 0 || overflow() || TRACKING_FAILURE.load(SeqCst) != 0 {
        return false;
    }
    tracking_call(8, 0);
    crate::audit_tables::cvr_sda_tables_update(4, 0, 0, 0);
    BINDING_ONLY.store(true, Relaxed);
    select_storage();
    TRACKING_FAILURE.load(SeqCst) == 0
}
pub fn end_bindings() {
    let _g = guard();
    tracking_call(8, 0);
    BINDING_ONLY.store(false, Relaxed);
    crate::audit_tables::cvr_sda_tables_update(4, 0, 0, 0);
    select_storage();
}
pub fn healthy() -> bool {
    if !enabled() {
        return false;
    }
    let _g = guard();
    drain_retired();
    !overflow() && TRACKING_FAILURE.load(SeqCst) == 0
}
pub fn before_submit(lists: &[u64]) {
    if !enabled() || a!(pending_lists).load(SeqCst) == 0 {
        return;
    }
    let _g = guard();
    drain_retired();
    a!(capture_seq).fetch_add(1, SeqCst);
    // SAFETY: our list addresses.
    let left = unsafe { crate::audit_capture::cvr_sda_capture_submit(store(), lists.as_ptr(), lists.len()) };
    a!(pending_lists).store(left as u32, SeqCst);
    publish();
    a!(capture_seq).fetch_add(1, SeqCst);
}
pub fn reset(list: u64) {
    if !enabled() {
        return;
    }
    let _g = guard();
    crate::audit_tables::cvr_sda_tables_update(3, list, 0, 0);
    let left = unsafe { crate::audit_capture::cvr_sda_capture_reset(list) };
    if left == usize::MAX {
        return;
    }
    a!(capture_seq).fetch_add(1, SeqCst);
    a!(pending_lists).store(left as u32, SeqCst);
    publish();
    a!(capture_seq).fetch_add(1, SeqCst);
}
fn probe_on() -> bool {
    a!(native_probe_state).load(Relaxed) == 1
}
pub fn root(list: u64, root: u64) {
    if !enabled() || !probe_on() {
        return;
    }
    let _g = guard();
    if !RECORDING.load(Relaxed) && !BINDING_ONLY.load(Relaxed) {
        return;
    }
    crate::audit_tables::cvr_sda_tables_update(0, list, 0, root);
}
pub fn table(list: u64, index: u32, address: u64) {
    if index >= 64 || !enabled() || !probe_on() {
        return;
    }
    let _g = guard();
    if !RECORDING.load(Relaxed) && !BINDING_ONLY.load(Relaxed) {
        return;
    }
    crate::audit_tables::cvr_sda_tables_update(1, list, index, address);
}
pub fn clear_bindings(list: u64) {
    if !enabled() {
        return;
    }
    let _g = guard();
    crate::audit_tables::cvr_sda_tables_update(2, list, 0, 0);
}

// ---- the lookups and the resolutions at a draw
pub fn find_binding(root: u64, kind: u32, space: u32, register: u32, visibility: u32) -> Option<(u32, u32)> {
    if !enabled() || root == 0 || kind > D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 || visibility > D3D12_SHADER_VISIBILITY_MESH.0 as u32 {
        return None;
    }
    let _g = guard();
    if TRACKING_FAILURE.load(SeqCst) != 0 {
        return None;
    }
    let found = root_view(root).filter(|r| r.reasons & 5 == 0)?;
    let (mut parameter, mut offset) = (u32::MAX, u32::MAX);
    // SAFETY: the layout's ranges, under the lock.
    unsafe { crate::audit_lookup::cvr_sda_find_binding(found.ranges, found.count, kind, space, register, visibility, &mut parameter, &mut offset) }.then_some((parameter, offset))
}
/// descriptors::CbvBinding.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CbvBinding {
    pub descriptor: u64,
    pub heap: u64,
    pub address: u64,
    pub bytes: u32,
}
/// descriptors::DescriptorStamp.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorStamp {
    pub gpu: u64,
    pub heap: u64,
    pub value: Value,
}
const _: () = assert!(core::mem::size_of::<CbvBinding>() == 32 && core::mem::size_of::<DescriptorStamp>() == 176);
fn range_at(address: u64) -> RangeIdentity {
    // SAFETY: our output; the store, under the lock.
    let mut range: RangeIdentity = unsafe { core::mem::zeroed() };
    unsafe { crate::descriptor_store::cvr_ds_range(store(), address, 1, &mut range) };
    range
}
fn cbv_address(address: u64) -> Option<CbvBinding> {
    if RESOURCE_STRIDE.load(Relaxed) == 0 || overflow() {
        return None;
    }
    let range = range_at(address);
    if !range.valid {
        return None;
    }
    let value = stored_at(range.cpu)?;
    // SAFETY: our value.
    if !unsafe { crate::audit_lookup::cvr_sda_admitted(0, &value, false) } {
        return None;
    }
    Some(CbvBinding { descriptor: address, heap: range.heap, address: value[4], bytes: value[5] as u32 })
}
fn descriptor_address(address: u64) -> Option<DescriptorStamp> {
    if address == 0 || overflow() || TRACKING_FAILURE.load(SeqCst) != 0 {
        return None;
    }
    let range = range_at(address);
    // SAFETY: lifetime queries.
    if !range.valid || range.heap == 0 || !unsafe { crate::audit_lifetimes::cvr_sda_heap(0, core::ptr::null_mut(), range.heap) } {
        return None;
    }
    let value = stored_at(range.cpu)?;
    let tracked = !unsafe { crate::audit_lifetimes::cvr_sda_resource_find(value[2]) }.is_null();
    // SAFETY: our value.
    if !unsafe { crate::audit_lookup::cvr_sda_admitted(1, &value, tracked) } {
        return None;
    }
    Some(DescriptorStamp { gpu: address, heap: range.heap, value })
}
fn table_address(list: u64, root: u64, parameter: u32, offset: u32, stride: u32) -> Option<u64> {
    let mut address = 0u64;
    // SAFETY: our output.
    unsafe { crate::audit_tables::cvr_sda_tables_address(list, root, parameter, offset, stride, &mut address) }.then_some(address)
}
pub fn resolve_cbv(list: u64, root: u64, parameter: u32, offset: u32) -> Option<CbvBinding> {
    if !enabled() || parameter >= 64 {
        return None;
    }
    let _g = guard();
    drain_retired();
    cbv_address(table_address(list, root, parameter, offset, RESOURCE_STRIDE.load(Relaxed))?)
}
pub fn resolve_cbv_address(descriptor: u64) -> Option<CbvBinding> {
    if !enabled() {
        return None;
    }
    let _g = guard();
    drain_retired();
    cbv_address(descriptor)
}
pub fn capture_descriptor(list: u64, root: u64, parameter: u32, offset: u32) -> Option<DescriptorStamp> {
    let stride = RESOURCE_STRIDE.load(Relaxed);
    if !enabled() || list == 0 || parameter >= 64 || stride == 0 {
        return None;
    }
    let _g = guard();
    drain_retired();
    let stamp = descriptor_address(table_address(list, root, parameter, offset, stride)?)?;
    (stamp.value[0] == 1 || stamp.value[0] == 2).then_some(stamp)
}
pub fn matches_descriptor(expected: &DescriptorStamp) -> bool {
    if !rt().is_some_and(|r| (r.observe_bindings)()) || expected.gpu == 0 || expected.heap == 0 {
        return false;
    }
    let _g = guard();
    drain_retired();
    descriptor_address(expected.gpu).is_some_and(|current| current.heap == expected.heap && current.value == expected.value)
}
fn add_ref(object: *mut c_void) {
    // SAFETY: a live COM object the lock keeps alive.
    if let Some(unknown) = unsafe { windows::core::IUnknown::from_raw_borrowed(&object) } {
        core::mem::forget(unknown.clone());
    }
}
/// CaptureCbvSource: the CBV stamp and its creation origin's owner (a reference handed over).
pub fn capture_cbv_source(list: u64, root: u64, parameter: u32, offset: u32) -> Option<(DescriptorStamp, *mut c_void)> {
    if !enabled() || list == 0 || parameter >= 64 {
        return None;
    }
    let _g = guard();
    drain_retired();
    let stride = RESOURCE_STRIDE.load(Relaxed);
    if stride == 0 {
        return None;
    }
    let stamp = descriptor_address(table_address(list, root, parameter, offset, stride)?).filter(|s| s.value[0] == 1)?;
    let mut origin = core::ptr::null_mut();
    if stamp.value[3] != 0 {
        let (mut value, mut owner): (Value, *mut c_void) = ([0; 20], core::ptr::null_mut());
        // SAFETY: our outputs.
        if !unsafe { crate::audit_origins::cvr_sda_origin_find(stamp.value[3], &mut value, &mut owner) } || value != stamp.value {
            return None;
        }
        if !owner.is_null() {
            add_ref(owner);
        }
        origin = owner;
    }
    Some((stamp, origin))
}
/// ResolveSrvAtDraw: the stamp, the resource (a reference handed over) and its view, when the held resource's cookie
/// is the descriptor's.
pub fn resolve_srv_at_draw(list: u64, root: u64, parameter: u32, offset: u32) -> Option<(DescriptorStamp, *mut c_void, D3D12_SHADER_RESOURCE_VIEW_DESC)> {
    if !enabled() || list == 0 || parameter >= 64 {
        return None;
    }
    let _g = guard();
    drain_retired();
    let stride = RESOURCE_STRIDE.load(Relaxed);
    if stride == 0 || overflow() || TRACKING_FAILURE.load(SeqCst) != 0 {
        return None;
    }
    let stamp = descriptor_address(table_address(list, root, parameter, offset, stride)?).filter(|s| s.value[0] == 2)?;
    // SAFETY: the lifetime record (vtable, refs, next, cookie, resource), alive under the lock.
    let lifetime = unsafe { crate::audit_lifetimes::cvr_sda_resource_find(stamp.value[2]) };
    if lifetime.is_null() {
        return None;
    }
    let resource = unsafe { *lifetime.cast::<u8>().add(32).cast::<*mut c_void>() };
    if resource.is_null() || held_cookie(resource) != Some(stamp.value[2]) {
        return None;
    }
    let v = &stamp.value;
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC { Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT(v[4] as i32), ViewDimension: D3D12_SRV_DIMENSION(v[5] as i32), Shader4ComponentMapping: v[6] as u32, ..Default::default() };
    match view.ViewDimension {
        D3D12_SRV_DIMENSION_BUFFER => view.Anonymous.Buffer = D3D12_BUFFER_SRV { FirstElement: v[7], NumElements: v[8] as u32, StructureByteStride: v[9] as u32, Flags: D3D12_BUFFER_SRV_FLAGS(v[10] as i32) },
        D3D12_SRV_DIMENSION_TEXTURE2D => view.Anonymous.Texture2D = D3D12_TEX2D_SRV { MostDetailedMip: v[7] as u32, MipLevels: v[8] as u32, PlaneSlice: v[9] as u32, ResourceMinLODClamp: f32::from_bits(v[10] as u32) },
        D3D12_SRV_DIMENSION_TEXTURE2DARRAY => {
            view.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_SRV { MostDetailedMip: v[7] as u32, MipLevels: v[8] as u32, FirstArraySlice: v[9] as u32, ArraySize: v[10] as u32, PlaneSlice: v[11] as u32, ResourceMinLODClamp: f32::from_bits(v[12] as u32) }
        }
        _ => return None,
    }
    add_ref(resource);
    Some((stamp, resource, view))
}
pub fn resolve_sampler_at_draw(list: u64, root: u64, parameter: u32, offset: u32) -> Option<(DescriptorStamp, D3D12_SAMPLER_DESC)> {
    if !enabled() || list == 0 || parameter >= 64 {
        return None;
    }
    let _g = guard();
    drain_retired();
    let stride = SAMPLER_STRIDE.load(Relaxed);
    if stride == 0 {
        return None;
    }
    let stamp = descriptor_address(table_address(list, root, parameter, offset, stride)?).filter(|s| s.value[0] == 4)?;
    let v = &stamp.value;
    let f = |i: usize| f32::from_bits(v[i] as u32);
    let view = D3D12_SAMPLER_DESC {
        Filter: D3D12_FILTER(v[4] as i32),
        AddressU: D3D12_TEXTURE_ADDRESS_MODE(v[5] as i32),
        AddressV: D3D12_TEXTURE_ADDRESS_MODE(v[6] as i32),
        AddressW: D3D12_TEXTURE_ADDRESS_MODE(v[7] as i32),
        MipLODBias: f(8),
        MaxAnisotropy: v[9] as u32,
        ComparisonFunc: D3D12_COMPARISON_FUNC(v[10] as i32),
        BorderColor: [f(11), f(12), f(13), f(14)],
        MinLOD: f(15),
        MaxLOD: f(16),
    };
    Some((stamp, view))
}
pub fn resolve_cpu_cbv_address(descriptor: u64) -> Option<CbvBinding> {
    if !rt().is_some_and(|r| (r.observe_bindings)()) || descriptor == 0 {
        return None;
    }
    let _g = guard();
    drain_retired();
    if overflow() || TRACKING_FAILURE.load(SeqCst) != 0 {
        return None;
    }
    let v = stored_at(descriptor)?;
    if v[0] != 1 || v[4] == 0 || v[5] == 0 || v[5] > 65536 || v[4] & 255 != 0 || v[5] & 255 != 0 {
        return None;
    }
    Some(CbvBinding { descriptor, heap: 0, address: v[4], bytes: v[5] as u32 })
}

// ---- the C ABI
macro_rules! out {
    ($ptr:ident, $value:expr) => {{
        let value = $value;
        // SAFETY: the caller's output.
        if let (Some(v), Some(o)) = (value, unsafe { $ptr.as_mut() }) {
            *o = v;
            true
        } else {
            false
        }
    }};
}
/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_audit_lock(enter: bool) {
    if enter {
        core::mem::forget(guard());
    } else {
        drop(Guard);
    }
}
/// The state the .cpp's remaining captures read: 0 drain retired, 1 select storage, 2 the tracking failure (its
/// address), 3 the resource stride, 4 the sampler stride, 5 fail with `value`.
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_audit_state(which: u32, value: u32) -> u64 {
    match which {
        0 => drain_retired(),
        1 => select_storage(),
        2 => return &TRACKING_FAILURE as *const AtomicU32 as u64,
        3 => return RESOURCE_STRIDE.load(Relaxed).into(),
        4 => return SAMPLER_STRIDE.load(Relaxed).into(),
        5 => fail(value),
        _ => {}
    }
    0
}
/// The writes: 0 initialize, 1 heap created, 2 root created (`a` the blob, `b` its size), 3 CBV (`a` the description,
/// `b` the CPU handle, `c` and `d` the origin's cookie and id, `e` its owner), 4 SRV (`e` the resource), 5 UAV (`e` the
/// resource, `f` the counter), 6 sampler, 7 heaps bound (`a` the heaps, `b` their count).
///
/// # Safety
/// The device call's arguments.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_audit_write(which: u32, device: *mut c_void, a: usize, b: u64, c: u64, d: u64, e: *mut c_void, f: *mut c_void) -> u64 {
    match which {
        0 => initialize(device),
        1 => heap_created(device, e),
        2 => root_created(e, a as *const u8, b as usize),
        3 => cbv(a as *const _, b, c, d, e),
        4 => srv(e, a as *const _, b),
        5 => uav(e, f, a as *const _, b),
        6 => sampler(a as *const _, b),
        7 if a != 0 => heaps_bound(device, unsafe { core::slice::from_raw_parts(a as *const *mut c_void, b as usize) }),
        8 => return source_resource_cookie_at_creation(e),
        _ => {}
    }
    0
}
/// The held resource's cookie (MatchesHeldResourceCookie, GetHeldResourceCookie), 0 without one.
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_audit_held_cookie(resource: *mut c_void) -> u64 {
    if resource.is_null() {
        return 0;
    }
    held_cookie(resource).unwrap_or(0)
}
/// # Safety
/// As copy.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_audit_copy(device: *mut c_void, n: u32, dest: *const u64, sizes: *const u32, m: u32, source: *const u64, counts: *const u32, kind: i32) {
    unsafe { copy(device, n, dest, sizes, m, source, counts, kind) };
}
/// The capture: 0 begin, 1 end, 2 begin bindings, 3 end bindings, 4 healthy, 5 reset `a`, 6 root (`a` list, `b` root),
/// 7 table (`a` list, `c` index, `b` address), 8 clear bindings `a`, 9 enabled.
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_audit_capture(which: u32, a: u64, b: u64, c: u32) -> bool {
    match which {
        0 => begin_capture(),
        1 => end_capture(),
        2 => return begin_bindings(),
        3 => end_bindings(),
        4 => return healthy(),
        5 => reset(a),
        6 => root(a, b),
        7 => table(a, c, b),
        8 => clear_bindings(a),
        9 => return enabled(),
        _ => {}
    }
    true
}
/// # Safety
/// `draw` is the native probe's record.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_snapshot(draw: *const crate::geometry_commands::DrawRecord) {
    if let Some(d) = unsafe { draw.as_ref() } {
        snapshot(d);
    }
}
/// # Safety
/// `lists` holds `count` lists.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_submit(count: u32, lists: *const u64) {
    if lists.is_null() {
        return;
    }
    before_submit(unsafe { core::slice::from_raw_parts(lists, count as usize) });
}
/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_find_binding(root: u64, kind: u32, space: u32, register: u32, visibility: u32, parameter: *mut u32, offset: *mut u32) -> bool {
    let found = find_binding(root, kind, space, register, visibility);
    unsafe {
        let (p, o) = found.unwrap_or((u32::MAX, u32::MAX));
        (*parameter, *offset) = (p, o);
    }
    found.is_some()
}
/// The CBVs: 0 at a draw's table slot, 1 at a descriptor, 2 at a CPU descriptor.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_cbv(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut CbvBinding) -> bool {
    out!(
        out,
        match which {
            0 => resolve_cbv(list, root, parameter, offset),
            1 => resolve_cbv_address(list),
            _ => resolve_cpu_cbv_address(list),
        }
    )
}
/// The stamps: 0 capture at a draw's slot, 1 matches `out`, 2 the CBV source with its origin (`object`).
///
/// # Safety
/// `out` and `object` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_stamp(which: u32, list: u64, root: u64, parameter: u32, offset: u32, out: *mut DescriptorStamp, object: *mut *mut c_void) -> bool {
    match which {
        0 => out!(out, capture_descriptor(list, root, parameter, offset)),
        1 => unsafe { out.as_ref() }.is_some_and(matches_descriptor),
        _ => match capture_cbv_source(list, root, parameter, offset) {
            Some((stamp, origin)) => unsafe {
                *out = stamp;
                *object = origin;
                true
            },
            None => false,
        },
    }
}
/// # Safety
/// The outputs are writable; `resource` receives a reference.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_srv(list: u64, root: u64, parameter: u32, offset: u32, stamp: *mut DescriptorStamp, resource: *mut *mut c_void, view: *mut D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    match resolve_srv_at_draw(list, root, parameter, offset) {
        Some((s, r, v)) => unsafe {
            (*stamp, *resource, *view) = (s, r, v);
            true
        },
        None => false,
    }
}
/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_audit_sampler(list: u64, root: u64, parameter: u32, offset: u32, stamp: *mut DescriptorStamp, view: *mut D3D12_SAMPLER_DESC) -> bool {
    match resolve_sampler_at_draw(list, root, parameter, offset) {
        Some((s, v)) => unsafe {
            (*stamp, *view) = (s, v);
            true
        },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! The lock's recursion, the state's failure and strides, and the paths gated off without the probe's bindings.
    use super::*;

    #[test]
    fn lock_state_and_gates() {
        {
            let _a = guard();
            let _b = guard(); // recursive, as the .cpp's critical section
        }
        cvr_sda_audit_lock(true);
        cvr_sda_audit_lock(false);
        fail(5);
        assert_eq!(cvr_sda_audit_state(2, 0) as *const AtomicU32, &TRACKING_FAILURE as *const AtomicU32);
        assert_eq!(TRACKING_FAILURE.load(SeqCst), 5);
        TRACKING_FAILURE.store(0, SeqCst);
        // Unattached: the probe's bindings are not observed, so nothing writes, captures or resolves.
        assert!(!enabled() && !cvr_sda_audit_capture(9, 0, 0, 0));
        assert!(!begin_bindings() && !healthy());
        assert!(find_binding(0x1000, 2, 0, 1, 0).is_none() && resolve_cbv(1, 2, 3, 4).is_none() && resolve_cpu_cbv_address(0x40).is_none());
        let mut p = (0u32, 0u32);
        assert!(!unsafe { cvr_sda_audit_find_binding(0x1000, 2, 0, 1, 0, &mut p.0, &mut p.1) } && p == (u32::MAX, u32::MAX));
        assert_eq!(cvr_sda_audit_state(3, 0), 0);
    }
}
