//! The source tables (formerly src/Render/StereoDescriptorAudit.cpp's CaptureSourceTablesAtDraw,
//! CaptureSourceSlotsAtDraw, their owner and capture receipts, the source table cache, MatchesCapturedSourceSrv,
//! MatchesSourceSlot and MatchesCpuSourceSrv, and the thread's source failure).
//!
//! At a native draw, each requested table (or each caller-selected slot of it) is laid out against the root, its
//! bound range walked in the store under the metadata lock (the live owners held), the CBVs backed through the
//! caller's provider outside the lock, the slots re-checked under the lock, and the captured values interned per heap
//! type into an owner -- a COM object holding the values, the leased owners and resources and the port's own CPU
//! heap with the emitted descriptors -- cached across draws. A table's capture receipt (another COM object) proves an
//! SRV later against its exact captured slot and held resource. The .cpp keeps the public types and builds them from
//! the rows emitted here, and calls the provider through a trampoline.

use crate::audit_source_capture::CaptureSlot;
use crate::audit_source_checks::RecheckSlot;
use crate::audit_sources::{Failure, SlotIn, SlotOut};
use crate::descriptor_audit::{self as core_, DescriptorStamp};
use crate::descriptor_store::{RangeIdentity, Value};
use crate::source_layout::{SlotPlan, SlotUse};
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::collections::{BTreeMap, BTreeSet};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

const OWNER_GUID: GUID = GUID::from_u128(0x18495B39_DC38_43BD_88FB_763265F404A9);
const CAPTURE_GUID: GUID = GUID::from_u128(0x66C72536_4657_4498_A4C0_6F118D661B95);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
pub const SLOT_CAPTURE_LIMIT: usize = 4096;
const FORBIDDEN: u32 =
    (D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE.0 | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED.0 | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED.0) as u32;

thread_local! {
    static FAILURE: Cell<[u32; 3]> = const { Cell::new([0, u32::MAX, u32::MAX]) };
}
/// SourceError: the thread's failure, always false.
fn error(reason: u32, parameter: u32, offset: u32) -> bool {
    FAILURE.with(|f| f.set([reason, parameter, offset]));
    false
}
fn error_at(reason: u32, parameter: u32) -> bool {
    error(reason, parameter, u32::MAX)
}
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

// ---- raw COM references
fn add_ref(p: *mut c_void) -> *mut c_void {
    // SAFETY: a live object (or null).
    if let Some(u) = unsafe { IUnknown::from_raw_borrowed(&p) } {
        core::mem::forget(u.clone());
    }
    p
}
fn release(p: *mut c_void) {
    if !p.is_null() {
        // SAFETY: one reference owned by the caller.
        drop(unsafe { IUnknown::from_raw(p) });
    }
}
/// The object's own `iid` interface (one reference), or null.
fn query(p: *mut c_void, iid: &GUID) -> *mut c_void {
    let Some(u) = (unsafe { IUnknown::from_raw_borrowed(&p) }) else { return core::ptr::null_mut() };
    let mut out = core::ptr::null_mut();
    // SAFETY: a live object's QueryInterface.
    unsafe { (u.vtable().QueryInterface)(p, iid, &mut out) }.is_ok().then_some(out).unwrap_or(core::ptr::null_mut())
}

// ---- the C++ public types' layouts
/// OwnedSourceTable.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OwnedTable {
    pub parameter: u32,
    pub count: u32,
    pub cpu_heap: *mut c_void,
    pub owner: *mut c_void,
    pub identity: u64,
    pub version: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub provenance: *mut c_void,
}
/// SrvBinding.
#[repr(C)]
pub struct SrvBinding {
    pub description: Value,
    pub resource: *mut c_void,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub stamp: DescriptorStamp,
}
/// OwnedSourceSlot.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OwnedSlot {
    pub offset: u32,
    pub compact_index: u32,
    pub kind: i32,
    pub stamp: DescriptorStamp,
}
/// An OwnedSourceSlots row: its slots live until the emit returns.
#[repr(C)]
pub struct OwnedSlotsRow {
    pub parameter: u32,
    pub count: u32,
    pub cpu_heap: *mut c_void,
    pub owner: *mut c_void,
    pub identity: u64,
    pub version: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub slots: *const OwnedSlot,
    pub slot_count: usize,
}
/// SourceResourceBinding (references handed over).
#[repr(C)]
pub struct Dependency {
    pub parameter: u32,
    pub offset: u32,
    pub kind: i32,
    pub stamp: DescriptorStamp,
    pub resource: *mut c_void,
    pub counter: *mut c_void,
}
const _: () = assert!(
    core::mem::size_of::<OwnedTable>() == 64
        && core::mem::size_of::<SrvBinding>() == 384
        && core::mem::size_of::<OwnedSlot>() == 192
        && core::mem::size_of::<OwnedSlotsRow>() == 72
        && core::mem::size_of::<Dependency>() == 208
);
/// SourceTableRequest.
#[repr(C)]
pub struct Request {
    pub parameter: u32,
    pub count: u32,
    pub uses: *const SlotUse,
    pub use_count: usize,
}
/// The scene_state::Bindings fields a capture reads.
#[repr(C)]
pub struct Bindings {
    pub root: *mut c_void,
    pub tables: *const u64,
    pub table_mask: u64,
    pub heaps: *const *mut c_void,
    pub heap_count: u32,
    pub heaps_seen: bool,
}
/// The .cpp's provider trampoline: 0 refused, 1 backed (`resource` and `owner` references handed over), 2 threw.
pub type Provider = unsafe extern "C-unwind" fn(context: *const c_void, list: *mut c_void, parameter: u32, offset: u32, stamp: *const DescriptorStamp, origin: *mut c_void, resource: *mut *mut c_void, owner: *mut *mut c_void) -> u32;
/// The .cpp's output: 0 an OwnedTable, 1 an OwnedSlotsRow, 2 a Dependency (references handed over).
pub type Emit = unsafe extern "C-unwind" fn(context: *mut c_void, which: u32, row: *const c_void);

