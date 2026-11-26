//! The source descriptor table layouts (src/Render/StereoDescriptorAudit.cpp, `descriptors`): a descriptor range type's
//! kind (CBV 1, SRV 2, UAV 3, sampler 4), a recorded SRV's exact explicit view (every buffer / Texture2D /
//! Texture2DArray field, the LOD clamp by bits), a canonical descriptor's validity for its kind, and the two capture
//! layouts of one root table parameter from the root signature's ranges and the draw's certified uses -- the whole table
//! (every slot's kind, the used ones marked, unknown slots defaulting to SRV or sampler) and the compact selected slots
//! (at most 4096, in offset order); both refuse mixed sampler / view tables, overlapping ranges of different kinds,
//! uncovered extents, unbounded tails past the table and uses outside a range of their kind.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's DescriptorKind, SourceSrvView, SourceDescriptorValid, SourceLayout and
//! SourceSlotLayout forward to `cvr_sl_*`. `tools/single_pass_tests/descriptor_audit_test` runs its cases against them in
//! its Rust variant.

use std::collections::BTreeMap;

use windows::Win32::Graphics::Direct3D12::*;

use crate::descriptor_views::{decode_cbv, decode_sampler, decode_srv, decode_uav, Value};

pub const SLOT_CAPTURE_LIMIT: usize = 4096;
const TABLE_LIMIT: u32 = 1_000_000;
const SAMPLER_LIMIT: u32 = 2048;
/// D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV / SAMPLER.
const HEAP_VIEWS: u32 = 0;
const HEAP_SAMPLERS: u32 = 1;

/// descriptors::Binding (a root signature range).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Binding {
    pub parameter: u32,
    pub kind: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
    pub table_offset: u32,
    pub value_offset: u32,
    pub value_count: u32,
    pub gpu: u64,
    pub heap: u64,
    pub version: u64,
    pub status: u32,
    pub visibility: u32,
}
/// descriptors::SourceSlotUse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotUse {
    pub offset: u32,
    pub count: u32,
    pub kind: u32,
}
/// One slot of the whole-table layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotPlan {
    pub kind: u32,
    pub used: bool,
}
const _: () = assert!(core::mem::size_of::<Binding>() == 64 && core::mem::size_of::<SlotUse>() == 12 && core::mem::size_of::<SlotPlan>() == 8);

