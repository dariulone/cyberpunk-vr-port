//! The paired root signature of single pass (formerly src/Render/StereoPairedRoot.cpp; matches
//! tools/single_pass_tests/patch_resource_pairs.py: space-0 register N -> 2*N + SV_ViewID in a private space).
//!
//! Every original parameter index, range, flag and static sampler is preserved; at most one mixed CBV/SRV table is
//! added per stage (vertex, pixel), holding a pair of descriptors per paired binding. No resource is retained or
//! copied: the plan only says where each binding's data lives in the original layout (`source*`) and where its pair
//! goes (`destination*`). The serialized input may be root signature version 1.0 or 1.1.
use core::ffi::c_void;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;

/// PairedBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Binding {
    pub kind: D3D12_DESCRIPTOR_RANGE_TYPE,
    pub visibility: D3D12_SHADER_VISIBILITY,
    pub source_register: u32,
    pub private_space: u32,
}
/// PairedBindingLocation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Location {
    pub binding: Binding,
    pub source_parameter: u32,
    pub source_offset: u32,
    pub destination_parameter: u32,
    pub destination_offset: u32,
    pub source_type: D3D12_ROOT_PARAMETER_TYPE,
}
const _: () = assert!(core::mem::size_of::<Binding>() == 16 && core::mem::size_of::<Location>() == 36);

/// PairedRootPlan.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub serialized: Vec<u8>,
    pub bindings: Vec<Location>,
    /// The added tables' parameter indices (vertex, pixel), u32::MAX for none, and their descriptor counts.
    pub parameters: [u32; 2],
    pub descriptors: [u32; 2],
    pub original_parameters: u32,
    pub root_dwords: u32,
}

fn visible(declared: D3D12_SHADER_VISIBILITY, stage: D3D12_SHADER_VISIBILITY) -> bool {
    declared == D3D12_SHADER_VISIBILITY_ALL || declared == stage
}
fn matches(root: D3D12_ROOT_PARAMETER_TYPE, kind: D3D12_DESCRIPTOR_RANGE_TYPE) -> bool {
    (root == D3D12_ROOT_PARAMETER_TYPE_CBV && kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV)
        || (root == D3D12_ROOT_PARAMETER_TYPE_SRV && kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV)
}
fn overlaps(base: u32, count: u32, target: u32, length: u32) -> bool {
    let end = if count == u32::MAX { u32::MAX as u64 + 1 } else { base as u64 + count as u64 };
    (target as u64) < end && (base as u64) < target as u64 + length as u64
}