// ---- the owner: interned values, leased owners and resources, the port's CPU heap
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    heap: *mut c_void,
    key: Vec<Value>,
    leases: Vec<*mut c_void>,
    resources: BTreeMap<u64, usize>,
    kind: i32,
    identity: u64,
}
impl Drop for Owner {
    fn drop(&mut self) {
        release(self.heap);
        self.leases.iter().for_each(|&l| release(l));
        self.resources.values().for_each(|&r| release(r as *mut c_void));
    }
}
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { proof_query(this, iid, out, &OWNER_GUID) }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    let owner = this as *mut Owner;
    // SAFETY: a live owner; freed at zero.
    let left = unsafe { (*owner).refs.fetch_sub(1, SeqCst) } - 1;
    if left == 0 {
        drop(unsafe { Box::from_raw(owner) });
    }
    left
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: proof_add_ref, Release: owner_release };
/// Both proofs begin with the vtable and the count.
unsafe extern "system" fn proof_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live proof (its count right after the vtable).
    unsafe { (*(this as *mut u8).add(8).cast::<AtomicU32>()).fetch_add(1, SeqCst) + 1 }
}
unsafe fn proof_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void, own: &GUID) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != *own) {
            return E_NOINTERFACE;
        }
        *out = this;
        proof_add_ref(this);
    }
    S_OK
}
impl Owner {
    fn create(key: Vec<Value>, leases: Vec<*mut c_void>, resources: BTreeMap<u64, usize>, kind: i32, identity: u64) -> OwnerRef {
        OwnerRef(Box::into_raw(Box::new(Owner { vtbl: &OWNER_VTBL, refs: AtomicU32::new(1), heap: core::ptr::null_mut(), key, leases, resources, kind, identity })))
    }
    /// SourceTableOwner::MatchesSrv against a receipt's own table.
    fn matches_srv(&self, owner: *mut c_void, cpu_heap: *mut c_void, identity: u64, version: u64, count: u32, offset: u32, binding: &SrvBinding) -> bool {
        if owner != self as *const Owner as *mut c_void || cpu_heap != self.heap || identity != self.identity || version != 1 || count as usize != self.key.len() || offset as usize >= self.key.len() || self.kind != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 || binding.resource.is_null() {
            return false;
        }
        let value = &self.key[offset as usize];
        // SAFETY: our value and the binding's view.
        if value[0] != 2 || value[2] == 0 || *value != binding.description || *value != binding.stamp.value || !unsafe { crate::source_layout::cvr_sl_descriptor(0, value, &binding.view, 0) } {
            return false;
        }
        self.resources.get(&value[2]).is_some_and(|&r| r as *mut c_void == binding.resource) && self.leases.iter().any(|&l| l == binding.resource)
    }
}
/// One counted reference to an owner.
struct OwnerRef(*mut Owner);
impl Clone for OwnerRef {
    fn clone(&self) -> Self {
        unsafe { proof_add_ref(self.0.cast()) };
        OwnerRef(self.0)
    }
}
impl Drop for OwnerRef {
    fn drop(&mut self) {
        unsafe { owner_release(self.0.cast()) };
    }
}
impl OwnerRef {
    fn get(&self) -> &Owner {
        // SAFETY: counted.
        unsafe { &*self.0 }
    }
    fn raw(&self) -> *mut c_void {
        self.0.cast()
    }
}
// SAFETY: owners are immutable once published; their counts are atomic.
unsafe impl Send for OwnerRef {}

