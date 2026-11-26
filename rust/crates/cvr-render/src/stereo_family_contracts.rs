//! The stereo families' manifest contracts and pass gates (src/Render/NativeStereoFamilies.cpp, `Digest`, `Load`'s
//! pair, use and native stage camera rules, `PairedRoot`'s variant key and `ObserveNativeDepth`'s gates): a digest is 64
//! lowercase hex characters; a pair is an SRV or CBV for the vertex stage (or the pixel stage when the family has a
//! pixel shader) in a private space; a use has descriptors and the vertex (or, with a pixel shader, the pixel) stage;
//! every pair names exactly one single-descriptor use in space 0 of its type, stage and register; the native stage
//! cameras are the CBV b1 uses in space 0 (one 848-byte descriptor per stage, vertex bit 1, pixel bit 2, each once);
//! the paired root's key is its pairs sorted by stage, type, register and private space, four values each; a family's
//! uses write no UAV; and the native depth pass (no colour targets, D32S8X24 depth, one sample, a fixed raster and a
//! known depth state, a vertex shader, no other stages or stream output) and the native geometry pass (three or four
//! targets R10G10B10A2 / R10G10B10A2 / R8G8B8A8 [/ R16G16_FLOAT] over the same depth and sampling) gates.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nsf_*` (the pairs, uses and pass contract mirrored); the
//! unit tests below cover each.

use windows::Win32::Graphics::Direct3D12::{D3D12_DEPTH_STENCIL_DESC, D3D12_PRIMITIVE_TOPOLOGY_TYPE, D3D12_RT_FORMAT_ARRAY};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};

/// StereoPairedRoot.hpp's PairedBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PairedBinding {
    pub kind: i32,
    pub visibility: i32,
    pub source_register: u32,
    pub private_space: u32,
}
/// NativeStereoFamilies.hpp's ResourceUse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceUse {
    pub kind: u32,
    pub visibility: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
    pub bytes: u32,
}
/// NativeStereoFamilies.hpp's PassContract.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PassContract {
    pub targets: D3D12_RT_FORMAT_ARRAY,
    pub depth: DXGI_FORMAT,
    pub samples: DXGI_SAMPLE_DESC,
    pub topology: D3D12_PRIMITIVE_TOPOLOGY_TYPE,
    pub fixed_raster_state: bool,
    pub pixel_shader: bool,
    pub depth_state: D3D12_DEPTH_STENCIL_DESC,
    pub depth_state_known: bool,
}
const _: () = assert!(
    core::mem::size_of::<PairedBinding>() == 16
        && core::mem::size_of::<ResourceUse>() == 24
        && core::mem::size_of::<PassContract>() == 112
        && core::mem::offset_of!(PassContract, depth_state_known) == 108
);

const SRV: i32 = 0;
const UAV: u32 = 1;
const CBV: i32 = 2;
const VERTEX: i32 = 1;
const PIXEL: i32 = 5;
const D32S8X24: i32 = 20;
const R10G10B10A2: i32 = 24;
const R8G8B8A8: i32 = 28;
const R16G16_FLOAT: i32 = 34;

