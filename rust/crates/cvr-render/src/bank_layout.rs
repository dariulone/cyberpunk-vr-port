//! The stereo binding bank's root layout rules (src/Render/StereoBindingBank.cpp): a native SRV's component mapping
//! (the always-set bit and four selectors of at most 5), whether a descriptor table parameter fits a table of `count`
//! descriptors of its kind (every range's absolute offset inside it, appended ranges after the previous, an unbounded
//! range only as the last), a CBV-only table's per-descriptor shader registers (space 0, non-empty, at most 65536
//! descriptors, no slot twice), and a source pass root's layout from its serialized bytes: at most 1 MiB, version 1.1,
//! at most 64 table parameters, no local or directly indexed heaps, each table one heap type with non-empty ranges
//! inside its descriptor limit (2048 samplers, 1000000 resources) and their registers inside 32 bits, an unbounded
//! range only as the final one and not aliasing the finite prefix; its rejection names the stage, the parameter and
//! the reason bit.
//!
//! The bank (`binding_bank`) applies them; `tools/single_pass_tests/binding_bank_test` runs its cases against them in its
//! Rust variant.

use core::ffi::c_void;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D12::*;

const DESCRIPTOR_LIMIT: u32 = 1_000_000;
const SAMPLER_LIMIT: u32 = 2048;

/// SourcePassFailureStage::RootLayout.
const STAGE_ROOT_LAYOUT: u32 = 4;
/// SourcePassFailureReason bits.
mod reason {
    pub const BLOB_EXTENT: u64 = 1 << 6;
    pub const DECODER_CREATE: u64 = 1 << 9;
    pub const DECODER_VERSION: u64 = 1 << 10;
    pub const ROOT_PARAMETERS: u64 = 1 << 11;
    pub const ROOT_FLAGS: u64 = 1 << 12;
    pub const ROOT_PARAMETER_KIND: u64 = 1 << 13;
    pub const ROOT_RANGES: u64 = 1 << 14;
    pub const RANGE_TYPE: u64 = 1 << 15;
    pub const RANGE_COUNT: u64 = 1 << 16;
    pub const RANGE_ORDER: u64 = 1 << 17;
    pub const RANGE_ALIAS: u64 = 1 << 18;
    pub const RANGE_EXTENT: u64 = 1 << 19;
    pub const RANGE_REGISTERS: u64 = 1 << 20;
}

/// SourcePassFailure (its default the .cpp's: no input, no parameter).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePassFailure {
    pub stage: u32,
    pub input: u32,
    pub parameter: u32,
    pub bits: u64,
}
impl Default for SourcePassFailure {
    fn default() -> Self {
        SourcePassFailure { stage: 0, input: u32::MAX, parameter: u32::MAX, bits: 0 }
    }
}
/// The .cpp's SourceTableLayout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceTableLayout {
    pub count: u32,
    pub kind: i32,
    pub unbounded: bool,
    pub tail_offset: u32,
    pub tail_register: u32,
}
/// The .cpp's SourceLayout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLayout {
    pub parameters: u32,
    pub tables: [SourceTableLayout; 64],
}
impl Default for SourceLayout {
    fn default() -> Self {
        SourceLayout { parameters: 0, tables: [SourceTableLayout::default(); 64] }
    }
}
const _: () = assert!(
    core::mem::size_of::<SourcePassFailure>() == 24
        && core::mem::size_of::<SourceTableLayout>() == 20
        && core::mem::size_of::<SourceLayout>() == 1284
);

/// ComponentMapping: the always-set bit, nothing above it, four selectors of at most 5.
pub fn component_mapping(mapping: u32) -> bool {
    mapping & 0x1000 != 0 && mapping & !0x1fff == 0 && (0..4).all(|i| (mapping >> (i * 3)) & 7 <= 5)
}

fn ranges(p: &D3D12_ROOT_PARAMETER1) -> &[D3D12_DESCRIPTOR_RANGE1] {
    // SAFETY: a descriptor table parameter (checked by every caller) with its ranges.
    unsafe {
        let table = &p.Anonymous.DescriptorTable;
        if table.NumDescriptorRanges == 0 || table.pDescriptorRanges.is_null() {
            &[]
        } else {
            core::slice::from_raw_parts(table.pDescriptorRanges, table.NumDescriptorRanges as usize)
        }
    }
}
fn parameters(root: &D3D12_ROOT_SIGNATURE_DESC1) -> &[D3D12_ROOT_PARAMETER1] {
    if root.NumParameters == 0 || root.pParameters.is_null() {
        &[]
    } else {
        // SAFETY: the description's parameters.
        unsafe { core::slice::from_raw_parts(root.pParameters, root.NumParameters as usize) }
    }
}

