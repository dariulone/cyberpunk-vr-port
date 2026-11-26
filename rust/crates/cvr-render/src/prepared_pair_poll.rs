//! The native prepared pair's poll results (src/Render/NativePreparedPair.cpp, `PollMembership` and `Poll`): each source's
//! actual membership row (its eight 32-bit results after the 16-byte predicates, and its predicate pair) is accepted when
//! the result is 1, exactly one of the two predicates is set -- the first exactly when no failure bit is set -- the
//! failure bits stay below 32 and the checked layers (at most 32) have five checks each, and the work's accepted and
//! fallback counts match the input stats; each mesh pair's result is equal or fallback (or neither, counted), never
//! both; and the query readback's verdict: the inputs accepted, the baseline's refusal and silence flags, every query's
//! pipeline statistics executed, the draw count and the mesh's pass flag.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_membership_rows`, `cvr_npp_mesh_results` and
//! `cvr_npp_query_verdict`; the unit tests below cover each row condition, each mesh result and each verdict flag.

pub const ROW: usize = 32;
pub const MAX_ROWS: usize = 512;

/// PollMembership's rows and verdict (`input` the pair's input stats); false also when the readback is too short.
pub fn membership_rows(bytes: &[u8], count: u32, rows: &mut [u64; ROW * MAX_ROWS], stats: &mut [u64; 24], input: &[u64; 8]) -> bool {
    let n = count as usize;
    if n > MAX_ROWS || bytes.len() < n * 48 {
        return false;
    }
    let mut valid = true;
    for i in 0..n {
        let row = &mut rows[i * ROW..(i + 1) * ROW];
        let results = &bytes[n * 16 + i * 32..n * 16 + i * 32 + 32];
        for (k, value) in results.chunks_exact(4).enumerate() {
            row[16 + k] = u64::from(u32::from_le_bytes(value.try_into().unwrap()));
        }
        for (k, value) in bytes[i * 16..i * 16 + 16].chunks_exact(8).enumerate() {
            row[24 + k] = u64::from_le_bytes(value.try_into().unwrap());
        }
        valid = valid
            && row[16] == 1
            && row[24] <= 1
            && row[25] <= 1
            && row[24].wrapping_add(row[25]) == 1
            && row[24] == u64::from(row[17] == 0)
            && row[17] & !31 == 0
            && row[20] <= 32
            && row[21] == row[20].wrapping_mul(5);
        for (stat, value) in [(10, row[16]), (11, row[24]), (12, row[25]), (13, row[20]), (14, row[21])] {
            stats[stat] = stats[stat].wrapping_add(value);
        }
    }
    stats[9] = 1;
    valid && input[5] == u64::from(stats[11] == u64::from(count)) && input[6] == u64::from(stats[12] != 0)
}
/// Poll's mesh results over `pairs` pairs: true when a pair's result is malformed (both, or above 1).
pub fn mesh_results(results: &[u64; 512], pairs: u32, mesh: &mut [u64; 12], batch: &mut [u64; 16], pair_stats: &mut [u64; 16]) -> bool {
    let mut malformed = false;
    for pair in results.chunks_exact(2).take(pairs as usize) {
        let (equal, fallback) = (pair[0], pair[1]);
        malformed |= equal > 1 || fallback > 1 || equal.wrapping_add(fallback) > 1;
        mesh[4] = mesh[4].wrapping_add(equal);
        mesh[5] = mesh[5].wrapping_add(fallback);
        mesh[6] = mesh[6].wrapping_add(u64::from(equal.wrapping_add(fallback) == 0));
    }
    batch[9] = mesh[5];
    pair_stats[3] = u64::from(mesh[4] > 0 && mesh[6] == 0);
    malformed
}
/// The verdict's flags.
pub struct Verdict<'a> {
    pub input: &'a [u64; 8],
    pub gpu: &'a [u64; 36],
    pub mesh: &'a [u64; 12],
    pub baseline: &'a mut [u64; 24],
    pub exit: &'a mut [u64; 24],
    pub rate: &'a mut [u64; 16],
    pub pair_stats: &'a mut [u64; 16],
}
/// Poll's query verdict: true when the pass failed (Error 17).
pub fn query_verdict(has_inputs: bool, native_exit: bool, sequence: bool, draws: u64, v: Verdict<'_>) -> bool {
    let accepted = !has_inputs || (v.input[5] == 1 && v.input[6] == 0);
    v.baseline[22] = u64::from(!accepted);
    if native_exit {
        v.exit[22] = u64::from(!accepted);
        v.rate[15] = u64::from(!accepted);
    }
    v.baseline[19] = u64::from(!accepted && v.gpu.iter().all(|&value| value == 0));
    let executed = accepted && (0..3).all(|query| v.gpu[query * 12 + 2] != 0 && v.gpu[query * 12 + 7] != 0 && v.gpu[query * 12 + 11] != 0);
    v.baseline[20] = u64::from(executed);
    v.pair_stats[5] = if accepted { draws.wrapping_add(v.mesh[5]) } else { 0 };
    if !accepted {
        v.pair_stats[4] = 0;
    }
    v.pair_stats[3] == 0 || (!sequence && v.mesh[5] != 0)
}

