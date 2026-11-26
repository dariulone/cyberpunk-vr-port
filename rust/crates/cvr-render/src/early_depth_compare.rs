//! The early depth pass's comparison (src/Render/EarlyDepthPass.cpp, `EarlyDepthPass::Read`): per eye, the reference
//! and paired depth (plane 0, 32-bit) and stencil (plane 1, 8-bit) copies are compared pixel by pixel -- the written
//! reference depths, the differing depths, the depths not finite on either side and the largest finite depth error;
//! the differing stencils -- and, in a native mode, each reference pixel against the native copy (planes 8..11: depth
//! then stencil, per eye).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the loop forwards to `cvr_edp_compare` (bounds-checked against the readback; a
//! plane outside it fails the read; the plane and the result mirrored); the unit tests below cover each counter.

use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;

/// The pass's Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub stride: u32,
    pub component: u32,
}
/// DepthComparisonResult (include/Render/StereoDepthComparison.hpp).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Comparison {
    pub width: u32,
    pub height: u32,
    pub draws: u32,
    pub complete: u32,
    pub depth_different: [u64; 2],
    pub stencil_different: [u64; 2],
    pub written: [u64; 2],
    pub invalid_depth: [u64; 2],
    pub max_depth_error: [f32; 2],
}
const _: () = assert!(core::mem::size_of::<Plane>() == 40 && core::mem::size_of::<Comparison>() == 88);

/// The row of a plane, `bytes` wide past the last pixel, or None outside the image.
fn row(image: &[u8], plane: &Plane, y: u32, width: u32, bytes: u64) -> Option<usize> {
    let start = plane.layout.Offset.checked_add(u64::from(y).checked_mul(u64::from(plane.layout.Footprint.RowPitch))?)?.checked_add(u64::from(plane.component))?;
    let span = if width == 0 { 0 } else { u64::from(width - 1).checked_mul(u64::from(plane.stride))?.checked_add(bytes)? };
    (start.checked_add(span)? <= image.len() as u64).then_some(start as usize)
}
/// The comparison; false when a plane leaves the readback.
pub fn compare(image: &[u8], planes: &[Plane; 12], native: bool, result: &mut Comparison, native_differences: &mut [u64; 4]) -> bool {
    let (width, height) = (result.width, result.height);
    for eye in 0..2usize {
        for plane in 0..2usize {
            let (a, b) = (&planes[plane * 4 + eye * 2], &planes[plane * 4 + eye * 2 + 1]);
            let n = &planes[8 + plane * 2 + eye];
            let bytes = if plane != 0 { 1u64 } else { 4 };
            for y in 0..height {
                let (Some(left), Some(right)) = (row(image, a, y, width, bytes), row(image, b, y, width, bytes)) else { return false };
                let actual = if native {
                    match row(image, n, y, width, bytes) {
                        Some(at) => Some(at),
                        None => return false,
                    }
                } else {
                    None
                };
                for x in 0..width as usize {
                    let l = left + x * a.stride as usize;
                    let r = right + x * b.stride as usize;
                    if let Some(at) = actual {
                        let m = at + x * n.stride as usize;
                        let k = bytes as usize;
                        native_differences[plane * 2 + eye] += u64::from(image[l..l + k] != image[m..m + k]);
                    }
                    if plane != 0 {
                        result.stencil_different[eye] += u64::from(image[l] != image[r]);
                        continue;
                    }
                    let av = u32::from_le_bytes(image[l..l + 4].try_into().unwrap());
                    let bv = u32::from_le_bytes(image[r..r + 4].try_into().unwrap());
                    result.written[eye] += u64::from(av != 0);
                    result.depth_different[eye] += u64::from(av != bv);
                    let (af, bf) = (f32::from_bits(av), f32::from_bits(bv));
                    if !af.is_finite() || !bf.is_finite() {
                        result.invalid_depth[eye] += 1;
                    } else {
                        let error = (af - bf).abs();
                        if result.max_depth_error[eye] < error {
                            result.max_depth_error[eye] = error;
                        }
                    }
                }
            }
        }
    }
    true
}

// ---- the C ABI