// ---- the capture receipt: the exact captured table, proving an SRV against its owner
#[repr(C)]
struct Receipt {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    parameter: u32,
    count: u32,
    heap: *mut c_void,
    owner: *mut c_void,
    identity: u64,
    source_gpu: u64,
    source_heap: u64,
    stride: u32,
}
impl Drop for Receipt {
    fn drop(&mut self) {
        release(self.heap);
        release(self.owner);
    }
}
unsafe extern "system" fn receipt_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { proof_query(this, iid, out, &CAPTURE_GUID) }
}
unsafe extern "system" fn receipt_release(this: *mut c_void) -> u32 {
    let receipt = this as *mut Receipt;
    // SAFETY: a live receipt; freed at zero.
    let left = unsafe { (*receipt).refs.fetch_sub(1, SeqCst) } - 1;
    if left == 0 {
        drop(unsafe { Box::from_raw(receipt) });
    }
    left
}
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: receipt_query, AddRef: proof_add_ref, Release: receipt_release };
impl Receipt {
    fn matches_srv(&self, held: &OwnedTable, offset: u32, binding: &SrvBinding) -> bool {
        let stride = u64::from(self.stride);
        if held.parameter != self.parameter || held.count != self.count || held.identity != self.identity || held.version != 1 || held.source_gpu != self.source_gpu || held.source_heap != self.source_heap || held.owner != self.owner || held.cpu_heap != self.heap || offset >= self.count || stride == 0 || u64::from(offset) > (u64::MAX - self.source_gpu) / stride || binding.stamp.gpu != self.source_gpu + u64::from(offset) * stride || binding.stamp.heap != self.source_heap {
            return false;
        }
        let owner = query(self.owner, &OWNER_GUID);
        if owner.is_null() {
            return false;
        }
        // SAFETY: only owners answer the owner proof's IID.
        let matched = unsafe { &*(owner as *const Owner) }.matches_srv(self.owner, self.heap, self.identity, 1, self.count, offset, binding);
        release(owner);
        matched
    }
}
/// MatchesCapturedSourceSrv.
pub fn matches_captured_srv(table: &OwnedTable, offset: u32, binding: &SrvBinding) -> bool {
    if table.provenance.is_null() || table.owner.is_null() || table.cpu_heap.is_null() || table.identity == 0 || table.version == 0 || table.count == 0 || table.source_gpu == 0 || table.source_heap == 0 {
        return false;
    }
    let receipt = query(table.provenance, &CAPTURE_GUID);
    if receipt.is_null() {
        return false;
    }
    // SAFETY: only receipts answer the capture proof's IID.
    let matched = unsafe { &*(receipt as *const Receipt) }.matches_srv(table, offset, binding);
    release(receipt);
    matched
}

// ---- the cache
/// SourceTableCache::Storage.
pub struct Cache {
    device: ID3D12Device,
    entries: Vec<OwnerRef>,
    resources: u32,
    samplers: u32,
    slot_entries: Vec<OwnerRef>,
    slot_resources: u32,
    slot_samplers: u32,
}

// ---- the captured slots (their owners and resources held)
struct Slots(Vec<CaptureSlot>);
impl Drop for Slots {
    fn drop(&mut self) {
        for slot in &self.0 {
            [slot.resource, slot.counter, slot.origin, slot.extra].into_iter().for_each(release);
        }
    }
}
fn empty_slot(kind: u32, used: bool) -> CaptureSlot {
    // SAFETY: an all-zero slot is its default.
    let mut slot: CaptureSlot = unsafe { core::mem::zeroed() };
    (slot.kind, slot.used) = (kind, used);
    slot
}
fn stamp_of(slot: &CaptureSlot) -> DescriptorStamp {
    DescriptorStamp { gpu: slot.stamp_gpu, heap: slot.stamp_heap, value: slot.stamp_value }
}
struct Captured {
    parameter: u32,
    kind: i32,
    range: RangeIdentity,
    /// The original table extent (the slots' own count when whole).
    count: u32,
    offsets: Option<Vec<u32>>,
    slots: Slots,
}
impl Captured {
    fn offset(&self, n: usize) -> u32 {
        self.offsets.as_ref().map_or(n as u32, |o| o[n])
    }
}

