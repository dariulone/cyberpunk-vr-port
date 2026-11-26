//! The stereo geometry pass's coverage (src/Render/StereoGeometryPass.cpp, `stereo::Coverage` and `Selections`): every
//! shader resource a family's source draw reads is accounted for exactly once -- a singleton space-0 CBV / SRV by its
//! paired binding (in the original root parameter it names, with the pair's patched bytes or none for an SRV), a
//! sampler by the static samplers, anything else by one admitted shared table (a CPU heap of the use's kind whose finite
//! range holds the use, or for an unbounded SRV range both eyes' selector results, whose tables cover the use's slot
//! intervals and whose site selections land on those tables' offsets); every paired binding and shared table used, and
//! a shared read contract needed whenever a shared table or an SRV pair is read. The root is the paired root's original
//! serialized description (1.1, at most 64 parameters, no local root or directly indexed heaps).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Coverage forwards to `cvr_sgp_coverage` with the family, the pairs, the shared
//! tables and both eyes' selector results lent as flat views (the heaps' descriptions are read here); the unit tests
//! below cover the admitted layout and each refusal.

use std::collections::{BTreeMap, BTreeSet};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::early_depth_checks::ResourceUse;
use crate::geometry_pass_checks::{locate, static_sampler};
use crate::geometry_pass_scope::{draw_scope, Scope};
use crate::paired_root::{Binding, Location};

/// source_selectors::MaxTotalSites.
pub const MAX_TOTAL_SITES: usize = 4096;

/// A lent string (a SHA-256 name).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Str {
    pub data: *const u8,
    pub len: usize,
}
impl Str {
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the caller's string, alive for the call.
        unsafe { slice(self.data, self.len) }
    }
}
/// A BankPair as Coverage reads it: its binding, each eye's SRV source (address) and CBV bytes' length.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Pair {
    pub binding: Binding,
    pub source: [u64; 2],
    pub bytes: [u64; 2],
}
/// A SharedBankTable as Coverage reads it (`leased`: it holds leases; `heap`: its CPU heap's address).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Shared {
    pub parameter: u32,
    pub offset: u32,
    pub count: u32,
    pub leased: u32,
    pub heap: u64,
}
/// source_selectors::SlotInterval.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Interval {
    pub offset: u32,
    pub count: u32,
    pub kind: u32,
}
/// source_selectors::SiteSelection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    pub site_index: u32,
    pub source_dword: u32,
    pub shader_index: u32,
    pub descriptor_offset: u32,
}
/// source_selectors::TableSelection lent (its key's root name, the key, offsets, count and slot intervals).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub root: Str,
    pub visibility: u32,
    pub parameter: u32,
    pub space: u32,
    pub shader_register_base: u32,
    pub original_table_offset: u32,
    pub declared_extent: u32,
    pub offsets: *const u32,
    pub offset_count: usize,
    pub count: u32,
    pub uses: *const Interval,
    pub use_count: usize,
}
/// source_selectors::Result lent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Selectors {
    pub family: [Str; 3],
    pub evaluated_sites: u32,
    pub suppressed: u32,
    pub complete: bool,
    pub permission: bool,
    pub permitted: bool,
    pub selections: *const Selection,
    pub selection_count: usize,
    pub tables: *const Table,
    pub table_count: usize,
}
/// One eye: the shared-read proof's scope, the source draw's root and side, the proof's GPU input owner and selectors.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Eye {
    pub scope: Scope,
    pub root: u64,
    pub side: u32,
    pub owner: u64,
    pub selectors: *const Selectors,
}
/// Coverage's input: the family (source root, uses, names), the paired root's plan, the pairs and shared tables.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub source_root: u64,
    pub uses: *const ResourceUse,
    pub use_count: usize,
    pub bindings: *const Location,
    pub binding_count: usize,
    pub serialized: *const u8,
    pub serialized_len: usize,
    pub pairs: *const Pair,
    pub pair_count: usize,
    pub shared: *const Shared,
    pub shared_count: usize,
    pub original_parameters: u32,
    pub family: [Str; 3],
    pub eyes: [Eye; 2],
}
const _: () = assert!(
    core::mem::size_of::<Str>() == 16
        && core::mem::size_of::<Pair>() == 48
        && core::mem::size_of::<Shared>() == 24
        && core::mem::size_of::<Interval>() == 12
        && core::mem::size_of::<Selection>() == 16
        && core::mem::size_of::<Table>() == 80
        && core::mem::size_of::<Selectors>() == 96
        && core::mem::size_of::<Eye>() == 80
        && core::mem::size_of::<Input>() == 304
);

