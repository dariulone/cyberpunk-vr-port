//! The sight's root signature dump (src/Stereo/DeviceHooks.cpp, `rootsig_dump` after the blob is found): the
//! signature's serialized bytes read back at version 1.1 and logged as the .cpp's printf lines -- its parameter and
//! static sampler counts and flags, then each parameter: root constants (register, space, count), a root CBV, SRV or
//! UAV (register, space), or a descriptor table, its ranges as `t<base>+<count>/sp<space>` (b, t, u, s by range type)
//! until the line passes 340 characters; with the shader visibility of each. A blob that does not deserialize, or has
//! no 1.1 view, is said so.
//!
//! In the Rust build `rootsig_dump` forwards to `cvr_dh_rootsig_dump`, which logs the lines through the stereo log;
//! the unit tests below serialize signatures and read their lines.
use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// MSVC's `%p`: sixteen upper-case hex digits.
fn pointer(p: usize) -> String {
    format!("{p:016X}")
}
fn range_letter(kind: D3D12_DESCRIPTOR_RANGE_TYPE) -> &'static str {
    match kind {
        D3D12_DESCRIPTOR_RANGE_TYPE_CBV => "b",
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV => "t",
        D3D12_DESCRIPTOR_RANGE_TYPE_UAV => "u",
        _ => "s",
    }
}
/// The lines of a 1.1 description.
///
/// # Safety
/// `d` is a valid description: its parameters, ranges and samplers alive.
pub unsafe fn desc_lines(rs: usize, d: &D3D12_ROOT_SIGNATURE_DESC1) -> Vec<String> {
    let mut lines = vec![format!(
        "[rootsig] sight signature {}: {} params, {} samplers, flags=0x{:X}",
        pointer(rs),
        d.NumParameters,
        d.NumStaticSamplers,
        d.Flags.0 as u32
    )];
    let params = if d.pParameters.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(d.pParameters, d.NumParameters as usize) } };
    for (i, p) in params.iter().enumerate() {
        let vis = p.ShaderVisibility.0 as u32;
        // SAFETY: the union member the parameter type selects.
        let line = unsafe {
            match p.ParameterType {
                D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS => {
                    let c = &p.Anonymous.Constants;
                    format!("[rootsig]  [{i:2}] CONSTANTS b{} space{} x{}  vis={vis}", c.ShaderRegister, c.RegisterSpace, c.Num32BitValues)
                }
                D3D12_ROOT_PARAMETER_TYPE_CBV | D3D12_ROOT_PARAMETER_TYPE_SRV | D3D12_ROOT_PARAMETER_TYPE_UAV => {
                    let (name, letter) = match p.ParameterType {
                        D3D12_ROOT_PARAMETER_TYPE_CBV => ("CBV", "b"),
                        D3D12_ROOT_PARAMETER_TYPE_SRV => ("SRV", "t"),
                        _ => ("UAV", "u"),
                    };
                    let r = &p.Anonymous.Descriptor;
                    format!("[rootsig]  [{i:2}] {name}       {letter}{} space{}  vis={vis}", r.ShaderRegister, r.RegisterSpace)
                }
                D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE => {
                    let t = &p.Anonymous.DescriptorTable;
                    let ranges = if t.pDescriptorRanges.is_null() {
                        &[][..]
                    } else {
                        core::slice::from_raw_parts(t.pDescriptorRanges, t.NumDescriptorRanges as usize)
                    };
                    let mut r = String::new();
                    for rg in ranges {
                        if r.len() >= 340 {
                            break;
                        }
                        r += &format!("{}{}+{}/sp{} ", range_letter(rg.RangeType), rg.BaseShaderRegister, rg.NumDescriptors, rg.RegisterSpace);
                    }
                    format!("[rootsig]  [{i:2}] TABLE     vis={vis}  {r}")
                }
                other => format!("[rootsig]  [{i:2}] type={}", other.0 as u32),
            }
        };
        lines.push(line);
    }
    lines
}
/// The lines for a signature's serialized bytes.
pub fn blob_lines(rs: usize, blob: &[u8]) -> Vec<String> {
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: the signature's bytes; an out-interface.
    let created = unsafe {
        D3D12CreateVersionedRootSignatureDeserializer(blob.as_ptr() as *const c_void, blob.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw)
    };
    if created.is_err() || raw.is_null() {
        return vec![format!("[rootsig] sight signature {}: deserialize failed ({} bytes)", pointer(rs), blob.len())];
    }
    // SAFETY: the reference the creation added.
    let reader = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
    // SAFETY: a live deserializer; the description lives with it.
    match unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) } {
        Ok(v) if !v.is_null() => unsafe { desc_lines(rs, &(*v).Anonymous.Desc_1_1) },
        _ => vec![format!("[rootsig] sight signature {}: no 1.1 view", pointer(rs))],
    }
}