fn same_device(device: &ID3D12Device, object: *mut c_void) -> bool {
    // SAFETY: a live device child (list or root).
    let Some(child) = (unsafe { ID3D12DeviceChild::from_raw_borrowed(&object) }) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.as_ref() == Some(device)
}
fn bindings_match(list: u64, b: &Bindings, requests: &[Request]) -> bool {
    let found = crate::audit_tables::cvr_sda_tables_find(list);
    // SAFETY: the tracked tables, under the lock; the caller's 64 tables.
    let (Some(t), tables) = (unsafe { found.as_ref() }, unsafe { core::slice::from_raw_parts(b.tables, 64) }) else { return false };
    t.root == b.root as u64 && requests.iter().all(|r| r.parameter < 64 && t.mask & (1 << r.parameter) != 0 && t.addresses[r.parameter as usize] == tables[r.parameter as usize])
}
fn range_of(gpu: u64, count: u32) -> RangeIdentity {
    // SAFETY: our output; the store under the lock.
    let mut range: RangeIdentity = unsafe { core::mem::zeroed() };
    unsafe { crate::descriptor_store::cvr_ds_range(core_::store(), gpu, count, &mut range) };
    range
}
/// SourceSlotWalk: the used slots' values, stamps and owners.
fn walk(device: &ID3D12Device, table: &mut Captured) -> bool {
    let n = table.slots.0.len();
    let input: Vec<SlotIn> = (0..n).map(|i| SlotIn { offset: table.offset(i), kind: table.slots.0[i].kind, used: table.slots.0[i].used }).collect();
    // SAFETY: zeroed outputs the walk fills.
    let mut output: Vec<SlotOut> = vec![unsafe { core::mem::zeroed() }; n];
    let mut failure = Failure { reason: 0, offset: 0 };
    if !unsafe { crate::audit_sources::cvr_sda_source_walk(core_::store(), &table.range, device.as_raw(), input.as_ptr(), n, output.as_mut_ptr(), &mut failure) } {
        return error(failure.reason, table.parameter, failure.offset);
    }
    for (i, walked) in output.iter().enumerate() {
        if !input[i].used {
            continue;
        }
        let slot = &mut table.slots.0[i];
        (slot.value, slot.stamp_gpu, slot.stamp_heap, slot.stamp_value) = (walked.value, walked.gpu, walked.heap, walked.value);
        (slot.origin, slot.resource, slot.counter) = (walked.origin, walked.resource, walked.counter);
    }
    true
}
/// SourceSlotRecheck: the slots unchanged since the walk, the CBVs' cookies.
fn recheck(table: &mut Captured) -> bool {
    let n = table.slots.0.len();
    let lent: Vec<RecheckSlot> = (0..n)
        .map(|i| {
            let s = &table.slots.0[i];
            RecheckSlot { value: s.value, resource: s.resource, offset: table.offset(i), kind: s.kind, used: s.used }
        })
        .collect();
    let mut cookies = vec![0u64; n];
    let mut failure = Failure { reason: 0, offset: 0 };
    // SAFETY: our arrays; the store under the lock.
    if !unsafe { crate::audit_source_checks::cvr_sda_source_recheck(core_::store(), &table.range, table.count, lent.as_ptr(), n, cookies.as_mut_ptr(), &mut failure) } {
        return error(failure.reason, table.parameter, failure.offset);
    }
    for (slot, cookie) in table.slots.0.iter_mut().zip(cookies) {
        if slot.used && slot.kind == 1 {
            slot.cookie = cookie;
        }
    }
    true
}
fn range_type(kind: u32) -> i32 {
    match kind {
        1 => D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0,
        2 => D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0,
        3 => D3D12_DESCRIPTOR_RANGE_TYPE_UAV.0,
        _ => D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0,
    }
}

/// What one capture emits on success.
enum Row {
    Table(OwnedTable),
    Slots(OwnedSlotsRow, Vec<OwnedSlot>),
}

