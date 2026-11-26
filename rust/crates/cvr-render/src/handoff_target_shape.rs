//! The target handoff's target shape (src/Render/NativeTargetHandoff.cpp, `TargetShape`): a captured geometry target is
//! the bound view of exactly this resource (an identity, a CPU handle, no view flags) over a single-mip, single-layer,
//! single-sample 2D texture in the default layout, the view's size, format and resource flags its own; and by its
//! format's plane count and slot, the depth (slot 4: two planes, D32S8X24 viewed as such, depth-stencil allowed) or a
//! colour target (one plane, render target allowed, viewed in its own format, R10G10B10A2 / R10G10B10A2 / R8G8B8A8
//! for slots 0..2).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nth_view_shape` and `cvr_nth_format_shape` (the view as
//! cvr-gpu's TargetInfo; the format query stays the .cpp's); the unit tests below cover each condition.

use cvr_gpu::scene_state::TargetInfo;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The view's shape against the resource's description.
pub fn view_shape(desc: &D3D12_RESOURCE_DESC, view: &TargetInfo, resource: u64, handle: usize) -> bool {
    view.identity != 0
        && view.resource == resource
        && handle != 0
        && view.view_flags == 0
        && desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && desc.Width != 0
        && desc.Height != 0
        && desc.MipLevels == 1
        && desc.DepthOrArraySize == 1
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && desc.Layout == D3D12_TEXTURE_LAYOUT_UNKNOWN
        && view.width == desc.Width
        && view.height == desc.Height
        && view.format == desc.Format.0 as u32
        && view.resource_flags == desc.Flags.0 as u32
}
/// The format's planes and the slot's rules.
pub fn format_shape(slot: u32, desc: &D3D12_RESOURCE_DESC, view_format: u32, planes: u32) -> bool {
    if planes != if slot == 4 { 2 } else { 1 } {
        return false;
    }
    if slot == 4 {
        (desc.Format == DXGI_FORMAT_R32G8X24_TYPELESS || desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT)
            && view_format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0 as u32
            && (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL).0 != 0
    } else {
        const FORMATS: [DXGI_FORMAT; 3] = [DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM];
        (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET).0 != 0
            && view_format == desc.Format.0 as u32
            && FORMATS.get(slot as usize) == Some(&desc.Format)
    }
}

// ---- the C ABI

/// # Safety
/// `desc` and `view` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nth_view_shape(desc: *const D3D12_RESOURCE_DESC, view: *const TargetInfo, resource: u64, handle: usize) -> bool {
    unsafe { view_shape(&*desc, &*view, resource, handle) }
}
/// # Safety
/// `desc` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nth_format_shape(slot: u32, desc: *const D3D12_RESOURCE_DESC, view_format: u32, planes: u32) -> bool {
    format_shape(slot, unsafe { &*desc }, view_format, planes)
}

#[cfg(test)]
mod tests {
    //! Each condition.
    use super::*;

    fn desc(format: DXGI_FORMAT, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 64,
            Height: 32,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: flags,
            ..Default::default()
        }
    }
    fn view(d: &D3D12_RESOURCE_DESC) -> TargetInfo {
        TargetInfo { identity: 9, resource: 0x100, width: d.Width, height: d.Height, format: d.Format.0 as u32, view_format: d.Format.0 as u32, resource_flags: d.Flags.0 as u32, view_flags: 0, reserved: 0 }
    }
    #[test]
    fn views() {
        let d = desc(DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
        assert!(view_shape(&d, &view(&d), 0x100, 7));
        assert!(!view_shape(&d, &view(&d), 0x101, 7));
        assert!(!view_shape(&d, &view(&d), 0x100, 0));
        let views: [fn(&mut TargetInfo); 6] = [|v| v.identity = 0, |v| v.view_flags = 1, |v| v.width = 65, |v| v.height = 33, |v| v.format = 1, |v| v.resource_flags = 0];
        for change in views {
            let mut v = view(&d);
            change(&mut v);
            assert!(!view_shape(&d, &v, 0x100, 7));
        }
        let descs: [fn(&mut D3D12_RESOURCE_DESC); 7] = [
            |d| d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE3D,
            |d| d.MipLevels = 2,
            |d| d.DepthOrArraySize = 2,
            |d| d.SampleDesc.Count = 4,
            |d| d.SampleDesc.Quality = 1,
            |d| d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            |d| (d.Width, d.Height) = (0, 0),
        ];
        for change in descs {
            let mut e = d;
            change(&mut e);
            let mut v = view(&d);
            (v.width, v.height) = (e.Width, e.Height);
            assert!(!view_shape(&e, &v, 0x100, 7));
        }
    }
    #[test]
    fn formats() {
        let colour = |f| desc(f, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
        assert!(format_shape(0, &colour(DXGI_FORMAT_R10G10B10A2_UNORM), DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32, 1));
        assert!(format_shape(2, &colour(DXGI_FORMAT_R8G8B8A8_UNORM), DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32, 1));
        assert!(!format_shape(2, &colour(DXGI_FORMAT_R10G10B10A2_UNORM), DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32, 1));
        assert!(!format_shape(0, &colour(DXGI_FORMAT_R10G10B10A2_UNORM), DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32, 2));
        assert!(!format_shape(0, &colour(DXGI_FORMAT_R10G10B10A2_UNORM), DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32, 1));
        assert!(!format_shape(0, &desc(DXGI_FORMAT_R10G10B10A2_UNORM, D3D12_RESOURCE_FLAG_NONE), DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32, 1));
        assert!(!format_shape(3, &colour(DXGI_FORMAT_R10G10B10A2_UNORM), DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32, 1));
        let depth = |f| desc(f, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
        let d32 = DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0 as u32;
        assert!(format_shape(4, &depth(DXGI_FORMAT_R32G8X24_TYPELESS), d32, 2));
        assert!(format_shape(4, &depth(DXGI_FORMAT_D32_FLOAT_S8X24_UINT), d32, 2));
        assert!(!format_shape(4, &depth(DXGI_FORMAT_D32_FLOAT_S8X24_UINT), d32, 1));
        assert!(!format_shape(4, &depth(DXGI_FORMAT_D24_UNORM_S8_UINT), d32, 2));
        assert!(!format_shape(4, &depth(DXGI_FORMAT_D32_FLOAT_S8X24_UINT), DXGI_FORMAT_R32G8X24_TYPELESS.0 as u32, 2));
        assert!(!format_shape(4, &desc(DXGI_FORMAT_D32_FLOAT_S8X24_UINT, D3D12_RESOURCE_FLAG_NONE), d32, 2));
    }
}
