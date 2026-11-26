//! The constant capture's checks (src/Render/NativeConstantCapture.cpp, `Hash`, `CopyConstantPair`'s pair and family
//! rules, `Poll`'s deferred resolution and its depth-pass target check): the payload hash (FNV-1a 64); a pair is the
//! second eye's complete draw and the first's of one family, frame and binding count; the family reads only single
//! CBVs in space 0; a deferred binding that resolved neither ready nor pending becomes unreadable (missing) or
//! unverified (anything else), its status and detail kept in the reserved word; and every eye's target matches the
//! native one's dimension, size and format, single-layer, single-mip and single-sample.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_iw_fnv` and `cvr_ncc_*` (the draws mirrored; the family's
//! uses as stereo_family_contracts' ResourceUse); the unit tests below cover each.

use crate::stereo_family_contracts::ResourceUse;
use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC;

/// NativeConstantCapture.hpp's Draw.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Draw {
    pub family: u64,
    pub sequence: u32,
    pub first: u32,
    pub count: u32,
    pub flags: u32,
    pub side: u32,
    pub frame: u32,
}
const _: () = assert!(core::mem::size_of::<Draw>() == 32);

/// CopyConstantPair's pair (both found by the .cpp).
pub fn pair_admitted(a: &Draw, b: &Draw) -> bool {
    a.family == b.family && a.side == 1 && b.side == 0 && a.frame != 0 && a.frame == b.frame && a.count != 0 && a.count == b.count
}
/// CopyConstantPair's family uses.
pub fn single_cbvs(uses: &[ResourceUse]) -> bool {
    const CBV: u32 = 2;
    uses.iter().all(|u| u.kind == CBV && u.space == 0 && u.count == 1)
}
/// ConstantWriteTimeline::Status.
pub mod status {
    pub const READY: u32 = 0;
    pub const MISSING: u32 = 1;
    pub const PENDING: u32 = 2;
}
/// Poll's deferred resolution: (binding status, reserved word) once settled, None while ready or pending.
pub fn deferred(result_status: u32, detail: u32) -> Option<(u32, u32)> {
    if result_status == status::READY || result_status == status::PENDING {
        return None;
    }
    let settled = if result_status == status::MISSING { 8 } else { 32 };
    Some((settled, 0x8000_0000 | (result_status << 16) | detail))
}
/// Poll's depth-pass target check.
pub fn same_target(candidate: &D3D12_RESOURCE_DESC, native: &D3D12_RESOURCE_DESC) -> bool {
    candidate.Dimension == native.Dimension
        && candidate.Width == native.Width
        && candidate.Height == native.Height
        && candidate.Format == native.Format
        && candidate.DepthOrArraySize == 1
        && candidate.MipLevels == 1
        && candidate.SampleDesc.Count == 1
}

// ---- the C ABI

/// # Safety
/// Both draws are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc_pair(a: *const Draw, b: *const Draw) -> bool {
    unsafe { pair_admitted(&*a, &*b) }
}
/// # Safety
/// `uses` holds `count` uses (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc_single_cbvs(uses: *const ResourceUse, count: usize) -> bool {
    single_cbvs(if uses.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(uses, count) } })
}
/// # Safety
/// `settled` and `reserved` are writable (written only when settled).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc_deferred(result_status: u32, detail: u32, settled: *mut u32, reserved: *mut u32) -> bool {
    match deferred(result_status, detail) {
        Some((s, r)) => {
            unsafe { (*settled, *reserved) = (s, r) };
            true
        }
        None => false,
    }
}
/// # Safety
/// Both descriptions are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc_same_target(candidate: *const D3D12_RESOURCE_DESC, native: *const D3D12_RESOURCE_DESC) -> bool {
    unsafe { same_target(&*candidate, &*native) }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;
    use windows::Win32::Graphics::Direct3D12::{D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_DIMENSION_TEXTURE2D};
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM};

    #[test]
    fn pairs() {
        let a = Draw { family: 5, side: 1, frame: 3, count: 4, ..Default::default() };
        let b = Draw { side: 0, ..a };
        assert!(pair_admitted(&a, &b));
        for (x, y) in [
            (Draw { family: 6, ..a }, b),
            (Draw { side: 0, ..a }, b),
            (a, Draw { side: 1, ..b }),
            (Draw { frame: 0, ..a }, Draw { frame: 0, ..b }),
            (a, Draw { frame: 4, ..b }),
            (Draw { count: 0, ..a }, Draw { count: 0, ..b }),
            (a, Draw { count: 5, ..b }),
        ] {
            assert!(!pair_admitted(&x, &y), "{x:?} {y:?}");
        }
    }
    #[test]
    fn uses() {
        let cbv = ResourceUse { kind: 2, visibility: 1, space: 0, shader_register: 1, count: 1, bytes: 16 };
        assert!(single_cbvs(&[cbv, cbv]));
        assert!(single_cbvs(&[]));
        assert!(!single_cbvs(&[cbv, ResourceUse { kind: 0, ..cbv }]));
        assert!(!single_cbvs(&[ResourceUse { space: 1, ..cbv }]));
        assert!(!single_cbvs(&[ResourceUse { count: 2, ..cbv }]));
    }
    #[test]
    fn deferrals() {
        assert_eq!(deferred(status::READY, 0), None);
        assert_eq!(deferred(status::PENDING, 0), None);
        assert_eq!(deferred(status::MISSING, 5), Some((8, 0x8001_0005)));
        assert_eq!(deferred(4, 2), Some((32, 0x8004_0002)));
    }
    #[test]
    fn targets() {
        let native = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 64,
            Height: 32,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        assert!(same_target(&native, &native));
        let changes: [fn(&mut D3D12_RESOURCE_DESC); 7] = [
            |d| d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER,
            |d| d.Width = 65,
            |d| d.Height = 33,
            |d| d.Format = DXGI_FORMAT_R8G8B8A8_UNORM,
            |d| d.DepthOrArraySize = 2,
            |d| d.MipLevels = 2,
            |d| d.SampleDesc.Count = 4,
        ];
        for change in changes {
            let mut c = native;
            change(&mut c);
            assert!(!same_target(&c, &native));
        }
    }
}
