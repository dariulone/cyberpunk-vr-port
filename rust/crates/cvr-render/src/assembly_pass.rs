//! The source binding assembly's tables (src/Render/SourceBindingAssembly.cpp, `Assemble`, `CpuTableCache` and
//! `AssemblePass`): a ready plan's tables become NONE-flag CPU heaps of their kind and count -- every slot first a null
//! view of its range type, then the captured copies, the private CBV and SRV views -- on the plan's device (every copy
//! heap and resource lease on it); the cache keeps one heap per identical table (a FNV-1a key over the source root,
//! shape, copies, views and descriptor owners, a union's sparse slots), within a descriptor budget, counting hits and
//! misses; a pass first unites the eligible sparse SRV tables of its plans that agree on their source and every common
//! slot (the lowest copy source kept per slot, every contributor's owners held), assembles the combined plan through a
//! staged copy of the cache, and commits that copy only once every plan's tables are issued.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the three forward here with the Rust plans (assembly_build.rs); each assembled
//! table is handed back to the C++ half (`Issue`), which builds its SharedBankTable and SRV coverage receipt; the
//! source_binding_assembly_test GPU cases run against it in its Rust variant, and the unit tests below cover the
//! cache key and the pass's union.

use std::collections::BTreeMap;
use std::sync::Arc;

use windows::core::{Interface, IUnknown, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::assembly_build::failure::*;
use crate::assembly_build::{borrowed, valid_scope, CopyRow, Plan, Slot, Table};
use crate::assembly_layout::same_srv;

/// The C++ half's issue of one assembled table: `table` (a Rust table) on `heap` with its storage scope and extent;
/// `keep` puts it in the call's output, else it only checks its coverage. False when its SRV coverage is refused (a C++
/// exception unwinds through to the C++ caller).
pub type Issue = unsafe extern "C-unwind" fn(user: *mut core::ffi::c_void, table: *const Table, heap: *mut core::ffi::c_void, scope: u64, extent: u32, keep: bool) -> bool;

const DEFAULT_LIMIT: u32 = 1_000_000;

fn same_device(device: &ID3D12Device, child: u64) -> bool {
    let Some(child) = borrowed::<ID3D12DeviceChild>(child) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|d| d.as_raw() == device.as_raw())
}
/// Every copy heap on the device, and every lease that is a resource.
fn table_devices(device: &ID3D12Device, table: &Table) -> bool {
    table.copies.iter().all(|c| same_device(device, c.heap))
        && table.leases.iter().all(|&lease| {
            borrowed::<IUnknown>(lease).and_then(|l| l.cast::<ID3D12Resource>().ok()).is_none_or(|r| same_device(device, r.as_raw() as u64))
        })
}
fn handle(base: usize, index: u32, stride: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base + index as usize * stride as usize }
}
/// NullSlot.
///
/// # Safety
/// `target` is a descriptor of the device's heap.
unsafe fn null_slot(device: &ID3D12Device, kind: i32, target: D3D12_CPU_DESCRIPTOR_HANDLE) {
    // SAFETY: as documented.
    unsafe {
        match D3D12_DESCRIPTOR_RANGE_TYPE(kind) {
            D3D12_DESCRIPTOR_RANGE_TYPE_CBV => device.CreateConstantBufferView(None, target),
            D3D12_DESCRIPTOR_RANGE_TYPE_UAV => {
                let mut view = D3D12_UNORDERED_ACCESS_VIEW_DESC { Format: DXGI_FORMAT_R32_UINT, ViewDimension: D3D12_UAV_DIMENSION_BUFFER, ..Default::default() };
                view.Anonymous.Buffer.NumElements = 1;
                device.CreateUnorderedAccessView(None::<&ID3D12Resource>, None::<&ID3D12Resource>, Some(&view), target);
            }
            D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER => {
                let view = D3D12_SAMPLER_DESC {
                    Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
                    AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                    AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                    AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                };
                device.CreateSampler(&view, target);
            }
            _ => {
                let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_FLOAT,
                    ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    ..Default::default()
                };
                view.Anonymous.Buffer.NumElements = 1;
                device.CreateShaderResourceView(None::<&ID3D12Resource>, Some(&view), target);
            }
        }
    }
}

