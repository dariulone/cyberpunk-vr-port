//! The stereo GPU probe's timings (src/Render/StereoGpuProbe.cpp, `Poll`): each timed scope's two resolved timestamps
//! (at the readback's timestamp offset plus its index's 8 bytes) add their ticks to its stage's and eye's total and
//! count; a pair running backwards is dropped instead.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the loop forwards to `cvr_sgpt_timings` (bounds-checked against the readback; the
//! timed scope mirrored; the drops returned for the .cpp's atomic counter); the unit tests below cover each case.

/// The .cpp's Work::Timed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Timed {
    pub stage: u32,
    pub eye: u32,
    pub index: u32,
}
const _: () = assert!(core::mem::size_of::<Timed>() == 12);

/// The timings: adds to `ticks` and `counts` and returns the dropped pairs.
pub fn timings(image: &[u8], timed: &[Timed], offset: u64, ticks: &mut [[u64; 2]; 7], counts: &mut [[u32; 2]; 7]) -> u32 {
    let mut drops = 0;
    for t in timed {
        let Some(at) = offset.checked_add(u64::from(t.index) * 8) else { continue };
        if at.checked_add(16).is_none_or(|end| end > image.len() as u64) || t.stage >= 7 || t.eye >= 2 {
            continue;
        }
        let at = at as usize;
        let begin = u64::from_le_bytes(image[at..at + 8].try_into().unwrap());
        let end = u64::from_le_bytes(image[at + 8..at + 16].try_into().unwrap());
        if end < begin {
            drops += 1;
            continue;
        }
        let (s, e) = (t.stage as usize, t.eye as usize);
        ticks[s][e] = ticks[s][e].wrapping_add(end - begin);
        counts[s][e] = counts[s][e].wrapping_add(1);
    }
    drops
}

// ---- the C ABI

/// # Safety
/// `image` holds `length` bytes, `timed` `count` scopes; `ticks` and `counts` hold 7 x 2 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpt_timings(
    image: *const u8,
    length: usize,
    timed: *const Timed,
    count: usize,
    offset: u64,
    ticks: *mut [[u64; 2]; 7],
    counts: *mut [[u32; 2]; 7],
) -> u32 {
    let image = if image.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(image, length) } };
    let timed = if timed.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(timed, count) } };
    timings(image, timed, offset, unsafe { &mut *ticks }, unsafe { &mut *counts })
}

#[cfg(test)]
mod tests {
    //! Each case.
    use super::*;

    #[test]
    fn ticks() {
        let mut image = vec![0u8; 64];
        let put = |image: &mut Vec<u8>, at: usize, v: u64| image[at..at + 8].copy_from_slice(&v.to_le_bytes());
        // Offset 16: scope index 0 at 16 (100 -> 150), index 2 at 32 (200 -> 190, backwards), index 4 at 48 (5 -> 5).
        put(&mut image, 16, 100);
        put(&mut image, 24, 150);
        put(&mut image, 32, 200);
        put(&mut image, 40, 190);
        put(&mut image, 48, 5);
        put(&mut image, 56, 5);
        let timed = [Timed { stage: 3, eye: 1, index: 0 }, Timed { stage: 3, eye: 1, index: 2 }, Timed { stage: 6, eye: 0, index: 4 }, Timed { stage: 0, eye: 0, index: 5 }];
        let (mut ticks, mut counts) = ([[0u64; 2]; 7], [[0u32; 2]; 7]);
        assert_eq!(timings(&image, &timed, 16, &mut ticks, &mut counts), 1);
        assert_eq!((ticks[3][1], counts[3][1], ticks[6][0], counts[6][0], counts[0][0]), (50, 1, 0, 1, 0));
        // A stage or eye out of range is skipped.
        let (mut ticks, mut counts) = ([[0u64; 2]; 7], [[0u32; 2]; 7]);
        assert_eq!(timings(&image, &[Timed { stage: 7, eye: 0, index: 0 }, Timed { stage: 0, eye: 2, index: 0 }], 16, &mut ticks, &mut counts), 0);
        assert_eq!(counts, [[0; 2]; 7]);
    }
}
