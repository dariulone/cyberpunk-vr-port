//! A root signature's table layout as the descriptor audit records it (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::RootCreated`): from the version 1.1 description, every descriptor table range in parameter order with
//! its type, space, base register, count, absolute table offset (appended ranges follow the previous one) and the
//! parameter's visibility; at most 64 parameters and 256 ranges; a root constant / descriptor parameter, an empty range
//! or an offset past 32 bits marks the layout unsupported with its reason bits (1 too many parameters, 2 a non-table
//! parameter, 4 a bad range).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the root registry (audit_roots) parses each created root with it;
//! `tools/single_pass_tests/descriptor_audit_test` runs its cases against it in its Rust variant.

use windows::Win32::Graphics::Direct3D12::*;

use crate::source_layout::Binding;

pub const MAX_RANGES: usize = 256;
const MAX_PARAMETERS: u32 = 64;

/// The parsed layout.
#[derive(Clone, Debug, Default)]
pub struct RootLayout {
    pub ranges: Vec<Binding>,
    pub supported: bool,
    pub parameters: u32,
    pub reasons: u32,
    pub flags: u32,
}

/// RootCreated's parse of the version 1.1 description.
///
/// # Safety
/// The description's parameter and range arrays are valid for their counts.
pub unsafe fn root_layout(desc: &D3D12_ROOT_SIGNATURE_DESC1) -> RootLayout {
    let mut result = RootLayout { parameters: desc.NumParameters, flags: desc.Flags.0 as u32, supported: desc.NumParameters <= MAX_PARAMETERS, ..RootLayout::default() };
    if !result.supported {
        result.reasons |= 1;
    }
    for i in 0..desc.NumParameters.min(MAX_PARAMETERS) {
        let p = unsafe { &*desc.pParameters.add(i as usize) };
        if p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            result.supported = false;
            result.reasons |= 2;
            continue;
        }
        let table = unsafe { p.Anonymous.DescriptorTable };
        let mut offset = 0u64;
        for j in 0..table.NumDescriptorRanges {
            let r = unsafe { &*table.pDescriptorRanges.add(j as usize) };
            if r.OffsetInDescriptorsFromTableStart != D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND {
                offset = r.OffsetInDescriptorsFromTableStart as u64;
            }
            if offset > u32::MAX as u64 || r.NumDescriptors == 0 || result.ranges.len() >= MAX_RANGES {
                result.supported = false;
                result.reasons |= 4;
                break;
            }
            result.ranges.push(Binding {
                parameter: i,
                kind: r.RangeType.0 as u32,
                space: r.RegisterSpace,
                shader_register: r.BaseShaderRegister,
                count: r.NumDescriptors,
                table_offset: offset as u32,
                visibility: p.ShaderVisibility.0 as u32,
                ..Binding::default()
            });
            offset += r.NumDescriptors as u64;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    //! The parse's append, non-table and capacity cases (descriptor_audit_test's roots hold one range per table).
    use super::*;

    fn range(kind: D3D12_DESCRIPTOR_RANGE_TYPE, count: u32, register: u32, offset: u32) -> D3D12_DESCRIPTOR_RANGE1 {
        D3D12_DESCRIPTOR_RANGE1 {
            RangeType: kind,
            NumDescriptors: count,
            BaseShaderRegister: register,
            RegisterSpace: 0,
            Flags: D3D12_DESCRIPTOR_RANGE_FLAG_NONE,
            OffsetInDescriptorsFromTableStart: offset,
        }
    }
    fn table(ranges: &[D3D12_DESCRIPTOR_RANGE1]) -> D3D12_ROOT_PARAMETER1 {
        D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: ranges.len() as u32, pDescriptorRanges: ranges.as_ptr() },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        }
    }
    fn desc(parameters: &[D3D12_ROOT_PARAMETER1]) -> D3D12_ROOT_SIGNATURE_DESC1 {
        D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: parameters.len() as u32, pParameters: parameters.as_ptr(), ..Default::default() }
    }

    #[test]
    fn appended_ranges_follow_the_previous_one() {
        let append = D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND;
        let ranges = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 4, 0, 2), range(D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 3, 1, append), range(D3D12_DESCRIPTOR_RANGE_TYPE_UAV, 1, 0, append)];
        let parameters = [table(&ranges)];
        let layout = unsafe { root_layout(&desc(&parameters)) };
        assert!(layout.supported && layout.reasons == 0);
        assert_eq!(layout.ranges.iter().map(|r| r.table_offset).collect::<Vec<_>>(), [2, 6, 9]);
        assert_eq!(layout.ranges.iter().map(|r| r.kind).collect::<Vec<_>>(), [0, 2, 1]);
        assert_eq!(layout.ranges[1].visibility, D3D12_SHADER_VISIBILITY_PIXEL.0 as u32);
    }
    #[test]
    fn unsupported_parameters_and_ranges() {
        let ranges = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, 0, 0)];
        let mut constants = table(&ranges);
        constants.ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
        let parameters = [constants, table(&ranges)];
        let layout = unsafe { root_layout(&desc(&parameters)) };
        assert!(!layout.supported && layout.reasons == 2 && layout.ranges.len() == 1 && layout.ranges[0].parameter == 1);
        let empty = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 0, 0, 0), range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, 1, 1)];
        let parameters = [table(&empty)];
        let layout = unsafe { root_layout(&desc(&parameters)) };
        assert!(!layout.supported && layout.reasons == 4 && layout.ranges.is_empty(), "an empty range ends its table");
        let past = [range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, u32::MAX - 1, 0, 2), range(D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 1, 0, D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND)];
        let parameters = [table(&past)];
        let layout = unsafe { root_layout(&desc(&parameters)) };
        assert!(!layout.supported && layout.reasons == 4 && layout.ranges.len() == 1, "an appended offset past 32 bits");
    }
}