/// CaptureSourceTablesAtDraw (`slots_mode` false) and CaptureSourceSlotsAtDraw (true).
#[allow(clippy::too_many_arguments)]
fn capture(
    slots_mode: bool,
    device: *mut c_void,
    list: *mut c_void,
    b: &Bindings,
    requests: &[Request],
    provider: Provider,
    context: *const c_void,
    cache: &mut *mut Cache,
    dependencies: bool,
    rows: &mut Vec<Row>,
    used: &mut Vec<Dependency>,
) -> bool {
    FAILURE.with(|f| f.set([0, u32::MAX, u32::MAX]));
    // SAFETY: the caller's live device and list.
    let (Some(d), Some(l)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return error_at(1, u32::MAX) };
    if !core_::enabled() || b.root.is_null() || requests.len() > 64 || !b.heaps_seen || b.heap_count > 2 || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(d, list) || !same_device(d, b.root) {
        return error_at(1, u32::MAX);
    }
    // SAFETY: a live cache handle (or null).
    if let Some(c) = unsafe { cache.as_ref() } {
        if c.device.as_raw() != device {
            return error_at(10, u32::MAX);
        }
    }
    let list_key = list as u64;
    // SAFETY: the caller's 64 tables.
    let tables = unsafe { core::slice::from_raw_parts(b.tables, 64) };
    let mut captured: Vec<Captured> = Vec::with_capacity(requests.len());
    let mut parameters = BTreeSet::new();
    let mut selected = 0usize;
    {
        let _g = core_::guard();
        core_::drain_retired();
        if core_::overflow() || core_::tracking_failure() != 0 || !bindings_match(list_key, b, requests) {
            return error_at(2, u32::MAX);
        }
        let Some(layout) = core_::root_view(b.root as u64).filter(|l| l.supported && l.flags & FORBIDDEN == 0) else { return error_at(3, u32::MAX) };
        for request in requests {
            let p = request.parameter;
            if p >= 64 || !parameters.insert(p) || b.table_mask & (1 << p) == 0 {
                return error_at(4, p);
            }
            let uses = if request.use_count == 0 { core::ptr::null() } else { request.uses };
            let mut table = if !slots_mode {
                // SourceLayout.
                if request.count == 0 || request.count > 1_000_000 {
                    return error_at(4, p);
                }
                let mut plan = vec![SlotPlan::default(); request.count as usize];
                let mut heap_type = 0u32;
                // SAFETY: the root's layout under the lock; our outputs.
                if !unsafe { crate::source_layout::cvr_sl_layout(layout.ranges, layout.count, layout.parameters, p, request.count, uses, request.use_count, plan.as_mut_ptr(), &mut heap_type) } {
                    return error_at(4, p);
                }
                let slots = plan.iter().map(|s| empty_slot(s.kind, s.used)).collect();
                Captured { parameter: p, kind: heap_type as i32, range: unsafe { core::mem::zeroed() }, count: request.count, offsets: None, slots: Slots(slots) }
            } else {
                // SourceSlotLayout.
                let (mut offsets, mut kinds) = (vec![0u32; SLOT_CAPTURE_LIMIT], vec![0u32; SLOT_CAPTURE_LIMIT]);
                let (mut count, mut heap_type) = (0usize, 0u32);
                // SAFETY: as above.
                if !unsafe { crate::source_layout::cvr_sl_slot_layout(layout.ranges, layout.count, layout.parameters, p, request.count, uses, request.use_count, offsets.as_mut_ptr(), kinds.as_mut_ptr(), &mut count, &mut heap_type) } || count > SLOT_CAPTURE_LIMIT - selected {
                    return error_at(4, p);
                }
                selected += count;
                offsets.truncate(count);
                let slots = kinds[..count].iter().map(|&k| empty_slot(k, true)).collect();
                Captured { parameter: p, kind: heap_type as i32, range: unsafe { core::mem::zeroed() }, count: request.count, offsets: Some(offsets), slots: Slots(slots) }
            };
            table.range = range_of(tables[p as usize], request.count);
            if !table.range.valid || !crate::audit_lifetimes::heap_known(table.range.heap) {
                return error_at(5, p);
            }
            // SAFETY: the caller's bound heaps.
            if !unsafe { crate::audit_source_capture::cvr_sda_bound_heap(device, b.heaps, b.heap_count, table.kind as u32, table.range.heap, table.range.stride) } {
                return error_at(5, p);
            }
            if !walk(d, &mut table) {
                return false;
            }
            captured.push(table);
        }
    }
    // The CBVs backed through the provider, outside the lock.
    for table in &mut captured {
        for n in 0..table.slots.0.len() {
            let offset = table.offset(n);
            let slot = &mut table.slots.0[n];
            if slot.kind != 1 || (!slots_mode && !slot.used) {
                continue;
            }
            let reported = if slots_mode { offset } else { n as u32 };
            let stamp = stamp_of(slot);
            let (mut resource, mut owner) = (core::ptr::null_mut(), core::ptr::null_mut());
            // SAFETY: the .cpp's trampoline.
            match unsafe { provider(context, list, table.parameter, reported, &stamp, slot.origin, &mut resource, &mut owner) } {
                1 => {}
                2 => return error_at(13, u32::MAX),
                _ => return error(8, table.parameter, reported),
            }
            // SAFETY: the provider's resource.
            if !unsafe { crate::audit_source_capture::cvr_sda_backing(device, resource, slot.value[4], slot.value[5]) } {
                release(resource);
                release(owner);
                return error(8, table.parameter, reported);
            }
            release(core::mem::replace(&mut slot.resource, resource));
            release(core::mem::replace(&mut slot.extra, owner));
        }
    }
    {
        let _g = core_::guard();
        core_::drain_retired();
        if !bindings_match(list_key, b, requests) {
            return error_at(9, u32::MAX);
        }
        for table in &mut captured {
            if !recheck(table) {
                return false;
            }
        }
    }
    if cache.is_null() {
        *cache = Box::into_raw(Box::new(Cache { device: d.clone(), entries: Vec::new(), resources: 0, samplers: 0, slot_entries: Vec::new(), slot_resources: 0, slot_samplers: 0 }));
    }
    // SAFETY: the live cache.
    let store = unsafe { &mut **cache };
    let mut pending: Vec<OwnerRef> = Vec::new();
    let (mut resources, mut samplers) = if slots_mode { (store.slot_resources, store.slot_samplers) } else { (store.resources, store.samplers) };
    let mut built: Vec<Row> = Vec::with_capacity(captured.len());
    let mut chosen: Vec<OwnerRef> = Vec::with_capacity(captured.len());
    for table in &captured {
        let mut key: Vec<Value> = Vec::with_capacity(table.slots.0.len());
        let mut leases: Vec<*mut c_void> = Vec::new();
        let mut held = BTreeSet::new();
        let mut resource_map: BTreeMap<u64, usize> = BTreeMap::new();
        let mut lease = |p: *mut c_void, leases: &mut Vec<*mut c_void>| {
            if !p.is_null() && held.insert(p as usize) {
                leases.push(add_ref(p));
            }
        };
        let mut conflict = false;
        for slot in &table.slots.0 {
            let mut value: Value = [0; 20];
            if slots_mode || slot.used {
                value = slot.value;
                for p in [slot.resource, slot.counter, slot.origin, slot.extra] {
                    lease(p, &mut leases);
                }
                if slot.kind == 1 {
                    (value[2], value[6], value[7]) = (slot.cookie, slot.extra as u64, slot.origin as u64);
                }
                if !slots_mode {
                    for (cookie, resource) in [(value[2], slot.resource), (if slot.kind == 3 { value[3] } else { 0 }, slot.counter)] {
                        if cookie != 0 && !resource.is_null() {
                            match resource_map.get(&cookie) {
                                None => {
                                    resource_map.insert(cookie, add_ref(resource) as usize);
                                }
                                Some(&r) if r != resource as usize => conflict = true,
                                _ => {}
                            }
                        }
                    }
                }
            } else {
                (value[0], value[1]) = (if slot.kind != 0 { slot.kind.into() } else { 2 }, u64::MAX);
            }
            key.push(value);
        }
        let entries = if slots_mode { &store.slot_entries } else { &store.entries };
        let mut owner = entries.iter().chain(pending.iter()).find(|e| e.get().kind == table.kind && e.get().key == key).cloned();
        let same_resources = |o: &OwnerRef| o.get().resources == resource_map;
        if conflict || owner.as_ref().is_some_and(|o| !slots_mode && !same_resources(o)) {
            leases.iter().for_each(|&l| release(l));
            resource_map.values().for_each(|&r| release(r as *mut c_void));
            return error_at(7, table.parameter);
        }
        if owner.is_some() {
            leases.iter().for_each(|&l| release(l));
            resource_map.values().for_each(|&r| release(r as *mut c_void));
        } else {
            let sampler = table.kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER.0;
            let (count, limit) = if sampler { (&mut samplers, 2048u32) } else { (&mut resources, 1_000_000u32) };
            if key.len() > (limit - *count) as usize {
                leases.iter().for_each(|&l| release(l));
                resource_map.values().for_each(|&r| release(r as *mut c_void));
                return error_at(12, table.parameter);
            }
            *count += key.len() as u32;
            let identity = NEXT_IDENTITY.fetch_add(1, Relaxed);
            let created = Owner::create(key, leases, resource_map, table.kind, identity);
            if identity == 0 {
                return error_at(12, table.parameter);
            }
            let mut heap = core::ptr::null_mut();
            // SAFETY: the captured slots; our output.
            if !unsafe { crate::audit_source_capture::cvr_sda_owner_heap(device, table.kind as u32, table.slots.0.as_ptr(), table.slots.0.len(), &mut heap) } {
                return error_at(11, table.parameter);
            }
            // SAFETY: the new owner is ours alone until published.
            unsafe { (*created.0).heap = heap };
            pending.push(created.clone());
            owner = Some(created);
        }
        chosen.push(owner.expect("found or created"));
    }
    // Every table interned: the rows, each table's receipt holding its owner and CPU heap.
    for (table, owner) in captured.iter().zip(&chosen) {
        let o = owner.get();
        if !slots_mode {
            let receipt = Box::into_raw(Box::new(Receipt {
                vtbl: &RECEIPT_VTBL,
                refs: AtomicU32::new(1),
                parameter: table.parameter,
                count: table.slots.0.len() as u32,
                heap: add_ref(o.heap),
                owner: add_ref(owner.raw()),
                identity: o.identity,
                source_gpu: table.range.gpu,
                source_heap: table.range.heap,
                stride: table.range.stride,
            }));
            built.push(Row::Table(OwnedTable {
                parameter: table.parameter,
                count: table.slots.0.len() as u32,
                cpu_heap: o.heap,
                owner: owner.raw(),
                identity: o.identity,
                version: 1,
                source_gpu: table.range.gpu,
                source_heap: table.range.heap,
                provenance: receipt.cast(),
            }));
        } else {
            let slots: Vec<OwnedSlot> = table.slots.0.iter().enumerate().map(|(n, s)| OwnedSlot { offset: table.offset(n), compact_index: n as u32, kind: range_type(s.kind), stamp: stamp_of(s) }).collect();
            built.push(Row::Slots(
                OwnedSlotsRow { parameter: table.parameter, count: table.count, cpu_heap: o.heap, owner: owner.raw(), identity: o.identity, version: 1, source_gpu: table.range.gpu, source_heap: table.range.heap, slots: core::ptr::null(), slot_count: 0 },
                slots,
            ));
        }
    }
    if dependencies {
        for table in &captured {
            for (n, slot) in table.slots.0.iter().enumerate() {
                if (slots_mode || slot.used) && (slot.kind == 2 || slot.kind == 3) {
                    used.push(Dependency {
                        parameter: table.parameter,
                        offset: table.offset(n),
                        kind: if slot.kind == 2 { D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 } else { D3D12_DESCRIPTOR_RANGE_TYPE_UAV.0 },
                        stamp: stamp_of(slot),
                        resource: slot.resource,
                        counter: slot.counter,
                    });
                }
            }
        }
    }
    // Committed: the new owners cached, the rows' references taken.
    for row in &mut built {
        match row {
            Row::Table(t) => {
                add_ref(t.cpu_heap);
                add_ref(t.owner);
            }
            Row::Slots(r, _) => {
                add_ref(r.cpu_heap);
                add_ref(r.owner);
            }
        }
    }
    used.iter().for_each(|d| {
        add_ref(d.resource);
        add_ref(d.counter);
    });
    if slots_mode {
        store.slot_entries.extend(pending);
        (store.slot_resources, store.slot_samplers) = (resources, samplers);
    } else {
        store.entries.extend(pending);
        (store.resources, store.samplers) = (resources, samplers);
    }
    *rows = built;
    true
}

