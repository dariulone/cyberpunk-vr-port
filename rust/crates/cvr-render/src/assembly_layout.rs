//! The source binding assembly's root and view rules (src/Render/SourceBindingAssembly.cpp): the per-parameter table
//! layouts of an original root (1..64 table parameters, no local or directly indexed heaps, at most 4096 ranges, each
//! table one heap type with non-empty, non-overlapping ranges inside its limit -- 2048 samplers, 1000000 resources --
//! and registers inside 32 bits, an unbounded range only last and not below the finite prefix), a table extent's
//! admission for its layout (exactly its finite extent, or at least it for an unbounded tail whose registers still fit),
//! the range holding one slot, a CBV view's agreement with its resource (256-aligned, at most 64 KiB, covering the
//! reflected bytes, inside the buffer at the given offset; a private one on a default heap), the exact structured-128
//! view of the multilayer table and SRV buffer view equality.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's Layouts, Extent, SlotRange, CbvRange, ExactStructured and SameSrv
//! forward to `cvr_sba_*`; `tools/single_pass_tests/source_binding_assembly_test` runs its cases against them in its
//! Rust variant.

use core::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

const RESOURCE_LIMIT: u32 = 1_000_000;
const SAMPLER_LIMIT: u32 = 2048;

/// The .cpp's Range.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub offset: u32,
    pub count: u32,
    pub desc: D3D12_DESCRIPTOR_RANGE1,
}
/// The .cpp's Layout without its ranges.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutHead {
    pub minimum: u32,
    pub tail: bool,
    pub sampler: bool,
}
const _: () = assert!(core::mem::size_of::<Range>() == 32 && core::mem::size_of::<LayoutHead>() == 8);

/// A layout: its head and its ranges.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub head: LayoutHead,
    pub ranges: Vec<Range>,
}

/// Layouts: every table parameter's layout, or None.
pub fn layouts(root: &D3D12_ROOT_SIGNATURE_DESC1) -> Option<Vec<Layout>> {
    let forbidden = D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE
        | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED
        | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
    if root.NumParameters == 0 || root.NumParameters > 64 || root.pParameters.is_null() || (root.Flags & forbidden).0 != 0 {
        return None;
    }
    // SAFETY: the description's parameters.
    let parameters = unsafe { core::slice::from_raw_parts(root.pParameters, root.NumParameters as usize) };
    let mut result = Vec::with_capacity(parameters.len());
    let mut total = 0u32;
    for parameter in parameters {
        if parameter.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            return None;
        }
        // SAFETY: a descriptor table parameter.
        let table = unsafe { parameter.Anonymous.DescriptorTable };
        if table.NumDescriptorRanges == 0 || table.pDescriptorRanges.is_null() || table.NumDescriptorRanges > 4096 - total {
            return None;
        }
        total += table.NumDescriptorRanges;
        // SAFETY: the table's ranges.
        let ranges = unsafe { core::slice::from_raw_parts(table.pDescriptorRanges, table.NumDescriptorRanges as usize) };
        let mut layout = Layout::default();
        layout.head.sampler = ranges[0].RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        let limit = if layout.head.sampler { SAMPLER_LIMIT } else { RESOURCE_LIMIT } as u64;
        let mut next = 0u64;
        for (n, range) in ranges.iter().enumerate() {
            if range.RangeType.0 as u32 > D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32
                || range.NumDescriptors == 0
                || layout.head.tail
                || (range.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) != layout.head.sampler
            {
                return None;
            }
            let offset = if range.OffsetInDescriptorsFromTableStart == u32::MAX { next } else { range.OffsetInDescriptorsFromTableStart as u64 };
            let tail = range.NumDescriptors == u32::MAX;
            let end = if tail { u32::MAX as u64 + 1 } else { offset + range.NumDescriptors as u64 };
            if offset >= limit
                || (!tail && end > limit)
                || (!tail && range.BaseShaderRegister as u64 + range.NumDescriptors as u64 > u32::MAX as u64 + 1)
                || (tail && (n + 1 != ranges.len() || offset < layout.head.minimum as u64))
            {
                return None;
            }
            if layout.ranges.iter().any(|prior| offset < prior.offset as u64 + prior.count as u64 && (prior.offset as u64) < end) {
                return None;
            }
            layout.ranges.push(Range { offset: offset as u32, count: range.NumDescriptors, desc: *range });
            layout.head.tail = tail;
            layout.head.minimum = layout.head.minimum.max(if tail { offset as u32 + 1 } else { end as u32 });
            next = end;
        }
        result.push(layout);
    }
    Some(result)
}

