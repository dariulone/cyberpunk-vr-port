//! The source binding assembly's plan (src/Render/SourceBindingAssembly.cpp, `BuildCore`, and the source table domains
//! of include/Render/SourceTablePlan.hpp): every shader use of a source draw located in exactly one root table range
//! (an unbounded range only through a complete selector result whose table covers the use's slot intervals, the
//! result's every table consumed and every site selection on its offsets), each required slot typed once and CBV byte
//! extents kept; the owned finite tables (one per declared finite domain, a NONE-flag CPU heap of the table's kind and
//! count) and sparse tail tables (canonical captured slots at their GPU positions, compact and distinct, SRV coverage
//! receipts) supply every non-CBV slot; private CBV copies (this occurrence's, fully copied, a DEFAULT buffer range) and
//! certified original CBVs replace the CBV slots, the rest listed missing; the one private T5 SRV of the multilayer
//! family (its original slot 23/5 certified, the private b4 finalized and byte-equal, the record plan matching) is
//! placed with its leases. The plan is ready when nothing is missing and every private CBV copy is finalized.
//!
//! In the Rust build (`CVR_RUST_VRIK`) BuildCore forwards here with its input lent as flat views (the private view's
//! own validation and the record plan computed by the C++ half, which then materializes the plan's tables); the D3D12
//! queries go through `Devices`; `tools/single_pass_tests/source_binding_assembly_test` runs its GPU cases against it in
//! its Rust variant, and the unit tests below cover the plan's rules.

use std::collections::{BTreeMap, BTreeSet};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::assembly_layout::{cbv_range, exact_structured, extent, layouts, same_srv, slot_range, Layout};
use crate::descriptor_views::{decode_cbv, decode_sampler, decode_srv, decode_uav};
use crate::early_depth_checks::ResourceUse;
use crate::geometry_pass_coverage::{Selectors, Str};
use crate::geometry_pass_scope::Scope;

/// source_binding_assembly::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const ROOT: u32 = 2;
    pub const SCOPE: u32 = 3;
    pub const EXTENT: u32 = 4;
    pub const TYPE: u32 = 5;
    pub const COLLISION: u32 = 6;
    pub const MISSING_TABLE: u32 = 7;
    pub const MISSING_USE: u32 = 8;
    pub const DYNAMIC_COVERAGE: u32 = 9;
    pub const CBV_RANGE: u32 = 10;
    pub const CBV_OWNER: u32 = 11;
    pub const CAPACITY: u32 = 12;
    pub const NOT_READY: u32 = 13;
    pub const DEVICE: u32 = 14;
    pub const SRV_RANGE: u32 = 16;
    pub const SRV_OWNER: u32 = 17;
}
use failure::*;

const RESOURCE_LIMIT: u32 = 1_000_000;
const SAMPLER_LIMIT: u32 = 2048;
/// descriptors::SourceSlotCaptureLimit.
const SLOT_LIMIT: usize = 4096;
/// source_cbv_snapshot::detail::MaxRequests.
const MAX_REQUESTS: u32 = 28;
/// multilayer_selectors::B4Bytes / MaxRecords; structured_snapshot_view::DomainBytes.
pub const B4_BYTES: usize = 228;
pub const MAX_RECORDS: usize = 32;
const DOMAIN_BYTES: u64 = 8192 * 128;
const CBV: i32 = D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0;
const SRV: i32 = D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0;

// ---- the lent input

/// descriptors::DescriptorStamp.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub gpu: u64,
    pub heap: u64,
    pub value: [u64; 20],
}
/// descriptors::OwnedSourceSlot.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub offset: u32,
    pub compact_index: u32,
    pub kind: i32,
    pub stamp: Stamp,
}
/// A FiniteTable (`present`: its table is set) with its descriptors::OwnedSourceTable's fields; a SparseTable adds its
/// slots.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Owned {
    pub scope: Scope,
    pub present: u32,
    pub parameter: u32,
    pub count: u32,
    pub cpu_heap: u64,
    pub owner: u64,
    pub identity: u64,
    pub version: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub slots: *const Slot,
    pub slot_count: usize,
}
/// A PrivateCbv with its value's and copies' fields (`value` / `copies`: their identities, 0 when unset).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PrivateCbv {
    pub scope: Scope,
    pub value: u64,
    pub copies: u64,
    pub parameter: u32,
    pub offset: u32,
    pub reflected: u32,
    pub padded: u32,
    pub private_offset: u64,
    pub copied: u64,
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub resource: u64,
    pub owner: u64,
    pub copies_owner: u64,
    pub copies_list: u64,
    pub copies_generation: u64,
    pub copies_occurrence: u64,
    pub copies_copied: u64,
    pub copies_count: u32,
    pub finalized: bool,
}
/// An OriginalCbv.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OriginalCbv {
    pub scope: Scope,
    pub parameter: u32,
    pub offset: u32,
    pub reflected: u32,
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub resource: u64,
    pub owner: u64,
    pub certified: bool,
}
/// structured_snapshot_view::Scope.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewScope {
    pub root: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub eye: u32,
}
/// The private SRV (`value`: its view's identity) with its view's, original slot's and private b4's fields, the view's
/// own validation and the record plan the C++ half computed (`plan_*`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PrivateSrv {
    pub scope: Scope,
    pub value: u64,
    pub parameter: u32,
    pub offset: u32,
    pub view_scope: ViewScope,
    pub view_resource: u64,
    pub view_owner: u64,
    pub view_srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub view_source_offset: u64,
    pub view_copied: u64,
    pub view_first_record: u32,
    pub view_record_span: u32,
    pub view_record_count: u32,
    pub view_records: [u32; MAX_RECORDS],
    pub view_b4: [u8; B4_BYTES],
    pub view_valid: bool,
    pub original_description: [u64; 20],
    pub original_resource: u64,
    pub original_view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub original_stamp: Stamp,
    pub original_owner: u64,
    pub original_identity: u64,
    pub original_version: u64,
    pub original_gpu: u64,
    pub original_heap: u64,
    pub resource_cookie: u64,
    pub original_count: u32,
    pub slot_certified: bool,
    pub b4: u64,
    pub b4_copies: u64,
    pub b4_resource: u64,
    pub b4_owner: u64,
    pub b4_reflected: u32,
    pub b4_copied: u64,
    pub b4_copies_owner: u64,
    pub b4_bytes: *const u8,
    pub b4_bytes_len: usize,
    pub b4_bytes_certified: bool,
    pub plan_ok: bool,
    pub plan_coverage_complete: bool,
    pub plan_superset: bool,
    pub plan_records: *const u32,
    pub plan_record_count: usize,
    pub plan_first_record: u32,
    pub plan_record_span: u32,
    pub plan_resource_offset: u64,
    pub plan_byte_extent: u32,
}
/// BuildCore's input (`guarded`: a guarded validation, with its T5 view's identity and receipt).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Input {
    pub scope: Scope,
    pub root: *const D3D12_ROOT_SIGNATURE_DESC1,
    pub uses: *const ResourceUse,
    pub use_count: usize,
    pub finite: *const Owned,
    pub finite_count: usize,
    pub sparse: *const Owned,
    pub sparse_count: usize,
    pub private_cbvs: *const PrivateCbv,
    pub private_cbv_count: usize,
    pub original_cbvs: *const OriginalCbv,
    pub original_cbv_count: usize,
    pub family: [Str; 3],
    pub selector_scope: Scope,
    pub selectors: *const Selectors,
    pub dynamic_coverage_certified: bool,
    pub guarded: bool,
    pub private_srvs: *const PrivateSrv,
    pub private_srv_count: usize,
    pub guarded_t5: u64,
    pub guarded_receipt: u64,
    pub multilayer: [Str; 3],
}

