//! THE NATIVE TEXTURES' CREATION RECEIPTS (formerly the registry of src/Render/NativeTextureRead.cpp): every resource
//! the engine creates gets its placement (committed, or placed with its heap retained as private data); a texture
//! created in COMMON on a default heap with a known placement also gets a receipt -- a COM object held as the resource's
//! private data -- whose creation proof follows the engine's initial upload: the copy list that writes it (the
//! registry's own upload context), that list's submission on the registry's upload queue, and nothing else touching
//! its placement (a transition, an alias, a write by anything else). A read may acquire a receipt once, after the
//! upload completed; the rest of the module (src/Render/NativeTextureRead.cpp: Acquire, the immutable leases, the fixed
//! pins and the writable identities) asks this registry through the C ABI below.
//!
//! The observers stay opt-in (observe_bindings). Receipts and recordings are retired on their last release (which may
//! happen under a D3D12 object lock) and deleted under this registry's lock.
use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface as _, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use crate::buffer_read::{History, RecordingState};
use crate::texture_read::{
    alias_affects, could_alias, immutable_shape, known_allocation, Allocation,
};

const PLACEMENT_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd53488);
const HEAP_OWNER_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd53489);
const RECEIPT_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd5348a);
const RECORDING_COOKIE_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd5348b);
const RECORDING_LIFETIME_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd5348c);
const RECEIPT_LIMIT: usize = 16384;
const RECORDING_LIMIT: usize = 2048;
const WRITE_LIMIT: usize = 4096;

/// The resource's placement (its private data): the allocation and its receipt, if any.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Placement {
    pub allocation: Allocation,
    pub receipt: u64,
}
const _: () = assert!(core::mem::size_of::<Placement>() == 40);

/// detail::CreationProof: the upload history of a new texture, acquired at most once.
#[derive(Clone)]
struct CreationProof {
    history: History,
    acquired: bool,
}
impl CreationProof {
    fn new(enabled: bool) -> Self {
        let mut history = History::new();
        history.valid = enabled;
        CreationProof {
            history,
            acquired: false,
        }
    }
    fn acquire(&mut self) -> bool {
        if !self.history.matches(self.history.epoch) {
            return false;
        }
        self.acquired = true;
        true
    }
    fn written(&mut self, recording: u64) {
        if self.acquired {
            self.history.invalidate();
            return;
        }
        self.history.written(recording);
    }
}

struct ReceiptData {
    id: u64,
    resource: usize,
    /// Retained only: the placement heap lives as long as the receipt.
    #[allow(dead_code)]
    heap: Option<ID3D12Heap>,
    upload_queue: Option<ID3D12CommandQueue>,
    registry: u64,
    allocation: Allocation,
    proof: CreationProof,
}
struct RecordingData {
    cookie: u64,
    generation: u64,
    list: usize,
    state: RecordingState,
    writes: Vec<u64>,
}
#[repr(C)]
struct Held<T> {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    next: *mut Held<T>,
    data: T,
}
static RETIRED_RECEIPTS: AtomicPtr<Held<ReceiptData>> = AtomicPtr::new(core::ptr::null_mut());
static RETIRED_RECORDINGS: AtomicPtr<Held<RecordingData>> = AtomicPtr::new(core::ptr::null_mut());
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn held_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Held (refs at +8).
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn held_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Held (refs at +8).
    unsafe { (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst) + 1 }
}
/// The last release only retires: resource destruction may hold a D3D12 object lock, so neither this registry's lock
/// nor the retained heap is touched here.
unsafe fn retire<T>(this: *mut c_void, stack: &AtomicPtr<Held<T>>) -> u32 {
    let held = this as *mut Held<T>;
    // SAFETY: a live Held; at zero it is pushed on its retired stack.
    unsafe {
        let n = (*held).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let mut next = stack.load(Relaxed);
            loop {
                (*held).next = next;
                match stack.compare_exchange_weak(next, held, Release, Relaxed) {
                    Ok(_) => break,
                    Err(actual) => next = actual,
                }
            }
        }
        n
    }
}
unsafe extern "system" fn receipt_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { retire(this, &RETIRED_RECEIPTS) }
}
unsafe extern "system" fn recording_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { retire(this, &RETIRED_RECORDINGS) }
}
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: held_query,
    AddRef: held_add_ref,
    Release: receipt_release,
};
static RECORDING_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: held_query,
    AddRef: held_add_ref,
    Release: recording_release,
};

