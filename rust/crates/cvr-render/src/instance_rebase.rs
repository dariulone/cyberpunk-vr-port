//! THE INSTANCE REBASE ADMISSION (include/Render/InstanceRebase.hpp): splitting a batch changes the draw-local instance
//! ordinal, so a vertex shader is admitted only with one parsed input signature (ISG1 or ISGN) that has no
//! SV_InstanceID -- and for rebasing native IA streams, per-instance data only in slot 7 at step rate 1, no SM6.8
//! base-instance feature bit (SFI0 bit 32) and a single DXIL vertex shader part of shader model 6.0..6.7. Bytes are
//! read only inside their chunk; a container that would make the C++ rules read outside it is refused. The header
//! forwards to these in the Rust build; tools/single_pass_tests/geometry_commands_test and instance_coverage_test run
//! their cases against them.
use windows::Win32::Graphics::Direct3D::D3D_NAME_INSTANCE_ID;
use windows::Win32::Graphics::Direct3D12::*;

fn u32_at(data: &[u8], at: u64) -> Option<u32> {
    let at = usize::try_from(at).ok()?;
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn tag(data: &[u8], at: u64, name: &[u8; 4]) -> bool {
    usize::try_from(at)
        .ok()
        .and_then(|at| data.get(at..at + 4))
        .is_some_and(|t| t == name)
}

/// HasIndependentInstanceOrdinal.
pub fn has_independent_instance_ordinal(data: &[u8]) -> bool {
    check_ordinal(data).unwrap_or(false)
}
fn check_ordinal(data: &[u8]) -> Option<bool> {
    if data.len() < 32 || &data[..4] != b"DXBC" || u32_at(data, 24)? as usize != data.len() {
        return Some(false);
    }
    let (bytes, chunks) = (u32_at(data, 24)? as u64, u32_at(data, 28)? as u64);
    if chunks > 64 || chunks > (bytes - 32) / 4 {
        return Some(false);
    }
    let mut signature = false;
    for i in 0..chunks {
        let at = u32_at(data, 32 + i * 4)? as u64;
        if at < 32 + chunks * 4 || at > bytes - 8 {
            return Some(false);
        }
        let length = u32_at(data, at + 4)? as u64;
        if length > bytes - at - 8 {
            return Some(false);
        }
        let modern = tag(data, at, b"ISG1");
        let legacy = tag(data, at, b"ISGN");
        if !modern && !legacy {
            continue;
        }
        if signature || length < 8 {
            return Some(false);
        }
        signature = true;
        let count = u32_at(data, at + 8)? as u64;
        let stride = if modern { 32 } else { 24 };
        if count > 32 || count > (length - 8) / stride {
            return Some(false);
        }
        for j in 0..count {
            let entry = at + 16 + j * stride + if modern { 4 } else { 0 };
            let name = u32_at(data, entry)? as u64;
            if name >= length {
                return Some(false);
            }
            let start = (at + 8 + name) as usize;
            let end = start + (length - name) as usize;
            if !data.get(start..end)?.contains(&0) {
                return Some(false);
            }
            if u32_at(data, entry + 8)? == D3D_NAME_INSTANCE_ID.0 as u32 {
                return Some(false);
            }
        }
    }
    Some(signature)
}

/// SupportsInstanceRebase.
///
/// # Safety
/// The layout's element array holds its count.
pub unsafe fn supports_instance_rebase(data: &[u8], layout: &D3D12_INPUT_LAYOUT_DESC) -> bool {
    if data.len() < 32 || layout.NumElements > 32 || layout.pInputElementDescs.is_null() {
        return false;
    }
    let elements = unsafe {
        core::slice::from_raw_parts(layout.pInputElementDescs, layout.NumElements as usize)
    };
    let mut instance = false;
    for e in elements {
        if e.InputSlotClass == D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA {
            if e.InputSlot != 7 || e.InstanceDataStepRate != 1 {
                return false;
            }
            instance = true;
        } else if e.InputSlot == 7 {
            return false;
        }
    }
    instance && check_rebase(data).unwrap_or(false)
}
fn check_rebase(data: &[u8]) -> Option<bool> {
    if &data[..4] != b"DXBC" {
        return Some(false);
    }
    let (bytes, count) = (u32_at(data, 24)? as u64, u32_at(data, 28)? as u64);
    if bytes != data.len() as u64 || count > (bytes - 32) / 4 || count > 64 {
        return Some(false);
    }
    let mut vertex = false;
    for i in 0..count {
        let at = u32_at(data, 32 + 4 * i)? as u64;
        if at < 32 + 4 * count || at > bytes - 8 {
            return Some(false);
        }
        let size = u32_at(data, at + 4)? as u64;
        if size > bytes - at - 8 {
            return Some(false);
        }
        if tag(data, at, b"SFI0") {
            if size != 8 {
                return Some(false);
            }
            let low = u32_at(data, at + 8)? as u64;
            let high = u32_at(data, at + 12)? as u64;
            if (low | high << 32) & (1u64 << 32) != 0 {
                return Some(false);
            }
        }
        if tag(data, at, b"DXIL") {
            if vertex || size < 4 {
                return Some(false);
            }
            let version = u32_at(data, at + 8)?;
            if version >> 16 != 1 || (version & 0xFFFF) < 0x60 || (version & 0xFFFF) > 0x67 {
                return Some(false);
            }
            vertex = true;
        }
    }
    Some(vertex)
}

unsafe fn bytecode<'a>(shader: &D3D12_SHADER_BYTECODE) -> Option<&'a [u8]> {
    (!shader.pShaderBytecode.is_null() && shader.BytecodeLength >= 32).then(|| unsafe {
        core::slice::from_raw_parts(shader.pShaderBytecode as *const u8, shader.BytecodeLength)
    })
}

/// HasIndependentInstanceOrdinal.
///
/// # Safety
/// `shader` holds its bytecode.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ir_independent_ordinal(
    shader: *const D3D12_SHADER_BYTECODE,
) -> bool {
    unsafe { bytecode(&*shader).is_some_and(has_independent_instance_ordinal) }
}

/// SupportsInstanceRebase.
///
/// # Safety
/// `shader` holds its bytecode; `layout` is valid with its elements.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ir_supports_rebase(
    shader: *const D3D12_SHADER_BYTECODE,
    layout: *const D3D12_INPUT_LAYOUT_DESC,
) -> bool {
    unsafe { bytecode(&*shader).is_some_and(|data| supports_instance_rebase(data, &*layout)) }
}
