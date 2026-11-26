//! THE NATIVE CBV UPLOADER'S OWN BUFFERS (formerly src/Render/ConstantUploadRegistry.cpp), for the explicit
//! descriptor-observation launch only: the CPU-accessible buffers the engine's CBV uploader creates, mapped once and
//! kept (512 MiB and 256 buffers at most, the least recently touched evicted first), so the constant bytes a CBV
//! names can be read back later. Generic streaming allocations are not observed: they do not resolve these CBVs and
//! would churn the bounded cache.
//!
//! A buffer is learnt three ways: created (UploadCreated), its address taken by the uploader itself (the resource's
//! GetGPUVirtualAddress, hooked per vtable, from the uploader's two call sites), or found from a CBV whose address no
//! known buffer covers by walking up to the native CreateCBV return sites (cvr_engine::stack) and checking the
//! uploader's CPU allocation against the buffer's own mapping. A thread-local hint and a one-use touch receipt keep
//! the hot path off the registry's lock; neither pins a buffer or authorises a read.
//!
//! The exports stay C++ (src/Render/ConstantUploadRegistry.cpp attaches them, with the descriptor families' switch
//! and the observation profile's counters, at its static initialisation).
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::GetCurrentProcess;

/// CvrUploadRegistryAttach: the C++ half's exports and switches.
#[repr(C)]
pub struct Attach {
    count: *const AtomicU32,
    failures: *const AtomicU32,
    bytes: *const AtomicU64,
    reads: *const AtomicU64,
    misses: *const AtomicU64,
    evictions: *const AtomicU32,
    types: *const AtomicU32,
    native_imports: *const AtomicU32,
    native_rejects: *const AtomicU32,
    api_hits: *const AtomicU32,
    fast_path: *const AtomicU32,
    fast_stats: *const AtomicU64,
    profile_enable: *const AtomicU32,
    profile_stats: *const AtomicU64,
    families_enabled: Option<extern "C" fn() -> bool>,
}
const _: () = assert!(core::mem::size_of::<Attach>() == 120);
static ATTACH: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own table (formerly lent by src/Render/ConstantUploadRegistry.cpp): constant_registry_exports.rs'
/// statics, the C++ observation profile's exports and families::Enabled. The Rust test variants still attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    #![allow(non_upper_case_globals)]
    use super::Attach;
    use crate::constant_registry_exports::*;
    use core::sync::atomic::{AtomicU32, AtomicU64};
    extern "C" {
        /// include/Render/ObservationProfile.hpp's (C++ inline variables).
        static CyberpunkVR_ObservationProfileEnable: AtomicU32;
        static CyberpunkVR_ObservationProfileStats: [AtomicU64; 16];
        #[link_name = "cvr_nsf2_enabled"]
        fn families_enabled() -> bool;
    }
    extern "C" fn enabled() -> bool {
        // SAFETY: a plain C++ query.
        unsafe { families_enabled() }
    }
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Attach {}
    pub static ATTACH: Attach = Attach {
        count: &raw const CyberpunkVR_ConstantUploadCount,
        failures: &raw const CyberpunkVR_ConstantUploadFailures,
        bytes: &raw const CyberpunkVR_ConstantUploadBytes,
        reads: &raw const CyberpunkVR_ConstantUploadReads,
        misses: &raw const CyberpunkVR_ConstantUploadMisses,
        evictions: &raw const CyberpunkVR_ConstantUploadEvictions,
        types: (&raw const CyberpunkVR_ConstantUploadTypes).cast(),
        native_imports: &raw const CyberpunkVR_ConstantUploadNativeImports,
        native_rejects: &raw const CyberpunkVR_ConstantUploadNativeRejects,
        api_hits: &raw const CyberpunkVR_ConstantUploadApiHits,
        fast_path: &raw const CyberpunkVR_ConstantUploadFastPath,
        fast_stats: (&raw const CyberpunkVR_ConstantUploadFastStats).cast(),
        profile_enable: &raw const CyberpunkVR_ObservationProfileEnable,
        profile_stats: (&raw const CyberpunkVR_ObservationProfileStats).cast(),
        families_enabled: Some(enabled),
    };
}
fn attach() -> Option<&'static Attach> {
    // SAFETY: the C++ half's table, alive for the process.
    let attached = unsafe { ATTACH.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::ATTACH));
    attached
}
/// # Safety
/// `table` and everything it points at live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_registry_attach(table: *const Attach) {
    ATTACH.store(table as *mut Attach, Release);
}
fn at32(p: *const AtomicU32, index: usize) -> &'static AtomicU32 {
    // SAFETY: an exported std::atomic<uint32_t> (array).
    unsafe { &*p.add(index) }
}
fn at64(p: *const AtomicU64, index: usize) -> &'static AtomicU64 {
    // SAFETY: an exported std::atomic<uint64_t> (array).
    unsafe { &*p.add(index) }
}
/// The observation's gate: not the port's own commands, bindings observed, the descriptor families on.
fn observing(t: &Attach) -> bool {
    !cvr_host::probe::internal()
        && crate::stereo_probe::observe_bindings()
        && t.families_enabled.is_some_and(|f| f())
}