/// Creates one table's heap and fills it.
///
/// # Safety
/// The table's heaps and resources are live.
unsafe fn create(device: &ID3D12Device, table: &Table, failure: &mut u32) -> Result<ID3D12DescriptorHeap, HRESULT> {
    // SAFETY: a live device.
    let stride = unsafe { device.GetDescriptorHandleIncrementSize(table.kind) };
    *failure = DEVICE;
    if stride == 0 || !table_devices(device, table) {
        return Err(E_INVALIDARG);
    }
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: table.kind, NumDescriptors: table.count, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
    // SAFETY: a live device.
    let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|e| e.code())?;
    // SAFETY: the new heap.
    let base = unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr;
    *failure = EXTENT;
    if base == 0 || (table.count as u64).wrapping_sub(1) > ((usize::MAX - base) / stride as usize) as u64 {
        return Err(E_INVALIDARG);
    }
    // SAFETY: descriptors of the new heap; the views' resources are the table's live leases.
    unsafe {
        for n in 0..table.count {
            null_slot(device, table.slot_types[n as usize], handle(base, n, stride));
        }
        for copy in &table.copies {
            let Some(source) = borrowed::<ID3D12DescriptorHeap>(copy.heap) else { return Err(E_INVALIDARG) };
            let start = source.GetCPUDescriptorHandleForHeapStart().ptr;
            if start == 0 || (copy.source_offset as u64).wrapping_add(copy.count as u64).wrapping_sub(1) > ((usize::MAX - start) / stride as usize) as u64 {
                return Err(E_INVALIDARG);
            }
            device.CopyDescriptorsSimple(copy.count, handle(base, copy.offset, stride), handle(start, copy.source_offset, stride), table.kind);
        }
        for cbv in &table.cbvs {
            device.CreateConstantBufferView(Some(&cbv.view), handle(base, cbv.offset, stride));
        }
        for srv in &table.srvs {
            let resource = borrowed::<ID3D12Resource>(srv.resource);
            device.CreateShaderResourceView(resource.as_ref(), Some(&srv.view), handle(base, srv.offset, stride));
        }
    }
    Ok(heap)
}
fn ready(plan: Option<&Plan>) -> Option<&Plan> {
    plan.filter(|p| p.ready && valid_scope(&p.scope) && !p.tables.is_empty())
}

/// Assemble (without a cache): each table's heap, issued in order.
///
/// # Safety
/// The plan's heaps and resources are live; `issue` and `user` are the C++ half's.
pub unsafe fn assemble(device: &ID3D12Device, plan: Option<&Plan>, issue: Issue, user: *mut core::ffi::c_void, failure: &mut u32) -> HRESULT {
    *failure = NOT_READY;
    let Some(plan) = ready(plan) else { return E_INVALIDARG };
    for table in &plan.tables {
        // SAFETY: as documented.
        let heap = match unsafe { create(device, table, failure) } {
            Ok(h) => h,
            Err(hr) => return hr,
        };
        *failure = SRV_OWNER;
        // SAFETY: the C++ half's issue of a live table and heap.
        if !unsafe { issue(user, table, heap.as_raw(), heap.as_raw() as u64, table.count, true) } {
            return E_INVALIDARG;
        }
    }
    *failure = NONE;
    S_OK
}

