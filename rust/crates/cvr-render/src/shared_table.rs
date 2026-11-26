//! The assembled source binding tables (formerly StereoBindingBank.hpp's SharedBankTable as SourceBindingAssembly.cpp
//! issued it) and their SRV coverage receipts (OwnedSrvCoverage, IssueCoverage, ReissueCoverage, CoverageShape,
//! DescribeOwnedSrvCoverage, MatchesOwnedSrvCoverage); AssemblePass over Rust plans (assembly_pass.rs with this side's
//! issue of each assembled table). A table holds its CPU heap and every lease; a receipt can only be issued here for
//! the exact immutable CPU table it was built from, and copied numeric labels or another heap cannot reuse it.

use core::ffi::c_void;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use windows::core::{IUnknown, Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use crate::assembly_build::{failure, CoverageRow, Plan, Slot, Stamp, Table as PlanTable};
use crate::geometry_pass_scope::Scope;

/// CpuTableCache::DefaultDescriptorLimit.
pub const DEFAULT_DESCRIPTOR_LIMIT: u32 = 1_000_000;

/// A table's shape a receipt is bound to (its CPU heap and leases held).
#[derive(Clone)]
pub struct Shape {
    pub parameter: u32,
    pub offset: u32,
    pub count: u32,
    pub cpu_heap: Option<ID3D12DescriptorHeap>,
    pub leases: Vec<IUnknown>,
    pub identity: u64,
    pub version: u64,
    pub storage_scope: u64,
    pub storage_extent: u32,
}
/// OwnedSrvCoverage: the source it was issued for, the exact table and the covered SRV slots.
pub struct Coverage {
    pub source: Scope,
    pub table: Shape,
    pub slots: Arc<Vec<CoverageRow>>,
}
/// SharedBankTable.
#[derive(Clone)]
pub struct SharedTable {
    pub shape: Shape,
    pub coverage: Option<Arc<Coverage>>,
}
impl SharedTable {
    pub fn parameter(&self) -> u32 {
        self.shape.parameter
    }
    pub fn heap(&self) -> *mut c_void {
        self.shape.cpu_heap.as_ref().map_or(core::ptr::null_mut(), |h| h.as_raw())
    }
    /// The table as the binding bank and the prepared pair read it (its leases lent from the table).
    pub fn lend(&self) -> crate::binding_bank::Table {
        crate::binding_bank::Table {
            parameter: self.shape.parameter,
            offset: self.shape.offset,
            count: self.shape.count,
            cpu_heap: self.heap(),
            leases: self.shape.leases.as_ptr().cast(),
            lease_count: self.shape.leases.len(),
            identity: self.shape.identity,
            version: self.shape.version,
            storage_scope: self.shape.storage_scope,
            storage_extent: self.shape.storage_extent,
        }
    }
}

fn valid_scope(s: &Scope) -> bool {
    s.root != 0 && s.list != 0 && s.generation != 0 && s.occurrence != 0 && s.source != 0 && s.eye < 2
}
fn same_scope(a: &Scope, b: &Scope) -> bool {
    a.root == b.root && a.list == b.list && a.generation == b.generation && a.occurrence == b.occurrence && a.source == b.source && a.eye == b.eye
}
fn same_stamp(a: &Stamp, b: &Stamp) -> bool {
    a.gpu == b.gpu && a.heap == b.heap && a.value == b.value
}
fn device_of(child: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child; a local output.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}
fn borrowed<T: Interface>(address: u64) -> Option<T> {
    let raw = address as *mut c_void;
    // SAFETY: a live object the caller's tables hold.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
fn lease_set(leases: &[IUnknown]) -> BTreeSet<usize> {
    leases.iter().map(|l| l.as_raw() as usize).collect()
}
/// CoverageShape: `table` is `expected`'s exact immutable CPU table (heap, labels, extent; every expected lease held).
fn coverage_shape(table: &Shape, expected: &Shape) -> bool {
    let Some(heap) = table.cpu_heap.as_ref() else { return false };
    if expected.cpu_heap.as_ref().map(|h| h.as_raw()) != Some(heap.as_raw())
        || table.parameter != expected.parameter
        || table.offset != expected.offset
        || table.count != expected.count
        || table.identity != expected.identity
        || table.version != expected.version
        || table.storage_scope != expected.storage_scope
        || table.storage_extent != expected.storage_extent
        || table.count == 0
        || table.storage_scope == 0
        || table.count > table.storage_extent
    {
        return false;
    }
    // SAFETY: a live heap.
    let desc = unsafe { heap.GetDesc() };
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE || table.offset > desc.NumDescriptors || table.storage_extent > desc.NumDescriptors - table.offset {
        return false;
    }
    let owners = lease_set(&table.leases);
    expected.leases.iter().all(|l| owners.contains(&(l.as_raw() as usize)))
}
/// IssueCoverage: the receipt of the table's covered SRV slots (none when `slots` is empty), each slot's original
/// descriptor provenance checked against the table's leases and the copies emitted into it.
fn issue_coverage(table: &mut SharedTable, source: &Scope, slots: &[CoverageRow], emitted: &[Slot]) -> bool {
    table.coverage = None;
    if slots.is_empty() {
        return true;
    }
    if !valid_scope(source) || !coverage_shape(&table.shape, &table.shape) || emitted.is_empty() {
        return false;
    }
    let Some(heap) = table.shape.cpu_heap.as_ref() else { return false };
    let Some(device) = heap.cast::<ID3D12DeviceChild>().ok().and_then(|c| device_of(&c)) else { return false };
    // SAFETY: a live device.
    let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
    if stride == 0 {
        return false;
    }
    let owners = lease_set(&table.shape.leases);
    let mut copied: BTreeMap<u32, &Slot> = BTreeMap::new();
    for slot in emitted.iter().filter(|s| s.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0) {
        match copied.get(&slot.offset) {
            Some(found) if !same_stamp(&found.stamp, &slot.stamp) => return false,
            Some(_) => {}
            None => {
                copied.insert(slot.offset, slot);
            }
        }
    }
    for slot in slots {
        let decoded = crate::descriptor_views::decode_srv(&slot.stamp.value, None);
        let origin = borrowed::<ID3D12DescriptorHeap>(slot.original_cpu_heap);
        let origin_device = origin.as_ref().and_then(|h| h.cast::<ID3D12DeviceChild>().ok()).and_then(|c| device_of(&c));
        if !valid_scope(&slot.contributor)
            || slot.contributor.root != source.root
            || slot.original_owner == 0
            || slot.original_cpu_heap == 0
            || slot.original_identity == 0
            || slot.original_version == 0
            || slot.original_gpu == 0
            || slot.original_heap == 0
            || slot.offset >= slot.original_count
            || slot.offset >= table.shape.storage_extent
            || !owners.contains(&(slot.original_owner as usize))
            || !owners.contains(&(slot.original_cpu_heap as usize))
            || origin_device.as_ref().map(|d| d.as_raw()) != Some(device.as_raw())
            || u64::from(slot.offset) > (u64::MAX - slot.original_gpu) / u64::from(stride)
            || slot.stamp.gpu != slot.original_gpu + u64::from(slot.offset) * u64::from(stride)
            || slot.stamp.heap != slot.original_heap
            || decoded.is_none_or(|d| d.resource_cookie != slot.resource_cookie)
        {
            return false;
        }
        let Some(origin) = origin else { return false };
        // SAFETY: a live heap.
        let desc = unsafe { origin.GetDesc() };
        if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE || slot.compact_index >= desc.NumDescriptors {
            return false;
        }
        if copied.get(&slot.offset).is_none_or(|m| !same_stamp(&m.stamp, &slot.stamp)) {
            return false;
        }
    }
    table.coverage = Some(Arc::new(Coverage { source: *source, table: table.shape.clone(), slots: Arc::new(slots.to_vec()) }));
    true
}
/// ReissueCoverage: a plan table's receipt carried over from the physical table it shares (its own count).
fn reissue_coverage(table: &mut SharedTable, source: &Scope, physical: &SharedTable) -> bool {
    table.coverage = None;
    let Some(prior) = physical.coverage.as_ref() else { return true };
    if !valid_scope(source) || source.root != prior.source.root || !coverage_shape(&physical.shape, &prior.table) {
        return false;
    }
    let mut expected = prior.table.clone();
    expected.count = table.shape.count;
    if !coverage_shape(&table.shape, &expected) {
        return false;
    }
    table.coverage = Some(Arc::new(Coverage { source: *source, table: table.shape.clone(), slots: prior.slots.clone() }));
    true
}
/// DescribeOwnedSrvCoverage: the table's receipt for exactly this source, its slots non-empty, the table its own.
pub fn describe(table: &SharedTable, source: &Scope) -> Option<Arc<Coverage>> {
    let receipt = table.coverage.as_ref()?;
    (valid_scope(source) && same_scope(source, &receipt.source) && !receipt.slots.is_empty() && coverage_shape(&table.shape, &receipt.table)).then(|| receipt.clone())
}
/// MatchesOwnedSrvCoverage: the receipt covers this slot's exact descriptor.
pub fn matches(table: &SharedTable, source: &Scope, offset: u32, stamp: &Stamp) -> bool {
    let Some(receipt) = describe(table, source) else { return false };
    let Some(decoded) = crate::descriptor_views::decode_srv(&stamp.value, None) else { return false };
    receipt.slots.iter().any(|s| s.offset == offset && s.resource_cookie == decoded.resource_cookie && same_stamp(&s.stamp, stamp))
}

// ---- the pass

/// The issue of one assembled table (IssueTable): its heap and leases held, its SRV coverage receipt when it holds
/// sparse SRV slots only.
unsafe extern "C-unwind" fn issue(user: *mut c_void, table: *const PlanTable, heap: *mut c_void, scope: u64, extent: u32, keep: bool) -> bool {
    // SAFETY: the pass's combined table and this call's output.
    let (t, out) = unsafe { (&*table, &mut *user.cast::<Vec<SharedTable>>()) };
    // SAFETY: the pass's new heap (the call's reference: cloned here).
    let cpu_heap = unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&heap) }.cloned();
    let mut value = SharedTable {
        shape: Shape {
            parameter: t.parameter,
            offset: 0,
            count: t.count,
            cpu_heap,
            leases: t.leases.iter().filter_map(|&l| borrowed::<IUnknown>(l)).collect(),
            identity: 0,
            version: 0,
            storage_scope: scope,
            storage_extent: extent,
        },
        coverage: None,
    };
    if t.cbvs.is_empty() && t.srvs.is_empty() && !issue_coverage(&mut value, &t.source_scope, &t.coverage, &t.sparse_slots) {
        return false;
    }
    if keep {
        out.push(value);
    }
    true
}
/// AssemblePass over Rust plans: every plan's tables (in order) from one pass of `cache`; the cache committed only once
/// every table is issued. Err: the HRESULT and the failure (source_binding_assembly::Failure).
pub fn assemble_pass(device: &ID3D12Device, plans: &[&Plan], cache: &mut Option<crate::assembly_pass::Cache>) -> Result<Vec<Vec<SharedTable>>, (HRESULT, u32)> {
    let mut assembled: Vec<SharedTable> = Vec::new();
    let mut reason = failure::ARGUMENTS;
    let lent: Vec<Option<&Plan>> = plans.iter().map(|p| Some(*p)).collect();
    let pass = {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the plans' heaps and resources are live; the issue fills `assembled`.
        unsafe { crate::assembly_pass::assemble_pass(device, &lent, cache, DEFAULT_DESCRIPTOR_LIMIT, issue, (&raw mut assembled).cast(), &mut reason) }
    };
    let mut pass = pass.map_err(|hr| (hr, reason))?;
    let mut result = Vec::with_capacity(plans.len());
    for (p, plan) in plans.iter().enumerate() {
        let mut current = Vec::with_capacity(plan.tables.len());
        for (t, original) in plan.tables.iter().enumerate() {
            let (index, united) = pass.references[p][t];
            let Some(physical) = assembled.get(index) else { return Err((E_FAIL, failure::SRV_OWNER)) };
            let mut value = SharedTable {
                shape: Shape {
                    parameter: original.parameter,
                    offset: 0,
                    count: original.count,
                    cpu_heap: physical.shape.cpu_heap.clone(),
                    leases: original.leases.iter().filter_map(|&l| borrowed::<IUnknown>(l)).collect(),
                    identity: 0,
                    version: 0,
                    storage_scope: physical.shape.storage_scope,
                    storage_extent: physical.shape.storage_extent,
                },
                coverage: None,
            };
            if united {
                for &owner in &pass.combined.tables[index].descriptor_owners {
                    if !value.shape.leases.iter().any(|l| l.as_raw() as u64 == owner) {
                        if let Some(o) = borrowed::<IUnknown>(owner) {
                            value.shape.leases.push(o);
                        }
                    }
                }
            }
            if !reissue_coverage(&mut value, &plan.scope, physical) {
                return Err((E_INVALIDARG, failure::SRV_OWNER));
            }
            current.push(value);
        }
        result.push(current);
    }
    if let Some(staged) = pass.staged.as_mut() {
        staged.hits += pass.followers;
    }
    *cache = pass.staged.take();
    Ok(result)
}
/// The binding bank's source pass (StereoBindingBank::CreateSourcePass) over the assembled sources' roots, serialized
/// roots and tables: the bank's storage, or the HRESULT and the refusal.
pub fn create_source_pass(device: &ID3D12Device, sources: &[(*mut c_void, &[u8], &[SharedTable])]) -> (Result<IUnknown, HRESULT>, crate::bank_layout::SourcePassFailure) {
    let tables: Vec<Vec<crate::binding_bank::Table>> = sources.iter().map(|(_, _, t)| t.iter().map(SharedTable::lend).collect()).collect();
    let lent: Vec<crate::binding_bank::SourceInput> = sources
        .iter()
        .zip(&tables)
        .map(|((root, serialized, _), t)| crate::binding_bank::SourceInput { root: *root, serialized: serialized.as_ptr(), serialized_len: serialized.len(), tables: t.as_ptr(), table_count: t.len() })
        .collect();
    let mut rejected = crate::bank_layout::SourcePassFailure::default();
    let mut out = core::ptr::null_mut();
    // SAFETY: the live device and the lent inputs (their tables' heaps and leases held by `sources`).
    let hr = unsafe { crate::binding_bank::cvr_bank_create_source_pass(device.as_raw(), lent.as_ptr(), lent.len(), &mut rejected, &mut out) };
    if hr.is_err() || out.is_null() {
        return (Err(if hr.is_err() { hr } else { E_FAIL }), rejected);
    }
    // SAFETY: the new storage's one reference.
    (Ok(unsafe { IUnknown::from_raw(out) }), rejected)
}
/// The bank's draw and descriptor counts.
pub fn bank_counts(bank: &IUnknown) -> [u64; 2] {
    let mut counts = crate::binding_bank::Counts { draws: 0, descriptors: 0, snapshots: 0, constant_bytes: 0 };
    // SAFETY: the bank's live storage; a local output.
    unsafe { crate::binding_bank::cvr_bank_counts(bank.as_raw(), &mut counts) };
    [u64::from(counts.draws), u64::from(counts.descriptors)]
}
#[allow(dead_code)]
const _OK: HRESULT = S_OK;