/// DescriptorKind.
pub fn descriptor_kind(kind: u32) -> u32 {
    match kind as i32 {
        k if k == D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 => 1,
        k if k == D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 => 2,
        k if k == D3D12_DESCRIPTOR_RANGE_TYPE_UAV.0 => 3,
        k if k == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 => 4,
        _ => 0,
    }
}
/// SourceSrvView: the view equals the recorded explicit one field by field.
pub fn source_srv_view(value: &Value, view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    let Some(decoded) = decode_srv(value, None).filter(|decoded| !decoded.implicit) else { return false };
    let expected = &decoded.view;
    if view.Format != expected.Format || view.ViewDimension != expected.ViewDimension || view.Shader4ComponentMapping != expected.Shader4ComponentMapping {
        return false;
    }
    // SAFETY: the shared view dimension selects the union member of both.
    unsafe {
        if view.ViewDimension == D3D12_SRV_DIMENSION_BUFFER {
            let (a, b) = (view.Anonymous.Buffer, expected.Anonymous.Buffer);
            return a.FirstElement == b.FirstElement && a.NumElements == b.NumElements && a.StructureByteStride == b.StructureByteStride && a.Flags == b.Flags;
        }
        if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2D {
            let (a, b) = (view.Anonymous.Texture2D, expected.Anonymous.Texture2D);
            return a.MostDetailedMip == b.MostDetailedMip
                && a.MipLevels == b.MipLevels
                && a.PlaneSlice == b.PlaneSlice
                && a.ResourceMinLODClamp.to_bits() == b.ResourceMinLODClamp.to_bits();
        }
        if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2DARRAY {
            let (a, b) = (view.Anonymous.Texture2DArray, expected.Anonymous.Texture2DArray);
            return a.MostDetailedMip == b.MostDetailedMip
                && a.MipLevels == b.MipLevels
                && a.FirstArraySlice == b.FirstArraySlice
                && a.ArraySize == b.ArraySize
                && a.PlaneSlice == b.PlaneSlice
                && a.ResourceMinLODClamp.to_bits() == b.ResourceMinLODClamp.to_bits();
        }
    }
    false
}
/// SourceDescriptorValid.
pub fn source_descriptor_valid(value: &Value, kind: u32) -> bool {
    if value[0] != kind as u64 {
        return false;
    }
    match kind {
        1 => decode_cbv(value).is_some(),
        2 => decode_srv(value, None).is_some(),
        3 => decode_uav(value).is_some(),
        _ => decode_sampler(value).is_some(),
    }
}
/// The ranges of one parameter: (start, end, kind) with the table's sampler class and whether it ends in an unbounded
/// tail; None when they cannot form the table.
fn parameter_ranges(ranges: &[Binding], parameter: u32, count: u32, disjoint_kinds: bool) -> Option<(Vec<(u64, u64, u32)>, bool)> {
    let (mut found, mut sampler, mut tail) = (false, false, false);
    let mut extent = 0u64;
    let mut intervals: Vec<(u64, u64, u32)> = Vec::new();
    for range in ranges.iter().filter(|range| range.parameter == parameter) {
        let kind = descriptor_kind(range.kind);
        if kind == 0 || tail {
            return None;
        }
        let is_sampler = kind == 4;
        if found && sampler != is_sampler {
            return None;
        }
        found = true;
        sampler = is_sampler;
        let start = range.table_offset as u64;
        let mut length = range.count as u64;
        if range.count == u32::MAX {
            tail = true;
            if start >= count as u64 {
                return None;
            }
            length = count as u64 - start;
        }
        if start > count as u64 || length > count as u64 - start || range.shader_register as u64 + length > u32::MAX as u64 + 1 {
            return None;
        }
        if disjoint_kinds && intervals.iter().any(|&(s, e, k)| start < e && s < start + length && k != kind) {
            return None;
        }
        intervals.push((start, start + length, kind));
        extent = extent.max(start + length);
    }
    if !found || (!tail && extent != count as u64) || (sampler && count > SAMPLER_LIMIT) {
        return None;
    }
    Some((intervals, sampler))
}
/// SourceLayout: the whole table's slots and its heap type.
pub fn source_layout(ranges: &[Binding], parameters: u32, parameter: u32, count: u32, uses: &[SlotUse]) -> Option<(u32, Vec<SlotPlan>)> {
    if parameter >= parameters || count == 0 || count > TABLE_LIMIT || uses.is_empty() {
        return None;
    }
    // The C++ checks and assigns in one pass; any refusal is the same refusal (the caller drops a refused layout).
    let (intervals, sampler) = parameter_ranges(ranges, parameter, count, false)?;
    let mut slots = vec![SlotPlan::default(); count as usize];
    for (start, end, kind) in intervals {
        for slot in &mut slots[start as usize..end as usize] {
            if slot.kind != 0 && slot.kind != kind {
                return None;
            }
            slot.kind = kind;
        }
    }
    for usage in uses {
        let kind = descriptor_kind(usage.kind);
        if kind == 0 || usage.count == 0 || usage.offset > count || usage.count > count - usage.offset {
            return None;
        }
        for slot in &mut slots[usage.offset as usize..(usage.offset + usage.count) as usize] {
            if slot.kind != kind {
                return None;
            }
            slot.used = true;
        }
    }
    for slot in &mut slots {
        if slot.kind == 0 {
            slot.kind = if sampler { 4 } else { 2 };
        }
    }
    Some((if sampler { HEAP_SAMPLERS } else { HEAP_VIEWS }, slots))
}
/// SourceSlotLayout: the selected slots (offset, kind) in offset order and the heap type.
pub fn source_slot_layout(ranges: &[Binding], parameters: u32, parameter: u32, count: u32, uses: &[SlotUse]) -> Option<(u32, Vec<(u32, u32)>)> {
    if parameter >= parameters || count == 0 || count > TABLE_LIMIT || uses.is_empty() || uses.len() > SLOT_CAPTURE_LIMIT {
        return None;
    }
    let (intervals, sampler) = parameter_ranges(ranges, parameter, count, true)?;
    let mut selected: BTreeMap<u32, u32> = BTreeMap::new();
    for usage in uses {
        let kind = descriptor_kind(usage.kind);
        if kind == 0 || usage.count == 0 || usage.count as usize > SLOT_CAPTURE_LIMIT || usage.offset > count || usage.count > count - usage.offset {
            return None;
        }
        for offset in usage.offset..usage.offset + usage.count {
            let at = offset as u64;
            if !intervals.iter().any(|&(start, end, k)| at >= start && at < end && k == kind) {
                return None;
            }
            let entry = *selected.entry(offset).or_insert(kind);
            if entry != kind || selected.len() > SLOT_CAPTURE_LIMIT {
                return None;
            }
        }
    }
    Some((if sampler { HEAP_SAMPLERS } else { HEAP_VIEWS }, selected.into_iter().collect()))
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// DescriptorKind.
#[no_mangle]
pub extern "C-unwind" fn cvr_sl_kind(kind: u32) -> u32 {
    descriptor_kind(kind)
}
/// SourceSrvView (`which` 0, `view` used) or SourceDescriptorValid (1, `kind` used).
///
/// # Safety
/// `value` is valid; `view` is valid for SourceSrvView.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sl_descriptor(which: u32, value: *const Value, view: *const D3D12_SHADER_RESOURCE_VIEW_DESC, kind: u32) -> bool {
    unsafe {
        if which == 0 { source_srv_view(&*value, &*view) } else { source_descriptor_valid(&*value, kind) }
    }
}
/// SourceLayout: on success `slots` (room for `count`) and `heap_type` written.
///
/// # Safety
/// The ranges and uses hold their counts; `slots` has room for `count`; `heap_type` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sl_layout(
    ranges: *const Binding,
    range_count: usize,
    parameters: u32,
    parameter: u32,
    count: u32,
    uses: *const SlotUse,
    use_count: usize,
    slots: *mut SlotPlan,
    heap_type: *mut u32,
) -> bool {
    let Some((kind, plan)) = (unsafe { source_layout(slice(ranges, range_count), parameters, parameter, count, slice(uses, use_count)) }) else {
        return false;
    };
    unsafe {
        core::ptr::copy_nonoverlapping(plan.as_ptr(), slots, plan.len());
        *heap_type = kind;
    }
    true
}
/// SourceSlotLayout: on success up to SLOT_CAPTURE_LIMIT `offsets` and `kinds` with their `selected` count, and
/// `heap_type`.
///
/// # Safety
/// The ranges and uses hold their counts; `offsets` and `kinds` have room for SLOT_CAPTURE_LIMIT; the outputs are
/// writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sl_slot_layout(
    ranges: *const Binding,
    range_count: usize,
    parameters: u32,
    parameter: u32,
    count: u32,
    uses: *const SlotUse,
    use_count: usize,
    offsets: *mut u32,
    kinds: *mut u32,
    selected: *mut usize,
    heap_type: *mut u32,
) -> bool {
    let Some((kind, chosen)) = (unsafe { source_slot_layout(slice(ranges, range_count), parameters, parameter, count, slice(uses, use_count)) }) else {
        return false;
    };
    unsafe {
        for (i, (offset, slot_kind)) in chosen.iter().enumerate() {
            *offsets.add(i) = *offset;
            *kinds.add(i) = *slot_kind;
        }
        *selected = chosen.len();
        *heap_type = kind;
    }
    true
}