/// observation_profile::Scope: inclusive CPU time of one phase, when the profile is on.
struct Profile {
    stats: *const AtomicU64,
    index: usize,
    begin: Option<i64>,
}
const PHASE_TOUCH_UPLOAD: usize = 4;
const PHASE_UNWIND_UPLOAD: usize = 5;
fn qpc() -> i64 {
    let mut now = 0i64;
    // SAFETY: one output value.
    let _ = unsafe { QueryPerformanceCounter(&mut now) };
    now
}
impl Profile {
    fn new(t: &Attach, phase: usize) -> Self {
        let on = at32(t.profile_enable, 0).load(Relaxed) != 0;
        Profile {
            stats: t.profile_stats,
            index: phase * 2,
            begin: on.then(qpc),
        }
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        if let Some(begin) = self.begin {
            let end = qpc();
            at64(self.stats, self.index).fetch_add(1, Relaxed);
            at64(self.stats, self.index + 1).fetch_add(end.wrapping_sub(begin) as u64, Relaxed);
        }
    }
}

struct Upload {
    resource: ID3D12Resource,
    mapped: *mut u8,
    bytes: u64,
    used: AtomicU64,
    fixed_heap: bool,
}
// SAFETY: D3D12 resources are free-threaded; the mapping is read through ReadProcessMemory only.
unsafe impl Send for Upload {}
unsafe impl Sync for Upload {}
impl Drop for Upload {
    fn drop(&mut self) {
        if !self.mapped.is_null() {
            let none = D3D12_RANGE::default();
            // SAFETY: the buffer this upload mapped.
            unsafe { self.resource.Unmap(0, Some(&none)) };
        }
    }
}
static UPLOADS: Mutex<BTreeMap<u64, Arc<Upload>>> = Mutex::new(BTreeMap::new());
static CLOCK: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(1);
const BYTE_LIMIT: u64 = 512 * 1024 * 1024;
fn uploads() -> MutexGuard<'static, BTreeMap<u64, Arc<Upload>>> {
    UPLOADS.lock().unwrap_or_else(|p| p.into_inner())
}
fn touch(upload: &Upload) {
    let now = CLOCK.fetch_add(1, Relaxed) + 1;
    let mut previous = upload.used.load(Relaxed);
    while previous < now {
        match upload
            .used
            .compare_exchange_weak(previous, now, Relaxed, Relaxed)
        {
            Ok(_) => break,
            Err(actual) => previous = actual,
        }
    }
}