// ---- the plan

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyRow {
    pub offset: u32,
    pub source_offset: u32,
    pub count: u32,
    pub heap: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CbvRow {
    pub offset: u32,
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SrvRow {
    pub offset: u32,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub resource: u64,
}
/// source_binding_assembly::OwnedSrvSlot (its owners as addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CoverageRow {
    pub contributor: Scope,
    pub offset: u32,
    pub compact_index: u32,
    pub stamp: Stamp,
    pub resource_cookie: u64,
    pub original_identity: u64,
    pub original_version: u64,
    pub original_gpu: u64,
    pub original_heap: u64,
    pub original_count: u32,
    pub original_owner: u64,
    pub original_cpu_heap: u64,
}
/// A Plan::Table.
#[derive(Clone, Default)]
pub struct Table {
    pub parameter: u32,
    pub count: u32,
    pub source_scope: Scope,
    pub kind: D3D12_DESCRIPTOR_HEAP_TYPE,
    pub slot_types: Vec<i32>,
    pub copies: Vec<CopyRow>,
    pub cbvs: Vec<CbvRow>,
    pub srvs: Vec<SrvRow>,
    pub source_root: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub sparse_slots: Vec<Slot>,
    pub coverage: Vec<CoverageRow>,
    pub sparse_union_certified: bool,
    /// A pass's union of sparse tables (its slots and source keyed in the cache).
    pub union_storage: bool,
    pub descriptor_owners: Vec<u64>,
    pub leases: Vec<u64>,
}
/// A Plan.
#[derive(Default)]
pub struct Plan {
    pub scope: Scope,
    pub tables: Vec<Table>,
    pub missing: Vec<[u32; 3]>,
    pub descriptors: u32,
    pub ready: bool,
}

impl core::fmt::Debug for Plan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Plan({} tables, {} missing, ready {})", self.tables.len(), self.missing.len(), self.ready)
    }
}

/// The D3D12 queries BuildCore makes (the plugin's: the live objects by address).
pub trait Devices {
    fn heap_desc(&mut self, heap: u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC>;
    /// The device a device child (heap or resource) belongs to (0 when it cannot be read).
    fn device_of(&mut self, child: u64) -> u64;
    fn increment(&mut self, device: u64, kind: D3D12_DESCRIPTOR_HEAP_TYPE) -> u32;
    fn resource_desc(&mut self, resource: u64) -> D3D12_RESOURCE_DESC;
    fn gpu_va(&mut self, resource: u64) -> u64;
    fn cbv_range(&mut self, resource: u64, view: &D3D12_CONSTANT_BUFFER_VIEW_DESC, reflected: u32, offset: u64, private_buffer: bool) -> bool;
}

/// # Safety
/// `data` holds `len` elements (or `len` is 0).
unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    // SAFETY: as documented.
    if len == 0 || data.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(data, len) } }
}
pub(crate) fn valid_scope(s: &Scope) -> bool {
    s.root != 0 && s.list != 0 && s.generation != 0 && s.occurrence != 0 && s.source != 0 && s.eye < 2
}
fn same_names(a: &[Str; 3], b: &[Str; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.bytes() == y.bytes())
}
fn cpu_heap(dev: &mut dyn Devices, heap: u64, kind: D3D12_DESCRIPTOR_HEAP_TYPE, count: u32) -> bool {
    heap != 0 && dev.heap_desc(heap).is_some_and(|d| d.Type == kind && d.Flags == D3D12_DESCRIPTOR_HEAP_FLAG_NONE && d.NumDescriptors == count)
}
fn canonical(slot: &Slot) -> bool {
    match D3D12_DESCRIPTOR_RANGE_TYPE(slot.kind) {
        D3D12_DESCRIPTOR_RANGE_TYPE_CBV => decode_cbv(&slot.stamp.value).is_some(),
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV => decode_srv(&slot.stamp.value, None).is_some(),
        D3D12_DESCRIPTOR_RANGE_TYPE_UAV => decode_uav(&slot.stamp.value).is_some(),
        D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER => decode_sampler(&slot.stamp.value).is_some(),
        _ => false,
    }
}

