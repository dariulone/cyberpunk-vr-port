//! The native early depth's alpha inputs (src/Render/NativeEarlyDepth.cpp, `AlphaMaterial`): the pixel modifiers keep
//! the dither on (word 2 exactly 1.0f) with no sign-free offset (word 3, sign aside, zero); the dither texture is a
//! 64-slice (at least) R16G16B16A16_UNORM 2D array view with the default mapping, starting at its first mip with mips
//! and plane 0, on a resource of at least 64 x 64 holding 64 slices from the view's first; and the optimized path's alpha
//! image is a 2D BC1 or BC7 sRGB view with the default mapping.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ned_alpha_modifiers`, `cvr_ned_dither_view` and
//! `cvr_ned_alpha_image`; the unit tests below cover each condition.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The modifiers' dither words.
pub fn alpha_modifiers(modifiers: &[u32; 4]) -> bool {
    modifiers[2] == 0x3F80_0000 && modifiers[3] & 0x7FFF_FFFF == 0
}
/// The dither texture's view on its resource.
pub fn dither_view(view: &D3D12_SHADER_RESOURCE_VIEW_DESC, desc: &D3D12_RESOURCE_DESC) -> bool {
    if view.ViewDimension != D3D12_SRV_DIMENSION_TEXTURE2DARRAY
        || view.Format != DXGI_FORMAT_R16G16B16A16_UNORM
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
    {
        return false;
    }
    // SAFETY: the dimension selects the 2D array member.
    let array = unsafe { view.Anonymous.Texture2DArray };
    let slices = u32::from(desc.DepthOrArraySize);
    array.MostDetailedMip == 0
        && array.MipLevels != 0
        && array.ArraySize >= 64
        && array.PlaneSlice == 0
        && desc.Width >= 64
        && desc.Height >= 64
        && array.FirstArraySlice <= slices
        && slices - array.FirstArraySlice >= 64
}
/// The optimized path's alpha image view.
pub fn alpha_image(view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2D
        && (view.Format == DXGI_FORMAT_BC1_UNORM_SRGB || view.Format == DXGI_FORMAT_BC7_UNORM_SRGB)
        && view.Shader4ComponentMapping == D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
}

// ---- the C ABI

/// # Safety
/// `modifiers` holds four words.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_alpha_modifiers(modifiers: *const [u32; 4]) -> bool {
    alpha_modifiers(unsafe { &*modifiers })
}
/// # Safety
/// Both descriptions are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_dither_view(view: *const D3D12_SHADER_RESOURCE_VIEW_DESC, desc: *const D3D12_RESOURCE_DESC) -> bool {
    dither_view(unsafe { &*view }, unsafe { &*desc })
}
/// # Safety
/// `view` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_alpha_image(view: *const D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    alpha_image(unsafe { &*view })
}

#[cfg(test)]
mod tests {
    //! Each condition.
    use super::*;

    fn dither() -> (D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_RESOURCE_DESC) {
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16G16B16A16_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            ..Default::default()
        };
        view.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_SRV { MostDetailedMip: 0, MipLevels: 1, FirstArraySlice: 4, ArraySize: 64, PlaneSlice: 0, ResourceMinLODClamp: 0.0 };
        let desc = D3D12_RESOURCE_DESC { Width: 64, Height: 64, DepthOrArraySize: 68, ..Default::default() };
        (view, desc)
    }
    #[test]
    fn modifiers() {
        assert!(alpha_modifiers(&[7, 8, 0x3F80_0000, 0x8000_0000]));
        assert!(alpha_modifiers(&[0, 0, 0x3F80_0000, 0]));
        assert!(!alpha_modifiers(&[0, 0, 0x3F80_0001, 0]));
        assert!(!alpha_modifiers(&[0, 0, 0x3F80_0000, 1]));
    }
    #[test]
    fn dithers() {
        let (view, desc) = dither();
        assert!(dither_view(&view, &desc));
        let array = |change: fn(&mut D3D12_TEX2D_ARRAY_SRV)| {
            let (mut v, d) = dither();
            unsafe { change(&mut v.Anonymous.Texture2DArray) };
            dither_view(&v, &d)
        };
        assert!(!array(|a| a.MostDetailedMip = 1));
        assert!(!array(|a| a.MipLevels = 0));
        assert!(!array(|a| a.ArraySize = 63));
        assert!(!array(|a| a.PlaneSlice = 1));
        assert!(!array(|a| a.FirstArraySlice = 5));
        assert!(!array(|a| a.FirstArraySlice = 69));
        let (mut v, d) = dither();
        v.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        assert!(!dither_view(&v, &d));
        let (mut v, d) = dither();
        v.Shader4ComponentMapping = 0;
        assert!(!dither_view(&v, &d));
        let (mut v, d) = dither();
        v.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
        assert!(!dither_view(&v, &d));
        let (v, mut d) = dither();
        d.Width = 63;
        assert!(!dither_view(&v, &d));
        let (v, mut d) = dither();
        d.Height = 63;
        assert!(!dither_view(&v, &d));
    }
    #[test]
    fn images() {
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_BC1_UNORM_SRGB,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            ..Default::default()
        };
        assert!(alpha_image(&view));
        view.Format = DXGI_FORMAT_BC7_UNORM_SRGB;
        assert!(alpha_image(&view));
        view.Format = DXGI_FORMAT_BC7_UNORM;
        assert!(!alpha_image(&view));
        view.Format = DXGI_FORMAT_BC1_UNORM_SRGB;
        view.Shader4ComponentMapping = 0;
        assert!(!alpha_image(&view));
        view.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        view.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2DARRAY;
        assert!(!alpha_image(&view));
    }
}