/// A cached heap and the table it was assembled from (its leases dropped).
pub struct Entry {
    pub key: Table,
    pub heap: ID3D12DescriptorHeap,
    pub hash: u64,
    /// The key's objects held as the C++ key's references held them (its copy heaps, SRV resources, coverage owners and
    /// heaps, descriptor owners), so a cached table keeps its contributors alive and no address is reused while cached.
    _held: Vec<IUnknown>,
}
fn held_by(t: &Table) -> Vec<IUnknown> {
    let coverage = t.coverage.iter().flat_map(|c| [c.original_owner, c.original_cpu_heap]);
    let all = t.copies.iter().map(|c| c.heap).chain(t.srvs.iter().map(|s| s.resource)).chain(coverage).chain(t.descriptor_owners.iter().copied());
    all.filter(|&a| a != 0).filter_map(borrowed::<IUnknown>).collect()
}
/// CpuTableCache::Storage.
#[derive(Clone, Default)]
pub struct Cache {
    pub device: u64,
    pub entries: Vec<Arc<Entry>>,
    pub buckets: BTreeMap<u64, Vec<usize>>,
    pub descriptors: u32,
    pub hits: u64,
    pub misses: u64,
}
/// The cache key's FNV-1a.
pub fn hash(t: &Table) -> u64 {
    let mut h = 14695981039346656037u64;
    let mut word = |mut value: u64| {
        for _ in 0..8 {
            h = (h ^ (value as u8 as u64)).wrapping_mul(1099511628211);
            value >>= 8;
        }
    };
    word(t.source_root);
    word(t.parameter as u64);
    word(t.count as u64);
    word(t.kind.0 as u32 as u64);
    word(t.slot_types.len() as u64);
    for &kind in &t.slot_types {
        word(kind as u32 as u64);
    }
    word(t.copies.len() as u64);
    for c in &t.copies {
        word(c.offset as u64);
        word(c.source_offset as u64);
        word(c.count as u64);
        word(c.heap);
    }
    word(t.cbvs.len() as u64);
    for c in &t.cbvs {
        word(c.offset as u64);
        word(c.view.BufferLocation);
        word(c.view.SizeInBytes as u64);
    }
    word(t.srvs.len() as u64);
    for s in &t.srvs {
        word(s.offset as u64);
        word(s.resource);
        word(s.view.Format.0 as u32 as u64);
        word(s.view.ViewDimension.0 as u32 as u64);
        word(s.view.Shader4ComponentMapping as u64);
        // SAFETY: a buffer view's fields (the hash reads them whatever the dimension, as the C++ key did).
        let b = unsafe { s.view.Anonymous.Buffer };
        word(b.FirstElement);
        word(b.NumElements as u64);
        word(b.StructureByteStride as u64);
        word(b.Flags.0 as u32 as u64);
    }
    word(t.descriptor_owners.len() as u64);
    for &o in &t.descriptor_owners {
        word(o);
    }
    word(t.union_storage as u64);
    if t.union_storage {
        word(t.source_root);
        word(t.source_gpu);
        word(t.source_heap);
        word(t.sparse_slots.len() as u64);
        for s in &t.sparse_slots {
            word(s.offset as u64);
            word(s.kind as u32 as u64);
            word(s.stamp.gpu);
            word(s.stamp.heap);
            for &v in &s.stamp.value {
                word(v);
            }
        }
    }
    h
}
/// The cache key's equality.
pub fn same(a: &Table, b: &Table) -> bool {
    if a.source_root != b.source_root
        || a.parameter != b.parameter
        || a.count != b.count
        || a.kind != b.kind
        || a.slot_types != b.slot_types
        || a.copies.len() != b.copies.len()
        || a.cbvs.len() != b.cbvs.len()
        || a.srvs.len() != b.srvs.len()
        || a.descriptor_owners.len() != b.descriptor_owners.len()
        || a.union_storage != b.union_storage
    {
        return false;
    }
    if a.union_storage {
        if a.source_root != b.source_root || a.source_gpu != b.source_gpu || a.source_heap != b.source_heap || a.sparse_slots.len() != b.sparse_slots.len() {
            return false;
        }
        if a.sparse_slots.iter().zip(&b.sparse_slots).any(|(x, y)| x.offset != y.offset || x.kind != y.kind || x.stamp != y.stamp) {
            return false;
        }
    }
    a.copies == b.copies
        && a.cbvs.iter().zip(&b.cbvs).all(|(x, y)| x.offset == y.offset && x.view.BufferLocation == y.view.BufferLocation && x.view.SizeInBytes == y.view.SizeInBytes)
        && a.srvs.iter().zip(&b.srvs).all(|(x, y)| x.offset == y.offset && x.resource == y.resource && same_srv(&x.view, &y.view))
        && a.descriptor_owners == b.descriptor_owners
}