/// Extent: `count` descriptors admitted for the layout.
pub fn extent(head: &LayoutHead, last: Option<&Range>, count: u32) -> bool {
    let limit = if head.sampler { SAMPLER_LIMIT } else { RESOURCE_LIMIT };
    if count == 0 || count > limit || (if head.tail { count < head.minimum } else { count != head.minimum }) {
        return false;
    }
    !head.tail || last.is_some_and(|r| r.desc.BaseShaderRegister as u64 + count as u64 - r.offset as u64 <= u32::MAX as u64 + 1)
}

/// SlotRange: the index of the range holding the slot.
pub fn slot_range(ranges: &[Range], offset: u32) -> Option<usize> {
    ranges.iter().position(|r| offset >= r.offset && ((offset - r.offset) as u64) < r.count as u64)
}

/// CbvRange: the CBV view lies in its resource at `offset` and covers the reflected bytes.
pub fn cbv_range(resource: *mut c_void, view: &D3D12_CONSTANT_BUFFER_VIEW_DESC, reflected: u32, offset: u64, private_buffer: bool) -> bool {
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    if reflected == 0
        || reflected > 65536
        || view.SizeInBytes == 0
        || view.SizeInBytes > 65536
        || view.SizeInBytes & 255 != 0
        || reflected > view.SizeInBytes
        || view.BufferLocation == 0
        || view.BufferLocation & 255 != 0
    {
        return false;
    }
    // SAFETY (whole block): a live resource.
    unsafe {
        let desc = r.GetDesc();
        let gpu = r.GetGPUVirtualAddress();
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || gpu == 0
            || gpu > u64::MAX - desc.Width
            || offset > desc.Width
            || view.SizeInBytes as u64 > desc.Width - offset
            || view.BufferLocation != gpu + offset
        {
            return false;
        }
        if private_buffer {
            let mut heap = D3D12_HEAP_PROPERTIES::default();
            let mut flags = D3D12_HEAP_FLAGS::default();
            if r.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err() || heap.Type != D3D12_HEAP_TYPE_DEFAULT {
                return false;
            }
        }
    }
    true
}

/// ExactStructured: the multilayer table's structured-128 view of 8192 elements.
pub fn exact_structured(view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    if view.Format != DXGI_FORMAT_UNKNOWN || view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING {
        return false;
    }
    // SAFETY: a buffer view.
    let b = unsafe { view.Anonymous.Buffer };
    b.FirstElement == 0 && b.NumElements == 8192 && b.StructureByteStride == 128 && b.Flags.0 == 0
}

/// SameSrv: equal buffer views (the buffer member compared whatever the dimension, as the .cpp does).
pub fn same_srv(a: &D3D12_SHADER_RESOURCE_VIEW_DESC, b: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    // SAFETY: the buffer member of the union, read as plain data.
    let (x, y) = unsafe { (a.Anonymous.Buffer, b.Anonymous.Buffer) };
    a.Format == b.Format
        && a.ViewDimension == b.ViewDimension
        && a.Shader4ComponentMapping == b.Shader4ComponentMapping
        && x.FirstElement == y.FirstElement
        && x.NumElements == y.NumElements
        && x.StructureByteStride == y.StructureByteStride
        && x.Flags == y.Flags
}

// ---- the C ABI