/// # Safety
/// `data` is valid for `len` elements (or `len` is 0).
unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if len == 0 || data.is_null() {
        &[]
    } else {
        // SAFETY: as documented.
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
fn same_family(a: &[Str; 3], b: &[Str; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.bytes() == y.bytes())
}
fn visible(root: D3D12_SHADER_VISIBILITY, use_: u32) -> bool {
    root == D3D12_SHADER_VISIBILITY_ALL || root.0 as u32 == use_
}

/// Selections: one of the result's tables covers the use at its location (sorted, distinct offsets made of its slot
/// intervals inside the use, within the shared table's extent); that table is consumed.
///
/// # Safety
/// The result's arrays are valid.
unsafe fn selections(result: &Selectors, family: &[Str; 3], use_: &ResourceUse, parameter: u32, offset: u32, extent: u32, consumed: &mut BTreeSet<usize>) -> bool {
    if !result.complete
        || !same_family(&result.family, family)
        || result.table_count > 64
        || result.selection_count > MAX_TOTAL_SITES
        || result.evaluated_sites as usize != result.selection_count
        || result.permission
        || result.permitted
        || result.suppressed != 0
    {
        return false;
    }
    let mut matched = false;
    // SAFETY: the result's tables.
    for (index, table) in unsafe { slice(result.tables, result.table_count) }.iter().enumerate() {
        if table.parameter != parameter
            || table.visibility != use_.visibility
            || table.space != use_.space
            || table.shader_register_base != use_.shader_register
            || table.original_table_offset != offset
            || table.declared_extent != use_.count
            || table.root.bytes() != family[0].bytes()
        {
            continue;
        }
        // SAFETY: the table's offsets and intervals.
        let (offsets, uses) = unsafe { (slice(table.offsets, table.offset_count), slice(table.uses, table.use_count)) };
        if matched
            || offsets.is_empty()
            || offsets.len() > MAX_TOTAL_SITES
            || offsets.windows(2).any(|w| w[0] >= w[1])
            || table.count as u64 != *offsets.last().unwrap() as u64 + 1
            || table.count > extent
        {
            return false;
        }
        let mut intervals = Vec::new();
        for interval in uses {
            if interval.kind != use_.kind
                || interval.count == 0
                || interval.offset < offset
                || interval.offset as u64 + interval.count as u64 > offset as u64 + use_.count as u64
                || interval.count as usize > MAX_TOTAL_SITES - intervals.len()
            {
                return false;
            }
            intervals.extend((0..interval.count).map(|n| interval.offset.wrapping_add(n)));
        }
        if intervals != offsets {
            return false;
        }
        if offsets.iter().any(|&o| o < offset || (o - offset) as u64 >= use_.count as u64 || o >= extent) {
            return false;
        }
        consumed.insert(index);
        matched = true;
    }
    matched
}

/// Coverage; `heap` describes a shared table's CPU heap (none when it has none). `needs_contract` is set when a shared
/// read contract must back the draw.
///
/// # Safety
/// The input's arrays, strings and selector results are valid.
pub unsafe fn coverage(input: &Input, heap: &dyn Fn(u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC>, needs_contract: &mut bool) -> bool {
    // SAFETY: the lent arrays.
    let (uses, bindings, serialized, pairs, shared) = unsafe {
        (
            slice(input.uses, input.use_count),
            slice(input.bindings, input.binding_count),
            slice(input.serialized, input.serialized_len),
            slice(input.pairs, input.pair_count),
            slice(input.shared, input.shared_count),
        )
    };
    *needs_contract = !shared.is_empty();
    if input.source_root == 0
        || input.eyes[0].root != input.source_root
        || input.eyes[1].root != input.source_root
        || uses.is_empty()
        || uses.len() > 4096
        || shared.len() > 64
        || pairs.len() != bindings.len()
        || serialized.is_empty()
    {
        return false;
    }
    let mut raw: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: the plan's serialized bytes; an out-interface (one reference).
    if unsafe { D3D12CreateVersionedRootSignatureDeserializer(serialized.as_ptr() as *const _, serialized.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw) }
        .is_err()
        || raw.is_null()
    {
        return false;
    }
    // SAFETY: the reference the creation added.
    let reader = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
    // SAFETY: the deserializer's own description, alive with it.
    let Ok(decoded) = (unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }) else {
        return false;
    };
    // SAFETY: a 1.1 description, as asked for.
    let root = unsafe { &(*decoded).Anonymous.Desc_1_1 };
    let original = input.original_parameters;
    let forbidden = D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE
        | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED
        | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
    if root.NumParameters > 64 || original > root.NumParameters || (root.Flags & forbidden).0 != 0 {
        return false;
    }
    // SAFETY: the description's parameters.
    let parameters = unsafe { slice(root.pParameters, root.NumParameters as usize) };
    let mut tables: BTreeMap<u32, (&Shared, D3D12_DESCRIPTOR_HEAP_DESC)> = BTreeMap::new();
    for table in shared {
        if table.parameter >= original || table.count == 0 || table.heap == 0 || table.leased == 0 || tables.contains_key(&table.parameter) {
            return false;
        }
        let Some(desc) = heap(table.heap) else { return false };
        if desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE || table.offset > desc.NumDescriptors || table.count > desc.NumDescriptors - table.offset {
            return false;
        }
        if parameters[table.parameter as usize].ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            return false;
        }
        tables.insert(table.parameter, (table, desc));
    }
    let mut used_pairs = BTreeSet::new();
    let mut used_shared = BTreeSet::new();
    let mut consumed = [BTreeSet::new(), BTreeSet::new()];
    let (cbv, srv, uav, sampler) = (
        D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 as u32,
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32,
        D3D12_DESCRIPTOR_RANGE_TYPE_UAV.0 as u32,
        D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32,
    );
    for use_ in uses {
        if use_.count == 0 || (use_.visibility != 1 && use_.visibility != 5) || use_.kind == uav || use_.kind > 3 {
            return false;
        }
        let mut paired = None;
        if use_.space == 0 && use_.count == 1 {
            for (index, binding) in bindings.iter().enumerate() {
                if binding.binding.kind.0 as u32 == use_.kind
                    && binding.binding.visibility.0 as u32 == use_.visibility
                    && binding.binding.source_register == use_.shader_register
                {
                    if paired.is_some() {
                        return false;
                    }
                    paired = Some(index);
                }
            }
        }
        if let Some(index) = paired {
            let location = &bindings[index];
            if !used_pairs.insert(index) || location.source_parameter >= original {
                return false;
            }
            let source = &parameters[location.source_parameter as usize];
            if !visible(source.ShaderVisibility, use_.visibility) || source.ParameterType != location.source_type {
                return false;
            }
            if source.ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                // SAFETY: the description's tables.
                match unsafe { locate(root, original, use_) } {
                    Some((parameter, offset, _)) if parameter == location.source_parameter && offset == location.source_offset => {}
                    _ => return false,
                }
            } else {
                let expected = if use_.kind == cbv { D3D12_ROOT_PARAMETER_TYPE_CBV } else { D3D12_ROOT_PARAMETER_TYPE_SRV };
                // SAFETY: a root descriptor parameter.
                let descriptor = unsafe { source.Anonymous.Descriptor };
                if source.ParameterType != expected || descriptor.RegisterSpace != 0 || descriptor.ShaderRegister != use_.shader_register {
                    return false;
                }
            }
            let mut value = None;
            for pair in pairs {
                if pair.binding == location.binding {
                    if value.is_some() {
                        return false;
                    }
                    value = Some(pair);
                }
            }
            let Some(value) = value else { return false };
            if use_.kind == cbv {
                if use_.bytes == 0 || use_.bytes > 65536 {
                    return false;
                }
                if (0..2).any(|eye| value.source[eye] != 0 || value.bytes[eye] < use_.bytes as u64) {
                    return false;
                }
            } else if use_.kind == srv {
                *needs_contract = true;
                if value.bytes.iter().any(|&b| b != 0) {
                    return false;
                }
            } else {
                return false;
            }
            continue;
        }
        if use_.kind == cbv {
            return false;
        }
        // SAFETY: the description's tables and samplers.
        let (location, fixed) = unsafe { (locate(root, original, use_), static_sampler(root, use_)) };
        if fixed {
            if location.is_some() {
                return false;
            }
            continue;
        }
        let Some((parameter, offset, range)) = location else { return false };
        let Some(&(table, desc)) = tables.get(&parameter) else { return false };
        let kind = if use_.kind == sampler { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV };
        if desc.Type != kind {
            return false;
        }
        used_shared.insert(parameter);
        *needs_contract = true;
        // SAFETY: the located range, inside the description.
        if unsafe { (*range).NumDescriptors } == u32::MAX && use_.count > 1 {
            if use_.kind != srv || input.family[0].bytes().is_empty() || input.family[1].bytes().is_empty() {
                return false;
            }
            for (eye, proof) in input.eyes.iter().enumerate() {
                if !draw_scope(&proof.scope, input.source_root, proof.root, proof.side) || proof.owner == 0 || proof.selectors.is_null() {
                    return false;
                }
                // SAFETY: the eye's lent selector result.
                if !unsafe { selections(&*proof.selectors, &input.family, use_, parameter, offset, table.count, &mut consumed[eye]) } {
                    return false;
                }
            }
        } else if offset as u64 + use_.count as u64 > table.count as u64 {
            return false;
        }
    }
    if used_pairs.len() != bindings.len() || used_shared.len() != tables.len() {
        return false;
    }
    for (eye, proof) in input.eyes.iter().enumerate() {
        if proof.selectors.is_null() {
            continue;
        }
        // SAFETY: the eye's lent selector result and its arrays.
        let result = unsafe { &*proof.selectors };
        if !same_family(&result.family, &input.family)
            || !result.complete
            || result.table_count != consumed[eye].len()
            || result.evaluated_sites as usize != result.selection_count
            || result.selection_count > MAX_TOTAL_SITES
        {
            return false;
        }
        if (0..result.table_count).any(|t| !consumed[eye].contains(&t)) {
            return false;
        }
        // SAFETY: as above.
        let (tables, chosen) = unsafe { (slice(result.tables, result.table_count), slice(result.selections, result.selection_count)) };
        let mut sites = BTreeSet::new();
        for selection in chosen {
            if selection.site_index >= result.evaluated_sites
                || !sites.insert(selection.site_index)
                || !tables.iter().any(|table| {
                    // SAFETY: the table's offsets.
                    let offsets = unsafe { slice(table.offsets, table.offset_count) };
                    selection.shader_index < table.declared_extent
                        && table.original_table_offset as u64 + selection.shader_index as u64 == selection.descriptor_offset as u64
                        && offsets.binary_search(&selection.descriptor_offset).is_ok()
                })
            {
                return false;
            }
        }
        for table in tables {
            // SAFETY: the table's offsets.
            for &offset in unsafe { slice(table.offsets, table.offset_count) } {
                if !chosen.iter().any(|s| s.descriptor_offset == offset && table.original_table_offset as u64 + s.shader_index as u64 == offset as u64) {
                    return false;
                }
            }
        }
    }
    true
}

