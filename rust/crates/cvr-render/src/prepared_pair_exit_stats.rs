//! The native prepared pair's exit statistics (src/Render/NativePreparedPair.cpp, `CompareNativeExit`): each compared
//! plane's sample row (the eye, slot and plane, both comparisons' counts, first differing pixel -- all ones when none --
//! and values, and the plane's stride, component and bytes) at its eye's plane index; the native rate comparison's
//! accumulation (the plane count, the compared bytes, the plane kind's per-eye differences -- colors 5, depth 7,
//! stencil 9 -- and the per-eye totals 11 and 12); and the exit's verdict flags: complete (all ten planes compared),
//! exact (complete without the reference and optimized difference counts 16..19), the rate exit complete and exact
//! (without the pair differences 15 or the per-eye totals), and the color attribution complete (six planes, the
//! inside and outside counts summing to each eye's differences).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_exit_sample`, `cvr_npp_rate_sample` and
//! `cvr_npp_exit_verdict`; the unit tests below cover each row field, each plane kind and each flag's condition.

use crate::native_exit::{plane_index, Comparison, Difference, NativeExitPlane};

/// The sample row of one compared plane.
pub fn sample(eye: u32, native: &NativeExitPlane, comparison: &Comparison) -> [u64; 16] {
    let pixel = |d: &Difference| if d.count != 0 { (u64::from(d.x), u64::from(d.y)) } else { (u64::MAX, u64::MAX) };
    let (a, b) = (&comparison.reference, &comparison.optimized);
    let ((ax, ay), (bx, by)) = (pixel(a), pixel(b));
    [
        u64::from(eye), u64::from(native.slot), u64::from(native.plane), a.count, b.count, ax, ay, a.value, a.native_value, bx, by,
        b.value, b.native_value, u64::from(native.stride), u64::from(native.component), u64::from(native.bytes),
    ]
}
/// Writes the sample row at its eye's plane index (a row outside the 2 x 5 rows is not written).
pub fn write_sample(samples: &mut [u64; 160], eye: u32, native: &NativeExitPlane, comparison: &Comparison) {
    let index = plane_index(native.slot, native.plane);
    if eye < 2 && index < 5 {
        let at = ((eye * 5 + index) * 16) as usize;
        samples[at..at + 16].copy_from_slice(&sample(eye, native, comparison));
    }
}
/// The native rate comparison's accumulation.
pub fn rate_sample(rate: &mut [u64; 16], eye: u32, native: &NativeExitPlane, width: u32, count: u64) {
    let delta = if native.slot < 3 { 5 } else if native.plane != 0 { 9 } else { 7 };
    rate[3] = rate[3].wrapping_add(1);
    rate[4] = rate[4].wrapping_add(u64::from(width).wrapping_mul(u64::from(native.rows)).wrapping_mul(u64::from(native.bytes)));
    let eye = eye as usize;
    rate[delta + eye] = rate[delta + eye].wrapping_add(count);
    rate[11 + eye] = rate[11 + eye].wrapping_add(count);
}
/// The verdict flags.
pub fn verdict(complete: bool, rate_complete: bool, attributed: bool, attributed_planes: u32, exit: &mut [u64; 24], rate: &mut [u64; 16], attribution: &mut [u64; 12]) {
    exit[20] = u64::from(complete && exit[2] == 10);
    exit[21] = u64::from(exit[20] != 0 && exit[16..20].iter().all(|&value| value == 0));
    rate[13] = u64::from(rate_complete && rate[3] == 10);
    rate[14] = u64::from(rate[13] != 0 && rate[15] == 0 && rate[11] == 0 && rate[12] == 0);
    attribution[11] = u64::from(
        attributed
            && attributed_planes == 6
            && exit[20] != 0
            && attribution[5].wrapping_add(attribution[7]) == exit[4]
            && attribution[6].wrapping_add(attribution[8]) == exit[5],
    );
}

// ---- the C ABI

/// # Safety
/// `samples` holds 160 values; `native` and `comparison` are the mirrored records.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_exit_sample(samples: *mut [u64; 160], eye: u32, native: *const NativeExitPlane, comparison: *const Comparison) {
    write_sample(unsafe { &mut *samples }, eye, unsafe { &*native }, unsafe { &*comparison });
}
/// # Safety
/// `rate` holds 16 values; `native` is the mirrored plane; `eye` is 0 or 1.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_rate_sample(rate: *mut [u64; 16], eye: u32, native: *const NativeExitPlane, width: u32, count: u64) {
    rate_sample(unsafe { &mut *rate }, eye, unsafe { &*native }, width, count);
}
/// # Safety
/// The arrays hold 24, 16 and 12 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_exit_verdict(
    complete: bool,
    rate_complete: bool,
    attributed: bool,
    attributed_planes: u32,
    exit: *mut [u64; 24],
    rate: *mut [u64; 16],
    attribution: *mut [u64; 12],
) {
    verdict(complete, rate_complete, attributed, attributed_planes, unsafe { &mut *exit }, unsafe { &mut *rate }, unsafe { &mut *attribution });
}