struct Registry {
    receipts: BTreeMap<u64, *mut Held<ReceiptData>>,
    recordings: BTreeMap<u64, *mut Held<RecordingData>>,
    next_receipt: u64,
    next_cookie: u64,
    next_recording: u64,
    failure: bool,
}
// SAFETY: the raw objects are reached only under the registry's lock.
unsafe impl Send for Registry {}
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    receipts: BTreeMap::new(),
    recordings: BTreeMap::new(),
    next_receipt: 0,
    next_cookie: 0,
    next_recording: 0,
    failure: false,
});
fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|p| p.into_inner())
}
fn receipt<'a>(p: *mut Held<ReceiptData>) -> &'a mut ReceiptData {
    // SAFETY: a registered receipt, reached under the registry's lock.
    unsafe { &mut (*p).data }
}
fn recording<'a>(p: *mut Held<RecordingData>) -> &'a mut RecordingData {
    // SAFETY: a registered recording, reached under the registry's lock.
    unsafe { &mut (*p).data }
}
fn drain_retired(reg: &mut Registry) {
    let mut p = RETIRED_RECEIPTS.swap(core::ptr::null_mut(), Acquire);
    while !p.is_null() {
        // SAFETY: a retired receipt (refcount 0), deleted once here.
        let next = unsafe { (*p).next };
        let id = receipt(p).id;
        if reg.receipts.get(&id) == Some(&p) {
            reg.receipts.remove(&id);
        }
        drop(unsafe { Box::from_raw(p) });
        p = next;
    }
    let mut p = RETIRED_RECORDINGS.swap(core::ptr::null_mut(), Acquire);
    while !p.is_null() {
        // SAFETY: a retired recording (refcount 0), deleted once here.
        let next = unsafe { (*p).next };
        let (cookie, generation) = (recording(p).cookie, recording(p).generation);
        if reg.recordings.get(&cookie) == Some(&p) {
            for &r in reg.receipts.values() {
                receipt(r).proof.history.abandon(generation);
            }
            reg.recordings.remove(&cookie);
        }
        drop(unsafe { Box::from_raw(p) });
        p = next;
    }
}
fn observe() -> bool {
    crate::stereo_probe::observe_bindings()
}
fn game() -> u64 {
    cvr_engine::code::image_base() as u64
}
fn read_value<T: Copy + Default>(address: u64) -> Option<T> {
    if address == 0 || address > u64::MAX - core::mem::size_of::<T>() as u64 {
        return None;
    }
    cvr_engine::stack::read_value(address)
}
fn object<'a>(p: &'a *mut c_void) -> Option<&'a ID3D12Object> {
    // SAFETY: a live D3D12 object of the caller's, or null.
    unsafe { ID3D12Object::from_raw_borrowed(p) }
}
/// PlacementOf: the resource's placement private data (all zero when it has none).
pub fn placement_of(resource: *mut c_void) -> Placement {
    let Some(r) = object(&resource) else {
        return Placement::default();
    };
    let mut placement = Placement::default();
    let mut bytes = 40u32;
    // SAFETY: at most 40 bytes written into the placement.
    let ok = unsafe {
        r.GetPrivateData(
            &PLACEMENT_KEY,
            &mut bytes,
            Some(&mut placement as *mut Placement as *mut c_void),
        )
    };
    if ok.is_err() || bytes != 40 {
        return Placement::default();
    }
    placement
}
fn recording_of(reg: &Registry, list: *mut c_void) -> Option<*mut Held<RecordingData>> {
    let l = object(&list)?;
    let mut cookie = 0u64;
    let mut bytes = 8u32;
    // SAFETY: at most 8 bytes written into `cookie`.
    unsafe {
        l.GetPrivateData(
            &RECORDING_COOKIE_KEY,
            &mut bytes,
            Some(&mut cookie as *mut u64 as *mut c_void),
        )
    }
    .ok()?;
    if bytes != 8 {
        return None;
    }
    let &p = reg.recordings.get(&cookie)?;
    (recording(p).list == list as usize).then_some(p)
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}
/// FixedCompatible: the engine functions behind the fixed-asset and initial-upload paths are the audited ones (one
/// 2.5 rewrite is recorded with no audited hash, so this refuses until it is re-audited -- as the C++ did).
pub fn fixed_compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| {
        const EXPECTED: [(u64, usize, u64); 20] = [
            (0x346f24, 771, 0x51ba1810b87b452e),
            (0x5ea284, 196, 0x511481cf9505a352),
            (0x625a58, 834, 0x3e4654fab947d49f),
            (0x347f74, 687, 0x0eaa63353e6cd86c),
            (0x3490f4, 98, 0xaf5dd974e8f5ed23),
            (0x346374, 1076, 0x36978fd0f3168dce),
            (0x342f48, 1188, 0xadfee15f3177a181),
            (0x3e5d54, 1789, 0x63d0eefeac6254d8),
            (0x3e8a9c, 55, 0xc10a06ab43ea88f1),
            (0x3e7b90, 180, 0x22067cf1c84d3113),
            (0x5b3da8, 641, 0x5e94d16a5dbeca40),
            (0x5b4110, 77, 0x9e7d6243f6f033c4),
            (0x22ef830, 1470, 0x3541631d8d75a2e6),
            (0x3e1020, 682, 0x898f20179715110f),
            (0x3e47e8, 371, 0x936d5b6c41cb0a2e),
            (0x3e12cc, 148, 0xa99eac2c327a1a01),
            (0x3e1abc, 97, 0xf0a1f30d3afc8031),
            // 2.5 rewrite of 2.31 sub_14021EB24 (8 call sites agree), not re-audited.
            (0x22F09C0, 0x3F, 0x0000000000000000),
            (0x34b588, 346, 0x6e1062b700b72556),
            (0x34c16c, 438, 0xf20d07f3c00d184f),
        ];
        let base = game();
        let mut bytes = [0u8; 2048];
        EXPECTED.iter().all(|&(rva, len, hash)| {
            len <= bytes.len()
                && cvr_engine::stack::read(base + rva, &mut bytes[..len])
                && fnv(&bytes[..len]) == hash
        })
    })
}
fn queue_type(queue: &ID3D12CommandQueue) -> D3D12_COMMAND_LIST_TYPE {
    // SAFETY: a live queue.
    unsafe { queue.GetDesc() }.Type
}
/// NativeQueue: `queue` is the registry's own upload (or direct) queue.
pub fn native_queue(registry: u64, queue: Option<&ID3D12CommandQueue>, upload: bool) -> bool {
    let offset = if upload { 0x1410630 } else { 0x14104d0 };
    let Some(queue) = queue else { return false };
    registry != 0
        && registry <= u64::MAX - offset - 8
        && read_value::<u64>(game() + 0x3bcdca8) == Some(registry)
        && read_value::<u64>(registry + offset) == Some(queue.as_raw() as u64)
        && queue_type(queue)
            == if upload {
                D3D12_COMMAND_LIST_TYPE_COPY
            } else {
                D3D12_COMMAND_LIST_TYPE_DIRECT
            }
}
fn list_type(list: *mut c_void) -> Option<D3D12_COMMAND_LIST_TYPE> {
    // SAFETY: a live command list of the caller's, or null.
    unsafe { ID3D12CommandList::from_raw_borrowed(&list) }.map(|l| unsafe { l.GetType() })
}
/// InitialUploader: the list is the registry's own upload context's copy list; its registry and upload queue.
fn initial_uploader(list: *mut c_void) -> Option<(u64, ID3D12CommandQueue)> {
    if list_type(list) != Some(D3D12_COMMAND_LIST_TYPE_COPY) || !fixed_compatible() {
        return None;
    }
    let registry = read_value::<u64>(game() + 0x3bcdca8)?;
    if registry == 0 || registry > u64::MAX - 0x1410638 {
        return None;
    }
    let context = read_value::<u64>(registry + 0x1410628)?;
    if context == 0 || context > u64::MAX - 0x6c {
        return None;
    }
    if read_value::<u64>(context + 0x30)? != list as u64 || read_value::<u32>(context + 0x68)? != 3
    {
        return None;
    }
    let raw = read_value::<u64>(registry + 0x1410630)? as *mut c_void;
    // SAFETY: the engine's live upload queue (an ID3D12CommandQueue it owns).
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&raw) }?.clone();
    (queue_type(&queue) == D3D12_COMMAND_LIST_TYPE_COPY).then_some((registry, queue))
}
fn default_heap(resource: &ID3D12Resource) -> bool {
    let mut properties = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    unsafe { resource.GetHeapProperties(Some(&mut properties), Some(&mut flags)) }.is_ok()
        && properties.Type == D3D12_HEAP_TYPE_DEFAULT
}