/// Layouts: false, or every parameter's head in `heads` (64) and its ranges through `push`.
///
/// # Safety
/// `root` is a valid version 1.1 description; `heads` has room for 64; `push` accepts `layouts` and the ranges.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_layouts(
    root: *const D3D12_ROOT_SIGNATURE_DESC1,
    heads: *mut LayoutHead,
    layouts: *mut c_void,
    push: unsafe extern "C-unwind" fn(*mut c_void, u32, *const Range, usize),
) -> bool {
    let Some(result) = self::layouts(unsafe { &*root }) else {
        return false;
    };
    for (p, layout) in result.iter().enumerate() {
        unsafe {
            *heads.add(p) = layout.head;
            push(layouts, p as u32, layout.ranges.as_ptr(), layout.ranges.len());
        }
    }
    true
}
/// # Safety
/// `head` is valid; `last` is valid or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_extent(head: *const LayoutHead, last: *const Range, count: u32) -> bool {
    extent(unsafe { &*head }, unsafe { last.as_ref() }, count)
}
/// The index of the slot's range, or -1.
///
/// # Safety
/// `ranges` holds `count` ranges (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_slot_range(ranges: *const Range, count: usize, offset: u32) -> isize {
    let ranges = if ranges.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(ranges, count) } };
    slot_range(ranges, offset).map_or(-1, |i| i as isize)
}
/// # Safety
/// `view` is valid; the resource is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_cbv_range(resource: *mut c_void, view: *const D3D12_CONSTANT_BUFFER_VIEW_DESC, reflected: u32, offset: u64, private_buffer: bool) -> bool {
    cbv_range(resource, unsafe { &*view }, reflected, offset, private_buffer)
}
/// ExactStructured (`b` null) or SameSrv.
///
/// # Safety
/// `a` is valid; `b` is valid or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sba_srv(a: *const D3D12_SHADER_RESOURCE_VIEW_DESC, b: *const D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    match unsafe { b.as_ref() } {
        None => exact_structured(unsafe { &*a }),
        Some(b) => same_srv(unsafe { &*a }, b),
    }
}

#[cfg(test)]
mod tests {
    //! Cases derived from the .cpp.
    use super::*;

    fn range(kind: D3D12_DESCRIPTOR_RANGE_TYPE, count: u32, register: u32, offset: u32) -> D3D12_DESCRIPTOR_RANGE1 {
        D3D12_DESCRIPTOR_RANGE1 { RangeType: kind, NumDescriptors: count, BaseShaderRegister: register, RegisterSpace: 0, Flags: D3D12_DESCRIPTOR_RANGE_FLAG_NONE, OffsetInDescriptorsFromTableStart: offset }
    }
    fn table(ranges: &[D3D12_DESCRIPTOR_RANGE1]) -> D3D12_ROOT_PARAMETER1 {
        D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: ranges.len() as u32, pDescriptorRanges: ranges.as_ptr() } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        }
    }
    fn root(parameters: &[D3D12_ROOT_PARAMETER1]) -> D3D12_ROOT_SIGNATURE_DESC1 {
        D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: parameters.len() as u32, pParameters: parameters.as_ptr(), ..Default::default() }
    }
    const APPEND: u32 = D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND;

    #[test]
    fn layouts_and_extents() {
        let srvs = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 4, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, u32::MAX, 4, APPEND)];
        let cbvs = [range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 2, 0, 1)];
        let parameters = [table(&srvs), table(&cbvs)];
        let layouts = layouts(&root(&parameters)).expect("valid root");
        assert_eq!(layouts[0].head, LayoutHead { minimum: 5, tail: true, sampler: false });
        assert_eq!(layouts[1].head, LayoutHead { minimum: 3, tail: false, sampler: false });
        assert!(extent(&layouts[0].head, layouts[0].ranges.last(), 9) && !extent(&layouts[0].head, layouts[0].ranges.last(), 4));
        assert!(extent(&layouts[1].head, layouts[1].ranges.last(), 3) && !extent(&layouts[1].head, layouts[1].ranges.last(), 4));
        assert_eq!(slot_range(&layouts[1].ranges, 0), None);
        assert_eq!(slot_range(&layouts[1].ranges, 2), Some(0));
        assert_eq!(slot_range(&layouts[0].ranges, 100), Some(1));
    }
    #[test]
    fn rejected_layouts() {
        let overlap = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 4, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 1, 0, 3)];
        assert!(layouts(&root(&[table(&overlap)])).is_none(), "overlapping ranges");
        let below = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 4, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, u32::MAX, 0, 2)];
        assert!(layouts(&root(&[table(&below)])).is_none(), "a tail below the finite prefix");
        let mixed = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, 1, 0, APPEND)];
        assert!(layouts(&root(&[table(&mixed)])).is_none(), "a mixed heap type");
        assert!(layouts(&root(&[])).is_none(), "no parameters");
    }
}