/// BuildPairedRoot: the plan for `bytes` (a serialized root signature) and `bindings`; Err leaves nothing behind.
pub fn build(bytes: &[u8], bindings: &[Binding]) -> Result<Plan, HRESULT> {
    if bytes.is_empty() || bytes.len() > 1024 * 1024 || bindings.len() > 512 {
        return Err(E_INVALIDARG);
    }
    // SAFETY: the caller's bytes and length; an out-interface (one reference).
    let reader = unsafe {
        let mut raw: *mut c_void = core::ptr::null_mut();
        D3D12CreateVersionedRootSignatureDeserializer(
            bytes.as_ptr() as *const c_void,
            bytes.len(),
            &ID3D12VersionedRootSignatureDeserializer::IID,
            &mut raw,
        )
        .map_err(|e| e.code())?;
        if raw.is_null() {
            return Err(E_INVALIDARG);
        }
        ID3D12VersionedRootSignatureDeserializer::from_raw(raw)
    };
    // SAFETY: the deserializer's own description, alive with it.
    let version = unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }.map_err(|e| e.code())?;
    // SAFETY: a 1.1 description, as asked for; alive with the deserializer.
    let (version, source) = unsafe { (&*version, &(*version).Anonymous.Desc_1_1) };
    if source.NumParameters > 64 || (source.Flags & D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE).0 != 0 {
        return Err(E_INVALIDARG);
    }
    let original: &[D3D12_ROOT_PARAMETER1] = if source.NumParameters == 0 {
        &[]
    } else {
        // SAFETY: the description's parameter array.
        unsafe { core::slice::from_raw_parts(source.pParameters, source.NumParameters as usize) }
    };
    let mut result = Plan {
        parameters: [u32::MAX; 2],
        original_parameters: source.NumParameters,
        ..Default::default()
    };
    let mut parameters: Vec<D3D12_ROOT_PARAMETER1> = original.to_vec();
    for p in original {
        result.root_dwords += match p.ParameterType {
            D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE => 1,
            D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS => {
                // SAFETY: the constants member of a constants parameter.
                let n = unsafe { p.Anonymous.Constants.Num32BitValues };
                if n > 64 {
                    return Err(E_INVALIDARG);
                }
                n
            }
            D3D12_ROOT_PARAMETER_TYPE_CBV | D3D12_ROOT_PARAMETER_TYPE_SRV | D3D12_ROOT_PARAMETER_TYPE_UAV => 2,
            _ => return Err(E_INVALIDARG),
        };
    }
    let mut ordered = bindings.to_vec();
    ordered.sort_by_key(|b| (b.visibility.0, b.kind.0, b.source_register, b.private_space));
    let mut banks: [Vec<D3D12_DESCRIPTOR_RANGE1>; 2] = [Vec::new(), Vec::new()];
    for (n, b) in ordered.iter().enumerate() {
        if (b.kind != D3D12_DESCRIPTOR_RANGE_TYPE_CBV && b.kind != D3D12_DESCRIPTOR_RANGE_TYPE_SRV)
            || (b.visibility != D3D12_SHADER_VISIBILITY_VERTEX && b.visibility != D3D12_SHADER_VISIBILITY_PIXEL)
            || b.private_space == 0
            || b.private_space >= 65536
            || b.source_register > (u32::MAX - 2) / 2
        {
            return Err(E_INVALIDARG);
        }
        if n > 0 {
            let previous = &ordered[n - 1];
            if previous.visibility == b.visibility && previous.kind == b.kind && previous.source_register == b.source_register {
                return Err(E_INVALIDARG);
            }
        }
        let stage = if b.visibility == D3D12_SHADER_VISIBILITY_VERTEX { 0 } else { 1 };
        let deny = if stage == 1 {
            D3D12_ROOT_SIGNATURE_FLAG_DENY_PIXEL_SHADER_ROOT_ACCESS
        } else {
            D3D12_ROOT_SIGNATURE_FLAG_DENY_VERTEX_SHADER_ROOT_ACCESS
        };
        if (source.Flags & deny).0 != 0 {
            return Err(E_INVALIDARG);
        }
        let mut location = Location {
            binding: *b,
            ..Default::default()
        };
        let mut found = 0;
        for (i, p) in original.iter().enumerate() {
            if !visible(p.ShaderVisibility, b.visibility) {
                continue;
            }
            if p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                // SAFETY: the table member of a table parameter, and its range array.
                let ranges = unsafe {
                    let t = &p.Anonymous.DescriptorTable;
                    if t.NumDescriptorRanges == 0 {
                        &[][..]
                    } else {
                        core::slice::from_raw_parts(t.pDescriptorRanges, t.NumDescriptorRanges as usize)
                    }
                };
                let mut next = 0u64;
                for range in ranges {
                    let offset = if range.OffsetInDescriptorsFromTableStart == D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND {
                        next
                    } else {
                        range.OffsetInDescriptorsFromTableStart as u64
                    };
                    if offset > u32::MAX as u64 {
                        return Err(E_INVALIDARG);
                    }
                    next = if range.NumDescriptors == u32::MAX {
                        u32::MAX as u64 + 1
                    } else {
                        offset + range.NumDescriptors as u64
                    };
                    if range.RangeType != b.kind {
                        continue;
                    }
                    if range.RegisterSpace == b.private_space && overlaps(range.BaseShaderRegister, range.NumDescriptors, b.source_register * 2, 2) {
                        return Err(E_INVALIDARG);
                    }
                    if range.RegisterSpace != 0 || !overlaps(range.BaseShaderRegister, range.NumDescriptors, b.source_register, 1) {
                        continue;
                    }
                    let at = offset + b.source_register as u64 - range.BaseShaderRegister as u64;
                    if at > u32::MAX as u64 {
                        return Err(E_INVALIDARG);
                    }
                    location.source_parameter = i as u32;
                    location.source_offset = at as u32;
                    location.source_type = p.ParameterType;
                    found += 1;
                }
            } else {
                let constants = p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
                if !(matches(p.ParameterType, b.kind) || (constants && b.kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV)) {
                    continue;
                }
                // SAFETY: the member the parameter type names.
                let (space, register) = unsafe {
                    if constants {
                        (p.Anonymous.Constants.RegisterSpace, p.Anonymous.Constants.ShaderRegister)
                    } else {
                        (p.Anonymous.Descriptor.RegisterSpace, p.Anonymous.Descriptor.ShaderRegister)
                    }
                };
                if space == b.private_space && overlaps(register, 1, b.source_register * 2, 2) {
                    return Err(E_INVALIDARG);
                }
                if space != 0 || register != b.source_register {
                    continue;
                }
                if constants {
                    return Err(E_NOTIMPL); // no CBV data layout exists for root constants here
                }
                location.source_parameter = i as u32;
                location.source_offset = 0;
                location.source_type = p.ParameterType;
                found += 1;
            }
        }
        if found != 1 {
            return Err(E_INVALIDARG);
        }
        if result.parameters[stage] == u32::MAX {
            result.parameters[stage] = parameters.len() as u32;
            parameters.push(D3D12_ROOT_PARAMETER1::default());
            result.root_dwords += 1;
        }
        location.destination_parameter = result.parameters[stage];
        location.destination_offset = result.descriptors[stage];
        banks[stage].push(D3D12_DESCRIPTOR_RANGE1 {
            RangeType: b.kind,
            NumDescriptors: 2,
            BaseShaderRegister: b.source_register * 2,
            RegisterSpace: b.private_space,
            Flags: D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE | D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE,
            OffsetInDescriptorsFromTableStart: result.descriptors[stage],
        });
        result.descriptors[stage] += 2;
        result.bindings.push(location);
    }
    if result.root_dwords > 64 {
        return Err(E_INVALIDARG);
    }
    for stage in 0..2 {
        if result.parameters[stage] != u32::MAX {
            let p = &mut parameters[result.parameters[stage] as usize];
            p.ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
            p.ShaderVisibility = if stage == 1 {
                D3D12_SHADER_VISIBILITY_PIXEL
            } else {
                D3D12_SHADER_VISIBILITY_VERTEX
            };
            p.Anonymous.DescriptorTable = D3D12_ROOT_DESCRIPTOR_TABLE1 {
                NumDescriptorRanges: banks[stage].len() as u32,
                pDescriptorRanges: banks[stage].as_ptr(),
            };
        }
    }
    let mut desc = *version;
    desc.Anonymous.Desc_1_1.NumParameters = parameters.len() as u32;
    desc.Anonymous.Desc_1_1.pParameters = parameters.as_ptr();
    let (mut blob, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: a description whose arrays (parameters, banks, the deserializer's ranges and samplers) outlive the call.
    unsafe { D3D12SerializeVersionedRootSignature(&desc, &mut blob, Some(&mut error)) }.map_err(|e| e.code())?;
    let blob = blob.ok_or(E_INVALIDARG)?;
    // SAFETY: the blob's own buffer.
    result.serialized = unsafe { core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }.to_vec();
    Ok(result)
}

// ---- the C ABI (include/Render/StereoPairedRoot.hpp copies the plan out of the handle)

/// Builds the plan into `*plan` (a handle for the accessors below, freed by cvr_paired_root_free); the answer is the
/// HRESULT.
///
/// # Safety
/// `bytes` is `length` readable bytes or null; `bindings` is `count` bindings or null; `plan` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_paired_root_build(bytes: *const u8, length: usize, bindings: *const Binding, count: usize, plan: *mut *mut Plan) -> HRESULT {
    // SAFETY: the caller's buffers.
    let (bytes, bindings) = unsafe {
        (
            if bytes.is_null() {
                &[][..]
            } else {
                core::slice::from_raw_parts(bytes, length)
            },
            if bindings.is_null() || count == 0 {
                &[][..]
            } else {
                core::slice::from_raw_parts(bindings, count)
            },
        )
    };
    match build(bytes, bindings) {
        Ok(p) => {
            // SAFETY: the caller's out-pointer.
            if let Some(out) = unsafe { plan.as_mut() } {
                *out = Box::into_raw(Box::new(p));
            }
            HRESULT(0)
        }
        Err(hr) => hr,
    }
}
/// The plan's parts: serialized bytes, the binding locations, and the scalars.
///
/// # Safety
/// `plan` came from cvr_paired_root_build; the out-pointers are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_paired_root_parts(
    plan: *const Plan,
    serialized: *mut *const u8,
    serialized_len: *mut usize,
    bindings: *mut *const Location,
    bindings_len: *mut usize,
    scalars: *mut [u32; 6],
) {
    // SAFETY: the caller's plan and out-pointers.
    unsafe {
        let Some(p) = plan.as_ref() else { return };
        *serialized = p.serialized.as_ptr();
        *serialized_len = p.serialized.len();
        *bindings = p.bindings.as_ptr();
        *bindings_len = p.bindings.len();
        *scalars = [
            p.parameters[0],
            p.parameters[1],
            p.descriptors[0],
            p.descriptors[1],
            p.original_parameters,
            p.root_dwords,
        ];
    }
}
/// # Safety
/// `plan` came from cvr_paired_root_build (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_paired_root_free(plan: *mut Plan) {
    if !plan.is_null() {
        // SAFETY: the caller's plan, freed once.
        drop(unsafe { Box::from_raw(plan) });
    }
}