/// # Safety
/// `image` holds `length` bytes; `planes` holds twelve planes; `result` and `native_differences` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_compare(
    image: *const u8,
    length: usize,
    planes: *const [Plane; 12],
    native: bool,
    result: *mut Comparison,
    native_differences: *mut [u64; 4],
) -> bool {
    let image = if image.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(image, length) } };
    compare(image, unsafe { &*planes }, native, unsafe { &mut *result }, unsafe { &mut *native_differences })
}

#[cfg(test)]
mod tests {
    //! Each counter.
    use super::*;

    /// Twelve 2 x 1 planes, 16 bytes apart (depth stride 4, stencil stride 1 at component 0).
    fn planes() -> [Plane; 12] {
        core::array::from_fn(|i| {
            let mut p = Plane { stride: if (4..8).contains(&i) || i >= 10 { 1 } else { 4 }, ..Default::default() };
            p.layout.Offset = i as u64 * 16;
            p.layout.Footprint.RowPitch = 16;
            p
        })
    }
    fn put_f32(image: &mut [u8], at: usize, value: f32) {
        image[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    #[test]
    fn depths() {
        let mut image = vec![0u8; 12 * 16];
        // Eye 0: reference plane 0, paired plane 1. Pixel 0: 0.5 vs 0.25; pixel 1: 1.0 vs 1.0.
        put_f32(&mut image, 0, 0.5);
        put_f32(&mut image, 16, 0.25);
        put_f32(&mut image, 4, 1.0);
        put_f32(&mut image, 20, 1.0);
        // Eye 1: reference plane 2, paired plane 3: pixel 0 NaN vs 0.
        put_f32(&mut image, 32, f32::NAN);
        let mut result = Comparison { width: 2, height: 1, ..Default::default() };
        let mut native = [0u64; 4];
        assert!(compare(&image, &planes(), false, &mut result, &mut native));
        assert_eq!((result.written, result.depth_different, result.invalid_depth), ([2, 1], [1, 1], [0, 1]));
        assert_eq!(result.max_depth_error, [0.25, 0.0]);
        assert_eq!(native, [0; 4]);
    }
    #[test]
    fn stencils_and_native() {
        let mut image = vec![0u8; 12 * 16];
        // Eye 1 stencil: reference plane 6, paired plane 7: pixel 1 differs.
        image[6 * 16 + 1] = 3;
        // Native depth eye 0 (plane 8) differs from the reference at pixel 0; native stencil eye 1 (plane 11) at pixel 1.
        image[8 * 16] = 1;
        image[11 * 16 + 1] = 3;
        let mut result = Comparison { width: 2, height: 1, ..Default::default() };
        let mut native = [0u64; 4];
        assert!(compare(&image, &planes(), true, &mut result, &mut native));
        assert_eq!(result.stencil_different, [0, 1]);
        assert_eq!(native, [1, 0, 0, 0]);
        let mut result = Comparison { width: 2, height: 1, ..Default::default() };
        image[11 * 16 + 1] = 4;
        let mut native = [0u64; 4];
        assert!(compare(&image, &planes(), true, &mut result, &mut native));
        assert_eq!(native, [1, 0, 0, 1]);
    }
    #[test]
    fn bounds() {
        let image = vec![0u8; 12 * 16];
        let mut result = Comparison { width: 2, height: 1, ..Default::default() };
        let mut p = planes();
        p[1].layout.Offset = 12 * 16 - 7; // the second pixel's depth ends one byte past the image
        assert!(!compare(&image, &p, false, &mut result, &mut [0; 4]));
        p[1].layout.Offset = 12 * 16 - 8;
        assert!(compare(&image, &p, false, &mut Comparison { width: 2, height: 1, ..Default::default() }, &mut [0; 4]));
        // The native planes are only read in a native mode.
        let mut q = planes();
        q[9].layout.Offset = 1 << 20;
        assert!(compare(&image, &q, false, &mut Comparison { width: 2, height: 1, ..Default::default() }, &mut [0; 4]));
        assert!(!compare(&image, &q, true, &mut Comparison { width: 2, height: 1, ..Default::default() }, &mut [0; 4]));
    }
}
