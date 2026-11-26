//! The native prepared pair's image comparison (src/Render/NativePreparedPair.cpp, `Poll`): each readback plane row
//! compares, per pixel at the plane's stride and component, the reference (layout 0) and paired (layout 1) outputs with
//! each other (the per-eye pair differences) and with the baseline (layout 2) -- the plane kind's per-eye changes,
//! colors 6, depth 10, stencil 14, the reference's at the kind and the paired's two after -- and, for colors, counts
//! the reference's and paired's non-zero 32-bit pixels per eye and slot, the reference's changed colors per eye and the
//! baseline's non-zero colors; it counts the planes and their compared bytes and flags the plane set complete (two eyes
//! of the colors, depth and stencil). Then the verdict: pair differences fail the pass (20); a refused input keeps the
//! baseline silent (no changes, 42) with the silent queries; otherwise every eye changes some plane with executed
//! queries (21).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_compare_images` (bounds-checked against the readback;
//! a plane outside it fails the map, 19) and `cvr_npp_baseline_verdict`; the unit tests below cover each counter, the
//! plane kinds, the bounds and each verdict.

use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;

/// The .cpp's Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub eye: u32,
    pub slot: u32,
    pub plane: u32,
    pub rows: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
    pub layouts: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 4],
}
const _: () = assert!(core::mem::size_of::<Plane>() == 160 && core::mem::offset_of!(Plane, layouts) == 32);

/// The counters the comparison adds to.
pub struct Counters<'a> {
    pub pair_stats: &'a mut [u64; 16],
    pub baseline: &'a mut [u64; 24],
    pub color: &'a mut [u64; 12],
}
fn bump(value: &mut u64, by: bool) {
    *value = value.wrapping_add(u64::from(by));
}
/// The row start of one layout, with the row's last pixel in bounds.
fn row(image: &[u8], layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, y: u32, component: u32, span: u64) -> Option<usize> {
    let start = layout.Offset.checked_add(u64::from(y).checked_mul(u64::from(layout.Footprint.RowPitch))?)?.checked_add(u64::from(component))?;
    (start.checked_add(span)? <= image.len() as u64).then_some(start as usize)
}
/// Compares the planes; false when a plane leaves the readback (the counters then hold the rows compared before it).
pub fn compare_images(image: &[u8], planes: &[Plane], width: u32, colors: u32, c: Counters<'_>) -> bool {
    for plane in planes {
        let eye = plane.eye as usize;
        let delta = if plane.slot < colors { 6 } else if plane.plane != 0 { 14 } else { 10 };
        let color = plane.slot < colors;
        let read = if color { plane.bytes.max(4) } else { plane.bytes } as u64;
        let span = if width == 0 { 0 } else { u64::from(width - 1) * u64::from(plane.stride) + read };
        let bytes = plane.bytes as usize;
        for y in 0..plane.rows {
            let (Some(a), Some(b), Some(base)) = (
                row(image, &plane.layouts[0], y, plane.component, span),
                row(image, &plane.layouts[1], y, plane.component, span),
                row(image, &plane.layouts[2], y, plane.component, span),
            ) else {
                return false;
            };
            for x in 0..width as usize {
                let offset = x * plane.stride as usize;
                let (pa, pb, pc) = (a + offset, b + offset, base + offset);
                let reference_changed = image[pa..pa + bytes] != image[pc..pc + bytes];
                bump(&mut c.pair_stats[6 + eye], image[pa..pa + bytes] != image[pb..pb + bytes]);
                bump(&mut c.baseline[delta + eye], reference_changed);
                bump(&mut c.baseline[delta + 2 + eye], image[pb..pb + bytes] != image[pc..pc + bytes]);
                if color {
                    let word = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().unwrap());
                    let slot = plane.slot as usize;
                    bump(&mut c.color[eye * 3 + slot], word(pa) != 0);
                    bump(&mut c.color[6 + eye * 3 + slot], word(pb) != 0);
                    bump(&mut c.pair_stats[8 + eye], reference_changed);
                    bump(&mut c.baseline[5], word(pc) != 0);
                }
            }
        }
        c.baseline[3] = c.baseline[3].wrapping_add(1);
        c.baseline[4] = c.baseline[4].wrapping_add(u64::from(width).wrapping_mul(u64::from(plane.rows)).wrapping_mul(u64::from(plane.bytes)));
    }
    c.baseline[21] = u64::from(planes.len() as u64 == 2 * (u64::from(colors) + 2) && c.baseline[3] == planes.len() as u64);
    true
}
/// The verdict's errors: the first (20 or 0) and the second (42, 21 or 0).
pub fn baseline_verdict(baseline: &mut [u64; 24], pair_stats: &[u64; 16]) -> (u32, u32) {
    let first = if pair_stats[6] != 0 || pair_stats[7] != 0 { 20 } else { 0 };
    let second = if baseline[22] != 0 {
        baseline[18] = u64::from(baseline[21] != 0 && baseline[6..18].iter().all(|&value| value == 0));
        if baseline[18] == 0 || baseline[19] == 0 { 42 } else { 0 }
    } else {
        let contribution =
            baseline[21] != 0 && baseline[20] != 0 && (0..2).all(|eye| baseline[6 + eye] != 0 || baseline[10 + eye] != 0 || baseline[14 + eye] != 0);
        if contribution { 0 } else { 21 }
    };
    (first, second)
}

// ---- the C ABI

