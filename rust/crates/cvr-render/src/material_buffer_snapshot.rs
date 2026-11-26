//! The material buffer snapshot's rules (`include/Render/NativeMaterialBufferSnapshot.hpp`,
//! `material_buffer_snapshot::detail`): the same native recording (a nonzero token, the list, generation, context and
//! registry); the same buffer view (every SRV buffer field, never the union's padding); the original material t4 / t6
//! binding at the source root parameter 23 (the resource, no counter, the canonical descriptor equal to its stamp and to
//! the binding's, decoded as an explicit buffer view equal to the original); and the private view of a copied slice (raw
//! t4 in 4-byte elements, structured t6 in 64-byte ones, the whole original view at an aligned offset).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions forward to `cvr_mbs_*`; the bindings are mirrored here (the
//! COM pointers read as addresses). `tools/single_pass_tests/material_buffer_snapshot_test` runs its cases against them
//! in its Rust variant.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::descriptor_views::{decode_srv, Value};
use crate::source_cbv_binding::DescriptorStamp;
use crate::source_cbv_snapshot::RecordingLease;

/// source_srv::RootParameter.
pub const ROOT_PARAMETER: u32 = 23;

/// descriptors::SrvBinding (the resource as an address).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SrvBinding {
    pub description: Value,
    pub resource: u64,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub stamp: DescriptorStamp,
}
/// descriptors::SourceResourceBinding (the resources as addresses).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SourceResourceBinding {
    pub parameter: u32,
    pub offset: u32,
    pub kind: u32,
    pub stamp: DescriptorStamp,
    pub resource: u64,
    pub counter: u64,
}
const _: () = assert!(
    core::mem::size_of::<D3D12_SHADER_RESOURCE_VIEW_DESC>() == 40
        && core::mem::size_of::<SrvBinding>() == 384
        && core::mem::offset_of!(SrvBinding, view) == 168
        && core::mem::offset_of!(SrvBinding, stamp) == 208
        && core::mem::size_of::<SourceResourceBinding>() == 208
        && core::mem::offset_of!(SourceResourceBinding, stamp) == 16
        && core::mem::offset_of!(SourceResourceBinding, resource) == 192
);

/// detail::SameRecording.
pub fn same_recording(a: &RecordingLease, b: &RecordingLease) -> bool {
    a.token != 0 && a.token == b.token && a.list == b.list && a.generation == b.generation && a.context == b.context && a.registry == b.registry
}
/// detail::SameBufferView.
pub fn same_buffer_view(a: &D3D12_SHADER_RESOURCE_VIEW_DESC, b: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    if a.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || b.ViewDimension != a.ViewDimension
        || a.Format != b.Format
        || a.Shader4ComponentMapping != b.Shader4ComponentMapping
    {
        return false;
    }
    // SAFETY: both views are buffer views; the union's buffer member is plain data.
    let (x, y) = unsafe { (a.Anonymous.Buffer, b.Anonymous.Buffer) };
    x.FirstElement == y.FirstElement && x.NumElements == y.NumElements && x.StructureByteStride == y.StructureByteStride && x.Flags == y.Flags
}
/// detail::SourceBinding: the original binding's register and the private table's binding.
pub fn source_binding(original: &SrvBinding, shader_register: u32, binding: &SourceResourceBinding) -> bool {
    if !((shader_register == 4 || shader_register == 6)
        && original.resource != 0
        && binding.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32
        && binding.parameter == ROOT_PARAMETER
        && binding.offset == shader_register
        && binding.resource == original.resource
        && binding.counter == 0
        && original.description == original.stamp.value
        && binding.stamp.value == original.stamp.value
        && binding.stamp.heap == original.stamp.heap
        && binding.stamp.gpu == original.stamp.gpu)
    {
        return false;
    }
    match decode_srv(&binding.stamp.value, None) {
        Some(decoded) => decoded.resource_cookie != 0 && !decoded.implicit && same_buffer_view(&decoded.view, &original.view),
        None => false,
    }
}
/// detail::PrivateView: the original view at the copied slice's first element.
pub fn private_view(original: &D3D12_SHADER_RESOURCE_VIEW_DESC, reg: u32, offset: u64, bytes: u64) -> Option<D3D12_SHADER_RESOURCE_VIEW_DESC> {
    let stride: u64 = if reg == 4 { 4 } else { 64 };
    // SAFETY: the union's buffer member is plain data; it is used only for a buffer view.
    let mut buffer = unsafe { original.Anonymous.Buffer };
    if (reg != 4 && reg != 6)
        || original.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || original.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || bytes == 0
        || bytes != buffer.NumElements as u64 * stride
        || offset % stride != 0
    {
        return None;
    }
    if reg == 4 {
        if original.Format != DXGI_FORMAT_R32_TYPELESS || buffer.Flags != D3D12_BUFFER_SRV_FLAG_RAW || buffer.StructureByteStride != 0 {
            return None;
        }
    } else if original.Format != DXGI_FORMAT_UNKNOWN || buffer.Flags != D3D12_BUFFER_SRV_FLAG_NONE || buffer.StructureByteStride != 64 {
        return None;
    }
    let mut result = *original;
    buffer.FirstElement = offset / stride;
    result.Anonymous.Buffer = buffer;
    Some(result)
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// SameRecording.
///
/// # Safety
/// `a` and `b` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbs_same_recording(a: *const RecordingLease, b: *const RecordingLease) -> bool {
    unsafe { same_recording(&*a, &*b) }
}
/// SameBufferView.
///
/// # Safety
/// `a` and `b` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbs_same_buffer_view(a: *const D3D12_SHADER_RESOURCE_VIEW_DESC, b: *const D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    unsafe { same_buffer_view(&*a, &*b) }
}
/// SourceBinding: the backing's original binding and register.
///
/// # Safety
/// `original` and `binding` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbs_source_binding(original: *const SrvBinding, shader_register: u32, binding: *const SourceResourceBinding) -> bool {
    unsafe { source_binding(&*original, shader_register, &*binding) }
}
/// PrivateView (`result` zeroed, written on success).
///
/// # Safety
/// `original` is valid; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mbs_private_view(
    original: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    reg: u32,
    offset: u64,
    bytes: u64,
    result: *mut D3D12_SHADER_RESOURCE_VIEW_DESC,
) -> bool {
    let view = private_view(unsafe { &*original }, reg, offset, bytes);
    unsafe { *result = view.unwrap_or_default() };
    view.is_some()
}