// ---- the stamps matched now
/// MatchesSourceSlot.
pub fn matches_slot(slot: &OwnedSlot) -> bool {
    let expected = &slot.stamp;
    let kind = crate::source_layout::cvr_sl_kind(slot.kind as u32);
    // SAFETY: our value.
    if !core_::observe_bindings() || expected.gpu == 0 || expected.heap == 0 || kind == 0 || !unsafe { crate::source_layout::cvr_sl_descriptor(1, &expected.value, core::ptr::null(), kind) } {
        return false;
    }
    let _g = core_::guard();
    core_::drain_retired();
    if core_::overflow() || core_::tracking_failure() != 0 {
        return false;
    }
    // SAFETY: the store under the lock.
    unsafe { crate::audit_source_checks::cvr_sda_stamp_current(core_::store(), expected.gpu, expected.heap, &expected.value, 0) }
}
/// MatchesCpuSourceSrv.
pub fn matches_cpu_srv(cpu: u64, expected: &DescriptorStamp) -> bool {
    // SAFETY: our value.
    if !core_::observe_bindings() || cpu == 0 || expected.gpu == 0 || expected.heap == 0 || expected.value[0] != 2 || !unsafe { crate::source_layout::cvr_sl_descriptor(1, &expected.value, core::ptr::null(), 2) } {
        return false;
    }
    let _g = core_::guard();
    core_::drain_retired();
    if core_::overflow() || core_::tracking_failure() != 0 {
        return false;
    }
    // SAFETY: the store under the lock.
    unsafe { crate::audit_source_checks::cvr_sda_stamp_current(core_::store(), expected.gpu, expected.heap, &expected.value, cpu) }
}