// ---- the observers

fn receipt_error() {
    registry().failure = true;
}
fn resource_created(
    resource: *mut c_void,
    initial: u32,
    placed: bool,
    heap: *mut c_void,
    offset: u64,
    bytes: u64,
) {
    if !observe() {
        return;
    }
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    let mut reg = registry();
    drain_retired(&mut reg);
    if placement_of(resource).allocation.kind != 0 {
        return;
    }
    let mut placement = Placement::default();
    placement.allocation.kind = if placed { 2 } else { 1 };
    // SAFETY: a live heap of the caller's, or null.
    let heap_ref = unsafe { ID3D12Heap::from_raw_borrowed(&heap) };
    if placed {
        if let Some(h) = heap_ref {
            // SAFETY: the resource keeps its own reference to the heap.
            if unsafe { r.SetPrivateDataInterface(&HEAP_OWNER_KEY, h) }.is_ok() {
                placement.allocation = Allocation {
                    heap: heap as u64,
                    offset,
                    bytes,
                    kind: 2,
                };
            }
        }
    }
    let mut owned: Option<IUnknown> = None;
    let mut held: *mut Held<ReceiptData> = core::ptr::null_mut();
    // SAFETY: a live resource.
    let desc = unsafe { r.GetDesc() };
    if !cvr_host::probe::internal()
        && !reg.failure
        && initial == D3D12_RESOURCE_STATE_COMMON.0 as u32
        && immutable_shape(&desc)
        && default_heap(r)
        && known_allocation(&placement.allocation)
        && reg.receipts.len() < RECEIPT_LIMIT
        && reg.next_receipt != u64::MAX
    {
        reg.next_receipt += 1;
        held = Box::into_raw(Box::new(Held {
            vtbl: &RECEIPT_VTBL,
            refs: AtomicU32::new(1),
            next: core::ptr::null_mut(),
            data: ReceiptData {
                id: reg.next_receipt,
                resource: resource as usize,
                heap: heap_ref.cloned(),
                upload_queue: None,
                registry: 0,
                allocation: placement.allocation,
                proof: CreationProof::new(true),
            },
        }));
        // SAFETY: the new object, an IUnknown (the creator's reference, dropped below).
        let unknown = unsafe { IUnknown::from_raw(held.cast()) };
        if unsafe { r.SetPrivateDataInterface(&RECEIPT_KEY, &unknown) }.is_ok() {
            placement.receipt = receipt(held).id;
        }
        owned = Some(unknown);
    }
    // SAFETY: 40 bytes of the placement.
    let set = unsafe {
        r.SetPrivateData(
            &PLACEMENT_KEY,
            40,
            Some(&placement as *const Placement as *const c_void),
        )
    };
    if set.is_ok() && placement.receipt != 0 {
        reg.receipts.insert(placement.receipt, held);
    }
    drop(reg);
    drop(owned);
}
pub(crate) fn alias_affects_resources(watched: *mut c_void, before: *mut c_void, after: *mut c_void) -> bool {
    if watched.is_null() || !observe() {
        return true;
    }
    let reg = registry();
    if reg.failure {
        return true;
    }
    let (source, from, to) = (
        placement_of(watched),
        placement_of(before),
        placement_of(after),
    );
    alias_affects(
        watched as u64,
        &source.allocation,
        before as u64,
        &from.allocation,
        after as u64,
        &to.allocation,
    )
}
/// QueryObservedAllocation: the resource's observed placement (committed, or placed with its retained heap, checked
/// against the heap's own size and the resource's allocation info). `heap_out` receives an owned reference.
fn query_observed_allocation(
    resource: *mut c_void,
    kind: &mut u32,
    heap_out: &mut *mut c_void,
    offset: &mut u64,
    bytes: &mut u64,
) -> bool {
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    if !observe() {
        return false;
    }
    let reg = registry();
    if reg.failure {
        return false;
    }
    let saved = placement_of(resource);
    let a = saved.allocation;
    if !known_allocation(&a) {
        return false;
    }
    let mut heap: Option<ID3D12Heap> = None;
    if a.kind == 1 {
        if a.heap != 0 || a.offset != 0 || a.bytes != 0 {
            return false;
        }
    } else {
        let mut raw: *mut c_void = core::ptr::null_mut();
        let mut size = 8u32;
        // SAFETY: the interface private data: an owned IUnknown pointer.
        if unsafe {
            r.GetPrivateData(
                &HEAP_OWNER_KEY,
                &mut size,
                Some(&mut raw as *mut *mut c_void as *mut c_void),
            )
        }
        .is_err()
            || size != 8
            || raw.is_null()
        {
            return false;
        }
        // SAFETY: the reference GetPrivateData added.
        let owner = unsafe { IUnknown::from_raw(raw) };
        let Ok(h) = owner.cast::<ID3D12Heap>() else {
            return false;
        };
        if h.as_raw() as u64 != a.heap {
            return false;
        }
        // SAFETY (whole block): live objects.
        unsafe {
            let (mut resource_device, mut heap_device): (
                Option<ID3D12Device>,
                Option<ID3D12Device>,
            ) = (None, None);
            if r.GetDevice(&mut resource_device).is_err() || h.GetDevice(&mut heap_device).is_err()
            {
                return false;
            }
            let (Some(resource_device), Some(heap_device)) = (resource_device, heap_device) else {
                return false;
            };
            if resource_device.as_raw() != heap_device.as_raw() {
                return false;
            }
            let heap_desc = h.GetDesc();
            let desc = r.GetDesc();
            let required = resource_device.GetResourceAllocationInfo(0, &[desc]);
            if a.offset > heap_desc.SizeInBytes
                || a.bytes > heap_desc.SizeInBytes - a.offset
                || required.SizeInBytes == 0
                || required.SizeInBytes == u64::MAX
                || required.Alignment == 0
                || a.offset % required.Alignment != 0
                || a.bytes < required.SizeInBytes
            {
                return false;
            }
        }
        heap = Some(h);
    }
    let current = placement_of(resource);
    if current.receipt != saved.receipt
        || current.allocation.heap != a.heap
        || current.allocation.offset != a.offset
        || current.allocation.bytes != a.bytes
        || current.allocation.kind != a.kind
    {
        return false;
    }
    drop(reg);
    *kind = a.kind;
    *offset = if a.kind == 2 { a.offset } else { 0 };
    *bytes = if a.kind == 2 { a.bytes } else { 0 };
    *heap_out = heap.map_or(core::ptr::null_mut(), |h| h.into_raw());
    true
}
fn barriers(count: u32, items: *const D3D12_RESOURCE_BARRIER) {
    if !observe() || count == 0 {
        return;
    }
    if items.is_null() {
        receipt_error();
        return;
    }
    // SAFETY: the caller's `count` barriers.
    let items = unsafe { core::slice::from_raw_parts(items, count as usize) };
    let mut reg = registry();
    drain_retired(&mut reg);
    if reg.receipts.is_empty() {
        return;
    }
    for b in items {
        // SAFETY: the union member the barrier's type names.
        unsafe {
            if b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
                let t = &b.Anonymous.Transition;
                let resource = t
                    .pResource
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                if resource.is_null() {
                    reg.failure = true;
                    return;
                }
                if let Some(&p) = reg.receipts.get(&placement_of(resource).receipt) {
                    receipt(p).proof.history.invalidate();
                }
            } else if b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING {
                let a = &b.Anonymous.Aliasing;
                let before = a
                    .pResourceBefore
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                let after = a
                    .pResourceAfter
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                let (from, to) = (placement_of(before), placement_of(after));
                for &p in reg.receipts.values() {
                    let r = receipt(p);
                    if alias_affects(
                        r.resource as u64,
                        &r.allocation,
                        before as u64,
                        &from.allocation,
                        after as u64,
                        &to.allocation,
                    ) {
                        r.proof.history.invalidate();
                    }
                }
            }
        }
    }
}
fn written(list: *mut c_void, destination: *mut c_void) {
    if !observe() {
        return;
    }
    if destination.is_null() {
        receipt_error();
        return;
    }
    let placement = placement_of(destination);
    let mut reg = registry();
    drain_retired(&mut reg);
    for &p in reg.receipts.values() {
        let r = receipt(p);
        if r.resource != destination as usize && could_alias(&r.allocation, &placement.allocation) {
            r.proof.history.invalidate();
        }
    }
    let Some(&rp) = reg.receipts.get(&placement.receipt) else {
        return;
    };
    let r = receipt(rp);
    if r.resource != destination as usize || !r.proof.history.valid {
        return;
    }
    let origin = initial_uploader(list);
    let rec = recording_of(&reg, list).map(recording);
    let (Some((origin_registry, origin_queue)), Some(c)) = (origin, rec) else {
        r.proof.history.invalidate();
        return;
    };
    if !c.state.valid
        || c.state.pending
        || c.state.submitted
        || (r.registry != 0
            && (r.registry != origin_registry
                || r.upload_queue.as_ref().map(|q| q.as_raw()) != Some(origin_queue.as_raw())))
    {
        r.proof.history.invalidate();
        return;
    }
    if !c.writes.contains(&r.id) {
        if c.writes.len() >= WRITE_LIMIT {
            reg.failure = true;
            return;
        }
        c.writes.push(r.id);
    }
    r.registry = origin_registry;
    r.upload_queue = Some(origin_queue);
    r.proof.written(c.generation);
}
fn reset(list: *mut c_void) {
    if !observe() || list_type(list) != Some(D3D12_COMMAND_LIST_TYPE_COPY) {
        return;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    if let Some(cp) = recording_of(&reg, list) {
        let generation = recording(cp).generation;
        for &p in reg.receipts.values() {
            receipt(p).proof.history.abandon(generation);
        }
        let c = recording(cp);
        if reg.next_recording == u64::MAX {
            c.generation = 0;
            reg.failure = true;
            return;
        }
        reg.next_recording += 1;
        c.generation = reg.next_recording;
        c.state = RecordingState::default();
        c.writes.clear();
        return;
    }
    if reg.recordings.len() >= RECORDING_LIMIT
        || reg.next_cookie == u64::MAX
        || reg.next_recording == u64::MAX
    {
        reg.failure = true;
        return;
    }
    reg.next_cookie += 1;
    reg.next_recording += 1;
    let cookie = reg.next_cookie;
    let held = Box::into_raw(Box::new(Held {
        vtbl: &RECORDING_VTBL,
        refs: AtomicU32::new(1),
        next: core::ptr::null_mut(),
        data: RecordingData {
            cookie,
            generation: reg.next_recording,
            list: list as usize,
            state: RecordingState::default(),
            writes: Vec::new(),
        },
    }));
    // SAFETY: the new object, an IUnknown (the creator's reference, dropped below); the list keeps its own.
    let owned = unsafe { IUnknown::from_raw(held.cast()) };
    let l = object(&list).unwrap();
    let set = unsafe {
        l.SetPrivateDataInterface(&RECORDING_LIFETIME_KEY, &owned)
            .is_ok()
            && l.SetPrivateData(
                &RECORDING_COOKIE_KEY,
                8,
                Some(&cookie as *const u64 as *const c_void),
            )
            .is_ok()
    };
    if !set {
        reg.failure = true;
        return;
    }
    reg.recordings.insert(cookie, held);
    drop(reg);
    drop(owned);
}
fn submit_step(queue: *mut c_void, count: u32, lists: *const *mut c_void, after: bool) {
    if !observe() || count == 0 {
        return;
    }
    if queue.is_null() || lists.is_null() {
        receipt_error();
        return;
    }
    // SAFETY: the caller's queue and `count` lists.
    let q = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) };
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let mut reg = registry();
    drain_retired(&mut reg);
    for &list in lists {
        let Some(cp) = recording_of(&reg, list) else {
            continue;
        };
        let c = recording(cp);
        if c.generation == 0 {
            continue;
        }
        let fresh = if after {
            c.state.submitted()
        } else {
            c.state.before_submit()
        };
        for id in c.writes.clone() {
            let Some(&p) = reg.receipts.get(&id) else {
                continue;
            };
            let r = receipt(p);
            let matches = fresh
                && r.upload_queue.as_ref().map(|q| q.as_raw()) == Some(queue)
                && native_queue(r.registry, q, true);
            if after {
                r.proof.history.submitted(c.generation, matches);
            } else {
                r.proof.history.before_submit(c.generation, matches);
            }
        }
    }
}
/// AcquireReceipt: the resource's receipt, once its initial upload completed and nothing else touched it (acquired
/// here, so a later write invalidates it); 0 otherwise. `failure` receives the module's failure code (68 on success,
/// as the C++ left it).
pub fn acquire_receipt(resource: *mut c_void, registry_address: u64, failure: &mut u32) -> u64 {
    let placement = placement_of(resource);
    let mut reg = registry();
    drain_retired(&mut reg);
    let found = reg.receipts.get(&placement.receipt).copied();
    *failure = 60;
    if !known_allocation(&placement.allocation) || placement.receipt == 0 {
        return 0;
    }
    *failure = 61;
    if reg.failure {
        return 0;
    }
    *failure = 62;
    let Some(p) = found else { return 0 };
    let r = receipt(p);
    *failure = 63;
    if r.resource != resource as usize {
        return 0;
    }
    *failure = 64;
    if r.registry != registry_address {
        return 0;
    }
    *failure = 65;
    if !native_queue(registry_address, r.upload_queue.as_ref(), true) {
        return 0;
    }
    *failure = 66;
    if !r.proof.history.valid {
        return 0;
    }
    *failure = 67;
    if !r.proof.history.uploaded || r.proof.history.pending {
        return 0;
    }
    *failure = 68;
    if !r.proof.acquire() {
        return 0;
    }
    placement.receipt
}
/// The registry's part of Validate: the receipt is still the resource's, acquired, uploaded and untouched.
pub fn receipt_valid(resource: *mut c_void, id: u64, registry_address: u64) -> bool {
    let placement = placement_of(resource);
    let mut reg = registry();
    drain_retired(&mut reg);
    let Some(&p) = reg.receipts.get(&id) else {
        return false;
    };
    let r = receipt(p);
    !reg.failure
        && placement.receipt == id
        && r.resource == resource as usize
        && r.registry == registry_address
        && r.proof.history.valid
        && r.proof.acquired
        && r.proof.history.uploaded
        && !r.proof.history.pending
}