/// Assemble through the cache (the cache replaced by its next state on success).
///
/// # Safety
/// As for assemble.
pub unsafe fn assemble_cached(device: &ID3D12Device, plan: Option<&Plan>, cache: &mut Option<Cache>, limit: u32, issue: Issue, user: *mut core::ffi::c_void, failure: &mut u32) -> HRESULT {
    *failure = ARGUMENTS;
    if limit == 0 || limit > DEFAULT_LIMIT {
        return E_INVALIDARG;
    }
    *failure = NOT_READY;
    let Some(plan) = ready(plan) else { return E_INVALIDARG };
    let prior = cache.as_ref();
    *failure = DEVICE;
    if prior.is_some_and(|c| c.device != device.as_raw() as u64) {
        return E_INVALIDARG;
    }
    *failure = CAPACITY;
    let used = prior.map_or(0, |c| c.descriptors);
    if used > limit {
        return E_OUTOFMEMORY;
    }
    // (the entry found, or the miss it shares)
    let mut selections: Vec<Result<Arc<Entry>, usize>> = Vec::with_capacity(plan.tables.len());
    let mut misses: Vec<(&Table, u64)> = Vec::new();
    let (mut hits, mut added) = (0u64, 0u32);
    for table in &plan.tables {
        *failure = DEVICE;
        if !table_devices(device, table) {
            return E_INVALIDARG;
        }
        let key = hash(table);
        let found = prior.and_then(|c| c.buckets.get(&key).and_then(|bucket| bucket.iter().map(|&i| &c.entries[i]).find(|e| same(table, &e.key)).cloned()));
        if let Some(entry) = found {
            selections.push(Ok(entry));
            hits += 1;
            continue;
        }
        if let Some(at) = misses.iter().position(|&(t, h)| h == key && same(table, t)) {
            selections.push(Err(at));
            hits += 1;
            continue;
        }
        *failure = CAPACITY;
        if table.count > limit - used - added {
            return E_OUTOFMEMORY;
        }
        added += table.count;
        selections.push(Err(misses.len()));
        misses.push((table, key));
    }
    *failure = CAPACITY;
    let (entries, prior_hits, prior_misses) = prior.map_or((0, 0, 0), |c| (c.entries.len(), c.hits, c.misses));
    if entries + misses.len() > 65536 || hits > u64::MAX - prior_hits || misses.len() as u64 > u64::MAX - prior_misses {
        return E_OUTOFMEMORY;
    }
    let mut created = Vec::with_capacity(misses.len());
    for &(table, key) in &misses {
        // SAFETY: as documented.
        let heap = match unsafe { create(device, table, failure) } {
            Ok(h) => h,
            Err(hr) => return hr,
        };
        *failure = SRV_OWNER;
        // SAFETY: the C++ half's check of the single table's coverage.
        if !unsafe { issue(user, table, heap.as_raw(), heap.as_raw() as u64, table.count, false) } {
            return E_INVALIDARG;
        }
        let mut stored = table.clone();
        stored.leases.clear();
        let held = held_by(&stored);
        created.push(Arc::new(Entry { key: stored, heap, hash: key, _held: held }));
    }
    for (table, selected) in plan.tables.iter().zip(&selections) {
        let entry = match selected {
            Ok(entry) => entry,
            Err(miss) => &created[*miss],
        };
        *failure = SRV_OWNER;
        // SAFETY: the C++ half's issue of a live table and heap.
        if !unsafe { issue(user, table, entry.heap.as_raw(), entry.heap.as_raw() as u64, table.count, true) } {
            return E_INVALIDARG;
        }
    }
    if !created.is_empty() {
        let mut next = prior.cloned().unwrap_or(Cache { device: device.as_raw() as u64, ..Cache::default() });
        for entry in created {
            next.buckets.entry(entry.hash).or_default().push(next.entries.len());
            next.entries.push(entry);
        }
        next.descriptors = used + added;
        next.hits += hits;
        next.misses += misses.len() as u64;
        *cache = Some(next);
    } else if let Some(c) = cache.as_mut() {
        c.hits += hits;
    }
    *failure = NONE;
    S_OK
}

