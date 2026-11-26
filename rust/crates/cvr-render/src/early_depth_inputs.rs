//! The native early depth's material and skinning inputs (src/Render/NativeEarlyDepth.cpp, `Material` and
//! `SkinMaterial`): an opaque depth family reads exactly two vertex CBVs in space 0, one descriptor each -- the camera
//! (register 1, 848 bytes) and the material (register 5, 112 bytes); and a skinned family's deformation texture is a 2D
//! R8_UNORM view with the default mapping from its first mip, with mips, plane 0 and no LOD clamp.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ned_material_uses` and `cvr_ned_deformation_view` (the
//! family's ResourceUse mirrored by early_depth_checks.rs); the unit tests below cover each condition.

use crate::early_depth_checks::ResourceUse;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8_UNORM;

const CBV: u32 = 2;
const VERTEX: u32 = 1;

/// Material's use checks (the pixel shader and the paired root's two bindings stay the .cpp's).
pub fn material_uses(uses: &[ResourceUse]) -> bool {
    if uses.len() != 2 {
        return false;
    }
    let (mut camera, mut material) = (false, false);
    for u in uses {
        if u.kind != CBV || u.visibility != VERTEX || u.space != 0 || u.count != 1 {
            return false;
        }
        match (u.shader_register, u.bytes) {
            (1, 848) => camera = true,
            (5, 112) => material = true,
            _ => return false,
        }
    }
    camera && material
}
/// SkinMaterial's deformation view.
pub fn deformation_view(view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    if view.ViewDimension != D3D12_SRV_DIMENSION_TEXTURE2D || view.Format != DXGI_FORMAT_R8_UNORM || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING {
        return false;
    }
    // SAFETY: the dimension selects the 2D member.
    let t = unsafe { view.Anonymous.Texture2D };
    t.MostDetailedMip == 0 && t.MipLevels != 0 && t.PlaneSlice == 0 && t.ResourceMinLODClamp == 0.0
}

// ---- the C ABI

/// # Safety
/// `uses` holds `count` uses (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_material_uses(uses: *const ResourceUse, count: usize) -> bool {
    let uses = if uses.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(uses, count) } };
    material_uses(uses)
}
/// # Safety
/// `view` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_deformation_view(view: *const D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    deformation_view(unsafe { &*view })
}

#[cfg(test)]
mod tests {
    //! Each condition.
    use super::*;

    fn cbv(register: u32, bytes: u32) -> ResourceUse {
        ResourceUse { kind: CBV, visibility: VERTEX, space: 0, shader_register: register, count: 1, bytes }
    }
    #[test]
    fn uses() {
        assert!(material_uses(&[cbv(1, 848), cbv(5, 112)]));
        assert!(material_uses(&[cbv(5, 112), cbv(1, 848)]));
        assert!(!material_uses(&[cbv(1, 848)]));
        assert!(!material_uses(&[cbv(1, 848), cbv(1, 848)]));
        assert!(!material_uses(&[cbv(1, 848), cbv(5, 111)]));
        assert!(!material_uses(&[cbv(1, 848), cbv(5, 112), cbv(6, 16)]));
        for change in [
            (|u: &mut ResourceUse| u.kind = 0) as fn(&mut ResourceUse),
            |u| u.visibility = 5,
            |u| u.space = 1,
            |u| u.count = 2,
        ] {
            let mut u = cbv(5, 112);
            change(&mut u);
            assert!(!material_uses(&[cbv(1, 848), u]));
        }
    }
    #[test]
    fn views() {
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            ..Default::default()
        };
        view.Anonymous.Texture2D = D3D12_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1, PlaneSlice: 0, ResourceMinLODClamp: 0.0 };
        assert!(deformation_view(&view));
        let changes: [fn(&mut D3D12_TEX2D_SRV); 4] = [|t| t.MostDetailedMip = 1, |t| t.MipLevels = 0, |t| t.PlaneSlice = 1, |t| t.ResourceMinLODClamp = 0.5];
        for change in changes {
            let mut v = view;
            unsafe { change(&mut v.Anonymous.Texture2D) };
            assert!(!deformation_view(&v));
        }
        let mut v = view;
        v.Format = windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8_UNORM;
        assert!(!deformation_view(&v));
        let mut v = view;
        v.Shader4ComponentMapping = 0;
        assert!(!deformation_view(&v));
        let mut v = view;
        v.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2DARRAY;
        assert!(!deformation_view(&v));
    }
}
