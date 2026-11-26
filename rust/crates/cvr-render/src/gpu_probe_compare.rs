//! The stereo GPU probe's readback comparison (src/Render/StereoGpuProbe.cpp, `Poll`): each compared target (the depth
//! target alone in the depth-only mode, else the three colors and the depth) compares its reference (mode 0) and shared
//! (mode 1) copies byte by byte per sub-plane row (two eyes per plane), counting the differing and compared bytes, the
//! per-eye and per-target differences; for 1- or 4-byte pixels it records the first 128 differing pixels (target, eye,
//! plane, x, y, both values); and the shared copy's written pixels of the first color target (or the depth-only depth
//! planes) give each eye's coverage.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the loop forwards to `cvr_sgpc_compare` (bounds-checked against the readback; a
//! plane outside it fails the probe; the surfaces' copy layouts and the differences mirrored); the unit tests below
//! cover each counter.

use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;

/// One surface's copy layout (the .cpp's Surface without its target).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Layout {
    pub planes: u32,
    pub reserved: u32,
    pub footprints: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 4],
    pub row_bytes: [u64; 4],
    pub rows: [u32; 4],
}
/// gpu_probe::Difference.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Difference {
    pub target: u32,
    pub eye: u32,
    pub plane: u32,
    pub x: u32,
    pub y: u32,
    pub reference: u32,
    pub shared: u32,
}
/// The probe's counters.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub different: u64,
    pub compared: u64,
    pub coverage: [u64; 2],
}
const _: () = assert!(core::mem::size_of::<Layout>() == 184 && core::mem::size_of::<Difference>() == 28 && core::mem::size_of::<Totals>() == 32);

/// The exported difference counters and samples.
pub struct Exports<'a> {
    pub eyes: &'a mut [u64; 2],
    pub targets: &'a mut [u64; 4],
    pub samples: &'a mut [Difference; 128],
    pub count: &'a mut u32,
}
fn start(footprint: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, row: u32, span: u64, len: usize) -> Option<usize> {
    let at = footprint.Offset.checked_add(u64::from(row).checked_mul(u64::from(footprint.Footprint.RowPitch))?)?;
    (at.checked_add(span)? <= len as u64).then_some(at as usize)
}
/// The comparison of `surfaces[mode][target]`; None when a plane leaves the readback.
pub fn compare(image: &[u8], surfaces: &[[Layout; 4]; 2], width: u32, depth_only: bool, e: Exports<'_>) -> Option<Totals> {
    let mut totals = Totals::default();
    for target in if depth_only { 3 } else { 0 }..4usize {
        let (a, b) = (&surfaces[0][target], &surfaces[1][target]);
        for sub in 0..(a.planes as usize * 2).min(4) {
            let row_bytes = a.row_bytes[sub];
            let stride = if width == 0 { 0 } else { row_bytes / u64::from(width) };
            let covered = target == 0 || (depth_only && target == 3 && sub < 2);
            let span = row_bytes.max(if covered { u64::from(width) * 4 } else { 0 });
            for row in 0..a.rows[sub] {
                let left = start(&a.footprints[sub], row, span, image.len())?;
                let right = start(&b.footprints[sub], row, span, image.len())?;
                let n = row_bytes as usize;
                for byte in 0..n {
                    if image[left + byte] != image[right + byte] {
                        totals.different += 1;
                        e.eyes[sub % 2] += 1;
                        e.targets[target] += 1;
                    }
                    totals.compared += 1;
                }
                if stride == 1 || stride == 4 {
                    let s = stride as usize;
                    let mut x = 0u32;
                    while x < width && *e.count < 128 {
                        let value = |at: usize| {
                            let mut word = [0u8; 4];
                            word[..s].copy_from_slice(&image[at..at + s]);
                            u32::from_le_bytes(word)
                        };
                        let (expected, actual) = (value(left + x as usize * s), value(right + x as usize * s));
                        if expected != actual {
                            e.samples[*e.count as usize] =
                                Difference { target: target as u32, eye: (sub % 2) as u32, plane: (sub / 2) as u32, x, y: row, reference: expected, shared: actual };
                            *e.count += 1;
                        }
                        x += 1;
                    }
                }
                if covered {
                    for x in 0..width as usize {
                        let at = right + x * 4;
                        totals.coverage[sub] += u64::from(image[at..at + 4] != [0, 0, 0, 0]);
                    }
                }
            }
        }
    }
    Some(totals)
}

// ---- the C ABI

