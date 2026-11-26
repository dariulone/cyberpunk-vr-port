//! The stereo geometry pass's draw and root checks (src/Render/StereoGeometryPass.cpp, `stereo::`): a stereo target's
//! shape (the draw's format -- or a depth view's typeless resource format -- a two-slice single-mip single-sample 2D
//! texture allowing render targets or depth, every target the same size), a draw's viewport and scissor inside the
//! targets (finite, non-empty, within the depth range), a shader use's single table slot in the original root parameters
//! (a visible table range of its type, space and registers holding the whole use; exactly one, its offset inside 32
//! bits), and a sampler use's static samplers (each register exactly one visible static sampler; at most 2048).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Target's shape, View, Locate and StaticSampler forward to `cvr_sgp_*` (the
//! target's device stays the .cpp's); `tools/single_pass_tests/geometry_pass_test` runs its cases against them in its
//! Rust variant, and the unit tests below cover them.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::early_depth_checks::ResourceUse;
use crate::geometry_commands::DrawRecord;

/// Target's shape: `width` / `height` take the first target's size, later ones must match.
pub fn target(d: &D3D12_RESOURCE_DESC, format: DXGI_FORMAT, depth: bool, width: &mut u64, height: &mut u32) -> bool {
    let typeless = (d.Format == DXGI_FORMAT_R32G8X24_TYPELESS && format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT)
        || (d.Format == DXGI_FORMAT_R24G8_TYPELESS && format == DXGI_FORMAT_D24_UNORM_S8_UINT)
        || (d.Format == DXGI_FORMAT_R32_TYPELESS && format == DXGI_FORMAT_D32_FLOAT)
        || (d.Format == DXGI_FORMAT_R16_TYPELESS && format == DXGI_FORMAT_D16_UNORM);
    let compatible = d.Format == format || (depth && typeless);
    let allow = if depth { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET };
    if !compatible
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.DepthOrArraySize != 2
        || d.MipLevels != 1
        || d.SampleDesc.Count != 1
        || d.SampleDesc.Quality != 0
        || d.Flags.0 & allow.0 == 0
    {
        return false;
    }
    if *width != 0 && (*width != d.Width || *height != d.Height) {
        return false;
    }
    *width = d.Width;
    *height = d.Height;
    *width != 0 && *height != 0
}
/// View.
pub fn view(draw: &DrawRecord, width: u64, height: u32) -> bool {
    let (v, s) = (&draw.viewport, &draw.scissor);
    v.TopLeftX.is_finite()
        && v.TopLeftY.is_finite()
        && v.Width.is_finite()
        && v.Height.is_finite()
        && v.TopLeftX >= 0.0
        && v.TopLeftY >= 0.0
        && v.Width > 0.0
        && v.Height > 0.0
        && v.TopLeftX as f64 + v.Width as f64 <= width as f64
        && v.TopLeftY as f64 + v.Height as f64 <= height as f64
        && v.MinDepth >= 0.0
        && v.MaxDepth <= 1.0
        && v.MinDepth <= v.MaxDepth
        && s.left >= 0
        && s.top >= 0
        && s.right > s.left
        && s.bottom > s.top
        && s.right as u64 <= width
        && s.bottom as u32 <= height
}
fn visible(root: D3D12_SHADER_VISIBILITY, use_: u32) -> bool {
    root == D3D12_SHADER_VISIBILITY_ALL || root.0 as u32 == use_
}
/// Locate: (parameter, offset, range index within the parameter).
///
/// # Safety
/// The root's parameter and range arrays are valid for `original` parameters.
pub unsafe fn locate(root: &D3D12_ROOT_SIGNATURE_DESC1, original: u32, use_: &ResourceUse) -> Option<(u32, u32, *const D3D12_DESCRIPTOR_RANGE1)> {
    let mut found = None;
    for p in 0..original {
        let parameter = unsafe { &*root.pParameters.add(p as usize) };
        if parameter.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE || !visible(parameter.ShaderVisibility, use_.visibility) {
            continue;
        }
        let table = unsafe { parameter.Anonymous.DescriptorTable };
        let mut next = 0u64;
        for n in 0..table.NumDescriptorRanges {
            let range = unsafe { &*table.pDescriptorRanges.add(n as usize) };
            let offset = if range.OffsetInDescriptorsFromTableStart == u32::MAX { next } else { range.OffsetInDescriptorsFromTableStart as u64 };
            next = if range.NumDescriptors == u32::MAX { u32::MAX as u64 + 1 } else { offset + range.NumDescriptors as u64 };
            if range.RangeType.0 as u32 != use_.kind || range.RegisterSpace != use_.space || use_.shader_register < range.BaseShaderRegister {
                continue;
            }
            let relative = (use_.shader_register - range.BaseShaderRegister) as u64;
            if relative >= range.NumDescriptors as u64 || use_.count as u64 > range.NumDescriptors as u64 - relative {
                continue;
            }
            if found.is_some() || offset + relative > u32::MAX as u64 {
                return None;
            }
            found = Some((p, (offset + relative) as u32, range as *const _));
        }
    }
    found
}
/// StaticSampler.
///
/// # Safety
/// The root's static sampler array is valid.
pub unsafe fn static_sampler(root: &D3D12_ROOT_SIGNATURE_DESC1, use_: &ResourceUse) -> bool {
    if use_.kind != D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 || use_.count == 0 || use_.count > 2048 || use_.shader_register as u64 + use_.count as u64 > u32::MAX as u64 + 1 {
        return false;
    }
    let samplers = if root.NumStaticSamplers == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(root.pStaticSamplers, root.NumStaticSamplers as usize) } };
    (0..use_.count).all(|n| {
        samplers.iter().filter(|s| s.RegisterSpace == use_.space && s.ShaderRegister == use_.shader_register + n && visible(s.ShaderVisibility, use_.visibility)).count() == 1
    })
}

