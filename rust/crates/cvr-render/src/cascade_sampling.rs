//! The sun cascades' sampling matrices (formerly include/Stereo/CascadeSampling.hpp, and Grading.cpp's millimetre
//! measure): CSConstants carries four affine world-to-shadow matrices, the native quality setting selecting a
//! contiguous active prefix (unused ones zero). Pure; the second view's lend to MAIN is grading_lends.

/// The matrices in CSConstants, and their floats.
pub const CASC_MATRICES: usize = 4;
pub const CASC_MAT_FLOATS: usize = CASC_MATRICES * 16;

/// CascadeSampling.hpp's ActiveCascadeMatrices: the native quality setting's contiguous active prefix (unused
/// matrices are zero; an active one is affine with non-degenerate columns).
pub fn active_cascade_matrices(m: &[f32; CASC_MAT_FLOATS]) -> usize {
    let mut count = 0;
    let mut ended = false;
    for i in 0..CASC_MATRICES {
        let mx = &m[16 * i..16 * i + 16];
        if mx.iter().any(|v| !v.is_finite()) {
            return 0;
        }
        if mx.iter().all(|v| *v == 0.0) {
            ended = true;
            continue;
        }
        if ended || mx[3] != 0.0 || mx[7] != 0.0 || mx[11] != 0.0 || mx[15] != 1.0 {
            return 0;
        }
        for column in 0..3 {
            let l2 = (mx[column] as f64).powi(2)
                + (mx[4 + column] as f64).powi(2)
                + (mx[8 + column] as f64).powi(2);
            if l2 <= 0.0 || !l2.is_finite() {
                return 0;
            }
        }
        count += 1;
    }
    count
}
/// SameCascadeSamplingLayout: every active cascade's extent matches within 2% (translation and orientation are free:
/// sharing them is the correction's purpose).
pub fn same_cascade_sampling_layout(
    a: &[f32; CASC_MAT_FLOATS],
    b: &[f32; CASC_MAT_FLOATS],
    worst: &mut f32,
) -> bool {
    let count = active_cascade_matrices(a);
    if count == 0 || count != active_cascade_matrices(b) {
        *worst = 1.0;
        return false;
    }
    let mut difference = 0f64;
    for i in 0..count {
        for column in 0..3 {
            let (ma, mb) = (&a[16 * i..], &b[16 * i..]);
            let (mut la, mut lb) = (0f64, 0f64);
            for row in 0..3 {
                la += (ma[4 * row + column] as f64) * ma[4 * row + column] as f64;
                lb += (mb[4 * row + column] as f64) * mb[4 * row + column] as f64;
            }
            let (la, lb) = (la.sqrt(), lb.sqrt());
            difference = difference.max((la - lb).abs() / la.max(lb));
        }
    }
    *worst = difference as f32;
    difference <= 0.02
}
/// Two matrices' disagreement along the LIGHT, in millimetres (the translation's depth term over the depth axis'
/// length in depth units per metre).
pub fn cascs_mm(pa: &[f32], pb: &[f32]) -> f32 {
    let len = (0..3)
        .map(|r| pa[r * 4 + 2] * pa[r * 4 + 2])
        .sum::<f32>()
        .sqrt();
    if len > 1e-9 {
        (pa[14] - pb[14]).abs() / len * 1000.0
    } else {
        0.0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn affine(scale: f32, tz: f32) -> [f32; 16] {
        [
            scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0, 1.0, 2.0, tz, 1.0,
        ]
    }
    fn setup(mats: &[[f32; 16]]) -> [f32; CASC_MAT_FLOATS] {
        let mut m = [0f32; CASC_MAT_FLOATS];
        for (i, x) in mats.iter().enumerate() {
            m[i * 16..i * 16 + 16].copy_from_slice(x);
        }
        m
    }
    #[test]
    fn active_prefix_and_layout() {
        let a = setup(&[affine(0.08, 3.0), affine(0.025, 1.0)]);
        assert_eq!(active_cascade_matrices(&a), 2);
        let mut gap = setup(&[affine(0.08, 3.0)]);
        gap[32..48].copy_from_slice(&affine(0.01, 0.0));
        assert_eq!(
            active_cascade_matrices(&gap),
            0,
            "an active matrix after a zero one is no prefix"
        );
        let b = setup(&[affine(0.0805, 9.0), affine(0.025, -4.0)]);
        let mut worst = 0.0;
        assert!(same_cascade_sampling_layout(&a, &b, &mut worst));
        assert!(worst < 0.02);
        let c = setup(&[affine(0.09, 3.0), affine(0.025, 1.0)]);
        assert!(!same_cascade_sampling_layout(&a, &c, &mut worst));
        assert!(worst > 0.02);
        let one = setup(&[affine(0.08, 3.0)]);
        assert!(!same_cascade_sampling_layout(&a, &one, &mut worst));
        assert_eq!(worst, 1.0);
    }
    #[test]
    fn mm_along_the_light() {
        let a = affine(0.5, 1.0);
        let b = affine(0.5, 1.029);
        assert!((cascs_mm(&a, &b) - 58.0).abs() < 0.01);
    }
}