/// UploadTouchReceipt: an advisory, one-use LRU touch. It owns no pointer and never authorises reading mapped bytes.
#[derive(Clone, Copy, Default)]
struct Receipt {
    base: u64,
    bytes: u64,
    generation: u64,
}
/// LookupHint: a weak hint that does not pin an evicted buffer between calls; a successful lock retains the owner
/// only while its LRU timestamp is updated.
#[derive(Default)]
struct Hint {
    base: u64,
    generation: u64,
    owner: Weak<Upload>,
}
thread_local! {
    static RECEIPT: Cell<Receipt> = const { Cell::new(Receipt { base: 0, bytes: 0, generation: 0 }) };
    static HINT: RefCell<Hint> = RefCell::new(Hint::default());
}
fn receipt_clear() {
    RECEIPT.set(Receipt::default());
}
fn receipt_set(base: u64, bytes: u64, generation: u64) -> bool {
    receipt_clear();
    if base == 0 || bytes == 0 || generation == 0 || bytes > u64::MAX - base {
        return false;
    }
    RECEIPT.set(Receipt {
        base,
        bytes,
        generation,
    });
    true
}
fn receipt_take(address: u64, bytes: u64, generation: u64) -> bool {
    let r = RECEIPT.get();
    receipt_clear();
    r.generation != 0
        && r.generation == generation
        && bytes != 0
        && bytes <= 65536
        && address >= r.base
        && address - r.base <= r.bytes
        && bytes <= r.bytes - (address - r.base)
}
fn hint_clear() {
    HINT.with_borrow_mut(|h| *h = Hint::default());
}
fn hint_remember(address: u64, value: &Arc<Upload>) {
    HINT.with_borrow_mut(|h| {
        h.base = address;
        h.generation = GENERATION.load(Relaxed);
        h.owner = Arc::downgrade(value);
    });
}
fn hint_generation() -> u64 {
    HINT.with_borrow(|h| h.generation)
}
fn hint_lock(address: u64, bytes: u64, expected: *mut c_void) -> Option<Arc<Upload>> {
    HINT.with_borrow(|h| {
        if h.generation == 0
            || h.generation != GENERATION.load(Acquire)
            || address < h.base
            || bytes == 0
        {
            return None;
        }
        let live = h.owner.upgrade()?;
        if (!expected.is_null() && (address != h.base || live.resource.as_raw() != expected))
            || address - h.base > live.bytes
            || bytes > live.bytes - (address - h.base)
            || h.generation != GENERATION.load(Acquire)
        {
            return None;
        }
        Some(live)
    })
}
fn borrowed(resource: &*mut c_void) -> Option<&ID3D12Resource> {
    // SAFETY: a live resource of the caller's, or null.
    unsafe { ID3D12Resource::from_raw_borrowed(resource) }
}

// ---- the resources' GetGPUVirtualAddress, hooked per vtable (slot 11)

type GpuAddressFn = unsafe extern "system" fn(*mut c_void) -> u64;
type CreationObserver = unsafe extern "C" fn(*mut c_void, u64, usize);
static CREATION_OBSERVER: AtomicUsize = AtomicUsize::new(0);
const HOOKS: usize = 16;
static HOOK_TABLES: [AtomicUsize; HOOKS] = [const { AtomicUsize::new(0) }; HOOKS];
static HOOK_ORIGINALS: [AtomicUsize; HOOKS] = [const { AtomicUsize::new(0) }; HOOKS];
static HOOK_COUNT: AtomicU32 = AtomicU32::new(0);
static HOOK_MUTEX: Mutex<()> = Mutex::new(());
/// The uploader's two GetGPUVirtualAddress call sites.
const UPLOADER_SITES: [usize; 2] = [0x33468F, 0x333BE9];