/// # Safety
/// `image` holds `length` bytes; `surfaces` holds two modes of four layouts; the exports and `totals` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sgpc_compare(
    image: *const u8,
    length: usize,
    surfaces: *const [[Layout; 4]; 2],
    width: u32,
    depth_only: bool,
    eyes: *mut [u64; 2],
    targets: *mut [u64; 4],
    samples: *mut [Difference; 128],
    count: *mut u32,
    totals: *mut Totals,
) -> bool {
    let image = if image.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(image, length) } };
    let exports = unsafe { Exports { eyes: &mut *eyes, targets: &mut *targets, samples: &mut *samples, count: &mut *count } };
    match compare(image, unsafe { &*surfaces }, width, depth_only, exports) {
        Some(found) => {
            unsafe { *totals = found };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each counter.
    use super::*;

    /// One-plane surfaces of `rows` rows of four 4-byte pixels, each copy 64 bytes apart.
    fn layouts(rows: u32) -> [[Layout; 4]; 2] {
        core::array::from_fn(|mode| {
            core::array::from_fn(|target| {
                let mut l = Layout { planes: 1, ..Default::default() };
                for sub in 0..2 {
                    l.footprints[sub].Offset = ((mode * 4 + target) * 2 + sub) as u64 * 64;
                    l.footprints[sub].Footprint.RowPitch = 16;
                    l.row_bytes[sub] = 16;
                    l.rows[sub] = rows;
                }
                l
            })
        })
    }
    fn run(image: &[u8], surfaces: &[[Layout; 4]; 2], depth_only: bool) -> (Option<Totals>, [u64; 2], [u64; 4], Vec<Difference>) {
        let (mut eyes, mut targets, mut samples, mut count) = ([0u64; 2], [0u64; 4], [Difference::default(); 128], 0u32);
        let totals = compare(image, surfaces, 4, depth_only, Exports { eyes: &mut eyes, targets: &mut targets, samples: &mut samples, count: &mut count });
        (totals, eyes, targets, samples[..count as usize].to_vec())
    }
    #[test]
    fn differences() {
        let mut image = vec![0u8; 16 * 64];
        // Shared target 1, eye 1 (sub 1), row 0, pixel 2: one byte differs.
        image[(4 + 1) * 2 * 64 + 64 + 8] = 7;
        // Shared target 0 eye 0: pixels 0 and 3 written (coverage), the first also differing.
        image[4 * 2 * 64] = 1;
        image[4 * 2 * 64 + 12] = 2;
        // Reference target 0 pixel 3 equals the shared one.
        image[12] = 2;
        let (totals, eyes, targets, samples) = run(&image, &layouts(1), false);
        let totals = totals.unwrap();
        assert_eq!((totals.different, totals.compared, totals.coverage), (2, 4 * 2 * 16, [2, 0]));
        assert_eq!((eyes, targets), ([1, 1], [1, 1, 0, 0]));
        assert_eq!(samples, vec![Difference { target: 0, eye: 0, plane: 0, x: 0, y: 0, reference: 0, shared: 1 }, Difference { target: 1, eye: 1, plane: 0, x: 2, y: 0, reference: 0, shared: 7 }]);
        // The depth-only mode compares and covers the depth target alone.
        let (totals, _, targets, samples) = run(&image, &layouts(1), true);
        assert_eq!((totals.unwrap().compared, targets, samples.len()), (2 * 16, [0; 4], 0));
    }
    #[test]
    fn samples_cap_and_bounds() {
        let mut image = vec![0u8; 16 * 64];
        for i in 0..64 {
            image[4 * 2 * 64 + i] = 1; // every byte of shared target 0 differs
        }
        let mut s = layouts(1);
        for target in 0..4 {
            for mode in 0..2 {
                s[mode][target].rows = [4, 4, 0, 0];
            }
        }
        let (mut eyes, mut targets, mut samples, mut count) = ([0u64; 2], [0u64; 4], [Difference::default(); 128], 126u32);
        compare(&image, &s, 4, false, Exports { eyes: &mut eyes, targets: &mut targets, samples: &mut samples, count: &mut count }).unwrap();
        assert_eq!(count, 128);
        // A copy ending one byte past the readback fails the comparison.
        let mut s = layouts(1);
        s[1][3].footprints[1].Offset = 16 * 64 - 15;
        assert!(run(&image, &s, false).0.is_none());
        s[1][3].footprints[1].Offset = 16 * 64 - 16;
        assert!(run(&image, &s, false).0.is_some());
    }
}