// ---- the C ABI (the Rust build: the stereo log is the DLL's)

#[cfg(feature = "rust-build")]
mod abi {
    use core::ffi::c_char;
    extern "C" {
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    /// # Safety
    /// `blob` holds `size` bytes (or is null with 0).
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_dh_rootsig_dump(rs: *const core::ffi::c_void, blob: *const u8, size: usize) {
        let bytes = if blob.is_null() || size == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(blob, size) } };
        for line in super::blob_lines(rs as usize, bytes) {
            let text = std::ffi::CString::new(line).unwrap_or_default();
            // SAFETY: a C string for the C++ half's log.
            unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    //! Serialized signatures read back.
    use super::*;
    use windows::Win32::Graphics::Direct3D::ID3DBlob;

    fn serialize(params: &[D3D12_ROOT_PARAMETER1], flags: D3D12_ROOT_SIGNATURE_FLAGS) -> Vec<u8> {
        let desc = D3D12_VERSIONED_ROOT_SIGNATURE_DESC {
            Version: D3D_ROOT_SIGNATURE_VERSION_1_1,
            Anonymous: D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0 {
                Desc_1_1: D3D12_ROOT_SIGNATURE_DESC1 {
                    NumParameters: params.len() as u32,
                    pParameters: params.as_ptr(),
                    NumStaticSamplers: 0,
                    pStaticSamplers: core::ptr::null(),
                    Flags: flags,
                },
            },
        };
        let mut blob: Option<ID3DBlob> = None;
        unsafe { D3D12SerializeVersionedRootSignature(&desc, &mut blob, None) }.unwrap();
        let blob = blob.unwrap();
        unsafe { core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }.to_vec()
    }
    #[test]
    fn lines() {
        let ranges = [
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 4, BaseShaderRegister: 2, RegisterSpace: 1, ..Default::default() },
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, NumDescriptors: u32::MAX, BaseShaderRegister: 0, RegisterSpace: 0, ..Default::default() },
        ];
        let params = [
            D3D12_ROOT_PARAMETER1 {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER1_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 3, RegisterSpace: 0, Num32BitValues: 8 } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER1 {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER1_0 { Descriptor: D3D12_ROOT_DESCRIPTOR1 { ShaderRegister: 5, RegisterSpace: 2, ..Default::default() } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER1 {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER1_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 2, pDescriptorRanges: ranges.as_ptr() },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
            },
        ];
        let blob = serialize(&params, D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT);
        assert_eq!(blob_lines(0x1A2B, &blob), [
            "[rootsig] sight signature 0000000000001A2B: 3 params, 0 samplers, flags=0x1",
            "[rootsig]  [ 0] CONSTANTS b3 space0 x8  vis=5",
            "[rootsig]  [ 1] SRV       t5 space2  vis=0",
            "[rootsig]  [ 2] TABLE     vis=1  t2+4/sp1 b0+4294967295/sp0 ",
        ]);
        assert_eq!(blob_lines(0x10, &[1, 2, 3]), ["[rootsig] sight signature 0000000000000010: deserialize failed (3 bytes)"]);
    }
    #[test]
    fn long_tables() {
        // Ranges are written until the line passes 340 characters.
        let ranges: Vec<D3D12_DESCRIPTOR_RANGE1> = (0..40)
            .map(|k| D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 1, BaseShaderRegister: 1000 + k, RegisterSpace: 0, ..Default::default() })
            .collect();
        let params = [D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 40, pDescriptorRanges: ranges.as_ptr() } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        }];
        let lines = blob_lines(1, &serialize(&params, D3D12_ROOT_SIGNATURE_FLAG_NONE));
        let table = lines[1].split("  ").last().unwrap();
        // Each range is "u1000+1/sp0 " (12 characters): 29 of them reach 348.
        assert_eq!(table.len(), 29 * 12);
    }
}