/// The hooked slot: the return address into the second argument, then the body.
#[unsafe(naked)]
unsafe extern "system" fn gpu_address_thunk() {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym gpu_address);
}
unsafe extern "system" fn gpu_address(resource: *mut c_void, caller: usize) -> u64 {
    // SAFETY: a live resource whose vtable this registry hooked.
    let table = unsafe { *(resource as *const usize) };
    let count = HOOK_COUNT.load(Acquire) as usize;
    let original = (0..count.min(HOOKS))
        .find(|&i| HOOK_TABLES[i].load(Relaxed) == table)
        .map(|i| HOOK_ORIGINALS[i].load(Relaxed))
        .unwrap_or(0);
    if original == 0 {
        return 0;
    }
    // SAFETY: the slot's original function.
    let value = unsafe { core::mem::transmute::<usize, GpuAddressFn>(original)(resource) };
    let game = cvr_engine::code::image_base();
    if value != 0
        && UPLOADER_SITES
            .iter()
            .any(|&site| caller == game.wrapping_add(site))
    {
        let observer = CREATION_OBSERVER.load(Acquire);
        if observer != 0 {
            // SAFETY: the C++ observer set through cvr_upload_set_creation_observer.
            unsafe {
                core::mem::transmute::<usize, CreationObserver>(observer)(resource, value, caller)
            };
        }
        // The native uploader retains this API argument while it gets the VA. Calls made by upload_created originate
        // in the plugin and cannot recur.
        upload_address_observed(resource, value);
        if let Some(t) = attach() {
            at32(t.api_hits, 0).fetch_add(1, SeqCst);
        }
    }
    value
}
fn observe_resource_api(resource: *mut c_void) {
    let Some(t) = attach() else { return };
    if !observing(t) {
        return;
    }
    let Some(r) = borrowed(&resource) else { return };
    // SAFETY: a live resource.
    if unsafe { r.GetDesc() }.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER {
        return;
    }
    let _lock = HOOK_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the resource's vtable pointer.
    let table = unsafe { *(resource as *const *mut usize) };
    let count = HOOK_COUNT.load(Relaxed) as usize;
    if (0..count).any(|i| HOOK_TABLES[i].load(Relaxed) == table as usize) {
        return;
    }
    if count >= HOOKS {
        at32(t.failures, 0).fetch_add(1, SeqCst);
        return;
    }
    // SAFETY: slot 11 of a live vtable, made writable for the swap and restored.
    unsafe {
        let slot = table.add(11);
        let mut old = PAGE_PROTECTION_FLAGS(0);
        if VirtualProtect(slot.cast(), 8, PAGE_READWRITE, &mut old).is_err() {
            at32(t.failures, 0).fetch_add(1, SeqCst);
            return;
        }
        HOOK_TABLES[count].store(table as usize, Relaxed);
        HOOK_ORIGINALS[count].store(*slot, Relaxed);
        HOOK_COUNT.store(count as u32 + 1, Release);
        AtomicUsize::from_ptr(slot).swap(gpu_address_thunk as *const () as usize, SeqCst);
        let mut ignored = PAGE_PROTECTION_FLAGS(0);
        let _ = VirtualProtect(slot.cast(), 8, old, &mut ignored);
    }
}

// ---- the registry