// ---- the C ABI

/// # Safety
/// `bytes` holds `length` bytes (or is null with 0); the arrays hold 32 * 512, 24 and 8 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_membership_rows(
    bytes: *const u8,
    length: usize,
    count: u32,
    rows: *mut [u64; ROW * MAX_ROWS],
    stats: *mut [u64; 24],
    input: *const [u64; 8],
) -> bool {
    let bytes = if bytes.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(bytes, length) } };
    membership_rows(bytes, count, unsafe { &mut *rows }, unsafe { &mut *stats }, unsafe { &*input })
}
/// # Safety
/// The arrays hold 512, 12, 16 and 16 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_mesh_results(results: *const [u64; 512], pairs: u32, mesh: *mut [u64; 12], batch: *mut [u64; 16], pair_stats: *mut [u64; 16]) -> bool {
    mesh_results(unsafe { &*results }, pairs, unsafe { &mut *mesh }, unsafe { &mut *batch }, unsafe { &mut *pair_stats })
}
/// # Safety
/// The arrays hold 8, 36, 12, 24, 24, 16 and 16 values.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp_query_verdict(
    has_inputs: bool,
    native_exit: bool,
    sequence: bool,
    draws: u64,
    input: *const [u64; 8],
    gpu: *const [u64; 36],
    mesh: *const [u64; 12],
    baseline: *mut [u64; 24],
    exit: *mut [u64; 24],
    rate: *mut [u64; 16],
    pair_stats: *mut [u64; 16],
) -> bool {
    let v = unsafe {
        Verdict { input: &*input, gpu: &*gpu, mesh: &*mesh, baseline: &mut *baseline, exit: &mut *exit, rate: &mut *rate, pair_stats: &mut *pair_stats }
    };
    query_verdict(has_inputs, native_exit, sequence, draws, v)
}

#[cfg(test)]
mod tests {
    //! Each row condition, each mesh result and each verdict flag.
    use super::*;