// ---- the C ABI

fn plugin_heap(heap: u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC> {
    let raw = heap as *mut core::ffi::c_void;
    // SAFETY: the table's held CPU heap, alive for the call.
    unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&raw) }.map(|h| unsafe { h.GetDesc() })
}
/// Coverage (the C++ half lends the family, plan, pairs, shared tables and selector results).
///
/// # Safety
/// `input` and its arrays are valid; `needs_contract` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_coverage(input: *const Input, needs_contract: *mut bool) -> bool {
    let mut needs = false;
    // SAFETY: as documented.
    let covered = unsafe { coverage(&*input, &plugin_heap, &mut needs) };
    // SAFETY: as documented.
    unsafe { *needs_contract = needs };
    covered
}

#[cfg(test)]
mod tests {
    //! A family with a paired root CBV, a shared finite SRV table, an unbounded selector-covered SRV table and a static
    //! sampler; then each refusal.
    use super::*;
    use windows::Win32::Graphics::Direct3D::ID3DBlob;

    const ROOT: u64 = 0x5000;
    fn serialized() -> Vec<u8> {
        let ranges = [
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 8, BaseShaderRegister: 0, RegisterSpace: 0, ..Default::default() },
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: u32::MAX, BaseShaderRegister: 0, RegisterSpace: 1, ..Default::default() },
        ];
        let mut parameters = [D3D12_ROOT_PARAMETER1::default(); 3];
        parameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
        parameters[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_VERTEX;
        parameters[0].Anonymous.Descriptor = D3D12_ROOT_DESCRIPTOR1 { ShaderRegister: 0, RegisterSpace: 0, Flags: D3D12_ROOT_DESCRIPTOR_FLAG_NONE };
        for (p, range) in parameters[1..].iter_mut().zip(&ranges) {
            p.ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
            p.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            p.Anonymous.DescriptorTable = D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 1, pDescriptorRanges: range };
        }
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        let mut desc = D3D12_VERSIONED_ROOT_SIGNATURE_DESC { Version: D3D_ROOT_SIGNATURE_VERSION_1_1, ..Default::default() };
        desc.Anonymous.Desc_1_1 = D3D12_ROOT_SIGNATURE_DESC1 {
            NumParameters: 3,
            pParameters: parameters.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let (mut blob, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        // SAFETY: a description whose arrays outlive the call.
        unsafe { D3D12SerializeVersionedRootSignature(&desc, &mut blob, Some(&mut error)) }.unwrap();
        let blob = blob.unwrap();
        // SAFETY: the blob's own buffer.
        unsafe { core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }.to_vec()
    }
    fn s(text: &'static str) -> Str {
        Str { data: text.as_ptr(), len: text.len() }
    }
    fn names() -> [Str; 3] {
        [s("root"), s("vertex"), s("pixel")]
    }
    fn heap(h: u64) -> Option<D3D12_DESCRIPTOR_HEAP_DESC> {
        let kind = if h == 3 { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV };
        (h != 0).then_some(D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: 64, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 })
    }
    const BINDING: Binding = Binding { kind: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, visibility: D3D12_SHADER_VISIBILITY_VERTEX, source_register: 0, private_space: 100 };

    /// The owned arrays an Input points into.
    struct Case {
        bytes: Vec<u8>,
        uses: Vec<ResourceUse>,
        bindings: Vec<Location>,
        pairs: Vec<Pair>,
        shared: Vec<Shared>,
        offsets: Vec<u32>,
        intervals: Vec<Interval>,
        selections: Vec<Selection>,
        tables: Vec<Table>,
        selectors: Selectors,
        dynamic: bool,
    }
    impl Case {
        fn new() -> Box<Case> {
            let mut c = Box::new(Case {
                bytes: serialized(),
                uses: vec![
                    ResourceUse { kind: 2, visibility: 1, space: 0, shader_register: 0, count: 1, bytes: 64 },
                    ResourceUse { kind: 0, visibility: 5, space: 0, shader_register: 2, count: 2, bytes: 0 },
                    ResourceUse { kind: 0, visibility: 5, space: 1, shader_register: 0, count: 16, bytes: 0 },
                    ResourceUse { kind: 3, visibility: 5, space: 0, shader_register: 0, count: 1, bytes: 0 },
                ],
                bindings: vec![Location { binding: BINDING, source_parameter: 0, source_type: D3D12_ROOT_PARAMETER_TYPE_CBV, ..Default::default() }],
                pairs: vec![Pair { binding: BINDING, source: [0, 0], bytes: [64, 64] }],
                shared: vec![
                    Shared { parameter: 1, offset: 0, count: 8, leased: 1, heap: 1 },
                    Shared { parameter: 2, offset: 0, count: 32, leased: 1, heap: 2 },
                ],
                offsets: vec![1, 3],
                intervals: vec![Interval { offset: 1, count: 1, kind: 0 }, Interval { offset: 3, count: 1, kind: 0 }],
                selections: vec![
                    Selection { site_index: 0, source_dword: 0, shader_index: 1, descriptor_offset: 1 },
                    Selection { site_index: 1, source_dword: 0, shader_index: 3, descriptor_offset: 3 },
                ],
                tables: Vec::new(),
                selectors: Selectors {
                    family: names(),
                    evaluated_sites: 2,
                    suppressed: 0,
                    complete: true,
                    permission: false,
                    permitted: false,
                    selections: core::ptr::null(),
                    selection_count: 0,
                    tables: core::ptr::null(),
                    table_count: 0,
                },
                dynamic: true,
            });
            c.link();
            c
        }
        /// Points the table and the selectors at the owned arrays (after any edit of them).
        fn link(&mut self) {
            self.tables = vec![Table {
                root: s("root"),
                visibility: 5,
                parameter: 2,
                space: 1,
                shader_register_base: 0,
                original_table_offset: 0,
                declared_extent: 16,
                offsets: self.offsets.as_ptr(),
                offset_count: self.offsets.len(),
                count: 4,
                uses: self.intervals.as_ptr(),
                use_count: self.intervals.len(),
            }];
            self.selectors.selections = self.selections.as_ptr();
            self.selectors.selection_count = self.selections.len();
            self.selectors.tables = self.tables.as_ptr();
            self.selectors.table_count = self.tables.len();
        }
        fn input(&self) -> Input {
            let eye = |e: u32| Eye {
                scope: Scope { root: ROOT, list: 1, generation: 2, occurrence: 3, source: 4, eye: e },
                root: ROOT,
                side: e,
                owner: 9,
                selectors: if self.dynamic { &self.selectors } else { core::ptr::null() },
            };
            Input {
                source_root: ROOT,
                uses: self.uses.as_ptr(),
                use_count: self.uses.len(),
                bindings: self.bindings.as_ptr(),
                binding_count: self.bindings.len(),
                serialized: self.bytes.as_ptr(),
                serialized_len: self.bytes.len(),
                pairs: self.pairs.as_ptr(),
                pair_count: self.pairs.len(),
                shared: self.shared.as_ptr(),
                shared_count: self.shared.len(),
                original_parameters: 3,
                family: names(),
                eyes: [eye(0), eye(1)],
            }
        }
        fn run(&self, input: &Input) -> (bool, bool) {
            let mut needs = false;
            // SAFETY: the case's arrays outlive the call.
            let covered = unsafe { coverage(input, &heap, &mut needs) };
            (covered, needs)
        }
        fn covered(&self) -> bool {
            self.run(&self.input()).0
        }
    }

    #[test]
    fn admitted_layout() {
        let c = Case::new();
        assert_eq!(c.run(&c.input()), (true, true));
        // Only the paired CBV and the static sampler: no shared read, no contract.
        let mut only = Case::new();
        only.uses = vec![only.uses[0], only.uses[3]];
        only.shared.clear();
        only.dynamic = false;
        assert_eq!(only.run(&only.input()), (true, false));
    }
    #[test]
    fn pairs_and_tables() {
        let mut c = Case::new();
        c.pairs[0].bytes[1] = 32;
        assert!(!c.covered(), "a pair's bytes shorter than the use");
        let mut c = Case::new();
        c.pairs[0].source[0] = 7;
        assert!(!c.covered(), "a CBV pair with an SRV source");
        let mut c = Case::new();
        c.pairs.push(c.pairs[0]);
        c.bindings.push(c.bindings[0]);
        assert!(!c.covered(), "a binding paired twice");
        let mut c = Case::new();
        c.shared[1].parameter = 1;
        assert!(!c.covered(), "two tables on one parameter");
        let mut c = Case::new();
        c.shared[0].heap = 3;
        assert!(!c.covered(), "an SRV read from a sampler heap");
        let mut c = Case::new();
        c.shared[0].leased = 0;
        assert!(!c.covered(), "a table without leases");
        let mut c = Case::new();
        c.shared[0].count = 3;
        assert!(!c.covered(), "a finite use beyond the table");
        let mut c = Case::new();
        c.uses.remove(1);
        assert!(!c.covered(), "a shared table nobody reads");
        let c = Case::new();
        let mut input = c.input();
        input.eyes[1].root = ROOT + 1;
        assert!(!c.run(&input).0, "an eye drawn with another root");
        input = c.input();
        input.original_parameters = 1;
        assert!(!c.run(&input).0, "a table outside the original parameters");
    }
    #[test]
    fn selector_results() {
        let c = Case::new();
        let mut input = c.input();
        input.eyes[0].selectors = core::ptr::null();
        assert!(!c.run(&input).0, "an unbounded use without an eye's selectors");
        input = c.input();
        input.eyes[1].owner = 0;
        assert!(!c.run(&input).0, "an eye without its GPU input owner");
        let mut c = Case::new();
        c.selectors.permitted = true;
        assert!(!c.covered(), "a result granting native execution");
        let mut c = Case::new();
        c.selections[1].descriptor_offset = 2;
        c.link();
        assert!(!c.covered(), "a selection off its table's offsets");
        let mut c = Case::new();
        c.intervals[1].offset = 2;
        c.link();
        assert!(!c.covered(), "intervals unlike the offsets");
        let mut c = Case::new();
        c.offsets = vec![3, 1];
        c.intervals = vec![Interval { offset: 3, count: 1, kind: 0 }, Interval { offset: 1, count: 1, kind: 0 }];
        c.link();
        assert!(!c.covered(), "unsorted offsets");
        let mut c = Case::new();
        c.selections.pop();
        c.selectors.evaluated_sites = 1;
        c.link();
        assert!(!c.covered(), "an offset no selection lands on");
        let mut c = Case::new();
        c.selections[1].site_index = 0;
        c.link();
        assert!(!c.covered(), "one site selected twice");
        let mut c = Case::new();
        c.selectors.family[2] = s("other");
        assert!(!c.covered(), "another family's result");
    }
}