// ---- the C ABI (src/Render/NativeTextureRead.cpp in the Rust build)

/// # Safety
/// `resource` and `heap` are live objects or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_resource_created(
    resource: *mut c_void,
    initial: u32,
    placed: bool,
    heap: *mut c_void,
    offset: u64,
    bytes: u64,
) {
    resource_created(resource, initial, placed, heap, offset, bytes);
}
/// # Safety
/// The resources are live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_alias_affects_resources(
    watched: *mut c_void,
    before: *mut c_void,
    after: *mut c_void,
) -> bool {
    alias_affects_resources(watched, before, after)
}
/// # Safety
/// `resource` is live or null; the outputs are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_query_observed_allocation(
    resource: *mut c_void,
    kind: *mut u32,
    heap: *mut *mut c_void,
    offset: *mut u64,
    bytes: *mut u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        *kind = 0;
        *heap = core::ptr::null_mut();
        *offset = 0;
        *bytes = 0;
        query_observed_allocation(resource, &mut *kind, &mut *heap, &mut *offset, &mut *bytes)
    }
}
/// # Safety
/// `items` holds `count` barriers (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_barriers(count: u32, items: *const D3D12_RESOURCE_BARRIER) {
    barriers(count, items);
}
/// # Safety
/// `list` and `destination` are live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_written(list: *mut c_void, destination: *mut c_void) {
    written(list, destination);
}
/// # Safety
/// `list` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_reset(list: *mut c_void) {
    reset(list);
}
/// # Safety
/// `lists` holds `count` command lists (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_before_submit(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    submit_step(queue, count, lists, false);
}
/// # Safety
/// As cvr_tr_before_submit.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_submitted(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    submit_step(queue, count, lists, true);
}
#[no_mangle]
pub extern "C" fn cvr_tr_unavailable() {
    receipt_error();
}
/// # Safety
/// `resource` is live or null; `failure` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_acquire_receipt(
    resource: *mut c_void,
    registry_address: u64,
    failure: *mut u32,
) -> u64 {
    // SAFETY: as the caller promises.
    acquire_receipt(resource, registry_address, unsafe { &mut *failure })
}
/// # Safety
/// `resource` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_receipt_valid(
    resource: *mut c_void,
    id: u64,
    registry_address: u64,
) -> bool {
    receipt_valid(resource, id, registry_address)
}
#[no_mangle]
pub extern "C" fn cvr_tr_fixed_compatible() -> bool {
    fixed_compatible()
}
/// # Safety
/// `queue` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_native_queue(
    registry_address: u64,
    queue: *mut c_void,
    upload: bool,
) -> bool {
    // SAFETY: as the caller promises.
    native_queue(
        registry_address,
        unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) },
        upload,
    )
}