/// # Safety
/// `image` holds `length` bytes; `planes` holds `count` planes; the arrays hold 16, 24 and 12 values.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp_compare_images(
    image: *const u8,
    length: usize,
    planes: *const Plane,
    count: usize,
    width: u32,
    colors: u32,
    pair_stats: *mut [u64; 16],
    baseline: *mut [u64; 24],
    color: *mut [u64; 12],
) -> bool {
    let image = if image.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(image, length) } };
    let planes = if planes.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(planes, count) } };
    let counters = unsafe { Counters { pair_stats: &mut *pair_stats, baseline: &mut *baseline, color: &mut *color } };
    compare_images(image, planes, width, colors, counters)
}
/// # Safety
/// The arrays hold 24 and 16 values. Returns the first error in the low byte and the second in the next.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_baseline_verdict(baseline: *mut [u64; 24], pair_stats: *const [u64; 16]) -> u32 {
    let (first, second) = baseline_verdict(unsafe { &mut *baseline }, unsafe { &*pair_stats });
    first | second << 8
}

#[cfg(test)]
mod tests {
    //! Each counter, the plane kinds, the bounds and each verdict.
    use super::*;

    const W: u32 = 3;
    fn plane(eye: u32, slot: u32, index: u32, stride: u32, bytes: u32, at: [u64; 3]) -> Plane {
        let mut p = Plane { eye, slot, plane: index, rows: 2, stride, component: 0, bytes, ..Default::default() };
        for (layout, offset) in p.layouts.iter_mut().zip(at) {
            layout.Offset = offset;
            layout.Footprint.RowPitch = 16;
        }
        p
    }
    fn counters() -> ([u64; 16], [u64; 24], [u64; 12]) {
        ([0; 16], [0; 24], [0; 12])
    }
    #[test]
    fn colors() {
        let mut image = vec![0u8; 96];
        // Reference at 0, paired at 32, baseline at 64; pixel (1, 0) differs between reference and baseline only,
        // pixel (2, 1) between paired and both.
        image[4] = 1;
        image[32 + 4] = 1;
        image[32 + 16 + 8] = 2;
        image[64] = 3;
        image[0] = 3;
        image[32] = 3;
        let (mut s, mut b, mut c) = counters();
        let p = plane(1, 2, 0, 4, 4, [0, 32, 64]);
        assert!(compare_images(&image, &[p], W, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
        assert_eq!(s[7], 1);
        assert_eq!(s[9], 1);
        assert_eq!((b[7], b[9]), (1, 2));
        assert_eq!(b[5], 1);
        assert_eq!((c[5], c[11]), (2, 3));
        assert_eq!((b[3], b[4], b[21]), (1, 24, 0));
    }
    #[test]
    fn kinds() {
        let mut image = vec![0u8; 96];
        image[32 + 1] = 1;
        for (index, delta) in [(0u32, 10usize), (1, 14)] {
            let (mut s, mut b, mut c) = counters();
            let p = Plane { component: 1, ..plane(0, 4, index, 8, 1, [0, 32, 64]) };
            assert!(compare_images(&image, &[p], 1, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
            assert_eq!((s[6], b[delta], b[delta + 2]), (1, 0, 1));
            assert_eq!(c, [0; 12]);
            assert_eq!((b[5], s[8]), (0, 0));
        }
    }
    #[test]
    fn complete_and_bounds() {
        let image = vec![0u8; 96];
        let planes: Vec<Plane> = (0..10).map(|i| plane(i / 5, [0, 1, 2, 4, 4][i as usize % 5], u32::from(i % 5 == 4), 4, 4, [0, 32, 64])).collect();
        let (mut s, mut b, mut c) = counters();
        assert!(compare_images(&image, &planes, W, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
        assert_eq!((b[3], b[21]), (10, 1));
        let (mut s, mut b, mut c) = counters();
        assert!(compare_images(&image, &planes[..9], W, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
        assert_eq!(b[21], 0);
        // The baseline's last row ends at 64 + 16 + 12 = 92; four more bytes of offset still fit, five do not.
        let (mut s, mut b, mut c) = counters();
        assert!(compare_images(&image, &[plane(0, 0, 0, 4, 4, [0, 32, 68])], W, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
        let (mut s, mut b, mut c) = counters();
        assert!(!compare_images(&image, &[plane(0, 0, 0, 4, 4, [0, 32, 69])], W, 3, Counters { pair_stats: &mut s, baseline: &mut b, color: &mut c }));
        assert_eq!(b[3], 0);
    }
    #[test]
    fn verdicts() {
        let mut b = [0u64; 24];
        b[21] = 1;
        b[20] = 1;
        b[6] = 1;
        b[15] = 1;
        assert_eq!(baseline_verdict(&mut b.clone(), &[0; 16]), (0, 0));
        let mut s = [0u64; 16];
        s[7] = 1;
        assert_eq!(baseline_verdict(&mut b.clone(), &s), (20, 0));
        for (at, value) in [(21, 0), (20, 0), (6, 0), (15, 0)] {
            let mut x = b;
            x[at] = value;
            assert_eq!(baseline_verdict(&mut x, &[0; 16]).1, 21, "{at}");
        }
        let mut quiet = [0u64; 24];
        quiet[22] = 1;
        quiet[21] = 1;
        quiet[19] = 1;
        let mut x = quiet;
        assert_eq!(baseline_verdict(&mut x, &[0; 16]), (0, 0));
        assert_eq!(x[18], 1);
        for at in [6, 17] {
            let mut x = quiet;
            x[at] = 1;
            assert_eq!(baseline_verdict(&mut x, &[0; 16]).1, 42);
            assert_eq!(x[18], 0);
        }
        let mut x = quiet;
        x[19] = 0;
        assert_eq!(baseline_verdict(&mut x, &[0; 16]).1, 42);
        let mut x = quiet;
        x[21] = 0;
        assert_eq!(baseline_verdict(&mut x, &[0; 16]).1, 42);
    }
}
