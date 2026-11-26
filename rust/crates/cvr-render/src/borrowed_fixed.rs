//! The borrowed fixed range's descriptor rules (src/Render/StereoDescriptorAudit.cpp, `descriptors::borrowed_fixed_detail`,
//! declared in include/Render/StereoDescriptorAudit.hpp): a canonical descriptor's class (unknown; an explicit typed
//! null Texture2D / Texture2DArray SRV with a valid mip / slice / clamp range; a resource candidate whose recorded shape,
//! when described, is a flagless Texture2D; or unsupported descriptor / view / shape), the resource / view format
//! compatibility (a typeless family and its members), and one explicit view's agreement with an already held resource
//! (immutable Texture2D shape, typed compatible format, default mapping, the recorded creation description when present,
//! the plane, array and mip ranges).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's Classify and ViewMatches forward to `cvr_bfd_*`.
//! `tools/single_pass_tests/descriptor_audit_test` runs its cases against them in its Rust variant.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::descriptor_views::{decode_srv, typed_format, Value};
use crate::texture_read::immutable_shape;

const MIP_LEVELS: u32 = D3D12_REQ_MIP_LEVELS;
const ARRAY_AXIS: u32 = D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION;

/// borrowed_fixed_detail::DescriptorClass.
pub mod class {
    pub const UNKNOWN: u32 = 0;
    pub const TYPED_NULL: u32 = 1;
    pub const CANDIDATE: u32 = 2;
    pub const UNSUPPORTED_DESCRIPTOR: u32 = 3;
    pub const UNSUPPORTED_VIEW: u32 = 4;
    pub const UNSUPPORTED_SHAPE: u32 = 5;
}