#[cfg(test)]
mod tests {
    //! Each row field, each plane kind and each flag's condition.
    use super::*;

    fn plane(slot: u32, plane: u32) -> NativeExitPlane {
        NativeExitPlane { slot, plane, rows: 3, stride: 8, component: 4, bytes: 1, ..Default::default() }
    }
    #[test]
    fn samples() {
        let c = Comparison {
            reference: Difference { count: 2, value: 11, native_value: 12, x: 5, y: 6 },
            optimized: Difference { count: 0, value: 13, native_value: 14, x: 7, y: 8 },
        };
        assert_eq!(sample(1, &plane(4, 1), &c), [1, 4, 1, 2, 0, 5, 6, 11, 12, u64::MAX, u64::MAX, 13, 14, 8, 4, 1]);
        let mut rows = [0u64; 160];
        write_sample(&mut rows, 1, &plane(4, 1), &c);
        assert_eq!(rows[(5 + 4) * 16..(5 + 4) * 16 + 3], [1, 4, 1]);
        assert_eq!(rows.iter().filter(|&&v| v != 0).count(), 15);
        let mut rows = [0u64; 160];
        write_sample(&mut rows, 0, &plane(2, 0), &c);
        assert_eq!(rows[2 * 16 + 1], 2);
        write_sample(&mut rows, 0, &plane(3, 0), &c);
        assert_eq!(rows.iter().filter(|&&v| v != 0).count(), 13);
    }
    #[test]
    fn rates() {
        let mut rate = [0u64; 16];
        rate_sample(&mut rate, 0, &plane(1, 0), 10, 4);
        rate_sample(&mut rate, 1, &plane(4, 0), 10, 6);
        rate_sample(&mut rate, 1, &plane(4, 1), 10, 7);
        assert_eq!(rate[3], 3);
        assert_eq!(rate[4], 90);
        assert_eq!((rate[5], rate[8], rate[10]), (4, 6, 7));
        assert_eq!((rate[6], rate[7], rate[9]), (0, 0, 0));
        assert_eq!((rate[11], rate[12]), (4, 13));
    }
    #[test]
    fn verdicts() {
        let run = |complete, rate_complete, attributed, planes, exit: [u64; 24], rate: [u64; 16], attribution: [u64; 12]| {
            let (mut exit, mut rate, mut attribution) = (exit, rate, attribution);
            verdict(complete, rate_complete, attributed, planes, &mut exit, &mut rate, &mut attribution);
            (exit[20], exit[21], rate[13], rate[14], attribution[11])
        };
        let mut exit = [0u64; 24];
        exit[2] = 10;
        exit[4] = 9;
        exit[5] = 3;
        let mut rate = [0u64; 16];
        rate[3] = 10;
        let mut attribution = [0u64; 12];
        attribution[5] = 4;
        attribution[7] = 5;
        attribution[6] = 3;
        assert_eq!(run(true, true, true, 6, exit, rate, attribution), (1, 1, 1, 1, 1));
        assert_eq!(run(false, true, true, 6, exit, rate, attribution), (0, 0, 1, 1, 0));
        assert_eq!(run(true, false, true, 6, exit, rate, attribution), (1, 1, 0, 0, 1));
        assert_eq!(run(true, true, false, 6, exit, rate, attribution), (1, 1, 1, 1, 0));
        assert_eq!(run(true, true, true, 5, exit, rate, attribution), (1, 1, 1, 1, 0));
        let mut e = exit;
        e[2] = 9;
        assert_eq!(run(true, true, true, 6, e, rate, attribution), (0, 0, 1, 1, 0));
        for i in 16..20 {
            let mut e = exit;
            e[i] = 1;
            assert_eq!(run(true, true, true, 6, e, rate, attribution), (1, 0, 1, 1, 1));
        }
        let mut e = exit;
        e[20] = 1;
        e[21] = 1;
        e[15] = 1;
        assert_eq!(run(true, true, true, 6, e, rate, attribution), (1, 1, 1, 1, 1));
        for i in [11, 12, 15] {
            let mut r = rate;
            r[i] = 1;
            assert_eq!(run(true, true, true, 6, exit, r, attribution), (1, 1, 1, 0, 1));
        }
        let mut r = rate;
        r[3] = 11;
        assert_eq!(run(true, true, true, 6, exit, r, attribution), (1, 1, 0, 0, 1));
        let mut a = attribution;
        a[7] = 6;
        assert_eq!(run(true, true, true, 6, exit, rate, a), (1, 1, 1, 1, 0));
        let mut a = attribution;
        a[8] = 1;
        assert_eq!(run(true, true, true, 6, exit, rate, a), (1, 1, 1, 1, 0));
    }
}