/// source_tables::Build: the root's declared finite table domains (the parameters holding finite uses, with their
/// extents) and whether it holds; uses in an unbounded tail are deferred.
pub fn source_tables(root: &D3D12_ROOT_SIGNATURE_DESC1, uses: &[ResourceUse]) -> Option<Vec<(u32, u32)>> {
    source_plan(root, uses).map(|plan| plan.finite.iter().map(|t| (t.parameter, t.count)).collect())
}
/// A finite table of source_tables::Plan: its parameter, extent and the uses' slots (offset, count, range type).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FiniteTable {
    pub parameter: u32,
    pub count: u32,
    pub uses: Vec<(u32, u32, u32)>,
}
/// source_tables::Plan: the finite tables by parameter, and the uses deferred to an unbounded tail (parameter, offset,
/// use) in their order.
#[derive(Clone, Debug, Default)]
pub struct SourcePlan {
    pub finite: Vec<FiniteTable>,
    pub deferred: Vec<(u32, u32, ResourceUse)>,
}
/// source_tables::Build in full.
pub fn source_plan(root: &D3D12_ROOT_SIGNATURE_DESC1, uses: &[ResourceUse]) -> Option<SourcePlan> {
    let forbidden = D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE
        | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED
        | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
    if root.NumParameters == 0 || root.NumParameters > 64 || root.pParameters.is_null() || uses.is_empty() || (root.Flags & forbidden).0 != 0 {
        return None;
    }
    struct Range {
        parameter: u32,
        offset: u32,
        extent: u32,
        visibility: u32,
        desc: D3D12_DESCRIPTOR_RANGE1,
        tail: bool,
    }
    let mut ranges = Vec::new();
    let (mut extents, mut tails) = ([0u32; 64], [false; 64]);
    // SAFETY: the description's parameters.
    let parameters = unsafe { slice(root.pParameters, root.NumParameters as usize) };
    for (p, parameter) in parameters.iter().enumerate() {
        if parameter.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            continue;
        }
        // SAFETY: a table parameter.
        let table = unsafe { parameter.Anonymous.DescriptorTable };
        if table.NumDescriptorRanges == 0 || table.pDescriptorRanges.is_null() {
            return None;
        }
        // SAFETY: the table's ranges.
        let table_ranges = unsafe { slice(table.pDescriptorRanges, table.NumDescriptorRanges as usize) };
        let mut cursor = 0u64;
        let sampler = table_ranges[0].RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        for (n, range) in table_ranges.iter().enumerate() {
            if tails[p] || range.NumDescriptors == 0 || range.RangeType.0 > D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 || (range.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) != sampler {
                return None;
            }
            if range.OffsetInDescriptorsFromTableStart != D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND {
                cursor = range.OffsetInDescriptorsFromTableStart as u64;
            }
            if cursor > u32::MAX as u64 {
                return None;
            }
            let tail = range.NumDescriptors == u32::MAX;
            if tail {
                if n + 1 != table_ranges.len() {
                    return None;
                }
                tails[p] = true;
            } else if cursor + range.NumDescriptors as u64 > 1_000_000 || range.BaseShaderRegister as u64 + range.NumDescriptors as u64 > u32::MAX as u64 + 1 {
                return None;
            }
            ranges.push(Range { parameter: p as u32, offset: cursor as u32, extent: range.NumDescriptors, visibility: parameter.ShaderVisibility.0 as u32, desc: *range, tail });
            if !tail {
                cursor += range.NumDescriptors as u64;
                extents[p] = extents[p].max(cursor as u32);
            }
        }
    }
    let mut plan = SourcePlan::default();
    for use_ in uses {
        if use_.kind > D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 || use_.count == 0 {
            return None;
        }
        let mut matched: Option<&Range> = None;
        for range in &ranges {
            if range.desc.RangeType.0 as u32 != use_.kind
                || range.desc.RegisterSpace != use_.space
                || (range.visibility != D3D12_SHADER_VISIBILITY_ALL.0 as u32 && range.visibility != use_.visibility)
                || use_.shader_register < range.desc.BaseShaderRegister
            {
                continue;
            }
            let relative = (use_.shader_register - range.desc.BaseShaderRegister) as u64;
            if !range.tail && (relative >= range.extent as u64 || use_.count as u64 > range.extent as u64 - relative) {
                continue;
            }
            if relative + range.offset as u64 > u32::MAX as u64 || use_.shader_register as u64 + use_.count as u64 > u32::MAX as u64 + 1 || matched.is_some() {
                return None;
            }
            matched = Some(range);
        }
        let range = matched?;
        let offset = range.offset + (use_.shader_register - range.desc.BaseShaderRegister);
        if tails[range.parameter as usize] {
            plan.deferred.push((range.parameter, offset, *use_));
            continue;
        }
        let at = match plan.finite.iter().position(|t| t.parameter == range.parameter) {
            Some(at) => at,
            None => {
                plan.finite.push(FiniteTable { parameter: range.parameter, count: extents[range.parameter as usize], uses: Vec::new() });
                plan.finite.len() - 1
            }
        };
        plan.finite[at].uses.push((offset, use_.count, use_.kind));
    }
    plan.finite.sort_by_key(|t| t.parameter);
    Some(plan)
}

type Key = (u32, u32);
struct Required {
    types: BTreeMap<Key, i32>,
    cbv_bytes: BTreeMap<Key, u32>,
}
impl Required {
    fn add(&mut self, parameter: u32, offset: u32, kind: i32, bytes: u32) -> Result<(), u32> {
        match self.types.entry((parameter, offset)) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(kind);
            }
            std::collections::btree_map::Entry::Occupied(at) => {
                if *at.get() != kind {
                    return Err(COLLISION);
                }
            }
        }
        if self.types.len() > (RESOURCE_LIMIT + SAMPLER_LIMIT) as usize {
            return Err(CAPACITY);
        }
        if kind == CBV {
            if bytes == 0 || bytes > 65536 {
                return Err(CBV_RANGE);
            }
            let extent = self.cbv_bytes.entry((parameter, offset)).or_insert(0);
            *extent = (*extent).max(bytes);
            if self.cbv_bytes.len() > 128 {
                return Err(CAPACITY);
            }
        }
        Ok(())
    }
}