fn upload_created(resource: *mut c_void) {
    let Some(t) = attach() else { return };
    if !observing(t) {
        return;
    }
    let Some(r) = borrowed(&resource) else { return };
    // SAFETY (whole function): a live resource.
    let desc = unsafe { r.GetDesc() };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER {
        return;
    }
    let mut properties = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    if unsafe { r.GetHeapProperties(Some(&mut properties), Some(&mut flags)) }.is_err() {
        return;
    }
    if (properties.Type.0 as u32) < 6 {
        at32(t.types, properties.Type.0 as usize).fetch_add(1, SeqCst);
    }
    let fixed_heap =
        properties.Type == D3D12_HEAP_TYPE_UPLOAD || properties.Type == D3D12_HEAP_TYPE_GPU_UPLOAD;
    let cpu_accessible = fixed_heap
        || (properties.Type == D3D12_HEAP_TYPE_CUSTOM
            && (properties.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE
                || properties.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_WRITE_BACK));
    if !cpu_accessible || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0 {
        return;
    }
    let address = unsafe { r.GetGPUVirtualAddress() };
    if address == 0 || desc.Width == 0 || desc.Width > BYTE_LIMIT || address > u64::MAX - desc.Width
    {
        return;
    }
    // Declared before the lock: the evicted buffers are unmapped after it is released.
    let mut retired: Vec<Arc<Upload>> = Vec::new();
    let mut map = uploads();
    let next = map.range(address..).next().map(|(&k, v)| (k, v.clone()));
    if let Some((key, ref upload)) = next {
        if key == address && upload.resource.as_raw() == resource {
            return;
        }
    }
    let overlaps_next = next
        .as_ref()
        .is_some_and(|&(key, _)| key < address + desc.Width);
    let overlaps_previous = map
        .range(..address)
        .next_back()
        .is_some_and(|(&key, v)| key + v.bytes > address);
    if overlaps_next || overlaps_previous {
        at32(t.failures, 0).fetch_add(1, SeqCst);
        return;
    }
    let bytes = at64(t.bytes, 0);
    while !map.is_empty()
        && (map.len() >= 256 || desc.Width > BYTE_LIMIT.wrapping_sub(bytes.load(SeqCst)))
    {
        let mut oldest: Option<(u64, u64)> = None;
        for (&key, value) in map.iter() {
            let used = value.used.load(Relaxed);
            if oldest.is_none_or(|(_, best)| used < best) {
                oldest = Some((key, used));
            }
        }
        let (key, _) = oldest.unwrap();
        let evicted = map.remove(&key).unwrap();
        bytes.fetch_sub(evicted.bytes, SeqCst);
        retired.push(evicted);
        GENERATION.fetch_add(1, Release);
        at32(t.evictions, 0).fetch_add(1, SeqCst);
    }
    let mut owned = Upload {
        resource: r.clone(),
        mapped: core::ptr::null_mut(),
        bytes: desc.Width,
        used: AtomicU64::new(0),
        fixed_heap,
    };
    let range = D3D12_RANGE {
        Begin: 0,
        End: desc.Width as usize,
    };
    let mut mapped: *mut c_void = core::ptr::null_mut();
    if unsafe { r.Map(0, Some(&range), Some(&mut mapped)) }.is_err() {
        at32(t.failures, 0).fetch_add(1, SeqCst);
        return;
    }
    if mapped.is_null() {
        let none = D3D12_RANGE::default();
        unsafe { r.Unmap(0, Some(&none)) };
        at32(t.failures, 0).fetch_add(1, SeqCst);
        return;
    }
    owned.mapped = mapped.cast();
    touch(&owned);
    map.insert(address, Arc::new(owned));
    GENERATION.fetch_add(1, Release);
    bytes.fetch_add(desc.Width, SeqCst);
    at32(t.count, 0).store(map.len() as u32, SeqCst);
}

fn upload_address_observed(resource: *mut c_void, address: u64) {
    receipt_clear();
    let Some(t) = attach() else { return };
    if !observing(t) || resource.is_null() || address == 0 {
        return;
    }
    let stats = |i: usize| at64(t.fast_stats, i).fetch_add(1, SeqCst);
    if at32(t.fast_path, 0).load(Relaxed) != 1 {
        hint_clear();
        upload_created(resource);
        return;
    }
    if let Some(owned) = hint_lock(address, 1, resource) {
        touch(&owned);
        receipt_set(address, owned.bytes, hint_generation());
        stats(0);
        stats(5);
        return;
    }
    // The API supplies a live resource: matching its retained owner avoids repeating the immutable resource and heap
    // queries for every allocation.
    let known = || {
        let map = uploads();
        let Some(found) = map.get(&address) else {
            return false;
        };
        if found.resource.as_raw() != resource {
            return false;
        }
        touch(found);
        hint_remember(address, found);
        receipt_set(address, found.bytes, GENERATION.load(Relaxed))
    };
    if known() {
        stats(0);
        return;
    }
    stats(1);
    upload_created(resource);
    if !known() {
        hint_clear();
    }
}