// ---- the C ABI

/// # Safety
/// `desc` is valid; `width` and `height` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_target(desc: *const D3D12_RESOURCE_DESC, format: i32, depth: bool, width: *mut u64, height: *mut u32) -> bool {
    target(unsafe { &*desc }, DXGI_FORMAT(format), depth, unsafe { &mut *width }, unsafe { &mut *height })
}
/// # Safety
/// `draw` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_view(draw: *const DrawRecord, width: u64, height: u32) -> bool {
    view(unsafe { &*draw }, width, height)
}
/// # Safety
/// `root` and `use_` are valid; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_locate(root: *const D3D12_ROOT_SIGNATURE_DESC1, original: u32, use_: *const ResourceUse, parameter: *mut u32, offset: *mut u32, range: *mut *const D3D12_DESCRIPTOR_RANGE1) -> bool {
    match unsafe { locate(&*root, original, &*use_) } {
        Some((p, o, r)) => {
            unsafe {
                *parameter = p;
                *offset = o;
                *range = r;
            }
            true
        }
        None => false,
    }
}
/// # Safety
/// `root` and `use_` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_static_sampler(root: *const D3D12_ROOT_SIGNATURE_DESC1, use_: *const ResourceUse) -> bool {
    unsafe { static_sampler(&*root, &*use_) }
}

#[cfg(test)]
mod tests {
    //! Target shapes, view bounds, table locations and static samplers.
    use super::*;

    #[test]
    fn targets() {
        let d = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 64,
            Height: 32,
            DepthOrArraySize: 2,
            MipLevels: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
            ..Default::default()
        };
        let (mut w, mut h) = (0u64, 0u32);
        assert!(target(&d, DXGI_FORMAT_D32_FLOAT, true, &mut w, &mut h));
        assert_eq!((w, h), (64, 32));
        assert!(!target(&d, DXGI_FORMAT_D32_FLOAT, false, &mut w, &mut h));
        assert!(!target(&D3D12_RESOURCE_DESC { Width: 128, ..d }, DXGI_FORMAT_D32_FLOAT, true, &mut w, &mut h));
        assert!(!target(&D3D12_RESOURCE_DESC { DepthOrArraySize: 1, ..d }, DXGI_FORMAT_D32_FLOAT, true, &mut 0, &mut 0));
        let color = D3D12_RESOURCE_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, ..d };
        assert!(target(&color, DXGI_FORMAT_R8G8B8A8_UNORM, false, &mut 0, &mut 0));
        assert!(!target(&color, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, false, &mut 0, &mut 0));
    }
    #[test]
    fn views() {
        // SAFETY: a plain record, all zero.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: 64.0, Height: 32.0, MinDepth: 0.0, MaxDepth: 1.0 };
        d.scissor = windows::Win32::Foundation::RECT { left: 0, top: 0, right: 64, bottom: 32 };
        assert!(view(&d, 64, 32));
        assert!(!view(&d, 63, 32));
        d.viewport.MaxDepth = 1.5;
        assert!(!view(&d, 64, 32));
        d.viewport.MaxDepth = 1.0;
        d.viewport.Width = f32::NAN;
        assert!(!view(&d, 64, 32));
        d.viewport.Width = 64.0;
        d.scissor.right = 0;
        assert!(!view(&d, 64, 32));
    }
    #[test]
    fn locations_and_samplers() {
        let ranges = [
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 4, BaseShaderRegister: 0, OffsetInDescriptorsFromTableStart: 0, ..Default::default() },
            D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, NumDescriptors: 2, BaseShaderRegister: 1, OffsetInDescriptorsFromTableStart: u32::MAX, ..Default::default() },
        ];
        let parameters = [D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 2, pDescriptorRanges: ranges.as_ptr() } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        }];
        let samplers = [D3D12_STATIC_SAMPLER_DESC { ShaderRegister: 3, ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL, ..Default::default() }];
        let root = D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: 1, pParameters: parameters.as_ptr(), NumStaticSamplers: 1, pStaticSamplers: samplers.as_ptr(), ..Default::default() };
        let cbv = ResourceUse { kind: 2, visibility: 5, space: 0, shader_register: 2, count: 1, bytes: 256 };
        let (p, o, r) = unsafe { locate(&root, 1, &cbv) }.unwrap();
        assert_eq!((p, o, r), (0, 5, &ranges[1] as *const _));
        assert!(unsafe { locate(&root, 1, &ResourceUse { visibility: 1, ..cbv }) }.is_none());
        assert!(unsafe { locate(&root, 1, &ResourceUse { count: 2, ..cbv }) }.is_none());
        assert!(unsafe { locate(&root, 0, &cbv) }.is_none());
        let sampler = ResourceUse { kind: 3, visibility: 5, space: 0, shader_register: 3, count: 1, bytes: 0 };
        assert!(unsafe { static_sampler(&root, &sampler) });
        assert!(!unsafe { static_sampler(&root, &ResourceUse { count: 2, ..sampler }) });
        assert!(!unsafe { static_sampler(&root, &ResourceUse { kind: 0, ..sampler }) });
    }
}