/// The (most detailed mip, mip levels, plane, minimum LOD clamp) of a Texture2D / Texture2DArray view.
fn mip_range(view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> (u32, u32, u32, f32) {
    // SAFETY: the view dimension selects the union member.
    unsafe {
        if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2D {
            let v = view.Anonymous.Texture2D;
            (v.MostDetailedMip, v.MipLevels, v.PlaneSlice, v.ResourceMinLODClamp)
        } else {
            let v = view.Anonymous.Texture2DArray;
            (v.MostDetailedMip, v.MipLevels, v.PlaneSlice, v.ResourceMinLODClamp)
        }
    }
}
/// Classify: the class, and the decoded view for a typed null or a candidate.
pub fn classify(value: &Value) -> (u32, D3D12_SHADER_RESOURCE_VIEW_DESC) {
    let none = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    if value[0] == 0 {
        return (class::UNKNOWN, none);
    }
    if value[0] != 2 {
        return (class::UNSUPPORTED_DESCRIPTOR, none);
    }
    if value[1] & 2 != 0 || (value[5] != D3D12_SRV_DIMENSION_TEXTURE2D.0 as u64 && value[5] != D3D12_SRV_DIMENSION_TEXTURE2DARRAY.0 as u64) {
        return (class::UNSUPPORTED_VIEW, none);
    }
    let Some(decoded) = decode_srv(value, None).filter(|decoded| !decoded.implicit) else {
        return (class::UNSUPPORTED_DESCRIPTOR, none);
    };
    if decoded.resource_cookie == 0 {
        if value[1] != 0 || value[2] != 0 || value[3] != 0 {
            return (class::UNSUPPORTED_DESCRIPTOR, none);
        }
        if decoded.view.ViewDimension != D3D12_SRV_DIMENSION_TEXTURE2D {
            // SAFETY: a Texture2DArray view.
            let array = unsafe { decoded.view.Anonymous.Texture2DArray };
            if array.ArraySize == 0 || array.ArraySize == u32::MAX || array.FirstArraySlice >= ARRAY_AXIS || array.ArraySize > ARRAY_AXIS - array.FirstArraySlice {
                return (class::UNSUPPORTED_DESCRIPTOR, none);
            }
        }
        let (first, mips, plane, clamp) = mip_range(&decoded.view);
        if mips == 0
            || first >= MIP_LEVELS
            || (mips != u32::MAX && mips > MIP_LEVELS - first)
            || plane != 0
            || !clamp.is_finite()
            || clamp < 0.0
            || clamp > (MIP_LEVELS - 1) as f32
        {
            return (class::UNSUPPORTED_DESCRIPTOR, none);
        }
        return (class::TYPED_NULL, decoded.view);
    }
    if decoded.metadata.described && (decoded.metadata.dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || decoded.metadata.flags.0 != 0) {
        return (class::UNSUPPORTED_SHAPE, none);
    }
    (class::CANDIDATE, decoded.view)
}
/// Format: a resource format and a view format compatible with it (equal, or a member of its typeless family).
pub fn format(resource: DXGI_FORMAT, view: DXGI_FORMAT) -> bool {
    if resource == view {
        return true;
    }
    let within = |first: DXGI_FORMAT, last: DXGI_FORMAT| view.0 >= first.0 && view.0 <= last.0;
    match resource {
        DXGI_FORMAT_R32G32B32A32_TYPELESS => within(DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32B32A32_SINT),
        DXGI_FORMAT_R32G32B32_TYPELESS => within(DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32_SINT),
        DXGI_FORMAT_R16G16B16A16_TYPELESS => within(DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16B16A16_SINT),
        DXGI_FORMAT_R32G32_TYPELESS => within(DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32_SINT),
        DXGI_FORMAT_R10G10B10A2_TYPELESS => within(DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R10G10B10A2_UINT),
        DXGI_FORMAT_R8G8B8A8_TYPELESS => within(DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_SINT),
        DXGI_FORMAT_R16G16_TYPELESS => within(DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16_SINT),
        DXGI_FORMAT_R32_TYPELESS => within(DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_SINT),
        DXGI_FORMAT_R8G8_TYPELESS => within(DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8G8_SINT),
        DXGI_FORMAT_R16_TYPELESS => view == DXGI_FORMAT_R16_FLOAT || within(DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16_SINT),
        DXGI_FORMAT_R8_TYPELESS => within(DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8_SINT),
        DXGI_FORMAT_BC1_TYPELESS => within(DXGI_FORMAT_BC1_UNORM, DXGI_FORMAT_BC1_UNORM_SRGB),
        DXGI_FORMAT_BC2_TYPELESS => within(DXGI_FORMAT_BC2_UNORM, DXGI_FORMAT_BC2_UNORM_SRGB),
        DXGI_FORMAT_BC3_TYPELESS => within(DXGI_FORMAT_BC3_UNORM, DXGI_FORMAT_BC3_UNORM_SRGB),
        DXGI_FORMAT_BC4_TYPELESS => within(DXGI_FORMAT_BC4_UNORM, DXGI_FORMAT_BC4_SNORM),
        DXGI_FORMAT_BC5_TYPELESS => within(DXGI_FORMAT_BC5_UNORM, DXGI_FORMAT_BC5_SNORM),
        DXGI_FORMAT_BC6H_TYPELESS => within(DXGI_FORMAT_BC6H_UF16, DXGI_FORMAT_BC6H_SF16),
        DXGI_FORMAT_BC7_TYPELESS => within(DXGI_FORMAT_BC7_UNORM, DXGI_FORMAT_BC7_UNORM_SRGB),
        DXGI_FORMAT_B8G8R8A8_TYPELESS => view == DXGI_FORMAT_B8G8R8A8_UNORM || view == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        DXGI_FORMAT_B8G8R8X8_TYPELESS => view == DXGI_FORMAT_B8G8R8X8_UNORM || view == DXGI_FORMAT_B8G8R8X8_UNORM_SRGB,
        _ => false,
    }
}
/// ViewMatches: one explicit view against an already held resource (shape metadata only).
pub fn view_matches(value: &Value, resource: &D3D12_RESOURCE_DESC, view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    if !immutable_shape(resource)
        || resource.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || !format(resource.Format, view.Format)
        || !typed_format(view.Format.0 as u32 as u64)
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || value[1] & 2 != 0
    {
        return false;
    }
    if value[1] & 4 != 0
        && (value[13] != (resource.Dimension.0 as u32 as u64 | (resource.Format.0 as u32 as u64) << 32)
            || value[14] != resource.Width
            || value[15] != (resource.Height as u64 | (resource.DepthOrArraySize as u64) << 32 | (resource.MipLevels as u64) << 48)
            || value[16] != (resource.Flags.0 as u32 as u64 | (resource.Layout.0 as u32 as u64) << 32)
            || value[17] != (resource.SampleDesc.Count as u64 | (resource.SampleDesc.Quality as u64) << 32))
    {
        return false;
    }
    let (first, mips, clamp) = if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2D {
        // SAFETY: a Texture2D view.
        let v = unsafe { view.Anonymous.Texture2D };
        if resource.DepthOrArraySize != 1 || v.PlaneSlice != 0 {
            return false;
        }
        (v.MostDetailedMip, v.MipLevels, v.ResourceMinLODClamp)
    } else if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2DARRAY {
        // SAFETY: a Texture2DArray view.
        let v = unsafe { view.Anonymous.Texture2DArray };
        let layers = resource.DepthOrArraySize as u32;
        if v.PlaneSlice != 0 || v.ArraySize == 0 || v.FirstArraySlice >= layers || v.ArraySize > layers - v.FirstArraySlice {
            return false;
        }
        (v.MostDetailedMip, v.MipLevels, v.ResourceMinLODClamp)
    } else {
        return false;
    };
    let levels = resource.MipLevels as u32;
    first < levels
        && mips != 0
        && (mips == u32::MAX || mips <= levels - first)
        && clamp.is_finite()
        && clamp >= 0.0
        && clamp <= (resource.MipLevels as i32 - 1) as f32
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Classify (`output` always written).
///
/// # Safety
/// `value` is valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bfd_classify(value: *const Value, output: *mut D3D12_SHADER_RESOURCE_VIEW_DESC) -> u32 {
    let (result, view) = classify(unsafe { &*value });
    unsafe { *output = view };
    result
}
/// ViewMatches.
///
/// # Safety
/// The records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bfd_view_matches(value: *const Value, resource: *const D3D12_RESOURCE_DESC, view: *const D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    unsafe { view_matches(&*value, &*resource, &*view) }
}