/// BuildCore; the error is the failure.
///
/// # Safety
/// The input's root, arrays, strings and selector result are valid.
pub unsafe fn build_core(i: &Input, dev: &mut dyn Devices) -> Result<Plan, u32> {
    // SAFETY: the lent arrays.
    let (uses, finite, sparse, private_cbvs, original_cbvs, private_srvs) = unsafe {
        (
            slice(i.uses, i.use_count),
            slice(i.finite, i.finite_count),
            slice(i.sparse, i.sparse_count),
            slice(i.private_cbvs, i.private_cbv_count),
            slice(i.original_cbvs, i.original_cbv_count),
            slice(i.private_srvs, i.private_srv_count),
        )
    };
    if !valid_scope(&i.scope)
        || i.root.is_null()
        || uses.is_empty()
        || uses.len() > 4096
        || finite.len() > 64
        || sparse.len() > 64
        || private_cbvs.len() > 128
        || original_cbvs.len() > 128
        || private_srvs.len() > 1
    {
        return Err(ARGUMENTS);
    }
    if !i.selectors.is_null() && i.selector_scope != i.scope {
        return Err(SCOPE);
    }
    // SAFETY: the lent root.
    let root = unsafe { &*i.root };
    let layouts: Vec<Layout> = layouts(root).ok_or(ROOT)?;
    let domains = source_tables(root, uses).ok_or(ROOT)?;
    // SAFETY: the root's parameters (checked by layouts).
    let parameters = unsafe { slice(root.pParameters, root.NumParameters as usize) };
    // SAFETY: the lent selector result.
    let selectors = unsafe { i.selectors.as_ref() };
    let (tables_of, selections_of) = match selectors {
        // SAFETY: the result's arrays.
        Some(s) => unsafe { (slice(s.tables, s.table_count), slice(s.selections, s.selection_count)) },
        None => (&[][..], &[][..]),
    };
    let mut required = Required { types: BTreeMap::new(), cbv_bytes: BTreeMap::new() };
    let mut dynamic_parameters = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    for use_ in uses {
        let (mut parameter, mut offset, mut matched) = (u32::MAX, 0u32, None);
        for (p, layout) in layouts.iter().enumerate() {
            let visibility = parameters[p].ShaderVisibility;
            for range in &layout.ranges {
                if range.desc.RangeType.0 as u32 != use_.kind
                    || range.desc.RegisterSpace != use_.space
                    || (visibility != D3D12_SHADER_VISIBILITY_ALL && visibility.0 as u32 != use_.visibility)
                    || use_.shader_register < range.desc.BaseShaderRegister
                {
                    continue;
                }
                let relative = (use_.shader_register - range.desc.BaseShaderRegister) as u64;
                if relative >= range.count as u64 || use_.count as u64 > range.count as u64 - relative {
                    continue;
                }
                if matched.is_some() || range.offset as u64 + relative > u32::MAX as u64 {
                    return Err(ROOT);
                }
                matched = Some(range);
                parameter = p as u32;
                offset = range.offset + relative as u32;
            }
        }
        let Some(matched) = matched else { return Err(MISSING_USE) };
        if matched.count == u32::MAX && use_.count > 1 {
            if selectors.filter(|s| (i.dynamic_coverage_certified || i.guarded) && s.complete && same_names(&s.family, &i.family) && !i.family[0].bytes().is_empty()).is_none() {
                return Err(DYNAMIC_COVERAGE);
            }
            let mut covered = false;
            for (index, table) in tables_of.iter().enumerate() {
                if table.parameter != parameter
                    || table.visibility != use_.visibility
                    || table.space != use_.space
                    || table.shader_register_base != use_.shader_register
                    || table.original_table_offset != offset
                    || table.declared_extent != use_.count
                    || table.root.bytes() != i.family[0].bytes()
                {
                    continue;
                }
                // SAFETY: the table's offsets and intervals.
                let (offsets, intervals) = unsafe { (slice(table.offsets, table.offset_count), slice(table.uses, table.use_count)) };
                if covered || offsets.is_empty() || offsets.len() > SLOT_LIMIT || offsets.windows(2).any(|w| w[0] >= w[1]) || table.count as u64 != *offsets.last().unwrap() as u64 + 1 {
                    return Err(DYNAMIC_COVERAGE);
                }
                let mut declared = Vec::new();
                for interval in intervals {
                    if interval.kind != use_.kind
                        || interval.count == 0
                        || interval.offset as u64 + interval.count as u64 > offset as u64 + use_.count as u64
                        || interval.offset < offset
                        || interval.count as usize > SLOT_LIMIT - declared.len()
                    {
                        return Err(DYNAMIC_COVERAGE);
                    }
                    declared.extend((0..interval.count).map(|n| interval.offset.wrapping_add(n)));
                }
                if declared != offsets {
                    return Err(DYNAMIC_COVERAGE);
                }
                for &selected in offsets {
                    if selected < offset || (selected - offset) as u64 >= use_.count as u64 {
                        return Err(DYNAMIC_COVERAGE);
                    }
                    required.add(parameter, selected, use_.kind as i32, use_.bytes)?;
                }
                consumed.insert(index);
                covered = true;
            }
            if !covered {
                return Err(DYNAMIC_COVERAGE);
            }
            dynamic_parameters.insert(parameter);
        } else {
            if use_.count > RESOURCE_LIMIT || offset as u64 + use_.count as u64 > RESOURCE_LIMIT as u64 {
                return Err(CAPACITY);
            }
            for n in 0..use_.count {
                required.add(parameter, offset + n, use_.kind as i32, use_.bytes)?;
            }
        }
    }
    if let Some(s) = selectors {
        if !same_names(&s.family, &i.family) || s.table_count > 64 || s.evaluated_sites as usize != s.selection_count || s.selection_count > SLOT_LIMIT {
            return Err(DYNAMIC_COVERAGE);
        }
        for (index, table) in tables_of.iter().enumerate() {
            if !dynamic_parameters.contains(&table.parameter) || !consumed.contains(&index) {
                return Err(DYNAMIC_COVERAGE);
            }
        }
        for selection in selections_of {
            let on_table = tables_of.iter().any(|table| {
                // SAFETY: the table's offsets.
                let offsets = unsafe { slice(table.offsets, table.offset_count) };
                selection.shader_index < table.declared_extent
                    && table.original_table_offset as u64 + selection.shader_index as u64 == selection.descriptor_offset as u64
                    && offsets.binary_search(&selection.descriptor_offset).is_ok()
            });
            if !on_table {
                return Err(DYNAMIC_COVERAGE);
            }
        }
    }
    let mut pending = Plan { scope: i.scope, ..Plan::default() };
    let mut indices: BTreeMap<u32, usize> = BTreeMap::new();
    let mut supplied: BTreeSet<Key> = BTreeSet::new();
    let (mut resource_count, mut sampler_count, mut selected_count) = (0u32, 0u32, 0usize);
    let mut new_table = |pending: &mut Plan, parameter: u32, count: u32| -> Result<usize, u32> {
        if parameter >= root.NumParameters {
            return Err(ROOT);
        }
        if indices.contains_key(&parameter) {
            return Err(COLLISION);
        }
        let layout = &layouts[parameter as usize];
        if !extent(&layout.head, layout.ranges.last(), count) {
            return Err(EXTENT);
        }
        let (cursor, limit) = if layout.head.sampler { (&mut sampler_count, SAMPLER_LIMIT) } else { (&mut resource_count, RESOURCE_LIMIT) };
        if count > limit - *cursor {
            return Err(CAPACITY);
        }
        *cursor += count;
        let fallback = if layout.head.sampler { D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 } else { SRV };
        let slot_types = (0..count).map(|n| slot_range(&layout.ranges, n).map_or(fallback, |r| layout.ranges[r].desc.RangeType.0)).collect();
        let kind = if layout.head.sampler { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV };
        indices.insert(parameter, pending.tables.len());
        pending.tables.push(Table { parameter, count, source_scope: i.scope, kind, slot_types, source_root: i.scope.root, ..Table::default() });
        Ok(pending.tables.len() - 1)
    };
    for q in finite {
        if q.scope != i.scope || q.present == 0 {
            return Err(SCOPE);
        }
        if !domains.iter().any(|&(p, _)| p == q.parameter) || q.owner == 0 || q.identity == 0 || q.version == 0 || q.source_gpu == 0 || q.source_heap == 0 {
            return Err(MISSING_TABLE);
        }
        let at = new_table(&mut pending, q.parameter, q.count)?;
        let table = &mut pending.tables[at];
        if !cpu_heap(dev, q.cpu_heap, table.kind, q.count) {
            return Err(TYPE);
        }
        table.copies.push(CopyRow { offset: 0, source_offset: 0, count: q.count, heap: q.cpu_heap });
        table.leases = vec![q.owner, q.cpu_heap];
        table.descriptor_owners = table.leases.clone();
        supplied.extend(required.types.keys().filter(|k| k.0 == q.parameter).copied());
    }
    for q in sparse {
        if q.scope != i.scope || q.present == 0 {
            return Err(SCOPE);
        }
        // SAFETY: the sparse table's slots.
        let slots = unsafe { slice(q.slots, q.slot_count) };
        if q.parameter >= root.NumParameters
            || !layouts[q.parameter as usize].head.tail
            || q.owner == 0
            || q.identity == 0
            || q.version == 0
            || q.source_gpu == 0
            || q.source_heap == 0
            || slots.is_empty()
            || !required.types.keys().any(|k| k.0 == q.parameter)
        {
            return Err(MISSING_TABLE);
        }
        if slots.len() > SLOT_LIMIT - selected_count {
            return Err(CAPACITY);
        }
        selected_count += slots.len();
        let at = new_table(&mut pending, q.parameter, q.count)?;
        let kind = pending.tables[at].kind;
        if !cpu_heap(dev, q.cpu_heap, kind, slots.len() as u32) {
            return Err(TYPE);
        }
        let device = dev.device_of(q.cpu_heap);
        if device == 0 {
            return Err(DEVICE);
        }
        let stride = dev.increment(device, kind) as u64;
        if stride == 0 {
            return Err(DEVICE);
        }
        let table = &mut pending.tables[at];
        table.leases = vec![q.owner, q.cpu_heap];
        table.descriptor_owners = table.leases.clone();
        table.source_root = i.scope.root;
        table.source_gpu = q.source_gpu;
        table.source_heap = q.source_heap;
        table.sparse_union_certified = dynamic_parameters.contains(&q.parameter) && (i.dynamic_coverage_certified || i.guarded);
        let mut compact = BTreeSet::new();
        let layout = &layouts[q.parameter as usize];
        for slot in slots {
            let key = (q.parameter, slot.offset);
            let use_ = required.types.get(&key);
            let range = slot_range(&layout.ranges, slot.offset).map(|r| &layout.ranges[r]);
            if slot.offset >= q.count
                || range.is_none_or(|r| r.desc.RangeType.0 != slot.kind)
                || use_.is_none_or(|&t| t != slot.kind)
                || slot.stamp.heap != q.source_heap
                || slot.offset as u64 > (u64::MAX - q.source_gpu) / stride
                || slot.stamp.gpu != q.source_gpu + slot.offset as u64 * stride
                || !canonical(slot)
            {
                return Err(TYPE);
            }
            if !supplied.insert(key) || !compact.insert(slot.compact_index) {
                return Err(COLLISION);
            }
            if slot.compact_index as usize >= slots.len() {
                return Err(EXTENT);
            }
            table.copies.push(CopyRow { offset: slot.offset, source_offset: slot.compact_index, count: 1, heap: q.cpu_heap });
            table.sparse_slots.push(*slot);
            if slot.kind == SRV {
                let Some(decoded) = decode_srv(&slot.stamp.value, None) else { return Err(EXTENT) };
                table.coverage.push(CoverageRow {
                    contributor: i.scope,
                    offset: slot.offset,
                    compact_index: slot.compact_index,
                    stamp: slot.stamp,
                    resource_cookie: decoded.resource_cookie,
                    original_identity: q.identity,
                    original_version: q.version,
                    original_gpu: q.source_gpu,
                    original_heap: q.source_heap,
                    original_count: q.count,
                    original_owner: q.owner,
                    original_cpu_heap: q.cpu_heap,
                });
            }
        }
    }
    if domains.iter().any(|(p, _)| !indices.contains_key(p)) {
        return Err(MISSING_TABLE);
    }
    if required.types.iter().any(|(k, &t)| t != CBV && !supplied.contains(k)) {
        return Err(MISSING_USE);
    }
    let mut replaced: BTreeSet<Key> = BTreeSet::new();
    let mut finalized = true;
    for q in private_cbvs {
        let key = (q.parameter, q.offset);
        if q.scope != i.scope
            || q.value == 0
            || q.copies == 0
            || q.copies_list != i.scope.list
            || q.copies_generation != i.scope.generation
            || q.copies_occurrence != i.scope.occurrence
        {
            return Err(SCOPE);
        }
        if q.owner == 0 || q.resource == 0 || q.copies_owner == 0 || q.copies_count == 0 || q.copies_count > MAX_REQUESTS || q.copied != q.reflected as u64 || q.copies_copied < q.copied {
            return Err(CBV_OWNER);
        }
        let need = required.cbv_bytes.get(&key);
        if need.is_none_or(|&n| q.reflected < n) || q.padded != q.view.SizeInBytes || !dev.cbv_range(q.resource, &q.view, q.reflected, q.private_offset, true) {
            return Err(CBV_RANGE);
        }
        if !replaced.insert(key) {
            return Err(COLLISION);
        }
        let Some(&at) = indices.get(&key.0) else { return Err(MISSING_TABLE) };
        let table = &mut pending.tables[at];
        if key.1 >= table.count {
            return Err(EXTENT);
        }
        table.cbvs.push(CbvRow { offset: key.1, view: q.view });
        table.leases.extend([q.resource, q.owner, q.copies_owner]);
        table.descriptor_owners.extend([q.resource, q.owner]);
        finalized &= q.finalized;
    }
    for v in original_cbvs {
        if v.scope != i.scope {
            return Err(SCOPE);
        }
        let key = (v.parameter, v.offset);
        if v.owner == 0 || v.resource == 0 || !v.certified {
            return Err(CBV_OWNER);
        }
        let need = required.cbv_bytes.get(&key);
        let gpu = dev.gpu_va(v.resource);
        if need.is_none_or(|&n| v.reflected < n) || v.view.BufferLocation < gpu || !dev.cbv_range(v.resource, &v.view, v.reflected, v.view.BufferLocation - gpu, false) {
            return Err(CBV_RANGE);
        }
        if !replaced.insert(key) {
            return Err(COLLISION);
        }
        let Some(&at) = indices.get(&key.0) else { return Err(MISSING_TABLE) };
        let table = &mut pending.tables[at];
        if key.1 >= table.count {
            return Err(EXTENT);
        }
        table.cbvs.push(CbvRow { offset: key.1, view: v.view });
        table.leases.extend([v.resource, v.owner]);
        table.descriptor_owners.extend([v.resource, v.owner]);
    }
    for q in private_srvs {
        if q.scope != i.scope || q.value == 0 {
            return Err(SCOPE);
        }
        let conditional = i.guarded && i.guarded_t5 == q.value;
        let expected = ViewScope { root: i.scope.root, list: i.scope.list, generation: i.scope.generation, occurrence: i.scope.occurrence, eye: i.scope.eye };
        if q.view_scope != expected {
            return Err(SCOPE);
        }
        if !q.slot_certified
            || q.original_owner == 0
            || q.original_resource == 0
            || q.resource_cookie == 0
            || q.original_description != q.original_stamp.value
            || q.original_stamp.value[2] != q.resource_cookie
            || (!conditional && !q.view_valid)
        {
            return Err(SRV_OWNER);
        }
        // SAFETY: the lent b4 bytes.
        let b4_bytes = unsafe { slice(q.b4_bytes, q.b4_bytes_len) };
        let b4 = private_cbvs.iter().find(|c| c.value != 0 && c.parameter == 19 && c.offset == 0);
        let b4_ok = b4.is_some_and(|b| {
            b.scope == i.scope
                && b.finalized
                && b.value == q.b4
                && b.copies == q.b4_copies
                && (q.b4_bytes_certified || conditional)
                && q.b4_bytes_len == B4_BYTES
                && !q.b4_bytes.is_null()
                && b4_bytes == q.view_b4
                && q.b4 != 0
                && q.b4_copies != 0
                && q.b4_reflected as usize == B4_BYTES
                && q.b4_copied == B4_BYTES as u64
        });
        if !b4_ok {
            return Err(SRV_OWNER);
        }
        if !same_names(&i.family, &i.multilayer) {
            return Err(DYNAMIC_COVERAGE);
        }
        let key = (q.parameter, q.offset);
        let layout = &layouts[23.min(layouts.len() - 1)];
        let use_ = required.types.get(&key);
        if key != (23, 5) || layouts.len() <= 23 || use_ != Some(&SRV) || parameters[23].ShaderVisibility != D3D12_SHADER_VISIBILITY_PIXEL || layout.head.tail {
            return Err(TYPE);
        }
        let range = slot_range(&layout.ranges, 5).map(|r| &layout.ranges[r]);
        if range.is_none_or(|r| r.desc.RangeType != D3D12_DESCRIPTOR_RANGE_TYPE_SRV || r.desc.RegisterSpace != 0 || (r.desc.BaseShaderRegister as u64 + 5).wrapping_sub(r.offset as u64) != 5) {
            return Err(TYPE);
        }
        let Some(original) = finite.iter().find(|t| t.present != 0 && t.parameter == 23) else { return Err(MISSING_TABLE) };
        if original.owner != q.original_owner
            || original.identity != q.original_identity
            || original.version != q.original_version
            || original.count != q.original_count
            || original.source_gpu != q.original_gpu
            || original.source_heap != q.original_heap
            || q.original_stamp.heap != original.source_heap
        {
            return Err(SRV_OWNER);
        }
        let device = dev.device_of(original.cpu_heap);
        if device == 0 || dev.device_of(q.original_resource) != device || q.view_resource == 0 || dev.device_of(q.view_resource) != device {
            return Err(DEVICE);
        }
        let stride = dev.increment(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as u64;
        if stride == 0 || 5 > (u64::MAX - original.source_gpu) / stride || q.original_stamp.gpu != original.source_gpu + 5 * stride {
            return Err(SRV_RANGE);
        }
        let Some(decoded) = decode_srv(&q.original_stamp.value, None) else { return Err(SRV_RANGE) };
        if decoded.implicit || decoded.resource_cookie != q.resource_cookie || !exact_structured(&decoded.view) || !same_srv(&decoded.view, &q.original_view) {
            return Err(SRV_RANGE);
        }
        let desc = dev.resource_desc(q.original_resource);
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width != DOMAIN_BYTES {
            return Err(SRV_RANGE);
        }
        let m = &decoded.metadata;
        if m.described
            && (m.dimension != desc.Dimension
                || m.format != desc.Format
                || m.width != desc.Width
                || m.height != desc.Height
                || m.depth_or_array_size != desc.DepthOrArraySize
                || m.mip_levels != desc.MipLevels
                || m.flags != desc.Flags
                || m.layout != desc.Layout
                || m.samples.Count != desc.SampleDesc.Count
                || m.samples.Quality != desc.SampleDesc.Quality)
        {
            return Err(SRV_RANGE);
        }
        // SAFETY: the C++ half's record plan.
        let records = unsafe { slice(q.plan_records, q.plan_record_count) };
        if !q.plan_ok
            || !q.plan_coverage_complete
            || !q.plan_superset
            || records.is_empty()
            || records.len() > MAX_RECORDS
            || q.view_record_count as usize != records.len()
            || q.view_first_record != q.plan_first_record
            || q.view_record_span != q.plan_record_span
            || q.view_source_offset != q.plan_resource_offset
            || q.view_copied != q.plan_byte_extent as u64
        {
            return Err(DYNAMIC_COVERAGE);
        }
        let mut padded = [0u32; MAX_RECORDS];
        padded[..records.len()].copy_from_slice(records);
        if q.view_records != padded {
            return Err(DYNAMIC_COVERAGE);
        }
        if !replaced.insert(key) {
            return Err(COLLISION);
        }
        let Some(&at) = indices.get(&23) else { return Err(MISSING_TABLE) };
        let table = &mut pending.tables[at];
        if 5 >= table.count {
            return Err(EXTENT);
        }
        table.srvs.push(SrvRow { offset: 5, view: q.view_srv, resource: q.view_resource });
        table.leases.extend([q.original_resource, q.original_owner, q.view_resource, q.view_owner]);
        if conditional {
            table.leases.push(i.guarded_receipt);
        }
        table.leases.extend([q.b4_resource, q.b4_owner, q.b4_copies_owner]);
        table.descriptor_owners.extend([q.view_resource, q.view_owner]);
    }
    for (&(parameter, offset), &bytes) in &required.cbv_bytes {
        if !replaced.contains(&(parameter, offset)) {
            pending.missing.push([parameter, offset, bytes]);
        }
    }
    pending.tables.sort_by_key(|t| t.parameter);
    pending.descriptors = resource_count + sampler_count;
    pending.ready = pending.missing.is_empty() && finalized;
    Ok(pending)
}

// ---- the C ABI

/// The plugin's D3D12 queries.
struct PluginDevices;
/// BuildCore with the plugin's D3D12 queries (the retained sources' plans, built on the Rust side).
///
/// # Safety
/// The input's arrays and the objects it names are live for the call.
pub unsafe fn build_plugin(i: &Input) -> Result<Plan, u32> {
    // SAFETY: as documented.
    unsafe { build_core(i, &mut PluginDevices) }
}
pub(crate) fn borrowed<T: Interface>(address: u64) -> Option<T> {
    let raw = address as *mut core::ffi::c_void;
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
impl Devices for PluginDevices {
    fn heap_desc(&mut self, heap: u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC> {
        // SAFETY: a live heap.
        borrowed::<ID3D12DescriptorHeap>(heap).map(|h| unsafe { h.GetDesc() })
    }
    fn device_of(&mut self, child: u64) -> u64 {
        let Some(child) = borrowed::<ID3D12DeviceChild>(child) else { return 0 };
        // SAFETY: a live device child.
        let mut device: Option<ID3D12Device> = None;
        match unsafe { child.GetDevice(&mut device) } {
            Ok(()) => device.map_or(0, |d| d.as_raw() as u64),
            Err(_) => 0,
        }
    }
    fn increment(&mut self, device: u64, kind: D3D12_DESCRIPTOR_HEAP_TYPE) -> u32 {
        // SAFETY: a live device.
        borrowed::<ID3D12Device>(device).map_or(0, |d| unsafe { d.GetDescriptorHandleIncrementSize(kind) })
    }
    fn resource_desc(&mut self, resource: u64) -> D3D12_RESOURCE_DESC {
        // SAFETY: a live resource.
        borrowed::<ID3D12Resource>(resource).map_or_else(D3D12_RESOURCE_DESC::default, |r| unsafe { r.GetDesc() })
    }
    fn gpu_va(&mut self, resource: u64) -> u64 {
        // SAFETY: a live resource.
        borrowed::<ID3D12Resource>(resource).map_or(0, |r| unsafe { r.GetGPUVirtualAddress() })
    }
    fn cbv_range(&mut self, resource: u64, view: &D3D12_CONSTANT_BUFFER_VIEW_DESC, reflected: u32, offset: u64, private_buffer: bool) -> bool {
        cbv_range(resource as *mut core::ffi::c_void, view, reflected, offset, private_buffer)
    }
}

/// The plan's head as the C++ half materializes it.
#[repr(C)]
pub struct PlanHead {
    pub scope: Scope,
    pub tables: usize,
    pub missing: *const [u32; 3],
    pub missing_count: usize,
    pub descriptors: u32,
    pub ready: bool,
}
/// One table's arrays as the C++ half copies them.
#[repr(C)]
pub struct TableView {
    pub parameter: u32,
    pub count: u32,
    pub source_scope: Scope,
    pub kind: i32,
    pub slot_types: *const i32,
    pub slot_type_count: usize,
    pub copies: *const CopyRow,
    pub copy_count: usize,
    pub cbvs: *const CbvRow,
    pub cbv_count: usize,
    pub srvs: *const SrvRow,
    pub srv_count: usize,
    pub source_root: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub sparse_slots: *const Slot,
    pub sparse_slot_count: usize,
    pub coverage: *const CoverageRow,
    pub coverage_count: usize,
    pub sparse_union_certified: bool,
    pub union_storage: bool,
    pub descriptor_owners: *const u64,
    pub descriptor_owner_count: usize,
    pub leases: *const u64,
    pub lease_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<Stamp>() == 176
        && core::mem::size_of::<Slot>() == 192
        && core::mem::size_of::<Owned>() == 128
        && core::mem::size_of::<PrivateCbv>() == 176
        && core::mem::size_of::<OriginalCbv>() == 104
        && core::mem::size_of::<ViewScope>() == 40
        && core::mem::size_of::<CopyRow>() == 24
        && core::mem::size_of::<CbvRow>() == 24
        && core::mem::size_of::<SrvRow>() == 56
        && core::mem::size_of::<CoverageRow>() == 296
        && core::mem::size_of::<PlanHead>() == 80
        && core::mem::size_of::<TableView>() == 224
        && core::mem::size_of::<PrivateSrv>() == 1112
        && core::mem::size_of::<Input>() == 328
);

/// BuildCore: the plan into `*plan` (a handle for the accessors below, freed by cvr_sba_plan_free) when it holds,
/// else its failure.
///
/// # Safety
/// `input` and its arrays are valid; `plan` and `failure` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_build_core(input: *const Input, plan: *mut *mut Plan, failure: *mut u32) -> bool {
    // SAFETY: as documented.
    unsafe {
        *plan = core::ptr::null_mut();
        match build_core(&*input, &mut PluginDevices) {
            Ok(built) => {
                *plan = Box::into_raw(Box::new(built));
                *failure = NONE;
                true
            }
            Err(reason) => {
                *failure = reason;
                false
            }
        }
    }
}
/// # Safety
/// `plan` came from cvr_sba_build_core; `head` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_plan_head(plan: *const Plan, head: *mut PlanHead) {
    // SAFETY: as documented.
    let (p, head) = unsafe { (&*plan, &mut *head) };
    *head = PlanHead { scope: p.scope, tables: p.tables.len(), missing: p.missing.as_ptr(), missing_count: p.missing.len(), descriptors: p.descriptors, ready: p.ready };
}
/// # Safety
/// `plan` came from cvr_sba_build_core; `index` is below its table count; `view` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_plan_table(plan: *const Plan, index: usize, view: *mut TableView) {
    // SAFETY: as documented.
    unsafe { cvr_sba_table_view(&(&*plan).tables[index], view) };
}
/// One table's arrays (a plan's, or one a pass assembles).
///
/// # Safety
/// `table` is live; `view` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_table_view(table: *const Table, view: *mut TableView) {
    // SAFETY: as documented.
    let (t, view) = unsafe { (&*table, &mut *view) };
    *view = TableView {
        parameter: t.parameter,
        count: t.count,
        source_scope: t.source_scope,
        kind: t.kind.0,
        slot_types: t.slot_types.as_ptr(),
        slot_type_count: t.slot_types.len(),
        copies: t.copies.as_ptr(),
        copy_count: t.copies.len(),
        cbvs: t.cbvs.as_ptr(),
        cbv_count: t.cbvs.len(),
        srvs: t.srvs.as_ptr(),
        srv_count: t.srvs.len(),
        source_root: t.source_root,
        source_gpu: t.source_gpu,
        source_heap: t.source_heap,
        sparse_slots: t.sparse_slots.as_ptr(),
        sparse_slot_count: t.sparse_slots.len(),
        coverage: t.coverage.as_ptr(),
        coverage_count: t.coverage.len(),
        sparse_union_certified: t.sparse_union_certified,
        union_storage: t.union_storage,
        descriptor_owners: t.descriptor_owners.as_ptr(),
        descriptor_owner_count: t.descriptor_owners.len(),
        leases: t.leases.as_ptr(),
        lease_count: t.leases.len(),
    };
}
/// A C++ plan's tables lent as views (their owners as addresses): a Rust plan copying them, freed by cvr_sba_plan_free.
///
/// # Safety
/// `scope` and the views' arrays are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_plan_lend(scope: *const Scope, ready: bool, descriptors: u32, views: *const TableView, count: usize) -> *mut Plan {
    // SAFETY: as documented.
    let views = unsafe { slice(views, count) };
    let tables = views
        .iter()
        // SAFETY: each view's arrays.
        .map(|v| unsafe {
            Table {
                parameter: v.parameter,
                count: v.count,
                source_scope: v.source_scope,
                kind: D3D12_DESCRIPTOR_HEAP_TYPE(v.kind),
                slot_types: slice(v.slot_types, v.slot_type_count).to_vec(),
                copies: slice(v.copies, v.copy_count).to_vec(),
                cbvs: slice(v.cbvs, v.cbv_count).to_vec(),
                srvs: slice(v.srvs, v.srv_count).to_vec(),
                source_root: v.source_root,
                source_gpu: v.source_gpu,
                source_heap: v.source_heap,
                sparse_slots: slice(v.sparse_slots, v.sparse_slot_count).to_vec(),
                coverage: slice(v.coverage, v.coverage_count).to_vec(),
                sparse_union_certified: v.sparse_union_certified,
                union_storage: v.union_storage,
                descriptor_owners: slice(v.descriptor_owners, v.descriptor_owner_count).to_vec(),
                leases: slice(v.leases, v.lease_count).to_vec(),
            }
        })
        .collect();
    // SAFETY: as documented.
    Box::into_raw(Box::new(Plan { scope: unsafe { *scope }, tables, missing: Vec::new(), descriptors, ready }))
}
/// # Safety
/// `plan` came from cvr_sba_build_core or cvr_sba_plan_lend (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_plan_free(plan: *mut Plan) {
    if !plan.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(plan) });
    }
}