/// Digest.
pub fn digest(s: &[u8]) -> bool {
    s.len() == 64 && s.iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
/// Load's pair contract.
pub fn pair_valid(b: &PairedBinding, pixel: bool) -> bool {
    (b.kind == SRV || b.kind == CBV) && (b.visibility == VERTEX || (pixel && b.visibility == PIXEL)) && b.private_space != 0
}
/// Load's use contract.
pub fn use_valid(u: &ResourceUse, pixel: bool) -> bool {
    u.count != 0 && (u.visibility == 1 || (pixel && u.visibility == 5))
}
/// Every pair has its unique original resource.
pub fn pairs_unique(pairs: &[PairedBinding], uses: &[ResourceUse]) -> bool {
    pairs.iter().all(|b| {
        uses.iter()
            .filter(|u| u.kind == b.kind as u32 && u.visibility == b.visibility as u32 && u.space == 0 && u.shader_register == b.source_register && u.count == 1)
            .count()
            == 1
    })
}
/// The native stage cameras' bits, or None.
pub fn stage_cameras(uses: &[ResourceUse]) -> Option<u32> {
    let mut expected = 0u32;
    for u in uses.iter().filter(|u| u.kind == CBV as u32 && u.space == 0 && u.shader_register == 1) {
        let bit = if u.visibility == VERTEX as u32 { 1 } else { 2 };
        if u.count != 1 || u.bytes != 848 || expected & bit != 0 {
            return None;
        }
        expected |= bit;
    }
    Some(expected)
}
/// PairedRoot's ordering and key.
pub fn paired_key(pairs: &mut [PairedBinding]) -> Vec<u32> {
    pairs.sort_by_key(|b| (b.visibility, b.kind, b.source_register, b.private_space));
    pairs.iter().flat_map(|b| [b.visibility as u32, b.kind as u32, b.source_register, b.private_space]).collect()
}
/// Whether a use writes a UAV.
pub fn uses_uav(uses: &[ResourceUse]) -> bool {
    uses.iter().any(|u| u.kind == UAV)
}
fn sampling(p: &PassContract) -> bool {
    p.depth.0 == D32S8X24 && p.samples.Count == 1 && p.samples.Quality == 0 && p.fixed_raster_state && p.depth_state_known
}
/// ObserveNativeDepth's depth gate.
pub fn depth_pass(p: &PassContract, vertex: bool, other_stages: bool, stream_output: bool) -> bool {
    !other_stages && !stream_output && vertex && p.targets.NumRenderTargets == 0 && sampling(p)
}
/// ObserveNativeDepth's geometry gate.
pub fn geometry_pass(p: &PassContract, other_stages: bool, stream_output: bool) -> bool {
    let (count, f) = (p.targets.NumRenderTargets, &p.targets.RTFormats);
    !other_stages
        && !stream_output
        && (count == 3 || count == 4)
        && f[0].0 == R10G10B10A2
        && f[1].0 == R10G10B10A2
        && f[2].0 == R8G8B8A8
        && (count != 4 || f[3].0 == R16G16_FLOAT)
        && sampling(p)
}

// ---- the C ABI

unsafe fn slice<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if p.is_null() || n == 0 { &[] } else { unsafe { core::slice::from_raw_parts(p, n) } }
}
/// # Safety
/// `s` holds `n` bytes (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_digest(s: *const u8, n: usize) -> bool {
    digest(unsafe { slice(s, n) })
}
/// # Safety
/// `b` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_pair_valid(b: *const PairedBinding, pixel: bool) -> bool {
    pair_valid(unsafe { &*b }, pixel)
}
/// # Safety
/// `u` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_use_valid(u: *const ResourceUse, pixel: bool) -> bool {
    use_valid(unsafe { &*u }, pixel)
}
/// # Safety
/// The arrays hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_pairs_unique(pairs: *const PairedBinding, pair_count: usize, uses: *const ResourceUse, use_count: usize) -> bool {
    unsafe { pairs_unique(slice(pairs, pair_count), slice(uses, use_count)) }
}
/// # Safety
/// `uses` holds `count` uses. Returns u32::MAX for an unsupported binding.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_stage_cameras(uses: *const ResourceUse, count: usize) -> u32 {
    stage_cameras(unsafe { slice(uses, count) }).unwrap_or(u32::MAX)
}
/// # Safety
/// `pairs` holds `count` pairs (sorted in place); `key` holds four values per pair.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_paired_key(pairs: *mut PairedBinding, count: usize, key: *mut u32) {
    if pairs.is_null() || count == 0 {
        return;
    }
    let pairs = unsafe { core::slice::from_raw_parts_mut(pairs, count) };
    let values = paired_key(pairs);
    unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), key, values.len()) };
}
/// # Safety
/// `uses` holds `count` uses.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_uses_uav(uses: *const ResourceUse, count: usize) -> bool {
    uses_uav(unsafe { slice(uses, count) })
}
/// # Safety
/// `pass` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_depth_pass(pass: *const PassContract, vertex: bool, other_stages: bool, stream_output: bool) -> bool {
    depth_pass(unsafe { &*pass }, vertex, other_stages, stream_output)
}
/// # Safety
/// `pass` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_geometry_pass(pass: *const PassContract, other_stages: bool, stream_output: bool) -> bool {
    geometry_pass(unsafe { &*pass }, other_stages, stream_output)
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn digests() {
        let good = "17f7e646a02eeaa6ea496dd75a21a494588b29dd926a545fb52bee45dec68117";
        assert!(digest(good.as_bytes()));
        assert!(!digest(&good.as_bytes()[..63]));
        assert!(!digest(good.to_uppercase().as_bytes()));
        assert!(!digest(good.replace('7', "g").as_bytes()));
        assert!(!digest(b""));
    }
    fn pair(kind: i32, visibility: i32, source_register: u32, private_space: u32) -> PairedBinding {
        PairedBinding { kind, visibility, source_register, private_space }
    }
    fn usage(kind: u32, visibility: u32, space: u32, shader_register: u32, count: u32, bytes: u32) -> ResourceUse {
        ResourceUse { kind, visibility, space, shader_register, count, bytes }
    }
    #[test]
    fn pairs_and_uses() {
        assert!(pair_valid(&pair(0, 1, 3, 7), false));
        assert!(pair_valid(&pair(2, 1, 3, 7), false));
        assert!(!pair_valid(&pair(1, 1, 3, 7), true));
        assert!(!pair_valid(&pair(3, 1, 3, 7), true));
        assert!(!pair_valid(&pair(0, 5, 3, 7), false));
        assert!(pair_valid(&pair(0, 5, 3, 7), true));
        assert!(!pair_valid(&pair(0, 0, 3, 7), true));
        assert!(!pair_valid(&pair(0, 1, 3, 0), true));
        assert!(use_valid(&usage(0, 1, 0, 0, 1, 0), false));
        assert!(!use_valid(&usage(0, 1, 0, 0, 0, 0), true));
        assert!(!use_valid(&usage(0, 5, 0, 0, 1, 0), false));
        assert!(use_valid(&usage(0, 5, 0, 0, 1, 0), true));
        assert!(!use_valid(&usage(0, 0, 0, 0, 1, 0), true));
        let uses = [usage(0, 1, 0, 3, 1, 0), usage(2, 1, 0, 3, 1, 16), usage(0, 1, 0, 4, 2, 0), usage(0, 1, 1, 5, 1, 0)];
        assert!(pairs_unique(&[pair(0, 1, 3, 9), pair(2, 1, 3, 9)], &uses));
        assert!(pairs_unique(&[], &uses));
        // A multi-descriptor use, one in another space, another stage or none is no original.
        for b in [pair(0, 1, 4, 9), pair(0, 1, 5, 9), pair(0, 5, 3, 9), pair(0, 1, 6, 9)] {
            assert!(!pairs_unique(&[b], &uses), "{b:?}");
        }
        // Two matching uses are not unique.
        assert!(!pairs_unique(&[pair(0, 1, 3, 9)], &[uses[0], uses[0]]));
    }
    #[test]
    fn cameras() {
        let camera = |visibility| usage(2, visibility, 0, 1, 1, 848);
        assert_eq!(stage_cameras(&[]), Some(0));
        assert_eq!(stage_cameras(&[camera(1)]), Some(1));
        assert_eq!(stage_cameras(&[camera(5), camera(1), usage(2, 1, 0, 2, 1, 16)]), Some(3));
        assert_eq!(stage_cameras(&[camera(1), camera(1)]), None);
        assert_eq!(stage_cameras(&[usage(2, 1, 0, 1, 2, 848)]), None);
        assert_eq!(stage_cameras(&[usage(2, 1, 0, 1, 1, 847)]), None);
        // Another space or type is no camera.
        assert_eq!(stage_cameras(&[usage(2, 1, 1, 1, 2, 0), usage(0, 1, 0, 1, 2, 0)]), Some(0));
        // Any stage other than vertex takes the pixel bit.
        assert_eq!(stage_cameras(&[camera(0)]), Some(2));
    }
    #[test]
    fn keys() {
        let mut pairs = [pair(2, 5, 1, 9), pair(0, 1, 4, 9), pair(2, 1, 3, 8), pair(0, 1, 3, 9)];
        assert_eq!(paired_key(&mut pairs), vec![1, 0, 3, 9, 1, 0, 4, 9, 1, 2, 3, 8, 5, 2, 1, 9]);
        assert!(uses_uav(&[usage(0, 1, 0, 0, 1, 0), usage(1, 1, 0, 0, 1, 0)]));
        assert!(!uses_uav(&[usage(0, 1, 0, 0, 1, 0), usage(2, 1, 0, 0, 1, 0)]));
    }
    fn pass(formats: &[i32]) -> PassContract {
        let mut p = PassContract { depth: DXGI_FORMAT(D32S8X24), fixed_raster_state: true, depth_state_known: true, ..Default::default() };
        p.samples.Count = 1;
        p.targets.NumRenderTargets = formats.len() as u32;
        for (i, &f) in formats.iter().enumerate() {
            p.targets.RTFormats[i] = DXGI_FORMAT(f);
        }
        p
    }
    #[test]
    fn gates() {
        assert!(depth_pass(&pass(&[]), true, false, false));
        assert!(!depth_pass(&pass(&[]), false, false, false));
        assert!(!depth_pass(&pass(&[]), true, true, false));
        assert!(!depth_pass(&pass(&[]), true, false, true));
        assert!(!depth_pass(&pass(&[R8G8B8A8]), true, false, false));
        let changes: [fn(&mut PassContract); 5] = [
            |p| p.depth = DXGI_FORMAT(45),
            |p| p.samples.Count = 2,
            |p| p.samples.Quality = 1,
            |p| p.fixed_raster_state = false,
            |p| p.depth_state_known = false,
        ];
        for change in changes {
            let mut p = pass(&[]);
            change(&mut p);
            assert!(!depth_pass(&p, true, false, false));
            let mut g = pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8]);
            change(&mut g);
            assert!(!geometry_pass(&g, false, false));
        }
        assert!(geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8]), false, false));
        assert!(geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8, R16G16_FLOAT]), false, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8, R8G8B8A8]), false, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2]), false, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R8G8B8A8, R8G8B8A8]), false, false));
        assert!(!geometry_pass(&pass(&[R8G8B8A8, R10G10B10A2, R8G8B8A8]), false, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R10G10B10A2]), false, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8]), true, false));
        assert!(!geometry_pass(&pass(&[R10G10B10A2, R10G10B10A2, R8G8B8A8]), false, true));
    }
}