/// A pass's union candidate: a certified sparse SRV-only table of live SRV slots, one copy per slot.
fn eligible(t: &Table) -> bool {
    t.sparse_union_certified
        && t.source_root != 0
        && t.source_gpu != 0
        && t.source_heap != 0
        && t.kind == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
        && t.cbvs.is_empty()
        && t.srvs.is_empty()
        && !t.sparse_slots.is_empty()
        && t.sparse_slots.len() == t.copies.len()
        && t.slot_types.iter().all(|&k| k == D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0)
        && t.sparse_slots.iter().all(|s| s.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 && s.stamp.value[0] == 2)
}
fn same_slot(a: &Slot, b: &Slot) -> bool {
    a.offset == b.offset && a.kind == b.kind && a.stamp == b.stamp
}
struct Group {
    table: Table,
    slots: BTreeMap<u32, (Slot, CopyRow)>,
    index: usize,
}
/// A pass assembled but not yet committed: the combined plan, the staged cache, each plan table's combined index and
/// whether it joined a union, the followers counted as hits.
pub struct Pass {
    pub combined: Plan,
    pub staged: Option<Cache>,
    pub references: Vec<Vec<(usize, bool)>>,
    pub followers: u64,
}
/// The pass's combined plan: the eligible sparse tables united per source and parameter when every common slot agrees.
pub fn combine(plans: &[&Plan]) -> (Plan, Vec<Vec<(usize, bool)>>, usize) {
    let mut groups: Vec<Group> = Vec::new();
    let mut references: Vec<Vec<(Option<usize>, usize)>> = Vec::new();
    let mut combined = Plan::default();
    let mut count = 0usize;
    for plan in plans {
        if references.is_empty() {
            combined.scope = plan.scope;
            combined.ready = true;
        }
        count += plan.tables.len();
        let mut current = Vec::with_capacity(plan.tables.len());
        for table in &plan.tables {
            if !eligible(table) {
                current.push((None, combined.tables.len()));
                combined.tables.push(table.clone());
                continue;
            }
            let at = groups.iter().position(|g| {
                let p = &g.table;
                p.source_root == table.source_root
                    && p.source_gpu == table.source_gpu
                    && p.source_heap == table.source_heap
                    && p.parameter == table.parameter
                    && p.kind == table.kind
                    && table.sparse_slots.iter().all(|s| g.slots.get(&s.offset).is_none_or(|(prior, _)| same_slot(prior, s)))
            });
            let at = at.unwrap_or_else(|| {
                let mut seed = table.clone();
                seed.copies.clear();
                seed.sparse_slots.clear();
                seed.coverage.clear();
                seed.descriptor_owners.clear();
                seed.leases.clear();
                seed.union_storage = true;
                groups.push(Group { table: seed, slots: BTreeMap::new(), index: 0 });
                groups.len() - 1
            });
            let group = &mut groups[at];
            group.table.count = group.table.count.max(table.count);
            group.table.descriptor_owners.extend_from_slice(&table.descriptor_owners);
            group.table.coverage.extend_from_slice(&table.coverage);
            for (slot, copy) in table.sparse_slots.iter().zip(&table.copies) {
                match group.slots.entry(slot.offset) {
                    std::collections::btree_map::Entry::Vacant(v) => {
                        v.insert((*slot, *copy));
                    }
                    std::collections::btree_map::Entry::Occupied(mut o) => {
                        let found = &o.get().1;
                        if copy.heap < found.heap || (copy.heap == found.heap && copy.source_offset < found.source_offset) {
                            o.insert((*slot, *copy));
                        }
                    }
                }
            }
            current.push((Some(at), usize::MAX));
        }
        references.push(current);
    }
    for group in &mut groups {
        let mut table = core::mem::take(&mut group.table);
        table.slot_types = vec![D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0; table.count as usize];
        table.descriptor_owners.sort_unstable();
        table.descriptor_owners.dedup();
        table.leases = table.descriptor_owners.clone();
        for (slot, copy) in group.slots.values() {
            table.sparse_slots.push(*slot);
            table.copies.push(*copy);
        }
        group.index = combined.tables.len();
        combined.tables.push(table);
    }
    let references = references
        .into_iter()
        .map(|refs| refs.into_iter().map(|(group, table)| group.map_or((table, false), |g| (groups[g].index, true))).collect())
        .collect();
    (combined, references, count)
}

