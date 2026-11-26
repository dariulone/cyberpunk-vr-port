//! The retained geometry's selector verdicts (src/Render/NativeRetainedGeometry.cpp, `Poll`): a selector input row (24
//! words) meets the first CBV snapshot row (32) of its source, side, visibility, register, parameter and offset; when
//! that snapshot is the input's own private copy (status 1, the input's resource, stamp, heap, GPU address, recording
//! generation and context, the reflected bytes, a payload in the blob) the input records the snapshot (14, plus one) and
//! whether its owned bytes equal the copy's (13: 1 equal, 2 different); a selector command (32) whose slots were
//! captured passes when its inputs all equal their copies and nothing was truncated (11), and a multilayer command also
//! needs its t5 view row's ownership and its t5 snapshot (none needed without a byte extent, else equal bytes, 27);
//! and a t5 view row passes with its source's first input equal and a window inside the resource (28).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_match_input`, `cvr_nrg_selector_command` and
//! `cvr_nrg_t5_view`; the unit tests below cover each key, each copy field and each verdict.

/// Matches one input with intent bytes `owned` against the snapshot rows and the blob.
pub fn match_input(row: &mut [u64; 24], snapshots: &[[u64; 32]], blob: &[u8], owned: &[u8]) {
    let Some((n, snapshot)) = snapshots.iter().enumerate().find(|(_, s)| s[..6] == row[..6]) else { return };
    let blob_len = blob.len() as u64;
    let copy = snapshot[8] == 1
        && snapshot[12] == row[19]
        && snapshot[13] == row[20]
        && snapshot[23] == row[17]
        && snapshot[24] == row[18]
        && snapshot[25] == row[21]
        && snapshot[26] == row[22]
        && snapshot[20] == row[6]
        && snapshot[29] != 0
        && snapshot[30] != 0
        && snapshot[31] != 0
        && snapshot[31] - 1 <= blob_len
        && snapshot[20] <= blob_len - (snapshot[31] - 1);
    if copy {
        row[14] = n as u64 + 1;
        let (start, bytes) = ((snapshot[31] - 1) as usize, snapshot[20] as usize);
        // The owned bytes are the reflected bytes; a shorter intent cannot equal the copy.
        row[13] = if owned.len() >= bytes && owned[..bytes] == blob[start..start + bytes] { 1 } else { 2 };
    }
}
/// One captured selector command's verdict; `inputs` its source's input rows (indices into `selector_inputs`), `t5_row`
/// the source's t5 snapshot row (u64::MAX when none) and `byte_extent` its plan's.
pub fn selector_command(
    command: &mut [u64; 32],
    inputs: &[u64],
    selector_inputs: &[[u64; 24]],
    t5_rows: &[[u64; 32]],
    t5_snapshots: &[[u64; 32]],
    t5_row: u64,
    byte_extent: u64,
) {
    let equal = |index: &u64| selector_inputs.get(*index as usize).is_some_and(|row| row[13] == 1);
    command[11] = u64::from(!inputs.is_empty() && command[23] == 0 && inputs.iter().all(equal));
    if command[25] != 0 {
        let view = t5_rows.iter().find(|row| row[0] == command[0]);
        let snapshot = usize::try_from(t5_row).ok().and_then(|at| t5_snapshots.get(at));
        command[27] = u64::from(match (view, snapshot) {
            (Some(view), Some(snapshot)) => view[29] != 0 && ((byte_extent == 0 && snapshot[2] == 2) || snapshot[28] == 1),
            _ => false,
        });
        command[11] &= command[27];
    }
}
/// A t5 view row's verdict; `first_input` the source's first input row (u64::MAX when none).
pub fn t5_view(row: &mut [u64; 32], first_input: u64, selector_inputs: &[[u64; 24]]) {
    let equal = usize::try_from(first_input).ok().and_then(|at| selector_inputs.get(at)).is_some_and(|input| input[13] == 1);
    row[28] = u64::from(equal && row[19] != 0);
}

// ---- the C ABI

unsafe fn rows<'a, const N: usize>(data: *const [u64; N], count: usize) -> &'a [[u64; N]] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// # Safety
/// `row` is a selector input row; the snapshots, blob and owned bytes hold their counts (null when empty).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_match_input(
    row: *mut [u64; 24],
    snapshots: *const [u64; 32],
    snapshot_count: usize,
    blob: *const u8,
    blob_bytes: usize,
    owned: *const u8,
    owned_bytes: usize,
) {
    let bytes = |data: *const u8, n: usize| if data.is_null() || n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(data, n) } };
    match_input(unsafe { &mut *row }, unsafe { rows(snapshots, snapshot_count) }, bytes(blob, blob_bytes), bytes(owned, owned_bytes));
}
/// # Safety
/// `command` is a selector command; the arrays hold their counts (null when empty).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nrg_selector_command(
    command: *mut [u64; 32],
    inputs: *const u64,
    input_count: usize,
    selector_inputs: *const [u64; 24],
    selector_input_count: usize,
    t5_rows: *const [u64; 32],
    t5_row_count: usize,
    t5_snapshots: *const [u64; 32],
    t5_snapshot_count: usize,
    t5_row: u64,
    byte_extent: u64,
) {
    let inputs = if inputs.is_null() || input_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(inputs, input_count) } };
    unsafe {
        selector_command(
            &mut *command,
            inputs,
            rows(selector_inputs, selector_input_count),
            rows(t5_rows, t5_row_count),
            rows(t5_snapshots, t5_snapshot_count),
            t5_row,
            byte_extent,
        )
    };
}
/// # Safety
/// `row` is a t5 view row; `selector_inputs` holds its count (null when empty).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_t5_view(row: *mut [u64; 32], first_input: u64, selector_inputs: *const [u64; 24], selector_input_count: usize) {
    t5_view(unsafe { &mut *row }, first_input, unsafe { rows(selector_inputs, selector_input_count) });
}