/// NativeCbvSource: the buffer behind a CBV the registry does not know, found at the exact native CreateCBV return
/// sites (verified from the current EXE) while the uploader owns the allocation: the buffer must hold the view and
/// its own mapping must be the uploader's CPU allocation, not merely a recycled GPU address in a native weak table.
struct NativeSource {
    resource: *mut c_void,
}
fn native_cbv_source(desc: &D3D12_CONSTANT_BUFFER_VIEW_DESC) -> Option<NativeSource> {
    use cvr_engine::stack::{read_value, walk};
    let game = cvr_engine::code::image_base() as u64;
    walk(24, |c, _low, high| {
        let (resource, cpu) =
            if c.Rip == game.wrapping_add(0x333C1A) && c.Rsp <= high.wrapping_sub(0x50) {
                let Some(resource) = read_value::<u64>(c.Rsp.wrapping_add(0x30)) else {
                    return Some(None);
                };
                let Some(cpu) = read_value::<u64>(c.Rsp.wrapping_add(0x48)) else {
                    return Some(None);
                };
                (resource, cpu)
            } else if c.Rip == game.wrapping_add(0x3346B1) {
                if c.R14 > u64::MAX - 0x14F2240 {
                    return Some(None);
                }
                let Some(resource) = read_value::<u64>(c.R14 + 0x14F2240) else {
                    return Some(None);
                };
                (resource, c.R15)
            } else {
                (0, 0)
            };
        if resource == 0 || cpu == 0 {
            return None;
        }
        let raw = resource as *mut c_void;
        let object = borrowed(&raw)?;
        // SAFETY (whole block): the uploader's live buffer at its own call site.
        unsafe {
            let d = object.GetDesc();
            let gpu = object.GetGPUVirtualAddress();
            if d.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
                || desc.BufferLocation < gpu
                || desc.BufferLocation - gpu > d.Width
                || desc.SizeInBytes as u64 > d.Width - (desc.BufferLocation - gpu)
            {
                return Some(None);
            }
            let mut heap = D3D12_HEAP_PROPERTIES::default();
            let mut flags = D3D12_HEAP_FLAGS::default();
            if object
                .GetHeapProperties(Some(&mut heap), Some(&mut flags))
                .is_err()
                || (heap.Type != D3D12_HEAP_TYPE_UPLOAD
                    && heap.Type != D3D12_HEAP_TYPE_GPU_UPLOAD
                    && !(heap.Type == D3D12_HEAP_TYPE_CUSTOM
                        && (heap.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE
                            || heap.CPUPageProperty == D3D12_CPU_PAGE_PROPERTY_WRITE_BACK)))
            {
                return Some(None);
            }
            let offset = desc.BufferLocation - gpu;
            let range = D3D12_RANGE {
                Begin: offset as usize,
                End: (offset + desc.SizeInBytes as u64) as usize,
            };
            let mut mapped: *mut c_void = core::ptr::null_mut();
            if object.Map(0, Some(&range), Some(&mut mapped)).is_err() {
                return Some(None);
            }
            let same = !mapped.is_null()
                && (mapped as u64) <= u64::MAX - offset
                && (mapped as u64).wrapping_add(offset) == cpu;
            let none = D3D12_RANGE::default();
            object.Unmap(0, Some(&none));
            Some(same.then_some(NativeSource { resource: raw }))
        }
    })
    .flatten()
}

fn touch_upload(desc: *const D3D12_CONSTANT_BUFFER_VIEW_DESC) {
    let Some(t) = attach() else { return };
    // SAFETY: the caller's view description, or null.
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return;
    };
    if !observing(t) {
        return;
    }
    let _profile = Profile::new(t, PHASE_TOUCH_UPLOAD);
    let stats = |i: usize| at64(t.fast_stats, i).fetch_add(1, SeqCst);
    let fast = at32(t.fast_path, 0).load(Relaxed) == 1;
    let touched = receipt_take(
        desc.BufferLocation,
        desc.SizeInBytes as u64,
        GENERATION.load(Acquire),
    );
    if fast && touched {
        stats(2);
        return;
    }
    if fast {
        if let Some(owned) = hint_lock(
            desc.BufferLocation,
            desc.SizeInBytes as u64,
            core::ptr::null_mut(),
        ) {
            touch(&owned);
            stats(4);
            return;
        }
    } else {
        hint_clear();
    }
    stats(3);
    {
        let map = uploads();
        if let Some((&key, owned)) = map.range(..=desc.BufferLocation).next_back() {
            let offset = desc.BufferLocation - key;
            if offset <= owned.bytes && desc.SizeInBytes as u64 <= owned.bytes - offset {
                touch(owned);
                if fast {
                    hint_remember(key, owned);
                }
                return;
            }
        }
    }
    let source = {
        let _unwind = Profile::new(t, PHASE_UNWIND_UPLOAD);
        native_cbv_source(desc)
    };
    match source {
        Some(source) => {
            upload_created(source.resource);
            at32(t.native_imports, 0).fetch_add(1, SeqCst);
        }
        None => {
            at32(t.native_rejects, 0).fetch_add(1, SeqCst);
        }
    }
}