/// AssemblePass up to its commit (the C++ half issues every plan's tables from the combined ones, then commits).
///
/// # Safety
/// The plans' heaps and resources are live; `issue` and `user` are the C++ half's (the combined tables kept).
pub unsafe fn assemble_pass(device: &ID3D12Device, plans: &[Option<&Plan>], cache: &Option<Cache>, limit: u32, issue: Issue, user: *mut core::ffi::c_void, failure: &mut u32) -> Result<Pass, HRESULT> {
    *failure = ARGUMENTS;
    if plans.is_empty() || plans.len() > 2048 || limit == 0 || limit > DEFAULT_LIMIT {
        return Err(E_INVALIDARG);
    }
    *failure = DEVICE;
    if cache.as_ref().is_some_and(|c| c.device != device.as_raw() as u64) {
        return Err(E_INVALIDARG);
    }
    let mut ready_plans = Vec::with_capacity(plans.len());
    let mut count = 0usize;
    for &plan in plans {
        *failure = NOT_READY;
        let Some(plan) = ready(plan) else { return Err(E_INVALIDARG) };
        *failure = CAPACITY;
        if plan.tables.len() > 65536 - count {
            return Err(E_OUTOFMEMORY);
        }
        count += plan.tables.len();
        *failure = DEVICE;
        // Validate even contributors whose equivalent descriptor is not chosen as the union's physical copy source.
        if !plan.tables.iter().all(|t| table_devices(device, t)) {
            return Err(E_INVALIDARG);
        }
        ready_plans.push(plan);
    }
    let (combined, references, count) = combine(&ready_plans);
    let mut staged = cache.clone();
    // SAFETY: as documented.
    let hr = unsafe { assemble_cached(device, Some(&combined), &mut staged, limit, issue, user, failure) };
    if hr.is_err() {
        return Err(hr);
    }
    let followers = (count - combined.tables.len()) as u64;
    *failure = CAPACITY;
    if followers > u64::MAX - staged.as_ref().map_or(0, |c| c.hits) {
        return Err(E_OUTOFMEMORY);
    }
    Ok(Pass { combined, staged, references, followers })
}

// ---- the C ABI