#[cfg(test)]
mod tests {
    //! The layouts' cases, from the C++ in src/Render/StereoDescriptorAudit.cpp (descriptor_audit_test does not reach
    //! them: they run only inside a live source table capture).
    use super::*;

    const SRV: u32 = 0;
    const UAV: u32 = 1;
    const CBV: u32 = 2;
    const SAMPLER: u32 = 3;
    fn range(parameter: u32, kind: u32, table_offset: u32, count: u32, shader_register: u32) -> Binding {
        Binding { parameter, kind, table_offset, count, shader_register, ..Binding::default() }
    }
    fn uses(list: &[(u32, u32, u32)]) -> Vec<SlotUse> {
        list.iter().map(|&(offset, count, kind)| SlotUse { offset, count, kind }).collect()
    }

    #[test]
    fn kinds() {
        assert_eq!([CBV, SRV, UAV, SAMPLER, 7].map(descriptor_kind), [1, 2, 3, 4, 0]);
    }
    #[test]
    fn whole_table() {
        let ranges = [range(1, SRV, 0, 4, 0), range(1, CBV, 4, 2, 0), range(0, UAV, 0, 9, 0)];
        let (heap, slots) = source_layout(&ranges, 4, 1, 6, &uses(&[(1, 2, SRV)])).unwrap();
        assert_eq!(heap, HEAP_VIEWS);
        assert_eq!(slots.iter().map(|s| s.kind).collect::<Vec<_>>(), [2, 2, 2, 2, 1, 1]);
        assert_eq!(slots.iter().map(|s| s.used).collect::<Vec<_>>(), [false, true, true, false, false, false]);
        // The extent must cover the table, a use must match its slots' kind, the parameter must exist.
        assert!(source_layout(&ranges, 4, 1, 7, &uses(&[(1, 2, SRV)])).is_none());
        assert!(source_layout(&ranges, 4, 1, 6, &uses(&[(3, 2, SRV)])).is_none());
        assert!(source_layout(&ranges, 1, 1, 6, &uses(&[(1, 2, SRV)])).is_none());
        assert!(source_layout(&ranges, 4, 1, 6, &[]).is_none());
        // Overlapping ranges of different kinds conflict; samplers do not mix with views.
        let overlap = [range(1, SRV, 0, 4, 0), range(1, CBV, 3, 3, 0)];
        assert!(source_layout(&overlap, 4, 1, 6, &uses(&[(0, 1, SRV)])).is_none());
        let mixed = [range(1, SRV, 0, 4, 0), range(1, SAMPLER, 4, 2, 0)];
        assert!(source_layout(&mixed, 4, 1, 6, &uses(&[(0, 1, SRV)])).is_none());
        // An unbounded tail covers the rest; nothing may follow it; unused tail slots keep their range's kind.
        let tail = [range(2, SRV, 1, u32::MAX, 0)];
        let (_, slots) = source_layout(&tail, 4, 2, 5, &uses(&[(2, 1, SRV)])).unwrap();
        assert_eq!(slots.iter().map(|s| s.kind).collect::<Vec<_>>(), [2, 2, 2, 2, 2]);
        let after_tail = [range(2, SRV, 1, u32::MAX, 0), range(2, SRV, 0, 1, 0)];
        assert!(source_layout(&after_tail, 4, 2, 5, &uses(&[(2, 1, SRV)])).is_none());
        // A sampler table defaults unknown slots to samplers and is bounded by 2048.
        let samplers = [range(3, SAMPLER, 1, 2, 0)];
        // Only the extent must reach the table's end (as in C++): uncovered slot 0 defaults to a sampler.
        let (_, slots) = source_layout(&samplers, 4, 3, 3, &uses(&[(1, 1, SAMPLER)])).unwrap();
        assert_eq!(slots.iter().map(|s| s.kind).collect::<Vec<_>>(), [4, 4, 4]);
        let samplers = [range(3, SAMPLER, 0, u32::MAX, 0)];
        let (heap, slots) = source_layout(&samplers, 4, 3, 3, &uses(&[(1, 1, SAMPLER)])).unwrap();
        assert_eq!((heap, slots.iter().map(|s| s.kind).collect::<Vec<_>>()), (HEAP_SAMPLERS, vec![4, 4, 4]));
        assert!(source_layout(&samplers, 4, 3, 2049, &uses(&[(1, 1, SAMPLER)])).is_none());
    }
    #[test]
    fn selected_slots() {
        let ranges = [range(1, SRV, 0, 4, 0), range(1, CBV, 4, 2, 0)];
        let (heap, chosen) = source_slot_layout(&ranges, 4, 1, 6, &uses(&[(2, 1, SRV), (4, 1, CBV), (1, 2, SRV)])).unwrap();
        assert_eq!(heap, HEAP_VIEWS);
        assert_eq!(chosen, vec![(1, 2), (2, 2), (4, 1)]);
        assert!(source_slot_layout(&ranges, 4, 1, 6, &uses(&[(4, 1, SRV)])).is_none(), "use outside a range of its kind");
        let overlap = [range(1, SRV, 0, 4, 0), range(1, CBV, 3, 3, 0)];
        assert!(source_slot_layout(&overlap, 4, 1, 6, &uses(&[(0, 1, SRV)])).is_none());
        let same_kind = [range(1, SRV, 0, 4, 0), range(1, SRV, 3, 3, 0)];
        assert!(source_slot_layout(&same_kind, 4, 1, 6, &uses(&[(0, 1, SRV)])).is_some(), "same-kind overlap is allowed");
    }
    #[test]
    fn descriptor_validity() {
        let mut value: Value = [0; 20];
        assert!(!source_descriptor_valid(&value, 2));
        value[0] = 4;
        assert!(!source_descriptor_valid(&value, 2), "a sampler value is not an SRV");
    }
}