// ---- the C ABI
/// # Safety
/// The device call's objects; `requests` holds `request_count`; `cache` is the cache's handle slot; `emit` takes
/// the rows on success.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_st_capture(
    slots_mode: bool,
    device: *mut c_void,
    list: *mut c_void,
    bindings: *const Bindings,
    requests: *const Request,
    request_count: usize,
    provider: Provider,
    context: *const c_void,
    cache: *mut *mut Cache,
    dependencies: bool,
    emit: Emit,
    emit_context: *mut c_void,
) -> bool {
    let (Some(b), Some(cache)) = (unsafe { bindings.as_ref() }, unsafe { cache.as_mut() }) else { return error_at(1, u32::MAX) };
    if b.tables.is_null() || b.heaps.is_null() || (request_count != 0 && requests.is_null()) {
        return error_at(1, u32::MAX);
    }
    let requests = if request_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(requests, request_count) } };
    let (mut rows, mut used) = (Vec::new(), Vec::new());
    if !capture(slots_mode, device, list, b, requests, provider, context, cache, dependencies, &mut rows, &mut used) {
        return false;
    }
    for row in &mut rows {
        match row {
            Row::Table(t) => unsafe { emit(emit_context, 0, (t as *const OwnedTable).cast()) },
            Row::Slots(r, slots) => {
                (r.slots, r.slot_count) = (slots.as_ptr(), slots.len());
                unsafe { emit(emit_context, 1, (r as *const OwnedSlotsRow).cast()) }
            }
        }
    }
    for d in &used {
        unsafe { emit(emit_context, 2, (d as *const Dependency).cast()) };
    }
    true
}
/// # Safety
/// A cache's handle (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_cache_free(cache: *mut Cache) {
    if !cache.is_null() {
        drop(unsafe { Box::from_raw(cache) });
    }
}
/// 0 tables, 1 descriptors, 2 slot tables, 3 slot descriptors.
///
/// # Safety
/// A cache's handle (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_cache_count(cache: *const Cache, which: u32) -> u32 {
    let Some(c) = (unsafe { cache.as_ref() }) else { return 0 };
    match which {
        0 => c.entries.len() as u32,
        1 => c.resources + c.samplers,
        2 => c.slot_entries.len() as u32,
        _ => c.slot_resources + c.slot_samplers,
    }
}
/// The thread's source failure (reason, parameter, offset); `set` sets it first.
///
/// # Safety
/// `out` is writable (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_failure(set: *const [u32; 3], out: *mut [u32; 3]) {
    if let Some(value) = unsafe { set.as_ref() } {
        FAILURE.with(|f| f.set(*value));
    }
    if let Some(out) = unsafe { out.as_mut() } {
        *out = FAILURE.with(Cell::get);
    }
}
/// # Safety
/// The caller's table and binding.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_matches(table: *const OwnedTable, offset: u32, binding: *const SrvBinding) -> bool {
    match (unsafe { table.as_ref() }, unsafe { binding.as_ref() }) {
        (Some(t), Some(b)) => matches_captured_srv(t, offset, b),
        _ => false,
    }
}
/// # Safety
/// The caller's slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_matches_slot(slot: *const OwnedSlot) -> bool {
    unsafe { slot.as_ref() }.is_some_and(matches_slot)
}
/// # Safety
/// The caller's stamp.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_st_matches_cpu(cpu: u64, expected: *const DescriptorStamp) -> bool {
    unsafe { expected.as_ref() }.is_some_and(|e| matches_cpu_srv(cpu, e))
}