/// # Safety
/// `cache` came from cvr_sba_cache_new (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_cache_free(cache: *mut Option<Cache>) {
    if !cache.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(cache) });
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sba_cache_new() -> *mut Option<Cache> {
    Box::into_raw(Box::new(None))
}
/// UniqueDescriptorCount, TableCount, HitCount, MissCount.
///
/// # Safety
/// `cache` is live (or null); `out` holds four values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_cache_counts(cache: *const Option<Cache>, out: *mut [u64; 4]) {
    // SAFETY: as documented.
    let (cache, out) = unsafe { (cache.as_ref().and_then(|c| c.as_ref()), &mut *out) };
    *out = cache.map_or([0; 4], |c| [c.descriptors as u64, c.entries.len() as u64, c.hits, c.misses]);
}
/// Assemble (`cache` null: without one).
///
/// # Safety
/// `device` and `plan` are live (the plan may be null); `cache` is live or null; `failure` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_assemble(
    device: *mut core::ffi::c_void,
    plan: *const Plan,
    cache: *mut Option<Cache>,
    limit: u32,
    issue: Issue,
    user: *mut core::ffi::c_void,
    failure: *mut u32,
) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device as u64) else { return E_INVALIDARG };
    // SAFETY: as documented.
    unsafe {
        match cache.as_mut() {
            None => assemble(&device, plan.as_ref(), issue, user, &mut *failure),
            Some(cache) => assemble_cached(&device, plan.as_ref(), cache, limit, issue, user, &mut *failure),
        }
    }
}
/// AssemblePass up to its commit: the pass into `*pass` (freed by cvr_sba_pass_free) when it holds.
///
/// # Safety
/// `device`, the plans (each possibly null) and `cache` are live; `pass` and `failure` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sba_assemble_pass(
    device: *mut core::ffi::c_void,
    plans: *const *const Plan,
    plan_count: usize,
    cache: *const Option<Cache>,
    limit: u32,
    issue: Issue,
    user: *mut core::ffi::c_void,
    pass: *mut *mut Pass,
    failure: *mut u32,
) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device as u64) else { return E_INVALIDARG };
    // SAFETY: as documented.
    unsafe {
        *pass = core::ptr::null_mut();
        let plans: Vec<Option<&Plan>> = if plan_count == 0 { Vec::new() } else { core::slice::from_raw_parts(plans, plan_count).iter().map(|p| p.as_ref()).collect() };
        match assemble_pass(&device, &plans, &*cache, limit, issue, user, &mut *failure) {
            Ok(built) => {
                *pass = Box::into_raw(Box::new(built));
                S_OK
            }
            Err(hr) => hr,
        }
    }
}
/// A plan table's place in the pass: its combined table (`*table`) and whether it joined a union.
///
/// # Safety
/// `pass` is live; the indices are in range; `table` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_pass_reference(pass: *const Pass, plan: usize, index: usize, table: *mut *const Table) -> bool {
    // SAFETY: as documented.
    let p = unsafe { &*pass };
    let (at, union) = p.references[plan][index];
    // SAFETY: as documented.
    unsafe { *table = &p.combined.tables[at] };
    union
}
/// The combined table's index (the C++ half's issued tables are in the combined order).
///
/// # Safety
/// As for cvr_sba_pass_reference.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_pass_index(pass: *const Pass, plan: usize, index: usize) -> usize {
    // SAFETY: as documented.
    unsafe { (&*pass).references[plan][index].0 }
}
/// Commits the pass: the staged cache, with its followers counted as hits, replaces `cache`.
///
/// # Safety
/// `pass` and `cache` are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_pass_commit(pass: *mut Pass, cache: *mut Option<Cache>) {
    // SAFETY: as documented.
    let (p, cache) = unsafe { (&mut *pass, &mut *cache) };
    if let Some(staged) = p.staged.as_mut() {
        staged.hits += p.followers;
    }
    *cache = p.staged.take();
}
/// # Safety
/// `pass` came from cvr_sba_assemble_pass (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_pass_free(pass: *mut Pass) {
    if !pass.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[cfg(test)]
mod tests {
    //! The cache key; the pass's union of sparse tables.
    use super::*;
    use crate::assembly_build::Stamp;
    use crate::geometry_pass_scope::Scope;