/// TableFits: the parameter is a non-empty table of `sampler` ranges inside `count` descriptors.
pub fn table_fits(root: &D3D12_ROOT_SIGNATURE_DESC1, parameter: u32, count: u32, sampler: bool) -> bool {
    let Some(p) = parameters(root).get(parameter as usize) else {
        return false;
    };
    if p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
        return false;
    }
    let list = ranges(p);
    if list.is_empty() {
        return false;
    }
    let mut next = 0u64;
    for r in list {
        if (r.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) != sampler {
            return false;
        }
        let offset = if r.OffsetInDescriptorsFromTableStart == u32::MAX { next } else { r.OffsetInDescriptorsFromTableStart as u64 };
        if offset >= count as u64 || (r.NumDescriptors != u32::MAX && r.NumDescriptors as u64 > count as u64 - offset) {
            return false;
        }
        next = if r.NumDescriptors == u32::MAX { u32::MAX as u64 + 1 } else { offset + r.NumDescriptors as u64 };
    }
    true
}

/// CbvRegisters: each table descriptor's shader register (u32::MAX where none), extending `registers`.
pub fn cbv_registers(p: &D3D12_ROOT_PARAMETER1, registers: &mut Vec<u32>) -> bool {
    if p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
        return false;
    }
    let list = ranges(p);
    if list.is_empty() {
        return false;
    }
    let mut next = 0u32;
    for range in list {
        let offset = if range.OffsetInDescriptorsFromTableStart == u32::MAX { next } else { range.OffsetInDescriptorsFromTableStart };
        if range.RangeType != D3D12_DESCRIPTOR_RANGE_TYPE_CBV
            || range.RegisterSpace != 0
            || range.NumDescriptors == 0
            || offset > 65536
            || range.NumDescriptors > 65536 - offset
            || range.BaseShaderRegister > u32::MAX - range.NumDescriptors
        {
            return false;
        }
        next = offset + range.NumDescriptors;
        if registers.len() < next as usize {
            registers.resize(next as usize, u32::MAX);
        }
        for n in 0..range.NumDescriptors {
            let slot = &mut registers[(offset + n) as usize];
            if *slot != u32::MAX {
                return false;
            }
            *slot = range.BaseShaderRegister + n;
        }
    }
    !registers.is_empty()
}