#[cfg(test)]
mod tests {
    //! Each key, each copy field and each verdict.
    use super::*;

    fn input() -> [u64; 24] {
        let mut r = [0u64; 24];
        r[..6].copy_from_slice(&[3, 1, 1, 4, 19, 0]);
        r[6] = 4;
        r[17] = 100;
        r[18] = 101;
        r[19] = 102;
        r[20] = 103;
        r[21] = 104;
        r[22] = 105;
        r
    }
    fn snapshot() -> [u64; 32] {
        let mut s = [0u64; 32];
        s[..6].copy_from_slice(&[3, 1, 1, 4, 19, 0]);
        s[8] = 1;
        s[12] = 102;
        s[13] = 103;
        s[23] = 100;
        s[24] = 101;
        s[25] = 104;
        s[26] = 105;
        s[20] = 4;
        s[29] = 1;
        s[30] = 1;
        s[31] = 3;
        s
    }
    const BLOB: [u8; 6] = [9, 9, 1, 2, 3, 4];
    #[test]
    fn matches() {
        let mut other = snapshot();
        other[0] = 4;
        let mut row = input();
        match_input(&mut row, &[other, snapshot()], &BLOB, &[1, 2, 3, 4]);
        assert_eq!((row[13], row[14]), (1, 2));
        let mut row = input();
        match_input(&mut row, &[snapshot()], &BLOB, &[1, 2, 3, 5]);
        assert_eq!((row[13], row[14]), (2, 1));
        let mut row = input();
        match_input(&mut row, &[snapshot()], &BLOB, &[1, 2, 3]);
        assert_eq!(row[13], 2);
        for key in 0..6 {
            let mut s = snapshot();
            s[key] = 77;
            let mut row = input();
            match_input(&mut row, &[s], &BLOB, &[1, 2, 3, 4]);
            assert_eq!((row[13], row[14]), (0, 0), "{key}");
        }
        for (at, value) in [(8, 2), (12, 0), (13, 0), (23, 0), (24, 0), (25, 0), (26, 0), (20, 3), (29, 0), (30, 0), (31, 0), (31, 4)] {
            let mut s = snapshot();
            s[at] = value;
            let mut row = input();
            // The first key match decides: a later exact copy is not consulted.
            match_input(&mut row, &[s, snapshot()], &BLOB, &[1, 2, 3, 4]);
            assert_eq!((row[13], row[14]), (0, 0), "{at}");
        }
        let mut s = snapshot();
        s[31] = 8;
        s[20] = 0;
        let mut row = input();
        row[6] = 0;
        match_input(&mut row, &[s], &BLOB, &[]);
        assert_eq!((row[13], row[14]), (0, 0));
        s[31] = 7;
        match_input(&mut row, &[s], &BLOB, &[]);
        assert_eq!((row[13], row[14]), (1, 1));
    }
    #[test]
    fn commands() {
        let mut equal = [0u64; 24];
        equal[13] = 1;
        let mut differ = [0u64; 24];
        differ[13] = 2;
        let inputs = [equal, differ, equal];
        let run = |input_rows: &[u64], truncated: u64| {
            let mut c = [0u64; 32];
            c[23] = truncated;
            selector_command(&mut c, input_rows, &inputs, &[], &[], u64::MAX, 0);
            c[11]
        };
        assert_eq!(run(&[0, 2], 0), 1);
        assert_eq!(run(&[0, 1], 0), 0);
        assert_eq!(run(&[], 0), 0);
        assert_eq!(run(&[0], 1), 0);
        assert_eq!(run(&[9], 0), 0);
        let mut view = [0u64; 32];
        view[0] = 5;
        view[29] = 1;
        let mut skipped = [0u64; 32];
        skipped[0] = 6;
        skipped[29] = 1;
        let mut same = [0u64; 32];
        same[28] = 1;
        let mut absent = [0u64; 32];
        absent[2] = 2;
        let multilayer = |views: &[[u64; 32]], snapshots: &[[u64; 32]], row: u64, extent: u64| {
            let mut c = [0u64; 32];
            c[0] = 5;
            c[25] = 1;
            selector_command(&mut c, &[0], &inputs, views, snapshots, row, extent);
            (c[27], c[11])
        };
        assert_eq!(multilayer(&[skipped, view], &[absent, same], 1, 64), (1, 1));
        assert_eq!(multilayer(&[view], &[absent], 0, 0), (1, 1));
        assert_eq!(multilayer(&[view], &[absent], 0, 64), (0, 0));
        assert_eq!(multilayer(&[view], &[same], 1, 64), (0, 0));
        assert_eq!(multilayer(&[view], &[same], u64::MAX, 64), (0, 0));
        assert_eq!(multilayer(&[skipped], &[same], 0, 64), (0, 0));
        let mut unowned = view;
        unowned[29] = 0;
        assert_eq!(multilayer(&[unowned], &[same], 0, 64), (0, 0));
    }
    #[test]
    fn views() {
        let mut equal = [0u64; 24];
        equal[13] = 1;
        let inputs = [[0u64; 24], equal];
        let run = |first: u64, inside: u64| {
            let mut row = [0u64; 32];
            row[19] = inside;
            t5_view(&mut row, first, &inputs);
            row[28]
        };
        assert_eq!(run(1, 1), 1);
        assert_eq!(run(0, 1), 0);
        assert_eq!(run(1, 0), 0);
        assert_eq!(run(u64::MAX, 1), 0);
    }
}