#[cfg(test)]
mod tests {
    //! A finite table with a private CBV copy: the plan, then each refusal.
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        heaps: HashMap<u64, D3D12_DESCRIPTOR_HEAP_DESC>,
    }
    impl Devices for Fake {
        fn heap_desc(&mut self, heap: u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC> {
            self.heaps.get(&heap).copied()
        }
        fn device_of(&mut self, _: u64) -> u64 {
            1
        }
        fn increment(&mut self, _: u64, _: D3D12_DESCRIPTOR_HEAP_TYPE) -> u32 {
            32
        }
        fn resource_desc(&mut self, _: u64) -> D3D12_RESOURCE_DESC {
            D3D12_RESOURCE_DESC::default()
        }
        fn gpu_va(&mut self, _: u64) -> u64 {
            0x5000
        }
        fn cbv_range(&mut self, resource: u64, _: &D3D12_CONSTANT_BUFFER_VIEW_DESC, _: u32, _: u64, _: bool) -> bool {
            resource != 0xBAD
        }
    }
    const SCOPE_: Scope = Scope { root: 1, list: 2, generation: 3, occurrence: 4, source: 4, eye: 0 };
    fn none() -> [Str; 3] {
        [Str { data: core::ptr::null(), len: 0 }; 3]
    }
    struct Case {
        ranges: Vec<D3D12_DESCRIPTOR_RANGE1>,
        parameters: Vec<D3D12_ROOT_PARAMETER1>,
        root: D3D12_ROOT_SIGNATURE_DESC1,
        uses: Vec<ResourceUse>,
        finite: Vec<Owned>,
        cbvs: Vec<PrivateCbv>,
        dev: Fake,
    }
    fn case() -> Box<Case> {
        let mut c = Box::new(Case {
            ranges: vec![
                D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 4, OffsetInDescriptorsFromTableStart: 0, ..Default::default() },
                D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, NumDescriptors: 1, OffsetInDescriptorsFromTableStart: 4, ..Default::default() },
            ],
            parameters: vec![D3D12_ROOT_PARAMETER1::default()],
            root: D3D12_ROOT_SIGNATURE_DESC1::default(),
            uses: vec![
                ResourceUse { kind: SRV as u32, visibility: 5, space: 0, shader_register: 1, count: 1, bytes: 0 },
                ResourceUse { kind: CBV as u32, visibility: 5, space: 0, shader_register: 0, count: 1, bytes: 64 },
            ],
            finite: vec![Owned {
                scope: SCOPE_,
                present: 1,
                parameter: 0,
                count: 5,
                cpu_heap: 0x10,
                owner: 0x20,
                identity: 1,
                version: 1,
                source_gpu: 0x1000,
                source_heap: 0x2000,
                slots: core::ptr::null(),
                slot_count: 0,
            }],
            cbvs: vec![PrivateCbv {
                scope: SCOPE_,
                value: 0x30,
                copies: 0x31,
                parameter: 0,
                offset: 4,
                reflected: 64,
                padded: 256,
                private_offset: 0,
                copied: 64,
                view: D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: 0x5000, SizeInBytes: 256 },
                resource: 0x40,
                owner: 0x41,
                copies_owner: 0x42,
                copies_list: 2,
                copies_generation: 3,
                copies_occurrence: 4,
                copies_copied: 64,
                copies_count: 1,
                finalized: true,
            }],
            dev: Fake::default(),
        });
        c.dev.heaps.insert(0x10, D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 5, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 });
        c
    }
    fn build(c: &mut Case, scope: Scope) -> Result<Plan, u32> {
        c.parameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        c.parameters[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
        c.parameters[0].Anonymous.DescriptorTable = D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: c.ranges.len() as u32, pDescriptorRanges: c.ranges.as_ptr() };
        c.root = D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: 1, pParameters: c.parameters.as_ptr(), ..Default::default() };
        let input = Input {
            scope,
            root: &c.root,
            uses: c.uses.as_ptr(),
            use_count: c.uses.len(),
            finite: c.finite.as_ptr(),
            finite_count: c.finite.len(),
            sparse: core::ptr::null(),
            sparse_count: 0,
            private_cbvs: c.cbvs.as_ptr(),
            private_cbv_count: c.cbvs.len(),
            original_cbvs: core::ptr::null(),
            original_cbv_count: 0,
            family: none(),
            selector_scope: Scope::default(),
            selectors: core::ptr::null(),
            dynamic_coverage_certified: false,
            guarded: false,
            private_srvs: core::ptr::null(),
            private_srv_count: 0,
            guarded_t5: 0,
            guarded_receipt: 0,
            multilayer: none(),
        };
        // SAFETY: the case's arrays outlive the call.
        unsafe { build_core(&input, &mut c.dev) }
    }

    #[test]
    fn plan() {
        let mut c = case();
        let plan = build(&mut c, SCOPE_).unwrap();
        assert_eq!((plan.tables.len(), plan.descriptors, plan.ready), (1, 5, true));
        let t = &plan.tables[0];
        assert_eq!((t.parameter, t.count, t.kind, t.source_root), (0, 5, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 1));
        assert_eq!(t.slot_types, vec![SRV, SRV, SRV, SRV, CBV]);
        assert_eq!(t.copies, vec![CopyRow { offset: 0, source_offset: 0, count: 5, heap: 0x10 }]);
        assert_eq!((t.cbvs.len(), t.cbvs[0].offset), (1, 4));
        assert_eq!(t.leases, vec![0x20, 0x10, 0x40, 0x41, 0x42]);
        assert_eq!(t.descriptor_owners, vec![0x20, 0x10, 0x40, 0x41]);
        c.cbvs[0].finalized = false;
        assert!(!build(&mut c, SCOPE_).unwrap().ready, "an unfinalized copy: not ready");
        c.cbvs.clear();
        let plan = build(&mut c, SCOPE_).unwrap();
        assert_eq!((plan.missing.clone(), plan.ready), (vec![[0, 4, 64]], false), "the CBV missing");
    }
    #[test]
    fn refusals() {
        let mut c = case();
        assert_eq!(build(&mut c, Scope { eye: 2, ..SCOPE_ }).unwrap_err(), ARGUMENTS);
        let mut c = case();
        c.finite[0].present = 0;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), SCOPE);
        let mut c = case();
        c.finite.clear();
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), MISSING_TABLE, "the declared finite domain unsupplied");
        let mut c = case();
        c.uses[0].shader_register = 9;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), ROOT, "no range holds the use: the table domains refuse it");
        let mut c = case();
        c.dev.heaps.get_mut(&0x10).unwrap().NumDescriptors = 4;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), TYPE, "a heap of another size");
        let mut c = case();
        c.finite.push(c.finite[0]);
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), COLLISION, "two tables on one parameter");
        let mut c = case();
        c.cbvs[0].copies_count = 0;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), CBV_OWNER);
        let mut c = case();
        c.cbvs[0].resource = 0xBAD;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), CBV_RANGE);
        let mut c = case();
        c.cbvs[0].reflected = 32;
        c.cbvs[0].copied = 32;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), CBV_RANGE, "fewer bytes than the use reads");
        let mut c = case();
        c.cbvs.push(c.cbvs[0]);
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), COLLISION, "one slot replaced twice");
        let mut c = case();
        c.cbvs[0].copies_generation = 9;
        assert_eq!(build(&mut c, SCOPE_).unwrap_err(), SCOPE, "another occurrence's copies");
    }
}