    fn readback(rows: &[([u32; 8], [u64; 2])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (_, predicates) in rows {
            for p in predicates {
                bytes.extend_from_slice(&p.to_le_bytes());
            }
        }
        for (results, _) in rows {
            for r in results {
                bytes.extend_from_slice(&r.to_le_bytes());
            }
        }
        bytes
    }
    const GOOD: ([u32; 8], [u64; 2]) = ([1, 0, 9, 9, 3, 15, 9, 9], [1, 0]);
    const FAILED: ([u32; 8], [u64; 2]) = ([1, 4, 9, 9, 2, 10, 9, 9], [0, 1]);
    fn run(rows: &[([u32; 8], [u64; 2])], input5: u64, input6: u64) -> (bool, Box<[u64; ROW * MAX_ROWS]>, [u64; 24]) {
        let bytes = readback(rows);
        let mut out = Box::new([0u64; ROW * MAX_ROWS]);
        let mut stats = [0u64; 24];
        let mut input = [0u64; 8];
        input[5] = input5;
        input[6] = input6;
        let valid = membership_rows(&bytes, rows.len() as u32, &mut out, &mut stats, &input);
        (valid, out, stats)
    }
    #[test]
    fn memberships() {
        let (valid, rows, stats) = run(&[GOOD, FAILED], 0, 1);
        assert!(valid);
        assert_eq!(rows[16..26], [1, 0, 9, 9, 3, 15, 9, 9, 1, 0]);
        assert_eq!(rows[32 + 16..32 + 26], [1, 4, 9, 9, 2, 10, 9, 9, 0, 1]);
        assert_eq!(stats[9..15], [1, 2, 1, 1, 5, 25]);
        assert!(run(&[GOOD, GOOD], 1, 0).0);
        assert!(!run(&[GOOD, GOOD], 0, 0).0);
        assert!(!run(&[GOOD, FAILED], 1, 1).0);
        assert!(!run(&[GOOD, FAILED], 0, 0).0);
        let bad = |change: fn(&mut ([u32; 8], [u64; 2]))| {
            let mut row = GOOD;
            change(&mut row);
            run(&[row], 1, u64::from(row.1[1] != 0)).0
        };
        assert!(!bad(|r| r.0[0] = 2));
        assert!(!bad(|r| r.1 = [1, 1]));
        assert!(!bad(|r| r.1 = [0, 0]));
        assert!(!bad(|r| r.1 = [2, 0]));
        assert!(!bad(|r| r.0[1] = 1));
        assert!(!bad(|r| r.0[1] = 32));
        assert!(!bad(|r| r.0[4] = 4));
        assert!(!bad(|r| {
            r.0[4] = 33;
            r.0[5] = 165;
        }));
        assert!(bad(|r| {
            r.0[4] = 32;
            r.0[5] = 160;
        }));
        let mut failed = FAILED;
        failed.0[1] = 31;
        assert!(run(&[failed], 0, 1).0);
        let mut failed = FAILED;
        failed.0[1] = 0;
        assert!(!run(&[failed], 0, 1).0);
        let mut out = Box::new([0u64; ROW * MAX_ROWS]);
        assert!(!membership_rows(&readback(&[GOOD])[..47], 1, &mut out, &mut [0; 24], &[0; 8]));
    }
    #[test]
    fn meshes() {
        let mut results = [0u64; 512];
        results[..8].copy_from_slice(&[1, 0, 0, 1, 0, 0, 1, 0]);
        results[8] = 1;
        let (mut mesh, mut batch, mut stats) = ([0u64; 12], [0u64; 16], [0u64; 16]);
        assert!(!mesh_results(&results, 4, &mut mesh, &mut batch, &mut stats));
        assert_eq!((mesh[4], mesh[5], mesh[6], batch[9], stats[3]), (2, 1, 1, 1, 0));
        let (mut mesh, mut batch, mut stats) = ([0u64; 12], [0u64; 16], [0u64; 16]);
        assert!(!mesh_results(&results, 2, &mut mesh, &mut batch, &mut stats));
        assert_eq!((mesh[4], mesh[5], mesh[6], stats[3]), (1, 1, 0, 1));
        for bad in [[1u64, 1], [2, 0], [0, 2]] {
            let mut r = [0u64; 512];
            r[..2].copy_from_slice(&bad);
            assert!(mesh_results(&r, 1, &mut [0; 12], &mut [0; 16], &mut [0; 16]));
        }
        let (mut mesh, mut stats) = ([0u64; 12], [0u64; 16]);
        mesh_results(&[0; 512], 0, &mut mesh, &mut [0; 16], &mut stats);
        assert_eq!(stats[3], 0);
    }
    #[test]
    fn verdicts() {
        let run = |has_inputs: bool, native: bool, sequence: bool, input: [u64; 8], gpu: [u64; 36], mesh: [u64; 12], pass: u64| {
            let (mut baseline, mut exit, mut rate, mut stats) = ([0u64; 24], [0u64; 24], [0u64; 16], [0u64; 16]);
            stats[3] = pass;
            stats[4] = 7;
            let failed = query_verdict(
                has_inputs,
                native,
                sequence,
                10,
                Verdict { input: &input, gpu: &gpu, mesh: &mesh, baseline: &mut baseline, exit: &mut exit, rate: &mut rate, pair_stats: &mut stats },
            );
            (failed, baseline[22], baseline[19], baseline[20], exit[22], rate[15], stats[5], stats[4])
        };
        let mut gpu = [0u64; 36];
        for q in 0..3 {
            gpu[q * 12 + 2] = 1;
            gpu[q * 12 + 7] = 1;
            gpu[q * 12 + 11] = 1;
        }
        let mut input = [0u64; 8];
        input[5] = 1;
        let mut mesh = [0u64; 12];
        mesh[5] = 2;
        assert_eq!(run(true, true, true, input, gpu, mesh, 1), (false, 0, 0, 1, 0, 0, 12, 7));
        assert_eq!(run(false, false, true, [0; 8], gpu, mesh, 1), (false, 0, 0, 1, 0, 0, 12, 7));
        assert_eq!(run(true, true, true, [0; 8], gpu, mesh, 1), (false, 1, 0, 0, 1, 1, 0, 0));
        assert_eq!(run(true, false, true, [0; 8], gpu, mesh, 1), (false, 1, 0, 0, 0, 0, 0, 0));
        assert_eq!(run(true, true, true, [0; 8], [0; 36], mesh, 1), (false, 1, 1, 0, 1, 1, 0, 0));
        let mut i = input;
        i[6] = 1;
        assert_eq!(run(true, true, true, i, gpu, mesh, 1).1, 1);
        for at in [2, 7, 11, 14, 19, 23, 26, 31, 35] {
            let mut g = gpu;
            g[at] = 0;
            assert_eq!(run(true, true, true, input, g, mesh, 1).3, 0, "{at}");
        }
        assert!(run(true, true, true, input, gpu, mesh, 0).0);
        assert!(run(true, true, false, input, gpu, mesh, 1).0);
        assert!(!run(true, true, false, input, gpu, [0; 12], 1).0);
    }
}