/// ReadSourceLayout: the source pass root's table layouts, or the rejection (stage, parameter, reason bit).
pub fn read_source_layout(bytes: &[u8], diagnostic: &mut SourcePassFailure) -> Result<SourceLayout, HRESULT> {
    fn fail(diagnostic: &mut SourcePassFailure, bit: u64, result: HRESULT) -> Result<SourceLayout, HRESULT> {
        diagnostic.bits = bit;
        Err(result)
    }
    macro_rules! reject {
        ($bit:expr, $result:expr) => {
            fail(diagnostic, $bit, $result)
        };
    }
    diagnostic.stage = STAGE_ROOT_LAYOUT;
    diagnostic.parameter = u32::MAX;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return reject!(reason::BLOB_EXTENT, E_INVALIDARG);
    }
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: the caller's bytes; an out-interface (one reference).
    if let Err(e) = unsafe {
        D3D12CreateVersionedRootSignatureDeserializer(bytes.as_ptr() as *const c_void, bytes.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw)
    } {
        return reject!(reason::DECODER_CREATE, e.code());
    }
    if raw.is_null() {
        return reject!(reason::DECODER_CREATE, E_INVALIDARG);
    }
    // SAFETY: the reference the creation added.
    let decoder = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
    // SAFETY: the deserializer's own description, alive with it.
    let decoded = match unsafe { decoder.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) } {
        Ok(d) => d,
        Err(e) => return reject!(reason::DECODER_VERSION, e.code()),
    };
    // SAFETY: a 1.1 description, as asked for.
    let root = unsafe { &(*decoded).Anonymous.Desc_1_1 };
    if root.NumParameters > 64 {
        return reject!(reason::ROOT_PARAMETERS, E_INVALIDARG);
    }
    let forbidden = D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE
        | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED
        | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
    if (root.Flags & forbidden).0 != 0 {
        return reject!(reason::ROOT_FLAGS, E_INVALIDARG);
    }
    let mut pending = SourceLayout { parameters: root.NumParameters, ..SourceLayout::default() };
    for (parameter, table) in parameters(root).iter().enumerate() {
        diagnostic.parameter = parameter as u32;
        if table.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            return reject!(reason::ROOT_PARAMETER_KIND, E_INVALIDARG);
        }
        let list = ranges(table);
        if list.is_empty() {
            return reject!(reason::ROOT_RANGES, E_INVALIDARG);
        }
        let sampler = list[0].RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        let limit = if sampler { SAMPLER_LIMIT } else { DESCRIPTOR_LIMIT } as u64;
        let (mut next, mut extent) = (0u64, 0u64);
        let mut entry = SourceTableLayout {
            kind: if sampler { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER.0 } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 },
            ..SourceTableLayout::default()
        };
        for (index, r) in list.iter().enumerate() {
            if r.RangeType.0 as u32 > D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 || (r.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) != sampler {
                return reject!(reason::RANGE_TYPE, E_INVALIDARG);
            }
            if r.NumDescriptors == 0 {
                return reject!(reason::RANGE_COUNT, E_INVALIDARG);
            }
            let offset = if r.OffsetInDescriptorsFromTableStart == D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND { next } else { r.OffsetInDescriptorsFromTableStart as u64 };
            if r.NumDescriptors == u32::MAX {
                // No meaningful APPEND position follows an unbounded range; a final tail also cannot alias its finite
                // prefix.
                if index + 1 != list.len() {
                    return reject!(reason::RANGE_ORDER, E_INVALIDARG);
                }
                if extent > offset {
                    return reject!(reason::RANGE_ALIAS, E_INVALIDARG);
                }
                entry.unbounded = true;
                entry.tail_offset = offset as u32;
                entry.tail_register = r.BaseShaderRegister;
                extent = offset + 1;
                break;
            }
            if offset >= limit || r.NumDescriptors as u64 > limit - offset {
                return reject!(reason::RANGE_EXTENT, E_INVALIDARG);
            }
            if r.BaseShaderRegister as u64 + r.NumDescriptors as u64 > u32::MAX as u64 + 1 {
                return reject!(reason::RANGE_REGISTERS, E_INVALIDARG);
            }
            next = offset + r.NumDescriptors as u64;
            extent = extent.max(next);
        }
        entry.count = extent as u32;
        pending.tables[parameter] = entry;
    }
    Ok(pending)
}

#[cfg(test)]
mod tests {
    //! Cases derived from the .cpp (binding_bank_test runs the deserializer path with real roots).
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
    const APPEND: u32 = D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND;

    #[test]
    fn component_mappings() {
        assert!(component_mapping(D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING));
        assert!(!component_mapping(0x688), "the always-set bit");
        assert!(!component_mapping(0x3688), "a bit above it");
        assert!(!component_mapping(0x1006), "a selector of 6");
    }
    #[test]
    fn tables_fit_their_count() {
        let srvs = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 2, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 2, 0, APPEND)];
        let parameters = [table(&srvs)];
        let root = D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: 1, pParameters: parameters.as_ptr(), ..Default::default() };
        assert!(table_fits(&root, 0, 4, false));
        assert!(!table_fits(&root, 0, 3, false), "the appended range ends past the count");
        assert!(!table_fits(&root, 0, 4, true), "not a sampler table");
        assert!(!table_fits(&root, 1, 4, false), "no such parameter");
    }
    #[test]
    fn cbv_registers_per_descriptor() {
        let cbvs = [range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 2, 4, 1), range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 1, 9, APPEND)];
        let mut registers = Vec::new();
        assert!(cbv_registers(&table(&cbvs), &mut registers));
        assert_eq!(registers, [u32::MAX, 4, 5, 9]);
        let twice = [range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 2, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 1, 7, 1)];
        assert!(!cbv_registers(&table(&twice), &mut Vec::new()), "a slot twice");
        let srv = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, 0, 0)];
        assert!(!cbv_registers(&table(&srv), &mut Vec::new()));
    }
}