    fn slot(offset: u32, cookie: u64) -> Slot {
        let mut value = [0u64; 20];
        value[0] = 2;
        value[2] = cookie;
        Slot { offset, compact_index: offset, kind: D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0, stamp: Stamp { gpu: 0x1000 + offset as u64 * 32, heap: 9, value } }
    }
    fn sparse(offsets: &[(u32, u64)], heap: u64, owner: u64) -> Table {
        Table {
            parameter: 23,
            count: 16,
            kind: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            slot_types: vec![D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0; 16],
            copies: offsets.iter().map(|&(o, _)| CopyRow { offset: o, source_offset: o, count: 1, heap }).collect(),
            source_root: 1,
            source_gpu: 0x1000,
            source_heap: 9,
            sparse_slots: offsets.iter().map(|&(o, c)| slot(o, c)).collect(),
            sparse_union_certified: true,
            descriptor_owners: vec![owner],
            leases: vec![owner],
            ..Table::default()
        }
    }
    fn plan(tables: Vec<Table>) -> Plan {
        Plan { scope: Scope { root: 1, list: 2, generation: 3, occurrence: 4, source: 4, eye: 0 }, tables, missing: Vec::new(), descriptors: 16, ready: true }
    }

    #[test]
    fn cache_key() {
        let a = sparse(&[(1, 7)], 0x50, 0x60);
        let mut b = a.clone();
        assert!(same(&a, &b) && hash(&a) == hash(&b));
        b.leases = vec![0x99];
        assert!(same(&a, &b) && hash(&a) == hash(&b), "leases are not the key");
        b.descriptor_owners = vec![0x61];
        assert!(!same(&a, &b) && hash(&a) != hash(&b), "descriptor owners are");
        let mut c = a.clone();
        c.copies[0].heap = 0x51;
        assert!(!same(&a, &c) && hash(&a) != hash(&c));
        let (mut u, mut v) = (a.clone(), a.clone());
        u.union_storage = true;
        v.union_storage = true;
        v.sparse_slots[0].stamp.value[2] = 8;
        assert!(!same(&u, &v), "a union's slots are its key");
        assert!(same(&a, &{ let mut w = a.clone(); w.sparse_slots[0].stamp.value[2] = 8; w }), "a plain table's are not");
    }
    #[test]
    fn pass_union() {
        let first = plan(vec![sparse(&[(1, 7), (3, 8)], 0x52, 0x60)]);
        let second = plan(vec![sparse(&[(3, 8), (5, 9)], 0x51, 0x61), Table { parameter: 2, count: 4, slot_types: vec![0; 4], ..Table::default() }]);
        let (combined, references, count) = combine(&[&first, &second]);
        assert_eq!((combined.tables.len(), count), (2, 3), "the plain table, then one union");
        assert_eq!(references, vec![vec![(1, true)], vec![(1, true), (0, false)]]);
        let union = &combined.tables[1];
        assert!(union.union_storage);
        assert_eq!(union.sparse_slots.iter().map(|s| s.offset).collect::<Vec<_>>(), vec![1, 3, 5]);
        assert_eq!(union.copies[1].heap, 0x51, "the lowest copy source kept for a shared slot");
        assert_eq!(union.descriptor_owners, vec![0x60, 0x61]);
        assert_eq!(union.leases, union.descriptor_owners);
        let conflicting = plan(vec![sparse(&[(3, 99)], 0x53, 0x62)]);
        let (combined, references, _) = combine(&[&first, &conflicting]);
        assert_eq!((combined.tables.len(), references), (2, vec![vec![(0, true)], vec![(1, true)]]), "a slot that disagrees: another union");
        let mut uncertified = sparse(&[(1, 7)], 0x52, 0x60);
        uncertified.sparse_union_certified = false;
        let (combined, references, _) = combine(&[&plan(vec![uncertified])]);
        assert_eq!((combined.tables[0].union_storage, references), (false, vec![vec![(0, false)]]), "an uncertified table kept as it is");
    }
}