#[cfg(test)]
mod tests {
    //! The proofs' identities and counts, the receipt's refusals, the failure's thread-local and the gated captures.
    use super::*;

    #[test]
    fn proofs_failures_and_gates() {
        // An owner answers its own proof and IUnknown, never the receipt's; the last release frees it.
        let owner = Owner::create(vec![[0; 20]], Vec::new(), BTreeMap::new(), D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0, 9);
        let raw = owner.raw();
        assert_eq!(query(raw, &CAPTURE_GUID), core::ptr::null_mut());
        let again = query(raw, &OWNER_GUID);
        assert_eq!(again, raw);
        assert_eq!(owner.get().refs.load(SeqCst), 2);
        release(again);
        // A receipt refuses a table it did not capture and an SRV off its slots before asking the owner.
        let receipt = Box::into_raw(Box::new(Receipt { vtbl: &RECEIPT_VTBL, refs: AtomicU32::new(1), parameter: 3, count: 1, heap: core::ptr::null_mut(), owner: add_ref(raw), identity: 9, source_gpu: 0x1000, source_heap: 7, stride: 32 }));
        let table = OwnedTable { parameter: 3, count: 1, cpu_heap: 0x10 as *mut c_void, owner: raw, identity: 9, version: 1, source_gpu: 0x1000, source_heap: 7, provenance: receipt.cast() };
        // SAFETY: zeroed is a valid binding.
        let mut binding: SrvBinding = unsafe { core::mem::zeroed() };
        (binding.stamp.gpu, binding.stamp.heap) = (0x1000, 7);
        assert!(!matches_captured_srv(&table, 0, &binding)); // the receipt held no CPU heap
        assert!(!matches_captured_srv(&OwnedTable { provenance: core::ptr::null_mut(), ..table }, 0, &binding));
        assert!(!matches_captured_srv(&table, 1, &binding));
        assert_eq!(owner.get().refs.load(SeqCst), 2); // the receipt's reference
        release(receipt.cast());
        assert_eq!(owner.get().refs.load(SeqCst), 1);
        drop(owner);
        // The thread's failure, and the capture refused before any object without observed bindings.
        let set = [12u32, 3, 4];
        let mut out = [0u32; 3];
        unsafe { cvr_sda_st_failure(&set, &mut out) };
        assert_eq!(out, set);
        assert!(error_at(5, 2) == false && FAILURE.with(Cell::get) == [5, 2, u32::MAX]);
        let tables = [0u64; 64];
        let heaps = [core::ptr::null_mut::<c_void>(); 2];
        let b = Bindings { root: core::ptr::null_mut(), tables: tables.as_ptr(), table_mask: 0, heaps: heaps.as_ptr(), heap_count: 0, heaps_seen: true };
        unsafe extern "C-unwind" fn never(_: *const c_void, _: *mut c_void, _: u32, _: u32, _: *const DescriptorStamp, _: *mut c_void, _: *mut *mut c_void, _: *mut *mut c_void) -> u32 {
            unreachable!()
        }
        unsafe extern "C-unwind" fn no_rows(_: *mut c_void, _: u32, _: *const c_void) {
            unreachable!()
        }
        let mut cache: *mut Cache = core::ptr::null_mut();
        assert!(!unsafe { cvr_sda_st_capture(false, core::ptr::null_mut(), core::ptr::null_mut(), &b, core::ptr::null(), 0, never, core::ptr::null(), &mut cache, false, no_rows, core::ptr::null_mut()) });
        assert!(cache.is_null() && FAILURE.with(Cell::get) == [1, u32::MAX, u32::MAX]);
        assert_eq!(unsafe { cvr_sda_st_cache_count(core::ptr::null(), 1) }, 0);
        // SAFETY: zeroed slot and stamp.
        let slot: OwnedSlot = unsafe { core::mem::zeroed() };
        assert!(!matches_slot(&slot) && !matches_cpu_srv(0x40, &slot.stamp));
    }
}