/// RegisteredSource: the resource of the record the native BufUpload(handle, size, data) is about to access (not a
/// historical GPU-address entry; low type 6 is a CBV). An opaque identity only: no resource method is called and
/// nothing is retained until CopyBufferRegion supplies the actual live destination.
fn registered_source(handle: u32, size: u32) -> *mut c_void {
    use cvr_engine::stack::{read, read_value};
    let game = cvr_engine::code::image_base() as u64;
    if handle == 0 || handle > 0x100000 || size == 0 || size > 65536 {
        return core::ptr::null_mut();
    }
    let Some(registry) = read_value::<u64>(game.wrapping_add(0x3BCDCA8)) else {
        return core::ptr::null_mut();
    };
    let offset = (handle as u64 - 1) * 0xB0;
    if registry == 0 || registry > u64::MAX - 0x5C0B18 - offset {
        return core::ptr::null_mut();
    }
    let mut record = [0u8; 32];
    if !read(registry + offset + 0x5C0AF8, &mut record) {
        return core::ptr::null_mut();
    }
    let bytes = u32::from_le_bytes(record[0..4].try_into().unwrap());
    let address = u64::from_le_bytes(record[8..16].try_into().unwrap());
    let resource = u64::from_le_bytes(record[16..24].try_into().unwrap());
    if record[6] & 15 != 6 || size > bytes || resource == 0 || address == 0 {
        return core::ptr::null_mut();
    }
    resource as *mut c_void
}

fn read_owned_upload(address: u64, output: *mut c_void, bytes: usize, fixed_only: bool) -> bool {
    let Some(t) = attach() else { return false };
    if address == 0 || output.is_null() || bytes == 0 || bytes > 65536 {
        return false;
    }
    let miss = || {
        at64(t.misses, 0).fetch_add(1, SeqCst);
        false
    };
    let map = uploads();
    let Some((&key, owned)) = map.range(..=address).next_back() else {
        return miss();
    };
    let offset = address - key;
    if offset > owned.bytes
        || bytes as u64 > owned.bytes - offset
        || (fixed_only && !owned.fixed_heap)
    {
        return miss();
    }
    let mut copied = 0usize;
    // SAFETY: the registry's own mapping (the copy reports what it could read).
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            owned.mapped.add(offset as usize).cast(),
            output,
            bytes,
            Some(&mut copied),
        )
    }
    .is_ok();
    if !ok || copied != bytes {
        return miss();
    }
    touch(owned);
    at64(t.reads, 0).fetch_add(1, SeqCst);
    true
}

// ---- the C ABI (include/Render/ConstantUploadRegistry.hpp in the Rust build)

/// # Safety
/// `resource` is a live resource or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_created(resource: *mut c_void) {
    upload_created(resource);
}
/// # Safety
/// `resource` is a live resource or null; `address` its own GetGPUVirtualAddress.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_address_observed(resource: *mut c_void, address: u64) {
    upload_address_observed(resource, address);
}
/// # Safety
/// `observer` is a C function of (ID3D12Resource*, uint64_t, uintptr_t) or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_set_creation_observer(observer: usize) {
    CREATION_OBSERVER.store(observer, Release);
}
/// # Safety
/// `resource` is a live resource or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_observe_resource_api(resource: *mut c_void) {
    observe_resource_api(resource);
}
/// # Safety
/// `desc` is a valid view description or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_touch(desc: *const D3D12_CONSTANT_BUFFER_VIEW_DESC) {
    touch_upload(desc);
}
#[no_mangle]
pub extern "C" fn cvr_upload_registered_source(handle: u32, size: u32) -> *mut c_void {
    registered_source(handle, size)
}
/// # Safety
/// `output` holds `bytes` bytes or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_read(
    address: u64,
    output: *mut c_void,
    bytes: usize,
    fixed_only: bool,
) -> bool {
    read_owned_upload(address, output, bytes, fixed_only)
}
